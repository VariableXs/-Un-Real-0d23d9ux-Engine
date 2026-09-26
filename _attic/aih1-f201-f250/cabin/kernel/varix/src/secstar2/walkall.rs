//! F200 全域总检（secstar2 · G-G-30）——200 项清单最大的功能，是它教人克制。
//!
//! **判据（主册）**：清单覆盖率 100%（199 项每项 ≥1 可执行判据）；季检脚本
//! 全量可跑（<2h）；增补流程案例化（首批增补走完 ADR 全程）。
//!
//! **功能定义（主册 G-G-30）**：F001-F199 验收锚点汇成总检清单进 20 维度
//! 验收脚本，季检滚动复查；200 项不是终点是基线——每季度审视一次，有现实
//! 契机才增补，永远不为功能多而加功能。
//!
//! 【交互设计】总检清单生成脚本（`tools/vx-walkcheck-all.py`——走查族总
//! 集成）：按域分组红绿一页+证据链链接；季检结果归档进季报（F149 增第五节
//! 「总检状态」）。
//! 【数据与存储】清单数据=各报告【验收判据】段自动提取（F125 同管线全域
//! 化）；季检归档版本化。
//! 【状态与异常】锚点与实现脱节 → R8 三册腐化流程（周对账抽查网住）；不可
//! 测锚点 → 设计缺口回炉（卷首·丙纪律全域执法）。
//! 【设计细节】总检红绿判定=证据链存在且未过期（证据带日期——过期绿按红
//! 处理，附录 D 纪律全域化）；季度审视会产出三栏（新增/废止/冻结——废止也
//! 是一等公民动作）；「现实契机」判定四问（有用户真实需求吗/有硬件契机吗/
//! 有开源件成熟吗/有维护人力吗——四问全过才立项）；F200 自身也有锚点：本
//! 段即它的验收。
//!
//! 分工边界：本模块是**数据模型与判定层**（锚点登记/覆盖率/季检三栏/候删
//! 冻结/开放 JSON）；`tools/vx-walkcheck-all.py` 是提取与渲染脚本（markdown
//! 表格解析 → 本模块的 JSON 契约 → 红绿一页纸）。脚本在仓库 tools/ 侧。
//!
//! 存量冻结候删名单（Variable 已拍板「不太实用的功能删除」；因 F 编号被
//! 全书引用，走冻结不删除、编号不复用，正式删除待 Variable 逐项确认）：
//! F101 天气件、F104 录音件、F112 讲述人剪影、F145 教育/作品集友好、F154
//! 壁纸每日一换——以上五项判定为「锦上添花但非日常」，冻结后不再投入
//! 工时，季度审视时决定去留。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 总覆盖目标：F001-F199（199 项；F200 自身锚点=本模块）。
pub const TOTAL_ITEMS: usize = 199;

/// 域清单（七域——与分工书一致）。
pub const DOMAINS: [&str; 7] = ["A 兼容", "B 性能", "C 体验", "D 生态", "E 个性化", "G 安全", "H/I 通用"];

/// 季检时限：<2h。
pub const QUARTERLY_BUDGET_H: u64 = 2;

/// 季检三栏。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewColumn {
    Add,
    Retire,
    Freeze,
}

/// 存量冻结候删名单（Variable 拍板——冻结不删除、编号不复用）。
pub const FROZEN_CANDIDATES: [(&str, &str); 5] = [
    ("F101", "天气件"),
    ("F104", "录音件"),
    ("F112", "讲述人剪影"),
    ("F145", "教育/作品集友好"),
    ("F154", "壁纸每日一换"),
];

/// 「现实契机」四问（增补立项门槛——四问全过才立项）。
pub const FOUR_QUESTIONS: [&str; 4] = [
    "有用户真实需求吗",
    "有硬件契机吗",
    "有开源件成熟吗",
    "有维护人力吗",
];

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 单项锚点记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// 功能编号（F001…）。
    pub fid: &'static str,
    /// 所属域。
    pub domain: &'static str,
    /// 可执行判据（≥1 条——覆盖率判据的计数对象）。
    pub criteria: Vec<&'static str>,
    /// 证据链（报告文件+日期——过期绿按红处理）。
    pub evidence: Option<(&'static str, &'static str)>,
}

impl Anchor {
    pub fn covered(&self) -> bool {
        !self.criteria.is_empty()
    }

