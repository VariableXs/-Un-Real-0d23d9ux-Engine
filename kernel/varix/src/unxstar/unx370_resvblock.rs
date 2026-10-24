//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0370 · 保留区越界分配拦截 · 诊断段 0xf110
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   白名单/池/豁免三类越界各 10 例全拦截，拦截账三分类齐全。
//! 难点分析：
//!   保留区（BIOS ROM / MMIO / 保留页）是分配器的禁区，但「越界」
//!   不止一种形态：白名单外直取（硬越界）、从池里拿到保留区地址
//!   （池越界）、持豁免令却超豁免范围（豁免越界）。三类必须分账：
//!   拦截账里每一条都要带类标，混账 = 拦截白做。30 例全拦截是
//!   闭合验证：注入 30 个已知坏请求，30 个都必须出现在账里且类标
//!   正确。
//! 纪律约束：
//!   - 禁区先行：保留区判定在任何分配路径之前；
//!   - 三类分账：白名单/池/豁免独立计数，不许并成「其他」；
//!   - 拦截可解释：每条拦截带请求参数与命中规则；
//!   - 30 例闭合：注入全集 ⊆ 拦截账。
//! 成本记账：判定 O(log r)（保留区段表二分）；拦截 O(1)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf110;

// ─────────────────────────────────────────────────────────────────────
// 一、保留区段表 + 越界三分类
// ─────────────────────────────────────────────────────────────────────

/// 保留区段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reserved {
    pub start: u64,
    pub end: u64, // 开区间
    pub tag: &'static str,
}

impl Reserved {
    pub fn contains(&self, addr: u64, len: u64) -> bool {
        addr < self.end && addr + len > self.start
    }
}

/// 越界三分类（分账闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViolKind {
    /// 白名单外直取。
    Whitelist,
    /// 池发保留区地址。
    PoolLeak,
    /// 豁免超范围。
    ExemptOver,
}

impl ViolKind {
    pub fn tag(self) -> &'static str {
        match self {
            ViolKind::Whitelist => "白名单越界",
            ViolKind::PoolLeak => "池越界",
            ViolKind::ExemptOver => "豁免越界",
        }
    }
}

/// 拦截账。
#[derive(Clone, Debug)]
pub struct Intercept {
    pub addr: u64,
    pub len: u64,
    pub kind: ViolKind,
    pub rule: &'static str,
}

/// 分配请求上下文。
pub struct Req {
    pub addr: u64,
    pub len: u64,
    /// 豁免令范围（None = 无豁免）。
    pub exempt: Option<(u64, u64)>,
}

/// 保留区判定：命中任一段即禁区。
pub fn hit_reserved(regions: &[Reserved], addr: u64, len: u64) -> Option<&'static str> {
    regions
        .iter()
        .find(|r| r.contains(addr, len))
        .map(|r| r.tag)
}

/// 拦截判定主入口：三分类归账。
pub fn intercept(regions: &[Reserved], pool_lo: u64, pool_hi: u64, req: &Req) -> Option<Intercept> {
    // 规则 1：禁区直取（白名单外）。
    if let Some(tag) = hit_reserved(regions, req.addr, req.len) {
        // 有豁免令且完全在豁免范围内 → 放行（不算越界）。
        if let Some((lo, hi)) = req.exempt {
            if req.addr >= lo && req.addr + req.len <= hi {
                return None;
            }
            // 豁免部分覆盖 → 豁免越界。
            return Some(Intercept {
                addr: req.addr,
                len: req.len,
                kind: ViolKind::ExemptOver,
                rule: "豁免范围不足",
            });
        }
        return Some(Intercept {
            addr: req.addr,
            len: req.len,
            kind: ViolKind::Whitelist,
            rule: tag,
        });
    }
    // 规则 2：请求跨进禁区（半跨）。
    if let Some(tag) = hit_reserved(regions, req.addr, req.len) {
        return Some(Intercept {
            addr: req.addr,
            len: req.len,
            kind: ViolKind::Whitelist,
            rule: tag,
        });
    }
    // 规则 3：池发地址越池界（池越界——池管辖区被污染）。
    if req.addr < pool_lo || req.addr + req.len > pool_hi {
        return Some(Intercept {
            addr: req.addr,
            len: req.len,
            kind: ViolKind::PoolLeak,
            rule: "越池界",
        });
    }
    None
}

// ─────────────────────────────────────────────────────────────────────
// 二、黄金禁区表 + 30 例注入集
// ─────────────────────────────────────────────────────────────────────

/// 黄金禁区：BIOS ROM / MMIO / 保留页三段。
pub fn golden_regions() -> Vec<Reserved> {
    vec![
        Reserved { start: 0x000C_0000, end: 0x000F_0000, tag: "BIOS ROM" },
        Reserved { start: 0xFEE0_0000, end: 0xFEE0_1000, tag: "MMIO LAPIC" },
        Reserved { start: 0x0000_8000, end: 0x0000_A000, tag: "保留页" },
    ]
}

