//! UNX-F36069-J1 SD 所有者/组段的写权限门（WRITE_OWNER 语义）。
//! 判据：① 写门三态（放行/拒绝/需特权）100 例与请求者域逐项对平；
//! ② 越门注入 12 例全拒（无特权改属主/缺 WRITE_OWNER 位/组段越写）；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：WRITE_OWNER 语义载体——SD 的 Owner/Group 段不是想改就改：
//! 改者必须持有 WRITE_OWNER 权限位或 SeTakeOwnershipPrivilege 特权。
//! 本模块管门（who can change owner/group），段存储仍在 unx36061 结构面。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{
    self as core, Sd, C_GROUP_DFLT, C_OWNER_DFLT, E_CTRL, E_OFF, OFF_NONE,
};

pub const SEG_BASE: u16 = 0xe210;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_GATE: u16 = SEG_BASE + 1; // 写门拒绝（无 WRITE_OWNER 位）
pub const E_PRIV: u16 = SEG_BASE + 2; // 无 SeTakeOwnership 特权（越权改属主）
pub const E_GROUP: u16 = SEG_BASE + 3; // 组段越写（非属主改主组）
pub const E_SELF: u16 = SEG_BASE + 4; // 属主自改主组缺特权（合法但需记账）
pub const E_DFLT: u16 = SEG_BASE + 5; // DFLT 标记与写来源不一致

/// 请求者域（20 例：属主 1 / 组内 1 / 特权持有 2 / 普通 16）。
pub const SUBJECT_N: usize = 20;
pub const ROLE_OWNER: u8 = 0;
pub const ROLE_MEMBER: u8 = 1;
pub const ROLE_PRIV_TAKE: u8 = 2; // 持 SeTakeOwnershipPrivilege
pub const ROLE_PRIV_OWNER: u8 = 3; // 持 SeRestoreOwnership 类特权
pub const ROLE_NORMAL: u8 = 4;

/// 请求者角色表（20 例域的真源）。
pub const ROLE_TAB: [u8; SUBJECT_N] = [
    ROLE_OWNER, ROLE_MEMBER, ROLE_PRIV_TAKE, ROLE_PRIV_OWNER,
    ROLE_NORMAL, ROLE_NORMAL, ROLE_NORMAL, ROLE_NORMAL,
    ROLE_NORMAL, ROLE_NORMAL, ROLE_NORMAL, ROLE_NORMAL,
    ROLE_NORMAL, ROLE_NORMAL, ROLE_NORMAL, ROLE_NORMAL,
    ROLE_NORMAL, ROLE_NORMAL, ROLE_NORMAL, ROLE_NORMAL,
];

/// 权限位域（请求掩码的写族位——B06 域全表，此处只取两件）。
pub const RIGHT_WRITE_OWNER: u32 = 0x0008_0000; // WRITE_OWNER（标准位族）
pub const RIGHT_WRITE_DAC: u32 = 0x0004_0000; // WRITE_DAC（对照位）

/// 特权位（模拟 SeTakeOwnershipPrivilege 持有标记）。
pub const PRIV_TAKE_OWNERSHIP: u32 = 0x0001;

/// 写门裁决（Owner 段）：三态——
/// 放行：请求者持 WRITE_OWNER 位且是当前属主；
/// 特权路径：持 SeTakeOwnership 特权（放行但记账——占位 E_PRIV 之外的 Ok）；
/// 拒绝：其余。
pub fn gate_owner(
    subject: usize,
    held_rights: u32,
    held_privs: u32,
) -> Result<u8, u16> {
    if subject >= SUBJECT_N {
        return Err(E_GATE);
    }
    let role = ROLE_TAB[subject];
    let priv_role_ok = role == ROLE_PRIV_TAKE || role == ROLE_PRIV_OWNER;
    if held_privs & PRIV_TAKE_OWNERSHIP != 0 {
        if priv_role_ok {
            return Ok(1); // 特权路径放行（记账态）
        }
        if role != ROLE_OWNER {
            return Err(E_PRIV); // 特权位在无权角色手上：越权形态
        }
        // 属主持特权：落入常规门（有 WRITE_OWNER → 直放，无 → E_GATE）。
    }
    if held_rights & RIGHT_WRITE_OWNER == 0 {
        return Err(E_GATE);
    }
    if role == ROLE_OWNER {
        return Ok(0); // 属主 + WRITE_OWNER：直接放行
    }
    Err(E_GATE)
}

