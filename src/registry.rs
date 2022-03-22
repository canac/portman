use crate::config::Config;
use crate::error::ApplicationError;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
    process::{Command, Stdio},
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
    ports: BTreeMap<String, u16>,
}

pub struct PortRegistry {
    path: PathBuf,
    ports: BTreeMap<String, u16>,
    allocator: PortAllocator,
}

impl PortRegistry {
    // Load a port registry from the file
    pub fn load(path: PathBuf, config: &Config) -> Result<Self, ApplicationError> {
        let registry_data = match fs::read_to_string(&path) {
            Ok(registry_str) => {
                toml::from_str(&registry_str).map_err(ApplicationError::DeserializeRegistry)
            }
            Err(io_err) => match io_err.kind() {
                // If the file doesn't exist, give it a default value of an empty port registry
                std::io::ErrorKind::NotFound => Ok(RegistryData::default()),
                _ => Err(ApplicationError::ReadRegistry {
                    path: path.clone(),
                    io_err,
                }),
            },
        }?;

        // Validate all ports in the registry against the required config and
        // regenerate invalid ones as necessary
        let mut changed = false;
        let mut allocator = PortAllocator::new(config.get_valid_ports());
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
        let registry = PortRegistry {
            path,
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
        let registry_str = toml::to_string(&RegistryData {
            ports: self.ports.clone(),
        })
        .map_err(ApplicationError::SerializeRegistry)?;

        let parent_dir = self
            .path
            .parent()
            .ok_or_else(|| ApplicationError::WriteRegistry(self.path.clone()))?;
        fs::create_dir_all(parent_dir)
            .map_err(|_| ApplicationError::WriteRegistry(self.path.clone()))?;
        fs::write(self.path.clone(), registry_str)
            .map_err(|_| ApplicationError::WriteRegistry(self.path.clone()))?;

        self.reload_caddy()
    }

    // Get a project's port from the registry
    pub fn get(&self, project: &str) -> Option<u16> {
        self.ports.get(project).cloned()
    }

    // Return a reference to all the ports in the registry
    pub fn get_all(&self) -> impl Iterator<Item = (&String, &u16)> + '_ {
        self.ports.iter()
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

    // Return the generated Caddyfile
    pub fn caddyfile(&self) -> String {
        let caddyfile = self
            .get_all()
            .map(|(project, port)| {
                format!(
                    "{}.localhost {{\n\treverse_proxy 127.0.0.1:{}\n}}\n",
                    project, port
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("# WARNING: This file is automatically generated by portman. Any manual edits will be overridden.\n\n{caddyfile}")
    }

    // Reload the caddy service with the current port registry
    pub fn reload_caddy(&self) -> Result<(), ApplicationError> {
        // Write the caddyfile to a file
        let caddyfile = self.caddyfile();
        let brew_prefix = std::env::var("HOMEBREW_PREFIX").map_err(ApplicationError::ReadEnv)?;
        let caddyfile_path = PathBuf::from(brew_prefix).join("etc").join("Caddyfile");
        fs::write(caddyfile_path.clone(), caddyfile).map_err(ApplicationError::WriteCaddyfile)?;

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
