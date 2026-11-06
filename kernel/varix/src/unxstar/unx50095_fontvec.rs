//! UNX-F50095 字体向量账（嵌入用例集+校验和）（0x43800 段）
//!
//! 锚点：UNX-F50095-J1 用例可复跑；判据锚定 §AI-63 §5。
//!
//! 向量账的作用是把「字体在真实文本上的推进结果」固化成可重放的证据：
//! 每条用例记录字体摘要、码点序列与期望校验和，入账时重算校验和比对，
//! 复核时再整账重放。校验和按 (字形号, 步进, 位序) 三元组顺序折叠，
//! 因此换序、换字体、换码点都会得到不同结果——不需要比对像素。
//!
//! 结构清单：
//! * 用例 [`VectorCase`] 与运行结果 [`VectorRun`]；
//! * 账本 [`VectorLedger`]（定容、Open→Sealed 状态机、重复 id 拒绝）；
//! * 确定性度量/校验和 [`glyph_of`] / [`advance_of`] / [`checksum`]；
//! * 重放器 [`VectorLedger::run_all`]（逐条重算，任一不符即 Mismatch）；
//! * 注入核 [`injection_core`]：9 类坏用例逐类断言具体诊断码。
//!
//! 诊断码闭集：`SEG_BASE + n`，n ∈ [1,10]。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43800 独占 0x20）。
pub const SEG_BASE: u32 = 0x43800;

pub const E_EMPTY: u16 = 1;
pub const E_BADFONT: u16 = 2;
pub const E_BADCODE: u16 = 3;
pub const E_GLYPHMISS: u16 = 4;
pub const E_DUP: u16 = 5;
pub const E_CAP: u16 = 6;
pub const E_MISMATCH: u16 = 7;
pub const E_NOCASES: u16 = 8;
pub const E_ORDER: u16 = 9;
pub const E_OVERFLOW: u16 = 10;

const FNV32_SEED: u32 = 2166136261;
const FNV32_PRIME: u32 = 16777619;
const FNV64_SEED: u64 = 0xcbf2_9ce4_8422_2325;
const FNV64_PRIME: u64 = 0x1000_0000_01b3;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    Empty,
    BadFont,
    BadCode,
    GlyphMissing,
    Duplicate,
    Cap,
    Mismatch,
    NoCases,
    Order,
    Overflow,
}

impl VecError {
    /// 段内序号（闭集 1..=10）。
    pub fn ordinal(self) -> u16 {
        match self {
            VecError::Empty => E_EMPTY,
            VecError::BadFont => E_BADFONT,
            VecError::BadCode => E_BADCODE,
            VecError::GlyphMissing => E_GLYPHMISS,
            VecError::Duplicate => E_DUP,
            VecError::Cap => E_CAP,
            VecError::Mismatch => E_MISMATCH,
            VecError::NoCases => E_NOCASES,
            VecError::Order => E_ORDER,
            VecError::Overflow => E_OVERFLOW,
        }
    }

    /// 全仓诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断说明。
    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "vector text empty",
            VecError::BadFont => "font digest is zero",
            VecError::BadCode => "codepoint beyond unicode range",
            VecError::GlyphMissing => "font has no glyph for codepoint",
            VecError::Duplicate => "case id already recorded",
            VecError::Cap => "ledger capacity exhausted",
            VecError::Mismatch => "checksum does not recompute",
            VecError::NoCases => "ledger has no cases",
            VecError::Order => "ledger already sealed",
            VecError::Overflow => "case id space exhausted",
        }
    }
}

/// 确定性字形号：仅由码点决定，便于跨机复算。
pub fn glyph_of(cp: u32) -> Result<u16, VecError> {
    if cp > 0x10FFFF {
        return Err(VecError::BadCode);
    }
    // 补充私用区不参与向量集：给「字形缺失」留一个真实可触发的分支。
    if cp >= 0xF0000 {
        return Err(VecError::GlyphMissing);
    }
    Ok(((cp.wrapping_mul(0x85EB_CA6B) >> 7) & 0xFFFF) as u16)
}

/// 确定性步进（字体单位）：空格 0 / 半角 1024 / 扩展拉丁 1100 / 全角 2048。
pub fn advance_of(cp: u32) -> Result<u16, VecError> {
    let _ = glyph_of(cp)?;
    if cp == 0x20 {
        Ok(0)
    } else if cp >= 0x2E80 {
        Ok(2048)
    } else if cp >= 0x80 {
        Ok(1100)
    } else {
        Ok(1024)
    }
}

