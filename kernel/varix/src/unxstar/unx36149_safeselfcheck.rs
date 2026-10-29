//! UNX-F36149-J1 F 型段安全自检套件（启动链探针集成版）。
//! 判据：① 十段启动链探针（B01~B06 批代表 drill 五段 + B05 真源两段 +
//! 本批剧本一段 + 本模块自检两段）逐段 match 收敛、任一段出红即断链；
//! ② 断链定位（段号可回查）、双跑确定性对拍；③ 纯功能代码 ≥300 行。
//! 设计纪律：B08 汇总面——探针链只消费已存在批次的 pub drill()，
//! B07 未写（不 import 未写出模块），链位保留并显性记账。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use super::unx36001_sidcore;
use super::unx36021_tokencore;
use super::unx36041_privtable;
use super::unx36061_sdcore;
use super::unx36101_masktable;
use super::unx36141_script1;
use super::unx36142_script2;
use super::unx36143_script3;
use super::unx36144_script4;
use super::unx36145_script5;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xee30;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_CHAIN: u16 = SEG_BASE + 2; // 探针链出红
pub const E_STAGE: u16 = SEG_BASE + 3; // 断链段号越界/定位失败
pub const E_PEND: u16 = SEG_BASE + 4;  // 保留链位记账破
pub const E_CS: u16 = SEG_BASE + 5;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 探针链段数锚。
pub const CHAIN_N: usize = 10;
/// 保留链位（B07 到场后接入的段号集合——当前恰 {10}）。
pub const PENDING_STAGES: [usize; 1] = [10];

/// 探针段类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeStage {
    /// 段 1：B01 SID 真源。
    SidCore   = 0x0001,
    /// 段 2：B02 令牌真源。
    TokenCore = 0x0002,
    /// 段 3：B03 特权真源。
    PrivTable = 0x0003,
    /// 段 4：B04 SD 真源。
    SdCore    = 0x0004,
    /// 段 5：B06 掩码真源。
    MaskTable = 0x0005,
    /// 段 6：B08 剧本 I（全链）。
    Script1   = 0x0006,
    /// 段 7：B08 剧本 II（SID 传播）。
    Script2   = 0x0007,
    /// 段 8：B08 剧本 I~V。
    Scripts   = 0x0008,
    /// 段 9：本地自检（锚位/码集）。
    LocalGate = 0x0009,
    /// 段 10：保留（B07 到场接入）。
    Reserved  = 0x000a,
}

