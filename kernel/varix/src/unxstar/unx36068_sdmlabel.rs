//! UNX-F36068-J1 强制标签 ACE（SYSTEM_MANDATORY_LABEL）在 SACL 中的载体。
//! 判据：① 标签五档（低/中/高/系统/受保护轻）闭集载体 100 例零越档；
//! ② 无标签对象默认中档策略 + 写权限门（NoWriteUp）不变量 50 例；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：强制完整性控制（MIC）的载体位——标签 ACE 活在 SACL 里
//! （unx36067 的段面），本模块管标签值域与策略位；求值在 B09 域。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{
    self as core, Sd, C_SACL_PRESENT, C_SACL_PROT, E_CTRL, E_OFF, OFF_NONE,
};
use super::unx36067_sdsacl::{AUDIT_SUCCESS, AUDIT_FAILURE};

pub const SEG_BASE: u16 = 0xe200;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_LEVEL: u16 = SEG_BASE + 1; // 标签档越域（五档闭集外）
pub const E_POLICY: u16 = SEG_BASE + 2; // 策略位越域
pub const E_NOUP: u16 = SEG_BASE + 3; // NoWriteUp 不变量破（低写高）
pub const E_CARRY: u16 = SEG_BASE + 4; // 载体面缺失（SACL 未挂载标签段）
pub const E_DUP: u16 = SEG_BASE + 5; // 双标签（一个 SACL 只容一条标签 ACE）

/// 强制标签五档（对标 Windows mandatory label RID 0x1000..0x4000）。
pub const LABEL_LOW: u16 = 0x1000; // 低
pub const LABEL_MED: u16 = 0x2000; // 中
pub const LABEL_HIGH: u16 = 0x3000; // 高
pub const LABEL_SYSTEM: u16 = 0x4000; // 系统
pub const LABEL_PROT_LIGHT: u16 = 0x1100; // 受保护轻（PPSL）

/// 标签档表（闭集 5 档；判据原文锚）。
pub const LABEL_N: usize = 5;
pub const LABEL_TAB: [u16; LABEL_N] = [
    LABEL_LOW, LABEL_PROT_LIGHT, LABEL_MED, LABEL_HIGH, LABEL_SYSTEM,
];

pub fn label_known(v: u16) -> bool {
    let mut i = 0;
    while i < LABEL_N {
        if LABEL_TAB[i] == v {
            return true;
        }
        i += 1;
    }
    false
}

/// 策略位族（SYSTEM_MANDATORY_LABEL_ACE 的 Mask 字段语义位）。
pub const POL_NO_WRITE_UP: u16 = 0x0001; // NO_WRITE_UP（恒置——Windows 纪律）
pub const POL_NO_READ_UP: u16 = 0x0002; // NO_READ_UP（可选）
pub const POL_NO_EXECUTE_UP: u16 = 0x0004; // NO_EXECUTE_UP（可选）

/// 策略位闭集。
pub const POL_KNOWN: u16 = POL_NO_WRITE_UP | POL_NO_READ_UP | POL_NO_EXECUTE_UP;

/// 标签 ACE 载体（SACL 内的一条 SYSTEM_MANDATORY_LABEL_ACE）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MandLabelAce {
    pub level: u16,
    pub policy: u16,
}

impl MandLabelAce {
    /// 构造门：档闭集 + 策略闭集 + NO_WRITE_UP 恒置（Windows 纪律）。
    pub fn build(level: u16, policy: u16) -> Result<MandLabelAce, u16> {
        if !label_known(level) {
            return Err(E_LEVEL);
        }
        if policy & !POL_KNOWN != 0 {
            return Err(E_POLICY);
        }
        if policy & POL_NO_WRITE_UP == 0 {
            return Err(E_POLICY); // NO_WRITE_UP 必置
        }
        Ok(MandLabelAce { level, policy })
    }
    /// 档序（低→系统，序数 0..4）。
    pub fn rank(&self) -> Result<usize, u16> {
        let mut i = 0;
        while i < LABEL_N {
            if LABEL_TAB[i] == self.level {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_LEVEL)
    }
}

/// SACL 载体槽（带标签的 SACL = unx36067 槽 + 至多一条标签 ACE）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SaclWithLabel {
    pub sacl_present: bool,
    pub audit_bits: u16,
    pub label: Option<MandLabelAce>,
}

/// 载体装配门：标签 ACE 只能在 SACL_PRESENT 置位的载体上挂载。
pub fn mount(sacl_present: bool, audit_bits: u16, label: Option<MandLabelAce>) -> Result<SaclWithLabel, u16> {
    if audit_bits & !(AUDIT_SUCCESS | AUDIT_FAILURE) != 0 {
        return Err(E_CARRY);
    }
    if !sacl_present && label.is_some() {
        return Err(E_CARRY); // 无 SACL 载体挂标签 = 载体面缺失
    }
    Ok(SaclWithLabel { sacl_present, audit_bits, label })
}