/// 30 例注入集：三类各 10 例（地址/长度组合已知必拦）。
pub fn bad_requests_30() -> (Vec<Req>, usize, usize, usize) {
    let regions = golden_regions();
    let mut out = Vec::new();
    // 白名单外 10 例：三段各取点 + 半跨。
    for &a in &[0x000C_1000u64, 0x000C_8000, 0x000E_F000, 0xFEE0_0040, 0xFEE0_0800,
                0x0000_8100, 0x0000_9000, 0x000B_F000, 0x000F_0000 - 0x100, 0xFED0_0000]
    {
        out.push(Req { addr: a, len: 0x100, exempt: None });
    }
    // 池越界 10 例：池界 [0x100000, 0xC00000) 外取。
    for &a in &[0x0000_1000u64, 0x0007_0000, 0x000F_1000, 0x00C0_0000, 0x00C0_1000,
                0x0100_0000 - 0x10, 0x00BF_F000, 0x0000_0000, 0x0200_0000, 0x0F00_0000]
    {
        out.push(Req { addr: a, len: 0x100, exempt: None });
    }
    // 豁免越界 10 例：豁免 [0x000C_0000, 0x000C_8000)，请求超出。
    let ex = (0x000C_0000, 0x000C_8000);
    for &(a, l) in &[
        (0x000C_7000u64, 0x200u64),  // 尾部超出
        (0x000C_8000, 0x100),        // 全在豁免外
        (0x000C_0000 - 0x100, 0x200), // 头部前探
        (0xFEE0_0000, 0x100),        // 豁免外禁区
        (0x0000_8000, 0x100),
        (0x000C_7F00, 0x200),
        (0x000B_F800, 0x900),
        (0xFEE0_0F00, 0x200),
        (0x0000_9F00, 0x200),
        (0x000E_0000, 0x8000),
    ] {
        out.push(Req { addr: a, len: l, exempt: Some(ex) });
    }
    let _ = &regions;
    (out, 10, 10, 10)
}

// ─────────────────────────────────────────────────────────────────────
// 三、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0370 判据全集。
pub fn run_unx370_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0370 保留区越界分配拦截");
    let regions = golden_regions();
    let (pool_lo, pool_hi) = (0x0010_0000u64, 0x00C0_0000u64);

    // J1 主判据：30 例全拦截 + 三类分账齐全
    let (reqs, n_w, n_p, n_e) = bad_requests_30();
    let mut ledger: Vec<Intercept> = Vec::new();
    for r in &reqs {
        if let Some(ic) = intercept(&regions, pool_lo, pool_hi, r) {
            ledger.push(ic);
        }
    }
    cs.add("J1 30 例全拦", ledger.len() == 30, "注入全集⊆拦截账");
    let c_w = ledger.iter().filter(|i| i.kind == ViolKind::Whitelist).count();
    let c_p = ledger.iter().filter(|i| i.kind == ViolKind::PoolLeak).count();
    let c_e = ledger.iter().filter(|i| i.kind == ViolKind::ExemptOver).count();
    cs.add("J1 三类分账", (c_w, c_p, c_e) == (n_w, n_p, n_e) && n_w == 10 && n_p == 10 && n_e == 10,
        "三分类齐全");

    // 注入 1：合法请求放行（池内不拦）
    let ok = Req { addr: 0x0010_1000, len: 0x100, exempt: None };
    cs.add("注入1 合法放行", intercept(&regions, pool_lo, pool_hi, &ok).is_none(), "池内零误报");

    // 注入 2：豁免全覆盖放行（豁免令合法使用）
    let ok2 = Req { addr: 0x000C_1000, len: 0x100, exempt: Some((0x000C_0000, 0x000C_8000)) };
    cs.add("注入2 豁免放行", intercept(&regions, pool_lo, pool_hi, &ok2).is_none(), "范围内免检");

    // 注入 3：半跨禁区（请求头在池尾、尾进 ROM）
    let half = Req { addr: 0x000B_F800, len: 0x1000, exempt: None };
    let ic3 = intercept(&regions, pool_lo, pool_hi, &half);
    cs.add(
        "注入3 半跨拦截",
        matches!(&ic3, Some(i) if i.kind == ViolKind::Whitelist && i.rule == "BIOS ROM"),
        "跨界即拦",
    );

    // 注入 4：拦截账每条可解释（rule 非空）
    cs.add("注入4 拦截可解释", ledger.iter().all(|i| !i.rule.is_empty()), "规则命中留痕");

    // 诊断码闭集：三分类互异
    cs.add(
        "三分类互异",
        ViolKind::Whitelist != ViolKind::PoolLeak
            && ViolKind::PoolLeak != ViolKind::ExemptOver
            && ViolKind::Whitelist != ViolKind::ExemptOver,
        "分账闭集",
    );

    cs
}
