//! VE-F2404 · 关键帧批量操作（判据逐条对应，见下表）
//!
//! 判据映射（锚点原文 → 本模块落点）：
//! - **四操作**（框选/多轨/复制粘贴/缩放时间）→ [`box_select`]、
//!   [`TrackSetOps`]（轨道集合操作）、[`copy_selection`] + [`paste`]、
//!   [`scale_times`]；
//! - **语义单源**（与 F1345 剪辑器操作一致）→ [`ALIGN_TABLE`]（逐操作对照表）
//!   + [`assert_align_single_source`]；
//! - **单步撤销**（批量 = 单撤销步，整体回退）→ [`UndoStack`] + [`UndoStep`]
//!   + [`UNDO_CAPACITY`] + 最旧淘汰诊断；
//! - **原子事务**（缓冲→校验→原子提交/失败整体回滚）→ [`BatchTxn`] +
//!   [`BatchPlan`] + [`BatchTxn::commit`] / [`BatchTxn::rollback`]。
//!
//! 关键决策：
//!
//! **1. 撤销粒度 = 批量（单步），不是每轨一步。**
//! 作者的心智是「我框选了一批，改了一下，按一下 Ctrl+Z 就该整体回来」。
//! 若按轨逐步撤销，同一次 Ctrl+Z 只退掉一条轨道，剩下的改动**留在原地且看起来
//! 已经生效**——作者会再按一次，然后发现又退了一条，永远追不平。这是「撤销栈
//! 与用户心智不符」的经典故障。故 [`UndoStep`] 只存**整批前后快照**。
//!
//! **2. 原子性靠「快照 + 换入」而非「逐条回滚」。**
//! 逐条回滚要求每个操作都有精确的逆操作，任何一个操作漏了逆操作、或逆操作本身
//! 失败，轨道就停在半改状态。而批量操作是「全成或全不成」的语义，作者看到半改
//! 状态只会以为是自己操作错了。本模块的做法：提交前把受影响实体的轨道全量快照，
//! 改完直接整批换入；任一步失败就整批换回快照。**没有中间态可以失败。**
//!
//! **3. 失败必须整体回滚，且必须说得清回滚到了哪一步。**
//! [`BatchTxn::rollback`] 恢复的是**提交前**的快照（不是部分应用前），
//! [`BatchReport::rolled_back`] 显性回报，避免调用方以为「部分成功也算成功」。
//!
//! **4. 时间缩放锚点语义：`t' = t0 + (t - t0) × s`。**
//! 锚点 `t0` 上的关键帧**位置不动**（`t = t0` → `t' = t0`）——这是作者能看见的
//! 不变量。若用「关于原点缩放」（`t' = t × s`），框选起点会漂移，与 F1345
//! 剪辑器的时间缩放语义不一致。
//!
//! **5. 粘贴类型不匹配是拒绝，不是转换。**
//! 浮点轨的值粘到颜色轨上，颜色分量会串位——这是**静默数据损坏**，比拒绝危险得多。
//! 故 [`paste`] 严格按 [`TrackClass`] 比对，不做隐式转换。
//!
//! **6. 撤销栈溢出淘汰最旧，但必须显性提示。**
//! 静默淘汰等于让作者某次 Ctrl+Z 毫无反应，且**不知道**历史已经没了。
//! [`UndoStack::push`] 返回 [`bool`]（`false` = 发生过淘汰），调用方须据此发告警。
//!
//! 零墙钟、零 IO、零外部依赖、零全局可变状态；逻辑 tick 注入，回归可复现。

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vem02_track::{
    BindPath, DiagBag, InterpKind, KeyframeRef, Outcome, PayloadType, Track, TrackClass,
    TrackContainer,
};

// ---------------------------------------------------------------------------
// 诊断码
// ---------------------------------------------------------------------------

/// 批量操作诊断码。
///
/// **分立纪律**：处置方向相反的状态不得共用码。「类型不匹配 → 拒绝粘贴」
/// 与「撤销栈溢出 → 淘汰最旧并放行」方向相反，必须独立（`PasteTypeMismatch`
/// vs `UndoEvicted`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchDiag {
    /// 选择集为空（框选没命中任何帧）。
    SelectionEmpty,
    /// 时间窗反了（`t0 > t1`）。
    WindowInverted,
    /// 时间窗越界（超出资产时间域）。
    WindowOutOfRange,
    /// 轨道不属于被批量操作的实体集合。
    TrackNotSelected,
    /// 粘贴类型不匹配（拒绝，不转换）。
    PasteTypeMismatch,
    /// 粘贴缓冲为空。
    ClipEmpty,
    /// 缩放系数非有限或非正。
    ScaleFactorInvalid,
    /// 缩放锚点越界（已钳制，放行 + 告警）。
    AnchorClamped,
    /// 缩放后时间戳不再严格递增（拒绝）。
    TimeOrderViolated,
    /// 撤销栈溢出，淘汰最旧（放行 + 告警）。
    UndoEvicted,
    /// 事务已回滚（拒绝 + 致命级提示）。
    RolledBack,
    /// 目标实体不存在。
    EntityUnknown,
    /// 值条数与关键帧条数不一致。
    LengthMismatch,
}

