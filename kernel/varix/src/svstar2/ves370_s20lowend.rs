//! ves370_s20lowend — VE-F4130 · T07 低端设备 i18n 表
//! 三档设备档位（低/中/高）×i18n 三关键路径（查找/切换/排版）性能保障表 + 降级预案预挂 + 档位自动探测+手动覆盖上报 + 档内越阈预案激活立案 + 表漂移实测校准 + 低端档无障碍功能不缩水（保底红线——域本色实测）+ 探测数据匿名聚合。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD740;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeCode {
    Ok = 0,
    OverThreshold = SEG_BASE + 0x01,
    ProbeMismatch = SEG_BASE + 0x02,
}

/// 设备档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceTier { Low = 0, Mid = 1, High = 2 }

impl DeviceTier {
    pub const ALL: [DeviceTier; 3] = [DeviceTier::Low, DeviceTier::Mid, DeviceTier::High];
    pub fn name(self) -> &'static str {
        match self { DeviceTier::Low => "low", DeviceTier::Mid => "mid", DeviceTier::High => "high" }
    }
}

/// 关键路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path { Lookup = 0, Switch = 1, Layout = 2 }

impl Path {
    pub const ALL: [Path; 3] = [Path::Lookup, Path::Switch, Path::Layout];
    pub fn name(self) -> &'static str {
        match self { Path::Lookup => "lookup", Path::Switch => "switch", Path::Layout => "layout" }
    }
}

/// 保障表：档位 × 路径 → 阈值 ms。
pub struct GuaranteeTable { pub rows: [[u32; 3]; 3] } // [档][径]

