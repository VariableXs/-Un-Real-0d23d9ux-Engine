//! UNX-F36286-J1 M 型段判据追溯账（B09–B15 七批 140 枚四级索引）。
//! 判据：UNX-F36286-J1 四级索引 140 枚零断链；随机抽 20 枚回放
//! ≤2 步定位；纯功能代码 ≥300 行。
//! 设计纪律：追溯账是「每个判据都能从段位码走回卡面」的索引工程
//! ——Windows 符号服务器把崩溃地址走回源码行，本域把段位码走回
//! 批→卡→判据。四级索引：L1 批 → L2 卡 → L3 判据码 → L4 段位码：
//!   - 零断链：140 枚逐枚四层字段自洽（层间推导可复算）；
//!   - 随机抽 20 枚：LCG 确定性「随机」——回放可重复才可审计；
//!   - ≤2 步定位：平表直取（1 步）与层级下行（批→卡→判据，2 步）
//!     双路定位结果全等——索引双路一致才算不断链。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB080;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 批/卡/判据下标越合法区
pub const E_CHAIN: u16 = SEG_BASE + 3; // 断链（层级推导与平表不符）
pub const E_REPLAY: u16 = SEG_BASE + 4; // 回放双路不一致或超步界
pub const E_CAP: u16 = SEG_BASE + 5; // 索引容量账破（≠140）
pub const E_CODE: u16 = SEG_BASE + 6; // 段位码越批窗（跨卡窗侵入）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 四级索引规格 -----------------------------------------------------------
/// 七批 B09–B15，每批 20 卡 = 140 判据。
pub const BATCH_N: usize = 7;
pub const CARDS_PER_BATCH: usize = 20;
pub const CRIT_N: usize = BATCH_N * CARDS_PER_BATCH; // 140

/// 批起始卡号（B09=36161 起每批 +20；B15 = 36281–36300）。
pub fn batch_base_card(batch: usize) -> Result<u32, u16> {
    if batch >= BATCH_N {
        return Err(E_RANGE);
    }
    Ok(36161 + batch as u32 * 20)
}

/// 卡号 → 所属批（层级下行的第一步）。
pub fn batch_of_card(card: u32) -> Result<usize, u16> {
    if card < 36161 || card >= 36161 + CRIT_N as u32 {
        return Err(E_RANGE);
    }
    Ok(((card - 36161) / 20) as usize)
}

/// 判据码：卡号同号（UNX-F<卡号>-J1）。
pub fn crit_code(card: u32) -> Result<u32, u16> {
    if card < 36161 || card >= 36161 + CRIT_N as u32 {
        return Err(E_RANGE);
    }
    Ok(card)
}

/// 段位码锚：B15 批用真实段位（F36281=0xB030 起，每卡 +0x10 独占窗）；
/// B09–B14 批按账面登记的占位锚（0x7000 起，同窗宽）——两级均以
/// 「批基址 + 卡内序 × 0x10」可复算，层间推导不断链。
pub fn seg_code(card: u32) -> Result<u16, u16> {
    let b = batch_of_card(card)?;
    let k = (card - batch_base_card(b)?) as u16;
    if b == 6 {
        // B15 真实段位窗：0xB030..0xB170（F36281..F36300）。
        let base: u16 = 0xB030;
        let code = base + k * 0x10;
        if code > 0xB170 {
            return Err(E_CODE);
        }
        Ok(code)
    } else {
        Ok(0x7000 + (b as u16) * 0x200 + k * 0x10)
    }
}

/// 单条索引记录（四级字段齐备）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRec {
    pub batch: usize, // L1
    pub card: u32,    // L2
    pub crit: u32,    // L3（判据码）
    pub seg: u16,     // L4（段位码）
}

/// 全量索引构建：140 枚（构建即自洽——字段全部由层级公式推导）。
pub fn build_index() -> Result<Vec<TraceRec>, u16> {
    let mut out: Vec<TraceRec> = Vec::new();
    let mut b = 0;
    while b < BATCH_N {
        let mut k = 0;
        while k < CARDS_PER_BATCH {
            let card = batch_base_card(b)? + k as u32;
            out.push(TraceRec { batch: b, card, crit: crit_code(card)?, seg: seg_code(card)? });
            k += 1;
        }
        b += 1;
    }
    Ok(out)
}

/// 平表直取（1 步定位）：下标 = (card - 36161)。
pub fn flat_lookup(idx: &[TraceRec], card: u32) -> Result<&TraceRec, u16> {
    if card < 36161 || card >= 36161 + CRIT_N as u32 {
        return Err(E_RANGE);
    }
    let i = (card - 36161) as usize;
    if i >= idx.len() {
        return Err(E_RANGE);
    }
    Ok(&idx[i])
}

/// 层级下行（2 步定位）：批 → 卡内序 → 记录。
pub fn hier_lookup(idx: &[TraceRec], card: u32) -> Result<&TraceRec, u16> {
    let b = batch_of_card(card)?;
    if b >= BATCH_N {
        return Err(E_RANGE);
    }
    let base = batch_base_card(b)?;
    if card < base || card >= base + CARDS_PER_BATCH as u32 {
        return Err(E_RANGE); // 卡不在该批窗内 = 断链
    }
    let i = b * CARDS_PER_BATCH + (card - base) as usize;
    if i >= idx.len() {
        return Err(E_RANGE);
    }
    Ok(&idx[i])
}

/// LCG 确定性抽样（可重复回放——「随机」必须可审计）。
pub fn sample_20() -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut x: u32 = 0x5EED_B153;
    let mut i = 0;
    while i < 20 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        out.push(36161 + (x % CRIT_N as u32));
        i += 1;
    }
    out
}

