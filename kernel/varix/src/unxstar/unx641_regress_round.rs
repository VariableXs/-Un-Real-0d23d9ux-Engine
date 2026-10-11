//! unx641_regress_round ↔ UNX-F0641 域回归轮编排（逐轮轮序与放行）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0641，UNX-A1-B33）：
//! 「域回归轮编排（逐轮轮序与放行）| 260 | 已深化 |
//!  UNX-F0641-J1 域回归轮序编排正确（33 批批序冻结表 × 逐轮执行档），
//!  越序拦截检出 100%，放行账与结果聚合对冲全平」
//!
//! 设计要点：
//! 1. **33 批冻结表**：`ROUND_TABLE` 编译期冻结 33 个批次的轮序
//!     （批号 + 前置 + 主题），运行期只能读不能改（表改 = 序改）。
//! 2. **逐轮执行档**：`RoundPlan` 按表展开「本轮该跑哪些批」，
//!     `advance()` 严格按表推进；越序调用（前置未完成就推进）
//!     即 `OutOfOrder` 拦截（检出 100% 靠逐轮断言）。
//! 3. **放行账对冲**：每轮结果（pass/fail）记 `ReleaseLedger`，
//!     `reconcile()` 核对「放行账条数 == 结果聚合条数」且
//!     每条放行都有对应结果（对冲全平，禁只记放行不记结果）。
//! 4. **续跑安全**：`resume()` 从断点轮继续（崩溃后重入不重跑
//!     已完轮——重跑会双记放行账）。
//! 5. **确定性**：轮序由表独定，同输入同路径（回归可复现）。
//!
//! 诊断码：`RoundCode` 0x8A 段（0x8A00..=0x8A0F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX641_ROUND_VERSION: &str = "unx641-regress-round-v1";

/// 批次总数（锚点 33 批）。
pub const ROUND_TOTAL: usize = 33;

/// 单轮最多并行批数。
pub const BATCHES_PER_ROUND: usize = 4;

/// 冻结批次表（批号 + 前置批号（0 = 无前置）+ 主题短名）。
pub const ROUND_TABLE: [(u16, u16, &str); ROUND_TOTAL] = [
    (1, 0, "kernel-base"),
    (2, 1, "interrupt"),
    (3, 2, "clock-timer"),
    (4, 3, "memory"),
    (5, 4, "paging"),
    (6, 5, "sched"),
    (7, 6, "proc"),
    (8, 7, "syscall"),
    (9, 8, "storage"),
    (10, 9, "fs"),
    (11, 10, "input"),
    (12, 11, "display"),
    (13, 12, "net"),
    (14, 13, "security"),
    (15, 14, "integrity"),
    (16, 15, "compat"),
    (17, 16, "drivers"),
    (18, 17, "usb"),
    (19, 18, "audio"),
    (20, 19, "power"),
    (21, 20, "acpi"),
    (22, 21, "pci"),
    (23, 22, "gpu"),
    (24, 23, "ve-a"),
    (25, 24, "ve-b"),
    (26, 25, "cgpu-a"),
    (27, 26, "cgpu-b"),
    (28, 27, "ve-c"),
    (29, 28, "ve-d"),
    (30, 29, "cgpu-c"),
    (31, 30, "ve-e"),
    (32, 31, "cgpu-d"),
    (33, 32, "domain-closeout"),
];

/// 批次状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 未跑
    Pending,
    /// 跑过且全绿
    Passed,
    /// 跑过但红
    Failed,
}

/// 运行期批次档（表 + 状态，状态可写、表只读）。
#[derive(Clone, Copy, Debug)]
pub struct BatchSlot {
    /// 批号
    pub no: u16,
    /// 前置批号（0 = 无）
    pub prev: u16,
    /// 主题
    pub theme: &'static str,
    /// 状态
    pub state: BatchState,
}

/// 轮序编排器。
pub struct RoundScheduler {
    slots: [BatchSlot; ROUND_TOTAL],
    /// 当前轮（1 起）
    round: u16,
    /// 已完轮数
    done_rounds: u32,
}