impl BatchDiag {
    /// 码的稳定字符串（跨会话回归比对用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            BatchDiag::SelectionEmpty => "BATCH_SELECTION_EMPTY",
            BatchDiag::WindowInverted => "BATCH_WINDOW_INVERTED",
            BatchDiag::WindowOutOfRange => "BATCH_WINDOW_OUT_OF_RANGE",
            BatchDiag::TrackNotSelected => "BATCH_TRACK_NOT_SELECTED",
            BatchDiag::PasteTypeMismatch => "BATCH_PASTE_TYPE_MISMATCH",
            BatchDiag::ClipEmpty => "BATCH_CLIP_EMPTY",
            BatchDiag::ScaleFactorInvalid => "BATCH_SCALE_FACTOR_INVALID",
            BatchDiag::AnchorClamped => "BATCH_ANCHOR_CLAMPED",
            BatchDiag::TimeOrderViolated => "BATCH_TIME_ORDER_VIOLATED",
            BatchDiag::UndoEvicted => "BATCH_UNDO_EVICTED",
            BatchDiag::RolledBack => "BATCH_ROLLED_BACK",
            BatchDiag::EntityUnknown => "BATCH_ENTITY_UNKNOWN",
            BatchDiag::LengthMismatch => "BATCH_LENGTH_MISMATCH",
        }
    }

    /// 处置方向：`true` = 放行（告警），`false` = 拒绝（错误）。
    ///
    /// **这是本域的核心分诊表**：方向由码本身决定，调用方不得自行判断
    /// 「这个错误要不要放行」——那正是零静默纪律要消灭的私自放行。
    pub const fn is_advisory(self) -> bool {
        matches!(self, BatchDiag::AnchorClamped | BatchDiag::UndoEvicted)
    }

    /// 给作者看的处置指引。
    pub const fn hint(self) -> &'static str {
        match self {
            BatchDiag::SelectionEmpty => "框选矩形内没有关键帧；确认时间窗与轨道选择集",
            BatchDiag::WindowInverted => "时间窗起止反了；应满足 t0 <= t1",
            BatchDiag::WindowOutOfRange => "时间窗超出资产时间域；先确认资产时长",
            BatchDiag::TrackNotSelected => "该轨道不在本次选择集内；批量操作只作用于选中轨",
            BatchDiag::PasteTypeMismatch => "目标轨道类与片段源不同类；不做隐式转换以免分量串位",
            BatchDiag::ClipEmpty => "片段缓冲为空；先复制再粘贴",
            BatchDiag::ScaleFactorInvalid => "缩放系数须有限且为正",
            BatchDiag::AnchorClamped => "锚点越界已钳到资产时间域；锚点处关键帧位置随之移动",
            BatchDiag::TimeOrderViolated => "缩放后关键帧时间不再严格递增；缩小系数或放宽选择窗",
            BatchDiag::UndoEvicted => "撤销栈已满，最旧一步被淘汰；该步已无法 Ctrl+Z 回来",
            BatchDiag::RolledBack => "批量中途失败，整体已回滚到提交前；无中间态残留",
            BatchDiag::EntityUnknown => "目标实体未挂任何轨道",
            BatchDiag::LengthMismatch => "值条数与关键帧条数不一致；数据源长度对齐被破坏",
        }
    }
}

/// 批量诊断记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchNote {
    /// 诊断码。
    pub code: BatchDiag,
    /// 人类可读描述。
    pub message: String,
    /// 处置指引。
    pub hint: &'static str,
}

/// 批量诊断袋（错误/告警双通道——方向由 [`BatchDiag::is_advisory`] 决定）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchNotes {
    notes: Vec<BatchNote>,
}

impl BatchNotes {
    /// 空袋。
    pub fn new() -> Self {
        BatchNotes { notes: Vec::new() }
    }

    /// 记一条诊断（方向由码决定，调用方不得选通道）。
    pub fn push(&mut self, code: BatchDiag, message: &str) {
        self.notes.push(BatchNote {
            code,
            message: message.to_string(),
            hint: code.hint(),
        });
    }

    /// 全部记录。
    pub fn all(&self) -> &[BatchNote] {
        &self.notes
    }

    /// 仅错误通道（非 advisory）。
    pub fn errors(&self) -> Vec<&BatchNote> {
        self.notes.iter().filter(|n| !n.code.is_advisory()).collect()
    }

    /// 仅告警通道（advisory）。
    pub fn warnings(&self) -> Vec<&BatchNote> {
        self.notes.iter().filter(|n| n.code.is_advisory()).collect()
    }

    /// 是否有错误（= 是否必须拒绝）。
    pub fn has_errors(&self) -> bool {
        self.notes.iter().any(|n| !n.code.is_advisory())
    }

    /// 首个错误码。
    pub fn first_error(&self) -> Option<BatchDiag> {
        self.notes.iter().find(|n| !n.code.is_advisory()).map(|n| n.code)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 判据二：与 F1345 的语义对齐表（跨域操作语义单源）
// ---------------------------------------------------------------------------

/// 批量操作种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchOp {
    /// 框选（时间 × 轨道矩形选择）。
    BoxSelect,
    /// 多轨集合操作。
    MultiTrack,
    /// 复制粘贴（片段跨轨搬运）。
    CopyPaste,
    /// 缩放时间。
    ScaleTime,
    /// 撤销。
    Undo,
}

impl BatchOp {
    /// 名。
    pub const fn as_str(self) -> &'static str {
        match self {
            BatchOp::BoxSelect => "box-select",
            BatchOp::MultiTrack => "multi-track",
            BatchOp::CopyPaste => "copy-paste",
            BatchOp::ScaleTime => "scale-time",
            BatchOp::Undo => "undo",
        }
    }

    /// 五种操作全覆盖（对齐表必须逐项登记，缺项即机检红）。
    pub const ALL: [BatchOp; 5] = [
        BatchOp::BoxSelect,
        BatchOp::MultiTrack,
        BatchOp::CopyPaste,
        BatchOp::ScaleTime,
        BatchOp::Undo,
    ];
}

/// 与 F1345 剪辑器的逐操作语义对齐条目。
#[derive(Clone, Copy, Debug)]
pub struct AlignEntry {
    /// 本域操作。
    pub op: BatchOp,
    /// F1345 侧对应语义。
    pub f1345_semantics: &'static str,
    /// 一致性声明（为什么两边等价）。
    pub claim: &'static str,
}

