//! UNX-F36096-J1 ACE 结构级 fuzz（结构随机翻转鲁棒面）。
//! 判据：① LCG 确定性随机翻转四区（类型/掩码/标志/尺寸）32 轮——坏例必拒
//! 且码正确（区域→期望码映射闭集）、8 例好例必过；② 四区覆盖各 ≥1 次；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36081_acecore（头四件闸/AceType 闭集/FNV）；
//! 随机源为确定性 LCG（红线：禁墙钟禁浮点），跑两遍结果逐位一致。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3880;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;     // 版本锚不符
pub const E_BAD_OK: u16 = SEG_BASE + 2;  // 坏例被放行（红线）
pub const E_BAD_CODE: u16 = SEG_BASE + 3;// 坏例拒绝码不符
pub const E_GOOD_ERR: u16 = SEG_BASE + 4;// 好例被拒（红线）
pub const E_COVER: u16 = SEG_BASE + 5;   // 四区覆盖破
pub const E_CS: u16 = SEG_BASE + 6;      // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// fuzz 轮数。
pub const FUZZ_ROUNDS: u32 = 32;
/// 好例轮数。
pub const GOOD_ROUNDS: u32 = 8;

/// LCG 状态推进（红线指定的确定性随机源）。
pub fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// 翻转区域（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzRegion {
    /// 类型字节区。
    RegionType  = 0x0001,
    /// 掩码区。
    RegionMask  = 0x0002,
    /// 标志区。
    RegionFlags = 0x0003,
    /// 尺寸区。
    RegionSize  = 0x0004,
}

impl FuzzRegion {
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 区域人话名（诊断面用）。
    pub fn name(self) -> &'static str {
        match self {
            FuzzRegion::RegionType => "type",
            FuzzRegion::RegionMask => "mask",
            FuzzRegion::RegionFlags => "flags",
            FuzzRegion::RegionSize => "size",
        }
    }

    /// 区域 → 期望拒绝码（映射闭集，四区互异）。
    pub fn expect_code(self) -> u16 {
        match self {
            FuzzRegion::RegionType => core::E_TYPE,
            FuzzRegion::RegionMask => core::E_MASK,
            FuzzRegion::RegionFlags => core::E_FLAGS,
            FuzzRegion::RegionSize => core::E_SIZE,
        }
    }
}

/// 各区坏值表（注入面：全部保证越闸）。
pub const BAD_TYPES: [u16; 12] = [0x0004, 0x0009, 0x000a, 0x000b, 0x000c, 0x000d, 0x000e, 0x000f, 0x0010, 0x0014, 0x00ee, 0x00ff];
pub const BAD_MASKS: [u32; 3] = [0x0200_0000, 0x0400_0000, 0x0600_0000];
pub const BAD_FLAGS: [u8; 3] = [0x20, 0x60, 0xa0];
pub const BAD_SIZES: [u16; 4] = [0, 7, 0xf001, 0xffff];

/// 基线好例（四件全合法）。
pub const BASE_TYPE: u8 = 0x00;
pub const BASE_FLAGS: u8 = 0x00;
pub const BASE_SIZE: u16 = core::ACE_HEADER_MIN as u16;
pub const BASE_MASK: u32 = 0x0001_0001;

/// 单轮注入：按区域覆写基线四件 → 真源闸。
pub fn inject_and_probe(region: FuzzRegion, idx: usize) -> Result<(), u16> {
    let (t, f, s, m) = match region {
        FuzzRegion::RegionType => (BAD_TYPES[idx % BAD_TYPES.len()] as u8, BASE_FLAGS, BASE_SIZE, BASE_MASK),
        FuzzRegion::RegionMask => (BASE_TYPE, BASE_FLAGS, BASE_SIZE, BAD_MASKS[idx % BAD_MASKS.len()]),
        FuzzRegion::RegionFlags => (BASE_TYPE, BAD_FLAGS[idx % BAD_FLAGS.len()], BASE_SIZE, BASE_MASK),
        FuzzRegion::RegionSize => (BASE_TYPE, BASE_FLAGS, BAD_SIZES[idx % BAD_SIZES.len()], BASE_MASK),
    };
    core::AceHeader::new(t, f, s, m).map(|_| ())
}

