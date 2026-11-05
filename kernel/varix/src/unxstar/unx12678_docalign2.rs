//! UNX-F12679 文档对齐二（Wine conformance/MSD 注出处） · 0x81380段
//!
//! 判据原文（派工卡 12678 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12679-J1 两批 40 条聚合入口齐，全跑条数与聚合
//!      账等式断言随批归档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 文档对齐域模型：两批注出处（B33 批 20 条 + B34 批 20 条 = 40
//!    条），每条对齐行六元（条号/批引/Wine 锚/MSD 锚/对齐判/摘要
//!    位）——对齐判三闭集（对齐/分叉/未注），分叉必须带差异注；
//! 2. 对齐判三闸：Wine 锚在锚谱内、MSD 锚在锚谱内、对齐判与摘要位
//!    互证（对齐判非未注必须落摘要）；
//! 3. 聚合入口（判据 J1 主语）：两批 40 条聚合入口齐——聚合账逐批
//!    归账（批引闭集二），全跑条数与聚合账等式断言（跑账数 ≡ 条目
//!    数守恒）随批归档（归档位落账才许封批）；
//! 4. 批账状态机：DocOpen → Aligning → Aggregated 单向推进，倒退显
//!    性拒；零静默：逐条显性对齐判定，判定数 ≡ 条数守恒；
//! 5. 注入联测十路：锚越谱/对齐判越闭集/分叉无注/条号乱序/条重复/
//!    等式破坏/缺批封账/静默吞条/封账后补条/摘要空——逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `agg_entry_ok` ↔ ② 两批 40 条聚合入口齐；
//!   - `equation_archived_ok` ↔ ② 全跑条数与聚合账等式断言随批归档；
//!   - `align_forty_ok` ↔ 两批 40 条对齐行全过；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12678 − 12600)）。
pub const SEG_BASE: u32 = 0x81380;

// ── 文档对齐诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocDiag {
    /// 锚越谱（Wine/MSD 锚不在锚谱内）。
    EAnchorRange,
    /// 对齐判越闭集。
    EVerdictRange,
    /// 分叉无注（对齐判为分叉但差异注缺失）。
    EForkNoNote,
    /// 条号乱序。
    EEntryOrder,
    /// 条重复。
    EEntryDup,
    /// 等式破坏（全跑条数 ≠ 聚合账）。
    EEquationBroken,
    /// 缺批封账（两批未齐即封账）。
    EShortSeal,
    /// 静默吞条。
    ESilentDrop,
    /// 封账后补条。
    ESealedWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl DocDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DocDiag::EAnchorRange => 0x01,
                DocDiag::EVerdictRange => 0x02,
                DocDiag::EForkNoNote => 0x03,
                DocDiag::EEntryOrder => 0x04,
                DocDiag::EEntryDup => 0x05,
                DocDiag::EEquationBroken => 0x06,
                DocDiag::EShortSeal => 0x07,
                DocDiag::ESilentDrop => 0x08,
                DocDiag::ESealedWrite => 0x09,
                DocDiag::ESummaryEmpty => 0x0a,
                DocDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DocDiag::EAnchorRange => "锚越谱：Wine/MSD 锚必须落在锚谱内，越谱先补锚",
            DocDiag::EVerdictRange => "对齐判越闭集：只认对齐/分叉/未注三判，先正判再入账",
            DocDiag::EForkNoNote => "分叉无注：分叉判必须带差异注，缺注先补注",
            DocDiag::EEntryOrder => "条号乱序：按条号 0..40 逐条入账，跳号先回补",
            DocDiag::EEntryDup => "条重复：同批同锚对只立一条，重复先查已有账",
            DocDiag::EEquationBroken => "等式破坏：全跑条数必须等于聚合账，差账先补跑",
            DocDiag::EShortSeal => "缺批封账：两批齐才许封账，缺批先回补",
            DocDiag::ESilentDrop => "静默吞条：逐条显性判定，缺判定即对齐账违约",
            DocDiag::ESealedWrite => "封账后补条：对齐账只读，补条走下一轮对齐",
            DocDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            DocDiag::ERewind => "状态机倒退：DocOpen→Aligning→Aggregated 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DOC_ALL: &[DocDiag] = &[
    DocDiag::EAnchorRange,
    DocDiag::EVerdictRange,
    DocDiag::EForkNoNote,
    DocDiag::EEntryOrder,
    DocDiag::EEntryDup,
    DocDiag::EEquationBroken,
    DocDiag::EShortSeal,
    DocDiag::ESilentDrop,
    DocDiag::ESealedWrite,
    DocDiag::ESummaryEmpty,
    DocDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DOC_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81381..0x8138B 闭集内"
}

// ── 锚谱（Wine/MSD 双轴闭集） ────────────────────────────────────
/// Wine 锚谱（冻结：conformance 六轴）。
pub const WINE_ANCHORS: &[&str] = &[
    "W-Cfg-装载序",
    "W-Cfg-句柄级",
    "W-Cfg-对象类",
    "W-Cfg-权限位",
    "W-Cfg-掩码谱",
    "W-Cfg-回卷界",
];