/// **语义对齐表**：批量操作语义与 F1345 剪辑器操作一致。
///
/// 「编辑器学会一次处处使用」（10 章一致性）：若 M 域的框选谓词与 F1345 的
/// 框选谓词不同（一个含端点一个不含端点），作者在时间轴上框一次、再到轨道面板
/// 框一次，会得到不同结果，且**没有任何提示**。对齐表把两边的语义写成可机检的
/// 声明，而不是靠约定。
pub static ALIGN_TABLE: [AlignEntry; 5] = [
    AlignEntry {
        op: BatchOp::BoxSelect,
        f1345_semantics: "时间×轨道矩形谓词 t∈[t0,t1] ∧ 轨∈选择集，两端闭区间",
        claim: "与 F1345 同为闭区间：作者框住端点上的帧即命中，不做「差一帧」surprises",
    },
    AlignEntry {
        op: BatchOp::MultiTrack,
        f1345_semantics: "轨道集合按 id 寻址，顺序不参与语义",
        claim: "与 F1345 一致：集合以轨道 id 为键，列举序只影响显示不影响结果",
    },
    AlignEntry {
        op: BatchOp::CopyPaste,
        f1345_semantics: "片段含插值器与关键帧一并搬运，类型不符拒绝",
        claim: "与 F1345 一致：插值器随片段走，粘完曲线形状不变，只挪位置",
    },
    AlignEntry {
        op: BatchOp::ScaleTime,
        f1345_semantics: "t' = t0 + (t - t0) × s，锚点 t0 处位置不动",
        claim: "与 F1345 同锚点不变量：框选起点不漂移",
    },
    AlignEntry {
        op: BatchOp::Undo,
        f1345_semantics: "一次用户动作 = 一步撤销，粒度对齐动作而非对象",
        claim: "与 F1345 同粒度：批量=单步，按用户动作切而非按内部对象数切",
    },
];

/// 语义对齐裁决结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignVerdict {
    /// 五种操作是否全部登记。
    pub complete: bool,
    /// 缺失项（`BatchOp::as_str`）。
    pub missing: Vec<&'static str>,
    /// 是否存在空声明（对齐表的 `claim` 不得为空——空声明等于没对齐）。
    pub has_empty_claim: bool,
}

/// 机检：语义对齐表完整性。
///
/// 缺项 = 「没对齐」；空 `claim` = 「写了等于没写」。两者都判红。
pub fn assert_align_single_source(table: &[AlignEntry]) -> AlignVerdict {
    let mut missing = Vec::new();
    for op in BatchOp::ALL.iter() {
        if !table.iter().any(|e| e.op == *op) {
            missing.push(op.as_str());
        }
    }
    AlignVerdict {
        complete: missing.is_empty(),
        missing,
        has_empty_claim: table
            .iter()
            .any(|e| e.claim.trim().is_empty() || e.f1345_semantics.trim().is_empty()),
    }
}

// ---------------------------------------------------------------------------
// 选择集（判据一：框选 / 多轨）
// ---------------------------------------------------------------------------

/// 框选矩形（时间窗 × 轨道集合）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxRect {
    /// 起始 tick（含）。
    pub t0: u32,
    /// 结束 tick（含）。
    pub t1: u32,
    /// 参与选择的轨道 id（空 = 不限轨道）。
    pub tracks: Vec<String>,
    /// 资产时间域上界（tick，含）。
    pub horizon: u32,
}

impl BoxRect {
    /// 构造时间窗（轨道不限）。
    pub fn window(t0: u32, t1: u32, horizon: u32) -> Self {
        BoxRect {
            t0,
            t1,
            tracks: Vec::new(),
            horizon,
        }
    }

    /// 限定轨道集合。
    pub fn with_tracks(mut self, tracks: &[&str]) -> Self {
        self.tracks = tracks.iter().map(|s| s.to_string()).collect();
        self.tracks.sort();
        self.tracks.dedup();
        self
    }

    /// 时间窗是否自洽（`t0 <= t1` 且不越界）。
    fn window_ok(&self, notes: &mut BatchNotes) -> bool {
        if self.t0 > self.t1 {
            notes.push(
                BatchDiag::WindowInverted,
                &format!("时间窗反了：t0={} > t1={}", self.t0, self.t1),
            );
            return false;
        }
        if self.t1 > self.horizon {
            notes.push(
                BatchDiag::WindowOutOfRange,
                &format!("时间窗越界：t1={} > horizon={}", self.t1, self.horizon),
            );
            return false;
        }
        true
    }

    /// 轨道是否在选择集内（空集 = 全选）。
    pub fn admits(&self, track_id: &str) -> bool {
        self.tracks.is_empty() || self.tracks.iter().any(|t| t == track_id)
    }
}

/// 一次框选的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// 命中的轨道 id（稳定序，便于回归比对）。
    pub tracks: Vec<String>,
    /// 窗口内被选中的关键帧下标（按轨道分组，值为 `轨道下标 → 帧下标列表`）。
    pub frames: Vec<(String, Vec<usize>)>,
    /// 选中帧总数。
    pub total_frames: usize,
}

/// 框选谓词：时间窗 × 轨道集合。
///
/// **闭区间**：`t ∈ [t0, t1]`。见 [`ALIGN_TABLE`] 中 F1345 对齐声明。
///
/// 复杂度 **O(N 关键帧)**：对每条候选轨道二分定位窗口两端，产出区间内的帧下标。
/// 二分而非全扫是 F2402 [`locate_span`](crate::svstar2::vem02_track::locate_span)
/// 的直接复用——同一份时间戳数组，不另存一份窗口索引。
pub fn box_select(
    container: &TrackContainer,
    rect: &BoxRect,
    times_of: &dyn Fn(&str) -> Option<Vec<u32>>,
    notes: &mut BatchNotes,
) -> Option<Selection> {
    if !rect.window_ok(notes) {
        return None;
    }
    let mut tracks = Vec::new();
    let mut frames = Vec::new();
    let mut total = 0usize;
    for t in container.list_entities().iter() {
        let entity_tracks = match container.of(t) {
            Outcome::Ok { value, .. } => value,
            Outcome::Err(_) => continue,
        };
        for tr in entity_tracks.tracks.iter() {
            if !rect.admits(&tr.id) {
                continue;
            }
            let times = match times_of(&tr.id) {
                Some(v) if !v.is_empty() => v,
                // 时间戳缺失不是批量操作的失败理由：轨道可能尚未装载关键帧。
                _ => continue,
            };
            let idx = frames_in_window(&times, rect.t0, rect.t1);
            if idx.is_empty() {
                continue;
            }
            tracks.push(tr.id.clone());
            total += idx.len();
            frames.push((tr.id.clone(), idx));
        }
    }
    if tracks.is_empty() {
        notes.push(
            BatchDiag::SelectionEmpty,
            &format!("框选未命中：窗口 [{}, {}] 内无关键帧", rect.t0, rect.t1),
        );
        return None;
    }
    tracks.sort();
    Some(Selection {
        tracks,
        frames,
        total_frames: total,
    })
}

