use crate::dependencies::ChoosePort;
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

    // Allocate a new port, using the desired port if it is provided and is valid
    pub fn allocate(&mut self, deps: &impl ChoosePort, desired_port: Option<u16>) -> Option<u16> {
        let allocated_port = desired_port
            .and_then(|port| {
                if self.available_ports.contains(&port) {
                    Some(port)
                } else {
                    None
                }
            })
            .or_else(|| deps.choose_port(&self.available_ports));
        if let Some(port) = allocated_port {
            self.available_ports.remove(&port);
        }
        allocated_port
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
            .answers(|available_ports| available_ports.iter().min().cloned())
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
    fn test_allocate() {
        let mut allocator = PortAllocator::new(3000..=3001);
        let deps = get_deps();
        assert!(allocator.allocate(&deps, None).is_some());
        assert!(allocator.allocate(&deps, None).is_some());
        assert_eq!(allocator.allocate(&deps, None), None);
    }

    #[test]
    fn test_desired_port() {
        let mut allocator = PortAllocator::new(3000..=3002);
        let deps = get_deps();
        assert_eq!(allocator.allocate(&deps, Some(3001)), Some(3001));
        assert_eq!(allocator.allocate(&deps, Some(4000)), Some(3000));
        assert_eq!(allocator.allocate(&deps, None), Some(3002));
        assert_eq!(allocator.allocate(&deps, None), None);
    }
}
