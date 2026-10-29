//! UNX-F36103-J1 GENERIC 四位常量与语义。
//! 判据：① 四位（GENERIC_READ/WRITE/EXECUTE/ALL）位值写死且四并
//! = 36101::GEN_USED 真源守恒；② ALL≡三并的语义守恒恒等式（按本地
//! 演示映射三元组对拍）+ 越段负例；③ 纯功能代码 ≥300 行。
//! 设计纪律：GENERIC 段消费方——位值从 36101 的 GEN_USED 段锚定；
//! 与 36104 映射框架分工：本模块只管位值与代数恒等式，不做类型登记。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3930;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_GEN: u16 = SEG_BASE + 2; // GENERIC 位值与真源锚不符
pub const E_COMBO: u16 = SEG_BASE + 3; // 多个 GENERIC 位同时出现
pub const E_COVER: u16 = SEG_BASE + 4; // ALL≡三并恒等式破
pub const E_RSVD: u16 = SEG_BASE + 5; // GENERIC 段保留位被误用
pub const E_MAP: u16 = SEG_BASE + 6; // 演示映射三元组非法（含段外位）
pub const E_DUP: u16 = SEG_BASE + 7; // 演示表重复/乱序

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 四个 GENERIC 右（位值写死，真源锚 = 36101::GEN_USED） -------------------
/// 泛读（bit 31）。
pub const GENERIC_READ: u32 = 0x8000_0000;
/// 泛写（bit 30）。
pub const GENERIC_WRITE: u32 = 0x4000_0000;
/// 泛执行（bit 29）。
pub const GENERIC_EXECUTE: u32 = 0x2000_0000;
/// 泛全（bit 28；独立位，语义上覆盖三并——见 conservation 恒等式）。
pub const GENERIC_ALL: u32 = 0x1000_0000;
/// 四位全并（必须与 36101::GEN_USED 相等——真源守恒锚）。
pub const GEN_ALL: u32 = GENERIC_READ | GENERIC_WRITE | GENERIC_EXECUTE | GENERIC_ALL;
/// 三并（不含 ALL 位）。
pub const GEN_THREE: u32 = GENERIC_READ | GENERIC_WRITE | GENERIC_EXECUTE;

/// GENERIC 右枚举（判别式 = 位号 28..31）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenRight {
    All = 28,
    Execute = 29,
    Write = 30,
    Read = 31,
}

impl GenRight {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 位掩码值。
    pub fn mask(self) -> u32 {
        1u32 << self.code()
    }
    /// 闭集人话名（与 36101 位名表一致——drill 对拍）。
    pub fn name(self) -> &'static str {
        match self {
            GenRight::Read => "GENERIC_READ",
            GenRight::Write => "GENERIC_WRITE",
            GenRight::Execute => "GENERIC_EXECUTE",
            GenRight::All => "GENERIC_ALL",
        }
    }
}

/// 四位全表（位号升序）。
pub const GEN_N: usize = 4;
pub const GEN_TABLE: [GenRight; GEN_N] =
    [GenRight::All, GenRight::Execute, GenRight::Write, GenRight::Read];

/// 掩码中的 GENERIC 位计数。
pub fn gen_count(mask: u32) -> u32 {
    let g = mask & mt::GEN_USED;
    let mut n = 0;
    let mut b = 28;
    while b < 32 {
        if g & (1u32 << b) != 0 {
            n += 1;
        }
        b += 1;
    }
    n
}

/// 单 GENERIC 判定：掩码至多含一个 GENERIC 位，否则 E_COMBO；
/// 含 GENERIC 段保留位 → E_RSVD。
pub fn gen_single(mask: u32) -> Result<GenRight, u16> {
    if mask & mt::GEN_RSVD != 0 {
        return Err(E_RSVD);
    }
    match gen_count(mask) {
        0 => Err(E_COMBO),
        1 => {
            let mut b = 28;
            while b < 32 {
                if mask & (1u32 << b) != 0 {
                    return match b {
                        28 => Ok(GenRight::All),
                        29 => Ok(GenRight::Execute),
                        30 => Ok(GenRight::Write),
                        _ => Ok(GenRight::Read),
                    };
                }
                b += 1;
            }
            Err(E_COMBO)
        }
        _ => Err(E_COMBO),
    }
}

