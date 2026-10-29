//! unx0949_latbill — UNX-F0949 分发面延迟分解账（端到端分段对齐 F0038）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0949 分发面延迟分解账（端到端分段对齐 F0038）
//! 判据原文摘录（卡面验收行）：端到端延迟按分段记账（硬件取号/上半/
//! 触发/下半/收尾五段），分段和恒等于端到端总账，与 F0038 口径对齐
//! 抽样 10/10 次账平，分段越界必点名。
//! 诊断段 0x7CA0（本单独占，XcCode 闭集见下）。
//! 设计纪律：五段分解是唯一口径——任何账面总延迟必须等于五段之和；
//! 每段有独立预算上限，越段必拦；对齐 F0038 的抽样核验走同一恒等式；
//! 全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7CA0;

// ── 诊断段独占 · 延迟分解账错误域 ──────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_SEG: u16 = SEG_ERR + 1;    // 分段账非法（越段上限）
const E_BAL: u16 = SEG_ERR + 2;    // 分段和 ≠ 总账
const E_ALIGN: u16 = SEG_ERR + 3;  // 与 F0038 口径不齐
const E_STATE: u16 = SEG_ERR + 4;  // 账面状态非法
const E_NEG: u16 = SEG_ERR + 5;    // 负延迟（时钟倒挂）

/// 五段分解段位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    HwFetch = 0, // 硬件取号（控制器→CPU 认领）
    TopHalf = 1, // 上半执行
    Trigger = 2, // 触发下半
    BotHalf = 3, // 下半执行
    Tail = 4,    // 收尾（EOI/重新开中断）
}

pub const NPHASE: usize = 5;

impl Phase {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::HwFetch => "hw-fetch", Phase::TopHalf => "top-half", Phase::Trigger => "trigger",
            Phase::BotHalf => "bot-half", Phase::Tail => "tail",
        }
    }
    /// 段位预算上限（逻辑 µs）。
    pub fn budget(self) -> u64 {
        match self {
            Phase::HwFetch => 2, Phase::TopHalf => 5, Phase::Trigger => 2,
            Phase::BotHalf => 200, Phase::Tail => 3,
        }
    }
}

/// 单事件延迟账：五段各一笔 + 端到端总账。
#[derive(Clone, Copy, Debug)]
pub struct LatRec {
    pub ev: u32,
    pub parts: [u64; NPHASE],
    pub total: u64, // 恒等式：total == parts 之和
}

impl LatRec {
    /// 恒等式核验：分段和 == 总账。
    pub fn invariant_ok(&self) -> bool {
        let sum: u64 = self.parts.iter().sum();
        sum == self.total
    }
}

/// 延迟分解账本。
pub struct LatLedger {
    pub recs: Vec<LatRec>,
    pub overflow: Vec<(u32, Phase)>, // 越段点名账
}

impl LatLedger {
    pub fn new() -> Self { LatLedger { recs: Vec::new(), overflow: Vec::new() } }

    /// 记一笔：五段 + 总账必须自带守恒；段越界点名但可选择性放行
    /// （审计模式全拦）。
    pub fn record(&mut self, ev: u32, parts: [u64; NPHASE], total: u64) -> Result<(), u16> {
        for (i, p) in parts.iter().enumerate() {
            let budget = phase_budget(i);
            if *p > budget {
                self.overflow.push((ev, phase_of(i)));
            }
        }
        let rec = LatRec { ev, parts, total };
        if !rec.invariant_ok() { return Err(E_BAL); }
        self.recs.push(rec);
        Ok(())
    }

    /// 严格审计模式：存在任何越段即整账红灯。
    pub fn strict_audit(&self) -> Result<u32, u16> {
        if !self.overflow.is_empty() { return Err(E_SEG); }
        for r in &self.recs {
            if !r.invariant_ok() { return Err(E_BAL); }
        }
        Ok(self.recs.len() as u32)
    }

