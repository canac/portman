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

struct PortAllocator {
    available_ports: HashSet<u16>,
    rng: ThreadRng,
}

impl PortAllocator {
    // Create a new port allocator that allocates from the provided available ports
    pub fn new(available_ports: impl Iterator<Item = u16>) -> Self {
        PortAllocator {
            available_ports: available_ports.collect(),
            rng: rand::thread_rng(),
        }
    }

    // Allocate a new port, using the desired port if it is provided and is valid
    pub fn allocate(&mut self, desired_port: Option<u16>) -> Option<u16> {
        let allocated_port = desired_port
            .and_then(|port| {
                if self.available_ports.contains(&port) {
                    Some(port)
                } else {
                    None
                }
            })
            .or_else(|| self.available_ports.iter().choose(&mut self.rng).cloned());
        if let Some(port) = allocated_port {
            self.available_ports.remove(&port);
        }
        allocated_port
    }
}

// The port registry data that will be serialized and deserialized in the database
#[derive(Default, Deserialize, Serialize)]
pub struct RegistryData {
    ports: HashMap<String, u16>,
}

pub struct PortRegistry {
    ports: HashMap<String, u16>,
    allocator: PortAllocator,
}

impl PortRegistry {
    // Load a port registry from the file
    pub fn load(config: &Config) -> Result<Self, ApplicationError> {
        let registry_path = Self::get_registry_path()?;
        let registry_data = match fs::read_to_string(&registry_path) {
            Ok(registry_str) => {
                toml::from_str(&registry_str).map_err(ApplicationError::DeserializeRegistry)
            }
            Err(io_err) => match io_err.kind() {
                // If the file doesn't exist, give it a default value of an empty port registry
                std::io::ErrorKind::NotFound => Ok(RegistryData::default()),
                _ => Err(ApplicationError::ReadRegistry {
                    path: registry_path,
                    io_err,
                }),
            },
        }?;

        // Validate all ports in the registry against the required config and
        // regenerate invalid ones as necessary
        let mut allocator = PortAllocator::new(config.get_valid_ports());
        let validated_ports = registry_data
            .ports
            .into_iter()
            .map(|(project, port)| allocator.allocate(Some(port)).map(|port| (project, port)))
            .collect::<Option<HashMap<_, _>>>()
            .ok_or(ApplicationError::AllPortsAllocated)?;
        let registry = PortRegistry {
            ports: validated_ports,
            allocator,
        };
        registry.save()?;
        Ok(registry)
    }

    // Save a port registry to the file
    pub fn save(&self) -> Result<(), ApplicationError> {
        let registry_str = toml::to_string(&RegistryData {
            ports: self.ports.clone(),
        })
        .map_err(ApplicationError::SerializeRegistry)?;

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

    // Get a a project's port from the registry
    pub fn get(&self, project: &str) -> Result<u16, ApplicationError> {
        self.ports
            .get(project)
            .cloned()
            .ok_or_else(|| ApplicationError::NonExistentProject(project.to_string()))
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
        self.ports = HashMap::new();
        self.save()
    }

    // Return the path to the persisted registry file
    fn get_registry_path() -> Result<PathBuf, ApplicationError> {
        let project_dirs =
            ProjectDirs::from("com", "canac", "portman").ok_or(ApplicationError::ProjectDirs)?;
        Ok(project_dirs.data_local_dir().join("registry.toml"))
    }
}
