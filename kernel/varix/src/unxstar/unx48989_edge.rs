//! UNX-F48989 手势 20 操作集（判据主轴操作集入库）· 0x71740
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族第五槽）：
//!   F48989 ① 纯功能代码 ≥300 行；② UNX-F48989-J1 20 操作全过入库。
//!   F48990 ① 纯功能代码 ≥300 行；② UNX-F48990-J1 参数 ×5 组校验一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：时钟 → 坐标窗 → 屏内合法性 → 事件×相位 → 边缘表 →
//!           （保留区优先）→ 几何（最小位移 + 方向一致）→ 终判。
//! 时钟先于坐标：坐标越界是「这一拍的数据坏了」，时钟回退是「这一拍的
//! 顺序坏了」。先报坐标会把事件乱序误诊成手指越界，排障方向被带偏；
//! 且时钟一旦回退入账，后续 duration/inward 全部不可信。
//! 坐标窗先于屏内：坐标窗（±30,000）是算术安全域，屏内（0..=宽/高）是
//! 语义域。屏外起手是被拒绝的语义输入（ZoneBad），而坐标窗越界连减法
//! 都可能出域，必须先拦。
//! 事件×相位先于边缘表：相位表是总函数，任何 (相位, 事件) 都有转移，
//! 坏事件也能在表里走一步。若只靠表，Move-before-Down 会被当成合法转移
//! 静默起算，起手点变成 (0,0)，四边分类整体漂移。
//! **保留区判定先于几何**：保留区的所有权与位移无关。若先算几何，
//! 「保留区轻点」（位移 0）会先落进 ShortTravel 变成 None，系统手势被
//! 用户侧吞掉——这正是 F48990 明令禁止的误杀。故所有权先定，几何只回答
//! 「用户区这条轨迹够不够格」。
//! 20 操作集（F48989）：每个操作登记指头区间、最小位移、是否要求边缘起手、
//! 期望方向与归一化输出族；入库前先过表审计（id 连续、名字唯一、族覆盖），
//! 再用「构造出的合法输入」逐个跑通，并配一条必然失败的负输入。
//! 参数面（F48990）：阈值/速度/时长/距离/上限五组，各自带闭区间；
//! 值只能落在 [lo, hi]，序列化格式固定为 kind=value@lo..hi，往返必须逐位相等。
//! 关键差异：本模块只回答两个问题——「这条轨迹归系统还是归用户」以及
//! 「用户区这条轨迹是否构成合法边缘滑入」。它不做边缘手势→动作映射
//! （B10-DISPATCH）、不做误触抑制的统计学习（B10-GUARD）、不做多指边缘
//! 组合（B10-COMPOSE）、不做圆角/热区变形（B10-SHAPE）、不解释系统手势
//! 语义（返回/下拉/多任务）。裁决只产出归属与理由，不产出动作码。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71740，VX6 批 B10 族第五槽）。
pub const SEG_BASE: u32 = 0x71740;

/// 屏幕逻辑宽度（含右边界坐标 1080，坐标系闭区间）。
pub const SCREEN_W: i32 = 1_080;
/// 屏幕逻辑高度（含下边界坐标 1920，坐标系闭区间）。
pub const SCREEN_H: i32 = 1_920;
/// 边缘带厚度（像素）：贴边 [0,16] / [1064,1080] 等，共 17 个整数坐标。
pub const EDGE_BAND: i32 = 16;
/// 系统手势保留区宽度（像素）：8 像素系统手势区，贴边最深 8 个坐标。
pub const RESERVED_PX: i32 = 8;
/// 最小滑入位移（像素，沿法向）。
pub const MIN_DISP: i32 = 48;
/// 坐标绝对值上界（保证差值运算不越出 i32）。
pub const COORD_MAX: i32 = 30_000;
/// 时钟上界。
pub const TICK_CAP: u32 = 1_000_000;

/// 边数（左/右/上/下）。
pub const EDGE_KINDS: usize = 4;
/// 方向数（LTR/RTL/TTB/BTT）。
pub const DIR_KINDS: usize = 4;
/// 相位数（Idle/Active/Settled/Locked）。
pub const PHASES: usize = 4;
/// 边缘事件数（Down/Move/Up/Cancel）。
pub const EDGE_EVENTS: usize = 4;
/// 相位转移数（4 × 4 = 16）。
pub const PHASE_TRANS: usize = PHASES * EDGE_EVENTS;

/// F48989 八型用例数。
pub const VARIANTS: u32 = 8;
/// F48989 千次分类轮数（卡面 ×10^3）。
pub const EDGE_ROUNDS: u32 = 1_000;
/// F48990 冲突变体数（六型）。
pub const CONFLICT_VARIANTS: u32 = 6;
/// F48990 千次冲突轮数（卡面 ×10^3）。
pub const CONFLICT_ROUNDS: u32 = 1_000;
/// fuzz 轮数（卡面 ×10^4 载体，每轮一条完整手势三分事件）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 单条手势的事件拍数（Down/Move/Up）。
pub const FUZZ_EVENTS: u32 = 3;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数（渲染/解析共用，改了必须两处同步）。
pub const EDGE_FIELDS: usize = 6;
/// 黄金轮数（64 = 8 变体 × 8，四边各覆盖两次）。
pub const GOLDEN_ROUNDS: u32 = 64;
/// 角区仲裁采样点数。
pub const CORNER_SAMPLES: usize = 9;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0E） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdCode {
    /// 时钟回退或越界
    TickBad,
    /// 坐标越出算术安全窗
    CoordBad,
    /// 屏外起手/落点
    ZoneBad,
    /// 事件在当下相位不被接受
    EventBad,
    /// 边缘表/相位表缺项、重项或歧义
    TableBad,
    /// 保留区/用户区划分非法
    BandBad,
    /// 几何量不自洽（位移、方向、极值）
    GeomBad,
    /// 方向分类与边缘要求不符
    DirBad,
    /// 保留区裁决被越权改判
    SystemBad,
    /// 分类账不平
    LedgerBad,
    /// 渲染非法（字段缺失或回读失败）
    RenderBad,
    /// 复算不一致
    ReplayBad,
    /// 覆盖不足（轮数/变体/边数不足）
    CoverBad,
    /// fuzz 处置错配
    FuzzBad,
}

