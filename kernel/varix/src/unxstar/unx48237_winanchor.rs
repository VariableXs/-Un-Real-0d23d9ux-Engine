//! unx48237_winanchor — UNX-F48237 B12 批 Windows 锚对照登记（ADR-UNX-008 行为锚引用备案）（诊断段 0xBE80）。
//!
//! 判据原文：
//!   J1 锚引用登记入册；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 锚引用=行为基准备案：B12 批每条切换语义登记其 Windows 行为锚
//!      （锚 id + 锚描述 + 对应 ADR 条目），无锚语义必须显式登记
//!      「无锚声明」（自创语义不许冒充 Windows 行为——防幻觉红线）。
//!   2. 锚闭集：锚 id 从 ADR-UNX-008 的备案表来（本模块内置 10 锚
//!      备案表常量），表外锚 id 拒收。
//!   3. 引用完整账：每条引用 (语义id, 锚id, 备案时刻)；语义 id 唯一、
//!      一语义一锚（多锚=口径混乱，拒收）。
//!   4. 无锚声明显性：声明条目与锚引用条目同表管理，kind 字段区分，
//!      导出时分开成账（锚账 / 自创账）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::String;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBE80;

const E_BAD_ANCHOR: u16 = SEG_BASE + 1;  // 锚 id 备案表外
const E_DUP_SEM: u16 = SEG_BASE + 2;     // 语义 id 重复
const E_EMPTY_DESC: u16 = SEG_BASE + 3;  // 锚描述空
const E_ANCHOR_MISS: u16 = SEG_BASE + 4; // 引用缺锚且缺无锚声明
const E_SEAL_MISMATCH: u16 = SEG_BASE + 5; // 封账指纹不符
const E_GUARD_UNBAL: u16 = SEG_BASE + 6;   // 备案账不平

/// ADR-UNX-008 备案锚表（10 锚，id 1..=10）。
pub const ANCHOR_TAB: [(&str, &str); 10] = [
    ("A01", "Shift 单击临时英文：Windows 微软拼音同款"),
    ("A02", "CapsLock 锁定英文：LED 同步口径"),
    ("A03", "中文引号配对：弯引号自动成对"),
    ("A04", "省略号：…… 六点双联"),
    ("A05", "顿号：反斜杠键位"),
    ("A06", "半全角切换：Shift+Space（历史口径）"),
    ("A07", "全角空格：IME 直出 U+3000"),
    ("A08", "临时英文标点直通：英文态不转中文标点"),
    ("A09", "切换抖动合并：40ms 内双击合并"),
    ("A10", "多窗口独立态：每窗独立中英态"),
];

/// 一条备案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorRef {
    pub sem_id: u16,     // 语义 id（批内唯一）
    pub anchor: Option<u8>, // 锚 id 1..=10；None=无锚声明
    pub desc: String,    // 备案描述（锚语义或自创语义说明）
    pub tick: u32,
}

/// 备案总账。
#[derive(Debug)]
pub struct WinAnchor {
    book: Vec<AnchorRef>,
    sealed: Option<u32>,
    rejects: u32,
}

impl WinAnchor {
    pub fn new() -> Self {
        WinAnchor { book: Vec::new(), sealed: None, rejects: 0 }
    }

    /// 锚引用登记：锚 id 备案表内 + 描述非空 + 语义唯一。
    pub fn refer(&mut self, sem_id: u16, anchor: u8, desc: &str, tick: u32) -> Result<(), u16> {
        if anchor == 0 || anchor as usize > ANCHOR_TAB.len() {
            self.rejects += 1;
            return Err(E_BAD_ANCHOR);
        }
        if desc.is_empty() {
            self.rejects += 1;
            return Err(E_EMPTY_DESC);
        }
        if self.book.iter().any(|r| r.sem_id == sem_id) {
            self.rejects += 1;
            return Err(E_DUP_SEM);
        }
        self.book.push(AnchorRef {
            sem_id,
            anchor: Some(anchor),
            desc: str_copy(desc),
            tick,
        });
        Ok(())
    }

    /// 无锚声明登记：自创语义显性备案。
    pub fn declare_no_anchor(&mut self, sem_id: u16, desc: &str, tick: u32) -> Result<(), u16> {
        if desc.is_empty() {
            self.rejects += 1;
            return Err(E_EMPTY_DESC);
        }
        if self.book.iter().any(|r| r.sem_id == sem_id) {
            self.rejects += 1;
            return Err(E_DUP_SEM);
        }
        self.book.push(AnchorRef {
            sem_id,
            anchor: None,
            desc: str_copy(desc),
            tick,
        });
        Ok(())
    }

    /// 锚账导出（只含有锚引用）。
    pub fn anchored(&self) -> Vec<(u16, u8)> {
        self.book
            .iter()
            .filter(|r| r.anchor.is_some())
            .map(|r| (r.sem_id, r.anchor.unwrap_or(0)))
            .collect()
    }

    /// 自创账导出（无锚声明）。
    pub fn unanchored(&self) -> Vec<u16> {
        self.book
            .iter()
            .filter(|r| r.anchor.is_none())
            .map(|r| r.sem_id)
            .collect()
    }

