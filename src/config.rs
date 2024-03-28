use crate::dependencies::ReadFile;
use crate::error::{ApplicationError, Result};
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::Path;

fn default_ranges() -> Vec<(u16, u16)> {
    vec![(3000, 3999)]
}

#[derive(Deserialize, Serialize)]
#[cfg_attr(test, derive(Debug))]
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
            reserved: vec![],
        }
    }
}

impl Config {
    // Load the configuration from the file
    // Return None if the file doesn't exist
    pub fn load(deps: &impl ReadFile, path: &Path) -> Result<Option<Self>> {
        deps.read_file(path)
            .map_err(ApplicationError::InvalidConfig)?
            .map(|config_str| Self::from_toml(&config_str))
            .transpose()
            .map_err(ApplicationError::InvalidConfig)
    }

    // Return a new configuration from a TOML string
    fn from_toml(toml_str: &str) -> anyhow::Result<Self> {
        let config: Config = toml::from_str(toml_str)?;

        if config.ranges.is_empty() {
            bail!("Validation error: port ranges must not be empty\n")
        }
        for (start, end) in &config.ranges {
            if start >= end {
                bail!("Validation error at port range ({start}-{end}), start must be less than range end\n")
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
                .map(|(start, end)| format!("{start}-{end}"))
                .collect::<Vec<_>>()
                .join(" & ")
        )?;

        if !self.reserved.is_empty() {
            write!(
                fmt,
                "\nReserved ports: {}",
                self.reserved
                    .iter()
                    .map(|port| format!("{port}"))
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
    use crate::dependencies;
    use std::{
        io::{Error, ErrorKind},
        path::PathBuf,
    };
    use unimock::{matching, MockFn, Unimock};

    #[test]
    fn test_load_config() {
        let deps = Unimock::new(
            dependencies::ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("config.toml")))
                .answers(&|_, _| Ok(String::from("ranges = [[3000, 3999]]\nreserved = []")))
                .once(),
        );

        let config = Config::load(&deps, &PathBuf::from("config.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(config.ranges, vec![(3000, 3999)]);
        assert_eq!(config.reserved, vec![]);
    }

    #[test]
    fn test_load_config_not_found() {
        let deps = Unimock::new(
            dependencies::ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("config.toml")))
                .answers(&|_, _| Err(Error::from(ErrorKind::NotFound)))
                .once(),
        );

        let config = Config::load(&deps, &PathBuf::from("config.toml")).unwrap();
        assert!(config.is_none());
    }

    #[test]
    fn test_load_config_not_readable() {
        let deps = Unimock::new(
            dependencies::ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("config.toml")))
                .answers(&|_, _| Err(Error::from(ErrorKind::PermissionDenied)))
                .once(),
        );

        let err = Config::load(&deps, &PathBuf::from("config.toml")).unwrap_err();
        assert!(matches!(err, ApplicationError::InvalidConfig(_)));
    }

    #[test]
    fn test_empty_config() {
        let config = Config::from_toml("").unwrap();
        assert_eq!(config.ranges, vec![(3000, 3999)]);
        assert_eq!(config.reserved, vec![]);
    }

    #[test]
    fn test_empty_ranges() {
        let result = Config::from_toml("ranges = []");
        assert!(result.is_err());
    }

    #[test]
    fn test_inverted_ranges() {
        let result = Config::from_toml("ranges = [[3999, 3000]]");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_range() {
        let result = Config::from_toml("ranges = [[3000, 3000]]");
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_ports() {
        let config = Config::from_toml(
            "ranges = [[3000, 3002], [4000, 4005]]\nreserved = [3001, 4000, 4004]",
        )
        .unwrap();
        assert_eq!(
            config.get_valid_ports().collect::<Vec<_>>(),
            vec![3000, 3002, 4001, 4002, 4003, 4005]
        );
    }

    #[test]
    fn test_display() {
        let config = Config::from_toml(
            "ranges = [[3000, 3999], [4500, 4999]]\nreserved = [3000, 3100, 3200]",
        )
        .unwrap();
        assert_eq!(
            format!("{config}"),
            "Allowed port ranges: 3000-3999 & 4500-4999\nReserved ports: 3000, 3100, 3200",
        );
    }

    #[test]
    fn test_display_none_reserved() {
        let config = Config::from_toml("ranges = [[3000, 3999], [4500, 4999]]").unwrap();
        assert_eq!(
            format!("{config}"),
            "Allowed port ranges: 3000-3999 & 4500-4999",
        );
    }
}
