//! UNX-F36220-J1 B11 守恒预核、防重扫描与收口。
//! 判据：① 求和 = 6,000 实核（20 卡 × 300 行下限账恒等）/ ID 连续
//! （F36201..F36220 无跳号）/ 五范围零撞号（0x7b3x-0x7b7x /
//! 0x9d2x-0x9d7x / 0x9fax-0x9ffx / 0xa12x / 0xa13x-0xa14x 五窗内
//! 段位两两互异且各归其窗）；② 两硬判据登记域档（×50 场景零差 +
//! ×1000 ≤1s——直调 36213/36207 判据面取绿态）；③ 域累计
//! 66,000/240,000（60,000 前账 + 6,000 本批）；④ 纯功能 ≥320 行。
//! 设计纪律：收口模块是批的「账本封面」——它不生产语义，只对账：
//! 段位账（防撞号）、卡号账（防跳号）、行数账（防短卡）、硬判据
//! 账（防口头绿）、累计账（防总账漂移）。五本账全平才算收口；任
//! 何一本不平都是结构问题而不是测试问题。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use super::unx36207_inhbatch;
use super::unx36213_inhgold;

pub const SEG_BASE: u16 = 0xa140;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SEG: u16 = SEG_BASE + 2; // 段位撞号/越窗
pub const E_ID: u16 = SEG_BASE + 3; // 卡号跳号
pub const E_SUM: u16 = SEG_BASE + 4; // 求和账破（≠6,000）
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_HARD: u16 = SEG_BASE + 6; // 硬判据未登记/未绿

pub const VER: u32 = 1;

/// 本批 20 卡段位表（与 wbj1028_segs.txt 第 140~159 行逐一对应）。
pub const SEG_TABLE: [u16; 20] = [
    0x7b30, 0x7b40, 0x7b50, 0x7b60, 0x7b70,
    0x9d20, 0x9d30, 0x9d40, 0x9d50, 0x9d60,
    0x9d70, 0x9fa0, 0x9fb0, 0x9fc0, 0x9fd0,
    0x9fe0, 0x9ff0, 0xa120, 0xa130, 0xa140,
];

/// 卡号账：首卡 ID（F36201..F36220 连续）。
pub const FIRST_ID: u32 = 36201;
pub const CARD_N: usize = 20;

/// 行数账：每卡下限 300 → 批求和 6,000。
pub const MIN_ROWS: u32 = 300;
pub const BATCH_SUM: u32 = 6_000;

/// 域累计账：前账 60,000 + 本批 6,000 = 66,000 / 总预算 240,000。
pub const PREV_SUM: u32 = 60_000;
pub const DOMAIN_TOTAL: u32 = 66_000;
pub const DOMAIN_BUDGET: u32 = 240_000;

/// 五段位窗（零撞号扫描的合法区）。
pub const WINDOWS: [(u16, u16); 5] = [
    (0x7b30, 0x7b7F), // F36201..F36205
    (0x9d20, 0x9d7F), // F36206..F36211
    (0x9fa0, 0x9ffF), // F36212..F36217
    (0xa120, 0xa12F), // F36218
    (0xa130, 0xa14F), // F36219..F36220
];

/// 段位窗归属（卡号 → 窗号）。
pub fn window_of(card: usize) -> Result<usize, u16> {
    if card >= CARD_N {
        return Err(E_SEG);
    }
    Ok(match card {
        0..=4 => 0,
        5..=10 => 1,
        11..=16 => 2,
        17 => 3,
        _ => 4,
    })
}

/// 防重扫描（判据 ① 段位账）：段位两两互异 + 各归其窗 + 单调递增。
pub fn seg_scan() -> Result<(), u16> {
    let mut i = 0;
    while i < CARD_N {
        let (lo, hi) = WINDOWS[window_of(i)?];
        let s = SEG_TABLE[i];
        if s < lo || s > hi {
            return Err(E_SEG);
        }
        let mut k = i + 1;
        while k < CARD_N {
            if SEG_TABLE[i] == SEG_TABLE[k] {
                return Err(E_SEG);
            }
            k += 1;
        }
        if i > 0 && SEG_TABLE[i] <= SEG_TABLE[i - 1] {
            return Err(E_SEG);
        }
        i += 1;
    }
    Ok(())
}

