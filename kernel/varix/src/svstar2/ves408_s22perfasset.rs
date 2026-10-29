//! ves408_s22perfasset — VE-F4168 · 性能资产汇总
//! T 域性能资产汇总：八组基准/预算/实测三档归集；汇总规则（每资产挂组/挂条/挂基准三指针 + 阈值登记）；
//! 与 F4123 预算体系对拍；阈值缺→立案补立；实测越阈→标记+重议；对拍不符→修正留痕；
//! 性能资产含翻译/排版/切换 i18n 专项开销档（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0860;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    Ok = 0,
    ThresholdMissing = SEG_BASE + 0x01, // 0x0861
    OverThreshold = SEG_BASE + 0x02,    // 0x0862
    Mismatch = SEG_BASE + 0x03,         // 0x0863
}

/// 三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfTier { Bench = 0, Budget = 1, Measured = 2 }

impl PerfTier {
    pub const ALL: [PerfTier; 3] = [PerfTier::Bench, PerfTier::Budget, PerfTier::Measured];
    pub fn name(self) -> &'static str {
        match self { PerfTier::Bench => "bench", PerfTier::Budget => "budget", PerfTier::Measured => "measured" }
    }
}

/// 性能资产条目：三指针 + 阈值 + 实测 + i18n 专项开销档标记。
#[derive(Clone, Copy)]
pub struct PerfAsset {
    pub tier: PerfTier,
    pub group: u8,      // 挂组
    pub card: u32,      // 挂条
    pub bench_ref: u32, // 挂基准（0 = 失联）
    pub threshold: u32, // 阈值登记（0 = 缺）
    pub measured: u32,  // 实测值
    pub i18n_specific: bool, // i18n 专项开销档（域本色）
}

impl PerfAsset {
    pub fn linked(&self) -> bool { self.bench_ref != 0 && self.card != 0 && self.group < 8 }
    pub fn over(&self) -> bool { self.threshold != 0 && self.measured > self.threshold }
}

/// 台账。
pub struct PerfBook {
    pub items: Vec<PerfAsset>,
    pub cases: Vec<&'static str>,   // 阈值缺立案
    pub remarks: Vec<&'static str>, // 重议/留痕
}

impl PerfBook {
    pub fn new() -> Self { PerfBook { items: Vec::new(), cases: Vec::new(), remarks: Vec::new() } }

    pub fn put(&mut self, e: PerfAsset) {
        match self.items.iter_mut().find(|x| x.tier == e.tier && x.group == e.group) {
            Some(x) => *x = e,
            None => self.items.push(e),
        }
    }

    /// 三档×八组完备（24 格）。
    pub fn grid_complete(&self) -> bool {
        PerfTier::ALL.iter().all(|t| (0..8).all(|g| self.items.iter().any(|x| x.tier == *t && x.group == g)))
    }

    /// 阈值缺立案定位。
    pub fn missing_thresholds(&self) -> Vec<usize> {
        self.items.iter().enumerate().filter(|(_, x)| x.threshold == 0).map(|(i, _)| i).collect()
    }

    pub fn file_threshold(&mut self, idx: usize, th: u32) -> bool {
        match self.items.get_mut(idx) {
            Some(x) => { x.threshold = th; self.cases.push("threshold-added"); true }
            None => false,
        }
    }

