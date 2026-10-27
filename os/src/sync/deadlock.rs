//! Track allocations and outstanding requests for deadlock detection.
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

/// Allocation and request matrices for one family of process-local resources.
pub struct ResourceTracker {
    available: Vec<usize>,
    allocations: BTreeMap<usize, Vec<usize>>,
    requests: BTreeMap<usize, Vec<usize>>,
}

impl ResourceTracker {
    /// Create an empty resource family.
    pub fn new() -> Self {
        Self {
            available: Vec::new(),
            allocations: BTreeMap::new(),
            requests: BTreeMap::new(),
        }
    }

    /// Register a newly created resource, indexed by its syscall id.
    pub fn create(&mut self, id: usize, count: usize) {
        let width = self.available.len().max(id + 1);
        self.available.resize(width, 0);
        self.available[id] = count;
        for row in self.allocations.values_mut() {
            row.resize(width, 0);
            row[id] = 0;
        }
        for row in self.requests.values_mut() {
            row.resize(width, 0);
            row[id] = 0;
        }
    }

    fn row(&mut self, tid: usize) {
        let width = self.available.len();
        self.allocations
            .entry(tid)
            .or_insert_with(|| vec![0; width]);
        self.requests.entry(tid).or_insert_with(|| vec![0; width]);
    }

    /// Record a pending request; reject it without side effects if unsafe.
    pub fn request(&mut self, tid: usize, id: usize, detect: bool) -> bool {
        if id >= self.available.len() {
            return false;
        }
        self.row(tid);
        self.requests.get_mut(&tid).unwrap()[id] += 1;
        if detect && !self.safe() {
            self.requests.get_mut(&tid).unwrap()[id] -= 1;
            return false;
        }
        true
    }

    fn safe(&self) -> bool {
        let mut work = self.available.clone();
        let mut finished = BTreeMap::new();
        loop {
            let mut progress = false;
            for (&tid, allocation) in &self.allocations {
                if finished.contains_key(&tid) {
                    continue;
                }
                let request = &self.requests[&tid];
                if request.iter().zip(&work).all(|(&need, &free)| need <= free) {
                    for (free, &held) in work.iter_mut().zip(allocation) {
                        *free += held;
                    }
                    finished.insert(tid, ());
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
        finished.len() == self.allocations.len()
    }

    /// Finish a previously recorded request after the real primitive grants it.
    pub fn acquired(&mut self, tid: usize, id: usize) {
        self.row(tid);
        self.requests.get_mut(&tid).unwrap()[id] -= 1;
        self.available[id] -= 1;
        self.allocations.get_mut(&tid).unwrap()[id] += 1;
    }

    /// Release a resource, including semaphore signals by a non-owning producer.
    pub fn release(&mut self, tid: usize, id: usize) {
        self.row(tid);
        let held = &mut self.allocations.get_mut(&tid).unwrap()[id];
        *held = held.saturating_sub(1);
        self.available[id] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recursive_mutex_is_rejected() {
        let mut state = ResourceTracker::new();
        state.create(0, 1);
        assert!(state.request(0, 0, true));
        state.acquired(0, 0);
        assert!(!state.request(0, 0, true));
        state.release(0, 0);
        assert!(state.request(0, 0, true));
    }
    #[test]
    fn cycle_is_rejected_and_rolled_back() {
        let mut state = ResourceTracker::new();
        state.create(0, 1);
        state.create(1, 1);
        for (tid, id) in [(1, 0), (2, 1)] {
            assert!(state.request(tid, id, true));
            state.acquired(tid, id);
        }
        assert!(state.request(1, 1, true));
        assert!(!state.request(2, 0, true));
        state.release(2, 1);
        state.acquired(1, 1);
        state.release(1, 1);
        state.release(1, 0);
    }
    #[test]
    fn unavailable_resource_can_be_released_by_another_thread() {
        let mut state = ResourceTracker::new();
        state.create(0, 1);
        assert!(state.request(0, 0, true));
        state.acquired(0, 0);
        assert!(state.request(1, 0, true));
        state.release(0, 0);
        state.acquired(1, 0);
    }
}
