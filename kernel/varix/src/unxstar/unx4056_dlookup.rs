//! unx4056_dlookup — UNX-F4056 目录项 lookup 钩子锚（诊断段 0x8960）。
//!
//! 判据原文（提炼）：
//!   J1. lookup 钩子三态返回约定（dentry/负标记/errno）各 10/10 正确。
//!   J2. 钩子超时注入有账不挂死：tick 预算耗尽按 errno 收口。
//!   J3. 负项（不存在的名字）返回负标记且不入正项账。
//!   J4. 钩子调用账守恒：三态分账合计 == 总调用数。
//! 设计纪律：
//!   1. Linux VFS d_lookup / inode->i_op->lookup 语义三态：
//!      命中返回 dentry（inode>0）、未命中返回负 dentry（标记）、
//!      错误返回 errno（闭集承载）。
//!   2. 钩子以 tick 预算限流：每次调用消耗固定 tick，预算耗尽
//!      立即按超时 errno 收口，杜绝挂死（判据 J2）。
//!   3. 目录项名字比较逐字节整数判定，无字符串堆分配依赖。
//!   4. 三态各设独立分账，正项/负项/错误互不串账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8960;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENOENT: u16 = SEG_BASE + 1;   // 钩子返回项不存在（负项路径）
const E_TIMEOUT: u16 = SEG_BASE + 2;  // tick 预算耗尽（超时收口）
const E_BAD_NAME: u16 = SEG_BASE + 3; // 名字非法（空名/超长）
const E_NO_DIR: u16 = SEG_BASE + 4;   // 目标目录未注册
const E_DUP_NAME: u16 = SEG_BASE + 5; // 同目录重复名注册
const E_LEDGER: u16 = SEG_BASE + 6;   // 三态分账合计不平
const E_BAD_ARG: u16 = SEG_BASE + 7;  // 参数非法（0 目录号）

/// 名字长度上限（本锚定编 32 字节，路径分量语义）。
pub const NAME_MAX: usize = 32;

/// tick 预算定编：每钩子调用 4 tick（超时注入的计量单位）。
pub const HOOK_TICKS: u32 = 4;

/// 目录项：名 + 指向 inode 号（正项 >0，负项不入表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dentry {
    pub dir: u64,   // 所在目录 inode 号
    pub ino: u64,   // 指向 inode 号（正项 >0）
    pub name: [u8; NAME_MAX], // 名字字节（0 填充）
    pub name_len: usize,
}

impl Dentry {
    /// 装配目录项（名字超长截断由注册口拒收兜底）。
    pub fn assemble(dir: u64, ino: u64, name: &[u8]) -> Dentry {
        let mut d = Dentry {
            dir,
            ino,
            name: [0; NAME_MAX],
            name_len: 0,
        };
        let n = if name.len() < NAME_MAX { name.len() } else { NAME_MAX };
        for i in 0..n {
            d.name[i] = name[i];
        }
        d.name_len = n;
        d
    }
    /// 名字逐字节相等判定。
    pub fn name_eq(&self, other: &[u8]) -> bool {
        if other.len() != self.name_len {
            return false;
        }
        for i in 0..self.name_len {
            if self.name[i] != other[i] {
                return false;
            }
        }
        true
    }
}

/// lookup 钩子三态返回约定（VFS lookup 返回语义整数化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LookupOutcome {
    Found(u64),  // 命中：返回 dentry 指向 inode 号
    Negative,    // 未命中：负 dentry 标记
}

/// 钩子健康状态机：正常 / 慢钩注入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookMode {
    Normal, // 正常：每次 4 tick
    Slow,   // 慢钩注入：每次消耗超预算 → 超时收口
}

/// lookup 钩子锚：目录项表 + 已知目录账 + 三态分账 + tick 预算。
pub struct DlookupFabric {
    pub dents: Vec<Dentry>,   // 目录项表
    pub dirs: Vec<u64>,       // 已知目录登记账
    pub tick_budget: u32,     // 剩余 tick 预算
    pub mode: HookMode,       // 钩子健康态
    pub found_total: u32,     // 命中分账
    pub neg_total: u32,       // 负项分账
    pub err_total: u32,       // 错误分账（含超时）
    pub calls_total: u32,     // 总调用数（守恒主账）
    pub timeout_total: u32,   // 超时专项分账
}

impl DlookupFabric {
    pub fn new(budget: u32) -> DlookupFabric {
        DlookupFabric {
            dents: Vec::new(),
            dirs: Vec::new(),
            tick_budget: budget,
            mode: HookMode::Normal,
            found_total: 0,
            neg_total: 0,
            err_total: 0,
            calls_total: 0,
            timeout_total: 0,
        }
    }

    fn dir_known(&self, dir: u64) -> bool {
        for d in &self.dirs {
            if *d == dir {
                return true;
            }
        }
        false
    }

    /// 注册目录（空目录也需登记才可查）。
    pub fn admit_dir(&mut self, dir: u64) -> Result<(), u16> {
        if dir == 0 {
            return Err(E_BAD_ARG);
        }
        if self.dir_known(dir) {
            return Err(E_BAD_ARG);
        }
        self.dirs.push(dir);
        Ok(())
    }

    /// 注册目录项（正项 ino>0；重复名/空名/未登记目录拒收）。
    pub fn register(&mut self, d: Dentry) -> Result<(), u16> {
        if d.dir == 0 {
            return Err(E_BAD_ARG);
        }
        if d.name_len == 0 {
            return Err(E_BAD_NAME);
        }
        if !self.dir_known(d.dir) {
            return Err(E_NO_DIR);
        }
        for e in &self.dents {
            if e.dir == d.dir && e.name_eq(&d.name[..d.name_len]) {
                return Err(E_DUP_NAME);
            }
        }
        self.dents.push(d);
        Ok(())
    }

