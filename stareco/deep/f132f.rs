//! 深化层三 · F132 差异表公开（2026-09-26 深化批次三）。
//!
//! 补深 CI 对接件面（主册 G-D-07 + 账本回炉扩列方向）：差异表查询/
//! 过滤引擎（族×影响×状态矩阵检索）、迁移完成度计算（千分比口径）、
//! 消费端兼容性判定器（调用面 × 差异表交叉裁决）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 差异表查询引擎：过滤 + 稳定序（追踪编号升序）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Impact3 {
    Lethal,
    Degraded,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Open,
    Acknowledged,
    Resolved,
}

#[derive(Clone, Copy)]
pub struct DiffRow {
    pub track_id: u32,
    pub family: u8,
    pub impact: Impact3,
    pub status: DiffStatus,
}

/// 查询条件：全 None 表示不过滤（0 值哨兵，避免 Option 泛型膨胀）。
#[derive(Clone, Copy)]
pub struct DiffQuery {
    pub family: Option<u8>,
    pub impact: Option<Impact3>,
    pub status: Option<DiffStatus>,
}

impl DiffQuery {
    pub fn all() -> DiffQuery {
        DiffQuery { family: None, impact: None, status: None }
    }
}

fn row_matches(r: &DiffRow, q: &DiffQuery) -> bool {
    if let Some(f) = q.family {
        if r.family != f {
            return false;
        }
    }
    if let Some(i) = q.impact {
        if r.impact != i {
            return false;
        }
    }
    if let Some(s) = q.status {
        if r.status != s {
            return false;
        }
    }
    true
}

/// 查询：命中行按 track_id 升序稳定输出（选择序，n 小无性能压力）。
pub fn query_rows(rows: &[DiffRow], q: &DiffQuery) -> alloc::vec::Vec<DiffRow> {
    let mut hit: alloc::vec::Vec<DiffRow> =
        rows.iter().copied().filter(|r| row_matches(r, q)).collect();
    // 插入序按 track_id。
    for i in 1..hit.len() {
        let key = hit[i];
        let mut j = i;
        while j > 0 && hit[j - 1].track_id > key.track_id {
            hit[j] = hit[j - 1];
            j -= 1;
        }
        hit[j] = key;
    }
    hit
}

// ---------------------------------------------------------------------------
// 迁移完成度：消费方已迁移 / 应迁移 → 千分比（ebase 口径同源）
// ---------------------------------------------------------------------------

pub struct MigrationLedger {
    /// (api 名, 已迁移消费方数, 应迁移消费方数)
    pub apis: alloc::vec::Vec<(&'static str, u32, u32)>,
}

impl MigrationLedger {
    pub fn new() -> MigrationLedger {
        MigrationLedger { apis: alloc::vec::Vec::new() }
    }

    pub fn admit(&mut self, api: &'static str, migrated: u32, total: u32) -> Result<(), &'static str> {
        if api.is_empty() {
            return Err("API 名缺失");
        }
        if total == 0 || migrated > total {
            return Err("迁移计数矛盾：total=0 或 migrated>total");
        }
        self.apis.push((api, migrated, total));
        Ok(())
    }

    /// 单 API 千分比。
    pub fn per_mille(&self, idx: usize) -> Result<u32, &'static str> {
        let (_, m, t) = self.apis.get(idx).ok_or("序号越界")?;
        Ok(m * 1000 / t)
    }

    /// 全表千分比（分子分母合计——不是均值的均值）。
    pub fn overall_per_mille(&self) -> u32 {
        let (mut m, mut t) = (0u32, 0u32);
        for (_, a, b) in &self.apis {
            m += a;
            t += b;
        }
        if t == 0 {
            0
        } else {
            m * 1000 / t
        }
    }

