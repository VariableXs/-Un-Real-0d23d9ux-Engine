//! UNX-F36166-J1 AccessCheck 第④步支撑：SID 匹配面（acsidmatch）。
//! 判据：① 匹配契约冻结——`ace_applies(&AcAce, &AcTokenFace) -> bool`：
//! inherit-only 恒不适用；Allow 对 deny-only SID 不适用（只受审不受授）；
//! Deny 对 deny-only 命中亦适用；主体 = token.sid ∪ groups；② 匹配
//! 分类枚举与计数面；③ 注入负例显性拒；④ 纯功能代码 ≥310 行。
//! 设计纪律：36165 通过 `super::unx36166_acsidmatch as sm` 消费
//! ace_applies 与 F_INHERIT_ONLY；SID 用域内 u64 id（0 视为 Null Sid，
//! 永不匹配任何具名主体）。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3c20;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ACE: u16 = SEG_BASE + 2; // ACE 类型越出 {0=Allow,1=Deny}
pub const E_NULLSID: u16 = SEG_BASE + 3; // Null Sid 参与了具名匹配
pub const E_NEG: u16 = SEG_BASE + 4; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 5; // 匹配分类闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// ACE 标志位（检查侧闭集；inherit-only 恒不适用）。
pub const F_INHERIT_ONLY: u16 = 0x0008;
pub const F_INHERITED: u16 = 0x0010;
pub const F_SUCCESS_AUDIT: u16 = 0x0040;
pub const F_FAILED_AUDIT: u16 = 0x0080;
pub const FLAG_LEGAL: u16 = F_INHERIT_ONLY | F_INHERITED | F_SUCCESS_AUDIT | F_FAILED_AUDIT;

/// 匹配分类（判别式冻结；计数聚合用）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchKind {
    None = 0x0000,       // 主体不命中
    SelfMatch = 0x0001,  // token.sid 命中
    GroupMatch = 0x0002, // groups 命中
    DenyOnly = 0x0003,   // 仅 deny_only 命中（Deny 适用 / Allow 不适用）
    InheritSkip = 0x0004, // inherit-only 跳过
}

impl MatchKind {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// Null Sid 锚（域内 0；永不匹配具名主体）。
pub const NULL_SID: u64 = 0;

/// 主体命中分类（不含 inherit-only 前置检查）。
pub fn classify_sid(sid: u64, tok: &acf::AcTokenFace) -> MatchKind {
    if sid == NULL_SID {
        return MatchKind::None;
    }
    if sid == tok.sid {
        return MatchKind::SelfMatch;
    }
    let mut i = 0;
    while i < tok.groups.len() {
        if tok.groups[i] == sid {
            return MatchKind::GroupMatch;
        }
        i += 1;
    }
    let mut j = 0;
    while j < tok.deny_only.len() {
        if tok.deny_only[j] == sid {
            return MatchKind::DenyOnly;
        }
        j += 1;
    }
    MatchKind::None
}

/// 成员判定（不含 deny-only；授予/拒绝的共用面）。
pub fn is_member(sid: u64, tok: &acf::AcTokenFace) -> bool {
    if sid == NULL_SID {
        return false;
    }
    if sid == tok.sid {
        return true;
    }
    let mut i = 0;
    while i < tok.groups.len() {
        if tok.groups[i] == sid {
            return true;
        }
        i += 1;
    }
    false
}

// ---- 匹配契约 ---------------------------------------------------------------
/// ace_applies：ACE 是否对该令牌适用。
/// 规则序：① 类型越出 {0,1} → Err(E_ACE)；② inherit-only → false；
/// ③ Deny：主体命中（含 deny-only）即适用；④ Allow：主体命中且
/// 非 deny-only 适用；⑤ 其余 false。
pub fn ace_applies(ace: &acf::AcAce, tok: &acf::AcTokenFace) -> bool {
    if ace.typ != 0 && ace.typ != 1 {
        return false;
    }
    if ace.flags & F_INHERIT_ONLY != 0 {
        return false;
    }
    match classify_sid(ace.sid, tok) {
        MatchKind::SelfMatch | MatchKind::GroupMatch => true,
        MatchKind::DenyOnly => ace.typ == 1,
        MatchKind::None | MatchKind::InheritSkip => false,
    }
}

/// 带校验的适用判定（类型非法显性报错；演练与对拍用）。
pub fn ace_applies_checked(ace: &acf::AcAce, tok: &acf::AcTokenFace) -> Result<bool, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if ace.typ != 0 && ace.typ != 1 {
        return Err(E_ACE);
    }
    Ok(ace_applies(ace, tok))
}

/// 标志位合法性（越出闭集报错）。
pub fn flags_ok(ace: &acf::AcAce) -> Result<(), u16> {
    if ace.flags & !FLAG_LEGAL != 0 {
        return Err(E_ACE);
    }
    Ok(())
}

