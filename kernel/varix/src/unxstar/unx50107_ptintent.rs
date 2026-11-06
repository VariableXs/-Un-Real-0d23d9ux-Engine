//! UNX-F50107 直通与色彩意图透传（Intent 声明透传不处理档）（0x43980 段）
//!
//! 锚点：UNX-F50107-J1 意图透传 ×2 档落账；判据锚定 §AI-63 §1 防重声明（B21 联签）。
//!
//! 直通路径的纪律：渲染意图（rendering intent）在 PDF 页组 `/Intent` 与作业
//! 票据里只是一个**声明**——直通时不解释、不映射、不补偿，原值搬进打印流。
//! 任何「顺手做一次处理」都会让下游 RIP 与预览不一致，所以本模块把「未处理」
//! 做成可审计的落账字段（`emitted` 必须逐位等于声明线值），而不是口头约定。
//!
//! 数据段：`SEG_BASE = 0x43980`；诊断码闭集 = `SEG_BASE + 1..=8`
//! （见 `PtIntentError`，8 个码值全部在 `injection_core` 里被真实构造）。
//!
//! 结构清单：
//! * `ColorIntent` —— 四档渲染意图的**声明值**（只搬运，不映射）；
//! * `DeclMode` / `DeclChannel` —— 两档（设备默认 / 显式）与两路声明通道；
//! * `JobPhase` + `JobSlot` —— 每作业声明状态机 `Open→Declared→Sealed`；
//! * `IntentLedger` —— 定容声明账本，`(job,page,channel)` 防重 + 序列号单调；
//! * `injection_core` —— 9 类坏声明注入，逐类断言诊断码。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（本模块独占 0x43980）。
pub const SEG_BASE: u32 = 0x43980;

pub const E_EMPTY: u16 = 1;
pub const E_DUP: u16 = 2;
pub const E_UNKNOWN: u16 = 3;
pub const E_CAPACITY: u16 = 4;
pub const E_SEALED: u16 = 5;
pub const E_PAGE: u16 = 6;
pub const E_DRIFT: u16 = 7;
pub const E_NOJOB: u16 = 8;

/// 意图透传错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtIntentError {
    /// 空声明批（票据里一条帧都没有）。
    Empty,
    /// 同一 `(job,page,channel)` 重复声明——§1 防重声明主轴。
    Dup,
    /// 意图码 / 通道码不在闭集内。
    Unknown,
    /// 账本或作业槽已满（显式拒绝，不静默丢帧）。
    Capacity,
    /// 作业已封板，后到的声明一律拒绝。
    Sealed,
    /// 页锚或作业锚越界（必须以 1 起算）。
    Page,
    /// 声明值与被写进打印流的线值不一致（说明中间被处理过）。
    Drift,
    /// 对未登记作业封板。
    NoJob,
}

impl PtIntentError {
    /// 段内偏移（1..=8）。
    pub fn n(self) -> u16 {
        match self {
            PtIntentError::Empty => E_EMPTY,
            PtIntentError::Dup => E_DUP,
            PtIntentError::Unknown => E_UNKNOWN,
            PtIntentError::Capacity => E_CAPACITY,
            PtIntentError::Sealed => E_SEALED,
            PtIntentError::Page => E_PAGE,
            PtIntentError::Drift => E_DRIFT,
            PtIntentError::NoJob => E_NOJOB,
        }
    }

    /// 全仓唯一诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.n() as u32
    }

    /// 稳定英文短语（日志/验收文本共用，不随版本改）。
    pub fn message(self) -> &'static str {
        match self {
            PtIntentError::Empty => "empty declaration batch",
            PtIntentError::Dup => "duplicate declaration (job,page,channel)",
            PtIntentError::Unknown => "intent or channel code outside closed set",
            PtIntentError::Capacity => "ledger or job slot full",
            PtIntentError::Sealed => "declaration after job sealed",
            PtIntentError::Page => "job/page anchor out of range",
            PtIntentError::Drift => "wire value differs from declaration",
            PtIntentError::NoJob => "seal of unregistered job",
        }
    }

    /// 码值反解（零漂移判据用：每个码必须能回到唯一变体）。
    pub fn from_code(code: u32) -> Option<PtIntentError> {
        if code <= SEG_BASE {
            return None;
        }
        match (code - SEG_BASE) as u16 {
            E_EMPTY => Some(PtIntentError::Empty),
            E_DUP => Some(PtIntentError::Dup),
            E_UNKNOWN => Some(PtIntentError::Unknown),
            E_CAPACITY => Some(PtIntentError::Capacity),
            E_SEALED => Some(PtIntentError::Sealed),
            E_PAGE => Some(PtIntentError::Page),
            E_DRIFT => Some(PtIntentError::Drift),
            E_NOJOB => Some(PtIntentError::NoJob),
            _ => None,
        }
    }
}

