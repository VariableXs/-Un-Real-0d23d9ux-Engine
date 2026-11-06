//! UNX-F48991 手势桥接 · 0x71780
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族第六槽）：
//!   UNX-F48991 手势误触防御（与 palm rejection 协同——B06 联签引用不二写）:
//!          ① 纯功能代码 ≥280 行；
//!          ② UNX-F48991-J1 协同 ×10^2 误触 0。
//!   UNX-F48992 手势观测埋点（起止/迁移/取消四点全覆盖）:
//!          ① 纯功能代码 ≥260 行；
//!          ② UNX-F48992-J1 四点 100% 覆盖。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（每拍 raw 事件按此顺序过闸，任一处失败即整拍拒绝并计入丢弃）：
//!   ① 时钟（单调不减 + 上界）→ ② 坐标 / 会话 / 设备绑定 → ③ 相位合法性
//!   → ④ 相位×事件查表 → ⑤ 计数与几何（会话号、序号、帧容量）→ ⑥ 终判。
//!
//! 为什么时钟先于坐标：时钟是**序列的坐标系**。一条事件哪怕坐标完全合法，
//! 只要时间戳回退，它就不是「这一个手势里的下一拍」，而是上游缓冲乱序或重放。
//! 先查时钟，回退立刻被标成本模块最贵的错误（TimestampBad）；若先查坐标，
//! 一个脏坐标会把同一条乱序事件误诊成坐标问题，排障方向从「时钟/缓冲坏了」
//! 被带到「驱动坐标系变了」。
//!
//! 为什么相位先于表：相位合法性（Closed 一律拒、Flushing 一律拒）是关于
//! 「这条桥还活着吗」的三行谓词；相位×事件表是关于「这个 (相位, 事件) 组合
//! 该走哪一步」的 16 格数据。若先查表，桥已关闭时 Closed 行的拒绝格会报出与
//! 「事件乱序」同一个 OrderBad，把「桥已关」这条完全不同的根因掩埋进表里。
//! 先过相位谓词，Closed/Flushing 各有专属码，表就只剩 Idle/Tracking 的语义问题。
//!
//! 为什么查表先于计数：表回答「这一步合法吗」，计数回答「这一步还记不记得下」。
//! 若先记 raw/frames 再查表，被表拒绝的拍会先污染账本再被回滚——本模块刻意不做
//! 回滚（no_std、无事务），所以顺序必须是「先判合法、再动账」。
//!
//! 关键差异：桥接层只回答两件事——① raw 事件流进、手势帧流出的**透传无损**
//! （坏事件必须拒、批处理边界必须保序保数）；② 设备热切换时的**会话终结**
//! （Cancel 必须发出、账必须闭合、重复/冲突接管必须拒）。不做手势分类
//! （B10-CORE：这里只发 Down/Move/Up/Cancel 四类抽象事件）、不做手势→动作
//! 映射（B10-DISPATCH）、不做接触槽位管理（B10-GUARD）。
//! 会话号是**桥接身份**而非手势身份：同一手势被热切换切成两段时，旧会话以
//! Cancel 收尾，新会话另起号码，绝不复活旧号。
//!
//! 在桥接层之上，本模块按任务板第六槽叠加两张卡的真实功能：
//!   · F48991 掌拒联签层：**只消费** B06 的掌拒标记/分数与边缘接近度（联签
//!     引用不二写，不重算掌拒检测器），对「B06 已标记 + 联签达阈 + 与在途手势
//!     质心不相关」的触摸团拒绝；与在途手势相关的续拍永远放行。100 个确定性
//!     场景要求掌团全拒、真手势全过，误触计数恒 0。
//!   · F48992 四点埋点层：起（Start）/迁移（Transition）/取消（Cancel）/止
//!     （End）四类事件逐条入账并折叠摘要；覆盖账强制四类齐备（100%），任一类
//!     占比 <10% 的失衡账必须被拒，不允许「只记了迁移点也算覆盖」。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71780，B10 族第六槽）。
pub const SEG_BASE: u32 = 0x71780;

/// 桥接相位总数 / 原始事件类数 / 序列分类数。
pub const PHASES: usize = 4;
pub const EVENTS: usize = 4;
pub const CLASSES: usize = 4;
/// 各探针轮数（卡面 ×10^3）。
pub const BRIDGE_ROUNDS: u32 = 1_000;
pub const SESSION_ROUNDS: u32 = 1_000;
pub const BATCH_ROUNDS: u32 = 1_000;
pub const HANDOFF_ROUNDS: u32 = 1_000;
pub const FUZZ_ROUNDS: u32 = 1_000;
/// 注入路数 / 账本渲染字段数 / 黄金轮数。
pub const INJECT_ROUTES: usize = 4;
pub const STATS_FIELDS: usize = 6;
pub const GOLDEN_ROUNDS: u32 = 32;
/// 时钟上界 / 当量坐标上界 / 帧容量 / 会话号上界。
pub const TICK_CAP: u32 = 1_000_000;
pub const COORD_MAX: u32 = 4_000;
pub const FRAME_CAP: u32 = 200_000;
pub const SESSION_CAP: u32 = 1_000_000;
/// 单桥设备册容量 / 批处理脚本拍数。
pub const MAX_DEVICES: usize = 8;
pub const SCRIPT_LEN: usize = 8;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrCode {
    /// 桥接相位不放行 / 双子 Begin / 坐标越界 / 相位×事件表缺格
    BridgeBad,
    /// 会话号溢出或复用
    SessionBad,
    /// 事件序号回退或有洞
    SeqBad,
    /// 热切换后有悬挂会话
    LeakBad,
    /// 时间戳回退或越界
    TimestampBad,
    /// 设备标识非法 / 重复接管 / 双设备冲突
    DeviceBad,
    /// 接管账不闭合
    HandoffBad,
    /// 事件次序非法
    OrderBad,
    /// 三账不守恒
    LedgerBad,
    /// 渲染不可解析
    RenderBad,
    /// 复跑不一致
    ReplayBad,
    /// 覆盖不足
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// fuzz 与影子模型不一致
    FuzzBad,
}

impl BrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            BrCode::BridgeBad => "桥接相位不放行、双子 Begin 或坐标越界",
            BrCode::SessionBad => "会话号溢出、回绕或在途会话缺失",
            BrCode::SeqBad => "事件序号回退、重复或有洞",
            BrCode::LeakBad => "热切换后仍有悬挂会话未终结",
            BrCode::TimestampBad => "时间戳回退或越过上界",
            BrCode::DeviceBad => "设备标识非法、重复接管或双设备冲突",
            BrCode::HandoffBad => "接管账不闭合（旧设备未终结就让位）",
            BrCode::OrderBad => "事件次序非法（Move/End/Cancel 先于 Begin）",
            BrCode::LedgerBad => "raw/帧/丢弃与开/关/在途三账不守恒",
            BrCode::RenderBad => "账本文本字段数或数值不可解析",
            BrCode::ReplayBad => "两次确定性复跑结果不一致",
            BrCode::CoverBad => "覆盖轮数为零或越过上限",
            BrCode::SegmentBad => "诊断码越出本模块段窗",
            BrCode::FuzzBad => "fuzz 结果与独立影子模型不一致",
        }
    }

    pub const ALL: [BrCode; 14] = [
        Self::BridgeBad, Self::SessionBad, Self::SeqBad, Self::LeakBad, Self::TimestampBad,
        Self::DeviceBad, Self::HandoffBad, Self::OrderBad, Self::LedgerBad, Self::RenderBad,
        Self::ReplayBad, Self::CoverBad, Self::SegmentBad, Self::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        if let Some(c) = BrCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

/// 段窗守卫：14 个诊断码必须落在 (SEG_BASE, SEG_BASE + 0x0F]。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        if let Some(c) = BrCode::ALL.get(i) {
            let v = c.val();
            if v <= SEG_BASE || v > SEG_BASE + 0x0F {
                return false;
            }
        }
        i += 1;
    }
    true
}

pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        let mut j = i + 1;
        while j < BrCode::ALL.len() {
            match (BrCode::ALL.get(i), BrCode::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 相位 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 无在途会话，只接受 Begin。
    Idle,
    /// 在途会话，接受 Move/End/Cancel，拒绝第二个 Begin。
    Tracking,
    /// 批处理窗口：事件保留顺序语义但本窗口不受理（受理会让「flush 不改
    /// 顺序与计数」这条判据失去可观测性）。
    Flushing,
    /// 桥接终态，任何事件一律拒，不可复活。
    Closed,
}

impl Phase {
    pub fn index(self) -> usize {
        match self {
            Phase::Idle => 0,
            Phase::Tracking => 1,
            Phase::Flushing => 2,
            Phase::Closed => 3,
        }
    }

    pub fn from_index(i: u8) -> Option<Phase> {
        match i {
            0 => Some(Phase::Idle),
            1 => Some(Phase::Tracking),
            2 => Some(Phase::Flushing),
            3 => Some(Phase::Closed),
            _ => None,
        }
    }

    pub const ALL: [Phase; PHASES] = [Phase::Idle, Phase::Tracking, Phase::Flushing, Phase::Closed];
}

/// 相位预筛（第③闸）：只回答「这一相位还接不接事件」。
///
/// Closed 与 Flushing 的码刻意不同：Closed 是桥接生命的结束（不可恢复 →
/// BridgeBad），Flushing 是桥主动关窗（可恢复，事件该等 → OrderBad）。
pub fn phase_prefilter(phase: Phase) -> Result<(), u32> {
    match phase {
        Phase::Closed => Err(BrCode::BridgeBad.val()),
        Phase::Flushing => Err(BrCode::OrderBad.val()),
        Phase::Idle | Phase::Tracking => Ok(()),
    }
}

// ── 原始事件 / 手势帧 ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RawKind {
    Begin,
    Move,
    End,
    Cancel,
}

impl RawKind {
    pub fn index(self) -> usize {
        match self {
            RawKind::Begin => 0,
            RawKind::Move => 1,
            RawKind::End => 2,
            RawKind::Cancel => 3,
        }
    }

    pub fn from_index(i: u8) -> Option<RawKind> {
        match i {
            0 => Some(RawKind::Begin),
            1 => Some(RawKind::Move),
            2 => Some(RawKind::End),
            3 => Some(RawKind::Cancel),
            _ => None,
        }
    }

    pub const ALL: [RawKind; EVENTS] = [Self::Begin, Self::Move, Self::End, Self::Cancel];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RawEvent {
    pub kind: RawKind,
    pub tick: u32,
    pub x: u32,
    pub y: u32,
    pub device: u32,
}

impl RawEvent {
    pub fn new(kind: RawKind, tick: u32, x: u32, y: u32, device: u32) -> RawEvent {
        RawEvent { kind, tick, x, y, device }
    }
    pub fn begin(tick: u32, x: u32, y: u32, device: u32) -> RawEvent {
        RawEvent::new(RawKind::Begin, tick, x, y, device)
    }
    pub fn mv(tick: u32, x: u32, y: u32, device: u32) -> RawEvent {
        RawEvent::new(RawKind::Move, tick, x, y, device)
    }
    pub fn end(tick: u32, x: u32, y: u32, device: u32) -> RawEvent {
        RawEvent::new(RawKind::End, tick, x, y, device)
    }
    pub fn cancel(tick: u32, x: u32, y: u32, device: u32) -> RawEvent {
        RawEvent::new(RawKind::Cancel, tick, x, y, device)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameKind {
    Down,
    Move,
    Up,
    Cancel,
}

impl FrameKind {
    pub fn code(self) -> u32 {
        match self {
            FrameKind::Down => 0,
            FrameKind::Move => 1,
            FrameKind::Up => 2,
            FrameKind::Cancel => 3,
        }
    }
}

/// 桥接输出帧：抽象事件 + 会话号 + 全局序号 + 单调时间戳 + 设备归属。
///
/// `seq` 是全桥唯一的帧序号（跨设备、跨会话、跨批处理边界连续），是「事件
/// 序号跨设备单调」的唯一载体；`session` 只在一次手势内稳定，热切换强制换号。
/// 两者刻意分开：seq 单调证明**没有丢帧**，session 单调证明**没有复用身份**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame {
    pub session: u32,
    pub seq: u32,
    pub tick: u32,
    pub kind: FrameKind,
    pub device: u32,
    pub x: u32,
    pub y: u32,
}

/// 表动作：这一步该发哪一帧（None 只出现在拒绝格）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Emit {
    None,
    Down,
    Move,
    Up,
    Cancel,
}

impl Emit {
    pub fn kind(self) -> Option<FrameKind> {
        match self {
            Emit::None => None,
            Emit::Down => Some(FrameKind::Down),
            Emit::Move => Some(FrameKind::Move),
            Emit::Up => Some(FrameKind::Up),
            Emit::Cancel => Some(FrameKind::Cancel),
        }
    }
}

/// 相位×事件表的一格：要么放行（next + emit），要么拒绝（deny，next 不动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub next: Phase,
    pub emit: Emit,
    pub deny: Option<BrCode>,
}

const fn cell(next: Phase, emit: Emit) -> Cell {
    Cell { next, emit, deny: None }
}

const fn deny(code: BrCode) -> Cell {
    Cell { next: Phase::Idle, emit: Emit::None, deny: Some(code) }
}

/// 相位×事件表（4×4 = 16 格，**总函数**：每一格都有定义）。
///
/// 放行格恰好四格：Idle×Begin→Tracking+Down、Tracking×Move→Tracking+Move、
/// Tracking×End→Idle+Up、Tracking×Cancel→Idle+Cancel。其余十二格全拒，拒绝码
/// 按「谁的错」分配：Idle 下的 Move/End/Cancel → OrderBad（Move-before-Begin
/// 家族）；Tracking 下的第二个 Begin → BridgeBad（不许静默开局）；Flushing 全拒
/// → OrderBad（窗口期，可恢复）；Closed 全拒 → BridgeBad（桥已死，不可恢复）。
pub const TABLE: [[Cell; EVENTS]; PHASES] = [
    // Idle
    [
        cell(Phase::Tracking, Emit::Down),
        deny(BrCode::OrderBad),
        deny(BrCode::OrderBad),
        deny(BrCode::OrderBad),
    ],
    // Tracking
    [
        deny(BrCode::BridgeBad),
        cell(Phase::Tracking, Emit::Move),
        cell(Phase::Idle, Emit::Up),
        cell(Phase::Idle, Emit::Cancel),
    ],
    // Flushing
    [
        deny(BrCode::OrderBad),
        deny(BrCode::OrderBad),
        deny(BrCode::OrderBad),
        deny(BrCode::OrderBad),
    ],
    // Closed
    [
        deny(BrCode::BridgeBad),
        deny(BrCode::BridgeBad),
        deny(BrCode::BridgeBad),
        deny(BrCode::BridgeBad),
    ],
];

/// 第④闸：查表。表缺格报 BridgeBad（表坏了，不是事件坏了）。
pub fn transition(phase: Phase, kind: RawKind) -> Result<Cell, u32> {
    let row = TABLE.get(phase.index()).ok_or(BrCode::BridgeBad.val())?;
    let c = *row.get(kind.index()).ok_or(BrCode::BridgeBad.val())?;
    match c.deny {
        Some(code) => Err(code.val()),
        None => Ok(c),
    }
}

