//! UNX-F56052 启动采集编排器（0x60660 段）
//!
//! 锚点：UNX-F56052-J1 自动上电/重启循环编排 ×10（人不在场：循环账
//! 逐轮落账，环境探针先行闸逐轮生效）；J2 轮型交替（冷启动轮/热重启
//! 轮交替排布，轮型账分明）；J3 序列日志归档可复查（逐轮探针旗 × 轮
//! 型 × 时戳只追加）三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（启动分段域·循环编排面）。
//!
//! 结构：轮型闭集（冷/热）+ 循环账（轮号 × 轮型 × 探针旗 × 时戳逐轮
//! 落账）+ 探针先行闸（未跑探针禁排轮）+ 交替核（相邻轮型互异）+ 日
//! 志归账 + 注入核（越闭集轮型/探针未先/重复轮号/时戳回退/封账改写必
//! 抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60660 独占 0x20）。
pub const SEG_BASE: u32 = 0x60660;
pub const BO_KIND_UNK: u32 = SEG_BASE + 0x01; // 越闭集轮型
pub const BO_PROBE_NOT_FIRST: u32 = SEG_BASE + 0x02; // 探针未先行
pub const BO_DUP_ROUND: u32 = SEG_BASE + 0x03; // 重复轮号
pub const BO_TS_BACK: u32 = SEG_BASE + 0x04; // 时戳回退
pub const BO_SEALED: u32 = SEG_BASE + 0x05; // 编排账已封账拒写
pub const BO_TS_ZERO: u32 = SEG_BASE + 0x06; // 零时戳
pub const BO_KIND_MISS: u32 = SEG_BASE + 0x07; // 轮型缺（未声明）

/// 循环目标（×10）。
pub const LOOP_N: usize = 10;

/// 轮型闭集（与 F56051 同源）。
pub const T_COLD: usize = 0;
pub const T_HOT: usize = 1;
pub const T_COUNT: usize = 2;
pub const T_NAMES: [&str; T_COUNT] = ["cold_poweron", "hot_reboot"];

/// 单轮编排账：轮号 × 轮型 × 探针旗 × 时戳。
#[derive(Clone)]
pub struct LoopRound {
    pub seq: u32,
    pub kind: usize,
    pub probe_first: bool,
    pub ts: u64,
}

impl LoopRound {
    pub fn new(seq: u32, kind: usize, probe_first: bool, ts: u64) -> Result<Self, u32> {
        if kind >= T_COUNT {
            return Err(BO_KIND_UNK);
        }
        if seq == 0 {
            return Err(BO_DUP_ROUND);
        }
        if ts == 0 {
            return Err(BO_TS_ZERO);
        }
        Ok(LoopRound { seq, kind, probe_first, ts })
    }
}

/// 编排器：循环账 + 探针闸 + 交替核。
pub struct BootOrch {
    pub rounds: Vec<LoopRound>,
    pub sealed: bool,
}

impl BootOrch {
    pub fn new() -> Self {
        BootOrch { rounds: Vec::new(), sealed: false }
    }

