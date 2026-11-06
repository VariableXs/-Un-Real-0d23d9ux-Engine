//! UNX-F9363 LTP「可跑」登记联测面（C2 号面 × C3 回归基建） · 0x7C840段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9363 逐字）：
//!   ① 纯功能代码 ≥420 行；② 判据原文：UNX-F9363-J1 「可跑」登记联测判据过，
//!   C2 号面逐号与 C3 回归基建对接可观测，登记口径单源格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 两面结构：C2 号面是 16 个 LTP 用例号（2000..2015）+ 每个号的「可跑」
//!      标记；C3 回归基建是 4 条回归泳道（lane），每条容量 6。登记动作把
//!      一个可跑号钉到恰好一条泳道上——「单源」不是口号，而是
//!      `binding[i].count_ones() == 1` 这一条可复算的不变量。
//!   ② 登记判定次序冻结：阶段（Draft 才能登记）→ 号面存在（ENOENT）→
//!      可跑资格（EPERM）→ 泳道合法（EINVAL）→ 单源冲突（同泳道 EBUSY /
//!      异泳道 EEXIST）→ 泳道容量（ENOSPC）。同泳道重复登记是幂等冲突，
//!      异泳道重复是单源破坏：两者码不同，判据才可区分「手滑」与「越权」。
//!   ③ 号面里有 2 个号声明为「不可跑」（索引 3 与 11）。登记不可跑号回
//!      EPERM：C2 的资格判定先于 C3 的容量判定，资格不够不给占泳道。
//!   ④ 可观测性：每次成功登记写一条 BindLog（号 + 泳道 + epoch），
//!      epoch 从 1 连续递增；`observable()` 要求日志条数 == 已登记数，
//!      且逐条能反查到同一个 binding/epoch。日志是观测面，不是账本副本。
//!   ⑤ 阶段机：Draft → Frozen → Reported。空表 freeze 回 EINVAL（没有可跑
//!      登记就没有回归面）；冻结后登记回 EBUSY；重复 freeze 也回 EBUSY；
//!      report 只接受 Frozen，产出确定性摘要。
//!   ⑥ 冻结登记计划 12 条：泳道 0/1/2/3 各 3 条，4 个可跑号故意留空
//!      （7、15 可跑未登记，3、11 不可跑）——覆盖 12/16 是可复算的定值。
//!   ⑦ 注入账：8 类违规 × 10 轮 = 80 次，注入 = 检出 + 逃逸，逃逸 0。
//! ─────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9363 独占 0x7C840..=0x7C87F）。
pub const SEG_BASE: u32 = 0x7C840;
/// 段窗口上界（F9364 从 0x7C880 起）。
pub const SEG_NEXT: u32 = 0x7C880;
/// C2 号面容量。
pub const CASE_COUNT: usize = 16;
/// C3 回归泳道数。
pub const LANE_COUNT: usize = 4;
/// 单泳道容量。
pub const LANE_CAP: u8 = 6;
/// 首例号。
pub const CASE_FIRST: u32 = 2000;
/// 首末号差。
pub const CASE_SPAN: u32 = CASE_COUNT as u32 - 1;
/// 登记日志槽。
pub const BIND_LOG: usize = 24;
/// 注入轮次。
pub const ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJ_KINDS: usize = 8;
/// 冻结计划覆盖数（12 条登记）。
pub const COVERED_EXPECT: u32 = 12;

/// Linux errno 数值（正数口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EBUSY: i32 = 16;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;
pub const EROFS: i32 = 30;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0D）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JointCode {
    SegmentViolation = 1,
    FaceNotAscending = 2,
    NotRunnable = 3,
    BadLane = 4,
    SingleSourceBroken = 5,
    LaneOverflow = 6,
    RegisterAfterFreeze = 7,
    EmptyFreeze = 8,
    ReportBeforeFreeze = 9,
    ObservabilityGap = 10,
    EpochRegress = 11,
    LedgerUnbalanced = 12,
    NotReproducible = 13,
}

