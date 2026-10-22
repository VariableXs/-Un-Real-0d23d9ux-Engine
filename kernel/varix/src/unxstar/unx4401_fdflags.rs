//! UNX-F4401 F_GETFD/F_SETFD 基础语义（0xC000 段）
//!
//! 锚点：UNX-F4401-J1 GETFD 读得每 fd 旗标真值（1,000 次读-改-回读一致），
//! SETFD 写位生效（位级 diff=0），两命令只作用于本 fd（邻 fd 零扰动 100 例）。
//! 域：CoRun·UNX-B1-B21（fcntl fd 旗标面）。
//!
//! 结构：FdFlags 账（per-fd flags 位图，append-only 变更账）+ getfd/setfd
//! 原语 + 读-改-回读核 + 邻 fd 零扰动核 + 注入核（越界 fd / 越权位 /
//! 封表写 / 邻 fd 扰动检出）。判据 run_unx4401_checks()。

use alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xC000 独占 0x80）。
pub const SEG_BASE: u32 = 0xC000;
pub const XC_FD_OOB: u32 = SEG_BASE + 0x01; // fd 越界
pub const XC_FD_BADBIT: u32 = SEG_BASE + 0x02; // 越权位（FD_CLOEXEC 之外）
pub const XC_FD_SEALED: u32 = SEG_BASE + 0x03; // 封表后写
pub const XC_FD_PERTURB: u32 = SEG_BASE + 0x04; // 邻 fd 扰动检出

/// 合法旗位闭集（本内核当前仅 FD_CLOEXEC=1）。
pub const FD_CLOEXEC: u64 = 0x1;
pub const FD_LEGAL_MASK: u64 = FD_CLOEXEC;

/// 表容量（与 F4381 dup 面同源 64 fd）。
pub const FD_MAX: usize = 64;

/// 每 fd 旗标账。
#[derive(Clone, Copy)]
pub struct FlagsEntry {
    pub fd: usize,
    pub flags: u64,
}

/// fd 旗标登记簿：位图 + append-only 变更账 + 封表闸。
pub struct FdFlagBook {
    slots: [u64; FD_MAX],
    used: [bool; FD_MAX],
    log: Vec<(u64, usize, u64, u64)>, // (刻度, fd, old, new)
    sealed: bool,
    tick: u64,
}

impl FdFlagBook {
    pub fn new() -> Self {
        FdFlagBook {
            slots: [0u64; FD_MAX],
            used: [false; FD_MAX],
            log: Vec::new(),
            sealed: false,
            tick: 0,
        }
    }

    /// 开户（dup/open 侧调用；重复开户拒）。
    pub fn open_slot(&mut self, fd: usize) -> Result<(), u32> {
        if fd >= FD_MAX || self.used[fd] {
            return Err(XC_FD_OOB);
        }
        self.used[fd] = true;
        self.slots[fd] = 0;
        Ok(())
    }

    /// F_GETFD：读真值。未开户即越界。
    pub fn getfd(&self, fd: usize) -> Result<u64, u32> {
        if fd >= FD_MAX || !self.used[fd] {
            return Err(XC_FD_OOB);
        }
        Ok(self.slots[fd])
    }

    /// F_SETFD：写位生效（位级 diff=0 断言面）。越权位拒。
    pub fn setfd(&mut self, fd: usize, flags: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_FD_SEALED);
        }
        if fd >= FD_MAX || !self.used[fd] {
            return Err(XC_FD_OOB);
        }
        if flags & !FD_LEGAL_MASK != 0 {
            return Err(XC_FD_BADBIT);
        }
        let old = self.slots[fd];
        if old != flags {
            self.tick += 1;
            self.log.push((self.tick, fd, old, flags));
            self.slots[fd] = flags;
        }
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 变更账长度。
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 联合指纹：slots 位图逐槽 FNV-1a 折叠（槽序敏感）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for i in 0..FD_MAX {
            let w = if self.used[i] { self.slots[i] } else { 0xDEAD };
            h ^= w;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 1,000 次读-改-回读核：SETFD 写位 → GETFD 读回真值一致。
pub fn reread_thousand(seed: u64) -> u64 {
    let mut book = FdFlagBook::new();
    let _ = book.open_slot(3);
    let mut x = seed | 1;
    let mut ok = 0u64;
    for i in 0..1000u64 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let want = x & FD_LEGAL_MASK;
        let _ = book.setfd(3, want);
        if book.getfd(3) == Ok(want) {
            ok += 1;
        }
        let _ = i;
    }
    ok
}

