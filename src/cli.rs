use clap::{ArgEnum, Parser, Subcommand};

#[derive(ArgEnum, Clone)]
pub enum InitShell {
    Fish,
}

#[derive(Subcommand)]
pub enum Config {
    /// Display the current configuration
    Show,

    /// Open the configuration file in $EDITOR
    Edit,
}

#[derive(Parser)]
#[clap(
    name = env!("CARGO_PKG_NAME"),
    about = env!("CARGO_PKG_DESCRIPTION"),
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS")
)]
pub enum Cli {
    /// Print the shell configuration command to initialize portman
    Init {
        /// Specifies the shell to use
        #[clap(arg_enum)]
        shell: InitShell,
    },

    /// Manage the configuration
    #[clap(subcommand)]
    Config(Config),

    /// Print the port allocated for a project
    Get {
        /// The name of the project to get a port for (defaults to the current git project name)
        project_name: Option<String>,

        /// Allocate a new port for the project if one isn't allocated yet
        #[clap(long)]
        allocate: bool,
    },

    /// Allocate a port for a new project
    Allocate {
        /// The name of the project to allocate a port for (defaults to the current git project name)
        project_name: Option<String>,
    },

    /// Release an allocated port
    Release {
        /// The name of the project to release (defaults to the current git project name)
        project_name: Option<String>,
    },

    /// Reset all of the port assignments
    Reset,

    /// List all of the port assignments
    List,

    /// Print the generated Caddyfile for the allocated ports
    Caddyfile,

    /// Regenerate the Caddyfile and restart caddy
    ReloadCaddy,
}
