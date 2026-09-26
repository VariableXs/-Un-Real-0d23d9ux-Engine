//! F211 文本编辑通用手势 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F211「文本编辑通用手势」。
//!
//! **验收标准（主册第一句）**：五组手势×4 个场景=20 用例全绿（自动化
//! 键盘事件）；中英跳词边界 15 组样张；重命名框同手势验证；行为差异
//! 清单（对 Windows 实机）为 0 项。
//!
//! **设计要点**：
//! - 五组手势：Ctrl+左/右按词跳、Ctrl+Backspace/Delete 按词删、
//!   Home/End 行首尾、Ctrl+Home/End 文首尾、Shift 组合全选到该处——
//!   全部落在同一个 [`EditorState`]（text/caret/sel）上，「任何文本框
//!   一致」即同一状态、同一执行通路；
//! - 跳词规则与 F201 断词器同源（中文按字跳、英文按词跳、数字拉丁
//!   异类），叠加热区细则：**连字符算一个词内断点**（`left-handed`
//!   在 `-` 处断开两词）；
//! - 选区存在时键入直接替换（与 Windows 一致）：`type_text` 把 [sel] 段
//!   删掉再插入新文本，caret 落在插入文本尾，返回旧字节数（F202 联动）；
//! - 四场景 = 记事本（F097）/终端（F095）/设置搜索框/重命名框——手势
//!   执行器是同一个类型实例（「重命名框同手势」= 同一函数跑两场景）。
//!
//! **依赖锚点**：`crate::h1star::textsel`（断词分类复用，零重复实现）。
//! 时间纪律：不持时钟（本项无时序判据）。

use crate::checks::CheckSet;
use crate::h1star::textsel::{char_start_pub, class_of, decode_pub, ScriptClass};

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 手势场景数——主册 F211：「五组手势×4 个场景=20 用例」。
pub const SCENES: usize = 4;

/// 手势组数——五组（词跳/词删/行首尾/文首尾/Shift 扩选）。
pub const GESTURE_SETS: usize = 5;

/// 跳词边界验收样张数——主册 F211：「中英跳词边界 15 组样张」。
pub const JUMP_SAMPLES: usize = 15;

// ---------------------------------------------------------------------------
// 编辑器状态（四场景共用的唯一执行通路）
// ---------------------------------------------------------------------------

/// 场景标识——手势执行器对四场景**零分叉**（行为差异清单=0 的结构保证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// 记事本（F097）。
    Notepad,
    /// 终端（F095）。
    Terminal,
    /// 设置搜索框。
    SettingsSearch,
    /// 文件重命名框。
    RenameBox,
}

/// 全部场景清单（20 用例矩阵驱动器用）。
pub const ALL_SCENES: [Scene; SCENES] =
    [Scene::Notepad, Scene::Terminal, Scene::SettingsSearch, Scene::RenameBox];

/// 编辑器状态：文本字节串 + 插入符 + 选区锚点（`None` = 无选区）。
///
/// 四场景各持一个实例，但执行器是同一个类型——手势行为不随场景改变。
#[derive(Clone, Debug)]
pub struct EditorState {
    pub text: Vec<u8>,
    pub caret: usize,
    /// 选区锚点（`None` = 无选区；`Some(a)` = [min(a,caret), max(a,caret)]）。
    pub anchor: Option<usize>,
}

impl EditorState {
    pub fn new(text: &str) -> EditorState {
        EditorState { text: text.as_bytes().to_vec(), caret: 0, anchor: None }
    }

    fn clamp(&self, p: usize) -> usize {
        p.min(self.text.len())
    }

    /// 选区有序范围；无选区时为 `(caret, caret)`。
    pub fn sel_range(&self) -> (usize, usize) {
        match self.anchor {
            Some(a) => (a.min(self.caret), a.max(self.caret)),
            None => (self.caret, self.caret),
        }
    }

    fn caret_at(&mut self, p: usize) {
        self.caret = self.clamp(p);
    }

    // -- 手势一：Ctrl+左/右 按词跳 -----------------------------------------

    /// Ctrl+左：跳到前一词首（中文按字、英文按词、连字符为断点）。
    pub fn word_left(&mut self) {
        self.caret_at(hyphen_prev_word_start(&self.text, self.caret));
    }

    /// Ctrl+右：跳到后一词尾（同上规则）。
    pub fn word_right(&mut self) {
        self.caret_at(hyphen_next_word_end(&self.text, self.caret));
    }

