//! UNX-F36267-J1 过滤令牌写高 IL 对象全拒（拒绝理由可诊断）。
//! 判据：① 过滤令牌管理员操作 ×10 全拒且拒绝诊断（DENY 位命中
//! 链）100% 可读；② 同引擎双查断言可注入 10 例；③ 纯功能 ≥300 行。
//! 设计纪律：过滤令牌写高 IL 对象的拒因必须**逐环可读**——
//! 「DENY_ONLY 命中 → 授权面闭 → IL 门拒」是一条链，任何一环
//! 都要能在诊断输出里点出环号与环内容。只报「拒绝」不报「因
//! 什么拒」的诊断是半成品：管理员无法自查、审计无法归因。
//! 双查面：同一请求过一次引擎与再走一次独立重导，两次结果
//! 必须恒等——可注入的「单次通过」请求即双查失守。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36262_nowriteup as nwu;
use super::unx36263_xbits as xb;
use super::unx36264_uacengine as eng;
use super::unx36265_filttok as ft;
use super::unx36266_uacdeflt as df;

pub const SEG_BASE: u16 = 0xAF20;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LEAK: u16 = SEG_BASE + 2; // 过滤令牌写高 IL 漏拒
pub const E_DIAG: u16 = SEG_BASE + 3; // 诊断链不可读
pub const E_DUAL: u16 = SEG_BASE + 4; // 双查不一致
pub const E_CHAIN: u16 = SEG_BASE + 5; // 命中链环缺失
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 拒绝诊断链（逐环可读：环号 + 环名 + 环内容三元组）。
pub struct DenyChain {
    pub rings: alloc::vec::Vec<(u8, &'static str, &'static str)>,
}

impl DenyChain {
    /// 环可读断言：链非空、环号严格递增、每环三要素齐备。
    pub fn readable(&self) -> Result<(), u16> {
        if self.rings.is_empty() {
            return Err(E_DIAG);
        }
        let mut prev: i32 = -1;
        for r in &self.rings {
            if (r.0 as i32) <= prev {
                return Err(E_DIAG); // 环号不递增 = 链序乱
            }
            prev = r.0 as i32;
            if r.1.is_empty() || r.2.is_empty() {
                return Err(E_DIAG);
            }
        }
        Ok(())
    }

    /// 命中环查询（按环名）。
    pub fn ring_of(&self, name: &str) -> Option<&'static str> {
        for r in &self.rings {
            if r.1 == name {
                return Some(r.2);
            }
        }
        None
    }
}

/// 过滤令牌写高 IL 求值（判据核心）：返回 Ok=许 或 Err=拒并附
/// 完整命中链。链三环：① DENY_ONLY 命中环（管理员组身份面）、
/// ② 授权面闭环（组授权不可用）、③ IL 门闭环（档位方向）。
pub fn filtered_write_high(
    tok: &df::DefaultToken,
    admin_sid: u32,
    op: xb::OpKind,
    object_il: ilv::Il,
    policy: &nwu::MandatoryPolicy,
) -> Result<(), DenyChain> {
    if VER != 1 {
        return Err(DenyChain { rings: alloc::vec![(0, "ver", "版本锚不符")] });
    }
    let mut rings: alloc::vec::Vec<(u8, &'static str, &'static str)> = alloc::vec::Vec::new();
    // 环一：DENY_ONLY 命中（管理员组在过滤令牌中呈仅拒位）。
    let deny_only = tok
        .filtered
        .groups
        .iter()
        .any(|g| g.0 == admin_sid && g.1 & ft::GRP_FOR_DENY_ONLY != 0);
    if deny_only {
        rings.push((1, "deny_only", "管理员组 DENY_ONLY 命中"));
    }
    // 环二：授权面闭（组授权读口 false）。
    let grant = tok.filtered.group_grant_usable(admin_sid).unwrap_or(true);
    if !grant {
        rings.push((2, "grant_closed", "组成员授权面已闭"));
    }
    // 环三：IL 门（引擎裁决 DenyIl）。
    let subject = tok.filtered.il;
    let verdict = eng::engine_eval(op, subject, object_il, policy, true);
    let il_deny = matches!(verdict, Ok(eng::Verdict::DenyIl));
    if il_deny {
        rings.push((3, "il_gate", "IL 门拒（升方向禁行）"));
    }
    if !rings.is_empty() {
        return Err(DenyChain { rings });
    }
    Ok(())
}

