//! VE-F4023 · 混合方向文本
//!
//! 混合方向文本渲染（RTL 段落内 LTR 片段/数字/URL）：isolate 隔离渲
//! 染（复用 F4003 isolate——单源）；混合边界（方向边界视觉清晰——边界
//! 红线：方向边界模糊=阅读错乱——边界断言）；数字与单位（数字在 RTL
//! 中方向语义——CLDR 规范）；混合断行（断行边界协同）。
//! 数据结构：isolate 复用；边界断言；数字规范；断行协同。
//! 错误路径：边界模糊→P1；数字错向→P1；隔离缺失→P0；断行错→协同。
//! 性能：isolate O(片段)；边界 O(断言)；数字 O(规范)；断行 O(协同)。
//! 跨批对接：F4003/F4021 复用单源声明；F4047 前向；N02 对端。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6980）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6980;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T29XCode(pub u16);

impl T29XCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const ISOLATE_MISS: Self = Self(SEG_BASE + 1);
    pub const EDGE_BLUR: Self = Self(SEG_BASE + 2);
    pub const NUM_WRONG: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::ISOLATE_MISS => "隔离缺失→P0",
            Self::EDGE_BLUR => "边界模糊→P1",
            Self::NUM_WRONG => "数字错向→P1",
            _ => "未知",
        }
    }
}

// ── isolate 隔离 ────────────────────────────────────────────────────

pub const LRI: char = '\u{2066}';
pub const RLI: char = '\u{2067}';
pub const FSI: char = '\u{2068}';
pub const PDI: char = '\u{2069}';

/// isolate 片段：起止码点 + 内部方向。
#[derive(Debug, Clone)]
pub struct IsolateSpan {
    pub start: usize,
    pub end: usize,
    pub inner_ltr: bool,
}

/// isolate 包裹检测：片段必须被 LRI...PDI（或 RLI/FSI...PDI）成对包
/// 裹——隔离缺失 = P0。
pub fn isolate_pairs(text: &[char]) -> Vec<IsolateSpan> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, bool)> = Vec::new();
    for (i, &c) in text.iter().enumerate() {
        match c {
            LRI => stack.push((i, true)),
            RLI | FSI => stack.push((i, false)),
            PDI => {
                if let Some((s, dir)) = stack.pop() {
                    out.push(IsolateSpan { start: s, end: i, inner_ltr: dir });
                }
            }
            _ => {}
        }
    }
    out
}

/// 隔离断言：内嵌反向片段未包裹 → P0。
pub fn isolate_assert(text: &str, has_reverse_embed: bool) -> T29XCode {
    let chars: Vec<char> = text.chars().collect();
    let pairs = isolate_pairs(&chars);
    if has_reverse_embed && pairs.is_empty() {
        return T29XCode::ISOLATE_MISS;
    }
    // 未闭合（栈残留）也判隔离缺失。
    if !pairs.is_empty() && pairs.len() * 2 < chars.iter().filter(|c| matches!(**c, LRI | RLI | FSI | PDI)).count() {
        return T29XCode::ISOLATE_MISS;
    }
    T29XCode::OK
}

// ── 边界断言 ────────────────────────────────────────────────────────

/// 方向边界：两个反向片段的接缝必须有可视边界标记。
#[derive(Debug, Clone)]
pub struct EdgeMark {
    /// 接缝位置。
    pub at: usize,
    /// 有边界标记。
    pub marked: bool,
}

/// 边界断言：接缝未标记 → P1。
pub fn edge_assert(seams: &[EdgeMark]) -> T29XCode {
    if seams.iter().any(|s| !s.marked) {
        T29XCode::EDGE_BLUR
    } else {
        T29XCode::OK
    }
}

// ── 数字与单位规范（CLDR 复述）──────────────────────────────────────

/// 数字在 RTL 中：数字串保持 LTR 方向（逻辑序显示）。
/// 单位跟随数字：km 在 RTL 中置于数字左侧（视觉）但逻辑序不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumPos {
    /// 数字串逻辑序正显。
    Logical,
    /// 数字被反转（错向）。
    Reversed,
}

/// 数字方向断言。
pub fn num_dir_assert(pos: NumPos) -> T29XCode {
    match pos {
        NumPos::Logical => T29XCode::OK,
        NumPos::Reversed => T29XCode::NUM_WRONG,
    }
}

/// 单位序：数字+单位逻辑序恒为「数字后单位」。
pub fn unit_order_assert(num_before_unit: bool) -> T29XCode {
    if num_before_unit {
        T29XCode::OK
    } else {
        T29XCode::NUM_WRONG
    }
}

// ── 混合断行 ────────────────────────────────────────────────────────

/// 断行点：位置 + 是否允许（不得在 isolate 内部断开）。
#[derive(Debug, Clone)]
pub struct BreakPoint {
    pub at: usize,
    pub allowed: bool,
    pub inside_isolate: bool,
}

