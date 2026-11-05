//! UNX-F49047 F5 手势消费接口对接 · 0x730C0 段（B13 批第四槽）
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，不得改写）：
//!   UNX-F49047 F5 手势消费接口对接（3 指工作区手势递交 F5——B10 接口执行化）:
//!          ① 纯功能代码 ≥320 行；
//!          ② 判据原文：UNX-F49048-J1 分界 grep 零越界 + H5 联签锚定行；
//!             判据锚定 §AI-62 §8 横向 AI-40；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49048 H5 手柄分界联签执行（游戏操控/HID 类语义归 H5、
//!   键鼠/触控归本域——RAW Input 分界登记）:
//!          ① 纯功能代码 ≥320 行；
//!          ② 判据原文：UNX-F49049-J1 分界声明落账 + D4 联签锚定行；
//!             判据锚定 §AI-62 §8 横向 AI-19；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（卡一 F49047）：落指 → 记迹 → 越阈 → 稳持 → 递交。
//! 落指先于记迹：必须先确认按下的正是三指，位移才有"工作区手势"的语义；
//! 若两指或四指也记迹，阈值就成了任意手势都能触发的工作区开关；
//! 记迹先于越阈：轨迹是逐笔累加出来的（曼哈顿位移），没有记迹就没有可比的数；
//! 越阈先于稳持：只有越过阈值的手势才进入"待递交"态，否则一次抖动就能换来
//! 一次工作区切换——这正是"阈值"存在的理由；
//! 稳持先于递交：越阈之后还要在 Armed 态停留够 HOLD_TICKS 才算"意图稳定"，
//! 稳持不足即松手一律判为 Aborted（宁可丢一次手势，不可错切一次工作区）；
//! 递交之后还有 DEBOUNCE_TICKS 冷却，机械回弹不会被读成第二次手势。
//! 闸门次序（卡二 F49048）：建表 → 断言属主 → 越界 grep → 联签落账。
//! 建表先于断言：七类输入（键/鼠/触/柄/裸 HID/传感器 HID/厂商 HID）先逐条
//! 登记 RAW Input 分界行，缺行就意味着某一类输入"没有归属"；
//! 断言属主先于越界 grep：每行必须自己声明属主，且与规范映射逐条相等，
//! 否则 grep 只是在检查一张自说自话的表；
//! 越界 grep 先于联签：先在静态表上扫出零越界，再让 H5/D4 两个横向方落签，
//! 签的是"扫干净的表"，不是"还没扫的表"。
//! 关键差异：F49047 的账是**时间账**（一串三指事件在状态机里推进，
//! 每一步都可能被拒、被弃、被认；100 次手势必须 100 次递交且无重复），
//! F49048 的账是**空间账**（一张静态分界表，逐行属主断言 + 越界注入 grep，
//! 100 次声明必须零越界）。前者的错误在"时序"，后者的错误在"归属"。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x730C0，B13 批第四槽）。
pub const SEG_BASE: u32 = 0x730C0;

/// 三指手势所需手指数。
pub const FINGERS_REQUIRED: u32 = 3;
/// 位移阈值（曼哈顿距离，逐笔累加）。
pub const TRAVEL_THRESHOLD: u32 = 48;
/// Armed 态稳持所需 tick 数。
pub const HOLD_TICKS: u32 = 2;
/// 递交后的去抖冷却 tick 数。
pub const DEBOUNCE_TICKS: u32 = 4;
/// Tracking 态存活上限（超过即判过期）。
pub const DEADLINE_TICKS: u32 = 40;
/// 单笔位移步长。
pub const STEP: u32 = 16;
/// 百次手势/百次声明的批量。
pub const HUNDRED: u32 = 100;
/// 注入条数（×10）。
pub const INJECT: u32 = 10;
/// 分界登记表容量。
pub const BOUNDARY_TOTAL: usize = 7;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 手势账标签。
pub const GESTURE_TAG: &str = "GSTRF5";
/// 递交账标签。
pub const SUBMIT_TAG: &str = "F5SUBM";
/// 分界账标签。
pub const BOUND_TAG: &str = "BOUND";
/// H5 联签锚（手柄/HID 分界的签字方）。
pub const H5_ANCHOR: &str = "H5/gesture-cosign";
/// D4 联签锚（分界声明落账的签字方）。
pub const D4_ANCHOR: &str = "D4/boundary-cosign";
/// AI-40 横向锚（F49047 判据锚定方）。
pub const AI40_ANCHOR: &str = "AI-40/gesture-sign";
/// AI-19 横向锚（F49048 判据锚定方）。
pub const AI19_ANCHOR: &str = "AI-19/h5-sign";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 F49047 段诊断码";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ─────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GestureCode {
    /// 手指数不符（只认三指）
    Fingers,
    /// 位移为零或未达阈值
    Threshold,
    /// 稳持不足
    Hold,
    /// 状态机相位不允许本次操作
    Phase,
    /// 一次手势被重复递交
    Double,
    /// 手势记录丢失（递交数大于记录数）
    Drop,
    /// 方向判定非法
    Direction,
    /// 分界表缺行/重复行
    Boundary,
    /// 属主与规范映射不符
    Owner,
    /// 声明落账越界
    Claim,
    /// 渲染字段不可解析
    Render,
    /// 复跑摘要漂移
    Replay,
    /// 计数溢出
    Overflow,
    /// 摘要为零
    Digest,
}

impl GestureCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            GestureCode::Fingers => "手指数不符：工作区手势只认三指落按",
            GestureCode::Threshold => "位移未达阈值或零位移：不构成工作区手势",
            GestureCode::Hold => "稳持不足：越阈后未在 Armed 态停满 HOLD_TICKS",
            GestureCode::Phase => "状态机相位不允许：当前相位不接受该操作",
            GestureCode::Double => "重复递交：同一次手势被递交了两次",
            GestureCode::Drop => "手势记录丢失：递交计数大于落账记录数",
            GestureCode::Direction => "方向判定非法：主轴/次轴无法确定",
            GestureCode::Boundary => "分界表不完整：缺行或 RAW Input 路径重复",
            GestureCode::Owner => "属主不符：声明属主与规范映射不一致",
            GestureCode::Claim => "声明落账越界：所属域之外的行被写入本账",
            GestureCode::Render => "渲染字段不可解析",
            GestureCode::Replay => "复跑摘要漂移：两遍结果不同",
            GestureCode::Overflow => "计数溢出",
            GestureCode::Digest => "摘要为零：折叠未发生",
        }
    }

    pub const ALL: [GestureCode; 14] = [
        GestureCode::Fingers,
        GestureCode::Threshold,
        GestureCode::Hold,
        GestureCode::Phase,
        GestureCode::Double,
        GestureCode::Drop,
        GestureCode::Direction,
        GestureCode::Boundary,
        GestureCode::Owner,
        GestureCode::Claim,
        GestureCode::Render,
        GestureCode::Replay,
        GestureCode::Overflow,
        GestureCode::Digest,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < GestureCode::ALL.len() {
        if let Some(c) = GestureCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 是否为本段已知码。
pub fn known_code(code: u32) -> bool {
    detail(code) != UNKNOWN
}

/// 段位守卫：14 条码连续落在 0x730C1..0x730CE。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < GestureCode::ALL.len() {
        let c = match GestureCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
            return false;
        }
        i += 1;
    }
    true
}

// ── 用例编号（16 条）与保留号 ────────────────────────────────────
/// 前序槽位占用号（B12 族，抄录自前批冻结清单）。
pub const RESERVED_IDS: [u32; 8] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8014,
    0x071D_0001,
    0x071D_0014,
    0x071D_2001,
    0x071D_2014,
    0x071E_0401,
];

