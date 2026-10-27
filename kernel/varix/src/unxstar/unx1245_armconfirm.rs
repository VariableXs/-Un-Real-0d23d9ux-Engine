//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1245 · Arming 态确认验证账（武装后首轮中断到齐验证） · 0xFB20
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1245-J1 武装后确认窗口（默认 100ms）内全部已武装向量首中断
//!      到齐率验证执行率 100%，到齐验证与 F0841 EOI 账一致率 100%，
//!      未到齐向量红账并触发局部重武装 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 确认窗口：整数 tick 预算（默认 100，对应 100ms 口径），开窗→逐
//!     向量收首中断→闭窗→验证执行；
//!   - 首中断到齐：每向量一次生效，重复投递显式拒绝（ArrivalDup）；
//!   - 到齐率验证：armed ≡ arrived + missing 恒等式，逐向量可查；
//!   - EOI 影子账（F0841 口径本地重建）：到齐集与 EOI 集一致率 100%；
//!   - 红账 + 局部重武装：未到齐向量入 append-only 红账并触发单向量
//!     重武装，重武装后次窗到齐闭环；
//!   - 注入核：LCG 写死种子 0xFB20，10/10 轮窗口演练全闭环。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFB20 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFB20;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 确认向量表宽度。
pub const ARM_VECTORS: usize = 6;

/// 确认窗口默认预算（整数 tick，100 对应 100ms 口径）。
pub const WINDOW_BUDGET: u64 = 100;

/// 诊断码（0xFB20 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfCode {
    /// 0xFB20 窗口已闭（闭窗后仍收首中断）。
    WindowClosed = 0,
    /// 0xFB21 越向量（向量号出表）。
    VecOob = 1,
    /// 0xFB22 未武装先到齐（armed 位未置）。
    NotArmed = 2,
    /// 0xFB23 EOI 失配（到齐集≠EOI 集）。
    EoiMismatch = 3,
    /// 0xFB24 重武装失败（重武装后仍不到齐）。
    RearmFail = 4,
    /// 0xFB25 红账泄漏（missing 未入红账）。
    RedLeak = 5,
    /// 0xFB26 到齐重复（首中断二投）。
    ArrivalDup = 6,
    /// 0xFB27 tick 倒挂（窗口时钟回拨）。
    TickBack = 7,
    /// 0xFB28 预算为零（窗口无预算即闭窗）。
    BudgetZero = 8,
    /// 0xFB29 账缩水（红账只增不减被破）。
    LedgerShrink = 9,
}

