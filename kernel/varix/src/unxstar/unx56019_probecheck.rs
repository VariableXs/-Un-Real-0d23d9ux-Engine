//! UNX-F56019 环境探针自检件（探针本身的健康账）（0x60240 段）
//!
//! 锚点：UNX-F56019-J1 探针自身失败路径三态（工具缺失/权限不足/字段
//! 读取失败）显性化并留机器可读痕迹（逐次失败落账：探针名 × 失败态 ×
//! 诊断摘要）；J2 禁静默丢弃（每次探针运行必有结论：成功或三态之一，
//! 无结论即自检失败）；J3 自检健康账可导（探针闭集逐项健康位 + 静默
//! 丢弃计数恒零）三判据全绿。判据锚定 §AI-71.2 专题三错误路径纪律。
//! 域：CoRun·UNX-F2-B40（环境冻结域·探针自检面）。
//!
//! 结构：探针闭集登记册（六探针逐项落账）+ 三态失败闭集（工具缺失/
//! 权限不足/字段失败）+ 运行结论闸（必落结论，禁静默）+ 健康账导出 +
//! 注入核（越闭集探针/越闭集失败态/空摘要/静默丢弃/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60240 独占 0x20）。
pub const SEG_BASE: u32 = 0x60240;
pub const SC_PROBE_UNK: u32 = SEG_BASE + 0x01; // 越闭集探针
pub const SC_FAIL_UNK: u32 = SEG_BASE + 0x02; // 越闭集失败态
pub const SC_MSG_EMPTY: u32 = SEG_BASE + 0x03; // 诊断摘要空
pub const SC_SILENT_DROP: u32 = SEG_BASE + 0x04; // 静默丢弃（运行无结论）
pub const SC_SEALED: u32 = SEG_BASE + 0x05; // 自检账已封账拒写
pub const SC_DUP_PROBE: u32 = SEG_BASE + 0x06; // 同探针重复登记
pub const SC_HEALTH_MISS: u32 = SEG_BASE + 0x07; // 健康位缺（未跑自检即出账）

/// 探针闭集。
pub const P_HW: usize = 0;
pub const P_FW: usize = 1;
pub const P_POWER: usize = 2;
pub const P_FS: usize = 3;
pub const P_BG: usize = 4;
pub const P_CLOCK: usize = 5;
pub const P_COUNT: usize = 6;
pub const P_NAMES: [&str; P_COUNT] =
    ["probe_hw", "probe_fw", "probe_power", "probe_fs", "probe_bg", "probe_clock"];

/// 失败态闭集（三态）。
pub const F_TOOL_MISS: usize = 0;
pub const F_PERM: usize = 1;
pub const F_FIELD: usize = 2;
pub const F_COUNT: usize = 3;
pub const F_NAMES: [&str; F_COUNT] = ["tool_missing", "permission_denied", "field_read_fail"];

pub fn fail_state_ok(f: usize) -> bool {
    f < F_COUNT
}

/// 单次失败痕迹：探针 × 失败态 × 诊断摘要。
#[derive(Clone)]
pub struct FailTrace {
    pub probe: usize,
    pub state: usize,
    pub msg: String,
}

impl FailTrace {
    pub fn new(probe: usize, state: usize, msg: &str) -> Result<Self, u32> {
        if probe >= P_COUNT {
            return Err(SC_PROBE_UNK);
        }
        if !fail_state_ok(state) {
            return Err(SC_FAIL_UNK);
        }
        if msg.is_empty() {
            return Err(SC_MSG_EMPTY);
        }
        Ok(FailTrace { probe, state, msg: msg.to_string() })
    }

    /// 机器可读行（FAIL|探针|失败态|摘要 四段）。
    pub fn render(&self) -> String {
        format!("FAIL|{}|{}|{}", P_NAMES[self.probe], F_NAMES[self.state], self.msg)
    }
}

/// 探针自检账：探针登记 + 运行结论账（成功/失败必落其一）。
pub struct SelfCheckBook {
    pub registered: [bool; P_COUNT],
    pub outcomes: Vec<(usize, bool)>,
    pub failures: Vec<FailTrace>,
    pub silent_drops: usize,
    pub sealed: bool,
}

