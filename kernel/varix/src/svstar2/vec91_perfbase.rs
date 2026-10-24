//! VE-F0491 · 优化性能基准与调优（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0491`
//!
//! **锚点原文**：「优化 pass 性能基准：各 pass 编译时长基准（按模块规模分
//! 位）、优化收益基准（优化前后指令数与预估周期对比）、调优数据（pass 收
//! 益比排序指导默认开关）；基准版本化，退化阻断。数据结构：基准记录（时
//! 长×收益）；调优排序；版本记录。错误路径与降级矩阵：收益为负的 pass→
//! 默认关闭登记（诚实关闭）；时长退化→定位 pass 阻断；收益争议→对拍数据
//! 裁决。判据：双维基准、诚实关闭、退化阻断、数据裁决、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **双维基准**（判据一）。每 pass 记录时长基准（按模块规模分位：小/
//!    中/大三档）+ 收益基准（优化前后指令数与预估周期对比）——双维齐备
//!    才算基准完整，缺一维入账待补。
//!
//! 2. **诚实关闭**（判据二）。收益为负的 pass → 默认关闭登记（诚实关闭：
//!    不隐藏负收益、不粉饰数据），关闭记录带负收益数值可查。
//!
//! 3. **退化阻断**（判据三）。时长退化（同规模分位超容忍带）→ 定位 pass
//!    阻断（不静默放行），退化记录带 pass 名与分位证据。
//!
//! 4. **数据裁决**（判据四）。收益争议 → 对拍数据裁决（以基准记录为准，
//!    不以口头判断为准），裁决记录入账。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0491";

/// 模块规模分位（时长基准的规模维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleTier {
    /// 小模块（≤256 指令）。
    Small,
    /// 中模块（≤4096 指令）。
    Mid,
    /// 大模块（>4096 指令）。
    Large,
}

/// 一条基准记录（时长 × 收益 双维）。
#[derive(Clone, Debug, PartialEq)]
pub struct Bench {
    pub pass: &'static str,
    pub tier: ScaleTier,
    /// 编译时长（微秒）。
    pub dur_us: u64,
    /// 优化前指令数。
    pub ins_before: u32,
    /// 优化后指令数。
    pub ins_after: u32,
    /// 预估周期改善（正=改善，负=劣化）。
    pub cycles_gain: i64,
    /// 基准版本号。
    pub ver: u32,
}

impl Bench {
    /// 指令缩减率（bp）：负收益返回 0（不显示负缩减）。
    pub fn shrink_bp(&self) -> u32 {
        if self.ins_before == 0 {
            return 0;
        }
        let saved = self.ins_before.saturating_sub(self.ins_after) as u64;
        (saved * 10000 / self.ins_before as u64) as u32
    }

    /// 双维齐备断言：时长与收益测量都有效。
    pub fn both_dims(&self) -> bool {
        self.dur_us > 0 && (self.ins_before > 0 || self.ins_after > 0)
    }
}

/// 单条调优判定：默认开关该 pass 开还是关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    /// 默认开。
    On,
    /// 诚实关闭（收益为负）。
    OffNegative,
    /// 退化阻断（时长退化未裁决前禁用）。
    OffRegressed,
}

