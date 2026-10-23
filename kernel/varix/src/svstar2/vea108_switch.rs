//! VE-F0108 · 集显/独显动态切换双模式（VE-A 域 · A05 多 GPU 与异构组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0108`
//!
//! **职责定位（锚点原文）**：两卡切换的显式与隐式双模式（用户手选/系统自动），
//! 切换成本预告（切一次要多久），切换的保存点（进度不丢）。
//!
//! **铁律**：
//! 1. **双模式**——显式（用户手选）与隐式（系统自动）并存，显式优先。
//! 2. **成本预告**——切换前给成本估计，实测校准。
//! 3. **保存点**——进度不丢，丢失即阻断级错误。
//! 4. **失败回退**——切换失败回到原卡。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B2（卡切换）。
pub const SEG_BASE: u16 = 0xE8B2;

/// 切换失败。
pub const XC_SWITCH_FAIL: u16 = SEG_BASE | 0x01;
/// 进度丢失（阻断级）。
pub const XC_PROGRESS_LOST: u16 = SEG_BASE | 0x02;
/// 成本虚标。
pub const XC_COST_LIE: u16 = SEG_BASE | 0x03;
/// 保存点损坏。
pub const XC_CHECKPOINT_BAD: u16 = SEG_BASE | 0x04;
/// 未知卡。
pub const XC_UNKNOWN_GPU: u16 = SEG_BASE | 0x05;

/// 适配器类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuClass {
    /// 集显（低功耗）。
    Integrated,
    /// 独显（高性能）。
    Discrete,
}

/// 切换模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchMode {
    /// 显式：用户手选。
    Explicit,
    /// 隐式：系统自动。
    Implicit,
}

/// 切换成本预告。
#[derive(Clone, Copy, Debug)]
pub struct SwitchCost {
    /// 预估毫秒。
    pub estimate_ms: u32,
    /// 实测毫秒（校准后）。
    pub measured_ms: u32,
    /// 是否已校准。
    pub calibrated: bool,
}

impl SwitchCost {
    /// 预告与实测偏差（千分比）。
    pub fn drift_permille(&self) -> i32 {
        if self.estimate_ms == 0 {
            return 0;
        }
        ((self.measured_ms as i64 - self.estimate_ms as i64) * 1000 / self.estimate_ms as i64)
            as i32
    }
}

/// 保存点（进度快照）。
#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub seq: u64,
    /// 进度指纹（状态完整性）。
    pub fingerprint: u32,
    /// 已完成步骤数。
    pub steps_done: u32,
}

impl Checkpoint {
    pub fn verify(&self) -> bool {
        self.fingerprint == (self.seq as u32) ^ (self.steps_done.rotate_left(9))
    }
}

/// 适配器卡记录。
#[derive(Clone, Debug)]
pub struct AdapterCard {
    pub id: u16,
    pub class: GpuClass,
    pub healthy: bool,
}

/// 双模式切换管理器。
pub struct GpuSwitcher {
    cards: Vec<AdapterCard>,
    /// 当前活跃卡。
    active: u16,
    /// 当前模式。
    mode: SwitchMode,
    /// 保存点。
    checkpoint: Option<Checkpoint>,
    /// 切换成本记录。
    cost: SwitchCost,
    last_code: u16,
    /// 成功切换计数。
    switches: u32,
    /// 回退计数。
    fallbacks: u32,
    /// 阻断事件计数（进度丢失）。
    blockers: u32,
    /// 历史切换日志（模式×from×to×成功）。
    log: Vec<(SwitchMode, u16, u16, bool)>,
}

