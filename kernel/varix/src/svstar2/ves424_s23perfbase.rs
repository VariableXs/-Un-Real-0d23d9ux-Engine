//! ves424_s23perfbase — VE-F4184 · T 域性能总基准
//! 终审：T 域性能总基准合成（八组基准并入+五档阈值复核（查找/切换/排版/翻译管线/资源加载）+与家族基准对拍）；
//! 预算基线冻结前最后一核；多语言无障碍专项开销档设独立阈值（域本色）；
//! 阈值越界→重议或降级声明；对拍分歧→仲裁；基线未冻→阻断 F4190。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1040;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PbCode {
    Ok = 0,
    OverThreshold = SEG_BASE + 0x01, // 0x1041
    Disagreement = SEG_BASE + 0x02,  // 0x1042
    BlockFreeze = SEG_BASE + 0x03,   // 0x1043
}

/// 五档 + 无障碍专项档（第六档，域本色独立阈值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfTier { Lookup = 0, Switch = 1, Layout = 2, MtPipeline = 3, AssetLoad = 4, A11yExtra = 5 }

impl PerfTier {
    pub const ALL: [PerfTier; 6] = [
        PerfTier::Lookup, PerfTier::Switch, PerfTier::Layout,
        PerfTier::MtPipeline, PerfTier::AssetLoad, PerfTier::A11yExtra];
    pub fn name(self) -> &'static str {
        match self {
            PerfTier::Lookup => "lookup", PerfTier::Switch => "switch", PerfTier::Layout => "layout",
            PerfTier::MtPipeline => "mt-pipeline", PerfTier::AssetLoad => "asset-load",
            PerfTier::A11yExtra => "a11y-extra",
        }
    }
}

/// 总基准册：各档并入值（μs 档）。
pub struct BaselineBook {
    pub merged: Vec<(PerfTier, u32)>,   // 并入值
    pub thresholds: Vec<(PerfTier, u32)>, // 阈值（越界即红）
    pub degraded: Vec<PerfTier>,        // 降级声明档
    pub last_check_ok: bool,            // 终核通过（F4190 冻结前置）
}

impl BaselineBook {
    pub fn new() -> Self {
        BaselineBook { merged: Vec::new(), thresholds: Vec::new(), degraded: Vec::new(), last_check_ok: false }
    }
    pub fn merge_group(&mut self, tier: PerfTier, v: u32) {
        for m in self.merged.iter_mut() { if m.0 == tier { m.1 = m.1.max(v); return; } }
        self.merged.push((tier, v));
    }
    pub fn set_threshold(&mut self, tier: PerfTier, t: u32) {
        for x in self.thresholds.iter_mut() { if x.0 == tier { x.1 = t; return; } }
        self.thresholds.push((tier, t));
    }
    pub fn threshold_of(&self, tier: PerfTier) -> u32 {
        self.thresholds.iter().find(|(t, _)| *t == tier).map(|(_, v)| *v).unwrap_or(0)
    }
    /// 五档+专项档全复核：越界且未降级声明→红档清单。
    pub fn recheck(&self) -> Vec<PerfTier> {
        let mut bad = Vec::new();
        for t in PerfTier::ALL {
            let v = self.merged.iter().find(|(m, _)| *m == t).map(|(_, v)| *v).unwrap_or(0);
            if v > self.threshold_of(t) && !self.degraded.contains(&t) { bad.push(t); }
        }
        bad
    }
    /// 降级声明：越界档显性入册（留痕）。
    pub fn declare_degrade(&mut self, tier: PerfTier) { self.degraded.push(tier); }
    /// 终核裁决：全部绿→可进 F4190 冻结；否则阻断。
    pub fn final_check(&mut self) -> Result<(), PbCode> {
        let bad = self.recheck();
        if bad.is_empty() {
            self.last_check_ok = true;
            Ok(())
        } else {
            self.last_check_ok = false;
            Err(PbCode::OverThreshold)
        }
    }
}

