#![warn(clippy::str_to_string, clippy::pedantic, clippy::nursery)]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

mod allocator;
mod caddy;
mod cli;
mod config;
mod dependencies;
mod error;
#[cfg(test)]
mod mocks;
mod registry;

use crate::allocator::PortAllocator;
use crate::caddy::{generate_caddyfile, reload};
use crate::cli::{Cli, Config as ConfigSubcommand, InitShell};
use crate::config::Config;
use crate::error::Result;
use crate::registry::Registry;
use anyhow::Context;
use clap::Parser;
use cli::Repo;
use dependencies::{
    Args, CheckPath, ChoosePort, DataDir, Environment, Exec, ReadFile, Tty, WorkingDirectory,
    WriteFile,
};
use entrait::Impl;
use error::{ApplicationError, CaddyError, ExecError};
use registry::Project;
use std::fmt::Write as FmtWrite;
use std::io::{ErrorKind, Write as IoWrite};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

// Find and return a reference to the active project based on the current directory
fn get_active_project<'registry>(
    deps: &impl WorkingDirectory,
    registry: &'registry Registry,
) -> Result<(&'registry String, &'registry Project)> {
    registry
        .match_cwd(deps)?
        .ok_or(ApplicationError::NoActiveProject)
}

// Find and return a reference to the active project based on the current directory
fn get_active_repo(deps: &impl Exec) -> Result<String> {
    deps.exec(Command::new("git").args(["remote", "get-url", "origin"]))
        .map(|repo| repo.trim_end().to_owned())
        .map_err(ApplicationError::GitCommand)
}

fn format_project(name: &str, project: &Project) -> String {
    let directory = project
        .directory
        .as_ref()
        .map(|directory| format!(" ({})", directory.display()))
        .unwrap_or_default();
    let linked_port = project
        .linked_port
        .map(|port| format!(" -> :{port}"))
        .unwrap_or_default();
    format!("{name} :{}{linked_port}{directory}", project.port)
}

fn format_repo(repo: &str, port: u16) -> String {
    format!("{repo}: {port}")
}

fn create(
    deps: &(impl ChoosePort + WorkingDirectory),
    registry: &mut Registry,
    name: Option<String>,
    no_activate: bool,
    linked_port: Option<u16>,
    overwrite: bool,
) -> Result<(String, Project, bool)> {
    let name = if let Some(name) = name {
        name
    } else {
        let directory = deps.get_cwd()?;
        let basename = directory
            .file_name()
            .context("Failed to extract directory basename")?;
        let name = basename
            .to_str()
            .context("Failed to convert directory to string")?;
        Registry::normalize_name(name)
    };
    let directory = if no_activate {
        None
    } else {
        Some(deps.get_cwd()?)
    };

    if overwrite && registry.get(&name).is_some() {
        if let Some(port) = linked_port {
            registry.link(deps, &name, port)?;
        }
        let project = registry.update(&name, directory)?;
        return Ok((name, project, true));
    }

    let project = registry.create(deps, &name, directory, linked_port)?;
    Ok((name, project, false))
}

fn cleanup(
    deps: &(impl CheckPath + DataDir + Environment + Exec + ReadFile + WriteFile),
    registry: &mut Registry,
) -> Result<Vec<(String, Project)>> {
    // Find all existing projects with a directory that doesn't exist
    let removed_projects = registry
        .iter_projects()
        .filter_map(|(name, project)| {
            project.directory.as_ref().and_then(|directory| {
                if deps.path_exists(directory) {
                    None
                } else {
                    Some(name.clone())
                }
            })
        })
        .collect();
    registry.delete_many(removed_projects)
}

// Return the path to the config file and a flag indicating whether the location was customized with
// the PORTMAN_CONFIG environment variable
fn get_config_path(deps: &(impl DataDir + Environment)) -> Result<(PathBuf, bool)> {
    let config_env = deps.read_var("PORTMAN_CONFIG").ok();
    match config_env {
        Some(config_path) => Ok((PathBuf::from(config_path), true)),
        None => Ok((deps.get_data_dir()?.join("config.toml"), false)),
    }
}

