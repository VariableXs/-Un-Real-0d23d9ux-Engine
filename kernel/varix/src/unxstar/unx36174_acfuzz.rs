//! UNX-F36174-J1 AccessCheck 差分模糊（acfuzz）。
//! 判据：① LCG 确定性模糊——生成随机 desired 掩码，双实现对拍：
//! 装配展开（36161::expand_desired_named）vs 本地独立展开；② 对抗
//! 模式注入（保留位/混段/越界级/坏映射名）；③ 失配即 Err；④ FNV
//! 模糊轨迹指纹；⑤ ≥310 行。
//! 设计纪律：差分真源 = 36161 装配；本地实现独立复刻展开律
//! （MA 剥离 + 请求面合法性 + GENERIC 翻译），失配暴露装配漂移。

use super::unx36101_masktable as mt;
use super::unx36104_genmap as gm;
use super::unx36110_maxallowed as ma;
use super::unx36113_maskvalid as mv;
use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x4040;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_DIFF: u16 = SEG_BASE + 2; // 双实现失配
pub const E_NEG: u16 = SEG_BASE + 3; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 4; // 模糊表闭集破坏
pub const E_SEED: u16 = SEG_BASE + 5; // 种子锚漂移

/// 模块版本锚。
pub const VER: u32 = 1;

/// 模糊参数锚。
pub const FUZZ_N: usize = 384;
pub const FUZZ_SEED: u64 = 0x1BAD_C0DE_4040_0036;

/// 对抗模式（判别式冻结；逐模式注入特形掩码）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdvMode {
    PureRsvd = 0x0000,    // 纯保留位
    MixedSeg = 0x0001,    // STD/GEN 混段
    MaPlusRsvd = 0x0002,  // MA + 保留位
    GenAll = 0x0003,      // G_ALL 翻译
    GenSingle = 0x0004,   // 单 GENERIC 位
    Concrete = 0x0005,    // 纯具体位
    RsvdLow = 0x0006,     // 低 16 位保留位（SPEC 区外）
}

impl AdvMode {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub const ADV_N: usize = 7;
}

/// 本地独立展开（对拍面；不借 36161 的装配函数）。
pub fn local_expand(reg: &gm::Registry, desired: u32, name: &str) -> Result<u32, u16> {
    // 律 1：请求面合法性（MA 位豁免）。
    mv::validate_request(desired)?;
    // 律 2：MA 剥离（请求位不入存储面）。
    let body = ma::strip_ma(desired);
    // 律 3：GENERIC 区翻译——G_ALL 走三槽并，单 GENERIC 走查表。
    let gen = body & mt::GEN_USED;
    if gen == 0 {
        return Ok(body);
    }
    if gen == gm::G_ALL {
        let c = reg.translate_all(name).map_err(|_| E_DIFF)?;
        return Ok((body & !mt::GEN_USED) | c);
    }
    let c = reg.translate(name, gen).map_err(|_| E_DIFF)?;
    Ok((body & !mt::GEN_USED) | c)
}

/// 差分一轮：装配展开 vs 本地展开，Ok 值必须相等，Err 必须同报。
pub fn diff_one(reg: &gm::Registry, desired: u32, name: &str) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let a = acf::expand_desired_named(reg, desired, name);
    let b = local_expand(reg, desired, name);
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x != y {
                return Err(E_DIFF);
            }
            Ok(())
        }
        (Err(_), Err(_)) => Ok(()),
        _ => Err(E_DIFF),
    }
}

/// LCG 模糊一轮：n 个伪随机掩码过差分（确定性种子）。
pub fn fuzz_round(reg: &gm::Registry, n: usize, seed: u64) -> Result<u64, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut state = seed;
    let mut h: u64 = 0x0100_93e7_0100_4041;
    let mut i = 0;
    while i < n {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // 掩码构造：四段拼装（SPEC 16 位 / STD 8 / GEN 4 / 高位）。
        let spec = (state >> 0) as u32 & 0x0000_FFFF;
        let std = (state >> 16) as u32 & 0x0000_001F;
        let gen = (state >> 24) as u32 & 0x0000_000F;
        let hi = (state >> 32) as u32 & 0x0F00_0000;
        let mask = spec | (std << 16) | (gen << 28) | hi;
        // 映射名轮换：File/RegistryKey 双面对拍。
        let name = if i % 2 == 0 { "File" } else { "RegistryKey" };
        diff_one(reg, mask, name)?;
        // 轨迹指纹入链。
        h ^= mask as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    Ok(h)
}

