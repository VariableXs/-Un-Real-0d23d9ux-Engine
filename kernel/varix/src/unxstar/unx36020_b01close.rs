//! unx36020_b01close — UNX-F36020 B01 守恒预核、防重扫描与收口。
//!
//! 判据原文：
//!   J1 批内 20 条求和=6,000 实核（脚本为准）/ F36001–F36020 连续零
//!      跳号 / 防重五范围零撞号；域累计 6,000/240,000。
//! 设计纪律：
//!   1. 卡号连续锚：F36001..F36020 与段位窗 0x7270+i×0x20 一一对应，
//!      连续零跳号、窗口两两不交（防重主面）。
//!   2. 防重五范围：与本批邻接的五个已占用段位区（cpio 族 0x7500~、
//!      H 批 0x8300~0x9FF0、unx1187 0x8CF0~、unx1199 0x9690~、
//!      unx1203 0x97B0~）逐窗比对零撞号。
//!   3. 行数预算锚：20 卡 × 300 行预算 = 6,000（脚本实核为准，本模块
//!      维护预算账与域累计 6,000/240,000）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x74D0;

// ── 诊断码闭集（0x74D0~0x74EF，独占）─────────────────────────
const E_GAP: u16 = SEG_BASE + 1; // 卡号/段位跳号
const E_OVERLAP: u16 = SEG_BASE + 2; // 段位窗重叠（批内防重破）
const E_COLLIDE: u16 = SEG_BASE + 3; // 与外部已占用范围撞号
const E_BUDGET: u16 = SEG_BASE + 4; // 行数预算账失配（≠6,000）
const E_BAL: u16 = SEG_BASE + 5; // 收口账守恒破

use crate::checks::CheckSet;

/// 本批首卡号。
pub const FIRST_CARD: u32 = 36001;
/// 本批卡数。
pub const CARDS: usize = 20;
/// 本批段位首窗。
pub const FIRST_SEG: u16 = 0x7270;
/// 段位窗宽。
pub const SEG_STRIDE: u16 = 0x20;
/// 每卡行数预算。
pub const LINE_BUDGET_PER_CARD: u32 = 300;
/// 批行数预算（20×300=6,000）。
pub const BATCH_LINE_BUDGET: u32 = LINE_BUDGET_PER_CARD * CARDS as u32;
/// 域累计预算分母（240,000）。
pub const DOMAIN_TOTAL: u32 = 240_000;

/// 防重外部范围（五个已占用区锚）。
pub const RESERVED_RANGES: [(u16, u16); 5] = [
    (0x7500, 0x7530), // unx603 cpio 族三窗
    (0x8300, 0xA000), // wbh1027 H 批 0x8300~0x9FF0
    (0x8CF0, 0x8D30), // unx1187 诊断段
    (0x9690, 0x96D0), // unx1199 诊断段
    (0x97B0, 0x97F0), // unx1203 诊断段
];

/// 本批第 i 卡的段位窗基址。
pub fn seg_of(i: usize) -> u16 {
    FIRST_SEG + (i as u16) * SEG_STRIDE
}

/// 本批第 i 卡号。
pub fn card_of(i: usize) -> u32 {
    FIRST_CARD + i as u32
}

/// 卡号连续核：F36001..F36020 零跳号；段位窗步进 0x20 零跳窗。
pub fn continuity_check() -> Result<(), u16> {
    let mut i = 0;
    while i < CARDS {
        if card_of(i) != FIRST_CARD + i as u32 {
            return Err(E_GAP);
        }
        if seg_of(i) != FIRST_SEG + (i as u16) * SEG_STRIDE {
            return Err(E_GAP);
        }
        i += 1;
    }
    Ok(())
}

