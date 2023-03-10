// const MODULE_PROCESS: usize = 0x114514;
// const MODULE_TEST_INTERFACE: usize = 0x233666;
// const MODULE_TASK: usize = 0x7777777;
//
// const FUNC_PROCESS_EXIT: usize = 0x1919810;
// const FUNC_PROCESS_PANIC: usize = 0x11451419;
//
// const FUNC_TEST_WRITE: usize = 0x666233;
// const FUNC_TEST_WRITE_ONE: usize = 0x444555;
// const FUNC_TEST_READ_ONE: usize = 0x999888;
// const FUNC_TEST_READ_LINE: usize = 0x11117777;
// const FUNC_TEST_RESET_TIMER: usize = 0x333;
// const FUNC_TEST_READ_TIMER: usize = 0x22;
//
// const FUNC_SWITCH_TASK: usize = 0x666666;
// const FUNC_IO_TASK: usize = 0x55555;
//
// const FUNC_CHECK: usize = 0x4444;
//
// const BLOCK_SIZE: usize = 512;

use syscall_macro::{
    syscall0, syscall1, syscall2, syscall3, syscall6,
    syscall_number::{
        test::{
            FUNC_TEST_READ_LINE, FUNC_TEST_READ_ONE, FUNC_TEST_READ_TIMER, FUNC_TEST_RESET_TIMER,
            FUNC_TEST_WRITE, FUNC_TEST_WRITE_ONE, MODULE_TEST_INTERFACE,
        },
        user::{
            FUNC_CHECK, FUNC_IO_TASK, FUNC_PROCESS_EXIT, FUNC_PROCESS_PANIC, FUNC_SWITCH_TASK,
            MODULE_PROCESS, MODULE_TASK,
        },
    },
};

use config::BLOCK_SIZE;

pub fn sys_exit(exit_code: i32) -> usize {
    unsafe { syscall1(MODULE_PROCESS, FUNC_PROCESS_EXIT, exit_code as usize) }
}

pub fn sys_panic(file_name: Option<&str>, line: u32, col: u32, msg: Option<&str>) -> usize {
    let (f_buf, f_len) = file_name
        .map(|s| (s.as_ptr() as usize, s.len()))
        .unwrap_or((0, 0));
    let (m_buf, m_len) = msg
        .map(|s| (s.as_ptr() as usize, s.len()))
        .unwrap_or((0, 0));
    // syscall_6(
    //     MODULE_PROCESS,
    //     FUNC_PROCESS_PANIC,
    //     [line as usize, col as usize, f_buf, f_len, m_buf, m_len],
    // )
    unsafe {
        syscall6(
            MODULE_PROCESS,
            FUNC_PROCESS_PANIC,
            line as usize,
            col as usize,
            f_buf,
            f_len,
            m_buf,
            m_len,
        )
    }
}

pub fn sys_yield(next_asid: usize) -> usize {
    unsafe { syscall1(MODULE_TASK, FUNC_SWITCH_TASK, next_asid) }
}

pub fn sys_test_write(buf: &[u8]) -> usize {
    // syscall_3(
    //     MODULE_TEST_INTERFACE,
    //     FUNC_TEST_WRITE,
    //     [0, buf.as_ptr() as usize, buf.len()],
    // )
    unsafe {
        syscall3(
            MODULE_TEST_INTERFACE,
            FUNC_TEST_WRITE,
            0,
            buf.as_ptr() as usize,
            buf.len(),
        )
    }
}

pub fn sys_test_write_one(data: usize) -> usize {
    // syscall_2(MODULE_TEST_INTERFACE, FUNC_TEST_WRITE_ONE, [0, data])
    unsafe { syscall2(MODULE_TEST_INTERFACE, FUNC_TEST_WRITE_ONE, 0, data) }
}

pub fn sys_test_read_one() -> usize {
    // syscall_1(MODULE_TEST_INTERFACE, FUNC_TEST_READ_ONE, 0)
    unsafe { syscall1(MODULE_TEST_INTERFACE, FUNC_TEST_READ_ONE, 0) }
}

pub fn sys_test_read_line(buf: &mut [u8]) -> usize {
    unsafe {
        syscall3(
            MODULE_TEST_INTERFACE,
            FUNC_TEST_READ_LINE,
            0,
            buf.as_ptr() as usize,
            buf.len(),
        )
    }
}

pub fn sys_test_rest_timer() -> usize {
    unsafe { syscall0(MODULE_TEST_INTERFACE, FUNC_TEST_RESET_TIMER) }
}

pub fn sys_read_timer() -> usize {
    unsafe { syscall0(MODULE_TEST_INTERFACE, FUNC_TEST_READ_TIMER) }
}

pub fn sys_enroll_read(block_id: usize, buf: &mut [u8]) -> usize {
    assert!(BLOCK_SIZE == buf.len());
    unsafe {
        syscall3(
            MODULE_TASK,
            FUNC_IO_TASK,
            0,
            block_id,
            buf.as_ptr() as usize,
        )
    }
}

pub fn sys_error_write(block_id: usize, buf: &[u8]) -> usize {
    assert!(BLOCK_SIZE == buf.len());
    unsafe {
        syscall3(
            MODULE_TASK,
            FUNC_IO_TASK,
            1,
            block_id,
            buf.as_ptr() as usize,
        )
    }
}

pub fn sys_kernel_check() -> usize {
    unsafe { syscall0(MODULE_TASK, FUNC_CHECK) }
}