fn load_config(deps: &(impl ChoosePort + DataDir + Environment + ReadFile)) -> Result<Config> {
    let (config_path, custom_path) = get_config_path(deps)?;
    match Config::load(deps, &config_path)? {
        Some(config) => Ok(config),
        None if custom_path => Err(ApplicationError::MissingCustomConfig(config_path)),
        None => Ok(Config::default()),
    }
}

fn load_registry(deps: &(impl ChoosePort + DataDir + Environment + ReadFile)) -> Result<Registry> {
    let config = load_config(deps)?;
    let port_allocator = PortAllocator::new(config.get_valid_ports());
    Registry::new(deps, port_allocator)
}

#[allow(clippy::too_many_lines)]
fn run(
    deps: &(
         impl Args
         + CheckPath
         + ChoosePort
         + DataDir
         + Environment
         + Exec
         + ReadFile
         + Tty
         + WriteFile
         + WorkingDirectory
     ),
    cli: Cli,
) -> Result<String> {
    let mut output = String::new();
    match cli {
        Cli::Init { shell } => {
            output += match shell {
                InitShell::Bash => include_str!("./shells/init.bash"),
                InitShell::Fish => include_str!("./shells/init.fish"),
                InitShell::Zsh => include_str!("./shells/init.zsh"),
            }
        }

        Cli::Config(subcommand) => match subcommand {
            ConfigSubcommand::Show => {
                let config_path = get_config_path(deps)?.0;
                let config = load_config(deps)?;
                let registry_path = deps.get_data_dir()?.join(PathBuf::from("registry.toml"));
                writeln!(
                    output,
                    "Config path: {}\nRegistry path: {}\nConfiguration:\n--------------\n{config}",
                    config_path.display(),
                    registry_path.display()
                )
                .unwrap();
            }
            ConfigSubcommand::Edit => {
                let config_path = get_config_path(deps)?.0;
                let editor = deps.read_var("EDITOR")?;
                writeln!(
                    output,
                    "Opening \"{}\" with \"{editor}\"",
                    config_path.display()
                )
                .unwrap();
                deps.exec(Command::new(editor).arg(config_path))
                    .map_err(ApplicationError::EditorCommand)?;
            }
        },

        Cli::Get {
            project_name,
            extended,
        } => {
            let registry = load_registry(deps)?;
            let (name, project) = project_name.as_ref().map_or_else(
                || get_active_project(deps, &registry),
                |name| {
                    registry
                        .get(name)
                        .map(|project| (name, project))
                        .ok_or_else(|| ApplicationError::NonExistentProject(name.clone()))
                },
            )?;
            if extended {
                let directory = project
                    .directory
                    .as_ref()
                    .map(|directory| directory.display().to_string())
                    .unwrap_or_default();
                let linked_port = project
                    .linked_port
                    .map(|port| port.to_string())
                    .unwrap_or_default();
                if deps.is_tty() {
                    write!(output, "port: {}\nname: {name}\ndirectory: {directory}\nlinked port: {linked_port}\n", project.port).unwrap();
                } else {
                    write!(
                        output,
                        "{}\n{name}\n{directory}\n{linked_port}\n",
                        project.port
                    )
                    .unwrap();
                }
            } else {
                writeln!(output, "{}", project.port).unwrap();
            }
        }

        Cli::Create {
            project_name,
            no_link,
            no_activate,
            overwrite,
        } => {
            let mut registry = load_registry(deps)?;
            let linked_port = if no_link {
                None
            } else {
                get_active_repo(deps)
                    .ok()
                    .and_then(|repo| registry.get_repo_port(&repo).ok())
            };
            let (name, project, updated) = create(
                deps,
                &mut registry,
                project_name,
                no_activate,
                linked_port,
                overwrite,
            )?;

            registry.save(deps)?;
            if deps.is_tty() {
                writeln!(
                    output,
                    "{} project {}",
                    if updated { "Updated" } else { "Created" },
                    format_project(&name, &project)
                )
                .unwrap();
            } else {
                // Only print the port if stdout isn't a TTY for easier scripting
                writeln!(output, "{}", project.port).unwrap();
            }
        }

        Cli::Delete { project_name } => {
            let mut registry = load_registry(deps)?;
            let project_name = match project_name {
                Some(name) => name,
                None => get_active_project(deps, &registry)?.0.clone(),
            };
            let project = registry.delete(&project_name)?;
            registry.save(deps)?;
            writeln!(
                output,
                "Deleted project {}",
                format_project(&project_name, &project),
            )
            .unwrap();
        }

        Cli::Cleanup => {
            let mut registry = load_registry(deps)?;
            let deleted_projects = cleanup(deps, &mut registry)?;
            registry.save(deps)?;
            writeln!(
                output,
                "Deleted {}",
                match deleted_projects.len() {
                    1 => String::from("1 project"),
                    count => format!("{count} projects"),
                }
            )
            .unwrap();
            for (name, project) in deleted_projects {
                writeln!(output, "{}", format_project(&name, &project)).unwrap();
            }
        }

        Cli::List => {
            let registry = load_registry(deps)?;
            registry.save(deps)?;
            for (name, project) in registry.iter_projects() {
                writeln!(output, "{}", format_project(name, project)).unwrap();
            }
        }

        Cli::Link {
            port,
            project_name,
            no_save,
        } => {
            let save_repo = port.is_some() && project_name.is_none() && !no_save;
            let mut registry = load_registry(deps)?;
            let project_name = match project_name {
                Some(name) => name,
                None => get_active_project(deps, &registry)?.0.clone(),
            };
            let port = match port {
                Some(name) => name,
                None => registry.get_repo_port(&get_active_repo(deps)?)?,
            };
            registry.link(deps, &project_name, port)?;
            writeln!(output, "Linked port {port} to project {project_name}").unwrap();
            if save_repo {
                if let Ok(repo) = get_active_repo(deps) {
                    writeln!(output, "Saved default port {port} for repo {repo}").unwrap();
                    registry.set_repo_port(repo, port);
                }
            }
            registry.save(deps)?;
        }

        Cli::Unlink { port } => {
            let mut registry = load_registry(deps)?;
            let unlinked_port = registry.unlink(port);
            registry.save(deps)?;
            match unlinked_port {
                Some(project_name) => {
                    writeln!(output, "Unlinked port {port} from project {project_name}").unwrap();
                }
                None => writeln!(output, "Port {port} was not linked to a project").unwrap(),
            }
        }

        Cli::Repo(subcommand) => match subcommand {
            Repo::Delete { repo } => {
                let mut registry = load_registry(deps)?;
                let port = registry.delete_repo(&repo)?;
                writeln!(output, "Deleted repo {}", format_repo(&repo, port)).unwrap();
                registry.save(deps)?;
            }

            Repo::List => {
                let registry = load_registry(deps)?;
                for (repo, port) in registry.iter_repos() {
                    writeln!(output, "{}", format_repo(repo, *port)).unwrap();
                }
            }
        },

        Cli::Caddyfile => {
            let registry = load_registry(deps)?;
            write!(output, "{}", generate_caddyfile(deps, &registry)?).unwrap();
        }

        Cli::ReloadCaddy => {
            let registry = load_registry(deps)?;
            reload(deps, &registry).map_err(ApplicationError::Caddy)?;
            writeln!(output, "Successfully reloaded caddy").unwrap();
        }
    }

    Ok(output)
}

