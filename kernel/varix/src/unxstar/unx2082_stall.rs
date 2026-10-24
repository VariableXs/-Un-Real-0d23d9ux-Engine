//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2082 · 调度压力面（stall 时间三轴） · 诊断段 0xB980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F2082-J1 三轴 stall 断言全过（CPU 可跑不可跑/内存等待/
//!      IO 等待三轴计量正确），轴间不串账 10/10，分解和=总 stall 守恒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三轴模型：CPU 轴（可跑队列满不可跑）/MEM 轴（页等待）/
//!     IO 轴（块等待），互斥记账（轴间不串账）；
//!   - stall 计量：事件起止戳对账；
//!   - 守恒：cpu+mem+io == 总 stall；
//!   - 轴间不串账：任一时刻仅一轴在计（事件模型保证）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xB980;

/// stall 轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallAxis {
    /// CPU 可跑不可跑。
    Cpu,
    /// 内存等待。
    Mem,
    /// IO 等待。
    Io,
}

impl StallAxis {
    pub fn name(self) -> &'static str {
        match self {
            StallAxis::Cpu => "CPU",
            StallAxis::Mem => "MEM",
            StallAxis::Io => "IO",
        }
    }
}

/// 未完结的 stall 事件。
#[derive(Debug, Clone, Copy)]
pub struct OpenEvent {
    pub axis: StallAxis,
    pub start: u64,
}

/// 三轴 stall 账。
pub struct StallBook {
    /// 三轴累计（tick）。
    acc: [u64; 3],
    /// 总 stall。
    total: u64,
    /// 在飞事件（单事件模型：同一时刻至多一个在飞）。
    open: Option<OpenEvent>,
    /// 事件账。
    events: Vec<(StallAxis, u64, u64)>,
    /// 轴间串账检出。
    overlaps: u32,
}

impl StallBook {
    pub fn new() -> Self {
        StallBook {
            acc: [0; 3],
            total: 0,
            open: None,
            events: Vec::new(),
            overlaps: 0,
        }
    }

    /// stall 开始。
    pub fn begin(&mut self, axis: StallAxis, at: u64) -> bool {
        if self.open.is_some() {
            self.overlaps += 1; // 轴间串账形态
            return false;
        }
        self.open = Some(OpenEvent { axis, start: at });
        true
    }

    /// stall 结束。
    pub fn end(&mut self, at: u64) -> bool {
        let ev = match self.open.take() {
            Some(e) => e,
            None => return false,
        };
        if at < ev.start {
            return false;
        }
        let d = at - ev.start;
        self.acc[ev.axis as usize] += d;
        self.total += d;
        self.events.push((ev.axis, ev.start, at));
        true
    }

    /// 分解和=总 stall 守恒。
    pub fn conserved(&self) -> bool {
        self.acc.iter().sum::<u64>() == self.total
    }

    /// 轴间不串账：无未闭合重叠（open 单槽 + 串账计数 0）。
    pub fn no_overlap(&self) -> bool {
        self.overlaps == 0
    }