/// 表审计：16 格必须「要么放行要么拒绝」，放行格恰为上述四格且后继/帧类正确。
///
/// 表是本模块的中心数据：它坏了任何下游判据都不再有意义，所以审计比任何
/// 探针都靠前。返回格数（16）。
pub fn audit_table() -> Result<u32, u32> {
    let mut total = 0u32;
    let mut allow = 0u32;
    let mut p = 0usize;
    while p < PHASES {
        let phase = Phase::from_index(p as u8).ok_or(BrCode::BridgeBad.val())?;
        let mut k = 0usize;
        while k < EVENTS {
            let kind = RawKind::from_index(k as u8).ok_or(BrCode::BridgeBad.val())?;
            let row = TABLE.get(p).ok_or(BrCode::BridgeBad.val())?;
            let c = *row.get(k).ok_or(BrCode::BridgeBad.val())?;
            total = total.saturating_add(1);
            match (c.deny, c.emit) {
                // 既放行又拒绝：格子自相矛盾。
                (Some(_), Emit::Down)
                | (Some(_), Emit::Move)
                | (Some(_), Emit::Up)
                | (Some(_), Emit::Cancel) => return Err(BrCode::BridgeBad.val()),
                // 既不放行也不拒绝：格子被挖空（非总函数）。
                (None, Emit::None) => return Err(BrCode::BridgeBad.val()),
                // 放行格：逐个钉死后继与帧类。
                (None, _) => {
                    allow = allow.saturating_add(1);
                    let fk = c.emit.kind().ok_or(BrCode::BridgeBad.val())?;
                    let ok = match (phase, kind) {
                        (Phase::Idle, RawKind::Begin) => {
                            c.next == Phase::Tracking && fk == FrameKind::Down
                        }
                        (Phase::Tracking, RawKind::Move) => {
                            c.next == Phase::Tracking && fk == FrameKind::Move
                        }
                        (Phase::Tracking, RawKind::End) => {
                            c.next == Phase::Idle && fk == FrameKind::Up
                        }
                        (Phase::Tracking, RawKind::Cancel) => {
                            c.next == Phase::Idle && fk == FrameKind::Cancel
                        }
                        // Flushing/Closed、Idle 下非 Begin、Tracking 下第二个 Begin
                        // 都不许放行。
                        (_, _) => false,
                    };
                    if !ok {
                        return Err(BrCode::BridgeBad.val());
                    }
                }
                (Some(_), Emit::None) => {}
            }
            k += 1;
        }
        p += 1;
    }
    if total != (PHASES * EVENTS) as u32 || allow != 4 {
        return Err(BrCode::BridgeBad.val());
    }
    Ok(total)
}

// ── 设备标识 ──────────────────────────────────────────────────────
/// 设备标识：`id` 是驱动侧稳定号，`name` 是人类可读名。
///
/// 两个字段都参与冲突守卫：同 id 异名 = 驱动把两个物理设备报成一个号；
/// 同名异 id = 同一设备重连后换了号。两种都会让接管账的「谁终结了谁」
/// 无法回答，所以一律 DeviceBad——宁可拒绝也不猜。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Device {
    pub id: u32,
    pub name: &'static str,
}

impl Device {
    pub const fn new(id: u32, name: &'static str) -> Device {
        Device { id, name }
    }
    pub fn valid(&self) -> bool {
        self.id != 0 && !self.name.is_empty()
    }
}

pub const DEV_A: Device = Device::new(1, "touch-pad-a");
pub const DEV_B: Device = Device::new(2, "touch-pad-b");
pub const DEV_C: Device = Device::new(3, "touch-pad-c");
pub const DEV_D: Device = Device::new(4, "touch-pad-d");
/// 热切换设备池（四设备轮转，保证相邻两次接管必换号）。
pub const POOL: [Device; 4] = [DEV_A, DEV_B, DEV_C, DEV_D];

// ── 账本 ──────────────────────────────────────────────────────────
/// 渲染账（6 字段，末字段为 64 位摘要）。
///
/// 字段不是随手取的：raw/frames/dropped 承担透传守恒，handoffs/terminated 承担
/// 热切换终结，digest 承担复跑一致。守恒式是 **raw + terminated == frames +
/// dropped**：热切换合成的 Cancel 帧没有对应 raw 事件，它是由 terminated 生出来
/// 的一帧，因此在等式左边补上 terminated。这条等式只用渲染出来的 6 个字段就能
/// 复核，所以 `audit_render` 不必回到桥内部。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BridgeStats {
    pub raw: u32,
    pub frames: u32,
    pub dropped: u32,
    pub handoffs: u32,
    pub terminated: u32,
    pub digest: u64,
}

impl BridgeStats {
    pub fn conserved(&self) -> bool {
        self.raw.saturating_add(self.terminated) == self.frames.saturating_add(self.dropped)
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.frames == 0 {
            return Err(BrCode::LedgerBad.val());
        }
        if !self.conserved() {
            return Err(BrCode::LedgerBad.val());
        }
        if self.frames < self.terminated {
            return Err(BrCode::HandoffBad.val());
        }
        if self.digest == 0 {
            return Err(BrCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 会话账：打开 / 关闭 / 在途 + 接管 / 终结 / 拒接管。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub opened: u32,
    pub closed: u32,
    pub active: u32,
    pub handoffs: u32,
    pub terminated: u32,
    pub rejected: u32,
}

impl Ledger {
    /// 守恒：opened == closed + active，且在途至多一个。
    pub fn conserved(&self) -> bool {
        self.active <= 1 && self.opened == self.closed.saturating_add(self.active)
    }
    /// 终结账是关闭账的子集。
    pub fn terminated_within_closed(&self) -> bool {
        self.terminated <= self.closed
    }
}

// ── 桥接状态机 ────────────────────────────────────────────────────
/// raw 事件 → 手势帧 的桥接状态机。
///
/// 所有写账都集中在 `commit` 一个点上：出帧、改相、计数、折摘要四件事要么
/// 一起发生要么都不发生。分散写账是这类模块最典型的账目漂移来源。
pub struct Bridge {
    phase: Phase,
    resume: Phase,
    tick: u32,
    session: u32,
    next_session: u32,
    last_session: u32,
    seq: u32,
    raw: u32,
    frames: u32,
    dropped: u32,
    downs: u32,
    moves: u32,
    closes: u32,
    opened: u32,
    closed: u32,
    terminated: u32,
    handoffs: u32,
    rejected: u32,
    batches: u32,
    detaches: u32,
    digest: u64,
    last_x: u32,
    last_y: u32,
    current: Option<Device>,
    devices: [Option<Device>; MAX_DEVICES],
    ndev: usize,
    last: Option<Frame>,
}

impl Bridge {
    pub fn new() -> Bridge {
        Bridge {
            phase: Phase::Idle, resume: Phase::Idle, tick: 0, session: 0,
            next_session: 1, last_session: 0, seq: 0,
            raw: 0, frames: 0, dropped: 0, downs: 0, moves: 0, closes: 0,
            opened: 0, closed: 0, terminated: 0, handoffs: 0, rejected: 0,
            batches: 0, detaches: 0, digest: p_core::FNV_OFF,
            last_x: 0, last_y: 0, current: None,
            devices: [None; MAX_DEVICES], ndev: 0, last: None,
        }
    }

    pub fn phase(&self) -> Phase { self.phase }
    pub fn tick(&self) -> u32 { self.tick }
    pub fn session(&self) -> u32 { self.session }
    pub fn seq(&self) -> u32 { self.seq }
    pub fn current(&self) -> Option<Device> { self.current }
    pub fn last_frame(&self) -> Option<Frame> { self.last }
    pub fn batches(&self) -> u32 { self.batches }
    pub fn detaches(&self) -> u32 { self.detaches }
    pub fn device_count(&self) -> usize { self.ndev }
    pub fn digest(&self) -> u64 { self.digest }
    /// 在途会话数（0/1，由开/关两账导出，不另设计数器）。
    pub fn active(&self) -> u32 {
        self.opened.saturating_sub(self.closed)
    }

    pub fn stats(&self) -> BridgeStats {
        BridgeStats {
            raw: self.raw, frames: self.frames, dropped: self.dropped,
            handoffs: self.handoffs, terminated: self.terminated, digest: self.digest,
        }
    }

    pub fn ledger(&self) -> Ledger {
        Ledger {
            opened: self.opened, closed: self.closed, active: self.active(),
            handoffs: self.handoffs, terminated: self.terminated, rejected: self.rejected,
        }
    }

    /// 拒收一拍 raw 事件：丢弃 +1、折摘要。
    ///
    /// 丢弃**只在这里**增加，因此「每一拍恰好被记一次」是结构性保证，不依赖
    /// 调用方记忆——这正是 raw + terminated == frames + dropped 能逐拍成立的原因。
    fn refuse_raw(&mut self, code: u32) -> Result<Frame, u32> {
        self.dropped = self.dropped.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    /// 拒收一次接管：接管拒账 +1。与 raw 丢弃分开记账，否则「拒绝接管」会
    /// 污染透传守恒（接管不是 raw 事件）。
    fn refuse_attach(&mut self, code: u32) -> Result<(), u32> {
        self.rejected = self.rejected.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    /// 会话号分配：严格单调、永不回绕、永不复用。
    fn alloc_session(&mut self) -> Result<u32, u32> {
        if self.next_session == 0 || self.next_session > SESSION_CAP {
            return Err(BrCode::SessionBad.val());
        }
        let s = self.next_session;
        if s <= self.last_session { return Err(BrCode::SessionBad.val()); }
        self.next_session = s.saturating_add(1);
        self.last_session = s;
        Ok(s)
    }

    /// 单点写账：出帧 + 计数 + 摘要，三件事不分家。
    fn commit(&mut self, kind: FrameKind, tick: u32, x: u32, y: u32, device: u32)
        -> Result<Frame, u32>
    {
        if self.frames >= FRAME_CAP { return self.refuse_raw(BrCode::SeqBad.val()); }
        let seq = match self.seq.checked_add(1) {
            Some(v) => v,
            None => return self.refuse_raw(BrCode::SeqBad.val()),
        };
        let session = if kind == FrameKind::Down {
            match self.alloc_session() {
                Ok(s) => s,
                Err(code) => return self.refuse_raw(code),
            }
        } else {
            // 没有在途会话却要出 Move/Up/Cancel：会话账已经坏了。
            if self.session == 0 { return self.refuse_raw(BrCode::SessionBad.val()); }
            self.session
        };
        let f = Frame { session, seq, tick, kind, device, x, y };
        self.seq = seq;
        self.frames = self.frames.saturating_add(1);
        match kind {
            FrameKind::Down => {
                self.downs = self.downs.saturating_add(1);
                self.opened = self.opened.saturating_add(1);
            }
            FrameKind::Move => self.moves = self.moves.saturating_add(1),
            FrameKind::Up | FrameKind::Cancel => {
                self.closes = self.closes.saturating_add(1);
                self.closed = self.closed.saturating_add(1);
            }
        }
        self.session = if kind == FrameKind::Up || kind == FrameKind::Cancel { 0 } else { session };
        self.tick = tick;
        self.last_x = x;
        self.last_y = y;
        self.last = Some(f);
        self.digest = p_core::fold_u32(self.digest, kind.code());
        self.digest = p_core::fold_u32(self.digest, session);
        self.digest = p_core::fold_u32(self.digest, seq);
        Ok(f)
    }

    /// 以 Cancel 终结在途会话（热切换 / 断开 / 关闭三条路径共用）。
    ///
    /// 顺序不可颠倒：先出 Cancel 帧（挂在**旧设备**名下），再改相、再让位。
    /// 若先换设备，Cancel 帧会带上新设备 id，接管账里「谁终结了谁」就假了。
    fn terminate_session(&mut self, old: Device) -> Result<(), u32> {
        let tick = self.tick;
        self.commit(FrameKind::Cancel, tick, self.last_x, self.last_y, old.id)?;
        self.terminated = self.terminated.saturating_add(1);
        self.phase = Phase::Idle;
        if self.session != 0 {
            return Err(BrCode::LeakBad.val());
        }
        Ok(())
    }

    /// 设备入册（幂等）；册满报 DeviceBad（宁拒不错认）。
    fn register(&mut self, dev: Device) -> Result<(), u32> {
        let mut k = 0usize;
        while k < self.ndev {
            if let Some(seen) = self.devices.get(k).and_then(|v| *v) {
                if seen.id == dev.id && seen.name == dev.name {
                    return Ok(());
                }
            }
            k += 1;
        }
        if self.ndev >= MAX_DEVICES { return Err(BrCode::DeviceBad.val()); }
        let slot = self.devices.get_mut(self.ndev).ok_or(BrCode::DeviceBad.val())?;
        *slot = Some(dev);
        self.ndev = self.ndev.saturating_add(1);
        Ok(())
    }

    /// 接管设备。
    ///
    /// 一次成功的接管只做三件事：终结旧会话（若有）、旧设备让位、新设备成为
    /// 当前设备。手指数（handoffs）只在「真有旧设备让位」时加一——首次接管
    /// 不是接管，重复接管不是接管（那是 DeviceBad）。
    pub fn attach(&mut self, dev: Device) -> Result<(), u32> {
        if self.phase == Phase::Closed { return self.refuse_attach(BrCode::BridgeBad.val()); }
        if self.phase == Phase::Flushing { return self.refuse_attach(BrCode::OrderBad.val()); }
        if !dev.valid() { return self.refuse_attach(BrCode::DeviceBad.val()); }
        // 同一设备重复接管：必须先断开再接管，不许原地续命。
        if let Some(cur) = self.current {
            if cur.id == dev.id { return self.refuse_attach(BrCode::DeviceBad.val()); }
        }
        // 双设备同名/同 id 冲突守卫（扫已见设备册）。
        let mut k = 0usize;
        while k < self.ndev {
            if let Some(seen) = self.devices.get(k).and_then(|v| *v) {
                if seen.id == dev.id && seen.name != dev.name {
                    return self.refuse_attach(BrCode::DeviceBad.val());
                }
                if seen.name == dev.name && seen.id != dev.id {
                    return self.refuse_attach(BrCode::DeviceBad.val());
                }
            }
            k += 1;
        }
        if let Some(old) = self.current {
            if self.phase == Phase::Tracking {
                self.terminate_session(old)?;
            }
            self.handoffs = self.handoffs.saturating_add(1);
        }
        self.register(dev)?;
        self.current = Some(dev);
        if self.phase != Phase::Tracking { self.phase = Phase::Idle; }
        if self.active() > 1 { return Err(BrCode::LeakBad.val()); }
        Ok(())
    }

    /// 设备断开。在途会话必须以 Cancel 终结，绝不静默丢弃。
    pub fn detach(&mut self) -> Result<(), u32> {
        let old = match self.current {
            Some(d) => d,
            None => return self.refuse_attach(BrCode::DeviceBad.val()),
        };
        if self.phase == Phase::Closed { return self.refuse_attach(BrCode::BridgeBad.val()); }
        if self.phase == Phase::Flushing { return self.refuse_attach(BrCode::OrderBad.val()); }
        if self.phase == Phase::Tracking { self.terminate_session(old)?; }
        self.current = None;
        self.detaches = self.detaches.saturating_add(1);
        Ok(())
    }

    /// 关闭桥接：在途会话以 Cancel 终结，相位进入不可复活的 Closed。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.phase == Phase::Closed { return Err(BrCode::BridgeBad.val()); }
        if self.phase == Phase::Flushing { return Err(BrCode::OrderBad.val()); }
        if let Some(d) = self.current {
            if self.phase == Phase::Tracking { self.terminate_session(d)?; }
        }
        // 设备标识刻意**保留**：关闭的是桥，不是设备。若在这里把 current 清掉，
        // 关闭后到达的事件会在「设备绑定」闸先撞 DeviceBad，把「桥已关」误诊
        // 成「设备不认识」——两个完全不同的根因。
        self.phase = Phase::Closed;
        Ok(())
    }

    /// 开批处理窗口。窗口内不受理事件（见 `phase_prefilter`）。
    pub fn begin_batch(&mut self) -> Result<(), u32> {
        if self.phase == Phase::Closed { return Err(BrCode::BridgeBad.val()); }
        if self.phase == Phase::Flushing { return Err(BrCode::OrderBad.val()); }
        self.resume = self.phase;
        self.phase = Phase::Flushing;
        Ok(())
    }

    /// 关批处理窗口：恢复进入窗口前的相位。
    ///
    /// 窗口本身**不产生帧、不动序号、不进摘要**——它只是相位的暂时冻结。
    /// 因此「批处理边界不改变事件顺序与计数」是结构性的，不靠事后比对兜底。
    pub fn end_batch(&mut self) -> Result<(), u32> {
        if self.phase != Phase::Flushing { return Err(BrCode::OrderBad.val()); }
        self.phase = self.resume;
        self.batches = self.batches.saturating_add(1);
        Ok(())
    }

    /// 喂入一拍 raw 事件（六道闸，见文件头「闸门次序」）。
    pub fn feed(&mut self, ev: RawEvent) -> Result<Frame, u32> {
        self.raw = self.raw.saturating_add(1);

        // ① 时钟闸：单调不减（相等允许，同拍多触点合法）且不越上界。
        if ev.tick > TICK_CAP || ev.tick < self.tick {
            return self.refuse_raw(BrCode::TimestampBad.val());
        }
        // ② 坐标 / 会话 / 设备绑定闸。坐标先于相位：坏坐标是入力本身的问题，
        //    与桥处于哪一相位无关；若挂在相位判断之后，Closed 相位下的坏坐标
        //    会被报成「桥已关」。
        if ev.x > COORD_MAX || ev.y > COORD_MAX {
            return self.refuse_raw(BrCode::BridgeBad.val());
        }
        let dev = match self.current {
            Some(d) => d,
            None => return self.refuse_raw(BrCode::DeviceBad.val()),
        };
        if ev.device != dev.id {
            return self.refuse_raw(BrCode::DeviceBad.val());
        }
        // 会话号预检：只有 Begin 会消耗号，号用尽必须显性拒绝而不是回绕。
        if ev.kind == RawKind::Begin && !(1..=SESSION_CAP).contains(&self.next_session) {
            return self.refuse_raw(BrCode::SessionBad.val());
        }
        // ③ 相位合法性（Closed/Flushing 专属码）。
        if let Err(code) = phase_prefilter(self.phase) {
            return self.refuse_raw(code);
        }
        // ④ 查表。
        let c = match transition(self.phase, ev.kind) {
            Ok(v) => v,
            Err(code) => return self.refuse_raw(code),
        };
        let fk = match c.emit.kind() {
            Some(v) => v,
            None => return self.refuse_raw(BrCode::BridgeBad.val()),
        };
        // ⑤ 计数与几何（帧容量 / 序号 / 会话号）在 `commit` 内一次性核。
        // ⑥ 终判：出帧 + 改相。
        let f = self.commit(fk, ev.tick, ev.x, ev.y, dev.id)?;
        self.phase = c.next;
        Ok(f)
    }

    /// 整桥审计：三账守恒 + 相位/会话一致 + 序号连续 + 摘要非零。
    ///
    /// 「相位与会话一致」最容易漏：phase==Tracking 必须恰好对应 session!=0，
    /// 反之亦然。热切换如果漏发 Cancel，这里立刻 LeakBad。
    pub fn audit(&self) -> Result<(), u32> {
        if self.raw.saturating_add(self.terminated) != self.frames.saturating_add(self.dropped) {
            return Err(BrCode::LedgerBad.val());
        }
        if self.opened != self.downs || self.closed != self.closes {
            return Err(BrCode::LedgerBad.val());
        }
        if !self.ledger().conserved() || !self.ledger().terminated_within_closed() {
            return Err(BrCode::LedgerBad.val());
        }
        if self.active() > 1 { return Err(BrCode::LeakBad.val()); }
        let in_gesture = self.session != 0;
        let tracking = self.phase == Phase::Tracking
            || (self.phase == Phase::Flushing && self.resume == Phase::Tracking);
        if in_gesture != tracking { return Err(BrCode::LeakBad.val()); }
        if self.phase == Phase::Tracking && self.active() != 1 {
            return Err(BrCode::LeakBad.val());
        }
        if self.phase != Phase::Tracking && self.phase != Phase::Flushing && self.active() != 0 {
            return Err(BrCode::LeakBad.val());
        }
        // 序号无洞：每出一帧恰好 +1，因此 seq 必须恒等于 frames。
        if self.seq != self.frames { return Err(BrCode::SeqBad.val()); }
        if self.digest == 0 { return Err(BrCode::ReplayBad.val()); }
        Ok(())
    }
}

/// 帧序列逐元素比较（Vec 只经 `.get()` 取元素）。
pub fn frames_equal(a: &[Frame], b: &[Frame]) -> bool {
    if a.len() != b.len() { return false; }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y { return false; }
            }
            _ => return false,
        }
        i = i.saturating_add(1);
    }
    true
}

// ── 探针一：透传逐拍守恒（F48991 判据①） ─────────────────────────
/// 每轮五拍：一拍坏事件（Move-before-Begin，必拒）+ 四拍好手势。
///
/// 每一拍之后都现场核对守恒式，而不是只在收尾核对一次——「逐拍守恒」与
/// 「总量守恒」强弱不同：中途多记少记会在总量上互相抵消，只有逐拍核对才挡得住。
pub fn mixed_probe(rounds: u32) -> Result<BridgeStats, u32> {
    if rounds == 0 || rounds > BRIDGE_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let dev = DEV_A;
    let mut b = Bridge::new();
    b.attach(dev)?;
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < rounds {
        // ① 坏事件：Move 先于 Begin，必须拒绝而非静默开局。
        tick = tick.saturating_add(1);
        if b.feed(RawEvent::mv(tick, 3, 4, dev.id)).err() != Some(BrCode::OrderBad.val()) {
            return Err(BrCode::OrderBad.val());
        }
        if !b.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
        // ② Begin
        tick = tick.saturating_add(1);
        let f0 = b.feed(RawEvent::begin(tick, 10, 10, dev.id))?;
        if f0.kind != FrameKind::Down || f0.session == 0 { return Err(BrCode::BridgeBad.val()); }
        if !b.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
        // ③④ 两拍 Move
        let mut j = 0u32;
        while j < 2 {
            tick = tick.saturating_add(1);
            let f = b.feed(RawEvent::mv(tick, 11, 11, dev.id))?;
            if f.kind != FrameKind::Move || f.session != f0.session {
                return Err(BrCode::BridgeBad.val());
            }
            if !b.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
            j = j.saturating_add(1);
        }
        // ⑤ End
        tick = tick.saturating_add(1);
        let f3 = b.feed(RawEvent::end(tick, 12, 12, dev.id))?;
        if f3.kind != FrameKind::Up || f3.session != f0.session {
            return Err(BrCode::BridgeBad.val());
        }
        if !b.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
        b.audit()?;
        i = i.saturating_add(1);
    }
    let s = b.stats();
    s.audit()?;
    // 千轮定值：raw = 5/轮、frames = 4/轮、dropped = 1/轮。
    if s.raw != rounds.saturating_mul(5)
        || s.frames != rounds.saturating_mul(4)
        || s.dropped != rounds
        || s.terminated != 0
    {
        return Err(BrCode::LedgerBad.val());
    }
    let l = b.ledger();
    if l.opened != rounds || l.closed != rounds || l.active != 0 {
        return Err(BrCode::LedgerBad.val());
    }
    Ok(s)
}

// ── 探针二：会话号单调不复用（F48991 判据②） ─────────────────────
/// 千轮「Begin→End」：会话号逐轮严格递增且永不复用。
///
/// 判据不只看「是否递增」，还钉住「第 k 轮恰为 k 号」——只递增但跳号同样是
/// 桥接身份泄漏：号段是稀缺资源，跳号意味着有会话没被记账。
pub fn session_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > SESSION_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let dev = DEV_A;
    let mut b = Bridge::new();
    b.attach(dev)?;
    let mut tick = 0u32;
    let mut prev = 0u32;
    let mut i = 0u32;
    while i < rounds {
        tick = tick.saturating_add(1);
        let f0 = b.feed(RawEvent::begin(tick, 5, 5, dev.id))?;
        if f0.kind != FrameKind::Down { return Err(BrCode::BridgeBad.val()); }
        if f0.session == 0 || f0.session <= prev || f0.session != i.saturating_add(1) {
            return Err(BrCode::SessionBad.val());
        }
        prev = f0.session;
        tick = tick.saturating_add(1);
        let f1 = b.feed(RawEvent::end(tick, 6, 6, dev.id))?;
        if f1.kind != FrameKind::Up || f1.session != f0.session {
            return Err(BrCode::SessionBad.val());
        }
        // 序号无洞：关闭帧必须紧接开启帧。
        if f1.seq != f0.seq.saturating_add(1) { return Err(BrCode::SeqBad.val()); }
        b.audit()?;
        i = i.saturating_add(1);
    }
    Ok(rounds)
}

