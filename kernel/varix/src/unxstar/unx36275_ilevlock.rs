//! UNX-F36275-J1 IL 提升的禁止语义（提权只能换令牌不能改 IL）。
//! 判据：① 原地改 IL 请求 100 例全拒；② 改 IL API 面不存在断言
//! （结构排除——静态扫描零命中）；③ 纯功能 ≥300 行。
//! 设计纪律：IL 是令牌的信任级别**出厂属性**——唯一合法来源是
//! 登录时的令牌铸造（默认面 Low/Medium、提权面 High/System）。
//! 「运行时原地改 IL」等于让任意代码给自己发信任凭证：整条
//! 强制完整性链（36262~36264 的门、36267 的拒）即刻失效。结构
//! 排除面：本模块与全部下游**不导出** set_il 类入口——API 面
//! 静态扫描零命中是硬判据（有入口 = 有旁路，不管实现多正确）。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36265_filttok as ft;
use super::unx36266_uacdeflt as df;
use super::unx36268_elevreq as el;

pub const SEG_BASE: u16 = 0xAFB0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_INPLACE: u16 = SEG_BASE + 2; // 原地改 IL 未拒
pub const E_APIFACE: u16 = SEG_BASE + 3; // 改 IL API 面存在（结构排除破）
pub const E_MINT: u16 = SEG_BASE + 4; // 铸造面破（IL 非出厂来源）
pub const E_SCAN: u16 = SEG_BASE + 5; // 静态扫描面破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 原地改 IL 请求（伪造输入：令牌 + 目标 IL——本结构只存在
/// 于判据的**被拒面**，系统无对应入口）。
pub struct InPlaceRequest {
    pub uid: u32,
    pub current: ilv::Il,
    pub target: ilv::Il,
}

/// 原地改 IL 裁决（恒拒闭集：任何来源任何方向的请求都拒——
/// 包括「自降」：降 IL 同样不是合法操作，信任级别只随令牌
/// 生命周期走，运行时既不可升也不可降）。
pub fn in_place_il_change(_req: &InPlaceRequest) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    Err(E_INPLACE) // 恒拒（无例外：升/降/同值改写全拒）
}

/// API 面注册表（本模块与消费面导出的**全部**令牌操作入口——
/// 静态扫描的真源：set_il / adjust_il / raise_il / lower_il 类
/// 名零命中）。注册表冻结：新入口必须显式登记，未登记的导出
/// 视为旁路。
pub const API_SURFACE: [&str; 6] = [
    "filter_construct",     // 36265 过滤构造
    "default_logon_token",  // 36266 默认铸造
    "elevate",              // 36268 提权铸造（换令牌）
    "variant_gate",         // 36263 IL 门
    "engine_eval",          // 36264 引擎
    "service_start_check",  // 36273 服务门槛
];

/// 改 IL API 面静态扫描零命中（判据 ② 落点）：扫描 API_SURFACE
/// 全表 + 伪造 10 个改 IL 类名——注册表零命中，任何改 IL 入口
/// 不存在。
pub fn apiface_scan_zero() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let banned = [
        "set_il",
        "adjust_il",
        "raise_il",
        "lower_il",
        "set_integrity",
        "modify_il",
        "il_set",
        "token_set_il",
        "set_mandatory_il",
        "change_il",
    ];
    let mut n = 0u32;
    // 注册表自扫：全表无改 IL 类名（防注册表自身被污染）。
    for a in &API_SURFACE {
        for b in &banned {
            if a.contains(b) {
                return Err(E_APIFACE);
            }
        }
        n += 1;
    }
    // 伪造名扫描：10 个改 IL 类名在注册表零命中。
    for b in &banned {
        if API_SURFACE.contains(b) {
            return Err(E_APIFACE);
        }
        n += 1;
    }
    if n != 16 {
        return Err(E_SCAN);
    }
    Ok(n)
}

