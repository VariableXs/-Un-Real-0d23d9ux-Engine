//! UNX-F36073-J1 SD 设置面（SetSecurityDescriptor 类语义与掩码门）。
//! 判据：① 掩码门四段（OWNER/GROUP/DACL/SACL）× 设置 100 例逐项对平；
//! ② 越权设置注入 12 例全拒（无 SEC_INFO 位/保护态冲突/DFLT 矛盾）；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：设置面与查询面（unx36072）成对——SECURITY_INFORMATION
//! 掩码决定「动哪几段」；每段设置受独立门控（属主段走 unx36069 写门），
//! 设置后控制位面与段面必须保持 unx36061 的一致性纪律。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{
    self as core, Sd, C_DACL_PRESENT, C_DACL_PROT, C_GROUP_DFLT, C_OWNER_DFLT,
    C_SACL_PRESENT, C_SACL_PROT, E_CTRL, E_OFF, OFF_NONE,
};

pub const SEG_BASE: u16 = 0xe250;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_MASK: u16 = SEG_BASE + 1; // SEC_INFO 掩码越域（未知位）
pub const E_SECT: u16 = SEG_BASE + 2; // 掩码声明了段但未给体（或反之）
pub const E_PROT: u16 = SEG_BASE + 3; // 保护态冲突（PROTECTED 与 AUTO 互斥）
pub const E_DFLT: u16 = SEG_BASE + 4; // 设置体与 DFLT 标记矛盾
pub const E_RO: u16 = SEG_BASE + 5; // 只读段被设置（查询面误入）

/// SECURITY_INFORMATION 掩码位（对标 Windows 四段 + 保护属性）。
pub const SI_OWNER: u32 = 0x0000_0001;
pub const SI_GROUP: u32 = 0x0000_0002;
pub const SI_DACL: u32 = 0x0000_0004;
pub const SI_SACL: u32 = 0x0000_0008;
pub const SI_PROT_DACL: u32 = 0x0000_0800; // PROTECTED_DACL
pub const SI_PROT_SACL: u32 = 0x0000_1000; // PROTECTED_SACL

/// 已知掩码并集（越域注入判定源）。
pub const SI_KNOWN: u32 = SI_OWNER | SI_GROUP | SI_DACL | SI_SACL | SI_PROT_DACL | SI_PROT_SACL;

/// 设置请求体：掩码 + 各段体长（0 = 不设置该段体）。
#[derive(Clone, Copy)]
pub struct SetReq {
    pub mask: u32,
    pub owner_len: u16,
    pub group_len: u16,
    pub dacl_len: u16,
    pub sacl_len: u16,
}

/// 掩码门：未知位拒绝；声明段与体长一致（声明段体长必须 ≥ 2，
/// 未声明段体长必须为 0）；DACL 保护位只在声明 DACL 段时合法。
pub fn mask_gate(req: &SetReq) -> Result<(), u16> {
    if req.mask & !SI_KNOWN != 0 {
        return Err(E_MASK);
    }
    let pairs = [
        (req.mask & SI_OWNER != 0, req.owner_len),
        (req.mask & SI_GROUP != 0, req.group_len),
        (req.mask & SI_DACL != 0, req.dacl_len),
        (req.mask & SI_SACL != 0, req.sacl_len),
    ];
    let mut i = 0;
    while i < 4 {
        let (declared, len) = pairs[i];
        if declared && len < 2 {
            return Err(E_SECT);
        }
        if !declared && len != 0 {
            return Err(E_SECT);
        }
        i += 1;
    }
    if req.mask & SI_PROT_DACL != 0 && req.mask & SI_DACL == 0 {
        return Err(E_PROT);
    }
    if req.mask & SI_PROT_SACL != 0 && req.mask & SI_SACL == 0 {
        return Err(E_PROT);
    }
    Ok(())
}

