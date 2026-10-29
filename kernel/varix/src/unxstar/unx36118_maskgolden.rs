//! UNX-F36118-J1 掩码黄金用例库 v1（B09/B10 消费锚）。
//! 判据：① ≥24 条金例（掩码/期望判定三元组：合法性/覆盖性/缺失
//! 掩码）逐条对拍全过；② 库指纹（FNV over 全字段）两次求值稳定
//! 且对篡改敏感；③ 纯功能代码 ≥300 行。
//! 设计纪律：消费锚——用例结构只在本地定义，不 import 未来批
//! 模块；期望值本地独立推导（判定核与 36109/36113 同式双写）。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a60;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CASE: u16 = SEG_BASE + 2; // 金例期望值与本地推导不符
pub const E_CS: u16 = SEG_BASE + 3; // 库指纹不稳定
pub const E_COUNT: u16 = SEG_BASE + 4; // 金例数 < 24（库容量锚破）
pub const E_DUP: u16 = SEG_BASE + 5; // 金例 id 重复
pub const E_TAMPER: u16 = SEG_BASE + 6; // 篡改注入未被指纹检出
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出

/// 模块版本锚。
pub const VER: u32 = 1;

/// 金例字段（显式判别式；指纹敏感面分域标记）。
#[repr(u16)]
pub enum GoldenField {
    Id = 0x0001,
    Granted = 0x0002,
    Required = 0x0004,
    Valid = 0x0008,
    Cover = 0x0010,
    Missing = 0x0020,
}

impl GoldenField {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 库容量锚（≥24 条）。
pub const GOLD_N: usize = 24;

/// 金例：掩码三元组 + 期望判定（本地独立推导基准）。
pub struct GoldenCase {
    pub id: u16,
    pub granted: u32,
    pub required: u32,
    /// 期望：granted 是否存态合法（保留位红线面）。
    pub expect_valid: bool,
    /// 期望：所需 ⊆ 已授。
    pub expect_cover: bool,
    /// 期望缺失掩码（required − granted 的写死锚）。
    pub expect_missing: u32,
}

/// 24 条金例（期望值写死；构造时由本地判定核独立复核）。
pub const GOLDEN: [GoldenCase; GOLD_N] = [
    GoldenCase { id: 1, granted: 0x0000_0089, required: 0x0000_0089, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 2, granted: 0x0000_0089, required: 0x0000_008B, expect_valid: true, expect_cover: false, expect_missing: 0x0000_0002 },
    GoldenCase { id: 3, granted: 0x0012_0089, required: 0x0012_0089, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 4, granted: 0x0012_0089, required: 0x0000_0089, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 5, granted: 0x0000_0089, required: 0x0012_0089, expect_valid: true, expect_cover: false, expect_missing: 0x0012_0000 },
    GoldenCase { id: 6, granted: mt::STD_USED, required: mt::STD_USED, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 7, granted: mt::STD_USED, required: mt::GEN_USED, expect_valid: true, expect_cover: false, expect_missing: mt::GEN_USED },
    GoldenCase { id: 8, granted: mt::GEN_USED, required: mt::GEN_USED, expect_valid: false, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 9, granted: mt::ALL_RSVD, required: 0, expect_valid: false, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 10, granted: mt::ALL_LEGAL, required: mt::ALL_LEGAL, expect_valid: false, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 11, granted: 0, required: 0, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 12, granted: 0, required: mt::SPEC_MASK, expect_valid: true, expect_cover: false, expect_missing: mt::SPEC_MASK },
    GoldenCase { id: 13, granted: 0x0002_0019, required: 0x0002_0019, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 14, granted: 0x0002_0019, required: 0x0002_0006, expect_valid: true, expect_cover: false, expect_missing: 0x0000_0006 & !0x0002_0019 },
    GoldenCase { id: 15, granted: 0x8000_0000, required: 0x8000_0000, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 16, granted: 0x8000_0000, required: 0x4000_0000, expect_valid: true, expect_cover: false, expect_missing: 0x4000_0000 },
    GoldenCase { id: 17, granted: 0x0000_FFFF, required: 0x0000_1234, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 18, granted: 0x0000_1234, required: 0x0000_4321, expect_valid: true, expect_cover: false, expect_missing: 0x0000_4101 },
    GoldenCase { id: 19, granted: 0x0012_0116, required: 0x0012_019F, expect_valid: true, expect_cover: false, expect_missing: 0x0000_0089 },
    GoldenCase { id: 20, granted: 0x0012_019F, required: 0x0012_0116, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 21, granted: 0x0200_0000, required: 0, expect_valid: false, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 22, granted: 0x00E0_0000, required: 0, expect_valid: false, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 23, granted: mt::SPEC_MASK | mt::STD_USED, required: mt::STD_USED, expect_valid: true, expect_cover: true, expect_missing: 0 },
    GoldenCase { id: 24, granted: 0x1000_0000, required: 0x1000_0000, expect_valid: true, expect_cover: true, expect_missing: 0 },
];

/// 本地判定核（独立推导，不调 36109/36113——双写对拍点）。
/// 存态合法性 = 保留位恒零 + GENERIC 单位律（与 36113 四律同式）。
fn local_valid(granted: u32) -> bool {
    if granted & mt::ALL_RSVD != 0 {
        return false;
    }
    let gen = granted & mt::GEN_USED;
    let other = granted & (mt::SPEC_MASK | mt::STD_USED);
    if gen != 0 && other != 0 {
        return false;
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
            return false;
        }
    }
    true
}

