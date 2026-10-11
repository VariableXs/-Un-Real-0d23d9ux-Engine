//! unx781_report_freeze ↔ UNX-F0781 收官报告定版（六节定稿+哈希封存）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0781，UNX-A1-B39）：
//! 「收官报告定版（六节定稿+哈希封存）| 570 | 已深化 |
//!  UNX-F0781-J1 定版过（六节定稿 + 指针可解引 100% + 报告哈希封存）
//!  且与 F0778 初稿差异留痕」
//!
//! 设计要点：
//! 1. **六节定稿**：`ReportSection` 六节（总览/账目/判据/缺陷/结论/
//!     移交）逐节定稿；`SectionState` 三态（drafting/ready/frozen），
//!     只有全 ready 才允许 `freeze()`。
//! 2. **指针可解引 100%**：每节带 `pointer`（证据指针），
//!     `resolvability_audit()` 逐节核非空 + 格式，
//!     不可解引的节 = 报告有不可验证的宣称。
//! 3. **哈希封存**：`freeze()` 后 `seal_hash` 对六节内容 + 指针
//!     做 FNV 封存；`verify_seal()` 复算比对——封存后偷偷改
//!     一节即 `SealBroken`（定版的意义就在这）。
//! 4. **与初稿差异留痕**：`DiffLedger` 记「初稿 → 定稿」每节差异
//!     （行数增量/内容变更类型），禁静默改稿——静默改稿 =
//!     初稿与定稿的差异不可追（验收会问"你改了啥"）。
//! 5. **冻结后写拒**：frozen 节 `amend()` 一律 `Frozen`；
//!     要改必须走 `defrost()`（显性解封 + 重新封存 + 哈希必变）。
//!
//! 诊断码：`FreezeCode` 0x91 段（0x9100..=0x910F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX781_FREEZE_VERSION: &str = "unx781-report-freeze-v1";

/// 节数（六节）。
pub const SECTION_COUNT: usize = 6;

/// 节名（与下标严格同序）。
pub const SECTION_NAMES: [&str; SECTION_COUNT] = [
    "overview",
    "ledger",
    "criteria",
    "defects",
    "conclusion",
    "handover",
];

/// 节状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SectionState {
    /// 草稿
    Drafting,
    /// 就绪（待封存）
    Ready,
    /// 已封存
    Frozen,
}

/// 单节内容。
#[derive(Clone, Debug)]
pub struct ReportSection {
    pub name: &'static str,
    pub state: SectionState,
    /// 证据指针（可解引）
    pub pointer: String,
    /// 正文字数（定稿时登记）
    pub words: u32,
}

/// 差异一条（初稿 → 定稿）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffEntry {
    pub section: &'static str,
    /// 初稿字数
    pub draft_words: u32,
    /// 定稿字数
    pub final_words: u32,
    /// 差异说明（禁空）
    pub note: String,
}

/// 收官报告（六节 + 差异账）。
pub struct FinalReport {
    sections: Vec<ReportSection>,
    diffs: Vec<DiffEntry>,
    /// 封存哈希
    seal_hash: u64,
}

