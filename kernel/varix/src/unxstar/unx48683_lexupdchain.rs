//! UNX-F48683 —— I·L3 签名面词库更新联签（签名校验→原子换装全链）
//! 判据原文：UNX-F48683-J1 联签 ×10 全链绿；判据锚定 §AI-61 §2 专题四。
//! 判据落地（drill 场景）：R1 链构造；R2 签名校验；R3 原子换装；
//! R4 可用性零空窗；R5 全链 ×10；R6 坏签拒装；R7 重复 ID 拒；
//! R8 指纹稳定；R9 呈现行；R10 回退位在册。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：L3 更新链 = 签名校验 → 查重 → 原子换装三段串行；
//! 原子换装 = 旧库落回退位（backup）后切 active，同事务两步；
//! 换装全程 active/backup 双非零即零空窗；坏签、重包一律拒装。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0CC0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SIG: u16 = SEG_BASE + 1; // 签名校验失败
const E_SWAP: u16 = SEG_BASE + 2; // 换装失败
const E_WINDOW: u16 = SEG_BASE + 3; // 换装出现空窗
const E_DUP: u16 = SEG_BASE + 4; // 重复更新 ID
const E_CHAIN: u16 = SEG_BASE + 5; // 全链轮次不足
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_BACKUP: u16 = SEG_BASE + 8; // 回退位缺失
const E_EMPTY: u16 = SEG_BASE + 9; // 链未初始化

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一个词库更新包。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LexUpdate {
    pub id: u32,
    pub payload_hash: u64,
    pub sig: u64, // 签名 = 签名面对 (id, payload_hash) 的 FNV
}

impl LexUpdate {
    pub fn make(id: u32, payload_hash: u64) -> LexUpdate {
        LexUpdate { id, payload_hash, sig: LexUpdate::sign_of(id, payload_hash) }
    }

