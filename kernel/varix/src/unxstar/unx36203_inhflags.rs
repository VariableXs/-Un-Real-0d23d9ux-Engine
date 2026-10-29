//! UNX-F36203-J1 继承传播标志（IS/ID 位）与已继承 ACE 识别。
//! 判据：① 传播标志写入/识别 100 例对平——写入侧（IS：继承副本
//! 恒置 INHERITED 位、显式 ACE 恒不带）与识别侧（ID：按 flags 位
//! 分类 inherited/explicit）双账对平，并与来源锚（origin）交叉恒等；
//! ② 伪已继承位注入 10 例被校验拦截（显式 DACL 携 ID 位被 set_dacl
//! 守卫拒；混入有效集的伪 ID ACE 被 provenance_audit 质检拒）；
//! ③ 纯功能代码 ≥320 行。
//! 设计纪律：ID 位（INHERITED_ACE，0x10）是继承引擎的「账目指纹」
//! ——规范序分组（36205）、溯源账（36212）、保护短路（36204）都
//! 靠它区分显式与继承。写入与识别必须单向恒等：写侧只对继承副本
//! 置位，识别侧只认位不认猜；两条路对不上 = 账目撕裂。来源锚
//! origin（36201：显式=0，继承=来源节点+1）是独立第二来源，三方
//! （flags 位 / origin / 结构位置）恒等才算对平。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7b50;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FLAG: u16 = SEG_BASE + 2; // 写入侧纪律破（副本缺 ID/显式带 ID）
pub const E_ID: u16 = SEG_BASE + 3; // 识别侧分类错
pub const E_FAKE: u16 = SEG_BASE + 4; // 伪已继承位未被拦截
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;

/// 对平例数（判据锚）。
pub const PAIR_N: usize = 100;
/// 伪已继承位注入数。
pub const FAKE_N: usize = 10;

/// 识别侧基元：按 flags 位分类（只认位，不认猜）。
pub fn is_inherited(a: &ie::Ace) -> bool {
    a.flags & ie::AI_INHERITED != 0
}

/// 分类账：(显式数, 继承数)。
pub fn classify(acl: &[ie::Ace]) -> (usize, usize) {
    let mut e = 0;
    let mut i = 0;
    while i < acl.len() {
        if is_inherited(&acl[i]) {
            i += 1;
        } else {
            e += 1;
            i += 1;
        }
    }
    (e, acl.len() - e)
}

/// 写入侧纪律核（IS）：继承副本恒置 ID、清 IO；显式恒无 ID。
pub fn writer_discipline(inherited: &[ie::Ace], explicit: &[ie::Ace]) -> Result<(), u16> {
    let mut i = 0;
    while i < inherited.len() {
        let a = &inherited[i];
        if a.flags & ie::AI_INHERITED == 0 || a.flags & ie::AI_INHERIT_ONLY != 0 {
            return Err(E_FLAG);
        }
        i += 1;
    }
    let mut k = 0;
    while k < explicit.len() {
        if explicit[k].flags & ie::AI_INHERITED != 0 {
            return Err(E_FLAG);
        }
        k += 1;
    }
    Ok(())
}

/// 溯源交叉恒等：ID 位 ⟺ origin ≠ 0（flags 与 origin 独立双来源）。
pub fn provenance_cross(acl: &[ie::Ace]) -> Result<(), u16> {
    let mut i = 0;
    while i < acl.len() {
        let a = &acl[i];
        if is_inherited(a) != (a.origin != 0) {
            return Err(E_ID);
        }
        i += 1;
    }
    Ok(())
}

/// 质检审计：混入有效集的伪 ID ACE（ID 置位但 origin=0，或 origin≠0
/// 却无 ID 位）显性拒——伪已继承位注入的拦截面。
pub fn provenance_audit(acl: &[ie::Ace]) -> Result<(usize, usize), u16> {
    provenance_cross(acl)?;
    Ok(classify(acl))
}

/// 显式面守卫（set_dacl 同口径的独立面）：显式 DACL 禁带 ID。
pub fn validate_explicit(aces: &[ie::Ace]) -> Result<(), u16> {
    let mut i = 0;
    while i < aces.len() {
        if aces[i].flags & ie::AI_INHERITED != 0 {
            return Err(E_FLAG);
        }
        i += 1;
    }
    Ok(())
}

