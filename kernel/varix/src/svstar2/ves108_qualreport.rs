// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves108_qualreport
// VE-F3708 · 主题质量报告（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3708
// 判据原文：报告结构、人话呈现、分发对路、家族管道、判据。
//
// 难点分析：
//   1. 人话呈现是三要素：说清问题在哪、为什么是问题、怎么改。
//   2. 报告分发对路：创作者看自己的/运营看全局，越权分发即隐私边界。
//   3. 报告结构必须包含五维评分+问题清单+改进建议+历史对比四区。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9C00 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9C00;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C9CCode(pub u16);

impl C9CCode {
    pub const STRUCT_INCOMPLETE: Self = Self(SEG_BASE);  // 结构不完整
    pub const JARGON_HEAVY: Self = Self(SEG_BASE + 1);   // 非人话
    pub const DISTRIBUTION_LEAK: Self = Self(SEG_BASE + 2); // 分发越权
    pub const SECTION_MISSING: Self = Self(SEG_BASE + 3); // 区段缺失
    pub const REPORT_READY: Self = Self(SEG_BASE + 4);   // 报告就绪
    pub const HUMAN_READABLE: Self = Self(SEG_BASE + 5); // 人话合规
    pub const DISTRIBUTION_OK: Self = Self(SEG_BASE + 6); // 分发合规

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::STRUCT_INCOMPLETE => "报告结构不完整——四区全量要求",
            Self::JARGON_HEAVY => "报告非人话——三要素缺失",
            Self::DISTRIBUTION_LEAK => "报告分发越权——隐私边界",
            Self::SECTION_MISSING => "报告区段缺失",
            Self::REPORT_READY => "报告就绪",
            Self::HUMAN_READABLE => "人话呈现合规",
            Self::DISTRIBUTION_OK => "分发合规",
            _ => "未知诊断",
        }
    }
}

// ── 报告结构 ──────────────────────────────────────────────────────

/// 报告区段（四区全量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    /// 五维评分区。
    Scores,
    /// 问题清单区（分级）。
    Issues,
    /// 改进建议区。
    Suggestions,
    /// 历史对比区。
    History,
}

impl Section {
    pub fn name(self) -> &'static str {
        match self {
            Self::Scores => "五维评分",
            Self::Issues => "问题清单",
            Self::Suggestions => "改进建议",
            Self::History => "历史对比",
        }
    }

    pub fn all() -> [Section; 4] {
        [Self::Scores, Self::Issues, Self::Suggestions, Self::History]
    }
}

/// 报告结构检查器。
pub struct ReportStructure {
    present: [bool; 4],
}

impl ReportStructure {
    pub fn new() -> Self {
        Self { present: [false; 4] }
    }

    fn idx(s: Section) -> usize {
        match s {
            Section::Scores => 0,
            Section::Issues => 1,
            Section::Suggestions => 2,
            Section::History => 3,
        }
    }

    pub fn mark(&mut self, s: Section) {
        self.present[Self::idx(s)] = true;
    }

    /// 全区齐备？
    pub fn is_complete(&self) -> bool {
        self.present.iter().all(|&b| b)
    }

    /// 检查。
    pub fn check(&self) -> Result<(), C9CCode> {
        if self.is_complete() {
            Ok(())
        } else {
            Err(C9CCode::STRUCT_INCOMPLETE)
        }
    }

    pub fn missing_count(&self) -> usize {
        self.present.iter().filter(|&&b| !b).count()
    }
}

impl Default for ReportStructure {
    fn default() -> Self {
        Self::new()
    }
}

// ── 人话呈现 ──────────────────────────────────────────────────────

/// 问题条目（三要素）。
pub struct IssueEntry {
    /// 问题在哪。
    pub where_: &'static str,
    /// 为什么是问题。
    pub why: &'static str,
    /// 怎么改。
    pub how: &'static str,
}

impl IssueEntry {
    pub fn new(where_: &'static str, why: &'static str, how: &'static str) -> Self {
        Self { where_, why, how }
    }

    /// 三要素齐备？
    pub fn is_human_readable(&self) -> bool {
        !self.where_.is_empty() && !self.why.is_empty() && !self.how.is_empty()
    }
}

/// 人话审计器。
pub struct HumanReadabilityAuditor {
    entries: Vec<IssueEntry>,
    jargon_hits: u32,
}

impl HumanReadabilityAuditor {
    pub fn new() -> Self {
        Self { entries: Vec::new(), jargon_hits: 0 }
    }

    pub fn add(&mut self, e: IssueEntry) {
        self.entries.push(e);
    }

    /// 全量审计。
    pub fn audit(&mut self) -> Result<(), C9CCode> {
        self.jargon_hits = 0;
        for e in &self.entries {
            if !e.is_human_readable() {
                self.jargon_hits = self.jargon_hits.saturating_add(1);
            }
        }
        if self.jargon_hits > 0 {
            return Err(C9CCode::JARGON_HEAVY);
        }
        Ok(())
    }

    pub fn jargon_hits(&self) -> u32 {
        self.jargon_hits
    }
}

impl Default for HumanReadabilityAuditor {
    fn default() -> Self {
        Self::new()
    }
}

