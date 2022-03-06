use directories::ProjectDirs;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

#[derive(Deserialize, Serialize)]
pub struct PortRegistry {
    ports: HashMap<String, u16>,
}

impl PortRegistry {
    // Load a port registry from the file
    pub fn load() -> Result<Self, ()> {
        let registry_str = fs::read_to_string(&Self::get_registry_path())
            .unwrap_or_else(|_| "ports = {}".to_string());
        toml::from_str(&registry_str).map_err(|_| ())
    }

    // Save a port registry to the file
    pub fn save(&self) -> Result<(), ()> {
        let registry_str = toml::to_string(&self).map_err(|_| ())?;

        let registry_path = Self::get_registry_path();
        let parent_dir = registry_path.parent().ok_or(())?;
        fs::create_dir_all(parent_dir).map_err(|_| ())?;
        fs::write(registry_path, registry_str).map_err(|_| ())?;
        Ok(())
    }

    // Get a port from the registry
    pub fn get(&mut self, project: String) -> Result<u16, ()> {
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
    fn get_registry_path() -> PathBuf {
        ProjectDirs::from("com", "github.canac", "portman")
            .unwrap()
            .data_local_dir()
            .join("registry.toml")
    }

    // Generate a new unique port
    fn generate_port(&self) -> Result<u16, ()> {
        let mut available_ports = (3000..4000).collect::<HashSet<u16>>();
        for (_, port) in self.ports.iter() {
            available_ports.remove(port);
        }

        let mut rng = rand::thread_rng();
        Ok(*available_ports.iter().choose(&mut rng).unwrap())
    }
}