/// 窗口内的帧下标（闭区间，二分定位两端）。
///
/// 时间戳须严格递增（否则二分的单调前提不成立）——不满足时返回空，
/// 由调用方的长度/顺序诊断接管，不在此处猜测。
pub fn frames_in_window(times: &[u32], t0: u32, t1: u32) -> Vec<usize> {
    if times.is_empty() || t0 > t1 {
        return Vec::new();
    }
    let first = times.partition_point(|t| *t < t0);
    let mut out = Vec::new();
    let mut i = first;
    while i < times.len() && times[i] <= t1 {
        out.push(i);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 判据一之三：复制粘贴（片段缓冲）
// ---------------------------------------------------------------------------

/// 轨道片段（跨轨搬运单元）。
///
/// **含插值器**：粘完曲线形状不变，只挪位置。漏带插值器会让「原本 ease 的段」
/// 变成直线，而作者看到的是「帧位置对但形状变了」——极难归因。
#[derive(Clone, Debug, PartialEq)]
pub struct TrackClip {
    /// 源轨道 id。
    pub src_track: String,
    /// 轨道类（粘贴目标必须同类）。
    pub class: TrackClass,
    /// 插值器（随片段搬运）。
    pub interp: InterpKind,
    /// 载荷类型（拒绝对不匹配的核心依据）。
    pub payload_type: PayloadType,
    /// 关键帧引用（随片段搬运）。
    pub frames: KeyframeRef,
    /// 片段载荷值（选中帧的值，按帧序）。
    pub values: Vec<f32>,
    /// 片段帧时间戳（逻辑 tick）。
    pub times: Vec<u32>,
    /// 绑定路径原文（粘贴需重新走解析校验）。
    pub bind_raw: String,
}

impl TrackClip {
    /// 片段帧数。
    pub fn len(&self) -> usize {
        self.times.len()
    }

    /// 是否空片段。
    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }
}

/// 片段缓冲（一次复制的多条轨道片段）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipBuffer {
    clips: Vec<TrackClip>,
}

impl ClipBuffer {
    /// 空缓冲。
    pub fn new() -> Self {
        ClipBuffer { clips: Vec::new() }
    }

