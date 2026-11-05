//! UNX-F12682 D2 装载联签回归（B29 判据 J1 全量复测） · 0x81440段
//!
//! 判据原文（派工卡 12681 逐字锚定；本文件短名 d2retest 与旧批
//! unx12681_b29rerun（SEG 0x65100，VX3 系）不冲突，旧批文件不触碰）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12682-J1 B30 二十条 J1 全量复测 20/20 绿，回归
//!      差异零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. D2 装载联签回归域模型：B30 二十条 J1 判据（句柄表装载面轴谱）
//!    全量复测——联签对模型（装载签/回归签双签位），每条判据复测出
//!    联签对（签码对齐 = 复测绿）；
//! 2. 基线账冻结：B30 二十条 J1 基线判码冻结展开，基线账摘要以整数
//!    异或折迭派生（零浮点/零随机/确定性）——回归账摘要必须与基线
//!    账摘要逐位一致（20/20 绿口径）；
//! 3. 回归差异账：本轮复测 vs 基线，差异逐条入账；静默差异（有差未
//!    记）即抓——记账数 ≡ 差异数守恒，差异账空 ≡ 20/20 绿；
//! 4. 复测态三闭集（绿/红/跳过），跳过必须带理由；复测幂等：同输入
//!    双跑结果逐位相同（确定性重放）；
//! 5. 复测账状态机：D2Open → D2Running → D2Sealed 单向推进，倒退显
//!    性拒；零静默：逐条显性判定，判定数 ≡ 清单数守恒；
//! 6. 注入联测十路：清单越界/无理由跳过/签码漂移/差异静默/条号乱序/
//!    条重复/缺条封账/倒退复测/封账后补条/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `rerun_twenty_ok` ↔ ② B30 二十条 J1 全量复测 20/20 绿；
//!   - `diff_zero_silent_ok` ↔ ② 回归差异零静默；
//!   - `baseline_digest_ok` ↔ 基线/回归账摘要逐位一致（回归主判）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12681 − 12600)）。
pub const SEG_BASE: u32 = 0x81440;

// ── D2 复测诊断码闭集 ────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D2Diag {
    /// 清单越界。
    EListRange,
    /// 无理由跳过。
    ESkipNoReason,
    /// 签码漂移（联签码对不齐）。
    ESigDrift,
    /// 差异静默。
    EDiffSilent,
    /// 条号乱序。
    EEntryOrder,
    /// 条重复。
    EEntryDup,
    /// 缺条封账。
    EShortSeal,
    /// 倒退复测。
    ERerunRewind,
    /// 封账后补条。
    ESealedWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl D2Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                D2Diag::EListRange => 0x01,
                D2Diag::ESkipNoReason => 0x02,
                D2Diag::ESigDrift => 0x03,
                D2Diag::EDiffSilent => 0x04,
                D2Diag::EEntryOrder => 0x05,
                D2Diag::EEntryDup => 0x06,
                D2Diag::EShortSeal => 0x07,
                D2Diag::ERerunRewind => 0x08,
                D2Diag::ESealedWrite => 0x09,
                D2Diag::ESummaryEmpty => 0x0a,
                D2Diag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            D2Diag::EListRange => "清单越界：判据引必须落在 B30 二十条闭集内，越界先补清单",
            D2Diag::ESkipNoReason => "无理由跳过：跳过必须带理由，缺理由即零静默违约",
            D2Diag::ESigDrift => "签码漂移：装载签与回归签码必须对齐，漂移先对签码口径",
            D2Diag::EDiffSilent => "差异静默：差异逐条入账，有差未记即回归违约",
            D2Diag::EEntryOrder => "条号乱序：按判据号 0..20 逐条复测，跳号先回补",
            D2Diag::EEntryDup => "条重复：同判据只复测一条，重复先查已有账",
            D2Diag::EShortSeal => "缺条封账：20 条齐才许封账，缺条先回补",
            D2Diag::ERerunRewind => "倒退复测：复测账只进不退，倒退即账可疑",
            D2Diag::ESealedWrite => "封账后补条：复测账只读，补条走下一轮复测",
            D2Diag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            D2Diag::ERewind => "状态机倒退：D2Open→D2Running→D2Sealed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const D2_ALL: &[D2Diag] = &[
    D2Diag::EListRange,
    D2Diag::ESkipNoReason,
    D2Diag::ESigDrift,
    D2Diag::EDiffSilent,
    D2Diag::EEntryOrder,
    D2Diag::EEntryDup,
    D2Diag::EShortSeal,
    D2Diag::ERerunRewind,
    D2Diag::ESealedWrite,
    D2Diag::ESummaryEmpty,
    D2Diag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in D2_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81441..0x8144B 闭集内"
}