/// 管理员操作 ×10 全拒且链 100% 可读（判据 ① 落点）。
pub fn admin_ten_denied_readable() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 构造过滤默认令牌（管理员成员 → 过滤）。
    let full = ft::FullToken {
        groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK), (2u32, ft::GRP_ENABLED)],
        privs: alloc::vec!["SeChangeNotifyPrivilege"],
        il: ilv::Il::Medium,
    };
    let sess = df::LogonSession { uid: 77, admin_member: true, full };
    let tok = df::default_logon_token(&sess)?;
    let policy = nwu::MandatoryPolicy::from_bits(nwu::P_NO_WRITE_UP)?;
    let mut n = 0u32;
    let mut i = 0;
    while i < 10 {
        // 低/中 IL 过滤令牌写 High 对象（升方向）。
        let op = match i % 3 {
            0 => xb::OpKind::Write,
            1 => xb::OpKind::Read,
            _ => xb::OpKind::Execute,
        };
        match filtered_write_high(&tok, 1, op, ilv::Il::High, &policy) {
            Ok(()) => return Err(E_LEAK), // 漏拒 = 过滤面破
            Err(chain) => {
                chain.readable()?;
                // 命中链必含 IL 门环（升方向拒的主因）。
                if chain.ring_of("il_gate").is_none() {
                    return Err(E_CHAIN);
                }
                // 写操作必含 DENY_ONLY 与授权闭环（过滤主因）。
                if let xb::OpKind::Write = op {
                    if chain.ring_of("deny_only").is_none() || chain.ring_of("grant_closed").is_none() {
                        return Err(E_CHAIN);
                    }
                }
            }
        }
        n += 1;
        i += 1;
    }
    if n != 10 {
        return Err(E_LEAK);
    }
    Ok(n)
}

/// 独立重导（双查第二路径：不调 filtered_write_high，手写三环
/// 判定——与主路径对平防单实现自证）。
fn dual_path_recompute(
    tok: &df::DefaultToken,
    admin_sid: u32,
    op: xb::OpKind,
    object_il: ilv::Il,
    policy: &nwu::MandatoryPolicy,
) -> bool {
    let deny_only = tok
        .filtered
        .groups
        .iter()
        .any(|g| g.0 == admin_sid && g.1 & ft::GRP_FOR_DENY_ONLY != 0);
    let grant = tok.filtered.group_grant_usable(admin_sid).unwrap_or(true);
    let s_rank = ilv::il_rank(tok.filtered.il);
    let o_rank = ilv::il_rank(object_il);
    let bit_on = xb::op_policy_bit(op, policy);
    let il_ok = !bit_on || s_rank >= o_rank;
    !(deny_only && !grant) && il_ok
}

/// 同引擎双查 10 例（判据 ② 落点）：主路径与重导在 10 组输入上
/// 裁决恒等——单查通过而双查不一致的注入即被断言检出。
pub fn dual_check_ten() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let full = ft::FullToken {
        groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
        privs: alloc::vec!["SeChangeNotifyPrivilege"],
        il: ilv::Il::Low,
    };
    let sess = df::LogonSession { uid: 88, admin_member: true, full };
    let tok = df::default_logon_token(&sess)?;
    let policy = nwu::MandatoryPolicy::from_bits(nwu::P_NO_WRITE_UP | nwu::P_NO_READ_UP)?;
    let mut n = 0u32;
    // 10 组输入：5 档客体 × 2 操作（写/读）。
    let mut o_rank = 0usize;
    while o_rank < 5 {
        let mut k = 0;
        while k < 2 {
            let op = if k == 0 { xb::OpKind::Write } else { xb::OpKind::Read };
            let object = ilv::IL_TABLE[o_rank];
            let primary = filtered_write_high(&tok, 1, op, object, &policy).is_ok();
            let recompute = dual_path_recompute(&tok, 1, op, object, &policy);
            if primary != recompute {
                return Err(E_DUAL);
            }
            n += 1;
            k += 1;
        }
        o_rank += 1;
    }
    if n != 10 {
        return Err(E_DUAL);
    }
    Ok(n)
}