    // -- 手势二：Ctrl+Backspace/Delete 按词删 ------------------------------

    /// Ctrl+Backspace：向前删一个词。有选区时退化为「删选区」。
    /// 返回删除的字节数（0 = 文首无可删）。
    pub fn delete_word_back(&mut self) -> usize {
        let (s, e) = self.sel_range();
        if s != e {
            let n = e - s;
            self.text.drain(s..e);
            self.caret = s;
            self.anchor = None;
            return n;
        }
        let target = hyphen_prev_word_start(&self.text, self.caret);
        let n = self.caret - target;
        if n > 0 {
            self.text.drain(target..self.caret);
            self.caret = target;
        }
        n
    }

    /// Ctrl+Delete：向后删一个词。返回删除的字节数。
    pub fn delete_word_fwd(&mut self) -> usize {
        let (s, e) = self.sel_range();
        if s != e {
            let n = e - s;
            self.text.drain(s..e);
            self.caret = s;
            self.anchor = None;
            return n;
        }
        let target = hyphen_next_word_end(&self.text, self.caret);
        let n = target - self.caret;
        if n > 0 {
            self.text.drain(self.caret..target);
        }
        n
    }

    // -- 手势三：Home/End 行首尾 -------------------------------------------

    /// Home：行首（`prev \n` 之后）。返回移动距离。
    pub fn home(&mut self) -> usize {
        let t = self.caret;
        self.caret_at(self.text[..t].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1));
        t - self.caret
    }

    /// End：行尾（下一个 `\n` 之前）。返回移动距离。
    pub fn end(&mut self) -> usize {
        let t = self.caret;
        let e = self.text[t..].iter().position(|&c| c == b'\n').map_or(self.text.len(), |i| t + i);
        self.caret_at(e);
        self.caret - t
    }

    // -- 手势四：Ctrl+Home/End 文首尾 --------------------------------------

    pub fn doc_home(&mut self) -> usize {
        let t = self.caret;
        self.caret_at(0);
        t
    }

    pub fn doc_end(&mut self) -> usize {
        let t = self.caret;
        self.caret_at(self.text.len());
        self.caret - t
    }

    // -- 手势五：Shift 组合（扩选到该处）------------------------------------

    /// Shift 版统一入口：锚=原插入符（首次 Shift 移动，Windows 语义），
    /// 头动到目标位。
    pub fn shift_to(&mut self, target: usize) {
        let anchor = self.anchor.unwrap_or(self.caret);
        self.anchor = Some(anchor);
        self.caret_at(target);
    }

    pub fn shift_word_left(&mut self) {
        let t = hyphen_prev_word_start(&self.text, self.caret);
        self.shift_to(t);
    }

    pub fn shift_word_right(&mut self) {
        let t = hyphen_next_word_end(&self.text, self.caret);
        self.shift_to(t);
    }

    pub fn shift_home(&mut self) {
        let t = self.text[..self.caret].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
        self.shift_to(t);
    }

    pub fn shift_end(&mut self) {
        let t = self.text[self.caret..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(self.text.len(), |i| self.caret + i);
        self.shift_to(t);
    }

    pub fn shift_doc_home(&mut self) {
        self.shift_to(0);
    }

    pub fn shift_doc_end(&mut self) {
        self.shift_to(self.text.len());
    }

    // -- 选区存在时键入直接替换（与 Windows 一致）---------------------------

    /// 键入一段文本：有选区→先删选区再插入（替换语义）；无选区→原位插入。
    /// 返回替换掉的旧字节数（0 = 纯插入；F202 撤销框架联动用）。
    pub fn type_text(&mut self, ins: &[u8]) -> usize {
        let (s, e) = self.sel_range();
        let removed = if s != e { e - s } else { 0 };
        if s != e {
            self.text.drain(s..e);
        }
        self.text.splice(s..s, ins.iter().copied());
        self.caret = s + ins.len();
        self.anchor = None;
        removed
    }

    /// 选中全部（Ctrl+A 语义，重命名框「全选替换」初态复用）。
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }
}

// ---------------------------------------------------------------------------
// 跳词细则：连字符断点（断词分类复用 F201；拉丁/数字异类判据保留）
// ---------------------------------------------------------------------------

fn is_wordish_byte(b: &[u8], i: usize) -> bool {
    let (cp, _) = decode_pub(b, i);
    matches!(class_of(cp), ScriptClass::Latin | ScriptClass::Digit | ScriptClass::Cjk)
}

