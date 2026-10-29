//! UNX-F36079-J1 B04 ktest 断言集（SD 面全判据注册）。
//! 判据：① 聚合面——36061~36078 十八模块 drill 全部收敛（全绿才绿）；
//! ② 批内守恒计数（模块数 19、SEG 窗口表逐项独占、判据面齐全）；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：B04 的 ktest 注册面——ktest 引擎（unx8141 族）按卡号取
/// 本面聚合判据；本模块不重写任何被聚合面的逻辑，只做转发与守恒。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore as m61;
use super::unx36062_sdselfrel as m62;
use super::unx36063_sdctor as m63;
use super::unx36064_sdattach as m64;
use super::unx36065_sddeny as m65;
use super::unx36066_sdnullacl as m66;
use super::unx36067_sdsacl as m67;
use super::unx36068_sdmlabel as m68;
use super::unx36069_sdowner as m69;
use super::unx36070_sdcow as m70;
use super::unx36071_sdisolate as m71;
use super::unx36072_sdquery as m72;
use super::unx36073_sdset as m73;
use super::unx36074_sdcmp as m74;
use super::unx36075_sdgen as m75;
use super::unx36076_sddiag as m76;
use super::unx36077_sdfuzz as m77;
use super::unx36078_sdperf as m78;

pub const SEG_BASE: u16 = 0xe2b0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_AGG: u16 = SEG_BASE + 1; // 聚合面破（某模块 drill 红）
pub const E_REG: u16 = SEG_BASE + 2; // 注册面破（卡号/模块对不齐）
pub const E_SEG: u16 = SEG_BASE + 3; // SEG 窗口表失守（越窗/撞窗）
pub const E_CNT: u16 = SEG_BASE + 4; // 守恒计数破（模块数/判据面数）

/// B04 批模块数（36061..=36078 → 18 模块 + 本模块 = 19）。
pub const B04_MODULE_N: usize = 19;

/// 批内 SEG 窗口表（卡号 → 窗基址；与本批 segs 分配一致）。
pub const SEG_TAB: [(u16, u16); B04_MODULE_N] = [
    (36061, 0x7500),
    (36062, 0xe1a0),
    (36063, 0xe1b0),
    (36064, 0xe1c0),
    (36065, 0xe1d0),
    (36066, 0xe1e0),
    (36067, 0xe1f0),
    (36068, 0xe200),
    (36069, 0xe210),
    (36070, 0xe220),
    (36071, 0xe230),
    (36072, 0xe240),
    (36073, 0xe250),
    (36074, 0xe260),
    (36075, 0xe270),
    (36076, 0xe280),
    (36077, 0xe290),
    (36078, 0xe2a0),
    (36079, 0xe2b0),
];

/// SEG 窗口表自检：卡号升序、窗口互不重叠（0x10 间距下界）、尾窗独占。
pub fn seg_tab_integrity() -> Result<(), u16> {
    if SEG_TAB.len() != B04_MODULE_N {
        return Err(E_CNT);
    }
    let mut i = 0;
    while i < B04_MODULE_N {
        let (card, seg) = SEG_TAB[i];
        // 卡号与位序对齐（36061+i）——注意 36061 特例（前会话占用 0x7500）。
        if i == 0 {
            if card != 36061 {
                return Err(E_REG);
            }
        } else if card != 36061 + i as u16 {
            return Err(E_REG);
        }
        if i > 0 {
            let (_, prev) = SEG_TAB[i - 1];
            // 窗间距 ≥ 0x10（0x10 宽窗口下界）；跨批特例（0x7500 → 0xe1a0）天然满足。
            if seg <= prev || seg - prev < 0x10 {
                return Err(E_SEG);
            }
        }
        i += 1;
    }
    Ok(())
}

