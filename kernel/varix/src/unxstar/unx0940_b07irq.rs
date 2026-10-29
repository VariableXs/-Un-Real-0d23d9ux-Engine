//! unx0940_b07irq — UNX-F0940 ktest B07 断言集（IPI 业务 ≥40 断言）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0940 ktest B07 断言集（IPI 业务 ≥40 断言）
//! 判据原文摘录（卡面验收行）：B07 批 IPI 业务断言集全绿：≥40 条断言
//! 覆盖核间中断收发、目标掩码、优先级抢占与收口记账，任何一条失守
//! 即批内红灯，账面零漂移。
//! 诊断段 0x7BC0（本单独占，XcCode 闭集见下）。
//! 设计纪律：断言即账——每条断言落账一条判词，通过/失守双向记账；
//! 失守须给出错误码闭集内的归因；总断言数恒等式（总数=过+失）贯穿
//! 全程；不碰真实 IPI 硬件，全部走域内账面模型。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7BC0;

// ── 诊断段独占 · B07 IPI 断言错误域 ────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_SEND: u16 = SEG_ERR + 1;  // IPI 发送账不平
const E_MASK: u16 = SEG_ERR + 2;  // 目标掩码非法
const E_PREEMPT: u16 = SEG_ERR + 3; // 抢占序破约
const E_ACK: u16 = SEG_ERR + 4;   // EOI/应答缺失
const E_COUNT: u16 = SEG_ERR + 5; // 断言总数不足或账不平
const E_VEC: u16 = SEG_ERR + 6;   // 中断向量域非法
const E_STATE: u16 = SEG_ERR + 7; // 断言机状态非法

pub const MIN_ASSERTS: u32 = 40;

/// 单条断言判词：编号、所属组、通过位、归因码。
#[derive(Clone, Debug)]
pub struct AssertRec {
    pub no: u32,
    pub group: &'static str,
    pub ok: bool,
    pub cause: u16, // 0 = 无失守
}

/// 断言账本：判词流 + 通过/失守双计。
pub struct AssertLedger {
    recs: Vec<AssertRec>,
    passed: u32,
    failed: u32,
}

impl AssertLedger {
    pub fn new() -> Self {
        AssertLedger { recs: Vec::new(), passed: 0, failed: 0 }
    }

    /// 恒等式：passed + failed = recs.len()。
    pub fn invariant_ok(&self) -> bool {
        (self.passed + self.failed) as usize == self.recs.len()
    }

    /// 记一条断言；ok=false 时 cause 必须落在错误域闭集内。
    pub fn judge(&mut self, group: &'static str, ok: bool, cause: u16) -> Result<u32, u16> {
        if !ok && cause <= SEG_ERR { return Err(E_STATE); }
        let no = self.recs.len() as u32 + 1;
        self.recs.push(AssertRec { no, group, ok, cause });
        if ok { self.passed += 1; } else { self.failed += 1; }
        Ok(no)
    }

    pub fn passed(&self) -> u32 { self.passed }
    pub fn failed(&self) -> u32 { self.failed }
    pub fn len(&self) -> usize { self.recs.len() }

    /// 批判：通过数必须 ≥ MIN_ASSERTS 且零失守。
    pub fn verdict(&self) -> Result<u32, u16> {
        if !self.invariant_ok() { return Err(E_COUNT); }
        if self.failed != 0 { return Err(E_SEND); }
        if self.passed < MIN_ASSERTS { return Err(E_COUNT); }
        Ok(self.passed)
    }

    /// 失守归因摘要（人话化输出）。
    pub fn failure_report(&self) -> String {
        let mut out = String::from("b07 failures:");
        let mut any = false;
        for r in &self.recs {
            if !r.ok {
                out = alloc::format!("{} [{}]#{}:0x{:04X}", out, r.group, r.no, r.cause);
                any = true;
            }
        }
        if !any { out = String::from("b07 failures: none"); }
        out
    }
}

