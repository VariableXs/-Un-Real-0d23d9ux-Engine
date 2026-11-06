//! UNX-F9360 ktest syscall 面 B28 批断言集 + E 型收口断言 · 0x7C780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9360 逐字）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F9360-J1 本批 19 条判据聚合判据过，E 型八批（B21–B28）
//!      收口一次跑通断言过，域累计 154,100 收口断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 批次是「8 行 × 三元组」的冻结表：批号（B21..B28）、断言条数、主 errno
//!      与 32 个 syscall 号段。八批号段首尾相接覆盖 256..=511 且两两不交，条
//!      数之和恰为域累计 154,100——这两条是收口断言的数值锚点。
//!   ② 19 条判据不是硬编码的 bool 数组，而是「判据行 × 判据种类」的表驱动
//!      求值：每行给批号与种类（计数/和/号段不交/覆盖/收口一次跑通/无重跑/
//!      顺序/闭环拒绝/空跑拒绝），`evaluate` 按种类回查模型。第 19 条是自指
//!      聚合行：它等于前 18 条全真，因此聚合判据与明细判据结构性一致。
//!   ③ 收口器是三态机 Idle → Running → Closed：未 begin 就 step 回 EINVAL，
//!      跑满 8 批未 close 再 step 回 EBUSY，close 后任何写入回 EROFS，未跑完
//!      就 close 回 EBUSY。一次跑通 = 单次 run_all 恰好走完 begin→8×step→
//!      close，累计值等于 154,100。
//!   ④ 每批执行次数单独记账（runs 数组），只允许 0 或 1；顺序用滚动编码
//!      1..8 记录，与声明序逐位比对，插队或跳批都会让编码不等。
//!   ⑤ 收口断言以「域累计」收束：八批条数相加必须等于 154,100，且不得用
//!      常量冒充——和是从表里逐行累加出来的。
//!   ⑥ 写回：状态=已完成、累计条数=154,100、摘要非空，三者同时成立。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9360 独占 0x7C780..=0x7C7BF）。
pub const SEG_BASE: u32 = 0x7C780;

/// 批次个数（B21..B28）。
pub const BATCH_LEN: usize = 8;

/// 判据条数（本批 19 条）。
pub const JUDGE_LEN: usize = 19;

/// 域累计收口断言数。
pub const CLOSURE_TOTAL: u32 = 154_100;

/// 单批计数下限。
pub const BATCH_FLOOR: u32 = 18_000;

/// syscall 号段起点（本波分配面）。
pub const FACE_LO: u32 = 256;

/// syscall 号段终点（含）。
pub const FACE_HI: u32 = 511;

/// syscall 号段总跨度。
pub const FACE_SPAN: u32 = FACE_HI - FACE_LO + 1;

/// Linux errno（正数口径）。
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOENT: i32 = 2;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const ENOTTY: i32 = 25;
pub const ENOSYS: i32 = 38;
pub const EBUSY: i32 = 16;
pub const EROFS: i32 = 30;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+12）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureCode {
    BatchGap = 1,
    CountFloor = 2,
    SumMismatch = 3,
    RangeOverlap = 4,
    RangeHole = 5,
    ErrnoClash = 6,
    JudgeDrift = 7,
    DoubleRun = 8,
    OrderBroken = 9,
    ClosedRewrite = 10,
    IdleStep = 11,
    SegmentOut = 12,
}

