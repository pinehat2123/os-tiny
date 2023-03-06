use crate::rcore_main;

macro_rules! boot0 {
    ($entry:ident; stack = $stack:expr) => {
        #[naked]
        #[no_mangle]
        #[link_section = ".text.entry"]
        unsafe extern "C" fn _start() -> ! {
            #[link_section = ".boot.stack"]
            static mut STACK: [u8; $stack] = [0u8; $stack];

            core::arch::asm!(
                "la sp, __end",
                "j  {main}",
                main = sym $entry,
                options(noreturn),
            )
        }
    };
}

boot0!(rcore_main; stack = 16 * 4096);