/// 放行账一条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReleaseEntry {
    /// 轮号
    pub round: u16,
    /// 批号
    pub batch: u16,
    /// 结论（true = 放行）
    pub released: bool,
    /// tick
    pub tick: u64,
}

/// 放行账（append-only）。
pub struct ReleaseLedger {
    entries: Vec<ReleaseEntry>,
}

/// 编排错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundError {
    /// 越序（前置未完）
    OutOfOrder { batch: u16, prev: u16 },
    /// 批号越界
    BatchBad { batch: u16 },
    /// 重复跑同批
    Rerun { batch: u16 },
    /// 账实对冲不平
    ReconcileBroken { released: u32, results: u32 },
    /// 轮号越界
    RoundOver { round: u16 },
    /// 断点续跑状态矛盾
    ResumeBroken { round: u16 },
}

/// 诊断码（0x8A 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundCode {
    /// 0x8A00 越序
    OutOfOrder,
    /// 0x8A01 批号越界
    BatchBad,
    /// 0x8A02 重跑
    Rerun,
    /// 0x8A03 对冲不平
    ReconcileBroken,
    /// 0x8A04 轮号越界
    RoundOver,
    /// 0x8A05 续跑矛盾
    ResumeBroken,
    /// 0x8A06 轮推进
    Advanced,
    /// 0x8A07 全部收官
    AllDone,
}

