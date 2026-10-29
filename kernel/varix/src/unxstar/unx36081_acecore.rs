//! UNX-F36081-J1 ACE 结构全字段（类型/掩码/SID/标志）与版本锚。
//! 判据：① AceHeader 四件（类型/标志/尺寸/掩码）编码-解析往返 11 类全对平；
//! ② 未知类型 12 例、坏尺寸 4 例、保留标志 3 例、保留掩码 1 例全部显性拒绝且码正确；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：B05 全批的 ACE 结构与常量真源——36082/83/84/85/86/87/88/89/92/96/98
//! 都从这里取 AceHeader/AceType/尺寸常量与 FNV 原语，不造第二套。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xe2d0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_TYPE: u16 = SEG_BASE + 2;   // ACE 类型不在闭集内
pub const E_SIZE: u16 = SEG_BASE + 3;   // 尺寸越界（<最小 或 >上限）
pub const E_FLAGS: u16 = SEG_BASE + 4;  // 保留标志位非零
pub const E_MASK: u16 = SEG_BASE + 5;   // 掩码保留段非零
pub const E_CS: u16 = SEG_BASE + 6;     // 校验和不符
pub const E_TABLE: u16 = SEG_BASE + 7;  // 类型定义表纪律破
pub const E_LAYOUT: u16 = SEG_BASE + 8; // 编码缓冲长度不足

/// 版本锚（B05 全批一致，36100 收口对拍）。
pub const VER: u32 = 1;

/// ACE 头最小尺寸：type(1)+flags(1)+size(2)+mask(4)。
pub const ACE_HEADER_MIN: usize = 8;
/// ACE 头最大承载（防注入式膨胀上限；对象 ACE 族可到 0xf000 量级）。
pub const ACE_SIZE_MAX: usize = 0xf000;
/// 保留标志位（仅 bit5；bit6/bit7 为审计成功/失败触发位，语义归 36084）。
pub const FLAG_RESERVED: u8 = 0x20;
/// 掩码保留段（bit25..27；三段细分归 36087）。
pub const MASK_RESERVED: u32 = 0x0e00_0000;

/// ACE 类型闭集（#[repr(u16)] 显式判别式，对标 Windows AceType 值域）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AceType {
    AccessAllowed        = 0x0000,
    AccessDenied         = 0x0001,
    SystemAudit          = 0x0002,
    SystemAlarm          = 0x0003,
    AccessAllowedObject  = 0x0005,
    AccessDeniedObject   = 0x0006,
    SystemAuditObject    = 0x0007,
    SystemAlarmObject    = 0x0008,
    SystemMandatoryLabel = 0x0011,
    SystemResourceAttr   = 0x0012,
    SystemScopedPolicy   = 0x0013,
}

impl AceType {
    /// 判别式显式取值（红线 4：code() 返回 self as u16）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 线上类型字节 → 闭集枚举；表外返回 None（调用方给 E_TYPE）。
    pub fn from_code(c: u16) -> Option<AceType> {
        match c {
            0x0000 => Some(AceType::AccessAllowed),
            0x0001 => Some(AceType::AccessDenied),
            0x0002 => Some(AceType::SystemAudit),
            0x0003 => Some(AceType::SystemAlarm),
            0x0005 => Some(AceType::AccessAllowedObject),
            0x0006 => Some(AceType::AccessDeniedObject),
            0x0007 => Some(AceType::SystemAuditObject),
            0x0008 => Some(AceType::SystemAlarmObject),
            0x0011 => Some(AceType::SystemMandatoryLabel),
            0x0012 => Some(AceType::SystemResourceAttr),
            0x0013 => Some(AceType::SystemScopedPolicy),
            _ => None,
        }
    }

    /// 线上类型字节（u8 窄化：闭集内全部 ≤ 0x13，窄化不丢真）。
    pub fn wire_byte(self) -> Result<u8, u16> {
        let c = self.code();
        if c > 0x00FF {
            return Err(E_TYPE);
        }
        Ok(c as u8)
    }

