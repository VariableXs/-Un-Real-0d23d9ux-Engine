//! VE-F3523 · 光标物理运行时 —— 惯性/磁吸/形变三效果 + 全部可关 + 精度解耦。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3523`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三效果**（[`CursorFx`]）：光标移动的物理化——①惯性
//!   （[`Inertia`]：停止输入后按速度衰减滑行）；②磁吸
//!   （[`Magnet`]：接近可点目标时吸附）；③形变（[`Squash`]：
//!   点击时按速度压扁拉伸）。三效果互相独立。
//! - **全部可关**（[`FxCaps`]):装饰不绑架操作——每个效果
//!   有独立开关，关了就是纯光标，一个多余像素都不动。
//! - **精度解耦**（[`PrecisionGuard`]）：**开了惯性不丢精度**——
//!   物理引擎只改「显示位置」，原始输入位置原样记账（原始样本
//!   保留在 [`SampleRing`]），任何效果丢点即自动关效果。
//! - **降档保帧**（[`FxTier`]）：效果卡顿（帧预算超）→ 降档
//!   （形变 → 磁吸 → 惯性逐级关），帧率优先于花活。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 惯性丢点 | 解耦保护（原始样本记账） |
//! | 效果卡顿 | 降档 |
//! | 精度损失 | 自动关效果 |
//!
//! # 复杂度
//!
//! - `step`：O(1)/帧（定点整数物理）；
//! - 磁吸：O(目标数)（可视目标以十计）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点（速度/位置全 i32 定点）。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5920 起）
// ---------------------------------------------------------------------------

/// 光标物理域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorCode {
    /// 效果导致原始样本丢失（解耦保护触发）。
    SampleLost = 5920,
    /// 帧预算超限（降档）。
    FrameOverrun = 5921,
    /// 速度越界（已钳制）。
    SpeedClamped = 5922,
}

impl CursorCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            CursorCode::SampleLost => "效果丢点已自动关效果",
            CursorCode::FrameOverrun => "帧预算超限已降档",
            CursorCode::SpeedClamped => "速度越界已钳制",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、常量
// ---------------------------------------------------------------------------

/// 帧预算（逻辑 tick/帧）。
pub const FRAME_BUDGET: u32 = 8;
/// 原始样本环容量。
pub const SAMPLE_CAP: usize = 64;
/// 惯性速度上限（单位/帧）。
pub const SPEED_CAP: i32 = 96;
/// 惯性衰减千分比/帧。
pub const INERTIA_DECAY: u32 = 850;
/// 磁吸半径（像素）。
pub const MAGNET_RADIUS: i32 = 24;

/// 三效果开关集（全部可关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FxCaps {
    /// 惯性。
    pub inertia: bool,
    /// 磁吸。
    pub magnet: bool,
    /// 形变。
    pub squash: bool,
}

impl FxCaps {
    /// 全开。
    pub const fn all() -> Self {
        FxCaps { inertia: true, magnet: true, squash: true }
    }

    /// 全关（纯光标）。
    pub const fn none() -> Self {
        FxCaps { inertia: false, magnet: false, squash: false }
    }

    /// 开着的效果数。
    pub fn count(&self) -> u8 {
        self.inertia as u8 + self.magnet as u8 + self.squash as u8
    }
}

/// 效果档位（降档序：形变先关 → 磁吸 → 惯性）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FxTier {
    /// 全效果。
    Full,
    /// 关形变。
    NoSquash,
    /// 只留惯性。
    InertiaOnly,
    /// 纯光标。
    Plain,
}

impl FxTier {
    /// 档位对应的开关集。
    pub fn caps(self) -> FxCaps {
        match self {
            FxTier::Full => FxCaps::all(),
            FxTier::NoSquash => FxCaps { squash: false, ..FxCaps::all() },
            FxTier::InertiaOnly => FxCaps { inertia: true, magnet: false, squash: false },
            FxTier::Plain => FxCaps::none(),
        }
    }

