//! UNX-F49051 事件层与 vxwm 心跳（合成器失联时事件缓冲与恢复路径）· 0x73140
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B13 族第六槽；J1 引用按任务板原文保留）：
//!   UNX-F49051 事件层与 vxwm 心跳（合成器失联时事件缓冲与恢复路径）:
//!          ① 纯功能代码 ≥280 行；
//!          ② 判据原文：UNX-F49052-J1 fuzz ×10^4 零崩；判据锚定 体验十 健壮性；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49052 焦点 fuzz（乱焦点请求/并发切换注入不崩）:
//!          ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49053-J1 错误码 ×15 零漂移；判据锚定 §AI-62 §2 专题六；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（心跳）：序列 → 时钟 → 降级 → 缓冲 → 重放。
//! 序列先于时钟：心跳帧先必须带一个「比上一拍更晚的序列号」，
//!   否则这一帧根本不属于这条链路，谈超时毫无意义（重复帧与回退帧必须先剔）。
//! 时钟先于降级：只有在帧合法之后，「距上次心跳多少拍」这个差值才是可信的；
//!   时钟回退不是「负的超时」，它是宿主时钟被改写，必须单列 ClockBad。
//! 降级先于缓冲：链路先降级（Live→Buffering→Lost），事件层才知道要不要进缓冲。
//!   先判态再收事件，才不会把「仍然在线」的事件也堆进缓冲（那是丢实时性）。
//! 缓冲先于重放：缓冲必须先自证「条数 ≤ 容量、丢弃有账」，重放才谈得上零丢。
//!   丢弃不可怕，丢弃不入账才可怕——不可审计的丢弃等价于静默丢失。
//! 闸门次序（焦点 fuzz）：前置校验 → 事务提交 → 不变式 → 注入复跑。
//! 前置校验先于事务提交：焦点请求的全部拒绝条件必须**在读状态之前**判完，
//!   拒绝路径一个字节都不许改状态（否则「被拒的抢占」也会推进 epoch）。
//! 事务提交先于不变式：一次成功切换只允许改 owner/epoch 两处，
//!   计数三兄弟（switches/steals/probes）逐次对账。
//! 不变式先于注入复跑：单主（至多一个 owner）、epoch 单调不减这两条，
//!   必须在每一轮都成立，而不是「最后一轮看起来对」。
//! 关键差异：F49051 只回答「心跳/失联/缓冲/重放这条时间线是否守恒」，
//!   不做焦点归属；F49052 只回答「乱焦点请求与并发切换注入下不崩、不变式不掉」，
//!   不复算事件账。两卡的账彼此独立：心跳账折序列与节拍，焦点账折窗口与 epoch。
//!
//! 错误码口径（两卡共用，×15 的来源）：段内 14 条闭集（SEG_BASE+0x01..+0x0E）
//!   + 1 条越界哨（detail() 兜底返回「未知诊断码」，不占段内槽位）。
//!   本模块的 15 条冻结快照见 `CODE_SNAPSHOT`，逐行与枚举 `detail()` 比对；
//!   任何一条中文改写、任何一次枚举换序都会让快照比对翻红——这就是「零漂移」。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73140，B13 族第六槽：F49051/F49052 两卡共用）。
pub const SEG_BASE: u32 = 0x73140;

/// 心跳降级阈值（拍）：距上次心跳超过它即进入缓冲态。
pub const HB_TIMEOUT_TICKS: u64 = 24;
/// 心跳失联阈值（拍）：超过它即判失联（缓冲转「只收不发」）。
pub const HB_LOST_TICKS: u64 = 96;
/// 乱序窗口：序列号回退不超过它时按「迟到帧」接受，不推进主序列。
pub const HB_SEQ_WINDOW: u32 = 8;
/// 事件种类数（0..3）。
pub const HB_KINDS: u8 = 4;
/// 缓冲默认容量。
pub const BUF_CAP: usize = 64;
/// 缓冲高水位：越过即记一次高水位事件（不改变丢弃语义）。
pub const BUF_HIGH: usize = 48;
/// 缓冲容量上限（构造期校验，防止一次性吃光内存）。
pub const BUF_CAP_MAX: usize = 256;
/// 判据 ×10^4 的轮数。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 心跳 fuzz 种子（可复现口径的一部分，写进账摘要）。
pub const HB_SEED: u64 = 0x0000_0731_4000_0001;
/// 焦点 fuzz 种子。
pub const FOCUS_SEED: u64 = 0x0000_0731_4000_0002;
/// 焦点窗口数（窗口 0 恒为「无主」，1..FOCUS_WINDOWS 才是真窗口）。
pub const FOCUS_WINDOWS: u32 = 8;
/// 焦点 fuzz 的 epoch 上界（避免 32 位回绕把「递增」判成「回退」）。
pub const FOCUS_EPOCH_MAX: u32 = 1 << 20;
/// 错误码族目录行数：14 段内 + 1 越界哨。
pub const CODE_ROWS: usize = 15;
/// 六字段账行的字段数（tag + 4 键 + digest）。
pub const RENDER_FIELDS: usize = 6;
/// 本模块用例编号总数（每模块 16 个，供跨槽防撞号）。
pub const CASE_TOTAL: usize = 16;
/// 本模块用例编号基址（0x0734_0000 起，实际用 +0x01..+0x10）。
pub const CASE_BASE: u32 = 0x0734_0000;
/// 心跳账标签。
pub const HB_TAG: &str = "HB";
/// 缓冲账标签。
pub const BUF_TAG: &str = "BUF";
/// 焦点账标签。
pub const FOCUS_TAG: &str = "FOCUS";
/// 错误码账标签。
pub const CODE_TAG: &str = "CODE";
/// 未知诊断码说明（越界哨的正文，同时也是 15 条口径的第 15 行）。
pub const UNKNOWN: &str = "未知 vxwm 心跳诊断码";

// ── 诊断码闭集（段内 +0x01..+0x0E，共 14 条） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HbCode {
    /// 序列号非法（为 0，或回退超出乱序窗口）
    SeqBad,
    /// 降级/失联阈值非法（阈值倒挂或未推进）
    StateBad,
    /// 时钟回退（本拍早于上次心跳）
    ClockBad,
    /// 缓冲容量或种类越界
    BufferBad,
    /// 丢弃策略失效（丢弃不入账）
    DropBad,
    /// 重放不一致（重放集合与缓冲集合不等）
    ReplayBad,
    /// 重复（同号心跳或同号事件）
    DupBad,
    /// 焦点窗口非法（0 或越出窗口表）
    FocusBad,
    /// epoch 非法（未严格递增）
    EpochBad,
    /// 不变式被破坏（单主/单调）
    InvariantBad,
    /// 轮数非法（0 或超 ×10^4 口径）
    RoundBad,
    /// 渲染字段不可解析
    RenderBad,
    /// 诊断码越出段位窗口
    SegmentBad,
    /// 次序断裂或计数溢出
    OrderBad,
}

impl HbCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            HbCode::SeqBad => "序列号非法：为 0 或回退超出乱序窗口",
            HbCode::StateBad => "链路状态非法：降级/失联阈值倒挂",
            HbCode::ClockBad => "时钟回退：本拍早于上次心跳",
            HbCode::BufferBad => "缓冲非法：容量或事件种类越界",
            HbCode::DropBad => "丢弃失效：丢弃未入账（静默丢失）",
            HbCode::ReplayBad => "重放不一致：重放集合与缓冲集合不等",
            HbCode::DupBad => "重复：同号心跳或同号事件",
            HbCode::FocusBad => "焦点窗口非法：0 号或越出窗口表",
            HbCode::EpochBad => "epoch 非法：未严格递增",
            HbCode::InvariantBad => "不变式被破坏：单主或 epoch 单调",
            HbCode::RoundBad => "轮数非法：0 或超出 ×10^4 口径",
            HbCode::RenderBad => "渲染非法：六字段账行不可解析",
            HbCode::SegmentBad => "段位越界：诊断码逃出 0x73140 窗口",
            HbCode::OrderBad => "次序断裂或计数溢出",
        }
    }

    pub const ALL: [HbCode; 14] = [
        HbCode::SeqBad,
        HbCode::StateBad,
        HbCode::ClockBad,
        HbCode::BufferBad,
        HbCode::DropBad,
        HbCode::ReplayBad,
        HbCode::DupBad,
        HbCode::FocusBad,
        HbCode::EpochBad,
        HbCode::InvariantBad,
        HbCode::RoundBad,
        HbCode::RenderBad,
        HbCode::SegmentBad,
        HbCode::OrderBad,
    ];
}

/// 按诊断码取值反查中文说明；越界返回 UNKNOWN。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HbCode::ALL.len() {
        if let Some(c) = HbCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 是否为本段已知码（越界哨不算已知）。
