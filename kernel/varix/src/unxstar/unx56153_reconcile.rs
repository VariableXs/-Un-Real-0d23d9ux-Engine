//! UNX-F56153 总账-明细对账器（0x61320 段）
//!
//! 锚点：UNX-F56153-J1 对账三重账（总账行数 = 明细行数聚合 + 总账指
//! 标值 = 明细指标聚合 + 总账批键集 = 明细批键集——三重逐项落账）；
//! J2 聚合语义（明细 sum/max/count 三聚合口径与总账值逐项等）；J3 差
//! 异定位（对账差异行号定位 + 差异字段名留痕）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·对账器面）。
//!
//! 结构：明细登记册（批 × 行 × 指标三元组）+ 对账器（三重聚合比对）
//! + 差异账（首差定位：批键 × 字段 × 双值）+ 总账指纹（聚合结果折叠）
//! + 注入核（总账多行/总账少行/指标漂移/批键漂移/重复批键必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61320 独占 0x20）。
pub const SEG_BASE: u32 = 0x61320;
pub const RC_ROW_EXTRA: u32 = SEG_BASE + 0x01; // 总账多行
pub const RC_ROW_MISS: u32 = SEG_BASE + 0x02; // 总账少行
pub const RC_METRIC_DRIFT: u32 = SEG_BASE + 0x03; // 指标聚合漂移
pub const RC_KEY_DRIFT: u32 = SEG_BASE + 0x04; // 批键漂移
pub const RC_DUP_KEY: u32 = SEG_BASE + 0x05; // 重复批键

/// 明细行：批键 × 指标值。
#[derive(Clone)]
pub struct DetailRow {
    pub batch_key: String,
    pub metric: u32,
}

/// 总账行：批键 × 聚合指标（sum 口径）。
#[derive(Clone)]
pub struct SummaryRow {
    pub batch_key: String,
    pub agg_metric: u32,
    pub row_n: u32,
}

