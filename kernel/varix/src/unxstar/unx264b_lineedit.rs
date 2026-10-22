//! unx264b_lineedit —— UNX-F0264 终端行编辑器增强（历史/补全）· 双实现并存 B 面。
//! 锚点（CoRun Unxreal 主册 L1692）：UNX-F0264 · 终端行编辑器增强（历史/补全） · 目标 280 行 · 已深化
//! 判据原文：UNX-F0264-J1 历史 16 条循环/前缀补全/Tab 二义提示全过，边界输入不崩溃
//! 并存声明：unx264_lineedit.rs（并行工号 A 面）在先，本面为独立第二实现（源码只增不减）。
//! 组件：历史环（16 条定容循环 · 满丢最旧 · 上下键循环游标）
//!      + 前缀补全器（命令表前缀匹配 · 唯一命中即补全 · 零命中回显原行）
//!      + Tab 二义提示（多候选公共前缀推进）
//!      + 边界输入防线（空历史/越界/超长/控制字符/空行 · 全不崩溃留案）
//! 判据落地：历史 16 循环 + 前缀补全 + 二义提示 + 边界不崩
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u32 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本（B 面）。
pub const LB_VERSION: &str = "unx264b_lineedit-v1";

/// 诊断段基址（0x5140 段 · 本模块独占）。
pub const LB_SEG_BASE: u16 = 0x5140;

/// 历史容量（判据口径 16 条）。
pub const LB_HIST_CAP: usize = 16;

/// 行长上限（边界防线 · 冻结 v1）。
pub const LB_LINE_CAP: usize = 120;

/// 历史环（定容循环 · 满丢最旧）。
pub struct LbHistory {
    /// 环槽。
    pub slots: [Option<String>; LB_HIST_CAP],
    /// 写游标。
    pub head: usize,
    /// 有效条数。
    pub filled: u32,
    /// 读游标（相对最新条目偏移）。
    pub cursor: usize,
    /// 满环淘汰计数。
    pub evicted: u32,
}

impl LbHistory {
    /// 空历史。
    pub fn new() -> Self {
        LbHistory {
            slots: core::array::from_fn(|_| None),
            head: 0,
            filled: 0,
            cursor: 0,
            evicted: 0,
        }
    }

    /// 追加一条（与最新重复不入环）。
    pub fn push(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if self.filled >= 1 {
            let last_idx = (self.head + LB_HIST_CAP - 1) % LB_HIST_CAP;
            if let Some(last) = &self.slots[last_idx] {
                if last == line {
                    return;
                }
            }
        }
        if self.filled == LB_HIST_CAP as u32 {
            self.evicted += 1;
        } else {
            self.filled += 1;
        }
        self.slots[self.head] = Some(String::from(line));
        self.head = (self.head + 1) % LB_HIST_CAP;
        self.cursor = 0;
    }

    /// 上键：向更旧移动（不越最旧）。
    pub fn up(&mut self) -> Option<String> {
        if self.filled == 0 {
            return None;
        }
        if self.cursor < self.filled as usize {
            self.cursor += 1;
        }
        self.at_offset(self.cursor)
    }

    /// 下键：向更新移动，越过最新回 None（还原输入行）。
    pub fn down(&mut self) -> Option<String> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.at_offset(self.cursor)
    }

    /// 偏移取值。
    fn at_offset(&self, off: usize) -> Option<String> {
        if off == 0 || off > self.filled as usize {
            return None;
        }
        let idx = (self.head + LB_HIST_CAP - off) % LB_HIST_CAP;
        self.slots[idx].clone()
    }

    /// 16 循环断言：灌 32 条后满环 + 淘汰恰 16。
    pub fn cyclic_ok(&self) -> bool {
        self.filled == LB_HIST_CAP as u32 && self.evicted == 16
    }
}

/// 补全候选。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LbComplete {
    /// 唯一命中。
    Unique(String),
    /// 二义（候选表 + 公共前缀）。
    Ambiguous(Vec<String>, String),
    /// 零命中。
    None,
}

/// 前缀补全。
pub fn lb_complete(cmd_table: &[&str], prefix: &str) -> LbComplete {
    if prefix.is_empty() {
        return LbComplete::None;
    }
    let hits: Vec<String> = cmd_table
        .iter()
        .filter(|c| c.starts_with(prefix))
        .map(|c| String::from(*c))
        .collect();
    match hits.len() {
        0 => LbComplete::None,
        1 => LbComplete::Unique(hits.into_iter().next().unwrap_or_default()),
        _ => {
            let mut common = prefix.to_string();
            'outer: while common.len() < hits.iter().map(|h| h.len()).min().unwrap_or(0) {
                let nc = hits[0].as_bytes().get(common.len()).copied();
                for h in &hits {
                    if h.as_bytes().get(common.len()).copied() != nc {
                        break 'outer;
                    }
                }
                match nc {
                    Some(c) => common.push(c as char),
                    None => break,
                }
            }
            LbComplete::Ambiguous(hits, common)
        }
    }
}

/// 行编辑器（B 面）。
pub struct LbEditor {
    /// 历史。
    pub hist: LbHistory,
    /// 行缓冲。
    pub line: String,
    /// 命令表。
    pub table: Vec<String>,
    /// 超长拒绝数。
    pub overlong: u32,
    /// 控制字符拒绝数。
    pub ctrl_rejects: u32,
    /// 空行提交忽略数。
    pub empty_submits: u32,
}

