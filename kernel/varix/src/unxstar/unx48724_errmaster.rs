//! UNX-F48724 C·错误码全域终表（F/M/E/I 四段并表入 D1）
//! 判据：UNX-F48724-J1 并表 ×全量零漂移；判据锚定 D1 同源、联签 AI-51。
//! 纪律：四段登记-并表-查表三段闭环，段内连续、跨段零重叠、并表序恒 F→M→E→I。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x14A0;

const E_SEG_BAD: u16 = SEG_BASE + 1; // 段号越界
const E_DUP_CODE: u16 = SEG_BASE + 2; // 码值重复
const E_GAP_BREAK: u16 = SEG_BASE + 3; // 段内断档
const E_OVERLAP: u16 = SEG_BASE + 4; // 跨段重叠
const E_ORDER_BAD: u16 = SEG_BASE + 5; // 并表序违 F→M→E→I
const E_LOOKUP_MISS: u16 = SEG_BASE + 6; // 查表未命中
const E_NAME_EMPTY: u16 = SEG_BASE + 7; // 码名空白
const E_COUNT_BREAK: u16 = SEG_BASE + 8; // 全量计数守恒破

/// 段号：F/M/E/I。
#[derive(Clone, Copy, PartialEq)]
pub enum SegId {
    F = 0,
    M = 1,
    E = 2,
    I = 3,
}

impl SegId {
    fn from_u8(v: u8) -> Option<SegId> {
        match v {
            0 => Some(SegId::F),
            1 => Some(SegId::M),
            2 => Some(SegId::E),
            3 => Some(SegId::I),
            _ => None,
        }
    }
    fn order(&self) -> u8 {
        *self as u8
    }
}

/// 单条错误码登记。
#[derive(Clone)]
pub struct ErrEntry {
    /// 段号。
    pub seg: u8,
    /// 码值（全域唯一）。
    pub code: u32,
    /// 码名。
    pub name: String,
}

/// 错误码全域终表。
pub struct ErrMaster {
    entries: Vec<ErrEntry>,
    /// 各段登记基数。
    seg_base: [u32; 4],
    /// 并表序（必须 0,1,2,3）。
    merged_order: [u8; 4],
}

impl ErrMaster {
    pub fn new() -> ErrMaster {
        ErrMaster {
            entries: Vec::new(),
            seg_base: [0; 4],
            merged_order: [0, 1, 2, 3],
        }
    }

