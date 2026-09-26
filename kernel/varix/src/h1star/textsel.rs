//! F201 文本选择与光标规范 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F201「文本选择与光标规范」。
//!
//! **验收标准（主册第一句）**：五语义在 3 个不同应用内行为一致；混排断词
//! 正确率 100%（20 组中英混排样张）；插入符深浅底对比度均 ≥4.5:1
//! （用 `contrast_ratio_100` 判定）。
//!
//! **设计要点**：
//! - 点击分类状态机：单击/双击/三击，间隔阈值 500ms、原点位移容差 4px，
//!   双击三击沿同链升档、超窗或移位回落单击；
//! - 五语义选择器：双击选词、三击选段、Shift+单击扩选、拖拽逐字符选、
//!   Shift+双击按词扩选——全部落在同一个 `SelState`（anchor/head）上，
//!   「全系统统一」即同一状态、同一渲染通路；
//! - 混排断词器：按 UTF-8 码点分类 CJK/拉丁/数字/标点/空白五类，
//!   CJK 单字成词（中文按字断）、拉丁/数字连串成词、标点单字独立——
//!   语言切换点天然是类边界，「hello世界」绝不会被当一个词整选；
//! - 插入符：粗 2px，色随主题令牌、深浅底自动反色——深/浅两枚令牌
//!   取对比度高者，且必须过 ≥4.5:1 门才算合格。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）；
//! `crate::h1star::h1base::{Rgb8, contrast_ratio_100}`（对比度判定）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::{contrast_ratio_100, Rgb8};

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 多击判定间隔阈值——主册 F201：「单击/双击/三击，间隔阈值 500ms」。
pub const MULTI_CLICK_MS: u64 = 500;

/// 多击原点位移容差（px）：超过视为新点击序列（实装定值，主册只定间隔）。
pub const MULTI_CLICK_SLOP_PX: i32 = 4;

/// 插入符宽度——主册 F201：「插入符粗 2px」。
pub const CARET_W_PX: i32 = 2;

/// 插入符对比度门——主册 F201：「深浅底对比度均 ≥4.5:1」（×100 定点 = 450）。
pub const CARET_MIN_CONTRAST: u32 = 450;

/// 混排断词验收样张数——主册 F201：「混排断词正确率 100%（20 组中英混排样张）」。
pub const MIXED_SAMPLES: usize = 20;

// ---------------------------------------------------------------------------
// 点击分类状态机
// ---------------------------------------------------------------------------

/// 一次按压的分类结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind {
    Single,
    Double,
    Triple,
}

/// 点击分类状态机：500ms 内且位移 ≤4px 的连续按压沿 单击→双击→三击 升档，
/// 三击封顶（四击回落双击——三段语义已覆盖主册五选择语义的触发面）。
#[derive(Clone, Copy)]
pub struct ClickClassifier {
    last_ts: u64,
    last_x: i32,
    last_y: i32,
    streak: u8,
}

impl ClickClassifier {
    pub fn new() -> ClickClassifier {
        ClickClassifier { last_ts: 0, last_x: 0, last_y: 0, streak: 0 }
    }

    /// 注入一次按压（时间戳与位置均由调用方注入）。返回本次分类。
    pub fn feed(&mut self, ts: u64, x: i32, y: i32) -> ClickKind {
        let near = (x - self.last_x).abs() <= MULTI_CLICK_SLOP_PX
            && (y - self.last_y).abs() <= MULTI_CLICK_SLOP_PX;
        let in_win = ts >= self.last_ts && ts - self.last_ts <= MULTI_CLICK_MS;
        self.streak = if self.streak > 0 && near && in_win { (self.streak + 1).min(3) } else { 1 };
        self.last_ts = ts;
        self.last_x = x;
        self.last_y = y;
        match self.streak {
            3 => ClickKind::Triple,
            2 => ClickKind::Double,
            _ => ClickKind::Single,
        }
    }