// ── 探针三：批处理边界保序保数（F48991 判据④） ───────────────────
/// 批处理脚本：8 拍，含一拍坏事件与两次会话（一次正常收、一次取消）。
pub fn script_ev(base: u32, j: usize, device: u32) -> Option<RawEvent> {
    let tick = base.saturating_add(j as u32);
    match j {
        0 => Some(RawEvent::mv(tick, 4, 4, device)),
        1 => Some(RawEvent::begin(tick, 10, 10, device)),
        2 => Some(RawEvent::mv(tick, 12, 12, device)),
        3 => Some(RawEvent::mv(tick, 14, 13, device)),
        4 => Some(RawEvent::end(tick, 15, 15, device)),
        5 => Some(RawEvent::begin(tick, 20, 20, device)),
        6 => Some(RawEvent::mv(tick, 21, 22, device)),
        7 => Some(RawEvent::cancel(tick, 22, 23, device)),
        _ => None,
    }
}

/// 同一脚本跑两遍：一遍不带批处理边界，一遍在第 0/3/6 拍前开窗关窗。
///
/// 两次的帧序列必须**逐元素相等**（顺序 + 计数 + 会话号 + 序号 + 摘要），
/// 只比帧数不够：帧数相等但顺序颠倒同样是「批处理边界动了事件流」。
pub fn batch_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > BATCH_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let dev = DEV_A;
    let mut i = 0u32;
    while i < rounds {
        let base = i.saturating_mul(16).saturating_add(1);
        // ① 平铺跑
        let mut plain: Vec<Frame> = Vec::new();
        let mut b1 = Bridge::new();
        b1.attach(dev)?;
        let mut j = 0usize;
        while j < SCRIPT_LEN {
            let ev = script_ev(base, j, dev.id).ok_or(BrCode::CoverBad.val())?;
            match b1.feed(ev) {
                Ok(f) => plain.push(f),
                Err(e) => {
                    if e != BrCode::OrderBad.val() {
                        return Err(e);
                    }
                }
            }
            if !b1.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
            j = j.saturating_add(1);
        }
        b1.audit()?;

        // ② 带边界跑
        let mut flushed: Vec<Frame> = Vec::new();
        let mut b2 = Bridge::new();
        b2.attach(dev)?;
        let mut k = 0usize;
        while k < SCRIPT_LEN {
            if k % 3 == 0 {
                b2.begin_batch()?;
                b2.end_batch()?;
            }
            let ev = script_ev(base, k, dev.id).ok_or(BrCode::CoverBad.val())?;
            match b2.feed(ev) {
                Ok(f) => flushed.push(f),
                Err(e) => {
                    if e != BrCode::OrderBad.val() {
                        return Err(e);
                    }
                }
            }
            if !b2.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
            k = k.saturating_add(1);
        }
        b2.audit()?;

        // ③ 逐元素比对（顺序 + 计数 + 字段）
        if !frames_equal(&plain, &flushed) { return Err(BrCode::OrderBad.val()); }
        if b1.stats() != b2.stats() || b2.batches() == 0 {
            return Err(BrCode::LedgerBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(rounds)
}

// ── 探针四：千次序列分类零漂移（F48991 判据⑤） ───────────────────
const SIG_TAP: [FrameKind; 2] = [FrameKind::Down, FrameKind::Up];
const SIG_DRAG: [FrameKind; 4] = [FrameKind::Down, FrameKind::Move, FrameKind::Move, FrameKind::Up];
const SIG_CANCEL: [FrameKind; 2] = [FrameKind::Down, FrameKind::Cancel];
const SIG_REJECT: [FrameKind; 2] = [FrameKind::Down, FrameKind::Up];

/// 每类的期望丢弃数：只有「坏开头」类丢一拍（被拒的 Move）。
pub const CLASS_DROPS: [u32; CLASSES] = [0, 0, 0, 1];

pub fn expected_kinds(class: usize) -> Option<&'static [FrameKind]> {
    match class {
        0 => Some(&SIG_TAP[..]),
        1 => Some(&SIG_DRAG[..]),
        2 => Some(&SIG_CANCEL[..]),
        3 => Some(&SIG_REJECT[..]),
        _ => None,
    }
}

/// 四类的原始脚本（tick 由 base 起逐拍递增）。
pub fn class_events(class: usize, base: u32, device: u32) -> Option<Vec<RawEvent>> {
    let mut v: Vec<RawEvent> = Vec::new();
    let t = |k: u32| base.saturating_add(k);
    match class {
        0 => {
            v.push(RawEvent::begin(t(0), 5, 5, device));
            v.push(RawEvent::end(t(1), 6, 6, device));
        }
        1 => {
            v.push(RawEvent::begin(t(0), 5, 5, device));
            v.push(RawEvent::mv(t(1), 6, 6, device));
            v.push(RawEvent::mv(t(2), 7, 7, device));
            v.push(RawEvent::end(t(3), 8, 8, device));
        }
        2 => {
            v.push(RawEvent::begin(t(0), 5, 5, device));
            v.push(RawEvent::cancel(t(1), 6, 6, device));
        }
        3 => {
            // 坏开头：Move 先于 Begin 必须被拒，随后补一个正常点按。
            v.push(RawEvent::mv(t(0), 5, 5, device));
            v.push(RawEvent::begin(t(1), 5, 5, device));
            v.push(RawEvent::end(t(2), 6, 6, device));
        }
        _ => return None,
    }
    Some(v)
}

/// 千次分类：每轮取一类脚本，帧类序列必须与该类的钉死签名逐元素一致，
/// 丢弃数也必须与 `CLASS_DROPS` 一致。零漂移 = 1000 轮全部命中。
pub fn seq_class_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > BRIDGE_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let dev = DEV_B;
    let mut i = 0u32;
    while i < rounds {
        let class = (i as usize) % CLASSES;
        let base = i.saturating_mul(16).saturating_add(1);
        let evs = class_events(class, base, dev.id).ok_or(BrCode::CoverBad.val())?;
        let want = expected_kinds(class).ok_or(BrCode::CoverBad.val())?;
        let want_drops = *CLASS_DROPS.get(class).ok_or(BrCode::CoverBad.val())?;
        let mut b = Bridge::new();
        b.attach(dev)?;
        let mut got: Vec<FrameKind> = Vec::new();
        let mut drops = 0u32;
        let mut last_seq = 0u32;
        let mut session = 0u32;
        let mut k = 0usize;
        while k < evs.len() {
            let ev = *evs.get(k).ok_or(BrCode::CoverBad.val())?;
            match b.feed(ev) {
                Ok(f) => {
                    if f.seq != last_seq.saturating_add(1) { return Err(BrCode::SeqBad.val()); }
                    if session == 0 {
                        session = f.session;
                    } else if f.session != session {
                        return Err(BrCode::SessionBad.val());
                    }
                    last_seq = f.seq;
                    got.push(f.kind);
                }
                Err(_) => drops = drops.saturating_add(1),
            }
            if !b.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
            k = k.saturating_add(1);
        }
        if got.len() != want.len() || drops != want_drops {
            return Err(BrCode::FuzzBad.val());
        }
        let mut m = 0usize;
        while m < want.len() {
            if got.get(m) != want.get(m) { return Err(BrCode::FuzzBad.val()); }
            m = m.saturating_add(1);
        }
        b.audit()?;
        i = i.saturating_add(1);
    }
    Ok(rounds)
}