/// 向量校验和：字体摘要做种子，按 (glyph, advance, index) 顺序折叠。
pub fn checksum(font: u64, text: &[u32]) -> Result<u32, VecError> {
    if font == 0 {
        return Err(VecError::BadFont);
    }
    let mut h = FNV32_SEED ^ (font as u32) ^ ((font >> 32) as u32);
    let mut i = 0usize;
    while let Some(cp) = text.get(i) {
        let gid = glyph_of(*cp)?;
        let adv = advance_of(*cp)?;
        h ^= gid as u32;
        h = h.wrapping_mul(FNV32_PRIME);
        h ^= adv as u32;
        h = h.wrapping_mul(FNV32_PRIME);
        h ^= i as u32;
        h = h.wrapping_mul(FNV32_PRIME);
        i += 1;
    }
    Ok(h)
}

/// 一条向量用例：期望校验和由 [`make_case`] 从内容算出。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorCase {
    pub id: u16,
    pub font: u64,
    pub text: Vec<u32>,
    pub expected: u32,
}

/// 账本阶段。Sealed 后停止收新用例，已交付的账不被追加污染。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecStage {
    Open,
    Sealed,
}

impl VecStage {
    fn code(self) -> u8 {
        match self {
            VecStage::Open => 0,
            VecStage::Sealed => 1,
        }
    }
}

/// 整账重放的结果。`digest` 覆盖 (id, checksum) 序列，顺序敏感。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VectorRun {
    pub cases: usize,
    pub glyphs: u32,
    pub digest: u64,
}

/// 向量账本：定容、单写者、逐条可复核。
pub struct VectorLedger {
    pub cases: Vec<VectorCase>,
    cap: usize,
    stage: VecStage,
}