    pub fn streak(&self) -> u8 {
        self.streak
    }
}

impl Default for ClickClassifier {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 混排断词器（UTF-8 码点分类）
// ---------------------------------------------------------------------------

/// 码点类别——断词的最小语义单元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptClass {
    /// CJK 统一表意文字（中文按字断）。
    Cjk,
    /// 拉丁字母（英文按词断）。
    Latin,
    /// 阿拉伯数字连串成词。
    Digit,
    /// 标点（单字独立）。
    Punct,
    /// 空白（空格/制表/换行）。
    Space,
}

/// 解码 `i` 处（必须是码点首字节）的 UTF-8 码点，返回 (码点, 字节长)。
/// 输入约定为合法 UTF-8（内核文本面）；截断序列按已有字节尽力解码。
fn decode_at(b: &[u8], i: usize) -> (u32, usize) {
    let c0 = b[i];
    if c0 < 0x80 {
        return (c0 as u32, 1);
    }
    let len = if c0 >= 0xF0 {
        4
    } else if c0 >= 0xE0 {
        3
    } else {
        2
    };
    let mut cp: u32 = match len {
        2 => (c0 & 0x1F) as u32,
        3 => (c0 & 0x0F) as u32,
        _ => (c0 & 0x07) as u32,
    };
    let mut k = 1usize;
    while k < len && i + k < b.len() {
        cp = (cp << 6) | (b[i + k] & 0x3F) as u32;
        k += 1;
    }
    (cp, len.min(b.len() - i))
}

/// `i` 处若是多字节序列的后续字节，回退到该序列首字节。
fn char_start(b: &[u8], i: usize) -> usize {
    let mut j = i.min(b.len());
    while j > 0 && (b[j] & 0xC0) == 0x80 {
        j -= 1;
    }
    j
}

/// [`decode_at`] 的公开包装（F211 跳词细则复用同一解码器——一处一事实）。
pub fn decode_pub(b: &[u8], i: usize) -> (u32, usize) {
    decode_at(b, i)
}

/// [`char_start`] 的公开包装（F211 码点首字节对齐复用）。
pub fn char_start_pub(b: &[u8], i: usize) -> usize {
    char_start(b, i)
}

/// 码点类别判定（主册断词规则的唯一实现点）：
/// 中文按字断、英文按词断、数字连串、标点独立、空白连片。
pub fn class_of(cp: u32) -> ScriptClass {
    match cp {
        0x30..=0x39 => ScriptClass::Digit,
        0x41..=0x5A | 0x61..=0x7A => ScriptClass::Latin,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF => ScriptClass::Cjk,
        0x20 | 0x09 | 0x0A | 0x0D => ScriptClass::Space,
        _ => ScriptClass::Punct,
    }
}

fn class_at(b: &[u8], i: usize) -> ScriptClass {
    let (cp, _) = decode_at(b, i);
    class_of(cp)
}

fn is_wordish(c: ScriptClass) -> bool {
    matches!(c, ScriptClass::Latin | ScriptClass::Digit | ScriptClass::Cjk)
}

/// 包含字节偏移 `pos` 的词的范围 `(start, end)`。
///
/// 规则（主册 F201 断词语义）：拉丁/数字向两侧扩到同类 run 边界；
/// CJK 与标点单字成词——「hello世界」在 'o' 处选得 "hello"，
/// 在 '世' 处只选 "世"，绝不全串整选。
pub fn word_range_at(b: &[u8], pos: usize) -> (usize, usize) {
    let len = b.len();
    if len == 0 {
        return (0, 0);
    }
    let p = char_start(b, pos.min(len - 1));
    let (cp, clen) = decode_at(b, p);
    let cls = class_of(cp);
    match cls {
        ScriptClass::Cjk | ScriptClass::Punct => (p, (p + clen).min(len)),
        _ => {
            let mut s = p;
            while s > 0 {
                let q = char_start(b, s - 1);
                if class_at(b, q) != cls {
                    break;
                }
                s = q;
            }
            let mut e = p + clen;
            while e < len {
                if class_at(b, e) != cls {
                    break;
                }
                let (_, nl) = decode_at(b, e);
                e += nl;
            }
            (s, e)
        }
    }
}