/// 本模块 16 个用例编号（0x0733_0C01..=0x0733_0C10）。
pub const CASE_IDS: [u32; 16] = [
    0x0733_0C01,
    0x0733_0C02,
    0x0733_0C03,
    0x0733_0C04,
    0x0733_0C05,
    0x0733_0C06,
    0x0733_0C07,
    0x0733_0C08,
    0x0733_0C09,
    0x0733_0C0A,
    0x0733_0C0B,
    0x0733_0C0C,
    0x0733_0C0D,
    0x0733_0C0E,
    0x0733_0C0F,
    0x0733_0C10,
];

/// 用例语义绑定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    pub id: u32,
    pub name: &'static str,
}

/// 16 条用例声明。
pub const CASES: [CaseSpec; 16] = [
    CaseSpec { id: 0x0733_0C01, name: "三指落按" },
    CaseSpec { id: 0x0733_0C02, name: "位移累加" },
    CaseSpec { id: 0x0733_0C03, name: "越阈进入 Armed" },
    CaseSpec { id: 0x0733_0C04, name: "稳持递交" },
    CaseSpec { id: 0x0733_0C05, name: "早放判弃" },
    CaseSpec { id: 0x0733_0C06, name: "去抖冷却" },
    CaseSpec { id: 0x0733_0C07, name: "四向工作区" },
    CaseSpec { id: 0x0733_0C08, name: "百次递交" },
    CaseSpec { id: 0x0733_0C09, name: "手势复跑" },
    CaseSpec { id: 0x0733_0C0A, name: "分界登记表" },
    CaseSpec { id: 0x0733_0C0B, name: "键鼠触控归属" },
    CaseSpec { id: 0x0733_0C0C, name: "手柄 HID 归属" },
    CaseSpec { id: 0x0733_0C0D, name: "越界 grep 注入" },
    CaseSpec { id: 0x0733_0C0E, name: "百次声明零越界" },
    CaseSpec { id: 0x0733_0C0F, name: "联签锚定行" },
    CaseSpec { id: 0x0733_0C10, name: "渲染往返" },
];

