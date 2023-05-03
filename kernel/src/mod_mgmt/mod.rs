#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
#[repr(C)]
pub enum ModuleType {
    Block = 0,
    Other = 1,
}

#[derive(Debug)]
#[repr(C)]
pub struct Module {
    pub init: *const fn() -> (),
    pub ty: ModuleType,
}

unsafe impl Sync for Module {}

#[macro_export]
macro_rules! module_init {
    ($init_function:expr, $ty:path) => {
        use super::{Module, ModuleType};

        #[used]
        #[link_section = ".kernel_modules.init"]
        static __MODULE_INIT: Module = Module {
            init: $init_function as *const fn() -> (),
            ty: $ty,
        };
    };
}

pub mod md1 {
    pub fn test_init() {
        use super::*;
        println!("KERN: MODULE_test test_init fn");
    }
    module_init!(test_init, ModuleType::Other);
}
pub mod md2 {
    use super::*;
    pub fn test_init2() {
        println!("KERN: MODULE_test test_init fn");
    }
    module_init!(test_init2, ModuleType::Other);
}

pub(crate) fn init() {
    extern "C" {
        static mut __kernel_modules_start: u8;
        static mut __kernel_modules_end: u8;
    }
    unsafe {
        let size = &__kernel_modules_end as *const u8 as usize
            - &__kernel_modules_start as *const u8 as usize;

        // Just read from the .kernel_modules
        let modules = core::slice::from_raw_parts_mut(
            &mut __kernel_modules_start as *mut u8 as *mut Module,
            size / core::mem::size_of::<Module>(),
        );
        println!("KERN: Module: {:?}", modules);
        // modules.sort_by(|e, a| e.ty.cmp(&a.ty));
        // The code is doesn't not work, some block before the code.
        // for module in modules {
        //     println!("KERN: modules: {:?}", module);
        //     let _init = core::mem::transmute::<*const fn() -> (), fn() -> ()> (module.init);
        //     init();
        // }
    }
}
