//! 深化层四 · F142 安全披露通道（2026-09-27 深化批次四 · g 层）。
//!
//! 影响版本区间判定（受影响/已修复区间）、修复优先级矩阵（评分×
//! 利用流行度）、公告四段模板渲染门、逐版本回归清单、漏洞知识库
//! 条目模型（CVE 唯一性）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 影响版本区间：[first_affected, last_affected] 闭区间 + fixed_in
// ---------------------------------------------------------------------------

pub struct AffectedRange {
    pub first: u32,
    pub last: u32,
    pub fixed_in: u32,
}

impl AffectedRange {
    pub fn new(first: u32, last: u32, fixed_in: u32) -> Result<AffectedRange, &'static str> {
        if first > last {
            return Err("区间倒置");
        }
        if fixed_in <= last {
            return Err("修复版本必须在受影响区间之后");
        }
        Ok(AffectedRange { first, last, fixed_in })
    }

    /// 版本受影响判定：落在 [first, last] 即受影响；≥fixed_in 已修。
    pub fn status_of(&self, ver: u32) -> &'static str {
        if ver >= self.fixed_in {
            "已修复"
        } else if ver >= self.first && ver <= self.last {
            "受影响"
        } else {
            "不受影响"
        }
    }
}

// ---------------------------------------------------------------------------
// 修复优先级矩阵：cvss 百分位 × 利用流行度 → P0-P3
// ---------------------------------------------------------------------------

/// popularity: 0-2（无/有/野外利用）。
pub fn fix_priority(cvss_pct: u32, popularity: u8) -> u8 {
    let pop = popularity.min(2) as u32;
    // 矩阵：野外利用一律 P0；高分为 P1 起。
    if pop == 2 {
        return 0;
    }
    if cvss_pct >= 70 {
        return if pop == 1 { 0 } else { 1 };
    }
    if cvss_pct >= 40 {
        return if pop == 1 { 1 } else { 2 };
    }
    (2 + pop.min(1)) as u8
}

// ---------------------------------------------------------------------------
// 公告四段模板：概述/影响/缓解/致谢——缺段不发布
// ---------------------------------------------------------------------------

pub struct Advisory {
    pub summary: &'static str,
    pub impact: &'static str,
    pub mitigation: &'static str,
    pub credits: &'static str,
}

impl Advisory {
    pub fn validate(&self) -> Result<(), &'static str> {
        for (n, s) in [
            ("概述", self.summary),
            ("影响", self.impact),
            ("缓解", self.mitigation),
            ("致谢", self.credits),
        ] {
            if s.trim().is_empty() {
                return Err("公告缺段");
            }
            let _ = n;
        }
        Ok(())
    }

    /// 渲染行（标题+正文四组）。
    pub fn render(&self) -> alloc::vec::Vec<&'static str> {
        alloc::vec![
            "## 概述", self.summary,
            "## 影响", self.impact,
            "## 缓解", self.mitigation,
            "## 致谢", self.credits,
        ]
    }
}

// ---------------------------------------------------------------------------
// 逐版本回归清单：每个受影响版本修复后必须回测全绿才可公告
// ---------------------------------------------------------------------------

pub struct RegressionRow {
    pub ver: u32,
    pub tested: bool,
    pub passed: bool,
}

/// 公告门：受影响清单内全部已测且过 → Ok；返回 (未测, 未过)。
pub fn regression_gate(rows: &[RegressionRow], affected: &[u32]) -> (alloc::vec::Vec<u32>, alloc::vec::Vec<u32>) {
    let mut untested: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
    let mut failed: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
    for v in affected {
        match rows.iter().find(|r| r.ver == *v) {
            None => untested.push(*v),
            Some(r) if !r.passed => failed.push(*v),
            Some(r) if !r.tested => untested.push(*v),
            _ => {}
        }
    }
    (untested, failed)
}

// ---------------------------------------------------------------------------
// 漏洞知识库条目：CVE 唯一性 + 状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VulnStatus {
    Triaging,
    Fixed,
    Published,
}

pub struct VulnEntry {
    pub cve: &'static str,
    pub status: VulnStatus,
}

