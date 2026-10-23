//! VE-F0115 · 异构故障隔离（VE-A 域 · A05 多 GPU 与异构组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0115`
//!
//! **职责定位（锚点原文）**：异构故障隔离——单卡故障的隔离（不拖垮其他卡），
//! 故障传播的阻断（看门狗），故障卡的恢复尝试与最终摘除。
//!
//! **铁律**：
//! 1. **故障隔离**——单卡故障不扩散。
//! 2. **传播阻断**——看门狗阻断故障传播。
//! 3. **恢复尝试**——先恢复后摘除。
//! 4. **摘除留痕**——最终摘除有完整记录。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B9（故障隔离）。
pub const SEG_BASE: u16 = 0xE8B9;

/// 故障传播被阻断。
pub const XC_PROPAGATION_BLOCKED: u16 = SEG_BASE | 0x01;
/// 卡已摘除。
pub const XC_GPU_REMOVED: u16 = SEG_BASE | 0x02;
/// 恢复失败。
pub const XC_RECOVER_FAIL: u16 = SEG_BASE | 0x03;
/// 未知卡。
pub const XC_UNKNOWN_GPU: u16 = SEG_BASE | 0x04;

/// 卡健康状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    Healthy,
    Degraded,
    Faulted,
    Removed,
}

/// 故障记录。
#[derive(Clone, Copy, Debug)]
pub struct FaultRecord {
    pub gpu_id: u16,
    pub tick: u64,
    /// 故障类型码。
    pub kind: u16,
    /// 是否已恢复。
    pub recovered: bool,
}

/// 看门狗（传播阻断）。
#[derive(Clone, Debug)]
pub struct Watchdog {
    /// 检测阈值。
    pub threshold: u32,
    /// 连续故障计数。
    pub streak: u32,
}

impl Watchdog {
    /// 喂故障信号；true=触发阻断。
    pub fn feed(&mut self) -> bool {
        self.streak += 1;
        self.streak >= self.threshold
    }

    pub fn reset(&mut self) {
        self.streak = 0;
    }
}

/// 异构故障隔离器。
pub struct FaultIsolator {
    /// 卡健康表。
    health: Vec<(u16, Health)>,
    /// 看门狗（每卡一个）。
    watchdogs: Vec<(u16, Watchdog)>,
    faults: Vec<FaultRecord>,
    last_code: u16,
    /// 隔离次数。
    isolations: u32,
    /// 恢复成功。
    recoveries: u32,
    /// 摘除。
    removals: u32,
    /// 传播阻断。
    blocks: u32,
    /// 最大恢复尝试。
    max_recover_attempts: u32,
}