    /// 实测越阈标记 + 重议登记。
    pub fn audit_over(&mut self) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, x) in self.items.iter().enumerate() {
            if x.over() { out.push(i); }
        }
        for _ in out.iter() { self.remarks.push("over-threshold-rebook"); }
        out
    }

    /// 与 F4123 预算体系对拍：预算档各维上限一致。
    pub fn agrees_budget(&self, family: &[(u8, u32)]) -> bool {
        family.iter().all(|(g, cap)| {
            self.items.iter().any(|x| x.tier == PerfTier::Budget && x.group == *g && x.threshold == *cap)
        })
    }

    /// 域本色：i18n 专项开销档在账（bench 档）。
    pub fn i18n_specific_present(&self) -> bool {
        self.items.iter().any(|x| x.i18n_specific && x.tier == PerfTier::Bench && x.linked())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves408_checks() -> CheckSet {
    let mut set = CheckSet::new("ves408");
    let mut bk = PerfBook::new();

    // C408-01 三档×八组 24 格归集（阈值随格登记）。
    for t in PerfTier::ALL.iter() {
        for g in 0..8u8 {
            bk.put(PerfAsset { tier: *t, group: g, card: 4100 + g as u32, bench_ref: 0xC000 + g as u32,
                threshold: 100 + g as u32 * 10, measured: 90 + g as u32 * 10, i18n_specific: false });
        }
    }
    chk(&mut set, "C408-01", bk.grid_complete() && bk.items.len() == 24, "24 格齐");

    // C408-02 阈值缺立案。
    let mut bk2 = PerfBook::new();
    bk2.put(PerfAsset { tier: PerfTier::Budget, group: 0, card: 4123, bench_ref: 1, threshold: 0, measured: 0, i18n_specific: false });
    bk2.put(PerfAsset { tier: PerfTier::Bench, group: 1, card: 4124, bench_ref: 2, threshold: 50, measured: 0, i18n_specific: false });
    chk(&mut set, "C408-02", bk2.missing_thresholds() == vec![0], "阈值缺定位");

    // C408-03 补立阈值留痕。
    bk2.file_threshold(0, 150);
    chk(&mut set, "C408-03", bk2.missing_thresholds().is_empty() && bk2.cases.len() == 1, "补立留痕");

    // C408-04 实测越阈检出 + 重议登记。
    let mut bk3 = PerfBook::new();
    bk3.put(PerfAsset { tier: PerfTier::Measured, group: 0, card: 4116, bench_ref: 3, threshold: 100, measured: 120, i18n_specific: false });
    bk3.put(PerfAsset { tier: PerfTier::Measured, group: 1, card: 4116, bench_ref: 4, threshold: 100, measured: 80, i18n_specific: false });
    chk(&mut set, "C408-04", bk3.audit_over() == vec![0] && bk3.remarks.len() == 1, "越阈一标记一重议");

    // C408-05 与 F4123 预算对拍。
    let family = [(0u8, 150u32), (1u8, 160u32)];
    bk3.put(PerfAsset { tier: PerfTier::Budget, group: 0, card: 4123, bench_ref: 5, threshold: 150, measured: 0, i18n_specific: false });
    bk3.put(PerfAsset { tier: PerfTier::Budget, group: 1, card: 4123, bench_ref: 6, threshold: 160, measured: 0, i18n_specific: false });
    chk(&mut set, "C408-05", bk3.agrees_budget(&family), "预算对拍一致");

    // C408-06 对拍失配检出。
    chk(&mut set, "C408-06", !bk3.agrees_budget(&[(0u8, 999u32), (1u8, 160u32)]), "预算 999 失配");

    // C408-07 域本色：i18n 专项开销档在账。
    bk3.put(PerfAsset { tier: PerfTier::Bench, group: 0, card: 4122, bench_ref: 7, threshold: 40, measured: 35, i18n_specific: true });
    chk(&mut set, "C408-07", bk3.i18n_specific_present(), "专项开销档在账");

    // C408-08 失联检测。
    let mut bk4 = PerfBook::new();
    bk4.put(PerfAsset { tier: PerfTier::Bench, group: 0, card: 0, bench_ref: 0, threshold: 1, measured: 0, i18n_specific: false });
    chk(&mut set, "C408-08", !bk4.items[0].linked(), "失联检出");

    // C408-09 三档名单单源。
    chk(&mut set, "C408-09", PerfTier::ALL.len() == 3 && PerfTier::ALL[0].name() == "bench"
        && PerfTier::ALL[2].name() == "measured", "三档单源");

    // C408-10 覆盖登记幂等。
    let before = bk.items.len();
    bk.put(PerfAsset { tier: PerfTier::Bench, group: 0, card: 4100, bench_ref: 0xC000, threshold: 100, measured: 95, i18n_specific: false });
    chk(&mut set, "C408-10", bk.items.len() == before, "覆盖不增行");

    // C408-11 空册全缺。
    let empty = PerfBook::new();
    chk(&mut set, "C408-11", !empty.grid_complete(), "空册不全");

    // C408-12 错误码判别式唯一性（0x0861-0x0863 无碰撞）。
    chk(&mut set, "C408-12", PfCode::ThresholdMissing as u16 == 0x0861
        && PfCode::OverThreshold as u16 == 0x0862 && PfCode::Mismatch as u16 == 0x0863, "判别式唯一");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves408_smoke() {
        let set = run_ves408_checks();
        assert!(set.failed_count() == 0, "ves408 red: {}", set.red_items_text());
    }
}
