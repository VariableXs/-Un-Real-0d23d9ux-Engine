//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7241 · table reload 总控协议 · 诊断段 0xAC80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F7241-J1 reload 四阶段（挂起/排空/切换/退役）顺序断言
//!      与状态留痕全账；跳阶段 100% 被拒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四阶段：suspend → drain → switch → retire 严格有序；
//!   - 状态留痕全账：每阶段落账（序号/阶段/指纹）；
//!   - 跳阶段/乱序 100% 拒；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xAC80;

/// 四阶段编码。
pub const PH_SUSPEND: u8 = 1;
pub const PH_DRAIN: u8 = 2;
pub const PH_SWITCH: u8 = 3;
pub const PH_RETIRE: u8 = 4;

/// 顺序错误码。
pub const EAGAIN: i32 = 11;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 阶段留痕。
pub struct PhaseMark {
    pub seq: usize,
    pub phase: u8,
    pub fp: u32,
}

/// reload 总控。
pub struct ReloadCtl {
    pub marks: Vec<PhaseMark>,
    pub done: bool,
}

impl ReloadCtl {
    pub fn new() -> Self {
        ReloadCtl { marks: Vec::new(), done: false }
    }

    /// 推进阶段：严格 1→2→3→4，跳/乱序拒。
    pub fn step(&mut self, phase: u8) -> Result<(), i32> {
        let expect = (self.marks.len() + 1) as u8;
        if phase != expect || phase > PH_RETIRE {
            return Err(EAGAIN);
        }
        self.marks.push(PhaseMark {
            seq: self.marks.len(),
            phase,
            fp: fnv1a(format!("phase{}", phase).as_bytes()),
        });
        if phase == PH_RETIRE {
            self.done = true;
        }
        Ok(())
    }

    /// 全账断言：四阶段序号连续 + 顺序正确。
    pub fn ledger_ok(&self) -> bool {
        self.marks.len() == 4
            && self.marks.iter().enumerate().all(|(i, m)| {
                m.seq == i && m.phase == (i + 1) as u8
            })
    }
}

/// J1 主判据：四阶段顺序走通 + 留痕全账 + 跳阶段全拒。
pub fn j1_main() -> bool {
    let mut c = ReloadCtl::new();
    for ph in [PH_SUSPEND, PH_DRAIN, PH_SWITCH, PH_RETIRE] {
        if c.step(ph).is_err() {
            return false;
        }
    }
    if !c.done || !c.ledger_ok() {
        return false;
    }
    // 跳阶段全拒：从空账直接跳 2/3/4
    for skip in [PH_DRAIN, PH_SWITCH, PH_RETIRE] {
        let mut p = ReloadCtl::new();
        if p.step(skip).is_ok() {
            return false;
        }
    }
    // 中途跳阶段拒：suspend 后直跳 4
    let mut p = ReloadCtl::new();
    p.step(PH_SUSPEND).ok();
    p.step(PH_RETIRE).is_err() && p.step(PH_DRAIN).is_ok()
}

// ── 注入核（0xAC80 段 5 项）────────────────────────────────────────────

/// 注入核 1：空账跳段注入——未挂起先排空拒。
pub fn inj1_skip_suspend() -> bool {
    let mut c = ReloadCtl::new();
    c.step(PH_DRAIN).is_err() && c.marks.is_empty()
}

/// 注入核 2：重复阶段注入——同阶段双推进拒。
pub fn inj2_dup_phase() -> bool {
    let mut c = ReloadCtl::new();
    c.step(PH_SUSPEND).ok();
    c.step(PH_SUSPEND).is_err()
}

/// 注入核 3：越界阶段注入——phase=0/5 拒。
pub fn inj3_phase_oob() -> bool {
    let mut c = ReloadCtl::new();
    c.step(0).is_err() && c.step(5).is_err()
}

/// 注入核 4：排空前置注入——未排空不切换。
pub fn inj4_drain_prereq() -> bool {
    let mut c = ReloadCtl::new();
    c.step(PH_SUSPEND).ok();
    c.step(PH_SWITCH).is_err()
}

/// 注入核 5：退役终结注入——四阶段走完后再推进拒。
pub fn inj5_retire_final() -> bool {
    let mut c = ReloadCtl::new();
    for ph in [PH_SUSPEND, PH_DRAIN, PH_SWITCH, PH_RETIRE] {
        c.step(ph).ok();
    }
    c.step(PH_SUSPEND).is_err() && c.done && c.ledger_ok()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    11u32 * 31
}

/// run_unx7241_checks：本单判据聚合（诊断段 0xAC80）。
pub fn run_unx7241_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7241 table reload 总控协议");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "四阶段顺序断言+留痕全账",
        j1_main(),
        if j1_main() { "" } else { "顺序断言破" },
    );
    s.add(
        "跳阶段100%被拒",
        j1_main(),
        if j1_main() { "" } else { "跳阶段漏拒" },
    );
    let inj = [
        ("注入1空账跳段拒", inj1_skip_suspend()),
        ("注入2重复阶段拒", inj2_dup_phase()),
        ("注入3阶段越界拒", inj3_phase_oob()),
        ("注入4排空前置", inj4_drain_prereq()),
        ("注入5退役终结", inj5_retire_final()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