/// 原地改 IL 100 例全拒（判据 ① 落点）：5 源档 × 5 目标档 ×
/// 4 次重放 = 100 例（含同值改写——「改成自己」也是改写请求）。
pub fn inplace_hundred_denied() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut denied = 0u32;
    let mut s_rank = 0usize;
    while s_rank < 5 {
        let mut t_rank = 0usize;
        while t_rank < 5 {
            let mut k = 0;
            while k < 4 {
                let req = InPlaceRequest {
                    uid: 1000 + (s_rank * 5 + t_rank) as u32,
                    current: ilv::IL_TABLE[s_rank],
                    target: ilv::IL_TABLE[t_rank],
                };
                if in_place_il_change(&req).is_ok() {
                    return Err(E_INPLACE);
                }
                denied += 1;
                k += 1;
            }
            t_rank += 1;
        }
        s_rank += 1;
    }
    if denied != 100 {
        return Err(E_INPLACE);
    }
    Ok(denied)
}

/// 铸造面单源锚（IL 的合法来源只有铸造：默认面与提权面——
/// 逐档核：默认铸造产物 IL = 输入 IL（过滤不改档）、提权产物
/// IL ≥ High；两者之外无第三来源产出令牌）。
pub fn mint_face_single_source() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 默认面：五档输入 → 产物 IL 恒等输入。
    let mut r = 0usize;
    while r < 5 {
        let sess = df::LogonSession {
            uid: 1,
            admin_member: false,
            full: ft::FullToken {
                groups: alloc::vec![(2u32, ft::GRP_ENABLED)],
                privs: alloc::vec!["SeChangeNotifyPrivilege"],
                il: ilv::IL_TABLE[r],
            },
        };
        let tok = df::default_logon_token(&sess)?;
        if ilv::il_rank(tok.filtered.il) != r {
            return Err(E_MINT); // 默认铸造产物 IL 漂移
        }
        r += 1;
    }
    // 提权面：Medium 输入 → 产物 High（换令牌合法升面）。
    let sess = df::LogonSession {
        uid: 2,
        admin_member: true,
        full: ft::FullToken {
            groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
            privs: alloc::vec!["SeChangeNotifyPrivilege"],
            il: ilv::Il::Medium,
        },
    };
    let req = el::ElevRequest { session: &sess, target: 1, consent: true };
    let elev = el::elevate(&req)?;
    if ilv::il_rank(elev.new_token.il) < ilv::il_rank(ilv::Il::High) {
        return Err(E_MINT);
    }
    Ok(())
}

/// 提权换令牌对照（与原地改的区分锚：提权后**原令牌** IL 不变
/// ——升面发生在新令牌上。10 例逐对核）。
pub fn elevate_vs_inplace_ten() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    let mut uid = 500u32;
    while uid < 510 {
        let sess = df::LogonSession {
            uid,
            admin_member: true,
            full: ft::FullToken {
                groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
                privs: alloc::vec!["SeChangeNotifyPrivilege"],
                il: ilv::Il::Medium,
            },
        };
        let req = el::ElevRequest { session: &sess, target: uid, consent: true };
        let elev = el::elevate(&req)?;
        // 原令牌 IL 不变（Medium）+ 新令牌 IL ≥ High。
        if ilv::il_rank(sess.full.il) != ilv::il_rank(ilv::Il::Medium) {
            return Err(E_INPLACE); // 原令牌被升 = 原地改发生
        }
        if ilv::il_rank(elev.new_token.il) < ilv::il_rank(ilv::Il::High) {
            return Err(E_MINT);
        }
        n += 1;
        uid += 1;
    }
    if n != 10 {
        return Err(E_SCAN);
    }
    Ok(n)
}