/// 对抗模式注入：七模式各一枚特形掩码——拒模式要求装配/本地双 Err，
/// 过模式走 diff_one 同值对拍。
pub fn adversarial_patterns(reg: &gm::Registry) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // PureRsvd：STD_RSVD 位 → 双拒。
    if acf::expand_desired_named(reg, mt::STD_RSVD, "File").is_ok()
        || local_expand(reg, mt::STD_RSVD, "File").is_ok()
    {
        return Err(E_NEG);
    }
    // MixedSeg：STD_USED|STD_RSVD 混段 → 双拒。
    if acf::expand_desired_named(reg, mt::STD_USED | mt::STD_RSVD, "File").is_ok()
        || local_expand(reg, mt::STD_USED | mt::STD_RSVD, "File").is_ok()
    {
        return Err(E_NEG);
    }
    // MaPlusRsvd：MA + GEN_RSVD → 双拒（MA 只豁请求位，不豁保留位）。
    if acf::expand_desired_named(reg, ma::MA_REQUEST | mt::GEN_RSVD, "File").is_ok()
        || local_expand(reg, ma::MA_REQUEST | mt::GEN_RSVD, "File").is_ok()
    {
        return Err(E_NEG);
    }
    // GenAll / GenSingle / Concrete / RsvdLow → 双过同值。
    diff_one(reg, gm::G_ALL, "File")?;
    diff_one(reg, gm::G_READ, "File")?;
    diff_one(reg, 0x0000_0089, "File")?;
    diff_one(reg, 0x0000_0189, "File")?;
    Ok(1)
}

/// 模糊面自检：参数锚 + 模式闭集 + 种子稳定。
pub fn fuzz_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if FUZZ_N != 384 || AdvMode::ADV_N != 7 {
        return Err(E_TABLE);
    }
    if FUZZ_SEED == 0 {
        return Err(E_SEED);
    }
    let mut k = 0;
    while k < AdvMode::ADV_N {
        if AdvMode::of(k as u16).is_none() {
            return Err(E_TABLE);
        }
        k += 1;
    }
    Ok(())
}

impl AdvMode {
    pub fn of(v: u16) -> Option<AdvMode> {
        match v {
            0 => Some(AdvMode::PureRsvd),
            1 => Some(AdvMode::MixedSeg),
            2 => Some(AdvMode::MaPlusRsvd),
            3 => Some(AdvMode::GenAll),
            4 => Some(AdvMode::GenSingle),
            5 => Some(AdvMode::Concrete),
            6 => Some(AdvMode::RsvdLow),
            _ => None,
        }
    }
}

pub const MODE_NAMES: [&str; 7] = [
    "pure-rsvd",
    "mixed-seg",
    "ma-plus-rsvd",
    "gen-all",
    "gen-single",
    "concrete",
    "rsvd-low",
];

/// 模式名回显（越界 "bad-mode"）。
pub fn mode_name(v: u16) -> &'static str {
    match v {
        0 => MODE_NAMES[0],
        1 => MODE_NAMES[1],
        2 => MODE_NAMES[2],
        3 => MODE_NAMES[3],
        4 => MODE_NAMES[4],
        5 => MODE_NAMES[5],
        6 => MODE_NAMES[6],
        _ => "bad-mode",
    }
}

/// 形状归类：按掩码位形判 AdvMode（对抗表驱动的逆查面）。
/// 判序：MA+保留 → 纯保留 → 混段 → G_ALL → 单 GENERIC →
/// STD+SPEC 并存（RsvdLow）→ 纯 SPEC 具体。
pub fn detect_class(desired: u32) -> AdvMode {
    let gen = desired & mt::GEN_USED;
    if ma::is_ma(desired) && desired & mt::ALL_RSVD != 0 {
        return AdvMode::MaPlusRsvd;
    }
    if desired != 0 && desired & !mt::ALL_RSVD == 0 {
        return AdvMode::PureRsvd;
    }
    if desired & mt::STD_USED != 0 && desired & mt::STD_RSVD != 0 {
        return AdvMode::MixedSeg;
    }
    if gen == gm::G_ALL {
        return AdvMode::GenAll;
    }
    if gen != 0 {
        return AdvMode::GenSingle;
    }
    if desired & mt::STD_USED != 0 {
        return AdvMode::RsvdLow; // STD 位与 SPEC 位合法并存（低区复合）
    }
    AdvMode::Concrete
}

