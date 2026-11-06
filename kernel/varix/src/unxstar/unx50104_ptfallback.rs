//! UNX-F50104 直通失败回退光栅化链（自动切换显式登记）（0x43920 段）
//!
//! 锚点：UNX-F50104-J1 回退 ×20 输出可达且登记留痕；判据锚定 体验九 错误三要素。
//!
//! 数据段 `SEG_BASE = 0x43920`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 11`，
//! 逐项见 [`PtError`]。
//!
//! 直通（native）失败后不能「静默丢单」：必须自动切到光栅化链，并且把
//! **为什么切**、**从哪个状态切到哪个状态**、**产出哪件光栅件**三件事写进
//! append-only 账本。本模块把这条路径拆成三层：
//! 1. 状态机 [`Stage`]：New → Prechecked → Dispatched → Native →
//!    RasterQueued → Rasterizing → Rasterized → Delivered；失败走
//!    Failed → RasterQueued 的单次重试弧。任何非法迁移返回
//!    `IllegalTransition`，不允许「跳状态」把失败洗成成功。
//! 2. 回退注册表 [`FallbackRegistry`]：失败原因 → 动作（Rasterize /
//!    Reject）的唯一映射；重复登记与缺规则都显式报错，禁止隐式兜底。
//! 3. 账本 [`FallbackLedger`]：每条迁移一行，序号单调，封账指纹对顺序
//!    敏感；回退件 [`Artifact`] 的 id 由 (job, reason, dpi, attempt) 复算。
//!
//! 结构清单：
//! * [`PtTicket`] / [`JobKind`] / [`FallbackReason`]：输入与失败语义；
//! * [`Stage`] + [`Chain`]：状态机与编排器；
//! * [`FallbackRegistry`] / [`Rule`] / [`Action`]：注册表；
//! * [`FallbackLedger`] / [`LedgerEntry`]：审计账本；
//! * [`RasterQueue`] / [`RasterParams`] / [`Artifact`] / [`Outcome`]：光栅面；
//! * [`fallback_twenty`]：20 张坏票的回退核；[`injection_core`]：11 类注入。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43920 独占 0x20）。
pub const SEG_BASE: u32 = 0x43920;

const E_EMPTY: u16 = 1;
const E_TRANSITION: u16 = 2;
const E_NO_FALLBACK: u16 = 3;
const E_RASTER_FAIL: u16 = 4;
const E_QUEUE_FULL: u16 = 5;
const E_DUPLICATE: u16 = 6;
const E_REGISTRY_FULL: u16 = 7;
const E_UNKNOWN_REASON: u16 = 8;
const E_PARAMS: u16 = 9;
const E_ARTIFACT: u16 = 10;
const E_ATTEMPTS: u16 = 11;

/// 一次回退最多两次光栅尝试：首次驱动拒收可能是临时态，第二次仍失败
/// 说明是内容问题，继续重试只会烧纸。
pub const MAX_ATTEMPTS: u8 = 2;

/// 回退链诊断。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtError {
    /// 作业号为 0（未分配）。
    Empty,
    /// 状态迁移不在合法表内。
    Transition,
    /// 该失败原因在注册表里映射为 Reject。
    NoFallback,
    /// 两次光栅尝试都失败。
    RasterFail,
    /// 光栅队列/账本容量已满。
    QueueFull,
    /// 注册表重复登记同一原因。
    Duplicate,
    /// 注册表容量已满。
    RegistryFull,
    /// 注册表没有该原因的动作。
    UnknownReason,
    /// 光栅参数非法（DPI/页上限）。
    Params,
    /// 交付时缺少光栅件。
    Artifact,
    /// 重试次数已耗尽。
    Attempts,
}

impl PtError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PtError::Empty => E_EMPTY,
            PtError::Transition => E_TRANSITION,
            PtError::NoFallback => E_NO_FALLBACK,
            PtError::RasterFail => E_RASTER_FAIL,
            PtError::QueueFull => E_QUEUE_FULL,
            PtError::Duplicate => E_DUPLICATE,
            PtError::RegistryFull => E_REGISTRY_FULL,
            PtError::UnknownReason => E_UNKNOWN_REASON,
            PtError::Params => E_PARAMS,
            PtError::Artifact => E_ARTIFACT,
            PtError::Attempts => E_ATTEMPTS,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PtError::Empty => "job id not assigned",
            PtError::Transition => "stage transition not allowed",
            PtError::NoFallback => "reason maps to explicit reject",
            PtError::RasterFail => "raster chain exhausted attempts",
            PtError::QueueFull => "raster queue or ledger full",
            PtError::Duplicate => "fallback rule already registered",
            PtError::RegistryFull => "fallback registry full",
            PtError::UnknownReason => "no fallback rule for reason",
            PtError::Params => "raster parameters invalid",
            PtError::Artifact => "raster artifact missing at delivery",
            PtError::Attempts => "retry attempts exhausted",
        }
    }
}

/// 作业类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    /// PDF 直通件。
    Pdf,
    /// PostScript 直通件。
    Ps,
}

impl JobKind {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            JobKind::Pdf => "pdf",
            JobKind::Ps => "ps",
        }
    }
}