/// 断行协同断言：isolate 内部断行 → 禁。
pub fn break_assert(points: &[BreakPoint]) -> T29XCode {
    if points.iter().any(|p| p.allowed && p.inside_isolate) {
        T29XCode::ISOLATE_MISS
    } else {
        T29XCode::OK
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves263_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves263_s12mixdir");

    // C4023-01 诊断码段不越窗。
    chk(&mut set, "C4023-01",
        T29XCode::NUM_WRONG.0 < SEG_BASE + 16 && SEG_BASE == 0x6980, "err");

    // C4023-02 isolate 配对检出。
    let chars: Vec<char> = "a\u{2066}123\u{2069}b".chars().collect();
    let pairs = isolate_pairs(&chars);
    chk(&mut set, "C4023-02",
        pairs.len() == 1 && pairs[0].start == 1 && pairs[0].end == 5 && pairs[0].inner_ltr, "err");

    // C4023-03 RLI 配对（RTL 内嵌）。
    let chars2: Vec<char> = "\u{2067}عربي\u{2069}".chars().collect();
    let pairs2 = isolate_pairs(&chars2);
    chk(&mut set, "C4023-03", pairs2.len() == 1 && !pairs2[0].inner_ltr, "err");

    // C4023-04 未闭合 isolate 检出。
    let chars3: Vec<char> = "\u{2066}abc".chars().collect();
    chk(&mut set, "C4023-04", isolate_pairs(&chars3).is_empty(), "err");

    // C4023-05 隔离缺失 → P0。
    chk(&mut set, "C4023-05",
        isolate_assert("عربي123", true) == T29XCode::ISOLATE_MISS, "err");

    // C4023-06 有 isolate → 过。
    chk(&mut set, "C4023-06",
        isolate_assert("عربي\u{2066}123\u{2069}", true) == T29XCode::OK, "err");

    // C4023-07 无反向内嵌 → 不需要 isolate。
    chk(&mut set, "C4023-07", isolate_assert("纯 LTR", false) == T29XCode::OK, "err");

    // C4023-08 边界断言：全标记过。
    let seams = [EdgeMark { at: 4, marked: true }, EdgeMark { at: 9, marked: true }];
    chk(&mut set, "C4023-08", edge_assert(&seams) == T29XCode::OK, "err");

    // C4023-09 边界模糊 → P1。
    let seams2 = [EdgeMark { at: 4, marked: true }, EdgeMark { at: 9, marked: false }];
    chk(&mut set, "C4023-09", edge_assert(&seams2) == T29XCode::EDGE_BLUR, "err");

    // C4023-10 数字方向：逻辑序过。
    chk(&mut set, "C4023-10", num_dir_assert(NumPos::Logical) == T29XCode::OK, "err");

    // C4023-11 数字错向 → P1。
    chk(&mut set, "C4023-11", num_dir_assert(NumPos::Reversed) == T29XCode::NUM_WRONG, "err");

    // C4023-12 单位序：数字在前过。
    chk(&mut set, "C4023-12", unit_order_assert(true) == T29XCode::OK, "err");

    // C4023-13 单位序颠倒 → P1。
    chk(&mut set, "C4023-13", unit_order_assert(false) == T29XCode::NUM_WRONG, "err");

    // C4023-14 断行：isolate 外可断。
    let bp = [
        BreakPoint { at: 2, allowed: true, inside_isolate: false },
        BreakPoint { at: 6, allowed: false, inside_isolate: true },
    ];
    chk(&mut set, "C4023-14", break_assert(&bp) == T29XCode::OK, "err");

    // C4023-15 断行：isolate 内断 → P0。
    let bp2 = [
        BreakPoint { at: 2, allowed: true, inside_isolate: false },
        BreakPoint { at: 6, allowed: true, inside_isolate: true },
    ];
    chk(&mut set, "C4023-15", break_assert(&bp2) == T29XCode::ISOLATE_MISS, "err");

    // C4023-16 四码点常量互异。
    let codes = [LRI as u32, RLI as u32, FSI as u32, PDI as u32];
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if codes[i] == codes[j] { uniq = false; }
        }
    }
    chk(&mut set, "C4023-16", uniq, "err");

    // C4023-17 URL 内嵌数字片段（isolate 用例）。
    let url_case = "راجع \u{2066}https://example.com/a?b=1\u{2069} للتفاصيل";
    chk(&mut set, "C4023-17", isolate_assert(url_case, true) == T29XCode::OK, "err");

    // C4023-18 错误码互异。
    let codes2 = [T29XCode::OK.0, T29XCode::ISOLATE_MISS.0, T29XCode::EDGE_BLUR.0, T29XCode::NUM_WRONG.0];
    chk(&mut set, "C4023-18",
        codes2[0] != codes2[1] && codes2[1] != codes2[2] && codes2[2] != codes2[3], "err");

    // C4023-19 空串 isolate 安全。
    chk(&mut set, "C4023-19",
        isolate_pairs(&Vec::new()).is_empty() && isolate_assert("", false) == T29XCode::OK, "err");

    // C4023-20 P0/P1 语义分层（隔离 P0 > 边界 P1）。
    chk(&mut set, "C4023-20",
        T29XCode::ISOLATE_MISS.explain().contains("P0")
            && T29XCode::EDGE_BLUR.explain().contains("P1"), "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolate_pairing() {
        let chars: Vec<char> = "x\u{2066}y\u{2069}z".chars().collect();
        let p = isolate_pairs(&chars);
        assert_eq!(p.len(), 1);
        assert!(p[0].inner_ltr);
    }

    #[test]
    fn missing_isolate_p0() {
        assert_eq!(isolate_assert("عربي99", true), T29XCode::ISOLATE_MISS);
    }

    #[test]
    fn break_inside_isolate_blocked() {
        let bp = [BreakPoint { at: 3, allowed: true, inside_isolate: true }];
        assert_eq!(break_assert(&bp), T29XCode::ISOLATE_MISS);
    }
}
