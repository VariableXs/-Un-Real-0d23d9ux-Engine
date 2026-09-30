//! 深化层四 · F134 主题分享页（2026-09-27 深化批次四 · g 层）。
//!
//! 主题元数据深校验（颜色令牌引用完整性）、兼容性矩阵（主题×内核版
//! 本）、下载排行周结算器、举报处理流水线状态机、作者认证分级。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 主题元数据深校验：引用的颜色令牌必须存在于令牌表（悬空引用检出）
// ---------------------------------------------------------------------------

/// 主题元数据：使用的令牌名清单（其余字段由基础批 schema 把关）。
pub struct ThemeMetaDeep {
    pub theme: &'static str,
    pub token_refs: alloc::vec::Vec<&'static str>,
}

/// 校验：返回悬空引用清单（token 表为登记名序列）。
pub fn dangling_tokens(m: &ThemeMetaDeep, token_table: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    m.token_refs.iter().filter(|t| !token_table.contains(t)).copied().collect()
}

/// 重复引用检出（同令牌引两次是死配置）。
pub fn duplicate_refs(m: &ThemeMetaDeep) -> bool {
    for (i, t) in m.token_refs.iter().enumerate() {
        if m.token_refs[i + 1..].contains(t) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 兼容性矩阵：主题 × 内核版本 → 测试结论；发布门要求当版全测
// ---------------------------------------------------------------------------

pub struct CompatCell {
    pub theme: &'static str,
    pub kernel_ver: u32,
    pub passed: Option<bool>, // None=未测
}

/// 发布门：给定版本，所有登记主题都已测且全过 → Ok；返回 (未测, 未过) 名单。
pub fn release_gate(cells: &[CompatCell], ver: u32) -> (alloc::vec::Vec<&'static str>, alloc::vec::Vec<&'static str>) {
    let mut untested: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    let mut failed: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for c in cells.iter().filter(|c| c.kernel_ver == ver) {
        match c.passed {
            None => untested.push(c.theme),
            Some(false) => failed.push(c.theme),
            Some(true) => {}
        }
    }
    (untested, failed)
}

// ---------------------------------------------------------------------------
// 下载排行周结算：日分桶 → 周窗口聚合 → TopN（平局主题号小者先）
// ---------------------------------------------------------------------------

pub struct DownloadDay {
    pub theme: u32,
    pub day: u32,
    pub count: u32,
}

/// 聚合 [week_start, week_start+7) 的每主题总量，降序取前 n。
pub fn weekly_top(rows: &[DownloadDay], week_start: u32, n: usize) -> alloc::vec::Vec<(u32, u32)> {
    let mut agg: alloc::vec::Vec<(u32, u32)> = alloc::vec::Vec::new();
    for r in rows.iter().filter(|r| r.day >= week_start && r.day < week_start + 7) {
        match agg.iter_mut().find(|(t, _)| *t == r.theme) {
            Some(e) => e.1 += r.count,
            None => agg.push((r.theme, r.count)),
        }
    }
    for i in 1..agg.len() {
        let k = agg[i];
        let mut j = i;
        while j > 0
            && (agg[j - 1].1 < k.1 || (agg[j - 1].1 == k.1 && agg[j - 1].0 > k.0))
        {
            agg[j] = agg[j - 1];
            j -= 1;
        }
        agg[j] = k;
    }
    agg.truncate(n);
    agg
}

// ---------------------------------------------------------------------------
// 举报处理流水线：Filed→Triaged→Actioned→Closed；过期自动升级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportStage {
    Filed,
    Triaged,
    Actioned,
    Closed,
}

pub struct ReportPipeline {
    pub id: u32,
    pub stage: ReportStage,
    pub filed_day: u32,
}

pub const TRIAGE_SLA_DAYS: u32 = 7;

impl ReportPipeline {
    pub fn advance(&mut self, to: ReportStage) -> Result<(), &'static str> {
        let legal = match (self.stage, to) {
            (ReportStage::Filed, ReportStage::Triaged)
            | (ReportStage::Triaged, ReportStage::Actioned)
            | (ReportStage::Actioned, ReportStage::Closed)
            | (ReportStage::Filed, ReportStage::Closed) => true, // 无效举报直关
            _ => false,
        };
        if !legal {
            return Err("非法举报迁移");
        }
        self.stage = to;
        Ok(())
    }

    /// 分诊逾期：Filed 且超 7 天。
    pub fn triage_overdue(&self, today: u32) -> bool {
        self.stage == ReportStage::Filed && today > self.filed_day + TRIAGE_SLA_DAYS
    }
}

// ---------------------------------------------------------------------------
// 作者认证分级：上传数 × 下载量 × 违规数 → 等级与权益
// ---------------------------------------------------------------------------

