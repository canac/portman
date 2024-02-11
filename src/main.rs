#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod allocator;
mod caddy;
mod cli;
mod config;
mod dependencies;
mod init;
#[cfg(test)]
mod mocks;
mod registry;

use crate::allocator::PortAllocator;
use crate::caddy::{generate_caddyfile, reload};
use crate::cli::{Cli, Config as ConfigSubcommand, InitShell};
use crate::config::Config;
use crate::init::init_fish;
use crate::registry::Registry;
use anyhow::{anyhow, bail, Context, Result};
use clap::{error::ErrorKind, Parser};
use dependencies::{
    Args, CheckPath, ChoosePort, DataDir, Environment, Exec, ReadFile, WorkingDirectory, WriteFile,
};
use entrait::Impl;
use registry::Project;
use std::fmt::Write;
use std::io::{stdout, IsTerminal};
use std::path::PathBuf;
use std::process;

// Find and return a reference to the active project based on the current directory
fn active_project<'registry>(
    deps: &impl WorkingDirectory,
    registry: &'registry Registry,
) -> Result<(&'registry String, &'registry Project)> {
    registry
        .match_cwd(deps)?
        .ok_or_else(|| anyhow!("No projects match the current directory"))
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
) -> Result<(String, Project)> {
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
        return Ok((name, project));
    }

    let project = registry.create(deps, &name, directory, linked_port)?;
    Ok((name, project))
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