impl ProbeStage {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 单段探针：跑对应 drill，绿返回命中数。
pub fn probe_stage(stage: ProbeStage) -> Result<u32, u16> {
    match stage {
        ProbeStage::SidCore => match unx36001_sidcore::drill() {
            Ok(_) => Ok(1),
            Err(_) => Err(E_CHAIN),
        },
        ProbeStage::TokenCore => match unx36021_tokencore::drill() {
            Ok(_) => Ok(1),
            Err(_) => Err(E_CHAIN),
        },
        ProbeStage::PrivTable => match unx36041_privtable::drill() {
            Ok(_) => Ok(1),
            Err(_) => Err(E_CHAIN),
        },
        ProbeStage::SdCore => match unx36061_sdcore::drill() {
            Ok(_) => Ok(1),
            Err(_) => Err(E_CHAIN),
        },
        ProbeStage::MaskTable => match unx36101_masktable::drill() {
            Ok(_) => Ok(1),
            Err(_) => Err(E_CHAIN),
        },
        ProbeStage::Script1 => match unx36141_script1::drill() {
            Ok(_) => Ok(1),
            Err(_) => Err(E_CHAIN),
        },
        ProbeStage::Script2 => match unx36142_script2::drill() {
            Ok(_) => Ok(1),
            Err(_) => Err(E_CHAIN),
        },
        ProbeStage::Scripts => {
            // 剧本五连：任一出红即断链。
            unx36143_script3::drill()?;
            unx36144_script4::drill()?;
            unx36145_script5::drill()?;
            Ok(5)
        }
        ProbeStage::LocalGate => {
            // 本地闸：链段判别式序 + 版本锚 + 码集互异（借用 script1 的码集面）。
            if VER != 1 {
                return Err(E_VER);
            }
            if !(ProbeStage::SidCore.code() < ProbeStage::Scripts.code()
                && ProbeStage::Scripts.code() < ProbeStage::Reserved.code())
            {
                return Err(E_CHAIN);
            }
            if !unx36141_script1::codes_distinct() || !unx36142_script2::codes_distinct() {
                return Err(E_CHAIN);
            }
            Ok(3)
        }
        ProbeStage::Reserved => Err(E_PEND), // 保留位显性报待接
    }
}

/// 全链跑：按段序逐段收敛（保留段显性记账不计绿），返回绿段数。
pub fn run_chain() -> Result<u32, u16> {
    let stages: Vec<ProbeStage> = vec![
        ProbeStage::SidCore,
        ProbeStage::TokenCore,
        ProbeStage::PrivTable,
        ProbeStage::SdCore,
        ProbeStage::MaskTable,
        ProbeStage::Script1,
        ProbeStage::Script2,
        ProbeStage::Scripts,
        ProbeStage::LocalGate,
        ProbeStage::Reserved,
    ];
    let mut green_n: u32 = 0;
    let mut pending_n: u32 = 0;
    let mut i = 0;
    while i < stages.len() {
        match probe_stage(stages[i]) {
            Ok(_) => green_n += 1,
            Err(E_PEND) => pending_n += 1,
            Err(_) => return Err(E_STAGE),
        }
        i += 1;
    }
    // 记账守恒：绿段 + 保留段 = 段数锚；保留段位与 PENDING_STAGES 恰合。
    if green_n + pending_n != CHAIN_N as u32 {
        return Err(E_CHAIN);
    }
    if pending_n as usize != PENDING_STAGES.len() {
        return Err(E_PEND);
    }
    Ok(green_n)
}

/// 断链定位演示：保留段按段号查询应报 E_PEND（定位面）。
pub fn locate_stage(no: usize) -> Result<ProbeStage, u16> {
    match no {
        1 => Ok(ProbeStage::SidCore),
        2 => Ok(ProbeStage::TokenCore),
        3 => Ok(ProbeStage::PrivTable),
        4 => Ok(ProbeStage::SdCore),
        5 => Ok(ProbeStage::MaskTable),
        6 => Ok(ProbeStage::Script1),
        7 => Ok(ProbeStage::Script2),
        8 => Ok(ProbeStage::Scripts),
        9 => Ok(ProbeStage::LocalGate),
        10 => Ok(ProbeStage::Reserved),
        _ => Err(E_STAGE),
    }
}

/// 摘要行（String 面，无 IO）。
pub fn summary(green: u32) -> String {
    let mut s = String::new();
    s.push_str("safeselfcheck green=");
    s.push_str(&green.to_string());
    s.push_str("/");
    s.push_str(&CHAIN_N.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_CHAIN, E_STAGE, E_PEND, E_CS];
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
        E_CHAIN => "探针链出红/记账守恒破",
        E_STAGE => "断链段号越界/定位失败",
        E_PEND => "保留链位显性待接",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 链位记账守恒；R2 全链 9 绿 + 1 保留；R3 段号定位 10 段全可回查、
/// 越界 2 例拒；R4 保留段显性报待接；R5 指纹稳定；R6 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 段数锚。
    if VER != 1 {
        return Err(E_VER);
    }
    if CHAIN_N != 10 || PENDING_STAGES != [10] {
        return Err(E_PEND);
    }
    hits += 1;
    // R2：全链收敛（9 绿 + 1 保留显性记账）。
    let green = run_chain()?;
    if green != 9 {
        return Err(E_CHAIN);
    }
    hits += green;
    // R3：段号定位全可回查 + 越界 2 例。
    let mut s = 1;
    while s <= CHAIN_N {
        let st = locate_stage(s)?;
        if st.code() != s as u16 {
            return Err(E_STAGE);
        }
        hits += 1;
        s += 1;
    }
    match locate_stage(0) {
        Ok(_) => return Err(E_STAGE),
        Err(c) if c == E_STAGE => {}
        Err(_) => return Err(E_STAGE),
    }
    hits += 1;
    match locate_stage(11) {
        Ok(_) => return Err(E_STAGE),
        Err(c) if c == E_STAGE => {}
        Err(_) => return Err(E_STAGE),
    }
    hits += 1;
    // R4：保留段显性报待接（非静默绿）。
    match probe_stage(ProbeStage::Reserved) {
        Ok(_) => return Err(E_PEND),
        Err(c) if c == E_PEND => {}
        Err(_) => return Err(E_PEND),
    }
    hits += 1;
    // R5：指纹两次求值稳定且非零。
    let fp1 = core::fnv1a(&[CHAIN_N as u8, PENDING_STAGES.len() as u8, VER as u8]);
    let fp2 = core::fnv1a(&[CHAIN_N as u8, PENDING_STAGES.len() as u8, VER as u8]);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：双跑一致。
    let a = run_chain()?;
    let b = run_chain()?;
    if a != b {
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
        assert_eq!(drill(), Ok(25));
    }

    #[test]
    fn chain_converges() {
        assert_eq!(run_chain(), Ok(9));
        assert!(summary(9).contains("safeselfcheck"));
    }

    #[test]
    fn locate_shapes() {
        assert_eq!(locate_stage(10), Ok(ProbeStage::Reserved));
        assert_eq!(locate_stage(12), Err(E_STAGE));
    }

    #[test]
    fn pending_explicit() {
        assert_eq!(probe_stage(ProbeStage::Reserved), Err(E_PEND));
        assert!(codes_distinct());
    }
}

pub fn run_unx36149_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36149_safeselfcheck");
    cs.add("SEG 段位独占", SEG_BASE == 0xee30, "0xee30~0xee3f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "十段启动链 9 绿 + 1 保留显性记账"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("链段锚 10", CHAIN_N == 10, "探针链段数锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
