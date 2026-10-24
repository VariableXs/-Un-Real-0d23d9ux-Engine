//! VE-F4028 · RTL 输入法
//!
//! RTL 输入法协同（阿拉伯/希伯来输入）：RTL 光标（光标方向随语言——
//! 光标方向断言）；RTL 候选（候选窗位置——复用跟随——单源）；RTL 组
//! 合期（组合期渲染——复用组合期红线）；双向输入（LTR 文档内 RTL 输
//! 入——isolate 输入复用）。
//! 数据结构：光标断言；候选复用；组合复用；isolate 复述。
//! 错误路径：光标方向错→P1；候选错位→复用；组合期破→P0；isolate
//! 缺→复用。
//! 性能：光标 O(断言)；候选 O(复用)；组合 O(复用)；isolate O(复述)。
//! 跨批对接：F4009/F4023 复用单源声明；N03 IME 对端；F4022 镜像对端。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6BC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6BC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T34ICode(pub u16);

impl T34ICode {
    pub const OK: Self = Self(SEG_BASE);
    pub const CURSOR_WRONG: Self = Self(SEG_BASE + 1);
    pub const COMPOSE_BREAK: Self = Self(SEG_BASE + 2);
    pub const ISOLATE_MISS: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CURSOR_WRONG => "光标方向错→P1",
            Self::COMPOSE_BREAK => "组合期破→P0",
            Self::ISOLATE_MISS => "isolate 缺→复用",
            _ => "未知",
        }
    }
}

// ── RTL 光标断言 ────────────────────────────────────────────────────

/// 输入语言方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDir {
    Ltr,
    Rtl,
}

/// 光标态：语言方向 + 光标视觉移动方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    pub lang: InputDir,
    /// 前进键视觉方向（true = 视觉向左）。
    pub advance_visual_left: bool,
}

/// 光标方向断言：RTL 语言前进 = 视觉向左；LTR = 向右。
pub fn cursor_assert(st: CursorState) -> T34ICode {
    let ok = match st.lang {
        InputDir::Rtl => st.advance_visual_left,
        InputDir::Ltr => !st.advance_visual_left,
    };
    if ok {
        T34ICode::OK
    } else {
        T34ICode::CURSOR_WRONG
    }
}

// ── RTL 候选窗（复用跟随单源）───────────────────────────────────────

/// 候选窗：锚点语言方向 + 窗对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateWin {
    pub lang: InputDir,
    /// 窗右对齐（RTL 应右对齐）。
    pub right_aligned: bool,
}

/// 候选跟随断言：RTL 候选窗右对齐、LTR 左对齐。
pub fn candidate_assert(w: CandidateWin) -> T34ICode {
    let ok = match w.lang {
        InputDir::Rtl => w.right_aligned,
        InputDir::Ltr => !w.right_aligned,
    };
    if ok {
        T34ICode::OK
    } else {
        T34ICode::CURSOR_WRONG // 复用跟随通道（错位同 P1 级）
    }
}

// ── RTL 组合期（P0 红线）────────────────────────────────────────────

/// 组合期状态：组合串 + 方向 + 渲染保持。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposeState {
    pub lang: InputDir,
    /// 组合期渲染方向保持（不闪切）。
    pub stable: bool,
    /// 组合长度。
    pub len: u32,
}

/// 组合期断言：方向不稳 = P0。
pub fn compose_assert(st: ComposeState) -> T34ICode {
    if !st.stable {
        T34ICode::COMPOSE_BREAK
    } else {
        T34ICode::OK
    }
}

// ── 双向输入 isolate ────────────────────────────────────────────────

/// 输入片段：文本 + 语言方向 + 已包裹 isolate。
#[derive(Debug, Clone)]
pub struct InputSpan {
    pub text: String,
    pub dir: InputDir,
    pub isolated: bool,
}

