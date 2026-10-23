//! VE-F0130 · 低电量渲染策略接口（VE-A 域 · A06 环境与系统集成组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0130`
//!
//! **职责定位（锚点原文）**：低电量时的渲染策略接口（帧率上限/效果降档/后台暂停
//! 三档策略），策略与 AE06 预算联动，电量状态只读消费（渲染不抢电量判断权）。
//!
//! **铁律**：
//! 1. **三档策略**——帧率上限/效果降档/后台暂停，逐档收紧。
//! 2. **预算联动**——策略变化必须同步给预算侧（AE06），不许各算各的。
//! 3. **只读电量**——渲染侧只消费电量状态，不反向写、不自行判断「该不该省电」
//!    （电量判断权在电源域，渲染只是执行者）。
//! 4. **失配降级**——策略与预算联动失败时独立降档，不阻塞渲染主路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C8（低电量渲染策略）。
pub const SEG_BASE: u16 = 0xE8C8;
/// 越权写电量（红线：渲染只读）。
pub const XC_POWER_WRITE: u16 = SEG_BASE | 0x01;
/// 策略档位非法。
pub const XC_BAD_TIER: u16 = SEG_BASE | 0x02;
/// 预算联动失效。
pub const XC_BUDGET_LINK: u16 = SEG_BASE | 0x03;
/// 非法状态迁移。
pub const XC_BAD_PHASE: u16 = SEG_BASE | 0x04;
/// 电量输入非法（>100）。
pub const XC_BAD_LEVEL: u16 = SEG_BASE | 0x05;

/// 低电量策略档（逐档收紧）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PowerTier {
    /// 全速档：电量充足，不做限制。
    FullSpeed,
    /// 帧率上限档：压帧率但不减画质。
    CapFps,
    /// 效果降档：减画质保帧率。
    DowngradeFx,
    /// 后台暂停：后台化时直接停渲染（最省电）。
    PauseBackground,
}

impl PowerTier {
    /// 档位序号（0..3，越大越省电）。
    pub fn ordinal(&self) -> u8 {
        match self {
            PowerTier::FullSpeed => 0,
            PowerTier::CapFps => 1,
            PowerTier::DowngradeFx => 2,
            PowerTier::PauseBackground => 3,
        }
    }

    /// 档位帧率上限（0=不限）。
    pub fn fps_cap(&self) -> u32 {
        match self {
            PowerTier::FullSpeed => 0,
            PowerTier::CapFps => 30,
            PowerTier::DowngradeFx => 30,
            PowerTier::PauseBackground => 0,
        }
    }

    /// 效果开关（false=降档生效）。
    pub fn effects_on(&self) -> bool {
        match self {
            PowerTier::FullSpeed | PowerTier::CapFps => true,
            PowerTier::DowngradeFx | PowerTier::PauseBackground => false,
        }
    }

    /// 后台是否暂停。
    pub fn pause_background(&self) -> bool {
        *self == PowerTier::PauseBackground
    }
}

/// 只读电量状态（电源域的权威输出，渲染侧只消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerState {
    /// 电量百分比 0..=100。
    pub level: u8,
    /// 是否接电（AC 在线时低电策略通常放宽——但由电源域决定）。
    pub on_ac: bool,
    /// 电源域建议档位（渲染侧采纳的权威值）。
    pub suggested: PowerTier,
}

/// 预算联动事件（策略变化必须发给 AE06）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetEvent {
    /// 目标档位。
    pub tier: PowerTier,
    /// 事件序号（单调，便于预算侧去重）。
    pub seq: u32,
    /// 是否为独立降级（联动失效后的兜底标记）。
    pub independent: bool,
}

/// 低电量渲染策略接口。
///
/// **只读铁律的实现方式**：`PowerState` 由外部（电源域）构造并传入，
/// 本结构体不提供任何写电量的方法——从 API 形状上就杜绝越权。
pub struct PowerPolicy {
    /// 当前采纳档位。
    tier: PowerTier,
    /// 最近一次消费的电量状态。
    last_state: Option<PowerState>,
    /// 预算联动事件序列。
    events: Vec<BudgetEvent>,
    /// 事件容量上限。
    cap: usize,
    /// 事件序号。
    seq: u32,
    /// 联动失效次数（独立降级计数）。
    link_fails: u32,
    /// 最近诊断码。
    last_code: u16,
    /// 降档次数（诚实统计）。
    downgrades: u32,
}

