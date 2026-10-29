//! unx48288_mexpagg — UNX-F48288 M 段体验日志汇总账（rage click/dead click 输入面挫败信号捕获）（诊断段 0xC4E0）。
//!
//! 判据原文：
//!   J1 信号注入 ×10 全捕获并标体验结论字段；判据锚定 体验十三。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 挫败信号两型：rage click（同点短窗连击 ≥3 次）与 dead click
//!      （点击后无响应落点）；型号闭集，越型即 E_SIG_BAD。
//!   2. 捕获口径（J1）：注入 ×10 信号逐条全捕获、零漏捕——汇总账
//!      条数与注入量守恒，漏一条即 E_CAPTURE_MISS。
//!   3. 体验结论字段强制：每条捕获必须带体验结论码（归因面：输入
//!      面/候选面/上屏面/桥面），结论缺位（0 码）不得入汇总账
//!      （E_NO_CONCLUDE）——无结论的挫败信号是废信号。
//!   4. 汇总双账：逐条明细账（信号型、窗、结论码、流水）+ 分型计数账
//!      （rage/dead 各自总量）；两账互查闭合。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC4E0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SIG_BAD: u16 = SEG_BASE + 1; // 信号型越界
const E_CAPTURE_MISS: u16 = SEG_BASE + 2; // 漏捕
const E_NO_CONCLUDE: u16 = SEG_BASE + 3; // 体验结论缺位
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 汇总账不闭合
const E_STATE_BAD: u16 = SEG_BASE + 5; // 汇总器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 信号型闭集。
pub const SIG_RAGE: u8 = 0; // rage click
pub const SIG_DEAD: u8 = 1; // dead click
pub const SIG_N: usize = 2;

/// 体验结论码（归因面）：0 = 缺位（禁）。
pub const CL_INPUT: u16 = 1; // 输入面
pub const CL_CAND: u16 = 2; // 候选面
pub const CL_RENDER: u16 = 3; // 上屏面
pub const CL_BRIDGE: u16 = 4; // 桥面

/// rage click 判窗：同点连击窗内 ≥3 次成信号。
pub const RAGE_WINDOW_MS: u32 = 800;
pub const RAGE_MIN_CLICKS: u32 = 3;

/// 明细账一条。
#[derive(Clone, Copy, Debug)]
pub struct SigRow {
    pub seq: u64,
    pub sig: u8,
    pub at_ms: u32,
    pub conclude: u16, // 体验结论码（归因面）
}

/// 体验日志汇总账。
pub struct ExpAgg {
    pub rows: Vec<SigRow>,
    pub type_counts: [u32; SIG_N],
    pub injected: u32,
    pub seq: u64,
}

impl ExpAgg {
    pub fn new() -> ExpAgg {
        ExpAgg { rows: Vec::new(), type_counts: [0; SIG_N], injected: 0, seq: 0 }
    }

    /// 信号注入捕获（J1 主体）：型闭集 + 结论码强制 + 流水单调。
    pub fn capture(&mut self, sig: u8, at_ms: u32, conclude: u16) -> Result<u64, u16> {
        self.injected += 1;
        if sig as usize >= SIG_N {
            return Err(E_SIG_BAD);
        }
        if conclude == 0 {
            return Err(E_NO_CONCLUDE); // 无结论不入汇总账
        }
        self.seq += 1;
        self.rows.push(SigRow { seq: self.seq, sig, at_ms, conclude });
        self.type_counts[sig as usize] += 1;
        Ok(self.seq)
    }