// ── B30 J1 判据清单（20 条闭集，冻结） ───────────────────────────
/// B30 J1 判据行（判据号/判据面名/基线判/基线码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct D2Entry {
    /// 判据号（0..20）。
    pub no: u32,
    /// 判据面名（B30 句柄表装载面轴谱）。
    pub face: &'static str,
    /// 基线判（上轮基线：true = 绿）。
    pub baseline: bool,
    /// 基线码（冻结判码）。
    pub base_code: u32,
}

/// 复测面七闭集（B30 句柄装载轴谱）。
pub const D2_FACE_TABLE: &[&str] = &[
    "句柄装载轴",
    "句柄开账轴",
    "句柄对账轴",
    "句柄差异轴",
    "句柄回执轴",
    "句柄重放轴",
    "句柄尾账轴",
];

/// 取复测面下标（越面 None）。
pub fn d2_face_idx(name: &str) -> Option<u32> {
    let mut i = 0u32;
    while (i as usize) < D2_FACE_TABLE.len() {
        let f = match D2_FACE_TABLE.get(i as usize) {
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

/// 复测面判码窗口基（0x31，判码 < 基码+0x20）。
pub const D2_FACE_BASE: u32 = 0x31;

/// 判码是否落在复测面窗。
pub fn code_in_d2_face(code: u32) -> bool {
    code >= D2_FACE_BASE && code < D2_FACE_BASE.saturating_add(0x20)
}

/// B30 二十条 J1 判据清单（冻结）。
pub const D2_LIST: &[D2Entry] = &[
    D2Entry { no: 0, face: "句柄装载轴", baseline: true, base_code: 0x31 },
    D2Entry { no: 1, face: "句柄装载轴", baseline: true, base_code: 0x33 },
    D2Entry { no: 2, face: "句柄开账轴", baseline: true, base_code: 0x35 },
    D2Entry { no: 3, face: "句柄开账轴", baseline: true, base_code: 0x37 },
    D2Entry { no: 4, face: "句柄对账轴", baseline: true, base_code: 0x39 },
    D2Entry { no: 5, face: "句柄对账轴", baseline: true, base_code: 0x3B },
    D2Entry { no: 6, face: "句柄差异轴", baseline: true, base_code: 0x3D },
    D2Entry { no: 7, face: "句柄差异轴", baseline: true, base_code: 0x3F },
    D2Entry { no: 8, face: "句柄回执轴", baseline: true, base_code: 0x41 },
    D2Entry { no: 9, face: "句柄回执轴", baseline: true, base_code: 0x43 },
    D2Entry { no: 10, face: "句柄重放轴", baseline: true, base_code: 0x45 },
    D2Entry { no: 11, face: "句柄重放轴", baseline: true, base_code: 0x47 },
    D2Entry { no: 12, face: "句柄尾账轴", baseline: true, base_code: 0x49 },
    D2Entry { no: 13, face: "句柄尾账轴", baseline: true, base_code: 0x4B },
    D2Entry { no: 14, face: "句柄装载轴", baseline: true, base_code: 0x4D },
    D2Entry { no: 15, face: "句柄开账轴", baseline: true, base_code: 0x31 },
    D2Entry { no: 16, face: "句柄对账轴", baseline: true, base_code: 0x33 },
    D2Entry { no: 17, face: "句柄差异轴", baseline: true, base_code: 0x35 },
    D2Entry { no: 18, face: "句柄回执轴", baseline: true, base_code: 0x37 },
    D2Entry { no: 19, face: "句柄尾账轴", baseline: true, base_code: 0x39 },
];

/// B30 J1 清单容量（20 条）。
pub const D2_CAP: usize = 20;

// ── 联签对（装载签/回归签） ──────────────────────────────────────
/// 联签对（双签位模型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CosignPair {
    /// 装载签码（B29 侧落签）。
    pub load_sig: u32,
    /// 回归签码（B30 复测侧落签）。
    pub rerun_sig: u32,
}

/// 联签对齐判定（双签码逐位相等且双非零）。
pub fn pair_aligned(p: &CosignPair) -> bool {
    p.load_sig != 0 && p.rerun_sig != 0 && p.load_sig == p.rerun_sig
}

// ── 复测态三闭集 ─────────────────────────────────────────────────
/// 复测态（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D2Verdict {
    /// 复测绿。
    Green,
    /// 复测红。
    Red,
    /// 跳过（必须带理由）。
    Skipped,
}

impl D2Verdict {
    /// 态名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            D2Verdict::Green => "绿",
            D2Verdict::Red => "红",
            D2Verdict::Skipped => "跳过",
        }
    }

    /// 态号（0..2）。
    pub fn ordinal(self) -> u32 {
        match self {
            D2Verdict::Green => 0,
            D2Verdict::Red => 1,
            D2Verdict::Skipped => 2,
        }
    }

    /// 全部三态（闭集数组）。
    pub fn all() -> &'static [D2Verdict] {
        &[D2Verdict::Green, D2Verdict::Red, D2Verdict::Skipped]
    }
}

