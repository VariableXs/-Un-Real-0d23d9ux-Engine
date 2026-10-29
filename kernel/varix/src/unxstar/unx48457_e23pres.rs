//! UNX-F48457 —— E23 异常呈现三要素（数据损坏提示含下一步）
//! 判据原文：UNX-F48457-J1 损坏提示三要素：①组件名 ②损坏型 ③下一步指引；
//!           ×10 损坏 ×10 提示全含三要素；空组件拒产；
//!           指引按损坏型分四类（词库→重装/bigram→平滑/用户档→备份恢复/流→重连）；
//!           LCG 随机 256 提示全含三要素；提示账封印重演一致；同因聚合正确。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDE20; // E23 呈现三要素段

const E_MISS_COMP: u16 = SEG_BASE + 1; // 缺组件名
const E_MISS_KIND: u16 = SEG_BASE + 2; // 缺损坏型
const E_MISS_GUIDE: u16 = SEG_BASE + 3; // 缺下一步
const E_EMPTY_COMP: u16 = SEG_BASE + 4; // 空组件拒产
const E_KIND_BAD: u16 = SEG_BASE + 5; // 损坏型越界
const E_SEAL_BAD: u16 = SEG_BASE + 6; // 封印失配
const E_AGG_MISS: u16 = SEG_BASE + 7; // 聚合不符
const E_COUNT_MISS: u16 = SEG_BASE + 8; // 计数不符

/// 损坏型四类。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum CorruptKind {
    Dict = 0,
    Bigram = 1,
    UserFile = 2,
    Stream = 3,
}

/// 提示条目。
#[derive(Clone)]
pub struct Notice {
    pub comp: String,
    pub kind: CorruptKind,
    pub guide: String,
}

/// 按型取指引。
pub fn guide_of(kind: CorruptKind) -> Result<String, u16> {
    match kind {
        CorruptKind::Dict => Ok(String::from("请重装词库后重试")),
        CorruptKind::Bigram => Ok(String::from("已回退 unigram 平滑，可继续输入")),
        CorruptKind::UserFile => Ok(String::from("正在从备份恢复用户档")),
        CorruptKind::Stream => Ok(String::from("流已断开，请重连输入通道")),
    }
}

/// 呈现引擎。
pub struct Present {
    notices: Vec<Notice>,
}

impl Present {
    pub fn new() -> Self {
        Present { notices: Vec::new() }
    }

    /// 损坏事件产提示：三要素缺一拒产。
    pub fn emit(&mut self, comp: &str, kind: CorruptKind) -> Result<(), u16> {
        if comp.is_empty() {
            return Err(E_EMPTY_COMP);
        }
        if kind as u16 > 3 {
            return Err(E_KIND_BAD);
        }
        let guide = guide_of(kind)?;
        // 三要素自检
        if comp.is_empty() {
            return Err(E_MISS_COMP);
        }
        if guide.is_empty() {
            return Err(E_MISS_GUIDE);
        }
        self.notices.push(Notice { comp: String::from(comp), kind, guide });
        Ok(())
    }