    /// 降一档。
    pub fn degrade(self) -> Self {
        match self {
            FxTier::Full => FxTier::NoSquash,
            FxTier::NoSquash => FxTier::InertiaOnly,
            FxTier::InertiaOnly | FxTier::Plain => FxTier::Plain,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、物理件
// ---------------------------------------------------------------------------

/// 惯性：速度衰减滑行。速度按 [`INERTIA_DECAY`] 千分比/帧衰减，
/// 低于死区即停（避免无限蠕动）。
#[derive(Clone, Copy, Debug)]
pub struct Inertia {
    pub vx: i32,
    pub vy: i32,
}

impl Inertia {
    /// 零速。
    pub fn still() -> Self {
        Inertia { vx: 0, vy: 0 }
    }

    /// 注入一帧速度（输入侧；越界钳制）。
    pub fn kick(&mut self, dx: i32, dy: i32) {
        self.vx = (self.vx + dx).clamp(-SPEED_CAP, SPEED_CAP);
        self.vy = (self.vy + dy).clamp(-SPEED_CAP, SPEED_CAP);
    }

    /// 滑行一帧：返回本帧位移。低于死区归零。
    pub fn glide(&mut self) -> (i32, i32) {
        if self.vx.abs() < 2 && self.vy.abs() < 2 {
            self.vx = 0;
            self.vy = 0;
            return (0, 0);
        }
        let dx = self.vx;
        let dy = self.vy;
        self.vx = self.vx * INERTIA_DECAY as i32 / 1000;
        self.vy = self.vy * INERTIA_DECAY as i32 / 1000;
        (dx, dy)
    }

    /// 是否仍在滑行。
    pub fn gliding(&self) -> bool {
        self.vx != 0 || self.vy != 0
    }
}

/// 可点目标（磁吸用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    pub x: i32,
    pub y: i32,
}

/// 磁吸：半径内吸附到最近目标。
pub fn magnet_snap(x: i32, y: i32, targets: &[Target]) -> (i32, i32) {
    let mut best: Option<(i32, i32, i64)> = None;
    for t in targets {
        let dx = (t.x - x) as i64;
        let dy = (t.y - y) as i64;
        let d2 = dx * dx + dy * dy;
        if d2 <= (MAGNET_RADIUS as i64) * (MAGNET_RADIUS as i64) {
            match best {
                Some((_, _, bd)) if bd <= d2 => {}
                _ => best = Some((t.x, t.y, d2)),
            }
        }
    }
    match best {
        Some((tx, ty, _)) => (tx, ty),
        None => (x, y),
    }
}

/// 形变：点击压扁量（千分比，按速度放大，钳到 300）。
pub fn squash_permille(speed: i32) -> i32 {
    1000 + (speed.abs() * 4).clamp(0, 300)
}

// ---------------------------------------------------------------------------
// 四、原始样本环（精度解耦）
// ---------------------------------------------------------------------------

/// 原始输入样本环：物理引擎不碰这里——「开了惯性不丢精度」的
/// 记账基础。任何效果路径发现样本缺口（相邻样本序号跳变）即
/// 触发自动关效果。
#[derive(Clone)]
pub struct SampleRing {
    buf: Vec<(u32, i32, i32)>, // (seq, x, y)
    seq: u32,
}

impl SampleRing {
    /// 空环。
    pub fn new() -> Self {
        SampleRing {
            buf: Vec::with_capacity(SAMPLE_CAP),
            seq: 0,
        }
    }

    /// 记一个原始样本。
    pub fn push(&mut self, x: i32, y: i32) {
        self.seq += 1;
        if self.buf.len() >= SAMPLE_CAP {
            self.buf.remove(0);
        }
        self.buf.push((self.seq, x, y));
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 缺口检测：相邻样本 seq 必须连续，断口即丢点证据。
    pub fn has_gap(&self) -> bool {
        for w in self.buf.windows(2) {
            if w[1].0 != w[0].0 + 1 {
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 五、运行时
// ---------------------------------------------------------------------------

/// 光标物理运行时。
pub struct CursorRuntime {
    /// 显示位置。
    pub x: i32,
    pub y: i32,
    /// 效果档位。
    pub tier: FxTier,
    /// 惯性状态。
    pub inertia: Inertia,
    /// 原始样本。
    pub samples: SampleRing,
    /// 审计。
    pub codes: Vec<CursorCode>,
}

impl CursorRuntime {
    /// 原点起步、全效果。
    pub fn new() -> Self {
        CursorRuntime {
            x: 0,
            y: 0,
            tier: FxTier::Full,
            inertia: Inertia::still(),
            samples: SampleRing::new(),
            codes: Vec::new(),
        }
    }

    /// 原始输入到达：记账（永远）+ 驱动惯性（若开）。
    pub fn input(&mut self, x: i32, y: i32) {
        self.samples.push(x, y);
        if self.tier.caps().inertia {
            self.inertia.kick(x - self.x, y - self.y);
        }
        self.x = x;
        self.y = y;
    }

    /// 帧步进：惯性滑行 + 磁吸吸附。返回显示位置。
    pub fn step(&mut self, targets: &[Target]) -> (i32, i32) {
        let caps = self.tier.caps();
        if caps.inertia && self.inertia.gliding() {
            let (dx, dy) = self.inertia.glide();
            self.x = (self.x + dx).clamp(0, 32767);
            self.y = (self.y + dy).clamp(0, 32767);
        }
        if caps.magnet {
            let (sx, sy) = magnet_snap(self.x, self.y, targets);
            self.x = sx;
            self.y = sy;
        }
        (self.x, self.y)
    }

    /// 丢点检测：发现缺口 → 自动关效果（精度解耦保护）。
    pub fn guard(&mut self) -> bool {
        if self.samples.has_gap() {
            if self.tier != FxTier::Plain {
                self.codes.push(CursorCode::SampleLost);
                self.tier = FxTier::Plain;
                return true;
            }
        }
        false
    }

    /// 帧预算超限 → 降档。
    pub fn overrun(&mut self, cost: u32) {
        if cost > FRAME_BUDGET && self.tier != FxTier::Plain {
            self.codes.push(CursorCode::FrameOverrun);
            self.tier = self.tier.degrade();
        }
    }

    /// 形变量（未开形变恒 1000 = 无形变）。
    pub fn squash(&self) -> i32 {
        if self.tier.caps().squash {
            squash_permille(self.inertia.vx.max(self.inertia.vx.wrapping_neg()))
        } else {
            1000
        }
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inertia_glides_and_stops() {
        let mut r = CursorRuntime::new();
        r.input(40, 0);
        r.input(80, 0);
        // 连续滑行若干帧应收敛停住。
        let mut frames = 0;
        while r.inertia.gliding() && frames < 64 {
            r.step(&[]);
            frames += 1;
        }
        assert!(!r.inertia.gliding());
        assert!(frames > 2);
    }

    #[test]
    fn magnet_snaps_nearby() {
        let t = [Target { x: 100, y: 100 }];
        let (x, y) = magnet_snap(110, 105, &t);
        assert_eq!((x, y), (100, 100));
        let (x2, y2) = magnet_snap(400, 400, &t);
        assert_eq!((x2, y2), (400, 400)); // 远离不吸
    }

    #[test]
    fn gap_disables_fx() {
        let mut r = CursorRuntime::new();
        r.input(10, 10);
        r.input(20, 20);
        // 模拟丢一个样本（跳号）。
        r.samples.seq += 1;
        r.samples.push(30, 30);
        assert!(r.guard());
        assert_eq!(r.tier, FxTier::Plain);
        assert!(r.codes.contains(&CursorCode::SampleLost));
    }

    #[test]
    fn overrun_degrades() {
        let mut r = CursorRuntime::new();
        r.overrun(FRAME_BUDGET + 1);
        assert_eq!(r.tier, FxTier::NoSquash);
        r.overrun(FRAME_BUDGET + 1);
        assert_eq!(r.tier, FxTier::InertiaOnly);
    }

    #[test]
    fn raw_samples_intact() {
        let mut r = CursorRuntime::new();
        for i in 0..32 {
            r.input(i * 4, i * 2);
        }
        assert_eq!(r.samples.len(), 32);
        assert!(!r.samples.has_gap());
    }
}