pub struct VulnKb {
    entries: alloc::vec::Vec<VulnEntry>,
}

impl VulnKb {
    pub fn new() -> VulnKb {
        VulnKb { entries: alloc::vec::Vec::new() }
    }

    pub fn add(&mut self, cve: &'static str) -> Result<(), &'static str> {
        if !crate::stareco::deep::f142f::cve_id_ok(cve) {
            return Err("CVE 编号非法");
        }
        if self.entries.iter().any(|e| e.cve == cve) {
            return Err("CVE 重复登记：知识库唯一性");
        }
        self.entries.push(VulnEntry { cve, status: VulnStatus::Triaging });
        Ok(())
    }

    pub fn advance(&mut self, cve: &str, to: VulnStatus) -> Result<(), &'static str> {
        let e = self.entries.iter_mut().find(|e| e.cve == cve).ok_or("未知 CVE")?;
        let legal = match (e.status, to) {
            (VulnStatus::Triaging, VulnStatus::Fixed)
            | (VulnStatus::Fixed, VulnStatus::Published) => true,
            _ => false,
        };
        if !legal {
            return Err("非法漏洞状态迁移");
        }
        e.status = to;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F142G_TAG: &str = "stareco-F142-deep4";

pub fn run_f142_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F142G_TAG);

    // 影响区间
    let r = AffectedRange::new(10, 20, 21).expect("ok");
    set.add(
        "f142g range status",
        r.status_of(9) == "不受影响"
            && r.status_of(15) == "受影响"
            && r.status_of(21) == "已修复",
        "三态判定",
    );
    set.add("f142g range invert", AffectedRange::new(20, 10, 21).is_err(), "区间倒置拒绝");
    set.add("f142g fix inside", AffectedRange::new(10, 20, 15).is_err(), "修复版本在区间内拒绝");

    // 优先级
    set.add("f142g wild p0", fix_priority(10, 2) == 0, "野外利用一律 P0");
    set.add("f142g high pop", fix_priority(80, 1) == 0, "高分+有人利用=P0");
    set.add("f142g high lonely", fix_priority(80, 0) == 1, "高分无人利用=P1");
    set.add("f142g low", fix_priority(10, 0) == 2, "低分=P2");

    // 公告
    let ok = Advisory { summary: "s", impact: "i", mitigation: "m", credits: "c" };
    set.add("f142g advisory ok", ok.validate().is_ok() && ok.render().len() == 8, "四段齐渲染");
    let no_credit = Advisory { summary: "s", impact: "i", mitigation: "m", credits: " " };
    set.add("f142g advisory missing", no_credit.validate().is_err(), "缺致谢段拒绝");

    // 回归门
    let rows = [
        RegressionRow { ver: 10, tested: true, passed: true },
        RegressionRow { ver: 11, tested: true, passed: false },
    ];
    let (unt, fail) = regression_gate(&rows, &[10, 11, 12]);
    set.add(
        "f142g regression gate",
        unt == alloc::vec![12] && fail == alloc::vec![11],
        "未测/未过分列",
    );

    // 知识库
    let mut kb = VulnKb::new();
    set.add("f142g cve format", kb.add("CVE-2026-999").is_err(), "编号非法拒绝");
    let _ = kb.add("CVE-2026-0142");
    set.add("f142g cve dup", kb.add("CVE-2026-0142").is_err(), "重复 CVE 拒绝");
    let _ = kb.advance("CVE-2026-0142", VulnStatus::Fixed);
    let _ = kb.advance("CVE-2026-0142", VulnStatus::Published);
    set.add("f142g vuln path", kb.advance("CVE-2026-0142", VulnStatus::Fixed).is_err(), "发布后不可回退");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn range_boundaries() {
        let r = AffectedRange::new(5, 5, 6).unwrap();
        assert_eq!(r.status_of(5), "受影响");
        assert_eq!(r.status_of(6), "已修复");
        assert_eq!(r.status_of(4), "不受影响");
    }

    #[test]
    fn priority_matrix_full() {
        // 40-69 分：pop1→P1，pop0→P2。
        assert_eq!(fix_priority(50, 1), 1);
        assert_eq!(fix_priority(50, 0), 2);
    }
}