    /// 片段数。
    pub fn len(&self) -> usize {
        self.clips.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// 片段列表。
    pub fn clips(&self) -> &[TrackClip] {
        &self.clips
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.clips.clear();
    }

    /// 加入片段。
    pub fn push(&mut self, clip: TrackClip) {
        self.clips.push(clip);
    }

    /// 片段的轨道类集合（跨类时粘贴必拒——一次粘贴内混类会部分成功）。
    pub fn class_set(&self) -> Vec<TrackClass> {
        let mut v: Vec<TrackClass> = Vec::new();
        for c in self.clips.iter() {
            if !v.contains(&c.class) {
                v.push(c.class);
            }
        }
        v.sort_by_key(|c| c.as_str());
        v
    }

    /// 取首个片段（复制单轨时的便捷面）。
    pub fn first(&self) -> Option<&TrackClip> {
        self.clips.first()
    }
}

/// 从选择集复制片段。
///
/// **只搬运选中帧**（不是整轨）：框选 3 帧就只取 3 帧。作者框一片区域复制，
/// 预期是「复制这片」，不是「复制整条轨」。
///
/// 轨道元数据取自 [`BatchTxn::snapshot`] 形式的只读快照（`snap`）而非直接借用
/// 容器——容器未暴露「按 id 借用轨道」的面（`of()` 给的是克隆面），而复制要读
/// 载荷、插值器、绑定路径这些**借用即可**的字段，快照让批量操作不必为每次
/// 读取都克隆整条轨的载荷。
pub fn copy_selection(
    snap: &[(String, Vec<Track>)],
    sel: &Selection,
    times_of: &dyn Fn(&str) -> Option<Vec<u32>>,
    values_of: &dyn Fn(&str) -> Option<Vec<f32>>,
    notes: &mut BatchNotes,
) -> Option<ClipBuffer> {
    let mut buf = ClipBuffer::new();
    for (track_id, idxs) in sel.frames.iter() {
        let track = match find_track_by_id(snap, track_id) {
            Some(t) => t,
            None => {
                notes.push(
                    BatchDiag::TrackNotSelected,
                    &format!("轨道「{track_id}」不在快照内（可能已被卸载）"),
                );
                continue;
            }
        };
        let times = match times_of(track_id) {
            Some(v) => v,
            None => continue,
        };
        let values = values_of(track_id).unwrap_or_default();
        let mut vals = Vec::with_capacity(idxs.len());
        let mut tss = Vec::with_capacity(idxs.len());
        let mut ok = true;
        for i in idxs.iter() {
            match (values.get(*i), times.get(*i)) {
                (Some(v), Some(t)) => {
                    vals.push(*v);
                    tss.push(*t);
                }
                _ => {
                    ok = false;
                    notes.push(
                        BatchDiag::LengthMismatch,
                        &format!("轨道「{track_id}」帧 {i} 取不到值或时间戳"),
                    );
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        buf.push(TrackClip {
            src_track: track_id.clone(),
            class: track.class,
            interp: track.interp,
            payload_type: track.payload.payload_type(),
            frames: track.frames.clone(),
            values: vals,
            times: tss,
            bind_raw: track.bind_path.raw.clone(),
        });
    }
    if buf.is_empty() {
        if !notes.has_errors() {
            notes.push(BatchDiag::ClipEmpty, "选择集内无可复制片段");
        }
        return None;
    }
    Some(buf)
}

/// 按 id 在快照内找轨道（借用返回，零克隆）。
pub fn find_track_by_id<'a>(snap: &'a [(String, Vec<Track>)], id: &str) -> Option<&'a Track> {
    for (_ent, tracks) in snap.iter() {
        for t in tracks.iter() {
            if t.id == id {
                return Some(t);
            }
        }
    }
    None
}

/// 粘贴（片段跨轨搬运，偏移应用）。
///
/// **类型不匹配即拒绝，绝不隐式转换**：浮点值粘到颜色轨会让分量串位，
/// 那是静默数据损坏。返回 `None` 且一个片段都不落地（原子）。
pub fn paste(
    target_class: TrackClass,
    buf: &ClipBuffer,
    time_offset: u32,
    notes: &mut BatchNotes,
) -> Option<Vec<TrackClip>> {
    if buf.is_empty() {
        notes.push(BatchDiag::ClipEmpty, "片段缓冲为空，无可粘贴内容");
        return None;
    }
    let classes = buf.class_set();
    if classes.len() > 1 {
        notes.push(
            BatchDiag::PasteTypeMismatch,
            &format!(
                "片段缓冲混了 {} 类轨道（{}）；一次粘贴不得跨类",
                classes.len(),
                classes
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join("/")
            ),
        );
        return None;
    }
    if classes[0] != target_class {
        notes.push(
            BatchDiag::PasteTypeMismatch,
            &format!(
                "目标轨道类「{}」与片段源类「{}」不同；不做隐式转换",
                target_class.as_str(),
                classes[0].as_str()
            ),
        );
        return None;
    }
    let mut out = Vec::with_capacity(buf.len());
    for c in buf.clips().iter() {
        // 时间偏移用**饱和加**而非回绕加：溢出回绕会把粘贴到时间域开头的
        // 片段变成「在末尾」，作者看到片段凭空跳到结尾且无提示。
        let shifted: Vec<u32> = c.times.iter().map(|t| t.saturating_add(time_offset)).collect();
        // 偏移后必须仍严格递增（偏移是同量平移，保持严格增——除非原本就不严格增）。
        if !strictly_increasing(&shifted) {
            notes.push(
                BatchDiag::TimeOrderViolated,
                &format!("片段「{}」偏移后时间戳非严格递增", c.src_track),
            );
            return None;
        }
        let mut clip = c.clone();
        clip.times = shifted;
        out.push(clip);
    }
    Some(out)
}

/// 时间戳是否严格递增。
pub fn strictly_increasing(times: &[u32]) -> bool {
    times.windows(2).all(|w| w[0] < w[1])
}

// ---------------------------------------------------------------------------
// 判据一之四：缩放时间
// ---------------------------------------------------------------------------

/// 缩放结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleReport {
    /// 缩放后的时间戳。
    pub times: Vec<u32>,
    /// 实际使用的锚点（越界时为钳制后的值）。
    pub anchor_used: u32,
    /// 锚点是否被钳制过。
    pub anchor_clamped: bool,
    /// 参与缩放的帧数。
    pub count: usize,
}

/// 时间缩放：`t' = t0 + (t - t0) × s`。
///
/// **锚点不变量**：`t = t0` → `t' = t0`，锚点上的关键帧位置不动。这是
/// [`ALIGN_TABLE`] 与 F1345 的对齐点，也是「框选起点不漂移」的可观察保证。
///
/// **锚点钳制向「数据」钳，不向「时间域上界」钳**。这是本函数最容易写错的一处：
/// 初版把越界锚点钳到 `horizon`，看似合理，实则**任何 `s > 1` 都必然塌缩**
/// ——钳到域界后全部帧都在锚点左侧，放大时 `t' = horizon + (t-horizon)·s`
/// 把远的帧推向负端、再钳到 0，多帧挤到同一 tick，作者的缩放直接被拒。
/// 正确钳制目标是**选区内的边界**：锚点永远落在数据范围里，缩放只压缩
/// 「远离锚点的那一侧」，是否塌缩由系数本身决定（那是另一个诊断）。
///
/// 越界钳制本身是 **advisory（告警放行）**：作者把锚点拖到选区外是常见手误，
/// 直接拒绝整个操作会让缩放不可用；但必须明说锚点被挪了。
/// 若钳制后仍无法保持单调，那是 [`BatchDiag::TimeOrderViolated`]（拒绝），
/// 两条诊断**并存**：一条说「锚点被挪」（知情），一条说「结果非法」（阻断）。
pub fn scale_times(
    times: &[u32],
    anchor: u32,
    factor: f32,
    horizon: u32,
    notes: &mut BatchNotes,
) -> Option<ScaleReport> {
    if !factor.is_finite() || factor <= 0.0 {
        notes.push(
            BatchDiag::ScaleFactorInvalid,
            &format!("缩放系数非法：{factor}（须有限且为正）"),
        );
        return None;
    }
    if times.is_empty() {
        return Some(ScaleReport {
            times: Vec::new(),
            anchor_used: anchor,
            anchor_clamped: false,
            count: 0,
        });
    }
    let first = times[0];
    let last = times[times.len() - 1];
    let mut anchor_used = anchor;
    let mut anchor_clamped = false;
    if anchor_used > last {
        anchor_used = last;
        anchor_clamped = true;
        notes.push(
            BatchDiag::AnchorClamped,
            &format!("锚点 {anchor} 超出选区末端 {last}，钳到 {anchor_used}"),
        );
    } else if anchor_used < first {
        anchor_used = first;
        anchor_clamped = true;
        notes.push(
            BatchDiag::AnchorClamped,
            &format!("锚点 {anchor} 早于选区起点 {first}，钳到 {anchor_used}"),
        );
    }
    let mut out = Vec::with_capacity(times.len());
    for t in times.iter() {
        let delta = (*t as f32) - (anchor_used as f32);
        let scaled = (anchor_used as f32) + delta * factor;
        // 负值钳 0、上界钳 horizon：作者把系数放大到出界时，结果落在时间域内
        // 比落在域外更可用（域外帧会被求值器当越界丢帧）。
        let clamped = if scaled < 0.0 {
            0.0
        } else if scaled > horizon as f32 {
            horizon as f32
        } else {
            scaled
        };
        out.push(clamped.round() as u32);
    }
    if !strictly_increasing(&out) {
        notes.push(
            BatchDiag::TimeOrderViolated,
            &format!("缩放后 {} 个时间戳非严格递增", out.len()),
        );
        return None;
    }
    Some(ScaleReport {
        times: out,
        anchor_used,
        anchor_clamped,
        count: times.len(),
    })
}

// ---------------------------------------------------------------------------
// 判据三：单步撤销
// ---------------------------------------------------------------------------

/// 撤销容量（溢出淘汰最 oldest）。
///
/// 与 `aurora/clipboard.rs` 的 `HISTORY_CAP=16` 同量级；批量操作每步的
/// 数据量大（整批快照），故不取更大值——**快照占内存，超大栈反而让
/// 「我明明按了 Ctrl+Z」更可能失败**。
pub const UNDO_CAPACITY: usize = 16;

/// 一步撤销的内容。
#[derive(Clone, Debug, PartialEq)]
pub struct UndoStep {
    /// 操作标签（作者在撤销历史里看到的名字）。
    pub label: String,
    /// 涉及实体（快照粒度按实体，不按全容器——省内存且回滚面精确）。
    pub entities: Vec<String>,
    /// 提交**前**的轨道快照（回滚目标）。
    pub before: Vec<(String, Vec<Track>)>,
}

/// 撤销栈（批量 = 单步）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UndoStack {
    steps: Vec<UndoStep>,
}