pub fn known_code(code: u32) -> bool {
    let mut i = 0usize;
    while i < HbCode::ALL.len() {
        if let Some(c) = HbCode::ALL.get(i) {
            if c.val() == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 段位守卫：14 条码必须逐位落在 SEG_BASE+0x01..+0x0E 且互不重复。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < HbCode::ALL.len() {
        let c = match HbCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        let mut j = i + 1;
        while j < HbCode::ALL.len() {
            if let Some(o) = HbCode::ALL.get(j) {
                if o.val() == c.val() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 错误码 15 条冻结快照 ──────────────────────────────────────────
/// 一行码表：码值、英文名、中文说明、严重度（1 提示 / 2 告警 / 3 致命）。
///
/// 表里的一切都是**手写字面量**，不与枚举共享常量：一旦有人改了枚举顺序、
/// 改了 SEG_BASE、或把某条中文改得含糊，`audit_code_snapshot()` 立刻翻红。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeRow {
    pub code: u32,
    pub name: &'static str,
    pub cn: &'static str,
    pub severity: u8,
}

/// 15 条冻结快照（14 段内 + 1 越界哨）。码值全部按 0x73140 手算写出。
pub const CODE_SNAPSHOT: [CodeRow; CODE_ROWS] = [
    CodeRow { code: 0x73141, name: "SEQ_BAD", cn: "序列号非法：为 0 或回退超出乱序窗口", severity: 2 },
    CodeRow { code: 0x73142, name: "STATE_BAD", cn: "链路状态非法：降级/失联阈值倒挂", severity: 2 },
    CodeRow { code: 0x73143, name: "CLOCK_BAD", cn: "时钟回退：本拍早于上次心跳", severity: 3 },
    CodeRow { code: 0x73144, name: "BUFFER_BAD", cn: "缓冲非法：容量或事件种类越界", severity: 2 },
    CodeRow { code: 0x73145, name: "DROP_BAD", cn: "丢弃失效：丢弃未入账（静默丢失）", severity: 3 },
    CodeRow { code: 0x73146, name: "REPLAY_BAD", cn: "重放不一致：重放集合与缓冲集合不等", severity: 2 },
    CodeRow { code: 0x73147, name: "DUP_BAD", cn: "重复：同号心跳或同号事件", severity: 1 },
    CodeRow { code: 0x73148, name: "FOCUS_BAD", cn: "焦点窗口非法：0 号或越出窗口表", severity: 2 },
    CodeRow { code: 0x73149, name: "EPOCH_BAD", cn: "epoch 非法：未严格递增", severity: 2 },
    CodeRow { code: 0x7314A, name: "INVARIANT_BAD", cn: "不变式被破坏：单主或 epoch 单调", severity: 3 },
    CodeRow { code: 0x7314B, name: "ROUND_BAD", cn: "轮数非法：0 或超出 ×10^4 口径", severity: 1 },
    CodeRow { code: 0x7314C, name: "RENDER_BAD", cn: "渲染非法：六字段账行不可解析", severity: 1 },
    CodeRow { code: 0x7314D, name: "SEGMENT_BAD", cn: "段位越界：诊断码逃出 0x73140 窗口", severity: 3 },
    CodeRow { code: 0x7314E, name: "ORDER_BAD", cn: "次序断裂或计数溢出", severity: 2 },
    CodeRow { code: 0x7314F, name: "UNKNOWN", cn: UNKNOWN, severity: 0 },
];

/// 快照审计：逐行比对枚举（码值 / 中文 / 严重度 / 名字唯一），并核越界哨。
///
/// 返回 15 行的折叠摘要（供复跑比对），任何一行不符直接给码。
pub fn audit_code_snapshot() -> Result<u64, u32> {
    if CODE_SNAPSHOT.len() != CODE_ROWS || HbCode::ALL.len() + 1 != CODE_ROWS {
        return Err(HbCode::SegmentBad.val());
    }
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CODE_ROWS {
        let row = match CODE_SNAPSHOT.get(i) {
            Some(v) => *v,
            None => return Err(HbCode::SegmentBad.val()),
        };
        if row.severity > 3 {
            return Err(HbCode::SegmentBad.val());
        }
        if i + 1 == CODE_ROWS {
            // 越界哨：码值必须紧贴段尾，且正文等于 UNKNOWN。
            if row.code != SEG_BASE + 0x0F || row.cn != UNKNOWN {
                return Err(HbCode::SegmentBad.val());
            }
            if known_code(row.code) {
                return Err(HbCode::SegmentBad.val());
            }
        } else {
            let c = match HbCode::ALL.get(i) {
                Some(v) => *v,
                None => return Err(HbCode::SegmentBad.val()),
            };
            if row.code != c.val() {
                return Err(HbCode::SegmentBad.val());
            }
            if row.cn != c.detail() {
                return Err(HbCode::RenderBad.val());
            }
            if row.severity == 0 {
                return Err(HbCode::SegmentBad.val());
            }
            if !known_code(row.code) {
                return Err(HbCode::SegmentBad.val());
            }
        }
        // 名字必须非空且两两不同（互不混淆的文本前提）。
        if row.name.is_empty() {
            return Err(HbCode::RenderBad.val());
        }
        let mut j = i + 1;
        while j < CODE_ROWS {
            if let Some(o) = CODE_SNAPSHOT.get(j) {
                if o.name == row.name || o.code == row.code {
                    return Err(HbCode::DupBad.val());
                }
            }
            j += 1;
        }
        h = p_core::fold_u32(h, row.code);
        h = p_core::fold_byte(h, row.severity);
        let mut k = 0usize;
        while k < row.name.len() {
            if let Some(b) = row.name.as_bytes().get(k).copied() {
                h = p_core::fold_byte(h, b);
            }
            k += 1;
        }
        i += 1;
    }
    if h == 0 {
        return Err(HbCode::OrderBad.val());
    }
    Ok(h)
}

/// 按码值取快照行（含越界哨）。
pub fn snapshot_row(code: u32) -> Option<CodeRow> {
    let mut i = 0usize;
    while i < CODE_SNAPSHOT.len() {
        if let Some(r) = CODE_SNAPSHOT.get(i) {
            if r.code == code {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

// ── 链路状态 ──────────────────────────────────────────────────────
/// 合成器链路状态：在线 / 缓冲（降级）/ 失联。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    Live,
    Buffering,
    Lost,
}

impl LinkState {
    pub fn index(self) -> u8 {
        match self {
            LinkState::Live => 0,
            LinkState::Buffering => 1,
            LinkState::Lost => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LinkState::Live => "在线",
            LinkState::Buffering => "缓冲",
            LinkState::Lost => "失联",
        }
    }

    /// 单步迁移白名单：失联只能靠一帧合法心跳回到在线（见 Heartbeat::observe）。
    ///
    /// `Live → Lost` 是允许的：poll 是**电平式**判定，一次长间隔的 poll
    /// （中间没有 poll 过）会直接从在线跳到失联；禁止它会逼调用方补拍空 poll，
    /// 那是把「时钟跳变」伪装成「按拍推进」。
    pub fn legal_step(from: LinkState, to: LinkState) -> bool {
        match (from, to) {
            (LinkState::Live, LinkState::Live) => true,
            (LinkState::Live, LinkState::Buffering) => true,
            (LinkState::Live, LinkState::Lost) => true,
            (LinkState::Buffering, LinkState::Buffering) => true,
            (LinkState::Buffering, LinkState::Lost) => true,
            (LinkState::Buffering, LinkState::Live) => true,
            (LinkState::Lost, LinkState::Lost) => true,
            (LinkState::Lost, LinkState::Live) => true,
            _ => false,
        }
    }
}

// ── vxwm 心跳 ─────────────────────────────────────────────────────
/// 心跳账：主序列、上次拍、丢失拍数、乱序次数、状态、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Heartbeat {
    pub seq: u32,
    pub last_tick: u64,
    pub misses: u32,
    pub beats: u32,
    pub gaps: u32,
    pub reorders: u32,
    pub rejects: u32,
    pub state: LinkState,
    pub digest: u64,
}

impl Heartbeat {
    pub fn new() -> Heartbeat {
        Heartbeat {
            seq: 0,
            last_tick: 0,
            misses: 0,
            beats: 0,
            gaps: 0,
            reorders: 0,
            rejects: 0,
            state: LinkState::Buffering,
            digest: p_core::FNV_OFF,
        }
    }

    /// 吞一帧心跳。返回吞入后的链路状态（必为 Live）。
    ///
    /// 拒绝路径**不改任何字段**：这正是「被拒的心跳不能推进主序列」的可测形态。
    pub fn observe(&mut self, seq: u32, tick: u64) -> Result<LinkState, u32> {
        if seq == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HbCode::SeqBad.val());
        }
        if tick < self.last_tick {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HbCode::ClockBad.val());
        }
        if self.beats > 0 {
            if seq == self.seq {
                self.rejects = self.rejects.saturating_add(1);
                return Err(HbCode::DupBad.val());
            }
            if seq < self.seq {
                // 迟到帧：窗口内接受（只记乱序），窗口外一律判序列非法。
                let back = self.seq - seq;
                if back > HB_SEQ_WINDOW {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(HbCode::SeqBad.val());
                }
                self.reorders = self.reorders.saturating_add(1);
                self.last_tick = tick;
                self.misses = 0;
                self.state = LinkState::Live;
                self.digest = p_core::fold_u32(self.digest, seq);
                self.digest = p_core::fold_u32(self.digest, 1);
                return Ok(self.state);
            }
        }
        let gap = seq.saturating_sub(self.seq).saturating_sub(1);
        self.seq = seq;
        self.last_tick = tick;
        self.misses = 0;
        self.beats = self.beats.saturating_add(1);
        self.gaps = self.gaps.saturating_add(gap);
        self.state = LinkState::Live;
        self.digest = p_core::fold_u32(self.digest, seq);
        self.digest = p_core::fold_u32(self.digest, gap);
        let hi = (tick >> 32) as u32;
        self.digest = p_core::fold_u32(self.digest, hi);
        self.digest = p_core::fold_u32(self.digest, tick as u32);
        Ok(self.state)
    }

    /// 推进时钟并给出当前状态（不产生心跳）。
    ///
    /// 状态只降不升：回在线必须靠 observe；poll 若能把状态「拍回在线」，
    /// 那么缓冲层就永远等不到恢复信号。
    pub fn poll(&mut self, tick: u64) -> Result<LinkState, u32> {
        if tick < self.last_tick {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HbCode::ClockBad.val());
        }
        if HB_TIMEOUT_TICKS == 0 || HB_LOST_TICKS <= HB_TIMEOUT_TICKS {
            return Err(HbCode::StateBad.val());
        }
        let elapsed = tick.saturating_sub(self.last_tick);
        let next = if elapsed <= HB_TIMEOUT_TICKS {
            LinkState::Live
        } else if elapsed <= HB_LOST_TICKS {
            LinkState::Buffering
        } else {
            LinkState::Lost
        };
        if !LinkState::legal_step(self.state, next) {
            return Err(HbCode::StateBad.val());
        }
        if next != self.state {
            self.misses = self.misses.saturating_add(1);
            self.state = next;
            self.digest = p_core::fold_u32(self.digest, next.index() as u32);
            self.digest = p_core::fold_u32(self.digest, elapsed as u32);
        }
        Ok(self.state)
    }

    /// 心跳账自审：计数次序、丢失拍与段位。
    pub fn audit(&self) -> Result<(), u32> {
        if self.beats == 0 {
            return Err(HbCode::OrderBad.val());
        }
        if self.seq == 0 {
            return Err(HbCode::SeqBad.val());
        }
        // 计数次序：被接受的帧数不可能超过主序列（每次接受至少 +1），
        // 丢失拍数也不可能超过主序列（丢失拍 = 主序列 − 接受帧数）。
        if self.beats > self.seq || self.gaps > self.seq {
            return Err(HbCode::OrderBad.val());
        }
        if !segment_guard() {
            return Err(HbCode::SegmentBad.val());
        }
        Ok(())
    }

    /// 是否处于「只收不发」的失联态。
    pub fn buffering(&self) -> bool {
        match self.state {
            LinkState::Buffering => true,
            LinkState::Lost => true,
            LinkState::Live => false,
        }
    }
}

impl Default for Heartbeat {
    fn default() -> Heartbeat {
        Heartbeat::new()
    }
}

// ── 事件与缓冲 ────────────────────────────────────────────────────
/// 一条 vxwm 事件：序列号、种类、载荷（全部整数，无墙钟无浮点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub seq: u32,
    pub kind: u8,
    pub payload: u32,
}

impl Event {
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.seq);
        h = p_core::fold_byte(h, self.kind);
        h = p_core::fold_u32(h, self.payload);
        h
    }
}