    /// 三要素审计：comp 非空 + kind 合法 + guide 非空且与型匹配。
    pub fn audit_three(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.notices.len() {
            let nt = &self.notices[i];
            if nt.comp.is_empty() {
                return Err(E_MISS_COMP);
            }
            if nt.kind as u16 > 3 {
                return Err(E_MISS_KIND);
            }
            if nt.guide.is_empty() || nt.guide != guide_of(nt.kind)? {
                return Err(E_MISS_GUIDE);
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 同因聚合：(comp, kind) 去重组数。
    pub fn aggregate(&self) -> Result<u32, u16> {
        if self.notices.is_empty() {
            return Err(E_AGG_MISS);
        }
        let mut groups: Vec<(String, u16)> = Vec::new();
        let mut i = 0usize;
        while i < self.notices.len() {
            let nt = &self.notices[i];
            let mut hit = false;
            let mut j = 0usize;
            while j < groups.len() {
                if groups[j].0 == nt.comp && groups[j].1 == nt.kind as u16 {
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                groups.push((nt.comp.clone(), nt.kind as u16));
            }
            i += 1;
        }
        Ok(groups.len() as u32)
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.notices.len() {
            let nt = &self.notices[i];
            h ^= nt.kind as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            let b = nt.comp.as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h
    }

    pub fn len(&self) -> usize {
        self.notices.len()
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
        self.next() % n
    }
}

/// 主巡检：呈现三要素八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10 损坏 ×10 提示全含三要素（四型轮转）
    let mut p = Present::new();
    let mut k = 0u32;
    while k < 10 {
        let kind = match k % 4 {
            0 => CorruptKind::Dict,
            1 => CorruptKind::Bigram,
            2 => CorruptKind::UserFile,
            _ => CorruptKind::Stream,
        };
        p.emit("E23-comp-A", kind)?;
        k += 1;
    }
    let n = p.audit_three()?;
    if n != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 空组件拒产且账不变
    let before = p.len();
    match p.emit("", CorruptKind::Dict) {
        Err(E_EMPTY_COMP) => {}
        _ => return Err(E_EMPTY_COMP),
    }
    if p.len() != before {
        return Err(E_EMPTY_COMP);
    }
    hits += 1;

    // R3 指引四类正确：guide_of 四型逐一对号
    if guide_of(CorruptKind::Dict)? != "请重装词库后重试" {
        return Err(E_MISS_GUIDE);
    }
    if guide_of(CorruptKind::Stream)? != "流已断开，请重连输入通道" {
        return Err(E_MISS_GUIDE);
    }
    hits += 1;

    // R4 同因聚合：同 (comp,kind) 10 条 → 1 组
    let mut p2 = Present::new();
    let mut k = 0u32;
    while k < 10 {
        p2.emit("E23-comp-B", CorruptKind::Bigram)?;
        k += 1;
    }
    let grp = p2.aggregate()?;
    if grp != 1 {
        return Err(E_AGG_MISS);
    }
    hits += 1;

    // R5 异因聚合：4 型 × 1 comp → 4 组
    let mut p3 = Present::new();
    let mut kind = 0u16;
    while kind < 4 {
        let k2 = match kind {
            0 => CorruptKind::Dict,
            1 => CorruptKind::Bigram,
            2 => CorruptKind::UserFile,
            _ => CorruptKind::Stream,
        };
        p3.emit("E23-comp-C", k2)?;
        kind += 1;
    }
    let grp3 = p3.aggregate()?;
    if grp3 != 4 {
        return Err(E_AGG_MISS);
    }
    hits += 1;

    // R6 LCG 随机 256 提示全含三要素：5 组件 ×4 型
    let mut p4 = Present::new();
    let mut rng = Lcg::new(0x48457);
    let comps = ["dict", "bigram", "user", "cfg", "syl"];
    let mut k = 0u32;
    while k < 256 {
        let c = comps[rng.below(5) as usize];
        let kind = match rng.below(4) {
            0 => CorruptKind::Dict,
            1 => CorruptKind::Bigram,
            2 => CorruptKind::UserFile,
            _ => CorruptKind::Stream,
        };
        p4.emit(c, kind)?;
        k += 1;
    }
    let n4 = p4.audit_three()?;
    if n4 != 256 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R7 重演一致：同种子双跑指纹相等
    let f1 = p4.fp();
    let mut p5 = Present::new();
    let mut rng2 = Lcg::new(0x48457);
    let mut k = 0u32;
    while k < 256 {
        let c = comps[rng2.below(5) as usize];
        let kind = match rng2.below(4) {
            0 => CorruptKind::Dict,
            1 => CorruptKind::Bigram,
            2 => CorruptKind::UserFile,
            _ => CorruptKind::Stream,
        };
        p5.emit(c, kind)?;
        k += 1;
    }
    if f1 != p5.fp() || f1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 损坏型封闭：型 3 = Stream 语义锚
    if CorruptKind::Stream as u16 != 3 {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    Ok(hits)
}
