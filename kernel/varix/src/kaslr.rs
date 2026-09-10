//! F018 引导期 KASLR — boot-time entropy pool + address slide computation.
//!
//! Reality check for the Limine protocol: the bootloader loads the kernel at
//! its link-time virtual base, so the *kernel image* itself is not relocatable
//! after handoff. What this module provides (and what W1 can honestly
//! deliver) is:
//!   1. a mixed entropy pool fed by RDTSC, boot epoch, memory map geometry
//!      and framebuffer address (all attacker-hostile sources),
//!   2. `slide()` — an aligned random slide used for future mappings
//!      (page-table seed for AI-03, module placement for AI-05),
//!   3. `kaslr=off` compliance — the whole feature degrades to a fixed
//!      slide of 0 with the pool frozen, per the reduce-motion class of
//!      boot-time knobs.
//!
//! Mixing: splitmix64 finalizer (Zierman/Vigna public domain variant).

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// splitmix64 finalizer — cheap, good avalanche.
pub fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

static POOL: AtomicU64 = AtomicU64::new(0);
static SEEDED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(true);
/// Slides handed out so far (diagnostics).
static SLIDES: AtomicU64 = AtomicU64::new(0);

/// Stir one entropy source into the pool.
pub fn stir(value: u64) {
    let cur = POOL.load(Ordering::Relaxed);
    POOL.store(cur ^ mix64(value ^ (cur.rotate_left(31))), Ordering::Relaxed);
    SEEDED.store(true, Ordering::Relaxed);
}

/// Read the raw pool state (diagnostics / self-test).
pub fn pool_state() -> (u64, bool) {
    (POOL.load(Ordering::Relaxed), SEEDED.load(Ordering::Relaxed))
}

/// Disable randomization (the `kaslr=off` cmdline knob).
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Derive a pseudo-random value from the pool without consuming it.
/// Deterministic given the same pool state; different `salt` values
/// decorrelate (used for independent slides).
pub fn derive(salt: u64) -> u64 {
    let (pool, seeded) = pool_state();
    if !seeded {
        return 0;
    }
    mix64(pool ^ mix64(salt))
}

/// Compute a random, `align`-aligned slide in `[0, span]` derived from
/// `salt`. Returns 0 when KASLR is disabled or the pool is empty.
///
/// `span` is rounded down to a multiple of `align` first.
pub fn slide(salt: u64, span: u64, align: u64) -> u64 {
    if !enabled() || span == 0 || align == 0 {
        return 0;
    }
    let span = span - (span % align); // largest aligned span
    if span == 0 {
        return 0;
    }
    let r = derive(salt);
    // Map onto [0, span): result is always aligned to `align`.
    (r % span) / align * align
}

/// Same as [`slide`] but rejects 0 (a zero slide defeats the purpose).
/// Falls back to `align` when the draw is 0.
pub fn slide_nonzero(salt: u64, span: u64, align: u64) -> u64 {
    let s = slide(salt, span, align);
    if s == 0 {
        align
    } else {
        s
    }
}

pub fn slides_issued() -> u64 {
    SLIDES.load(Ordering::Relaxed)
}

fn note_slide() {
    SLIDES.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Target init — collect the boot entropy sources
// ---------------------------------------------------------------------------

/// Stir all boot-time entropy sources into the pool (F018 target path).
/// Sources: RDTSC, boot epoch, memory map geometry, framebuffer address,
/// kernel physical base. Applies the `kaslr=off` knob.
pub fn init() {
    // `kaslr` absent → default on; `kaslr=off|0|false|no` → disabled.
    if crate::cmdline::get("kaslr").is_some() && !crate::cmdline::flag("kaslr") {
        disable();
    }
    // RDTSC — always varying, hardware-guaranteed on x86-64.
    stir(read_tsc());
    // Boot wall-clock time (seconds granularity is fine after mixing).
    if let Some(epoch) = crate::limine::boot_time() {
        stir(epoch as u64);
    }
    // Memory map geometry: base addresses and lengths of the entries.
    if let Some(entries) = crate::limine::memmap() {
        for e in entries.iter() {
            stir(e.base);
            stir(e.length.rotate_left(17) ^ e.kind);
        }
    }
    // Framebuffer address (varies with GOP allocation).
    if let Some(fb) = crate::limine::framebuffer() {
        stir(fb.address as u64 ^ (fb.width as u64) << 32 ^ fb.height as u64);
    }
    // Kernel placement.
    if let Some((phys, _virt)) = crate::limine::executable_address() {
        stir(phys.rotate_left(23));
    }
    // Bootloader identity — version differences change the pool.
    if let Some((name, version)) = crate::limine::bootloader_info() {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for &b in name.as_bytes().iter().chain(version.as_bytes()) {
            h = (h ^ b as u64).wrapping_mul(0x1000_0000_01B3);
        }
        stir(h);
    }
    note_slide();
}

/// Read the timestamp counter (target) / a host-stable value (tests).
#[cfg(target_os = "none")]
pub fn read_tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(all(not(target_os = "none"), test))]
pub fn read_tsc() -> u64 {
    // Host test build: a cheap monotonic-ish value; determinism is not required.
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678)
}

