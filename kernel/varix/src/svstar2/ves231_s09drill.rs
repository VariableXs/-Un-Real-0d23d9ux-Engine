//! VE-F3977 · 预备演练
//!
//! S 域收口前预备演练：终审全流程空跑一遍——联测脚本干跑 + 评分卡空转 +
//! 走查表试填 + 报告模板试出。演练发现直通 F3972 补齐（直通单）。
//! 错误路径：演练失败→发现直通补齐；演练通过→放行收口；脚本缺→补本。
//! 性能：空跑 O(流程)；发现 O(项)；放行 O(1)。
//! 跨批对接：F3976 报告上游；F3983 联测正式版前置。
//! 无障碍与隐私：演练含五角色剧本干跑（域本色）；试填表不含真实录证。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3640）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3640;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9QCode(pub u16);

impl S9QCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const DRILL_FAIL: Self = Self(SEG_BASE + 1);
    pub const SCRIPT_MISSING: Self = Self(SEG_BASE + 2);
    pub const GATE_CLOSED: Self = Self(SEG_BASE + 3);
    pub const FAKE_FILL: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::DRILL_FAIL => "演练失败→发现直通补齐",
            Self::SCRIPT_MISSING => "脚本缺→补本",
            Self::GATE_CLOSED => "放行门未开→拦截",
            Self::FAKE_FILL => "试填代真→拒收",
            _ => "未知",
        }
    }
}

// ── 全流程空跑四站 ───────────────────────────────────────────────────

/// 四站。
pub const STATIONS: [&str; 4] = ["联测脚本干跑", "评分卡空转", "走查表试填", "报告模板试出"];

/// 演练站记录。
#[derive(Debug, Clone)]
pub struct StationRun {
    pub station: usize,
    pub pass: bool,
}

/// 演练执行器。
#[derive(Debug, Clone)]
pub struct Drill {
    pub runs: Vec<StationRun>,
    /// 干跑标记（不产真实数据）。
    pub dry: bool,
}

impl Drill {
    pub fn new() -> Self {
        Self { runs: Vec::new(), dry: true }
    }

    pub fn run(&mut self, station: usize, pass: bool) -> S9QCode {
        if station >= STATIONS.len() {
            return S9QCode::SCRIPT_MISSING;
        }
        self.runs.push(StationRun { station, pass });
        if pass {
            S9QCode::OK
        } else {
            S9QCode::DRILL_FAIL
        }
    }

    /// 站覆盖缺口。
    pub fn missing_stations(&self) -> Vec<usize> {
        (0..STATIONS.len()).filter(|s| !self.runs.iter().any(|r| r.station == *s)).collect()
    }

    /// 失败站清单（发现直通）。
    pub fn failed_stations(&self) -> Vec<usize> {
        self.runs.iter().filter(|r| !r.pass).map(|r| r.station).collect()
    }

    /// 演练通过：四站全跑全过。
    pub fn passed(&self) -> bool {
        self.missing_stations().is_empty() && self.failed_stations().is_empty()
    }
}

impl Default for Drill {
    fn default() -> Self {
        Self::new()
    }
}

// ── 直通单（发现 → F3972 补齐）───────────────────────────────────────

/// 直通单。
#[derive(Debug, Clone)]
pub struct PassThrough {
    pub finding: &'static str,
    /// 目标站。
    pub station: usize,
    /// 已被补齐消费。
    pub consumed: bool,
}

/// 直通通道。
#[derive(Debug, Clone)]
pub struct PassPipe {
    pub tickets: Vec<PassThrough>,
}

impl PassPipe {
    pub fn new() -> Self {
        Self { tickets: Vec::new() }
    }

    pub fn raise(&mut self, finding: &'static str, station: usize) -> S9QCode {
        if station >= STATIONS.len() {
            return S9QCode::SCRIPT_MISSING;
        }
        self.tickets.push(PassThrough { finding, station, consumed: false });
        S9QCode::OK
    }

    pub fn consume(&mut self, finding: &str) -> S9QCode {
        match self.tickets.iter_mut().find(|t| t.finding == finding && !t.consumed) {
            Some(t) => {
                t.consumed = true;
                S9QCode::OK
            }
            None => S9QCode::DRILL_FAIL,
        }
    }

    pub fn open(&self) -> Vec<&'static str> {
        self.tickets.iter().filter(|t| !t.consumed).map(|t| t.finding).collect()
    }
}

impl Default for PassPipe {
    fn default() -> Self {
        Self::new()
    }
}

// ── 放行门 ───────────────────────────────────────────────────────────

/// 放行门：演练全过 + 直通单全消费 → 开门。
#[derive(Debug, Clone)]
pub struct GoGate {
    pub open: bool,
}

impl GoGate {
    pub fn new() -> Self {
        Self { open: false }
    }

    /// 放行判定 O(1)。
    pub fn evaluate(&mut self, drill: &Drill, pipe: &PassPipe) -> S9QCode {
        if drill.passed() && pipe.open().is_empty() {
            self.open = true;
            S9QCode::OK
        } else {
            self.open = false;
            S9QCode::GATE_CLOSED
        }
    }
}

