//! UNX-F36061-J1 SD 结构全字段与控制位族定义。
//! 判据：① 控制位 ≥14 个（SE_DACL_PRESENT 族）与版本锚逐位一致；
//! ② 未知控制位注入 10 例拒绝解析；③ 纯功能代码 ≥300 行。
//! 设计纪律：本模块是 B04 全批的安全描述符结构真源——Sd 布局、控制位
//! 常量、已知位掩码都从这里取，不造第二套；布局锚与 FNV 校验和是自检面。
//!
//! Windows SECURITY_DESCRIPTOR 语义对标：
//! - Revision 恒 1（SECURITY_DESCRIPTOR_REVISION）；
//! - Control 为 16 位位图，其中 14 位有定义，其余位必须为零
//!   （未知位注入即拒绝解析——前向兼容红线）；
//! - 四段偏移在自相对包内以字节偏移寻址；独立态用 0xFFFFFFFF 表示缺省。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7500;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_REV: u16 = SEG_BASE + 1; // 版本锚非 1
pub const E_CTRL: u16 = SEG_BASE + 2; // 控制位含未知位
pub const E_OFF: u16 = SEG_BASE + 3; // 偏移非法（SENTINEL 语义错用）
pub const E_LAYOUT: u16 = SEG_BASE + 4; // 布局锚破（字段尺寸序不符）
pub const E_CS: u16 = SEG_BASE + 5; // 校验和不符
pub const E_MASK: u16 = SEG_BASE + 6; // 位图读写不守恒
pub const E_BITS: u16 = SEG_BASE + 7; // 控制位族定义错位

/// SD 修订版锚（Windows SECURITY_DESCRIPTOR_REVISION = 1）。
pub const SD_REV: u8 = 1;

/// 控制位族（14 个，逐位对标 Windows SE_* 常量值）。
pub const C_OWNER_DFLT: u16 = 0x0001; // SE_OWNER_DEFAULTED
pub const C_GROUP_DFLT: u16 = 0x0002; // SE_GROUP_DEFAULTED
pub const C_DACL_PRESENT: u16 = 0x0004; // SE_DACL_PRESENT（判据原文锚位）
pub const C_DACL_DFLT: u16 = 0x0008; // SE_DACL_DEFAULTED
pub const C_SACL_PRESENT: u16 = 0x0010; // SE_SACL_PRESENT
pub const C_SACL_DFLT: u16 = 0x0020; // SE_SACL_DEFAULTED
pub const C_DACL_AUTO_REQ: u16 = 0x0100; // SE_DACL_AUTO_INHERIT_REQ
pub const C_SACL_AUTO_REQ: u16 = 0x0200; // SE_SACL_AUTO_INHERIT_REQ
pub const C_DACL_AUTO: u16 = 0x0400; // SE_DACL_AUTO_INHERITED
pub const C_SACL_AUTO: u16 = 0x0800; // SE_SACL_AUTO_INHERITED
pub const C_DACL_PROT: u16 = 0x1000; // SE_DACL_PROTECTED
pub const C_SACL_PROT: u16 = 0x2000; // SE_SACL_PROTECTED
pub const C_RM_VALID: u16 = 0x4000; // SE_RM_CONTROL_VALID
pub const C_SELF_REL: u16 = 0x8000; // SE_SELF_RELATIVE

/// 已知位掩码（14 位并集；其余位必须为 0）。
pub const C_KNOWN_MASK: u16 = 0xff3f;

/// 控制位族规模锚（判据原文 ≥14）。
pub const CTRL_N: usize = 14;

