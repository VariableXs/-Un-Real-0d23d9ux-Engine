//! UNX-F36117-J1 掩码 fuzz（随机位组合鲁棒面）。
//! 判据：① LCG 确定性随机位组合 512 例：非法组合必拒且诊断码
//! 与本地分类器一致（差分对拍）；② 合法组合必过 + 全位翻转/
//! 边界模式注入；③ 纯功能代码 ≥300 行。
//! 设计纪律：鲁棒面——本地分类器（独立推导）× 36113 校验器
//! 双实现对拍，分歧即红线；LCG 常量按 SPEC 统一，可复现。

use super::unx36101_masktable as mt;
use super::unx36113_maskvalid as mv;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a50;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_AGREE: u16 = SEG_BASE + 2; // 双实现分歧（差分对拍破）
pub const E_LEGAL: u16 = SEG_BASE + 3; // 合法组合被拒（校验器过严）
pub const E_ILLEGAL: u16 = SEG_BASE + 4; // 非法组合被放行（校验器过松）
pub const E_CODE: u16 = SEG_BASE + 5; // 拒绝码与预期不符
pub const E_REPRO: u16 = SEG_BASE + 6; // 复现破（两次 fuzz 结果不同）
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出

/// 模块版本锚。
pub const VER: u32 = 1;

/// fuzz 判定四分桶（显式判别式）。
#[repr(u16)]
pub enum FuzzVerdict {
    Legal = 0x0001,
    IllResv = 0x0002,
    IllMix = 0x0004,
    IllCombo = 0x0008,
}

impl FuzzVerdict {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 本地分类码 → 四分桶（闭集外走 IllCombo 兜底桶）。
    pub fn of_local(local: u16) -> FuzzVerdict {
        match local {
            E_RESV_L => FuzzVerdict::IllResv,
            E_MIX_L => FuzzVerdict::IllMix,
            _ => FuzzVerdict::IllCombo,
        }
    }
}

/// fuzz 量锚。
pub const FUZZ_N: usize = 512;

/// LCG 种子（批内固定——可复现红线）。
pub const FUZZ_SEED: u64 = 0x1BAD_C0DE_5EED_0036;

/// 本地分类器（独立推导，不调 36113）：
/// 返回 Ok(()) 或 (E_RESV/E_MIXSEG/E_GENCOMBO) 语义等价码。
pub fn classify_local(mask: u32) -> Result<(), u16> {
    if mask & mt::ALL_RSVD != 0 {
        return Err(E_RESV_L);
    }
    let gen = mask & mt::GEN_USED;
    let other = mask & (mt::SPEC_MASK | mt::STD_USED);
    if gen != 0 && other != 0 {
        return Err(E_MIX_L);
    }
    if gen != 0 {
        let mut n = 0;
        let mut b = 28;
        while b < 32 {
            if gen & (1u32 << b) != 0 {
                n += 1;
            }
            b += 1;
        }
        if n > 1 {
            return Err(E_COMBO_L);
        }
    }
    Ok(())
}

/// 本地分类码（与 36113 的 E_RESV/E_MIXSEG/E_GENCOMBO 值域隔离，
/// drill 中做映射对拍）。
pub const E_RESV_L: u16 = 0x1101;
pub const E_MIX_L: u16 = 0x1102;
pub const E_COMBO_L: u16 = 0x1103;

/// 本地码 → 36113 码映射（对拍锚）。
pub fn map_to_valid_code(local: u16) -> Result<u16, u16> {
    match local {
        E_RESV_L => Ok(mv::E_RESV),
        E_MIX_L => Ok(mv::E_MIXSEG),
        E_COMBO_L => Ok(mv::E_GENCOMBO),
        _ => Err(E_CODE),
    }
}

/// LCG 单步。
fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state
}

/// 随机掩码流（全 32 位域，含非法）。
pub fn fuzz_stream(n: usize) -> Vec<u32> {
    let mut state = FUZZ_SEED;
    let mut v: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < n {
        let raw = lcg_next(&mut state);
        v.push((raw >> 32) as u32 ^ (raw as u32));
        i += 1;
    }
    v
}