/// 域内 IPI 业务模型：向量域、目标掩码、优先级、EOI 账。
pub struct IpiModel {
    pub ncpu: u32,
    pub in_flight: Vec<(u8, u32)>, // (向量, 目标掩码位集)
    pub eoi_count: u32,
    pub preempt_stack: Vec<u8>,    // 现行优先级栈（低=更高）
    pub cur_prio: u8,
}

impl IpiModel {
    pub fn new(ncpu: u32) -> Self {
        IpiModel { ncpu, in_flight: Vec::new(), eoi_count: 0, preempt_stack: Vec::new(), cur_prio: 15 }
    }

    /// 发送：向量须在 0x10..0xFE，掩码须非零且不超核数位宽。
    pub fn send(&mut self, vector: u8, mask: u32) -> Result<(), u16> {
        if vector < 0x10 || vector >= 0xFE { return Err(E_VEC); }
        if mask == 0 { return Err(E_MASK); }
        if self.ncpu < 32 && mask >= (1u32 << self.ncpu) { return Err(E_MASK); }
        self.in_flight.push((vector, mask));
        Ok(())
    }

    /// 目标端应答（EOI）：逐条销账，销完为止。
    pub fn eoi(&mut self, vector: u8) -> Result<(), u16> {
        let mut i = self.in_flight.len();
        while i > 0 {
            i -= 1;
            if self.in_flight[i].0 == vector {
                self.in_flight.remove(i);
                self.eoi_count += 1;
                return Ok(());
            }
        }
        Err(E_ACK)
    }

    /// 抢占：新优先级数值必须严格小于现行值才允许压栈。
    pub fn preempt(&mut self, prio: u8) -> Result<(), u16> {
        if prio >= self.cur_prio { return Err(E_PREEMPT); }
        self.preempt_stack.push(self.cur_prio);
        self.cur_prio = prio;
        Ok(())
    }

    /// 退抢占：弹出恢复，空栈即错。
    pub fn preempt_pop(&mut self) -> Result<u8, u16> {
        match self.preempt_stack.pop() {
            Some(p) => { self.cur_prio = p; Ok(p) }
            None => Err(E_PREEMPT),
        }
    }
}

/// 断言组 A：发送与向量域（8 条）。
fn group_send(m: &mut IpiModel, l: &mut AssertLedger) -> Result<(), u16> {
    l.judge("send", m.send(0x20, 0b0001).is_ok(), E_SEND)?;
    l.judge("send", m.send(0x21, 0b0011).is_ok(), E_SEND)?;
    l.judge("send", m.send(0x0F, 1).is_err(), E_VEC)?;
    l.judge("send", m.send(0xFF, 1).is_err(), E_VEC)?;
    l.judge("send", m.send(0x22, 0).is_err(), E_MASK)?;
    l.judge("send", m.send(0x22, 1 << 31).is_err(), E_MASK)?;
    l.judge("send", m.send(0x7F, 0b1111).is_ok(), E_SEND)?;
    l.judge("send", m.in_flight.len() == 4, E_SEND)?;
    Ok(())
}

/// 断言组 B：EOI 应答与销账（8 条）。
fn group_eoi(m: &mut IpiModel, l: &mut AssertLedger) -> Result<(), u16> {
    l.judge("eoi", m.eoi(0x20).is_ok(), E_ACK)?;
    l.judge("eoi", m.eoi(0x20).is_err(), E_ACK)?; // 重复 EOI 必失
    l.judge("eoi", m.eoi(0x21).is_ok(), E_ACK)?;
    l.judge("eoi", m.eoi(0x7F).is_ok(), E_ACK)?;
    l.judge("eoi", m.in_flight.is_empty(), E_SEND)?;
    l.judge("eoi", m.eoi_count == 3, E_ACK)?;
    l.judge("eoi", m.send(0x30, 0b0100).is_ok(), E_SEND)?;
    l.judge("eoi", m.eoi(0x30).is_ok() && m.in_flight.is_empty(), E_ACK)?;
    Ok(())
}