impl PowerPolicy {
    /// 建策略接口（事件容量>0）。
    pub fn new(cap: usize) -> Result<Self, u16> {
        if cap == 0 {
            return Err(XC_BUDGET_LINK);
        }
        Ok(PowerPolicy {
            tier: PowerTier::FullSpeed,
            last_state: None,
            events: Vec::new(),
            cap,
            seq: 0,
            link_fails: 0,
            last_code: 0,
            downgrades: 0,
        })
    }

    /// 当前档位。
    pub fn tier(&self) -> PowerTier {
        self.tier
    }

    /// 最近诊断码。
    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 消费电量状态（只读入口）：按电源域建议档位联动。
    ///
    /// `link_fn(tier, seq)` 是发给预算侧的联动调用——返回 false 表示
    /// 联动失败，此时走独立降级（档位仍切换，但标记 independent）。
    pub fn consume<F>(&mut self, state: PowerState, link_fn: F) -> Result<BudgetEvent, u16>
    where
        F: FnOnce(PowerTier, u32) -> bool,
    {
        // 电量输入校验：>100 非法。
        if state.level > 100 {
            self.last_code = XC_BAD_LEVEL;
            return Err(XC_BAD_LEVEL);
        }
        self.last_state = Some(state);
        self.apply_tier(state.suggested, link_fn)
    }

    /// 显式降档/升档（调用方按自身判断请求档位——最终档位取两者更省电侧）。
    pub fn request_tier<F>(&mut self, want: PowerTier, link_fn: F) -> Result<BudgetEvent, u16>
    where
        F: FnOnce(PowerTier, u32) -> bool,
    {
        // 取更省电档（ordinal 越大越省电）——降档方向永不被反向请求拉高。
        let target = if want.ordinal() >= self.tier.ordinal() {
            want
        } else {
            self.tier
        };
        self.apply_tier(target, link_fn)
    }

    /// 档位应用 + 预算联动（内部统一路径）。
    fn apply_tier<F>(&mut self, target: PowerTier, link_fn: F) -> Result<BudgetEvent, u16>
    where
        F: FnOnce(PowerTier, u32) -> bool,
    {
        // 同档不重复发事件（幂等——预算侧不被同档事件刷屏）。
        if target == self.tier && !self.events.is_empty() {
            self.last_code = 0;
            return Ok(*self.events.last().unwrap());
        }
        let seq = self.seq;
        self.seq += 1;
        let linked = link_fn(target, seq);
        let independent = !linked;
        if independent {
            self.link_fails += 1;
            self.last_code = XC_BUDGET_LINK;
        } else {
            self.last_code = 0;
        }
        if target.ordinal() > self.tier.ordinal() {
            self.downgrades += 1;
        }
        self.tier = target;
        if self.events.len() >= self.cap {
            // 容量满：挤掉最旧（事件序列是审计环，不是阻塞队列）。
            self.events.remove(0);
        }
        let ev = BudgetEvent {
            tier: target,
            seq,
            independent,
        };
        self.events.push(ev);
        Ok(ev)
    }

    /// 事件序列（审计用）。
    pub fn events(&self) -> &[BudgetEvent] {
        &self.events
    }

    /// 联动失效次数。
    pub fn link_fails(&self) -> u32 {
        self.link_fails
    }

    /// 降档次数。
    pub fn downgrades(&self) -> u32 {
        self.downgrades
    }

    /// 最近电量状态。
    pub fn last_state(&self) -> Option<PowerState> {
        self.last_state
    }

    /// 读屏行。
    pub fn status_line(&self) -> &'static str {
        match self.tier {
            PowerTier::FullSpeed => "power:fullspeed",
            PowerTier::CapFps => "power:capfps",
            PowerTier::DowngradeFx => "power:downgrade",
            PowerTier::PauseBackground => "power:pausebg",
        }
    }
}