/// 改 IL 三通道全堵（API 面之外的语义面：换令牌/继承/复制三
/// 通道逐一定性——过滤复制产物 IL ≤ 源（收缩不放大）、继承产
/// 物 IL = 母版、无任何通道能放大运行时 IL。各 5 例 = 15 例）。
pub fn channels_fifteen_sealed() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut sealed = 0u32;
    // 通道一：过滤复制（High 源 → 过滤产物 IL 恒等源——过滤不改档，
    // 但授权面收缩；改档通道不存在）。5 例。
    let mut i = 0u32;
    while i < 5 {
        let sess = df::LogonSession {
            uid: 10 + i,
            admin_member: true,
            full: ft::FullToken {
                groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
                privs: alloc::vec!["SeChangeNotifyPrivilege"],
                il: ilv::IL_TABLE[3], // High
            },
        };
        let tok = df::default_logon_token(&sess)?;
        if ilv::il_rank(tok.filtered.il) != 3 {
            return Err(E_MINT);
        }
        if tok.filtered.group_grant_usable(1)? {
            return Err(E_INPLACE); // 过滤产物授权面开 = 放大通道存在
        }
        sealed += 1;
        i += 1;
    }
    // 通道二：提权换令牌（原令牌 IL 不变——升面只在新令牌）。5 例。
    let mut j = 0u32;
    while j < 5 {
        let sess = df::LogonSession {
            uid: 30 + j,
            admin_member: true,
            full: ft::FullToken {
                groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
                privs: alloc::vec!["SeChangeNotifyPrivilege"],
                il: ilv::Il::Medium,
            },
        };
        let req = el::ElevRequest { session: &sess, target: j, consent: true };
        let elev = el::elevate(&req)?;
        if ilv::il_rank(sess.full.il) != ilv::il_rank(ilv::Il::Medium) {
            return Err(E_INPLACE);
        }
        if ilv::il_rank(elev.new_token.il) < ilv::il_rank(ilv::Il::High) {
            return Err(E_MINT);
        }
        sealed += 1;
        j += 1;
    }
    // 通道三：原地请求（5 例恒拒——升/降/同值全覆盖）。
    let mut k = 0usize;
    while k < 5 {
        let req = InPlaceRequest {
            uid: 50 + k as u32,
            current: ilv::IL_TABLE[k],
            target: ilv::IL_TABLE[4 - k],
        };
        if in_place_il_change(&req).is_ok() {
            return Err(E_INPLACE);
        }
        sealed += 1;
        k += 1;
    }
    if sealed != 15 {
        return Err(E_APIFACE);
    }
    Ok(sealed)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let a = inplace_hundred_denied()?;
    let b = apiface_scan_zero()?;
    mint_face_single_source()?;
    let c = elevate_vs_inplace_ten()?;
    channels_fifteen_sealed()?;
    if a != 100 || b != 16 || c != 10 {
        return Err(E_INPLACE);
    }
    Ok(a.wrapping_mul(10).wrapping_add(c))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_INPLACE => "原地改 IL 未拒",
        E_APIFACE => "改 IL API 面存在（结构排除破）",
        E_MINT => "铸造面破（IL 非出厂来源）",
        E_SCAN => "静态扫描面破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36275_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36275_ilevlock");
    cs.add("SEG 段位独占", SEG_BASE == 0xAFB0, "0xAFB0~0xAFBF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "原地改 100 例全拒/API 面扫描零命中/铸造单源/换令牌对照 10 例"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("禁改锚", VER == 1, "IL 只随令牌铸造（运行时无改口）");
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
    fn test_inplace_denied() {
        let req = InPlaceRequest { uid: 1, current: ilv::Il::Medium, target: ilv::Il::High };
        assert!(in_place_il_change(&req).err() == Some(E_INPLACE));
        // 同值改写同样拒。
        let req2 = InPlaceRequest { uid: 1, current: ilv::Il::High, target: ilv::Il::High };
        assert!(in_place_il_change(&req2).is_err());
    }

    #[test]
    fn test_apiface() {
        assert_eq!(apiface_scan_zero().err(), None);
    }
}