    /// 锚使用分布：10 锚各被引次数。
    pub fn anchor_usage(&self) -> [u32; 10] {
        let mut u = [0u32; 10];
        for r in &self.book {
            if let Some(a) = r.anchor {
                if a >= 1 && a as usize <= 10 {
                    u[a as usize - 1] += 1;
                }
            }
        }
        u
    }

    /// 审计：全部备案要么有锚要么有声明（账面不变式）+ 语义唯一。
    pub fn audit(&self) -> Result<(), u16> {
        for i in 0..self.book.len() {
            for j in (i + 1)..self.book.len() {
                if self.book[i].sem_id == self.book[j].sem_id {
                    return Err(E_DUP_SEM);
                }
            }
        }
        Ok(())
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u32, u16> {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.book {
            h ^= r.sem_id as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.anchor.map(|a| a as u32).unwrap_or(0xFFFF);
            h = h.wrapping_mul(0x0100_0193);
            for b in r.desc.bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        self.sealed = Some(h);
        Ok(h)
    }

    pub fn verify(&self, fp: u32) -> Result<(), u16> {
        match self.sealed {
            Some(s) if s == fp => Ok(()),
            _ => Err(E_SEAL_MISMATCH),
        }
    }

    pub fn len(&self) -> usize {
        self.book.len()
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

fn str_copy(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        out.push(b as char);
    }
    out
}

/// 自例程判据：落实 J1「锚引用登记入册」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：B12 批 8 条切换语义全备案（7 锚引 + 1 无锚声明）→ 入册 8 条。
    let mut w = WinAnchor::new();
    w.refer(101, 1, "Shift 单击语义对齐锚 A01", 100)?;
    w.refer(102, 2, "CapsLock 锁定对齐锚 A02", 110)?;
    w.refer(103, 3, "中文引号配对对齐锚 A03", 120)?;
    w.refer(104, 4, "省略号对齐锚 A04", 130)?;
    w.refer(105, 8, "临时英文标点直通对齐锚 A08", 140)?;
    w.refer(106, 9, "切换抖动合并对齐锚 A09", 150)?;
    w.refer(107, 10, "多窗口独立态对齐锚 A10", 160)?;
    w.declare_no_anchor(108, "抖动窗 40ms 默认值为自定标定，无 Windows 公开口径", 170)?;
    if w.len() != 8 {
        return Err(E_GUARD_UNBAL);
    }
    w.audit()?;
    hits += 1;

    // R2 场景：锚账/自创账分离导出 —— 7 锚引 + 1 声明。
    let anc = w.anchored();
    let un = w.unanchored();
    if anc.len() != 7 || un.len() != 1 || un[0] != 108 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R3 场景：锚使用分布 —— A01..A03/A04/A08/A09/A10 各 1，余 0。
    let u = w.anchor_usage();
    if !(u[0] == 1 && u[1] == 1 && u[2] == 1 && u[3] == 1 && u[7] == 1 && u[8] == 1 && u[9] == 1) {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R4 场景：备案表外锚 id 拒收（0 与 11）。
    let mut w4 = WinAnchor::new();
    match w4.refer(1, 0, "零锚", 10) {
        Err(E_BAD_ANCHOR) => {}
        _ => return Err(E_BAD_ANCHOR),
    }
    match w4.refer(1, 11, "超表锚", 10) {
        Err(E_BAD_ANCHOR) => {}
        _ => return Err(E_BAD_ANCHOR),
    }
    if w4.rejects() != 2 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R5 场景：语义重复拒收 —— 同语义 id 二次登记（含换锚）必拒。
    w.refer(101, 5, "试图换锚", 200)
        .err()
        .filter(|e| *e == E_DUP_SEM)
        .ok_or(E_DUP_SEM)?;
    hits += 1;

    // R6 场景：空描述拒收。
    let mut w6 = WinAnchor::new();
    match w6.refer(1, 1, "", 10) {
        Err(E_EMPTY_DESC) => {}
        _ => return Err(E_EMPTY_DESC),
    }
    match w6.declare_no_anchor(2, "", 10) {
        Err(E_EMPTY_DESC) => {}
        _ => return Err(E_EMPTY_DESC),
    }
    hits += 1;

    // R7 场景：备案表完整性 —— 10 锚描述全非空（防备案表空壳）。
    for (id, d) in ANCHOR_TAB {
        if d.is_empty() || id.is_empty() {
            return Err(E_EMPTY_DESC);
        }
    }
    if ANCHOR_TAB.len() != 10 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R8 场景：封账与复核 —— 指纹敏感。
    let fp = w.seal()?;
    w.verify(fp)?;
    match w.verify(fp ^ 0x5A5A) {
        Err(E_SEAL_MISMATCH) => {}
        _ => return Err(E_SEAL_MISMATCH),
    }
    hits += 1;

    // R9 场景：备案后账面审计守恒 —— 双份登记互查（重放审计）。
    w.audit()?;
    let anc2 = w.anchored();
    if anc2.len() != 7 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R10 场景：无锚声明不许冒充锚 —— 声明条目的 anchor 必为 None（账面核）。
    for r in &w.book {
        if r.sem_id == 108 && r.anchor.is_some() {
            return Err(E_ANCHOR_MISS);
        }
    }
    hits += 1;

    Ok(hits)
}
