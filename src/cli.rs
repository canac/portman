use structopt::StructOpt;
use strum::{EnumString, EnumVariantNames, VariantNames};

#[derive(EnumString, EnumVariantNames)]
#[strum(serialize_all = "kebab_case")]
pub enum InitShell {
    Fish,
}

#[derive(StructOpt)]
pub enum Sync {
    #[structopt(about = "Start syncing the current directory")]
    Start,

    #[structopt(about = "Stop syncing the current directory")]
    Stop,

    #[structopt(about = "Check whether the current directory is being synced")]
    Check,
}

#[derive(StructOpt)]
#[structopt(
    name = "portman",
    about = "Manage local port assignments",
    version = "0.1.0",
    author = "Caleb Cox"
)]
pub enum Cli {
    #[structopt(about = "Print the shell configuration command to initialize portman")]
    Init {
        #[structopt(
            possible_values = InitShell::VARIANTS,
            about = "Specifies the shell to use"
        )]
        shell: InitShell,
    },

    #[structopt(about = "Print the port for a project")]
    Get {
        #[structopt(about = "The name of the project to get a port for")]
        project_name: String,
    },

    #[structopt(about = "Release an assigned port")]
    Release {
        #[structopt(about = "The name of the project to release")]
        project_name: String,
    },

    #[structopt(about = "Reset all of the port assignments")]
    Reset,

    #[structopt(about = "Manage synced directories")]
    Sync(Sync),

    #[structopt(about = "Print the generated Caddyfile for the assigned ports")]
    Caddyfile,
}