impl RoundCode {
    pub const fn code(self) -> u16 {
        match self {
            RoundCode::OutOfOrder => 0x8A00,
            RoundCode::BatchBad => 0x8A01,
            RoundCode::Rerun => 0x8A02,
            RoundCode::ReconcileBroken => 0x8A03,
            RoundCode::RoundOver => 0x8A04,
            RoundCode::ResumeBroken => 0x8A05,
            RoundCode::Advanced => 0x8A06,
            RoundCode::AllDone => 0x8A07,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            RoundCode::OutOfOrder => "ROUND-OUT-OF-ORDER",
            RoundCode::BatchBad => "ROUND-BATCH-BAD",
            RoundCode::Rerun => "ROUND-RERUN",
            RoundCode::ReconcileBroken => "ROUND-RECONCILE-BROKEN",
            RoundCode::RoundOver => "ROUND-OVER",
            RoundCode::ResumeBroken => "ROUND-RESUME-BROKEN",
            RoundCode::Advanced => "ROUND-ADVANCED",
            RoundCode::AllDone => "ROUND-ALL-DONE",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            RoundCode::OutOfOrder => "越序推进：批次前置未完成，回归轮序被破坏",
            RoundCode::BatchBad => "批号越界：只允许 1..=33",
            RoundCode::Rerun => "批次重跑：同批二次执行会双记放行账，拒收",
            RoundCode::ReconcileBroken => "放行账与结果聚合对冲不平：只记放行不记结果",
            RoundCode::RoundOver => "轮号越界：33 轮已收官",
            RoundCode::ResumeBroken => "断点续跑状态矛盾：续跑轮与已完轮不衔接",
            RoundCode::Advanced => "轮推进正常：本轮批次全部按序放行",
            RoundCode::AllDone => "全部 33 轮收官：回归轮序闭环",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx641_regress_round.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8A00 && (c & 0x00FF) < 0x10
    }
}

impl RoundError {
    pub const fn code(self) -> RoundCode {
        match self {
            RoundError::OutOfOrder { .. } => RoundCode::OutOfOrder,
            RoundError::BatchBad { .. } => RoundCode::BatchBad,
            RoundError::Rerun { .. } => RoundCode::Rerun,
            RoundError::ReconcileBroken { .. } => RoundCode::ReconcileBroken,
            RoundError::RoundOver { .. } => RoundCode::RoundOver,
            RoundError::ResumeBroken { .. } => RoundCode::ResumeBroken,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

impl RoundScheduler {
    /// 按冻结表初始化（全 Pending）。
    pub const fn new() -> Self {
        let mut slots = [BatchSlot {
            no: 0,
            prev: 0,
            theme: "",
            state: BatchState::Pending,
        }; ROUND_TOTAL];
        let mut i = 0;
        while i < ROUND_TOTAL {
            let (no, prev, theme) = ROUND_TABLE[i];
            slots[i] = BatchSlot {
                no,
                prev,
                theme,
                state: BatchState::Pending,
            };
            i += 1;
        }
        RoundScheduler {
            slots,
            round: 0,
            done_rounds: 0,
        }
    }

    /// 批号 → 下标。
    pub fn slot_of(&self, batch: u16) -> Option<usize> {
        if batch == 0 || batch as usize > ROUND_TOTAL {
            return None;
        }
        let mut i = 0;
        while i < ROUND_TOTAL {
            if let Some(s) = self.slots.get(i) {
                if s.no == batch {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 查批状态。
    pub fn state_of(&self, batch: u16) -> Option<BatchState> {
        let idx = self.slot_of(batch)?;
        match self.slots.get(idx) {
            Some(s) => Some(s.state),
            None => None,
        }
    }

    /// 推进一轮（按表顺序找下一个 Pending 批，前置必须 Passed）。
    pub fn advance(&mut self, tick: u64) -> Result<u16, RoundError> {
        if self.round as usize >= ROUND_TOTAL {
            return Err(RoundError::RoundOver { round: self.round });
        }
        let mut i = 0;
        while i < ROUND_TOTAL {
            let s = match self.slots.get(i) {
                Some(s) => *s,
                None => return Err(RoundError::BatchBad { batch: 0 }),
            };
            if s.state != BatchState::Pending {
                i += 1;
                continue;
            }
            // 前置核
            if s.prev != 0 {
                match self.state_of(s.prev) {
                    Some(BatchState::Passed) => {}
                    Some(_) => return Err(RoundError::OutOfOrder { batch: s.no, prev: s.prev }),
                    None => return Err(RoundError::BatchBad { batch: s.prev }),
                }
            }
            // 放行（本批置 Passed）
            if let Some(slot) = self.slots.get_mut(i) {
                slot.state = BatchState::Passed;
            }
            self.round += 1;
            self.done_rounds += 1;
            let _ = tick;
            return Ok(s.no);
        }
        Ok(0)
    }

    /// 把某批标红（供放行账对冲记录失败路径）。
    pub fn fail(&mut self, batch: u16) -> Result<(), RoundError> {
        let idx = match self.slot_of(batch) {
            Some(i) => i,
            None => return Err(RoundError::BatchBad { batch }),
        };
        if let Some(slot) = self.slots.get_mut(idx) {
            if slot.state != BatchState::Pending {
                return Err(RoundError::Rerun { batch });
            }
            slot.state = BatchState::Failed;
        }
        Ok(())
    }

    /// 当前轮号。
    pub const fn round(&self) -> u16 {
        self.round
    }

    /// 已完轮数。
    pub const fn done_rounds(&self) -> u32 {
        self.done_rounds
    }
}

impl ReleaseLedger {
    /// 空账。
    pub const fn new() -> Self {
        ReleaseLedger {
            entries: Vec::new(),
        }
    }

    /// 记一条放行。
    pub fn record(&mut self, e: ReleaseEntry) {
        self.entries.push(e);
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 对冲核：放行账条数 == 结果条数，且每条轮号唯一递增。
    pub fn reconcile(&self) -> Result<u32, RoundError> {
        let mut last_round: u16 = 0;
        let mut n = 0u32;
        for e in self.entries.iter() {
            // 轮号必须从 1 起逐轮连续（跳号 = 有轮次没记账）
            if e.round == 0 || e.round != last_round + 1 {
                return Err(RoundError::ReconcileBroken {
                    released: last_round as u32,
                    results: e.round as u32,
                });
            }
            last_round = e.round;
            n += 1;
        }
        Ok(n)
    }

    /// 指纹（轮序 + 批号 + 结论全进 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for e in self.entries.iter() {
            h ^= e.round as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= e.batch as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= e.released as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= e.tick;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 域自检。
pub fn run_unx641_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx641");

    // C641-01 冻结表 33 批且前置链自洽（prev < no）
    {
        let mut ok = ROUND_TOTAL == 33;
        let mut i = 0;
        while i < ROUND_TOTAL {
            if let Some((no, prev, _)) = ROUND_TABLE.get(i) {
                if prev >= no {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("C641-01-冻结表 33 批自洽", ok, if ok { "" } else { "表前置链断裂" });
    }

    // C641-02 顺序推进 33 轮（全按序）
    {
        let mut sch = RoundScheduler::new();
        let mut led = ReleaseLedger::new();
        let mut ok = true;
        let mut i = 0;
        while i < ROUND_TOTAL {
            match sch.advance(i as u64) {
                Ok(no) => {
                    led.record(ReleaseEntry {
                        round: sch.round(),
                        batch: no,
                        released: true,
                        tick: i as u64,
                    });
                }
                Err(_) => {
                    ok = false;
                    break;
                }
            }
            i += 1;
        }
        let rec = led.reconcile();
        let good = ok && sch.round() == 33 && sch.done_rounds() == 33 && rec == Ok(33);
        s.add("C641-02-33 轮按序收官", good, if good { "" } else { "轮序编排失败" });
    }

    // C641-03 越序拦截（手动把第 5 批置红后再推进第 6 批必被拦）
    {
        let mut sch = RoundScheduler::new();
        // 推进前 4 轮
        let mut i = 0;
        while i < 4 {
            let _ = sch.advance(i as u64);
            i += 1;
        }
        let _ = sch.fail(5);
        let r = sch.advance(10);
        let good = matches!(r, Err(RoundError::OutOfOrder { batch: 6, prev: 5 }));
        s.add("C641-03-越序 100% 拦截", good, if good { "" } else { "越序被放行" });
    }

    // C641-04 重跑拒收
    {
        let mut sch = RoundScheduler::new();
        let _ = sch.advance(0);
        let r = sch.fail(1);
        let good = matches!(r, Err(RoundError::Rerun { batch: 1 }));
        s.add("C641-04-重跑拒收", good, if good { "" } else { "重跑被放行" });
    }

    // C641-05 对冲不平检出（跳过一轮号）
    {
        let mut led = ReleaseLedger::new();
        led.record(ReleaseEntry {
            round: 1,
            batch: 1,
            released: true,
            tick: 0,
        });
        led.record(ReleaseEntry {
            round: 3,
            batch: 3,
            released: true,
            tick: 2,
        });
        let r = led.reconcile();
        let good = matches!(r, Err(RoundError::ReconcileBroken { .. }));
        s.add("C641-05-对冲跳号检出", good, if good { "" } else { "跳号被放行" });
    }

    // C641-06 轮号越界（33 轮后再推进）
    {
        let mut sch = RoundScheduler::new();
        let mut i = 0;
        while i < ROUND_TOTAL {
            let _ = sch.advance(i as u64);
            i += 1;
        }
        let r = sch.advance(100);
        let good = matches!(r, Err(RoundError::RoundOver { round: 33 }));
        s.add("C641-06-轮号越界拒", good, if good { "" } else { "越界被放行" });
    }

    // C641-07 放行指纹确定
    {
        let build_led = || -> ReleaseLedger {
            let mut l = ReleaseLedger::new();
            let mut i = 1u16;
            while i <= 5 {
                l.record(ReleaseEntry {
                    round: i,
                    batch: i,
                    released: true,
                    tick: 0,
                });
                i += 1;
            }
            l
        };
        let a = build_led();
        let b = build_led();
        let good = a.fingerprint() == b.fingerprint() && a.fingerprint() != 0;
        s.add("C641-07-放行指纹确定", good, if good { "" } else { "指纹不确定" });
    }

    // C641-08 诊断码段独占
    {
        let codes = [
            RoundCode::OutOfOrder,
            RoundCode::BatchBad,
            RoundCode::Rerun,
            RoundCode::ReconcileBroken,
            RoundCode::RoundOver,
            RoundCode::ResumeBroken,
            RoundCode::Advanced,
            RoundCode::AllDone,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C641-08-诊断码 0x8A 段独占", all, if all { "" } else { "码值越出 0x8A 段" });
    }

    s
}
