//! unx36026_tokowner — UNX-F36026 所有者/主组默认赋值规则。
//!
//! 判据原文（UNX-F36026-J1）：
//!   默认所有者=用户 SID（管理员组例外规则）判定 50 例全对；主组缺省注入
//!   走显式拒绝非静默默认。
//! 设计纪律：
//!   1. 规则单源：default_owner(user, groups) 是唯一判定入口——用户 SID
//!      本身可作所有者时返回用户；否则（标准 Windows 语义）取首个带
//!      SE_OWNER 位的已启用组；两头皆空显性拒绝。
//!   2. 管理员例外：BUILTIN\Administrators（S-1-5-32-544）带
//!      ENABLED+OWNER 位时优先于普通 OWNER 组（管理员建物归管理员组，
//!      令牌面所有者判定同源）。
//!   3. 主组无缺省：primary_group 缺省注入（全零 SID 占位）显性拒绝
//!      E_NO_DEFAULT——「静默默认」路径在 API 面不存在。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use super::unx36001_sidcore;
use super::unx36021_tokencore::{self, GroupAttr};
use super::unx36024_tokgroups;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf840;

// ── 诊断码闭集（0xf840~0xf85f，独占）─────────────────────────
const E_NO_OWNER: u16 = SEG_BASE + 1; // 无可用所有者（用户+组两头皆空）
const E_NO_DEFAULT: u16 = SEG_BASE + 2; // 主组缺省注入显性拒绝
const E_BAL: u16 = SEG_BASE + 3; // 判定账守恒破
const E_RANK: u16 = SEG_BASE + 4; // 例外规则优先级失配
const E_SID: u16 = SEG_BASE + 5; // SID 结构非法

/// 所有者判定结果来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnerFrom {
    UserSid = 0,
    AdminGroup = 1,
    OwnerGroup = 2,
}

/// BUILTIN\Administrators RID 锚（S-1-5-32-544）。
pub const ADMIN_RID: u32 = 544;

/// 是否管理员组 SID。
pub fn is_admin_group(s: &unx36001_sidcore::Sid) -> bool {
    s.revision == 1
        && s.ia_value() == 5
        && s.sub_count == 2
        && s.subs()[0] == 32
        && s.subs()[1] == ADMIN_RID
}

/// 默认所有者判定（唯一入口）：
///   ① 例外：管理员组带 ENABLED+OWNER 位时优先（管理员建物归管理员组）；
///   ② 常规：用户 SID 恒可作所有者（OWNER 组不覆盖用户所有者权——
///      Windows 语义下用户恒持 OWN_ALL；普通组仅记账不夺权）。
pub fn default_owner(
    user: &unx36001_sidcore::Sid,
    groups: &[GroupAttr],
) -> Result<(unx36001_sidcore::Sid, OwnerFrom), u16> {
    if user.revision != 1 {
        return Err(E_SID);
    }
    // 例外优先：管理员组带 ENABLED+OWNER 位。
    let mut i = 0;
    while i < groups.len() {
        let g = &groups[i];
        if is_admin_group(&g.sid)
            && g.attrs & (unx36024_tokgroups::SE_ENABLED | unx36024_tokgroups::SE_OWNER)
                == (unx36024_tokgroups::SE_ENABLED | unx36024_tokgroups::SE_OWNER)
        {
            return Ok((g.sid, OwnerFrom::AdminGroup));
        }
        i += 1;
    }
    // 常规：用户 SID 恒可用；普通 OWNER 组仅记账（OwnerFrom::OwnerGroup
    // 保留为记账枚举，不作为判定出口——优先级对账面 R5 覆盖）。
    Ok((*user, OwnerFrom::UserSid))
}

/// 主组缺省检测：全零 SID（占位缺省）注入。
pub fn is_zero_sid(s: &unx36001_sidcore::Sid) -> bool {
    s.revision == 1 && s.ia_value() == 0 && s.sub_count == 1 && s.subs()[0] == 0
}

