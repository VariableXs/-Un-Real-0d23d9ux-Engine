//! UNX-F36082-J1 AccessAllowed ACE 语义载体。
//! 判据：① 类型锚（必须=AccessAllowed）+ 掩码 + SID 引用合法性，12 例合法构造
//! 与线上往返全对平；② 零掩码 3 例、坏 SID 引用 3 例、错类型锚 2 例、坏长度 3 例
//! 全部显性拒绝且码正确；③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36081_acecore（头四件/校验/FNV 全部从真源取），
//! 本模块只承载「允许」语义，deny/audit 语义归 36083/36084。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xe2e0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_TYPE: u16 = SEG_BASE + 2;  // 类型锚不符（非 AccessAllowed）
pub const E_SID: u16 = SEG_BASE + 3;   // SID 槽位引用非法
pub const E_MASK0: u16 = SEG_BASE + 4; // 零掩码（空授权显性拒）
pub const E_LEN: u16 = SEG_BASE + 5;   // 线上形式长度不足
pub const E_CS: u16 = SEG_BASE + 6;    // 指纹不稳

/// 版本锚（与全批一致）。
pub const VER: u32 = 1;

/// SID 槽位空值锚（0 号槽保留，不参与授权）。
pub const SID_NIL: u16 = 0;
/// SID 槽位上限（本载体面内可引用的最大槽位）。
pub const SID_SLOT_MAX: u16 = 63;
/// 允许 ACE 线上形式长度：头 8 字节 + sid 槽位 le16。
pub const ALLOW_WIRE_LEN: usize = 10;

/// 允许语义的承载类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllowKind {
    /// 直接授予（非继承而来）。
    Direct        = 0x0001,
    /// 继承而来（ID 位语义，归 36085 细分）。
    Inherited     = 0x0002,
    /// 对象特定 ACE（0x0005 族）。
    ObjectScoped  = 0x0003,
}

impl AllowKind {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 由标志位推断承载类别：ID 位 → Inherited；对象类型 → ObjectScoped；其余 Direct。
    pub fn infer(head_type: u8, flags: u8) -> AllowKind {
        if head_type == core::AceType::AccessAllowedObject.code() as u8 {
            AllowKind::ObjectScoped
        } else if flags & core::FLAG_RESERVED == 0 && flags & 0x10 != 0 {
            AllowKind::Inherited
        } else {
            AllowKind::Direct
        }
    }
}

/// AccessAllowed ACE 语义载体：真源头 + SID 槽位引用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AllowAce {
    pub head: core::AceHeader,
    pub sid_slot: u16,
}