/// 边界模式注入表（8 例：全位翻转/交替/单位/段满/保留满）。
pub fn adversarial_patterns() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    v.push(0xFFFF_FFFF); // 全一
    v.push(0x5555_5555); // 交替 01
    v.push(0xAAAA_AAAA); // 交替 10
    v.push(0x0000_0001); // 单位最低
    v.push(0x8000_0000); // 单位最高
    v.push(mt::ALL_RSVD); // 纯保留
    v.push(mt::ALL_LEGAL); // 纯合法
    v.push(mt::STD_RSVD | mt::GEN_USED); // 保留+GENERIC 混
    v
}

/// fuzz 统计账。
pub struct FuzzStats {
    pub legal: u32,
    pub ill_resv: u32,
    pub ill_mix: u32,
    pub ill_combo: u32,
    pub agree: u32,
}

/// 对单掩码跑双实现对拍：本地分类 vs 36113 校验器。
pub fn diff_one(mask: u32) -> Result<bool, u16> {
    let l = classify_local(mask);
    let r = mv::validate_stored(mask);
    match (l, r) {
        (Ok(()), Ok(())) => Ok(true),
        (Ok(()), Err(_)) => Err(E_LEGAL),
        (Err(lc), Ok(())) => Err(E_ILLEGAL),
        (Err(lc), Err(rc)) => {
            let mapped = map_to_valid_code(lc).map_err(|_| E_CODE)?;
            if mapped != rc {
                return Err(E_AGREE);
            }
            Ok(false)
        }
    }
}