/// 控制位表：位值 + 名称（诊断面用）。
pub const CTRL_TAB: [(u16, &'static str); CTRL_N] = [
    (C_OWNER_DFLT, "SE_OWNER_DEFAULTED"),
    (C_GROUP_DFLT, "SE_GROUP_DEFAULTED"),
    (C_DACL_PRESENT, "SE_DACL_PRESENT"),
    (C_DACL_DFLT, "SE_DACL_DEFAULTED"),
    (C_SACL_PRESENT, "SE_SACL_PRESENT"),
    (C_SACL_DFLT, "SE_SACL_DEFAULTED"),
    (C_DACL_AUTO_REQ, "SE_DACL_AUTO_INHERIT_REQ"),
    (C_SACL_AUTO_REQ, "SE_SACL_AUTO_INHERIT_REQ"),
    (C_DACL_AUTO, "SE_DACL_AUTO_INHERITED"),
    (C_SACL_AUTO, "SE_SACL_AUTO_INHERITED"),
    (C_DACL_PROT, "SE_DACL_PROTECTED"),
    (C_SACL_PROT, "SE_SACL_PROTECTED"),
    (C_RM_VALID, "SE_RM_CONTROL_VALID"),
    (C_SELF_REL, "SE_SELF_RELATIVE"),
];

/// 缺省段哨兵（独立态无该段时四偏移取此值）。
pub const OFF_NONE: u32 = 0xffff_ffff;

/// SD 独立态结构（全字段；自相对包见 unx36062）。
/// 语义约定：off_* == OFF_NONE 表示该段缺省；PRESENT 位与偏移须一致——
/// DACL_PRESENT 置位则 off_dacl 必非 SENTINEL（null DACL 特例见 unx36066，
/// 其偏移指向零长 ACL 头而非 SENTINEL，位面纪律在 36066 收紧）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sd {
    pub revision: u8,
    pub control: u16,
    pub off_owner: u32,
    pub off_group: u32,
    pub off_sacl: u32,
    pub off_dacl: u32,
}

impl Sd {
    /// 空构造（全段缺省、控制位清零、修订锚 1）。
    pub fn bare() -> Sd {
        Sd {
            revision: SD_REV,
            control: 0,
            off_owner: OFF_NONE,
            off_group: OFF_NONE,
            off_sacl: OFF_NONE,
            off_dacl: OFF_NONE,
        }
    }

    /// 控制位检查（单位）。
    pub fn has(&self, bit: u16) -> bool {
        self.control & bit == bit
    }

    /// 控制位置位（返回旧值——unx36073 设置面语义锚）。
    pub fn set_bit(&mut self, bit: u16) -> u16 {
        let old = self.control;
        self.control |= bit;
        old
    }

    /// 控制位清位（返回旧值）。
    pub fn clear_bit(&mut self, bit: u16) -> u16 {
        let old = self.control;
        self.control &= !bit;
        old
    }

    /// 段缺省判定。
    pub fn owner_absent(&self) -> bool {
        self.off_owner == OFF_NONE
    }
    pub fn group_absent(&self) -> bool {
        self.off_group == OFF_NONE
    }
    pub fn sacl_absent(&self) -> bool {
        self.off_sacl == OFF_NONE
    }
    pub fn dacl_absent(&self) -> bool {
        self.off_dacl == OFF_NONE
    }
}

/// 结构解析前置校验：修订锚 + 未知控制位拒绝（判据②解析入口）。
pub fn sd_parse_ok(sd: &Sd) -> Result<(), u16> {
    if sd.revision != SD_REV {
        return Err(E_REV);
    }
    if sd.control & !C_KNOWN_MASK != 0 {
        return Err(E_CTRL);
    }
    Ok(())
}

/// PRESENT 位与偏移一致性（结构性纪律，缺一段即破）。
pub fn present_offset_consistent(sd: &Sd) -> bool {
    // DACL：PRESENT 置位 ⇔ off_dacl 非 SENTINEL。
    // SACL：同构（null DACL/SACL 的偏移语义在 unx36066 收紧为零长段头）。
    (sd.has(C_DACL_PRESENT) != sd.dacl_absent())
        && (sd.has(C_SACL_PRESENT) != sd.sacl_absent())
}

/// 布局锚：核心结构字段尺寸序（u8 + u8(Sbz1) + u16 + 4×u32 = 20 字节）。
pub const LAYOUT_REV_SZ: usize = 1;
pub const LAYOUT_SBZ_SZ: usize = 1;
pub const LAYOUT_CTRL_SZ: usize = 2;
pub const LAYOUT_OFF_SZ: usize = 4;
pub const LAYOUT_OFFS_N: usize = 4;
pub const LAYOUT_TOTAL: usize =
    LAYOUT_REV_SZ + LAYOUT_SBZ_SZ + LAYOUT_CTRL_SZ + LAYOUT_OFF_SZ * LAYOUT_OFFS_N;