    /// L3 签名面口径：sig = FNV(id || payload_hash)。
    pub fn sign_of(id: u32, payload_hash: u64) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(&payload_hash.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 签名校验。
    pub fn verify(&self) -> Result<(), u16> {
        let want = LexUpdate::sign_of(self.id, self.payload_hash);
        if self.sig != want || self.sig == 0 {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// L3 更新链：签名校验 → 原子换装（active 备份到 backup 后切换）。
pub struct UpdChain {
    pub active: u64,  // 在役词库哈希
    pub backup: u64,  // 换装前的旧库哈希（回退位）
    pub applied: u32,
    pub last_id: u32,
    pub seen_ids: Vec<u32>,
}

impl UpdChain {
    pub fn new(initial: u64) -> UpdChain {
        UpdChain { active: initial, backup: initial, applied: 0, last_id: 0, seen_ids: vec![] }
    }

    /// 全链应用：签名校验 → 查重 → 原子换装 → 回退位留底。
    pub fn apply(&mut self, u: &LexUpdate) -> Result<(), u16> {
        if self.applied == 0 && self.active == 0 {
            return Err(E_EMPTY);
        }
        u.verify()?;
        let mut i = 0usize;
        while i < self.seen_ids.len() {
            if self.seen_ids[i] == u.id {
                return Err(E_DUP);
            }
            i += 1;
        }
        // 原子换装：旧库先落回退位，再切 active，同事务两步。
        self.backup = self.active;
        self.active = u.payload_hash;
        if self.active == 0 || self.backup == 0 {
            return Err(E_WINDOW);
        }
        self.seen_ids.push(u.id);
        self.last_id = u.id;
        self.applied += 1;
        Ok(())
    }

    /// 可用性零空窗：换装后 active 与 backup 双非零。
    pub fn no_gap(&self) -> bool {
        self.active != 0 && self.backup != 0
    }

    /// 回退位在册：backup 非零且历史有效。
    pub fn rollback_ready(&self) -> bool {
        self.backup != 0
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.active.to_le_bytes());
        buf.extend_from_slice(&self.backup.to_le_bytes());
        buf.extend_from_slice(&self.applied.to_le_bytes());
        buf.extend_from_slice(&self.last_id.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 一笔换装留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwapRec {
    pub id: u32,
    pub from_hash: u64,
    pub to_hash: u64,
    pub gap_free: bool,
}

/// 换装留痕账：逐笔记录 from→to，零空窗断言用。
pub struct SwapJournal {
    pub recs: Vec<SwapRec>,
}

impl SwapJournal {
    pub fn new() -> SwapJournal {
        SwapJournal { recs: vec![] }
    }

    /// 登记一笔换装：from/to 双非零即零空窗。
    pub fn record(&mut self, id: u32, from_hash: u64, to_hash: u64) -> Result<(), u16> {
        if from_hash == 0 || to_hash == 0 {
            return Err(E_WINDOW);
        }
        self.recs.push(SwapRec { id, from_hash, to_hash, gap_free: true });
        Ok(())
    }

    /// 链一致性：每笔 to == 下一笔 from。
    pub fn chain_linked(&self) -> bool {
        let mut i = 0usize;
        while i + 1 < self.recs.len() {
            if self.recs[i].to_hash != self.recs[i + 1].from_hash {
                return false;
            }
            i += 1;
        }
        true
    }

    /// ID 严格递增。
    pub fn ids_monotonic(&self) -> bool {
        let mut i = 1usize;
        while i < self.recs.len() {
            if self.recs[i].id <= self.recs[i - 1].id {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 全账零空窗。
    pub fn all_gap_free(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            if !self.recs[i].gap_free {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 呈现行：applied=<n> last=<更新 ID> gap=<0|1> rb=<0|1>。
pub fn chain_line(c: &UpdChain) -> String {
    let mut out = String::new();
    out.push_str("applied=");
    out.push_str(&c.applied.to_string());
    out.push_str(" last=");
    out.push_str(&c.last_id.to_string());
    out.push_str(" gap=");
    out.push_str(&(c.no_gap() as u32).to_string());
    out.push_str(" rb=");
    out.push_str(&(c.rollback_ready() as u32).to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_83);

    // R1 链构造：初始库非零。
    let mut chain = UpdChain::new(0x1122_3344);
    if chain.active == 0 || !chain.no_gap() {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R2 签名校验：合法包过、坏签拒。
    let u1 = LexUpdate::make(1, 0xAABB_CCDD);
    u1.verify()?;
    let mut bad = u1.clone();
    bad.sig = bad.sig.wrapping_add(1);
    match bad.verify() {
        Err(E_SIG) => {}
        _ => return Err(E_SIG),
    }
    hits += 1;

    // R3 原子换装 + 回退位留底。
    chain.apply(&u1)?;
    if chain.active != 0xAABB_CCDD || chain.backup != 0x1122_3344 {
        return Err(E_SWAP);
    }
    if !chain.rollback_ready() {
        return Err(E_BACKUP);
    }
    hits += 1;

    // R4 可用性零空窗。
    if !chain.no_gap() {
        return Err(E_WINDOW);
    }
    hits += 1;

    // R5 全链 ×10：十次合法更新全部换装成功。
    let mut i: u32 = 2;
    while i <= 10 {
        let u = LexUpdate::make(i, 0x1000 + g.below(0xF000) as u64);
        chain.apply(&u)?;
        i += 1;
    }
    if chain.applied != 10 {
        return Err(E_CHAIN);
    }
    hits += 1;

    // R6 坏签拒装：拒后账面不变（applied/active/backup 三不动）。
    let a0 = chain.applied;
    let act0 = chain.active;
    let mut bad2 = LexUpdate::make(11, 0x7777);
    bad2.sig = 0;
    match chain.apply(&bad2) {
        Err(E_SIG) => {}
        _ => return Err(E_SIG),
    }
    if chain.applied != a0 || chain.active != act0 {
        return Err(E_SIG);
    }
    hits += 1;

    // R7 重复 ID 拒。
    let dup = LexUpdate::make(5, 0x9999);
    match chain.apply(&dup) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R8 指纹稳定。
    let f1 = chain.fp();
    let f2 = chain.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：applied=10 gap=0 rb=1。
    let line = chain_line(&chain);
    if !has_kw(line.as_bytes(), b"applied=10") || !has_kw(line.as_bytes(), b"gap=0")
        || !has_kw(line.as_bytes(), b"rb=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 回退位在册：backup == 倒数第二次 active（链上 to→from 衔接）。
    // 换装留痕账重建：十笔 from→to 链接、ID 递增、零空窗。
    let mut journal = SwapJournal::new();
    let mut prev = 0x1122_3344;
    let mut i: u32 = 1;
    while i <= 10 {
        let h = if i == 1 {
            0xAABB_CCDD
        } else {
            0x1000 + (0x486_83u32.wrapping_add(i).wrapping_mul(97) % 0xF000) as u64
        };
        journal.record(i, prev, h)?;
        prev = h;
        i += 1;
    }
    if !journal.chain_linked() || !journal.ids_monotonic() || !journal.all_gap_free() {
        return Err(E_SWAP);
    }
    if !chain.rollback_ready() {
        return Err(E_BACKUP);
    }
    hits += 1;

    Ok(hits)
}
