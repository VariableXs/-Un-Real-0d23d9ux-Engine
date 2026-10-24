//! VE-F0497 · IR 单元测试语料（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0497`
//!
//! **锚点原文**：「IR 与优化单元测试语料库：每 pass 正反语料（正例应优化、
//! 反例应保守）、黄金 IR 对（输入 IR 加期望输出 IR 快照）、边界语料（深嵌
//! 套、大循环、病态模式）；快照失配处理与 F0454 同构（改进需显性更新）。
//! 数据结构：语料库（正反边界三区）；黄金 IR 快照；版本记录。错误路径与
//! 降级矩阵：快照失配→改进或回归显性判定；语料缺区→补齐阻断；病态语料
//! 挂起→防护上限验证联动（F0495）。判据：正反边界、黄金快照、显性判定、
//! 防护联动、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **正反边界三区**（判据一）。每 pass 三区语料（正例应优化 / 反例应
//!    保守 / 边界病态）；缺区 → 补齐阻断。
//!
//! 2. **黄金快照**（判据二）。黄金 IR 对（输入 IR + 期望输出 IR 快照）；
//!    版本化（快照带版本，版本不符拒绝比对）。
//!
//! 3. **显性判定**（判据三）。快照失配 → 改进或回归显性判定（二选一
//!    登记，不允许静默放行或静默更新）。
//!
//! 4. **防护联动**（判据四）。病态语料挂起 → 防护上限验证联动（F0495
//!    面：挂起即防护未生效，入账阻断）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0497";

/// 语料区。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// 正例：应优化。
    Positive,
    /// 反例：应保守。
    Negative,
    /// 边界：深嵌套 / 大循环 / 病态。
    Boundary,
}

/// 黄金 IR 对（输入 + 期望输出快照）。
#[derive(Clone, Debug, PartialEq)]
pub struct GoldenPair {
    pub pass: &'static str,
    /// 输入 IR（简化为指令计数序列的哈希面）。
    pub input: u64,
    /// 期望输出 IR 快照。
    pub expect: u64,
    /// 快照版本。
    pub ver: u32,
}

/// 失配判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 实测匹配快照。
    Match,
    /// 改进（显性更新快照）。
    Improved,
    /// 回归（阻断）。
    Regressed,
}

/// 语料库。
#[derive(Debug, Default)]
pub struct Corpus {
    /// 三区语料：pass → 区 → 用例数。
    pub zones: Vec<(&'static str, Zone, usize)>,
    pub goldens: Vec<GoldenPair>,
    /// 判定登记（显性判定账）。
    pub verdicts: Vec<String>,
    /// 防护联动账（病态挂起记录）。
    pub hangs: Vec<String>,
    pub ver: u32,
}

impl Corpus {
    pub fn new(ver: u32) -> Self {
        Corpus { ver, ..Default::default() }
    }

    /// 语料入区。
    pub fn add_case(&mut self, pass: &'static str, z: Zone) {
        match self.zones.iter_mut().find(|(p, zz, _)| *p == pass && *zz == z) {
            Some((_, _, n)) => *n += 1,
            None => self.zones.push((pass, z, 1)),
        }
    }

    /// 缺区扫描：任一 pass 三区缺一即缺口（补齐阻断面）。
    pub fn missing_zones(&self, passes: &[&'static str]) -> Vec<String> {
        let mut gaps = Vec::new();
        for p in passes.iter() {
            for z in [Zone::Positive, Zone::Negative, Zone::Boundary].iter() {
                let has = self.zones.iter().any(|(pp, zz, n)| pp == p && zz == z && *n > 0);
                if !has {
                    gaps.push(format!("{} 缺区 {:?}", p, z));
                }
            }
        }
        gaps
    }

    /// 黄金对登记（版本化）。
    pub fn add_golden(&mut self, g: GoldenPair) -> Result<(), &'static str> {
        if g.ver != self.ver {
            return Err("快照版本不符（版本化纪律）");
        }
        self.goldens.push(g);
        Ok(())
    }

