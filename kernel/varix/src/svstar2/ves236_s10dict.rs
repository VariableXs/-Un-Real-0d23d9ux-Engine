//! VE-F3982 · S 域交互词典与一致性终审
//!
//! 终审：S 域交互词典冻结（焦点/朗读/快捷键/动效/色彩五类术语 + 操作
//! 定义）；一致性终审（八组用法与词典逐条比对）；与家族词典（R 版
//! F3782）对拍。
//! 数据结构：词典终版；比对表；对拍记录。
//! 错误路径：用法越词→改词或改用法（二选一留痕）；词典冲突→仲裁；
//! 冻结后发现→变更纪律（F3996）。
//! 性能：比对 O(条)；仲裁 O(冲突)；冻结 O(1)。
//! 跨批对接：F3965 口径上游；F3983 联测用词基准。
//! 无障碍与隐私：词典含读屏朗读措辞规范（域本色）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3AC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3AC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T2DCode(pub u16);

impl T2DCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const OUT_OF_WORD: Self = Self(SEG_BASE + 1);
    pub const CONFLICT: Self = Self(SEG_BASE + 2);
    pub const POST_FROZEN: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::OUT_OF_WORD => "用法越词→二选一留痕",
            Self::CONFLICT => "词典冲突→仲裁",
            Self::POST_FROZEN => "冻结后发现→变更纪律",
            _ => "未知",
        }
    }
}

// ── 五类术语 ─────────────────────────────────────────────────────────

/// 术语五类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKind {
    /// 焦点。
    Focus,
    /// 朗读。
    Speech,
    /// 快捷键。
    Hotkey,
    /// 动效。
    Motion,
    /// 色彩。
    Color,
}

pub const TERM_KINDS: [TermKind; 5] = [
    TermKind::Focus, TermKind::Speech, TermKind::Hotkey, TermKind::Motion, TermKind::Color,
];

impl TermKind {
    pub fn name(self) -> &'static str {
        match self {
            TermKind::Focus => "焦点",
            TermKind::Speech => "朗读",
            TermKind::Hotkey => "快捷键",
            TermKind::Motion => "动效",
            TermKind::Color => "色彩",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            TermKind::Focus => 0,
            TermKind::Speech => 1,
            TermKind::Hotkey => 2,
            TermKind::Motion => 3,
            TermKind::Color => 4,
        }
    }
}

/// 词典条目：术语 + 类别 + 操作定义。
#[derive(Debug, Clone)]
pub struct TermEntry {
    /// 术语词。
    pub word: String,
    /// 类别。
    pub kind: TermKind,
    /// 操作定义。
    pub def: String,
}

/// 词典终版（可冻结）。
#[derive(Debug, Clone)]
pub struct DictFinal {
    pub terms: Vec<TermEntry>,
    pub frozen: bool,
}

impl DictFinal {
    pub fn new() -> Self {
        Self { terms: Vec::new(), frozen: false }
    }

    /// 收词：同词同类拒绝（冲突候选）。
    pub fn put(&mut self, word: &str, kind: TermKind, def: &str) -> bool {
        if self.frozen || self.terms.iter().any(|t| t.word == word && t.kind == kind) {
            return false;
        }
        self.terms.push(TermEntry {
            word: String::from(word),
            kind,
            def: String::from(def),
        });
        true
    }

    /// 词典冲突检出：同词不同类双录 = 冲突（须仲裁）。
    pub fn conflicts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in self.terms.iter() {
            let dup_kind = self.terms.iter().any(|o| o.word == t.word && o.kind != t.kind);
            if dup_kind && !out.iter().any(|w: &String| w == &t.word) {
                out.push(t.word.clone());
            }
        }
        out
    }

    /// 查词。
    pub fn lookup(&self, word: &str, kind: TermKind) -> Option<&TermEntry> {
        self.terms.iter().find(|t| t.word == word && t.kind == kind)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> T2DCode {
        if self.frozen {
            return T2DCode::OK;
        }
        if !self.conflicts().is_empty() {
            return T2DCode::CONFLICT;
        }
        self.frozen = true;
        T2DCode::OK
    }
}

// ── 一致性终审：八组用法逐条比对 ────────────────────────────────────

pub const USAGE_GROUPS: usize = 8;

/// 单条用法。
#[derive(Debug, Clone)]
pub struct Usage {
    /// 组号 0..8。
    pub group: u8,
    /// 用词。
    pub word: String,
    /// 类别。
    pub kind: TermKind,
}

/// 比对裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 在词。
    InWord,
    /// 越词（须二选一：改词或改用法）。
    OutWord,
}

