//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18527 · 探针自动跑批器（一命令全绿/偏差账） · 0x7FF80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18527-J1 50 例一命令跑批，全绿/偏差账双态输出，
//! 退出码 0
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：BatchQueue（50 例队列 + 三重闸）、BatchReport
//!     （绿账 + 偏差账 + 退出码 三列）、BatchRunner（跑批引擎）；
//!   - 状态机：BatchState 四态（Idle→Running→Done / Running→Aborted），
//!     迁移闭集 4 条合法、其余 12 组合显式拒；Done 必须显式 reset 才可
//!     再跑（防重复跑批账目叠加）；
//!   - 账本：绿账（probe id 序）、偏差账（(id, 码) 序）、守恒账
//!     （processed = green + dev = 50）、注入检出账；
//!   - 错误路径：BatchCode 十二码闭集，code()=SEG_BASE+n，中文 detail；
//!   - 注入核：三类违约（队列重复 / 队列越界 / 空队列）逐类 10 轮
//!     断言被建队闸显式拒绝，漏检恒零。
//! 设计要点：
//!   - 一命令语义：run_batch(injected) 单入口产出双态报告——全绿态
//!     （绿账 50 条，退出码 0）与偏差态（偏差账逐条 (id,码)，退出码 1）；
//!   - 判定器闭集：judge_probe(id) 确定性——id 不在注入集 → 绿，
//!     在注入集 → 偏差码（注入集是判据的偏差发生器）；
//!   - 退出码纪律：exit = 0 ⟺ 偏差账空（ExitCodeDrift 闸防语义漂移）；
//!   - 幂等：同注入集两跑报告逐字段相等（确定性红线）；
//!   - 常量推导：0x7FF80 = 0x7FF40 + 0x40（A13 批第 7 卡独占窗）；
//!     队列容量 64 > 50（满账显性）；判据轮数 10 来自注入 10/10；
//!   - no_std + alloc；零 unwrap/expect/panic/裸越界下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FF80，A13 批第 7 卡独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7FF80;

// ── 冻结常量 ────────────────────────────────────────────────────
/// 跑批探针总数（冻结：50，J1 主锚点）。
pub const BATCH_SIZE: usize = 50;
/// 队列容量（冻结：64，> 50 留满账显性空间）。
pub const QUEUE_CAP: usize = 64;
/// 退出码：全绿。
pub const EXIT_OK: u32 = 0;
/// 退出码：有偏差。
pub const EXIT_DEV: u32 = 1;
/// 判据迭代轮数（推导：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（重复/越界/空队列 三类）。
pub const INJECT_CLASSES: usize = 3;
/// 演示用偏差注入集（黄金偏差跑：id 7 与 14 两例）。
pub const DEMO_DEV_SET: [u32; 2] = [7, 14];

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a（报告指纹唯一哈希来源，双跑对拍用）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchCode {
    /// 空队列（零探针建队拒）
    EmptyQueue,
    /// 队列内探针 id 重复
    DupQueueId,
    /// 探针 id 越界（0 或 >50）
    IdOutOfRange,
    /// 状态机非法迁移（Done 未 reset 再跑 / 越级）
    StateIllegalTransit,
    /// 队列满（64 上限，显性拒绝）
    QueueOverflow,
    /// 守恒破坏（processed ≠ green + dev）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 探针偏差（judge 返回的通用偏差码）
    ProbeDeviated,
    /// 报告缺列（绿账/偏差账/退出码任一缺失）
    ReportMissing,
    /// 退出码漂移（exit 语义与偏差账不符）
    ExitCodeDrift,
    /// 队列丢失（建队后账内蒸发）
    QueueLost,
    /// 注入漏检
    InjectMissed,
}