#[allow(clippy::too_many_lines)]
fn run(
    deps: &(impl Args
          + CheckPath
          + ChoosePort
          + DataDir
          + Environment
          + Exec
          + ReadFile
          + WriteFile
          + WorkingDirectory),
) -> Result<()> {
    let data_dir = deps.get_data_dir()?;
    let config_env = deps.read_var("PORTMAN_CONFIG").ok();
    let config_env_present = config_env.is_some();
    let config_path = match config_env {
        Some(config_path) => PathBuf::from(config_path),
        None => data_dir.join("config.toml"),
    };
    let config = Config::load(deps, &config_path)?.unwrap_or_else(|| {
        if config_env_present {
            eprintln!("Warning: config file doesn't exist. Using default config.");
        }
        Config::default()
    });
    let port_allocator = PortAllocator::new(config.get_valid_ports());

    let cli = Cli::try_parse_from(deps.get_args());
    // Ignore errors caused by passing --help and --version
    if let Err(err) = cli.as_ref() {
        if matches!(
            err.kind(),
            ErrorKind::DisplayHelp
                | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                | ErrorKind::DisplayVersion
        ) {
            // The version or help text is contained in the error
            println!("{err}");
            return Ok(());
        }
    }

    match cli? {
        Cli::Init { shell } => match shell {
            InitShell::Fish => {
                println!("{}", init_fish());
            }
        },

        Cli::Config(subcommand) => match subcommand {
            ConfigSubcommand::Show => {
                println!(
                    "Config path: {}\nRegistry path: {}\nConfiguration:\n--------------\n{config}",
                    config_path.display(),
                    data_dir.join(PathBuf::from("registry.toml")).display()
                );
            }
            ConfigSubcommand::Edit => {
                let editor = deps.read_var("EDITOR")?;
                println!("Opening \"{}\" with \"{editor}\"", config_path.display());
                let (status, _) =
                    deps.exec(std::process::Command::new(editor).arg(config_path), &mut ())?;
                if !status.success() {
                    bail!("Editor command failed to execute successfully");
                }
            }
        },

        Cli::Get {
            project_name,
            extended,
        } => {
            let registry = Registry::new(deps, port_allocator)?;
            let (name, project) = match project_name {
                Some(ref name) => registry
                    .get(name)
                    .map(|project| (name, project))
                    .ok_or_else(|| anyhow!("Project {name} does not exist")),
                None => active_project(deps, &registry),
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
                if stdout().is_terminal() {
                    print!("port: {}\nname: {name}\ndirectory: {directory}\nlinked port: {linked_port}\n", project.port);
                } else {
                    print!("{}\n{name}\n{directory}\n{linked_port}\n", project.port);
                }
            } else {
                println!("{}", project.port);
            }
        }

        Cli::Create {
            project_name,
            link,
            no_activate,
            overwrite,
        } => {
            let mut registry = Registry::new(deps, port_allocator)?;
            let (name, project) = create(
                deps,
                &mut registry,
                project_name,
                no_activate,
                link,
                overwrite,
            )?;
            registry.save(deps)?;
            if stdout().is_terminal() {
                println!("Created project {}", format_project(&name, &project));
                if !no_activate {
                    println!("\nThe PORT environment variable will now be automatically set whenever this directory is cd-ed into from an initialized shell.");
                }
            } else {
                // Only print the port if stdout isn't a TTY for easier scripting
                println!("{}", project.port);
            }
        }

        Cli::Delete { project_name } => {
            let mut registry = Registry::new(deps, port_allocator)?;
            let project_name = match project_name {
                Some(name) => name,
                None => active_project(deps, &registry)?.0.clone(),
            };
            let project = registry.delete(&project_name)?;
            registry.save(deps)?;
            println!(
                "Deleted project {}",
                format_project(&project_name, &project),
            );
        }

        Cli::Cleanup => {
            let mut registry = Registry::new(deps, port_allocator)?;
            let deleted_projects = cleanup(deps, &mut registry)?;
            registry.save(deps)?;
            print!(
                "Deleted {}\n{}",
                match deleted_projects.len() {
                    1 => String::from("1 project"),
                    count => format!("{count} projects"),
                },
                deleted_projects
                    .iter()
                    .fold(String::new(), |mut output, (name, project)| {
                        let _ = writeln!(output, "{}", format_project(name, project));
                        output
                    })
            );
        }

        Cli::Reset => {
            let mut registry = Registry::new(deps, port_allocator)?;
            registry.delete_all();
            registry.save(deps)?;
            println!("Deleted all projects");
        }

        Cli::List => {
            let registry = Registry::new(deps, port_allocator)?;
            let output =
                registry
                    .iter_projects()
                    .fold(String::new(), |mut output, (name, project)| {
                        let _ = writeln!(output, "{}", format_project(name, project));
                        output
                    });
            registry.save(deps)?;
            print!("{output}");
        }

        Cli::Link { port, project_name } => {
            let mut registry = Registry::new(deps, port_allocator)?;
            let project_name = match project_name {
                Some(name) => name,
                None => active_project(deps, &registry)?.0.clone(),
            };
            registry.link(deps, &project_name, port)?;
            registry.save(deps)?;
            println!("Linked port {port} to project {project_name}");
        }

        Cli::Unlink { port } => {
            let mut registry = Registry::new(deps, port_allocator)?;
            let unlinked_port = registry.unlink(port);
            registry.save(deps)?;
            match unlinked_port {
                Some(project_name) => println!("Unlinked port {port} from project {project_name}"),
                None => println!("Port {port} was not linked to a project"),
            };
        }

        Cli::Caddyfile => {
            let registry = Registry::new(deps, port_allocator)?;
            print!("{}", generate_caddyfile(deps, &registry)?);
        }

        Cli::ReloadCaddy => {
            let registry = Registry::new(deps, port_allocator)?;
            reload(deps, &registry)?;
            println!("Successfully reloaded caddy");
        }
    }

    Ok(())
}