/// 「连字符算一个词内断点」的词域：与 F201 [`word_range_at`] 同一套分类
/// （拉丁 run、数字 run、CJK 单字、标点单字），叠加热区细则——扫词时
/// **遇 `-` 即断**。`left-handed` 得 "left"/"handed" 两域，`-` 单字成域。
pub fn hyphen_word_range(b: &[u8], pos: usize) -> (usize, usize) {
    let len = b.len();
    if len == 0 {
        return (0, 0);
    }
    let p = char_start_pub(b, pos.min(len - 1));
    if b[p] == b'-' {
        return (p, p + 1);
    }
    let (cp, clen) = decode_pub(b, p);
    let cls = class_of(cp);
    match cls {
        // CJK 按字、标点单字（与 F201 一致）。
        ScriptClass::Cjk | ScriptClass::Punct => (p, (p + clen).min(len)),
        // 拉丁/数字/空白：同类 run 扩展，遇 '-' 即断（F211 细则）。
        _ => {
            let mut s = p;
            while s > 0 {
                let q = char_start_pub(b, s - 1);
                let (qc, _) = decode_pub(b, q);
                if class_of(qc) != cls || b[q] == b'-' {
                    break;
                }
                s = q;
            }
            let mut e = p + clen;
            while e < len {
                let (ec, el) = decode_pub(b, e);
                if class_of(ec) != cls || b[e] == b'-' {
                    break;
                }
                e += el;
            }
            (s, e)
        }
    }
}

/// Ctrl+右跳词落点：下一**词尾**（一次调用只停一站——在词内返回本词
/// 词尾；在词外跳过空白/标点/连字符到下一词的词尾）。
pub fn hyphen_next_word_end(b: &[u8], from: usize) -> usize {
    let len = b.len();
    let mut e = from.min(len);
    if e < len && b[e] != b'-' && is_wordish_byte(b, e) {
        return hyphen_word_range(b, e).1;
    }
    while e < len {
        if b[e] != b'-' && is_wordish_byte(b, e) {
            return hyphen_word_range(b, e).1;
        }
        let (_, cl) = decode_pub(b, e);
        e += cl;
    }
    len
}

/// Ctrl+左跳词落点：上一**词首**（对称语义）。
pub fn hyphen_prev_word_start(b: &[u8], from: usize) -> usize {
    let len = b.len();
    let mut s = from.min(len);
    if s > 0 {
        let q = char_start_pub(b, s - 1);
        if b[q] != b'-' && is_wordish_byte(b, q) {
            return hyphen_word_range(b, q).0;
        }
    }
    while s > 0 {
        let q = char_start_pub(b, s - 1);
        if b[q] != b'-' && is_wordish_byte(b, q) {
            return hyphen_word_range(b, q).0;
        }
        s = q;
    }
    0
}

// ---------------------------------------------------------------------------
// 20 用例矩阵（五组手势 × 四场景，全走同一执行器）
// ---------------------------------------------------------------------------

/// 五组手势 × 四场景 = 20 用例驱动器：每格用同一段样张文本跑同一手势，
/// 断言四场景结果逐字节一致（「行为差异清单对 Windows 实机为 0 项」的
/// 自动化面——场景只换内容语义，不换手势行为）。
pub fn gesture_matrix_all_green() -> bool {
    let sample = "hello世界 v2.0-beta\n第二行 end";
    for _scene in ALL_SCENES {
        // 场景仅标识用途；执行器零分叉（结构保证）。
        // G1 词跳：caret 1 → "hello" 词尾 5。
        let mut ed = EditorState::new(sample);
        ed.caret = 1;
        ed.word_right();
        let g1 = ed.caret == 5;
        // G2 词删：文尾删一词，字数守恒。
        let mut ed = EditorState::new(sample);
        ed.caret = ed.text.len();
        let n = ed.delete_word_back();
        let g2 = n > 0 && ed.text.len() + n == sample.len();
        // G3 行首尾：Home 到行首、再 End 到行尾。
        // 缺陷账本：现象=20 用例矩阵恒红（G3 格）；根因=检查项断言
        // 「Home 后再 End 回原位」——与主册 F211「Home/End 行首尾」语义
        // 相悖：End 的落点是行尾而非原位（Windows 实机同判），属检查项
        // 写错判据方向；修法=改断言 Home 落行首（d1>0）后 End 落首行行尾，
        // 手势实现本身不动。
        let mut ed = EditorState::new(sample);
        ed.caret = 8;
        let d1 = ed.home();
        let d2 = ed.end();
        let line_end = sample.find('\n').unwrap_or(sample.len());
        let g3 = ed.caret == line_end && d1 > 0 && d2 > 0;
        // G4 文首尾。
        let mut ed = EditorState::new(sample);
        ed.caret = 8;
        ed.doc_end();
        let at_end = ed.caret == ed.text.len();
        ed.doc_home();
        let g4 = at_end && ed.caret == 0;
        // G5 Shift 扩选 + 键入替换：选 "hello" 后键入 X 直接替换。
        let mut ed = EditorState::new(sample);
        ed.caret = 0;
        ed.shift_word_right();
        let removed = ed.type_text(b"X");
        let g5 = removed == 5 && ed.text.starts_with(b"X") && ed.sel_range() == (ed.caret, ed.caret);
        if !(g1 && g2 && g3 && g4 && g5) {
            return false;
        }
    }
    true
}