impl FaultIsolator {
    pub fn new(max_recover_attempts: u32) -> Self {
        FaultIsolator {
            health: Vec::new(),
            watchdogs: Vec::new(),
            faults: Vec::new(),
            last_code: 0,
            isolations: 0,
            recoveries: 0,
            removals: 0,
            blocks: 0,
            max_recover_attempts: if max_recover_attempts == 0 {
                3
            } else {
                max_recover_attempts
            },
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn register_gpu(&mut self, id: u16, wd_threshold: u32) {
        self.health.retain(|(i, _)| *i != id);
        self.health.push((id, Health::Healthy));
        self.watchdogs.retain(|(i, _)| *i != id);
        self.watchdogs.push((
            id,
            Watchdog {
                threshold: if wd_threshold == 0 { 3 } else { wd_threshold },
                streak: 0,
            },
        ));
    }

    /// 报告故障（隔离第一步）。
    pub fn report_fault(&mut self, gpu_id: u16, kind: u16, tick: u64) -> Result<(), u16> {
        match self.health.iter_mut().find(|(i, _)| *i == gpu_id) {
            Some((_, h)) => {
                if *h == Health::Removed {
                    self.last_code = XC_GPU_REMOVED;
                    return Err(XC_GPU_REMOVED);
                }
                *h = Health::Faulted;
                self.faults.push(FaultRecord {
                    gpu_id,
                    tick,
                    kind,
                    recovered: false,
                });
                self.isolations += 1;
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = XC_UNKNOWN_GPU;
                Err(XC_UNKNOWN_GPU)
            }
        }
    }

    /// 看门狗阻断传播。
    pub fn block_propagation(&mut self, gpu_id: u16) -> Result<bool, u16> {
        match self.watchdogs.iter_mut().find(|(i, _)| *i == gpu_id) {
            Some((_, wd)) => {
                if wd.feed() {
                    wd.reset();
                    self.blocks += 1;
                    self.last_code = XC_PROPAGATION_BLOCKED;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            None => {
                self.last_code = XC_UNKNOWN_GPU;
                Err(XC_UNKNOWN_GPU)
            }
        }
    }

    /// 恢复尝试。
    pub fn try_recover(&mut self, gpu_id: u16) -> Result<bool, u16> {
        // 已摘除不可恢复。
        let h = match self.health.iter().find(|(i, _)| *i == gpu_id) {
            Some((_, h)) => *h,
            None => {
                self.last_code = XC_UNKNOWN_GPU;
                return Err(XC_UNKNOWN_GPU);
            }
        };
        if h == Health::Removed {
            self.last_code = XC_GPU_REMOVED;
            return Err(XC_GPU_REMOVED);
        }
        // 恢复成功判定（模拟：非 Removed 即可恢复）。
        if let Some((_, hh)) = self.health.iter_mut().find(|(i, _)| *i == gpu_id) {
            *hh = Health::Healthy;
        }
        // 标记最近故障为已恢复。
        if let Some(f) = self.faults.iter_mut().rev().find(|f| f.gpu_id == gpu_id) {
            f.recovered = true;
        }
        self.recoveries += 1;
        self.last_code = 0;
        Ok(true)
    }

    /// 摘除（恢复尝试耗尽）。
    pub fn remove_gpu(&mut self, gpu_id: u16) -> Result<(), u16> {
        match self.health.iter_mut().find(|(i, _)| *i == gpu_id) {
            Some((_, h)) => {
                *h = Health::Removed;
                self.removals += 1;
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = XC_UNKNOWN_GPU;
                Err(XC_UNKNOWN_GPU)
            }
        }
    }

    /// 恢复尝试次数检查后决定摘除。
    pub fn recover_or_remove(&mut self, gpu_id: u16, attempts: u32) -> Result<(), u16> {
        if attempts >= self.max_recover_attempts {
            self.remove_gpu(gpu_id)
        } else {
            self.try_recover(gpu_id).map(|_| ())
        }
    }

    pub fn health_of(&self, gpu_id: u16) -> Option<Health> {
        self.health.iter().find(|(i, _)| *i == gpu_id).map(|(_, h)| *h)
    }

    pub fn isolations(&self) -> u32 {
        self.isolations
    }

    pub fn recoveries(&self) -> u32 {
        self.recoveries
    }

    pub fn removals(&self) -> u32 {
        self.removals
    }

    pub fn blocks(&self) -> u32 {
        self.blocks
    }

    pub fn faults(&self) -> &[FaultRecord] {
        &self.faults
    }
}

/// 自检：故障隔离、传播阻断、恢复尝试、摘除留痕。
pub fn run_vea115_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea115_isolate");

    // C115-01 故障隔离
    {
        let mut f = FaultIsolator::new(3);
        f.register_gpu(0, 3);
        f.register_gpu(1, 3);
        let r = f.report_fault(0, 1, 10);
        let ok = r.is_ok() && f.health_of(0) == Some(Health::Faulted) && f.health_of(1) == Some(Health::Healthy);
        chk(&mut set, "C115-01 故障隔离", ok, "故障扩散");
    }

    // C115-02 传播阻断
    {
        let mut f = FaultIsolator::new(3);
        f.register_gpu(0, 2);
        let _ = f.block_propagation(0);
        let blocked = f.block_propagation(0);
        chk(&mut set, "C115-02 传播阻断", blocked == Ok(true) && f.blocks() == 1, "未阻断");
    }

    // C115-03 恢复尝试
    {
        let mut f = FaultIsolator::new(3);
        f.register_gpu(0, 3);
        let _ = f.report_fault(0, 1, 1);
        let r = f.try_recover(0);
        chk(&mut set, "C115-03 恢复", r == Ok(true) && f.health_of(0) == Some(Health::Healthy), "恢复失败");
    }

    // C115-04 摘除留痕
    {
        let mut f = FaultIsolator::new(3);
        f.register_gpu(0, 3);
        let _ = f.report_fault(0, 1, 1);
        let r = f.remove_gpu(0);
        chk(&mut set, "C115-04 摘除", r.is_ok() && f.removals() == 1 && !f.faults().is_empty(), "摘除无痕");
    }

    // C115-05 摘除后拒绝恢复
    {
        let mut f = FaultIsolator::new(3);
        f.register_gpu(0, 3);
        let _ = f.remove_gpu(0);
        let r = f.try_recover(0);
        chk(
            &mut set,
            "C115-05 摘除禁恢复",
            r.is_err() && f.last_code() == XC_GPU_REMOVED,
            "摘除后被恢复",
        );
    }

    // C115-06 未知卡拒绝
    {
        let mut f = FaultIsolator::new(3);
        let r = f.report_fault(99, 1, 1);
        chk(&mut set, "C115-06 未知拒", r.is_err() && f.last_code() == XC_UNKNOWN_GPU, "未知被放");
    }

    // C115-07 恢复耗尽摘除
    {
        let mut f = FaultIsolator::new(2);
        f.register_gpu(0, 3);
        let _ = f.report_fault(0, 1, 1);
        let r = f.recover_or_remove(0, 2);
        chk(&mut set, "C115-07 耗尽摘除", r.is_ok() && f.health_of(0) == Some(Health::Removed), "未摘除");
    }

    // C115-08 未耗尽可恢复
    {
        let mut f = FaultIsolator::new(3);
        f.register_gpu(0, 3);
        let _ = f.report_fault(0, 1, 1);
        let r = f.recover_or_remove(0, 1);
        chk(&mut set, "C115-08 未耗尽恢复", r.is_ok() && f.health_of(0) == Some(Health::Healthy), "误摘除");
    }

    // C115-09 故障记录完整
    {
        let mut f = FaultIsolator::new(3);
        f.register_gpu(0, 3);
        let _ = f.report_fault(0, 7, 42);
        let ok = f.faults().len() == 1 && f.faults()[0].kind == 7 && f.faults()[0].tick == 42;
        chk(&mut set, "C115-09 记录完整", ok, "记录缺失");
    }

    // C115-10 隔离计数自洽
    {
        let mut f = FaultIsolator::new(3);
        f.register_gpu(0, 3);
        f.register_gpu(1, 3);
        let _ = f.report_fault(0, 1, 1);
        let _ = f.report_fault(1, 2, 2);
        chk(&mut set, "C115-10 计数自洽", f.isolations() == 2, "计数错");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea115_all_green() {
        let s = run_vea115_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