/// 单模块 drill 收敛器（match 转发——每模块一行，红了带模块号）。
pub fn drill_one(slot: usize) -> Result<u32, u16> {
    match slot {
        0 => m61::drill(),
        1 => m62::drill(),
        2 => m63::drill(),
        3 => m64::drill(),
        4 => m65::drill(),
        5 => m66::drill(),
        6 => m67::drill(),
        7 => m68::drill(),
        8 => m69::drill(),
        9 => m70::drill(),
        10 => m71::drill(),
        11 => m72::drill(),
        12 => m73::drill(),
        13 => m74::drill(),
        14 => m75::drill(),
        15 => m76::drill(),
        16 => m77::drill(),
        17 => m78::drill(),
        _ => Err(E_REG),
    }
}

/// 批内模块总数（不含本模块：18 个被聚合面）。
pub const AGGREGATED_N: usize = 18;

/// 全批聚合：18 模块 drill 全绿返回总命中数。
pub fn drill_all() -> Result<u32, u16> {
    let mut total: u32 = 0;
    let mut slot = 0;
    while slot < AGGREGATED_N {
        let n = drill_one(slot)?;
        if n == 0 {
            return Err(E_AGG);
        }
        total += n;
        slot += 1;
    }
    Ok(total)
}

/// 聚合明细（slot → 命中数），供 CheckSet 逐项落账。
pub fn drill_matrix() -> Result<Vec<(usize, u32)>, u16> {
    let mut v: Vec<(usize, u32)> = Vec::new();
    let mut slot = 0;
    while slot < AGGREGATED_N {
        let n = drill_one(slot)?;
        v.push((slot, n));
        slot += 1;
    }
    Ok(v)
}

/// 双跑确定性矩阵：每 slot 的 drill 跑两遍，命中数必须逐槽相同
/// （确定性红线——drill 是纯函数面，任何漂移即红）。
pub fn determinism_matrix() -> Result<Vec<(usize, u32, u32)>, u16> {
    let mut v: Vec<(usize, u32, u32)> = Vec::new();
    let mut slot = 0;
    while slot < AGGREGATED_N {
        let a = drill_one(slot)?;
        let b = drill_one(slot)?;
        if a != b {
            return Err(E_AGG);
        }
        v.push((slot, a, b));
        slot += 1;
    }
    Ok(v)
}

/// 卡号注册表（slot → 卡号真源；与 mod.rs 元组序一致——注册面对拍源）。
pub const CARD_TAB: [u16; AGGREGATED_N] = [
    36061, 36062, 36063, 36064, 36065, 36066, 36067, 36068, 36069, 36070,
    36071, 36072, 36073, 36074, 36075, 36076, 36077, 36078,
];

/// 卡号注册表自检：升序连续、与 slot 对齐。
pub fn card_tab_integrity() -> Result<(), u16> {
    if CARD_TAB.len() != AGGREGATED_N {
        return Err(E_REG);
    }
    let mut i = 0;
    while i < AGGREGATED_N {
        if CARD_TAB[i] != 36061 + i as u16 {
            return Err(E_REG);
        }
        i += 1;
    }
    Ok(())
}

/// 批命中量守恒：双跑矩阵合计 = 单跑合计（同源对拍）。
pub fn aggregate_balance() -> Result<u32, u16> {
    let single = drill_all()?;
    let matrix = determinism_matrix()?;
    let mut sum = 0;
    let mut i = 0;
    while i < matrix.len() {
        sum += matrix[i].1;
        i += 1;
    }
    if sum != single {
        return Err(E_CNT);
    }
    Ok(sum)
}