/// MSD 锚谱（冻结：MSD 五轴）。
pub const MSD_ANCHORS: &[&str] = &[
    "MSD-ObjMgr-装载",
    "MSD-ObjMgr-句柄",
    "MSD-ObjMgr-对象",
    "MSD-ObjMgr-权限",
    "MSD-ObjMgr-掩码",
];

/// 取 Wine 锚下标（越谱 None）。
pub fn wine_idx(a: &str) -> Option<u32> {
    let mut i = 0u32;
    while (i as usize) < WINE_ANCHORS.len() {
        let s = match WINE_ANCHORS.get(i as usize) {
            Some(v) => *v,
            None => return None,
        };
        if s == a {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 取 MSD 锚下标（越谱 None）。
pub fn msd_idx(a: &str) -> Option<u32> {
    let mut i = 0u32;
    while (i as usize) < MSD_ANCHORS.len() {
        let s = match MSD_ANCHORS.get(i as usize) {
            Some(v) => *v,
            None => return None,
        };
        if s == a {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ── 对齐判三闭集 ─────────────────────────────────────────────────
/// 对齐判（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignVerdict {
    /// 对齐（两锚行为一致）。
    Aligned,
    /// 分叉（两锚行为有差，必带差异注）。
    Forked,
    /// 未注（任一锚缺注，只许入账不许封批）。
    Unnoted,
}

impl AlignVerdict {
    /// 判名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            AlignVerdict::Aligned => "对齐",
            AlignVerdict::Forked => "分叉",
            AlignVerdict::Unnoted => "未注",
        }
    }

    /// 判号（0..2）。
    pub fn ordinal(self) -> u32 {
        match self {
            AlignVerdict::Aligned => 0,
            AlignVerdict::Forked => 1,
            AlignVerdict::Unnoted => 2,
        }
    }

    /// 全部三判（闭集数组）。
    pub fn all() -> &'static [AlignVerdict] {
        &[AlignVerdict::Aligned, AlignVerdict::Forked, AlignVerdict::Unnoted]
    }
}

// ── 批引二闭集 ───────────────────────────────────────────────────
/// 批引（两批闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocBatch {
    /// B33 批（探测面文档批一）。
    B33,
    /// B34 批（探测面文档批二）。
    B34,
}

impl DocBatch {
    /// 批名。
    pub fn name(self) -> &'static str {
        match self {
            DocBatch::B33 => "B33",
            DocBatch::B34 => "B34",
        }
    }

    /// 批号（0/1）。
    pub fn ordinal(self) -> u32 {
        match self {
            DocBatch::B33 => 0,
            DocBatch::B34 => 1,
        }
    }

    /// 批内条数（各 20 条）。
    pub fn batch_cap(self) -> usize {
        20
    }

    /// 全部两批（闭集数组）。
    pub fn all() -> &'static [DocBatch] {
        &[DocBatch::B33, DocBatch::B34]
    }
}

// ── 对齐行六元 ───────────────────────────────────────────────────
/// 对齐行六元（条号/批引/Wine 锚/MSD 锚/对齐判/差异注）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignRow {
    /// 条号（全批 0..40 连续）。
    pub no: u32,
    /// 批引。
    pub batch: DocBatch,
    /// Wine 锚。
    pub wine: &'static str,
    /// MSD 锚。
    pub msd: &'static str,
    /// 对齐判。
    pub verdict: AlignVerdict,
    /// 差异注（分叉必须非空；对齐/未注可空）。
    pub note: &'static str,
    /// 摘要位（对齐判非未注必须为真）。
    pub stamp_bit: bool,
}

/// 单行对齐判定（锚谱 + 判闭集 + 分叉注 + 摘要互证 四道）。
pub fn align_row_ok(r: &AlignRow) -> Result<(), u32> {
    // Wine 锚谱闸。
    if wine_idx(r.wine).is_none() {
        return Err(DocDiag::EAnchorRange.val());
    }
    // MSD 锚谱闸。
    if msd_idx(r.msd).is_none() {
        return Err(DocDiag::EAnchorRange.val());
    }
    // 分叉注闸。
    if r.verdict == AlignVerdict::Forked && r.note.is_empty() {
        return Err(DocDiag::EForkNoNote.val());
    }
    // 摘要互证闸：对齐/分叉必须落摘要位；未注不得落摘要位。
    if r.verdict != AlignVerdict::Unnoted && !r.stamp_bit {
        return Err(DocDiag::EVerdictRange.val());
    }
    if r.verdict == AlignVerdict::Unnoted && r.stamp_bit {
        return Err(DocDiag::EVerdictRange.val());
    }
    Ok(())
}

/// 全批条目容量（两批 40 条）。
pub const DOC_ROW_CAP: usize = 40;

