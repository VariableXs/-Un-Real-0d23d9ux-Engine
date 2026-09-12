//! F059 · 内存映射包装.
//!
//! W^X 在内核侧是硬约束（`PROT_WRITE | PROT_EXEC` 一定被拒）。本层不重复
//! 检查，但把 `Prot` 表达成类型，让"同时可写可执行"在类型上就显得不对劲。

use crate::abi::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_EXEC, PROT_NONE, PROT_READ, PROT_WRITE, SYS_MMAP, SYS_MPROTECT, SYS_MUNMAP};
use crate::error::{invoke, Error, Result};

/// 映射保护位。`Prot::RWX` 故意不提供——想构造出 W+X 必须自己拼常量，
/// 而且会被内核以 `EPERM` 拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Prot(pub u32);

impl Prot {
    pub const NONE: Prot = Prot(PROT_NONE);
    pub const R: Prot = Prot(PROT_READ);
    pub const W: Prot = Prot(PROT_WRITE);
    pub const X: Prot = Prot(PROT_EXEC);
    pub const RW: Prot = Prot(PROT_READ | PROT_WRITE);
    pub const RX: Prot = Prot(PROT_READ | PROT_EXEC);

    pub const fn bits(self) -> u32 {
        self.0
    }

    /// 该组合是否满足 W^X。与内核 `prot_wx_ok` 同判据。
    pub const fn wx_ok(self) -> bool {
        self.0 & PROT_WRITE == 0 || self.0 & PROT_EXEC == 0
    }
}

/// 匿名私有映射，返回映射基址。
pub fn mmap_anon(len: usize, prot: Prot) -> Result<*mut u8> {
    if !prot.wx_ok() {
        // 早失败，省一次陷入；判据与内核一致，不会出现两边结论不同的情形。
        return Err(Error::Perm);
    }
    let addr = unsafe {
        invoke(
            SYS_MMAP,
            [
                0,
                len as u64,
                prot.bits() as u64,
                (MAP_PRIVATE | MAP_ANONYMOUS) as u64,
                u64::MAX, // fd = -1
                0,
            ],
        )?
    };
    Ok(addr as *mut u8)
}

pub fn munmap(addr: *mut u8, len: usize) -> Result<()> {
    unsafe {
        invoke(SYS_MUNMAP, [addr as u64, len as u64, 0, 0, 0, 0])?;
    }
    Ok(())
}

pub fn mprotect(addr: *mut u8, len: usize, prot: Prot) -> Result<()> {
    if !prot.wx_ok() {
        return Err(Error::Perm);
    }
    unsafe {
        invoke(
            SYS_MPROTECT,
            [addr as u64, len as u64, prot.bits() as u64, 0, 0, 0],
        )?;
    }
    Ok(())
}

/// 一个 RAII 式映射：离开作用域自动 `munmap`。`no_std` 下没有 `Drop` 争议，
/// 这是用户程序最不容易泄漏资源的方式。
pub struct Mapping {
    base: *mut u8,
    len: usize,
}

impl Mapping {
    pub fn anonymous(len: usize, prot: Prot) -> Result<Mapping> {
        let base = mmap_anon(len, prot)?;
        Ok(Mapping { base, len })
    }

    pub const fn as_ptr(&self) -> *mut u8 {
        self.base
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 赋权（内核侧同样会做 W^X 判定）。
    pub fn protect(&self, prot: Prot) -> Result<()> {
        mprotect(self.base, self.len, prot)
    }

    /// 提前归还，返回后不再自动 `munmap`。
    pub fn forget(self) -> *mut u8 {
        let base = self.base;
        core::mem::forget(self);
        base
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // 析构里不能失败，也不该回滚：内核侧映射记录会随进程退出一起回收。
        let _ = munmap(self.base, self.len);
    }
}
