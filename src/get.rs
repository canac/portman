use crate::registry::PortRegistry;

// Lookup the port for a project, generating a new one if necessary
pub fn get_port(project: String) -> u16 {
    let mut registry = PortRegistry::load().unwrap();
    registry.get(project).unwrap()
}