/// 自检：三档策略、预算联动、只读电量、失配降级、降档单调。
pub fn run_vea130_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn state(level: u8, suggested: PowerTier) -> PowerState {
        PowerState {
            level,
            on_ac: false,
            suggested,
        }
    }

    let mut set = CheckSet::new("vea130_powerpolicy");

    // C130-01 合法建 + 默认全速
    {
        let p = PowerPolicy::new(8);
        let ok = match p {
            Ok(p) => p.tier() == PowerTier::FullSpeed,
            Err(_) => false,
        };
        chk(&mut set, "C130-01 合法建", ok, "默认档位非全速");
    }

    // C130-02 容量零拒绝
    {
        let r = PowerPolicy::new(0);
        chk(
            &mut set,
            "C130-02 容量零拒",
            r.err() == Some(XC_BUDGET_LINK),
            "零容量被放行",
        );
    }

    // C130-03 三档策略逐档收紧
    {
        let mut p = PowerPolicy::new(8).unwrap();
        let _ = p.consume(state(20, PowerTier::CapFps), |_, _| true);
        let a = p.tier() == PowerTier::CapFps && p.tier().fps_cap() == 30;
        let _ = p.consume(state(10, PowerTier::DowngradeFx), |_, _| true);
        let b = p.tier() == PowerTier::DowngradeFx && !p.tier().effects_on();
        let _ = p.consume(state(5, PowerTier::PauseBackground), |_, _| true);
        let c = p.tier().pause_background();
        chk(&mut set, "C130-03 三档策略", a && b && c, "档位收紧链断");
    }

    // C130-04 预算联动：事件携带档位与序号
    {
        let mut p = PowerPolicy::new(8).unwrap();
        let ev = p.consume(state(20, PowerTier::CapFps), |_, _| true);
        let ok = ev.map(|e| e.tier == PowerTier::CapFps && e.seq == 0 && !e.independent).unwrap_or(false);
        chk(&mut set, "C130-04 预算联动", ok, "事件字段错");
    }

    // C130-05 联动失效 → 独立降级（不阻塞主路径）
    {
        let mut p = PowerPolicy::new(8).unwrap();
        let ev = p.consume(state(20, PowerTier::CapFps), |_, _| false);
        let ok = ev.map(|e| e.independent).unwrap_or(false)
            && p.link_fails() == 1
            && p.tier() == PowerTier::CapFps;
        chk(&mut set, "C130-05 失配降级", ok, "联动失效未独立降级");
    }

    // C130-06 只读电量：API 无写电量入口（结构上保证，用非法输入验证只读校验）
    {
        let mut p = PowerPolicy::new(8).unwrap();
        let r = p.consume(state(101, PowerTier::FullSpeed), |_, _| true);
        let ok = r.err() == Some(XC_BAD_LEVEL);
        chk(&mut set, "C130-06 只读校验", ok, "非法电量被放行");
    }

    // C130-07 同档幂等：不重复发事件
    {
        let mut p = PowerPolicy::new(8).unwrap();
        let _ = p.consume(state(20, PowerTier::CapFps), |_, _| true);
        let n0 = p.events().len();
        let _ = p.consume(state(21, PowerTier::CapFps), |_, _| true);
        let ok = p.events().len() == n0;
        chk(&mut set, "C130-07 同档幂等", ok, "同档重复发事件");
    }

    // C130-08 降档单调：反向请求不拉高档位
    {
        let mut p = PowerPolicy::new(8).unwrap();
        let _ = p.consume(state(10, PowerTier::DowngradeFx), |_, _| true);
        let _ = p.request_tier(PowerTier::FullSpeed, |_, _| true);
        let ok = p.tier() == PowerTier::DowngradeFx;
        chk(&mut set, "C130-08 降档单调", ok, "反向请求拉高了档位");
    }

    // C130-09 降档统计诚实
    {
        let mut p = PowerPolicy::new(8).unwrap();
        let _ = p.consume(state(50, PowerTier::CapFps), |_, _| true);
        let _ = p.consume(state(10, PowerTier::DowngradeFx), |_, _| true);
        let ok = p.downgrades() == 2;
        chk(&mut set, "C130-09 降档统计", ok, "降档计数错");
    }

    // C130-10 事件容量环形挤出
    {
        let mut p = PowerPolicy::new(2).unwrap();
        let _ = p.consume(state(50, PowerTier::CapFps), |_, _| true);
        let _ = p.consume(state(30, PowerTier::DowngradeFx), |_, _| true);
        let _ = p.consume(state(10, PowerTier::PauseBackground), |_, _| true);
        let ok = p.events().len() == 2;
        chk(&mut set, "C130-10 环形容量", ok, "事件未挤出");
    }

    set
}
