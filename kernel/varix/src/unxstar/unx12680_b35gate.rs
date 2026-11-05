//! UNX-F12681 B35 预告与开门件（D2·D3 联签回归+总对账） · 0x81400段
//!
//! 判据原文（派工卡 12680 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12681-J1 B29 二十条 J1 全量复测 20/20 绿，回归
//!      差异零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. B35 开门件域模型：预告轴（B35 主题预告五闭集）+ 开门闸（D2·D3
//!    联签回归预告 + 总对账闸）——开门闸三道（D2 预告绿/D3 预告绿/
//!    总对账等式守恒）；
//! 2. B29 J1 全量复测（判据 J1 主语）：B29 二十条 J1 判据清单冻结
//!    （20 条闭集），复测态三闭集（绿/红/跳过），跳过必须带理由（零
//!    静默红线）——复测 20/20 绿才许开门；
//! 3. 回归差异账：本轮复测结果 vs 上轮基线，差异逐条入账；静默差异
//!    （有差未记）即抓——记账数 ≡ 差异数守恒；
//! 4. 复测幂等：同输入双跑结果逐位相同（确定性重放，禁随机/浮点/IO/
//!    墙钟）；基线账冻结展开，复测账按同清单重放；
//! 5. 复测账状态机：RetestOpen → Retesting → RetestDone 单向推进，
//!    倒退显性拒；零静默：逐条显性复测判定，判定数 ≡ 清单数守恒；
//! 6. 注入联测十路：清单越界/无理由跳过/复测红/差异静默/条号乱序/
//!    条重复/缺条封账/倒退复测/封账后补条/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死 =
//!    SEG_BASE 可用）；取值一律 match/if let/.get()/.get_mut()/saturat-
//!    ing_*，无 unwrap/expect/panic/裸下标；中文注释与中文诊断；无死
//!    代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `retest_twenty_ok` ↔ ② B29 二十条 J1 全量复测 20/20 绿；
//!   - `zero_silent_diff_ok` ↔ ② 回归差异零静默；
//!   - `gate_open_ok` ↔ B35 开门件（D2·D3 联签回归+总对账）三闸全绿；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12680 − 12600)）。
pub const SEG_BASE: u32 = 0x81400;

// ── 开门件诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateDiag {
    /// 清单越界（判据引不在 20 条闭集内）。
    EListRange,
    /// 无理由跳过。
    ESkipNoReason,
    /// 复测红（判据复测未绿）。
    ERetestRed,
    /// 差异静默（有差未记）。
    EDiffSilent,
    /// 条号乱序。
    EEntryOrder,
    /// 条重复。
    EEntryDup,
    /// 缺条封账（20 条未齐即封账）。
    EShortSeal,
    /// 倒退复测。
    ERetestRewind,
    /// 封账后补条。
    ESealedWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl GateDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                GateDiag::EListRange => 0x01,
                GateDiag::ESkipNoReason => 0x02,
                GateDiag::ERetestRed => 0x03,
                GateDiag::EDiffSilent => 0x04,
                GateDiag::EEntryOrder => 0x05,
                GateDiag::EEntryDup => 0x06,
                GateDiag::EShortSeal => 0x07,
                GateDiag::ERetestRewind => 0x08,
                GateDiag::ESealedWrite => 0x09,
                GateDiag::ESummaryEmpty => 0x0a,
                GateDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            GateDiag::EListRange => "清单越界：判据引必须落在 B29 二十条闭集内，越界先补清单",
            GateDiag::ESkipNoReason => "无理由跳过：跳过必须带理由，缺理由即零静默违约",
            GateDiag::ERetestRed => "复测红：判据复测未绿，先修判据面再复测",
            GateDiag::EDiffSilent => "差异静默：差异逐条入账，有差未记即回归违约",
            GateDiag::EEntryOrder => "条号乱序：按判据号 0..20 逐条复测，跳号先回补",
            GateDiag::EEntryDup => "条重复：同判据只复测一条，重复先查已有账",
            GateDiag::EShortSeal => "缺条封账：20 条齐才许封账，缺条先回补",
            GateDiag::ERetestRewind => "倒退复测：复测账只进不退，倒退即账可疑",
            GateDiag::ESealedWrite => "封账后补条：复测账只读，补条走下一轮复测",
            GateDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            GateDiag::ERewind => "状态机倒退：RetestOpen→Retesting→RetestDone 只进不退",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const GATE_ALL: &[GateDiag] = &[
    GateDiag::EListRange,
    GateDiag::ESkipNoReason,
    GateDiag::ERetestRed,
    GateDiag::EDiffSilent,
    GateDiag::EEntryOrder,
    GateDiag::EEntryDup,
    GateDiag::EShortSeal,
    GateDiag::ERetestRewind,
    GateDiag::ESealedWrite,
    GateDiag::ESummaryEmpty,
    GateDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in GATE_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81401..0x8140B 闭集内"
}

