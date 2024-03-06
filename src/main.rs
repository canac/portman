#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod allocator;
mod caddy;
mod cli;
mod config;
mod dependencies;
mod error;
mod init;
#[cfg(test)]
mod mocks;
mod registry;

use crate::allocator::PortAllocator;
use crate::caddy::{generate_caddyfile, reload};
use crate::cli::{Cli, Config as ConfigSubcommand, InitShell};
use crate::config::Config;
use crate::error::Result;
use crate::init::init_fish;
use crate::registry::Registry;
use anyhow::Context;
use clap::Parser;
use dependencies::{
    Args, CheckPath, ChoosePort, DataDir, Environment, Exec, ReadFile, Tty, WorkingDirectory,
    WriteFile,
};
use entrait::Impl;
use error::ApplicationError;
use registry::Project;
use std::fmt::Write;
use std::path::PathBuf;
use std::process::{self, Command};

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
    deps.exec(
        Command::new("git").args(["remote", "get-url", "origin"]),
        &mut (),
    )
    .map(|repo| repo.trim_end().to_string())
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
    deps: &(impl Args
          + CheckPath
          + ChoosePort
          + DataDir
          + Environment
          + Exec
          + ReadFile
          + Tty
          + WriteFile
          + WorkingDirectory),
    cli: Cli,
) -> Result<String> {
    let mut output = String::new();
    match cli {
        Cli::Init { shell } => match shell {
            InitShell::Fish => {
                writeln!(output, "{}", init_fish()).unwrap();
            }
        },

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
                deps.exec(Command::new(editor).arg(config_path), &mut ())
                    .map_err(ApplicationError::EditorCommand)?;
            }
        },

        Cli::Get {
            project_name,
            extended,
        } => {
            let registry = load_registry(deps)?;
            let (name, project) = match project_name {
                Some(ref name) => registry
                    .get(name)
                    .map(|project| (name, project))
                    .ok_or_else(|| ApplicationError::NonExistentProject(name.clone())),
                None => get_active_project(deps, &registry),
            }?;
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
            if save_repo {
                if let Ok(repo) = get_active_repo(deps) {
                    registry.set_repo_port(repo, port);
                }
            }
            registry.save(deps)?;
            writeln!(output, "Linked port {port} to project {project_name}").unwrap();
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
            };
        }

        Cli::Caddyfile => {
            let registry = load_registry(deps)?;
            write!(output, "{}", generate_caddyfile(deps, &registry)?).unwrap();
        }

        Cli::ReloadCaddy => {
            let registry = load_registry(deps)?;
            reload(deps, &registry)?;
            writeln!(output, "Successfully reloaded caddy").unwrap();
        }
    };

    Ok(output)
}