/// 伪已继承位注入构造（10 例）：显式 ACE 携 ID 位 + 不同的 flags 变体
/// （OI/CI/NP/IO 交叉），全部应被 set_dacl 守卫拒绝。
pub fn fake_injected_dacl(case: usize) -> Result<Vec<ie::Ace>, u16> {
    if case >= FAKE_N {
        return Err(E_INTG);
    }
    let mut flags = ie::AI_INHERITED;
    if case % 2 == 0 {
        flags |= ie::AI_OBJECT_INHERIT;
    }
    if case % 3 == 0 {
        flags |= ie::AI_CONTAINER_INHERIT;
    }
    if case % 5 == 0 {
        flags |= ie::AI_NO_PROPAGATE;
    }
    if case % 7 == 0 {
        flags |= ie::AI_INHERIT_ONLY;
    }
    let mut aces: Vec<ie::Ace> = Vec::new();
    aces.push(ie::Ace { sid: 200 + case as u16, mask: 0x3, flags, kind: ie::AK_ALLOW, origin: 0 });
    Ok(aces)
}

/// 25 棵对平树（root 2 ACE / mid 1 ACE 变体）——每棵产出 root/mid/leaf
/// 三个有效集的分类例。
fn pair_tree(case: usize) -> Result<ie::InhTree, u16> {
    if case >= 25 {
        return Err(E_INTG);
    }
    let mut t = ie::InhTree::new();
    let root = t.add_node("root", None, true, ie::SE_DACL_PRESENT)?;
    let mid = t.add_node("mid", Some(root), true, ie::SE_DACL_PRESENT)?;
    t.add_node("leaf", Some(mid), false, ie::SE_DACL_PRESENT)?;
    let mut ra: Vec<ie::Ace> = Vec::new();
    ra.push(ie::Ace {
        sid: 10 + case as u16,
        mask: 0x7,
        flags: if case % 2 == 0 { ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT } else { ie::AI_CONTAINER_INHERIT },
        kind: ie::AK_ALLOW,
        origin: 0,
    });
    ra.push(ie::Ace {
        sid: 20 + case as u16,
        mask: 0x1,
        flags: if case % 3 == 0 { ie::AI_OBJECT_INHERIT } else { ie::AI_INHERIT_ONLY | ie::AI_OBJECT_INHERIT },
        kind: ie::AK_DENY,
        origin: 0,
    });
    let mut ma: Vec<ie::Ace> = Vec::new();
    ma.push(ie::Ace {
        sid: 30 + case as u16,
        mask: 0x2,
        flags: if case % 4 == 0 { ie::AI_OBJECT_INHERIT } else { 0 },
        kind: ie::AK_ALLOW,
        origin: 0,
    });
    t.set_dacl(root, ra)?;
    t.set_dacl(mid, ma)?;
    Ok(t)
}

/// 100 例写入/识别对平主扫（判据 ① 主面）：
///   - 写侧：collect_inherited 输出全带 ID 清 IO（writer_discipline）；
///   - 识侧：有效集逐 ACE 按位分类；
///   - 交叉：ID 位 ⟺ origin ≠ 0 三方恒等。
pub fn pair_sweep() -> Result<u32, u16> {
    let mut compared = 0u32;
    let mut case = 0;
    while case < 25 {
        let t = pair_tree(case)?;
        // 三观测点：root / mid / leaf（下标 0/1/2）。
        let mut node = 0;
        while node < 3 {
            let eff = t.effective_dacl(node)?;
            let inh = t.collect_inherited(node)?;
            writer_discipline(&inh, &t.nodes[node].aces)?;
            provenance_cross(&eff)?;
            let (e, i) = classify(&eff);
            if e + i != eff.len() {
                return Err(E_ID);
            }
            compared += eff.len() as u32;
            node += 1;
        }
        case += 1;
    }
    if compared < PAIR_N as u32 {
        return Err(E_INTG);
    }
    Ok(compared)
}

