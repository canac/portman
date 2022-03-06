mod cli;
mod error;
mod get;
mod init;
mod registry;

use crate::cli::{Cli, InitShell};
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
    }

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        println!("{}", err)
    }
}