impl SelfCheckBook {
    pub fn new() -> Self {
        SelfCheckBook {
            registered: [false; P_COUNT],
            outcomes: Vec::new(),
            failures: Vec::new(),
            silent_drops: 0,
            sealed: false,
        }
    }

    /// 探针登记（同探针重复拒）。
    pub fn register(&mut self, probe: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        if probe >= P_COUNT {
            return Err(SC_PROBE_UNK);
        }
        if self.registered[probe] {
            return Err(SC_DUP_PROBE);
        }
        self.registered[probe] = true;
        Ok(())
    }

    /// 运行结论落账：成功落 (probe, true)；失败落 trace 并落 (probe, false)。
    /// 未登记探针运行即静默丢弃计数（自检失败口径）。
    pub fn record_success(&mut self, probe: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        if probe >= P_COUNT {
            self.silent_drops += 1;
            return Err(SC_PROBE_UNK);
        }
        if !self.registered[probe] {
            self.silent_drops += 1;
            return Err(SC_DUP_PROBE);
        }
        self.outcomes.push((probe, true));
        Ok(())
    }

    /// 失败结论落账（三态 trace 必落）。
    pub fn record_failure(&mut self, t: FailTrace) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        if t.probe >= P_COUNT {
            self.silent_drops += 1;
            return Err(SC_PROBE_UNK);
        }
        let probe = t.probe;
        self.failures.push(t);
        self.outcomes.push((probe, false));
        Ok(())
    }

    /// J2：禁静默丢弃核（静默计数恒零）。
    pub fn no_silent_drop(&self) -> bool {
        self.silent_drops == 0
    }

    /// J3：健康账导出（逐探针健康位：有成功结论且无未决失败即健康）。
    pub fn health_bits(&self) -> [bool; P_COUNT] {
        let mut bits = [false; P_COUNT];
        for p in 0..P_COUNT {
            let mut ran = false;
            let mut ok = true;
            for o in self.outcomes.iter() {
                if o.0 == p {
                    ran = true;
                    if !o.1 {
                        ok = false;
                    }
                }
            }
            bits[p] = ran && ok && self.registered[p];
        }
        bits
    }

    /// 健康账可导核（登记全 + 静默零 + 结论非空）。
    pub fn health_exportable(&self) -> bool {
        for r in self.registered.iter() {
            if !r {
                return false;
            }
        }
        self.no_silent_drop() && !self.outcomes.is_empty()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：六探针全登记 + 全成功结论。
pub fn standard_check() -> SelfCheckBook {
    let mut b = SelfCheckBook::new();
    for p in 0..P_COUNT {
        b.register(p).expect("fixture reg");
    }
    for p in 0..P_COUNT {
        b.record_success(p).expect("fixture ok");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集探针
    if FailTrace::new(9, F_PERM, "x").err() == Some(SC_PROBE_UNK) {
        caught += 1;
    }
    // 2) 越闭集失败态
    if FailTrace::new(P_HW, 7, "x").err() == Some(SC_FAIL_UNK) {
        caught += 1;
    }
    // 3) 空摘要
    if FailTrace::new(P_HW, F_TOOL_MISS, "").err() == Some(SC_MSG_EMPTY) {
        caught += 1;
    }
    // 4) 静默丢弃（未登记探针跑成功路径 → 计数 +1）
    let mut b = SelfCheckBook::new();
    if b.record_success(P_FS) == Err(SC_DUP_PROBE) && b.silent_drops == 1 {
        caught += 1;
    }
    // 5) 封账改写
    let mut b2 = standard_check();
    b2.seal();
    if b2.record_success(P_HW) == Err(SC_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 三态演示账：六探针各态注入一遍（错误路径显性化）。
pub fn tri_state_demo() -> SelfCheckBook {
    let mut b = standard_check();
    let _ = b.record_failure(FailTrace::new(P_HW, F_TOOL_MISS, "lspci 缺").expect("t1"));
    let _ = b.record_failure(FailTrace::new(P_FW, F_PERM, "msr 只读被拒").expect("t2"));
    let _ = b.record_failure(FailTrace::new(P_POWER, F_FIELD, "power_plan 字段空").expect("t3"));
    b
}

/// 判据入口（CheckSet）。
pub fn run_unx56019_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56019");
    // J1-1 探针闭集登记齐（六探针）
    let b = standard_check();
    let mut reg = 0usize;
    for r in b.registered.iter() {
        if *r {
            reg += 1;
        }
    }
    let ok = reg == P_COUNT;
    s.add("56019-J1-六探针登记齐", ok, if ok { "" } else { "探针登记缺" });
    // J1-2 三态失败闭集语义
    let ok = fail_state_ok(F_TOOL_MISS) && fail_state_ok(F_PERM) && fail_state_ok(F_FIELD)
        && !fail_state_ok(3);
    s.add("56019-J1-三态闭集", ok, if ok { "" } else { "失败态越集" });
    // J1-3 三态演示全落痕（trace 逐条机读）
    let demo = tri_state_demo();
    let ok = demo.failures.len() == 3
        && demo.failures[0].render().contains("probe_hw|tool_missing|")
        && demo.failures[1].render().contains("permission_denied")
        && demo.failures[2].render().contains("field_read_fail");
    s.add("56019-J1-三态痕迹机读", ok, if ok { "" } else { "痕迹不机读" });
    // J2-1 禁静默丢弃（静默计数恒零）
    let ok = demo.no_silent_drop() && demo.silent_drops == 0;
    s.add("56019-J2-禁静默丢弃", ok, if ok { "" } else { "静默丢弃发生" });
    // J2-2 每次运行必有结论（结论数 ≥ 探针数 + 失败数）
    let ok = demo.outcomes.len() == P_COUNT + 3;
    s.add("56019-J2-结论必有", ok, if ok { "" } else { "运行无结论" });
    // J2-3 失败结论健康位落假
    let bits = demo.health_bits();
    let ok = !bits[P_HW] && !bits[P_FW] && !bits[P_POWER] && bits[P_FS] && bits[P_CLOCK];
    s.add("56019-J2-失败位落假", ok, if ok { "" } else { "健康位漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56019-J3-注入核必抓", ok, &detail);
    // J3-2 健康账可导（全成功夹具全绿）
    let bits2 = standard_check().health_bits();
    let mut all_green = true;
    for b2 in bits2.iter() {
        if !b2 {
            all_green = false;
        }
    }
    let ok = all_green && standard_check().health_exportable();
    s.add("56019-J3-健康账全绿可导", ok, if ok { "" } else { "健康账不可导" });
    // J3-3 未登记探针重复拒（SC_DUP_PROBE 复用闸）
    let mut b3 = SelfCheckBook::new();
    let ok = b3.register(P_CLOCK).is_ok() && b3.register(P_CLOCK) == Err(SC_DUP_PROBE);
    s.add("56019-J3-重复登记拒", ok, if ok { "" } else { "重复穿透" });
    // J3-4 封账冻结
    let mut b4 = standard_check();
    b4.seal();
    let ok = b4.register(P_HW) == Err(SC_SEALED)
        && b4.record_success(P_HW) == Err(SC_SEALED)
        && b4.record_failure(FailTrace::new(P_HW, F_PERM, "x").expect("f")) == Err(SC_SEALED);
    s.add("56019-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 失败态名闭集同源
    let ok = F_NAMES[F_TOOL_MISS] == "tool_missing" && F_NAMES[F_FIELD] == "field_read_fail";
    s.add("56019-J1-失败态名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-4 全绿账零失败条目
    let b5 = standard_check();
    let ok = b5.failures.is_empty() && b5.outcomes.len() == P_COUNT;
    s.add("56019-J2-全绿零失败", ok, if ok { "" } else { "全绿夹具污染" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56019_all_checks_green() {
        let set = run_unx56019_checks();
        assert!(set.all_passed(), "UNX-F56019 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56019_no_silent() {
        let demo = tri_state_demo();
        assert!(demo.no_silent_drop());
        assert_eq!(demo.failures.len(), 3);
    }
}
