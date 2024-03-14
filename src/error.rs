use std::{ffi::OsString, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaddyError {
    #[error(transparent)]
    Exec(#[from] ExecError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type CaddyResult<T> = std::result::Result<T, CaddyError>;

#[derive(Debug, Error)]
pub enum ExecError {
    #[error("Command \"{}\" failed with output:\n{output}", command.to_string_lossy())]
    Failed { command: OsString, output: String },

    #[error("Command \"{}\" failed:\n{io_err}", command.to_string_lossy())]
    IO {
        command: OsString,
        io_err: std::io::Error,
    },
}

pub type ExecResult<T> = std::result::Result<T, ExecError>;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("Error reloading caddy:\n{0}")]
    Caddy(CaddyError),

    #[error("Project {0} already uses the directory \"{1}\"")]
    DuplicateDirectory(String, PathBuf),

    #[error("A project already has the name {0}")]
    DuplicateProject(String),

    #[error("Editor command could not be run:\n\n{0}")]
    EditorCommand(ExecError),

    #[error("All available ports have been allocated already")]
    EmptyAllocator,

    #[error("Git command could not be run:\n\n{0}")]
    GitCommand(ExecError),

    #[error("Configuration is invalid:\n\n{0}")]
    InvalidConfig(anyhow::Error),

    #[error("Project name \"{0}\" is invalid: {1}")]
    InvalidProjectName(String, &'static str),

    #[error("Custom config file at \"{0}\" does not exist")]
    MissingCustomConfig(PathBuf),

    #[error("The current directory does not contain a project")]
    NoActiveProject,

    #[error("Project {0} does not exist")]
    NonExistentProject(String),

    #[error("Repo {0} does not exist")]
    NonExistentRepo(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, ApplicationError>;
