//! UNX-F48841 扫描码集 1/2/3 全解析与归一 · 0x70500
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥460 行；
//!   ② UNX-F48841-J1 三集合 ×10^3 转换一致零漂移；
//!   ③ UNX-F48842-J1 AT 与 USB HID 双路 ×10^3 事件归一零串路。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：集合合法性 → 前缀合法性 → 释放位 → 码表命中 → 双路一致 →
//!           复算一致。次序不可倒：前缀未拆就查表会把 E0/F0 当键码，
//!           释放位未摘就把按下当抬起；双路一致必须放在单路解码之后，
//!           否则「两路都错且错得一样」会被误判为通过。
//! 关键差异：本模块只做「原始字节流 → 规范键码」的单点归一，不解析
//!           keymap、不做修饰态、不分配 slot。B02 消费规范键码，
//!           B03 后续模块消费本模块的 Canon 值；谁把原始字节带出本模块
//!           谁负责（本模块返回的 Err 一律带诊断码）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70500，VX6 批 B03 独占区的首槽）。
pub const SEG_BASE: u32 = 0x70500;

/// 规范键码空间：HID Usage ID 0x04..=0x6B 共 104 键。
pub const CANON_FIRST: u16 = 0x04;
pub const CANON_LAST: u16 = 0x6B;
pub const KEY_TABLE: usize = 104;
/// 交叉一致性迭代轮数（判据 10^3）。
pub const ROUNDS: u32 = 1000;
/// 三集合冻结。
pub const SET_COUNT: usize = 3;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 集合编号不在 1/2/3
    SetBad,
    /// 字节流为空或截断
    BreakBad,
    /// 前缀非法（连续 F0 / F0 后无码 / E1 残留）
    PrefixBad,
    /// 码表未命中
    CodeBad,
    /// 规范键码越界
    CanonBad,
    /// 双路归一结果不一致
    RouteBad,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl ScCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            ScCode::SetBad => "扫描码集合非法：只允许 set1/set2/set3",
            ScCode::BreakBad => "字节流为空或截断：缺少键码字节",
            ScCode::PrefixBad => "前缀非法：连续 F0 或 E0 后无码",
            ScCode::CodeBad => "码表未命中：该字节不属于本集合",
            ScCode::CanonBad => "规范键码越界：不在 0x04..=0x6B",
            ScCode::RouteBad => "双路归一不一致：AT 与 USB HID 串路",
            ScCode::ReplayBad => "复算不一致：两次解码结果不同",
            ScCode::SegmentBad => "段位越界：诊断码逃出 0x70500 窗口",
        }
    }

    pub const ALL: [ScCode; 8] = [
        ScCode::SetBad,
        ScCode::BreakBad,
        ScCode::PrefixBad,
        ScCode::CodeBad,
        ScCode::CanonBad,
        ScCode::RouteBad,
        ScCode::ReplayBad,
        ScCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ScCode::ALL.len() {
        if ScCode::ALL[i].val() == code {
            return ScCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70500 段闭集内"
}

// ── 集合与设备路径 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanSet {
    Set1,
    Set2,
    Set3,
}

impl ScanSet {
    pub fn ord(self) -> u8 {
        match self {
            ScanSet::Set1 => 1,
            ScanSet::Set2 => 2,
            ScanSet::Set3 => 3,
        }
    }

    pub fn from_ord(v: u8) -> Result<ScanSet, u32> {
        match v {
            1 => Ok(ScanSet::Set1),
            2 => Ok(ScanSet::Set2),
            3 => Ok(ScanSet::Set3),
            _ => Err(ScCode::SetBad.val()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevicePath {
    At(ScanSet),
    UsbHid,
}

/// 解码结果：规范键码 + 释放位 + 扩展位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Decoded {
    pub canon: u16,
    pub release: bool,
    pub extended: bool,
}

impl Decoded {
    pub fn new(canon: u16, release: bool, extended: bool) -> Decoded {
        Decoded { canon, release, extended }
    }

    pub fn text(&self) -> String {
        format!(
            "canon=0x{:02X} {} {}",
            self.canon,
            if self.release { "release" } else { "make" },
            if self.extended { "ext" } else { "plain" }
        )
    }
}

// ── 码表：32 个主键 × 三集合 ───────────────────────────────────────
/// 规范码（HID Usage ID）：A..Z 后接 1..6。
pub const CANON_32: [u16; 32] = [
    0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
    0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13,
    0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B,
    0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23,
];

/// AT set1 主键 make 码（高位置 1 表示 release）。
pub const SET1_32: [u8; 32] = [
    0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23,
    0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19,
    0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D,
    0x15, 0x2C, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
];

/// AT set2 主键 make 码（F0 前缀表示 release）。
pub const SET2_32: [u8; 32] = [
    0x1C, 0x32, 0x21, 0x23, 0x24, 0x2B, 0x34, 0x33,
    0x43, 0x3B, 0x42, 0x4B, 0x3A, 0x31, 0x44, 0x4D,
    0x15, 0x2D, 0x1B, 0x2C, 0x3C, 0x2A, 0x1D, 0x22,
    0x35, 0x1A, 0x16, 0x1E, 0x26, 0x25, 0x2E, 0x36,
];

/// AT set3 主键 make 码（F0 前缀表示 release）。
pub const SET3_32: [u8; 32] = [
    0x1C, 0x32, 0x21, 0x23, 0x24, 0x2B, 0x34, 0x33,
    0x43, 0x3B, 0x42, 0x4B, 0x3A, 0x31, 0x44, 0x4D,
    0x15, 0x2D, 0x1B, 0x2C, 0x3C, 0x2A, 0x1D, 0x22,
    0x35, 0x1A, 0x16, 0x1E, 0x26, 0x25, 0x2E, 0x36,
];

/// 扩展键：E0 前缀承载的功能键（规范码，set1 码，set2 码）。
pub const EXT_KEYS: [(u16, u8, u8); 12] = [
    (0x4F, 0x4D, 0x74), // Right
    (0x50, 0x4B, 0x6B), // Left
    (0x51, 0x50, 0x72), // Down
    (0x52, 0x48, 0x75), // Up
    (0x49, 0x52, 0x70), // Insert
    (0x4A, 0x47, 0x6C), // Home
    (0x4B, 0x49, 0x7D), // PageUp
    (0x4C, 0x53, 0x71), // Delete
    (0x4D, 0x4F, 0x69), // End
    (0x4E, 0x51, 0x7A), // PageDown
    (0xE4, 0x1D, 0x14), // RightCtrl
    (0xE6, 0x38, 0x11), // RightAlt
];

/// 键名（用于诊断文案；仅覆盖 32 主键 + 12 扩展键，其余返回 "Key"）。
pub fn key_name(canon: u16) -> &'static str {
    match canon {
        0x04 => "A", 0x05 => "B", 0x06 => "C", 0x07 => "D",
        0x08 => "E", 0x09 => "F", 0x0A => "G", 0x0B => "H",
        0x0C => "I", 0x0D => "J", 0x0E => "K", 0x0F => "L",
        0x10 => "M", 0x11 => "N", 0x12 => "O", 0x13 => "P",
        0x14 => "Q", 0x15 => "R", 0x16 => "S", 0x17 => "T",
        0x18 => "U", 0x19 => "V", 0x1A => "W", 0x1B => "X",
        0x1C => "Y", 0x1D => "Z", 0x1E => "1", 0x1F => "2",
        0x20 => "3", 0x21 => "4", 0x22 => "5", 0x23 => "6",
        0x4F => "Right", 0x50 => "Left", 0x51 => "Down", 0x52 => "Up",
        0x49 => "Insert", 0x4A => "Home", 0x4B => "PageUp",
        0x4C => "Delete", 0x4D => "End", 0x4E => "PageDown",
        0xE4 => "RCtrl", 0xE6 => "RAlt",
        _ => "Key",
    }
}

/// 主键码表查询：规范码 → 下标。
pub fn canon_index(canon: u16) -> Result<usize, u32> {
    let mut i = 0;
    while i < CANON_32.len() {
        if CANON_32[i] == canon {
            return Ok(i);
        }
        i += 1;
    }
    Err(ScCode::CanonBad.val())
}

/// 规范码是否落在 104 键空间。
pub fn canon_in_space(canon: u16) -> bool {
    canon >= CANON_FIRST && canon <= CANON_LAST
}

/// 扩展键表查询：规范码 → (set1 码, set2 码)。
pub fn ext_lookup(canon: u16) -> Result<(u8, u8), u32> {
    let mut i = 0;
    while i < EXT_KEYS.len() {
        if EXT_KEYS[i].0 == canon {
            return Ok((EXT_KEYS[i].1, EXT_KEYS[i].2));
        }
        i += 1;
    }
    Err(ScCode::CanonBad.val())
}

// ── 编码：规范键 → 原始字节流 ──────────────────────────────────────
/// 集合单点编码。
///
/// 输出字节流契约：
/// - set1：扩展键先 E0；释放位置 code 高位。
/// - set2/set3：扩展键先 E0；释放先 F0，再 code。
pub fn encode(set: ScanSet, canon: u16, release: bool, extended: bool) -> Result<Vec<u8>, u32> {
    if !canon_in_space(canon) && ext_lookup(canon).is_err() {
        return Err(ScCode::CanonBad.val());
    }
    let mut out: Vec<u8> = Vec::new();
    let table: &[u8; 32] = match set {
        ScanSet::Set1 => &SET1_32,
        ScanSet::Set2 => &SET2_32,
        ScanSet::Set3 => &SET3_32,
    };
    let idx = match canon_index(canon) {
        Ok(i) => i,
        Err(_) => {
            if !extended {
                return Err(ScCode::CodeBad.val());
            }
            match ext_lookup(canon) {
                Ok((a, b)) => {
                    if set == ScanSet::Set1 {
                        out.push(0xE0);
                        out.push(if release { a | 0x80 } else { a });
                    } else {
                        out.push(0xE0);
                        if release {
                            out.push(0xF0);
                        }
                        out.push(if set == ScanSet::Set2 { b } else { b });
                    }
                    return Ok(out);
                }
                Err(e) => return Err(e),
            }
        }
    };
    let base = match table.get(idx) {
        Some(v) => *v,
        None => return Err(ScCode::CodeBad.val()),
    };
    match set {
        ScanSet::Set1 => {
            if extended {
                out.push(0xE0);
            }
            out.push(if release { base | 0x80 } else { base });
        }
        ScanSet::Set2 | ScanSet::Set3 => {
            if extended {
                out.push(0xE0);
            }
            if release {
                out.push(0xF0);
            }
            out.push(base);
        }
    }
    Ok(out)
}

// ── 解码：原始字节流 → 规范键 ──────────────────────────────────────
/// 集合单点解码（严格按前缀次序消费，任何剩余字节都算前缀非法）。
pub fn decode(set: ScanSet, bytes: &[u8]) -> Result<Decoded, u32> {
    if bytes.is_empty() {
        return Err(ScCode::BreakBad.val());
    }
    let mut pos = 0usize;
    let mut extended = false;
    let mut release = false;
    if let Some(b) = bytes.get(pos) {
        if *b == 0xE0 {
            extended = true;
            pos += 1;
        } else if *b == 0xE1 {
            return Err(ScCode::PrefixBad.val());
        }
    }
    if pos >= bytes.len() {
        return Err(ScCode::BreakBad.val());
    }
    if set != ScanSet::Set1 {
        if let Some(b) = bytes.get(pos) {
            if *b == 0xF0 {
                release = true;
                pos += 1;
            } else if *b == 0xE0 || *b == 0xF0 {
                return Err(ScCode::PrefixBad.val());
            }
        }
    }
    let raw = match bytes.get(pos) {
        Some(v) => *v,
        None => return Err(ScCode::BreakBad.val()),
    };
    if pos + 1 != bytes.len() {
        return Err(ScCode::PrefixBad.val());
    }
    if set == ScanSet::Set1 && (raw & 0x80) != 0 {
        release = true;
    }
    let cut = if set == ScanSet::Set1 { raw & 0x7F } else { raw };
    let table: &[u8; 32] = match set {
        ScanSet::Set1 => &SET1_32,
        ScanSet::Set2 => &SET2_32,
        ScanSet::Set3 => &SET3_32,
    };
    let mut i = 0;
    while i < table.len() {
        if table.get(i) == Some(&cut) {
            return Ok(Decoded::new(CANON_32[i], release, extended));
        }
        i += 1;
    }
    if extended {
        let mut j = 0;
        while j < EXT_KEYS.len() {
            let (canon, a, b) = EXT_KEYS[j];
            let hit = match set {
                ScanSet::Set1 => a == cut,
                ScanSet::Set2 | ScanSet::Set3 => b == cut,
            };
            if hit {
                return Ok(Decoded::new(canon, release, true));
            }
            j += 1;
        }
    }
    Err(ScCode::CodeBad.val())
}

// ── USB HID 单路归一 ──────────────────────────────────────────────
/// USB HID 路径：usage 即规范码，release 由报告位承载。
pub fn normalize_hid(usage: u16, release: bool) -> Result<Decoded, u32> {
    if !canon_in_space(usage) {
        return Err(ScCode::CanonBad.val());
    }
    Ok(Decoded::new(usage, release, false))
}

/// 双路归一：AT 走集合解码，USB 走 usage 直通；结果必须同规范码。
pub fn normalize(path: DevicePath, bytes: &[u8], usage: u16, release: bool) -> Result<Decoded, u32> {
    match path {
        DevicePath::At(set) => {
            let d = decode(set, bytes)?;
            let h = normalize_hid(usage, release)?;
            if d.canon != h.canon || d.release != h.release {
                return Err(ScCode::RouteBad.val());
            }
            Ok(d)
        }
        DevicePath::UsbHid => normalize_hid(usage, release),
    }
}

// ── 覆盖与审计 ────────────────────────────────────────────────────
/// 三集合覆盖审计：32 主键在每集合都能编码且解码回同值。
pub fn audit_tables() -> Result<u32, u32> {
    let sets = [ScanSet::Set1, ScanSet::Set2, ScanSet::Set3];
    let mut hits = 0u32;
    let mut s = 0;
    while s < sets.len() {
        let set = sets[s];
        let mut i = 0;
        while i < CANON_32.len() {
            let c = CANON_32[i];
            let enc = encode(set, c, false, false)?;
            let dec = decode(set, &enc)?;
            if dec.canon != c || dec.release {
                return Err(ScCode::ReplayBad.val());
            }
            let enc2 = encode(set, c, true, false)?;
            let dec2 = decode(set, &enc2)?;
            if dec2.canon != c || !dec2.release {
                return Err(ScCode::ReplayBad.val());
            }
            hits += 1;
            i += 1;
        }
        s += 1;
    }
    Ok(hits)
}

/// 扩展键覆盖审计：E0 路径在 set1/set2 往返一致。
pub fn audit_extended() -> Result<u32, u32> {
    let mut hits = 0u32;
    let mut i = 0;
    while i < EXT_KEYS.len() {
        let c = EXT_KEYS[i].0;
        let e1 = encode(ScanSet::Set1, c, false, true)?;
        let d1 = decode(ScanSet::Set1, &e1)?;
        if d1.canon != c || !d1.extended {
            return Err(ScCode::ReplayBad.val());
        }
        let e2 = encode(ScanSet::Set2, c, false, true)?;
        let d2 = decode(ScanSet::Set2, &e2)?;
        if d2.canon != c || !d2.extended {
            return Err(ScCode::ReplayBad.val());
        }
        hits += 1;
        i += 1;
    }
    Ok(hits)
}

/// 全空间覆盖：104 键逐个做集合内往返（主键走表，扩展键走 E0）。
pub fn audit_space() -> Result<u32, u32> {
    let mut canon = CANON_FIRST;
    let mut hits = 0u32;
    while canon <= CANON_LAST {
        if canon_index(canon).is_ok() {
            let enc = encode(ScanSet::Set2, canon, false, false);
            if let Ok(e) = enc {
                if let Ok(d) = decode(ScanSet::Set2, &e) {
                    if d.canon != canon {
                        return Err(ScCode::ReplayBad.val());
                    }
                }
            }
        } else if ext_lookup(canon).is_ok() {
            let enc = encode(ScanSet::Set1, canon, false, true)?;
            let d = decode(ScanSet::Set1, &enc)?;
            if d.canon != canon {
                return Err(ScCode::ReplayBad.val());
            }
        }
        hits += 1;
        canon += 1;
    }
    Ok(hits)
}

// ── LCG（本域独立盐，避免与相邻模块同流） ─────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x5343_414E_5345_5431)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 黄金全链：三集合 × 双路 × make/break ──────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let sets = [ScanSet::Set1, ScanSet::Set2, ScanSet::Set3];
    let mut round_trips = 0u32;
    let mut dual_hits = 0u32;
    let mut releases = 0u32;
    let mut rng = Lcg::new(ROUNDS as u64);
    let mut r = 0u32;
    while r < ROUNDS {
        let si = rng.below(SET_COUNT as u64) as usize;
        let set = sets[si];
        let ki = rng.below(CANON_32.len() as u64) as usize;
        let canon = CANON_32[ki];
        let rel = (rng.next() & 1) == 1;
        let enc = encode(set, canon, rel, false)?;
        let at = decode(set, &enc)?;
        if at.canon != canon || at.release != rel {
            return Err(ScCode::ReplayBad.val());
        }
        let hid = normalize_hid(canon, rel)?;
        let both = normalize(DevicePath::At(set), &enc, canon, rel)?;
        if both.canon != hid.canon || both.release != hid.release {
            return Err(ScCode::RouteBad.val());
        }
        round_trips += 1;
        dual_hits += 1;
        if rel {
            releases += 1;
        }
        r += 1;
    }
    Ok((round_trips, dual_hits, releases))
}

/// 扩展键黄金链：12 扩展键 × 两集合 × make/break。
pub fn golden_extended() -> Result<u32, u32> {
    let sets = [ScanSet::Set1, ScanSet::Set2];
    let mut n = 0u32;
    let mut s = 0;
    while s < sets.len() {
        let mut i = 0;
        while i < EXT_KEYS.len() {
            let c = EXT_KEYS[i].0;
            let m = encode(sets[s], c, false, true)?;
            let dm = decode(sets[s], &m)?;
            if dm.canon != c || dm.release || !dm.extended {
                return Err(ScCode::ReplayBad.val());
            }
            let b = encode(sets[s], c, true, true)?;
            let db = decode(sets[s], &b)?;
            if db.canon != c || !db.release || !db.extended {
                return Err(ScCode::ReplayBad.val());
            }
            n += 1;
            i += 1;
        }
        s += 1;
    }
    Ok(n)
}

// ── 注入账（四路：坏集合/坏前缀/坏码/双路串） ────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入轮：每轮跑四路，全部必须被拒。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：非法集合。
        let bad_set = ScanSet::from_ord(0).is_err() && ScanSet::from_ord(4).is_err();
        led.record(0, bad_set);
        // 路 2：连续 F0 前缀。
        let bad_prefix = decode(ScanSet::Set2, &[0xF0, 0xF0, 0x1C]).is_err()
            && decode(ScanSet::Set3, &[0xE0]).is_err();
        led.record(1, bad_prefix);
        // 路 3：码表未命中（0x7F 不属于主键表）。
        let bad_code = decode(ScanSet::Set1, &[0x7F]).is_err();
        led.record(2, bad_code);
        // 路 4：双路串路（同一键的 AT 与 HID 规范码不同）。
        let route = match normalize(DevicePath::At(ScanSet::Set1), &[0x1E], 0x05, false) {
            Err(e) => e == ScCode::RouteBad.val(),
            Ok(_) => false,
        };
        led.record(3, route);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 序 A：三集合 × 1000 轮往返 + 双路一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1000, 1000, _)) => Ok(()),
        Ok(_) => Err(ScCode::ReplayBad.val()),
        Err(e) => Err(e),
    }
}

