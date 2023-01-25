#![no_main]
#![no_std]
#![feature(naked_functions, asm_const)]
#![deny(warnings)]

#[cfg(test)]
mod test {}



pub mod plantform;

/// 非常简单的 Supervisor 裸机程序。
///
/// 打印 `Hello, World!`，然后关机。
#[no_mangle]
extern "C" fn rcore_main() -> ! {
    use sbi_rt::*;
    for c in b"Hello, world!" {
        #[allow(deprecated)]
        legacy::console_putchar(*c as _);
    }
    system_reset(Shutdown, NoReason);
    unreachable!()
}


use core::panic::PanicInfo;

#[panic_handler]
pub fn panic(_info: &PanicInfo) -> ! {
    use sbi_rt::*;
    system_reset(Shutdown, SystemFailure);
    loop {}
}



