use crate::allocator::PortAllocator;
use crate::error::ApplicationError;
use crate::registry_store::RegistryStore;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

// The port registry data that will be serialized and deserialized in the database
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct RegistryData {
    pub ports: BTreeMap<String, u16>,
}

pub struct PortRegistry {
    store: Box<dyn RegistryStore>,
    ports: BTreeMap<String, u16>,
    allocator: PortAllocator,
}

impl PortRegistry {
    // Create a new port registry
    pub fn new(
        registry_store: impl RegistryStore + 'static,
        port_allocator: PortAllocator,
    ) -> Result<Self, ApplicationError> {
        let registry_data = registry_store.load()?;

        // Validate all ports in the registry against the required config and
        // regenerate invalid ones as necessary
        let mut changed = false;
        let mut allocator = port_allocator;
        let validated_ports = registry_data
            .ports
            .into_iter()
            .map(|(project, old_port)| {
                allocator.allocate(Some(old_port)).map(|port| {
                    if port != old_port {
                        changed = true;
                    }
                    (project, port)
                })
            })
            .collect::<Option<BTreeMap<_, _>>>()
            .ok_or(ApplicationError::AllPortsAllocated)?;
        let registry = Self {
            store: Box::new(registry_store),
            ports: validated_ports,
            allocator,
        };
        if changed {
            registry.save()?;
        }
        Ok(registry)
    }

    // Save a port registry to the file
    pub fn save(&self) -> Result<(), ApplicationError> {
        self.store.save(&RegistryData {
            ports: self.ports.clone(),
        })?;
        if let Err(err) = self.reload_caddy() {
            // An error reloading Caddy is just a warning, not a fatal error
            println!("Warning: couldn't reload Caddy config.\n\n{err}");
        }
        Ok(())
    }

    // Get a project's port from the registry
    pub fn get(&self, project: &str) -> Option<u16> {
        self.ports.get(project).cloned()
    }

    // Allocate a port to a new project
    pub fn allocate(&mut self, project: &str) -> Result<u16, ApplicationError> {
        if self.ports.get(project).is_some() {
            Err(ApplicationError::DuplicateProject(project.to_string()))
        } else {
            let new_port = self
                .allocator
                .allocate(None)
                .ok_or_else(|| ApplicationError::NonExistentProject(project.to_string()))?;
            self.ports.insert(project.to_string(), new_port);
            self.save()?;
            Ok(new_port)
        }
    }

    // Release a previously allocated project's port
    pub fn release(&mut self, project: &str) -> Result<u16, ApplicationError> {
        match self.ports.remove(project) {
            Some(port) => {
                self.save()?;
                Ok(port)
            }
            None => Err(ApplicationError::NonExistentProject(project.to_string())),
        }
    }

    // Release all previously allocated projects
    pub fn release_all(&mut self) -> Result<(), ApplicationError> {
        self.ports = BTreeMap::new();
        self.save()
    }

    // Iterate over all port assignments
    pub fn iter(&self) -> impl Iterator<Item = (&String, &u16)> {
        self.ports.iter()
    }

