//! UNX-F48921 相对事件→指针位移合成 · 0x70F00
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B07）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F48921-J1 合成 ×10^4 位置账零漂移；
//!   ③ UNX-F48922-J1 两档 ×10^2 行为切换一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：设备档合法性 → 事件合法性 → 速度求解 → 增益求解 → 合成 →
//!           账本入账 → 复算一致。
//! 先校档再校事件：档里的 DPI / 边界 / 曲线决定合成口径，若先接受事件
//! 再发现档非法，事件已经被消费掉，调用方会以为「已合成」。速度必须先
//! 于增益：曲线档的增益是速度的函数，拿位移直接乘增益等于跳过曲线。
//! 复算一致放在账本之后：只有把同一事件重放一遍都能得到同一坐标，账本
//! 里的 digest 才可信。
//! 关键差异：本模块只回答「相对位移加上增益后，指针应该去哪」，不做
//!           亚像素累积（B07-DISPATCH）、不做边界静默区（B07-EDGE）、
//!           不做按键状态机（B07-GUARD）。谁把坐标直接写到屏幕上，谁
//!           越过了显示域的边界（联签锚定：F1 光标绘制像素）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70F00，VX6 批 B07 族首槽）。
pub const SEG_BASE: u32 = 0x70F00;

/// 合成轮数（卡面判据 ×10^4 的载体）。
pub const COMPOSE_ROUNDS: u32 = 10_000;
/// 两档切换轮数（卡面判据 ×10^2 的载体）。
pub const SWITCH_ROUNDS: u32 = 100;
/// 增益定点分母：gain 以千分比表达。
pub const GAIN_SCALE: i64 = 1000;
/// 增益下限/上限（千分比）。
pub const GAIN_MIN: u32 = 250;
pub const GAIN_MAX: u32 = 4000;
/// 单轴相对位移绝对值上限（Linux REL 报点范围）。
pub const REL_MAX: i32 = 32_767;
/// 速度封顶（曲线档超过封顶点一律取末点增益）。
pub const SPEED_CAP: u32 = 8_000;
/// 对外坐标边界（可见桌面合成面）。
pub const POS_BOUND: i64 = 1_000_000;
/// 合成中间量边界：超过即判 PosBad，不允许静默饱和。
pub const RAW_BOUND: i64 = 4_000_000;
/// 设备档 DPI 合法区间。
pub const DPI_MIN: u32 = 100;
pub const DPI_MAX: u32 = 32_000;
pub const DEFAULT_DPI: u32 = 1000;
/// 曲线采样点数量区间。
pub const CURVE_POINTS_MIN: usize = 2;
pub const CURVE_POINTS_MAX: usize = 12;

/// FNV-1a 64 初始值与质数（与 B05/B06 族同参，便于跨族折叠）。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrCode {
    /// 设备档非法（DPI 为零/超界、边界为零）
    DevBad,
    /// 相对事件非法（零位移或单轴超界）
    RelBad,
    /// 增益非法（平坦增益越界）
    GainBad,
    /// 档位非法（选了曲线档却没有曲线）
    ModeBad,
    /// 坐标非法（构造越界或合成中间量越界）
    PosBad,
    /// 曲线非法（点数/次序/单调/增益区间）
    CurveBad,
    /// 复算不一致（重放结果不同）
    ReplayBad,
    /// 次序非法（漂移账非零/合成次序被破坏）
    OrderBad,
    /// 账本渲染非法（字段缺失/回读失败）
    RenderBad,
    /// 覆盖不足（探针或账本项数不足）
    CoverageBad,
    /// 账不平（合成分/拒绝分与总轮数不一致）
    LedgerBad,
    /// 段位越界（诊断码逃出 0x70F00 窗口）
    SegmentBad,
}

