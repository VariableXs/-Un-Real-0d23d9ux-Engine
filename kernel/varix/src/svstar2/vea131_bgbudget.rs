//! VE-F0131 · 后台执行预算联动（VE-A 域 · A06 环境与系统集成组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0131`
//!
//! **职责定位（锚点原文）**：应用后台化时的渲染预算联动（后台降帧/暂停渲染两档），
//! 后台的合规承诺（不偷偷渲染耗电），回前台恢复爬回；含后台渲染的功耗实测
//! （合规承诺的验证数据）。
//!
//! **铁律**：
//! 1. **两档后台**——降帧（10fps）/暂停（0fps）两档，明确区分。
//! 2. **偷渲红线**——后台化后仍有渲染提交即红线检测（合规承诺被打破）。
//! 3. **爬回恢复**——回前台不瞬间全速，按爬回曲线逐步恢复（防帧率尖峰）。
//! 4. **合规承诺**——功耗实测数据可查（承诺有验证数据支撑，不是口号）。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C9（后台执行预算联动）。
pub const SEG_BASE: u16 = 0xE8C9;
/// 偷渲红线（后台化后仍有渲染提交）。
pub const XC_STEAL_RENDER: u16 = SEG_BASE | 0x01;
/// 非法档位。
pub const XC_BAD_TIER: u16 = SEG_BASE | 0x02;
/// 恢复失败（爬回中断走重初始化）。
pub const XC_RAMP_FAIL: u16 = SEG_BASE | 0x03;
/// 非法状态迁移。
pub const XC_BAD_PHASE: u16 = SEG_BASE | 0x04;
/// 联动失效。
pub const XC_LINK_FAIL: u16 = SEG_BASE | 0x05;

/// 后台执行档（两档，锚点原文铁律）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BgTier {
    /// 降帧档：后台保持 10fps（通知横幅等仍可见）。
    Throttle,
    /// 暂停档：0fps（完全静默，最省电）。
    Pause,
}

impl BgTier {
    /// 档位帧率上限。
    pub fn fps_cap(&self) -> u32 {
        match self {
            BgTier::Throttle => 10,
            BgTier::Pause => 0,
        }
    }
}

/// 前后台相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppPhase {
    /// 前台。
    Foreground,
    /// 后台。
    Background,
    /// 爬回中（回前台后的渐进恢复）。
    Ramping,
}

/// 渲染提交记录（偷渲检测的证据单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RenderSubmit {
    /// 逻辑帧号。
    pub frame: u32,
    /// 提交时刻（毫秒）。
    pub tick_ms: u64,
    /// 提交时相位（与记录比对，事后审计）。
    pub phase: AppPhase,
}

/// 功耗实测样本（合规承诺的验证数据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerSample {
    /// 采样窗口（毫秒）。
    pub window_ms: u32,
    /// 窗口内渲染帧数。
    pub frames: u32,
    /// 实测功耗（毫瓦，由电源侧供给——渲染侧只记录）。
    pub mw: u32,
}

/// 后台执行预算联动器。
///
/// **偷渲红线的实现**：`submit()` 在后台相位调用时立即拒绝并累计违规——
/// 红线不是「记一笔下次再说」，是当场阻断 + 显性计数（合规承诺零容忍）。
pub struct BgBudget {
    /// 当前相位。
    phase: AppPhase,
    /// 后台档位。
    bg_tier: BgTier,
    /// 爬回总步数（回前台后逐步拉回帧率）。
    ramp_steps: u32,
    /// 当前爬回步。
    ramp_step: u32,
    /// 偷渲违规累计。
    steal_hits: u32,
    /// 合法提交累计（前台+爬回期）。
    ok_submits: u32,
    /// 功耗实测样本（合规验证数据）。
    samples: Vec<PowerSample>,
    /// 样本容量。
    sample_cap: usize,
    /// 最近诊断码。
    last_code: u16,
    /// 联动失效次数。
    link_fails: u32,
}

impl BgBudget {
    /// 建联动器（爬回步数>0，样本容量>0）。
    pub fn new(ramp_steps: u32, sample_cap: usize) -> Result<Self, u16> {
        if ramp_steps == 0 || sample_cap == 0 {
            return Err(XC_BAD_TIER);
        }
        Ok(BgBudget {
            phase: AppPhase::Foreground,
            bg_tier: BgTier::Throttle,
            ramp_steps,
            ramp_step: 0,
            steal_hits: 0,
            ok_submits: 0,
            samples: Vec::new(),
            sample_cap,
            last_code: 0,
            link_fails: 0,
        })
    }

    /// 当前相位。
    pub fn phase(&self) -> AppPhase {
        self.phase
    }

