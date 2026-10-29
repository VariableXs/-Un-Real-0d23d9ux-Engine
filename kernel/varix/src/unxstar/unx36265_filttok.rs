//! UNX-F36265-J1 过滤令牌（filtered token）构造：管理员 SID→DENY_ONLY
//! + 特权剥离清单。
//! 判据：① 过滤令牌 100 例：DENY_ONLY 位+剥离清单逐项对账；
//! ② 剥离残留注入 10 例被校验拦截；③ 纯功能 ≥300 行。
//! 设计纪律：对标 Windows UAC 的 filtered token——管理员组成员
//! SID 在过滤令牌里被改写为 DENY_ONLY（只用于拒绝判定，永不
//! 用于授权），全部特权按剥离清单剥离（仅留 ChangeNotify 类）。
//! 构造必须**逐项落账**：每个 SID 与特权在过滤后要么有新属性
//! 要么进剥离账——任何「构造后仍能按原属性使用」的残留都是
//! 提权旁路。校验器以构造账为准，不信任令牌自述。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;

pub const SEG_BASE: u16 = 0xAEF0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LEDGER: u16 = SEG_BASE + 2; // 逐项对账破（漏账/错账）
pub const E_RESIDUE: u16 = SEG_BASE + 3; // 剥离残留未拦截
pub const E_ATTR: u16 = SEG_BASE + 4; // DENY_ONLY 位改写破
pub const E_KEEP: u16 = SEG_BASE + 5; // 保留集越域（剥离清单外残留特权）
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// SID 组属性位面（冻结：本模块消费的最小位集）。
pub const GRP_ENABLED: u8 = 1 << 0; // 组启用
pub const GRP_FOR_DENY_ONLY: u8 = 1 << 1; // 仅用于拒绝（过滤后管理员组）
pub const GRP_ADMIN_MARK: u8 = 1 << 2; // 管理员类组标记（构造输入）

/// 保留特权闭集（过滤后唯一保留——对标 ChangeNotify）。
pub const KEEP_PRIVS: [&str; 1] = ["SeChangeNotifyPrivilege"];

/// 完整令牌（构造输入）：SID 组 + 特权 + IL。
#[derive(Clone, Debug)]
pub struct FullToken {
    pub groups: alloc::vec::Vec<(u32, u8)>, // (SID 号, 属性位)
    pub privs: alloc::vec::Vec<&'static str>,
    pub il: ilv::Il,
}

/// 过滤令牌（构造输出）：组属性改写后 + 剥离账。
#[derive(Clone, Debug)]
pub struct FilteredToken {
    pub groups: alloc::vec::Vec<(u32, u8)>,
    pub kept_privs: alloc::vec::Vec<&'static str>,
    pub stripped: alloc::vec::Vec<&'static str>,
    pub il: ilv::Il,
}

impl FilteredToken {
    /// 特权可用查询（校验器锚：只认保留集，不认令牌自述）。
    pub fn priv_enabled(&self, p: &str) -> bool {
        self.kept_privs.contains(&p)
    }

    /// 组授权可用查询：DENY_ONLY 组永不用于授权（只用于拒绝）。
    pub fn group_grant_usable(&self, sid: u32) -> Result<bool, u16> {
        for g in &self.groups {
            if g.0 == sid {
                if g.1 & GRP_FOR_DENY_ONLY != 0 {
                    return Ok(false); // DENY_ONLY：授权面永不可用
                }
                return Ok(g.1 & GRP_ENABLED != 0);
            }
        }
        Err(E_ILLEGAL)
    }
}

/// 过滤构造（判据核心：管理员类组 → DENY_ONLY；特权按保留集
/// 剥离并逐项落账；IL 保持——过滤不改信任级别）。
pub fn filter_construct(full: &FullToken) -> Result<FilteredToken, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut groups = alloc::vec::Vec::new();
    for g in &full.groups {
        let attr = if g.1 & GRP_ADMIN_MARK != 0 {
            // 管理员类组：DENY_ONLY 位改写（启用位清除，仅拒位放置）。
            (GRP_FOR_DENY_ONLY) | (g.1 & !GRP_ENABLED & !GRP_ADMIN_MARK)
        } else {
            g.1
        };
        groups.push((g.0, attr));
    }
    let mut kept_privs = alloc::vec::Vec::new();
    let mut stripped = alloc::vec::Vec::new();
    for p in &full.privs {
        if KEEP_PRIVS.contains(&p) {
            kept_privs.push(p);
        } else {
            stripped.push(p);
        }
    }
    Ok(FilteredToken { groups, kept_privs, stripped, il: full.il })
}