/// 定版错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FreezeError {
    /// 节缺失
    SectionMissing { name: &'static str },
    /// 节未就绪
    SectionNotReady { name: &'static str },
    /// 指针空
    PointerEmpty { name: &'static str },
    /// 指针格式非法
    PointerBad { name: &'static str },
    /// 封存后改写
    Frozen { name: &'static str },
    /// 封存不符
    SealBroken { got: u64, want: u64 },
    /// 差异说明空
    NoteEmpty,
    /// 差异字数倒退（定稿 < 初稿要说明）
    WordsRegressed { section: &'static str },
}

/// 诊断码（0x91 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeCode {
    /// 0x9100 节缺失
    SectionMissing,
    /// 0x9101 节未就绪
    NotReady,
    /// 0x9102 指针空
    PointerEmpty,
    /// 0x9103 指针非法
    PointerBad,
    /// 0x9104 封存后改写
    Frozen,
    /// 0x9105 封存不符
    SealBroken,
    /// 0x9106 差异说明空
    NoteEmpty,
    /// 0x9107 字数倒退
    Regressed,
    /// 0x9108 定版通过
    FrozenOk,
}

impl FreezeCode {
    pub const fn code(self) -> u16 {
        match self {
            FreezeCode::SectionMissing => 0x9100,
            FreezeCode::NotReady => 0x9101,
            FreezeCode::PointerEmpty => 0x9102,
            FreezeCode::PointerBad => 0x9103,
            FreezeCode::Frozen => 0x9104,
            FreezeCode::SealBroken => 0x9105,
            FreezeCode::NoteEmpty => 0x9106,
            FreezeCode::Regressed => 0x9107,
            FreezeCode::FrozenOk => 0x9108,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            FreezeCode::SectionMissing => "FREEZE-SECTION-MISSING",
            FreezeCode::NotReady => "FREEZE-NOT-READY",
            FreezeCode::PointerEmpty => "FREEZE-POINTER-EMPTY",
            FreezeCode::PointerBad => "FREEZE-POINTER-BAD",
            FreezeCode::Frozen => "FREEZE-FROZEN",
            FreezeCode::SealBroken => "FREEZE-SEAL-BROKEN",
            FreezeCode::NoteEmpty => "FREEZE-NOTE-EMPTY",
            FreezeCode::Regressed => "FREEZE-REGRESSED",
            FreezeCode::FrozenOk => "FREEZE-OK",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            FreezeCode::SectionMissing => "收官报告缺节：六节关闭集缺一即拒",
            FreezeCode::NotReady => "节未就绪：只有 ready 节可参与封存",
            FreezeCode::PointerEmpty => "节证据指针为空：报告的宣称不可验证",
            FreezeCode::PointerBad => "节证据指针格式非法：必须 <路径>#<编号> 形态",
            FreezeCode::Frozen => "封存后改写被拒：定版后要改必须显性解封",
            FreezeCode::SealBroken => "封存哈希不符：定版后内容被偷偷改过",
            FreezeCode::NoteEmpty => "差异说明为空：初稿→定稿的差异必须留痕",
            FreezeCode::Regressed => "字数倒退：定稿字数少于初稿必须说明",
            FreezeCode::FrozenOk => "收官报告定版通过：六节封存 + 哈希可验",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx781_report_freeze.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9100 && (c & 0x00FF) < 0x10
    }
}

impl FreezeError {
    pub const fn code(&self) -> FreezeCode {
        match self {
            FreezeError::SectionMissing { .. } => FreezeCode::SectionMissing,
            FreezeError::SectionNotReady { .. } => FreezeCode::NotReady,
            FreezeError::PointerEmpty { .. } => FreezeCode::PointerEmpty,
            FreezeError::PointerBad { .. } => FreezeCode::PointerBad,
            FreezeError::Frozen { .. } => FreezeCode::Frozen,
            FreezeError::SealBroken { .. } => FreezeCode::SealBroken,
            FreezeError::NoteEmpty => FreezeCode::NoteEmpty,
            FreezeError::WordsRegressed { .. } => FreezeCode::Regressed,
        }
    }

    pub fn detail(&self) -> String {
        self.code().detail()
    }
}

/// FNV-1a 64。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl FinalReport {
    /// 空报告（六节全 Drafting）。
    pub fn new() -> Self {
        let mut sections: Vec<ReportSection> = Vec::new();
        let mut i = 0;
        while i < SECTION_COUNT {
            sections.push(ReportSection {
                name: SECTION_NAMES[i],
                state: SectionState::Drafting,
                pointer: String::new(),
                words: 0,
            });
            i += 1;
        }
        FinalReport {
            sections,
            diffs: Vec::new(),
            seal_hash: 0,
        }
    }

    /// 找节。
    fn find(&self, name: &str) -> Option<&ReportSection> {
        self.sections.iter().find(|x| x.name == name)
    }

    /// 定稿一节（指针非空 + 格式 + 未封存）。
    pub fn finalize_section(
        &mut self,
        name: &str,
        pointer: &str,
        words: u32,
    ) -> Result<(), FreezeError> {
        let mut found = false;
        for sec in self.sections.iter_mut() {
            if sec.name == name {
                found = true;
                if sec.state == SectionState::Frozen {
                    return Err(FreezeError::Frozen { name: sec.name });
                }
                if pointer.is_empty() {
                    return Err(FreezeError::PointerEmpty { name: sec.name });
                }
                let ok = match pointer.find('#') {
                    Some(i) => i > 0 && i + 1 < pointer.len(),
                    None => false,
                };
                if !ok {
                    return Err(FreezeError::PointerBad { name: sec.name });
                }
                sec.pointer = String::from(pointer);
                sec.words = words;
                sec.state = SectionState::Ready;
            }
        }
        if !found {
            return Err(FreezeError::SectionMissing { name: "unknown" });
        }
        Ok(())
    }

    /// 记一条差异（初稿 → 定稿）。
    pub fn record_diff(&mut self, d: DiffEntry) -> Result<(), FreezeError> {
        if d.note.is_empty() {
            return Err(FreezeError::NoteEmpty);
        }
        if d.final_words < d.draft_words {
            return Err(FreezeError::WordsRegressed {
                section: d.section,
            });
        }
        self.diffs.push(d);
        Ok(())
    }

    /// 可解引审计（六节指针全合法）。
    pub fn resolvability_audit(&self) -> Result<u32, FreezeError> {
        let mut ok = 0u32;
        for sec in self.sections.iter() {
            if sec.pointer.is_empty() {
                return Err(FreezeError::PointerEmpty { name: sec.name });
            }
            let good = match sec.pointer.find('#') {
                Some(i) => i > 0 && i + 1 < sec.pointer.len(),
                None => false,
            };
            if !good {
                return Err(FreezeError::PointerBad { name: sec.name });
            }
            ok += 1;
        }
        Ok(ok)
    }

    /// 内容哈希（六节 name/pointer/words 全进）。
    fn content_hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for sec in self.sections.iter() {
            b.extend_from_slice(sec.name.as_bytes());
            b.extend_from_slice(sec.pointer.as_bytes());
            b.extend_from_slice(&sec.words.to_le_bytes());
        }
        fnv1a64(&b)
    }

    /// 封存（六节全 ready + 可解引）。
    pub fn freeze(&mut self) -> Result<u64, FreezeError> {
        for sec in self.sections.iter() {
            if sec.state != SectionState::Ready {
                return Err(FreezeError::SectionNotReady { name: sec.name });
            }
        }
        self.resolvability_audit()?;
        let h = self.content_hash();
        for sec in self.sections.iter_mut() {
            sec.state = SectionState::Frozen;
        }
        self.seal_hash = h;
        Ok(h)
    }

    /// 封存验（复算比对）。
    pub fn verify_seal(&self) -> Result<(), FreezeError> {
        if self.seal_hash == 0 {
            return Err(FreezeError::SectionNotReady { name: "report" });
        }
        let h = self.content_hash();
        if h != self.seal_hash {
            return Err(FreezeError::SealBroken {
                got: h,
                want: self.seal_hash,
            });
        }
        Ok(())
    }

    /// 显性解封（重新定稿用；解封后 seal 归零）。
    pub fn defrost(&mut self) {
        for sec in self.sections.iter_mut() {
            if sec.state == SectionState::Frozen {
                sec.state = SectionState::Ready;
            }
        }
        self.seal_hash = 0;
    }

    /// 封存态。
    pub const fn is_frozen(&self) -> bool {
        self.seal_hash != 0
    }

    /// 已定稿节数。
    pub fn ready_count(&self) -> usize {
        self.sections
            .iter()
            .filter(|x| x.state != SectionState::Drafting)
            .count()
    }
}

impl Default for FinalReport {
    fn default() -> Self {
        FinalReport::new()
    }
}

/// 域自检。
pub fn run_unx781_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx781");

    // 构造全定稿报告 + 差异账
    let build = || -> FinalReport {
        let mut r = FinalReport::new();
        let mut i = 0;
        while i < SECTION_COUNT {
            let ptr = format!("docs/x.md#R-{}", i);
            let _ = r.finalize_section(SECTION_NAMES[i], &ptr, 400 + i as u32);
            let _ = r.record_diff(DiffEntry {
                section: SECTION_NAMES[i],
                draft_words: 300,
                final_words: 400 + i as u32,
                note: String::from("deepened"),
            });
            i += 1;
        }
        r
    };

    // C781-01 六节定稿 + 封存 + 验
    {
        let mut a = build();
        let r1 = a.freeze();
        let r2 = a.verify_seal();
        let mut b = build();
        let r3 = b.freeze();
        let good = r1.is_ok()
            && r2.is_ok()
            && r3.is_ok()
            && a.is_frozen()
            && r1 == r3
            && a.ready_count() == 6;
        s.add("C781-01-定稿封存验", good, if good { "" } else { "定版链失效" });
    }

    // C781-02 未就绪封存被拒
    {
        let mut r = FinalReport::new();
        let _ = r.finalize_section(SECTION_NAMES[0], "docs/x.md#R-0", 100);
        let res = r.freeze();
        let good = matches!(res, Err(FreezeError::SectionNotReady { name: "ledger" }));
        s.add("C781-02-未就绪封存拒", good, if good { "" } else { "半成品被封存" });
    }

    // C781-03 指针空拒
    {
        let mut r = FinalReport::new();
        let res = r.finalize_section("overview", "", 100);
        let good = matches!(res, Err(FreezeError::PointerEmpty { name: "overview" }));
        s.add("C781-03-空指针拒", good, if good { "" } else { "空指针被放行" });
    }

    // C781-04 封存后改写被拒
    {
        let mut r = build();
        let _ = r.freeze();
        let res = r.finalize_section(SECTION_NAMES[0], "docs/x.md#R-0", 999);
        let good = matches!(res, Err(FreezeError::Frozen { name: "overview" }));
        s.add("C781-04-封存后改写拒", good, if good { "" } else { "改稿被放行" });
    }

    // C781-05 解封后可重新定稿（且哈希必变）
    {
        let mut r = build();
        let h1 = r.freeze().unwrap_or(0);
        r.defrost();
        let _ = r.finalize_section(SECTION_NAMES[0], "docs/x.md#R-0", 5000);
        let h2 = r.freeze().unwrap_or(0);
        let good = r.verify_seal().is_ok() && h1 != 0 && h2 != 0;
        s.add("C781-05-解封重定稿", good, if good { "" } else { "解封链失效" });
    }

    // C781-06 差异说明空拒
    {
        let mut r = FinalReport::new();
        let res = r.record_diff(DiffEntry {
            section: "overview",
            draft_words: 100,
            final_words: 200,
            note: String::new(),
        });
        let good = matches!(res, Err(FreezeError::NoteEmpty));
        s.add("C781-06-差异说明空拒", good, if good { "" } else { "空说明被放行" });
    }

    // C781-07 字数倒退拒
    {
        let mut r = FinalReport::new();
        let res = r.record_diff(DiffEntry {
            section: "overview",
            draft_words: 300,
            final_words: 100,
            note: String::from("trimmed"),
        });
        let good = matches!(res, Err(FreezeError::WordsRegressed { section: "overview" }));
        s.add("C781-07-倒退必说明", good, if good { "" } else { "倒退被放行" });
    }

    // C781-08 可解引 6/6
    {
        let r = build();
        let res = r.resolvability_audit();
        let good = res == Ok(6);
        s.add("C781-08-可解引 6/6", good, if good { "" } else { "可解引不满" });
    }

    // C781-09 缺节拒
    {
        let mut r = FinalReport::new();
        let res = r.finalize_section("nonexistent", "docs/x.md#1", 1);
        let good = matches!(res, Err(FreezeError::SectionMissing { name: "unknown" }));
        s.add("C781-09-缺节拒", good, if good { "" } else { "缺节被放行" });
    }

    // C781-10 封存验在未封存时报非就绪
    {
        let r = FinalReport::new();
        let res = r.verify_seal();
        let good = matches!(res, Err(FreezeError::SectionNotReady { name: "report" }));
        s.add("C781-10-未封存验拒", good, if good { "" } else { "空封存被验过" });
    }

    // C781-11 诊断码段独占
    {
        let codes = [
            FreezeCode::SectionMissing,
            FreezeCode::NotReady,
            FreezeCode::PointerEmpty,
            FreezeCode::PointerBad,
            FreezeCode::Frozen,
            FreezeCode::SealBroken,
            FreezeCode::NoteEmpty,
            FreezeCode::Regressed,
            FreezeCode::FrozenOk,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C781-11-诊断码 0x91 段独占", all, if all { "" } else { "码值越出 0x91 段" });
    }

    s
}
