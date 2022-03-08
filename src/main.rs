mod cli;
mod config;
mod error;
mod init;
mod registry;

use crate::cli::{Cli, InitShell};
use crate::config::Config;
use crate::error::ApplicationError;
use crate::init::init_fish;
use crate::registry::PortRegistry;
use regex::Regex;
use structopt::StructOpt;

// Extract the name of the project using the git repo in the current directory
fn extract_project_name() -> Result<String, ApplicationError> {
    let stdout = std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .map_err(ApplicationError::Exec)?
        .stdout;
    let repo = std::str::from_utf8(&stdout)
        .map_err(ApplicationError::ReadGitStdout)?
        .trim_end();
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
    Ok(match cli_project_name {
        Some(project) => project,
        None => extract_project_name()?,
    })
}

fn run() -> Result<(), ApplicationError> {
    let config = Config::load()?;
    let mut registry = PortRegistry::load(&config)?;

    let cli = Cli::from_args();
    match cli {
        Cli::Init { shell } => match shell {
            InitShell::Fish => {
                println!("{}", init_fish())
            }
        },

        Cli::Get { project_name } => {
            let project = get_project_name(project_name)?;
            println!("{}", registry.get(project.as_str())?)
        }

        Cli::Allocate { project_name } => {
            let project = get_project_name(project_name.clone())?;
            let port = registry.allocate(project.as_str())?;
            println!("Allocated port {} for project {}", port, project);
            if project_name.is_none() {
                println!("\nThe PORT environment variable will now be automatically set whenever this git repo is cd-ed into from an initialized shell.\nRun `cd .` to manually set the PORT now.")
            }
        }

        Cli::Release { project_name } => {
            let project = get_project_name(project_name.clone())?;
            let port = registry.release(project.as_str())?;
            println!("Released port {} for project {}", port, project);
            if project_name.is_none() {
                println!("\nRun `cd .` to manually remove the PORT environment variable.")
            }
        }

        Cli::Reset => {
            registry.release_all()?;
            println!("All allocated ports have been released")
        }

        Cli::Caddyfile => {
            print!("{}", registry.caddyfile())
        }
    }

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}