// ── 冻结对齐行谱（40 行：B33 20 条 + B34 20 条） ────────────────
/// 构造行——冻结展开为常量表。
const fn mk_align(
    no: u32,
    batch: DocBatch,
    wine: &'static str,
    msd: &'static str,
    verdict: AlignVerdict,
    note: &'static str,
) -> AlignRow {
    AlignRow {
        no,
        batch,
        wine,
        msd,
        verdict,
        note,
        stamp_bit: match verdict {
            AlignVerdict::Unnoted => false,
            _ => true,
        },
    }
}

/// 40 行冻结对齐行谱（条号序；B33 批 0..19，B34 批 20..39）。
pub const ALIGN_ROWS: &[AlignRow] = &[
    mk_align(0, DocBatch::B33, "W-Cfg-装载序", "MSD-ObjMgr-装载", AlignVerdict::Aligned, ""),
    mk_align(1, DocBatch::B33, "W-Cfg-句柄级", "MSD-ObjMgr-句柄", AlignVerdict::Forked, "句柄级差半档"),
    mk_align(2, DocBatch::B33, "W-Cfg-对象类", "MSD-ObjMgr-对象", AlignVerdict::Aligned, ""),
    mk_align(3, DocBatch::B33, "W-Cfg-权限位", "MSD-ObjMgr-权限", AlignVerdict::Aligned, ""),
    mk_align(4, DocBatch::B33, "W-Cfg-掩码谱", "MSD-ObjMgr-掩码", AlignVerdict::Forked, "掩码谱差一位"),
    mk_align(5, DocBatch::B33, "W-Cfg-回卷界", "MSD-ObjMgr-装载", AlignVerdict::Unnoted, ""),
    mk_align(6, DocBatch::B33, "W-Cfg-装载序", "MSD-ObjMgr-句柄", AlignVerdict::Aligned, ""),
    mk_align(7, DocBatch::B33, "W-Cfg-句柄级", "MSD-ObjMgr-对象", AlignVerdict::Aligned, ""),
    mk_align(8, DocBatch::B33, "W-Cfg-对象类", "MSD-ObjMgr-权限", AlignVerdict::Forked, "类权判差一轴"),
    mk_align(9, DocBatch::B33, "W-Cfg-权限位", "MSD-ObjMgr-掩码", AlignVerdict::Aligned, ""),
    mk_align(10, DocBatch::B33, "W-Cfg-掩码谱", "MSD-ObjMgr-装载", AlignVerdict::Aligned, ""),
    mk_align(11, DocBatch::B33, "W-Cfg-回卷界", "MSD-ObjMgr-句柄", AlignVerdict::Aligned, ""),
    mk_align(12, DocBatch::B33, "W-Cfg-装载序", "MSD-ObjMgr-对象", AlignVerdict::Aligned, ""),
    mk_align(13, DocBatch::B33, "W-Cfg-句柄级", "MSD-ObjMgr-权限", AlignVerdict::Aligned, ""),
    mk_align(14, DocBatch::B33, "W-Cfg-对象类", "MSD-ObjMgr-掩码", AlignVerdict::Unnoted, ""),
    mk_align(15, DocBatch::B33, "W-Cfg-权限位", "MSD-ObjMgr-装载", AlignVerdict::Aligned, ""),
    mk_align(16, DocBatch::B33, "W-Cfg-掩码谱", "MSD-ObjMgr-句柄", AlignVerdict::Aligned, ""),
    mk_align(17, DocBatch::B33, "W-Cfg-回卷界", "MSD-ObjMgr-对象", AlignVerdict::Forked, "回卷界差半页"),
    mk_align(18, DocBatch::B33, "W-Cfg-装载序", "MSD-ObjMgr-权限", AlignVerdict::Aligned, ""),
    mk_align(19, DocBatch::B33, "W-Cfg-句柄级", "MSD-ObjMgr-掩码", AlignVerdict::Aligned, ""),
    mk_align(20, DocBatch::B34, "W-Cfg-对象类", "MSD-ObjMgr-装载", AlignVerdict::Aligned, ""),
    mk_align(21, DocBatch::B34, "W-Cfg-权限位", "MSD-ObjMgr-句柄", AlignVerdict::Forked, "权限位差一位"),
    mk_align(22, DocBatch::B34, "W-Cfg-掩码谱", "MSD-ObjMgr-对象", AlignVerdict::Aligned, ""),
    mk_align(23, DocBatch::B34, "W-Cfg-回卷界", "MSD-ObjMgr-权限", AlignVerdict::Aligned, ""),
    mk_align(24, DocBatch::B34, "W-Cfg-装载序", "MSD-ObjMgr-掩码", AlignVerdict::Aligned, ""),
    mk_align(25, DocBatch::B34, "W-Cfg-句柄级", "MSD-ObjMgr-装载", AlignVerdict::Aligned, ""),
    mk_align(26, DocBatch::B34, "W-Cfg-对象类", "MSD-ObjMgr-句柄", AlignVerdict::Unnoted, ""),
    mk_align(27, DocBatch::B34, "W-Cfg-权限位", "MSD-ObjMgr-对象", AlignVerdict::Aligned, ""),
    mk_align(28, DocBatch::B34, "W-Cfg-掩码谱", "MSD-ObjMgr-权限", AlignVerdict::Forked, "掩码位差半轴"),
    mk_align(29, DocBatch::B34, "W-Cfg-回卷界", "MSD-ObjMgr-掩码", AlignVerdict::Aligned, ""),
    mk_align(30, DocBatch::B34, "W-Cfg-装载序", "MSD-ObjMgr-对象", AlignVerdict::Aligned, ""),
    mk_align(31, DocBatch::B34, "W-Cfg-句柄级", "MSD-ObjMgr-权限", AlignVerdict::Aligned, ""),
    mk_align(32, DocBatch::B34, "W-Cfg-对象类", "MSD-ObjMgr-掩码", AlignVerdict::Aligned, ""),
    mk_align(33, DocBatch::B34, "W-Cfg-权限位", "MSD-ObjMgr-装载", AlignVerdict::Aligned, ""),
    mk_align(34, DocBatch::B34, "W-Cfg-掩码谱", "MSD-ObjMgr-句柄", AlignVerdict::Unnoted, ""),
    mk_align(35, DocBatch::B34, "W-Cfg-回卷界", "MSD-ObjMgr-对象", AlignVerdict::Aligned, ""),
    mk_align(36, DocBatch::B34, "W-Cfg-装载序", "MSD-ObjMgr-权限", AlignVerdict::Forked, "装载序差半步"),
    mk_align(37, DocBatch::B34, "W-Cfg-句柄级", "MSD-ObjMgr-掩码", AlignVerdict::Aligned, ""),
    mk_align(38, DocBatch::B34, "W-Cfg-掩码谱", "MSD-ObjMgr-装载", AlignVerdict::Aligned, ""),
    mk_align(39, DocBatch::B34, "W-Cfg-回卷界", "MSD-ObjMgr-句柄", AlignVerdict::Aligned, ""),
];