/// 家族基准对拍：与家族总表档位值偏差 ≤ 容差（bp，万分之一）。
pub fn family_agrees(own: u32, family: u32, tol_bp: u32) -> bool {
    let a = own.max(family) as u64;
    let b = own.min(family) as u64;
    if a == 0 { return true; }
    (a - b) * 10_000 / a <= tol_bp as u64
}

/// 对拍分歧仲裁：取保守（较大）值为准。
pub fn arbitrate(own: u32, family: u32) -> u32 { own.max(family) }

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves424_checks() -> CheckSet {
    let mut set = CheckSet::new("ves424");
    let mut bk = BaselineBook::new();

    // C424-01 五档并入+专项档共六档。
    bk.merge_group(PerfTier::Lookup, 80);
    bk.merge_group(PerfTier::Switch, 150);
    bk.merge_group(PerfTier::Layout, 200);
    bk.merge_group(PerfTier::MtPipeline, 900);
    bk.merge_group(PerfTier::AssetLoad, 500);
    bk.merge_group(PerfTier::A11yExtra, 250); // 域本色独立阈值档
    chk(&mut set, "C424-01", bk.merged.len() == 6, "六档并入");

    // C424-02 并入取最大（八组同档并最大）。
    bk.merge_group(PerfTier::Switch, 140);
    chk(&mut set, "C424-02", bk.merged.iter().find(|(t, _)| *t == PerfTier::Switch).unwrap().1 == 150, "并入取大");

    // C424-03 阈值复核：全部未越界→绿。
    for (t, th) in [(PerfTier::Lookup, 120u32), (PerfTier::Switch, 200), (PerfTier::Layout, 250),
                    (PerfTier::MtPipeline, 1000), (PerfTier::AssetLoad, 600), (PerfTier::A11yExtra, 300)] {
        bk.set_threshold(t, th);
    }
    chk(&mut set, "C424-03", bk.recheck().is_empty(), "六档全绿");

    // C424-04 终核通过→可进冻结。
    chk(&mut set, "C424-04", bk.final_check().is_ok() && bk.last_check_ok, "终核放行");

    // C424-05 翻译管线越界→红档定位。
    bk.merge_group(PerfTier::MtPipeline, 1200);
    let bad = bk.recheck();
    chk(&mut set, "C424-05", bad == vec![PerfTier::MtPipeline], "越界定位");

    // C424-06 终核阻断（F4190 前置门）。
    chk(&mut set, "C424-06", bk.final_check() == Err(PbCode::OverThreshold) && !bk.last_check_ok, "终核阻断");

    // C424-07 降级声明后复核复绿（留痕）。
    bk.declare_degrade(PerfTier::MtPipeline);
    chk(&mut set, "C424-07", bk.recheck().is_empty() && bk.degraded.len() == 1, "降级声明复绿");

    // C424-08 重议阈值复绿（不降级路径）。
    let mut bk2 = BaselineBook::new();
    bk2.merge_group(PerfTier::A11yExtra, 400);
    bk2.set_threshold(PerfTier::A11yExtra, 300);
    chk(&mut set, "C424-08", bk2.recheck() == vec![PerfTier::A11yExtra], "专项档越界");
    bk2.set_threshold(PerfTier::A11yExtra, 450);
    chk(&mut set, "C424-08b", bk2.recheck().is_empty(), "重议阈值复绿");

    // C424-09 家族对拍：容差内一致。
    chk(&mut set, "C424-09", family_agrees(150, 152, 200), "对拍一致");

    // C424-10 家族对拍：超容差失配。
    chk(&mut set, "C424-10", !family_agrees(150, 200, 200), "对拍失配");

    // C424-11 分歧仲裁取保守（大值）。
    chk(&mut set, "C424-11", arbitrate(150, 200) == 200 && arbitrate(300, 200) == 300, "仲裁取大");

    // C424-12 六档单源齐备断言。
    chk(&mut set, "C424-12", PerfTier::ALL.len() == 6
        && PerfTier::ALL[5].name() == "a11y-extra"
        && PerfTier::ALL[0].name() == "lookup", "六档单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves424_smoke() {
        let set = run_ves424_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves424 red: {:?}", red);
    }
}