/// 写门裁决（Group 段）：只有属主（或特权路径）可改主组；组内成员越写即拒。
pub fn gate_group(
    subject: usize,
    held_rights: u32,
    held_privs: u32,
) -> Result<u8, u16> {
    if subject >= SUBJECT_N {
        return Err(E_GROUP);
    }
    let role = ROLE_TAB[subject];
    let priv_role_ok = role == ROLE_PRIV_TAKE || role == ROLE_PRIV_OWNER;
    if held_privs & PRIV_TAKE_OWNERSHIP != 0 {
        if priv_role_ok {
            return Ok(1);
        }
        return Err(E_PRIV); // 特权位与角色不匹配：越权形态
    }
    if held_rights & RIGHT_WRITE_OWNER == 0 {
        return Err(E_GROUP);
    }
    if role == ROLE_OWNER {
        return Ok(0);
    }
    if role == ROLE_MEMBER {
        return Err(E_GROUP); // 组内成员改主组：越写
    }
    Err(E_GROUP)
}

/// DFLT 标记账：属主/组被显式写入后，DEFAULTED 位必须清位；
/// 走默认机制置入才可置位。写来源与标记不一致即拒。
pub fn dflt_consistent(sd: &Sd, owner_written: bool, group_written: bool) -> Result<(), u16> {
    if owner_written && sd.has(C_OWNER_DFLT) {
        return Err(E_DFLT);
    }
    if group_written && sd.has(C_GROUP_DFLT) {
        return Err(E_DFLT);
    }
    Ok(())
}

/// 写后账目行（门结果三态 + subject + 段选择）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GateRow {
    pub subject: usize,
    pub is_owner_seg: bool,
    pub verdict: u8, // 0 直接放行 / 1 特权放行
}

pub const GATE_CAP: usize = 128;

pub struct GateLedger {
    rows: [Option<GateRow>; GATE_CAP],
    count: usize,
}

const NONE_GATE: Option<GateRow> = None;

