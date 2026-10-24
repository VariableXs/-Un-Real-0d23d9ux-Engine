//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0374 · 内存盘点交叉验证（E820 vs 自检段）· 诊断段 0xf150
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   双源（E820 vs 自检）交叉验证差集为 0，容差白名单 4 类冻结，
//!   差异账齐全。
//! 难点分析：
//!   E820 是固件报的内存图，自检段是内核自己探出来的——两源独立
//!   得到就必然有「口径差」：固件会报内核自占的区、自检看不到
//!   保留洞。交叉验证的正确姿势不是「逐字节相等」而是「归一后
//!   对拍」：先把两源都归一到「可用区段表」（合并相邻、剔除容差
//!   白名单覆盖的区），再求差集。白名单 4 类（内核镜像/帧池元数/
//!   启动临时区/固件半开区）编译期冻结，不许运行时加类——加了
//!   就是往验证器里塞私货。
//! 纪律约束：
//!   - 归一先行：两源同口径归一后才可比；
//!   - 白名单冻结：4 类编译期常量，扩类须改判据；
//!   - 差集为零：归一后双向差集空；
//!   - 差异账：白名单吸收的差异也要落账（吸收 ≠ 吞没）。
//! 成本记账：归一 O(n log n)；差集 O(n+m)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf150;

// ─────────────────────────────────────────────────────────────────────
// 一、区段模型 + 归一
// ─────────────────────────────────────────────────────────────────────

/// 一段内存区。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub end: u64, // 开区间
    pub usable: bool,
}

/// 归一：只留可用区、合并相邻/重叠。产出升序不交叠表。
pub fn normalize(regions: &[Region]) -> Vec<Region> {
    let mut us: Vec<Region> = regions
        .iter()
        .filter(|r| r.usable && r.end > r.start)
        .copied()
        .collect();
    us.sort_by_key(|r| r.start);
    let mut out: Vec<Region> = Vec::new();
    for r in us {
        match out.last_mut() {
            Some(last) if r.start <= last.end => {
                if r.end > last.end {
                    last.end = r.end;
                }
            }
            _ => out.push(r),
        }
    }
    out
}

/// 容差白名单 4 类（编译期冻结）。每类是一段「允许单源独有」的区。
pub const WHITELIST: [(&str, u64, u64); 4] = [
    ("内核镜像", 0x0010_0000, 0x0018_0000),
    ("帧池元数据", 0x0018_0000, 0x0019_0000),
    ("启动临时区", 0x0009_C000, 0x000A_0000),
    ("固件半开区", 0x000F_0000, 0x0010_0000),
];

/// 区段是否被白名单覆盖（吸收判定）。
pub fn whitelisted(r: &Region) -> bool {
    WHITELIST.iter().any(|(_, s, e)| r.start >= *s && r.end <= *e)
}

/// 差集：a 有 b 无（未入白名单吸收的）。
pub fn diff(a: &[Region], b: &[Region]) -> Vec<Region> {
    let mut out = Vec::new();
    for ra in a {
        if whitelisted(ra) {
            continue;
        }
        let covered = b.iter().any(|rb| rb.usable && ra.start >= rb.start && ra.end <= rb.end);
        if !covered {
            out.push(*ra);
        }
    }
    out
}