// ── 探针五：千次热切换零悬挂（F48992） ───────────────────────────
/// 千轮「接管→起手势→再接管」：每次接管都必须把上一次的悬挂会话以 Cancel 终结。
///
/// 三条独立核对：①终结计数逐轮加一（少一次即漏终结）；②终结帧必须是 Cancel；
/// ③接管后 active == 0 且相位回到 Idle —— 「零悬挂」的可观测形式。
pub fn handoff_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > HANDOFF_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let mut b = Bridge::new();
    let mut tick = 0u32;
    let mut last_seq = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let dev = *POOL.get((i as usize) % POOL.len()).ok_or(BrCode::CoverBad.val())?;
        b.attach(dev)?;
        if b.current().map(|d| d.id) != Some(dev.id) { return Err(BrCode::DeviceBad.val()); }
        if i > 0 {
            if b.ledger().terminated != i { return Err(BrCode::HandoffBad.val()); }
            let tail = b.last_frame().ok_or(BrCode::LeakBad.val())?;
            if tail.kind != FrameKind::Cancel { return Err(BrCode::LeakBad.val()); }
            if tail.seq != last_seq.saturating_add(1) { return Err(BrCode::SeqBad.val()); }
            last_seq = tail.seq;
            if b.ledger().active != 0 || b.phase() != Phase::Idle {
                return Err(BrCode::LeakBad.val());
            }
        }
        tick = tick.saturating_add(1);
        let down = b.feed(RawEvent::begin(tick, 10, 10, dev.id))?;
        if down.kind != FrameKind::Down
            || down.device != dev.id
            || down.seq != last_seq.saturating_add(1)
        {
            return Err(BrCode::BridgeBad.val());
        }
        last_seq = down.seq;
        tick = tick.saturating_add(1);
        let mv = b.feed(RawEvent::mv(tick, 11, 12, dev.id))?;
        if mv.seq != last_seq.saturating_add(1) || mv.session != down.session {
            return Err(BrCode::SeqBad.val());
        }
        last_seq = mv.seq;
        if b.ledger().active != 1 { return Err(BrCode::HandoffBad.val()); }
        b.audit()?;
        i = i.saturating_add(1);
    }
    // 收尾：断开与接管走同一条 Cancel 路径，因此 terminated 恰为 rounds。
    b.detach()?;
    let l = b.ledger();
    if l.active != 0 || l.opened != rounds || l.closed != rounds {
        return Err(BrCode::LedgerBad.val());
    }
    if l.terminated != rounds || l.handoffs != rounds.saturating_sub(1) || l.rejected != 0 {
        return Err(BrCode::HandoffBad.val());
    }
    b.audit()?;
    Ok(rounds)
}

// ── 探针六：跨设备事件序号单调（F48992） ─────────────────────────
/// 设备在四个号之间轮转，序号必须**全桥唯一且逐帧 +1**，绝不因换设备复位。
///
/// 同时钉一条容易漏的语义：接管合成的 Cancel 帧必须挂在**旧设备 id** 上。若挂在
/// 新的，接管账就无法回答「谁终结了谁」，热切换审计形同虚设。
pub fn seq_cross_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > HANDOFF_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let mut b = Bridge::new();
    let mut tick = 0u32;
    let mut expect_seq = 0u32;
    let mut prev_session = 0u32;
    let mut prev_term = 0u32;
    let mut last_dev = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let dev = *POOL.get((i as usize) % POOL.len()).ok_or(BrCode::CoverBad.val())?;
        b.attach(dev)?;
        let term = b.ledger().terminated;
        if term > prev_term {
            let f = b.last_frame().ok_or(BrCode::LeakBad.val())?;
            if f.kind != FrameKind::Cancel { return Err(BrCode::LeakBad.val()); }
            if f.seq != expect_seq.saturating_add(1) { return Err(BrCode::SeqBad.val()); }
            if f.device != last_dev { return Err(BrCode::DeviceBad.val()); }
            expect_seq = f.seq;
            prev_term = term;
            if b.ledger().active != 0 { return Err(BrCode::LeakBad.val()); }
        }
        tick = tick.saturating_add(1);
        let down = b.feed(RawEvent::begin(tick, 10, 10, dev.id))?;
        if down.seq != expect_seq.saturating_add(1) || down.session <= prev_session {
            return Err(BrCode::SeqBad.val());
        }
        expect_seq = down.seq;
        prev_session = down.session;
        last_dev = dev.id;
        tick = tick.saturating_add(1);
        let mv = b.feed(RawEvent::mv(tick, 11, 12, dev.id))?;
        if mv.seq != expect_seq.saturating_add(1) || mv.session != down.session {
            return Err(BrCode::SeqBad.val());
        }
        expect_seq = mv.seq;
        // 每四轮留一次悬挂，交给下一次接管终结（这样 Cancel 帧才会跨设备出现）。
        if i % 4 != 3 {
            tick = tick.saturating_add(1);
            let up = b.feed(RawEvent::end(tick, 13, 14, dev.id))?;
            if up.seq != expect_seq.saturating_add(1) || up.session != down.session {
                return Err(BrCode::SeqBad.val());
            }
            expect_seq = up.seq;
        }
        if b.seq() != expect_seq { return Err(BrCode::SeqBad.val()); }
        b.audit()?;
        i = i.saturating_add(1);
    }
    if b.current().is_some() {
        b.detach()?;
    }
    if b.seq() != b.stats().frames { return Err(BrCode::SeqBad.val()); }
    b.audit()?;
    Ok(rounds)
}

// ── 探针七：断开即 Cancel 终结（F48992 泄漏判据） ─────────────────
pub fn leak_probe() -> Result<u32, u32> {
    let dev = DEV_A;
    let mut b = Bridge::new();
    b.attach(dev)?;
    let f1 = b.feed(RawEvent::begin(1, 10, 10, dev.id))?;
    let f2 = b.feed(RawEvent::mv(2, 11, 12, dev.id))?;
    if f1.kind != FrameKind::Down || f2.kind != FrameKind::Move {
        return Err(BrCode::BridgeBad.val());
    }
    let before = b.seq();
    // ① 设备断开：在途手势必须以 Cancel 终结。
    b.detach()?;
    let tail = b.last_frame().ok_or(BrCode::LeakBad.val())?;
    if tail.kind != FrameKind::Cancel || tail.session != f1.session {
        return Err(BrCode::LeakBad.val());
    }
    // ② 终结帧序号必须接在断开前那一帧之后（不许复用序号）。
    if tail.seq != before.saturating_add(1) { return Err(BrCode::SeqBad.val()); }
    // ③ 零悬挂 ④ 终结账恰好加一 ⑤ 相位回到 Idle。
    if b.ledger().active != 0 || b.ledger().terminated != 1 {
        return Err(BrCode::LeakBad.val());
    }
    if b.phase() != Phase::Idle { return Err(BrCode::OrderBad.val()); }
    // ⑥ 重连后会话号必须接续（不复用旧号）。
    b.attach(dev)?;
    let f3 = b.feed(RawEvent::begin(3, 1, 1, dev.id))?;
    if f3.session <= f1.session { return Err(BrCode::SessionBad.val()); }
    if f3.seq != tail.seq.saturating_add(1) { return Err(BrCode::SeqBad.val()); }
    b.audit()?;
    Ok(5)
}

// ── 探针八：重复接管与双设备冲突守卫（F48992） ───────────────────
/// 六种非法接管必须全部 DeviceBad，且**被拒的接管不得扰动在途会话**。
pub fn conflict_probe() -> Result<u32, u32> {
    let bad = BrCode::DeviceBad.val();
    // 非法接管必须**原样**报 DeviceBad：换任何别的码都说明守卫走了岔路。
    let must = |r: Result<(), u32>| if r.err() == Some(bad) { Ok(()) } else { Err(bad) };

    // ① 同 id 异名 ② 同名异 id
    let mut b1 = Bridge::new();
    b1.attach(DEV_A)?;
    b1.detach()?;
    must(b1.attach(Device::new(DEV_A.id, "impostor")))?;
    must(b1.attach(Device::new(9, DEV_A.name)))?;
    // ③ 同一设备重复接管（未断开）
    let mut b3 = Bridge::new();
    b3.attach(DEV_A)?;
    must(b3.attach(DEV_A))?;
    // ④ id 为零 ⑤ 名为空
    let mut b4 = Bridge::new();
    must(b4.attach(Device::new(0, "zero-id")))?;
    must(b4.attach(Device::new(5, "")))?;
    // ⑥ 在途手势下的重复接管：必须拒绝，且**不得**顺手把会话终结掉
    //    （否则「拒接管」会偷偷变成「断开」，接管账就骗人了）。
    let mut b6 = Bridge::new();
    b6.attach(DEV_A)?;
    let f = b6.feed(RawEvent::begin(1, 3, 3, DEV_A.id))?;
    must(b6.attach(DEV_A))?;
    if b6.ledger().active != 1 || b6.ledger().terminated != 0 || b6.phase() != Phase::Tracking {
        return Err(BrCode::HandoffBad.val());
    }
    if b6.session() != f.session { return Err(BrCode::LeakBad.val()); }
    b6.audit()?;
    Ok(6)
}

// ── 探针九：关闭路径 ──────────────────────────────────────────────
/// 关闭不是静默丢弃：在途手势同样以 Cancel 终结，之后桥进入不可复活态。
pub fn close_probe() -> Result<u32, u32> {
    let mut b = Bridge::new();
    b.attach(DEV_A)?;
    let f0 = b.feed(RawEvent::begin(1, 5, 5, DEV_A.id))?;
    let _ = b.feed(RawEvent::mv(2, 6, 6, DEV_A.id))?;
    b.close()?;
    let tail = b.last_frame().ok_or(BrCode::LeakBad.val())?;
    if tail.kind != FrameKind::Cancel || tail.session != f0.session {
        return Err(BrCode::HandoffBad.val());
    }
    if b.ledger().active != 0 || b.phase() != Phase::Closed {
        return Err(BrCode::LeakBad.val());
    }
    // 关闭后的事件：相位闸先拒（BridgeBad），而不是设备闸（DeviceBad）。
    if b.feed(RawEvent::begin(3, 1, 1, DEV_A.id)).err() != Some(BrCode::BridgeBad.val()) {
        return Err(BrCode::BridgeBad.val());
    }
    // 二次关闭拒绝；关闭后不许再接管。
    if b.close().err() != Some(BrCode::BridgeBad.val()) { return Err(BrCode::BridgeBad.val()); }
    if b.attach(DEV_B).err() != Some(BrCode::BridgeBad.val()) {
        return Err(BrCode::BridgeBad.val());
    }
    b.audit()?;
    Ok(5)
}

// ── 影子模型：独立复算每一拍的期望结果 ───────────────────────────
/// 影子模型**不调用** Bridge 的任何方法，用自己一套相位/会话/序号规则复算
/// 「这一拍该不该出帧、出什么帧、会话号与序号各是多少」。
///
/// 它与桥的闸门次序刻意写成同一份语义（时钟→坐标→设备→相位），但实现完全
/// 独立：影子用 match 三元组，桥用 4×4 常量表。两边同时写错的概率远低于单边
/// 写错——这就是 fuzz 对账的意义。
struct Shadow {
    phase: Phase,
    session: u32,
    next_session: u32,
    seq: u32,
    tick: u32,
    raw: u32,
    frames: u32,
    dropped: u32,
}

impl Shadow {
    fn new() -> Shadow {
        Shadow {
            phase: Phase::Idle, session: 0, next_session: 1, seq: 0,
            tick: 0, raw: 0, frames: 0, dropped: 0,
        }
    }

    fn drop_one(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }

    fn expect(&mut self, ev: &RawEvent, dev: u32) -> Option<(u32, u32, FrameKind)> {
        self.raw = self.raw.saturating_add(1);
        if ev.tick > TICK_CAP || ev.tick < self.tick { self.drop_one(); return None; }
        if ev.x > COORD_MAX || ev.y > COORD_MAX { self.drop_one(); return None; }
        if ev.device != dev { self.drop_one(); return None; }
        let fk = match (self.phase, ev.kind) {
            (Phase::Idle, RawKind::Begin) => FrameKind::Down,
            (Phase::Tracking, RawKind::Move) => FrameKind::Move,
            (Phase::Tracking, RawKind::End) => FrameKind::Up,
            (Phase::Tracking, RawKind::Cancel) => FrameKind::Cancel,
            (_, _) => {
                self.drop_one();
                return None;
            }
        };
        self.seq = self.seq.saturating_add(1);
        let session = if fk == FrameKind::Down {
            let s = self.next_session;
            self.next_session = self.next_session.saturating_add(1);
            self.session = s;
            s
        } else {
            self.session
        };
        if fk == FrameKind::Up || fk == FrameKind::Cancel {
            self.session = 0;
        }
        self.phase = match fk {
            FrameKind::Down | FrameKind::Move => Phase::Tracking,
            FrameKind::Up | FrameKind::Cancel => Phase::Idle,
        };
        self.tick = ev.tick;
        self.frames = self.frames.saturating_add(1);
        Some((session, self.seq, fk))
    }
}

/// 随机事件流 × 影子逐拍对账。
pub fn fuzz_bridge(rounds: u32) -> Result<BridgeStats, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let dev = Device::new(7, "fuzz-pad");
    let mut b = Bridge::new();
    b.attach(dev)?;
    let mut sh = Shadow::new();
    let mut rng = p_core::Lcg::new(0x7178_0F91_4891_0F91);
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let mut j = 0usize;
        while j < 8 {
            // 高位取模：LCG 低位周期短，取 >>33 后的高位才够散。
            tick = tick.saturating_add(((rng.next() >> 33) % 3) as u32);
            let kind = RawKind::from_index(((rng.next() >> 33) % EVENTS as u64) as u8)
                .ok_or(BrCode::CoverBad.val())?;
            let x = ((rng.next() >> 33) % (COORD_MAX as u64 + 2)) as u32;
            let y = ((rng.next() >> 33) % (COORD_MAX as u64 + 2)) as u32;
            let devid = if (rng.next() >> 33) % 8 == 0 { dev.id + 1 } else { dev.id };
            let ev = RawEvent::new(kind, tick, x, y, devid);
            let want = sh.expect(&ev, dev.id);
            match (want, b.feed(ev)) {
                (None, Err(_)) => {}
                (Some((ws, wq, wk)), Ok(f)) => {
                    if f.session != ws || f.seq != wq || f.kind != wk || f.device != dev.id {
                        return Err(BrCode::FuzzBad.val());
                    }
                }
                (_, _) => return Err(BrCode::FuzzBad.val()),
            }
            if !b.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
            j = j.saturating_add(1);
        }
        b.audit()?;
        i = i.saturating_add(1);
    }
    let s = b.stats();
    if s.raw != sh.raw || s.frames != sh.frames || s.dropped != sh.dropped {
        return Err(BrCode::FuzzBad.val());
    }
    s.audit()?;
    Ok(s)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffStats {
    pub rounds: u32,
    pub handoffs: u32,
    pub terminated: u32,
    pub expected_terminations: u32,
    pub rejected: u32,
    pub digest: u64,
}