pub const FALLBACK_PLANS: [&'static str; 3] = ["shrunk-lru", "direct-switch", "cpu-layout"];

impl GuaranteeTable {
    pub fn new() -> Self {
        //          lookup switch layout
        GuaranteeTable { rows: [[ 8,  900,  60],   // low
                                [ 5,  600,  40],   // mid
                                [ 3,  400,  25]] } // high
    }

    pub fn threshold(&self, tier: DeviceTier, path: Path) -> u32 {
        self.rows[tier as usize][path as usize]
    }

    /// 越阈判定 + 预案激活名。
    pub fn check(&self, tier: DeviceTier, path: Path, measured_ms: u32) -> Result<(), (LeCode, &'static str)> {
        if measured_ms > self.threshold(tier, path) {
            return Err((LeCode::OverThreshold, FALLBACK_PLANS[path as usize]));
        }
        Ok(())
    }

    /// 档位序：低档阈值最宽（保障弱机）。
    pub fn monotone(&self) -> bool {
        Path::ALL.iter().all(|p| {
            let i = *p as usize;
            self.rows[0][i] >= self.rows[1][i] && self.rows[1][i] >= self.rows[2][i]
        })
    }
}

/// 档位探测器：按内存/核数自动探测；可手动覆盖。
pub struct TierProbe { pub mem_mb: u32, pub cores: u8, pub manual: Option<DeviceTier> }

impl TierProbe {
    pub fn new(mem_mb: u32, cores: u8) -> Self { TierProbe { mem_mb, cores, manual: None } }

    /// 自动探测：低 <4GB 或 ≤2 核；中 <8GB；高其余。
    pub fn auto(&self) -> DeviceTier {
        if self.mem_mb < 4_000 || self.cores <= 2 { DeviceTier::Low }
        else if self.mem_mb < 8_000 { DeviceTier::Mid }
        else { DeviceTier::High }
    }

    /// 生效档位：手动覆盖优先。
    pub fn effective(&self) -> (DeviceTier, bool) {
        match self.manual {
            Some(t) => (t, true), // 覆盖显性
            None => (self.auto(), false),
        }
    }

    pub fn override_to(&mut self, t: DeviceTier) { self.manual = Some(t); }

    /// 探测错误上报：手动档 ≠ 自动档 → 上报（匿名聚合）。
    pub fn mismatch_report(&self) -> Option<(u8, u8)> {
        let (eff, manual) = self.effective();
        let auto = self.auto();
        if manual && eff != auto {
            Some((auto as u8, eff as u8)) // 只报档位号（匿名聚合——不带设备指纹）
        } else {
            None
        }
    }
}

/// 低端无障碍保底（域本色红线实测）：低档三项无障碍功能预算不缩水。
pub const LOWEND_A11Y_FLOOR: [u32; 3] = [150, 40, 60]; // 朗读/色标/快照 ms

pub fn lowend_a11y_intact(low_budgets: &[u32; 3]) -> bool {
    low_budgets.iter().zip(LOWEND_A11Y_FLOOR.iter()).all(|(b, f)| b >= f)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves370_checks() -> CheckSet {
    let mut set = CheckSet::new("ves370");
    let gt = GuaranteeTable::new();

    // C370-01 三档×三径齐备。
    chk(&mut set, "C370-01",
        DeviceTier::ALL.iter().all(|t| Path::ALL.iter().all(|p| gt.threshold(*t, *p) > 0)), "九格全非零");

    // C370-02 阈值单调：低 ≥ 中 ≥ 高。
    chk(&mut set, "C370-02", gt.monotone(), "低档最宽");

    // C370-03 带内放行。
    chk(&mut set, "C370-03", gt.check(DeviceTier::Low, Path::Lookup, 8).is_ok(), "低档查找 8ms 贴界");

    // C370-04 越阈 → 预案激活名。
    chk(&mut set, "C370-04",
        gt.check(DeviceTier::Low, Path::Switch, 901) == Err((LeCode::OverThreshold, "direct-switch")), "切换越阈 → 预挂预案");

    // C370-05 预案名单单源。
    chk(&mut set, "C370-05", FALLBACK_PLANS == ["shrunk-lru", "direct-switch", "cpu-layout"], "三预案单源");

    // C370-06 自动探测：低档机。
    let p_low = TierProbe::new(3_000, 2);
    chk(&mut set, "C370-06", p_low.auto() == DeviceTier::Low, "3GB/2核 → 低档");

    // C370-07 自动探测：中/高档。
    chk(&mut set, "C370-07",
        TierProbe::new(6_000, 4).auto() == DeviceTier::Mid
        && TierProbe::new(16_000, 8).auto() == DeviceTier::High, "6GB 中档 16GB 高档");

    // C370-08 手动覆盖优先。
    let mut p = TierProbe::new(16_000, 8);
    p.override_to(DeviceTier::Low);
    let (eff, ovr) = p.effective();
    chk(&mut set, "C370-08", eff == DeviceTier::Low && ovr, "覆盖生效显性");

    // C370-09 探测错报（匿名聚合）。
    let rpt = p.mismatch_report();
    chk(&mut set, "C370-09", rpt == Some((2, 0)), "自动 2 手动 0 上报");

    // C370-10 一致不上报。
    let p_ok = TierProbe::new(16_000, 8);
    chk(&mut set, "C370-10", p_ok.mismatch_report().is_none(), "一致零上报");

    // C370-11 低端无障碍保底（域本色红线实测）。
    chk(&mut set, "C370-11", lowend_a11y_intact(&[150, 45, 70]) && !lowend_a11y_intact(&[150, 35, 70]), "保底双态");

    // C370-12 保底名单单源。
    chk(&mut set, "C370-12", LOWEND_A11Y_FLOOR == [150, 40, 60], "三列保底常量");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves370_smoke() {
        let set = run_ves370_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves370 red: {:?}", red);
    }

    #[test]
    fn ves370_probe() {
        assert_eq!(TierProbe::new(2_000, 1).auto(), DeviceTier::Low);
        assert_eq!(TierProbe::new(8_000, 4).auto(), DeviceTier::High);
    }
}