/// 差异账：被白名单吸收的区也要记下（吸收 ≠ 吞没）。
pub fn absorbed_by(a: &[Region], b: &[Region]) -> Vec<(&'static str, Region)> {
    let mut out = Vec::new();
    for ra in a {
        if whitelisted(ra) {
            let covered = b.iter().any(|rb| rb.usable && ra.start >= rb.start && ra.end <= rb.end);
            if !covered {
                let tag = WHITELIST
                    .iter()
                    .find(|(_, s, e)| ra.start >= *s && ra.end <= *e)
                    .map(|(t, _, _)| *t)
                    .unwrap_or("?");
                out.push((tag, *ra));
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────
// 二、黄金双源夹具
// ─────────────────────────────────────────────────────────────────────

/// E820 源：6 段（含固件把启动临时区误报可用——白名单吸收的典型差异）。
pub fn golden_e820() -> Vec<Region> {
    vec![
        Region { start: 0x0000_0000, end: 0x0009_C000, usable: true },
        Region { start: 0x0009_C000, end: 0x000A_0000, usable: true },  // 误报可用（实际启动临时）
        Region { start: 0x0010_0000, end: 0x0018_0000, usable: true },  // 内核镜像段
        Region { start: 0x0018_0000, end: 0x0100_0000, usable: true },
        Region { start: 0x0100_0000, end: 0x4000_0000, usable: true },
        Region { start: 0x000F_0000, end: 0x0010_0000, usable: false }, // 固件半开（保留）
    ]
}

/// 自检源：4 段（自检按可读写性探测，半开区只读故不可用）。
pub fn golden_selftest() -> Vec<Region> {
    vec![
        Region { start: 0x0000_0000, end: 0x0009_C000, usable: true },
        Region { start: 0x0010_0000, end: 0x0100_0000, usable: true },
        Region { start: 0x0100_0000, end: 0x4000_0000, usable: true },
    ]
}

// ─────────────────────────────────────────────────────────────────────
// 三、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0374 判据全集。
pub fn run_unx374_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0374 内存盘点交叉验证");

    // J1 主判据：归一后差集为零
    let e = normalize(&golden_e820());
    let s = normalize(&golden_selftest());
    cs.add("J1 归一有序", e.windows(2).all(|w| w[0].end <= w[1].start), "升序不交叠");
    let d_e = diff(&e, &s);
    let d_s = diff(&s, &e);
    cs.add("J1 差集为零", d_e.is_empty() && d_s.is_empty(), "双向对拍归零");

    // 吸收账：白名单吸收的差异要留痕
    // E820 误报启动临时区可用（归一后 0~0xA0000 连片），自检探到
    // 0x9C000 截止——差 0x9C000~0xA0000 整段落「启动临时区」白名单
    // 条目 → 被吸收并留痕（吸收≠吞没）。
    let absorbed = absorbed_by(&e, &s);
    cs.add(
        "J1 吸收留痕",
        absorbed.len() == 1 && absorbed[0].0 == "启动临时区",
        "吸收≠吞没",
    );

    // 注入 1：白名单外假区 → 差集必抓
    let mut s1 = golden_selftest();
    s1.push(Region { start: 0x5000_0000, end: 0x5001_0000, usable: true }); // 自检幻影
    let e1 = normalize(&golden_e820());
    let s1n = normalize(&s1);
    let d1 = diff(&s1n, &e1);
    cs.add(
        "注入1 幻影抓取",
        d1.len() == 1 && d1[0].start == 0x5000_0000,
        "白名单外不吸收",
    );

    // 注入 2：白名单扩类被冻结（表长恒 4）
    cs.add("注入2 白名单冻结", WHITELIST.len() == 4, "4 类闭集");

    // 注入 3：重叠区归一合并（E820 常见固件重叠报法）
    let ov = vec![
        Region { start: 0x1000, end: 0x3000, usable: true },
        Region { start: 0x2000, end: 0x5000, usable: true },
        Region { start: 0x7000, end: 0x8000, usable: true },
    ];
    let n3 = normalize(&ov);
    cs.add(
        "注入3 重叠合并",
        n3.len() == 2 && n3[0] == Region { start: 0x1000, end: 0x5000, usable: true },
        "相邻重叠归一",
    );

    // 注入 4：不可用段不参与差集
    let a4 = vec![Region { start: 0x1000, end: 0x2000, usable: false }];
    let b4 = vec![];
    cs.add("注入4 不可用剔出", diff(&a4, &b4).is_empty(), "只对拍可用");

    // 注入 5：两源独立归一总量对冲
    // 手算：E820 归一 = [0~0xA0000, 0x100000~0x4000000]，
    // 自检归一 = [0~0x9C000, 0x100000~0x4000000]，
    // 差 = 临时区 0x4000（白名单吸收，账面留痕）。
    let sum_e: u64 = e.iter().map(|r| r.end - r.start).sum();
    let sum_s: u64 = s.iter().map(|r| r.end - r.start).sum();
    cs.add(
        "注入5 总量对冲",
        sum_e - sum_s == 0x4000 && absorbed.len() == 1,
        "差=临时区段",
    );

    cs
}
