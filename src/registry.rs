use crate::dependencies::{ChoosePort, Exec, ReadFile, WorkingDirectory, WriteFile};
use crate::matcher::Matcher;
use crate::{allocator::PortAllocator, dependencies::Environment};
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, process::Command};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Project {
    pub port: u16,

    // If pinned, this port won't ever be changed, if it is not an available
    // port according to the config
    #[serde(default)]
    pub pinned: bool,

    // Use redirection instead of a reverse-proxy in the Caddyfile
    #[serde(default)]
    pub redirect: bool,

    pub matcher: Option<Matcher>,
}

// The port registry data that will be serialized and deserialized in the database
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct RegistryData {
    pub projects: BTreeMap<String, Project>,
}

pub struct PortRegistry {
    store_path: PathBuf,
    projects: BTreeMap<String, Project>,
    allocator: PortAllocator,
}

impl PortRegistry {
    // Create a new port registry
    pub fn new(
        deps: &(impl ChoosePort + Environment + Exec + ReadFile + WriteFile),
        store_path: PathBuf,
        port_allocator: PortAllocator,
    ) -> Result<Self> {
        let registry_data = deps
            .read_file(&store_path)
            .context("Failed to load registry")?
            .map(|registry_str| {
                toml::from_str::<RegistryData>(&registry_str).with_context(|| {
                    format!(
                        "Failed to deserialize project registry at \"{}\"",
                        store_path.to_string_lossy()
                    )
                })
            })
            .transpose()?
            .unwrap_or_default();

        // Validate all ports in the registry against the required config and
        // regenerate invalid ones as necessary
        let mut changed = false;
        let mut allocator = port_allocator;
        let validated_projects = registry_data
            .projects
            .into_iter()
            .map(|(name, old_project)| {
                if old_project.pinned {
                    // Don't reallocate the project's port if the port is pinned
                    Some((name, old_project))
                } else {
                    allocator
                        .allocate(deps, Some(old_project.port))
                        .map(|port| {
                            if port != old_project.port {
                                changed = true;
                            }
                            (
                                name,
                                Project {
                                    port,
                                    ..old_project
                                },
                            )
                        })
                }
            })
            .collect::<Option<BTreeMap<_, _>>>()
            .ok_or_else(|| anyhow!("All available ports have been allocated already"))?;
        let registry = Self {
            store_path,
            projects: validated_projects,
            allocator,
        };
        if changed {
            registry.save(deps)?;
        }
        Ok(registry)
    }

    // Save a port registry to the file
    pub fn save(&self, deps: &(impl Environment + Exec + ReadFile + WriteFile)) -> Result<()> {
        let registry = RegistryData {
            projects: self.projects.clone(),
        };
        let registry_str =
            toml::to_string(&registry).context("Failed to serialize project registry")?;
        deps.write_file(&self.store_path, &registry_str)
            .context("Failed to save registry")?;
        if let Err(err) = self.reload_caddy(deps) {
            // An error reloading Caddy is just a warning, not a fatal error
            println!("Warning: couldn't reload Caddy config.\n\n{err}");
        }
        Ok(())
    }

    // Get a project from the registry
    pub fn get(&self, name: &String) -> Option<&Project> {
        self.projects.get(name)
    }

    // Allocate a port to a new project
    pub fn allocate(
        &mut self,
        deps: &(impl ChoosePort + Environment + Exec + ReadFile + WriteFile),
        name: String,
        port: Option<u16>,
        redirect: bool,
        matcher: Option<Matcher>,
    ) -> Result<Project> {
        if self.projects.get(&name).is_some() {
            bail!("Project \"{name}\" already exists");
        }

        if let Some(matcher) = matcher.as_ref() {
            if self.projects.values().any(|project| {
                project
                    .matcher
                    .as_ref()
                    .map_or(false, |existing_matcher| existing_matcher == matcher)
            }) {
                bail!("Project with matcher \"{matcher}\" already exists");
            }
        }

        let new_port = match port {
            Some(port) => port,
            None => self
                .allocator
                .allocate(deps, None)
                .ok_or_else(|| anyhow!("Failed to choose a port"))?,
        };
        let new_project = Project {
            port: new_port,
            pinned: port.is_some(),
            redirect,
            matcher,
        };
        self.projects.insert(name, new_project.clone());
        self.save(deps)?;
        Ok(new_project)
    }

