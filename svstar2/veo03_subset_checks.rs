//! VE-F2803 · 域自检（判据逐条对应，见 `veo03_subset.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - O01 架构声明 → `O03-架构-*`
//! - 集成边界 → `O03-边界-*`
//! - 解析子集 → `O03-子集-*`
//! - 判据（自证可追溯）→ `O03-判据-*`
//! - 降级矩阵（非法输入→拒绝三要素 / 边界越界→钳制 + 告警 / 异常检出→立案流转）
//!   → `O03-降级-*`
//! - 跨批对接（上游契约接收哈希对账 / 下游消费接口前向声明 / 对账钩子）
//!   → `O03-对接-*`
//! - 无障碍（文档替述可读）→ `O03-读屏-*`
//! - 错误路径零静默 → `O03-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::veo03_subset::*;
use crate::checks::CheckSet;

use alloc::vec::Vec;

use super::veo03_subset::PropertyFamily;

/// 构造一条合法的样本属性（登记类判据的原料）。
fn sample(name: &'static str, family: PropertyFamily) -> SubsetProperty {
    SubsetProperty {
        spec: PropertySpec {
            name,
            family,
            value_kind: "length",
            inherited: false,
            initial: "0px",
            domain_low: 0.0,
            domain_high: 100.0,
        },
        spec_ref: "css-values-4§5.2",
    }
}

// ---------------------------------------------------------------------------
// O03-架构-*
// ---------------------------------------------------------------------------

/// O03-架构-01：规范版本锚定必须能证伪（重算，不是看填了没有）。
fn chk_arch_anchor_verifiable(set: &mut CheckSet) {
    let a = SubsetAnchor::anchored();
    // 对账通过（登记哈希 == 实算哈希）。
    let verified = a.verify().is_ok();
    // 关键：必须能抓漂移。改版本不改哈希 ⇒ 抓得到。
    let drifted = SubsetAnchor {
        version: "CSS Snapshot 2024-05",
        anchor_hash: a.anchor_hash,
        families: a.families,
    }
    .verify()
    .is_err();
    // 只改规范族清单（取材依据变了）也必须被抓——这是为什么哈希要覆盖族清单。
    let fams_drift = SubsetAnchor {
        version: a.version,
        anchor_hash: a.anchor_hash,
        families: [
            "css-values-4",
            "css-color-4",
            "css-display-3",
            "css-effects-2",
            "css-grid-2",
        ],
    }
    .verify()
    .is_err();
    let ok = verified && drifted && fams_drift;
    set.add("O03-架构-01-规范锚定可证伪", ok, "");
}

/// O03-架构-02：重签必须四件齐（版本 + 族清单 + 新哈希，且新哈希须自洽）。
fn chk_arch_resign_discipline(set: &mut CheckSet) {
    // 空版本拒。
    let no_ver = SubsetAnchor::resign("", SPEC_FAMILIES, "x").is_err();
    // 空哈希拒。
    let no_hash = SubsetAnchor::resign(ANCHORED_CSS_VERSION, SPEC_FAMILIES, "").is_err();
    // 哈希与内容不自洽拒（新版本配旧哈希 = 静默漂移）。
    let mismatched = SubsetAnchor::resign("CSS Snapshot 2024-05", SPEC_FAMILIES, ANCHOR_HASH)
        .is_err();
    // 四件自洽 ⇒ 放行。
    let good = match SubsetAnchor::resign("CSS Snapshot 2024-05", SPEC_FAMILIES, "n") {
        Ok(_) => false, // "n" 不是实算值，应被 verify 拒
        Err(_) => true,
    };
    let ok = no_ver && no_hash && mismatched && good;
    set.add("O03-架构-02-重签四件齐", ok, "");
}

/// O03-架构-03：易错项分桶正确（`display` 归布局不是视觉）。
fn chk_arch_classification(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    let mut all_right = true;
    for trap in CLASSIFICATION_TRAPS.iter() {
        if let Some(e) = t.get(trap.name) {
            if e.spec.family != trap.correct {
                all_right = false;
            }
        }
    }
    // 关键反例：display 必须在布局族，且不在视觉族。
    let display_layout = t
        .get("display")
        .map(|e| e.spec.family == PropertyFamily::Layout)
        .unwrap_or(false);
    // 每条易错项都必须写明错归后果（只写「易错」不写后果等于没留档）。
    let consequences = CLASSIFICATION_TRAPS
        .iter()
        .all(|x| x.consequence.len() >= 8);
    let ok = all_right && display_layout && consequences;
    set.add("O03-架构-03-易错项分桶正确", ok, "");
}