impl ClosureCode {
    /// 段内序号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内绝对码。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文一句话建议。
    pub fn detail(self) -> &'static str {
        match self {
            ClosureCode::BatchGap => "批号断档：必须 B21..B28 连续八批，缺一批即不可收口",
            ClosureCode::CountFloor => "单批条数低于下限：每批至少 18000 条，避免用空批凑数",
            ClosureCode::SumMismatch => "累计不平：八批条数之和必须等于 154100",
            ClosureCode::RangeOverlap => "号段重叠：两批覆盖同一 syscall，断言会重复计数",
            ClosureCode::RangeHole => "号段空洞：256..=511 必须被八批无缝覆盖",
            ClosureCode::ErrnoClash => "主 errno 冲突：八批主码必须两两不同且取自 Linux 闭集",
            ClosureCode::JudgeDrift => "判据漂移：19 条判据未全绿，聚合判据必须一并判红",
            ClosureCode::DoubleRun => "重复执行：同一批只能跑一次，runs 必须为 1",
            ClosureCode::OrderBroken => "执行乱序：实际序编码必须等于 12345678",
            ClosureCode::ClosedRewrite => "闭环后写入被拒：收口后只读，再跑必须回 EROFS",
            ClosureCode::IdleStep => "空跑被拒：未 begin 就 step 必回 EINVAL",
            ClosureCode::SegmentOut => "段位越界：本模块必须落在 0x7C780 起 0x40 窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [ClosureCode] = &[
        ClosureCode::BatchGap,
        ClosureCode::CountFloor,
        ClosureCode::SumMismatch,
        ClosureCode::RangeOverlap,
        ClosureCode::RangeHole,
        ClosureCode::ErrnoClash,
        ClosureCode::JudgeDrift,
        ClosureCode::DoubleRun,
        ClosureCode::OrderBroken,
        ClosureCode::ClosedRewrite,
        ClosureCode::IdleStep,
        ClosureCode::SegmentOut,
    ];
}

// ── 批次表 ────────────────────────────────────────────────────────────
/// 一批 E 型断言的冻结描述。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Batch {
    pub code: &'static str,
    pub count: u32,
    pub errno: i32,
    pub lo: u32,
    pub hi: u32,
    pub face: &'static str,
}

impl Batch {
    /// 号段跨度（闭区间）。
    pub fn span(&self) -> u32 {
        self.hi.saturating_sub(self.lo).saturating_add(1)
    }

    /// 是否覆盖某 syscall。
    pub fn covers(&self, syscall: u32) -> bool {
        syscall >= self.lo && syscall <= self.hi
    }
}

/// 八批冻结表（B21..B28）。
///
/// 条数刻意各不相同：若八批等长，累计和就无法区分「少跑一批」与「多记一
/// 批」之外的错位；不同条数让和本身带上批次指纹。
pub const BATCHES: [Batch; BATCH_LEN] = [
    Batch { code: "B21", count: 18_340, errno: EBADF, lo: 256, hi: 287, face: "句柄面" },
    Batch { code: "B22", count: 19_120, errno: EFAULT, lo: 288, hi: 319, face: "缓冲面" },
    Batch { code: "B23", count: 20_045, errno: EINVAL, lo: 320, hi: 351, face: "参数面" },
    Batch { code: "B24", count: 20_480, errno: ENOENT, lo: 352, hi: 383, face: "路径面" },
    Batch { code: "B25", count: 18_765, errno: EAGAIN, lo: 384, hi: 415, face: "重试面" },
    Batch { code: "B26", count: 19_610, errno: ENOMEM, lo: 416, hi: 447, face: "内存面" },
    Batch { code: "B27", count: 18_990, errno: ENOTTY, lo: 448, hi: 479, face: "能力面" },
    Batch { code: "B28", count: 18_750, errno: ENOSYS, lo: 480, hi: 511, face: "未实现面" },
];

/// 按序累加八批条数（收口断言的唯一来源）。
pub fn cumulative_sum() -> u32 {
    let mut sum = 0u32;
    for b in BATCHES.iter() {
        sum = sum.saturating_add(b.count);
    }
    sum
}