/// 从 `from` 向后找下一个词尾（Shift+双击向右按词扩选用）。
/// 已在词内 → 本词词尾；在空白/标点上 → 跳过后的下一词词尾。
pub fn next_word_end(b: &[u8], from: usize) -> usize {
    let len = b.len();
    if from >= len {
        return len;
    }
    // 缺陷账本：现象=Shift+双击在 CJK 字节中部扩选落点错位（rw.head=11≠8）；
    // 根因=`from` 未做码点首字节对齐，落在多字节序列中间时按续字节解码、
    // 类别误判为标点，词域判定失真；修法=与 word_range_at 入口同规——先
    // char_start 对齐再判类（word_range_at 本就如此对齐，一处一事实）。
    let mut e = char_start(b, from);
    if e < len && is_wordish(class_at(b, e)) {
        return word_range_at(b, e).1;
    }
    while e < len {
        let (_, cl) = decode_at(b, e);
        e += cl;
        if e < len && is_wordish(class_at(b, e)) {
            return word_range_at(b, e).1;
        }
    }
    len
}

/// 从 `from` 向前找上一个词首（Shift+双击向左按词扩选用）。
pub fn prev_word_start(b: &[u8], from: usize) -> usize {
    let len = b.len();
    let mut s = from.min(len);
    if s > 0 {
        let q = char_start(b, s - 1);
        if is_wordish(class_at(b, q)) {
            return word_range_at(b, q).0;
        }
    }
    while s > 0 {
        let q = char_start(b, s - 1);
        if is_wordish(class_at(b, q)) {
            return word_range_at(b, q).0;
        }
        s = q;
    }
    0
}

/// 包含 `pos` 的段落范围（三击选段）：以 `\n` 为段界，前后开区间收缩。
pub fn paragraph_range_at(b: &[u8], pos: usize) -> (usize, usize) {
    let len = b.len();
    let p = pos.min(len);
    let start = b[..p].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
    let end = b[p..].iter().position(|&c| c == b'\n').map_or(len, |i| p + i);
    (start, end)
}

// ---------------------------------------------------------------------------
// 五语义选择器（统一落在 SelState 上）
// ---------------------------------------------------------------------------

/// 统一选择状态：anchor（锚点）与 head（活动端）的字节偏移。
/// 五种选择语义都只产出这一种状态——全系统统一的落点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelState {
    pub anchor: usize,
    pub head: usize,
}

