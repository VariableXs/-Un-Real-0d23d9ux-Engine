//! UNX-F50029 布局埋点（编排/预览/输出三点）（0x42fc0 段）
//!
//! 锚点：UNX-F50029-J1 埋点 100% 覆盖；判据锚定 体验十三·补。
//!
//! 数据段：`SEG_BASE = 0x42fc0`，诊断码闭集 `0x42fc0 + 1..=12`。
//!
//! 本模块结构：
//! - 埋点计划：`Point`/`REQUIRED` 登记三阶段共 10 个必打点（编排 4、预览 3、
//!   输出 3）。计划是**只读常量表**，覆盖率的分子分母都由它导出，任何
//!   调试点增删都必须同步改表，否则覆盖率立刻失真；
//! - 状态机：`TraceLedger` 三阶段单向推进（编排 → 预览 → 输出），阶段
//!   切换前上一阶段必须满覆盖（`StageSkip`）；同一阶段内允许任意点序，
//!   但时间戳必须单调（`TimeReversal`）；关闭后只读（`Closed`）；
//! - 账本：定容 + 丢弃计数（`dropped`），容量满返回 `Capacity` 而不是静默
//!   吞点——埋点丢失会让覆盖率看起来 100% 而实际缺证据，必须显性化；
//! - 错误路径：空计划 / 未知点 / 阶段错配 / 重复点 / 阶段回退 / 容量满 /
//!   时间倒流 / 覆盖缺口 / 跳阶段 / 已关闭 / 空账本 / 码漂移；
//! - 注入核：`injection_core()` 5 类坏样本（重复、未知点、跳阶段、
//!   时间倒流、容量满）逐类断言被拒。
//!
//! 零 panic 纪律：全部入口 `Result`；`REQUIRED` 遍历用 `iter().find` 与
//! `get()`；测试块之外不做空解包、不触发断言。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x42fc0 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x42fc0;

/// 三阶段点数（与 `REQUIRED` 逐项一致，判据交叉核对）。
pub const ORCH_POINTS: usize = 4;
pub const PREVIEW_POINTS: usize = 3;
pub const OUTPUT_POINTS: usize = 3;

const E_EMPTY_PLAN: u16 = 1;
const E_UNKNOWN_POINT: u16 = 2;
const E_DUP_SPAN: u16 = 3;
const E_ORDER: u16 = 4;
const E_CAPACITY: u16 = 5;
const E_TIME: u16 = 6;
const E_MISSING: u16 = 7;
const E_SKIP: u16 = 8;
const E_CLOSED: u16 = 9;
const E_EMPTY_TRACE: u16 = 10;
const E_UNKNOWN_STAGE: u16 = 11;
const E_CODE: u16 = 12;

/// 埋点域错误十二型，`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceError {
    EmptyPlan,
    UnknownPoint,
    DuplicateSpan,
    OutOfOrder,
    Capacity,
    TimeReversal,
    MissingStage,
    StageSkip,
    Closed,
    EmptyTrace,
    UnknownStage,
    CodeDrift,
}

impl TraceError {
    fn ordinal(self) -> u16 {
        match self {
            TraceError::EmptyPlan => E_EMPTY_PLAN,
            TraceError::UnknownPoint => E_UNKNOWN_POINT,
            TraceError::DuplicateSpan => E_DUP_SPAN,
            TraceError::OutOfOrder => E_ORDER,
            TraceError::Capacity => E_CAPACITY,
            TraceError::TimeReversal => E_TIME,
            TraceError::MissingStage => E_MISSING,
            TraceError::StageSkip => E_SKIP,
            TraceError::Closed => E_CLOSED,
            TraceError::EmptyTrace => E_EMPTY_TRACE,
            TraceError::UnknownStage => E_UNKNOWN_STAGE,
            TraceError::CodeDrift => E_CODE,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            TraceError::EmptyPlan => "trace plan has no points",
            TraceError::UnknownPoint => "point not registered in plan",
            TraceError::DuplicateSpan => "point already emitted",
            TraceError::OutOfOrder => "stage moved backwards",
            TraceError::Capacity => "trace ledger at capacity",
            TraceError::TimeReversal => "timestamp decreased",
            TraceError::MissingStage => "stage coverage incomplete",
            TraceError::StageSkip => "stage advanced before completion",
            TraceError::Closed => "trace ledger closed",
            TraceError::EmptyTrace => "trace has no events",
            TraceError::UnknownStage => "point stage mismatch",
            TraceError::CodeDrift => "diagnostic code outside segment",
        }
    }
}