    /// 类型的人话名（诊断面 36095 复用）。
    pub fn name(self) -> &'static str {
        match self {
            AceType::AccessAllowed => "access-allowed",
            AceType::AccessDenied => "access-denied",
            AceType::SystemAudit => "system-audit",
            AceType::SystemAlarm => "system-alarm",
            AceType::AccessAllowedObject => "access-allowed-object",
            AceType::AccessDeniedObject => "access-denied-object",
            AceType::SystemAuditObject => "system-audit-object",
            AceType::SystemAlarmObject => "system-alarm-object",
            AceType::SystemMandatoryLabel => "mandatory-label",
            AceType::SystemResourceAttr => "resource-attribute",
            AceType::SystemScopedPolicy => "scoped-policy",
        }
    }
}

/// 类型定义表项（表纪律自检面）。
pub struct AceTypeDef {
    pub code: u16,
    pub name: &'static str,
}

/// 类型闭集表项数锚。
pub const ACE_TYPE_N: usize = 11;

/// 类型闭集定义表（与 AceType 判别式一一对应，顺序按码升序）。
pub const ACE_TYPE_TABLE: [AceTypeDef; ACE_TYPE_N] = [
    AceTypeDef { code: 0x0000, name: "access-allowed" },
    AceTypeDef { code: 0x0001, name: "access-denied" },
    AceTypeDef { code: 0x0002, name: "system-audit" },
    AceTypeDef { code: 0x0003, name: "system-alarm" },
    AceTypeDef { code: 0x0005, name: "access-allowed-object" },
    AceTypeDef { code: 0x0006, name: "access-denied-object" },
    AceTypeDef { code: 0x0007, name: "system-audit-object" },
    AceTypeDef { code: 0x0008, name: "system-alarm-object" },
    AceTypeDef { code: 0x0011, name: "mandatory-label" },
    AceTypeDef { code: 0x0012, name: "resource-attribute" },
    AceTypeDef { code: 0x0013, name: "scoped-policy" },
];

/// ACE 头四件：类型字节 / 标志字节 / 自述尺寸 / 访问掩码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AceHeader {
    pub ace_type: u8,
    pub flags: u8,
    pub size: u16,
    pub mask: u32,
}

impl AceHeader {
    /// 受控构造：四件全过闸才算合法头（真源唯一入口）。
    pub fn new(ace_type: u8, flags: u8, size: u16, mask: u32) -> Result<AceHeader, u16> {
        let h = AceHeader { ace_type, flags, size, mask };
        validate(&h)?;
        Ok(h)
    }

    /// 按 AceType 构造（掩码/标志由调用方给，仍走全闸）。
    pub fn of(t: AceType, flags: u8, mask: u32) -> Result<AceHeader, u16> {
        AceHeader::new(t.wire_byte()?, flags, ACE_HEADER_MIN as u16, mask)
    }
}

/// 头四件合法性闸：类型闭集 → 尺寸区间 → 保留标志 → 保留掩码（顺序固定）。
pub fn validate(h: &AceHeader) -> Result<(), u16> {
    if AceType::from_code(h.ace_type as u16).is_none() {
        return Err(E_TYPE);
    }
    if (h.size as usize) < ACE_HEADER_MIN || (h.size as usize) > ACE_SIZE_MAX {
        return Err(E_SIZE);
    }
    if h.flags & FLAG_RESERVED != 0 {
        return Err(E_FLAGS);
    }
    if h.mask & MASK_RESERVED != 0 {
        return Err(E_MASK);
    }
    Ok(())
}

/// 头编码为 8 字节线上形式（type|flags|size_le16|mask_le32）。
pub fn encode(h: &AceHeader) -> [u8; ACE_HEADER_MIN] {
    let mut out = [0u8; ACE_HEADER_MIN];
    out[0] = h.ace_type;
    out[1] = h.flags;
    let sz = h.size.to_le_bytes();
    out[2] = sz[0];
    out[3] = sz[1];
    let m = h.mask.to_le_bytes();
    out[4] = m[0];
    out[5] = m[1];
    out[6] = m[2];
    out[7] = m[3];
    out
}