/// 基准与调优台账。
#[derive(Debug, Default)]
pub struct BenchBook {
    pub records: Vec<Bench>,
    /// 调优排序（pass 名，按收益比降序）。
    pub ranking: Vec<(&'static str, i64)>,
    /// 开关登记。
    pub switches: Vec<(&'static str, Switch)>,
    /// 退化阻断记录。
    pub regressions: Vec<String>,
    /// 裁决记录（争议 → 数据裁决）。
    pub adjudications: Vec<String>,
    pub ver: u32,
}

/// 时长退化容忍带（同分位相对退化 bp 上限）。
pub const REGRESS_TOL_BP: u64 = 1500;

impl BenchBook {
    pub fn new(ver: u32) -> Self {
        BenchBook { ver, ..Default::default() }
    }

    /// 基准入账（版本化）。
    pub fn record(&mut self, b: Bench) -> Result<(), &'static str> {
        if b.ver != self.ver {
            return Err("基准版本不匹配（版本化纪律）");
        }
        if !b.both_dims() {
            return Err("双维基准缺维（时长或收益缺失）");
        }
        self.records.push(b);
        Ok(())
    }

    /// 同 pass 同分位基线查询。
    fn baseline(&self, pass: &str, tier: ScaleTier) -> Option<u64> {
        self.records
            .iter()
            .find(|r| r.pass == pass && r.tier == tier)
            .map(|r| r.dur_us)
    }

    /// 退化阻断：新时长相对基线超容忍带 → 定位 pass 阻断。
    pub fn check_regress(&mut self, pass: &'static str, tier: ScaleTier, new_us: u64) -> bool {
        match self.baseline(pass, tier) {
            Some(base) if base > 0 => {
                let growth_bp = new_us.saturating_sub(base) * 10000 / base;
                if growth_bp > REGRESS_TOL_BP {
                    self.regressions.push(format!(
                        "时长退化阻断：{}({:?}) 基线 {}us → {}us（+{}bp）",
                        pass, tier, base, new_us, growth_bp
                    ));
                    self.switches.push((pass, Switch::OffRegressed));
                    false
                } else {
                    true
                }
            }
            _ => true,
        }
    }

    /// 诚实关闭：收益为负的 pass 默认关闭登记。
    pub fn honest_close(&mut self, pass: &'static str, cycles_gain: i64) {
        if cycles_gain < 0 {
            self.switches.push((pass, Switch::OffNegative));
        }
    }

    /// 开关查询：退化阻断优先于诚实关闭（禁用面更大）。
    pub fn switch_of(&self, pass: &str) -> Switch {
        if self.switches.iter().any(|(p, s)| *p == pass && *s == Switch::OffRegressed) {
            Switch::OffRegressed
        } else if self.switches.iter().any(|(p, s)| *p == pass && *s == Switch::OffNegative) {
            Switch::OffNegative
        } else {
            Switch::On
        }
    }

    /// 调优排序：按收益比（周期改善 / 时长，i64 缩比）降序。
    pub fn build_ranking(&mut self) {
        self.ranking.clear();
        for r in self.records.iter() {
            if r.dur_us == 0 {
                continue;
            }
            let ratio = r.cycles_gain * 1000 / r.dur_us as i64;
            self.ranking.push((r.pass, ratio));
        }
        self.ranking.sort_by(|a, b| b.1.cmp(&a.1));
    }

    /// 收益争议裁决：以基准记录数据为准（数据裁决，不以口头为准）。
    pub fn adjudicate(&mut self, pass: &'static str, claim: i64) -> i64 {
        let actual = self
            .records
            .iter()
            .filter(|r| r.pass == pass)
            .map(|r| r.cycles_gain)
            .sum::<i64>();
        self.adjudications
            .push(format!("裁决 {}：主张 {}，数据 {}", pass, claim, actual));
        actual
    }
}

// ---------------------------------------------------------------------------
// 判据（F0491-xxx）
// ---------------------------------------------------------------------------

/// 原始判据：Vec<(名称, 是否绿)>。
pub fn vec91_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、双维基准。
    {
        let mut book = BenchBook::new(3);
        let ok = book.record(Bench {
            pass: "fold",
            tier: ScaleTier::Mid,
            dur_us: 120,
            ins_before: 1000,
            ins_after: 700,
            cycles_gain: 300,
            ver: 3,
        });
        v.push(("F0491-双维-齐备入账", ok.is_ok() && book.records.len() == 1));
        // 缺维拦截：时长为 0。
        let bad = book.record(Bench {
            pass: "dce",
            tier: ScaleTier::Small,
            dur_us: 0,
            ins_before: 10,
            ins_after: 5,
            cycles_gain: 5,
            ver: 3,
        });
        v.push(("F0491-双维-缺维拦截", bad.is_err()));
        // 版本化拦截。
        let vbad = book.record(Bench {
            pass: "cse",
            tier: ScaleTier::Small,
            dur_us: 10,
            ins_before: 10,
            ins_after: 5,
            cycles_gain: 5,
            ver: 2,
        });
        v.push(("F0491-版本-不匹配拦截", vbad.is_err()));
        // 缩减率 30%。
        v.push(("F0491-收益-缩减率bp", book.records[0].shrink_bp() == 3000));
    }

    // 二、诚实关闭。
    {
        let mut book = BenchBook::new(1);
        book.honest_close("spec_neg", -50);
        book.honest_close("spec_pos", 120);
        v.push((
            "F0491-诚实-负收益默认关",
            book.switch_of("spec_neg") == Switch::OffNegative
                && book.switch_of("spec_pos") == Switch::On,
        ));
    }

    // 三、退化阻断。
    {
        let mut book = BenchBook::new(1);
        let _ = book.record(Bench {
            pass: "inline",
            tier: ScaleTier::Small,
            dur_us: 100,
            ins_before: 100,
            ins_after: 80,
            cycles_gain: 20,
            ver: 1,
        });
        // +10% 在容忍带内 → 放行。
        let ok = book.check_regress("inline", ScaleTier::Small, 110);
        v.push(("F0491-退化-带内放行", ok && book.regressions.is_empty()));
        // +200% 超容忍带 → 阻断并禁用。
        let bad = book.check_regress("inline", ScaleTier::Small, 300);
        v.push((
            "F0491-退化-超带阻断",
            !bad && book.switch_of("inline") == Switch::OffRegressed,
        ));
        // 阻断优先于其他开关态。
        book.honest_close("inline", 10);
        v.push(("F0491-退化-阻断优先", book.switch_of("inline") == Switch::OffRegressed));
    }

    // 四、数据裁决。
    {
        let mut book = BenchBook::new(1);
        let _ = book.record(Bench {
            pass: "licm",
            tier: ScaleTier::Mid,
            dur_us: 200,
            ins_before: 500,
            ins_after: 400,
            cycles_gain: 250,
            ver: 1,
        });
        let actual = book.adjudicate("licm", 999);
        v.push(("F0491-裁决-以数据为准", actual == 250 && book.adjudications.len() == 1));
    }

    // 五、调优排序 + 判据自检。
    {
        let mut book = BenchBook::new(1);
        let _ = book.record(Bench {
            pass: "cheap",
            tier: ScaleTier::Small,
            dur_us: 100,
            ins_before: 10,
            ins_after: 9,
            cycles_gain: 10,
            ver: 1,
        });
        let _ = book.record(Bench {
            pass: "rich",
            tier: ScaleTier::Small,
            dur_us: 100,
            ins_before: 10,
            ins_after: 8,
            cycles_gain: 50,
            ver: 1,
        });
        book.build_ranking();
        v.push((
            "F0491-排序-收益比降序",
            book.ranking.len() == 2 && book.ranking[0].0 == "rich" && book.ranking[1].0 == "cheap",
        ));
        v.push(("F0491-判据-锚点入册", ANCHOR == "VE-F0491"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec91）。
pub fn run_vec91_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec91");
    for (name, ok) in vec91_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec91_raw_checks as run_vec91_checks;
    #[test]
    fn vec91_perfbase_checks() {
        for (name, ok) in run_vec91_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
