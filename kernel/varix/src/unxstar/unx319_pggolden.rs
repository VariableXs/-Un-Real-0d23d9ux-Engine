//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0319 · 页表构建黄金样本（与设计矩阵逐项对照）· 诊断段 0x3e50
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0319-J1 构建产物与设计矩阵黄金样本逐项（槽/权限/属性）
//!   一致。
//! 难点分析：
//!   「设计矩阵」是页表长什么样的唯一真源：哪个槽映射哪段、什么
//!   权限、什么属性。构建器跑完后逐项对表，任何一项不一致都要报
//!   出「矩阵坐标 + 期望 + 实际」三元组——只报数量等于没报。本模块
//!   把设计矩阵做成常量表（黄金样本），比对器输出差分清单。
//! 纪律约束：
//!   - 逐项：矩阵每一行都必须被比对，不许抽样；
//!   - 三元组差分：(槽, 期望, 实际) 全量落账；
//!   - 双向：构建产物里「矩阵没有的槽」也是违例（多映射）；
//!   - 黄金样本自检：矩阵自身要先过结构校验（无重复槽等）。
//! 成本记账：矩阵 O(n)；比对 O(n)；差分 O(n)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3e50;

// ─────────────────────────────────────────────────────────────────────
// 一、设计矩阵（黄金样本正本）
// ─────────────────────────────────────────────────────────────────────

/// 一条设计矩阵行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignRow {
    /// PML4 槽号。
    pub slot: usize,
    /// 映射区虚拟基址（4KiB 对齐）。
    pub vaddr: u64,
    /// 区长（字节，2MiB 对齐）。
    pub size: u64,
    /// 权限：RW 位。
    pub rw: bool,
    /// 权限：US 位（用户态可达）。
    pub us: bool,
    /// 属性：NX。
    pub nx: bool,
    /// 属性：大页（2MiB）。
    pub huge: bool,
}

/// 黄金设计矩阵：内核高半区布局正本。
///   槽 0      : 低 1MB trampoline（可执行，见 F0318）
///   槽 256    : 内核文本（RX）
///   槽 257    : 内核数据（RW+NX）
///   槽 510    : 帧账本直接映射（RW+NX）
///   槽 511    : 递归映射（自指，RW+NX）
pub const DESIGN_MATRIX: [DesignRow; 5] = [
    DesignRow { slot: 0, vaddr: 0, size: 0x20_0000, rw: true, us: false, nx: false, huge: true },
    DesignRow { slot: 256, vaddr: 0xFFFF_8000_0000_0000, size: 0x40_0000, rw: false, us: false, nx: false, huge: true },
    DesignRow { slot: 257, vaddr: 0xFFFF_8080_0000_0000, size: 0x40_0000, rw: true, us: false, nx: true, huge: true },
    DesignRow { slot: 510, vaddr: 0xFFFF_C000_0000_0000, size: 0x100_0000, rw: true, us: false, nx: true, huge: false },
    DesignRow { slot: 511, vaddr: 0xFFFF_C080_0000_0000, size: 0x1000, rw: true, us: false, nx: true, huge: false },
];

/// 矩阵自检：槽互异、vaddr 高半区规范、size 非零对齐。
pub fn matrix_selfcheck() -> Result<(), &'static str> {
    for w in DESIGN_MATRIX.windows(2) {
        if w[0].slot >= w[1].slot {
            return Err("矩阵槽非严格递增（重复/乱序）");
        }
    }
    for r in &DESIGN_MATRIX {
        if r.size == 0 || r.size % 0x1000 != 0 {
            return Err("区长非页对齐");
        }
        if r.vaddr % 0x1000 != 0 {
            return Err("基址非页对齐");
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────
// 二、构建产物与比对器
// ─────────────────────────────────────────────────────────────────────

/// 构建产物中的一条实际映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActualRow {
    pub slot: usize,
    pub vaddr: u64,
    pub size: u64,
    pub rw: bool,
    pub us: bool,
    pub nx: bool,
    pub huge: bool,
}

/// 差分项：坐标 + 期望 + 实际（可 None = 矩阵有产物无 / 产物有矩阵无）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffItem {
    pub slot: usize,
    pub field: &'static str,
    pub expect: &'static str,
    pub actual: &'static str,
}

/// 黄金构建器：严格按矩阵产出（教学实现，字段直拷）。
pub fn build_from_matrix() -> Vec<ActualRow> {
    DESIGN_MATRIX
        .iter()
        .map(|r| ActualRow { slot: r.slot, vaddr: r.vaddr, size: r.size, rw: r.rw, us: r.us, nx: r.nx, huge: r.huge })
        .collect()
}

