//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;

const BIG_STRIDE: usize = 1 << 20;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Fetch the ready process with the smallest stride.
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let mut best = None;
        let mut best_stride = usize::MAX;
        for (index, task) in self.ready_queue.iter().enumerate() {
            let stride = task.inner_exclusive_access().stride;
            if stride < best_stride {
                best = Some(index);
                best_stride = stride;
            }
        }
        let task = self.ready_queue.remove(best?)?;
        let mut inner = task.inner_exclusive_access();
        let pass = (BIG_STRIDE / inner.priority.max(2)).max(1);
        inner.stride = inner.stride.saturating_add(pass);
        drop(inner);
        Some(task)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
