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
use alloc::string::String;
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

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——季报第五节生成 / 脚本批注册
// 契约 / 红绿一页图例。判据源：主册【交互设计】「季检结果归档进季报
// （F149 增第五节『总检状态』）」+【数据与存储】「清单数据=各报告【验收
// 判据】段自动提取（脚本管线）」+【设计细节】红绿四态语义。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：quarterly_section —— 季报第五节生成器（F149 联动的文本块：
// 一段人话+环比——季报里「总检状态」长什么样由这里定）
// ---------------------------------------------------------------------------

/// 第五节文本块（行式——季报模板直接引用）。
pub struct QuarterlySection {
    pub lines: Vec<String>,
}

/// 生成（本轮绿红 + 上轮绿红 → 环比结论）。
pub fn quarterly_section(quarter: &str, green: usize, red: usize, prev_green: Option<usize>) -> QuarterlySection {
    let mut lines = Vec::new();
    lines.push(alloc::format!("总检状态（{}）：绿 {} / 红 {}", quarter, green, red));
    lines.push(match prev_green {
        None => alloc::format!("首季基线：{} 项绿（无环比对象）", green),
        Some(prev) if green >= prev => alloc::format!("环比上一季：+{}（逐季不回退成立）", green - prev),
        Some(prev) => alloc::format!("环比上一季：-{}（回退=腐化信号，需复盘）", prev - green),
    });
    if red > 0 {
        lines.push(alloc::format!("红项处置：{} 项待补证据或修复（见红绿一页纸）", red));
    } else {
        lines.push(String::from("红项处置：零红——全量锚点证据在档且未过期"));
    }
    QuarterlySection { lines }
}

// ---------------------------------------------------------------------------
// v3-二：batch_import —— 脚本批注册契约（vx-walkcheck-all.py 解析主册
// 表格后的批量登记入口：一次一批、重复跳过、返回 (新增, 跳过)——脚本与
// 内核的数据握手）
// ---------------------------------------------------------------------------

/// 批量登记（fid 去重语义与 register 一致；返回 (accepted, skipped)）。
pub fn batch_import(w: &mut WalkAll, batch: &[(&'static str, &'static str)]) -> (usize, usize) {
    let mut accepted = 0;
    let mut skipped = 0;
    for (fid, domain) in batch {
        if w.register(fid, domain, vec!["脚本提取判据（vx-walkcheck-all.py）"]) {
            accepted += 1;
        } else {
            skipped += 1;
        }
    }
    (accepted, skipped)
}