/// 布局锚自检（编译期常量关系 + 运行期复核）。
pub fn layout_integrity() -> Result<(), u16> {
    if LAYOUT_REV_SZ != 1 || LAYOUT_SBZ_SZ != 1 || LAYOUT_CTRL_SZ != 2 || LAYOUT_OFF_SZ != 4 {
        return Err(E_LAYOUT);
    }
    if LAYOUT_OFFS_N != 4 {
        return Err(E_LAYOUT);
    }
    if LAYOUT_TOTAL != 20 {
        return Err(E_LAYOUT);
    }
    if core::mem::size_of::<u32>() != LAYOUT_OFF_SZ {
        return Err(E_LAYOUT);
    }
    if core::mem::size_of::<u16>() != LAYOUT_CTRL_SZ {
        return Err(E_LAYOUT);
    }
    Ok(())
}

/// 控制位族纪律自检：14 位逐位唯一、并集恰为已知掩码、无零位。
pub fn ctrl_table_integrity() -> Result<(), u16> {
    if CTRL_N != 14 {
        return Err(E_BITS);
    }
    let mut seen: u16 = 0;
    let mut i = 0;
    while i < CTRL_N {
        let (bit, name) = CTRL_TAB[i];
        if bit == 0 {
            return Err(E_BITS);
        }
        if seen & bit != 0 {
            return Err(E_BITS); // 重复位
        }
        if name.len() < 8 || !name.starts_with("SE_") {
            return Err(E_BITS);
        }
        seen |= bit;
        i += 1;
    }
    if seen != C_KNOWN_MASK {
        return Err(E_BITS); // 并集须恰为已知掩码
    }
    Ok(())
}

/// FNV-1a（B04 批统一校验和真源；后续模块按字段序复用）。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv_push(bytes: &[u8], h: &mut u64) {
    let mut i = 0;
    while i < bytes.len() {
        *h ^= bytes[i] as u64;
        *h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
}

/// SD 校验和（over 修订+控制位+四偏移）。
pub fn sd_checksum(sd: &Sd) -> u64 {
    let mut h = FNV_OFF;
    fnv_push(&[sd.revision], &mut h);
    fnv_push(&sd.control.to_le_bytes(), &mut h);
    fnv_push(&sd.off_owner.to_le_bytes(), &mut h);
    fnv_push(&sd.off_group.to_le_bytes(), &mut h);
    fnv_push(&sd.off_sacl.to_le_bytes(), &mut h);
    fnv_push(&sd.off_dacl.to_le_bytes(), &mut h);
    h
}

/// SD 结构纪律全检（unx36071 隔离面与 unx36058 类自检入口）。
pub fn sd_integrity(sd: &Sd) -> Result<(), u16> {
    sd_parse_ok(sd)?;
    if !present_offset_consistent(sd) {
        return Err(E_OFF);
    }
    if sd_checksum(sd) == 0 {
        return Err(E_CS);
    }
    Ok(())
}

/// 控制位名称（诊断面）。
pub fn ctrl_name(bit: u16) -> &'static str {
    let mut i = 0;
    while i < CTRL_N {
        if CTRL_TAB[i].0 == bit {
            return CTRL_TAB[i].1;
        }
        i += 1;
    }
    "?"
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_REV => "SD 版本锚非 1",
        E_CTRL => "控制位含未知位（拒绝解析）",
        E_OFF => "段偏移与 PRESENT 位不一致",
        E_LAYOUT => "布局锚破（字段尺寸序不符）",
        E_CS => "SD 校验和不符",
        E_MASK => "控制位图读写不守恒",
        E_BITS => "控制位族定义错位（重复/零位/掩码缺角）",
        _ => "未知诊断码",
    }
}

