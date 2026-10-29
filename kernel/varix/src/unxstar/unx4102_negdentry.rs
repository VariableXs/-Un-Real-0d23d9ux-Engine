//! unx4102_negdentry — UNX-F4102 负 dentry 语义（不存在也入账）（诊断段 0x8EC0）。
//!
//! 判据原文（提炼）：
//!   J1 查找失败结果入负缓存：负缓存计数 == 查找失败数（守恒）。
//!   J2 负命中返回 ENOENT 且不穿透 fs 层：首次失败穿透计数 == 1，
//!      其后负命中穿透计数不再增长。
//!   J3 负条目有明确标记位可区分：kind == Negative 与正条目互斥可查。
//!   J4 同路径重复失败不重复入账：账守恒，重复注入被拒（E_DUP_ENTRY）。
//!   J5 空名/超长名/超界查找全拒收并记错误账。
//! 设计纪律：
//!   1. 负缓存与正缓存同表异旗：Dentry.kind 区分 Positive/Negative，
//!      负条目 ino 恒为 0，正条目 ino 非零，双保险可交叉验证。
//!   2. fs 穿透单一入口：lookup 只在缓存未命中时调 fs 钩子恰好一次，
//!      命中（正/负皆算）零穿透。
//!   3. 守恒账：fail_count == negative_count；每次 lookup 三态
//!      （正命中/负命中/穿透后入账）恰居其一，事件账不灭失。
//!   4. 名长上限写死：NAME_MAX = 255（Linux include/uapi/linux/limits.h），
//!      超限与空名拒收。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8EC0 本单独占。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8EC0;

/// 名字长度上限：Linux UAPI limits.h 的 NAME_MAX（来源常量）。
pub const NAME_MAX: usize = 255;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_EMPTY: u16 = SEG_BASE + 1; // 空名拒收
const E_NAME_LONG: u16 = SEG_BASE + 2;  // 名超 NAME_MAX 拒收
const E_DUP_ENTRY: u16 = SEG_BASE + 3;  // 同名条目重复入账（注入）
const E_NOT_FOUND: u16 = SEG_BASE + 4;  // 点名条目不在账（ENOENT 语义）
const E_KIND_MIX: u16 = SEG_BASE + 5;   // kind 与 ino 交叉验证失守
const E_BAL: u16 = SEG_BASE + 6;        // 失败账与负缓存账不平（守恒失守）
const E_EVENT_LEAK: u16 = SEG_BASE + 7; // lookup 事件三态不恰居其一

/// ENOENT 的 Linux errno 值（uapi/asm-generic/errno-base.h）。
pub const ENOENT: u16 = 2;

/// dentry 种类：正/负二值，明确标记位（判据 J3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DentryKind {
    Positive, // 目录项指向真实 inode（ino != 0）
    Negative, // 目录项确认不存在（ino == 0），即负缓存条目
}

/// 单条 dentry：名 + kind + ino + 命中账。
pub struct Dentry {
    pub name: String,   // 名字（非空、≤NAME_MAX）
    pub kind: DentryKind,
    pub ino: u32,       // 正条目 inode 号；负条目恒 0
    pub hits: u32,      // 本条目命中次数
}

impl Dentry {
    /// kind 与 ino 交叉验证（判据 J3 双保险）。
    fn coherent(&self) -> bool {
        match self.kind {
            DentryKind::Positive => self.ino != 0,
            DentryKind::Negative => self.ino == 0,
        }
    }
}

/// 查找结果三态：恰居其一（判据守恒的事件分类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LookupOutcome {
    PosHit,   // 正命中，零穿透
    NegHit,   // 负命中，返回 ENOENT，零穿透
    MissThenNeg, // 未命中→穿透 fs 一次→失败入负缓存
}

/// 负缓存枢纽：条目账 + 查找账 + 穿透账 + fs 实态供给。
pub struct NegCache {
    pub entries: Vec<Dentry>,  // 缓存条目（正负混存，kind 区分）
    pub fs_files: Vec<String>, // fs 层真实存在名集合（模拟 ramfs 实态）
    pub lookups: u32,          // lookup 总请求数
    pub pos_hits: u32,         // 正命中数
    pub neg_hits: u32,         // 负命中数（返回 ENOENT 不穿透）
    pub fails: u32,            // 查找失败（穿透后确认不存在）数
    pub penetrations: u32,     // fs 穿透总次数（判据 J2 核对面）
    pub negatives: u32,        // 负缓存条目计数（判据 J1 对账面）
    pub rejected: u32,         // 非法输入拒收计数
}