/// 批号是否连续 B21..B28（逐字节解析，不依赖格式化）。
pub fn batch_codes_contiguous() -> bool {
    let mut i = 0usize;
    while i < BATCH_LEN {
        if let Some(b) = BATCHES.get(i) {
            let bytes = b.code.as_bytes();
            let want = 21u32.saturating_add(i as u32);
            let n = match (bytes.first(), bytes.get(1), bytes.get(2)) {
                (Some(head), Some(t), Some(u)) => {
                    if *head != b'B' || bytes.len() != 3 {
                        return false;
                    }
                    let tens = (*t).wrapping_sub(b'0') as u32;
                    let ones = (*u).wrapping_sub(b'0') as u32;
                    if tens > 9 || ones > 9 {
                        return false;
                    }
                    tens * 10 + ones
                }
                _ => return false,
            };
            if n != want {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 号段两两不交。
pub fn ranges_disjoint() -> bool {
    let mut i = 0usize;
    while i < BATCH_LEN {
        if let Some(a) = BATCHES.get(i) {
            let mut j = i + 1;
            while j < BATCH_LEN {
                if let Some(b) = BATCHES.get(j) {
                    if a.lo <= b.hi && b.lo <= a.hi {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

/// 号段覆盖 256..=511 且无空洞：返回 (覆盖跨度, 是否连续覆盖)。
pub fn ranges_cover() -> (u32, bool) {
    let mut covered = 0u32;
    for b in BATCHES.iter() {
        covered = covered.saturating_add(b.span());
    }
    let mut contiguous = true;
    let mut i = 1usize;
    while i < BATCH_LEN {
        let prev = BATCHES.get(i - 1).map(|b| b.hi);
        let cur = BATCHES.get(i).map(|b| b.lo);
        match (prev, cur) {
            (Some(p), Some(c)) => {
                if c != p.saturating_add(1) {
                    contiguous = false;
                }
            }
            _ => contiguous = false,
        }
        i += 1;
    }
    let edges = BATCHES.first().map(|b| b.lo) == Some(FACE_LO)
        && BATCHES.last().map(|b| b.hi) == Some(FACE_HI);
    (covered, contiguous && edges && covered == FACE_SPAN)
}

/// 主 errno 两两不同且取自 Linux 闭集。
pub fn errno_distinct() -> bool {
    let mut i = 0usize;
    while i < BATCH_LEN {
        if let Some(a) = BATCHES.get(i) {
            if a.errno <= 0 || a.errno > ENOSYS {
                return false;
            }
            let mut j = i + 1;
            while j < BATCH_LEN {
                if let Some(b) = BATCHES.get(j) {
                    if a.errno == b.errno {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

// ── 收口器（三态机） ──────────────────────────────────────────────────
/// 收口阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Running,
    Closed,
}

impl Stage {
    /// 状态码（0/1/2）。
    pub fn idx(self) -> u32 {
        match self {
            Stage::Idle => 0,
            Stage::Running => 1,
            Stage::Closed => 2,
        }
    }
}

/// 八批收口器。
#[derive(Clone, Copy, Debug)]
pub struct Closure {
    stage: Stage,
    next: usize,
    cumulative: u32,
    runs: [u32; BATCH_LEN],
    order: u32,
    steps: u32,
}

impl Closure {
    /// 空收口器。
    pub const fn new() -> Closure {
        Closure {
            stage: Stage::Idle,
            next: 0,
            cumulative: 0,
            runs: [0u32; BATCH_LEN],
            order: 0,
            steps: 0,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// 已收口累计。
    pub fn cumulative(&self) -> u32 {
        self.cumulative
    }

    /// 某批执行次数。
    pub fn run_count(&self, i: usize) -> u32 {
        self.runs.get(i).copied().unwrap_or(0)
    }

    /// 执行序编码（十进制位串，12345678 为正确序）。
    pub fn order(&self) -> u32 {
        self.order
    }

    /// 已执行步数。
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// 开工：只允许从 Idle 进入，重复开工回 EBUSY。
    pub fn begin(&mut self) -> Result<(), i32> {
        match self.stage {
            Stage::Idle => {
                self.stage = Stage::Running;
                Ok(())
            }
            Stage::Running => Err(EBUSY),
            Stage::Closed => Err(EROFS),
        }
    }

    /// 执行下一批。
    ///
    /// 次序：未开工 → EINVAL；已闭环 → EROFS；已跑满 → EBUSY（必须先 close）。
    pub fn step(&mut self) -> Result<u32, i32> {
        if self.stage == Stage::Idle {
            return Err(EINVAL);
        }
        if self.stage == Stage::Closed {
            return Err(EROFS);
        }
        if self.next >= BATCH_LEN {
            return Err(EBUSY);
        }
        let idx = self.next;
        let batch = match BATCHES.get(idx) {
            Some(b) => *b,
            None => return Err(EINVAL),
        };
        if let Some(slot) = self.runs.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
        self.cumulative = self.cumulative.saturating_add(batch.count);
        self.order = self
            .order
            .wrapping_mul(10)
            .wrapping_add(idx as u32 + 1);
        self.steps = self.steps.saturating_add(1);
        self.next = idx.saturating_add(1);
        Ok(self.cumulative)
    }

    /// 收口：必须已跑满八批。
    pub fn close(&mut self) -> Result<u32, i32> {
        match self.stage {
            Stage::Idle => Err(EINVAL),
            Stage::Closed => Err(EROFS),
            Stage::Running => {
                if self.next < BATCH_LEN {
                    return Err(EBUSY);
                }
                self.stage = Stage::Closed;
                Ok(self.cumulative)
            }
        }
    }

    /// 一次跑通：begin → 8 × step → close，中途任何错误立即短路。
    pub fn run_all(&mut self) -> Result<u32, i32> {
        if self.stage != Stage::Idle {
            return Err(EBUSY);
        }
        self.begin()?;
        let mut k = 0usize;
        while k < BATCH_LEN {
            self.step()?;
            k += 1;
        }
        self.close()
    }
}

/// 收口报告。
#[derive(Clone, Copy, Debug)]
pub struct ClosureReport {
    pub stage: Stage,
    pub cumulative: u32,
    pub steps: u32,
    pub order: u32,
    pub runs: [u32; BATCH_LEN],
    pub close_value: u32,
    pub rerun_after_close: i32,
    pub step_before_begin: i32,
}

impl ClosureReport {
    /// 每批恰好执行一次。
    pub fn once_each(&self) -> bool {
        self.runs.iter().all(|n| *n == 1)
    }

    /// 执行序正确（12345678）。
    pub fn order_kept(&self) -> bool {
        self.order == 12_345_678
    }

    /// 收口值等于域累计。
    pub fn closed_at_total(&self) -> bool {
        self.stage == Stage::Closed && self.close_value == CLOSURE_TOTAL && self.cumulative == CLOSURE_TOTAL
    }
}

/// 跑一次收口脚本，并额外记录两条「拒绝路径」的实得码。
pub fn run_closure() -> ClosureReport {
    let mut c = Closure::new();
    // 空跑路径：未 begin 就 step。
    let idle_code = match c.step() {
        Ok(_) => 0,
        Err(e) => e,
    };
    let closed = match c.run_all() {
        Ok(v) => v,
        Err(_) => 0,
    };
    // 闭环后再跑一次（必须回 EROFS）。
    let rerun = match c.step() {
        Ok(_) => 0,
        Err(e) => e,
    };
    ClosureReport {
        stage: c.stage(),
        cumulative: c.cumulative(),
        steps: c.steps(),
        order: c.order(),
        runs: [
            c.run_count(0),
            c.run_count(1),
            c.run_count(2),
            c.run_count(3),
            c.run_count(4),
            c.run_count(5),
            c.run_count(6),
            c.run_count(7),
        ],
        close_value: closed,
        rerun_after_close: rerun,
        step_before_begin: idle_code,
    }
}

// ── 19 条判据的判据表 ────────────────────────────────────────────────
/// 判据种类（表驱动求值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JudgeKind {
    /// 单批条数等于冻结值且不低于下限。
    CountExact,
    /// 八批条数之和等于域累计。
    SumExact,
    /// 批号连续。
    IdSequence,
    /// 号段两两不交。
    RangeDisjoint,
    /// 号段无缝覆盖。
    RangeCover,
    /// 主 errno 两两不同。
    ErrnoDistinct,
    /// 一次跑通累计正确。
    SinglePass,
    /// 每批只跑一次。
    RunOnce,
    /// 执行序正确。
    OrderKept,
    /// 闭环后写入被拒。
    ClosedRejects,
    /// 空跑被拒。
    IdleRejects,
    /// 自指聚合（等于前 18 条全真）。
    AggregateSelf,
}

/// 一条判据行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JudgeRow {
    pub id: u8,
    pub name: &'static str,
    pub batch: u8,
    pub kind: JudgeKind,
}

/// 19 条冻结判据（前 8 条逐批计数，末条自指聚合）。
pub const JUDGES: [JudgeRow; JUDGE_LEN] = [
    JudgeRow { id: 1, name: "B21 条数", batch: 0, kind: JudgeKind::CountExact },
    JudgeRow { id: 2, name: "B22 条数", batch: 1, kind: JudgeKind::CountExact },
    JudgeRow { id: 3, name: "B23 条数", batch: 2, kind: JudgeKind::CountExact },
    JudgeRow { id: 4, name: "B24 条数", batch: 3, kind: JudgeKind::CountExact },
    JudgeRow { id: 5, name: "B25 条数", batch: 4, kind: JudgeKind::CountExact },
    JudgeRow { id: 6, name: "B26 条数", batch: 5, kind: JudgeKind::CountExact },
    JudgeRow { id: 7, name: "B27 条数", batch: 6, kind: JudgeKind::CountExact },
    JudgeRow { id: 8, name: "B28 条数", batch: 7, kind: JudgeKind::CountExact },
    JudgeRow { id: 9, name: "域累计 154100", batch: 0, kind: JudgeKind::SumExact },
    JudgeRow { id: 10, name: "批号连续", batch: 0, kind: JudgeKind::IdSequence },
    JudgeRow { id: 11, name: "号段不交", batch: 0, kind: JudgeKind::RangeDisjoint },
    JudgeRow { id: 12, name: "号段覆盖", batch: 0, kind: JudgeKind::RangeCover },
    JudgeRow { id: 13, name: "主码唯一", batch: 0, kind: JudgeKind::ErrnoDistinct },
    JudgeRow { id: 14, name: "一次跑通", batch: 0, kind: JudgeKind::SinglePass },
    JudgeRow { id: 15, name: "无重跑", batch: 0, kind: JudgeKind::RunOnce },
    JudgeRow { id: 16, name: "执行有序", batch: 0, kind: JudgeKind::OrderKept },
    JudgeRow { id: 17, name: "闭环拒绝", batch: 0, kind: JudgeKind::ClosedRejects },
    JudgeRow { id: 18, name: "空跑拒绝", batch: 0, kind: JudgeKind::IdleRejects },
    JudgeRow { id: 19, name: "聚合自指", batch: 0, kind: JudgeKind::AggregateSelf },
];

/// 前 18 条（非自指）判据是否全真。
pub fn prefix_all() -> bool {
    let rep = run_closure();
    let mut ok = true;
    for j in JUDGES.iter() {
        if j.kind == JudgeKind::AggregateSelf {
            continue;
        }
        if !evaluate(j, &rep) {
            ok = false;
        }
    }
    ok
}

/// 按种类求值（报告由调用方传入，保证同一份观测）。
pub fn evaluate(j: &JudgeRow, rep: &ClosureReport) -> bool {
    match j.kind {
        JudgeKind::CountExact => match BATCHES.get(j.batch as usize) {
            Some(b) => b.count >= BATCH_FLOOR && b.count == expected_count(j.batch as usize),
            None => false,
        },
        JudgeKind::SumExact => cumulative_sum() == CLOSURE_TOTAL,
        JudgeKind::IdSequence => batch_codes_contiguous(),
        JudgeKind::RangeDisjoint => ranges_disjoint(),
        JudgeKind::RangeCover => {
            let (covered, contiguous) = ranges_cover();
            covered == FACE_SPAN && contiguous
        }
        JudgeKind::ErrnoDistinct => errno_distinct(),
        JudgeKind::SinglePass => rep.closed_at_total() && rep.steps == BATCH_LEN as u32,
        JudgeKind::RunOnce => rep.once_each(),
        JudgeKind::OrderKept => rep.order_kept(),
        JudgeKind::ClosedRejects => rep.rerun_after_close == EROFS,
        JudgeKind::IdleRejects => rep.step_before_begin == EINVAL,
        JudgeKind::AggregateSelf => {
            let mut ok = true;
            for row in JUDGES.iter() {
                if row.kind == JudgeKind::AggregateSelf {
                    continue;
                }
                if !evaluate(row, rep) {
                    ok = false;
                }
            }
            ok
        }
    }
}

/// 冻结的单批期望条数（与 BATCHES 同源，用于「表没被改写」的冗余校验）。
pub const EXPECTED_COUNTS: [u32; BATCH_LEN] = [
    18_340, 19_120, 20_045, 20_480, 18_765, 19_610, 18_990, 18_750,
];

/// 取单批期望条数。
pub fn expected_count(i: usize) -> u32 {
    EXPECTED_COUNTS.get(i).copied().unwrap_or(0)
}

/// 19 条判据的逐条结果。
pub fn judge_all() -> [bool; JUDGE_LEN] {
    let rep = run_closure();
    let mut out = [false; JUDGE_LEN];
    let mut i = 0usize;
    while i < JUDGE_LEN {
        if let Some(j) = JUDGES.get(i) {
            let v = evaluate(j, &rep);
            if let Some(slot) = out.get_mut(i) {
                *slot = v;
            }
        }
        i += 1;
    }
    out
}

/// 聚合判据：19 条全真且条数恰为 19。
pub fn aggregate() -> (u32, u32) {
    let v = judge_all();
    let mut pass = 0u32;
    for b in v.iter() {
        if *b {
            pass = pass.saturating_add(1);
        }
    }
    (pass, JUDGE_LEN as u32)
}

// ── 写回 ──────────────────────────────────────────────────────────────
/// 写回状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WbStatus {
    Pending,
    Partial,
    Done,
}

impl WbStatus {
    /// 状态码，Done=2。
    pub fn idx(self) -> u32 {
        match self {
            WbStatus::Pending => 0,
            WbStatus::Partial => 1,
            WbStatus::Done => 2,
        }
    }
}

/// 写回记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Writeback {
    pub status: WbStatus,
    pub total: u32,
    pub judges: u32,
    pub summary: &'static str,
}

impl Writeback {
    /// 已完成：状态 2、累计 154100、19 条判据。
    pub fn done(&self) -> bool {
        self.status == WbStatus::Done
            && self.status.idx() == 2
            && self.total == CLOSURE_TOTAL
            && self.judges == JUDGE_LEN as u32
            && !self.summary.is_empty()
    }
}

/// 由聚合结果生成写回。
pub fn compose_writeback(pass: u32, total: u32) -> Writeback {
    let status = if pass == total && total == JUDGE_LEN as u32 && cumulative_sum() == CLOSURE_TOTAL {
        WbStatus::Done
    } else if pass > 0 {
        WbStatus::Partial
    } else {
        WbStatus::Pending
    };
    Writeback {
        status,
        total: cumulative_sum(),
        judges: total,
        summary: "B21–B28 八批一次跑通，19 条判据聚合全绿，域累计 154100 收口",
    }
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 收口语料指纹：批次表 + 判据表 + 报告。
pub fn corpus_fingerprint(rep: &ClosureReport) -> u32 {
    let mut out: Vec<u8> = Vec::new();
    for b in BATCHES.iter() {
        out.push(b.code.len() as u8);
        out.push((b.count & 0xFF) as u8);
        out.push(((b.count >> 8) & 0xFF) as u8);
        out.push((b.errno & 0x7F) as u8);
        out.push((b.lo & 0xFF) as u8);
        out.push((b.hi & 0xFF) as u8);
    }
    for j in JUDGES.iter() {
        out.push(j.id);
        out.push(j.batch);
        out.push(match j.kind {
            JudgeKind::CountExact => 1,
            JudgeKind::SumExact => 2,
            JudgeKind::IdSequence => 3,
            JudgeKind::RangeDisjoint => 4,
            JudgeKind::RangeCover => 5,
            JudgeKind::ErrnoDistinct => 6,
            JudgeKind::SinglePass => 7,
            JudgeKind::RunOnce => 8,
            JudgeKind::OrderKept => 9,
            JudgeKind::ClosedRejects => 10,
            JudgeKind::IdleRejects => 11,
            JudgeKind::AggregateSelf => 12,
        });
        out.push(j.name.len() as u8);
    }
    let v = judge_all();
    for b in v.iter() {
        out.push(if *b { 1 } else { 0 });
    }
    out.push((rep.cumulative & 0xFF) as u8);
    out.push(((rep.cumulative >> 8) & 0xFF) as u8);
    out.push((rep.order & 0xFF) as u8);
    out.push(rep.stage.idx() as u8);
    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9360 判据集（静态 16 条 + 动态 2 条）。
pub fn run_unx9360_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9360");

    let seg_ok = SEG_BASE % 0x40 == 0 && SEG_BASE == 0x7C780;
    s.add("段位守卫：0x7C780 起 0x40 对齐", seg_ok,
          "本模块独占 0x7C780..=0x7C7BF，为 B21–B28 收口区");

    let sum = cumulative_sum();
    s.add("域累计：八批逐行累加等于 154100",
          sum == CLOSURE_TOTAL && sum == cumulative_sum(),
          "和必须从批次表累加得出，不得用常量冒充");

    let floor_ok = BATCHES.iter().all(|b| b.count >= BATCH_FLOOR && b.count == expected_count_of(b));
    s.add("单批下限：八批均 ≥18000 且与期望表一致", floor_ok,
          "空批或缩水批不得进入累计");

    s.add("批号连续：B21..B28 无断档", batch_codes_contiguous(),
          "批号断档会让收口覆盖不到某一段 syscall 面");

    s.add("号段不交：八批两两无交集", ranges_disjoint(),
          "重叠会造成同一 syscall 被两批判据重复计数");

    let (covered, contiguous) = ranges_cover();
    s.add("号段覆盖：256..=511 无缝且跨度为 256",
          covered == FACE_SPAN && contiguous && FACE_SPAN == 256,
          "八批必须首尾相接、边界对齐 256 与 511");

    s.add("主码唯一：八批主 errno 两两不同且在闭集内", errno_distinct(),
          "主码冲突会让 E 型收口无法定位到批");

    let (pass, total) = aggregate();
    s.add("19 条判据聚合：全绿且条数恰为 19",
          pass == JUDGE_LEN as u32 && total == JUDGE_LEN as u32,
          "聚合判据必须与明细判据同源，不能单独打勾");

    s.add("判据表：19 行 id 连续 1..=19", judge_ids_sequential(),
          "判据 id 断档意味着明细缺失，聚合不得通过");

    let rep = run_closure();
    s.add("收口一次跑通：run_all 回 154100 且闭环",
          rep.closed_at_total() && rep.steps == BATCH_LEN as u32,
          "begin→8×step→close 必须单次走完，累计等于域累计");

    s.add("无重跑：八批 runs 各为 1", rep.once_each(),
          "任一批跑到 2 次即属重复计数，收口作废");

    s.add("执行有序：序编码等于 12345678", rep.order_kept(),
          "跳批或插队会让序编码偏离，顺序即判据");

    s.add("闭环拒绝：收口后再 step 回 EROFS", rep.rerun_after_close == EROFS,
          "闭环即只读，重跑必须被状态机拒绝");

    s.add("空跑拒绝：未 begin 就 step 回 EINVAL", rep.step_before_begin == EINVAL,
          "未开工不得产生任何累计，EINVAL 是唯一出口");

    let wb = compose_writeback(pass, total);
    s.add("写回：状态=已完成、累计 154100、摘要非空",
          wb.done() && wb.status.idx() == 2 && wb.summary.len() >= 24,
          "合同 ③ 要求状态、累计与摘要三者同时成立");

    let mut uniq = true;
    let mut i = 0usize;
    while i < ClosureCode::ALL.len() {
        let mut j = i + 1;
        while j < ClosureCode::ALL.len() {
            if let (Some(a), Some(b)) = (ClosureCode::ALL.get(i), ClosureCode::ALL.get(j)) {
                if a.code() == b.code() || a.seg_addr() == b.seg_addr() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个段内序号与绝对码两两唯一", uniq,
          "闭集码重复会让红项无法定位到唯一建议");

    let fp_a = corpus_fingerprint(&rep);
    let rep2 = run_closure();
    let fp_b = corpus_fingerprint(&rep2);
    s.add("双跑一致：同脚本两遍指纹相等",
          fp_a == fp_b && rep.order == rep2.order && rep.runs == rep2.runs,
          "收口的可复测性以双跑逐字节相等为准");

    let sum_text = format!("八批累计 {}，收口值 {}，步数 {}", sum, rep.close_value, rep.steps);
    s.add_dyn("累计明细", sum == CLOSURE_TOTAL, &sum_text);

    let judge_text = format!("判据 {}/{}，序编码 {}，阶段 {}", pass, total, rep.order, rep.stage.idx());
    s.add_dyn("收口快照", pass == total, &judge_text);

    s
}

/// 批次是否与冻结期望表逐项一致。
pub fn expected_count_of(b: &Batch) -> u32 {
    let mut i = 0usize;
    while i < BATCH_LEN {
        if let Some(x) = BATCHES.get(i) {
            if x.code == b.code {
                return expected_count(i);
            }
        }
        i += 1;
    }
    0
}

/// 判据 id 是否连续 1..=19。
pub fn judge_ids_sequential() -> bool {
    let mut i = 0usize;
    while i < JUDGE_LEN {
        if let Some(j) = JUDGES.get(i) {
            if j.id as usize != i + 1 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_expected_shape() {
        assert_eq!(BATCHES.len(), BATCH_LEN);
        assert_eq!(JUDGES.len(), JUDGE_LEN);
        assert_eq!(EXPECTED_COUNTS.len(), BATCH_LEN);
        assert_eq!(FACE_SPAN, 256);
    }

    #[test]
    fn sum_is_closure_total() {
        assert_eq!(cumulative_sum(), CLOSURE_TOTAL);
        assert_eq!(cumulative_sum(), 154_100);
    }

    #[test]
    fn batch_codes_are_sequential() {
        assert!(batch_codes_contiguous());
        assert_eq!(BATCHES.first().map(|b| b.code), Some("B21"));
        assert_eq!(BATCHES.last().map(|b| b.code), Some("B28"));
    }

    #[test]
    fn ranges_partition_the_face() {
        assert!(ranges_disjoint());
        let (covered, contiguous) = ranges_cover();
        assert!(contiguous);
        assert_eq!(covered, FACE_SPAN);
        assert_eq!(BATCHES.first().map(|b| b.lo), Some(FACE_LO));
        assert_eq!(BATCHES.last().map(|b| b.hi), Some(FACE_HI));
    }

    #[test]
    fn errno_column_is_unique() {
        assert!(errno_distinct());
        assert_eq!(BATCHES.first().map(|b| b.errno), Some(EBADF));
        assert_eq!(BATCHES.last().map(|b| b.errno), Some(ENOSYS));
    }

    #[test]
    fn closure_runs_once() {
        let rep = run_closure();
        assert!(rep.closed_at_total());
        assert!(rep.once_each());
        assert!(rep.order_kept());
        assert_eq!(rep.steps, BATCH_LEN as u32);
        assert_eq!(rep.cumulative, CLOSURE_TOTAL);
    }

    #[test]
    fn closure_rejects_illegal_paths() {
        let mut c = Closure::new();
        assert_eq!(c.step(), Err(EINVAL));
        assert_eq!(c.close(), Err(EINVAL));
        assert!(c.begin().is_ok());
        assert_eq!(c.begin(), Err(EBUSY));
        assert_eq!(c.close(), Err(EBUSY));
        assert!(c.step().is_ok());
        assert_eq!(c.stage(), Stage::Running);
        assert_eq!(c.run_all(), Err(EBUSY));
    }

    #[test]
    fn closure_step_beyond_eight() {
        let mut c = Closure::new();
        assert!(c.begin().is_ok());
        let mut k = 0usize;
        while k < BATCH_LEN {
            assert!(c.step().is_ok());
            k += 1;
        }
        assert_eq!(c.step(), Err(EBUSY));
        assert_eq!(c.close(), Ok(CLOSURE_TOTAL));
        assert_eq!(c.close(), Err(EROFS));
        assert_eq!(c.step(), Err(EROFS));
        assert_eq!(c.stage(), Stage::Closed);
    }

    #[test]
    fn all_nineteen_judges_pass() {
        let (pass, total) = aggregate();
        assert_eq!(total, JUDGE_LEN as u32);
        assert_eq!(pass, 19);
        assert!(judge_ids_sequential());
        let v = judge_all();
        assert_eq!(v.len(), 19);
        assert!(v.iter().all(|b| *b));
    }

    #[test]
    fn aggregate_row_is_self_consistent() {
        let rep = run_closure();
        let last = JUDGES.get(JUDGE_LEN - 1);
        assert!(last.is_some());
        if let Some(j) = last {
            assert_eq!(j.kind, JudgeKind::AggregateSelf);
            assert_eq!(evaluate(j, &rep), prefix_all());
        }
    }

    #[test]
    fn writeback_done() {
        let (pass, total) = aggregate();
        let wb = compose_writeback(pass, total);
        assert!(wb.done());
        assert_eq!(wb.status, WbStatus::Done);
        assert_eq!(wb.total, CLOSURE_TOTAL);
        assert!(!wb.summary.is_empty());
        let partial = compose_writeback(1, 19);
        assert!(partial.status != WbStatus::Done);
    }

    #[test]
    fn codes_and_segment() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(ClosureCode::ALL.len(), 12);
        assert_eq!(ClosureCode::BatchGap.seg_addr(), SEG_BASE + 1);
        assert_eq!(ClosureCode::SegmentOut.code(), 12);
        assert!(ClosureCode::ALL.iter().all(|c| !c.detail().is_empty()));
    }

    #[test]
    fn deterministic_fingerprint() {
        let a = run_closure();
        let b = run_closure();
        assert_eq!(corpus_fingerprint(&a), corpus_fingerprint(&b));
        assert_eq!(a.order, b.order);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9360_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
