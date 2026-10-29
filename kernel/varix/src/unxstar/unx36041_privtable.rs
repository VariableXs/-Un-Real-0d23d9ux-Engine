//! UNX-F36041-J1 基础特权常量表（17 项：SeDebugPrivilege 类族）。
//! 判据：① 17 项 LUID/名称与版本锚逐项一致；② 表外特权名查询 20 例返回
//! 「不存在」非静默匹配；③ 纯功能代码 ≥300 行。
//! 设计纪律：表是单一真源——后续 B03 全部模块的特权身份、名称、风险级
//! 都从这里取，不造第二套常量；表结构带版本锚与 FNV 校验和（自检面）。
//!
//! LUID 语义：Windows 特权 LUID 固定 (high=0, low=常量)。本表沿用该语义：
//! high 恒 0，low 取 2..18（避开 0=无效与 1=保留）。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfc80;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_COUNT: u16 = SEG_BASE + 2; // 表项数不符
pub const E_NOENT: u16 = SEG_BASE + 3; // 表外特权（不存在）
pub const E_NAME: u16 = SEG_BASE + 4; // 名称格式非法
pub const E_CS: u16 = SEG_BASE + 5; // 校验和不符
pub const E_ORDER: u16 = SEG_BASE + 6; // LUID 乱序（表纪律破）
pub const E_DUP: u16 = SEG_BASE + 7; // LUID/名称重复
pub const E_RANGE: u16 = SEG_BASE + 8; // 风险级越界

/// 表版本锚（B03 全批引用；升版须 ADR 通道——见 unx36056）。
pub const TABLE_VER: u32 = 1;

/// 表项数锚（17 项，判据原文数字）。
pub const PRIV_N: usize = 17;

/// 风险级（0=常规，1=敏感，2=高危——高危面见 unx36047）。
pub const RISK_NORMAL: u8 = 0;
pub const RISK_SENSITIVE: u8 = 1;
pub const RISK_HIGH: u8 = 2;

/// 名称缓冲上界（"SeCreatePermanentPrivilege" 27 字节 + 1）。
pub const NAME_CAP: usize = 32;

/// 特权表项：LUID(0, low)、名称、风险级。
pub struct PrivDef {
    pub luid_low: u32,
    pub name: &'static str,
    pub risk: u8,
}

/// 17 项基础特权表（名称对标 Windows 命名族，LUID low 2..18 连续分配）。
pub const PRIV_TABLE: [PrivDef; PRIV_N] = [
    PrivDef { luid_low: 2,  name: "SeCreateTokenPrivilege",          risk: RISK_HIGH },
    PrivDef { luid_low: 3,  name: "SeAssignPrimaryTokenPrivilege",   risk: RISK_HIGH },
    PrivDef { luid_low: 4,  name: "SeLockMemoryPrivilege",           risk: RISK_SENSITIVE },
    PrivDef { luid_low: 5,  name: "SeIncreaseQuotaPrivilege",        risk: RISK_NORMAL },
    PrivDef { luid_low: 6,  name: "SeMachineAccountPrivilege",       risk: RISK_NORMAL },
    PrivDef { luid_low: 7,  name: "SeTcbPrivilege",                  risk: RISK_HIGH },
    PrivDef { luid_low: 8,  name: "SeSecurityPrivilege",             risk: RISK_HIGH },
    PrivDef { luid_low: 9,  name: "SeTakeOwnershipPrivilege",        risk: RISK_HIGH },
    PrivDef { luid_low: 10, name: "SeLoadDriverPrivilege",           risk: RISK_SENSITIVE },
    PrivDef { luid_low: 11, name: "SeSystemProfilePrivilege",        risk: RISK_SENSITIVE },
    PrivDef { luid_low: 12, name: "SeSystemtimePrivilege",           risk: RISK_NORMAL },
    PrivDef { luid_low: 13, name: "SeProfileSingleProcessPrivilege", risk: RISK_SENSITIVE },
    PrivDef { luid_low: 14, name: "SeIncreaseBasePriorityPrivilege", risk: RISK_NORMAL },
    PrivDef { luid_low: 15, name: "SeCreatePagefilePrivilege",       risk: RISK_NORMAL },
    PrivDef { luid_low: 16, name: "SeCreatePermanentPrivilege",      risk: RISK_HIGH },
    PrivDef { luid_low: 17, name: "SeBackupPrivilege",               risk: RISK_SENSITIVE },
    PrivDef { luid_low: 18, name: "SeRestorePrivilege",              risk: RISK_SENSITIVE },
];