    // Return the generated Caddyfile
    pub fn caddyfile(&self) -> String {
        let caddyfile = self
            .ports
            .iter()
            .map(|(project, port)| {
                format!(
                    "{}.localhost {{\n\treverse_proxy 127.0.0.1:{}\n}}\n",
                    project, port
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("# portman begin\n# WARNING: This section is automatically generated by portman. Any manual edits will be overridden.\n\n{caddyfile}# portman end\n")
    }

    // Merge the registry's caddyfile into the existing caddyfile
    fn merge_caddyfile(
        &self,
        existing_caddyfile: Option<String>,
    ) -> Result<String, ApplicationError> {
        // Merge the portman caddyfile section into the existing caddyfile
        let portman_caddyfile = self.caddyfile();
        lazy_static::lazy_static! {
            static ref RE: Regex =
            Regex::new(r"# portman begin\n[\s\S+]*# portman end\n").unwrap();
        }
        let merged_caddyfile = existing_caddyfile
            .as_ref()
            .map(|existing_caddyfile| {
                if RE.is_match(existing_caddyfile) {
                    // Replace the portman caddyfile section if it exists
                    String::from(RE.replace(existing_caddyfile, portman_caddyfile.clone()))
                } else {
                    // Otherwise prepend the portman caddyfile section
                    format!("{portman_caddyfile}\n{existing_caddyfile}")
                }
            })
            // The caddyfile didn't exist before, so only use the portman caddyfile section
            .unwrap_or_else(|| portman_caddyfile);
        Ok(merged_caddyfile)
    }

    // Reload the caddy service with the current port registry
    pub fn reload_caddy(&self) -> Result<(), ApplicationError> {
        // Determine the caddyfile path
        let var_name = std::ffi::OsString::from("HOMEBREW_PREFIX");
        let brew_prefix =
            std::env::var(var_name.clone()).map_err(|var_err| ApplicationError::ReadEnv {
                name: var_name,
                var_err,
            })?;
        let caddyfile_path = PathBuf::from(brew_prefix).join("etc").join("Caddyfile");

        // Read the existing caddyfile so that we can augment it with the portman caddyfile entries
        let existing_caddyfile = match fs::read_to_string(caddyfile_path.clone()) {
            Ok(contents) => Some(contents),
            Err(io_err) => match io_err.kind() {
                // If the file doesn't exist, give it a default value of an empty port registry
                std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(ApplicationError::ReadCaddyfile(io_err)),
            }?,
        };
        fs::write(
            caddyfile_path.clone(),
            self.merge_caddyfile(existing_caddyfile)?,
        )
        .map_err(ApplicationError::WriteCaddyfile)?;

        // Reload the caddy config using the new Caddyfile
        let status = Command::new("caddy")
            .args(["reload", "--adapter", "caddyfile", "--config"])
            .arg(caddyfile_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(ApplicationError::Exec)?;
        if status.success() {
            Ok(())
        } else {
            Err(ApplicationError::ReloadCaddy)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::allocator::RandomPortChooser;
    use crate::config::Config;

    use super::*;

    struct RegistryMockStore {
        contents: RegistryData,
    }

    impl RegistryStore for RegistryMockStore {
        fn load(&self) -> Result<RegistryData, ApplicationError> {
            Ok(self.contents.clone())
        }

        fn save(&self, _: &RegistryData) -> Result<(), ApplicationError> {
            Ok(())
        }
    }

    // Return a PortRegistry that won't save to the filesystem
    fn get_mocked_registry(config: Option<Config>) -> Result<PortRegistry, ApplicationError> {
        let ports = vec![
            ("app1".to_string(), 3001),
            ("app2".to_string(), 3002),
            ("app3".to_string(), 3003),
        ]
        .into_iter()
        .collect::<BTreeMap<String, u16>>();
        let mock_store = RegistryMockStore {
            contents: RegistryData { ports },
        };
        let config = config.unwrap_or_default();
        let mock_allocator = PortAllocator::new(config.get_valid_ports(), RandomPortChooser::new());
        PortRegistry::new(mock_store, mock_allocator)
    }

    // Convert Err(ApplicationError::Exec(_)) into Ok(()), leaving all other results untouched
    fn suppress_exec_error<Value>(
        result: Result<Value, ApplicationError>,
    ) -> Result<(), ApplicationError> {
        match result {
            Ok(_) | Err(ApplicationError::Exec(_)) => Ok(()),
            Err(err) => Err(err),
        }
    }

    #[test]
    fn test_load_normalizes() -> Result<(), ApplicationError> {
        let config = Config {
            ranges: vec![(4000, 4999)],
            ..Default::default()
        };
        let registry = get_mocked_registry(Some(config))?;
        assert!(registry
            .ports
            .values()
            .into_iter()
            .all(|port| (4000..=4999).contains(port)));
        Ok(())
    }

    #[test]
    fn test_get() -> Result<(), ApplicationError> {
        let registry = get_mocked_registry(None)?;
        assert_eq!(registry.get("app1"), Some(3001));
        assert_eq!(registry.get("app4"), None);
        Ok(())
    }

    #[test]
    fn test_allocate() -> Result<(), ApplicationError> {
        let mut registry = get_mocked_registry(None)?;
        suppress_exec_error(registry.allocate("app4"))?;
        assert!(registry.ports.get("app4").is_some());
        Ok(())
    }

    #[test]
    fn test_release() -> Result<(), ApplicationError> {
        let mut registry = get_mocked_registry(None)?;
        suppress_exec_error(registry.release("app2"))?;
        assert!(registry.ports.get("app2").is_none());
        Ok(())
    }

    #[test]
    fn test_release_all() -> Result<(), ApplicationError> {
        let mut registry = get_mocked_registry(None)?;
        suppress_exec_error(registry.release_all())?;
        assert!(registry.ports.is_empty());
        Ok(())
    }

    const GOLDEN_CADDYFILE: &str = "# portman begin
# WARNING: This section is automatically generated by portman. Any manual edits will be overridden.

app1.localhost {
\treverse_proxy 127.0.0.1:3001
}

app2.localhost {
\treverse_proxy 127.0.0.1:3002
}

app3.localhost {
\treverse_proxy 127.0.0.1:3003
}
# portman end
";

    #[test]
    fn test_caddyfile() -> Result<(), ApplicationError> {
        let registry = get_mocked_registry(None)?;
        assert_eq!(registry.caddyfile(), GOLDEN_CADDYFILE);
        Ok(())
    }

    #[test]
    fn test_merge_caddyfile_no_existing() -> Result<(), ApplicationError> {
        let registry = get_mocked_registry(None)?;
        assert_eq!(registry.merge_caddyfile(None)?, GOLDEN_CADDYFILE);
        Ok(())
    }

    #[test]
    fn test_merge_caddyfile_update() -> Result<(), ApplicationError> {
        let registry = get_mocked_registry(None)?;
        assert_eq!(
            registry.merge_caddyfile(Some(
                "# Prefix\n\n# portman begin\n# portman end\n\n# Suffix\n".to_string()
            ))?,
            format!("# Prefix\n\n{}\n# Suffix\n", GOLDEN_CADDYFILE)
        );
        Ok(())
    }

    #[test]
    fn test_merge_caddyfile_prepend() -> Result<(), ApplicationError> {
        let registry = get_mocked_registry(None)?;
        assert_eq!(
            registry.merge_caddyfile(Some("# Suffix\n".to_string()))?,
            format!("{}\n# Suffix\n", GOLDEN_CADDYFILE)
        );
        Ok(())
    }
}
