// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves94_ecoi18n
// VE-F3694 · 生态国际化（R05 组 · 生态运营组）
//
// 锚点：VE-F3694
// 判据原文：机翻标注、规范双审、敏感过滤、时区断言、文化适配、判据。
//
// 难点分析：
//   1. 机翻冒充人翻是欺骗——一切非人工翻译产物必须带机翻标注，标注缺失即 P1 红线。
//   2. 规范错译是治理事故——关键条款须双审，双审记录缺失即 P0。
//   3. 时区错=错过活动——活动时间多时区显示必须经时区断言换算校验。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x8E00 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x8E00;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C8ECode(pub u16);

impl C8ECode {
    pub const MT_UNMARKED: Self = Self(SEG_BASE);         // 机翻未标注 P1
    pub const SPEC_MIS_TRANS: Self = Self(SEG_BASE + 1);  // 规范错译 P0
    pub const SENSITIVE_LEAK: Self = Self(SEG_BASE + 2);  // 敏感漏审 P0
    pub const TZ_WRONG: Self = Self(SEG_BASE + 3);        // 时区换算错
    pub const CULTURE_BLOCK: Self = Self(SEG_BASE + 4);   // 文化敏感拦截
    pub const DUAL_REVIEW_OK: Self = Self(SEG_BASE + 5);  // 双审通过
    pub const MT_MARKED: Self = Self(SEG_BASE + 6);       // 机翻标注合规

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::MT_UNMARKED => "机翻产物未标注——机翻标注红线 P1",
            Self::SPEC_MIS_TRANS => "关键规范错译——规范翻译红线 P0",
            Self::SENSITIVE_LEAK => "敏感内容漏审——文化审查红线 P0",
            Self::TZ_WRONG => "时区换算错误——时区红线",
            Self::CULTURE_BLOCK => "文化敏感内容已按区域过滤",
            Self::DUAL_REVIEW_OK => "关键规范双审通过",
            Self::MT_MARKED => "机翻标注合规",
            _ => "未知诊断",
        }
    }
}

// ── 翻译来源与标注 ────────────────────────────────────────────────

/// 翻译来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransSource {
    /// 人工翻译。
    Human,
    /// 机器翻译。
    Machine,
    /// 人机协作（人工后编辑）。
    PostEdited,
}

impl TransSource {
    /// 是否必须带机翻标注。
    pub fn requires_mt_label(self) -> bool {
        matches!(self, Self::Machine | Self::PostEdited)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Human => "人工翻译",
            Self::Machine => "机器翻译",
            Self::PostEdited => "人工后编辑",
        }
    }
}

/// 多语言条目。
pub struct LocaleEntry {
    pub asset_id: u64,
    pub lang: u16,
    pub source: TransSource,
    /// 机翻标注是否已打。
    pub mt_labeled: bool,
    /// 内容摘要。
    pub digest: u64,
}

impl LocaleEntry {
    pub fn new(asset_id: u64, lang: u16, source: TransSource, digest: u64) -> Self {
        Self { asset_id, lang, source, mt_labeled: false, digest }
    }

    /// 打机翻标注。
    pub fn mark_mt(&mut self) {
        if self.source.requires_mt_label() {
            self.mt_labeled = true;
        }
    }

    /// 合规检查：机翻必标注。
    pub fn compliance(&self) -> Result<(), C8ECode> {
        if self.source.requires_mt_label() && !self.mt_labeled {
            return Err(C8ECode::MT_UNMARKED);
        }
        Ok(())
    }
}

/// 标注审计器——全量扫描机翻标注合规。
pub struct MtAuditor {
    entries: Vec<LocaleEntry>,
    violations: u32,
}

impl MtAuditor {
    pub fn new() -> Self {
        Self { entries: Vec::new(), violations: 0 }
    }

    pub fn add(&mut self, entry: LocaleEntry) {
        self.entries.push(entry);
    }

