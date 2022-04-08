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
        match std::fs::read_to_string(&path) {
            Ok(config_str) => Ok(Some(Self::from_toml(&config_str)?)),
            Err(io_err) => match io_err.kind() {
                // If the file doesn't exist, load the default config
                std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(ApplicationError::ReadConfig { path, io_err }),
            },
        }
    }

    // Return a new configuration from a TOML string
    fn from_toml(toml_str: &str) -> Result<Self, ApplicationError> {
        let config: Config =
            toml::from_str(toml_str).map_err(ApplicationError::DeserializeConfig)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_config() -> Result<(), ApplicationError> {
        let config = Config::from_toml("")?;
        assert_eq!(config.ranges, vec![(3000, 3999)]);
        assert_eq!(config.reserved, vec![]);
        Ok(())
    }

    #[test]
    fn test_default_config() -> Result<(), ApplicationError> {
        let config = Config::load(PathBuf::from("./default_config.toml"))?.unwrap();
        let default_config = Config::default();
        assert_eq!(config.ranges, default_config.ranges);
        assert_eq!(config.reserved, default_config.reserved);
        Ok(())
    }

    #[test]
    fn test_missing_config() -> Result<(), ApplicationError> {
        let config = Config::load(PathBuf::from("./missing_config.toml"))?;
        assert!(config.is_none());
        Ok(())
    }

    #[test]
    fn test_empty_ranges() {
        let result = Config::from_toml("ranges = []");
        assert!(matches!(result, Err(ApplicationError::ValidateConfig(_))));
    }

    #[test]
    fn test_inverted_ranges() {
        let result = Config::from_toml("ranges = [[3999, 3000]]");
        assert!(matches!(result, Err(ApplicationError::ValidateConfig(_))));
    }

    #[test]
    fn test_empty_range() {
        let result = Config::from_toml("ranges = [[3000, 3000]]");
        assert!(matches!(result, Err(ApplicationError::ValidateConfig(_))));
    }

    #[test]
    fn test_valid_ports() -> Result<(), ApplicationError> {
        let config = Config::from_toml(
            "ranges = [[3000, 3002], [4000, 4005]]\nreserved = [3001, 4000, 4004]",
        )?;
        assert_eq!(
            config.get_valid_ports().collect::<Vec<_>>(),
            vec![3000, 3002, 4001, 4002, 4003, 4005]
        );
        Ok(())
    }

    #[test]
    fn test_display() -> Result<(), ApplicationError> {
        let config = Config::from_toml(
            "ranges = [[3000, 3999], [4500, 4999]]\nreserved = [3000, 3100, 3200]",
        )?;
        assert_eq!(
            format!("{}", config),
            "Allowed port ranges: 3000-3999 & 4500-4999\nReserved ports: 3000, 3100, 3200"
        );
        Ok(())
    }
}
