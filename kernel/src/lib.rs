#![no_main]
#![no_std]
#![feature(naked_functions, asm_const, alloc_error_handler, panic_info_message)]
#![deny(warnings, unused_imports, dead_code)]
#![allow(unused_imports, dead_code)]

//use crate::drivers::{GPU_DEVICE, KEYBOARD_DEVICE, MOUSE_DEVICE, INPUT_CONDVAR};
use crate::drivers::{GPU_DEVICE, KEYBOARD_DEVICE, MOUSE_DEVICE};
extern crate alloc;



#[cfg(test)]
mod test {}

mod plantform;

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
mod drivers;
mod fs;
mod lang_items;
mod sbi;
mod sync;
mod syscall;
mod task;
mod timer;
mod trap;
mod async_rt;


use crate::drivers::chardev::CharDevice;
use crate::drivers::chardev::UART;

use lazy_static::*;
use safe_cell::UPIntrFreeCell;

core::arch::global_asm!(include_str!("plantform/arch/riscv64gc/asm/entry.S"));

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

use config::DEV_NON_BLOCKING_ACCESS;

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
    fs::list_apps();
    task::add_initproc();
    *DEV_NON_BLOCKING_ACCESS.exclusive_access() = true;
    task::run_tasks();
    panic!("Unreachable in rust_main!");
}