/// 直通失败原因（闭集）。每一位都对应一条注册表规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    /// 加密 PDF。
    Encrypted,
    /// xref/结构损坏。
    XrefBroken,
    /// PS 语言档不被设备接受。
    PsLevel,
    /// 页范围非法。
    PageRange,
    /// 副本数非法。
    Copies,
    /// 设备侧拒绝（驱动回执）。
    DriverReject,
    /// 设备超时。
    Timeout,
}

impl FallbackReason {
    /// 稳定编号（用于指纹与账本）。
    pub fn code(self) -> u8 {
        match self {
            FallbackReason::Encrypted => 1,
            FallbackReason::XrefBroken => 2,
            FallbackReason::PsLevel => 3,
            FallbackReason::PageRange => 4,
            FallbackReason::Copies => 5,
            FallbackReason::DriverReject => 6,
            FallbackReason::Timeout => 7,
        }
    }

    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            FallbackReason::Encrypted => "encrypted",
            FallbackReason::XrefBroken => "xref-broken",
            FallbackReason::PsLevel => "ps-level",
            FallbackReason::PageRange => "page-range",
            FallbackReason::Copies => "copies",
            FallbackReason::DriverReject => "driver-reject",
            FallbackReason::Timeout => "timeout",
        }
    }
}

/// 直通票：上游判定器的产出摘要（本模块不重新解析字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtTicket {
    /// 作业号（0 = 未分配）。
    pub job: u32,
    /// 类型。
    pub kind: JobKind,
    /// 页数。
    pub pages: u16,
    /// 副本数。
    pub copies: u16,
    /// 上游标记位（见各常量）。
    pub flags: u16,
    /// 光栅敌意度：0 正常，1 首次失败、2 两次都失败。
    pub hostile: u8,
}

/// 加密标记。
pub const F_ENCRYPTED: u16 = 0x0001;
/// xref 损坏标记。
pub const F_XREF_BROKEN: u16 = 0x0002;
/// PS 语言档不符标记。
pub const F_PS_LEVEL: u16 = 0x0004;
/// 页范围非法标记。
pub const F_RANGE: u16 = 0x0008;
/// 副本数非法标记。
pub const F_COPIES: u16 = 0x0010;
/// 驱动拒收标记。
pub const F_DRIVER: u16 = 0x0020;
/// 超时标记。
pub const F_TIMEOUT: u16 = 0x0040;

/// 状态机状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 新单。
    New,
    /// 已完成直通前检。
    Prechecked,
    /// 已派发。
    Dispatched,
    /// 直通尝试中。
    Native,
    /// 排队等光栅。
    RasterQueued,
    /// 光栅中。
    Rasterizing,
    /// 光栅完成。
    Rasterized,
    /// 已交付。
    Delivered,
    /// 失败（可单次重试）。
    Failed,
}

impl Stage {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            Stage::New => "new",
            Stage::Prechecked => "prechecked",
            Stage::Dispatched => "dispatched",
            Stage::Native => "native",
            Stage::RasterQueued => "raster-queued",
            Stage::Rasterizing => "rasterizing",
            Stage::Rasterized => "rasterized",
            Stage::Delivered => "delivered",
            Stage::Failed => "failed",
        }
    }

    /// 是否终态。
    pub fn terminal(self) -> bool {
        matches!(self, Stage::Delivered | Stage::Failed)
    }
}

/// 合法迁移表。显式枚举而不是 `_ => true`，防「跳状态洗失败」。
pub fn legal_transition(from: Stage, to: Stage) -> bool {
    matches!(
        (from, to),
        (Stage::New, Stage::Prechecked)
            | (Stage::Prechecked, Stage::Dispatched)
            | (Stage::Prechecked, Stage::Failed)
            | (Stage::Dispatched, Stage::Native)
            | (Stage::Dispatched, Stage::RasterQueued)
            | (Stage::Dispatched, Stage::Failed)
            | (Stage::Native, Stage::Delivered)
            | (Stage::Native, Stage::RasterQueued)
            | (Stage::Native, Stage::Failed)
            | (Stage::RasterQueued, Stage::Rasterizing)
            | (Stage::Rasterizing, Stage::Rasterized)
            | (Stage::Rasterizing, Stage::Failed)
            | (Stage::Rasterized, Stage::Delivered)
            | (Stage::Failed, Stage::RasterQueued)
    )
}

/// 注册表动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 自动切光栅。
    Rasterize,
    /// 显式拒绝（不自动重试）。
    Reject,
}

/// 一条注册规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// 失败原因。
    pub reason: FallbackReason,
    /// 动作。
    pub action: Action,
}

/// 回退注册表：定容、原因唯一。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackRegistry {
    /// 规则。
    pub rules: Vec<Rule>,
    /// 容量。
    pub capacity: usize,
}

impl FallbackRegistry {
    /// 空表。
    pub fn empty(capacity: usize) -> FallbackRegistry {
        FallbackRegistry {
            rules: Vec::new(),
            capacity,
        }
    }

    /// 标准表：除 Timeout 外全部自动光栅；Timeout 交给上层显式拒绝，
    /// 因为超时通常是链路问题，盲目重光栅会放大队列压力。
    pub fn standard() -> FallbackRegistry {
        let mut r = FallbackRegistry::empty(16);
        let entries = [
            (FallbackReason::Encrypted, Action::Rasterize),
            (FallbackReason::XrefBroken, Action::Rasterize),
            (FallbackReason::PsLevel, Action::Rasterize),
            (FallbackReason::PageRange, Action::Rasterize),
            (FallbackReason::Copies, Action::Rasterize),
            (FallbackReason::DriverReject, Action::Rasterize),
            (FallbackReason::Timeout, Action::Reject),
        ];
        let mut i = 0usize;
        while i < entries.len() {
            if let Some(e) = entries.get(i) {
                let _ = r.register(e.0, e.1);
            }
            i += 1;
        }
        r
    }

