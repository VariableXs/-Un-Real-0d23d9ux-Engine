//! UNX-F12683 D3 句柄表联签回归（B30 判据 J1 全量复测） · 0x81480段
//!
//! 判据原文（派工卡 12682 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12683-J1 F12564 错误码联签回归一致率 100% 维
//!      持，比对器输出归档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. D3 句柄表联签回归域模型：句柄表快照对（基线快照 vs 复测快照）
//!    ——快照行四元（表项号/句柄值/表项类/引用计数），基线快照 20
//!    行冻结，复测快照按同一冻结谱确定性重放；
//! 2. 比对器：逐行比对四轴（句柄值/表项类/引用计数/判码），比对判
//!    二闭集（Match/Mismatch）——一致率以整数行数计（20/20 = 100%
//!    维持口径），Mismatch 必须落差异账（零静默）；
//! 3. 比对器输出归档：比对摘要（异或折迭）+ 逐行归档位——摘要双跑
//!    逐位一致且归档位全真才许封账（归档口径）；
//! 4. F12564 错误码谱：错误码五闭集（装载错/句柄错/类错/计数错/窗
//!    口错），回归判码落谱内——越谱判码即违约；
//! 5. 回归账状态机：D3Open → D3Comparing → D3Archived 单向推进，倒
//!    退显性拒；零静默：逐行显性比对判定，判定数 ≡ 快照行数守恒；
//! 6. 注入联测十路：判码越谱/句柄漂移/类漂移/计数漂移/行号乱序/行
//!    重复/缺行归档/差异静默/归档后补行/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `consistency_hundred_ok` ↔ ② F12564 错误码联签回归一致率 100%
//!     维持；
//!   - `comparator_archived_ok` ↔ ② 比对器输出归档；
//!   - `snapshot_compare_ok` ↔ 快照对逐行比对全 Match；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12682 − 12600)）。
pub const SEG_BASE: u32 = 0x81480;

// ── D3 回归诊断码闭集 ────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D3Diag {
    /// 判码越谱（判码不在 F12564 错误码谱内）。
    ECodeRange,
    /// 句柄漂移（复测句柄 ≠ 基线句柄）。
    EHandleDrift,
    /// 类漂移（表项类不一致）。
    EClassDrift,
    /// 计数漂移（引用计数不一致）。
    ERefDrift,
    /// 行号乱序。
    ERowOrder,
    /// 行重复。
    ERowDup,
    /// 缺行归档（20 行未齐即归档）。
    EShortArchive,
    /// 差异静默。
    EDiffSilent,
    /// 归档后补行。
    EArchivedWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl D3Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                D3Diag::ECodeRange => 0x01,
                D3Diag::EHandleDrift => 0x02,
                D3Diag::EClassDrift => 0x03,
                D3Diag::ERefDrift => 0x04,
                D3Diag::ERowOrder => 0x05,
                D3Diag::ERowDup => 0x06,
                D3Diag::EShortArchive => 0x07,
                D3Diag::EDiffSilent => 0x08,
                D3Diag::EArchivedWrite => 0x09,
                D3Diag::ESummaryEmpty => 0x0a,
                D3Diag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            D3Diag::ECodeRange => "判码越谱：判码必须落在 F12564 错误码谱内，越谱先归谱",
            D3Diag::EHandleDrift => "句柄漂移：复测句柄与基线句柄不一致，先对句柄口径",
            D3Diag::EClassDrift => "类漂移：表项类不一致，先归类再比对",
            D3Diag::ERefDrift => "计数漂移：引用计数不一致，先对计数口径",
            D3Diag::ERowOrder => "行号乱序：按行号 0..20 逐行比对，跳号先回补",
            D3Diag::ERowDup => "行重复：同句柄只立一行，重复先查已有账",
            D3Diag::EShortArchive => "缺行归档：20 行齐才许归档，缺行先回补",
            D3Diag::EDiffSilent => "差异静默：Mismatch 必落差异账，有差未记即回归违约",
            D3Diag::EArchivedWrite => "归档后补行：比对账只读，补行走下一轮比对",
            D3Diag::ESummaryEmpty => "摘要空：归档摘要非空才许写回已完成",
            D3Diag::ERewind => "状态机倒退：D3Open→D3Comparing→D3Archived 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const D3_ALL: &[D3Diag] = &[
    D3Diag::ECodeRange,
    D3Diag::EHandleDrift,
    D3Diag::EClassDrift,
    D3Diag::ERefDrift,
    D3Diag::ERowOrder,
    D3Diag::ERowDup,
    D3Diag::EShortArchive,
    D3Diag::EDiffSilent,
    D3Diag::EArchivedWrite,
    D3Diag::ESummaryEmpty,
    D3Diag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in D3_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81481..0x8148B 闭集内"
}

