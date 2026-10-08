//! CGPU-F2882 域自检（CGPU-S 域多用户模型与角色判据层）
//!
//! 判据侧**独立写死**期望（三级名册、六行模型表、三角色与权限声明、
//! 0x9B 段 0x9B1x 六码与 0x9B0x 八码合并互异）。判据侧自备独立校验器
//! 跑变异表（断链/重名/空权限/错位各向拒绝）——常量表上的同源恒绿
//! 不是门禁。聚合防自调：族内判据只调另一族 standalone + 进行中
//! set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcs82_usermodel as um;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据 tally 码（S 域 0x9B 段续编，与实现文件五码互异）
// ---------------------------------------------------------------------------

/// 判据自检 tally 失配码。
pub const E_S820_CHECK_TALLY: u16 = 0x9B15;

const EXPECT_VERSION: &str = "CS82-usermodel-v1";

/// 单遍词法剥除（行注释+块注释+字符串字面量全剥——panic 词扫描防自引用）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    match String::from_utf8(out) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 判据侧独立校验器（独立实现——不调实现文件逻辑，变异表双向验证用）
// ---------------------------------------------------------------------------

/// 独立模型表校验器（与 um::verify_model_table 同判据异实现：
/// 行名唯一 + tier<3 + note 非空 + 父锚可追溯 + 至少一平台根）。
fn audit_table(rows: &[um::ModelRow]) -> Result<usize, u16> {
    if rows.is_empty() {
        return Err(um::E_S820_MODEL_INCOMPLETE);
    }
    let mut i = 0usize;
    while i < rows.len() {
        let mut j = i + 1;
        while j < rows.len() {
            if rows[i].name == rows[j].name {
                return Err(um::E_S820_MODEL_INCOMPLETE);
            }
            j += 1;
        }
        i += 1;
    }
    let mut roots = 0usize;
    for row in rows.iter() {
        if row.tier >= 3 {
            return Err(um::E_S820_MODEL_INCOMPLETE);
        }
        if row.note.is_empty() {
            return Err(um::E_S820_MODEL_INCOMPLETE);
        }
        if row.parent == "平台根" {
            roots += 1;
            continue;
        }
        let mut linked = false;
        for other in rows.iter() {
            if other.name == row.parent {
                linked = true;
            }
        }
        if !linked {
            return Err(um::E_S820_TIER_ORPHAN);
        }
    }
    if roots == 0 {
        return Err(um::E_S820_TIER_ORPHAN);
    }
    Ok(rows.len())
}

/// 独立角色权限校验器（空 scope 拒 / 声明不含角色名拒）。
fn audit_roles(roles: &[&str], scopes: &[String]) -> Result<usize, u16> {
    if roles.is_empty() {
        return Err(um::E_S820_ROLE_UNKNOWN);
    }
    let mut i = 0usize;
    while i < roles.len() {
        if scopes[i].is_empty() {
            return Err(um::E_S820_PERM_EMPTY);
        }
        if !scopes[i].contains(roles[i]) {
            return Err(um::E_S820_ROLE_UNKNOWN);
        }
        i += 1;
    }
    Ok(roles.len())
}

// ---------------------------------------------------------------------------
// A 族 · 模型组 + 角色组（规格 + 边界）
// ---------------------------------------------------------------------------

fn chk_spec_tiers(set: &mut CheckSet) {
    // 规格-01：三级名册写死对拍（用户/租户/角色，锚点原文次序）。
    let want: [&str; 3] = ["用户", "租户", "角色"];
    let mut ok = um::MODEL_TIERS.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if um::MODEL_TIERS[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ES820-规格-01-三级名册写死对拍");
    } else {
        set.fail("ES820-规格-01-三级名册写死对拍", "三级漂移或次序错");
    }
}

fn chk_spec_model_table(set: &mut CheckSet) {
    // 规格-02：模型表六行、行名写死对拍、tier 分布（租户 2/用户 2/角色 2）。
    let want_names: [&str; 6] = [
        "租户-独占", "租户-共享", "用户-成员", "用户-访客", "角色-操作", "角色-审计",
    ];
    let mut ok = um::MODEL_TABLE.len() == 6;
    let mut t = [0usize; 3];
    let mut i = 0usize;
    while i < 6 {
        if um::MODEL_TABLE[i].name != want_names[i] {
            ok = false;
        }
        if um::MODEL_TABLE[i].tier < 3 {
            t[um::MODEL_TABLE[i].tier] += 1;
        }
        i += 1;
    }
    if t[0] != 2 || t[1] != 2 || t[2] != 2 {
        ok = false;
    }
    if ok {
        set.ok("ES820-规格-02-模型表六行写死对拍");
    } else {
        set.fail("ES820-规格-02-模型表六行写死对拍", "模型表漂移或级分布失衡");
    }
}

