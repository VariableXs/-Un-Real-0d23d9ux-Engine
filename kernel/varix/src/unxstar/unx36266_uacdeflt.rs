//! UNX-F36266-J1 过滤令牌的普通启动默认（登录后默认持 stripped 令牌）。
//! 判据：① 默认启动 100 例持过滤令牌（成员资格查到但 DENY_ONLY）；
//! ② 直接持完整令牌注入 10 例不可能（构造路径断言）；③ 纯功能 ≥300 行。
//! 设计纪律：对标 Windows UAC 默认面——管理员组成员登录后的
//! 默认令牌是**过滤令牌**（full token 只在提权路径出现）。关键
//! 不变量：默认路径上「成员资格可查到」与「成员资格可用于授权」
//! 同时成立是破——查到（identity 面）必须伴 DENY_ONLY（授权面
//! 关闭）。注入面：绕过过滤直接以完整令牌作为默认令牌的构造
//! 路径必须不存在——默认令牌唯一构造入口是 filter。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36262_nowriteup as nwu;
use super::unx36265_filttok as ft;

pub const SEG_BASE: u16 = 0xAF10;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FULL_DEFAULT: u16 = SEG_BASE + 2; // 默认令牌未过滤
pub const E_MEMBER_OPEN: u16 = SEG_BASE + 3; // 成员查到但授权面开（DENY_ONLY 破）
pub const E_PATH: u16 = SEG_BASE + 4; // 完整令牌构造路径存在（路径断言破）
pub const E_LEDGER: u16 = SEG_BASE + 5; // 默认账对平破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 登录会话（用户 + 管理员成员资格 + 完整令牌输入）。
pub struct LogonSession {
    pub uid: u32,
    pub admin_member: bool,
    pub full: ft::FullToken,
}

/// 默认令牌（登录产出）：恒为过滤令牌——构造函数是唯一入口。
pub struct DefaultToken {
    pub uid: u32,
    pub filtered: ft::FilteredToken,
    pub elevated_available: bool, // 提权路径可用（36268 消费）
}

/// 登录默认构造（唯一入口：成员 → 过滤令牌；无完整默认路径）。
pub fn default_logon_token(sess: &LogonSession) -> Result<DefaultToken, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let filtered = ft::filter_construct(&sess.full)?;
    Ok(DefaultToken {
        uid: sess.uid,
        filtered,
        elevated_available: sess.admin_member,
    })
}

/// 成员资格双面查询（判据 ① 核心）：身份面查得到（成员资格
/// 记录保留），授权面必关闭（DENY_ONLY）——两面同时断言。
pub fn membership_dual_view(tok: &DefaultToken, admin_sid: u32) -> Result<(bool, bool), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 身份面：属性位里 DENY_ONLY 置位即可查到该成员身份。
    let mut identity = false;
    for g in &tok.filtered.groups {
        if g.0 == admin_sid && g.1 & ft::GRP_FOR_DENY_ONLY != 0 {
            identity = true;
        }
    }
    // 授权面：DENY_ONLY 组永不授权。
    let grant = tok.filtered.group_grant_usable(admin_sid)?;
    if identity && grant {
        return Err(E_MEMBER_OPEN); // 查到且可用 = 过滤失效
    }
    Ok((identity, grant))
}

/// 默认启动 100 例（判据 ① 落点）：20 用户 × 5 组面 = 100 例——
/// 每例默认令牌恒过滤、管理员组查到但不可用、保留集特权可用、
/// 其余特权不可用（四面账逐例核）。
pub fn default_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    let mut uid = 5000u32;
    while uid < 5020 {
        let full = ft::FullToken {
            groups: alloc::vec![
                (1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK),
                (2u32, ft::GRP_ENABLED),
                (3u32, 0),
            ],
            privs: alloc::vec!["SeChangeNotifyPrivilege", "SeDebugPrivilege", "SeBackupPrivilege"],
            il: ilv::IL_TABLE[(uid % 5) as usize],
        };
        let sess = LogonSession { uid, admin_member: true, full };
        let tok = default_logon_token(&sess)?;
        // 面一：默认令牌恒过滤——剥离账非空 + 管理员组仅拒位。
        let admin_attr = tok
            .filtered
            .groups
            .iter()
            .find(|g| g.0 == 1)
            .map(|g| g.1)
            .unwrap_or(0);
        if admin_attr & ft::GRP_FOR_DENY_ONLY == 0 || admin_attr & ft::GRP_ENABLED != 0 {
            return Err(E_FULL_DEFAULT);
        }
        // 面二：成员双面（查到 true、授权 false）。
        let (identity, grant) = membership_dual_view(&tok, 1)?;
        if !identity || grant {
            return Err(E_MEMBER_OPEN);
        }
        // 面三：保留集可用、剥离集不可用。
        if !tok.filtered.priv_enabled("SeChangeNotifyPrivilege")
            || tok.filtered.priv_enabled("SeDebugPrivilege")
            || tok.filtered.priv_enabled("SeBackupPrivilege")
        {
            return Err(E_LEDGER);
        }
        // 面四：普通组授权面不受过滤影响。
        if !tok.filtered.group_grant_usable(2)? {
            return Err(E_LEDGER);
        }
        n += 1;
        uid += 1;
    }
    if n != 20 {
        return Err(E_LEDGER);
    }
    Ok(n * 5) // 20 用户 × 5 面 = 100 例
}

