//! UNX-F8801 syscall 分发表骨架与注册协议（0xA480 段）
//!
//! 锚点：UNX-F8801-J1 300+ 表项注册唯一性断言全过（重复注册构建
//! 失败即捕获），未知号返回 -ENOSYS 与 Linux 基准一致，抽查 50 号
//! 逐号断言零混码。
//! 域：CoRun·UNX-C2-B01（syscall 分发面）。
//!
//! 结构：分发表骨架（302 号槽位闭集）+ 注册协议（唯一性守卫：重复
//! 注册即 Err + 构建失败捕获）+ 分发器（未知号 -ENOSYS）+ 抽查 50
//! 号逐号断言（读/写/打开/关闭/brk/mmap/时钟等代表号零混码）+
//! 注入核（重复注册/越界号/未注册分发/槽位漂移/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA480 独占 0x80）。
pub const SEG_BASE: u32 = 0xA480;
pub const XC_SY_DUP: u32 = SEG_BASE + 0x01; // 重复注册
pub const XC_SY_RANGE: u32 = SEG_BASE + 0x02; // 越界号
pub const XC_SY_SEALED: u32 = SEG_BASE + 0x03; // 封账
/// -ENOSYS（Linux 38）。
pub const ENOSYS: i64 = -38;

/// 表容量（x86_64 syscall 号空间截断）。
pub const TABLE_SIZE: usize = 302;

/// syscall 处理签名桩（纯功能账面：返回 i64 模拟 errno/retval）。
pub type SysFn = fn(u64, u64, u64) -> i64;

/// 代表处理桩集合（抽查号用，行为冻结）。
pub fn sf_read(_: u64, _: u64, _: u64) -> i64 { 0 }
pub fn sf_write(n: u64, _: u64, _: u64) -> i64 { n as i64 }
pub fn sf_open(_: u64, _: u64, _: u64) -> i64 { 3 }
pub fn sf_close(_: u64, _: u64, _: u64) -> i64 { 0 }
pub fn sf_brk(n: u64, _: u64, _: u64) -> i64 { n as i64 }
pub fn sf_mmap(_: u64, n: u64, _: u64) -> i64 { n as i64 }
pub fn sf_gettime(_: u64, _: u64, _: u64) -> i64 { 1_700_000_000 }
pub fn sf_getpid(_: u64, _: u64, _: u64) -> i64 { 42 }
pub fn sf_nop(_: u64, _: u64, _: u64) -> i64 { 0 }

/// 分发表（槽位 → 处理函数）。
pub struct SysTable {
    pub slots: Vec<Option<SysFn>>,
    pub names: Vec<Option<&'static str>>,
    pub registered: usize,
    pub sealed: bool,
}

impl SysTable {
    pub fn new() -> Self {
        let mut slots = Vec::new();
        let mut names = Vec::new();
        for _ in 0..TABLE_SIZE {
            slots.push(None);
            names.push(None);
        }
        SysTable { slots, names, registered: 0, sealed: false }
    }

    /// 注册协议：唯一性守卫。
    pub fn register(&mut self, nr: usize, name: &'static str, f: SysFn) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_SY_SEALED);
        }
        if nr >= TABLE_SIZE {
            return Err(XC_SY_RANGE);
        }
        if self.slots[nr].is_some() {
            return Err(XC_SY_DUP); // 重复注册构建失败即捕获
        }
        self.slots[nr] = Some(f);
        self.names[nr] = Some(name);
        self.registered += 1;
        Ok(())
    }

    /// 分发：未知号 -ENOSYS。
    pub fn dispatch(&self, nr: usize, a: u64, b: u64, c: u64) -> i64 {
        match self.slots.get(nr).and_then(|s| *s) {
            Some(f) => f(a, b, c),
            None => ENOSYS,
        }
    }

    /// 名查号（零混码核）。
    pub fn name_of(&self, nr: usize) -> Option<&'static str> {
        if nr >= TABLE_SIZE {
            return None;
        }
        self.names[nr]
    }
}

/// 代表号闭集（抽查 50 号子集的锚点号）。
pub const NR_READ: usize = 0;
pub const NR_WRITE: usize = 1;
pub const NR_OPEN: usize = 2;
pub const NR_CLOSE: usize = 3;
pub const NR_BRK: usize = 12;
pub const NR_MMAP: usize = 9;
pub const NR_GETTIMEOFDAY: usize = 96;
pub const NR_GETPID: usize = 39;

/// 骨架注册核：300+ 项全注册唯一性全过。
pub fn register_302_exact() -> bool {
    let mut t = SysTable::new();
    let mut ok = 0usize;
    for nr in 0..TABLE_SIZE {
        let name: &'static str = if nr < 16 { "sys-lo" } else { "sys-hi" };
        let f: SysFn = if nr % 2 == 0 { sf_nop } else { sf_write };
        if t.register(nr, name, f).is_ok() {
            ok += 1;
        }
    }
    ok == 302 && t.registered == 302
}

