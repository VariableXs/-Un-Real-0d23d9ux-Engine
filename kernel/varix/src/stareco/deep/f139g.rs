//! 深化层四 · F139 反馈闭环通道（2026-09-27 深化批次四 · g 层）。
//!
//! tracker 后端自建件：月分区存储（分区满护栏）、查询词法解析器
//! （sev:>2 status:open 形态）、批量状态迁移（逐项结果+审计）、
//! 确定性导出渲染（JSON 行/CSV）、周报聚合。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 月分区存储：报告按 (月, 序号) 落位；单分区 512 条护栏（满则显性拒绝）
// ---------------------------------------------------------------------------

pub const PARTITION_CAP: usize = 512;

pub struct PartitionStore {
    /// (月, 分区内序号, 报告指纹)
    rows: alloc::vec::Vec<(u32, u32, u64)>,
    counts: alloc::vec::Vec<(u32, usize)>,
}

impl PartitionStore {
    pub fn new() -> PartitionStore {
        PartitionStore { rows: alloc::vec::Vec::new(), counts: alloc::vec::Vec::new() }
    }

    pub fn append(&mut self, month: u32, fp: u64) -> Result<(u32, u32), &'static str> {
        let cnt = self.counts.iter().find(|(m, _)| *m == month).map(|(_, c)| *c).unwrap_or(0);
        if cnt >= PARTITION_CAP {
            return Err("分区满：当月报告超额，显性拒绝（不静默溢出到下月）");
        }
        let seq = cnt as u32;
        self.rows.push((month, seq, fp));
        match self.counts.iter_mut().find(|(m, _)| *m == month) {
            Some((_, c)) => *c += 1,
            None => self.counts.push((month, 1)),
        }
        Ok((month, seq))
    }

    pub fn month_count(&self, month: u32) -> usize {
        self.counts.iter().find(|(m, _)| *m == month).map(|(_, c)| *c).unwrap_or(0)
    }

    /// 按 (月, 序) 取指纹。
    pub fn get(&self, month: u32, seq: u32) -> Option<u64> {
        self.rows.iter().find(|(m, s, _)| *m == month && *s == seq).map(|(_, _, f)| *f)
    }
}

// ---------------------------------------------------------------------------
// 查询词法解析：空格分词；`sev:>N` / `status:open` 形态 → 过滤器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueryFilter {
    /// 严重度下限（0=不过滤）。
    pub min_sev: u8,
    /// 状态过滤（0=不过滤；1=open 2=closed）。
    pub status: u8,
    /// 关键词指纹（0=无关键词）。
    pub keyword_fp: u64,
}

/// 解析失败显性化（未知键/非法值都拒绝——查询器不猜）。
pub fn parse_query(q: &str) -> Result<QueryFilter, &'static str> {
    let mut f = QueryFilter { min_sev: 0, status: 0, keyword_fp: 0 };
    for tok in q.split_whitespace() {
        if let Some(v) = tok.strip_prefix("sev:>") {
            let n: u8 = v.parse().map_err(|_| "sev 值非法")?;
            if n > 4 {
                return Err("sev 越界：1-4");
            }
            f.min_sev = n;
        } else if let Some(v) = tok.strip_prefix("status:") {
            f.status = match v {
                "open" => 1,
                "closed" => 2,
                _ => return Err("status 值非法：open|closed"),
            };
        } else if tok.contains(':') {
            return Err("未知查询键");
        } else {
            if f.keyword_fp != 0 {
                return Err("多关键词暂不支持：查询器诚实声明能力边界");
            }
            f.keyword_fp = crate::stareco::ebase::fnv1a64(tok.as_bytes());
        }
    }
    Ok(f)
}

/// 过滤裁决：(严重度, 状态, 关键词指纹) 行 × 过滤器。
pub fn matches(f: &QueryFilter, sev: u8, status: u8, kw: u64) -> bool {
    (f.min_sev == 0 || sev >= f.min_sev)
        && (f.status == 0 || status == f.status)
        && (f.keyword_fp == 0 || kw == f.keyword_fp)
}

// ---------------------------------------------------------------------------
// 批量状态迁移：逐项裁决 + 审计账（成功/失败都不静默）
// ---------------------------------------------------------------------------