// ── 报告分发 ──────────────────────────────────────────────────────

/// 分发目标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Audience {
    /// 创作者（只看自己）。
    Creator,
    /// 运营（看全局）。
    Operator,
}

impl Audience {
    pub fn name(self) -> &'static str {
        match self {
            Self::Creator => "创作者",
            Self::Operator => "运营",
        }
    }
}

/// 分发器——越权即隐私边界。
pub struct DistributionRouter {
    /// creator_id → 可见 theme_id 列表。
    ownership: Vec<(u64, u64)>,
    leaks: u32,
}

impl DistributionRouter {
    pub fn new() -> Self {
        Self { ownership: Vec::new(), leaks: 0 }
    }

    /// 登记归属。
    pub fn own(&mut self, creator_id: u64, theme_id: u64) {
        self.ownership.push((creator_id, theme_id));
    }

    /// 分发检查：Creator 只能看自己的 theme。
    pub fn check(&mut self, audience: Audience, viewer_id: u64, theme_id: u64) -> Result<(), C9CCode> {
        match audience {
            Audience::Operator => Ok(()),
            Audience::Creator => {
                for (cid, tid) in &self.ownership {
                    if *cid == viewer_id && *tid == theme_id {
                        return Ok(());
                    }
                }
                self.leaks = self.leaks.saturating_add(1);
                Err(C9CCode::DISTRIBUTION_LEAK)
            }
        }
    }

    pub fn leaks(&self) -> u32 {
        self.leaks
    }
}

impl Default for DistributionRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves108_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves108_qualreport");
    {
        // 01 四区全量
        chk(&mut set, "C39C-01", Section::all().len() == 4, "评分/问题/建议/历史四区");
    }
    {
        // 02 结构不完整被拒
        let mut rs = ReportStructure::new();
        rs.mark(Section::Scores);
        rs.mark(Section::Issues);
        chk(
            &mut set,
            "C39C-02",
            rs.check().err() == Some(C9CCode::STRUCT_INCOMPLETE) && rs.missing_count() == 2,
            "缺 2 区→结构不完整",
        );
    }
    {
        // 03 结构完整
        let mut rs = ReportStructure::new();
        for s in Section::all() {
            rs.mark(s);
        }
        chk(&mut set, "C39C-03", rs.check().is_ok() && rs.is_complete(), "四区齐备");
    }
    {
        // 04 三要素齐备
        let e = IssueEntry::new("对比度卡片 3", "低于 4.5:1 阈值", "提高前景亮度 20%");
        chk(&mut set, "C39C-04", e.is_human_readable(), "哪里/为什么/怎么改三要素");
    }
    {
        // 05 缺要素被拒
        let e = IssueEntry::new("对比度卡片 3", "", "提高亮度");
        chk(&mut set, "C39C-05", !e.is_human_readable(), "缺「为什么」不算人话");
    }
    {
        // 06 人话审计
        let mut a = HumanReadabilityAuditor::new();
        a.add(IssueEntry::new("a", "b", "c"));
        a.add(IssueEntry::new("x", "", "z"));
        let r = a.audit();
        chk(
            &mut set,
            "C39C-06",
            r.err() == Some(C9CCode::JARGON_HEAVY) && a.jargon_hits() == 1,
            "1 条非人话→审计失败",
        );
    }
    {
        // 07 全人话通过
        let mut a = HumanReadabilityAuditor::new();
        a.add(IssueEntry::new("a", "b", "c"));
        a.add(IssueEntry::new("x", "y", "z"));
        chk(&mut set, "C39C-07", a.audit().is_ok(), "全人话审计通过");
    }
    {
        // 08 创作者越权被拒
        let mut dr = DistributionRouter::new();
        dr.own(1, 100);
        let r = dr.check(Audience::Creator, 2, 100); // 创作者 2 看 theme 100（属 1）
        chk(
            &mut set,
            "C39C-08",
            r.err() == Some(C9CCode::DISTRIBUTION_LEAK) && dr.leaks() == 1,
            "创作者越权看他人主题→隐私边界",
        );
    }
    {
        // 09 创作者看自己 OK
        let mut dr = DistributionRouter::new();
        dr.own(1, 100);
        chk(&mut set, "C39C-09", dr.check(Audience::Creator, 1, 100).is_ok(), "创作者看自己主题");
    }
    {
        // 10 运营看全局 OK
        let mut dr = DistributionRouter::new();
        dr.own(1, 100);
        chk(&mut set, "C39C-10", dr.check(Audience::Operator, 99, 100).is_ok(), "运营看全局不越权");
    }
    {
        // 11 空报告不完整
        let rs = ReportStructure::new();
        chk(&mut set, "C39C-11", !rs.is_complete() && rs.missing_count() == 4, "空报告缺 4 区");
    }
    {
        // 12 分发合规计
        let mut dr = DistributionRouter::new();
        dr.own(1, 100);
        assert!(dr.check(Audience::Creator, 1, 100).is_ok());
        assert!(dr.check(Audience::Operator, 2, 100).is_ok());
        assert!(dr.check(Audience::Creator, 3, 100).is_err());
        chk(&mut set, "C39C-12", dr.leaks() == 1, "1 次越权 1 计");
    }
    set
}