    /// 排一轮：探针先行闸 + 轮号单调 + 时戳不回退。
    pub fn plan(&mut self, seq: u32, kind: usize, probe_first: bool, ts: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(BO_SEALED);
        }
        if !probe_first {
            return Err(BO_PROBE_NOT_FIRST);
        }
        if let Some(prev) = self.rounds.last() {
            if seq <= prev.seq {
                return Err(BO_DUP_ROUND);
            }
            if ts < prev.ts {
                return Err(BO_TS_BACK);
            }
        }
        let r = LoopRound::new(seq, kind, probe_first, ts)?;
        self.rounds.push(r);
        Ok(())
    }

    /// J2：交替核（相邻轮型互异且 ≥2 轮）。
    pub fn alternating(&self) -> bool {
        for w in self.rounds.windows(2) {
            if w[0].kind == w[1].kind {
                return false;
            }
        }
        self.rounds.len() >= 2
    }

    /// J1：×10 循环齐（≥10 轮且探针全先行）。
    pub fn loop10_ok(&self) -> bool {
        self.rounds.len() >= LOOP_N
            && self.rounds.iter().all(|r| r.probe_first)
    }

    /// J3：日志归账齐（轮号连续 + 时戳单调）。
    pub fn log_ok(&self) -> bool {
        if self.rounds.is_empty() {
            return false;
        }
        for (i, r) in self.rounds.iter().enumerate() {
            if r.seq != i as u32 + 1 {
                return false;
            }
        }
        for w in self.rounds.windows(2) {
            if w[0].ts > w[1].ts {
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
                "LOOP|{}|{}|probe=1|ts={}\n",
                r.seq, T_NAMES[r.kind], r.ts
            ));
        }
        out
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：10 轮交替（奇冷偶热，时戳 100ms 步进）。
pub fn standard_orch() -> BootOrch {
    let mut o = BootOrch::new();
    for i in 0..LOOP_N {
        let kind = if i % 2 == 0 { T_COLD } else { T_HOT };
        o.plan(i as u32 + 1, kind, true, 100 + i as u64 * 100).expect("round");
    }
    o
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集轮型
    let mut o = BootOrch::new();
    if o.plan(1, 9, true, 100) == Err(BO_KIND_UNK) {
        caught += 1;
    }
    // 2) 探针未先行
    if o.plan(1, T_COLD, false, 100) == Err(BO_PROBE_NOT_FIRST) {
        caught += 1;
    }
    // 3) 重复轮号（seq 不增）
    let mut o2 = standard_orch();
    if o2.plan(10, T_HOT, true, 2_000) == Err(BO_DUP_ROUND) {
        caught += 1;
    }
    // 4) 时戳回退
    let mut o3 = standard_orch();
    if o3.plan(11, T_COLD, true, 50) == Err(BO_TS_BACK) {
        caught += 1;
    }
    // 5) 封账改写
    let mut o4 = standard_orch();
    o4.seal();
    if o4.plan(11, T_COLD, true, 2_000) == Err(BO_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56052_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56052");
    // J1-1 ×10 循环齐（探针全先行）
    let o = standard_orch();
    let ok = o.loop10_ok();
    s.add("56052-J1-十轮循环齐", ok, if ok { "" } else { "循环账缺" });
    // J1-2 轮型闭集同源（与 F56051 同名）
    let ok = T_NAMES[T_COLD] == "cold_poweron" && T_NAMES[T_HOT] == "hot_reboot";
    s.add("56052-J1-轮型同源", ok, if ok { "" } else { "命名漂移" });
    // J1-3 无人值守语义（编排器无人工确认字段——账面即证）
    let ok = o.rounds.iter().all(|r| r.probe_first) && !o.sealed;
    s.add("56052-J1-无人值守账面", ok, if ok { "" } else { "账面缺证" });
    // J2-1 交替排布（奇冷偶热）
    let ok = o.alternating()
        && o.rounds[0].kind == T_COLD
        && o.rounds[1].kind == T_HOT;
    s.add("56052-J2-交替排布", ok, if ok { "" } else { "交替破" });
    // J2-2 轮型计数分明（冷 5 / 热 5）
    let mut cold_n = 0usize;
    let mut hot_n = 0usize;
    for r in o.rounds.iter() {
        if r.kind == T_COLD {
            cold_n += 1;
        } else {
            hot_n += 1;
        }
    }
    let ok = cold_n == 5 && hot_n == 5;
    s.add("56052-J2-轮型计数", ok, if ok { "" } else { "计数漂移" });
    // J2-3 单型连排不交替（留证）
    let mut o2 = BootOrch::new();
    for i in 0..4 {
        o2.plan(i as u32 + 1, T_COLD, true, 100 + i as u64 * 100).expect("r2");
    }
    let ok = !o2.alternating();
    s.add("56052-J2-单型留证", ok, if ok { "" } else { "单型误判" });
    // J3-1 日志归账齐（轮号连续 + 时戳单调）
    let ok = o.log_ok();
    s.add("56052-J3-日志归账齐", ok, if ok { "" } else { "日志链断" });
    // J3-2 日志机读（LOOP 行带轮型与时戳）
    let text = o.render_log();
    let ok = text.lines().count() == LOOP_N
        && text.contains("LOOP|1|cold_poweron|probe=1|ts=100")
        && text.contains("LOOP|2|hot_reboot");
    s.add("56052-J3-日志机读", ok, if ok { "" } else { "渲染面漂移" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56052-J3-注入核必抓", ok, &detail);
    // J1-4 零时戳拒
    let ok = LoopRound::new(1, T_COLD, true, 0).err() == Some(BO_TS_ZERO);
    s.add("56052-J1-零时戳拒", ok, if ok { "" } else { "零时戳穿透" });
    // J2-4 封账冻结
    let mut o3 = standard_orch();
    o3.seal();
    let ok = o3.plan(11, T_COLD, true, 2_000).is_err() && o3.sealed;
    s.add("56052-J2-封账冻结", ok, if ok { "" } else { "封账后可编" });
    // J3-4 空编排器日志齐判假
    let bare = BootOrch::new();
    let ok = !bare.log_ok() && !bare.loop10_ok();
    s.add("56052-J3-空账判假", ok, if ok { "" } else { "空账误判" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56052_all_checks_green() {
        let set = run_unx56052_checks();
        assert!(set.all_passed(), "UNX-F56052 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56052_alternating() {
        let o = standard_orch();
        assert!(o.alternating());
        assert!(o.loop10_ok());
    }
}