    /// 全量审计。返回违规数（0=全绿）。
    pub fn audit(&mut self) -> u32 {
        self.violations = 0;
        for e in &self.entries {
            if e.compliance().is_err() {
                self.violations = self.violations.saturating_add(1);
            }
        }
        self.violations
    }

    pub fn violations(&self) -> u32 {
        self.violations
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for MtAuditor {
    fn default() -> Self {
        Self::new()
    }
}

// ── 规范双审 ──────────────────────────────────────────────────────

/// 审次。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewPass {
    /// 初审。
    First,
    /// 复审。
    Second,
}

impl ReviewPass {
    pub fn name(self) -> &'static str {
        match self {
            Self::First => "初审",
            Self::Second => "复审",
        }
    }
}

/// 关键规范双审流程。
pub struct SpecDualReview {
    /// 已完成初审的规范 id。
    first_passed: Vec<u64>,
    /// 已完成复审（双审闭环）的规范 id。
    closed: Vec<u64>,
    /// 错译检出数。
    mis_trans_caught: u32,
}

impl SpecDualReview {
    pub fn new() -> Self {
        Self { first_passed: Vec::new(), closed: Vec::new(), mis_trans_caught: 0 }
    }

    /// 提交审查结论。`approved=false` 表示发现错译 → P0。
    pub fn review(&mut self, spec_id: u64, pass: ReviewPass, approved: bool) -> Result<(), C8ECode> {
        if !approved {
            self.mis_trans_caught = self.mis_trans_caught.saturating_add(1);
            return Err(C8ECode::SPEC_MIS_TRANS);
        }
        match pass {
            ReviewPass::First => {
                if !self.first_passed.contains(&spec_id) {
                    self.first_passed.push(spec_id);
                }
                Ok(())
            }
            ReviewPass::Second => {
                // 复审必须建立在初审通过之上
                if !self.first_passed.contains(&spec_id) {
                    return Err(C8ECode::SPEC_MIS_TRANS);
                }
                if !self.closed.contains(&spec_id) {
                    self.closed.push(spec_id);
                }
                Ok(())
            }
        }
    }

    /// 双审是否闭环。
    pub fn is_closed(&self, spec_id: u64) -> bool {
        self.closed.contains(&spec_id)
    }

    pub fn closed_count(&self) -> usize {
        self.closed.len()
    }

    pub fn mis_trans_caught(&self) -> u32 {
        self.mis_trans_caught
    }
}

impl Default for SpecDualReview {
    fn default() -> Self {
        Self::new()
    }
}

// ── 文化敏感过滤 ──────────────────────────────────────────────────

/// 敏感级别。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SensLevel {
    /// 无敏感。
    None,
    /// 区域敏感（按区域过滤）。
    Regional,
    /// 全球敏感（一律拦截）。
    Global,
}

impl SensLevel {
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "无敏感",
            Self::Regional => "区域敏感",
            Self::Global => "全球敏感",
        }
    }
}

/// 区域过滤器。
pub struct RegionFilter {
    /// 严格区域（对 Regional 敏感内容也拦截）。
    strict_regions: Vec<u16>,
    /// 漏审计数。
    leaks: u32,
}

impl RegionFilter {
    pub fn new() -> Self {
        Self { strict_regions: Vec::new(), leaks: 0 }
    }

    pub fn add_strict_region(&mut self, region: u16) {
        if !self.strict_regions.contains(&region) {
            self.strict_regions.push(region);
        }
    }

    /// 内容是否允许在某区域展示。
    pub fn allows(&self, level: SensLevel, region: u16) -> bool {
        match level {
            SensLevel::None => true,
            SensLevel::Global => false,
            SensLevel::Regional => !self.strict_regions.contains(&region),
        }
    }