/// 四档渲染意图的声明值。数值与 PDF `/Intent`、ICC 头渲染意图字段一致，
/// 直通时只做搬运，不做任何域映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorIntent {
    Perceptual,
    Relative,
    Saturation,
    Absolute,
}

impl ColorIntent {
    pub fn code(self) -> u8 {
        match self {
            ColorIntent::Perceptual => 0,
            ColorIntent::Relative => 1,
            ColorIntent::Saturation => 2,
            ColorIntent::Absolute => 3,
        }
    }

    pub fn from_code(c: u8) -> Result<ColorIntent, PtIntentError> {
        match c {
            0 => Ok(ColorIntent::Perceptual),
            1 => Ok(ColorIntent::Relative),
            2 => Ok(ColorIntent::Saturation),
            3 => Ok(ColorIntent::Absolute),
            _ => Err(PtIntentError::Unknown),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ColorIntent::Perceptual => "perceptual",
            ColorIntent::Relative => "relative-colorimetric",
            ColorIntent::Saturation => "saturation",
            ColorIntent::Absolute => "absolute-colorimetric",
        }
    }
}

/// 声明通道：PDF 页组内嵌 `/Intent`，或作业票据显式声明。
///
/// 两条通道必须各占一个防重键位——同一页在两条通道给出不同意图是**合法**的，
/// 由下游按通道优先级自行取舍；直通层只保证「声明不丢、值不改」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclChannel {
    Embedded,
    Ticket,
}

impl DeclChannel {
    pub fn code(self) -> u8 {
        match self {
            DeclChannel::Embedded => 0,
            DeclChannel::Ticket => 1,
        }
    }

    pub fn from_code(c: u8) -> Result<DeclChannel, PtIntentError> {
        match c {
            0 => Ok(DeclChannel::Embedded),
            1 => Ok(DeclChannel::Ticket),
            _ => Err(PtIntentError::Unknown),
        }
    }
}

/// 两档透传模式。
///
/// * `DeviceDefault`：**不处理档**——票据没有意图字段，直通层写入 0xFF 哨兵，
///   含义是「保持设备默认」，绝不去猜一个意图。
/// * `Explicit(intent)`：**显式档**——携带 0..=3 的声明值，同样不处理。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclMode {
    DeviceDefault,
    Explicit(ColorIntent),
}

impl DeclMode {
    /// 写进打印流的线值。0xFF 是「无声明」哨兵，与四档意图天然不重叠。
    pub fn wire(self) -> u8 {
        match self {
            DeclMode::DeviceDefault => 0xFF,
            DeclMode::Explicit(c) => c.code(),
        }
    }

    pub fn is_default(self) -> bool {
        matches!(self, DeclMode::DeviceDefault)
    }

    pub fn intent(self) -> Option<ColorIntent> {
        match self {
            DeclMode::DeviceDefault => None,
            DeclMode::Explicit(c) => Some(c),
        }
    }
}

/// 声明锚：作业号 + 页号（均以 1 起算；0 视为未指定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclKey {
    pub job: u16,
    pub page: u16,
}

impl DeclKey {
    /// 锚校验：0 号作业/页是排版器的「未指定」占位，透传层必须拒绝，
    /// 否则防重键会把所有未指定锚塌成同一条。
    pub fn new(job: u16, page: u16) -> Result<DeclKey, PtIntentError> {
        if job == 0 || page == 0 {
            return Err(PtIntentError::Page);
        }
        Ok(DeclKey { job, page })
    }
}

/// 单条声明落账。`emitted` 是**实际写进打印流的字节**，
/// 审计时与 `mode.wire()` 比对，任何不等都判 `Drift`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub key: DeclKey,
    pub channel: DeclChannel,
    pub mode: DeclMode,
    pub emitted: u8,
    pub seq: u32,
}

/// 作业声明状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobPhase {
    Open,
    Declared,
    Sealed,
}