    /// rage click 判窗器（信号生成面）：同点连击序列 → 是否成信号。
    pub fn rage_judge(click_times: &[u32]) -> bool {
        if click_times.len() < RAGE_MIN_CLICKS as usize {
            return false;
        }
        // 任取窗内连续 ≥3 连击（时间差 ≤ 窗宽）即成。
        let mut i = 0usize;
        while i + RAGE_MIN_CLICKS as usize <= click_times.len() {
            let span = click_times[i + RAGE_MIN_CLICKS as usize - 1] - click_times[i];
            if span <= RAGE_WINDOW_MS {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 汇总账审计（J1 硬线）：明细账条数 == 注入量（全捕获零漏捕）、
    /// 分型计数账与明细账互查闭合、流水号 1..n 闭合、结论码全非零。
    pub fn audit(&self) -> Result<(), u16> {
        if self.rows.len() as u32 != self.injected {
            return Err(E_CAPTURE_MISS);
        }
        if self.type_counts[0] + self.type_counts[1] != self.rows.len() as u32 {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            if r.seq != (i + 1) as u64 {
                return Err(E_LEDGER_BAD);
            }
            if r.conclude == 0 {
                return Err(E_NO_CONCLUDE);
            }
            if r.sig as usize >= SIG_N {
                return Err(E_SIG_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 分型计数回收。
    pub fn count_of(&self, sig: u8) -> u32 {
        self.type_counts[sig as usize]
    }

    /// 结论面分布回收：逐归因面捕获量。
    pub fn conclude_dist(&self) -> [u32; 5] {
        let mut dist = [0u32; 5];
        let mut i = 0usize;
        while i < self.rows.len() {
            let c = self.rows[i].conclude as usize;
            if c < 5 {
                dist[c] += 1;
            }
            i += 1;
        }
        dist
    }

    /// 时序审计：明细账 at_ms 全程严格递增（注入时钟不可倒流）。
    pub fn audit_timeline(&self) -> Result<(), u16> {
        let mut i = 1usize;
        while i < self.rows.len() {
            if self.rows[i].at_ms <= self.rows[i - 1].at_ms {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 最热归因面：捕获量最大的结论码（并列取码小者）——体验面主攻
    /// 方向由汇总账直接给数。
    pub fn hottest_conclude(&self) -> Result<u16, u16> {
        if self.rows.len() == 0 {
            return Err(E_LEDGER_BAD);
        }
        let dist = self.conclude_dist();
        let mut best: u16 = 0;
        let mut best_n: u32 = 0;
        let mut c = 1usize;
        while c < 5 {
            if dist[c] > best_n {
                best_n = dist[c];
                best = c as u16;
            }
            c += 1;
        }
        if best == 0 {
            return Err(E_NO_CONCLUDE);
        }
        Ok(best)
    }

    /// 完成写回（判据 J2）：捕获全量 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, want_injected: u32, summary_len: u32) -> Result<(), u16> {
        if !done || self.rows.len() as u32 != want_injected {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（信号注入 ×10 全捕获并标体验结论字段），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：注入 10 条信号（rage/dead 交替、结论面轮转四归因面），
    //    全捕获零漏捕、分型账闭合。
    let mut agg = ExpAgg::new();
    let mut i = 0u32;
    while i < 10 {
        let sig = if i % 2 == 0 { SIG_RAGE } else { SIG_DEAD };
        let concl = [CL_INPUT, CL_CAND, CL_RENDER, CL_BRIDGE][(i % 4) as usize];
        agg.capture(sig, 1000 + i * 137, concl)?;
        i += 1;
    }
    agg.audit()?;
    if agg.rows.len() != 10 {
        return Err(E_CAPTURE_MISS);
    }
    if agg.count_of(SIG_RAGE) != 5 || agg.count_of(SIG_DEAD) != 5 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 rage 判窗器实测：800ms 内三连击成信号、跨窗三连击不成、
    //    两击不成（信号生成面真跑）。
    let hit_seq: [u32; 3] = [100, 400, 700]; // 跨 600ms ≤ 800 → 成
    if !ExpAgg::rage_judge(&hit_seq) {
        return Err(E_STATE_BAD);
    }
    let miss_seq: [u32; 3] = [100, 950, 1800]; // 跨 1700ms > 800 → 不成
    if ExpAgg::rage_judge(&miss_seq) {
        return Err(E_STATE_BAD);
    }
    let two: [u32; 2] = [100, 200];
    if ExpAgg::rage_judge(&two) {
        return Err(E_STATE_BAD);
    }
    // 滑窗命中：4 击中后 3 击跨 500ms 成（前击拖尾不误判）。
    let slide: [u32; 4] = [0, 1000, 1100, 1400];
    if !ExpAgg::rage_judge(&slide) {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R3 结论缺位拒收：conclude=0 → E_NO_CONCLUDE、不入账不计入注入
    //    守恒面（注入量照计、捕获账不动——漏捕口径以入账守恒核）。
    let mut agg2 = ExpAgg::new();
    agg2.capture(SIG_RAGE, 100, CL_INPUT)?;
    if agg2.capture(SIG_DEAD, 200, 0).is_ok() {
        return Err(E_NO_CONCLUDE);
    }
    if agg2.rows.len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    if agg2.audit().is_ok() {
        return Err(E_CAPTURE_MISS); // 注入 2 捕获 1 = 漏捕口径红
    }
    hits += 1;

    // R4 信号型越界拒收：sig=2 → E_SIG_BAD。
    let mut agg3 = ExpAgg::new();
    if agg3.capture(2, 100, CL_CAND).is_ok() {
        return Err(E_SIG_BAD);
    }
    if agg3.rows.len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 结论面分布：R1 注入下四归因面各捕 2 条、缺位面 0 条
    //    （汇总账的分型可溯面）。
    let dist = agg.conclude_dist();
    if dist[CL_INPUT as usize] != 2
        || dist[CL_CAND as usize] != 2
        || dist[CL_RENDER as usize] != 2
        || dist[CL_BRIDGE as usize] != 2
        || dist[0] != 0
    {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 流水闭合：×10 捕获流水号 1..=10 逐条闭合、与注入序严格同序。
    let mut i = 0usize;
    while i < agg.rows.len() {
        if agg.rows[i].seq != (i + 1) as u64 {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    if agg.seq != 10 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 时序审计：×10 注入 at_ms 严格递增（137ms 步进）全过；
    //    乱序注入（时戳倒流）必报 E_LEDGER_BAD。
    agg.audit_timeline()?;
    let mut agg5 = ExpAgg::new();
    agg5.capture(SIG_RAGE, 500, CL_INPUT)?;
    if agg5.capture(SIG_DEAD, 400, CL_CAND).is_ok() {
        // 捕获允许但时序审计必须抓到倒流。
    } else {
        return Err(E_LEDGER_BAD);
    }
    if agg5.audit_timeline().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 最热归因面：R1 四面均分（各 2 条）→ 并列取码小者 = CL_INPUT；
    //    单面倾斜账 → 倾斜面胜出。
    let hot = agg.hottest_conclude()?;
    if hot != CL_INPUT {
        return Err(E_LEDGER_BAD);
    }
    let mut agg6 = ExpAgg::new();
    let mut i = 0u32;
    while i < 6 {
        agg6.capture(SIG_DEAD, 100 + i, CL_BRIDGE)?;
        i += 1;
    }
    if agg6.hottest_conclude()? != CL_BRIDGE {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R9 漏捕负例哨兵：人工抹一条明细 → 守恒核必报 E_CAPTURE_MISS
    //    （全捕获口径具备失守报警能力）。
    let mut agg4 = ExpAgg::new();
    let mut i = 0u32;
    while i < 10 {
        let sig = if i % 2 == 0 { SIG_RAGE } else { SIG_DEAD };
        agg4.capture(sig, 1000 + i, CL_INPUT)?;
        i += 1;
    }
    agg4.rows.remove(7); // 人为漏一条
    if agg4.audit().is_ok() {
        return Err(E_CAPTURE_MISS);
    }
    hits += 1;

    // R8 写回（判据 J2）：捕获未达注入量不写回、空摘要拒、全捕+非空摘要放行。
    if agg.writeback(true, 9, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if agg.writeback(true, 10, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    agg.writeback(true, 10, 0x28)?;
    hits += 1;

    Ok(hits)
}