enum RunStatus {
    Success,
    Failure,
}

fn run_and_suggest(
    deps: &(
         impl Args
         + CheckPath
         + ChoosePort
         + DataDir
         + Environment
         + Exec
         + ReadFile
         + Tty
         + WriteFile
         + WorkingDirectory
     ),
) -> (RunStatus, String) {
    let cli = Cli::parse_from(deps.get_args());

    let has_create_project_name = if let Cli::Create {
        ref project_name, ..
    } = cli
    {
        Some(project_name.is_some())
    } else {
        None
    };

    let linking_project = matches!(cli, Cli::Link { .. });
    let deleting_repo = matches!(cli, Cli::Repo(Repo::Delete { .. }));

    let err = match run(deps, cli) {
        Err(err) => err,
        Ok(output) => {
            return (RunStatus::Success, output);
        }
    };

    let mut output = format!("{err}\n");

    match err {
        ApplicationError::Caddy(CaddyError::Exec(ExecError::IO { io_err, .. }))
            if io_err.kind() == ErrorKind::NotFound =>
        {
            output +=
                "Try running `brew install caddy` or making sure that caddy is in your PATH.\n";
        }
        ApplicationError::Caddy(CaddyError::Exec(ExecError::Failed { code: 1, .. })) => {
            output +=
                "Try running `brew services start caddy` to make sure that caddy is running.\n";
        }
        ApplicationError::DuplicateDirectory(name, _) => {
            writeln!(output, "Try running the command in a different directory, providing the --no-activate flag, or running `portman delete {name}` and rerunning the command.").unwrap();
        }
        ApplicationError::DuplicateProject(_) => {
            if let Some(has_project_name) = has_create_project_name {
                if has_project_name {
                    output +=
                        "Try providing the --overwrite flag to modify the existing project.\n";
                } else {
                    output += "Try manually providing a project name.\n";
                }
            }
        }
        ApplicationError::EditorCommand(ExecError::IO { io_err, .. })
            if io_err.kind() == ErrorKind::NotFound =>
        {
            output += "Try setting the $EDITOR environment variable to a valid command like vi or nano.\n";
        }
        ApplicationError::EmptyAllocator => {
            output += "Try running `portman config edit` to edit the config file and modify the `ranges` field to allow more ports.\n";
        }
        ApplicationError::GitCommand(_) => {
            output += "Try running `portman link` in a directory with a git repo or providing an explicit port.\n";
        }
        ApplicationError::InvalidConfig(_) => {
            output += "Try running `portman config edit` to edit the config file and correct the error.\n";
        }
        ApplicationError::InvalidProjectName(_, _) => {
            if has_create_project_name == Some(false) {
                output += "Try manually providing a project name.\n";
            }
        }
        ApplicationError::MissingCustomConfig(path) => {
            writeln!(output, "Try creating a config file at \"{}\" or unsetting the $PORTMAN_CONFIG environment variable.", path.display()).unwrap();
        }
        ApplicationError::NoActiveProject => {
            output += "Try running the command again in a directory containing a project or providing an explicit project name.\n";
        }
        ApplicationError::NonExistentProject(_) => {
            output += "Try providing a different project name.\n";
        }
        ApplicationError::NonExistentRepo(_) => {
            if linking_project {
                output += "Try providing an explicit port.\n";
            }
            if deleting_repo {
                output += "Try running `portman repo list` to see which repos exist.\n";
            }
        }
        _ => {}
    }

    (RunStatus::Failure, output)
}

