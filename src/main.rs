#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod allocator;
mod caddy;
mod cli;
mod config;
mod dependencies;
mod init;
mod matcher;
mod registry;

use crate::allocator::PortAllocator;
use crate::caddy::{generate_caddyfile, reload};
use crate::cli::{Cli, Config as ConfigSubcommand, InitShell};
use crate::config::Config;
use crate::init::init_fish;
use crate::matcher::Matcher;
use crate::registry::PortRegistry;
use anyhow::{anyhow, bail, Result};
use clap::StructOpt;
use dependencies::{
    Args, ChoosePort, DataDir, Environment, Exec, ReadFile, WorkingDirectory, WriteFile,
};
use entrait::Impl;
use registry::Project;
use std::process::{self, Command};

fn allocate(
    deps: &(impl ChoosePort + DataDir + Environment + Exec + ReadFile + WriteFile + WorkingDirectory),
    registry: &mut PortRegistry,
    cli_name: Option<String>,
    cli_port: Option<u16>,
    cli_matcher: &cli::Matcher,
    cli_redirect: bool,
) -> Result<(String, Project)> {
    let matcher = match cli_matcher {
        cli::Matcher::Dir => Some(Matcher::from_cwd(deps)?),
        cli::Matcher::Git => Some(Matcher::from_git(deps)?),
        cli::Matcher::None => None,
    };
    let name = match cli_name {
        Some(cli_name) => cli_name,
        None => matcher.as_ref().unwrap().get_name()?,
    };
    Ok((
        name.clone(),
        registry.allocate(deps, name, cli_port, cli_redirect, matcher)?,
    ))
}