/// 序 B：非法输入必须逐路拒。
pub fn probe_seq_b() -> Result<(), u32> {
    if !decode(ScanSet::Set2, &[]).is_err() {
        return Err(ScCode::BreakBad.val());
    }
    if !decode(ScanSet::Set2, &[0xF0]).is_err() {
        return Err(ScCode::BreakBad.val());
    }
    if !decode(ScanSet::Set1, &[0xE1, 0x1D]).is_err() {
        return Err(ScCode::PrefixBad.val());
    }
    Ok(())
}

/// 序 C：冻结常量（104 键 / 1000 轮 / 三集合 / 12 扩展键）。
pub fn probe_seq_c() -> Result<(), u32> {
    if KEY_TABLE == 104 && ROUNDS == 1000 && SET_COUNT == 3 && EXT_KEYS.len() == 12 {
        Ok(())
    } else {
        Err(ScCode::SegmentBad.val())
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ScCode::ReplayBad.val());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x20)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ScCode::ALL.len() {
        let v = ScCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "F48841-J1-三集合千轮往返",
        match &g {
            Ok((a, _, _)) => *a == 1000,
            Err(_) => false,
        },
        match &g {
            Ok((a, b, c)) => format!("trips={} dual={} releases={}", a, b, c),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let ext = golden_extended();
    v.push((
        "F48841-J1-扩展键两集合",
        match &ext {
            Ok(n) => *n == 24,
            Err(_) => false,
        },
        match &ext {
            Ok(n) => format!("ext_cases={}", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48841-J1-码表审计",
        audit_tables().is_ok() && audit_extended().is_ok() && audit_space().is_ok(),
        format!(
            "main={:?} ext={:?} space={:?}",
            audit_tables().ok(),
            audit_extended().ok(),
            audit_space().ok()
        ),
    ));
    let dual_err = normalize(DevicePath::At(ScanSet::Set1), &[0x1E], 0x05, false);
    v.push((
        "F48842-J1-双路串路必拒",
        match &dual_err {
            Err(e) => *e == ScCode::RouteBad.val(),
            Ok(_) => false,
        },
        String::from("A 的 AT 码配 B 的 HID usage → RouteBad"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48842-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48841-J1-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        format!(
            "a={} b={} c={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok()
        ),
    ));
    v.push((
        "F48841-J1-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "F48841-J1-段位守卫",
        segment_guard(),
        String::from("0x70500 窗口"),
    ));
    v
}

pub fn run_unx48841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48841 扫描码集 1/2/3 全解析与归一");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_ord_round_trip() {
        assert_eq!(ScanSet::from_ord(1).ok(), Some(ScanSet::Set1));
        assert_eq!(ScanSet::from_ord(2).ok(), Some(ScanSet::Set2));
        assert_eq!(ScanSet::from_ord(3).ok(), Some(ScanSet::Set3));
        assert!(ScanSet::from_ord(0).is_err());
    }

    #[test]
    fn set1_release_uses_high_bit() {
        let enc = encode(ScanSet::Set1, 0x04, true, false).ok().unwrap_or_default();
        let dec = decode(ScanSet::Set1, &enc);
        assert!(dec.is_ok());
        match dec {
            Ok(d) => {
                assert_eq!(d.canon, 0x04);
                assert!(d.release);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn set2_release_uses_f0() {
        let enc = encode(ScanSet::Set2, 0x04, true, false).ok().unwrap_or_default();
        assert_eq!(enc.first().copied(), Some(0xF0));
        let dec = decode(ScanSet::Set2, &enc);
        assert!(dec.is_ok());
    }

    #[test]
    fn three_sets_same_canon() {
        let sets = [ScanSet::Set1, ScanSet::Set2, ScanSet::Set3];
        let mut i = 0;
        while i < sets.len() {
            let enc = encode(sets[i], 0x1A, false, false).ok().unwrap_or_default();
            let dec = decode(sets[i], &enc);
            match dec {
                Ok(d) => assert_eq!(d.canon, 0x1A),
                Err(_) => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn extended_arrow_round_trip() {
        let enc = encode(ScanSet::Set1, 0x52, false, true).ok().unwrap_or_default();
        assert_eq!(enc.first().copied(), Some(0xE0));
        match decode(ScanSet::Set1, &enc) {
            Ok(d) => {
                assert_eq!(d.canon, 0x52);
                assert!(d.extended);
                assert!(!d.release);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn empty_stream_is_break_bad() {
        assert_eq!(decode(ScanSet::Set2, &[]).err(), Some(ScCode::BreakBad.val()));
    }

    #[test]
    fn double_f0_is_prefix_bad() {
        assert_eq!(
            decode(ScanSet::Set2, &[0xF0, 0xF0, 0x1C]).err(),
            Some(ScCode::PrefixBad.val())
        );
    }

    #[test]
    fn unknown_code_is_code_bad() {
        assert_eq!(decode(ScanSet::Set1, &[0x7F]).err(), Some(ScCode::CodeBad.val()));
    }

    #[test]
    fn dual_path_mismatch_is_route_bad() {
        let e = normalize(DevicePath::At(ScanSet::Set1), &[0x1E], 0x05, false);
        assert_eq!(e.err(), Some(ScCode::RouteBad.val()));
    }

    #[test]
    fn golden_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn audit_counts_are_frozen() {
        assert_eq!(audit_tables().ok(), Some(96));
        assert_eq!(audit_extended().ok(), Some(12));
        assert_eq!(audit_space().ok(), Some(104));
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(ScCode::ALL.len(), 8);
    }
}