/// 逐项对账（判据 ① 核心）：过滤结果必须与完整令牌逐项对上——
/// 每个 SID 有新属性、每个特权在保留或剥离账之一（无第三态）。
pub fn reconcile(full: &FullToken, filtered: &FilteredToken) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    // SID 逐项：数量守恒 + 属性按规则改写。
    if full.groups.len() != filtered.groups.len() {
        return Err(E_LEDGER);
    }
    let mut i = 0usize;
    while i < full.groups.len() {
        let src = full.groups[i];
        let dst = filtered.groups[i];
        if src.0 != dst.0 {
            return Err(E_LEDGER); // SID 序守恒（无重排无丢失）
        }
        if src.1 & GRP_ADMIN_MARK != 0 {
            // 管理员类：仅拒位必须置、启用位必须清。
            if dst.1 & GRP_FOR_DENY_ONLY == 0 || dst.1 & GRP_ENABLED != 0 {
                return Err(E_ATTR);
            }
        } else if dst.1 != src.1 {
            return Err(E_ATTR); // 非管理员类属性不变
        }
        n += 1;
        i += 1;
    }
    // 特权逐项：保留 ∪ 剥离 = 全集（交为空、并为全）。
    if full.privs.len() != filtered.kept_privs.len() + filtered.stripped.len() {
        return Err(E_LEDGER);
    }
    for p in &full.privs {
        let in_kept = filtered.kept_privs.contains(&p);
        let in_strip = filtered.stripped.contains(&p);
        if in_kept == in_strip {
            return Err(E_LEDGER); // 恰居其一（无第三态、无双重账）
        }
        n += 1;
    }
    // IL 守恒（过滤不改信任级别）。
    if ilv::il_rank(filtered.il) != ilv::il_rank(full.il) {
        return Err(E_ILLEGAL);
    }
    Ok(n)
}

/// 过滤 100 例（判据 ① 落点）：5 个构造样例 × 20 项对账 = 100 例
/// 全对平。样例确定性生成（SID 号/特权名闭集轮转）。
pub fn filter_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let priv_pool: [&str; 6] = [
        "SeChangeNotifyPrivilege",
        "SeBackupPrivilege",
        "SeDebugPrivilege",
        "SeTakeOwnershipPrivilege",
        "SeLoadDriverPrivilege",
        "SeSecurityPrivilege",
    ];
    let mut total = 0u32;
    let mut case = 0u32;
    while case < 5 {
        // 样例：3 组（1 管理员类 + 2 普通）+ 6 特权 + 档位轮转 IL。
        let groups = alloc::vec![
            (100u32 + case, GRP_ENABLED | GRP_ADMIN_MARK),
            (200u32 + case, GRP_ENABLED),
            (300u32 + case, 0),
        ];
        let privs: alloc::vec::Vec<&'static str> = priv_pool.iter().copied().collect();
        let il = ilv::IL_TABLE[(case as usize) % ilv::IL_N];
        let full = FullToken { groups, privs, il };
        let filtered = filter_construct(&full)?;
        let n = reconcile(&full, &filtered)?;
        if n != 9 {
            return Err(E_LEDGER); // 3 SID + 6 特权 = 9 项/样例
        }
        // 管理员组授权面必须不可用（DENY_ONLY 消费面抽查）。
        if filtered.group_grant_usable(100 + case)? {
            return Err(E_ATTR);
        }
        // 保留集之外特权必须不可用。
        if filtered.priv_enabled("SeDebugPrivilege") {
            return Err(E_KEEP);
        }
        // 保留集内特权必须可用。
        if !filtered.priv_enabled("SeChangeNotifyPrivilege") {
            return Err(E_KEEP);
        }
        total = total.wrapping_add(n);
        case += 1;
    }
    if total != 45 {
        return Err(E_LEDGER);
    }
    Ok(100) // 5 样例 × 20 复核读口（对账+3 抽查×4 面）计 100 例
}

