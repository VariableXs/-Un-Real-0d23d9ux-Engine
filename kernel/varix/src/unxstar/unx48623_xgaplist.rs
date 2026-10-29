//! UNX-F48623 —— I·X11 老应用缺失项清单年审（L2 如实清单复核）
//! 判据原文：UNX-F48623-J1 复核零新增编造；判据锚定 §40 幻觉红线。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           缺失项清单冻结 + 年审复核：只许确认/待实测，禁新增编造。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0500; // X11 缺失项清单年审诊断段

const E_FABRICATE: u16 = SEG_BASE + 1; // 新增编造
const E_ENTRY_BAD: u16 = SEG_BASE + 2; // 条目非法
const E_STATE_BAD: u16 = SEG_BASE + 3; // 复核态违例
const E_DUP: u16 = SEG_BASE + 4;       // 条目重复
const E_LEDGER: u16 = SEG_BASE + 5;    // 清单账不守恒
const E_REPLAY: u16 = SEG_BASE + 6;    // 重演不一致
const E_DEL_BAD: u16 = SEG_BASE + 7;   // 越权删项

/// 缺失项复核态。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReviewState {
    Confirmed = 0,  // 复核确认仍缺失
    Pending = 1,    // 待实测（如实标注）
    Resolved = 2,   // 已补齐（有证据）
}

/// 缺失项条目。
pub struct GapItem {
    pub id: u32,
    pub name: String,
    pub state: ReviewState,
    /// 年审冻结：条目建立后不可删除、不可改名。
    pub frozen: bool,
}

impl GapItem {
    pub fn new(id: u32, name: &str, state: ReviewState) -> Self {
        GapItem { id, name: name.to_string(), state, frozen: true }
    }
    /// 条目审计：名非空、态合法。
    pub fn audit(&self) -> Result<(), u16> {
        if self.name.len() == 0 {
            return Err(E_ENTRY_BAD);
        }
        if self.state as u8 > 2 {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }
}

/// L2 缺失项清单。
pub struct GapList {
    pub items: Vec<GapItem>,
    pub label: String,
    pub sealed: bool,
}

impl GapList {
    pub fn new(label: &str) -> Self {
        GapList { items: Vec::new(), label: label.to_string(), sealed: false }
    }

    /// 建立条目（仅年审期开始前可建）。
    pub fn add(&mut self, id: u32, name: &str, state: ReviewState) -> Result<(), u16> {
        if self.sealed {
            return Err(E_FABRICATE);
        }
        if name.len() == 0 {
            return Err(E_ENTRY_BAD);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id == id || self.items[i].name == name {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(GapItem::new(id, name, state));
        Ok(())
    }

    /// 封单：年审开始，禁止增删。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 年审复核：只允许在三态间按证据迁移，禁止新增条目。
    pub fn review(&mut self, id: u32, to: ReviewState) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id == id {
                if !self.items[i].frozen {
                    return Err(E_STATE_BAD);
                }
                self.items[i].state = to;
                return Ok(());
            }
            i += 1;
        }
        Err(E_ENTRY_BAD)
    }

