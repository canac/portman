use clap::{ArgEnum, Parser, Subcommand};

#[derive(ArgEnum, Clone)]
pub enum InitShell {
    Fish,
}

#[derive(ArgEnum, Clone)]
pub enum Matcher {
    Dir,
    Git,
    None,
}

#[derive(Subcommand)]
pub enum Config {
    /// Display the current configuration
    Show,

    /// Open the configuration file in $EDITOR
    Edit,
}

#[derive(Parser)]
#[clap(about, version, author)]
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
        /// The name of the project to look for (defaults to searching through projects using their configured matcher)
        #[clap(required_if_eq("matcher", "none"))]
        project_name: Option<String>,

        /// Allocate a new port for the project if one isn't allocated yet
        #[clap(long)]
        allocate: bool,

        /// If allocating a project, the matching strategy to use when activating the project
        #[clap(long, arg_enum, default_value = "dir", requires = "allocate")]
        matcher: Matcher,
    },

    /// Allocate a port for a new project
    Allocate {
        /// The name of the project to allocate a port for (defaults to being provided by the matcher if not none)
        #[clap(required_if_eq("matcher", "none"))]
        project_name: Option<String>,

        /// The matching strategy to use when activating the project
        #[clap(long, arg_enum, default_value = "dir")]
        matcher: Matcher,
    },

    /// Release an allocated port
    Release {
        /// The name of the project to release (defaults to searching through projects using their configured matcher)
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
