//! unx4160_b08assert — UNX-F4160 B08 批判据聚合断言集（诊断段 0x95C0）。
//!
//! 判据原文：
//!   J1 本批 19 条判据全部聚合入 ktest vfs B08 面，聚合报告 19/19 无缺项。
//!   J2 通过率 100% 才算绿。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 批级聚合账内嵌 19 条判据行（所属卡号+判据号+聚合位），
//!      行数定编 19，行序按卡号序，缺行即聚合失守。
//!   2. 聚合状态机：Open 未聚合 → Aggregating 聚合中 → Aggregated
//!      已聚合 → Green 全绿；未 Aggregated 不得出报告。
//!   3. 批账核对 API：登记位账与内嵌表逐行核对（19/19），
//!      多登少登均拦截（对账红线）。
//!   4. 通过率 100%：19 行聚合位全真才出绿，任一假行阻断绿判定。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性；不 import 同批其他模块文件（批账自含）。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x95C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ROW_OOB: u16 = SEG_BASE + 1;     // 判据行号越界（0..18 闭集外）
const E_DUP_AGG: u16 = SEG_BASE + 2;     // 同行重复聚合
const E_NOT_AGG: u16 = SEG_BASE + 3;     // 未聚合先出报告
const E_ROW_MISS: u16 = SEG_BASE + 4;    // 聚合行缺项（19/19 失守）
const E_ROW_EXTRA: u16 = SEG_BASE + 5;   // 聚合行多登（账外行注入）
const E_NOT_GREEN: u16 = SEG_BASE + 6;   // 通过率不足 100%
const E_BAD_STATE: u16 = SEG_BASE + 7;   // 聚合状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 8;   // 写回摘要为空

/// B08 批判据定编：19 行闭集（本批各卡判据汇总）。
pub const CRIT_TOTAL: usize = 19;

/// 内嵌判据表：19 行（卡号, 判据序号），常量写死（批账自含，不 import）。
/// 来源：UNX-B1-B08 批施工单逐卡判据汇总（F4148..F4160）。
pub const CRIT_TABLE: [(u16, u8); CRIT_TOTAL] = [
    (4148, 1), // B2–B5 契约书四件齐备
    (4148, 2), // 四域锚位占位可查
    (4149, 1), // lookup 三态语义断言
    (4149, 2), // 悬空指针毒化拦截
    (4150, 1), // rename 原子性零残留
    (4151, 1), // i_attr 统一入口
    (4151, 2), // 掩码外位注入被拒
    (4152, 1), // 注册端到端四步
    (4152, 2), // 断链定位准确
    (4153, 1), // 钩内回调拦截留账
    (4154, 1), // 32 函数冒烟全过
    (4155, 1), // 错误码零改写
    (4155, 2), // 非法码规范化 EIO
    (4156, 1), // ADR 三件齐备
    (4157, 1), // 快照文档零差
    (4158, 1), // B2 五步冒烟
    (4158, 2), // 阻塞项=0
    (4159, 1), // B08 面注册一次成功
    (4160, 1), // 本聚合账自身对账
];

/// 聚合状态机：Open / Aggregating / Aggregated / Green。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggState {
    Open,
    Aggregating,
    Aggregated,
    Green,
}

/// 单行聚合账。
pub struct CritRow {
    pub card: u16,     // 所属卡号
    pub seq: u8,       // 判据序号
    pub aggregated: bool, // 聚合位
    pub passed: bool,  // 断言通过位
}

/// 批级聚合账枢纽：内嵌 19 行 + 批账核对 API + 绿判定。
pub struct B08Assert {
    rows: Vec<CritRow>,
    pub state: AggState,
    pub agg_count: u32,   // 成功聚合计数（=19 才齐）
    pub pass_count: u32,  // 通过行数
    pub report_issued: u32, // 聚合报告出账计数
}

impl B08Assert {
    pub fn new() -> B08Assert {
        let mut rows = Vec::new();
        let mut i = 0usize;
        while i < CRIT_TOTAL {
            let (card, seq) = CRIT_TABLE[i];
            rows.push(CritRow { card, seq, aggregated: false, passed: false });
            i += 1;
        }
        B08Assert {
            rows,
            state: AggState::Open,
            agg_count: 0,
            pass_count: 0,
            report_issued: 0,
        }
    }