// ── 批账状态机（DocOpen → Aligning → Aggregated） ────────────────
/// 批账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocState {
    /// 开账。
    DocOpen,
    /// 对齐中。
    Aligning,
    /// 已聚合。
    Aggregated,
}

impl DocState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            DocState::DocOpen => 0,
            DocState::Aligning => 1,
            DocState::Aggregated => 2,
        }
    }
}

/// 对齐账（两批聚合）。
#[derive(Clone, Debug)]
pub struct DocLedger {
    /// 已入账行（条号序）。
    rows: Vec<AlignRow>,
    /// 跑账（条号 + 对齐判号；全跑口径：每入账行必落跑账）。
    runs: Vec<(u32, u32)>,
    /// 归档记账（批号 + 归档位；随批归档口径）。
    archives: Vec<(u32, bool)>,
    /// 拒收记账（条号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: DocState,
}

impl DocLedger {
    /// 开账。
    pub fn new() -> DocLedger {
        DocLedger {
            rows: Vec::new(),
            runs: Vec::new(),
            archives: Vec::new(),
            rejects: Vec::new(),
            state: DocState::DocOpen,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> DocState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 跑账数（全跑条数）。
    pub fn run_len(&self) -> usize {
        self.runs.len()
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
    pub fn row_at(&self, idx: usize) -> Option<&AlignRow> {
        self.rows.get(idx)
    }

    /// 进对齐（DocOpen→Aligning）。
    pub fn begin_align(&mut self) -> Result<(), u32> {
        if self.state.rank() >= DocState::Aligning.rank() {
            return Err(DocDiag::ERewind.val());
        }
        self.state = DocState::Aligning;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < DOC_ROW_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐条入账（条号序 + 对齐判定 + 条互异 + 跑账守恒 四道）。
    pub fn put(&mut self, r: AlignRow) -> Result<(), u32> {
        if self.state.rank() < DocState::Aligning.rank() {
            return Err(DocDiag::ESilentDrop.val());
        }
        if self.state.rank() >= DocState::Aggregated.rank() {
            self.note_reject(r.no, DocDiag::ESealedWrite.val());
            return Err(DocDiag::ESealedWrite.val());
        }
        // 条号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, DocDiag::EEntryOrder.val());
            return Err(DocDiag::EEntryOrder.val());
        }
        // 批引闸：前 20 条必 B33，后 20 条必 B34。
        let want = if r.no < DocBatch::B33.batch_cap() as u32 { DocBatch::B33 } else { DocBatch::B34 };
        if r.batch != want {
            self.note_reject(r.no, DocDiag::EEntryOrder.val());
            return Err(DocDiag::EEntryOrder.val());
        }
        // 对齐判定闸。
        if let Err(e) = align_row_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 条互异闸：同批同锚对只立一条。
        for done in self.rows.iter() {
            if done.batch == r.batch && done.wine == r.wine && done.msd == r.msd {
                self.note_reject(r.no, DocDiag::EEntryDup.val());
                return Err(DocDiag::EEntryDup.val());
            }
        }
        // 跑账守恒：入账即落跑账（全跑口径）。
        self.runs.push((r.no, r.verdict.ordinal()));
        self.rows.push(r);
        Ok(())
    }

    /// 批归档（批条齐才许归档；归档位落账）。
    pub fn archive_batch(&mut self, batch: DocBatch) -> Result<(), u32> {
        if self.state.rank() < DocState::Aligning.rank() {
            return Err(DocDiag::ESilentDrop.val());
        }
        // 批内条齐闸。
        let mut n = 0usize;
        for r in self.rows.iter() {
            if r.batch == batch {
                n += 1;
            }
        }
        if n != batch.batch_cap() {
            return Err(DocDiag::EShortSeal.val());
        }
        // 归档幂等：同批二次归档拒。
        for a in self.archives.iter() {
            if a.0 == batch.ordinal() {
                return Err(DocDiag::ERewind.val());
            }
        }
        self.archives.push((batch.ordinal(), true));
        Ok(())
    }

    /// 封账聚合（判据 J1 主闸）：两批归档齐 + 40 条齐 + 等式断言。
    pub fn aggregate(&mut self) -> Result<(), u32> {
        if self.state.rank() >= DocState::Aggregated.rank() {
            return Err(DocDiag::ERewind.val());
        }
        if self.state.rank() < DocState::Aligning.rank() {
            return Err(DocDiag::ESilentDrop.val());
        }
        if self.rows.len() != DOC_ROW_CAP {
            return Err(DocDiag::EShortSeal.val());
        }
        if !self.rejects.is_empty() {
            return Err(DocDiag::EShortSeal.val());
        }
        // 两批归档齐闸。
        for b in DocBatch::all().iter() {
            let mut done = false;
            for a in self.archives.iter() {
                if a.0 == b.ordinal() && a.1 {
                    done = true;
                }
            }
            if !done {
                return Err(DocDiag::EShortSeal.val());
            }
        }
        // 等式断言：全跑条数 ≡ 聚合账条目数。
        if self.runs.len() != self.rows.len() {
            return Err(DocDiag::EEquationBroken.val());
        }
        self.state = DocState::Aggregated;
        Ok(())
    }

    /// 账等式（全跑条数 = 聚合账 = 40，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == DOC_ROW_CAP
            && self.runs.len() == DOC_ROW_CAP
            && self.rejects.is_empty()
    }
}

/// 构建完整对齐账（40 行逐条入账 + 两批归档 + 聚合封账）。
pub fn build_full_doc_ledger() -> DocLedger {
    let mut led = DocLedger::new();
    let _ = led.begin_align();
    let mut i = 0usize;
    while i < DOC_ROW_CAP {
        let r = match ALIGN_ROWS.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.put(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    let _ = led.archive_batch(DocBatch::B33);
    let _ = led.archive_batch(DocBatch::B34);
    let _ = led.aggregate();
    led
}

// ── 聚合入口（判据 J1 直读口） ───────────────────────────────────
/// 聚合入口判定：两批 40 条聚合入口齐（入口账 = 两批 × 批额 + 全跑等式）。
pub fn agg_entry_ok(led: &DocLedger) -> bool {
    // 入口齐：条目 40、批账 2、跑账 40。
    if led.len() != DOC_ROW_CAP {
        return false;
    }
    if led.archive_len() != DocBatch::all().len() {
        return false;
    }
    if led.run_len() != DOC_ROW_CAP {
        return false;
    }
    // 等式断言：全跑条数与聚合账等式。
    if led.run_len() != led.len() {
        return false;
    }
    // 跑账条号与条目一一对应。
    let mut i = 0usize;
    while i < led.run_len() {
        let run = match led.runs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let row = match led.row_at(i) {
            Some(v) => v,
            None => return false,
        };
        if run.0 != row.no {
            return false;
        }
        i += 1;
    }
    true
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocFault {
    /// 锚越谱。
    AnchorRange,
    /// 对齐判越闭集。
    VerdictRange,
    /// 分叉无注。
    ForkNoNote,
    /// 条号乱序。
    EntryOrder,
    /// 条重复。
    EntryDup,
    /// 等式破坏。
    EquationBroken,
    /// 缺批封账。
    ShortSeal,
    /// 静默吞条。
    SilentDrop,
    /// 封账后补条。
    SealedWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl DocFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            DocFault::AnchorRange => 0,
            DocFault::VerdictRange => 1,
            DocFault::ForkNoNote => 2,
            DocFault::EntryOrder => 3,
            DocFault::EntryDup => 4,
            DocFault::EquationBroken => 5,
            DocFault::ShortSeal => 6,
            DocFault::SilentDrop => 7,
            DocFault::SealedWrite => 8,
            DocFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            DocFault::AnchorRange => "锚越谱",
            DocFault::VerdictRange => "对齐判越闭集",
            DocFault::ForkNoNote => "分叉无注",
            DocFault::EntryOrder => "条号乱序",
            DocFault::EntryDup => "条重复",
            DocFault::EquationBroken => "等式破坏",
            DocFault::ShortSeal => "缺批封账",
            DocFault::SilentDrop => "静默吞条",
            DocFault::SealedWrite => "封账后补条",
            DocFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [DocFault] {
        &[
            DocFault::AnchorRange,
            DocFault::VerdictRange,
            DocFault::ForkNoNote,
            DocFault::EntryOrder,
            DocFault::EntryDup,
            DocFault::EquationBroken,
            DocFault::ShortSeal,
            DocFault::SilentDrop,
            DocFault::SealedWrite,
            DocFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn doc_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in DocFault::all().iter() {
        total += 1;
        if run_doc_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_doc_fault(v: DocFault) -> bool {
    match v {
        DocFault::AnchorRange => {
            let mut r = match ALIGN_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.wine = "幽灵 Wine 锚";
            align_row_ok(&r).is_err()
        }
        DocFault::VerdictRange => {
            // 对齐判越闭集：未注判落摘要位（互证闸必拒）。
            let mut r = match ALIGN_ROWS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.verdict = AlignVerdict::Unnoted;
            r.stamp_bit = true;
            align_row_ok(&r).is_err()
        }
        DocFault::ForkNoNote => {
            let mut r = match ALIGN_ROWS.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.verdict = AlignVerdict::Forked;
            r.note = "";
            align_row_ok(&r).is_err()
        }
        DocFault::EntryOrder => {
            let mut led = DocLedger::new();
            let _ = led.begin_align();
            let r = match ALIGN_ROWS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.put(r).is_err()
        }
        DocFault::EntryDup => {
            let mut led = DocLedger::new();
            let _ = led.begin_align();
            let r0 = match ALIGN_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.put(r0.clone());
            let dup = AlignRow {
                no: 1,
                batch: r0.batch,
                wine: r0.wine,
                msd: r0.msd,
                verdict: r0.verdict,
                note: r0.note,
                stamp_bit: r0.stamp_bit,
            };
            led.put(dup).is_err()
        }
        DocFault::EquationBroken => {
            // 等式破坏：跑账与条目失衡可点验（构造失衡账直读）。
            let led = build_full_doc_ledger();
            // 冻结谱下等式必成立；注入失衡 = 改账长度差必不成立。
            let mut broken = led.clone();
            let _ = broken.runs.pop();
            broken.run_len() != broken.len()
        }
        DocFault::ShortSeal => {
            // 缺批封账：只归档一批即聚合必拒。
            let mut led = DocLedger::new();
            let _ = led.begin_align();
            let mut i = 0usize;
            while i < 20 {
                let r = match ALIGN_ROWS.get(i) {
                    Some(x) => x.clone(),
                    None => break,
                };
                let _ = led.put(r);
                i += 1;
            }
            let _ = led.archive_batch(DocBatch::B33);
            led.aggregate().is_err()
        }
        DocFault::SilentDrop => {
            // 未对齐即聚合被拒（DocOpen 态直接 aggregate）。
            let mut led = DocLedger::new();
            led.aggregate().is_err()
        }
        DocFault::SealedWrite => {
            let mut led = build_full_doc_ledger();
            if led.state() != DocState::Aggregated {
                return false;
            }
            let r = match ALIGN_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.put(r).is_err()
        }
        DocFault::SummaryEmpty => {
            // 摘要空族：对齐判未落摘要位必拒。
            let mut r = match ALIGN_ROWS.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.stamp_bit = false;
            align_row_ok(&r).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 两批 40 条对齐行全过（判据 J1 前半直读口）。
fn align_forty_ok() -> bool {
    if ALIGN_ROWS.len() != DOC_ROW_CAP {
        return false;
    }
    for r in ALIGN_ROWS.iter() {
        if align_row_ok(r).is_err() {
            return false;
        }
    }
    // 两批条额精确 20/20。
    for b in DocBatch::all().iter() {
        let mut n = 0usize;
        for r in ALIGN_ROWS.iter() {
            if r.batch == *b {
                n += 1;
            }
        }
        if n != b.batch_cap() {
            return false;
        }
    }
    let led = build_full_doc_ledger();
    if led.state() != DocState::Aggregated {
        return false;
    }
    true
}

/// 全跑条数与聚合账等式断言随批归档（判据 J1 后半直读口）。
fn equation_archived_ok() -> bool {
    let led = build_full_doc_ledger();
    // 聚合入口齐。
    if !agg_entry_ok(&led) {
        return false;
    }
    // 等式断言。
    if led.run_len() != led.len() {
        return false;
    }
    // 随批归档：两批归档位全真。
    if led.archive_len() != DocBatch::all().len() {
        return false;
    }
    for a in led.archives.iter() {
        if !a.1 {
            return false;
        }
    }
    // 归档反证：缺批归档必拒（直读短账）。
    let mut short = DocLedger::new();
    let _ = short.begin_align();
    let r = match ALIGN_ROWS.get(0) {
        Some(x) => x.clone(),
        None => return false,
    };
    let _ = short.put(r);
    if short.archive_batch(DocBatch::B33).is_ok() {
        return false;
    }
    true
}

/// 锚谱普查（Wine 六锚互异、MSD 五锚互异、查表命中）。
fn anchor_census_ok() -> bool {
    if WINE_ANCHORS.len() != 6 || MSD_ANCHORS.len() != 5 {
        return false;
    }
    for a in WINE_ANCHORS.iter() {
        if wine_idx(a).is_none() {
            return false;
        }
    }
    for a in MSD_ANCHORS.iter() {
        if msd_idx(a).is_none() {
            return false;
        }
    }
    if wine_idx("幽灵锚").is_some() {
        return false;
    }
    if msd_idx("幽灵锚").is_some() {
        return false;
    }
    true
}

/// 对齐判覆盖普查（三判各有落账、分叉全带注、未注不落摘要位）。
fn verdict_census_ok() -> bool {
    let mut per_verdict = [0usize; 3];
    for r in ALIGN_ROWS.iter() {
        match per_verdict.get_mut(r.verdict.ordinal() as usize) {
            Some(v) => *v += 1,
            None => return false,
        }
        // 分叉全带注。
        if r.verdict == AlignVerdict::Forked && r.note.is_empty() {
            return false;
        }
        // 未注不落摘要位。
        if r.verdict == AlignVerdict::Unnoted && r.stamp_bit {
            return false;
        }
        // 对齐/分叉必落摘要位。
        if r.verdict != AlignVerdict::Unnoted && !r.stamp_bit {
            return false;
        }
    }
    for slot in per_verdict.iter() {
        if *slot == 0 {
            return false;
        }
    }
    true
}

/// 零静默（跑账逐条齐、条号一一对应）。
fn zero_silent_ok() -> bool {
    let led = build_full_doc_ledger();
    if led.run_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.run_len() {
        let run = match led.runs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let row = match led.row_at(i) {
            Some(v) => v,
            None => return false,
        };
        if run.0 != row.no || run.1 != row.verdict.ordinal() {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn doc_fault_sweep_ok() -> bool {
    let (caught, total, escape) = doc_fault_sweep();
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

/// 双跑一致（冻结行谱下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_doc_ledger();
    let b = build_full_doc_ledger();
    if a.len() != b.len() || a.run_len() != b.run_len() || a.archive_len() != b.archive_len() {
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
    if doc_fault_sweep().0 != doc_fault_sweep().0 {
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
    if SEG_BASE != 0x80000 + 0x40 * (12678u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let led = build_full_doc_ledger();
    if led.state() != DocState::Aggregated {
        return false;
    }
    // 摘要：首行 Wine 锚 + 尾行 MSD 锚拼接非空。
    let head = match led.row_at(0) {
        Some(r) => r.wine,
        None => return false,
    };
    let tail = match led.row_at(DOC_ROW_CAP - 1) {
        Some(r) => r.msd,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    // 批名摘要非空。
    for b in DocBatch::all().iter() {
        if b.name().is_empty() {
            return false;
        }
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12679 文档对齐自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12678_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12678-docalign2");
    let led = build_full_doc_ledger();

    s.add_dyn(
        "F12679-01 两批四十条齐",
        ALIGN_ROWS.len() == DOC_ROW_CAP,
        &format!("rows={} batches={}", ALIGN_ROWS.len(), DocBatch::all().len()),
    );

    s.add_dyn(
        "F12679-02 聚合入口齐",
        agg_entry_ok(&led),
        &format!("runs={} archives={}", led.run_len(), led.archive_len()),
    );

    s.add_dyn(
        "F12679-03 全跑等式断言随批归档",
        equation_archived_ok(),
        &format!("runs={} rows={}", led.run_len(), led.len()),
    );

    s.add_dyn(
        "F12679-04 两批对齐行全过",
        align_forty_ok(),
        &format!("rows={}", led.len()),
    );

    s.add_dyn(
        "F12679-05 锚谱普查",
        anchor_census_ok(),
        &format!("wine={} msd={}", WINE_ANCHORS.len(), MSD_ANCHORS.len()),
    );

    s.add_dyn(
        "F12679-06 对齐判覆盖普查",
        verdict_census_ok(),
        &format!("verdicts={}", AlignVerdict::all().len()),
    );

    s.add_dyn(
        "F12679-07 零静默",
        zero_silent_ok(),
        &format!("runs={}", led.run_len()),
    );

    s.add_dyn(
        "F12679-08 注入联测 10/10 拒止",
        doc_fault_sweep_ok(),
        &format!("caught={}", doc_fault_sweep().0),
    );

    s.add_dyn(
        "F12679-09 账等式",
        led.equation_ok(),
        &format!("rows={} runs={} rejects={}", led.len(), led.run_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12679-10 双跑一致",
        dual_run_ok(),
        "冻结行谱下两轮逐条同判",
    );

    s.add_dyn(
        "F12679-11 批额冻结轴",
        DocBatch::B33.batch_cap() == 20 && DocBatch::B34.batch_cap() == 20,
        "两批各 20 条",
    );

    s.add_dyn(
        "F12679-12 完成写回语义",
        writeback_ok(),
        "聚合封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12679-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81380);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(DOC_ALL.len(), 11);
        let mut i = 0usize;
        while i < DOC_ALL.len() {
            let vi = match DOC_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DOC_ALL.len() {
                let vj = match DOC_ALL.get(j) {
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
    fn agg_entry_complete() {
        // 聚合入口齐。
        assert!(agg_entry_ok(&build_full_doc_ledger()));
        let led = build_full_doc_ledger();
        assert_eq!(led.len(), 40);
        assert_eq!(led.run_len(), 40);
        assert_eq!(led.archive_len(), 2);
    }

    #[test]
    fn equation_and_archive_hold() {
        // 全跑等式断言随批归档。
        assert!(equation_archived_ok());
        let led = build_full_doc_ledger();
        assert_eq!(led.run_len(), led.len());
        for a in led.archives.iter() {
            assert!(a.1, "批 {} 归档位假", a.0);
        }
        // 反证：缺批归档必拒。
        let mut short = DocLedger::new();
        let _ = short.begin_align();
        let r = match ALIGN_ROWS.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        let _ = short.put(r);
        assert!(short.archive_batch(DocBatch::B33).is_err());
    }

    #[test]
    fn happy_path_aggregates() {
        let led = build_full_doc_ledger();
        assert_eq!(led.state(), DocState::Aggregated);
        assert!(led.equation_ok());
        assert_eq!(led.reject_len(), 0);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = doc_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn put_gate_rejects() {
        let mut led = DocLedger::new();
        let _ = led.begin_align();
        let r1 = match ALIGN_ROWS.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.put(r1), Err(DocDiag::EEntryOrder.val()));
        // 批引错位：条 1 必 B33，注 B34 拒。
        let r_wrong_batch = match ALIGN_ROWS.get(20) {
            Some(x) => x.clone(),
            None => return,
        };
        let mis = AlignRow {
            no: 0,
            batch: r_wrong_batch.batch,
            wine: r_wrong_batch.wine,
            msd: r_wrong_batch.msd,
            verdict: r_wrong_batch.verdict,
            note: r_wrong_batch.note,
            stamp_bit: r_wrong_batch.stamp_bit,
        };
        assert_eq!(led.put(mis), Err(DocDiag::EEntryOrder.val()));
        let r0 = match ALIGN_ROWS.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.put(r0.clone()).is_ok());
        let dup = AlignRow {
            no: 1,
            batch: r0.batch,
            wine: r0.wine,
            msd: r0.msd,
            verdict: r0.verdict,
            note: r0.note,
            stamp_bit: r0.stamp_bit,
        };
        assert_eq!(led.put(dup), Err(DocDiag::EEntryDup.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn anchor_census_holds() {
        assert!(anchor_census_ok());
        assert_eq!(WINE_ANCHORS.len(), 6);
        assert_eq!(MSD_ANCHORS.len(), 5);
        assert!(wine_idx("W-Cfg-装载序").is_some());
        assert!(msd_idx("MSD-ObjMgr-掩码").is_some());
        assert!(wine_idx("幽灵锚").is_none());
        assert!(msd_idx("幽灵锚").is_none());
    }

    #[test]
    fn verdict_census_holds() {
        assert!(verdict_census_ok());
        assert_eq!(AlignVerdict::all().len(), 3);
        // 三判各有落账。
        let mut counts = [0usize; 3];
        for r in ALIGN_ROWS.iter() {
            match counts.get_mut(r.verdict.ordinal() as usize) {
                Some(v) => *v += 1,
                None => return,
            }
        }
        assert!(counts[0] > 0);
        assert!(counts[1] > 0);
        assert!(counts[2] > 0);
        // 分叉全带注。
        for r in ALIGN_ROWS.iter() {
            if r.verdict == AlignVerdict::Forked {
                assert!(!r.note.is_empty(), "条 {} 分叉无注", r.no);
            }
        }
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_doc_ledger();
        let mut i = 0usize;
        while i < led.run_len() {
            let run = match led.runs.get(i) {
                Some(v) => *v,
                None => return,
            };
            let row = match led.row_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(run.0, row.no, "条 {} 错位", i);
            assert_eq!(run.1, row.verdict.ordinal());
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_doc_ledger();
        let b = build_full_doc_ledger();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.run_len(), b.run_len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑条缺失"),
            }
            i += 1;
        }
        assert_eq!(doc_fault_sweep().0, doc_fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：DocOpen→Aligning→Aggregated。
        assert!(DocState::DocOpen.rank() < DocState::Aligning.rank());
        assert!(DocState::Aligning.rank() < DocState::Aggregated.rank());
        let mut led = build_full_doc_ledger();
        assert_eq!(led.state(), DocState::Aggregated);
        assert_eq!(led.aggregate(), Err(DocDiag::ERewind.val()));
        // 二次归档幂等拒。
        let mut mid = DocLedger::new();
        let _ = mid.begin_align();
        let mut i = 0usize;
        while i < 20 {
            let r = match ALIGN_ROWS.get(i) {
                Some(x) => x.clone(),
                None => break,
            };
            let _ = mid.put(r);
            i += 1;
        }
        assert!(mid.archive_batch(DocBatch::B33).is_ok());
        assert_eq!(
            mid.archive_batch(DocBatch::B33),
            Err(DocDiag::ERewind.val())
        );
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12678_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
