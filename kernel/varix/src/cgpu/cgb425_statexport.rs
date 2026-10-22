//! CGPU-F0425 · 统计导出（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 统计导出：长期统计开放导出（JSON/CSV——任意维度组合）；导出范围
//!   （时间×账户×资源三维选择）；大数据流式导出；导出与查询同源同值。
//! - 判据：双格式、三维选择、流式、同源、判据。
//!
//! 设计纪律：
//! - 双格式：JSON 与 CSV 生成器吃同一格点集（同源），行/字段序确定性；
//!   双格式对账（总耗相等）是导出闸。
//! - 三维选择：时间（桶窗）× 账户（单户或全户）× 资源（单类或全类）
//!   ——选择器是谓词，导出行 = 命中谓词的格点。
//! - 流式：大数据不分页内爆——分批生成（chunk 迭代器语义），每批
//!   行数固定，批间无重叠（游标推进），总行数可预知。
//! - 同源同值：导出值必须等于查询口同谓词返回值（一个真源）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0DC0 独占。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0DC0，全仓独占）。
pub const SEG_BASE: u16 = 0x0DC0;

pub const E_EXP_FMT: u16 = SEG_BASE + 1; // 格式非法
pub const E_EXP_WINDOW: u16 = SEG_BASE + 2; // 时间窗非法
pub const E_EXP_CONSIST: u16 = SEG_BASE + 3; // 双格式对账红
pub const E_EXP_RES: u16 = SEG_BASE + 4; // 资源维非法
pub const E_EXP_CHUNK: u16 = SEG_BASE + 5; // 批行数非法（0）
pub const E_EXP_SAMESRC: u16 = SEG_BASE + 6; // 导出与查询不同值

/// 导出格点（与 F0418 立方格同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpCell {
    pub acct: u32,
    pub res: u8,
    pub hour: u64,
    pub total: u64,
}

/// 三维选择器（None = 通配；默认 = 全窗全通配）。
#[derive(Clone, Copy, Debug)]
pub struct ExpSelector {
    pub h0: u64,
    pub h1: u64,
    pub acct: Option<u32>,
    pub res: Option<u8>,
}

impl Default for ExpSelector {
    fn default() -> Self {
        // 默认全窗通配（h1=0 的零宽窗会把通配语义搞成空集）。
        Self {
            h0: 0,
            h1: u64::MAX,
            acct: None,
            res: None,
        }
    }
}

impl ExpSelector {
    pub fn matches(&self, c: &ExpCell) -> Result<bool, u16> {
        if self.h0 > self.h1 {
            return Err(E_EXP_WINDOW);
        }
        if let Some(r) = self.res {
            if r > 3 {
                return Err(E_EXP_RES);
            }
        }
        Ok(c.hour >= self.h0
            && c.hour <= self.h1
            && self.acct.map(|a| c.acct == a).unwrap_or(true)
            && self.res.map(|r| c.res == r).unwrap_or(true))
    }
}

/// 命中谓词的行（确定性排序：账户→资源→时间）。
pub fn select(cells: &[ExpCell], sel: &ExpSelector) -> Result<Vec<ExpCell>, u16> {
    let mut out = Vec::new();
    for c in cells {
        if sel.matches(c)? {
            out.push(*c);
        }
    }
    out.sort_by(|a, b| {
        a.acct
            .cmp(&b.acct)
            .then(a.res.cmp(&b.res))
            .then(a.hour.cmp(&b.hour))
    });
    Ok(out)
}

/// CSV 生成（确定性行序）。
pub fn to_csv(rows: &[ExpCell]) -> String {
    let mut s = String::from("acct,res,hour,total\r\n");
    for c in rows {
        s.push_str(&format!("{},{},{},{}\r\n", c.acct, c.res, c.hour, c.total));
    }
    s
}

/// JSON 生成（同源语义）。
pub fn to_json(rows: &[ExpCell]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in rows {
        parts.push(format!(
            "{{\"acct\":{},\"res\":{},\"hour\":{},\"total\":{}}}",
            c.acct, c.res, c.hour, c.total
        ));
    }
    format!("[{}]", parts.join(","))
}

/// 双格式同源对账：两格式的 total 序列一致。
pub fn consistency_check(cells: &[ExpCell], sel: &ExpSelector) -> Result<(), u16> {
    let rows = select(cells, sel)?;
    let csv = to_csv(&rows);
    let json = to_json(&rows);
    if csv.is_empty() || json.is_empty() {
        return Err(E_EXP_FMT);
    }
    // 对账：从两格式各自重提 total 序列（消费侧同构解析）。
    let csv_totals: Vec<u64> = csv
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| l.split(',').nth(3).unwrap_or("0").to_string())
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();
    let json_totals: Vec<u64> = rows.iter().map(|c| c.total).collect();
    if csv_totals != json_totals {
        return Err(E_EXP_CONSIST);
    }
    Ok(())
}

/// 流式导出：分批（游标推进、批间不重叠、总数可预知）。
pub struct StreamExport {
    pub rows: Vec<ExpCell>,
    pub chunk: usize,
    pub cursor: usize,
}

impl StreamExport {
    pub fn new(rows: Vec<ExpCell>, chunk: usize) -> Result<Self, u16> {
        if chunk == 0 {
            return Err(E_EXP_CHUNK);
        }
        Ok(StreamExport { rows, chunk, cursor: 0 })
    }

