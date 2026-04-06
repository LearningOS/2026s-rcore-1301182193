//! Process management syscalls
#![allow(unused)]
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next};
use crate::timer::*;
use crate::mm::*;
use crate::config::*;
use crate::task::*;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let sec = us / 1_000_000;
    let usec = us % 1_000_000;

    let page_table = PageTable::from_token(current_user_token());

    let base = ts as usize;
    let ptr_sec = base;
    let ptr_usec = base + core::mem::size_of::<usize>();

    let vpn_sec: VirtPageNum = VirtAddr::from(ptr_sec).floor();
    let vpn_usec: VirtPageNum = VirtAddr::from(ptr_usec).floor();

    let ppn_sec = page_table.translate(vpn_sec).unwrap().ppn();
    let ppn_usec = page_table.translate(vpn_usec).unwrap().ppn();

    let pa_sec = ((ppn_sec.0 << PAGE_SIZE_BITS) | (ptr_sec & ((1 << PAGE_SIZE_BITS) - 1))) as *mut usize;
    let pa_usec = ((ppn_usec.0 << PAGE_SIZE_BITS) | (ptr_usec & ((1 << PAGE_SIZE_BITS) - 1))) as *mut usize;

    unsafe {
        *pa_sec = sec;
        *pa_usec = usec;
    }

    0

}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => {
            let vpn = VirtAddr::from(id).floor();
            let page_table = PageTable::from_token(current_user_token());
            let pte = page_table.translate(vpn);
            match pte {
                Some(pte) => {
                    if !pte.readable() || !pte.flags().contains(PTEFlags::U) {
                        return -1;
                    }
                },
                None => {
                    return -1;
                }
            }
            let ppn = page_table.translate(vpn).unwrap().ppn();
            let pa = ((ppn.0 << PAGE_SIZE_BITS) + (id & ((1 << PAGE_SIZE_BITS) - 1))) as *const u8;
            let value = unsafe {*pa};
            value as isize
        },
        1 => {
            let vpn = VirtAddr::from(id).floor();
            let page_table = PageTable::from_token(current_user_token());
            let pte = page_table.translate(vpn);
            match pte {
                Some(pte) => {
                    if !pte.writable() || !pte.flags().contains(PTEFlags::U) {
                        return -1;
                    }
                },
                None => {
                    return -1;
                }
            }
            let ppn = page_table.translate(vpn).unwrap().ppn();
            let pa = ((ppn.0 << PAGE_SIZE_BITS) + (id & ((1 << PAGE_SIZE_BITS) - 1))) as *mut u8;
            unsafe {
                *pa = data as u8;
            }
            0
        },
        2 => {
            get_current_task_syscall_times(id) as isize
        },
        _ => {
            -1
        }
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap");
    if len == 0 {
        return 0;
    }
    if start % PAGE_SIZE != 0 || port & (!0x7) != 0 || port & (0x7) == 0 {
        return -1;
    }
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);

    let start_vpn = start_va.floor();
    let end_vpn = end_va.ceil();

    let page_table = PageTable::from_token(current_user_token());
    for vpn in VPNRange::new(start_vpn, end_vpn) {
        let pte = page_table.translate(vpn);
        if let Some(pte) = pte {
            if pte.is_valid() {
                return -1;
            }
        }
    }
    let mut perm = MapPermission::U;
    if (port >> 0 & 1) == 1 {
        perm |= MapPermission::R;
    }

    if (port >> 1 & 1) == 1 {
        perm |= MapPermission::W;
    }

    if (port >> 2 & 1) == 1{
        perm |= MapPermission::X;
    }
    insert_framed_area(start_va, end_va, perm);
    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if len == 0 {
        return 0;
    }
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);

    let start_vpn = start_va.floor();
    let end_vpn = end_va.ceil();
    let mut page_table = PageTable::from_token(current_user_token());
    for vpn in VPNRange::new(start_vpn, end_vpn) {
        let pte = page_table.translate(vpn);
        match pte {
            Some(pte) if pte.is_valid() => {}
            _ => return -1,
        }
    }
    remove_framed_area(start_vpn, end_vpn);
    0
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