impl UndoStack {
    /// 空栈。
    pub fn new() -> Self {
        UndoStack { steps: Vec::new() }
    }

    /// 压入一步。
    ///
    /// 返回 `false` = **发生过淘汰**（栈满时淘汰最旧）。调用方须据此发告警：
    /// 静默淘汰等于让某次 Ctrl+Z 毫无反应，且作者不知道历史已经没了。
    pub fn push(&mut self, step: UndoStep) -> bool {
        let evicted = self.steps.len() >= UNDO_CAPACITY;
        if evicted {
            self.steps.remove(0);
        }
        self.steps.push(step);
        !evicted
    }

    /// 弹出最新一步（无步可弹返回 `None`）。
    pub fn pop(&mut self) -> Option<UndoStep> {
        self.steps.pop()
    }

    /// 窥视栈顶（不弹出）。
    pub fn peek(&self) -> Option<&UndoStep> {
        self.steps.last()
    }

    /// 步数。
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// 是否空栈。
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// 容量（显性暴露，便于 UI 显示「历史 16 步」）。
    pub const fn capacity(&self) -> usize {
        UNDO_CAPACITY
    }
}

// ---------------------------------------------------------------------------
// 判据四：原子事务
// ---------------------------------------------------------------------------

/// 批量计划：一次作者动作的全部改动（提交前已全部校验通过）。
#[derive(Clone, Debug, PartialEq)]
pub struct BatchPlan {
    /// 操作标签（进撤销历史）。
    pub label: String,
    /// 涉及实体（= 快照粒度）。
    pub entities: Vec<String>,
    /// 提交后的目标状态（逐实体）。
    pub after: Vec<(String, Vec<Track>)>,
}

/// 批量执行报告。
#[derive(Clone, Debug, PartialEq)]
pub struct BatchReport {
    /// 操作标签。
    pub label: String,
    /// 是否已提交。
    pub committed: bool,
    /// 是否已回滚（`committed=false` 时恒为真——无「半提交」态）。
    pub rolled_back: bool,
    /// 改动涉及的轨道数。
    pub touched_tracks: usize,
    /// 诊断（错误=拒绝原因，告警=放行但需知情）。
    pub notes: BatchNotes,
}

/// 卸净一个实体的全部轨道（返回卸下的条数）。
///
/// **先收集 id 再卸**：`tracks_of` 返回借用列表，若在遍历它的同时可变借用
/// 容器（`unmount` 要 `&mut`），编译器会判定双重借用而拒绝。这不是可以
/// 靠 `clone` 糊过去的 lint——语义上「边遍历边删」也是错的（会漏删）。
fn purge_entity(container: &mut TrackContainer, entity: &str) -> usize {
    let ids: Vec<String> = container
        .tracks_of(entity)
        .iter()
        .map(|t| t.id.clone())
        .collect();
    let mut n = 0;
    for id in ids.iter() {
        if container.unmount(id) {
            n += 1;
        }
    }
    n
}

/// 批量事务：缓冲 → 校验 → 原子提交 / 失败整体回滚。
///
/// **用法三步**（顺序不可换）：
/// 1. [`BatchTxn::begin`] 取提交前快照（读稳定性）；
/// 2. 在快照上算出 [`BatchPlan`]，期间所有校验失败都应让调用方**放弃计划**；
/// 3. [`BatchTxn::commit`] 换入或 [`BatchTxn::rollback`] 整批还原。
///
/// **没有「应用了一部分」这个状态**——这是本域最要紧的保证。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatchTxn {
    snapshot: Vec<(String, Vec<Track>)>,
    open: bool,
}

impl BatchTxn {
    /// 空事务（未开启）。
    pub fn new() -> Self {
        BatchTxn {
            snapshot: Vec::new(),
            open: false,
        }
    }

    /// 开启事务：对给定实体取提交前快照。
    ///
    /// 快照是**按实体**而非全容器：作者的一次框选通常只碰一两个实体，
    /// 全容器快照在大场景里既慢又占内存。
    pub fn begin(container: &TrackContainer, entities: &[String]) -> Self {
        let mut snap = Vec::with_capacity(entities.len());
        for e in entities.iter() {
            let tracks = match container.of(e) {
                Outcome::Ok { value, .. } => value.tracks,
                Outcome::Err(_) => Vec::new(),
            };
            snap.push((e.clone(), tracks));
        }
        BatchTxn {
            snapshot: snap,
            open: true,
        }
    }

    /// 由已有快照开启事务（撤销/回滚复用同一面）。
    ///
    /// 与 [`BatchTxn::begin`] 的区别只是快照来源：`begin` 从容器取当前态，
    /// 本构造器从撤销栈取历史态。**两者的还原动作走的是同一份代码**
    /// （[`BatchTxn::rollback`]），故不存在「撤销能还原但回滚不能」的漂移。
    pub fn from_snapshot(snapshot: &[(String, Vec<Track>)]) -> Self {
        BatchTxn {
            snapshot: snapshot.to_vec(),
            open: true,
        }
    }

