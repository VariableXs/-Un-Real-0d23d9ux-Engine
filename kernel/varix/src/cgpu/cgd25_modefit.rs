//! cgd25_modefit — CGPU-F0505 呈现模式适配（三策略集/无缝迁移/合成器配合/探测复用）。
//!
//! 判据原文（施工书锚点 CGPU-F0505）：模式适配：三模式策略集（全屏独占/
//! 无边框窗口/系统合成器各自的均衡与缓冲策略——模式差异显性化）；模式切
//! 换（窗口模式变化→策略无缝迁移）；合成器交互（与 DWM 等系统合成器的配
//! 合语义——排队帧数的适配）；适配探测（当前模式自动识别复用 F0486）。
//! 判据：三策略、无缝迁移、合成器配合、探测复用、判据。
//!
//! 模型：三模式各持一份（目标缓冲深度/合成器排队帧上限/切换容差）；切换
//! 时在帧边界迁移——迁移期一帧内旧策略收尾新策略接管（零丢失）；合成器
//! 模式排队帧上限受系统约束钳制；探测接口复用 F0486 语义（0=独占 1=无边
//! 框 2=合成器，未知值拒）。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 诊断码段 0x9700（cgpu 域内唯一）。
pub const XC_MODE_UNKNOWN: u32 = 0x9701;
pub const XC_MODE_POLICY_MISSING: u32 = 0x9702;
pub const XC_MODE_GLITCH: u32 = 0x9703;
pub const XC_MODE_QUEUE_CLAMP: u32 = 0x9704;
pub const XC_MODE_MIDFRAME: u32 = 0x9705;

/// 三呈现模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentMode {
    /// 全屏独占。
    Exclusive = 0,
    /// 无边框窗口。
    Borderless = 1,
    /// 系统合成器。
    Composited = 2,
}

impl PresentMode {
    /// F0486 探测值→模式（2 与未知值区分：2 合法，≥3 未知）。
    pub fn from_probe(v: u32) -> Result<Self, u32> {
        match v {
            0 => Ok(PresentMode::Exclusive),
            1 => Ok(PresentMode::Borderless),
            2 => Ok(PresentMode::Composited),
            _ => Err(XC_MODE_UNKNOWN),
        }
    }
}

/// 单模式策略集。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModePolicy {
    /// 目标缓冲深度（帧）。
    pub depth: u8,
    /// 合成器排队帧上限（独占=0 表不适用）。
    pub queue_cap: u8,
    /// 切换容差（帧）。
    pub switch_tol: u8,
}

impl ModePolicy {
    /// 独占默认：无排队上限。
    pub const EXCLUSIVE: ModePolicy = ModePolicy {
        depth: 1,
        queue_cap: 0,
        switch_tol: 2,
    };
    /// 无边框默认。
    pub const BORDERLESS: ModePolicy = ModePolicy {
        depth: 2,
        queue_cap: 1,
        switch_tol: 3,
    };
    /// 合成器默认。
    pub const COMPOSITED: ModePolicy = ModePolicy {
        depth: 3,
        queue_cap: 2,
        switch_tol: 4,
    };

    /// 合法性：深度 1..=4、容差 1..=8、排队上限 ≤3。
    /// （const fn 里 RangeInclusive::contains 尚未稳定——显式比较等价。）
    pub const fn is_valid(&self) -> bool {
        let tol = switch_tol_raw(self);
        self.depth >= 1 && self.depth <= 4 && tol >= 1 && tol <= 8 && self.queue_cap <= 3
    }
}

const fn switch_tol_raw(p: &ModePolicy) -> u8 {
    p.switch_tol
}

/// 合成器排队钳制：上限受系统配额约束。
pub fn clamp_queue(p: &ModePolicy, sys_quota: u8) -> Result<u8, u32> {
    if p.queue_cap == 0 {
        // 独占不适用——原样返回 0。
        return Ok(0);
    }
    if sys_quota == 0 {
        return Err(XC_MODE_QUEUE_CLAMP);
    }
    Ok(p.queue_cap.min(sys_quota))
}

/// 迁移事件留痕。
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    /// 起点。
    pub from: PresentMode,
    /// 终点。
    pub to: PresentMode,
    /// 迁移帧边界（帧号）。
    pub frame: u64,
    /// 迁移期丢弃帧数（须为 0）。
    pub dropped: u32,
}