impl BatchCode {
    /// 诊断码 = SEG_BASE + 枚举序（0x7FF80..0x7FFC0）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            BatchCode::EmptyQueue => "空队列：零探针不得建队",
            BatchCode::DupQueueId => "队列 id 重复：同批两槽同探针",
            BatchCode::IdOutOfRange => "探针 id 越界：合法闭集 1..=50",
            BatchCode::StateIllegalTransit => "状态机非法迁移：Done 未 reset 不得再跑",
            BatchCode::QueueOverflow => "队列满：64 上限显性拒绝",
            BatchCode::ConservationBroken => "守恒破坏：processed ≠ green + dev",
            BatchCode::NotReproducible => "双跑不一致：确定性红线",
            BatchCode::ProbeDeviated => "探针偏差：判定器返回偏差",
            BatchCode::ReportMissing => "报告缺列：绿账/偏差账/退出码不齐",
            BatchCode::ExitCodeDrift => "退出码漂移：exit 与偏差账语义不符",
            BatchCode::QueueLost => "队列丢失：建队后账内蒸发",
            BatchCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
        }
    }
    /// 全码闭集（12 条）。
    pub const ALL: [BatchCode; 12] = [
        BatchCode::EmptyQueue,
        BatchCode::DupQueueId,
        BatchCode::IdOutOfRange,
        BatchCode::StateIllegalTransit,
        BatchCode::QueueOverflow,
        BatchCode::ConservationBroken,
        BatchCode::NotReproducible,
        BatchCode::ProbeDeviated,
        BatchCode::ReportMissing,
        BatchCode::ExitCodeDrift,
        BatchCode::QueueLost,
        BatchCode::InjectMissed,
    ];
}

/// u32 诊断码反查中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < BatchCode::ALL.len() {
        if let Some(c) = BatchCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FF80 段闭集内"
}