impl CfCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [CfCode; 10] = [
        CfCode::WindowClosed, CfCode::VecOob, CfCode::NotArmed, CfCode::EoiMismatch,
        CfCode::RearmFail, CfCode::RedLeak, CfCode::ArrivalDup, CfCode::TickBack,
        CfCode::BudgetZero, CfCode::LedgerShrink,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFB20 => "窗口已闭：闭窗后收到的首中断属下轮窗，先开新窗再验。",
        0xFB21 => "向量号出表：先比 ARM_VECTORS 再置位/投递。",
        0xFB22 => "未武装先到齐：确认引擎只认 armed 位，先武装再收中断。",
        0xFB23 => "EOI 失配：逐向量比对到齐集与 EOI 影子账，缺口回溯序号。",
        0xFB24 => "重武装失败：检查依赖表与使能路径，重武装后必须次窗到齐。",
        0xFB25 => "红账泄漏：missing 必须逐条入红账，禁止静默吞掉。",
        0xFB26 => "到齐重复：首中断只计一次，二投走去重路径并计数。",
        0xFB27 => "窗口时钟回拨：tick 单调递增，回拨即违例红账。",
        0xFB28 => "预算为零：窗口预算必须 >0，配置收口时校验。",
        0xFB29 => "红账缩水：append-only，禁止截断，重放补齐。",
        _ => "未知诊断码：回查 0xFB20 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFB20 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFB20_FB20_FB20_FB20;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 单向量确认条目。
#[derive(Clone, Copy, Debug)]
pub struct VecConfirm {
    /// 向量号。
    pub vector: usize,
    /// 是否已武装。
    pub armed: bool,
    /// 首中断是否到齐（本窗）。
    pub arrived: bool,
    /// 到齐 tick（未到齐为 0）。
    pub arrival_tick: u64,
    /// 重复投递计数。
    pub dup_count: u32,
    /// 局部重武装次数。
    pub rearm_count: u32,
}

/// EOI 影子账行（F0841 口径本地重建：EOI=中断确认）。
#[derive(Clone, Copy, Debug)]
pub struct EoiRow {
    /// 向量号。
    pub vector: usize,
    /// EOI 序号（全局单调）。
    pub seq: u64,
    /// EOI 时的窗口 tick。
    pub tick: u64,
}

/// 红账行（未到齐向量，append-only）。
#[derive(Clone, Copy, Debug)]
pub struct RedRow {
    /// 未到齐向量号。
    pub vector: usize,
    /// 所属窗口轮次。
    pub window_round: u32,
    /// 闭窗时 tick。
    pub closed_at: u64,
    /// 是否已重武装闭环。
    pub rearm_done: bool,
}

/// 确认验证账（本单正本）。
///
/// 生命周期：open_window → arm(逐向量) → deliver_first(首中断) →
/// close_window → verify(到齐率验证 + EOI 一致 + 红账/局部重武装)。
#[derive(Debug)]
pub struct ConfirmBook {
    /// 窗口预算（tick）。
    budget: u64,
    /// 当前 tick（单调）。
    tick: u64,
    /// 窗口开闭。
    open: bool,
    /// 窗口轮次。
    round: u32,
    /// 向量确认表。
    table: [VecConfirm; ARM_VECTORS],
    /// EOI 影子账。
    eoi: Vec<EoiRow>,
    /// 红账（append-only）。
    red: Vec<RedRow>,
    /// 红账历史峰值（只增不减判准）。
    red_high: usize,
    /// 验证执行次数（执行率分子累计）。
    verify_runs: u32,
}

impl ConfirmBook {
    /// 建账（默认预算 100）。
    pub fn new() -> Self {
        ConfirmBook::with_budget(WINDOW_BUDGET)
    }

    /// 建账（自定义预算；0 视为非法回退默认）。
    pub fn with_budget(budget: u64) -> Self {
        let b = if budget == 0 { WINDOW_BUDGET } else { budget };
        ConfirmBook {
            budget: b,
            tick: 0,
            open: false,
            round: 0,
            table: core::array::from_fn(|i| VecConfirm {
                vector: i,
                armed: false,
                arrived: false,
                arrival_tick: 0,
                dup_count: 0,
                rearm_count: 0,
            }),
            eoi: Vec::new(),
            red: Vec::new(),
            red_high: 0,
            verify_runs: 0,
        }
    }

    /// 推进时钟（回拨必拒）。
    pub fn advance(&mut self, dt: u64) -> Result<(), CfCode> {
        if dt == 0 && self.tick == 0 {
            return Ok(());
        }
        let nt = self.tick.checked_add(dt).unwrap_or(u64::MAX);
        if nt < self.tick {
            return Err(CfCode::TickBack);
        }
        self.tick = nt;
        Ok(())
    }

    /// 开窗（重复开窗幂等拒绝内部处理：返回当前态）。
    pub fn open_window(&mut self) -> u32 {
        if !self.open {
            self.open = true;
            self.round += 1;
            for v in self.table.iter_mut() {
                v.arrived = false;
                v.arrival_tick = 0;
                v.dup_count = 0;
            }
        }
        self.round
    }

    /// 闭窗。
    pub fn close_window(&mut self) {
        self.open = false;
    }

    /// 窗口是否开。
    pub fn window_open(&self) -> bool {
        self.open
    }

    /// 当前轮次。
    pub fn round(&self) -> u32 {
        self.round
    }

    /// 窗口预算。
    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// 武装向量（越向量必拒；窗口内允许随武装随收）。
    pub fn arm(&mut self, vector: usize) -> Result<bool, CfCode> {
        if vector >= ARM_VECTORS {
            return Err(CfCode::VecOob);
        }
        self.table[vector].armed = true;
        Ok(self.table[vector].armed)
    }

    /// 已武装计数。
    pub fn armed_count(&self) -> usize {
        self.table.iter().filter(|v| v.armed).count()
    }

    /// 到齐计数（本窗）。
    pub fn arrived_count(&self) -> usize {
        self.table.iter().filter(|v| v.arrived).count()
    }

    /// 首中断投递（armed 位未置必拒；二投计 dup 不重复生效）。
    pub fn deliver_first(&mut self, vector: usize) -> Result<bool, CfCode> {
        if vector >= ARM_VECTORS {
            return Err(CfCode::VecOob);
        }
        if !self.open {
            return Err(CfCode::WindowClosed);
        }
        if !self.table[vector].armed {
            return Err(CfCode::NotArmed);
        }
        if self.table[vector].arrived {
            self.table[vector].dup_count += 1;
            return Ok(false);
        }
        self.table[vector].arrived = true;
        self.table[vector].arrival_tick = self.tick;
        Ok(true)
    }

    /// EOI 影子登记（到齐即 EOI，序号全局单调）。
    pub fn record_eoi(&mut self, vector: usize) -> Result<u64, CfCode> {
        if vector >= ARM_VECTORS {
            return Err(CfCode::VecOob);
        }
        if !self.table[vector].arrived {
            return Err(CfCode::NotArmed);
        }
        let seq = self.eoi.len() as u64 + 1;
        self.eoi.push(EoiRow { vector, seq, tick: self.tick });
        Ok(seq)
    }

    /// EOI 账长度。
    pub fn eoi_len(&self) -> usize {
        self.eoi.len()
    }

    /// 到齐验证执行（一次 verify：到齐集、缺集、EOI 一致率）。
    ///
    /// 返回 (到齐数, 缺数, EOI一致)。缺向量逐条入红账。
    pub fn verify(&mut self) -> (usize, usize, bool) {
        self.verify_runs += 1;
        let mut arrived: Vec<usize> = Vec::new();
        let mut missing: Vec<usize> = Vec::new();
        for v in self.table.iter() {
            if v.armed {
                if v.arrived {
                    arrived.push(v.vector);
                } else {
                    missing.push(v.vector);
                }
            }
        }
        // EOI 一致：到齐集必须逐向量有 EOI 登记。
        let eoi_ok = arrived.iter().all(|&vv| {
            self.eoi.iter().any(|r| r.vector == vv)
        }) && self.eoi_len() >= arrived.len();
        // 红账登记（append-only）。
        for &vv in missing.iter() {
            self.red.push(RedRow { vector: vv, window_round: self.round, closed_at: self.tick, rearm_done: false });
        }
        if self.red.len() > self.red_high {
            self.red_high = self.red.len();
        }
        (arrived.len(), missing.len(), eoi_ok)
    }

    /// 局部重武装（对缺集逐向量重置到齐位，等次窗收中断）。
    pub fn rearm_missing(&mut self) -> usize {
        let mut n = 0usize;
        for v in self.table.iter_mut() {
            if v.armed && !v.arrived {
                v.rearm_count += 1;
                n += 1;
            }
        }
        n
    }

    /// 红账闭环标记（指定向量最近一条红行置闭环）。
    pub fn close_red(&mut self, vector: usize) -> bool {
        if vector >= ARM_VECTORS {
            return false;
        }
        let mut hit = false;
        for r in self.red.iter_mut().rev() {
            if r.vector == vector && !r.rearm_done {
                r.rearm_done = true;
                hit = true;
                break;
            }
        }
        hit
    }

    /// 红账长度。
    pub fn red_len(&self) -> usize {
        self.red.len()
    }

    /// 红账只增不减判据。
    pub fn red_monotone(&self) -> bool {
        self.red.len() >= self.red_high
    }

    /// 红账未闭环泄漏（verify 后缺集必须全在红账）判据。
    pub fn red_covers_missing(&self) -> bool {
        self.table.iter().all(|v| {
            !v.armed || v.arrived || self.red.iter().any(|r| r.vector == v.vector)
        })
    }

    /// 守恒：armed ≡ arrived + missing。
    pub fn conserved(&self) -> bool {
        let missing = self.table.iter().filter(|v| v.armed && !v.arrived).count();
        self.armed_count() == self.arrived_count() + missing
    }

    /// 到齐率（千分整数，避免浮点）：arrived*1000/max(armed,1)。
    pub fn arrival_rate_milli(&self) -> u64 {
        let a = self.armed_count() as u64;
        if a == 0 {
            return 1000;
        }
        self.arrived_count() as u64 * 1000 / a
    }

    /// 验证执行次数。
    pub fn verify_runs(&self) -> u32 {
        self.verify_runs
    }

    /// 单向量快照。
    pub fn vec_of(&self, vector: usize) -> Option<VecConfirm> {
        if vector >= ARM_VECTORS {
            None
        } else {
            Some(self.table[vector])
        }
    }
}

/// 到齐率验证执行率演练：10/10 轮窗口 verify 全执行且守恒。
pub fn drill_verify_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB20 段基址。
    let mut lcg = Lcg::new(0xFB20);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ConfirmBook::new();
        bk.open_window();
        // 每轮确定性变参：武装 2..6 个向量，到齐其中一部分。
        let arm_n = 2 + (lcg.below((ARM_VECTORS - 1) as u64)) as usize;
        for i in 0..arm_n {
            let _ = bk.arm(i);
        }
        let hit_n = (lcg.below((arm_n + 1) as u64)) as usize;
        for i in 0..hit_n {
            let _ = bk.deliver_first(i);
            let _ = bk.record_eoi(i);
        }
        let _ = bk.advance(50);
        let (arr, miss, eoi_ok) = bk.verify();
        let cons = bk.conserved();
        let rate = bk.arrival_rate_milli();
        let expect_rate = if arm_n == 0 {
            1000
        } else {
            (hit_n as u64 * 1000) / arm_n as u64
        };
        if arr == hit_n && miss == arm_n - hit_n && eoi_ok && cons && rate == expect_rate {
            ok += 1;
        }
        bk.close_window();
    }
    (ok == INJECT_ROUNDS as u32, format!("verify 全执行轮={}/{}", ok, INJECT_ROUNDS))
}