/// 模式适配器。
#[derive(Debug)]
pub struct ModeFitter {
    cur: PresentMode,
    frame: u64,
    /// 迁移账。
    pub trail: Vec<Migration>,
    /// 各模式策略登记（三份齐才可切换）。
    pub policies: Vec<(PresentMode, ModePolicy)>,
    /// 迁移中标记（迁移占一帧）。
    migrating: bool,
}

impl ModeFitter {
    /// 构造并登记三默认策略集。
    pub fn new() -> Self {
        let mut policies = Vec::new();
        policies.push((PresentMode::Exclusive, ModePolicy::EXCLUSIVE));
        policies.push((PresentMode::Borderless, ModePolicy::BORDERLESS));
        policies.push((PresentMode::Composited, ModePolicy::COMPOSITED));
        ModeFitter {
            cur: PresentMode::Composited,
            frame: 0,
            trail: Vec::new(),
            policies,
            migrating: false,
        }
    }

    /// 当前模式。
    pub fn mode(&self) -> PresentMode {
        self.cur
    }

    /// 当前帧号。
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// 查策略：未登记即拒。
    pub fn policy_of(&self, m: PresentMode) -> Result<ModePolicy, u32> {
        let mut i = 0;
        while i < self.policies.len() {
            if self.policies[i].0 == m {
                return Ok(self.policies[i].1);
            }
            i += 1;
        }
        Err(XC_MODE_POLICY_MISSING)
    }

    /// 探测接管：F0486 值→若与当前不同则触发迁移。
    pub fn probe_apply(&mut self, probe: u32) -> Result<Option<Migration>, u32> {
        let m = PresentMode::from_probe(probe)?;
        if m == self.cur && !self.migrating {
            return Ok(None);
        }
        self.switch_to(m)
    }

    /// 显式切换：帧边界迁移，迁移期一帧（零丢失）；帧中切换拒。
    pub fn switch_to(&mut self, to: PresentMode) -> Result<Option<Migration>, u32> {
        // 迁移须在帧边界：migrating 标志在 advance 后清除——此处若上一次
        // 迁移未收尾即拒。
        if self.migrating {
            return Err(XC_MODE_MIDFRAME);
        }
        if to == self.cur {
            return Ok(None);
        }
        // 目标策略必须已登记。
        let _ = self.policy_of(to)?;
        let m = Migration {
            from: self.cur,
            to,
            frame: self.frame,
            dropped: 0,
        };
        self.trail.push(m);
        self.cur = to;
        self.migrating = true;
        Ok(Some(m))
    }

    /// 帧推进：迁移帧过后迁移结束。
    pub fn advance(&mut self) {
        self.frame += 1;
        self.migrating = false;
    }