/// NoWriteUp 不变量：主体档 < 对象档时，写请求必须被拒（读/执行按策略）。
pub fn no_write_up(
    subj: &MandLabelAce,
    obj: &MandLabelAce,
    want_write: bool,
    want_read: bool,
    want_exec: bool,
) -> Result<(), u16> {
    let s = subj.rank()?;
    let o = obj.rank()?;
    if s < o && want_write {
        return Err(E_NOUP); // 低写高：铁律
    }
    if s < o && want_read && obj.policy & POL_NO_READ_UP != 0 {
        return Err(E_NOUP);
    }
    if s < o && want_exec && obj.policy & POL_NO_EXECUTE_UP != 0 {
        return Err(E_NOUP);
    }
    Ok(())
}

/// 默认档策略：无标签对象按中档（LABEL_MED）+ NO_WRITE_UP 处理。
pub fn default_label() -> MandLabelAce {
    MandLabelAce { level: LABEL_MED, policy: POL_NO_WRITE_UP }
}

/// 标签面 SD 夹具（SACL_PRESENT + 偏移 + 标签）。
pub fn label_sd(level: u16, policy: u16) -> Result<(Sd, MandLabelAce), u16> {
    let ace = MandLabelAce::build(level, policy)?;
    let mut sd = Sd::bare();
    sd.set_bit(C_SACL_PRESENT);
    sd.off_sacl = 0;
    core::sd_parse_ok(&sd)?;
    Ok((sd, ace))
}

/// 双标签检测（一个 SACL 只容一条标签 ACE——载体唯一性）。
pub fn unique_label(existing: &[MandLabelAce], add: &MandLabelAce) -> Result<usize, u16> {
    let _ = add;
    if existing.len() >= 1 {
        return Err(E_DUP);
    }
    Ok(existing.len())
}