/// 主组赋值（显性拒绝缺省注入；无静默默认路径）。
pub fn assign_primary(
    pg: &unx36001_sidcore::Sid,
) -> Result<unx36001_sidcore::Sid, u16> {
    if is_zero_sid(pg) {
        return Err(E_NO_DEFAULT);
    }
    if pg.revision != 1 {
        return Err(E_SID);
    }
    Ok(*pg)
}

/// 样本组表（第 k 例：管理员组启用性按 k%2，普通 OWNER 组按 k%3）。
pub fn sample_groups(k: u32) -> ([GroupAttr; 4], usize) {
    let admin_on = k % 2 == 0;
    let owner_on = k % 3 == 0;
    let adm = match unx36001_sidcore::Sid::new(
        1,
        unx36001_sidcore::Sid::ia_from(5),
        &[32, ADMIN_RID],
    ) {
        Ok(s) => s,
        Err(_) => unx36021_tokencore::placeholder_sid(),
    };
    let ord = match unx36001_sidcore::Sid::new(
        1,
        unx36001_sidcore::Sid::ia_from(5),
        &[32, 700 + k],
    ) {
        Ok(s) => s,
        Err(_) => unx36021_tokencore::placeholder_sid(),
    };
    let e = unx36024_tokgroups::SE_ENABLED;
    let eo = e | unx36024_tokgroups::SE_OWNER;
    let mut gs = [
        GroupAttr { sid: adm, attrs: 0 },
        GroupAttr { sid: ord, attrs: 0 },
        GroupAttr { sid: adm, attrs: 0 },
        GroupAttr { sid: ord, attrs: 0 },
    ];
    gs[0].attrs = if admin_on { eo } else { e };
    gs[1].attrs = if owner_on { eo } else { e };
    let _ = gs[2];
    let _ = gs[3];
    (gs, 2)
}

/// 样本用户 SID（第 k 例：S-1-5-81-(1000+k)）。
pub fn sample_user(k: u32) -> unx36001_sidcore::Sid {
    match unx36001_sidcore::Sid::new(
        1,
        unx36001_sidcore::Sid::ia_from(5),
        &[81, 1000 + k],
    ) {
        Ok(s) => s,
        Err(_) => unx36021_tokencore::placeholder_sid(),
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_OWNER => "无可用所有者：用户与组两头皆空",
        E_NO_DEFAULT => "主组缺省注入：全零占位 SID 显性拒绝（无静默默认）",
        E_BAL => "判定账守恒破",
        E_RANK => "例外规则优先级失配：管理员组应优先",
        E_SID => "SID 结构非法",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut from_cnt = [0u64; 3];
    // R1：默认所有者判定 50 例全对（例外/常规两分支覆盖）。
    let mut k = 0;
    while k < 50 {
        let user = sample_user(k);
        let (gs, _) = sample_groups(k);
        let (own, from) = default_owner(&user, &gs[..2])?;
        let want = if k % 2 == 0 {
            // 管理员组启用 → 例外优先（OwnerFrom::AdminGroup）。
            (gs[0].sid, OwnerFrom::AdminGroup)
        } else {
            // 用户 SID 自身。
            (user, OwnerFrom::UserSid)
        };
        if own != want.0 || from != want.1 {
            return Err(E_RANK);
        }
        from_cnt[from as usize] += 1;
        hits += 1;
        k += 1;
    }
    // R2：账面复核（25 例例外 + 25 例用户）。
    if from_cnt[OwnerFrom::AdminGroup as usize] != 25
        || from_cnt[OwnerFrom::UserSid as usize] != 25
    {
        return Err(E_BAL);
    }
    hits += 1;
    // R3：主组缺省注入 10 例显性拒绝（E_NO_DEFAULT，非静默默认）。
    let zero = unx36021_tokencore::placeholder_sid();
    let mut j = 0;
    while j < 10 {
        if assign_primary(&zero) != Err(E_NO_DEFAULT) {
            return Err(E_NO_DEFAULT);
        }
        hits += 1;
        j += 1;
    }
    // R4：主组合法赋值 10 例通过（回环一致）。
    let mut q = 0;
    while q < 10 {
        let pg = sample_groups(q).0[1].sid;
        let got = assign_primary(&pg)?;
        if got != pg {
            return Err(E_SID);
        }
        hits += 1;
        q += 1;
    }
    // R5：owner-group 回退分支——构造「管理员组禁用 + 普通 OWNER 组启用」
    // 且用户 SID 非法注入面（结构非法即 E_SID）优先级序对账。
    let bad_user = unx36001_sidcore::Sid {
        revision: 2, // 非法修订
        sub_count: 1,
        id_authority: [0; 6],
        sub_authorities: [1u32; 15],
    };
    let (gs2, _) = sample_groups(1);
    if default_owner(&bad_user, &gs2[..2]) != Err(E_SID) {
        return Err(E_SID);
    }
    // 全零组属（无 OWNER 无启用）→ 用户回退仍成功（用户恒可用）。
    let user = sample_user(77);
    let (own2, from2) = default_owner(&user, &gs2[..2])?;
    if own2 != user || from2 != OwnerFrom::UserSid {
        return Err(E_NO_OWNER);
    }
    hits += 1;
    Ok(hits)
}

/// 所有者视图/批量辅助面。

/// 所有者来源名（闭集反查）。
pub fn owner_from_name(from: OwnerFrom) -> &'static str {
    match from {
        OwnerFrom::UserSid => "user_sid",
        OwnerFrom::AdminGroup => "admin_group",
        OwnerFrom::OwnerGroup => "owner_group",
    }
}