    /// 登记；重复与满容都显式拒。
    pub fn register(&mut self, reason: FallbackReason, action: Action) -> Result<(), PtError> {
        if self.rules.len() >= self.capacity {
            return Err(PtError::RegistryFull);
        }
        let mut i = 0usize;
        while i < self.rules.len() {
            if let Some(r) = self.rules.get(i) {
                if r.reason == reason {
                    return Err(PtError::Duplicate);
                }
            }
            i += 1;
        }
        self.rules.push(Rule { reason, action });
        Ok(())
    }

    /// 查询动作；缺规则显式报 UnknownReason。
    pub fn resolve(&self, reason: FallbackReason) -> Result<Action, PtError> {
        let mut i = 0usize;
        while i < self.rules.len() {
            if let Some(r) = self.rules.get(i) {
                if r.reason == reason {
                    return Ok(r.action);
                }
            }
            i += 1;
        }
        Err(PtError::UnknownReason)
    }

    /// 规则数。
    pub fn len(&self) -> usize {
        self.rules.len()
    }
}

/// 账本行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 单调序号（1 起）。
    pub seq: u32,
    /// 作业号。
    pub job: u32,
    /// 源状态。
    pub from: Stage,
    /// 目标状态。
    pub to: Stage,
    /// 触发原因（成功迁移为 `None`）。
    pub reason: Option<FallbackReason>,
    /// 关联光栅件 id（0 = 尚无）。
    pub artifact: u32,
}

/// append-only 审计账本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackLedger {
    /// 容量。
    pub capacity: usize,
    /// 行。
    pub entries: Vec<LedgerEntry>,
}

impl FallbackLedger {
    /// 新建。
    pub fn new(capacity: usize) -> FallbackLedger {
        FallbackLedger {
            capacity,
            entries: Vec::new(),
        }
    }

    /// 追加；满账显式拒，不覆盖旧证据。
    pub fn append(&mut self, e: LedgerEntry) -> Result<(), PtError> {
        if self.entries.len() >= self.capacity {
            return Err(PtError::QueueFull);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 带某原因的行数（回退留痕判据）。
    pub fn count_reason(&self, reason: FallbackReason) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.reason == Some(reason) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 最后一行。
    pub fn last(&self) -> Option<&LedgerEntry> {
        let n = self.entries.len();
        if n == 0 {
            None
        } else {
            self.entries.get(n - 1)
        }
    }

    /// 链式 FNV 封账：对行顺序敏感。
    pub fn seal(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h ^= e.seq;
                h = h.wrapping_mul(0x0100_0193);
                h ^= e.job;
                h = h.wrapping_mul(0x0100_0193);
                h ^= e.from as u32;
                h = h.wrapping_mul(0x0100_0193);
                h ^= e.to as u32;
                h = h.wrapping_mul(0x0100_0193);
                h ^= match e.reason {
                    Some(r) => r.code() as u32,
                    None => 0,
                };
                h = h.wrapping_mul(0x0100_0193);
                h ^= e.artifact;
                h = h.wrapping_mul(0x0100_0193);
            }
            i += 1;
        }
        h
    }
}

/// 光栅队列（FIFO）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterQueue {
    /// 作业号队列。
    pub items: Vec<u32>,
    /// 容量。
    pub capacity: usize,
}

impl RasterQueue {
    /// 新建。
    pub fn new(capacity: usize) -> RasterQueue {
        RasterQueue {
            items: Vec::new(),
            capacity,
        }
    }

    /// 入队；满显式拒。
    pub fn push(&mut self, job: u32) -> Result<(), PtError> {
        if self.items.len() >= self.capacity {
            return Err(PtError::QueueFull);
        }
        self.items.push(job);
        Ok(())
    }

    /// 出队（FIFO）。
    pub fn pop(&mut self) -> Option<u32> {
        if self.items.is_empty() {
            return None;
        }
        Some(self.items.remove(0))
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

/// 光栅配色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    /// 黑白。
    Mono,
    /// 灰度。
    Gray,
    /// 彩色。
    Color,
}

impl ColorMode {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            ColorMode::Mono => "mono",
            ColorMode::Gray => "gray",
            ColorMode::Color => "color",
        }
    }
}

/// 光栅参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterParams {
    /// DPI（150..=1200）。
    pub dpi: u16,
    /// 配色。
    pub color: ColorMode,
    /// 单次光栅页上限。
    pub page_limit: u16,
}

impl RasterParams {
    /// 构造。
    pub const fn new(dpi: u16, color: ColorMode, page_limit: u16) -> RasterParams {
        RasterParams {
            dpi,
            color,
            page_limit,
        }
    }

    /// 默认档：600dpi 灰度、1000 页上限（办公直通回退的常用档）。
    pub fn standard() -> RasterParams {
        RasterParams::new(600, ColorMode::Gray, 1000)
    }

    /// 参数是否可用。DPI 低于 150 文字不可读，高于 1200 只会撑爆内存。
    pub fn valid(&self) -> bool {
        self.dpi >= 150 && self.dpi <= 1200 && self.page_limit > 0
    }
}