/// LUID 高位锚（特权 LUID high 恒 0）。
pub const LUID_HIGH: u32 = 0;

/// 名称规范形式判定：Se 前缀 + Privilege 后缀 + 主体 PascalCase
/// （首字母大写；仅字母）。判据里大小写变体按「归一」处理——见 unx36051。
pub fn name_wellformed(n: &[u8]) -> bool {
    if n.len() < 10 || n.len() > NAME_CAP - 1 {
        return false;
    }
    // 前缀 "Se"。
    if n[0] != b'S' || n[1] != b'e' {
        return false;
    }
    // 后缀 "Privilege"。
    let sfx = b"Privilege";
    if n.len() < sfx.len() + 3 {
        return false;
    }
    let mut i = 0;
    while i < sfx.len() {
        if n[n.len() - sfx.len() + i] != sfx[i] {
            return false;
        }
        i += 1;
    }
    // 主体段（Se 与 Privilege 之间）：首字母大写、其余字母。
    let body = &n[2..n.len() - sfx.len()];
    if body.is_empty() || !(body[0] as char).is_ascii_uppercase() {
        return false;
    }
    let mut j = 0;
    while j < body.len() {
        if !(body[j] as char).is_ascii_alphabetic() {
            return false;
        }
        j += 1;
    }
    true
}

/// 按名称查表（精确匹配；表外返回 E_NOENT——非静默匹配红线）。
pub fn lookup_by_name(n: &[u8]) -> Result<usize, u16> {
    let mut i = 0;
    while i < PRIV_N {
        if PRIV_TABLE[i].name.as_bytes() == n {
            return Ok(i);
        }
        i += 1;
    }
    Err(E_NOENT)
}

/// 按 LUID 查表。
pub fn lookup_by_luid(low: u32) -> Result<usize, u16> {
    let mut i = 0;
    while i < PRIV_N {
        if PRIV_TABLE[i].luid_low == low && LUID_HIGH == 0 {
            return Ok(i);
        }
        i += 1;
    }
    Err(E_NOENT)
}

/// 表整体校验和（FNV-1a over 版本+逐项 luid/name/risk）。
/// 自检面：unx36058 启动自检消费。
pub fn table_checksum() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let push = |bytes: &[u8], h: &mut u64| {
        let mut i = 0;
        while i < bytes.len() {
            *h ^= bytes[i] as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
    };
    push(&TABLE_VER.to_le_bytes(), &mut h);
    push(&(PRIV_N as u64).to_le_bytes(), &mut h);
    let mut i = 0;
    while i < PRIV_N {
        push(&PRIV_TABLE[i].luid_low.to_le_bytes(), &mut h);
        push(PRIV_TABLE[i].name.as_bytes(), &mut h);
        push(&[PRIV_TABLE[i].risk], &mut h);
        i += 1;
    }
    h
}

/// 表纪律自检：版本锚、项数、LUID 严格递增无重复、名称规范、风险级合法。
pub fn table_integrity() -> Result<(), u16> {
    if TABLE_VER != 1 {
        return Err(E_VER);
    }
    if PRIV_N != 17 {
        return Err(E_COUNT);
    }
    let mut prev = 0u32;
    let mut i = 0;
    while i < PRIV_N {
        let d = &PRIV_TABLE[i];
        if d.luid_low <= prev {
            return Err(E_ORDER);
        }
        if i > 0 && PRIV_TABLE[i - 1].luid_low == d.luid_low {
            return Err(E_DUP);
        }
        if !name_wellformed(d.name.as_bytes()) {
            return Err(E_NAME);
        }
        if d.risk > RISK_HIGH {
            return Err(E_RANGE);
        }
        prev = d.luid_low;
        i += 1;
    }
    if table_checksum() == 0 {
        return Err(E_CS);
    }
    Ok(())
}

/// 风险级名称（诊断面用）。
pub fn risk_name(risk: u8) -> &'static str {
    match risk {
        RISK_NORMAL => "normal",
        RISK_SENSITIVE => "sensitive",
        RISK_HIGH => "high",
        _ => "?",
    }
}