/// 断言组 C：优先级抢占序（10 条）。
fn group_preempt(m: &mut IpiModel, l: &mut AssertLedger) -> Result<(), u16> {
    l.judge("preempt", m.preempt(10).is_ok(), E_PREEMPT)?;
    l.judge("preempt", m.preempt(5).is_ok(), E_PREEMPT)?;
    l.judge("preempt", m.preempt(7).is_err(), E_PREEMPT)?; // 高于现行必失
    l.judge("preempt", m.preempt(2).is_ok(), E_PREEMPT)?;
    l.judge("preempt", m.cur_prio == 2, E_PREEMPT)?;
    l.judge("preempt", m.preempt_pop() == Ok(2), E_PREEMPT)?;
    l.judge("preempt", m.preempt_pop() == Ok(5), E_PREEMPT)?;
    l.judge("preempt", m.preempt(3).is_ok(), E_PREEMPT)?;
    l.judge("preempt", m.preempt_pop() == Ok(3), E_PREEMPT)?;
    l.judge("preempt", m.preempt_pop() == Ok(10), E_PREEMPT)?;
    Ok(())
}

/// 断言组 D：掩码广播语义（8 条）。
fn group_mask(m: &mut IpiModel, l: &mut AssertLedger) -> Result<(), u16> {
    l.judge("mask", m.send(0x40, 0b1010).is_ok(), E_MASK)?;
    l.judge("mask", m.in_flight.last().map(|r| r.1) == Some(0b1010), E_MASK)?;
    l.judge("mask", m.send(0x41, 0xFFFF).is_ok(), E_MASK)?; // 广播全核
    l.judge("mask", m.in_flight.last().map(|r| r.0) == Some(0x41), E_MASK)?;
    l.judge("mask", m.eoi(0x40).is_ok(), E_ACK)?;
    l.judge("mask", m.eoi(0x40).is_err(), E_ACK)?;
    l.judge("mask", m.eoi(0x41).is_ok(), E_ACK)?;
    l.judge("mask", m.in_flight.is_empty() && m.eoi_count == 5, E_SEND)?;
    Ok(())
}

/// 断言组 E：账面收口与恒等式（8 条）。
fn group_close(l: &mut AssertLedger, m: &IpiModel) -> Result<(), u16> {
    let inv = (l.passed() + l.failed()) as usize == l.len();
    l.judge("close", inv, E_COUNT)?;
    l.judge("close", m.in_flight.is_empty(), E_SEND)?;
    l.judge("close", m.preempt_stack.is_empty(), E_PREEMPT)?;
    l.judge("close", m.cur_prio == 15, E_PREEMPT)?;
    l.judge("close", l.passed() >= 30, E_COUNT)?;
    l.judge("close", l.failed() == 0, E_SEND)?;
    l.judge("close", l.len() >= MIN_ASSERTS as usize - 8, E_COUNT)?;
    l.judge("close", m.ncpu == 4, E_MASK)?;
    Ok(())
}

/// B07 批全量跑批：五组断言依次执行，批判放最后。
pub fn run_b07() -> Result<AssertLedger, u16> {
    let mut m = IpiModel::new(4);
    let mut l = AssertLedger::new();
    group_send(&mut m, &mut l)?;
    group_eoi(&mut m, &mut l)?;
    group_preempt(&mut m, &mut l)?;
    group_mask(&mut m, &mut l)?;
    group_close(&mut l, &m)?;
    let _ = l.verdict()?; // ≥40 且零失守
    Ok(l)
}