/// 演练：R1 布局锚+控制位族纪律；R2 控制位 14 位逐位与版本锚一致；
/// R3 未知控制位 10 例全拒；R4 位图读写守恒 100 例；R5 校验和稳定；
/// R6 PRESENT-偏移一致性双向。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：布局锚与控制位族纪律。
    layout_integrity()?;
    ctrl_table_integrity()?;
    hits += 1;
    // R2：14 位逐位一致（位值非零递增语义、名称可回查、版本锚 1）。
    let mut prev: u16 = 0;
    let mut i = 0;
    while i < CTRL_N {
        let (bit, _) = CTRL_TAB[i];
        if bit == 0 || bit & prev != 0 {
            return Err(E_BITS);
        }
        if ctrl_name(bit) != CTRL_TAB[i].1 {
            return Err(E_BITS);
        }
        if !C_KNOWN_MASK & bit != 0 {
            return Err(E_BITS);
        }
        prev |= bit;
        hits += 1;
        i += 1;
    }
    if prev != C_KNOWN_MASK {
        return Err(E_BITS);
    }
    if SD_REV != 1 {
        return Err(E_REV);
    }
    hits += 1;
    // R3：未知控制位 10 例全拒（解析入口红线）——0x0040/0x0080 是
    // 已知位图仅有的两个低位洞，其余为洞位与合法位的组合注入。
    let unknowns: [u16; 10] = [
        0x0040,
        0x0080,
        0x00c0, // 双洞组合
        0x0040 | C_DACL_PRESENT,
        0x0080 | C_SACL_PROT,
        0x00c0 | C_SELF_REL,
        0x0040 | 0x0080 | C_DACL_AUTO,
        0x0040 | C_RM_VALID | C_OWNER_DFLT,
        0x0080 | C_DACL_PRESENT | C_SACL_PRESENT,
        0x00c0 | C_KNOWN_MASK, // 全已知位 + 全洞
    ];
    let mut k = 0;
    while k < 10 {
        let mut sd = Sd::bare();
        sd.control = unknowns[k];
        if sd_parse_ok(&sd) != Err(E_CTRL) {
            return Err(E_CTRL); // 未知位放行 = 判据红线
        }
        hits += 1;
        k += 1;
    }
    // R4：位图读写守恒 100 例（置位/清位往返 + 旧值语义）。
    let mut r = 0u32;
    while r < 100 {
        let mut sd = Sd::bare();
        let bit = CTRL_TAB[(r as usize) % CTRL_N].0;
        let old = sd.set_bit(bit);
        if old != 0 || !sd.has(bit) {
            return Err(E_MASK);
        }
        let old2 = sd.clear_bit(bit);
        if old2 != bit || sd.has(bit) {
            return Err(E_MASK);
        }
        if sd.control != 0 {
            return Err(E_MASK);
        }
        hits += 1;
        r += 1;
    }
    // R5：校验和两次求值稳定 + 非零。
    let mut sd = Sd::bare();
    sd.control = C_DACL_PRESENT;
    let c1 = sd_checksum(&sd);
    let c2 = sd_checksum(&sd);
    if c1 != c2 || c1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：PRESENT-偏移一致性双向（置位缺偏移即破；缺位带偏移即破）。
    let mut a = Sd::bare();
    a.control = C_DACL_PRESENT; // 置位但 off_dacl 仍 SENTINEL
    if present_offset_consistent(&a) {
        return Err(E_OFF);
    }
    let mut b = Sd::bare();
    b.off_dacl = 0; // 带偏移但 PRESENT 未置
    if present_offset_consistent(&b) {
        return Err(E_OFF);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(128));
    }

    #[test]
    fn unknown_ctrl_rejected() {
        let mut sd = Sd::bare();
        sd.control = 0x0040;
        assert_eq!(sd_parse_ok(&sd), Err(E_CTRL));
    }

    #[test]
    fn ctrl_family_complete() {
        assert_eq!(CTRL_N, 14);
        assert_eq!(ctrl_table_integrity(), Ok(()));
    }
}

pub fn run_unx36061_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36061_sdcore");
    cs.add("SEG 段位独占", SEG_BASE == 0x7500, "0x7500~0x751F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "控制位族 14 位对平+未知位 10 例全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
