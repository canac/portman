use rand::prelude::*;
use std::collections::HashSet;

pub trait PortChooser {
    fn choose(&mut self, available_ports: &HashSet<u16>) -> Option<u16>;
}

pub struct PortAllocator {
    available_ports: HashSet<u16>,
    chooser: Box<dyn PortChooser>,
}

impl PortAllocator {
    // Create a new port allocator that allocates from the provided available ports
    pub fn new(
        available_ports: impl Iterator<Item = u16>,
        chooser: impl PortChooser + 'static,
    ) -> Self {
        PortAllocator {
            available_ports: available_ports.collect(),
            chooser: Box::new(chooser),
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
            .or_else(|| self.chooser.choose(&self.available_ports));
        if let Some(port) = allocated_port {
            self.available_ports.remove(&port);
        }
        allocated_port
    }
}

pub struct RandomPortChooser {
    rng: ThreadRng,
}

impl PortChooser for RandomPortChooser {
    fn choose(&mut self, available_ports: &HashSet<u16>) -> Option<u16> {
        available_ports.iter().choose(&mut self.rng).cloned()
    }
}

impl RandomPortChooser {
    // Create a new port allocator that allocates from the provided available ports
    pub fn new() -> Self {
        RandomPortChooser { rng: thread_rng() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SequentialPortChooser always chooses the smallest available port
    struct SequentialPortChooser;
    impl PortChooser for SequentialPortChooser {
        fn choose(&mut self, available_ports: &std::collections::HashSet<u16>) -> Option<u16> {
            available_ports.iter().min().cloned()
        }
    }

    #[test]
    fn test_random_chooser() {
        let range = 3000..=3999;
        let mut allocator = PortAllocator::new(range.clone(), RandomPortChooser::new());
        assert!(range.contains(&allocator.allocate(None).unwrap()));
    }

    #[test]
    fn test_allocate() {
        let mut allocator = PortAllocator::new(3000..=3001, SequentialPortChooser {});
        assert_eq!(allocator.allocate(None), Some(3000));
        assert_eq!(allocator.allocate(None), Some(3001));
        assert_eq!(allocator.allocate(None), None);
    }

    #[test]
    fn test_desired_port() {
        let mut allocator = PortAllocator::new(3000..=3002, SequentialPortChooser {});
        assert_eq!(allocator.allocate(Some(3001)), Some(3001));
        assert_eq!(allocator.allocate(Some(4000)), Some(3000));
        assert_eq!(allocator.allocate(None), Some(3002));
        assert_eq!(allocator.allocate(None), None);
    }
}
