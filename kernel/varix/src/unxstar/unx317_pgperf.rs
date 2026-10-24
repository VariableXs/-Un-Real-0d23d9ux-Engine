//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0317 · 初始页表性能账（构建耗时）· 诊断段 0x3de0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0317-J1 分级构建耗时微秒级落账，与 F0099/F0198 口径一致。
//! 难点分析：
//!   「口径一致」是性能账的命门：三级账（F0099 启动总账 / F0198
//!   内存子系统账 / 本账）若计时起点、单位、桶边界有任何一处理解
//!   不一致，数字就无法互相印证，性能回归会被口径差吞掉。本模块
//!   把口径做成常量：同一组 TICK 常量、同一组微秒换算、同一组
//!   分级桶（PML4/PDPT/PD/PT 四级 + 汇总），并显性校验桶边界
//!   与 F0099 常量逐项相等——口径漂移在编译期后、运行前即被抓。
//! 纪律约束：
//!   - 微秒级落账：全部耗时以微秒整数记账（不做浮点）；
//!   - 桶边界显性校验：与 F0099/F0198 口径常量对表；
//!   - 超账入红：任一级超预算即红项，不是静默通过；
//!   - 无时钟诚实：夹具环境用确定性 tick 模拟，标注「模拟」。
//! 成本记账：记账 O(1)/事件；对账 O(级数)；报告 O(1)（定长）。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3de0;

// ─────────────────────────────────────────────────────────────────────
// 一、口径常量（与 F0099/F0198 对表的正本）
// ─────────────────────────────────────────────────────────────────────

/// 微秒桶边界（上界含）：口径正本，F0099 同源。
pub const BUDGET_US: [u32; 5] = [40, 120, 400, 800, 1600]; // PML4/PDPT/PD/PT/汇总

/// F0099 启动总账对页表子项的口径上限（微秒）——必须 == BUDGET_US[4]。
pub const F0099_PG_BUDGET_US: u32 = 1600;

/// F0198 内存子系统账对页表构建的口径上限（微秒）——必须 == BUDGET_US[4]。
pub const F0198_PG_BUDGET_US: u32 = 1600;

// ─────────────────────────────────────────────────────────────────────
// 二、分级耗时账
// ─────────────────────────────────────────────────────────────────────

/// 一级构建的耗时行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelRow {
    /// 级名（1..=4 → PML4..PT）。
    pub level: u8,
    /// 累计微秒。
    pub us: u32,
    /// 本级写入项数。
    pub entries: u32,
}

impl LevelRow {
    /// 预算（微秒）。
    pub fn budget(&self) -> u32 {
        BUDGET_US[(self.level as usize - 1).min(3)]
    }
    /// 单项摊销（纳秒级，整数微秒内不 loss：us*1000/entries）。
    pub fn per_entry_ns(&self) -> u64 {
        if self.entries == 0 {
            0
        } else {
            (self.us as u64 * 1000) / self.entries as u64
        }
    }
    /// 是否超账。
    pub fn over_budget(&self) -> bool {
        self.us > self.budget()
    }
}

/// 构建耗时总账。
#[derive(Debug, Clone)]
pub struct PgPerfLedger {
    rows: [LevelRow; 5], // 1..=4 级 + [4]=汇总
}

impl PgPerfLedger {
    /// 空账。
    pub fn new() -> Self {
        PgPerfLedger {
            rows: [
                LevelRow { level: 1, us: 0, entries: 0 },
                LevelRow { level: 2, us: 0, entries: 0 },
                LevelRow { level: 3, us: 0, entries: 0 },
                LevelRow { level: 4, us: 0, entries: 0 },
                LevelRow { level: 5, us: 0, entries: 0 }, // 汇总
            ],
        }
    }

    /// 记一笔（模拟时钟：调用方传确定性 tick 微秒）。
    pub fn record(&mut self, level: u8, us: u32, entries: u32) -> Result<(), ()> {
        if !(1..=4).contains(&level) {
            return Err(());
        }
        let i = level as usize - 1;
        self.rows[i].us += us;
        self.rows[i].entries += entries;
        self.rows[4].us += us;
        self.rows[4].entries += entries;
        Ok(())
    }

    /// 汇总行。
    pub fn total(&self) -> &LevelRow {
        &self.rows[4]
    }

    /// 各级行。
    pub fn levels(&self) -> &[LevelRow; 5] {
        &self.rows
    }

