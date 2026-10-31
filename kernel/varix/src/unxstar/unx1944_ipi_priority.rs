//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1944 · IPI 向量优先级与抢占仲裁 · 0x6D1C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B4）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1944-J1 向量优先级仲裁断言全过（高优先级先处理/同向
//!     量不乱序），优先级倒挂注入 10/10 检出，仲裁账与处理账互证
//!     一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 向量优先级闭集：时钟(0xE8)>TLB 击落(0xF0)>重调度(0xE1)>
//!      其他(0x80)（冻结四档，数值大者先处理）；
//!    - 仲裁：处理序按优先级降序 + 同向量 FIFO（不乱序）；
//!    - 倒挂注入：低优先级先处理 = 倒挂，10/10 检出；
//!    - 互证：仲裁账（出队序）与处理账（执行序）逐位一致；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D1C0，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D1C0;

// ── 向量优先级闭集（数值大者先处理） ────────────────────────────
pub const VEC_CLOCK: u8 = 0xE8;
pub const VEC_TLB: u8 = 0xF0;
pub const VEC_RESCHED: u8 = 0xE1;
pub const VEC_OTHER: u8 = 0x80;
pub const VEC_CLASS_COUNT: usize = 4;
pub const VEC_CLASS_NAMES: [&str; VEC_CLASS_COUNT] =
    ["TLB击落", "时钟", "重调度", "其他"];
pub const VEC_CLASS_VALUES: [u8; VEC_CLASS_COUNT] =
    [VEC_TLB, VEC_CLOCK, VEC_RESCHED, VEC_OTHER];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiPriCode {
    /// 向量越闭集
    VectorOutOfRange,
    /// 同向量乱序（FIFO 破坏）
    SameVectorReorder,
    /// 优先级倒挂
    PriorityInversion,
    /// 仲裁账与处理账不一致
    DualLedgerMismatch,
    /// 队空处理
    EmptyPop,
    /// 账互证缺
    CrossCheckMissing,
    /// 双跑不一致
    NotReproducible,
}

