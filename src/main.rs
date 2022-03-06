mod cli;
mod init;

use crate::cli::{Cli, InitShell};
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
    }
}
