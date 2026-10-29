//! UNX-F36084-J1 系统审计 ACE（成功/失败双触发位）。
//! 判据：① 双触发位（SUCCESS=0x40 / FAILURE=0x80）至少一位才合法，
//! 审计矩阵 4 态（无/仅成功/仅失败/双开）判定与命中求值 8 例对平；
//! ② 无触发位 2 例、错类型锚 2 例、坏 SID 2 例全部显性拒绝；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36081_acecore；与 36082/36083 对偶——
//! 审计 ACE 只记账不裁决，命中事件格式归 36094（fake 联签对拍）。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xe300;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_TYPE: u16 = SEG_BASE + 2;  // 类型锚不符（非 SystemAudit）
pub const E_SID: u16 = SEG_BASE + 3;   // SID 槽位引用非法
pub const E_TRIG: u16 = SEG_BASE + 4;  // 双触发位全零（至少一位才合法）
pub const E_MATRIX: u16 = SEG_BASE + 5;// 审计矩阵纪律破
pub const E_CS: u16 = SEG_BASE + 6;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 审计触发位：成功触发（bit6）。
pub const TRIG_SUCCESS: u8 = 0x40;
/// 审计触发位：失败触发（bit7）。
pub const TRIG_FAILURE: u8 = 0x80;
/// 双触发联合掩码。
pub const TRIG_BOTH: u8 = TRIG_SUCCESS | TRIG_FAILURE;
/// SID 槽位锚（与 36082/36083 同锚）。
pub const SID_NIL: u16 = 0;
pub const SID_SLOT_MAX: u16 = 63;
/// 审计 ACE 线上形式长度。
pub const AUDIT_WIRE_LEN: usize = 10;

/// 审计矩阵四态（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditState {
    /// 双零：非法态（至少一位才合法）。
    Neither      = 0x0000,
    /// 仅成功触发。
    SuccessOnly  = 0x0001,
    /// 仅失败触发。
    FailureOnly  = 0x0002,
    /// 双开。
    BothArmed    = 0x0003,
}

impl AuditState {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 由触发位归类审计态。
    pub fn classify(flags: u8) -> AuditState {
        let s = flags & TRIG_SUCCESS != 0;
        let f = flags & TRIG_FAILURE != 0;
        match (s, f) {
            (false, false) => AuditState::Neither,
            (true, false) => AuditState::SuccessOnly,
            (false, true) => AuditState::FailureOnly,
            (true, true) => AuditState::BothArmed,
        }
    }
    /// 审计矩阵合法性（唯一非法态 = Neither）。
    pub fn legal(self) -> bool {
        self.code() != AuditState::Neither.code()
    }
}

/// 命中事件类别（36094 事件格式的类别槽）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditHit {
    /// 成功命中（触发位含 SUCCESS 且本次访问被授）。
    SuccessHit = 0x0001,
    /// 失败命中（触发位含 FAILURE 且本次访问被拒）。
    FailureHit = 0x0002,
}

impl AuditHit {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 系统审计 ACE 语义载体：真源头 + SID 槽位引用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditAce {
    pub head: core::AceHeader,
    pub sid_slot: u16,
}

impl AuditAce {
    /// 受控构造：类型锚 → 真源四件闸 → SID 引用闸 → 触发位闸。
    pub fn new(flags: u8, mask: u32, sid_slot: u16) -> Result<AuditAce, u16> {
        let head = core::AceHeader::new(
            core::AceType::SystemAudit.wire_byte()?,
            flags,
            core::ACE_HEADER_MIN as u16,
            mask,
        )?;
        if sid_slot == SID_NIL || sid_slot > SID_SLOT_MAX {
            return Err(E_SID);
        }
        if flags & TRIG_BOTH == 0 {
            return Err(E_TRIG);
        }
        Ok(AuditAce { head, sid_slot })
    }