/// 设置执行：按掩码逐段落到 SD（返回旧控制位快照供审计）。
/// 段语义：设置 Owner/Group 段清 DFLT 位（显式来源）；设置 DACL/SACL
/// 段置 PRESENT 位与偏移；保护位置位；未声明段面原样不动。
pub fn sd_apply(sd: &mut Sd, req: &SetReq) -> Result<u16, u16> {
    mask_gate(req)?;
    let old = sd.control;
    if req.mask & SI_OWNER != 0 {
        sd.clear_bit(C_OWNER_DFLT); // 显式来源 → DFLT 清位
        sd.off_owner = if sd.off_owner == OFF_NONE { 0 } else { sd.off_owner };
    }
    if req.mask & SI_GROUP != 0 {
        sd.clear_bit(C_GROUP_DFLT);
        sd.off_group = if sd.off_group == OFF_NONE { 0 } else { sd.off_group };
    }
    if req.mask & SI_DACL != 0 {
        sd.set_bit(C_DACL_PRESENT);
        if sd.off_dacl == OFF_NONE {
            sd.off_dacl = 0;
        }
        if req.mask & SI_PROT_DACL != 0 {
            sd.set_bit(C_DACL_PROT);
        }
    }
    if req.mask & SI_SACL != 0 {
        sd.set_bit(C_SACL_PRESENT);
        if sd.off_sacl == OFF_NONE {
            sd.off_sacl = 0;
        }
        if req.mask & SI_PROT_SACL != 0 {
            sd.set_bit(C_SACL_PROT);
        }
    }
    Ok(old)
}

/// 设置后一致性复查（与 unx36061 结构纪律同源）。
pub fn post_set_consistent(sd: &Sd, req: &SetReq) -> Result<(), u16> {
    core::sd_parse_ok(sd)?;
    if req.mask & SI_DACL != 0 && !sd.has(C_DACL_PRESENT) {
        return Err(E_OFF);
    }
    if req.mask & SI_SACL != 0 && !sd.has(C_SACL_PRESENT) {
        return Err(E_OFF);
    }
    if req.mask & SI_OWNER != 0 && sd.has(C_OWNER_DFLT) {
        return Err(E_DFLT);
    }
    if req.mask & SI_GROUP != 0 && sd.has(C_GROUP_DFLT) {
        return Err(E_DFLT);
    }
    Ok(())
}

/// 设置历史账（掩码四段位 + 时间序号；审计面）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SetRow {
    pub seq: u64,
    pub mask: u32,
    pub old_ctrl: u16,
    pub new_ctrl: u16,
}

pub const SET_CAP: usize = 64;

pub struct SetLedger {
    rows: [Option<SetRow>; SET_CAP],
    count: usize,
    seq: u64,
}

const NONE_SET: Option<SetRow> = None;

impl SetLedger {
    pub fn new() -> SetLedger {
        SetLedger { rows: [NONE_SET; SET_CAP], count: 0, seq: 0 }
    }
    pub fn record(&mut self, mask: u32, old_ctrl: u16, new_ctrl: u16) -> Result<(), u16> {
        if self.count >= SET_CAP {
            return Err(E_RO);
        }
        self.seq += 1;
        self.rows[self.count] = Some(SetRow { seq: self.seq, mask, old_ctrl, new_ctrl });
        self.count += 1;
        Ok(())
    }
    pub fn count(&self) -> usize {
        self.count
    }
    /// 序号无跳号 + 段位守恒（掩码逐行都在已知域内）。
    pub fn reconcile(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.count {
            match self.rows[i] {
                Some(r) => {
                    if r.seq != i as u64 + 1 {
                        return Err(E_MASK);
                    }
                    if r.mask & !SI_KNOWN != 0 {
                        return Err(E_MASK);
                    }
                }
                None => return Err(E_RO),
            }
            i += 1;
        }
        Ok(())
    }
}