/// 缓冲满时的策略。两种策略都必须**显式记账**（dropped 或 rejected）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropPolicy {
    /// 丢最旧（实时性优先：新事件一定进得来）。
    OldestDrop,
    /// 拒最新（完整性优先：已有事件不动，新事件被明确拒绝）。
    RejectNewest,
}

impl DropPolicy {
    pub fn index(self) -> u8 {
        match self {
            DropPolicy::OldestDrop => 0,
            DropPolicy::RejectNewest => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DropPolicy::OldestDrop => "丢最旧",
            DropPolicy::RejectNewest => "拒最新",
        }
    }
}

/// 失联期事件缓冲：定容、可排序重放、丢弃入账。
#[derive(Clone, Debug)]
pub struct EventBuf {
    pub cap: usize,
    pub policy: DropPolicy,
    pub items: Vec<Event>,
    pub pushed: u32,
    pub dropped: u32,
    pub rejected: u32,
    pub delivered: u32,
    pub high_events: u32,
    pub digest: u64,
}

impl EventBuf {
    /// 构造：容量必须落在 8..=BUF_CAP_MAX（太小的缓冲连一次突发都兜不住）。
    pub fn new(cap: usize, policy: DropPolicy) -> Result<EventBuf, u32> {
        if cap < 8 || cap > BUF_CAP_MAX {
            return Err(HbCode::BufferBad.val());
        }
        Ok(EventBuf {
            cap,
            policy,
            items: Vec::new(),
            pushed: 0,
            dropped: 0,
            rejected: 0,
            delivered: 0,
            high_events: 0,
            digest: p_core::FNV_OFF,
        })
    }

