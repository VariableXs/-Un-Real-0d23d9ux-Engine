//! UNX-F48983 2 指滚动（切轴锁定）+ UNX-F48984 2 指捏合（阈值采样）· 0x71680
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族第二槽）：
//!   UNX-F48983 2 指滚动（切轴锁定）: ① 纯功能代码 ≥400 行；
//!          ② UNX-F48983-J1 切轴锁定 ×10^2 正确。
//!   UNX-F48984 2 指捏合（间距阈值采样）: ① 纯功能代码 ≥380 行；
//!          ② UNX-F48984-J1 捏合 ×10^2 阈值采样一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（切轴）：位移合法性 → 双轴累积 → 死区 → 优势比 → 滞回锁轴 → 入账。
//! 死区先于优势比：起手 2~3 像素的抖动两轴都小，"比值"在分母极小时毫无
//! 意义（1px vs 0px 就是无穷倍）。先过死区，比值判定才有物理意义。
//! 滞回后于锁轴：一旦锁上，反轴必须比锁定轴多出 FLIP_MARGIN 才允许翻轴。
//! 若只按"当拍谁大"翻轴，手指在斜向 45° 附近会每帧抖动翻轴，滚动变成
//! 二维乱窜；滞回是"切轴锁定"这个名字本身的语义。
//! 闸门次序（捏合）：距离合法性 → 去抖计数 → 阈值跨越 → 滞回释放 → 落账。
//! 去抖先于跨越：单帧误报（读数毛刺）不得触发捏合。连续 PINCH_DEBOUNCE
//! 拍同向越阈才换态；断续越阈只累加不换态。
//! 滞回后于跨越：释放阈值 = 起手阈值 + PINCH_HYST，避免在阈值线上反复
//! 收拢/张开（抖动会把一次捏合记成 N 次）。
//! 既有闸门次序（误触抑制）：时钟 → 坐标 → 面积边界 → 手掌面积 → 边缘起手 → 收账。
//! 面积边界先于手掌：w/h 越界是「事件本身坏了」（坏数据不该参与任何
//! 判定），而手掌是「数据合法但语义是误触」。若先判手掌，坏数据会被
//! 误诊成手掌，排障时把传感器缺陷当用户姿势问题。
//! 手掌先于边缘：手掌常常同时压住屏幕边缘。若先报 EdgeBad，真实原因
//! （整只手）被边缘标签掩盖，而且「边缘起手」的业务含义是「用户主动
//! 从边缘滑入」，手掌恰好相反。先炸 PalmBad 才能保住边缘手势的语义。
//! 关键差异：本模块回答三组互不改写的问题——
//! ① 切轴：2 指轨迹"朝哪个轴、锁没锁住"（纯累积 + 比值 + 滞回，不产生动作码）；
//! ② 捏合：两指间距"跨没跨阈值"（纯采样 + 去抖 + 滞回，不做缩放倍率）；
//! ③ 既有：该不该收这一拍接触（手掌/边缘抑制）与这一拍有多可信（四特征加权）。
//! 抑制是纯函数不反向修改置信度：先收后评，评级只用于上层降权，不改本模块
//! 的收账。切轴/捏合同样只产出状态与统计，不做手势→动作映射（B10-DISPATCH）、
//! 不做槽位生命周期（B10-GUARD）、不做边缘保留区裁决（B10-EDGE）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71680，B10 族第二槽）。
pub const SEG_BASE: u32 = 0x71680;

/// 逻辑屏宽（像素）。
pub const SCREEN_W: i32 = 1_920;
/// 逻辑屏高（像素）。
pub const SCREEN_H: i32 = 1_080;

/// 边缘带宽度（像素）。
pub const EDGE_MARGIN: u32 = 8;
/// 手掌面积阈值（当量 30×30 单位）。
pub const PALM_AREA: u32 = 900;
/// 合法面积上界（超过即视为坏数据）。
pub const AREA_MAX: u32 = 40_000;

/// 置信度满分。
pub const CONF_FULL: u32 = 1_000;
/// 高置信下界。
pub const HIGH: u32 = 700;
/// 中置信下界。
pub const MID: u32 = 400;

/// 边缘距归一参考（像素）。
pub const EDGE_REF: u32 = 200;
/// 停留时长归一参考（毫秒）。
pub const DWELL_REF: u32 = 600;
/// 轨迹长度归一参考（像素）。
pub const PATH_REF: u32 = 2_000;

/// 四特征权重（面积 / 边缘距 / 停留 / 轨迹），必须求和 1000。
pub const WEIGHTS: [u32; 4] = [300, 300, 200, 200];

/// 手掌注入轮数（卡面 ×10^3）。
pub const PALM_ROUNDS: u32 = 1_000;
/// 边缘注入轮数。
pub const EDGE_ROUNDS: u32 = 1_000;
/// 合法点击轮数。
pub const LEGIT_ROUNDS: u32 = 1_000;
/// 置信度样本轮数。
pub const CONF_ROUNDS: u32 = 1_000;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数。
pub const STATS_FIELDS: usize = 6;
/// 黄金脚本轮数。
pub const GOLDEN_ROUNDS: u32 = 16;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdCode {
    /// 手掌面积越界（误触）
    PalmBad,
    /// 边缘起手（误触）
    EdgeBad,
    /// 权重/阈值表损坏
    TableBad,
    /// 面积字段非法
    AreaBad,
    /// 置信度重算不一致
    ScoreBad,
    /// 置信层与分数不一致
    LayerBad,
    /// 坐标越界
    CoordBad,
    /// 时钟回退或越界
    TickBad,
    /// 账本不守恒
    LedgerBad,
    /// 渲染不可解析
    RenderBad,
    /// 复跑不一致
    ReplayBad,
    /// 覆盖不足
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// fuzz 断言失败
    FuzzBad,
}

impl EdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            EdCode::PalmBad => "手掌面积达到误触阈值，接触被抑制",
            EdCode::EdgeBad => "接触起手于屏幕边缘带，按误触抑制",
            EdCode::TableBad => "权重表/阈值表不满足守恒或有序",
            EdCode::AreaBad => "接触面积字段为零或越过合法上界",
            EdCode::ScoreBad => "置信度重算与声称值不一致",
            EdCode::LayerBad => "置信层与分数区间不匹配",
            EdCode::CoordBad => "接触坐标落在逻辑屏外",
            EdCode::TickBad => "时钟回退或越过上界",
            EdCode::LedgerBad => "收账/拒账/手掌/边缘四账不守恒",
            EdCode::RenderBad => "账本文本字段数或数值不可解析",
            EdCode::ReplayBad => "两次确定性复跑结果不一致",
            EdCode::CoverBad => "覆盖轮数为零或越过上限",
            EdCode::SegmentBad => "诊断码越出本模块段窗",
            EdCode::FuzzBad => "fuzz 结果与独立复算不一致",
        }
    }

    pub const ALL: [EdCode; 14] = [
        EdCode::PalmBad,
        EdCode::EdgeBad,
        EdCode::TableBad,
        EdCode::AreaBad,
        EdCode::ScoreBad,
        EdCode::LayerBad,
        EdCode::CoordBad,
        EdCode::TickBad,
        EdCode::LedgerBad,
        EdCode::RenderBad,
        EdCode::ReplayBad,
        EdCode::CoverBad,
        EdCode::SegmentBad,
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

// ── 边缘带 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    None,
    Left,
    Right,
    Top,
    Bottom,
}

impl Zone {
    pub fn name(self) -> &'static str {
        match self {
            Zone::None => "none",
            Zone::Left => "left",
            Zone::Right => "right",
            Zone::Top => "top",
            Zone::Bottom => "bottom",
        }
    }

    pub const ALL: [Zone; 5] = [
        Zone::None,
        Zone::Left,
        Zone::Right,
        Zone::Top,
        Zone::Bottom,
    ];
}

/// 接触点（坐标 + 接触框，单位：像素 / 当量单位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Contact {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Contact {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Contact {
        Contact { x, y, w, h }
    }

    /// 接触面积（饱和乘，避免坏数据回绕）。
    pub fn area(&self) -> u32 {
        self.w.saturating_mul(self.h)
    }
}

/// 边缘判定：显式 L→R→T→B 优先级，保证角落确定性。
pub fn edge_zone(c: &Contact) -> Zone {
    let dl = c.x;
    let dr = SCREEN_W - 1 - c.x;
    let dt = c.y;
    let db = SCREEN_H - 1 - c.y;
    if dl >= 0 && (dl as u32) <= EDGE_MARGIN {
        return Zone::Left;
    }
    if dr >= 0 && (dr as u32) <= EDGE_MARGIN {
        return Zone::Right;
    }
    if dt >= 0 && (dt as u32) <= EDGE_MARGIN {
        return Zone::Top;
    }
    if db >= 0 && (db as u32) <= EDGE_MARGIN {
        return Zone::Bottom;
    }
    Zone::None
}

/// 到最近边缘的距离（调用前坐标必须已在屏内）。
pub fn edge_distance(c: &Contact) -> u32 {
    let dl = c.x.max(0) as u32;
    let dr = (SCREEN_W - 1 - c.x).max(0) as u32;
    let dt = c.y.max(0) as u32;
    let db = (SCREEN_H - 1 - c.y).max(0) as u32;
    let mut m = dl;
    if dr < m {
        m = dr;
    }
    if dt < m {
        m = dt;
    }
    if db < m {
        m = db;
    }
    m
}

// ── 收账引擎 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    pub tick: u32,
    pub attempts: u32,
    pub accepted: u32,
    pub palm: u32,
    pub edge: u32,
    pub invalid: u32,
    pub digest: u64,
}

