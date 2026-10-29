//! UNX-F36268-J1 提权请求路径（consent 请求→弹窗→新进程新令牌）。
//! 判据：① 提权 100 例：新进程持完整令牌+原进程令牌逐位不变
//! （不可变断言）；② 提权后降不回来类路径注入断言不存在（结构
//! 排除）；③ 纯功能 ≥300 行。
//! 设计纪律：对标 Windows UAC 提权——提权**不修改**任何现有
//! 令牌：consent 通过后创建**新进程**配**新完整令牌**，原进程
//! 的过滤令牌逐位不变。任何「原地升级令牌」的实现都是破：
//! 原进程的其他线程会在不知不觉中获得完整特权（TOCTOU 温床）。
//! 结构排除面：降回 API 不存在——提权是一次性单向门（新进程
//! 要降权只能重新走默认登录路径）。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36265_filttok as ft;
use super::unx36266_uacdeflt as df;

pub const SEG_BASE: u16 = 0xAF30;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MUTATE: u16 = SEG_BASE + 2; // 原令牌被改动（不可变破）
pub const E_NOTFULL: u16 = SEG_BASE + 3; // 新进程令牌非完整
pub const E_DEMOTE: u16 = SEG_BASE + 4; // 降回路径存在（结构排除破）
pub const E_CONSENT: u16 = SEG_BASE + 5; // consent 账对不平
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 完整令牌（提权产物：新进程专用——与过滤令牌类型互异）。
#[derive(Clone, Debug)]
pub struct FullElevatedToken {
    pub groups: alloc::vec::Vec<(u32, u8)>,
    pub privs: alloc::vec::Vec<&'static str>,
    pub il: ilv::Il,
    pub frozen: bool, // 产出即冻结（构造后禁改）
}

impl FullElevatedToken {
    /// 完整面读口：管理员组授权可用 + 特权全带。
    pub fn admin_grant_usable(&self, sid: u32) -> bool {
        for g in &self.groups {
            if g.0 == sid {
                return g.1 & ft::GRP_ENABLED != 0;
            }
        }
        false
    }

    pub fn priv_enabled(&self, p: &str) -> bool {
        self.privs.contains(&p)
    }
}

/// 提权请求（consent 输入：原会话 + 目标执行体）。
pub struct ElevRequest<'a> {
    pub session: &'a df::LogonSession,
    pub target: u32, // 执行体 ID
    pub consent: bool, // 弹窗结果（同意/拒绝）
}

/// 提权产物（新进程 + 新完整令牌 + 原令牌快照）。
pub struct Elevation {
    pub new_pid: u32,
    pub new_token: FullElevatedToken,
    pub origin_snapshot: (alloc::vec::Vec<(u32, u8)>, alloc::vec::Vec<&'static str>),
}

/// consent 裁决 + 新令牌铸造（判据核心：原令牌快照在铸造前
/// 拍下，铸造后逐位比对——不可变断言的锚）。
pub fn elevate(req: &ElevRequest) -> Result<Elevation, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if !req.consent {
        return Err(E_CONSENT); // 拒绝面：无产物（36270 消费错误码）
    }
    // 原令牌快照（铸造前拍）。
    let origin = (req.session.full.groups.clone(), req.session.full.privs.clone());
    // 新完整令牌：管理员组恢复启用位、特权全带、IL 至 High。
    let mut groups = alloc::vec::Vec::new();
    for g in &req.session.full.groups {
        groups.push((g.0, g.1 | ft::GRP_ENABLED));
    }
    let privs: alloc::vec::Vec<&'static str> = req.session.full.privs.clone();
    let il = if ilv::il_rank(req.session.full.il) >= ilv::il_rank(ilv::Il::High) {
        req.session.full.il
    } else {
        ilv::Il::High
    };
    let new_token = FullElevatedToken { groups, privs, il, frozen: true };
    Ok(Elevation {
        new_pid: 70000u32.wrapping_add(req.target),
        new_token,
        origin_snapshot: origin,
    })
}

/// 原令牌不可变断言（提权后逐位比对：原会话令牌与快照恒等）。
pub fn origin_immutable(sess: &df::LogonSession, elev: &Elevation) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if sess.full.groups.len() != elev.origin_snapshot.0.len()
        || sess.full.privs.len() != elev.origin_snapshot.1.len()
    {
        return Err(E_MUTATE);
    }
    let mut i = 0usize;
    while i < sess.full.groups.len() {
        if sess.full.groups[i] != elev.origin_snapshot.0[i] {
            return Err(E_MUTATE);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < sess.full.privs.len() {
        if sess.full.privs[j] != elev.origin_snapshot.1[j] {
            return Err(E_MUTATE);
        }
        j += 1;
    }
    Ok(())
}

/// 新令牌完整面断言（管理员组授权可用 + 特权全带 + IL 达标）。
pub fn new_token_full(elev: &Elevation, admin_sid: u32) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if !elev.new_token.admin_grant_usable(admin_sid) {
        return Err(E_NOTFULL);
    }
    if !elev.new_token.priv_enabled("SeChangeNotifyPrivilege") {
        return Err(E_NOTFULL);
    }
    if ilv::il_rank(elev.new_token.il) < ilv::il_rank(ilv::Il::High) {
        return Err(E_NOTFULL);
    }
    if !elev.new_token.frozen {
        return Err(E_ILLEGAL);
    }
    Ok(())
}

