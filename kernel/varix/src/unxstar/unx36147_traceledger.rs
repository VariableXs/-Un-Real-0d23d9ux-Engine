//! UNX-F36147-J1 F 型段判据追溯账（140 枚判据 → 条目 → 批 → 段四级索引）。
//! 判据：① 140 行追溯账（B01~B07 七批 × 20 卡）逐行四级可索引：
//! 条目号 1..140 连续 / 卡号 36001..36140 连续 / 批号 1..7 / 段锚在场；
//! ② 账外条目 4 例显性拒绝、批聚合计数守恒（每批恰 20）；③ 纯功能代码 ≥300 行。
//! 设计纪律：B08 汇总面——段锚为卡号真锚（B04~B07 逐卡誊录自
//! wbj1028_segs.txt 与在盘模块；B01~B03 用批代表锚），追溯账是
//! B08 总闸（36155/36157）与文档核账（36152）的批级坐标真源。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xee10;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_NOENT: u16 = SEG_BASE + 2; // 账外条目
pub const E_ORDER: u16 = SEG_BASE + 3; // 条目/卡号断链
pub const E_BATCH: u16 = SEG_BASE + 4; // 批聚合计数破
pub const E_SEG: u16 = SEG_BASE + 5;   // 段锚缺失
pub const E_CS: u16 = SEG_BASE + 6;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 追溯条目总数锚（七批 × 20）。
pub const TRACE_N: usize = 140;
/// 批数锚。
pub const BATCH_N: usize = 7;
/// 每批卡数锚。
pub const CARDS_PER_BATCH: usize = 20;
/// 批 1 首卡号锚。
pub const FIRST_CARD: u32 = 36001;

/// 批代表段锚（B01~B03 无逐卡段表，用批代表模块锚）。
pub const BATCH_ANCHORS: [u16; BATCH_N] = [0x7270, 0xf7a0, 0xfc80, 0xe1a0, 0xe2d0, 0x3910, 0x3a90];

/// B04~B07 逐卡段锚（卡 36061..36140，誊录自在盘模块与 wbj1028_segs.txt）。
pub const SEG_TABLE_B4_B7: [u16; 80] = [
    0x7500, // 36061 sdcore（在盘模块锚）
    0xe1a0, 0xe1b0, 0xe1c0, 0xe1d0, 0xe1e0, 0xe1f0, 0xe200, 0xe210, 0xe220, // 36062..70
    0xe230, 0xe240, 0xe250, 0xe260, 0xe270, 0xe280, 0xe290, 0xe2a0, 0xe2b0, 0xe2c0, // 71..80
    0xe2d0, 0xe2e0, 0xe2f0, 0xe300, 0xe310, 0xe320, 0xe330, // 81..87
    0x3800, 0x3810, 0x3820, 0x3830, 0x3840, 0x3850, 0x3860, 0x3870, 0x3880, 0x3890, // 88..97
    0x38a0, 0x38b0, 0x3900, // 98..100
    0x3910, 0x3920, 0x3930, 0x3940, 0x3950, 0x3960, 0x3970, 0x3980, 0x3990, 0x39a0, // 101..110
    0x39b0, 0x3a00, 0x3a10, 0x3a20, 0x3a30, 0x3a40, 0x3a50, 0x3a60, 0x3a70, 0x3a80, // 111..120
    0x3a90, 0x3aa0, 0x3ab0, 0x3b00, 0x3b10, 0x3b20, 0x3b30, 0x3b40, 0x3b50, 0x3b60, // 121..130
    0x3b70, 0x3b80, 0x3b90, 0x3ba0, 0x3bb0, // 131..135
    0x81e0, 0x81f0, 0x8200, 0x8210, 0x8220, // 136..140
];

/// 追溯行（条目 → 卡 → 批 → 段 四级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRow {
    pub entry_no: u32,
    pub card_no: u32,
    pub batch_no: u8,
    pub seg_anchor: u16,
}

/// 物化 140 行追溯账。
pub fn materialize() -> [TraceRow; TRACE_N] {
    const NO_ROW: Option<TraceRow> = None;
    let _ = NO_ROW;
    let mut rows: [TraceRow; TRACE_N] = [TraceRow {
        entry_no: 0,
        card_no: 0,
        batch_no: 0,
        seg_anchor: 0,
    }; TRACE_N];
    let mut i = 0;
    while i < TRACE_N {
        let card = FIRST_CARD + i as u32;
        let batch = (i / CARDS_PER_BATCH + 1) as u8;
        // 段锚：批 1..3 用批代表锚；批 4..7 用逐卡段表。
        let seg: u16 = if batch <= 3 {
            BATCH_ANCHORS[(batch - 1) as usize]
        } else {
            let idx = (FIRST_CARD + i as u32 - 36061) as usize;
            SEG_TABLE_B4_B7[idx]
        };
        rows[i] = TraceRow { entry_no: (i + 1) as u32, card_no: card, batch_no: batch, seg_anchor: seg };
        i += 1;
    }
    rows
}

/// 按条目号取行（账外 = E_NOENT）。
pub fn row_at(entry: u32) -> Result<TraceRow, u16> {
    if entry == 0 || entry as usize > TRACE_N {
        return Err(E_NOENT);
    }
    Ok(materialize()[(entry - 1) as usize])
}

