use crate::error::ApplicationError;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_ranges() -> Vec<(u16, u16)> {
    vec![(3000, 4000)]
}

#[derive(Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_ranges")]
    pub ranges: Vec<(u16, u16)>,

    #[serde(default)]
    pub reserved: Vec<u16>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ranges: default_ranges(),
            reserved: Default::default(),
        }
    }
}

impl Config {
    // Load the configuration from the file
    pub fn load() -> Result<Self, ApplicationError> {
        let config_path = Self::get_config_path()?;
        let config = match std::fs::read_to_string(&config_path) {
            Ok(config_str) => {
                toml::from_str(&config_str).map_err(ApplicationError::DeserializeConfig)
            }
            Err(io_err) => match io_err.kind() {
                // If the file doesn't exist, load the default config
                std::io::ErrorKind::NotFound => Ok(Self::default()),
                _ => Err(ApplicationError::ReadConfig {
                    path: config_path,
                    io_err,
                }),
            },
        }?;

        if config.ranges.is_empty() {
            return Err(ApplicationError::ValidateConfig(
                "port ranges must not be empty".to_string(),
            ));
        }
        for (start, end) in config.ranges.iter() {
            if start >= end {
                return Err(ApplicationError::ValidateConfig(format!(
                    "at port range ({:?}-{:?}), start must be less than range end",
                    start, end
                )));
            }
        }

        Ok(config)
    }

    // Return an iterator of the valid ports allowed by this configuration
    pub fn get_valid_ports(&self) -> impl Iterator<Item = u16> + '_ {
        self.ranges
            .iter()
            .flat_map(|(start, end)| (*start..*end))
            .filter(|port| !self.reserved.contains(port))
    }

    // Determine whether the port is a valid port for this configuration
    // To be valid, it must fall between one of the ranges and not be reserved
    pub fn is_port_valid(&self, port: u16) -> bool {
        self.get_valid_ports().any(|valid_port| valid_port == port)
    }

    // Return the path to the config file
    fn get_config_path() -> Result<PathBuf, ApplicationError> {
        let project_dirs =
            ProjectDirs::from("com", "canac", "portman").ok_or(ApplicationError::ProjectDirs)?;
        Ok(project_dirs.data_local_dir().join("config.toml"))
    }
}
