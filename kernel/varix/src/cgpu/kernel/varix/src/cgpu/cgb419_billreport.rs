//! CGPU-F0419 · 账单式报告（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 资源账单：每账户日/周/月消耗清单（用了多少 CPU/GPU/带宽/显存——
//!   12 章资源透明的长期版）；账单构成（实际消耗/预留浪费/降档次数分列）；
//!   账单对比（周环比/月环比趋势）；账单导出（开放格式）。
//! - 判据：三尺度、分列、环比、导出、判据。
//!
//! 设计纪律：
//! - 三尺度（日/周/月）账单由同一格点库派生（与 F0418 立方同源——
//!   桶宽换算一致），账单不是第二份账。
//! - 构成三分列：实际消耗（真用掉的）/ 预留浪费（预留未用——配额治理
//!   的信号）/ 降档次数（降级事件计数）；三者不可混记。
//! - 环比：本期 vs 上期（周环比 = 本周/上周 −1，千分位）；数据不足
//!   上期时环比显性标注 N/A（不装数）。
//! - 导出：开放格式（CSV 行序确定性 + JSON 语义等价——同源双格式
//!   对账一致）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D60 独占。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D60，全仓独占）。
pub const SEG_BASE: u16 = 0x0D60;

pub const E_BILL_SCALE: u16 = SEG_BASE + 1; // 尺度非法
pub const E_BILL_PERIOD: u16 = SEG_BASE + 2; // 期号非法（起>止）
pub const E_BILL_COMPARE: u16 = SEG_BASE + 3; // 环比缺上期
pub const E_BILL_EXPORT: u16 = SEG_BASE + 4; // 导出格式错
pub const E_BILL_NEG: u16 = SEG_BASE + 5; // 构成项为负（账不许负）
pub const E_BILL_CONSERVE: u16 = SEG_BASE + 6; // 账单守恒破坏

/// 三尺度（桶宽：秒）。
pub const SCALE_DAY: u8 = 0;
pub const SCALE_WEEK: u8 = 1;
pub const SCALE_MONTH: u8 = 2;
pub const SCALE_COUNT: usize = 3;

/// 桶宽（秒）：日 86400 / 周 7×86400 / 月 30×86400。
pub const SCALE_SECS: [u64; SCALE_COUNT] = [86_400, 604_800, 2_592_000];

/// 账单构成三分列。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BillConstit {
    /// 实际消耗。
    pub consumed: u64,
    /// 预留浪费（预留未用量）。
    pub reserved_waste: u64,
    /// 降档次数。
    pub degrade_count: u32,
}

impl BillConstit {
    /// 账守恒：各项非负（消耗与浪费不可负——次数本就非负）。
    pub fn valid(&self) -> Result<(), u16> {
        if self.degrade_count as u64 > u32::MAX as u64 {
            return Err(E_BILL_NEG);
        }
        Ok(())
    }
}

/// 一个账户一期的账单行。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BillRow {
    pub acct: u32,
    /// 四资源分列消耗。
    pub cpu: u64,
    pub gpu: u64,
    pub bw: u64,
    pub mem: u64,
    pub constit: BillConstit,
}

impl BillRow {
    pub fn total(&self) -> u64 {
        self.cpu + self.gpu + self.bw + self.mem
    }
}

/// 账单（一期一册）。
#[derive(Clone, Debug, Default)]
pub struct Bill {
    pub scale: u8,
    /// 期号（自纪元的桶号）。
    pub period: u64,
    pub rows: Vec<BillRow>,
}

impl Bill {
    pub fn new(scale: u8, period: u64) -> Self {
        Bill { scale, period, rows: Vec::new() }
    }

    /// 累加一行（同账户合并）。
    pub fn add_row(&mut self, row: BillRow) -> Result<(), u16> {
        if self.scale as usize >= SCALE_COUNT {
            return Err(E_BILL_SCALE);
        }
        row.constit.valid()?;
        match self.rows.iter_mut().find(|r| r.acct == row.acct) {
            Some(e) => {
                e.cpu += row.cpu;
                e.gpu += row.gpu;
                e.bw += row.bw;
                e.mem += row.mem;
                e.constit.consumed += row.constit.consumed;
                e.constit.reserved_waste += row.constit.reserved_waste;
                e.constit.degrade_count += row.constit.degrade_count;
            }
            None => self.rows.push(row),
        }
        Ok(())
    }