    /// 登记一条（段合法、码全域唯一、名非空）。
    pub fn put(&mut self, seg: u8, code: u32, name: &str) -> Result<bool, u16> {
        if SegId::from_u8(seg).is_none() {
            return Err(E_SEG_BAD);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        for e in self.entries.iter() {
            if e.code == code {
                return Err(E_DUP_CODE);
            }
        }
        self.entries.push(ErrEntry {
            seg,
            code,
            name: name.to_string(),
        });
        Ok(true)
    }

    /// 段内连续校验：每段登记的码必须是等差 1 的连续段。
    pub fn assert_contig(&self) -> Result<(), u16> {
        for s in 0u8..4 {
            let mut codes: Vec<u32> = Vec::new();
            for e in self.entries.iter() {
                if e.seg == s {
                    codes.push(e.code);
                }
            }
            if codes.is_empty() {
                continue;
            }
            codes.sort();
            let mut i = 1usize;
            while i < codes.len() {
                if codes[i] != codes[i - 1] + 1 {
                    return Err(E_GAP_BREAK);
                }
                i += 1;
            }
        }
        Ok(())
    }

    /// 跨段零重叠：并表后同码只出现一次（码全域唯一即保证，双保险）。
    pub fn assert_no_overlap(&self) -> Result<(), u16> {
        let mut seen: Vec<u32> = Vec::new();
        for e in self.entries.iter() {
            for s in seen.iter() {
                if *s == e.code {
                    return Err(E_OVERLAP);
                }
            }
            seen.push(e.code);
        }
        Ok(())
    }

    /// 并表：按 F→M→E→I 段序输出全量码表。
    pub fn merge(&mut self) -> Result<Vec<ErrEntry>, u16> {
        self.merged_order = [0, 1, 2, 3];
        let mut out: Vec<ErrEntry> = Vec::new();
        for s in 0u8..4 {
            let mut seg_items: Vec<ErrEntry> = Vec::new();
            for e in self.entries.iter() {
                if e.seg == s {
                    seg_items.push(e.clone());
                }
            }
            seg_items.sort_by(|a, b| a.code.cmp(&b.code));
            for it in seg_items.into_iter() {
                out.push(it);
            }
        }
        if out.len() != self.entries.len() {
            return Err(E_COUNT_BREAK);
        }
        Ok(out)
    }

    /// 并表序断言：输出表的段序必须单调不减且覆盖 0..=3 全序。
    pub fn assert_merge_order(&self, table: &[ErrEntry]) -> Result<(), u16> {
        let mut last = 0u8;
        let mut touched = [false; 4];
        for e in table.iter() {
            if e.seg < last {
                return Err(E_ORDER_BAD);
            }
            last = e.seg;
            touched[e.seg as usize] = true;
        }
        let mut t = 0;
        for f in touched.iter() {
            if *f {
                t += 1;
            }
        }
        if t != 4 {
            return Err(E_ORDER_BAD);
        }
        Ok(())
    }

    /// 查表：码→名。
    pub fn lookup(&self, table: &[ErrEntry], code: u32) -> Result<String, u16> {
        for e in table.iter() {
            if e.code == code {
                if e.name.is_empty() {
                    return Err(E_NAME_EMPTY);
                }
                return Ok(e.name.clone());
            }
        }
        Err(E_LOOKUP_MISS)
    }

    /// 全量计数守恒：登记数 = 四段和。
    pub fn assert_total(&self, table: &[ErrEntry]) -> Result<(), u16> {
        let mut per = [0u32; 4];
        for e in self.entries.iter() {
            per[e.seg as usize] += 1;
        }
        if table.len() as u32 != per[0] + per[1] + per[2] + per[3] {
            return Err(E_COUNT_BREAK);
        }
        Ok(())
    }

    fn len(&self) -> usize {
        self.entries.len()
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

/// 段基址：四段码域互不重叠。
const SEG_F_BASE: u32 = 0x1000;
const SEG_M_BASE: u32 = 0x2000;
const SEG_E_BASE: u32 = 0x3000;
const SEG_I_BASE: u32 = 0x4000;

/// 错误码全域终表主判据：四段登记-并表-全量查表零漂移。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 四段各登记连续 8 条，名带段前缀
    let mut em = ErrMaster::new();
    let bases = [SEG_F_BASE, SEG_M_BASE, SEG_E_BASE, SEG_I_BASE];
    let tags = ["F", "M", "E", "I"];
    let mut s = 0usize;
    while s < 4 {
        let mut k = 0u32;
        while k < 8 {
            let code = bases[s] + k;
            let name = tags[s].to_string() + "_E" + &(k + 1).to_string();
            em.put(s as u8, code, &name)?;
            k += 1;
        }
        s += 1;
    }
    if em.len() != 32 {
        return Err(E_COUNT_BREAK);
    }
    hits += 1;

    // R2 重复码必拒
    if em.put(0, SEG_F_BASE, "dup").err() == Some(E_DUP_CODE) {
        hits += 1;
    } else {
        return Err(E_DUP_CODE);
    }

    // R3 段号越界必拒
    if em.put(9, 0x9999, "bad").err() == Some(E_SEG_BAD) {
        hits += 1;
    } else {
        return Err(E_SEG_BAD);
    }

    // R4 空名必拒
    if em.put(1, 0x2FFF, "").err() == Some(E_NAME_EMPTY) {
        hits += 1;
    } else {
        return Err(E_NAME_EMPTY);
    }

    // R5 段内连续 + 跨段零重叠
    em.assert_contig()?;
    em.assert_no_overlap()?;
    hits += 1;

    // R6 并表：全量 32 条、段序 F→M→E→I、计数守恒
    let table = em.merge()?;
    if table.len() != 32 {
        return Err(E_COUNT_BREAK);
    }
    em.assert_merge_order(&table)?;
    em.assert_total(&table)?;
    hits += 1;

    // R7 全量查表零漂移：32 条逐一对名
    let mut s2 = 0usize;
    while s2 < 4 {
        let mut k = 0u32;
        while k < 8 {
            let code = bases[s2] + k;
            let want = tags[s2].to_string() + "_E" + &(k + 1).to_string();
            let got = em.lookup(&table, code)?;
            if got != want {
                return Err(E_LOOKUP_MISS);
            }
            k += 1;
        }
        s2 += 1;
    }
    hits += 1;

    // R8 查表未命中必拒
    if em.lookup(&table, 0x9999).err() == Some(E_LOOKUP_MISS) {
        hits += 1;
    } else {
        return Err(E_LOOKUP_MISS);
    }

    // R9 段内断档必抓：漏登一条后连续校验应红
    let mut em2 = ErrMaster::new();
    let mut k2 = 0u32;
    while k2 < 8 {
        if k2 != 4 {
            em2.put(0, SEG_F_BASE + k2, "F_E")?;
        }
        k2 += 1;
    }
    if em2.assert_contig().err() == Some(E_GAP_BREAK) {
        hits += 1;
    } else {
        return Err(E_GAP_BREAK);
    }

    // R10 LCG 随机查表 ×256 对照登记表
    let mut rng = Lcg::new(0x48724);
    let mut k3 = 0u32;
    while k3 < 256 {
        let seg_pick = rng.below(4) as usize;
        let off = rng.below(8) as u32;
        let code = bases[seg_pick] + off;
        let want = tags[seg_pick].to_string() + "_E" + &(off + 1).to_string();
        let got = em.lookup(&table, code)?;
        if got != want {
            return Err(E_LOOKUP_MISS);
        }
        k3 += 1;
    }
    hits += 1;

    Ok(hits)
}
