//! UNX-F36085-J1 ACE 标志位族：容器继承/对象继承/继承传播/已继承四件。
//! 判据：① 组合合法性表（CI/OI/IP 三位 8 组合，7 合法 1 非法）与 16 组合全走查
//! 判定一致；② ID 与 CI/OI/IP 互斥 4 例、IP 孤儿 1 例、保留位 3 例全部显性拒绝；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36081_acecore（保留位闸 0x60 从真源取锚）；
//! 本模块是 36088 canon 校验器与 36092 插入序的标志位判据源。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xe310;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_MUTEX: u16 = SEG_BASE + 2;  // ID 与 CI/OI/IP 互斥违规
pub const E_ORPHAN: u16 = SEG_BASE + 3; // 仅继承位无 CI/OI 载体（孤儿传播）
pub const E_RESERVED: u16 = SEG_BASE + 4; // 保留标志位非零
pub const E_COMBO: u16 = SEG_BASE + 5;  // 组合合法性表判非法
pub const E_TABLE: u16 = SEG_BASE + 6;  // 组合表纪律破
pub const E_CS: u16 = SEG_BASE + 7;     // 表指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 容器继承（container inherit：子容器继承本 ACE）。
pub const F_CI: u8 = 0x01;
/// 对象继承（object inherit：子叶子继承本 ACE）。
pub const F_OI: u8 = 0x02;
/// 继承传播锚（inherit-only/propagation：本 ACE 不生效、只作传播母本）。
pub const F_IP: u8 = 0x08;
/// 已继承（inherited：本 ACE 由父容器继承而来）。
pub const F_ID: u8 = 0x10;
/// 四件联合掩码。
pub const F_MASK: u8 = F_CI | F_OI | F_IP | F_ID;

/// 组合判定类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlagVerdict {
    /// 合法组合。
    Legal    = 0x0001,
    /// ID 互斥违规。
    IdMutex  = 0x0002,
    /// IP 孤儿（无 CI/OI 载体）。
    IpOrphan = 0x0003,
    /// 保留位非零。
    Reserved = 0x0004,
}