/// 全错误表（判据验证码值闭集）。
pub const ALL_ERRORS: [TraceError; 12] = [
    TraceError::EmptyPlan,
    TraceError::UnknownPoint,
    TraceError::DuplicateSpan,
    TraceError::OutOfOrder,
    TraceError::Capacity,
    TraceError::TimeReversal,
    TraceError::MissingStage,
    TraceError::StageSkip,
    TraceError::Closed,
    TraceError::EmptyTrace,
    TraceError::UnknownStage,
    TraceError::CodeDrift,
];

/// 三阶段：编排（拆活/分张/封签）、预览（格表/栅格/比对）、输出（队列/引擎/回执）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Orchestrate,
    Preview,
    Output,
}

impl Stage {
    pub fn code(self) -> u8 {
        match self {
            Stage::Orchestrate => 0,
            Stage::Preview => 1,
            Stage::Output => 2,
        }
    }

    pub fn from_code(v: u8) -> Result<Stage, TraceError> {
        match v {
            0 => Ok(Stage::Orchestrate),
            1 => Ok(Stage::Preview),
            2 => Ok(Stage::Output),
            _ => Err(TraceError::UnknownStage),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Orchestrate => "orchestrate",
            Stage::Preview => "preview",
            Stage::Output => "output",
        }
    }

    /// 该阶段的必打点数（与常量表核对，防止表与枚举漂移）。
    pub fn required_count(self) -> usize {
        match self {
            Stage::Orchestrate => ORCH_POINTS,
            Stage::Preview => PREVIEW_POINTS,
            Stage::Output => OUTPUT_POINTS,
        }
    }
}

/// 一个埋点点位：阶段 + 稳定键。键是 `&'static str`，不参与拼接，
/// 避免运行期构造键导致「看起来覆盖了、其实对不上」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub stage: Stage,
    pub key: &'static str,
}

const fn pt(stage: Stage, key: &'static str) -> Point {
    Point { stage, key }
}

/// 必打点清单（唯一真值）。判据 J1 的 100% 即「本表每项都有事件」。
pub const REQUIRED: [Point; 10] = [
    pt(Stage::Orchestrate, "job.admit"),
    pt(Stage::Orchestrate, "layout.compute"),
    pt(Stage::Orchestrate, "deal.split"),
    pt(Stage::Orchestrate, "sign.seal"),
    pt(Stage::Preview, "preview.cells"),
    pt(Stage::Preview, "preview.raster"),
    pt(Stage::Preview, "preview.compare"),
    pt(Stage::Output, "output.spool"),
    pt(Stage::Output, "output.engine"),
    pt(Stage::Output, "output.receipt"),
];

/// 一条埋点事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    pub seq: u16,
    pub stage: Stage,
    pub key: &'static str,
    pub t_ms: u32,
    pub ok: bool,
}

/// 覆盖率快照。`per_stage` 与 `required` 同序（编排/预览/输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub per_stage: [u16; 3],
    pub required: [u16; 3],
    pub total: u16,
    pub required_total: u16,
}

impl Coverage {
    pub fn complete(&self) -> bool {
        self.total == self.required_total && self.required_total > 0
    }

    /// 万分比覆盖率（10000 = 100%）。用整数避免浮点判据抖动。
    pub fn permille(&self) -> u32 {
        if self.required_total == 0 {
            return 0;
        }
        (self.total as u32) * 10000 / self.required_total as u32
    }
}

/// 埋点账本：定容、阶段单调、关闭只读。
#[derive(Clone, Debug)]
pub struct TraceLedger {
    pub events: Vec<TraceEvent>,
    pub cap: usize,
    pub closed: bool,
    pub dropped: u16,
}

impl TraceLedger {
    pub fn new(cap: usize) -> TraceLedger {
        TraceLedger {
            events: Vec::new(),
            cap,
            closed: false,
            dropped: 0,
        }
    }