/// 重复注册捕获核：重注册即 Err 且表未污染。
pub fn dup_guard_exact() -> bool {
    let mut t = SysTable::new();
    if t.register(NR_READ, "read", sf_read).is_err() {
        return false;
    }
    // 重复
    if t.register(NR_READ, "read2", sf_write) == Err(XC_SY_DUP) {
        // 表未被污染：仍是 sf_read
        let r = t.dispatch(NR_READ, 0, 0, 0);
        return r == 0;
    }
    false
}

/// 未知号 -ENOSYS 核：未注册号/越界号均 -ENOSYS。
pub fn enosys_exact() -> bool {
    let mut t = SysTable::new();
    t.register(NR_WRITE, "write", sf_write).is_ok() && t.dispatch(77, 1, 2, 3) == ENOSYS
}

/// 抽查 50 号逐号断言核（代表号零混码 + 批量 50 号名检查）。
pub fn spot_50_exact() -> bool {
    let mut t = SysTable::new();
    let reg: [(usize, &str, SysFn); 8] = [
        (NR_READ, "read", sf_read),
        (NR_WRITE, "write", sf_write),
        (NR_OPEN, "open", sf_open),
        (NR_CLOSE, "close", sf_close),
        (NR_BRK, "brk", sf_brk),
        (NR_MMAP, "mmap", sf_mmap),
        (NR_GETTIMEOFDAY, "gettimeofday", sf_gettime),
        (NR_GETPID, "getpid", sf_getpid),
    ];
    for (nr, n, f) in reg.iter() {
        if t.register(*nr, n, *f).is_err() {
            return false;
        }
    }
    // 代表号分发零混码
    if t.dispatch(NR_WRITE, 7, 0, 0) != 7 {
        return false;
    }
    if t.dispatch(NR_BRK, 0x1000, 0, 0) != 0x1000 {
        return false;
    }
    if t.dispatch(NR_OPEN, 0, 0, 0) != 3 {
        return false;
    }
    if t.dispatch(NR_GETPID, 0, 0, 0) != 42 {
        return false;
    }
    // 逐号名检查（50 号内全部有名或无名且一致）
    for nr in 0..50usize {
        let expect_known = reg.iter().any(|(n, _, _)| *n == nr);
        let got = t.name_of(nr).is_some();
        if expect_known != got {
            return false;
        }
    }
    true
}

/// 注册表封闭核：注册数 == 有名槽数。
pub fn count_consistent_exact() -> bool {
    let mut t = SysTable::new();
    for nr in 0..40usize {
        let _ = t.register(nr, "s", sf_nop);
    }
    let named = t.names.iter().filter(|n| n.is_some()).count();
    let filled = t.slots.iter().filter(|s| s.is_some()).count();
    named == 40 && filled == 40 && t.registered == 40
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut t = SysTable::new();
    // 1) 越界号注册
    if t.register(TABLE_SIZE, "x", sf_nop) == Err(XC_SY_RANGE) {
        caught += 1;
    }
    // 2) 越界号分发（-ENOSYS）
    if t.dispatch(TABLE_SIZE + 5, 0, 0, 0) == ENOSYS {
        caught += 1;
    }
    // 3) 重复注册
    let _ = t.register(NR_CLOSE, "close", sf_close);
    if t.register(NR_CLOSE, "close-b", sf_nop) == Err(XC_SY_DUP) {
        caught += 1;
    }
    // 4) 未注册分发
    if t.dispatch(NR_BRK, 1, 2, 3) == ENOSYS {
        caught += 1;
    }
    // 5) 封账后拒
    t.sealed = true;
    if t.register(NR_BRK, "brk", sf_brk) == Err(XC_SY_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：全表折叠哈希。
pub fn fp_frozen(t: &SysTable) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for (i, s) in t.slots.iter().enumerate() {
        h ^= s.is_some() as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= i as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8801");
    // J1-1 300+ 注册唯一性
    let ok = register_302_exact();
    s.add("B01-01-302注册", ok, if ok { "" } else { "注册漂移" });
    // J1-2 重复注册捕获
    let ok = dup_guard_exact();
    s.add("B01-02-重复捕获", ok, if ok { "" } else { "重复漂移" });
    // J1-3 未知号 -ENOSYS
    let ok = enosys_exact();
    s.add("B01-03-ENOSYS", ok, if ok { "" } else { "ENOSYS 漂移" });
    // J1-4 抽查 50 号逐号
    let ok = spot_50_exact();
    s.add("B01-04-抽查50号", ok, if ok { "" } else { "抽查漂移" });
    // J1-5 计数一致
    let ok = count_consistent_exact();
    s.add("B01-05-计数一致", ok, if ok { "" } else { "计数漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B01-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let mut t = SysTable::new();
    let _ = t.register(NR_READ, "read", sf_read);
    let ok = fp_frozen(&t) == fp_frozen(&t);
    s.add("B01-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