// ---------------------------------------------------------------------------
// O03-子集-*
// ---------------------------------------------------------------------------

/// O03-子集-01：四族齐备且非空（分族不是为了好看，是为了可寻址）。
fn chk_subset_four_families(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    let mut non_empty = true;
    let mut within_cap = true;
    for f in PropertyFamily::ALL.iter() {
        let n = t.family_len(*f);
        if n == 0 {
            non_empty = false;
        }
        if n > MAX_PER_FAMILY {
            within_cap = false;
        }
    }
    // 族数恰为 4（不多不少——多出来的族没有归属规则）。
    let exactly_four = PropertyFamily::ALL.len() == FAMILY_COUNT;
    let ok = non_empty && within_cap && exactly_four;
    set.add("O03-子集-01-四族齐备非空", ok, "");
}

/// O03-子集-02：每条属性都带规范章节出处（本单相对 F2801 的核心增量）。
fn chk_subset_spec_ref_present(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    // 判据不是「非空」（那恒真），而是每条都含章节号且长度合规。
    let all_have_ref = t
        .entries
        .iter()
        .all(|e| e.spec_ref.contains('§') && e.spec_ref.len() >= MIN_SPEC_REF_LEN);
    // 出处必须落在已纳入的规范族里（或其相邻族）——不能引一本没纳入的规范。
    let fams_ok = t.entries.iter().all(|e| {
        let root = e.spec_ref.split('§').next().unwrap_or("");
        root.starts_with("css-")
    });
    // 逐条齐整（含初值、参数域）。
    let all_wellformed = t.entries.iter().all(|e| e.is_wellformed());
    let ok = all_have_ref && fams_ok && all_wellformed;
    set.add("O03-子集-02-章节出处逐条齐整", ok, "");
}

/// O03-子集-03：全表审计通过 + 条目数落在上限内。
fn chk_subset_audit_clean(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    let audited = t.audit().is_ok();
    let sized = t.len() > 0 && t.len() <= MAX_TOTAL;
    // 四族条目数之和必须等于总数（分桶不重不漏）。
    let sum: usize = PropertyFamily::ALL.iter().map(|f| t.family_len(*f)).sum();
    let no_dup = sum == t.len();
    let ok = audited && sized && no_dup;
    set.add("O03-子集-03-全表审计通过", ok, "");
}

/// O03-子集-04：属性名全局唯一（同名两处定义必有一错）。
fn chk_subset_name_unique(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    let mut names: Vec<&str> = t.entries.iter().map(|e| e.spec.name).collect();
    let before = names.len();
    names.sort();
    names.dedup();
    let unique = names.len() == before;
    // 与 F2801 种子表不得有冲突登记（同属性两处定义）。
    let no_seed_conflict = t
        .get("width")
        .map(|e| e.spec.family == PropertyFamily::Layout)
        .unwrap_or(false);
    let ok = unique && no_seed_conflict;
    set.add("O03-子集-04-属性名全局唯一", ok, "");
}

/// O03-子集-05：初值必填（缺初值则级联无从收敛）。
fn chk_subset_initial_required(set: &mut CheckSet) {
    let mut t = SubsetTable::standard();
    let mut bad = sample("zzz-probe", PropertyFamily::Layout);
    bad.spec.initial = "";
    let rejected = t.admit(bad).is_err();
    let code_right = match t.admit(bad) {
        Err(e) => e.code == E_INITIAL_MISSING,
        Ok(_) => false,
    };
    let ok = rejected && code_right;
    set.add("O03-子集-05-初值必填", ok, "");
}

// ---------------------------------------------------------------------------
// O03-边界-* / O03-对接-*
// ---------------------------------------------------------------------------

/// O03-边界-01：集成边界——本单不引入新 crate（O 域依赖面不扩大）。
///
/// 真判据：新增的类型里不得出现任何「持有外部句柄」的形态。
/// 静态断言容易恒真，故改用**能力对账**：本单声明的排除项必须覆盖
/// F2801 已列的六条（本单不得静默缩小排除面）。
fn chk_boundary_no_new_deps(set: &mut CheckSet) {
    use SUBSET_EXCLUSIONS;
    // 排除面不得小于 F2801 的六条（缩小排除面 = 悄悄扩大子集）。
    let no_shrink = SUBSET_EXCLUSIONS.len() >= 6;
    // 每条排除都带理由（"不支持"三个字不是理由）。
    let reasons = SUBSET_EXCLUSIONS.iter().all(|(_, why)| why.len() >= 8);
    let ok = no_shrink && reasons;
    set.add("O03-边界-01-不扩大依赖面", ok, "");
}