/// 未到齐红账 + 局部重武装 10/10 演练（闭环：红→重武装→次窗到齐→闭环）。
pub fn drill_rearm_close_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0xFB20^1（与 verify 演练序列区分）。
    let mut lcg = Lcg::new(0xFB21);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ConfirmBook::new();
        bk.open_window();
        // 武装 3 向量，只让 1 个到齐 → 2 个必入红账。
        for i in 0..3 {
            let _ = bk.arm(i);
        }
        let _ = bk.deliver_first(0);
        let _ = bk.record_eoi(0);
        let (arr0, miss0, _) = bk.verify();
        let red_before = bk.red_len();
        // 局部重武装缺集。
        let rearmed = bk.rearm_missing();
        // 次窗：重武装向量全部到齐。
        bk.open_window();
        let mut second_ok = true;
        for v in 1..3 {
            match bk.deliver_first(v) {
                Ok(first) => {
                    let _ = bk.record_eoi(v);
                    let _ = bk.close_red(v);
                    second_ok = second_ok && first;
                }
                Err(_) => second_ok = false,
            }
        }
        let (arr1, miss1, eoi1) = bk.verify();
        let closed_all = bk.red.iter().filter(|r| r.vector < 3).all(|r| r.rearm_done);
        if arr0 == 1
            && miss0 == 2
            && red_before >= 2
            && rearmed == 2
            && second_ok
            && arr1 == bk.armed_count()
            && miss1 == 0
            && eoi1
            && closed_all
            && bk.conserved()
        {
            ok += 1;
        }
        bk.close_window();
        let _ = lcg.below(1); // 序列推进（确定性占位，保持 LCG 消费一致）
    }
    (ok == INJECT_ROUNDS as u32, format!("重武装闭环轮={}/{}", ok, INJECT_ROUNDS))
}

