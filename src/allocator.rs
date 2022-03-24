use rand::prelude::*;
use std::collections::HashSet;

pub struct PortAllocator {
    available_ports: HashSet<u16>,
    rng: ThreadRng,
}

impl PortAllocator {
    // Create a new port allocator that allocates from the provided available ports
    pub fn new(available_ports: impl Iterator<Item = u16>) -> Self {
        PortAllocator {
            available_ports: available_ports.collect(),
            rng: thread_rng(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocator() {
        let mut allocator = PortAllocator::new(vec![3000, 3001].into_iter());
        let ports = (allocator.allocate(None), allocator.allocate(None));
        assert!(ports == (Some(3000), Some(3001)) || ports == (Some(3001), Some(3000)));
        assert_eq!(allocator.allocate(None), None);
    }

    #[test]
    fn test_desired_port() {
        let mut allocator = PortAllocator::new(vec![3000, 3001].into_iter());
        assert_eq!(allocator.allocate(Some(3001)), Some(3001));
        assert_eq!(allocator.allocate(Some(4000)), Some(3000));
        assert_eq!(allocator.allocate(None), None);
    }
}