fn main() {
    let deps = Impl::new(());
    let cli = Cli::parse_from(deps.get_args());

    let has_create_project_name = if let Cli::Create {
        ref project_name, ..
    } = cli
    {
        Some(project_name.is_some())
    } else {
        None
    };

    let err = match run(&deps, cli) {
        Err(err) => err,
        Ok(output) => {
            print!("{output}");
            return;
        }
    };
    eprintln!("{err}");

    match err {
        ApplicationError::Caddy(_) => {
            eprintln!("\nTry running `brew install caddy`");
        }
        ApplicationError::DuplicateDirectory(name, _) => {
            eprintln!("\nTry running the command in a different directory, providing the --no-activate flag, or running `portman delete {name}` and rerunning the command.");
        }
        ApplicationError::DuplicateProject(_) => {
            if let Some(has_project_name) = has_create_project_name {
                if has_project_name {
                    eprintln!(
                        "\nTry providing the --overwrite flag to modify the existing project."
                    );
                } else {
                    eprintln!("\nTry manually providing a project name.");
                }
            }
        }
        ApplicationError::EditorCommand(_) => {
            eprintln!("\nTry setting the $EDITOR environment variable to a valid command like vi or nano.");
        }
        ApplicationError::EmptyAllocator => {
            eprintln!("\nTry running `portman config edit` to edit the config file and modify the `ranges` field to allow more ports.");
        }
        ApplicationError::GitCommand(_) => {
            eprintln!("\nTry running `portman link` in a directory with a git repo or providing an explicit port.");
        }
        ApplicationError::InvalidConfig(_) => {
            eprintln!("\nTry running `portman config edit` to edit the config file and correct the error.");
        }
        ApplicationError::InvalidProjectName(_, _) => {
            if has_create_project_name == Some(false) {
                eprintln!("\nTry manually providing a project name.");
            }
        }
        ApplicationError::MissingCustomConfig(path) => {
            eprintln!("\nTry creating a config file at \"{}\" or unsetting the $PORTMAN_CONFIG environment variable.", path.display());
        }
        ApplicationError::NoActiveProject => {
            eprintln!("\nTry running the command again in a directory containing a project or providing an explicit project name.");
        }
        ApplicationError::NonExistentRepo(_) => {
            eprintln!("\nTry providing an explicit port.");
        }
        _ => {}
    };

    process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependencies::{CheckPathMock, EnvironmentMock, ExecMock, ReadFileMock};
    use crate::mocks::{
        args_mock, choose_port_mock, cwd_mock, data_dir_mock, exec_git_mock, exec_mock,
        read_registry_mock, read_var_mock, tty_mock, write_caddyfile_mock, write_file_mock,
        write_registry_mock,
    };
    use anyhow::bail;
    use unimock::{matching, Clause, MockFn, Unimock};

    fn exec_git_no_repo_mock() -> impl Clause {
        ExecMock
            .each_call(matching!((command, _) if command.get_program() == "git"))
            .answers(|_| bail!("No repo"))
            .once()
    }

    fn read_file_mock() -> impl Clause {
        ReadFileMock
            .each_call(matching!((path) if path == &PathBuf::from("/data/config.toml") || path == &PathBuf::from("/homebrew/etc/Caddyfile")))
            .answers(|_| Ok(None))
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
    fn test_config_init_fish() {
        let mocked_deps = Unimock::new(args_mock("portman init fish"));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Opening \"/data/config.toml\" with \"editor\"\n");
    }

    #[test]
    fn test_config_edit_no_editor_env() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            EnvironmentMock
                .each_call(matching!("PORTMAN_CONFIG"))
                .answers(|_| bail!("Failed"))
                .once(),
            EnvironmentMock
                .each_call(matching!("EDITOR"))
                .answers(|_| bail!("Failed"))
                .once(),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let err = run(&mocked_deps, cli).unwrap_err();
        assert!(matches!(err, ApplicationError::Other(_)));
    }

    #[test]
    fn test_config_edit_editor_exec_fails() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            read_var_mock(),
            ExecMock
                .each_call(matching!((command, _) if command.get_program() == "editor"))
                .answers(|_| bail!("Failed"))
                .once(),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let err = run(&mocked_deps, cli).unwrap_err();
        assert!(matches!(err, ApplicationError::EditorCommand(_)));
    }

    #[test]
    fn test_config_show() {
        let mocked_deps = Unimock::new((
            args_mock("portman config show"),
            data_dir_mock(),
            read_file_mock(),
            read_var_mock(),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Config path: /data/config.toml\nRegistry path: /data/registry.toml\nConfiguration:\n--------------\nAllowed port ranges: 3000-3999\n");
    }

    #[test]
    fn test_config_show_custom_config() {
        let mocked_deps = Unimock::new((
            args_mock("portman config show"),
            data_dir_mock(),
            read_var_mock(),
            ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("/data/config.toml")))
                .answers(|_| Ok(Some(include_str!("fixtures/config.toml").to_string())))
                .once(),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Config path: /data/config.toml\nRegistry path: /data/registry.toml\nConfiguration:\n--------------\nAllowed port ranges: 2000-2199 & 4100-4199\nReserved ports: 2002, 4004\n");
    }

    #[test]
    fn test_config_show_custom_path() {
        let mocked_deps = Unimock::new((
            args_mock("portman config show"),
            data_dir_mock(),
            EnvironmentMock
                .each_call(matching!("PORTMAN_CONFIG"))
                .answers(|_| Ok("/data/custom_config.toml".to_string()))
                .at_least_times(1),
            ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("/data/custom_config.toml")))
                .answers(|_| Ok(Some(include_str!("fixtures/config.toml").to_string())))
                .once(),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Config path: /data/custom_config.toml\nRegistry path: /data/registry.toml\nConfiguration:\n--------------\nAllowed port ranges: 2000-2199 & 4100-4199\nReserved ports: 2002, 4004\n");
    }

    #[test]
    fn test_config_show_custom_path_missing() {
        let mocked_deps = Unimock::new((
            args_mock("portman config show"),
            EnvironmentMock
                .each_call(matching!("PORTMAN_CONFIG"))
                .answers(|_| Ok("/data/custom_config.toml".to_string()))
                .at_least_times(1),
            ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("/data/custom_config.toml")))
                .answers(|_| Ok(None))
                .once(),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let err = run(&mocked_deps, cli).unwrap_err();
        assert!(matches!(err, ApplicationError::MissingCustomConfig(_)));
    }

    #[test]
    fn test_get() {
        let mocked_deps =
            Unimock::new((readonly_mocks(), args_mock("portman get"), cwd_mock("app3")));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "3003\n");
    }

    #[test]
    fn test_get_name() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman get app2")));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "3002\n");
    }

    #[test]
    fn test_get_extended() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman get --extended"),
            cwd_mock("app3"),
            tty_mock(true),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "3003\napp3\n/projects/app3\n\n");
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "3004\n");
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(
            output,
            "Created project project :3004 (/projects/project)\n"
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Deleted project app3 :3003 (/projects/app3)\n");
    }

    #[test]
    fn test_delete_name() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman delete app3"),
            write_registry_mock(include_str!("snapshots/delete_name.toml")),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Deleted 1 project\napp3 :3003 (/projects/app3)\n");
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Deleted 0 projects\n");
    }

    #[test]
    fn test_list() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman list")));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(
            output,
            "app1 :3001\napp2 :3002 -> :3000\napp3 :3003 (/projects/app3)\n"
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let err = run(&mocked_deps, cli).unwrap_err();
        assert!(matches!(err, ApplicationError::GitCommand(_)));
    }

    #[test]
    fn test_link_unknown_repo() {
        let mocked_deps = Unimock::new((
            readonly_mocks(),
            args_mock("portman link"),
            cwd_mock("app3"),
            exec_git_mock("project"),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let err = run(&mocked_deps, cli).unwrap_err();
        assert!(matches!(err, ApplicationError::NonExistentRepo(_)));
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Linked port 3005 to project app3\n");
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Linked port 3005 to project app3\n");
    }

    #[test]
    fn test_link_port_and_project() {
        let mocked_deps = Unimock::new((
            readwrite_mocks(),
            args_mock("portman link 3005 app3"),
            write_registry_mock(include_str!("snapshots/link_port_and_project.toml")),
        ));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Unlinked port 3000 from project app2\n");
    }

    #[test]
    fn test_unlink_not_linked() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman unlink 3005")));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Port 3005 was not linked to a project\n");
    }

    #[test]
    fn test_caddyfile() {
        let mocked_deps = Unimock::new((readonly_mocks(), args_mock("portman caddyfile")));
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
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
        let cli = Cli::try_parse_from(mocked_deps.get_args()).unwrap();

        let output = run(&mocked_deps, cli).unwrap();
        assert_eq!(output, "Successfully reloaded caddy\n");
    }
}