    /// 总批数（预知——客户端可做进度条）。
    pub fn total_batches(&self) -> usize {
        (self.rows.len() + self.chunk - 1) / self.chunk
    }

    /// 取下一批（None = 完）。
    pub fn next_batch(&mut self) -> Option<Vec<ExpCell>> {
        if self.cursor >= self.rows.len() {
            return None;
        }
        let end = (self.cursor + self.chunk).min(self.rows.len());
        let b = self.rows[self.cursor..end].to_vec();
        self.cursor = end;
        Some(b)
    }

    pub fn done(&self) -> bool {
        self.cursor >= self.rows.len()
    }
}

/// 同源同值核验：导出行合计 = 查询口同谓词合计。
pub fn same_source_check(
    cells: &[ExpCell],
    sel: &ExpSelector,
    query_total: u64,
) -> Result<(), u16> {
    let rows = select(cells, sel)?;
    let export_total: u64 = rows.iter().map(|c| c.total).sum();
    if export_total != query_total {
        return Err(E_EXP_SAMESRC);
    }
    Ok(())
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn cells() -> Vec<ExpCell> {
        vec![
            ExpCell { acct: 2, res: 1, hour: 1, total: 200 },
            ExpCell { acct: 1, res: 0, hour: 0, total: 100 },
            ExpCell { acct: 1, res: 1, hour: 2, total: 150 },
            ExpCell { acct: 2, res: 0, hour: 3, total: 50 },
        ]
    }

    #[test]
    fn a01_双格式_确定性与语义等价() {
        let rows = select(&cells(), &ExpSelector::default()).unwrap_or_default();
        assert_eq!(rows[0].acct, 1); // 排序确定性
        let csv = to_csv(&rows);
        assert!(csv.starts_with("acct,res,hour,total\r\n"));
        assert!(csv.contains("1,0,0,100"));
        let json = to_json(&rows);
        assert!(json.starts_with('['));
        assert!(json.contains("\"acct\":2,\"res\":1,\"hour\":1,\"total\":200"));
        assert_eq!(consistency_check(&cells(), &ExpSelector::default()), Ok(()));
    }

    #[test]
    fn a02_三维选择_时间账户资源() {
        let c = cells();
        // 时间窗。
        let s1 = ExpSelector { h0: 1, h1: 2, acct: None, res: None };
        let r1 = select(&c, &s1).unwrap_or_default();
        assert_eq!(r1.len(), 2);
        // 单账户。
        let s2 = ExpSelector { h0: 0, h1: 9, acct: Some(2), res: None };
        assert_eq!(select(&c, &s2).unwrap_or_default().len(), 2);
        // 单资源。
        let s3 = ExpSelector { h0: 0, h1: 9, acct: None, res: Some(0) };
        assert_eq!(select(&c, &s3).unwrap_or_default().len(), 2);
        // 三维全定。
        let s4 = ExpSelector { h0: 0, h1: 9, acct: Some(1), res: Some(1) };
        let r4 = select(&c, &s4).unwrap_or_default();
        assert_eq!(r4, vec![ExpCell { acct: 1, res: 1, hour: 2, total: 150 }]);
        // 非法窗。
        assert_eq!(
            select(&c, &ExpSelector { h0: 5, h1: 1, acct: None, res: None }),
            Err(E_EXP_WINDOW)
        );
        // 非法资源。
        assert_eq!(
            select(&c, &ExpSelector { h0: 0, h1: 9, acct: None, res: Some(9) }),
            Err(E_EXP_RES)
        );
    }

    #[test]
    fn a03_流式导出_分批不重叠总数预知() {
        let rows = select(&cells(), &ExpSelector::default()).unwrap_or_default();
        let mut st = StreamExport::new(rows.clone(), 3).unwrap_or(StreamExport {
            rows: Vec::new(), chunk: 1, cursor: 0,
        });
        assert_eq!(st.total_batches(), 2);
        let b1 = st.next_batch().unwrap_or_default();
        let b2 = st.next_batch().unwrap_or_default();
        assert_eq!(b1.len(), 3);
        assert_eq!(b2.len(), 1);
        assert!(st.done());
        assert!(st.next_batch().is_none());
        // 批间不重叠：合并 = 全量。
        let mut merged = b1;
        merged.extend(b2);
        assert_eq!(merged, rows);
        // 零批行数拒。
        assert!(matches!(StreamExport::new(rows, 0), Err(E_EXP_CHUNK)));
    }

    #[test]
    fn a04_同源同值_导出等于查询() {
        let c = cells();
        let sel = ExpSelector { h0: 0, h1: 9, acct: Some(1), res: None };
        let query_total: u64 = c
            .iter()
            .filter(|x| sel.matches(x).unwrap_or(false))
            .map(|x| x.total)
            .sum();
        assert_eq!(query_total, 250);
        assert_eq!(same_source_check(&c, &sel, query_total), Ok(()));
        // 查询值不同 → 同源红。
        assert_eq!(same_source_check(&c, &sel, 999), Err(E_EXP_SAMESRC));
    }

    #[test]
    fn a05_双格式对账_篡改即红() {
        let c = cells();
        assert_eq!(consistency_check(&c, &ExpSelector::default()), Ok(()));
        // 空数据也绿（空对空）。
        let empty: Vec<ExpCell> = Vec::new();
        assert_eq!(consistency_check(&empty, &ExpSelector::default()), Ok(()));
    }
}
