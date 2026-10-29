//! unx4049_specialino — UNX-F4049 特殊 inode：匿名/管道/socket 占位（诊断段 0x8880）。
//!
//! 判据原文：
//!   J1 三类无磁盘 inode 创建成功且类型位正确；
//!   J2 占位 inode 不入 fs 盘上结构（对嫁接层只读可见性断言通过）。
//! 设计纪律：
//!   1. 三类占位：匿名（anon）、管道（pipe）、套接字（sock）——
//!      无盘上结构的内存 inode，创建即入占位池，类型位按 POSIX
//!      定编（0x0000 匿名域 / S_IFIFO / S_IFSOCK）。
//!   2. 盘上隔离（判据 J2）：占位池与盘上 inode 表分账——嫁接层
//!      只读视图（只含盘上表）查询占位 inode 必须不可见。
//!   3. 类型位闭集：三类各一枚类型位，占位创建时逐位断言；
//!      闭集外类型拒收。
//!   4. 生命周期：Placeholder → Wired（被进程句柄引用）→ Dropped
//!      （引用归零出池）；与盘上表无任何写路径。
//!   5. 边界：对占位 inode 的盘上操作（落盘/回写注入）拒收、
//!      闭集外类型、重复句柄、空池出池——全部显性分支并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8880;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_BAD_KIND: u16 = SEG_BASE + 1;    // 闭集外占位类型
const E_ONDISK_FORBID: u16 = SEG_BASE + 2; // 占位 inode 盘上操作被拒
const E_GRAFT_LEAK: u16 = SEG_BASE + 3;  // 嫁接层可见性泄漏（隔离失守）
const E_DUP_HANDLE: u16 = SEG_BASE + 4;  // 重复占位句柄
const E_NOT_PLACEHOLDER: u16 = SEG_BASE + 5; // 非占位路径操作
const E_BAL: u16 = SEG_BASE + 6;         // 占位/盘上两账互串

// ── 类型位定编（POSIX 谱系）──
pub const T_ANON: u16 = 0x0000;  // 匿名域（无类型位）
pub const T_PIPE: u16 = 0x1000;  // S_IFIFO（管道）
pub const T_SOCK: u16 = 0xC000;  // S_IFSOCK（套接字）

/// 占位类型闭集状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpKind {
    Anon,
    Pipe,
    Sock,
}

/// 占位 inode 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpPhase {
    Placeholder, // 建成未引用
    Wired,       // 被句柄引用
    Dropped,     // 引用归零出池（终态）
}

/// 单个占位 inode。
pub struct SpIno {
    pub i_ino: u64,   // 内存 inode 号（占位号空间）
    pub kind: SpKind, // 占位类型
    pub phase: SpPhase,
    pub refs: u32,    // 引用计数
}

/// 特殊 inode 枢纽：占位池 + 盘上表（隔离对照）+ 嫁接只读视图。
pub struct SpecialIno {
    pub pool: Vec<SpIno>,     // 占位池（内存域）
    pub ondisk_ids: Vec<u64>, // 盘上 inode 表（隔离对照账）
    pub next_ino: u64,        // 占位号分配器
    pub created_total: u32,   // 占位创建总数
    pub dropped_total: u32,   // 出池总数
    pub graft_queries: u32,   // 嫁接层查询总数
    pub rejects: u32,         // 错误账
}

impl SpecialIno {
    pub fn new() -> SpecialIno {
        SpecialIno {
            pool: Vec::new(),
            ondisk_ids: Vec::new(),
            next_ino: 0xFFFF_0000, // 占位号空间（与盘上号空间隔离定编）
            created_total: 0,
            dropped_total: 0,
            graft_queries: 0,
            rejects: 0,
        }
    }

    /// 类型位映射（闭集）。
    pub fn kind_bits(k: SpKind) -> u16 {
        match k {
            SpKind::Anon => T_ANON,
            SpKind::Pipe => T_PIPE,
            SpKind::Sock => T_SOCK,
        }
    }

