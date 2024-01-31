use crate::caddy::reload;
use crate::dependencies::{ChoosePort, DataDir, Exec, ReadFile, WorkingDirectory, WriteFile};
use crate::{allocator::PortAllocator, dependencies::Environment};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Project {
    pub port: u16,
    pub directory: Option<PathBuf>,
    pub linked_port: Option<u16>,
}

// The port registry data that will be serialized and deserialized in the database
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct RegistryData {
    pub projects: BTreeMap<String, Project>,
}

pub struct Registry {
    store_path: PathBuf,
    projects: BTreeMap<String, Project>,
    allocator: PortAllocator,
    dirty: bool,
}

impl Registry {
    // Create a new registry
    pub fn new(
        deps: &(impl ChoosePort + DataDir + Environment + ReadFile),
        port_allocator: PortAllocator,
    ) -> Result<Self> {
        let store_path = deps.get_data_dir()?.join(PathBuf::from("registry.toml"));
        let registry_data = deps
            .read_file(&store_path)
            .context("Failed to load registry")?
            .map(|registry_str| {
                toml::from_str::<RegistryData>(&registry_str).with_context(|| {
                    format!(
                        "Failed to deserialize project registry at \"{}\"",
                        store_path.display()
                    )
                })
            })
            .transpose()?
            .unwrap_or_default();

        let mut allocator = port_allocator;
        let mut linked_ports = HashSet::new();
        for project in registry_data.projects.values() {
            if let Some(linked_port) = project.linked_port {
                // Prevent projects from using this port
                allocator.discard(linked_port);
            };
        }

        let mut dirty = false;
        let mut directories: HashSet<PathBuf> = HashSet::new();

        // Validate all ports in the registry against the config and regenerate
        // invalid ones as necessary
        let projects = registry_data
            .projects
            .into_iter()
            .map(|(name, old_project)| {
                Self::validate_name(&name)?;

                let mut old_project = old_project;

                if let Some(linked_port) = old_project.linked_port {
                    if !linked_ports.insert(linked_port) {
                        old_project.linked_port = None;
                        dirty = true;
                    }
                }

                if let Some(directory) = old_project.directory.as_ref() {
                    if !directories.insert(directory.clone()) {
                        old_project.directory = None;
                        dirty = true;
                    }
                }

                let existing_port = old_project.port;
                allocator.allocate(deps, Some(existing_port)).map(|port| {
                    if port != existing_port {
                        dirty = true;
                    }
                    (
                        name,
                        Project {
                            port,
                            ..old_project
                        },
                    )
                })
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let registry = Self {
            store_path,
            projects,
            allocator,
            dirty,
        };
        Ok(registry)
    }

    // Save a port registry to the file
    pub fn save(
        &self,
        deps: &(impl DataDir + Environment + Exec + ReadFile + WriteFile),
    ) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let registry = RegistryData {
            projects: self.projects.clone(),
        };
        let registry_str =
            toml::to_string(&registry).context("Failed to serialize project registry")?;
        deps.write_file(&self.store_path, &registry_str)
            .context("Failed to save registry")?;
        if let Err(err) = reload(deps, self) {
            // An error reloading Caddy is just a warning, not a fatal error
            eprintln!("Warning: couldn't reload Caddy config.\n\n{err}");
        }
        Ok(())
    }

    // Get a project from the registry
    pub fn get(&self, name: &str) -> Option<&Project> {
        self.projects.get(name)
    }

    // Create a new project and return it
    pub fn create(
        &mut self,
        deps: &impl ChoosePort,
        name: &str,
        directory: Option<PathBuf>,
        linked_port: Option<u16>,
    ) -> Result<Project> {
        Self::validate_name(name)?;

        if self.projects.contains_key(name) {
            bail!("A project already has the name {name}");
        }

        if let Some(directory) = directory.as_ref() {
            if self
                .projects
                .values()
                .any(|project| project.directory.as_ref() == Some(directory))
            {
                bail!(
                    "A project already has the directory \"{}\"",
                    directory.display()
                );
            }
        }

        let port = self.allocator.allocate(deps, None)?;
        let new_project = Project {
            port,
            directory,
            linked_port: None,
        };
        self.projects.insert(name.to_string(), new_project.clone());

        if let Some(port) = linked_port {
            self.link(deps, name, port)?;
        }

        self.dirty = true;

        Ok(new_project)
    }

    // Update a project and return the updated project
    pub fn update(
        &mut self,
        _deps: &impl Any,
        name: &str,
        directory: Option<PathBuf>,
    ) -> Result<Project> {
        let Some(project) = self.projects.get_mut(name) else {
            bail!("Project {name} does not exist");
        };

        if project.directory != directory {
            project.directory = directory;
            self.dirty = true;
        }
        Ok(project.clone())
    }

    // Delete a project and return the deleted project and its names
    pub fn delete(&mut self, _deps: &impl Any, name: &str) -> Result<Project> {
        let Some(project) = self.projects.remove(name) else {
            bail!("Project {name} does not exist");
        };
        self.dirty = true;
        Ok(project)
    }

    // Delete multiple projects and return the deleted projects and their names
    pub fn delete_many(
        &mut self,
        _deps: &impl Any,
        project_names: Vec<String>,
    ) -> Result<Vec<(String, Project)>> {
        let deleted_projects: Vec<(String, Project)> = project_names
            .into_iter()
            .map(|name| {
                if let Some(project) = self.projects.remove(&name) {
                    Ok((name, project))
                } else {
                    bail!("Project {name} does not exist");
                }
            })
            .collect::<Result<Vec<_>>>()?;
        if !deleted_projects.is_empty() {
            self.dirty = true;
        }
        Ok(deleted_projects)
    }

    // Delete all projects
    pub fn delete_all(&mut self, _deps: &impl Any) {
        self.projects = BTreeMap::new();
        self.dirty = true;
    }

    // Iterate over all projects with their names
    pub fn iter_projects(&self) -> impl Iterator<Item = (&String, &Project)> {
        self.projects.iter()
    }

    // Link a port to a project
    pub fn link(
        &mut self,
        deps: &impl ChoosePort,
        project_name: &str,
        linked_port: u16,
    ) -> Result<()> {
        if !self.projects.contains_key(project_name) {
            bail!("Project {project_name} does not exist");
        }

        for (name, project) in &mut self.projects.iter_mut() {
            if project.port == linked_port {
                // Take the port from the project so that it can be used by the linked port
                project.port = self.allocator.allocate(deps, None)?;
                self.dirty = true;
            }
            if name == project_name {
                if project.linked_port != Some(linked_port) {
                    // Link the port to the new project
                    project.linked_port = Some(linked_port);
                    self.dirty = true;
                }
            } else if project.linked_port == Some(linked_port) {
                // Unlink the port from the previous project
                project.linked_port = None;
                self.dirty = true;
            }
        }

        Ok(())
    }

    // Unlink the port linked to a project and return the previous linked port
    pub fn unlink(&mut self, _deps: &impl Any, project_name: &str) -> Result<Option<u16>> {
        let Some(project) = self.projects.get_mut(project_name) else {
            bail!("Project {project_name} does not exist");
        };

        let previous_linked_port = project.linked_port.take();
        if previous_linked_port.is_some() {
            self.dirty = true;
        }
        Ok(previous_linked_port)
    }

    // Find and return the project that matches the current working directory, if any
    pub fn match_cwd(&self, deps: &impl WorkingDirectory) -> Result<Option<(&String, &Project)>> {
        let cwd = deps.get_cwd()?;
        Ok(self.iter_projects().find(|(_, project)| {
            project
                .directory
                .as_ref()
                .map_or(false, |directory| directory == &cwd)
        }))
    }

    // Normalize a potential project name by stripping out invalid characters
    pub fn normalize_name(name: &str) -> String {
        let mut normalized = name
            .chars()
            .map(|char| {
                if char.is_ascii_alphanumeric() || char == '-' {
                    char
                } else {
                    '-'
                }
            })
            .skip_while(|char| char == &'-')
            .fold(String::with_capacity(name.len()), |mut result, char| {
                // Remove adjacent dashes
                if !(result.ends_with('-') && char == '-') {
                    result.push(char.to_ascii_lowercase());
                }
                result
            })
            .to_string();
        normalized.truncate(63);
        if normalized.ends_with('-') {
            normalized.pop();
        }
        normalized
    }

    // Validate a project name
    pub fn validate_name(name: &str) -> Result<()> {
        if name.is_empty() {
            bail!("Project name cannot be empty")
        }
        if name.len() > 63 {
            bail!("Project name cannot exceed 63 characters")
        }
        if name.starts_with('-') || name.ends_with('-') {
            bail!("Project name cannot start or end with a dash")
        }
        if name.contains("--") {
            bail!("Project name cannot contain consecutive dashes")
        }
        if name
            .chars()
            .any(|char| !(char.is_ascii_lowercase() || char.is_numeric() || char == '-'))
        {
            bail!("Project name can only contain the lowercase alphanumeric characters and dashes")
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::Config;
    use crate::dependencies::mocks::{data_dir_mock, write_file_mock};
    use crate::dependencies::{self};
    use std::os::unix::process::ExitStatusExt;
    use unimock::{matching, MockFn};

    pub const REGISTRY: &str = "
[projects]

[projects.app1]
port = 3001

[projects.app2]
port = 3002
linked_port = 3000

[projects.app3]
port = 3003
directory = '/projects/app3'
";

    fn choose_port_mock() -> unimock::Clause {
        dependencies::choose_port::Fn
            .each_call(matching!(_))
            .answers(|available_ports| available_ports.iter().min().copied())
            .in_any_order()
    }

    fn read_file_mock() -> unimock::Clause {
        dependencies::read_file::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(Some(String::from(REGISTRY))))
            .in_any_order()
    }

    fn read_var_mock() -> unimock::Clause {
        dependencies::read_var::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(String::new()))
            .in_any_order()
    }

    pub fn get_mocked_registry() -> Result<Registry> {
        let mocked_deps = unimock::mock([data_dir_mock(), read_file_mock()]);
        let config = Config::default();
        let allocator = PortAllocator::new(config.get_valid_ports());
        Registry::new(&mocked_deps, allocator)
    }

    #[test]
    fn test_load_invalid() {
        let config = Config::default();
        let mocked_deps = unimock::mock([
            data_dir_mock(),
            dependencies::read_file::Fn
                .each_call(matching!(_))
                .answers(|_| Ok(Some(String::from(";"))))
                .in_any_order(),
        ]);
        let allocator = PortAllocator::new(config.get_valid_ports());
        assert!(Registry::new(&mocked_deps, allocator).is_err());
    }

    #[test]
    fn test_load_invalid_name() {
        let config = Config::default();
        let mocked_deps = unimock::mock([
            data_dir_mock(),
            dependencies::read_file::Fn
                .each_call(matching!(_))
                .answers(|_| Ok(Some(String::from("projects.App1 = { port = 3001 }"))))
                .in_any_order(),
        ]);
        let allocator = PortAllocator::new(config.get_valid_ports());
        assert!(Registry::new(&mocked_deps, allocator).is_err());
    }

    #[test]
    fn test_load_duplicate_directory() {
        let config = Config::default();
        let mocked_deps = unimock::mock([
            data_dir_mock(),
            dependencies::read_file::Fn
                .each_call(matching!(_))
                .answers(|_| {
                    Ok(Some(String::from(
                        "[projects]

[projects.app1]
port = 3001
directory = '/projects/app'

[projects.app2]
port = 3002
directory = '/projects/app'",
                    )))
                })
                .in_any_order(),
        ]);
        let allocator = PortAllocator::new(config.get_valid_ports());
        let registry = Registry::new(&mocked_deps, allocator).unwrap();
        assert!(registry.get("app1").unwrap().directory.is_some());
        assert!(registry.get("app2").unwrap().directory.is_none());
        assert!(registry.dirty);
    }

    #[test]
    fn test_load_duplicate_linked() {
        let config = Config::default();
        let mocked_deps = unimock::mock([
            data_dir_mock(),
            dependencies::read_file::Fn
                .each_call(matching!(_))
                .answers(|_| {
                    Ok(Some(String::from(
                        "[projects]

[projects.app1]
port = 3001
linked_port = 3000

[projects.app2]
port = 3002
linked_port = 3000",
                    )))
                })
                .in_any_order(),
        ]);
        let allocator = PortAllocator::new(config.get_valid_ports());
        let registry = Registry::new(&mocked_deps, allocator).unwrap();
        assert!(registry.get("app1").unwrap().linked_port.is_some());
        assert!(registry.get("app2").unwrap().linked_port.is_none());
        assert!(registry.dirty);
    }

    #[test]
    fn test_load_reallocates_for_linked() {
        let config = Config::default();
        let mocked_deps = unimock::mock([
            choose_port_mock(),
            data_dir_mock(),
            dependencies::read_file::Fn
                .each_call(matching!(_))
                .answers(|_| {
                    Ok(Some(String::from(
                        "projects.app1 = { port = 3001, linked_port = 3001 }",
                    )))
                })
                .in_any_order(),
        ]);
        let allocator = PortAllocator::new(config.get_valid_ports());
        let registry = Registry::new(&mocked_deps, allocator).unwrap();
        assert_eq!(registry.projects.get("app1").unwrap().port, 3000);
        assert!(registry.dirty);
    }

    #[test]
    fn test_load_normalizes() {
        let config = Config {
            ranges: vec![(4000, 4999)],
            ..Default::default()
        };
        let mocked_deps = unimock::mock([choose_port_mock(), data_dir_mock(), read_file_mock()]);
        let allocator = PortAllocator::new(config.get_valid_ports());
        let registry = Registry::new(&mocked_deps, allocator).unwrap();
        assert_eq!(registry.get("app1").unwrap().port, 4000);
        assert_eq!(registry.get("app2").unwrap().port, 4001);
        assert_eq!(registry.get("app3").unwrap().port, 4002);
        assert!(registry.dirty);
    }

    #[test]
    fn test_save_clean() {
        let config = Config::default();
        let mocked_deps = unimock::mock([data_dir_mock(), read_file_mock()]);
        let allocator = PortAllocator::new(config.get_valid_ports());
        let registry = Registry::new(&mocked_deps, allocator).unwrap();
        assert!(!registry.dirty);
        registry.save(&mocked_deps).unwrap();
    }

    #[test]
    fn test_save_caddy_read_failure() {
        let mocked_deps = unimock::mock([
            data_dir_mock(),
            dependencies::read_file::Fn
                .each_call(matching!(_))
                .answers(|_| bail!("Error reading"))
                .in_any_order(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        registry.dirty = true;
        assert!(registry.save(&mocked_deps).is_ok());
    }

    #[test]
    fn test_save_caddy_write_portman_caddyfile_failure() {
        let mocked_deps = unimock::mock([
            data_dir_mock(),
            dependencies::write_file::Fn
                .next_call(matching!((path, _) if path == &PathBuf::from("/data/registry.toml")))
                .answers(|_| Ok(()))
                .once()
                .in_order(),
            dependencies::write_file::Fn
                .next_call(matching!((path, _) if path == &PathBuf::from("/data/Caddyfile")))
                .answers(|_| bail!("Error writing"))
                .once()
                .in_order(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        registry.dirty = true;
        assert!(registry.save(&mocked_deps).is_ok());
    }

    #[test]
    fn test_save_caddy_write_root_caddyfile_failure() {
        let mocked_deps = unimock::mock([
            data_dir_mock(),
            read_file_mock(),
            read_var_mock(),
            dependencies::write_file::Fn
                .next_call(matching!((path, _) if path == &PathBuf::from("/data/registry.toml")))
                .answers(|_| Ok(()))
                .once()
                .in_order(),
            dependencies::write_file::Fn
                .next_call(matching!((path, _) if path == &PathBuf::from("/data/Caddyfile")))
                .answers(|_| Ok(()))
                .once()
                .in_order(),
            dependencies::write_file::Fn
                .next_call(matching!((path, _) if path == &PathBuf::from("etc/Caddyfile")))
                .answers(|_| bail!("Error writing"))
                .once()
                .in_order(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        registry.dirty = true;
        assert!(registry.save(&mocked_deps).is_ok());
    }

    #[test]
    fn test_save_caddy_exec_failure() {
        let mocked_deps = unimock::mock([
            data_dir_mock(),
            dependencies::exec::Fn
                .each_call(matching!(_))
                .answers(|_| Ok((ExitStatusExt::from_raw(1), String::new())))
                .in_any_order(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        registry.dirty = true;
        assert!(registry.save(&mocked_deps).is_ok());
    }

    #[test]
    fn test_get() {
        let registry = get_mocked_registry().unwrap();
        assert_eq!(registry.get("app1").unwrap().port, 3001);
        assert!(registry.get("app4").is_none());
    }

    #[test]
    fn test_create() {
        let mocked_deps = unimock::mock([choose_port_mock()]);
        let mut registry = get_mocked_registry().unwrap();
        registry.create(&mocked_deps, "app4", None, None).unwrap();
        assert!(registry.get("app4").is_some());
        assert!(registry.dirty);
    }

    #[test]
    fn test_create_invalid_name() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.create(&mocked_deps, "App3", None, None).is_err());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_create_duplicate_name() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.create(&mocked_deps, "app3", None, None).is_err());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_create_linked_port() {
        let mocked_deps = unimock::mock([choose_port_mock()]);
        let mut registry = get_mocked_registry().unwrap();
        registry
            .create(&mocked_deps, "app4", None, Some(3100))
            .unwrap();
        assert_eq!(registry.get("app4").unwrap().linked_port.unwrap(), 3100);
        assert!(registry.dirty);
    }

    #[test]
    fn test_create_linked_port_reallocates_previous() {
        let mocked_deps = unimock::mock([choose_port_mock()]);
        let mut registry = get_mocked_registry().unwrap();
        registry
            .create(&mocked_deps, "app4", None, Some(3001))
            .unwrap();
        assert_eq!(registry.get("app1").unwrap().port, 3005);
        assert!(registry.dirty);
    }

    #[test]
    fn test_create_duplicate_directory() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry
            .create(
                &mocked_deps,
                &"app4",
                Some(PathBuf::from("/projects/app3")),
                None,
            )
            .is_err());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_update() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        let path = PathBuf::from("/projects/app2");
        assert_eq!(
            registry
                .update(&mocked_deps, "app2", Some(path.clone()))
                .unwrap()
                .directory
                .unwrap(),
            path,
        );
        assert_eq!(
            registry.get("app2").unwrap().directory.as_ref().unwrap(),
            &path,
        );
        assert!(registry.dirty);
    }

    #[test]
    fn test_update_same() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        let path = PathBuf::from("/projects/app3");
        assert_eq!(
            registry
                .update(&mocked_deps, "app3", Some(path.clone()))
                .unwrap()
                .directory
                .unwrap(),
            path,
        );
        assert_eq!(
            registry.get("app3").unwrap().directory.as_ref().unwrap(),
            &path,
        );
        assert!(!registry.dirty);
    }

    #[test]
    fn test_update_nonexistent() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.update(&mocked_deps, "app4", None).is_err());
    }

    #[test]
    fn test_delete() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        registry.delete(&mocked_deps, "app2").unwrap();
        assert!(registry.get("app2").is_none());
    }

    #[test]
    fn test_delete_nonexistent() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.delete(&mocked_deps, "app4").is_err());
    }

    #[test]
    fn test_delete_many() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        let deleted_projects = registry
            .delete_many(
                &mocked_deps,
                vec![String::from("app1"), String::from("app2")],
            )
            .unwrap()
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        assert_eq!(
            deleted_projects,
            vec![String::from("app1"), String::from("app2")],
        );
        assert_eq!(registry.projects.keys().collect::<Vec<_>>(), vec!["app3"]);
    }

    #[test]
    fn test_delete_many_none() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry
            .delete_many(&mocked_deps, vec![])
            .unwrap()
            .is_empty());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_delete_all() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        registry.delete_all(&mocked_deps);
        assert!(registry.projects.is_empty());
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_create() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app2", 3005).unwrap();
        assert_eq!(registry.get("app2").unwrap().linked_port.unwrap(), 3005);
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_change() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app2", 3100).unwrap();
        assert_eq!(registry.get("app2").unwrap().linked_port.unwrap(), 3100);
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_move() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app3", 3000).unwrap();
        assert!(registry.get("app2").unwrap().linked_port.is_none());
        assert_eq!(registry.get("app3").unwrap().linked_port.unwrap(), 3000);
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_noop() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app2", 3000).unwrap();
        assert!(!registry.dirty);
    }

