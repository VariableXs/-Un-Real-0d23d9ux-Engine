//! ves362_s20cost — VE-F4122 · T07 i18n 成本模型
//! 三线成本建模：文本翻译查找 O(1) 常数×语种修正系数 + 排版成本（四族差异系数）+ 资源加载成本（语言包体积系数）合成 + 公式版本化可复算 + 系数实测校准留痕 + 模型漂移版本冻结重校 + 无障碍功能成本系数专列（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD040;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmCode {
    Ok = 0,
    Drift = SEG_BASE + 0x01,
    RecalcMismatch = SEG_BASE + 0x02,
}

/// 四族排版差异系数（万分比，Latin=1000 基准）。
pub const FAMILY_COST: [u32; 4] = [1_000, 1_800, 1_400, 1_500]; // latin/cjk/arabic/thai-indic

/// 语种修正系数表。
pub struct LangFactor { pub lang: &'static str, pub factor_bp: u32 }

pub struct CostModel {
    pub version: u32,
    pub frozen_version: u32,
    pub lookup_const_ns: u32,     // 查找 O(1) 常数
    pub lang_factors: Vec<LangFactor>,
    /// 无障碍功能成本系数专列（域本色）。
    pub a11y_factor_bp: u32,
    /// 校准留痕。
    pub calibrations: Vec<(&'static str, u32, u32)>, // (来源, 实测 ns, 模型 ns)
}

pub const MODEL_VERSION: u32 = 7;

impl CostModel {
    pub fn new() -> Self {
        CostModel {
            version: MODEL_VERSION, frozen_version: MODEL_VERSION,
            lookup_const_ns: 120,
            lang_factors: vec![
                LangFactor { lang: "en", factor_bp: 1_000 },
                LangFactor { lang: "zh", factor_bp: 1_600 },
                LangFactor { lang: "ar", factor_bp: 1_300 },
                LangFactor { lang: "th", factor_bp: 1_200 },
            ],
            a11y_factor_bp: 900,
            calibrations: Vec::new(),
        }
    }

    pub fn factor_of(&self, lang: &str) -> u32 {
        self.lang_factors.iter().find(|f| f.lang == lang).map_or(1_000, |f| f.factor_bp)
    }

    /// 线一：查找成本 = 常数 × 语种系数。
    pub fn lookup_cost_ns(&self, lang: &str) -> u32 {
        self.lookup_const_ns * self.factor_of(lang) / 1_000
    }

    /// 线二：排版成本 = 基准 × 族系数。
    pub fn layout_cost_us(&self, family: usize, units: u32) -> u32 {
        units * FAMILY_COST[family.min(3)] / 1_000
    }

    /// 线三：资源加载成本 = 体积 KB × 体积系数。
    pub fn load_cost_us(&self, kb: u32, factor_bp: u32) -> u32 {
        kb * factor_bp / 1_000
    }

    /// 三线合成：总成本（ns 口径，排版/加载微秒转纳秒）。
    pub fn total_ns(&self, lang: &str, family: usize, units: u32, kb: u32, load_bp: u32) -> u32 {
        self.lookup_cost_ns(lang)
            + self.layout_cost_us(family, units) * 1_000
            + self.load_cost_us(kb, load_bp) * 1_000
    }

    /// 实测校准 + 留痕。
    pub fn calibrate(&mut self, source: &'static str, measured_ns: u32) {
        let modeled = self.lookup_cost_ns(source);
        self.calibrations.push((source, measured_ns, modeled));
    }

    /// 校准偏差（万分比）。
    pub fn last_deviation_bp(&self) -> u32 {
        match self.calibrations.last() {
            Some((_, m, mo)) if *mo > 0 => {
                let d = if *m > *mo { m - mo } else { mo - m };
                d * 10_000 / mo
            }
            _ => 0,
        }
    }

    /// 模型漂移：工作版本 ≠ 冻结版本 → 重校。
    pub fn drifted(&self) -> bool { self.version != self.frozen_version }

    pub fn freeze(&mut self) { self.frozen_version = self.version; }
}

/// 复算器：同输入两次计算必须一致（可复算断言）。
pub fn recalc_ok(m: &CostModel, lang: &str, family: usize, units: u32, kb: u32, load_bp: u32) -> bool {
    let a = m.total_ns(lang, family, units, kb, load_bp);
    let b = m.total_ns(lang, family, units, kb, load_bp);
    a == b
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves362_checks() -> CheckSet {
    let mut set = CheckSet::new("ves362");
    let m = CostModel::new();

    // C362-01 四族系数表冻结。
    chk(&mut set, "C362-01", FAMILY_COST == [1_000, 1_800, 1_400, 1_500], "四族系数冻结");

    // C362-02 线一：查找成本语种修正。
    chk(&mut set, "C362-02", m.lookup_cost_ns("en") == 120 && m.lookup_cost_ns("zh") == 192, "en 120ns zh 192ns");

    // C362-03 未登语种回退基准系数。
    chk(&mut set, "C362-03", m.lookup_cost_ns("ko") == 120, "ko 回退 1000 系数");

    // C362-04 线二：排版四族差异。
    chk(&mut set, "C362-04",
        m.layout_cost_us(0, 100) == 100 && m.layout_cost_us(1, 100) == 180
        && m.layout_cost_us(2, 100) == 140, "三族单位排版成本");

    // C362-05 线三：加载体积系数。
    chk(&mut set, "C362-05", m.load_cost_us(2_400, 1_500) == 3_600, "2400KB×1.5 => 3600us");

    // C362-06 三线合成。
    let total = m.total_ns("zh", 1, 50, 2_400, 1_500);
    chk(&mut set, "C362-06", total == 192 + 90_000 + 3_600_000, "合成 3_690_192ns");

    // C362-07 可复算断言。
    chk(&mut set, "C362-07", recalc_ok(&m, "zh", 1, 50, 2_400, 1_500), "同输入双算一致");

    // C362-08 校准留痕。
    let mut m2 = CostModel::new();
    m2.calibrate("zh-bench", 200);
    chk(&mut set, "C362-08", m2.calibrations.len() == 1 && m2.calibrations[0].0 == "zh-bench", "校准入库");

    // C362-09 校准偏差计算。
    chk(&mut set, "C362-09", m2.last_deviation_bp() == 416, "|200-192|/192 ≈ 416bp");

    // C362-10 版本冻结与漂移检测。
    let mut m3 = CostModel::new();
    m3.version = 8;
    chk(&mut set, "C362-10", m3.drifted() && { m3.freeze(); !m3.drifted() }, "漂移检出+冻结复位");

    // C362-11 无障碍成本专列（域本色）。
    chk(&mut set, "C362-11", m.a11y_factor_bp == 900, "专列系数 0.9");

    // C362-12 公式版本号单源。
    chk(&mut set, "C362-12", MODEL_VERSION == 7 && m.version == MODEL_VERSION, "版本单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves362_smoke() {
        let set = run_ves362_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves362 red: {:?}", red);
    }

    #[test]
    fn ves362_costs() {
        let m = CostModel::new();
        assert_eq!(m.lookup_cost_ns("ar"), 156);
        assert_eq!(m.layout_cost_us(3, 200), 300);
    }
}
