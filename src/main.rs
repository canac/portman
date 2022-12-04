mod allocator;
mod cli;
mod config;
mod dependencies;
mod init;
mod matcher;
mod registry;

use crate::allocator::PortAllocator;
use crate::cli::{Cli, Config as ConfigSubcommand, InitShell};
use crate::config::Config;
use crate::init::init_fish;
use crate::matcher::Matcher;
use crate::registry::PortRegistry;
use anyhow::{anyhow, bail, Context, Result};
use clap::StructOpt;
use dependencies::{Args, ChoosePort, Environment, Exec, ReadFile, WorkingDirectory, WriteFile};
use entrait::Impl;
use registry::Project;
use std::process::{self, Command};

fn allocate(
    deps: &(impl ChoosePort + Environment + Exec + ReadFile + WriteFile + WorkingDirectory),
    registry: &mut PortRegistry,
    cli_name: Option<String>,
    cli_port: Option<u16>,
    cli_matcher: &cli::Matcher,
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
        registry.allocate(deps, name, cli_port, matcher)?,
    ))
}

fn run(
    deps: &(impl Args + ChoosePort + Environment + Exec + ReadFile + WriteFile + WorkingDirectory),
) -> Result<()> {
    let project_dirs = directories::ProjectDirs::from("com", "canac", "portman")
        .context("Failed to determine application directories")?;
    let data_dir = project_dirs.data_local_dir();
    let registry_path = data_dir.join("registry.toml");
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

    let cli = Cli::try_parse_from(deps.get_args())?;
    match cli {
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
                    registry_path.to_string_lossy(),
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
        } => {
            let mut registry = PortRegistry::new(deps, registry_path, port_allocator)?;
            let project = match project_name {
                Some(ref name) => registry.get(name),
                None => registry.match_cwd(deps).map(|(_, project)| project),
            };
            let port = if let Some(project) = project {
                project.port
            } else if cli_allocate {
                let (_, project) = allocate(deps, &mut registry, project_name, port, &matcher)?;
                project.port
            } else {
                bail!("No projects match the current directory")
            };
            println!("{}", port);
        }

        Cli::Allocate {
            project_name,
            port,
            matcher,
        } => {
            let mut registry = PortRegistry::new(deps, registry_path, port_allocator)?;
            let (name, project) = allocate(deps, &mut registry, project_name, port, &matcher)?;
            println!("Allocated port {} for project {}", project.port, name);
            if let Some(matcher) = project.matcher {
                let matcher_trigger = match matcher {
                    Matcher::GitRepository { .. } => "git repository",
                    Matcher::Directory { .. } => "directory",
                };
                println!("\nThe PORT environment variable will now be automatically set whenever this {matcher_trigger} is cd-ed into from an initialized shell.\nRun `cd .` to manually set the PORT now.");
            }
        }

        Cli::Release { project_name } => {
            let mut registry = PortRegistry::new(deps, registry_path, port_allocator)?;
            let project_name = match project_name {
                Some(name) => name,
                None => registry
                    .match_cwd(deps)
                    .map(|(name, _)| name.clone())
                    .ok_or_else(|| anyhow!("No projects match the current directory"))?,
            };
            let project = registry.release(deps, &project_name)?;
            println!(
                "Released port {} for project {}",
                project.port, project_name
            );
            if project.matcher.is_some() {
                println!("\nRun `cd .` to manually remove the PORT environment variable.");
            }
        }

        Cli::Reset => {
            let mut registry = PortRegistry::new(deps, registry_path, port_allocator)?;
            registry.release_all(deps)?;
            println!("All allocated ports have been released");
        }

        Cli::List => {
            let registry = PortRegistry::new(deps, registry_path, port_allocator)?;
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
            let registry = PortRegistry::new(deps, registry_path, port_allocator)?;
            print!("{}", registry.caddyfile());
        }

        Cli::ReloadCaddy => {
            let registry = PortRegistry::new(deps, registry_path, port_allocator)?;
            registry.reload_caddy(deps)?;
            println!("caddy was successfully reloaded");
        }
    }

    Ok(())
}

fn main() {
    let deps = Impl::new(());
    if let Err(err) = run(&deps) {
        eprintln!("{}", err);
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn exec_mock() -> Clause {
        dependencies::exec::Fn
            .each_call(matching!(_))
            .answers(|_| Ok((ExitStatusExt::from_raw(0), String::new())))
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

    fn write_file_mock() -> Clause {
        dependencies::write_file::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(()))
            .in_any_order()
    }

    #[test]
    fn test_allocate() {
        let mocked_deps = unimock::mock([read_file_mock()]);
        let config = Config::default();
        let allocator = PortAllocator::new(config.get_valid_ports());
        let mut registry =
            PortRegistry::new(&mocked_deps, PathBuf::from("registry.toml"), allocator).unwrap();

        let mocked_deps = unimock::mock([
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
        )
        .unwrap();
        assert_eq!(name, String::from("project"));
        assert_eq!(
            project,
            Project {
                port: 3000,
                pinned: false,
                matcher: None
            }
        );
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