impl EdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            EdCode::TickBad => "时钟回退：事件时间必须严格递增且在 10^6 内",
            EdCode::CoordBad => "坐标越界：绝对坐标越过 30,000 算术安全窗",
            EdCode::ZoneBad => "屏外起手：边缘判定只在 0..=宽/高 内成立",
            EdCode::EventBad => "事件非法：该事件在当下相位不被接受",
            EdCode::TableBad => "表非法：边缘表或相位表缺项、重项、歧义",
            EdCode::BandBad => "分区非法：保留区与用户区不构成边缘带的划分",
            EdCode::GeomBad => "几何不自洽：内移量、侧移量或极值对不上",
            EdCode::DirBad => "方向非法：滑入方向与边缘表要求不符",
            EdCode::SystemBad => "越权改判：保留区起手被判给了用户手势",
            EdCode::LedgerBad => "账不平：用户+系统+非边缘三分之和不等于轮数",
            EdCode::RenderBad => "渲染非法：字段缺失或数值回读失败",
            EdCode::ReplayBad => "复算不一致：同一序列两次裁决不同",
            EdCode::CoverBad => "覆盖不足：轮数、变体数或边数不足",
            EdCode::FuzzBad => "fuzz 处置错配：非法被接或合法被拒",
        }
    }

    pub const ALL: [EdCode; 14] = [
        EdCode::TickBad,
        EdCode::CoordBad,
        EdCode::ZoneBad,
        EdCode::EventBad,
        EdCode::TableBad,
        EdCode::BandBad,
        EdCode::GeomBad,
        EdCode::DirBad,
        EdCode::SystemBad,
        EdCode::LedgerBad,
        EdCode::RenderBad,
        EdCode::ReplayBad,
        EdCode::CoverBad,
        EdCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < EdCode::ALL.len() {
        if let Some(c) = EdCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < EdCode::ALL.len() {
        let mut j = i + 1;
        while j < EdCode::ALL.len() {
            match (EdCode::ALL.get(i), EdCode::ALL.get(j)) {
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

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < EdCode::ALL.len() {
        if let Some(c) = EdCode::ALL.get(i) {
            let v = c.val();
            if v <= SEG_BASE || v > SEG_BASE + 0x0F {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 坐标与轴 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 屏内判定（闭区间：右/下边界坐标合法）。
pub fn in_screen(p: Point) -> bool {
    p.x >= 0 && p.x <= SCREEN_W && p.y >= 0 && p.y <= SCREEN_H
}

/// 坐标窗判定（算术安全域，宽于屏内）。
pub fn in_coord_cap(p: Point) -> bool {
    p.x.unsigned_abs() <= COORD_MAX as u32 && p.y.unsigned_abs() <= COORD_MAX as u32
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub fn idx(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
        }
    }
}

// ── 四边 / 四向 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub fn idx(self) -> usize {
        match self {
            Edge::Left => 0,
            Edge::Right => 1,
            Edge::Top => 2,
            Edge::Bottom => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        }
    }

    pub const ALL: [Edge; EDGE_KINDS] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Ltr,
    Rtl,
    Ttb,
    Btt,
}

impl Dir {
    pub fn idx(self) -> usize {
        match self {
            Dir::Ltr => 0,
            Dir::Rtl => 1,
            Dir::Ttb => 2,
            Dir::Btt => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Dir::Ltr => "ltr",
            Dir::Rtl => "rtl",
            Dir::Ttb => "ttb",
            Dir::Btt => "btt",
        }
    }

    pub const ALL: [Dir; DIR_KINDS] = [Dir::Ltr, Dir::Rtl, Dir::Ttb, Dir::Btt];
}

/// 方向分类：主轴判向，等长时 X 轴优先（角平分线归横向，写死为定则）。
pub fn classify_dir(dx: i32, dy: i32) -> Option<Dir> {
    if dx == 0 && dy == 0 {
        return None;
    }
    let adx = (dx as i64).unsigned_abs();
    let ady = (dy as i64).unsigned_abs();
    if adx >= ady {
        Some(if dx > 0 { Dir::Ltr } else { Dir::Rtl })
    } else {
        Some(if dy > 0 { Dir::Ttb } else { Dir::Btt })
    }
}

// ── 边缘带 / 保留区 表 ────────────────────────────────────────────
/// 区间归属：边缘带外 / 系统保留区 / 用户区。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Outside,
    Reserved,
    User,
}

impl Band {
    pub fn idx(self) -> usize {
        match self {
            Band::Outside => 0,
            Band::Reserved => 1,
            Band::User => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Band::Outside => "outside",
            Band::Reserved => "reserved",
            Band::User => "user",
        }
    }

    pub const ALL: [Band; 3] = [Band::Outside, Band::Reserved, Band::User];
}

/// 一条边的坐标区间账：法向带 [band_lo,band_hi]，其中贴边段为系统保留区
/// [res_lo,res_hi]、内侧段为用户区 [user_lo,user_hi]；切向覆盖 [span_lo,span_hi]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeZone {
    pub edge: Edge,
    pub axis: Axis,
    pub inward: Dir,
    pub band_lo: i32,
    pub band_hi: i32,
    pub span_lo: i32,
    pub span_hi: i32,
    pub res_lo: i32,
    pub res_hi: i32,
    pub user_lo: i32,
    pub user_hi: i32,
}

/// 四边区间全表（逐条可验：左 [0,16]、右 [1064,1080]、上 [0,16]、下 [1904,1920]）。
pub const ZONES: [EdgeZone; EDGE_KINDS] = [
    EdgeZone {
        edge: Edge::Left,
        axis: Axis::X,
        inward: Dir::Ltr,
        band_lo: 0,
        band_hi: EDGE_BAND,
        span_lo: 0,
        span_hi: SCREEN_H,
        res_lo: 0,
        res_hi: RESERVED_PX - 1,
        user_lo: RESERVED_PX,
        user_hi: EDGE_BAND,
    },
    EdgeZone {
        edge: Edge::Right,
        axis: Axis::X,
        inward: Dir::Rtl,
        band_lo: SCREEN_W - EDGE_BAND,
        band_hi: SCREEN_W,
        span_lo: 0,
        span_hi: SCREEN_H,
        res_lo: SCREEN_W - RESERVED_PX + 1,
        res_hi: SCREEN_W,
        user_lo: SCREEN_W - EDGE_BAND,
        user_hi: SCREEN_W - RESERVED_PX,
    },
    EdgeZone {
        edge: Edge::Top,
        axis: Axis::Y,
        inward: Dir::Ttb,
        band_lo: 0,
        band_hi: EDGE_BAND,
        span_lo: 0,
        span_hi: SCREEN_W,
        res_lo: 0,
        res_hi: RESERVED_PX - 1,
        user_lo: RESERVED_PX,
        user_hi: EDGE_BAND,
    },
    EdgeZone {
        edge: Edge::Bottom,
        axis: Axis::Y,
        inward: Dir::Btt,
        band_lo: SCREEN_H - EDGE_BAND,
        band_hi: SCREEN_H,
        span_lo: 0,
        span_hi: SCREEN_W,
        res_lo: SCREEN_H - RESERVED_PX + 1,
        res_hi: SCREEN_H,
        user_lo: SCREEN_H - EDGE_BAND,
        user_hi: SCREEN_H - RESERVED_PX,
    },
];

/// 角区仲裁次序（写死定则：先左后右、先水平后垂直，保证角点唯一归属）。
pub const CORNER_PRIORITY: [Edge; EDGE_KINDS] =
    [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];

pub fn zone_of(edge: Edge) -> Option<EdgeZone> {
    let mut i = 0usize;
    while i < EDGE_KINDS {
        if let Some(z) = ZONES.get(i) {
            if z.edge == edge {
                return Some(*z);
            }
        }
        i += 1;
    }
    None
}

/// 单点在某条边的归属（切向越界即带外，法向按保留区/用户区切分）。
pub fn band_member(p: Point, z: &EdgeZone) -> Band {
    let (c, s) = match z.axis {
        Axis::X => (p.x, p.y),
        Axis::Y => (p.y, p.x),
    };
    if s < z.span_lo || s > z.span_hi {
        return Band::Outside;
    }
    if c < z.band_lo || c > z.band_hi {
        return Band::Outside;
    }
    if c >= z.res_lo && c <= z.res_hi {
        return Band::Reserved;
    }
    if c >= z.user_lo && c <= z.user_hi {
        return Band::User;
    }
    Band::Outside
}

/// 角区唯一仲裁：按 CORNER_PRIORITY 取第一条命中的边。
pub fn edge_hit(p: Point) -> Option<Edge> {
    let mut i = 0usize;
    while i < EDGE_KINDS {
        let e = match CORNER_PRIORITY.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if let Some(z) = zone_of(e) {
            if band_member(p, &z) != Band::Outside {
                return Some(e);
            }
        }
        i += 1;
    }
    None
}

/// 车道坐标（区段中点）：生成器与审计共用，避免硬编码魔数。
pub fn lane(edge: Edge, band: Band) -> Result<i32, u32> {
    let z = match zone_of(edge) {
        Some(v) => v,
        None => return Err(EdCode::TableBad.val()),
    };
    let (lo, hi) = match band {
        Band::Reserved => (z.res_lo, z.res_hi),
        Band::User => (z.user_lo, z.user_hi),
        Band::Outside => return Err(EdCode::ZoneBad.val()),
    };
    if hi < lo {
        return Err(EdCode::BandBad.val());
    }
    Ok(lo + (hi - lo) / 2)
}

/// 沿边法向的内移量（正向 = 向屏内）。
pub fn inward_travel(edge: Edge, a: Point, b: Point) -> i32 {
    match edge {
        Edge::Left => b.x.saturating_sub(a.x),
        Edge::Right => a.x.saturating_sub(b.x),
        Edge::Top => b.y.saturating_sub(a.y),
        Edge::Bottom => a.y.saturating_sub(b.y),
    }
}

/// 沿边切向的侧移量（带符号）。
pub fn cross_travel(edge: Edge, a: Point, b: Point) -> i32 {
    match edge {
        Edge::Left | Edge::Right => b.y.saturating_sub(a.y),
        Edge::Top | Edge::Bottom => b.x.saturating_sub(a.x),
    }
}

/// 由 (边, 内移量, 侧移量) 生成位移向量（四边共用一套口径）。
pub fn offset_for(edge: Edge, travel: i32, cross: i32) -> (i32, i32) {
    match edge {
        Edge::Left => (travel, cross),
        Edge::Right => (-travel, cross),
        Edge::Top => (cross, travel),
        Edge::Bottom => (cross, -travel),
    }
}

/// 表审计：轴一致、跨度全覆盖、带=保留区∪用户区、宽度与常量相符、
/// 边名互异、方向名互异、方向与边一一对应。返回校验过的行数。
pub fn audit_zones() -> Result<u32, u32> {
    if ZONES.len() != EDGE_KINDS {
        return Err(EdCode::TableBad.val());
    }
    let mut edge_mask = 0u32;
    let mut dir_mask = 0u32;
    let mut n = 0u32;
    let mut i = 0usize;
    while i < EDGE_KINDS {
        let z = match ZONES.get(i) {
            Some(v) => *v,
            None => return Err(EdCode::TableBad.val()),
        };
        // 轴必须与边一致。
        let axis_ok = match z.edge {
            Edge::Left | Edge::Right => z.axis == Axis::X,
            Edge::Top | Edge::Bottom => z.axis == Axis::Y,
        };
        if !axis_ok {
            return Err(EdCode::TableBad.val());
        }
        // 切向跨度必须覆盖整条垂直边长。
        let perp_len = match z.axis {
            Axis::X => SCREEN_H,
            Axis::Y => SCREEN_W,
        };
        if z.span_lo != 0 || z.span_hi != perp_len {
            return Err(EdCode::TableBad.val());
        }
        // 法向带必须落在屏内。
        let axis_len = match z.axis {
            Axis::X => SCREEN_W,
            Axis::Y => SCREEN_H,
        };
        if z.band_lo < 0 || z.band_hi > axis_len || z.band_hi <= z.band_lo {
            return Err(EdCode::BandBad.val());
        }
        // 三条宽度恒等式。
        if z.band_hi - z.band_lo + 1 != EDGE_BAND + 1 {
            return Err(EdCode::BandBad.val());
        }
        if z.res_hi - z.res_lo + 1 != RESERVED_PX {
            return Err(EdCode::BandBad.val());
        }
        if z.user_hi - z.user_lo + 1 != EDGE_BAND + 1 - RESERVED_PX {
            return Err(EdCode::BandBad.val());
        }
        // 分区必须贴边且相邻（并集恰为整条带，交集为空）。
        let res_flush = z.res_lo == z.band_lo || z.res_hi == z.band_hi;
        let user_flush = z.user_lo == z.band_lo || z.user_hi == z.band_hi;
        let contiguous = z.res_hi + 1 == z.user_lo || z.user_hi + 1 == z.res_lo;
        if !res_flush || !user_flush || !contiguous {
            return Err(EdCode::BandBad.val());
        }
        // 边名与方向名各占一位（全表一一对应）。
        let eb = 1u32 << z.edge.idx();
        if edge_mask & eb != 0 {
            return Err(EdCode::TableBad.val());
        }
        edge_mask |= eb;
        let db = 1u32 << z.inward.idx();
        if dir_mask & db != 0 {
            return Err(EdCode::TableBad.val());
        }
        dir_mask |= db;
        // 方向必须与边的内法向一致（左→LTR、右→RTL、上→TTB、下→BTT）。
        let want = match z.edge {
            Edge::Left => Dir::Ltr,
            Edge::Right => Dir::Rtl,
            Edge::Top => Dir::Ttb,
            Edge::Bottom => Dir::Btt,
        };
        if z.inward != want {
            return Err(EdCode::DirBad.val());
        }
        n = n.saturating_add(1);
        i += 1;
    }
    let full = (1u32 << EDGE_KINDS) - 1;
    let full_dir = (1u32 << DIR_KINDS) - 1;
    if edge_mask != full || dir_mask != full_dir {
        return Err(EdCode::TableBad.val());
    }
    Ok(n)
}

/// 角区采样表（x, y, 期望边编码：0=无, idx+1）。
pub const CORNER_TABLE: [(i32, i32, u8); CORNER_SAMPLES] = [
    (0, 0, 1),
    (SCREEN_W, 0, 2),
    (SCREEN_W, SCREEN_H, 2),
    (0, SCREEN_H, 1),
    (SCREEN_W / 2, 0, 3),
    (SCREEN_W / 2, SCREEN_H, 4),
    (0, SCREEN_H / 2, 1),
    (SCREEN_W, SCREEN_H / 2, 2),
    (SCREEN_W / 2, SCREEN_H / 2, 0),
];

/// 角区唯一 + 确定性审计：九个采样点各归一条边（或都不归），两次结果相同。
pub fn audit_corner_priority() -> Result<u32, u32> {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CORNER_TABLE.len() {
        let (x, y, want) = match CORNER_TABLE.get(i) {
            Some(v) => *v,
            None => return Err(EdCode::TableBad.val()),
        };
        let p = Point { x, y };
        let a = edge_hit(p);
        let b = edge_hit(p);
        if a != b {
            return Err(EdCode::ReplayBad.val());
        }
        let got = match a {
            Some(e) => e.idx() as u8 + 1,
            None => 0u8,
        };
        if got != want {
            return Err(EdCode::TableBad.val());
        }
        n = n.saturating_add(1);
        i += 1;
    }
    Ok(n)
}

fn audit_names(names: &[&'static str]) -> Result<u32, u32> {
    let mut i = 0usize;
    while i < names.len() {
        let a = match names.get(i) {
            Some(v) => *v,
            None => return Err(EdCode::TableBad.val()),
        };
        if a.is_empty() {
            return Err(EdCode::TableBad.val());
        }
        let mut j = i + 1;
        while j < names.len() {
            let b = match names.get(j) {
                Some(v) => *v,
                None => return Err(EdCode::TableBad.val()),
            };
            if a == b {
                return Err(EdCode::TableBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(names.len() as u32)
}

/// 标签审计：日志口径的名字必须互异（同名会让日志无法区分归属）。
pub fn audit_labels() -> Result<u32, u32> {
    let mut n = 0u32;
    n = n.saturating_add(audit_names(&[
        Edge::Left.name(),
        Edge::Right.name(),
        Edge::Top.name(),
        Edge::Bottom.name(),
    ])?);
    n = n.saturating_add(audit_names(&[
        Dir::Ltr.name(),
        Dir::Rtl.name(),
        Dir::Ttb.name(),
        Dir::Btt.name(),
    ])?);
    n = n.saturating_add(audit_names(&[
        Axis::X.name(),
        Axis::Y.name(),
    ])?);
    n = n.saturating_add(audit_names(&[
        Band::Outside.name(),
        Band::Reserved.name(),
        Band::User.name(),
    ])?);
    n = n.saturating_add(audit_names(&[
        Phase::Idle.name(),
        Phase::Active.name(),
        Phase::Settled.name(),
        Phase::Locked.name(),
    ])?);
    n = n.saturating_add(audit_names(&[
        EdgeEv::Down.name(),
        EdgeEv::Move.name(),
        EdgeEv::Up.name(),
        EdgeEv::Cancel.name(),
    ])?);
    n = n.saturating_add(audit_names(&[
        Verdict::None.name(),
        Verdict::User.name(),
        Verdict::System.name(),
    ])?);
    n = n.saturating_add(audit_names(&[
        Why::OutsideEdge.name(),
        Why::ReservedBand.name(),
        Why::UserBand.name(),
        Why::ShortTravel.name(),
        Why::WrongDirection.name(),
    ])?);
    Ok(n)
}

// ── 裁决器（F48990 的唯一权威） ───────────────────────────────────
/// 归属判定：非边缘 / 用户手势 / 系统手势。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    None,
    User,
    System,
}

impl Verdict {
    pub fn idx(self) -> usize {
        match self {
            Verdict::None => 0,
            Verdict::User => 1,
            Verdict::System => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Verdict::None => "none",
            Verdict::User => "user",
            Verdict::System => "system",
        }
    }

    pub const ALL: [Verdict; 3] = [Verdict::None, Verdict::User, Verdict::System];
}

/// 裁决理由（每个裁决必须给出且只给出一条理由，便于逐条对账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    OutsideEdge,
    ReservedBand,
    UserBand,
    ShortTravel,
    WrongDirection,
}

impl Why {
    pub fn idx(self) -> usize {
        match self {
            Why::OutsideEdge => 0,
            Why::ReservedBand => 1,
            Why::UserBand => 2,
            Why::ShortTravel => 3,
            Why::WrongDirection => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Why::OutsideEdge => "outside-edge",
            Why::ReservedBand => "reserved-band",
            Why::UserBand => "user-band",
            Why::ShortTravel => "short-travel",
            Why::WrongDirection => "wrong-direction",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Arbitration {
    pub verdict: Verdict,
    pub edge: Option<Edge>,
    pub dir: Option<Dir>,
    pub inward: i32,
    pub why: Why,
}

/// (归属, 理由) 的合法配对：合法手势不会被合法地冠以错误理由。
pub fn pair_ok(a: &Arbitration) -> bool {
    match (a.verdict, a.why) {
        (Verdict::User, Why::UserBand) => true,
        (Verdict::System, Why::ReservedBand) => true,
        (Verdict::None, Why::OutsideEdge) => true,
        (Verdict::None, Why::ShortTravel) => true,
        (Verdict::None, Why::WrongDirection) => true,
        _ => false,
    }
}

/// 冲突裁决：给定起手点与抬手点，返回归属。
///
/// 闸门次序：坐标窗 → 屏内 → 边缘表 → 保留区优先 → 几何。
/// 保留区优先**先于**几何：所有权与位移无关（见模块头注）。
pub fn arbitrate(start: Point, end: Point) -> Result<Arbitration, u32> {
    if !in_coord_cap(start) || !in_coord_cap(end) {
        return Err(EdCode::CoordBad.val());
    }
    if !in_screen(start) || !in_screen(end) {
        return Err(EdCode::ZoneBad.val());
    }
    let edge = match edge_hit(start) {
        None => {
            return Ok(Arbitration {
                verdict: Verdict::None,
                edge: None,
                dir: classify_dir(
                    end.x.saturating_sub(start.x),
                    end.y.saturating_sub(start.y),
                ),
                inward: 0,
                why: Why::OutsideEdge,
            });
        }
        Some(e) => e,
    };
    let z = match zone_of(edge) {
        Some(v) => v,
        None => return Err(EdCode::TableBad.val()),
    };
    let band = band_member(start, &z);
    let inward = inward_travel(edge, start, end);
    let dir = classify_dir(end.x.saturating_sub(start.x), end.y.saturating_sub(start.y));
    // 保留区优先：起手落在系统手势区，归属唯一且与位移无关。
    if band == Band::Reserved {
        return Ok(Arbitration {
            verdict: Verdict::System,
            edge: Some(edge),
            dir,
            inward,
            why: Why::ReservedBand,
        });
    }
    if band != Band::User {
        return Err(EdCode::BandBad.val());
    }
    // 几何：先阈值后方向（位移不足时不谈方向，理由更准确）。
    if inward < MIN_DISP {
        return Ok(Arbitration {
            verdict: Verdict::None,
            edge: Some(edge),
            dir,
            inward,
            why: Why::ShortTravel,
        });
    }
    if dir != Some(z.inward) {
        return Ok(Arbitration {
            verdict: Verdict::None,
            edge: Some(edge),
            dir,
            inward,
            why: Why::WrongDirection,
        });
    }
    Ok(Arbitration {
        verdict: Verdict::User,
        edge: Some(edge),
        dir,
        inward,
        why: Why::UserBand,
    })
}

// ── 相位 / 事件 / 动作（4 × 4 = 16 转移全表） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Active,
    Settled,
    Locked,
}

impl Phase {
    pub fn idx(self) -> usize {
        match self {
            Phase::Idle => 0,
            Phase::Active => 1,
            Phase::Settled => 2,
            Phase::Locked => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Active => "active",
            Phase::Settled => "settled",
            Phase::Locked => "locked",
        }
    }

    pub const ALL: [Phase; PHASES] = [Phase::Idle, Phase::Active, Phase::Settled, Phase::Locked];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeEv {
    Down,
    Move,
    Up,
    Cancel,
}

impl EdgeEv {
    pub fn idx(self) -> usize {
        match self {
            EdgeEv::Down => 0,
            EdgeEv::Move => 1,
            EdgeEv::Up => 2,
            EdgeEv::Cancel => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeEv::Down => "down",
            EdgeEv::Move => "move",
            EdgeEv::Up => "up",
            EdgeEv::Cancel => "cancel",
        }
    }

    pub const ALL: [EdgeEv; EDGE_EVENTS] =
        [EdgeEv::Down, EdgeEv::Move, EdgeEv::Up, EdgeEv::Cancel];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    None,
    Begin,
    Acc,
    Emit,
    Cancel,
    Reset,
}

impl Act {
    pub fn code(self) -> u8 {
        match self {
            Act::None => 0,
            Act::Begin => 1,
            Act::Acc => 2,
            Act::Emit => 3,
            Act::Cancel => 4,
            Act::Reset => 5,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PhaseTrans {
    pub from: Phase,
    pub ev: EdgeEv,
    pub to: Phase,
    pub act: Act,
}

/// 16 条转移全表（按 相位 × 事件 展开）。
pub const PHASE_TABLE: [PhaseTrans; PHASE_TRANS] = [
    // Idle
    PhaseTrans { from: Phase::Idle, ev: EdgeEv::Down, to: Phase::Active, act: Act::Begin },
    PhaseTrans { from: Phase::Idle, ev: EdgeEv::Move, to: Phase::Idle, act: Act::None },
    PhaseTrans { from: Phase::Idle, ev: EdgeEv::Up, to: Phase::Idle, act: Act::None },
    PhaseTrans { from: Phase::Idle, ev: EdgeEv::Cancel, to: Phase::Idle, act: Act::None },
    // Active
    PhaseTrans { from: Phase::Active, ev: EdgeEv::Down, to: Phase::Active, act: Act::None },
    PhaseTrans { from: Phase::Active, ev: EdgeEv::Move, to: Phase::Active, act: Act::Acc },
    PhaseTrans { from: Phase::Active, ev: EdgeEv::Up, to: Phase::Settled, act: Act::Emit },
    PhaseTrans { from: Phase::Active, ev: EdgeEv::Cancel, to: Phase::Locked, act: Act::Cancel },
    // Settled
    PhaseTrans { from: Phase::Settled, ev: EdgeEv::Down, to: Phase::Active, act: Act::Begin },
    PhaseTrans { from: Phase::Settled, ev: EdgeEv::Move, to: Phase::Settled, act: Act::None },
    PhaseTrans { from: Phase::Settled, ev: EdgeEv::Up, to: Phase::Settled, act: Act::None },
    PhaseTrans { from: Phase::Settled, ev: EdgeEv::Cancel, to: Phase::Settled, act: Act::None },
    // Locked
    PhaseTrans { from: Phase::Locked, ev: EdgeEv::Down, to: Phase::Locked, act: Act::None },
    PhaseTrans { from: Phase::Locked, ev: EdgeEv::Move, to: Phase::Locked, act: Act::None },
    PhaseTrans { from: Phase::Locked, ev: EdgeEv::Up, to: Phase::Idle, act: Act::Reset },
    PhaseTrans { from: Phase::Locked, ev: EdgeEv::Cancel, to: Phase::Idle, act: Act::Reset },
];

/// 查表（总函数，缺项即表坏）。
pub fn phase_step(from: Phase, ev: EdgeEv) -> Result<(Phase, Act), u32> {
    let mut i = 0usize;
    while i < PHASE_TRANS {
        if let Some(t) = PHASE_TABLE.get(i) {
            if t.from == from && t.ev == ev {
                return Ok((t.to, t.act));
            }
        }
        i += 1;
    }
    Err(EdCode::TableBad.val())
}

/// 事件合法性闸（先于查表）：非法组合必须在闸上被拒，而不是靠表空转。
pub fn event_legal(phase: Phase, ev: EdgeEv) -> bool {
    match phase {
        Phase::Idle => ev == EdgeEv::Down,
        Phase::Active => ev == EdgeEv::Move || ev == EdgeEv::Up || ev == EdgeEv::Cancel,
        Phase::Settled => ev == EdgeEv::Down,
        Phase::Locked => ev == EdgeEv::Up || ev == EdgeEv::Cancel,
    }
}

/// 16 格审计：全组合命中、唯一、动作码在闭集内。
pub fn audit_phase_table() -> Result<u32, u32> {
    if PHASE_TABLE.len() != PHASE_TRANS {
        return Err(EdCode::TableBad.val());
    }
    let mut seen = [[0u8; EDGE_EVENTS]; PHASES];
    let mut i = 0usize;
    while i < PHASE_TRANS {
        let t = match PHASE_TABLE.get(i) {
            Some(v) => v,
            None => return Err(EdCode::TableBad.val()),
        };
        if t.act.code() > 5 {
            return Err(EdCode::TableBad.val());
        }
        let row = t.from.idx();
        let col = t.ev.idx();
        if row >= PHASES || col >= EDGE_EVENTS {
            return Err(EdCode::TableBad.val());
        }
        match seen.get_mut(row).and_then(|r| r.get_mut(col)) {
            Some(slot) => {
                if *slot != 0 {
                    return Err(EdCode::TableBad.val());
                }
                *slot = 1;
            }
            None => return Err(EdCode::TableBad.val()),
        }
        let _ = phase_step(t.from, t.ev)?;
        i += 1;
    }
    let mut r = 0usize;
    while r < PHASES {
        let mut c = 0usize;
        while c < EDGE_EVENTS {
            match seen.get(r).and_then(|row| row.get(c)) {
                Some(1) => {}
                _ => return Err(EdCode::TableBad.val()),
            }
            c += 1;
        }
        r += 1;
    }
    Ok(PHASE_TRANS as u32)
}

/// 闸表一致性审计：合法组合的动作不得为 None，非法组合的动作必须是 None。
pub fn audit_gate_consistency() -> Result<u32, u32> {
    let mut n = 0u32;
    let mut r = 0usize;
    while r < PHASES {
        let ph = match Phase::ALL.get(r) {
            Some(v) => *v,
            None => return Err(EdCode::CoverBad.val()),
        };
        let mut c = 0usize;
        while c < EDGE_EVENTS {
            let ev = match EdgeEv::ALL.get(c) {
                Some(v) => *v,
                None => return Err(EdCode::CoverBad.val()),
            };
            let (_to, act) = phase_step(ph, ev)?;
            if event_legal(ph, ev) {
                if act == Act::None {
                    return Err(EdCode::EventBad.val());
                }
            } else if act != Act::None {
                return Err(EdCode::EventBad.val());
            }
            n = n.saturating_add(1);
            c += 1;
        }
        r += 1;
    }
    Ok(n)
}

// ── 跟踪引擎（六道闸 + 饱和计数） ─────────────────────────────────
/// 起手归属角色（按下瞬间由区间表定，不再变）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Plain,
    User,
    System,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    pub phase: Phase,
    pub act: Act,
    pub verdict: Verdict,
    pub edge: Option<Edge>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeTrack {
    pub phase: Phase,
    pub role: Role,
    pub edge: Option<Edge>,
    pub start: Point,
    pub last: Point,
    pub start_tick: u32,
    pub tick: u32,
    pub moves: u32,
    pub inward: i32,
    pub max_inward: i32,
    pub dir: Option<Dir>,
    pub verdict: Verdict,
    pub why: Why,
    pub user_emitted: u32,
    pub system_emitted: u32,
    pub none_emitted: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub digest: u64,
}

impl EdgeTrack {
    pub fn new() -> EdgeTrack {
        EdgeTrack {
            phase: Phase::Idle,
            role: Role::Plain,
            edge: None,
            start: Point { x: 0, y: 0 },
            last: Point { x: 0, y: 0 },
            start_tick: 0,
            tick: 0,
            moves: 0,
            inward: 0,
            max_inward: 0,
            dir: None,
            verdict: Verdict::None,
            why: Why::OutsideEdge,
            user_emitted: 0,
            system_emitted: 0,
            none_emitted: 0,
            accepted: 0,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    fn reject(&mut self, code: u32) -> Result<Step, u32> {
        self.rejected = self.rejected.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    /// 六道闸：时钟 → 坐标窗 → 屏内 → 事件×相位 → 边缘表 → 几何（终判）。
    pub fn touch(&mut self, ev: EdgeEv, p: Point, tick: u32) -> Result<Step, u32> {
        // 闸 1：时钟（顺序坏先于数据坏）。
        if tick <= self.tick || tick > TICK_CAP {
            return self.reject(EdCode::TickBad.val());
        }
        // 闸 2：坐标窗（算术安全域）。
        if !in_coord_cap(p) {
            return self.reject(EdCode::CoordBad.val());
        }
        // 闸 3：屏内（语义域）。
        if !in_screen(p) {
            return self.reject(EdCode::ZoneBad.val());
        }
        // 闸 4：事件 × 相位。
        if !event_legal(self.phase, ev) {
            return self.reject(EdCode::EventBad.val());
        }
        // 闸 5：边缘表（仅按下时查表；命中必须能取回唯一一区）。
        let mut down_edge: Option<Edge> = None;
        let mut down_role = self.role;
        if ev == EdgeEv::Down {
            down_edge = edge_hit(p);
            down_role = match down_edge {
                None => Role::Plain,
                Some(e) => {
                    let z = match zone_of(e) {
                        Some(v) => v,
                        None => return self.reject(EdCode::TableBad.val()),
                    };
                    match band_member(p, &z) {
                        Band::Reserved => Role::System,
                        Band::User => Role::User,
                        Band::Outside => return self.reject(EdCode::BandBad.val()),
                    }
                }
            };
        }
        // 相位表查表（总函数，缺项即表坏）。
        let (next, act) = match phase_step(self.phase, ev) {
            Ok(v) => v,
            Err(code) => return self.reject(code),
        };
        // ── 所有闸已过，开始入账 ──
        if ev == EdgeEv::Down {
            self.start = p;
            self.last = p;
            self.start_tick = tick;
            self.moves = 0;
            self.inward = 0;
            self.max_inward = 0;
            self.dir = None;
            self.verdict = Verdict::None;
            self.why = Why::OutsideEdge;
            self.edge = down_edge;
            self.role = down_role;
        }
        if ev == EdgeEv::Move {
            if let Some(e) = self.edge {
                let t = inward_travel(e, self.start, p);
                self.inward = t;
                if t > self.max_inward {
                    self.max_inward = t;
                }
            }
            self.dir = classify_dir(
                p.x.saturating_sub(self.start.x),
                p.y.saturating_sub(self.start.y),
            );
            self.last = p;
            self.moves = self.moves.saturating_add(1);
        }
        if ev == EdgeEv::Up {
            if let Some(e) = self.edge {
                let t = inward_travel(e, self.start, p);
                self.inward = t;
                if t > self.max_inward {
                    self.max_inward = t;
                }
            }
            if self.role == Role::Plain {
                self.verdict = Verdict::None;
                self.why = Why::OutsideEdge;
                self.dir = classify_dir(
                    p.x.saturating_sub(self.start.x),
                    p.y.saturating_sub(self.start.y),
                );
            } else {
                // 终判交给裁决器（与 F48990 同一权威，禁止二次口径）。
                let a = match arbitrate(self.start, p) {
                    Ok(v) => v,
                    Err(code) => return self.reject(code),
                };
                if a.edge != self.edge {
                    return self.reject(EdCode::SystemBad.val());
                }
                if !pair_ok(&a) {
                    return self.reject(EdCode::GeomBad.val());
                }
                self.verdict = a.verdict;
                self.why = a.why;
                self.dir = a.dir;
            }
            self.last = p;
            match self.verdict {
                Verdict::User => self.user_emitted = self.user_emitted.saturating_add(1),
                Verdict::System => self.system_emitted = self.system_emitted.saturating_add(1),
                Verdict::None => self.none_emitted = self.none_emitted.saturating_add(1),
            }
        }
        if ev == EdgeEv::Cancel {
            self.verdict = Verdict::None;
            self.why = Why::OutsideEdge;
            self.dir = None;
            self.last = p;
        }
        self.phase = next;
        self.tick = tick;
        self.accepted = self.accepted.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, next.idx() as u32);
        self.digest = p_core::fold_u32(self.digest, ev.idx() as u32);
        self.digest = p_core::fold_u32(self.digest, self.verdict.idx() as u32);
        self.digest = p_core::fold_u32(self.digest, tick);
        Ok(Step {
            phase: next,
            act,
            verdict: self.verdict,
            edge: self.edge,
        })
    }

    /// 引擎审计：计数饱和、账目自洽、极值单调、终判与角色一致。
    pub fn audit(&self) -> Result<(), u32> {
        if self.accepted.saturating_add(self.rejected) == 0 {
            return Err(EdCode::CoverBad.val());
        }
        if self.digest == 0 {
            return Err(EdCode::ReplayBad.val());
        }
        let emitted = self
            .user_emitted
            .saturating_add(self.system_emitted)
            .saturating_add(self.none_emitted);
        if emitted > self.accepted {
            return Err(EdCode::LedgerBad.val());
        }
        if self.max_inward < self.inward {
            return Err(EdCode::GeomBad.val());
        }
        if self.phase == Phase::Active && self.start_tick >= self.tick {
            return Err(EdCode::TickBad.val());
        }
        if self.phase == Phase::Settled {
            match self.verdict {
                Verdict::User => {
                    if self.role != Role::User {
                        return Err(EdCode::SystemBad.val());
                    }
                }
                Verdict::System => {
                    if self.role != Role::System {
                        return Err(EdCode::SystemBad.val());
                    }
                }
                Verdict::None => {}
            }
        }
        Ok(())
    }
}

// ── F48989 八型用例（四边 × 八型 = 逐条可验） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeCase {
    pub index: u32,
    pub variant: u32,
    pub edge: Edge,
    pub band: Band,
    pub start: Point,
    pub end: Point,
    pub expect: Verdict,
    pub why: Why,
}

/// 八型口径（三要素逐条写死）：
///   0 用户区合法滑入：内移 = 48+6+(i%5)×3，侧移 = 53（< 内移，方向一致）
///   1 用户区临界滑入：内移 = 48，侧移 = 47（恰在阈值上）
///   2 保留区大位移：内移 = 88 → 系统（所有权与位移无关）
///   3 保留区原地按下：内移 = 0 → 系统（轻点不得被几何闸吞掉）
///   4 屏心起手：非边缘 → None / outside-edge
///   5 用户区内移 47（差 1px）→ None / short-travel
///   6 用户区反向滑出：内移 = -12 → None / short-travel
///   7 用户区横向主导：内移 50、侧移 88 → None / wrong-direction
pub fn edge_expect(i: u32) -> Result<EdgeCase, u32> {
    if i >= EDGE_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let variant = i % VARIANTS;
    let edge = match Edge::ALL.get(((i / VARIANTS) as usize) % EDGE_KINDS) {
        Some(v) => *v,
        None => return Err(EdCode::CoverBad.val()),
    };
    let band = match variant {
        0 | 1 | 5 | 6 | 7 => Band::User,
        2 | 3 => Band::Reserved,
        _ => Band::Outside,
    };
    let span = 240 + (i % 5) as i32 * 60;
    let (start, travel, cross, expect, why) = match band {
        Band::Outside => (
            Point { x: SCREEN_W / 2, y: SCREEN_H / 2 },
            200,
            -150,
            Verdict::None,
            Why::OutsideEdge,
        ),
        _ => {
            let lane_c = lane(edge, band)?;
            let p = match edge {
                Edge::Left | Edge::Right => Point { x: lane_c, y: span },
                Edge::Top | Edge::Bottom => Point { x: span, y: lane_c },
            };
            let (t, c, v, w) = match variant {
                0 => (
                    MIN_DISP + 6 + (i % 5) as i32 * 3,
                    MIN_DISP + 5,
                    Verdict::User,
                    Why::UserBand,
                ),
                1 => (MIN_DISP, MIN_DISP - 1, Verdict::User, Why::UserBand),
                2 => (MIN_DISP + 40, 0, Verdict::System, Why::ReservedBand),
                3 => (0, 0, Verdict::System, Why::ReservedBand),
                5 => (MIN_DISP - 1, 0, Verdict::None, Why::ShortTravel),
                6 => (-(MIN_DISP / 4), 0, Verdict::None, Why::ShortTravel),
                _ => (MIN_DISP + 2, MIN_DISP + 40, Verdict::None, Why::WrongDirection),
            };
            (p, t, c, v, w)
        }
    };
    let (dx, dy) = offset_for(edge, travel, cross);
    let end = Point {
        x: start.x.saturating_add(dx),
        y: start.y.saturating_add(dy),
    };
    if !in_screen(start) || !in_screen(end) {
        return Err(EdCode::ZoneBad.val());
    }
    Ok(EdgeCase {
        index: i,
        variant,
        edge,
        band,
        start,
        end,
        expect,
        why,
    })
}

/// 独立于引擎的期望三分账（只看变体位，不看裁决结果）。
pub fn expected_ledger(rounds: u32) -> Result<(u32, u32, u32), u32> {
    if rounds == 0 || rounds > EDGE_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut user = 0u32;
    let mut system = 0u32;
    let mut none = 0u32;
    let mut i = 0u32;
    while i < rounds {
        match i % VARIANTS {
            0 | 1 => user = user.saturating_add(1),
            2 | 3 => system = system.saturating_add(1),
            _ => none = none.saturating_add(1),
        }
        i = i.saturating_add(1);
    }
    Ok((user, system, none))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeStats {
    pub attempts: u32,
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
    pub system: u32,
    pub none: u32,
    pub short: u32,
    pub digest: u64,
}

impl EdgeStats {
    pub fn new() -> EdgeStats {
        EdgeStats {
            attempts: 0,
            left: 0,
            right: 0,
            top: 0,
            bottom: 0,
            system: 0,
            none: 0,
            short: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn user_total(&self) -> u32 {
        self.left
            .saturating_add(self.right)
            .saturating_add(self.top)
            .saturating_add(self.bottom)
    }

    pub fn balanced(&self) -> bool {
        self.attempts > 0
            && self.attempts
                == self
                    .user_total()
                    .saturating_add(self.system)
                    .saturating_add(self.none)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(EdCode::LedgerBad.val());
        }
        if self.short > self.none {
            return Err(EdCode::GeomBad.val());
        }
        if self.user_total() == 0 {
            return Err(EdCode::CoverBad.val());
        }
        if self.digest == 0 {
            return Err(EdCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// F48989 千次分类：每条用例喂给引擎（Down/Move/Up 三拍），逐条比期望。
pub fn edge_rounds(rounds: u32) -> Result<EdgeStats, u32> {
    if rounds == 0 || rounds > EDGE_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut s = EdgeStats::new();
    let mut i = 0u32;
    while i < rounds {
        let c = edge_expect(i)?;
        let mut t = EdgeTrack::new();
        let _ = t.touch(EdgeEv::Down, c.start, 1)?;
        let mid = Point {
            x: c.start.x + (c.end.x - c.start.x) / 2,
            y: c.start.y + (c.end.y - c.start.y) / 2,
        };
        let _ = t.touch(EdgeEv::Move, mid, 2)?;
        let step = t.touch(EdgeEv::Up, c.end, 3)?;
        t.audit()?;
        if t.moves != 1 || step.phase != Phase::Settled {
            return Err(EdCode::GeomBad.val());
        }
        if step.verdict != c.expect {
            if c.expect == Verdict::User && step.verdict == Verdict::System {
                return Err(EdCode::SystemBad.val());
            }
            return Err(EdCode::GeomBad.val());
        }
        match step.verdict {
            Verdict::User => match c.edge {
                Edge::Left => s.left = s.left.saturating_add(1),
                Edge::Right => s.right = s.right.saturating_add(1),
                Edge::Top => s.top = s.top.saturating_add(1),
                Edge::Bottom => s.bottom = s.bottom.saturating_add(1),
            },
            Verdict::System => s.system = s.system.saturating_add(1),
            Verdict::None => {
                s.none = s.none.saturating_add(1);
                if c.why == Why::ShortTravel {
                    s.short = s.short.saturating_add(1);
                }
            }
        }
        s.attempts = s.attempts.saturating_add(1);
        s.digest = p_core::fold_u32(s.digest, c.edge.idx() as u32);
        s.digest = p_core::fold_u32(s.digest, c.variant);
        s.digest = p_core::fold_u32(s.digest, step.verdict.idx() as u32);
        i = i.saturating_add(1);
    }
    let (u, sys, n) = expected_ledger(rounds)?;
    if s.user_total() != u || s.system != sys || s.none != n {
        return Err(EdCode::LedgerBad.val());
    }
    s.audit()?;
    Ok(s)
}

// ── F48990 六型冲突用例 ──────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictCase {
    pub index: u32,
    pub variant: u32,
    pub edge: Edge,
    pub band: Band,
    pub start: Point,
    pub end: Point,
    pub expect: Verdict,
    pub why: Why,
}

/// 六型冲突口径：
///   0 保留区 + 内移 78 → 系统（系统手势区优先）
///   1 保留区 + 外移 3（贴边回弹）→ 系统（归系统即定，按下去就定了）
///   2 用户区 + 内移 68 → 用户（保留区外不得被误杀）
///   3 用户区 + 内移 48 侧移 46（临界合法）→ 用户
///   4 用户区 + 内移 47（差 1px）→ 非边缘（阈值不是保留区的锅）
///   5 屏心 + 斜移 → 非边缘
pub fn conflict_expect(i: u32) -> Result<ConflictCase, u32> {
    if i >= CONFLICT_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let variant = i % CONFLICT_VARIANTS;
    let edge = match Edge::ALL.get(((i / CONFLICT_VARIANTS) as usize) % EDGE_KINDS) {
        Some(v) => *v,
        None => return Err(EdCode::CoverBad.val()),
    };
    let span = 300 + (i % 6) as i32 * 50;
    let (band, travel, cross, expect, why) = match variant {
        0 => (
            Band::Reserved,
            MIN_DISP + 30,
            0,
            Verdict::System,
            Why::ReservedBand,
        ),
        1 => (
            Band::Reserved,
            -(MIN_DISP / 16),
            0,
            Verdict::System,
            Why::ReservedBand,
        ),
        2 => (
            Band::User,
            MIN_DISP + 20,
            0,
            Verdict::User,
            Why::UserBand,
        ),
        3 => (
            Band::User,
            MIN_DISP,
            MIN_DISP - 2,
            Verdict::User,
            Why::UserBand,
        ),
        4 => (
            Band::User,
            MIN_DISP - 1,
            0,
            Verdict::None,
            Why::ShortTravel,
        ),
        _ => (
            Band::Outside,
            120,
            -90,
            Verdict::None,
            Why::OutsideEdge,
        ),
    };
    let start = match band {
        Band::Outside => Point {
            x: SCREEN_W / 2,
            y: SCREEN_H / 2,
        },
        _ => {
            let lane_c = lane(edge, band)?;
            match edge {
                Edge::Left | Edge::Right => Point { x: lane_c, y: span },
                Edge::Top | Edge::Bottom => Point { x: span, y: lane_c },
            }
        }
    };
    let (dx, dy) = offset_for(edge, travel, cross);
    let end = Point {
        x: start.x.saturating_add(dx),
        y: start.y.saturating_add(dy),
    };
    if !in_screen(start) || !in_screen(end) {
        return Err(EdCode::ZoneBad.val());
    }
    Ok(ConflictCase {
        index: i,
        variant,
        edge,
        band,
        start,
        end,
        expect,
        why,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictStats {
    pub attempts: u32,
    pub system_win: u32,
    pub user_win: u32,
    pub no_conflict: u32,
    pub false_kill: u32,
    pub jitter: u32,
    pub digest: u64,
}

impl ConflictStats {
    pub fn new() -> ConflictStats {
        ConflictStats {
            attempts: 0,
            system_win: 0,
            user_win: 0,
            no_conflict: 0,
            false_kill: 0,
            jitter: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn balanced(&self) -> bool {
        self.attempts > 0
            && self.attempts
                == self
                    .system_win
                    .saturating_add(self.user_win)
                    .saturating_add(self.no_conflict)
    }

    /// 零抖动 + 零误杀（F48990 的两条硬判据）。
    pub fn clean(&self) -> bool {
        self.false_kill == 0 && self.jitter == 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(EdCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(EdCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// F48990 千次冲突：每条用例裁决两次（抖动检测），并与期望逐条对齐。
pub fn conflict_rounds(rounds: u32) -> Result<ConflictStats, u32> {
    if rounds == 0 || rounds > CONFLICT_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut s = ConflictStats::new();
    let mut i = 0u32;
    while i < rounds {
        let c = conflict_expect(i)?;
        let a = arbitrate(c.start, c.end)?;
        let b = arbitrate(c.start, c.end)?;
        if a != b {
            s.jitter = s.jitter.saturating_add(1);
        }
        if !pair_ok(&a) {
            return Err(EdCode::GeomBad.val());
        }
        if a.verdict != c.expect {
            // 用户区合法手势被系统吞掉 = 误杀；其余属实现错配。
            if c.expect == Verdict::User && a.verdict == Verdict::System {
                s.false_kill = s.false_kill.saturating_add(1);
            } else {
                return Err(EdCode::GeomBad.val());
            }
        }
        if a.why != c.why {
            return Err(EdCode::BandBad.val());
        }
        match a.verdict {
            Verdict::System => s.system_win = s.system_win.saturating_add(1),
            Verdict::User => s.user_win = s.user_win.saturating_add(1),
            Verdict::None => s.no_conflict = s.no_conflict.saturating_add(1),
        }
        s.attempts = s.attempts.saturating_add(1);
        s.digest = p_core::fold_u32(s.digest, c.variant);
        s.digest = p_core::fold_u32(s.digest, a.verdict.idx() as u32);
        s.digest = p_core::fold_u32(s.digest, a.why.idx() as u32);
        s.digest = p_core::fold_u32(s.digest, a.inward.unsigned_abs());
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

// ── fuzz（LCG 高位取模，×10^4 轮） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeFuzz {
    pub attempts: u32,
    pub user: u32,
    pub system: u32,
    pub none: u32,
    pub short: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub engine_mismatch: u32,
    pub false_user: u32,
    pub digest: u64,
}

impl EdgeFuzz {
    pub fn new() -> EdgeFuzz {
        EdgeFuzz {
            attempts: 0,
            user: 0,
            system: 0,
            none: 0,
            short: 0,
            accepted: 0,
            rejected: 0,
            engine_mismatch: 0,
            false_user: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn balanced(&self) -> bool {
        self.attempts > 0
            && self.attempts
                == self
                    .user
                    .saturating_add(self.system)
                    .saturating_add(self.none)
    }

    /// 两路（裁决器 / 引擎）必须一致，且不得出现非法配对。
    pub fn clean(&self) -> bool {
        self.engine_mismatch == 0 && self.false_user == 0 && self.balanced()
    }

    pub fn events(&self) -> u32 {
        self.accepted.saturating_add(self.rejected)
    }
}

/// fuzz：随机 边 × 区 × (内移, 侧移)，裁决器与引擎双路对齐。
pub fn fuzz_edges(rounds: u32) -> Result<EdgeFuzz, u32> {
    if rounds == 0 {
        return Err(EdCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x7174_0F89_0000_0004);
    let mut f = EdgeFuzz::new();
    let mut i = 0u32;
    while i < rounds {
        let eidx = ((rng.next() >> 33) as usize) % EDGE_KINDS;
        let edge = match Edge::ALL.get(eidx) {
            Some(v) => *v,
            None => return Err(EdCode::CoverBad.val()),
        };
        let bsel = ((rng.next() >> 33) % 3) as usize;
        let band = match bsel {
            0 => Band::Reserved,
            1 => Band::User,
            _ => Band::Outside,
        };
        let span_off = 100 + ((rng.next() >> 33) % 800) as i32;
        let cross_limit = 40u32;
        let cross = rng.signed(cross_limit);
        let travel_limit = 90u32;
        let travel = rng.signed(travel_limit);
        let start = match band {
            Band::Outside => Point {
                x: 300 + (span_off % 400),
                y: 400 + (span_off % 800),
            },
            _ => {
                let lane_c = lane(edge, band)?;
                match edge {
                    Edge::Left | Edge::Right => Point { x: lane_c, y: span_off },
                    Edge::Top | Edge::Bottom => Point { x: span_off, y: lane_c },
                }
            }
        };
        // 外移量受「离屏幕外沿还有多深」限制（车道本身只有 3/12 px 深）。
        let room = match edge {
            Edge::Left => start.x,
            Edge::Right => SCREEN_W - start.x,
            Edge::Top => start.y,
            Edge::Bottom => SCREEN_H - start.y,
        };
        let travel = if travel < 0 {
            travel.max(-(room.max(1)))
        } else {
            travel
        };
        let (dx, dy) = offset_for(edge, travel, cross);
        let end = Point {
            x: start.x.saturating_add(dx),
            y: start.y.saturating_add(dy),
        };
        if !in_screen(start) || !in_screen(end) {
            return Err(EdCode::ZoneBad.val());
        }
        let a = arbitrate(start, end)?;
        if !pair_ok(&a) {
            f.false_user = f.false_user.saturating_add(1);
        }
        // 第二路：引擎全链（Down/Move/Up）。
        let mut t = EdgeTrack::new();
        let mid = Point {
            x: start.x + (end.x - start.x) / 2,
            y: start.y + (end.y - start.y) / 2,
        };
        let _ = t.touch(EdgeEv::Down, start, 1)?;
        let _ = t.touch(EdgeEv::Move, mid, 2)?;
        let step = t.touch(EdgeEv::Up, end, 3)?;
        t.audit()?;
        f.accepted = f.accepted.saturating_add(t.accepted);
        f.rejected = f.rejected.saturating_add(t.rejected);
        if step.verdict != a.verdict || step.edge != a.edge {
            f.engine_mismatch = f.engine_mismatch.saturating_add(1);
        }
        match a.verdict {
            Verdict::User => f.user = f.user.saturating_add(1),
            Verdict::System => f.system = f.system.saturating_add(1),
            Verdict::None => {
                f.none = f.none.saturating_add(1);
                if a.why == Why::ShortTravel {
                    f.short = f.short.saturating_add(1);
                }
            }
        }
        f.attempts = f.attempts.saturating_add(1);
        f.digest = p_core::fold_u32(f.digest, edge.idx() as u32);
        f.digest = p_core::fold_u32(f.digest, band.idx() as u32);
        f.digest = p_core::fold_u32(f.digest, a.verdict.idx() as u32);
        f.digest = p_core::fold_u32(f.digest, a.inward.unsigned_abs());
        i = i.saturating_add(1);
    }
    if !f.clean() || f.digest == 0 {
        return Err(EdCode::FuzzBad.val());
    }
    if f.events() != f.attempts.saturating_mul(FUZZ_EVENTS) {
        return Err(EdCode::LedgerBad.val());
    }
    Ok(f)
}

// ── 渲染 / 解析 ──────────────────────────────────────────────────
pub fn render_edge(s: &EdgeStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.attempts,
        s.user_total(),
        s.system,
        s.none,
        s.short,
        s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 10 {
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
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
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
        v = v.checked_mul(16)?.checked_add(d as u64)?;
        i += 1;
    }
    Some(v)
}

/// 账本回读审计：字段数、三分账平衡、短移量不越界、指纹非零。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; EDGE_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < EDGE_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != EDGE_FIELDS {
        return Err(EdCode::RenderBad.val());
    }
    let attempts = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let user = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let system = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let none = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let short = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    if attempts == 0 {
        return Err(EdCode::RenderBad.val());
    }
    if user.saturating_add(system).saturating_add(none) != attempts {
        return Err(EdCode::LedgerBad.val());
    }
    if short > none {
        return Err(EdCode::GeomBad.val());
    }
    if digest == 0 {
        return Err(EdCode::RenderBad.val());
    }
    Ok(EDGE_FIELDS)
}

// ── 黄金运行 / 复算 ──────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u32, u64), u32> {
    let s = edge_rounds(GOLDEN_ROUNDS)?;
    let text = render_edge(&s);
    if audit_render(&text)? != EDGE_FIELDS {
        return Err(EdCode::RenderBad.val());
    }
    Ok((s.attempts, s.user_total(), s.system, s.none, s.digest))
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EdCode::ReplayBad.val());
    }
    let c1 = conflict_rounds(CONFLICT_ROUNDS)?;
    let c2 = conflict_rounds(CONFLICT_ROUNDS)?;
    if c1.digest != c2.digest
        || c1.system_win != c2.system_win
        || c1.user_win != c2.user_win
        || c1.false_kill != 0
        || c1.jitter != 0
    {
        return Err(EdCode::ReplayBad.val());
    }
    let f1 = fuzz_edges(500)?;
    let f2 = fuzz_edges(500)?;
    if f1.digest != f2.digest || f1.attempts != f2.attempts {
        return Err(EdCode::ReplayBad.val());
    }
    Ok(())
}

// ── 注入（四路） ─────────────────────────────────────────────────
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

/// 四路注入：
///   ① 屏外起手 → 闸 3 必须炸 ZoneBad（不得进入边缘表）
///   ② 坐标窗越界 → 闸 2 必须炸 CoordBad（先于屏内判定）
///   ③ 保留区起手 → 必须判系统（越权判用户即漏）
///   ④ 屏心起手 → 必须判非边缘（误判用户即漏）
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(EdCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; INJECT_ROUTES],
        caught: [0; INJECT_ROUTES],
    };
    let mut i = 0u32;
    while i < rounds {
        let mut t = EdgeTrack::new();
        if t.touch(EdgeEv::Down, Point { x: SCREEN_W + 1, y: 4 }, 1).err()
            == Some(EdCode::ZoneBad.val())
        {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);

        let mut t2 = EdgeTrack::new();
        if t2
            .touch(EdgeEv::Down, Point { x: COORD_MAX + 1, y: 0 }, 1)
            .err()
            == Some(EdCode::CoordBad.val())
        {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);

        let reserved = arbitrate(Point { x: 3, y: 500 }, Point { x: 120, y: 500 });
        if matches!(reserved, Ok(a) if a.verdict == Verdict::System) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);

        let interior = arbitrate(
            Point { x: SCREEN_W / 2, y: SCREEN_H / 2 },
            Point { x: SCREEN_W / 2 + 120, y: SCREEN_H / 2 - 60 },
        );
        if matches!(interior, Ok(a) if a.why == Why::OutsideEdge && a.verdict == Verdict::None) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(EdCode::FuzzBad.val());
    }
    Ok(log)
}

// ── 探针 ─────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if SCREEN_W != 1_080
        || SCREEN_H != 1_920
        || EDGE_BAND != 16
        || RESERVED_PX != 8
        || MIN_DISP != 48
    {
        return Err(EdCode::TableBad.val());
    }
    if audit_zones()? != EDGE_KINDS as u32 {
        return Err(EdCode::TableBad.val());
    }
    if audit_corner_priority()? != CORNER_SAMPLES as u32 {
        return Err(EdCode::TableBad.val());
    }
    if audit_phase_table()? != PHASE_TRANS as u32 {
        return Err(EdCode::TableBad.val());
    }
    if audit_gate_consistency()? != PHASE_TRANS as u32 {
        return Err(EdCode::TableBad.val());
    }
    if audit_labels()? != 29 {
        return Err(EdCode::TableBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let mut v = 0u32;
    while v < VARIANTS {
        let mut e = 0u32;
        while e < EDGE_KINDS as u32 {
            let i = v + e * VARIANTS;
            let c = edge_expect(i)?;
            if c.variant != v || c.edge.idx() as u32 != e {
                return Err(EdCode::CoverBad.val());
            }
            let a = arbitrate(c.start, c.end)?;
            if a.verdict != c.expect || a.why != c.why {
                return Err(EdCode::GeomBad.val());
            }
            if !pair_ok(&a) {
                return Err(EdCode::GeomBad.val());
            }
            if !in_coord_cap(c.start) || !in_coord_cap(c.end) {
                return Err(EdCode::CoordBad.val());
            }
            // 变体 0/1 必须「侧移严格小于内移」（方向一致而非角平分线）。
            if c.variant <= 1 {
                let inward = inward_travel(c.edge, c.start, c.end);
                let cross = cross_travel(c.edge, c.start, c.end).unsigned_abs();
                if inward != a.inward || cross >= inward.unsigned_abs() {
                    return Err(EdCode::GeomBad.val());
                }
            }
            e = e.saturating_add(1);
        }
        v = v.saturating_add(1);
    }
    let s = edge_rounds(GOLDEN_ROUNDS)?;
    if !s.balanced() || s.user_total() == 0 {
        return Err(EdCode::LedgerBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let c = conflict_rounds(60)?;
    if !c.clean() || !c.balanced() {
        return Err(EdCode::SystemBad.val());
    }
    // 保留区优先：同一手势换位移不改判。
    let a1 = arbitrate(Point { x: 3, y: 600 }, Point { x: 90, y: 600 })?;
    let a2 = arbitrate(Point { x: 3, y: 600 }, Point { x: 3, y: 600 })?;
    if a1.verdict != Verdict::System || a2.verdict != Verdict::System {
        return Err(EdCode::SystemBad.val());
    }
    // 用户区：正好 MIN_DISP 算合法，差 1px 不算。
    let u1 = arbitrate(
        Point { x: 12, y: 600 },
        Point { x: 12 + MIN_DISP, y: 599 },
    )?;
    let u2 = arbitrate(
        Point { x: 12, y: 600 },
        Point { x: 12 + MIN_DISP - 1, y: 600 },
    )?;
    if u1.verdict != Verdict::User || u2.verdict != Verdict::None {
        return Err(EdCode::GeomBad.val());
    }
    let g = golden_run()?;
    if g.0 != GOLDEN_ROUNDS || g.1.saturating_add(g.2).saturating_add(g.3) != g.0 {
        return Err(EdCode::LedgerBad.val());
    }
    Ok(())
}

// ── F48989 手势 20 操作集（J1：20 操作全过入库） ──────────────────
/// 操作集规模（卡面 20 操作）。
pub const OP_COUNT: usize = 20;
/// 参数组数（卡面 ×5 组）。
pub const PARAM_GROUPS: usize = 5;

/// 操作族：点按 / 滑动 / 捏合 / 旋转 / 滚动 / 多指。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpFamily {
    Point,
    Swipe,
    Pinch,
    Rotate,
    Scroll,
    Multi,
}

impl OpFamily {
    pub fn idx(self) -> usize {
        match self {
            OpFamily::Point => 0,
            OpFamily::Swipe => 1,
            OpFamily::Pinch => 2,
            OpFamily::Rotate => 3,
            OpFamily::Scroll => 4,
            OpFamily::Multi => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            OpFamily::Point => "point",
            OpFamily::Swipe => "swipe",
            OpFamily::Pinch => "pinch",
            OpFamily::Rotate => "rotate",
            OpFamily::Scroll => "scroll",
            OpFamily::Multi => "multi",
        }
    }

    pub const ALL: [OpFamily; 6] = [
        OpFamily::Point,
        OpFamily::Swipe,
        OpFamily::Pinch,
        OpFamily::Rotate,
        OpFamily::Scroll,
        OpFamily::Multi,
    ];
}

/// 操作期望方向/轴：方向族校验符号，轴族只校验主副轴。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpWant {
    Any,
    DxPos,
    DxNeg,
    DyPos,
    DyNeg,
    AxisX,
    AxisY,
    PinchIn,
    PinchOut,
    Cw,
    Ccw,
}

/// 一条操作登记：指头区间、最小位移、边缘要求、期望方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Operation {
    pub id: u8,
    pub name: &'static str,
    pub family: OpFamily,
    pub fingers_min: u8,
    pub fingers_max: u8,
    pub min_disp: u16,
    pub edge_required: bool,
    pub want: OpWant,
}

/// 20 操作集（id 1..=20，顺序即入库顺序）。
pub const OPS: [Operation; OP_COUNT] = [
    Operation { id: 1, name: "tap", family: OpFamily::Point, fingers_min: 1, fingers_max: 1, min_disp: 0, edge_required: false, want: OpWant::Any },
    Operation { id: 2, name: "double-tap", family: OpFamily::Point, fingers_min: 1, fingers_max: 1, min_disp: 0, edge_required: false, want: OpWant::Any },
    Operation { id: 3, name: "long-press", family: OpFamily::Point, fingers_min: 1, fingers_max: 1, min_disp: 0, edge_required: false, want: OpWant::Any },
    Operation { id: 4, name: "swipe-left", family: OpFamily::Swipe, fingers_min: 1, fingers_max: 1, min_disp: 120, edge_required: false, want: OpWant::DxNeg },
    Operation { id: 5, name: "swipe-right", family: OpFamily::Swipe, fingers_min: 1, fingers_max: 1, min_disp: 120, edge_required: false, want: OpWant::DxPos },
    Operation { id: 6, name: "swipe-up", family: OpFamily::Swipe, fingers_min: 1, fingers_max: 1, min_disp: 120, edge_required: false, want: OpWant::DyNeg },
    Operation { id: 7, name: "swipe-down", family: OpFamily::Swipe, fingers_min: 1, fingers_max: 1, min_disp: 120, edge_required: false, want: OpWant::DyPos },
    Operation { id: 8, name: "pinch-in", family: OpFamily::Pinch, fingers_min: 2, fingers_max: 2, min_disp: 150, edge_required: false, want: OpWant::PinchIn },
    Operation { id: 9, name: "pinch-out", family: OpFamily::Pinch, fingers_min: 2, fingers_max: 2, min_disp: 150, edge_required: false, want: OpWant::PinchOut },
    Operation { id: 10, name: "rotate-cw", family: OpFamily::Rotate, fingers_min: 2, fingers_max: 2, min_disp: 200, edge_required: false, want: OpWant::Cw },
    Operation { id: 11, name: "rotate-ccw", family: OpFamily::Rotate, fingers_min: 2, fingers_max: 2, min_disp: 200, edge_required: false, want: OpWant::Ccw },
    Operation { id: 12, name: "scroll-h", family: OpFamily::Scroll, fingers_min: 2, fingers_max: 2, min_disp: 120, edge_required: false, want: OpWant::AxisX },
    Operation { id: 13, name: "scroll-v", family: OpFamily::Scroll, fingers_min: 2, fingers_max: 2, min_disp: 120, edge_required: false, want: OpWant::AxisY },
    Operation { id: 14, name: "two-finger-tap", family: OpFamily::Point, fingers_min: 2, fingers_max: 2, min_disp: 0, edge_required: false, want: OpWant::Any },
    Operation { id: 15, name: "three-finger-swipe-left", family: OpFamily::Swipe, fingers_min: 3, fingers_max: 3, min_disp: 150, edge_required: false, want: OpWant::DxNeg },
    Operation { id: 16, name: "three-finger-swipe-right", family: OpFamily::Swipe, fingers_min: 3, fingers_max: 3, min_disp: 150, edge_required: false, want: OpWant::DxPos },
    Operation { id: 17, name: "three-finger-swipe-up", family: OpFamily::Swipe, fingers_min: 3, fingers_max: 3, min_disp: 150, edge_required: false, want: OpWant::DyNeg },
    Operation { id: 18, name: "four-finger-tap", family: OpFamily::Multi, fingers_min: 4, fingers_max: 4, min_disp: 0, edge_required: false, want: OpWant::Any },
    Operation { id: 19, name: "edge-swipe-left", family: OpFamily::Swipe, fingers_min: 1, fingers_max: 2, min_disp: 48, edge_required: true, want: OpWant::DxPos },
    Operation { id: 20, name: "edge-swipe-right", family: OpFamily::Swipe, fingers_min: 1, fingers_max: 2, min_disp: 48, edge_required: true, want: OpWant::DxNeg },
];

/// 操作输入（单拍归一化后的观测量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpInput {
    pub fingers: u8,
    pub dx: i32,
    pub dy: i32,
    pub spread: i32,
    pub angle_mdeg: i32,
    pub from_edge: bool,
}

/// 操作输出：id + 族 + 归一化幅度（0..=1000）+ 有符号方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpOut {
    pub id: u8,
    pub family: OpFamily,
    pub magnitude: u32,
    pub dir: i8,
}

impl OpOut {
    pub fn normalized(&self) -> bool {
        self.magnitude <= 1000 && self.dir >= -1 && self.dir <= 1
    }
}

fn clamp_mag(v: u64) -> u32 {
    if v > 1000 {
        1000
    } else {
        v as u32
    }
}

fn sign_i32(v: i32) -> i8 {
    if v > 0 {
        1
    } else if v < 0 {
        -1
    } else {
        0
    }
}

/// 主副轴拆分：两轴都没过最小位移 → GeomBad；等长 → DirBad。
fn split_axes(ax: u64, ay: u64, min: u64) -> Result<(u64, u64, bool), u32> {
    if ax < min && ay < min {
        return Err(EdCode::GeomBad.val());
    }
    if ax == ay {
        return Err(EdCode::DirBad.val());
    }
    if ax > ay {
        Ok((ax, ay, true))
    } else {
        Ok((ay, ax, false))
    }
}

/// 主轴至少 1.5 倍次轴，否则不构成有向操作。
fn dominance_ok(major: u64, minor: u64) -> bool {
    major.saturating_mul(2) >= minor.saturating_mul(3)
}

/// 跑一条操作：表自洽 → 指头数 → 坐标窗 → 边缘要求 → 族内几何/方向。
pub fn run_operation(op: &Operation, input: &OpInput) -> Result<OpOut, u32> {
    if op.fingers_min == 0 || op.fingers_min > op.fingers_max || op.fingers_max > 5 {
        return Err(EdCode::TableBad.val());
    }
    if input.fingers < op.fingers_min || input.fingers > op.fingers_max {
        return Err(EdCode::GeomBad.val());
    }
    if (input.dx as i64).unsigned_abs() > COORD_MAX as u64
        || (input.dy as i64).unsigned_abs() > COORD_MAX as u64
        || (input.spread as i64).unsigned_abs() > COORD_MAX as u64
        || (input.angle_mdeg as i64).unsigned_abs() > COORD_MAX as u64
    {
        return Err(EdCode::CoordBad.val());
    }
    if op.edge_required && !input.from_edge {
        return Err(EdCode::DirBad.val());
    }
    let ax = (input.dx as i64).unsigned_abs();
    let ay = (input.dy as i64).unsigned_abs();
    let min = op.min_disp as u64;
    match op.family {
        OpFamily::Point | OpFamily::Multi => Ok(OpOut {
            id: op.id,
            family: op.family,
            magnitude: 0,
            dir: 0,
        }),
        OpFamily::Swipe => {
            let (major, minor, horiz) = split_axes(ax, ay, min)?;
            if !dominance_ok(major, minor) {
                return Err(EdCode::DirBad.val());
            }
            let dir = match op.want {
                OpWant::DxPos => {
                    if !horiz || input.dx <= 0 {
                        return Err(EdCode::DirBad.val());
                    }
                    1
                }
                OpWant::DxNeg => {
                    if !horiz || input.dx >= 0 {
                        return Err(EdCode::DirBad.val());
                    }
                    -1
                }
                OpWant::DyPos => {
                    if horiz || input.dy <= 0 {
                        return Err(EdCode::DirBad.val());
                    }
                    1
                }
                OpWant::DyNeg => {
                    if horiz || input.dy >= 0 {
                        return Err(EdCode::DirBad.val());
                    }
                    -1
                }
                _ => return Err(EdCode::TableBad.val()),
            };
            Ok(OpOut {
                id: op.id,
                family: op.family,
                magnitude: clamp_mag(major),
                dir,
            })
        }
        OpFamily::Scroll => {
            let (major, minor, horiz) = split_axes(ax, ay, min)?;
            if !dominance_ok(major, minor) {
                return Err(EdCode::DirBad.val());
            }
            let dir = match op.want {
                OpWant::AxisX => {
                    if !horiz {
                        return Err(EdCode::DirBad.val());
                    }
                    sign_i32(input.dx)
                }
                OpWant::AxisY => {
                    if horiz {
                        return Err(EdCode::DirBad.val());
                    }
                    sign_i32(input.dy)
                }
                _ => return Err(EdCode::TableBad.val()),
            };
            Ok(OpOut {
                id: op.id,
                family: op.family,
                magnitude: clamp_mag(major),
                dir,
            })
        }
        OpFamily::Pinch => {
            let s = (input.spread as i64).unsigned_abs();
            if s < min {
                return Err(EdCode::GeomBad.val());
            }
            match op.want {
                OpWant::PinchIn => {
                    if input.spread >= 0 {
                        return Err(EdCode::DirBad.val());
                    }
                    Ok(OpOut {
                        id: op.id,
                        family: op.family,
                        magnitude: clamp_mag(s),
                        dir: -1,
                    })
                }
                OpWant::PinchOut => {
                    if input.spread <= 0 {
                        return Err(EdCode::DirBad.val());
                    }
                    Ok(OpOut {
                        id: op.id,
                        family: op.family,
                        magnitude: clamp_mag(s),
                        dir: 1,
                    })
                }
                _ => Err(EdCode::TableBad.val()),
            }
        }
        OpFamily::Rotate => {
            let a = (input.angle_mdeg as i64).unsigned_abs();
            if a < min {
                return Err(EdCode::GeomBad.val());
            }
            match op.want {
                OpWant::Cw => {
                    if input.angle_mdeg <= 0 {
                        return Err(EdCode::DirBad.val());
                    }
                    Ok(OpOut {
                        id: op.id,
                        family: op.family,
                        magnitude: clamp_mag(a),
                        dir: 1,
                    })
                }
                OpWant::Ccw => {
                    if input.angle_mdeg >= 0 {
                        return Err(EdCode::DirBad.val());
                    }
                    Ok(OpOut {
                        id: op.id,
                        family: op.family,
                        magnitude: clamp_mag(a),
                        dir: -1,
                    })
                }
                _ => Err(EdCode::TableBad.val()),
            }
        }
    }
}

/// 为一条操作构造确定性合法输入（供入库复跑与测试共用）。
pub fn sample_input(op: &Operation) -> OpInput {
    let fingers = op.fingers_min;
    let d = if op.min_disp == 0 {
        10
    } else {
        op.min_disp as i32
    };
    let (dx, dy, spread, angle) = match op.family {
        OpFamily::Point | OpFamily::Multi => (2, 1, 0, 0),
        OpFamily::Swipe => match op.want {
            OpWant::DxPos => (d, 1, 0, 0),
            OpWant::DxNeg => (-d, 1, 0, 0),
            OpWant::DyPos => (1, d, 0, 0),
            _ => (1, -d, 0, 0),
        },
        OpFamily::Scroll => match op.want {
            OpWant::AxisY => (1, d, 0, 0),
            _ => (d, 1, 0, 0),
        },
        OpFamily::Pinch => match op.want {
            OpWant::PinchIn => (0, 0, -d, 0),
            _ => (0, 0, d, 0),
        },
        OpFamily::Rotate => match op.want {
            OpWant::Cw => (0, 0, 0, d),
            _ => (0, 0, 0, -d),
        },
    };
    OpInput {
        fingers,
        dx,
        dy,
        spread,
        angle_mdeg: angle,
        from_edge: op.edge_required,
    }
}

/// 操作表审计：id 连续、名字唯一、指头区间合法、族覆盖与边缘操作数吻合。
pub fn audit_operations() -> Result<u32, u32> {
    if OP_COUNT != 20 {
        return Err(EdCode::TableBad.val());
    }
    let mut families = [0u32; 6];
    let mut edge_ops = 0u32;
    let mut i = 0usize;
    while i < OP_COUNT {
        let op = match OPS.get(i) {
            Some(v) => *v,
            None => return Err(EdCode::TableBad.val()),
        };
        if op.id as usize != i.saturating_add(1) || op.name.is_empty() {
            return Err(EdCode::TableBad.val());
        }
        if op.fingers_min == 0 || op.fingers_min > op.fingers_max || op.fingers_max > 5 {
            return Err(EdCode::TableBad.val());
        }
        if op.min_disp > 1_000 {
            return Err(EdCode::TableBad.val());
        }
        let mut j = i.saturating_add(1);
        while j < OP_COUNT {
            if let Some(o) = OPS.get(j) {
                if o.id == op.id || o.name == op.name {
                    return Err(EdCode::TableBad.val());
                }
            }
            j = j.saturating_add(1);
        }
        if op.edge_required {
            edge_ops = edge_ops.saturating_add(1);
            if op.family != OpFamily::Swipe {
                return Err(EdCode::TableBad.val());
            }
        }
        if let Some(c) = families.get_mut(op.family.idx()) {
            *c = c.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    if edge_ops != 2 {
        return Err(EdCode::TableBad.val());
    }
    if families != [4, 9, 2, 2, 2, 1] {
        return Err(EdCode::TableBad.val());
    }
    Ok(OP_COUNT as u32)
}

/// 操作集入库探针：表审计 + 20 条合法输入全跑通 + 逐族反例被拒。
pub fn operation_set_probe() -> Result<u32, u32> {
    let n = audit_operations()?;
    let mut i = 0usize;
    while i < OP_COUNT {
        let op = match OPS.get(i) {
            Some(v) => *v,
            None => return Err(EdCode::TableBad.val()),
        };
        let input = sample_input(&op);
        let out = run_operation(&op, &input)?;
        if out.id != op.id || out.family != op.family || !out.normalized() {
            return Err(EdCode::FuzzBad.val());
        }
        let mut rev = input;
        let exp;
        match op.family {
            OpFamily::Point | OpFamily::Multi => {
                rev.fingers = 0;
                exp = EdCode::GeomBad.val();
            }
            OpFamily::Swipe => {
                rev.dx = -rev.dx;
                rev.dy = -rev.dy;
                exp = EdCode::DirBad.val();
            }
            OpFamily::Pinch => {
                rev.spread = -rev.spread;
                exp = EdCode::DirBad.val();
            }
            OpFamily::Rotate => {
                rev.angle_mdeg = -rev.angle_mdeg;
                exp = EdCode::DirBad.val();
            }
            OpFamily::Scroll => {
                let d = if op.min_disp == 0 {
                    1
                } else {
                    op.min_disp as i32
                };
                match op.want {
                    OpWant::AxisY => {
                        rev.dx = d;
                        rev.dy = 1;
                    }
                    _ => {
                        rev.dx = 1;
                        rev.dy = d;
                    }
                }
                exp = EdCode::DirBad.val();
            }
        }
        if run_operation(&op, &rev) != Err(exp) {
            return Err(EdCode::FuzzBad.val());
        }
        if op.edge_required {
            let mut off = input;
            off.from_edge = false;
            if run_operation(&op, &off) != Err(EdCode::DirBad.val()) {
                return Err(EdCode::FuzzBad.val());
            }
        }
        i = i.saturating_add(1);
    }
    Ok(n)
}

/// 操作集批量账（20 操作 + 20 负判据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpStats {
    pub total: u32,
    pub passed: u32,
    pub rejected: u32,
    pub digest: u64,
}

impl OpStats {
    pub fn new() -> OpStats {
        OpStats {
            total: 0,
            passed: 0,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn all_passed(&self) -> bool {
        self.total as usize == OP_COUNT
            && self.passed as usize == OP_COUNT
            && self.rejected as usize == OP_COUNT
            && self.digest != 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.all_passed() {
            return Err(EdCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 20 操作入库：逐条合法输入必须过，逐条「少指头」必须 GeomBad。
pub fn operation_rounds() -> Result<OpStats, u32> {
    audit_operations()?;
    let mut s = OpStats::new();
    let mut i = 0usize;
    while i < OP_COUNT {
        let op = match OPS.get(i) {
            Some(v) => *v,
            None => return Err(EdCode::TableBad.val()),
        };
        let input = sample_input(&op);
        let out = run_operation(&op, &input)?;
        if out.id != op.id || out.family != op.family || !out.normalized() {
            return Err(EdCode::FuzzBad.val());
        }
        s.passed = s.passed.saturating_add(1);
        s.digest = p_core::fold_u32(s.digest, out.id as u32);
        s.digest = p_core::fold_u32(s.digest, out.magnitude);
        s.digest = p_core::fold_u32(s.digest, out.dir as u32);
        // 负判据：指头数清零必须先报 GeomBad（不是静默吞掉）。
        let mut bad = input;
        bad.fingers = 0;
        if run_operation(&op, &bad) != Err(EdCode::GeomBad.val()) {
            return Err(EdCode::FuzzBad.val());
        }
        s.rejected = s.rejected.saturating_add(1);
        s.total = s.total.saturating_add(1);
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

// ── F48990 手势参数配置面（5 组阈值/速度/时长/距离/上限） ────────
/// 参数类型：五组闭区间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamKind {
    Threshold,
    Speed,
    Timing,
    Distance,
    Limit,
}

impl ParamKind {
    pub fn idx(self) -> usize {
        match self {
            ParamKind::Threshold => 0,
            ParamKind::Speed => 1,
            ParamKind::Timing => 2,
            ParamKind::Distance => 3,
            ParamKind::Limit => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ParamKind::Threshold => "threshold",
            ParamKind::Speed => "speed",
            ParamKind::Timing => "timing",
            ParamKind::Distance => "distance",
            ParamKind::Limit => "limit",
        }
    }

    pub fn from_name(s: &str) -> Option<ParamKind> {
        let mut i = 0usize;
        while i < ParamKind::ALL.len() {
            if let Some(k) = ParamKind::ALL.get(i) {
                if k.name() == s {
                    return Some(*k);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    pub const ALL: [ParamKind; PARAM_GROUPS] = [
        ParamKind::Threshold,
        ParamKind::Speed,
        ParamKind::Timing,
        ParamKind::Distance,
        ParamKind::Limit,
    ];
}

/// 一组参数：闭区间 [lo, hi] + 当前值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParamGroup {
    pub kind: ParamKind,
    pub lo: u32,
    pub hi: u32,
    pub value: u32,
}

impl ParamGroup {
    pub fn valid(&self) -> bool {
        self.lo < self.hi && self.value >= self.lo && self.value <= self.hi
    }
}

/// 参数面：恰好五组，顺序固定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParamConfig {
    pub groups: [ParamGroup; PARAM_GROUPS],
}

impl ParamConfig {
    pub fn default_config() -> ParamConfig {
        ParamConfig {
            groups: [
                ParamGroup { kind: ParamKind::Threshold, lo: 1, hi: 10_000, value: 240 },
                ParamGroup { kind: ParamKind::Speed, lo: 50, hi: 20_000, value: 800 },
                ParamGroup { kind: ParamKind::Timing, lo: 10, hi: 60_000, value: 300 },
                ParamGroup { kind: ParamKind::Distance, lo: 8, hi: 4_096, value: 48 },
                ParamGroup { kind: ParamKind::Limit, lo: 1, hi: 10, value: 5 },
            ],
        }
    }

    /// 校验：五组齐全且不重、区间非空、值在区间内、各组上界不越语义帽。
    pub fn validate(&self) -> Result<u32, u32> {
        let mut seen = [false; PARAM_GROUPS];
        let mut i = 0usize;
        while i < PARAM_GROUPS {
            let g = match self.groups.get(i) {
                Some(v) => *v,
                None => return Err(EdCode::TableBad.val()),
            };
            if g.kind.idx() >= PARAM_GROUPS {
                return Err(EdCode::TableBad.val());
            }
            match seen.get_mut(g.kind.idx()) {
                Some(slot) => {
                    if *slot {
                        return Err(EdCode::BandBad.val());
                    }
                    *slot = true;
                }
                None => return Err(EdCode::TableBad.val()),
            }
            if !g.valid() {
                return Err(EdCode::BandBad.val());
            }
            let cap = match g.kind {
                ParamKind::Threshold => 10_000,
                ParamKind::Speed => 20_000,
                ParamKind::Timing => 60_000,
                ParamKind::Distance => 4_096,
                ParamKind::Limit => 10,
            };
            if g.hi > cap {
                return Err(EdCode::BandBad.val());
            }
            i = i.saturating_add(1);
        }
        Ok(PARAM_GROUPS as u32)
    }
}

/// 规范序列化：`kind=value@lo..hi|...`（顺序固定）。
pub fn serialize_params(c: &ParamConfig) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < PARAM_GROUPS {
        if let Some(g) = c.groups.get(i) {
            if i > 0 {
                s.push('|');
            }
            s.push_str(&format!(
                "{}={}@{}..{}",
                g.kind.name(),
                g.value,
                g.lo,
                g.hi
            ));
        }
        i = i.saturating_add(1);
    }
    s
}

/// 严格反序列化：字段数、种类顺序、数值格式、区间与值都必须合法。
pub fn parse_params(text: &str) -> Option<ParamConfig> {
    let mut groups = [ParamGroup {
        kind: ParamKind::Threshold,
        lo: 1,
        hi: 2,
        value: 1,
    }; PARAM_GROUPS];
    let mut n = 0usize;
    for part in text.split('|') {
        if n >= PARAM_GROUPS {
            return None;
        }
        let mut it = part.split('=');
        let name = it.next()?;
        let rest = it.next()?;
        if it.next().is_some() {
            return None;
        }
        let mut it2 = rest.split('@');
        let val = it2.next()?;
        let range = it2.next()?;
        if it2.next().is_some() {
            return None;
        }
        let mut it3 = range.split("..");
        let lo = it3.next()?;
        let hi = it3.next()?;
        if it3.next().is_some() {
            return None;
        }
        let kind = ParamKind::from_name(name)?;
        if kind.idx() != n {
            return None;
        }
        let g = ParamGroup {
            kind,
            lo: parse_u32(lo)?,
            hi: parse_u32(hi)?,
            value: parse_u32(val)?,
        };
        if !g.valid() {
            return None;
        }
        if let Some(cell) = groups.get_mut(n) {
            *cell = g;
        }
        n = n.saturating_add(1);
    }
    if n != PARAM_GROUPS {
        return None;
    }
    let c = ParamConfig { groups };
    if c.validate().is_err() {
        return None;
    }
    Some(c)
}

/// 5 组一致性：各组上下界擦边值必须被拒，规范序列化必须往返逐位相等。
pub fn param_consistency() -> Result<u32, u32> {
    let c = ParamConfig::default_config();
    if c.validate()? != PARAM_GROUPS as u32 {
        return Err(EdCode::BandBad.val());
    }
    match parse_params(&serialize_params(&c)) {
        Some(d) => {
            if d != c {
                return Err(EdCode::ReplayBad.val());
            }
        }
        None => return Err(EdCode::RenderBad.val()),
    }
    let mut i = 0usize;
    while i < PARAM_GROUPS {
        let g = match c.groups.get(i) {
            Some(v) => *v,
            None => return Err(EdCode::TableBad.val()),
        };
        // 下界减一：必须 BandBad。
        if g.lo > 0 {
            let mut low = c;
            if let Some(cell) = low.groups.get_mut(i) {
                *cell = ParamGroup {
                    value: g.lo.saturating_sub(1),
                    ..g
                };
            }
            if low.validate() != Err(EdCode::BandBad.val()) {
                return Err(EdCode::FuzzBad.val());
            }
        }
        // 上界加一：必须 BandBad（越值或越语义帽）。
        let mut high = c;
        if let Some(cell) = high.groups.get_mut(i) {
            *cell = ParamGroup {
                value: g.hi.saturating_add(1),
                ..g
            };
        }
        if high.validate() != Err(EdCode::BandBad.val()) {
            return Err(EdCode::FuzzBad.val());
        }
        // 组内换值后规范序列化必须往返一致。
        let mut tweak = c;
        if let Some(cell) = tweak.groups.get_mut(i) {
            *cell = ParamGroup { value: g.lo, ..g };
        }
        if parse_params(&serialize_params(&tweak)) != Some(tweak) {
            return Err(EdCode::ReplayBad.val());
        }
        i = i.saturating_add(1);
    }
    // 负判据：坏文本一律 None。
    if parse_params("").is_some()
        || parse_params("threshold=240@1..10000").is_some()
        || parse_params("speed=800@50..20000|threshold=240@1..10000|timing=300@10..60000|distance=48@8..4096|limit=5@1..10").is_some()
        || parse_params("threshold=99999@1..10000|speed=800@50..20000|timing=300@10..60000|distance=48@8..4096|limit=5@1..10").is_some()
    {
        return Err(EdCode::FuzzBad.val());
    }
    Ok(PARAM_GROUPS as u32)
}

// ── 域检查项（动态明细槽上限 8） ──────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let ops = operation_rounds();
    out.push((
        "F48989 20操作全过入库",
        matches!(&ops, Ok(s) if s.all_passed()),
        match &ops {
            Ok(s) => format!("ok={} rej={}", s.passed, s.rejected),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let set = operation_set_probe();
    out.push((
        "F48989 操作集唯一与反例",
        matches!(&set, Ok(n) if *n as usize == OP_COUNT),
        match &set {
            Ok(n) => format!("set={} uniq/neg", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let params = param_consistency();
    out.push((
        "F48990 参数5组校验一致",
        matches!(&params, Ok(n) if *n as usize == PARAM_GROUPS),
        match &params {
            Ok(n) => format!("groups={} rt/edge", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let zones = audit_zones();
    let corners = audit_corner_priority();
    let phases = audit_phase_table();
    out.push((
        "F48989 常量与边缘区间锚定",
        SCREEN_W == 1_080
            && SCREEN_H == 1_920
            && EDGE_BAND == 16
            && RESERVED_PX == 8
            && MIN_DISP == 48
            && zones == Ok(EDGE_KINDS as u32)
            && corners == Ok(CORNER_SAMPLES as u32)
            && phases == Ok(PHASE_TRANS as u32),
        format!(
            "{}x{} b={} r={} m={}",
            SCREEN_W, SCREEN_H, EDGE_BAND, RESERVED_PX, MIN_DISP
        ),
    ));

    let rounds = edge_rounds(EDGE_ROUNDS);
    // 负判据：非边缘起手 + 反向/差 1px 零误判（与分类账一起折叠进本项）。
    let mut interior = 0u32;
    let mut false_user = 0u32;
    let mut i = 0u32;
    while i < EDGE_ROUNDS {
        match edge_expect(i) {
            Ok(c) => {
                if c.variant == 4 || c.variant == 5 || c.variant == 6 {
                    interior = interior.saturating_add(1);
                    match arbitrate(c.start, c.end) {
                        Ok(a) => {
                            if a.verdict != Verdict::None || a.why != c.why {
                                false_user = false_user.saturating_add(1);
                            }
                        }
                        Err(_) => false_user = false_user.saturating_add(1),
                    }
                }
            }
            Err(_) => false_user = false_user.saturating_add(1),
        }
        i = i.saturating_add(1);
    }
    out.push((
        "F48989 分类×10^3与非边缘零误判",
        matches!(&rounds, Ok(s) if s.balanced()
            && s.attempts == EDGE_ROUNDS
            && s.left > 0 && s.right > 0 && s.top > 0 && s.bottom > 0)
            && interior > 0
            && false_user == 0,
        match &rounds {
            Ok(s) => format!(
                "n={} u={} s={} x={} f={}",
                s.attempts, s.user_total(), s.system, s.none, false_user
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz = fuzz_edges(FUZZ_ROUNDS);
    out.push((
        "F48989 fuzz×10^4零错配",
        matches!(&fuzz, Ok(f) if f.clean() && f.attempts >= FUZZ_ROUNDS),
        match &fuzz {
            Ok(f) => format!("n={} mis={}", f.attempts, f.engine_mismatch),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 保留区优先：变体 2/3 必须全判系统。
    let mut reserved = 0u32;
    let mut reserved_system = 0u32;
    let mut j = 0u32;
    while j < EDGE_ROUNDS {
        match conflict_expect(j) {
            Ok(c) => {
                if c.band == Band::Reserved {
                    reserved = reserved.saturating_add(1);
                    match arbitrate(c.start, c.end) {
                        Ok(a) => {
                            if a.verdict == Verdict::System {
                                reserved_system = reserved_system.saturating_add(1);
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            Err(_) => {}
        }
        j = j.saturating_add(1);
    }
    let conflict = conflict_rounds(CONFLICT_ROUNDS);
    out.push((
        "F48990 保留区优先与零误杀",
        reserved > 0
            && reserved_system == reserved
            && matches!(&conflict, Ok(s) if s.user_win > 0 && s.false_kill == 0 && s.balanced()),
        match &conflict {
            Ok(s) => format!(
                "res={} kill={} u={}",
                reserved, s.false_kill, s.user_win
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let again = conflict_rounds(CONFLICT_ROUNDS);
    let stable = match (&conflict, &again) {
        (Ok(a), Ok(b)) => a.digest == b.digest && a.jitter == 0 && b.jitter == 0,
        _ => false,
    };
    let golden = golden_run();
    let render_ok = matches!(&golden, Ok(_))
        && edge_rounds(GOLDEN_ROUNDS)
            .map(|s| audit_render(&render_edge(&s)) == Ok(EDGE_FIELDS))
            .unwrap_or(false);
    out.push((
        "F48990 冲突零抖动与账本",
        stable && segment_guard() && codes_are_distinct() && render_ok,
        match (&conflict, &golden) {
            (Ok(a), Ok(_)) => format!("jit={} txt={}", a.jitter, EDGE_FIELDS),
            (Err(e), _) => format!("err=0x{:X}", e),
            (_, Err(e)) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48989_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48989 边缘手势识别与 F48990 冲突裁决");
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

// ── 单元判据 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71740);
        assert_eq!(SCREEN_W, 1_080);
        assert_eq!(SCREEN_H, 1_920);
        assert_eq!(EDGE_BAND, 16);
        assert_eq!(RESERVED_PX, 8);
        assert_eq!(MIN_DISP, 48);
        assert_eq!(EDGE_KINDS, 4);
        assert_eq!(DIR_KINDS, 4);
        assert_eq!(PHASE_TRANS, 16);
        assert_eq!(VARIANTS, 8);
        assert_eq!(CONFLICT_VARIANTS, 6);
        assert_eq!(EDGE_ROUNDS, 1_000);
        assert_eq!(CONFLICT_ROUNDS, 1_000);
        assert_eq!(FUZZ_ROUNDS, 10_000);
        assert_eq!(EDGE_FIELDS, 6);
        assert_eq!(EdCode::ALL.len(), 14);
    }

    #[test]
    fn zone_table_is_total_and_unique() {
        assert_eq!(audit_zones(), Ok(EDGE_KINDS as u32));
        assert_eq!(audit_labels(), Ok(29));
        assert_eq!(audit_phase_table(), Ok(PHASE_TRANS as u32));
        assert_eq!(audit_gate_consistency(), Ok(PHASE_TRANS as u32));
    }

    #[test]
    fn zone_intervals_are_literal() {
        let l = match zone_of(Edge::Left) {
            Some(z) => z,
            None => return,
        };
        assert_eq!((l.band_lo, l.band_hi), (0, 16));
        assert_eq!((l.res_lo, l.res_hi), (0, 7));
        assert_eq!((l.user_lo, l.user_hi), (8, 16));
        let r = match zone_of(Edge::Right) {
            Some(z) => z,
            None => return,
        };
        assert_eq!((r.band_lo, r.band_hi), (1_064, 1_080));
        assert_eq!((r.res_lo, r.res_hi), (1_073, 1_080));
        assert_eq!((r.user_lo, r.user_hi), (1_064, 1_072));
        let t = match zone_of(Edge::Top) {
            Some(z) => z,
            None => return,
        };
        assert_eq!((t.band_lo, t.band_hi), (0, 16));
        let b = match zone_of(Edge::Bottom) {
            Some(z) => z,
            None => return,
        };
        assert_eq!((b.band_lo, b.band_hi), (1_904, 1_920));
        assert_eq!((b.res_lo, b.res_hi), (1_913, 1_920));
        assert_eq!(l.inward, Dir::Ltr);
        assert_eq!(r.inward, Dir::Rtl);
        assert_eq!(t.inward, Dir::Ttb);
        assert_eq!(b.inward, Dir::Btt);
    }

    #[test]
    fn corner_priority_is_unique() {
        assert_eq!(audit_corner_priority(), Ok(CORNER_SAMPLES as u32));
        assert_eq!(edge_hit(Point { x: 0, y: 0 }), Some(Edge::Left));
        assert_eq!(
            edge_hit(Point { x: SCREEN_W, y: 0 }),
            Some(Edge::Right)
        );
        assert_eq!(
            edge_hit(Point { x: 540, y: 0 }),
            Some(Edge::Top)
        );
        assert_eq!(
            edge_hit(Point { x: 540, y: SCREEN_H }),
            Some(Edge::Bottom)
        );
        assert_eq!(edge_hit(Point { x: 540, y: 960 }), None);
    }

    #[test]
    fn band_membership_is_exact() {
        let z = match zone_of(Edge::Left) {
            Some(v) => v,
            None => return,
        };
        assert_eq!(band_member(Point { x: 0, y: 500 }, &z), Band::Reserved);
        assert_eq!(band_member(Point { x: 7, y: 500 }, &z), Band::Reserved);
        assert_eq!(band_member(Point { x: 8, y: 500 }, &z), Band::User);
        assert_eq!(band_member(Point { x: 16, y: 500 }, &z), Band::User);
        assert_eq!(band_member(Point { x: 17, y: 500 }, &z), Band::Outside);
        assert_eq!(band_member(Point { x: 3, y: -1 }, &z), Band::Outside);
        assert_eq!(lane(Edge::Left, Band::User), Ok(12));
        assert_eq!(lane(Edge::Left, Band::Reserved), Ok(3));
        assert_eq!(lane(Edge::Right, Band::Reserved), Ok(1_076));
        assert_eq!(lane(Edge::Bottom, Band::User), Ok(1_908));
        assert!(lane(Edge::Left, Band::Outside).is_err());
    }

    #[test]
    fn dir_classification_boundaries() {
        assert_eq!(classify_dir(0, 0), None);
        assert_eq!(classify_dir(5, 0), Some(Dir::Ltr));
        assert_eq!(classify_dir(-5, 0), Some(Dir::Rtl));
        assert_eq!(classify_dir(0, 5), Some(Dir::Ttb));
        assert_eq!(classify_dir(0, -5), Some(Dir::Btt));
        // 等长时 X 轴优先（定则，不是巧合）。
        assert_eq!(classify_dir(7, 7), Some(Dir::Ltr));
        assert_eq!(classify_dir(-7, 7), Some(Dir::Rtl));
        assert_eq!(classify_dir(6, 7), Some(Dir::Ttb));
    }

    #[test]
    fn travel_helpers_agree_with_zones() {
        let a = Point { x: 12, y: 600 };
        let b = Point { x: 60, y: 650 };
        assert_eq!(inward_travel(Edge::Left, a, b), 48);
        assert_eq!(cross_travel(Edge::Left, a, b), 50);
        assert_eq!(inward_travel(Edge::Right, a, b), -48);
        let (dx, dy) = offset_for(Edge::Left, 48, 50);
        assert_eq!((dx, dy), (48, 50));
        let (dx2, dy2) = offset_for(Edge::Right, 48, 50);
        assert_eq!((dx2, dy2), (-48, 50));
        let (dx3, dy3) = offset_for(Edge::Bottom, 48, 50);
        assert_eq!((dx3, dy3), (50, -48));
    }

    #[test]
    fn gate_clock_precedes_coordinate() {
        let mut t = EdgeTrack::new();
        let _ = t.touch(EdgeEv::Down, Point { x: 12, y: 600 }, 10);
        assert_eq!(
            t.touch(EdgeEv::Move, Point { x: COORD_MAX + 1, y: 0 }, 9),
            Err(EdCode::TickBad.val())
        );
        assert_eq!(t.rejected, 1);
    }

    #[test]
    fn gate_coordinate_precedes_screen() {
        let mut t = EdgeTrack::new();
        assert_eq!(
            t.touch(EdgeEv::Down, Point { x: COORD_MAX + 1, y: 0 }, 1),
            Err(EdCode::CoordBad.val())
        );
        assert_eq!(
            t.touch(EdgeEv::Down, Point { x: -1, y: 0 }, 1),
            Err(EdCode::ZoneBad.val())
        );
        assert_eq!(
            t.touch(EdgeEv::Down, Point { x: 0, y: SCREEN_H + 1 }, 1),
            Err(EdCode::ZoneBad.val())
        );
    }

    #[test]
    fn gate_event_legality_is_enforced() {
        let mut t = EdgeTrack::new();
        // Move-before-Down：表里有转移，但闸 4 必须先拒。
        assert_eq!(
            t.touch(EdgeEv::Move, Point { x: 12, y: 600 }, 1),
            Err(EdCode::EventBad.val())
        );
        assert_eq!(
            t.touch(EdgeEv::Cancel, Point { x: 12, y: 600 }, 2),
            Err(EdCode::EventBad.val())
        );
    }

    #[test]
    fn double_down_is_rejected() {
        let mut t = EdgeTrack::new();
        let _ = t.touch(EdgeEv::Down, Point { x: 12, y: 600 }, 1);
        assert_eq!(
            t.touch(EdgeEv::Down, Point { x: 12, y: 600 }, 2),
            Err(EdCode::EventBad.val())
        );
    }

    #[test]
    fn engine_user_gesture_is_classified() {
        let mut t = EdgeTrack::new();
        let _ = t.touch(EdgeEv::Down, Point { x: 12, y: 600 }, 1);
        let _ = t.touch(EdgeEv::Move, Point { x: 40, y: 610 }, 2);
        let step = match t.touch(EdgeEv::Up, Point { x: 100, y: 620 }, 3) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(step.verdict, Verdict::User);
        assert_eq!(step.edge, Some(Edge::Left));
        assert_eq!(step.phase, Phase::Settled);
        assert_eq!(step.act, Act::Emit);
        assert_eq!(t.moves, 1);
        assert_eq!(t.max_inward, 88);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn engine_reserved_gesture_is_system() {
        let mut t = EdgeTrack::new();
        let _ = t.touch(EdgeEv::Down, Point { x: 3, y: 600 }, 1);
        let step = match t.touch(EdgeEv::Up, Point { x: 3, y: 600 }, 2) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(step.verdict, Verdict::System);
        assert_eq!(t.why, Why::ReservedBand);
        assert_eq!(t.system_emitted, 1);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn engine_interior_gesture_is_none() {
        let mut t = EdgeTrack::new();
        let _ = t.touch(EdgeEv::Down, Point { x: 540, y: 960 }, 1);
        let _ = t.touch(EdgeEv::Move, Point { x: 600, y: 900 }, 2);
        let step = match t.touch(EdgeEv::Up, Point { x: 700, y: 800 }, 3) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(step.verdict, Verdict::None);
        assert_eq!(t.why, Why::OutsideEdge);
        assert_eq!(t.none_emitted, 1);
    }

    #[test]
    fn engine_short_and_wrong_are_not_user() {
        let mut a = EdgeTrack::new();
        let _ = a.touch(EdgeEv::Down, Point { x: 12, y: 600 }, 1);
        let s1 = match a.touch(EdgeEv::Up, Point { x: 40, y: 600 }, 2) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(s1.verdict, Verdict::None);
        assert_eq!(a.why, Why::ShortTravel);

        let mut b = EdgeTrack::new();
        let _ = b.touch(EdgeEv::Down, Point { x: 12, y: 600 }, 1);
        let s2 = match b.touch(EdgeEv::Up, Point { x: 70, y: 800 }, 2) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(s2.verdict, Verdict::None);
        assert_eq!(b.why, Why::WrongDirection);
    }

    #[test]
    fn cancel_locks_until_up() {
        let mut t = EdgeTrack::new();
        let _ = t.touch(EdgeEv::Down, Point { x: 12, y: 600 }, 1);
        let (s, a) = match t.touch(EdgeEv::Cancel, Point { x: 20, y: 600 }, 2) {
            Ok(v) => (v.phase, v.act),
            Err(_) => (Phase::Idle, Act::None),
        };
        assert_eq!(s, Phase::Locked);
        assert_eq!(a, Act::Cancel);
        assert_eq!(
            t.touch(EdgeEv::Move, Point { x: 30, y: 600 }, 3),
            Err(EdCode::EventBad.val())
        );
        let (s2, _) = match t.touch(EdgeEv::Up, Point { x: 30, y: 600 }, 4) {
            Ok(v) => (v.phase, v.act),
            Err(_) => (Phase::Active, Act::None),
        };
        assert_eq!(s2, Phase::Idle);
    }

    #[test]
    fn edge_cases_are_geometry_anchored() {
        let mut v = 0u32;
        while v < VARIANTS {
            let mut e = 0u32;
            while e < EDGE_KINDS as u32 {
                let i = v + e * VARIANTS;
                let c = match edge_expect(i) {
                    Ok(x) => x,
                    Err(_) => return,
                };
                assert!(in_screen(c.start));
                assert!(in_screen(c.end));
                // 变体 3 是保留区原地按下（内移 0 也要归系统），其余内移非零。
                assert!(c.variant == 3 || inward_travel(c.edge, c.start, c.end).unsigned_abs() > 0);
                e += 1;
            }
            v += 1;
        }
        assert_eq!(edge_expect(EDGE_ROUNDS), Err(EdCode::CoverBad.val()));
    }

    #[test]
    fn edge_rounds_have_zero_drift() {
        match edge_rounds(EDGE_ROUNDS) {
            Ok(s) => {
                assert_eq!(s.attempts, EDGE_ROUNDS);
                assert!(s.balanced());
                assert_eq!(s.user_total(), 250);
                assert_eq!(s.system, 250);
                assert_eq!(s.none, 500);
                assert_eq!(s.short, 250);
                assert!(s.left > 0 && s.right > 0 && s.top > 0 && s.bottom > 0);
                assert!(s.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
        assert_eq!(expected_ledger(EDGE_ROUNDS), Ok((250, 250, 500)));
    }

    #[test]
    fn edge_rounds_reject_bad_ranges() {
        assert_eq!(edge_rounds(0), Err(EdCode::CoverBad.val()));
        assert_eq!(edge_rounds(EDGE_ROUNDS + 1), Err(EdCode::CoverBad.val()));
        assert_eq!(expected_ledger(0), Err(EdCode::CoverBad.val()));
    }

    #[test]
    fn negative_criterion_no_false_user() {
        let mut i = 0u32;
        while i < EDGE_ROUNDS {
            let c = match edge_expect(i) {
                Ok(v) => v,
                Err(_) => return,
            };
            if c.expect == Verdict::None {
                match arbitrate(c.start, c.end) {
                    Ok(a) => {
                        assert_ne!(a.verdict, Verdict::User);
                        assert_eq!(a.why, c.why);
                    }
                    Err(_) => assert!(false),
                }
            }
            i += 1;
        }
    }

    #[test]
    fn reserved_band_always_wins() {
        let mut i = 0u32;
        while i < CONFLICT_ROUNDS {
            let c = match conflict_expect(i) {
                Ok(v) => v,
                Err(_) => return,
            };
            if c.band == Band::Reserved {
                match arbitrate(c.start, c.end) {
                    Ok(a) => {
                        assert_eq!(a.verdict, Verdict::System);
                        assert_eq!(a.why, Why::ReservedBand);
                    }
                    Err(_) => assert!(false),
                }
            }
            i += 1;
        }
    }

    #[test]
    fn conflict_rounds_are_clean() {
        match conflict_rounds(CONFLICT_ROUNDS) {
            Ok(s) => {
                assert_eq!(s.attempts, CONFLICT_ROUNDS);
                assert!(s.balanced());
                assert!(s.clean());
                assert_eq!(s.false_kill, 0);
                assert_eq!(s.jitter, 0);
                assert!(s.user_win > 0 && s.system_win > 0);
                assert!(s.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn conflict_rounds_are_reproducible() {
        let a = conflict_rounds(240);
        let b = conflict_rounds(240);
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.digest, y.digest);
                assert_eq!(x.system_win, y.system_win);
                assert_eq!(x.user_win, y.user_win);
            }
            _ => assert!(false),
        }
        assert_eq!(conflict_rounds(0), Err(EdCode::CoverBad.val()));
    }

    #[test]
    fn arbitrate_gates_are_ordered() {
        assert_eq!(
            arbitrate(Point { x: COORD_MAX + 1, y: 0 }, Point { x: 0, y: 0 }),
            Err(EdCode::CoordBad.val())
        );
        assert_eq!(
            arbitrate(Point { x: -5, y: 0 }, Point { x: 0, y: 0 }),
            Err(EdCode::ZoneBad.val())
        );
        assert_eq!(
            arbitrate(Point { x: 12, y: 600 }, Point { x: 90, y: 1_930 }),
            Err(EdCode::ZoneBad.val())
        );
        match arbitrate(Point { x: 12, y: 600 }, Point { x: 60, y: 600 }) {
            Ok(a) => {
                assert_eq!(a.verdict, Verdict::User);
                assert_eq!(a.inward, 48);
                assert!(pair_ok(&a));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_is_balanced_and_clean() {
        match fuzz_edges(FUZZ_ROUNDS) {
            Ok(f) => {
                assert_eq!(f.attempts, FUZZ_ROUNDS);
                assert!(f.balanced());
                assert!(f.clean());
                assert_eq!(f.engine_mismatch, 0);
                assert_eq!(f.false_user, 0);
                assert_eq!(f.events(), FUZZ_ROUNDS * FUZZ_EVENTS);
                assert!(f.user > 0 && f.system > 0 && f.none > 0);
                assert!(f.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_rejects_zero_and_is_stable() {
        assert_eq!(fuzz_edges(0), Err(EdCode::CoverBad.val()));
        match (fuzz_edges(400), fuzz_edges(400)) {
            (Ok(a), Ok(b)) => assert_eq!(a.digest, b.digest),
            _ => assert!(false),
        }
    }

    #[test]
    fn injection_catches_four_routes() {
        match inject_rounds(9) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [9, 9, 9, 9]);
                assert_eq!(l.caught, [9, 9, 9, 9]);
            }
            Err(_) => assert!(false),
        }
        assert_eq!(inject_rounds(0), Err(EdCode::CoverBad.val()));
    }

    #[test]
    fn render_round_trips() {
        match edge_rounds(GOLDEN_ROUNDS) {
            Ok(s) => {
                let text = render_edge(&s);
                assert_eq!(text.split('|').count(), EDGE_FIELDS);
                assert_eq!(audit_render(&text), Ok(EDGE_FIELDS));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_tamper_is_rejected() {
        match edge_rounds(GOLDEN_ROUNDS) {
            Ok(s) => {
                let text = render_edge(&s);
                let bad = text.replacen("64|", "65|", 1);
                assert!(bad != text);
                assert_eq!(audit_render(&bad), Err(EdCode::LedgerBad.val()));
                assert_eq!(audit_render("64|1"), Err(EdCode::RenderBad.val()));
                let bad2 = text.replacen("0x", "0y", 1);
                assert_eq!(audit_render(&bad2), Err(EdCode::RenderBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn parse_helpers_are_strict() {
        assert_eq!(parse_u32("42"), Some(42));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("4x"), None);
        assert_eq!(parse_u32("99999999999"), None);
        assert_eq!(parse_hex16("0x00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("0x1"), None);
        assert_eq!(parse_hex16("0y00000000000000FF"), None);
    }

    #[test]
    fn golden_is_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
        match golden_run() {
            Ok((n, u, s, x, d)) => {
                assert_eq!(n, GOLDEN_ROUNDS);
                assert_eq!(n, u + s + x);
                assert!(d != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn engine_audit_catches_tamper() {
        let mut t = EdgeTrack::new();
        let _ = t.touch(EdgeEv::Down, Point { x: 12, y: 600 }, 1);
        let _ = t.touch(EdgeEv::Up, Point { x: 90, y: 600 }, 2);
        assert!(t.audit().is_ok());
        let mut bad = t;
        bad.role = Role::System;
        assert_eq!(bad.audit(), Err(EdCode::SystemBad.val()));
        let mut bad2 = t;
        bad2.none_emitted = bad2.accepted.saturating_add(5);
        assert_eq!(bad2.audit(), Err(EdCode::LedgerBad.val()));
    }

    #[test]
    fn codes_are_closed() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(detail(EdCode::SystemBad.val()), EdCode::SystemBad.detail());
        assert_eq!(detail(0xDEAD), "未知诊断码");
        let mut i = 0usize;
        while i < EdCode::ALL.len() {
            if let Some(c) = EdCode::ALL.get(i) {
                assert!(c.val() > SEG_BASE);
                assert!(c.val() <= SEG_BASE + 0x0F);
                assert!(!c.detail().is_empty());
            }
            i += 1;
        }
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48989_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(extra_checks().len(), 8);
    }

    // ── F48989 20 操作集 ──────────────────────────────────────────
    #[test]
    fn operation_table_is_audited() {
        assert_eq!(OP_COUNT, 20);
        assert_eq!(audit_operations(), Ok(20));
        assert_eq!(OPS.len(), 20);
        // id 连续、名字唯一、边缘操作恰 2 条。
        let mut i = 0usize;
        let mut edge_ops = 0u32;
        while i < OP_COUNT {
            let op = OPS[i];
            assert_eq!(op.id as usize, i + 1);
            let mut j = i + 1;
            while j < OP_COUNT {
                assert_ne!(OPS[j].name, op.name);
                j += 1;
            }
            if op.edge_required {
                edge_ops += 1;
            }
            i += 1;
        }
        assert_eq!(edge_ops, 2);
        assert_eq!(OpFamily::ALL.len(), 6);
        assert_eq!(OpFamily::Rotate.name(), "rotate");
    }

    #[test]
    fn operation_set_probe_covers_all() {
        assert_eq!(operation_set_probe(), Ok(20));
    }

    #[test]
    fn operation_preconditions_are_gated() {
        // 边缘操作：非边缘起手 → DirBad。
        let edge_op = OPS[18];
        assert!(edge_op.edge_required);
        let mut input = sample_input(&edge_op);
        input.from_edge = false;
        assert_eq!(
            run_operation(&edge_op, &input),
            Err(EdCode::DirBad.val())
        );
        // 方向反了 → DirBad。
        let swipe = OPS[3];
        let mut rev = sample_input(&swipe);
        rev.dx = -rev.dx;
        assert_eq!(run_operation(&swipe, &rev), Err(EdCode::DirBad.val()));
        // 捏合方向反了 → DirBad。
        let pinch = OPS[7];
        let mut rev2 = sample_input(&pinch);
        rev2.spread = -rev2.spread;
        assert_eq!(run_operation(&pinch, &rev2), Err(EdCode::DirBad.val()));
        // 旋转方向反了 → DirBad。
        let rot = OPS[9];
        let mut rev3 = sample_input(&rot);
        rev3.angle_mdeg = -rev3.angle_mdeg;
        assert_eq!(run_operation(&rot, &rev3), Err(EdCode::DirBad.val()));
        // 指头数不足 → GeomBad。
        let mut few = sample_input(&OPS[14]);
        few.fingers = 2;
        assert_eq!(
            run_operation(&OPS[14], &few),
            Err(EdCode::GeomBad.val())
        );
        // 位移不足 → GeomBad。
        let mut short = sample_input(&OPS[4]);
        short.dx = 10;
        short.dy = 1;
        assert_eq!(
            run_operation(&OPS[4], &short),
            Err(EdCode::GeomBad.val())
        );
        // 坐标越算术窗 → CoordBad。
        let mut far = sample_input(&OPS[0]);
        far.dx = COORD_MAX + 1;
        assert_eq!(run_operation(&OPS[0], &far), Err(EdCode::CoordBad.val()));
        // 滚动轴不符 → DirBad。
        let scroll_v = OPS[12];
        let mut wrong_axis = sample_input(&scroll_v);
        wrong_axis.dx = 200;
        wrong_axis.dy = 1;
        assert_eq!(
            run_operation(&scroll_v, &wrong_axis),
            Err(EdCode::DirBad.val())
        );
    }

    #[test]
    fn operation_rounds_pass_twenty() {
        match operation_rounds() {
            Ok(s) => {
                assert_eq!(s.total, 20);
                assert_eq!(s.passed, 20);
                assert_eq!(s.rejected, 20);
                assert!(s.all_passed());
                assert_eq!(s.audit(), Ok(()));
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    // ── F48990 参数配置面 ─────────────────────────────────────────
    #[test]
    fn param_defaults_validate_and_round_trip() {
        let c = ParamConfig::default_config();
        assert_eq!(c.validate(), Ok(5));
        assert_eq!(ParamKind::ALL.len(), 5);
        assert_eq!(ParamKind::Distance.name(), "distance");
        assert_eq!(
            serialize_params(&c),
            "threshold=240@1..10000|speed=800@50..20000|timing=300@10..60000|distance=48@8..4096|limit=5@1..10"
        );
        assert_eq!(parse_params(&serialize_params(&c)), Some(c));
        assert_eq!(param_consistency(), Ok(5));
    }

    #[test]
    fn param_values_are_range_checked() {
        let c = ParamConfig::default_config();
        let mut low = c;
        low.groups[0].value = 0;
        assert_eq!(low.validate(), Err(EdCode::BandBad.val()));
        let mut high = c;
        high.groups[4].value = 11;
        assert_eq!(high.validate(), Err(EdCode::BandBad.val()));
        // 区间倒置。
        let mut inverted = c;
        inverted.groups[1].lo = 900;
        inverted.groups[1].hi = 800;
        assert_eq!(inverted.validate(), Err(EdCode::BandBad.val()));
        // 种类重复。
        let mut dup = c;
        dup.groups[1].kind = ParamKind::Threshold;
        assert_eq!(dup.validate(), Err(EdCode::BandBad.val()));
        // 上界越过语义帽（上限组最大 10）。
        let mut cap = c;
        cap.groups[4].hi = 11;
        cap.groups[4].value = 5;
        assert_eq!(cap.validate(), Err(EdCode::BandBad.val()));
    }

    #[test]
    fn param_consistency_checks_edges() {
        assert_eq!(param_consistency(), Ok(5));
        // 每组的 lo-1 / hi+1 都必须被拒（顺序遍历验证 5 组全覆盖）。
        let c = ParamConfig::default_config();
        let mut i = 0usize;
        while i < PARAM_GROUPS {
            let g = c.groups[i];
            let low = ParamGroup {
                value: g.lo - 1,
                ..g
            };
            assert!(!low.valid());
            let high = ParamGroup {
                value: g.hi + 1,
                ..g
            };
            assert!(!high.valid());
            i += 1;
        }
    }

    #[test]
    fn param_parser_is_strict() {
        let good = serialize_params(&ParamConfig::default_config());
        assert!(parse_params(&good).is_some());
        assert!(parse_params("").is_none());
        assert!(parse_params("threshold=240@1..10000").is_none());
        assert!(parse_params("threshold=240@1..10000|speed=800@50..20000|timing=300@10..60000|distance=48@8..4096").is_none());
        assert!(parse_params(&format!("{}|limit=5@1..10", good)).is_none());
        assert!(parse_params("speed=800@50..20000|threshold=240@1..10000|timing=300@10..60000|distance=48@8..4096|limit=5@1..10").is_none());
        assert!(parse_params("threshold=240@1..10000|speed=800@50..20000|timing=300@10..60000|distance=48@8..4096|limit=5@1..10x").is_none());
        assert!(parse_params("threshold=0@1..10000|speed=800@50..20000|timing=300@10..60000|distance=48@8..4096|limit=5@1..10").is_none());
    }
}