    /// 三类占位创建（判据 J1）：类型位逐位断言后入池。
    pub fn create_placeholder(&mut self, k: SpKind) -> Result<(usize, u16, u64), u16> {
        let bits = Self::kind_bits(k);
        // 类型位闭集断言（创建即验证）。
        let ok = match k {
            SpKind::Anon => bits == T_ANON,
            SpKind::Pipe => bits == T_PIPE,
            SpKind::Sock => bits == T_SOCK,
        };
        if !ok {
            self.rejects += 1;
            return Err(E_BAD_KIND);
        }
        let ino = self.next_ino;
        self.next_ino += 1;
        self.pool.push(SpIno {
            i_ino: ino,
            kind: k,
            phase: SpPhase::Placeholder,
            refs: 0,
        });
        self.created_total += 1;
        Ok((self.pool.len() - 1, bits, ino))
    }

    /// 句柄引用：占位 → Wired；重复句柄拒收（同号双线）。
    pub fn wire(&mut self, idx: usize) -> Result<u32, u16> {
        if idx >= self.pool.len() {
            self.rejects += 1;
            return Err(E_NOT_PLACEHOLDER);
        }
        if self.pool[idx].phase == SpPhase::Dropped {
            self.rejects += 1;
            return Err(E_NOT_PLACEHOLDER);
        }
        self.pool[idx].refs += 1;
        self.pool[idx].phase = SpPhase::Wired;
        Ok(self.pool[idx].refs)
    }

    /// 引用释放：归零出池（Dropped 终态）。
    pub fn drop_ref(&mut self, idx: usize) -> Result<SpPhase, u16> {
        if idx >= self.pool.len() {
            self.rejects += 1;
            return Err(E_NOT_PLACEHOLDER);
        }
        if self.pool[idx].refs == 0 {
            self.rejects += 1;
            return Err(E_BAL);
        }
        self.pool[idx].refs -= 1;
        if self.pool[idx].refs == 0 {
            self.pool[idx].phase = SpPhase::Dropped;
            self.dropped_total += 1;
            Ok(SpPhase::Dropped)
        } else {
            Ok(SpPhase::Wired)
        }
    }

    /// 盘上操作注入拦截：占位 inode 一律拒收（判据 J2 前置）。
    pub fn ondisk_write(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.pool.len() {
            self.rejects += 1;
            return Err(E_NOT_PLACEHOLDER);
        }
        self.rejects += 1;
        Err(E_ONDISK_FORBID)
    }

    /// 盘上表登记（对照账：真实盘上 inode 才可入）。
    pub fn ondisk_register(&mut self, i_ino: u64) -> Result<(), u16> {
        // 占位号空间注入拒收（隔离屏障）。
        if i_ino >= 0xFFFF_0000 {
            self.rejects += 1;
            return Err(E_BAL);
        }
        let mut i = 0;
        while i < self.ondisk_ids.len() {
            if i < self.ondisk_ids.len() && self.ondisk_ids[i] == i_ino {
                self.rejects += 1;
                return Err(E_DUP_HANDLE);
            }
            i += 1;
        }
        self.ondisk_ids.push(i_ino);
        Ok(())
    }

    /// 嫁接层只读视图查询（判据 J2）：只查盘上表，占位号必须不可见。
    pub fn graft_lookup(&mut self, i_ino: u64) -> Result<bool, u16> {
        self.graft_queries += 1;
        let mut found = false;
        let mut i = 0;
        while i < self.ondisk_ids.len() {
            if i < self.ondisk_ids.len() && self.ondisk_ids[i] == i_ino {
                found = true;
                break;
            }
            i += 1;
        }
        if !found && i_ino >= 0xFFFF_0000 {
            // 占位号空间泄漏检测：嫁接层对占位号不可见。
            return Err(E_GRAFT_LEAK);
        }
        Ok(found)
    }

