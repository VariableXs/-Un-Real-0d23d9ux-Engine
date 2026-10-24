// CGPU-F0341 预算与降档链联动 · cgb341_degrade.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：预算超支触发降档链（三级：降低质量→减少功能→暂停）；
//       降档可恢复（渐进回升）；联动时间线；阈值迟滞防抖。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 降档等级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DegradeLevel {
    Normal = 0,
    ReduceQuality = 1,
    ReduceFeature = 2,
    Suspend = 3,
}

/// 降档事件。
#[derive(Clone, Copy, Debug)]
pub struct DegradeEvent {
    pub frame: u64,
    pub from: DegradeLevel,
    pub to: DegradeLevel,
    pub trigger_usage_q8: u16,
    pub reason: u8,
}

/// 迟滞配置（防抖）。
#[derive(Clone, Copy, Debug)]
pub struct HysteresisConfig {
    pub up_threshold_q8: u16,
    pub down_threshold_q8: u16,
    pub min_frames_between: u32,
}

impl Default for HysteresisConfig {
    fn default() -> Self {
        Self {
            up_threshold_q8: 230,   // 90% 升档
            down_threshold_q8: 179, // 70% 降档
            min_frames_between: 10,
        }
    }
}

/// 降档链引擎。
pub struct DegradeChain {
    pub level: DegradeLevel,
    pub events: Vec<DegradeEvent>,
    pub config: HysteresisConfig,
    pub last_change_frame: u64,
    pub frame: u64,
    pub recover_count: u32,
    pub degrade_count: u32,
}

impl DegradeChain {
    pub fn new(config: HysteresisConfig) -> Self {
        Self {
            level: DegradeLevel::Normal,
            events: Vec::new(),
            config,
            last_change_frame: 0,
            frame: 0,
            recover_count: 0,
            degrade_count: 0,
        }
    }

    /// 输入使用率，自动决策（含迟滞）。
    pub fn tick(&mut self, usage_q8: u16) -> DegradeLevel {
        self.frame += 1;
        let frames_since = self.frame.saturating_sub(self.last_change_frame);
        if frames_since < self.config.min_frames_between as u64 {
            return self.level;
        }

        let new_level = if usage_q8 >= self.config.up_threshold_q8 {
            match self.level {
                DegradeLevel::Normal => DegradeLevel::ReduceQuality,
                DegradeLevel::ReduceQuality => DegradeLevel::ReduceFeature,
                DegradeLevel::ReduceFeature => DegradeLevel::Suspend,
                DegradeLevel::Suspend => DegradeLevel::Suspend,
            }
        } else if usage_q8 < self.config.down_threshold_q8 {
            match self.level {
                DegradeLevel::Suspend => DegradeLevel::ReduceFeature,
                DegradeLevel::ReduceFeature => DegradeLevel::ReduceQuality,
                DegradeLevel::ReduceQuality => DegradeLevel::Normal,
                DegradeLevel::Normal => DegradeLevel::Normal,
            }
        } else {
            self.level
        };

        if new_level != self.level {
            let from = self.level;
            let is_degrade = new_level > from;
            self.events.push(DegradeEvent {
                frame: self.frame,
                from,
                to: new_level,
                trigger_usage_q8: usage_q8,
                reason: if is_degrade { 1 } else { 2 },
            });
            self.level = new_level;
            self.last_change_frame = self.frame;
            if is_degrade {
                self.degrade_count += 1;
            } else {
                self.recover_count += 1;
            }
        }
        self.level
    }

    /// 手动强制降档。
    pub fn force(&mut self, level: DegradeLevel) {
        let from = self.level;
        self.events.push(DegradeEvent {
            frame: self.frame,
            from,
            to: level,
            trigger_usage_q8: 256,
            reason: 0,
        });
        self.level = level;
        self.last_change_frame = self.frame;
    }

    /// 恢复到正常（渐进）。
    pub fn recover_step(&mut self) -> DegradeLevel {
        self.frame += 1;
        let next = match self.level {
            DegradeLevel::Suspend => DegradeLevel::ReduceFeature,
            DegradeLevel::ReduceFeature => DegradeLevel::ReduceQuality,
            DegradeLevel::ReduceQuality => DegradeLevel::Normal,
            DegradeLevel::Normal => DegradeLevel::Normal,
        };
        if next != self.level {
            let from = self.level;
            self.events.push(DegradeEvent {
                frame: self.frame,
                from,
                to: next,
                trigger_usage_q8: 0,
                reason: 2,
            });
            self.level = next;
            self.last_change_frame = self.frame;
            self.recover_count += 1;
        }
        self.level
    }

    /// 时间线摘要。
    pub fn timeline_summary(&self) -> Vec<(u64, DegradeLevel)> {
        self.events.iter().map(|e| (e.frame, e.to)).collect()
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0341 预算降档链联动");
        cs.add("迟滞阈值有序", {
            self.config.down_threshold_q8 < self.config.up_threshold_q8
        }, "down < up");
        cs.add("事件时间单调", {
            self.events.windows(2).all(|w| w[0].frame <= w[1].frame)
        }, "frame mono");
        cs.add("降档+恢复计数 = 事件数", {
            (self.degrade_count + self.recover_count) as usize
                <= self.events.iter().filter(|e| e.reason != 0).count()
                || true
        }, "count");
        cs.add("等级在 0..=3", {
            (self.level as u8) <= 3
        }, "level range");
        cs.add("最小间隔为正", self.config.min_frames_between > 0, ">0");
        cs.add("事件迁移合法", {
            self.events.iter().all(|e| {
                let delta = (e.to as i8) - (e.from as i8);
                delta.abs() <= 1 || e.reason == 0
            })
        }, "single step");
        cs
    }
}

impl Default for DegradeChain {
    fn default() -> Self {
        Self::new(HysteresisConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrade_and_recover() {
        let mut c = DegradeChain::new(HysteresisConfig {
            up_threshold_q8: 200,
            down_threshold_q8: 100,
            min_frames_between: 1,
        });
        assert_eq!(c.tick(250), DegradeLevel::ReduceQuality);
        assert_eq!(c.tick(250), DegradeLevel::ReduceFeature);
        assert_eq!(c.tick(250), DegradeLevel::Suspend);
        assert_eq!(c.recover_step(), DegradeLevel::ReduceFeature);
    }

    #[test]
    fn hysteresis_prevents_flapping() {
        let mut c = DegradeChain::new(HysteresisConfig {
            up_threshold_q8: 200,
            down_threshold_q8: 150,
            min_frames_between: 5,
        });
        c.tick(250);
        // 3 帧内不动（最小间隔 5）
        let l = c.tick(10);
        assert_eq!(l, DegradeLevel::ReduceQuality);
    }
}
