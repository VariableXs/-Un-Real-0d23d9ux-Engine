//! AI-03 · F054 — 参数安全层（copyin / copyout）.
//!
//! Every user pointer that reaches the kernel goes through here. The module
//! never dereferences a raw address: callers hand in a [`UserMem`] implementation
//! (the real one walks the current address space; the tests use [`FlatMem`]),
//! and this layer decides *whether* the access is legal before a single byte
//! moves.
//!
//! Three rules, all enforced in one place:
//!   1. the whole `[ptr, ptr+len)` window must sit inside the caller's
//!      declared user region — a single check, not a per-byte one;
//!   2. `ptr + len` must not wrap (arithmetic is checked, never wrapping);
//!   3. a partial copy reports how much moved and still returns the error, so
//!      the caller cannot mistake a half-transfer for success.
//!
//! `no_std`, no allocation: all buffers are caller-supplied slices.

use super::errno::Errno;

/// Half-open user address window the calling process owns (F054).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UserRegion {
    pub lo: u64,
    pub hi: u64,
}

impl UserRegion {
    pub const fn new(lo: u64, hi: u64) -> UserRegion {
        UserRegion { lo, hi }
    }

    /// The default ring-3 window: null guard page excluded, canonical top.
    pub const fn standard() -> UserRegion {
        UserRegion {
            lo: 0x0000_0000_0000_1000,
            hi: 0x0000_7FFF_FFFF_FFFF,
        }
    }

    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.lo && addr <= self.hi
    }

    /// Windowing check — the only place user pointers are validated.
    pub fn check(&self, ptr: u64, len: u64) -> Result<(), Errno> {
        if len == 0 {
            // Zero-length accesses are legal only when the pointer itself is
            // sane; this keeps `write(fd, NULL, 0)` working.
            return if self.contains(ptr) { Ok(()) } else { Err(Errno::Fault) };
        }
        let end = checked_add(ptr, len).ok_or(Errno::Overflow)?;
        // `end - 1` is the last byte touched; both ends must be inside.
        if !self.contains(ptr) || !self.contains(end - 1) {
            return Err(Errno::Fault);
        }
        Ok(())
    }

    pub fn check_aligned(&self, ptr: u64, len: u64, align: u64) -> Result<(), Errno> {
        if align > 1 && ptr % align != 0 {
            return Err(Errno::Inval);
        }
        self.check(ptr, len)
    }
}

/// Checked addition used by every range computation in this domain.
pub const fn checked_add(a: u64, b: u64) -> Option<u64> {
    match a.checked_add(b) {
        Some(v) => Some(v),
        None => None,
    }
}

/// The byte transport the safety layer needs. Real kernels implement this on
/// top of the page tables; tests implement it on a flat buffer.
pub trait UserMem {
    /// `None` when the address is not backed by an accessible frame.
    fn read_u8(&self, addr: u64) -> Option<u8>;
    /// `false` when the write could not be committed (unmapped / read-only).
    fn write_u8(&mut self, addr: u64, value: u8) -> bool;
}

/// Flat in-process buffer used by unit tests and by the fuzz harness (F072).
pub struct FlatMem {
    pub base: u64,
    pub bytes: [u8; 256],
}

impl FlatMem {
    pub fn new(base: u64) -> FlatMem {
        FlatMem { base, bytes: [0u8; 256] }
    }

    pub const fn len(&self) -> u64 {
        256
    }

    pub fn region(&self) -> UserRegion {
        UserRegion::new(self.base, self.base + 256)
    }

    fn index(&self, addr: u64) -> Option<usize> {
        if addr < self.base {
            return None;
        }
        let off = addr - self.base;
        if off < 256 {
            Some(off as usize)
        } else {
            None
        }
    }
}

impl UserMem for FlatMem {
    fn read_u8(&self, addr: u64) -> Option<u8> {
        self.index(addr).map(|i| self.bytes[i])
    }

    fn write_u8(&mut self, addr: u64, value: u8) -> bool {
        match self.index(addr) {
            Some(i) => {
                self.bytes[i] = value;
                true
            }
            None => false,
        }
    }
}

/// Result of a partial transfer: bytes moved plus the reason it stopped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Partial {
    pub moved: usize,
    pub err: Errno,
}

/// F054 core — user → kernel. Returns bytes moved.
pub fn copy_from_user<M: UserMem>(
    mem: &M,
    region: &UserRegion,
    dst: &mut [u8],
    src: u64,
) -> Result<usize, Errno> {
    region.check_aligned(src, dst.len() as u64, 1)?;
    let mut n = 0usize;
    while n < dst.len() {
        match mem.read_u8(src + n as u64) {
            Some(b) => {
                dst[n] = b;
                n += 1;
            }
            // The window was legal but a frame was missing: still a fault, and
            // the caller learns how far we got.
            None => return Err(Errno::Fault),
        }
    }
    Ok(n)
}

/// F054 core — kernel → user.
pub fn copy_to_user<M: UserMem>(
    mem: &mut M,
    region: &UserRegion,
    dst: u64,
    src: &[u8],
) -> Result<usize, Errno> {
    region.check_aligned(dst, src.len() as u64, 1)?;
    let mut n = 0usize;
    while n < src.len() {
        if !mem.write_u8(dst + n as u64, src[n]) {
            return Err(Errno::Fault);
        }
        n += 1;
    }
    Ok(n)
}