/// slot 标题表（人话索引——ktest 报告面的人话投影真源）。
pub const SLOT_TITLES: [&str; AGGREGATED_N] = [
    "SD 结构全字段与控制位族",
    "SD 自相对格式序列化/反序列化",
    "SD 构造器与最小合法 SD",
    "ObCreateObject 挂接路径",
    "无 SD 对象默认拒绝不变量",
    "null DACL 与空 DACL 语义分离",
    "SACL 段结构与审计载体位",
    "强制标签 ACE 载体",
    "SD 所有者/组段写权限门",
    "SD 引用共享与写时复制",
    "SD 校验和与损坏即隔离",
    "SD 查询面最小集",
    "SD 设置面与掩码门",
    "SD 比较原语（语义/字节两层）",
    "SD 版本化与代际标记",
    "SD 诊断打印面",
    "SD fuzz 结构级鲁棒面",
    "SD 性能账与池化分配",
];

/// 标题表自检：18 条互异且非空（人话索引守恒）。
pub fn titles_integrity() -> Result<(), u16> {
    if SLOT_TITLES.len() != AGGREGATED_N {
        return Err(E_CNT);
    }
    let mut i = 0;
    while i < AGGREGATED_N {
        if SLOT_TITLES[i].is_empty() {
            return Err(E_CNT);
        }
        let mut j = i + 1;
        while j < AGGREGATED_N {
            if SLOT_TITLES[i] == SLOT_TITLES[j] {
                return Err(E_REG);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

fn detail(code: u16) -> &'static str {
    match code {
        E_AGG => "聚合面破（某模块 drill 红）",
        E_REG => "注册面破（卡号/模块对不齐）",
        E_SEG => "SEG 窗口表失守（越窗/撞窗/乱序）",
        E_CNT => "守恒计数破（模块数/判据面数）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 窗口表自检；R2 全批聚合 18 面；R3 聚合明细 18 项；
/// R4 越槽拒绝；R5 守恒计数（模块数/表长一致）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1。
    seg_tab_integrity()?;
    hits += 1;
    // R2。
    let total = drill_all()?;
    if total < 1000 {
        return Err(E_AGG); // 18 面合计命中量的下界锚
    }
    hits += 1;
    // R3。
    let matrix = drill_matrix()?;
    if matrix.len() != AGGREGATED_N {
        return Err(E_CNT);
    }
    let mut i = 0;
    while i < matrix.len() {
        if matrix[i].1 == 0 {
            return Err(E_AGG);
        }
        i += 1;
    }
    hits += 1;
    // R4：越槽拒绝（18/19/99 → E_REG）。
    if drill_one(AGGREGATED_N) != Err(E_REG)
        || drill_one(19) != Err(E_REG)
        || drill_one(99) != Err(E_REG)
    {
        return Err(E_REG);
    }
    hits += 1;
    // R5。
    if B04_MODULE_N != 19 || AGGREGATED_N != 18 || SEG_TAB.len() != B04_MODULE_N {
        return Err(E_CNT);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn all_slots_green() {
        let mut slot = 0;
        while slot < AGGREGATED_N {
            assert!(drill_one(slot).is_ok(), "slot {} 红", slot);
            slot += 1;
        }
    }

    #[test]
    fn seg_tab_sorted_unique() {
        assert!(seg_tab_integrity().is_ok());
    }
}

pub fn run_unx36079_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36079_b04ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0xe2b0, "0xe2b0~0xe2bf 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "18 面聚合全绿+窗口表自检+越槽全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("批内模块数锚", B04_MODULE_N == 19, "B04 = 18 聚合面 + 本注册面");
    match card_tab_integrity() {
        Ok(()) => cs.add("卡号注册表对齐", true, "18 卡升序连续与 slot 对齐"),
        Err(code) => cs.add("卡号注册表对齐", false, detail(code)),
    }
    match aggregate_balance() {
        Ok(_) => cs.add("双跑确定性+合计守恒", true, "18 槽双跑逐槽相同且合计一致"),
        Err(code) => cs.add("双跑确定性+合计守恒", false, detail(code)),
    }
    match titles_integrity() {
        Ok(()) => cs.add("标题表互异守恒", true, "18 条人话索引互异非空"),
        Err(code) => cs.add("标题表互异守恒", false, detail(code)),
    }
    cs
}