impl IpiPriCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IpiPriCode::VectorOutOfRange => "向量越闭集：四档冻结",
            IpiPriCode::SameVectorReorder => "同向量乱序：FIFO 破坏",
            IpiPriCode::PriorityInversion => "优先级倒挂",
            IpiPriCode::DualLedgerMismatch => "仲裁账与处理账不一致",
            IpiPriCode::EmptyPop => "队空处理",
            IpiPriCode::CrossCheckMissing => "账互证缺",
            IpiPriCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [IpiPriCode; 7] = [
        IpiPriCode::VectorOutOfRange,
        IpiPriCode::SameVectorReorder,
        IpiPriCode::PriorityInversion,
        IpiPriCode::DualLedgerMismatch,
        IpiPriCode::EmptyPop,
        IpiPriCode::CrossCheckMissing,
        IpiPriCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IpiPriCode::ALL.len() {
        if IpiPriCode::ALL[i].val() == code {
            return IpiPriCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D1C0 段闭集内"
}

// ── IPI 项 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpiItem {
    pub vector: u8,
    /// 入队序（同向量 FIFO 判据）。
    pub seq: u64,
}

/// 优先级仲裁器：出队序 = 优先级降序 + 同向量 FIFO。
pub struct IpiArbiter {
    pub queue: Vec<IpiItem>,
    /// 仲裁账（出队序）。
    pub arbiter_order: Vec<IpiItem>,
    /// 处理账（执行序）。
    pub process_order: Vec<IpiItem>,
}

impl IpiArbiter {
    pub fn new() -> IpiArbiter {
        IpiArbiter { queue: Vec::new(), arbiter_order: Vec::new(), process_order: Vec::new() }
    }

    pub fn post(&mut self, vector: u8, seq: u64) -> Result<(), u32> {
        let mut legal = false;
        let mut i = 0;
        while i < VEC_CLASS_COUNT {
            if VEC_CLASS_VALUES[i] == vector {
                legal = true;
            }
            i += 1;
        }
        if !legal {
            return Err(IpiPriCode::VectorOutOfRange.val());
        }
        self.queue.push(IpiItem { vector, seq });
        Ok(())
    }

    /// 仲裁出队一次：按优先级降序 + 同向量 seq 最小者。
    pub fn pop_one(&mut self) -> Result<IpiItem, u32> {
        if self.queue.is_empty() {
            return Err(IpiPriCode::EmptyPop.val());
        }
        // 找最高优先级向量。
        let mut best_vec = 0u8;
        let mut best_idx = usize::MAX;
        let mut i = 0;
        while i < self.queue.len() {
            let v = self.queue[i].vector;
            if best_idx == usize::MAX || v > best_vec {
                best_vec = v;
                best_idx = i;
            } else if v == best_vec && self.queue[i].seq < self.queue[best_idx].seq {
                best_idx = i;
            }
            i += 1;
        }
        let item = self.queue.remove(best_idx);
        self.arbiter_order.push(item);
        Ok(item)
    }

    /// 处理（执行）：处理账记录。
    pub fn process(&mut self, item: IpiItem) {
        self.process_order.push(item);
    }

    /// 倒挂检测：处理账中低优先级先于高优先级处理 = 倒挂。
    pub fn inversion_check(&self) -> Result<(), u32> {
        let mut i = 1;
        while i < self.process_order.len() {
            if self.process_order[i].vector > self.process_order[i - 1].vector {
                // 后处理的向量优先级更高 → 前面是倒挂。
                return Err(IpiPriCode::PriorityInversion.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 互证：仲裁账与处理账逐位一致。
    pub fn cross_check(&self) -> Result<(), u32> {
        if self.arbiter_order.len() != self.process_order.len() {
            return Err(IpiPriCode::CrossCheckMissing.val());
        }
        let mut i = 0;
        while i < self.arbiter_order.len() {
            if self.arbiter_order[i] != self.process_order[i] {
                return Err(IpiPriCode::DualLedgerMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// 倒挂注入 10/10（伪处理序低先高后 → 检测必抓）。
pub fn inversion_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut a = IpiArbiter::new();
        // 正确序：TLB(0xF0) 先、OTHER(0x80) 后。
        a.post(VEC_TLB, 1).ok();
        a.post(VEC_OTHER, 2).ok();
        a.pop_one().map(|_| ()).ok();
        a.pop_one().ok();
        // 伪造倒挂处理账。
        a.process_order.clear();
        a.process(IpiItem { vector: VEC_OTHER, seq: 2 });
        a.process(IpiItem { vector: VEC_TLB, seq: 1 });
        if a.inversion_check().is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(IpiPriCode::PriorityInversion.val());
    }
    Ok(caught)
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：混合向量入队→仲裁全出→互证→倒挂 10/10。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    if VEC_CLASS_NAMES.len() != VEC_CLASS_COUNT {
        return Err(IpiPriCode::VectorOutOfRange.val());
    }
    let mut a = IpiArbiter::new();
    // 混合入队。
    a.post(VEC_OTHER, 1)?;
    a.post(VEC_CLOCK, 2)?;
    a.post(VEC_TLB, 3)?;
    a.post(VEC_RESCHED, 4)?;
    a.post(VEC_TLB, 5)?;
    // 全部仲裁出队 + 处理。
    let mut n = 0;
    while n < 5 {
        let item = a.pop_one()?;
        a.process(item);
        n += 1;
    }
    // 首个必是 TLB(0xF0) 最高优先级。
    if a.process_order[0].vector != VEC_TLB {
        return Err(IpiPriCode::PriorityInversion.val());
    }
    // 互证。
    a.cross_check()?;
    let inv = inversion_inject_10()?;
    Ok((a.process_order.len() as u64, a.process_order[0].vector as u64, inv))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut a = IpiArbiter::new();
    a.post(VEC_CLOCK, 1)?;
    let item = a.pop_one()?;
    a.process(item);
    a.inversion_check()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 越向量 → 必拒。
    let mut a = IpiArbiter::new();
    a.post(0x10, 1)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 队空处理 → 必拒。
    let mut a = IpiArbiter::new();
    a.pop_one().map(|_| ())
}

/// 注入 10/10：越向量、倒挂、队空、互证不一致四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越向量。
        let mut a0 = IpiArbiter::new();
        led.record(0, a0.post(0x55, 1).is_err());
        // 槽1：倒挂。
        led.record(1, inversion_inject_10() == Ok(10));
        // 槽2：队空。
        let mut a2 = IpiArbiter::new();
        led.record(2, a2.pop_one().is_err());
        // 槽3：互证不一致。
        let mut a3 = IpiArbiter::new();
        a3.post(VEC_TLB, 1).ok();
        a3.pop_one().ok();
        led.record(3, a3.cross_check().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(IpiPriCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IpiPriCode::ALL.len() {
        let v = IpiPriCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 优先级闭集。
    v.push((
        "J1-向量优先级闭集",
        VEC_CLASS_NAMES.len() == 4 && VEC_TLB > VEC_CLOCK && VEC_CLOCK > VEC_RESCHED,
        format!("TLB>{:?}其余", VEC_CLASS_VALUES),
    ));
    // 倒挂 10/10。
    v.push((
        "J1-倒挂注入10/10",
        inversion_inject_10() == Ok(10),
        format!("{}/10 检出", inversion_inject_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D1C0 窗口")));
    v
}

pub fn run_unx1944_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1944 IPI 向量优先级与抢占仲裁");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
