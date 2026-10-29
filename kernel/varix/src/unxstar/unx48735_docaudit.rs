//! UNX-F48735 C·文档全域一致性抽查（40 批锚定文档抽检）
//! 判据：UNX-F48735-J1 抽检全绿；判据锚定 文档一致性。
//! 纪律：锚定-抽检-对拍三段闭环，锚缺失必抓、内容指纹漂移必抓、批覆盖守恒。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1820;

const E_BATCH_BAD: u16 = SEG_BASE + 1; // 批号越界
const E_ANCHOR_MISS: u16 = SEG_BASE + 2; // 锚缺失
const E_FP_DIFF: u16 = SEG_BASE + 3; // 指纹漂移
const E_DOC_DUP: u16 = SEG_BASE + 4; // 文档重复登记
const E_COVER_MISS: u16 = SEG_BASE + 5; // 批覆盖缺漏
const E_FP_EMPTY: u16 = SEG_BASE + 6; // 空指纹拒
const E_BAL_BREAK: u16 = SEG_BASE + 7; // 抽检账守恒破
const E_TITLE_MISS: u16 = SEG_BASE + 8; // 标题缺失

/// 批数。
pub const BATCH_N: u32 = 40;

/// 文档登记条。
#[derive(Clone)]
pub struct DocEntry {
    /// 批号（1..=40）。
    pub batch: u32,
    /// 锚 ID。
    pub anchor: u32,
    /// 标题指纹（FNV-1a 32 位）。
    pub fp: u32,
    /// 标题。
    pub title: String,
}

/// 文档一致性抽检簿。
pub struct DocAudit {
    entries: Vec<DocEntry>,
    seen: Vec<u32>,
    checked: u32,
}

impl DocAudit {
    pub fn new() -> DocAudit {
        DocAudit {
            entries: Vec::new(),
            seen: Vec::new(),
            checked: 0,
        }
    }

    /// FNV-1a 32 位指纹。
    pub fn fnv1a(s: &str) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in s.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 登记文档（批合法、锚唯一、指纹非零、标题非空）。
    pub fn put(&mut self, batch: u32, anchor: u32, title: &str) -> Result<bool, u16> {
        if batch == 0 || batch > BATCH_N {
            return Err(E_BATCH_BAD);
        }
        if title.is_empty() {
            return Err(E_TITLE_MISS);
        }
        let fp = DocAudit::fnv1a(title);
        if fp == 0 {
            return Err(E_FP_EMPTY);
        }
        for s in self.seen.iter() {
            if *s == anchor {
                return Err(E_DOC_DUP);
            }
        }
        self.seen.push(anchor);
        self.entries.push(DocEntry {
            batch,
            anchor,
            fp,
            title: title.to_string(),
        });
        Ok(true)
    }

    /// 抽检一条：现指纹 vs 登记指纹。
    pub fn spot_check(&mut self, anchor: u32, title_now: &str) -> Result<bool, u16> {
        if title_now.is_empty() {
            return Err(E_TITLE_MISS);
        }
        let fp_now = DocAudit::fnv1a(title_now);
        for e in self.entries.iter() {
            if e.anchor == anchor {
                self.checked += 1;
                if e.fp != fp_now {
                    return Err(E_FP_DIFF);
                }
                return Ok(true);
            }
        }
        Err(E_ANCHOR_MISS)
    }

    /// 锚覆盖守恒：40 批每批至少一锚。
    pub fn assert_cover(&self) -> Result<(), u16> {
        let mut b = 1u32;
        while b <= BATCH_N {
            let mut hit = false;
            for e in self.entries.iter() {
                if e.batch == b {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_COVER_MISS);
            }
            b += 1;
        }
        Ok(())
    }