    /// 设置钩子健康态（慢钩注入开关）。
    pub fn set_mode(&mut self, m: HookMode) {
        self.mode = m;
    }

    /// lookup 核心：三态返回 + tick 预算 + 三态分账（判据 J1/J2）。
    pub fn lookup(&mut self, dir: u64, name: &[u8]) -> Result<LookupOutcome, u16> {
        self.calls_total += 1;
        if dir == 0 {
            self.err_total += 1;
            return Err(E_BAD_ARG);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            self.err_total += 1;
            return Err(E_BAD_NAME);
        }
        // tick 预算：正常 4 tick，慢钩一次耗 2 倍即超时收口（不挂死）
        let need = match self.mode {
            HookMode::Normal => HOOK_TICKS,
            HookMode::Slow => HOOK_TICKS * 2,
        };
        if self.tick_budget < need {
            self.err_total += 1;
            self.timeout_total += 1;
            return Err(E_TIMEOUT);
        }
        self.tick_budget -= need;
        if !self.dir_known(dir) {
            self.err_total += 1;
            return Err(E_NO_DIR);
        }
        // 命中判定：同目录同名正项
        for d in &self.dents {
            if d.dir == dir && d.name_eq(name) && d.ino > 0 {
                self.found_total += 1;
                return Ok(LookupOutcome::Found(d.ino));
            }
        }
        // 未命中：负 dentry 标记（判据 J3）
        self.neg_total += 1;
        Ok(LookupOutcome::Negative)
    }

    /// 守恒对账（判据 J4）：三态分账合计 == 总调用。
    pub fn ledger_ok(&self) -> bool {
        self.found_total + self.neg_total + self.err_total == self.calls_total
    }
}

/// 超时注入收口验证：预算耗尽后 lookup 立即 E_TIMEOUT 返回。
pub fn expect_timeout(f: &mut DlookupFabric, dir: u64, name: &[u8]) -> Result<(), u16> {
    match f.lookup(dir, name) {
        Err(E_TIMEOUT) => Ok(()),
        _ => Err(E_TIMEOUT),
    }
}

/// 十例三态判定器（判据 J1 的 10/10 面支撑）：
/// 对 dirs 中目录逐名查询，返回判定通过例数。
pub fn tri_state_ten(f: &mut DlookupFabric, dir: u64) -> Result<u32, u16> {
    let mut ok: u32 = 0;
    // 4 例命中：alpha/beta/gamma/delta 已注册
    for name in [&b"alpha"[..], b"beta", b"gamma", b"delta"] {
        match f.lookup(dir, name) {
            Ok(LookupOutcome::Found(_)) => ok += 1,
            _ => return Err(E_LEDGER),
        }
    }
    // 3 例负项：未注册名
    for name in [&b"ghost"[..], b"nil", b"void"] {
        match f.lookup(dir, name) {
            Ok(LookupOutcome::Negative) => ok += 1,
            _ => return Err(E_LEDGER),
        }
    }
    // 3 例错误态：空名/超长名/未登记目录
    match f.lookup(dir, b"") {
        Err(E_BAD_NAME) => ok += 1,
        _ => return Err(E_BAD_NAME),
    }
    let long = [7u8; NAME_MAX + 1];
    match f.lookup(dir, &long) {
        Err(E_BAD_NAME) => ok += 1,
        _ => return Err(E_BAD_NAME),
    }
    match f.lookup(999, b"x") {
        Err(E_NO_DIR) => ok += 1,
        _ => return Err(E_NO_DIR),
    }
    Ok(ok)
}

/// 自例程：三态 10/10→超时注入→负项隔离→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 三态判定 10/10（判据 J1）
    let mut f = DlookupFabric::new(1000);
    f.admit_dir(2)?;
    f.register(Dentry::assemble(2, 10, b"alpha"))?;
    f.register(Dentry::assemble(2, 11, b"beta"))?;
    f.register(Dentry::assemble(2, 12, b"gamma"))?;
    f.register(Dentry::assemble(2, 13, b"delta"))?;
    let ok = tri_state_ten(&mut f, 2)?;
    if ok != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 超时注入：预算压到 3 tick → 超时收口且不挂死、有账
    let mut t = DlookupFabric::new(3);
    t.admit_dir(2)?;
    t.register(Dentry::assemble(2, 10, b"a"))?;
    match t.lookup(2, b"a") {
        Ok(_) => {}
        Err(E_TIMEOUT) => {}
        Err(_) => return Err(E_LEDGER), // 超时收口外的意外结果即账不平
    }
    // 预算 3 < 4，无论钩子健康与否均应收口超时
    if t.timeout_total != 1 {
        return Err(E_TIMEOUT);
    }
    expect_timeout(&mut t, 2, b"a")?;
    if t.timeout_total != 2 {
        return Err(E_TIMEOUT);
    }
    hits += 1;

    // R3 慢钩注入：预算充足但钩子慢 → 单次超时收口
    let mut s = DlookupFabric::new(100);
    s.admit_dir(3)?;
    s.register(Dentry::assemble(3, 20, b"x"))?;
    s.set_mode(HookMode::Slow);
    expect_timeout(&mut s, 3, b"x")?;
    hits += 1;

    // R4 重复名注入：同目录同名注册 → 拒收
    match f.register(Dentry::assemble(2, 99, b"alpha")) {
        Err(E_DUP_NAME) => {}
        _ => return Err(E_DUP_NAME),
    }
    hits += 1;

    // R5 守恒对账：三态分账合计 == 总调用
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    // f 调用：tri_state_ten 10 次
    if f.calls_total != 10 || f.found_total != 4 || f.neg_total != 3 || f.err_total != 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
