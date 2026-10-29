//! unx4137_pagechan — UNX-F4137 file 与页缓存读写通道锚（诊断段 0x9300）。
//!
//! 判据原文：UNX-F4137-J1 读路径经页缓存命中账（fake A4），写路径脏页标记账——两账
//!           与 B03/B04 页缓存锚交叉一致。
//!   J2 缺页填页数恒等于读未命中数。
//!   J3 脏页数不超过驻留页数，置脏幂等。
//!   J4 换出脏页被拒；B04 换出集与脏页不交。
//! 设计纪律：
//!   1. 读路径只有三种结局：命中（页在缓存且未失效）、未命中（缺页→填页）、越界（拒）。
//!      三结局之和恒等于读请求数，缺页填页数与未命中数严格相等，不允许「预读多填」
//!      把账做平。
//!   2. 写路径不填页：写只在已存在的帧上置脏位（写-未命中走「读-填-改」三步），
//!      故脏页数恒 ≤ 缓存页数；置脏是幂等的，重复置脏不重复计数。
//!   3. 与 B03/B04 的交叉一致以「页号集合」为锚：本域缓存页集合必须是 B03 页框表的
//!      子集，B04 的换出页不得同时被本域标脏（互斥性）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9300 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9300;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ABSENT: u16 = SEG_BASE + 1; // 读未命中（缺页，需填页）
const E_RANGE: u16 = SEG_BASE + 2; // 偏移/长度越界（EFAULT 面）
const E_NOPAGE: u16 = SEG_BASE + 3; // 写目标页不在缓存（写未命中）
const E_ACCT: u16 = SEG_BASE + 4; // 命中/未命中/填页三账不平
const E_DIRTY: u16 = SEG_BASE + 5; // 脏页账与帧脏位不一致
const E_CROSS: u16 = SEG_BASE + 6; // 与 B03/B04 锚交叉不一致
const E_FLUSH: u16 = SEG_BASE + 7; // 未置脏即回写（空回写）
const E_FRAME: u16 = SEG_BASE + 8; // 帧号越界

/// 页大小（4 KiB，与 B03 页框表一致）。
pub const PAGE_SIZE: u32 = 4096;
/// 缓存帧容量（定长，便于内存账手算）。
pub const FRAME_CAP: usize = 32;

/// 页状态机：Absent → Cached → Dirty → Writeback → Cached；Invalid 为终态（被换出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    Absent,
    Cached,
    Dirty,
    Writeback,
    Invalid,
}

impl PageState {
    pub fn resident(self) -> bool {
        match self {
            PageState::Cached | PageState::Dirty | PageState::Writeback => true,
            PageState::Absent | PageState::Invalid => false,
        }
    }

    pub fn dirty(self) -> bool {
        match self {
            PageState::Dirty | PageState::Writeback => true,
            _ => false,
        }
    }
}

/// 缓存帧：页号 + 状态 + 命中次数。
pub struct Frame {
    /// 页号（帧内映射的文件页号；0 表空帧）
    pub page: u32,
    /// 状态
    pub state: PageState,
    /// 本页命中次数
    pub hits: u32,
    /// 本页被填次数（换出后重填会再增）
    pub fills: u32,
}

/// 页缓存通道：帧表 + 读三结局账 + 脏页账。
pub struct PageChan {
    /// 帧表
    pub frames: Vec<Frame>,
    /// 读请求数
    pub reads: u32,
    /// 命中数
    pub hits: u32,
    /// 未命中数
    pub misses: u32,
    /// 填页数（必须 == misses）
    pub fills: u32,
    /// 写请求数
    pub writes: u32,
    /// 置脏次数（幂等，只在新脏时 +1）
    pub dirty_marks: u32,
    /// 回写完成次数
    pub flushed: u32,
    /// 越界拒收数
    pub range_denies: u32,
}

impl PageChan {
    pub fn new() -> PageChan {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < FRAME_CAP {
            v.push(Frame {
                page: 0,
                state: PageState::Absent,
                hits: 0,
                fills: 0,
            });
            i += 1;
        }
        PageChan {
            frames: v,
            reads: 0,
            hits: 0,
            misses: 0,
            fills: 0,
            writes: 0,
            dirty_marks: 0,
            flushed: 0,
            range_denies: 0,
        }
    }

