#![no_std]
use core::cell::{RefCell, RefMut, UnsafeCell};
use core::ops::{Deref, DerefMut};
use lazy_static::*;
use riscv::register::sstatus;

pub use rv_lock::{Lock, LockGuard};
pub mod rv_lock {
    use core::{
        arch::asm,
        ops::{Deref, DerefMut},
    };
    use spin::{Mutex, MutexGuard};

    #[derive(Default)]
    pub struct Lock<T>(pub(self) Mutex<T>);

    pub struct LockGuard<'a, T> {
        guard: Option<MutexGuard<'a, T>>,
        sstatus: usize,
    }

    impl<T> Lock<T> {
        pub const fn new(obj: T) -> Self {
            Self(Mutex::new(obj))
        }

        pub fn lock(&self) -> LockGuard<'_, T> {
            let sstatus: usize = 0usize;
            unsafe {
                asm!("csrrci {0}, sstatus, 1 << 1", in(reg) (sstatus));
            }
            LockGuard {
                guard: Some(self.0.lock()),
                sstatus,
            }
        }
    }

    impl<'a, T> Drop for LockGuard<'a, T> {
        fn drop(&mut self) {
            self.guard.take();
            unsafe { asm!("csrs sstatus, {0}", lateout(reg) self.sstatus) };
        }
    }

    impl<'a, T> Deref for LockGuard<'a, T> {
        type Target = T;
        fn deref(&self) -> &Self::Target {
            self.guard.as_ref().unwrap().deref()
        }
    }

    impl<'a, T> DerefMut for LockGuard<'a, T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            self.guard.as_mut().unwrap().deref_mut()
        }
    }
}
/*
/// Wrap a static data structure inside it so that we are
/// able to access it without any `unsafe`.
///
/// We should only use it in uniprocessor.
///
/// In order to get mutable reference of inner data, call
/// `exclusive_access`.
pub struct UPSafeCell<T> {
    /// inner data
    inner: RefCell<T>,
}

unsafe impl<T> Sync for UPSafeCell<T> {}

impl<T> UPSafeCell<T> {
    /// User is responsible to guarantee that inner struct is only used in
    /// uniprocessor.
    pub unsafe fn new(value: T) -> Self {
        Self {
            inner: RefCell::new(value),
        }
    }
    /// Panic if the data has been borrowed.
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        self.inner.borrow_mut()
    }
}
*/

pub struct UPSafeCellRaw<T> {
    inner: UnsafeCell<T>,
}

unsafe impl<T> Sync for UPSafeCellRaw<T> {}

impl<T> UPSafeCellRaw<T> {
    pub unsafe fn new(value: T) -> Self {
        Self {
            inner: UnsafeCell::new(value),
        }
    }
    pub fn get_mut(&self) -> &mut T {
        unsafe { &mut (*self.inner.get()) }
    }
}

pub struct IntrMaskingInfo {
    nested_level: usize,
    sie_before_masking: bool,
}

lazy_static! {
    static ref INTR_MASKING_INFO: UPSafeCellRaw<IntrMaskingInfo> =
        unsafe { UPSafeCellRaw::new(IntrMaskingInfo::new()) };
}

impl IntrMaskingInfo {
    pub fn new() -> Self {
        Self {
            nested_level: 0,
            sie_before_masking: false,
        }
    }

    pub fn enter(&mut self) {
        let sie = sstatus::read().sie();
        unsafe {
            sstatus::clear_sie();
        }
        if self.nested_level == 0 {
            self.sie_before_masking = sie;
        }
        self.nested_level += 1;
    }

    pub fn exit(&mut self) {
        self.nested_level -= 1;
        if self.nested_level == 0 && self.sie_before_masking {
            unsafe {
                sstatus::set_sie();
            }
        }
    }
}

pub struct UPIntrFreeCell<T> {
    /// inner data
    inner: RefCell<T>,
}

unsafe impl<T> Sync for UPIntrFreeCell<T> {}

pub struct UPIntrRefMut<'a, T>(Option<RefMut<'a, T>>);

impl<T> UPIntrFreeCell<T> {
    pub unsafe fn new(value: T) -> Self {
        Self {
            inner: RefCell::new(value),
        }
    }

    /// Panic if the data has been borrowed.
    pub fn exclusive_access(&self) -> UPIntrRefMut<'_, T> {
        INTR_MASKING_INFO.get_mut().enter();
        UPIntrRefMut(Some(self.inner.borrow_mut()))
    }

    pub fn exclusive_session<F, V>(&self, f: F) -> V
    where
        F: FnOnce(&mut T) -> V,
    {
        let mut inner = self.exclusive_access();
        f(inner.deref_mut())
    }
}

impl<'a, T> Drop for UPIntrRefMut<'a, T> {
    fn drop(&mut self) {
        self.0 = None;
        INTR_MASKING_INFO.get_mut().exit();
    }
}

impl<'a, T> Deref for UPIntrRefMut<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap().deref()
    }
}
impl<'a, T> DerefMut for UPIntrRefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().unwrap().deref_mut()
    }
}