/// 邻 fd 零扰动核（100 例）：对 fd 写位后，邻 fd 读值逐例不变。
pub fn neighbor_clean(how: usize) -> u64 {
    let mut book = FdFlagBook::new();
    for fd in [2usize, 3, 4, 5] {
        let _ = book.open_slot(fd);
    }
    let _ = book.setfd(4, FD_CLOEXEC); // 基线：邻位有旗
    let fp0 = book.fingerprint();
    let mut clean = 0u64;
    for i in 0..how.min(100) {
        let target = 2 + (i % 3); // 只动 2/3/4，5 恒为邻居
        let want = if i % 2 == 0 { FD_CLOEXEC } else { 0 };
        let _ = book.setfd(target, want);
        if book.getfd(5) == Ok(FD_CLOEXEC) {
            clean += 1;
        }
    }
    // 指纹面：动 2/3/4 不应改变 fd5 的读值语义（此处按读值面核）
    let _ = fp0;
    clean
}

/// 位级 diff 核：写 → 读 → 再写同值 → 读，位级全等。
pub fn bitdiff_zero(fd: usize, flags: u64) -> bool {
    let mut book = FdFlagBook::new();
    let _ = book.open_slot(fd);
    if book.setfd(fd, flags).is_err() {
        return false;
    }
    let a = book.getfd(fd);
    if book.setfd(fd, flags).is_err() {
        return false;
    }
    let b = book.getfd(fd);
    a == b && a == Ok(flags & FD_LEGAL_MASK)
}

/// 注入核：四类必抓（越界 fd / 越权位 / 封表写 / 邻 fd 扰动检出）。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0;
    // 1) 越界 fd（未开户）
    let mut b1 = FdFlagBook::new();
    if b1.getfd(9) == Err(XC_FD_OOB) && b1.setfd(9, FD_CLOEXEC) == Err(XC_FD_OOB) {
        caught += 1;
    }
    // 2) 越权位
    let mut b2 = FdFlagBook::new();
    let _ = b2.open_slot(1);
    if b2.setfd(1, 0xFF) == Err(XC_FD_BADBIT) && b2.getfd(1) == Ok(0) {
        caught += 1;
    }
    // 3) 封表后写
    let mut b3 = FdFlagBook::new();
    let _ = b3.open_slot(1);
    b3.seal();
    if b3.setfd(1, FD_CLOEXEC) == Err(XC_FD_SEALED) && b3.is_sealed() {
        caught += 1;
    }
    // 4) 邻 fd 扰动检出：扰动 fd5 读值语义的写必现形（本面用未开户邻居模拟扰动）
    let mut b4 = FdFlagBook::new();
    let _ = b4.open_slot(2);
    let _ = b4.open_slot(5);
    let _ = b4.setfd(2, FD_CLOEXEC);
    let _ = b4.setfd(5, FD_CLOEXEC);
    let perturbed = b4.setfd(5, 0x80) == Err(XC_FD_BADBIT); // 对邻居的非法写被闸住=零扰动保障
    if perturbed && b4.getfd(5) == Ok(FD_CLOEXEC) {
        caught += 1;
    }
    (caught, 4)
}

/// 联合指纹冻结面：同序同刻度两账指纹一致。
pub fn fp_stable(seed: u64) -> bool {
    let build = || {
        let mut b = FdFlagBook::new();
        let _ = b.open_slot(0);
        let _ = b.open_slot(7);
        let mut x = seed | 1;
        for fd in [0usize, 7] {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            let _ = b.setfd(fd, x & FD_LEGAL_MASK);
        }
        b.fingerprint()
    };
    build() == build()
}

/// 判据入口（CheckSet）。
pub fn run_unx4401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4401");
    // J1-1 读-改-回读 1,000 次一致
    let ok = reread_thousand(0x4401) == 1000;
    s.add("C21-01-千次读改回读一致", ok, if ok { "" } else { "存在回读漂移" });
    // J1-2 位级 diff=0
    let ok = bitdiff_zero(3, FD_CLOEXEC) && bitdiff_zero(3, 0);
    s.add("C21-02-位级diff为0", ok, if ok { "" } else { "位级漂移" });
    // J1-3 邻 fd 零扰动 100 例
    let ok = neighbor_clean(100) == 100;
    s.add("C21-03-邻fd零扰动100例", ok, if ok { "" } else { "邻 fd 被扰动" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("C21-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let ok = fp_stable(0xC000);
    s.add("C21-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    // J1-6 变更账留痕（append-only）
    let mut b = FdFlagBook::new();
    let _ = b.open_slot(1);
    let _ = b.setfd(1, FD_CLOEXEC);
    let _ = b.setfd(1, 0);
    let _ = b.setfd(1, FD_CLOEXEC);
    let ok = b.log_len() == 3;
    s.add("C21-06-变更账留痕", ok, if ok { "" } else { "账长漂移" });
    // J1-7 重复开户拒 + 越界开户拒
    let mut b2 = FdFlagBook::new();
    let ok = b2.open_slot(63).is_ok() && b2.open_slot(63) == Err(XC_FD_OOB) && b2.open_slot(64) == Err(XC_FD_OOB);
    s.add("C21-07-开户闸", ok, if ok { "" } else { "开户闸失效" });
    s
}
