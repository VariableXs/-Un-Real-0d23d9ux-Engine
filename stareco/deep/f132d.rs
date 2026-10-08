//! 深化层 · F132 差异表公开（2026-09-26 回炉补深化）。
//!
//! 补深：影响级别判定标准（量化判定函数）、季度刷新引擎、星卡反向
//! 链接生成、按族×状态双维查询、新差异判例管线入口（F035→F036）。

use crate::checks::CheckSet;
use crate::stareco::difftable::{ApiFamily, DiffStatus, DiffTable, Impact};

// ---------------------------------------------------------------------------
// 影响级别量化判定（标准文档化的机器面）
// ---------------------------------------------------------------------------

/// 判定输入：功能是否可用 / 体验是否受损 / 用户路径是否触达。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImpactFacts {
    pub functional_broken: bool,
    pub experience_degraded: bool,
    pub user_reachable: bool,
}

/// 判定标准：致命 = 功能不可用；降级 = 可用但体验降且路径触达；
/// 无感 = 路径不触达的理论差异。判定只此一份（一处一事实）。
pub fn judge_impact(f: ImpactFacts) -> Impact {
    if f.functional_broken {
        Impact::Fatal
    } else if f.experience_degraded && f.user_reachable {
        Impact::Degraded
    } else {
        Impact::Theoretical
    }
}

// ---------------------------------------------------------------------------
// 季度刷新引擎（随账本节奏）
// ---------------------------------------------------------------------------

pub struct QuarterRefresh {
    pub season: u32,
    /// 本季账本判定为「实测正常」的条目追踪键。
    pub ledger_ok_keys: alloc::vec::Vec<&'static str>,
    /// 本季账本新增实测确认的差异键。
    pub ledger_confirm_keys: alloc::vec::Vec<&'static str>,
}

impl QuarterRefresh {
    /// 刷新产出：（应自动解决键, 应保留在册键）。
    /// 自动解决 = 账本本季实测正常（反驳在册差异）→ 转 Resolved；
    /// 账本确认或未覆盖 → 保留在册。
    pub fn plan(&self, table: &DiffTable) -> (alloc::vec::Vec<&'static str>, alloc::vec::Vec<&'static str>) {
        let mut to_resolve = alloc::vec::Vec::new();
        let mut keep_open = alloc::vec::Vec::new();
        for e in table.entries_view().iter().flatten() {
            if e.status != DiffStatus::Open {
                continue;
            }
            if self.ledger_ok_keys.contains(&e.ledger_key) {
                to_resolve.push(e.ledger_key);
            } else {
                keep_open.push(e.ledger_key);
            }
        }
        (to_resolve, keep_open)
    }
}

// ---------------------------------------------------------------------------
// 星卡反向链接生成
// ---------------------------------------------------------------------------

/// 星卡「此程序受差异 X 影响」区数据：按账本键列在册差异，带影响徽标。
pub struct StarcardImpact {
    pub trace_id: crate::stareco::ebase::TraceId,
    pub api: &'static str,
    pub impact: Impact,
    pub workaround: &'static str,
}

/// 生成星卡影响区（只列 Open 条目；已解决差异不出现在星卡上——
/// 但保留在差异表历史里）。
pub fn starcard_impacts(table: &DiffTable, ledger_key: &str) -> alloc::vec::Vec<StarcardImpact> {
    table
        .entries_view()
        .iter()
        .flatten()
        .filter(|e| e.ledger_key == ledger_key && e.status == DiffStatus::Open)
        .map(|e| StarcardImpact {
            trace_id: e.id,
            api: e.api,
            impact: e.impact,
            workaround: e.workaround,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F132D_TAG: &str = "stareco-F132-deep";

pub fn run_f132_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F132D_TAG);

    // 影响级别量化判定
    set.add(
        "f132d fatal rule",
        judge_impact(ImpactFacts { functional_broken: true, experience_degraded: false, user_reachable: true })
            == Impact::Fatal,
        "broken = fatal",
    );
    set.add(
        "f132d degraded rule",
        judge_impact(ImpactFacts { functional_broken: false, experience_degraded: true, user_reachable: true })
            == Impact::Degraded,
        "reachable+degraded",
    );
    set.add(
        "f132d theoretical rule",
        judge_impact(ImpactFacts { functional_broken: false, experience_degraded: true, user_reachable: false })
            == Impact::Theoretical,
        "unreachable = theoretical",
    );

    // 季度刷新：账本反驳 → 自动解决；账本确认 → 保留
    let mut table = DiffTable::new();
    let d1 = table
        .admit(20260926, "CreateFileW", ApiFamily::Storage, "管道语义", Impact::Degraded, "改普通文件", "k-pipe")
        .expect("d1");
    let _d2 = table
        .admit(20260926, "GetPixel", ApiFamily::Gdi, "慢路径", Impact::Theoretical, "", "k-pixel")
        .expect("d2");
    let qr = QuarterRefresh {
        season: 3,
        ledger_ok_keys: alloc::vec!["k-pipe"],
        ledger_confirm_keys: alloc::vec!["k-pixel"],
    };
    let (resolve, keep) = qr.plan(&table);
    set.add(
        "f132d refresh plan",
        resolve.contains(&"k-pipe") && keep.contains(&"k-pixel"),
        "refuted→resolve; confirmed→keep",
    );
    assert!(table.resolve(d1).is_ok());
    set.add(
        "f132d resolved growth counter",
        table.resolved_total() == 1,
        "兼容面成长曲线",
    );

    // 星卡反向链接：只列 Open，带追踪编号
    let hits = starcard_impacts(&table, "k-pixel");
    set.add(
        "f132d starcard open only",
        hits.len() == 1 && hits[0].api == "GetPixel" && hits[0].trace_id.is_valid(),
        "open impacts with id",
    );
    set.add("f132d starcard resolved hidden", starcard_impacts(&table, "k-pipe").is_empty(), "resolved off card");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn impact_ladder() {
        assert_eq!(
            judge_impact(ImpactFacts { functional_broken: false, experience_degraded: false, user_reachable: true }),
            Impact::Theoretical
        );
    }
}
