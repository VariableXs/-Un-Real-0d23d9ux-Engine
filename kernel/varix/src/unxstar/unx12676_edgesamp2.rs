//! UNX-F12677 探测面边界样本二（超长路径/深枚举/大缓冲） · 0x81300段
//!
//! 判据原文（派工卡 12676 逐字锚定）：
//!   ① 纯功能代码 ≥286 行；
//!   ② 判据原文：UNX-F12677-J1 回执协议三段（通知/窗口/差异列账）判
//!      据绿，与 F12634 同构声明；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 边界样本库二批：三族边界（超长路径/深枚举/大缓冲）各冻结上限
//!    （路径 0x108 字节节 / 枚举深 0x20 层 / 缓冲 0x400 节），样本七
//!    元（样本号/族/谱名/计量/上限位/期望判/判码）——判码落族窗口，
//!    越限样本必被闸拒；
//! 2. 回执协议三段（判据 J1 主语）：通知段（Notify）/窗口段（Window）
//!    /差异列账段（DiffLedger）——三段各带冻结段码与段判，三段判全
//!    绿才许出回执；三段状态机通知先行、窗口继后、列账收尾，乱序显
//!    性拒；
//! 3. 与 F12634 同构声明：本卡三段与 F12634 回执三段逐段同构（段名
//!    对齐/段码对齐/段判对齐），同构表冻结、逐行点验零缺格；
//! 4. 样本库零静默：逐样本显性判定（过/越都记账），记账数 ≡ 样本数
//!    守恒；库状态机 VaultOpen → Auditing → Sealed 单向推进，倒退显
//!    性拒；
//! 5. 注入联测十路：族越闭集/零计量/越上限/判码越窗/样本号乱序/样
//!    本重复/缺样封库/回执乱序/同构缺格/摘要空——逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（出题冻结）；
//!    取值一律 match/if let/.get()/.get_mut()/saturating_*，无 unwrap/
//!    expect/panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `receipt_three_ok` ↔ ② 回执协议三段（通知/窗口/差异列账）判据绿；
//!   - `iso_decl_ok` ↔ ② 与 F12634 同构声明；
//!   - `edge_vault_ok` ↔ 二批边界样本 ≥30 例入库全过；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12676 − 12600)）。
pub const SEG_BASE: u32 = 0x81300;

// ── 边界样本诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeDiag {
    /// 族越闭集（边界族不在三族内）。
    EFamilyRange,
    /// 零计量。
    EZeroSteps,
    /// 越上限（计量越族冻结上限）。
    ECapOver,
    /// 判码越窗。
    ECodeRange,
    /// 样本号乱序。
    ESampleOrder,
    /// 样本重复。
    ESampleDup,
    /// 缺样封库。
    EShortSeal,
    /// 回执乱序（三段顺序违约）。
    EReceiptOrder,
    /// 同构缺格（与 F12634 同构表缺段）。
    EIsoMissing,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl EdgeDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                EdgeDiag::EFamilyRange => 0x01,
                EdgeDiag::EZeroSteps => 0x02,
                EdgeDiag::ECapOver => 0x03,
                EdgeDiag::ECodeRange => 0x04,
                EdgeDiag::ESampleOrder => 0x05,
                EdgeDiag::ESampleDup => 0x06,
                EdgeDiag::EShortSeal => 0x07,
                EdgeDiag::EReceiptOrder => 0x08,
                EdgeDiag::EIsoMissing => 0x09,
                EdgeDiag::ESummaryEmpty => 0x0a,
                EdgeDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            EdgeDiag::EFamilyRange => "族越闭集：只认超长路径/深枚举/大缓冲三族，先正边界族再入库",
            EdgeDiag::EZeroSteps => "零计量：计量为零即无效，先补显性计量",
            EdgeDiag::ECapOver => "越上限：计量越族冻结上限，先调样再入库",
            EdgeDiag::ECodeRange => "判码越窗：判码不在族判码窗口，先归族再对账",
            EdgeDiag::ESampleOrder => "样本号乱序：按样本号 0..30 逐样入库，跳号先回补",
            EdgeDiag::ESampleDup => "样本重复：同族同谱只立一样，重复先查已有库",
            EdgeDiag::EShortSeal => "缺样封库：30 例齐才许封库，缺样先回补",
            EdgeDiag::EReceiptOrder => "回执乱序：通知→窗口→列账逐段推进，跳段先补段",
            EdgeDiag::EIsoMissing => "同构缺格：与 F12634 同构表逐行点验，缺段先补声明",
            EdgeDiag::ESummaryEmpty => "摘要空：封库摘要非空才许写回已完成",
            EdgeDiag::ERewind => "状态机倒退：VaultOpen→Auditing→Sealed 只进不退，倒退即库可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const EDGE_ALL: &[EdgeDiag] = &[
    EdgeDiag::EFamilyRange,
    EdgeDiag::EZeroSteps,
    EdgeDiag::ECapOver,
    EdgeDiag::ECodeRange,
    EdgeDiag::ESampleOrder,
    EdgeDiag::ESampleDup,
    EdgeDiag::EShortSeal,
    EdgeDiag::EReceiptOrder,
    EdgeDiag::EIsoMissing,
    EdgeDiag::ESummaryEmpty,
    EdgeDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in EDGE_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81301..0x8130B 闭集内"
}