/// 比对表。
#[derive(Debug, Clone)]
pub struct CompareSheet {
    pub rows: Vec<(Usage, Verdict)>,
}

impl CompareSheet {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// 逐条比对（词典查词）。
    pub fn compare(&mut self, dict: &DictFinal, usages: &[Usage]) {
        self.rows.clear();
        for u in usages.iter() {
            let v = if dict.lookup(&u.word, u.kind).is_some() {
                Verdict::InWord
            } else {
                Verdict::OutWord
            };
            self.rows.push((u.clone(), v));
        }
    }

    /// 越词清单。
    pub fn out_words(&self) -> Vec<&Usage> {
        self.rows.iter().filter(|(_, v)| *v == Verdict::OutWord).map(|(u, _)| u).collect()
    }
}

/// 二选一留痕：改词或改用法。
#[derive(Debug, Clone)]
pub struct DuoChoice {
    /// 用词原文。
    pub word: String,
    /// true = 改词（换词典内词）；false = 改用法（用法侧纠正）。
    pub fix_word: bool,
    /// 留痕说明。
    pub note: String,
}

/// 留痕册。
#[derive(Debug, Clone)]
pub struct ChoiceLedger {
    pub items: Vec<DuoChoice>,
}

impl ChoiceLedger {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// 登记（二选一必填其一，note 非空强制）。
    pub fn record(&mut self, word: &str, fix_word: bool, note: &str) -> bool {
        if note.is_empty() {
            return false;
        }
        self.items.push(DuoChoice {
            word: String::from(word),
            fix_word,
            note: String::from(note),
        });
        true
    }
}

// ── 家族词典对拍（R 版 F3782）───────────────────────────────────────

/// 对拍记录：同词在 S 词典与家族词典的类别一致性。
#[derive(Debug, Clone)]
pub struct PeerAlignRecord {
    /// 对拍词数。
    pub checked: u32,
    /// 类别不一致数。
    pub mismatch: u32,
    /// 对拍结论（mismatch == 0）。
    pub aligned: bool,
}