    #[test]
    fn test_link_nonexistent() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.link(&mocked_deps, "app4", 3004).is_err());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_link_reallocates_previous() {
        let mocked_deps = unimock::mock([choose_port_mock()]);
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app2", 3001).unwrap();
        assert_ne!(registry.get("app1").unwrap().port, 3001);
        assert_eq!(registry.get("app2").unwrap().linked_port.unwrap(), 3001);
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_reallocates_self() {
        let mocked_deps = unimock::mock([choose_port_mock()]);
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app1", 3001).unwrap();
        assert_ne!(registry.get("app1").unwrap().port, 3001);
        assert_eq!(registry.get("app1").unwrap().linked_port.unwrap(), 3001);
        assert!(registry.dirty);
    }

    #[test]
    fn test_unlink() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert_eq!(
            registry.unlink(&mocked_deps, "app2").unwrap().unwrap(),
            3000,
        );
        assert!(registry.dirty);
    }

    #[test]
    fn test_unlink_no_previous() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.unlink(&mocked_deps, "app1").unwrap().is_none());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_unlink_nonexistent() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.unlink(&mocked_deps, "app4").is_err());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_match_cwd_dir() {
        let mocked_deps = unimock::mock([dependencies::get_cwd::Fn
            .each_call(matching!(_))
            .answers(|()| Ok(PathBuf::from("/projects/app3")))
            .in_any_order()]);
        let registry = get_mocked_registry().unwrap();
        assert_eq!(
            registry.match_cwd(&mocked_deps).unwrap().unwrap().0,
            ("app3")
        );
    }

    #[test]
    fn test_normalize_name() {
        assert_eq!(
            Registry::normalize_name("--ABC_def---_123-"),
            String::from("abc-def-123"),
        );
        assert_eq!(Registry::normalize_name(&"a".repeat(100)).len(), 63);
        assert_eq!(
            Registry::normalize_name(&format!("{}-a", "a".repeat(62))).len(),
            62,
        );
    }

    #[test]
    fn test_validate_name() {
        assert!(Registry::validate_name("").is_err());
        assert!(Registry::validate_name(&"a".repeat(64)).is_err());
        assert!(Registry::validate_name("-a").is_err());
        assert!(Registry::validate_name("a-").is_err());
        assert!(Registry::validate_name("a--b").is_err());
        assert!(Registry::validate_name("a_b").is_err());
        assert!(Registry::validate_name("A-B").is_err());
        assert!(Registry::validate_name("a").is_ok());
        assert!(Registry::validate_name("a-0").is_ok());
    }
}