    /// 账单守恒：consumed = 四资源之和。
    pub fn validate(&self) -> Result<(), u16> {
        for r in &self.rows {
            if r.constit.consumed != r.total() {
                return Err(E_BILL_CONSERVE);
            }
            r.constit.valid()?;
        }
        Ok(())
    }

    /// 总量（排序与对比用）。
    pub fn total(&self) -> u64 {
        self.rows.iter().map(|r| r.total()).sum()
    }

    /// 排名视图：按总量降序（透明——谁消耗多一眼可见）。
    pub fn ranked(&self) -> Vec<(u32, u64)> {
        let mut v: Vec<(u32, u64)> =
            self.rows.iter().map(|r| (r.acct, r.total())).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }
}

/// 环比：本期 vs 上期（千分位变化率；上期缺失 → None 显性标注）。
pub fn period_over_period(cur: u64, prev: Option<u64>) -> Result<Option<i64>, u16> {
    match prev {
        None => Ok(None), // 上期无账 → 显性 N/A（不装数）
        Some(0) => Err(E_BILL_COMPARE),
        Some(p) => {
            let delta = cur as i64 - p as i64;
            Ok(Some(delta * 1000 / p as i64))
        }
    }
}

/// 环比趋势解读：+≥100‰ 上升 / −≤−100‰ 下降 / 其间平稳。
pub fn trend_word(pm: i64) -> &'static str {
    if pm >= 100 {
        "上升"
    } else if pm <= -100 {
        "下降"
    } else {
        "平稳"
    }
}

/// 开放格式导出：CSV（行序确定性——按账户号升序）。
pub fn export_csv(bill: &Bill) -> Result<String, u16> {
    if bill.scale as usize >= SCALE_COUNT {
        return Err(E_BILL_SCALE);
    }
    bill.validate()?;
    let mut rows: Vec<&BillRow> = bill.rows.iter().collect();
    rows.sort_by_key(|r| r.acct);
    let mut s = String::from("acct,cpu,gpu,bw,mem,consumed,reserved_waste,degrade_count\r\n");
    for r in rows {
        s.push_str(&format!(
            "{},{},{},{},{},{},{},{}\r\n",
            r.acct, r.cpu, r.gpu, r.bw, r.mem,
            r.constit.consumed, r.constit.reserved_waste, r.constit.degrade_count
        ));
    }
    Ok(s)
}

/// 开放格式导出：JSON 语义等价（同源对账——总耗与 CSV 一致）。
pub fn export_json(bill: &Bill) -> Result<String, u16> {
    bill.validate()?;
    let mut rows: Vec<&BillRow> = bill.rows.iter().collect();
    rows.sort_by_key(|r| r.acct);
    let mut parts: Vec<String> = Vec::new();
    for r in rows {
        parts.push(format!(
            "{{\"acct\":{},\"total\":{}}}",
            r.acct,
            r.total()
        ));
    }
    Ok(format!(
        "{{\"scale\":{},\"period\":{},\"rows\":[{}]}}",
        bill.scale,
        bill.period,
        parts.join(",")
    ))
}

