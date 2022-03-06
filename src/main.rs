mod cli;
mod error;
mod get;
mod init;
mod registry;

use crate::cli::{Cli, InitShell};
use crate::get::get_port;
use crate::init::init_fish;
use error::ApplicationError;
use structopt::StructOpt;

fn run() -> Result<(), ApplicationError> {
    let cli = Cli::from_args();
    match cli {
        Cli::Init { shell } => match shell {
            InitShell::Fish => {
                println!("{}", init_fish())
            }
        },

        Cli::Get { project_name } => {
            println!("{}", get_port(project_name)?);
        }
    }

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        println!("{}", err)
    }
}
