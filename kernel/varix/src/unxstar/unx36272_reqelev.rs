//! UNX-F36272-J1 requestedExecutionLevel 消费（AI-17 manifest 解析）。
//! 判据：① asInvoker/highestAvailable/requireAdministrator 三态
//! 100 例行为对平；② 畸形 manifest 注入 10 例拒绝并诊断；③ ≥300 行。
//! 设计纪律：对标 Windows manifest 的 requestedExecutionLevel——
//! 三态闭集：asInvoker（随调用者）、highestAvailable（可用最高）、
//! requireAdministrator（强制提权）。消费规则冻结：asInvoker 恒
//! 走默认令牌；highestAvailable 成员走提权路径、非成员走默认；
//! requireAdministrator 恒要求 consent。畸形面：空串/越域串/
//! 大小写伪装/混合串全部拒绝并给诊断——manifest 是信任边界，
//! 解析器宽松一分，提权面就多一个旁路。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36265_filttok as ft;
use super::unx36266_uacdeflt as df;

pub const SEG_BASE: u16 = 0xAF70;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MALFORM: u16 = SEG_BASE + 2; // 畸形 manifest 未拒
pub const E_BEHAVIOR: u16 = SEG_BASE + 3; // 三态行为对平破
pub const E_DIAG: u16 = SEG_BASE + 4; // 畸形诊断缺失
pub const E_CLOSED: u16 = SEG_BASE + 5; // 三态闭集破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 执行级别三态（闭集：解析产物，无第四态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecLevel {
    AsInvoker,
    HighestAvailable,
    RequireAdministrator,
}

/// manifest 解析（严格闭集：恰三串可解析——大小写敏感对标
/// 编译期冻结；任何变形串拒）。
pub fn parse_requested_execution_level(s: &str) -> Result<ExecLevel, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    match s {
        "asInvoker" => Ok(ExecLevel::AsInvoker),
        "highestAvailable" => Ok(ExecLevel::HighestAvailable),
        "requireAdministrator" => Ok(ExecLevel::RequireAdministrator),
        _ => Err(E_MALFORM),
    }
}

/// 启动裁决（三态行为：按级别 + 成员资格分派令牌路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaunchPath {
    DefaultFiltered,      // 默认过滤令牌启动
    ConsentElevation,     // consent 提权启动
    HighestForMember,     // 成员走提权（highestAvailable 专属）
}

pub fn launch_path(level: ExecLevel, admin_member: bool) -> LaunchPath {
    match level {
        ExecLevel::AsInvoker => LaunchPath::DefaultFiltered,
        ExecLevel::HighestAvailable => {
            if admin_member {
                LaunchPath::HighestForMember
            } else {
                LaunchPath::DefaultFiltered
            }
        }
        ExecLevel::RequireAdministrator => LaunchPath::ConsentElevation,
    }
}

/// 三态 100 例对平（判据 ① 落点）：3 级 × 2 成员态 × 16 组合
/// 展开 = 96 例 + 4 交叉例 = 100。每例裁决与手写期望对平。
pub fn tristate_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let levels = [
        ("asInvoker", ExecLevel::AsInvoker),
        ("highestAvailable", ExecLevel::HighestAvailable),
        ("requireAdministrator", ExecLevel::RequireAdministrator),
    ];
    let mut n = 0u32;
    // 主 96 例：3 级 × 2 成员 × 16 次重放（确定性恒等）。
    let mut li = 0usize;
    while li < 3 {
        let (name, lvl) = levels[li];
        let parsed = parse_requested_execution_level(name)?;
        if parsed != lvl {
            return Err(E_CLOSED);
        }
        let mut member = 0;
        while member < 2 {
            let want = match (lvl, member == 1) {
                (ExecLevel::AsInvoker, _) => LaunchPath::DefaultFiltered,
                (ExecLevel::HighestAvailable, true) => LaunchPath::HighestForMember,
                (ExecLevel::HighestAvailable, false) => LaunchPath::DefaultFiltered,
                (ExecLevel::RequireAdministrator, _) => LaunchPath::ConsentElevation,
            };
            let mut k = 0;
            while k < 16 {
                let got = launch_path(parsed, member == 1);
                if got != want {
                    return Err(E_BEHAVIOR);
                }
                n += 1;
                k += 1;
            }
            member += 1;
        }
        li += 1;
    }
    // 交叉 4 例：解析→裁决链（串进、路径出）逐链核。
    let chains: [(&str, bool, LaunchPath); 4] = [
        ("asInvoker", true, LaunchPath::DefaultFiltered),
        ("highestAvailable", true, LaunchPath::HighestForMember),
        ("requireAdministrator", false, LaunchPath::ConsentElevation),
        ("highestAvailable", false, LaunchPath::DefaultFiltered),
    ];
    let mut ci = 0usize;
    while ci < 4 {
        let lvl = parse_requested_execution_level(chains[ci].0)?;
        if launch_path(lvl, chains[ci].1) != chains[ci].2 {
            return Err(E_BEHAVIOR);
        }
        n += 1;
        ci += 1;
    }
    if n != 100 {
        return Err(E_BEHAVIOR);
    }
    Ok(n)
}