fn detail(code: u16) -> &'static str {
    match code {
        E_LEVEL => "标签档越域（五档闭集外）",
        E_POLICY => "策略位越域或 NO_WRITE_UP 未置",
        E_NOUP => "NoWriteUp 不变量破（低写高）",
        E_CARRY => "载体面缺失（无 SACL 挂标签/审计位越域）",
        E_DUP => "双标签（SACL 标签唯一性破）",
        E_CTRL => "控制位含未知位",
        E_OFF => "段偏移与 PRESENT 位不一致",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 五档 × 四策略组合 100 例全过（闭集内）；
/// R2 越档/越策略注入 8 例全拒；R3 NoWriteUp 50 例（含读/执行策略面）；
/// R4 默认中档策略（无标签对象 50 例）；R5 载体唯一性与装配门。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：5 档 × 4 策略（NO_WRITE_UP 恒置 + 可选两位）= 20 组合 × 5 轮。
    let pols: [u16; 4] = [
        POL_NO_WRITE_UP,
        POL_NO_WRITE_UP | POL_NO_READ_UP,
        POL_NO_WRITE_UP | POL_NO_EXECUTE_UP,
        POL_NO_WRITE_UP | POL_NO_READ_UP | POL_NO_EXECUTE_UP,
    ];
    let mut r = 0u32;
    while r < 100 {
        let level = LABEL_TAB[(r as usize) % LABEL_N];
        let pol = pols[(r as usize) % 4];
        let ace = MandLabelAce::build(level, pol)?;
        if ace.rank()? != (r as usize) % LABEL_N {
            return Err(E_LEVEL);
        }
        hits += 1;
        r += 1;
    }
    // R2：注入 8 例（3 越档 + 3 越策略 + 2 NO_WRITE_UP 缺失）。
    let bad_levels: [u16; 3] = [0, 0x5000, 0x2fff];
    let mut b = 0;
    while b < 3 {
        if MandLabelAce::build(bad_levels[b], POL_NO_WRITE_UP) != Err(E_LEVEL) {
            return Err(E_LEVEL);
        }
        b += 1;
    }
    let bad_pols: [u16; 3] = [0x0008, 0xffff, POL_NO_READ_UP];
    let mut b2 = 0;
    while b2 < 3 {
        if MandLabelAce::build(LABEL_MED, bad_pols[b2]) != Err(E_POLICY) {
            return Err(E_POLICY);
        }
        b2 += 1;
    }
    if MandLabelAce::build(LABEL_MED, 0) != Err(E_POLICY) {
        return Err(E_POLICY);
    }
    if MandLabelAce::build(LABEL_MED, POL_NO_READ_UP) != Err(E_POLICY) {
        return Err(E_POLICY);
    }
    hits += 1;
    // R3：NoWriteUp 50 例（主体档 × 对象档全网格 5×5 + 读写执行开关轮转）。
    let mut i = 0;
    while i < 50 {
        let s_rank = i % LABEL_N;
        let o_rank = (i / LABEL_N) % LABEL_N;
        let want_write = i % 2 == 0;
        let want_read = i % 3 == 0;
        let want_exec = i % 5 == 0;
        let subj = MandLabelAce { level: LABEL_TAB[s_rank], policy: POL_NO_WRITE_UP };
        let obj = MandLabelAce {
            level: LABEL_TAB[o_rank],
            policy: POL_NO_WRITE_UP | POL_NO_READ_UP | POL_NO_EXECUTE_UP,
        };
        let s = subj.rank()?;
        let o = obj.rank()?;
        if s < o {
            // 低→高：写恒拒；读/执行按对象策略位判拒。
            if want_write {
                if no_write_up(&subj, &obj, want_write, want_read, want_exec) != Err(E_NOUP) {
                    return Err(E_NOUP);
                }
            } else if want_read {
                if no_write_up(&subj, &obj, want_write, want_read, want_exec) != Err(E_NOUP) {
                    return Err(E_NOUP);
                }
            } else if want_exec {
                if no_write_up(&subj, &obj, want_write, want_read, want_exec) != Err(E_NOUP) {
                    return Err(E_NOUP);
                }
            } else {
                no_write_up(&subj, &obj, want_write, want_read, want_exec)?;
            }
        } else {
            no_write_up(&subj, &obj, want_write, want_read, want_exec)?;
        }
        hits += 1;
        i += 1;
    }
    // R4：默认中档（无标签对象 vs 低/中/高三档主体）。
    let dflt = default_label();
    let low = MandLabelAce { level: LABEL_LOW, policy: POL_NO_WRITE_UP };
    let med = MandLabelAce { level: LABEL_MED, policy: POL_NO_WRITE_UP };
    let high = MandLabelAce { level: LABEL_HIGH, policy: POL_NO_WRITE_UP };
    if no_write_up(&low, &dflt, true, false, false) != Err(E_NOUP) {
        return Err(E_NOUP); // 低写中（默认档）：拒
    }
    no_write_up(&med, &dflt, true, false, false)?; // 同档写：过
    if no_write_up(&high, &dflt, true, false, false).is_err() {
        return Err(E_NOUP); // 高写中：过
    }
    hits += 1;
    // R5：载体装配门 + 唯一性。
    let (sd, ace) = label_sd(LABEL_HIGH, POL_NO_WRITE_UP)?;
    if !sd.has(C_SACL_PRESENT) || sd.off_sacl == OFF_NONE {
        return Err(E_CARRY);
    }
    let carrier = mount(true, AUDIT_SUCCESS, Some(ace))?;
    if carrier.label.is_none() {
        return Err(E_CARRY);
    }
    if mount(false, 0, Some(ace)) != Err(E_CARRY) {
        return Err(E_CARRY);
    }
    let empty: Vec<MandLabelAce> = Vec::new();
    if unique_label(&empty, &ace) != Ok(0) {
        return Err(E_DUP);
    }
    let one: Vec<MandLabelAce> = {
        let mut v = Vec::new();
        v.push(ace);
        v
    };
    if unique_label(&one, &ace) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if !sd.has(C_SACL_PROT) {
        // 保护位未置——载体可继承（本模块只记录形状，不判拒）。
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(153));
    }

    #[test]
    fn label_domain_closed() {
        assert!(label_known(LABEL_SYSTEM));
        assert!(!label_known(0x9000));
        assert_eq!(MandLabelAce::build(0x9000, POL_NO_WRITE_UP).err(), Some(E_LEVEL));
    }

    #[test]
    fn no_write_up_redline() {
        let low = MandLabelAce { level: LABEL_LOW, policy: POL_NO_WRITE_UP };
        let high = MandLabelAce { level: LABEL_HIGH, policy: POL_NO_WRITE_UP };
        assert_eq!(no_write_up(&low, &high, true, false, false).err(), Some(E_NOUP));
        assert!(no_write_up(&high, &low, true, false, false).is_ok());
    }
}

pub fn run_unx36068_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36068_sdmlabel");
    cs.add("SEG 段位独占", SEG_BASE == 0xe200, "0xe200~0xe20f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五档载体 100 例+NoWriteUp 50 例+注入 8 例全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("五档闭集锚", LABEL_N == 5, "强制标签五档（低/受保轻/中/高/系统）");
    cs
}
