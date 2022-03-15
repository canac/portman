use structopt::StructOpt;
use strum::{EnumString, EnumVariantNames, VariantNames};

#[derive(EnumString, EnumVariantNames)]
#[strum(serialize_all = "kebab_case")]
pub enum InitShell {
    Fish,
}

#[derive(StructOpt)]
#[structopt(
    name = "portman",
    about = "Manage local port allocations",
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

    #[structopt(about = "Display the current configuration")]
    Config,

    #[structopt(about = "Print the port allocated for a project")]
    Get {
        #[structopt(
            about = "The name of the project to get a port for (defaults to the current git project name)"
        )]
        project_name: Option<String>,

        #[structopt(
            long,
            about = "Allocate a new port for the project if one isn't allocated yet"
        )]
        allocate: bool,
    },

    #[structopt(about = "Allocate a port for a new project")]
    Allocate {
        #[structopt(
            about = "The name of the project to allocate a port for (defaults to the current git project name)"
        )]
        project_name: Option<String>,
    },

    #[structopt(about = "Release an allocated port")]
    Release {
        #[structopt(
            about = "The name of the project to release (defaults to the current git project name)"
        )]
        project_name: Option<String>,
    },

    #[structopt(about = "Reset all of the port assignments")]
    Reset,

    #[structopt(about = "Print the generated Caddyfile for the allocated ports")]
    Caddyfile,
}