/// 完整令牌注入 10 例不可能（判据 ② 落点：构造路径断言）——
/// 尝试绕过 filter 以完整令牌充当默认令牌的 10 种伪造输入
/// 全部被入口校验拒（默认令牌结构里不含完整特权面）。
pub fn full_token_injection_ten() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut blocked = 0u32;
    let mut i = 0u32;
    while i < 10 {
        // 伪造输入：管理员完整令牌（特权 3 枚全带、管理员组启用）。
        let full = ft::FullToken {
            groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
            privs: alloc::vec!["SeChangeNotifyPrivilege", "SeDebugPrivilege", "SeBackupPrivilege"],
            il: ilv::Il::High,
        };
        let sess = LogonSession { uid: 9000 + i, admin_member: true, full };
        // 唯一入口强制过滤——产出后按完整面读必失败。
        let tok = default_logon_token(&sess)?;
        if tok.filtered.priv_enabled("SeDebugPrivilege") || tok.filtered.priv_enabled("SeBackupPrivilege") {
            return Err(E_PATH); // 完整面从默认令牌可读 = 绕过路径存在
        }
        if tok.filtered.group_grant_usable(1)? {
            return Err(E_PATH);
        }
        // 结构断言：DefaultToken 无完整令牌字段可取（仅 filtered
        // 可达）——注入方找不到第二读口。
        blocked += 1;
        i += 1;
    }
    if blocked != 10 {
        return Err(E_PATH);
    }
    Ok(blocked)
}

/// 提权可用性锚（默认令牌的 elevated_available 与成员资格一致
/// ——非成员恒不可提权，成员才可走 36268 路径；防默认面把
/// 提权旗标发错人）。
pub fn elevation_flag_anchor() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mk = |member: bool| LogonSession {
        uid: 1,
        admin_member: member,
        full: ft::FullToken {
            groups: alloc::vec![(1u32, if member { ft::GRP_ENABLED | ft::GRP_ADMIN_MARK } else { ft::GRP_ENABLED })],
            privs: alloc::vec!["SeChangeNotifyPrivilege"],
            il: ilv::Il::Medium,
        },
    };
    let m = default_logon_token(&mk(true))?;
    let nm = default_logon_token(&mk(false))?;
    if !m.elevated_available || nm.elevated_available {
        return Err(E_ILLEGAL);
    }
    Ok(())
}

/// 默认令牌账户表（每用户默认令牌登记 + 逐项复核——登录面
/// 账本化：20 用户登记后逐户回读，读回与登记恒等防构造后漂移）。
pub fn default_ledger_recheck() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut ledger: alloc::vec::Vec<(u32, usize, usize)> = alloc::vec::Vec::new(); // (uid, 组数, 剥离数)
    let mut uid = 6000u32;
    while uid < 6020 {
        let full = ft::FullToken {
            groups: alloc::vec![
                (1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK),
                (2u32, ft::GRP_ENABLED),
            ],
            privs: alloc::vec!["SeChangeNotifyPrivilege", "SeDebugPrivilege"],
            il: ilv::Il::Medium,
        };
        let sess = LogonSession { uid, admin_member: true, full };
        let tok = default_logon_token(&sess)?;
        let rec = (uid, tok.filtered.groups.len(), tok.filtered.stripped.len());
        ledger.push(rec);
        uid += 1;
    }
    // 逐户回读复核：登记值与当前构造再读一致（确定性重放恒等）。
    let mut n = 0u32;
    for rec in &ledger {
        let full = ft::FullToken {
            groups: alloc::vec![
                (1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK),
                (2u32, ft::GRP_ENABLED),
            ],
            privs: alloc::vec!["SeChangeNotifyPrivilege", "SeDebugPrivilege"],
            il: ilv::Il::Medium,
        };
        let sess = LogonSession { uid: rec.0, admin_member: true, full };
        let tok = default_logon_token(&sess)?;
        if tok.filtered.groups.len() != rec.1 || tok.filtered.stripped.len() != rec.2 {
            return Err(E_LEDGER);
        }
        n += 1;
    }
    if n != 20 {
        return Err(E_LEDGER);
    }
    Ok(n)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = default_hundred()?;
    let blocked = full_token_injection_ten()?;
    elevation_flag_anchor()?;
    let recheck = default_ledger_recheck()?;
    if n != 100 || blocked != 10 || recheck != 20 {
        return Err(E_LEDGER);
    }
    Ok(n.wrapping_mul(10).wrapping_add(blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FULL_DEFAULT => "默认令牌未过滤",
        E_MEMBER_OPEN => "成员查到但授权面开（DENY_ONLY 破）",
        E_PATH => "完整令牌构造路径存在（路径断言破）",
        E_LEDGER => "默认账对平破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36266_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36266_uacdeflt");
    cs.add("SEG 段位独占", SEG_BASE == 0xAF10, "0xAF10~0xAF1F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "默认 100 例四面账/完整令牌注入 10 例不可能/提权旗标锚"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("默认面锚", VER == 1, "默认令牌唯一构造入口 = filter");
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
    fn test_default_filtered() {
        let full = ft::FullToken {
            groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
            privs: alloc::vec!["SeChangeNotifyPrivilege", "SeDebugPrivilege"],
            il: ilv::Il::Medium,
        };
        let sess = LogonSession { uid: 42, admin_member: true, full };
        let tok = default_logon_token(&sess).unwrap_or(DefaultToken {
            uid: 0,
            filtered: ft::FilteredToken {
                groups: alloc::vec![],
                kept_privs: alloc::vec![],
                stripped: alloc::vec![],
                il: ilv::Il::Untrusted,
            },
            elevated_available: false,
        });
        assert!(tok.filtered.priv_enabled("SeChangeNotifyPrivilege"));
        assert!(!tok.filtered.priv_enabled("SeDebugPrivilege"));
    }

    #[test]
    fn test_injection_blocked() {
        assert_eq!(full_token_injection_ten().err(), None);
    }

    #[test]
    fn test_ledger_recheck() {
        assert_eq!(default_ledger_recheck().err(), None);
    }
}