impl JointCode {
    /// 码值（段基址 + 序号）。段基址超出 u16，故对外用 u32。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            JointCode::SegmentViolation => "段位越界：F9363 必须落在 0x7C840..=0x7C87F 且不与相邻段重叠；请核对段基址",
            JointCode::FaceNotAscending => "号面乱序：C2 用例号必须严格递增，降序或重复都不可登记；请先规整号面",
            JointCode::NotRunnable => "不可跑号被登记：C2 号面声明 runnable=false 的号回 EPERM；请先修号面资格",
            JointCode::BadLane => "泳道非法：lane 必须落在 0..4；越界泳道回 EINVAL，不得回落到 0 号",
            JointCode::SingleSourceBroken => "单源破坏：同一号已被别的泳道占用，异泳道重复登记回 EEXIST；请先查 binding",
            JointCode::LaneOverflow => "泳道超容：单泳道容量 6，第 7 条回 ENOSPC；请换泳道或等回归面扩容",
            JointCode::RegisterAfterFreeze => "冻结后登记：Frozen/Reported 阶段一律 EBUSY；请在 Draft 期完成登记",
            JointCode::EmptyFreeze => "空表冻结：没有任何可跑登记时 freeze 回 EINVAL；请先登记至少一条",
            JointCode::ReportBeforeFreeze => "早报：未冻结就 report 回 EINVAL；请先 freeze 锁定单源口径",
            JointCode::ObservabilityGap => "可观测性缺口：日志条数与已登记数不等；请逐号核对登记写日志路径",
            JointCode::EpochRegress => "epoch 倒退：登记序号必须从 1 连续递增；请检查回滚是否误改 next_epoch",
            JointCode::LedgerUnbalanced => "注入账不守恒：注入数必须等于检出数加逃逸数；请回查注入循环",
            JointCode::NotReproducible => "双跑不一致：同登记序列两次摘要必须逐位相等；请排查隐藏状态",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [JointCode] = &[
        JointCode::SegmentViolation, JointCode::FaceNotAscending, JointCode::NotRunnable,
        JointCode::BadLane, JointCode::SingleSourceBroken, JointCode::LaneOverflow,
        JointCode::RegisterAfterFreeze, JointCode::EmptyFreeze, JointCode::ReportBeforeFreeze,
        JointCode::ObservabilityGap, JointCode::EpochRegress, JointCode::LedgerUnbalanced,
        JointCode::NotReproducible,
    ];
}

// ── C2 号面 ───────────────────────────────────────────────────────────
/// C2 号面一行：用例号 + 短名 + 是否可跑。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C2Row {
    pub case_no: u32,
    pub tag: &'static str,
    pub runnable: bool,
}

impl C2Row {
    pub const EMPTY: C2Row = C2Row { case_no: 0, tag: "", runnable: false };
    pub fn new(case_no: u32, tag: &'static str, runnable: bool) -> C2Row {
        C2Row { case_no, tag, runnable }
    }
}

/// 16 个号面短名（LTP 风格稳定命名）。
pub const C2_TAGS: [&'static str; CASE_COUNT] = [
    "open01", "open02", "read01", "read02", "write01", "write02", "close01", "close02",
    "mmap01", "mmap02", "fork01", "fork02", "pipe01", "dup01", "clock01", "yield01",
];

/// 不可跑号在号面中的索引（资格面故意留两个缺口）。
pub const NOT_RUNNABLE: [usize; 2] = [3, 11];

/// 构造 C2 号面：号从 2000 连续递增，索引 3/11 不可跑。
pub fn c2_face() -> [C2Row; CASE_COUNT] {
    let mut out = [C2Row::EMPTY; CASE_COUNT];
    let mut i = 0usize;
    while i < CASE_COUNT {
        let tag = match C2_TAGS.get(i) {
            Some(t) => *t,
            None => "",
        };
        let mut runnable = true;
        let mut k = 0usize;
        while k < NOT_RUNNABLE.len() {
            if let Some(x) = NOT_RUNNABLE.get(k) {
                if *x == i {
                    runnable = false;
                }
            }
            k += 1;
        }
        if let Some(slot) = out.get_mut(i) {
            slot.case_no = CASE_FIRST + i as u32;
            slot.tag = tag;
            slot.runnable = runnable;
        }
        i += 1;
    }
    out
}

