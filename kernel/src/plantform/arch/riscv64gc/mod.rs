// Supervisor 汇编入口。
//
// 设置栈并跳转到 Rust。
//
// #[naked]
// #[no_mangle]
// #[link_section = ".text.entry"]
// unsafe extern "C" fn _start() -> ! {
//     const STACK_SIZE: usize = 4096;
// 
//     #[link_section = ".bss.uninit"]
//     static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];
// 
//     core::arch::asm!(
//         "la sp, {stack} + {stack_size}",
//         "j  {main}",
//         stack_size = const STACK_SIZE,
//         stack      =   sym STACK,
//         main       =   sym rcore_main,
//         options(noreturn),
//     )
// }
// 
use crate::rcore_main;

linker::boot0!(rcore_main; stack = 16 * 4096);
