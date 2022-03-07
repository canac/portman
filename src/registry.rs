use crate::config::Config;
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

    // Get a a project's port from the registry_path
    // A port is not generated if the project does not exist. However, if an
    // existing project's port is invalid due to a configuration change, a new
    // valid port is transparently generated.
    pub fn get(&mut self, project: &str) -> Result<u16, ApplicationError> {
        let config = Config::load()?;
        match self.ports.get(project) {
            None => Err(ApplicationError::NonExistentProject(project.to_string())),
            Some(&port) => {
                if config.is_port_valid(port) {
                    Ok(port)
                } else {
                    // Regenerate a valid port for this project
                    let new_port = self.generate_port()?;
                    self.ports.insert(project.to_string(), new_port);
                    self.save()?;
                    Ok(new_port)
                }
            }
        }
    }

    // Return a reference to all the ports in the registry
    pub fn get_all(&self) -> &HashMap<String, u16> {
        &self.ports
    }

    // Allocate a port to a new project
    pub fn allocate(&mut self, project: &str) -> Result<u16, ApplicationError> {
        if self.ports.get(project).is_some() {
            Err(ApplicationError::DuplicateProject(project.to_string()))
        } else {
            let new_port = self.generate_port()?;
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

    // Return the path to the persisted registry file
    fn get_registry_path() -> Result<PathBuf, ApplicationError> {
        let project_dirs =
            ProjectDirs::from("com", "canac", "portman").ok_or(ApplicationError::ProjectDirs)?;
        Ok(project_dirs.data_local_dir().join("registry.toml"))
    }

    // Generate a new unique port
    fn generate_port(&self) -> Result<u16, ApplicationError> {
        let assigned_ports = self.ports.values().collect::<HashSet<_>>();
        let config = Config::load()?;
        let available_ports = config
            .get_valid_ports()
            .filter(|port| !assigned_ports.contains(port));

        let mut rng = rand::thread_rng();
        available_ports
            .choose(&mut rng)
            .ok_or(ApplicationError::AllPortsAllocated)
    }
}
