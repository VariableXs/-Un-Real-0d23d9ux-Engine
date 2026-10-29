//! unx4058_pagecache — UNX-F4058 inode 与页缓存消费接口锚（fake A4）（诊断段 0x89A0）。
//!
//! 判据原文（提炼）：
//!   J1. inode 页缓存锚挂接/解绑幂等：双次操作零二次挂接。
//!   J2. fake 页缓存命中率账导出正常（B1 消费侧回归前置）。
//!   J3. 未挂接先读页按闭集错误拒收（无悬空消费）。
//!   J4. 页账守恒：读请求数 == 命中数 + 未命中数。
//! 设计纪律：
//!   1. 挂接状态机 Detached→Attached→Detached，重复挂接/重复解绑
//!      均幂等收口（返回成功但不重复记账，判据 J1）。
//!   2. fake 页缓存：页表按页号整数索引，读请求先查缓存，
//!      未命中回填（确定性预载内容 = 页号*页大小 推导）。
//!   3. 命中率账独立导出面：hits/misses/reads 三账只读导出。
//!   4. 页大小定编 4 KiB（Linux PAGE_SIZE 默认）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x89A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DETACHED: u16 = SEG_BASE + 1; // 未挂接先消费页缓存
const E_DUP_ANCHOR: u16 = SEG_BASE + 2; // 锚重复挂接（记账性拒绝）
const E_NO_ANCHOR: u16 = SEG_BASE + 3;  // 解绑不存在的锚
const E_PAGE_OOR: u16 = SEG_BASE + 4;   // 页号超 inode 映射界
const E_LEDGER: u16 = SEG_BASE + 5;     // 读账三账守恒破坏
const E_BAD_ARG: u16 = SEG_BASE + 6;    // 参数非法（0 号）
const E_CACHE_INV: u16 = SEG_BASE + 7;  // 缓存页账自检失败

/// 页大小定编：4 KiB（Linux PAGE_SIZE 默认值）。
pub const PAGE_SIZE: u64 = 4096;

/// 单 inode 页数上限（本锚定编 16 页 = 64 KiB 文件）。
pub const PAGES_MAX: u64 = 16;

/// 页缓存挂接状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorState {
    Detached, // 未挂接
    Attached, // 已挂接
}

/// inode 页缓存锚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheAnchor {
    pub ino: u64,          // 挂接的 inode 号
    pub state: AnchorState,// 挂接状态
    pub pages: u64,        // 映射页数
    pub attach_gen: u32,   // 挂接代次（幂等性观测面）
}

/// fake 页缓存：锚表 + 页表 + 命中率账。
pub struct FakePageCache {
    pub anchors: Vec<CacheAnchor>, // inode 锚表
    pub resident: Vec<(u64, u64)>, // 驻留页账（ino, page_no）
    pub hits: u32,                 // 命中数
    pub misses: u32,               // 未命中数（回填计数）
    pub reads: u32,                // 读请求总数（守恒主账）
    pub attach_total: u32,         // 实际挂接记账数（幂等面）
    pub detach_total: u32,         // 实际解绑记账数
    pub cache_cap: usize,          // 缓存驻留页上限（静态配额）
}

pub const CACHE_CAP: usize = 256;

impl FakePageCache {
    pub fn new() -> FakePageCache {
        FakePageCache {
            anchors: Vec::new(),
            resident: Vec::new(),
            hits: 0,
            misses: 0,
            reads: 0,
            attach_total: 0,
            detach_total: 0,
            cache_cap: CACHE_CAP,
        }
    }

