use clap::{Parser, Subcommand, ValueEnum};

#[derive(ValueEnum, Clone)]
#[cfg_attr(test, derive(Debug))]
pub enum InitShell {
    Fish,
}

#[derive(Subcommand)]
#[cfg_attr(test, derive(Debug))]
pub enum Config {
    /// Display the current configuration
    Show,

    /// Open the configuration file in $EDITOR
    Edit,
}

#[derive(Subcommand)]
#[cfg_attr(test, derive(Debug))]
pub enum Repo {
    /// Delete a repo and its associated port
    Delete {
        /// The repo to delete
        repo: String,
    },

    /// List all repos and their associated ports
    List,
}

#[derive(Parser)]
#[cfg_attr(test, derive(Debug))]
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

    /// Print a project's port
    Get {
        /// The name of the project to print (defaults to the active project)
        project_name: Option<String>,

        /// Print the project's name, directory, and linked port in addition to its port
        #[clap(long, short = 'e')]
        extended: bool,
    },

    /// Create a new project
    Create {
        /// The name of the project (defaults to the basename of the current directory unless --no-activate is present)
        project_name: Option<String>,

        /// Do not automatically activate this project
        #[clap(long, short = 'A', requires("project_name"))]
        no_activate: bool,

        /// Do not automatically link this project to a port based on its repo
        #[clap(long, short = 'N', conflicts_with("no_activate"))]
        no_link: bool,

        /// Modify the project if it already exists instead of failing
        #[clap(long, short = 'o')]
        overwrite: bool,
    },

    /// Delete an existing project
    Delete {
        /// The name of the project to delete (defaults to the active project)
        project_name: Option<String>,
    },

    /// Cleanup projects whose directory has been deleted
    Cleanup,

    /// List all projects
    List,

    /// Link a project to a port
    Link {
        /// The port to link (defaults to the port assigned to the active project's repo)
        port: Option<u16>,

        /// The name of the project to link (defaults to the active project)
        project_name: Option<String>,

        /// Do not remember which port the active project's repo was assigned to
        #[clap(long, short = 'S', requires("port"), conflicts_with("project_name"))]
        no_save: bool,
    },

    /// Unlink a port from a project
    Unlink {
        /// The port to unlink
        port: u16,
    },

    /// Manage repos
    #[clap(subcommand)]
    Repo(Repo),

    /// Print the generated Caddyfile
    Caddyfile,

    /// Regenerate the Caddyfile and restart caddy
    ReloadCaddy,
}