    // Release a previously allocated project's port
    pub fn release(
        &mut self,
        deps: &(impl Environment + Exec + ReadFile + WriteFile),
        name: &String,
    ) -> Result<Project> {
        match self.projects.remove(name) {
            Some(project) => {
                self.save(deps)?;
                Ok(project)
            }
            None => Err(anyhow!("Project \"{name}\" does not exist")),
        }
    }

    // Release all previously allocated projects
    pub fn release_all(
        &mut self,
        deps: &(impl Environment + Exec + ReadFile + WriteFile),
    ) -> Result<()> {
        self.projects = BTreeMap::new();
        self.save(deps)
    }

    // Iterate over all projects with their names
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Project)> {
        self.projects.iter()
    }

    // Find and return the project that matches the current working directory, if any
    pub fn match_cwd(
        &self,
        deps: &(impl Environment + Exec + WorkingDirectory),
    ) -> Option<(&String, &Project)> {
        self.iter().find(|(_, project)| {
            project
                .matcher
                .as_ref()
                .map_or(false, |matcher| matcher.matches_cwd(deps).unwrap_or(false))
        })
    }

    // Return the generated Caddyfile
    pub fn caddyfile(&self) -> String {
        let caddyfile = self
            .projects
            .iter()
            .map(|(name, project)| {
                let action = if project.redirect {
                    format!("redir http://127.0.0.1:{}", project.port)
                } else {
                    format!("reverse_proxy 127.0.0.1:{}", project.port)
                };
                format!("{name}.localhost {{\n\t{action}\n}}\n")
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("# portman begin\n# WARNING: This section is automatically generated by portman. Any manual edits will be overridden.\n\n{caddyfile}# portman end\n")
    }

    // Merge the registry's caddyfile into the existing caddyfile
    fn merge_caddyfile(&self, existing_caddyfile: Option<&str>) -> String {
        // Merge the portman caddyfile section into the existing caddyfile
        let portman_caddyfile = self.caddyfile();
        lazy_static::lazy_static! {
            static ref RE: Regex =
            Regex::new(r"# portman begin\n[\s\S+]*# portman end\n").unwrap();
        }
        existing_caddyfile
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
            .unwrap_or(portman_caddyfile)
    }

    // Reload the caddy service with the current port registry
    pub fn reload_caddy(
        &self,
        deps: &(impl Environment + Exec + ReadFile + WriteFile),
    ) -> Result<()> {
        // Determine the caddyfile path
        let brew_prefix = deps.read_var("HOMEBREW_PREFIX")?;
        let caddyfile_path = PathBuf::from(brew_prefix).join("etc").join("Caddyfile");

        // Read the existing caddyfile so that we can augment it with the portman caddyfile entries
        let existing_caddyfile = deps.read_file(&caddyfile_path).with_context(|| {
            format!(
                "Failed to read Caddyfile at \"{}\"",
                caddyfile_path.to_string_lossy()
            )
        })?;
        deps.write_file(
            &caddyfile_path,
            &self.merge_caddyfile(existing_caddyfile.as_deref()),
        )
        .with_context(|| {
            format!(
                "Failed to write Caddyfile at \"{}\"",
                caddyfile_path.to_string_lossy()
            )
        })?;

        // Reload the caddy config using the new Caddyfile
        let (status, _) = deps.exec(
            Command::new("caddy")
                .args(["reload", "--adapter", "caddyfile", "--config"])
                .arg(caddyfile_path),
        )?;
        if status.success() {
            Ok(())
        } else {
            bail!(
                "Failed to execute \"caddy reload\", failed with error code {}",
                match status.code() {
                    Some(code) => code.to_string(),
                    None => String::from("unknown"),
                }
            )
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::dependencies;
    use std::os::unix::process::ExitStatusExt;
    use unimock::{matching, MockFn};

    const REGISTRY: &str = "
[projects]

[projects.app1]
port = 3001

[projects.app2]
port = 3002
pinned = true

[projects.app2.matcher]
type = 'git_repository'
repository = 'https://github.com/user/app2.git'

[projects.app3]
port = 3003
redirect = true

[projects.app3.matcher]
type = 'directory'
directory = '/projects/app3'
";

    fn choose_port_mock() -> unimock::Clause {
        dependencies::choose_port::Fn
            .each_call(matching!(_))
            .answers(|available_ports| available_ports.iter().min().cloned())
            .in_any_order()
    }

    fn exec_mock() -> unimock::Clause {
        dependencies::exec::Fn
            .each_call(matching!(_))
            .answers(|_| Ok((ExitStatusExt::from_raw(0), String::new())))
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

    fn write_file_mock() -> unimock::Clause {
        dependencies::write_file::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(()))
            .in_any_order()
    }

    fn get_mocked_registry() -> Result<PortRegistry> {
        let mocked_deps = unimock::mock([read_file_mock()]);
        let config = Config::default();
        let mock_allocator = PortAllocator::new(config.get_valid_ports());
        PortRegistry::new(&mocked_deps, PathBuf::from("registry.toml"), mock_allocator)
    }

    #[test]
    fn test_load_invalid() {
        let config = Config::default();
        let mocked_deps = unimock::mock([dependencies::read_file::Fn
            .each_call(matching!(_))
            .answers(|_| Ok(Some(String::from(";"))))
            .in_any_order()]);
        let mock_allocator = PortAllocator::new(config.get_valid_ports());
        assert!(PortRegistry::new(&mocked_deps, PathBuf::from(""), mock_allocator).is_err());
    }

    #[test]
    fn test_load_normalizes() -> Result<()> {
        let config = Config {
            ranges: vec![(4000, 4999)],
            ..Default::default()
        };
        let mocked_deps = unimock::mock([
            choose_port_mock(),
            exec_mock(),
            read_var_mock(),
            read_file_mock(),
            write_file_mock(),
        ]);
        let mock_allocator = PortAllocator::new(config.get_valid_ports());
        let registry = PortRegistry::new(&mocked_deps, PathBuf::from(""), mock_allocator)?;
        assert_eq!(registry.projects.get("app1").unwrap().port, 4000);
        assert_eq!(registry.projects.get("app2").unwrap().port, 3002);
        assert_eq!(registry.projects.get("app3").unwrap().port, 4001);
        Ok(())
    }

    #[test]
    fn test_get() -> Result<()> {
        let registry = get_mocked_registry()?;
        assert_eq!(registry.get(&String::from("app1")).unwrap().port, 3001);
        assert!(registry.get(&String::from("app4")).is_none());
        Ok(())
    }

    #[test]
    fn test_allocate() -> Result<()> {
        let mocked_deps = unimock::mock([
            choose_port_mock(),
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry()?;
        registry.allocate(&mocked_deps, String::from("app4"), None, false, None)?;
        assert!(registry.projects.get(&String::from("app4")).is_some());
        Ok(())
    }

    #[test]
    fn test_allocate_duplicate_name() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry
            .allocate(&mocked_deps, String::from("app3"), None, false, None)
            .is_err());
    }

    #[test]
    fn test_allocate_with_port() {
        let mocked_deps = unimock::mock([
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        assert_eq!(
            registry
                .allocate(&mocked_deps, String::from("app4"), Some(3100), false, None)
                .unwrap()
                .port,
            3100
        );
    }

    #[test]
    fn test_allocate_with_duplicate_port() {
        let mocked_deps = unimock::mock([
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        assert_eq!(
            registry
                .allocate(&mocked_deps, String::from("app4"), Some(3001), false, None)
                .unwrap(),
            Project {
                port: 3001,
                pinned: true,
                redirect: false,
                matcher: None,
            }
        );
    }

    #[test]
    fn test_allocate_duplicate_matcher() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry
            .allocate(
                &mocked_deps,
                String::from("app4"),
                None,
                false,
                Some(Matcher::Directory {
                    directory: PathBuf::from("/projects/app3")
                }),
            )
            .is_err());
    }

    #[test]
    fn test_allocate_redirect() {
        let mocked_deps = unimock::mock([
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(
            registry
                .allocate(&mocked_deps, String::from("app4"), Some(3100), true, None)
                .unwrap()
                .redirect,
        );
    }

    #[test]
    fn test_allocate_caddy_read_failure() {
        let mocked_deps = unimock::mock([
            choose_port_mock(),
            dependencies::read_file::Fn
                .each_call(matching!(_))
                .answers(|_| bail!("Error reading"))
                .in_any_order(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry
            .allocate(&mocked_deps, String::from("app4"), None, false, None,)
            .is_ok());
    }

    #[test]
    fn test_allocate_caddy_write_failure() {
        let mocked_deps = unimock::mock([
            choose_port_mock(),
            read_file_mock(),
            read_var_mock(),
            dependencies::write_file::Fn
                .next_call(matching!((path, _) if path == &PathBuf::from("registry.toml")))
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
        assert!(registry
            .allocate(&mocked_deps, String::from("app4"), None, false, None,)
            .is_ok());
    }

    #[test]
    fn test_allocate_caddy_exec_failure() {
        let mocked_deps = unimock::mock([
            choose_port_mock(),
            dependencies::exec::Fn
                .each_call(matching!(_))
                .answers(|_| Ok((ExitStatusExt::from_raw(1), String::new())))
                .in_any_order(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry
            .allocate(&mocked_deps, String::from("app4"), None, false, None)
            .is_ok());
    }

    #[test]
    fn test_release() -> Result<()> {
        let mocked_deps = unimock::mock([
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry()?;
        registry.release(&mocked_deps, &String::from("app2"))?;
        assert!(registry.projects.get(&String::from("app2")).is_none());
        Ok(())
    }

    #[test]
    fn test_release_nonexistent() {
        let mocked_deps = unimock::mock([]);
        let mut registry = get_mocked_registry().unwrap();
        assert!(registry
            .release(&mocked_deps, &String::from("app4"))
            .is_err());
    }

    #[test]
    fn test_release_all() -> Result<()> {
        let mocked_deps = unimock::mock([
            exec_mock(),
            read_file_mock(),
            read_var_mock(),
            write_file_mock(),
        ]);
        let mut registry = get_mocked_registry()?;
        registry.release_all(&mocked_deps)?;
        assert!(registry.projects.is_empty());
        Ok(())
    }

    #[test]
    fn test_match_cwd_dir() {
        let mocked_deps = unimock::mock([
            dependencies::get_cwd::Fn
                .each_call(matching!(_))
                .answers(|_| Ok(PathBuf::from("/projects/app3")))
                .in_any_order(),
            exec_mock(),
        ]);
        let registry = get_mocked_registry().unwrap();
        assert_eq!(
            registry.match_cwd(&mocked_deps).unwrap().0,
            &String::from("app3")
        )
    }

    #[test]
    fn test_match_cwd_git() {
        let mocked_deps = unimock::mock([dependencies::exec::Fn
            .each_call(matching!(_))
            .answers(|_| {
                Ok((
                    ExitStatusExt::from_raw(0),
                    String::from("https://github.com/user/app2.git"),
                ))
            })
            .in_any_order()]);
        let registry = get_mocked_registry().unwrap();
        assert_eq!(
            registry.match_cwd(&mocked_deps).unwrap().0,
            &String::from("app2")
        )
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
\tredir http://127.0.0.1:3003
}
# portman end
";

    #[test]
    fn test_caddyfile() -> Result<()> {
        let registry = get_mocked_registry()?;
        assert_eq!(registry.caddyfile(), GOLDEN_CADDYFILE);
        Ok(())
    }

    #[test]
    fn test_merge_caddyfile_no_existing() -> Result<()> {
        let registry = get_mocked_registry()?;
        assert_eq!(registry.merge_caddyfile(None), GOLDEN_CADDYFILE);
        Ok(())
    }

    #[test]
    fn test_merge_caddyfile_update() -> Result<()> {
        let registry = get_mocked_registry()?;
        assert_eq!(
            registry.merge_caddyfile(Some(
                "# Prefix\n\n# portman begin\n# portman end\n\n# Suffix\n"
            )),
            format!("# Prefix\n\n{}\n# Suffix\n", GOLDEN_CADDYFILE)
        );
        Ok(())
    }

    #[test]
    fn test_merge_caddyfile_prepend() -> Result<()> {
        let registry = get_mocked_registry()?;
        assert_eq!(
            registry.merge_caddyfile(Some("# Suffix\n")),
            format!("{}\n# Suffix\n", GOLDEN_CADDYFILE)
        );
        Ok(())
    }
}