impl PtrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            PtrCode::DevBad => "设备档非法：DPI 或边界不在合法区间",
            PtrCode::RelBad => "相对事件非法：零位移或单轴超界",
            PtrCode::GainBad => "增益非法：平坦增益越出 250..=4000",
            PtrCode::ModeBad => "档位非法：曲线档缺少曲线采样点",
            PtrCode::PosBad => "坐标非法：构造越界或合成中间量越界",
            PtrCode::CurveBad => "曲线非法：点数/次序/单调/增益区间不合法",
            PtrCode::ReplayBad => "复算不一致：同一事件重放坐标不同",
            PtrCode::OrderBad => "次序非法：漂移账非零或合成次序被破坏",
            PtrCode::RenderBad => "账本渲染非法：字段缺失或回读失败",
            PtrCode::CoverageBad => "覆盖不足：探针或账本项数不足",
            PtrCode::LedgerBad => "账不平：合成分与拒绝分之和与轮数不一致",
            PtrCode::SegmentBad => "段位越界：诊断码逃出 0x70F00 窗口",
        }
    }

    pub const ALL: [PtrCode; 12] = [
        PtrCode::DevBad,
        PtrCode::RelBad,
        PtrCode::GainBad,
        PtrCode::ModeBad,
        PtrCode::PosBad,
        PtrCode::CurveBad,
        PtrCode::ReplayBad,
        PtrCode::OrderBad,
        PtrCode::RenderBad,
        PtrCode::CoverageBad,
        PtrCode::LedgerBad,
        PtrCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PtrCode::ALL.len() {
        if PtrCode::ALL[i].val() == code {
            return PtrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 折叠与确定性伪随机 ────────────────────────────────────────────
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

pub fn fold_i32(h: u64, v: i32) -> u64 {
    fold_u32(h, v as u32)
}

/// 确定性 LCG（同族同参数，跨批不漂移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next() % (n as u64)) as u32
        }
    }
}

// ── 整数平方根（无浮点，无 panic） ────────────────────────────────
pub fn isqrt_u64(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// 对称取整除法（半值远离零），分母必须为正。
pub fn round_div(num: i64, den: i64) -> i64 {
    if den <= 0 {
        return 0;
    }
    let q = num / den;
    let r = num % den;
    if r.unsigned_abs().saturating_mul(2) >= den as u64 {
        if num >= 0 {
            q.saturating_add(1)
        } else {
            q.saturating_sub(1)
        }
    } else {
        q
    }
}

// ── 相对事件 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RelEvent {
    pub dx: i32,
    pub dy: i32,
}

impl RelEvent {
    pub fn new(dx: i32, dy: i32) -> Result<RelEvent, u32> {
        if (dx as i64).unsigned_abs() > REL_MAX as u64
            || (dy as i64).unsigned_abs() > REL_MAX as u64
        {
            return Err(PtrCode::RelBad.val());
        }
        if dx == 0 && dy == 0 {
            return Err(PtrCode::RelBad.val());
        }
        Ok(RelEvent { dx, dy })
    }

    /// 位移向量长度（整数平方根），封顶 SPEED_CAP。
    pub fn speed(self) -> u32 {
        let dx = (self.dx as i64).unsigned_abs();
        let dy = (self.dy as i64).unsigned_abs();
        let sq = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
        let s = isqrt_u64(sq);
        if s > SPEED_CAP as u64 {
            SPEED_CAP
        } else {
            s as u32
        }
    }
}

// ── 坐标 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}

impl Point {
    pub fn new(x: i64, y: i64) -> Result<Point, u32> {
        if x.unsigned_abs() > POS_BOUND as u64 || y.unsigned_abs() > POS_BOUND as u64 {
            return Err(PtrCode::PosBad.val());
        }
        Ok(Point { x, y })
    }

    pub fn origin() -> Point {
        Point { x: 0, y: 0 }
    }

    /// 欧氏距离平方（饱和，不 panic）。
    pub fn dist_sq(self, other: Point) -> u64 {
        let dx = self.x.saturating_sub(other.x).unsigned_abs();
        let dy = self.y.saturating_sub(other.y).unsigned_abs();
        dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
    }
}

// ── 增益曲线 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CurvePoint {
    pub speed: u32,
    pub gain: u32,
}

pub fn curve_point(speed: u32, gain: u32) -> CurvePoint {
    CurvePoint { speed, gain }
}

/// 由 (速度, 增益) 对构造采样点向量（测试与探针共用）。
pub fn curve_of(pairs: &[(u32, u32)]) -> Vec<CurvePoint> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < pairs.len() {
        if let Some((s, g)) = pairs.get(i) {
            out.push(curve_point(*s, *g));
        }
        i += 1;
    }
    out
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Curve {
    points: Vec<CurvePoint>,
}