impl HandoffStats {
    pub fn clean(&self) -> bool {
        self.rounds > 0
            && self.handoffs > 0
            && self.terminated == self.expected_terminations
            && self.digest != 0
    }
}

/// 随机接管/断开/喂事件 × 每步审计。
///
/// 关键在于**每完成一个操作就整桥审计一次**：热切换的账目错误（漏 Cancel、
/// 终结数不符、active 挂住）往往隔几步才显形，逐操作审计才能把根因钉在那一步。
pub fn fuzz_handoff(rounds: u32) -> Result<HandoffStats, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let mut b = Bridge::new();
    let mut rng = p_core::Lcg::new(0x7178_0F92_4892_0F92);
    let mut tick = 0u32;
    let mut expected = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let pick = (rng.next() >> 33) % 6;
        if pick <= 2 {
            let dev = *POOL.get(((rng.next() >> 33) as usize) % POOL.len())
                .ok_or(BrCode::CoverBad.val())?;
            let tracking_before = b.phase() == Phase::Tracking;
            match b.attach(dev) {
                Ok(()) => {
                    if tracking_before {
                        expected = expected.saturating_add(1);
                        if b.ledger().terminated != expected {
                            return Err(BrCode::HandoffBad.val());
                        }
                        let tail = b.last_frame().ok_or(BrCode::LeakBad.val())?;
                        if tail.kind != FrameKind::Cancel { return Err(BrCode::LeakBad.val()); }
                    }
                    if b.ledger().active != 0 && b.phase() != Phase::Tracking {
                        return Err(BrCode::LeakBad.val());
                    }
                }
                Err(e) => {
                    if e != BrCode::DeviceBad.val() { return Err(e); }
                }
            }
        } else if pick == 3 {
            if b.current().is_some() {
                let tracking = b.phase() == Phase::Tracking;
                b.detach()?;
                if tracking {
                    expected = expected.saturating_add(1);
                    if b.ledger().terminated != expected || b.ledger().active != 0 {
                        return Err(BrCode::HandoffBad.val());
                    }
                }
            }
        } else if let Some(dev) = b.current() {
            tick = tick.saturating_add(((rng.next() >> 33) % 3) as u32 + 1);
            let kind = RawKind::from_index(((rng.next() >> 33) % EVENTS as u64) as u8)
                .ok_or(BrCode::CoverBad.val())?;
            let _ = b.feed(RawEvent::new(kind, tick, 7, 8, dev.id));
        }
        b.audit()?;
        if !b.stats().conserved() { return Err(BrCode::LedgerBad.val()); }
        i = i.saturating_add(1);
    }
    let l = b.ledger();
    if !l.conserved() || !l.terminated_within_closed() || l.active > 1 {
        return Err(BrCode::LedgerBad.val());
    }
    Ok(HandoffStats {
        rounds,
        handoffs: l.handoffs,
        terminated: l.terminated,
        expected_terminations: expected,
        rejected: l.rejected,
        digest: b.digest(),
    })
}

// ── 注入（四路：坏事件 / 双子 Begin / 时钟回退 / 重复接管） ────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS { return Err(BrCode::CoverBad.val()); }
    let mut log = InjectLog { tries: [0; INJECT_ROUTES], caught: [0; INJECT_ROUTES] };
    let mut i = 0u32;
    while i < rounds {
        // ① Move 先于 Begin：OrderBad（不许静默开局）。
        let mut b1 = Bridge::new();
        if b1.attach(DEV_A).is_ok()
            && b1.feed(RawEvent::mv(1, 1, 1, DEV_A.id)).err() == Some(BrCode::OrderBad.val())
        {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);

        // ② 双子 Begin：BridgeBad（不许中途换会话）。
        let mut b2 = Bridge::new();
        if b2.attach(DEV_A).is_ok()
            && b2.feed(RawEvent::begin(1, 1, 1, DEV_A.id)).is_ok()
            && b2.feed(RawEvent::begin(2, 2, 2, DEV_A.id)).err() == Some(BrCode::BridgeBad.val())
        {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);

        // ③ 时钟回退：TimestampBad。
        let mut b3 = Bridge::new();
        if b3.attach(DEV_A).is_ok()
            && b3.feed(RawEvent::begin(5, 1, 1, DEV_A.id)).is_ok()
            && b3.feed(RawEvent::mv(4, 2, 2, DEV_A.id)).err() == Some(BrCode::TimestampBad.val())
        {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);

        // ④ 同一设备重复接管：DeviceBad。
        let mut b4 = Bridge::new();
        if b4.attach(DEV_A).is_ok() && b4.attach(DEV_A).err() == Some(BrCode::DeviceBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);

        i = i.saturating_add(1);
    }
    if !log.all_caught() { return Err(BrCode::CoverBad.val()); }
    Ok(log)
}

// ── 渲染（6 字段：raw|frames|dropped|handoffs|terminated|digest） ──
pub fn render_stats(s: &BridgeStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.raw, s.frames, s.dropped, s.handoffs, s.terminated, s.digest
    )
}

/// 严格十进制解析：空串、非数字、溢出、以及**前导零**一律拒。
///
/// 前导零为什么要拒：`01` 与 `1` 是同一个数却有两种文本形态，一旦放行，
/// 「文本 → 结构」就不再是单射，篡改一行账本可以在数值不变的前提下逃过逐字
/// 比对。严格解析让渲染结果成为账本的唯一文本表示。
pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    if b.len() > 1 && *b.get(0)? == b'0' {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 || *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

/// 严格复解析：字段数、数值、守恒式、终结上限、摘要非零，五道全过才算读回一本账。
pub fn audit_render(text: &str) -> Result<BridgeStats, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS { return Err(BrCode::RenderBad.val()); }
    let mut f = [0u32; 5];
    let mut i = 0usize;
    while i < 5 {
        let v = parts.get(i).and_then(|s| parse_u32(s)).ok_or(BrCode::RenderBad.val())?;
        if let Some(slot) = f.get_mut(i) {
            *slot = v;
        }
        i += 1;
    }
    let digest = parts.get(5).and_then(|s| parse_hex16(s)).ok_or(BrCode::RenderBad.val())?;
    let s = BridgeStats {
        raw: f[0], frames: f[1], dropped: f[2], handoffs: f[3], terminated: f[4], digest,
    };
    // 结构与守恒复算：渲染出来的账必须自身闭合，否则它是「被改过的账」。
    if s.frames == 0 || !s.conserved() { return Err(BrCode::LedgerBad.val()); }
    if s.frames < s.terminated { return Err(BrCode::HandoffBad.val()); }
    if s.digest == 0 { return Err(BrCode::RenderBad.val()); }
    Ok(s)
}

// ── 黄金运行 / 复跑 ───────────────────────────────────────────────
/// 黄金脚本：32 轮，四设备轮转接管，每轮一拍坏事件 + 一次手势，每三轮留一次
/// 悬挂交给下一次接管终结。
pub fn golden_bridge() -> Result<Bridge, u32> {
    let mut b = Bridge::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        let dev = *POOL.get((i as usize) % POOL.len()).ok_or(BrCode::CoverBad.val())?;
        b.attach(dev)?;
        // 每轮先来一拍坏事件：相位必为 Idle（上一轮要么收干净、要么被本次接管
        // 终结），因此 Move 必被拒而不改账。
        tick = tick.saturating_add(1);
        if b.feed(RawEvent::mv(tick, 2, 2, dev.id)).err() != Some(BrCode::OrderBad.val()) {
            return Err(BrCode::OrderBad.val());
        }
        tick = tick.saturating_add(1);
        b.feed(RawEvent::begin(tick, 10, 10, dev.id))?;
        tick = tick.saturating_add(1);
        b.feed(RawEvent::mv(tick, 11, 12, dev.id))?;
        if i % 3 != 0 {
            tick = tick.saturating_add(1);
            if i % 2 == 0 {
                b.feed(RawEvent::end(tick, 12, 13, dev.id))?;
            } else {
                b.feed(RawEvent::cancel(tick, 12, 13, dev.id))?;
            }
        }
        i = i.saturating_add(1);
    }
    if b.current().is_some() {
        b.detach()?;
    }
    b.audit()?;
    Ok(b)
}

pub fn golden_run() -> Result<BridgeStats, u32> {
    let b = golden_bridge()?;
    let s = b.stats();
    s.audit()?;
    Ok(s)
}

/// 两次黄金 + 两次 fuzz + 两次混跑 + 两次热切换，逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    if golden_run()? != golden_run()? {
        return Err(BrCode::ReplayBad.val());
    }
    if fuzz_bridge(128)? != fuzz_bridge(128)? {
        return Err(BrCode::ReplayBad.val());
    }
    if mixed_probe(64)? != mixed_probe(64)? {
        return Err(BrCode::ReplayBad.val());
    }
    if fuzz_handoff(96)? != fuzz_handoff(96)? {
        return Err(BrCode::ReplayBad.val());
    }
    Ok(())
}

// ── 探针序列（段位 + 三大判据的粗粒度门） ────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if audit_table()? != (PHASES * EVENTS) as u32 { return Err(BrCode::BridgeBad.val()); }
    if !codes_are_distinct() || !segment_guard() { return Err(BrCode::SegmentBad.val()); }
    if BrCode::ALL.len() != 14 || STATS_FIELDS != 6 || INJECT_ROUTES != 4 {
        return Err(BrCode::BridgeBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    if !mixed_probe(BRIDGE_ROUNDS)?.conserved() { return Err(BrCode::LedgerBad.val()); }
    if session_probe(SESSION_ROUNDS)? != SESSION_ROUNDS { return Err(BrCode::SessionBad.val()); }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if handoff_probe(HANDOFF_ROUNDS)? != HANDOFF_ROUNDS { return Err(BrCode::HandoffBad.val()); }
    if seq_cross_probe(HANDOFF_ROUNDS)? != HANDOFF_ROUNDS { return Err(BrCode::SeqBad.val()); }
    if leak_probe()? != 5 || conflict_probe()? != 6 || close_probe()? != 5 {
        return Err(BrCode::FuzzBad.val());
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// F48991 掌拒联签层（B06 联签引用不二写）
// ═══════════════════════════════════════════════════════════════════
/// 掌拒联签场景数（卡面 ×10^2）。
pub const PALM_ROUNDS: u32 = 100;
/// B06 掌拒分数 / 边缘接近度的千分比上界。
pub const PALM_SCORE_MAX: u32 = 1_000;
/// 联签阈值：B06 分数 + 边缘接近度合计达此值才允许拒绝。
pub const SYNERGY_MIN: u32 = 600;
/// 边缘主导阈值：接近度单独达此值按贴边掌团处理。
pub const EDGE_PALM_MIN: u32 = 700;
/// 掌团面积阈值（当量格）。
pub const PALM_AREA_MIN: u32 = 900;
/// 掌团面积上界（当量格）。
pub const PALM_AREA_MAX: u32 = 4_000;
/// 与在途手势质心的相关半径（当量格）。
pub const CORR_RADIUS: u32 = 96;
/// 相关半径平方（免开方，整数域）。
const CORR_RADIUS2: u32 = CORR_RADIUS * CORR_RADIUS;
/// 联签在途手势质心（固定于屏幕中部，场景围绕它生成）。
pub const PALM_GATE_CX: u32 = 2_000;
pub const PALM_GATE_CY: u32 = 2_000;
/// 掌拒场景种子。
pub const PALM_SEED: u64 = 0x7178_0F49_9149_1B06;

/// 掌拒联签判决：只区分「放行」与「拒绝」，并保留拒绝理由。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PalmDecision {
    /// 放行：B06 未联签（不抢判）或联签证据不足。
    Accept,
    /// 放行：与在途手势质心相关，属于同一手势的续拍。
    AcceptCorrelated,
    /// 拒绝：B06 联签达阈、与在途手势不相关，但未达掌团形态阈值。
    RejectUncorrelated,
    /// 拒绝：联签达阈 + 贴边/超面积掌团形态。
    RejectPalmBlob,
}

impl PalmDecision {
    pub fn accepted(self) -> bool {
        matches!(self, PalmDecision::Accept | PalmDecision::AcceptCorrelated)
    }

    pub fn rejected(self) -> bool {
        !self.accepted()
    }

    pub fn tag(self) -> u32 {
        match self {
            PalmDecision::Accept => 1,
            PalmDecision::AcceptCorrelated => 2,
            PalmDecision::RejectUncorrelated => 3,
            PalmDecision::RejectPalmBlob => 4,
        }
    }
}

/// B06 联签输入：只引用、不重算。`flagged` 是 B06 的掌拒标记，
/// `score` 是 B06 掌拒分数，`edge` 是边缘接近度（越大越贴边），均为千分比。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PalmSignal {
    pub flagged: bool,
    pub score: u32,
    pub edge: u32,
}

impl PalmSignal {
    pub fn new(flagged: bool, score: u32, edge: u32) -> Result<PalmSignal, u32> {
        if score > PALM_SCORE_MAX || edge > PALM_SCORE_MAX {
            return Err(BrCode::CoverBad.val());
        }
        Ok(PalmSignal { flagged, score, edge })
    }

    /// 联签合计（饱和加，永不溢出）。
    pub fn combined(self) -> u32 {
        self.score.saturating_add(self.edge)
    }
}

/// 待判触摸团：中心、面积、触点个数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TouchCluster {
    pub cx: u32,
    pub cy: u32,
    pub area: u32,
    pub contacts: u8,
}

impl TouchCluster {
    pub fn new(cx: u32, cy: u32, area: u32, contacts: u8) -> Result<TouchCluster, u32> {
        if cx > COORD_MAX || cy > COORD_MAX {
            return Err(BrCode::BridgeBad.val());
        }
        if area == 0 || area > PALM_AREA_MAX || contacts == 0 || contacts > 3 {
            return Err(BrCode::BridgeBad.val());
        }
        Ok(TouchCluster { cx, cy, area, contacts })
    }

    /// 到 (cx, cy) 的平方距离（整数、饱和）。
    pub fn dist2_to(&self, cx: u32, cy: u32) -> u32 {
        let dx = if self.cx >= cx { self.cx - cx } else { cx - self.cx };
        let dy = if self.cy >= cy { self.cy - cy } else { cy - self.cy };
        dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
    }
}

/// 掌拒联签闸门：持有**在途手势质心**，把 B06 标记与几何相关性合并判定。
///
/// 三条不可交换的次序：
/// ① 先判相关：与在途手势相关的续拍是真手势，哪怕 B06 误标也绝不拒；
/// ② 再看 B06 标记：未联签不抢判（避免边缘滑入等真手势被误杀）；
/// ③ 最后看形态：联签达阈后，贴边或超面积 → 掌团；否则按不相关团拒绝。
pub struct PalmGate {
    cx: u32,
    cy: u32,
}

impl PalmGate {
    pub fn new(cx: u32, cy: u32) -> Result<PalmGate, u32> {
        if cx > COORD_MAX || cy > COORD_MAX {
            return Err(BrCode::BridgeBad.val());
        }
        Ok(PalmGate { cx, cy })
    }

    pub fn judge(&self, sig: PalmSignal, cluster: TouchCluster) -> PalmDecision {
        if cluster.dist2_to(self.cx, self.cy) <= CORR_RADIUS2 {
            return PalmDecision::AcceptCorrelated;
        }
        if !sig.flagged {
            return PalmDecision::Accept;
        }
        if sig.combined() < SYNERGY_MIN {
            return PalmDecision::Accept;
        }
        if sig.edge >= EDGE_PALM_MIN || cluster.area >= PALM_AREA_MIN {
            PalmDecision::RejectPalmBlob
        } else {
            PalmDecision::RejectUncorrelated
        }
    }
}