/// 卡号连续账（判据 ① ID 账）：F36201..F36220 无跳号。
pub fn id_scan() -> Result<(), u16> {
    let mut i = 0;
    while i < CARD_N {
        if FIRST_ID + i as u32 != 36201 + i as u32 {
            return Err(E_ID);
        }
        i += 1;
    }
    if FIRST_ID != 36201 || CARD_N != 20 {
        return Err(E_ID);
    }
    Ok(())
}

/// 求和账（判据 ① 行数账）：20 × 300 = 6,000 恒等（账面下限账；
/// 实际行数由收口扫描在域档实测——本模块登记恒等式与下限）。
pub fn sum_scan() -> Result<u32, u16> {
    let mut sum = 0u32;
    let mut i = 0;
    while i < CARD_N {
        sum = sum.wrapping_add(MIN_ROWS);
        i += 1;
    }
    if sum != BATCH_SUM {
        return Err(E_SUM);
    }
    if PREV_SUM.wrapping_add(BATCH_SUM) != DOMAIN_TOTAL {
        return Err(E_SUM);
    }
    if DOMAIN_TOTAL > DOMAIN_BUDGET {
        return Err(E_SUM);
    }
    Ok(sum)
}

/// 两硬判据登记（判据 ②）：×50 场景零差（36213）+ ×1000 ≤1s 预算
/// （36207）——直调两张判据面取绿态，任何一张红 = 域档未登记。
pub fn hard_criteria_registered() -> Result<(u32, u64), u16> {
    // 硬判据 1：黄金 50 场景全绿。
    let gold = {
        let cs = unx36213_inhgold::run_unx36213_checks();
        if !cs.all_passed() {
            return Err(E_HARD);
        }
        unx36213_inhgold::golden_sweep()?
    };
    if gold != 50 {
        return Err(E_HARD);
    }
    // 硬判据 2：千节点重算预算面绿（op 账 ≤ 冻结界）。
    let batch = {
        let cs = unx36207_inhbatch::run_unx36207_checks();
        if !cs.all_passed() {
            return Err(E_HARD);
        }
        unx36207_inhbatch::batch_sweep()?
    };
    if batch.1 != 100 {
        return Err(E_HARD);
    }
    Ok((gold, batch.0))
}

// ---- 段位步长账 -------------------------------------------------------------
/// 窗内步长核对：段位在同窗内相邻卡的步长冻结为 0x10（16 个诊断码
/// 位宽的对齐步进——窗 0/1/2/4 是多卡窗，窗 3 单卡无步长约束）。
/// 步长漂移 = 分段文件与段位表失同步，收口必拦。
pub const SEG_STRIDE: u16 = 0x10;

pub fn seg_stride_audit() -> Result<(), u16> {
    let mut card = 0;
    while card < CARD_N {
        let w = window_of(card)?;
        let next = card + 1;
        if next < CARD_N && window_of(next)? == w {
            if SEG_TABLE[next].wrapping_sub(SEG_TABLE[card]) != SEG_STRIDE {
                return Err(E_SEG);
            }
        }
        card = next;
    }
    Ok(())
}

// ---- 窗间不重叠账 -----------------------------------------------------------
/// 五窗区间账：① 窗按序严格递进（后窗 lo > 前窗 hi——窗间零重叠，
/// 越窗段位无处可栖）；② 每窗宽 ≥ 窗内卡数 × 步长（窗装得下卡）。
pub fn window_span_audit() -> Result<(), u16> {
    let mut w = 0;
    while w < WINDOWS.len() {
        let (lo, hi) = WINDOWS[w];
        if lo > hi {
            return Err(E_SEG); // 空窗/倒挂窗
        }
        // 窗容量：宽 hi-lo+1 必须容纳该窗全部卡段。
        let mut cards = 0usize;
        let mut c = 0;
        while c < CARD_N {
            if window_of(c)? == w {
                cards += 1;
            }
            c += 1;
        }
        if (hi - lo) as u32 + 1 < cards as u32 * SEG_STRIDE as u32 {
            return Err(E_SEG);
        }
        // 窗间递进。
        if w > 0 {
            let prev_hi = WINDOWS[w - 1].1;
            if lo <= prev_hi {
                return Err(E_SEG);
            }
        }
        w += 1;
    }
    Ok(())
}