/// 家族对拍：以家族词表（词→类别槽）为真源。
pub fn peer_align(dict: &DictFinal, family: &[(&str, TermKind)]) -> PeerAlignRecord {
    let mut checked = 0u32;
    let mut mismatch = 0u32;
    for (w, k) in family.iter() {
        if let Some(e) = dict.lookup(w, *k) {
            checked += 1;
            let _ = e;
        } else if dict.terms.iter().any(|t| t.word == *w) {
            // 同词异类 = 不一致。
            mismatch += 1;
        } else {
            // 家族词未收录 S 侧 = 缺录（不计 mismatch，计缺）。
            let _ = w;
        }
    }
    PeerAlignRecord { checked, mismatch, aligned: mismatch == 0 }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves236_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves236_s10dict");

    // C3982-01 诊断码段不越窗。
    chk(&mut set, "C3982-01",
        T2DCode::POST_FROZEN.0 < SEG_BASE + 16 && SEG_BASE == 0x3AC0, "err");

    // C3982-02 五类术语齐备。
    chk(&mut set, "C3982-02",
        TERM_KINDS.len() == 5
            && TERM_KINDS[0].name() == "焦点"
            && TERM_KINDS[4].name() == "色彩", "err");

    // C3982-03 收词 + 同词同类拒绝。
    let mut d = DictFinal::new();
    chk(&mut set, "C3982-03",
        d.put("焦点环", TermKind::Focus, "可视聚焦描边")
            && !d.put("焦点环", TermKind::Focus, "重复收词"), "err");

    // C3982-04 同词异类 = 冲突检出。
    d.put("播报", TermKind::Speech, "朗读出声");
    d.put("播报", TermKind::Motion, "动效播报——越类");
    chk(&mut set, "C3982-04", d.conflicts() == vec![String::from("播报")], "err");

    // C3982-05 冲突未仲裁 → 冻结拒绝。
    chk(&mut set, "C3982-05", d.freeze() == T2DCode::CONFLICT && !d.frozen, "err");

    // C3982-06 仲裁（撤越类条目）后冻结放行。
    d.terms.retain(|t| !(t.word == "播报" && t.kind == TermKind::Motion));
    chk(&mut set, "C3982-06", d.conflicts().is_empty() && d.freeze() == T2DCode::OK, "err");

    // C3982-07 冻结后收词拒绝（POST_FROZEN 前置：变更走纪律）。
    chk(&mut set, "C3982-07", !d.put("新词", TermKind::Focus, "冻后"), "err");

    // C3982-08 八组用法比对：全在词。
    let mut usages = Vec::new();
    for g in 0..USAGE_GROUPS as u8 {
        usages.push(Usage { group: g, word: String::from("焦点环"), kind: TermKind::Focus });
    }
    let mut sheet = CompareSheet::new();
    sheet.compare(&d, &usages);
    chk(&mut set, "C3982-08", sheet.out_words().is_empty() && sheet.rows.len() == 8, "err");

    // C3982-09 越词检出（未收录词）。
    usages.push(Usage { group: 8, word: String::from("高亮圈"), kind: TermKind::Focus });
    let mut sheet2 = CompareSheet::new();
    sheet2.compare(&d, &usages);
    chk(&mut set, "C3982-09", sheet2.out_words().len() == 1, "err");

    // C3982-10 二选一留痕：note 非空强制。
    let mut led = ChoiceLedger::new();
    chk(&mut set, "C3982-10",
        !led.record("高亮圈", true, "") && led.record("高亮圈", true, "改词为焦点环"), "err");

    // C3982-11 改用法路线留痕。
    led.record("高亮圈", false, "用法侧纠正为焦点环");
    chk(&mut set, "C3982-11", led.items.len() == 2 && !led.items[1].fix_word, "err");

    // C3982-12 家族对拍：同词同类一致。
    let family = [("焦点环", TermKind::Focus), ("播报", TermKind::Speech)];
    let rec = peer_align(&d, &family);
    chk(&mut set, "C3982-12", rec.aligned && rec.checked == 2 && rec.mismatch == 0, "err");

    // C3982-13 家族对拍：同词异类 → mismatch。
    let family2 = [("焦点环", TermKind::Color)];
    let rec2 = peer_align(&d, &family2);
    chk(&mut set, "C3982-13", !rec2.aligned && rec2.mismatch == 1, "err");

    // C3982-14 词典查词命中与未命中。
    chk(&mut set, "C3982-14",
        d.lookup("焦点环", TermKind::Focus).is_some()
            && d.lookup("焦点环", TermKind::Color).is_none(), "err");

    // C3982-15 越词错误码语义。
    chk(&mut set, "C3982-15", T2DCode::OUT_OF_WORD.explain().contains("二选一"), "err");

    // C3982-16 类别槽位双射（5 类互异槽位）。
    let slots = [TermKind::Focus.slot(), TermKind::Speech.slot(), TermKind::Hotkey.slot(),
        TermKind::Motion.slot(), TermKind::Color.slot()];
    let mut uniq = true;
    for i in 0..5 {
        for j in (i + 1)..5 {
            if slots[i] == slots[j] {
                uniq = false;
            }
        }
    }
    chk(&mut set, "C3982-16", uniq, "err");

    // C3982-17 空词典冻结放行（无冲突即冻）。
    let mut d2 = DictFinal::new();
    chk(&mut set, "C3982-17", d2.freeze() == T2DCode::OK, "err");

    // C3982-18 比对表复用清空（rows.clear 幂等）。
    let mut sheet3 = CompareSheet::new();
    sheet3.compare(&d, &usages);
    let n1 = sheet3.rows.len();
    sheet3.compare(&d, &usages);
    chk(&mut set, "C3982-18", sheet3.rows.len() == n1, "err");

    // C3982-19 诊断码互异。
    let codes = [T2DCode::OK.0, T2DCode::OUT_OF_WORD.0, T2DCode::CONFLICT.0, T2DCode::POST_FROZEN.0];
    chk(&mut set, "C3982-19",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    // C3982-20 词典含朗读措辞规范条目（域本色抽查）。
    let mut d3 = DictFinal::new();
    d3.put("朗读停顿", TermKind::Speech, "读屏措辞规范：句读停顿 300ms");
    chk(&mut set, "C3982-20",
        d3.lookup("朗读停顿", TermKind::Speech)
            .map(|t| t.def.contains("读屏措辞规范")).unwrap_or(false), "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_blocks_freeze() {
        let mut d = DictFinal::new();
        d.put("词", TermKind::Focus, "甲");
        d.put("词", TermKind::Motion, "乙");
        assert_eq!(d.freeze(), T2DCode::CONFLICT);
    }

    #[test]
    fn usage_compare() {
        let mut d = DictFinal::new();
        d.put("焦点环", TermKind::Focus, "x");
        d.freeze();
        let usages = [Usage { group: 0, word: String::from("焦点环"), kind: TermKind::Focus }];
        let mut s = CompareSheet::new();
        s.compare(&d, &usages);
        assert!(s.out_words().is_empty());
    }

    #[test]
    fn peer_align_detects_kind_drift() {
        let mut d = DictFinal::new();
        d.put("词", TermKind::Focus, "x");
        let rec = peer_align(&d, &[("词", TermKind::Color)]);
        assert!(!rec.aligned);
    }
}