impl VectorLedger {
    /// 新建账本。
    pub fn new(cap: usize) -> VectorLedger {
        VectorLedger {
            cases: Vec::with_capacity(cap),
            cap,
            stage: VecStage::Open,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> VecStage {
        self.stage
    }

    /// 用例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.stage = VecStage::Sealed;
    }

    /// 入账：顺序为 阶段→id 空间→字体→空文本→重复→容量→校验和。
    pub fn admit(&mut self, case: VectorCase) -> Result<u32, VecError> {
        if self.stage != VecStage::Open {
            return Err(VecError::Order);
        }
        if case.id == u16::MAX {
            return Err(VecError::Overflow);
        }
        if case.font == 0 {
            return Err(VecError::BadFont);
        }
        if case.text.is_empty() {
            return Err(VecError::Empty);
        }
        let mut i = 0usize;
        while let Some(c) = self.cases.get(i) {
            if c.id == case.id {
                return Err(VecError::Duplicate);
            }
            i += 1;
        }
        if self.cases.len() >= self.cap {
            return Err(VecError::Cap);
        }
        let want = checksum(case.font, &case.text)?;
        if want != case.expected {
            return Err(VecError::Mismatch);
        }
        self.cases.push(case);
        Ok(want)
    }

    /// 按 id 取用例。
    pub fn find(&self, id: u16) -> Option<&VectorCase> {
        let mut i = 0usize;
        while let Some(c) = self.cases.get(i) {
            if c.id == id {
                return Some(c);
            }
            i += 1;
        }
        None
    }

    /// 整账重放：逐条重算校验和，任一不符立即 Mismatch。
    pub fn run_all(&self) -> Result<VectorRun, VecError> {
        if self.cases.is_empty() {
            return Err(VecError::NoCases);
        }
        let mut glyphs = 0u32;
        let mut digest = FNV64_SEED;
        let mut i = 0usize;
        while let Some(c) = self.cases.get(i) {
            let got = checksum(c.font, &c.text)?;
            if got != c.expected {
                return Err(VecError::Mismatch);
            }
            glyphs = glyphs.saturating_add(c.text.len() as u32);
            digest = (digest ^ c.id as u64).wrapping_mul(FNV64_PRIME);
            digest = (digest ^ got as u64).wrapping_mul(FNV64_PRIME);
            i += 1;
        }
        Ok(VectorRun {
            cases: self.cases.len(),
            glyphs,
            digest,
        })
    }

    /// 账本摘要（含阶段），用于跨批次比对。
    pub fn digest(&self) -> u64 {
        let mut h = (FNV64_SEED ^ self.stage.code() as u64).wrapping_mul(FNV64_PRIME);
        h = (h ^ self.cases.len() as u64).wrapping_mul(FNV64_PRIME);
        let mut i = 0usize;
        while let Some(c) = self.cases.get(i) {
            h = (h ^ c.id as u64).wrapping_mul(FNV64_PRIME);
            h = (h ^ c.font).wrapping_mul(FNV64_PRIME);
            h = (h ^ c.expected as u64).wrapping_mul(FNV64_PRIME);
            i += 1;
        }
        h
    }
}

/// 从内容生成用例（期望校验和现算，供构造标准集使用）。
pub fn make_case(id: u16, font: u64, text: Vec<u32>) -> Result<VectorCase, VecError> {
    if text.is_empty() {
        return Err(VecError::Empty);
    }
    let expected = checksum(font, &text)?;
    Ok(VectorCase {
        id,
        font,
        text,
        expected,
    })
}

fn push_ascii(out: &mut Vec<u32>, text: &[u8]) {
    let mut i = 0usize;
    while let Some(b) = text.get(i) {
        out.push(*b as u32);
        i += 1;
    }
}

fn push_codes(out: &mut Vec<u32>, codes: &[u16]) {
    let mut i = 0usize;
    while let Some(c) = codes.get(i) {
        out.push(*c as u32);
        i += 1;
    }
}

/// 四条标准用例：西文 / 扩展拉丁 / CJK / 混排，字体摘要固定。
pub fn standard_cases() -> Vec<VectorCase> {
    let mut out = Vec::new();
    let mut a = Vec::new();
    push_ascii(&mut a, b"Hamburgefonstiv");
    if let Ok(c) = make_case(1, 0xA11CE, a) {
        out.push(c);
    }
    let mut l = Vec::new();
    push_codes(&mut l, &[0x00C0, 0x00E9, 0x00CE, 0x00F5, 0x00FC, 0x00DF]);
    if let Ok(c) = make_case(2, 0xA11CE, l) {
        out.push(c);
    }
    let mut k = Vec::new();
    push_codes(&mut k, &[0x6C38, 0x8F7D, 0x5B57, 0x4F53]);
    if let Ok(c) = make_case(3, 0xA11CE, k) {
        out.push(c);
    }
    let mut m = Vec::new();
    push_ascii(&mut m, b"Mix ");
    push_codes(&mut m, &[0x00E9, 0x6C38]);
    if let Ok(c) = make_case(4, 0xB0B, m) {
        out.push(c);
    }
    out
}

fn case_two() -> Option<VectorCase> {
    let mut t = Vec::new();
    push_ascii(&mut t, b"Ab");
    match make_case(9, 0xA11CE, t) {
        Ok(c) => Some(c),
        Err(_) => None,
    }
}

/// 9 类坏用例，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    let mut empty = VectorLedger::new(4);
    let mut t = Vec::new();
    push_ascii(&mut t, b"x");
    if let Ok(mut c) = make_case(1, 0xA11CE, t) {
        c.text.clear();
        if empty.admit(c).err() == Some(VecError::Empty) {
            caught += 1;
        }
    }
    let mut badfont = VectorLedger::new(4);
    let mut t = Vec::new();
    push_ascii(&mut t, b"x");
    if let Ok(mut c) = make_case(2, 0xA11CE, t) {
        c.font = 0;
        if badfont.admit(c).err() == Some(VecError::BadFont) {
            caught += 1;
        }
    }
    let mut badcode = VectorLedger::new(4);
    let mut t = Vec::new();
    t.push(0x0011_0000u32);
    let c = VectorCase {
        id: 3,
        font: 0xA11CE,
        text: t,
        expected: 0,
    };
    if badcode.admit(c).err() == Some(VecError::BadCode) {
        caught += 1;
    }
    let mut pua = VectorLedger::new(4);
    let mut t = Vec::new();
    t.push(0x000F_0000u32);
    let c = VectorCase {
        id: 4,
        font: 0xA11CE,
        text: t,
        expected: 0,
    };
    if pua.admit(c).err() == Some(VecError::GlyphMissing) {
        caught += 1;
    }
    let mut dup = VectorLedger::new(4);
    if let (Some(a), Some(b)) = (case_two(), case_two()) {
        if dup.admit(a).is_ok() && dup.admit(b).err() == Some(VecError::Duplicate) {
            caught += 1;
        }
    }
    let mut cap = VectorLedger::new(1);
    let mut other = case_two();
    if let Some(c) = other.as_mut() {
        c.id = 10;
    }
    if let Some(a) = case_two() {
        if cap.admit(a).is_ok() {
            if let Some(b) = other {
                if cap.admit(b).err() == Some(VecError::Cap) {
                    caught += 1;
                }
            }
        }
    }
    let mut mismatch = VectorLedger::new(4);
    if let Some(mut c) = case_two() {
        c.expected ^= 1;
        if mismatch.admit(c).err() == Some(VecError::Mismatch) {
            caught += 1;
        }
    }
    let mut sealed = VectorLedger::new(4);
    if let Some(a) = case_two() {
        if sealed.admit(a).is_ok() {
            sealed.seal();
            let mut b = case_two();
            if let Some(c) = b.as_mut() {
                c.id = 11;
            }
            if let Some(c) = b {
                if sealed.admit(c).err() == Some(VecError::Order) {
                    caught += 1;
                }
            }
        }
    }
    let mut overflow = VectorLedger::new(4);
    if let Some(mut c) = case_two() {
        c.id = u16::MAX;
        if overflow.admit(c).err() == Some(VecError::Overflow) {
            caught += 1;
        }
    }
    (caught, total)
}