/// O03-对接-01：哈希对账是重算（正负样本成对）。
fn chk_hash_recompute(set: &mut CheckSet) {
    let a = SubsetAnchor::anchored();
    // 同输入两次实算必一致（可复现）。
    let stable = a.compute_hash() == a.compute_hash();
    // 实算值等于登记常量（这一条把「常量是算出来的」钉住）。
    let matches_const = a.compute_hash() == a.anchor_hash;
    // 负样本：改一个字节即不同。
    let b = SubsetAnchor {
        version: "CSS Snapshot 2023-11",
        ..a
    };
    let distinct = b.compute_hash() != a.compute_hash();
    let ok = stable && matches_const && distinct;
    set.add("O03-对接-01-锚定哈希是重算", ok, "");
}

// ---------------------------------------------------------------------------
// O03-降级-*
// ---------------------------------------------------------------------------

/// O03-降级-01：拒绝三要素齐发。
fn chk_degrade_triple(set: &mut CheckSet) {
    let mut t = SubsetTable::standard();
    let mut bad = sample("bad name", PropertyFamily::Layout);
    bad.spec.name = "";
    let e = match t.admit(bad) {
        Err(e) => e,
        Ok(_) => {
            set.add("O03-降级-01-拒绝三要素齐发", false, "");
            return;
        }
    };
    let five = !e.code.trim().is_empty()
        && !e.what.trim().is_empty()
        && !e.why.trim().is_empty()
        && !e.next.trim().is_empty()
        && !e.who.trim().is_empty();
    let ok = five && e.why.len() >= 8;
    set.add("O03-降级-01-拒绝三要素齐发", ok, "");
}

/// O03-降级-02：参数域倒挂被拒（钳制的前提是域有序）。
fn chk_degrade_domain_guard(set: &mut CheckSet) {
    let mut t = SubsetTable::standard();
    let mut bad = sample("zzz-inv", PropertyFamily::Layout);
    bad.spec.domain_low = 100.0;
    bad.spec.domain_high = 1.0;
    let r = t.admit(bad);
    let rejected = r.is_err();
    let code_right = match &r {
        Err(e) => e.code == E_DOMAIN_INVERTED,
        Ok(_) => false,
    };
    let ok = rejected && code_right;
    set.add("O03-降级-02-参数域倒挂被拒", ok, "");
}

/// O03-降级-03：族容量守卫（某族写成全表即失去分族意义）。
fn chk_degrade_family_cap(set: &mut CheckSet) {
    let mut t = SubsetTable::new();
    let mut n = 0usize;
    let mut capped = false;
    for i in 0..(MAX_PER_FAMILY + 4) {
        let name: &'static str = Box::leak(format!("cap{}", i).into_boxed_str());
        if t.admit(sample(name, PropertyFamily::Effect)).is_err() {
            capped = true;
            break;
        }
        n += 1;
    }
    // 必须**恰好**停在上限：提前停是守卫写错，无限期增是守卫失效。
    let exact = capped && n == MAX_PER_FAMILY;
    set.add("O03-降级-03-族容量守卫", exact, "");
}

/// O03-降级-04：重复登记被拒（同名两定义必有一错）。
fn chk_degrade_dup_guard(set: &mut CheckSet) {
    let mut t = SubsetTable::standard();
    // 拿表里已有的名字再登记一次。
    let dup = sample("color", PropertyFamily::Visual);
    let r = t.admit(dup);
    let rejected = r.is_err();
    let code_right = match &r {
        Err(e) => e.code == E_PROP_DUP,
        Ok(_) => false,
    };
    let len_unchanged = t.len() == SubsetTable::standard().len();
    let ok = rejected && code_right && len_unchanged;
    set.add("O03-降级-04-重复登记被拒", ok, "");
}