fn main() -> std::io::Result<ExitCode> {
    let deps = Impl::new(());
    let (status, output) = run_and_suggest(&deps);
    Ok(match status {
        RunStatus::Success => {
            std::io::stdout().write_all(output.as_bytes())?;
            ExitCode::SUCCESS
        }
        RunStatus::Failure => {
            std::io::stderr().write_all(output.as_bytes())?;
            ExitCode::FAILURE
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependencies::{
        CheckPathMock, ChoosePortMock, EnvironmentMock, ExecMock, ExecStatus, ReadFileMock,
    };
    use crate::mocks::{
        args_mock, choose_port_mock, cwd_mock, data_dir_mock, exec_git_mock, exec_mock,
        read_registry_mock, read_var_mock, tty_mock, write_caddyfile_mock, write_file_mock,
        write_registry_mock,
    };
    use anyhow::bail;
    use std::io::Error;
    use unimock::{Clause, MockFn, Unimock, matching};

    fn exec_git_no_repo_mock() -> impl Clause {
        ExecMock
            .each_call(matching!((command) if command.get_program() == "git"))
            .answers(&|_, _| {
                Ok(ExecStatus::Failure {
                    output: String::from("No repo\n"),
                    code: 1,
                })
            })
            .once()
    }

    fn read_file_mock() -> impl Clause {
        ReadFileMock
            .each_call(matching!((path) if path == &PathBuf::from("/data/config.toml") || path == &PathBuf::from("/homebrew/etc/Caddyfile")))
            .answers(&|_, _| Err(Error::from(std::io::ErrorKind::NotFound)))
            .at_least_times(1)
    }

    fn readonly_mocks() -> impl Clause {
        (
            data_dir_mock(),
            read_registry_mock(None),
            read_file_mock(),
            read_var_mock(),
        )
    }

    fn readwrite_mocks() -> impl Clause {
        (readonly_mocks(), exec_mock(), write_caddyfile_mock())
    }

    #[test]
    fn test_format_project_simple() {
        assert_eq!(
            format_project(
                "app1",
                &Project {
                    port: 3001,
                    directory: None,
                    linked_port: None,
                },
            ),
            String::from("app1 :3001"),
        );
    }

    #[test]
    fn test_format_project_complex() {
        assert_eq!(
            format_project(
                "app1",
                &Project {
                    port: 3001,
                    directory: Some(PathBuf::from("/projects/app1")),
                    linked_port: Some(3000),
                },
            ),
            String::from("app1 :3001 -> :3000 (/projects/app1)"),
        );
    }

    #[test]
    fn test_config_init_bash() {
        let mocked_deps = Unimock::new(args_mock("portman init bash"));

        let output = run_and_suggest(&mocked_deps).1;
        assert!(!output.is_empty());
    }

    #[test]
    fn test_config_init_fish() {
        let mocked_deps = Unimock::new(args_mock("portman init fish"));

        let output = run_and_suggest(&mocked_deps).1;
        assert!(!output.is_empty());
    }

    #[test]
    fn test_config_init_zsh() {
        let mocked_deps = Unimock::new(args_mock("portman init zsh"));

        let output = run_and_suggest(&mocked_deps).1;
        assert!(!output.is_empty());
    }

    #[test]
    fn test_config_edit() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            exec_mock(),
            read_var_mock(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Opening \"/data/config.toml\" with \"editor\"\n");
    }

    #[test]
    fn test_config_edit_no_editor_env() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            EnvironmentMock
                .each_call(matching!("PORTMAN_CONFIG"))
                .answers(&|_, _| bail!("Failed"))
                .once(),
            EnvironmentMock
                .each_call(matching!("EDITOR"))
                .answers(&|_, _| bail!("Failed"))
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Failed\n");
    }

    #[test]
    fn test_config_edit_editor_not_found() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            read_var_mock(),
            ExecMock
                .each_call(matching!((command) if command.get_program() == "editor"))
                .answers(&|_, _| Err(Error::from(std::io::ErrorKind::NotFound)))
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r#"Editor command could not be run:

Command "editor /data/config.toml" failed to run:
entity not found
Try setting the $EDITOR environment variable to a valid command like vi or nano.
"#
        );
    }

    #[test]
    fn test_config_edit_editor_failed() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            read_var_mock(),
            ExecMock
                .each_call(matching!((command) if command.get_program() == "editor"))
                .answers(&|_, _| {
                    Ok(ExecStatus::Failure {
                        output: String::from("Invalid config\n"),
                        code: 1,
                    })
                })
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r#"Editor command could not be run:

Command "editor /data/config.toml" failed with exit code 1 and output:
Invalid config

"#
        );
    }

    #[test]
    fn test_config_show() {
        let mocked_deps = Unimock::new((
            args_mock("portman config show"),
            data_dir_mock(),
            read_file_mock(),
            read_var_mock(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Config path: /data/config.toml
Registry path: /data/registry.toml
Configuration:
--------------
Allowed port ranges: 3000-3999
"
        );
    }

    #[test]
    fn test_config_show_custom_config() {
        let mocked_deps = Unimock::new((
            args_mock("portman config show"),
            data_dir_mock(),
            read_var_mock(),
            ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("/data/config.toml")))
                .answers(&|_, _| Ok(include_str!("fixtures/custom_config.toml").to_owned()))
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Config path: /data/config.toml
Registry path: /data/registry.toml
Configuration:
--------------
Allowed port ranges: 2000-2199 & 4100-4199
Reserved ports: 2002, 4004
"
        );
    }

    #[test]
    fn test_config_show_custom_path() {
        let mocked_deps = Unimock::new((
            args_mock("portman config show"),
            data_dir_mock(),
            EnvironmentMock
                .each_call(matching!("PORTMAN_CONFIG"))
                .answers(&|_, _| Ok("/data/custom_config.toml".to_owned()))
                .at_least_times(1),
            ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("/data/custom_config.toml")))
                .answers(&|_, _| Ok(include_str!("fixtures/custom_config.toml").to_owned()))
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Config path: /data/custom_config.toml
Registry path: /data/registry.toml
Configuration:
--------------
Allowed port ranges: 2000-2199 & 4100-4199
Reserved ports: 2002, 4004
"
        );
    }

    #[test]
    fn test_config_show_custom_path_not_found() {
        let mocked_deps = Unimock::new((
            args_mock("portman config show"),
            EnvironmentMock
                .each_call(matching!("PORTMAN_CONFIG"))
                .answers(&|_, _| Ok("/data/custom_config.toml".to_owned()))
                .at_least_times(1),
            ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("/data/custom_config.toml")))
                .answers(&|_, _| Err(Error::from(std::io::ErrorKind::NotFound)))
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r#"Custom config file at "/data/custom_config.toml" does not exist
Try creating a config file at "/data/custom_config.toml" or unsetting the $PORTMAN_CONFIG environment variable.
"#
        );
    }

    #[test]
    fn test_get() {
        let mocked_deps =
            Unimock::new((readonly_mocks(), args_mock("portman get"), cwd_mock("app3")));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "3003\n");
    }

    #[test]
    fn test_get_name() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman get app2")));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "3002\n");
    }

    #[test]
    fn test_get_name_non_existent() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman get project")));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Project project does not exist