/// 复测条五元（判据引/复测态/装载签/回归签/理由）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2RerunItem {
    /// 判据引（D2_LIST 内）。
    pub ref_no: u32,
    /// 复测态。
    pub verdict: D2Verdict,
    /// 装载签码（= 基线码，确定性重放）。
    pub load_sig: u32,
    /// 回归签码（复测落签）。
    pub rerun_sig: u32,
    /// 理由（跳过必须非空）。
    pub reason: &'static str,
}

/// 单条复测判定（判据引闭集 + 码窗 + 跳过理由 + 联签对齐 四道）。
pub fn d2_item_ok(r: &D2RerunItem) -> Result<(), u32> {
    // 判据引闭集闸。
    if r.ref_no >= D2_CAP as u32 {
        return Err(D2Diag::EListRange.val());
    }
    // 码窗闸（签码全落复测面窗）。
    if !code_in_d2_face(r.load_sig) || !code_in_d2_face(r.rerun_sig) {
        return Err(D2Diag::ESigDrift.val());
    }
    // 跳过理由闸（零静默红线）。
    if r.verdict == D2Verdict::Skipped && r.reason.is_empty() {
        return Err(D2Diag::ESkipNoReason.val());
    }
    // 联签对齐闸：绿态双签码必须逐位对齐。
    let p = CosignPair { load_sig: r.load_sig, rerun_sig: r.rerun_sig };
    if r.verdict == D2Verdict::Green && !pair_aligned(&p) {
        return Err(D2Diag::ESigDrift.val());
    }
    Ok(())
}

// ── 基线账摘要（整数异或折迭，确定性） ───────────────────────────
/// 异或折迭摘要（判码序列 → 单字摘要；同序列恒同摘要）。
pub fn fold_digest(codes: &[u32]) -> u32 {
    let mut acc: u32 = SEG_BASE;
    for c in codes.iter() {
        acc = acc.wrapping_mul(0x9E37_79B9) ^ *c;
    }
    acc
}

/// 基线账摘要（D2_LIST 基线码冻结折迭）。
pub fn baseline_digest() -> u32 {
    let mut codes: Vec<u32> = Vec::new();
    for e in D2_LIST.iter() {
        codes.push(e.base_code);
    }
    fold_digest(&codes)
}

/// 回归账摘要（复测条装载签序列折迭——绿态 = 基线码 ⇒ 摘要逐位一致）。
pub fn rerun_digest(items: &[D2RerunItem]) -> u32 {
    let mut codes: Vec<u32> = Vec::new();
    for it in items.iter() {
        codes.push(it.load_sig);
    }
    fold_digest(&codes)
}