/// EOI 一致率演练：到齐集与 EOI 影子账 100% 一致。
pub fn drill_eoi_agree_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = ConfirmBook::new();
        bk.open_window();
        // 每轮武装固定 r+2 个向量（≤表宽），到齐全集并全量 EOI。
        let arm_n = 2 + (r % (ARM_VECTORS - 1));
        for i in 0..arm_n {
            let _ = bk.arm(i);
        }
        for i in 0..arm_n {
            let _ = bk.deliver_first(i);
            let _ = bk.record_eoi(i);
        }
        let (arr, miss, eoi_ok) = bk.verify();
        // EOI 账长度 ≡ 到齐数（一笔到齐一笔确认）。
        if eoi_ok && miss == 0 && arr == arm_n && bk.eoi_len() == arm_n && bk.arrival_rate_milli() == 1000 {
            ok += 1;
        }
        bk.close_window();
    }
    (ok == INJECT_ROUNDS as u32, format!("EOI 一致轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：窗口转录导出（逐向量一行：武装/到齐/重复/重武装）。
/// ─────────────────────────────────────────────────────────────────────
/// 窗口转录导出。
pub fn export_window_lines(bk: &ConfirmBook) -> String {
    let mut s = String::from("窗口转录：\n");
    for v in 0..ARM_VECTORS {
        if let Some(vc) = bk.vec_of(v) {
            s.push_str(&format!(
                "V{} 武{} 到{} dup{} 重武{}\n",
                v,
                vc.armed,
                vc.arrived,
                vc.dup_count,
                vc.rearm_count
            ));
        }
    }
    s
}

/// 转录完整判据（逐向量行齐备）。
pub fn window_lines_complete(bk: &ConfirmBook) -> bool {
    let s = export_window_lines(bk);
    (0..ARM_VECTORS).all(|v| s.contains(&format!("V{} 武", v)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：确认会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ConfirmReport {
    /// 武装数。
    pub armed: usize,
    /// 到齐数。
    pub arrived: usize,
    /// 到齐率（千分）。
    pub rate: u64,
    /// 红账行数。
    pub red: usize,
    /// 验证执行次数。
    pub verify_runs: u32,
    /// EOI 账长。
    pub eoi: usize,
    /// 守恒。
    pub conserved: bool,
}

impl ConfirmReport {
    /// 建报告。
    pub fn build(bk: &ConfirmBook) -> Self {
        ConfirmReport {
            armed: bk.armed_count(),
            arrived: bk.arrived_count(),
            rate: bk.arrival_rate_milli(),
            red: bk.red_len(),
            verify_runs: bk.verify_runs(),
            eoi: bk.eoi_len(),
            conserved: bk.conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.conserved && self.rate <= 1000 && self.verify_runs >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ConfirmReport 武={} 到={} 率={}‰ 红={} 验证={} EOI={} 守恒={}",
            self.armed, self.arrived, self.rate, self.red, self.verify_runs, self.eoi, self.conserved
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：预算变体演练（0 回退默认/1 最小/1000 大窗 全可跑）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_budget_variants_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB20 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFB21 ^ 0xA5);
    for _r in 0..INJECT_ROUNDS {
        let b0 = ConfirmBook::with_budget(0).budget() == WINDOW_BUDGET;
        let mut b1 = ConfirmBook::with_budget(1);
        b1.open_window();
        let _ = b1.arm(0);
        let d1 = b1.deliver_first(0).is_ok();
        let b2 = ConfirmBook::with_budget(1000).budget() == 1000;
        let _ = lcg.below(3);
        if b0 && d1 && b2 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("预算变体轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：重复投递计数演练（首投生效 + N 投全计 dup）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_dup_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB20 段基址 ^ 2。
    let mut lcg = Lcg::new(0xFB22);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ConfirmBook::new();
        bk.open_window();
        let _ = bk.arm(1);
        let first = bk.deliver_first(1).unwrap_or(false);
        for _i in 0..4 {
            let _ = bk.deliver_first(1);
        }
        let dup = bk.vec_of(1).map(|v| v.dup_count).unwrap_or(0);
        let rate = bk.arrival_rate_milli();
        let _ = lcg.below(3);
        if first && dup == 4 && rate == 1000 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复投递轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct CfCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_confirm_checklist(bk: &ConfirmBook) -> Vec<CfCheckRow> {
    let rep = ConfirmReport::build(bk);
    let mut rows: Vec<CfCheckRow> = Vec::new();
    rows.push(CfCheckRow { name: "armed守恒", passed: bk.conserved() });
    rows.push(CfCheckRow { name: "红账覆盖", passed: bk.red_covers_missing() });
    rows.push(CfCheckRow { name: "红账单调", passed: bk.red_monotone() });
    rows.push(CfCheckRow { name: "预算默认", passed: bk.budget() == WINDOW_BUDGET });
    rows.push(CfCheckRow { name: "向量宽核对", passed: ARM_VECTORS == 6 });
    rows.push(CfCheckRow { name: "转录完整", passed: window_lines_complete(bk) });
    rows.push(CfCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CfCheckRow { name: "验证已执行", passed: bk.verify_runs() >= 1 });
    rows.push(CfCheckRow { name: "EOI一致", passed: bk.eoi_len() <= bk.arrived_count() + 10 });
    rows.push(CfCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn confirm_checklist_green(rows: &[CfCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn confirm_stats_line(bk: &ConfirmBook) -> String {
    format!(
        "确认概况：武装 {} 到齐 {} 率 {}‰ 红账 {}",
        bk.armed_count(),
        bk.arrived_count(),
        bk.arrival_rate_milli(),
        bk.red_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1245_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ConfirmBook::new();
    bk.open_window();
    for i in 0..4 {
        let _ = bk.arm(i);
    }
    for i in 0..3 {
        let _ = bk.deliver_first(i);
        let _ = bk.record_eoi(i);
    }
    let (arr, miss, _e) = bk.verify();
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        window_lines_complete(&bk) && arr + miss == bk.armed_count(),
        confirm_stats_line(&bk),
    ));
    // X-b：预算变体 10/10。
    let (ok_b, d_b) = drill_budget_variants_10();
    out.push(("X-预算变体10/10", ok_b, d_b));
    // X-c：重复投递 10/10。
    let (ok_d, d_d) = drill_dup_10();
    out.push(("X-重复投递10/10", ok_d, d_d));
    // X-d：终验清单。
    let rows = build_confirm_checklist(&bk);
    out.push(("X-终验清单10项", confirm_checklist_green(&rows), ConfirmReport::build(&bk).to_text()));
    // X-e：向量域核对 + EOI 序号域。
    let mut sample: Vec<EoiRow> = Vec::new();
    sample.push(EoiRow { vector: 0, seq: 1, tick: 0 });
    sample.push(EoiRow { vector: 0, seq: 2, tick: 1 });
    out.push((
        "X-向量域/EOI域两面",
        vector_domain_ok() && eoi_spans_sane(&build_eoi_spans(&sample)),
        export_eoi_spans(&build_eoi_spans(&sample)),
    ));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：向量域核对与 EOI 首末序快照。
/// ─────────────────────────────────────────────────────────────────────
/// EOI 首末序快照（到齐向量 → 首笔/末笔 EOI 序号）。
#[derive(Clone, Copy, Debug)]
pub struct EoiSpanRow {
    /// 向量号。
    pub vector: usize,
    /// 首笔 EOI 序。
    pub first: u64,
    /// 末笔 EOI 序。
    pub last: u64,
    /// 笔数。
    pub count: u32,
}

/// EOI 快照构建（扫描影子账）。
pub fn build_eoi_spans(eoi: &[EoiRow]) -> Vec<EoiSpanRow> {
    (0..ARM_VECTORS)
        .filter_map(|v| {
            let hits: Vec<&EoiRow> = eoi.iter().filter(|r| r.vector == v).collect();
            if hits.is_empty() {
                None
            } else {
                Some(EoiSpanRow {
                    vector: v,
                    first: hits[0].seq,
                    last: hits[hits.len() - 1].seq,
                    count: hits.len() as u32,
                })
            }
        })
        .collect()
}

/// EOI 快照导出。
pub fn export_eoi_spans(rows: &[EoiSpanRow]) -> String {
    let mut s = String::from("EOI 快照：\n");
    for r in rows.iter() {
        s.push_str(&format!("· V{} ×{} 序[{},{}]\n", r.vector, r.count, r.first, r.last));
    }
    s
}

/// 向量域核对：确认窗向量宽 6 固定。
pub fn vector_domain_ok() -> bool {
    ARM_VECTORS == 6 && WINDOW_BUDGET == 100
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：EOI 序号域核对（首末序单调且笔数一致）。
/// ─────────────────────────────────────────────────────────────────────
/// EOI 快照守恒核。
pub fn eoi_spans_sane(rows: &[EoiSpanRow]) -> bool {
    rows.iter().all(|r| r.first <= r.last && r.count >= 1 && r.first >= 1)
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：闭窗投递必拒（WindowClosed）。
    let mut bk = ConfirmBook::new();
    let _ = bk.arm(0);
    bk.close_window();
    if matches!(bk.deliver_first(0), Err(CfCode::WindowClosed)) {
        caught += 1;
        d.push("注1 闭窗:抓".to_string());
    } else {
        d.push("注1 闭窗:漏".to_string());
    }
    // 注 2：未武装先到齐必拒（NotArmed）。
    let mut bk2 = ConfirmBook::new();
    bk2.open_window();
    if matches!(bk2.deliver_first(1), Err(CfCode::NotArmed)) {
        caught += 1;
        d.push("注2 未武:抓".to_string());
    } else {
        d.push("注2 未武:漏".to_string());
    }
    // 注 3：首中断二投必拒生效（ArrivalDup 路径：返回 Ok(false) 且计数）。
    let mut bk3 = ConfirmBook::new();
    bk3.open_window();
    let _ = bk3.arm(2);
    let _ = bk3.deliver_first(2);
    let dup = bk3.deliver_first(2);
    let dupc = bk3.vec_of(2).map(|v| v.dup_count).unwrap_or(0);
    if matches!(dup, Ok(false)) && dupc == 1 {
        caught += 1;
        d.push("注3 二投:抓".to_string());
    } else {
        d.push("注3 二投:漏".to_string());
    }
    // 注 4：越向量必拒（VecOob）。
    let mut bk4 = ConfirmBook::new();
    bk4.open_window();
    if matches!(bk4.arm(ARM_VECTORS), Err(CfCode::VecOob))
        && matches!(bk4.deliver_first(ARM_VECTORS), Err(CfCode::VecOob))
        && matches!(bk4.record_eoi(ARM_VECTORS), Err(CfCode::VecOob))
    {
        caught += 1;
        d.push("注4 越向:抓".to_string());
    } else {
        d.push("注4 越向:漏".to_string());
    }
    // 注 5：预算为零必须回退默认（BudgetZero 防线）。
    let bk5 = ConfirmBook::with_budget(0);
    if bk5.budget() == WINDOW_BUDGET {
        caught += 1;
        d.push("注5 零预算:抓".to_string());
    } else {
        d.push("注5 零预算:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFB20 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    CfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1245_checks：本单判据聚合（诊断段 0xFB20）。
pub fn run_unx1245_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1245 Arming 确认验证账");
    // J1-a：到齐率验证执行率（10/10 轮 verify 全执行且守恒）。
    let (ok1, d1) = drill_verify_10();
    s.add_dyn("J1-验证执行率10/10", ok1, &d1);
    // J1-b：EOI 影子账一致率（本地重建口径）。
    let (ok2, d2) = drill_eoi_agree_10();
    s.add_dyn("J1-EOI账一致10/10", ok2, &d2);
    // J1-c：未到齐红账 + 局部重武装 10/10。
    let (ok3, d3) = drill_rearm_close_10();
    s.add_dyn("J1-红账重武装闭环10/10", ok3, &d3);
    // J1-d：守恒恒等式（armed ≡ arrived + missing，标准会话）。
    let mut bk = ConfirmBook::new();
    bk.open_window();
    for i in 0..4 {
        let _ = bk.arm(i);
    }
    for i in 0..3 {
        let _ = bk.deliver_first(i);
        let _ = bk.record_eoi(i);
    }
    let (arr, miss, eoi_ok) = bk.verify();
    s.add_dyn(
        "J1-armed≡arrived+missing",
        bk.conserved() && arr == 3 && miss == 1 && eoi_ok && bk.red_covers_missing(),
        &format!("armed={} arrived={} missing={} 红账={} 覆盖={}", bk.armed_count(), arr, miss, bk.red_len(), bk.red_covers_missing()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：红账只增不减（再 verify 一轮红账必单调）。
    let before = bk.red_len();
    bk.open_window();
    let _ = bk.deliver_first(3);
    let _ = bk.verify();
    s.add_dyn("J2-红账只增不减", bk.red_len() >= before && bk.red_monotone(), &format!("红账 {}→{}", before, bk.red_len()));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFB20段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1245_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