impl Default for GoGate {
    fn default() -> Self {
        Self::new()
    }
}

// ── 试填不代真 ───────────────────────────────────────────────────────

/// 试填行：演练期填写，必须带演练标记。
#[derive(Debug, Clone)]
pub struct TrialRow {
    pub field: &'static str,
    pub value: &'static str,
    /// 演练水印。
    pub dry_mark: bool,
}

impl TrialRow {
    pub fn legal(&self, dry: bool) -> bool {
        // 干跑期必须带水印；正式期不允许带水印。
        if dry {
            self.dry_mark
        } else {
            !self.dry_mark
        }
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves231_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves231_s09drill");

    // 01 四站清单守恒
    {
        chk(&mut set, "C3977-01", STATIONS.len() == 4 && STATIONS[0] == "联测脚本干跑" && STATIONS[3] == "报告模板试出", "err");
    }
    // 02 四站全跑全过
    {
        let mut d = Drill::new();
        for s in 0..4 {
            d.run(s, true);
        }
        chk(&mut set, "C3977-02", d.passed() && d.missing_stations().is_empty(), "err");
    }
    // 03 站缺→补本检出
    {
        let mut d = Drill::new();
        d.run(0, true);
        d.run(2, true);
        chk(&mut set, "C3977-03", d.missing_stations() == vec![1, 3] && !d.passed(), "err");
    }
    // 04 失败站→发现直通
    {
        let mut d = Drill::new();
        for s in 0..4 {
            d.run(s, s != 1);
        }
        chk(&mut set, "C3977-04", d.failed_stations() == vec![1] && !d.passed(), "err");
    }
    // 05 直通单消费闭环
    {
        let mut p = PassPipe::new();
        p.raise("评分卡字段缺", 1);
        chk(&mut set, "C3977-05", p.open() == vec!["评分卡字段缺"] && p.consume("评分卡字段缺") == S9QCode::OK && p.open().is_empty(), "err");
    }
    // 06 消费不存在发现拒收
    {
        let mut p = PassPipe::new();
        chk(&mut set, "C3977-06", p.consume("幽灵") == S9QCode::DRILL_FAIL, "err");
    }
    // 07 直通单站越界拒收
    {
        let mut p = PassPipe::new();
        chk(&mut set, "C3977-07", p.raise("x", 4) == S9QCode::SCRIPT_MISSING, "err");
    }
    // 08 放行门：全过 + 单清 → 开
    {
        let mut d = Drill::new();
        for s in 0..4 {
            d.run(s, true);
        }
        let mut g = GoGate::new();
        chk(&mut set, "C3977-08", g.evaluate(&d, &PassPipe::new()) == S9QCode::OK && g.open, "err");
    }
    // 09 放行门：有失败 → 关
    {
        let mut d = Drill::new();
        for s in 0..4 {
            d.run(s, true);
        }
        d.run(3, false);
        let mut g = GoGate::new();
        chk(&mut set, "C3977-09", g.evaluate(&d, &PassPipe::new()) == S9QCode::GATE_CLOSED && !g.open, "err");
    }
    // 10 放行门：直通单未清 → 关
    {
        let mut d = Drill::new();
        for s in 0..4 {
            d.run(s, true);
        }
        let mut p = PassPipe::new();
        p.raise("遗留发现", 0);
        let mut g = GoGate::new();
        chk(&mut set, "C3977-10", g.evaluate(&d, &p) == S9QCode::GATE_CLOSED && !g.open, "err");
    }
    // 11 试填不代真（干跑需水印）
    {
        let wet = TrialRow { field: "分数", value: "90", dry_mark: false };
        let dry = TrialRow { field: "分数", value: "90", dry_mark: true };
        chk(&mut set, "C3977-11", !wet.legal(true) && dry.legal(true) && !dry.legal(false), "err");
    }
    // 12 站越界拒收
    {
        let mut d = Drill::new();
        chk(&mut set, "C3977-12", d.run(9, true) == S9QCode::SCRIPT_MISSING, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3977-13", S9QCode::FAKE_FILL.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9QCode::OK, S9QCode::DRILL_FAIL, S9QCode::SCRIPT_MISSING, S9QCode::GATE_CLOSED, S9QCode::FAKE_FILL];
        chk(&mut set, "C3977-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_drill_all_green() {
        let s = run_ves231_checks();
        assert!(s.all_passed(), "ves231 red: {}", s.red_items_text());
    }

    #[test]
    fn test_gate_full_cycle() {
        let mut d = Drill::new();
        for s in 0..4 {
let _ =             d.run(s, true);
        }
let _ =         d.run(1, false);
        let mut p = PassPipe::new();
let _ =         p.raise("f", 1);
        let mut g = GoGate::new();
        assert_eq!(g.evaluate(&d, &p), S9QCode::GATE_CLOSED);
let _ =         p.consume("f");
let _ =         d.run(1, true);
        assert_eq!(g.evaluate(&d, &p), S9QCode::OK);
    }

    #[test]
    fn test_dry_mark_matrix() {
        let w = TrialRow { field: "f", value: "v", dry_mark: true };
        assert!(w.legal(true));
        assert!(!w.legal(false));
    }
}