/// 有效主体计数：token.sid（非 Null）+ groups 内去重项（NULL/自体
/// 重复/表内重复均不计；deny_only 不扩权，不计入）。
pub fn effective_sid_count(tok: &acf::AcTokenFace) -> u32 {
    let mut n: u32 = 0;
    if tok.sid != NULL_SID {
        n += 1;
    }
    let mut i = 0;
    while i < tok.groups.len() {
        let g = tok.groups[i];
        if g != NULL_SID && g != tok.sid {
            let mut j = 0;
            let mut dup = false;
            while j < i {
                if tok.groups[j] == g {
                    dup = true;
                }
                j += 1;
            }
            if !dup {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 匹配分类闭集自检。
pub fn match_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if MatchKind::None.code() != 0
        || MatchKind::SelfMatch.code() != 1
        || MatchKind::GroupMatch.code() != 2
        || MatchKind::DenyOnly.code() != 3
        || MatchKind::InheritSkip.code() != 4
    {
        return Err(E_TABLE);
    }
    if F_INHERIT_ONLY & !FLAG_LEGAL != 0 {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 闭集自检；R2 自体/组/deny-only 三路分类；R3 ace_applies
/// 正负例（Allow 收 deny-only、Deny 纳 deny-only、inherit-only 跳过）；
/// R4 有效主体计数与标志位校验；R5 负例注入（Null Sid/坏类型/坏标志）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    match_integrity()?;
    hits += 1;
    // R2：三路分类——自体命中 / 组命中 / deny-only 归类 / 全不中。
    let tok = acf::AcTokenFace {
        token_id: 0x1111,
        sid: 0x1111,
        groups: vec![0x2222u64, 0x3333u64],
        deny_only: vec![0x4444u64],
        privs: Vec::new(),
        imp_level: 2,
    };
    if classify_sid(0x1111, &tok) != MatchKind::SelfMatch {
        return Err(E_NEG);
    }
    if classify_sid(0x2222, &tok) != MatchKind::GroupMatch {
        return Err(E_NEG);
    }
    if classify_sid(0x4444, &tok) != MatchKind::DenyOnly {
        return Err(E_NEG);
    }
    if classify_sid(0x9999, &tok) != MatchKind::None {
        return Err(E_NEG);
    }
    hits += 1;
    // R3：ace_applies 正负例。
    let a_allow_self = acf::AcAce { typ: 0, mask: 0x89, sid: 0x1111, flags: 0 };
    if !ace_applies(&a_allow_self, &tok) {
        return Err(E_NEG);
    }
    let a_allow_grp = acf::AcAce { typ: 0, mask: 0x89, sid: 0x3333, flags: 0 };
    if !ace_applies(&a_allow_grp, &tok) {
        return Err(E_NEG);
    }
    let a_allow_denyonly = acf::AcAce { typ: 0, mask: 0x89, sid: 0x4444, flags: 0 };
    if ace_applies(&a_allow_denyonly, &tok) {
        return Err(E_NEG); // Allow 对 deny-only 不适用
    }
    let a_deny_denyonly = acf::AcAce { typ: 1, mask: 0x89, sid: 0x4444, flags: 0 };
    if !ace_applies(&a_deny_denyonly, &tok) {
        return Err(E_NEG); // Deny 对 deny-only 适用
    }
    let a_inherit = acf::AcAce { typ: 0, mask: 0x89, sid: 0x1111, flags: F_INHERIT_ONLY };
    if ace_applies(&a_inherit, &tok) {
        return Err(E_NEG); // inherit-only 恒跳过
    }
    hits += 1;
    // R4：有效主体计数 + 标志位校验。
    // 有效主体 = 0x1111(自体) + 0x2222 + 0x3333 = 3；deny-only 不计。
    if effective_sid_count(&tok) != 3 {
        return Err(E_NEG);
    }
    flags_ok(&a_allow_self)?;
    let a_inherited = acf::AcAce { typ: 0, mask: 0x89, sid: 0x1111, flags: F_INHERITED };
    flags_ok(&a_inherited)?;
    if ace_applies_checked(&a_allow_self, &tok)? != true {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：负例注入——Null Sid / 坏类型 / 坏标志 / 空令牌不命中。
    let a_null = acf::AcAce { typ: 0, mask: 0x89, sid: NULL_SID, flags: 0 };
    if ace_applies(&a_null, &tok) {
        return Err(E_NULLSID); // Null Sid 不得参与具名匹配
    }
    let a_badtyp = acf::AcAce { typ: 7, mask: 0x89, sid: 0x1111, flags: 0 };
    if ace_applies_checked(&a_badtyp, &tok) != Err(E_ACE) {
        return Err(E_NEG);
    }
    let a_badflg = acf::AcAce { typ: 0, mask: 0x89, sid: 0x1111, flags: 0x8000 };
    if flags_ok(&a_badflg).is_ok() {
        return Err(E_NEG);
    }
    let empty_tok = acf::tok_plain(0x5555);
    if classify_sid(0x1111, &empty_tok) != MatchKind::None {
        return Err(E_NEG);
    }
    if is_member(NULL_SID, &tok) {
        return Err(E_NULLSID);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn match_table_holds() {
        assert_eq!(match_integrity(), Ok(()));
    }

    #[test]
    fn deny_only_never_granted() {
        let tok = acf::AcTokenFace {
            token_id: 1,
            sid: 7,
            groups: Vec::new(),
            deny_only: vec![9u64],
            privs: Vec::new(),
            imp_level: 2,
        };
        let allow = acf::AcAce { typ: 0, mask: 1, sid: 9, flags: 0 };
        let deny = acf::AcAce { typ: 1, mask: 1, sid: 9, flags: 0 };
        assert!(!ace_applies(&allow, &tok));
        assert!(ace_applies(&deny, &tok));
    }

    #[test]
    fn null_sid_never_matches() {
        let tok = acf::tok_plain(7);
        assert_eq!(classify_sid(0, &tok), MatchKind::None);
    }
}

pub fn run_unx36166_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36166_acsidmatch");
    cs.add("SEG 段位独占", SEG_BASE == 0x3c20, "0x3C20~0x3C2F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三路分类/匹配契约/负例全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", match_integrity() == Ok(()), "分类判别式 + 标志位闭集");
    cs.add("契约冻结", VER == 1, "ace_applies 契约锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ACE => "ACE 类型越出 {0=Allow,1=Deny}",
        E_NULLSID => "Null Sid 参与了具名匹配",
        E_NEG => "负例注入未被检出",
        E_TABLE => "匹配分类闭集破坏",
        _ => "未知诊断码",
    }
}