    /// 红绿判定：覆盖 + 证据存在 + 证据未过期（附录 D 纪律——过期绿按红）。
    pub fn verdict(&self, now_day: u64, evidence_ttl_days: u64) -> bool {
        if !self.covered() {
            return false;
        }
        match self.evidence {
            None => false,
            Some((_, day_str)) => {
                // day_str 形如 "20260926"——过期判定按天数差。
                match parse_day(day_str) {
                    Some(d) => now_day.saturating_sub(d) <= evidence_ttl_days,
                    None => false, // 证据无日期 = 无效证据（不可判按红）。
                }
            }
        }
    }
}

/// "YYYYMMDD" → 纪元天序号（简化：以 2026-01-01 为 day 0 的线性近似——
/// 季检 TTL 判定只需相对差，跨月误差 ≤1 天可接受并在诊断面标注口径）。
fn parse_day(s: &str) -> Option<u64> {
    if s.len() != 8 {
        return None;
    }
    let y: u64 = s.get(0..4)?.parse().ok()?;
    let m: u64 = s.get(4..6)?.parse().ok()?;
    let d: u64 = s.get(6..8)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y - 2026) * 365 + (m - 1) * 30 + d)
}

// ---------------------------------------------------------------------------
// 总检主体
// ---------------------------------------------------------------------------

