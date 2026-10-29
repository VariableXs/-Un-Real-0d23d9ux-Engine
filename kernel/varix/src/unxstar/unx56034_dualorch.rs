//! UNX-F56034 双机采集编排器（0x60420 段）
//!
//! 锚点：UNX-F56034-J1 双系统采集顺序编排（同负载脚本同版本逐轮落
//! 账）；J2 顺序效应 counterbalance（先 Varix 后 Windows / 先 Windows
//! 后 Varix 两序交替，两序轮数差 ≤1）；J3 编排日志归档可复查（逐轮
//! 探针先行断言 + 轮账只追加）三判据全绿。判据锚定 §AI-71.2 专题一
//! 环境可比。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·双机编排面）。
//!
//! 结构：顺序闭集登记册（两序）+ 轮账（轮号 × 顺序 × 探针先行旗 ×
//! 脚本版本逐轮落账）+ counterbalance 核（两序计数差 ≤1）+ 探针先行
//! 闸（未跑探针禁采）+ 日志归账 + 注入核（越闭集顺序/探针未先/版本空
//! /重复轮号/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60420 独占 0x20）。
pub const SEG_BASE: u32 = 0x60420;
pub const OR_ORDER_UNK: u32 = SEG_BASE + 0x01; // 越闭集顺序
pub const OR_PROBE_NOT_FIRST: u32 = SEG_BASE + 0x02; // 探针未先行
pub const OR_VER_EMPTY: u32 = SEG_BASE + 0x03; // 脚本版本空
pub const OR_DUP_ROUND: u32 = SEG_BASE + 0x04; // 重复轮号
pub const OR_SEQ_BACK: u32 = SEG_BASE + 0x05; // 轮号回退
pub const OR_SEALED: u32 = SEG_BASE + 0x06; // 编排账已封账拒写
pub const OR_VER_DRIFT: u32 = SEG_BASE + 0x07; // 脚本版本漂移（跨轮异版）

/// 顺序闭集（counterbalance 两序）。
pub const ORD_VARIX_FIRST: usize = 0;
pub const ORD_WIN_FIRST: usize = 1;
pub const ORD_COUNT: usize = 2;
pub const ORD_NAMES: [&str; ORD_COUNT] = ["varix_first", "win_first"];

/// 轮账：轮号 × 顺序 × 探针先行 × 脚本版本。
#[derive(Clone)]
pub struct RoundEntry {
    pub seq: u32,
    pub order: usize,
    pub probe_first: bool,
    pub script_ver: String,
}

impl RoundEntry {
    pub fn new(seq: u32, order: usize, probe_first: bool, script_ver: &str) -> Result<Self, u32> {
        if order >= ORD_COUNT {
            return Err(OR_ORDER_UNK);
        }
        if script_ver.is_empty() {
            return Err(OR_VER_EMPTY);
        }
        if seq == 0 {
            return Err(OR_SEQ_BACK);
        }
        Ok(RoundEntry {
            seq,
            order,
            probe_first,
            script_ver: script_ver.to_string(),
        })
    }
}

/// 编排器：轮账 + counterbalance + 版本一致性。
pub struct Orchestrator {
    pub rounds: Vec<RoundEntry>,
    pub script_ver: String,
    pub sealed: bool,
}

impl Orchestrator {
    pub fn new(script_ver: &str) -> Result<Self, u32> {
        if script_ver.is_empty() {
            return Err(OR_VER_EMPTY);
        }
        Ok(Orchestrator {
            rounds: Vec::new(),
            script_ver: script_ver.to_string(),
            sealed: false,
        })
    }