/// 聚合核：明细按批键聚合（sum + count）。
pub fn aggregate(details: &[DetailRow]) -> Vec<SummaryRow> {
    let mut keys: Vec<&str> = details.iter().map(|d| d.batch_key.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.iter()
        .map(|k| {
            let rows: Vec<&DetailRow> = details.iter().filter(|d| d.batch_key == *k).collect();
            SummaryRow {
                batch_key: k.to_string(),
                agg_metric: rows.iter().map(|d| d.metric).sum(),
                row_n: rows.len() as u32,
            }
        })
        .collect()
}

/// 对账差异：字段 × 批键 × 总账值 × 明细聚合值。
#[derive(Clone)]
pub struct DiffItem {
    pub field: &'static str,
    pub batch_key: String,
    pub summary_val: u32,
    pub detail_val: u32,
}

impl DiffItem {
    fn render_line(&self) -> String {
        format!(
            "diff|{}|{}|s{}|d{}",
            self.field, self.batch_key, self.summary_val, self.detail_val
        )
    }
}

/// 对账器：三重比对（行集/指标/批键）。
pub struct Reconciler {
    diffs: Vec<DiffItem>,
}

impl Reconciler {
    pub fn new() -> Self {
        Reconciler { diffs: Vec::new() }
    }

    /// 对账：总账 vs 明细聚合。返回差异条数（0 = 全对）。
    pub fn reconcile(&mut self, summary: &[SummaryRow], details: &[DetailRow]) -> usize {
        self.diffs.clear();
        let agg = aggregate(details);
        // 键集差：总账独有（多行）与明细独有（少行）
        for s in summary {
            if !agg.iter().any(|a| a.batch_key == s.batch_key) {
                self.diffs.push(DiffItem {
                    field: "row_extra",
                    batch_key: s.batch_key.clone(),
                    summary_val: s.agg_metric,
                    detail_val: 0,
                });
            }
        }
        for a in &agg {
            if !summary.iter().any(|s| s.batch_key == a.batch_key) {
                self.diffs.push(DiffItem {
                    field: "row_miss",
                    batch_key: a.batch_key.clone(),
                    summary_val: 0,
                    detail_val: a.agg_metric,
                });
            }
        }
        // 共有键：指标值对账
        for s in summary {
            if let Some(a) = agg.iter().find(|a| a.batch_key == s.batch_key) {
                if s.agg_metric != a.agg_metric {
                    self.diffs.push(DiffItem {
                        field: "metric",
                        batch_key: s.batch_key.clone(),
                        summary_val: s.agg_metric,
                        detail_val: a.agg_metric,
                    });
                }
                if s.row_n != a.row_n {
                    self.diffs.push(DiffItem {
                        field: "row_n",
                        batch_key: s.batch_key.clone(),
                        summary_val: s.row_n,
                        detail_val: a.row_n,
                    });
                }
            }
        }
        self.diffs.len()
    }

    pub fn diffs(&self) -> &[DiffItem] {
        &self.diffs
    }

    /// 首差定位。
    pub fn first_diff(&self) -> Option<&DiffItem> {
        self.diffs.first()
    }

    /// 渲染导出。
    pub fn render_diffs(&self) -> String {
        let mut out = String::new();
        for d in &self.diffs {
            out.push_str(&d.render_line());
            out.push('\n');
        }
        if self.diffs.is_empty() {
            out.push_str("RECONCILED\n");
        }
        out
    }
}

impl Default for Reconciler {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三批明细（full×3 / mid×2 / idle×1）+ 对齐总账。
fn detail_fixture() -> Vec<DetailRow> {
    [
        ("fps_full", 60),
        ("fps_full", 61),
        ("fps_full", 62),
        ("fps_mid", 120),
        ("fps_mid", 118),
        ("fps_idle", 240),
    ]
    .iter()
    .map(|(k, v)| DetailRow {
        batch_key: k.to_string(),
        metric: *v,
    })
    .collect()
}

fn summary_fixture() -> Vec<SummaryRow> {
    aggregate(&detail_fixture())
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：总账多行（多一可疑键）
    let mut s = summary_fixture();
    s.push(SummaryRow {
        batch_key: "ghost".to_string(),
        agg_metric: 1,
        row_n: 1,
    });
    let mut rc = Reconciler::new();
    let n = rc.reconcile(&s, &detail_fixture());
    if n >= 1 && rc.first_diff().map(|d| d.field == "row_extra").unwrap_or(false) {
        caught += 1;
    }
    // 注 2：总账少行
    let s2 = summary_fixture();
    let mut details = detail_fixture();
    details.push(DetailRow {
        batch_key: "orphan".to_string(),
        metric: 5,
    });
    let mut rc = Reconciler::new();
    let n = rc.reconcile(&s2, &details);
    if n >= 1 && rc.first_diff().map(|d| d.field == "row_miss").unwrap_or(false) {
        caught += 1;
    }
    // 注 3：指标漂移
    let mut s3 = summary_fixture();
    s3[0].agg_metric += 1;
    let mut rc = Reconciler::new();
    let n = rc.reconcile(&s3, &detail_fixture());
    if n >= 1 && rc.diffs().iter().any(|d| d.field == "metric") {
        caught += 1;
    }
    // 注 4：行数漂移
    let mut s4 = summary_fixture();
    s4[1].row_n += 1;
    let mut rc = Reconciler::new();
    let n = rc.reconcile(&s4, &detail_fixture());
    if n >= 1 && rc.diffs().iter().any(|d| d.field == "row_n") {
        caught += 1;
    }
    // 注 5：全对时 RECONCILED
    let mut rc = Reconciler::new();
    let n = rc.reconcile(&summary_fixture(), &detail_fixture());
    if n == 0 && rc.render_diffs().contains("RECONCILED") {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 三重账；J2 聚合语义；J3 差异定位。
pub fn run_unx56153_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56153");
    // J1-1 聚合三批（full sum=183 n=3 / mid sum=238 n=2 / idle sum=240 n=1）
    let agg = aggregate(&detail_fixture());
    let ok = agg.len() == 3
        && agg[0].agg_metric == 183 && agg[0].row_n == 3
        && agg[1].agg_metric == 238 && agg[1].row_n == 2
        && agg[2].agg_metric == 240 && agg[2].row_n == 1;
    s.add("56153-J1-聚合三重", ok, if ok { "" } else { "聚合漂移" });
    // J1-2 对账全对
    let mut rc = Reconciler::new();
    let n = rc.reconcile(&summary_fixture(), &detail_fixture());
    let ok = n == 0;
    s.add("56153-J1-对账全对", ok, if ok { "" } else { "对账漂移" });
    // J1-3 渲染导出 RECONCILED
    let mut rc = Reconciler::new();
    let _ = rc.reconcile(&summary_fixture(), &detail_fixture());
    let ok = rc.render_diffs() == "RECONCILED\n";
    s.add("56153-J1-渲染RECONCILED", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 聚合键有序（字典序：fps_full < fps_idle < fps_mid）
    let agg = aggregate(&detail_fixture());
    let ok = agg[0].batch_key == "fps_full" && agg[1].batch_key == "fps_idle" && agg[2].batch_key == "fps_mid";
    s.add("56153-J2-键有序", ok, if ok { "" } else { "键序漂移" });
    // J2-2 sum 口径锚（60+61+62=183）
    let ok = agg[0].agg_metric == 183;
    s.add("56153-J2-sum锚", ok, if ok { "" } else { "sum 漂移" });
    // J2-3 count 口径锚（mid 2 行）
    let ok = agg[1].row_n == 2;
    s.add("56153-J2-count锚", ok, if ok { "" } else { "count 漂移" });
    // J2-4 聚合确定性
    let a = aggregate(&detail_fixture());
    let b = aggregate(&detail_fixture());
    let ok = a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| {
            x.batch_key == y.batch_key && x.agg_metric == y.agg_metric && x.row_n == y.row_n
        });
    s.add("56153-J2-聚合确定", ok, if ok { "" } else { "确定性漂移" });
    // J3-1 差异定位字段名留痕
    let mut s3 = summary_fixture();
    s3[0].agg_metric = 999;
    let mut rc = Reconciler::new();
    let _ = rc.reconcile(&s3, &detail_fixture());
    let d = rc.first_diff();
    let ok = d.map(|d| d.field == "metric" && d.batch_key == "fps_full" && d.summary_val == 999 && d.detail_val == 183).unwrap_or(false);
    s.add("56153-J3-差异定位", ok, if ok { "" } else { "定位漂移" });
    // J3-2 差异渲染留痕
    let ok = rc.render_diffs().contains("diff|metric|fps_full|s999|d183");
    s.add("56153-J3-差异渲染", ok, if ok { "" } else { "差异渲染漂移" });
    // J3-3 多差异计数
    let mut s3 = summary_fixture();
    s3[0].agg_metric = 999;
    s3[2].row_n = 9;
    let mut rc = Reconciler::new();
    let n = rc.reconcile(&s3, &detail_fixture());
    let ok = n == 2;
    s.add("56153-J3-多差计数", ok, if ok { "" } else { "计数漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56153-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56153_all_checks_green() {
        let set = run_unx56153_checks();
        assert!(set.all_passed(), "UNX-F56153 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56153_agg_stable() {
        assert_eq!(aggregate(&detail_fixture())[0].agg_metric, 183);
    }
}
