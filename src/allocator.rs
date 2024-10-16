use crate::dependencies::ChoosePort;
use crate::error::{ApplicationError, Result};
use std::collections::HashSet;

#[cfg_attr(test, derive(Debug))]
pub struct PortAllocator {
    available_ports: HashSet<u16>,
}

impl PortAllocator {
    // Create a new port allocator that allocates from the provided available ports
    pub fn new(available_ports: impl Iterator<Item = u16>) -> Self {
        Self {
            available_ports: available_ports.collect(),
        }
    }

    // Remove a port from the pool of available ports
    pub fn discard(&mut self, port: u16) {
        self.available_ports.remove(&port);
    }

    // Allocate a new port, using the desired port if it is provided and is valid
    pub fn allocate(&mut self, deps: &impl ChoosePort, desired_port: Option<u16>) -> Result<u16> {
        let allocated_port = desired_port
            .and_then(|port| {
                if self.available_ports.contains(&port) {
                    Some(port)
                } else {
                    None
                }
            })
            .or_else(|| deps.choose_port(&self.available_ports));
        let Some(port) = allocated_port else {
            return Err(ApplicationError::EmptyAllocator);
        };
        self.available_ports.remove(&port);
        Ok(port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::choose_port_mock;
    use unimock::Unimock;

    #[test]
    fn test_random_chooser() {
        let range = 3000..=3999;
        let mut allocator = PortAllocator::new(range.clone());
        let mocked_deps = Unimock::new(choose_port_mock());
        assert!(range.contains(&allocator.allocate(&mocked_deps, None).unwrap()));
    }

    #[test]
    fn test_discard() {
        let mut allocator = PortAllocator::new(3000..=3001);
        let mocked_deps = Unimock::new(choose_port_mock());
        allocator.discard(3000);
        assert_eq!(allocator.allocate(&mocked_deps, None).unwrap(), 3001);
        assert!(matches!(
            allocator.allocate(&mocked_deps, None),
            Err(ApplicationError::EmptyAllocator),
        ));
    }

    #[test]
    fn test_allocate() {
        let mut allocator = PortAllocator::new(3000..=3001);
        let mocked_deps = Unimock::new(choose_port_mock());
        assert!(allocator.allocate(&mocked_deps, None).is_ok());
        assert!(allocator.allocate(&mocked_deps, None).is_ok());
        assert!(matches!(
            allocator.allocate(&mocked_deps, None),
            Err(ApplicationError::EmptyAllocator),
        ));
    }

    #[test]
    fn test_desired_port() {
        let mut allocator = PortAllocator::new(3000..=3002);
        let mocked_deps = Unimock::new(choose_port_mock());
        assert_eq!(allocator.allocate(&mocked_deps, Some(3001)).unwrap(), 3001);
        assert_eq!(allocator.allocate(&mocked_deps, Some(4000)).unwrap(), 3000);
        assert_eq!(allocator.allocate(&mocked_deps, None).unwrap(), 3002);
        assert!(matches!(
            allocator.allocate(&mocked_deps, None),
            Err(ApplicationError::EmptyAllocator),
        ));
    }
}