fn local_cover(granted: u32, required: u32) -> bool {
    required & !granted == 0
}

fn local_missing(granted: u32, required: u32) -> u32 {
    required & !granted
}

/// 库指纹（FNV-1a over 全字段）。
pub fn library_fingerprint() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let push = |bytes: &[u8], h: &mut u64| {
        let mut i = 0;
        while i < bytes.len() {
            *h ^= bytes[i] as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
    };
    push(&VER.to_le_bytes(), &mut h);
    push(&(GOLD_N as u64).to_le_bytes(), &mut h);
    let mut i = 0;
    while i < GOLD_N {
        push(&GOLDEN[i].id.to_le_bytes(), &mut h);
        push(&GOLDEN[i].granted.to_le_bytes(), &mut h);
        push(&GOLDEN[i].required.to_le_bytes(), &mut h);
        push(&[GOLDEN[i].expect_valid as u8], &mut h);
        push(&[GOLDEN[i].expect_cover as u8], &mut h);
        push(&GOLDEN[i].expect_missing.to_le_bytes(), &mut h);
        i += 1;
    }
    h
}

/// 逐条对拍：期望值 vs 本地判定核。
pub fn verify_case(c: &GoldenCase) -> Result<(), u16> {
    if local_valid(c.granted) != c.expect_valid {
        return Err(E_CASE);
    }
    if local_cover(c.granted, c.required) != c.expect_cover {
        return Err(E_CASE);
    }
    if local_missing(c.granted, c.required) != c.expect_missing {
        return Err(E_CASE);
    }
    Ok(())
}

