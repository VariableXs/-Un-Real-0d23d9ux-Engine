//! VE-F3643 · 动态壁纸创作（VE-S 域 · 批次 R03 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3643`
//!
//! **职责定位（锚点原文）**：时间轴动画壁纸——
//! - **时间轴编辑**：关键帧 + 缓动（复用 F3005/F3015 动效模型跨域单源）；
//! - **性能约束**：动态壁纸常驻 GPU → 功耗红线（F3128 复述执法）：
//!   动态壁纸必须过功耗门（静态功耗预算，超门→降帧提示）；
//! - **帧率档**：60/30/按需三档（复述分档）；
//! - **暂停语义**：全屏应用时自动暂停（自动暂停红线：盖住时还在动 = 耗电白烧）。
//!
//! **判据（锚点原文六条）**：动效单源、功耗门、自动暂停、三档帧率、
//! 盖住红线、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 功耗破门→P1+降帧（红线实测） | [`W43Code::POWER_BREAK`] |
//! | 盖住仍动→P1（暂停红线实测） | [`W43Code::COVERED_STILL_MOVING`] |
//! | 动效分歧→单源修正（复述） | [`W43Code::MOTION_DIVERGED`] |
//! | 帧率失真→校准（复述） | [`W43Code::FPS_DRIFT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3643 诊断码段（独占 0x5Bxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct W43Code(pub u16);

impl W43Code {
    /// 功耗破门（P1 + 降帧）。
    pub const POWER_BREAK: W43Code = W43Code(0x5B01);
    /// 盖住仍动（暂停红线 P1）。
    pub const COVERED_STILL_MOVING: W43Code = W43Code(0x5B02);
    /// 动效模型分歧（单源修正）。
    pub const MOTION_DIVERGED: W43Code = W43Code(0x5B03);
    /// 帧率失真。
    pub const FPS_DRIFT: W43Code = W43Code(0x5B04);
    /// 关键帧时间非法。
    pub const BAD_KEYFRAME: W43Code = W43Code(0x5B05);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            W43Code::POWER_BREAK => "动态壁纸功耗破门（P1）。建议：降帧至下一档。",
            W43Code::COVERED_STILL_MOVING => "盖住仍动（暂停红线 P1）。建议：全屏时立即暂停。",
            W43Code::MOTION_DIVERGED => "动效模型分歧。建议：回归 F3005/F3015 单源。",
            W43Code::FPS_DRIFT => "帧率失真。建议：按档位校准。",
            W43Code::BAD_KEYFRAME => "关键帧时间非法。建议：时间须单调递增。",
            _ => "未知诊断码。",
        }
    }
}

// ── 缓动与关键帧（复用 F3005/F3015 口径） ───────────────────────────

/// 缓动曲线（动效单源子集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Easing {
    /// 线性。
    Linear,
    /// 缓入。
    EaseIn,
    /// 缓出。
    EaseOut,
    /// 缓入缓出。
    EaseInOut,
}

/// 关键帧。
#[derive(Clone, Copy)]
pub struct Keyframe {
    /// 时间（毫秒，须单调）。
    pub t_ms: u32,
    /// 值（0..=10000）。
    pub value_bps: u32,
    pub easing: Easing,
}

// ── 帧率档 ──────────────────────────────────────────────────────────

/// 帧率三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpsTier {
    /// 60 fps。
    High,
    /// 30 fps。
    Mid,
    /// 按需。
    OnDemand,
}

impl FpsTier {
    /// 名义帧率。
    pub fn nominal_fps(self) -> u32 {
        match self {
            FpsTier::High => 60,
            FpsTier::Mid => 30,
            FpsTier::OnDemand => 10,
        }
    }

    /// 降一档（功耗破门降帧）。
    pub fn step_down(self) -> FpsTier {
        match self {
            FpsTier::High => FpsTier::Mid,
            FpsTier::Mid => FpsTier::OnDemand,
            FpsTier::OnDemand => FpsTier::OnDemand,
        }
    }
}