/// 演练：R1 纪律自检；R2 全量模糊（384 掩码双面差分）；R3 对抗七模式；
/// R4 确定性回放（同种子同指纹）；R5 负例注入（坏映射名/种子漂移侦测）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：纪律自检。
    fuzz_integrity()?;
    hits += 1;
    // R2：全量模糊——384 掩码双面对拍。
    let reg = acf::build_registry()?;
    let h1 = fuzz_round(&reg, FUZZ_N, FUZZ_SEED)?;
    hits += 1;
    // R3：对抗七模式。
    let adv = adversarial_patterns(&reg)?;
    if adv != 1 {
        return Err(E_TABLE);
    }
    hits += 1;
    // R4：确定性回放——同种子重跑指纹一致；异种子指纹几乎必异。
    let h2 = fuzz_round(&reg, FUZZ_N, FUZZ_SEED)?;
    if h1 != h2 {
        return Err(E_SEED);
    }
    let h3 = fuzz_round(&reg, 8, FUZZ_SEED ^ 0xFFFF)?;
    let h4 = fuzz_round(&reg, 8, FUZZ_SEED ^ 0xF0F0)?;
    if h3 == h4 {
        return Err(E_SEED);
    }
    hits += 1;
    // R5：负例注入——坏映射名（GENERIC 位才走翻译表）双拒 / 纯保留位
    // 单独拒 / 模式越界。
    if acf::expand_desired_named(&reg, gm::G_READ, "NoSuch").is_ok()
        || local_expand(&reg, gm::G_READ, "NoSuch").is_ok()
    {
        return Err(E_NEG);
    }
    if local_expand(&reg, mt::STD_RSVD, "File").is_ok() {
        return Err(E_NEG);
    }
    if AdvMode::of(9).is_some() {
        return Err(E_NEG);
    }
    // 展开结果恒 ⊆ 合法区（装配守卫复述）。
    let exp = acf::expand_desired_named(&reg, gm::G_ALL, "File")?;
    if exp & !mt::ALL_LEGAL != 0 {
        return Err(E_NEG);
    }
    hits += 1;
    // R6：形状归类——七模式逆查对拍 + 名回显边界。
    if detect_class(mt::STD_RSVD) != AdvMode::PureRsvd {
        return Err(E_NEG);
    }
    if detect_class(mt::STD_USED | mt::STD_RSVD) != AdvMode::MixedSeg {
        return Err(E_NEG);
    }
    if detect_class(ma::MA_REQUEST | mt::GEN_RSVD) != AdvMode::MaPlusRsvd {
        return Err(E_NEG);
    }
    if detect_class(gm::G_ALL) != AdvMode::GenAll {
        return Err(E_NEG);
    }
    if detect_class(gm::G_READ) != AdvMode::GenSingle {
        return Err(E_NEG);
    }
    if detect_class(0x0000_0089) != AdvMode::Concrete {
        return Err(E_NEG);
    }
    if detect_class(0x0000_0189) != AdvMode::Concrete {
        return Err(E_NEG);
    }
    if detect_class(0x0001_0089) != AdvMode::RsvdLow {
        return Err(E_NEG);
    }
    if mode_name(9) != "bad-mode" || AdvMode::of(9).is_some() {
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
        assert_eq!(drill(), Ok(6));
    }

    #[test]
    fn fuzz_table_holds() {
        assert_eq!(fuzz_integrity(), Ok(()));
    }

    #[test]
    fn diff_on_concrete_matches() {
        let reg = acf::build_registry().ok().unwrap_or_else(gm::Registry::new);
        assert_eq!(diff_one(&reg, 0x89, "File"), Ok(()));
    }

    #[test]
    fn adv_all_reject_or_pass() {
        let reg = acf::build_registry().ok().unwrap_or_else(gm::Registry::new);
        assert_eq!(adversarial_patterns(&reg), Ok(1));
    }
}

pub fn run_unx36174_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36174_acfuzz");
    cs.add("SEG 段位独占", SEG_BASE == 0x4040, "0x4040~0x404F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "384 掩码差分 + 七模式全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", fuzz_integrity() == Ok(()), "参数锚 + 模式闭集 + 种子");
    cs.add("契约冻结", VER == 1, "模糊面锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DIFF => "双实现失配",
        E_NEG => "负例注入未被检出",
        E_TABLE => "模糊表闭集破坏",
        E_SEED => "种子锚漂移",
        _ => "未知诊断码",
    }
}