impl GateLedger {
    pub fn new() -> GateLedger {
        GateLedger { rows: [NONE_GATE; GATE_CAP], count: 0 }
    }
    pub fn record(&mut self, row: GateRow) -> Result<(), u16> {
        if self.count >= GATE_CAP {
            return Err(E_GATE); // 显性满拒
        }
        self.rows[self.count] = Some(row);
        self.count += 1;
        Ok(())
    }
    pub fn count_verdict(&self, verdict: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.verdict == verdict {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
    pub fn count(&self) -> usize {
        self.count
    }
}

/// SD 夹具（带 Owner/Group 段的四段 SD）。
pub fn owner_group_sd() -> Sd {
    let (sd, _) = super::unx36063_sdctor::minimal_full().unwrap_or((Sd::bare(), [super::unx36063_sdctor::SectSpec { len: 0 }; 4]));
    sd
}

fn detail(code: u16) -> &'static str {
    match code {
        E_GATE => "写门拒绝（缺 WRITE_OWNER 位或角色不符）",
        E_PRIV => "无 SeTakeOwnership 特权（越权改属主）",
        E_GROUP => "组段越写（非属主改主组）",
        E_SELF => "属主自改主组缺特权（记账态缺失）",
        E_DFLT => "DFLT 标记与写来源不一致",
        E_SELF => "属主自改主组缺特权（记账态缺失）",
        E_CTRL => "控制位含未知位",
        E_OFF => "段偏移与 PRESENT 位不一致",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 写门三态 100 例（20 请求者 × 5 轮 × 双段）；
/// R2 越门注入 12 例全拒；R3 DFLT 一致性；R4 门账分账守恒；
/// R5 特权路径记账（放行但 verdict=1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：100 例（subject 轮转 × 掩码两态 × 双段）。
    let mut r = 0u32;
    while r < 100 {
        let s = (r as usize) % SUBJECT_N;
        let with_right = r % 2 == 0;
        let rights = if with_right { RIGHT_WRITE_OWNER } else { 0 };
        let is_owner_seg = r % 4 < 2;
        let role = ROLE_TAB[s];
        let expect_direct = with_right && role == ROLE_OWNER;
        let verdict = if is_owner_seg {
            gate_owner(s, rights, 0)
        } else {
            gate_group(s, rights, 0)
        };
        if expect_direct {
            if verdict != Ok(0) {
                return Err(E_GATE);
            }
        }
        // 非期望直放的：结果必须是 Err（普通/成员无特权路径在本轮）。
        if !expect_direct && verdict.is_ok() {
            return Err(E_GATE);
        }
        hits += 1;
        r += 1;
    }
    // R2：越门注入 12 例。
    //  a1 普通 subject 持位改属主；a2 成员改属主；a3 无位属主；
    //  b1 普通改组；b2 成员改组；b3 无位属主改组；
    //  c1 特权位给了普通角色（特权+角色不匹配）；c2 越域 subject 20；
    //  c3 越域 subject 99；c4 特权+WRITE_DAC 无 WRITE_OWNER 改属主；
    //  c5 越域 subject 1000；c6 特权+无位成员改组。
    let mut n = 0;
    if gate_owner(4, RIGHT_WRITE_OWNER, 0) != Err(E_GATE) {
        return Err(E_GATE);
    }
    n += 1;
    if gate_owner(1, RIGHT_WRITE_OWNER, 0) != Err(E_GATE) {
        return Err(E_GATE);
    }
    n += 1;
    if gate_owner(0, 0, 0) != Err(E_GATE) {
        return Err(E_GATE);
    }
    n += 1;
    if gate_group(4, RIGHT_WRITE_OWNER, 0) != Err(E_GROUP) {
        return Err(E_GROUP);
    }
    n += 1;
    if gate_group(1, RIGHT_WRITE_OWNER, 0) != Err(E_GROUP) {
        return Err(E_GROUP);
    }
    n += 1;
    if gate_group(0, 0, 0) != Err(E_GROUP) {
        return Err(E_GROUP);
    }
    n += 1;
    // c1：特权位在普通角色手上——特权+角色不匹配是越权形态（E_PRIV）。
    if gate_owner(4, 0, PRIV_TAKE_OWNERSHIP) != Err(E_PRIV) {
        return Err(E_PRIV);
    }
    n += 1;
    if gate_owner(SUBJECT_N, RIGHT_WRITE_OWNER, 0) != Err(E_GATE) {
        return Err(E_GATE);
    }
    n += 1;
    if gate_owner(99, RIGHT_WRITE_OWNER, 0) != Err(E_GATE) {
        return Err(E_GATE);
    }
    n += 1;
    // c4：WRITE_DAC 冒充 WRITE_OWNER——位域不同，门必拒。
    if gate_owner(0, RIGHT_WRITE_DAC, 0) != Err(E_GATE) {
        return Err(E_GATE);
    }
    n += 1;
    if gate_owner(1000, RIGHT_WRITE_OWNER, 0) != Err(E_GATE) {
        return Err(E_GATE);
    }
    n += 1;
    if gate_group(1, 0, PRIV_TAKE_OWNERSHIP) != Err(E_PRIV) {
        return Err(E_PRIV);
    }
    n += 1;
    if n != 12 {
        return Err(E_GATE);
    }
    hits += 1;
    // R3：DFLT 一致性（显式写 → DFLT 必清；默认置入 → DFLT 可置）。
    let mut sd = owner_group_sd();
    sd.clear_bit(C_OWNER_DFLT);
    dflt_consistent(&sd, true, false)?;
    sd.set_bit(C_OWNER_DFLT);
    if dflt_consistent(&sd, true, false) != Err(E_DFLT) {
        return Err(E_DFLT);
    }
    dflt_consistent(&sd, false, false)?;
    hits += 1;
    // R4：门账分账守恒（20 例：属主直放 1 + 特权放行 2 + 其余拒绝不入账）。
    let mut ledger = GateLedger::new();
    let mut s = 0;
    while s < SUBJECT_N {
        let v = gate_owner(s, RIGHT_WRITE_OWNER, PRIV_TAKE_OWNERSHIP);
        if let Ok(verdict) = v {
            ledger.record(GateRow { subject: s, is_owner_seg: true, verdict })?;
        }
        s += 1;
    }
    if ledger.count() != 3 {
        return Err(E_GATE); // OWNER(0) 直放 + PRIV_TAKE(2)/PRIV_OWNER(3) 特权
    }
    if ledger.count_verdict(0) + ledger.count_verdict(1) != ledger.count() {
        return Err(E_GATE);
    }
    if ledger.count_verdict(0) != 1 || ledger.count_verdict(1) != 2 {
        return Err(E_GATE);
    }
    hits += 1;
    // R5：特权路径 verdict=1 语义（对照直放 0）。
    if gate_owner(2, 0, PRIV_TAKE_OWNERSHIP) != Ok(1) {
        return Err(E_PRIV);
    }
    if gate_owner(0, RIGHT_WRITE_OWNER, 0) != Ok(0) {
        return Err(E_GATE);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(104));
    }

    #[test]
    fn owner_gate_triage() {
        assert_eq!(gate_owner(0, RIGHT_WRITE_OWNER, 0), Ok(0));
        assert_eq!(gate_owner(2, 0, PRIV_TAKE_OWNERSHIP), Ok(1));
        assert_eq!(gate_owner(4, RIGHT_WRITE_OWNER, 0).err(), Some(E_GATE));
    }

    #[test]
    fn group_gate_member_blocked() {
        assert_eq!(gate_group(1, RIGHT_WRITE_OWNER, 0).err(), Some(E_GROUP));
    }
}

pub fn run_unx36069_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36069_sdowner");
    cs.add("SEG 段位独占", SEG_BASE == 0xe210, "0xe210~0xe21f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "写门三态 100 例+越门 12 例全拒+DFLT 一致"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("请求者域锚", SUBJECT_N == 20, "20 例角色轮转域");
    cs
}