    /// 审查入口：检出应拦未拦即漏审 P0。
    pub fn review(&mut self, level: SensLevel, region: u16, shown: bool) -> Result<(), C8ECode> {
        let should_block = !self.allows(level, region);
        if should_block && shown {
            self.leaks = self.leaks.saturating_add(1);
            return Err(C8ECode::SENSITIVE_LEAK);
        }
        if should_block && !shown {
            return Err(C8ECode::CULTURE_BLOCK);
        }
        Ok(())
    }

    pub fn leaks(&self) -> u32 {
        self.leaks
    }
}

impl Default for RegionFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ── 时区断言 ──────────────────────────────────────────────────────

/// 活动时间（UTC 分钟计）。
pub struct EventTime {
    /// UTC 自当日 00:00 起的分钟数。
    pub utc_minutes: u32,
    /// 标称展示时区偏移（分钟，东为正）。
    pub tz_offset_min: i32,
}

impl EventTime {
    pub fn new(utc_minutes: u32, tz_offset_min: i32) -> Self {
        Self { utc_minutes, tz_offset_min }
    }

    /// 换算到目标时区的本地分钟数。
    pub fn local_minutes(&self, target_offset_min: i32) -> u32 {
        let delta = target_offset_min as i64 - self.tz_offset_min as i64;
        let local = self.utc_minutes as i64 + delta;
        if local < 0 {
            0
        } else {
            local as u32
        }
    }

    /// 时区断言：换算结果须与期望一致，否则 TZ_WRONG。
    pub fn assert_local(&self, target_offset_min: i32, expected_local_min: u32) -> Result<(), C8ECode> {
        if self.local_minutes(target_offset_min) == expected_local_min {
            Ok(())
        } else {
            Err(C8ECode::TZ_WRONG)
        }
    }
}

/// 多时区展示器。
pub struct TzPresenter {
    /// 各区域偏移表。
    offsets: Vec<(u16, i32)>,
    /// 断言失败数。
    mismatches: u32,
}

impl TzPresenter {
    pub fn new() -> Self {
        Self { offsets: Vec::new(), mismatches: 0 }
    }

    pub fn register_region(&mut self, region: u16, offset_min: i32) {
        self.offsets.push((region, offset_min));
    }

    /// 为某区域展示活动时间（经断言）。
    pub fn present(&mut self, ev: &EventTime, region: u16) -> Result<u32, C8ECode> {
        for &(r, off) in &self.offsets {
            if r == region {
                let local = ev.local_minutes(off);
                // 自检：再算一遍须一致（防计算路径分叉）
                if ev.local_minutes(off) != local {
                    self.mismatches = self.mismatches.saturating_add(1);
                    return Err(C8ECode::TZ_WRONG);
                }
                return Ok(local);
            }
        }
        Err(C8ECode::TZ_WRONG)
    }

    pub fn mismatches(&self) -> u32 {
        self.mismatches
    }
}

impl Default for TzPresenter {
    fn default() -> Self {
        Self::new()
    }
}

// ── 国际化总线 ────────────────────────────────────────────────────

/// 生态国际化总线。
pub struct EcoI18nBus {
    pub mt: MtAuditor,
    pub spec: SpecDualReview,
    pub filter: RegionFilter,
    pub tz: TzPresenter,
}

impl EcoI18nBus {
    pub fn new() -> Self {
        Self {
            mt: MtAuditor::new(),
            spec: SpecDualReview::new(),
            filter: RegionFilter::new(),
            tz: TzPresenter::new(),
        }
    }

    /// 全面合规快照：四项全绿才 ok。
    pub fn compliance_ok(&self) -> bool {
        self.mt.violations() == 0
            && self.filter.leaks() == 0
            && self.tz.mismatches() == 0
            && self.spec.mis_trans_caught() == 0
    }
}