/// 光栅件（回退链的唯一交付物）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// 件 id（由输入复算）。
    pub id: u32,
    /// 页数。
    pub pages: u16,
    /// DPI。
    pub dpi: u16,
    /// 估算字节数。
    pub bytes: u32,
    /// 内容指纹。
    pub digest: u32,
}

impl Artifact {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "artifact {} pages={} dpi={} bytes={} digest=0x{:x}",
            self.id, self.pages, self.dpi, self.bytes, self.digest
        )
    }
}

/// 一次运行结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 是否走了回退。
    pub fallback: bool,
    /// 回退原因（直通成功为 `None`）。
    pub reason: Option<FallbackReason>,
    /// 光栅件（直通成功为 `None`）。
    pub artifact: Option<Artifact>,
    /// 光栅尝试次数。
    pub attempts: u8,
    /// 账本封账指纹。
    pub seal: u32,
}

impl Outcome {
    /// 一行摘要（结果摘要必须非空）。
    pub fn summary(&self) -> String {
        let why = match self.reason {
            Some(r) => r.label().to_string(),
            None => "native".to_string(),
        };
        let art = match &self.artifact {
            Some(a) => a.summary(),
            None => "none".to_string(),
        };
        format!(
            "outcome fallback={} reason={} attempts={} {}",
            self.fallback, why, self.attempts, art
        )
    }
}

/// 直通前检：返回失败原因；顺序固定，保证同一票据结论可复算。
pub fn passthrough_precheck(t: &PtTicket) -> Result<(), FallbackReason> {
    if t.pages == 0 {
        return Err(FallbackReason::PageRange);
    }
    if t.copies == 0 || t.copies > 99 {
        return Err(FallbackReason::Copies);
    }
    if t.flags & F_ENCRYPTED != 0 {
        return Err(FallbackReason::Encrypted);
    }
    if t.flags & F_XREF_BROKEN != 0 {
        return Err(FallbackReason::XrefBroken);
    }
    if t.kind == JobKind::Ps && t.flags & F_PS_LEVEL != 0 {
        return Err(FallbackReason::PsLevel);
    }
    if t.flags & F_RANGE != 0 {
        return Err(FallbackReason::PageRange);
    }
    if t.flags & F_COPIES != 0 {
        return Err(FallbackReason::Copies);
    }
    if t.flags & F_DRIVER != 0 {
        return Err(FallbackReason::DriverReject);
    }
    if t.flags & F_TIMEOUT != 0 {
        return Err(FallbackReason::Timeout);
    }
    Ok(())
}

/// 回退链编排器：状态 + 注册表 + 队列 + 账本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    /// 当前状态。
    pub stage: Stage,
    /// 注册表。
    pub registry: FallbackRegistry,
    /// 账本。
    pub ledger: FallbackLedger,
    /// 队列。
    pub queue: RasterQueue,
    /// 参数。
    pub params: RasterParams,
    /// 提交计数。
    pub submitted: u32,
    /// 直通成功计数。
    pub native: u32,
    /// 回退成功计数。
    pub fallback: u32,
    /// 失败计数。
    pub failed: u32,
    /// 当前作业的重试次数。
    pub attempts: u8,
    /// 账本序号。
    pub seq: u32,
}

impl Chain {
    /// 标准链：注册表 16 格、队列 8 格、账本 64 行、600dpi 灰度。
    pub fn new() -> Chain {
        Chain {
            stage: Stage::New,
            registry: FallbackRegistry::standard(),
            ledger: FallbackLedger::new(64),
            queue: RasterQueue::new(8),
            params: RasterParams::standard(),
            submitted: 0,
            native: 0,
            fallback: 0,
            failed: 0,
            attempts: 0,
            seq: 0,
        }
    }

    /// 显式迁移：非法迁移直接拒，成功迁移写账。
    pub fn advance(
        &mut self,
        to: Stage,
        job: u32,
        reason: Option<FallbackReason>,
        artifact: u32,
    ) -> Result<(), PtError> {
        if !legal_transition(self.stage, to) {
            return Err(PtError::Transition);
        }
        self.seq = self.seq.saturating_add(1);
        let entry = LedgerEntry {
            seq: self.seq,
            job,
            from: self.stage,
            to,
            reason,
            artifact,
        };
        self.ledger.append(entry)?;
        self.stage = to;
        Ok(())
    }

    /// 由输入复算光栅件：同 (job, reason, dpi, attempt) 必得同 id。
    fn make_artifact(
        &self,
        t: &PtTicket,
        reason: FallbackReason,
        attempt: u8,
    ) -> Result<Artifact, PtError> {
        if t.pages == 0 || self.params.page_limit == 0 {
            return Err(PtError::Params);
        }
        let mut h: u32 = 0x811c_9dc5;
        let feed = |h: &mut u32, v: u32| {
            *h ^= v;
            *h = (*h).wrapping_mul(0x0100_0193);
        };
        feed(&mut h, t.job);
        feed(&mut h, reason.code() as u32);
        feed(&mut h, self.params.dpi as u32);
        feed(&mut h, attempt as u32);
        feed(&mut h, t.pages as u32);
        let id = if h == 0 { 1 } else { h };
        let bytes = (t.pages as u32)
            .saturating_mul(self.params.dpi as u32)
            .saturating_mul(t.copies as u32)
            .saturating_div(3);
        let mut d = id ^ 0x9e37_79b9;
        d = d.wrapping_mul(0x0100_0193);
        Ok(Artifact {
            id,
            pages: t.pages,
            dpi: self.params.dpi,
            bytes,
            digest: d,
        })
    }