// ── 功耗门 ──────────────────────────────────────────────────────────

/// 功耗门：静态功耗预算（毫瓦）。
#[derive(Clone, Copy)]
pub struct PowerGate {
    pub budget_mw: u32,
    pub breaks: u32,
}

impl PowerGate {
    /// 是否破门。
    pub fn check(&mut self, actual_mw: u32) -> bool {
        if actual_mw > self.budget_mw {
            self.breaks += 1;
            false
        } else {
            true
        }
    }
}

// ── 暂停检测器 ──────────────────────────────────────────────────────

/// 全屏遮挡状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cover {
    /// 无遮挡。
    Exposed,
    /// 全屏应用覆盖。
    Covered,
}

// ── 动态壁纸时间轴 ──────────────────────────────────────────────────

/// 动态壁纸创作时间轴。
pub struct MotionWallpaper {
    keys: Vec<Keyframe>,
    gate: PowerGate,
    tier: FpsTier,
    cover: Cover,
    paused: bool,
    moving_ticks: u32,
    source_rev: u32,
    last_code: W43Code,
    has_error: bool,
}

impl MotionWallpaper {
    /// 新建；`budget_mw` 为功耗预算，`source_rev` 为动效单源版本。
    pub fn new(budget_mw: u32, source_rev: u32) -> Self {
        Self {
            keys: Vec::new(),
            gate: PowerGate { budget_mw, breaks: 0 },
            tier: FpsTier::High,
            cover: Cover::Exposed,
            paused: false,
            moving_ticks: 0,
            source_rev,
            last_code: W43Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> W43Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: W43Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 追加关键帧；时间须严格递增、值在界内。
    pub fn add_key(&mut self, t_ms: u32, value_bps: u32, easing: Easing) -> Result<usize, W43Code> {
        if value_bps > 10_000 {
            self.fail(W43Code::BAD_KEYFRAME);
            return Err(W43Code::BAD_KEYFRAME);
        }
        if let Some(last) = self.keys.last() {
            if t_ms <= last.t_ms {
                self.fail(W43Code::BAD_KEYFRAME);
                return Err(W43Code::BAD_KEYFRAME);
            }
        }
        self.keys.push(Keyframe { t_ms, value_bps, easing });
        self.last_code = W43Code(0);
        self.has_error = false;
        Ok(self.keys.len())
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// 动效单源核验（F3005/F3015 对齐）。
    pub fn verify_motion_source(&mut self, rev: u32) -> Result<bool, W43Code> {
        if rev != self.source_rev {
            self.fail(W43Code::MOTION_DIVERGED);
            return Err(W43Code::MOTION_DIVERGED);
        }
        self.last_code = W43Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 功耗采样：破门自动降帧。
    pub fn sample_power(&mut self, actual_mw: u32) -> Result<FpsTier, W43Code> {
        if self.gate.check(actual_mw) {
            self.last_code = W43Code(0);
            self.has_error = false;
            Ok(self.tier)
        } else {
            self.tier = self.tier.step_down();
            self.fail(W43Code::POWER_BREAK);
            Err(W43Code::POWER_BREAK)
        }
    }

    pub fn power_breaks(&self) -> u32 {
        self.gate.breaks
    }

    /// 设帧率档；与名义帧率对不上判失真。
    pub fn set_tier(&mut self, tier: FpsTier, claimed_fps: u32) -> Result<u32, W43Code> {
        if claimed_fps != tier.nominal_fps() {
            self.fail(W43Code::FPS_DRIFT);
            return Err(W43Code::FPS_DRIFT);
        }
        self.tier = tier;
        self.last_code = W43Code(0);
        self.has_error = false;
        Ok(claimed_fps)
    }

    pub fn tier(&self) -> FpsTier {
        self.tier
    }

    /// 遮挡事件：Covered 必须自动暂停（盖住红线）。
    pub fn on_cover(&mut self, cover: Cover) -> Result<bool, W43Code> {
        self.cover = cover;
        if cover == Cover::Covered {
            self.paused = true;
        }
        self.last_code = W43Code(0);
        self.has_error = false;
        Ok(self.paused)
    }

    /// 推进一拍；盖住或暂停时仍计动 → 红线。
    pub fn tick(&mut self) -> Result<u32, W43Code> {
        if self.cover == Cover::Covered || self.paused {
            self.fail(W43Code::COVERED_STILL_MOVING);
            return Err(W43Code::COVERED_STILL_MOVING);
        }
        self.moving_ticks += 1;
        self.last_code = W43Code(0);
        self.has_error = false;
        Ok(self.moving_ticks)
    }

    /// 恢复播放（须先解除遮挡）。
    pub fn resume(&mut self) -> Result<bool, W43Code> {
        if self.cover == Cover::Covered {
            self.fail(W43Code::COVERED_STILL_MOVING);
            return Err(W43Code::COVERED_STILL_MOVING);
        }
        self.paused = false;
        self.last_code = W43Code(0);
        self.has_error = false;
        Ok(true)
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn moving_ticks(&self) -> u32 {
        self.moving_ticks
    }

    /// 暂停语义总评：盖住即停、暴露可续。
    pub fn pause_semantics_green(&self) -> bool {
        !(self.cover == Cover::Covered && !self.paused)
    }

    /// 时间轴单调性复核。
    pub fn timeline_monotonic(&self) -> bool {
        self.keys.windows(2).all(|w| w[0].t_ms < w[1].t_ms)
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves43_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves43_motionwall");
    {
        let m = MotionWallpaper::new(500, 1);
        chk(&mut set, "C3643-01", m.key_count() == 0 && m.tier() == FpsTier::High, "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        let r = m.add_key(0, 0, Easing::Linear);
        chk(&mut set, "C3643-02", r == Ok(1), "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        m.add_key(100, 0, Easing::Linear);
        let r = m.add_key(100, 1, Easing::Linear);
        chk(&mut set, "C3643-03", r.err() == Some(W43Code::BAD_KEYFRAME), "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        let r = m.add_key(0, 20_000, Easing::Linear);
        chk(&mut set, "C3643-04", r.err() == Some(W43Code::BAD_KEYFRAME), "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 7);
        let r = m.verify_motion_source(7);
        chk(&mut set, "C3643-05", r == Ok(true), "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 7);
        let r = m.verify_motion_source(8);
        chk(&mut set, "C3643-06", r.err() == Some(W43Code::MOTION_DIVERGED), "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        let r = m.sample_power(400);
        chk(&mut set, "C3643-07", r == Ok(FpsTier::High) && m.power_breaks() == 0, "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        let r = m.sample_power(800);
        chk(&mut set, "C3643-08", r.err() == Some(W43Code::POWER_BREAK) && m.tier() == FpsTier::Mid, "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        let r = m.set_tier(FpsTier::Mid, 60);
        chk(&mut set, "C3643-09", r.err() == Some(W43Code::FPS_DRIFT), "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        m.on_cover(Cover::Covered);
        let r = m.tick();
        chk(&mut set, "C3643-10", r.err() == Some(W43Code::COVERED_STILL_MOVING), "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        m.on_cover(Cover::Covered);
        chk(&mut set, "C3643-11", m.is_paused() && m.pause_semantics_green(), "err");
    }
    {
        let mut m = MotionWallpaper::new(500, 1);
        m.add_key(0, 0, Easing::Linear);
        m.add_key(100, 100, Easing::EaseIn);
        m.add_key(200, 200, Easing::EaseOut);
        chk(&mut set, "C3643-12", m.timeline_monotonic() && m.key_count() == 3, "err");
    }
    set
}