    /// 未达标清单（<900‰ 视为迁移未收口）。
    pub fn laggards(&self) -> alloc::vec::Vec<&'static str> {
        self.apis
            .iter()
            .filter(|(_, m, t)| m * 1000 / t < 900)
            .map(|(n, _, _)| *n)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 消费端兼容性判定器：调用面 × 差异表交叉裁决
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 干净：未触碰任何 Open 差异。
    Clean,
    /// 警告：触碰降级级差异——可运行但体验降级。
    Warn,
    /// 阻断：触碰致命级差异——必须迁移。
    Block,
}

/// 裁决：消费方调用的 API 集合 × 差异表 Open 行 → Clean/Warn/Block。
/// 致命 > 降级 > 无感（命中最高级别即该级别）。
pub fn consumer_verdict(calls: &[&'static str], rows: &[DiffRow], api_names: &[&'static str]) -> Verdict {
    // api_names 与 rows 同序（登记纪律）；Open 行才参与裁决。
    let mut verdict = Verdict::Clean;
    for (i, row) in rows.iter().enumerate() {
        if row.status == DiffStatus::Resolved {
            continue;
        }
        let name = match api_names.get(i) {
            Some(n) => *n,
            None => continue,
        };
        if !calls.contains(&name) {
            continue;
        }
        let level = match row.impact {
            Impact3::Lethal => 2,
            Impact3::Degraded => 1,
            Impact3::None => 0,
        };
        let cur = match verdict {
            Verdict::Clean => 0,
            Verdict::Warn => 1,
            Verdict::Block => 2,
        };
        if level > cur {
            verdict = match level {
                2 => Verdict::Block,
                1 => Verdict::Warn,
                _ => Verdict::Clean,
            };
        }
    }
    verdict
}

// ---------------------------------------------------------------------------
// CI 交叉校验扩展：同 API 双行矛盾检测（登记纪律机器面）
// ---------------------------------------------------------------------------

/// 同一 API 不得同时存在两条 Open 行（重复登记矛盾）。
pub fn duplicate_open(rows: &[DiffRow], api_names: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    let mut dup: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if r.status != DiffStatus::Open {
            continue;
        }
        let name = match api_names.get(i) {
            Some(n) => *n,
            None => continue,
        };
        for (j, r2) in rows.iter().enumerate() {
            if j <= i || r2.status != DiffStatus::Open {
                continue;
            }
            if api_names.get(j) == Some(&name) {
                dup.push(name);
                break;
            }
        }
    }
    dup
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F132F_TAG: &str = "stareco-F132-deep3";

pub fn run_f132_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F132F_TAG);

    let rows = [
        DiffRow { track_id: 3, family: 1, impact: Impact3::Lethal, status: DiffStatus::Open },
        DiffRow { track_id: 1, family: 2, impact: Impact3::Degraded, status: DiffStatus::Open },
        DiffRow { track_id: 2, family: 1, impact: Impact3::None, status: DiffStatus::Resolved },
        DiffRow { track_id: 4, family: 2, impact: Impact3::None, status: DiffStatus::Acknowledged },
    ];

    // 查询引擎：过滤 + track_id 稳定序
    let all = query_rows(&rows, &DiffQuery::all());
    set.add("f132f stable order", all.len() == 4 && all[0].track_id == 1 && all[3].track_id == 4, "全查按编号升序");
    let fam1 = query_rows(&rows, &DiffQuery { family: Some(1), ..DiffQuery::all() });
    set.add("f132f family filter", fam1.len() == 2 && fam1.iter().all(|r| r.family == 1), "族过滤");
    let open = query_rows(&rows, &DiffQuery { status: Some(DiffStatus::Open), ..DiffQuery::all() });
    set.add("f132f open filter", open.len() == 2, "Open 过滤");
    let lethal =
        query_rows(&rows, &DiffQuery { impact: Some(Impact3::Lethal), ..DiffQuery::all() });
    set.add("f132f impact filter", lethal.len() == 1 && lethal[0].track_id == 3, "影响级过滤");

    // 迁移完成度
    let mut led = MigrationLedger::new();
    set.add("f132f admit zero total", led.admit("api_a", 0, 0).is_err(), "total=0 拒绝");
    set.add("f132f admit over", led.admit("api_a", 5, 4).is_err(), "migrated>total 拒绝");
    let _ = led.admit("api_a", 95, 100);
    let _ = led.admit("api_b", 10, 100);
    set.add("f132f per mille", led.per_mille(0) == Ok(950) && led.per_mille(1) == Ok(100), "千分比直算");
    set.add("f132f overall", led.overall_per_mille() == 525, "合计口径 105/200");
    set.add("f132f laggards", led.laggards() == alloc::vec!["api_b"], "未达标清单");

    // 消费端裁决
    let names = ["api_x", "api_y", "api_z", "api_w"];
    set.add(
        "f132f verdict block",
        consumer_verdict(&["api_x"], &rows, &names) == Verdict::Block,
        "触碰致命 Open→阻断",
    );
    set.add(
        "f132f verdict warn",
        consumer_verdict(&["api_y"], &rows, &names) == Verdict::Warn,
        "触碰降级 Open→警告",
    );
    set.add(
        "f132f verdict clean",
        consumer_verdict(&["api_z", "api_w"], &rows, &names) == Verdict::Clean,
        "已解决/无感→干净",
    );
    set.add(
        "f132f verdict none",
        consumer_verdict(&["api_absent"], &rows, &names) == Verdict::Clean,
        "未调用→干净",
    );

    // 重复 Open 检测
    let dup_rows = [
        DiffRow { track_id: 1, family: 1, impact: Impact3::None, status: DiffStatus::Open },
        DiffRow { track_id: 2, family: 1, impact: Impact3::None, status: DiffStatus::Open },
        DiffRow { track_id: 3, family: 1, impact: Impact3::None, status: DiffStatus::Resolved },
    ];
    let dups = duplicate_open(&dup_rows, &["api_a", "api_a", "api_a"]);
    set.add("f132f dup open", dups == alloc::vec!["api_a"], "同 API 双 Open 检出");
    set.add("f132f dup none", duplicate_open(&rows, &names).is_empty(), "正常表零矛盾");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn query_insertion_sort_stable() {
        let rows = [
            DiffRow { track_id: 9, family: 1, impact: Impact3::None, status: DiffStatus::Open },
            DiffRow { track_id: 2, family: 1, impact: Impact3::None, status: DiffStatus::Open },
            DiffRow { track_id: 5, family: 1, impact: Impact3::None, status: DiffStatus::Open },
        ];
        let out = query_rows(&rows, &DiffQuery::all());
        assert_eq!(out.iter().map(|r| r.track_id).collect::<alloc::vec::Vec<_>>(), alloc::vec![2, 5, 9]);
    }

    #[test]
    fn migration_math_guards() {
        let mut l = MigrationLedger::new();
        assert!(l.admit("", 1, 2).is_err());
        assert!(l.admit("a", 1, 2).is_ok());
        assert_eq!(l.per_mille(0), Ok(500));
        assert_eq!(l.per_mille(9), Err("序号越界"));
        assert_eq!(l.overall_per_mille(), 500);
    }
}