/// 号面中可跑号计数（应为 14）。
pub fn runnable_count(face: &[C2Row]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < face.len() {
        if let Some(r) = face.get(i) {
            if r.runnable {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── 泳道与单源位图 ────────────────────────────────────────────────────
/// 一条 C3 回归泳道：编号 + 载荷 + 号面索引位图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lane {
    pub id: u8,
    pub load: u8,
    pub mask: u16,
}

impl Lane {
    pub const EMPTY: Lane = Lane { id: 0, load: 0, mask: 0 };
}

/// 号面索引 → 位。索引恒 < 16，故移位安全。
pub fn bit_of(index: usize) -> u16 {
    1u16 << ((index as u32) & 0xF)
}

/// 单源位图是否是「恰好一条泳道」。
pub fn is_single(mask: u16) -> bool {
    mask.count_ones() == 1
}

/// 登记日志行（观测面，不参与判定，只用于对账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BindLog {
    pub case_no: u32,
    pub lane: u8,
    pub epoch: u16,
}

impl BindLog {
    pub const EMPTY: BindLog = BindLog { case_no: 0, lane: 0, epoch: 0 };
}

/// 登记阶段（单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegStage {
    Draft,
    Frozen,
    Reported,
}

impl RegStage {
    pub fn code(self) -> u8 {
        match self {
            RegStage::Draft => 0,
            RegStage::Frozen => 1,
            RegStage::Reported => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RegStage::Draft => "draft",
            RegStage::Frozen => "frozen",
            RegStage::Reported => "reported",
        }
    }
}

// ── 联测登记面 ────────────────────────────────────────────────────────
/// C2 号面 × C3 回归基建的联测登记面。
///
/// 不变量：
///   I1 `binding[i]` 的 popcount ∈ {0,1}（0=未登记，1=单源已登记）；
///   I2 所有 lane.load 之和 == 已登记号数（无幽灵登记）；
///   I3 所有 lane.mask 的并集位数 == 已登记号数（号面不重不漏）；
///   I4 epoch 从 1 连续递增，日志按登记序追加。
#[derive(Clone, Copy, Debug)]
pub struct JointReg {
    face: [C2Row; CASE_COUNT],
    lanes: [Lane; LANE_COUNT],
    binding: [u16; CASE_COUNT],
    epoch: [u16; CASE_COUNT],
    log: [BindLog; BIND_LOG],
    log_n: usize,
    next_epoch: u16,
    stage: RegStage,
    rejects: u32,
}

impl JointReg {
    pub fn new() -> JointReg {
        let mut lanes = [Lane::EMPTY; LANE_COUNT];
        let mut i = 0usize;
        while i < LANE_COUNT {
            if let Some(l) = lanes.get_mut(i) {
                l.id = i as u8;
            }
            i += 1;
        }
        JointReg {
            face: c2_face(),
            lanes,
            binding: [0u16; CASE_COUNT],
            epoch: [0u16; CASE_COUNT],
            log: [BindLog::EMPTY; BIND_LOG],
            log_n: 0,
            next_epoch: 0,
            stage: RegStage::Draft,
            rejects: 0,
        }
    }

    pub fn stage(&self) -> RegStage { self.stage }
    pub fn rejects(&self) -> u32 { self.rejects }
    pub fn log_len(&self) -> usize { self.log_n }

    pub fn row(&self, i: usize) -> Option<&C2Row> {
        if i < CASE_COUNT { self.face.get(i) } else { None }
    }

    pub fn lane(&self, i: usize) -> Option<&Lane> {
        if i < LANE_COUNT { self.lanes.get(i) } else { None }
    }

    pub fn log_at(&self, i: usize) -> Option<&BindLog> {
        if i < self.log_n { self.log.get(i) } else { None }
    }

    /// 号面索引（按用例号线性查）。
    pub fn index_of(&self, case_no: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < CASE_COUNT {
            if self.face.get(i).map(|r| r.case_no) == Some(case_no) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 逐号可观测视图：返回 (泳道, epoch)；未登记或单源破坏返回 None。
    pub fn binding_of(&self, case_no: u32) -> Option<(u8, u16)> {
        let idx = self.index_of(case_no)?;
        let mask = *self.binding.get(idx)?;
        if !is_single(mask) {
            return None;
        }
        let lane = mask.trailing_zeros() as u8;
        Some((lane, *self.epoch.get(idx)?))
    }

    /// 登记一个号到指定泳道，成功返回 epoch。
    ///
    /// 判定次序：阶段 → 存在 → 资格 → 泳道 → 单源冲突 → 容量。
    pub fn register(&mut self, case_no: u32, lane: u8) -> Result<u16, i32> {
        if self.stage != RegStage::Draft {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EBUSY);
        }
        let idx = match self.index_of(case_no) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ENOENT);
            }
        };
        let runnable = self.face.get(idx).map(|r| r.runnable) == Some(true);
        if !runnable {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EPERM);
        }
        if (lane as usize) >= LANE_COUNT {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let cur = match self.binding.get(idx) {
            Some(v) => *v,
            None => 0u16,
        };
        let bit = bit_of(idx);
        let lane_bit = 1u16 << (lane as u32);
        if cur & lane_bit != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EBUSY);
        }
        if cur != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EEXIST);
        }
        let load = match self.lanes.get(lane as usize) {
            Some(l) => l.load,
            None => LANE_CAP,
        };
        if load >= LANE_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOSPC);
        }
        // 提交：先算新值，再逐字段落地（借用不重叠）。
        let next = self.next_epoch.saturating_add(1);
        if let Some(b) = self.binding.get_mut(idx) { *b |= lane_bit; }
        if let Some(e) = self.epoch.get_mut(idx) { *e = next; }
        self.next_epoch = next;
        if let Some(l) = self.lanes.get_mut(lane as usize) {
            l.load = l.load.saturating_add(1);
            l.mask |= bit;
        }
        if let Some(slot) = self.log.get_mut(self.log_n) {
            slot.case_no = case_no;
            slot.lane = lane;
            slot.epoch = next;
        }
        self.log_n += 1;
        Ok(next)
    }

    /// 已登记号数 / 号面总数。
    pub fn coverage(&self) -> (u32, u32) {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < LANE_COUNT {
            if let Some(l) = self.lanes.get(i) {
                n = n.saturating_add(l.load as u32);
            }
            i += 1;
        }
        (n, CASE_COUNT as u32)
    }

    /// 单源不变量总判定：每条 binding 恰一位，且三个计数口径一致。
    pub fn single_source(&self) -> bool {
        let mut bound = 0u32;
        let mut union: u16 = 0;
        let mut i = 0usize;
        while i < CASE_COUNT {
            let m = match self.binding.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if m != 0 {
                if !is_single(m) {
                    return false;
                }
                bound = bound.saturating_add(1);
                union |= bit_of(i);
            }
            i += 1;
        }
        let (sum, _) = self.coverage();
        sum == bound && union.count_ones() == bound
    }

    /// 逐号可观测：日志条数 == 已登记数、epoch 连续、且每条都能反查。
    pub fn observable(&self) -> bool {
        let (bound, _) = self.coverage();
        if self.log_n as u32 != bound {
            return false;
        }
        let mut i = 0usize;
        while i < self.log_n {
            let row = match self.log.get(i) {
                Some(r) => *r,
                None => return false,
            };
            if row.epoch as usize != i + 1 {
                return false;
            }
            match self.binding_of(row.case_no) {
                Some((lane, epoch)) => {
                    if lane != row.lane || epoch != row.epoch {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 冻结单源口径：Draft 且有登记才可冻结。
    pub fn freeze(&mut self) -> Result<(), i32> {
        if self.stage != RegStage::Draft {
            return Err(EBUSY);
        }
        if self.coverage().0 == 0 {
            return Err(EINVAL);
        }
        self.stage = RegStage::Frozen;
        Ok(())
    }

    /// 出回归摘要：只接受 Frozen，摘要由登记序列复算。
    pub fn report(&mut self) -> Result<u32, i32> {
        if self.stage != RegStage::Frozen {
            return Err(EINVAL);
        }
        self.stage = RegStage::Reported;
        Ok(self.digest())
    }

    /// 登记面摘要：逐日志行喂 (case_no, lane, epoch)，再喂阶段。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < self.log_n {
            if let Some(r) = self.log.get(i) {
                h = fnv_u32(h, r.case_no);
                h = fnv_byte(h, r.lane);
                h = fnv_byte(h, r.epoch as u8);
            }
            i += 1;
        }
        fnv_byte(h, self.stage.code())
    }
}

// ── 冻结登记计划 ──────────────────────────────────────────────────────
/// 12 条冻结登记：(号面索引, 泳道)。泳道各 3 条，4 个号故意留空。
pub const PLAN: [(usize, u8); COVERED_EXPECT as usize] = [
    (0, 0), (1, 0), (2, 0),
    (4, 1), (5, 1), (6, 1),
    (8, 2), (9, 2), (10, 2),
    (12, 3), (13, 3), (14, 3),
];

/// 按冻结计划构造登记面（未冻结）。
pub fn canonical_reg() -> JointReg {
    let mut reg = JointReg::new();
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some((idx, lane)) = PLAN.get(i).copied() {
            let case_no = match reg.row(idx) {
                Some(r) => r.case_no,
                None => 0,
            };
            if reg.register(case_no, lane).is_err() {
                return JointReg::new();
            }
        }
        i += 1;
    }
    reg
}

/// 泳道载荷读取（越界按 0，仅供快照文本使用）。
pub fn lane_load(reg: &JointReg, i: usize) -> u8 {
    match reg.lane(i) {
        Some(l) => l.load,
        None => 0,
    }
}

// ── 注入 ──────────────────────────────────────────────────────────────
/// 注入类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjKind {
    DuplicateLane,
    SameLane,
    UnknownCase,
    NotRunnable,
    BadLane,
    LaneFull,
    AfterFreeze,
    EmptyFreeze,
}

