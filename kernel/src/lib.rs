#![no_std]
#![no_main]
#![feature(
    naked_functions,
    asm_const,
    alloc_error_handler,
    panic_info_message,
    linked_list_remove
)]
#![deny(warnings, unused_imports, dead_code)]
#![allow(unused_imports, dead_code)]

//use crate::drivers::{GPU_DEVICE, KEYBOARD_DEVICE, MOUSE_DEVICE, INPUT_CONDVAR};
use crate::drivers::{GPU_DEVICE, KEYBOARD_DEVICE, MOUSE_DEVICE};
extern crate alloc;

#[cfg(test)]
mod test {}

mod boot;

#[macro_use]
extern crate bitflags;

extern crate config;
extern crate mm;

use config::*;
use mm::*;

#[path = "boards/qemu.rs"]
mod board;

#[macro_use]
mod console;
mod async_rt;
mod drivers;
mod fs;
#[cfg(feature = "async_tiny")]
mod hart;
mod lang_items;
#[cfg(feature = "async_tiny")]
mod memory;
mod sbi;
mod sync;
mod syscall;
mod task;
mod timer;
mod trap;

use crate::drivers::chardev::CharDevice;
use crate::drivers::chardev::UART;

use lazy_static::*;
use safe_cell::UPIntrFreeCell;

#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    r#"
    .section .text.entry
    # .globl _start
_start:
    la sp, boot_stack_top
    call rcore_main
    .section .bss.stack
    # .globl boot_stack_lower_bound
boot_stack_lower_bound:
    .space 4096 * 16
    # .globl boot_stack_top
boot_stack_top:

"#
);

fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    unsafe {
        core::slice::from_raw_parts_mut(sbss as usize as *mut u8, ebss as usize - sbss as usize)
            .fill(0);
    }
}

lazy_static! {
    pub static ref DEV_NON_BLOCKING_ACCESS: UPIntrFreeCell<bool> =
        unsafe { UPIntrFreeCell::new(false) };
}

// 内核的入口
#[no_mangle]
extern "C" fn rcore_main() -> ! {
    clear_bss();
    mm::init();
    UART.init();
    // 这里不需要初始化 GUI 的部分
    // println!("KERN: init gpu");
    // let _gpu = GPU_DEVICE.clone();
    println!("KERN: init keyboard");
    let _keyboard = KEYBOARD_DEVICE.clone();
    println!("KERN: init mouse");
    let _mouse = MOUSE_DEVICE.clone();
    println!("KERN: init trap");
    trap::init();
    trap::enable_timer_interrupt();
    timer::set_next_trigger();
    board::device_init();
    println!("KERN: init async");
    #[cfg(feature = "async_tiny")]
    async_rt::init();
    fs::list_apps();
    task::add_initproc();
    *DEV_NON_BLOCKING_ACCESS.exclusive_access() = true;
    task::run_tasks();
    panic!("Unreachable in rust_main!");
}
