use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("Error reloading caddy:\n\n{0:?}")]
    Caddy(anyhow::Error),

    #[error("Project {0} already uses the directory \"{1}\"")]
    DuplicateDirectory(String, PathBuf),

    #[error("A project already has the name {0}")]
    DuplicateProject(String),

    #[error("Editor command could not be run:\n\n{0:?}")]
    EditorCommand(anyhow::Error),

    #[error("All available ports have been allocated already")]
    EmptyAllocator,

    #[error("Configuration is invalid:\n\n{0:?}")]
    InvalidConfig(anyhow::Error),

    #[error("Project name \"{0}\" is invalid: {1}")]
    InvalidProjectName(String, &'static str),

    #[error("Custom config file at \"{0}\" does not exist")]
    MissingCustomConfig(PathBuf),

    #[error("The current directory does not contain a project")]
    NoActiveProject,

    #[error("Project {0} does not exist")]
    NonExistentProject(String),

    #[error("{0:?}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, ApplicationError>;