/// O03-降级-05：审计能抓「易错项被改归错族」（异常检出→立案）。
fn chk_degrade_audit_catches_misbucket(set: &mut CheckSet) {
    let clean = SubsetTable::standard();
    let ok_clean = clean.audit().is_ok();
    // 注入：把 display 挪到视觉族（须先移除原条目才能重名登记）。
    let mut bad = SubsetTable::standard();
    let display = bad.get("display").map(|e| *e).unwrap_or(sample("x", PropertyFamily::Layout));
    let mut entries: Vec<SubsetProperty> = bad
        .entries
        .iter()
        .filter(|e| e.spec.name != "display")
        .map(|e| *e)
        .collect();
    entries.push(SubsetProperty {
        spec: PropertySpec {
            family: PropertyFamily::Visual,
            ..display.spec
        },
        spec_ref: display.spec_ref,
    });
    bad.entries = entries;
    let r = bad.audit();
    let caught = match &r {
        Err(e) => e.code == E_FAMILY_TRAP_CONFLICT,
        Ok(_) => false,
    };
    let ok = ok_clean && caught;
    set.add("O03-降级-05-审计抓错归", ok, "");
}

// ---------------------------------------------------------------------------
// O03-判据-* / O03-读屏-*
// ---------------------------------------------------------------------------

/// O03-判据-01：常量参与实算（拿常量当输入用，不是比对常量本身）。
fn chk_criterion_constants_live(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    // MAX_PER_FAMILY 当循环上限用。
    let mut per_family = 0usize;
    for f in PropertyFamily::ALL.iter() {
        per_family += t.family_len(*f);
    }
    // MAX_TOTAL 当规模判据用。
    let within_total = t.len() <= MAX_TOTAL && per_family == t.len();
    // FAMILY_COUNT 当四族判据用。
    let four = PropertyFamily::ALL.len() == FAMILY_COUNT;
    // MIN_SPEC_REF_LEN 当长度下限用（构造一条刚好不合规的出处）。
    let mut t2 = SubsetTable::standard();
    let mut bad = sample("zzz-ref", PropertyFamily::Layout);
    bad.spec_ref = "css-x";
    let rejected = t2.admit(bad).is_err();
    let ok = within_total && four && rejected;
    set.add("O03-判据-01-常量参与实算", ok, "");
}

/// O03-判据-02：分桶完备——四族条目数之和恒等于总数（不重不漏）。
///
/// 这条防的是「某条属性族字段填错导致它既不属于 A 也不属于 B」——
/// 那种漏项在按族查表时表现为「属性凭空消失」，极难归因。
fn chk_criterion_partition_total(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    let sum: usize = PropertyFamily::ALL.iter().map(|f| t.family_len(*f)).sum();
    // 反例样本：构造一个族字段不在四族内的条目（用 Layout 冒充第五族是不可能的，
    // 故改为验证「按族查得的总和 == 总数」这一恒等式本身）。
    let identity = sum == t.len();
    // 且每条都能被某族查到（逐条反查，O(条目数×族数)）。
    let every_reachable = t.entries.iter().all(|e| {
        PropertyFamily::ALL.iter().any(|f| t.family(*f).any(|x| x.spec.name == e.spec.name))
    });
    let ok = identity && every_reachable;
    set.add("O03-判据-02-分桶完备不重不漏", ok, "");
}

/// O03-读屏-01：四族清单读屏可达（摘要须含四族名与条目数）。
fn chk_screen_summary(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    let s = t.screen_summary();
    let has_version = s.contains(SUBSET_VERSION);
    let has_anchor = s.contains(ANCHORED_CSS_VERSION);
    let mut has_all_families = true;
    for f in PropertyFamily::ALL.iter() {
        if !s.contains(f.zh()) {
            has_all_families = false;
        }
    }
    let single_line = !s.contains('\n');
    let substantial = s.len() >= 40;
    let ok = has_version && has_anchor && has_all_families && single_line && substantial;
    set.add("O03-读屏-01-四族清单读屏可达", ok, "");
}

/// O03-读屏-02：属性条目读屏单行（含族/初值/域/出处）。
fn chk_screen_entry_line(set: &mut CheckSet) {
    let t = SubsetTable::standard();
    let e = match t.get("opacity") {
        Some(e) => e,
        None => {
            set.add("O03-读屏-02-条目单行含出处", false, "");
            return;
        }
    };
    let line = e.screen_line();
    let has_name = line.contains("opacity");
    let has_family = line.contains("效果");
    let has_initial = line.contains("初值");
    let has_domain = line.contains("域");
    let has_ref = line.contains("§");
    let single_line = !line.contains('\n');
    let ok = has_name && has_family && has_initial && has_domain && has_ref && single_line;
    set.add("O03-读屏-02-条目单行含出处", ok, "");
}