Try providing a different project name.
"
        );
    }

    #[test]
    fn test_get_extended() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman get --extended"),
            cwd_mock("app3"),
            tty_mock(true),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "port: 3003\nname: app3\ndirectory: /projects/app3\nlinked port: \n"
        );
    }

    #[test]
    fn test_get_extended_not_tty() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman get --extended"),
            cwd_mock("app3"),
            tty_mock(false),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"3003
app3
/projects/app3

"
        );
    }

    #[test]
    fn test_create() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create"),
            choose_port_mock(),
            cwd_mock("project"),
            exec_git_mock("project"),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "Created project project :3004 (/projects/project)\n"
        );
    }

    #[test]
    fn test_create_not_tty() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create"),
            choose_port_mock(),
            cwd_mock("project"),
            exec_git_mock("project"),
            tty_mock(false),
            write_registry_mock(include_str!("snapshots/create.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "3004\n");
    }

    #[test]
    fn test_create_duplicate_directory() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman create project"),
            cwd_mock("app3"),
            exec_git_mock("project"),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r#"Project app3 already uses the directory "/projects/app3"
Try running the command in a different directory, providing the --no-activate flag, or running `portman delete app3` and rerunning the command.
"#
        );
    }

    #[test]
    fn test_create_duplicate_project() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman create"),
            cwd_mock("app3"),
            exec_git_mock("app3"),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"A project already has the name app3
Try manually providing a project name.
"
        );
    }

    #[test]
    fn test_create_duplicate_project_explicit_name() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman create app3"),
            cwd_mock("app3"),
            exec_git_mock("app3"),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"A project already has the name app3
Try providing the --overwrite flag to modify the existing project.
"
        );
    }

    #[test]
    fn test_create_link() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create"),
            choose_port_mock(),
            cwd_mock("project"),
            exec_git_mock("app3"),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create_link.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "Created project project :3005 -> :3004 (/projects/project)\n"
        );
    }

    #[test]
    fn test_create_no_repo() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create"),
            choose_port_mock(),
            cwd_mock("project"),
            exec_git_no_repo_mock(),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create_no_repo.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "Created project project :3004 (/projects/project)\n"
        );
    }

    #[test]
    fn test_create_unknown_repo() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create"),
            choose_port_mock(),
            cwd_mock("project"),
            exec_git_mock("project"),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create_unknown_repo.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "Created project project :3004 (/projects/project)\n"
        );
    }

    #[test]
    fn test_create_caddy_failed() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman create"),
            choose_port_mock(),
            cwd_mock("project"),
            exec_git_mock("project"),
            ExecMock
                .each_call(matching!((command) if command.get_program() == "caddy"))
                .answers(&|_, _| {
                    Ok(ExecStatus::Failure {
                        output: String::from("caddy is not running\n"),
                        code: 1,
                    })
                })
                .once(),
            write_file_mock(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r#"Error reloading caddy:
Command "caddy reload --adapter caddyfile --config /homebrew/etc/Caddyfile" failed with exit code 1 and output:
caddy is not running

Try running `brew services start caddy` to make sure that caddy is running.
"#
        );
    }

    #[test]
    fn test_create_caddy_not_found() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman create"),
            choose_port_mock(),
            cwd_mock("project"),
            exec_git_mock("project"),
            ExecMock
                .each_call(matching!((command) if command.get_program() == "caddy"))
                .answers(&|_, _| Err(Error::from(std::io::ErrorKind::NotFound)))
                .once(),
            write_file_mock(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r#"Error reloading caddy:
Command "caddy reload --adapter caddyfile --config /homebrew/etc/Caddyfile" failed to run:
entity not found
Try running `brew install caddy` or making sure that caddy is in your PATH.
"#
        );
    }

    #[test]
    fn test_create_empty_allocator() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman create"),
            ChoosePortMock.each_call(matching!(_)).returns(None).once(),
            cwd_mock("project"),
            exec_git_mock("project"),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"All available ports have been allocated already