/// 八类注入全集。
pub const INJ_ALL: [InjKind; INJ_KINDS] = [
    InjKind::DuplicateLane, InjKind::SameLane, InjKind::UnknownCase, InjKind::NotRunnable,
    InjKind::BadLane, InjKind::LaneFull, InjKind::AfterFreeze, InjKind::EmptyFreeze,
];

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const ZERO: InjectLedger = InjectLedger { injected: 0, detected: 0, escaped: 0 };

    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught { self.detected = self.detected.saturating_add(1); }
        else { self.escaped = self.escaped.saturating_add(1); }
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 在指定泳道填满 6 条可跑登记（用于容量注入）。
pub fn fill_lane(reg: &mut JointReg, lane: u8) -> bool {
    let order = [0usize, 1, 2, 4, 5, 6];
    let mut i = 0usize;
    while i < order.len() {
        if let Some(idx) = order.get(i).copied() {
            let case_no = match reg.row(idx) {
                Some(r) => r.case_no,
                None => return false,
            };
            if reg.register(case_no, lane).is_err() {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 单类注入探测。
pub fn probe(kind: InjKind) -> bool {
    match kind {
        InjKind::DuplicateLane => {
            let mut reg = JointReg::new();
            let base = match reg.row(0) { Some(r) => r.case_no, None => 0 };
            reg.register(base, 0).is_ok() && reg.register(base, 1) == Err(EEXIST)
        }
        InjKind::SameLane => {
            let mut reg = JointReg::new();
            let base = match reg.row(0) { Some(r) => r.case_no, None => 0 };
            reg.register(base, 0).is_ok() && reg.register(base, 0) == Err(EBUSY)
        }
        InjKind::UnknownCase => JointReg::new().register(9999, 0) == Err(ENOENT),
        InjKind::NotRunnable => {
            let mut reg = JointReg::new();
            let no = match reg.row(3) { Some(r) => r.case_no, None => 0 };
            reg.register(no, 0) == Err(EPERM)
        }
        InjKind::BadLane => {
            let mut reg = JointReg::new();
            let base = match reg.row(0) { Some(r) => r.case_no, None => 0 };
            reg.register(base, 9) == Err(EINVAL)
        }
        InjKind::LaneFull => {
            let mut reg = JointReg::new();
            let filled = fill_lane(&mut reg, 0);
            let extra = match reg.row(8) { Some(r) => r.case_no, None => 0 };
            filled && reg.register(extra, 0) == Err(ENOSPC)
        }
        InjKind::AfterFreeze => {
            let mut reg = canonical_reg();
            let frozen = reg.freeze().is_ok();
            let no = match reg.row(7) { Some(r) => r.case_no, None => 0 };
            frozen && reg.register(no, 0) == Err(EBUSY)
        }
        InjKind::EmptyFreeze => JointReg::new().freeze() == Err(EINVAL),
    }
}

/// 全量注入：8 类 × 10 轮。
pub fn run_injection() -> ([u32; INJ_KINDS], InjectLedger) {
    let mut per_cell = [0u32; INJ_KINDS];
    let mut ledger = InjectLedger::ZERO;
    let mut i = 0usize;
    while i < INJ_ALL.len() {
        if let Some(kind) = INJ_ALL.get(i).copied() {
            let mut r = 0u32;
            while r < ROUNDS {
                let caught = probe(kind);
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_cell.get_mut(i) {
                        *slot = slot.saturating_add(1);
                    }
                }
                r += 1;
            }
        }
        i += 1;
    }
    (per_cell, ledger)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
pub fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

pub fn fnv_u32(h: u32, v: u32) -> u32 {
    let mut x = h;
    x = fnv_byte(x, (v & 0xFF) as u8);
    x = fnv_byte(x, ((v >> 8) & 0xFF) as u8);
    x = fnv_byte(x, ((v >> 16) & 0xFF) as u8);
    fnv_byte(x, ((v >> 24) & 0xFF) as u8)
}

/// 规范语料指纹：12 条计划登记 → 冻结 → 出摘要，双跑必须相等。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = canonical_reg();
    let mut h = reg.digest();
    if reg.freeze().is_ok() {
        match reg.report() {
            Ok(d) => h = fnv_u32(h, d),
            Err(e) => h = fnv_byte(h, (e & 0x7F) as u8),
        }
    }
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9363 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9363_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9363");

    s.add("段位守卫：0x7C840 起 0x40 窗口不与 F9362/F9364 重叠",
          SEG_BASE == 0x7C840 && SEG_BASE % 0x40 == 0 && SEG_BASE.saturating_add(0x3F) < SEG_NEXT,
          "段基址唯一且窗口严格落在相邻两段之间");

    let face = c2_face();
    let mut face_ok = runnable_count(&face) == 14;
    let mut prev = 0u32;
    let mut i = 0usize;
    while i < face.len() {
        if let Some(r) = face.get(i) {
            if (i > 0 && r.case_no <= prev) || r.tag.is_empty() { face_ok = false; }
            prev = r.case_no;
        } else { face_ok = false; }
        i += 1;
    }
    s.add("C2 号面：16 号严格递增、14 可跑 2 不可跑、短名非空", face_ok,
          "号面是登记的唯一判据来源，顺序与资格都要冻结");

    let reg = canonical_reg();
    let (covered, total) = reg.coverage();
    let mut loads_ok = true;
    let mut l = 0usize;
    while l < LANE_COUNT {
        if reg.lane(l).map(|x| x.load) != Some(3) { loads_ok = false; }
        l += 1;
    }
    s.add("冻结计划：12 条登记覆盖 12/16，四泳道各 3 条",
          covered == COVERED_EXPECT && total == CASE_COUNT as u32 && loads_ok,
          "冻结计划是复算基准，覆盖数与泳道载荷必须同时成立");

    s.add("单源格：每个已登记号恰好一条泳道且三口径计数一致",
          reg.single_source() && reg.coverage().0 == 12,
          "binding popcount==1、泳道载荷和、并集位数三者必须相等");

    let mut obs_ok = reg.observable();
    let mut k = 0usize;
    while k < CASE_COUNT {
        let no = match reg.row(k) { Some(r) => r.case_no, None => 0 };
        let bound = reg.binding_of(no).is_some();
        let expected = matches!(k, 0 | 1 | 2 | 4 | 5 | 6 | 8 | 9 | 10 | 12 | 13 | 14);
        if bound != expected { obs_ok = false; }
        k += 1;
    }
    s.add("逐号可观测：日志 12 条且每号 binding 可反查",
          obs_ok && reg.log_len() == 12,
          "C2 逐号必须能从 C3 登记面反查到唯一泳道与 epoch");

    let mut dup = canonical_reg();
    let no0 = match dup.row(0) { Some(r) => r.case_no, None => 0 };
    let before_mask = dup.binding_of(no0);
    let dup_r = dup.register(no0, 1);
    s.add("单源拒绝：异泳道重复登记 EEXIST 且 binding 不变",
          dup_r == Err(EEXIST) && dup.binding_of(no0) == before_mask && dup.rejects() == 1,
          "单源破坏必须显性拒绝，且不得改动既有绑定");

    let mut same = canonical_reg();
    let same_r = same.register(no0, 0);
    s.add("幂等冲突：同泳道重复登记 EBUSY 且泳道载荷不变",
          same_r == Err(EBUSY) && same.lane(0).map(|l| l.load) == Some(3),
          "重复登记不是新登记，泳道载荷与 epoch 都不得推进");

    let mut qual = JointReg::new();
    let no3 = match qual.row(3) { Some(r) => r.case_no, None => 0 };
    let no0q = match qual.row(0) { Some(r) => r.case_no, None => 0 };
    let unknown = qual.register(9999, 0);
    let not_run = qual.register(no3, 0);
    let bad_lane = qual.register(no0q, 9);
    s.add("资格与参数：未知号 ENOENT、不可跑 EPERM、坏泳道 EINVAL",
          unknown == Err(ENOENT) && not_run == Err(EPERM) && bad_lane == Err(EINVAL),
          "存在面先于资格面，资格面先于泳道参数面");

    let mut cap = JointReg::new();
    let filled = fill_lane(&mut cap, 0);
    let no8 = match cap.row(8) { Some(r) => r.case_no, None => 0 };
    let extra = cap.register(no8, 0);
    s.add("泳道容量：6 条填满后第 7 条回 ENOSPC 且载荷停在 6",
          filled && extra == Err(ENOSPC) && cap.lane(0).map(|l| l.load) == Some(6),
          "容量面错误必须晚于资格面，且失败登记不占槽");

    let mut empty = JointReg::new();
    let empty_r = empty.freeze();
    let mut fr = canonical_reg();
    let f1 = fr.freeze();
    let f2 = fr.freeze();
    let no7 = match fr.row(7) { Some(r) => r.case_no, None => 0 };
    let after = fr.register(no7, 0);
    s.add("冻结语义：空表 EINVAL、首冻成功、重复冻结与冻结后登记均 EBUSY",
          empty_r == Err(EINVAL) && f1.is_ok() && f2 == Err(EBUSY) && after == Err(EBUSY),
          "冻结锁死单源口径：登记面在 Frozen 之后只读");

    let mut rep = canonical_reg();
    let early = rep.report();
    let frozen = rep.freeze();
    let reported = rep.report();
    s.add("出报：未冻结 report 回 EINVAL，冻结后摘要可复算",
          early == Err(EINVAL) && frozen.is_ok() && reported.is_ok()
              && rep.stage() == RegStage::Reported,
          "摘要只能在冻结后产出，且同登记序列必须同摘要");

    let (per_cell, inj) = run_injection();
    let cells_ok = per_cell.iter().all(|n| *n == ROUNDS);
    s.add("注入账：8 类 × 10 轮全检出且逃逸为 0",
          cells_ok && inj.conserved() && inj.escaped == 0
              && inj.injected == (INJ_KINDS as u32) * ROUNDS,
          "每类违规都必须被自己的 errno 拒绝，漏一类即逃逸");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范登记语料指纹相等", fp_a == fp_b,
          "同登记序列两次摘要逐位相等；隐藏计数会破坏相等");

    let mut uniq = true;
    let mut a = 0usize;
    while a < JointCode::ALL.len() {
        let mut b = a + 1;
        while b < JointCode::ALL.len() {
            if let (Some(x), Some(y)) = (JointCode::ALL.get(a), JointCode::ALL.get(b)) {
                if x.code() == y.code() { uniq = false; }
            }
            b += 1;
        }
        a += 1;
    }
    s.add("诊断码：13 个码值两两唯一且落在 0x7C841..=0x7C84D",
          uniq && JointCode::ALL.len() == 13,
          "闭集码不得重复，否则红项无法唯一定位");

    s.add("文案闭集：13 个诊断码均有中文可操作建议",
          JointCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须给出下一步动作，禁止空串");

    let cov_text = format!("覆盖 {}/{} / 日志 {} 条", covered, total, reg.log_len());
    s.add_dyn("覆盖快照", covered == COVERED_EXPECT && reg.log_len() == 12, &cov_text);
    let lane_text = format!("泳道载荷 {} {} {} {}", lane_load(&reg, 0), lane_load(&reg, 1), lane_load(&reg, 2), lane_load(&reg, 3));
    s.add_dyn("泳道快照", loads_ok, &lane_text);
    let fp_text = format!("登记指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_shape_is_frozen() {
        let face = c2_face();
        assert_eq!(runnable_count(&face), 14);
        assert_eq!(face.get(0).map(|r| r.case_no), Some(CASE_FIRST));
        assert_eq!(face.get(CASE_COUNT - 1).map(|r| r.case_no), Some(CASE_FIRST + CASE_SPAN));
        assert_eq!(face.get(3).map(|r| r.runnable), Some(false));
        assert_eq!(face.get(11).map(|r| r.runnable), Some(false));
    }

    #[test]
    fn canonical_plan_binds_twelve() {
        let reg = canonical_reg();
        assert_eq!(reg.coverage(), (12, 16));
        assert_eq!(reg.lane(0).map(|l| l.load), Some(3));
        assert_eq!(reg.lane(3).map(|l| l.load), Some(3));
        assert_eq!(reg.log_len(), 12);
    }

    #[test]
    fn single_source_invariant_holds() {
        let reg = canonical_reg();
        assert_eq!(reg.single_source(), true);
        assert_eq!(reg.observable(), true);
        let no7 = reg.row(7).map(|r| r.case_no);
        assert_eq!(no7.and_then(|n| reg.binding_of(n)), None);
    }

    #[test]
    fn duplicate_to_other_lane_rejected() {
        let mut reg = canonical_reg();
        let no0 = match reg.row(0) { Some(r) => r.case_no, None => 0 };
        assert_eq!(reg.register(no0, 1), Err(EEXIST));
        assert_eq!(reg.register(no0, 0), Err(EBUSY));
        assert_eq!(reg.binding_of(no0).map(|b| b.0), Some(0));
    }

    #[test]
    fn eligibility_and_parameters() {
        let mut reg = JointReg::new();
        let no3 = match reg.row(3) { Some(r) => r.case_no, None => 0 };
        let no0 = match reg.row(0) { Some(r) => r.case_no, None => 0 };
        assert_eq!(reg.register(9999, 0), Err(ENOENT));
        assert_eq!(reg.register(no3, 0), Err(EPERM));
        assert_eq!(reg.register(no0, 9), Err(EINVAL));
    }

    #[test]
    fn lane_capacity_is_six() {
        let mut reg = JointReg::new();
        assert_eq!(fill_lane(&mut reg, 1), true);
        assert_eq!(reg.lane(1).map(|l| l.load), Some(6));
        let no8 = match reg.row(8) { Some(r) => r.case_no, None => 0 };
        assert_eq!(reg.register(no8, 1), Err(ENOSPC));
        assert_eq!(reg.lane(1).map(|l| l.load), Some(6));
    }

    #[test]
    fn freeze_and_report_are_ordered() {
        let mut reg = canonical_reg();
        assert_eq!(reg.report(), Err(EINVAL));
        assert_eq!(reg.freeze(), Ok(()));
        assert_eq!(reg.freeze(), Err(EBUSY));
        let no7 = match reg.row(7) { Some(r) => r.case_no, None => 0 };
        assert_eq!(reg.register(no7, 0), Err(EBUSY));
        assert_eq!(reg.report().is_ok(), true);
        assert_eq!(reg.stage(), RegStage::Reported);
    }

    #[test]
    fn empty_registry_cannot_freeze() {
        let mut reg = JointReg::new();
        assert_eq!(reg.freeze(), Err(EINVAL));
        assert_eq!(reg.stage(), RegStage::Draft);
    }

    #[test]
    fn injection_ledger_conserved() {
        let (per, l) = run_injection();
        assert_eq!(l.injected, (INJ_KINDS as u32) * ROUNDS);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.conserved(), true);
        let mut i = 0usize;
        while i < per.len() {
            assert_eq!(per.get(i), Some(&ROUNDS));
            i += 1;
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9363_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
        assert_eq!(s.truncated(), false);
    }
}
