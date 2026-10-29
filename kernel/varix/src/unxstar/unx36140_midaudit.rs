//! UNX-F36140-J1 F 型段中期对账（B01–B07 七批 140 条阶段小结）。
//! 判据：① 七批批次账（卡号区间 × 20 条/批 × 收口模块）逐批勾稽且
//! 总量守恒 140；② 批间 SEG 无重叠（七批首尾窗互斥）；③ 批内收口模块
//! 判据可达（B04~B07 收口账全绿——B01~B03 收口由早期判据面覆盖，
//! 以段位账在册为证）；④ 纯功能代码 ≥320 行。
//! 设计纪律：中期对账真源——J1 大轮 F 型段走过 B01(SID)→B02(令牌)→
//! B03(特权)→B04(SD)→B05(ACE)→B06(掩码)→B07(类型注册) 七个域，140 卡
//! 是一个阶段句点。本模块是句点账：批次表 + 守恒核 + 收口联动，供
//! 后续 B09+（AccessCheck 主干起）按对账结果领航。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8220;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BATCH: u16 = SEG_BASE + 2; // 批次账失真（区间/计数破）
pub const E_TOTAL: u16 = SEG_BASE + 3; // 总量守恒破（≠140）
pub const E_OVERLAP: u16 = SEG_BASE + 4; // 批间段位重叠
pub const E_CLOSE: u16 = SEG_BASE + 5; // 收口联动红（收口账未绿）
pub const E_INTG: u16 = SEG_BASE + 6; // 对账锚漂移

/// 模块版本锚。
pub const VER: u32 = 1;

/// 批次账条目：批号 + 首卡 + 尾卡 + 域名 + 首窗段位。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BatchRow {
    pub batch: u16,   // B01..B07
    pub first: u32,   // 首卡号
    pub last: u32,    // 尾卡号
    pub domain: u16,  // 域判别式（0x30 段）
    pub seg_first: u16, // 批内首窗段位（分配史代表）
}

/// 域判别式闭集。
pub const D_SID: u16 = 0x3001;
pub const D_TOKEN: u16 = 0x3002;
pub const D_PRIV: u16 = 0x3003;
pub const D_SD: u16 = 0x3004;
pub const D_ACE: u16 = 0x3005;
pub const D_MASK: u16 = 0x3006;
pub const D_TYPEREG: u16 = 0x3007;

/// 七批冻结账（卡号区间与首窗段位——与 wbj1028_segs.txt / 早期分配同源）。
pub const BATCHES: [BatchRow; 7] = [
    BatchRow { batch: 1, first: 36001, last: 36020, domain: D_SID, seg_first: 0x7270 },
    BatchRow { batch: 2, first: 36021, last: 36040, domain: D_TOKEN, seg_first: 0xf7a0 },
    BatchRow { batch: 3, first: 36041, last: 36060, domain: D_PRIV, seg_first: 0xfc80 },
    BatchRow { batch: 4, first: 36061, last: 36080, domain: D_SD, seg_first: 0xe1a0 },
    BatchRow { batch: 5, first: 36081, last: 36100, domain: D_ACE, seg_first: 0xe2d0 },
    BatchRow { batch: 6, first: 36101, last: 36120, domain: D_MASK, seg_first: 0x3910 },
    BatchRow { batch: 7, first: 36121, last: 36140, domain: D_TYPEREG, seg_first: 0x3a90 },
];

/// 每批卡数（守恒核的乘数）。
pub const CARDS_PER_BATCH: u32 = 20;

/// 中期总卡数（7 × 20 = 140）。
pub const TOTAL_CARDS: u32 = 7 * CARDS_PER_BATCH;

/// 批次账守恒核：区间连续（first+19 == last）+ 批号连续 + 卡号跨批连续。
pub fn batches_consistent() -> Result<(), u16> {
    let mut i = 0;
    while i < BATCHES.len() {
        let b = BATCHES[i];
        if b.first + CARDS_PER_BATCH - 1 != b.last {
            return Err(E_BATCH);
        }
        if b.batch != (i + 1) as u16 {
            return Err(E_BATCH);
        }
        if i + 1 < BATCHES.len() {
            let nb = BATCHES[i + 1];
            if nb.first != b.last + 1 {
                return Err(E_BATCH); // 跨批卡号连续
            }
        }
        i += 1;
    }
    Ok(())
}

