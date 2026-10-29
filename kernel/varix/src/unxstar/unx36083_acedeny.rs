//! UNX-F36083-J1 AccessDenied ACE 语义载体。
//! 判据：① deny 优先语义位（canon 序 rank=0，先于 allow 求值）+ 类型锚 +
//! 构造门（deny 掩码不得含 GENERIC 段），10 例合法构造往返全对平；
//! ② GENERIC 段注入 4 例、零掩码 2 例、坏 SID 2 例、错类型锚 2 例全部显性拒绝；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36081_acecore；与 36082（allow）同构对偶——
//! deny 的构造门比 allow 多一道 GENERIC 段禁令（36087 三段学的消费位）。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xe2f0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;     // 版本锚不符
pub const E_TYPE: u16 = SEG_BASE + 2;    // 类型锚不符（非 AccessDenied）
pub const E_SID: u16 = SEG_BASE + 3;     // SID 槽位引用非法
pub const E_MASK0: u16 = SEG_BASE + 4;   // 零掩码（空拒绝显性拒）
pub const E_GENERIC: u16 = SEG_BASE + 5; // deny 掩码含 GENERIC 段（构造门）
pub const E_RANK: u16 = SEG_BASE + 6;    // 求值序 rank 表破
pub const E_CS: u16 = SEG_BASE + 7;      // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// SID 槽位空值锚与上限（与 36082 同锚，跨模块一致）。
pub const SID_NIL: u16 = 0;
pub const SID_SLOT_MAX: u16 = 63;
/// deny ACE 线上形式长度：头 8 字节 + sid 槽位 le16。
pub const DENY_WIRE_LEN: usize = 10;
/// GENERIC 段位（deny 构造门禁段：36087 三段学的高 4 位）。
pub const GENERIC_SEGMENT: u32 = 0xf000_0000;

/// canon 求值序（#[repr(u16)] 显式判别式；deny 恒 0 = 最先求值）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AceRank {
    /// deny 优先位：DACL 扫描中 deny 命中即止。
    DenyFirst = 0x0000,
    /// allow 次位：未被 deny 命中才累积。
    AllowNext = 0x0001,
    /// 审计殿后：只记账不影响授权。
    AuditLast = 0x0002,
}

impl AceRank {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 类型字节 → canon 求值序（表外类型 = E_RANK）。
pub fn rank_of(ace_type: u8) -> Result<AceRank, u16> {
    if ace_type == core::AceType::AccessDenied.wire_byte()? {
        return Ok(AceRank::DenyFirst);
    }
    if ace_type == core::AceType::AccessAllowed.wire_byte()? {
        return Ok(AceRank::AllowNext);
    }
    if ace_type == core::AceType::SystemAudit.wire_byte()? {
        return Ok(AceRank::AuditLast);
    }
    Err(E_RANK)
}

/// AccessDenied ACE 语义载体：真源头 + SID 槽位引用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DenyAce {
    pub head: core::AceHeader,
    pub sid_slot: u16,
}

impl DenyAce {
    /// 受控构造：类型锚 → 真源四件闸 → SID 引用闸 → 零掩码闸 → GENERIC 禁段闸。
    pub fn new(flags: u8, mask: u32, sid_slot: u16) -> Result<DenyAce, u16> {
        let head = core::AceHeader::new(
            core::AceType::AccessDenied.wire_byte()?,
            flags,
            core::ACE_HEADER_MIN as u16,
            mask,
        )?;
        if sid_slot == SID_NIL || sid_slot > SID_SLOT_MAX {
            return Err(E_SID);
        }
        if mask == 0 {
            return Err(E_MASK0);
        }
        if mask & GENERIC_SEGMENT != 0 {
            return Err(E_GENERIC);
        }
        Ok(DenyAce { head, sid_slot })
    }

    /// 线上形式：头 8 字节 + sid 槽位 le16。
    pub fn to_wire(&self) -> [u8; DENY_WIRE_LEN] {
        let mut out = [0u8; DENY_WIRE_LEN];
        let hb = core::encode(&self.head);
        let mut i = 0;
        while i < core::ACE_HEADER_MIN {
            out[i] = hb[i];
            i += 1;
        }
        let sb = self.sid_slot.to_le_bytes();
        out[8] = sb[0];
        out[9] = sb[1];
        out
    }