impl Gate {
    pub fn new() -> Gate {
        Gate {
            tick: 0,
            attempts: 0,
            accepted: 0,
            palm: 0,
            edge: 0,
            invalid: 0,
            digest: p_core::FNV_OFF,
        }
    }

    fn reject(&mut self, code: u32) -> Result<Zone, u32> {
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    /// 提交一拍接触：闸门次序固定为 时钟 → 坐标 → 面积 → 手掌 → 边缘。
    pub fn submit(&mut self, c: Contact, tick: u32) -> Result<Zone, u32> {
        self.attempts = self.attempts.saturating_add(1);
        if tick <= self.tick {
            self.invalid = self.invalid.saturating_add(1);
            return self.reject(EdCode::TickBad.val());
        }
        self.tick = tick;
        if c.x < 0 || c.y < 0 || c.x >= SCREEN_W || c.y >= SCREEN_H {
            self.invalid = self.invalid.saturating_add(1);
            return self.reject(EdCode::CoordBad.val());
        }
        let a = c.area();
        if a == 0 || a > AREA_MAX {
            self.invalid = self.invalid.saturating_add(1);
            return self.reject(EdCode::AreaBad.val());
        }
        if a >= PALM_AREA {
            self.palm = self.palm.saturating_add(1);
            return self.reject(EdCode::PalmBad.val());
        }
        let zone = edge_zone(&c);
        if zone != Zone::None {
            self.edge = self.edge.saturating_add(1);
            return self.reject(EdCode::EdgeBad.val());
        }
        self.accepted = self.accepted.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, 1);
        Ok(Zone::None)
    }

    pub fn audit(&self) -> Result<(), u32> {
        let sum = self
            .accepted
            .saturating_add(self.palm)
            .saturating_add(self.edge)
            .saturating_add(self.invalid);
        if self.attempts != sum {
            return Err(EdCode::LedgerBad.val());
        }
        if self.attempts > 0 && self.digest == 0 {
            return Err(EdCode::ReplayBad.val());
        }
        Ok(())
    }

    pub fn stats(&self) -> MetaStats {
        MetaStats {
            attempts: self.attempts,
            accepted: self.accepted,
            palm: self.palm,
            edge: self.edge,
            other: self.invalid,
            digest: self.digest,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetaStats {
    pub attempts: u32,
    pub accepted: u32,
    pub palm: u32,
    pub edge: u32,
    pub other: u32,
    pub digest: u64,
}

impl MetaStats {
    pub fn balanced(&self) -> bool {
        self.attempts > 0
            && self.attempts
                == self
                    .accepted
                    .saturating_add(self.palm)
                    .saturating_add(self.edge)
                    .saturating_add(self.other)
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

// ── 置信度（纯函数） ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    pub area: u32,
    pub edge_dist: u32,
    pub dwell: u32,
    pub path: u32,
}

impl Sample {
    pub fn new(area: u32, edge_dist: u32, dwell: u32, path: u32) -> Sample {
        Sample {
            area,
            edge_dist,
            dwell,
            path,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Low,
    Mid,
    High,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Low => "low",
            Tier::Mid => "mid",
            Tier::High => "high",
        }
    }

    pub fn code(self) -> u32 {
        match self {
            Tier::Low => 0,
            Tier::Mid => 1,
            Tier::High => 2,
        }
    }

    pub const ALL: [Tier; 3] = [Tier::Low, Tier::Mid, Tier::High];
}

fn norm_area(area: u32) -> u32 {
    let scaled = area.saturating_mul(CONF_FULL) / PALM_AREA.max(1);
    CONF_FULL.saturating_sub(scaled.min(CONF_FULL))
}

fn norm_edge(d: u32) -> u32 {
    d.saturating_mul(CONF_FULL).saturating_div(EDGE_REF.max(1)).min(CONF_FULL)
}

fn norm_dwell(ms: u32) -> u32 {
    ms.saturating_mul(CONF_FULL)
        .saturating_div(DWELL_REF.max(1))
        .min(CONF_FULL)
}

fn norm_path(px: u32) -> u32 {
    px.saturating_mul(CONF_FULL)
        .saturating_div(PATH_REF.max(1))
        .min(CONF_FULL)
}

/// 四特征归一值（顺序：面积 / 边缘距 / 停留 / 轨迹）。
pub fn features(s: &Sample) -> [u32; 4] {
    [
        norm_area(s.area),
        norm_edge(s.edge_dist),
        norm_dwell(s.dwell),
        norm_path(s.path),
    ]
}

/// 置信度总分（0..=1000）。
pub fn confidence(s: &Sample) -> u32 {
    let f = features(s);
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < 4 {
        let fi = match f.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let wi = match WEIGHTS.get(i) {
            Some(v) => *v,
            None => 0,
        };
        acc = acc.saturating_add(fi.saturating_mul(wi));
        i += 1;
    }
    acc / CONF_FULL
}

/// 置信层：阈值单调（分数越高层越高）。
pub fn tier_of(score: u32) -> Tier {
    if score >= HIGH {
        Tier::High
    } else if score >= MID {
        Tier::Mid
    } else {
        Tier::Low
    }
}

/// 重算校验：声称分数必须与重算分数逐位一致。
pub fn verify_score(s: &Sample, claimed: u32) -> Result<u32, u32> {
    let real = confidence(s);
    if real != claimed {
        return Err(EdCode::ScoreBad.val());
    }
    Ok(real)
}

/// 重算校验：声称层必须与分数区间一致。
pub fn verify_tier(score: u32, claimed: Tier) -> Result<Tier, u32> {
    if score > CONF_FULL {
        return Err(EdCode::LayerBad.val());
    }
    let real = tier_of(score);
    if real != claimed {
        return Err(EdCode::LayerBad.val());
    }
    Ok(real)
}

/// 权重与阈值表审计。
pub fn audit_tables() -> Result<u32, u32> {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < WEIGHTS.len() {
        match WEIGHTS.get(i) {
            Some(w) => {
                if *w == 0 || *w > CONF_FULL {
                    return Err(EdCode::TableBad.val());
                }
                sum = sum.saturating_add(*w);
            }
            None => return Err(EdCode::TableBad.val()),
        }
        i += 1;
    }
    if sum != CONF_FULL {
        return Err(EdCode::TableBad.val());
    }
    if !(MID > 0 && MID < HIGH && HIGH <= CONF_FULL) {
        return Err(EdCode::TableBad.val());
    }
    Ok(sum)
}

/// 阈值单调审计：0..=1000 全部枚举，层号必须随分数不降。
pub fn audit_monotone() -> Result<u32, u32> {
    let mut prev = 0u32;
    let mut score = 0u32;
    while score <= CONF_FULL {
        let t = tier_of(score).code();
        if t < prev {
            return Err(EdCode::LayerBad.val());
        }
        prev = t;
        if score == CONF_FULL {
            break;
        }
        score = score.saturating_add(1);
    }
    if tier_of(HIGH) != Tier::High || tier_of(MID) != Tier::Mid {
        return Err(EdCode::LayerBad.val());
    }
    if MID > 0 && tier_of(MID - 1) != Tier::Low {
        return Err(EdCode::LayerBad.val());
    }
    if HIGH > 0 && tier_of(HIGH - 1) != Tier::Mid {
        return Err(EdCode::LayerBad.val());
    }
    Ok(CONF_FULL.saturating_add(1))
}

// ── 三路探针（手掌 / 边缘 / 合法） ────────────────────────────────
pub fn palm_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > PALM_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut g = Gate::new();
    let mut i = 0u32;
    while i < rounds {
        let x = 300 + (i % 1_200) as i32;
        let y = 200 + (i % 600) as i32;
        let side = 30 + (i % 11);
        let c = Contact::new(x, y, side, side);
        if g.submit(c, i + 1) != Err(EdCode::PalmBad.val()) {
            return Err(EdCode::FuzzBad.val());
        }
        i = i.saturating_add(1);
    }
    if g.palm != rounds || g.accepted != 0 {
        return Err(EdCode::LedgerBad.val());
    }
    g.audit()?;
    Ok(g.palm)
}

pub fn edge_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > EDGE_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut g = Gate::new();
    let mut i = 0u32;
    while i < rounds {
        // 轮流压四条边，接触框保持「小面积」，确保只有边缘闸命中。
        let c = match i % 4 {
            0 => Contact::new((i % 8) as i32, 500, 4, 4),
            1 => Contact::new(SCREEN_W - 1 - (i % 8) as i32, 500, 4, 4),
            2 => Contact::new(900, (i % 8) as i32, 4, 4),
            _ => Contact::new(900, SCREEN_H - 1 - (i % 8) as i32, 4, 4),
        };
        if g.submit(c, i + 1) != Err(EdCode::EdgeBad.val()) {
            return Err(EdCode::FuzzBad.val());
        }
        i = i.saturating_add(1);
    }
    if g.edge != rounds || g.accepted != 0 {
        return Err(EdCode::LedgerBad.val());
    }
    g.audit()?;
    Ok(g.edge)
}

pub fn legit_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > LEGIT_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut g = Gate::new();
    let mut i = 0u32;
    while i < rounds {
        let x = 100 + (i % 1_600) as i32;
        let y = 100 + (i % 800) as i32;
        let w = 1 + (i % 20);
        let h = 1 + ((i / 3) % 20);
        if g.submit(Contact::new(x, y, w, h), i + 1) != Ok(Zone::None) {
            return Err(EdCode::FuzzBad.val());
        }
        i = i.saturating_add(1);
    }
    if g.accepted != rounds || g.palm != 0 || g.edge != 0 {
        return Err(EdCode::LedgerBad.val());
    }
    g.audit()?;
    Ok(g.accepted)
}

/// 面积先于边缘：大面积 + 边缘起手必须报 PalmBad。
pub fn order_probe() -> Result<u32, u32> {
    let mut g = Gate::new();
    let c = Contact::new(0, 0, 40, 40);
    if g.submit(c, 1) != Err(EdCode::PalmBad.val()) {
        return Err(EdCode::PalmBad.val());
    }
    if g.palm != 1 || g.edge != 0 {
        return Err(EdCode::LedgerBad.val());
    }
    // 小面积 + 边缘 → EdgeBad。
    if g.submit(Contact::new(0, 500, 4, 4), 2) != Err(EdCode::EdgeBad.val()) {
        return Err(EdCode::EdgeBad.val());
    }
    // 屏外 → CoordBad；零面积 → AreaBad；时钟回退 → TickBad。
    if g.submit(Contact::new(-1, 5, 4, 4), 3) != Err(EdCode::CoordBad.val()) {
        return Err(EdCode::CoordBad.val());
    }
    if g.submit(Contact::new(500, 500, 0, 9), 4) != Err(EdCode::AreaBad.val()) {
        return Err(EdCode::AreaBad.val());
    }
    if g.submit(Contact::new(500, 500, 4, 4), 4) != Err(EdCode::TickBad.val()) {
        return Err(EdCode::TickBad.val());
    }
    g.audit()?;
    Ok(5)
}

// ── 置信度批量对账 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConfStats {
    pub rounds: u32,
    pub low: u32,
    pub mid: u32,
    pub high: u32,
    pub digest: u64,
}

