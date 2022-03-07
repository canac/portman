use crate::error::ApplicationError;
use directories::ProjectDirs;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Default, Deserialize, Serialize)]
pub struct PortRegistry {
    synced_dirs: HashSet<PathBuf>,
    ports: HashMap<String, u16>,
}

impl PortRegistry {
    // Load a port registry from the file
    pub fn load() -> Result<Self, ApplicationError> {
        let registry_path = Self::get_registry_path()?;
        match fs::read_to_string(&registry_path) {
            Ok(registry_str) => {
                toml::from_str(&registry_str).map_err(ApplicationError::DeserializeRegistry)
            }
            Err(io_err) => match io_err.kind() {
                // If the file doesn't exist, give it a default value of an empty port registry
                std::io::ErrorKind::NotFound => Ok(Self::default()),
                _ => Err(ApplicationError::ReadRegistry {
                    path: registry_path,
                    io_err,
                }),
            },
        }
    }

    // Save a port registry to the file
    pub fn save(&self) -> Result<(), ApplicationError> {
        let registry_str = toml::to_string(&self).map_err(ApplicationError::SerializeRegistry)?;

        let registry_path = Self::get_registry_path()?;
        let parent_dir = registry_path
            .parent()
            .ok_or_else(|| ApplicationError::WriteRegistry(registry_path.clone()))?;
        fs::create_dir_all(parent_dir)
            .map_err(|_| ApplicationError::WriteRegistry(registry_path.clone()))?;
        fs::write(registry_path.clone(), registry_str)
            .map_err(|_| ApplicationError::WriteRegistry(registry_path.clone()))?;
        Ok(())
    }

    // Get a port from the registry
    pub fn get(&mut self, project: &str) -> Result<u16, ApplicationError> {
        match self.ports.get(project) {
            Some(&port) => Ok(port),
            None => {
                let new_port = self.generate_port()?;
                self.ports.insert(project.to_string(), new_port);
                self.save()?;
                Ok(new_port)
            }
        }
    }

    // Return a reference to all the ports in the registry
    pub fn get_all(&self) -> &HashMap<String, u16> {
        &self.ports
    }

    // Release a project's port from the registry
    // Return an option with the removed port if the project existed, none if it didn't
    pub fn release(&mut self, project: &str) -> Result<Option<u16>, ApplicationError> {
        let removed = self.ports.remove(project);
        if removed.is_some() {
            self.save()?;
        }
        Ok(removed)
    }

    // When synced directories are cd-ed into, the PORT environment variable
    // will be automatically set to the assigned port via the shell integration
    // installed by the init script. This only happens to synced directories
    // that are manually whitelisted.

    // Start syncing a directory
    pub fn add_sync_dir(&mut self, dir: PathBuf) -> Result<bool, ApplicationError> {
        let added = self.synced_dirs.insert(dir);
        if added {
            self.save()?;
        }
        Ok(added)
    }

    // Stop syncing a directory
    pub fn remove_sync_dir(&mut self, dir: &Path) -> Result<bool, ApplicationError> {
        let removed = self.synced_dirs.remove(dir);
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    // Return a boolean indicating whether the directory is in the whitelist
    // of directories that will have their PORT
    pub fn check_dir_synced(&self, dir: &Path) -> bool {
        self.synced_dirs.contains(dir)
    }

    // Return the path to the persisted registry file
    fn get_registry_path() -> Result<PathBuf, ApplicationError> {
        let project_dirs =
            ProjectDirs::from("com", "canac", "portman").ok_or(ApplicationError::ProjectDirs)?;
        Ok(project_dirs.data_local_dir().join("registry.toml"))
    }

    // Generate a new unique port
    fn generate_port(&self) -> Result<u16, ApplicationError> {
        let mut available_ports = (3000..4000).collect::<HashSet<u16>>();
        for (_, port) in self.ports.iter() {
            available_ports.remove(port);
        }

        let mut rng = rand::thread_rng();
        Ok(*available_ports.iter().choose(&mut rng).unwrap())
    }
}