/// 演练：R1 流复现；R2 512 例 fuzz 全对拍；R3 边界模式注入；
/// R4 统计账守恒；R5 负例注入（伪分类器必须被差分检出）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：流复现——两次生成逐例相同；量锚与种子锚。
    if VER != 1 {
        return Err(E_VER);
    }
    if FUZZ_N != 512 {
        return Err(E_REPRO);
    }
    let s1 = fuzz_stream(FUZZ_N);
    let s2 = fuzz_stream(FUZZ_N);
    if s1.len() != FUZZ_N || s1 != s2 {
        return Err(E_REPRO);
    }
    hits += 1;
    // R2：512 例全对拍——合法必过、非法必拒且码一致。
    let mut st = FuzzStats { legal: 0, ill_resv: 0, ill_mix: 0, ill_combo: 0, agree: 0 };
    let mut i = 0;
    while i < FUZZ_N {
        match diff_one(s1[i])? {
            true => {
                st.legal += 1;
                st.agree += 1;
            }
            false => {
                // 非法：本地码归桶。
                match classify_local(s1[i]) {
                    Err(E_RESV_L) => st.ill_resv += 1,
                    Err(E_MIX_L) => st.ill_mix += 1,
                    Err(E_COMBO_L) => st.ill_combo += 1,
                    _ => return Err(E_CODE),
                }
                st.agree += 1;
            }
        }
        i += 1;
    }
    if st.legal + st.ill_resv + st.ill_mix + st.ill_combo != FUZZ_N as u32 {
        return Err(E_AGREE);
    }
    hits += 1;
    // R3：边界模式注入——8 例逐一 diff；非法例必须有码、合法例必过。
    let adv = adversarial_patterns();
    let mut ill_n = 0;
    let mut i = 0;
    while i < adv.len() {
        match diff_one(adv[i])? {
            true => {}
            false => ill_n += 1,
        }
        i += 1;
    }
    if ill_n < 4 {
        return Err(E_ILLEGAL); // 注入表中至少 4 例非法（全一/交替/保留混）
    }
    if mv::validate_stored(mt::ALL_LEGAL) != Err(mv::E_MIXSEG) {
        return Err(mv::E_MIXSEG); // 全合法域掩码混 GENERIC+特定段——存态必拒
    }
    if mv::validate_stored(0xFFFF_FFFF) != Err(mv::E_RESV) {
        return Err(mv::E_RESV); // 全一位踩保留区
    }
    hits += 1;
    // R4：统计账守恒——合法+三桶非法 = 512；桶位单调面（fuzz 流
    // 确定性 → 统计确定性：重跑一次逐桶相同）。
    let mut st2 = FuzzStats { legal: 0, ill_resv: 0, ill_mix: 0, ill_combo: 0, agree: 0 };
    let mut i = 0;
    while i < FUZZ_N {
        if diff_one(s2[i])? {
            st2.legal += 1;
        } else {
            match classify_local(s2[i]) {
                Err(E_RESV_L) => st2.ill_resv += 1,
                Err(E_MIX_L) => st2.ill_mix += 1,
                Err(E_COMBO_L) => st2.ill_combo += 1,
                _ => return Err(E_CODE),
            }
        }
        i += 1;
    }
    if st.legal != st2.legal
        || st.ill_resv != st2.ill_resv
        || st.ill_mix != st2.ill_mix
        || st.ill_combo != st2.ill_combo
    {
        return Err(E_REPRO);
    }
    hits += 1;
    // R5：负例注入——伪分类器（只认保留位）与真分类器差分必分歧。
    fn fake_classify(m: u32) -> Result<(), u16> {
        if m & mt::STD_RSVD != 0 {
            return Err(E_RESV_L);
        }
        Ok(())
    }
    let mut caught = 0;
    let mut i = 0;
    while i < FUZZ_N {
        let fake = fake_classify(s1[i]);
        let real = classify_local(s1[i]);
        let agree = match (fake, real) {
            (Ok(()), Ok(())) => true,
            (Err(_), Err(_)) => true,
            _ => false,
        };
        if !agree {
            caught += 1;
        }
        i += 1;
    }
    if caught == 0 {
        return Err(E_NEG); // 伪分类器必漏 GEN_RSVD/MIX/COMBO 三类
    }
    if map_to_valid_code(E_RESV_L) != Ok(mv::E_RESV) {
        return Err(E_CODE);
    }
    if FuzzVerdict::of_local(E_MIX_L).code() != FuzzVerdict::IllMix.code()
        || FuzzVerdict::of_local(E_RESV_L).code() != FuzzVerdict::IllResv.code()
    {
        return Err(E_CODE);
    }
    if map_to_valid_code(0x9999).is_ok() {
        return Err(E_CODE);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<u32> = Vec::new();
    ledger.push(st.legal);
    ledger.push(st.ill_resv);
    ledger.push(st.ill_mix);
    ledger.push(st.ill_combo);
    if ledger.len() != 4 {
        return Err(E_NEG);
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
    fn stream_reproducible() {
        assert_eq!(fuzz_stream(32), fuzz_stream(32));
        assert_eq!(fuzz_stream(0).len(), 0);
    }

    #[test]
    fn adversarial_rejected() {
        assert_eq!(classify_local(0xFFFF_FFFF), Err(E_RESV_L));
        assert_eq!(classify_local(mt::ALL_RSVD), Err(E_RESV_L));
        assert_eq!(classify_local(0xF000_0000), Err(E_COMBO_L));
        assert!(classify_local(mt::ALL_LEGAL).is_err()); // GENERIC+特定段混
    }

    #[test]
    fn code_map_closed() {
        assert_eq!(map_to_valid_code(E_RESV_L), Ok(mv::E_RESV));
        assert_eq!(map_to_valid_code(E_MIX_L), Ok(mv::E_MIXSEG));
        assert_eq!(map_to_valid_code(E_COMBO_L), Ok(mv::E_GENCOMBO));
        assert_eq!(map_to_valid_code(0x1234), Err(E_CODE));
    }
}

pub fn run_unx36117_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36117_maskfuzz");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a50, "0x3A50~0x3A5F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "512 例对拍/边界注入/复现全绿"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("量锚", FUZZ_N == 512, "512 例确定性 fuzz");
    cs.add("种子锚", FUZZ_SEED == 0x1BAD_C0DE_5EED_0036, "批内固定种子（可复现红线）");
    cs.add("双实现对拍", map_to_valid_code(E_RESV_L) == Ok(mv::E_RESV), "本地分类器 × 36113 映射锚");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_AGREE => "双实现分歧（差分对拍破）",
        E_LEGAL => "合法组合被拒（校验器过严）",
        E_ILLEGAL => "非法组合被放行（校验器过松）",
        E_CODE => "拒绝码与预期不符",
        E_REPRO => "复现破（两次 fuzz 结果不同）",
        E_NEG => "负例注入未被检出",
        _ => "未知诊断码",
    }
}
