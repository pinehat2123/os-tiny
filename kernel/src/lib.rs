#![no_main]
#![no_std]
#![feature(naked_functions, asm_const)]
#![deny(warnings)]

//use crate::drivers::{GPU_DEVICE, KEYBOARD_DEVICE, MOUSE_DEVICE, INPUT_CONDVAR};
use crate::drivers::{GPU_DEVICE, KEYBOARD_DEVICE, MOUSE_DEVICE};
extern crate alloc;


#[cfg(test)]
mod test {}

mod plantform;

#[macro_use]
extern crate bitflags;

#[path = "boards/qemu.rs"]
mod board;

#[macro_use]
mod console;
mod config;
mod drivers;
mod fs;
mod lang_items;
mod mm;
mod sbi;
mod sync;
mod syscall;
mod task;
mod timer;
mod trap;

use crate::drivers::chardev::CharDevice;
use crate::drivers::chardev::UART;


use lazy_static::*;
use sync::UPIntrFreeCell;

use sbi_rt::*;


lazy_static! {
    pub static ref DEV_NON_BLOCKING_ACCESS: UPIntrFreeCell<bool> =
        unsafe { UPIntrFreeCell::new(false) };
}
/// 非常简单的 Supervisor 裸机程序。
///
/// 打印 `Hello, World!`，然后关机。
#[no_mangle]
extern "C" fn rcore_main() -> ! {
    // for c in b"Hello, world!" {
    //     #[allow(deprecated)]
    //     legacy::console_putchar(*c as _);
    // }
    // system_reset(Shutdown, NoReason);
    // unreachable!()
    linker::zero_bss();
    mm::init();
    UART.init();
    println!("KERN: init gpu");
    let _gpu = GPU_DEVICE.clone();
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

use core::panic::PanicInfo;

#[panic_handler]
pub fn panic(_info: &PanicInfo) -> ! {
    use sbi_rt::*;
    system_reset(Shutdown, SystemFailure);
    loop {}
}