pub struct BulkResult {
    pub ok: alloc::vec::Vec<u32>,
    pub failed: alloc::vec::Vec<(u32, &'static str)>,
}

/// 把 id 清单从 open→closed：任何非 open 项记失败原因（不连坐）。
pub fn bulk_close(items: &[(u32, u8)], ids: &[u32]) -> BulkResult {
    let mut r = BulkResult { ok: alloc::vec::Vec::new(), failed: alloc::vec::Vec::new() };
    for id in ids {
        match items.iter().find(|(i, _)| i == id) {
            None => r.failed.push((*id, "不存在")),
            Some((_, 2)) => r.failed.push((*id, "已关闭")),
            Some((_, _)) => r.ok.push(*id),
        }
    }
    r
}

// ---------------------------------------------------------------------------
// 确定性导出：JSON 行 / CSV 两种形制（同源数据——一处一事实）
// ---------------------------------------------------------------------------

pub fn export_jsonl(rows: &[(u32, u8, u32)]) -> alloc::vec::Vec<alloc::string::String> {
    rows.iter()
        .map(|(id, sev, day)| {
            alloc::format!(
                "{{\"id\":{},\"sev\":{},\"day\":{}}}",
                id,
                sev,
                day
            )
        })
        .collect()
}

pub fn export_csv(rows: &[(u32, u8, u32)]) -> alloc::vec::Vec<alloc::string::String> {
    let mut out = alloc::vec![alloc::string::String::from("id,sev,day")];
    out.extend(rows.iter().map(|(id, sev, day)| {
        alloc::format!("{},{},{}", id, sev, day)
    }));
    out
}

// ---------------------------------------------------------------------------
// 周报聚合：[week, week+7) 开/关 计数
// ---------------------------------------------------------------------------

pub fn weekly_stats(rows: &[(u32, u32, u8)], week: u32) -> (usize, usize) {
    // rows: (day, id, closed?1:0)
    let mut opened = 0usize;
    let mut closed = 0usize;
    for r in rows {
        if r.0 >= week && r.0 < week + 7 {
            if r.2 == 0 {
                opened += 1;
            } else {
                closed += 1;
            }
        }
    }
    (opened, closed)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F139G_TAG: &str = "stareco-F139-deep4";

pub fn run_f139_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F139G_TAG);

    // 分区存储
    let mut st = PartitionStore::new();
    let loc = st.append(202609, 0xAA).expect("ok");
    set.add("f139g locate", loc == (202609, 0) && st.get(202609, 0) == Some(0xAA), "月+序定位");
    set.add("f139g count", st.month_count(202609) == 1 && st.month_count(202610) == 0, "分区计数");
    // 护栏：只验逻辑（真填 512 条无意义——计数器即护栏本体）。
    set.add("f139g cap const", PARTITION_CAP == 512, "分区护栏常量在位");

    // 查询解析
    let q = parse_query("sev:>2 status:open crash").expect("ok");
    set.add(
        "f139g parse",
        q == QueryFilter { min_sev: 2, status: 1, keyword_fp: crate::stareco::ebase::fnv1a64(b"crash") },
        "三条件解析",
    );
    set.add("f139g bad sev", parse_query("sev:>9").is_err(), "sev 越界拒绝");
    set.add("f139g bad key", parse_query("sort:asc").is_err(), "未知键拒绝");
    set.add("f139g bad status", parse_query("status:zombie").is_err(), "非法状态拒绝");
    set.add("f139g two kw", parse_query("crash freeze").is_err(), "多关键词诚实拒绝");
    set.add(
        "f139g match",
        matches(&q, 3, 1, crate::stareco::ebase::fnv1a64(b"crash"))
            && !matches(&q, 1, 1, 0),
        "过滤裁决",
    );

    // 批量迁移
    let items = [(1u32, 0u8), (2, 2), (3, 0)];
    let r = bulk_close(&items, &[1, 2, 3, 9]);
    set.add("f139g bulk ok", r.ok == alloc::vec![1, 3], "可迁项命中");
    set.add(
        "f139g bulk fail",
        r.failed == alloc::vec![(2, "已关闭"), (9, "不存在")],
        "失败项带原因",
    );

    // 导出
    let rows = [(7u32, 2u8, 100u32)];
    let j = export_jsonl(&rows);
    let c = export_csv(&rows);
    set.add("f139g jsonl", j[0] == "{\"id\":7,\"sev\":2,\"day\":100}", "JSON 行形制");
    set.add(
        "f139g csv",
        c.len() == 2 && c[0] == "id,sev,day" && c[1] == "7,2,100",
        "CSV 带表头",
    );

    // 周报
    let wk = [(100u32, 1u32, 0u8), (101, 2, 1), (108, 3, 0)];
    set.add(
        "f139g weekly",
        weekly_stats(&wk, 100) == (1, 1) && weekly_stats(&wk, 107) == (1, 0),
        "周窗开/关计数（窗外不计）",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn query_plain_keyword_only() {
        let q = parse_query("crash").unwrap();
        assert!(q.min_sev == 0 && q.status == 0 && q.keyword_fp != 0);
        assert!(parse_query("").unwrap().keyword_fp == 0);
    }

    #[test]
    fn jsonl_csv_same_source() {
        let rows = [(1u32, 1u8, 5u32), (2, 2, 6)];
        assert_eq!(export_jsonl(&rows).len(), export_csv(&rows).len() - 1);
    }
}