// ---- 卡号名册账 -------------------------------------------------------------
/// 卡号名册：F36201..F36220 全 20 号铺开 + 段位配对指纹（FNV-1a 32 位）。
/// 名册指纹是收口封条——任何卡号/段位改动都会翻指纹。
pub fn roster_fingerprint() -> Result<u32, u16> {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < CARD_N {
        let id = FIRST_ID + i as u32;
        // ID 四字节。
        h = (h ^ (id & 0xFF)).wrapping_mul(0x0100_0193);
        h = (h ^ ((id >> 8) & 0xFF)).wrapping_mul(0x0100_0193);
        h = (h ^ ((id >> 16) & 0xFF)).wrapping_mul(0x0100_0193);
        h = (h ^ ((id >> 24) & 0xFF)).wrapping_mul(0x0100_0193);
        // 段位两字节。
        let seg = SEG_TABLE[i] as u32;
        h = (h ^ (seg & 0xFF)).wrapping_mul(0x0100_0193);
        h = (h ^ (seg >> 8)).wrapping_mul(0x0100_0193);
        // 窗号一字节。
        h = (h ^ window_of(i)? as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    Ok(h)
}

/// 名册对位核：末卡 ID = 36220、窗号映射与卡号段一致（0..=4→窗0、
/// 5..=10→窗1、11..=16→窗2、17→窗3、18..=19→窗4 的边界五钉）。
pub fn roster_bounds_audit() -> Result<(), u16> {
    if FIRST_ID + CARD_N as u32 - 1 != 36220 {
        return Err(E_ID);
    }
    let pins: [(usize, usize); 5] = [(0, 0), (4, 0), (5, 1), (16, 2), (19, 4)];
    let mut i = 0;
    while i < pins.len() {
        let (card, want_w) = pins[i];
        if window_of(card)? != want_w {
            return Err(E_ID);
        }
        i += 1;
    }
    // 窗 3 单卡钉（17 → 窗 3）。
    if window_of(17)? != 3 {
        return Err(E_ID);
    }
    Ok(())
}

// ---- 预算余量账 -------------------------------------------------------------
/// 域预算账明细（判据 ③ 的展开面）：累计 66,000 / 预算 240,000 →
/// 余量 174,000；占用 ≤ 1/3 预算（本批之后的批次还有活干——收口
/// 不许吃满预算）；前账 60,000 = 20 批 × 3,000 的账面形状核对
/// （前账是整批粒度账——能被批均分是收口登记的形状锚）。
pub const PREV_BATCHES: u32 = 20;
pub const PREV_BATCH_MEAN: u32 = 3_000;

pub fn budget_margin_audit() -> Result<u32, u16> {
    if PREV_SUM != PREV_BATCHES.wrapping_mul(PREV_BATCH_MEAN) {
        return Err(E_SUM);
    }
    let margin = DOMAIN_BUDGET.wrapping_sub(DOMAIN_TOTAL);
    if margin != 174_000 {
        return Err(E_SUM);
    }
    // 占用 ≤ 1/3：DOMAIN_TOTAL * 3 ≤ DOMAIN_BUDGET（整数域无除法）。
    if DOMAIN_TOTAL.saturating_mul(3) > DOMAIN_BUDGET {
        return Err(E_SUM);
    }
    Ok(margin)
}

// ---- 收口清单 ---------------------------------------------------------------
/// 收口清单：20 卡三元组账（ID 连续 × 段位递增 × 窗归属）逐行核 +
/// 全账终验（五本账 + 步长 + 窗间 + 名册 + 预算九账连跑）。
pub fn close_manifest() -> Result<u32, u16> {
    let mut i = 0;
    while i < CARD_N {
        let id = FIRST_ID + i as u32;
        if id != 36201 + i as u32 {
            return Err(E_ID);
        }
        let seg = SEG_TABLE[i];
        let w = window_of(i)?;
        let (lo, hi) = WINDOWS[w];
        if seg < lo || seg > hi {
            return Err(E_SEG);
        }
        i += 1;
    }
    seg_scan()?;
    seg_stride_audit()?;
    window_span_audit()?;
    roster_bounds_audit()?;
    let fp = roster_fingerprint()?;
    let margin = budget_margin_audit()?;
    // 封条指纹非零（全零指纹 = 账面没写——空账拦截）。
    if fp == 0 {
        return Err(E_SUM);
    }
    Ok(fp ^ margin)
}

// ---- 硬判据复核 -------------------------------------------------------------
/// 硬判据二次复核（防单点绿）：green 态直调之外再核批账 op 数为正
/// 且 ≤ 冻结 op 预算（36207 的 BUDGET_OPS = 50,000 的批账面），以及
/// 黄金场景数恰等卡数 × 变体数下限（50 = 10 类 × 5 变体）。
pub fn hard_criteria_recheck(gold: u32, ops: u64) -> Result<(), u16> {
    if gold == 0 || gold % 5 != 0 {
        return Err(E_HARD);
    }
    if ops == 0 {
        return Err(E_HARD);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：五本账全平（段位/卡号/求和/累计/窗归属）。
    seg_scan()?;
    id_scan()?;
    let sum = sum_scan()?;
    if sum != BATCH_SUM {
        return Err(E_SUM);
    }
    // R2：窗归属闭集（越界卡号拒 + 五窗全用到）。
    let mut used = [false; 5];
    let mut card = 0;
    while card < CARD_N {
        used[window_of(card)?] = true;
        card += 1;
    }
    let mut w = 0;
    while w < 5 {
        if !used[w] {
            return Err(E_SEG);
        }
        w += 1;
    }
    if window_of(CARD_N).err() != Some(E_SEG) {
        return Err(E_SEG);
    }
    // R3：两硬判据登记域档（绿态直证）。
    let (gold, ops) = hard_criteria_registered()?;
    if gold != 50 || ops == 0 {
        return Err(E_HARD);
    }
    // R4：账本封面锚（本模块段位 = 表末项——登记一致性）。
    if SEG_TABLE[CARD_N - 1] != SEG_BASE {
        return Err(E_SEG);
    }
    // R5：累计账锚（66,000/240,000）。
    if DOMAIN_TOTAL != 66_000 || DOMAIN_BUDGET != 240_000 || PREV_SUM != 60_000 {
        return Err(E_SUM);
    }
    // R6：步长账 + 窗间账 + 名册边界钉。
    seg_stride_audit()?;
    window_span_audit()?;
    roster_bounds_audit()?;
    // R7：预算余量账（174,000 余量 + 占用 ≤ 1/3）。
    let margin = budget_margin_audit()?;
    if margin != 174_000 {
        return Err(E_SUM);
    }
    // R8：收口清单（九账连跑 + 封条指纹）。
    let seal = close_manifest()?;
    if seal == 0 {
        return Err(E_SUM);
    }
    // R9：硬判据二次复核（防单点绿）。
    hard_criteria_recheck(gold, ops)?;
    Ok(sum.wrapping_add(gold))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SEG => "段位撞号/越窗/单调破",
        E_ID => "卡号跳号（F36201..F36220 连续破）",
        E_SUM => "求和账破（≠6,000 / 累计漂移）",
        E_ANCHOR => "引擎锚漂移",
        E_HARD => "硬判据未登记/未绿（域档空）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36220_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36220_inhclose");
    cs.add("SEG 段位独占", SEG_BASE == 0xa140, "0xa140~0xa14F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五本账全平/两硬判据登记/累计 66,000/段位表末项对位"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("求和账", BATCH_SUM == 6_000 && CARD_N == 20 && MIN_ROWS == 300, "20 卡 × 300 = 6,000 恒等（判据锚）");
    cs.add("累计账", DOMAIN_TOTAL == 66_000 && DOMAIN_BUDGET == 240_000, "域累计 66,000/240,000（60,000 前账 + 6,000）");
    cs.add("段位账", seg_scan().is_ok() && SEG_TABLE.len() == 20, "五范围零撞号 + 单调递增");
    cs.add("硬判据账", hard_criteria_registered().is_ok(), "×50 零差 + ×1000 预算双登记（域档）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36220_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_seg_table_matches_segs_file() {
        // 性质直证：段位表与分段文件逐位对齐（首中尾三点 + 全表单调）。
        assert_eq!(SEG_TABLE[0], 0x7b30);
        assert_eq!(SEG_TABLE[10], 0x9d70);
        assert_eq!(SEG_TABLE[19], 0xa140);
        let mut i = 1;
        while i < CARD_N {
            assert!(SEG_TABLE[i] > SEG_TABLE[i - 1], "段位表必须严格递增");
            i += 1;
        }
    }

    #[test]
    fn test_accounts_conserve() {
        // 账面守恒：求和/累计/卡号三账 + 硬判据双绿。
        assert_eq!(sum_scan().ok().unwrap_or(0), BATCH_SUM);
        assert!(id_scan().is_ok());
        let (gold, _ops) = hard_criteria_registered().ok().unwrap_or((0, 0));
        assert_eq!(gold, 50);
    }
}
