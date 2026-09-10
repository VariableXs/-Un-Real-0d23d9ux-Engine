//! A `no_std` once-initialized cell for statics.
//!
//! `core::cell::OnceCell` is `!Sync`, so it cannot back a `static`. This
//! module provides the kernel's own `OnceLock<T>`: an `AtomicBool` ready
//! flag (Release/Acquire pair) plus an uninitialized `UnsafeCell` slot.
//!
//! SAFETY contract: the boot path is single-threaded (pre-SMP there is
//! exactly one core running), so the write-once / read-many pattern below
//! cannot race. Once AI-02 brings up SMP, secondary cores only ever read
//! already-initialized state.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

/// A cell that is initialized at most once, usable in `static` context.
pub struct OnceLock<T> {
    ready: AtomicBool,
    value: UnsafeCell<MaybeUninit<T>>,
}

// SAFETY: single-threaded boot contract (see module docs). The value becomes
// visible to readers only after the Release store of `ready`; readers load it
// with Acquire. There is exactly one writer (the BSP boot path).
unsafe impl<T> Sync for OnceLock<T> {}
unsafe impl<T> Send for OnceLock<T> {}

impl<T> OnceLock<T> {
    pub const fn new() -> OnceLock<T> {
        OnceLock {
            ready: AtomicBool::new(false),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// Current value, if initialized.
    pub fn get(&self) -> Option<&T> {
        if self.ready.load(Ordering::Acquire) {
            Some(unsafe { (*self.value.get()).assume_init_ref() })
        } else {
            None
        }
    }

    /// Initialize with `v`. Fails (returning `v`) when already initialized.
    pub fn set(&self, v: T) -> Result<(), T> {
        if self.ready.load(Ordering::Acquire) {
            return Err(v);
        }
        unsafe {
            *self.value.get() = MaybeUninit::new(v);
        }
        self.ready.store(true, Ordering::Release);
        Ok(())
    }

    /// Value or lazy initialization through `f`.
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        if let Some(v) = self.get() {
            return v;
        }
        let v = f();
        let _ = self.set(v);
        // `set` only fails when already initialized — in which case `get`
        // succeeds for the winner's value.
        self.get().expect("OnceLock::set cannot fail on an empty cell")
    }
}

impl<T: Copy> OnceLock<T> {
    /// Copied-out convenience for small Copy payloads.
    pub fn get_copy(&self) -> Option<T> {
        self.get().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_get() {
        let c: OnceLock<u32> = OnceLock::new();
        assert!(c.get().is_none());
        assert!(c.set(7).is_ok());
        assert_eq!(c.get(), Some(&7));
        // second set rejected, value returned
        assert_eq!(c.set(9), Err(9));
        assert_eq!(c.get(), Some(&7));
    }

    #[test]
    fn get_or_init_runs_once() {
        let c: OnceLock<u32> = OnceLock::new();
        assert_eq!(*c.get_or_init(|| 1), 1);
        assert_eq!(*c.get_or_init(|| 2), 1); // initializer ignored
        assert_eq!(c.get_copy(), Some(1));
    }

    #[test]
    fn works_in_static() {
        static S: OnceLock<(u8, u16)> = OnceLock::new();
        assert!(S.get().is_none());
        assert!(S.set((1, 2)).is_ok());
        assert_eq!(S.get(), Some(&(1, 2)));
    }
}