impl SelState {
    /// 有序化后的范围 `(lo, hi)`（渲染与取文用）。
    pub fn ordered(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// 语义一：拖拽逐字符选（按下点为锚，随拖动头部逐字符跟随）。
pub fn sel_drag(anchor: usize, head: usize) -> SelState {
    SelState { anchor, head }
}

/// 语义二：双击选词。
pub fn sel_word(b: &[u8], pos: usize) -> SelState {
    let (s, e) = word_range_at(b, pos);
    SelState { anchor: s, head: e }
}

/// 语义三：三击选段。
pub fn sel_paragraph(b: &[u8], pos: usize) -> SelState {
    let (s, e) = paragraph_range_at(b, pos);
    SelState { anchor: s, head: e }
}

/// 语义四：Shift+单击扩选（锚不动，头部移到点击处）。
pub fn sel_extend(prev: SelState, pos: usize) -> SelState {
    SelState { anchor: prev.anchor, head: pos }
}

/// 语义五：Shift+双击按词扩选（锚不动，头部按词跳）。
pub fn sel_extend_word(b: &[u8], prev: SelState, pos: usize) -> SelState {
    let head = if pos >= prev.head {
        next_word_end(b, pos)
    } else {
        prev_word_start(b, pos)
    };
    SelState { anchor: prev.anchor, head }
}

// ---------------------------------------------------------------------------
// 插入符（2px · 主题令牌 · 深浅底自动反色）
// ---------------------------------------------------------------------------

/// 插入符主题令牌对：浅底用字色 / 深底用字色各一枚。
#[derive(Clone, Copy)]
pub struct CaretTokens {
    pub on_light: Rgb8,
    pub on_dark: Rgb8,
}

/// 深浅底自动反色：两枚令牌取对当前背景对比度高者。
/// 「随主题令牌变色」——颜色完全来自令牌，模块不造色。
pub fn caret_color(t: &CaretTokens, bg: Rgb8) -> Rgb8 {
    let cl = contrast_ratio_100(t.on_light, bg);
    let cd = contrast_ratio_100(t.on_dark, bg);
    if cl >= cd {
        t.on_light
    } else {
        t.on_dark
    }
}

/// 对比度合格判定：选定令牌对背景必须 ≥4.5:1（主册验收线）。
pub fn caret_contrast_ok(t: &CaretTokens, bg: Rgb8) -> bool {
    contrast_ratio_100(caret_color(t, bg), bg) >= CARET_MIN_CONTRAST
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 断词不变量（20 组混排样张与 fuzz 共用）：
/// 1) `s ≤ pos < e ≤ len`；2) 拉丁/数字 run 最大化（两端异类）；
/// 3) CJK/标点单字成词（范围恰为一个码点长）。
fn word_break_ok(b: &[u8]) -> bool {
    let len = b.len();
    if len == 0 {
        return true;
    }
    for pos in 0..len {
        if pos > 0 && (b[pos] & 0xC0) == 0x80 {
            continue; // 只在码点首字节判（与选择器入口一致）
        }
        let (s, e) = word_range_at(b, pos);
        if s > pos || e > len || e <= s {
            return false;
        }
        let p = char_start(b, pos);
        let (cp, cl) = decode_at(b, p);
        let cls = class_of(cp);
        match cls {
            ScriptClass::Cjk | ScriptClass::Punct => {
                if e - s != cl {
                    return false;
                }
            }
            ScriptClass::Latin | ScriptClass::Digit => {
                if s > 0 && class_at(b, char_start(b, s - 1)) == cls {
                    return false;
                }
                if e < len && class_at(b, e) == cls {
                    return false;
                }
            }
            ScriptClass::Space => {}
        }
    }
    true
}

/// F201 自检（判据面：五语义统一 + 混排断词 100% + 插入符对比度）。
pub fn run_textsel_checks() -> CheckSet {
    let mut set = CheckSet::new("F201-textsel");

    // 1. 点击分类：首按为单击。
    let mut cc = ClickClassifier::new();
    set.add(
        "click single on first press",
        cc.feed(1_000, 10, 10) == ClickKind::Single,
        "",
    );

    // 2. 500ms 窗内原地再按 → 双击（恰在 500ms 边界仍算同链）。
    set.add(
        "click double within 500ms",
        cc.feed(1_500, 11, 12) == ClickKind::Double,
        "",
    );

    // 3. 三连击沿链升到三击。
    set.add(
        "click triple on third press",
        cc.feed(2_000, 10, 10) == ClickKind::Triple,
        "",
    );

    // 4. 超窗（501ms）或移位（>4px）回落单击。
    let mut cc2 = ClickClassifier::new();
    let _ = cc2.feed(0, 10, 10);
    let mut cc3 = ClickClassifier::new();
    let _ = cc3.feed(0, 10, 10);
    set.add(
        "click resets after window or move",
        cc2.feed(501, 10, 10) == ClickKind::Single && cc3.feed(1, 20, 10) == ClickKind::Single,
        "",
    );

    // 5. 混排断词：'o' 处选 "hello"（语言切换点为界，不吞 CJK）。
    let mixed = "hello世界".as_bytes();
    set.add(
        "word at latin excludes cjk",
        word_range_at(mixed, 1) == (0, 5),
        "",
    );

    // 6. 混排断词：'世' 处单字成词（中文按字断）。
    set.add(
        "word at cjk is single char",
        word_range_at(mixed, 6) == (5, 8),
        "",
    );

    // 7. 三击选段：段界为 \n。
    let para = "第一段\n第二段\n第三段".as_bytes();
    let (ps, pe) = paragraph_range_at(para, 12);
    set.add(
        "paragraph bounded by newlines",
        &para[ps..pe] == "第二段".as_bytes(),
        "",
    );

    // 8. 拖拽逐字符选：anchor/head 有序化。
    let st = sel_drag(9, 3);
    set.add(
        "drag char selection ordered",
        st.ordered() == (3, 9) && !st.is_empty(),
        "",
    );

    // 9. Shift+单击扩选：锚不动头动。
    let base = sel_word(mixed, 1); // "hello"
    let ext = sel_extend(base, 7);
    set.add(
        "shift click extends keeping anchor",
        ext.anchor == 0 && ext.head == 7,
        "",
    );

    // 10. Shift+双击按词扩选：向右扩到词尾、向左缩到词首。
    let rw = sel_extend_word(mixed, base, 6);
    let lw = sel_extend_word(mixed, base, 4);
    set.add(
        "shift double click extends by word",
        rw.head == 8 && lw.head == 0 && lw.anchor == 0,
        "",
    );

    // 11. 插入符深浅底对比度均 ≥4.5:1（主册验收线，两底各测）。
    let tokens = CaretTokens { on_light: Rgb8::new(20, 20, 20), on_dark: Rgb8::new(235, 235, 235) };
    let light_bg = Rgb8::new(250, 250, 250);
    let dark_bg = Rgb8::new(24, 24, 28);
    set.add(
        "caret contrast >= 4.5:1 on both bgs",
        caret_contrast_ok(&tokens, light_bg) && caret_contrast_ok(&tokens, dark_bg),
        "",
    );

    // 12. 自动反色：深浅两底选中的令牌不同。
    set.add(
        "caret auto-inverts between bgs",
        caret_color(&tokens, light_bg) == tokens.on_light
            && caret_color(&tokens, dark_bg) == tokens.on_dark,
        "",
    );

    // 13. 混排断词验收：20 组中英混排样张全通过（正确率 100%）。
    const MIXED: [&str; MIXED_SAMPLES] = [
        "hello世界",
        "文件file",
        "第1章",
        "foo123bar",
        "中文English混排",
        "a中b英c",
        "x射线X光",
        "2024年",
        "abc，abc",
        "点pixel点",
        "OK不OK",
        "v1.2版本",
        "go语言rust",
        "A1区B2区",
        "yes或者no",
        "3D渲染2D",
        "e插入i",
        "测试test中",
        "z黏y",
        "混mixed合",
    ];
    let mut ok_n = 0usize;
    for s in MIXED.iter() {
        if word_break_ok(s.as_bytes()) {
            ok_n += 1;
        }
    }
    set.add(
        "20 mixed samples 100% correct",
        ok_n == MIXED_SAMPLES,
        "",
    );

    // 14. fuzz（xorshift32 范式）：随机拼接混排串 × 随机位置，断词不变量 2000 轮全守恒。
    const ALPHABET: [&str; 8] = ["h", "e", "世", "界", "1", "，", " ", "x"];
    let mut x: u32 = 0x9E3779B9;
    let mut fuzz_ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let n = (x % 12) as usize + 1;
        let mut buf = [0u8; 48];
        let mut blen = 0usize;
        for _ in 0..n {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let piece = ALPHABET[(x as usize) % ALPHABET.len()];
            let pb = piece.as_bytes();
            if blen + pb.len() <= buf.len() {
                buf[blen..blen + pb.len()].copy_from_slice(pb);
                blen += pb.len();
            }
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let pos = (x as usize) % (blen + 1);
        if !word_break_ok(&buf[..blen]) {
            fuzz_ok = false;
        }
        let (s, e) = word_range_at(&buf[..blen], pos);
        if s > pos.min(blen) || e > blen {
            fuzz_ok = false;
        }
    }
    set.add("word-boundary fuzz 2000 rounds invariants", fuzz_ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_classifier_timing_edges() {
        let mut cc = ClickClassifier::new();
        assert_eq!(cc.feed(0, 0, 0), ClickKind::Single);
        // 恰 500ms 仍算同链；501ms 出窗。
        assert_eq!(cc.feed(500, 0, 0), ClickKind::Double);
        let mut cc2 = ClickClassifier::new();
        assert_eq!(cc2.feed(0, 0, 0), ClickKind::Single);
        assert_eq!(cc2.feed(501, 0, 0), ClickKind::Single);
        // 位移超出 4px 容差回落单击。
        let mut cc3 = ClickClassifier::new();
        assert_eq!(cc3.feed(0, 0, 0), ClickKind::Single);
        assert_eq!(cc3.feed(100, 5, 0), ClickKind::Single);
    }

    #[test]
    fn mixed_word_ranges_exact() {
        let b = "hello世界".as_bytes();
        assert_eq!(word_range_at(b, 0), (0, 5));
        assert_eq!(word_range_at(b, 4), (0, 5));
        assert_eq!(word_range_at(b, 5), (5, 8));
        assert_eq!(word_range_at(b, 7), (5, 8));
        // 数字与拉丁异类：'o' 处选 "hello"，'1' 处只选 "1"。
        let c = "abc123".as_bytes();
        assert_eq!(word_range_at(c, 1), (0, 3));
        assert_eq!(word_range_at(c, 4), (3, 6));
    }

    #[test]
    fn extend_word_skips_punctuation() {
        let b = "foo, bar".as_bytes();
        // 头部在标点上向右扩 → 跳到 "bar" 词尾。
        let st = SelState { anchor: 0, head: 3 };
        let ext = sel_extend_word(b, st, 4);
        assert_eq!(ext.head, 8);
        // 向左缩：从 "bar" 词尾按词跳——越过「, 」到 "foo" 词首。
        let st2 = SelState { anchor: 8, head: 8 };
        let ext2 = sel_extend_word(b, st2, 5);
        assert_eq!(ext2.head, 0);
        assert!(ext2.ordered() == (0, 8));
    }

    #[test]
    fn paragraph_and_multibyte_edges() {
        let b = "甲乙丙\n丁戊".as_bytes();
        let (s, e) = paragraph_range_at(b, 4);
        assert_eq!(&b[s..e], "甲乙丙".as_bytes());
        let (s2, e2) = paragraph_range_at(b, 13);
        assert_eq!(&b[s2..e2], "丁戊".as_bytes());
        // pos 落在多字节序列中间 → 回退到序列首字节再断词。
        assert_eq!(word_range_at(b, 2), (0, 3));
    }

    #[test]
    fn fuzz_word_break_5000() {
        const ALPHABET: [&str; 6] = ["a", "B", "中", "9", ".", " "];
        let mut x: u32 = 0xDEADBEEF;
        for _ in 0..5000u32 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let n = (x % 16) as usize + 1;
            let mut buf = [0u8; 48];
            let mut blen = 0usize;
            for _ in 0..n {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let pb = ALPHABET[(x as usize) % ALPHABET.len()].as_bytes();
                buf[blen..blen + pb.len()].copy_from_slice(pb);
                blen += pb.len();
            }
            assert!(word_break_ok(&buf[..blen]), "断词不变量被破坏: {:?}", &buf[..blen]);
        }
    }

    #[test]
    fn textsel_selfcheck_all_green() {
        let set = run_textsel_checks();
        assert!(set.all_passed(), "F201 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

// 持久化 I/O 面：断词样张哈希登记表（20 组混排样张的字节级档案）。
const VXH1_MAGIC: [u8; 4] = *b"VXH1";
/// 记录版本：字段布局变更即升版，旧版拒绝解析（不猜测）。
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：magic/版本/校验/长度四类全拒，绝不静默截断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2CodecErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadSum,
}

/// FNV-1a 32 位：校验和与样张哈希共用同一实现（一处一事实）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 登记容量——主册 F201「混排断词正确率 100%（20 组中英混排样张）」：
/// 容量即 20，超出显性拒绝（登记面在册上限，非交互热路径）。
pub const SAMPLE_BOOK_CAP: usize = MIXED_SAMPLES;

/// 记录长：magic4 + ver1 + count1 + 20×u32 哈希 + sum4。
pub const SAMPLE_BOOK_REC_LEN: usize = 4 + 1 + 1 + 4 * SAMPLE_BOOK_CAP + 4;

/// 断词样张登记表：每组样张以 FNV-1a 哈希入册，整表可编解码（定长零堆）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SampleBook {
    pub count: usize,
    pub hashes: [u32; SAMPLE_BOOK_CAP],
}

impl SampleBook {
    pub fn new() -> SampleBook {
        SampleBook { count: 0, hashes: [0u32; SAMPLE_BOOK_CAP] }
    }

    /// 登记一组样张（容量满显性拒绝，返回 false）。
    pub fn register(&mut self, sample: &str) -> bool {
        if self.count >= SAMPLE_BOOK_CAP {
            return false;
        }
        self.hashes[self.count] = fnv1a(sample.as_bytes());
        self.count += 1;
        true
    }

    /// 整表编码：尾部 FNV-1a 覆盖 magic+ver+payload。
    pub fn to_bytes(&self) -> [u8; SAMPLE_BOOK_REC_LEN] {
        let mut out = [0u8; SAMPLE_BOOK_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        out[5] = self.count as u8;
        for k in 0..SAMPLE_BOOK_CAP {
            out[6 + k * 4..10 + k * 4].copy_from_slice(&self.hashes[k].to_le_bytes());
        }
        let body = 6 + 4 * SAMPLE_BOOK_CAP;
        let sum = fnv1a(&out[..body]).to_le_bytes();
        out[body..body + 4].copy_from_slice(&sum);
        out
    }

    /// 解码：长度/magic/版本/校验四类损坏全拒；count 超容量按 BadLen 拒。
    pub fn from_bytes(b: &[u8]) -> Result<SampleBook, V2CodecErr> {
        if b.len() != SAMPLE_BOOK_REC_LEN {
            return Err(V2CodecErr::BadLen);
        }
        let mut mg = [0u8; 4];
        mg.copy_from_slice(&b[..4]);
        if mg != VXH1_MAGIC {
            return Err(V2CodecErr::BadMagic);
        }
        if b[4] != VXH1_VER {
            return Err(V2CodecErr::BadVersion);
        }
        let body = 6 + 4 * SAMPLE_BOOK_CAP;
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[body..body + 4]);
        if fnv1a(&b[..body]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        if b[5] as usize > SAMPLE_BOOK_CAP {
            return Err(V2CodecErr::BadLen);
        }
        let count = b[5] as usize;
        let mut hashes = [0u32; SAMPLE_BOOK_CAP];
        for k in 0..count {
            let mut v = [0u8; 4];
            v.copy_from_slice(&b[6 + k * 4..10 + k * 4]);
            hashes[k] = u32::from_le_bytes(v);
        }
        Ok(SampleBook { count, hashes })
    }
}

// UI 壳接线面：插入符绘制清单（F151 令牌联动：几何与配色分离）。
/// 绘制图元：几何 + 颜色索引（颜色由壳层按索引取令牌，模块不造色）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2Prim {
    pub rect: crate::h1star::h1base::Rect,
    pub color_idx: u8,
}

/// 插入符绘制清单（定长 1 项）：粗 2px 竖条，索引 0 = 深浅底反色令牌
/// （反色判定在 `caret_contrast_ok`，绘制面只发几何+索引）。
pub fn caret_draw_list(x: i32, y: i32, h: i32) -> [V2Prim; 1] {
    [V2Prim { rect: crate::h1star::h1base::Rect::new(x, y, CARET_W_PX, h), color_idx: 0 }]
}

/// DPI 缩放四档（壳层判据：×100/125/150/200% 整数换算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DpiPct {
    P100,
    P125,
    P150,
    P200,
}

impl DpiPct {
    pub fn num(self) -> u32 {
        match self {
            DpiPct::P100 => 100,
            DpiPct::P125 => 125,
            DpiPct::P150 => 150,
            DpiPct::P200 => 200,
        }
    }
}

/// px×pct/100 四舍五入（全整数，壳层布局整批走此函数换算）。
pub fn dpi_scale_px(px: i32, pct: DpiPct) -> i32 {
    ((px as i64 * pct.num() as i64 + 50) / 100) as i32
}

/// F201 v2 自检（首条恒为持久化 round-trip；判定锚见各条注释）。
pub fn run_textsel_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F201-textsel-v2");

    // 1. 持久化 round-trip：样张登记表编码→解码逐哈希还原（v2 首条惯例）。
    let mut book = SampleBook::new();
    let _ = book.register("hello世界");
    let _ = book.register("第1章");
    let bytes = book.to_bytes();
    let ok_rt = match SampleBook::from_bytes(&bytes) {
        Ok(b2) => b2.count == 2 && b2.hashes[0] == book.hashes[0] && b2.hashes[1] == book.hashes[1],
        Err(_) => false,
    };
    set.add("v2 persist roundtrip sample book", ok_rt, "");

    // 2. 损坏拒绝四类：magic/版本/长度/校验（档案面纪律：坏档案不静默解析）。
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[6] ^= 0xFF;
    set.add(
        "v2 persist rejects corrupt inputs",
        SampleBook::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && SampleBook::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && SampleBook::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && SampleBook::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen),
        "",
    );

    // 3. UI 壳接线：插入符 2px 几何 + 颜色索引分离；DPI 四档换算
    //    （验主册 F201「插入符粗 2px」+ F151 令牌联动 + 壳层 DPI 判据）。
    let d = caret_draw_list(10, 20, 24);
    set.add(
        "v2 caret draw list & dpi scale",
        d[0].rect.w == CARET_W_PX && d[0].rect.h == 24 && d[0].color_idx == 0
            && dpi_scale_px(8, DpiPct::P100) == 8
            && dpi_scale_px(8, DpiPct::P125) == 10
            && dpi_scale_px(8, DpiPct::P150) == 12
            && dpi_scale_px(8, DpiPct::P200) == 16,
        "",
    );

    // 4. 容量在册：20 组样张满后显性拒绝（验主册 F201「20 组样张」口径）。
    let mut full = SampleBook::new();
    let mut accepted = 0usize;
    for k in 0..25u32 {
        let _ = k;
        if full.register("样张") {
            accepted += 1;
        }
    }
    set.add("v2 sample book cap 20 rejects extra", accepted == SAMPLE_BOOK_CAP, "");

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn sample_book_roundtrip_exact() {
        let mut b = SampleBook::new();
        for s in ["a1中", "x,y", "混mixed合"] {
            assert!(b.register(s));
        }
        let back = SampleBook::from_bytes(&b.to_bytes()).unwrap();
        assert_eq!(back.count, 3);
        assert_eq!(back.hashes[2], fnv1a("混mixed合".as_bytes()));
    }

    #[test]
    fn dpi_and_caret_shell() {
        assert_eq!(dpi_scale_px(20, DpiPct::P150), 30);
        assert_eq!(caret_draw_list(3, 4, 20)[0].rect.h, 20);
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_textsel_v2_checks();
        assert!(set.all_passed(), "F201 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