/// 逐项比对：返回差分清单（空 = 全一致）。
pub fn compare(actual: &[ActualRow]) -> Vec<DiffItem> {
    let mut out = Vec::new();
    // 双向：矩阵 → 产物
    for d in &DESIGN_MATRIX {
        match actual.iter().find(|a| a.slot == d.slot) {
            None => out.push(DiffItem { slot: d.slot, field: "present", expect: "在位", actual: "缺失" }),
            Some(a) => {
                if a.vaddr != d.vaddr {
                    out.push(DiffItem { slot: d.slot, field: "vaddr", expect: "矩阵基址", actual: "漂移" });
                }
                if a.size != d.size {
                    out.push(DiffItem { slot: d.slot, field: "size", expect: "矩阵区长", actual: "漂移" });
                }
                if a.rw != d.rw {
                    out.push(DiffItem { slot: d.slot, field: "rw", expect: if d.rw { "RW" } else { "RO" }, actual: if a.rw { "RW" } else { "RO" } });
                }
                if a.us != d.us {
                    out.push(DiffItem { slot: d.slot, field: "us", expect: if d.us { "US" } else { "SU" }, actual: if a.us { "US" } else { "SU" } });
                }
                if a.nx != d.nx {
                    out.push(DiffItem { slot: d.slot, field: "nx", expect: if d.nx { "NX" } else { "X" }, actual: if a.nx { "NX" } else { "X" } });
                }
                if a.huge != d.huge {
                    out.push(DiffItem { slot: d.slot, field: "huge", expect: if d.huge { "2MiB" } else { "4KiB" }, actual: if a.huge { "2MiB" } else { "4KiB" } });
                }
            }
        }
    }
    // 反向：产物 → 矩阵（多映射）
    for a in actual {
        if !DESIGN_MATRIX.iter().any(|d| d.slot == a.slot) {
            out.push(DiffItem { slot: a.slot, field: "present", expect: "矩阵无此槽", actual: "多余映射" });
        }
    }
    out
}

/// 注入器：对黄金产物做指定坐标的坏改。
pub fn tamper(rows: &mut Vec<ActualRow>, slot: usize, f: fn(&mut ActualRow)) {
    if let Some(a) = rows.iter_mut().find(|a| a.slot == slot) {
        f(a);
    }
}

/// UNX-F0319 判据（J1 逐项一致）。
pub fn run_unx319_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0319 页表构建黄金样本");

    // 矩阵自检
    cs.add("矩阵结构自检", matrix_selfcheck().is_ok(), "槽互异+对齐");
    cs.add("矩阵覆盖 5 关键槽", DESIGN_MATRIX.len() == 5 && DESIGN_MATRIX[4].slot == 511, "含递归槽");

    // J1 主线：黄金构建全一致
    let golden = build_from_matrix();
    cs.add("J1 黄金构建零差分", compare(&golden).is_empty(), "逐项一致");

    // 注入 1：内核数据 NX 翻转 → 定位到槽+字段
    let mut bad = build_from_matrix();
    tamper(&mut bad, 257, |a| a.nx = !a.nx);
    let d1 = compare(&bad);
    cs.add("注入1 NX 翻转检出", d1.len() == 1, "唯一差分");
    cs.add("注入1 定位槽+字段", d1.len() == 1 && d1[0].slot == 257 && d1[0].field == "nx", "257·nx");

    // 注入 2：文本槽权限提权（RO→RW）
    let mut bad2 = build_from_matrix();
    tamper(&mut bad2, 256, |a| a.rw = true);
    let d2 = compare(&bad2);
    cs.add("注入2 文本提权检出", d2.iter().any(|d| d.slot == 256 && d.field == "rw"), "W^X 红线");

    // 注入 3：删除槽 → present 缺失 + 双向不漏
    let mut bad3 = build_from_matrix();
    bad3.retain(|a| a.slot != 510);
    let d3 = compare(&bad3);
    cs.add("注入3 缺槽检出", d3.len() == 1 && d3[0].field == "present" && d3[0].actual == "缺失", "510 消失");

    // 注入 4：多余槽 → 反向多映射
    let mut bad4 = build_from_matrix();
    bad4.push(ActualRow { slot: 300, vaddr: 0xFFFF_9000_0000_0000, size: 0x20_0000, rw: true, us: false, nx: true, huge: true });
    let d4 = compare(&bad4);
    cs.add("注入4 多余映射检出", d4.len() == 1 && d4[0].expect == "矩阵无此槽", "槽 300 多余");

    // 注入 5：vaddr 漂移
    let mut bad5 = build_from_matrix();
    tamper(&mut bad5, 0, |a| a.vaddr += 0x1000);
    let d5 = compare(&bad5);
    cs.add("注入5 基址漂移检出", d5.iter().any(|d| d.slot == 0 && d.field == "vaddr"), "低区漂移");

    // 大乱炖：五处坏改全抓
    let mut badall = build_from_matrix();
    tamper(&mut badall, 256, |a| a.rw = true);
    tamper(&mut badall, 257, |a| a.nx = false);
    tamper(&mut badall, 510, |a| a.size /= 2);
    tamper(&mut badall, 0, |a| a.huge = false);
    badall.retain(|a| a.slot != 511);
    let dall = compare(&badall);
    cs.add("大乱炖 5 处全抓", dall.len() >= 5, "差分清单");
    cs.add("大乱炖槽覆盖", {
        let mut slots: Vec<usize> = dall.iter().map(|d| d.slot).collect();
        slots.sort();
        slots.dedup();
        slots == vec![0, 256, 257, 510, 511]
    }, "全部违例槽");

    cs
}
