use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
/// Create a process-local mutex and register its one resource.
pub fn sys_mutex_create(blocking: bool) -> isize {
    let mutex: Arc<dyn Mutex> = if blocking {
        Arc::new(MutexBlocking::new())
    } else {
        Arc::new(MutexSpin::new())
    };
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let id = inner
        .mutex_list
        .iter()
        .position(Option::is_none)
        .unwrap_or(inner.mutex_list.len());
    if id == inner.mutex_list.len() {
        inner.mutex_list.push(Some(mutex));
    } else {
        inner.mutex_list[id] = Some(mutex);
    }
    inner.mutex_tracker.create(id, 1);
    id as isize
}
/// mutex lock syscall
/// Refuse an unsafe mutex request before invoking the real lock.
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let tid = super::thread::sys_gettid() as usize;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let Some(mutex) = inner
        .mutex_list
        .get(mutex_id)
        .and_then(Option::as_ref)
        .cloned()
    else {
        return -1;
    };
    let detect = inner.deadlock_enabled;
    if !inner.mutex_tracker.request(tid, mutex_id, detect) {
        return -0xDEAD;
    }
    drop(inner);
    mutex.lock();
    process
        .inner_exclusive_access()
        .mutex_tracker
        .acquired(tid, mutex_id);
    0
}
/// mutex unlock syscall
/// Release a mutex in both the detector and the actual primitive.
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let tid = super::thread::sys_gettid() as usize;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let Some(mutex) = inner
        .mutex_list
        .get(mutex_id)
        .and_then(Option::as_ref)
        .cloned()
    else {
        return -1;
    };
    inner.mutex_tracker.release(tid, mutex_id);
    drop(inner);
    mutex.unlock();
    0
}
/// semaphore create syscall
/// Create a semaphore and register its available resource count.
pub fn sys_semaphore_create(res_count: usize) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let id = inner
        .semaphore_list
        .iter()
        .position(Option::is_none)
        .unwrap_or(inner.semaphore_list.len());
    let semaphore = Some(Arc::new(Semaphore::new(res_count)));
    if id == inner.semaphore_list.len() {
        inner.semaphore_list.push(semaphore);
    } else {
        inner.semaphore_list[id] = semaphore;
    }
    inner.semaphore_tracker.create(id, res_count);
    id as isize
}
/// semaphore up syscall
/// Signal a semaphore, updating available resources before waking a waiter.
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let tid = super::thread::sys_gettid() as usize;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let Some(sem) = inner
        .semaphore_list
        .get(sem_id)
        .and_then(Option::as_ref)
        .cloned()
    else {
        return -1;
    };
    inner.semaphore_tracker.release(tid, sem_id);
    drop(inner);
    sem.up();
    0
}
/// semaphore down syscall
/// Check for deadlock before waiting on a semaphore.
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let tid = super::thread::sys_gettid() as usize;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let Some(sem) = inner
        .semaphore_list
        .get(sem_id)
        .and_then(Option::as_ref)
        .cloned()
    else {
        return -1;
    };
    let detect = inner.deadlock_enabled;
    if !inner.semaphore_tracker.request(tid, sem_id, detect) {
        return -0xDEAD;
    }
    drop(inner);
    sem.down();
    process
        .inner_exclusive_access()
        .semaphore_tracker
        .acquired(tid, sem_id);
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
/// Wait on a condition variable, reflecting its implicit mutex release/reacquire.
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    let tid = super::thread::sys_gettid() as usize;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let Some(condvar) = inner
        .condvar_list
        .get(condvar_id)
        .and_then(Option::as_ref)
        .cloned()
    else {
        return -1;
    };
    let Some(mutex) = inner
        .mutex_list
        .get(mutex_id)
        .and_then(Option::as_ref)
        .cloned()
    else {
        return -1;
    };
    inner.mutex_tracker.release(tid, mutex_id);
    drop(inner);
    condvar.wait(mutex);
    let mut inner = process.inner_exclusive_access();
    inner.mutex_tracker.request(tid, mutex_id, false);
    inner.mutex_tracker.acquired(tid, mutex_id);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
/// Enable or disable deadlock checks for the current process.
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    if enabled > 1 {
        return -1;
    }
    current_process().inner_exclusive_access().deadlock_enabled = enabled == 1;
    0
}