impl ConfStats {
    pub fn new() -> ConfStats {
        ConfStats {
            rounds: 0,
            low: 0,
            mid: 0,
            high: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn balanced(&self) -> bool {
        self.rounds > 0
            && self.rounds
                == self
                    .low
                    .saturating_add(self.mid)
                    .saturating_add(self.high)
    }
}

pub fn conf_rounds(rounds: u32) -> Result<ConfStats, u32> {
    if rounds == 0 || rounds > CONF_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut s = ConfStats::new();
    let mut i = 0u32;
    while i < rounds {
        let area = 1 + (i % PALM_AREA);
        let edge_dist = (i * 7) % 600;
        let dwell = 10 + (i * 13) % 900;
        let path = (i * 31) % 4_000;
        let sample = Sample::new(area, edge_dist, dwell, path);
        let score = confidence(&sample);
        verify_score(&sample, score)?;
        let tier = tier_of(score);
        verify_tier(score, tier)?;
        match tier {
            Tier::Low => s.low = s.low.saturating_add(1),
            Tier::Mid => s.mid = s.mid.saturating_add(1),
            Tier::High => s.high = s.high.saturating_add(1),
        }
        s.digest = p_core::fold_u32(s.digest, score);
        s.digest = p_core::fold_u32(s.digest, tier.code());
        s.rounds = s.rounds.saturating_add(1);
        i = i.saturating_add(1);
    }
    if !s.balanced() || s.digest == 0 {
        return Err(EdCode::LedgerBad.val());
    }
    Ok(s)
}

/// 独立复算：不经 confidence()，用展开式重算分数（账本交叉验证）。
pub fn recompute(s: &Sample) -> u32 {
    let a = norm_area(s.area);
    let e = norm_edge(s.edge_dist);
    let d = norm_dwell(s.dwell);
    let p = norm_path(s.path);
    let w0 = match WEIGHTS.get(0) { Some(v) => *v, None => 0 };
    let w1 = match WEIGHTS.get(1) { Some(v) => *v, None => 0 };
    let w2 = match WEIGHTS.get(2) { Some(v) => *v, None => 0 };
    let w3 = match WEIGHTS.get(3) { Some(v) => *v, None => 0 };
    (a.saturating_mul(w0)
        .saturating_add(e.saturating_mul(w1))
        .saturating_add(d.saturating_mul(w2))
        .saturating_add(p.saturating_mul(w3)))
        / CONF_FULL
}

// ── 稳定注入（四路） ──────────────────────────────────────────────
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
    if rounds == 0 {
        return Err(EdCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; INJECT_ROUTES],
        caught: [0; INJECT_ROUTES],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 手掌面积必须炸 PalmBad。
        let mut g = Gate::new();
        if g.submit(Contact::new(500, 500, 30, 30), 1).err()
            == Some(EdCode::PalmBad.val())
        {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);

        // ② 边缘起手必须炸 EdgeBad。
        let mut g2 = Gate::new();
        if g2.submit(Contact::new(0, 500, 3, 3), 1).err()
            == Some(EdCode::EdgeBad.val())
        {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);

        // ③ 声称分数被篡改必须炸 ScoreBad。
        let s = Sample::new(10, 200, 300, 500);
        let real = confidence(&s);
        let tampered = real.saturating_add(1).min(CONF_FULL);
        if tampered != real
            && verify_score(&s, tampered).err() == Some(EdCode::ScoreBad.val())
        {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);

        // ④ 声称层与分数不符必须炸 LayerBad。
        if verify_tier(HIGH, Tier::Low).err() == Some(EdCode::LayerBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(EdCode::CoverBad.val());
    }
    Ok(log)
}

// ── fuzz：随机接触流 ×10^4 ────────────────────────────────────────
pub fn fuzz_contacts(rounds: u32) -> Result<MetaStats, u32> {
    if rounds == 0 {
        return Err(EdCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x7168_0F84_0000_0053);
    let mut g = Gate::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let x = rng.below(SCREEN_W as u32) as i32;
        let y = rng.below(SCREEN_H as u32) as i32;
        let w = 1 + rng.below(60);
        let h = 1 + rng.below(60);
        let c = Contact::new(x, y, w, h);
        tick = tick.saturating_add(1);
        // 独立预测：面积 → 手掌 → 边缘 → 收账（与引擎闸序一致）。
        let a = c.area();
        let predicted_ok = a < PALM_AREA && edge_zone(&c) == Zone::None;
        let got = g.submit(c, tick);
        if predicted_ok {
            if got != Ok(Zone::None) {
                return Err(EdCode::FuzzBad.val());
            }
        } else if a >= PALM_AREA {
            if got != Err(EdCode::PalmBad.val()) {
                return Err(EdCode::FuzzBad.val());
            }
        } else if got != Err(EdCode::EdgeBad.val()) {
            return Err(EdCode::FuzzBad.val());
        }
        i = i.saturating_add(1);
    }
    g.audit()?;
    Ok(g.stats())
}

// ── 渲染 ──────────────────────────────────────────────────────────
pub fn render_stats(s: &MetaStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.attempts, s.accepted, s.palm, s.edge, s.other, s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
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
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

pub fn audit_render(text: &str) -> Result<usize, u32> {
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
    if n != STATS_FIELDS {
        return Err(EdCode::RenderBad.val());
    }
    let attempts = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let accepted = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let palm = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let edge = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let other = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    if attempts == 0
        || accepted.saturating_add(palm).saturating_add(edge).saturating_add(other)
            != attempts
    {
        return Err(EdCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(EdCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
pub fn golden_run() -> Result<MetaStats, u32> {
    let mut g = Gate::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        // 每轮三拍：手掌（拒）、边缘（拒）、活性区点击（收）。
        let c1 = Contact::new(400 + (i as i32) * 7, 300, 32, 32);
        tick = tick.saturating_add(1);
        if g.submit(c1, tick).err() != Some(EdCode::PalmBad.val()) {
            return Err(EdCode::PalmBad.val());
        }
        let c2 = match i % 4 {
            0 => Contact::new(2, 400, 3, 3),
            1 => Contact::new(SCREEN_W - 3, 400, 3, 3),
            2 => Contact::new(800, 2, 3, 3),
            _ => Contact::new(800, SCREEN_H - 3, 3, 3),
        };
        tick = tick.saturating_add(1);
        if g.submit(c2, tick).err() != Some(EdCode::EdgeBad.val()) {
            return Err(EdCode::EdgeBad.val());
        }
        let c3 = Contact::new(200 + (i as i32) * 11, 200 + (i as i32) * 3, 6, 5);
        tick = tick.saturating_add(1);
        if g.submit(c3, tick) != Ok(Zone::None) {
            return Err(EdCode::CoordBad.val());
        }
        i = i.saturating_add(1);
    }
    g.audit()?;
    let s = g.stats();
    if s.palm != GOLDEN_ROUNDS || s.edge != GOLDEN_ROUNDS || s.accepted != GOLDEN_ROUNDS {
        return Err(EdCode::LedgerBad.val());
    }
    Ok(s)
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EdCode::ReplayBad.val());
    }
    let c = conf_rounds(256)?;
    let d = conf_rounds(256)?;
    if c != d {
        return Err(EdCode::ReplayBad.val());
    }
    Ok(())
}

// ── 码族与段位 ────────────────────────────────────────────────────
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

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if audit_tables()? != CONF_FULL {
        return Err(EdCode::TableBad.val());
    }
    if audit_monotone()? != CONF_FULL.saturating_add(1) {
        return Err(EdCode::LayerBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let p = palm_probe(PALM_ROUNDS)?;
    let e = edge_probe(EDGE_ROUNDS)?;
    let l = legit_probe(LEGIT_ROUNDS)?;
    if p != PALM_ROUNDS || e != EDGE_ROUNDS || l != LEGIT_ROUNDS {
        return Err(EdCode::LedgerBad.val());
    }
    if order_probe()? != 5 {
        return Err(EdCode::CoverBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let s = conf_rounds(CONF_ROUNDS)?;
    if !s.balanced() {
        return Err(EdCode::LedgerBad.val());
    }
    let mut i = 0u32;
    while i < 64 {
        let sample = Sample::new(1 + (i * 17) % 2_000, (i * 23) % 500, 1 + (i * 9) % 800, (i * 37) % 4_000);
        let a = confidence(&sample);
        let b = recompute(&sample);
        if a != b {
            return Err(EdCode::ScoreBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(())
}

// ── F48983 2 指滚动：切轴锁定（J1 ×10^2） ────────────────────────
/// 切轴场景轮数（卡面 ×10^2 的载体）。
pub const LOCK_ROUNDS: u32 = 100;
/// 单场景沿主轴推进的拍数。
pub const LOCK_STEPS: usize = 5;
/// 单场景横轴噪声拍数（滞回内不得翻轴）。
pub const NOISE_STEPS: usize = 3;
/// 起手死区（像素）：两轴都没越过它时不判轴。
pub const SLOP_PX: i64 = 8;
/// 主轴优势比分子：主轴 × DOMINANCE_DEN ≥ 次轴 × DOMINANCE_NUM。
pub const DOMINANCE_NUM: i64 = 3;
/// 主轴优势比分母（NUM/DEN = 1.5 倍）。
pub const DOMINANCE_DEN: i64 = 2;
/// 翻轴滞回（像素）：候选轴必须比锁定轴多出该值才允许翻轴。
pub const FLIP_MARGIN: i64 = 24;
/// 单拍位移绝对值上界（越界即坏数据，先于任何轴判定）。
pub const AXIS_CAP: i64 = 30_000;

/// 切轴状态：未定 / 水平锁定 / 垂直锁定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AxisLock {
    Unknown,
    Horizontal,
    Vertical,
}

impl AxisLock {
    pub fn code(self) -> u32 {
        match self {
            AxisLock::Unknown => 0,
            AxisLock::Horizontal => 1,
            AxisLock::Vertical => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AxisLock::Unknown => "unknown",
            AxisLock::Horizontal => "horizontal",
            AxisLock::Vertical => "vertical",
        }
    }

    /// 另一条轴（死亡分支返回 Unknown，不 panic）。
    pub fn other(self) -> AxisLock {
        match self {
            AxisLock::Horizontal => AxisLock::Vertical,
            AxisLock::Vertical => AxisLock::Horizontal,
            AxisLock::Unknown => AxisLock::Unknown,
        }
    }

    pub const ALL: [AxisLock; 3] = [AxisLock::Unknown, AxisLock::Horizontal, AxisLock::Vertical];
}

/// 主副轴判定（纯函数）：死区 → 等长不判 → 1.5 倍优势比。
///
/// 等长（角平分线）写死不判轴：45° 斜划在物理上就不属于任何一条轴，
/// 硬分一边会让同一轨迹在不同抖动下给出不同结论。
pub fn dominant_axis(dx: i64, dy: i64) -> AxisLock {
    let ax = dx.unsigned_abs();
    let ay = dy.unsigned_abs();
    if ax < SLOP_PX as u64 && ay < SLOP_PX as u64 {
        return AxisLock::Unknown;
    }
    if ax == ay {
        return AxisLock::Unknown;
    }
    let (major, minor, axis) = if ax > ay {
        (ax, ay, AxisLock::Horizontal)
    } else {
        (ay, ax, AxisLock::Vertical)
    };
    if (major as u128) * (DOMINANCE_DEN as u128) < (minor as u128) * (DOMINANCE_NUM as u128) {
        return AxisLock::Unknown;
    }
    axis
}

/// 切轴锁定引擎：双轴累积 + 优势比 + 滞回。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AxisLockEngine {
    acc_x: i64,
    acc_y: i64,
    pub lock: AxisLock,
    pub samples: u32,
    pub transitions: u32,
    pub flips: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl AxisLockEngine {
    pub fn new() -> AxisLockEngine {
        AxisLockEngine {
            acc_x: 0,
            acc_y: 0,
            lock: AxisLock::Unknown,
            samples: 0,
            transitions: 0,
            flips: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 双轴累积量（只读，供独立复算核对）。
    pub fn accum(&self) -> (i64, i64) {
        (self.acc_x, self.acc_y)
    }

    /// 提交一拍两指滚动位移：合法性 → 累积 → 主副轴 → 滞回锁轴。
    pub fn update(&mut self, dx: i32, dy: i32) -> Result<AxisLock, u32> {
        if (dx as i64).unsigned_abs() > AXIS_CAP as u64
            || (dy as i64).unsigned_abs() > AXIS_CAP as u64
        {
            self.rejects = self.rejects.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, EdCode::CoordBad.val());
            return Err(EdCode::CoordBad.val());
        }
        self.acc_x = self.acc_x.saturating_add(dx as i64);
        self.acc_y = self.acc_y.saturating_add(dy as i64);
        let cand = dominant_axis(self.acc_x, self.acc_y);
        let mut next = self.lock;
        if cand != AxisLock::Unknown && cand != self.lock {
            let mx = self.acc_x.unsigned_abs();
            let my = self.acc_y.unsigned_abs();
            // cand_mag = 候选轴自身的累积量；held_mag = 当前锁定轴的累积量。
            let (cand_mag, held_mag) = match cand {
                AxisLock::Horizontal => (mx, my),
                _ => (my, mx),
            };
            // 未锁时无条件接受候选；已锁时要求越过滞回线（这就是"切轴锁定"）。
            let allow = self.lock == AxisLock::Unknown
                || cand_mag >= held_mag.saturating_add(FLIP_MARGIN as u64);
            if allow {
                if self.lock != AxisLock::Unknown {
                    self.flips = self.flips.saturating_add(1);
                }
                self.lock = cand;
                self.transitions = self.transitions.saturating_add(1);
                next = cand;
            }
        }
        self.samples = self.samples.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, next.code());
        self.digest = p_core::fold_i32(self.digest, dx);
        self.digest = p_core::fold_i32(self.digest, dy);
        Ok(next)
    }

    /// 审计：样本非零、翻轴不越迁移、锁定轴有真实位移背书、指纹非零。
    ///
    /// 不要求「当拍主副轴 == 锁定轴」：滞回的全部意义就是让锁比数据慢一步，
    /// 若审计要求两者恒等，滞回本身就会被判成不一致。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples == 0 {
            return Err(EdCode::CoverBad.val());
        }
        if self.flips > self.transitions || self.transitions > self.samples {
            return Err(EdCode::LedgerBad.val());
        }
        if self.lock != AxisLock::Unknown {
            let mx = self.acc_x.unsigned_abs();
            let my = self.acc_y.unsigned_abs();
            let held = match self.lock {
                AxisLock::Horizontal => mx,
                _ => my,
            };
            if held < SLOP_PX as u64 {
                return Err(EdCode::FuzzBad.val());
            }
        }
        if self.digest == 0 {
            return Err(EdCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 切轴批量账（100 个确定性场景的汇总）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockStats {
    pub scenarios: u32,
    pub updates: u32,
    pub locks: u32,
    pub flips: u32,
    pub noise_holds: u32,
    pub digest: u64,
}

impl LockStats {
    pub fn new() -> LockStats {
        LockStats {
            scenarios: 0,
            updates: 0,
            locks: 0,
            flips: 0,
            noise_holds: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 场景全覆盖 + 每场景锁住一条轴 + 噪声段每次都被滞回挡住。
    pub fn balanced(&self) -> bool {
        self.scenarios > 0
            && self.updates > 0
            && self.locks == self.scenarios
            && self.noise_holds == self.scenarios.saturating_mul(NOISE_STEPS as u32)
            && self.digest != 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(EdCode::LedgerBad.val());
        }
        if self.flips > self.locks {
            return Err(EdCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 切轴 ×10^2：每个场景按"指定主轴推进 → 横轴噪声 → 每 5 个场景一次真翻轴"
/// 走一遍，逐拍与独立期望比对。期望值来自场景生成参数（i 的奇偶与 i%5），
/// 不来自引擎，故这是一条可以证伪的判据而不是自证。
pub fn axis_lock_scenarios(rounds: u32) -> Result<LockStats, u32> {
    if rounds == 0 || rounds > LOCK_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut s = LockStats::new();
    let mut i = 0u32;
    while i < rounds {
        let want = if i % 2 == 0 {
            AxisLock::Horizontal
        } else {
            AxisLock::Vertical
        };
        let mut e = AxisLockEngine::new();
        let mut primary = 0i64;
        let mut j = 0usize;
        while j < LOCK_STEPS {
            // 主轴 14..=34 px/拍、次轴 1..=3 px/拍：比例恒 > 1.5。
            let p = 14 + (i.saturating_mul(7).saturating_add((j as u32).saturating_mul(5)) % 21) as i64;
            let c = 1 + (i.saturating_add(j as u32) % 3) as i64;
            let (dx, dy) = match want {
                AxisLock::Horizontal => (p as i32, c as i32),
                _ => (c as i32, p as i32),
            };
            primary = primary.saturating_add(p);
            let got = e.update(dx, dy)?;
            if got != want {
                return Err(EdCode::FuzzBad.val());
            }
            s.updates = s.updates.saturating_add(1);
            s.digest = p_core::fold_u32(s.digest, got.code());
            j += 1;
        }
        // 噪声段：每拍只在次轴上加 3..=6 px，累积量远不到滞回线。
        let noise = 3 + (i % 4) as i64;
        let mut k = 0usize;
        while k < NOISE_STEPS {
            let (dx, dy) = match want {
                AxisLock::Horizontal => (2, noise as i32),
                _ => (noise as i32, 2),
            };
            let got = e.update(dx, dy)?;
            s.updates = s.updates.saturating_add(1);
            if got != want {
                return Err(EdCode::FuzzBad.val());
            }
            s.noise_holds = s.noise_holds.saturating_add(1);
            k += 1;
        }
        // 每 5 个场景做一次真翻轴：反轴一次越过滞回线，必须恰好翻一次。
        if i % 5 == 0 {
            // 候选轴一次推入「1.5 倍主轴累积 + 滞回余量」：既满足优势比（否则判 Unknown），
            // 又越过 FLIP_MARGIN（否则滞回挡下），两关同时过才算一次合法翻轴。
            let cross = primary
                .saturating_add(primary / 2)
                .saturating_add(FLIP_MARGIN)
                .saturating_add(20 + (i % 9) as i64);
            let (dx, dy) = match want {
                AxisLock::Horizontal => (0, cross as i32),
                _ => (cross as i32, 0),
            };
            let got = e.update(dx, dy)?;
            s.updates = s.updates.saturating_add(1);
            if got != want.other() || e.flips != 1 {
                return Err(EdCode::FuzzBad.val());
            }
            // 翻轴后旧轴小幅回推：必须仍停在新区，不得翻回。
            let back = 2 + (i % 5) as i64;
            let (bdx, bdy) = match want {
                AxisLock::Horizontal => (back as i32, 0),
                _ => (0, back as i32),
            };
            let again = e.update(bdx, bdy)?;
            s.updates = s.updates.saturating_add(1);
            if again != want.other() {
                return Err(EdCode::FuzzBad.val());
            }
            s.flips = s.flips.saturating_add(1);
        }
        e.audit()?;
        s.locks = s.locks.saturating_add(1);
        s.scenarios = s.scenarios.saturating_add(1);
        s.digest = p_core::fold_u32(s.digest, e.lock.code());
        s.digest = p_core::fold_u32(s.digest, e.flips);
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

/// 切轴复跑：两次 100 场景必须逐位一致（确定性）。
pub fn lock_replay() -> Result<u64, u32> {
    let a = axis_lock_scenarios(LOCK_ROUNDS)?;
    let b = axis_lock_scenarios(LOCK_ROUNDS)?;
    if a != b {
        return Err(EdCode::ReplayBad.val());
    }
    Ok(a.digest)
}

/// 切轴账本文本：`场景|拍数|锁定|翻轴|滞回挡住|指纹`。
pub fn render_lock(s: &LockStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.scenarios, s.updates, s.locks, s.flips, s.noise_holds, s.digest
    )
}

/// 切轴账本回读：字段数、场景自洽、锁定数与场景数相等、指纹非零。
pub fn audit_lock_render(text: &str) -> Result<usize, u32> {
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
    if n != STATS_FIELDS {
        return Err(EdCode::RenderBad.val());
    }
    let scenarios = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let updates = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let locks = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let flips = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let holds = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    if scenarios == 0 || updates == 0 || locks != scenarios {
        return Err(EdCode::LedgerBad.val());
    }
    if flips > locks || holds != scenarios.saturating_mul(NOISE_STEPS as u32) {
        return Err(EdCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(EdCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── F48984 2 指捏合：间距阈值采样（J1 ×10^2） ────────────────────
/// 捏合场景轮数（卡面 ×10^2 的载体）。
pub const PINCH_ROUNDS: u32 = 100;
/// 起手间距阈值（像素当量）：两指间距低于它即视为收拢。
pub const PINCH_THRESHOLD: u32 = 240;
/// 释放滞回（像素）：间距必须升到 阈值 + 该值 以上才释放。
pub const PINCH_HYST: u32 = 24;
/// 去抖拍数：连续同向越阈这么多拍才换态。
pub const PINCH_DEBOUNCE: u32 = 2;
/// 间距采样上界（越界即坏数据）。
pub const PINCH_MAX: u32 = 20_000;

/// 捏合状态：中立 / 已收拢 / 已张开。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PinchState {
    Neutral,
    Closer,
    Farther,
}

impl PinchState {
    pub fn code(self) -> u32 {
        match self {
            PinchState::Neutral => 0,
            PinchState::Closer => 1,
            PinchState::Farther => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PinchState::Neutral => "neutral",
            PinchState::Closer => "closer",
            PinchState::Farther => "farther",
        }
    }

    pub const ALL: [PinchState; 3] = [PinchState::Neutral, PinchState::Closer, PinchState::Farther];
}

/// 单次采样结果：间距、状态、本拍是否换态、间距增量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PinchSample {
    pub dist: u32,
    pub state: PinchState,
    pub crossed: bool,
    pub delta: i32,
}

/// 捏合阈值采样器：去抖 + 滞回 + 双向计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PinchSampler {
    pub threshold: u32,
    pub state: PinchState,
    pub samples: u32,
    pub engages: u32,
    pub releases: u32,
    pub rejects: u32,
    streak: u32,
    dir: i8,
    last: u32,
    primed: bool,
    digest: u64,
}

impl PinchSampler {
    pub fn new(threshold: u32) -> PinchSampler {
        PinchSampler {
            threshold: if threshold == 0 { 1 } else { threshold },
            state: PinchState::Neutral,
            samples: 0,
            engages: 0,
            releases: 0,
            rejects: 0,
            streak: 0,
            dir: 0,
            last: 0,
            primed: false,
            digest: p_core::FNV_OFF,
        }
    }

    /// 释放线：起手阈值 + 滞回。
    pub fn release_line(&self) -> u32 {
        self.threshold.saturating_add(PINCH_HYST)
    }

    /// 采一拍间距：合法性 → 增量 → 去抖 → 跨越/释放。
    pub fn feed(&mut self, dist: u32) -> Result<PinchSample, u32> {
        if dist == 0 || dist > PINCH_MAX {
            self.rejects = self.rejects.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, EdCode::AreaBad.val());
            return Err(EdCode::AreaBad.val());
        }
        let raw = if self.primed {
            dist as i64 - self.last as i64
        } else {
            0
        };
        let delta = if raw > i32::MAX as i64 {
            i32::MAX
        } else if raw < i32::MIN as i64 {
            i32::MIN
        } else {
            raw as i32
        };
        self.last = dist;
        self.primed = true;
        let band = self.release_line();
        // −1 = 低于起手阈值；+1 = 高于释放线；0 = 滞回带内（不表态）。
        let dir: i8 = if dist < self.threshold {
            -1
        } else if dist > band {
            1
        } else {
            0
        };
        if dir == 0 {
            self.streak = 0;
            self.dir = 0;
        } else if dir == self.dir {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.dir = dir;
            self.streak = 1;
        }
        let mut crossed = false;
        if self.streak >= PINCH_DEBOUNCE {
            if self.state == PinchState::Neutral && self.dir == -1 {
                self.state = PinchState::Closer;
                self.engages = self.engages.saturating_add(1);
                crossed = true;
            } else if self.state == PinchState::Closer && self.dir == 1 {
                self.state = PinchState::Farther;
                self.releases = self.releases.saturating_add(1);
                crossed = true;
            } else if self.state == PinchState::Farther && self.dir == -1 {
                self.state = PinchState::Closer;
                self.engages = self.engages.saturating_add(1);
                crossed = true;
            }
            // 换态后清计数：同一段持续越阈不得重复计数。
            self.streak = 0;
            self.dir = 0;
        }
        self.samples = self.samples.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, dist);
        self.digest = p_core::fold_u32(self.digest, self.state.code());
        self.digest = p_core::fold_u32(self.digest, if crossed { 1 } else { 0 });
        Ok(PinchSample {
            dist,
            state: self.state,
            crossed,
            delta,
        })
    }

    /// 审计：样本非零、收拢不落后于张开（最多领先一拍）、事件不超样本。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples == 0 {
            return Err(EdCode::CoverBad.val());
        }
        if self.engages < self.releases || self.engages > self.releases.saturating_add(1) {
            return Err(EdCode::LedgerBad.val());
        }
        if self.engages.saturating_add(self.releases) > self.samples {
            return Err(EdCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(EdCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 捏合批量账（100 个确定性场景的汇总）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PinchStats {
    pub scenarios: u32,
    pub samples: u32,
    pub engages: u32,
    pub releases: u32,
    pub digest: u64,
}

impl PinchStats {
    pub fn new() -> PinchStats {
        PinchStats {
            scenarios: 0,
            samples: 0,
            engages: 0,
            releases: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 每场景恰一次收拢 + 一次释放，且指纹非零。
    pub fn consistent(&self) -> bool {
        self.scenarios > 0
            && self.engages == self.scenarios
            && self.releases == self.scenarios
            && self.digest != 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.consistent() {
            return Err(EdCode::LedgerBad.val());
        }
        if self.samples < self.scenarios.saturating_mul(2) {
            return Err(EdCode::CoverBad.val());
        }
        Ok(())
    }
}

/// 捏合 ×10^2：每场景一段"高位 → 低位（越阈）→ 高位（释放）"的间距序列，
/// 跨越拍位置由序列构造决定（索引 3 收拢、索引 7 释放），与采样器实现无关。
pub fn pinch_threshold_rounds(rounds: u32) -> Result<PinchStats, u32> {
    if rounds == 0 || rounds > PINCH_ROUNDS {
        return Err(EdCode::CoverBad.val());
    }
    let mut s = PinchStats::new();
    let mut i = 0u32;
    while i < rounds {
        let hi = PINCH_THRESHOLD
            .saturating_add(40)
            .saturating_add(i % 17);
        let lo = PINCH_THRESHOLD
            .saturating_sub(32)
            .saturating_sub(i % 23);
        let seq = [hi, hi, lo, lo, lo, lo, hi, hi, hi];
        let want_cross: [usize; 2] = [3, 7];
        let mut p = PinchSampler::new(PINCH_THRESHOLD);
        let mut idx = 0usize;
        let mut crosses = 0u32;
        let mut k = 0usize;
        while k < seq.len() {
            let d = match seq.get(k) {
                Some(v) => *v,
                None => return Err(EdCode::CoverBad.val()),
            };
            let smp = p.feed(d)?;
            s.samples = s.samples.saturating_add(1);
            if smp.crossed {
                if want_cross.get(idx) != Some(&k) {
                    return Err(EdCode::FuzzBad.val());
                }
                idx += 1;
                crosses = crosses.saturating_add(1);
            }
            k += 1;
        }
        if crosses != 2 || p.engages != 1 || p.releases != 1 {
            return Err(EdCode::LedgerBad.val());
        }
        p.audit()?;
        s.engages = s.engages.saturating_add(p.engages);
        s.releases = s.releases.saturating_add(p.releases);
        s.digest = p_core::fold_u32(s.digest, p.digest as u32);
        s.digest = p_core::fold_u32(s.digest, p.samples);
        s.scenarios = s.scenarios.saturating_add(1);
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

/// 去抖与滞回负判据：单拍浅越阈不得收拢；滞回带内波动不得释放。
pub fn pinch_debounce_probe() -> Result<u32, u32> {
    let hi = PINCH_THRESHOLD.saturating_add(80);
    let lo = PINCH_THRESHOLD.saturating_sub(10);
    // ① 单拍交替越阈：去抖拍数 2 未满，不得收拢。
    let mut a = PinchSampler::new(PINCH_THRESHOLD);
    let flicker = [hi, lo, hi, lo, hi];
    let mut k = 0usize;
    while k < flicker.len() {
        if let Some(d) = flicker.get(k) {
            let _ = a.feed(*d)?;
        }
        k += 1;
    }
    if a.engages != 0 || a.state != PinchState::Neutral {
        return Err(EdCode::FuzzBad.val());
    }
    // ② 收拢后滞回带内波动不得释放，越过释放线两拍才释放。
    let mut b = PinchSampler::new(PINCH_THRESHOLD);
    let _ = b.feed(hi)?;
    let _ = b.feed(hi)?;
    let _ = b.feed(lo)?;
    let s1 = b.feed(lo)?;
    if !s1.crossed || b.state != PinchState::Closer {
        return Err(EdCode::LedgerBad.val());
    }
    let band = PINCH_THRESHOLD.saturating_add(PINCH_HYST / 2);
    let _ = b.feed(band)?;
    let s2 = b.feed(band)?;
    if s2.crossed || b.releases != 0 {
        return Err(EdCode::FuzzBad.val());
    }
    let up = b.release_line().saturating_add(30);
    let _ = b.feed(up)?;
    let s3 = b.feed(up)?;
    if !s3.crossed || b.releases != 1 {
        return Err(EdCode::LedgerBad.val());
    }
    b.audit()?;
    Ok(2)
}

/// 捏合账本文本：`场景|样本|收拢|释放|保留|指纹`。
pub fn render_pinch(s: &PinchStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.scenarios,
        s.samples,
        s.engages,
        s.releases,
        0u32,
        s.digest
    )
}

/// 捏合账本回读：字段数、收拢=释放=场景、指纹非零。
pub fn audit_pinch_render(text: &str) -> Result<usize, u32> {
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
    if n != STATS_FIELDS {
        return Err(EdCode::RenderBad.val());
    }
    let scenarios = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let samples = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let engages = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let releases = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(EdCode::RenderBad.val()),
    };
    if scenarios == 0 || engages != scenarios || releases != scenarios {
        return Err(EdCode::LedgerBad.val());
    }
    if samples < scenarios.saturating_mul(2) {
        return Err(EdCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(EdCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 域检查项（动态明细槽上限 8） ──────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) F48983 J1：切轴锁定 ×10^2（含复跑一致性）。
    let lock = axis_lock_scenarios(LOCK_ROUNDS);
    let lock_replay_ok = lock_replay().is_ok();
    let lock_render = lock
        .as_ref()
        .map(render_lock)
        .map(|t| audit_lock_render(&t) == Ok(STATS_FIELDS))
        .unwrap_or(false);
    out.push((
        "F48983 切轴锁定×10^2正确",
        matches!(&lock, Ok(s) if s.balanced() && s.locks == LOCK_ROUNDS)
            && lock_replay_ok
            && lock_render,
        match &lock {
            Ok(s) => format!(
                "lock={} flips={} holds={} rep={}",
                s.locks, s.flips, s.noise_holds, lock_replay_ok as u8
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 2) F48984 J1：捏合阈值采样 ×10^2（含去抖/滞回负判据）。
    let pinch = pinch_threshold_rounds(PINCH_ROUNDS);
    let debounce = pinch_debounce_probe();
    let pinch_render = pinch
        .as_ref()
        .map(render_pinch)
        .map(|t| audit_pinch_render(&t) == Ok(STATS_FIELDS))
        .unwrap_or(false);
    out.push((
        "F48984 捏合阈值×10^2一致",
        matches!(&pinch, Ok(s) if s.consistent() && s.engages == PINCH_ROUNDS)
            && debounce == Ok(2)
            && pinch_render,
        match &pinch {
            Ok(s) => format!(
                "eng={} rel={} n={} deb={:?}",
                s.engages,
                s.releases,
                s.samples,
                debounce.is_ok()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 3) 既有：手掌千次零漏放。
    let palm = palm_probe(PALM_ROUNDS);
    out.push((
        "F48983 手掌千次零漏放",
        matches!(&palm, Ok(n) if *n == PALM_ROUNDS),
        match &palm {
            Ok(n) => format!("palm={} rounds={}", n, PALM_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 4) 既有：边缘千次零漏放 + 合法点击零误杀。
    let edge = edge_probe(EDGE_ROUNDS);
    let legit = legit_probe(LEGIT_ROUNDS);
    out.push((
        "F48983 边缘千次零漏放",
        matches!(&edge, Ok(n) if *n == EDGE_ROUNDS)
            && matches!(&legit, Ok(n) if *n == LEGIT_ROUNDS),
        match (&edge, &legit) {
            (Ok(e), Ok(l)) => format!("edge={} legit={}", e, l),
            _ => String::from("edge/legit err"),
        },
    ));

    // 5) 既有：闸序面积先于边缘 + 四路注入。
    let order = order_probe();
    let inj = inject_rounds(4);
    out.push((
        "F48983 闸序与四路注入",
        matches!(&order, Ok(5)) && matches!(&inj, Ok(l) if l.all_caught()),
        match (&order, &inj) {
            (Ok(n), Ok(l)) => format!("gates={} caught={:?}", n, l.caught),
            _ => String::from("order/inject err"),
        },
    ));

    // 6) 既有：权重守恒 + 阈值单调。
    let tables = audit_tables();
    let mono = audit_monotone();
    out.push((
        "F48984 权重守恒与单调",
        tables == Ok(CONF_FULL) && mono == Ok(CONF_FULL.saturating_add(1)),
        match &tables {
            Ok(n) => format!("weights sum={} mid={} high={}", n, MID, HIGH),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 7) 既有：千次置信对账 + 独立复算交叉验证。
    let conf = conf_rounds(CONF_ROUNDS);
    let mut cross_ok = true;
    let mut i = 0u32;
    while i < 32 {
        let sample = Sample::new(
            1 + (i * 19) % 2_000,
            (i * 29) % 500,
            1 + (i * 7) % 800,
            (i * 41) % 4_000,
        );
        if confidence(&sample) != recompute(&sample) {
            cross_ok = false;
        }
        i = i.saturating_add(1);
    }
    out.push((
        "F48984 千次置信对账",
        matches!(&conf, Ok(s) if s.balanced() && s.rounds == CONF_ROUNDS) && cross_ok,
        match &conf {
            Ok(s) => format!(
                "n={} low={} mid={} high={}",
                s.rounds, s.low, s.mid, s.high
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 8) 既有：fuzz 与账本（含切轴/捏合两本账）。
    let fuzz = fuzz_contacts(4_096);
    let render = golden_run().map(|s| render_stats(&s));
    out.push((
        "F48983/84 fuzz与账本",
        matches!(&fuzz, Ok(s) if s.balanced())
            && matches!(&render, Ok(t) if audit_render(t) == Ok(STATS_FIELDS))
            && lock_render
            && pinch_render,
        match (&fuzz, &render) {
            (Ok(f), Ok(_)) => format!(
                "acc={}/{} lk={} pn={}",
                f.accepted, f.attempts, lock_render as u8, pinch_render as u8
            ),
            _ => String::from("fuzz/render err"),
        },
    ));

    out
}

pub fn run_unx48983_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48983 误触抑制与手势置信度");
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
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71680);
        assert_eq!(SCREEN_W, 1_920);
        assert_eq!(SCREEN_H, 1_080);
        assert_eq!(EDGE_MARGIN, 8);
        assert_eq!(PALM_AREA, 900);
        assert_eq!(AREA_MAX, 40_000);
        assert_eq!(WEIGHTS, [300, 300, 200, 200]);
        assert_eq!(EdCode::ALL.len(), 14);
    }

    #[test]
    fn area_is_saturating() {
        let c = Contact::new(0, 0, u32::MAX, 2);
        assert_eq!(c.area(), u32::MAX);
    }

    #[test]
    fn edge_zones_are_deterministic() {
        assert_eq!(edge_zone(&Contact::new(0, 500, 3, 3)), Zone::Left);
        assert_eq!(edge_zone(&Contact::new(SCREEN_W - 1, 500, 3, 3)), Zone::Right);
        assert_eq!(edge_zone(&Contact::new(900, 0, 3, 3)), Zone::Top);
        assert_eq!(edge_zone(&Contact::new(900, SCREEN_H - 1, 3, 3)), Zone::Bottom);
        assert_eq!(edge_zone(&Contact::new(900, 500, 3, 3)), Zone::None);
        // 角落按 L→R→T→B 优先级判为 Left。
        assert_eq!(edge_zone(&Contact::new(0, 0, 3, 3)), Zone::Left);
    }

    #[test]
    fn edge_distance_counts() {
        assert_eq!(edge_distance(&Contact::new(0, 500, 3, 3)), 0);
        assert_eq!(edge_distance(&Contact::new(100, 500, 3, 3)), 100);
        assert_eq!(edge_distance(&Contact::new(900, SCREEN_H - 1, 3, 3)), 0);
    }

    #[test]
    fn palm_is_rejected_first() {
        let mut g = Gate::new();
        assert_eq!(
            g.submit(Contact::new(0, 0, 30, 30), 1),
            Err(EdCode::PalmBad.val())
        );
        assert_eq!(g.palm, 1);
        assert_eq!(g.edge, 0);
    }

    #[test]
    fn small_edge_is_rejected() {
        let mut g = Gate::new();
        assert_eq!(
            g.submit(Contact::new(4, 500, 3, 3), 1),
            Err(EdCode::EdgeBad.val())
        );
        assert_eq!(g.edge, 1);
    }

    #[test]
    fn legit_center_is_accepted() {
        let mut g = Gate::new();
        assert_eq!(g.submit(Contact::new(900, 500, 6, 6), 1), Ok(Zone::None));
        assert_eq!(g.accepted, 1);
        assert!(g.audit().is_ok());
    }

    #[test]
    fn invalid_area_is_rejected() {
        let mut g = Gate::new();
        assert_eq!(
            g.submit(Contact::new(900, 500, 0, 5), 1),
            Err(EdCode::AreaBad.val())
        );
        assert_eq!(
            g.submit(Contact::new(900, 500, 500, 500), 2),
            Err(EdCode::AreaBad.val())
        );
    }

    #[test]
    fn out_of_screen_is_rejected() {
        let mut g = Gate::new();
        assert_eq!(
            g.submit(Contact::new(SCREEN_W, 500, 3, 3), 1),
            Err(EdCode::CoordBad.val())
        );
        assert_eq!(
            g.submit(Contact::new(-1, 500, 3, 3), 2),
            Err(EdCode::CoordBad.val())
        );
    }

    #[test]
    fn tick_regression_is_rejected() {
        let mut g = Gate::new();
        assert_eq!(g.submit(Contact::new(900, 500, 3, 3), 5), Ok(Zone::None));
        assert_eq!(
            g.submit(Contact::new(900, 500, 3, 3), 5),
            Err(EdCode::TickBad.val())
        );
    }

    #[test]
    fn gate_ledger_balances() {
        let mut g = Gate::new();
        let _ = g.submit(Contact::new(900, 500, 30, 30), 1);
        let _ = g.submit(Contact::new(0, 500, 3, 3), 2);
        let _ = g.submit(Contact::new(900, 500, 3, 3), 3);
        let _ = g.submit(Contact::new(-5, 500, 3, 3), 4);
        let s = g.stats();
        assert!(s.balanced());
        assert_eq!(s.attempts, 4);
        assert_eq!(s.palm, 1);
        assert_eq!(s.edge, 1);
        assert_eq!(s.accepted, 1);
        assert_eq!(s.other, 1);
    }

    #[test]
    fn weights_sum_to_full() {
        assert_eq!(audit_tables(), Ok(CONF_FULL));
    }

    #[test]
    fn thresholds_are_ordered() {
        assert!(MID > 0 && MID < HIGH && HIGH <= CONF_FULL);
    }

    #[test]
    fn tiers_follow_thresholds() {
        assert_eq!(tier_of(0), Tier::Low);
        assert_eq!(tier_of(MID - 1), Tier::Low);
        assert_eq!(tier_of(MID), Tier::Mid);
        assert_eq!(tier_of(HIGH - 1), Tier::Mid);
        assert_eq!(tier_of(HIGH), Tier::High);
        assert_eq!(tier_of(CONF_FULL), Tier::High);
    }

    #[test]
    fn monotone_audit_passes() {
        assert_eq!(audit_monotone(), Ok(CONF_FULL + 1));
    }

    #[test]
    fn perfect_sample_scores_near_full() {
        // 面积最小也占 1 单位，归一后是 999 而非 1000；其余三项已顶格。
        let s = Sample::new(1, 500, 900, 5_000);
        assert_eq!(confidence(&s), 999);
        assert_eq!(tier_of(999), Tier::High);
        assert_eq!(tier_of(CONF_FULL), Tier::High);
    }

    #[test]
    fn palm_sized_sample_scores_zero_on_area() {
        let s = Sample::new(PALM_AREA, 0, 0, 0);
        let f = features(&s);
        assert_eq!(f[0], 0);
        assert_eq!(confidence(&s), 0);
        assert_eq!(tier_of(0), Tier::Low);
    }

    #[test]
    fn score_recompute_matches() {
        let mut i = 0u32;
        while i < 200 {
            let s = Sample::new(1 + i * 13, i * 7, 5 + i * 11, i * 17);
            assert_eq!(confidence(&s), recompute(&s));
            i += 1;
        }
    }

    #[test]
    fn score_tamper_is_rejected() {
        let s = Sample::new(10, 200, 300, 500);
        let real = confidence(&s);
        assert_eq!(verify_score(&s, real), Ok(real));
        let bad = if real == CONF_FULL { real - 1 } else { real + 1 };
        assert_eq!(verify_score(&s, bad), Err(EdCode::ScoreBad.val()));
    }

    #[test]
    fn tier_tamper_is_rejected() {
        assert_eq!(verify_tier(HIGH, Tier::High), Ok(Tier::High));
        assert_eq!(verify_tier(HIGH, Tier::Low), Err(EdCode::LayerBad.val()));
        assert_eq!(
            verify_tier(CONF_FULL + 1, Tier::High),
            Err(EdCode::LayerBad.val())
        );
    }

    #[test]
    fn palm_probe_is_exact() {
        assert_eq!(palm_probe(64), Ok(64));
        assert_eq!(palm_probe(0), Err(EdCode::CoverBad.val()));
        assert_eq!(
            palm_probe(PALM_ROUNDS + 1),
            Err(EdCode::CoverBad.val())
        );
    }

    #[test]
    fn edge_probe_is_exact() {
        assert_eq!(edge_probe(64), Ok(64));
    }

    #[test]
    fn legit_probe_is_exact() {
        assert_eq!(legit_probe(64), Ok(64));
    }

    #[test]
    fn order_probe_covers_five_gates() {
        assert_eq!(order_probe(), Ok(5));
    }

    #[test]
    fn conf_rounds_is_balanced() {
        match conf_rounds(CONF_ROUNDS) {
            Ok(s) => {
                assert_eq!(s.rounds, CONF_ROUNDS);
                assert!(s.balanced());
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn conf_rounds_rejects_out_of_range() {
        assert_eq!(conf_rounds(0), Err(EdCode::CoverBad.val()));
        assert_eq!(
            conf_rounds(CONF_ROUNDS + 1),
            Err(EdCode::CoverBad.val())
        );
    }

    #[test]
    fn fuzz_contacts_balance() {
        match fuzz_contacts(4_096) {
            Ok(s) => {
                assert_eq!(s.attempts, 4_096);
                assert!(s.balanced());
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [5, 5, 5, 5]);
                assert_eq!(l.caught, [5, 5, 5, 5]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn injection_rejects_zero() {
        assert_eq!(inject_rounds(0), Err(EdCode::CoverBad.val()));
    }

    #[test]
    fn render_round_trips() {
        match golden_run().map(|s| render_stats(&s)) {
            Ok(text) => {
                assert_eq!(text.split('|').count(), STATS_FIELDS);
                assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_tamper_is_rejected() {
        match golden_run().map(|s| render_stats(&s)) {
            Ok(text) => {
                let bad = text.replacen("48|", "49|", 1);
                assert!(bad != text || text.starts_with("48|"));
                assert!(audit_render(&bad).is_err());
                assert_eq!(audit_render("48|1"), Err(EdCode::RenderBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_is_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
        match golden_run() {
            Ok(s) => {
                assert_eq!(s.attempts, GOLDEN_ROUNDS * 3);
                assert_eq!(s.palm, GOLDEN_ROUNDS);
                assert_eq!(s.edge, GOLDEN_ROUNDS);
                assert_eq!(s.accepted, GOLDEN_ROUNDS);
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
    fn codes_are_closed() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(detail(EdCode::PalmBad.val()), EdCode::PalmBad.detail());
        assert_eq!(detail(0xDEAD), "未知诊断码");
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48983_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }

    // ── F48983 切轴锁定 ────────────────────────────────────────────
    #[test]
    fn axis_constants_are_anchored() {
        assert_eq!(LOCK_ROUNDS, 100);
        assert_eq!(SLOP_PX, 8);
        assert_eq!(DOMINANCE_NUM * 2, DOMINANCE_DEN * 3);
        assert!(FLIP_MARGIN > SLOP_PX);
        assert_eq!(AxisLock::ALL.len(), 3);
        assert_eq!(AxisLock::Unknown.code(), 0);
        assert_eq!(AxisLock::Horizontal.name(), "horizontal");
        assert_eq!(AxisLock::Vertical.other(), AxisLock::Horizontal);
        assert_eq!(AxisLock::Unknown.other(), AxisLock::Unknown);
    }

    #[test]
    fn dominant_axis_follows_slop_and_ratio() {
        // 死区：两轴都没过 SLOP，不判。
        assert_eq!(dominant_axis(7, -7), AxisLock::Unknown);
        // 45° 等长：物理上不属于任何一条轴。
        assert_eq!(dominant_axis(60, 60), AxisLock::Unknown);
        assert_eq!(dominant_axis(-60, 60), AxisLock::Unknown);
        // 恰好 1.5 倍：边界算主副轴（≥ 即过）。
        assert_eq!(dominant_axis(30, 45), AxisLock::Vertical);
        assert_eq!(dominant_axis(45, 30), AxisLock::Horizontal);
        // 差一点不到 1.5 倍：不判。
        assert_eq!(dominant_axis(31, 45), AxisLock::Unknown);
        // 越过死区、单轴独大。
        assert_eq!(dominant_axis(8, 1), AxisLock::Horizontal);
        assert_eq!(dominant_axis(1, -8), AxisLock::Vertical);
    }

    #[test]
    fn axis_lock_hysteresis_holds_then_flips_once() {
        let mut e = AxisLockEngine::new();
        assert_eq!(e.update(30, 0), Ok(AxisLock::Horizontal));
        // 候选轴 45 < 30 + 24：优势比已成立，但滞回挡下，锁不翻。
        assert_eq!(e.update(0, 45), Ok(AxisLock::Horizontal));
        assert_eq!(e.flips, 0);
        assert_eq!(e.transitions, 1);
        // 累积到 55 ≥ 30 + 24：翻一次，且只翻一次。
        assert_eq!(e.update(0, 10), Ok(AxisLock::Vertical));
        assert_eq!(e.flips, 1);
        assert_eq!(e.transitions, 2);
        // 反向小幅回推：仍停在新轴。
        assert_eq!(e.update(5, 0), Ok(AxisLock::Vertical));
        assert_eq!(e.flips, 1);
        assert!(e.audit().is_ok());
    }

    #[test]
    fn axis_lock_rejects_out_of_cap_delta() {
        let mut e = AxisLockEngine::new();
        let over = (AXIS_CAP as i32).saturating_add(1);
        assert_eq!(e.update(over, 0), Err(EdCode::CoordBad.val()));
        assert_eq!(e.update(0, -over), Err(EdCode::CoordBad.val()));
        assert_eq!(e.rejects, 2);
        assert_eq!(e.samples, 0);
        assert_eq!(e.audit(), Err(EdCode::CoverBad.val()));
        // 合法边界值必须放行。
        assert_eq!(
            e.update(AXIS_CAP as i32, 0),
            Ok(AxisLock::Horizontal)
        );
    }

    #[test]
    fn axis_lock_100_scenarios_are_balanced() {
        match axis_lock_scenarios(LOCK_ROUNDS) {
            Ok(s) => {
                assert_eq!(s.scenarios, LOCK_ROUNDS);
                assert_eq!(s.locks, LOCK_ROUNDS);
                assert_eq!(s.noise_holds, LOCK_ROUNDS * NOISE_STEPS as u32);
                // 每 5 个场景一次真翻轴：100 / 5 = 20。
                assert_eq!(s.flips, LOCK_ROUNDS / 5);
                assert!(s.balanced());
                assert_eq!(s.audit(), Ok(()));
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn axis_lock_scenarios_reject_bad_rounds() {
        assert_eq!(axis_lock_scenarios(0), Err(EdCode::CoverBad.val()));
        assert_eq!(
            axis_lock_scenarios(LOCK_ROUNDS + 1),
            Err(EdCode::CoverBad.val())
        );
    }

    #[test]
    fn axis_lock_replay_is_deterministic() {
        match (lock_replay(), axis_lock_scenarios(LOCK_ROUNDS)) {
            (Ok(d), Ok(s)) => assert_eq!(d, s.digest),
            _ => assert!(false),
        }
    }

    #[test]
    fn lock_render_round_trips_and_rejects_tamper() {
        match axis_lock_scenarios(LOCK_ROUNDS) {
            Ok(s) => {
                let text = render_lock(&s);
                assert_eq!(text.split('|').count(), STATS_FIELDS);
                assert_eq!(audit_lock_render(&text), Ok(STATS_FIELDS));
                // 篡改锁定数（与场景数脱钩）必须被账本回读拒绝。
                let bad = text.replacen("|100|20|", "|99|20|", 1);
                assert!(bad != text);
                assert!(audit_lock_render(&bad).is_err());
                assert_eq!(audit_lock_render("100|1"), Err(EdCode::RenderBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    // ── F48984 捏合阈值采样 ────────────────────────────────────────
    #[test]
    fn pinch_constants_are_anchored() {
        assert_eq!(PINCH_ROUNDS, 100);
        assert_eq!(PINCH_DEBOUNCE, 2);
        assert!(PINCH_HYST > 0 && PINCH_HYST < PINCH_THRESHOLD);
        assert_eq!(PinchState::ALL.len(), 3);
        assert_eq!(PinchState::Neutral.code(), 0);
        assert_eq!(PinchState::Closer.name(), "closer");
        let p = PinchSampler::new(PINCH_THRESHOLD);
        assert_eq!(p.release_line(), PINCH_THRESHOLD + PINCH_HYST);
        // 阈值 0 被抬到 1，避免除零式退化。
        assert_eq!(PinchSampler::new(0).threshold, 1);
    }

    #[test]
    fn pinch_sampler_crosses_at_debounced_samples() {
        let hi = PINCH_THRESHOLD + 40;
        let lo = PINCH_THRESHOLD - 32;
        let seq = [hi, hi, lo, lo, lo, lo, hi, hi, hi];
        let want = [false, false, false, true, false, false, false, true, false];
        let mut p = PinchSampler::new(PINCH_THRESHOLD);
        let mut k = 0usize;
        while k < seq.len() {
            let d = match seq.get(k) {
                Some(v) => *v,
                None => break,
            };
            match p.feed(d) {
                Ok(smp) => {
                    assert_eq!(Some(&smp.crossed), want.get(k));
                    if k == 0 {
                        // 首拍只做基线，不产生增量。
                        assert_eq!(smp.delta, 0);
                    }
                    if k == 3 {
                        assert_eq!(smp.state, PinchState::Closer);
                    }
                    if k == 7 {
                        assert_eq!(smp.state, PinchState::Farther);
                    }
                }
                Err(_) => assert!(false),
            }
            k += 1;
        }
        assert_eq!(p.engages, 1);
        assert_eq!(p.releases, 1);
        assert_eq!(p.samples, seq.len() as u32);
        assert!(p.audit().is_ok());
    }

    #[test]
    fn pinch_probe_returns_two() {
        assert_eq!(pinch_debounce_probe(), Ok(2));
    }

    #[test]
    fn pinch_rejects_out_of_range_distance() {
        let mut p = PinchSampler::new(PINCH_THRESHOLD);
        assert_eq!(p.feed(0), Err(EdCode::AreaBad.val()));
        assert_eq!(p.feed(PINCH_MAX + 1), Err(EdCode::AreaBad.val()));
        assert_eq!(p.rejects, 2);
        assert_eq!(p.samples, 0);
        assert_eq!(p.audit(), Err(EdCode::CoverBad.val()));
        assert!(p.feed(PINCH_MAX).is_ok());
    }

    #[test]
    fn pinch_100_scenarios_are_consistent() {
        match pinch_threshold_rounds(PINCH_ROUNDS) {
            Ok(s) => {
                assert_eq!(s.scenarios, PINCH_ROUNDS);
                assert_eq!(s.engages, PINCH_ROUNDS);
                assert_eq!(s.releases, PINCH_ROUNDS);
                assert_eq!(s.samples, PINCH_ROUNDS * 9);
                assert!(s.consistent());
                assert_eq!(s.audit(), Ok(()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn pinch_rounds_reject_out_of_range() {
        assert_eq!(pinch_threshold_rounds(0), Err(EdCode::CoverBad.val()));
        assert_eq!(
            pinch_threshold_rounds(PINCH_ROUNDS + 1),
            Err(EdCode::CoverBad.val())
        );
    }

    #[test]
    fn pinch_render_round_trips_and_rejects_tamper() {
        match pinch_threshold_rounds(PINCH_ROUNDS) {
            Ok(s) => {
                let text = render_pinch(&s);
                assert_eq!(text.split('|').count(), STATS_FIELDS);
                assert_eq!(audit_pinch_render(&text), Ok(STATS_FIELDS));
                let bad = text.replacen("|100|100|", "|100|99|", 1);
                assert!(bad != text);
                assert!(audit_pinch_render(&bad).is_err());
                assert_eq!(audit_pinch_render(""), Err(EdCode::RenderBad.val()));
            }
            Err(_) => assert!(false),
        }
    }
}