#[allow(clippy::too_many_lines)]
fn run(
    deps: &(impl Args
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
    let config_path = match config_env.clone() {
        Some(config_path) => std::path::PathBuf::from(config_path),
        None => data_dir.join("config.toml"),
    };
    let config = Config::load(deps, &config_path)?.unwrap_or_else(|| {
        if config_env.is_some() {
            println!("Warning: config file doesn't exist. Using default config.");
        }
        Config::default()
    });
    let port_allocator = PortAllocator::new(config.get_valid_ports());

    let cli = Cli::try_parse_from(deps.get_args());
    // Ignore errors caused by passing --help and --version
    if let Err(err) = cli.as_ref() {
        if matches!(
            err.kind(),
            clap::ErrorKind::DisplayHelp
                | clap::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                | clap::ErrorKind::DisplayVersion
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
                    "Config path: {}\nRegistry path: {}\nConfiguration:\n--------------\n{}",
                    config_path.to_string_lossy(),
                    data_dir
                        .join(std::path::PathBuf::from("registry.toml"))
                        .to_string_lossy(),
                    config
                );
            }
            ConfigSubcommand::Edit => {
                let editor = deps.read_var("EDITOR")?;
                println!(
                    "Opening \"{}\" with \"{}\"",
                    config_path.to_string_lossy(),
                    editor,
                );
                let (status, _) = deps.exec(Command::new(editor).arg(config_path))?;
                if !status.success() {
                    bail!("Editor command failed to execute successfully");
                }
            }
        },

        Cli::Get {
            project_name,
            allocate: cli_allocate,
            port,
            matcher,
            redirect,
        } => {
            let mut registry = PortRegistry::new(deps, port_allocator)?;
            let project = match project_name {
                Some(ref name) => registry.get(name),
                None => registry.match_cwd(deps).map(|(_, project)| project),
            };
            let port = if let Some(project) = project {
                project.port
            } else if cli_allocate {
                let (_, project) =
                    allocate(deps, &mut registry, project_name, port, &matcher, redirect)?;
                project.port
            } else {
                bail!("No projects match the current directory")
            };
            println!("{port}");
        }

        Cli::Allocate {
            project_name,
            port,
            matcher,
            redirect,
        } => {
            let mut registry = PortRegistry::new(deps, port_allocator)?;
            let (name, project) =
                allocate(deps, &mut registry, project_name, port, &matcher, redirect)?;
            println!("Allocated port {} for project {name}", project.port);
            if let Some(matcher) = project.matcher {
                let matcher_trigger = match matcher {
                    Matcher::GitRepository { .. } => "git repository",
                    Matcher::Directory { .. } => "directory",
                };
                println!("\nThe PORT environment variable will now be automatically set whenever this {matcher_trigger} is cd-ed into from an initialized shell.\nRun `cd .` to manually set the PORT now.");
            }
        }

        Cli::Release { project_name } => {
            let mut registry = PortRegistry::new(deps, port_allocator)?;
            let project_name = match project_name {
                Some(name) => name,
                None => registry
                    .match_cwd(deps)
                    .map(|(name, _)| name.clone())
                    .ok_or_else(|| anyhow!("No projects match the current directory"))?,
            };
            let project = registry.release(deps, &project_name)?;
            println!("Released port {} for project {project_name}", project.port);
            if project.matcher.is_some() {
                println!("\nRun `cd .` to manually remove the PORT environment variable.");
            }
        }

        Cli::Reset => {
            let mut registry = PortRegistry::new(deps, port_allocator)?;
            registry.release_all(deps)?;
            println!("All allocated ports have been released");
        }

        Cli::List => {
            let registry = PortRegistry::new(deps, port_allocator)?;
            for (name, project) in registry.iter() {
                println!(
                    "{} :{}{}",
                    name,
                    project.port,
                    project
                        .matcher
                        .as_ref()
                        .map(|matcher| format!(" (matches {matcher})"))
                        .unwrap_or_default()
                );
            }
        }

        Cli::Caddyfile => {
            let registry = PortRegistry::new(deps, port_allocator)?;
            print!("{}", generate_caddyfile(deps, &registry)?);
        }

        Cli::ReloadCaddy => {
            let registry = PortRegistry::new(deps, port_allocator)?;
            reload(deps, &registry)?;
            println!("caddy was successfully reloaded");
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
    use crate::dependencies::mocks::{data_dir_mock, exec_mock, write_file_mock};
    use std::{os::unix::process::ExitStatusExt, path::PathBuf};
    use unimock::{matching, Clause, MockFn};

    fn choose_port_mock() -> Clause {
        dependencies::choose_port::Fn
            .each_call(matching!(_))
            .answers(|_| Some(3000))
            .in_any_order()
    }

    fn cwd_mock() -> Clause {
        dependencies::get_cwd::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(PathBuf::from("/portman")))
            .in_any_order()
    }

    fn read_file_mock() -> Clause {
        dependencies::read_file::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(None))
            .in_any_order()
    }

    fn read_var_mock() -> Clause {
        dependencies::read_var::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(String::from("editor")))
            .in_any_order()
    }

    #[test]
    fn test_allocate() {
        let mocked_deps = unimock::mock([data_dir_mock(), read_file_mock()]);
        let config = Config::default();
        let allocator = PortAllocator::new(config.get_valid_ports());
        let mut registry = PortRegistry::new(&mocked_deps, allocator).unwrap();

        let mocked_deps = unimock::mock([
            data_dir_mock(),
            choose_port_mock(),
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            dependencies::write_file::Fn
                .each_call(matching!(_))
                .answers(|_| Ok(()))
                .in_any_order(),
        ]);
        let (name, project) = allocate(
            &mocked_deps,
            &mut registry,
            Some(String::from("project")),
            None,
            &cli::Matcher::None,
            false,
        )
        .unwrap();
        assert_eq!(name, String::from("project"));
        assert_eq!(
            project,
            Project {
                port: 3000,
                pinned: false,
                matcher: None,
                redirect: false,
            }
        );
    }

    #[test]
    fn test_cli_version() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![String::from("portman"), String::from("--version")])
                .in_any_order(),
            data_dir_mock(),
            read_file_mock(),
            read_var_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_allocate() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![String::from("portman"), String::from("allocate")])
                .in_any_order(),
            choose_port_mock(),
            cwd_mock(),
            data_dir_mock(),
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_allocate_dir_matcher() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![
                    String::from("portman"),
                    String::from("allocate"),
                    String::from("--matcher=dir"),
                ])
                .in_any_order(),
            choose_port_mock(),
            cwd_mock(),
            data_dir_mock(),
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_allocate_git_matcher() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![
                    String::from("portman"),
                    String::from("allocate"),
                    String::from("--matcher=git"),
                ])
                .in_any_order(),
            choose_port_mock(),
            data_dir_mock(),
            dependencies::exec::Fn
                .each_call(matching!(_))
                .answers(|_| {
                    Ok((
                        ExitStatusExt::from_raw(0),
                        String::from("https://github.com/user/project.git"),
                    ))
                })
                .in_any_order(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_allocate_none_matcher() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![
                    String::from("portman"),
                    String::from("allocate"),
                    String::from("--matcher=none"),
                ])
                .in_any_order(),
            data_dir_mock(),
            read_file_mock(),
            read_var_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_allocate_none_matcher_name() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![
                    String::from("portman"),
                    String::from("allocate"),
                    String::from("project"),
                    String::from("--matcher=none"),
                ])
                .in_any_order(),
            choose_port_mock(),
            data_dir_mock(),
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_edit_config() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![
                    String::from("portman"),
                    String::from("config"),
                    String::from("edit"),
                ])
                .in_any_order(),
            data_dir_mock(),
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_edit_config_no_editor_env() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![
                    String::from("portman"),
                    String::from("config"),
                    String::from("edit"),
                ])
                .in_any_order(),
            data_dir_mock(),
            read_file_mock(),
            dependencies::read_var::Fn
                .each_call(matching!(_))
                .answers(|_| bail!("Failed"))
                .in_any_order(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_err());
    }

    #[test]
    fn test_edit_config_editor_exec_fails() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![
                    String::from("portman"),
                    String::from("config"),
                    String::from("edit"),
                ])
                .in_any_order(),
            data_dir_mock(),
            dependencies::exec::Fn
                .each_call(matching!(_))
                .answers(|_| bail!("Failed"))
                .in_any_order(),
            read_file_mock(),
            read_var_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_err());
    }

    #[test]
    fn test_edit_config_editor_command_fails() {
        let mocked_deps = unimock::mock([
            dependencies::get_args::Fn
                .each_call(matching!())
                .returns(vec![
                    String::from("portman"),
                    String::from("config"),
                    String::from("edit"),
                ])
                .in_any_order(),
            data_dir_mock(),
            dependencies::exec::Fn
                .each_call(matching!(_))
                .answers(|_| Ok((ExitStatusExt::from_raw(1), String::new())))
                .in_any_order(),
            read_file_mock(),
            read_var_mock(),
        ]);

        let result = run(&mocked_deps);
        assert!(result.is_err());
    }
}