    /// 重试弧：Failed → RasterQueued，最多 `MAX_ATTEMPTS` 次。
    pub fn retry(
        &mut self,
        t: &PtTicket,
        reason: FallbackReason,
    ) -> Result<(), PtError> {
        if self.attempts >= MAX_ATTEMPTS {
            return Err(PtError::Attempts);
        }
        self.advance(Stage::RasterQueued, t.job, Some(reason), 0)
    }

    /// 交付既有光栅件；缺失显式报 Artifact（防止空交付冒充成功）。
    pub fn deliver_existing(&mut self, art: Option<Artifact>) -> Result<Artifact, PtError> {
        let a = match art {
            Some(v) => v,
            None => return Err(PtError::Artifact),
        };
        self.advance(Stage::Delivered, a.id, None, a.id)?;
        Ok(a)
    }

    /// 主编排入口。每张票的生命周期从 `New` 起；若上一张票已到终态，
    /// 先回到 `New`（账本、队列与累计计数全部保留），保证多票连续下发时
    /// 不是「第二张票从 Delivered 出发」这种非法起点。
    pub fn run(&mut self, t: &PtTicket) -> Result<Outcome, PtError> {
        self.submitted = self.submitted.saturating_add(1);
        if t.job == 0 {
            self.failed = self.failed.saturating_add(1);
            return Err(PtError::Empty);
        }
        if !self.params.valid() {
            self.failed = self.failed.saturating_add(1);
            return Err(PtError::Params);
        }
        if self.stage.terminal() {
            self.stage = Stage::New;
        }
        self.attempts = 0;

        self.advance(Stage::Prechecked, t.job, None, 0)?;
        self.advance(Stage::Dispatched, t.job, None, 0)?;

        let pre = passthrough_precheck(t);
        let native_reason = match pre {
            Ok(()) => {
                self.advance(Stage::Native, t.job, None, 0)?;
                if t.hostile > 0 {
                    // 驱动回执才是最终裁判：前检通过也可能被设备拒收。
                    Some(FallbackReason::DriverReject)
                } else {
                    None
                }
            }
            Err(r) => Some(r),
        };

        let reason = match native_reason {
            None => {
                self.advance(Stage::Delivered, t.job, None, 0)?;
                self.native = self.native.saturating_add(1);
                return Ok(Outcome {
                    fallback: false,
                    reason: None,
                    artifact: None,
                    attempts: 0,
                    seal: self.ledger.seal(),
                });
            }
            Some(r) => r,
        };

        let action = match self.registry.resolve(reason) {
            Ok(a) => a,
            Err(e) => {
                self.failed = self.failed.saturating_add(1);
                return Err(e);
            }
        };
        if action == Action::Reject {
            let _ = self.advance(Stage::Failed, t.job, Some(reason), 0);
            self.failed = self.failed.saturating_add(1);
            return Err(PtError::NoFallback);
        }

        self.advance(Stage::RasterQueued, t.job, Some(reason), 0)?;
        if let Err(e) = self.queue.push(t.job) {
            self.failed = self.failed.saturating_add(1);
            return Err(e);
        }

        loop {
            self.attempts = self.attempts.saturating_add(1);
            self.advance(Stage::Rasterizing, t.job, Some(reason), 0)?;
            let fail_now = self.attempts == 1 && t.hostile >= 1;
            if fail_now {
                self.advance(Stage::Failed, t.job, Some(reason), 0)?;
                if t.hostile >= 2 || self.attempts >= MAX_ATTEMPTS {
                    let _ = self.queue.pop();
                    self.failed = self.failed.saturating_add(1);
                    return Err(PtError::RasterFail);
                }
                // 单次重试：直接走重试弧，不重复入队（队列里那份仍在）。
                self.advance(Stage::RasterQueued, t.job, Some(reason), 0)?;
                continue;
            }
            let art = self.make_artifact(t, reason, self.attempts)?;
            self.advance(Stage::Rasterized, t.job, Some(reason), art.id)?;
            let _ = self.queue.pop();
            self.advance(Stage::Delivered, t.job, Some(reason), art.id)?;
            self.fallback = self.fallback.saturating_add(1);
            return Ok(Outcome {
                fallback: true,
                reason: Some(reason),
                artifact: Some(art),
                attempts: self.attempts,
                seal: self.ledger.seal(),
            });
        }
    }
}

impl Default for Chain {
    fn default() -> Chain {
        Chain::new()
    }
}

// ---------------------------------------------------------------------------
// 回退 20 例与注入核
// ---------------------------------------------------------------------------

fn base_ticket(job: u32) -> PtTicket {
    PtTicket {
        job,
        kind: JobKind::Pdf,
        pages: 5,
        copies: 1,
        flags: 0,
        hostile: 0,
    }
}