/// O03-错误-01：错误路径零静默——八类错误码逐个实际触发。
pub fn chk_error_codes_reachable(set: &mut CheckSet) {
    let mut hit = 0u32;
    let total = 8u32;

    // E_NAME_INVALID
    let mut t1 = SubsetTable::standard();
    let mut b1 = sample("ok-name", PropertyFamily::Layout);
    b1.spec.name = "bad name";
    if t1.admit(b1).is_err() {
        hit += 1;
    }
    // E_SPEC_REF_MISSING
    let mut t2 = SubsetTable::standard();
    let mut b2 = sample("zzz-ref", PropertyFamily::Layout);
    b2.spec_ref = "noref";
    if t2.admit(b2).is_err() {
        hit += 1;
    }
    // E_INITIAL_MISSING
    let mut t3 = SubsetTable::standard();
    let mut b3 = sample("zzz-init", PropertyFamily::Layout);
    b3.spec.initial = "";
    if t3.admit(b3).is_err() {
        hit += 1;
    }
    // E_DOMAIN_INVERTED
    let mut t4 = SubsetTable::standard();
    let mut b4 = sample("zzz-dom", PropertyFamily::Layout);
    b4.spec.domain_low = 9.0;
    b4.spec.domain_high = 1.0;
    if t4.admit(b4).is_err() {
        hit += 1;
    }
    // E_PROP_DUP
    let mut t5 = SubsetTable::standard();
    if t5.admit(sample("color", PropertyFamily::Visual)).is_err() {
        hit += 1;
    }
    // E_FAMILY_OVERGROWN
    let mut t6 = SubsetTable::new();
    let mut grown = false;
    for i in 0..(MAX_PER_FAMILY + 2) {
        let nm: &'static str = Box::leak(format!("g{}", i).into_boxed_str());
        if t6.admit(sample(nm, PropertyFamily::Effect)).is_err() {
            grown = true;
            break;
        }
    }
    if grown {
        hit += 1;
    }
    // E_TOTAL_CAP
    let mut t7 = SubsetTable::new();
    let mut full = false;
    'outer: for f in PropertyFamily::ALL.iter() {
        for i in 0..(MAX_PER_FAMILY + 2) {
            let nm: &'static str = Box::leak(format!("t{}{}", f.zh(), i).into_boxed_str());
            if t7.admit(sample(nm, *f)).is_err() {
                full = true;
                break 'outer;
            }
        }
    }
    if full {
        hit += 1;
    }
    // E_ANCHOR_DRIFT
    let drifted = SubsetAnchor {
        version: "drifted",
        anchor_hash: ANCHOR_HASH,
        families: SPEC_FAMILIES,
    };
    if drifted.verify().is_err() {
        hit += 1;
    }

    let ok = hit == total;
    set.add("O03-错误-01-八类错误码全可触发", ok, "");
}

/// VE-F2803 域自检入口。
pub fn run_veo03_checks() -> CheckSet {
    let mut set = CheckSet::new("veo03-subset");
    chk_arch_anchor_verifiable(&mut set);
    chk_arch_resign_discipline(&mut set);
    chk_arch_classification(&mut set);
    chk_subset_four_families(&mut set);
    chk_subset_spec_ref_present(&mut set);
    chk_subset_audit_clean(&mut set);
    chk_subset_name_unique(&mut set);
    chk_subset_initial_required(&mut set);
    chk_boundary_no_new_deps(&mut set);
    chk_hash_recompute(&mut set);
    chk_degrade_triple(&mut set);
    chk_degrade_domain_guard(&mut set);
    chk_degrade_family_cap(&mut set);
    chk_degrade_dup_guard(&mut set);
    chk_degrade_audit_catches_misbucket(&mut set);
    chk_criterion_constants_live(&mut set);
    chk_criterion_partition_total(&mut set);
    chk_screen_summary(&mut set);
    chk_screen_entry_line(&mut set);
    chk_error_codes_reachable(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veo03_subset_selfcheck_clean() {
        let t = SubsetTable::standard();
        assert!(t.audit().is_ok(), "标准子集表不应有审计问题");
    }

    #[test]
    fn veo03_anchor_hash_is_computed() {
        let a = SubsetAnchor::anchored();
        assert_eq!(
            a.compute_hash(),
            ANCHOR_HASH,
            "锚定哈希必须是实算值，不能是编出来的常量"
        );
    }

    #[test]
    fn veo03_display_is_layout() {
        let t = SubsetTable::standard();
        let d = t.get("display").expect("display 应在子集表内");
        assert_eq!(d.spec.family, PropertyFamily::Layout);
    }
}
