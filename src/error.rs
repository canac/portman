use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("Couldn't determine application directories")]
    ProjectDirs,

    #[error("Couldn't determine current directory")]
    CurrentDirectory,

    #[error("Couldn't read port registry file \"{path}\"\nError: {io_err}")]
    ReadRegistry {
        path: PathBuf,
        io_err: std::io::Error,
    },

    #[error("Couldn't serialize port registry\nError: {0}")]
    SerializeRegistry(#[from] toml::ser::Error),

    #[error("Couldn't write port registry file \"{0}\"")]
    WriteRegistry(PathBuf),

    #[error("Couldn't deserialize port registry\nError: {0}")]
    DeserializeRegistry(#[from] toml::de::Error),
}
