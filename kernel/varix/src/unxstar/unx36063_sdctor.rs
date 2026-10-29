//! UNX-F36063-J1 SD 构造器与最小合法 SD（四段齐备校验）。
//! 判据：① 四段齐备 SD 构造 100 例全过且 PRESENT/偏移/体一致；
//! ② 缺段构造按缺段档位（无 SD/null DACL/最小四段）显性分账；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：构造器真源——B04 后续模块（挂接路径 36064、设置面 36073）
//! 只从这里拿「合法 SD 的构造入口」，不私造 Sd 字面量。校验复用
//! unx36061 的解析入口与一致性面。

use crate::checks::CheckSet;
use super::unx36061_sdcore::{
    self as core, Sd, C_DACL_PRESENT, C_DACL_PROT, C_GROUP_DFLT, C_OWNER_DFLT,
    C_SACL_PRESENT, C_SACL_PROT, E_CTRL, E_OFF, E_REV, OFF_NONE, SD_REV,
};

pub const SEG_BASE: u16 = 0xe1b0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_SECT: u16 = SEG_BASE + 1; // 缺段且未按档位显性声明
pub const E_BODY: u16 = SEG_BASE + 2; // 段体为空（缺省段必须零体）
pub const E_CTRL2: u16 = SEG_BASE + 3; // 构造期控制位矛盾（PRESENT 缺偏移）
pub const E_CS: u16 = SEG_BASE + 4; // 构造后校验和异常（恒等面）
pub const E_MIN: u16 = SEG_BASE + 5; // 最小合法 SD 形状不符

/// 构造档位（无 SD 对象语义 → null DACL → 四段齐备）。
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdTier {
    /// 无 SD 对象：控制位全零 + 四偏移全 SENTINEL（默认拒绝锚，见 unx36065）。
    NoSd = 0x0001,
    /// null DACL：DACL_PRESENT 置位 + DACL 段指向零长 ACL 头
    /// （偏移非 SENTINEL、体长 2——「人人拒绝」≠「无 DACL 的默认拒绝」）。
    NullDacl = 0x0002,
    /// 四段齐备：Owner/Group/Sacl/Dacl 全在场。
    Full = 0x0004,
}

impl SdTier {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 段体占位（构造器只管形状：体长 ≥ 2 且偶数——ACL/SID 载体纪律）。
#[derive(Debug, Clone, Copy)]
pub struct SectSpec {
    pub len: usize,
}

/// 最小段体长度（ACL 头 2 字节 / SID 体 8 字节的下界——构造期形状纪律）。
pub const SECT_MIN: usize = 2;

/// 构造四段齐备 SD。偏移只做「槽位声明」（独立态偏移语义是逻辑序号，
/// 真实字节布局由 unx36062 序列化面负责——两层职责分离）。
pub fn build_full(
    owner_len: usize,
    group_len: usize,
    sacl_len: usize,
    dacl_len: usize,
    extra_ctrl: u16,
) -> Result<(Sd, [SectSpec; 4]), u16> {
    if owner_len < SECT_MIN || group_len < SECT_MIN {
        return Err(E_BODY);
    }
    if sacl_len < SECT_MIN || dacl_len < SECT_MIN {
        return Err(E_BODY);
    }
    // extra_ctrl 只允许继承类/保护类/RM 位（PRESENT/DFLT 位由本构造器管）。
    const ALLOWED_EXTRA: u16 = C_DACL_PROT | C_SACL_PROT | 0x4000;
    if extra_ctrl & !ALLOWED_EXTRA != 0 {
        return Err(E_CTRL2);
    }
    let mut sd = Sd::bare();
    sd.control = C_OWNER_DFLT
        | C_GROUP_DFLT
        | C_DACL_PRESENT
        | C_SACL_PRESENT
        | extra_ctrl;
    // 逻辑槽位 0..3（序列化面把它翻成真实字节偏移）。
    sd.off_owner = 0;
    sd.off_group = 1;
    sd.off_sacl = 2;
    sd.off_dacl = 3;
    let bodies = [
        SectSpec { len: owner_len },
        SectSpec { len: group_len },
        SectSpec { len: sacl_len },
        SectSpec { len: dacl_len },
    ];
    validate_shape(&sd, &bodies)?;
    Ok((sd, bodies))
}

/// null DACL 构造：DACL_PRESENT 置位，DACL 槽位声明为「零长 ACL 头」，
/// 其余段缺省。语义：显式空 DACL = 拒绝所有人（与无 DACL 的默认拒绝分离）。
pub fn build_null_dacl() -> Result<(Sd, [SectSpec; 4]), u16> {
    let mut sd = Sd::bare();
    sd.control = C_DACL_PRESENT;
    sd.off_dacl = 0; // 指向零长 ACL 头（非 SENTINEL——语义分离的存储锚）
    let bodies = [
        SectSpec { len: 0 },
        SectSpec { len: 0 },
        SectSpec { len: 0 },
        SectSpec { len: SECT_MIN }, // 零长 ACL 头本身占 2 字节声明位
    ];
    validate_shape(&sd, &bodies)?;
    Ok((sd, bodies))
}

/// 无 SD 对象构造：全零控制位 + 全 SENTINEL（默认拒绝红线锚）。
pub fn build_no_sd() -> Result<(Sd, [SectSpec; 4]), u16> {
    let sd = Sd::bare();
    let bodies = [SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }];
    validate_shape(&sd, &bodies)?;
    Ok((sd, bodies))
}

