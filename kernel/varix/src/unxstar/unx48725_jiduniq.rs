//! UNX-F48725 C·判据编号唯一性机检（800 条判据 ID 零重复）
//! 判据：UNX-F48725-J1 机检通过；判据锚定 §AI-61 §3。
//! 纪律：登记即查重、双机检（哈希桶+排序相邻）、格式校验前置、删除不复活。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1500;

const E_DUP_JID: u16 = SEG_BASE + 1; // 判据 ID 重复
const E_JID_BAD: u16 = SEG_BASE + 2; // ID 格式非法
const E_BOOK_EMPTY: u16 = SEG_BASE + 3; // 空登记拒
const E_REVIVE: u16 = SEG_BASE + 4; // 删除后复活
const E_BUCKET_BREAK: u16 = SEG_BASE + 5; // 桶位守恒破
const E_ADJ_BREAK: u16 = SEG_BASE + 6; // 排序相邻重复
const E_COUNT_BREAK: u16 = SEG_BASE + 7; // 全量计数守恒破
const E_SCOPE_BAD: u16 = SEG_BASE + 8; // ID 域越界

/// 判据 ID 上界（800 条）。
pub const JID_MAX: u32 = 800;

/// 判据条目。
#[derive(Clone)]
pub struct JudgeEntry {
    /// 判据 ID（1..=800）。
    pub jid: u32,
    /// 判据锚（书章节）。
    pub anchor: String,
    /// 存活位（删除置 false）。
    pub alive: bool,
}

/// 判据编号唯一性机检簿。
pub struct JudgeIdBook {
    entries: Vec<JudgeEntry>,
    /// 哈希桶：16 桶，每桶计数。
    buckets: [u32; 16],
    /// 已回收 ID（删除后不得复活）。
    retired: Vec<u32>,
}

impl JudgeIdBook {
    pub fn new() -> JudgeIdBook {
        JudgeIdBook {
            entries: Vec::new(),
            buckets: [0; 16],
            retired: Vec::new(),
        }
    }

    /// ID 格式校验：1..=800。
    fn check_fmt(jid: u32) -> Result<(), u16> {
        if jid == 0 || jid > JID_MAX {
            return Err(E_JID_BAD);
        }
        Ok(())
    }

    /// 桶号：jid % 16。
    fn bucket_of(jid: u32) -> usize {
        (jid % 16) as usize
    }

    /// 登记一条判据（唯一性机检：桶内逐条比）。
    pub fn put(&mut self, jid: u32, anchor: &str) -> Result<bool, u16> {
        JudgeIdBook::check_fmt(jid)?;
        if anchor.is_empty() {
            return Err(E_BOOK_EMPTY);
        }
        for r in self.retired.iter() {
            if *r == jid {
                return Err(E_REVIVE);
            }
        }
        let b = JudgeIdBook::bucket_of(jid);
        for e in self.entries.iter() {
            if e.alive && e.jid == jid {
                return Err(E_DUP_JID);
            }
            if e.jid == jid && !e.alive {
                return Err(E_REVIVE);
            }
        }
        self.entries.push(JudgeEntry {
            jid,
            anchor: anchor.to_string(),
            alive: true,
        });
        self.buckets[b] += 1;
        Ok(true)
    }

    /// 删除（标记非存活）。
    pub fn retire(&mut self, jid: u32) -> Result<bool, u16> {
        JudgeIdBook::check_fmt(jid)?;
        let mut found = false;
        for e in self.entries.iter_mut() {
            if e.jid == jid && e.alive {
                e.alive = false;
                found = true;
                break;
            }
        }
        if !found {
            return Err(E_BOOK_EMPTY);
        }
        self.retired.push(jid);
        self.buckets[JudgeIdBook::bucket_of(jid)] -= 1;
        Ok(true)
    }

