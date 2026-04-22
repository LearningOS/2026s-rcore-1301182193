use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::{sync::Arc, vec, vec::Vec};

const EDEADLOCK: isize = -0xdead;

fn current_tid() -> usize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid
}

fn ensure_thread_row(matrix: &mut Vec<Vec<usize>>, tid: usize, res_count: usize) {
    while matrix.len() <= tid {
        matrix.push(vec![0; res_count]);
    }
    for row in matrix.iter_mut() {
        if row.len() < res_count {
            row.resize(res_count, 0);
        }
    }
}

fn detect_deadlock(available: &[usize], need: &[Vec<usize>], allocation: &[Vec<usize>]) -> bool {
    let thread_count = need.len().max(allocation.len());
    let res_count = available.len();
    let mut work = available.to_vec();
    let mut finish = vec![false; thread_count];
    loop {
        let mut progressed = false;
        for tid in 0..thread_count {
            if finish[tid] {
                continue;
            }
            let can_finish = (0..res_count).all(|res| {
                let req = need
                    .get(tid)
                    .and_then(|row| row.get(res))
                    .copied()
                    .unwrap_or(0);
                req <= work[res]
            });
            if can_finish {
                for res in 0..res_count {
                    let alloc = allocation
                        .get(tid)
                        .and_then(|row| row.get(res))
                        .copied()
                        .unwrap_or(0);
                    work[res] += alloc;
                }
                finish[tid] = true;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    finish.iter().any(|done| !done)
}

/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}

/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() - 1
    };
    if id == process_inner.mutex_available.len() {
        process_inner.mutex_available.push(1);
        let res_count = process_inner.mutex_available.len();
        for row in process_inner.mutex_need.iter_mut() {
            row.resize(res_count, 0);
        }
        for row in process_inner.mutex_allocation.iter_mut() {
            row.resize(res_count, 0);
        }
    } else {
        process_inner.mutex_available[id] = 1;
        let res_count = process_inner.mutex_available.len();
        for row in process_inner.mutex_need.iter_mut() {
            row.resize(res_count, 0);
            row[id] = 0;
        }
        for row in process_inner.mutex_allocation.iter_mut() {
            row.resize(res_count, 0);
            row[id] = 0;
        }
    }
    id as isize
}

/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let tid = current_tid();
    let mutex: Arc<dyn Mutex>;
    {
        let mut process_inner = process.inner_exclusive_access();
        mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
        let res_count = process_inner.mutex_available.len();
        ensure_thread_row(&mut process_inner.mutex_need, tid, res_count);
        ensure_thread_row(&mut process_inner.mutex_allocation, tid, res_count);
        process_inner.mutex_need[tid][mutex_id] += 1;
        if process_inner.detect_deadlock
            && mutex.is_blocking()
            && detect_deadlock(
                &process_inner.mutex_available,
                &process_inner.mutex_need,
                &process_inner.mutex_allocation,
            )
        {
            process_inner.mutex_need[tid][mutex_id] -= 1;
            return EDEADLOCK;
        }
    }
    mutex.lock();
    {
        let mut process_inner = process.inner_exclusive_access();
        let res_count = process_inner.mutex_available.len();
        ensure_thread_row(&mut process_inner.mutex_need, tid, res_count);
        ensure_thread_row(&mut process_inner.mutex_allocation, tid, res_count);
        if process_inner.mutex_need[tid][mutex_id] > 0 {
            process_inner.mutex_need[tid][mutex_id] -= 1;
        }
        process_inner.mutex_allocation[tid][mutex_id] += 1;
        if process_inner.mutex_available[mutex_id] > 0 {
            process_inner.mutex_available[mutex_id] -= 1;
        }
    }
    0
}

/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let tid = current_tid();
    let mutex: Arc<dyn Mutex>;
    {
        let process_inner = process.inner_exclusive_access();
        mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    }
    mutex.unlock();
    {
        let mut process_inner = process.inner_exclusive_access();
        let res_count = process_inner.mutex_available.len();
        ensure_thread_row(&mut process_inner.mutex_need, tid, res_count);
        ensure_thread_row(&mut process_inner.mutex_allocation, tid, res_count);
        if process_inner.mutex_allocation[tid][mutex_id] > 0 {
            process_inner.mutex_allocation[tid][mutex_id] -= 1;
        }
        process_inner.mutex_available[mutex_id] += 1;
    }
    0
}

/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    if id == process_inner.sema_available.len() {
        process_inner.sema_available.push(res_count);
        let cur_res_count = process_inner.sema_available.len();
        for row in process_inner.sema_need.iter_mut() {
            row.resize(cur_res_count, 0);
        }
        for row in process_inner.sema_allocation.iter_mut() {
            row.resize(cur_res_count, 0);
        }
    } else {
        process_inner.sema_available[id] = res_count;
        let cur_res_count = process_inner.sema_available.len();
        for row in process_inner.sema_need.iter_mut() {
            row.resize(cur_res_count, 0);
            row[id] = 0;
        }
        for row in process_inner.sema_allocation.iter_mut() {
            row.resize(cur_res_count, 0);
            row[id] = 0;
        }
    }
    id as isize
}

/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let tid = current_tid();
    let sem: Arc<Semaphore>;
    {
        let process_inner = process.inner_exclusive_access();
        sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    }
    sem.up();
    {
        let mut process_inner = process.inner_exclusive_access();
        let res_count = process_inner.sema_available.len();
        ensure_thread_row(&mut process_inner.sema_need, tid, res_count);
        ensure_thread_row(&mut process_inner.sema_allocation, tid, res_count);
        if process_inner.sema_allocation[tid][sem_id] > 0 {
            process_inner.sema_allocation[tid][sem_id] -= 1;
        }
        process_inner.sema_available[sem_id] += 1;
    }
    0
}

/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let tid = current_tid();
    let sem: Arc<Semaphore>;
    {
        let mut process_inner = process.inner_exclusive_access();
        sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
        let res_count = process_inner.sema_available.len();
        ensure_thread_row(&mut process_inner.sema_need, tid, res_count);
        ensure_thread_row(&mut process_inner.sema_allocation, tid, res_count);
        process_inner.sema_need[tid][sem_id] += 1;
        if process_inner.detect_deadlock
            && detect_deadlock(
                &process_inner.sema_available,
                &process_inner.sema_need,
                &process_inner.sema_allocation,
            )
        {
            process_inner.sema_need[tid][sem_id] -= 1;
            return EDEADLOCK;
        }
    }
    sem.down();
    {
        let mut process_inner = process.inner_exclusive_access();
        let res_count = process_inner.sema_available.len();
        ensure_thread_row(&mut process_inner.sema_need, tid, res_count);
        ensure_thread_row(&mut process_inner.sema_allocation, tid, res_count);
        if process_inner.sema_need[tid][sem_id] > 0 {
            process_inner.sema_need[tid][sem_id] -= 1;
        }
        process_inner.sema_allocation[tid][sem_id] += 1;
        if process_inner.sema_available[sem_id] > 0 {
            process_inner.sema_available[sem_id] -= 1;
        }
    }
    0
}

/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
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
        current_tid()
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}

/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}

/// enable deadlock detection syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    if enabled != 1 && enabled != 0 {
        return -1;
    }
    process_inner.detect_deadlock = enabled == 1;
    0
}