fn main() {
    let deps = Impl::new(());
    if let Err(err) = run(&deps) {
        eprintln!("{err}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::{
        args_mock, choose_port_mock, cwd_mock, data_dir_mock, exec_mock, get_mocked_registry,
        read_registry_mock, read_var_mock, write_file_mock,
    };
    use std::os::unix::process::ExitStatusExt;
    use unimock::{matching, Clause, MockFn, Unimock};

    fn read_file_mock() -> impl Clause {
        dependencies::ReadFileMock
            .each_call(matching!((path) if path == &PathBuf::from("/data/config.toml") || path == &PathBuf::from("/homebrew/etc/Caddyfile")))
            .answers(|_| Ok(None))
            .at_least_times(1)
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
    fn test_create() {
        let mut registry = get_mocked_registry().unwrap();
        let mocked_deps = Unimock::new((choose_port_mock(), cwd_mock("project")));
        let (name, project) = create(
            &mocked_deps,
            &mut registry,
            Some(String::from("project")),
            false,
            None,
            false,
        )
        .unwrap();
        assert_eq!(name, String::from("project"));
        assert_eq!(
            project,
            Project {
                port: 3004,
                directory: Some(PathBuf::from("/projects/project")),
                linked_port: None,
            },
        );
    }

    #[test]
    fn test_create_overwrite() {
        let mut registry = get_mocked_registry().unwrap();
        let mocked_deps = Unimock::new(cwd_mock("app2"));
        let (name, project) = create(
            &mocked_deps,
            &mut registry,
            Some(String::from("app2")),
            false,
            Some(3100),
            true,
        )
        .unwrap();
        assert_eq!(name, String::from("app2"));
        assert_eq!(
            project,
            Project {
                port: 3002,
                directory: Some(PathBuf::from("/projects/app2")),
                linked_port: Some(3100),
            },
        );
    }

    #[test]
    fn test_cleanup() {
        let mut registry = get_mocked_registry().unwrap();
        let mocked_deps = Unimock::new(
            dependencies::CheckPathMock
                .each_call(matching!((path) if path == &PathBuf::from("/projects/app3")))
                .returns(false)
                .n_times(1),
        );

        let cleaned_projects = cleanup(&mocked_deps, &mut registry).unwrap();
        assert_eq!(cleaned_projects.len(), 1);
        assert_eq!(cleaned_projects.first().unwrap().0, String::from("app3"));
    }

    #[test]
    fn test_cli_version() {
        let mocked_deps = Unimock::new((
            args_mock("portman --version"),
            data_dir_mock(),
            read_file_mock(),
            read_var_mock(),
        ));

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_create() {
        let mocked_deps = Unimock::new((
            args_mock("portman create"),
            choose_port_mock(),
            cwd_mock("project"),
            data_dir_mock(),
            exec_mock(),
            read_file_mock(),
            read_registry_mock(None),
            read_var_mock(),
            write_file_mock(),
        ));

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_create_no_activate() {
        let mocked_deps = Unimock::new((
            args_mock("portman create project --no-activate"),
            choose_port_mock(),
            data_dir_mock(),
            exec_mock(),
            read_file_mock(),
            read_registry_mock(None),
            read_var_mock(),
            write_file_mock(),
        ));

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_create_no_activate_no_name() {
        let mocked_deps = Unimock::new((
            args_mock("portman create --no-activate"),
            data_dir_mock(),
            read_file_mock(),
            read_var_mock(),
        ));

        let result = run(&mocked_deps);
        assert!(result.is_err());
    }

    #[test]
    fn test_edit_config() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
        ));

        assert!(run(&mocked_deps).is_ok());
    }

    #[test]
    fn test_edit_config_no_editor_env() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            read_file_mock(),
            dependencies::EnvironmentMock
                .each_call(matching!("PORTMAN_CONFIG"))
                .answers(|_| bail!("Failed"))
                .n_times(1),
            dependencies::EnvironmentMock
                .each_call(matching!("EDITOR"))
                .answers(|_| bail!("Failed"))
                .n_times(1),
        ));

        assert!(run(&mocked_deps).is_err());
    }

    #[test]
    fn test_edit_config_editor_exec_fails() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            dependencies::ExecMock
                .each_call(matching!((command, _) if command.get_program() == "editor"))
                .answers(|_| bail!("Failed"))
                .n_times(1),
            read_file_mock(),
            read_var_mock(),
        ));

        assert!(run(&mocked_deps).is_err());
    }

    #[test]
    fn test_edit_config_editor_command_fails() {
        let mocked_deps = Unimock::new((
            args_mock("portman config edit"),
            data_dir_mock(),
            dependencies::ExecMock
                .each_call(matching!((command, _) if command.get_program() == "editor"))
                .answers(|_| Ok((ExitStatusExt::from_raw(1), String::new())))
                .n_times(1),
            read_file_mock(),
            read_var_mock(),
        ));

        assert!(run(&mocked_deps).is_err());
    }
}