/// 8 字节线上形式解析回头（长度不足 = E_LAYOUT，四件过闸）。
pub fn parse(buf: &[u8]) -> Result<AceHeader, u16> {
    if buf.len() < ACE_HEADER_MIN {
        return Err(E_LAYOUT);
    }
    let h = AceHeader {
        ace_type: buf[0],
        flags: buf[1],
        size: u16::from_le_bytes([buf[2], buf[3]]),
        mask: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
    };
    validate(&h)?;
    Ok(h)
}

/// FNV-1a（B05 全批共用哈希原语：36094 指纹/36098 金例库指纹/36100 防重）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 头指纹：线上 8 字节 + 版本锚联合摘要（自检面）。
pub fn header_fingerprint(h: &AceHeader) -> u64 {
    let mut feed = Vec::new();
    feed.extend_from_slice(&encode(h));
    feed.extend_from_slice(&VER.to_le_bytes());
    fnv1a(&feed)
}

/// 类型闭集表纪律自检：项数、码严格升序无重复、名字非空、与枚举互证。
pub fn type_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if ACE_TYPE_N != 11 {
        return Err(E_TABLE);
    }
    let mut prev: u16 = 0;
    let mut i = 0;
    while i < ACE_TYPE_N {
        let d = &ACE_TYPE_TABLE[i];
        if d.name.is_empty() {
            return Err(E_TABLE);
        }
        if AceType::from_code(d.code).is_none() {
            return Err(E_TYPE);
        }
        if i > 0 && d.code <= prev {
            return Err(E_TABLE);
        }
        prev = d.code;
        i += 1;
    }
    if fnv1a(&(VER.to_le_bytes())) == 0 {
        return Err(E_CS);
    }
    Ok(())
}

/// 诊断码闭集互异自检（8 个码两两不等）。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_TYPE, E_SIZE, E_FLAGS, E_MASK, E_CS, E_TABLE, E_LAYOUT];
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

/// 模块摘要行（String 面，无 IO，诊断上层拼装用）。
pub fn summary() -> String {
    let mut s = String::new();
    s.push_str("unx36081 acecore VER=");
    s.push_str(&VER.to_string());
    s.push_str(" types=");
    s.push_str(&ACE_TYPE_N.to_string());
    s
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_TYPE => "ACE 类型不在闭集内",
        E_SIZE => "尺寸越界",
        E_FLAGS => "保留标志位非零",
        E_MASK => "掩码保留段非零",
        E_CS => "校验和不符",
        E_TABLE => "类型定义表纪律破",
        E_LAYOUT => "编码缓冲长度不足",
        _ => "未知诊断码",
    }
}

