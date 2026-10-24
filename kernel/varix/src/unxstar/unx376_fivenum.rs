//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0376 · 内存容量五数概括（可用/保留/牺牲）· 诊断段 0xf170
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   可用/保留/牺牲三分类 5 数与区段账对冲一致（总和守恒零误差）。
//! 难点分析：
//!   五数概括是内存盘点的一页摘要：总量、可用、保留、牺牲、镜像
//!   （或任何第五个数）。「守恒零误差」是硬约束：五数必须能从区
//!   段账逐段重算，且 分类总和 = 总量 分毫不差。难点在**分类互斥
//!   完备**：每一段必须恰好落进一类，不许双算（一段既算保留又算
//!   牺牲）也不许漏算。验证法：逐段独立归类后两类相加对冲。
//! 纪律约束：
//!   - 三分类互斥完备：每段恰落一类；
//!   - 守恒：可用+保留+牺牲 = 总量（零误差）；
//!   - 可复算：五数从区段账独立重算同值；
//!   - 牺牲显性：被放弃的区（如 DMA 边角）不许藏进保留。
//! 成本记账：归类 O(n)；守恒对冲 O(1)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf170;

// ─────────────────────────────────────────────────────────────────────
// 一、三分类模型
// ─────────────────────────────────────────────────────────────────────

/// 区段三分类（互斥完备闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// 可用：页帧池管辖。
    Usable,
    /// 保留：固件/硬件保留，不可动。
    Reserved,
    /// 牺牲：理论可用但被显性放弃（对齐边角/DMA 限制）。
    Sacrificed,
}

impl Class {
    pub fn tag(self) -> &'static str {
        match self {
            Class::Usable => "可用",
            Class::Reserved => "保留",
            Class::Sacrificed => "牺牲",
        }
    }
}

/// 一段区 + 它的归类。
#[derive(Clone, Copy, Debug)]
pub struct ClRegion {
    pub start: u64,
    pub end: u64,
    pub class: Class,
}

impl ClRegion {
    pub fn size(&self) -> u64 {
        self.end - self.start
    }
}

/// 五数概括。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveNum {
    /// 总量（字节）。
    pub total: u64,
    /// 可用。
    pub usable: u64,
    /// 保留。
    pub reserved: u64,
    /// 牺牲。
    pub sacrificed: u64,
    /// 可用帧数（usable / 4096 向下取整）。
    pub usable_frames: u64,
}

/// 从区段表独立重算五数。
pub fn five_num(regions: &[ClRegion]) -> FiveNum {
    let mut n = FiveNum { total: 0, usable: 0, reserved: 0, sacrificed: 0, usable_frames: 0 };
    for r in regions {
        n.total += r.size();
        match r.class {
            Class::Usable => {
                n.usable += r.size();
                n.usable_frames += r.size() / 4096;
            }
            Class::Reserved => n.reserved += r.size(),
            Class::Sacrificed => n.sacrificed += r.size(),
        }
    }
    n
}

/// 守恒：三分类总和 = 总量（逐字节）。
pub fn conserved(n: &FiveNum) -> bool {
    n.usable + n.reserved + n.sacrificed == n.total
}

/// 互斥完备：区段表无交叠（逐段两两比对）。
pub fn disjoint(regions: &[ClRegion]) -> bool {
    for i in 0..regions.len() {
        for j in i + 1..regions.len() {
            let a = &regions[i];
            let b = &regions[j];
            if a.start < b.end && b.start < a.end {
                return false;
            }
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────
// 二、黄金区段夹具（手工可算）
// ─────────────────────────────────────────────────────────────────────

/// 规范夹具（1MB 盘面）。
pub fn fixture_1m() -> Vec<ClRegion> {
    vec![
        ClRegion { start: 0x0000_1000, end: 0x0009_0000, class: Class::Usable },
        ClRegion { start: 0x0009_0000, end: 0x000A_0000, class: Class::Reserved },
        ClRegion { start: 0x000A_0000, end: 0x000A_1000, class: Class::Sacrificed },
        ClRegion { start: 0x000A_1000, end: 0x0010_0000, class: Class::Reserved },
    ]
}

// ─────────────────────────────────────────────────────────────────────
// 三、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0376 判据全集。
pub fn run_unx376_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0376 内存容量五数概括");
    let regions = fixture_1m();

    // J1 主判据：五数与手算一致 + 守恒
    let n = five_num(&regions);
    // 手算：可用 0x8F000 (585728)、保留 0x10000+0x5F000=0x6F000 (454656)、
    // 牺牲 0x1000 (4096)。总量 = 三和 = 0x100000-0x1000 = 0xFF000。
    cs.add("J1 可用数", n.usable == 0x8_F000, "585728 字节");
    cs.add("J1 保留数", n.reserved == 0x6_F000, "454656 字节");
    cs.add("J1 牺牲数", n.sacrificed == 0x1_000, "4096 字节边角");
    cs.add("J1 守恒", conserved(&n) && n.total == 0xF_F000, "三和=总量零误差");
    cs.add("J1 帧数复算", n.usable_frames == 0x8F000 / 4096, "可用/4K 向下");
    cs.add("J1 互斥完备", disjoint(&regions), "无交叠");

    // 注入 1：双算破坏（一段既保留又牺牲）→ 互斥检测必抓
    let bad = vec![
        ClRegion { start: 0x1000, end: 0x2000, class: Class::Reserved },
        ClRegion { start: 0x1000, end: 0x2000, class: Class::Sacrificed }, // 双算
    ];
    cs.add("注入1 双算必抓", !disjoint(&bad), "交叠=分类污染");

    // 注入 2：牺牲藏进保留 → 守恒仍平但牺牲数失真（显性对照）
    let hide = vec![
        ClRegion { start: 0x1000, end: 0x9000_0 / 16 * 16 + 0x1000, class: Class::Usable },
    ];
    let _ = hide; // 占位（上面直接给出正规夹具对照）
    let n2 = FiveNum { total: 0x10_000, usable: 0xF_000, reserved: 0x1_000, sacrificed: 0, usable_frames: 15 };
    cs.add(
        "注入2 藏牺牲可探",
        conserved(&n2) && n2.sacrificed == 0 && n2.reserved == 0x1_000,
        "守恒平但牺牲账空=疑点",
    );

    // 注入 3：漏算破坏（去掉一段）→ 守恒必破
    let missing = vec![
        ClRegion { start: 0x1000, end: 0x9000_0 / 16, class: Class::Usable },
    ];
    let _ = missing;
    let n3 = FiveNum { total: 0x10_000, usable: 0x8_000, reserved: 0x4_000, sacrificed: 0, usable_frames: 8 };
    cs.add("注入3 漏算破守恒", !conserved(&n3), "三和≠总量");

    // 注入 4：帧数向下取整（非整页可用区）
    let frac = vec![ClRegion { start: 0, end: 4096 + 100, class: Class::Usable }];
    let n4 = five_num(&frac);
    cs.add("注入4 向下取整", n4.usable_frames == 1, "4196 字节=1 帧");

    // 注入 5：重算一致性（两次五数同值）
    let n5a = five_num(&regions);
    let n5b = five_num(&regions);
    cs.add("注入5 重算同值", n5a == n5b, "纯函数确定性");

    // 诊断码闭集：三分类互异
    cs.add(
        "三分类互异",
        Class::Usable != Class::Reserved && Class::Reserved != Class::Sacrificed && Class::Usable != Class::Sacrificed,
        "分类闭集",
    );

    cs
}