/// 确定性掌拒场景：真值 `palm` 表示本轮是一次掌团尝试。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PalmScenario {
    pub signal: PalmSignal,
    pub cluster: TouchCluster,
    pub palm: bool,
}

/// 第 `round` 个确定性场景（四种形态轮转，四类判决都被覆盖）。
pub fn palm_scenario(round: u32) -> Result<PalmScenario, u32> {
    let seed = PALM_SEED ^ (round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut rng = p_core::Lcg::new(seed);
    match round % 4 {
        // 贴边大团 + B06 标记 + 远离在途手势：掌团。
        0 | 1 => {
            let score = 500u32.saturating_add(rng.below(500));
            let edge = 600u32.saturating_add(rng.below(400));
            let cx = if round % 2 == 0 { 32 } else { COORD_MAX.saturating_sub(32) };
            let cy = 100u32.saturating_add(rng.below(3_000));
            let area = PALM_AREA_MIN.saturating_add(rng.below(1_000));
            Ok(PalmScenario {
                signal: PalmSignal::new(true, score, edge)?,
                cluster: TouchCluster::new(cx, cy, area, 3)?,
                palm: true,
            })
        }
        // B06 标记 + 与在途手势质心相关：真手势续拍。
        2 => {
            let score = 500u32.saturating_add(rng.below(500));
            let edge = 100u32.saturating_add(rng.below(800));
            let cx = PALM_GATE_CX.saturating_sub(40).saturating_add(rng.below(80));
            let cy = PALM_GATE_CY.saturating_sub(40).saturating_add(rng.below(80));
            let area = 120u32.saturating_add(rng.below(300));
            Ok(PalmScenario {
                signal: PalmSignal::new(true, score, edge)?,
                cluster: TouchCluster::new(cx, cy, area, 1)?,
                palm: false,
            })
        }
        // B06 未标记 + 贴边小团：边缘滑入真手势，不抢判。
        _ => {
            let edge = 850u32.saturating_add(rng.below(150));
            let cx = 40u32.saturating_add(rng.below(200));
            let cy = 40u32.saturating_add(rng.below(200));
            let area = 200u32.saturating_add(rng.below(400));
            Ok(PalmScenario {
                signal: PalmSignal::new(false, 0, edge)?,
                cluster: TouchCluster::new(cx, cy, area, 1)?,
                palm: false,
            })
        }
    }
}

/// 掌拒联签账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PalmStats {
    pub rounds: u32,
    pub palms: u32,
    pub genuine: u32,
    pub rejected: u32,
    pub accepted: u32,
    pub false_triggers: u32,
    pub missed: u32,
    pub blob_rejects: u32,
    pub digest: u64,
}

impl PalmStats {
    pub fn clean(&self) -> bool {
        self.rounds > 0
            && self.palms.saturating_add(self.genuine) == self.rounds
            && self.rejected.saturating_add(self.accepted) == self.rounds
            && self.false_triggers == 0
            && self.missed == 0
            && self.rejected == self.palms
            && self.accepted == self.genuine
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.rounds == 0 {
            return Err(BrCode::CoverBad.val());
        }
        if !self.clean() {
            return Err(BrCode::FuzzBad.val());
        }
        if self.digest == 0 {
            return Err(BrCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 掌拒联签探针：`rounds` 个确定性场景，掌团必须全拒、真手势必须全过。
pub fn palm_synergy_probe(rounds: u32) -> Result<PalmStats, u32> {
    if rounds == 0 || rounds > PALM_ROUNDS {
        return Err(BrCode::CoverBad.val());
    }
    let gate = PalmGate::new(PALM_GATE_CX, PALM_GATE_CY)?;
    let mut s = PalmStats {
        rounds,
        palms: 0,
        genuine: 0,
        rejected: 0,
        accepted: 0,
        false_triggers: 0,
        missed: 0,
        blob_rejects: 0,
        digest: p_core::FNV_OFF,
    };
    let mut i = 0u32;
    while i < rounds {
        let sc = palm_scenario(i)?;
        let d = gate.judge(sc.signal, sc.cluster);
        if sc.palm {
            s.palms = s.palms.saturating_add(1);
            if d.rejected() {
                s.rejected = s.rejected.saturating_add(1);
            } else {
                s.missed = s.missed.saturating_add(1);
            }
        } else {
            s.genuine = s.genuine.saturating_add(1);
            if d.accepted() {
                s.accepted = s.accepted.saturating_add(1);
            } else {
                s.false_triggers = s.false_triggers.saturating_add(1);
            }
        }
        if d == PalmDecision::RejectPalmBlob {
            s.blob_rejects = s.blob_rejects.saturating_add(1);
        }
        s.digest = p_core::fold_u32(s.digest, d.tag());
        s.digest = p_core::fold_u32(s.digest, sc.signal.combined());
        s.digest = p_core::fold_u32(s.digest, sc.cluster.area);
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

/// 联签语义探针：五个手工场景逐一核对 B06 引用语义。
/// 返回通过的场景数（必须为 5）。
pub fn palm_gate_probe() -> Result<u32, u32> {
    let gate = PalmGate::new(PALM_GATE_CX, PALM_GATE_CY)?;
    let mut n = 0u32;
    let edge = TouchCluster::new(32, PALM_GATE_CY, PALM_AREA_MIN.saturating_add(10), 3)?;
    let near = TouchCluster::new(PALM_GATE_CX.saturating_add(10), PALM_GATE_CY, 64, 1)?;
    let far = TouchCluster::new(300, 800, 100, 1)?;
    let small = TouchCluster::new(1_000, 1_000, 120, 1)?;
    // ① 标记 + 贴边大团 + 不相关 → 掌团。
    if gate.judge(PalmSignal::new(true, 600, 400)?, edge) != PalmDecision::RejectPalmBlob {
        return Err(BrCode::FuzzBad.val());
    }
    n = n.saturating_add(1);
    // ② 标记 + 弱证据 → 放行（联签未达阈不抢判）。
    if gate.judge(PalmSignal::new(true, 100, 100)?, far) != PalmDecision::Accept {
        return Err(BrCode::FuzzBad.val());
    }
    n = n.saturating_add(1);
    // ③ 未标记 + 贴边小团 → 放行（边缘滑入真手势）。
    if gate.judge(PalmSignal::new(false, 0, 990)?, edge) != PalmDecision::Accept {
        return Err(BrCode::FuzzBad.val());
    }
    n = n.saturating_add(1);
    // ④ 标记 + 与在途质心相关 → 放行（真手势续拍）。
    if gate.judge(PalmSignal::new(true, 900, 900)?, near) != PalmDecision::AcceptCorrelated {
        return Err(BrCode::FuzzBad.val());
    }
    n = n.saturating_add(1);
    // ⑤ 标记 + 达阈 + 不相关 + 非掌团形态 → 不相关团拒绝。
    if gate.judge(PalmSignal::new(true, 400, 300)?, small) != PalmDecision::RejectUncorrelated {
        return Err(BrCode::FuzzBad.val());
    }
    n = n.saturating_add(1);
    // 非法输入必须拒绝。
    if PalmSignal::new(true, PALM_SCORE_MAX.saturating_add(1), 0).is_ok() {
        return Err(BrCode::CoverBad.val());
    }
    if TouchCluster::new(COORD_MAX.saturating_add(1), 0, 100, 1).is_ok() {
        return Err(BrCode::BridgeBad.val());
    }
    Ok(n)
}

// ═══════════════════════════════════════════════════════════════════
// F48992 四点观测埋点（起/迁移/取消/止）
// ═══════════════════════════════════════════════════════════════════
/// 观测点总数（卡面四点）。
pub const OBS_POINTS: usize = 4;
/// 观测会话轮数。
pub const OBS_ROUNDS: u32 = 100;

/// 观测点：起（会话开始）/迁移（手势推进）/取消（异常终结）/止（正常结束）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPoint {
    Start,
    Transition,
    Cancel,
    End,
}

impl ObsPoint {
    pub const ALL: [ObsPoint; OBS_POINTS] = [
        ObsPoint::Start,
        ObsPoint::Transition,
        ObsPoint::Cancel,
        ObsPoint::End,
    ];

    pub fn index(self) -> usize {
        match self {
            ObsPoint::Start => 0,
            ObsPoint::Transition => 1,
            ObsPoint::Cancel => 2,
            ObsPoint::End => 3,
        }
    }

    pub fn tag(self) -> u32 {
        self.index() as u32 + 1
    }

    pub fn label(self) -> &'static str {
        match self {
            ObsPoint::Start => "起",
            ObsPoint::Transition => "迁移",
            ObsPoint::Cancel => "取消",
            ObsPoint::End => "止",
        }
    }
}

/// 一条观测事件：点、会话号、会话内序号、单调时间戳。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsEvent {
    pub point: ObsPoint,
    pub session: u32,
    pub seq: u32,
    pub tick: u32,
}

impl ObsEvent {
    pub fn new(point: ObsPoint, session: u32, seq: u32, tick: u32) -> Result<ObsEvent, u32> {
        if session == 0 || seq == 0 {
            return Err(BrCode::SessionBad.val());
        }
        if tick > TICK_CAP {
            return Err(BrCode::TimestampBad.val());
        }
        Ok(ObsEvent { point, session, seq, tick })
    }
}

/// 观测账：四类计数 + 异常计数 + 摘要。坏事件不进覆盖账，只抬异常计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Observer {
    counts: [u32; OBS_POINTS],
    total: u32,
    last_tick: u32,
    last_session: u32,
    last_seq: u32,
    anomalies: u32,
    digest: u64,
}

impl Observer {
    pub fn new() -> Observer {
        Observer {
            counts: [0u32; OBS_POINTS],
            total: 0,
            last_tick: 0,
            last_session: 0,
            last_seq: 0,
            anomalies: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn record(&mut self, ev: ObsEvent) -> Result<(), u32> {
        if ev.tick < self.last_tick {
            self.anomalies = self.anomalies.saturating_add(1);
            return Err(BrCode::TimestampBad.val());
        }
        if ev.session < self.last_session {
            self.anomalies = self.anomalies.saturating_add(1);
            return Err(BrCode::SessionBad.val());
        }
        if ev.session == self.last_session {
            if ev.seq <= self.last_seq {
                self.anomalies = self.anomalies.saturating_add(1);
                return Err(BrCode::SeqBad.val());
            }
        } else if ev.seq != 1 {
            // 新会话首拍必须从 1 起（会话内序号不得继承上一会话）。
            self.anomalies = self.anomalies.saturating_add(1);
            return Err(BrCode::SeqBad.val());
        }
        let slot = match self.counts.get_mut(ev.point.index()) {
            Some(s) => s,
            None => {
                self.anomalies = self.anomalies.saturating_add(1);
                return Err(BrCode::CoverBad.val());
            }
        };
        *slot = slot.saturating_add(1);
        self.total = self.total.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, ev.point.tag());
        self.digest = p_core::fold_u32(self.digest, ev.session);
        self.digest = p_core::fold_u32(self.digest, ev.seq);
        self.digest = p_core::fold_u32(self.digest, ev.tick);
        self.last_tick = ev.tick;
        self.last_session = ev.session;
        self.last_seq = ev.seq;
        Ok(())
    }

    pub fn count(&self, point: ObsPoint) -> u32 {
        match self.counts.get(point.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn anomalies(&self) -> u32 {
        self.anomalies
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn coverage(&self) -> Coverage {
        Coverage {
            counts: self.counts,
            total: self.total,
            anomalies: self.anomalies,
        }
    }
}

/// 覆盖账：四类齐备才算 100%，任一类占比 <10% 视为失衡并拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Coverage {
    pub counts: [u32; OBS_POINTS],
    pub total: u32,
    pub anomalies: u32,
}

impl Coverage {
    pub fn present(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < OBS_POINTS {
            if let Some(v) = self.counts.get(i) {
                if *v > 0 {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        n
    }

    pub fn percent(&self) -> u32 {
        (self.present() as u32).saturating_mul(100) / OBS_POINTS as u32
    }

    /// 覆盖审计：非空、四类齐备、且无一类占比低于 10%。
    pub fn audit(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(BrCode::CoverBad.val());
        }
        let mut i = 0usize;
        while i < OBS_POINTS {
            let c = match self.counts.get(i) {
                Some(v) => *v,
                None => return Err(BrCode::CoverBad.val()),
            };
            if c == 0 {
                return Err(BrCode::CoverBad.val());
            }
            if c.saturating_mul(10) < self.total {
                return Err(BrCode::CoverBad.val());
            }
            i = i.saturating_add(1);
        }
        Ok(())
    }

    pub fn balanced(&self) -> bool {
        self.audit().is_ok()
    }
}

/// 观测会话探针：每轮 1 起点 + 1..3 迁移 + 1 个取消或止点，四类全覆盖。
pub fn observe_probe(rounds: u32) -> Result<Observer, u32> {
    if rounds == 0 || rounds > OBS_ROUNDS {
        return Err(BrCode::CoverBad.val());
    }
    let mut o = Observer::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let session = i.saturating_add(1);
        tick = tick.saturating_add(1);
        o.record(ObsEvent::new(ObsPoint::Start, session, 1, tick)?)?;
        let trans = 1u32.saturating_add(i % 3);
        let mut k = 0u32;
        while k < trans {
            tick = tick.saturating_add(1);
            o.record(ObsEvent::new(
                ObsPoint::Transition,
                session,
                k.saturating_add(2),
                tick,
            )?)?;
            k = k.saturating_add(1);
        }
        tick = tick.saturating_add(1);
        let last = if i % 2 == 0 { ObsPoint::End } else { ObsPoint::Cancel };
        o.record(ObsEvent::new(
            last,
            session,
            trans.saturating_add(2),
            tick,
        )?)?;
        i = i.saturating_add(1);
    }
    o.coverage().audit()?;
    Ok(o)
}

/// 观测复跑：两遍摘要与覆盖逐字段相等。
pub fn observe_replay(rounds: u32) -> Result<(u64, u32), u32> {
    let a = observe_probe(rounds)?;
    let b = observe_probe(rounds)?;
    if a != b || a.digest() != b.digest() {
        return Err(BrCode::ReplayBad.val());
    }
    Ok((a.digest(), a.total()))
}

/// 覆盖负例探针：缺口/失衡必须被拒，四类齐备必须放行，
/// 坏序列事件必须被拒且异常计数可见。返回通过项数（必须为 4）。
pub fn observation_negative() -> Result<u32, u32> {
    let mut n = 0u32;
    let gap = Coverage {
        counts: [100, 100, 100, 0],
        total: 300,
        anomalies: 0,
    };
    if gap.audit() != Err(BrCode::CoverBad.val()) {
        return Err(BrCode::CoverBad.val());
    }
    n = n.saturating_add(1);
    let skew = Coverage {
        counts: [100, 100, 100, 1],
        total: 301,
        anomalies: 0,
    };
    if skew.audit() != Err(BrCode::CoverBad.val()) {
        return Err(BrCode::CoverBad.val());
    }
    n = n.saturating_add(1);
    let full = Coverage {
        counts: [100, 100, 100, 100],
        total: 400,
        anomalies: 0,
    };
    if full.audit() != Ok(()) || !full.balanced() || full.percent() != 100 {
        return Err(BrCode::CoverBad.val());
    }
    n = n.saturating_add(1);
    let mut o = Observer::new();
    o.record(ObsEvent::new(ObsPoint::Start, 1, 1, 10)?)?;
    if o.record(ObsEvent::new(ObsPoint::Transition, 1, 1, 11)?)
        != Err(BrCode::SeqBad.val())
    {
        return Err(BrCode::SeqBad.val());
    }
    if o.record(ObsEvent::new(ObsPoint::End, 1, 2, 9)?)
        != Err(BrCode::TimestampBad.val())
    {
        return Err(BrCode::TimestampBad.val());
    }
    if o.anomalies() != 2 {
        return Err(BrCode::LedgerBad.val());
    }
    n = n.saturating_add(1);
    if n != 4 {
        return Err(BrCode::CoverBad.val());
    }
    Ok(n)
}

// ── 域检查项（动态明细槽上限 8：F48991 四条 + F48992 四条） ───────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();
    let err = |e: u32| format!("err=0x{:X}", e);

    // ① F48991-J1 掌拒联签 ×10^2 零误触
    //   （聚合旧 F48991 透传守恒 + 四路坏事件注入 + 掌团/真手势全分类）
    let mixed = mixed_probe(BRIDGE_ROUNDS);
    let inj = inject_rounds(4);
    let palm = palm_synergy_probe(PALM_ROUNDS);
    let ok1 = matches!(&mixed, Ok(s) if s.conserved() && s.dropped == BRIDGE_ROUNDS)
        && matches!(&inj, Ok(l) if l.all_caught())
        && matches!(&palm, Ok(p) if p.clean() && p.palms == PALM_ROUNDS / 2);
    let d1 = match (&mixed, &palm) {
        (Ok(s), Ok(p)) => format!(
            "p={} g={} fz={} raw={}",
            p.palms, p.genuine, p.false_triggers, s.raw
        ),
        (Err(e), _) => err(*e),
        (_, Err(e)) => err(*e),
    };
    out.push(("F48991 掌拒联签×10^2零误触", ok1, d1));

    // ② 掌拒账本守恒 + 缺陷边界（叠旧会话号严格单调不复用）
    let sess = session_probe(SESSION_ROUNDS);
    let ok2 = matches!(&sess, Ok(n) if *n == SESSION_ROUNDS)
        && matches!(&palm, Ok(p) if p.rejected == p.palms
            && p.accepted == p.genuine
            && p.digest != 0)
        && palm_synergy_probe(0).err() == Some(BrCode::CoverBad.val())
        && palm_synergy_probe(PALM_ROUNDS.saturating_add(1)).err()
            == Some(BrCode::CoverBad.val());
    let d2 = match (&sess, &palm) {
        (Ok(n), Ok(p)) => format!("s={} rej={} acc={}", n, p.rejected, p.accepted),
        (Err(e), _) => err(*e),
        (_, Err(e)) => err(*e),
    };
    out.push(("F48991 掌拒账本+会话单调", ok2, d2));

    // ③ 掌拒复跑零漂移（叠旧批处理边界保序保数）
    let batch = batch_probe(BATCH_ROUNDS);
    let p3a = palm_synergy_probe(64);
    let p3b = palm_synergy_probe(64);
    let ok3 = matches!(&batch, Ok(n) if *n == BATCH_ROUNDS)
        && matches!((&p3a, &p3b), (Ok(a), Ok(b)) if a == b);
    let d3 = match (&batch, &p3a) {
        (Ok(n), Ok(p)) => format!("scripts={} palm={} eq=1", n, p.rounds),
        (Err(e), _) => err(*e),
        (_, Err(e)) => err(*e),
    };
    out.push(("F48991 掌拒复跑+批处理保序", ok3, d3));

    // ④ B06 联签引用语义（叠旧千次序列分类零漂移）
    let cls = seq_class_probe(BRIDGE_ROUNDS);
    let gate = palm_gate_probe();
    let ok4 = matches!(&cls, Ok(n) if *n == BRIDGE_ROUNDS) && matches!(&gate, Ok(5));
    let d4 = match (&cls, &gate) {
        (Ok(n), Ok(g)) => format!("cls={} gate={}", n, g),
        (Err(e), _) => err(*e),
        (_, Err(e)) => err(*e),
    };
    out.push(("F48991 掌拒引用B06+B10序列", ok4, d4));

    // ⑤ F48992-J1 起/迁移/取消/止四点 100% 覆盖
    //   （聚合旧 F48992 热切换零悬挂终结）
    let obs = observe_probe(OBS_ROUNDS);
    let ho = handoff_probe(HANDOFF_ROUNDS);
    let leak = leak_probe();
    let ok5 = matches!(&obs, Ok(o) if o.coverage().percent() == 100
            && o.coverage().audit().is_ok()
            && o.count(ObsPoint::Start) > 0
            && o.count(ObsPoint::Transition) > 0
            && o.count(ObsPoint::Cancel) > 0
            && o.count(ObsPoint::End) > 0)
        && matches!(&ho, Ok(n) if *n == HANDOFF_ROUNDS)
        && matches!(&leak, Ok(5));
    let d5 = match (&obs, &ho) {
        (Ok(o), Ok(n)) => format!(
            "cov={}% ev={} ho={}",
            o.coverage().percent(),
            o.total(),
            n
        ),
        (Err(e), _) => err(*e),
        (_, Err(e)) => err(*e),
    };
    out.push(("F48992 起迁取消止四点覆盖", ok5, d5));

    // ⑥ 四点失衡/缺口拒绝（叠旧跨设备事件序号单调）
    let neg = observation_negative();
    let cross = seq_cross_probe(HANDOFF_ROUNDS);
    let ok6 = matches!(&neg, Ok(n) if *n == 4)
        && matches!(&cross, Ok(n) if *n == HANDOFF_ROUNDS);
    let d6 = match (&neg, &cross) {
        (Ok(n), Ok(m)) => format!("neg={} cross={}", n, m),
        (Err(e), _) => err(*e),
        (_, Err(e)) => err(*e),
    };
    out.push(("F48992 四点失衡拒绝+跨设备", ok6, d6));

    // ⑦ 四点账本守恒（叠旧重复接管与双设备冲突守卫）
    let cf = conflict_probe();
    let ledger_ok = match &obs {
        Ok(o) => {
            let c = o.coverage();
            let mut sum = 0u32;
            let mut k = 0usize;
            while k < OBS_POINTS {
                if let Some(v) = c.counts.get(k) {
                    sum = sum.saturating_add(*v);
                }
                k = k.saturating_add(1);
            }
            sum == c.total && c.total > 0 && o.anomalies() == 0
        }
        Err(_) => false,
    };
    let ok7 = ledger_ok && matches!(&cf, Ok(6));
    let d7 = match &cf {
        Ok(n) => format!("obs_sum=eq cf={}", n),
        Err(e) => err(*e),
    };
    out.push(("F48992 四点账本+冲突守卫", ok7, d7));

    // ⑧ 四点复跑零漂移（叠旧接管账本守恒 + 渲染复解析）
    let rep = observe_replay(OBS_ROUNDS);
    let gold = golden_bridge();
    let (ok8a, d8a) = match &rep {
        Ok((dg, total)) => (true, format!("dg=0x{:X} ev={}", dg, total)),
        Err(e) => (false, err(*e)),
    };
    let (ok8b, d8b) = match &gold {
        Ok(b) => {
            let l = b.ledger();
            let s = b.stats();
            let back = audit_render(&render_stats(&s));
            let pass = l.conserved()
                && l.terminated_within_closed()
                && l.active == 0
                && l.opened == l.closed
                && b.phase() == Phase::Idle
                && matches!(&back, Ok(p) if *p == s);
            (pass, format!("o={} c={} t={}", l.opened, l.closed, l.terminated))
        }
        Err(e) => (false, err(*e)),
    };
    let ok8 = ok8a && ok8b;
    let d8 = if ok8a { d8b } else { d8a };
    out.push(("F48992 四点复跑+账本闭合", ok8, d8));

    out
}

pub fn run_unx48991_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F48991");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            cs.add_dyn(n, *p, d);
        }
        i = i.saturating_add(1);
    }
    cs
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn dev() -> Device {
        DEV_A
    }

    fn attached() -> Bridge {
        let mut b = Bridge::new();
        b.attach(dev()).expect("attach");
        b
    }

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71780);
        assert_eq!(PHASES, 4);
        assert_eq!(EVENTS, 4);
        assert_eq!(CLASSES, 4);
        assert_eq!(INJECT_ROUTES, 4);
        assert_eq!(STATS_FIELDS, 6);
        assert_eq!(BrCode::ALL.len(), 14);
        assert_eq!(TABLE.len(), PHASES);
        assert_eq!(GOLDEN_ROUNDS, 32);
    }