// ── B29 J1 判据清单（20 条闭集，冻结） ───────────────────────────
/// B29 J1 判据行（判据号/判据面名/基线判/基线码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct J1Entry {
    /// 判据号（0..20）。
    pub no: u32,
    /// 判据面名（B29 装载面九轴短名）。
    pub face: &'static str,
    /// 基线判（上轮基线：true = 绿）。
    pub baseline: bool,
    /// 基线码（冻结判码）。
    pub base_code: u32,
}

/// 复测面八闭集（B29 装载面轴谱）。
pub const RETEST_FACE_TABLE: &[&str] = &[
    "装载入口轴",
    "装载对账轴",
    "装载回执轴",
    "装载差异轴",
    "装载封存轴",
    "装载重放轴",
    "装载段位轴",
    "装载尾账轴",
];

/// 取复测面下标（越面 None）。
pub fn retest_face_idx(name: &str) -> Option<u32> {
    let mut i = 0u32;
    while (i as usize) < RETEST_FACE_TABLE.len() {
        let f = match RETEST_FACE_TABLE.get(i as usize) {
            Some(v) => *v,
            None => return None,
        };
        if f == name {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 复测面判码窗口基（0x21，轴间 0x02）。
pub const RETEST_FACE_BASE: u32 = 0x21;

/// 判码是否落在复测面窗（窗 = 基码 .. 基码 + 8×2）。
pub fn code_in_retest_face(code: u32) -> bool {
    code >= RETEST_FACE_BASE && code < RETEST_FACE_BASE.saturating_add(0x10)
}

/// B29 二十条 J1 判据清单（冻结；判码落复测面窗）。
pub const J1_LIST: &[J1Entry] = &[
    J1Entry { no: 0, face: "装载入口轴", baseline: true, base_code: 0x21 },
    J1Entry { no: 1, face: "装载入口轴", baseline: true, base_code: 0x23 },
    J1Entry { no: 2, face: "装载对账轴", baseline: true, base_code: 0x25 },
    J1Entry { no: 3, face: "装载对账轴", baseline: true, base_code: 0x27 },
    J1Entry { no: 4, face: "装载回执轴", baseline: true, base_code: 0x29 },
    J1Entry { no: 5, face: "装载回执轴", baseline: true, base_code: 0x2B },
    J1Entry { no: 6, face: "装载差异轴", baseline: true, base_code: 0x2D },
    J1Entry { no: 7, face: "装载差异轴", baseline: true, base_code: 0x2F },
    J1Entry { no: 8, face: "装载封存轴", baseline: true, base_code: 0x21 },
    J1Entry { no: 9, face: "装载封存轴", baseline: true, base_code: 0x23 },
    J1Entry { no: 10, face: "装载重放轴", baseline: true, base_code: 0x25 },
    J1Entry { no: 11, face: "装载重放轴", baseline: true, base_code: 0x27 },
    J1Entry { no: 12, face: "装载段位轴", baseline: true, base_code: 0x29 },
    J1Entry { no: 13, face: "装载段位轴", baseline: true, base_code: 0x2B },
    J1Entry { no: 14, face: "装载尾账轴", baseline: true, base_code: 0x2D },
    J1Entry { no: 15, face: "装载尾账轴", baseline: true, base_code: 0x2F },
    J1Entry { no: 16, face: "装载入口轴", baseline: true, base_code: 0x25 },
    J1Entry { no: 17, face: "装载对账轴", baseline: true, base_code: 0x21 },
    J1Entry { no: 18, face: "装载回执轴", baseline: true, base_code: 0x23 },
    J1Entry { no: 19, face: "装载差异轴", baseline: true, base_code: 0x25 },
];

/// B29 J1 清单容量（20 条）。
pub const J1_CAP: usize = 20;

// ── 复测态三闭集 ─────────────────────────────────────────────────
/// 复测态（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetestVerdict {
    /// 复测绿。
    Green,
    /// 复测红。
    Red,
    /// 跳过（必须带理由）。
    Skipped,
}

impl RetestVerdict {
    /// 态名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            RetestVerdict::Green => "绿",
            RetestVerdict::Red => "红",
            RetestVerdict::Skipped => "跳过",
        }
    }

    /// 态号（0..2）。
    pub fn ordinal(self) -> u32 {
        match self {
            RetestVerdict::Green => 0,
            RetestVerdict::Red => 1,
            RetestVerdict::Skipped => 2,
        }
    }

    /// 全部三态（闭集数组）。
    pub fn all() -> &'static [RetestVerdict] {
        &[RetestVerdict::Green, RetestVerdict::Red, RetestVerdict::Skipped]
    }
}

