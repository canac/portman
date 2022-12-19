use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Display, Formatter},
    path::Path,
};

use crate::dependencies::ReadFile;

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
            reserved: vec![],
        }
    }
}

impl Config {
    // Load the configuration from the file
    // Return None if the file doesn't exist
    pub fn load(deps: &impl ReadFile, path: &Path) -> Result<Option<Self>> {
        deps.read_file(path)
            .with_context(|| format!("Failed to read config at \"{}\"", path.to_string_lossy()))?
            .map(|config_str| Self::from_toml(&config_str))
            .transpose()
            .with_context(|| {
                format!(
                    "Failed to deserialize config at \"{}\"",
                    path.to_string_lossy()
                )
            })
    }

    // Return a new configuration from a TOML string
    fn from_toml(toml_str: &str) -> Result<Self> {
        let config: Config = toml::from_str(toml_str).context("Failed to deserialize config")?;

        if config.ranges.is_empty() {
            bail!("Failed to validate config: port ranges must not be empty")
        }
        for (start, end) in &config.ranges {
            if start >= end {
                bail!("Failed to validate config: at port range ({start}-{end}), start must be less than range end")
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
    use std::path::PathBuf;
    use unimock::{matching, MockFn};

    #[test]
    fn test_load_config() -> Result<()> {
        let deps = unimock::mock([dependencies::read_file::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(Some(String::from("ranges = [[3000, 3999]]\nreserved = []"))))
            .in_any_order()]);

        let config = Config::load(&deps, &PathBuf::new())?.unwrap();
        assert_eq!(config.ranges, vec![(3000, 3999)]);
        assert_eq!(config.reserved, vec![]);
        Ok(())
    }

    #[test]
    fn test_load_missing_config() {
        let deps = unimock::mock([dependencies::read_file::Fn
            .each_call(matching!(_))
            .answers(|_| bail!("Read error"))
            .in_any_order()]);

        let config = Config::load(&deps, &PathBuf::new());
        assert!(config.is_err());
    }

    #[test]
    fn test_empty_config() -> Result<()> {
        let config = Config::from_toml("")?;
        assert_eq!(config.ranges, vec![(3000, 3999)]);
        assert_eq!(config.reserved, vec![]);
        Ok(())
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
    fn test_valid_ports() -> Result<()> {
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
    fn test_display() -> Result<()> {
        let config = Config::from_toml(
            "ranges = [[3000, 3999], [4500, 4999]]\nreserved = [3000, 3100, 3200]",
        )?;
        assert_eq!(
            format!("{config}"),
            "Allowed port ranges: 3000-3999 & 4500-4999\nReserved ports: 3000, 3100, 3200"
        );
        Ok(())
    }
}