    fn frame_of(&self, page: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.frames.len() {
            if self.frames[i].page == page && self.frames[i].state.resident() {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn empty_frame(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.frames.len() {
            if !self.frames[i].state.resident() {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 偏移合法性：off 与 len 之和不得越过文件上界（上界以页数表达）。
    fn check_range(off: u64, len: u64, file_pages: u32) -> Result<u32, u16> {
        if len == 0 {
            return Err(E_RANGE); // 零长请求拒收（不产生账）
        }
        let end = match off.checked_add(len) {
            Some(e) => e,
            None => return Err(E_RANGE), // 加法溢出即越界
        };
        let limit = (file_pages as u64) * (PAGE_SIZE as u64);
        if end > limit {
            return Err(E_RANGE);
        }
        Ok((off / (PAGE_SIZE as u64)) as u32)
    }

    /// 读：命中 / 未命中（填页）/ 越界三分。
    pub fn read(&mut self, off: u64, len: u64, file_pages: u32) -> Result<bool, u16> {
        let page = match Self::check_range(off, len, file_pages) {
            Ok(p) => p,
            Err(e) => {
                self.range_denies += 1;
                return Err(e);
            }
        };
        self.reads += 1;
        if let Some(i) = self.frame_of(page) {
            self.hits += 1;
            self.frames[i].hits += 1;
            return Ok(true); // 命中
        }
        // 未命中：填页
        self.misses += 1;
        let slot = match self.empty_frame() {
            Some(s) => s,
            None => return Err(E_FRAME), // 满缓存拒填（不做隐式驱逐，边界显性）
        };
        self.frames[slot] = Frame {
            page,
            state: PageState::Cached,
            hits: 0,
            fills: 1,
        };
        self.fills += 1;
        Ok(false)
    }

    /// 写：只对已在缓存且非无效的帧置脏；未命中即拒（写-未命中须先读-填）。
    pub fn write(&mut self, off: u64, len: u64, file_pages: u32) -> Result<(), u16> {
        let page = match Self::check_range(off, len, file_pages) {
            Ok(p) => p,
            Err(e) => {
                self.range_denies += 1;
                return Err(e);
            }
        };
        self.writes += 1;
        let i = match self.frame_of(page) {
            Some(x) => x,
            None => return Err(E_NOPAGE),
        };
        if !self.frames[i].state.dirty() {
            self.frames[i].state = PageState::Dirty;
            self.dirty_marks += 1; // 只在「净→脏」跃迁时计数（幂等）
        }
        Ok(())
    }

    /// 回写完成：Dirty → Cached。
    pub fn flush(&mut self, page: u32) -> Result<(), u16> {
        let i = match self.frame_of(page) {
            Some(x) => x,
            None => return Err(E_NOPAGE),
        };
        if !self.frames[i].state.dirty() {
            return Err(E_FLUSH); // 空回写拒收
        }
        self.frames[i].state = PageState::Cached;
        self.flushed += 1;
        Ok(())
    }

    /// 换出：任意驻留态 → Invalid（不计脏页，调用方须先 flush）。
    pub fn evict(&mut self, page: u32) -> Result<(), u16> {
        let i = match self.frame_of(page) {
            Some(x) => x,
            None => return Err(E_NOPAGE),
        };
        if self.frames[i].state.dirty() {
            return Err(E_DIRTY); // 脏页不得直接换出
        }
        self.frames[i].state = PageState::Invalid;
        Ok(())
    }

    /// 驻留页数。
    pub fn resident(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.frames.len() {
            if self.frames[i].state.resident() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 脏页数（按帧状态实点）。
    pub fn dirty_pages(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.frames.len() {
            if self.frames[i].state.dirty() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 命中率（千分比，整数）：hits * 1000 / reads；reads 为 0 时返回 0。
    pub fn hit_permille(&self) -> u32 {
        if self.reads == 0 {
            return 0;
        }
        (self.hits as u64 * 1000 / self.reads as u64) as u32
    }

    /// 读三账守恒：hits + misses == reads 且 fills == misses。
    pub fn read_balanced(&self) -> bool {
        self.hits + self.misses == self.reads && self.fills == self.misses
    }

    /// 与 B03 交叉：本域驻留页集合包含于 B03 页框表（以页号上界表达）。
    pub fn cross_b03(&self, b03_max_page: u32) -> bool {
        let mut i = 0usize;
        while i < self.frames.len() {
            if self.frames[i].state.resident() && self.frames[i].page > b03_max_page {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 与 B04 交叉：B04 已换出的页不得在本域仍标脏（互斥性）。
    pub fn cross_b04(&self, b04_evicted: &[u32]) -> bool {
        let mut i = 0usize;
        while i < self.frames.len() {
            if self.frames[i].state.dirty() {
                let mut k = 0usize;
                while k < b04_evicted.len() {
                    if b04_evicted[k] == self.frames[i].page {
                        return false;
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        true
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ABSENT => "读未命中（缺页需填页）",
        E_RANGE => "偏移/长度越界",
        E_NOPAGE => "目标页不在缓存",
        E_ACCT => "命中/未命中/填页三账不平",
        E_DIRTY => "脏页账与帧脏位不一致",
        E_CROSS => "与 B03/B04 锚交叉不一致",
        E_FLUSH => "未置脏即回写（空回写）",
        E_FRAME => "帧号越界或缓存满",
        _ => "未知诊断码",
    }
}

/// 确定性演练：顺序读填页 → 重读命中 → 写置脏 → 交叉锚 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：顺序读 10 页（每页一次）全部未命中 → 填页 10、命中 0。
    let mut pc = PageChan::new();
    let mut i = 0u64;
    while i < 10 {
        if pc.read(i * (PAGE_SIZE as u64), 64, 64)? {
            return Err(E_ACCT); // 首次读不可能命中
        }
        i += 1;
    }
    if pc.misses != 10 || pc.fills != 10 || pc.hits != 0 {
        return Err(E_ACCT);
    }
    if !pc.read_balanced() || pc.hit_permille() != 0 {
        return Err(E_ACCT);
    }
    if pc.resident() != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：重读同 10 页全部命中 → 命中率 500‰（20 读 10 中）。
    i = 0;
    while i < 10 {
        if !pc.read(i * (PAGE_SIZE as u64), 64, 64)? {
            return Err(E_ACCT);
        }
        i += 1;
    }
    if pc.hits != 10 || pc.reads != 20 || pc.fills != 10 {
        return Err(E_ACCT);
    }
    if pc.hit_permille() != 500 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：写已缓存页 10 次（同一页重复写）→ 置脏一次（幂等），脏页 1。
    let mut k = 0;
    while k < 10 {
        pc.write(0, 64, 64)?;
        k += 1;
    }
    if pc.writes != 10 || pc.dirty_marks != 1 {
        return Err(E_DIRTY);
    }
    if pc.dirty_pages() != 1 {
        return Err(E_DIRTY);
    }
    hits += 1;

    // R4：回写 → 脏页归零；空回写拒收。
    pc.flush(0)?;
    if pc.dirty_pages() != 0 || pc.flushed != 1 {
        return Err(E_DIRTY);
    }
    if pc.flush(0) != Err(E_FLUSH) {
        return Err(E_FLUSH);
    }
    hits += 1;

    // R5：写多页置脏，脏页数与置脏账一致（每页一次跃迁）。
    i = 1;
    while i < 4 {
        pc.write(i * (PAGE_SIZE as u64), 32, 64)?;
        i += 1;
    }
    if pc.dirty_pages() != 3 || pc.dirty_marks != 4 {
        return Err(E_DIRTY);
    }
    hits += 1;

    // R6：换出净页成功、换出脏页拒收；换出后重读再填页（fills 增加）。
    pc.evict(9)?; // 页 9 为净页
    if pc.dirty_pages() != 3 {
        return Err(E_DIRTY);
    }
    if pc.evict(1) != Err(E_DIRTY) {
        return Err(E_DIRTY);
    }
    let refill = pc.read(9 * (PAGE_SIZE as u64), 16, 64)?;
    if refill || pc.fills != 11 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：越界读（末页之后）→ E_RANGE；零长 → E_RANGE（不产账）。
    let req_before = pc.reads;
    if pc.read(64 * (PAGE_SIZE as u64), 16, 64) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    if pc.read(0, 0, 64) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    if pc.reads != req_before {
        return Err(E_ACCT); // 拒收不记读账
    }
    if pc.range_denies != 2 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I2：写未命中（未缓存的页 20）→ E_NOPAGE（不走隐式填页）。
    if pc.write(20 * (PAGE_SIZE as u64), 8, 64) != Err(E_NOPAGE) {
        return Err(E_NOPAGE);
    }
    if pc.resident() != 10 {
        return Err(E_ACCT); // 写不得增加驻留页
    }
    hits += 1;

    // I3：加法溢出 → E_RANGE（先算上界，绝不先做页号除法）。
    if pc.read(u64::MAX - 4, 16, 64) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // C1：交叉锚——驻留页都在 B03 页号上界内；B04 换出集合与脏页不交。
    if !pc.cross_b03(63) {
        return Err(E_CROSS);
    }
    if pc.cross_b03(5) {
        return Err(E_CROSS); // 反证：上界收紧到 5 后必须报不一致（页 9 越界）
    }
    let evicted = [1u32, 2];
    if pc.cross_b04(&evicted) {
        return Err(E_CROSS); // 页 1/2 已脏且出现在换出集 → 必须报不一致
    }
    let clean_evicted = [9u32, 10];
    if !pc.cross_b04(&clean_evicted) {
        return Err(E_CROSS);
    }
    hits += 1;

    // C2：三账终审——reads 21（20 + 重填 1）、hits 10、misses 11、fills 11。
    if !pc.read_balanced() {
        return Err(E_ACCT);
    }
    if pc.reads != 21 || pc.hits != 10 || pc.misses != 11 || pc.fills != 11 {
        return Err(E_ACCT);
    }
    if pc.hit_permille() != 476 {
        return Err(E_ACCT); // 10*1000/21 = 476（整数截断）
    }
    if pc.dirty_marks != 4 || pc.flushed != 1 {
        return Err(E_DIRTY);
    }
    hits += 1;

    Ok(hits)
}