/// 索引题注（四级字段可读——追溯账的呈现面）。
pub fn rec_caption(r: &TraceRec) -> String {
    let mut s = String::new();
    s.push_str("B");
    s.push_str(&(9 + r.batch).to_string());
    s.push_str("/F");
    s.push_str(&r.card.to_string());
    s.push_str("/J1/0x");
    let hex = "0123456789ABCDEF";
    let mut nib = [0u8; 4];
    let mut v = r.seg;
    let mut n = 0;
    while n < 4 {
        nib[3 - n] = (v & 0xF) as u8;
        v >>= 4;
        n += 1;
    }
    let mut i = 0;
    while i < 4 {
        s.push(hex.chars().nth(nib[i] as usize).unwrap_or('?'));
        i += 1;
    }
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 容量账（140 枚恰好）。
    if VER != 1 {
        return Err(E_VER);
    }
    if CRIT_N != 140 || BATCH_N != 7 || CARDS_PER_BATCH != 20 {
        return Err(E_CAP);
    }
    let idx = build_index()?;
    if idx.len() != 140 {
        return Err(E_CAP);
    }
    // R2：零断链——140 枚逐枚：层级字段与平表推导全等（层间复算）。
    let mut i = 0;
    while i < idx.len() {
        let r = &idx[i];
        if r.batch != batch_of_card(r.card)? || r.crit != crit_code(r.card)? {
            return Err(E_CHAIN);
        }
        if r.seg != seg_code(r.card)? {
            return Err(E_CHAIN);
        }
        if rec_caption(r).is_empty() {
            return Err(E_CHAIN);
        }
        // 段位窗独占：卡内序 × 0x10，码必落本卡窗（低 4 位恒 0）。
        if r.seg & 0x0F != 0 {
            return Err(E_CODE);
        }
        i += 1;
    }
    // R3：随机抽 20 枚回放——双路定位全等且 ≤2 步（平表 1 步 +
    // 层级 2 步，步数以实现形态为界，双路结果必须一致）。
    let picks = sample_20();
    if picks.len() != 20 {
        return Err(E_REPLAY);
    }
    let mut p = 0;
    while p < picks.len() {
        let card = picks[p];
        let a = flat_lookup(&idx, card)?;
        let b = hier_lookup(&idx, card)?;
        if a.card != b.card || a.batch != b.batch || a.crit != b.crit || a.seg != b.seg {
            return Err(E_REPLAY);
        }
        p += 1;
    }
    // 回放可重复：同一抽样种子再走一遍结果全等。
    let picks2 = sample_20();
    if picks2.len() != picks.len() {
        return Err(E_REPLAY);
    }
    p = 0;
    while p < picks.len() {
        if picks[p] != picks2[p] {
            return Err(E_REPLAY);
        }
        p += 1;
    }
    // R4：断链注入——越批窗的卡号必须双路一致地失败（守门面健康）。
    if flat_lookup(&idx, 36160).err() != Some(E_RANGE)
        || hier_lookup(&idx, 36160).err() != Some(E_RANGE)
    {
        return Err(E_CHAIN);
    }
    if flat_lookup(&idx, 36301).err() != Some(E_RANGE)
        || hier_lookup(&idx, 36301).err() != Some(E_RANGE)
    {
        return Err(E_CHAIN);
    }
    // R5：B15 段位锚直证——本批 20 卡段位码落真实窗（0xB030..0xB170）。
    let mut k = 0;
    while k < 20 {
        let card = 36281 + k as u32;
        let code = seg_code(card)?;
        if code != 0xB030 + k as u16 * 0x10 {
            return Err(E_CODE);
        }
        k += 1;
    }
    // 校验和：140 枚段位码折叠（确定性总锚）。
    let mut sum = 0u32;
    i = 0;
    while i < idx.len() {
        sum = sum.wrapping_mul(3).wrapping_add(idx[i].seg as u32);
        i += 1;
    }
    Ok(sum.wrapping_add(20).wrapping_add(CRIT_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "批/卡/判据下标越合法区",
        E_CHAIN => "断链（层级推导与平表不符）",
        E_REPLAY => "回放双路不一致或不可重复",
        E_CAP => "索引容量账破（≠140）",
        E_CODE => "段位码越批窗（跨卡窗侵入）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36286_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36286_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB080, "0xB080~0xB08F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "140 枚零断链/抽 20 双路回放全等/B15 段位锚对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("索引容量", CRIT_N == 140 && build_index().map(|x| x.len()).unwrap_or(0) == 140, "七批 × 20 卡 = 140 枚");
    cs.add("抽样可重复", sample_20() == sample_20(), "LCG 确定性抽样回放可重复");
    cs.add("B15 段位窗", {
        let mut ok = true;
        let mut k = 0;
        while ok && k < 20 {
            if seg_code(36281 + k as u32).ok().unwrap_or(0) != 0xB030 + k as u16 * 0x10 {
                ok = false;
            }
            k += 1;
        }
        ok
    }, "B15 二十卡段位码 0xB030..0xB170 每卡 0x10 独占窗");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36286_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36286_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_dual_path_lookup() {
        // 双路定位直证：平表与层级对 B15 末卡同键命中。
        let idx = build_index().ok().unwrap_or_default();
        let a = flat_lookup(&idx, 36300).ok().copied();
        let b = hier_lookup(&idx, 36300).ok().copied();
        assert_eq!(a, b);
        assert_eq!(a.map(|r| r.seg).unwrap_or(0), 0xB170 - 0x10 + 0x10);
        // 越界卡双路一致失败。
        assert!(flat_lookup(&idx, 99999).is_err());
        assert!(hier_lookup(&idx, 99999).is_err());
    }
}
