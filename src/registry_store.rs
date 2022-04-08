use crate::error::ApplicationError;
use crate::registry::RegistryData;
use std::fs;
use std::path::PathBuf;

// Trait defines an interface for loading and saving a port registry
pub trait RegistryStore {
    fn load(&self) -> Result<RegistryData, ApplicationError>;
    fn save(&self, registry: &RegistryData) -> Result<(), ApplicationError>;
}

// Registry store backed by the filesystem
pub struct RegistryFileStore {
    path: PathBuf,
}

impl RegistryFileStore {
    pub fn new(path: PathBuf) -> Self {
        RegistryFileStore { path }
    }
}

impl RegistryStore for RegistryFileStore {
    // Load the registry store from the filesystem
    fn load(&self) -> Result<RegistryData, ApplicationError> {
        match fs::read_to_string(&self.path) {
            Ok(registry_str) => {
                toml::from_str(&registry_str).map_err(ApplicationError::DeserializeRegistry)
            }
            Err(io_err) => match io_err.kind() {
                // If the file doesn't exist, give it a default value of an empty port registry
                std::io::ErrorKind::NotFound => Ok(RegistryData::default()),
                _ => Err(ApplicationError::ReadRegistry {
                    path: self.path.clone(),
                    io_err,
                }),
            },
        }
    }

    // Save the registry store to the filesystem
    fn save(&self, registry: &RegistryData) -> Result<(), ApplicationError> {
        let registry_str =
            toml::to_string(registry).map_err(ApplicationError::SerializeRegistry)?;

        let parent_dir = self
            .path
            .parent()
            .ok_or_else(|| ApplicationError::WriteRegistry(self.path.clone()))?;
        fs::create_dir_all(parent_dir)
            .map_err(|_| ApplicationError::WriteRegistry(self.path.clone()))?;
        fs::write(self.path.clone(), registry_str)
            .map_err(|_| ApplicationError::WriteRegistry(self.path.clone()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_load() -> Result<(), ApplicationError> {
        let registry_store = RegistryFileStore::new(PathBuf::from("./fixtures/registry.toml"));
        let registry_data = registry_store.load()?;
        let expected_ports = vec![
            ("app1".to_string(), 3001),
            ("app2".to_string(), 3002),
            ("app3".to_string(), 3003),
        ]
        .into_iter()
        .collect::<BTreeMap<String, u16>>();
        assert_eq!(registry_data.ports, expected_ports);
        Ok(())
    }

    #[test]
    fn test_load_missing() -> Result<(), ApplicationError> {
        let registry_store = RegistryFileStore::new(PathBuf::from("./missing_registry.toml"));
        let registry_data = registry_store.load()?;
        assert_eq!(registry_data.ports.len(), 0);
        Ok(())
    }

    #[test]
    fn test_save_creates_file() -> Result<(), ApplicationError> {
        let portman_dir = std::env::temp_dir().join("portman");
        let registry_path = portman_dir
            .join("deeply")
            .join("nested")
            .join("path")
            .join("registry.toml");
        fs::remove_dir_all(portman_dir).unwrap_or(());

        let ports = vec![
            ("app1".to_string(), 3001),
            ("app2".to_string(), 3002),
            ("app3".to_string(), 3003),
        ]
        .into_iter()
        .collect::<BTreeMap<String, u16>>();
        let registry_data = RegistryData { ports };

        let registry_store = RegistryFileStore::new(registry_path.clone());
        registry_store.save(&registry_data)?;

        assert!(std::path::Path::exists(&registry_path));
        Ok(())
    }
}