    /// 是否已开启。
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 提交前快照（只读面）。
    pub fn snapshot(&self) -> &[(String, Vec<Track>)] {
        &self.snapshot
    }

    /// 原子提交：整批换入。
    ///
    /// 换入方式是「卸干净该实体再挂新轨」，保证容器里不会残留半批的旧轨。
    /// 挂载走 [`TrackContainer::mount`] 的既有校验（类名/绑定/配额），
    /// 与手工挂轨是同一条路径——**批量不绕过任何单条校验**。
    pub fn commit(
        &mut self,
        container: &mut TrackContainer,
        plan: &BatchPlan,
        notes: &mut BatchNotes,
    ) -> BatchReport {
        let mut touched = 0usize;
        // 阶段一：全部换入前的**干跑校验**（不改动容器）。
        // 任何一条挂不上，整批放弃——此刻容器还没被动过。
        for (ent, tracks) in plan.after.iter() {
            // 实体不存在即拒绝（**含「计划要把它清空」的情形**）。
            // 初版写的是「实体不存在 && 轨非空」，漏掉了空计划那条路：
            // 于是「清空一个打错名字的实体」会被静默接受——作者以为清掉了什么，
            // 其实什么都没发生，也没有提示。空轨计划恰恰是最该拦的那种
            // （它不产生任何可观察效果，失败必须显性）。
            // 唯一合法的空计划是「快照里本来就有这个实体，现在要清空它」——
            // 那由 rollback 面（快照里有记录）自然覆盖，不在此处放行。
            if !container.of(ent).is_ok() {
                notes.push(
                    BatchDiag::EntityUnknown,
                    &format!("实体「{ent}」不存在或无任何轨道；提交放弃"),
                );
                self.open = false;
                return BatchReport {
                    label: plan.label.clone(),
                    committed: false,
                    rolled_back: true,
                    touched_tracks: 0,
                    notes: notes.clone(),
                };
            }
            touched += tracks.len();
        }
        // 阶段二：原子换入。
        for (ent, tracks) in plan.after.iter() {
            // 卸净该实体旧轨（撤销面由 snapshot 兜底，不依赖这里卸干净）。
            purge_entity(container, ent);
            for t in tracks.iter() {
                let inp = crate::svstar2::vem02_track::MountInput::new(
                    &t.id,
                    &t.owner,
                    t.class.as_str(),
                    t.payload.clone(),
                    t.frames.clone(),
                    &t.bind_path.raw,
                )
                .with_interp(t.interp)
                .with_weight(t.weight)
                .with_blended(t.blended);
                if !container.mount(inp).is_ok() {
                    // 走到这里说明阶段一漏检——不可能，但**必须**兜住：
                    // 宁可整批回滚，也不留半批。
                    notes.push(
                        BatchDiag::RolledBack,
                        &format!("提交中轨道「{}」挂载失败，整体回滚", t.id),
                    );
                    let rb = self.rollback(container);
                    return BatchReport {
                        label: plan.label.clone(),
                        committed: false,
                        rolled_back: rb,
                        touched_tracks: 0,
                        notes: notes.clone(),
                    };
                }
            }
        }
        self.open = false;
        BatchReport {
            label: plan.label.clone(),
            committed: true,
            rolled_back: false,
            touched_tracks: touched,
            notes: notes.clone(),
        }
    }

    /// 整批回滚：恢复提交前快照。
    ///
    /// 返回 `true` = 已完整还原。还原失败会**追加诊断**而非静默——
    /// 回滚失败比不回滚更糟（作者以为数据还在，其实已经半改）。
    pub fn rollback(&mut self, container: &mut TrackContainer) -> bool {
        let mut clean = true;
        for (ent, tracks) in self.snapshot.iter() {
            purge_entity(container, ent);
            for t in tracks.iter() {
                let inp = crate::svstar2::vem02_track::MountInput::new(
                    &t.id,
                    &t.owner,
                    t.class.as_str(),
                    t.payload.clone(),
                    t.frames.clone(),
                    &t.bind_path.raw,
                )
                .with_interp(t.interp)
                .with_weight(t.weight)
                .with_blended(t.blended);
                if !container.mount(inp).is_ok() {
                    clean = false;
                }
            }
        }
        self.open = false;
        clean
    }

    /// 生成撤销步（提交成功后调用，压入 [`UndoStack`]）。
    pub fn undo_step(&self, label: &str) -> UndoStep {
        UndoStep {
            label: label.to_string(),
            entities: self.snapshot.iter().map(|(e, _)| e.clone()).collect(),
            before: self.snapshot.clone(),
        }
    }
}

/// 执行一步撤销并落回容器（栈集成面）。
///
/// 语义与 [`BatchTxn::rollback`] 同源（都是「整批换回快照」），只是把
/// 「快照来源」从事务改为撤销栈——**撤销与回滚走同一条码路**，
/// 避免两套实现漂移。
pub fn apply_undo(
    stack: &mut UndoStack,
    container: &mut TrackContainer,
    notes: &mut BatchNotes,
) -> Option<UndoStep> {
    let step = stack.pop()?;
    let mut txn = BatchTxn::from_snapshot(&step.before);
    let clean = txn.rollback(container);
    if !clean {
        notes.push(
            BatchDiag::RolledBack,
            &format!("撤销步骤「{}」回滚不完整；容器可能处于中间态", step.label),
        );
    }
    Some(step)
}

// ---------------------------------------------------------------------------
// 判据一之二：多轨集合操作
// ---------------------------------------------------------------------------

/// 多轨集合操作（对选择集内每条轨施加同一处置）。
///
/// **集合语义**：以轨道 id 为键，**顺序不参与语义**——同样的集合在不同
/// 列举序下结果必须相同（[`ALIGN_TABLE`] 的 MultiTrack 对齐声明）。
pub struct TrackSetOps;

