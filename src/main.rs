mod allocator;
mod cli;
mod config;
mod error;
mod init;
mod registry;
mod registry_store;

use crate::allocator::{PortAllocator, RandomPortChooser};
use crate::cli::{Cli, Config as ConfigSubcommand, InitShell};
use crate::config::Config;
use crate::error::ApplicationError;
use crate::init::init_fish;
use crate::registry::PortRegistry;
use crate::registry_store::RegistryFileStore;
use clap::StructOpt;
use regex::Regex;
use std::process;

// Extract the name of a project from it's git repo URL
fn project_name_from_repo(repo: &str) -> Result<String, ApplicationError> {
    lazy_static::lazy_static! {
        static ref RE: Regex =
            Regex::new(r"^https://github\.com/(?:.+)/(?P<project>.+?)(?:\.git)?$").unwrap();
    }
    let cap = RE.captures(repo).ok_or(ApplicationError::ExtractProject)?;
    Ok(cap
        .name("project")
        .ok_or(ApplicationError::ExtractProject)?
        .as_str()
        .to_string())
}

// Get the project name using the name provided on the cli if present,
// defaulting to extracting it from the git repo in the current directory
fn get_project_name(cli_project_name: Option<String>) -> Result<String, ApplicationError> {
    match cli_project_name {
        Some(project) => Ok(project),
        None => {
            let stdout = process::Command::new("git")
                .args(["config", "--get", "remote.origin.url"])
                .output()
                .map_err(ApplicationError::Exec)?
                .stdout;
            let repo = std::str::from_utf8(&stdout)
                .map_err(ApplicationError::ReadGitStdout)?
                .trim_end();
            project_name_from_repo(repo)
        }
    }
}

fn run() -> Result<(), ApplicationError> {
    let project_dirs = directories::ProjectDirs::from("com", "canac", "portman")
        .ok_or(ApplicationError::ProjectDirs)?;
    let data_dir = project_dirs.data_local_dir();
    let registry_path = data_dir.join("registry.toml");
    let registry_store = RegistryFileStore::new(registry_path.clone());
    let config_env = std::env::var_os("PORTMAN_CONFIG");
    let config_path = match config_env.clone() {
        Some(config_path) => std::path::PathBuf::from(config_path),
        None => data_dir.join("config.toml"),
    };
    let config = Config::load(config_path.clone())?.unwrap_or_else(|| {
        if config_env.is_some() {
            println!("Warning: config file doesn't exist. Using default config.");
        }
        Config::default()
    });
    let port_allocator = PortAllocator::new(config.get_valid_ports(), RandomPortChooser::new());

    let cli = Cli::parse();
    match cli {
        Cli::Init { shell } => match shell {
            InitShell::Fish => {
                println!("{}", init_fish())
            }
        },

        Cli::Config(subcommand) => match subcommand {
            ConfigSubcommand::Show => {
                println!(
                    "Config path: {}\nRegistry path: {}\nConfiguration:\n--------------\n{}",
                    config_path.to_string_lossy(),
                    registry_path.to_string_lossy(),
                    config
                )
            }
            ConfigSubcommand::Edit => {
                let var_name = std::ffi::OsString::from("EDITOR");
                let editor = std::env::var(var_name.clone()).map_err(|var_err| {
                    ApplicationError::ReadEnv {
                        name: var_name,
                        var_err,
                    }
                })?;
                println!(
                    "Opening \"{}\" with \"{}\"",
                    config_path.to_string_lossy(),
                    editor,
                );
                let status = process::Command::new(editor)
                    .arg(config_path)
                    .status()
                    .map_err(ApplicationError::Exec)?;
                process::exit(status.code().unwrap_or(1))
            }
        },

        Cli::Get {
            project_name,
            allocate,
        } => {
            let mut registry = PortRegistry::new(registry_store, port_allocator)?;
            let project = get_project_name(project_name)?;
            let port = match registry.get(project.as_str()) {
                Some(port) => Ok(port),
                None => {
                    if allocate {
                        registry.allocate(project.as_str())
                    } else {
                        Err(ApplicationError::NonExistentProject(project))
                    }
                }
            }?;
            println!("{}", port)
        }

        Cli::Allocate { project_name } => {
            let mut registry = PortRegistry::new(registry_store, port_allocator)?;
            let project = get_project_name(project_name.clone())?;
            let port = registry.allocate(project.as_str())?;
            println!("Allocated port {} for project {}", port, project);
            if project_name.is_none() {
                println!("\nThe PORT environment variable will now be automatically set whenever this git repo is cd-ed into from an initialized shell.\nRun `cd .` to manually set the PORT now.")
            }
        }

        Cli::Release { project_name } => {
            let mut registry = PortRegistry::new(registry_store, port_allocator)?;
            let project = get_project_name(project_name.clone())?;
            let port = registry.release(project.as_str())?;
            println!("Released port {} for project {}", port, project);
            if project_name.is_none() {
                println!("\nRun `cd .` to manually remove the PORT environment variable.")
            }
        }

        Cli::Reset => {
            let mut registry = PortRegistry::new(registry_store, port_allocator)?;
            registry.release_all()?;
            println!("All allocated ports have been released")
        }

        Cli::List => {
            let registry = PortRegistry::new(registry_store, port_allocator)?;
            for (project, port) in registry.iter() {
                println!("{} :{}", project, port);
            }
        }

        Cli::Caddyfile => {
            let registry = PortRegistry::new(registry_store, port_allocator)?;
            print!("{}", registry.caddyfile())
        }

        Cli::ReloadCaddy => {
            let registry = PortRegistry::new(registry_store, port_allocator)?;
            registry.reload_caddy()?;
            println!("caddy was successfully reloaded")
        }
    }

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{}", err);
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_basic_project_name() -> Result<(), ApplicationError> {
        assert_eq!(
            project_name_from_repo("https://github.com/user/project-name")?,
            "project-name".to_string()
        );
        Ok(())
    }

    #[test]
    fn get_project_name_with_extension() -> Result<(), ApplicationError> {
        assert_eq!(
            project_name_from_repo("https://github.com/user/project-name.git")?,
            "project-name".to_string()
        );
        Ok(())
    }

    #[test]
    fn get_project_name_invalid() -> Result<(), ApplicationError> {
        assert!(matches!(
            project_name_from_repo("https://gitlab.com/user/project-name"),
            Err(ApplicationError::ExtractProject)
        ));
        Ok(())
    }

    #[test]
    fn get_portman_project_name() -> Result<(), ApplicationError> {
        assert_eq!(get_project_name(None)?, "portman".to_string());
        Ok(())
    }
}