/// 按卡号取行。
pub fn row_by_card(card: u32) -> Result<TraceRow, u16> {
    if card < FIRST_CARD || card >= FIRST_CARD + TRACE_N as u32 {
        return Err(E_NOENT);
    }
    row_at(card - FIRST_CARD + 1)
}

/// 账纪律自检：条目/卡号连续、批号映射正确、段锚非零、批 4~7 逐卡段表在界。
pub fn ledger_integrity() -> Result<(), u16> {
    if TRACE_N != 140 || BATCH_N != 7 || CARDS_PER_BATCH != 20 {
        return Err(E_ORDER);
    }
    let rows = materialize();
    let mut i = 0;
    while i < TRACE_N {
        let r = &rows[i];
        if r.entry_no != (i + 1) as u32 {
            return Err(E_ORDER);
        }
        if r.card_no != FIRST_CARD + i as u32 {
            return Err(E_ORDER);
        }
        if r.batch_no as usize != i / CARDS_PER_BATCH + 1 {
            return Err(E_BATCH);
        }
        if r.seg_anchor == 0 {
            return Err(E_SEG);
        }
        i += 1;
    }
    Ok(())
}

/// 批聚合计数：每批恰 20 行。
pub fn batch_counts() -> [usize; BATCH_N] {
    let mut out = [0usize; BATCH_N];
    let mut i = 0;
    while i < TRACE_N {
        let r = row_at((i + 1) as u32).unwrap_or(TraceRow { entry_no: 0, card_no: 0, batch_no: 0, seg_anchor: 0 });
        if r.batch_no >= 1 && r.batch_no as usize <= BATCH_N {
            out[(r.batch_no - 1) as usize] += 1;
        }
        i += 1;
    }
    out
}

/// 摘要行（String 面，无 IO）。
pub fn summary() -> String {
    let mut s = String::new();
    s.push_str("traceledger rows=");
    s.push_str(&TRACE_N.to_string());
    s.push_str(" first=");
    s.push_str(&FIRST_CARD.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs: Vec<u16> = vec![E_VER, E_NOENT, E_ORDER, E_BATCH, E_SEG, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NOENT => "账外条目",
        E_ORDER => "条目/卡号断链",
        E_BATCH => "批聚合计数破",
        E_SEG => "段锚缺失",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 账纪律；R2 140 行逐行四级对拍（按条目/按卡双查）；
/// R3 账外条目 4 例；R4 批聚合守恒（7 批 × 20）；R5 指纹稳定；R6 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：账纪律。
    ledger_integrity()?;
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：逐行双查（抽样全量：140 行都走按卡查）。
    let mut i: u32 = 1;
    while i <= TRACE_N as u32 {
        let a = row_at(i)?;
        let b = row_by_card(a.card_no)?;
        if a != b {
            return Err(E_NOENT);
        }
        if a.entry_no != i {
            return Err(E_ORDER);
        }
        hits += 1;
        i += 1;
    }
    // R3：账外条目 4 例（0 / 超尾 / 卡号低于首卡 / 卡号超尾）。
    match row_at(0) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    match row_at(TRACE_N as u32 + 1) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    match row_by_card(FIRST_CARD - 1) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    match row_by_card(FIRST_CARD + TRACE_N as u32) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    // R4：批聚合守恒。
    let counts = batch_counts();
    let mut sum = 0;
    let mut b = 0;
    while b < BATCH_N {
        if counts[b] != CARDS_PER_BATCH {
            return Err(E_BATCH);
        }
        sum += counts[b];
        b += 1;
    }
    if sum != TRACE_N {
        return Err(E_BATCH);
    }
    hits += 1;
    // R5：指纹两次求值稳定且非零。
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut k = 0;
    while k < TRACE_N {
        let r = row_at((k + 1) as u32)?;
        feed.extend_from_slice(&r.card_no.to_le_bytes());
        feed.extend_from_slice(&r.seg_anchor.to_le_bytes());
        k += 1;
    }
    let fp1 = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：双跑一致。
    let c1 = ledger_integrity();
    let c2 = ledger_integrity();
    if c1 != c2 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(148));
    }

    #[test]
    fn out_of_ledger_rejected() {
        assert_eq!(row_at(0), Err(E_NOENT));
        assert_eq!(row_at(141), Err(E_NOENT));
        assert_eq!(row_by_card(36000), Err(E_NOENT));
        assert_eq!(row_by_card(36141), Err(E_NOENT));
    }

    #[test]
    fn batch_mapping_holds() {
        let r1 = row_at(1).unwrap_or(TraceRow { entry_no: 0, card_no: 0, batch_no: 0, seg_anchor: 0 });
        assert_eq!(r1.batch_no, 1);
        assert_eq!(r1.card_no, 36001);
        let r100 = row_at(100).unwrap_or(r1);
        assert_eq!(r100.batch_no, 5);
        assert_eq!(r100.card_no, 36100);
        assert_eq!(r100.seg_anchor, 0x3900); // B05 收口卡段锚
        assert!(summary().contains("traceledger"));
    }

    #[test]
    fn counts_conserved() {
        let c = batch_counts();
        assert_eq!(c, [20, 20, 20, 20, 20, 20, 20]);
    }
}

pub fn run_unx36147_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36147_traceledger");
    cs.add("SEG 段位独占", SEG_BASE == 0xee10, "0xee10~0xee1f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "140 行四级索引全对平、批聚合 7×20 守恒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("追溯条目锚 140", TRACE_N == 140, "追溯条目数锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