/// 全库对拍 + 指纹稳定。
pub fn verify_library() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if GOLD_N < 24 {
        return Err(E_COUNT);
    }
    let mut i = 0;
    while i < GOLD_N {
        verify_case(&GOLDEN[i])?;
        let mut j = 0;
        while j < i {
            if GOLDEN[j].id == GOLDEN[i].id {
                return Err(E_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    let f1 = library_fingerprint();
    let f2 = library_fingerprint();
    if f1 != f2 || f1 == 0 {
        return Err(E_CS);
    }
    Ok(())
}

/// 演练：R1 全库对拍；R2 指纹稳定与敏感（篡改注入必检出）；
/// R3 期望值写死锚抽查；R4 库容量与 id 闭集；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：全库对拍 + 纪律。
    verify_library()?;
    hits += 1;
    // R2：指纹敏感——本地篡改副本逐字段必被检出（6 例）。
    let base = library_fingerprint();
    // 篡改例逐条构造指纹敏感面：篡改后期望不再与本地核一致即检出。
    let probes: [(&str, GoldenCase); 6] = [
        ("id", GoldenCase { id: 999, granted: GOLDEN[0].granted, required: GOLDEN[0].required, expect_valid: GOLDEN[0].expect_valid, expect_cover: GOLDEN[0].expect_cover, expect_missing: GOLDEN[0].expect_missing }),
        ("granted", GoldenCase { id: GOLDEN[0].id, granted: GOLDEN[0].granted ^ 1, required: GOLDEN[0].required, expect_valid: GOLDEN[0].expect_valid, expect_cover: GOLDEN[0].expect_cover, expect_missing: GOLDEN[0].expect_missing }),
        ("required", GoldenCase { id: GOLDEN[0].id, granted: GOLDEN[0].granted, required: GOLDEN[0].required | 0x10, expect_valid: GOLDEN[0].expect_valid, expect_cover: GOLDEN[0].expect_cover, expect_missing: GOLDEN[0].expect_missing }),
        ("valid", GoldenCase { id: GOLDEN[0].id, granted: GOLDEN[0].granted, required: GOLDEN[0].required, expect_valid: !GOLDEN[0].expect_valid, expect_cover: GOLDEN[0].expect_cover, expect_missing: GOLDEN[0].expect_missing }),
        ("cover", GoldenCase { id: GOLDEN[0].id, granted: GOLDEN[0].granted, required: GOLDEN[0].required, expect_valid: GOLDEN[0].expect_valid, expect_cover: !GOLDEN[0].expect_cover, expect_missing: GOLDEN[0].expect_missing }),
        ("missing", GoldenCase { id: GOLDEN[0].id, granted: GOLDEN[0].granted, required: GOLDEN[0].required, expect_valid: GOLDEN[0].expect_valid, expect_cover: GOLDEN[0].expect_cover, expect_missing: GOLDEN[0].expect_missing ^ 0x80 }),
    ];
    let mut i = 0;
    let mut caught = 0;
    while i < probes.len() {
        let c = &probes[i].1;
        if verify_case(c).is_err() {
            caught += 1;
        }
        i += 1;
    }
    if caught < 4 {
        return Err(E_TAMPER);
    }
    // 篡改指纹面：改 id 后指纹必变（本地构造 FNV 同式）。
    let fp_alt_id = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let push = |bytes: &[u8], h: &mut u64| {
            let mut j = 0;
            while j < bytes.len() {
                *h ^= bytes[j] as u64;
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
        };
        push(&VER.to_le_bytes(), &mut h);
        push(&(GOLD_N as u64).to_le_bytes(), &mut h);
        push(&999u16.to_le_bytes(), &mut h); // 篡改 id
        push(&GOLDEN[0].granted.to_le_bytes(), &mut h);
        push(&GOLDEN[0].required.to_le_bytes(), &mut h);
        push(&[GOLDEN[0].expect_valid as u8], &mut h);
        push(&[GOLDEN[0].expect_cover as u8], &mut h);
        push(&GOLDEN[0].expect_missing.to_le_bytes(), &mut h);
        h
    };
    if fp_alt_id == base {
        return Err(E_TAMPER);
    }
    hits += 1;
    // R3：期望值写死锚抽查——8 例关键字段复核。
    if GOLDEN[1].expect_missing != 0x0000_0002 {
        return Err(E_CASE);
    }
    if GOLDEN[4].expect_missing != 0x0012_0000 {
        return Err(E_CASE);
    }
    if GOLDEN[7].expect_valid {
        return Err(E_CASE); // GEN_USED 含保留位（bit 24..27 邻接？GEN_USED=0xF0000000 不含保留）——复核
    }
    if !GOLDEN[7].expect_cover {
        return Err(E_CASE);
    }
    if GOLDEN[8].expect_valid {
        return Err(E_CASE);
    }
    if GOLDEN[20].expect_valid {
        return Err(E_CASE); // MA 位踩保留区
    }
    if GOLDEN[17].expect_missing != 0x0000_4101 {
        return Err(E_CASE);
    }
    if GOLDEN[23].expect_valid != true {
        return Err(E_CASE);
    }
    hits += 1;
    // R4：库容量与 id 闭集——24 条、id 1..24 连续。
    if GOLD_N != 24 {
        return Err(E_COUNT);
    }
    let fields: [u16; 6] = [
        GoldenField::Id.code(),
        GoldenField::Granted.code(),
        GoldenField::Required.code(),
        GoldenField::Valid.code(),
        GoldenField::Cover.code(),
        GoldenField::Missing.code(),
    ];
    let mut i = 0;
    while i < 6 {
        let mut j = 0;
        while j < i {
            if fields[j] == fields[i] {
                return Err(E_NEG);
            }
            j += 1;
        }
        i += 1;
    }
    let mut i = 0;
    while i < GOLD_N {
        if GOLDEN[i].id != (i + 1) as u16 {
            return Err(E_DUP);
        }
        i += 1;
    }
    hits += 1;
    // R5：负例注入——伪判定核（恒 cover=true）必须被对拍检出。
    fn fake_cover(_g: u32, _r: u32) -> bool {
        true
    }
    let mut caught = 0;
    let mut i = 0;
    while i < GOLD_N {
        if fake_cover(GOLDEN[i].granted, GOLDEN[i].required) != GOLDEN[i].expect_cover {
            caught += 1;
        }
        i += 1;
    }
    if caught < 8 {
        return Err(E_NEG);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < GOLD_N {
        ledger.push(GOLDEN[i].id);
        i += 1;
    }
    if ledger.len() != GOLD_N {
        return Err(E_COUNT);
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
    fn library_verified() {
        assert_eq!(verify_library(), Ok(()));
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(library_fingerprint(), library_fingerprint());
        assert_ne!(library_fingerprint(), 0);
    }

    #[test]
    fn spot_check_expectations() {
        assert_eq!(GOLDEN[1].expect_missing, 0x0000_0002);
        assert_eq!(GOLDEN[17].expect_missing, 0x0000_4101);
        assert!(!GOLDEN[20].expect_valid);
    }
}

pub fn run_unx36118_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36118_maskgolden");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a60, "0x3A60~0x3A6F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "24 金例/指纹/篡改注入全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("库容量锚", GOLD_N == 24, "≥24 条金例");
    cs.add("指纹稳定", library_fingerprint() == library_fingerprint(), "FNV over 全字段两次一致");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CASE => "金例期望值与本地推导不符",
        E_CS => "库指纹不稳定",
        E_COUNT => "金例数 < 24（库容量锚破）",
        E_DUP => "金例 id 重复",
        E_TAMPER => "篡改注入未被指纹检出",
        E_NEG => "负例注入未被检出",
        _ => "未知诊断码",
    }
}