    /// 打点。错误优先级：关闭 > 容量 > 未知点 > 阶段错配 > 重复 > 时间 >
    /// 阶段顺序。优先级固定，注入核才能对每类坏样本断言唯一错误码。
    pub fn emit(
        &mut self,
        stage: Stage,
        key: &'static str,
        t_ms: u32,
        ok: bool,
    ) -> Result<u16, TraceError> {
        if self.closed {
            return Err(TraceError::Closed);
        }
        if self.events.len() >= self.cap {
            self.dropped = self.dropped.saturating_add(1);
            return Err(TraceError::Capacity);
        }
        let mut known = false;
        let mut stage_ok = false;
        let mut i = 0usize;
        while i < REQUIRED.len() {
            if let Some(p) = REQUIRED.get(i) {
                if p.key == key {
                    known = true;
                    if p.stage == stage {
                        stage_ok = true;
                    }
                }
            }
            i += 1;
        }
        if !known {
            return Err(TraceError::UnknownPoint);
        }
        if !stage_ok {
            return Err(TraceError::UnknownStage);
        }
        if let Some(last) = self.events.last() {
            if t_ms < last.t_ms {
                return Err(TraceError::TimeReversal);
            }
            let s = stage.code();
            let l = last.stage.code();
            if s < l {
                return Err(TraceError::OutOfOrder);
            }
            if s > l {
                if self.covered(last.stage) < last.stage.required_count() {
                    return Err(TraceError::StageSkip);
                }
                if s != l + 1 {
                    return Err(TraceError::StageSkip);
                }
            }
        } else if stage.code() != 0 {
            // 空账本只允许编排阶段起步，中途接管的埋点没有前序证据。
            return Err(TraceError::StageSkip);
        }
        // 重复判定放在阶段顺序之后：同阶段重复是「重复」，跨阶段重复
        // 本质是阶段回退，错误码必须落在 OutOfOrder 而非 DuplicateSpan。
        let mut j = 0usize;
        while j < self.events.len() {
            if let Some(e) = self.events.get(j) {
                if e.key == key {
                    return Err(TraceError::DuplicateSpan);
                }
            }
            j += 1;
        }
        let seq = (self.events.len() as u16).saturating_add(1);
        self.events.push(TraceEvent {
            seq,
            stage,
            key,
            t_ms,
            ok,
        });
        Ok(seq)
    }