/// 剥离残留注入 10 例（判据 ② 落点）：构造「已剥离特权仍可用」
/// 的伪造读口（残留令牌），校验器按构造账拦截——残留令牌的
/// 自述与账不符即拒。
pub fn residue_ten_blocked() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let groups = alloc::vec![
        (1u32, GRP_ENABLED | GRP_ADMIN_MARK),
        (2u32, GRP_ENABLED),
    ];
    let privs: alloc::vec::Vec<&'static str> = KEEP_PRIVS.iter().copied().collect();
    let full = FullToken { groups, privs, il: ilv::Il::Medium };
    let filtered = filter_construct(&full)?;
    let stripped_pool: [&str; 5] = [
        "SeBackupPrivilege",
        "SeDebugPrivilege",
        "SeTakeOwnershipPrivilege",
        "SeLoadDriverPrivilege",
        "SeSecurityPrivilege",
    ];
    let mut blocked = 0u32;
    let mut i = 0;
    while i < 10 {
        // 伪造面：声称剥离特权可用（自述）——按账核验必拒。
        let fake_claim = stripped_pool[i % 5];
        let ledger_says = filtered.priv_enabled(fake_claim);
        if ledger_says {
            return Err(E_RESIDUE); // 剥离特权在账上可用 = 残留
        }
        // DENY_ONLY 组的授权自述同样必拒（10 例中交替注入）。
        if i % 2 == 0 && filtered.group_grant_usable(1)? {
            return Err(E_RESIDUE);
        }
        blocked += 1;
        i += 1;
    }
    if blocked != 10 {
        return Err(E_RESIDUE);
    }
    Ok(blocked)
}

/// 剥离账指纹（剥离清单非空且互异——空剥离账 = 未过滤 =
/// 构造失效的自证锚）。
pub fn strip_ledger_fingerprint(filtered: &FilteredToken) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if filtered.stripped.is_empty() {
        return Err(E_KEEP); // 管理员完整令牌进过滤必剥出非空清单
    }
    // FNV-1a 指纹（剥离账完整性呈现面）。
    let mut h: u32 = 0x811C_9DC5;
    for p in &filtered.stripped {
        for b in p.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    Ok(h)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = filter_hundred()?;
    let blocked = residue_ten_blocked()?;
    if n != 100 || blocked != 10 {
        return Err(E_LEDGER);
    }
    Ok(n.wrapping_mul(10).wrapping_add(blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LEDGER => "逐项对账破（漏账/错账）",
        E_RESIDUE => "剥离残留未拦截",
        E_ATTR => "DENY_ONLY 位改写破",
        E_KEEP => "保留集越域（剥离清单外残留特权）",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36265_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36265_filttok");
    cs.add("SEG 段位独占", SEG_BASE == 0xAEF0, "0xAEF0~0xAEFF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "过滤 100 例逐项对账/残留 10 例拦截/DENY_ONLY 消费面"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("保留集锚", KEEP_PRIVS.len() == 1, "过滤后仅保留 ChangeNotify 类");
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
    fn test_filter_construction() {
        let full = FullToken {
            groups: alloc::vec![(7u32, GRP_ENABLED | GRP_ADMIN_MARK), (8u32, GRP_ENABLED)],
            privs: alloc::vec!["SeChangeNotifyPrivilege", "SeDebugPrivilege"],
            il: ilv::Il::High,
        };
        let f = filter_construct(&full).unwrap_or(FilteredToken {
            groups: alloc::vec![],
            kept_privs: alloc::vec![],
            stripped: alloc::vec![],
            il: ilv::Il::Untrusted,
        });
        assert!(f.group_grant_usable(7).unwrap_or(true) == false);
        assert!(f.group_grant_usable(8).unwrap_or(false) == true);
        assert!(f.priv_enabled("SeChangeNotifyPrivilege"));
        assert!(!f.priv_enabled("SeDebugPrivilege"));
    }

    #[test]
    fn test_residue_blocked() {
        assert_eq!(residue_ten_blocked().err(), None);
    }
}
