use crate::error::ApplicationError;
use directories::ProjectDirs;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

#[derive(Default, Deserialize, Serialize)]
pub struct PortRegistry {
    ports: HashMap<String, u16>,
}

impl PortRegistry {
    // Load a port registry from the file
    pub fn load() -> Result<Self, ApplicationError> {
        let registry_path = Self::get_registry_path()?;
        let registry_str =
            fs::read_to_string(&registry_path).or_else(|io_err| match io_err.kind() {
                // If the file doesn't exist, give it a default value of an empty port registry
                std::io::ErrorKind::NotFound => Ok("ports = {}".to_string()),
                _ => Err(ApplicationError::ReadRegistry {
                    path: registry_path.clone(),
                    io_err,
                }),
            })?;
        toml::from_str(&registry_str).map_err(|_| ApplicationError::DeserializeRegistry)
    }

    // Save a port registry to the file
    pub fn save(&self) -> Result<(), ApplicationError> {
        let registry_str =
            toml::to_string(&self).map_err(|_| ApplicationError::SerializeRegistry)?;

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
    pub fn get(&mut self, project: String) -> Result<u16, ApplicationError> {
        match self.ports.get(&project) {
            Some(&port) => Ok(port),
            None => {
                let new_port = self.generate_port()?;
                self.ports.insert(project, new_port);
                self.save()?;
                Ok(new_port)
            }
        }
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