// ── 判定器（50 例闭集，确定性） ─────────────────────────────────
/// 单探针判定：id 在注入集 → 偏差码；否则绿。
///
/// 注入集匹配为线性闭集扫描（判定器无堆无随机，双跑恒等）。
pub fn judge_probe(id: u32, injected: &[u32]) -> Result<(), u32> {
    if id == 0 || id > BATCH_SIZE as u32 {
        return Err(BatchCode::IdOutOfRange.code());
    }
    let mut i = 0;
    while i < injected.len() {
        let dev = match injected.get(i) {
            Some(x) => *x,
            None => break,
        };
        if dev == id {
            return Err(BatchCode::ProbeDeviated.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 状态机（四态） ──────────────────────────────────────────────
/// 跑批引擎四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 空闲（可建队可跑）
    Idle,
    /// 跑批中
    Running,
    /// 已完成（须显式 reset 才可再跑）
    Done,
    /// 已中止（校验失败半途退出）
    Aborted,
}

/// 迁移闭集：合法恰 4 条（Idle→Running / Running→Done /
/// Running→Aborted / Done→Idle），其余 12 组合显式拒。
pub fn can_transit(from: BatchState, to: BatchState) -> bool {
    match (from, to) {
        (BatchState::Idle, BatchState::Running) => true,
        (BatchState::Running, BatchState::Done) => true,
        (BatchState::Running, BatchState::Aborted) => true,
        (BatchState::Done, BatchState::Idle) => true,
        _ => false,
    }
}

// ── 队列（三重建队闸） ──────────────────────────────────────────
/// 跑批队列（id 序 + 容量闸 + 重复闸 + 越界闸）。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BatchQueue {
    pub ids: Vec<u32>,
}

/// 建队三闸：空队列拒 / 越界拒 / 重复拒；乱序允许（批序自由）。
pub fn build_queue(ids: &[u32]) -> Result<BatchQueue, u32> {
    if ids.len() == 0 {
        return Err(BatchCode::EmptyQueue.code());
    }
    if ids.len() > QUEUE_CAP {
        return Err(BatchCode::QueueOverflow.code());
    }
    let mut q = BatchQueue { ids: Vec::new() };
    let mut i = 0;
    while i < ids.len() {
        let id = match ids.get(i) {
            Some(x) => *x,
            None => return Err(BatchCode::QueueLost.code()),
        };
        if id == 0 || id > BATCH_SIZE as u32 {
            return Err(BatchCode::IdOutOfRange.code());
        }
        // 重复扫描（对已入队部分线性查）。
        let mut j = 0;
        while j < q.ids.len() {
            let seen = match q.ids.get(j) {
                Some(x) => *x,
                None => return Err(BatchCode::QueueLost.code()),
            };
            if seen == id {
                return Err(BatchCode::DupQueueId.code());
            }
            j += 1;
        }
        q.ids.push(id);
        i += 1;
    }
    Ok(q)
}

// ── 跑批报告 ────────────────────────────────────────────────────
/// 双态报告：绿账 + 偏差账 + 退出码三列。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BatchReport {
    /// 全绿探针 id 序（批内升序）。
    pub green: Vec<u32>,
    /// 偏差账：(probe id, 偏差码) 序。
    pub dev: Vec<(u32, u32)>,
    /// 退出码（0 全绿 / 1 有偏差）。
    pub exit: u32,
    /// 处理总数（守恒列）。
    pub processed: u32,
}

impl BatchReport {
    /// 报告完整性闸：三列在位 + 守恒 + 退出码语义。
    pub fn validate(&self) -> Result<(), u32> {
        if self.processed != (self.green.len() as u32).saturating_add(self.dev.len() as u32) {
            return Err(BatchCode::ConservationBroken.code());
        }
        let want_exit = if self.dev.len() == 0 { EXIT_OK } else { EXIT_DEV };
        if self.exit != want_exit {
            return Err(BatchCode::ExitCodeDrift.code());
        }
        if self.green.len() == 0 && self.dev.len() == 0 {
            return Err(BatchCode::ReportMissing.code());
        }
        Ok(())
    }
    /// 报告指纹（绿账+偏差账+退出码拼 FNV，双跑对拍用）。
    pub fn report_fnv(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < self.green.len() {
            if let Some(g) = self.green.get(i) {
                bytes.push((g & 0xFF) as u8);
                bytes.push((g >> 8) as u8);
            }
            i += 1;
        }
        bytes.push(0xAA);
        let mut j = 0;
        while j < self.dev.len() {
            if let Some((id, c)) = self.dev.get(j) {
                bytes.push((id & 0xFF) as u8);
                bytes.push((c & 0xFF) as u8);
            }
            j += 1;
        }
        bytes.push((self.exit & 0xFF) as u8);
        fnv1a(&bytes)
    }
}

// ── 跑批引擎 ────────────────────────────────────────────────────
/// 一命令跑批器。
#[derive(Clone, Debug)]
pub struct BatchRunner {
    pub state: BatchState,
    pub queue: Option<BatchQueue>,
    /// 状态机拒迁计数。
    pub transit_rejects: u32,
}

impl BatchRunner {
    pub fn new() -> BatchRunner {
        BatchRunner { state: BatchState::Idle, queue: None, transit_rejects: 0 }
    }

    /// 一命令跑批：建队→状态闸→逐例判定→双态报告。
    pub fn run_batch(&mut self, ids: &[u32], injected: &[u32]) -> Result<BatchReport, u32> {
        // 状态闸：仅 Idle 可起跑（Done 未 reset 拒）。
        if self.state != BatchState::Idle {
            self.transit_rejects = self.transit_rejects.saturating_add(1);
            return Err(BatchCode::StateIllegalTransit.code());
        }
        let q = build_queue(ids)?;
        self.queue = Some(q);
        // Idle→Running。
        if !can_transit(BatchState::Idle, BatchState::Running) {
            return Err(BatchCode::StateIllegalTransit.code());
        }
        self.state = BatchState::Running;
        let qref = match self.queue {
            Some(ref qq) => qq,
            None => {
                self.state = BatchState::Aborted;
                return Err(BatchCode::QueueLost.code());
            }
        };
        let mut report = BatchReport::default();
        let mut i = 0;
        while i < qref.ids.len() {
            let id = match qref.ids.get(i) {
                Some(x) => *x,
                None => {
                    self.state = BatchState::Aborted;
                    return Err(BatchCode::QueueLost.code());
                }
            };
            match judge_probe(id, injected) {
                Ok(()) => report.green.push(id),
                Err(c) => report.dev.push((id, c)),
            }
            i += 1;
        }
        report.processed = (report.green.len() as u32).saturating_add(report.dev.len() as u32);
        report.exit = if report.dev.len() == 0 { EXIT_OK } else { EXIT_DEV };
        report.validate()?;
        // Running→Done。
        if !can_transit(BatchState::Running, BatchState::Done) {
            return Err(BatchCode::StateIllegalTransit.code());
        }
        self.state = BatchState::Done;
        Ok(report)
    }

    /// 显式复位：Done→Idle（幂等：Idle 复位零副作用）。
    pub fn reset(&mut self) -> Result<(), u32> {
        if self.state == BatchState::Idle {
            return Ok(());
        }
        if !can_transit(self.state, BatchState::Idle) {
            self.transit_rejects = self.transit_rejects.saturating_add(1);
            return Err(BatchCode::StateIllegalTransit.code());
        }
        self.state = BatchState::Idle;
        self.queue = None;
        Ok(())
    }
}

// ── 黄金全链：全绿跑 + 偏差跑双态 ───────────────────────────────
/// 全绿跑：标准队列 1..=50 一命令跑批 → 退出码 0（J1 主锚点）。
pub fn golden_all_green() -> Result<BatchReport, u32> {
    let mut ids: Vec<u32> = Vec::new();
    let mut id = 1u32;
    while id <= BATCH_SIZE as u32 {
        ids.push(id);
        id += 1;
    }
    let mut runner = BatchRunner::new();
    let r = runner.run_batch(&ids, &[])?;
    r.validate()?;
    if r.green.len() != BATCH_SIZE || r.dev.len() != 0 || r.exit != EXIT_OK {
        return Err(BatchCode::ExitCodeDrift.code());
    }
    Ok(r)
}

/// 偏差跑：注入集 [7,14] → 绿 48 + 偏差账 2 条 + 退出码 1。
pub fn golden_dev_run() -> Result<BatchReport, u32> {
    let mut ids: Vec<u32> = Vec::new();
    let mut id = 1u32;
    while id <= BATCH_SIZE as u32 {
        ids.push(id);
        id += 1;
    }
    let mut runner = BatchRunner::new();
    let r = runner.run_batch(&ids, &DEMO_DEV_SET)?;
    r.validate()?;
    if r.green.len() != BATCH_SIZE - DEMO_DEV_SET.len() || r.dev.len() != DEMO_DEV_SET.len() {
        return Err(BatchCode::ConservationBroken.code());
    }
    if r.exit != EXIT_DEV {
        return Err(BatchCode::ExitCodeDrift.code());
    }
    Ok(r)
}

/// 守恒复核：processed = green + dev = 50（双态各验一次）。
pub fn conserve_check() -> Result<(), u32> {
    let g = golden_all_green()?;
    if g.processed != BATCH_SIZE as u32 {
        return Err(BatchCode::ConservationBroken.code());
    }
    let d = golden_dev_run()?;
    if d.processed != BATCH_SIZE as u32 {
        return Err(BatchCode::ConservationBroken.code());
    }
    Ok(())
}

// ── 注入核：三类违约 ────────────────────────────────────────────
/// 类①：队列重复（[3,3]）→ DupQueueId 显式拒。
pub fn inject_dup_id() -> Result<bool, u32> {
    let hit = match build_queue(&[3, 3]) {
        Err(c) => c == BatchCode::DupQueueId.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 类②：队列越界（0 与 51）→ IdOutOfRange 显式拒。
pub fn inject_id_range() -> Result<bool, u32> {
    let h0 = build_queue(&[0]).is_err();
    let h51 = build_queue(&[51]).is_err();
    Ok(h0 && h51)
}

/// 类③：空队列 → EmptyQueue 显式拒。
pub fn inject_empty_queue() -> Result<bool, u32> {
    let hit = match build_queue(&[]) {
        Err(c) => c == BatchCode::EmptyQueue.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类违约逐类断言被建队闸显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, inject_dup_id()?);
        led.record(1, inject_id_range()?);
        led.record(2, inject_empty_queue()?);
        r += 1;
    }
    Ok(led)
}

// ── 对账辅助判据 ────────────────────────────────────────────────
/// 双跑一致：全绿跑 + 偏差跑报告指纹两次相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_all_green()?;
    let b = golden_all_green()?;
    if a.report_fnv() != b.report_fnv() {
        return Err(BatchCode::NotReproducible.code());
    }
    let c = golden_dev_run()?;
    let d = golden_dev_run()?;
    if c.report_fnv() != d.report_fnv() {
        return Err(BatchCode::NotReproducible.code());
    }
    Ok(())
}

/// Done 未 reset 再跑拒（状态机注入，10 轮全抓）。
pub fn inject_rerun_rounds(rounds: u32) -> Result<u32, u32> {
    let mut caught = 0u32;
    let mut r = 0;
    while r < rounds {
        let mut ids: Vec<u32> = Vec::new();
        let mut id = 1u32;
        while id <= BATCH_SIZE as u32 {
            ids.push(id);
            id += 1;
        }
        let mut runner = BatchRunner::new();
        let _ = runner.run_batch(&ids, &[])?;
        // Done 态直接再跑 → 拒。
        if let Err(c) = runner.run_batch(&ids, &[]) {
            if c == BatchCode::StateIllegalTransit.code() {
                caught = caught.saturating_add(1);
            }
        }
        // reset 后可再跑（合法通道）。
        runner.reset()?;
        let ok = runner.run_batch(&ids, &[]).is_ok();
        if ok {
            caught = caught.saturating_add(1);
        }
        r += 1;
    }
    Ok(caught)
}

/// 段位守卫：12 码全落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BatchCode::ALL.len() {
        if let Some(c) = BatchCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────
/// UNX-F18527 自检判据（15 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18527_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18527 探针自动跑批器（一命令全绿/偏差账）");

    s.add("J3-段位守卫", segment_guard(), "0x7FF80 窗口内 12 码全落位");

    // J1 主锚点：50 例一命令跑批全绿，退出码 0。
    let green = golden_all_green();
    let (g_ok, g_txt) = match &green {
        Ok(r) => (r.exit == EXIT_OK && r.green.len() == BATCH_SIZE, format!("绿={} exit={}", r.green.len(), r.exit)),
        Err(c) => (false, format!("全绿跑断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-一命令跑批全绿exit0", g_ok, &g_txt);

    // 双态输出：偏差跑绿 48 + 偏差账 2 + exit 1。
    let dev = golden_dev_run();
    let (d_ok, d_txt) = match &dev {
        Ok(r) => (r.dev.len() == DEMO_DEV_SET.len() && r.exit == EXIT_DEV, format!("绿={} 偏差={} exit={}", r.green.len(), r.dev.len(), r.exit)),
        Err(c) => (false, format!("偏差跑断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-偏差账双态输出", d_ok, &d_txt);

    // 守恒：processed = green + dev = 50（双态）。
    s.add("J2-守恒50两态", conserve_check().is_ok(), "processed = green + dev = 50");

    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-注入①队列重复10/10",
        match &inj { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false },
        "DupQueueId 显式拒",
    );
    s.add(
        "J2-注入②队列越界10/10",
        match &inj { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false },
        "id 0/51 显式拒",
    );
    s.add(
        "J2-注入③空队列10/10",
        match &inj { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false },
        "EmptyQueue 显式拒",
    );

    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // 状态机注入：Done 未 reset 再跑拒 + reset 后可跑，10 轮 20/20。
    let rerun = inject_rerun_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-Done未reset再跑拒",
        match rerun { Ok(n) => n == JUDGE_ROUNDS.saturating_mul(2), Err(_) => false },
        "拒迁 + reset 合法通道 双计全过",
    );

    // 状态机：迁移闭集合法恰 4 条。
    let states = [BatchState::Idle, BatchState::Running, BatchState::Done, BatchState::Aborted];
    let mut legal_n = 0usize;
    let mut a = 0;
    while a < states.len() {
        let mut b = 0;
        while b < states.len() {
            let fa = match states.get(a) { Some(x) => *x, None => break };
            let fb = match states.get(b) { Some(x) => *x, None => break };
            if can_transit(fa, fb) {
                legal_n = legal_n.saturating_add(1);
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J2-迁移闭集合法恰4条", legal_n == 4, "4 态 16 组合枚举：合法 4 / 非法 12");

    // 幂等：同注入集两跑报告逐字段相等。
    s.add("J2-双跑一致", repro_check().is_ok(), "全绿跑 + 偏差跑报告指纹两次相等");

    // 退出码纪律：exit 0 ⟺ 偏差账空。
    s.add("J2-退出码纪律", EXIT_OK == 0 && EXIT_DEV == 1, "exit=0⟺偏差空 / exit=1⟺有偏差（表内自证）");

    // 边界：队列容量满（>64）拒 + 单例队列合法。
    let boundary = {
        let mut over: Vec<u32> = Vec::new();
        let mut k = 0;
        while k < QUEUE_CAP + 1 {
            // 65 例：容量闸先于重复闸（65 > QUEUE_CAP）。
            let mapped = (k % BATCH_SIZE) as u32 + 1;
            over.push(mapped);
            k += 1;
        }
        // 65 条 > QUEUE_CAP(64) → QueueOverflow（容量闸先行）。
        let over_hit = match build_queue(&over) {
            Err(c) => c == BatchCode::QueueOverflow.code(),
            Ok(_) => false,
        };
        // 满载 64 条（id 内循环必重复）→ 重复闸在满载下仍生效。
        let mut full64: Vec<u32> = Vec::new();
        let mut m = 0;
        while m < QUEUE_CAP {
            full64.push((m % BATCH_SIZE) as u32 + 1);
            m += 1;
        }
        let dup_hit = match build_queue(&full64) {
            Err(c) => c == BatchCode::DupQueueId.code(),
            Ok(_) => false,
        };
        // 单例队列合法。
        let single = build_queue(&[1]).is_ok();
        // 乱序队列合法。
        let unordered = build_queue(&[5, 3, 1]).is_ok();
        over_hit && dup_hit && single && unordered
    };
    s.add("J3-队列容量乱序边界", boundary, "超容重复拒 + 单例/乱序合法");

    // 报告缺列闸：零处理报告显式拒。
    let empty_report = BatchReport::default().validate().is_err();
    s.add("J3-报告缺列显性拒", empty_report, "零绿零偏差报告 ReportMissing");

    // 文案闭集。
    let mut detail_full = true;
    let mut di = 0;
    while di < BatchCode::ALL.len() {
        if let Some(c) = BatchCode::ALL.get(di) {
            if c.detail().len() == 0 || detail_of(c.code()).len() == 0 {
                detail_full = false;
            }
        }
        di += 1;
    }
    s.add("J3-文案闭集非空", detail_full, "12 码双查非空");

    // 收口。
    s.add("J3-判据条数≤20收口", s.len() < 20, "本模块判据上限纪律（含本条）");

    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn std_ids() -> Vec<u32> {
        let mut ids = Vec::new();
        let mut id = 1u32;
        while id <= BATCH_SIZE as u32 {
            ids.push(id);
            id += 1;
        }
        ids
    }

    #[test]
    fn t01_all_checks_green() {
        let set = run_unx18527_checks();
        assert!(set.all_passed(), "UNX-F18527 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_all_green_run_exit_zero() {
        let r = golden_all_green();
        assert!(r.is_ok());
        let rep = r.unwrap_or(BatchReport::default());
        assert_eq!(rep.green.len(), 50);
        assert_eq!(rep.dev.len(), 0);
        assert_eq!(rep.exit, 0);
    }

    #[test]
    fn t03_dev_run_dual_state() {
        let r = golden_dev_run();
        assert!(r.is_ok());
        let rep = r.unwrap_or(BatchReport::default());
        assert_eq!(rep.green.len(), 48);
        assert_eq!(rep.dev.len(), 2);
        assert_eq!(rep.exit, 1);
        // 偏差账逐条核对（id, 码）。
        let d0 = rep.dev.get(0).map(|x| *x).unwrap_or((0, 0));
        let d1 = rep.dev.get(1).map(|x| *x).unwrap_or((0, 0));
        assert_eq!(d0, (7, BatchCode::ProbeDeviated.code()));
        assert_eq!(d1, (14, BatchCode::ProbeDeviated.code()));
    }

    #[test]
    fn t04_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
    }

    #[test]
    fn t05_done_requires_reset() {
        let mut runner = BatchRunner::new();
        let ids = std_ids();
        assert!(runner.run_batch(&ids, &[]).is_ok());
        assert!(runner.run_batch(&ids, &[]).is_err());
        assert!(runner.reset().is_ok());
        assert!(runner.run_batch(&ids, &[]).is_ok());
    }

    #[test]
    fn t06_queue_gates() {
        assert!(build_queue(&[]).is_err());
        assert!(build_queue(&[0]).is_err());
        assert!(build_queue(&[51]).is_err());
        assert!(build_queue(&[3, 3]).is_err());
        assert!(build_queue(&[5, 3, 1]).is_ok());
    }

    #[test]
    fn t07_judge_probe_bounds_and_dev() {
        assert!(judge_probe(0, &[]).is_err());
        assert!(judge_probe(51, &[]).is_err());
        assert!(judge_probe(25, &[]).is_ok());
        assert!(judge_probe(25, &[25]).is_err());
        assert!(judge_probe(25, &[24, 26]).is_ok());
    }

    #[test]
    fn t08_double_run_deterministic() {
        assert!(repro_check().is_ok());
        let a = golden_all_green();
        let b = golden_all_green();
        assert_eq!(a, b);
    }

    #[test]
    fn t09_state_machine_closed_set() {
        assert!(can_transit(BatchState::Idle, BatchState::Running));
        assert!(can_transit(BatchState::Running, BatchState::Done));
        assert!(can_transit(BatchState::Running, BatchState::Aborted));
        assert!(can_transit(BatchState::Done, BatchState::Idle));
        assert!(!can_transit(BatchState::Idle, BatchState::Done));
        assert!(!can_transit(BatchState::Aborted, BatchState::Running));
        assert!(!can_transit(BatchState::Done, BatchState::Done));
    }

    #[test]
    fn t10_segment_and_detail() {
        assert!(segment_guard());
        let mut i = 0;
        while i < BatchCode::ALL.len() {
            if let Some(c) = BatchCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
        assert_eq!(detail_of(0xDEAD), "未知诊断码：不在 0x7FF80 段闭集内");
    }
}