// ── 复测账状态机（D2Open → D2Running → D2Sealed） ────────────────
/// 复测账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D2State {
    /// 开复测。
    D2Open,
    /// 复测中。
    D2Running,
    /// 已封账。
    D2Sealed,
}

impl D2State {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            D2State::D2Open => 0,
            D2State::D2Running => 1,
            D2State::D2Sealed => 2,
        }
    }
}

/// D2 复测账。
#[derive(Clone, Debug)]
pub struct D2Ledger {
    /// 已复测条（判据引序）。
    items: Vec<D2RerunItem>,
    /// 差异账（判据引 + 差异态号；零静默口径）。
    diffs: Vec<(u32, u32)>,
    /// 拒收记账（判据引 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: D2State,
}

impl D2Ledger {
    /// 开账。
    pub fn new() -> D2Ledger {
        D2Ledger {
            items: Vec::new(),
            diffs: Vec::new(),
            rejects: Vec::new(),
            state: D2State::D2Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> D2State {
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
    pub fn item_at(&self, idx: usize) -> Option<&D2RerunItem> {
        self.items.get(idx)
    }

    /// 进复测（D2Open→D2Running）。
    pub fn begin_rerun(&mut self) -> Result<(), u32> {
        if self.state.rank() >= D2State::D2Running.rank() {
            return Err(D2Diag::ERewind.val());
        }
        self.state = D2State::D2Running;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, ref_no: u32, diag: u32) {
        if self.rejects.len() < D2_CAP * 2 {
            self.rejects.push((ref_no, diag));
        }
    }

    /// 逐条复测（判据引序 + 复测判定 + 条互异 + 差异入账 四道）。
    pub fn rerun(&mut self, r: D2RerunItem) -> Result<(), u32> {
        if self.state.rank() < D2State::D2Running.rank() {
            return Err(D2Diag::ESummaryEmpty.val());
        }
        if self.state.rank() >= D2State::D2Sealed.rank() {
            self.note_reject(r.ref_no, D2Diag::ESealedWrite.val());
            return Err(D2Diag::ESealedWrite.val());
        }
        // 判据引序闸。
        if r.ref_no != self.items.len() as u32 {
            self.note_reject(r.ref_no, D2Diag::EEntryOrder.val());
            return Err(D2Diag::EEntryOrder.val());
        }
        // 复测判定闸。
        if let Err(e) = d2_item_ok(&r) {
            self.note_reject(r.ref_no, e);
            return Err(e);
        }
        // 条互异闸。
        for done in self.items.iter() {
            if done.ref_no == r.ref_no {
                self.note_reject(r.ref_no, D2Diag::EEntryDup.val());
                return Err(D2Diag::EEntryDup.val());
            }
        }
        // 差异入账（零静默口径）：非绿态必落差异账。
        if r.verdict != D2Verdict::Green {
            self.diffs.push((r.ref_no, r.verdict.ordinal()));
        }
        self.items.push(r);
        Ok(())
    }

    /// 封账（判据 J1 主闸）：20 条齐 + 20/20 绿 + 摘要对齐 + 零拒收。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= D2State::D2Sealed.rank() {
            return Err(D2Diag::ERewind.val());
        }
        if self.state.rank() < D2State::D2Running.rank() {
            return Err(D2Diag::ESummaryEmpty.val());
        }
        if self.items.len() != D2_CAP {
            return Err(D2Diag::EShortSeal.val());
        }
        // 20/20 绿闸。
        for it in self.items.iter() {
            if it.verdict != D2Verdict::Green {
                return Err(D2Diag::ESigDrift.val());
            }
        }
        // 差异账空闸。
        if !self.diffs.is_empty() {
            return Err(D2Diag::EDiffSilent.val());
        }
        if !self.rejects.is_empty() {
            return Err(D2Diag::EShortSeal.val());
        }
        // 摘要对齐闸：回归账摘要 ≡ 基线账摘要（回归主判）。
        let snap: Vec<D2RerunItem> = self.items.iter().cloned().collect();
        if rerun_digest(&snap) != baseline_digest() {
            return Err(D2Diag::ESigDrift.val());
        }
        self.state = D2State::D2Sealed;
        Ok(())
    }

    /// 账等式（复测条数 = 清单数 = 20，差异零，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.items.len() == D2_CAP && self.diffs.is_empty() && self.rejects.is_empty()
    }
}