    /// 某阶段已覆盖点数。
    pub fn covered(&self, stage: Stage) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < REQUIRED.len() {
            if let Some(p) = REQUIRED.get(i) {
                if p.stage == stage && self.has(p.key) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 某键是否已打点。
    pub fn has(&self, key: &'static str) -> bool {
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                if e.key == key {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 覆盖率：分子按「点在账本里」计，分母按常量表计。
    pub fn coverage(&self) -> Coverage {
        let mut per = [0u16; 3];
        let mut req = [0u16; 3];
        let mut i = 0usize;
        while i < REQUIRED.len() {
            if let Some(p) = REQUIRED.get(i) {
                let idx = p.stage.code() as usize;
                if idx < 3 {
                    req[idx] = req[idx].saturating_add(1);
                    if self.has(p.key) {
                        per[idx] = per[idx].saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        let total = per[0].saturating_add(per[1]).saturating_add(per[2]);
        let required_total = req[0].saturating_add(req[1]).saturating_add(req[2]);
        Coverage {
            per_stage: per,
            required: req,
            total,
            required_total,
        }
    }

    /// 缺失点位（按常量表顺序），供诊断文本与判据共用。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < REQUIRED.len() {
            if let Some(p) = REQUIRED.get(i) {
                if !self.has(p.key) {
                    out.push(p.key);
                }
            }
            i += 1;
        }
        out
    }

    /// 收尾校验：非空、序号连续、阶段单调、三阶段各满覆盖。
    pub fn verify(&self) -> Result<(), TraceError> {
        if self.events.is_empty() {
            return Err(TraceError::EmptyTrace);
        }
        let mut i = 0usize;
        while i < self.events.len() {
            let e = match self.events.get(i) {
                Some(v) => *v,
                None => return Err(TraceError::EmptyTrace),
            };
            if e.seq as usize != i + 1 {
                return Err(TraceError::OutOfOrder);
            }
            if i > 0 {
                if let Some(prev) = self.events.get(i - 1) {
                    if e.stage.code() < prev.stage.code() {
                        return Err(TraceError::OutOfOrder);
                    }
                    if e.t_ms < prev.t_ms {
                        return Err(TraceError::TimeReversal);
                    }
                }
            }
            i += 1;
        }
        let cov = self.coverage();
        let mut k = 0usize;
        while k < 3 {
            if cov.per_stage[k] != cov.required[k] {
                return Err(TraceError::MissingStage);
            }
            k += 1;
        }
        Ok(())
    }

    /// 关闭账本；关闭后任何打点都是 Closed（不能靠重开洗掉缺口）。
    pub fn close(&mut self) {
        self.closed = true;
    }
}

/// 跑一遍完整会话（可跳过某点，用于缺覆盖判据）。时间戳按 10ms 步进，
/// 保证确定性且严格递增。
pub fn run_session(skip: Option<&'static str>) -> Result<TraceLedger, TraceError> {
    let mut led = TraceLedger::new(32);
    let mut i = 0usize;
    while i < REQUIRED.len() {
        if let Some(p) = REQUIRED.get(i) {
            if Some(p.key) != skip {
                let t = 1000u32.saturating_add((i as u32).saturating_mul(10));
                led.emit(p.stage, p.key, t, true)?;
            }
        }
        i += 1;
    }
    Ok(led)
}

// ---------------------------------------------------------------------------
// 注入核：五类坏样本
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let total = 5usize;
    let mut caught = 0usize;

    // 1. 重复打点。
    let mut a = TraceLedger::new(8);
    if a.emit(Stage::Orchestrate, "job.admit", 10, true).is_ok()
        && a.emit(Stage::Orchestrate, "job.admit", 20, true).err()
            == Some(TraceError::DuplicateSpan)
    {
        caught += 1;
    }
    // 2. 未登记点位。
    let mut b = TraceLedger::new(8);
    if b.emit(Stage::Orchestrate, "job.ghost", 10, true).err() == Some(TraceError::UnknownPoint) {
        caught += 1;
    }
    // 3. 跳阶段：编排只打了 1/4 就进预览。
    let mut c = TraceLedger::new(8);
    if c.emit(Stage::Orchestrate, "job.admit", 10, true).is_ok()
        && c.emit(Stage::Preview, "preview.cells", 20, true).err() == Some(TraceError::StageSkip)
    {
        caught += 1;
    }
    // 4. 时间倒流。
    let mut d = TraceLedger::new(8);
    if d.emit(Stage::Orchestrate, "job.admit", 100, true).is_ok()
        && d.emit(Stage::Orchestrate, "layout.compute", 50, true).err()
            == Some(TraceError::TimeReversal)
    {
        caught += 1;
    }
    // 5. 容量满：cap=2 时第三点必须拒绝并记账。
    let mut e = TraceLedger::new(2);
    let mut ok_all = e.emit(Stage::Orchestrate, "job.admit", 10, true).is_ok();
    ok_all = ok_all && e.emit(Stage::Orchestrate, "layout.compute", 20, true).is_ok();
    if ok_all
        && e.emit(Stage::Orchestrate, "deal.split", 30, true).err() == Some(TraceError::Capacity)
        && e.dropped == 1
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50029_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50029");

    // J1-1 埋点 100% 覆盖：10/10，分阶段 4/3/3，verify 通过。
    let mut full_ok = false;
    let mut full_det = String::new();
    let mut full_cov = Coverage {
        per_stage: [0; 3],
        required: [0; 3],
        total: 0,
        required_total: 0,
    };
    match run_session(None) {
        Ok(led) => {
            let cov = led.coverage();
            full_cov = cov;
            full_ok = cov.complete()
                && cov.total == 10
                && cov.per_stage == [4, 3, 3]
                && cov.permille() == 10000
                && led.missing().is_empty()
                && led.verify().is_ok()
                && led.dropped == 0;
            full_det = format!(
                "{}/{} {}:{}:{}",
                cov.total, cov.required_total, cov.per_stage[0], cov.per_stage[1], cov.per_stage[2]
            );
        }
        Err(_) => {}
    }
    s.add_dyn("50029-J1-埋点100%覆盖", full_ok, &full_det);

    // J1-2 计划表与阶段枚举互核：10 点、分阶段计数与 required_count 一致。
    let ok = REQUIRED.len() == 10
        && Stage::Orchestrate.required_count() == ORCH_POINTS
        && Stage::Preview.required_count() == PREVIEW_POINTS
        && Stage::Output.required_count() == OUTPUT_POINTS
        && ORCH_POINTS + PREVIEW_POINTS + OUTPUT_POINTS == REQUIRED.len();
    s.add("50029-J1-计划表自洽", ok, if ok { "" } else { "点数与枚举漂移" });

    // J1-3 阶段顺序单调 + 序号 1..10 连续。
    let mut order_ok = false;
    if let Ok(led) = run_session(None) {
        let mut mono = true;
        let mut seq_ok = true;
        let mut i = 0usize;
        while i < led.events.len() {
            if let Some(e) = led.events.get(i) {
                if e.seq as usize != i + 1 {
                    seq_ok = false;
                }
                if i > 0 {
                    if let Some(prev) = led.events.get(i - 1) {
                        if e.stage.code() < prev.stage.code() {
                            mono = false;
                        }
                    }
                }
            }
            i += 1;
        }
        order_ok = mono && seq_ok && led.events.len() == 10;
    }
    s.add("50029-J1-阶段单调", order_ok, if order_ok { "" } else { "阶段回退/序号断裂" });

    // J1-4 覆盖可复算：同会话两次跑，事件逐项相同、覆盖率相同。
    let mut det_ok = false;
    if let (Ok(a), Ok(b)) = (run_session(None), run_session(None)) {
        det_ok = a.events == b.events && a.coverage() == b.coverage();
    }
    s.add("50029-J1-覆盖可复算", det_ok, if det_ok { "" } else { "会话不可复现" });

    // J1-5 覆盖分母来自计划表（不是账本长度）：空账本分母仍为 10。
    let empty = TraceLedger::new(4);
    let cov = empty.coverage();
    let ok = cov.total == 0 && cov.required_total == 10 && cov.permille() == 0 && !cov.complete();
    s.add("50029-J1-分母为计划表", ok, if ok { "" } else { "覆盖率分母错误" });

    // J1-6 重复点拒绝，码值 = 基址 + 3。
    let mut a = TraceLedger::new(8);
    let mut dup_ok = false;
    if a.emit(Stage::Orchestrate, "job.admit", 10, true).is_ok() {
        dup_ok = a.emit(Stage::Orchestrate, "job.admit", 20, true).err()
            == Some(TraceError::DuplicateSpan)
            && TraceError::DuplicateSpan.code() == SEG_BASE + 3;
    }
    s.add("50029-J1-重复点拒绝", dup_ok, if dup_ok { "" } else { "重复点被吞" });

    // J1-7 未知点 / 阶段错配分码。
    let mut b = TraceLedger::new(8);
    let unknown = b.emit(Stage::Orchestrate, "job.ghost", 10, true).err()
        == Some(TraceError::UnknownPoint);
    let mut c = TraceLedger::new(8);
    let mismatch = c.emit(Stage::Preview, "job.admit", 10, true).err()
        == Some(TraceError::UnknownStage);
    s.add("50029-J1-未知点与错配", unknown && mismatch, if unknown && mismatch { "" } else { "点位错误未分码" });

    // J1-8 阶段回退拒绝：编排→预览→再回编排必须 OutOfOrder（跨阶段重复
    // 归入回退，而不是重复点）。
    let mut e = TraceLedger::new(16);
    let mut i = 0usize;
    while i < 7 {
        if let Some(p) = REQUIRED.get(i) {
            if e.emit(p.stage, p.key, 100 + i as u32, true).is_err() {
                break;
            }
        }
        i += 1;
    }
    let back_ok = e.emit(Stage::Orchestrate, "job.admit", 900, true).err()
        == Some(TraceError::OutOfOrder);
    s.add("50029-J1-阶段回退拒绝", back_ok, if back_ok { "" } else { "阶段回退未拒绝" });

    // J1-9 跳阶段拒绝：编排未满就进预览。
    let mut e = TraceLedger::new(8);
    let mut skip_ok = false;
    if e.emit(Stage::Orchestrate, "job.admit", 10, true).is_ok() {
        skip_ok = e.emit(Stage::Preview, "preview.cells", 20, true).err()
            == Some(TraceError::StageSkip);
    }
    s.add("50029-J1-跳阶段拒绝", skip_ok, if skip_ok { "" } else { "跳阶段被放行" });

    // J1-10 时间倒流拒绝。
    let mut f = TraceLedger::new(8);
    let mut time_ok = false;
    if f.emit(Stage::Orchestrate, "job.admit", 100, true).is_ok() {
        time_ok = f.emit(Stage::Orchestrate, "layout.compute", 99, true).err()
            == Some(TraceError::TimeReversal);
    }
    s.add("50029-J1-时间倒流拒绝", time_ok, if time_ok { "" } else { "时间倒流被放行" });

    // J1-11 容量满显式拒绝并记账，不静默丢点。
    let mut g = TraceLedger::new(2);
    let mut cap_ok = g.emit(Stage::Orchestrate, "job.admit", 10, true).is_ok();
    cap_ok = cap_ok && g.emit(Stage::Orchestrate, "layout.compute", 20, true).is_ok();
    cap_ok = cap_ok
        && g.emit(Stage::Orchestrate, "deal.split", 30, true).err() == Some(TraceError::Capacity)
        && g.dropped == 1
        && g.events.len() == 2;
    s.add("50029-J1-容量满记账", cap_ok, if cap_ok { "" } else { "容量满未显式拒绝" });

    // J1-12 关闭后只读：close 之后任何点都是 Closed。
    let mut closed_ok = false;
    if let Ok(mut led) = run_session(None) {
        led.close();
        closed_ok = led.emit(Stage::Output, "output.receipt", 9000, true).err()
            == Some(TraceError::Closed);
    }
    s.add("50029-J1-关闭只读", closed_ok, if closed_ok { "" } else { "关闭后仍可写" });

    // J1-13 缺一点必须被覆盖率与 verify 同时抓出（非恒绿对照）。
    // 跳过输出阶段末点：前两阶段仍满覆盖，账本可正常收尾——这正是
    // 「覆盖率 9/10 但会话合法」的缺口形态（跳中间点会被 StageSkip 拦下，
    // 那是另一条判据）。
    let mut gap_ok = false;
    let mut gap_det = String::new();
    if let Ok(led) = run_session(Some("output.receipt")) {
        let cov = led.coverage();
        let miss = led.missing();
        gap_ok = cov.total == 9
            && cov.per_stage == [4, 3, 2]
            && !cov.complete()
            && miss.len() == 1
            && miss.get(0) == Some(&"output.receipt")
            && led.verify().err() == Some(TraceError::MissingStage);
        gap_det = format!("total={} miss={}", cov.total, miss.len());
    }
    s.add_dyn("50029-J1-缺覆盖必抓", gap_ok, &gap_det);

    // J1-14 空账本 verify 拒绝（与「缺覆盖」区分：EmptyTrace 不是 MissingStage）。
    let blank = TraceLedger::new(4);
    let ok = blank.verify().err() == Some(TraceError::EmptyTrace)
        && TraceError::EmptyTrace.code() == SEG_BASE + 10;
    s.add("50029-J1-空账本拒绝", ok, if ok { "" } else { "空账本未分码" });

    // J1-15 错误码闭集：12 条，码值 = 基址 + 序号且唯一。
    let mut codes_ok = ALL_ERRORS.len() == 12;
    let mut seen = [false; 13];
    let mut i = 0usize;
    while i < ALL_ERRORS.len() {
        if let Some(e) = ALL_ERRORS.get(i) {
            let c = e.code();
            if c != SEG_BASE + (i as u32) + 1 {
                codes_ok = false;
            }
            let off = (c - SEG_BASE) as usize;
            if off < seen.len() {
                if seen[off] {
                    codes_ok = false;
                }
                seen[off] = true;
            } else {
                codes_ok = false;
            }
        }
        i += 1;
    }
    s.add("50029-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    // J1-16 注入核 5 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50029-J1-注入必抓", ok, &det);

    // J1-17 覆盖率与事件数一致（多点重复不打，分子不会超分母）。
    let ok = full_cov.required_total == 10
        && full_cov.total <= full_cov.required_total
        && full_cov.per_stage[0] <= 4;
    s.add("50029-J1-覆盖不超分母", ok, if ok { "" } else { "覆盖率超过 100%" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50029_checks_green() {
        let set = run_unx50029_checks();
        assert!(set.all_passed(), "UNX-F50029 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50029_full_session_covers_plan() {
        match run_session(None) {
            Ok(led) => {
                assert_eq!(led.events.len(), REQUIRED.len());
                assert!(led.verify().is_ok());
                assert_eq!(led.coverage().permille(), 10000);
            }
            Err(e) => panic!("session failed: {:?}", e.code()),
        }
    }
}
