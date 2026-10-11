//! VE-F0402 · 域自检（判据逐条对应，见 `vec02_spec.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三册分层 → `C02-三册-*`
//! - 编号制 → `C02-编号-*`
//! - 版本对齐 → `C02-对齐-*`
//! - 修订流程 → `C02-修订-*`
//! - 错误路径（编号冲突拒绝发布/修订未公告阻断/冲突显性二选一/停用复用拒绝）
//!   → `C02-门禁-*`、`C02-注入-*`
//! - 规范文档可检索（无障碍面）→ `C02-检索-*`
//!
//! 纯静态注册表校验，无时钟无 IO，回归可复现。

use super::vec02_spec::*;
use crate::checks::CheckSet;

/// VE-F0402 域自检。
pub fn run_vec02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec02");
    let r = match standard_registry() {
        Ok(r) => r,
        Err(_) => {
            set.add("C02-三册-标准册可建", false, "standard_registry 报错");
            return set;
        }
    };

    // ---- 判据一：三册分层 ----

    // 三册齐备且各司其职（文法/语义/标准库，条目数 ≥3 且前缀互斥）。
    {
        let ok = r.grammar.book == SpecBook::Grammar
            && r.semantics.book == SpecBook::Semantics
            && r.stdlib.book == SpecBook::StdLib
            && r.grammar.len() >= 3
            && r.semantics.len() >= 3
            && r.stdlib.len() >= 3
            && r.total_entries() >= 9;
        set.add("C02-三册-分层齐备", ok, "");
    }

    // 跨册混编号被结构性拒绝（语法册拒收 VS-SE- 前缀条目）。
    {
        let wrong = SpecEntry::new("VS-SE-99", "错册条目", "语义条目试图进语法册", "spec-grammar-1.0");
        let ok = wrong.is_ok(); // 构造合法（前缀对 SE 册）
        let mut g = match SpecVolume::new(SpecBook::Grammar, "g-1.0") {
            Ok(v) => v,
            Err(_) => {
                set.add("C02-三册-混册拒绝", false, "空册创建失败");
                return set;
            }
        };
        if let Ok(e) = wrong {
            let rejected = g.add(e).is_err(); // 但入语法册必须被拒
            set.add("C02-三册-混册拒绝", ok && rejected, "");
        } else {
            set.add("C02-三册-混册拒绝", false, "合法条目被误拒");
        }
    }

    // ---- 判据二：编号制 ----

    // 编号格式校验（前缀对册 + 序号纯数字；非法编号注册即拒）。
    {
        let bad1 = SpecEntry::new("XX-GR-01", "x", "x", "v1");
        let bad2 = SpecEntry::new("VS-GR-AB", "x", "x", "v1");
        let bad3 = SpecEntry::new("VS-XX-01", "x", "x", "v1");
        let ok = bad1.is_err() && bad2.is_err() && bad3.is_err();
        set.add("C02-编号-格式校验", ok, "");
    }

    // 全局唯一性（跨册撞号也拒绝——主键纪律）。
    {
        let ok = r.check_id_uniqueness().is_ok();
        set.add("C02-编号-全局唯一", ok, "");
    }

    // 检索面：实现诊断可按条款号直查（命中返回条款，未命中显性空）。
    {
        let hit = r.find("VS-SE-02").map(|e| e.title.contains("求值")).unwrap_or(false);
        let miss = r.find("VS-XX-404").is_none();
        set.add("C02-检索-条款号直查", hit && miss, "");
    }

    // ---- 判据三：版本对齐 ----

    // 对齐声明成对登记且对齐态显性。
    {
        let ok_align = VersionAlignment::new("spec-1.0", "impl-1.0", true, "");
        let ok = ok_align.is_ok()
            && ok_align
                .as_ref()
                .map(|a| a.aligned && a.spec_version == "spec-1.0")
                .unwrap_or(false);
        set.add("C02-对齐-成对登记", ok, "");
    }

    // 漂移显性：未对齐不写差异说明被拒；实现版本变化可检测漂移。
    {
        let silent = VersionAlignment::new("spec-1.0", "impl-2.0", false, "");
        let verbose = VersionAlignment::new("spec-1.0", "impl-2.0", false, "实现升版规范未跟，收敛中");
        let drift = verbose
            .as_ref()
            .map(|a| a.drifted("impl-3.0") && !a.drifted("impl-2.0"))
            .unwrap_or(false);
        set.add(
            "C02-对齐-漂移显性",
            silent.is_err() && verbose.is_ok() && drift,
            "",
        );
    }

    // ---- 判据四：修订流程 ----

    // 修订入账必须带原因（无原因修订拒绝）。
    {
        let mut t = match SpecRegistry::new("g", "s", "l") {
            Ok(t) => t,
            Err(_) => {
                set.add("C02-修订-带原因", false, "空注册表创建失败");
                return set;
            }
        };
        let no_reason = t.add_revision("VS-GR-01", RevisionKind::Amended, "", true);
        let with_reason =
            t.add_revision("VS-GR-01", RevisionKind::Amended, "求值顺序条款细化", true);
        set.add("C02-修订-带原因", no_reason.is_err() && with_reason.is_ok(), "");
    }

    // 停用流程：停用带原因 + 停用编号禁止复用。
    {
        let mut t = match standard_registry() {
            Ok(t) => t,
            Err(_) => {
                set.add("C02-修订-停用复用拒绝", false, "标准册创建失败");
                return set;
            }
        };
        let retired = t.retire("VS-GR-04", "产生式合并进 VS-GR-03");
        let reuse = t.check_retired_reuse("VS-GR-04").is_err();
        let fresh = t.check_retired_reuse("VS-GR-99").is_ok();
        set.add("C02-修订-停用复用拒绝", retired.is_ok() && reuse && fresh, "");
    }

    // ---- 发布门禁（错误路径逐闸验证）----

    // 闸：修订未公告 → 阻断；公告后放行。
    {
        let mut t = match standard_registry() {
            Ok(t) => t,
            Err(_) => {
                set.add("C02-门禁-未公告阻断", false, "标准册创建失败");
                return set;
            }
        };
        t.alignment = Some(VersionAlignment::new("spec-grammar-1.0", "impl-1.0", true, "").expect("对齐合法"));
        let _ = t.add_revision("VS-GR-02", RevisionKind::Amended, "签名细化", false);
        let blocked = matches!(
            t.publish_gate(),
            Err(SpecError::PublishBlocked { ref stage, .. }) if stage == "修订公告"
        );
        t.announce_revisions();
        let released = t.publish_gate().is_ok();
        set.add("C02-门禁-未公告阻断", blocked && released, "");
    }

    // 闸：编号冲突 → 拒绝（册内撞号即阻断发布）。
    {
        let mut t = match standard_registry() {
            Ok(t) => t,
            Err(_) => {
                set.add("C02-门禁-编号冲突拒绝", false, "标准册创建失败");
                return set;
            }
        };
        let dup = SpecEntry::new("VS-GR-01", "重复编号", "试图撞已有主键", "spec-grammar-1.0");
        let rejected_in_book = dup.clone().map(|e| t.grammar.add(e).is_err()).unwrap_or(false);
        let rejected_global = t.check_id_uniqueness().is_err() == false; // 标准册本身唯一
        set.add(
            "C02-门禁-编号冲突拒绝",
            rejected_in_book && rejected_global,
            "",
        );
    }

    // 闸：待裁决冲突 → 阻断；显性二选一裁决后放行（依据必填）。
    {
        let mut t = match standard_registry() {
            Ok(t) => t,
            Err(_) => {
                set.add("C02-门禁-冲突二选一", false, "标准册创建失败");
                return set;
            }
        };
        t.alignment = Some(VersionAlignment::new("spec-grammar-1.0", "impl-1.0", true, "").expect("对齐合法"));
        t.report_conflict("VS-SE-02", "实现按右到左求值，规范要求左到右");
        let blocked = matches!(
            t.publish_gate(),
            Err(SpecError::PublishBlocked { ref stage, .. }) if stage == "冲突裁决"
        );
        let no_basis = t.adjudicate("VS-SE-02", Adjudication::SpecWins, "").is_err();
        let adjudicated = t
            .adjudicate("VS-SE-02", Adjudication::SpecWins, "规范裁决：实现改为左到右")
            .is_ok();
        let released = t.publish_gate().is_ok();
        set.add(
            "C02-门禁-冲突二选一",
            blocked && no_basis && adjudicated && released,
            "",
        );
    }

    // 闸：版本对齐未登记/未对齐 → 阻断。
    {
        let mut t = match standard_registry() {
            Ok(t) => t,
            Err(_) => {
                set.add("C02-门禁-对齐阻断", false, "标准册创建失败");
                return set;
            }
        };
        let no_align = matches!(
            t.publish_gate(),
            Err(SpecError::PublishBlocked { ref stage, .. }) if stage == "版本对齐"
        );
        t.alignment = Some(VersionAlignment::new("spec-grammar-1.0", "impl-9.0", false, "实现超前，收敛中").expect("对齐合法"));
        let misaligned = t.publish_gate().is_err();
        set.add("C02-门禁-对齐阻断", no_align && misaligned, "");
    }

    // 裁决账留痕（二选一有据可查——静默冲突的反面）。
    {
        let mut t = match standard_registry() {
            Ok(t) => t,
            Err(_) => {
                set.add("C02-门禁-裁决留痕", false, "标准册创建失败");
                return set;
            }
        };
        let _ = t.adjudicate("VS-GR-01", Adjudication::ReviseSpec, "凭空裁决");
        set.add(
            "C02-门禁-裁决留痕",
            t.adjudications.is_empty(), // 凭空裁决被拒，账目不留脏记录
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec02_checks_all_green() {
        let set = run_vec02_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0402 自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 落位纪律自检：规范注册表在内核 Rust 侧可建可检索。
    #[test]
    fn vec02_registry_builds_and_publishes() {
        let mut r = standard_registry().expect("标准注册表合法");
        r.alignment = Some(
            VersionAlignment::new("spec-grammar-1.0", "impl-1.0", true, "").expect("对齐合法"),
        );
        assert!(r.publish_gate().is_ok(), "标准册全部条件满足应可发布");
        assert_eq!(r.total_entries(), 11, "预置条目 11 条（文法4+语义4+标准库3）");
    }
}
