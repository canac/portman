mod cli;
mod config;
mod error;
mod get;
mod init;
mod registry;

use crate::cli::{Cli, InitShell, Sync};
use crate::get::get_port;
use crate::init::init_fish;
use crate::registry::PortRegistry;
use error::ApplicationError;
use structopt::StructOpt;

fn run() -> Result<(), ApplicationError> {
    let cli = Cli::from_args();
    match cli {
        Cli::Init { shell } => match shell {
            InitShell::Fish => {
                println!("{}", init_fish());
            }
        },

        Cli::Get { project_name } => {
            println!("{}", get_port(&project_name)?);
        }

        Cli::Release { project_name } => {
            let mut registry = PortRegistry::load()?;
            if let Some(port) = registry.release(&project_name)? {
                println!("Removed project {} with port {}", project_name, port);
            } else {
                println!("Project {} does not exist", project_name);
                std::process::exit(1);
            }
        }

        Cli::Reset => {
            let empty_registry = PortRegistry::default();
            empty_registry.save()?;
        }

        Cli::Sync(subcommand) => {
            let working_dir =
                std::env::current_dir().map_err(|_| ApplicationError::CurrentDirectory)?;
            let mut registry = PortRegistry::load()?;
            match subcommand {
                Sync::Start => {
                    if registry.add_sync_dir(working_dir)? {
                        println!("Current directory is now being synced\n\nRun `cd .` to update the PORT environment variable.");
                        std::process::exit(0);
                    } else {
                        println!("Current directory was already being synced");
                        std::process::exit(1);
                    }
                }
                Sync::Stop => {
                    if registry.remove_sync_dir(&working_dir)? {
                        println!("Current directory is no longer being synced\n\nRun `cd .` to update the PORT environment variable.");
                        std::process::exit(0);
                    } else {
                        println!("Current directory was already not being synced");
                        std::process::exit(1);
                    }
                }
                Sync::Check => {
                    if registry.check_dir_synced(&working_dir) {
                        println!("Current directory is being synced");
                        std::process::exit(0);
                    } else {
                        println!("Current directory is not being synced");
                        std::process::exit(1);
                    }
                }
            }
        }

        Cli::Caddyfile => {
            let registry = PortRegistry::load()?;
            let caddyfile = registry
                .get_all()
                .iter()
                .map(|(project, port)| {
                    format!(
                        "{}.localhost {{\n\treverse_proxy 127.0.0.1:{}\n}}\n",
                        project, port
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            print!("{}", caddyfile);
        }
    }

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        println!("{}", err);
        std::process::exit(1);
    }
}