    /// 无缝对账：迁移账每条 dropped==0 且相邻迁移之间帧号严格递增。
    pub fn trail_seamless(&self) -> bool {
        let mut i = 0;
        while i < self.trail.len() {
            if self.trail[i].dropped != 0 {
                return false;
            }
            if i > 0 && self.trail[i].frame <= self.trail[i - 1].frame {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for ModeFitter {
    fn default() -> Self {
        Self::new()
    }
}

/// 模式适配总门面。
#[derive(Debug, Default)]
pub struct ModeEngine {
    /// 适配器。
    pub fitter: ModeFitter,
}

impl ModeEngine {
    /// 自检一：三策略集显性化——三默认互异且合法、缺登记拒。
    pub fn selfcheck_policies(&self) -> Result<(), u32> {
        let e = ModePolicy::EXCLUSIVE;
        let b = ModePolicy::BORDERLESS;
        let c = ModePolicy::COMPOSITED;
        if e == b || b == c || e == c {
            return Err(XC_MODE_POLICY_MISSING);
        }
        if !e.is_valid() || !b.is_valid() || !c.is_valid() {
            return Err(XC_MODE_POLICY_MISSING);
        }
        // 深度递增、排队上限递增（差异显性）。
        if !(e.depth < b.depth && b.depth < c.depth) {
            return Err(XC_MODE_POLICY_MISSING);
        }
        if !(e.queue_cap < b.queue_cap && b.queue_cap < c.queue_cap) {
            return Err(XC_MODE_POLICY_MISSING);
        }
        // 去掉一份登记后切换拒。
        let mut f = ModeFitter::new();
        f.policies.retain(|(m, _)| *m != PresentMode::Exclusive);
        if f.switch_to(PresentMode::Exclusive).is_ok() {
            return Err(XC_MODE_POLICY_MISSING);
        }
        Ok(())
    }

    /// 自检二：无缝迁移——探测驱动切换、迁移帧收尾、账目零丢失。
    pub fn selfcheck_migration(&mut self) -> Result<(), u32> {
        let mut f = ModeFitter::new();
        if f.mode() != PresentMode::Composited {
            return Err(XC_MODE_POLICY_MISSING);
        }
        // 同模式探测→无迁移。
        if f.probe_apply(2)?.is_some() {
            return Err(XC_MODE_GLITCH);
        }
        // 探测 0→独占。
        let m = f.probe_apply(0)?.ok_or(XC_MODE_GLITCH)?;
        if m.from != PresentMode::Composited || m.to != PresentMode::Exclusive || m.dropped != 0 {
            return Err(XC_MODE_GLITCH);
        }
        if f.mode() != PresentMode::Exclusive {
            return Err(XC_MODE_GLITCH);
        }
        // 迁移未收尾再切→拒（帧中语义）。
        if f.switch_to(PresentMode::Borderless).is_ok() {
            return Err(XC_MODE_MIDFRAME);
        }
        f.advance();
        if f.switch_to(PresentMode::Borderless)?.is_none() {
            return Err(XC_MODE_GLITCH);
        }
        f.advance();
        if !f.trail_seamless() || f.trail.len() != 2 {
            return Err(XC_MODE_GLITCH);
        }
        Ok(())
    }

    /// 自检三：合成器配合——排队钳制、配额 0 拒、独占不受限。
    pub fn selfcheck_compositor(&self) -> Result<(), u32> {
        // 配额 1：无边框 1→1；合成器 2→1。
        if clamp_queue(&ModePolicy::BORDERLESS, 1)? != 1 {
            return Err(XC_MODE_QUEUE_CLAMP);
        }
        if clamp_queue(&ModePolicy::COMPOSITED, 1)? != 1 {
            return Err(XC_MODE_QUEUE_CLAMP);
        }
        // 配额 3：不钳。
        if clamp_queue(&ModePolicy::COMPOSITED, 3)? != 2 {
            return Err(XC_MODE_QUEUE_CLAMP);
        }
        // 配额 0 拒。
        if clamp_queue(&ModePolicy::COMPOSITED, 0).is_ok() {
            return Err(XC_MODE_QUEUE_CLAMP);
        }
        // 独占不受限（cap=0）。
        if clamp_queue(&ModePolicy::EXCLUSIVE, 0)? != 0 {
            return Err(XC_MODE_QUEUE_CLAMP);
        }
        Ok(())
    }

    /// 自检四：探测复用——F0486 值域三分、未知值拒。
    pub fn selfcheck_probe(&self) -> Result<(), u32> {
        if PresentMode::from_probe(0)? != PresentMode::Exclusive
            || PresentMode::from_probe(1)? != PresentMode::Borderless
            || PresentMode::from_probe(2)? != PresentMode::Composited
        {
            return Err(XC_MODE_UNKNOWN);
        }
        let mut v = 3;
        while v < 8 {
            if PresentMode::from_probe(v).is_ok() {
                return Err(XC_MODE_UNKNOWN);
            }
            v += 1;
        }
        Ok(())
    }

    /// 自检五：总账收口——策略/迁移/合成器/探测四线贯通。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut f = ModeFitter::new();
        // 合成器→无边框→独占 全链零丢失。
        let _ = f.probe_apply(1)?;
        f.advance();
        let _ = f.probe_apply(0)?;
        f.advance();
        if f.trail.len() != 2 || !f.trail_seamless() {
            return Err(XC_MODE_GLITCH);
        }
        // 迁移终点策略排队钳制一致性。
        let p = f.policy_of(f.mode())?;
        if clamp_queue(&p, 4)? != p.queue_cap {
            return Err(XC_MODE_QUEUE_CLAMP);
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9700..0x9800 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_MODE_UNKNOWN,
            XC_MODE_POLICY_MISSING,
            XC_MODE_GLITCH,
            XC_MODE_QUEUE_CLAMP,
            XC_MODE_MIDFRAME,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9700 || codes[i] >= 0x9800 {
                return Err(codes[i]);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(codes[i]);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgd25_selfchecks() {
        let mut e = ModeEngine::default();
        e.fitter.advance(); // 清初始迁移位。
        e.selfcheck_policies().unwrap();
        e.selfcheck_migration().unwrap();
        e.selfcheck_compositor().unwrap();
        e.selfcheck_probe().unwrap();
        e.selfcheck_verdict().unwrap();
        assert_eq!(ModeEngine::segment_guard(), Ok(()));
    }
}
