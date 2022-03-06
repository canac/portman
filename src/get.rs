use crate::error::ApplicationError;
use crate::registry::PortRegistry;

// Lookup the port for a project, generating a new one if necessary
pub fn get_port(project: String) -> Result<u16, ApplicationError> {
    let mut registry = PortRegistry::load()?;
    registry.get(project)
}