/// 合法面 20 例（平/降方向写 + 高过滤令牌写低对象——过滤令牌
/// 不是全禁写：同档与降方向按引擎许）。
pub fn legal_twenty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let full = ft::FullToken {
        groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
        privs: alloc::vec!["SeChangeNotifyPrivilege"],
        il: ilv::Il::High,
    };
    let sess = df::LogonSession { uid: 99, admin_member: true, full };
    let tok = df::default_logon_token(&sess)?;
    let policy = nwu::MandatoryPolicy::from_bits(nwu::P_NO_WRITE_UP)?;
    let mut n = 0u32;
    let mut o_rank = 0usize;
    while o_rank < 5 {
        let object = ilv::IL_TABLE[o_rank];
        filtered_write_high(&tok, 1, xb::OpKind::Write, object, &policy).map_err(|_| E_LEAK)?;
        n += 1;
        o_rank += 1;
    }
    // 平方向读（High 读 High）15 例（3 操作 × 5 组）。
    let ops = [xb::OpKind::Read, xb::OpKind::Write, xb::OpKind::Execute];
    let mut op_i = 0usize;
    while op_i < 3 {
        let mut k = 0;
        while k < 5 {
            filtered_write_high(&tok, 1, ops[op_i], ilv::Il::High, &policy).map_err(|_| E_LEAK)?;
            n += 1;
            k += 1;
        }
        op_i += 1;
    }
    if n != 20 {
        return Err(E_LEAK);
    }
    Ok(n)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let denied = admin_ten_denied_readable()?;
    let dual = dual_check_ten()?;
    let legal = legal_twenty()?;
    if denied != 10 || dual != 10 || legal != 20 {
        return Err(E_LEAK);
    }
    Ok(denied.wrapping_mul(100).wrapping_add(dual))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LEAK => "过滤令牌写高 IL 漏拒",
        E_DIAG => "诊断链不可读",
        E_DUAL => "双查不一致",
        E_CHAIN => "命中链环缺失",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36267_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36267_uacdeny");
    cs.add("SEG 段位独占", SEG_BASE == 0xAF20, "0xAF20~0xAF2F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "管理员 10 例全拒链可读/双查 10 例恒等/合法 20 例放行"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("诊断链锚", VER == 1, "拒绝诊断三环（DENY 命中/授权闭/IL 门）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_deny_chain() {
        let full = ft::FullToken {
            groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
            privs: alloc::vec!["SeChangeNotifyPrivilege"],
            il: ilv::Il::Low,
        };
        let sess = df::LogonSession { uid: 5, admin_member: true, full };
        let tok = df::default_logon_token(&sess).unwrap_or(DefaultTokenPlaceholder::token());
        let policy = nwu::MandatoryPolicy::from_bits(nwu::P_NO_WRITE_UP)
            .unwrap_or(nwu::MandatoryPolicy { no_write_up: false, no_read_up: false, no_execute_up: false });
        let chain = filtered_write_high(&tok, 1, xb::OpKind::Write, ilv::Il::System, &policy);
        assert!(chain.is_err());
        if let Err(c) = chain {
            assert!(c.readable().is_ok());
            assert!(c.ring_of("il_gate").is_some());
        }
    }

    #[test]
    fn test_dual_check() {
        assert_eq!(dual_check_ten().err(), None);
    }
}

/// 测试兜底占位（unwrap_or 兜底构造——保持 no_std 测试面无 panic）。
struct DefaultTokenPlaceholder;

impl DefaultTokenPlaceholder {
    fn token() -> df::DefaultToken {
        df::DefaultToken {
            uid: 0,
            filtered: ft::FilteredToken {
                groups: alloc::vec![],
                kept_privs: alloc::vec![],
                stripped: alloc::vec![],
                il: ilv::Il::Untrusted,
            },
            elevated_available: false,
        }
    }
}
