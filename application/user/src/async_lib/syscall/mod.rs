const MODULE_PROCESS: usize = 0x114514;
const MODULE_TEST_INTERFACE: usize = 0x233666;
const MODULE_TASK: usize = 0x7777777;

const FUNC_PROCESS_EXIT: usize = 0x1919810;
const FUNC_PROCESS_PANIC: usize = 0x11451419;

const FUNC_TEST_WRITE: usize = 0x666233;
const FUNC_TEST_WRITE_ONE: usize = 0x444555;
const FUNC_TEST_READ_ONE: usize = 0x999888;
const FUNC_TEST_READ_LINE: usize = 0x11117777;
const FUNC_TEST_RESET_TIMER: usize = 0x333;
const FUNC_TEST_READ_TIMER: usize = 0x22;

const FUNC_SWITCH_TASK: usize = 0x666666;
const FUNC_IO_TASK: usize = 0x55555;

const FUNC_CHECK: usize = 0x4444;

const BLOCK_SIZE: usize = 512;

use syscall_macro::{syscall1, syscall0};

pub fn sys_yield(next_asid: usize) -> usize {
    unsafe { syscall1(MODULE_TASK, FUNC_SWITCH_TASK, next_asid) }
}

pub fn sys_kernel_check() -> usize {
    unsafe { syscall0(MODULE_TASK, FUNC_CHECK) }
}