impl JobPhase {
    pub fn code(self) -> u8 {
        match self {
            JobPhase::Open => 0,
            JobPhase::Declared => 1,
            JobPhase::Sealed => 2,
        }
    }
}

/// 每作业一行状态。
#[derive(Clone, Copy, Debug)]
pub struct JobSlot {
    pub job: u16,
    pub phase: JobPhase,
    pub declared: u16,
    pub sealed_seq: u32,
}

/// 上游 spool 交下来的一帧声明（未解码的线格式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclFrame {
    pub job: u16,
    pub page: u16,
    pub channel: u8,
    pub payload: u8,
}

/// 解码一帧：通道码 + 载荷码（0xFF = 设备默认，0..=3 = 显式意图）。
pub fn decode_frame(f: &DeclFrame) -> Result<(DeclKey, DeclChannel, DeclMode), PtIntentError> {
    let key = DeclKey::new(f.job, f.page)?;
    let channel = DeclChannel::from_code(f.channel)?;
    let mode = match f.payload {
        0xFF => DeclMode::DeviceDefault,
        c => DeclMode::Explicit(ColorIntent::from_code(c)?),
    };
    Ok((key, channel, mode))
}

/// 定容意图声明账本。
///
/// 三个不变量：
/// 1. `(job,page,channel)` 唯一（防重）；
/// 2. `seq` 严格单调（落账顺序即透传顺序，不可重排）；
/// 3. 每条的 `emitted` 恒等于 `mode.wire()`（透传不处理）。
pub struct IntentLedger {
    cap: usize,
    entries: Vec<Declaration>,
    jobs: Vec<JobSlot>,
    seq: u32,
    rejected: u32,
}

impl IntentLedger {
    /// 建账：`cap` 同时约束声明条目与作业槽（作业数不会超过声明数）。
    pub fn new(cap: usize) -> IntentLedger {
        IntentLedger {
            cap,
            entries: Vec::new(),
            jobs: Vec::new(),
            seq: 0,
            rejected: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.cap
    }

    /// 被拒条数（显性记账：拒绝不等于静默丢弃）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn entries(&self) -> &[Declaration] {
        &self.entries
    }

    pub fn jobs(&self) -> &[JobSlot] {
        &self.jobs
    }