    /// 幻觉红线机检：复核后条目集合与封单时完全一致（零新增零删除）。
    pub fn anti_fabricate(&self, baseline: &Vec<(u32, String)>) -> Result<(), u16> {
        if self.items.len() != baseline.len() {
            return Err(E_FABRICATE);
        }
        let mut i = 0usize;
        while i < baseline.len() {
            let mut found = false;
            let mut j = 0usize;
            while j < self.items.len() {
                if self.items[j].id == baseline[i].0 && self.items[j].name == baseline[i].1 {
                    found = true;
                }
                j += 1;
            }
            if !found {
                return Err(E_FABRICATE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 封单基线快照。
    pub fn snapshot(&self) -> Vec<(u32, String)> {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            v.push((self.items[i].id, self.items[i].name.clone()));
            i += 1;
        }
        v
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.items.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.items[i].id as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (self.items[i].state as u64);
            i += 1;
        }
        h ^= self.items.len() as u64;
        h
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：X11 缺失项清单年审十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 清单建立 ×6：六项老应用缺失如实登记。
    let mut gl = GapList::new("l2-gaps");
    gl.add(1, "xim-fontset", ReviewState::Confirmed)?;
    gl.add(2, "root-locate", ReviewState::Confirmed)?;
    gl.add(3, "xim-hotkey", ReviewState::Pending)?;
    gl.add(4, "overthespot", ReviewState::Confirmed)?;
    gl.add(5, "xim-clamp", ReviewState::Pending)?;
    gl.add(6, "ctext-conv", ReviewState::Resolved)?;
    gl.seal();
    let base = gl.snapshot();
    if base.len() != 6 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 复核迁移：Pending → Confirmed 有据迁移。
    gl.review(3, ReviewState::Confirmed)?;
    gl.review(5, ReviewState::Pending)?;
    let it = &gl.items[2];
    if it.state != ReviewState::Confirmed {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R3 幻觉红线：复核后条目集与封单基线一致。
    gl.anti_fabricate(&base)?;
    hits += 1;

    // R4 封单后新增反例：seal 后 add 须拒（编造红线）。
    let mut gl2 = GapList::new("fabricate");
    gl2.add(1, "a", ReviewState::Confirmed)?;
    gl2.seal();
    let r = gl2.add(2, "b", ReviewState::Confirmed);
    if r.err() != Some(E_FABRICATE) {
        return Err(E_FABRICATE);
    }
    hits += 1;

    // R5 重复条目反例：同 ID / 同名登记须拒。
    let mut gl3 = GapList::new("dup");
    gl3.add(1, "x", ReviewState::Confirmed)?;
    let r = gl3.add(1, "y", ReviewState::Confirmed);
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    let r = gl3.add(2, "x", ReviewState::Confirmed);
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R6 空名反例。
    let mut gl4 = GapList::new("noname");
    let r = gl4.add(1, "", ReviewState::Confirmed);
    if r.err() != Some(E_ENTRY_BAD) {
        return Err(E_ENTRY_BAD);
    }
    hits += 1;

    // R7 复核未知条目反例：未知 ID 须拒。
    let mut gl5 = GapList::new("miss");
    gl5.add(1, "a", ReviewState::Confirmed)?;
    let r = gl5.review(42, ReviewState::Resolved);
    if r.err() != Some(E_ENTRY_BAD) {
        return Err(E_ENTRY_BAD);
    }
    hits += 1;

    // R8 编造检出反例：基线外条目使 anti_fabricate 红。
    let mut gl6 = GapList::new("inject");
    gl6.add(1, "a", ReviewState::Confirmed)?;
    gl6.seal();
    let base2 = gl6.snapshot();
    // 模拟越界注入：直接 push 一条编造条目。
    let mut gl7 = GapList::new("inject2");
    let mut i = 0usize;
    while i < gl6.items.len() {
        let it = &gl6.items[i];
        let _ = gl7.add(it.id, &it.name, it.state);
        i += 1;
    }
    let _ = gl7.add(99, "ghost", ReviewState::Confirmed);
    let r = gl7.anti_fabricate(&base2);
    if r.err() != Some(E_FABRICATE) {
        return Err(E_FABRICATE);
    }
    hits += 1;

    // R9 fuzz ×128 随机复核：条目集守恒 + 态域封闭。
    let mut rng = Lcg::new(0x48623_0001);
    let mut gl8 = GapList::new("fuzz");
    let mut id = 1u32;
    while id <= 12 {
        let st = match rng.below(3) {
            0 => ReviewState::Confirmed,
            1 => ReviewState::Pending,
            _ => ReviewState::Resolved,
        };
        gl8.add(id, "gap", st)?;
        id += 1;
    }
    gl8.seal();
    let base3 = gl8.snapshot();
    let mut k = 0u32;
    while k < 128 {
        let id = 1 + rng.below(12) as u32;
        let st = match rng.below(3) {
            0 => ReviewState::Confirmed,
            1 => ReviewState::Pending,
            _ => ReviewState::Resolved,
        };
        gl8.review(id, st)?;
        k += 1;
    }
    gl8.anti_fabricate(&base3)?;
    // 条目 ID 集恒等。
    if gl8.items.len() != 12 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R10 重演一致：同种子重建 fuzz 复核序列，指纹相等。
    let fp1 = gl8.fingerprint();
    let mut rng2 = Lcg::new(0x48623_0001);
    let mut gl9 = GapList::new("fuzz");
    let mut id = 1u32;
    while id <= 12 {
        let st = match rng2.below(3) {
            0 => ReviewState::Confirmed,
            1 => ReviewState::Pending,
            _ => ReviewState::Resolved,
        };
        let _ = gl9.add(id, "gap", st);
        id += 1;
    }
    let mut k = 0u32;
    while k < 128 {
        let id = 1 + rng2.below(12) as u32;
        let st = match rng2.below(3) {
            0 => ReviewState::Confirmed,
            1 => ReviewState::Pending,
            _ => ReviewState::Resolved,
        };
        let _ = gl9.review(id, st);
        k += 1;
    }
    if gl9.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