impl GpuSwitcher {
    pub fn new(active: u16, estimate_ms: u32) -> Result<Self, u16> {
        if estimate_ms == 0 {
            return Err(XC_COST_LIE);
        }
        Ok(GpuSwitcher {
            cards: Vec::new(),
            active,
            mode: SwitchMode::Implicit,
            checkpoint: None,
            cost: SwitchCost {
                estimate_ms,
                measured_ms: 0,
                calibrated: false,
            },
            last_code: 0,
            switches: 0,
            fallbacks: 0,
            blockers: 0,
            log: Vec::new(),
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn add_card(&mut self, id: u16, class: GpuClass) {
        self.cards.retain(|c| c.id != id);
        self.cards.push(AdapterCard {
            id,
            class,
            healthy: true,
        });
    }

    pub fn mark_unhealthy(&mut self, id: u16) {
        if let Some(c) = self.cards.iter_mut().find(|c| c.id == id) {
            c.healthy = false;
        }
    }

    /// 成本预告（切换前查询）。
    pub fn cost_estimate(&self) -> SwitchCost {
        self.cost
    }

    /// 实测校准（切完一次后调）。
    pub fn calibrate(&mut self, measured_ms: u32) -> Result<(), u16> {
        if measured_ms == 0 {
            self.last_code = XC_COST_LIE;
            return Err(XC_COST_LIE);
        }
        self.cost.measured_ms = measured_ms;
        self.cost.calibrated = true;
        // 偏差超 50% 判虚标。
        if self.cost.drift_permille().abs() > 500 {
            self.last_code = XC_COST_LIE;
            return Err(XC_COST_LIE);
        }
        self.last_code = 0;
        Ok(())
    }

    /// 建保存点。
    pub fn checkpoint(&mut self, steps_done: u32) -> Result<u64, u16> {
        let seq = self
            .checkpoint
            .as_ref()
            .map(|c| c.seq + 1)
            .unwrap_or(1);
        let cp = Checkpoint {
            seq,
            fingerprint: (seq as u32) ^ (steps_done.rotate_left(9)),
            steps_done,
        };
        self.checkpoint = Some(cp);
        self.last_code = 0;
        Ok(seq)
    }

    /// 切换（双模式统一入口）。显式优先于隐式。
    pub fn switch_to(&mut self, target: u16, mode: SwitchMode) -> Result<(), u16> {
        // 显式模式优先：隐式请求在显式锁定期拒绝。
        if mode == SwitchMode::Implicit && self.mode == SwitchMode::Explicit {
            self.log.push((mode, self.active, target, false));
            self.last_code = XC_SWITCH_FAIL;
            return Err(XC_SWITCH_FAIL);
        }
        // 保存点必须在且可验（进度不丢前置）。
        match &self.checkpoint {
            Some(cp) if cp.verify() => {}
            Some(_) => {
                self.blockers += 1;
                self.last_code = XC_CHECKPOINT_BAD;
                return Err(XC_CHECKPOINT_BAD);
            }
            None => {
                // 无保存点即切换 = 进度必丢，阻断。
                self.blockers += 1;
                self.last_code = XC_PROGRESS_LOST;
                return Err(XC_PROGRESS_LOST);
            }
        }
        // 目标卡健康检查。
        match self.cards.iter().find(|c| c.id == target) {
            Some(c) if c.healthy => {}
            _ => {
                self.log.push((mode, self.active, target, false));
                self.last_code = XC_UNKNOWN_GPU;
                return Err(XC_UNKNOWN_GPU);
            }
        }
        // 切换执行（模拟：成功路径）。
        let from = self.active;
        self.active = target;
        self.mode = mode;
        self.switches += 1;
        self.log.push((mode, from, target, true));
        self.last_code = 0;
        Ok(())
    }

    /// 切换失败回退（显式失败回原卡）。
    pub fn fallback(&mut self) -> Result<u16, u16> {
        if let Some((_, from, _, _)) = self.log.iter().rev().find(|l| l.3) {
            let prev = *from;
            self.active = prev;
            self.fallbacks += 1;
            self.last_code = XC_SWITCH_FAIL;
            return Ok(prev);
        }
        self.last_code = XC_SWITCH_FAIL;
        Err(XC_SWITCH_FAIL)
    }

    pub fn active(&self) -> u16 {
        self.active
    }

    pub fn mode(&self) -> SwitchMode {
        self.mode
    }

    pub fn switches(&self) -> u32 {
        self.switches
    }

    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }

    pub fn blockers(&self) -> u32 {
        self.blockers
    }

    pub fn log(&self) -> &[(SwitchMode, u16, u16, bool)] {
        &self.log
    }
}

/// 自检：双模式、成本预告、保存点、进度不丢。
pub fn run_vea108_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn sw() -> GpuSwitcher {
        let mut s = GpuSwitcher::new(0, 100).unwrap();
        s.add_card(0, GpuClass::Integrated);
        s.add_card(1, GpuClass::Discrete);
        s
    }

    let mut set = CheckSet::new("vea108_switch");

    // C108-01 成本预告
    {
        let s = sw();
        let c = s.cost_estimate();
        chk(&mut set, "C108-01 成本预告", c.estimate_ms == 100 && !c.calibrated, "预告缺");
    }

    // C108-02 无保存点切换阻断
    {
        let mut s = sw();
        let r = s.switch_to(1, SwitchMode::Explicit);
        chk(
            &mut set,
            "C108-02 无保存点阻断",
            r.is_err() && s.last_code() == XC_PROGRESS_LOST && s.blockers() == 1,
            "进度被丢",
        );
    }

    // C108-03 保存点后可切换
    {
        let mut s = sw();
        let _ = s.checkpoint(10);
        let r = s.switch_to(1, SwitchMode::Explicit);
        chk(&mut set, "C108-03 保存点切换", r.is_ok() && s.active() == 1, "切换失败");
    }

    // C108-04 显式优先于隐式
    {
        let mut s = sw();
        let _ = s.checkpoint(1);
        let _ = s.switch_to(1, SwitchMode::Explicit);
        let r = s.switch_to(0, SwitchMode::Implicit);
        chk(
            &mut set,
            "C108-04 显式优先",
            r.is_err() && s.active() == 1,
            "隐式越权",
        );
    }

    // C108-05 成本实测校准
    {
        let mut s = sw();
        let r = s.calibrate(95);
        chk(&mut set, "C108-05 校准", r.is_ok() && s.cost_estimate().calibrated, "未校准");
    }

    // C108-06 成本虚标检出
    {
        let mut s = sw();
        let r = s.calibrate(500); // 偏差 400%
        chk(
            &mut set,
            "C108-06 虚标检出",
            r.is_err() && s.last_code() == XC_COST_LIE,
            "虚标放行",
        );
    }

    // C108-07 坏保存点阻断
    {
        let mut s = sw();
        let _ = s.checkpoint(5);
        if let Some(cp) = s.checkpoint.as_mut() {
            cp.fingerprint ^= 0xFF;
        }
        let r = s.switch_to(1, SwitchMode::Explicit);
        chk(
            &mut set,
            "C108-07 坏保存点",
            r.is_err() && s.last_code() == XC_CHECKPOINT_BAD,
            "坏点被放",
        );
    }

    // C108-08 不健康卡拒绝
    {
        let mut s = sw();
        let _ = s.checkpoint(1);
        s.mark_unhealthy(1);
        let r = s.switch_to(1, SwitchMode::Explicit);
        chk(
            &mut set,
            "C108-08 坏卡拒绝",
            r.is_err() && s.last_code() == XC_UNKNOWN_GPU,
            "坏卡被切",
        );
    }

    // C108-09 失败回退
    {
        let mut s = sw();
        let _ = s.checkpoint(1);
        let _ = s.switch_to(1, SwitchMode::Explicit);
        let r = s.fallback();
        chk(&mut set, "C108-09 回退", r == Ok(0) && s.fallbacks() == 1, "回退失败");
    }

    // C108-10 切换日志留痕
    {
        let mut s = sw();
        let _ = s.checkpoint(1);
        let _ = s.switch_to(1, SwitchMode::Explicit);
        chk(&mut set, "C108-10 日志留痕", s.log().len() >= 1, "日志缺失");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea108_all_green() {
        let s = run_vea108_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