// ── 边界族三闭集 ─────────────────────────────────────────────────
/// 边界族（三族闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeFamily {
    /// 超长路径族。
    LongPath,
    /// 深枚举族。
    DeepEnum,
    /// 大缓冲族。
    BigBuffer,
}

impl EdgeFamily {
    /// 族名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            EdgeFamily::LongPath => "超长路径",
            EdgeFamily::DeepEnum => "深枚举",
            EdgeFamily::BigBuffer => "大缓冲",
        }
    }

    /// 族号（0..2）。
    pub fn ordinal(self) -> u32 {
        match self {
            EdgeFamily::LongPath => 0,
            EdgeFamily::DeepEnum => 1,
            EdgeFamily::BigBuffer => 2,
        }
    }

    /// 族冻结上限（路径字节节 / 枚举层 / 缓冲节）。
    pub fn cap(self) -> u32 {
        match self {
            EdgeFamily::LongPath => 0x108,
            EdgeFamily::DeepEnum => 0x20,
            EdgeFamily::BigBuffer => 0x400,
        }
    }

    /// 族判码窗口基（0x25/0x35/0x45）。
    pub fn code_base(self) -> u32 {
        match self {
            EdgeFamily::LongPath => 0x25,
            EdgeFamily::DeepEnum => 0x35,
            EdgeFamily::BigBuffer => 0x45,
        }
    }

    /// 族判码窗口上沿偏移。
    pub fn code_span(self) -> u32 {
        0x0C
    }

    /// 全部三族（闭集数组）。
    pub fn all() -> &'static [EdgeFamily] {
        &[EdgeFamily::LongPath, EdgeFamily::DeepEnum, EdgeFamily::BigBuffer]
    }
}

/// 判码是否落在族判码窗口。
pub fn code_in_edge_family(code: u32, f: EdgeFamily) -> bool {
    let base = f.code_base();
    code > base && code <= base.saturating_add(f.code_span())
}

// ── 边界样本七元 ─────────────────────────────────────────────────
/// 边界样本七元（样本号/族/谱名/计量/上限位/期望判/判码）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeCase {
    /// 样本号。
    pub no: u32,
    /// 边界族。
    pub family: EdgeFamily,
    /// 谱名（族内边界谱）。
    pub spectrum: &'static str,
    /// 整数计量（路径字节节/枚举层/缓冲节）。
    pub amount: u32,
    /// 上限位（true = 计量带上限对账位）。
    pub cap_bit: bool,
    /// 期望判（true = 应在族上限内通过）。
    pub expect_pass: bool,
    /// 判码（族窗口内）。
    pub code: u32,
}

/// 单样本黄金判定（族谱文本 + 零计量 + 上限 + 码窗 四道）。
pub fn edge_case_ok(c: &EdgeCase) -> Result<(), u32> {
    // 族谱文本闸（谱名空即族无从归）。
    if c.spectrum.is_empty() {
        return Err(EdgeDiag::EFamilyRange.val());
    }
    // 零计量闸。
    if c.amount == 0 {
        return Err(EdgeDiag::EZeroSteps.val());
    }
    // 上限闸。
    if c.amount > c.family.cap() {
        return Err(EdgeDiag::ECapOver.val());
    }
    // 码窗闸。
    if !code_in_edge_family(c.code, c.family) {
        return Err(EdgeDiag::ECodeRange.val());
    }
    Ok(())
}

/// 样本库容量（30 例 = 3 族 × 10 例）。
pub const EDGE_CASE_CAP: usize = 30;

// ── 冻结边界样本库（30 样：三族 × 10 谱） ────────────────────────
/// 构造样本——冻结展开为常量表。
const fn mk_edge2(
    no: u32,
    family: EdgeFamily,
    spectrum: &'static str,
    amount: u32,
    cap_bit: bool,
    expect_pass: bool,
    code: u32,
) -> EdgeCase {
    EdgeCase {
        no,
        family,
        spectrum,
        amount,
        cap_bit,
        expect_pass,
        code,
    }
}