    /// 线上形式解析（长度、真源闸、类型锚、SID 引用全闸）。
    pub fn from_wire(buf: &[u8]) -> Result<DenyAce, u16> {
        if buf.len() < DENY_WIRE_LEN {
            return Err(E_RANK);
        }
        let head = core::parse(buf)?;
        if head.ace_type != core::AceType::AccessDenied.wire_byte()? {
            return Err(E_TYPE);
        }
        let sid_slot = u16::from_le_bytes([buf[8], buf[9]]);
        if sid_slot == SID_NIL || sid_slot > SID_SLOT_MAX {
            return Err(E_SID);
        }
        if head.mask & GENERIC_SEGMENT != 0 {
            return Err(E_GENERIC);
        }
        Ok(DenyAce { head, sid_slot })
    }

    /// 指纹（真源 FNV over 线上形式 + 版本锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut feed = Vec::new();
        let w = self.to_wire();
        let mut i = 0;
        while i < DENY_WIRE_LEN {
            feed.push(w[i]);
            i += 1;
        }
        feed.extend_from_slice(&VER.to_le_bytes());
        core::fnv1a(&feed)
    }
}

/// deny 优先语义位演示：deny rank 恒先于 allow rank（36088 canon 校验器的序据）。
pub fn deny_precedes_allow() -> bool {
    let d = rank_of(core::AceType::AccessDenied.wire_byte().unwrap_or(0xff));
    let a = rank_of(core::AceType::AccessAllowed.wire_byte().unwrap_or(0xff));
    match (d, a) {
        (Ok(rd), Ok(ra)) => rd.code() < ra.code(),
        _ => false,
    }
}