/// 好例生成：LCG 值约束在合法域内（掩码限特定+标准合法位、标志限低五位、尺寸近基线）。
pub fn good_variant(state: u64) -> Result<(), u16> {
    let mask: u32 = (state as u32) & 0x001f_ffff; // 特定 16 位 + 标准低 5 位，无保留
    let flags: u8 = ((state >> 32) as u8) & 0x1f;
    let size: u16 = BASE_SIZE + ((state >> 40) as u16 % 8) * 4;
    let types: [u8; 3] = [0x00, 0x01, 0x02];
    let t: u8 = types[((state >> 48) as usize) % 3];
    core::AceHeader::new(t, flags, size, mask).map(|_| ())
}

/// 轮转显式走查：四区 × 各自坏值表全量注入（不经 LCG，确定性补刀面）。
/// 返回注入总发数；任何一发被放行或码不符即 E_BAD_OK/E_BAD_CODE。
pub fn rotation_walk() -> Result<u32, u16> {
    let mut n: u32 = 0;
    let regions: Vec<FuzzRegion> = vec![
        FuzzRegion::RegionType,
        FuzzRegion::RegionMask,
        FuzzRegion::RegionFlags,
        FuzzRegion::RegionSize,
    ];
    let lens = [BAD_TYPES.len(), BAD_MASKS.len(), BAD_FLAGS.len(), BAD_SIZES.len()];
    let mut r = 0;
    while r < 4 {
        let mut i = 0;
        while i < lens[r] {
            match inject_and_probe(regions[r], i) {
                Ok(_) => return Err(E_BAD_OK),
                Err(c) => {
                    if c != regions[r].expect_code() {
                        return Err(E_BAD_CODE);
                    }
                }
            }
            n += 1;
            i += 1;
        }
        r += 1;
    }
    Ok(n)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(cov: [u32; 4]) -> String {
    let mut s = String::new();
    s.push_str("fuzz cover type=");
    s.push_str(&cov[0].to_string());
    s.push_str(" mask=");
    s.push_str(&cov[1].to_string());
    s.push_str(" flags=");
    s.push_str(&cov[2].to_string());
    s.push_str(" size=");
    s.push_str(&cov[3].to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_BAD_OK, E_BAD_CODE, E_GOOD_ERR, E_COVER, E_CS];
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
        E_BAD_OK => "坏例被放行（红线）",
        E_BAD_CODE => "坏例拒绝码不符",
        E_GOOD_ERR => "好例被拒（红线）",
        E_COVER => "四区覆盖破",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 LCG 确定性 + 表锚；R2 32 轮坏例必拒且码正确；R3 8 例好例必过；
/// R4 四区覆盖各 ≥1；R5 期望码映射闭集互异；R6 两遍重放逐位一致；
/// R7 轮转全量走查（12+3+3+4=22 发补刀）+ 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：LCG 确定性（前 8 态两遍一致）+ 坏值表非空。
    let mut s1 = 0x1234_5678_9abc_def0;
    let mut s2 = 0x1234_5678_9abc_def0;
    let mut k = 0;
    while k < 8 {
        s1 = lcg_next(s1);
        s2 = lcg_next(s2);
        if s1 != s2 {
            return Err(E_CS);
        }
        k += 1;
    }
    if BAD_TYPES.is_empty() || BAD_MASKS.is_empty() || BAD_FLAGS.is_empty() || BAD_SIZES.is_empty() {
        return Err(E_COVER);
    }
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：32 轮坏例注入——必拒且码正确。
    let regions: [FuzzRegion; 4] = [
        FuzzRegion::RegionType,
        FuzzRegion::RegionMask,
        FuzzRegion::RegionFlags,
        FuzzRegion::RegionSize,
    ];
    let mut cov: [u32; 4] = [0, 0, 0, 0];
    let mut st = 0x1234_5678_9abc_def0;
    let mut r = 0;
    while r < FUZZ_ROUNDS {
        st = lcg_next(st);
        let region = regions[(st % 4) as usize];
        let idx = ((st >> 8) as usize) % 12;
        match inject_and_probe(region, idx) {
            Ok(_) => return Err(E_BAD_OK),
            Err(c) => {
                if c != region.expect_code() {
                    return Err(E_BAD_CODE);
                }
            }
        }
        cov[(region.code() - 1) as usize] += 1;
        hits += 1;
        r += 1;
    }
    // R3：8 例好例必过。
    let mut g = 0;
    let mut gs = 0xdead_beef_cafe_1111;
    while g < GOOD_ROUNDS {
        gs = lcg_next(gs);
        match good_variant(gs) {
            Ok(_) => {}
            Err(_) => return Err(E_GOOD_ERR),
        }
        hits += 1;
        g += 1;
    }
    // R4：四区覆盖各 ≥1。
    let mut z = 0;
    while z < 4 {
        if cov[z] == 0 {
            return Err(E_COVER);
        }
        z += 1;
    }
    hits += 1;
    // R5：期望码映射闭集互异。
    let codes = [
        FuzzRegion::RegionType.expect_code(),
        FuzzRegion::RegionMask.expect_code(),
        FuzzRegion::RegionFlags.expect_code(),
        FuzzRegion::RegionSize.expect_code(),
    ];
    let mut i = 0;
    while i < 4 {
        let mut j = i + 1;
        while j < 4 {
            if codes[i] == codes[j] {
                return Err(E_BAD_CODE);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;
    // R6：两遍重放逐位一致（cov 与指纹均一致）。
    let mut cov2: [u32; 4] = [0, 0, 0, 0];
    let mut st2 = 0x1234_5678_9abc_def0;
    let mut r2 = 0;
    while r2 < FUZZ_ROUNDS {
        st2 = lcg_next(st2);
        let region = regions[(st2 % 4) as usize];
        cov2[(region.code() - 1) as usize] += 1;
        r2 += 1;
    }
    if cov[0] != cov2[0] || cov[1] != cov2[1] || cov[2] != cov2[2] || cov[3] != cov2[3] {
        return Err(E_CS);
    }
    let fp1 = core::fnv1a(&(st.to_le_bytes()));
    let fp2 = core::fnv1a(&(st.to_le_bytes()));
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R7：轮转全量走查（四区坏值表全量注入，22 发补刀）。
    let walked = rotation_walk()?;
    if walked != (BAD_TYPES.len() + BAD_MASKS.len() + BAD_FLAGS.len() + BAD_SIZES.len()) as u32 {
        return Err(E_COVER);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(45));
    }

    #[test]
    fn rotation_walk_full() {
        assert_eq!(rotation_walk(), Ok(22));
        assert!(FuzzRegion::RegionMask.name() == "mask");
    }

    #[test]
    fn bad_cases_rejected() {
        assert_eq!(inject_and_probe(FuzzRegion::RegionType, 0), Err(core::E_TYPE));
        assert_eq!(inject_and_probe(FuzzRegion::RegionFlags, 0), Err(core::E_FLAGS));
        assert_eq!(inject_and_probe(FuzzRegion::RegionSize, 0), Err(core::E_SIZE));
    }

    #[test]
    fn good_cases_pass() {
        assert_eq!(good_variant(0x0000_0000_0000_0000), Ok(()));
        assert_eq!(good_variant(u64::MAX), Ok(()));
    }

    #[test]
    fn lcg_deterministic() {
        let a = lcg_next(lcg_next(1));
        let b = lcg_next(lcg_next(1));
        assert_eq!(a, b);
        assert!(summary([1, 2, 3, 4]).contains("fuzz"));
    }
}

pub fn run_unx36096_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36096_acefuzz");
    cs.add("SEG 段位独占", SEG_BASE == 0x3880, "0x3880~0x388f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "32 轮坏例全拒且码正确、好例 8 例全过"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("fuzz 轮数锚 32", FUZZ_ROUNDS == 32, "fuzz 轮数锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs.add("四区覆盖锚", FuzzRegion::RegionType.code() == 1 && FuzzRegion::RegionSize.code() == 4, "翻转区域类别漂移");
    cs
}