/// Like [`copy_from_user`] but reports partial progress instead of dropping it.
pub fn copy_from_user_partial<M: UserMem>(
    mem: &M,
    region: &UserRegion,
    dst: &mut [u8],
    src: u64,
) -> Result<usize, Partial> {
    if let Err(e) = region.check_aligned(src, dst.len() as u64, 1) {
        return Err(Partial { moved: 0, err: e });
    }
    let mut n = 0usize;
    while n < dst.len() {
        match mem.read_u8(src + n as u64) {
            Some(b) => {
                dst[n] = b;
                n += 1;
            }
            None => return Err(Partial { moved: n, err: Errno::Fault }),
        }
    }
    Ok(n)
}

/// NUL-terminated user string → fixed kernel buffer (path names, argv).
pub fn copy_cstr_from_user<M: UserMem>(
    mem: &M,
    region: &UserRegion,
    dst: &mut [u8],
    src: u64,
) -> Result<usize, Errno> {
    if dst.is_empty() {
        return Err(Errno::Inval);
    }
    region.check(src, 1)?;
    let mut n = 0usize;
    loop {
        let b = mem.read_u8(src + n as u64).ok_or(Errno::Fault)?;
        if b == 0 {
            return Ok(n);
        }
        if n + 1 >= dst.len() {
            // No room for the terminator — report the truncation rather than
            // silently returning a non-terminated string.
            return Err(Errno::Range);
        }
        dst[n] = b;
        n += 1;
    }
}

/// NUL-terminated kernel string → user buffer (F054, reverse direction).
pub fn copy_cstr_to_user<M: UserMem>(
    mem: &mut M,
    region: &UserRegion,
    dst: u64,
    src: &[u8],
) -> Result<usize, Errno> {
    region.check(dst, src.len() as u64 + 1)?;
    for (i, b) in src.iter().enumerate() {
        if *b == 0 {
            return Err(Errno::Inval); // embedded NUL: not a string
        }
        if !mem.write_u8(dst + i as u64, *b) {
            return Err(Errno::Fault);
        }
    }
    if !mem.write_u8(dst + src.len() as u64, 0) {
        return Err(Errno::Fault);
    }
    Ok(src.len())
}

/// Aligned u64 fetch from user space (syscall arguments are 8-byte scalars).
pub fn read_user_u64<M: UserMem>(mem: &M, region: &UserRegion, ptr: u64) -> Result<u64, Errno> {
    region.check_aligned(ptr, 8, 8)?;
    let mut buf = [0u8; 8];
    copy_from_user(mem, region, &mut buf, ptr)?;
    Ok(u64::from_le_bytes(buf))
}

pub fn write_user_u64<M: UserMem>(
    mem: &mut M,
    region: &UserRegion,
    ptr: u64,
    value: u64,
) -> Result<(), Errno> {
    region.check_aligned(ptr, 8, 8)?;
    copy_to_user(mem, region, ptr, &value.to_le_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f054_rejects_out_of_window_and_wrapping() {
        let r = UserRegion::standard();
        assert!(r.check(0x1000, 16).is_ok());
        // The classic: a huge length that wraps to a small end address.
        assert_eq!(r.check(0x1000, u64::MAX), Err(Errno::Overflow));
        // Null page is excluded.
        assert_eq!(r.check(0, 8), Err(Errno::Fault));
        // Range straddling the canonical top.
        assert_eq!(r.check(r.hi, 2), Err(Errno::Fault));
        // Zero length with a sane pointer is allowed.
        assert!(r.check(0x2000, 0).is_ok());
        assert_eq!(r.check(0, 0), Err(Errno::Fault));
        // Misalignment is a distinct error from a bad address.
        assert_eq!(r.check_aligned(0x1001, 8, 8), Err(Errno::Inval));
        assert!(r.check_aligned(0x1000, 8, 8).is_ok());
    }

    #[test]
    fn f054_copy_round_trips_both_directions() {
        let mut mem = FlatMem::new(0x4000);
        let r = mem.region();
        let payload = b"varix-syscall";
        assert_eq!(copy_to_user(&mut mem, &r, 0x4010, payload).unwrap(), 13);
        let mut back = [0u8; 13];
        assert_eq!(copy_from_user(&mem, &r, &mut back, 0x4010).unwrap(), 13);
        assert_eq!(&back, payload);

        // Strings terminate on the user side, not by over-reading.
        let name = b"init";
        assert_eq!(copy_cstr_to_user(&mut mem, &r, 0x4030, name).unwrap(), 4);
        let mut got = [0u8; 8];
        assert_eq!(copy_cstr_from_user(&mem, &r, &mut got, 0x4030).unwrap(), 4);
        assert_eq!(&got[..4], name);

        // A missing terminator inside the given window must not run away.
        let mut tiny = [0u8; 3];
        assert_eq!(
            copy_cstr_from_user(&mem, &r, &mut tiny, 0x4030),
            Err(Errno::Range)
        );

        // Scalars, including a deliberately misaligned pointer.
        write_user_u64(&mut mem, &r, 0x4020, 0xDEAD_BEEF).unwrap();
        assert_eq!(read_user_u64(&mem, &r, 0x4020).unwrap(), 0xDEAD_BEEF);
        assert_eq!(read_user_u64(&mem, &r, 0x4024), Err(Errno::Inval));
    }

    #[test]
    fn f054_partial_copy_reports_progress() {
        let mem = FlatMem::new(0x8000);
        // 声明的用户窗口比实际驻留的页更宽：这正是稀疏地址空间的常态，
        // 也是"部分拷贝"必须能报告进度的原因。
        let r = UserRegion::new(0x8000, 0x8200);
        let mut dst = [0u8; 8];
        let e = copy_from_user_partial(&mem, &r, &mut dst, 0x80FC).unwrap_err();
        assert_eq!(e.moved, 4);
        assert_eq!(e.err, Errno::Fault);
    }
}