/// 20 张必失败直通票：6 类原因 ×3 + 2 张组合票（前检顺序决定归因）。
pub fn fallback_tickets() -> Vec<(PtTicket, FallbackReason)> {
    let mut v: Vec<(PtTicket, FallbackReason)> = Vec::new();
    let kinds: [(u16, FallbackReason); 6] = [
        (F_ENCRYPTED, FallbackReason::Encrypted),
        (F_XREF_BROKEN, FallbackReason::XrefBroken),
        (F_PS_LEVEL, FallbackReason::PsLevel),
        (F_RANGE, FallbackReason::PageRange),
        (F_COPIES, FallbackReason::Copies),
        (F_DRIVER, FallbackReason::DriverReject),
    ];
    let mut k = 0usize;
    while k < kinds.len() {
        if let Some((flag, reason)) = kinds.get(k) {
            let mut n = 0u32;
            while n < 3 {
                let mut t = base_ticket(100 + (k as u32) * 10 + n);
                t.flags = *flag;
                if *flag == F_PS_LEVEL {
                    t.kind = JobKind::Ps;
                }
                if *flag == F_COPIES {
                    t.copies = 120;
                }
                v.push((t, *reason));
                n += 1;
            }
        }
        k += 1;
    }
    // 组合 1：加密 + 页范围标记 —— 前检顺序先命中加密，验证归因不被
    // 后面的标记覆盖；页数保持 5 以便光栅件可产出。
    let mut t = base_ticket(901);
    t.flags = F_ENCRYPTED | F_RANGE;
    v.push((t, FallbackReason::Encrypted));
    // 组合 2：副本为 0 且 PS 档不符 —— 前检先命中副本。
    let mut t = base_ticket(902);
    t.kind = JobKind::Ps;
    t.copies = 0;
    t.flags = F_PS_LEVEL;
    v.push((t, FallbackReason::Copies));
    v
}

/// 20 例回退核：返回 (产出可达数, 登记留痕数, 总数)。
pub fn fallback_twenty() -> (usize, usize, usize) {
    let cases = fallback_tickets();
    let mut reached = 0usize;
    let mut logged = 0usize;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((t, want)) = cases.get(i) {
            let mut c = Chain::new();
            match c.run(t) {
                Ok(o) => {
                    if o.fallback
                        && o.reason == Some(*want)
                        && o.artifact.as_ref().map(|a| a.pages) == Some(t.pages)
                    {
                        reached += 1;
                    }
                    let trace = c.ledger.count_reason(*want) >= 3;
                    let rule_ok = c.registry.resolve(*want) == Ok(Action::Rasterize);
                    if trace && rule_ok && c.stage == Stage::Delivered && c.fallback == 1 {
                        logged += 1;
                    }
                }
                Err(_) => {}
            }
        }
        i += 1;
    }
    (reached, logged, cases.len())
}

/// 11 类坏样本注入核，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 11usize;

    // 1) 作业号为 0
    let mut c = Chain::new();
    let t = base_ticket(0);
    if c.run(&t).err() == Some(PtError::Empty) {
        caught += 1;
    }

    // 2) 非法迁移 New → Delivered
    let mut c = Chain::new();
    if c.advance(Stage::Delivered, 1, None, 0).err() == Some(PtError::Transition) {
        caught += 1;
    }

    // 3) 注册表动作 = Reject 的 Timeout
    let mut c = Chain::new();
    let mut t = base_ticket(7);
    t.flags = F_TIMEOUT;
    if c.run(&t).err() == Some(PtError::NoFallback) && c.stage == Stage::Failed {
        caught += 1;
    }

    // 4) 两次光栅都失败
    let mut c = Chain::new();
    let mut t = base_ticket(8);
    t.flags = F_ENCRYPTED;
    t.hostile = 2;
    if c.run(&t).err() == Some(PtError::RasterFail) && c.failed == 1 {
        caught += 1;
    }

    // 5) 队列满
    let mut q = RasterQueue::new(1);
    let _ = q.push(1);
    if q.push(2).err() == Some(PtError::QueueFull) {
        caught += 1;
    }

    // 6) 注册表重复登记
    let mut r = FallbackRegistry::standard();
    if r.register(FallbackReason::Encrypted, Action::Reject).err() == Some(PtError::Duplicate) {
        caught += 1;
    }

    // 7) 注册表满容
    let mut r = FallbackRegistry::empty(1);
    let _ = r.register(FallbackReason::Encrypted, Action::Rasterize);
    if r.register(FallbackReason::XrefBroken, Action::Rasterize).err()
        == Some(PtError::RegistryFull)
    {
        caught += 1;
    }

    // 8) 缺规则
    let r = FallbackRegistry::empty(4);
    if r.resolve(FallbackReason::DriverReject).err() == Some(PtError::UnknownReason) {
        caught += 1;
    }

    // 9) 光栅参数非法
    let mut c = Chain::new();
    c.params.dpi = 0;
    if c.run(&base_ticket(9)).err() == Some(PtError::Params) {
        caught += 1;
    }

    // 10) 交付缺件
    let mut c = Chain::new();
    c.stage = Stage::Rasterized;
    if c.deliver_existing(None).err() == Some(PtError::Artifact) {
        caught += 1;
    }

    // 11) 重试次数耗尽
    let mut c = Chain::new();
    c.stage = Stage::Failed;
    c.attempts = MAX_ATTEMPTS;
    if c.retry(&base_ticket(11), FallbackReason::Encrypted).err() == Some(PtError::Attempts) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50104 自检：16 条判据。