    /// 线上形式：头 8 字节 + sid 槽位 le16。
    pub fn to_wire(&self) -> [u8; AUDIT_WIRE_LEN] {
        let mut out = [0u8; AUDIT_WIRE_LEN];
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

    /// 线上形式解析（长度、真源闸、类型锚、SID 引用、触发位全闸）。
    pub fn from_wire(buf: &[u8]) -> Result<AuditAce, u16> {
        if buf.len() < AUDIT_WIRE_LEN {
            return Err(E_CS);
        }
        let head = core::parse(buf)?;
        if head.ace_type != core::AceType::SystemAudit.wire_byte()? {
            return Err(E_TYPE);
        }
        let sid_slot = u16::from_le_bytes([buf[8], buf[9]]);
        if sid_slot == SID_NIL || sid_slot > SID_SLOT_MAX {
            return Err(E_SID);
        }
        if head.flags & TRIG_BOTH == 0 {
            return Err(E_TRIG);
        }
        Ok(AuditAce { head, sid_slot })
    }

    /// 审计态归类。
    pub fn state(&self) -> AuditState {
        AuditState::classify(self.head.flags)
    }

    /// 指纹（真源 FNV over 线上形式 + 版本锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut feed = Vec::new();
        let w = self.to_wire();
        let mut i = 0;
        while i < AUDIT_WIRE_LEN {
            feed.push(w[i]);
            i += 1;
        }
        feed.extend_from_slice(&VER.to_le_bytes());
        core::fnv1a(&feed)
    }
}

/// 命中求值：给定触发位与本次访问结果（granted=被授），返回命中类别或 None。
pub fn on_access(flags: u8, granted: bool) -> Option<AuditHit> {
    if granted && flags & TRIG_SUCCESS != 0 {
        return Some(AuditHit::SuccessHit);
    }
    if !granted && flags & TRIG_FAILURE != 0 {
        return Some(AuditHit::FailureHit);
    }
    None
}

/// 审计矩阵纪律自检：4 态恰好 3 合法 1 非法，且 classify 与 legal 互证。
pub fn matrix_integrity() -> Result<(), u16> {
    let states = [
        AuditState::Neither,
        AuditState::SuccessOnly,
        AuditState::FailureOnly,
        AuditState::BothArmed,
    ];
    let mut legal_n = 0;
    let mut i = 0;
    while i < states.len() {
        if states[i].legal() {
            legal_n += 1;
        }
        i += 1;
    }
    if legal_n != 3 {
        return Err(E_MATRIX);
    }
    // 互证：双零 flags 必归 Neither，双开 flags 必归 BothArmed。
    if AuditState::classify(0x00) != AuditState::Neither {
        return Err(E_MATRIX);
    }
    if AuditState::classify(TRIG_BOTH) != AuditState::BothArmed {
        return Err(E_MATRIX);
    }
    Ok(())
}

/// 摘要行（String 面，无 IO）。
pub fn summary(a: &AuditAce) -> String {
    let mut s = String::new();
    s.push_str("audit sid=");
    s.push_str(&a.sid_slot.to_string());
    s.push_str(" state=");
    s.push_str(&a.state().code().to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_TYPE, E_SID, E_TRIG, E_MATRIX, E_CS];
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
        E_TYPE => "类型锚不符（非 SystemAudit）",
        E_SID => "SID 槽位引用非法",
        E_TRIG => "双触发位全零（至少一位才合法）",
        E_MATRIX => "审计矩阵纪律破",
        E_CS => "指纹不稳/长度不足",
        _ => "未知诊断码",
    }
}