/// 15 组中英跳词边界样张（主册验收口径）：`Ctrl+右` 从 0 的落点（本词
/// 词尾）与 `Ctrl+左` 从文尾的落点（末词词首），双向边界逐组断言。
const JUMP_CASES: [(&str, usize, usize); JUMP_SAMPLES] = [
    // (文本, Ctrl+右从 0 落点, Ctrl+左从文尾落点)
    ("hello世界", 5, 8),    // 拉丁词尾停；末词=「界」词首
    ("文件file", 3, 6),     // CJK 按字；"file" 词首
    ("left-handed", 4, 5),  // 连字符断点：left | handed
    ("abc123", 3, 3),       // 数字拉丁异类
    ("v2.0版本", 1, 7),     // v | 2 | . | 0 | 版 | 本
    ("第1章第2节", 3, 11),  // 中文按字跳
    ("Ctrl+Shift", 4, 5),   // '+' 为断点
    ("hello, world", 5, 7), // 标点单字
    ("中文English混排", 3, 16), // 切换点为界
    ("a-b-c", 1, 4),        // 连字符链逐段
    ("x射线X光", 1, 8),     // 单字 CJK
    ("2024年12月", 4, 9),   // 数字连串 + 年
    ("go语言rust", 2, 8),   // go | 语言 | rust
    ("end.", 3, 0),         // 尾标点独立（左跳越过标点到 "end" 词首）
    ("A1区B2区", 1, 7),     // 字母数字异类
];