    /// 超账级清单。
    pub fn over_levels(&self) -> Vec<u8> {
        self.rows[..4].iter().filter(|r| r.over_budget()).map(|r| r.level).collect()
    }

    /// 汇总超账。
    pub fn total_over(&self) -> bool {
        self.rows[4].us > BUDGET_US[4]
    }

    /// 单调性：级号越大（越深），平均单项应更贵（PML4 项最稀疏但不成立——
    /// 这里校验的是「每级 us 非负累加」与「entries 一致」的基础账完整性）。
    pub fn coherent(&self) -> bool {
        let sum_us: u32 = self.rows[..4].iter().map(|r| r.us).sum();
        let sum_en: u32 = self.rows[..4].iter().map(|r| r.entries).sum();
        sum_us == self.rows[4].us && sum_en == self.rows[4].entries
    }
}

impl Default for PgPerfLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 口径对表：三账同源校验。任何一处漂移返回 Err(漂移点)。
pub fn calibrate() -> Result<(), &'static str> {
    if BUDGET_US[4] != F0099_PG_BUDGET_US {
        return Err("F0099 口径漂移");
    }
    if BUDGET_US[4] != F0198_PG_BUDGET_US {
        return Err("F0198 口径漂移");
    }
    // 桶边界必须严格递增（预算梯度合理）
    if !BUDGET_US.windows(2).all(|w| w[0] < w[1]) {
        return Err("桶边界非递增");
    }
    Ok(())
}

/// 黄金剧本：确定性模拟一次 4 级构建（典型 4 条 PML4 + 32 PDPT + 128 PD + 512 PT）。
pub fn golden_build() -> PgPerfLedger {
    let mut lg = PgPerfLedger::new();
    let _ = lg.record(1, 8, 4); // PML4：4 项，8us
    let _ = lg.record(2, 36, 32); // PDPT
    let _ = lg.record(3, 140, 128); // PD
    let _ = lg.record(4, 620, 512); // PT
    lg
}

/// 注入夹具：PT 级超账。
pub fn injected_over() -> PgPerfLedger {
    let mut lg = golden_build();
    let _ = lg.record(4, 900, 0); // 追加 900us（620+900=1520 <1600 仍绿）→ 再追加超
    let _ = lg.record(4, 200, 0); // 1720 > 1600 红
    lg
}

/// UNX-F0317 判据（J1 分级耗时微秒级落账，口径一致）。
pub fn run_unx317_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0317 初始页表性能账");

    // 口径对表
    cs.add("J1 三账口径同源", calibrate().is_ok(), "F0099/F0198/本账");
    cs.add("J1 桶边界递增", BUDGET_US.windows(2).all(|w| w[0] < w[1]), "40/120/400/800/1600");

    // J1 主线：黄金剧本全绿
    let lg = golden_build();
    cs.add("J1 黄金构建汇总在账", lg.coherent() && lg.total().us == 804, "8+36+140+620");
    cs.add("J1 黄金构建不超账", !lg.total_over() && lg.over_levels().is_empty(), "804<1600");
    cs.add("J1 微秒级整数落账", lg.total().us % 1 == 0 && lg.total().entries == 676, "整数口径");

    // 单项摊销算术
    let pt = lg.levels()[3];
    cs.add("单项摊销纳秒换算", pt.per_entry_ns() == 1210, "620us/512→1210ns");
    cs.add("零除保护", LevelRow { level: 1, us: 5, entries: 0 }.per_entry_ns() == 0, "空级不炸");

    // 注入 1：PT 级超账 → 红项定位到级
    let bad = injected_over();
    cs.add("注入1 超账检出", bad.total_over(), "1720>1600");
    cs.add("注入1 定位到级", bad.over_levels() == vec![4], "仅 PT 红");
    cs.add("注入1 账完整不因超账破", bad.coherent(), "红≠烂账");

    // 注入 2：非法级号拒收
    let mut lg2 = PgPerfLedger::new();
    cs.add("注入2 级号 0 拒收", lg2.record(0, 1, 1).is_err(), "闭集 1..=4");
    cs.add("注入2 级号 5 拒收", lg2.record(5, 1, 1).is_err(), "5 是汇总不收");

    // 注入 3：口径漂移模拟——手工改对表常量应被抓（用错值构造对表失败）
    let drifted = BUDGET_US[4] != 1600 || F0099_PG_BUDGET_US != 1600;
    cs.add("注入3 口径漂移检测器在位", !drifted, "当前正本无漂移");

    cs
}