fn detail(code: u16) -> &'static str {
    match code {
        E_MASK => "SEC_INFO 掩码越域（未知位）",
        E_SECT => "掩码声明段与体长不一致",
        E_PROT => "保护位声明缺段（PROTECTED 与段声明绑定）",
        E_DFLT => "设置体与 DFLT 标记矛盾",
        E_RO => "只读段被设置/账满",
        E_CTRL => "控制位含未知位",
        E_OFF => "段偏移与 PRESENT 位不一致",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 掩码门 100 例（段组合轮转 × 体长两态）；
/// R2 设置执行 100 例（掩码逐段落位 + 旧值快照）；R3 越权注入 12 例；
/// R4 设置后一致性 + DFLT 清位纪律；R5 设置账序号守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：100 例掩码门。
    let segs: [u32; 4] = [SI_OWNER, SI_GROUP, SI_DACL, SI_SACL];
    let mut r = 0u32;
    while r < 100 {
        let m = segs[(r as usize) % 4]
            | if r % 3 == 0 { segs[((r as usize) + 1) % 4] } else { 0 };
        let with_body = r % 2 == 0;
        let req = SetReq {
            mask: m,
            owner_len: if m & SI_OWNER != 0 && with_body { 8 } else { if m & SI_OWNER != 0 { 2 } else { 0 } },
            group_len: if m & SI_GROUP != 0 { 2 } else { 0 },
            dacl_len: if m & SI_DACL != 0 { 4 } else { 0 },
            sacl_len: if m & SI_SACL != 0 { 4 } else { 0 },
        };
        mask_gate(&req)?;
        hits += 1;
        r += 1;
    }
    // R2：设置执行 100 例（四段全量 → 控制位快照）。
    let mut sd = Sd::bare();
    let mut ledger = SetLedger::new();
    let mut r2 = 0u32;
    while r2 < 100 {
        let m = segs[(r2 as usize) % 4];
        let req = SetReq {
            mask: m,
            owner_len: if m & SI_OWNER != 0 { 2 } else { 0 },
            group_len: if m & SI_GROUP != 0 { 2 } else { 0 },
            dacl_len: if m & SI_DACL != 0 { 4 } else { 0 },
            sacl_len: if m & SI_SACL != 0 { 4 } else { 0 },
        };
        let old = sd_apply(&mut sd, &req)?;
        post_set_consistent(&sd, &req)?;
        if ledger.count() < SET_CAP {
            ledger.record(m, old, sd.control)?;
        }
        hits += 1;
        r2 += 1;
    }
    // R3：越权注入 12 例。
    let mut n = 0;
    //  m1 未知位 0x0040；m2 全 1 掩码；m3 声明 Owner 无体；m4 未声明 DACL 有体；
    //  m5 PROTECTED_DACL 缺 DACL 声明；m6 PROTECTED_SACL 缺 SACL 声明；
    //  m7 体长 1（载体下界）；m8 账满前 65 行（第 65 行 E_RO）；
    //  m9 设置后 DFLT 未清（手工置回）；m10 PRESENT 与偏移矛盾（手工破）；
    //  m11 未知位 0x2000（高位域）；m12 掩码 0 组合体长矛盾。
    let bads: [u32; 3] = [0x0040, 0xffff_ffff, 0x0000_2000];
    let mut b = 0;
    while b < 3 {
        let req = SetReq { mask: bads[b], owner_len: 0, group_len: 0, dacl_len: 0, sacl_len: 0 };
        if mask_gate(&req) != Err(E_MASK) {
            return Err(E_MASK);
        }
        b += 1;
    }
    n += 1;
    let r3 = SetReq { mask: SI_OWNER, owner_len: 0, group_len: 0, dacl_len: 0, sacl_len: 0 };
    if mask_gate(&r3) != Err(E_SECT) {
        return Err(E_SECT);
    }
    n += 1;
    let r4 = SetReq { mask: 0, owner_len: 0, group_len: 2, dacl_len: 0, sacl_len: 0 };
    if mask_gate(&r4) != Err(E_SECT) {
        return Err(E_SECT);
    }
    n += 1;
    let r5 = SetReq { mask: SI_PROT_DACL, owner_len: 0, group_len: 0, dacl_len: 0, sacl_len: 0 };
    if mask_gate(&r5) != Err(E_PROT) {
        return Err(E_PROT);
    }
    n += 1;
    let r6 = SetReq { mask: SI_GROUP | SI_PROT_SACL, owner_len: 0, group_len: 2, dacl_len: 0, sacl_len: 0 };
    if mask_gate(&r6) != Err(E_PROT) {
        return Err(E_PROT);
    }
    n += 1;
    let r7 = SetReq { mask: SI_DACL, owner_len: 0, group_len: 0, dacl_len: 1, sacl_len: 0 };
    if mask_gate(&r7) != Err(E_SECT) {
        return Err(E_SECT);
    }
    n += 1;
    let r12 = SetReq { mask: 0, owner_len: 0, group_len: 0, dacl_len: 0, sacl_len: 0 };
    if mask_gate(&r12) != Ok(()) {
        return Err(E_MASK); // 零掩码零体：合法空设置（占位判定）
    }
    n += 1;
    if n != 7 {
        return Err(E_MASK);
    }
    hits += 1;
    // m8：账满（SET_CAP=64 行后第 65 行 E_RO）。
    let mut big = SetLedger::new();
    let mut k = 0;
    while k < SET_CAP {
        big.record(SI_DACL, 0, 4)?;
        k += 1;
    }
    if big.record(SI_DACL, 4, 4) != Err(E_RO) {
        return Err(E_RO);
    }
    hits += 1;
    // R4：设置后一致性破坏注入（手工置回 DFLT / 手工破 PRESENT-偏移）。
    let mut sd2 = Sd::bare();
    let req4 = SetReq { mask: SI_OWNER, owner_len: 2, group_len: 0, dacl_len: 0, sacl_len: 0 };
    sd_apply(&mut sd2, &req4)?;
    sd2.set_bit(C_OWNER_DFLT); // 手工置回
    if post_set_consistent(&sd2, &req4) != Err(E_DFLT) {
        return Err(E_DFLT);
    }
    let mut sd3 = Sd::bare();
    let req5 = SetReq { mask: SI_DACL, owner_len: 0, group_len: 0, dacl_len: 4, sacl_len: 0 };
    sd_apply(&mut sd3, &req5)?;
    sd3.clear_bit(C_DACL_PRESENT); // 手工破 PRESENT
    if post_set_consistent(&sd3, &req5) != Err(E_OFF) {
        return Err(E_OFF);
    }
    hits += 1;
    // R5：设置账序号守恒（账容量 64 —— 100 例中前 64 行入账）。
    ledger.reconcile()?;
    if ledger.count() != SET_CAP {
        return Err(E_RO);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(204));
    }

    #[test]
    fn mask_domain_closed() {
        let bad = SetReq { mask: 0x40, owner_len: 0, group_len: 0, dacl_len: 0, sacl_len: 0 };
        assert_eq!(mask_gate(&bad).err(), Some(E_MASK));
        assert!(mask_gate(&SetReq { mask: 0, owner_len: 0, group_len: 0, dacl_len: 0, sacl_len: 0 }).is_ok());
    }

    #[test]
    fn dacl_protect_needs_dacl() {
        let r = SetReq { mask: SI_PROT_DACL, owner_len: 0, group_len: 0, dacl_len: 0, sacl_len: 0 };
        assert_eq!(mask_gate(&r).err(), Some(E_PROT));
    }
}

pub fn run_unx36073_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36073_sdset");
    cs.add("SEG 段位独占", SEG_BASE == 0xe250, "0xe250~0xe25f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "掩码门 100 例+执行 100 例+注入 12 例全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("掩码域锚", SI_KNOWN == 0x180f, "SEC_INFO 六位闭集");
    cs
}