/// 演练：R1 表纪律；R2 11 类编码-解析往返；R3 未知类型 12 例全拒且码正确；
/// R4 坏尺寸 4 例；R5 保留标志 3 例；R6 保留掩码 1 例；R7 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：类型表纪律 + 版本锚。
    type_integrity()?;
    hits += 1;
    // R2：11 类往返（new → encode → parse 相等；指纹两次求值一致）。
    let mut i = 0;
    while i < ACE_TYPE_N {
        let h = AceHeader::new(ACE_TYPE_TABLE[i].code as u8, 0x00, 0x0010, 0x0001_2003)?;
        let wire = encode(&h);
        let back = parse(&wire)?;
        if back != h {
            return Err(E_LAYOUT);
        }
        if header_fingerprint(&h) != header_fingerprint(&back) {
            return Err(E_CS);
        }
        hits += 1;
        i += 1;
    }
    // R3：未知类型 12 例——全部 E_TYPE（闭集红线）。
    let bads: Vec<u16> = vec![
        0x0004, 0x0009, 0x000a, 0x000b, 0x000c, 0x000d,
        0x000e, 0x000f, 0x0010, 0x0014, 0x00fe, 0x00ff,
    ];
    let mut k = 0;
    while k < bads.len() {
        let r = AceHeader::new(bads[k] as u8, 0x00, 0x0008, 0x0000_0001);
        match r {
            Ok(_) => return Err(E_TYPE), // 静默放行 = 红线
            Err(c) => {
                if c != E_TYPE {
                    return Err(E_TYPE);
                }
            }
        }
        hits += 1;
        k += 1;
    }
    // R4：坏尺寸 4 例（0 / 7 / 0xf001 / 0xFFFF）——全部 E_SIZE。
    let sizes: Vec<u16> = vec![0, 7, 0xf001, 0xffff];
    let mut s = 0;
    while s < sizes.len() {
        let r = AceHeader::new(0x00, 0x00, sizes[s], 0x0000_0001);
        match r {
            Ok(_) => return Err(E_SIZE),
            Err(c) => {
                if c != E_SIZE {
                    return Err(E_SIZE);
                }
            }
        }
        hits += 1;
        s += 1;
    }
    // R5：保留标志 3 例（0x20 / 0x60 / 0xa0，均含保留 bit5）——全部 E_FLAGS。
    let fl: Vec<u8> = vec![0x20, 0x60, 0xa0];
    let mut f = 0;
    while f < fl.len() {
        let r = AceHeader::new(0x00, fl[f], 0x0008, 0x0000_0001);
        match r {
            Ok(_) => return Err(E_FLAGS),
            Err(c) => {
                if c != E_FLAGS {
                    return Err(E_FLAGS);
                }
            }
        }
        hits += 1;
        f += 1;
    }
    // R6：保留掩码 1 例——E_MASK。
    let r = AceHeader::new(0x00, 0x00, 0x0008, MASK_RESERVED);
    match r {
        Ok(_) => return Err(E_MASK),
        Err(c) => {
            if c != E_MASK {
                return Err(E_MASK);
            }
        }
    }
    hits += 1;
    // R7：指纹两次求值稳定且非零（纯函数无漂移）。
    let h0 = AceHeader::new(0x00, 0x01, 0x0008, 0x000f_ffff)?;
    let p1 = header_fingerprint(&h0);
    let p2 = header_fingerprint(&h0);
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
        assert_eq!(drill(), Ok(33));
    }

    #[test]
    fn unknown_type_rejected() {
        assert_eq!(AceHeader::new(0xee, 0, 8, 1), Err(E_TYPE));
        assert_eq!(AceType::from_code(0x0004), None);
    }

    #[test]
    fn size_and_flag_gates() {
        assert_eq!(AceHeader::new(0x00, 0, 7, 1), Err(E_SIZE));
        assert_eq!(AceHeader::new(0x00, 0x20, 8, 1), Err(E_FLAGS));
    }

    #[test]
    fn roundtrip_and_summary() {
        let h = AceHeader::of(AceType::AccessAllowed, 0x10, 0x0001_2003).unwrap_or(AceHeader {
            ace_type: 0,
            flags: 0,
            size: 0,
            mask: 0,
        });
        let back = parse(&encode(&h)).unwrap_or(AceHeader {
            ace_type: 1,
            flags: 1,
            size: 1,
            mask: 1,
        });
        assert_eq!(back, h);
        assert!(summary().contains("acecore"));
    }
}

pub fn run_unx36081_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36081_acecore");
    cs.add("SEG 段位独占", SEG_BASE == 0xe2d0, "0xe2d0~0xe2df 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "往返 11 类对平、负例 20 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("类型闭集表 11 项", ACE_TYPE_N == 11, "闭集表项数锚不符");
    cs.add("版本锚 VER=1", VER == 1, "B05 全批版本锚必须一致");
    cs.add("诊断码 8 闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs.add("最小头尺寸锚 8", ACE_HEADER_MIN == 8, "四件头最小尺寸锚漂移");
    cs
}