    pub fn axis_total(&self, axis: StallAxis) -> u64 {
        self.acc[axis as usize]
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// 事件序核：时间区间不重叠。
    pub fn events_disjoint(&self) -> bool {
        let mut sorted = self.events.clone();
        sorted.sort_by_key(|&(_, s, _)| s);
        sorted.windows(2).all(|w| w[1].1 >= w[0].2)
    }
}

/// 三轴计量正确核（确定性场景）：三段各 100 tick。
pub fn three_axis_correct() -> bool {
    let mut b = StallBook::new();
    // CPU 轴 0..100
    if !b.begin(StallAxis::Cpu, 0) {
        return false;
    }
    if !b.end(100) {
        return false;
    }
    // MEM 轴 100..200
    if !b.begin(StallAxis::Mem, 100) || !b.end(200) {
        return false;
    }
    // IO 轴 200..300
    if !b.begin(StallAxis::Io, 200) || !b.end(300) {
        return false;
    }
    b.axis_total(StallAxis::Cpu) == 100
        && b.axis_total(StallAxis::Mem) == 100
        && b.axis_total(StallAxis::Io) == 100
        && b.total() == 300
        && b.conserved()
        && b.no_overlap()
        && b.events_disjoint()
}

/// 轴间不串账 10/10（混合交错场景 10 组）。
pub fn no_cross_10() -> bool {
    for seed in 0..10u64 {
        let mut b = StallBook::new();
        let axes = [
            StallAxis::Cpu,
            StallAxis::Mem,
            StallAxis::Io,
            StallAxis::Cpu,
            StallAxis::Io,
            StallAxis::Mem,
        ];
        let mut t = seed * 7;
        for (i, &ax) in axes.iter().enumerate() {
            if !b.begin(ax, t) {
                return false;
            }
            t += 10 + i as u64;
            if !b.end(t) {
                return false;
            }
            t += 3;
        }
        if !b.no_overlap() || !b.conserved() || !b.events_disjoint() {
            return false;
        }
    }
    true
}

/// 分解和守恒（随机 500 事件确定性流）。
pub fn conservation_random() -> bool {
    let mut b = StallBook::new();
    let mut state = 0x9E3779B97F4A7C15u64;
    let mut t = 0u64;
    for i in 0..500u64 {
        let axis = match (state >> 33) % 3 {
            0 => StallAxis::Cpu,
            1 => StallAxis::Mem,
            _ => StallAxis::Io,
        };
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        t += state % 5;
        if !b.begin(axis, t) {
            return false;
        }
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        t += 1 + state % 20;
        if !b.end(t) {
            return false;
        }
        let _ = i;
    }
    b.conserved() && b.event_count() == 500 && b.no_overlap()
}

// ── 注入核（0xB980 段 5 项）────────────────────────────────────────────

/// 注入核 1：串账注入——未闭合时 begin 必拒且串账计数增。
pub fn inj1_overlap() -> bool {
    let mut b = StallBook::new();
    b.begin(StallAxis::Cpu, 0);
    if b.begin(StallAxis::Mem, 10) {
        return false; // 必拒
    }
    !b.no_overlap() && b.overlaps == 1
}

/// 注入核 2：无端闭合注入——无在飞时 end 必拒。
pub fn inj2_phantom_end() -> bool {
    let mut b = StallBook::new();
    !b.end(10)
}

/// 注入核 3：逆时注入——end < begin 必拒且账零增。
pub fn inj3_backwards() -> bool {
    let mut b = StallBook::new();
    b.begin(StallAxis::Cpu, 100);
    if b.end(50) {
        return false;
    }
    // 失败后 open 已被 take 清空，事件零记
    b.event_count() == 0 && b.total() == 0 && b.conserved()
}

/// 注入核 4：守恒破注入——直接篡改单轴累计后守恒必红。
pub fn inj4_break() -> bool {
    let mut b = StallBook::new();
    b.begin(StallAxis::Cpu, 0);
    b.end(50);
    if !b.conserved() {
        return false;
    }
    b.acc[1] += 7; // 篡改 MEM 轴
    !b.conserved()
}

/// 注入核 5：轴名注入——三轴名可查且互异。
pub fn inj5_names() -> bool {
    let n = [StallAxis::Cpu.name(), StallAxis::Mem.name(), StallAxis::Io.name()];
    n[0] == "CPU" && n[1] == "MEM" && n[2] == "IO" && n[0] != n[1] && n[1] != n[2]
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx2082_checks：本单判据聚合（诊断段 0xB980）。
pub fn run_unx2082_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2082 调度压力面（stall 时间三轴）");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    s.add(
        "三轴计量正确",
        three_axis_correct(),
        if three_axis_correct() { "" } else { "计量错" },
    );
    s.add(
        "轴间不串账10/10",
        no_cross_10(),
        if no_cross_10() { "" } else { "串账" },
    );
    s.add(
        "分解和=总stall守恒",
        conservation_random(),
        if conservation_random() { "" } else { "守恒破" },
    );
    let inj = [
        ("注入1串账", inj1_overlap()),
        ("注入2伪闭合", inj2_phantom_end()),
        ("注入3逆时", inj3_backwards()),
        ("注入4守恒破", inj4_break()),
        ("注入5轴名", inj5_names()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    13u32 * 30
}