impl NegCache {
    pub fn new() -> NegCache {
        NegCache {
            entries: Vec::new(),
            fs_files: Vec::new(),
            lookups: 0,
            pos_hits: 0,
            neg_hits: 0,
            fails: 0,
            penetrations: 0,
            negatives: 0,
            rejected: 0,
        }
    }

    /// 名字合法性门槛（判据 J5）。
    fn name_ok(name: &str) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if name.len() > NAME_MAX {
            return Err(E_NAME_LONG);
        }
        Ok(())
    }

    /// 在账定位（缓存键 = 名字；同名唯一）。
    fn find(&self, name: &str) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].name == name {
                return Some(i);
            }
        }
        None
    }

    /// fs 实态探测（模拟 fs 钩子）：存在→Some(ino)，不存在→None。
    fn fs_probe(&self, name: &str) -> Option<u32> {
        for i in 0..self.fs_files.len() {
            if self.fs_files[i] == name {
                // 确定性 ino：基址 1000 + 槽位（无 rand）
                return Some(1000 + i as u32);
            }
        }
        None
    }

    /// 供给 fs 实态（重复供给拒收，实态集同名唯一）。
    pub fn fs_create(&mut self, name: &str) -> Result<(), u16> {
        NegCache::name_ok(name)?;
        for f in &self.fs_files {
            if f == name {
                self.rejected += 1;
                return Err(E_DUP_ENTRY);
            }
        }
        self.fs_files.push(name.to_string());
        Ok(())
    }

    /// fs 删除实态（unlink 语义：实态消失，缓存另由失效模块处置）。
    pub fn fs_unlink(&mut self, name: &str) -> Result<(), u16> {
        NegCache::name_ok(name)?;
        let mut idx = self.fs_files.len();
        for i in 0..self.fs_files.len() {
            if self.fs_files[i] == name {
                idx = i;
                break;
            }
        }
        if idx == self.fs_files.len() {
            self.rejected += 1;
            return Err(E_NOT_FOUND);
        }
        // 定位后换尾摘除（避免中途移位）
        let last = self.fs_files.len() - 1;
        self.fs_files.swap(idx, last);
        self.fs_files.pop();
        Ok(())
    }

    /// 负条目注入入口（判据 J4）：同名已在账即拒，防重复入账。
    pub fn inject_negative(&mut self, name: &str) -> Result<(), u16> {
        NegCache::name_ok(name)?;
        if self.find(name).is_some() {
            self.rejected += 1;
            return Err(E_DUP_ENTRY);
        }
        self.entries.push(Dentry {
            name: name.to_string(),
            kind: DentryKind::Negative,
            ino: 0,
            hits: 0,
        });
        self.negatives += 1;
        Ok(())
    }

    /// 核心查找（判据 J1/J2/J3）：三态事件恰居其一，
    /// 穿透只在未命中发生且恰好一次。
    pub fn lookup(&mut self, name: &str) -> Result<LookupOutcome, u16> {
        NegCache::name_ok(name)?;
        self.lookups += 1;
        // 命中路径：零穿透
        if let Some(i) = self.find(name) {
            self.entries[i].hits += 1;
            if self.entries[i].kind == DentryKind::Positive {
                self.pos_hits += 1;
                return Ok(LookupOutcome::PosHit);
            }
            self.neg_hits += 1;
            return Err(ENOENT); // 负命中：ENOENT 且零穿透
        }
        // 未命中：穿透恰好一次（判据 J2）
        self.penetrations += 1;
        match self.fs_probe(name) {
            Some(ino) => {
                // fs 说存在：补正条目（正缓存填充）
                self.entries.push(Dentry {
                    name: name.to_string(),
                    kind: DentryKind::Positive,
                    ino,
                    hits: 1,
                });
                self.pos_hits += 1;
                Ok(LookupOutcome::PosHit)
            }
            None => {
                // fs 说不存在：失败入账（判据 J1）
                self.fails += 1;
                self.entries.push(Dentry {
                    name: name.to_string(),
                    kind: DentryKind::Negative,
                    ino: 0,
                    hits: 1,
                });
                self.negatives += 1;
                self.neg_hits += 1;
                Err(ENOENT)
            }
        }
    }

    /// 守恒对账三连（判据 J1/J2/事件守恒）：
    /// ① 失败账 == 负缓存账；② 穿透账 ≤ 请求账；③ 事件三态和 == 请求账。
    pub fn conserved(&self) -> bool {
        if self.fails != self.negatives {
            return false; // 判据 J1 守恒
        }
        let events = self.pos_hits + self.neg_hits;
        if events != self.lookups {
            return false; // 事件不灭失（穿透后入账也计一次 neg/pos 事件）
        }
        if self.penetrations > self.lookups {
            return false;
        }
        // kind 与 ino 全表交叉验证
        for d in &self.entries {
            if !d.coherent() {
                return false;
            }
        }
        true
    }

    /// 条目总数快照。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// 自例程：R1 失败入账→R2 负命中零穿透→R3 注入拦截→R4 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常路径：建 fs 实态 → 首查不存在 → 穿透 1 次 → 负条目入账。
    let mut c = NegCache::new();
    c.fs_create("a.txt")?;
    c.fs_create("b.txt")?;
    // 正命中路径：首查 a.txt 穿透一次并填正缓存，再查正命中零穿透。
    if c.lookup("a.txt")? != LookupOutcome::PosHit {
        return Err(E_KIND_MIX);
    }
    let pen_after_pos = c.penetrations;
    if c.lookup("a.txt")? != LookupOutcome::PosHit || c.penetrations != pen_after_pos {
        return Err(E_EVENT_LEAK); // 正缓存再查不得再穿透
    }
    // 失败路径：查不存在名 → 穿透一次 → 入负账 → 返回 ENOENT。
    if c.lookup("ghost") != Err(ENOENT) {
        return Err(E_KIND_MIX);
    }
    if c.negatives != 1 || c.fails != 1 || c.penetrations != pen_after_pos + 1 {
        return Err(E_BAL); // 判据 J1：负缓存计数==失败数
    }
    hits += 1;

    // R2 负命中：再查 ghost 返回 ENOENT 且穿透不再增长（判据 J2）。
    let pen_before = c.penetrations;
    if c.lookup("ghost") != Err(ENOENT) {
        return Err(E_KIND_MIX);
    }
    if c.penetrations != pen_before {
        return Err(E_EVENT_LEAK); // 负命中零穿透
    }
    if c.neg_hits != 2 || c.negatives != 1 {
        return Err(E_BAL); // 负命中计事件但不重复入账
    }
    hits += 1;

    // R3 注入拦截：重复负注入、空名、超长名、未建实态删除全拒。
    if c.inject_negative("ghost").is_ok() {
        return Err(E_DUP_ENTRY); // 重复入账注入必须拒
    }
    let long_name = "x".repeat(NAME_MAX + 1);
    if c.lookup(&long_name).is_ok() {
        return Err(E_NAME_LONG); // 超长名应拒
    }
    if c.lookup("").is_ok() {
        return Err(E_NAME_EMPTY); // 空名应拒
    }
    if c.fs_unlink("nope").is_ok() {
        return Err(E_NOT_FOUND); // 删不存在实态应拒
    }
    hits += 1;

    // R4 守恒对账：unlink 实态后旧正条目仍在账（失效归失效模块管），
    // 但总事件账、kind 交叉验证必须全绿。
    c.fs_unlink("b.txt")?;
    if !c.conserved() {
        return Err(E_BAL);
    }
    let total = c.entry_count();
    if total != 2 {
        return Err(E_BAL); // a.txt(正) + ghost(负)
    }
    // 正负标记位可区分（判据 J3）：账内一正一负。
    let mut pos_n = 0u32;
    let mut neg_n = 0u32;
    for i in 0..c.entries.len() {
        match c.entries[i].kind {
            DentryKind::Positive => pos_n += 1,
            DentryKind::Negative => neg_n += 1,
        }
    }
    if pos_n != 1 || neg_n != 1 {
        return Err(E_KIND_MIX);
    }
    hits += 1;

    Ok(hits)
}