/// 伪已继承位注入扫描（10 例 set_dacl 守卫 + 10 例质检审计）。
pub fn fake_sweep() -> Result<u32, u16> {
    let mut caught = 0u32;
    let mut case = 0;
    while case < FAKE_N {
        let aces = fake_injected_dacl(case)?;
        // 路径 1：显式 DACL 守卫。
        let mut t = ie::InhTree::new();
        let root = t.add_node("root", None, true, 0)?;
        if t.set_dacl(root, aces.clone()).err() != Some(ie::E_FLAGS) {
            return Err(E_FAKE);
        }
        // 路径 2：混入有效集被质检拦出（伪 ID：origin=0 却带 ID 位）。
        if provenance_audit(&aces).is_ok() {
            return Err(E_FAKE);
        }
        caught += 1;
        case += 1;
    }
    Ok(caught)
}

// ---- 溯源统计面 -------------------------------------------------------------
/// 溯源统计：显式/继承两桶计数 + 掩码聚合（OR = 授权并、AND = 授权
/// 交）+ 来源节点去重数。计数账与 classify 双机对位；OR ⊇ AND 是
/// 掩码聚合的形状锚（交集的每一位必在并集中）。
pub fn provenance_stats(acl: &[ie::Ace]) -> Result<(usize, usize, u32, u32, usize), u16> {
    let mut e = 0usize;
    let mut i = 0usize;
    let mut mor = 0u32;
    let mut mand = u32::MAX;
    let mut seen: Vec<u32> = Vec::new();
    let mut k = 0;
    while k < acl.len() {
        let a = &acl[k];
        if is_inherited(a) {
            i += 1;
        } else {
            e += 1;
        }
        mor |= a.mask;
        mand &= a.mask;
        let mut dup = false;
        let mut s = 0;
        while s < seen.len() {
            if seen[s] == a.origin {
                dup = true;
                break;
            }
            s += 1;
        }
        if !dup {
            seen.push(a.origin);
        }
        k += 1;
    }
    if e + i != acl.len() {
        return Err(E_ID);
    }
    if !acl.is_empty() && (mor & mand) != mand {
        return Err(E_INTG);
    }
    Ok((e, i, mor, mand, seen.len()))
}

/// 统计面对平扫（25 棵 × 3 观测点）：provenance_stats 与 classify
/// 双机计数恒等 + OR ⊇ AND 形状锚。
pub fn stats_cross_check() -> Result<u32, u16> {
    let mut checked = 0u32;
    let mut case = 0;
    while case < 25 {
        let t = pair_tree(case)?;
        let mut node = 0;
        while node < 3 {
            let eff = t.effective_dacl(node)?;
            let (e, i) = classify(&eff);
            let (e2, i2, _mor, _mand, _src) = provenance_stats(&eff)?;
            if e != e2 || i != i2 {
                return Err(E_ID);
            }
            checked += 1;
            node += 1;
        }
        case += 1;
    }
    Ok(checked)
}