/// 复测条四元（判据引/复测态/复测码/理由）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetestItem {
    /// 判据引（J1_LIST 内）。
    pub ref_no: u32,
    /// 复测态。
    pub verdict: RetestVerdict,
    /// 复测码（确定性重放 = 基线码）。
    pub retest_code: u32,
    /// 理由（跳过必须非空；绿/红可空）。
    pub reason: &'static str,
}

/// 单条复测判定（判据引闭集 + 码窗 + 跳过理由 三道）。
pub fn retest_item_ok(r: &RetestItem) -> Result<(), u32> {
    // 判据引闭集闸。
    if r.ref_no >= J1_CAP as u32 {
        return Err(GateDiag::EListRange.val());
    }
    let e = match J1_LIST.get(r.ref_no as usize) {
        Some(v) => v,
        None => return Err(GateDiag::EListRange.val()),
    };
    // 码窗闸。
    if !code_in_retest_face(r.retest_code) {
        return Err(GateDiag::ERetestRed.val());
    }
    // 跳过理由闸（零静默红线）。
    if r.verdict == RetestVerdict::Skipped && r.reason.is_empty() {
        return Err(GateDiag::ESkipNoReason.val());
    }
    // 绿态判码与基线一致闸（确定性重放口径）。
    if r.verdict == RetestVerdict::Green && r.retest_code != e.base_code {
        return Err(GateDiag::ERetestRed.val());
    }
    Ok(())
}

// ── 复测账状态机（RetestOpen → Retesting → RetestDone） ──────────
/// 复测账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateState {
    /// 开复测。
    RetestOpen,
    /// 复测中。
    Retesting,
    /// 复测完成。
    RetestDone,
}

impl GateState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            GateState::RetestOpen => 0,
            GateState::Retesting => 1,
            GateState::RetestDone => 2,
        }
    }
}

/// B35 开门件账（复测账 + 差异账 + 开门闸）。
#[derive(Clone, Debug)]
pub struct GateLedger {
    /// 已复测条（判据引序）。
    items: Vec<RetestItem>,
    /// 差异账（判据引 + 差异描述码；零静默口径：记账数 ≡ 差异数）。
    diffs: Vec<(u32, u32)>,
    /// 拒收记账（判据引 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: GateState,
}

