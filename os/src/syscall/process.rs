//! Process management syscalls
use crate::mm::translated_byte_buffer;
use crate::task::{
    change_program_brk, current_user_token, exit_current_and_run_next, suspend_current_and_run_next,
};
use crate::timer::get_time_us;

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

pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let value = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let mut src = &value as *const TimeVal as *const u8;
    let mut remaining = core::mem::size_of::<TimeVal>();
    for dst in translated_byte_buffer(current_user_token(), ts as *const u8, remaining) {
        let len = dst.len().min(remaining);
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst.as_mut_ptr(), len);
            src = src.add(len);
        }
        remaining -= len;
        if remaining == 0 {
            break;
        }
    }
    if remaining == 0 {
        0
    } else {
        -1
    }
}

pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    match trace_request {
        0 => crate::task::read_current_user_byte(id)
            .map(|value| value as isize)
            .unwrap_or(-1),
        1 => {
            if crate::task::write_current_user_byte(id, data as u8) {
                0
            } else {
                -1
            }
        }
        2 => crate::task::syscall_count(id) as isize,
        _ => -1,
    }
}

pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    if prot == 0 || prot & !0x7 != 0 {
        return -1;
    }
    let mut permission = crate::mm::MapPermission::U;
    if prot & 1 != 0 {
        permission |= crate::mm::MapPermission::R;
    }
    if prot & 2 != 0 {
        permission |= crate::mm::MapPermission::W;
    }
    if prot & 4 != 0 {
        permission |= crate::mm::MapPermission::X;
    }
    if crate::task::mmap_current(start, len, permission) {
        0
    } else {
        -1
    }
}

pub fn sys_munmap(start: usize, len: usize) -> isize {
    if crate::task::munmap_current(start, len) {
        0
    } else {
        -1
    }
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