/// 运行 UNX-F50095 的 16 条判据。
pub fn run_unx50095_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50095");
    let cases = standard_cases();

    let mut ledger = VectorLedger::new(8);
    let mut admitted = 0usize;
    let mut i = 0usize;
    while let Some(c) = cases.get(i) {
        if ledger.admit(c.clone()).is_ok() {
            admitted += 1;
        }
        i += 1;
    }
    let ok = admitted == 4 && ledger.len() == 4;
    let det = format!("admitted={} n={}", admitted, ledger.len());
    s.add_dyn("50095-J1-标准用例入账", ok, &det);

    let r1 = ledger.run_all();
    let r2 = ledger.run_all();
    let ok = match (&r1, &r2) {
        (Ok(a), Ok(b)) => a == b && a.cases == 4 && a.glyphs > 0,
        _ => false,
    };
    let det = format!("run1={:?} run2={:?}", r1, r2);
    s.add_dyn("50095-J1-用例可复跑", ok, &det);

    let mut replay = VectorLedger::new(8);
    let mut i = 0usize;
    while let Some(c) = cases.get(i) {
        let mut copy = c.clone();
        copy.expected = c.expected;
        if replay.admit(copy).is_err() {
            break;
        }
        i += 1;
    }
    let ok = replay.digest() == ledger.digest();
    let det = format!("d1={} d2={}", replay.digest(), ledger.digest());
    s.add_dyn("50095-J1-账本可重建", ok, &det);

    let mut fwd = Vec::new();
    push_ascii(&mut fwd, b"ABC");
    let mut rev = Vec::new();
    push_ascii(&mut rev, b"CBA");
    let ok = match (checksum(0xA11CE, &fwd), checksum(0xA11CE, &rev)) {
        (Ok(a), Ok(b)) => a != b,
        _ => false,
    };
    s.add("50095-J1-校验和顺序敏感", ok, if ok { "" } else { "顺序未进校验和" });

    let ok = match (checksum(0xA11CE, &fwd), checksum(0xB0B, &fwd)) {
        (Ok(a), Ok(b)) => a != b,
        _ => false,
    };
    s.add("50095-J1-校验和字体敏感", ok, if ok { "" } else { "字体未进校验和" });

    let mut empt = VectorLedger::new(4);
    let mut t = Vec::new();
    push_ascii(&mut t, b"x");
    let ok = match make_case(1, 0xA11CE, t) {
        Ok(mut c) => {
            c.text.clear();
            empt.admit(c).err() == Some(VecError::Empty)
        }
        Err(_) => false,
    };
    s.add("50095-J1-空文本必拒", ok, if ok { "" } else { "空文本入账" });

    let mut zero_font = VectorLedger::new(4);
    let mut t = Vec::new();
    push_ascii(&mut t, b"x");
    let ok = match make_case(2, 0xA11CE, t) {
        Ok(mut c) => {
            c.font = 0;
            zero_font.admit(c).err() == Some(VecError::BadFont)
        }
        Err(_) => false,
    };
    s.add("50095-J1-零字体必拒", ok, if ok { "" } else { "零字体入账" });

    let ok = glyph_of(0x0011_0000).err() == Some(VecError::BadCode)
        && glyph_of(0x000F_0000).err() == Some(VecError::GlyphMissing)
        && glyph_of(0x6C38).is_ok();
    s.add("50095-J1-码点两档拒", ok, if ok { "" } else { "码点分支错" });

    let mut dup = VectorLedger::new(4);
    let ok = match (case_two(), case_two()) {
        (Some(a), Some(b)) => dup.admit(a).is_ok() && dup.admit(b).err() == Some(VecError::Duplicate),
        _ => false,
    };
    s.add("50095-J1-重复id必拒", ok, if ok { "" } else { "重复 id 放行" });

    let mut cap = VectorLedger::new(1);
    let mut second = case_two();
    if let Some(c) = second.as_mut() {
        c.id = 12;
    }
    let ok = match (case_two(), second) {
        (Some(a), Some(b)) => cap.admit(a).is_ok() && cap.admit(b).err() == Some(VecError::Cap),
        _ => false,
    };
    s.add("50095-J1-容量必拒", ok, if ok { "" } else { "容量未拒" });

    let empty_ledger = VectorLedger::new(4);
    let ok = empty_ledger.run_all().err() == Some(VecError::NoCases);
    s.add("50095-J1-空账拒重放", ok, if ok { "" } else { "空账产出结果" });

    let mut overflow = VectorLedger::new(4);
    let ok = match case_two() {
        Some(mut c) => {
            c.id = u16::MAX;
            overflow.admit(c).err() == Some(VecError::Overflow)
        }
        None => false,
    };
    s.add("50095-J1-id空间耗尽", ok, if ok { "" } else { "id 空间未守" });

    let mut tampered = VectorLedger::new(4);
    let ok = match case_two() {
        Some(mut c) => {
            c.expected ^= 1;
            tampered.admit(c).err() == Some(VecError::Mismatch)
        }
        None => false,
    };
    s.add("50095-J1-期望篡改必拒", ok, if ok { "" } else { "篡改期望入账" });

    let mut sealed = VectorLedger::new(4);
    let mut seal_ok = false;
    if let Some(a) = case_two() {
        seal_ok = sealed.admit(a).is_ok();
    }
    sealed.seal();
    let mut after = case_two();
    if let Some(c) = after.as_mut() {
        c.id = 13;
    }
    let ok = seal_ok && match after {
        Some(c) => sealed.admit(c).err() == Some(VecError::Order),
        None => false,
    };
    s.add("50095-J1-封账拒写", ok, if ok { "" } else { "封账后仍写" });

    let ok = match ledger.run_all() {
        Ok(r) => {
            let mut want = 0u32;
            let mut i = 0usize;
            while let Some(c) = ledger.cases.get(i) {
                want = want.saturating_add(c.text.len() as u32);
                i += 1;
            }
            r.glyphs == want && want > 0
        }
        Err(_) => false,
    };
    s.add("50095-J1-字形数守恒", ok, if ok { "" } else { "字形计数不守恒" });

    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50095-J1-注入必抓", ok, &det);

    let all = [
        VecError::Empty,
        VecError::BadFont,
        VecError::BadCode,
        VecError::GlyphMissing,
        VecError::Duplicate,
        VecError::Cap,
        VecError::Mismatch,
        VecError::NoCases,
        VecError::Order,
        VecError::Overflow,
    ];
    let mut unique = true;
    let mut i = 0usize;
    while let Some(a) = all.get(i) {
        if a.code() < SEG_BASE + 1 || a.code() > SEG_BASE + 10 {
            unique = false;
        }
        let mut j = i + 1;
        while let Some(b) = all.get(j) {
            if a.code() == b.code() {
                unique = false;
            }
            j += 1;
        }
        i += 1;
    }
    let ok = unique;
    s.add("50095-J1-码闭集", ok, if ok { "" } else { "码值越界或重复" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50095_checks_green() {
        let set = run_unx50095_checks();
        assert!(set.all_passed(), "UNX-F50095 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50095_standard_replay() {
        let cases = standard_cases();
        let mut l = VectorLedger::new(8);
        let mut i = 0usize;
        while let Some(c) = cases.get(i) {
            match l.admit(c.clone()) {
                Ok(_) => {}
                Err(e) => assert!(false, "admit {}: {}", i, e.code()),
            }
            i += 1;
        }
        match l.run_all() {
            Ok(r) => assert_eq!(r.cases, 4),
            Err(e) => assert!(false, "run_all: {}", e.code()),
        }
    }
}