pub struct AuthorStats {
    pub uploads: u32,
    pub total_downloads: u32,
    pub violations: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthorTier {
    New,
    Trusted,
    Featured,
    Banned,
}

impl AuthorStats {
    pub fn tier(&self) -> AuthorTier {
        if self.violations >= 3 {
            return AuthorTier::Banned;
        }
        if self.uploads >= 10 && self.total_downloads >= 1000 && self.violations == 0 {
            AuthorTier::Featured
        } else if self.uploads >= 3 && self.violations == 0 {
            AuthorTier::Trusted
        } else {
            AuthorTier::New
        }
    }

    /// Featured 可进首页陈列；Banned 全部下架待复审。
    pub fn showcase_eligible(&self) -> bool {
        self.tier() == AuthorTier::Featured
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F134G_TAG: &str = "stareco-F134-deep4";

pub fn run_f134_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F134G_TAG);

    // 令牌引用
    let table = ["bg", "fg", "accent"];
    let good = ThemeMetaDeep { theme: "t1", token_refs: alloc::vec!["bg", "accent"] };
    let dangly = ThemeMetaDeep { theme: "t2", token_refs: alloc::vec!["bg", "ghost"] };
    set.add("f134g tokens ok", dangling_tokens(&good, &table).is_empty(), "引用全在表");
    set.add(
        "f134g tokens dangle",
        dangling_tokens(&dangly, &table) == alloc::vec!["ghost"],
        "悬空引用点名",
    );
    let dup = ThemeMetaDeep { theme: "t3", token_refs: alloc::vec!["bg", "bg"] };
    set.add("f134g dup ref", duplicate_refs(&dup) && !duplicate_refs(&good), "重复引用检出");

    // 兼容矩阵
    let cells = [
        CompatCell { theme: "a", kernel_ver: 7, passed: Some(true) },
        CompatCell { theme: "b", kernel_ver: 7, passed: None },
        CompatCell { theme: "c", kernel_ver: 7, passed: Some(false) },
        CompatCell { theme: "a", kernel_ver: 6, passed: Some(true) },
    ];
    let (unt, fail) = release_gate(&cells, 7);
    set.add(
        "f134g gate",
        unt == alloc::vec!["b"] && fail == alloc::vec!["c"],
        "未测/未过分列",
    );
    set.add("f134g gate old ver", release_gate(&cells, 6) == (alloc::vec![], alloc::vec![]), "旧版全绿");

    // 周结算
    let rows = [
        DownloadDay { theme: 1, day: 100, count: 5 },
        DownloadDay { theme: 2, day: 101, count: 9 },
        DownloadDay { theme: 1, day: 103, count: 6 },
        DownloadDay { theme: 3, day: 200, count: 99 }, // 窗外
    ];
    let top = weekly_top(&rows, 100, 2);
    set.add(
        "f134g top",
        top == alloc::vec![(1, 11), (2, 9)],
        "窗口聚合降序截断",
    );

    // 举报流水线
    let mut r = ReportPipeline { id: 1, stage: ReportStage::Filed, filed_day: 100 };
    set.add("f134g overdue", !r.triage_overdue(107) && r.triage_overdue(108), "分诊 7 天线");
    let _ = r.advance(ReportStage::Triaged);
    set.add("f134g not overdue", !r.triage_overdue(200), "分诊后不再逾期");
    set.add("f134g skip reject", r.advance(ReportStage::Closed).is_err(), "跳步拒绝");
    let _ = r.advance(ReportStage::Actioned);
    let _ = r.advance(ReportStage::Closed);
    set.add("f134g closed", r.stage == ReportStage::Closed, "全链走通");
    let mut r2 = ReportPipeline { id: 2, stage: ReportStage::Filed, filed_day: 1 };
    set.add("f134g direct close", r2.advance(ReportStage::Closed).is_ok(), "无效举报直关合法");

    // 作者分级
    let a1 = AuthorStats { uploads: 1, total_downloads: 10, violations: 0 };
    let a2 = AuthorStats { uploads: 4, total_downloads: 10, violations: 0 };
    let a3 = AuthorStats { uploads: 12, total_downloads: 2000, violations: 0 };
    let a4 = AuthorStats { uploads: 12, total_downloads: 2000, violations: 3 };
    set.add(
        "f134g author tiers",
        a1.tier() == AuthorTier::New
            && a2.tier() == AuthorTier::Trusted
            && a3.tier() == AuthorTier::Featured
            && a4.tier() == AuthorTier::Banned,
        "四级判定",
    );
    set.add("f134g showcase", a3.showcase_eligible() && !a2.showcase_eligible(), "陈列权益");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn weekly_top_tie_by_id() {
        let rows = [
            DownloadDay { theme: 7, day: 1, count: 5 },
            DownloadDay { theme: 3, day: 2, count: 5 },
        ];
        assert_eq!(weekly_top(&rows, 1, 2), alloc::vec![(3, 5), (7, 5)]);
    }

    #[test]
    fn direct_close_only_from_filed() {
        let mut r = ReportPipeline { id: 1, stage: ReportStage::Triaged, filed_day: 1 };
        assert!(r.advance(ReportStage::Closed).is_err());
        // Triaged 后必须走 Actioned。
        assert!(r.advance(ReportStage::Actioned).is_ok());
    }
}