    /// 编排一轮：探针必须先行 + 版本一致 + 轮号单调。
    pub fn plan_round(&mut self, seq: u32, order: usize, probe_first: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(OR_SEALED);
        }
        if !probe_first {
            return Err(OR_PROBE_NOT_FIRST);
        }
        if let Some(prev) = self.rounds.last() {
            if seq == prev.seq {
                return Err(OR_DUP_ROUND);
            }
            if seq < prev.seq {
                return Err(OR_SEQ_BACK);
            }
            if prev.script_ver != self.script_ver {
                return Err(OR_VER_DRIFT);
            }
        }
        let e = RoundEntry::new(seq, order, probe_first, &self.script_ver)?;
        self.rounds.push(e);
        Ok(())
    }

    /// J2：counterbalance 核（两序轮数差 ≤1 且总数 ≥2）。
    pub fn counterbalance_ok(&self) -> bool {
        let mut a = 0usize;
        let mut b = 0usize;
        for r in self.rounds.iter() {
            if r.order == ORD_VARIX_FIRST {
                a += 1;
            } else {
                b += 1;
            }
        }
        self.rounds.len() >= 2
            && (a as i64 - b as i64).abs() <= 1
    }

    /// J2'：交替核（相邻轮顺序互异——严格交替口径）。
    pub fn alternating(&self) -> bool {
        for w in self.rounds.windows(2) {
            if w[0].order == w[1].order {
                return false;
            }
        }
        self.rounds.len() >= 2
    }

    /// J1：版本一致核（逐轮版本与编排器版本同）。
    pub fn versions_consistent(&self) -> bool {
        if self.rounds.is_empty() {
            return false;
        }
        self.rounds.iter().all(|r| r.script_ver == self.script_ver)
    }

    /// J3：日志归账齐（逐轮探针旗全真 + 轮号连续）。
    pub fn log_ok(&self) -> bool {
        if self.rounds.is_empty() {
            return false;
        }
        for (i, r) in self.rounds.iter().enumerate() {
            if !r.probe_first || r.seq != i as u32 + 1 {
                return false;
            }
        }
        true
    }

    /// 日志渲染。
    pub fn render_log(&self) -> String {
        let mut out = String::new();
        for r in self.rounds.iter() {
            out.push_str(&format!(
                "ROUND|{}|{}|probe=1|ver={}\n",
                r.seq, ORD_NAMES[r.order], r.script_ver
            ));
        }
        out
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：6 轮严格交替（奇数轮 varix 先 / 偶数轮 win 先）。
pub fn standard_orch() -> Orchestrator {
    let mut o = Orchestrator::new("load-1.0.0").expect("orch");
    for i in 0..6u32 {
        let order = if i % 2 == 0 { ORD_VARIX_FIRST } else { ORD_WIN_FIRST };
        o.plan_round(i + 1, order, true).expect("round");
    }
    o
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集顺序（order=9）
    let mut o = Orchestrator::new("v").expect("o");
    if o.plan_round(1, 9, true) == Err(OR_ORDER_UNK) {
        caught += 1;
    }
    // 2) 探针未先行
    if o.plan_round(1, ORD_VARIX_FIRST, false) == Err(OR_PROBE_NOT_FIRST) {
        caught += 1;
    }
    // 3) 版本空
    if Orchestrator::new("").err() == Some(OR_VER_EMPTY) {
        caught += 1;
    }
    // 4) 重复轮号
    let mut o2 = standard_orch();
    if o2.plan_round(6, ORD_WIN_FIRST, true) == Err(OR_DUP_ROUND) {
        caught += 1;
    }
    // 5) 轮号回退（seq=2 回退）与重复轮号（seq=6 撞尾）
    let mut o3 = standard_orch();
    if o3.plan_round(2, ORD_WIN_FIRST, true) == Err(OR_SEQ_BACK)
        && o3.plan_round(6, ORD_WIN_FIRST, true) == Err(OR_DUP_ROUND)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56034_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56034");
    // J1-1 六轮同版本落账
    let o = standard_orch();
    let ok = o.rounds.len() == 6 && o.versions_consistent();
    s.add("56034-J1-同版本六轮", ok, if ok { "" } else { "版本漂移" });
    // J1-2 顺序闭集名同源
    let ok = ORD_NAMES[ORD_VARIX_FIRST] == "varix_first"
        && ORD_NAMES[ORD_WIN_FIRST] == "win_first";
    s.add("56034-J1-顺序名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-1 counterbalance（两序各 3，差 0）
    let ok = o.counterbalance_ok();
    s.add("56034-J2-counterbalance", ok, if ok { "" } else { "两序失衡" });
    // J2-2 严格交替（奇 varix 先 / 偶 win 先）
    let ok = o.alternating()
        && o.rounds[0].order == ORD_VARIX_FIRST
        && o.rounds[1].order == ORD_WIN_FIRST;
    s.add("56034-J2-严格交替", ok, if ok { "" } else { "交替破" });
    // J2-3 失衡检出（4:2 差 2 → counterbalance 假）
    let mut o2 = Orchestrator::new("v").expect("o2");
    for i in 0..6u32 {
        let order = if i < 4 { ORD_VARIX_FIRST } else { ORD_WIN_FIRST };
        o2.plan_round(i + 1, order, true).expect("r");
    }
    let ok = !o2.counterbalance_ok();
    s.add("56034-J2-失衡检出", ok, if ok { "" } else { "失衡漏判" });
    // J3-1 日志归账齐（探针全先行 + 轮号连续 1..6）
    let ok = o.log_ok();
    s.add("56034-J3-日志归账齐", ok, if ok { "" } else { "日志链断" });
    // J3-2 日志渲染机读（ROUND 行逐轮带序名与版本）
    let text = o.render_log();
    let ok = text.lines().count() == 6
        && text.contains("ROUND|1|varix_first|probe=1|ver=load-1.0.0")
        && text.contains("ROUND|2|win_first");
    s.add("56034-J3-日志机读", ok, if ok { "" } else { "渲染面漂移" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56034-J3-注入核必抓", ok, &detail);
    // J1-3 探针先行旗逐轮全真
    let ok = o.rounds.iter().all(|r| r.probe_first);
    s.add("56034-J1-探针先行全真", ok, if ok { "" } else { "探针缺位" });
    // J3-4 封账冻结
    let mut o3 = standard_orch();
    o3.seal();
    let ok = o3.plan_round(7, ORD_VARIX_FIRST, true) == Err(OR_SEALED);
    s.add("56034-J3-封账冻结", ok, if ok { "" } else { "封账后可编" });
    // J2-4 单序全跑不交替（交替核判假留证）
    let mut o4 = Orchestrator::new("v").expect("o4");
    for i in 0..4u32 {
        o4.plan_round(i + 1, ORD_VARIX_FIRST, true).expect("r4");
    }
    let ok = !o4.alternating() && !o4.counterbalance_ok();
    s.add("56034-J2-单序留证", ok, if ok { "" } else { "单序误判" });
    // J1-4 轮号零哨兵拒
    let ok = RoundEntry::new(0, ORD_VARIX_FIRST, true, "v").err() == Some(OR_SEQ_BACK);
    s.add("56034-J1-轮号零哨兵", ok, if ok { "" } else { "零哨兵穿透" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56034_all_checks_green() {
        let set = run_unx56034_checks();
        assert!(set.all_passed(), "UNX-F56034 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56034_alternating() {
        let o = standard_orch();
        assert!(o.alternating());
        assert!(o.counterbalance_ok());
    }
}