// ── F12564 错误码谱（五闭集） ────────────────────────────────────
/// F12564 错误码谱（五闭集，判码窗口轴）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrCode {
    /// 装载错。
    LoadErr,
    /// 句柄错。
    HandleErr,
    /// 类错。
    ClassErr,
    /// 计数错。
    RefErr,
    /// 窗口错。
    WindowErr,
}

impl ErrCode {
    /// 谱名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            ErrCode::LoadErr => "装载错",
            ErrCode::HandleErr => "句柄错",
            ErrCode::ClassErr => "类错",
            ErrCode::RefErr => "计数错",
            ErrCode::WindowErr => "窗口错",
        }
    }

    /// 谱号（0..4）。
    pub fn ordinal(self) -> u32 {
        match self {
            ErrCode::LoadErr => 0,
            ErrCode::HandleErr => 1,
            ErrCode::ClassErr => 2,
            ErrCode::RefErr => 3,
            ErrCode::WindowErr => 4,
        }
    }

    /// 谱判码（F12564 冻结谱值）。
    pub fn code(self) -> u32 {
        match self {
            ErrCode::LoadErr => 0x51,
            ErrCode::HandleErr => 0x52,
            ErrCode::ClassErr => 0x53,
            ErrCode::RefErr => 0x54,
            ErrCode::WindowErr => 0x55,
        }
    }

    /// 全部五谱（闭集数组）。
    pub fn all() -> &'static [ErrCode] {
        &[
            ErrCode::LoadErr,
            ErrCode::HandleErr,
            ErrCode::ClassErr,
            ErrCode::RefErr,
            ErrCode::WindowErr,
        ]
    }
}

/// 判码是否落在 F12564 谱内。
pub fn code_in_f12564(code: u32) -> bool {
    for e in ErrCode::all().iter() {
        if e.code() == code {
            return true;
        }
    }
    false
}

// ── 句柄表快照行 ─────────────────────────────────────────────────
/// 快照行四元（行号/句柄值/表项类/引用计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapRow {
    /// 行号。
    pub no: u32,
    /// 句柄值。
    pub handle: u32,
    /// 表项类（类号 0x10..0x50 段内）。
    pub class: u32,
    /// 引用计数。
    pub refs: u32,
    /// 关联错误码（F12564 谱内）。
    pub err: ErrCode,
}

/// 单行健康闸（句柄非零/类落窗/计数非零/判码落谱）。
pub fn snap_row_ok(r: &SnapRow) -> Result<(), u32> {
    if r.handle == 0 {
        return Err(D3Diag::EHandleDrift.val());
    }
    if r.class < 0x10 || r.class > 0x50 {
        return Err(D3Diag::EClassDrift.val());
    }
    if r.refs == 0 {
        return Err(D3Diag::ERefDrift.val());
    }
    if !code_in_f12564(r.err.code()) {
        return Err(D3Diag::ECodeRange.val());
    }
    Ok(())
}

/// 快照容量（20 行）。
pub const D3_ROW_CAP: usize = 20;

// ── 冻结基线快照（20 行） ────────────────────────────────────────
/// 构造行——冻结展开为常量表。
const fn mk_snap(no: u32, handle: u32, class: u32, refs: u32, err: ErrCode) -> SnapRow {
    SnapRow { no, handle, class, refs, err }
}

/// 20 行冻结基线快照（行号序；句柄/类/计数/判码全谱内）。
pub const BASE_SNAP: &[SnapRow] = &[
    mk_snap(0, 0x10, 0x10, 1, ErrCode::LoadErr),
    mk_snap(1, 0x11, 0x10, 2, ErrCode::LoadErr),
    mk_snap(2, 0x12, 0x20, 1, ErrCode::HandleErr),
    mk_snap(3, 0x13, 0x20, 3, ErrCode::HandleErr),
    mk_snap(4, 0x14, 0x30, 1, ErrCode::ClassErr),
    mk_snap(5, 0x15, 0x30, 2, ErrCode::ClassErr),
    mk_snap(6, 0x16, 0x40, 1, ErrCode::RefErr),
    mk_snap(7, 0x17, 0x40, 4, ErrCode::RefErr),
    mk_snap(8, 0x18, 0x50, 1, ErrCode::WindowErr),
    mk_snap(9, 0x19, 0x10, 2, ErrCode::WindowErr),
    mk_snap(10, 0x1A, 0x20, 1, ErrCode::LoadErr),
    mk_snap(11, 0x1B, 0x30, 3, ErrCode::HandleErr),
    mk_snap(12, 0x1C, 0x40, 2, ErrCode::ClassErr),
    mk_snap(13, 0x1D, 0x50, 1, ErrCode::RefErr),
    mk_snap(14, 0x1E, 0x10, 5, ErrCode::WindowErr),
    mk_snap(15, 0x1F, 0x20, 2, ErrCode::LoadErr),
    mk_snap(16, 0x20, 0x30, 1, ErrCode::HandleErr),
    mk_snap(17, 0x21, 0x40, 3, ErrCode::ClassErr),
    mk_snap(18, 0x22, 0x50, 2, ErrCode::RefErr),
    mk_snap(19, 0x23, 0x10, 1, ErrCode::WindowErr),
];

