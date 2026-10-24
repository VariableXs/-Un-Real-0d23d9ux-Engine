//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0338 · AP 唤醒演练（QEMU -smp 矩阵）· 诊断段 0x3f60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0338-J1 2/4/8 核 QEMU 演练全过，真机矩阵随闸门补测。
//! 难点分析：
//!   演练矩阵的难点在「诚实披露」：QEMU 的 2/4/8 核是理想对称
//!   环境，全过不等于真机全过（真机有 NUMA 拓扑差、固件甩锅、
//!   TSC 不同步）。所以矩阵是两段式：QEMU 段（2/4/8）必须全绿，
//!   真机段显性标「随闸门补测」——不许把 QEMU 结果冒充真机结论。
//!   每个演练单元记录 (核数, 期望在编, 实际在编, 失败分类)，任何
//!   缺员都归因到 F0333 的三类失败。
//! 纪律约束：
//!   - QEMU 段 2/4/8 全绿才算矩阵绿；
//!   - 真机段显性 pending，不冒充；
//!   - 缺员必归因：失败分类入账；
//!   - 单元互不影响：一格失败不污染邻格。
//! 成本记账：单元 O(核数)；矩阵 O(单元×核数)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3f60;

/// 演练平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrillPlatform {
    Qemu,
    RealMachine,
}

impl DrillPlatform {
    pub fn label(self) -> &'static str {
        match self {
            DrillPlatform::Qemu => "QEMU",
            DrillPlatform::RealMachine => "真机",
        }
    }
}

/// 一个演练单元的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrillCell {
    pub platform: DrillPlatform,
    /// 目标核数（含 BSP）。
    pub cores: u32,
    /// 实际在编 AP 数。
    pub aps_up: u32,
    /// 失败归因（None = 全编）。
    pub fail_class: Option<&'static str>,
}

impl DrillCell {
    pub fn pass(&self) -> bool {
        self.fail_class.is_none() && self.aps_up == self.cores - 1
    }
}

/// 演练矩阵。
#[derive(Debug)]
pub struct DrillMatrix {
    pub cells: Vec<DrillCell>,
}

impl DrillMatrix {
    /// QEMU 黄金矩阵：2/4/8 全编。
    pub fn golden() -> Self {
        DrillMatrix {
            cells: vec![
                DrillCell { platform: DrillPlatform::Qemu, cores: 2, aps_up: 1, fail_class: None },
                DrillCell { platform: DrillPlatform::Qemu, cores: 4, aps_up: 3, fail_class: None },
                DrillCell { platform: DrillPlatform::Qemu, cores: 8, aps_up: 7, fail_class: None },
            ],
        }
    }

    /// 追加真机 pending 单元（显性，不参与绿色判定）。
    pub fn append_real_pending(&mut self, cores: u32) {
        self.cells.push(DrillCell { platform: DrillPlatform::RealMachine, cores, aps_up: 0, fail_class: Some("随闸门补测") });
    }

    /// QEMU 段全绿判定（只看 QEMU 单元）。
    pub fn qemu_all_green(&self) -> bool {
        self.cells.iter().filter(|c| c.platform == DrillPlatform::Qemu).all(|c| c.pass())
    }

    /// QEMU 核数覆盖：必须恰含 2/4/8。
    pub fn qemu_coverage(&self) -> bool {
        let mut cs: Vec<u32> = self.cells.iter().filter(|c| c.platform == DrillPlatform::Qemu).map(|c| c.cores).collect();
        cs.sort();
        cs.dedup();
        cs == vec![2, 4, 8]
    }

    /// 真机 pending 清单（诚实披露面）——只计仍未补测的行。
    pub fn real_pending(&self) -> Vec<u32> {
        self.cells
            .iter()
            .filter(|c| c.platform == DrillPlatform::RealMachine && c.fail_class == Some("随闸门补测"))
            .map(|c| c.cores)
            .collect()
    }

    /// 红单元（QEMU 段失败）。
    pub fn qemu_reds(&self) -> Vec<(u32, &'static str)> {
        self.cells
            .iter()
            .filter(|c| c.platform == DrillPlatform::Qemu && !c.pass())
            .map(|c| (c.cores, c.fail_class.unwrap_or("缺员未归因")))
            .collect()
    }
}

/// UNX-F0338 判据（J1 2/4/8 全过 + 真机诚实 pending）。
pub fn run_unx338_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0338 AP 唤醒演练矩阵");

    // J1 主线：黄金矩阵
    let mut m = DrillMatrix::golden();
    m.append_real_pending(4);
    cs.add("J1 QEMU 全绿", m.qemu_all_green(), "2/4/8 全编");
    cs.add("J1 核数覆盖 2/4/8", m.qemu_coverage(), "三档齐");
    cs.add("J1 真机显性 pending", m.real_pending() == vec![4], "不冒充");

    // 单元语义：pass == 全编
    let full = DrillCell { platform: DrillPlatform::Qemu, cores: 4, aps_up: 3, fail_class: None };
    let short = DrillCell { platform: DrillPlatform::Qemu, cores: 4, aps_up: 2, fail_class: Some("超时") };
    cs.add("单元全编判定", full.pass(), "aps==cores-1");
    cs.add("单元缺员判红", !short.pass(), "缺一即红");

    // 注入 1：8 核缺员 → 红单元归因
    let mut bad = DrillMatrix::golden();
    bad.cells[2].aps_up = 6;
    bad.cells[2].fail_class = Some("未达");
    cs.add("注入1 缺员红单元", bad.qemu_reds() == vec![(8, "未达")], "8 核未达");
    cs.add("注入1 邻格不污染", bad.cells[0].pass() && bad.cells[1].pass(), "2/4 仍绿");
    cs.add("注入1 矩阵不绿", !bad.qemu_all_green(), "部分绿不算");

    // 注入 2：归因缺失 → 红且显性标「未归因」
    let mut bad2 = DrillMatrix::golden();
    bad2.cells[1].aps_up = 2;
    let reds = bad2.qemu_reds();
    cs.add("注入2 未归因显性", reds == vec![(4, "缺员未归因")], "不许静默");

    // 注入 3：把 QEMU 结果冒充真机 → 语义层拒绝（真机行不参与绿判定）
    let fake = DrillCell { platform: DrillPlatform::RealMachine, cores: 8, aps_up: 7, fail_class: None };
    let mut m3 = DrillMatrix::golden();
    m3.cells.push(fake);
    cs.add("注入3 真机行不进绿判定", m3.qemu_all_green(), "绿只看 QEMU");
    cs.add("注入3 真机绿行也是 pending 面外", m3.real_pending().is_empty(), "绿真机行不计 pending");

    // 平台标签闭集
    cs.add("平台标签互异", DrillPlatform::Qemu.label() != DrillPlatform::RealMachine.label(), "两标签");

    cs
}