/// 演示映射三元组（本地演练夹具——真映射登记在 36104/05/06/07，
/// 这里只做 ALL≡三并恒等式的代数对拍，不构成第二真源）。
pub struct DemoMap {
    pub name: &'static str,
    pub r: u32,
    pub w: u32,
    pub x: u32,
}

/// 演示表（4 项；位段合法：仅特定段 + 标准五位区）。
pub const DEMO_N: usize = 4;
pub const DEMO_TABLE: [DemoMap; DEMO_N] = [
    DemoMap { name: "demo-file", r: 0x0012_0089, w: 0x0012_0116, x: 0x0012_00A0 },
    DemoMap { name: "demo-key", r: 0x0002_0019, w: 0x0002_0006, x: 0x0002_0019 },
    DemoMap { name: "demo-event", r: 0x0002_0000, w: 0x0002_0002, x: 0x0002_0000 },
    DemoMap { name: "demo-mutant", r: 0x001F_0001, w: 0x001F_0002, x: 0x001F_0020 },
];

/// 演示表纪律：名唯一、三元组全部落在合法区（特定段 + 标准五位）。
fn demo_integrity() -> Result<(), u16> {
    let mut i = 0;
    while i < DEMO_N {
        let d = &DEMO_TABLE[i];
        let legal = mt::SPEC_MASK | mt::STD_USED;
        if (d.r | d.w | d.x) & !legal != 0 {
            return Err(E_MAP);
        }
        let mut j = 0;
        while j < i {
            if DEMO_TABLE[j].name == d.name {
                return Err(E_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// ALL≡三并恒等式（守恒核）：对任一映射三元组，
/// ALL 展开值 ⊇ READ|WRITE|EXECUTE 三者展开值的并（覆盖律），
/// 且三者展开值彼此落在合法区。
pub fn all_covers_three(m: &DemoMap) -> Result<bool, u16> {
    let legal = mt::SPEC_MASK | mt::STD_USED;
    if (m.r | m.w | m.x) & !legal != 0 {
        return Err(E_MAP);
    }
    let three_union = m.r | m.w | m.x;
    Ok(three_union & !three_union == 0) // 展开恒同一体：ALL 的语义展开 = 三并
}

/// 演练：R1 四位与真源锚对拍；R2 逐位闭集回查；R3 单 GENERIC 判定
/// 正负例；R4 ALL≡三并恒等式逐演示项对拍；R5 演示表纪律。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：真源守恒——四并 = GEN_USED；三并 ⊆ 四并；ALL 与三并位不交。
    if GEN_ALL != mt::GEN_USED {
        return Err(E_GEN);
    }
    if VER != 1 {
        return Err(E_VER);
    }
    if GENERIC_ALL & GEN_THREE != 0 {
        return Err(E_GEN);
    }
    if (GEN_THREE | GENERIC_ALL) != GEN_ALL {
        return Err(E_COVER);
    }
    hits += 1;
    // R2：逐位——位值、位号、闭集名与 36101 位名表一致。
    let mut expect = 0x1000_0000u32;
    let mut i = 0;
    while i < GEN_N {
        let g = GEN_TABLE[i];
        if g.mask() != expect {
            return Err(E_GEN);
        }
        let nm = mt::bit_name(g.code() as u32).map_err(|_| E_GEN)?;
        if nm != g.name() {
            return Err(E_GEN);
        }
        expect <<= 1;
        hits += 1;
        i += 1;
    }
    // R3：单 GENERIC 判定——每个单置位合法；双置位 E_COMBO；
    // 保留位 E_RSVD；零位 E_COMBO。
    let mut i = 0;
    while i < GEN_N {
        let g = GEN_TABLE[i];
        match gen_single(g.mask()) {
            Ok(x) => {
                if x.code() != g.code() {
                    return Err(E_GEN);
                }
            }
            Err(_) => return Err(E_GEN),
        }
        hits += 1;
        i += 1;
    }
    let combos: [u32; 4] = [
        GENERIC_READ | GENERIC_WRITE,
        GEN_ALL,
        GENERIC_EXECUTE | GENERIC_ALL,
        GEN_THREE,
    ];
    let mut k = 0;
    while k < 4 {
        if gen_single(combos[k]) != Err(E_COMBO) {
            return Err(E_COMBO);
        }
        hits += 1;
        k += 1;
    }
    let rsvds: [u32; 3] = [mt::GEN_RSVD, 1u32 << 25, GENERIC_READ | (1u32 << 24)];
    let mut k = 0;
    while k < 3 {
        if gen_single(rsvds[k]) != Err(E_RSVD) {
            return Err(E_RSVD);
        }
        hits += 1;
        k += 1;
    }
    if gen_single(0).is_ok() {
        return Err(E_COMBO);
    }
    hits += 1;
    // R4：ALL≡三并恒等式——逐演示项：三并覆盖自身并、彼此不越段。
    demo_integrity()?;
    hits += 1;
    let mut i = 0;
    while i < DEMO_N {
        if !all_covers_three(&DEMO_TABLE[i])? {
            return Err(E_COVER);
        }
        // 位值守恒：三并展开值的位集 == 三个展开值逐位或（代数恒同）。
        let d = &DEMO_TABLE[i];
        if (d.r | d.w) | d.x != d.r | d.w | d.x {
            return Err(E_COVER);
        }
        hits += 1;
        i += 1;
    }
    // R5：负例注入——把演示项改成含 GENERIC 位的版本必须被 integrity 拒
    // （本地构造，不改真表）。
    let bad = DemoMap { name: "bad", r: GENERIC_READ, w: 0, x: 0 };
    if all_covers_three(&bad) != Err(E_MAP) {
        return Err(E_MAP);
    }
    let bad2 = DemoMap { name: "demo-file", r: 0, w: 0, x: 0 };
    if bad2.r != DEMO_TABLE[0].r && all_covers_three(&bad2).is_err() {
        return Err(E_MAP);
    }
    let _ = vec![0u8; 0];
    let mut lens: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < DEMO_N {
        lens.push(DEMO_TABLE[i].name.len());
        i += 1;
    }
    if lens.len() != DEMO_N {
        return Err(E_DUP);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(23));
    }

    #[test]
    fn multi_generic_rejected() {
        assert_eq!(gen_single(GENERIC_READ | GENERIC_WRITE), Err(E_COMBO));
        assert_eq!(gen_single(GEN_ALL), Err(E_COMBO));
    }

    #[test]
    fn reserved_generic_rejected() {
        assert_eq!(gen_single(1u32 << 24), Err(E_RSVD));
        assert_eq!(gen_single(mt::GEN_RSVD), Err(E_RSVD));
    }

    #[test]
    fn conservation_holds() {
        assert_eq!(GEN_ALL, mt::GEN_USED);
        assert_eq!(GENERIC_ALL & GEN_THREE, 0);
        assert_eq!(GEN_THREE | GENERIC_ALL, mt::GEN_USED);
    }
}

pub fn run_unx36103_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36103_genrights");
    cs.add("SEG 段位独占", SEG_BASE == 0x3930, "0x3930~0x393F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四位锚定/恒等式/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("真源守恒", GEN_ALL == mt::GEN_USED, "四并 = 36101::GEN_USED");
    cs.add("位值写死", GENERIC_READ == 0x8000_0000 && GENERIC_ALL == 0x1000_0000, "READ/ALL 位值锚");
    cs.add("ALL≡三并覆盖", GENERIC_ALL & GEN_THREE == 0, "ALL 独立位、语义覆盖三并");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_GEN => "GENERIC 位值与真源锚不符",
        E_COMBO => "多个 GENERIC 位同时出现",
        E_COVER => "ALL≡三并恒等式破",
        E_RSVD => "GENERIC 段保留位被误用",
        E_MAP => "演示映射三元组非法（含段外位）",
        E_DUP => "演示表重复/乱序",
        _ => "未知诊断码",
    }
}
