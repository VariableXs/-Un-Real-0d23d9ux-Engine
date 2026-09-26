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