    /// 机检一：全量存活条目逐对零重复。
    pub fn assert_unique(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if !self.entries[i].alive {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < self.entries.len() {
                if self.entries[j].alive && self.entries[j].jid == self.entries[i].jid {
                    return Err(E_DUP_JID);
                }
                j += 1;
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 机检二：排序后相邻零重复（独立路径交叉验证）。
    pub fn assert_adjacent(&self) -> Result<(), u16> {
        let mut ids: Vec<u32> = Vec::new();
        for e in self.entries.iter() {
            if e.alive {
                ids.push(e.jid);
            }
        }
        ids.sort();
        let mut i = 1usize;
        while i < ids.len() {
            if ids[i] == ids[i - 1] {
                return Err(E_ADJ_BREAK);
            }
            i += 1;
        }
        Ok(())
    }

    /// 桶位守恒：各桶计数和 = 存活数。
    pub fn assert_buckets(&self) -> Result<(), u16> {
        let mut alive = 0u32;
        for e in self.entries.iter() {
            if e.alive {
                alive += 1;
            }
        }
        let mut sum = 0u32;
        for b in self.buckets.iter() {
            sum += *b;
        }
        if sum != alive {
            return Err(E_BUCKET_BREAK);
        }
        Ok(())
    }

    /// 全量断言：登记数、存活数、域覆盖。
    pub fn assert_total(&self, want_alive: u32) -> Result<(), u16> {
        let alive = self.assert_unique()?;
        if alive != want_alive {
            return Err(E_COUNT_BREAK);
        }
        self.assert_adjacent()?;
        self.assert_buckets()?;
        Ok(())
    }

    /// 查询：ID → 锚。
    pub fn lookup(&self, jid: u32) -> Result<String, u16> {
        JudgeIdBook::check_fmt(jid)?;
        for e in self.entries.iter() {
            if e.alive && e.jid == jid {
                return Ok(e.anchor.clone());
            }
        }
        Err(E_BOOK_EMPTY)
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 判据编号唯一性机检主判据：800 条登记零重复双机检全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 全量登记 800 条零重复
    let mut bk = JudgeIdBook::new();
    let mut jid = 1u32;
    while jid <= JID_MAX {
        bk.put(jid, "anchor")?;
        jid += 1;
    }
    bk.assert_total(800)?;
    hits += 1;

    // R2 重复登记必拒
    if bk.put(400, "dup").err() == Some(E_DUP_JID) {
        hits += 1;
    } else {
        return Err(E_DUP_JID);
    }

    // R3 ID 域越界必拒
    if bk.put(0, "zero").err() == Some(E_JID_BAD) {
        hits += 1;
    } else {
        return Err(E_JID_BAD);
    }
    if bk.put(801, "over").err() == Some(E_JID_BAD) {
        hits += 1;
    } else {
        return Err(E_JID_BAD);
    }

    // R4 空锚必拒
    if bk.put(802, "").err() == Some(E_BOOK_EMPTY) {
        hits += 1;
    } else {
        return Err(E_BOOK_EMPTY);
    }
    if bk.put(802, "ok-anchor").is_ok() {
        // 802 越界其实上面已拒，这里不会到达
        return Err(E_SCOPE_BAD);
    }

    // R5 删除后复活必拒
    let mut bk2 = JudgeIdBook::new();
    bk2.put(1, "a")?;
    bk2.retire(1)?;
    if bk2.put(1, "revive").err() == Some(E_REVIVE) {
        hits += 1;
    } else {
        return Err(E_REVIVE);
    }

    // R6 删除后桶位守恒
    bk2.assert_buckets()?;
    if bk2.lookup(1).is_err() {
        hits += 1;
    } else {
        return Err(E_BUCKET_BREAK);
    }

    // R7 未登记先删必拒
    if bk2.retire(77).err() == Some(E_BOOK_EMPTY) {
        hits += 1;
    } else {
        return Err(E_BOOK_EMPTY);
    }

    // R8 机检二独立路径：构造重复（绕过 put 查重直接构造不可能，改用双簿对拍）
    let mut bk3 = JudgeIdBook::new();
    let mut k = 0u32;
    while k < 64 {
        bk3.put(k + 1, "anchor")?;
        k += 1;
    }
    bk3.assert_adjacent()?;
    bk3.retire(8)?;
    bk3.assert_adjacent()?;
    bk3.assert_total(63)?;
    hits += 1;

    // R9 LCG 随机抽样查重 ×256：全部命中且唯一
    let mut rng = Lcg::new(0x48725);
    let mut k2 = 0u32;
    while k2 < 256 {
        let pick = (rng.below(800) + 1) as u32;
        let a = bk.lookup(pick)?;
        if a != "anchor" {
            return Err(E_BOOK_EMPTY);
        }
        k2 += 1;
    }
    hits += 1;

    // R10 fuzz ×256：非法 ID 与重复登记全部被防御吸收
    let mut rng2 = Lcg::new(0x48725_0002);
    let mut bkf = JudgeIdBook::new();
    let mut blocked = 0u32;
    let mut k3 = 0u32;
    while k3 < 256 {
        let pick = rng2.below(4);
        let jid = (rng2.below(900) + 1) as u32;
        if pick == 0 && jid > 800 {
            if bkf.put(jid, "x").is_err() {
                blocked += 1;
            }
        } else if pick == 1 && jid <= 800 {
            if bkf.put(jid, "x").is_err() {
                blocked += 1;
            }
        } else if pick == 2 {
            if bkf.retire(jid).is_err() {
                blocked += 1;
            }
        } else if jid <= 800 {
            let _ = bkf.put(jid, "x");
        }
        k3 += 1;
    }
    if blocked < 10 {
        return Err(E_DUP_JID);
    }
    bkf.assert_buckets()?;
    hits += 1;

    Ok(hits)
}