Try running `portman config edit` to edit the config file and modify the `ranges` field to allow more ports.
"
        );
    }

    #[test]
    fn test_create_invalid_config() {
        let mocked_deps = Unimock::new((
            args_mock("portman create"),
            data_dir_mock(),
            read_var_mock(),
            ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("/data/config.toml")))
                .answers(&|_, _| Ok(include_str!("fixtures/invalid_config.toml").to_owned()))
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Configuration is invalid:

Validation error: port ranges must not be empty

Try running `portman config edit` to edit the config file and correct the error.
"
        );
    }

    #[test]
    fn test_create_invalid_name() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman create"),
            cwd_mock("-"),
            exec_git_mock("project"),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r#"Project name "" is invalid: must not be empty
Try manually providing a project name.
"#
        );
    }

    #[test]
    fn test_create_no_link() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create --no-link"),
            choose_port_mock(),
            cwd_mock("project"),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create_no_link.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "Created project project :3004 (/projects/project)\n"
        );
    }

    #[test]
    fn test_create_name() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create project"),
            choose_port_mock(),
            cwd_mock("project"),
            exec_git_mock("project"),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create_name.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "Created project project :3004 (/projects/project)\n"
        );
    }

    #[test]
    fn test_create_no_activate() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create project --no-activate"),
            choose_port_mock(),
            exec_git_mock("project"),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create_no_activate.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Created project project :3004\n");
    }

    #[test]
    fn test_create_no_activate_no_link() {
        let mocked_deps = Unimock::new(args_mock("portman create project --no-activate --no-link"));

        let err = Cli::try_parse_from(mocked_deps.get_args()).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn test_create_overwrite() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create app3 --overwrite"),
            cwd_mock("project"),
            exec_git_mock("project"),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create_overwrite.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Updated project app3 :3003 (/projects/project)\n");
    }

    #[test]
    fn test_create_overwrite_link() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman create app3 --overwrite"),
            cwd_mock("project"),
            exec_git_mock("app3"),
            tty_mock(true),
            write_registry_mock(include_str!("snapshots/create_overwrite_link.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "Updated project app3 :3003 -> :3004 (/projects/project)\n"
        );
    }

    #[test]
    fn test_create_no_activate_no_name() {
        let mocked_deps = Unimock::new(args_mock("portman create --no-activate"));

        let err = Cli::try_parse_from(mocked_deps.get_args()).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn test_delete() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman delete"),
            cwd_mock("app3"),
            write_registry_mock(include_str!("snapshots/delete.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Deleted project app3 :3003 (/projects/app3)\n");
    }

    #[test]
    fn test_delete_no_active() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman delete"),
            cwd_mock("app2"),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"The current directory does not contain a project
Try running the command again in a directory containing a project or providing an explicit project name.
"
        );
    }

    #[test]
    fn test_delete_name() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman delete app3"),
            write_registry_mock(include_str!("snapshots/delete_name.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Deleted project app3 :3003 (/projects/app3)\n");
    }

    #[test]
    fn test_cleanup_single() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman cleanup"),
            write_registry_mock(include_str!("snapshots/cleanup_single.toml")),
            CheckPathMock
                .each_call(matching!((path) if path == &PathBuf::from("/projects/app3")))
                .returns(false)
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Deleted 1 project
app3 :3003 (/projects/app3)
"
        );
    }

    #[test]
    fn test_cleanup_none() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman cleanup"),
            CheckPathMock
                .each_call(matching!((path) if path == &PathBuf::from("/projects/app3")))
                .returns(true)
                .once(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Deleted 0 projects\n");
    }

    #[test]
    fn test_list() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman list")));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"app1 :3001
