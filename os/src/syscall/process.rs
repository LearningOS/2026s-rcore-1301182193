//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, suspend_current_and_run_next},
    timer::get_time_us,
};
use lazy_static::lazy_static;
use crate::sync::UPSafeCell;
use alloc::vec;
use alloc::vec::Vec;

use crate::syscall::syscall_count;


#[repr(C)]
#[derive(Debug)]


pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    syscall_count[93] += 1;
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {

    syscall_count[124] += 1;
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}


/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    
    syscall_count[169] += 1;
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}




// TODO: implement the syscall
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    syscall_count[410] += 1;

    match _trace_request {
        0 => {
            let value = unsafe {*(_id as *const u8)};
            value as isize
        },
        1 => {
            unsafe {
                *(_id as *mut u8) = _data as u8;
            }
            0
        },
        2 => {
            let mut counts = syscall_count.exclusive_access();
            counts[_id] as isize
        },
        _ => {
            -1
        }
    }
}