/// 双向输入断言：LTR 文档中 RTL 输入片段须 isolate 包裹。
pub fn bidi_input_assert(spans: &[InputSpan], doc_dir: InputDir) -> T34ICode {
    for s in spans.iter() {
        if doc_dir != s.dir && !s.isolated {
            return T34ICode::ISOLATE_MISS;
        }
    }
    T34ICode::OK
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves268_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves268_s12ime");

    // C4028-01 诊断码段不越窗。
    chk(&mut set, "C4028-01",
        T34ICode::ISOLATE_MISS.0 < SEG_BASE + 16 && SEG_BASE == 0x6BC0, "err");

    // C4028-02 RTL 光标：前进视觉向左。
    chk(&mut set, "C4028-02",
        cursor_assert(CursorState { lang: InputDir::Rtl, advance_visual_left: true })
            == T34ICode::OK, "err");

    // C4028-03 RTL 光标方向错 → P1。
    chk(&mut set, "C4028-03",
        cursor_assert(CursorState { lang: InputDir::Rtl, advance_visual_left: false })
            == T34ICode::CURSOR_WRONG, "err");

    // C4028-04 LTR 光标：前进视觉向右。
    chk(&mut set, "C4028-04",
        cursor_assert(CursorState { lang: InputDir::Ltr, advance_visual_left: false })
            == T34ICode::OK, "err");

    // C4028-05 LTR 光标错 → P1。
    chk(&mut set, "C4028-05",
        cursor_assert(CursorState { lang: InputDir::Ltr, advance_visual_left: true })
            == T34ICode::CURSOR_WRONG, "err");

    // C4028-06 RTL 候选窗右对齐过。
    chk(&mut set, "C4028-06",
        candidate_assert(CandidateWin { lang: InputDir::Rtl, right_aligned: true })
            == T34ICode::OK, "err");

    // C4028-07 RTL 候选窗错位 → P1 通道。
    chk(&mut set, "C4028-07",
        candidate_assert(CandidateWin { lang: InputDir::Rtl, right_aligned: false })
            != T34ICode::OK, "err");

    // C4028-08 LTR 候选窗左对齐过。
    chk(&mut set, "C4028-08",
        candidate_assert(CandidateWin { lang: InputDir::Ltr, right_aligned: false })
            == T34ICode::OK, "err");

    // C4028-09 组合期稳定 → 过。
    chk(&mut set, "C4028-09",
        compose_assert(ComposeState { lang: InputDir::Rtl, stable: true, len: 5 })
            == T34ICode::OK, "err");

    // C4028-10 组合期方向闪切 → P0。
    chk(&mut set, "C4028-10",
        compose_assert(ComposeState { lang: InputDir::Rtl, stable: false, len: 5 })
            == T34ICode::COMPOSE_BREAK, "err");

    // C4028-11 组合期空串合法。
    chk(&mut set, "C4028-11",
        compose_assert(ComposeState { lang: InputDir::Rtl, stable: true, len: 0 })
            == T34ICode::OK, "err");

    // C4028-12 双向输入：RTL 片段已 isolate → 过。
    let spans = [
        InputSpan { text: String::from("hello"), dir: InputDir::Ltr, isolated: false },
        InputSpan { text: String::from("سلام"), dir: InputDir::Rtl, isolated: true },
    ];
    chk(&mut set, "C4028-12",
        bidi_input_assert(&spans, InputDir::Ltr) == T34ICode::OK, "err");

    // C4028-13 双向输入：未 isolate → 拦。
    let spans2 = [
        InputSpan { text: String::from("שלום"), dir: InputDir::Rtl, isolated: false },
    ];
    chk(&mut set, "C4028-13",
        bidi_input_assert(&spans2, InputDir::Ltr) == T34ICode::ISOLATE_MISS, "err");

    // C4028-14 RTL 文档中 LTR 片段须 isolate。
    let spans3 = [
        InputSpan { text: String::from("abc"), dir: InputDir::Ltr, isolated: false },
    ];
    chk(&mut set, "C4028-14",
        bidi_input_assert(&spans3, InputDir::Rtl) == T34ICode::ISOLATE_MISS, "err");

    // C4028-15 同向片段免 isolate。
    let spans4 = [
        InputSpan { text: String::from("مرحبا"), dir: InputDir::Rtl, isolated: false },
    ];
    chk(&mut set, "C4028-15",
        bidi_input_assert(&spans4, InputDir::Rtl) == T34ICode::OK, "err");

    // C4028-16 空片段集过。
    chk(&mut set, "C4028-16",
        bidi_input_assert(&[], InputDir::Ltr) == T34ICode::OK, "err");

    // C4028-17 方向枚举互异。
    chk(&mut set, "C4028-17", InputDir::Ltr != InputDir::Rtl, "err");

    // C4028-18 光标码语义（P1）。
    chk(&mut set, "C4028-18", T34ICode::CURSOR_WRONG.explain().contains("P1"), "err");

    // C4028-19 组合期码语义（P0）。
    chk(&mut set, "C4028-19",
        T34ICode::COMPOSE_BREAK.explain().contains("P0")
            && T34ICode::COMPOSE_BREAK.0 != T34ICode::CURSOR_WRONG.0, "err");

    // C4028-20 错误码互异。
    let codes = [T34ICode::OK.0, T34ICode::CURSOR_WRONG.0, T34ICode::COMPOSE_BREAK.0,
        T34ICode::ISOLATE_MISS.0];
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if codes[i] == codes[j] { uniq = false; }
        }
    }
    chk(&mut set, "C4028-20", uniq, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_follows_lang() {
        assert_eq!(cursor_assert(CursorState { lang: InputDir::Rtl, advance_visual_left: true }), T34ICode::OK);
        assert_eq!(cursor_assert(CursorState { lang: InputDir::Rtl, advance_visual_left: false }), T34ICode::CURSOR_WRONG);
    }

    #[test]
    fn compose_p0() {
        assert_eq!(compose_assert(ComposeState { lang: InputDir::Rtl, stable: false, len: 3 }), T34ICode::COMPOSE_BREAK);
    }

    #[test]
    fn bidi_input_isolate() {
        let spans = [InputSpan { text: String::from("ع"), dir: InputDir::Rtl, isolated: false }];
        assert_eq!(bidi_input_assert(&spans, InputDir::Ltr), T34ICode::ISOLATE_MISS);
    }
}
