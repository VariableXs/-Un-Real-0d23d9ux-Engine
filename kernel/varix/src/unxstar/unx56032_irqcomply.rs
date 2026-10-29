//! UNX-F56032 高频采集中断屏蔽合规件（0x603E0 段）
//!
//! 锚点：UNX-F56032-J1 采集窗内禁改中断/调度参数的合规断言（只观测
//! 不改造环境——O1 域边界红线，违规调用名逐条落违例账）；J2 违规路径
//! 静态扫描零命中（合规调用闭集外无任何改参调用名）；J3 合规证立（闭
//! 集调用逐条白名单比对全过才立证）三判据全绿。判据锚定 §AI-71.1 边
//! 界第一条。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·合规红线面）。
//!
//! 结构：合规调用闭集登记册（只观测类调用名逐条落账）+ 改参调用名黑
//! 名单闭集（禁改中断/调度/亲和性/优先级/时钟五类）+ 静态扫描核（采
//! 集器调用序列逐条比对：黑名单命中即违例落账）+ 合规证立账 + 注入核
//! （黑名单调用/空调用名/闭集越名/证立未立/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x603E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x603E0;
pub const IC_BLACK_HIT: u32 = SEG_BASE + 0x01; // 改参调用命中黑名单
pub const IC_NAME_EMPTY: u32 = SEG_BASE + 0x02; // 空调用名
pub const IC_NAME_UNK: u32 = SEG_BASE + 0x03; // 越白名单越黑名单的未知调用
pub const IC_PROOF_MISS: u32 = SEG_BASE + 0x04; // 合规证未立即采
pub const IC_SEALED: u32 = SEG_BASE + 0x05; // 合规账已封账拒写
pub const IC_DUP_PROOF: u32 = SEG_BASE + 0x06; // 同窗重复立证
pub const IC_VIOL_LOST: u32 = SEG_BASE + 0x07; // 违例未落账

/// 白名单闭集（只观测类调用，O1 红线口径）。
pub const WHITE_CALLS: [&str; 6] = [
    "clock_read",
    "cpu_id_read",
    "irq_count_read",
    "sched_stat_read",
    "msr_read",
    "tsc_read",
];

/// 黑名单闭集（禁改类调用，五类：中断/调度/亲和/优先级/时钟参数）。
pub const BLACK_CALLS: [&str; 5] = [
    "irq_mask_set",
    "sched_param_set",
    "affinity_set",
    "priority_set",
    "clock_param_set",
];

/// 白名单核。
pub fn whitelisted(call: &str) -> bool {
    for c in WHITE_CALLS.iter() {
        if *c == call {
            return true;
        }
    }
    false
}

/// 黑名单核。
pub fn blacklisted(call: &str) -> bool {
    for c in BLACK_CALLS.iter() {
        if *c == call {
            return true;
        }
    }
    false
}

/// 违例痕迹：窗号 × 调用名 × 码。
#[derive(Clone)]
pub struct Violation {
    pub window: u32,
    pub call: String,
    pub code: u32,
}

impl Violation {
    pub fn render(&self) -> String {
        format!("VIOL|w{}|{}|code={}", self.window, self.call, self.code)
    }
}

/// 合规账：调用序列扫描 + 违例账 + 立证账。
pub struct ComplyBook {
    pub violations: Vec<Violation>,
    pub proofs: Vec<u32>,
    pub sealed: bool,
}

impl ComplyBook {
    pub fn new() -> Self {
        ComplyBook { violations: Vec::new(), proofs: Vec::new(), sealed: false }
    }

    /// 静态扫描：采集窗调用序列逐条比对，黑名单命中落违例账。
    /// 返回命中数（零命中 = 本窗合规）。
    pub fn scan_window(&mut self, window: u32, calls: &[&str]) -> Result<usize, u32> {
        if self.sealed {
            return Err(IC_SEALED);
        }
        let mut hits = 0usize;
        for c in calls.iter() {
            if c.is_empty() {
                return Err(IC_NAME_EMPTY);
            }
            if blacklisted(c) {
                self.violations.push(Violation {
                    window,
                    call: (*c).to_string(),
                    code: IC_BLACK_HIT,
                });
                hits += 1;
            } else if !whitelisted(c) {
                self.violations.push(Violation {
                    window,
                    call: (*c).to_string(),
                    code: IC_NAME_UNK,
                });
                hits += 1;
            }
        }
        Ok(hits)
    }