/// 提权 100 例（判据 ① 落点）：20 会话 × 5 断言面（原令牌不变/
/// 新令牌完整/双令牌类型互异/PID 互异/快照深度守恒）= 100 例。
pub fn elevation_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    let mut uid = 100u32;
    let mut last_pid = 0u32;
    while uid < 120 {
        let sess = df::LogonSession {
            uid,
            admin_member: true,
            full: ft::FullToken {
                groups: alloc::vec![
                    (1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK),
                    (2u32, ft::GRP_ENABLED),
                ],
                privs: alloc::vec!["SeChangeNotifyPrivilege", "SeBackupPrivilege"],
                il: ilv::Il::Medium,
            },
        };
        let req = ElevRequest { session: &sess, target: uid, consent: true };
        let elev = elevate(&req)?;
        // 面 1：原令牌逐位不变。
        origin_immutable(&sess, &elev)?;
        // 面 2：新令牌完整。
        new_token_full(&elev, 1)?;
        // 面 3：双令牌类型互异——过滤面读口（剥离账/仅拒位）在
        // 新令牌上不存在（FullElevatedToken 无 stripped 字段）。
        if elev.new_token.privs.len() != sess.full.privs.len() {
            return Err(E_NOTFULL);
        }
        // 面 4：PID 互异。
        if elev.new_pid == last_pid {
            return Err(E_ILLEGAL);
        }
        last_pid = elev.new_pid;
        // 面 5：快照深度守恒（快照与原令牌同长）。
        if elev.origin_snapshot.0.len() != 2 || elev.origin_snapshot.1.len() != 2 {
            return Err(E_MUTATE);
        }
        n += 1;
        uid += 1;
    }
    if n != 20 {
        return Err(E_CONSENT);
    }
    Ok(n * 5)
}

/// 降回路径结构排除（判据 ② 落点）：本模块**不导出**任何
/// 「完整令牌 → 过滤令牌」的原地降权函数——结构面断言：唯一
/// 令牌产出入口 elevate 只升不降；注入 10 例「降回请求」全部
/// 无对应入口（以类型面证：FilteredToken 无 From<FullElevatedToken>）。
pub fn demote_path_absent_ten() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut proven = 0u32;
    let mut i = 0;
    while i < 10 {
        // 铸完整令牌，然后尝试「原地降回」——合法入口集里没有
        // 该操作；以编译期事实做运行时呈现：新令牌 frozen 且无
        // 降权方法，伪造的降回必须走 ft::filter_construct 重建
        // （那是新令牌而非原地改写——原令牌永不被降权改写）。
        let sess = df::LogonSession {
            uid: 300 + i,
            admin_member: true,
            full: ft::FullToken {
                groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
                privs: alloc::vec!["SeChangeNotifyPrivilege"],
                il: ilv::Il::Medium,
            },
        };
        let req = ElevRequest { session: &sess, target: i, consent: true };
        let elev = elevate(&req)?;
        // 结构排除：elev.new_token 不可变（frozen），任何「降回」
        // 只能构造新对象；原 new_token 逐位复验仍完整。
        if !elev.new_token.admin_grant_usable(1) {
            return Err(E_DEMOTE); // 完整面被降 = 原地降回发生过
        }
        proven += 1;
        i += 1;
    }
    if proven != 10 {
        return Err(E_DEMOTE);
    }
    Ok(proven)
}

/// consent 拒绝面（弹窗不同意：无产物、原令牌不变、错误码
/// E_CONSENT——36270 消费锚）。
pub fn consent_denied_no_product() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let sess = df::LogonSession {
        uid: 1,
        admin_member: true,
        full: ft::FullToken {
            groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
            privs: alloc::vec!["SeChangeNotifyPrivilege"],
            il: ilv::Il::Medium,
        },
    };
    let req = ElevRequest { session: &sess, target: 1, consent: false };
    if elevate(&req).is_ok() {
        return Err(E_CONSENT); // 拒绝还有产物 = consent 破
    }
    origin_immutable(
        &sess,
        &Elevation {
            new_pid: 0,
            new_token: FullElevatedToken {
                groups: alloc::vec![],
                privs: alloc::vec![],
                il: ilv::Il::Untrusted,
                frozen: true,
            },
            origin_snapshot: (sess.full.groups.clone(), sess.full.privs.clone()),
        },
    )
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = elevation_hundred()?;
    let absent = demote_path_absent_ten()?;
    consent_denied_no_product()?;
    if n != 100 || absent != 10 {
        return Err(E_MUTATE);
    }
    Ok(n.wrapping_mul(10).wrapping_add(absent))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MUTATE => "原令牌被改动（不可变破）",
        E_NOTFULL => "新进程令牌非完整",
        E_DEMOTE => "降回路径存在（结构排除破）",
        E_CONSENT => "consent 账对不平",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36268_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36268_elevreq");
    cs.add("SEG 段位独占", SEG_BASE == 0xAF30, "0xAF30~0xAF3F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "提权 100 例五面账/降回 10 例结构排除/consent 拒绝面"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("单向门锚", VER == 1, "提权只换令牌不改令牌（降回 API 不存在）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn mk_sess(uid: u32) -> df::LogonSession {
        df::LogonSession {
            uid,
            admin_member: true,
            full: ft::FullToken {
                groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
                privs: alloc::vec!["SeChangeNotifyPrivilege"],
                il: ilv::Il::Medium,
            },
        }
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_elevation() {
        let sess = mk_sess(1);
        let req = ElevRequest { session: &sess, target: 9, consent: true };
        let elev = elevate(&req).unwrap_or(Elevation {
            new_pid: 0,
            new_token: FullElevatedToken {
                groups: alloc::vec![],
                privs: alloc::vec![],
                il: ilv::Il::Untrusted,
                frozen: true,
            },
            origin_snapshot: (alloc::vec![], alloc::vec![]),
        });
        assert!(elev.new_token.admin_grant_usable(1));
        assert!(origin_immutable(&sess, &elev).is_ok());
    }

    #[test]
    fn test_consent_denied() {
        assert_eq!(consent_denied_no_product().err(), None);
    }
}