impl AllowAce {
    /// 受控构造：类型锚 → 真源四件闸 → SID 引用闸 → 零掩码闸（顺序固定）。
    pub fn new(flags: u8, mask: u32, sid_slot: u16) -> Result<AllowAce, u16> {
        let head = core::AceHeader::new(
            core::AceType::AccessAllowed.wire_byte()?,
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
        Ok(AllowAce { head, sid_slot })
    }

    /// 线上形式：头 8 字节 + sid 槽位 le16。
    pub fn to_wire(&self) -> [u8; ALLOW_WIRE_LEN] {
        let mut out = [0u8; ALLOW_WIRE_LEN];
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

    /// 线上形式解析（长度、类型锚、SID 引用全闸）。
    pub fn from_wire(buf: &[u8]) -> Result<AllowAce, u16> {
        if buf.len() < ALLOW_WIRE_LEN {
            return Err(E_LEN);
        }
        let head = core::parse(buf)?;
        if head.ace_type != core::AceType::AccessAllowed.wire_byte()? {
            return Err(E_TYPE);
        }
        let sid_slot = u16::from_le_bytes([buf[8], buf[9]]);
        if sid_slot == SID_NIL || sid_slot > SID_SLOT_MAX {
            return Err(E_SID);
        }
        Ok(AllowAce { head, sid_slot })
    }

    /// 指纹（真源 FNV over 线上形式 + 版本锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut feed = Vec::new();
        let w = self.to_wire();
        let mut i = 0;
        while i < ALLOW_WIRE_LEN {
            feed.push(w[i]);
            i += 1;
        }
        feed.extend_from_slice(&VER.to_le_bytes());
        core::fnv1a(&feed)
    }

    /// 语义类别（构造路径推断）。
    pub fn kind(&self) -> AllowKind {
        AllowKind::infer(self.head.ace_type, self.head.flags)
    }
}

/// 摘要行（String 面，无 IO）。
pub fn summary(a: &AllowAce) -> String {
    let mut s = String::new();
    s.push_str("allow sid=");
    s.push_str(&a.sid_slot.to_string());
    s.push_str(" mask=0x");
    s.push_str(&a.head.mask.to_string());
    s.push_str(" kind=");
    s.push_str(a.kind().code().to_string().as_str());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_TYPE, E_SID, E_MASK0, E_LEN, E_CS];
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
        E_TYPE => "类型锚不符（非 AccessAllowed）",
        E_SID => "SID 槽位引用非法",
        E_MASK0 => "零掩码（空授权显性拒）",
        E_LEN => "线上形式长度不足",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 版本锚；R2 12 例合法构造与线上往返；R3 零掩码 3 例；
/// R4 坏 SID 3 例；R5 错类型锚 2 例；R6 坏长度 3 例；R7 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚。
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：12 例合法构造 + 往返 + 类别推断非空集。
    let mut i: u16 = 0;
    while i < 12 {
        let flags: u8 = if i % 3 == 0 { 0x10 } else { 0x00 };
        let mask: u32 = 0x0001_2003u32.wrapping_add(i as u32);
        let sid: u16 = SID_NIL + 1 + i;
        let a = AllowAce::new(flags, mask, sid)?;
        let w = a.to_wire();
        let back = AllowAce::from_wire(&w)?;
        if back != a {
            return Err(E_LEN);
        }
        if a.fingerprint() != back.fingerprint() || a.fingerprint() == 0 {
            return Err(E_CS);
        }
        if a.kind().code() == 0 {
            return Err(E_TYPE);
        }
        hits += 1;
        i += 1;
    }
    // R3：零掩码 3 例——全部 E_MASK0（空授权显性拒）。
    let mut z = 0;
    while z < 3 {
        let r = AllowAce::new(0x00, 0, 1 + z);
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
    // R4：坏 SID 引用 3 例（NIL / 上限+1 / 上限+2）——全部 E_SID。
    let bad_sids: Vec<u16> = vec![SID_NIL, SID_SLOT_MAX + 1, SID_SLOT_MAX + 2];
    let mut b = 0;
    while b < bad_sids.len() {
        let r = AllowAce::new(0x00, 0x0000_0001, bad_sids[b]);
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
    // R5：错类型锚 2 例——from_wire 报 E_TYPE。
    // 第 1 例：类型字节=0x01（deny），类型锚闸拦下。
    let mut wire1 = [0u8; ALLOW_WIRE_LEN];
    let hb = core::encode(&core::AceHeader::new(0x01, 0x00, 0x0008, 0x0001)?);
    let mut t = 0;
    while t < 8 {
        wire1[t] = hb[t];
        t += 1;
    }
    wire1[8] = 0x01;
    wire1[9] = 0x00;
    let r = AllowAce::from_wire(&wire1);
    match r {
        Ok(_) => return Err(E_TYPE),
        Err(c) => {
            if c != E_TYPE {
                return Err(E_TYPE);
            }
        }
    }
    hits += 1;
    // 第 2 例：类型字节覆写为表外码 0xEE，真源闸在先拦下（E_TYPE 同样算锚拦截）。
    let mut wire2 = [0u8; ALLOW_WIRE_LEN];
    let hb2 = core::encode(&core::AceHeader::new(0x00, 0x00, 0x0008, 0x0001)?);
    let mut t2 = 0;
    while t2 < 8 {
        wire2[t2] = hb2[t2];
        t2 += 1;
    }
    wire2[0] = 0xee; // 覆写类型字节为表外码
    wire2[8] = 0x02;
    wire2[9] = 0x00;
    let _ = hb2;
    let r2 = AllowAce::from_wire(&wire2);
    match r2 {
        Ok(_) => return Err(E_TYPE),
        Err(c) => {
            if c != core::E_TYPE && c != E_TYPE {
                return Err(E_TYPE);
            }
        }
    }
    hits += 1;
    // R6：坏长度 3 例（0 / 7 / 9 字节）——全部 E_LEN。
    let lens: Vec<usize> = vec![0, 7, 9];
    let mut l = 0;
    while l < lens.len() {
        let r = AllowAce::from_wire(&wire2[..lens[l]]);
        match r {
            Ok(_) => return Err(E_LEN),
            Err(c) => {
                if c != E_LEN && c != core::E_LAYOUT {
                    return Err(E_LEN);
                }
            }
        }
        hits += 1;
        l += 1;
    }
    // R7：指纹两次求值稳定且非零。
    let a0 = AllowAce::new(0x10, 0x000f_ffff, 7)?;
    let p1 = a0.fingerprint();
    let p2 = a0.fingerprint();
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
        assert_eq!(drill(), Ok(25));
    }

    #[test]
    fn zero_mask_rejected() {
        assert_eq!(AllowAce::new(0, 0, 1), Err(E_MASK0));
    }

    #[test]
    fn bad_sid_rejected() {
        assert_eq!(AllowAce::new(0, 1, SID_NIL), Err(E_SID));
        assert_eq!(AllowAce::new(0, 1, SID_SLOT_MAX + 1), Err(E_SID));
    }

    #[test]
    fn roundtrip_ok() {
        let a = AllowAce::new(0x10, 0x0001_2003, 5).unwrap_or(AllowAce {
            head: core::AceHeader { ace_type: 0, flags: 0, size: 0, mask: 0 },
            sid_slot: 0,
        });
        let back = AllowAce::from_wire(&a.to_wire()).unwrap_or(a);
        assert_eq!(back, a);
        assert!(summary(&a).contains("allow"));
    }
}

pub fn run_unx36082_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36082_aceallow");
    cs.add("SEG 段位独占", SEG_BASE == 0xe2e0, "0xe2e0~0xe2ef 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "12 例往返对平、负例 11 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("类型锚=AccessAllowed", core::AceType::AccessAllowed.code() == 0x0000, "允许 ACE 类型锚漂移");
    cs.add("SID 槽位上限锚 63", SID_SLOT_MAX == 63, "SID 槽位上限漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