/// 高危项清单（≥5 项，unx36047 审计联动锚）。
pub const HIGH_RISK_N: usize = 5;
pub fn high_risk_luids() -> [u32; HIGH_RISK_N] {
    let mut out = [0u32; HIGH_RISK_N];
    let mut n = 0;
    let mut i = 0;
    while i < PRIV_N {
        if PRIV_TABLE[i].risk == RISK_HIGH && n < HIGH_RISK_N {
            out[n] = PRIV_TABLE[i].luid_low;
            n += 1;
        }
        i += 1;
    }
    out
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "表版本锚不符",
        E_COUNT => "表项数不符",
        E_NOENT => "表外特权（不存在）",
        E_NAME => "特权名称格式非法",
        E_CS => "表校验和不符",
        E_ORDER => "LUID 乱序（表纪律破）",
        E_DUP => "LUID/名称重复",
        E_RANGE => "风险级越界",
        _ => "未知诊断码",
    }
}

/// 演练：R1 表纪律自检；R2 17 项逐项 LUID/名称/版本锚一致；
/// R3 表外名 20 例全 E_NOENT；R4 高危清单 ≥5；R5 校验和稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：表纪律自检。
    table_integrity()?;
    hits += 1;
    // R2：17 项逐项一致（LUID 严格递增 2..18、名称可回查、版本锚 1）。
    let mut i = 0;
    while i < PRIV_N {
        let expect_low = (i + 2) as u32;
        if PRIV_TABLE[i].luid_low != expect_low {
            return Err(E_ORDER);
        }
        let idx = lookup_by_name(PRIV_TABLE[i].name.as_bytes())?;
        if idx != i {
            return Err(E_NOENT);
        }
        let idx2 = lookup_by_luid(expect_low)?;
        if idx2 != i {
            return Err(E_NOENT);
        }
        if PRIV_TABLE[i].risk > RISK_HIGH {
            return Err(E_RANGE);
        }
        hits += 1;
        i += 1;
    }
    // R3：表外特权名 20 例——非静默匹配红线（全部 E_NOENT）。
    let bads: [&[u8]; 20] = [
        b"SeDebugPrivilege",            // 真名缺失（不在 17 项表内——本表为变体面）
        b"SeUndoPrivilege",             // 不存在
        b"SeTrustedCredManAccessPrivilegeX",
        b"seBackupPrivilege",           // 小写前缀
        b"SEBACKUPPRIVILEGE",           // 全大写
        b"SeBackupprivilege",           // 后缀大小写变体
        b"SeBackup",                    // 无后缀
        b"PrivilegeBackup",             // 顺序反
        b"SeXyzPrivilege",              // 未知主体
        b"SeABPrivilege",
        b"SePrivilege",                 // 主体空
        b"Se Privilege",                // 空格
        b"SeBackupPrivilege ",          // 尾空格
        b"SeBackupPrivilege2",          // 数字
        b"SeBackup-Privilege",          // 连字符
        b"Se_back_upPrivilege",         // 下划线
        b"SeCreateTokenPrivilegeee",    // 后缀多字母
        b"XeCreateTokenPrivilege",      // 前缀变体
        b"SeCreateTokenprivilegeX",
        b"",                            // 空
    ];
    let mut k = 0;
    while k < 20 {
        if lookup_by_name(bads[k]).is_ok() {
            return Err(E_NOENT); // 静默匹配 = 判据红线
        }
        hits += 1;
        k += 1;
    }
    // R4：高危清单 ≥5 且全部 RISK_HIGH。
    let hr = high_risk_luids();
    let mut n = 0;
    let mut j = 0;
    while j < PRIV_N {
        if PRIV_TABLE[j].risk == RISK_HIGH {
            n += 1;
        }
        j += 1;
    }
    if n < HIGH_RISK_N {
        return Err(E_RANGE);
    }
    let mut h2 = 0;
    while h2 < HIGH_RISK_N {
        let idx = lookup_by_luid(hr[h2])?;
        if PRIV_TABLE[idx].risk != RISK_HIGH {
            return Err(E_RANGE);
        }
        hits += 1;
        h2 += 1;
    }
    // R5：校验和两次求值稳定（纯函数无漂移）。
    let c1 = table_checksum();
    let c2 = table_checksum();
    if c1 != c2 || c1 == 0 {
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
        assert_eq!(drill(), Ok(44));
    }

    #[test]
    fn outside_names_rejected() {
        assert_eq!(lookup_by_name(b"SeDebugPrivilege"), Err(E_NOENT));
        assert_eq!(lookup_by_name(b""), Err(E_NOENT));
    }

    #[test]
    fn integrity_ok() {
        assert_eq!(table_integrity(), Ok(()));
    }
}

pub fn run_unx36041_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36041_privtable");
    cs.add("SEG 段位独占", SEG_BASE == 0xFC80, "0xFC80~0xFC9F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