/// 演练：R1 矩阵纪律；R2 8 例合法构造往返（三类态覆盖）；R3 无触发位 2 例；
/// R4 错类型锚 2 例；R5 坏 SID 2 例；R6 命中求值矩阵 4 例；R7 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：审计矩阵纪律。
    matrix_integrity()?;
    hits += 1;
    // R2：8 例合法构造 + 往返（成功/失败/双开轮转）。
    let mut i: u16 = 0;
    while i < 8 {
        let flags: u8 = match i % 3 {
            0 => TRIG_SUCCESS,
            1 => TRIG_FAILURE,
            _ => TRIG_BOTH,
        };
        let a = AuditAce::new(flags, 0x0001_0003, SID_NIL + 1 + i)?;
        if !a.state().legal() {
            return Err(E_MATRIX);
        }
        let w = a.to_wire();
        let back = AuditAce::from_wire(&w)?;
        if back != a {
            return Err(E_CS);
        }
        if a.fingerprint() != back.fingerprint() || a.fingerprint() == 0 {
            return Err(E_CS);
        }
        hits += 1;
        i += 1;
    }
    // R3：无触发位 2 例——全部 E_TRIG（至少一位才合法）。
    let mut z = 0;
    while z < 2 {
        let r = AuditAce::new(0x00, 0x0001_0003, 1 + z);
        match r {
            Ok(_) => return Err(E_TRIG),
            Err(c) => {
                if c != E_TRIG {
                    return Err(E_TRIG);
                }
            }
        }
        hits += 1;
        z += 1;
    }
    // R4：错类型锚 2 例（0x00 allow / 0x01 deny）——from_wire 报 E_TYPE。
    let wrongs: Vec<u8> = vec![0x00, 0x01];
    let mut w = 0;
    while w < wrongs.len() {
        let hb = core::encode(&core::AceHeader::new(wrongs[w], TRIG_SUCCESS, 0x0008, 0x0001)?);
        let mut wire = [0u8; AUDIT_WIRE_LEN];
        let mut t = 0;
        while t < 8 {
            wire[t] = hb[t];
            t += 1;
        }
        wire[8] = 0x01;
        wire[9] = 0x00;
        match AuditAce::from_wire(&wire) {
            Ok(_) => return Err(E_TYPE),
            Err(c) => {
                if c != E_TYPE {
                    return Err(E_TYPE);
                }
            }
        }
        hits += 1;
        w += 1;
    }
    // R5：坏 SID 引用 2 例（NIL / 上限+1）——全部 E_SID。
    let bad_sids: Vec<u16> = vec![SID_NIL, SID_SLOT_MAX + 1];
    let mut b = 0;
    while b < bad_sids.len() {
        let r = AuditAce::new(TRIG_BOTH, 0x0001_0003, bad_sids[b]);
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
    // R6：命中求值矩阵 4 例（成功×授、失败×拒、成功×拒、失败×授）。
    if on_access(TRIG_SUCCESS, true) != Some(AuditHit::SuccessHit) {
        return Err(E_MATRIX);
    }
    hits += 1;
    if on_access(TRIG_FAILURE, false) != Some(AuditHit::FailureHit) {
        return Err(E_MATRIX);
    }
    hits += 1;
    if on_access(TRIG_SUCCESS, false).is_some() {
        return Err(E_MATRIX);
    }
    hits += 1;
    if on_access(TRIG_FAILURE, true).is_some() {
        return Err(E_MATRIX);
    }
    hits += 1;
    // R7：指纹两次求值稳定且非零。
    let a0 = AuditAce::new(TRIG_BOTH, 0x000f_ffff, 9)?;
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
        assert_eq!(drill(), Ok(20));
    }

    #[test]
    fn no_trigger_rejected() {
        assert_eq!(AuditAce::new(0x00, 1, 1), Err(E_TRIG));
        assert_eq!(AuditAce::new(0x0f, 1, 1), Err(E_TRIG));
    }

    #[test]
    fn matrix_holds() {
        assert_eq!(matrix_integrity(), Ok(()));
        assert_eq!(AuditState::classify(TRIG_BOTH), AuditState::BothArmed);
        assert!(!AuditState::Neither.legal());
    }

    #[test]
    fn hit_eval_holds() {
        assert_eq!(on_access(TRIG_SUCCESS, true), Some(AuditHit::SuccessHit));
        assert_eq!(on_access(TRIG_FAILURE, true), None);
        assert!(summary(
            &AuditAce::new(TRIG_BOTH, 1, 3).unwrap_or(AuditAce {
                head: core::AceHeader { ace_type: 0, flags: 0, size: 0, mask: 0 },
                sid_slot: 0,
            })
        )
        .contains("audit"));
    }
}

pub fn run_unx36084_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36084_aceaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0xe300, "0xe300~0xe30f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "矩阵 4 态对平、命中求值 4 例对平"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("审计矩阵 3 合法 1 非法", matrix_integrity().is_ok(), "审计矩阵纪律破");
    cs.add("触发位锚 0x40/0x80", TRIG_BOTH == 0xc0, "审计触发位锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