    /// 立证：窗内零违例才可立（重复立证拒）。
    pub fn prove(&mut self, window: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IC_SEALED);
        }
        for p in self.proofs.iter() {
            if *p == window {
                return Err(IC_DUP_PROOF);
            }
        }
        for v in self.violations.iter() {
            if v.window == window {
                return Err(IC_PROOF_MISS);
            }
        }
        self.proofs.push(window);
        Ok(())
    }

    /// 某窗违例数。
    pub fn violations_of(&self, window: u32) -> usize {
        let mut n = 0usize;
        for v in self.violations.iter() {
            if v.window == window {
                n += 1;
            }
        }
        n
    }

    /// J1：违例账全落痕（逐条机读行可读）。
    pub fn violations_trail_ok(&self) -> bool {
        self.violations.iter().all(|v| v.render().starts_with("VIOL|w"))
    }

    /// J3：立证账齐。
    pub fn proof_count(&self) -> usize {
        self.proofs.len()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准采集窗调用序列（夹具：纯白名单 8 笔）。
pub const STANDARD_WINDOW_CALLS: [&str; 8] = [
    "tsc_read",
    "cpu_id_read",
    "clock_read",
    "tsc_read",
    "cpu_id_read",
    "irq_count_read",
    "sched_stat_read",
    "msr_read",
];

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 黑名单调用（中断屏蔽）
    let mut b = ComplyBook::new();
    if b.scan_window(1, &["tsc_read", "irq_mask_set"]).expect("scan") == 1
        && b.violations_of(1) == 1
    {
        caught += 1;
    }
    // 2) 空调用名
    let mut b2 = ComplyBook::new();
    if b2.scan_window(1, &[""]) == Err(IC_NAME_EMPTY) {
        caught += 1;
    }
    // 3) 未知调用名（既不白也不黑）
    let mut b3 = ComplyBook::new();
    if b3.scan_window(1, &["ghost_call"]).expect("scan") == 1
        && b3.violations[0].code == IC_NAME_UNK
    {
        caught += 1;
    }
    // 4) 有违例窗立证拒
    let mut b4 = ComplyBook::new();
    let _ = b4.scan_window(1, &["priority_set"]);
    if b4.prove(1) == Err(IC_PROOF_MISS) {
        caught += 1;
    }
    // 5) 封账改写
    let mut b5 = ComplyBook::new();
    b5.seal();
    if b5.scan_window(1, &["tsc_read"]) == Err(IC_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56032_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56032");
    // J1-1 标准窗零违例（纯白名单序列）
    let mut b = ComplyBook::new();
    let hits = b.scan_window(1, &STANDARD_WINDOW_CALLS).expect("scan");
    let ok = hits == 0 && b.violations_of(1) == 0;
    s.add("56032-J1-标准窗零违例", ok, if ok { "" } else { "标准窗误报" });
    // J1-2 黑名单五类闭集（中断/调度/亲和/优先级/时钟参数）
    let mut five_hit = true;
    for c in BLACK_CALLS.iter() {
        let mut b2 = ComplyBook::new();
        let _ = b2.scan_window(9, &[*c]);
        if b2.violations_of(9) != 1 {
            five_hit = false;
        }
    }
    let ok = five_hit && BLACK_CALLS.len() == 5;
    s.add("56032-J1-黑名单五类全抓", ok, if ok { "" } else { "黑名单漏抓" });
    // J1-3 白名单闭集（六观测调用全过）
    let mut all_white = true;
    for c in WHITE_CALLS.iter() {
        if !whitelisted(c) {
            all_white = false;
        }
    }
    let ok = all_white && WHITE_CALLS.len() == 6;
    s.add("56032-J1-白名单闭集", ok, if ok { "" } else { "白名单漂移" });
    // J2-1 违规路径静态扫描留痕（违例行机读）
    let mut b3 = ComplyBook::new();
    let _ = b3.scan_window(2, &["affinity_set", "tsc_read"]);
    let ok = b3.violations_of(2) == 1
        && b3.violations_trail_ok()
        && b3.violations[0].render() == format!("VIOL|w2|affinity_set|code={}", IC_BLACK_HIT);
    s.add("56032-J2-违例留痕机读", ok, if ok { "" } else { "违例行漂移" });
    // J2-2 混合窗（违例与合规同窗，违例逐条落账合规零误伤）
    let mut b4 = ComplyBook::new();
    let hits = b4.scan_window(3, &["clock_read", "sched_param_set", "msr_read"]).expect("scan");
    let ok = hits == 1 && b4.violations_of(3) == 1;
    s.add("56032-J2-混合窗差分", ok, if ok { "" } else { "混合窗漂移" });
    // J3-1 合规证立（零违例窗可立证）
    let mut b5 = ComplyBook::new();
    let _ = b5.scan_window(1, &STANDARD_WINDOW_CALLS);
    b5.prove(1).expect("prove");
    let ok = b5.proof_count() == 1;
    s.add("56032-J3-合规证立", ok, if ok { "" } else { "立证失败" });
    // J3-2 重复立证拒
    let ok = b5.prove(1) == Err(IC_DUP_PROOF);
    s.add("56032-J3-重复立证拒", ok, if ok { "" } else { "重复立证穿透" });
    // J3-3 有违例窗立证拒（红线即拒）
    let mut b6 = ComplyBook::new();
    let _ = b6.scan_window(4, &["clock_param_set"]);
    let ok = b6.prove(4) == Err(IC_PROOF_MISS);
    s.add("56032-J3-违例窗拒立证", ok, if ok { "" } else { "违例窗立证穿透" });
    // J3-4 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56032-J3-注入核必抓", ok, &detail);
    // J2-3 违例码语义（黑名单 IC_BLACK_HIT / 未知 IC_NAME_UNK 两态分明）
    let mut b7 = ComplyBook::new();
    let _ = b7.scan_window(5, &["irq_mask_set", "ghost"]);
    let ok = b7.violations[0].code == IC_BLACK_HIT && b7.violations[1].code == IC_NAME_UNK;
    s.add("56032-J2-违例码两态", ok, if ok { "" } else { "违例码混" });
    // J1-4 扫描对空序列零违例
    let mut b8 = ComplyBook::new();
    let ok = b8.scan_window(6, &[]).expect("scan") == 0;
    s.add("56032-J1-空序列零违例", ok, if ok { "" } else { "空序列误报" });
    // J2-4 封账冻结
    let mut b9 = ComplyBook::new();
    b9.seal();
    let ok = b9.scan_window(7, &["tsc_read"]) == Err(IC_SEALED)
        && b9.prove(7) == Err(IC_SEALED);
    s.add("56032-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56032_all_checks_green() {
        let set = run_unx56032_checks();
        assert!(set.all_passed(), "UNX-F56032 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56032_blacklist_closure() {
        assert!(blacklisted("irq_mask_set"));
        assert!(whitelisted("tsc_read"));
        assert!(!blacklisted("tsc_read") && !whitelisted("irq_mask_set"));
    }
}
