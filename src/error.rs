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

    #[error("Couldn't serialize port registry")]
    SerializeRegistry,

    #[error("Couldn't write port registry file \"{0}\"")]
    WriteRegistry(PathBuf),

    #[error("Couldn't deserialize port registry")]
    DeserializeRegistry,
}