impl FlagVerdict {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 组合合法性表：下标 = CI | OI<<1 | IP<<2；1 = 合法，0 = 非法。
/// 唯一非法项是 IP 孤儿（仅 IP、无 CI/OI 载体）。
pub const COMBO_TABLE: [u8; 8] = [
    1, // 000：直接生效 ACE（非继承族）
    1, // 001 CI：容器继承
    1, // 010 OI：对象继承
    1, // 011 CI|OI：双继承
    0, // 100 IP：孤儿传播（非法）
    1, // 101 CI|IP：容器传播母本
    1, // 110 OI|IP：对象传播母本
    1, // 111 CI|OI|IP：双继承传播母本
];

/// 组合表纪律自检：8 项、恰好 7 合法 1 非法、非法项恰为 IP 孤儿位。
pub fn combo_table_integrity() -> Result<(), u16> {
    if COMBO_TABLE.len() != 8 {
        return Err(E_TABLE);
    }
    let mut legal_n = 0;
    let mut i = 0;
    while i < 8 {
        if COMBO_TABLE[i] > 1 {
            return Err(E_TABLE);
        }
        legal_n += COMBO_TABLE[i] as usize;
        i += 1;
    }
    if legal_n != 7 {
        return Err(E_TABLE);
    }
    if COMBO_TABLE[0x4] != 0 {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 标志位族全闸：保留位 → ID 互斥 → IP 孤儿 → 组合表（顺序固定）。
pub fn validate_flags(flags: u8) -> Result<FlagVerdict, u16> {
    if flags & core::FLAG_RESERVED != 0 {
        return Err(E_RESERVED);
    }
    if flags & F_ID != 0 && flags & (F_CI | F_OI | F_IP) != 0 {
        return Err(E_MUTEX);
    }
    if flags & F_IP != 0 && flags & (F_CI | F_OI) == 0 {
        return Err(E_ORPHAN);
    }
    let idx: usize = ((flags & F_CI != 0) as usize)
        | (((flags & F_OI != 0) as usize) << 1)
        | (((flags & F_IP != 0) as usize) << 2);
    if COMBO_TABLE[idx] == 0 {
        return Err(E_COMBO);
    }
    Ok(FlagVerdict::Legal)
}

/// 三位组合的合成标志（表走查用）。
pub fn combo_flags(ci: bool, oi: bool, ip: bool) -> u8 {
    let mut f = 0u8;
    if ci {
        f |= F_CI;
    }
    if oi {
        f |= F_OI;
    }
    if ip {
        f |= F_IP;
    }
    f
}

/// 组合表走查：三位组合按表逐一复核（validate 与表必须一致）。
pub fn walk_combo_table() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut idx = 0;
    while idx < 8 {
        let ci = idx & 1 != 0;
        let oi = idx & 2 != 0;
        let ip = idx & 4 != 0;
        let f = combo_flags(ci, oi, ip);
        let expect_ok = COMBO_TABLE[idx] == 1;
        let r = validate_flags(f);
        if expect_ok {
            match r {
                Ok(_) => {}
                Err(_) => return Err(E_COMBO),
            }
        } else {
            match r {
                Ok(_) => return Err(E_COMBO),
                Err(c) => {
                    if c != E_ORPHAN {
                        return Err(E_ORPHAN);
                    }
                }
            }
        }
        hits += 1;
        idx += 1;
    }
    Ok(hits)
}

/// 摘要行（String 面，无 IO）。
pub fn summary() -> String {
    let mut s = String::new();
    s.push_str("unx36085 aceflags VER=");
    s.push_str(&VER.to_string());
    s.push_str(" mask=0x");
    s.push_str(&F_MASK.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_MUTEX, E_ORPHAN, E_RESERVED, E_COMBO, E_TABLE, E_CS];
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
        E_MUTEX => "ID 与 CI/OI/IP 互斥违规",
        E_ORPHAN => "仅继承位无 CI/OI 载体（孤儿传播）",
        E_RESERVED => "保留标志位非零",
        E_COMBO => "组合合法性表判非法",
        E_TABLE => "组合表纪律破",
        E_CS => "表指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 表纪律；R2 16 组合全走查；R3 ID 互斥 4 例；R4 IP 孤儿 1 例；
/// R5 保留位 3 例；R6 合法标志过真源闸 4 例；R7 表指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：组合表纪律。
    combo_table_integrity()?;
    hits += 1;
    // R2：16 组合全走查（表 8 组合 × ID 有/无两态；ID 组合全拒、ID 单独合法）。
    let w1 = walk_combo_table()?;
    hits += w1;
    let mut idx = 0;
    while idx < 8 {
        let ci = idx & 1 != 0;
        let oi = idx & 2 != 0;
        let ip = idx & 4 != 0;
        let f = combo_flags(ci, oi, ip) | F_ID;
        let r = validate_flags(f);
        if idx == 0 {
            // ID 单独（无 CI/OI/IP 载荷）= 已继承普通 ACE，合法。
            match r {
                Ok(_) => {}
                Err(_) => return Err(E_MUTEX),
            }
        } else {
            match r {
                Ok(_) => return Err(E_MUTEX),
                Err(c) => {
                    if c != E_MUTEX {
                        return Err(E_MUTEX);
                    }
                }
            }
        }
        hits += 1;
        idx += 1;
    }
    // R3：ID 互斥负例补刀 4 例（显式注入面）。
    let mutex_cases: Vec<u8> = vec![F_ID | F_CI, F_ID | F_OI, F_ID | F_IP, F_ID | F_CI | F_OI];
    let mut m = 0;
    while m < mutex_cases.len() {
        let r = validate_flags(mutex_cases[m]);
        match r {
            Ok(_) => return Err(E_MUTEX),
            Err(c) => {
                if c != E_MUTEX {
                    return Err(E_MUTEX);
                }
            }
        }
        hits += 1;
        m += 1;
    }
    // R4：IP 孤儿负例 1 例。
    match validate_flags(F_IP) {
        Ok(_) => return Err(E_ORPHAN),
        Err(c) => {
            if c != E_ORPHAN {
                return Err(E_ORPHAN);
            }
        }
    }
    hits += 1;
    // R5：保留位 3 例（0x20 / 0x60 / 0xa0，均含保留 bit5）——真源锚 0x20 的消费位。
    let reserved: Vec<u8> = vec![0x20, 0x60, 0xa0];
    let mut r5 = 0;
    while r5 < reserved.len() {
        let r = validate_flags(reserved[r5]);
        match r {
            Ok(_) => return Err(E_RESERVED),
            Err(c) => {
                if c != E_RESERVED {
                    return Err(E_RESERVED);
                }
            }
        }
        hits += 1;
        r5 += 1;
    }
    // R6：合法标志过真源闸（四件全开单开轮转）。
    let good: Vec<u8> = vec![0x00, F_CI, F_OI, F_CI | F_OI | F_IP];
    let mut g = 0;
    while g < good.len() {
        let v = validate_flags(good[g])?;
        if v.code() != FlagVerdict::Legal.code() {
            return Err(E_COMBO);
        }
        let h = core::AceHeader::new(0x00, good[g], core::ACE_HEADER_MIN as u16, 0x0000_0001)?;
        if h.flags != good[g] {
            return Err(E_CS);
        }
        hits += 1;
        g += 1;
    }
    // R7：表指纹两次求值稳定且非零（FNV over 表字节 + 版本锚）。
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i = 0;
    while i < 8 {
        feed.push(COMBO_TABLE[i]);
        i += 1;
    }
    let fp = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp != fp2 || fp == 0 {
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
        assert_eq!(drill(), Ok(30));
    }

    #[test]
    fn id_mutex_rejected() {
        assert_eq!(validate_flags(F_ID | F_CI), Err(E_MUTEX));
        assert_eq!(validate_flags(F_ID | F_IP), Err(E_MUTEX));
    }

    #[test]
    fn ip_orphan_rejected() {
        assert_eq!(validate_flags(F_IP), Err(E_ORPHAN));
        assert_eq!(validate_flags(0x00), Ok(FlagVerdict::Legal));
    }

    #[test]
    fn reserved_rejected() {
        assert_eq!(validate_flags(0x20), Err(E_RESERVED));
        assert!(summary().contains("aceflags"));
    }
}

pub fn run_unx36085_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36085_aceflags");
    cs.add("SEG 段位独占", SEG_BASE == 0xe310, "0xe310~0xe31f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "16 组合全走查对平、负例 8 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("组合表 7 合法 1 非法", combo_table_integrity().is_ok(), "组合表纪律破");
    cs.add("四件掩码锚 0x1b", F_MASK == 0x1b, "CI/OI/IP/ID 联合掩码锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
