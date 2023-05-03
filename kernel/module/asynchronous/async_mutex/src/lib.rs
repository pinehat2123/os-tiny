#![no_std]
use core::{
    cell::UnsafeCell,
    fmt::{Debug, Formatter, Result},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};
use event::Event;

pub struct AsyncMutex<T: ?Sized> {
    state: AtomicUsize,
    lock_ops: Event,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send + ?Sized> Send for AsyncMutex<T> {}
unsafe impl<T: Sync + ?Sized> Sync for AsyncMutex<T> {}

impl<T> AsyncMutex<T> {
    pub const fn new(data: T) -> AsyncMutex<T> {
        AsyncMutex {
            state: AtomicUsize::new(0),
            lock_ops: Event::new(),
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T: ?Sized> AsyncMutex<T> {
    #[inline]
    pub async fn lock(&self) -> AsyncMutexGuard<'_, T> {
        if let Some(guard) = self.try_lock() {
            return guard;
        }
        self.acquire_slow().await;
        AsyncMutexGuard(self)
    }
    #[cold]
    async fn acquire_slow(&self) {
        loop {
            let listener = self.lock_ops.listen();

            match self
                .state
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Acquire)
                .unwrap_or_else(|x| x)
            {
                0 => return,

                1 => {}

                _ => break,
            }

            listener.await;

            match self
                .state
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Acquire)
                .unwrap_or_else(|x| x)
            {
                0 => return,

                1 => {}

                _ => {
                    self.lock_ops.notify(1);
                    break;
                }
            }
        }

        if self.state.fetch_add(2, Ordering::Release) > usize::MAX / 2 {
            panic!("In case of potential overflow, abort.");
        }

        let _call = CallOnDrop(|| {
            self.state.fetch_sub(2, Ordering::Release);
        });

        loop {
            let listener = self.lock_ops.listen();

            match self
                .state
                .compare_exchange(2, 2 | 1, Ordering::Acquire, Ordering::Acquire)
                .unwrap_or_else(|x| x)
            {
                2 => return,

                s if s % 2 == 1 => {}

                _ => {
                    self.lock_ops.notify(1);
                }
            }

            listener.await;

            if self.state.fetch_or(1, Ordering::Acquire) % 2 == 0 {
                return;
            }
        }
    }

    #[inline]
    pub fn try_lock(&self) -> Option<AsyncMutexGuard<'_, T>> {
        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Acquire)
            .is_ok()
        {
            Some(AsyncMutexGuard(self))
        } else {
            None
        }
    }
    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }
}

impl<T: Debug + ?Sized> Debug for AsyncMutex<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        struct Locked;
        impl Debug for Locked {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                f.write_str("<Locked>")
            }
        }

        match self.try_lock() {
            None => f.debug_struct("AsyncMutex").field("data", &Locked).finish(),
            Some(guard) => f
                .debug_struct("AsyncMutex")
                .field("data", &&*guard)
                .finish(),
        }
    }
}

impl<T> From<T> for AsyncMutex<T> {
    fn from(value: T) -> AsyncMutex<T> {
        AsyncMutex::new(value)
    }
}

impl<T: Default + ?Sized> Default for AsyncMutex<T> {
    fn default() -> AsyncMutex<T> {
        AsyncMutex::new(Default::default())
    }
}

pub struct AsyncMutexGuard<'a, T: ?Sized>(&'a AsyncMutex<T>);

unsafe impl<T: Send + ?Sized> Send for AsyncMutexGuard<'_, T> {}
unsafe impl<T: Sync + ?Sized> Sync for AsyncMutexGuard<'_, T> {}

impl<'a, T: ?Sized> AsyncMutexGuard<'a, T> {
    pub fn source(guard: &AsyncMutexGuard<'a, T>) -> &'a AsyncMutex<T> {
        guard.0
    }
}

impl<T: ?Sized> Drop for AsyncMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.0.state.fetch_sub(1, Ordering::Release);
        self.0.lock_ops.notify(1);
    }
}

impl<T: core::fmt::Debug + ?Sized> core::fmt::Debug for AsyncMutexGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&**self, f)
    }
}

impl<T: core::fmt::Display + ?Sized> core::fmt::Display for AsyncMutexGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: ?Sized> Deref for AsyncMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.0.data.get() }
    }
}

impl<T: ?Sized> DerefMut for AsyncMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.0.data.get() }
    }
}

struct CallOnDrop<F: Fn()>(F);

impl<F: Fn()> Drop for CallOnDrop<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}