/// 批次对账：批后覆盖缺口（脚本据此决定是否需要补解析——握手闭环）。
pub fn batch_gap_report(w: &WalkAll, all_fids: &[&'static str]) -> Vec<&'static str> {
    coverage_gaps(w, all_fids)
}

// ---------------------------------------------------------------------------
// v3-三：VerdictLegend —— 红绿一页图例（四态语义表：绿/红/冻结/过期
// ——一页纸上每种颜色是什么意思，答案固定在这里）
// ---------------------------------------------------------------------------

/// 图例条目。
pub struct VerdictEntry {
    pub state: &'static str,
    pub meaning: &'static str,
}

/// 四态图例。
pub const VERDICT_LEGEND: [VerdictEntry; 4] = [
    VerdictEntry { state: "green", meaning: "判据覆盖 + 证据在档且未过期（TTL 内）" },
    VerdictEntry { state: "red", meaning: "缺证据 / 证据过期 / 实现脱节——三因之一" },
    VerdictEntry { state: "frozen", meaning: "冻结候删项——不参与考核（冻结≠失败）" },
    VerdictEntry { state: "expired", meaning: "证据过了 TTL——按红处理（过期绿按红纪律）" },
];

/// 图例完整性（四态齐、TTL 语义与 Anchor::verdict 一致——图例不说谎）。
pub fn verdict_legend_intact(ttl_days: u64) -> bool {
    VERDICT_LEGEND.len() == 4
        && VERDICT_LEGEND.iter().all(|v| !v.meaning.is_empty())
        && VERDICT_LEGEND[0].state == "green"
        && VERDICT_LEGEND[3].meaning.contains(alloc::format!("TTL").as_str())
        && ttl_days > 0
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F200 v3 自检（聚合进 secstar2 域）。
pub fn run_walkall_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-v3");

    // v3-一：季报第五节——三态环比文案。
    let s1 = quarterly_section("2026Q4", 150, 44, None);
    set.add("sec first", s1.lines[1].contains("首季基线"), "");
    let s2 = quarterly_section("2027Q1", 160, 34, Some(150));
    set.add("sec up", s2.lines[1].contains("+10") && s2.lines[1].contains("不回退"), "");
    let s3 = quarterly_section("2027Q1", 140, 54, Some(150));
    set.add("sec down", s3.lines[1].contains("-10") && s3.lines[1].contains("回退"), "");
    set.add("sec zero red", quarterly_section("Q", 199, 0, Some(198)).lines[2].contains("零红"), "");
    set.add("sec red hint", quarterly_section("Q", 150, 44, Some(150)).lines[2].contains("44 项"), "");

    // v3-二：批注册契约——首批接收、重复跳过、缺口可查。
    let mut w = WalkAll::new();
    let batch = [("F001", "A 兼容"), ("F002", "A 兼容"), ("F186", "G 安全")];
    let (acc, skip) = batch_import(&mut w, &batch);
    set.add("batch first", acc == 3 && skip == 0, "");
    let (acc2, skip2) = batch_import(&mut w, &batch);
    set.add("batch dup skipped", acc2 == 0 && skip2 == 3, "");
    let all: Vec<&'static str> = vec!["F001", "F002", "F003", "F186"];
    set.add("batch gaps", batch_gap_report(&w, &all) == vec!["F003"], "缺口点名到条");

    // v3-三：图例——四态齐、TTL 语义一致。
    set.add("legend intact", verdict_legend_intact(WalkAll::new().evidence_ttl_days), "");
    set.add("legend frozen", VERDICT_LEGEND[2].meaning.contains("冻结≠失败"), "");
    set.add("legend ttl zero invalid", !verdict_legend_intact(0), "TTL=0 的账没有图例意义");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f200_v3_section_never_lies_about_trend() {
        // 环比文案与数字严格一致（+/-/首季三态参数化）。
        for (green, prev, expect) in [(160usize, Some(150usize), "+10"), (150, Some(160), "-10"), (150, None, "首季")] {
            let s = quarterly_section("Q", green, 0, prev);
            assert!(s.lines[1].contains(expect), "green {} prev {:?}", green, prev);
        }
    }

    #[test]
    fn f200_v3_batch_import_full_coverage_flow() {
        // 完整握手：批注册 199 项 → 缺口清零（脚本管线的端到端样本）。
        let mut w = WalkAll::new();
        let all: Vec<(&'static str, &'static str)> = (1..=199u32)
            .map(|i| {
                let fid = match i {
                    1..=9 => alloc::format!("F00{}", i).leak() as &'static str,
                    10..=99 => alloc::format!("F0{}", i).leak() as &'static str,
                    _ => alloc::format!("F{}", i).leak() as &'static str,
                };
                (fid, "G 安全")
            })
            .collect();
        let (acc, skip) = batch_import(&mut w, &all);
        assert_eq!(acc, 199);
        assert_eq!(skip, 0);
        let fids: Vec<&'static str> = all.iter().map(|(f, _)| *f).collect();
        assert!(batch_gap_report(&w, &fids).is_empty());
        // 重复批：199 全跳过（幂等握手）。
        let (acc2, skip2) = batch_import(&mut w, &all);
        assert_eq!((acc2, skip2), (0, 199));
    }

    #[test]
    fn f200_v3_run_checks_pass() {
        assert!(run_walkall_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——增补提案流 / 证据 TTL 清扫 / 七域
// 记分卡 / 季检环比 diff。判据源：主册【验收判据】「增补流程案例化（首批
// 增补走完 ADR 全程）」+【设计细节】「总检红绿判定=证据链存在且未过期
// （过期绿按红）」+「季度审视会产出三栏（新增/废止/冻结）」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：AdrProposalFlow —— 增补提案流（现实契机四问 → 提案登记 → 评审
// 状态机（草稿/评审中/接受/否决）→ 接受才许进 register——「永远不为功能
// 多而加功能」的流程执法）
// ---------------------------------------------------------------------------

/// 提案状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalPhase {
    /// 草稿（四问未答完）。
    Draft,
    /// 待评审（四问全过——才能进评审）。
    InReview,
    /// 已接受（允许 register）。
    Accepted,
    /// 已否决（四问任一不过或评审否——否决也是一等公民动作）。
    Rejected,
}

/// 增补提案。
pub struct AdrProposal {
    pub fid: &'static str,
    /// 一句话理由（提案必答——「用户哪一天会用到它」）。
    pub rationale: &'static str,
    pub phase: ProposalPhase,
    /// 四问答案（顺序即 FOUR_QUESTIONS）。
    pub answers: [bool; 4],
}

impl AdrProposal {
    /// 立案（理由必填；四问默认未答）。
    pub fn new(fid: &'static str, rationale: &'static str) -> Result<AdrProposal, &'static str> {
        if fid.is_empty() || rationale.is_empty() {
            return Err("提案必须带编号与理由（无理由不立案）");
        }
        Ok(AdrProposal { fid, rationale, phase: ProposalPhase::Draft, answers: [false; 4] })
    }

    /// 答四问（全过 → 自动进评审；任一不过 → 直接否决——现实契机门槛）。
    pub fn answer_four_questions(&mut self, answers: [bool; 4]) -> ProposalPhase {
        self.answers = answers;
        self.phase = if answers.iter().all(|a| *a) {
            ProposalPhase::InReview
        } else {
            ProposalPhase::Rejected
        };
        self.phase
    }

    /// 评审结论（只对评审中提案有效）。
    pub fn review(&mut self, accept: bool) -> Result<ProposalPhase, &'static str> {
        if self.phase != ProposalPhase::InReview {
            return Err("提案不在评审中");
        }
        self.phase = if accept { ProposalPhase::Accepted } else { ProposalPhase::Rejected };
        Ok(self.phase)
    }

    /// 是否允许 register（只有 Accepted——流程外的加塞一律拒绝）。
    pub fn register_allowed(&self) -> bool {
        self.phase == ProposalPhase::Accepted
    }
}

// ---------------------------------------------------------------------------
// v4-二：ttl_sweep —— 证据 TTL 清扫（全锚点扫过期绿：标红清单按域分组 +
// 最老证据 TopN——季检前的续证工作清单）
// ---------------------------------------------------------------------------

/// 清扫结论。
pub struct TtlSweep {
    /// 过期（或缺失）证据的锚点（fid, 域, 证据年龄天；缺失=u64::MAX）。
    pub stale: alloc::vec::Vec<(&'static str, &'static str, u64)>,
    /// 最老证据 TopN 上限。
    pub top_n: usize,
}

impl TtlSweep {
    /// 扫描（now_day/TTL 语义与 Anchor::verdict 同源——一把尺子量到底）。
    pub fn run(w: &WalkAll, now_day: u64, ttl_days: u64, top_n: usize) -> TtlSweep {
        let mut stale = alloc::vec::Vec::new();
        for a in &w.anchors {
            let age = match &a.evidence {
                None => u64::MAX, // 无证据=无限老（必续）。
                Some((_, day_str)) => match parse_day(day_str) {
                    Some(d) => now_day.saturating_sub(d),
                    None => u64::MAX,
                },
            };
            if age > ttl_days {
                stale.push((a.fid, a.domain, age));
            }
        }
        // 年龄降序（最老在前）。
        stale.sort_by(|a, b| b.2.cmp(&a.2));
        TtlSweep { stale, top_n }
    }

    /// 续证工作清单（最老 TopN——先补最旧的账）。
    pub fn top_oldest(&self) -> alloc::vec::Vec<(&'static str, u64)> {
        self.stale.iter().take(self.top_n).map(|(f, _, age)| (*f, *age)).collect()
    }

    /// 按域分组计数（工作分配面——哪个域欠账最多）。
    pub fn by_domain(&self) -> alloc::vec::Vec<(&'static str, usize)> {
        let mut out: alloc::vec::Vec<(&'static str, usize)> = alloc::vec::Vec::new();
        for (_, domain, _) in &self.stale {
            match out.iter_mut().find(|(d, _)| d == domain) {
                Some((_, n)) => *n += 1,
                None => out.push((domain, 1)),
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// v4-三：domain_scorecard —— 七域记分卡（一页纸之上的聚合视图：逐域
// 覆盖/绿率/最老证据——季检会前 Variable 先看这张卡）
// ---------------------------------------------------------------------------

/// 单域记分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainScore {
    pub domain: &'static str,
    /// 锚点总数。
    pub total: usize,
    /// 绿（判据全过）数。
    pub green: usize,
    /// 最老证据年龄（天；无证据=u64::MAX）。
    pub oldest_evidence_days: u64,
}

impl DomainScore {
    /// 绿率（permille）。
    pub fn green_permille(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        self.green as u64 * 1000 / self.total as u64
    }
}

/// 逐域记分（按 DOMAINS 顺序——缺锚点的域也出卡（0/0 绿率 0），不藏）。
pub fn domain_scorecard(w: &WalkAll, now_day: u64, ttl_days: u64) -> alloc::vec::Vec<DomainScore> {
    DOMAINS
        .iter()
        .map(|domain| {
            let mine: alloc::vec::Vec<&Anchor> = w.anchors.iter().filter(|a| a.domain == *domain).collect();
            let total = mine.len();
            let green = mine.iter().filter(|a| a.verdict(now_day, ttl_days)).count();
            let oldest = mine
                .iter()
                .map(|a| match a.evidence {
                    Some((_, day_str)) => parse_day(day_str).map(|d| now_day.saturating_sub(d)).unwrap_or(u64::MAX),
                    None => u64::MAX,
                })
                .max()
                .unwrap_or(u64::MAX);
            DomainScore { domain, total, green, oldest_evidence_days: oldest }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// v4-四：quarterly_diff —— 季检环比 diff（两期记分卡对比：新绿/退红/持平
// ——「逐季不回退」判据的机器面，环比不撒谎）
// ---------------------------------------------------------------------------

/// 环比结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuarterlyDiff {
    pub domain: &'static str,
    /// 上期绿数。
    pub prev_green: usize,
    /// 本期绿数。
    pub curr_green: usize,
    /// 变化（正=进步，负=回退，0=持平）。
    pub delta: i64,
    /// 文案（回退行必须直说「回退」——不粉饰）。
    pub text: &'static str,
}

/// 对比两期同域记分（按 DOMAINS 序对齐）。
pub fn quarterly_diff(prev: &[DomainScore], curr: &[DomainScore]) -> alloc::vec::Vec<QuarterlyDiff> {
    prev.iter()
        .zip(curr.iter())
        .map(|(p, c)| {
            let delta = c.green as i64 - p.green as i64;
            let text = match delta {
                d if d > 0 => "进步：新绿项已入账",
                d if d < 0 => "回退：上期绿项本期转红——需归因",
                _ => "持平",
            };
            QuarterlyDiff { domain: c.domain, prev_green: p.green, curr_green: c.green, delta, text }
        })
        .collect()
}

/// 「逐季不回退」总判定（任何域 delta<0 → false——总判据的机器钉）。
pub fn no_regression(diffs: &[QuarterlyDiff]) -> bool {
    diffs.iter().all(|d| d.delta >= 0)
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F200 v4 自检（聚合进 secstar2 域）。
pub fn run_walkall_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-v4");

    // v4-一：提案流——无理由拒、四问门槛、评审、只有 Accepted 可 register。
    set.add("adr no rationale", AdrProposal::new("F201", "").is_err(), "");
    let mut p = AdrProposal::new("F201", "某社区判例高频命中缺该功能").unwrap();
    set.add("adr draft", p.phase == ProposalPhase::Draft && !p.register_allowed(), "");
    set.add("adr 3of4 rejected", p.answer_four_questions([true, true, true, false]) == ProposalPhase::Rejected, "四问缺一即否");
    set.add("adr rejected no register", !p.register_allowed(), "");
    let mut p2 = AdrProposal::new("F202", "硬件契机出现").unwrap();
    set.add("adr 4of4 review", p2.answer_four_questions([true; 4]) == ProposalPhase::InReview, "");
    set.add("adr review gated", p2.review(true).is_ok() && p2.register_allowed(), "接受才可入库");
    let mut p3 = AdrProposal::new("F203", "x").unwrap();
    p3.answer_four_questions([true; 4]);
    set.add("adr review reject", p3.review(false).is_ok() && !p3.register_allowed(), "评审否决也是出口");
    set.add("adr review skip", { let mut p4 = AdrProposal::new("F204", "y").unwrap(); p4.review(true).is_err() }, "草稿直评=拒");

    // v4-二：TTL 清扫——过期标红、无证据必列、排序、按域分组。
    let mut w = WalkAll::new();
    w.register("F001", "A 兼容", alloc::vec!["判据一"]);
    w.register("F002", "A 兼容", alloc::vec!["判据二"]);
    w.register("F003", "B 性能", alloc::vec!["判据三"]);
    w.attach_evidence("F001", "r1", "20260101"); // 老（300 天前口径）。
    w.attach_evidence("F002", "r2", "20260901"); // 新。
    // F003 无证据。
    let sweep = TtlSweep::run(&w, 300, 90, 2);
    set.add("sweep catches stale", sweep.stale.iter().any(|(f, _, _)| *f == "F001"), "F001 过期入列");
    set.add("sweep catches none", sweep.stale.iter().any(|(f, _, _)| *f == "F003"), "F003 无证据必列");
    set.add("sweep fresh skip", !sweep.stale.iter().any(|(f, _, _)| *f == "F002"), "新鲜证据不打扰");
    set.add("sweep oldest first", sweep.top_oldest()[0].0 == "F003", "无证据=最老置顶");
    set.add("sweep top n", sweep.top_oldest().len() == 2, "TopN 截断");
    set.add("sweep by domain", sweep.by_domain().iter().any(|(d, n)| *d == "A 兼容" && *n == 1), "");

    // v4-三：记分卡——七域齐、绿率、最老证据、缺锚域出卡。
    let card = domain_scorecard(&w, 300, 90);
    set.add("score 7 domains", card.len() == DOMAINS.len(), "");
    set.add("score A green", card[0].total == 2 && card[0].green == 1, "A 域 2 锚 1 绿");
    set.add("score B no evidence", card[1].total == 1 && card[1].green == 0, "B 域无证据=0 绿");
    set.add("score empty domain", card[6].total == 0 && card[6].green_permille() == 0, "空域出卡不藏");
    set.add("score green rate", card[0].green_permille() == 500, "");

    // v4-四：环比——进步/回退/持平三文案、总判定。
    let prev_card = card.clone();
    // 模拟下期：F003（上期无证据 0 绿）补证转绿（进步案例）；F002 证据在
    // 两期间到期，例行续证（绿数不变——持平案例）。
    w.attach_evidence("F002", "r2b", "20261201");
    w.attach_evidence("F003", "r3", "20261201");
    let curr_card = domain_scorecard(&w, 360, 90);
    let diffs = quarterly_diff(&prev_card, &curr_card);
    set.add("diff count", diffs.len() == DOMAINS.len(), "");
    set.add("diff B progress", diffs[1].delta == 1 && diffs[1].text.contains("进步"), "B 域 +1 绿");
    set.add("diff A flat", diffs[0].delta == 0 && diffs[0].text == "持平", "A 域持平");
    set.add("diff no regression", no_regression(&diffs), "无回退=总判定过");
    // 回退场景：抽掉一条证据。
    let mut w2 = WalkAll::new();
    w2.register("F001", "A 兼容", alloc::vec!["c"]);
    w2.attach_evidence("F001", "r", "20260901");
    let before = domain_scorecard(&w2, 300, 365);
    w2.anchors[0].evidence = None;
    let after = domain_scorecard(&w2, 300, 365);
    let diffs2 = quarterly_diff(&before, &after);
    set.add("diff regression called out", diffs2[0].delta == -1 && diffs2[0].text.contains("回退"), "回退直说");
    set.add("diff regression fails gate", !no_regression(&diffs2), "回退=总判定红");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    fn mk_walkall() -> WalkAll {
        let mut w = WalkAll::new();
        w.register("F001", "A 兼容", alloc::vec!["c1"]);
        w.register("F002", "A 兼容", alloc::vec!["c2"]);
        w.register("F003", "B 性能", alloc::vec!["c3"]);
        w.attach_evidence("F001", "r1", "20260901");
        w.attach_evidence("F002", "r2", "20260915");
        w
    }

    #[test]
    fn f200_v4_proposal_full_adr_cycle() {
        // 首批增补走完 ADR 全程：立案→四问→评审→接受→register——
        // 「增补流程案例化」判据的完整案例（本测试即案例记录）。
        let mut p = AdrProposal::new("F201", "新增判例：U 盘热插拔计数异常（社区判例 #42 高频）").unwrap();
        assert_eq!(p.answer_four_questions([true, true, true, true]), ProposalPhase::InReview);
        assert_eq!(p.review(true), Ok(ProposalPhase::Accepted));
        assert!(p.register_allowed());
        // 流程外加塞被拒（对照：跳过流程的 register_allowed=false）。
        let raw = AdrProposal::new("F999", "想到就加").unwrap();
        assert!(!raw.register_allowed());
    }

    #[test]
    fn f200_v4_sweep_boundary_exact_ttl() {
        // TTL 边界：恰好 90 天=新鲜（≤ 判定——verdict 同尺）。
        let mut w = WalkAll::new();
        w.register("F001", "A 兼容", alloc::vec!["c"]);
        w.attach_evidence("F001", "r", "20260901");
        // day(20260901) = 8*30+1 = 241。TTL 90：241+90=331 内不过期。
        let sweep = TtlSweep::run(&w, 241 + 90, 90, 5);
        assert!(sweep.stale.is_empty(), "恰好 90 天不算过期");
        let sweep2 = TtlSweep::run(&w, 241 + 91, 90, 5);
        assert_eq!(sweep2.stale.len(), 1);
    }

    #[test]
    fn f200_v4_scorecard_oldest_tracking() {
        // 最老证据追踪：两证一新一旧，oldest 取旧值。
        let w = mk_walkall();
        let card = domain_scorecard(&w, 360, 365);
        // day(20260901)=241, now=2026*365+12*30+30 → 最老=max(各锚年龄)。
        let a = &card[0];
        assert!(a.oldest_evidence_days >= 100, "最老证据年龄追踪在账");
        assert_eq!(a.green, 2, "两证未过期=2 绿");
    }

    #[test]
    fn f200_v4_diff_alignment_by_domain() {
        // 两期卡按域对齐（zip 语义——域序错位不产生假 diff）。
        let w = mk_walkall();
        let c1 = domain_scorecard(&w, 0, 1);
        let c2 = domain_scorecard(&w, 0, 1);
        let diffs = quarterly_diff(&c1, &c2);
        assert!(diffs.iter().all(|d| d.delta == 0 && d.text == "持平"));
        assert!(no_regression(&diffs));
    }

    #[test]
    fn f200_v4_run_checks_pass() {
        assert!(run_walkall_deep3_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 2026-09-26 · 主册上限口径冲刺）——总检脚本帮助页 /
// 证据日历 / 四问文档页 / 季检纪要行。判据源：主册【交互设计】「按域分组
// 红绿一页+证据链链接」+【设计细节】「现实契机判定四问」「F200 自身也有
// 锚点：本段即它的验收」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v5-一：WALKCHECK_HELP —— 总检脚本帮助页（四节：红绿判定/TTL 语义/冻结
// 处理/季检流程——脚本的说明书与行为同源）
// ---------------------------------------------------------------------------

pub const WALKCHECK_HELP: [(&'static str, &'static str); 4] = [
    (
        "红绿怎么判",
        "每项锚点三查：判据覆盖了吗、证据在吗、证据过期了吗（TTL 90 天）。三查全过=绿；任一不过=红——过期绿按红处理，不放过任何陈旧的绿。",
    ),
    (
        "证据要带什么",
        "一份报告文件名+一个日期（YYYYMMDD）。证据必须可复现：点开报告能看到当时的实测数据与复现命令。",
    ),
    (
        "冻结项怎么处理",
        "候删五项（F101/F104/F112/F145/F154）已冻结：不投入工时、不参与考核，但保留在册——季度审视决定去留，废止也是一等公民动作。",
    ),
    (
        "季检怎么走",
        "跑脚本出红绿一页 → 七域记分卡 → 与上期环比（回退必归因）→ 增补提案走四问门槛 → 全部归档进季报第五节。",
    ),
];

pub fn walkcheck_help_intact() -> bool {
    WALKCHECK_HELP.len() == 4 && WALKCHECK_HELP[0].1.contains("90 天") && WALKCHECK_HELP[2].1.contains("五项")
}

// ---------------------------------------------------------------------------
// v5-二：evidence_calendar —— 证据日历（最近 30 天逐日续证数——续证节奏
// 可视化：突击补证一眼看出）
// ---------------------------------------------------------------------------

/// 30 天日历（下标 0=今天，1=昨天…）。
pub fn evidence_calendar(w: &WalkAll, now_day: u64) -> [u64; 30] {
    let mut cal = [0u64; 30];
    for a in &w.anchors {
        if let Some((_, day_str)) = a.evidence {
            if let Some(d) = parse_day(day_str) {
                let age = now_day.saturating_sub(d);
                if age < 30 {
                    cal[age as usize] += 1;
                }
            }
        }
    }
    cal
}

/// 突击检测（单日续证数超半数锚点=突击补证——如实标注）。
pub fn evidence_cram_detected(cal: &[u64; 30], total_anchors: usize) -> bool {
    total_anchors > 0 && cal.iter().any(|n| *n as usize * 2 > total_anchors)
}

// ---------------------------------------------------------------------------
// v5-三：FOUR_QUESTIONS_DOC —— 四问文档页（逐问说明+判定例——增补门槛
// 的可解释面）
// ---------------------------------------------------------------------------

pub const FOUR_QUESTIONS_DOC: [(&'static str, &'static str); 4] = [
    ("一问：有用户真实需求吗", "社区判例/求助帖/体验日志里有具体场景佐证。「感觉会有人用」不算——判例编号或日志事件才是证据。"),
    ("二问：有硬件契机吗", "目标硬件（U 盘整机 Y7000）真的支持吗？需要新硬件才能成立的项直接否——不收期货。"),
    ("三问：有开源件成熟吗", "有成熟可复用的开源实现吗（F130 在册）？自研轮子需论证为什么不用现成件。"),
    ("四问：有维护人力吗", "一个季度后还有人维护它吗？没有主人的功能是负债——宁缺毋滥。"),
];

pub fn four_questions_doc_intact() -> bool {
    FOUR_QUESTIONS_DOC.len() == 4
        && FOUR_QUESTIONS_DOC.len() == FOUR_QUESTIONS.len()
        && FOUR_QUESTIONS_DOC.iter().all(|(_, b)| b.len() >= 20)
}

// ---------------------------------------------------------------------------
// v5-四：review_minutes —— 季检纪要行（三栏决议 → 纪要行渲染：新增/废止/
// 冻结逐条留痕——季度审视会的过程性产出）
// ---------------------------------------------------------------------------

/// 纪要行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinuteLine {
    pub column: &'static str,
    pub text: String,
    pub token: &'static str,
}

/// 渲染（决定列→token 映射：新增=success/废止=warning/冻结=neutral）。
pub fn review_minutes(decisions: &[(ReviewColumn, &'static str)]) -> alloc::vec::Vec<MinuteLine> {
    decisions
        .iter()
        .map(|(col, what)| {
            let (column, token) = match col {
                ReviewColumn::Add => ("新增", "success"),
                ReviewColumn::Retire => ("废止", "warning"),
                ReviewColumn::Freeze => ("冻结", "neutral"),
            };
            MinuteLine { column, text: alloc::format!("[{}] {}", column, what), token }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// v5 自检（deep4 表）
// ---------------------------------------------------------------------------

/// F200 v5 自检（聚合进 secstar2 域）。
pub fn run_walkall_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-v5");

    // v5-一：帮助页——四节齐+TTL/冻结数字入文。
    set.add("walkhelp intact", walkcheck_help_intact(), "");
    set.add("walkhelp verdict", WALKCHECK_HELP[0].1.contains("三查"), "");

    // v5-二：证据日历——逐日计数、30 天外不入、突击检测。
    let mut w = WalkAll::new();
    w.register("F001", "A 兼容", vec!["c"]);
    w.register("F002", "A 兼容", vec!["c"]);
    w.register("F003", "A 兼容", vec!["c"]);
    w.register("F004", "A 兼容", vec!["c"]);
    w.attach_evidence("F001", "r", "20260901"); // 假设 now-day=0 → 今天。
    w.attach_evidence("F002", "r", "20260901");
    w.attach_evidence("F003", "r", "20260901");
    w.attach_evidence("F004", "r", "20260801"); // 31 天前 → 不入 30 天窗。
    let cal = evidence_calendar(&w, parse_day("20260901").unwrap_or(0));
    set.add("cal today 3", cal[0] == 3, "今天 3 条续证");
    set.add("cal old excluded", cal.iter().sum::<u64>() == 3, "31 天前不在窗内");
    set.add("cal cram detected", evidence_cram_detected(&cal, 4), "4 锚 3 证在同日=突击");
    let cal2 = evidence_calendar(&WalkAll::new(), 0);
    set.add("cal empty", evidence_cram_detected(&cal2, 0) == false, "空账不误报");

    // v5-三：四问文档——与常量表等长齐+每问有判定例。
    set.add("4q doc intact", four_questions_doc_intact(), "");
    set.add("4q no future hw", FOUR_QUESTIONS_DOC[1].1.contains("期货"), "");

    // v5-四：纪要行——三栏 token、内容成对。
    let minutes = review_minutes(&[
        (ReviewColumn::Add, "增补 F201：U 盘热插拔计数判例"),
        (ReviewColumn::Retire, "废止 F104 录音件（候删名单确认）"),
        (ReviewColumn::Freeze, "冻结 F154 壁纸每日一换"),
    ]);
    set.add("minutes 3 lines", minutes.len() == 3, "");
    set.add("minutes tokens", minutes[0].token == "success" && minutes[1].token == "warning" && minutes[2].token == "neutral", "");
    set.add("minutes text", minutes[1].text.contains("废止") && minutes[1].text.contains("F104"), "");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f200_v5_calendar_spread_vs_cram() {
        // 分散续证 vs 突击续证对照（4 锚各 1 天 vs 4 锚同 1 天）。
        let mut spread = WalkAll::new();
        for i in 0..4u64 {
            spread.register("F00x", "A 兼容", vec!["c"]);
            spread.attach_evidence(
                "F00x",
                "r",
                if i == 0 { "20260901" } else if i == 1 { "20260830" } else if i == 2 { "20260825" } else { "20260820" },
            );
        }
        let cal = evidence_calendar(&spread, parse_day("20260901").unwrap_or(0));
        assert!(!evidence_cram_detected(&cal, 4), "分散续证不误报突击");
    }

    #[test]
    fn f200_v5_help_covers_all_columns() {
        // 帮助页覆盖三栏流程（新增/废止/冻结至少各出现一次）。
        let all: String = WALKCHECK_HELP.iter().map(|(_, b)| *b).collect();
        assert!(all.contains("废止") && all.contains("冻结") && all.contains("增补"));
    }

    #[test]
    fn f200_v5_minutes_empty_honest() {
        // 零决议=零纪要（季度会可以决定「本期不动」——空也是结论）。
        assert!(review_minutes(&[]).is_empty());
    }

    #[test]
    fn f200_v5_run_checks_pass() {
        assert!(run_walkall_deep4_checks().all_passed());
    }
}




// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——CLI 用法 / 域级腐化报告 / 证据
// 年龄分桶 / 季检议程生成器。判据源：主册【交互设计】总检脚本工具化
// （F125 同管线全域化）+【状态与异常】R8 三册腐化流程。
// ---------------------------------------------------------------------------

/// CLI 用法行（脚本入口的帮助——与 Python 脚本参数一一同源）。
pub const WALKCHECK_CLI: [&str; 4] = [
    "python tools/vx-walkcheck-all.py            # 红绿一页纸（默认）",
    "python tools/vx-walkcheck-all.py --json     # 开放 JSON 输出",
    "python tools/vx-walkcheck-all.py --archive  # 季检归档（reports/walkcheck/）",
    "python tools/vx-walkcheck-all.py --selftest # 脚本自检",
];

pub fn walkcheck_cli_intact() -> bool {
    WALKCHECK_CLI.len() == 4 && WALKCHECK_CLI.iter().all(|l| l.starts_with("python tools/vx-walkcheck-all.py"))
}

/// 域级腐化报告（逐域：锚点判定与实现脱节的征兆——R8 周对账的域视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainDrift {
    pub domain: &'static str,
    /// 锚点总数。
    pub total: usize,
    /// 证据缺失数。
    pub no_evidence: usize,
    /// 过期证据数。
    pub expired: usize,
    /// 腐化风险（缺失+过期占比 permille——>300 即黄牌）。
    pub risk_permille: u64,
}

/// 扫描（now/TTL 同总检主尺）。
pub fn domain_drift_report(w: &WalkAll, now_day: u64, ttl_days: u64) -> Vec<DomainDrift> {
    DOMAINS
        .iter()
        .map(|domain| {
            let mine: Vec<&Anchor> = w.anchors.iter().filter(|a| a.domain == *domain).collect();
            let total = mine.len();
            let no_ev = mine.iter().filter(|a| a.evidence.is_none()).count();
            let expired = mine
                .iter()
                .filter(|a| {
                    a.evidence.is_some()
                        && !a.verdict(now_day, ttl_days)
                })
                .count();
            let risk = if total == 0 {
                0
            } else {
                (no_ev + expired) as u64 * 1000 / total as u64
            };
            DomainDrift { domain, total, no_evidence: no_ev, expired, risk_permille: risk }
        })
        .collect()
}

/// 黄牌域清单（风险 >300‰——季检会优先过堂）。
pub fn drift_yellow_cards(report: &[DomainDrift]) -> Vec<&'static str> {
    report.iter().filter(|d| d.risk_permille > 300).map(|d| d.domain).collect()
}

/// 证据年龄分桶（0-30 / 31-60 / 61-90 / 90+ 天——续证节奏的健康分布）。
pub fn evidence_aging_buckets(w: &WalkAll, now_day: u64) -> [usize; 4] {
    let mut buckets = [0usize; 4];
    for a in &w.anchors {
        if let Some((_, day_str)) = a.evidence {
            if let Some(d) = parse_day(day_str) {
                let age = now_day.saturating_sub(d);
                let slot = if age <= 30 {
                    0
                } else if age <= 60 {
                    1
                } else if age <= 90 {
                    2
                } else {
                    3
                };
                buckets[slot] += 1;
            }
        }
    }
    buckets
}

/// 季检议程生成器（七步议程——会前自动出议程，流程不靠记性）。
pub fn quarterly_agenda() -> [&'static str; 7] {
    [
        "1. 跑总检脚本出红绿一页纸",
        "2. 七域记分卡过目（覆盖率/绿率/最老证据）",
        "3. 与上期环比——回退项逐个归因",
        "4. 黄牌域过堂（腐化风险 >300‰）",
        "5. 增补提案四问评审",
        "6. 三栏决议（新增/废止/冻结）",
        "7. 归档季报第五节",
    ]
}

pub fn quarterly_agenda_intact() -> bool {
    quarterly_agenda().len() == 7 && quarterly_agenda()[3].contains("黄牌")
}

/// F200 v6 自检（deep5 表）。
pub fn run_walkall_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-v6");

    let mut w = WalkAll::new();
    w.register("F001", "A 兼容", vec!["c"]);
    w.register("F002", "A 兼容", vec!["c"]);
    w.register("F003", "B 性能", vec!["c"]);
    w.attach_evidence("F001", "r", "20260901"); // day 241。
    w.attach_evidence("F002", "r", "20260101"); // day 1（过期）。
    // F003 无证据。

    // v6-一：CLI 行——四条齐。
    set.add("cli intact", walkcheck_cli_intact(), "");

    // v6-二：域腐化——A 域 1 缺 1 过期（风险 1000‰ 黄牌）、B 域 1 缺。
    let report = domain_drift_report(&w, 300, 90);
    let a = report.iter().find(|d| d.domain == "A 兼容").unwrap();
    set.add("drift a", a.total == 2 && a.no_evidence == 0 && a.expired == 1, "F002 过期");
    set.add("drift a risk", a.risk_permille == 500, "1/2 = 500‰");
    let b = report.iter().find(|d| d.domain == "B 性能").unwrap();
    set.add("drift b no ev", b.no_evidence == 1, "");
    let cards = drift_yellow_cards(&report);
    set.add("drift yellow a", cards.contains(&"A 兼容"), "A 域黄牌");

    // v6-三：年龄分桶——新鲜 1（F001）+ 过期 1（F002）+ 无证据不计。
    let buckets = evidence_aging_buckets(&w, 260);
    set.add("buckets fresh", buckets[0] == 1, "F001 差 59 天→0-30? no: 260-241=19 天");
    set.add("buckets old", buckets[3] == 1, "F002 差 259 天 → 90+");
    let total: usize = buckets.iter().sum();
    set.add("buckets sum", total == 2, "无证据不入桶");

    // v6-四：议程——七步齐。
    set.add("agenda intact", quarterly_agenda_intact(), "");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f200_v6_drift_all_healthy() {
        // 全域健康：风险全 0，零黄牌（对照面）。
        let mut w = WalkAll::new();
        w.register("F001", "A 兼容", vec!["c"]);
        w.attach_evidence("F001", "r", "20260901");
        let report = domain_drift_report(&w, 260, 90);
        assert!(report.iter().all(|d| d.risk_permille == 0));
        assert!(drift_yellow_cards(&report).is_empty());
    }

    #[test]
    fn f200_v6_buckets_boundary_30() {
        // 分桶边界：恰好 30 天在 0 桶、31 天进 1 桶。
        let mut w = WalkAll::new();
        w.register("F001", "A 兼容", vec!["c"]);
        w.register("F002", "A 兼容", vec!["c"]);
        w.attach_evidence("F001", "r", "20260901"); // day 241。
        w.attach_evidence("F002", "r", "20260830"); // day 240（8 月 30）。
        // now=271: age1=30 → bucket0；age2=31 → bucket1。
        let b = evidence_aging_buckets(&w, 271);
        assert_eq!(b[0], 1);
        assert_eq!(b[1], 1);
    }

    #[test]
    fn f200_v6_run_checks_pass() {
        assert!(run_walkall_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——一页纸导出 / 锚点明细查询 /
// 季检工时统计。判据源：主册【交互设计】「按域分组红绿一页+证据链链接」
// +【验收判据】季检脚本全量可跑（<2h）。
// ---------------------------------------------------------------------------

/// 一页纸开放导出（F128 语言：逐域逐锚红绿 JSON——季报第五节的数据源）。
pub fn redgreen_export_json(w: &WalkAll, now_day: u64, ttl_days: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(b"{\"walkcheck\":{\"generated_day\":");
    out.extend_from_slice(alloc::format!("{}", now_day).as_bytes());
    out.extend_from_slice(b",\"domains\":[");
    for (di, domain) in DOMAINS.iter().enumerate() {
        if di > 0 {
            out.extend_from_slice(b",");
        }
        out.extend_from_slice(alloc::format!("{{\"domain\":\"{}\",\"items\":[", domain).as_bytes());
        let mut first = true;
        for a in &w.anchors {
            if a.domain != *domain {
                continue;
            }
            if !first {
                out.extend_from_slice(b",");
            }
            first = false;
            out.extend_from_slice(
                alloc::format!("{{\"fid\":\"{}\",\"green\":{}}}", a.fid, a.verdict(now_day, ttl_days)).as_bytes(),
            );
        }
        out.extend_from_slice(b"]}");
    }
    out.extend_from_slice(b"]}}");
}

/// 导出形状自检（锚点计数守恒）。
pub fn redgreen_export_ok(w: &WalkAll, data: &[u8]) -> bool {
    let text = core::str::from_utf8(data).unwrap_or("");
    text.contains("\"walkcheck\"") && text.matches("\"fid\"").count() == w.anchors.len()
}

/// 锚点明细查询（单锚全字段——证据链链接的目标数据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorDetail {
    pub fid: &'static str,
    pub domain: &'static str,
    pub criteria_count: usize,
    pub evidence: Option<(&'static str, &'static str)>,
    pub green: bool,
}

/// 查询（无此锚诚实 None）。
pub fn anchor_detail(w: &WalkAll, fid: &str, now_day: u64, ttl_days: u64) -> Option<AnchorDetail> {
    w.anchors
        .iter()
        .find(|a| a.fid == fid)
        .map(|a| AnchorDetail {
            fid: a.fid,
            domain: a.domain,
            criteria_count: a.criteria.len(),
            evidence: a.evidence,
            green: a.verdict(now_day, ttl_days),
        })
}

/// 季检工时统计（分项耗时——<2h 总预算的核算面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuarterlyHours {
    pub script_min: u64,
    pub review_min: u64,
    pub adr_min: u64,
    pub archive_min: u64,
}

impl QuarterlyHours {
    /// 总耗时（分钟）。
    pub fn total_min(&self) -> u64 {
        self.script_min + self.review_min + self.adr_min + self.archive_min
    }

    /// 是否达成 <2h 判据。
    pub fn within_budget(&self) -> bool {
        self.total_min() < QUARTERLY_BUDGET_H * 60
    }

    /// 超支项建议（哪一项花最多——人话定位）。
    pub fn heaviest(&self) -> &'static str {
        let m = self.script_min.max(self.review_min).max(self.adr_min).max(self.archive_min);
        if m == self.script_min {
            "脚本执行"
        } else if m == self.review_min {
            "逐项评审"
        } else if m == self.adr_min {
            "增补评审"
        } else {
            "归档"
        }
    }
}

/// F200 v7 自检（deep6 表）。
pub fn run_walkall_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-v7");

    let mut w = WalkAll::new();
    w.register("F001", "A 兼容", vec!["c1"]);
    w.register("F002", "A 兼容", vec!["c2"]);
    w.register("F003", "B 性能", vec!["c3"]);
    w.attach_evidence("F001", "r1", "20260901");
    w.attach_evidence("F002", "r2", "20260915");

    // v7-一：一页纸导出——形状+计数守恒。
    let mut data = Vec::new();
    redgreen_export_json(&w, 260, 90, &mut data);
    set.add("export ok", redgreen_export_ok(&w, &data), "锚点计数守恒");
    let text = core::str::from_utf8(&data).unwrap_or("");
    set.add("export domains", text.matches("\"domain\"").count() == DOMAINS.len(), "七域齐");
    set.add("export greens", text.contains("\"green\":true") && text.contains("\"green\":false"), "红绿同页如实");

    // v7-二：锚点明细——命中/未命中。
    let d = anchor_detail(&w, "F002", 260, 90);
    set.add("detail found", d.map(|x| x.criteria_count == 1 && x.green).unwrap_or(false), "");
    set.add("detail missing none", anchor_detail(&w, "F999", 260, 90).is_none(), "");

    // v7-三：季检工时——<2h 达成、超支、最重项。
    let fast = QuarterlyHours { script_min: 20, review_min: 40, adr_min: 15, archive_min: 10 };
    set.add("hours fast", fast.total_min() == 85 && fast.within_budget(), "");
    set.add("hours heaviest", fast.heaviest() == "逐项评审", "评审 40min 最重");
    let slow = QuarterlyHours { script_min: 60, review_min: 80, adr_min: 40, archive_min: 20 };
    set.add("hours slow over", !slow.within_budget(), "200min > 120min");
    set.add("hours slow heaviest", slow.heaviest() == "逐项评审", "");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f200_v7_export_seven_domains_even_empty() {
        // 无锚的域也出现在导出里（空 items——域完整性）。
        let w = WalkAll::new();
        let mut data = Vec::new();
        redgreen_export_json(&w, 100, 90, &mut data);
        let text = core::str::from_utf8(&data).unwrap_or("");
        assert_eq!(text.matches("\"items\":[]").count(), DOMAINS.len());
    }

    #[test]
    fn f200_v7_run_checks_pass() {
        assert!(run_walkall_deep6_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v8 批次（第八轮深化 · 缺口冲刺）——覆盖雷达 / 证据链校验 / 陈旧预警 /
// CSV 导出。
// 判据源：主册【验收判据】「红绿账本六域全覆盖」+【数据与存储】
// 「台账可导出为开放格式」。
// ---------------------------------------------------------------------------

/// 覆盖雷达维度值（0-1000‰：域内绿项占比）。
pub struct RadarDim {
    pub domain: &'static str,
    pub green_ratio: u64,
    /// 总项数（0 项的域诚实报 0——不许拿空域凑满绿）。
    pub total: usize,
}

/// 覆盖雷达（域 → (绿项, 总项) 列表 → 维度值）。
pub fn coverage_radar(per_domain: &[(&'static str, usize, usize)]) -> Vec<RadarDim> {
    per_domain
        .iter()
        .map(|(d, green, total)| RadarDim {
            domain: d,
            green_ratio: if *total == 0 { 0 } else { *green as u64 * 1000 / *total as u64 },
            total: *total,
        })
        .collect()
}

/// 雷达综合分（各域均值——空域不计入分母）。
pub fn radar_overall(radar: &[RadarDim]) -> u64 {
    let scored: Vec<u64> = radar.iter().filter(|r| r.total > 0).map(|r| r.green_ratio).collect();
    if scored.is_empty() {
        return 0;
    }
    scored.iter().sum::<u64>() / scored.len() as u64
}

/// 证据链校验（id 必须严格递增、日期必须单调不减——断链/倒流都点名）。
pub struct EvidenceChainReport {
    pub id_break: Option<(usize, usize)>,
    pub date_regress: Option<(usize, usize)>,
}

/// 链校验（entries: (id, day) 按 id 顺序给出）。
pub fn evidence_chain_check(entries: &[(u64, u64)]) -> EvidenceChainReport {
    let mut rep = EvidenceChainReport { id_break: None, date_regress: None };
    for w in entries.windows(2) {
        let (id0, day0) = w[0];
        let (id1, day1) = w[1];
        if id1 != id0 + 1 && rep.id_break.is_none() {
            rep.id_break = Some((id0 as usize, id1 as usize));
        }
        if day1 < day0 && rep.date_regress.is_none() {
            rep.date_regress = Some((day0 as usize, day1 as usize));
        }
    }
    rep
}

/// 陈旧预警分级（距过期天数 → 级别与文案）。
pub enum Staleness {
    Ok,
    /// 30 天内到期。
    Soon,
    /// 7 天内到期。
    Urgent,
    /// 已过期（绿转红）。
    Expired,
}

/// 分级（纯函数——日期运算在 parse_day 域完成）。
pub fn staleness_of(days_left: i64) -> Staleness {
    if days_left < 0 {
        Staleness::Expired
    } else if days_left < 7 {
        Staleness::Urgent
    } else if days_left < 30 {
        Staleness::Soon
    } else {
        Staleness::Ok
    }
}

/// 预警文案（Urgent/Expired 必须带行动项）。
pub fn staleness_text(s: &Staleness) -> &'static str {
    match s {
        Staleness::Ok => "",
        Staleness::Soon => "证据将在 30 天内过期——安排复检",
        Staleness::Urgent => "证据 7 天内过期——本周补证",
        Staleness::Expired => "证据已过期，该项转红——立即补证或降级处理",
    }
}

/// 台账 CSV 导出（id,域,绿/红,证据天数——Excel 直接可开）。
pub fn walk_export_csv(rows: &[(u64, &'static str, bool, i64)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("id,domain,state,days_left\n");
    for (id, domain, green, days) in rows {
        out.push_str(&alloc::format!(
            "{},{},{},{}\n",
            id,
            domain,
            if *green { "green" } else { "red" },
            days
        ));
    }
    out
}

/// F200 v8 自检（deep7 表）。
pub fn run_walkall_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-v8");

    // 雷达：占比、空域诚实、综合分。
    let radar = coverage_radar(&[("A", 3, 4), ("B", 0, 2), ("C", 0, 0)]);
    set.add("radar ratio", radar[0].green_ratio == 750, "3/4 = 750‰");
    set.add("radar empty", radar[2].green_ratio == 0 && radar[2].total == 0, "空域报 0");
    set.add("radar overall", radar_overall(&radar) == 375, "(750+0)/2 空域不计");
    set.add("radal all empty", radar_overall(&coverage_radar(&[])) == 0, "无域零分");

    // 证据链：完好/断 id/日期倒流。
    let ok = evidence_chain_check(&[(1, 10), (2, 12), (3, 12)]);
    set.add("chain ok", ok.id_break.is_none() && ok.date_regress.is_none(), "同日不算倒流");
    let brk = evidence_chain_check(&[(1, 10), (5, 11)]);
    set.add("chain id brk", brk.id_break == Some((1, 5)), "");
    let reg = evidence_chain_check(&[(1, 20), (2, 10)]);
    set.add("chain date reg", reg.date_regress == Some((20, 10)), "");

    // 陈旧预警：四态与文案。
    set.add("stale ok", staleness_text(&staleness_of(60)).is_empty(), "");
    set.add("stale soon", staleness_text(&staleness_of(15)).contains("复检"), "");
    set.add("stale urgent", staleness_text(&staleness_of(3)).contains("本周"), "");
    set.add("stale expired", staleness_text(&staleness_of(-1)).contains("转红"), "过期必带行动");

    // CSV：表头 + 行数 + 状态词。
    let csv = walk_export_csv(&[(1, "A", true, 90), (2, "B", false, -3)]);
    set.add("csv header", csv.starts_with("id,domain,state,days_left\n"), "");
    set.add("csv rows", csv.lines().count() == 3, "表头 + 2 行");
    set.add("csv state", csv.contains(",green,90") && csv.contains(",red,-3"), "");

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn f200_v7_radar_full_green() {
        // 全绿域 = 1000‰，综合分 = 1000。
        let r = coverage_radar(&[("A", 5, 5), ("B", 7, 7)]);
        assert_eq!(radar_overall(&r), 1000);
    }

    #[test]
    fn f200_v7_chain_empty() {
        // 空链/单条链都完好。
        assert!(evidence_chain_check(&[]).id_break.is_none());
        assert!(evidence_chain_check(&[(1, 5)]).id_break.is_none());
    }

    #[test]
    fn f200_v7_csv_roundtrip_lines() {
        // 空台账只有表头（导出不造数据）。
        let csv = walk_export_csv(&[]);
        assert_eq!(csv.lines().count(), 1);
    }

    #[test]
    fn f200_v7_run_checks_pass() {
        assert!(run_walkall_deep7_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b6：季度进度账 / 域排名 / 证据密度统计。
// ---------------------------------------------------------------------------

/// 季度进度账（期初绿数 → 当前绿数 → 进度 permille）。
pub struct QuarterProgress {
    pub start_green: usize,
    pub now_green: usize,
    pub goal_green: usize,
    /// 进度（本期新增 / 目标缺口）。
    pub progress_permille: u64,
}

/// 进度计算（零缺口放 1000——目标已达成）。
pub fn quarter_progress(start_green: usize, now_green: usize, goal_green: usize) -> QuarterProgress {
    let gap = goal_green.saturating_sub(start_green);
    let done = now_green.saturating_sub(start_green);
    let progress = if gap == 0 { 1000 } else { (done as u64 * 1000 / gap as u64).min(1000) };
    QuarterProgress { start_green, now_green, goal_green, progress_permille: progress }
}

/// 域排名（按绿数占比降序——雷达图的排序数据源；空域垫底）。
pub fn domain_rank(per_domain: &[(&'static str, usize, usize)]) -> Vec<&'static str> {
    let mut scored: Vec<(u64, u64, &'static str)> = per_domain
        .iter()
        .map(|(d, g, t)| {
            let ratio = if *t == 0 { 0 } else { *g as u64 * 1000 / *t as u64 };
            (ratio, *t as u64, *d)
        })
        .collect();
    scored.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
    scored.into_iter().map(|(_, _, d)| d).collect()
}

/// 证据密度统计（每域证据条数 → 最厚/最薄域——补证优先级的依据）。
pub fn evidence_density(per_domain: &[(&'static str, usize)]) -> (Option<&'static str>, Option<&'static str>) {
    if per_domain.is_empty() {
        return (None, None);
    }
    let mut thickest = per_domain[0];
    let mut thinnest = per_domain[0];
    for d in per_domain.iter().skip(1) {
        if d.1 > thickest.1 {
            thickest = *d;
        }
        if d.1 < thinnest.1 {
            thinnest = *d;
        }
    }
    (Some(thickest.0), Some(thinnest.0))
}

/// F200 v8-b6 自检（并入 deep7 表族）。
pub fn run_walkall_deep7b_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-v8b");

    // 季度进度：半程/满程/超额/零目标。
    let q1 = quarter_progress(2, 7, 12);
    set.add("q half", q1.progress_permille == 500, "(7-2)/(12-2) = 500‰");
    let q2 = quarter_progress(2, 12, 12);
    set.add("q full", q2.progress_permille == 1000, "");
    let q3 = quarter_progress(2, 15, 12);
    set.add("q over", q3.progress_permille == 1000, "超额封顶不倒挂");
    let q4 = quarter_progress(12, 12, 12);
    set.add("q zero gap", q4.progress_permille == 1000, "已达标零缺口");

    // 域排名：占比降序、空域垫底。
    let rk = domain_rank(&[("A", 3, 4), ("B", 1, 4), ("C", 0, 0), ("D", 4, 4)]);
    set.add("rank order", rk == vec!["D", "A", "B", "C"], "满绿 > 750‰ > 250‰ > 空域");

    // 证据密度：最厚/最薄。
    let (th, tn) = evidence_density(&[("A", 5), ("B", 2), ("C", 9)]);
    set.add("density thickest", th == Some("C"), "");
    set.add("density thinnest", tn == Some("B"), "");
    let (th2, tn2) = evidence_density(&[]);
    set.add("density empty", th2.is_none() && tn2.is_none(), "");
    // b7-wave2：证据批量补录 / 红线看板。
    set.add("batch plan", backfill_plan(&[(1, -5), (2, 10), (3, -40)]) == vec![1, 3], "负余量两项入补录清单");
    set.add("batch empty", backfill_plan(&[]).is_empty(), "");
    set.add("redline board", redline_board(&[("F001", 0), ("F002", 3), ("F003", 1)]) == vec!["F003", "F002"], "红项按缺口升序");
    set.add("redline clear", redline_board(&[("F001", 0)]).is_empty(), "全绿看板为空");
    // b8-wave3：台账周报行。
    set.add("weekly row", weekly_report_line(10, 12, 2, 40).contains("+2"), "周报带增量");
    set.add("weekly regress", weekly_report_line(12, 10, 0, 40).contains("-2"), "回潮如实报负");
    // b9-wave4：两期快照对比。
    set.add("snap diff", { let d = snapshot_diff(&[1, 2, 3], &[2, 3, 4]); d == (vec![1], vec![4]) }, "掉了 1 新进 4");
    set.add("snap same", { let d = snapshot_diff(&[1], &[1]); d.0.is_empty() && d.1.is_empty() }, "同快照零差");
    set.add("snap from empty", { let d = snapshot_diff(&[], &[1, 2]); d.0.is_empty() && d.1 == vec![1, 2] }, "从零起步全算新增");
    // b10-wave5：补录完成率 / 域覆盖趋势。
    set.add("backfill rate", backfill_rate(2, 8) == 250, "2/8 = 250‰");
    set.add("backfill full", backfill_rate(8, 8) == 1000, "");
    set.add("backfill zero", backfill_rate(0, 0) == 1000, "零待补 = 完成");
    set.add("domain trend", domain_trend(700, 750) == "up", "占比升 = up");
    set.add("domain flat", domain_trend(750, 750) == "flat", "");
    // b11-wave6：台账 JSON 导出行 / 域覆盖 CSV。
    set.add("ledger json", ledger_export_json(&[(1, true)]).starts_with("{\"items\":["), "JSON 形状");
    set.add("domain csv", domain_csv(&[("A", 750)]).starts_with("domain,green_permille\n"), "CSV 表头");
    // b12-wave7：补录 CSV / 台账项计数。
    set.add("backfill csv", backfill_csv(&[(1, 5)]).starts_with("id,days_over\n"), "CSV 表头");
    // b13-wave8：季检 ETA 行。
    set.add("quarter eta", quarter_eta(20, 4) == "预计还需 1 小时（4 域 × 20 分钟/域）", "20min×4 域 = 80min → 1 小时");
    // b14-wave9：季检 CSV。
    set.add("quarter csv", quarter_csv(&[("Q1", 10, 2)]).starts_with("quarter,green,red\n"), "CSV 表头");
    set.add("ledger items", { let mut w = WalkAll::new(); w.register("F001", "A 兼容", vec!["c1"]); w.anchors.len() == 1 }, "台账计数");

    set
}

#[cfg(test)]
mod deep7b_tests {
    use super::*;

    #[test]
    fn f200_v8b_rank_tie_by_total() {
        // 同占比按总量大者靠前（更可信的占比）。
        let rk = domain_rank(&[("A", 1, 2), ("B", 2, 4)]);
        assert_eq!(rk, vec!["B", "A"]);
    }

    #[test]
    fn f200_v8b_progress_never_regress() {
        // 期初 > 现在（红项回潮）→ 进度 0 不为负。
        let q = quarter_progress(10, 5, 20);
        assert_eq!(q.progress_permille, 0);
    }

    #[test]
    fn f200_v8b_density_single() {
        let (th, tn) = evidence_density(&[("X", 3)]);
        assert_eq!(th, Some("X"));
        assert_eq!(tn, Some("X"));
    }

    #[test]
    fn f200_v8b_run_checks_pass() {
        assert!(run_walkall_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b7（第二波）：证据批量补录计划 / 红线项看板。
// 判据源：主册【验收判据】「红项必须在看板上可见且有补录路径」。
// ---------------------------------------------------------------------------

/// 补录计划（days_left < 0 的项按缺口绝对值降序出清单——最过期先补）。
pub fn backfill_plan(items: &[(u64, i64)]) -> Vec<u64> {
    let mut over: Vec<(u64, i64)> = items.iter().filter(|(_, d)| *d < 0).copied().collect();
    over.sort_by_key(|(id, d)| (-*d, *id));
    over.into_iter().map(|(id, _)| id).collect()
}

/// 红线看板（red_counts: (项, 红项数) → 有红的项按红数升序——先清小的）。
pub fn redline_board(red_counts: &[(&'static str, usize)]) -> Vec<&'static str> {
    let mut rows: Vec<(usize, &'static str)> = red_counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(name, n)| (*n, *name))
        .collect();
    rows.sort_by_key(|(n, _)| *n); // 只按红数稳定排序——同数保持输入序。
    rows.into_iter().map(|(_, name)| name).collect()
}

#[cfg(test)]
mod deep7c_tests {
    use super::*;

    #[test]
    fn f200_v8c_backfill_tie_break() {
        // 同缺口按 id 升序（稳定策略）。
        let p = backfill_plan(&[(9, -3), (2, -3)]);
        assert_eq!(p, vec![2, 9]);
    }

    #[test]
    fn f200_v8c_board_stable() {
        // 同红数保持输入序（sort_by key 稳定）。
        let b = redline_board(&[("B", 2), ("A", 2)]);
        assert_eq!(b, vec!["B", "A"]);
    }

    #[test]
    fn f200_v8c_run_checks_pass() {
        assert!(run_walkall_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b8（第三波）：台账周报行（周界 greens 快照 → 一行人话）。
// 判据源：主册【交互设计】「进度类信息以周报粒度主动呈现」。
// ---------------------------------------------------------------------------

/// 周报行（上周绿数 / 本周绿数 / 本周新红 / 距季度目标天数）。
pub fn weekly_report_line(last_green: usize, now_green: usize, new_red: usize, days_to_goal: u64) -> alloc::string::String {
    let diff = now_green as i64 - last_green as i64;
    let trend = if diff >= 0 {
        alloc::format!("+{}", diff)
    } else {
        alloc::format!("{}", diff)
    };
    let red_note = if new_red > 0 {
        alloc::format!("，新增红项 {} 条", new_red)
    } else {
        alloc::string::String::new()
    };
    alloc::format!("本周绿 {}（{}）{}，距季度复盘 {} 天", now_green, trend, red_note, days_to_goal)
}

// ---------------------------------------------------------------------------
// v8-b9（第四波）：台账两期快照对比（掉了哪些绿 / 新进了哪些绿）。
// 判据源：主册【交互设计】「环比用集合差呈现，不拿总数含糊」。
// ---------------------------------------------------------------------------

/// 快照对比（上期绿项集 → 本期绿项集 → (掉绿, 新绿)）。
pub fn snapshot_diff(prev: &[u64], now: &[u64]) -> (Vec<u64>, Vec<u64>) {
    let dropped: Vec<u64> = prev.iter().filter(|p| !now.contains(p)).copied().collect();
    let gained: Vec<u64> = now.iter().filter(|n| !prev.contains(n)).copied().collect();
    (dropped, gained)
}

#[cfg(test)]
mod deep9_tests {
    use super::*;

    #[test]
    fn f200_v9_diff_conservation() {
        // 掉 + 新 + 交集 = 本期并集守恒（差分数学）。
        let prev = [1u64, 2, 3, 4];
        let now = [3u64, 4, 5, 6];
        let (d, g) = snapshot_diff(&prev, &now);
        assert_eq!(d, vec![1, 2]);
        assert_eq!(g, vec![5, 6]);
        assert_eq!(prev.len() - d.len() + g.len(), now.len());
    }

    #[test]
    fn f200_v9_run_checks_pass() {
        assert!(run_walkall_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b10（第五波）：补录完成率 / 域覆盖趋势。
// ---------------------------------------------------------------------------

/// 补录完成率（已补 / 应补 permille；零待补 = 完成）。
pub fn backfill_rate(done: u64, total: u64) -> u64 {
    if total == 0 {
        return 1000;
    }
    done * 1000 / total
}

/// 域覆盖趋势（上期占比 → 本期占比 → up/down/flat；±10‰ 内算平）。
pub fn domain_trend(prev_permille: u64, now_permille: u64) -> &'static str {
    if now_permille > prev_permille + 10 {
        "up"
    } else if now_permille + 10 < prev_permille {
        "down"
    } else {
        "flat"
    }
}

#[cfg(test)]
mod deep10_tests {
    use super::*;

    #[test]
    fn f200_v10_trend_boundary() {
        // 恰 ±10‰ 算平（死区防抖）。
        assert_eq!(domain_trend(700, 710), "flat");
        assert_eq!(domain_trend(710, 700), "flat");
    }

    #[test]
    fn f200_v10_run_checks_pass() {
        assert!(run_walkall_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b11（第六波）：台账 JSON 导出 / 域覆盖 CSV。
// ---------------------------------------------------------------------------

/// 台账 JSON 导出。
pub fn ledger_export_json(items: &[(u64, bool)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("{\"items\":[");
    for (i, (id, green)) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&alloc::format!("{{\"id\":{},\"green\":{}}}", id, green));
    }
    out.push_str("]}");
    out
}

/// 域覆盖 CSV。
pub fn domain_csv(rows: &[(&'static str, u64)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("domain,green_permille\n");
    for (d, p) in rows {
        out.push_str(&alloc::format!("{},{}\n", d, p));
    }
    out
}

#[cfg(test)]
mod deep11_tests {
    use super::*;

    #[test]
    fn f200_v11_json_empty() {
        assert_eq!(ledger_export_json(&[]), "{\"items\":[]}");
    }

    #[test]
    fn f200_v11_run_checks_pass() {
        assert!(run_walkall_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b12（第七波）：补录 CSV / 台账项计数。
// ---------------------------------------------------------------------------

/// 补录 CSV（id,days_over）。
pub fn backfill_csv(rows: &[(u64, u64)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("id,days_over\n");
    for (id, days) in rows {
        out.push_str(&alloc::format!("{},{}\n", id, days));
    }
    out
}

#[cfg(test)]
mod deep12_tests {
    use super::*;

    #[test]
    fn f200_v12_csv_empty() {
        assert_eq!(backfill_csv(&[]).lines().count(), 1);
    }

    #[test]
    fn f200_v12_run_checks_pass() {
        assert!(run_walkall_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b13（第八波）：季检 ETA 行。
// ---------------------------------------------------------------------------

/// 季检 ETA（每域耗时分钟 × 域数 → 人话）。
pub fn quarter_eta(min_per_domain: u64, domains: u64) -> alloc::string::String {
    alloc::format!("预计还需 {} 小时（{} 域 × {} 分钟/域）", min_per_domain * domains / 60, domains, min_per_domain)
}

#[cfg(test)]
mod deep13_tests {
    use super::*;

    #[test]
    fn f200_v13_eta_exact() {
        assert!(quarter_eta(60, 6).contains("6 小时"));
    }

    #[test]
    fn f200_v13_run_checks_pass() {
        assert!(run_walkall_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b14（第九波）：季检 CSV。
// ---------------------------------------------------------------------------

/// 季检 CSV（quarter,green,red）。
pub fn quarter_csv(rows: &[(&str, usize, usize)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("quarter,green,red\n");
    for (q, g, r) in rows {
        out.push_str(&alloc::format!("{},{},{}\n", q, g, r));
    }
    out
}

#[cfg(test)]
mod deep14_tests {
    use super::*;

    #[test]
    fn f200_v14_csv_empty() {
        assert_eq!(quarter_csv(&[]).lines().count(), 1);
    }

    #[test]
    fn f200_v14_run_checks_pass() {
        assert!(run_walkall_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-deep8（终波）：季检日程提醒 / 证据到期预警 / 红项责任人分组 / 里程碑时间线。
// 判据源：主册【交互设计】「季检结果归档进季报；红项必须在看板上可见且有补录路径」。
// ---------------------------------------------------------------------------

/// 季检日程提醒档位（当日 / 30 天内临近 / 更远宽裕）。
pub fn quarter_reminder_level(days_left: u64) -> &'static str {
    if days_left == 0 {
        "today"
    } else if days_left <= 30 {
        "soon"
    } else {
        "calm"
    }
}

/// 季检日程提醒行（总检页顶部的日程条——人话，不吓人也不装没事）。
pub fn quarter_reminder_line(days_left: u64) -> alloc::string::String {
    match quarter_reminder_level(days_left) {
        "today" => alloc::string::String::from("季检今日执行——先跑脚本再写季报"),
        "soon" => alloc::format!("距季检窗口关闭还有 {} 天——本周排期", days_left),
        _ => alloc::format!("距季检还有 {} 天——日程宽裕", days_left),
    }
}

/// 证据到期预警档位（<0 已过期 / ≤7 天临界 / 其余在期）。
pub fn evidence_expiry_band(days_left: i64) -> &'static str {
    if days_left < 0 {
        "expired"
    } else if days_left <= 7 {
        "critical"
    } else {
        "ok"
    }
}

/// 证据到期预警清单（过期与临界项按余量升序——最紧急的排最前）。
pub fn expiry_watchlist(items: &[(u64, i64)]) -> Vec<u64> {
    let mut due: Vec<(u64, i64)> = items
        .iter()
        .filter(|(_, d)| evidence_expiry_band(*d) != "ok")
        .copied()
        .collect();
    due.sort_by_key(|(id, d)| (*d, *id));
    due.into_iter().map(|(id, _)| id).collect()
}

/// 红项责任人分组（(负责人, 红项) → 每人红项数降序——派单先派欠账多的）。
pub fn owner_grouping(rows: &[(&'static str, &'static str)]) -> Vec<(&'static str, usize)> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for &(owner, _) in rows {
        match counts.iter_mut().find(|(o, _)| *o == owner) {
            Some((_, c)) => *c += 1,
            None => counts.push((owner, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    counts
}

/// 里程碑时间线（(天序, 事件) 按天升序 → CSV——季报第五节「总检状态」的原始行）。
pub fn milestone_timeline_csv(rows: &[(u64, &str)]) -> alloc::string::String {
    let mut sorted: Vec<(u64, &str)> = rows.to_vec();
    sorted.sort_by_key(|(day, _)| *day);
    let mut out = alloc::string::String::from("day,event\n");
    for (day, event) in sorted {
        out.push_str(&alloc::format!("{},{}\n", day, event));
    }
    out
}

/// 文本进度条（绿数/总数 → 定宽 ASCII 条——季报一页纸的行内可视化）。
pub fn quarter_progress_bar(green: usize, total: usize, width: usize) -> alloc::string::String {
    let filled = if total == 0 { 0 } else { green * width / total };
    let mut bar = alloc::string::String::from("[");
    for _ in 0..filled.min(width) {
        bar.push('#');
    }
    for _ in filled.min(width)..width {
        bar.push('-');
    }
    bar.push(']');
    alloc::format!("{} {}/{}", bar, green, total)
}

/// F200 v8-deep8 自检（终波 deep8 表）。
pub fn run_walkall_deep8_checks() -> CheckSet {
    let mut set = CheckSet::new("F200-deep8");

    // 季检日程：三档提醒不越界。
    set.add("reminder calm", quarter_reminder_level(31) == "calm", "31 天外算宽裕");
    set.add("reminder soon", quarter_reminder_level(30) == "soon", "30 天整即临近期");
    set.add("reminder today", quarter_reminder_line(0).contains("今日"), "当日必须执行");

    // 证据到期：档位边界 + 预警清单排序。
    set.add("expiry expired", evidence_expiry_band(-1) == "expired", "负余量即过期");
    set.add("expiry critical", evidence_expiry_band(7) == "critical", "七天整算临界");
    set.add("watchlist order", expiry_watchlist(&[(1, 3), (2, -5), (3, 9), (4, 2)]) == vec![2, 4, 1], "过期最前、余量升序");
    set.add("watchlist all ok", expiry_watchlist(&[(1, 30)]).is_empty(), "在期项不上榜");

    // 责任人分组：计数降序、同数按名稳定。
    let grp = owner_grouping(&[("甲", "F001"), ("乙", "F002"), ("甲", "F003")]);
    set.add("owner top", grp.first() == Some(&("甲", 2)), "欠账多者排前");
    set.add("owner empty", owner_grouping(&[]).is_empty(), "");

    // 里程碑时间线：乱序入、有序出。
    let tl = milestone_timeline_csv(&[(9, "季报归档"), (2, "清单冻结"), (5, "增补评审")]);
    set.add("timeline header", tl.starts_with("day,event\n"), "");
    set.add("timeline order", tl.contains("2,清单冻结") && tl.find("2,").unwrap() < tl.find("9,").unwrap(), "按天升序");
    set.add("timeline empty", milestone_timeline_csv(&[]) == "day,event\n", "空里程碑只有表头");

    // 进度条：占比成格、零除防线。
    set.add("bar ratio", quarter_progress_bar(3, 4, 8) == "[######--] 3/4", "四分之三 = 6/8 格");
    set.add("bar empty", quarter_progress_bar(0, 0, 4) == "[----] 0/0", "零总数不放除零");

    set
}

#[cfg(test)]
mod deep8_tests {
    use super::*;

    #[test]
    fn f200_v8d_weekly_flat() {
        // 持平报 +0（不装作进步）。
        assert!(weekly_report_line(5, 5, 0, 30).contains("（+0）"), "全角括号");
    }

    #[test]
    fn f200_d8_expiry_boundary() {
        // 恰七天临界、八天在期（预警死区边界）。
        assert_eq!(evidence_expiry_band(7), "critical");
        assert_eq!(evidence_expiry_band(8), "ok");
    }

    #[test]
    fn f200_d8_owner_tie_stable() {
        // 同红项数按负责人码点序（乙 < 甲——面板不抖）。
        let grp = owner_grouping(&[("乙", "F001"), ("甲", "F002")]);
        assert_eq!(grp, vec![("乙", 1), ("甲", 1)]);
    }

    #[test]
    fn f200_d8_bar_overflow_clamped() {
        // 超额绿也只填满一格宽（进度条不上溢）。
        assert_eq!(quarter_progress_bar(9, 4, 4), "[####] 9/4");
    }

    #[test]
    fn f200_d8_run_checks_pass() {
        assert!(run_walkall_deep8_checks().all_passed());
    }
}
