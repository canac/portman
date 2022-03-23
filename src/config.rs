use crate::error::ApplicationError;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Display, Formatter},
    path::PathBuf,
};

fn default_ranges() -> Vec<(u16, u16)> {
    vec![(3000, 3999)]
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
    // Return None if the file doesn't exist
    pub fn load(path: PathBuf) -> Result<Option<Self>, ApplicationError> {
        let config: Config = match std::fs::read_to_string(&path) {
            Ok(config_str) => {
                toml::from_str(&config_str).map_err(ApplicationError::DeserializeConfig)
            }
            Err(io_err) => match io_err.kind() {
                // If the file doesn't exist, load the default config
                std::io::ErrorKind::NotFound => return Ok(None),
                _ => Err(ApplicationError::ReadConfig { path, io_err }),
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

        Ok(Some(config))
    }

    // Return an iterator of the valid ports allowed by this configuration
    pub fn get_valid_ports(&self) -> impl Iterator<Item = u16> + '_ {
        self.ranges
            .iter()
            .flat_map(|(start, end)| (*start..=*end))
            .filter(|port| !self.reserved.contains(port))
    }
}

impl Display for Config {
    fn fmt(&self, fmt: &mut Formatter) -> std::fmt::Result {
        write!(
            fmt,
            "Allowed port ranges: {}",
            self.ranges
                .iter()
                .map(|(start, end)| format!("{}-{}", start, end))
                .collect::<Vec<_>>()
                .join(" & ")
        )?;

        if !self.reserved.is_empty() {
            write!(
                fmt,
                "\nReserved ports: {}",
                self.reserved
                    .iter()
                    .map(|port| format!("{}", port))
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }

        Ok(())
    }
}