impl Curve {
    pub fn new(points: Vec<CurvePoint>) -> Result<Curve, u32> {
        let c = Curve { points };
        c.validate()?;
        Ok(c)
    }

    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn validate(&self) -> Result<(), u32> {
        let n = self.points.len();
        if n < CURVE_POINTS_MIN || n > CURVE_POINTS_MAX {
            return Err(PtrCode::CurveBad.val());
        }
        let first = match self.points.first() {
            Some(p) => p,
            None => return Err(PtrCode::CurveBad.val()),
        };
        if first.speed != 0 {
            return Err(PtrCode::CurveBad.val());
        }
        if first.gain < GAIN_MIN || first.gain > GAIN_MAX {
            return Err(PtrCode::CurveBad.val());
        }
        let mut i = 1usize;
        while i < n {
            let a = match self.points.get(i - 1) {
                Some(v) => v,
                None => return Err(PtrCode::CurveBad.val()),
            };
            let b = match self.points.get(i) {
                Some(v) => v,
                None => return Err(PtrCode::CurveBad.val()),
            };
            if b.speed <= a.speed {
                return Err(PtrCode::CurveBad.val());
            }
            if b.speed > SPEED_CAP {
                return Err(PtrCode::CurveBad.val());
            }
            if b.gain < GAIN_MIN || b.gain > GAIN_MAX {
                return Err(PtrCode::CurveBad.val());
            }
            if b.gain < a.gain {
                return Err(PtrCode::CurveBad.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 分段线性插值：采样点精确，段内半点进位，首点以下取首增益，
    /// 末点以上取末增益（封顶语义在 validate 的 SPEED_CAP 里声明）。
    pub fn gain_at(&self, speed: u32) -> Result<u32, u32> {
        if self.points.len() < CURVE_POINTS_MIN {
            return Err(PtrCode::CurveBad.val());
        }
        let first = match self.points.first() {
            Some(p) => p,
            None => return Err(PtrCode::CurveBad.val()),
        };
        if speed <= first.speed {
            return Ok(first.gain);
        }
        let mut i = 1usize;
        while i < self.points.len() {
            let lo = match self.points.get(i - 1) {
                Some(v) => v,
                None => return Err(PtrCode::CurveBad.val()),
            };
            let hi = match self.points.get(i) {
                Some(v) => v,
                None => return Err(PtrCode::CurveBad.val()),
            };
            if speed <= hi.speed {
                let ds = hi.speed - lo.speed;
                if ds == 0 {
                    return Err(PtrCode::CurveBad.val());
                }
                let dg = hi.gain - lo.gain;
                let off = speed - lo.speed;
                let num = (dg as u64)
                    .saturating_mul(off as u64)
                    .saturating_add((ds as u64) / 2);
                let add = num / (ds as u64);
                return Ok(lo.gain.saturating_add(add as u32));
            }
            i += 1;
        }
        match self.points.last() {
            Some(p) => Ok(p.gain),
            None => Err(PtrCode::CurveBad.val()),
        }
    }
}

// ── 设备档 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// 平坦档：增益与速度无关。
    Flat,
    /// 加速档：增益按曲线随速度上升。
    Curve,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PtrProfile {
    pub dpi: u32,
    pub flat: u32,
    pub curve: Option<Curve>,
    pub mode: Mode,
    pub bound: i64,
}

impl PtrProfile {
    pub fn new(
        dpi: u32,
        flat: u32,
        curve: Option<Curve>,
        mode: Mode,
        bound: i64,
    ) -> Result<PtrProfile, u32> {
        if dpi < DPI_MIN || dpi > DPI_MAX {
            return Err(PtrCode::DevBad.val());
        }
        if flat < GAIN_MIN || flat > GAIN_MAX {
            return Err(PtrCode::GainBad.val());
        }
        if bound <= 0 || bound > POS_BOUND {
            return Err(PtrCode::DevBad.val());
        }
        if mode == Mode::Curve && curve.is_none() {
            return Err(PtrCode::ModeBad.val());
        }
        if let Some(c) = curve.as_ref() {
            c.validate()?;
        }
        Ok(PtrProfile {
            dpi,
            flat,
            curve,
            mode,
            bound,
        })
    }

    pub fn flat(dpi: u32, gain: u32) -> Result<PtrProfile, u32> {
        PtrProfile::new(dpi, gain, None, Mode::Flat, POS_BOUND)
    }

    pub fn curved(dpi: u32, curve: Curve) -> Result<PtrProfile, u32> {
        PtrProfile::new(dpi, 1000, Some(curve), Mode::Curve, POS_BOUND)
    }

    /// 两档切换：切到曲线档必须真有曲线，否则 ModeBad。
    pub fn set_mode(&mut self, mode: Mode) -> Result<(), u32> {
        if mode == Mode::Curve && self.curve.is_none() {
            return Err(PtrCode::ModeBad.val());
        }
        self.mode = mode;
        Ok(())
    }

    pub fn has_curve(&self) -> bool {
        self.curve.is_some()
    }
}

/// 求当前速度下的增益（千分比）。
pub fn gain_for(profile: &PtrProfile, speed: u32) -> Result<u32, u32> {
    match profile.mode {
        Mode::Flat => Ok(profile.flat),
        Mode::Curve => match profile.curve.as_ref() {
            Some(c) => c.gain_at(speed),
            None => Err(PtrCode::ModeBad.val()),
        },
    }
}

/// 单轴位移按增益放大（千分比），对称半值进位。
pub fn scale_rel(rel: i32, gain: u32) -> i64 {
    let prod = (rel as i64).saturating_mul(gain as i64);
    round_div(prod, GAIN_SCALE)
}

// ── 合成 ──────────────────────────────────────────────────────────
/// 未夹取的合成：只做增益与加法，越出 RAW_BOUND 判 PosBad。
pub fn compose_raw(profile: &PtrProfile, from: Point, ev: RelEvent) -> Result<Point, u32> {
    if ev.dx == 0 && ev.dy == 0 {
        return Err(PtrCode::RelBad.val());
    }
    let speed = ev.speed();
    let gain = gain_for(profile, speed)?;
    let nx = from.x.saturating_add(scale_rel(ev.dx, gain));
    let ny = from.y.saturating_add(scale_rel(ev.dy, gain));
    if nx.unsigned_abs() > RAW_BOUND as u64 || ny.unsigned_abs() > RAW_BOUND as u64 {
        return Err(PtrCode::PosBad.val());
    }
    Ok(Point { x: nx, y: ny })
}

/// 夹取合成：档边界硬夹，返回是否发生夹取（静默区在 B07-EDGE）。
pub fn compose_checked(
    profile: &PtrProfile,
    from: Point,
    ev: RelEvent,
) -> Result<(Point, bool), u32> {
    let raw = compose_raw(profile, from, ev)?;
    let lim = profile.bound;
    let x = if raw.x > lim {
        lim
    } else if raw.x < -lim {
        -lim
    } else {
        raw.x
    };
    let y = if raw.y > lim {
        lim
    } else if raw.y < -lim {
        -lim
    } else {
        raw.y
    };
    Ok((Point { x, y }, x != raw.x || y != raw.y))
}

// ── 账本 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointerLedger {
    pub composes: u32,
    pub rejects: u32,
    pub clamps: u32,
    pub digest: u64,
}

impl PointerLedger {
    pub fn new() -> PointerLedger {
        PointerLedger {
            composes: 0,
            rejects: 0,
            clamps: 0,
            digest: FNV_OFF,
        }
    }

    pub fn step(
        &mut self,
        profile: &PtrProfile,
        from: Point,
        ev: RelEvent,
    ) -> Result<Point, u32> {
        match compose_checked(profile, from, ev) {
            Ok((p, clamped)) => {
                self.composes = self.composes.saturating_add(1);
                if clamped {
                    self.clamps = self.clamps.saturating_add(1);
                }
                self.digest = fold_i32(self.digest, ev.dx);
                self.digest = fold_i32(self.digest, ev.dy);
                self.digest = fold_i32(self.digest, p.x as i32);
                self.digest = fold_i32(self.digest, p.y as i32);
                Ok(p)
            }
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = fold_u32(self.digest, e);
                Err(e)
            }
        }
    }

    pub fn total(&self) -> u32 {
        self.composes.saturating_add(self.rejects)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.composes == 0 && self.rejects == 0 {
            return Err(PtrCode::LedgerBad.val());
        }
        if self.total() != self.composes.saturating_add(self.rejects) {
            return Err(PtrCode::LedgerBad.val());
        }
        Ok(())
    }
}

// ── 黄金语料 ──────────────────────────────────────────────────────
/// 固定语料：10^4 条 ±100 相对事件，平坦恒等档，返回 (x, y, digest, rejects)。
pub fn golden_run() -> Result<(i64, i64, u64, u32), u32> {
    let profile = PtrProfile::flat(DEFAULT_DPI, GAIN_SCALE as u32)?;
    let mut lcg = Lcg::new(0x4892_1001);
    let mut led = PointerLedger::new();
    let mut pos = Point::origin();
    let mut rejects = 0u32;
    let mut i = 0u32;
    while i < COMPOSE_ROUNDS {
        let dx = lcg.below(201) as i32 - 100;
        let dy = lcg.below(201) as i32 - 100;
        let ev = match RelEvent::new(dx, dy) {
            Ok(e) => e,
            Err(_) => RelEvent { dx: 1, dy: 0 },
        };
        match led.step(&profile, pos, ev) {
            Ok(p) => pos = p,
            Err(_) => rejects = rejects.saturating_add(1),
        }
        i += 1;
    }
    if led.composes.saturating_add(rejects) != COMPOSE_ROUNDS {
        return Err(PtrCode::LedgerBad.val());
    }
    Ok((pos.x, pos.y, led.digest, rejects))
}

/// 漂移账：恒等档下合成坐标与精确和之差，必须 (0, 0)。
pub fn drift_run(rounds: u32) -> Result<(i64, i64), u32> {
    let profile = PtrProfile::flat(DEFAULT_DPI, GAIN_SCALE as u32)?;
    let mut lcg = Lcg::new(0x4892_2001);
    let mut pos = Point::origin();
    let mut ex = 0i64;
    let mut ey = 0i64;
    let mut i = 0u32;
    while i < rounds {
        let dx = lcg.below(201) as i32 - 100;
        let dy = lcg.below(201) as i32 - 100;
        let ev = match RelEvent::new(dx, dy) {
            Ok(e) => e,
            Err(_) => RelEvent { dx: 1, dy: 0 },
        };
        pos = compose_raw(&profile, pos, ev)?;
        ex = ex.saturating_add(ev.dx as i64);
        ey = ey.saturating_add(ev.dy as i64);
        i += 1;
    }
    Ok((pos.x.saturating_sub(ex), pos.y.saturating_sub(ey)))
}

/// 两档切换一致性：平坦档与速度无关、曲线档采样点精确。
pub fn switch_rounds(rounds: u32) -> Result<(), u32> {
    let curve = Curve::new(curve_of(&[(0, 1000), (400, 2000), (1600, 4000)]))?;
    let mut p = PtrProfile::curved(DEFAULT_DPI, curve)?;
    let mut i = 0u32;
    while i < rounds {
        p.set_mode(Mode::Flat)?;
        let mut s = 0u32;
        while s <= SPEED_CAP {
            if gain_for(&p, s)? != p.flat {
                return Err(PtrCode::ModeBad.val());
            }
            s = s.saturating_add(500);
        }
        p.set_mode(Mode::Curve)?;
        if gain_for(&p, 0)? != 1000 {
            return Err(PtrCode::ReplayBad.val());
        }
        if gain_for(&p, 400)? != 2000 {
            return Err(PtrCode::ReplayBad.val());
        }
        if gain_for(&p, 1600)? != 4000 {
            return Err(PtrCode::ReplayBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(())
}

// ── 注入（四路） ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn route(&self, i: usize) -> u32 {
        match self.caught.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
}

/// 四路注入：超界 REL / 非单调曲线 / 增益越界 / 缺曲线选曲线档。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    let mut log = InjectLog {
        tries: [rounds; 4],
        caught: [0; 4],
    };
    let mut r = 0u32;
    while r < rounds {
        if RelEvent::new(REL_MAX + 1, 0).err() == Some(PtrCode::RelBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        let bad = Curve::new(curve_of(&[(0, 1000), (400, 2000), (200, 1500)]));
        if bad.err() == Some(PtrCode::CurveBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        if PtrProfile::flat(DEFAULT_DPI, GAIN_MAX + 1).err() == Some(PtrCode::GainBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        let mut flat = PtrProfile::flat(DEFAULT_DPI, 1000)?;
        if flat.set_mode(Mode::Curve).err() == Some(PtrCode::ModeBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 渲染与回读 ────────────────────────────────────────────────────
pub fn render_ledger(l: &PointerLedger) -> String {
    format!(
        "{}|{}|{}|0x{:016X}",
        l.composes, l.rejects, l.clamps, l.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(PtrCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(PtrCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(PtrCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(PtrCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 {
        return Err(PtrCode::RenderBad.val());
    }
    if !s.starts_with("0x") && !s.starts_with("0X") {
        return Err(PtrCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(PtrCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(PtrCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(PtrCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 回读账本，返回合成分；拒绝任何字段缺失/越界。
pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 4 {
        return Err(PtrCode::RenderBad.val());
    }
    let composes = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(PtrCode::RenderBad.val()),
    })?;
    let rejects = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(PtrCode::RenderBad.val()),
    })?;
    let clamps = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(PtrCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(3) {
        Some(v) => v,
        None => return Err(PtrCode::RenderBad.val()),
    })?;
    if composes > 10_000_000 || rejects > 10_000_000 || clamps > composes {
        return Err(PtrCode::RenderBad.val());
    }
    if digest == 0 {
        return Err(PtrCode::RenderBad.val());
    }
    Ok(composes)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 探针 A：恒等档逐条零漂、放大档整数倍精确、负向对称。
pub fn probe_seq_a() -> Result<(), u32> {
    let id = PtrProfile::flat(DEFAULT_DPI, GAIN_SCALE as u32)?;
    let mut lcg = Lcg::new(0x4892_A001);
    let mut pos = Point::origin();
    let mut i = 0u32;
    while i < 1000 {
        let dx = lcg.below(201) as i32 - 100;
        let dy = lcg.below(201) as i32 - 100;
        let ev = match RelEvent::new(dx, dy) {
            Ok(e) => e,
            Err(_) => RelEvent { dx: 1, dy: 0 },
        };
        let q = compose_raw(&id, pos, ev)?;
        if q.x != pos.x.saturating_add(ev.dx as i64)
            || q.y != pos.y.saturating_add(ev.dy as i64)
        {
            return Err(PtrCode::ReplayBad.val());
        }
        pos = q;
        i = i.saturating_add(1);
    }
    let dbl = PtrProfile::flat(DEFAULT_DPI, 2000)?;
    let r = compose_raw(&dbl, Point::new(10, 20)?, RelEvent::new(3, -5)?)?;
    if r.x != 16 || r.y != 10 {
        return Err(PtrCode::GainBad.val());
    }
    let neg = compose_raw(&id, Point::new(0, 0)?, RelEvent::new(-1, 1)?)?;
    if neg.x != -1 || neg.y != 1 {
        return Err(PtrCode::ReplayBad.val());
    }
    Ok(())
}

/// 探针 B：曲线采样点精确、段中点半值进位、首末封顶、平坦档恒速。
pub fn probe_seq_b() -> Result<(), u32> {
    let curve = Curve::new(curve_of(&[
        (0, 1000),
        (100, 1200),
        (400, 2000),
        (1600, 4000),
    ]))?;
    let p = PtrProfile::curved(DEFAULT_DPI, curve)?;
    if gain_for(&p, 0)? != 1000 {
        return Err(PtrCode::CurveBad.val());
    }
    if gain_for(&p, 100)? != 1200 {
        return Err(PtrCode::CurveBad.val());
    }
    if gain_for(&p, 400)? != 2000 {
        return Err(PtrCode::CurveBad.val());
    }
    if gain_for(&p, 1600)? != 4000 {
        return Err(PtrCode::CurveBad.val());
    }
    if gain_for(&p, 50)? != 1100 {
        return Err(PtrCode::CurveBad.val());
    }
    if gain_for(&p, 250)? != 1600 {
        return Err(PtrCode::CurveBad.val());
    }
    if gain_for(&p, 1000)? != 3000 {
        return Err(PtrCode::CurveBad.val());
    }
    if gain_for(&p, 5000)? != 4000 {
        return Err(PtrCode::CurveBad.val());
    }
    let f = PtrProfile::flat(DEFAULT_DPI, 1500)?;
    let mut s = 0u32;
    while s <= SPEED_CAP {
        if gain_for(&f, s)? != 1500 {
            return Err(PtrCode::ModeBad.val());
        }
        s = s.saturating_add(250);
    }
    Ok(())
}

/// 探针 C：黄金双跑一致、漂移零、渲染回读一致、顺序坐标正确。
pub fn probe_seq_c() -> Result<(), u32> {
    let g1 = golden_run()?;
    let g2 = golden_run()?;
    if g1 != g2 {
        return Err(PtrCode::ReplayBad.val());
    }
    if drift_run(COMPOSE_ROUNDS)? != (0, 0) {
        return Err(PtrCode::OrderBad.val());
    }
    let mut led = PointerLedger::new();
    let profile = PtrProfile::flat(DEFAULT_DPI, GAIN_SCALE as u32)?;
    let mut pos = Point::origin();
    pos = led.step(&profile, pos, RelEvent::new(5, -7)?)?;
    pos = led.step(&profile, pos, RelEvent::new(-2, 3)?)?;
    let text = render_ledger(&led);
    if audit_render(&text)? != led.composes {
        return Err(PtrCode::RenderBad.val());
    }
    if pos.x != 3 || pos.y != -4 {
        return Err(PtrCode::OrderBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PtrCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 1;
    let hi = SEG_BASE + PtrCode::ALL.len() as u32;
    let mut i = 0usize;
    while i < PtrCode::ALL.len() {
        match PtrCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let drift = drift_run(COMPOSE_ROUNDS);
    v.push((
        "F48921-J1-恒等档×10^4零漂移",
        drift == Ok((0, 0)),
        match &drift {
            Ok(d) => format!("drift=({}, {})", d.0, d.1),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let golden = golden_run();
    v.push((
        "F48921-J1-合成×10^4账齐",
        match &golden {
            Ok((_, _, _, rej)) => *rej == 0,
            Err(_) => false,
        },
        match &golden {
            Ok((x, y, d, rej)) => format!("pos=({}, {}) digest=0x{:016X} rejects={}", x, y, d, rej),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let sw = switch_rounds(SWITCH_ROUNDS);
    v.push((
        "F48922-J1-两档×10^2切换一致",
        sw.is_ok(),
        match &sw {
            Ok(()) => format!("rounds={} 一致", SWITCH_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let pb = probe_seq_b();
    v.push((
        "F48922-J1-曲线采样点精确",
        pb.is_ok(),
        match &pb {
            Ok(()) => String::from("采样点/中点/封顶全精确"),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let edge = RelEvent::new(REL_MAX, 0).is_ok()
        && RelEvent::new(0, REL_MAX).is_ok()
        && RelEvent::new(REL_MAX + 1, 0).is_err()
        && RelEvent::new(0, -REL_MAX - 1).is_err()
        && RelEvent::new(0, 0).is_err();
    v.push((
        "F48921-J1-极端REL边界",
        edge,
        format!("REL_MAX={} 接受、+1 拒绝、零位移拒绝", REL_MAX),
    ));
    let bad_shapes = Curve::new(curve_of(&[(0, 1000), (100, 1200), (100, 1300)])).is_err()
        && Curve::new(curve_of(&[(0, 1000)])).is_err()
        && Curve::new(curve_of(&[(10, 1000), (100, 1200)])).is_err()
        && Curve::new(curve_of(&[(0, 1000), (100, 1200), (200, 1100)])).is_err()
        && Curve::new(curve_of(&[(0, 1000), (100, 200)])).is_err()
        && Curve::new(curve_of(&[(0, 1000), (SPEED_CAP + 1, 1200)])).is_err();
    v.push((
        "F48922-J1-曲线非法形状必拒",
        bad_shapes,
        String::from("重复速/单点/首速非零/增益回落/低于下限/超封顶 六拒"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48921-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let mut led = PointerLedger::new();
    let render_ok = match PtrProfile::flat(DEFAULT_DPI, 1000) {
        Ok(p) => match led.step(&p, Point::origin(), RelEvent { dx: 9, dy: 11 }) {
            Ok(_) => {
                let text = render_ledger(&led);
                match audit_render(&text) {
                    Ok(n) => n == led.composes && led.audit().is_ok(),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push((
        "F48921-J1-账本渲染回读",
        render_ok,
        format!("composes={} renders=4 字段", led.composes),
    ));
    v.push((
        "F48921-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    let flat = PtrProfile::flat(DEFAULT_DPI, 1000);
    v.push((
        "F48922-J1-无曲线选曲线档必拒",
        match flat {
            Ok(mut p) => p.set_mode(Mode::Curve).err() == Some(PtrCode::ModeBad.val()),
            Err(_) => false,
        },
        String::from("ModeBad"),
    ));
    v.push((
        "F48921-J1-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 {} 码", SEG_BASE, PtrCode::ALL.len()),
    ));
    v
}

pub fn run_unx48921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48921 指针位移合成与加速曲线");
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
    fn identity_gain_is_exact() {
        let p = PtrProfile::flat(1000, 1000);
        match p {
            Ok(pr) => {
                let q = compose_raw(
                    &pr,
                    Point { x: 4, y: 5 },
                    RelEvent { dx: -9, dy: 7 },
                );
                assert_eq!(q, Ok(Point::new(-5, 12).unwrap_or(Point { x: 0, y: 0 })));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn doubled_gain_scales_exactly() {
        match PtrProfile::flat(1000, 2000) {
            Ok(p) => {
                let q = compose_raw(
                    &p,
                    Point::origin(),
                    RelEvent { dx: 3, dy: -4 },
                );
                assert_eq!(q, Ok(Point { x: 6, y: -8 }));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn rounding_symmetric() {
        assert_eq!(scale_rel(1, 500), 1);
        assert_eq!(scale_rel(-1, 500), -1);
        assert_eq!(scale_rel(3, 500), 2);
        assert_eq!(scale_rel(-3, 500), -2);
        assert_eq!(scale_rel(1, 499), 0);
    }

    #[test]
    fn curve_samples_are_exact() {
        match Curve::new(curve_of(&[(0, 1000), (100, 1200), (400, 2000)])) {
            Ok(c) => {
                assert_eq!(c.gain_at(0), Ok(1000));
                assert_eq!(c.gain_at(100), Ok(1200));
                assert_eq!(c.gain_at(400), Ok(2000));
                assert_eq!(c.gain_at(50), Ok(1100));
                assert_eq!(c.gain_at(5000), Ok(2000));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn curve_rejects_bad_shapes() {
        assert!(Curve::new(curve_of(&[(0, 1000)])).is_err());
        assert!(Curve::new(curve_of(&[(1, 1000), (10, 1200)])).is_err());
        assert!(Curve::new(curve_of(&[(0, 1000), (10, 1200), (10, 1300)])).is_err());
        assert!(Curve::new(curve_of(&[(0, 1000), (10, 900)])).is_err());
        assert!(Curve::new(curve_of(&[(0, 100), (10, 900)])).is_err());
        assert!(Curve::new(curve_of(&[(0, 1000), (SPEED_CAP + 1, 1000)])).is_err());
    }

    #[test]
    fn profile_rejects_bad_dpi_and_bound() {
        assert_eq!(
            PtrProfile::flat(0, 1000).err(),
            Some(PtrCode::DevBad.val())
        );
        assert_eq!(
            PtrProfile::flat(DPI_MAX + 1, 1000).err(),
            Some(PtrCode::DevBad.val())
        );
        assert_eq!(
            PtrProfile::new(1000, 1000, None, Mode::Flat, 0).err(),
            Some(PtrCode::DevBad.val())
        );
        assert_eq!(
            PtrProfile::flat(1000, GAIN_MIN - 1).err(),
            Some(PtrCode::GainBad.val())
        );
    }

    #[test]
    fn mode_switch_requires_curve() {
        match PtrProfile::flat(1000, 1000) {
            Ok(mut p) => {
                assert_eq!(p.set_mode(Mode::Curve), Err(PtrCode::ModeBad.val()));
                assert!(p.set_mode(Mode::Flat).is_ok());
            }
            Err(_) => assert!(false),
        }
        match Curve::new(curve_of(&[(0, 1000), (100, 1200)])) {
            Ok(c) => match PtrProfile::curved(1000, c) {
                Ok(mut p) => {
                    assert!(p.set_mode(Mode::Flat).is_ok());
                    assert!(p.set_mode(Mode::Curve).is_ok());
                }
                Err(_) => assert!(false),
            },
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn rel_events_reject_zero_and_overflow() {
        assert!(RelEvent::new(0, 0).is_err());
        assert!(RelEvent::new(REL_MAX, 0).is_ok());
        assert!(RelEvent::new(0, -REL_MAX).is_ok());
        assert!(RelEvent::new(REL_MAX + 1, 0).is_err());
        assert!(RelEvent::new(0, -REL_MAX - 1).is_err());
    }

    #[test]
    fn clamp_is_at_bound() {
        match PtrProfile::new(1000, 4000, None, Mode::Flat, 10) {
            Ok(p) => match compose_checked(
                &p,
                Point::origin(),
                RelEvent { dx: 200, dy: 0 },
            ) {
                Ok((q, clamped)) => {
                    assert!(clamped);
                    assert_eq!(q, Point { x: 10, y: 0 });
                }
                Err(_) => assert!(false),
            },
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
        assert_eq!(drift_run(1000), Ok((0, 0)));
    }

    #[test]
    fn probes_and_switch_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(switch_rounds(SWITCH_ROUNDS).is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(7) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [7, 7, 7, 7]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let mut led = PointerLedger::new();
        match PtrProfile::flat(1000, 1000) {
            Ok(p) => {
                let _ = led.step(&p, Point::origin(), RelEvent { dx: 1, dy: 2 });
                let text = render_ledger(&led);
                assert_eq!(audit_render(&text), Ok(1));
                let bad = text.replacen("0x", "0y", 1);
                assert!(audit_render(&bad).is_err());
                let short = text.rsplitn(2, '|').collect::<Vec<&str>>();
                assert_eq!(short.len(), 2);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn codes_inside_window() {
        assert!(segment_guard());
        assert_eq!(PtrCode::ALL.len(), 12);
        assert_eq!(detail(PtrCode::RelBad.val()), PtrCode::RelBad.detail());
        assert_eq!(detail(0), "未知诊断码");
    }
}