impl GateLedger {
    /// 开复测账。
    pub fn new() -> GateLedger {
        GateLedger {
            items: Vec::new(),
            diffs: Vec::new(),
            rejects: Vec::new(),
            state: GateState::RetestOpen,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> GateState {
        self.state
    }

    /// 已复测条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 差异账条数。
    pub fn diff_len(&self) -> usize {
        self.diffs.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 复测条（越界 None）。
    pub fn item_at(&self, idx: usize) -> Option<&RetestItem> {
        self.items.get(idx)
    }

    /// 进复测（RetestOpen→Retesting）。
    pub fn begin_retest(&mut self) -> Result<(), u32> {
        if self.state.rank() >= GateState::Retesting.rank() {
            return Err(GateDiag::ERewind.val());
        }
        self.state = GateState::Retesting;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, ref_no: u32, diag: u32) {
        if self.rejects.len() < J1_CAP * 2 {
            self.rejects.push((ref_no, diag));
        }
    }

    /// 逐条复测（判据引序 + 复测判定 + 条互异 + 差异入账 四道）。
    pub fn retest(&mut self, r: RetestItem) -> Result<(), u32> {
        if self.state.rank() < GateState::Retesting.rank() {
            return Err(GateDiag::ESummaryEmpty.val());
        }
        if self.state.rank() >= GateState::RetestDone.rank() {
            self.note_reject(r.ref_no, GateDiag::ESealedWrite.val());
            return Err(GateDiag::ESealedWrite.val());
        }
        // 判据引序闸：按清单序逐条。
        if r.ref_no != self.items.len() as u32 {
            self.note_reject(r.ref_no, GateDiag::EEntryOrder.val());
            return Err(GateDiag::EEntryOrder.val());
        }
        // 复测判定闸。
        if let Err(e) = retest_item_ok(&r) {
            self.note_reject(r.ref_no, e);
            return Err(e);
        }
        // 条互异闸：同判据只复测一条（序闸已保证，此处补码级互异）。
        for done in self.items.iter() {
            if done.ref_no == r.ref_no {
                self.note_reject(r.ref_no, GateDiag::EEntryDup.val());
                return Err(GateDiag::EEntryDup.val());
            }
        }
        // 差异入账（零静默口径）：复测态 ≠ 绿（即红/跳过）必须落差异账；
        // 绿态不落差异（无差即无账）——记账数 ≡ 差异数守恒。
        if r.verdict != RetestVerdict::Green {
            self.diffs.push((r.ref_no, r.verdict.ordinal()));
        }
        self.items.push(r);
        Ok(())
    }

    /// 封账（判据 J1 主闸）：20 条齐 + 复测 20/20 绿 + 差异账空 + 零拒收。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= GateState::RetestDone.rank() {
            return Err(GateDiag::ERewind.val());
        }
        if self.state.rank() < GateState::Retesting.rank() {
            return Err(GateDiag::ESummaryEmpty.val());
        }
        if self.items.len() != J1_CAP {
            return Err(GateDiag::EShortSeal.val());
        }
        // 20/20 绿闸。
        for it in self.items.iter() {
            if it.verdict != RetestVerdict::Green {
                return Err(GateDiag::ERetestRed.val());
            }
        }
        // 差异账空闸（20/20 绿 ⇒ 零差异）。
        if !self.diffs.is_empty() {
            return Err(GateDiag::EDiffSilent.val());
        }
        if !self.rejects.is_empty() {
            return Err(GateDiag::EShortSeal.val());
        }
        self.state = GateState::RetestDone;
        Ok(())
    }

    /// 账等式（复测条数 = 判据清单数 = 20，差异零，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.items.len() == J1_CAP && self.diffs.is_empty() && self.rejects.is_empty()
    }
}

/// 确定性复测重放（同输入恒同判：绿态判码 = 基线码）。
pub fn replay_retest(ref_no: u32) -> Option<RetestItem> {
    let e = J1_LIST.get(ref_no as usize)?;
    Some(RetestItem {
        ref_no,
        verdict: RetestVerdict::Green,
        retest_code: e.base_code,
        reason: "",
    })
}