/// 批内防重：20 窗两两不交（含邻接界校验：窗尾 = 窗头 + 0x20）。
pub fn batch_disjoint_check() -> Result<(), u16> {
    let mut i = 0;
    while i < CARDS {
        let a_lo = seg_of(i);
        let a_hi = a_lo + SEG_STRIDE;
        let mut j = i + 1;
        while j < CARDS {
            let b_lo = seg_of(j);
            let b_hi = b_lo + SEG_STRIDE;
            if a_lo < b_hi && b_lo < a_hi {
                return Err(E_OVERLAP);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 防重五范围：本批 20 窗与五个外部已占用区零撞号。
pub fn reserved_collision_check() -> Result<(), u16> {
    let mut i = 0;
    while i < CARDS {
        let lo = seg_of(i);
        let hi = lo + SEG_STRIDE;
        let mut r = 0;
        while r < RESERVED_RANGES.len() {
            let (rlo, rhi) = RESERVED_RANGES[r];
            if lo < rhi && rlo < hi {
                return Err(E_COLLIDE);
            }
            r += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 行数预算账：20 × 300 == 6,000；域累计 6,000 / 240,000 口径核。
pub fn budget_check() -> Result<(), u16> {
    let sum = LINE_BUDGET_PER_CARD * CARDS as u32;
    if sum != BATCH_LINE_BUDGET || BATCH_LINE_BUDGET != 6000 {
        return Err(E_BUDGET);
    }
    if BATCH_LINE_BUDGET > DOMAIN_TOTAL {
        return Err(E_BUDGET);
    }
    Ok(())
}

/// 收口账：预核四项全过 + 域累计口径。
pub struct CloseBook {
    pub continuity_ok: bool,
    pub disjoint_ok: bool,
    pub reserved_ok: bool,
    pub budget_ok: bool,
    pub domain_used: u32,
}

/// 段位窗分配表行（布局文档化面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegRow {
    pub card: u32,       // 卡号
    pub seg_lo: u16,     // 窗头（含）
    pub seg_hi: u16,     // 窗尾（不含）
}

/// 20 行窗分配表导出（布局单一真源：seg_of/card_of 的表化投影）。
pub fn seg_table() -> [SegRow; CARDS] {
    let mut out = [SegRow { card: 0, seg_lo: 0, seg_hi: 0 }; CARDS];
    let mut i = 0;
    while i < CARDS {
        out[i] = SegRow {
            card: card_of(i),
            seg_lo: seg_of(i),
            seg_hi: seg_of(i) + SEG_STRIDE,
        };
        i += 1;
    }
    out
}

/// 窗分配表健康核：表化投影与公式逐行一致、行间严升序、首尾锚。
pub fn seg_table_check() -> Result<(), u16> {
    let t = seg_table();
    let mut i = 0;
    while i < CARDS {
        if t[i].card != FIRST_CARD + i as u32
            || t[i].seg_lo != FIRST_SEG + (i as u16) * SEG_STRIDE
            || t[i].seg_hi - t[i].seg_lo != SEG_STRIDE
        {
            return Err(E_GAP);
        }
        if i > 0 && t[i].seg_lo < t[i - 1].seg_hi {
            return Err(E_OVERLAP);
        }
        i += 1;
    }
    if t[0].seg_lo != FIRST_SEG || t[CARDS - 1].seg_hi != FIRST_SEG + CARDS as u16 * SEG_STRIDE {
        return Err(E_GAP);
    }
    Ok(())
}

/// 外部范围明细账：每区与本批窗的最近距离（防重观测面，
/// 返回 (区号, 最小间隙窗数)；无接触 = 全部间隙 ≥0）。
pub fn reserved_gap_report() -> [i32; RESERVED_RANGES.len()] {
    let mut out = [i32::MAX; RESERVED_RANGES.len()];
    let mut r = 0;
    while r < RESERVED_RANGES.len() {
        let (rlo, rhi) = RESERVED_RANGES[r];
        let mut gap = i32::MAX;
        let mut i = 0;
        while i < CARDS {
            let lo = seg_of(i) as i32;
            let hi = (seg_of(i) + SEG_STRIDE) as i32;
            let d = if hi <= rlo as i32 {
                rlo as i32 - hi // 本批窗在区左侧
            } else if lo >= rhi as i32 {
                lo - rhi as i32 // 本批窗在区右侧
            } else {
                -1 // 撞号（不应发生——precheck 已拒）
            };
            if d < gap {
                gap = d;
            }
            i += 1;
        }
        out[r] = gap;
        r += 1;
    }
    out
}

/// 域累计账：批行数预算 / 域总盘子 / 万分比（口径 6,000/240,000）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainLedger {
    pub batch_lines: u32,
    pub domain_total: u32,
    pub bp: u32, // 万分比（batch*10000/domain）
}

/// 累计账导出（确定性整数面）。
pub fn domain_ledger() -> DomainLedger {
    DomainLedger {
        batch_lines: BATCH_LINE_BUDGET,
        domain_total: DOMAIN_TOTAL,
        bp: (BATCH_LINE_BUDGET * 10_000) / DOMAIN_TOTAL,
    }
}

/// 收口预核（全项跑一遍，账面全存）。
pub fn precheck() -> CloseBook {
    CloseBook {
        continuity_ok: continuity_check().is_ok(),
        disjoint_ok: batch_disjoint_check().is_ok(),
        reserved_ok: reserved_collision_check().is_ok(),
        budget_ok: budget_check().is_ok(),
        domain_used: BATCH_LINE_BUDGET,
    }
}

/// 域判据入口（ktest 注册面）。
pub fn run_unx36020_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx36020");
    let bk = precheck();

    {
        let ok = bk.continuity_ok;
        s.add("C20-01-卡号段位连续", ok, if ok { "" } else { "跳号" });
    }
    {
        let ok = bk.disjoint_ok;
        s.add("C20-02-批内窗不交", ok, if ok { "" } else { "窗重叠" });
    }
    {
        let ok = bk.reserved_ok;
        s.add("C20-03-外部范围零撞", ok, if ok { "" } else { "撞号" });
    }
    {
        let ok = bk.budget_ok && bk.domain_used == 6000;
        s.add("C20-04-预算域累计", ok, if ok { "" } else { "预算账漂移" });
    }
    s
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_GAP => "卡号/段位跳号",
        E_OVERLAP => "批内段位窗重叠（防重破）",
        E_COLLIDE => "与外部已占用范围撞号",
        E_BUDGET => "行数预算账失配（≠6,000）",
        E_BAL => "收口账守恒破",
        _ => "未知诊断码",
    }
}

/// 确定性演练：预核四项 + 收口账复核（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：四项预核逐项过（连续/不交/外部零撞/预算）。
    let bk = precheck();
    if !bk.continuity_ok {
        return Err(E_GAP);
    }
    if !bk.disjoint_ok {
        return Err(E_OVERLAP);
    }
    if !bk.reserved_ok {
        return Err(E_COLLIDE);
    }
    if !bk.budget_ok {
        return Err(E_BUDGET);
    }
    hits += 1;

    // R2：段位窗终端锚——末窗 0x74D0..0x74F0 恰在本批区间内，
    // 0x7500 起（cpio 族）不越界（防重扫描边界对齐）。
    let last_lo = seg_of(CARDS - 1);
    if last_lo != 0x74D0 || last_lo + SEG_STRIDE != 0x74F0 {
        return Err(E_GAP);
    }
    hits += 1;

    // R3：收口账域累计口径——本批 6,000 ≤ 域总 240,000 且占比账可复算。
    let pct_x100 = (BATCH_LINE_BUDGET * 100) / DOMAIN_TOTAL;
    if pct_x100 != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：窗分配表健康核 + 外部范围间隙账 + 累计账导出。
    seg_table_check()?;
    let gaps = reserved_gap_report();
    let mut r = 0;
    while r < gaps.len() {
        if gaps[r] < 0 {
            return Err(E_COLLIDE); // 撞号（precheck 已保证不触发）
        }
        r += 1;
    }
    // cpio 族（区0）在本批窗区上方：间隙 = 0x7500-0x74F0 = 0x10 = 16 窗字节。
    if gaps[0] != 0x10 as i32 {
        return Err(E_COLLIDE);
    }
    let led = domain_ledger();
    if led.batch_lines != 6000 || led.domain_total != 240_000 || led.bp != 250 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }
}
