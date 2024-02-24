use crate::caddy::reload;
use crate::dependencies::{ChoosePort, DataDir, Exec, ReadFile, WorkingDirectory, WriteFile};
use crate::error::{ApplicationError, Result};
use crate::{allocator::PortAllocator, dependencies::Environment};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

#[derive(Clone, Deserialize, Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq))]
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

#[cfg_attr(test, derive(Debug))]
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
        reload(deps, self).map_err(ApplicationError::Caddy)
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
            return Err(ApplicationError::DuplicateProject(name.to_string()));
        }

        if let Some(directory) = directory.as_ref() {
            if let Some((name, _)) = self
                .projects
                .iter()
                .find(|(_, project)| project.directory.as_ref() == Some(directory))
            {
                return Err(ApplicationError::DuplicateDirectory(
                    name.clone(),
                    directory.clone(),
                ));
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
    pub fn update(&mut self, name: &str, directory: Option<PathBuf>) -> Result<Project> {
        let project = self
            .projects
            .get_mut(name)
            .ok_or_else(|| ApplicationError::NonExistentProject(String::from(name)))?;
        if project.directory != directory {
            project.directory = directory;
            self.dirty = true;
        }
        Ok(project.clone())
    }

    // Delete a project and return the deleted project and its names
    pub fn delete(&mut self, name: &str) -> Result<Project> {
        let project = self
            .projects
            .remove(name)
            .ok_or_else(|| ApplicationError::NonExistentProject(String::from(name)))?;
        self.dirty = true;
        Ok(project)
    }

    // Delete multiple projects and return the deleted projects and their names
    pub fn delete_many(&mut self, project_names: Vec<String>) -> Result<Vec<(String, Project)>> {
        let deleted_projects: Vec<(String, Project)> = project_names
            .into_iter()
            .map(|name| {
                let project = self
                    .projects
                    .remove(&name)
                    .ok_or_else(|| ApplicationError::NonExistentProject(name.clone()))?;
                Ok((name, project))
            })
            .collect::<Result<Vec<_>>>()?;
        if !deleted_projects.is_empty() {
            self.dirty = true;
        }
        Ok(deleted_projects)
    }

    // Delete all projects
    pub fn delete_all(&mut self) {
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
            return Err(ApplicationError::NonExistentProject(String::from(
                project_name,
            )));
        }

        for (name, project) in &mut self.projects {
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

    // Unlink the port linked to a project and return the name of the project it was linked to
    pub fn unlink(&mut self, port: u16) -> Option<String> {
        for (name, project) in &mut self.projects {
            if project.linked_port == Some(port) {
                project.linked_port = None;
                self.dirty = true;
                return Some(name.clone());
            }
        }
        None
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
    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(ApplicationError::InvalidProjectName(
                String::from(name),
                "must not be empty",
            ));
        }
        if name.len() > 63 {
            return Err(ApplicationError::InvalidProjectName(
                String::from(name),
                "must not exceed 63 characters",
            ));
        }
        if name.starts_with('-') || name.ends_with('-') {
            return Err(ApplicationError::InvalidProjectName(
                String::from(name),
                "must not start or end with a dash",
            ));
        }
        if name.contains("--") {
            return Err(ApplicationError::InvalidProjectName(
                String::from(name),
                "must not contain consecutive dashes",
            ));
        }
        if name
            .chars()
            .any(|char| !(char.is_ascii_lowercase() || char.is_numeric() || char == '-'))
        {
            return Err(ApplicationError::InvalidProjectName(
                String::from(name),
                "must only contain lowercase alphanumeric characters and dashes",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::Config;
    use crate::dependencies::{self};
    use crate::mocks::{
        choose_port_mock, cwd_mock, data_dir_mock, get_mocked_registry, read_registry_mock,
        read_var_mock, write_file_mock,
    };
    use anyhow::bail;
    use unimock::{matching, Clause, MockFn, Unimock};

    fn read_caddyfile_mock() -> impl Clause {
        dependencies::ReadFileMock
            .each_call(matching!((path) if path == &PathBuf::from("/homebrew/etc/Caddyfile")))
            .answers(|_| Ok(None))
            .n_times(1)
    }

    #[test]
    fn test_load_invalid() {
        let config = Config::default();
        let mocked_deps = Unimock::new((data_dir_mock(), read_registry_mock(Some(";"))));
        let allocator = PortAllocator::new(config.get_valid_ports());
        let err = Registry::new(&mocked_deps, allocator).unwrap_err();
        assert!(matches!(err, ApplicationError::Other(_)));
    }

    #[test]
    fn test_load_invalid_name() {
        let config = Config::default();
        let mocked_deps = Unimock::new((
            data_dir_mock(),
            read_registry_mock(Some("projects.App1 = { port = 3001 }")),
        ));
        let allocator = PortAllocator::new(config.get_valid_ports());
        let err = Registry::new(&mocked_deps, allocator).unwrap_err();
        assert!(matches!(err, ApplicationError::InvalidProjectName(name, _) if name == "App1"));
    }

    #[test]
    fn test_load_duplicate_directory() {
        let config = Config::default();
        let mocked_deps = Unimock::new((
            data_dir_mock(),
            read_registry_mock(Some(
                "[projects]

[projects.app1]
port = 3001
directory = '/projects/app'

[projects.app2]
port = 3002
directory = '/projects/app'",
            )),
        ));
        let allocator = PortAllocator::new(config.get_valid_ports());
        let registry = Registry::new(&mocked_deps, allocator).unwrap();
        assert!(registry.get("app1").unwrap().directory.is_some());
        assert!(registry.get("app2").unwrap().directory.is_none());
        assert!(registry.dirty);
    }

    #[test]
    fn test_load_duplicate_linked() {
        let config = Config::default();
        let mocked_deps = Unimock::new((
            data_dir_mock(),
            read_registry_mock(Some(
                "[projects]

[projects.app1]
port = 3001
linked_port = 3000

[projects.app2]
port = 3002
linked_port = 3000",
            )),
        ));
        let allocator = PortAllocator::new(config.get_valid_ports());
        let registry = Registry::new(&mocked_deps, allocator).unwrap();
        assert!(registry.get("app1").unwrap().linked_port.is_some());
        assert!(registry.get("app2").unwrap().linked_port.is_none());
        assert!(registry.dirty);
    }

    #[test]
    fn test_load_reallocates_for_linked() {
        let config = Config::default();
        let mocked_deps = Unimock::new((
            choose_port_mock(),
            data_dir_mock(),
            read_registry_mock(Some("projects.app1 = { port = 3001, linked_port = 3001 }")),
        ));
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
        let mocked_deps = Unimock::new((
            choose_port_mock(),
            data_dir_mock(),
            read_registry_mock(None),
        ));
        let allocator = PortAllocator::new(config.get_valid_ports());
        let registry = Registry::new(&mocked_deps, allocator).unwrap();
        assert_eq!(registry.get("app1").unwrap().port, 4000);
        assert_eq!(registry.get("app2").unwrap().port, 4001);
        assert_eq!(registry.get("app3").unwrap().port, 4002);
        assert!(registry.dirty);
    }

    #[test]
    fn test_save_clean() {
        let mocked_deps = Unimock::new(());
        let registry = get_mocked_registry().unwrap();
        assert!(!registry.dirty);
        registry.save(&mocked_deps).unwrap();
    }

    #[test]
    fn test_save_caddy_read_failure() {
        let mocked_deps = Unimock::new((
            data_dir_mock(),
            dependencies::ReadFileMock
                .each_call(matching!((path) if path == &PathBuf::from("/homebrew/etc/Caddyfile")))
                .answers(|_| bail!("Error reading"))
                .n_times(1),
            read_var_mock(),
            write_file_mock(),
        ));
        let mut registry = get_mocked_registry().unwrap();
        registry.dirty = true;
        let err = registry.save(&mocked_deps).unwrap_err();
        assert!(matches!(err, ApplicationError::Caddy(_)));
    }

    #[test]
    fn test_save_caddy_write_portman_caddyfile_failure() {
        let mocked_deps = Unimock::new((
            data_dir_mock(),
            dependencies::WriteFileMock
                .each_call(matching!((path, _) if path == &PathBuf::from("/data/registry.toml")))
                .answers(|_| Ok(()))
                .n_times(1),
            dependencies::WriteFileMock
                .each_call(matching!((path, _) if path == &PathBuf::from("/data/Caddyfile")))
                .answers(|_| bail!("Error writing"))
                .n_times(1),
        ));
        let mut registry = get_mocked_registry().unwrap();
        registry.dirty = true;
        let err = registry.save(&mocked_deps).unwrap_err();
        assert!(matches!(err, ApplicationError::Caddy(_)));
    }

    #[test]
    fn test_save_caddy_write_root_caddyfile_failure() {
        let mocked_deps = Unimock::new((
            data_dir_mock(),
            read_caddyfile_mock(),
            read_var_mock(),
            dependencies::WriteFileMock
                .each_call(matching!((path, _) if path == &PathBuf::from("/data/registry.toml") || path == &PathBuf::from("/data/Caddyfile")))
                .answers(|_| Ok(()))
                .n_times(2),
            dependencies::WriteFileMock
                .each_call(
                    matching!((path, _) if path == &PathBuf::from("/homebrew/etc/Caddyfile")),
                )
                .answers(|_| bail!("Error writing"))
                .n_times(1),
        ));
        let mut registry = get_mocked_registry().unwrap();
        registry.dirty = true;
        let err = registry.save(&mocked_deps).unwrap_err();
        assert!(matches!(err, ApplicationError::Caddy(_)));
    }

    #[test]
    fn test_save_caddy_exec_failure() {
        let mocked_deps = Unimock::new((
            data_dir_mock(),
            dependencies::ExecMock
                .each_call(matching!((command, _) if command.get_program() == "caddy"))
                .answers(|_| bail!("Error executing"))
                .n_times(1),
            read_caddyfile_mock(),
            read_var_mock(),
            write_file_mock(),
        ));
        let mut registry = get_mocked_registry().unwrap();
        registry.dirty = true;
        let err = registry.save(&mocked_deps).unwrap_err();
        assert!(matches!(err, ApplicationError::Caddy(_)));
    }

    #[test]
    fn test_get() {
        let registry = get_mocked_registry().unwrap();
        assert_eq!(registry.get("app1").unwrap().port, 3001);
        assert!(registry.get("app4").is_none());
    }

    #[test]
    fn test_create() {
        let mocked_deps = Unimock::new(choose_port_mock());
        let mut registry = get_mocked_registry().unwrap();
        registry.create(&mocked_deps, "app4", None, None).unwrap();
        assert!(registry.get("app4").is_some());
        assert!(registry.dirty);
    }

    #[test]
    fn test_create_invalid_name() {
        let mocked_deps = Unimock::new(());
        let mut registry = get_mocked_registry().unwrap();
        let err = registry
            .create(&mocked_deps, "App3", None, None)
            .unwrap_err();
        assert!(matches!(err, ApplicationError::InvalidProjectName(_, _)));
        assert!(!registry.dirty);
    }

    #[test]
    fn test_create_duplicate_name() {
        let mocked_deps = Unimock::new(());
        let mut registry = get_mocked_registry().unwrap();
        let err = registry
            .create(&mocked_deps, "app3", None, None)
            .unwrap_err();
        assert!(matches!(err, ApplicationError::DuplicateProject(_)));
        assert!(!registry.dirty);
    }

    #[test]
    fn test_create_linked_port() {
        let mocked_deps = Unimock::new(choose_port_mock());
        let mut registry = get_mocked_registry().unwrap();
        registry
            .create(&mocked_deps, "app4", None, Some(3100))
            .unwrap();
        assert_eq!(registry.get("app4").unwrap().linked_port.unwrap(), 3100);
        assert!(registry.dirty);
    }

    #[test]
    fn test_create_linked_port_reallocates_previous() {
        let mocked_deps = Unimock::new(choose_port_mock());
        let mut registry = get_mocked_registry().unwrap();
        registry
            .create(&mocked_deps, "app4", None, Some(3001))
            .unwrap();
        assert_eq!(registry.get("app1").unwrap().port, 3005);
        assert!(registry.dirty);
    }

    #[test]
    fn test_create_duplicate_directory() {
        let mocked_deps = Unimock::new(());
        let mut registry = get_mocked_registry().unwrap();
        let result = registry.create(
            &mocked_deps,
            "app4",
            Some(PathBuf::from("/projects/app3")),
            None,
        );
        assert!(matches!(
            result,
            Err(ApplicationError::DuplicateDirectory(_, _)),
        ));
        assert!(!registry.dirty);
    }

    #[test]
    fn test_update() {
        let mut registry = get_mocked_registry().unwrap();
        let path = PathBuf::from("/projects/app2");
        assert_eq!(
            registry
                .update("app2", Some(path.clone()))
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
        let mut registry = get_mocked_registry().unwrap();
        let path = PathBuf::from("/projects/app3");
        assert_eq!(
            registry
                .update("app3", Some(path.clone()))
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
        let mut registry = get_mocked_registry().unwrap();
        let err = registry.update("app4", None).unwrap_err();
        assert!(matches!(err, ApplicationError::NonExistentProject(_)));
    }

    #[test]
    fn test_delete() {
        let mut registry = get_mocked_registry().unwrap();
        registry.delete("app2").unwrap();
        assert!(registry.get("app2").is_none());
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut registry = get_mocked_registry().unwrap();
        let err = registry.delete("app4").unwrap_err();
        assert!(matches!(err, ApplicationError::NonExistentProject(_)));
    }

    #[test]
    fn test_delete_many() {
        let mut registry = get_mocked_registry().unwrap();
        let deleted_projects = registry
            .delete_many(vec![String::from("app1"), String::from("app2")])
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
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.delete_many(vec![]).unwrap().is_empty());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_delete_all() {
        let mut registry = get_mocked_registry().unwrap();
        registry.delete_all();
        assert!(registry.projects.is_empty());
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_create() {
        let mocked_deps = Unimock::new(());
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app2", 3005).unwrap();
        assert_eq!(registry.get("app2").unwrap().linked_port.unwrap(), 3005);
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_change() {
        let mocked_deps = Unimock::new(());
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app2", 3100).unwrap();
        assert_eq!(registry.get("app2").unwrap().linked_port.unwrap(), 3100);
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_move() {
        let mocked_deps = Unimock::new(());
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app3", 3000).unwrap();
        assert!(registry.get("app2").unwrap().linked_port.is_none());
        assert_eq!(registry.get("app3").unwrap().linked_port.unwrap(), 3000);
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_noop() {
        let mocked_deps = Unimock::new(());
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app2", 3000).unwrap();
        assert!(!registry.dirty);
    }

    #[test]
    fn test_link_nonexistent() {
        let mocked_deps = Unimock::new(());
        let mut registry = get_mocked_registry().unwrap();
        let err = registry.link(&mocked_deps, "app4", 3004).unwrap_err();
        assert!(matches!(err, ApplicationError::NonExistentProject(_)));
        assert!(!registry.dirty);
    }

    #[test]
    fn test_link_reallocates_previous() {
        let mocked_deps = Unimock::new(choose_port_mock());
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app2", 3001).unwrap();
        assert_ne!(registry.get("app1").unwrap().port, 3001);
        assert_eq!(registry.get("app2").unwrap().linked_port.unwrap(), 3001);
        assert!(registry.dirty);
    }

    #[test]
    fn test_link_reallocates_self() {
        let mocked_deps = Unimock::new(choose_port_mock());
        let mut registry = get_mocked_registry().unwrap();
        registry.link(&mocked_deps, "app1", 3001).unwrap();
        assert_ne!(registry.get("app1").unwrap().port, 3001);
        assert_eq!(registry.get("app1").unwrap().linked_port.unwrap(), 3001);
        assert!(registry.dirty);
    }

    #[test]
    fn test_unlink() {
        let mut registry = get_mocked_registry().unwrap();
        assert_eq!(registry.unlink(3000).unwrap(), String::from("app2"));
        assert!(registry.dirty);
    }

    #[test]
    fn test_unlink_not_linked() {
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry.unlink(3001).is_none());
        assert!(!registry.dirty);
    }

    #[test]
    fn test_match_cwd_dir() {
        let mocked_deps = Unimock::new(cwd_mock("app3"));
        let registry = get_mocked_registry().unwrap();
        assert_eq!(registry.match_cwd(&mocked_deps).unwrap().unwrap().0, "app3");
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
        assert!(matches!(
            Registry::validate_name("").unwrap_err(),
            ApplicationError::InvalidProjectName(_, reason) if reason == "must not be empty",
        ));
        assert!(matches!(
            Registry::validate_name(&"a".repeat(64)).unwrap_err(),
            ApplicationError::InvalidProjectName(_, reason) if reason == "must not exceed 63 characters",
        ));
        assert!(matches!(
            Registry::validate_name("-a").unwrap_err(),
            ApplicationError::InvalidProjectName(_, reason) if reason == "must not start or end with a dash",
        ));
        assert!(matches!(
            Registry::validate_name("a-").unwrap_err(),
            ApplicationError::InvalidProjectName(_, reason) if reason == "must not start or end with a dash",
        ));
        assert!(matches!(
            Registry::validate_name("a--b").unwrap_err(),
            ApplicationError::InvalidProjectName(_, reason) if reason == "must not contain consecutive dashes",
        ));
        assert!(matches!(
            Registry::validate_name("a_b").unwrap_err(),
            ApplicationError::InvalidProjectName(_, reason) if reason == "must only contain lowercase alphanumeric characters and dashes",
        ));
        assert!(matches!(
            Registry::validate_name("A-B").unwrap_err(),
            ApplicationError::InvalidProjectName(_, reason) if reason == "must only contain lowercase alphanumeric characters and dashes",
        ));
        assert!(Registry::validate_name("a").is_ok());
        assert!(Registry::validate_name("a-0").is_ok());
    }
}