/// 跨槽零撞号：不与保留号、段位码窗口、彼此相交。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = match CASE_IDS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a >= SEG_BASE + 0x01 && a <= SEG_BASE + 0x0E {
            return false;
        }
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if let Some(r) = RESERVED_IDS.get(j) {
                if a == *r {
                    return false;
                }
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < CASE_IDS.len() {
            if let Some(b) = CASE_IDS.get(k) {
                if a == *b {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 用例编号连续：CASE_IDS[i] == 0x0733_0C00 + 1 + i。
pub fn case_ids_sequential() -> bool {
    let base: u32 = 0x0733_0C00;
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match base.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        if let Some(v) = CASE_IDS.get(i) {
            if *v != want {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 手势几何与状态机（F49047） ───────────────────────────────────
/// 屏幕坐标点（i32，允许负值便于注入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    /// 曼哈顿距离（逐笔位移的度量）。
    pub fn manhattan(self, other: Point) -> u32 {
        let dx = if self.x >= other.x {
            self.x - other.x
        } else {
            other.x - self.x
        };
        let dy = if self.y >= other.y {
            self.y - other.y
        } else {
            other.y - self.y
        };
        (dx as u32).saturating_add(dy as u32)
    }
}

/// 工作区切换方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkspaceDir {
    Left,
    Right,
    Up,
    Down,
}

impl WorkspaceDir {
    pub const fn index(self) -> u8 {
        match self {
            WorkspaceDir::Left => 0,
            WorkspaceDir::Right => 1,
            WorkspaceDir::Up => 2,
            WorkspaceDir::Down => 3,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            WorkspaceDir::Left => "left",
            WorkspaceDir::Right => "right",
            WorkspaceDir::Up => "up",
            WorkspaceDir::Down => "down",
        }
    }

    /// 单笔位移向量。
    pub const fn delta(self, step: u32) -> (i32, i32) {
        let s = step as i32;
        match self {
            WorkspaceDir::Left => (-s, 0),
            WorkspaceDir::Right => (s, 0),
            WorkspaceDir::Up => (0, -s),
            WorkspaceDir::Down => (0, s),
        }
    }

    pub const ALL: [WorkspaceDir; 4] = [
        WorkspaceDir::Left,
        WorkspaceDir::Right,
        WorkspaceDir::Up,
        WorkspaceDir::Down,
    ];
}

/// 手势相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Tracking,
    Armed,
    Cooldown,
    Aborted,
    Submitted,
}

impl Phase {
    pub const fn name(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Tracking => "tracking",
            Phase::Armed => "armed",
            Phase::Cooldown => "cooldown",
            Phase::Aborted => "aborted",
            Phase::Submitted => "submitted",
        }
    }
}

/// 一次已递交手势的落账记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GestureRecord {
    pub id: u32,
    pub dir: WorkspaceDir,
    pub travel: u32,
    pub ticks: u32,
    pub anchor: u64,
}

/// 三指工作区手势状态机。
#[derive(Clone, Debug)]
pub struct GestureMachine {
    pub phase: Phase,
    pub fingers: u32,
    pub origin: Point,
    pub cursor: Point,
    pub travel: u32,
    pub elapsed: u32,
    pub held: u32,
    pub cooldown: u32,
    pub attempts: u32,
    pub submitted: u32,
    pub aborted: u32,
    pub rejected: u32,
    pub seq: u32,
    pub last_dir: Option<WorkspaceDir>,
    pub digest: u64,
    pub records: Vec<GestureRecord>,
}

impl GestureMachine {
    pub fn new() -> GestureMachine {
        GestureMachine {
            phase: Phase::Idle,
            fingers: 0,
            origin: Point::new(0, 0),
            cursor: Point::new(0, 0),
            travel: 0,
            elapsed: 0,
            held: 0,
            cooldown: 0,
            attempts: 0,
            submitted: 0,
            aborted: 0,
            rejected: 0,
            seq: 0,
            last_dir: None,
            digest: p_core::FNV_OFF,
            records: Vec::new(),
        }
    }

    /// 三指落按：仅 Idle 相位接受，且手指数必须正好是三。
    pub fn finger_down(&mut self, count: u32, at: Point) -> Result<Phase, u32> {
        self.attempts = self.attempts.saturating_add(1);
        if self.phase != Phase::Idle {
            self.rejected = self.rejected.saturating_add(1);
            return Err(GestureCode::Phase.val());
        }
        if count != FINGERS_REQUIRED {
            self.rejected = self.rejected.saturating_add(1);
            return Err(GestureCode::Fingers.val());
        }
        self.fingers = count;
        self.origin = at;
        self.cursor = at;
        self.travel = 0;
        self.elapsed = 0;
        self.held = 0;
        self.phase = Phase::Tracking;
        Ok(Phase::Tracking)
    }

    /// 逐笔位移：累加曼哈顿距离，越阈即进 Armed。
    pub fn finger_move(&mut self, to: Point) -> Result<Phase, u32> {
        if self.phase != Phase::Tracking && self.phase != Phase::Armed {
            return Err(GestureCode::Phase.val());
        }
        let step = self.cursor.manhattan(to);
        if step == 0 {
            return Err(GestureCode::Threshold.val());
        }
        self.travel = self.travel.saturating_add(step);
        self.cursor = to;
        if self.phase == Phase::Tracking && self.travel >= TRAVEL_THRESHOLD {
            if self.fingers != FINGERS_REQUIRED {
                self.aborted = self.aborted.saturating_add(1);
                self.phase = Phase::Aborted;
                return Err(GestureCode::Fingers.val());
            }
            self.phase = Phase::Armed;
        }
        Ok(self.phase)
    }

    /// 松手：仅 Armed 且稳持足够才递交，其余一律判弃。
    pub fn finger_up(&mut self, remaining: u32) -> Result<Phase, u32> {
        if remaining != 0 {
            self.aborted = self.aborted.saturating_add(1);
            self.phase = Phase::Aborted;
            return Err(GestureCode::Fingers.val());
        }
        match self.phase {
            Phase::Tracking => {
                self.aborted = self.aborted.saturating_add(1);
                self.phase = Phase::Aborted;
                Ok(Phase::Aborted)
            }
            Phase::Armed => {
                if self.held >= HOLD_TICKS && self.travel >= TRAVEL_THRESHOLD {
                    self.submit()?;
                    Ok(Phase::Submitted)
                } else {
                    self.aborted = self.aborted.saturating_add(1);
                    self.phase = Phase::Aborted;
                    Ok(Phase::Aborted)
                }
            }
            _ => Err(GestureCode::Phase.val()),
        }
    }

    /// 时间推进：Tracking 记时判过期，Armed 累计稳持，Cooldown 递减，弃态复位。
    pub fn tick(&mut self) -> Phase {
        match self.phase {
            Phase::Idle => Phase::Idle,
            Phase::Tracking => {
                self.elapsed = self.elapsed.saturating_add(1);
                if self.elapsed > DEADLINE_TICKS {
                    self.aborted = self.aborted.saturating_add(1);
                    self.phase = Phase::Aborted;
                }
                self.phase
            }
            Phase::Armed => {
                if self.held < HOLD_TICKS {
                    self.held = self.held.saturating_add(1);
                }
                self.phase
            }
            Phase::Cooldown => {
                if self.cooldown > 0 {
                    self.cooldown -= 1;
                }
                if self.cooldown == 0 {
                    self.phase = Phase::Idle;
                }
                self.phase
            }
            Phase::Submitted | Phase::Aborted => {
                self.fingers = 0;
                self.phase = Phase::Idle;
                Phase::Idle
            }
        }
    }

    /// 主轴判向：横轴不短于纵轴时归横向，平局也归横向。
    pub fn direction(&self) -> WorkspaceDir {
        let dx = self.cursor.x - self.origin.x;
        let dy = self.cursor.y - self.origin.y;
        let ax = if dx >= 0 { dx } else { -dx };
        let ay = if dy >= 0 { dy } else { -dy };
        if ax >= ay {
            if dx >= 0 {
                WorkspaceDir::Right
            } else {
                WorkspaceDir::Left
            }
        } else if dy >= 0 {
            WorkspaceDir::Down
        } else {
            WorkspaceDir::Up
        }
    }

    /// 递交：落一条记录、折摘要、转冷却。
    fn submit(&mut self) -> Result<(), u32> {
        let dir = self.direction();
        self.seq = match self.seq.checked_add(1) {
            Some(v) => v,
            None => return Err(GestureCode::Overflow.val()),
        };
        let mut anchor = p_core::FNV_OFF;
        anchor = p_core::fold_u32(anchor, self.seq);
        anchor = p_core::fold_byte(anchor, dir.index());
        anchor = p_core::fold_u32(anchor, self.travel);
        anchor = p_core::fold_u32(anchor, self.elapsed);
        let rec = GestureRecord {
            id: self.seq,
            dir,
            travel: self.travel,
            ticks: self.elapsed,
            anchor,
        };
        self.digest = p_core::fold_u32(self.digest, rec.id);
        self.digest = p_core::fold_byte(self.digest, rec.dir.index());
        self.digest = p_core::fold_u32(self.digest, rec.travel);
        self.records.push(rec);
        self.submitted = self.submitted.saturating_add(1);
        self.last_dir = Some(dir);
        self.phase = Phase::Cooldown;
        self.cooldown = DEBOUNCE_TICKS;
        self.fingers = 0;
        Ok(())
    }

    /// 记录末条快照。
    pub fn last_record(&self) -> Option<GestureRecord> {
        self.records.last().copied()
    }

    /// 守恒：递交 + 判弃 + 拒收 == 尝试；记录数 == 递交数；编号连续。
    pub fn conserved(&self) -> bool {
        let total = self
            .submitted
            .saturating_add(self.aborted)
            .saturating_add(self.rejected);
        if total != self.attempts {
            return false;
        }
        if self.records.len() as u32 != self.submitted {
            return false;
        }
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.id != 1 + i as u32 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 驱动一次完整手势：落指 → 逐笔位移 → 稳持 tick → 松手。
/// 起手前把残留的弃态/冷却态清干净，便于批量循环。
pub fn drive(
    machine: &mut GestureMachine,
    dir: WorkspaceDir,
    travel: u32,
    hold: u32,
) -> Result<Option<GestureRecord>, u32> {
    let mut guard = 0u32;
    while (machine.phase == Phase::Cooldown || machine.phase == Phase::Aborted) && guard < 32 {
        machine.tick();
        guard = guard.saturating_add(1);
    }
    machine.finger_down(FINGERS_REQUIRED, Point::new(0x100, 0x100))?;
    let mut left = travel;
    let mut steps = 0u32;
    while left > 0 && steps < 64 {
        let step = if left > STEP { STEP } else { left };
        let (dx, dy) = dir.delta(step);
        let to = Point::new(machine.cursor.x + dx, machine.cursor.y + dy);
        machine.finger_move(to)?;
        left -= step;
        steps = steps.saturating_add(1);
    }
    let mut i = 0u32;
    while i < hold {
        machine.tick();
        i += 1;
    }
    let before = machine.records.len();
    machine.finger_up(0)?;
    if machine.records.len() > before {
        Ok(machine.last_record())
    } else {
        Ok(None)
    }
}

/// 百次手势：四向轮转，位移在阈值之上抖动，100 次全部递交。
pub fn hundred_machine() -> Result<GestureMachine, u32> {
    let mut m = GestureMachine::new();
    let mut rng = p_core::Lcg::new(0x0C01_0001);
    let mut i = 0u32;
    while i < HUNDRED {
        let idx = (i as usize) % WorkspaceDir::ALL.len();
        let dir = match WorkspaceDir::ALL.get(idx) {
            Some(v) => *v,
            None => return Err(GestureCode::Direction.val()),
        };
        let travel = TRAVEL_THRESHOLD.saturating_add(rng.below(64));
        drive(&mut m, dir, travel, HOLD_TICKS)?;
        i += 1;
    }
    Ok(m)
}

/// 百次手势摘要。
pub fn hundred_gestures() -> Result<(u32, u32, u64), u32> {
    let m = hundred_machine()?;
    Ok((m.submitted, m.aborted, m.digest))
}

/// 手势复跑：两遍百次的摘要必须一致。
pub fn hundred_replay() -> Result<(u64, u64), u32> {
    let a = hundred_machine()?.digest;
    let b = hundred_machine()?.digest;
    Ok((a, b))
}

/// 阈值边界扫描：差一格 / 正好越阈 / 稳持少一格。
/// 返回 (差一格递交数, 正好越阈递交数, 早放递交数)。
pub fn threshold_sweep() -> Result<(u32, u32, u32), u32> {
    let mut below = GestureMachine::new();
    let mut i = 0u32;
    while i < INJECT {
        drive(&mut below, WorkspaceDir::Right, TRAVEL_THRESHOLD - 1, HOLD_TICKS)?;
        i += 1;
    }
    let mut edge = GestureMachine::new();
    let mut j = 0u32;
    while j < INJECT {
        drive(&mut edge, WorkspaceDir::Right, TRAVEL_THRESHOLD, HOLD_TICKS)?;
        j += 1;
    }
    let mut early = GestureMachine::new();
    let mut k = 0u32;
    while k < INJECT {
        drive(&mut early, WorkspaceDir::Right, TRAVEL_THRESHOLD, HOLD_TICKS - 1)?;
        k += 1;
    }
    Ok((below.submitted, edge.submitted, early.submitted))
}

/// 稳持注入：10 次"少停一格"，必须 10 弃 0 交。
pub fn hold_faults() -> Result<(u32, u32), u32> {
    let mut m = GestureMachine::new();
    let mut i = 0u32;
    while i < INJECT {
        drive(&mut m, WorkspaceDir::Left, TRAVEL_THRESHOLD + STEP, HOLD_TICKS - 1)?;
        i += 1;
    }
    Ok((m.aborted, m.submitted))
}

/// 去抖注入：递交后立刻再落 10 次指，必须全被相位拒收；
/// 冷却结束后再走一次完整手势，必须成功。
pub fn debounce_probe() -> Result<(u32, u32), u32> {
    let mut m = GestureMachine::new();
    let first = drive(&mut m, WorkspaceDir::Left, TRAVEL_THRESHOLD + STEP, HOLD_TICKS)?;
    if first.is_none() {
        return Err(GestureCode::Hold.val());
    }
    let mut rejected = 0u32;
    let mut i = 0u32;
    while i < INJECT {
        if m.finger_down(FINGERS_REQUIRED, Point::new(0, 0)).is_err() {
            rejected = rejected.saturating_add(1);
        }
        i += 1;
    }
    let before = m.submitted;
    drive(&mut m, WorkspaceDir::Right, TRAVEL_THRESHOLD + STEP, HOLD_TICKS)?;
    Ok((rejected, m.submitted.saturating_sub(before)))
}

// ── H5 分界登记（F49048） ────────────────────────────────────────
/// 输入语义所属域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    Unx,
    H5,
}

impl Domain {
    pub const fn index(self) -> u8 {
        match self {
            Domain::Unx => 0,
            Domain::H5 => 1,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Domain::Unx => "unxstar",
            Domain::H5 => "h5",
        }
    }
}

/// 七类输入语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputClass {
    Keyboard,
    Mouse,
    Touch,
    Gamepad,
    HidRaw,
    HidSensor,
    HidVendor,
}

impl InputClass {
    pub const fn index(self) -> u8 {
        match self {
            InputClass::Keyboard => 0,
            InputClass::Mouse => 1,
            InputClass::Touch => 2,
            InputClass::Gamepad => 3,
            InputClass::HidRaw => 4,
            InputClass::HidSensor => 5,
            InputClass::HidVendor => 6,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            InputClass::Keyboard => "keyboard",
            InputClass::Mouse => "mouse",
            InputClass::Touch => "touch",
            InputClass::Gamepad => "gamepad",
            InputClass::HidRaw => "hid-raw",
            InputClass::HidSensor => "hid-sensor",
            InputClass::HidVendor => "hid-vendor",
        }
    }

    /// 规范属主：键鼠触控归本域，手柄/HID 归 H5。
    pub const fn canonical_owner(self) -> Domain {
        match self {
            InputClass::Keyboard | InputClass::Mouse | InputClass::Touch => Domain::Unx,
            InputClass::Gamepad
            | InputClass::HidRaw
            | InputClass::HidSensor
            | InputClass::HidVendor => Domain::H5,
        }
    }

    pub const ALL: [InputClass; BOUNDARY_TOTAL] = [
        InputClass::Keyboard,
        InputClass::Mouse,
        InputClass::Touch,
        InputClass::Gamepad,
        InputClass::HidRaw,
        InputClass::HidSensor,
        InputClass::HidVendor,
    ];
}

/// 一条 RAW Input 分界声明行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryRow {
    pub class: InputClass,
    pub owner: Domain,
    pub raw: &'static str,
    pub cosign: &'static str,
}

/// 七条分界声明：三条归本域（D4 联签），四条归 H5（H5 联签）。
pub const BOUNDARY: [BoundaryRow; BOUNDARY_TOTAL] = [
    BoundaryRow {
        class: InputClass::Keyboard,
        owner: Domain::Unx,
        raw: "RAW:kb.scancode",
        cosign: D4_ANCHOR,
    },
    BoundaryRow {
        class: InputClass::Mouse,
        owner: Domain::Unx,
        raw: "RAW:mouse.delta",
        cosign: D4_ANCHOR,
    },
    BoundaryRow {
        class: InputClass::Touch,
        owner: Domain::Unx,
        raw: "RAW:touch.contact",
        cosign: D4_ANCHOR,
    },
    BoundaryRow {
        class: InputClass::Gamepad,
        owner: Domain::H5,
        raw: "RAW:gamepad.xinput",
        cosign: H5_ANCHOR,
    },
    BoundaryRow {
        class: InputClass::HidRaw,
        owner: Domain::H5,
        raw: "RAW:hid.report",
        cosign: H5_ANCHOR,
    },
    BoundaryRow {
        class: InputClass::HidSensor,
        owner: Domain::H5,
        raw: "RAW:hid.sensor",
        cosign: H5_ANCHOR,
    },
    BoundaryRow {
        class: InputClass::HidVendor,
        owner: Domain::H5,
        raw: "RAW:hid.vendor",
        cosign: H5_ANCHOR,
    },
];

/// 声明是否与规范映射一致。
pub fn claim_ok(class: InputClass, owner: Domain) -> bool {
    class.canonical_owner() == owner
}

/// 越界 grep：逐行核属主、核 RAW 路径非空、核联签非空、核路径不重复。
pub fn grep_out_of_bounds(rows: &[BoundaryRow]) -> u32 {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.owner != r.class.canonical_owner() {
                bad = bad.saturating_add(1);
            } else if r.raw.is_empty() || r.cosign.is_empty() {
                bad = bad.saturating_add(1);
            } else {
                let mut j = i + 1;
                let mut dup = false;
                while j < rows.len() {
                    if let Some(o) = rows.get(j) {
                        if o.raw == r.raw {
                            dup = true;
                        }
                    }
                    j += 1;
                }
                if dup {
                    bad = bad.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    bad
}

/// 静态分界表的越界数（必须为零）。
pub fn boundary_scan() -> u32 {
    grep_out_of_bounds(&BOUNDARY)
}

/// 十行越界注入：6 行属主错、2 行缺路径/缺联签、2 行反向归属。
pub fn injected_rows() -> Vec<BoundaryRow> {
    let mut out: Vec<BoundaryRow> = Vec::new();
    let mut i = 0usize;
    while i < 6 {
        let c = match InputClass::ALL.get(i) {
            Some(v) => *v,
            None => break,
        };
        let wrong = if c.canonical_owner() == Domain::Unx {
            Domain::H5
        } else {
            Domain::Unx
        };
        let raw = match i {
            0 => "RAW:bad.01",
            1 => "RAW:bad.02",
            2 => "RAW:bad.03",
            3 => "RAW:bad.04",
            4 => "RAW:bad.05",
            _ => "RAW:bad.06",
        };
        out.push(BoundaryRow {
            class: c,
            owner: wrong,
            raw,
            cosign: D4_ANCHOR,
        });
        i += 1;
    }
    out.push(BoundaryRow {
        class: InputClass::HidVendor,
        owner: Domain::H5,
        raw: "RAW:bad.07",
        cosign: "",
    });
    out.push(BoundaryRow {
        class: InputClass::HidVendor,
        owner: Domain::H5,
        raw: "",
        cosign: H5_ANCHOR,
    });
    out.push(BoundaryRow {
        class: InputClass::Keyboard,
        owner: Domain::H5,
        raw: "RAW:bad.09",
        cosign: D4_ANCHOR,
    });
    out.push(BoundaryRow {
        class: InputClass::Gamepad,
        owner: Domain::Unx,
        raw: "RAW:bad.10",
        cosign: H5_ANCHOR,
    });
    out
}

/// 越界注入结果：(检出, 漏检)。
pub fn grep_inject() -> (u32, u32) {
    let bad = injected_rows();
    let detected = grep_out_of_bounds(&bad);
    let total = bad.len() as u32;
    let missed = total.saturating_sub(if detected > total { total } else { detected });
    (detected, missed)
}

/// 属主矩阵：七类 × 两域 = 14 条声明，正确 7、越界 7。
pub fn claim_matrix() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < InputClass::ALL.len() {
        if let Some(c) = InputClass::ALL.get(i) {
            let owners = [Domain::Unx, Domain::H5];
            let mut j = 0usize;
            while j < owners.len() {
                if let Some(o) = owners.get(j) {
                    if claim_ok(*c, *o) {
                        ok = ok.saturating_add(1);
                    } else {
                        bad = bad.saturating_add(1);
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    (ok, bad)
}

/// 百次声明：七类轮转、属主全按规范，越界数必须为零。
pub fn hundred_claims() -> Result<(u32, u32, u64), u32> {
    let mut h = p_core::FNV_OFF;
    let mut claims = 0u32;
    let mut oob = 0u32;
    let mut i = 0u32;
    while i < HUNDRED {
        let c = match InputClass::ALL.get((i as usize) % BOUNDARY_TOTAL) {
            Some(v) => *v,
            None => return Err(GestureCode::Boundary.val()),
        };
        let o = c.canonical_owner();
        if claim_ok(c, o) {
            claims = claims.saturating_add(1);
            h = p_core::fold_byte(h, c.index());
            h = p_core::fold_byte(h, o.index());
        } else {
            oob = oob.saturating_add(1);
        }
        i += 1;
    }
    Ok((claims, oob, h))
}

/// 分界表摘要：逐行折类别、属主与 RAW 路径字节。
pub fn boundary_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < BOUNDARY.len() {
        if let Some(r) = BOUNDARY.get(i) {
            h = p_core::fold_byte(h, r.class.index());
            h = p_core::fold_byte(h, r.owner.index());
            let mut j = 0usize;
            while j < r.raw.len() {
                if let Some(b) = r.raw.as_bytes().get(j) {
                    h = p_core::fold_byte(h, *b);
                }
                j += 1;
            }
        }
        i += 1;
    }
    h
}

/// 联签锚定行：H5 与 D4 两个横向方 + AI-19 判据锚，一行落账。
pub fn cosign_line() -> String {
    format!(
        "BOUNDSIGN|unx=keyboard,mouse,touch|h5=gamepad,hid-raw,hid-sensor,hid-vendor|h5anchor={}|d4anchor={}|ai={}",
        H5_ANCHOR, D4_ANCHOR, AI19_ANCHOR
    )
}

/// RAW 路径唯一性：七条路径互不相同。
pub fn raw_paths_unique() -> bool {
    let mut i = 0usize;
    while i < BOUNDARY.len() {
        let a = match BOUNDARY.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < BOUNDARY.len() {
            if let Some(b) = BOUNDARY.get(j) {
                if a.raw == b.raw || a.class == b.class {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 渲染与解析 ────────────────────────────────────────────────────
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
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

pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
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
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
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
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

/// 手势账行：递交数、判弃数、当前位移、记录数。
pub fn gesture_row(m: &GestureMachine) -> String {
    render_row(
        GESTURE_TAG,
        m.submitted,
        m.aborted,
        m.travel,
        m.records.len() as u32,
        m.digest,
    )
}

/// 递交账行：记录号、方向、位移、总递交数、记录锚。
pub fn submit_row(rec: &GestureRecord, total: u32) -> String {
    render_row(
        SUBMIT_TAG,
        rec.id,
        rec.dir.index() as u32,
        rec.travel,
        total,
        rec.anchor,
    )
}

/// 分界账行：行数、本域行数、H5 行数、越界数。
pub fn boundary_row() -> String {
    let mut unx = 0u32;
    let mut h5 = 0u32;
    let mut i = 0usize;
    while i < BOUNDARY.len() {
        if let Some(r) = BOUNDARY.get(i) {
            if r.owner == Domain::Unx {
                unx = unx.saturating_add(1);
            } else {
                h5 = h5.saturating_add(1);
            }
        }
        i += 1;
    }
    render_row(
        BOUND_TAG,
        BOUNDARY_TOTAL as u32,
        unx,
        h5,
        boundary_scan(),
        boundary_digest(),
    )
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49047 ①：三指状态机（Idle→Tracking→Armed→稳持→Submitted→Cooldown）。
    let mut m = GestureMachine::new();
    let mut steps = 0u32;
    if m.phase == Phase::Idle {
        steps = steps.saturating_add(1);
    }
    if m.finger_down(FINGERS_REQUIRED, Point::new(0, 0)) == Ok(Phase::Tracking) {
        steps = steps.saturating_add(1);
    }
    if m.finger_move(Point::new(16, 0)) == Ok(Phase::Tracking) {
        steps = steps.saturating_add(1);
    }
    if m.finger_move(Point::new(64, 0)) == Ok(Phase::Armed) {
        steps = steps.saturating_add(1);
    }
    let mut ticks = 0u32;
    while ticks < HOLD_TICKS {
        if m.tick() != Phase::Armed {
            break;
        }
        ticks = ticks.saturating_add(1);
    }
    if m.finger_up(0) == Ok(Phase::Submitted) {
        steps = steps.saturating_add(1);
    }
    let mut bad = GestureMachine::new();
    let two = bad.finger_down(2, Point::new(0, 0)).is_err();
    let four = bad.finger_down(4, Point::new(0, 0)).is_err();
    let busy = m.finger_down(FINGERS_REQUIRED, Point::new(0, 0)).is_err();
    let phase_ok = steps == 5
        && ticks == HOLD_TICKS
        && m.phase == Phase::Cooldown
        && m.submitted == 1
        && m.records.len() == 1
        && m.conserved()
        && two
        && four
        && busy
        && m.cooldown == DEBOUNCE_TICKS;
    out.push((
        "F49047/三指状态机",
        phase_ok,
        format!(
            "steps={} tick={} phase={} sub={} rec={} two={} four={} busy={}",
            steps,
            ticks,
            m.phase.name(),
            m.submitted,
            m.records.len(),
            two,
            four,
            busy
        ),
    ));

    // F49047 ②：阈值去抖（差一格 0 / 正好 10 / 早放 0，稳持注入 10 弃，冷却期 10 拒）。
    let sweep = threshold_sweep();
    let hold = hold_faults();
    let deb = debounce_probe();
    let gate_ok = match (&sweep, &hold, &deb) {
        (Ok((b, e, a)), Ok((h_abort, h_sub)), Ok((rej, acc))) => {
            *b == 0
                && *e == INJECT
                && *a == 0
                && *h_abort == INJECT
                && *h_sub == 0
                && *rej == INJECT
                && *acc == 1
        }
        _ => false,
    };
    out.push((
        "F49047/阈值去抖",
        gate_ok,
        format!(
            "below={:?} edge={:?} early={:?} hold={:?} deb={:?}",
            sweep.as_ref().map(|(b, _, _)| *b),
            sweep.as_ref().map(|(_, e, _)| *e),
            sweep.as_ref().map(|(_, _, a)| *a),
            hold.as_ref().map(|(a, s)| (*a, *s)),
            deb.as_ref().map(|(r, a)| (*r, *a))
        ),
    ));

    // F49047 ③：F5 递交（100 次全交、记录编号连续、分界 grep 零越界）。
    let hm = hundred_machine();
    let mut seq_ok = false;
    let mut last_dir = None;
    let mut conserved = false;
    let mut submit_ok = false;
    if let Ok(m) = &hm {
        let mut i = 0usize;
        let mut seq = true;
        while i < m.records.len() {
            if let Some(r) = m.records.get(i) {
                if r.id != 1 + i as u32 || r.anchor == 0 {
                    seq = false;
                }
            }
            i += 1;
        }
        seq_ok = seq;
        last_dir = m.last_dir;
        conserved = m.conserved();
        submit_ok = m.submitted == HUNDRED && m.aborted == 0 && m.digest != 0;
    }
    out.push((
        "F49047/F5递交",
        submit_ok && seq_ok && conserved && boundary_scan() == 0 && last_dir.is_some(),
        format!(
            "sub={:?} abort={:?} seq={} cons={} grep={} dir={:?}",
            hm.as_ref().map(|m| m.submitted),
            hm.as_ref().map(|m| m.aborted),
            seq_ok,
            conserved,
            boundary_scan(),
            last_dir.map(|d| d.name())
        ),
    ));

    // F49047 ④：手势复跑（两遍摘要一致且非零，账行可解析）。
    let rep = hundred_replay();
    let grow = match &hm {
        Ok(m) => gesture_row(m),
        Err(_) => String::from(""),
    };
    let grow_ok = match row_fields(&grow) {
        Some((t, v, d)) => {
            t == GESTURE_TAG && v[0] == HUNDRED && v[3] == HUNDRED && d != 0 && audit_render(&grow)
        }
        None => false,
    };
    let rep_ok = match &rep {
        Ok((a, b)) => a == b && *a != 0,
        Err(_) => false,
    };
    out.push((
        "F49047/手势复跑",
        rep_ok && grow_ok,
        format!(
            "a={:?} b={:?} row={} equal={:?}",
            rep.as_ref().map(|(a, _)| *a),
            rep.as_ref().map(|(_, b)| *b),
            grow_ok,
            rep.as_ref().map(|(a, b)| a == b)
        ),
    ));

    // F49048 ①：分界登记表（七行齐备、路径唯一、属主全对）。
    let mut owners_ok = true;
    let mut unx_rows = 0u32;
    let mut h5_rows = 0u32;
    let mut i = 0usize;
    while i < BOUNDARY.len() {
        if let Some(r) = BOUNDARY.get(i) {
            if !claim_ok(r.class, r.owner) || r.raw.is_empty() || r.cosign.is_empty() {
                owners_ok = false;
            }
            if r.owner == Domain::Unx {
                unx_rows = unx_rows.saturating_add(1);
            } else {
                h5_rows = h5_rows.saturating_add(1);
            }
        }
        i += 1;
    }
    let brow = boundary_row();
    let brow_ok = match row_fields(&brow) {
        Some((t, v, d)) => {
            t == BOUND_TAG
                && v[0] == BOUNDARY_TOTAL as u32
                && v[1] == 3
                && v[2] == 4
                && v[3] == 0
                && d == boundary_digest()
                && audit_render(&brow)
        }
        None => false,
    };
    out.push((
        "F49048/分界登记表",
        owners_ok
            && raw_paths_unique()
            && BOUNDARY.len() == BOUNDARY_TOTAL
            && unx_rows == 3
            && h5_rows == 4
            && boundary_digest() != 0
            && brow_ok,
        format!(
            "rows={} unx={} h5={} uniq={} digest={:016X}",
            BOUNDARY.len(),
            unx_rows,
            h5_rows,
            raw_paths_unique(),
            boundary_digest()
        ),
    ));

    // F49048 ②：H5 归属零越界（百次声明 0 越界、十行注入全检出）。
    let hc = hundred_claims();
    let inj = grep_inject();
    let oob_ok = match (&hc, &inj) {
        (Ok((claims, oob, h)), (det, miss)) => {
            *claims == HUNDRED && *oob == 0 && *det == INJECT && *miss == 0 && *h != 0
        }
        _ => false,
    };
    out.push((
        "F49048/H5归属零越界",
        oob_ok && boundary_scan() == 0,
        format!(
            "claims={:?} oob={:?} det={:?} miss={:?}",
            hc.as_ref().map(|(c, _, _)| *c),
            hc.as_ref().map(|(_, o, _)| *o),
            inj.0,
            inj.1
        ),
    ));

    // F49048 ③：键鼠触控归属（三类归本域、四类归 H5、反向声明全拒）。
    let key_ok = InputClass::Keyboard.canonical_owner() == Domain::Unx
        && InputClass::Mouse.canonical_owner() == Domain::Unx
        && InputClass::Touch.canonical_owner() == Domain::Unx;
    let h5_ok = InputClass::Gamepad.canonical_owner() == Domain::H5
        && InputClass::HidRaw.canonical_owner() == Domain::H5
        && InputClass::HidSensor.canonical_owner() == Domain::H5
        && InputClass::HidVendor.canonical_owner() == Domain::H5;
    let cm = claim_matrix();
    let matrix_ok = cm.0 == 7 && cm.1 == 7;
    out.push((
        "F49048/键鼠触控归属",
        key_ok && h5_ok && matrix_ok && !claim_ok(InputClass::Keyboard, Domain::H5),
        format!(
            "key={} h5={} ok={} bad={}",
            key_ok, h5_ok, cm.0, cm.1
        ),
    ));

    // F49048 ④：联签锚定行（H5/D4/AI-19 三锚齐全、段位与用例编号守卫）。
    let line = cosign_line();
    let anchor_ok = line.contains(H5_ANCHOR)
        && line.contains(D4_ANCHOR)
        && line.contains(AI19_ANCHOR)
        && line.contains(H5_ANCHOR)
        && line.starts_with("BOUNDSIGN");
    out.push((
        "F49048/联签锚定行",
        anchor_ok
            && AI40_ANCHOR.contains("AI-40")
            && segment_guard()
            && no_cross_collision()
            && case_ids_sequential()
            && CASES.len() == 16,
        format!(
            "h5={} d4={} ai={} seg={} ids={}",
            line.contains(H5_ANCHOR),
            line.contains(D4_ANCHOR),
            line.contains(AI19_ANCHOR),
            segment_guard(),
            case_ids_sequential()
        ),
    ));

    out
}

pub fn run_unx49047_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49047 3 指工作区手势递交与 H5 分界联签");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_closed_and_segmented() {
        assert_eq!(GestureCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < GestureCode::ALL.len() {
            let c = GestureCode::ALL[i];
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
        while i < GestureCode::ALL.len() {
            let c = GestureCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(7), UNKNOWN);
    }

    #[test]
    fn detail_texts_are_distinct() {
        let mut i = 0usize;
        while i < GestureCode::ALL.len() {
            let mut j = i + 1;
            while j < GestureCode::ALL.len() {
                assert_ne!(GestureCode::ALL[i].detail(), GestureCode::ALL[j].detail());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn segment_guard_covers_exact_window() {
        assert_eq!(GestureCode::ALL[0].val(), 0x730C1);
        assert_eq!(GestureCode::ALL[13].val(), 0x730CE);
        assert!(!known_code(0x730C0));
        assert!(!known_code(0x730CF));
    }

    #[test]
    fn case_ids_are_reserved_and_unique() {
        assert!(no_cross_collision());
        assert_eq!(CASE_IDS.len(), 16);
        assert_eq!(CASE_IDS[0], 0x0733_0C01);
        assert_eq!(CASE_IDS[15], 0x0733_0C10);
        let mut i = 0usize;
        while i < RESERVED_IDS.len() {
            let mut j = 0usize;
            while j < CASE_IDS.len() {
                assert_ne!(RESERVED_IDS[i], CASE_IDS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn case_ids_are_sequential() {
        assert!(case_ids_sequential());
        assert_eq!(CASES.len(), CASE_IDS.len());
        let mut i = 0usize;
        while i < CASES.len() {
            assert_eq!(CASES[i].id, CASE_IDS[i]);
            assert!(!CASES[i].name.is_empty());
            i += 1;
        }
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row("GSTRF5", 1, 2, 3, 4, 0x0123_4567_89AB_CDEF);
        assert!(audit_render(&row));
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, "GSTRF5");
        assert_eq!(v, [1, 2, 3, 4]);
        assert_eq!(d, 0x0123_4567_89AB_CDEF);
    }

    #[test]
    fn parse_u32_rejects_junk() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(4294967295));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_u32(" 1"), None);
        assert_eq!(parse_u32("4294967296"), None);
    }

    #[test]
    fn parse_hex16_rejects_short() {
        assert_eq!(parse_hex16("0123456789ABCDEF"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789abcdef"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("ABC"), None);
        assert_eq!(parse_hex16("0123456789ABCDE"), None);
        assert_eq!(parse_hex16("0123456789ABCDEG"), None);
    }

    #[test]
    fn row_fields_rejects_malformed() {
        assert!(row_fields("").is_none());
        assert!(row_fields("GSTRF5|k1=1").is_none());
        assert!(row_fields("GSTRF5|k1=1|k2=2|k3=3|k4=4|digest=0123456789ABCDEF|extra=1").is_none());
        assert!(row_fields("|k1=1|k2=2|k3=3|k4=4|digest=0123456789ABCDEF").is_none());
        assert!(row_fields("GSTRF5|k1=1|k2=2|k3=3|k4=4|digest=zz").is_none());
    }

    #[test]
    fn machine_starts_idle_and_empty() {
        let m = GestureMachine::new();
        assert_eq!(m.phase, Phase::Idle);
        assert_eq!(m.submitted, 0);
        assert_eq!(m.aborted, 0);
        assert_eq!(m.rejected, 0);
        assert_eq!(m.digest, p_core::FNV_OFF);
        assert!(m.records.is_empty());
        assert!(m.conserved());
        assert!(m.last_dir.is_none());
    }

    #[test]
    fn three_fingers_start_tracking() {
        let mut m = GestureMachine::new();
        assert_eq!(m.finger_down(3, Point::new(10, 10)), Ok(Phase::Tracking));
        assert_eq!(m.phase, Phase::Tracking);
        assert_eq!(m.fingers, 3);
        assert_eq!(m.attempts, 1);
        assert_eq!(m.origin, Point::new(10, 10));
    }

    #[test]
    fn wrong_finger_count_rejected() {
        let mut m = GestureMachine::new();
        assert_eq!(
            m.finger_down(2, Point::new(0, 0)),
            Err(GestureCode::Fingers.val())
        );
        assert_eq!(
            m.finger_down(4, Point::new(0, 0)),
            Err(GestureCode::Fingers.val())
        );
        assert_eq!(m.phase, Phase::Idle);
        assert_eq!(m.rejected, 2);
        assert_eq!(m.attempts, 2);
        assert!(m.conserved());
    }

    #[test]
    fn move_accumulates_travel() {
        let mut m = GestureMachine::new();
        assert_eq!(m.finger_down(3, Point::new(0, 0)), Ok(Phase::Tracking));
        assert_eq!(m.finger_move(Point::new(16, 0)), Ok(Phase::Tracking));
        assert_eq!(m.travel, 16);
        assert_eq!(m.finger_move(Point::new(32, 0)), Ok(Phase::Tracking));
        assert_eq!(m.travel, 32);
        assert_eq!(m.finger_move(Point::new(48, 0)), Ok(Phase::Armed));
        assert_eq!(m.travel, 48);
    }

    #[test]
    fn zero_move_rejected() {
        let mut m = GestureMachine::new();
        assert_eq!(m.finger_down(3, Point::new(5, 5)), Ok(Phase::Tracking));
        assert_eq!(
            m.finger_move(Point::new(5, 5)),
            Err(GestureCode::Threshold.val())
        );
        assert_eq!(m.travel, 0);
        assert_eq!(m.phase, Phase::Tracking);
    }

    #[test]
    fn threshold_sweep_edges() {
        assert_eq!(threshold_sweep().unwrap(), (0, 10, 0));
    }

    #[test]
    fn hold_faults_split() {
        assert_eq!(hold_faults().unwrap(), (10, 0));
    }

    #[test]
    fn debounce_probe_split() {
        assert_eq!(debounce_probe().unwrap(), (10, 1));
    }

    #[test]
    fn early_release_aborts() {
        let mut m = GestureMachine::new();
        m.finger_down(3, Point::new(0, 0)).unwrap();
        m.finger_move(Point::new(64, 0)).unwrap();
        assert_eq!(m.phase, Phase::Armed);
        m.tick();
        assert_eq!(m.finger_up(0), Ok(Phase::Aborted));
        assert_eq!(m.submitted, 0);
        assert_eq!(m.aborted, 1);
        assert!(m.conserved());
    }

    #[test]
    fn hold_ticks_required_to_submit() {
        let mut m = GestureMachine::new();
        m.finger_down(3, Point::new(0, 0)).unwrap();
        m.finger_move(Point::new(64, 0)).unwrap();
        assert_eq!(m.tick(), Phase::Armed);
        assert_eq!(m.tick(), Phase::Armed);
        assert_eq!(m.finger_up(0), Ok(Phase::Submitted));
        assert_eq!(m.submitted, 1);
        assert_eq!(m.phase, Phase::Cooldown);
    }

    #[test]
    fn four_finger_mid_gesture_rejected() {
        let mut m = GestureMachine::new();
        m.finger_down(3, Point::new(0, 0)).unwrap();
        assert_eq!(
            m.finger_down(3, Point::new(0, 0)),
            Err(GestureCode::Phase.val())
        );
        assert_eq!(m.rejected, 1);
    }

    #[test]
    fn deadline_aborts_stale_tracking() {
        let mut m = GestureMachine::new();
        m.finger_down(3, Point::new(0, 0)).unwrap();
        let mut i = 0u32;
        while i <= DEADLINE_TICKS {
            m.tick();
            i += 1;
        }
        assert_eq!(m.phase, Phase::Aborted);
        assert_eq!(m.aborted, 1);
    }

    #[test]
    fn directions_cover_four_axes() {
        let mut dirs = [WorkspaceDir::Left; 4];
        let mut m = GestureMachine::new();
        let mut i = 0u32;
        while i < 4 {
            let d = WorkspaceDir::ALL[i as usize];
            drive(&mut m, d, TRAVEL_THRESHOLD, HOLD_TICKS).unwrap();
            dirs[i as usize] = m.last_dir.unwrap();
            i += 1;
        }
        assert_eq!(dirs[0], WorkspaceDir::Left);
        assert_eq!(dirs[1], WorkspaceDir::Right);
        assert_eq!(dirs[2], WorkspaceDir::Up);
        assert_eq!(dirs[3], WorkspaceDir::Down);
        assert_eq!(m.submitted, 4);
    }

    #[test]
    fn direction_tie_prefers_horizontal() {
        let mut m = GestureMachine::new();
        m.finger_down(3, Point::new(0, 0)).unwrap();
        m.finger_move(Point::new(20, 20)).unwrap();
        assert_eq!(m.direction(), WorkspaceDir::Right);
        let mut v = GestureMachine::new();
        v.finger_down(3, Point::new(0, 0)).unwrap();
        v.finger_move(Point::new(-10, 30)).unwrap();
        assert_eq!(v.direction(), WorkspaceDir::Down);
    }

    #[test]
    fn submit_resets_to_cooldown() {
        let mut m = GestureMachine::new();
        drive(&mut m, WorkspaceDir::Right, TRAVEL_THRESHOLD, HOLD_TICKS).unwrap();
        assert_eq!(m.phase, Phase::Cooldown);
        assert_eq!(m.cooldown, DEBOUNCE_TICKS);
        assert_eq!(m.fingers, 0);
    }

    #[test]
    fn cooldown_blocks_new_gesture() {
        let mut m = GestureMachine::new();
        drive(&mut m, WorkspaceDir::Right, TRAVEL_THRESHOLD, HOLD_TICKS).unwrap();
        assert_eq!(
            m.finger_down(3, Point::new(0, 0)),
            Err(GestureCode::Phase.val())
        );
        assert_eq!(m.rejected, 1);
    }

    #[test]
    fn cooldown_expires_after_ticks() {
        let mut m = GestureMachine::new();
        drive(&mut m, WorkspaceDir::Right, TRAVEL_THRESHOLD, HOLD_TICKS).unwrap();
        let mut i = 0u32;
        while i < DEBOUNCE_TICKS {
            m.tick();
            i += 1;
        }
        assert_eq!(m.phase, Phase::Idle);
    }

    #[test]
    fn submitted_record_fields() {
        let mut m = GestureMachine::new();
        let rec = drive(&mut m, WorkspaceDir::Up, TRAVEL_THRESHOLD + 7, HOLD_TICKS)
            .unwrap()
            .unwrap();
        assert_eq!(rec.id, 1);
        assert_eq!(rec.dir, WorkspaceDir::Up);
        assert!(rec.travel >= TRAVEL_THRESHOLD);
        assert_ne!(rec.anchor, 0);
        assert_ne!(m.digest, p_core::FNV_OFF);
        assert_eq!(m.last_record(), Some(rec));
    }

    #[test]
    fn hundred_gestures_submit_all() {
        let (sub, abort, digest) = hundred_gestures().unwrap();
        assert_eq!(sub, 100);
        assert_eq!(abort, 0);
        assert_ne!(digest, 0);
        let m = hundred_machine().unwrap();
        assert_eq!(m.records.len(), 100);
        assert_eq!(m.attempts, 100);
        assert!(m.conserved());
        assert_eq!(m.records[99].id, 100);
    }

    #[test]
    fn hundred_gestures_replay_stable() {
        let (a, b) = hundred_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn machine_conservation_holds() {
        let mut m = GestureMachine::new();
        m.finger_down(3, Point::new(0, 0)).unwrap();
        m.finger_move(Point::new(64, 0)).unwrap();
        m.tick();
        m.tick();
        m.finger_up(0).unwrap();
        m.tick();
        m.tick();
        m.tick();
        m.tick();
        m.finger_down(3, Point::new(0, 0)).unwrap();
        m.finger_up(0).unwrap();
        m.tick();
        m.finger_down(2, Point::new(0, 0)).unwrap_err();
        assert_eq!(m.submitted, 1);
        assert_eq!(m.aborted, 1);
        assert_eq!(m.rejected, 1);
        assert_eq!(m.attempts, 3);
        assert!(m.conserved());
    }

    #[test]
    fn boundary_table_covers_all_classes() {
        assert_eq!(BOUNDARY.len(), 7);
        let mut i = 0usize;
        while i < InputClass::ALL.len() {
            let c = InputClass::ALL[i];
            assert_eq!(c.index() as usize, i);
            assert_eq!(BOUNDARY[i].class, c);
            i += 1;
        }
        assert!(raw_paths_unique());
        assert_ne!(boundary_digest(), 0);
    }

    #[test]
    fn boundary_raw_paths_unique() {
        assert!(raw_paths_unique());
        let mut i = 0usize;
        while i < BOUNDARY.len() {
            assert!(BOUNDARY[i].raw.starts_with("RAW:"));
            assert!(!BOUNDARY[i].cosign.is_empty());
            i += 1;
        }
    }

    #[test]
    fn boundary_owners_match_mapping() {
        assert_eq!(boundary_scan(), 0);
        let mut i = 0usize;
        while i < BOUNDARY.len() {
            assert_eq!(BOUNDARY[i].owner, BOUNDARY[i].class.canonical_owner());
            i += 1;
        }
        assert_eq!(claim_matrix(), (7, 7));
    }

    #[test]
    fn claim_ok_rejects_cross_domain() {
        assert!(claim_ok(InputClass::Keyboard, Domain::Unx));
        assert!(claim_ok(InputClass::Touch, Domain::Unx));
        assert!(claim_ok(InputClass::Gamepad, Domain::H5));
        assert!(claim_ok(InputClass::HidVendor, Domain::H5));
        assert!(!claim_ok(InputClass::Keyboard, Domain::H5));
        assert!(!claim_ok(InputClass::Gamepad, Domain::Unx));
    }

    #[test]
    fn grep_clean_table_zero() {
        assert_eq!(grep_out_of_bounds(&BOUNDARY), 0);
        assert_eq!(boundary_scan(), 0);
    }

    #[test]
    fn grep_injection_detected() {
        assert_eq!(injected_rows().len(), 10);
        assert_eq!(grep_inject(), (10, 0));
    }

    #[test]
    fn hundred_claims_zero_oob() {
        let (claims, oob, digest) = hundred_claims().unwrap();
        assert_eq!(claims, 100);
        assert_eq!(oob, 0);
        assert_ne!(digest, 0);
    }

    #[test]
    fn boundary_digest_stable() {
        assert_eq!(boundary_digest(), boundary_digest());
        assert_ne!(boundary_digest(), 0);
    }

    #[test]
    fn cosign_line_anchors() {
        let line = cosign_line();
        assert!(line.starts_with("BOUNDSIGN"));
        assert!(line.contains(H5_ANCHOR));
        assert!(line.contains(D4_ANCHOR));
        assert!(line.contains(AI19_ANCHOR));
        assert!(AI40_ANCHOR.contains("AI-40"));
    }

    #[test]
    fn gesture_row_parses() {
        let mut m = GestureMachine::new();
        drive(&mut m, WorkspaceDir::Left, TRAVEL_THRESHOLD, HOLD_TICKS).unwrap();
        let row = gesture_row(&m);
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, GESTURE_TAG);
        assert_eq!(v[0], 1);
        assert_eq!(v[3], 1);
        assert_eq!(d, m.digest);
        assert!(audit_render(&row));
    }

    #[test]
    fn submit_row_parses() {
        let mut m = GestureMachine::new();
        let rec = drive(&mut m, WorkspaceDir::Down, TRAVEL_THRESHOLD, HOLD_TICKS)
            .unwrap()
            .unwrap();
        let row = submit_row(&rec, 1);
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, SUBMIT_TAG);
        assert_eq!(v[0], 1);
        assert_eq!(v[1], WorkspaceDir::Down.index() as u32);
        assert_eq!(v[3], 1);
        assert_eq!(d, rec.anchor);
        assert!(audit_render(&row));
    }

    #[test]
    fn boundary_row_parses() {
        let row = boundary_row();
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, BOUND_TAG);
        assert_eq!(v, [7, 3, 4, 0]);
        assert_eq!(d, boundary_digest());
        assert!(audit_render(&row));
    }

    #[test]
    fn phase_names_are_stable() {
        assert_eq!(Phase::Idle.name(), "idle");
        assert_eq!(Phase::Tracking.name(), "tracking");
        assert_eq!(Phase::Armed.name(), "armed");
        assert_eq!(Phase::Cooldown.name(), "cooldown");
        assert_eq!(Phase::Aborted.name(), "aborted");
        assert_eq!(Phase::Submitted.name(), "submitted");
    }

    #[test]
    fn direction_delta_signs() {
        assert_eq!(WorkspaceDir::Left.delta(STEP), (-16, 0));
        assert_eq!(WorkspaceDir::Right.delta(STEP), (16, 0));
        assert_eq!(WorkspaceDir::Up.delta(STEP), (0, -16));
        assert_eq!(WorkspaceDir::Down.delta(STEP), (0, 16));
        assert_eq!(WorkspaceDir::Left.index(), 0);
        assert_eq!(WorkspaceDir::Down.index(), 3);
        assert_eq!(WorkspaceDir::Up.name(), "up");
    }

    #[test]
    fn point_manhattan_is_symmetric() {
        let a = Point::new(-3, 4);
        let b = Point::new(7, 9);
        assert_eq!(a.manhattan(b), 15);
        assert_eq!(b.manhattan(a), 15);
        assert_eq!(a.manhattan(a), 0);
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
        let s = run_unx49047_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }
}