pub fn run_unx50104_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50104");

    // J1-1 合法迁移表：主链 8 条全通，跳跃必须拒。
    let legal = legal_transition(Stage::New, Stage::Prechecked)
        && legal_transition(Stage::Prechecked, Stage::Dispatched)
        && legal_transition(Stage::Dispatched, Stage::Native)
        && legal_transition(Stage::Native, Stage::Delivered)
        && legal_transition(Stage::Dispatched, Stage::RasterQueued)
        && legal_transition(Stage::RasterQueued, Stage::Rasterizing)
        && legal_transition(Stage::Rasterizing, Stage::Rasterized)
        && legal_transition(Stage::Rasterized, Stage::Delivered)
        && legal_transition(Stage::Failed, Stage::RasterQueued);
    let illegal = !legal_transition(Stage::New, Stage::Delivered)
        && !legal_transition(Stage::Delivered, Stage::RasterQueued)
        && !legal_transition(Stage::Failed, Stage::Delivered);
    s.add("50104-J1-迁移表", legal && illegal, if legal && illegal { "" } else { "迁移表错误" });

    // J1-2 干净票直通成功：fallback=false、无原因行、计数守恒
    let mut c = Chain::new();
    let clean = base_ticket(1);
    let o = c.run(&clean);
    let ok = match &o {
        Ok(v) => !v.fallback
            && v.artifact.is_none()
            && v.attempts == 0
            && c.stage == Stage::Delivered
            && c.native == 1
            && c.fallback == 0
            && c.ledger.count_reason(FallbackReason::Encrypted) == 0,
        Err(_) => false,
    };
    s.add("50104-J1-直通成功", ok, if ok { "" } else { "干净票走了回退" });

    // J1-3 20 张坏票：输出可达 + 登记留痕，且原因逐张一致
    let (reached, logged, total) = fallback_twenty();
    let ok = reached == total && logged == total && total == 20;
    let det = format!("回退 {}/{} 留痕 {}/{}", reached, total, logged, total);
    s.add_dyn("50104-J1-回退20", ok, &det);

    // J1-4 回退原因不被改写：加密票的账本原因必须是 Encrypted
    let mut c = Chain::new();
    let mut t = base_ticket(21);
    t.flags = F_ENCRYPTED;
    let o = c.run(&t);
    let ok = match (&o, c.ledger.last()) {
        (Ok(v), Some(last)) => {
            v.reason == Some(FallbackReason::Encrypted)
                && last.to == Stage::Delivered
                && c.ledger.count_reason(FallbackReason::Encrypted) >= 3
        }
        _ => false,
    };
    s.add("50104-J1-原因保真", ok, if ok { "" } else { "回退原因被改写" });

    // J1-5 账本序号单调、封账指纹随迁移变化
    let mut c = Chain::new();
    let mut t = base_ticket(22);
    t.flags = F_XREF_BROKEN;
    let r = c.run(&t);
    let mut mono = true;
    let mut i = 0usize;
    while i < c.ledger.entries.len() {
        if let Some(e) = c.ledger.entries.get(i) {
            if e.seq as usize != i + 1 {
                mono = false;
            }
            if i > 0 {
                if let Some(p) = c.ledger.entries.get(i - 1) {
                    if e.seq <= p.seq {
                        mono = false;
                    }
                }
            }
        }
        i += 1;
    }
    let ok = r.is_ok() && mono && c.ledger.seal() != 0 && c.ledger.len() >= 6;
    let det = format!("rows={} seal=0x{:x}", c.ledger.len(), c.ledger.seal());
    s.add_dyn("50104-J1-账本单调", ok, &det);

    // J1-6 标准注册表：6 类自动光栅、Timeout 显式拒绝
    let r = FallbackRegistry::standard();
    let ok = r.len() == 7
        && r.resolve(FallbackReason::Encrypted) == Ok(Action::Rasterize)
        && r.resolve(FallbackReason::XrefBroken) == Ok(Action::Rasterize)
        && r.resolve(FallbackReason::PsLevel) == Ok(Action::Rasterize)
        && r.resolve(FallbackReason::PageRange) == Ok(Action::Rasterize)
        && r.resolve(FallbackReason::Copies) == Ok(Action::Rasterize)
        && r.resolve(FallbackReason::DriverReject) == Ok(Action::Rasterize)
        && r.resolve(FallbackReason::Timeout) == Ok(Action::Reject);
    s.add("50104-J1-注册表覆盖", ok, if ok { "" } else { "注册表缺规则" });

    // J1-7 重复登记显式拒
    let mut r = FallbackRegistry::standard();
    let ok = r.register(FallbackReason::Encrypted, Action::Reject).err() == Some(PtError::Duplicate);
    s.add("50104-J1-重复必拒", ok, if ok { "" } else { "重复登记未拒" });

    // J1-8 Timeout 不自动回退：显式 NoFallback + Failed 终态
    let mut c = Chain::new();
    let mut t = base_ticket(23);
    t.flags = F_TIMEOUT;
    let e = c.run(&t);
    let ok = e.err() == Some(PtError::NoFallback)
        && c.stage == Stage::Failed
        && c.fallback == 0
        && c.failed == 1;
    s.add("50104-J1-超时不回退", ok, if ok { "" } else { "Timeout 被静默回退" });

    // J1-9 光栅件确定性：同票两次运行 id/digest/bytes 全同
    let run_once = |job: u32| -> Option<Artifact> {
        let mut c = Chain::new();
        let mut t = base_ticket(job);
        t.flags = F_DRIVER;
        match c.run(&t) {
            Ok(o) => o.artifact,
            Err(_) => None,
        }
    };
    let a = run_once(31);
    let b = run_once(31);
    let ok = a.is_some() && a == b;
    let det = match &a {
        Some(x) => x.summary(),
        None => "none".to_string(),
    };
    s.add_dyn("50104-J1-件确定", ok, &det);

    // J1-10 件指纹随原因变化（不同原因不得复用同一件）
    let mut c1 = Chain::new();
    let mut t1 = base_ticket(32);
    t1.flags = F_ENCRYPTED;
    let o1 = c1.run(&t1);
    let mut c2 = Chain::new();
    let mut t2 = base_ticket(32);
    t2.flags = F_XREF_BROKEN;
    let o2 = c2.run(&t2);
    let ok = match (&o1, &o2) {
        (Ok(a), Ok(b)) => {
            a.artifact.map(|x| x.id) != b.artifact.map(|x| x.id)
                && a.reason != b.reason
        }
        _ => false,
    };
    s.add("50104-J1-件随因变", ok, if ok { "" } else { "跨原因件碰撞" });

    // J1-11 单次重试：hostile=1 → attempts=2 且最终成功
    let mut c = Chain::new();
    let mut t = base_ticket(33);
    t.flags = F_ENCRYPTED;
    t.hostile = 1;
    let o = c.run(&t);
    let ok = match &o {
        Ok(v) => v.fallback && v.attempts == 2 && v.artifact.is_some() && c.fallback == 1,
        Err(_) => false,
    };
    let det = match &o {
        Ok(v) => format!("attempts={}", v.attempts),
        Err(e) => format!("err=0x{:x}", e.code()),
    };
    s.add_dyn("50104-J1-单次重试", ok, &det);

    // J1-12 两次都失败：RasterFail，且不产出假件
    let mut c = Chain::new();
    let mut t = base_ticket(34);
    t.flags = F_ENCRYPTED;
    t.hostile = 2;
    let e = c.run(&t);
    let ok = e.err() == Some(PtError::RasterFail) && c.failed == 1 && c.fallback == 0;
    s.add("50104-J1-两次失败", ok, if ok { "" } else { "失败被洗成成功" });

    // J1-13 队列 FIFO 且满容显式拒
    let mut q = RasterQueue::new(2);
    let push_ok = q.push(11).is_ok() && q.push(12).is_ok();
    let full = q.push(13).err() == Some(PtError::QueueFull);
    let fifo = q.pop() == Some(11) && q.pop() == Some(12) && q.pop().is_none();
    let ok = push_ok && full && fifo;
    s.add("50104-J1-队列FIFO", ok, if ok { "" } else { "队列语义错" });

    // J1-14 参数边界：150/1200 可用，149/1201 与零页上限不可用
    let v = RasterParams::new(150, ColorMode::Mono, 1).valid()
        && RasterParams::new(1200, ColorMode::Color, 10).valid()
        && !RasterParams::new(149, ColorMode::Gray, 10).valid()
        && !RasterParams::new(1201, ColorMode::Gray, 10).valid()
        && !RasterParams::new(600, ColorMode::Gray, 0).valid();
    s.add("50104-J1-参数边界", v, if v { "" } else { "DPI 边界失效" });

    // J1-15 计数守恒：submitted == native + fallback + failed（成功路径）
    let mut c = Chain::new();
    let mut ok = true;
    let mut job = 40u32;
    while job < 52 {
        let mut t = base_ticket(job);
        if job % 2 == 0 {
            t.flags = F_ENCRYPTED;
        }
        if c.run(&t).is_err() {
            ok = false;
        }
        job += 1;
    }
    ok = ok
        && c.submitted == 12
        && c.submitted == c.native + c.fallback + c.failed
        && c.fallback == 6
        && c.native == 6;
    let det = format!(
        "sub={} native={} fallback={} failed={}",
        c.submitted, c.native, c.fallback, c.failed
    );
    s.add_dyn("50104-J1-计数守恒", ok, &det);

    // J1-16 摘要非空（软件面写回）
    let mut c = Chain::new();
    let mut t = base_ticket(60);
    t.flags = F_RANGE;
    let o = c.run(&t);
    let ok = match &o {
        Ok(v) => {
            let sum = v.summary();
            !sum.is_empty() && sum.contains("fallback=true") && sum.contains("artifact")
        }
        Err(_) => false,
    };
    s.add("50104-J1-摘要非空", ok, if ok { "" } else { "摘要空" });

    // J1-17 11 类注入全抓
    let (caught, total) = injection_core();
    let ok = caught == total && total == 11;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50104-J1-注入11", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50104_checks_green() {
        let set = run_unx50104_checks();
        assert!(set.all_passed(), "UNX-F50104 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50104_fallback_twenty() {
        let (reached, logged, total) = fallback_twenty();
        assert_eq!(total, 20);
        assert_eq!(reached, total);
        assert_eq!(logged, total);
    }

    #[test]
    fn unx50104_injection_11() {
        let (caught, total) = injection_core();
        assert_eq!(total, 11);
        assert_eq!(caught, total);
    }

    #[test]
    fn unx50104_native_clean() {
        let mut c = Chain::new();
        let o = c.run(&base_ticket(1));
        assert!(o.is_ok());
        if let Ok(v) = o {
            assert!(!v.fallback);
            assert!(v.artifact.is_none());
        }
        assert_eq!(c.stage, Stage::Delivered);
    }
}