fn chk_bound_table_pass(set: &mut CheckSet) {
    // 边界-01：登记表过实现闸 + 过判据独立闸（双闸同绿）。
    let impl_ok = matches!(um::verify_model_table(), Ok(n) if n == 6);
    let audit_ok = matches!(audit_table(&um::MODEL_TABLE), Ok(n) if n == 6);
    if impl_ok && audit_ok {
        set.ok("ES820-边界-01-登记表双闸通过");
    } else {
        set.fail("ES820-边界-01-登记表双闸通过", "登记表被误拒或双闸不同判");
    }
}

fn chk_bound_table_orphan(set: &mut CheckSet) {
    // 边界-02：断链变异——父锚改不存在名 → 独立闸拒 TIER_ORPHAN。
    let mut rows: Vec<um::ModelRow> = Vec::new();
    for row in um::MODEL_TABLE.iter() {
        rows.push(row.clone());
    }
    rows[2].parent = "不存在的父锚";
    let ok = matches!(audit_table(&rows), Err(c) if c == um::E_S820_TIER_ORPHAN);
    if ok {
        set.ok("ES820-边界-02-断链变异必拒");
    } else {
        set.fail("ES820-边界-02-断链变异必拒", "断链闸漏抓");
    }
}

fn chk_bound_table_cycle(set: &mut CheckSet) {
    // 边界-03：循环互指变异（A↔B 互为父，全 linked 无平台根）→ 拒 TIER_ORPHAN。
    let rows: Vec<um::ModelRow> = vec![
        um::ModelRow { tier: 0, name: "甲", parent: "乙", note: "n" },
        um::ModelRow { tier: 1, name: "乙", parent: "甲", note: "n" },
    ];
    let ok = matches!(audit_table(&rows), Err(c) if c == um::E_S820_TIER_ORPHAN);
    if ok {
        set.ok("ES820-边界-03-循环断链必拒");
    } else {
        set.fail("ES820-边界-03-循环断链必拒", "无根闸漏抓");
    }
}

fn chk_bound_table_dup(set: &mut CheckSet) {
    // 边界-04：重名变异 → 拒 MODEL_INCOMPLETE。
    let rows: Vec<um::ModelRow> = vec![
        um::ModelRow { tier: 1, name: "同", parent: "平台根", note: "n" },
        um::ModelRow { tier: 0, name: "同", parent: "同", note: "n" },
    ];
    let ok = matches!(audit_table(&rows), Err(c) if c == um::E_S820_MODEL_INCOMPLETE);
    if ok {
        set.ok("ES820-边界-04-重名变异必拒");
    } else {
        set.fail("ES820-边界-04-重名变异必拒", "唯一性闸漏抓");
    }
}

fn chk_spec_roles(set: &mut CheckSet) {
    // 规格-03+04：三角色封闭 + 权限声明逐条写死对拍。
    let want_roles: [&str; 3] = ["前台", "后台", "服务"];
    let want_scope: [&str; 3] = [
        "前台：面向最终用户的渲染会话——提交/查看/取消自己的作业",
        "后台：面向运维与管理的管理面——租户与配额管理，不直接碰渲染上下文",
        "服务：面向机器对机器的信任面——服务间凭证调用，最小授权+全程审计",
    ];
    let mut ok = um::ROLES.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if um::ROLES[i] != want_roles[i] || um::ROLE_SCOPE[i] != want_scope[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ES820-规格-03-角色与权限写死对拍");
    } else {
        set.fail("ES820-规格-03-角色与权限写死对拍", "角色或权限声明漂移");
    }
}

fn chk_bound_roles_pass(set: &mut CheckSet) {
    // 边界-05：登记角色过实现闸 + 过判据独立闸。
    let impl_ok = matches!(um::verify_roles(), Ok(n) if n == 3);
    let scopes: Vec<String> = um::ROLE_SCOPE.iter().map(|s| String::from(*s)).collect();
    let audit_ok = matches!(audit_roles(&um::ROLES, &scopes), Ok(n) if n == 3);
    if impl_ok && audit_ok {
        set.ok("ES820-边界-05-登记角色双闸通过");
    } else {
        set.fail("ES820-边界-05-登记角色双闸通过", "登记角色被误拒或双闸不同判");
    }
}