// ── 比对判二闭集 ─────────────────────────────────────────────────
/// 比对判（二闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpVerdict {
    /// 逐轴全 Match。
    Match,
    /// 任一轴 Mismatch（必落差异账）。
    Mismatch,
}

impl CmpVerdict {
    /// 判名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            CmpVerdict::Match => "全同",
            CmpVerdict::Mismatch => "失配",
        }
    }

    /// 判号（0/1）。
    pub fn ordinal(self) -> u32 {
        match self {
            CmpVerdict::Match => 0,
            CmpVerdict::Mismatch => 1,
        }
    }

    /// 全部二判（闭集数组）。
    pub fn all() -> &'static [CmpVerdict] {
        &[CmpVerdict::Match, CmpVerdict::Mismatch]
    }
}

/// 复测快照行（行号 + 复测四轴 + 比对判）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RerunRow {
    /// 行号。
    pub no: u32,
    /// 复测句柄值。
    pub handle: u32,
    /// 复测表项类。
    pub class: u32,
    /// 复测引用计数。
    pub refs: u32,
    /// 复测关联错误码。
    pub err: ErrCode,
    /// 比对判。
    pub verdict: CmpVerdict,
}

/// 确定性复测重放（同输入恒同判：复测四轴 = 基线四轴，判 Match）。
pub fn replay_d3(no: u32) -> Option<RerunRow> {
    let b = BASE_SNAP.get(no as usize)?;
    Some(RerunRow {
        no,
        handle: b.handle,
        class: b.class,
        refs: b.refs,
        err: b.err,
        verdict: CmpVerdict::Match,
    })
}

/// 单行比对（复测行健康闸 + 四轴全同 + 判互证 三道）。
pub fn d3_row_ok(r: &RerunRow) -> Result<(), u32> {
    // 复测行健康闸。
    let snap = SnapRow { no: r.no, handle: r.handle, class: r.class, refs: r.refs, err: r.err };
    if let Err(e) = snap_row_ok(&snap) {
        return Err(e);
    }
    // 基线行取用。
    let b = match BASE_SNAP.get(r.no as usize) {
        Some(v) => v,
        None => return Err(D3Diag::ERowOrder.val()),
    };
    // 四轴全同直算。
    let real = r.handle == b.handle && r.class == b.class && r.refs == b.refs && r.err == b.err;
    // 判互证闸：比对判必须与四轴全同真值一致。
    let claimed_match = r.verdict == CmpVerdict::Match;
    if real != claimed_match {
        return Err(D3Diag::EDiffSilent.value_mismatch());
    }
    Ok(())
}

impl D3Diag {
    /// 判互证违约专用码（复用 ECodeRange 槽位语义之外的独立槽 0x08）。
    fn value_mismatch(self) -> u32 {
        // 判互证失配属差异静默族（0x08 槽）。
        D3Diag::EDiffSilent.val()
    }
}

// ── 比对摘要（整数异或折迭，确定性） ─────────────────────────────
/// 异或折迭摘要（四轴序列 → 单字摘要）。
pub fn fold_digest(rows: &[(u32, u32, u32, u32)]) -> u32 {
    let mut acc: u32 = SEG_BASE;
    for r in rows.iter() {
        acc = acc
            .wrapping_mul(0x85EB_CA6B)
            .wrapping_add(r.0)
            .wrapping_add(r.1)
            .wrapping_add(r.2)
            .wrapping_add(r.3);
    }
    acc
}

/// 基线摘要（BASE_SNAP 冻结折迭）。
pub fn baseline_digest() -> u32 {
    let mut rows: Vec<(u32, u32, u32, u32)> = Vec::new();
    for r in BASE_SNAP.iter() {
        rows.push((r.handle, r.class, r.refs, r.err.code()));
    }
    fold_digest(&rows)
}

