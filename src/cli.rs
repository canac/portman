use clap::{Parser, Subcommand, ValueEnum};

#[derive(ValueEnum, Clone)]
pub enum InitShell {
    Fish,
}

#[derive(ValueEnum, Clone)]
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
        #[clap(value_enum)]
        shell: InitShell,
    },

    /// Manage the configuration
    #[clap(subcommand)]
    Config(Config),

    /// Print the port allocated for a project
    Get {
        /// The name of the project to look for (defaults to searching through projects using their configured matcher)
        project_name: Option<String>,
    },

    /// Allocate a port for a new project
    Allocate {
        /// The name of the project to allocate a port for (defaults to being provided by the matcher if not none)
        #[clap(required_if_eq("matcher", "none"))]
        project_name: Option<String>,

        /// Allocate a specific port to the project instead of randomly assigning one
        #[clap(long)]
        port: Option<u16>,

        /// The matching strategy to use when activating the project
        #[clap(long, value_enum, default_value = "dir")]
        matcher: Matcher,

        /// Navigate to the project via a redirect instead of reverse-proxy
        #[clap(long)]
        redirect: bool,
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