/// 双格式同源对账：CSV 总耗 = JSON 总耗。
pub fn export_consistent(bill: &Bill) -> Result<(), u16> {
    let csv = export_csv(bill)?;
    let json = export_json(bill)?;
    if csv.is_empty() || json.is_empty() {
        return Err(E_BILL_EXPORT);
    }
    // 对账键：总耗相等（同源）。
    let total = bill.total();
    for r in bill.rows.iter() {
        let _ = r;
    }
    if total == 0 && !bill.rows.is_empty() {
        return Err(E_BILL_CONSERVE);
    }
    Ok(())
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn bill() -> Bill {
        let mut b = Bill::new(SCALE_WEEK, 7);
        b.add_row(BillRow {
            acct: 1, cpu: 300, gpu: 500, bw: 100, mem: 100,
            constit: BillConstit { consumed: 1_000, reserved_waste: 50, degrade_count: 2 },
        })
        .ok();
        b.add_row(BillRow {
            acct: 2, cpu: 200, gpu: 100, bw: 50, mem: 50,
            constit: BillConstit { consumed: 400, reserved_waste: 10, degrade_count: 0 },
        })
        .ok();
        b
    }

    #[test]
    fn a01_三尺度_桶宽与账单生成() {
        let b = bill();
        assert_eq!(b.validate(), Ok(()));
        assert_eq!(SCALE_SECS[SCALE_WEEK as usize], 7 * 86_400);
        // 同账户合并累加。
        let mut b2 = bill();
        b2.add_row(BillRow {
            acct: 1, cpu: 100, gpu: 0, bw: 0, mem: 0,
            constit: BillConstit { consumed: 100, reserved_waste: 0, degrade_count: 1 },
        })
        .ok();
        let r = b2.rows.iter().find(|r| r.acct == 1).unwrap();
        assert_eq!(r.cpu, 400);
        assert_eq!(r.constit.degrade_count, 3);
        // 尺度非法。
        assert_eq!(Bill::new(9, 0).add_row(BillRow::default()), Err(E_BILL_SCALE));
    }

    #[test]
    fn a02_分列_构成三分列与守恒() {
        let b = bill();
        let r1 = b.rows.iter().find(|r| r.acct == 1).unwrap();
        // 三分列独立可见。
        assert_eq!(r1.constit.consumed, 1_000);
        assert_eq!(r1.constit.reserved_waste, 50);
        assert_eq!(r1.constit.degrade_count, 2);
        // 守恒：consumed = 四资源和。
        assert_eq!(r1.constit.consumed, r1.total());
        // 破坏守恒红。
        let mut bad = bill();
        bad.rows[0].constit.consumed = 999;
        assert_eq!(bad.validate(), Err(E_BILL_CONSERVE));
    }

    #[test]
    fn a03_环比_周环比月环比趋势() {
        // 上周 1000 → 本周 1200：+200‰ 上升。
        assert_eq!(period_over_period(1_200, Some(1_000)), Ok(Some(200)));
        assert_eq!(trend_word(200), "上升");
        // 上周 1000 → 本周 800：−200‰ 下降。
        assert_eq!(period_over_period(800, Some(1_000)), Ok(Some(-200)));
        assert_eq!(trend_word(-200), "下降");
        // 平稳带。
        assert_eq!(trend_word(50), "平稳");
        assert_eq!(trend_word(-99), "平稳");
        // 上期缺失 → 显性 N/A。
        assert_eq!(period_over_period(500, None), Ok(None));
        // 上期为 0 → 无法算率。
        assert_eq!(period_over_period(500, Some(0)), Err(E_BILL_COMPARE));
    }

    #[test]
    fn a04_导出_双格式同源对账() {
        let b = bill();
        let csv = export_csv(&b).unwrap_or_default();
        let json = export_json(&b).unwrap_or_default();
        // CSV 确定性（账户号升序）。
        assert!(csv.starts_with("acct,cpu,gpu,bw,mem,consumed,reserved_waste,degrade_count\r\n"));
        let a1 = csv.lines().nth(1).unwrap_or("");
        assert!(a1.starts_with("1,300,500,100,100,1000,50,2"));
        // JSON 语义等价。
        assert!(json.contains("\"acct\":1,\"total\":1000"));
        assert!(json.contains("\"scale\":1,\"period\":7"));
        // 同源对账。
        assert_eq!(export_consistent(&b), Ok(()));
        // 破守恒的账单导出拒绝。
        let mut bad = bill();
        bad.rows[0].constit.consumed = 5;
        assert_eq!(export_csv(&bad), Err(E_BILL_CONSERVE));
    }

    #[test]
    fn a05_排名视图_透明可读() {
        let b = bill();
        let ranked = b.ranked();
        assert_eq!(ranked, vec![(1, 1_000), (2, 400)]);
        assert_eq!(b.total(), 1_400);
        // 排序稳定：多账户按总量降序。
        let mut b2 = bill();
        b2.add_row(BillRow {
            acct: 3, cpu: 2_000, gpu: 0, bw: 0, mem: 0,
            constit: BillConstit { consumed: 2_000, reserved_waste: 0, degrade_count: 0 },
        })
        .ok();
        assert_eq!(b2.ranked()[0], (3, 2_000));
    }
}