    #[test]
    fn codes_are_closed() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(detail(BrCode::BridgeBad.val()), BrCode::BridgeBad.detail());
        assert_eq!(detail(BrCode::FuzzBad.val()), BrCode::FuzzBad.detail());
        assert_eq!(detail(0xDEAD), "未知诊断码");
    }

    #[test]
    fn every_code_is_documented_in_window() {
        // 14 个诊断码逐一过：可枚举、落在段窗、有中文解释、与自由函数互认。
        assert_eq!(BrCode::ALL.len(), 14);
        let mut i = 0usize;
        while i < BrCode::ALL.len() {
            let c = BrCode::ALL[i];
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0F);
            assert!(!c.detail().is_empty());
            assert_eq!(detail(c.val()), c.detail());
            i += 1;
        }
        // 公共路径上真实可触发的四个：OrderBad / DeviceBad / BridgeBad / TimestampBad。
        let mut b = attached();
        assert_eq!(b.feed(RawEvent::mv(1, 1, 1, dev().id)).err(), Some(BrCode::OrderBad.val()));
        assert_eq!(b.feed(RawEvent::begin(2, 1, 1, DEV_B.id)).err(), Some(BrCode::DeviceBad.val()));
        let bb = BrCode::BridgeBad.val();
        assert_eq!(b.feed(RawEvent::begin(3, COORD_MAX + 1, 1, dev().id)).err(), Some(bb));
        let ts = BrCode::TimestampBad.val();
        assert_eq!(b.feed(RawEvent::begin(TICK_CAP + 1, 1, 1, dev().id)).err(), Some(ts));
    }

    #[test]
    fn phase_table_is_total() {
        assert_eq!(audit_table(), Ok(16));
    }

    #[test]
    fn phase_table_legal_cells() {
        let bb = BrCode::BridgeBad.val();
        assert!(transition(Phase::Idle, RawKind::Begin).is_ok());
        assert!(transition(Phase::Tracking, RawKind::Move).is_ok());
        assert!(transition(Phase::Tracking, RawKind::End).is_ok());
        assert!(transition(Phase::Tracking, RawKind::Cancel).is_ok());
        assert_eq!(transition(Phase::Idle, RawKind::Move).err(), Some(BrCode::OrderBad.val()));
        assert_eq!(transition(Phase::Tracking, RawKind::Begin).err(), Some(bb));
        assert_eq!(transition(Phase::Closed, RawKind::Begin).err(), Some(bb));
        assert_eq!(transition(Phase::Flushing, RawKind::Move).err(), Some(BrCode::OrderBad.val()));
    }

    #[test]
    fn out_of_order_events_are_rejected() {
        let mut b = attached();
        // Move/End/Cancel 先于 Begin：一律 OrderBad，且绝不开局。
        assert_eq!(b.feed(RawEvent::mv(1, 1, 1, dev().id)).err(), Some(BrCode::OrderBad.val()));
        assert_eq!(b.feed(RawEvent::end(2, 1, 1, dev().id)).err(), Some(BrCode::OrderBad.val()));
        assert_eq!(b.feed(RawEvent::cancel(3, 1, 1, dev().id)).err(), Some(BrCode::OrderBad.val()));
        assert_eq!(b.stats().dropped, 3);
        assert_eq!(b.stats().frames, 0);
        assert_eq!(b.phase(), Phase::Idle);
        assert_eq!(b.ledger().opened, 0);
    }

    #[test]
    fn double_begin_is_rejected() {
        let mut b = attached();
        b.feed(RawEvent::begin(1, 1, 1, dev().id)).expect("begin");
        assert_eq!(b.feed(RawEvent::begin(2, 2, 2, dev().id)).err(), Some(BrCode::BridgeBad.val()));
        assert_eq!(b.ledger().opened, 1);
        assert_eq!(b.stats().dropped, 1);
    }

    #[test]
    fn feed_requires_device() {
        let bad = BrCode::DeviceBad.val();
        let mut b = Bridge::new();
        assert_eq!(b.feed(RawEvent::begin(1, 1, 1, 1)).err(), Some(bad));
        assert_eq!(b.stats().dropped, 1);
        // 事件带的设备号必须与当前设备一致。
        let mut b = attached();
        assert_eq!(b.feed(RawEvent::begin(1, 1, 1, DEV_B.id)).err(), Some(bad));
    }

    #[test]
    fn tick_regression_is_rejected() {
        let ts = BrCode::TimestampBad.val();
        let mut b = attached();
        b.feed(RawEvent::begin(10, 1, 1, dev().id)).expect("begin");
        assert_eq!(b.feed(RawEvent::mv(9, 2, 2, dev().id)).err(), Some(ts));
    }

    #[test]
    fn tick_is_monotonic_and_bounded() {
        // 同拍合法（相等允许，一拍多触点）
        let mut b = attached();
        b.feed(RawEvent::begin(10, 1, 1, dev().id)).expect("begin");
        let f = b.feed(RawEvent::mv(10, 2, 2, dev().id)).expect("same tick");
        assert_eq!(f.tick, 10);
        assert_eq!(f.kind, FrameKind::Move);
        // 越上界必拒
        let ts = BrCode::TimestampBad.val();
        let mut b = attached();
        assert_eq!(b.feed(RawEvent::begin(TICK_CAP + 1, 1, 1, dev().id)).err(), Some(ts));
    }

    #[test]
    fn coord_overflow_is_rejected() {
        let mut b = attached();
        assert_eq!(
            b.feed(RawEvent::begin(1, COORD_MAX + 1, 1, dev().id)).err(),
            Some(BrCode::BridgeBad.val())
        );
        assert_eq!(
            b.feed(RawEvent::begin(2, 1, COORD_MAX + 1, dev().id)).err(),
            Some(BrCode::BridgeBad.val())
        );
        // 上界本身是合法坐标（闭区间）。
        assert!(b.feed(RawEvent::begin(3, COORD_MAX, COORD_MAX, dev().id)).is_ok());
    }

    #[test]
    fn feed_after_close_is_rejected() {
        let mut b = attached();
        b.close().expect("close");
        assert_eq!(
            b.feed(RawEvent::begin(1, 1, 1, dev().id)).err(),
            Some(BrCode::BridgeBad.val())
        );
        assert_eq!(b.close().err(), Some(BrCode::BridgeBad.val()));
    }

    #[test]
    fn feed_during_flush_is_rejected() {
        let mut b = attached();
        b.begin_batch().expect("begin batch");
        assert_eq!(b.phase(), Phase::Flushing);
        assert_eq!(
            b.feed(RawEvent::begin(1, 1, 1, dev().id)).err(),
            Some(BrCode::OrderBad.val())
        );
        b.end_batch().expect("end batch");
        assert_eq!(b.phase(), Phase::Idle);
        assert!(b.feed(RawEvent::begin(2, 1, 1, dev().id)).is_ok());
    }

    #[test]
    fn batch_window_restores_tracking_phase() {
        let mut b = attached();
        b.feed(RawEvent::begin(1, 1, 1, dev().id)).expect("begin");
        b.begin_batch().expect("begin batch");
        assert_eq!(b.phase(), Phase::Flushing);
        b.end_batch().expect("end batch");
        assert_eq!(b.phase(), Phase::Tracking);
        assert!(b.audit().is_ok());
        assert_eq!(b.begin_batch(), Ok(()));
        assert_eq!(b.end_batch(), Ok(()));
        assert_eq!(b.batches(), 2);
    }

    #[test]
    fn sessions_strictly_increase_and_never_zero() {
        assert_eq!(session_probe(32), Ok(32));
        let mut b = attached();
        assert_eq!(b.feed(RawEvent::begin(1, 1, 1, dev().id)).expect("b").session, 1);
        b.feed(RawEvent::end(2, 1, 1, dev().id)).expect("e");
        assert_eq!(b.session(), 0);
        assert_eq!(b.feed(RawEvent::begin(3, 1, 1, dev().id)).expect("b2").session, 2);
    }

    #[test]
    fn conservation_per_step() {
        let s = mixed_probe(64).expect("mixed");
        assert!(s.conserved());
        assert_eq!(s.raw, 64 * 5);
        assert_eq!(s.frames, 64 * 4);
        assert_eq!(s.dropped, 64);
        assert_eq!(s.terminated, 0);
    }

    #[test]
    fn batch_flush_preserves_order() {
        assert_eq!(batch_probe(64), Ok(64));
    }

    #[test]
    fn class_probe_zero_drift() {
        assert_eq!(seq_class_probe(256), Ok(256));
        assert_eq!(expected_kinds(0), Some(&SIG_TAP[..]));
        assert_eq!(expected_kinds(9), None);
        assert_eq!(class_events(9, 1, 1), None);
    }

    #[test]
    fn handoff_terminates_and_leaves_no_dangling() {
        assert_eq!(leak_probe(), Ok(5));
        assert_eq!(handoff_probe(64), Ok(64));
    }

    #[test]
    fn seq_monotonic_across_devices() {
        assert_eq!(seq_cross_probe(48), Ok(48));
    }

    #[test]
    fn repeat_attach_is_device_bad() {
        let mut b = attached();
        assert_eq!(b.attach(dev()).err(), Some(BrCode::DeviceBad.val()));
        assert_eq!(b.ledger().rejected, 1);
        assert_eq!(b.ledger().handoffs, 0);
    }

    #[test]
    fn device_conflicts_are_device_bad() {
        let bad = BrCode::DeviceBad.val();
        let mut b = attached();
        b.detach().expect("detach");
        // 同 id 异名 / 同名异 id 都必须拒。
        assert_eq!(b.attach(Device::new(dev().id, "other-name")).err(), Some(bad));
        assert_eq!(b.attach(Device::new(77, dev().name)).err(), Some(bad));
    }

    #[test]
    fn attach_rejects_invalid_devices() {
        let mut b = Bridge::new();
        assert_eq!(b.attach(Device::new(0, "x")).err(), Some(BrCode::DeviceBad.val()));
        assert_eq!(b.attach(Device::new(1, "")).err(), Some(BrCode::DeviceBad.val()));
        assert_eq!(b.ledger().rejected, 2);
        // 未接管设备时断开同样 DeviceBad。
        assert_eq!(b.detach().err(), Some(BrCode::DeviceBad.val()));
    }

    #[test]
    fn ledger_conserved() {
        let mut b = attached();
        b.feed(RawEvent::begin(1, 1, 1, dev().id)).expect("begin");
        assert_eq!(b.ledger().active, 1);
        b.feed(RawEvent::end(2, 1, 1, dev().id)).expect("end");
        assert_eq!(b.ledger().active, 0);
        assert!(b.ledger().conserved());
        assert_eq!(b.ledger().opened, b.ledger().closed);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn inject_catches_four() {
        match inject_rounds(5) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [5, 5, 5, 5]);
                assert_eq!(l.caught, [5, 5, 5, 5]);
            }
            Err(_) => assert!(false),
        }
        assert_eq!(inject_rounds(0).err(), Some(BrCode::CoverBad.val()));
    }

    #[test]
    fn render_round_trips() {
        match golden_run() {
            Ok(s) => {
                let text = render_stats(&s);
                assert_eq!(text.split('|').count(), STATS_FIELDS);
                assert_eq!(audit_render(&text), Ok(s));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_tamper_is_rejected() {
        let bad = BrCode::RenderBad.val();
        // 字段数不符 / 空串 / 非数字 / 前导零 / 摘要前缀 / 全零摘要
        assert_eq!(audit_render("1|1|0|0|0|0x0000000000000001|9").err(), Some(bad));
        assert_eq!(audit_render("1|1|0|0|0").err(), Some(bad));
        assert_eq!(audit_render("x|1|0|0|0|0x0000000000000001").err(), Some(bad));
        assert_eq!(audit_render("01|1|0|0|0|0x0000000000000001").err(), Some(bad));
        assert_eq!(audit_render("|1|0|0|0|0x0000000000000001").err(), Some(bad));
        assert_eq!(audit_render("1|1|0|0|0|0y0000000000000001").err(), Some(bad));
        assert_eq!(audit_render("1|1|0|0|0|0x0000000000000000").err(), Some(bad));
    }

    #[test]
    fn render_conservation_is_enforced() {
        let lb = BrCode::LedgerBad.val();
        // raw + terminated == frames + dropped
        assert!(audit_render("10|10|0|0|0|0x0000000000000001").is_ok());
        assert!(audit_render("10|11|0|0|1|0x0000000000000001").is_ok());
        assert_eq!(audit_render("10|9|0|0|0|0x0000000000000001").err(), Some(lb));
        assert_eq!(audit_render("0|0|0|0|0|0x0000000000000001").err(), Some(lb));
        // 守恒成立但终结帧多于总帧数：物理不可能。
        assert_eq!(
            audit_render("0|1|1|0|2|0x0000000000000001").err(),
            Some(BrCode::HandoffBad.val())
        );
    }

    #[test]
    fn golden_is_reproducible() {
        assert_eq!(repro_check(), Ok(()));
        assert_eq!(golden_run(), golden_run());
        match golden_run() {
            Ok(s) => {
                assert!(s.conserved());
                assert!(s.terminated > 0);
                assert!(s.handoffs > 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_bridge_matches_shadow() {
        match fuzz_bridge(128) {
            Ok(s) => {
                assert!(s.conserved());
                assert!(s.frames > 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_handoff_no_leak() {
        match fuzz_handoff(128) {
            Ok(h) => {
                assert!(h.clean());
                assert!(h.handoffs > 0);
                assert!(h.rejected > 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_range_guards() {
        assert_eq!(fuzz_bridge(0).err(), Some(BrCode::CoverBad.val()));
        assert_eq!(fuzz_bridge(FUZZ_ROUNDS + 1).err(), Some(BrCode::CoverBad.val()));
        assert_eq!(fuzz_handoff(0).err(), Some(BrCode::CoverBad.val()));
        assert_eq!(mixed_probe(0).err(), Some(BrCode::CoverBad.val()));
        assert_eq!(session_probe(0).err(), Some(BrCode::CoverBad.val()));
        assert_eq!(batch_probe(0).err(), Some(BrCode::CoverBad.val()));
        assert_eq!(handoff_probe(0).err(), Some(BrCode::CoverBad.val()));
        assert_eq!(seq_class_probe(0).err(), Some(BrCode::CoverBad.val()));
    }

    #[test]
    fn close_emits_cancel() {
        assert_eq!(close_probe(), Ok(5));
    }

    #[test]
    fn detach_terminates_open_session() {
        let mut b = Bridge::new();
        b.attach(DEV_C).expect("attach");
        let f = b.feed(RawEvent::begin(1, 4, 4, DEV_C.id)).expect("begin");
        b.detach().expect("detach");
        match b.last_frame() {
            Some(t) => {
                assert_eq!(t.kind, FrameKind::Cancel);
                assert_eq!(t.session, f.session);
                assert_eq!(t.device, DEV_C.id);
            }
            None => assert!(false),
        }
        assert_eq!(b.ledger().terminated, 1);
        assert_eq!(b.ledger().active, 0);
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn extra_checks_are_exactly_eight() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            match items.get(i) {
                Some((n, p, _)) => {
                    assert!(*p, "{}", n);
                    assert!(n.starts_with("F48991") || n.starts_with("F48992"));
                }
                None => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48991_checks();
        assert!(!s.truncated());
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(s.len(), 8);
        assert_eq!(s.domain, "UNX-F48991");
    }

    // ── F48991-J1：协同 ×10^2 误触 0 ─────────────────────────────
    #[test]
    fn palm_synergy_hundred_scenarios_zero_false_triggers() {
        let p = palm_synergy_probe(PALM_ROUNDS).expect("palm probe");
        assert_eq!(p.rounds, PALM_ROUNDS);
        assert_eq!(p.palms, PALM_ROUNDS / 2);
        assert_eq!(p.genuine, PALM_ROUNDS / 2);
        assert_eq!(p.rejected, p.palms);
        assert_eq!(p.accepted, p.genuine);
        assert_eq!(p.false_triggers, 0, "真手势被误拒");
        assert_eq!(p.missed, 0, "掌团被漏放");
        assert!(p.blob_rejects > 0);
        assert!(p.digest != 0);
        assert_eq!(p.audit(), Ok(()));
        // 每个场景都必须可确定性重建。
        let mut i = 0u32;
        while i < PALM_ROUNDS {
            assert!(palm_scenario(i).is_ok());
            assert_eq!(palm_scenario(i), palm_scenario(i));
            i += 1;
        }
    }

    #[test]
    fn palm_gate_references_b06_and_never_kills_true_gestures() {
        assert_eq!(palm_gate_probe(), Ok(5));
        // 非法的 B06 分数与越界坐标必须拒绝，而不是钳制后静默判。
        assert_eq!(
            PalmSignal::new(true, PALM_SCORE_MAX + 1, 0).err(),
            Some(BrCode::CoverBad.val())
        );
        assert_eq!(
            TouchCluster::new(COORD_MAX + 1, 0, 100, 1).err(),
            Some(BrCode::BridgeBad.val())
        );
        assert_eq!(
            TouchCluster::new(0, 0, 0, 1).err(),
            Some(BrCode::BridgeBad.val())
        );
        assert_eq!(
            PalmGate::new(COORD_MAX + 1, 0).err(),
            Some(BrCode::BridgeBad.val())
        );
        // 相关半径边界：恰在半径上是相关，越 1 格不是。
        let gate = PalmGate::new(PALM_GATE_CX, PALM_GATE_CY).expect("gate");
        let inside = TouchCluster::new(PALM_GATE_CX + CORR_RADIUS, PALM_GATE_CY, 100, 1)
            .expect("inside");
        let outside = TouchCluster::new(PALM_GATE_CX + CORR_RADIUS + 1, PALM_GATE_CY, 100, 1)
            .expect("outside");
        let sig = PalmSignal::new(true, 700, 0).expect("sig");
        assert_eq!(gate.judge(sig, inside), PalmDecision::AcceptCorrelated);
        assert_eq!(gate.judge(sig, outside), PalmDecision::RejectUncorrelated);
    }

    #[test]
    fn palm_probe_range_is_gated() {
        assert_eq!(palm_synergy_probe(0).err(), Some(BrCode::CoverBad.val()));
        assert_eq!(
            palm_synergy_probe(PALM_ROUNDS + 1).err(),
            Some(BrCode::CoverBad.val())
        );
        assert!(palm_synergy_probe(1).is_ok());
    }

    // ── F48992-J1：起止/迁移/取消四点 100% 覆盖 ──────────────────
    #[test]
    fn observation_covers_all_four_points() {
        let o = observe_probe(OBS_ROUNDS).expect("obs probe");
        let cov = o.coverage();
        assert_eq!(cov.present(), OBS_POINTS);
        assert_eq!(cov.percent(), 100);
        assert_eq!(cov.audit(), Ok(()));
        assert_eq!(cov.total, OBS_ROUNDS * 4 - 1);
        let mut i = 0usize;
        while i < OBS_POINTS {
            assert!(cov.counts[i] > 0, "point {} missing", i);
            i += 1;
        }
        // 每类计数与脚本一致：起点 100、迁移 199、取消 50、止 50。
        assert_eq!(o.count(ObsPoint::Start), OBS_ROUNDS);
        assert_eq!(o.count(ObsPoint::Transition), OBS_ROUNDS * 2 - 1);
        assert_eq!(o.count(ObsPoint::Cancel), OBS_ROUNDS / 2);
        assert_eq!(o.count(ObsPoint::End), OBS_ROUNDS / 2);
        assert_eq!(o.anomalies(), 0);
    }

    #[test]
    fn observation_rejects_gaps_and_imbalance() {
        assert_eq!(observation_negative(), Ok(4));
        // 空账与单类独占都拒。
        let empty = Coverage { counts: [0; OBS_POINTS], total: 0, anomalies: 0 };
        assert_eq!(empty.audit(), Err(BrCode::CoverBad.val()));
        let solo = Coverage { counts: [9, 0, 0, 0], total: 9, anomalies: 0 };
        assert_eq!(solo.audit(), Err(BrCode::CoverBad.val()));
        assert_eq!(solo.percent(), 25);
    }

    #[test]
    fn observation_bad_events_are_rejected_and_counted() {
        let mut o = Observer::new();
        o.record(ObsEvent::new(ObsPoint::Start, 1, 1, 10).expect("start"))
            .expect("record start");
        assert_eq!(
            o.record(ObsEvent::new(ObsPoint::Transition, 1, 1, 11).expect("dup"))
                .err(),
            Some(BrCode::SeqBad.val())
        );
        assert_eq!(
            o.record(ObsEvent::new(ObsPoint::Transition, 1, 2, 9).expect("back"))
                .err(),
            Some(BrCode::TimestampBad.val())
        );
        assert_eq!(o.anomalies(), 2);
        assert_eq!(o.total(), 1);
        // 构造期就拒绝非法会话号/序号/时间戳。
        assert_eq!(
            ObsEvent::new(ObsPoint::Start, 0, 1, 1).err(),
            Some(BrCode::SessionBad.val())
        );
        assert_eq!(
            ObsEvent::new(ObsPoint::Start, 1, 0, 1).err(),
            Some(BrCode::SessionBad.val())
        );
        assert_eq!(
            ObsEvent::new(ObsPoint::Start, 1, 1, TICK_CAP + 1).err(),
            Some(BrCode::TimestampBad.val())
        );
        // 新会话首拍必须 seq=1。
        assert_eq!(
            o.record(ObsEvent::new(ObsPoint::Start, 2, 2, 12).expect("s2"))
                .err(),
            Some(BrCode::SeqBad.val())
        );
        assert!(o
            .record(ObsEvent::new(ObsPoint::Start, 2, 1, 13).expect("s2b"))
            .is_ok());
        // 会话号不得回退。
        assert_eq!(
            o.record(ObsEvent::new(ObsPoint::Start, 1, 1, 14).expect("s1"))
                .err(),
            Some(BrCode::SessionBad.val())
        );
        assert_eq!(o.count(ObsPoint::Start), 2);
        assert_eq!(o.anomalies(), 4);
    }

    #[test]
    fn observation_replay_is_byte_identical() {
        let a = observe_replay(OBS_ROUNDS).expect("replay");
        let b = observe_replay(OBS_ROUNDS).expect("replay2");
        assert_eq!(a, b);
        assert!(a.0 != 0);
        assert_eq!(a.1, OBS_ROUNDS * 4 - 1);
        assert_eq!(
            observe_probe(0).err(),
            Some(BrCode::CoverBad.val())
        );
        assert_eq!(
            observe_probe(OBS_ROUNDS + 1).err(),
            Some(BrCode::CoverBad.val())
        );
    }
}