    /// 两档计数：`(设备默认档, 显式档)`——J1「×2 档落账」的直接度量。
    pub fn mode_census(&self) -> (usize, usize) {
        let mut def = 0usize;
        let mut exp = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.mode.is_default() {
                    def += 1;
                } else {
                    exp += 1;
                }
            }
            i += 1;
        }
        (def, exp)
    }

    /// 按 `(job,page,channel)` 查声明（防重与回读共用同一条路径）。
    pub fn find(&self, key: DeclKey, channel: DeclChannel) -> Option<&Declaration> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key && e.channel == channel {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    fn slot_of(&self, job: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.jobs.len() {
            if let Some(j) = self.jobs.get(i) {
                if j.job == job {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 声明入口：线值由模式推导（正常路径）。
    pub fn declare(
        &mut self,
        key: DeclKey,
        channel: DeclChannel,
        mode: DeclMode,
    ) -> Result<u32, PtIntentError> {
        self.declare_wire(key, channel, mode, mode.wire())
    }

    /// 带线值的声明入口：`wire` 是上游/回放流里实际的字节。
    ///
    /// 为什么单独开这条：J1 要证明「透传不处理」，就必须拿**外部给的字节**
    /// 和声明值比对；若线值 != 声明值，说明上游已经改过值，这里显式拒绝并
    /// 记账，而不是「以账本为准」掩盖漂移。
    pub fn declare_wire(
        &mut self,
        key: DeclKey,
        channel: DeclChannel,
        mode: DeclMode,
        wire: u8,
    ) -> Result<u32, PtIntentError> {
        if key.job == 0 || key.page == 0 {
            self.rejected += 1;
            return Err(PtIntentError::Page);
        }
        if self.cap == 0 || self.entries.len() >= self.cap {
            self.rejected += 1;
            return Err(PtIntentError::Capacity);
        }
        if self.find(key, channel).is_some() {
            self.rejected += 1;
            return Err(PtIntentError::Dup);
        }
        let slot = match self.slot_of(key.job) {
            Some(s) => s,
            None => {
                if self.jobs.len() >= self.cap {
                    self.rejected += 1;
                    return Err(PtIntentError::Capacity);
                }
                self.jobs.push(JobSlot {
                    job: key.job,
                    phase: JobPhase::Open,
                    declared: 0,
                    sealed_seq: 0,
                });
                match self.jobs.len().checked_sub(1) {
                    Some(s) => s,
                    None => {
                        self.rejected += 1;
                        return Err(PtIntentError::Capacity);
                    }
                }
            }
        };
        let phase = match self.jobs.get(slot) {
            Some(j) => j.phase,
            None => {
                self.rejected += 1;
                return Err(PtIntentError::NoJob);
            }
        };
        if phase == JobPhase::Sealed {
            self.rejected += 1;
            return Err(PtIntentError::Sealed);
        }
        if wire != mode.wire() {
            self.rejected += 1;
            return Err(PtIntentError::Drift);
        }
        self.seq = self.seq.saturating_add(1);
        self.entries.push(Declaration {
            key,
            channel,
            mode,
            emitted: wire,
            seq: self.seq,
        });
        if let Some(j) = self.jobs.get_mut(slot) {
            j.declared = j.declared.saturating_add(1);
            j.phase = JobPhase::Declared;
        }
        Ok(self.seq)
    }

    /// 解码并落账一帧；解码错误原样上抛（不在这里兜底）。
    pub fn feed(&mut self, f: &DeclFrame) -> Result<u32, PtIntentError> {
        let (key, channel, mode) = decode_frame(f)?;
        self.declare(key, channel, mode)
    }

    /// 批量喂帧：空批显式拒绝（`Empty`），不做「无事发生」的静默成功。
    pub fn feed_batch(&mut self, frames: &[DeclFrame]) -> Result<usize, PtIntentError> {
        if frames.is_empty() {
            self.rejected += 1;
            return Err(PtIntentError::Empty);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < frames.len() {
            if let Some(f) = frames.get(i) {
                self.feed(f)?;
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 封板：此后该作业的任何声明都拒绝。封板本身也是一次序列号推进，
    /// 保证「声明流」与「封板事件」在同一序里可审计。
    pub fn seal(&mut self, job: u16) -> Result<u32, PtIntentError> {
        let slot = match self.slot_of(job) {
            Some(s) => s,
            None => {
                self.rejected += 1;
                return Err(PtIntentError::NoJob);
            }
        };
        let (phase, declared) = match self.jobs.get(slot) {
            Some(j) => (j.phase, j.declared),
            None => {
                self.rejected += 1;
                return Err(PtIntentError::NoJob);
            }
        };
        if phase == JobPhase::Sealed {
            self.rejected += 1;
            return Err(PtIntentError::Sealed);
        }
        if declared == 0 {
            self.rejected += 1;
            return Err(PtIntentError::Empty);
        }
        self.seq = self.seq.saturating_add(1);
        if let Some(j) = self.jobs.get_mut(slot) {
            j.phase = JobPhase::Sealed;
            j.sealed_seq = self.seq;
        }
        Ok(self.seq)
    }

    /// 重放审计：逐条核对线值与声明值，返回核对条数。
    /// 任何一条不等即 `Drift`——这是「不处理档」的防伪证据。
    pub fn audit(&self) -> Result<usize, PtIntentError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) => {
                    if e.emitted != e.mode.wire() {
                        return Err(PtIntentError::Drift);
                    }
                }
                None => return Err(PtIntentError::Drift),
            }
            i += 1;
        }
        Ok(i)
    }

    /// 序列号严格单调（封板事件也占号）。
    pub fn seq_monotonic(&self) -> bool {
        let mut last = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) => {
                    if e.seq <= last {
                        return false;
                    }
                    last = e.seq;
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 注入核：9 类坏声明必须逐类命中各自的诊断码
// ---------------------------------------------------------------------------

/// 返回 `(命中数, 总类数)`。每类都断言具体码值，而不是「任意错误」。
pub fn injection_core() -> (usize, usize) {
    let total = 9usize;
    let mut caught = 0usize;

    // 1) 空锚（page = 0）
    let mut l = IntentLedger::new(8);
    let bad_key = DeclKey { job: 3, page: 0 };
    if l.declare(bad_key, DeclChannel::Embedded, DeclMode::DeviceDefault).err()
        == Some(PtIntentError::Page)
    {
        caught += 1;
    }

    // 2) 防重：同 (job,page,channel) 第二条
    let mut l = IntentLedger::new(8);
    let k = match DeclKey::new(3, 1) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let _ = l.declare(k, DeclChannel::Ticket, DeclMode::Explicit(ColorIntent::Relative));
    if l.declare(k, DeclChannel::Ticket, DeclMode::Explicit(ColorIntent::Absolute)).err()
        == Some(PtIntentError::Dup)
    {
        caught += 1;
    }

    // 3) 非法意图载荷
    let mut l = IntentLedger::new(8);
    let f = DeclFrame { job: 4, page: 1, channel: 0, payload: 9 };
    if l.feed(&f).err() == Some(PtIntentError::Unknown) {
        caught += 1;
    }

    // 4) 非法通道码
    let mut l = IntentLedger::new(8);
    let f = DeclFrame { job: 4, page: 1, channel: 7, payload: 0xFF };
    if l.feed(&f).err() == Some(PtIntentError::Unknown) {
        caught += 1;
    }

    // 5) 账本满（cap = 1）
    let mut l = IntentLedger::new(1);
    let k1 = match DeclKey::new(5, 1) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let k2 = match DeclKey::new(5, 2) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let _ = l.declare(k1, DeclChannel::Embedded, DeclMode::DeviceDefault);
    if l.declare(k2, DeclChannel::Embedded, DeclMode::DeviceDefault).err()
        == Some(PtIntentError::Capacity)
    {
        caught += 1;
    }

    // 6) 封板后声明
    let mut l = IntentLedger::new(8);
    let k = match DeclKey::new(6, 1) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let _ = l.declare(k, DeclChannel::Embedded, DeclMode::Explicit(ColorIntent::Perceptual));
    let _ = l.seal(6);
    let k2 = match DeclKey::new(6, 2) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if l.declare(k2, DeclChannel::Embedded, DeclMode::DeviceDefault).err()
        == Some(PtIntentError::Sealed)
    {
        caught += 1;
    }

    // 7) 线值漂移（声明 Relative=1，线值却是 0）
    let mut l = IntentLedger::new(8);
    let k = match DeclKey::new(7, 1) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if l.declare_wire(k, DeclChannel::Ticket, DeclMode::Explicit(ColorIntent::Relative), 0).err()
        == Some(PtIntentError::Drift)
    {
        caught += 1;
    }

    // 8) 未登记作业封板 → NoJob
    let mut l = IntentLedger::new(8);
    if l.seal(42).err() == Some(PtIntentError::NoJob) {
        caught += 1;
    }

    // 9) 空批 → Empty
    let mut l = IntentLedger::new(8);
    if l.feed_batch(&[]).err() == Some(PtIntentError::Empty) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50107_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50107");

    // J1-1 两档落账：设备默认 + 显式各一条，线值分别为 0xFF / 1
    let mut l = IntentLedger::new(16);
    let mut ok = true;
    let mut k = match DeclKey::new(11, 1) {
        Ok(v) => v,
        Err(_) => {
            s.add("50107-J1-两档落账", false, "锚构造失败");
            return s;
        }
    };
    if l.declare(k, DeclChannel::Embedded, DeclMode::DeviceDefault).is_err() {
        ok = false;
    }
    k = match DeclKey::new(11, 2) {
        Ok(v) => v,
        Err(_) => DeclKey { job: 11, page: 2 },
    };
    if l.declare(k, DeclChannel::Embedded, DeclMode::Explicit(ColorIntent::Relative)).is_err() {
        ok = false;
    }
    let (def, exp) = l.mode_census();
    ok = ok && def == 1 && exp == 1 && l.len() == 2;
    let det = format!("def={} exp={} n={}", def, exp, l.len());
    s.add_dyn("50107-J1-两档落账", ok, &det);

    // J1-2 透传不处理：两条的线值就是声明值，0xFF 哨兵不与意图码撞车
    let mut ok = true;
    let mut i = 0usize;
    while i < l.len() {
        match l.entries().get(i) {
            Some(e) => {
                if e.emitted != e.mode.wire() {
                    ok = false;
                }
                if e.mode.is_default() && e.emitted != 0xFF {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    ok = ok && l.audit() == Ok(2);
    s.add("50107-J1-线值逐位同", ok, if ok { "" } else { "线值被改写" });

    // J1-3 两通道并存：同 (job,page) 不同通道各一条，合法
    let mut l2 = IntentLedger::new(8);
    let k = DeclKey { job: 12, page: 3 };
    let a = l2.declare(k, DeclChannel::Embedded, DeclMode::Explicit(ColorIntent::Saturation));
    let b = l2.declare(k, DeclChannel::Ticket, DeclMode::Explicit(ColorIntent::Absolute));
    let ok = a.is_ok() && b.is_ok() && l2.len() == 2 && l2.rejected() == 0;
    s.add("50107-J1-通道各占键", ok, if ok { "" } else { "通道塌成同一键" });

    // J1-4 防重：第三条同键拒绝且账本不变（守恒）
    let before = l2.len();
    let c = l2.declare(k, DeclChannel::Ticket, DeclMode::DeviceDefault);
    let ok = c.err() == Some(PtIntentError::Dup) && l2.len() == before && l2.rejected() == 1;
    let det = format!("len={} rej={}", l2.len(), l2.rejected());
    s.add_dyn("50107-J1-防重声明", ok, &det);

    // J1-5 序列号严格单调（落账序 = 透传序）
    let ok = l.seq_monotonic() && l2.seq_monotonic();
    s.add("50107-J1-序号单调", ok, if ok { "" } else { "序号回退" });

    // J1-6 意图码闭集：0..=3 全收，4/9/255 全拒
    let mut ok = true;
    let mut c = 0u16;
    while c < 256 {
        let r = ColorIntent::from_code(c as u8);
        let legal = (c as usize) < 4;
        if r.is_ok() != legal {
            ok = false;
        }
        c += 1;
    }
    s.add("50107-J1-意图闭集", ok, if ok { "" } else { "闭集有洞" });

    // J1-7 诊断码零漂移：8 个码互异且可反解回唯一变体
    let all = [
        PtIntentError::Empty,
        PtIntentError::Dup,
        PtIntentError::Unknown,
        PtIntentError::Capacity,
        PtIntentError::Sealed,
        PtIntentError::Page,
        PtIntentError::Drift,
        PtIntentError::NoJob,
    ];
    let mut ok = true;
    let mut seen = [false; 8];
    let mut i = 0usize;
    while i < all.len() {
        let code = all[i].code();
        let n = all[i].n() as usize;
        if n < 1 || n > 8 {
            ok = false;
        } else if seen[n - 1] {
            ok = false;
        } else {
            seen[n - 1] = true;
        }
        if PtIntentError::from_code(code) != Some(all[i]) {
            ok = false;
        }
        i += 1;
    }
    ok = ok && PtIntentError::from_code(SEG_BASE) .is_none()
        && PtIntentError::from_code(SEG_BASE + 9).is_none();
    s.add("50107-J1-码值不漂", ok, if ok { "" } else { "码值漂移或撞号" });

    // J1-8 封板：封板后声明必拒；重复封板亦拒
    let mut l = IntentLedger::new(8);
    let k = DeclKey { job: 21, page: 1 };
    let _ = l.declare(k, DeclChannel::Embedded, DeclMode::DeviceDefault);
    let s1 = l.seal(21);
    let s2 = l.seal(21);
    let later = l.declare(DeclKey { job: 21, page: 2 }, DeclChannel::Ticket, DeclMode::DeviceDefault);
    let ok = s1.is_ok() && s2.err() == Some(PtIntentError::Sealed) && later.err() == Some(PtIntentError::Sealed);
    s.add("50107-J1-封板闸", ok, if ok { "" } else { "封板后仍可写" });

    // J1-9 未登记作业封板 → NoJob；空批 → Empty
    let mut l = IntentLedger::new(4);
    let a = l.seal(99);
    let b = l.feed_batch(&[]);
    let ok = a.err() == Some(PtIntentError::NoJob) && b.err() == Some(PtIntentError::Empty);
    s.add("50107-J1-空路径显式", ok, if ok { "" } else { "空路径被静默吞" });

    // J1-10 容量满：2 条后第 3 条 Capacity，且已落 2 条不丢
    let mut l = IntentLedger::new(2);
    let r1 = l.declare(DeclKey { job: 31, page: 1 }, DeclChannel::Embedded, DeclMode::DeviceDefault);
    let r2 = l.declare(DeclKey { job: 31, page: 2 }, DeclChannel::Embedded, DeclMode::DeviceDefault);
    let r3 = l.declare(DeclKey { job: 31, page: 3 }, DeclChannel::Embedded, DeclMode::DeviceDefault);
    let ok = r1.is_ok() && r2.is_ok() && r3.err() == Some(PtIntentError::Capacity) && l.len() == 2;
    let det = format!("len={} rej={}", l.len(), l.rejected());
    s.add_dyn("50107-J1-容量显式", ok, &det);

    // J1-11 线值漂移：外部字节与声明不符必拒，账本不变
    let mut l = IntentLedger::new(4);
    let k = DeclKey { job: 41, page: 1 };
    let bad = l.declare_wire(k, DeclChannel::Ticket, DeclMode::Explicit(ColorIntent::Absolute), 0);
    let ok = bad.err() == Some(PtIntentError::Drift) && l.len() == 0;
    s.add("50107-J1-漂移必拒", ok, if ok { "" } else { "漂移被接受" });

    // J1-12 批量喂帧：3 帧全落，顺序与序号一致
    let frames = [
        DeclFrame { job: 51, page: 1, channel: 0, payload: 0xFF },
        DeclFrame { job: 51, page: 2, channel: 0, payload: 2 },
        DeclFrame { job: 51, page: 3, channel: 1, payload: 1 },
    ];
    let mut l = IntentLedger::new(8);
    let n = l.feed_batch(&frames);
    let mut ok = n == Ok(3) && l.len() == 3;
    let mut prev = 0u32;
    let mut i = 0usize;
    while i < l.len() {
        match l.entries().get(i) {
            Some(e) => {
                if e.seq <= prev {
                    ok = false;
                }
                prev = e.seq;
            }
            None => ok = false,
        }
        i += 1;
    }
    s.add("50107-J1-批量保序", ok, if ok { "" } else { "批内乱序" });

    // J1-13 确定性：同脚本两次跑，落账逐条一致
    let run = || -> Vec<Declaration> {
        let mut l = IntentLedger::new(8);
        let _ = l.feed_batch(&frames);
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < l.len() {
            if let Some(e) = l.entries().get(i) {
                out.push(*e);
            }
            i += 1;
        }
        out
    };
    let a = run();
    let b = run();
    let ok = a == b && a.len() == 3;
    s.add("50107-J1-重放确定", ok, if ok { "" } else { "两次结果不同" });

    // J1-14 注入核：9 类坏声明全部命中各自诊断码
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50107-J1-注入必抓", ok, &det);

    // J1-15 拒绝可数：注入过程里的拒绝都被记账，没有静默丢帧
    let mut l = IntentLedger::new(2);
    let _ = l.declare(DeclKey { job: 61, page: 1 }, DeclChannel::Embedded, DeclMode::DeviceDefault);
    let _ = l.declare(DeclKey { job: 61, page: 2 }, DeclChannel::Embedded, DeclMode::DeviceDefault);
    let _ = l.declare(DeclKey { job: 61, page: 3 }, DeclChannel::Embedded, DeclMode::DeviceDefault);
    let _ = l.declare(DeclKey { job: 61, page: 1 }, DeclChannel::Embedded, DeclMode::DeviceDefault);
    let ok = l.rejected() == 2 && l.audit() == Ok(2);
    s.add("50107-J1-拒绝可数", ok, if ok { "" } else { "拒绝未记账" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50107_checks_green() {
        let set = run_unx50107_checks();
        assert!(set.all_passed(), "UNX-F50107 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50107_default_mode_is_passthrough_sentinel() {
        // 设备默认档的线值必须是 0xFF，且不可能与 0..=3 的意图码重合。
        assert_eq!(DeclMode::DeviceDefault.wire(), 0xFF);
        let mut i = 0u8;
        while i < 4 {
            let m = DeclMode::Explicit(ColorIntent::from_code(i).expect("legal code"));
            assert_ne!(m.wire(), DeclMode::DeviceDefault.wire());
            i += 1;
        }
    }

    #[test]
    fn unx50107_duplicate_never_grows_ledger() {
        let mut l = IntentLedger::new(4);
        let k = DeclKey::new(1, 1).expect("anchor");
        assert!(l.declare(k, DeclChannel::Ticket, DeclMode::DeviceDefault).is_ok());
        let n = l.len();
        assert_eq!(
            l.declare(k, DeclChannel::Ticket, DeclMode::DeviceDefault).err(),
            Some(PtIntentError::Dup)
        );
        assert_eq!(l.len(), n);
    }
}