// ---- 重物化对平（可重放锚） -------------------------------------------------
/// 同参重建 pair_tree(case)，新旧两树有效集逐 ACE 全字段对平——
/// 确定性传播 = 同输入同输出；任何一位漂移（尤其 origin/ID 位）都
/// 是传播实现退化的直接证据。
pub fn rematerialize_check() -> Result<u32, u16> {
    let mut checked = 0u32;
    let mut case = 0;
    while case < 25 {
        let t1 = pair_tree(case)?;
        let t2 = pair_tree(case)?;
        let mut node = 0;
        while node < 3 {
            let a1 = t1.effective_dacl(node)?;
            let a2 = t2.effective_dacl(node)?;
            if a1.len() != a2.len() {
                return Err(E_ID);
            }
            let mut k = 0;
            while k < a1.len() {
                if a1[k].sid != a2[k].sid
                    || a1[k].mask != a2[k].mask
                    || a1[k].flags != a2[k].flags
                    || a1[k].kind != a2[k].kind
                    || a1[k].origin != a2[k].origin
                {
                    return Err(E_ID);
                }
                k += 1;
            }
            checked += 1;
            node += 1;
        }
        case += 1;
    }
    Ok(checked)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：100 例写入/识别/交叉三方对平。
    let compared = pair_sweep()?;
    // R2：10 例伪 ID 注入双路径全拦截。
    let caught = fake_sweep()?;
    if caught != FAKE_N as u32 {
        return Err(E_FAKE);
    }
    // R3：分类基元钉子（显式/继承/空集）。
    let mut explicit_only: Vec<ie::Ace> = Vec::new();
    explicit_only.push(ie::Ace { sid: 1, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    let mut inh_only: Vec<ie::Ace> = Vec::new();
    inh_only.push(ie::Ace { sid: 2, mask: 2, flags: ie::AI_INHERITED, kind: ie::AK_ALLOW, origin: 5 });
    if classify(&explicit_only) != (1, 0) || classify(&inh_only) != (0, 1) || classify(&[]) != (0, 0) {
        return Err(E_ID);
    }
    // R4：识别侧不接受伪装——ID 位清零的继承副本（账面撕裂形态）
    // 被交叉恒等拦出。
    let mut torn: Vec<ie::Ace> = Vec::new();
    torn.push(ie::Ace { sid: 3, mask: 4, flags: ie::AI_OBJECT_INHERIT, kind: ie::AK_ALLOW, origin: 7 });
    if provenance_audit(&torn).is_ok() {
        return Err(E_FAKE);
    }
    // R5：validate_explicit 独立面与 set_dacl 同判（一致性）。
    if validate_explicit(&inh_only).err() != Some(E_FLAG) {
        return Err(E_FLAG);
    }
    if validate_explicit(&explicit_only).is_err() {
        return Err(E_FLAG);
    }
    // R6：统计面对平（stats ⟷ classify 双机计数恒等）。
    let stats = stats_cross_check()?;
    // R7：重物化对平（同参重建全字段一致——可重放锚）。
    let remat = rematerialize_check()?;
    if remat == 0 {
        return Err(E_INTG);
    }
    Ok(compared.wrapping_mul(3).wrapping_add(caught).wrapping_add(stats).wrapping_add(remat))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FLAG => "写入侧纪律破（副本缺 ID/显式带 ID）",
        E_ID => "识别侧分类错（flags/origin 交叉不恒等）",
        E_FAKE => "伪已继承位未被拦截",
        E_ANCHOR => "引擎锚漂移",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36203_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36203_inhflags");
    cs.add("SEG 段位独占", SEG_BASE == 0x7b50, "0x7b50~0x7b5F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例三方对平/10 例伪 ID 双路径拦截/分类钉子全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("对平例量", PAIR_N == 100 && FAKE_N == 10, "100 例对平 + 10 例注入（判据锚）");
    cs.add("ID 位锚", ie::AI_INHERITED == 0x10, "INHERITED_ACE = 0x10（win32 对齐）");
    cs.add("交叉恒等面", provenance_audit(&[]).is_ok() && provenance_audit(&inh_only_static()).is_ok(), "空集与纯继承集审计通过");
    cs.add("越界面", fake_injected_dacl(FAKE_N).is_err(), "注入构造越界显性拒");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

fn inh_only_static() -> Vec<ie::Ace> {
    let mut v: Vec<ie::Ace> = Vec::new();
    v.push(ie::Ace { sid: 2, mask: 2, flags: ie::AI_INHERITED, kind: ie::AK_ALLOW, origin: 5 });
    v
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36203_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_writer_sets_id_on_copies_only() {
        // 性质直证：同一棵树里，显式无 ID、副本必带 ID、origin 同步。
        let t = pair_tree(4).ok().unwrap_or(ie::InhTree::new());
        let mid_eff = t.effective_dacl(1).ok().unwrap_or_default();
        let mut saw_explicit = false;
        let mut saw_inherited = false;
        let mut i = 0;
        while i < mid_eff.len() {
            let a = &mid_eff[i];
            if is_inherited(a) {
                saw_inherited = true;
                assert_ne!(a.origin, 0, "继承副本 origin 必非零");
                assert_eq!(a.flags & ie::AI_INHERIT_ONLY, 0, "副本清 IO");
            } else {
                saw_explicit = true;
                assert_eq!(a.origin, 0, "显式 ACE origin 必为零");
            }
            i += 1;
        }
        assert!(saw_inherited && saw_explicit, "case4 的 mid 必须同时见到两类 ACE");
    }

    #[test]
    fn test_fake_bit_always_caught() {
        // 账面守恒：10 例伪 ID 注入拦截账 = 10，无一漏网。
        assert_eq!(fake_sweep().ok().unwrap_or(0), FAKE_N as u32);
        assert_eq!(fake_injected_dacl(FAKE_N + 1).err(), Some(E_INTG));
    }
}