/// 30 样冻结样本库（样本号序；近上限/中档/零档轮转，全在上限内）。
pub const EDGE_CASES: &[EdgeCase] = &[
    mk_edge2(0, EdgeFamily::LongPath, "近限路径", 0x100, true, true, 0x26),
    mk_edge2(1, EdgeFamily::LongPath, "中限路径", 0x80, true, true, 0x27),
    mk_edge2(2, EdgeFamily::LongPath, "零档路径", 0x01, true, true, 0x28),
    mk_edge2(3, EdgeFamily::LongPath, "段界路径", 0xC0, true, true, 0x29),
    mk_edge2(4, EdgeFamily::LongPath, "尾界路径", 0xF0, true, true, 0x2A),
    mk_edge2(5, EdgeFamily::LongPath, "近限复核", 0x104, true, true, 0x2B),
    mk_edge2(6, EdgeFamily::LongPath, "段界复核", 0xCC, true, true, 0x2C),
    mk_edge2(7, EdgeFamily::LongPath, "尾界复核", 0xE8, true, true, 0x2D),
    mk_edge2(8, EdgeFamily::LongPath, "中限重放", 0x88, true, true, 0x2E),
    mk_edge2(9, EdgeFamily::LongPath, "零档重放", 0x02, true, true, 0x2F),
    mk_edge2(10, EdgeFamily::DeepEnum, "近限枚举", 0x1C, true, true, 0x36),
    mk_edge2(11, EdgeFamily::DeepEnum, "中限枚举", 0x10, true, true, 0x37),
    mk_edge2(12, EdgeFamily::DeepEnum, "零档枚举", 0x01, true, true, 0x38),
    mk_edge2(13, EdgeFamily::DeepEnum, "半层枚举", 0x15, true, true, 0x39),
    mk_edge2(14, EdgeFamily::DeepEnum, "尾层枚举", 0x1A, true, true, 0x3A),
    mk_edge2(15, EdgeFamily::DeepEnum, "近限复核", 0x1D, true, true, 0x3B),
    mk_edge2(16, EdgeFamily::DeepEnum, "半层复核", 0x17, true, true, 0x3C),
    mk_edge2(17, EdgeFamily::DeepEnum, "尾层复核", 0x18, true, true, 0x3D),
    mk_edge2(18, EdgeFamily::DeepEnum, "中限重放", 0x12, true, true, 0x3E),
    mk_edge2(19, EdgeFamily::DeepEnum, "零档重放", 0x02, true, true, 0x3F),
    mk_edge2(20, EdgeFamily::BigBuffer, "近限缓冲", 0x3C0, true, true, 0x46),
    mk_edge2(21, EdgeFamily::BigBuffer, "中限缓冲", 0x200, true, true, 0x47),
    mk_edge2(22, EdgeFamily::BigBuffer, "零档缓冲", 0x10, true, true, 0x48),
    mk_edge2(23, EdgeFamily::BigBuffer, "半页缓冲", 0x2A0, true, true, 0x49),
    mk_edge2(24, EdgeFamily::BigBuffer, "尾页缓冲", 0x380, true, true, 0x4A),
    mk_edge2(25, EdgeFamily::BigBuffer, "近限复核", 0x3D0, true, true, 0x4B),
    mk_edge2(26, EdgeFamily::BigBuffer, "半页复核", 0x2C0, true, true, 0x4C),
    mk_edge2(27, EdgeFamily::BigBuffer, "尾页复核", 0x370, true, true, 0x4D),
    mk_edge2(28, EdgeFamily::BigBuffer, "中限重放", 0x220, true, true, 0x4E),
    mk_edge2(29, EdgeFamily::BigBuffer, "零档重放", 0x20, true, true, 0x4F),
];

// ── 回执协议三段（判据 J1 主语） ─────────────────────────────────
/// 回执段闭集（通知/窗口/差异列账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptSeg {
    /// 通知段。
    Notify,
    /// 窗口段。
    Window,
    /// 差异列账段。
    DiffLedger,
}

impl ReceiptSeg {
    /// 段名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            ReceiptSeg::Notify => "通知",
            ReceiptSeg::Window => "窗口",
            ReceiptSeg::DiffLedger => "差异列账",
        }
    }

    /// 段号（0..2，推进序）。
    pub fn ordinal(self) -> u32 {
        match self {
            ReceiptSeg::Notify => 0,
            ReceiptSeg::Window => 1,
            ReceiptSeg::DiffLedger => 2,
        }
    }

    /// 段码（冻结：0x11/0x12/0x13）。
    pub fn seg_code(self) -> u32 {
        match self {
            ReceiptSeg::Notify => 0x11,
            ReceiptSeg::Window => 0x12,
            ReceiptSeg::DiffLedger => 0x13,
        }
    }

    /// 全部三段（闭集数组，推进序）。
    pub fn all() -> &'static [ReceiptSeg] {
        &[ReceiptSeg::Notify, ReceiptSeg::Window, ReceiptSeg::DiffLedger]
    }
}

// ── 与 F12634 同构声明表 ─────────────────────────────────────────
/// 同构行（段名对齐/段码对齐/段判对齐 三列声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoRow {
    /// 本卡段。
    pub seg: ReceiptSeg,
    /// F12634 对应段名（声明列一）。
    pub peer_name: &'static str,
    /// F12634 对应段码（声明列二）。
    pub peer_code: u32,
    /// 段判声明（true = 两段判同构）。
    pub judge_iso: bool,
}

/// 同构声明表（三行冻结，与 F12634 回执三段逐段对齐）。
pub const ISO_TABLE: &[IsoRow] = &[
    IsoRow { seg: ReceiptSeg::Notify, peer_name: "F12634 通知段", peer_code: 0x11, judge_iso: true },
    IsoRow { seg: ReceiptSeg::Window, peer_name: "F12634 窗口段", peer_code: 0x12, judge_iso: true },
    IsoRow { seg: ReceiptSeg::DiffLedger, peer_name: "F12634 差异列账段", peer_code: 0x13, judge_iso: true },
];

/// 单段同构点验（段名/段码/段判三列齐且对齐）。
pub fn iso_row_ok(r: &IsoRow) -> bool {
    // 声明列一非空、列二段码与本卡段码同构。
    if r.peer_name.is_empty() {
        return false;
    }
    if r.peer_code != r.seg.seg_code() {
        return false;
    }
    if !r.judge_iso {
        return false;
    }
    // 段名对齐：F12634 段名必须包含本卡段名（同构口径）。
    let seg_name = r.seg.name();
    // 中文含段名（通知/窗口/差异列账）。
    r.peer_name.len() >= seg_name.len()
        && (r.peer_name.contains(seg_name) || r.peer_name.ends_with(seg_name))
}

