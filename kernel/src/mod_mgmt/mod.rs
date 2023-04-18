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
        use crate::modules::ModuleType;

        #[used]
        #[link_section = ".kernel_modules.init"]
        static __MODULE_INIT: $crate::modules::Module = $crate::modules::Module {
            init: $init_function as *const fn() -> (),
            ty: $ty,
        };
    };
}

/*
pub(crate) fn init() {
    extern "C" {
        static mut __kernel_modules_start: u8;
        static mut __kernel_modules_end: u8;
    }

    unsafe {
        let size = &__kernel_modules_end as *const u8 as usize
            - &__kernel_modules_start as *const u8 as usize;

        let modules = core::slice::from_raw_parts_mut(
            &mut __kernel_modules_start as *mut u8 as *mut Module,
            size / core::mem::size_of::<Module>(),
        );

        modules.sort_by(|e, a| e.ty.cmp(&a.ty));

        let mut launched_fs = false;

        for module in modules {
            log::debug!("{module:?} {launched_fs}");

            if module.ty != ModuleType::Block && !launched_fs {
                let mut address_space = crate::mem::AddressSpace::this();
                let mut offset_table = address_space.offset_page_table();

                #[cfg(target_arch = "x86_64")]
                drivers::pci::init(&mut offset_table);
                log::info!("loaded PCI driver");

                fs::block::launch().unwrap();
                launched_fs = true;
            }

            let init = core::mem::transmute::<*const fn() -> (), fn() -> ()>(module.init);
            init();
        }
    }
}
*/
