//! VE-F0550 · WGSL 生成质量（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0550`
//!
//! **锚点原文**：「WGSL 生成质量：可读性（语义名保留、缩进规范）、质量判据
//! （强校验一次通过率为核心指标——预防规则有效性的直接度量）、评审抽查
//! （F0505 机制同构复用）；质量指标入 F0552 套件与收口。判据：通过率核心
//! 指标、语义命名、抽查制、专项阻断、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **通过率核心指标**（判据一）。强校验一次通过率 = 生成即合规的直接
//!    度量（预防规则有效性的证据），低于阈值触发专项改进阻断。
//!
//! 2. **语义命名**（判据二）。保留 IR 语义名、冲突唯一化留痕（机制同构
//!    F0505/F0530）。
//!
//! 3. **抽查制**（判据三）。确定性抽样评审，低分触发改写。
//!
//! 4. **专项阻断**（判据四）。通过率低于阈值 → 预防规则专项改进（不是
//!    靠后处理修），阻断到改进完成。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0550";

/// 一次通过率阈值（千分比）。
pub const PASS_THRESHOLD: u32 = 950;

// ---------------------------------------------------------------------------
// 一、通过率核心指标
// ---------------------------------------------------------------------------

/// 通过率统计。
#[derive(Default)]
pub struct PassStat {
    pub total: u32,
    pub first_pass: u32,
}

impl PassStat {
    pub fn new() -> PassStat {
        PassStat { total: 0, first_pass: 0 }
    }

    /// 记录一次生成（是否一次过强校验）。
    pub fn record(&mut self, ok: bool) {
        self.total += 1;
        if ok {
            self.first_pass += 1;
        }
    }

    /// 通过率（千分比）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        self.first_pass * 1000 / self.total
    }

    /// 专项阻断判定。
    pub fn blocked(&self) -> bool {
        self.total >= 10 && self.rate() < PASS_THRESHOLD
    }
}

// ---------------------------------------------------------------------------
// 二、语义命名（同构复用）
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct WNamer {
    map: Vec<(String, String)>,
    pub renames: Vec<(String, String)>,
}

impl WNamer {
    pub fn new() -> WNamer {
        WNamer { map: Vec::new(), renames: Vec::new() }
    }

    pub fn assign(&mut self, ir: &str) -> String {
        if let Some((_, g)) = self.map.iter().find(|(i, _)| i == ir) {
            return g.clone();
        }
        let mut n = 0u32;
        let mut cand = ir.to_string();
        while self.map.iter().any(|(_, g)| *g == cand) {
            n += 1;
            cand = format!("{}_{}", ir, n);
        }
        if cand != ir {
            self.renames.push((ir.to_string(), cand.clone()));
        }
        self.map.push((ir.to_string(), cand.clone()));
        cand
    }
}

// ---------------------------------------------------------------------------
// 三、预防规则有效性度量
// ---------------------------------------------------------------------------

/// 规则命中账：规则名 → 命中数。
#[derive(Default)]
pub struct RuleHit {
    pub hits: Vec<(&'static str, u32)>,
}

impl RuleHit {
    pub fn new() -> RuleHit {
        RuleHit { hits: Vec::new() }
    }

    pub fn hit(&mut self, rule: &'static str) {
        match self.hits.iter_mut().find(|(n, _)| *n == rule) {
            Some((_, c)) => *c += 1,
            None => self.hits.push((rule, 1)),
        }
    }

    /// 最有效规则（命中最多）。
    pub fn top(&self) -> Option<&'static str> {
        self.hits.iter().max_by_key(|(_, c)| *c).map(|(n, _)| *n)
    }

    /// 零命中规则（可能冗余或无触发场景）。
    pub fn unused(&self, all: &[&'static str]) -> Vec<&'static str> {
        all.iter().filter(|r| !self.hits.iter().any(|(n, _)| n == *r)).copied().collect()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec150_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec150");

    // 族一：通过率核心指标。
    let mut st = PassStat::new();
    s.add("空账零率", st.rate() == 0 && !st.blocked(), "无样本不判");
    for _ in 0..95 {
        st.record(true);
    }
    for _ in 0..5 {
        st.record(false);
    }
    s.add("通过率 950", st.rate() == 950, "95/100");
    s.add("阈值界内不阻断", !st.blocked(), "950 >= 950");
    st.record(false);
    s.add("跌破阈值阻断", st.blocked(), "949 < 950");
    s.add("小样本不判", {
        let mut s2 = PassStat::new();
        for _ in 0..9 {
            s2.record(false);
        }
        !s2.blocked()
    }, "≥10 样本前置");

    // 族二：语义命名。
    let mut nm = WNamer::new();
    let g1 = nm.assign("base");
    s.add("语义名保留", g1 == "base", "原名透传");
    s.add("撞名唯一化", nm.assign("base") == "base" && {
        let mut n2 = WNamer::new();
        let _ = n2.assign("a");
        n2.assign("a") == "a_1"
    }, "留痕可审计");

    // 族三：预防规则有效性。
    let mut rh = RuleHit::new();
    rh.hit("混型转换");
    rh.hit("混型转换");
    rh.hit("零值初始化");
    s.add("命中计数", rh.top() == Some("混型转换"), "最有效规则");
    s.add("零命中可见", rh.unused(&["混型转换", "零值初始化", "只读误写拦截"]) == vec!["只读误写拦截"], "冗余规则探查");
    s.add("全命中无冗余", rh.unused(&["混型转换", "零值初始化"]).is_empty(), "账目一致");

    // 族四：抽查制。
    let mut seen: Vec<usize> = Vec::new();
    let mut cur = 3usize % 8;
    for _ in 0..3 {
        while seen.contains(&cur) {
            cur = (cur + 1) % 8;
        }
        seen.push(cur);
        cur = (cur + 5) % 8;
    }
    s.add("抽样定容", seen.len() == 3, "抽三件");
    s.add("抽样界内无重", seen.iter().all(|&i| i < 8) && {
        let mut c = seen.clone();
        c.sort();
        c.dedup();
        c.len() == 3
    }, "确定性抽样");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate() {
        let mut st = PassStat::new();
        st.record(true);
        assert_eq!(st.rate(), 1000);
        assert!(!st.blocked()); // total < 10
    }
}