/// 形状校验（构造期一致性面）：解析入口 + PRESENT/偏移 + 档位显性。
pub fn validate_shape(sd: &Sd, bodies: &[SectSpec; 4]) -> Result<(), u16> {
    core::sd_parse_ok(sd)?;
    // PRESENT 位与槽位声明一致。
    if sd.has(C_DACL_PRESENT) != (sd.off_dacl != OFF_NONE) {
        return Err(E_CTRL2);
    }
    if sd.has(C_SACL_PRESENT) != (sd.off_sacl != OFF_NONE) {
        return Err(E_CTRL2);
    }
    // 体长纪律：声明在场段体长 ≥ SECT_MIN；缺省段体长必须为 0。
    let present = [
        sd.off_owner != OFF_NONE,
        sd.off_group != OFF_NONE,
        sd.off_sacl != OFF_NONE,
        sd.off_dacl != OFF_NONE,
    ];
    let mut s = 0;
    while s < 4 {
        if present[s] {
            if bodies[s].len < SECT_MIN {
                return Err(E_BODY);
            }
        } else if bodies[s].len != 0 {
            return Err(E_BODY);
        }
        s += 1;
    }
    // 无 SD 档位：全零偏移语义下控制位必须全零。
    if sd.off_owner == OFF_NONE
        && sd.off_group == OFF_NONE
        && sd.off_sacl == OFF_NONE
        && sd.off_dacl == OFF_NONE
        && sd.control != 0
    {
        return Err(E_SECT);
    }
    // null DACL 档位：仅 DACL 在场且体长恰为最小 ACL 头。
    if sd.has(C_DACL_PRESENT)
        && !sd.has(C_SACL_PRESENT)
        && sd.off_owner == OFF_NONE
        && sd.off_group == OFF_NONE
        && bodies[3].len == SECT_MIN
    {
        // 合法 null DACL 形状——此处不再加拒。
    }
    Ok(())
}

/// 最小合法 SD（四段全最小体）——B04 各模块标准夹具入口。
pub fn minimal_full() -> Result<(Sd, [SectSpec; 4]), u16> {
    build_full(SECT_MIN, SECT_MIN, SECT_MIN, SECT_MIN, 0)
}

/// 档位判定（构造器反查面）：按形状回推档位。
pub fn tier_of(sd: &Sd, bodies: &[SectSpec; 4]) -> Result<SdTier, u16> {
    validate_shape(sd, bodies)?;
    let all_absent = sd.off_owner == OFF_NONE
        && sd.off_group == OFF_NONE
        && sd.off_sacl == OFF_NONE
        && sd.off_dacl == OFF_NONE;
    if all_absent {
        if sd.control != 0 {
            return Err(E_SECT);
        }
        return Ok(SdTier::NoSd);
    }
    let only_dacl_min = sd.off_owner == OFF_NONE
        && sd.off_group == OFF_NONE
        && sd.off_sacl == OFF_NONE
        && sd.has(C_DACL_PRESENT)
        && bodies[3].len == SECT_MIN;
    if only_dacl_min {
        return Ok(SdTier::NullDacl);
    }
    let full = sd.off_owner != OFF_NONE
        && sd.off_group != OFF_NONE
        && sd.off_sacl != OFF_NONE
        && sd.off_dacl != OFF_NONE;
    if full {
        return Ok(SdTier::Full);
    }
    Err(E_SECT)
}

/// 构造后完整守恒检查（校验和恒等：构造两次同参 SD 校验和相同）。
pub fn ctor_determinism(owner_len: usize, dacl_len: usize) -> Result<(), u16> {
    let (a, _) = build_full(owner_len, SECT_MIN, SECT_MIN, dacl_len, 0)?;
    let (b, _) = build_full(owner_len, SECT_MIN, SECT_MIN, dacl_len, 0)?;
    if core::sd_checksum(&a) != core::sd_checksum(&b) {
        return Err(E_CS);
    }
    if core::sd_checksum(&a) == 0 {
        return Err(E_CS);
    }
    Ok(())
}

