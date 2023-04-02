#![no_std]
#[allow(dead_code)]

pub mod syscall_number {

    pub mod user {
        pub const MODULE_PROCESS: usize = 0x114514;
        pub const MODULE_TASK: usize = 0x7777777;
        pub const FUNC_PROCESS_EXIT: usize = 0x1919810;
        pub const FUNC_PROCESS_PANIC: usize = 0x11451419;
        pub const FUNC_SWITCH_TASK: usize = 0x666666;
        pub const FUNC_IO_TASK: usize = 0x55555;
        pub const FUNC_CHECK: usize = 0x4444;
        pub const BLOCK_SIZE: usize = 512;
    }

    pub mod test {
        pub const MODULE_TEST_INTERFACE: usize = 0x233666;
        pub const FUNC_TEST_WRITE: usize = 0x666233;
        pub const FUNC_TEST_WRITE_ONE: usize = 0x444555;
        pub const FUNC_TEST_READ_ONE: usize = 0x999888;
        pub const FUNC_TEST_READ_LINE: usize = 0x11117777;
        pub const FUNC_TEST_RESET_TIMER: usize = 0x333;
        pub const FUNC_TEST_READ_TIMER: usize = 0x22;
    }

    pub mod kernel {
        pub const SYSCALL_DUP: usize = 24;
        pub const SYSCALL_OPEN: usize = 56;
        pub const SYSCALL_CLOSE: usize = 57;
        pub const SYSCALL_PIPE: usize = 59;
        pub const SYSCALL_READ: usize = 63;
        pub const SYSCALL_WRITE: usize = 64;
        pub const SYSCALL_EXIT: usize = 93;
        pub const SYSCALL_SLEEP: usize = 101;
        pub const SYSCALL_YIELD: usize = 124;
        pub const SYSCALL_KILL: usize = 129;
        pub const SYSCALL_GET_TIME: usize = 169;
        pub const SYSCALL_GETPID: usize = 172;
        pub const SYSCALL_FORK: usize = 220;
        pub const SYSCALL_EXEC: usize = 221;
        pub const SYSCALL_WAITPID: usize = 260;
        pub const SYSCALL_THREAD_CREATE: usize = 1000;
        pub const SYSCALL_GETTID: usize = 1001;
        pub const SYSCALL_WAITTID: usize = 1002;
        pub const SYSCALL_MUTEX_CREATE: usize = 1010;
        pub const SYSCALL_MUTEX_LOCK: usize = 1011;
        pub const SYSCALL_MUTEX_UNLOCK: usize = 1012;
        pub const SYSCALL_SEMAPHORE_CREATE: usize = 1020;
        pub const SYSCALL_SEMAPHORE_UP: usize = 1021;
        pub const SYSCALL_SEMAPHORE_DOWN: usize = 1022;
        pub const SYSCALL_CONDVAR_CREATE: usize = 1030;
        pub const SYSCALL_CONDVAR_SIGNAL: usize = 1031;
        pub const SYSCALL_CONDVAR_WAIT: usize = 1032;
        pub const SYSCALL_CREATE_DESKTOP: usize = 2000;
    }
}

macro_rules! syscall {
    ($($name:ident($a:ident, $($b:ident, $($c:ident, $($d:ident, $($e:ident, $($f:ident, $($g:ident, $($h:ident, )?)?)?)?)?)?)?);)+) => {
        $(
            pub unsafe fn $name($a: usize, $($b: usize, $($c: usize, $($d: usize, $($e: usize, $($f: usize, $($g: usize, $($h: usize, )?)?)?)?)?)?)?) -> isize {
                let _ret: isize;

                core::arch::asm!(
                    "ecall",
                    in("a7") $a,
                    $(
                        in("a0") $b,
                        $(
                            in("a1") $c,
                            $(
                                in("a2") $d,
                                $(
                                    in("a3") $e,
                                    $(
                                        in("a4") $f,
                                        $(
                                            in("a5") $g,
                                            $(
                                                in("a6") $h,
                                            )?
                                        )?
                                    )?
                                )?
                            )?
                        )?
                    )?
                    lateout("a0") _ret,
                    options(nostack),
                );
                _ret
            }
        )+
    };
}

syscall! {
    syscall0(a,z, );
    syscall1(a, b, z, );
    syscall2(a, b, c, z, );
    syscall3(a, b, c, d, z, );
    syscall4(a, b, c, d, e, z, );
    syscall5(a, b, c, d, e, f, z, );
    syscall6(a, b, c, d, e, f, g, z, );
}

fn syscall_3_helper(id: usize, args: [usize; 3]) -> isize {
    unsafe { syscall2(id, args[0], args[1], args[2]) }
}

pub fn syscall(id: usize, args: [usize; 3]) -> isize {
    syscall_3_helper(id, args)
}

#[cfg(all(any(target_os = "none"), target_arch = "riscv64"))]
#[path = "arch/riscv64.rs"]
mod arch;

mod number;