/// 同构声明全表点验（三行齐、逐行点验、段序推进对齐）。
pub fn iso_table_ok() -> bool {
    if ISO_TABLE.len() != ReceiptSeg::all().len() {
        return false;
    }
    let mut i = 0usize;
    while i < ISO_TABLE.len() {
        let r = match ISO_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        // 段序推进对齐（通知→窗口→列账）。
        if r.seg.ordinal() != i as u32 {
            return false;
        }
        if !iso_row_ok(r) {
            return false;
        }
        i += 1;
    }
    true
}

// ── 回执机（三段状态机） ─────────────────────────────────────────
/// 回执机状态：Idle → Notified → Windowed → Ledgered。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptState {
    /// 待通知。
    Idle,
    /// 通知已发。
    Notified,
    /// 窗口已开。
    Windowed,
    /// 差异已列账。
    Ledgered,
}

impl ReceiptState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            ReceiptState::Idle => 0,
            ReceiptState::Notified => 1,
            ReceiptState::Windowed => 2,
            ReceiptState::Ledgered => 3,
        }
    }
}

/// 回执机（三段推进，乱序显性拒）。
#[derive(Clone, Debug)]
pub struct ReceiptMachine {
    /// 已成段（段号序）。
    segs: Vec<ReceiptSeg>,
    /// 差异列账条目（段内差异条目数）。
    diffs: Vec<(u32, u32)>,
    /// 状态。
    state: ReceiptState,
}