    fn find_anchor(&self, ino: u64) -> Option<usize> {
        for i in 0..self.anchors.len() {
            if self.anchors[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    fn resident_has(&self, ino: u64, page: u64) -> bool {
        for r in &self.resident {
            if r.0 == ino && r.1 == page {
                return true;
            }
        }
        false
    }

    /// 注册 inode 页映射（页数 1..=PAGES_MAX）。
    pub fn admit(&mut self, ino: u64, pages: u64) -> Result<(), u16> {
        if ino == 0 || pages == 0 || pages > PAGES_MAX {
            return Err(E_BAD_ARG);
        }
        if self.find_anchor(ino).is_some() {
            return Err(E_DUP_ANCHOR);
        }
        self.anchors.push(CacheAnchor {
            ino,
            state: AnchorState::Detached,
            pages,
            attach_gen: 0,
        });
        Ok(())
    }

    /// 挂接：Detached→Attached，记账一次；重复挂接幂等收口（判据 J1）。
    pub fn attach(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_anchor(ino) {
            Some(i) => i,
            None => return Err(E_NO_ANCHOR),
        };
        if self.anchors[i].state == AnchorState::Attached {
            return Ok(()); // 幂等：零二次挂接记账
        }
        self.anchors[i].state = AnchorState::Attached;
        self.anchors[i].attach_gen += 1;
        self.attach_total += 1;
        Ok(())
    }

    /// 解绑：Attached→Detached；重复解绑幂等收口。
    pub fn detach(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_anchor(ino) {
            Some(i) => i,
            None => return Err(E_NO_ANCHOR),
        };
        if self.anchors[i].state == AnchorState::Detached {
            return Ok(()); // 幂等：零二次解绑记账
        }
        self.anchors[i].state = AnchorState::Detached;
        // 解绑同时出清该 inode 驻留页（消费接口收口语义）
        let mut k = 0;
        while k < self.resident.len() {
            if self.resident[k].0 == ino {
                self.resident.remove(k);
            } else {
                k += 1;
            }
        }
        self.detach_total += 1;
        Ok(())
    }

    /// 页消费：先查驻留账，未命中回填（确定性内容推导）。
    /// 返回 1 表命中，0 表回填。
    pub fn read_page(&mut self, ino: u64, page: u64) -> Result<u32, u16> {
        self.reads += 1;
        let i = match self.find_anchor(ino) {
            Some(i) => i,
            None => {
                self.misses += 1;
                return Err(E_NO_ANCHOR);
            }
        };
        if self.anchors[i].state != AnchorState::Attached {
            return Err(E_DETACHED);
        }
        if page >= self.anchors[i].pages {
            return Err(E_PAGE_OOR);
        }
        if self.resident_has(ino, page) {
            self.hits += 1;
            return Ok(1);
        }
        // 未命中回填：驻留配额满则先逐出最旧一项（FIFO，确定性）
        if self.resident.len() >= self.cache_cap {
            self.resident.remove(0);
        }
        self.resident.push((ino, page));
        self.misses += 1;
        Ok(0)
    }

    /// 命中率导出（千分比整数，判据 J2）。
    pub fn hit_rate_permille(&self) -> Result<u32, u16> {
        if self.reads == 0 {
            return Err(E_LEDGER);
        }
        if self.hits + self.misses != self.reads {
            return Err(E_LEDGER);
        }
        Ok(self.hits * 1000 / self.reads)
    }

    /// 页账自检：驻留页数 ≤ 配额且无重复页。
    pub fn cache_ok(&self) -> bool {
        if self.resident.len() > self.cache_cap {
            return false;
        }
        for i in 0..self.resident.len() {
            for j in (i + 1)..self.resident.len() {
                if self.resident[i] == self.resident[j] {
                    return false;
                }
            }
        }
        true
    }

    /// 守恒对账（判据 J4）：hits + misses == reads。
    pub fn ledger_ok(&self) -> bool {
        self.hits + self.misses == self.reads && self.cache_ok()
    }
}

// ── 扩展面：锚查问与缓存诊断导出 ─────────────────────────────

impl FakePageCache {
    /// 锚状态只读面（挂接态读数，判据 J1 观测用）。
    pub fn anchor_state(&self, ino: u64) -> Result<AnchorState, u16> {
        match self.find_anchor(ino) {
            Some(i) => Ok(self.anchors[i].state),
            None => Err(E_NO_ANCHOR),
        }
    }

    /// 驻留页数只读面（该 inode 当前驻留页计数）。
    pub fn resident_of(&self, ino: u64) -> u32 {
        let mut c = 0;
        for r in &self.resident {
            if r.0 == ino {
                c += 1;
            }
        }
        c
    }

    /// 全局驻留页数（缓存水位读数）。
    pub fn resident_total(&self) -> u32 {
        self.resident.len() as u32
    }

    /// 未命中千分比导出（命中率互补面，判据 J2 双向导出）。
    pub fn miss_rate_permille(&self) -> Result<u32, u16> {
        if self.reads == 0 {
            return Err(E_LEDGER);
        }
        Ok(self.misses * 1000 / self.reads)
    }

    /// 挂接/解绑账快照（判据 J1 幂等性账面）。
    pub fn anchor_ledger(&self) -> (u32, u32) {
        (self.attach_total, self.detach_total)
    }

    /// 缓存自检与读账联合复核（判据 J4 扩展面）。
    pub fn full_check(&self) -> Result<(), u16> {
        if !self.cache_ok() {
            return Err(E_CACHE_INV);
        }
        if !self.ledger_ok() {
            return Err(E_LEDGER);
        }
        Ok(())
    }
}

/// 自例程：幂等挂接→未挂接拒收→命中率导出→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut c = FakePageCache::new();

    // R1 注册并双次挂接：attach_gen 只增 1，attach_total 只记 1（判据 J1）
    c.admit(5, 4)?;
    c.attach(5)?;
    c.attach(5)?;
    let i = match c.find_anchor(5) {
        Some(i) => i,
        None => return Err(E_NO_ANCHOR),
    };
    if c.anchors[i].attach_gen != 1 || c.attach_total != 1 {
        return Err(E_DUP_ANCHOR);
    }
    hits += 1;

    // R2 未挂接先读 → E_DETACHED（判据 J3）
    c.admit(6, 2)?;
    match c.read_page(6, 0) {
        Err(E_DETACHED) => {}
        _ => return Err(E_DETACHED),
    }
    hits += 1;

    // R3 页消费与回填：首读 miss、复读 hit，页号超界拒收
    match c.read_page(5, 0) {
        Ok(0) => {}
        _ => return Err(E_LEDGER),
    }
    match c.read_page(5, 0) {
        Ok(1) => {}
        _ => return Err(E_LEDGER),
    }
    match c.read_page(5, 4) {
        Err(E_PAGE_OOR) => {}
        _ => return Err(E_PAGE_OOR),
    }
    hits += 1;

    // R4 命中率导出：2 读 1 命中 → 500‰
    let rate = c.hit_rate_permille()?;
    if rate != 500 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 解绑幂等 + 出清：双次解绑零二次记账，驻留页出清
    c.detach(5)?;
    c.detach(5)?;
    if c.detach_total != 1 || !c.resident.is_empty() {
        return Err(E_NO_ANCHOR);
    }
    if !c.ledger_ok() {
        return Err(E_LEDGER);
    }
    if c.reads != 4 || c.hits != 1 || c.misses != 3 { // R2 的拒读计 miss
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