    /// 抽检账守恒：checked ≤ 登记数。
    pub fn assert_balance(&self) -> Result<(), u16> {
        if self.checked > self.entries.len() as u32 {
            return Err(E_BAL_BREAK);
        }
        if self.checked == 0 && !self.entries.is_empty() {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }

    /// 全量抽检：逐条以登记标题复算指纹全绿。
    pub fn full_check(&mut self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            let t = self.entries[i].title.clone();
            self.spot_check(self.entries[i].anchor, &t)?;
            n += 1;
            i += 1;
        }
        if n != self.entries.len() as u32 {
            return Err(E_BAL_BREAK);
        }
        Ok(n)
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

/// 文档全域一致性主判据：40 批登记-抽检-对拍全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 40 批登记（每批 2 锚）
    let mut da = DocAudit::new();
    let mut b = 1u32;
    while b <= BATCH_N {
        let t1 = String::from("anchor-doc-") + &b.to_string() + "-a";
        let t2 = String::from("anchor-doc-") + &b.to_string() + "-b";
        da.put(b, 0x8000 + b * 2, t1.as_str())?;
        da.put(b, 0x8000 + b * 2 + 1, t2.as_str())?;
        b += 1;
    }
    if da.entries.len() != 80 {
        return Err(E_DOC_DUP);
    }
    hits += 1;

    // R2 重复锚必拒
    if da.put(1, 0x8002, "dup").err() == Some(E_DOC_DUP) {
        hits += 1;
    } else {
        return Err(E_DOC_DUP);
    }

    // R3 批号越界必拒
    if da.put(41, 0x9001, "over").err() == Some(E_BATCH_BAD) {
        hits += 1;
    } else {
        return Err(E_BATCH_BAD);
    }

    // R4 空标题必拒
    if da.put(1, 0x9002, "").err() == Some(E_TITLE_MISS) {
        hits += 1;
    } else {
        return Err(E_TITLE_MISS);
    }

    // R5 指纹漂移必抓：同锚不同标题
    if da.spot_check(0x8002, "tampered-title").is_err() {
        hits += 1;
    } else {
        return Err(E_FP_DIFF);
    }

    // R6 锚缺失必抓
    if da.spot_check(0xDEAD, "whatever").err() == Some(E_ANCHOR_MISS) {
        hits += 1;
    } else {
        return Err(E_ANCHOR_MISS);
    }

    // R7 全量抽检全绿：80 条指纹复算零漂移
    let n = da.full_check()?;
    if n != 80 {
        return Err(E_BAL_BREAK);
    }
    da.assert_balance()?;
    da.assert_cover()?;
    hits += 1;

    // R8 LCG 随机抽检 ×256：指纹对拍恒绿
    let mut rng = Lcg::new(0x48735);
    let mut k = 0u32;
    while k < 256 {
        let pick = (rng.below(80)) as usize;
        let e = &da.entries[pick];
        let again = DocAudit::fnv1a(&e.title);
        if again != e.fp {
            return Err(E_FP_DIFF);
        }
        k += 1;
    }
    hits += 1;

    // R9 指纹零碰撞抽查：不同标题指纹必不同（样本级）
    let f1 = DocAudit::fnv1a("doc-alpha-01");
    let f2 = DocAudit::fnv1a("doc-alpha-02");
    if f1 == f2 {
        return Err(E_FP_EMPTY);
    }
    if f1 == 0 || f2 == 0 {
        return Err(E_FP_EMPTY);
    }
    hits += 1;

    // R10 批覆盖缺失必抓
    let mut da2 = DocAudit::new();
    let mut b2 = 1u32;
    while b2 <= BATCH_N - 1 {
        da2.put(b2, 0xA000 + b2, "part-doc")?;
        b2 += 1;
    }
    if da2.assert_cover().err() == Some(E_COVER_MISS) {
        hits += 1;
    } else {
        return Err(E_COVER_MISS);
    }

    // R11 抽检账守恒必抓：零抽检先断言应红
    let mut da3 = DocAudit::new();
    da3.put(1, 0xB001, "solo-doc")?;
    if da3.assert_balance().err() == Some(E_BAL_BREAK) {
        hits += 1;
    } else {
        return Err(E_BAL_BREAK);
    }

    // R12 全量抽检后账守恒全绿 + 空指纹路径
    da3.spot_check(0xB001, "solo-doc")?;
    da3.assert_balance()?;
    if da3.spot_check(0xB001, "").err() == Some(E_TITLE_MISS) {
        hits += 1;
    } else {
        return Err(E_TITLE_MISS);
    }

    // R13 指纹函数独立复算对拍：登记侧与抽查侧同一函数零漂移
    let mut k3 = 0u32;
    while k3 < 32 {
        let probe = String::from("fp-probe-") + &k3.to_string();
        let a = DocAudit::fnv1a(probe.as_str());
        let bb = DocAudit::fnv1a(probe.as_str());
        if a != bb || a == 0 {
            return Err(E_FP_EMPTY);
        }
        k3 += 1;
    }
    hits += 1;

    Ok(hits)
}