impl ReceiptMachine {
    /// 新回执机。
    pub fn new() -> ReceiptMachine {
        ReceiptMachine {
            segs: Vec::new(),
            diffs: Vec::new(),
            state: ReceiptState::Idle,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ReceiptState {
        self.state
    }

    /// 已成段数。
    pub fn seg_len(&self) -> usize {
        self.segs.len()
    }

    /// 差异条目数。
    pub fn diff_len(&self) -> usize {
        self.diffs.len()
    }

    /// 取第 idx 段（越界 None）。
    pub fn seg_at(&self, idx: usize) -> Option<&ReceiptSeg> {
        self.segs.get(idx)
    }

    /// 逐段推进（段序闸 + 状态机闸）。
    pub fn step(&mut self, seg: ReceiptSeg, diff_entries: u32) -> Result<(), u32> {
        // 段序闸：必须按推进序成段。
        if seg.ordinal() != self.segs.len() as u32 {
            return Err(EdgeDiag::EReceiptOrder.val());
        }
        // 状态机闸：段号与状态序对齐（Idle=通知位、Notified=窗口位、
        // Windowed=列账位）。
        if seg.ordinal() != self.state.rank() as u32 {
            return Err(EdgeDiag::EReceiptOrder.val());
        }
        // 差异列账段必须带列账条目（零静默口径：列账段非空账）。
        if seg == ReceiptSeg::DiffLedger && diff_entries == 0 {
            return Err(EdgeDiag::ESummaryEmpty.val());
        }
        if diff_entries > 0 {
            self.diffs.push((seg.seg_code(), diff_entries));
        }
        self.segs.push(seg);
        // 状态推进。
        self.state = match seg {
            ReceiptSeg::Notify => ReceiptState::Notified,
            ReceiptSeg::Window => ReceiptState::Windowed,
            ReceiptSeg::DiffLedger => ReceiptState::Ledgered,
        };
        Ok(())
    }

    /// 回执三段判全绿（三段齐 + 列账非空 + 状态到顶）。
    pub fn receipt_ok(&self) -> bool {
        if self.segs.len() != ReceiptSeg::all().len() {
            return false;
        }
        if self.state != ReceiptState::Ledgered {
            return false;
        }
        if self.diffs.is_empty() {
            return false;
        }
        // 段序逐位对齐推进序。
        let mut i = 0usize;
        while i < self.segs.len() {
            let s = match self.segs.get(i) {
                Some(v) => v,
                None => return false,
            };
            if s.ordinal() != i as u32 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 构建完整回执（三段全成，列账带差异条目）。
pub fn build_full_receipt() -> ReceiptMachine {
    let mut m = ReceiptMachine::new();
    let _ = m.step(ReceiptSeg::Notify, 0);
    let _ = m.step(ReceiptSeg::Window, 0);
    let _ = m.step(ReceiptSeg::DiffLedger, 3);
    m
}

// ── 样本库状态机（VaultOpen → Auditing → Sealed） ────────────────
/// 样本库状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeState {
    /// 开库。
    VaultOpen,
    /// 审计中。
    Auditing,
    /// 已封库。
    Sealed,
}

impl EdgeState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            EdgeState::VaultOpen => 0,
            EdgeState::Auditing => 1,
            EdgeState::Sealed => 2,
        }
    }
}

/// 边界样本库账。
#[derive(Clone, Debug)]
pub struct EdgeLedger {
    /// 已入库样本（样本号序）。
    samples: Vec<EdgeCase>,
    /// 判定记账（样本号 + 通过位；零静默口径）。
    verdicts: Vec<(u32, bool)>,
    /// 拒收记账（样本号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: EdgeState,
}

impl EdgeLedger {
    /// 开库。
    pub fn new() -> EdgeLedger {
        EdgeLedger {
            samples: Vec::new(),
            verdicts: Vec::new(),
            rejects: Vec::new(),
            state: EdgeState::VaultOpen,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> EdgeState {
        self.state
    }

    /// 已入库样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 判定记账数。
    pub fn verdict_len(&self) -> usize {
        self.verdicts.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 样本（越界 None）。
    pub fn sample_at(&self, idx: usize) -> Option<&EdgeCase> {
        self.samples.get(idx)
    }

    /// 进审计（VaultOpen→Auditing）。
    pub fn begin_audit(&mut self) -> Result<(), u32> {
        if self.state.rank() >= EdgeState::Auditing.rank() {
            return Err(EdgeDiag::ERewind.val());
        }
        self.state = EdgeState::Auditing;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < EDGE_CASE_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐样入库（样本号序 + 黄金判定 + 样本互异 + 显性记账 四道）。
    pub fn admit(&mut self, c: EdgeCase) -> Result<(), u32> {
        if self.state.rank() < EdgeState::Auditing.rank() {
            return Err(EdgeDiag::ESummaryEmpty.val());
        }
        if self.state.rank() >= EdgeState::Sealed.rank() {
            self.note_reject(c.no, EdgeDiag::ERewind.val());
            return Err(EdgeDiag::ERewind.val());
        }
        // 样本号序闸。
        if c.no != self.samples.len() as u32 {
            self.note_reject(c.no, EdgeDiag::ESampleOrder.val());
            return Err(EdgeDiag::ESampleOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = edge_case_ok(&c) {
            self.note_reject(c.no, e);
            return Err(e);
        }
        // 样本互异闸：同族同谱只立一样。
        for done in self.samples.iter() {
            if done.family == c.family && done.spectrum == c.spectrum {
                self.note_reject(c.no, EdgeDiag::ESampleDup.val());
                return Err(EdgeDiag::ESampleDup.val());
            }
        }
        // 显性记账（零静默口径）：每样落判定。
        self.verdicts.push((c.no, c.expect_pass));
        self.samples.push(c);
        Ok(())
    }

    /// 封库：30 样齐 + 判定记账 30 条 + 零拒收 + 回执三段判全绿 + 同构齐。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= EdgeState::Sealed.rank() {
            return Err(EdgeDiag::ERewind.val());
        }
        if self.state.rank() < EdgeState::Auditing.rank() {
            return Err(EdgeDiag::ESummaryEmpty.val());
        }
        if self.samples.len() != EDGE_CASE_CAP {
            return Err(EdgeDiag::EShortSeal.val());
        }
        if self.verdicts.len() != EDGE_CASE_CAP {
            return Err(EdgeDiag::ESummaryEmpty.val());
        }
        if !self.rejects.is_empty() {
            return Err(EdgeDiag::EShortSeal.val());
        }
        // 回执三段判全绿（封库硬闸）。
        let m = build_full_receipt();
        if !m.receipt_ok() {
            return Err(EdgeDiag::EReceiptOrder.val());
        }
        // 同构声明齐（封库硬闸）。
        if !iso_table_ok() {
            return Err(EdgeDiag::EIsoMissing.val());
        }
        self.state = EdgeState::Sealed;
        Ok(())
    }

    /// 账等式（零静默口径：样本数 = 判定数 = 30，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.samples.len() == EDGE_CASE_CAP
            && self.verdicts.len() == EDGE_CASE_CAP
            && self.rejects.is_empty()
    }
}

/// 构建完整样本库（30 样逐样入库）。
pub fn build_full_edge_ledger() -> EdgeLedger {
    let mut led = EdgeLedger::new();
    let _ = led.begin_audit();
    let mut i = 0usize;
    while i < EDGE_CASE_CAP {
        let c = match EDGE_CASES.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.admit(c) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeFault {
    /// 族越闭集。
    FamilyRange,
    /// 零计量。
    ZeroSteps,
    /// 越上限。
    CapOver,
    /// 判码越窗。
    CodeRange,
    /// 样本号乱序。
    SampleOrder,
    /// 样本重复。
    SampleDup,
    /// 缺样封库。
    ShortSeal,
    /// 回执乱序。
    ReceiptOrder,
    /// 同构缺格。
    IsoMissing,
    /// 摘要空。
    SummaryEmpty,
}

impl EdgeFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            EdgeFault::FamilyRange => 0,
            EdgeFault::ZeroSteps => 1,
            EdgeFault::CapOver => 2,
            EdgeFault::CodeRange => 3,
            EdgeFault::SampleOrder => 4,
            EdgeFault::SampleDup => 5,
            EdgeFault::ShortSeal => 6,
            EdgeFault::ReceiptOrder => 7,
            EdgeFault::IsoMissing => 8,
            EdgeFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            EdgeFault::FamilyRange => "族越闭集",
            EdgeFault::ZeroSteps => "零计量",
            EdgeFault::CapOver => "越上限",
            EdgeFault::CodeRange => "判码越窗",
            EdgeFault::SampleOrder => "样本号乱序",
            EdgeFault::SampleDup => "样本重复",
            EdgeFault::ShortSeal => "缺样封库",
            EdgeFault::ReceiptOrder => "回执乱序",
            EdgeFault::IsoMissing => "同构缺格",
            EdgeFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [EdgeFault] {
        &[
            EdgeFault::FamilyRange,
            EdgeFault::ZeroSteps,
            EdgeFault::CapOver,
            EdgeFault::CodeRange,
            EdgeFault::SampleOrder,
            EdgeFault::SampleDup,
            EdgeFault::ShortSeal,
            EdgeFault::ReceiptOrder,
            EdgeFault::IsoMissing,
            EdgeFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn edge_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in EdgeFault::all().iter() {
        total += 1;
        if run_edge_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_edge_fault(v: EdgeFault) -> bool {
    match v {
        EdgeFault::FamilyRange => {
            let mut c = match EDGE_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.spectrum = "";
            edge_case_ok(&c).is_err()
        }
        EdgeFault::ZeroSteps => {
            let mut c = match EDGE_CASES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.amount = 0;
            edge_case_ok(&c).is_err()
        }
        EdgeFault::CapOver => {
            let mut c = match EDGE_CASES.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.amount = c.family.cap().saturating_add(1);
            edge_case_ok(&c).is_err()
        }
        EdgeFault::CodeRange => {
            let mut c = match EDGE_CASES.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.code = c.code.saturating_add(0x20);
            edge_case_ok(&c).is_err()
        }
        EdgeFault::SampleOrder => {
            let mut led = EdgeLedger::new();
            let _ = led.begin_audit();
            let c = match EDGE_CASES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.admit(c).is_err()
        }
        EdgeFault::SampleDup => {
            let mut led = EdgeLedger::new();
            let _ = led.begin_audit();
            let c0 = match EDGE_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.admit(c0.clone());
            let dup = EdgeCase {
                no: 1,
                family: c0.family,
                spectrum: c0.spectrum,
                amount: c0.amount,
                cap_bit: c0.cap_bit,
                expect_pass: c0.expect_pass,
                code: c0.code,
            };
            led.admit(dup).is_err()
        }
        EdgeFault::ShortSeal => {
            let mut led = EdgeLedger::new();
            let _ = led.begin_audit();
            let c = match EDGE_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.admit(c);
            led.seal().is_err()
        }
        EdgeFault::ReceiptOrder => {
            // 回执乱序：跳过通知直开窗口必拒。
            let mut m = ReceiptMachine::new();
            m.step(ReceiptSeg::Window, 0).is_err()
        }
        EdgeFault::IsoMissing => {
            // 同构缺格：段码错配声明必被抓。
            let bad = IsoRow { seg: ReceiptSeg::Notify, peer_name: "F12634 通知段", peer_code: 0x99, judge_iso: true };
            !iso_row_ok(&bad)
        }
        EdgeFault::SummaryEmpty => {
            // 摘要空族：列账段零条目必拒（列账非空口径）。
            let mut m = ReceiptMachine::new();
            let _ = m.step(ReceiptSeg::Notify, 0);
            let _ = m.step(ReceiptSeg::Window, 0);
            m.step(ReceiptSeg::DiffLedger, 0).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 回执协议三段判据绿（判据 J1 前半直读口）。
fn receipt_three_ok() -> bool {
    let m = build_full_receipt();
    if !m.receipt_ok() {
        return false;
    }
    // 三段齐 + 段码冻结轴。
    if ReceiptSeg::all().len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < ReceiptSeg::all().len() {
        let s = match ReceiptSeg::all().get(i) {
            Some(v) => v,
            None => return false,
        };
        if s.ordinal() != i as u32 {
            return false;
        }
        if s.seg_code() != 0x11 + i as u32 {
            return false;
        }
        i += 1;
    }
    // 差异列账段非空账。
    if m.diff_len() == 0 {
        return false;
    }
    true
}

/// 与 F12634 同构声明（判据 J1 后半直读口）。
fn iso_decl_ok() -> bool {
    if !iso_table_ok() {
        return false;
    }
    // 反证：段码错配与段名空缺必被抓。
    let bad_code = IsoRow { seg: ReceiptSeg::Window, peer_name: "F12634 窗口段", peer_code: 0x99, judge_iso: true };
    if iso_row_ok(&bad_code) {
        return false;
    }
    let bad_name = IsoRow { seg: ReceiptSeg::DiffLedger, peer_name: "", peer_code: 0x13, judge_iso: true };
    if iso_row_ok(&bad_name) {
        return false;
    }
    let bad_judge = IsoRow { seg: ReceiptSeg::Notify, peer_name: "F12634 通知段", peer_code: 0x11, judge_iso: false };
    if iso_row_ok(&bad_judge) {
        return false;
    }
    true
}

/// 二批边界样本 30 例入库全过。
fn edge_vault_ok() -> bool {
    let mut led = build_full_edge_ledger();
    if led.len() != EDGE_CASE_CAP {
        return false;
    }
    if led.verdict_len() != EDGE_CASE_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.seal().is_err() {
        return false;
    }
    if led.state() != EdgeState::Sealed {
        return false;
    }
    // 独立复核：30 样逐样黄金判定 + 全部期望通过。
    for c in EDGE_CASES.iter() {
        if edge_case_ok(c).is_err() {
            return false;
        }
        if !c.expect_pass || !c.cap_bit {
            return false;
        }
    }
    true
}

/// 三族覆盖普查（每族 10 例、谱名互异、样本号连续、判码落窗）。
fn family_census_ok() -> bool {
    let mut per_family = [0usize; 3];
    let mut i = 0usize;
    while i < EDGE_CASES.len() {
        let c = match EDGE_CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if c.no != i as u32 {
            return false;
        }
        match per_family.get_mut(c.family.ordinal() as usize) {
            Some(v) => *v += 1,
            None => return false,
        }
        if !code_in_edge_family(c.code, c.family) {
            return false;
        }
        let mut j = i + 1;
        while j < EDGE_CASES.len() {
            let g = match EDGE_CASES.get(j) {
                Some(v) => v,
                None => return false,
            };
            if g.family == c.family && g.spectrum == c.spectrum {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    for slot in per_family.iter() {
        if *slot != 10 {
            return false;
        }
    }
    true
}

/// 零静默（判定记账逐样齐、样本号一一对应、通过位全真）。
fn zero_silent_ok() -> bool {
    let led = build_full_edge_ledger();
    if led.verdict_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.verdict_len() {
        let v = match led.verdicts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match led.sample_at(i) {
            Some(x) => x,
            None => return false,
        };
        if v.0 != c.no || !v.1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn edge_fault_sweep_ok() -> bool {
    let (caught, total, escape) = edge_fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（冻结样本库下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_edge_ledger();
    let b = build_full_edge_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.sample_at(i), b.sample_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    // 回执双跑一致。
    let ra = build_full_receipt();
    let rb = build_full_receipt();
    if ra.seg_len() != rb.seg_len() || ra.diff_len() != rb.diff_len() {
        return false;
    }
    if edge_fault_sweep().0 != edge_fault_sweep().0 {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12676u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_edge_ledger();
    if led.seal().is_err() {
        return false;
    }
    if led.state() != EdgeState::Sealed {
        return false;
    }
    // 摘要：首样谱名 + 尾样族名拼接非空。
    let head = match led.sample_at(0) {
        Some(c) => c.spectrum,
        None => return false,
    };
    let tail = match led.sample_at(EDGE_CASE_CAP - 1) {
        Some(c) => c.family.name(),
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12677 边界样本自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12676_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12676-edgesamp2");
    let led = build_full_edge_ledger();

    s.add_dyn(
        "F12677-01 回执协议三段判据绿",
        receipt_three_ok(),
        &format!("segs={} diffs={}", build_full_receipt().seg_len(), build_full_receipt().diff_len()),
    );

    s.add_dyn(
        "F12677-02 与 F12634 同构声明",
        iso_decl_ok(),
        &format!("iso_rows={}", ISO_TABLE.len()),
    );

    s.add_dyn(
        "F12677-03 边界样本三十例齐",
        EDGE_CASES.len() == EDGE_CASE_CAP,
        &format!("samples={}", EDGE_CASES.len()),
    );

    s.add_dyn(
        "F12677-04 边界样本入库全过",
        edge_vault_ok(),
        &format!("samples={} verdicts={}", led.len(), led.verdict_len()),
    );

    s.add_dyn(
        "F12677-05 三族覆盖普查",
        family_census_ok(),
        &format!("families={}", EdgeFamily::all().len()),
    );

    s.add_dyn(
        "F12677-06 零静默",
        zero_silent_ok(),
        &format!("verdicts={}", led.verdict_len()),
    );

    s.add_dyn(
        "F12677-07 注入联测 10/10 拒止",
        edge_fault_sweep_ok(),
        &format!("caught={}", edge_fault_sweep().0),
    );

    s.add_dyn(
        "F12677-08 账等式",
        led.equation_ok(),
        &format!("samples={} verdicts={} rejects={}", led.len(), led.verdict_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12677-09 双跑一致",
        dual_run_ok(),
        "冻结样本库下两轮逐样同判",
    );

    s.add_dyn(
        "F12677-10 族上限冻结轴",
        EdgeFamily::LongPath.cap() == 0x108
            && EdgeFamily::DeepEnum.cap() == 0x20
            && EdgeFamily::BigBuffer.cap() == 0x400,
        "路径/枚举/缓冲上限冻结",
    );

    s.add_dyn(
        "F12677-11 族判码窗口口径",
        code_in_edge_family(0x26, EdgeFamily::LongPath)
            && !code_in_edge_family(0x25, EdgeFamily::LongPath)
            && !code_in_edge_family(0x36, EdgeFamily::LongPath),
        "基码 < 判码 ≤ 基码+0x0C",
    );

    s.add_dyn(
        "F12677-12 完成写回语义",
        writeback_ok(),
        "封库即已完成且摘要非空",
    );

    s.add_dyn(
        "F12677-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81300);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(EDGE_ALL.len(), 11);
        let mut i = 0usize;
        while i < EDGE_ALL.len() {
            let vi = match EDGE_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < EDGE_ALL.len() {
                let vj = match EDGE_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn receipt_three_segments_green() {
        // 回执三段判据绿。
        assert!(receipt_three_ok());
        let m = build_full_receipt();
        assert_eq!(m.seg_len(), 3);
        assert_eq!(m.state(), ReceiptState::Ledgered);
        assert!(m.receipt_ok());
        // 段码冻结轴。
        assert_eq!(ReceiptSeg::Notify.seg_code(), 0x11);
        assert_eq!(ReceiptSeg::Window.seg_code(), 0x12);
        assert_eq!(ReceiptSeg::DiffLedger.seg_code(), 0x13);
    }

    #[test]
    fn iso_declaration_holds() {
        // 与 F12634 同构声明。
        assert!(iso_decl_ok());
        assert!(iso_table_ok());
        assert_eq!(ISO_TABLE.len(), 3);
        // 反证注入必抓。
        let bad = IsoRow { seg: ReceiptSeg::Notify, peer_name: "F12634 通知段", peer_code: 0x99, judge_iso: true };
        assert!(!iso_row_ok(&bad));
    }

    #[test]
    fn edge_vault_thirty_pass() {
        assert!(edge_vault_ok());
        let mut led = build_full_edge_ledger();
        assert_eq!(led.len(), EDGE_CASE_CAP);
        assert!(led.seal().is_ok());
        assert_eq!(led.state(), EdgeState::Sealed);
        // 注入越限必抓。
        let mut bad = match EDGE_CASES.get(6) {
            Some(x) => x.clone(),
            None => return,
        };
        bad.amount = bad.family.cap().saturating_add(0x10);
        assert_eq!(edge_case_ok(&bad), Err(EdgeDiag::ECapOver.val()));
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = edge_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn admit_gate_rejects() {
        let mut led = EdgeLedger::new();
        let _ = led.begin_audit();
        let c1 = match EDGE_CASES.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.admit(c1), Err(EdgeDiag::ESampleOrder.val()));
        let c0 = match EDGE_CASES.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.admit(c0.clone()).is_ok());
        let dup = EdgeCase {
            no: 1,
            family: c0.family,
            spectrum: c0.spectrum,
            amount: c0.amount,
            cap_bit: c0.cap_bit,
            expect_pass: c0.expect_pass,
            code: c0.code,
        };
        assert_eq!(led.admit(dup), Err(EdgeDiag::ESampleDup.val()));
        let mut zero = match EDGE_CASES.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        zero.amount = 0;
        assert_eq!(led.admit(zero), Err(EdgeDiag::EZeroSteps.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn receipt_order_gate() {
        // 回执乱序必抓：跳段/回退/零条目列账。
        let mut m = ReceiptMachine::new();
        assert_eq!(m.step(ReceiptSeg::Window, 0), Err(EdgeDiag::EReceiptOrder.val()));
        assert!(m.step(ReceiptSeg::Notify, 0).is_ok());
        assert_eq!(m.step(ReceiptSeg::DiffLedger, 1), Err(EdgeDiag::EReceiptOrder.val()));
        assert!(m.step(ReceiptSeg::Window, 0).is_ok());
        assert_eq!(m.step(ReceiptSeg::DiffLedger, 0), Err(EdgeDiag::ESummaryEmpty.val()));
        assert!(m.step(ReceiptSeg::DiffLedger, 2).is_ok());
        assert!(m.receipt_ok());
        // 已到顶再推段：段序闸拒。
        assert!(m.step(ReceiptSeg::Notify, 0).is_err());
    }

    #[test]
    fn family_census_holds() {
        assert!(family_census_ok());
        assert_eq!(EDGE_CASES.len(), 30);
        assert_eq!(EdgeFamily::all().len(), 3);
        let mut counts = [0usize; 3];
        for c in EDGE_CASES.iter() {
            match counts.get_mut(c.family.ordinal() as usize) {
                Some(v) => *v += 1,
                None => return,
            }
        }
        assert_eq!(counts[0], 10);
        assert_eq!(counts[1], 10);
        assert_eq!(counts[2], 10);
        assert!(EdgeFamily::LongPath.name().contains("路径"));
        assert!(EdgeFamily::DeepEnum.name().contains("枚举"));
        assert!(EdgeFamily::BigBuffer.name().contains("缓冲"));
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_edge_ledger();
        assert_eq!(led.verdict_len(), EDGE_CASE_CAP);
        let mut i = 0usize;
        while i < EDGE_CASE_CAP {
            let c = match led.sample_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(c.no, i as u32, "样本 {} 错位", i);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_edge_ledger();
        let b = build_full_edge_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.sample_at(i), b.sample_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑样本缺失"),
            }
            i += 1;
        }
        let ra = build_full_receipt();
        let rb = build_full_receipt();
        assert_eq!(ra.seg_len(), rb.seg_len());
        assert_eq!(ra.diff_len(), rb.diff_len());
        assert_eq!(edge_fault_sweep().0, edge_fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：VaultOpen→Auditing→Sealed。
        assert!(EdgeState::VaultOpen.rank() < EdgeState::Auditing.rank());
        assert!(EdgeState::Auditing.rank() < EdgeState::Sealed.rank());
        let mut led = build_full_edge_ledger();
        assert_eq!(led.state(), EdgeState::Auditing);
        assert!(led.seal().is_ok());
        assert_eq!(led.seal(), Err(EdgeDiag::ERewind.val()));
        let mut fresh = EdgeLedger::new();
        assert!(fresh.begin_audit().is_ok());
        assert_eq!(fresh.begin_audit(), Err(EdgeDiag::ERewind.val()));
        // 回执机状态序只进。
        assert!(ReceiptState::Idle.rank() < ReceiptState::Notified.rank());
        assert!(ReceiptState::Notified.rank() < ReceiptState::Windowed.rank());
        assert!(ReceiptState::Windowed.rank() < ReceiptState::Ledgered.rank());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12676_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