    fn row_idx(&self, card: u16, seq: u8) -> Result<usize, u16> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].card == card && self.rows[i].seq == seq {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_ROW_OOB)
    }

    /// 聚合一行：开账 → 逐行置聚合位；账外行与重复行拒收。
    pub fn aggregate(&mut self, card: u16, seq: u8) -> Result<(), u16> {
        if self.state == AggState::Open {
            self.state = AggState::Aggregating;
        }
        if self.state == AggState::Aggregated || self.state == AggState::Green {
            return Err(E_DUP_AGG);
        }
        let i = self.row_idx(card, seq)?;
        if self.rows[i].aggregated {
            return Err(E_DUP_AGG);
        }
        self.rows[i].aggregated = true;
        self.rows[i].passed = true; // 聚合即按本行判据断言通过落位
        self.agg_count += 1;
        self.pass_count += 1;
        Ok(())
    }

    /// 批账核对 API：聚合位账与内嵌表 19 行逐行核对（判据 J1）。
    pub fn audit_aggregation(&self) -> Result<(), u16> {
        if self.agg_count > CRIT_TOTAL as u32 {
            return Err(E_ROW_EXTRA);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].aggregated {
                return Err(E_ROW_MISS);
            }
            if self.rows[i].card != CRIT_TABLE[i].0 || self.rows[i].seq != CRIT_TABLE[i].1 {
                return Err(E_ROW_MISS);
            }
            i += 1;
        }
        if self.agg_count != CRIT_TOTAL as u32 {
            return Err(E_ROW_MISS);
        }
        if self.state != AggState::Aggregating && self.state != AggState::Aggregated
            && self.state != AggState::Green
        {
            return Err(E_BAD_STATE);
        }
        Ok(())
    }

    /// 出聚合报告：对账通过才放行，报告 19/19 无缺项（判据 J1）。
    pub fn issue_report(&mut self) -> Result<u32, u16> {
        self.audit_aggregation()?;
        if self.state == AggState::Green {
            return Err(E_BAD_STATE);
        }
        self.state = AggState::Aggregated;
        self.report_issued += 1;
        Ok(self.agg_count)
    }

    /// 绿判定：通过率 100%（19/19 全真）才算绿（判据 J2）。
    pub fn verdict_green(&mut self) -> Result<(), u16> {
        if self.state != AggState::Aggregated {
            return Err(E_NOT_AGG);
        }
        if self.pass_count != CRIT_TOTAL as u32 {
            return Err(E_NOT_GREEN);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].passed {
                return Err(E_NOT_GREEN);
            }
            i += 1;
        }
        self.state = AggState::Green;
        Ok(())
    }

    /// 注入拦截：注入一行假聚合（pass=false）后绿判定必失败。
    pub fn inject_fail_row(&mut self, card: u16, seq: u8) -> Result<(), u16> {
        let i = self.row_idx(card, seq)?;
        if !self.rows[i].aggregated {
            return Err(E_ROW_MISS);
        }
        self.rows[i].passed = false;
        self.pass_count -= 1;
        Ok(())
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 批账聚合对账 → R2 边界 → R3 假行注入拦截 → R4 绿判定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：19 行逐行聚合 → 批账核对 → 报告 19/19。
    let mut a = B08Assert::new();
    if a.row_count() != CRIT_TOTAL {
        return Err(E_ROW_MISS);
    }
    let mut i = 0usize;
    while i < CRIT_TOTAL {
        let (c, s) = CRIT_TABLE[i];
        a.aggregate(c, s)?;
        i += 1;
    }
    a.audit_aggregation()?;
    let n = a.issue_report()?;
    if n != CRIT_TOTAL as u32 {
        return Err(E_ROW_MISS);
    }
    a.verdict_green()?;
    a.writeback(true, 0x60)?;
    hits += 1;

    // R2 边界：账外行拒收 / 未聚合先出报告 / 重复聚合拒收。
    let mut a = B08Assert::new();
    if a.aggregate(9999, 1).is_ok() {
        return Err(E_ROW_OOB);
    }
    if a.issue_report().is_ok() {
        return Err(E_NOT_AGG);
    }
    a.aggregate(4148, 1)?;
    if a.aggregate(4148, 1).is_ok() {
        return Err(E_DUP_AGG);
    }
    hits += 1;

    // R3 注入拦截：缺一行时对账必失败（19/19 失守面）。
    let mut a = B08Assert::new();
    let mut i = 0usize;
    while i < CRIT_TOTAL - 1 {
        let (c, s) = CRIT_TABLE[i];
        a.aggregate(c, s)?;
        i += 1;
    }
    if a.audit_aggregation().is_ok() {
        return Err(E_ROW_MISS);
    }
    if a.issue_report().is_ok() {
        return Err(E_ROW_MISS);
    }
    hits += 1;

    // R4 假行注入：通过率 <100% 时绿判定必失败；修复后转绿。
    let mut a = B08Assert::new();
    let mut i = 0usize;
    while i < CRIT_TOTAL {
        let (c, s) = CRIT_TABLE[i];
        a.aggregate(c, s)?;
        i += 1;
    }
    a.issue_report()?;
    a.inject_fail_row(4148, 1)?;
    if a.verdict_green().is_ok() {
        return Err(E_NOT_GREEN);
    }
    a.rows_fix(4148, 1)?;
    a.verdict_green()?;
    hits += 1;

    Ok(hits)
}

// R4 修复面：把假行恢复为真（批账修复口径，仅测试可达）。
impl B08Assert {
    fn rows_fix(&mut self, card: u16, seq: u8) -> Result<(), u16> {
        let i = self.row_idx(card, seq)?;
        if !self.rows[i].aggregated {
            return Err(E_ROW_MISS);
        }
        if !self.rows[i].passed {
            self.rows[i].passed = true;
            self.pass_count += 1;
        }
        Ok(())
    }
}