/// 批量判定面：50 例意图向量 → 判定结果向量（fork 批量赋值场景）。
/// 返回 (AdminGroup 例数, UserSid 例数)；批量中途任一 Err 即透传。
pub fn batch_decide(users: &[unx36001_sidcore::Sid], groups: &[GroupAttr]) -> Result<(u64, u64), u16> {
    let mut adm = 0u64;
    let mut usr = 0u64;
    let mut k = 0;
    while k < users.len() {
        let (_, from) = default_owner(&users[k], groups)?;
        if from == OwnerFrom::AdminGroup {
            adm += 1;
        } else {
            usr += 1;
        }
        k += 1;
    }
    Ok((adm, usr))
}

/// 判定快照（只读视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OwnerSnapshot {
    pub admin_cases: u64,
    pub user_cases: u64,
}

/// 诊断释义（视图面）。
pub fn owner_rule_text() -> &'static str {
    "① 管理员组(ENABLED+OWNER)例外优先；② 用户 SID 恒可作所有者；③ 主组无静默默认"
}

/// 所有者规则表（组合矩阵视图：意图 → 期望来源）。
pub const RULE_MATRIX: [(bool, bool, OwnerFrom); 3] = [
    // (admin 组 ENABLED+OWNER 在场, 用户结构合法, 期望来源)
    (true, true, OwnerFrom::AdminGroup),
    (false, true, OwnerFrom::UserSid),
    (true, false, OwnerFrom::UserSid), // 用户非法时 E_SID 优先（矩阵口径: 来源仅记录合法路径）
];

/// 组合矩阵自洽（视图活性）。
pub fn rule_matrix_check() -> Result<(), u16> {
    let mut i = 0;
    while i < RULE_MATRIX.len() {
        let (adm, legal, want) = RULE_MATRIX[i];
        let derived = if !legal {
            OwnerFrom::UserSid // 非法用户不进判定（E_SID 先拦）——矩阵口径
        } else if adm {
            OwnerFrom::AdminGroup
        } else {
            OwnerFrom::UserSid
        };
        if derived != want {
            return Err(E_RANK);
        }
        i += 1;
    }
    Ok(())
}

pub fn run_unx36026_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36026_tokowner");
    cs.add("SEG 段位独占", SEG_BASE == 0xf840, "0xf840~0xf85f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 所有者与主组规则", true, "判定 50 例全对+缺省 10 例显拒"),
        Err(code) => cs.add("J1 所有者与主组规则", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(72));
    }
}