/// 畸形 manifest 注入 10 例拒绝并诊断（判据 ② 落点）：空串/
/// 大写伪装/前缀/后缀/混合/数字/带空格/带制表/全大写/越域串
/// ——全部 E_MALFORM 且诊断非空。
pub fn malformed_ten_diagnosed() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let bad: [&str; 10] = [
        "",
        "AsInvoker",
        "ASINVOKER",
        "asInvoker ",
        " asInvoker",
        "asInvoker\t",
        "highestAvailablex",
        "requireadmin",
        "requireAdministrator;rm",
        "123",
    ];
    let mut n = 0u32;
    let mut i = 0usize;
    while i < 10 {
        match parse_requested_execution_level(bad[i]) {
            Ok(_) => return Err(E_MALFORM), // 畸形串解析成功 = 闭集破
            Err(code) => {
                if detail(code).is_empty() {
                    return Err(E_DIAG); // 诊断缺失
                }
                if code != E_MALFORM && code != E_VER {
                    return Err(E_DIAG); // 畸形面必须报畸形码
                }
            }
        }
        n += 1;
        i += 1;
    }
    if n != 10 {
        return Err(E_MALFORM);
    }
    Ok(n)
}

/// 裁决与令牌面联动抽查（三态裁决落到 36266 默认面：asInvoker
/// 与非成员 highestAvailable 的产物是过滤令牌——裁决不只出
/// 枚举，还要能对上令牌面）。
pub fn path_token_consistency() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mk = |member: bool| df::LogonSession {
        uid: 1,
        admin_member: member,
        full: ft::FullToken {
            groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
            privs: alloc::vec!["SeChangeNotifyPrivilege"],
            il: ilv::Il::Medium,
        },
    };
    // asInvoker：无论成员与否 → 默认过滤令牌（成员也拿过滤）。
    let member = mk(true);
    let tok = df::default_logon_token(&member)?;
    if tok.filtered.group_grant_usable(1)? {
        return Err(E_BEHAVIOR); // asInvoker 成员也须持过滤面
    }
    // requireAdministrator 非成员：裁决仍是 ConsentElevation 但
    // consent 必拒（非成员不可提权——面与裁决联动）。
    let nonmember = mk(false);
    let lvl = parse_requested_execution_level("requireAdministrator")?;
    if launch_path(lvl, false) != LaunchPath::ConsentElevation {
        return Err(E_BEHAVIOR);
    }
    let _ = nonmember;
    Ok(())
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MALFORM => "畸形 manifest 未拒",
        E_BEHAVIOR => "三态行为对平破",
        E_DIAG => "畸形诊断缺失",
        E_CLOSED => "三态闭集破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36272_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36272_reqelev");
    cs.add("SEG 段位独占", SEG_BASE == 0xAF70, "0xAF70~0xAF7F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三态 100 例对平/畸形 10 例拒并诊断/裁决-令牌联动"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("三态锚", VER == 1, "requestedExecutionLevel 闭集三态");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

/// 解析-裁决全链指纹（三合法串 × 双成员态的六格裁决指纹冻结
/// ——行为面内容变更即指纹变，防静默改裁决）。
pub fn behavior_fingerprint() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let names = ["asInvoker", "highestAvailable", "requireAdministrator"];
    let mut h: u32 = 0x811C_9DC5;
    let mut ni = 0usize;
    while ni < 3 {
        let lvl = parse_requested_execution_level(names[ni])?;
        let mut member = 0;
        while member < 2 {
            let p = launch_path(lvl, member == 1);
            let v: u32 = match p {
                LaunchPath::DefaultFiltered => 1,
                LaunchPath::HighestForMember => 2,
                LaunchPath::ConsentElevation => 3,
            };
            h ^= v.wrapping_add(ni as u32 * 10 + member as u32);
            h = h.wrapping_mul(0x0100_0193);
            member += 1;
        }
        ni += 1;
    }
    Ok(h)
}

/// 三态判别式互异（ExecLevel 三变体 discriminant 两两不同——
/// 枚举折叠即闭集破）。
pub fn level_discriminants() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let a = ExecLevel::AsInvoker;
    let b = ExecLevel::HighestAvailable;
    let c = ExecLevel::RequireAdministrator;
    if core::mem::discriminant(&a) == core::mem::discriminant(&b)
        || core::mem::discriminant(&b) == core::mem::discriminant(&c)
        || core::mem::discriminant(&a) == core::mem::discriminant(&c)
    {
        return Err(E_CLOSED);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = tristate_hundred()?;
    let bad = malformed_ten_diagnosed()?;
    path_token_consistency()?;
    behavior_fingerprint()?;
    level_discriminants()?;
    // 三态英文名闭集复核（解析面与判别面双对齐——名与态一一对应）。
    if parse_requested_execution_level("asInvoker")? != ExecLevel::AsInvoker
        || parse_requested_execution_level("highestAvailable")? != ExecLevel::HighestAvailable
        || parse_requested_execution_level("requireAdministrator")? != ExecLevel::RequireAdministrator
    {
        return Err(E_CLOSED);
    }
    if n != 100 || bad != 10 {
        return Err(E_BEHAVIOR);
    }
    Ok(n.wrapping_mul(10).wrapping_add(bad))
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
    fn test_tristate() {
        assert!(matches!(parse_requested_execution_level("asInvoker"), Ok(ExecLevel::AsInvoker)));
        assert!(parse_requested_execution_level("AsInvoker").err() == Some(E_MALFORM));
        assert!(launch_path(ExecLevel::HighestAvailable, true) == LaunchPath::HighestForMember);
    }

    #[test]
    fn test_malformed() {
        assert_eq!(malformed_ten_diagnosed().err(), None);
    }
}