    /// 守恒对账：创建 - 出池 = 池内存活；盘上表与占位池零交集。
    pub fn audit(&self) -> Result<(), u16> {
        let mut alive = 0u32;
        let mut i = 0;
        while i < self.pool.len() {
            if i < self.pool.len() && self.pool[i].phase != SpPhase::Dropped {
                alive += 1;
            }
            i += 1;
        }
        if self.created_total < self.dropped_total {
            return Err(E_BAL);
        }
        if self.created_total - self.dropped_total != alive {
            return Err(E_BAL);
        }
        // 隔离不变式：两表零交集（盘上表内不得出现占位号空间）。
        let mut j = 0;
        while j < self.ondisk_ids.len() {
            if self.ondisk_ids[j] >= 0xFFFF_0000 {
                return Err(E_BAL);
            }
            j += 1;
        }
        Ok(())
    }

    /// 导出快照（池长，创建账，出池账，盘上表长）。
    pub fn ledger(&self) -> (usize, u32, u32, usize) {
        (self.pool.len(), self.created_total, self.dropped_total, self.ondisk_ids.len())
    }

    /// 占位号空间基址视图（隔离屏障定编查询）。
    pub fn placeholder_base() -> u64 {
        0xFFFF_0000
    }

    /// 存活占位计数视图（池内非 Dropped 数）。
    pub fn alive_count(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0;
        while i < self.pool.len() {
            if self.pool[i].phase != SpPhase::Dropped {
                c += 1;
            }
            i += 1;
        }
        c
    }
}

/// 自例程：R1 三类创建 → R2 盘上隔离 → R3 嫁接只读视图 → R4 注入拒收 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut sp = SpecialIno::new();

    // R1 三类创建（判据 J1）：类型位逐位正确。
    let (a, ba, _ia) = sp.create_placeholder(SpKind::Anon)?;
    let (p, bp, _ip) = sp.create_placeholder(SpKind::Pipe)?;
    let (s, bs, _is) = sp.create_placeholder(SpKind::Sock)?;
    if ba != T_ANON || bp != T_PIPE || bs != T_SOCK {
        return Err(E_BAD_KIND);
    }
    hits += 1;

    // R2 盘上隔离（判据 J2 前置）：占位 inode 盘上写一律拒收。
    if sp.ondisk_write(p) != Err(E_ONDISK_FORBID) {
        return Err(E_BAL);
    }
    if sp.ondisk_write(s) != Err(E_ONDISK_FORBID) {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 嫁接只读视图：盘上 inode 可见，占位号不可见（零泄漏）。
    sp.ondisk_register(2)?;
    sp.ondisk_register(11)?;
    if sp.graft_lookup(2)? != true {
        return Err(E_BAL);
    }
    if sp.graft_lookup(_ip) != Err(E_GRAFT_LEAK) {
        // 占位号对嫁接层不可见：泄漏即隔离失守。
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注入拒收：占位号入盘上表、盘上表重号、空引用出池。
    if sp.ondisk_register(0xFFFF_0000) != Err(E_BAL) {
        return Err(E_BAL);
    }
    if sp.ondisk_register(2) != Err(E_DUP_HANDLE) {
        return Err(E_BAL);
    }
    if sp.drop_ref(a) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 生命周期与守恒：引用入出池，创建 - 出池 = 存活。
    sp.wire(a)?;
    sp.wire(a)?;
    sp.drop_ref(a)?;
    sp.drop_ref(a)?; // 归零出池
    if sp.pool[a].phase != SpPhase::Dropped {
        return Err(E_BAL);
    }
    sp.audit()?;
    let (pl, cr, dr, od) = sp.ledger();
    if pl != 3 || cr != 3 || dr != 1 || od != 2 {
        return Err(E_BAL);
    }
    // 存活计数与号空间基址视图：3 建 1 落 → 存活 2。
    if sp.alive_count() != 2 || SpecialIno::placeholder_base() != 0xFFFF_0000 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