    /// 快照比对：输出哈希 vs 期望 → 显性判定（Match / Improved / Regressed）。
    pub fn compare_golden(&mut self, pass: &str, actual: u64, improved_if_higher: bool) -> Verdict {
        let g = match self.goldens.iter().find(|g| g.pass == pass) {
            Some(g) => (g.expect, g.input),
            None => {
                self.verdicts.push(format!("快照缺失：{}（补齐阻断）", pass));
                return Verdict::Regressed;
            }
        };
        let (expect, input) = g;
        if actual == expect {
            Verdict::Match
        } else {
            // 失配 → 改进或回归显性判定（以优化方向为准登记）。
            let improved = if improved_if_higher { actual > expect } else { actual < expect };
            if improved && actual.saturating_sub(expect) <= input {
                self.verdicts.push(format!("显性判定 {}：改进（{} → {}）须显性更新快照", pass, expect, actual));
                Verdict::Improved
            } else {
                self.verdicts.push(format!("显性判定 {}：回归（{} → {}）阻断", pass, expect, actual));
                Verdict::Regressed
            }
        }
    }

    /// 病态挂起登记：挂起即防护未生效（F0495 联动阻断面）。
    pub fn hang(&mut self, case: &str, iter_cap: u32) {
        self.hangs.push(format!("病态语料挂起：{} 超 {} 轮上限 → F0495 防护未生效，阻断", case, iter_cap));
    }
}

// ---------------------------------------------------------------------------
// 判据（F0497-xxx）
// ---------------------------------------------------------------------------

/// 原始判据。
pub fn vec97_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、三区语料与缺区扫描。
    {
        let mut c = Corpus::new(1);
        c.add_case("fold", Zone::Positive);
        c.add_case("fold", Zone::Positive);
        c.add_case("fold", Zone::Negative);
        c.add_case("fold", Zone::Boundary);
        v.push((
            "F0497-三区-齐备",
            c.zones.len() == 3 && c.zones[0].2 == 2,
        ));
        let gaps = c.missing_zones(&["fold", "dce"]);
        v.push(("F0497-三区-缺区检出", gaps.len() == 3 && gaps[0].contains("dce")));
        // 补齐 dce 后缺口清零。
        c.add_case("dce", Zone::Positive);
        c.add_case("dce", Zone::Negative);
        c.add_case("dce", Zone::Boundary);
        v.push(("F0497-三区-补齐清零", c.missing_zones(&["fold", "dce"]).is_empty()));
    }

    // 二、黄金快照版本化。
    {
        let mut c = Corpus::new(2);
        let bad = c.add_golden(GoldenPair { pass: "fold", input: 100, expect: 80, ver: 1 });
        v.push(("F0497-快照-版本不符拒", bad.is_err()));
        let ok = c.add_golden(GoldenPair { pass: "fold", input: 100, expect: 80, ver: 2 });
        v.push(("F0497-快照-版本符入账", ok.is_ok() && c.goldens.len() == 1));
    }

    // 三、显性判定。
    {
        let mut c = Corpus::new(1);
        let _ = c.add_golden(GoldenPair { pass: "dce", input: 100, expect: 80, ver: 1 });
        // 匹配。
        v.push((
            "F0497-判定-匹配",
            c.compare_golden("dce", 80, true) == Verdict::Match,
        ));
        // 改进（更高指令数假设为改进方向）→ 显性更新登记。
        v.push((
            "F0497-判定-改进显性登记",
            c.compare_golden("dce", 70, true) == Verdict::Improved
                && c.verdicts.last().unwrap().contains("显性更新"),
        ));
        // 回归 → 阻断。
        v.push((
            "F0497-判定-回归阻断",
            c.compare_golden("dce", 95, false) == Verdict::Regressed
                && c.verdicts.last().unwrap().contains("阻断"),
        ));
        // 快照缺失 → 回归面。
        v.push((
            "F0497-判定-缺快照阻断",
            c.compare_golden("cse", 10, true) == Verdict::Regressed,
        ));
    }

    // 四、防护联动。
    {
        let mut c = Corpus::new(1);
        c.hang("深嵌套 4096 层", 8);
        v.push((
            "F0497-防护-挂起联动阻断",
            c.hangs.len() == 1 && c.hangs[0].contains("F0495"),
        ));
        v.push(("F0497-判据-锚点入册", ANCHOR == "VE-F0497"));
    }

    v
}

/// 注册表入口。
pub fn run_vec97_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec97");
    for (name, ok) in vec97_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec97_raw_checks as run_vec97_checks;
    #[test]
    fn vec97_corpus_checks() {
        for (name, ok) in run_vec97_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