fn chk_bound_roles_variants(set: &mut CheckSet) {
    // 边界-06：空权限/错位变异双向拒绝（空→PERM_EMPTY；错位→ROLE_UNKNOWN）。
    let roles: [&str; 2] = ["前台", "后台"];
    let blank: Vec<String> = vec![String::from("前台：可提交"), String::from("")];
    let misplaced: Vec<String> =
        vec![String::from("前台：可提交"), String::from("服务：凭证调用")];
    let ok = matches!(audit_roles(&roles, &blank), Err(c) if c == um::E_S820_PERM_EMPTY)
        && matches!(audit_roles(&roles, &misplaced), Err(c) if c == um::E_S820_ROLE_UNKNOWN);
    if ok {
        set.ok("ES820-边界-06-空权限与错位必拒");
    } else {
        set.fail("ES820-边界-06-空权限与错位必拒", "权限闸漏抓");
    }
}

fn chk_spec_linkage(set: &mut CheckSet) {
    // 规格-05：联动四行与四段名目逐行对拍 + verify_linkage 过闸。
    let want_seg: [&str; 4] = ["模型段", "隔离段", "调度段", "配额段"];
    let mut ok = um::LINKAGE_FOUR.len() == 4;
    let mut i = 0usize;
    while i < 4 {
        if !um::LINKAGE_FOUR[i].contains(want_seg[i]) {
            ok = false;
        }
        i += 1;
    }
    if !matches!(um::verify_linkage(), Ok(4)) {
        ok = false;
    }
    if ok {
        set.ok("ES820-规格-05-联动四段逐行对拍");
    } else {
        set.fail("ES820-规格-05-联动四段逐行对拍", "联动声明漂移或缺段");
    }
}