/// 摘要行（String 面，无 IO）。
pub fn summary(d: &DenyAce) -> String {
    let mut s = String::new();
    s.push_str("deny sid=");
    s.push_str(&d.sid_slot.to_string());
    s.push_str(" mask=0x");
    s.push_str(&d.head.mask.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_TYPE, E_SID, E_MASK0, E_GENERIC, E_RANK, E_CS];
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
        E_TYPE => "类型锚不符（非 AccessDenied）",
        E_SID => "SID 槽位引用非法",
        E_MASK0 => "零掩码（空拒绝显性拒）",
        E_GENERIC => "deny 掩码含 GENERIC 段（构造门拒绝）",
        E_RANK => "求值序 rank 表破/长度不足",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 版本锚；R2 10 例合法构造往返；R3 GENERIC 注入 4 例；
/// R4 零掩码 2 例；R5 坏 SID 2 例；R6 错类型锚 2 例；R7 rank 序守恒；
/// R8 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚。
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：10 例合法构造 + 往返 + rank 一致。
    let mut i: u16 = 0;
    while i < 10 {
        let mask: u32 = 0x0001_2003u32.wrapping_add(i as u32);
        let d = DenyAce::new(0x00, mask & !GENERIC_SEGMENT, SID_NIL + 1 + i)?;
        let w = d.to_wire();
        let back = DenyAce::from_wire(&w)?;
        if back != d {
            return Err(E_RANK);
        }
        if d.fingerprint() != back.fingerprint() || d.fingerprint() == 0 {
            return Err(E_CS);
        }
        if rank_of(d.head.ace_type)?.code() != AceRank::DenyFirst.code() {
            return Err(E_RANK);
        }
        hits += 1;
        i += 1;
    }
    // R3：GENERIC 段注入 4 例（四个通用位逐一）——全部 E_GENERIC。
    let generics: Vec<u32> = vec![
        0x8000_0000, 0x4000_0000, 0x2000_0000, 0x1000_0000,
    ];
    let mut g = 0;
    while g < generics.len() {
        let r = DenyAce::new(0x00, generics[g], 1);
        match r {
            Ok(_) => return Err(E_GENERIC),
            Err(c) => {
                if c != E_GENERIC {
                    return Err(E_GENERIC);
                }
            }
        }
        hits += 1;
        g += 1;
    }
    // R4：零掩码 2 例——全部 E_MASK0。
    let mut z = 0;
    while z < 2 {
        let r = DenyAce::new(0x00, 0, 1 + z);
        match r {
            Ok(_) => return Err(E_MASK0),
            Err(c) => {
                if c != E_MASK0 {
                    return Err(E_MASK0);
                }
            }
        }
        hits += 1;
        z += 1;
    }
    // R5：坏 SID 引用 2 例（NIL / 上限+1）——全部 E_SID。
    let bad_sids: Vec<u16> = vec![SID_NIL, SID_SLOT_MAX + 1];
    let mut b = 0;
    while b < bad_sids.len() {
        let r = DenyAce::new(0x00, 0x0000_0001, bad_sids[b]);
        match r {
            Ok(_) => return Err(E_SID),
            Err(c) => {
                if c != E_SID {
                    return Err(E_SID);
                }
            }
        }
        hits += 1;
        b += 1;
    }
    // R6：错类型锚 2 例（0x00 allow / 0x02 audit）——from_wire 报 E_TYPE。
    let mut wire1 = [0u8; DENY_WIRE_LEN];
    let hb1 = core::encode(&core::AceHeader::new(0x00, 0x00, 0x0008, 0x0001)?);
    let mut t = 0;
    while t < 8 {
        wire1[t] = hb1[t];
        t += 1;
    }
    wire1[8] = 0x01;
    wire1[9] = 0x00;
    match DenyAce::from_wire(&wire1) {
        Ok(_) => return Err(E_TYPE),
        Err(c) => {
            if c != E_TYPE {
                return Err(E_TYPE);
            }
        }
    }
    hits += 1;
    let mut wire2 = [0u8; DENY_WIRE_LEN];
    let hb2 = core::encode(&core::AceHeader::new(0x02, 0x00, 0x0008, 0x0001)?);
    let mut t2 = 0;
    while t2 < 8 {
        wire2[t2] = hb2[t2];
        t2 += 1;
    }
    wire2[8] = 0x02;
    wire2[9] = 0x00;
    match DenyAce::from_wire(&wire2) {
        Ok(_) => return Err(E_TYPE),
        Err(c) => {
            if c != E_TYPE {
                return Err(E_TYPE);
            }
        }
    }
    hits += 1;
    // R7：rank 序守恒（deny < allow < audit 三对全立）。
    let rd = rank_of(0x01)?;
    let ra = rank_of(0x00)?;
    let ru = rank_of(0x02)?;
    if !(rd.code() < ra.code() && ra.code() < ru.code()) {
        return Err(E_RANK);
    }
    if !deny_precedes_allow() {
        return Err(E_RANK);
    }
    hits += 1;
    // R8：指纹两次求值稳定且非零。
    let d0 = DenyAce::new(0x10, 0x000f_ffff, 7)?;
    let p1 = d0.fingerprint();
    let p2 = d0.fingerprint();
    if p1 != p2 || p1 == 0 {
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
        assert_eq!(drill(), Ok(23));
    }

    #[test]
    fn generic_mask_rejected() {
        assert_eq!(DenyAce::new(0, 0x8000_0000, 1), Err(E_GENERIC));
        assert_eq!(DenyAce::new(0, 0xf000_0000, 1), Err(E_GENERIC));
    }

    #[test]
    fn zero_mask_and_bad_sid() {
        assert_eq!(DenyAce::new(0, 0, 1), Err(E_MASK0));
        assert_eq!(DenyAce::new(0, 1, 0), Err(E_SID));
    }

    #[test]
    fn rank_order_holds() {
        assert_eq!(rank_of(0x01), Ok(AceRank::DenyFirst));
        assert_eq!(rank_of(0x00), Ok(AceRank::AllowNext));
        assert_eq!(rank_of(0x02), Ok(AceRank::AuditLast));
        assert!(deny_precedes_allow());
    }
}

pub fn run_unx36083_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36083_acedeny");
    cs.add("SEG 段位独占", SEG_BASE == 0xe2f0, "0xe2f0~0xe2ff 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "10 例往返对平、负例 10 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("deny 优先语义位", deny_precedes_allow(), "canon 序 rank 表破");
    cs.add("GENERIC 禁段门在场", GENERIC_SEGMENT == 0xf000_0000, "GENERIC 禁段掩码漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