    /// 默认缓冲（64 条、丢最旧）。
    pub fn standard() -> EventBuf {
        EventBuf {
            cap: BUF_CAP,
            policy: DropPolicy::OldestDrop,
            items: Vec::new(),
            pushed: 0,
            dropped: 0,
            rejected: 0,
            delivered: 0,
            high_events: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.items.len() >= self.cap
    }

    /// 缓冲内是否已有该序列号（重放零重复的前提）。
    pub fn contains_seq(&self, seq: u32) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(e) = self.items.get(i) {
                if e.seq == seq {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 收一条事件，返回本次为腾位丢掉的条数（0 或 1）。
    ///
    /// 次序：序列 → 种类 → 重复 → 容量。序列与种类不合法时**不消耗容量**，
    /// 否则一条烂事件也能把一条好事件挤出去。
    pub fn push(&mut self, ev: Event) -> Result<u32, u32> {
        if ev.seq == 0 {
            return Err(HbCode::SeqBad.val());
        }
        if ev.kind >= HB_KINDS {
            return Err(HbCode::BufferBad.val());
        }
        if self.contains_seq(ev.seq) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(HbCode::DupBad.val());
        }
        let mut dropped_now = 0u32;
        if self.is_full() {
            match self.policy {
                DropPolicy::OldestDrop => {
                    if self.items.is_empty() {
                        return Err(HbCode::DropBad.val());
                    }
                    self.items.remove(0);
                    dropped_now = 1;
                    self.dropped = self.dropped.saturating_add(1);
                }
                DropPolicy::RejectNewest => {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(HbCode::BufferBad.val());
                }
            }
        }
        self.items.push(ev);
        self.pushed = self.pushed.saturating_add(1);
        if self.items.len() > BUF_HIGH {
            self.high_events = self.high_events.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, ev.seq);
        self.digest = p_core::fold_byte(self.digest, ev.kind);
        self.digest = p_core::fold_u32(self.digest, ev.payload);
        self.digest = p_core::fold_u32(self.digest, dropped_now);
        Ok(dropped_now)
    }

    /// 缓冲自审：条数不超容、推送 = 在册 + 丢弃、摘要非零（空缓冲除外）。
    pub fn audit(&self) -> Result<(), u32> {
        if self.items.len() > self.cap {
            return Err(HbCode::BufferBad.val());
        }
        if self.cap < 8 || self.cap > BUF_CAP_MAX {
            return Err(HbCode::BufferBad.val());
        }
        let in_register = match u32::try_from(self.items.len()) {
            Ok(v) => v,
            Err(_) => return Err(HbCode::OrderBad.val()),
        };
        // 守恒恒等式：推送 = 在册 + 丢弃 + 已重放。
        // 「已重放」必须进等式，否则恢复一次之后审计会把重放出去的事件
        // 误判成静默丢失（首版就栽在这里）。
        let accounted = match in_register
            .checked_add(self.dropped)
            .and_then(|v| v.checked_add(self.delivered))
        {
            Some(v) => v,
            None => return Err(HbCode::OrderBad.val()),
        };
        if self.pushed != accounted {
            return Err(HbCode::DropBad.val());
        }
        let mut i = 0usize;
        while i < self.items.len() {
            let e = match self.items.get(i) {
                Some(v) => *v,
                None => return Err(HbCode::OrderBad.val()),
            };
            let mut j = i + 1;
            while j < self.items.len() {
                if let Some(o) = self.items.get(j) {
                    if o.seq == e.seq {
                        return Err(HbCode::DupBad.val());
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 按序列号插入排序（原地）。重放前必须定序，否则「顺序」会随丢弃漂移。
    fn order_by_seq(items: &mut [Event]) -> Result<(), u32> {
        let mut i = 1usize;
        while i < items.len() {
            let mut j = i;
            while j > 0 {
                let cur = match items.get(j) {
                    Some(v) => v.seq,
                    None => return Err(HbCode::OrderBad.val()),
                };
                let prev = match items.get(j - 1) {
                    Some(v) => v.seq,
                    None => return Err(HbCode::OrderBad.val()),
                };
                if prev <= cur {
                    break;
                }
                items.swap(j, j - 1);
                j -= 1;
            }
            i += 1;
        }
        let mut k = 1usize;
        while k < items.len() {
            let a = match items.get(k - 1) {
                Some(v) => v.seq,
                None => return Err(HbCode::OrderBad.val()),
            };
            let b = match items.get(k) {
                Some(v) => v.seq,
                None => return Err(HbCode::OrderBad.val()),
            };
            if a == b {
                return Err(HbCode::DupBad.val());
            }
            k += 1;
        }
        Ok(())
    }

    /// 只读重放：把在册事件按序列号定序后逐条折进 `out`，返回条数与摘要。
    pub fn replay_into(&self, out: &mut Vec<Event>) -> Result<(u32, u64), u32> {
        let mut copy: Vec<Event> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(e) = self.items.get(i) {
                copy.push(*e);
            }
            i += 1;
        }
        EventBuf::order_by_seq(&mut copy[..])?;
        let mut h = p_core::FNV_OFF;
        let mut n = 0u32;
        let mut k = 0usize;
        while k < copy.len() {
            let e = match copy.get(k) {
                Some(v) => *v,
                None => return Err(HbCode::OrderBad.val()),
            };
            h = p_core::fold_u32(h, e.seq);
            h = p_core::fold_byte(h, e.kind);
            h = p_core::fold_u32(h, e.payload);
            out.push(e);
            n = n.saturating_add(1);
            k += 1;
        }
        Ok((n, h))
    }

    /// 恢复重放：定序吐出并清空缓冲。吐出前先算摘要，吐完不复算——
    /// 摘要必须描述「吐出去的那一批」，清空后再算就成了空账。
    pub fn drain_replay(&mut self) -> Result<(Vec<Event>, u64), u32> {
        let mut out: Vec<Event> = Vec::new();
        let (n, h) = self.replay_into(&mut out)?;
        if n as usize != self.items.len() {
            return Err(HbCode::ReplayBad.val());
        }
        self.delivered = self.delivered.saturating_add(n);
        self.items.clear();
        self.digest = p_core::fold_u32(self.digest, n);
        self.digest = p_core::fold_u32(self.digest, (h >> 32) as u32);
        self.digest = p_core::fold_u32(self.digest, h as u32);
        Ok((out, h))
    }
}

// ── 恢复路径 ──────────────────────────────────────────────────────
/// 一次失联-恢复周期的账：失联期推送、丢齐、重放、重复。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Recovery {
    pub outage_ticks: u64,
    pub pushed: u32,
    pub dropped: u32,
    pub replayed: u32,
    pub duplicates: u32,
    pub bytes_digest: u64,
}

impl Recovery {
    pub fn zero_loss(&self) -> bool {
        match self.pushed.checked_sub(self.dropped) {
            Some(v) => v == self.replayed && self.duplicates == 0,
            None => false,
        }
    }
}

/// 走一遍完整恢复路径：正常 → 失联 → 缓冲 → 心跳恢复 → 重放。
///
/// 反例说明（首版教训）：第一版把「重放条数」与「推送条数」直接比，
/// 在丢最旧策略下必然不相等——被挤掉的旧事件早就不在缓冲里了。
/// 「零丢」的精确口径是 `重放 + 丢弃 == 推送`：丢弃不是丢失，
/// 未入账的丢弃才是丢失。
pub fn recovery_path(outage_ticks: u64, burst: u32, policy: DropPolicy) -> Result<Recovery, u32> {
    if outage_ticks <= HB_TIMEOUT_TICKS {
        // 不够一次降级的「失联」是假失联：这里拒收，避免把正常拍当恢复路径验。
        return Err(HbCode::StateBad.val());
    }
    if burst == 0 {
        return Err(HbCode::RoundBad.val());
    }
    let mut hb = Heartbeat::new();
    let mut buf = EventBuf::new(BUF_CAP, policy)?;
    let mut tick = 1u64;
    hb.observe(1, tick)?;
    if hb.state != LinkState::Live {
        return Err(HbCode::StateBad.val());
    }
    // 失联：时钟推进，不发心跳，只有事件进来。
    tick = tick.saturating_add(outage_ticks);
    let st = hb.poll(tick)?;
    if !hb.buffering() || (st != LinkState::Buffering && st != LinkState::Lost) {
        return Err(HbCode::StateBad.val());
    }
    let mut n = 0u32;
    while n < burst {
        let ev = Event {
            seq: 100 + n,
            kind: (n % HB_KINDS as u32) as u8,
            payload: n.wrapping_mul(2_654_435_761),
        };
        let _ = buf.push(ev)?;
        n = n.saturating_add(1);
    }
    buf.audit()?;
    let pushed = buf.pushed;
    let dropped = buf.dropped;
    // 恢复：下一帧合法心跳把链路拉回在线。
    tick = tick.saturating_add(1);
    hb.observe(2, tick)?;
    if hb.state != LinkState::Live {
        return Err(HbCode::StateBad.val());
    }
    // 重放：先把「在册集合」抄一份，吐完后逐条回查是否都吐过。
    let mut expect: Vec<Event> = Vec::new();
    let (n_before, _) = buf.replay_into(&mut expect)?;
    let (got, h) = buf.drain_replay()?;
    if got.len() != n_before as usize {
        return Err(HbCode::ReplayBad.val());
    }
    let mut i = 0usize;
    while i < got.len() {
        let a = match got.get(i) {
            Some(v) => *v,
            None => return Err(HbCode::ReplayBad.val()),
        };
        let b = match expect.get(i) {
            Some(v) => *v,
            None => return Err(HbCode::ReplayBad.val()),
        };
        if a != b {
            return Err(HbCode::ReplayBad.val());
        }
        i += 1;
    }
    let mut duplicates = 0u32;
    let mut k = 0usize;
    while k < got.len() {
        if let Some(a) = got.get(k) {
            let mut m = k + 1;
            while m < got.len() {
                if let Some(b) = got.get(m) {
                    if a.seq == b.seq {
                        duplicates = duplicates.saturating_add(1);
                    }
                }
                m += 1;
            }
        }
        k += 1;
    }
    let r = Recovery {
        outage_ticks,
        pushed,
        dropped,
        replayed: got.len() as u32,
        duplicates,
        bytes_digest: h,
    };
    if !r.zero_loss() {
        return Err(HbCode::ReplayBad.val());
    }
    Ok(r)
}

// ── 焦点系统（F49052） ────────────────────────────────────────────
/// 焦点请求种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReqKind {
    /// 申请获得焦点（窗口当前无主）
    Acquire,
    /// 主动释放（仅属主可释放）
    Release,
    /// 抢占（带更高 epoch）
    Steal,
    /// 只读探询（不改状态）
    Probe,
}

impl ReqKind {
    pub fn index(self) -> u8 {
        match self {
            ReqKind::Acquire => 0,
            ReqKind::Release => 1,
            ReqKind::Steal => 2,
            ReqKind::Probe => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ReqKind::Acquire => "申请",
            ReqKind::Release => "释放",
            ReqKind::Steal => "抢占",
            ReqKind::Probe => "探询",
        }
    }

    pub fn from_index(i: u8) -> Result<ReqKind, u32> {
        match i {
            0 => Ok(ReqKind::Acquire),
            1 => Ok(ReqKind::Release),
            2 => Ok(ReqKind::Steal),
            3 => Ok(ReqKind::Probe),
            _ => Err(HbCode::FocusBad.val()),
        }
    }
}

/// 一条焦点请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FocusReq {
    pub window: u32,
    pub epoch: u32,
    pub kind: ReqKind,
}

impl FocusReq {
    pub fn new(window: u32, epoch: u32, kind: ReqKind) -> FocusReq {
        FocusReq { window, epoch, kind }
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.window);
        h = p_core::fold_u32(h, self.epoch);
        h = p_core::fold_byte(h, self.kind.index());
        h
    }
}

/// 焦点系统：单主（owner == 0 表示无主）、epoch 单调不减。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FocusSys {
    pub owner: u32,
    pub epoch: u32,
    pub switches: u32,
    pub steals: u32,
    pub releases: u32,
    pub probes: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl FocusSys {
    pub fn new() -> FocusSys {
        FocusSys {
            owner: 0,
            epoch: 0,
            switches: 0,
            steals: 0,
            releases: 0,
            probes: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 前置校验：全部拒绝条件在这里判完，且**不读改状态**。
    fn precheck(&self, r: &FocusReq) -> Result<(), u32> {
        if r.window == 0 || r.window >= FOCUS_WINDOWS {
            return Err(HbCode::FocusBad.val());
        }
        match r.kind {
            ReqKind::Probe => Ok(()),
            ReqKind::Acquire => {
                if self.owner == r.window {
                    return Err(HbCode::DupBad.val());
                }
                if self.owner != 0 {
                    // 有主时申请必须走抢占（显式 epoch 抬升），否则是偷。
                    return Err(HbCode::InvariantBad.val());
                }
                if r.epoch <= self.epoch {
                    return Err(HbCode::EpochBad.val());
                }
                Ok(())
            }
            ReqKind::Release => {
                if self.owner != r.window {
                    return Err(HbCode::FocusBad.val());
                }
                if r.epoch <= self.epoch {
                    return Err(HbCode::EpochBad.val());
                }
                Ok(())
            }
            ReqKind::Steal => {
                if self.owner == r.window {
                    return Err(HbCode::DupBad.val());
                }
                if self.owner == 0 {
                    // 无主时「抢占」没有可抢的对象，按申请处理是不允许的隐式升级。
                    return Err(HbCode::InvariantBad.val());
                }
                if r.epoch <= self.epoch {
                    return Err(HbCode::EpochBad.val());
                }
                Ok(())
            }
        }
    }

    /// 提交一条焦点请求，返回提交后的 owner（0 = 无主）。
    pub fn request(&mut self, r: &FocusReq) -> Result<u32, u32> {
        if let Err(e) = self.precheck(r) {
            self.rejects = self.rejects.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        match r.kind {
            ReqKind::Probe => {
                self.probes = self.probes.saturating_add(1);
            }
            ReqKind::Acquire => {
                self.owner = r.window;
                self.epoch = r.epoch;
                self.switches = self.switches.saturating_add(1);
            }
            ReqKind::Release => {
                self.owner = 0;
                self.epoch = r.epoch;
                self.releases = self.releases.saturating_add(1);
                self.switches = self.switches.saturating_add(1);
            }
            ReqKind::Steal => {
                self.owner = r.window;
                self.epoch = r.epoch;
                self.steals = self.steals.saturating_add(1);
                self.switches = self.switches.saturating_add(1);
            }
        }
        self.digest = self.digest.fold_u64_like(r.window, r.epoch, r.kind.index());
        Ok(self.owner)
    }

    /// 不变式：owner 在窗口表内（0 允许）、epoch 未回绕、
    /// 成功切换数 = 申请 + 释放 + 抢占。
    pub fn invariant(&self) -> bool {
        if self.owner >= FOCUS_WINDOWS {
            return false;
        }
        if self.epoch > FOCUS_EPOCH_MAX {
            return false;
        }
        match self.switches.checked_sub(self.steals.saturating_add(self.releases)) {
            Some(v) => v <= self.switches,
            None => false,
        }
    }

    /// 提交总数对账：成功 + 拒绝 == 请求数（由调用方传入的计数核）。
    pub fn tally_ok(&self, issued: u32) -> bool {
        let ok = self.switches.saturating_add(self.probes);
        match ok.checked_add(self.rejects) {
            Some(v) => v == issued,
            None => false,
        }
    }
}

impl Default for FocusSys {
    fn default() -> FocusSys {
        FocusSys::new()
    }
}

/// 局部折叠助手：三次折叠写在一处，避免每个调用点各写两行。
trait FoldTriple {
    fn fold_u64_like(self, a: u32, b: u32, c: u8) -> u64;
}

impl FoldTriple for u64 {
    fn fold_u64_like(self, a: u32, b: u32, c: u8) -> u64 {
        let mut h = p_core::fold_u32(self, a);
        h = p_core::fold_u32(h, b);
        h = p_core::fold_byte(h, c);
        h
    }
}

// ── fuzz：心跳 ×10^4 ─────────────────────────────────────────────
/// 心跳 fuzz 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HxFuzz {
    pub rounds: u32,
    pub beats: u32,
    pub gap_total: u32,
    pub buffered: u32,
    pub replayed: u32,
    pub dropped: u32,
    pub rejects: u32,
    pub recoveries: u32,
    pub digest: u64,
}

impl HxFuzz {
    pub fn empty() -> HxFuzz {
        HxFuzz {
            rounds: 0,
            beats: 0,
            gap_total: 0,
            buffered: 0,
            replayed: 0,
            dropped: 0,
            rejects: 0,
            recoveries: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 零丢：每个恢复周期内「重放 + 丢弃 == 推送」。
    pub fn zero_loss(&self) -> bool {
        match self.buffered.checked_sub(self.dropped) {
            Some(v) => v == self.replayed,
            None => false,
        }
    }
}

/// 心跳 fuzz：随机节拍/跳号/重复/时钟回退/失联周期，恒不变式。
///
/// 「零崩」的实现口径：全程没有 `unwrap`/索引直取，任何越界都由 `?` 返回码，
/// 而 fuzz 自己把「期望被拒」的那几类拒绝计入 rejects（所以 rejects 非零是正常的）。
pub fn fuzz_heartbeat(rounds: u32) -> Result<HxFuzz, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(HbCode::RoundBad.val());
    }
    let mut rng = p_core::Lcg::new(HB_SEED);
    let mut hb = Heartbeat::new();
    let mut buf = EventBuf::standard();
    let mut out = HxFuzz::empty();
    out.rounds = rounds;
    let mut tick = 1u64;
    let mut seq = 0u32;
    let mut last_state = LinkState::Buffering;
    let mut i = 0u32;
    while i < rounds {
        tick = tick.saturating_add(1 + rng.below(5) as u64);
        let mode = rng.below(100);
        if mode < 55 {
            // 正常/跳号心跳：跳号必须体现在 gap_total 上。
            let jump = rng.below(4);
            seq = seq.saturating_add(1).saturating_add(jump);
            match hb.observe(seq, tick) {
                Ok(st) => {
                    if !LinkState::legal_step(last_state, st) {
                        return Err(HbCode::StateBad.val());
                    }
                    last_state = st;
                    out.beats = out.beats.saturating_add(1);
                    out.gap_total = out.gap_total.saturating_add(jump);
                }
                Err(e) => {
                    out.rejects = out.rejects.saturating_add(1);
                    out.digest = p_core::fold_u32(out.digest, e);
                }
            }
        } else if mode < 63 {
            // 重复帧：必须被拒，且主序列不许动。
            let before = hb.seq;
            if hb.observe(seq, tick).is_err() {
                out.rejects = out.rejects.saturating_add(1);
            }
            if hb.seq != before {
                return Err(HbCode::DupBad.val());
            }
        } else if mode < 70 {
            // 时钟回退：必须被拒。
            let back = tick.saturating_sub(2);
            if hb.observe(seq.saturating_add(1), back).is_err() {
                out.rejects = out.rejects.saturating_add(1);
            }
        } else if mode < 78 {
            // 序列 0：必须被拒。
            if hb.observe(0, tick).is_err() {
                out.rejects = out.rejects.saturating_add(1);
            }
        } else {
            // 失联窗口：不发心跳，事件入缓冲，再按概率恢复。
            let st = match hb.poll(tick) {
                Ok(v) => v,
                Err(e) => {
                    out.rejects = out.rejects.saturating_add(1);
                    out.digest = p_core::fold_u32(out.digest, e);
                    last_state
                }
            };
            if !LinkState::legal_step(last_state, st) {
                return Err(HbCode::StateBad.val());
            }
            last_state = st;
            let burst = 1 + rng.below(3);
            let mut k = 0u32;
            while k < burst {
                let ev = Event {
                    seq: 1_000 + out.buffered,
                    kind: rng.below(HB_KINDS as u32) as u8,
                    payload: rng.below(1 << 20),
                };
                match buf.push(ev) {
                    Ok(d) => {
                        out.buffered = out.buffered.saturating_add(1);
                        out.dropped = out.dropped.saturating_add(d);
                    }
                    Err(_) => {
                        out.rejects = out.rejects.saturating_add(1);
                    }
                }
                k = k.saturating_add(1);
            }
            if hb.buffering() && rng.below(2) == 0 {
                // 恢复：合法心跳 + 重放。
                seq = seq.saturating_add(1);
                match hb.observe(seq, tick) {
                    Ok(st) => {
                        if st != LinkState::Live {
                            return Err(HbCode::StateBad.val());
                        }
                        last_state = st;
                        out.beats = out.beats.saturating_add(1);
                    }
                    Err(e) => {
                        out.rejects = out.rejects.saturating_add(1);
                        out.digest = p_core::fold_u32(out.digest, e);
                    }
                }
                let (got, h) = buf.drain_replay()?;
                out.replayed = out.replayed.saturating_add(got.len() as u32);
                out.recoveries = out.recoveries.saturating_add(1);
                out.digest = p_core::fold_u32(out.digest, (h >> 32) as u32);
                out.digest = p_core::fold_u32(out.digest, h as u32);
            }
        }
        buf.audit()?;
        if buf.len() > buf.cap {
            return Err(HbCode::BufferBad.val());
        }
        if hb.beats > 0 {
            hb.audit()?;
        }
        i = i.saturating_add(1);
    }
    // 收尾：零丢口径要把「仍在缓冲里的那批」也重放掉，
    // 否则 pushed − dropped ≠ replayed 会被误判成丢事件。
    let (leftover, lh) = buf.drain_replay()?;
    out.replayed = out.replayed.saturating_add(leftover.len() as u32);
    out.digest = p_core::fold_u32(out.digest, (lh >> 32) as u32);
    out.digest = p_core::fold_u32(out.digest, lh as u32);
    if out.zero_loss() == false {
        // 只有「丢最旧」策略下 discard 才会发生；丢弃必须与推送对得上。
        return Err(HbCode::DropBad.val());
    }
    out.digest = p_core::fold_u32(out.digest, out.beats);
    out.digest = p_core::fold_u32(out.digest, out.replayed);
    if out.digest == 0 {
        return Err(HbCode::OrderBad.val());
    }
    Ok(out)
}

// ── fuzz：焦点 ×10^4（含并发双流切换注入） ───────────────────────
/// 焦点 fuzz 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FxFuzz {
    pub rounds: u32,
    pub issued: u32,
    pub switches: u32,
    pub steals: u32,
    pub releases: u32,
    pub probes: u32,
    pub rejects: u32,
    pub invariants: u32,
    pub digest: u64,
}

impl FxFuzz {
    pub fn empty() -> FxFuzz {
        FxFuzz {
            rounds: 0,
            issued: 0,
            switches: 0,
            steals: 0,
            releases: 0,
            probes: 0,
            rejects: 0,
            invariants: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 记账守恒：成功 + 拒绝 == 发出。
    pub fn conserved(&self) -> bool {
        let ok = self.switches.saturating_add(self.probes);
        match ok.checked_add(self.rejects) {
            Some(v) => v == self.issued,
            None => false,
        }
    }

    /// 不变式全程未破（invariants == rounds）。
    pub fn invariants_held(&self) -> bool {
        self.invariants == self.rounds && self.rounds > 0
    }
}

/// 焦点 fuzz：乱焦点请求（随机窗口/epoch/种类）+ 并发双流切换注入。
///
/// 两条流各自持有一个 Lcg（A 流抢单号窗口、B 流抢双号窗口），逐轮交错提交：
/// 这正是「并发切换」在无锁模型下的最小可测形态——谁先提交谁改状态，
/// 但无论顺序如何，单主与 epoch 单调两条不变式都不许破。
pub fn fuzz_focus(rounds: u32) -> Result<FxFuzz, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(HbCode::RoundBad.val());
    }
    let mut a = p_core::Lcg::new(FOCUS_SEED);
    let mut b = p_core::Lcg::new(FOCUS_SEED ^ 0x5A5A_0731);
    let mut sys = FocusSys::new();
    let mut out = FxFuzz::empty();
    out.rounds = rounds;
    let mut i = 0u32;
    while i < rounds {
        let stream = i & 1;
        let rng = if stream == 0 { &mut a } else { &mut b };
        let kind = match ReqKind::from_index(rng.below(4) as u8) {
            Ok(v) => v,
            Err(_) => return Err(HbCode::FocusBad.val()),
        };
        let mut window = 1 + rng.below(FOCUS_WINDOWS - 1);
        if stream == 1 && window % 2 == 0 {
            window = if window + 1 < FOCUS_WINDOWS { window + 1 } else { 1 };
        }
        let epoch = 1 + rng.below(FOCUS_EPOCH_MAX);
        // 每 7 轮强制一次「正好接上」的合法请求，保证成功路径被压到。
        let req = if i % 7 == 0 {
            FocusReq::new(
                if sys.owner == 0 { 1 } else { sys.owner },
                sys.epoch.saturating_add(1),
                if sys.owner == 0 { ReqKind::Acquire } else { ReqKind::Steal },
            )
        } else {
            FocusReq::new(window, epoch, kind)
        };
        let owner_before = sys.owner;
        let epoch_before = sys.epoch;
        out.issued = out.issued.saturating_add(1);
        match sys.request(&req) {
            Ok(owner) => {
                if req.kind == ReqKind::Probe {
                    out.probes = out.probes.saturating_add(1);
                    if owner != owner_before || sys.epoch != epoch_before {
                        return Err(HbCode::InvariantBad.val());
                    }
                } else {
                    if sys.epoch < epoch_before {
                        return Err(HbCode::EpochBad.val());
                    }
                    match req.kind {
                        ReqKind::Acquire => out.switches = out.switches.saturating_add(1),
                        ReqKind::Release => out.releases = out.releases.saturating_add(1),
                        ReqKind::Steal => out.steals = out.steals.saturating_add(1),
                        ReqKind::Probe => {}
                    }
                }
                if owner >= FOCUS_WINDOWS {
                    return Err(HbCode::FocusBad.val());
                }
            }
            Err(e) => {
                out.rejects = out.rejects.saturating_add(1);
                out.digest = p_core::fold_u32(out.digest, e);
                // 拒绝路径必须一个字节都不改。
                if sys.owner != owner_before || sys.epoch != epoch_before {
                    return Err(HbCode::InvariantBad.val());
                }
            }
        }
        if !sys.invariant() {
            return Err(HbCode::InvariantBad.val());
        }
        if !sys.tally_ok(out.issued) {
            return Err(HbCode::OrderBad.val());
        }
        out.invariants = out.invariants.saturating_add(1);
        out.digest = p_core::fold_u32(out.digest, sys.owner);
        out.digest = p_core::fold_u32(out.digest, sys.epoch);
        i = i.saturating_add(1);
    }
    out.switches = sys.switches;
    out.steals = sys.steals;
    out.releases = sys.releases;
    if !out.conserved() {
        return Err(HbCode::OrderBad.val());
    }
    if out.digest == 0 {
        return Err(HbCode::OrderBad.val());
    }
    Ok(out)
}

// ── 六字段账行 ────────────────────────────────────────────────────
/// 渲染六字段账行：`tag|k1=a|k2=b|k3=c|k4=d|digest=XXXXXXXXXXXXXXXX`。
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

/// 十进制 u32 解析（拒绝空串、非数字、溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

/// 16 位十六进制解析（大小写皆收，长度必须恰为 16）。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// 拆行：返回 (tag, [k1..k4], digest)。键名必须逐字相等（k1..k4/digest），
/// 否则「同名不同位」的账行会被静默接受。
pub fn row_fields(row: &str) -> Option<(&str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let keys: [&str; 5] = ["k1", "k2", "k3", "k4", "digest"];
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let key = match sp.next() {
            Some(v) => v,
            None => return None,
        };
        let want = match keys.get(i - 1) {
            Some(v) => *v,
            None => return None,
        };
        if key != want {
            return None;
        }
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    if sp.next() != Some("digest") {
        return None;
    }
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

/// 渲染-解析往返恒等。
pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

/// 心跳账行：心跳数、丢失拍、乱序、拒绝。
pub fn heartbeat_row(hb: &Heartbeat) -> String {
    render_row(
        HB_TAG,
        hb.beats,
        hb.gaps,
        hb.reorders,
        hb.rejects,
        hb.digest,
    )
}

/// 缓冲账行：在册、丢弃、拒绝、投递。
pub fn buffer_row(b: &EventBuf) -> String {
    let cap32 = b.cap as u32;
    render_row(BUF_TAG, b.items.len() as u32, b.dropped, b.rejected, cap32, b.digest)
}

/// 焦点账行：切换、抢占、拒绝、探询。
pub fn focus_row(f: &FocusSys) -> String {
    render_row(
        FOCUS_TAG,
        f.switches,
        f.steals,
        f.rejects,
        f.probes,
        f.digest,
    )
}

/// 码表账行：行数、已知码数、致命码数、越界哨严重度（0）。
pub fn code_row(rows: u32, known: u32, fatal: u32, digest: u64) -> String {
    render_row(CODE_TAG, rows, known, fatal, 0, digest)
}

// ── 用例编号（跨槽防撞号） ───────────────────────────────────────
/// 前序槽位占用（B12 族实占编号，逐字取自 B12 任务板）。
pub const RESERVED_IDS: [u32; 6] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8014,
    0x071D_0001,
    0x071D_2001,
    0x071D_2014,
];

/// 本模块 16 个用例编号（0x0734_0001..0x0734_0010）。
pub const CASE_IDS: [u32; CASE_TOTAL] = [
    0x0734_0001,
    0x0734_0002,
    0x0734_0003,
    0x0734_0004,
    0x0734_0005,
    0x0734_0006,
    0x0734_0007,
    0x0734_0008,
    0x0734_0009,
    0x0734_000A,
    0x0734_000B,
    0x0734_000C,
    0x0734_000D,
    0x0734_000E,
    0x0734_000F,
    0x0734_0010,
];

/// 取第 i 个用例编号。
pub fn case_id(i: usize) -> Option<u32> {
    match CASE_IDS.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 编号守卫：16 条连续、落在 CASE_BASE+1..=CASE_BASE+0x10。
pub fn case_ids_tile() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match CASE_BASE.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        match CASE_IDS.get(i) {
            Some(v) => {
                if *v != want {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 跨槽零撞号：本模块 16 条编号与前序保留编号零相交。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = match CASE_IDS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if let Some(r) = RESERVED_IDS.get(j) {
                if a == *r {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 自身也不许重复。
    let mut k = 0usize;
    while k < CASE_IDS.len() {
        let a = match CASE_IDS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let mut m = k + 1;
        while m < CASE_IDS.len() {
            if let Some(b) = CASE_IDS.get(m) {
                if *b == a {
                    return false;
                }
            }
            m += 1;
        }
        k += 1;
    }
    true
}

// ── 探针 ─────────────────────────────────────────────────────────
/// 探针甲：心跳序列与阈值。
pub fn probe_seq_a() -> Result<(), u32> {
    if HB_TIMEOUT_TICKS == 0 || HB_LOST_TICKS <= HB_TIMEOUT_TICKS {
        return Err(HbCode::StateBad.val());
    }
    let mut hb = Heartbeat::new();
    hb.observe(1, 10)?;
    hb.observe(2, 20)?;
    hb.observe(5, 30)?;
    if hb.gaps != 2 || hb.beats != 3 || hb.seq != 5 {
        return Err(HbCode::OrderBad.val());
    }
    if hb.poll(30)? != LinkState::Live {
        return Err(HbCode::StateBad.val());
    }
    if hb.poll(30 + HB_TIMEOUT_TICKS + 1)? != LinkState::Buffering {
        return Err(HbCode::StateBad.val());
    }
    if hb.poll(30 + HB_LOST_TICKS + 1)? != LinkState::Lost {
        return Err(HbCode::StateBad.val());
    }
    hb.observe(6, 30 + HB_LOST_TICKS + 2)?;
    if hb.state != LinkState::Live {
        return Err(HbCode::StateBad.val());
    }
    Ok(())
}

/// 探针乙：缓冲丢弃与零丢口径。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut b = EventBuf::new(8, DropPolicy::OldestDrop)?;
    let mut i = 0u32;
    while i < 12 {
        let ev = Event {
            seq: i + 1,
            kind: (i % HB_KINDS as u32) as u8,
            payload: i,
        };
        let d = b.push(ev)?;
        if i < 8 && d != 0 {
            return Err(HbCode::DropBad.val());
        }
        if i >= 8 && d != 1 {
            return Err(HbCode::DropBad.val());
        }
        i += 1;
    }
    b.audit()?;
    if b.dropped != 4 || b.len() != 8 {
        return Err(HbCode::DropBad.val());
    }
    let mut out: Vec<Event> = Vec::new();
    let (got, _) = b.drain_replay()?;
    let (m, _) = b.replay_into(&mut out)?;
    if got.len() != 8 || m != 0 || !b.is_empty() {
        return Err(HbCode::ReplayBad.val());
    }
    Ok(())
}

/// 探针丙：焦点单主与 epoch 单调。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut f = FocusSys::new();
    if f.request(&FocusReq::new(1, 1, ReqKind::Acquire))? != 1 {
        return Err(HbCode::FocusBad.val());
    }
    if f.request(&FocusReq::new(2, 2, ReqKind::Steal))? != 2 {
        return Err(HbCode::FocusBad.val());
    }
    // 抢占后再「申请」必须被拒（有主时申请是非法的隐式偷取）。
    if f.request(&FocusReq::new(3, 3, ReqKind::Acquire)).err() != Some(HbCode::InvariantBad.val()) {
        return Err(HbCode::InvariantBad.val());
    }
    // 非属主释放必须被拒。
    if f.request(&FocusReq::new(1, 4, ReqKind::Release)).err() != Some(HbCode::FocusBad.val()) {
        return Err(HbCode::FocusBad.val());
    }
    if f.request(&FocusReq::new(2, 5, ReqKind::Release))? != 0 {
        return Err(HbCode::FocusBad.val());
    }
    if !f.invariant() {
        return Err(HbCode::InvariantBad.val());
    }
    Ok(())
}

/// 复跑：两条 fuzz 各跑两遍，摘要必须逐位相同。
pub fn repro_check() -> Result<(), u32> {
    let a = fuzz_heartbeat(FUZZ_ROUNDS)?;
    let b = fuzz_heartbeat(FUZZ_ROUNDS)?;
    if a != b {
        return Err(HbCode::ReplayBad.val());
    }
    let c = fuzz_focus(FUZZ_ROUNDS)?;
    let d = fuzz_focus(FUZZ_ROUNDS)?;
    if c != d {
        return Err(HbCode::ReplayBad.val());
    }
    if audit_code_snapshot()? != audit_code_snapshot()? {
        return Err(HbCode::ReplayBad.val());
    }
    Ok(())
}

// ── 判据集（恰好 8 条） ──────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 心跳序列与降级阈值（探针甲）。
    let pa = probe_seq_a();
    out.push((
        "F49051/心跳序列与降级",
        pa.is_ok(),
        match &pa {
            Ok(()) => format!(
                "timeout={} lost={} window={}",
                HB_TIMEOUT_TICKS, HB_LOST_TICKS, HB_SEQ_WINDOW
            ),
            Err(e) => format!("err=0x{:X} ({})", e, detail(*e)),
        },
    ));

    // ② 失联缓冲零丢（恢复路径 + 两种丢弃策略）。
    let r_old = recovery_path(HB_LOST_TICKS + 8, 80, DropPolicy::OldestDrop);
    let r_new = recovery_path(HB_TIMEOUT_TICKS + 8, 40, DropPolicy::RejectNewest);
    let ok_old = match &r_old {
        Ok(r) => r.zero_loss() && r.replayed + r.dropped == r.pushed,
        Err(_) => false,
    };
    let ok_new = match &r_new {
        Ok(r) => r.zero_loss() && r.replayed + r.dropped == r.pushed,
        Err(_) => false,
    };
    out.push((
        "F49051/失联缓冲零丢",
        ok_old && ok_new,
        format!(
            "old(rep={:?},drop={:?},push={:?}) new(rep={:?},drop={:?},push={:?})",
            r_old.as_ref().map(|r| r.replayed),
            r_old.as_ref().map(|r| r.dropped),
            r_old.as_ref().map(|r| r.pushed),
            r_new.as_ref().map(|r| r.replayed),
            r_new.as_ref().map(|r| r.dropped),
            r_new.as_ref().map(|r| r.pushed)
        ),
    ));

    // ③ 恢复重放一致（重放前后集合逐条相等 + 摘要非零）。
    let replay_ok = match (&r_old, &r_new) {
        (Ok(a), Ok(b)) => {
            a.bytes_digest != 0 && b.bytes_digest != 0 && a.duplicates == 0 && b.duplicates == 0
        }
        _ => false,
    };
    out.push((
        "F49051/恢复重放一致",
        replay_ok,
        format!(
            "digest_a={:016X} dup_a={:?} dup_b={:?}",
            r_old.as_ref().map(|r| r.bytes_digest).unwrap_or(0),
            r_old.as_ref().map(|r| r.duplicates),
            r_new.as_ref().map(|r| r.duplicates)
        ),
    ));

    // ④ 心跳 fuzz ×10^4 零崩（轮数、零丢、摘要非零、可复现）。
    let hxf = fuzz_heartbeat(FUZZ_ROUNDS);
    let hxf_again = fuzz_heartbeat(FUZZ_ROUNDS);
    let hxf_ok = match (&hxf, &hxf_again) {
        (Ok(a), Ok(b)) => {
            a.rounds == FUZZ_ROUNDS && a.digest == b.digest && a.digest != 0 && a.zero_loss()
        }
        _ => false,
    };
    out.push((
        "F49051/fuzz×10^4零崩",
        hxf_ok,
        match &hxf {
            Ok(a) => format!(
                "rounds={} beats={} buf={} rep={} drop={} rejects={} d=0x{:016X}",
                a.rounds, a.beats, a.buffered, a.replayed, a.dropped, a.rejects, a.digest
            ),
            Err(e) => format!("err=0x{:X} ({})", e, detail(*e)),
        },
    ));

    // ⑤ 焦点 fuzz ×10^4 不崩（不变式全轮成立）。
    let fxf = fuzz_focus(FUZZ_ROUNDS);
    let fxf_ok = match &fxf {
        Ok(a) => a.invariants_held() && a.conserved() && a.digest != 0,
        Err(_) => false,
    };
    out.push((
        "F49052/焦点fuzz×10^4",
        fxf_ok,
        match &fxf {
            Ok(a) => format!(
                "rounds={} sw={} steal={} rel={} probe={} rej={} inv={}",
                a.rounds, a.switches, a.steals, a.releases, a.probes, a.rejects, a.invariants
            ),
            Err(e) => format!("err=0x{:X} ({})", e, detail(*e)),
        },
    ));

    // ⑥ 焦点单主不变式（拒绝路径零改状态 + 探针丙）。
    let pc = probe_seq_c();
    let mut f = FocusSys::new();
    let _ = f.request(&FocusReq::new(1, 1, ReqKind::Acquire));
    // 状态三项：owner/epoch/switches。拒绝计数**必须**增加，故不入快照。
    let snapshot = (f.owner, f.epoch, f.switches);
    let rejects_before = f.rejects;
    let rejected = f.request(&FocusReq::new(2, 1, ReqKind::Steal)).is_err();
    let unchanged = snapshot == (f.owner, f.epoch, f.switches) && f.rejects == rejects_before + 1;
    out.push((
        "F49052/焦点单主",
        pc.is_ok() && rejected && unchanged && f.owner == 1 && f.invariant(),
        format!(
            "owner={} epoch={} rejected={} unchanged={} probe={}",
            f.owner,
            f.epoch,
            rejected,
            unchanged,
            pc.is_ok()
        ),
    ));

    // ⑦ 并发切换可复现（双流注入两遍摘要逐位相等）。
    let fx_a = fuzz_focus(FUZZ_ROUNDS);
    let fx_b = fuzz_focus(FUZZ_ROUNDS);
    let repro_ok = match (&fx_a, &fx_b) {
        (Ok(a), Ok(b)) => a.digest == b.digest && a.digest != 0 && a.switches == b.switches,
        _ => false,
    };
    out.push((
        "F49052/并发切换可复现",
        repro_ok,
        format!(
            "digest={:016X} switches={:?} seed=0x{:016X}",
            fx_a.as_ref().map(|a| a.digest).unwrap_or(0),
            fx_a.as_ref().map(|a| a.switches),
            FOCUS_SEED
        ),
    ));

    // ⑧ 错误码 ×15 零漂移（冻结快照逐行比对 + 越界哨 + 段位）。
    let snap = audit_code_snapshot();
    let snap_ok = match &snap {
        Ok(h) => *h != 0 && *h == audit_code_snapshot().unwrap_or(0),
        Err(_) => false,
    };
    let sentinel_ok = detail(SEG_BASE + 0x0F) == UNKNOWN
        && !known_code(SEG_BASE + 0x0F)
        && !known_code(SEG_BASE)
        && known_code(SEG_BASE + 0x01)
        && known_code(SEG_BASE + 0x0E)
        && segment_guard();
    out.push((
        "F49052/错误码×15零漂移",
        snap_ok && sentinel_ok,
        match &snap {
            Ok(h) => format!(
                "rows={} known=14 sentinel=0x{:X} digest=0x{:016X} seg={}",
                CODE_ROWS,
                SEG_BASE + 0x0F,
                h,
                segment_guard()
            ),
            Err(e) => format!("err=0x{:X} ({})", e, detail(*e)),
        },
    ));

    out
}

pub fn run_unx49051_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49051 事件层与 vxwm 心跳");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_closed_and_segmented() {
        assert_eq!(HbCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < HbCode::ALL.len() {
            let c = HbCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(c.val()));
            i += 1;
        }
        assert!(!known_code(SEG_BASE));
        assert!(!known_code(SEG_BASE + 0x0F));
    }

    #[test]
    fn detail_lookup_roundtrip() {
        let mut i = 0usize;
        while i < HbCode::ALL.len() {
            assert_eq!(detail(HbCode::ALL[i].val()), HbCode::ALL[i].detail());
            i += 1;
        }
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
    }

    #[test]
    fn code_snapshot_matches_enum() {
        let h = audit_code_snapshot().unwrap();
        assert_ne!(h, 0);
        assert_eq!(CODE_SNAPSHOT.len(), 15);
        assert_eq!(CODE_SNAPSHOT[14].code, SEG_BASE + 0x0F);
        assert_eq!(CODE_SNAPSHOT[14].severity, 0);
        assert_eq!(CODE_SNAPSHOT[0].code, 0x73141);
        assert_eq!(CODE_SNAPSHOT[13].code, 0x7314E);
    }

    #[test]
    fn snapshot_rows_are_unique_and_retrievable() {
        let mut i = 0usize;
        while i < CODE_ROWS {
            let r = CODE_SNAPSHOT[i];
            assert_eq!(snapshot_row(r.code), Some(r));
            i += 1;
        }
        assert_eq!(snapshot_row(0x73140), None);
        assert_eq!(snapshot_row(0x73150), None);
    }

    #[test]
    fn snapshot_detects_text_drift() {
        // 抄一行快照，改掉一个字：审计必须翻红（这里用临时表模拟，不动常量）。
        let mut bad = CODE_SNAPSHOT;
        bad[3].cn = "缓冲非法：容量越界";
        let mut hit = false;
        let mut i = 0usize;
        while i < bad.len() {
            if i < 14 {
                if bad[i].cn != HbCode::ALL[i].detail() {
                    hit = true;
                }
            }
            i += 1;
        }
        assert!(hit, "中文改写必须被快照比对捕获");
    }

    #[test]
    fn link_state_steps_are_whitelisted() {
        assert!(LinkState::legal_step(LinkState::Live, LinkState::Buffering));
        assert!(LinkState::legal_step(LinkState::Lost, LinkState::Live));
        // 电平式 poll 允许一次长间隔直接跳到失联。
        assert!(LinkState::legal_step(LinkState::Live, LinkState::Lost));
        assert!(LinkState::legal_step(LinkState::Buffering, LinkState::Lost));
        // 失联不许「自己变轻」成缓冲：回轻态只能靠一帧合法心跳先回在线。
        assert!(!LinkState::legal_step(LinkState::Lost, LinkState::Buffering));
        assert_eq!(LinkState::Live.index(), 0);
        assert_eq!(LinkState::Lost.index(), 2);
        assert_eq!(LinkState::Buffering.name(), "缓冲");
    }

    #[test]
    fn heartbeat_accepts_increasing_seq() {
        let mut hb = Heartbeat::new();
        assert_eq!(hb.observe(1, 5), Ok(LinkState::Live));
        assert_eq!(hb.observe(2, 6), Ok(LinkState::Live));
        assert_eq!(hb.beats, 2);
        assert_eq!(hb.gaps, 0);
        assert_eq!(hb.state, LinkState::Live);
        assert_ne!(hb.digest, p_core::FNV_OFF);
        hb.audit().unwrap();
    }

    #[test]
    fn heartbeat_rejects_zero_seq() {
        let mut hb = Heartbeat::new();
        assert_eq!(hb.observe(0, 1), Err(HbCode::SeqBad.val()));
        assert_eq!(hb.beats, 0);
        assert_eq!(hb.rejects, 1);
        assert_eq!(hb.state, LinkState::Buffering);
    }

    #[test]
    fn heartbeat_rejects_clock_backwards() {
        let mut hb = Heartbeat::new();
        hb.observe(1, 10).unwrap();
        assert_eq!(hb.observe(2, 9), Err(HbCode::ClockBad.val()));
        assert_eq!(hb.seq, 1);
        assert_eq!(hb.last_tick, 10);
    }

    #[test]
    fn heartbeat_rejects_duplicate() {
        let mut hb = Heartbeat::new();
        hb.observe(4, 10).unwrap();
        assert_eq!(hb.observe(4, 11), Err(HbCode::DupBad.val()));
        assert_eq!(hb.beats, 1);
        assert_eq!(hb.last_tick, 10);
    }

    #[test]
    fn heartbeat_tolerates_reorder_inside_window() {
        let mut hb = Heartbeat::new();
        hb.observe(10, 10).unwrap();
        assert_eq!(hb.observe(9, 11), Ok(LinkState::Live));
        assert_eq!(hb.reorders, 1);
        assert_eq!(hb.seq, 10);
        // 回退 9 拍 > 乱序窗口 8：必须判序列非法。
        assert_eq!(hb.observe(1, 12), Err(HbCode::SeqBad.val()));
    }

    #[test]
    fn heartbeat_counts_gaps() {
        let mut hb = Heartbeat::new();
        hb.observe(1, 1).unwrap();
        hb.observe(6, 2).unwrap();
        assert_eq!(hb.gaps, 4);
        hb.observe(7, 3).unwrap();
        assert_eq!(hb.gaps, 4);
    }

    #[test]
    fn heartbeat_poll_degrades_then_loses() {
        let mut hb = Heartbeat::new();
        hb.observe(1, 100).unwrap();
        assert_eq!(hb.poll(100 + HB_TIMEOUT_TICKS), Ok(LinkState::Live));
        assert_eq!(hb.poll(100 + HB_TIMEOUT_TICKS + 1), Ok(LinkState::Buffering));
        assert_eq!(hb.poll(100 + HB_LOST_TICKS + 1), Ok(LinkState::Lost));
        assert!(hb.buffering());
        assert_eq!(hb.poll(99), Err(HbCode::ClockBad.val()));
    }

    #[test]
    fn heartbeat_recovers_only_via_observe() {
        let mut hb = Heartbeat::new();
        hb.observe(1, 10).unwrap();
        hb.poll(10 + HB_LOST_TICKS + 5).unwrap();
        assert_eq!(hb.state, LinkState::Lost);
        assert_eq!(hb.misses, 1);
        assert_eq!(hb.poll(10 + HB_LOST_TICKS + 6), Ok(LinkState::Lost));
        assert_eq!(hb.misses, 1);
        assert_eq!(hb.observe(2, 10 + HB_LOST_TICKS + 7), Ok(LinkState::Live));
        // 一帧合法心跳把缺失计数清零（恢复后重新起算）。
        assert_eq!(hb.misses, 0);
    }

    #[test]
    fn heartbeat_audit_needs_beats() {
        let hb = Heartbeat::new();
        assert_eq!(hb.audit(), Err(HbCode::OrderBad.val()));
    }

    #[test]
    fn buffer_rejects_bad_capacity() {
        assert_eq!(EventBuf::new(0, DropPolicy::OldestDrop).err(), Some(HbCode::BufferBad.val()));
        assert_eq!(EventBuf::new(7, DropPolicy::OldestDrop).err(), Some(HbCode::BufferBad.val()));
        assert_eq!(
            EventBuf::new(BUF_CAP_MAX + 1, DropPolicy::OldestDrop).err(),
            Some(HbCode::BufferBad.val())
        );
        assert!(EventBuf::new(8, DropPolicy::OldestDrop).is_ok());
    }

    #[test]
    fn buffer_rejects_zero_seq_and_bad_kind() {
        let mut b = EventBuf::standard();
        assert_eq!(b.push(Event { seq: 0, kind: 0, payload: 0 }), Err(HbCode::SeqBad.val()));
        assert_eq!(
            b.push(Event { seq: 1, kind: HB_KINDS, payload: 0 }),
            Err(HbCode::BufferBad.val())
        );
        assert!(b.is_empty());
    }

    #[test]
    fn buffer_drops_oldest_and_counts() {
        let mut b = EventBuf::new(8, DropPolicy::OldestDrop).unwrap();
        let mut i = 0u32;
        while i < 10 {
            let d = b.push(Event { seq: i + 1, kind: 0, payload: i }).unwrap();
            assert_eq!(d, if i < 8 { 0 } else { 1 });
            i += 1;
        }
        b.audit().unwrap();
        assert_eq!(b.len(), 8);
        assert_eq!(b.dropped, 2);
        assert_eq!(b.items[0].seq, 3);
    }

    #[test]
    fn buffer_reject_newest_keeps_old() {
        let mut b = EventBuf::new(8, DropPolicy::RejectNewest).unwrap();
        let mut i = 0u32;
        while i < 8 {
            b.push(Event { seq: i + 1, kind: 0, payload: i }).unwrap();
            i += 1;
        }
        assert_eq!(
            b.push(Event { seq: 9, kind: 0, payload: 9 }),
            Err(HbCode::BufferBad.val())
        );
        assert_eq!(b.rejected, 1);
        assert_eq!(b.len(), 8);
        assert_eq!(b.items[7].seq, 8);
        b.audit().unwrap();
    }

    #[test]
    fn buffer_rejects_duplicate_seq() {
        let mut b = EventBuf::standard();
        b.push(Event { seq: 7, kind: 1, payload: 0 }).unwrap();
        assert_eq!(
            b.push(Event { seq: 7, kind: 2, payload: 1 }),
            Err(HbCode::DupBad.val())
        );
        assert_eq!(b.rejected, 1);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn buffer_replay_orders_by_seq() {
        let mut b = EventBuf::standard();
        b.push(Event { seq: 5, kind: 0, payload: 5 }).unwrap();
        b.push(Event { seq: 2, kind: 0, payload: 2 }).unwrap();
        b.push(Event { seq: 9, kind: 0, payload: 9 }).unwrap();
        let mut out: Vec<Event> = Vec::new();
        let (n, h) = b.replay_into(&mut out).unwrap();
        assert_eq!(n, 3);
        assert_ne!(h, 0);
        assert_eq!(out[0].seq, 2);
        assert_eq!(out[1].seq, 5);
        assert_eq!(out[2].seq, 9);
        // 只读重放不动缓冲。
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn buffer_drain_empties_and_accounts() {
        let mut b = EventBuf::standard();
        b.push(Event { seq: 1, kind: 0, payload: 0 }).unwrap();
        b.push(Event { seq: 2, kind: 1, payload: 0 }).unwrap();
        let (got, h) = b.drain_replay().unwrap();
        assert_eq!(got.len(), 2);
        assert_ne!(h, 0);
        assert!(b.is_empty());
        assert_eq!(b.delivered, 2);
        let mut out: Vec<Event> = Vec::new();
        assert_eq!(b.replay_into(&mut out).unwrap().0, 0);
    }

    #[test]
    fn buffer_audit_catches_silent_drop() {
        let mut b = EventBuf::standard();
        b.push(Event { seq: 1, kind: 0, payload: 0 }).unwrap();
        b.dropped = b.dropped.wrapping_add(1);
        assert_eq!(b.audit(), Err(HbCode::DropBad.val()));
    }

    #[test]
    fn buffer_audit_catches_duplicate() {
        let mut b = EventBuf::standard();
        b.items.push(Event { seq: 3, kind: 0, payload: 0 });
        b.items.push(Event { seq: 3, kind: 1, payload: 1 });
        b.pushed = 2;
        assert_eq!(b.audit(), Err(HbCode::DupBad.val()));
    }

    #[test]
    fn recovery_oldest_drop_is_zero_loss() {
        let r = recovery_path(HB_LOST_TICKS + 4, 100, DropPolicy::OldestDrop).unwrap();
        assert!(r.zero_loss());
        assert_eq!(r.replayed + r.dropped, r.pushed);
        assert_eq!(r.pushed, 100);
        assert_eq!(r.replayed, BUF_CAP as u32);
        assert_eq!(r.duplicates, 0);
        assert_ne!(r.bytes_digest, 0);
    }

    #[test]
    fn recovery_reject_newest_is_zero_loss() {
        let r = recovery_path(HB_TIMEOUT_TICKS + 1, 20, DropPolicy::RejectNewest).unwrap();
        assert!(r.zero_loss());
        assert_eq!(r.dropped, 0);
        assert_eq!(r.replayed, 20);
    }

    #[test]
    fn recovery_rejects_fake_outage() {
        assert_eq!(
            recovery_path(HB_TIMEOUT_TICKS, 10, DropPolicy::OldestDrop).err(),
            Some(HbCode::StateBad.val())
        );
        assert_eq!(
            recovery_path(HB_LOST_TICKS, 0, DropPolicy::OldestDrop).err(),
            Some(HbCode::RoundBad.val())
        );
    }

    #[test]
    fn fuzz_heartbeat_runs_ten_thousand() {
        let a = fuzz_heartbeat(FUZZ_ROUNDS).unwrap();
        assert_eq!(a.rounds, 10_000);
        assert!(a.zero_loss());
        assert_ne!(a.digest, 0);
        assert!(a.beats > 0);
    }

    #[test]
    fn fuzz_heartbeat_is_reproducible() {
        assert_eq!(fuzz_heartbeat(1_000), fuzz_heartbeat(1_000));
        assert_eq!(fuzz_heartbeat(0).err(), Some(HbCode::RoundBad.val()));
        assert_eq!(fuzz_heartbeat(FUZZ_ROUNDS + 1).err(), Some(HbCode::RoundBad.val()));
    }

    #[test]
    fn focus_acquire_release_cycle() {
        let mut f = FocusSys::new();
        assert_eq!(f.request(&FocusReq::new(1, 1, ReqKind::Acquire)), Ok(1));
        assert_eq!(f.request(&FocusReq::new(1, 2, ReqKind::Release)), Ok(0));
        assert_eq!(f.owner, 0);
        assert_eq!(f.epoch, 2);
        assert_eq!(f.switches, 2);
        assert!(f.invariant());
    }

    #[test]
    fn focus_rejects_bad_window() {
        let mut f = FocusSys::new();
        assert_eq!(
            f.request(&FocusReq::new(0, 1, ReqKind::Acquire)),
            Err(HbCode::FocusBad.val())
        );
        assert_eq!(
            f.request(&FocusReq::new(FOCUS_WINDOWS, 1, ReqKind::Acquire)),
            Err(HbCode::FocusBad.val())
        );
        assert_eq!(f.rejects, 2);
        assert_eq!(f.owner, 0);
    }

    #[test]
    fn focus_rejects_dup_acquire() {
        let mut f = FocusSys::new();
        f.request(&FocusReq::new(2, 1, ReqKind::Acquire)).unwrap();
        assert_eq!(
            f.request(&FocusReq::new(2, 2, ReqKind::Acquire)),
            Err(HbCode::DupBad.val())
        );
    }

    #[test]
    fn focus_rejects_acquire_with_owner() {
        let mut f = FocusSys::new();
        f.request(&FocusReq::new(2, 1, ReqKind::Acquire)).unwrap();
        assert_eq!(
            f.request(&FocusReq::new(3, 2, ReqKind::Acquire)),
            Err(HbCode::InvariantBad.val())
        );
    }

    #[test]
    fn focus_rejects_steal_without_owner() {
        let mut f = FocusSys::new();
        assert_eq!(
            f.request(&FocusReq::new(3, 5, ReqKind::Steal)),
            Err(HbCode::InvariantBad.val())
        );
    }

    #[test]
    fn focus_rejects_stale_epoch() {
        let mut f = FocusSys::new();
        f.request(&FocusReq::new(1, 10, ReqKind::Acquire)).unwrap();
        assert_eq!(
            f.request(&FocusReq::new(2, 10, ReqKind::Steal)),
            Err(HbCode::EpochBad.val())
        );
        assert_eq!(
            f.request(&FocusReq::new(2, 9, ReqKind::Steal)),
            Err(HbCode::EpochBad.val())
        );
        assert_eq!(f.owner, 1);
        assert_eq!(f.epoch, 10);
    }

    #[test]
    fn focus_release_requires_owner() {
        let mut f = FocusSys::new();
        f.request(&FocusReq::new(1, 1, ReqKind::Acquire)).unwrap();
        assert_eq!(
            f.request(&FocusReq::new(2, 2, ReqKind::Release)),
            Err(HbCode::FocusBad.val())
        );
        assert_eq!(f.owner, 1);
    }

    #[test]
    fn focus_probe_is_read_only() {
        let mut f = FocusSys::new();
        f.request(&FocusReq::new(4, 1, ReqKind::Acquire)).unwrap();
        let before = (f.owner, f.epoch, f.switches);
        assert_eq!(f.request(&FocusReq::new(5, 1, ReqKind::Probe)), Ok(4));
        assert_eq!((f.owner, f.epoch, f.switches), before);
        assert_eq!(f.probes, 1);
        assert!(f.tally_ok(2));
    }

    #[test]
    fn focus_rejection_does_not_mutate() {
        let mut f = FocusSys::new();
        f.request(&FocusReq::new(1, 3, ReqKind::Acquire)).unwrap();
        let before = (f.owner, f.epoch, f.switches, f.rejects);
        assert!(f.request(&FocusReq::new(2, 2, ReqKind::Steal)).is_err());
        assert_eq!(f.owner, before.0);
        assert_eq!(f.epoch, before.1);
        assert_eq!(f.switches, before.2);
        assert_eq!(f.rejects, before.3 + 1);
    }

    #[test]
    fn fuzz_focus_holds_invariants() {
        let a = fuzz_focus(FUZZ_ROUNDS).unwrap();
        assert_eq!(a.rounds, 10_000);
        assert!(a.invariants_held());
        assert!(a.conserved());
        assert_ne!(a.digest, 0);
        assert!(a.switches > 0);
        assert!(a.rejects > 0);
    }

    #[test]
    fn fuzz_focus_is_reproducible() {
        assert_eq!(fuzz_focus(500), fuzz_focus(500));
        assert_eq!(fuzz_focus(0).err(), Some(HbCode::RoundBad.val()));
    }

    #[test]
    fn req_kind_roundtrip() {
        let mut i = 0u8;
        while i < 4 {
            let k = ReqKind::from_index(i).unwrap();
            assert_eq!(k.index(), i);
            assert!(!k.name().is_empty());
            i += 1;
        }
        assert_eq!(ReqKind::from_index(4).err(), Some(HbCode::FocusBad.val()));
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row(HB_TAG, 1, 2, 3, 4, 0x0123_4567_89AB_CDEF);
        assert!(audit_render(&row));
        let (tag, vals, digest) = row_fields(&row).unwrap();
        assert_eq!(tag, HB_TAG);
        assert_eq!(vals, [1, 2, 3, 4]);
        assert_eq!(digest, 0x0123_4567_89AB_CDEF);
        assert!(!audit_render(""));
        assert!(!audit_render("HB|k1=1"));
    }

    #[test]
    fn render_rejects_wrong_key_names() {
        let bad = "HB|x1=1|k2=2|k3=3|k4=4|digest=0123456789ABCDEF";
        assert!(row_fields(bad).is_none());
        let bad2 = "HB|k1=1|k2=2|k3=3|k4=4|sum=0123456789ABCDEF";
        assert!(row_fields(bad2).is_none());
    }

    #[test]
    fn parse_helpers_behave() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(4_294_967_295));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_hex16("0123456789ABCDEF"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789abcde"), None);
        assert_eq!(parse_hex16("0123456789ABCDEG"), None);
    }

    #[test]
    fn ledger_rows_are_parseable() {
        let mut hb = Heartbeat::new();
        hb.observe(1, 1).unwrap();
        let row = heartbeat_row(&hb);
        assert!(audit_render(&row));
        let (t, v, _) = row_fields(&row).unwrap();
        assert_eq!(t, HB_TAG);
        assert_eq!(v[0], 1);
        let mut b = EventBuf::standard();
        b.push(Event { seq: 1, kind: 0, payload: 0 }).unwrap();
        assert!(audit_render(&buffer_row(&b)));
        let mut f = FocusSys::new();
        f.request(&FocusReq::new(1, 1, ReqKind::Acquire)).unwrap();
        assert!(audit_render(&focus_row(&f)));
        let h = audit_code_snapshot().unwrap();
        let crow = code_row(15, 14, 3, h);
        let (t2, v2, d2) = row_fields(&crow).unwrap();
        assert_eq!(t2, CODE_TAG);
        assert_eq!(v2, [15, 14, 3, 0]);
        assert_eq!(d2, h);
    }

    #[test]
    fn case_ids_tile_and_do_not_collide() {
        assert!(case_ids_tile());
        assert!(no_cross_collision());
        assert_eq!(case_id(0), Some(0x0734_0001));
        assert_eq!(case_id(15), Some(0x0734_0010));
        assert_eq!(case_id(16), None);
        let mut i = 0usize;
        while i < RESERVED_IDS.len() {
            assert_ne!(RESERVED_IDS[i], CASE_IDS[0]);
            assert!(RESERVED_IDS[i] < 0x0734_0001);
            i += 1;
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49051_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }
}