    /// 当前后台档位。
    pub fn bg_tier(&self) -> BgTier {
        self.bg_tier
    }

    /// 最近诊断码。
    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 进入后台（选档：降帧/暂停）。
    ///
    /// `link_fn(tier, fps)` 是发给预算侧的联动调用——返回 false 表示联动
    /// 失效，此时仍切换档位（独立执行），但计一次 link_fails。
    pub fn to_background<F>(&mut self, tier: BgTier, link_fn: F) -> Result<(), u16>
    where
        F: FnOnce(BgTier, u32) -> bool,
    {
        match self.phase {
            AppPhase::Foreground | AppPhase::Ramping => {
                self.phase = AppPhase::Background;
                self.bg_tier = tier;
                self.ramp_step = 0;
                let linked = link_fn(tier, tier.fps_cap());
                if !linked {
                    self.link_fails += 1;
                    self.last_code = XC_LINK_FAIL;
                } else {
                    self.last_code = 0;
                }
                Ok(())
            }
            AppPhase::Background => {
                // 已在后台：允许换档（同相位内的档位调整合法）。
                self.bg_tier = tier;
                let linked = link_fn(tier, tier.fps_cap());
                if !linked {
                    self.link_fails += 1;
                    self.last_code = XC_LINK_FAIL;
                } else {
                    self.last_code = 0;
                }
                Ok(())
            }
        }
    }

    /// 回前台：进入爬回相位（不瞬间全速——防帧率尖峰）。
    pub fn to_foreground(&mut self) -> Result<(), u16> {
        match self.phase {
            AppPhase::Background => {
                self.phase = AppPhase::Ramping;
                self.ramp_step = 0;
                self.last_code = 0;
                Ok(())
            }
            _ => {
                self.last_code = XC_BAD_PHASE;
                Err(XC_BAD_PHASE)
            }
        }
    }

    /// 爬回推进一步（调用方按帧调用）。
    ///
    /// 返回当前步的帧率上限（线性爬回：从后台档 fps 到 0（不限））。
    /// 全部步走完自动转 Foreground。
    pub fn ramp_tick(&mut self) -> Result<u32, u16> {
        if self.phase != AppPhase::Ramping {
            self.last_code = XC_BAD_PHASE;
            return Err(XC_BAD_PHASE);
        }
        self.ramp_step += 1;
        if self.ramp_step >= self.ramp_steps {
            self.phase = AppPhase::Foreground;
            self.last_code = 0;
            return Ok(0);
        }
        // 线性爬回：起点=后台档 fps，终点=0（不限）。
        let start = self.bg_tier.fps_cap();
        let remain = self.ramp_steps - self.ramp_step;
        let cap = (start as u64 * remain as u64 / self.ramp_steps as u64) as u32;
        self.last_code = 0;
        Ok(cap)
    }

    /// 渲染提交（偷渲红线检测点）。
    ///
    /// 后台相位（含 Pause/Throttle 档）一律拒绝并计违规——合规承诺
    /// 「不偷偷渲染耗电」的红线就是这一行。
    pub fn submit(&mut self, frame: u32, tick_ms: u64) -> Result<(), u16> {
        match self.phase {
            AppPhase::Background => {
                self.steal_hits += 1;
                self.last_code = XC_STEAL_RENDER;
                let _ = (frame, tick_ms);
                Err(XC_STEAL_RENDER)
            }
            _ => {
                self.ok_submits += 1;
                self.last_code = 0;
                Ok(())
            }
        }
    }

    /// 偷渲违规累计。
    pub fn steal_hits(&self) -> u32 {
        self.steal_hits
    }

    /// 合法提交累计。
    pub fn ok_submits(&self) -> u32 {
        self.ok_submits
    }

    /// 记录功耗实测样本（合规承诺的验证数据）。
    pub fn record_power(&mut self, s: PowerSample) -> Result<(), u16> {
        if self.samples.len() >= self.sample_cap {
            self.samples.remove(0);
        }
        self.samples.push(s);
        self.last_code = 0;
        Ok(())
    }

    /// 功耗实测样本。
    pub fn power_samples(&self) -> &[PowerSample] {
        &self.samples
    }

    /// 联动失效次数。
    pub fn link_fails(&self) -> u32 {
        self.link_fails
    }

    /// 爬回进度（当前步/总步）。
    pub fn ramp_progress(&self) -> (u32, u32) {
        (self.ramp_step, self.ramp_steps)
    }