app2 :3002 -> :3000
app3 :3003 (/projects/app3)
"
        );
    }

    #[test]
    fn test_link() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman link"),
            cwd_mock("app3"),
            exec_git_mock("app3"),
            write_registry_mock(include_str!("snapshots/link.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Linked port 3004 to project app3\n");
    }

    #[test]
    fn test_link_no_repo() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman link"),
            cwd_mock("app3"),
            exec_git_no_repo_mock(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r#"Git command could not be run:

Command "git remote get-url origin" failed with exit code 1 and output:
No repo

Try running `portman link` in a directory with a git repo or providing an explicit port.
"#
        );
    }

    #[test]
    fn test_link_unknown_repo() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman link"),
            cwd_mock("app3"),
            exec_git_mock("project"),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Repo https://github.com/user/project.git does not exist
Try providing an explicit port.
"
        );
    }

    #[test]
    fn test_link_no_save() {
        let mocked_deps = Unimock::new(args_mock("portman link --no-save"));

        let err = Cli::try_parse_from(mocked_deps.get_args()).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn test_link_port() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman link 3005"),
            cwd_mock("app3"),
            exec_git_mock("app3"),
            write_registry_mock(include_str!("snapshots/link_port.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Linked port 3005 to project app3
Saved default port 3005 for repo https://github.com/user/app3.git
"
        );
    }

    #[test]
    fn test_link_port_no_repo() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman link 3005"),
            cwd_mock("app3"),
            exec_git_no_repo_mock(),
            write_registry_mock(include_str!("snapshots/link_port_no_repo.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Linked port 3005 to project app3\n");
    }

    #[test]
    fn test_link_port_no_save() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman link 3005 --no-save"),
            cwd_mock("app3"),
            write_registry_mock(include_str!("snapshots/link_port_no_save.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Linked port 3005 to project app3\n");
    }

    #[test]
    fn test_link_port_and_project() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman link 3005 app3"),
            write_registry_mock(include_str!("snapshots/link_port_and_project.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Linked port 3005 to project app3\n");
    }

    #[test]
    fn test_link_port_and_project_no_save() {
        let mocked_deps = Unimock::new(args_mock("portman link 3005 app3 --no-save"));

        let err = Cli::try_parse_from(mocked_deps.get_args()).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn test_unlink() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman unlink 3000"),
            write_registry_mock(include_str!("snapshots/unlink.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Unlinked port 3000 from project app2\n");
    }

    #[test]
    fn test_unlink_not_linked() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman unlink 3005")));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Port 3005 was not linked to a project\n");
    }

    #[test]
    fn test_repo_delete() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman repo delete https://github.com/user/app3.git"),
            write_registry_mock(include_str!("snapshots/repo_delete.toml")),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            "Deleted repo https://github.com/user/app3.git: 3004\n"
        );
    }

    #[test]
    fn test_repo_delete_non_existent() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman repo delete https://github.com/user/project.git"),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(
            output,
            r"Repo https://github.com/user/project.git does not exist
Try running `portman repo list` to see which repos exist.
"
        );
    }

    #[test]
    fn test_repo_delete_list() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman repo list")));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "https://github.com/user/app3.git: 3004\n");
    }

    #[test]
    fn test_caddyfile() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman caddyfile")));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, include_str!("snapshots/Caddyfile"));
    }

    #[test]
    fn test_reload_caddy() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman reload-caddy"),
            exec_mock(),
            write_file_mock(),
        ));

        let output = run_and_suggest(&mocked_deps).1;
        assert_eq!(output, "Successfully reloaded caddy\n");
    }
}