impl LbEditor {
    /// 新编辑器。
    pub fn new(table: &[&str]) -> Self {
        LbEditor {
            hist: LbHistory::new(),
            line: String::new(),
            table: table.iter().map(|s| String::from(*s)).collect(),
            overlong: 0,
            ctrl_rejects: 0,
            empty_submits: 0,
        }
    }

    /// 键入。
    pub fn type_char(&mut self, c: char) -> bool {
        if c.is_control() && c != '\n' && c != '\r' {
            self.ctrl_rejects += 1;
            return false;
        }
        if self.line.len() >= LB_LINE_CAP {
            self.overlong += 1;
            return false;
        }
        self.line.push(c);
        true
    }

    /// 提交。
    pub fn submit(&mut self) -> bool {
        let t = self.line.trim_end_matches(['\r', '\n']).to_string();
        self.line.clear();
        if t.is_empty() {
            self.empty_submits += 1;
            return false;
        }
        self.hist.push(&t);
        true
    }

    /// Tab。
    pub fn tab(&mut self) -> LbComplete {
        let prefix = self.line.trim_end().to_string();
        let tbl: Vec<&str> = self.table.iter().map(|s| s.as_str()).collect();
        let r = lb_complete(&tbl, &prefix);
        match &r {
            LbComplete::Unique(full) => self.line = full.clone(),
            LbComplete::Ambiguous(_, common) => {
                if common.len() > prefix.len() {
                    self.line = common.clone();
                }
            }
            LbComplete::None => {}
        }
        r
    }

    /// 边界不崩断言。
    pub fn bounds_ok(&self) -> bool {
        self.line.len() <= LB_LINE_CAP
            && self
                .hist
                .slots
                .iter()
                .all(|s| s.as_ref().map(|x| x.len() <= LB_LINE_CAP).unwrap_or(true))
    }
}

/// 边界注入核（五场）。
pub struct LbInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
}

impl LbInjector {
    /// 空注入核。
    pub fn new() -> Self {
        LbInjector { cases: 0, caught: 0 }
    }

    /// 五场注入（历史防线用独立探针编辑器 · 不依赖主链路状态）。
    pub fn inject_all(&mut self, ed: &mut LbEditor) {
        // 空历史上键（独立探针 · 真空历史）。
        let mut probe = LbEditor::new(&["a"]);
        if probe.hist.up().is_none() {
            self.caught += 1;
        }
        self.cases += 1;
        // 下键越界回 None。
        ed.hist.push("c1");
        ed.hist.up();
        ed.hist.up();
        let _ = ed.hist.down();
        if ed.hist.down().is_none() {
            self.caught += 1;
        }
        self.cases += 1;
        // 超长行。
        let b = ed.overlong;
        for _ in 0..(LB_LINE_CAP + 8) {
            ed.type_char('x');
        }
        if ed.overlong > b {
            self.caught += 1;
        }
        self.cases += 1;
        // 控制字符。
        let b2 = ed.ctrl_rejects;
        ed.type_char('\u{07}');
        ed.type_char('\u{1b}');
        if ed.ctrl_rejects >= b2 + 2 {
            self.caught += 1;
        }
        self.cases += 1;
        // 空行提交。
        ed.line.clear();
        let b3 = ed.empty_submits;
        ed.submit();
        if ed.empty_submits == b3 + 1 {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 5 && self.caught == self.cases
    }
}

/// 判据自检（t 系入口 · B 面）。
pub fn run_t264b_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx264b-t264");
    let mut ed = LbEditor::new(&[
        "dump", "dumpfull", "help", "history", "halt", "info", "init", "stats",
    ]);
    // 32 条灌入。
    for i in 0..32 {
        ed.hist.push(&format!("cmd-{:02}", i));
    }
    let cyclic = ed.hist.cyclic_ok();
    // up 走 20 步不越界。
    let mut got: usize = 0;
    for _ in 0..20 {
        if ed.hist.up().is_some() {
            got += 1;
        }
    }
    // 补全三态。
    let u = lb_complete(&["dump", "dumpfull", "help"], "dumpf");
    let a = lb_complete(&["dump", "dumpfull", "help"], "d");
    let n = lb_complete(&["dump", "dumpfull", "help"], "zz");
    let u_ok = matches!(u, LbComplete::Unique(ref s) if s == "dumpfull");
    let a_ok = matches!(a, LbComplete::Ambiguous(ref v, ref c) if v.len() == 2 && c == "dump");
    let n_ok = matches!(n, LbComplete::None);
    // Tab 公共前缀。
    ed.line = String::from("d");
    ed.tab();
    let tab_ok = ed.line == "dump";
    // 注入。
    let mut inj = LbInjector::new();
    inj.inject_all(&mut ed);
    let ok = cyclic
        && got >= LB_HIST_CAP
        && u_ok
        && a_ok
        && n_ok
        && tab_ok
        && inj.all_caught()
        && ed.bounds_ok();
    let msg = format!(
        "{}: cyclic={} up_hits={} unique={} ambigu={} none={} tab={} inj={}/{} bounds={} seg=0x{:04x}",
        LB_VERSION,
        cyclic,
        got,
        u_ok,
        a_ok,
        n_ok,
        tab_ok,
        inj.caught,
        inj.cases,
        ed.bounds_ok(),
        LB_SEG_BASE
    );
    set.add_dyn("UNX-F0264-J1", ok, &msg);
    set.add("LB-hist-cycle", cyclic, "");
    set.add("LB-complete-3state", u_ok && a_ok && n_ok, "");
    set.add("LB-tab-common", tab_ok, "");
    set.add("LB-bounds-5/5", inj.all_caught() && ed.bounds_ok(), "");
    set
}