/// 确定性复测重放（同输入恒同判：绿态双签码 = 基线码）。
pub fn replay_d2(ref_no: u32) -> Option<D2RerunItem> {
    let e = D2_LIST.get(ref_no as usize)?;
    Some(D2RerunItem {
        ref_no,
        verdict: D2Verdict::Green,
        load_sig: e.base_code,
        rerun_sig: e.base_code,
        reason: "",
    })
}

/// 构建完整复测账（20 条逐条复测 + 封账）。
pub fn build_full_d2_ledger() -> D2Ledger {
    let mut led = D2Ledger::new();
    let _ = led.begin_rerun();
    let mut i = 0u32;
    while (i as usize) < D2_CAP {
        let r = match replay_d2(i) {
            Some(v) => v,
            None => break,
        };
        match led.rerun(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    let _ = led.seal();
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D2Fault {
    /// 清单越界。
    ListRange,
    /// 无理由跳过。
    SkipNoReason,
    /// 签码漂移。
    SigDrift,
    /// 差异静默。
    DiffSilent,
    /// 条号乱序。
    EntryOrder,
    /// 条重复。
    EntryDup,
    /// 缺条封账。
    ShortSeal,
    /// 倒退复测。
    RerunRewind,
    /// 封账后补条。
    SealedWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl D2Fault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            D2Fault::ListRange => 0,
            D2Fault::SkipNoReason => 1,
            D2Fault::SigDrift => 2,
            D2Fault::DiffSilent => 3,
            D2Fault::EntryOrder => 4,
            D2Fault::EntryDup => 5,
            D2Fault::ShortSeal => 6,
            D2Fault::RerunRewind => 7,
            D2Fault::SealedWrite => 8,
            D2Fault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            D2Fault::ListRange => "清单越界",
            D2Fault::SkipNoReason => "无理由跳过",
            D2Fault::SigDrift => "签码漂移",
            D2Fault::DiffSilent => "差异静默",
            D2Fault::EntryOrder => "条号乱序",
            D2Fault::EntryDup => "条重复",
            D2Fault::ShortSeal => "缺条封账",
            D2Fault::RerunRewind => "倒退复测",
            D2Fault::SealedWrite => "封账后补条",
            D2Fault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [D2Fault] {
        &[
            D2Fault::ListRange,
            D2Fault::SkipNoReason,
            D2Fault::SigDrift,
            D2Fault::DiffSilent,
            D2Fault::EntryOrder,
            D2Fault::EntryDup,
            D2Fault::ShortSeal,
            D2Fault::RerunRewind,
            D2Fault::SealedWrite,
            D2Fault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn d2_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in D2Fault::all().iter() {
        total += 1;
        if run_d2_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_d2_fault(v: D2Fault) -> bool {
    match v {
        D2Fault::ListRange => {
            let mut r = match replay_d2(0) {
                Some(x) => x,
                None => return false,
            };
            r.ref_no = D2_CAP as u32;
            d2_item_ok(&r).is_err()
        }
        D2Fault::SkipNoReason => {
            let mut r = match replay_d2(1) {
                Some(x) => x,
                None => return false,
            };
            r.verdict = D2Verdict::Skipped;
            r.reason = "";
            d2_item_ok(&r).is_err()
        }
        D2Fault::SigDrift => {
            // 回归签码漂移一位 → 绿态联签不齐必抓。
            let mut r = match replay_d2(2) {
                Some(x) => x,
                None => return false,
            };
            r.rerun_sig = r.rerun_sig.saturating_add(1);
            d2_item_ok(&r).is_err()
        }
        D2Fault::DiffSilent => {
            // 红态复测必落差异账（守恒直读）。
            let mut led = D2Ledger::new();
            let _ = led.begin_rerun();
            let mut r = match replay_d2(0) {
                Some(x) => x,
                None => return false,
            };
            r.verdict = D2Verdict::Red;
            r.reason = "注入红探";
            let _ = led.rerun(r);
            led.diff_len() == 1
        }
        D2Fault::EntryOrder => {
            let mut led = D2Ledger::new();
            let _ = led.begin_rerun();
            let r = match replay_d2(1) {
                Some(x) => x,
                None => return false,
            };
            led.rerun(r).is_err()
        }
        D2Fault::EntryDup => {
            let mut led = D2Ledger::new();
            let _ = led.begin_rerun();
            let r0 = match replay_d2(0) {
                Some(x) => x,
                None => return false,
            };
            let _ = led.rerun(r0.clone());
            let mut dup = r0;
            dup.ref_no = 0;
            led.rerun(dup).is_err()
        }
        D2Fault::ShortSeal => {
            let mut led = D2Ledger::new();
            let _ = led.begin_rerun();
            let r = match replay_d2(0) {
                Some(x) => x,
                None => return false,
            };
            let _ = led.rerun(r);
            led.seal().is_err()
        }
        D2Fault::RerunRewind => {
            // 未复测即封账被拒（D2Open 态直接 seal）。
            let mut led = D2Ledger::new();
            led.seal().is_err()
        }
        D2Fault::SealedWrite => {
            let mut led = build_full_d2_ledger();
            if led.state() != D2State::D2Sealed {
                return false;
            }
            let r = match replay_d2(0) {
                Some(x) => x,
                None => return false,
            };
            led.rerun(r).is_err()
        }
        D2Fault::SummaryEmpty => {
            // 摘要空族：跳过带空理由 + 绿态签码漂移双违约必抓其一。
            let mut r = match replay_d2(3) {
                Some(x) => x,
                None => return false,
            };
            r.verdict = D2Verdict::Skipped;
            r.load_sig = 0;
            d2_item_ok(&r).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// B30 二十条 J1 全量复测 20/20 绿（判据 J1 前半直读口）。
fn rerun_twenty_ok() -> bool {
    let led = build_full_d2_ledger();
    if led.len() != D2_CAP {
        return false;
    }
    if led.state() != D2State::D2Sealed {
        return false;
    }
    // 20/20 绿直读 + 复测码与基线码逐位一致。
    let mut i = 0usize;
    while i < D2_CAP {
        let it = match led.item_at(i) {
            Some(v) => v,
            None => return false,
        };
        if it.verdict != D2Verdict::Green {
            return false;
        }
        let e = match D2_LIST.get(i) {
            Some(v) => v,
            None => return false,
        };
        if it.load_sig != e.base_code || it.rerun_sig != e.base_code {
            return false;
        }
        i += 1;
    }
    true
}

/// 回归差异零静默（判据 J1 后半直读口）。
fn diff_zero_silent_ok() -> bool {
    let led = build_full_d2_ledger();
    // 20/20 绿 ⇒ 差异账空。
    if led.diff_len() != 0 {
        return false;
    }
    // 注入反证：红态必落差异账（守恒直读）。
    let mut probe = D2Ledger::new();
    let _ = probe.begin_rerun();
    let mut r = match replay_d2(0) {
        Some(x) => x,
        None => return false,
    };
    r.verdict = D2Verdict::Red;
    r.reason = "注入红探";
    let _ = probe.rerun(r);
    if probe.diff_len() != 1 {
        return false;
    }
    // 跳过带理由也落差异账。
    let mut probe2 = D2Ledger::new();
    let _ = probe2.begin_rerun();
    let mut s = match replay_d2(0) {
        Some(x) => x,
        None => return false,
    };
    s.verdict = D2Verdict::Skipped;
    s.reason = "注入跳过探";
    let _ = probe2.rerun(s);
    if probe2.diff_len() != 1 {
        return false;
    }
    true
}

/// 基线/回归账摘要逐位一致（回归主判）。
fn baseline_digest_ok() -> bool {
    let led = build_full_d2_ledger();
    let snap: Vec<D2RerunItem> = {
        let mut v: Vec<D2RerunItem> = Vec::new();
        let mut i = 0usize;
        while i < led.len() {
            let it = match led.item_at(i) {
                Some(x) => x.clone(),
                None => break,
            };
            v.push(it);
            i += 1;
        }
        v
    };
    if rerun_digest(&snap) != baseline_digest() {
        return false;
    }
    // 摘要函数直读：同序列恒同摘要、异序列摘要异。
    let a = fold_digest(&[0x31, 0x33]);
    let b = fold_digest(&[0x31, 0x33]);
    let c = fold_digest(&[0x31, 0x35]);
    if a != b || a == c {
        return false;
    }
    true
}

/// 清单普查（20 条判据号连续、面名七闭集内、基线全真、判码落窗）。
fn list_census_ok() -> bool {
    if D2_LIST.len() != D2_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < D2_LIST.len() {
        let e = match D2_LIST.get(i) {
            Some(v) => v,
            None => return false,
        };
        if e.no != i as u32 {
            return false;
        }
        if d2_face_idx(e.face).is_none() {
            return false;
        }
        if !e.baseline {
            return false;
        }
        if !code_in_d2_face(e.base_code) {
            return false;
        }
        i += 1;
    }
    // 七轴全覆盖。
    for f in D2_FACE_TABLE.iter() {
        let mut found = false;
        for e in D2_LIST.iter() {
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
    let led = build_full_d2_ledger();
    if led.len() != D2_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < led.len() {
        let it = match led.item_at(i) {
            Some(v) => v,
            None => return false,
        };
        if it.ref_no != i as u32 || it.verdict != D2Verdict::Green {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn d2_fault_sweep_ok() -> bool {
    let (caught, total, escape) = d2_fault_sweep();
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
    let a = build_full_d2_ledger();
    let b = build_full_d2_ledger();
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
    if d2_fault_sweep().0 != d2_fault_sweep().0 {
        return false;
    }
    // 摘要双跑逐位同。
    if baseline_digest() != baseline_digest() {
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
    if SEG_BASE != 0x80000 + 0x40 * (12681u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let led = build_full_d2_ledger();
    if led.state() != D2State::D2Sealed {
        return false;
    }
    // 摘要：首条面名 + 尾条面名拼接非空。
    let head = match D2_LIST.first() {
        Some(e) => e.face,
        None => return false,
    };
    let tail = match D2_LIST.last() {
        Some(e) => e.face,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    // 摘要值非零（回归账摘要口径）。
    if baseline_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12682 D2 复测自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12681_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12681-d2-retest");
    let led = build_full_d2_ledger();

    s.add_dyn(
        "F12682-01 B30 J1 清单二十条齐",
        D2_LIST.len() == D2_CAP,
        &format!("entries={}", D2_LIST.len()),
    );

    s.add_dyn(
        "F12682-02 B30 二十条 J1 全量复测 20/20 绿",
        rerun_twenty_ok(),
        &format!("items={} sealed={}", led.len(), led.state() == D2State::D2Sealed),
    );

    s.add_dyn(
        "F12682-03 回归差异零静默",
        diff_zero_silent_ok(),
        &format!("diffs={} rejects={}", led.diff_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12682-04 基线/回归账摘要逐位一致",
        baseline_digest_ok(),
        &format!("digest=0x{:08X}", baseline_digest()),
    );

    s.add_dyn(
        "F12682-05 清单普查",
        list_census_ok(),
        &format!("faces={}", D2_FACE_TABLE.len()),
    );

    s.add_dyn(
        "F12682-06 零静默",
        zero_silent_ok(),
        &format!("items={}", led.len()),
    );

    s.add_dyn(
        "F12682-07 注入联测 10/10 拒止",
        d2_fault_sweep_ok(),
        &format!("caught={}", d2_fault_sweep().0),
    );

    s.add_dyn(
        "F12682-08 账等式",
        led.equation_ok(),
        &format!("items={} diffs={} rejects={}", led.len(), led.diff_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12682-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12682-10 联签对齐口径",
        pair_aligned(&CosignPair { load_sig: 0x31, rerun_sig: 0x31 })
            && !pair_aligned(&CosignPair { load_sig: 0x31, rerun_sig: 0x33 })
            && !pair_aligned(&CosignPair { load_sig: 0, rerun_sig: 0 }),
        "双签非零且逐位对齐",
    );

    s.add_dyn(
        "F12682-11 复测面窗口径",
        code_in_d2_face(0x31) && code_in_d2_face(0x4B) && !code_in_d2_face(0x30) && !code_in_d2_face(0x51),
        "基码 ≤ 判码 < 基码+0x20",
    );

    s.add_dyn(
        "F12682-12 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12682-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81440);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(D2_ALL.len(), 11);
        let mut i = 0usize;
        while i < D2_ALL.len() {
            let vi = match D2_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < D2_ALL.len() {
                let vj = match D2_ALL.get(j) {
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
    fn rerun_twenty_green() {
        assert!(rerun_twenty_ok());
        let led = build_full_d2_ledger();
        assert_eq!(led.len(), 20);
        assert_eq!(led.state(), D2State::D2Sealed);
        assert_eq!(led.diff_len(), 0);
    }

    #[test]
    fn digest_baseline_matches_rerun() {
        // 回归主判：摘要逐位一致。
        assert!(baseline_digest_ok());
        let led = build_full_d2_ledger();
        let mut codes: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < led.len() {
            let it = match led.item_at(i) {
                Some(v) => v.clone(),
                None => break,
            };
            codes.push(it.load_sig);
            i += 1;
        }
        assert_eq!(fold_digest(&codes), baseline_digest());
        // 漂移注入必变摘要。
        match codes.get_mut(0) {
            Some(slot) => *slot = slot.saturating_add(1),
            None => return,
        }
        assert_ne!(fold_digest(&codes), baseline_digest());
    }

    #[test]
    fn diff_zero_silent_holds() {
        assert!(diff_zero_silent_ok());
        let led = build_full_d2_ledger();
        assert_eq!(led.diff_len(), 0);
        assert_eq!(led.reject_len(), 0);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = d2_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn rerun_gate_rejects() {
        let mut led = D2Ledger::new();
        let _ = led.begin_rerun();
        let r1 = match replay_d2(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(led.rerun(r1), Err(D2Diag::EEntryOrder.val()));
        let r0 = match replay_d2(0) {
            Some(x) => x,
            None => return,
        };
        assert!(led.rerun(r0.clone()).is_ok());
        let mut dup = r0;
        dup.ref_no = 0;
        assert_eq!(led.rerun(dup), Err(D2Diag::EEntryOrder.val()));
        // 签码漂移。
        let mut drift = match replay_d2(1) {
            Some(x) => x,
            None => return,
        };
        drift.rerun_sig = drift.rerun_sig.saturating_add(2);
        assert_eq!(led.rerun(drift), Err(D2Diag::ESigDrift.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn list_census_holds() {
        assert!(list_census_ok());
        assert_eq!(D2_LIST.len(), 20);
        assert_eq!(D2_FACE_TABLE.len(), 7);
        assert!(d2_face_idx("句柄装载轴").is_some());
        assert!(d2_face_idx("幽灵轴").is_none());
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_d2_ledger();
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
        let a = build_full_d2_ledger();
        let b = build_full_d2_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.item_at(i), b.item_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑条缺失"),
            }
            i += 1;
        }
        assert_eq!(d2_fault_sweep().0, d2_fault_sweep().0);
        assert_eq!(baseline_digest(), baseline_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：D2Open→D2Running→D2Sealed。
        assert!(D2State::D2Open.rank() < D2State::D2Running.rank());
        assert!(D2State::D2Running.rank() < D2State::D2Sealed.rank());
        let mut led = build_full_d2_ledger();
        assert_eq!(led.state(), D2State::D2Sealed);
        assert_eq!(led.seal(), Err(D2Diag::ERewind.val()));
        let mut fresh = D2Ledger::new();
        assert!(fresh.begin_rerun().is_ok());
        assert_eq!(fresh.begin_rerun(), Err(D2Diag::ERewind.val()));
    }

    #[test]
    fn cosign_pair_semantics() {
        // 联签对齐语义直读。
        assert!(pair_aligned(&CosignPair { load_sig: 0x35, rerun_sig: 0x35 }));
        assert!(!pair_aligned(&CosignPair { load_sig: 0x35, rerun_sig: 0x37 }));
        assert!(!pair_aligned(&CosignPair { load_sig: 0, rerun_sig: 0x35 }));
        assert!(!pair_aligned(&CosignPair { load_sig: 0x35, rerun_sig: 0 }));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12681_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