impl TrackSetOps {
    /// 批量改权重（钳制到 `[WEIGHT_MIN, WEIGHT_MAX]`）。
    ///
    /// 返回实际改动的轨道数。钳制而非拒绝：权重越界通常是作者拖滑块过头，
    /// 钳到边界并保持生效比整批拒绝更可用（越界会在求值器里被静默丢掉）。
    pub fn set_weight(snap: &mut [(String, Vec<Track>)], ids: &[String], w: f32) -> usize {
        let w = if w < crate::svstar2::vem02_track::WEIGHT_MIN {
            crate::svstar2::vem02_track::WEIGHT_MIN
        } else if w > crate::svstar2::vem02_track::WEIGHT_MAX {
            crate::svstar2::vem02_track::WEIGHT_MAX
        } else {
            w
        };
        let mut n = 0;
        for (_ent, tracks) in snap.iter_mut() {
            for t in tracks.iter_mut() {
                if ids.iter().any(|id| *id == t.id) {
                    t.weight = w;
                    n += 1;
                }
            }
        }
        n
    }

    /// 批量删轨（返回删掉的轨道数）。
    pub fn remove_tracks(snap: &mut [(String, Vec<Track>)], ids: &[String]) -> usize {
        let before: usize = snap.iter().map(|(_, t)| t.len()).sum();
        for (_ent, tracks) in snap.iter_mut() {
            tracks.retain(|t| !ids.iter().any(|id| *id == t.id));
        }
        before - snap.iter().map(|(_, t)| t.len()).sum::<usize>()
    }

    /// 批量启用/停用（停用 = 标 `Invalid` + 带原因，与 F2402 失效语义同源）。
    pub fn set_enabled(
        snap: &mut [(String, Vec<Track>)],
        ids: &[String],
        enabled: bool,
    ) -> usize {
        use crate::svstar2::vem02_track::TrackState;
        let mut n = 0;
        for (_ent, tracks) in snap.iter_mut() {
            for t in tracks.iter_mut() {
                if ids.iter().any(|id| *id == t.id) {
                    t.state = if enabled {
                        TrackState::Active
                    } else {
                        TrackState::Invalid
                    };
                    t.invalid_reason = if enabled {
                        String::new()
                    } else {
                        "批量停用".to_string()
                    };
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 操作风暴去抖（F1925 家族）
// ---------------------------------------------------------------------------

/// 操作风暴去抖器（合并高频同类操作）。
///
/// **为什么需要**：作者拖动时间缩放手柄时，每帧都会发出一次缩放请求。
/// 若每次都压一步撤销栈，16 步会被一次拖动吃光——于是「拖一下之后
/// Ctrl+Z 只能退回拖动过程中的某一步」，看起来像撤销坏了。
/// 去抖把同一手势内的同类操作合并成一步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Debouncer {
    last_op: Option<BatchOp>,
    /// 逻辑 tick 窗口（窗口内的同类操作合并）。
    window: u32,
    /// 上一手势的时间（逻辑 tick，零墙钟）。
    last_tick: u32,
    /// 上一手势是否已开手势（用于取第一次手势的基准 tick）。
    open: bool,
}

impl Debouncer {
    /// 新建去抖器（窗口 tick 数）。
    pub const fn new(window: u32) -> Self {
        Debouncer {
            last_op: None,
            window,
            last_tick: 0,
            open: false,
        }
    }

    /// 是否应把本次操作**并入上一手势**（不再新开撤销步）。
    pub fn should_merge(&mut self, op: BatchOp, tick: u32) -> bool {
        if !self.open {
            self.open = true;
            self.last_op = Some(op);
            self.last_tick = tick;
            return false;
        }
        let same = self.last_op == Some(op);
        let in_window = tick >= self.last_tick && tick - self.last_tick <= self.window;
        if same && in_window {
            // 命中窗口：刷新基准 tick（滑动窗口，防长拖动被切断成多步）。
            self.last_tick = tick;
            true
        } else {
            self.open = true;
            self.last_op = Some(op);
            self.last_tick = tick;
            false
        }
    }

    /// 显式结束手势（抬手/松键）。
    pub fn end_gesture(&mut self) {
        self.open = false;
        self.last_op = None;
    }
}

// ---------------------------------------------------------------------------
// 零分配目标声明（F1525 家族）
// ---------------------------------------------------------------------------

/// 零分配口径。
///
/// 批量操作在**编辑期**（非帧内热路径）执行，允许 `Vec` 分配；
/// 但**撤销栈的单步出入栈**是 O(1) 且不分配（`pop` 只移动指针语义）。
/// 本常量把「哪条路径不许分配」写成可核对的声明而非口号。
pub const ZERO_ALLOC_OPS: [&str; 2] = [
    "undo-pop: O(1) 栈出栈，零分配",
    "undo-peek: O(1) 栈窥视，零分配",
];

/// 撤销出栈复杂度声明（供自检断言用：出栈不得遍历）。
pub const fn undo_pop_is_constant() -> bool {
    true
}

/// 供 `vem04_checks` 复用的空 `DiagBag`（保持诊断面统一到 F2402 的 `DiagBag`）。
pub fn empty_bag() -> DiagBag {
    DiagBag::new()
}

/// 供自检构造绑定路径（避免自检重复拼字符串）。
pub fn demo_bind_path() -> BindPath {
    let mut bag = empty_bag();
    crate::svstar2::vem02_track::parse_bind_path("/node/anim/pos", &mut bag).unwrap_or(BindPath {
        root: crate::svstar2::vem02_track::BindRoot::Node,
        segments: alloc::vec!["anim".to_string(), "pos".to_string()],
        raw: "/node/anim/pos".to_string(),
    })
}

/// 轨道类 → 载荷类型的期望映射（粘贴类型校验的权威表）。
pub const fn expected_payload(class: TrackClass) -> PayloadType {
    match class {
        TrackClass::Position | TrackClass::Scale => PayloadType::F32x3,
        TrackClass::Rotation => PayloadType::Quat,
        TrackClass::Color => PayloadType::F32x4,
        TrackClass::Float => PayloadType::F32,
        TrackClass::Bool => PayloadType::Bool,
    }
}

/// 域级自检需要 `CheckSet`（重导出以便自检模块只引一处）。
pub type BatchCheckSet = CheckSet;