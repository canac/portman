mod cli;
mod get;
mod init;
mod registry;

use crate::cli::{Cli, InitShell};
use crate::get::get_port;
use crate::init::init_fish;
use structopt::StructOpt;

fn main() {
    let cli = Cli::from_args();
    match cli {
        Cli::Init { shell } => match shell {
            InitShell::Fish => {
                println!("{}", init_fish())
            }
        },

        Cli::Get { project_name } => {
            println!("{}", get_port(project_name));
        }
    }
}