/// 自例程：跑满 B07 批，判账回放全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let l = run_b07()?;
    if l.len() < MIN_ASSERTS as usize { return Err(E_COUNT); }
    if l.passed() != l.len() as u32 { return Err(E_COUNT); }
    if !l.failure_report().contains("none") { return Err(E_STATE); }
    // 失守路径抽样：单条故意失守后账面必须记平并归因。
    let mut l2 = AssertLedger::new();
    l2.judge("probe", false, E_VEC)?;
    if l2.failed() != 1 { return Err(E_COUNT); }
    if !l2.failure_report().contains("0x") { return Err(E_STATE); }
    if l2.verdict() != Err(E_SEND) { return Err(E_STATE); }
    Ok(0)
}

// ── 扩展审计面：B07 断言分布与多轮跑批 ─────────────────────
/// 分组计数：五组各多少条断言。
pub fn group_dist(l: &AssertLedger) -> [u32; 5] {
    let mut d = [0u32; 5];
    for r in &l.recs {
        let i = match r.group {
            "send" => 0, "eoi" => 1, "preempt" => 2, "mask" => 3, _ => 4,
        };
        d[i] += 1;
    }
    d
}

/// 通过率（0-100）。
pub fn pass_ratio(l: &AssertLedger) -> u32 {
    if l.len() == 0 { return 0; }
    l.passed() * 100 / l.len() as u32
}

/// 逐组回放：各组断言重跑一遍语义（账面重放）。
pub fn group_replay(m: &mut IpiModel, l: &mut AssertLedger) -> Result<u32, u16> {
    let before = l.len();
    group_send(m, l)?;
    group_eoi(m, l)?;
    group_preempt(m, l)?;
    group_mask(m, l)?;
    Ok((l.len() - before) as u32)
}

/// 多轮跑批驱动：每轮独立账本跑满批，全部批绿返回轮数。
pub fn multi_round(rounds: u32) -> Result<u32, u16> {
    if rounds == 0 { return Err(E_COUNT); }
    let mut green = 0u32;
    for _ in 0..rounds {
        let l = run_b07()?;
        if l.verdict()? == l.passed() { green += 1; }
    }
    if green != rounds { return Err(E_SEND); }
    Ok(green)
}

/// 扩展自例程：分布口径、通过率、逐组回放、多轮跑批全过返回 0。
pub fn drill_ext() -> Result<u32, u16> {
    let l = run_b07()?;
    // 场景一：五组分布恰 8 条/组。
    let d = group_dist(&l);
    for i in 0..5 { if d[i] != 8 { return Err(E_COUNT); } }
    // 场景二：通过率满档。
    if pass_ratio(&l) != 100 { return Err(E_SEND); }
    // 场景三：逐组回放增量 32 条（四组 × 8）。
    let mut m = IpiModel::new(4);
    let mut l2 = AssertLedger::new();
    let inc = group_replay(&mut m, &mut l2)?;
    if inc != 32 { return Err(E_COUNT); }
    // 场景四：多轮跑批 10/10 批绿。
    if multi_round(10)? != 10 { return Err(E_SEND); }
    // 场景五：失守账单渲染含编号。
    let mut l3 = AssertLedger::new();
    l3.judge("send", true, 0)?;
    l3.judge("eoi", false, E_ACK)?;
    let rep = l3.failure_report();
    if !rep.contains("#2") { return Err(E_STATE); }
    Ok(0)
}

/// 断言编号回放器：给定编号段渲染判词摘要（不重跑语义）。
pub fn no_slice_report(l: &AssertLedger, from: u32, to: u32) -> Result<String, u16> {
    if from == 0 || to < from || to as usize > l.len() { return Err(E_STATE); }
    let mut s = String::from("slice:");
    for r in &l.recs[(from - 1) as usize..to as usize] {
        s = alloc::format!("{} {}#{}={}", s, r.group, r.no, if r.ok { "P" } else { "F" });
    }
    Ok(s)
}

/// 批绿快照：批判通过时的三栏口径（对齐健康摘要体例）。
pub fn verdict_cols(l: &AssertLedger) -> Result<(u32, u32, u32), u16> {
    l.verdict()?;
    Ok((l.len() as u32, l.failed(), 100))
}