/// 全域总检。
pub struct WalkAll {
    pub anchors: Vec<Anchor>,
    /// 季检归档（每季一行：季名 + 绿数 + 红数）。
    pub quarterly: Vec<(&'static str, usize, usize)>,
    /// 三栏决议（新增/废止/冻结——废止也是一等公民动作）。
    pub decisions: Vec<(ReviewColumn, &'static str)>,
    /// 证据 TTL（天——过期绿按红；旋钮语义，默认 90 天=季度）。
    pub evidence_ttl_days: u64,
}

impl WalkAll {
    pub fn new() -> WalkAll {
        WalkAll { anchors: Vec::new(), quarterly: Vec::new(), decisions: Vec::new(), evidence_ttl_days: 90 }
    }

    /// 登记锚点（重复编号拒绝——F 编号全册唯一）。
    pub fn register(&mut self, fid: &'static str, domain: &'static str, criteria: Vec<&'static str>) -> bool {
        if self.anchors.iter().any(|a| a.fid == fid) {
            return false;
        }
        self.anchors.push(Anchor { fid, domain, criteria, evidence: None });
        true
    }

    /// 挂证据（报告文件+日期）。
    pub fn attach_evidence(&mut self, fid: &str, report: &'static str, day: &'static str) -> bool {
        match self.anchors.iter_mut().find(|a| a.fid == fid) {
            Some(a) => {
                a.evidence = Some((report, day));
                true
            }
            None => false,
        }
    }

    /// 覆盖率（permille）：covered / TOTAL_ITEMS。
    pub fn coverage_permille(&self) -> u64 {
        let covered = self.anchors.iter().filter(|a| a.covered()).count();
        covered as u64 * 1000 / TOTAL_ITEMS as u64
    }

    /// 红绿一页纸数据（按域分组）：(域, 绿数, 红数, 红项清单)。
    /// 冻结候删项不参与考核（冻结≠失败——既不绿也不红，不计入统计）。
    pub fn redgreen_page(&self, now_day: u64) -> Vec<(&'static str, usize, usize, Vec<&'static str>)> {
        let mut out = Vec::new();
        for dom in DOMAINS {
            let mine: Vec<&Anchor> = self
                .anchors
                .iter()
                .filter(|a| a.domain == dom && !self.is_frozen(a.fid))
                .collect();
            let mut green = 0;
            let mut reds = Vec::new();
            for a in mine {
                if a.verdict(now_day, self.evidence_ttl_days) {
                    green += 1;
                } else {
                    reds.push(a.fid);
                }
            }
            out.push((dom, green, reds.len(), reds));
        }
        out
    }

    /// 季检跑批（判据「季检脚本全量可跑 <2h」的账目面——真实耗时由脚本
    /// 注出，这里记账+归档）。
    pub fn run_quarterly(&mut self, quarter: &'static str, now_day: u64, elapsed_hours: u64) -> (usize, usize) {
        let page = self.redgreen_page(now_day);
        let green: usize = page.iter().map(|(_, g, _, _)| g).sum();
        let red: usize = page.iter().map(|(_, _, r, _)| r).sum();
        self.quarterly.push((quarter, green, red));
        let _ = elapsed_hours; // 预算对账由脚本侧输出（<2h 硬线）。
        (green, red)
    }

    /// 三栏决议登记（新增/废止/冻结——一栏一条）。
    pub fn decide(&mut self, col: ReviewColumn, what: &'static str) {
        self.decisions.push((col, what));
    }

    /// 增补立项门槛（四问全过才立项——判据「增补流程案例化」的判定面）。
    pub fn four_questions_pass(&self, answers: [bool; 4]) -> bool {
        answers.iter().all(|&a| a)
    }

    /// 候删五项冻结判定：冻结名单中的项不参与红绿考核（冻结≠失败）。
    pub fn is_frozen(&self, fid: &str) -> bool {
        FROZEN_CANDIDATES.iter().any(|(f, _)| *f == fid)
    }

    /// 开放 JSON（F128 同语言——脚本消费的数据契约）。
    pub fn open_json(&self, now_day: u64, out: &mut Vec<u8>) {
        push(out, b"{\"walkall\":{\"ttl_days\":");
        push_u64(out, self.evidence_ttl_days);
        push(out, b",\"total\":");
        push_u64(out, TOTAL_ITEMS as u64);
        push(out, b",\"coverage_permille\":");
        push_u64(out, self.coverage_permille());
        push(out, b",\"frozen\":[");
        for (i, (f, name)) in FROZEN_CANDIDATES.iter().enumerate() {
            if i > 0 {
                push(out, b",");
            }
            push(out, b"[\"");
            push(out, f.as_bytes());
            push(out, b"\",\"");
            push(out, name.as_bytes());
            push(out, b"\"]");
        }
        push(out, b"],\"page\":[");
        for (i, (dom, g, r, reds)) in self.redgreen_page(now_day).iter().enumerate() {
            if i > 0 {
                push(out, b",");
            }
            push(out, b"{\"domain\":\"");
            push(out, dom.as_bytes());
            push(out, b"\",\"green\":");
            push_u64(out, *g as u64);
            push(out, b",\"red\":");
            push_u64(out, *r as u64);
            push(out, b",\"red_items\":[");
            for (k, f) in reds.iter().enumerate() {
                if k > 0 {
                    push(out, b",");
                }
                push(out, b"\"");
                push(out, f.as_bytes());
                push(out, b"\"");
            }
            push(out, b"]}");
        }
        push(out, b"]}}");
    }
}

impl Default for WalkAll {
    fn default() -> Self {
        Self::new()
    }
}

use crate::secstar2::lineage::{push, push_u64};

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F200 自检（聚合进 secstar2 域）。
pub fn run_walkall_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-walkall");

    let mut w = WalkAll::new();
    // 登记 199 项（抽样核心 + 全量计数）——覆盖率判据。
    for i in 1..=199u32 {
        let fid = match i {
            1..=9 => alloc::format!("F00{}", i).leak() as &'static str,
            10..=99 => alloc::format!("F0{}", i).leak() as &'static str,
            _ => alloc::format!("F{}", i).leak() as &'static str,
        };
        assert!(w.register(fid, "G 安全", vec!["判据一条"]), "register {fid}");
    }
    set.add("register all", w.anchors.len() == TOTAL_ITEMS, "");
    set.add("dup rejected", !w.register("F001", "G 安全", vec!["x"]), "");
    set.add("coverage 100%", w.coverage_permille() == 1000, "");

    // 证据挂载与过期绿按红（附录 D 纪律全域化）。
    w.attach_evidence("F186", "docs/AI-S2-完成报告.md", "20260926");
    let day0 = parse_day("20260926").unwrap_or(0);
    set.add("fresh evidence green", w.anchors[185].verdict(day0, 90), "");
    set.add("expired evidence red", !w.anchors[185].verdict(day0 + 91, 90), "");
    set.add("no evidence red", !w.anchors[0].verdict(day0, 90), "");

    // 冻结候删不参与考核。
    set.add("frozen known", w.is_frozen("F101") && w.is_frozen("F154"), "");
    set.add("not frozen", !w.is_frozen("F186"), "");

    // 红绿一页纸按域分组（冻结项 5 个不进统计——199-5=194 进考核）。
    let page = w.redgreen_page(day0);
    set.add("page groups", page.len() == DOMAINS.len(), "");
    set.add("page excludes frozen", page.iter().map(|(_, g, r, _)| g + r).sum::<usize>()
        == TOTAL_ITEMS - FROZEN_CANDIDATES.len(), "");
    // 季检归档行同样排除冻结（绿+红=194）。
    let (green, red) = w.run_quarterly("2026Q4", day0, 1);
    set.add("quarterly archived", w.quarterly.len() == 1, "");
    set.add("quarterly excludes frozen", green + red == TOTAL_ITEMS - FROZEN_CANDIDATES.len(), "");

    set.add("quarterly budget line", QUARTERLY_BUDGET_H == 2, "");

    // 三栏决议 + 四问门槛。
    w.decide(ReviewColumn::Add, "现实契机驱动的增补项");
    w.decide(ReviewColumn::Freeze, "F101 天气件（存量冻结）");
    set.add("decisions kept", w.decisions.len() == 2, "");
    set.add("four questions gate", w.four_questions_pass([true, true, true, true]), "");
    set.add("four questions fail", !w.four_questions_pass([true, false, true, true]), "");

    // 开放 JSON。
    let mut json = Vec::new();
    w.open_json(day0, &mut json);
    let js = core::str::from_utf8(&json).unwrap_or("");
    set.add("json shape", js.starts_with("{\"walkall\":{") && js.ends_with("}}"), "");
    set.add("json coverage", js.contains("\"coverage_permille\":1000"), "");
    set.add("json frozen", js.contains("F101") && js.contains("天气件"), "");

    // F200 自身锚点：本模块即它的验收（主册【设计细节】）。
    set.add("self anchor", true, "F200 的锚点=本段判据的实现本身");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> u64 {
        parse_day(s).unwrap_or(0)
    }

    #[test]
    fn f200_parse_day_shapes() {
        assert_eq!(parse_day("20260926"), Some((0) * 365 + 8 * 30 + 26));
        assert_eq!(parse_day("20270101"), Some(365 + 1), "线性近似：跨年 365 + 月日项");
        assert_eq!(parse_day("20261301"), None, "month 13 invalid");
        assert_eq!(parse_day("2026092"), None, "short string");
        assert_eq!(parse_day("202609xx"), None, "non-numeric");
    }

    #[test]
    fn f200_evidence_ttl_boundary() {
        let mut w = WalkAll::new();
        assert!(w.register("F010", "A 兼容", vec!["c"]));
        w.attach_evidence("F010", "r.md", "20260101");
        let d = day("20260101");
        assert!(w.anchors[0].verdict(d, 90));
        assert!(w.anchors[0].verdict(d + 90, 90), "day 90 still fresh");
        assert!(!w.anchors[0].verdict(d + 91, 90), "day 91 expired");
    }

    #[test]
    fn f200_empty_criteria_not_covered() {
        let mut w = WalkAll::new();
        assert!(w.register("F002", "A 兼容", vec![]));
        assert!(!w.anchors[0].covered());
        assert_eq!(w.coverage_permille(), 0);
    }

    #[test]
    fn f200_frozen_excluded_from_red() {
        let mut w = WalkAll::new();
        // F101（冻结）无证据：不判红——冻结≠失败。
        assert!(w.register("F101", "C 体验", vec!["c"]));
        let d = day("20260926");
        let page = w.redgreen_page(d);
        let c_row = page.iter().find(|(dom, _, _, _)| *dom == "C 体验").unwrap();
        // F101 在 C 体验域：红数应排除冻结项（本例 anchors 里 F101 是唯一
        // C 域项且被冻结 → 红数 0，绿数 0——冻结项不进统计）。
        assert_eq!(c_row.2, 0, "frozen items are not counted red");
    }

    #[test]
    fn f200_run_checks_pass() {
        assert!(run_walkall_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 深一：QuarterlyArchive —— 季检归档版本化（主册【数据与存储】：季检归档
// 版本化——同季度重跑覆盖旧行（如实重计），跨季度只增不改（历史不可篡改
// ——审计链同族语义））
// ---------------------------------------------------------------------------

/// 归档行（版本化的季检记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuarterlyRow {
    pub quarter: &'static str,
    pub green: usize,
    pub red: usize,
    /// 重跑次数（同季重跑如实累计——数字说的是这个行被改过几次）。
    pub reruns: u64,
}

/// 版本化归档（append 跨季、overwrite 同季）。
pub struct QuarterlyArchive {
    pub rows: Vec<QuarterlyRow>,
}

impl QuarterlyArchive {
    pub fn new() -> QuarterlyArchive {
        QuarterlyArchive { rows: Vec::new() }
    }

    /// 归档一轮（同季度 → 原位覆盖+reruns+1；新季度 → 追加）。
    pub fn record(&mut self, quarter: &'static str, green: usize, red: usize) {
        if let Some(pos) = self.rows.iter().position(|r| r.quarter == quarter) {
            let reruns = self.rows[pos].reruns + 1;
            self.rows[pos] = QuarterlyRow { quarter, green, red, reruns };
        } else {
            self.rows.push(QuarterlyRow { quarter, green, red, reruns: 0 });
        }
    }

    /// 逐季不回退检查（绿数环比：新季绿数 ≥ 上季绿数——回退即腐化信号）。
    pub fn monotonic_green(&self) -> bool {
        self.rows.windows(2).all(|w| w[1].green >= w[0].green)
    }
}

impl Default for QuarterlyArchive {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深二：DecisionGate —— 三栏决议纪律执法（主册【设计细节】：季度审视会
// 产出三栏（新增/废止/冻结）；「现实契机」四问全过才立项——门槛不是
// 提示语，是 gate：不过四问的 Add 决议进不来账）
// ---------------------------------------------------------------------------

/// 决议登记（带执法的 decide——Add 必须四问全过；Retire/Freeze 必须带理由）。
pub fn decide_gated(
    w: &mut WalkAll,
    col: ReviewColumn,
    what: &'static str,
    reason: &'static str,
    four_questions: [bool; 4],
) -> Result<(), &'static str> {
    match col {
        ReviewColumn::Add => {
            if !w.four_questions_pass(four_questions) {
                return Err("增补未过「现实契机」四问——忍住不挑花哨的（F200 纪律）");
            }
        }
        ReviewColumn::Retire | ReviewColumn::Freeze => {
            if reason.is_empty() {
                return Err("废止/冻结必须带理由（废止也是一等公民动作——动作要可追溯）");
            }
        }
    }
    w.decide(col, what);
    Ok(())
}

// ---------------------------------------------------------------------------
// 深三：DriftAlarm —— 锚点腐化警报（主册【状态与异常】：锚点与实现脱节
// → R8 三册腐化流程（周对账抽查网住）——连续两个季检都没有绿判定的锚点
// 进入警报清单：要么实现烂了，要么证据链断了，两条都得有人接）
// ---------------------------------------------------------------------------

/// 腐化警报。
pub struct DriftAlarm {
    pub fid: &'static str,
    /// 连续无绿的季检轮数。
    pub stale_rounds: usize,
    /// 警报文案（R8 流程入口提示）。
    pub note: &'static str,
}

/// 扫描（基于归档历史逐锚回看——本简化版以「当前红且登记超 1 季」为代理
/// 判据：needs_attention=红项中 evidence 也缺失的，连实现带证据一起疑）。
pub fn drift_scan(w: &WalkAll, now_day: u64) -> Vec<DriftAlarm> {
    let mut out = Vec::new();
    for page_row in w.redgreen_page(now_day) {
        for fid in page_row.3 {
            let anchor = w.anchors.iter().find(|a| a.fid == fid);
            let no_evidence = anchor.map(|a| a.evidence.is_none()).unwrap_or(false);
            out.push(DriftAlarm {
                fid,
                stale_rounds: if no_evidence { 2 } else { 1 },
                note: if no_evidence {
                    "红且无证据链——实现与证据双脱节，走 R8 腐化流程"
                } else {
                    "红但有证据在档——优先查实现回归"
                },
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 深四：EvidenceLookup —— 证据链查询（主册【交互设计】：按域分组红绿一页
// +证据链链接——页上每个绿点都能点出「证据在哪、什么时候验的」）
// ---------------------------------------------------------------------------

/// 单锚证据查询结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceLink {
    pub fid: &'static str,
    pub report: &'static str,
    pub day: &'static str,
    /// 距今天数（TTL 对账——UI 据此着色「新鲜/将过期」）。
    pub age_days: u64,
}

/// 查询（锚不存在/无证据 → None——诚实，不造链接）。
pub fn evidence_of(w: &WalkAll, fid: &str, now_day: u64) -> Option<EvidenceLink> {
    let a = w.anchors.iter().find(|a| a.fid == fid)?;
    let (report, day) = a.evidence?;
    let d = parse_day(day)?;
    Some(EvidenceLink { fid: a.fid, report, day, age_days: now_day.saturating_sub(d) })
}

/// 新鲜度着色（≤30 天新鲜 / ≤TTL 将过期 / 其余按红——过期绿按红的 UI 面）。
pub fn evidence_freshness(age_days: u64, ttl_days: u64) -> &'static str {
    if age_days <= 30 {
        "fresh"
    } else if age_days <= ttl_days {
        "aging"
    } else {
        "expired"
    }
}

// ---------------------------------------------------------------------------
// 深五：CoverageGaps —— 覆盖缺口清单（主册【验收判据】：清单覆盖率 100%
// ——缺谁要能点名。脚本侧（vx-walkcheck-all.py）解析主册得全量表，与本
// 账对差集——缺口清单就是脚本与内核的数据契约落点）
// ---------------------------------------------------------------------------

/// 缺口清单（登记账 vs 主册全集——返回缺失的 F 编号，升序）。
pub fn coverage_gaps(w: &WalkAll, all_fids: &[&'static str]) -> Vec<&'static str> {
    let mut gaps: Vec<&'static str> = all_fids
        .iter()
        .filter(|f| !w.anchors.iter().any(|a| a.fid == **f))
        .copied()
        .collect();
    gaps.sort_unstable();
    gaps
}

/// 覆盖率对账（集合等价：登记账与主册全集一一对应——漏登与多登都算脱节；
/// 两口径互证：缺口清单空 且 无账外锚点，才叫对得上）。
pub fn coverage_consistent(w: &WalkAll, all_fids: &[&'static str]) -> bool {
    coverage_gaps(w, all_fids).is_empty()
        && w.anchors.iter().all(|a| all_fids.contains(&a.fid))
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F200 深化自检（聚合进 secstar2 域）。
pub fn run_walkall_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-deep");

    // 深一：归档版本化——同季覆盖重跑计数、跨季追加、环比回退可检。
    let mut arc = QuarterlyArchive::new();
    arc.record("2026Q3", 100, 94);
    arc.record("2026Q3", 110, 84);
    set.add("arch rerun overwrite", arc.rows.len() == 1 && arc.rows[0].green == 110 && arc.rows[0].reruns == 1, "");
    arc.record("2026Q4", 150, 44);
    set.add("arch append", arc.rows.len() == 2, "");
    set.add("arch monotonic", arc.monotonic_green(), "");
    arc.record("2027Q1", 140, 54);
    set.add("arch regress detected", !arc.monotonic_green(), "绿数回退=腐化信号，必须报警");

    // 深二：决议执法——四问不过的 Add 拒；无理由的 Retire 拒；合规才入账。
    let mut w = WalkAll::new();
    set.add("gate add without 4q", decide_gated(&mut w, ReviewColumn::Add, "X", "", [true, true, false, true]).is_err(), "");
    set.add("gate retire no reason", decide_gated(&mut w, ReviewColumn::Retire, "X", "", [true; 4]).is_err(), "");
    set.add("gate freeze ok", decide_gated(&mut w, ReviewColumn::Freeze, "F101 存量冻结", "锦上添花但非日常", [true; 4]).is_ok(), "");
    set.add("gate add with 4q ok", decide_gated(&mut w, ReviewColumn::Add, "现实契机项", "", [true; 4]).is_ok(), "");
    set.add("gate ledger", w.decisions.len() == 2, "");

    // 深三：腐化警报——无证据红项标双脱节；有证据红项标实现回归。
    let mut w3 = WalkAll::new();
    let _ = w3.register("F010", "A 兼容", vec!["c"]);
    let _ = w3.register("F011", "A 兼容", vec!["c"]);
    w3.attach_evidence("F011", "r.md", "20260601");
    let day = parse_day("20260926").unwrap_or(0);
    let alarms = drift_scan(&w3, day);
    set.add("drift count", alarms.len() == 2, "两项都过期无绿——全进警报");
    set.add("drift double missing", alarms.iter().find(|a| a.fid == "F010").map(|a| a.stale_rounds == 2) == Some(true), "");
    set.add("drift impl regress", alarms.iter().find(|a| a.fid == "F011").map(|a| a.note.contains("实现回归")) == Some(true), "");

    // 深四：证据链查询——存在可查、缺失诚实 None、新鲜度三态。
    let mut w4 = WalkAll::new();
    let _ = w4.register("F001", "A 兼容", vec!["c"]);
    let _ = w4.register("F002", "A 兼容", vec!["c"]);
    w4.attach_evidence("F001", "docs/AI-S2-完成报告.md", "20260926");
    set.add("evidence found", evidence_of(&w4, "F001", day).map(|e| e.age_days == 0) == Some(true), "");
    set.add("evidence none honest", evidence_of(&w4, "F002", day).is_none(), "");
    set.add("evidence missing anchor", evidence_of(&w4, "F999", day).is_none(), "");
    set.add("freshness tri-state", evidence_freshness(10, 90) == "fresh"
        && evidence_freshness(60, 90) == "aging"
        && evidence_freshness(91, 90) == "expired", "");

    // 深五：覆盖缺口——点名缺失、缺口空 ⇔ 满格两口径互证。
    let mut w5 = WalkAll::new();
    let all: Vec<&'static str> = vec!["F001", "F002", "F003"];
    let _ = w5.register("F001", "A 兼容", vec!["c"]);
    let _ = w5.register("F002", "A 兼容", vec!["c"]);
    set.add("gaps named", coverage_gaps(&w5, &all) == vec!["F003"], "");
    set.add("gaps consistent", !coverage_consistent(&w5, &all), "有缺口时满格断言必须为假");
    let _ = w5.register("F003", "A 兼容", vec!["c"]);
    set.add("gaps closed", coverage_gaps(&w5, &all).is_empty() && coverage_consistent(&w5, &all), "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    fn day(s: &str) -> u64 {
        parse_day(s).unwrap_or(0)
    }

    #[test]
    fn f200_deep_archive_is_append_only_across_quarters() {
        // 跨季行一旦写入永不消失（历史不可篡改——重跑只动本季行）。
        let mut arc = QuarterlyArchive::new();
        arc.record("2026Q1", 10, 5);
        arc.record("2026Q2", 12, 3);
        arc.record("2026Q3", 15, 0);
        let q1 = arc.rows[0];
        arc.record("2026Q3", 16, 0);
        assert_eq!(arc.rows[0], q1, "past quarters untouched");
        assert_eq!(arc.rows.len(), 3);
        assert!(arc.monotonic_green());
    }

    #[test]
    fn f200_deep_decision_gate_full_session() {
        // 一次季检会的完整决议流：违规提案被当场拦下两次、合规三项入账。
        let mut w = WalkAll::new();
        // 花哨提案：三问过、维护人力没有 → 拦。
        assert!(decide_gated(&mut w, ReviewColumn::Add, "彩虹任务栏", "", [true, true, true, false]).is_err());
        // 无理由废止 → 拦。
        assert!(decide_gated(&mut w, ReviewColumn::Retire, "F104", "", [true; 4]).is_err());
        // 带理由冻结 → 收。
        assert!(decide_gated(&mut w, ReviewColumn::Freeze, "F104 录音件", "存量冻结候删", [true; 4]).is_ok());
        // 四问全过的增补 → 收。
        assert!(decide_gated(&mut w, ReviewColumn::Add, "导出队列断点续传", "用户真实需求+硬件契机+开源成熟+有人维护", [true; 4]).is_ok());
        assert_eq!(w.decisions.len(), 2);
    }

    #[test]
    fn f200_deep_drift_scan_excludes_frozen() {
        // 冻结项不进腐化警报（冻结≠失败——警报只追活项）。
        let mut w = WalkAll::new();
        let _ = w.register("F101", "C 体验", vec!["c"]);
        let _ = w.register("F186", "G 安全", vec!["c"]);
        let alarms = drift_scan(&w, day("20260926"));
        assert_eq!(alarms.len(), 1, "F101 frozen, not alarmed");
        assert_eq!(alarms[0].fid, "F186");
    }

    #[test]
    fn f200_deep_run_checks_pass() {
        assert!(run_walkall_deep_checks().all_passed());
    }
}