    /// 读屏行。
    pub fn status_line(&self) -> &'static str {
        match self.phase {
            AppPhase::Foreground => "bg:foreground",
            AppPhase::Background => match self.bg_tier {
                BgTier::Throttle => "bg:background throttle",
                BgTier::Pause => "bg:background pause",
            },
            AppPhase::Ramping => "bg:ramping",
        }
    }
}

/// 自检：两档后台、偷渲红线、爬回恢复、合规样本、联动失效。
pub fn run_vea131_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea131_bgbudget");

    // C131-01 合法建 + 默认前台
    {
        let b = BgBudget::new(4, 8);
        let ok = match b {
            Ok(b) => b.phase() == AppPhase::Foreground,
            Err(_) => false,
        };
        chk(&mut set, "C131-01 合法建", ok, "默认相位非前台");
    }

    // C131-02 零参数拒绝
    {
        let a = BgBudget::new(0, 8).err() == Some(XC_BAD_TIER);
        let b = BgBudget::new(4, 0).err() == Some(XC_BAD_TIER);
        chk(&mut set, "C131-02 参数守卫", a && b, "零参数被放行");
    }

    // C131-03 两档后台：降帧 10fps / 暂停 0fps
    {
        let mut b = BgBudget::new(4, 8).unwrap();
        let _ = b.to_background(BgTier::Throttle, |_, _| true);
        let a = b.bg_tier().fps_cap() == 10;
        let _ = b.to_background(BgTier::Pause, |_, _| true);
        let c = b.bg_tier().fps_cap() == 0;
        chk(&mut set, "C131-03 两档后台", a && c, "档位帧率错");
    }

    // C131-04 偷渲红线：后台提交被当场阻断
    {
        let mut b = BgBudget::new(4, 8).unwrap();
        let _ = b.to_background(BgTier::Pause, |_, _| true);
        let r = b.submit(1, 100);
        let ok = r.err() == Some(XC_STEAL_RENDER) && b.steal_hits() == 1;
        chk(&mut set, "C131-04 偷渲红线", ok, "后台提交未阻断");
    }

    // C131-05 前台提交合法
    {
        let mut b = BgBudget::new(4, 8).unwrap();
        let r = b.submit(1, 100);
        let ok = r.is_ok() && b.ok_submits() == 1;
        chk(&mut set, "C131-05 前台提交", ok, "前台提交被误拒");
    }

    // C131-06 爬回恢复：回前台逐帧爬回，步尽转前台
    {
        let mut b = BgBudget::new(2, 8).unwrap();
        let _ = b.to_background(BgTier::Throttle, |_, _| true);
        let _ = b.to_foreground();
        let ok1 = b.phase() == AppPhase::Ramping;
        let _ = b.ramp_tick();
        let ok2 = b.phase() == AppPhase::Ramping;
        let _ = b.ramp_tick();
        let ok3 = b.phase() == AppPhase::Foreground;
        chk(&mut set, "C131-06 爬回恢复", ok1 && ok2 && ok3, "爬回链断");
    }

    // C131-07 爬回期提交合法（Ramping 不算偷渲）
    {
        let mut b = BgBudget::new(4, 8).unwrap();
        let _ = b.to_background(BgTier::Throttle, |_, _| true);
        let _ = b.to_foreground();
        let r = b.submit(2, 200);
        let ok = r.is_ok() && b.steal_hits() == 0;
        chk(&mut set, "C131-07 爬回提交", ok, "爬回期被误判偷渲");
    }

    // C131-08 联动失效独立执行
    {
        let mut b = BgBudget::new(4, 8).unwrap();
        let r = b.to_background(BgTier::Throttle, |_, _| false);
        let ok = r.is_ok() && b.link_fails() == 1 && b.bg_tier() == BgTier::Throttle;
        chk(&mut set, "C131-08 联动失效", ok, "联动失效未独立执行");
    }

    // C131-09 非法迁移：未后台即回前台被拒
    {
        let mut b = BgBudget::new(4, 8).unwrap();
        let r = b.to_foreground();
        let ok = r.err() == Some(XC_BAD_PHASE);
        chk(&mut set, "C131-09 相位守卫", ok, "非法迁移被放行");
    }

    // C131-10 合规样本：功耗实测可查
    {
        let mut b = BgBudget::new(4, 2).unwrap();
        let _ = b.record_power(PowerSample { window_ms: 1000, frames: 10, mw: 500 });
        let _ = b.record_power(PowerSample { window_ms: 1000, frames: 12, mw: 520 });
        let _ = b.record_power(PowerSample { window_ms: 1000, frames: 11, mw: 510 });
        let ok = b.power_samples().len() == 2;
        chk(&mut set, "C131-10 合规样本", ok, "样本环挤出错");
    }

    set
}