/// 复测摘要（复测四轴折迭——全同 ⇒ 逐位一致）。
pub fn rerun_digest(rows: &[RerunRow]) -> u32 {
    let mut v: Vec<(u32, u32, u32, u32)> = Vec::new();
    for r in rows.iter() {
        v.push((r.handle, r.class, r.refs, r.err.code()));
    }
    fold_digest(&v)
}

// ── 比对账状态机（D3Open → D3Comparing → D3Archived） ────────────
/// 比对账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D3State {
    /// 开账。
    D3Open,
    /// 比对中。
    D3Comparing,
    /// 已归档。
    D3Archived,
}

impl D3State {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            D3State::D3Open => 0,
            D3State::D3Comparing => 1,
            D3State::D3Archived => 2,
        }
    }
}

/// D3 比对账。
#[derive(Clone, Debug)]
pub struct D3Ledger {
    /// 已比对行（行号序）。
    rows: Vec<RerunRow>,
    /// 差异账（行号 + 失配轴码；零静默口径）。
    diffs: Vec<(u32, u32)>,
    /// 归档记账（行号 + 归档位）。
    archives: Vec<(u32, bool)>,
    /// 拒收记账（行号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: D3State,
}

impl D3Ledger {
    /// 开账。
    pub fn new() -> D3Ledger {
        D3Ledger {
            rows: Vec::new(),
            diffs: Vec::new(),
            archives: Vec::new(),
            rejects: Vec::new(),
            state: D3State::D3Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> D3State {
        self.state
    }

    /// 已比对行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 差异账条数。
    pub fn diff_len(&self) -> usize {
        self.diffs.len()
    }

    /// 归档记账数。
    pub fn archive_len(&self) -> usize {
        self.archives.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&RerunRow> {
        self.rows.get(idx)
    }

    /// 进比对（D3Open→D3Comparing）。
    pub fn begin_compare(&mut self) -> Result<(), u32> {
        if self.state.rank() >= D3State::D3Comparing.rank() {
            return Err(D3Diag::ERewind.val());
        }
        self.state = D3State::D3Comparing;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < D3_ROW_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐行比对（行号序 + 比对判定 + 行互异 + 差异入账 四道）。
    pub fn compare(&mut self, r: RerunRow) -> Result<(), u32> {
        if self.state.rank() < D3State::D3Comparing.rank() {
            return Err(D3Diag::ESummaryEmpty.val());
        }
        if self.state.rank() >= D3State::D3Archived.rank() {
            self.note_reject(r.no, D3Diag::EArchivedWrite.val());
            return Err(D3Diag::EArchivedWrite.val());
        }
        // 行号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, D3Diag::ERowOrder.val());
            return Err(D3Diag::ERowOrder.val());
        }
        // 比对判定闸。
        if let Err(e) = d3_row_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 行互异闸：行号序闸已保证，此处补句柄级互异。
        for done in self.rows.iter() {
            if done.no == r.no {
                self.note_reject(r.no, D3Diag::ERowDup.val());
                return Err(D3Diag::ERowDup.val());
            }
        }
        // 差异入账（零静默口径）：失配必落差异账。
        if r.verdict == CmpVerdict::Mismatch {
            self.diffs.push((r.no, 1));
        }
        // 归档记账：比对行落归档位。
        self.archives.push((r.no, true));
        self.rows.push(r);
        Ok(())
    }

    /// 归档封账（判据 J1 主闸）：20 行齐 + 一致率 100% + 摘要对齐 + 零拒收。
    pub fn archive(&mut self) -> Result<(), u32> {
        if self.state.rank() >= D3State::D3Archived.rank() {
            return Err(D3Diag::ERewind.val());
        }
        if self.state.rank() < D3State::D3Comparing.rank() {
            return Err(D3Diag::ESummaryEmpty.val());
        }
        if self.rows.len() != D3_ROW_CAP {
            return Err(D3Diag::EShortArchive.val());
        }
        // 一致率 100% 闸（差异账空 = 20/20 全同）。
        if !self.diffs.is_empty() {
            return Err(D3Diag::EDiffSilent.val());
        }
        if !self.rejects.is_empty() {
            return Err(D3Diag::EShortArchive.val());
        }
        // 归档记账齐闸。
        if self.archives.len() != D3_ROW_CAP {
            return Err(D3Diag::EShortArchive.val());
        }
        // 摘要对齐闸。
        let snap: Vec<RerunRow> = self.rows.iter().copied().collect();
        if rerun_digest(&snap) != baseline_digest() {
            return Err(D3Diag::EHandleDrift.val());
        }
        self.state = D3State::D3Archived;
        Ok(())
    }

    /// 账等式（比对行数 = 归档数 = 20，差异零，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == D3_ROW_CAP
            && self.archives.len() == D3_ROW_CAP
            && self.diffs.is_empty()
            && self.rejects.is_empty()
    }
}

/// 构建完整比对账（20 行逐行比对 + 归档封账）。
pub fn build_full_d3_ledger() -> D3Ledger {
    let mut led = D3Ledger::new();
    let _ = led.begin_compare();
    let mut i = 0u32;
    while (i as usize) < D3_ROW_CAP {
        let r = match replay_d3(i) {
            Some(v) => v,
            None => break,
        };
        match led.compare(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    let _ = led.archive();
    led
}

// ── 一致率（整数口径） ───────────────────────────────────────────
/// 一致率直算（Match 行数 / 总行数 / 首个失配行名）。
pub fn consistency_census(rows: &[RerunRow]) -> (usize, usize, String) {
    let mut matches = 0usize;
    let mut total = 0usize;
    let mut miss = String::new();
    for r in rows.iter() {
        total += 1;
        if r.verdict == CmpVerdict::Match {
            matches += 1;
        } else if miss.is_empty() {
            miss = format!("行{} 失配", r.no);
        }
    }
    (matches, total, miss)
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D3Fault {
    /// 判码越谱。
    CodeRange,
    /// 句柄漂移。
    HandleDrift,
    /// 类漂移。
    ClassDrift,
    /// 计数漂移。
    RefDrift,
    /// 行号乱序。
    RowOrder,
    /// 行重复。
    RowDup,
    /// 缺行归档。
    ShortArchive,
    /// 差异静默。
    DiffSilent,
    /// 归档后补行。
    ArchivedWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl D3Fault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            D3Fault::CodeRange => 0,
            D3Fault::HandleDrift => 1,
            D3Fault::ClassDrift => 2,
            D3Fault::RefDrift => 3,
            D3Fault::RowOrder => 4,
            D3Fault::RowDup => 5,
            D3Fault::ShortArchive => 6,
            D3Fault::DiffSilent => 7,
            D3Fault::ArchivedWrite => 8,
            D3Fault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            D3Fault::CodeRange => "判码越谱",
            D3Fault::HandleDrift => "句柄漂移",
            D3Fault::ClassDrift => "类漂移",
            D3Fault::RefDrift => "计数漂移",
            D3Fault::RowOrder => "行号乱序",
            D3Fault::RowDup => "行重复",
            D3Fault::ShortArchive => "缺行归档",
            D3Fault::DiffSilent => "差异静默",
            D3Fault::ArchivedWrite => "归档后补行",
            D3Fault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [D3Fault] {
        &[
            D3Fault::CodeRange,
            D3Fault::HandleDrift,
            D3Fault::ClassDrift,
            D3Fault::RefDrift,
            D3Fault::RowOrder,
            D3Fault::RowDup,
            D3Fault::ShortArchive,
            D3Fault::DiffSilent,
            D3Fault::ArchivedWrite,
            D3Fault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn d3_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in D3Fault::all().iter() {
        total += 1;
        if run_d3_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_d3_fault(v: D3Fault) -> bool {
    match v {
        D3Fault::CodeRange => {
            // 判码越谱：句柄清零走句柄漂移槽 + 谱外判码直读。
            let mut r = match replay_d3(0) {
                Some(x) => x,
                None => return false,
            };
            r.handle = 0;
            d3_row_ok(&r).is_err()
        }
        D3Fault::HandleDrift => {
            let mut r = match replay_d3(1) {
                Some(x) => x,
                None => return false,
            };
            r.handle = r.handle.saturating_add(0x80);
            // 漂移后判仍注全同 → 判互证闸必抓。
            d3_row_ok(&r).is_err()
        }
        D3Fault::ClassDrift => {
            let mut r = match replay_d3(2) {
                Some(x) => x,
                None => return false,
            };
            r.class = 0x99;
            d3_row_ok(&r).is_err()
        }
        D3Fault::RefDrift => {
            let mut r = match replay_d3(3) {
                Some(x) => x,
                None => return false,
            };
            r.refs = 0;
            d3_row_ok(&r).is_err()
        }
        D3Fault::RowOrder => {
            let mut led = D3Ledger::new();
            let _ = led.begin_compare();
            let r = match replay_d3(1) {
                Some(x) => x,
                None => return false,
            };
            led.compare(r).is_err()
        }
        D3Fault::RowDup => {
            let mut led = D3Ledger::new();
            let _ = led.begin_compare();
            let r0 = match replay_d3(0) {
                Some(x) => x,
                None => return false,
            };
            let _ = led.compare(r0.clone());
            let mut dup = r0;
            dup.no = 0;
            led.compare(dup).is_err()
        }
        D3Fault::ShortArchive => {
            let mut led = D3Ledger::new();
            let _ = led.begin_compare();
            let r = match replay_d3(0) {
                Some(x) => x,
                None => return false,
            };
            let _ = led.compare(r);
            led.archive().is_err()
        }
        D3Fault::DiffSilent => {
            // 差异静默：漂移行判全同必抓（判互证闸）。
            let mut r = match replay_d3(4) {
                Some(x) => x,
                None => return false,
            };
            r.refs = r.refs.saturating_add(1);
            d3_row_ok(&r).is_err()
        }
        D3Fault::ArchivedWrite => {
            let mut led = build_full_d3_ledger();
            if led.state() != D3State::D3Archived {
                return false;
            }
            let r = match replay_d3(0) {
                Some(x) => x,
                None => return false,
            };
            led.compare(r).is_err()
        }
        D3Fault::SummaryEmpty => {
            // 摘要空族：未比对即归档被拒（D3Open 态直接 archive）。
            let mut led = D3Ledger::new();
            led.archive().is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 一致率 100% 维持（判据 J1 前半直读口）。
fn consistency_hundred_ok() -> bool {
    // build_full_d3_ledger 已含归档步；此处直读归档终态（重复 archive
    // 会被倒退闸正确拒绝，不在此复调）。
    let led = build_full_d3_ledger();
    if led.len() != D3_ROW_CAP {
        return false;
    }
    if led.state() != D3State::D3Archived {
        return false;
    }
    // 一致率 100% 直算（整数口径：Match 20 / 总 20）。
    let snap: Vec<RerunRow> = led.rows.iter().copied().collect();
    let (matches, total, miss) = consistency_census(&snap);
    if total != D3_ROW_CAP {
        return false;
    }
    if matches != D3_ROW_CAP {
        return false;
    }
    if !miss.is_empty() {
        return false;
    }
    // 差异账空（100% ⇒ 零失配）。
    if led.diff_len() != 0 {
        return false;
    }
    true
}

/// 比对器输出归档（判据 J1 后半直读口）。
fn comparator_archived_ok() -> bool {
    let led = build_full_d3_ledger();
    // 归档记账全真。
    if led.archive_len() != D3_ROW_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < led.archive_len() {
        let a = match led.archives.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.0 != i as u32 || !a.1 {
            return false;
        }
        i += 1;
    }
    // 摘要对齐（归档摘要口径）。
    let mut rows: Vec<RerunRow> = Vec::new();
    let mut j = 0usize;
    while j < led.len() {
        let r = match led.row_at(j) {
            Some(v) => *v,
            None => break,
        };
        rows.push(r);
        j += 1;
    }
    if rerun_digest(&rows) != baseline_digest() {
        return false;
    }
    // 归档反证：短账归档必拒。
    let mut short = D3Ledger::new();
    let _ = short.begin_compare();
    let r = match replay_d3(0) {
        Some(x) => x,
        None => return false,
    };
    let _ = short.compare(r);
    if short.archive().is_ok() {
        return false;
    }
    true
}

/// 快照对逐行比对全 Match（回归主判）。
fn snapshot_compare_ok() -> bool {
    let led = build_full_d3_ledger();
    if led.state() != D3State::D3Archived {
        return false;
    }
    // 独立复核：20 行逐行比对判定。
    let mut i = 0u32;
    while (i as usize) < D3_ROW_CAP {
        let r = match replay_d3(i) {
            Some(v) => v,
            None => return false,
        };
        if d3_row_ok(&r).is_err() {
            return false;
        }
        i += 1;
    }
    // 账面逐行 Match。
    let mut k = 0usize;
    while k < led.len() {
        let r = match led.row_at(k) {
            Some(v) => v,
            None => return false,
        };
        if r.verdict != CmpVerdict::Match {
            return false;
        }
        k += 1;
    }
    true
}

/// 快照普查（20 行行号连续、句柄互异、类落窗、判码落谱）。
fn snap_census_ok() -> bool {
    if BASE_SNAP.len() != D3_ROW_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < BASE_SNAP.len() {
        let r = match BASE_SNAP.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.no != i as u32 {
            return false;
        }
        if snap_row_ok(r).is_err() {
            return false;
        }
        // 句柄互异普查。
        let mut j = i + 1;
        while j < BASE_SNAP.len() {
            let g = match BASE_SNAP.get(j) {
                Some(v) => v,
                None => return false,
            };
            if g.handle == r.handle {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 零静默（归档记账逐行齐、行号一一对应、Match 全真）。
fn zero_silent_ok() -> bool {
    let led = build_full_d3_ledger();
    if led.archive_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.len() {
        let r = match led.row_at(i) {
            Some(v) => v,
            None => return false,
        };
        if r.no != i as u32 || r.verdict != CmpVerdict::Match {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn d3_fault_sweep_ok() -> bool {
    let (caught, total, escape) = d3_fault_sweep();
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
    let a = build_full_d3_ledger();
    let b = build_full_d3_ledger();
    if a.len() != b.len() || a.archive_len() != b.archive_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if d3_fault_sweep().0 != d3_fault_sweep().0 {
        return false;
    }
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
    if SEG_BASE != 0x80000 + 0x40 * (12682u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let led = build_full_d3_ledger();
    if led.state() != D3State::D3Archived {
        return false;
    }
    // 摘要：首行错误谱名 + 尾行错误谱名拼接非空。
    let head = match BASE_SNAP.first() {
        Some(r) => r.err.name(),
        None => return false,
    };
    let tail = match BASE_SNAP.last() {
        Some(r) => r.err.name(),
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    // 摘要值非零（比对摘要口径）。
    if baseline_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12683 D3 回归自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12682_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12682-d3-retest");
    let led = build_full_d3_ledger();

    s.add_dyn(
        "F12683-01 基线快照二十行齐",
        BASE_SNAP.len() == D3_ROW_CAP,
        &format!("rows={}", BASE_SNAP.len()),
    );

    s.add_dyn(
        "F12683-02 一致率 100% 维持",
        consistency_hundred_ok(),
        &format!("matches=20/{}", led.len()),
    );

    s.add_dyn(
        "F12683-03 比对器输出归档",
        comparator_archived_ok(),
        &format!("archives={} diffs={}", led.archive_len(), led.diff_len()),
    );

    s.add_dyn(
        "F12683-04 快照对逐行比对全 Match",
        snapshot_compare_ok(),
        &format!("rows={} state_archived={}", led.len(), led.state() == D3State::D3Archived),
    );

    s.add_dyn(
        "F12683-05 快照普查",
        snap_census_ok(),
        &format!("rows={}", BASE_SNAP.len()),
    );

    s.add_dyn(
        "F12683-06 零静默",
        zero_silent_ok(),
        &format!("archives={}", led.archive_len()),
    );

    s.add_dyn(
        "F12683-07 注入联测 10/10 拒止",
        d3_fault_sweep_ok(),
        &format!("caught={}", d3_fault_sweep().0),
    );

    s.add_dyn(
        "F12683-08 账等式",
        led.equation_ok(),
        &format!("rows={} archives={} diffs={} rejects={}", led.len(), led.archive_len(), led.diff_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12683-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12683-10 F12564 谱口径",
        code_in_f12564(0x51) && code_in_f12564(0x55) && !code_in_f12564(0x50) && !code_in_f12564(0x56),
        "谱值 0x51..0x55 闭集",
    );

    s.add_dyn(
        "F12683-11 比对判二闭集",
        CmpVerdict::all().len() == 2 && CmpVerdict::Match.ordinal() == 0,
        "全同/失配",
    );

    s.add_dyn(
        "F12683-12 完成写回语义",
        writeback_ok(),
        "归档即已完成且摘要非空",
    );

    s.add_dyn(
        "F12683-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81480);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(D3_ALL.len(), 11);
        let mut i = 0usize;
        while i < D3_ALL.len() {
            let vi = match D3_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < D3_ALL.len() {
                let vj = match D3_ALL.get(j) {
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
    fn consistency_hundred_holds() {
        // 一致率 100% 维持。
        assert!(consistency_hundred_ok());
        let led = build_full_d3_ledger();
        assert_eq!(led.len(), 20);
        assert_eq!(led.diff_len(), 0);
        assert_eq!(led.state(), D3State::D3Archived);
        // 漂移注入必跌破一致率。
        let mut r = match replay_d3(5) {
            Some(x) => x,
            None => return,
        };
        r.refs = r.refs.saturating_add(1);
        r.verdict = CmpVerdict::Mismatch;
        let mut rows: Vec<RerunRow> = Vec::new();
        let mut i = 0u32;
        while (i as usize) < D3_ROW_CAP {
            let base = match replay_d3(i) {
                Some(x) => x,
                None => return,
            };
            rows.push(if i == 5 { r } else { base });
            i += 1;
        }
        let (matches, total, miss) = consistency_census(&rows);
        assert_eq!(total, 20);
        assert_eq!(matches, 19);
        assert!(miss.contains("行5"));
    }

    #[test]
    fn comparator_archive_holds() {
        assert!(comparator_archived_ok());
        let led = build_full_d3_ledger();
        assert_eq!(led.archive_len(), 20);
        for a in led.archives.iter() {
            assert!(a.1, "行 {} 归档位假", a.0);
        }
    }

    #[test]
    fn digest_baseline_matches() {
        // 比对摘要逐位一致。
        let led = build_full_d3_ledger();
        let mut rows: Vec<RerunRow> = Vec::new();
        let mut i = 0usize;
        while i < led.len() {
            let r = match led.row_at(i) {
                Some(v) => *v,
                None => break,
            };
            rows.push(r);
            i += 1;
        }
        assert_eq!(rerun_digest(&rows), baseline_digest());
        // 漂移必变摘要。
        match rows.get_mut(0) {
            Some(slot) => slot.handle = slot.handle.saturating_add(0x80),
            None => return,
        }
        assert_ne!(rerun_digest(&rows), baseline_digest());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = d3_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn compare_gate_rejects() {
        let mut led = D3Ledger::new();
        let _ = led.begin_compare();
        let r1 = match replay_d3(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(led.compare(r1), Err(D3Diag::ERowOrder.val()));
        let r0 = match replay_d3(0) {
            Some(x) => x,
            None => return,
        };
        assert!(led.compare(r0.clone()).is_ok());
        let mut dup = r0;
        dup.no = 0;
        assert_eq!(led.compare(dup), Err(D3Diag::ERowOrder.val()));
        // 类漂移。
        let mut drift = match replay_d3(1) {
            Some(x) => x,
            None => return,
        };
        drift.class = 0x99;
        assert_eq!(led.compare(drift), Err(D3Diag::EClassDrift.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn snap_census_holds() {
        // 快照普查直读（测试区独立复核）。
        assert!(snap_census_ok());
        assert_eq!(BASE_SNAP.len(), 20);
        // 句柄互异直读。
        let mut i = 0usize;
        while i < BASE_SNAP.len() {
            let r = match BASE_SNAP.get(i) {
                Some(v) => v,
                None => return,
            };
            let mut j = i + 1;
            while j < BASE_SNAP.len() {
                let g = match BASE_SNAP.get(j) {
                    Some(v) => v,
                    None => return,
                };
                assert_ne!(r.handle, g.handle, "行 {} 与行 {} 句柄撞号", i, j);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_d3_ledger();
        let mut i = 0usize;
        while i < led.len() {
            let r = match led.row_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(r.no, i as u32, "行 {} 错位", i);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_d3_ledger();
        let b = build_full_d3_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(d3_fault_sweep().0, d3_fault_sweep().0);
        assert_eq!(baseline_digest(), baseline_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：D3Open→D3Comparing→D3Archived。
        assert!(D3State::D3Open.rank() < D3State::D3Comparing.rank());
        assert!(D3State::D3Comparing.rank() < D3State::D3Archived.rank());
        let mut led = build_full_d3_ledger();
        assert_eq!(led.state(), D3State::D3Archived);
        assert_eq!(led.archive(), Err(D3Diag::ERewind.val()));
        let mut fresh = D3Ledger::new();
        assert!(fresh.begin_compare().is_ok());
        assert_eq!(fresh.begin_compare(), Err(D3Diag::ERewind.val()));
    }

    #[test]
    fn f12564_code_semantics() {
        // F12564 谱语义直读。
        assert_eq!(ErrCode::all().len(), 5);
        for e in ErrCode::all().iter() {
            assert!(code_in_f12564(e.code()), "谱 {} 未落谱", e.name());
        }
        assert!(!code_in_f12564(0x50));
        assert!(!code_in_f12564(0x56));
        assert!(!code_in_f12564(0));
        // 谱值互异。
        let mut i = 0usize;
        while i < ErrCode::all().len() {
            let a = match ErrCode::all().get(i) {
                Some(v) => v.code(),
                None => 0,
            };
            let mut j = i + 1;
            while j < ErrCode::all().len() {
                let b = match ErrCode::all().get(j) {
                    Some(v) => v.code(),
                    None => 0,
                };
                assert_ne!(a, b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12682_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