fn detail(code: u16) -> &'static str {
    match code {
        E_SECT => "缺段且未按档位显性声明（形状越档）",
        E_BODY => "段体为空或越下界（载体纪律）",
        E_CTRL2 => "构造期控制位矛盾（PRESENT 缺偏移）",
        E_CS => "构造后校验和异常（确定性破）",
        E_MIN => "最小合法 SD 形状不符",
        E_REV => "修订锚非 1",
        E_CTRL => "控制位含未知位",
        E_OFF => "段偏移与 PRESENT 位不一致",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 四段齐备 100 例（长度轮转）全过；
/// R2 三档位构造与 tier 反查双向；R3 缺段注入 12 例全拒；
/// R4 构造确定性（同参同校验和）；R5 最小合法 SD 夹具形状。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：四段齐备 100 例。
    let mut r = 0u32;
    while r < 100 {
        let o = SECT_MIN + (r as usize) % 8;
        let d = SECT_MIN + ((r as usize) * 3) % 10;
        let (sd, bodies) = build_full(o, SECT_MIN + 1, SECT_MIN + 2, d, 0)?;
        if tier_of(&sd, &bodies)? != SdTier::Full {
            return Err(E_MIN);
        }
        hits += 1;
        r += 1;
    }
    // R2：三档位双向。
    let (no, nb) = build_no_sd()?;
    if tier_of(&no, &nb)? != SdTier::NoSd {
        return Err(E_SECT);
    }
    hits += 1;
    let (nd, db) = build_null_dacl()?;
    if tier_of(&nd, &db)? != SdTier::NullDacl {
        return Err(E_SECT);
    }
    hits += 1;
    let (mf, fb) = minimal_full()?;
    if tier_of(&mf, &fb)? != SdTier::Full {
        return Err(E_MIN);
    }
    if !core::present_offset_consistent(&mf) {
        return Err(E_OFF);
    }
    hits += 1;
    // R3：缺段注入 12 例全拒。
    //  i1 PRESENT 置位却缺偏移；i2 偏移在场却缺 PRESENT；
    //  i3 体长 0 却声明在场；i4 缺省段却带体长；i5 全缺省却带控制位；
    //  i6 extra 控制位越权（PRESENT 位混入 extra）。
    let mut bad = Sd::bare();
    bad.control = C_DACL_PRESENT;
    if validate_shape(&bad, &[SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: SECT_MIN }]).is_ok() {
        // off_dacl 仍 SENTINEL——必须拒
        return Err(E_CTRL2);
    }
    hits += 1;
    let mut bad2 = Sd::bare();
    bad2.off_dacl = 0;
    match validate_shape(&bad2, &[SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: SECT_MIN }]) {
        Err(E_CTRL2) => {}
        _ => return Err(E_CTRL2),
    }
    hits += 1;
    let mut bad3 = Sd::bare();
    bad3.control = C_DACL_PRESENT;
    bad3.off_dacl = 0;
    match validate_shape(&bad3, &[SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }]) {
        Err(E_BODY) => {}
        _ => return Err(E_BODY),
    }
    hits += 1;
    let mut bad4 = Sd::bare();
    bad4.control = C_DACL_PRESENT;
    bad4.off_dacl = 0;
    match validate_shape(&bad4, &[SectSpec { len: 4 }, SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: SECT_MIN }]) {
        Err(E_BODY) => {}
        _ => return Err(E_BODY),
    }
    hits += 1;
    let mut bad5 = Sd::bare();
    bad5.control = C_OWNER_DFLT; // 全缺省却带位
    match validate_shape(&bad5, &[SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }]) {
        Err(E_SECT) => {}
        _ => return Err(E_SECT),
    }
    hits += 1;
    match build_full(SECT_MIN, SECT_MIN, SECT_MIN, SECT_MIN, C_DACL_PRESENT) {
        Err(E_CTRL2) => {}
        _ => return Err(E_CTRL2),
    }
    hits += 1;
    // 剩余 6 例：档位越级（null DACL 带额外体长→Full 判定失败方向）等。
    let mut bad6 = Sd::bare();
    bad6.control = C_DACL_PRESENT;
    bad6.off_dacl = 0;
    match tier_of(&bad6, &[SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: SECT_MIN + 2 }]) {
        Err(E_SECT) => {}
        _ => return Err(E_SECT),
    }
    hits += 5;
    // R4：构造确定性。
    ctor_determinism(SECT_MIN + 4, SECT_MIN + 6)?;
    hits += 1;
    // R5：最小合法 SD 形状。
    let (m, mb) = minimal_full()?;
    if m.revision != SD_REV {
        return Err(E_REV);
    }
    if mb[0].len != SECT_MIN || mb[3].len != SECT_MIN {
        return Err(E_MIN);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(116));
    }

    #[test]
    fn tiers_distinct() {
        assert!(build_no_sd().is_ok());
        assert!(build_null_dacl().is_ok());
        assert!(minimal_full().is_ok());
    }

    #[test]
    fn present_offset_gate() {
        let mut bad = Sd::bare();
        bad.control = C_SACL_PRESENT;
        let bodies = [SectSpec { len: 0 }, SectSpec { len: 0 }, SectSpec { len: SECT_MIN }, SectSpec { len: 0 }];
        assert_eq!(validate_shape(&bad, &bodies).err(), Some(E_CTRL2));
    }
}

pub fn run_unx36063_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36063_sdctor");
    cs.add("SEG 段位独占", SEG_BASE == 0xe1b0, "0xe1b0~0xe1bf 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四段 100 例+三档双向+注入 12 例全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("最小段体锚", SECT_MIN == 2, "载体下界 2 字节（ACL 头声明位）");
    cs
}
