use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("Couldn't determine application directories")]
    ProjectDirs,

    #[error("Couldn't read port registry file \"{path}\"\nError: {io_err}")]
    ReadRegistry {
        path: PathBuf,
        io_err: std::io::Error,
    },

    #[error("Couldn't deserialize port registry\nError: {0}")]
    DeserializeRegistry(toml::de::Error),

    #[error("Couldn't serialize port registry\nError: {0}")]
    SerializeRegistry(toml::ser::Error),

    #[error("Couldn't write port registry file \"{0}\"")]
    WriteRegistry(PathBuf),

    #[error("Couldn't read config file \"{path}\"\nError: {io_err}")]
    ReadConfig {
        path: PathBuf,
        io_err: std::io::Error,
    },

    #[error("Couldn't deserialize config file\nError: {0}")]
    DeserializeConfig(toml::de::Error),

    #[error("Config validation error: {0}")]
    ValidateConfig(String),

    #[error("Project \"{0}\" already exists")]
    DuplicateProject(String),

    #[error("Project \"{0}\" does not exist")]
    NonExistentProject(String),

    #[error("All available ports have been allocated already")]
    AllPortsAllocated,

    #[error("Error executing git command\nError: {0}")]
    ExecGit(std::io::Error),

    #[error("Error reading git output\nError: {0}")]
    ReadGitStdout(std::str::Utf8Error),

    #[error("Error extracting GitHub project name")]
    ExtractProject,
}
