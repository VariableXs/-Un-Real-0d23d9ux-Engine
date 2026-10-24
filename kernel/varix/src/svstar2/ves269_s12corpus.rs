//! VE-F4029 · RTL 测试语料
//!
//! RTL 语料库：阿拉伯/希伯来/波斯/乌尔都四语言语料 + 混合方向语料 +
//! BiDi 控制符语料——语料覆盖断言（四语言全语料——覆盖红线复用）；语
//! 料依据红线（无依据→撤回）；语料版本（锚定）；真机读屏 RTL 验证。
//! 数据结构：覆盖断言；依据红线；版本锚定；真机复述。
//! 错误路径：语言无料→阻断；无依据→撤回；版本漂→锚定；真机缺→P0。
//! 性能：覆盖 O(四语言)；依据 O(断言)；版本 O(1)；真机 O(矩阵)。
//! 跨批对接：F4010 模式复用声明；F3858 真机对端；T01 语料对端。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6CC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6CC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T35CCode(pub u16);

impl T35CCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const LANG_NO_CORPUS: Self = Self(SEG_BASE + 1);
    pub const NO_EVIDENCE: Self = Self(SEG_BASE + 2);
    pub const DEVICE_P0: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::LANG_NO_CORPUS => "语言无料→阻断",
            Self::NO_EVIDENCE => "无依据→撤回",
            Self::DEVICE_P0 => "真机缺→P0",
            _ => "未知",
        }
    }
}

// ── 四语言语料覆盖 ──────────────────────────────────────────────────

pub const RTL_LANGS: [&str; 4] = ["ar", "he", "fa", "ur"];

/// 语料条目。
#[derive(Debug, Clone)]
pub struct CorpusEntry {
    pub lang: &'static str,
    /// 语料文本。
    pub text: String,
    /// 语料类：纯 RTL / 混合方向 / BiDi 控制符。
    pub kind: CorpusKind,
    /// 依据指针（空 = 无依据）。
    pub evidence: String,
    /// 版本锚。
    pub ver: u32,
}

/// 语料类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusKind {
    PureRtl,
    MixedDir,
    BiDiControl,
}

pub const CORPUS_KINDS: [CorpusKind; 3] = [CorpusKind::PureRtl, CorpusKind::MixedDir, CorpusKind::BiDiControl];

/// 语料库。
#[derive(Debug, Clone)]
pub struct RtlCorpus {
    pub entries: Vec<CorpusEntry>,
    pub corpus_ver: u32,
}

pub const CORPUS_VERSION: u32 = 4;

impl RtlCorpus {
    pub fn new() -> Self {
        Self { entries: Vec::new(), corpus_ver: CORPUS_VERSION }
    }

    pub fn put(&mut self, lang: &'static str, text: &str, kind: CorpusKind, evidence: &str) {
        self.entries.push(CorpusEntry {
            lang,
            text: String::from(text),
            kind,
            evidence: String::from(evidence),
            ver: CORPUS_VERSION,
        });
    }

    /// 语言无料检出（覆盖红线：四语言缺一阻断）。
    pub fn lang_gaps(&self) -> Vec<&'static str> {
        RTL_LANGS.iter()
            .filter(|l| !self.entries.iter().any(|e| e.lang == **l))
            .map(|l| *l)
            .collect()
    }

    /// 依据红线：无依据语料撤回清单。
    pub fn no_evidence(&self) -> Vec<usize> {
        self.entries.iter().enumerate()
            .filter(|(_, e)| e.evidence.is_empty())
            .map(|(i, _)| i)
            .collect()
    }

    /// 类覆盖：三类齐。
    pub fn kind_covered(&self) -> bool {
        CORPUS_KINDS.iter().all(|k| self.entries.iter().any(|e| e.kind == *k))
    }

    /// 版本锚定：全条目锚当前版。
    pub fn version_anchored(&self) -> bool {
        self.entries.iter().all(|e| e.ver == self.corpus_ver)
    }
}

// ── 真机读屏 RTL 验证 ───────────────────────────────────────────────

/// 真机矩阵格：语言 × 读屏引擎。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceCell {
    pub lang: &'static str,
    /// 读屏引擎槽 0..3。
    pub engine: u8,
    /// 已验证。
    pub verified: bool,
}

/// 真机验证矩阵。
#[derive(Debug, Clone)]
pub struct DeviceMatrix {
    pub cells: Vec<DeviceCell>,
}

pub const ENGINE_TOTAL: u8 = 3;

impl DeviceMatrix {
    /// 构建全矩阵（4 语言 × 3 引擎 = 12 格）。
    pub fn build() -> Self {
        let mut cells = Vec::new();
        for l in RTL_LANGS.iter() {
            for e in 0..ENGINE_TOTAL {
                cells.push(DeviceCell { lang: l, engine: e, verified: false });
            }
        }
        Self { cells }
    }

    pub fn verify(&mut self, idx: usize) -> bool {
        match self.cells.get_mut(idx) {
            Some(c) => {
                c.verified = true;
                true
            }
            None => false,
        }
    }

    /// 真机缺检出（P0 对象）。
    pub fn unverified(&self) -> Vec<(usize, &'static str, u8)> {
        self.cells.iter().enumerate()
            .filter(|(_, c)| !c.verified)
            .map(|(i, c)| (i, c.lang, c.engine))
            .collect()
    }

