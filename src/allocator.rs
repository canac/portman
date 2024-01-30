use crate::dependencies::ChoosePort;
use anyhow::{bail, Result};
use std::collections::HashSet;

pub struct PortAllocator {
    available_ports: HashSet<u16>,
}

impl PortAllocator {
    // Create a new port allocator that allocates from the provided available ports
    pub fn new(available_ports: impl Iterator<Item = u16>) -> Self {
        PortAllocator {
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
            bail!("All available ports have been allocated already")
        };
        self.available_ports.remove(&port);
        Ok(port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependencies;
    use unimock::{matching, MockFn, Unimock};

    fn get_deps() -> Unimock {
        unimock::mock([dependencies::choose_port::Fn
            .each_call(matching!(_))
            .answers(|available_ports| available_ports.iter().min().copied())
            .in_any_order()])
    }

    #[test]
    fn test_random_chooser() {
        let range = 3000..=3999;
        let mut allocator = PortAllocator::new(range.clone());
        let deps = get_deps();
        assert!(range.contains(&allocator.allocate(&deps, None).unwrap()));
    }

    #[test]
    fn test_discard() {
        let mut allocator = PortAllocator::new(3000..=3001);
        let deps = get_deps();
        allocator.discard(3000);
        assert_eq!(allocator.allocate(&deps, None).unwrap(), 3001);
        assert!(allocator.allocate(&deps, None).is_err());
    }

    #[test]
    fn test_allocate() {
        let mut allocator = PortAllocator::new(3000..=3001);
        let deps = get_deps();
        assert!(allocator.allocate(&deps, None).is_ok());
        assert!(allocator.allocate(&deps, None).is_ok());
        assert!(allocator.allocate(&deps, None).is_err());
    }

    #[test]
    fn test_desired_port() {
        let mut allocator = PortAllocator::new(3000..=3002);
        let deps = get_deps();
        assert_eq!(allocator.allocate(&deps, Some(3001)).unwrap(), 3001);
        assert_eq!(allocator.allocate(&deps, Some(4000)).unwrap(), 3000);
        assert_eq!(allocator.allocate(&deps, None).unwrap(), 3002);
        assert!(allocator.allocate(&deps, None).is_err());
    }
}