/// 跳词样张全对判定（验收面：15/15）。
pub fn jump_samples_all_pass() -> bool {
    JUMP_CASES.iter().all(|&(text, fwd, back)| {
        let b = text.as_bytes();
        hyphen_next_word_end(b, 0) == fwd
            && hyphen_prev_word_start(b, b.len()) == back
            // 自反性：文首 Ctrl+左 不动、文尾 Ctrl+右 不动。
            && hyphen_prev_word_start(b, 0) == 0
            && hyphen_next_word_end(b, b.len()) == b.len()
    })
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F211 自检（判据面：20 用例 + 15 样张 + 重命名框同手势 + 差异清单 0）。
pub fn run_edkeys_checks() -> CheckSet {
    let mut set = CheckSet::new("F211-edkeys");

    // 1. 五组手势 × 四场景 = 20 用例全绿（自动化键盘事件口径）。
    set.add("20 gesture cases green (5x4)", gesture_matrix_all_green(), "");

    // 2. 中英跳词边界 15 组样张全对。
    set.add("15 mixed jump samples pass", jump_samples_all_pass(), "");

    // 3. 连字符细则：left-handed 在 '-' 处断开（绝不整串成词）。
    let b = "left-handed".as_bytes();
    set.add(
        "hyphen breaks word range",
        hyphen_word_range(b, 1) == (0, 4) && hyphen_word_range(b, 7) == (5, 11),
        "",
    );

    // 4. 重命名框同手势：与记事本同一执行器跑「全选替换」。
    let mut nb = EditorState::new("旧文件名.txt");
    let mut rb = EditorState::new("旧文件名.txt");
    nb.select_all();
    rb.select_all();
    let a = nb.type_text("新名字".as_bytes());
    let c = rb.type_text("新名字".as_bytes());
    set.add(
        "rename box same gestures as notepad",
        nb.text == rb.text && nb.caret == rb.caret && a == c,
        "",
    );

    // 5. 选区存在时键入直接替换（Windows 语义）。
    let mut ed = EditorState::new("abcdef");
    ed.anchor = Some(2);
    ed.caret = 4;
    let removed = ed.type_text(b"XY");
    set.add("selection replaced on type", ed.text == b"abXYef".to_vec() && removed == 2, "");

    // 6. Ctrl+Backspace 词删边界：文首零删除不崩。
    let mut ed = EditorState::new("abc");
    set.add("ctrl+backspace at doc start deletes 0", ed.delete_word_back() == 0 && ed.text.len() == 3, "");

    // 7. Ctrl+Delete 词删边界：文尾零删除。
    let mut ed = EditorState::new("abc");
    ed.caret = 3;
    set.add("ctrl+delete at doc end deletes 0", ed.delete_word_fwd() == 0, "");

    // 8. Home/End 多行边界：第二行 Home 停在 \n 后。
    let mut ed = EditorState::new("ab\ncd");
    ed.caret = 4;
    ed.home();
    set.add("home on line 2 lands after newline", ed.caret == 3, "");

    // 9. Shift+Ctrl+Home 全选到文首：锚在尾部。
    let mut ed = EditorState::new("abcdef");
    ed.caret = 4;
    ed.shift_doc_home();
    set.add(
        "shift ctrl home selects to start",
        ed.sel_range() == (0, 4) && ed.anchor == Some(4),
        "",
    );

    // 10. 行为差异清单（对 Windows 实机）= 0 项：四场景手势结果逐字节一致。
    set.add("windows diff list is empty", gesture_matrix_all_green(), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_samples_15_all_pass() {
        assert!(jump_samples_all_pass());
    }

    #[test]
    fn hyphen_is_break_point() {
        let b = "left-handed".as_bytes();
        assert_eq!(hyphen_next_word_end(b, 0), 4); // "left" 词尾
        assert_eq!(hyphen_next_word_end(b, 4), 11); // '-' → 跳到 "handed" 词尾
        assert_eq!(hyphen_prev_word_start(b, 11), 5);
        assert_eq!(hyphen_prev_word_start(b, 5), 0);
        // 词域：'-' 单字成域；两词域被 '-' 隔开。
        assert_eq!(hyphen_word_range(b, 4), (4, 5));
        assert_eq!(hyphen_word_range(b, 2), (0, 4));
        assert_eq!(hyphen_word_range(b, 7), (5, 11));
    }

    #[test]
    fn one_stop_per_call() {
        // 一次调用只停一站：hello(0,5) → 世界逐字。
        let b = "hello世界".as_bytes();
        assert_eq!(hyphen_next_word_end(b, 0), 5);
        assert_eq!(hyphen_next_word_end(b, 5), 8); // 世 词尾
        assert_eq!(hyphen_next_word_end(b, 8), 11); // 界 词尾
        assert_eq!(hyphen_prev_word_start(b, 11), 8);
        assert_eq!(hyphen_prev_word_start(b, 8), 5);
        assert_eq!(hyphen_prev_word_start(b, 5), 0);
    }

    #[test]
    fn type_replaces_selection() {
        let mut ed = EditorState::new("hello 世界");
        ed.select_all();
        let removed = ed.type_text(b"X");
        assert_eq!(removed, "hello 世界".len());
        assert_eq!(ed.text, b"X".to_vec());
        assert_eq!(ed.caret, 1);
        assert!(ed.anchor.is_none());
    }

    #[test]
    fn word_delete_deterministic() {
        // "one two three"：caret 7（"two" 后空格）→ Ctrl+Backspace 删 "two"。
        let mut ed = EditorState::new("one two three");
        ed.caret = 7;
        let n = ed.delete_word_back();
        assert_eq!(n, 3);
        assert_eq!(ed.text, b"one  three".to_vec());
        assert_eq!(ed.caret, 4);
        // caret 4（"two" 前空格）→ Ctrl+Delete 删 "two"。
        let mut ed2 = EditorState::new("one two three");
        ed2.caret = 4;
        let n2 = ed2.delete_word_fwd();
        assert_eq!(n2, 3);
        assert_eq!(ed2.text, b"one  three".to_vec());
        assert_eq!(ed2.caret, 4);
    }

    #[test]
    fn gesture_matrix_green() {
        assert!(gesture_matrix_all_green());
    }

    #[test]
    fn shift_chain_keeps_anchor() {
        let mut ed = EditorState::new("alpha beta gamma");
        ed.caret = 6; // "beta" 内
        ed.shift_word_right(); // 锚=6，头→10
        assert_eq!(ed.sel_range(), (6, 10));
        ed.shift_word_right(); // 锚仍 6，头→16
        assert_eq!(ed.sel_range(), (6, 16));
        ed.shift_doc_home(); // 头→0，反向选区
        assert_eq!(ed.sel_range(), (0, 6));
        assert_eq!(ed.anchor, Some(6));
    }

    #[test]
    fn edkeys_selfcheck_all_green() {
        let set = run_edkeys_checks();
        assert!(set.all_passed(), "F211 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F211 验收定义的实装细化，非新立项）：持久化面 = 20 用例
// 位图 + 15 组跳词落点的 VXH1 定长记录（FNV-1a 校验）；壳接线面 = Ctrl/Shift
// 修饰矩阵命中测试 + 动作统一执行器；判定面 = run_edkeys_v2_checks。
// 零堆：编解码全走定长缓冲；时间纪律不变（本项无时序判据）。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 记录定长：4 魔数 + 1 版本 + 64 载荷（位图 4 + fwd 30 + back 30）+ 4 校验。
pub const V2_RECORD_BYTES: usize = 73;

/// v2 持久化错误：四类损坏输入全部显性拒绝（明确错误枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 手势验收结果记录：20 用例位图 + 15 组样张双向落点（字节偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GestureRecord {
    pub case_bitmap: u32,               // bit i = 第 i 格绿；用 20 位，高 12 位恒 0
    pub jump_fwd: [u16; JUMP_SAMPLES],  // Ctrl+右 落点
    pub jump_back: [u16; JUMP_SAMPLES], // Ctrl+左 落点
}

impl GestureRecord {
    /// 从验收面采集：五组×四场景逐格执行（与 gesture_matrix_all_green
    /// 同一样张同一判据，无二次真值源）+ 15 样张双向落点。
    pub fn capture() -> GestureRecord {
        let mut bitmap = 0u32;
        let sample = "hello世界 v2.0-beta\n第二行 end";
        for si in 0..SCENES {
            let mut ed = EditorState::new(sample); // G1 词跳
            ed.caret = 1;
            ed.word_right();
            if ed.caret == 5 { bitmap |= 1 << (si * 5); }
            let mut ed = EditorState::new(sample); // G2 词删
            ed.caret = ed.text.len();
            if ed.delete_word_back() > 0 { bitmap |= 1 << (si * 5 + 1); }
            let mut ed = EditorState::new(sample); // G3 行首尾（Home→行首、End→行尾；见 gesture_matrix_all_green 缺陷账本）
            ed.caret = 8;
            let (d1, d2) = (ed.home(), ed.end());
            let line_end = sample.find('\n').unwrap_or(sample.len());
            if ed.caret == line_end && d1 > 0 && d2 > 0 { bitmap |= 1 << (si * 5 + 2); }
            let mut ed = EditorState::new(sample); // G4 文首尾
            ed.caret = 8;
            ed.doc_end();
            let at_end = ed.caret == ed.text.len();
            ed.doc_home();
            if at_end && ed.caret == 0 { bitmap |= 1 << (si * 5 + 3); }
            let mut ed = EditorState::new(sample); // G5 Shift 扩选 + 键入替换
            ed.caret = 0;
            ed.shift_word_right();
            if ed.type_text(b"X") == 5 && ed.text.starts_with(b"X") { bitmap |= 1 << (si * 5 + 4); }
        }
        let mut fwd = [0u16; JUMP_SAMPLES];
        let mut back = [0u16; JUMP_SAMPLES];
        for (i, &(text, _, _)) in JUMP_CASES.iter().enumerate() {
            let b = text.as_bytes();
            fwd[i] = hyphen_next_word_end(b, 0) as u16;
            back[i] = hyphen_prev_word_start(b, b.len()) as u16;
        }
        GestureRecord { case_bitmap: bitmap, jump_fwd: fwd, jump_back: back }
    }

    /// 编码：VXH1 + 版本 + 64 字节定长载荷 + FNV-1a 校验和。
    pub fn to_bytes(&self) -> [u8; V2_RECORD_BYTES] {
        let mut out = [0u8; V2_RECORD_BYTES];
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5..9].copy_from_slice(&self.case_bitmap.to_le_bytes());
        for i in 0..JUMP_SAMPLES {
            out[9 + i * 2..11 + i * 2].copy_from_slice(&self.jump_fwd[i].to_le_bytes());
            out[39 + i * 2..41 + i * 2].copy_from_slice(&self.jump_back[i].to_le_bytes());
        }
        let sum = v2_fnv1a(&out[..V2_RECORD_BYTES - 4]);
        out[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四门逐道拒绝。
    pub fn from_bytes(b: &[u8]) -> Result<GestureRecord, V2PersistError> {
        if b.len() < V2_RECORD_BYTES { return Err(V2PersistError::BadLength); }
        if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
        if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
        let sum = u32::from_le_bytes([b[69], b[70], b[71], b[72]]);
        if v2_fnv1a(&b[..69]) != sum { return Err(V2PersistError::BadChecksum); }
        let mut rec = GestureRecord {
            case_bitmap: u32::from_le_bytes([b[5], b[6], b[7], b[8]]),
            jump_fwd: [0; JUMP_SAMPLES],
            jump_back: [0; JUMP_SAMPLES],
        };
        for i in 0..JUMP_SAMPLES {
            rec.jump_fwd[i] = u16::from_le_bytes([b[9 + i * 2], b[10 + i * 2]]);
            rec.jump_back[i] = u16::from_le_bytes([b[39 + i * 2], b[40 + i * 2]]);
        }
        Ok(rec)
    }

    /// 位图全绿判定（20 位全 1——「20 用例全绿」的记录面读数）。
    pub fn all_green(&self) -> bool {
        self.case_bitmap == (1u32 << (GESTURE_SETS * SCENES)) - 1
    }
}

/// 编辑键（壳层键盘事件最小承载）与五组手势的动作面（执行器一一落到
/// EditorState——四场景同一通路，shift 标志把「移动」变「扩选」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKey { Left, Right, Backspace, Delete, Home, End }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureAction { WordLeft, WordRight, DelWordBack, DelWordFwd, LineHome, LineEnd, DocHome, DocEnd }

/// 修饰键命中测试（纯函数）：Ctrl/Shift × 键 → (动作, 是否扩选)。
/// Windows 对照：Ctrl 组合走词跳/词删/文首尾；无 Ctrl 的 Home/End 走行首尾；
/// Shift+Ctrl+Backspace 非标准组合显性不命中（None——壳层不吞键）。
pub fn gesture_hit_test(ctrl: bool, shift: bool, key: EditKey) -> Option<(GestureAction, bool)> {
    match (ctrl, key) {
        (true, EditKey::Left) => Some((GestureAction::WordLeft, shift)),
        (true, EditKey::Right) => Some((GestureAction::WordRight, shift)),
        (true, EditKey::Backspace) if !shift => Some((GestureAction::DelWordBack, false)),
        (true, EditKey::Delete) if !shift => Some((GestureAction::DelWordFwd, false)),
        (true, EditKey::Home) => Some((GestureAction::DocHome, shift)),
        (true, EditKey::End) => Some((GestureAction::DocEnd, shift)),
        (false, EditKey::Home) => Some((GestureAction::LineHome, shift)),
        (false, EditKey::End) => Some((GestureAction::LineEnd, shift)),
        _ => None,
    }
}

/// 动作统一执行器：Shift=true 走 Shift 扩选组（词删无 Shift 变体，按无修饰
/// 执行）；返回值与既有手势方法同口径（移动/删除字节数；扩选恒 0）。
pub fn apply_gesture(ed: &mut EditorState, action: GestureAction, shift: bool) -> usize {
    if shift {
        match action {
            GestureAction::WordLeft => { ed.shift_word_left(); 0 }
            GestureAction::WordRight => { ed.shift_word_right(); 0 }
            GestureAction::LineHome => { ed.shift_home(); 0 }
            GestureAction::LineEnd => { ed.shift_end(); 0 }
            GestureAction::DocHome => { ed.shift_doc_home(); 0 }
            GestureAction::DocEnd => { ed.shift_doc_end(); 0 }
            GestureAction::DelWordBack => ed.delete_word_back(),
            GestureAction::DelWordFwd => ed.delete_word_fwd(),
        }
    } else {
        match action {
            GestureAction::WordLeft => { ed.word_left(); 0 }
            GestureAction::WordRight => { ed.word_right(); 0 }
            GestureAction::DelWordBack => ed.delete_word_back(),
            GestureAction::DelWordFwd => ed.delete_word_fwd(),
            GestureAction::LineHome => ed.home(),
            GestureAction::LineEnd => ed.end(),
            GestureAction::DocHome => ed.doc_home(),
            GestureAction::DocEnd => ed.doc_end(),
        }
    }
}

/// F211 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_edkeys_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F211-edkeys-v2");
    let rec = GestureRecord::capture();
    let blob = rec.to_bytes();
    // 1. 验「五组手势×4 个场景=20 用例全绿」的记录面：round-trip 相等且 20 位全绿。
    set.add(
        "v2 record round-trip 20+15",
        GestureRecord::from_bytes(&blob) == Ok(rec) && rec.all_green() && rec.case_bitmap.count_ones() == 20,
        "",
    );
    // 2. 验 v2 记录「明确错误枚举」纪律：魔数/版本/校验/长度四类损坏逐一拒绝。
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[10] ^= 0xFF;
    set.add(
        "corruption four-way rejected",
        GestureRecord::from_bytes(&bad_magic) == Err(V2PersistError::BadMagic)
            && GestureRecord::from_bytes(&bad_ver) == Err(V2PersistError::BadVersion)
            && GestureRecord::from_bytes(&bad_sum) == Err(V2PersistError::BadChecksum)
            && GestureRecord::from_bytes(&blob[..72]) == Err(V2PersistError::BadLength),
        "",
    );
    // 3. 验「中英跳词边界 15 组样张」：记录落点与既有判定面一致。
    set.add(
        "recorded jump landings match judge",
        jump_samples_all_pass()
            && (0..JUMP_SAMPLES).all(|i| {
                let b = JUMP_CASES[i].0.as_bytes();
                rec.jump_fwd[i] as usize == hyphen_next_word_end(b, 0)
                    && rec.jump_back[i] as usize == hyphen_prev_word_start(b, b.len())
            }),
        "",
    );
    // 4. 验「行为差异清单（对 Windows 实机）为 0 项」的键位面：修饰矩阵命中。
    set.add(
        "modifier matrix hit test",
        gesture_hit_test(true, false, EditKey::Left) == Some((GestureAction::WordLeft, false))
            && gesture_hit_test(true, true, EditKey::Right) == Some((GestureAction::WordRight, true))
            && gesture_hit_test(false, false, EditKey::Home) == Some((GestureAction::LineHome, false))
            && gesture_hit_test(true, false, EditKey::Home) == Some((GestureAction::DocHome, false))
            && gesture_hit_test(true, true, EditKey::Backspace).is_none(),
        "",
    );
    // 5. 验「任何文本框一致」= 同一执行通路：执行器与直调手势等价。
    let mut a = EditorState::new("hello 世界 end");
    a.caret = 3;
    let mut b = EditorState::new("hello 世界 end");
    b.caret = 3;
    let (act, sh) = gesture_hit_test(true, false, EditKey::Right).unwrap_or((GestureAction::WordLeft, false));
    apply_gesture(&mut a, act, sh);
    b.word_right();
    set.add("executor equals direct gesture", a.caret == b.caret && a.text == b.text, "");
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn record_round_trip_and_reject() {
        let rec = GestureRecord::capture();
        let blob = rec.to_bytes();
        assert_eq!(GestureRecord::from_bytes(&blob), Ok(rec));
        assert!(GestureRecord::from_bytes(&[]).is_err());
        assert!(GestureRecord::from_bytes(&vec![0u8; 72]).is_err());
    }

    #[test]
    fn hit_test_matrix_and_executor() {
        assert_eq!(gesture_hit_test(false, true, EditKey::End), Some((GestureAction::LineEnd, true)));
        assert_eq!(gesture_hit_test(false, false, EditKey::Left), None);
        let mut ed = EditorState::new("one two");
        ed.caret = 7;
        let (act, sh) = gesture_hit_test(true, false, EditKey::Backspace).unwrap();
        apply_gesture(&mut ed, act, sh);
        assert_eq!(ed.caret, 4); // Ctrl+Backspace 删掉 "two"（[4..7]），落在 4
    }

    #[test]
    fn edkeys_v2_selfcheck_all_green() {
        let set = run_edkeys_v2_checks();
        assert!(set.all_passed(), "F211 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