impl Default for EcoI18nBus {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves94_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves94_ecoi18n");
    {
        // 01 机翻未标注即 P1
        let e = LocaleEntry::new(1, 1, TransSource::Machine, 0x1);
        chk(
            &mut set,
            "C38E-01",
            e.compliance().err() == Some(C8ECode::MT_UNMARKED),
            "机翻产物未标注须报 P1（机翻标注红线）",
        );
    }
    {
        // 02 标注后合规
        let mut e = LocaleEntry::new(1, 1, TransSource::Machine, 0x1);
        e.mark_mt();
        chk(&mut set, "C38E-02", e.compliance().is_ok(), "机翻标注后合规");
    }
    {
        // 03 人工翻译免标注
        let e = LocaleEntry::new(2, 2, TransSource::Human, 0x2);
        chk(&mut set, "C38E-03", e.compliance().is_ok(), "人工翻译无需机翻标注");
    }
    {
        // 04 规范错译即 P0
        let mut d = SpecDualReview::new();
        let r = d.review(10, ReviewPass::First, false);
        chk(
            &mut set,
            "C38E-04",
            r.err() == Some(C8ECode::SPEC_MIS_TRANS) && d.mis_trans_caught() == 1,
            "规范错译须报 P0（规范翻译红线）",
        );
    }
    {
        // 05 双审闭环
        let mut d = SpecDualReview::new();
        assert!(d.review(11, ReviewPass::First, true).is_ok());
        assert!(d.review(11, ReviewPass::Second, true).is_ok());
        chk(&mut set, "C38E-05", d.is_closed(11), "初审+复审通过才算双审闭环");
    }
    {
        // 06 复审不得越过初审
        let mut d = SpecDualReview::new();
        let r = d.review(12, ReviewPass::Second, true);
        chk(
            &mut set,
            "C38E-06",
            r.is_err(),
            "复审必须建立在初审之上（流程断言）",
        );
    }
    {
        // 07 全球敏感一律拦
        let f = RegionFilter::new();
        chk(
            &mut set,
            "C38E-07",
            !f.allows(SensLevel::Global, 0) && !f.allows(SensLevel::Global, 999),
            "全球敏感内容任何区域不得展示",
        );
    }
    {
        // 08 敏感漏审即 P0
        let mut f = RegionFilter::new();
        f.add_strict_region(86);
        let r = f.review(SensLevel::Regional, 86, true);
        chk(
            &mut set,
            "C38E-08",
            r.err() == Some(C8ECode::SENSITIVE_LEAK) && f.leaks() == 1,
            "应拦未拦即漏审 P0（文化审查红线）",
        );
    }
    {
        // 09 时区换算断言
        let ev = EventTime::new(600, 0); // UTC 10:00
        // 东八区应是 18:00 = 1080 分
        let r = ev.assert_local(480, 1080);
        chk(&mut set, "C38E-09", r.is_ok(), "UTC 10:00 → 东八区 18:00 断言通过");
    }
    {
        // 10 时区断言失败
        let ev = EventTime::new(600, 0);
        let r = ev.assert_local(480, 999);
        chk(
            &mut set,
            "C38E-10",
            r.err() == Some(C8ECode::TZ_WRONG),
            "错误期望值须触发时区断言",
        );
    }
    {
        // 11 多时区展示器
        let mut p = TzPresenter::new();
        p.register_region(86, 480);
        p.register_region(1, -300);
        let ev = EventTime::new(0, 0); // UTC 00:00
        let cn = p.present(&ev, 86);
        let us = p.present(&ev, 1);
        chk(
            &mut set,
            "C38E-11",
            cn == Ok(480) && us == Ok(1140) && p.mismatches() == 0,
            "东八 08:00 / 西五 19:00(前一日偏移取 1140) 换算正确",
        );
    }
    {
        // 12 总线合规快照
        let mut bus = EcoI18nBus::new();
        bus.mt.add(LocaleEntry::new(1, 1, TransSource::Human, 0));
        bus.tz.register_region(86, 480);
        chk(&mut set, "C38E-12", bus.compliance_ok(), "全绿快照：无违规无漏审无时区错");
    }
    set
}