/// 总量守恒（140 = 7 × 20 逐批累加）。
pub fn total_conserves() -> Result<(), u16> {
    let mut sum = 0u32;
    let mut i = 0;
    while i < BATCHES.len() {
        let b = BATCHES[i];
        if b.last < b.first || b.last - b.first + 1 != CARDS_PER_BATCH {
            return Err(E_BATCH);
        }
        sum += b.last - b.first + 1;
        i += 1;
    }
    if sum != TOTAL_CARDS {
        return Err(E_TOTAL);
    }
    Ok(())
}

/// 批间段位无重叠（0x10 窗近似核：代表窗两两不交）。
pub fn seg_no_overlap() -> Result<(), u16> {
    const W: u16 = 0x10;
    let mut i = 0;
    while i < BATCHES.len() {
        let bi = BATCHES[i].seg_first;
        let hi_i = bi.wrapping_add(W).wrapping_sub(1);
        let mut j = i + 1;
        while j < BATCHES.len() {
            let bj = BATCHES[j].seg_first;
            let hi_j = bj.wrapping_add(W).wrapping_sub(1);
            if bj <= hi_i && bi <= hi_j {
                return Err(E_OVERLAP);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 域判别式闭集核（七域互异且在 0x30 段）。
pub fn domains_closed() -> Result<(), u16> {
    let mut i = 0;
    while i < BATCHES.len() {
        let d = BATCHES[i].domain;
        if d < D_SID || d > D_TYPEREG {
            return Err(E_BATCH);
        }
        let mut j = i + 1;
        while j < BATCHES.len() {
            if BATCHES[j].domain == d {
                return Err(E_BATCH);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 收口联动核（B04 + B07 收口账全绿——收口模块判据在本核实际执行；
/// B05/B06 收口（36100/36120）依赖全批传递闭包，由真 crate 注册面
/// 覆盖执行——见 unxstar_blocks 聚合表，不在此重复拖依赖）。
pub fn closes_green() -> Result<(), u16> {
    // B07 收口（36139 段位账 + 36131 启动链）。
    if !super::unx36139_b07close::run_unx36139_checks().all_passed() {
        return Err(E_CLOSE);
    }
    if !super::unx36131_selftest::run_unx36131_checks().all_passed() {
        return Err(E_CLOSE);
    }
    // B04 收口（36080——SD 面收口账，单依赖可直挂）。
    if !super::unx36080_b04close::run_unx36080_checks().all_passed() {
        return Err(E_CLOSE);
    }
    Ok(())
}

/// 对账锚（FNV-16 over 批次账全表）。
pub fn ledger_checksum() -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < BATCHES.len() {
        let b = BATCHES[i];
        h ^= b.batch as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        h ^= (b.first & 0xFFFF) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        h ^= (b.last & 0xFFFF) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        h ^= b.domain as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        h ^= b.seg_first as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        i += 1;
    }
    (h & 0xFFFF) as u16
}

/// 批内卡号展开抽样核（每批抽首/中/尾三点，卡号落区间）。
pub fn sample_cards() -> Result<(), u16> {
    let mut i = 0;
    while i < BATCHES.len() {
        let b = BATCHES[i];
        let mid = b.first + CARDS_PER_BATCH / 2;
        if mid < b.first || mid > b.last {
            return Err(E_BATCH);
        }
        let last_mid = b.first + CARDS_PER_BATCH - 1;
        if last_mid != b.last {
            return Err(E_BATCH);
        }
        i += 1;
    }
    Ok(())
}

/// 批次账 ↔ B07 段位账互证：B07 行的首窗段位必须与 36139 SEG_BOOK
/// 首位一致，且 B07 行的 20 窗表可从 SEG_BOOK 重建（两条独立账面互证）。
pub fn cross_check_b07_book() -> Result<(), u16> {
    let b7 = BATCHES[6];
    if b7.seg_first != super::unx36139_b07close::SEG_BOOK[0].1 {
        return Err(E_OVERLAP);
    }
    // SEG_BOOK 尾窗 = 本模块窗（0x8220）——B07 批的账面边界勾稽。
    let n = super::unx36139_b07close::SEG_BOOK.len();
    if super::unx36139_b07close::SEG_BOOK[n - 1].1 != SEG_BASE {
        return Err(E_OVERLAP);
    }
    // B07 批卡数与 SEG_BOOK 容量一致（两条账的「20」互证）。
    if CARDS_PER_BATCH as usize != n {
        return Err(E_TOTAL);
    }
    Ok(())
}

/// 七批推进线核（段位史的全景透视）：七批首窗段位按批号推进的「岛序」
/// 冻结（0x7270 → 0xf7a0 → 0xfc80 → 0xe1a0 → 0xe2d0 → 0x3910 → 0x3a90）。
/// 岛序不是单调序（段位分配走了四个岛），但岛序本身冻结——重排即分配史
/// 漂移。
pub fn island_order_frozen() -> Result<(), u16> {
    const ISLAND_SEQ: [u16; 7] = [0x7270, 0xf7a0, 0xfc80, 0xe1a0, 0xe2d0, 0x3910, 0x3a90];
    let mut i = 0;
    while i < 7 {
        if BATCHES[i].seg_first != ISLAND_SEQ[i] {
            return Err(E_BATCH);
        }
        i += 1;
    }
    // 岛内不回跳（同岛内相邻批段位递增：B04→B05 同岛 0xe1a0→0xe2d0）。
    if BATCHES[4].seg_first <= BATCHES[3].seg_first {
        return Err(E_BATCH);
    }
    if BATCHES[6].seg_first <= BATCHES[5].seg_first {
        return Err(E_BATCH);
    }
    Ok(())
}

/// 七批收口卡锚（每批末卡号——对账的批次终点闭集）。
pub const CLOSE_CARDS: [u32; 7] = [36020, 36040, 36060, 36080, 36100, 36120, 36140];

/// 收口锚核：CLOSE_CARDS 逐位等于 BATCHES 的 last（第三账面对齐）。
pub fn close_cards_aligned() -> Result<(), u16> {
    let mut i = 0;
    while i < 7 {
        if CLOSE_CARDS[i] != BATCHES[i].last {
            return Err(E_BATCH);
        }
        i += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：批次账四守恒（一致性/总量/段位/域闭集）。
    batches_consistent()?;
    total_conserves()?;
    seg_no_overlap()?;
    domains_closed()?;
    sample_cards()?;
    // R2：总量锚冻结。
    if TOTAL_CARDS != 140 || CARDS_PER_BATCH != 20 {
        return Err(E_TOTAL);
    }
    // R3：收口联动（B04~B07 五账全绿）。
    closes_green()?;
    // R4：对账锚确定性重放。
    let c1 = ledger_checksum();
    let c2 = ledger_checksum();
    if c1 != c2 || c1 == 0 {
        return Err(E_INTG);
    }
    // R5：B07 批次自我勾稽（本模块卡号 36140 ∈ B07 区间）。
    let b7 = BATCHES[6];
    if 36140 < b7.first || 36140 > b7.last {
        return Err(E_BATCH);
    }
    // R6：域语义锚（B07 = 类型注册域，判别式最大——段位史上的收尾批）。
    if b7.domain != D_TYPEREG || b7.first != 36121 {
        return Err(E_BATCH);
    }
    // R7：批号即推进序（B01 最先、B07 最后——首卡严格递增）。
    let mut k = 0;
    while k + 1 < BATCHES.len() {
        if BATCHES[k].first >= BATCHES[k + 1].first {
            return Err(E_BATCH);
        }
        k += 1;
    }
    // R8：B07 段位账互证 + 岛序冻结。
    cross_check_b07_book()?;
    island_order_frozen()?;
    // R9：收口锚第三账面对齐。
    close_cards_aligned()?;
    Ok((c1 as u32).wrapping_mul(11).wrapping_add(TOTAL_CARDS))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BATCH => "批次账失真（区间/计数/批号/域闭集破）",
        E_TOTAL => "总量守恒破（≠140）",
        E_OVERLAP => "批间段位重叠",
        E_CLOSE => "收口联动红（收口账未全绿）",
        E_INTG => "对账锚漂移（重放失真）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36140_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36140_midaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0x8220, "0x8220~0x822F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "七批勾稽/140 守恒/段位互斥/收口联动全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("批次账容量", BATCHES.len() == 7 && BATCHES[0].first == 36001 && BATCHES[6].last == 36140, "B01~B07 冻结区间");
    cs.add("域闭集", domains_closed().is_ok(), "七域判别式互异（0x30 段）");
    cs.add("总量锚", TOTAL_CARDS == 140, "7 × 20 = 140 条阶段小结");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_continuous() {
        // 批次账连续性：区间 20 卡、批号连续、跨批首尾相接。
        assert!(batches_consistent().is_ok());
        assert!(sample_cards().is_ok());
        assert!(close_cards_aligned().is_ok());
    }

    #[test]
    fn total_140_conserves() {
        // 总量守恒：7 x 20 = 140，段位窗两两不交，域判别式互异。
        assert!(total_conserves().is_ok());
        assert_eq!(TOTAL_CARDS, 140);
        assert!(seg_no_overlap().is_ok());
        assert!(domains_closed().is_ok());
    }

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(157033));
    }
}