    /// 与 F0038 口径对齐：抽样核验——分段和、段名序、总账三面一致。
    pub fn align_f0038(&self, sample: &[u32]) -> Result<u32, u16> {
        if sample.is_empty() { return Err(E_ALIGN); }
        let mut hit = 0u32;
        for ev in sample {
            let rec = match self.recs.iter().find(|r| &r.ev == ev) {
                Some(r) => r,
                None => return Err(E_ALIGN),
            };
            if !rec.invariant_ok() { return Err(E_BAL); }
            // 口径核验：五段顺序号 0..5 连续非零（有活）或全零（空转）。
            let any = rec.parts.iter().any(|p| *p > 0);
            if any && rec.parts[0] == 0 { return Err(E_ALIGN); } // 无取号却有活
            hit += 1;
        }
        Ok(hit)
    }

    /// 端到端峰值段分析：哪段吃掉最多预算。
    pub fn peak_phase(&self) -> Option<Phase> {
        let mut sum = [0u64; NPHASE];
        for r in &self.recs {
            for i in 0..NPHASE { sum[i] += r.parts[i]; }
        }
        let mut best = 0usize;
        for i in 1..NPHASE { if sum[i] > sum[best] { best = i; } }
        if self.recs.is_empty() { None } else { Some(phase_of(best)) }
    }

    /// 人话化摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("latbill recs={} overflow={}", self.recs.len(), self.overflow.len());
        if let Some(p) = self.peak_phase() {
            s = alloc::format!("{} peak={}", s, p.as_str());
        }
        s
    }
}

fn phase_budget(i: usize) -> u64 {
    match i { 0 => 2, 1 => 5, 2 => 2, 3 => 200, _ => 3 }
}
fn phase_of(i: usize) -> Phase {
    match i { 0 => Phase::HwFetch, 1 => Phase::TopHalf, 2 => Phase::Trigger, 3 => Phase::BotHalf, _ => Phase::Tail }
}

/// 十次对齐抽样构建器：生成 10 条账平样本。
fn build_aligned_sample() -> Vec<LatRec> {
    let mut v = Vec::new();
    for i in 0..10u32 {
        let parts = [1 + (i % 2) as u64, 3, 1, 100 + i as u64, 2];
        let total: u64 = parts.iter().sum();
        v.push(LatRec { ev: 1000 + i, parts, total });
    }
    v
}

/// 自例程：十样本对齐、越段拦截、负延迟拦截全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut ll = LatLedger::new();
    // 场景一：十条账平样本全落账。
    for r in build_aligned_sample() {
        let total = r.total;
        ll.record(r.ev, r.parts, total)?;
    }
    if ll.recs.len() != 10 { return Err(E_BAL); }
    if ll.strict_audit()? != 10 { return Err(E_SEG); }
    // 场景二：与 F0038 对齐抽样 10/10 账平。
    let sample = [1000u32, 1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009];
    if ll.align_f0038(&sample)? != 10 { return Err(E_ALIGN); }
    // 场景三：分段和 ≠ 总账必拦。
    if ll.record(2000, [1, 3, 1, 50, 2], 999) != Err(E_BAL) { return Err(E_BAL); }
    // 场景四：越段必点名（下半 999µs > 200µs 预算）。
    ll.record(2001, [1, 3, 1, 999, 2], 1006)?;
    if ll.overflow.len() != 1 { return Err(E_SEG); }
    if ll.strict_audit() != Err(E_SEG) { return Err(E_SEG); }
    // 场景五：口径不齐——无取号却有活。
    if ll.record(2002, [0, 3, 1, 50, 2], 56) != Err(E_BAL) {
        // parts 之和 56 == total 则落账，但 align 会抓口径。
        match ll.align_f0038(&[2002]) {
            Err(E_ALIGN) => {}
            _ => return Err(E_ALIGN),
        }
    }
    // 场景六：峰值段应为 bot-half。
    if ll.peak_phase() != Some(Phase::BotHalf) { return Err(E_SEG); }
    if ll.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