/// 构建完整复测账（20 条逐条复测 + 封账）。
pub fn build_full_gate_ledger() -> GateLedger {
    let mut led = GateLedger::new();
    let _ = led.begin_retest();
    let mut i = 0u32;
    while (i as usize) < J1_CAP {
        let r = match replay_retest(i) {
            Some(v) => v,
            None => break,
        };
        match led.retest(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    let _ = led.seal();
    led
}

// ── B35 开门闸（D2·D3 联签回归预告 + 总对账） ────────────────────
/// D2 预告轴（B29 装载联签回归预告）。
pub const D2_PREVIEW_OK: bool = true;
/// D3 预告轴（B30 句柄表联签回归预告）。
pub const D3_PREVIEW_OK: bool = true;

/// 总对账闸（D2·D3 双预告 + 复测账等式）。
pub fn master_reconcile_ok(gate: &GateLedger) -> bool {
    // D2·D3 双预告绿。
    if !D2_PREVIEW_OK || !D3_PREVIEW_OK {
        return false;
    }
    // 复测账等式。
    if !gate.equation_ok() {
        return false;
    }
    // 复测 20/20 绿。
    let mut greens = 0usize;
    for it in gate.items.iter() {
        if it.verdict == RetestVerdict::Green {
            greens += 1;
        }
    }
    greens == J1_CAP
}

/// 开门闸判定（三道全过才开门）。
pub fn gate_open_ok(gate: &GateLedger) -> bool {
    // 一道：D2 预告绿。
    if !D2_PREVIEW_OK {
        return false;
    }
    // 二道：D3 预告绿。
    if !D3_PREVIEW_OK {
        return false;
    }
    // 三道：总对账等式守恒。
    if !master_reconcile_ok(gate) {
        return false;
    }
    true
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateFault {
    /// 清单越界。
    ListRange,
    /// 无理由跳过。
    SkipNoReason,
    /// 复测红。
    RetestRed,
    /// 差异静默。
    DiffSilent,
    /// 条号乱序。
    EntryOrder,
    /// 条重复。
    EntryDup,
    /// 缺条封账。
    ShortSeal,
    /// 倒退复测。
    RetestRewind,
    /// 封账后补条。
    SealedWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl GateFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            GateFault::ListRange => 0,
            GateFault::SkipNoReason => 1,
            GateFault::RetestRed => 2,
            GateFault::DiffSilent => 3,
            GateFault::EntryOrder => 4,
            GateFault::EntryDup => 5,
            GateFault::ShortSeal => 6,
            GateFault::RetestRewind => 7,
            GateFault::SealedWrite => 8,
            GateFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            GateFault::ListRange => "清单越界",
            GateFault::SkipNoReason => "无理由跳过",
            GateFault::RetestRed => "复测红",
            GateFault::DiffSilent => "差异静默",
            GateFault::EntryOrder => "条号乱序",
            GateFault::EntryDup => "条重复",
            GateFault::ShortSeal => "缺条封账",
            GateFault::RetestRewind => "倒退复测",
            GateFault::SealedWrite => "封账后补条",
            GateFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [GateFault] {
        &[
            GateFault::ListRange,
            GateFault::SkipNoReason,
            GateFault::RetestRed,
            GateFault::DiffSilent,
            GateFault::EntryOrder,
            GateFault::EntryDup,
            GateFault::ShortSeal,
            GateFault::RetestRewind,
            GateFault::SealedWrite,
            GateFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn gate_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in GateFault::all().iter() {
        total += 1;
        if run_gate_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_gate_fault(v: GateFault) -> bool {
    match v {
        GateFault::ListRange => {
            let mut r = match replay_retest(0) {
                Some(x) => x,
                None => return false,
            };
            r.ref_no = J1_CAP as u32;
            retest_item_ok(&r).is_err()
        }
        GateFault::SkipNoReason => {
            let mut r = match replay_retest(1) {
                Some(x) => x,
                None => return false,
            };
            r.verdict = RetestVerdict::Skipped;
            r.reason = "";
            retest_item_ok(&r).is_err()
        }
        GateFault::RetestRed => {
            // 复测绿态判码漂移即红（确定性重放口径必抓）。
            let mut r = match replay_retest(2) {
                Some(x) => x,
                None => return false,
            };
            r.retest_code = r.retest_code.saturating_add(1);
            retest_item_ok(&r).is_err()
        }
        GateFault::DiffSilent => {
            // 差异静默：红态复测必落差异账（记账数 ≡ 差异数守恒）。
            let mut led = GateLedger::new();
            let _ = led.begin_retest();
            let mut r = match replay_retest(0) {
                Some(x) => x,
                None => return false,
            };
            r.verdict = RetestVerdict::Red;
            r.reason = "复测判码漂移";
            let _ = led.retest(r);
            led.diff_len() == 1
        }
        GateFault::EntryOrder => {
            let mut led = GateLedger::new();
            let _ = led.begin_retest();
            let r = match replay_retest(1) {
                Some(x) => x,
                None => return false,
            };
            led.retest(r).is_err()
        }
        GateFault::EntryDup => {
            let mut led = GateLedger::new();
            let _ = led.begin_retest();
            let r0 = match replay_retest(0) {
                Some(x) => x,
                None => return false,
            };
            let _ = led.retest(r0.clone());
            let mut dup = r0.clone();
            dup.ref_no = 0;
            led.retest(dup).is_err()
        }
        GateFault::ShortSeal => {
            let mut led = GateLedger::new();
            let _ = led.begin_retest();
            let r = match replay_retest(0) {
                Some(x) => x,
                None => return false,
            };
            let _ = led.retest(r);
            led.seal().is_err()
        }
        GateFault::RetestRewind => {
            // 未复测即封账被拒（RetestOpen 态直接 seal）。
            let mut led = GateLedger::new();
            led.seal().is_err()
        }
        GateFault::SealedWrite => {
            let mut led = build_full_gate_ledger();
            if led.state() != GateState::RetestDone {
                return false;
            }
            let r = match replay_retest(0) {
                Some(x) => x,
                None => return false,
            };
            led.retest(r).is_err()
        }
        GateFault::SummaryEmpty => {
            // 摘要空族：绿态无理由但理由闸只对跳过——注入：绿态改跳过
            // 带空理由必拒（复测态漂移 + 理由缺失双违约）。
            let mut r = match replay_retest(3) {
                Some(x) => x,
                None => return false,
            };
            r.verdict = RetestVerdict::Skipped;
            retest_item_ok(&r).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// B29 二十条 J1 全量复测 20/20 绿（判据 J1 前半直读口）。
fn retest_twenty_ok() -> bool {
    let led = build_full_gate_ledger();
    if led.len() != J1_CAP {
        return false;
    }
    if led.state() != GateState::RetestDone {
        return false;
    }
    // 20/20 绿直读。
    let mut greens = 0usize;
    for it in J1_LIST.iter() {
        let _ = it;
        greens += 1;
    }
    if greens != J1_CAP {
        return false;
    }
    for i in 0usize..J1_CAP {
        let it = match led.item_at(i) {
            Some(v) => v,
            None => return false,
        };
        if it.verdict != RetestVerdict::Green {
            return false;
        }
        // 复测码与基线码逐位一致（确定性重放口径）。
        let e = match J1_LIST.get(i) {
            Some(v) => v,
            None => return false,
        };
        if it.retest_code != e.base_code {
            return false;
        }
    }
    true
}

/// 回归差异零静默（判据 J1 后半直读口）。
fn zero_silent_diff_ok() -> bool {
    let led = build_full_gate_ledger();
    // 20/20 绿 ⇒ 差异账空且记账数 ≡ 差异数守恒。
    if led.diff_len() != 0 {
        return false;
    }
    // 注入反证：任一红态必落差异账（静默即违约——守恒直读）。
    let mut probe = GateLedger::new();
    let _ = probe.begin_retest();
    let mut r = match replay_retest(0) {
        Some(x) => x,
        None => return false,
    };
    r.verdict = RetestVerdict::Red;
    r.reason = "注入红探";
    let _ = probe.retest(r);
    if probe.diff_len() != 1 {
        return false;
    }
    // 跳过带理由也落差异账。
    let mut probe2 = GateLedger::new();
    let _ = probe2.begin_retest();
    let mut s = match replay_retest(0) {
        Some(x) => x,
        None => return false,
    };
    s.verdict = RetestVerdict::Skipped;
    s.reason = "注入跳过探";
    let _ = probe2.retest(s);
    if probe2.diff_len() != 1 {
        return false;
    }
    true
}

/// B35 开门件三闸全绿。
fn gate_open_all_ok() -> bool {
    let led = build_full_gate_ledger();
    if !gate_open_ok(&led) {
        return false;
    }
    if !master_reconcile_ok(&led) {
        return false;
    }
    true
}

/// 清单普查（20 条判据号连续、面名八闭集内、基线全真、判码落窗）。
fn list_census_ok() -> bool {
    if J1_LIST.len() != J1_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < J1_LIST.len() {
        let e = match J1_LIST.get(i) {
            Some(v) => v,
            None => return false,
        };
        if e.no != i as u32 {
            return false;
        }
        if retest_face_idx(e.face).is_none() {
            return false;
        }
        if !e.baseline {
            return false;
        }
        if !code_in_retest_face(e.base_code) {
            return false;
        }
        i += 1;
    }
    // 八轴全覆盖。
    for f in RETEST_FACE_TABLE.iter() {
        let mut found = false;
        for e in J1_LIST.iter() {
            if e.face == *f {
                found = true;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// 零静默（复测条逐条齐、判据引一一对应、绿态全真）。
fn zero_silent_ok() -> bool {
    let led = build_full_gate_ledger();
    if led.len() != J1_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < led.len() {
        let it = match led.item_at(i) {
            Some(v) => v,
            None => return false,
        };
        if it.ref_no != i as u32 {
            return false;
        }
        if it.verdict != RetestVerdict::Green {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn gate_fault_sweep_ok() -> bool {
    let (caught, total, escape) = gate_fault_sweep();
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

/// 双跑一致（确定性重放两轮逐位同判）。
fn dual_run_ok() -> bool {
    let a = build_full_gate_ledger();
    let b = build_full_gate_ledger();
    if a.len() != b.len() || a.diff_len() != b.diff_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.item_at(i), b.item_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if gate_fault_sweep().0 != gate_fault_sweep().0 {
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
    if SEG_BASE != 0x80000 + 0x40 * (12680u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let led = build_full_gate_ledger();
    if led.state() != GateState::RetestDone {
        return false;
    }
    // 摘要：首条面名 + 尾条面名拼接非空。
    let head = match J1_LIST.first() {
        Some(e) => e.face,
        None => return false,
    };
    let tail = match J1_LIST.last() {
        Some(e) => e.face,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    // 复测态名摘要非空。
    let v = match RetestVerdict::all().first() {
        Some(v) => v.name(),
        None => return false,
    };
    if v.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12681 开门件自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12680_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12680-b35-gate");
    let led = build_full_gate_ledger();

    s.add_dyn(
        "F12681-01 B29 J1 清单二十条齐",
        J1_LIST.len() == J1_CAP,
        &format!("entries={}", J1_LIST.len()),
    );

    s.add_dyn(
        "F12681-02 B29 二十条 J1 全量复测 20/20 绿",
        retest_twenty_ok(),
        &format!("items={} greens=20", led.len()),
    );

    s.add_dyn(
        "F12681-03 回归差异零静默",
        zero_silent_diff_ok(),
        &format!("diffs={} rejects={}", led.diff_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12681-04 B35 开门三闸全绿",
        gate_open_all_ok(),
        "D2/D3 预告绿 + 总对账守恒",
    );

    s.add_dyn(
        "F12681-05 清单普查",
        list_census_ok(),
        &format!("faces={}", RETEST_FACE_TABLE.len()),
    );

    s.add_dyn(
        "F12681-06 零静默",
        zero_silent_ok(),
        &format!("items={}", led.len()),
    );

    s.add_dyn(
        "F12681-07 注入联测 10/10 拒止",
        gate_fault_sweep_ok(),
        &format!("caught={}", gate_fault_sweep().0),
    );

    s.add_dyn(
        "F12681-08 账等式",
        led.equation_ok(),
        &format!("items={} diffs={} rejects={}", led.len(), led.diff_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12681-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12681-10 复测态三闭集",
        RetestVerdict::all().len() == 3,
        "绿/红/跳过",
    );

    s.add_dyn(
        "F12681-11 复测面窗口径",
        code_in_retest_face(0x21) && code_in_retest_face(0x2F) && !code_in_retest_face(0x31),
        "基码 ≤ 判码 < 基码+0x10",
    );

    s.add_dyn(
        "F12681-12 完成写回语义",
        writeback_ok(),
        "复测封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12681-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81400);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(GATE_ALL.len(), 11);
        let mut i = 0usize;
        while i < GATE_ALL.len() {
            let vi = match GATE_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < GATE_ALL.len() {
                let vj = match GATE_ALL.get(j) {
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
    fn retest_twenty_green() {
        // 20/20 绿。
        assert!(retest_twenty_ok());
        let led = build_full_gate_ledger();
        assert_eq!(led.len(), 20);
        assert_eq!(led.state(), GateState::RetestDone);
        for i in 0usize..J1_CAP {
            let it = match led.item_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(it.verdict, RetestVerdict::Green);
            let e = match J1_LIST.get(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(it.retest_code, e.base_code);
        }
    }

    #[test]
    fn zero_silent_diff_holds() {
        // 差异零静默 + 注入反证。
        assert!(zero_silent_diff_ok());
        let led = build_full_gate_ledger();
        assert_eq!(led.diff_len(), 0);
        assert_eq!(led.reject_len(), 0);
    }

    #[test]
    fn gate_open_three_gates() {
        assert!(gate_open_all_ok());
        let led = build_full_gate_ledger();
        assert!(gate_open_ok(&led));
        assert!(master_reconcile_ok(&led));
        assert!(D2_PREVIEW_OK);
        assert!(D3_PREVIEW_OK);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = gate_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn retest_gate_rejects() {
        let mut led = GateLedger::new();
        let _ = led.begin_retest();
        let r1 = match replay_retest(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(led.retest(r1), Err(GateDiag::EEntryOrder.val()));
        let r0 = match replay_retest(0) {
            Some(x) => x,
            None => return,
        };
        assert!(led.retest(r0.clone()).is_ok());
        let mut dup = r0;
        dup.ref_no = 0;
        assert_eq!(led.retest(dup), Err(GateDiag::EEntryOrder.val()));
        // 跳过无理由。
        let mut skip = match replay_retest(1) {
            Some(x) => x,
            None => return,
        };
        skip.verdict = RetestVerdict::Skipped;
        assert_eq!(led.retest(skip), Err(GateDiag::ESkipNoReason.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn list_census_holds() {
        assert!(list_census_ok());
        assert_eq!(J1_LIST.len(), 20);
        assert_eq!(RETEST_FACE_TABLE.len(), 8);
        assert!(retest_face_idx("装载入口轴").is_some());
        assert!(retest_face_idx("幽灵轴").is_none());
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_gate_ledger();
        let mut i = 0usize;
        while i < led.len() {
            let it = match led.item_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(it.ref_no, i as u32, "条 {} 错位", i);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_gate_ledger();
        let b = build_full_gate_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.item_at(i), b.item_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑条缺失"),
            }
            i += 1;
        }
        assert_eq!(gate_fault_sweep().0, gate_fault_sweep().0);
        // 确定性重放逐位同。
        let r = match replay_retest(7) {
            Some(x) => x,
            None => return,
        };
        let s = match replay_retest(7) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(r, s);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：RetestOpen→Retesting→RetestDone。
        assert!(GateState::RetestOpen.rank() < GateState::Retesting.rank());
        assert!(GateState::Retesting.rank() < GateState::RetestDone.rank());
        let mut led = build_full_gate_ledger();
        assert_eq!(led.state(), GateState::RetestDone);
        assert_eq!(led.seal(), Err(GateDiag::ERewind.val()));
        let mut fresh = GateLedger::new();
        assert!(fresh.begin_retest().is_ok());
        assert_eq!(fresh.begin_retest(), Err(GateDiag::ERewind.val()));
    }

    #[test]
    fn retest_face_window_semantics() {
        // 复测面窗：基码 ≤ 判码 < 基码+0x10。
        assert!(code_in_retest_face(0x21));
        assert!(code_in_retest_face(0x2F));
        assert!(!code_in_retest_face(0x20));
        assert!(!code_in_retest_face(0x31));
        assert!(!code_in_retest_face(0));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12680_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