fn chk_bound_codes_exclusive(set: &mut CheckSet) {
    // 边界-07：0x9B 全段 14 码（本单 0x9B1x 六码 + F2881 0x9B0x 八码）
    // 写死值逐一对拍 + 两两互异（跨文件码互异复核）。
    let codes: [u16; 14] = [
        um::E_S820_TIER_ORPHAN,
        um::E_S820_MODEL_INCOMPLETE,
        um::E_S820_PERM_EMPTY,
        um::E_S820_ROLE_UNKNOWN,
        um::E_S820_VERSION_MISMATCH,
        E_S820_CHECK_TALLY,
        sa81::E_S810_MISSING_ITEM,
        sa81::E_S810_LINK_UNCONFIRMED,
        sa81::E_S810_POSITIONING_FAULT,
        sa81::E_S810_SEGMENT_INCOMPLETE,
        sa81::E_S810_FAIRNESS_MISSING,
        sa81::E_S810_R10_UNFULFILLED,
        sa81::E_S810_VERSION_MISMATCH,
        sa81c::E_S810_CHECK_TALLY,
    ];
    let want: [u16; 14] = [
        0x9B10, 0x9B11, 0x9B12, 0x9B13, 0x9B14, 0x9B15, 0x9B00, 0x9B01, 0x9B02, 0x9B03,
        0x9B04, 0x9B05, 0x9B06, 0x9B07,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < 14 {
        if codes[i] != want[i] {
            ok = false;
        }
        let mut j = 0usize;
        while j < 14 {
            if i != j && codes[i] == codes[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if ok {
        set.ok("ES820-边界-07-十四码跨文件互异");
    } else {
        set.fail("ES820-边界-07-十四码跨文件互异", "码漂移或跨文件撞码");
    }
}

fn chk_bound_version(set: &mut CheckSet) {
    // 边界-08：版本写死 + 双向校验（冻结版放行/异版拒收）。
    let ok = um::VCS82_VERSION == EXPECT_VERSION
        && um::check_version(EXPECT_VERSION).is_ok()
        && matches!(um::check_version("CS82-usermodel-v2"), Err(c) if c == um::E_S820_VERSION_MISMATCH);
    if ok {
        set.ok("ES820-边界-08-版本双向校验");
    } else {
        set.fail("ES820-边界-08-版本双向校验", "版本闸漏抓");
    }
}

// ---------------------------------------------------------------------------
// B 族 · 承载力 + 判据
// ---------------------------------------------------------------------------

fn chk_carry_model_row(set: &mut CheckSet) {
    // B-01：ModelRow 字段承载——构造+四字段读回一致。
    let row = um::ModelRow {
        tier: 2,
        name: "角色-测试",
        parent: "用户-成员",
        note: "判据承载测试行",
    };
    let ok = row.tier == 2 && row.name == "角色-测试" && row.parent == "用户-成员"
        && row.note == "判据承载测试行";
    if ok {
        set.ok("ES820-承载力-01-模型行字段承载");
    } else {
        set.fail("ES820-承载力-01-模型行字段承载", "字段承载断裂");
    }
}

fn chk_carry_counts(set: &mut CheckSet) {
    // B-02：两闸返回数与常量表行数一致（6/3）。
    let t = matches!(um::verify_model_table(), Ok(n) if n == um::MODEL_TABLE.len());
    let r = matches!(um::verify_roles(), Ok(n) if n == um::ROLES.len());
    if t && r {
        set.ok("ES820-承载力-02-行数守恒");
    } else {
        set.fail("ES820-承载力-02-行数守恒", "行数承载断裂");
    }
}

fn chk_carry_scope_len(set: &mut CheckSet) {
    // B-03：权限声明承载——三声明均含冒号语义锚（角色：权限正文）。
    let mut ok = um::ROLE_SCOPE.len() == 3;
    for scope in um::ROLE_SCOPE.iter() {
        if !scope.contains("：") {
            ok = false;
        }
    }
    if ok {
        set.ok("ES820-承载力-03-权限声明承载");
    } else {
        set.fail("ES820-承载力-03-权限声明承载", "声明结构断裂");
    }
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-零 panic（自扫本文件；三剥防自引用——模式字面量在字符串里被剥掉）。
    let clean = strip_lexical_noise(&String::from(include_str!("vcs82_usermodel_checks.rs")));
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("ES820-判据-零panic面");
    } else {
        set.fail("ES820-判据-零panic面", "判据区含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-聚合守恒防自调——条数期望判据侧写死（A 族 12 条；本条登记前 B 族 4 条）。
    let a = run_vcs82_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 4;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("ES820-判据-聚合守恒防自调");
    } else {
        set.fail("ES820-判据-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=模型组+角色组 / b=承载力+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

use super::vcs81_sdomain_arch as sa81;
use super::vcs81_sdomain_arch_checks as sa81c;

/// 判据族 a：模型组 + 角色组（12 条：规格 5 + 边界 7）。
pub fn run_vcs82_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcs82/a");
    chk_spec_tiers(&mut s);
    chk_spec_model_table(&mut s);
    chk_bound_table_pass(&mut s);
    chk_bound_table_orphan(&mut s);
    chk_bound_table_cycle(&mut s);
    chk_bound_table_dup(&mut s);
    chk_spec_roles(&mut s);
    chk_bound_roles_pass(&mut s);
    chk_bound_roles_variants(&mut s);
    chk_spec_linkage(&mut s);
    chk_bound_codes_exclusive(&mut s);
    chk_bound_version(&mut s);
    s
}

/// 判据族 b：承载力 + 判据（5 条：承载力 3 + 零 panic + 防自调）。
pub fn run_vcs82_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcs82/b");
    chk_carry_model_row(&mut s);
    chk_carry_counts(&mut s);
    chk_carry_scope_len(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcs82_checks() -> CheckSet {
    CheckSet::merge(
        run_vcs82_checks_a_standalone(),
        run_vcs82_checks_b_standalone(),
    )
}

#[cfg(test)]
mod tests {
    use super::{run_vcs82_checks_a_standalone, run_vcs82_checks_b_standalone, run_vcs82_checks};

    #[test]
    fn s02_a_standalone_all_green() {
        let set = run_vcs82_checks_a_standalone();
        assert!(set.all_passed(), "vcs82 A 批红项存在");
    }

    #[test]
    fn s02_b_standalone_all_green() {
        let set = run_vcs82_checks_b_standalone();
        assert!(set.all_passed(), "vcs82 B 批红项存在");
    }

    #[test]
    fn s02_merged_all_green() {
        let set = run_vcs82_checks();
        assert!(set.all_passed(), "vcs82 merged 红项存在");
    }
}