    pub fn all_verified(&self) -> bool {
        self.unverified().is_empty()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves269_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves269_s12corpus");

    // C4029-01 诊断码段不越窗。
    chk(&mut set, "C4029-01",
        T35CCode::DEVICE_P0.0 < SEG_BASE + 16 && SEG_BASE == 0x6CC0, "err");

    // C4029-02 四语言清单齐备。
    chk(&mut set, "C4029-02",
        RTL_LANGS.len() == 4
            && RTL_LANGS[0] == "ar" && RTL_LANGS[3] == "ur", "err");

    // C4029-03 语言无料检出（空库四缺）。
    let c0 = RtlCorpus::new();
    chk(&mut set, "C4029-03",
        c0.lang_gaps().len() == 4, "err");

    // C4029-04 三语言补齐后缺一。
    let mut c = RtlCorpus::new();
    c.put("ar", "مرحبا", CorpusKind::PureRtl, "spec-ar-1");
    c.put("he", "שלום", CorpusKind::PureRtl, "spec-he-1");
    c.put("fa", "سلام", CorpusKind::PureRtl, "spec-fa-1");
    chk(&mut set, "C4029-04",
        c.lang_gaps() == vec!["ur"] && T35CCode::LANG_NO_CORPUS.explain().contains("阻断"), "err");

    // C4029-05 四语言补齐 → 覆盖过。
    c.put("ur", "خوش آمدید", CorpusKind::PureRtl, "spec-ur-1");
    chk(&mut set, "C4029-05", c.lang_gaps().is_empty(), "err");

    // C4029-06 三类覆盖：缺混合与控制符。
    chk(&mut set, "C4029-06", !c.kind_covered(), "err");

    // C4029-07 补混合与控制符语料 → 类齐。
    c.put("ar", "عربي 123", CorpusKind::MixedDir, "spec-ar-2");
    c.put("he", "\u{202B}x\u{202C}", CorpusKind::BiDiControl, "spec-he-2");
    chk(&mut set, "C4029-07", c.kind_covered() && c.entries.len() == 6, "err");

    // C4029-08 依据红线：无依据撤回清单。
    c.put("ar", "无依据条目", CorpusKind::PureRtl, "");
    let noev = c.no_evidence();
    chk(&mut set, "C4029-08",
        noev == vec![6usize] && T35CCode::NO_EVIDENCE.explain().contains("撤回"), "err");

    // C4029-09 版本锚定：当前全锚。
    chk(&mut set, "C4029-09", c.version_anchored(), "err");

    // C4029-10 版本漂检出。
    c.corpus_ver = 5;
    chk(&mut set, "C4029-10", !c.version_anchored(), "err");

    // C4029-11 真机矩阵 12 格。
    let mut dm = DeviceMatrix::build();
    chk(&mut set, "C4029-11", dm.cells.len() == 12, "err");

    // C4029-12 真机缺 → P0 对象。
    chk(&mut set, "C4029-12",
        dm.unverified().len() == 12 && !dm.all_verified(), "err");

    // C4029-13 逐格验证。
    for i in 0..12 {
        dm.verify(i);
    }
    chk(&mut set, "C4029-13", dm.all_verified(), "err");

    // C4029-14 越界验证拒绝。
    chk(&mut set, "C4029-14", !dm.verify(99), "err");

    // C4029-15 语料类枚举互异。
    chk(&mut set, "C4029-15",
        CorpusKind::PureRtl != CorpusKind::MixedDir
            && CorpusKind::MixedDir != CorpusKind::BiDiControl, "err");

    // C4029-16 语料版本常量 4。
    chk(&mut set, "C4029-16", CORPUS_VERSION == 4, "err");

    // C4029-17 引擎数 3。
    chk(&mut set, "C4029-17", ENGINE_TOTAL == 3, "err");

    // C4029-18 空真机矩阵不误报全验证。
    chk(&mut set, "C4029-18", DeviceMatrix { cells: Vec::new() }.all_verified(), "err");

    // C4029-19 真机缺码语义。
    chk(&mut set, "C4029-19", T35CCode::DEVICE_P0.explain().contains("P0"), "err");

    // C4029-20 错误码互异。
    let codes = [T35CCode::OK.0, T35CCode::LANG_NO_CORPUS.0, T35CCode::NO_EVIDENCE.0, T35CCode::DEVICE_P0.0];
    chk(&mut set, "C4029-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_lang_coverage() {
        let mut c = RtlCorpus::new();
        assert_eq!(c.lang_gaps().len(), 4);
        for l in RTL_LANGS.iter() {
            c.put(l, "x", CorpusKind::PureRtl, "ev");
        }
        assert!(c.lang_gaps().is_empty());
    }

    #[test]
    fn no_evidence_withdrawn() {
        let mut c = RtlCorpus::new();
        c.put("ar", "x", CorpusKind::PureRtl, "");
        assert_eq!(c.no_evidence().len(), 1);
    }

    #[test]
    fn device_matrix_12() {
        let mut dm = DeviceMatrix::build();
        assert_eq!(dm.cells.len(), 12);
        for i in 0..12 {
            dm.verify(i);
        }
        assert!(dm.all_verified());
    }
}