#[cfg(all(not(target_os = "none"), not(test)))]
pub fn read_tsc() -> u64 {
    // Host no_std build (lib as a dependency): deterministic stand-in — the
    // pool still receives other live entropy sources on the real target.
    0x9E37_79B9_7F4A_7C15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_avalanche() {
        // Adjacent inputs must produce very different outputs.
        let a = mix64(1);
        let b = mix64(2);
        assert_ne!(a, b);
        // Deterministic.
        assert_eq!(mix64(0xDEAD_BEEF), mix64(0xDEAD_BEEF));
        // Avalanche: flipping one input bit flips ~half the output bits.
        let x = mix64(0x1234_5678_9ABC_DEF0);
        let y = mix64(0x1234_5678_9ABC_DEF1); // one bit differs
        let diff = (x ^ y).count_ones();
        assert!(diff >= 24 && diff <= 40, "bit diff = {diff}");
    }

    #[test]
    fn stir_changes_pool() {
        let (before, _) = pool_state();
        stir(42);
        let (after, seeded) = pool_state();
        assert!(seeded);
        assert_ne!(before, after);
        stir(42);
        let (again, _) = pool_state();
        assert_ne!(after, again); // second stir with same value still changes state
    }

    #[test]
    fn derive_is_deterministic_per_salt() {
        stir(0xABCD);
        let a = derive(1);
        let b = derive(1);
        let c = derive(2);
        assert_eq!(a, b);
        // Different salts decorrelate (overwhelmingly).
        assert_ne!(a, c);
    }

    #[test]
    fn derive_distributes_over_salts() {
        stir(0xABCD);
        // Distinct salts must yield many distinct values (birthday-safe).
        let mut seen = std::collections::HashSet::new();
        for salt in 0..256u64 {
            seen.insert(derive(salt));
        }
        assert!(seen.len() > 250, "distinct values: {}", seen.len());
    }

    #[test]
    fn slide_alignment_and_bounds() {
        stir(1234);
        for salt in 0..64u64 {
            let s = slide(salt, 1 << 30, 1 << 21);
            assert!(s <= (1 << 30) - (1 << 21));
            assert_eq!(s % (1 << 21), 0);
        }
        // Span not aligned gets rounded down.
        let s = slide(7, 3 * 1024 * 1024 + 123, 1024 * 1024);
        assert!(s <= 2 * 1024 * 1024);
        assert_eq!(s % (1024 * 1024), 0);
    }

    #[test]
    fn slide_disabled_returns_zero() {
        stir(999);
        disable();
        assert_eq!(slide(1, 1 << 30, 1 << 21), 0);
        // restore for other tests
        ENABLED.store(true, Ordering::Relaxed);
    }

    #[test]
    fn slide_nonzero_never_zero() {
        stir(555);
        for salt in 0..128u64 {
            let s = slide_nonzero(salt, 1 << 20, 1 << 12);
            assert_ne!(s, 0);
            assert_eq!(s % (1 << 12), 0);
        }
    }

    #[test]
    fn slide_zero_span_or_align() {
        assert_eq!(slide(1, 0, 4096), 0);
        assert_eq!(slide(1, 4096, 0), 0);
        // span smaller than align → rounded to 0
        assert_eq!(slide(1, 2048, 4096), 0);
    }
}
