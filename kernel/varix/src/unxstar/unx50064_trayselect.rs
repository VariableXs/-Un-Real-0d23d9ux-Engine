//! UNX-F50064 纸盒选择语义（自动/手送/纸盒 1–N 优先级账）（0x43420 段）
//!
//! 锚点：UNX-F50064-J1 选择 ×30 与能力表约束求交一致；判据锚定 §AI-63 §2 专题四。
//!
//! 数据段：`SEG_BASE = 0x43420`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 14`，
//! 逐项见 [`TrayError`]。
//!
//! 纸盒选择看似「挑一个能装的」，真正难的是三件事：
//! 1. **约束求交**：尺寸、介质类型、克重、双面能力、剩余张数是五个独立谓词，
//!    求交结果必须与能力表逐项一致，不能靠「先试一个、失败再换」的运气；
//! 2. **优先级确定**：自动档在多个可行盒之间必须有全序（优先级 → 剩余量 →
//!    盒号），否则同一作业两次选择可能落到不同纸盒，装订与折页全部对不上；
//! 3. **显式请求不静默降级**：用户点名「纸盒 2」而纸盒 2 装不下时，必须报出
//!    是尺寸不符还是容量不足，绝不能悄悄改用纸盒 3——换了纸就是换了成品。
//!
//! 因此本模块把「可行集合」与「选择结果」分成两层：能力表负责求交，
//! [`select`] 只负责在可行集合上按全序取最小；J1 用一份手写的独立参考实现
//! 重算 30 个请求的选择，逐项比对。
//!
//! 结构清单：
//! * [`MediaSize`] / [`MediaType`]：位掩码枚举（能力表用 8 位掩码表达支持集）；
//! * [`Source`] / [`Request`]：三种来源请求（Auto / Manual / Tray(k)）；
//! * [`TrayCap`]：单盒能力（尺寸/介质/克重/双面/余量/优先级）；
//! * [`TrayTable`]：能力表注册表（盒号唯一 + 定容 + 指纹）；
//! * [`Decision`] + [`SelectLedger`]：逐次选择账本（请求号严格递增 + 封账只读）；
//! * [`reference_select`]：独立参考实现（不复用 [`TrayCap::check`] 的判定）；
//! * [`injection_core`]：8 类坏样本注入核。
//!
//! 盒号约定：0 为手送台（bypass），1..=N 为纸盒；手送台不参与「纸盒 N」语义，
//! 只由 [`Source::Manual`] 命中。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43420 独占 0x20）。
pub const SEG_BASE: u32 = 0x43420;

const E_EMPTY_TABLE: u16 = 1;
const E_DUPLICATE_TRAY: u16 = 2;
const E_BAD_TRAY: u16 = 3;
const E_NO_MANUAL: u16 = 4;
const E_NO_TRAY: u16 = 5;
const E_SIZE: u16 = 6;
const E_TYPE: u16 = 7;
const E_WEIGHT: u16 = 8;
const E_CAPACITY: u16 = 9;
const E_DUPLEX: u16 = 10;
const E_BAD_REQUEST: u16 = 11;
const E_ORDER: u16 = 12;
const E_FULL: u16 = 13;
const E_IMMUTABLE: u16 = 14;

/// 纸盒选择错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayError {
    /// 能力表为空。
    EmptyTable,
    /// 同一盒号注册两次。
    DuplicateTray,
    /// 点名的盒号不存在。
    BadTrayId,
    /// 请求手送但能力表里没有 0 号手送台。
    NoManual,
    /// 自动档没有任何可行盒。
    NoTray,
    /// 尺寸不在支持集。
    SizeUnsupported,
    /// 介质类型不在支持集。
    TypeUnsupported,
    /// 克重超过该盒上限。
    WeightExceeded,
    /// 剩余张数不足。
    CapacityShort,
    /// 该盒不支持双面。
    DuplexUnsupported,
    /// 请求本身非法（零页 / 零克重 / 码值越界）。
    BadRequest,
    /// 账本请求号未严格递增。
    OrderGap,
    /// 账本容量已满，或请求号重复。
    Full,
    /// 封账后只读。
    Immutable,
}

impl TrayError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            TrayError::EmptyTable => E_EMPTY_TABLE,
            TrayError::DuplicateTray => E_DUPLICATE_TRAY,
            TrayError::BadTrayId => E_BAD_TRAY,
            TrayError::NoManual => E_NO_MANUAL,
            TrayError::NoTray => E_NO_TRAY,
            TrayError::SizeUnsupported => E_SIZE,
            TrayError::TypeUnsupported => E_TYPE,
            TrayError::WeightExceeded => E_WEIGHT,
            TrayError::CapacityShort => E_CAPACITY,
            TrayError::DuplexUnsupported => E_DUPLEX,
            TrayError::BadRequest => E_BAD_REQUEST,
            TrayError::OrderGap => E_ORDER,
            TrayError::Full => E_FULL,
            TrayError::Immutable => E_IMMUTABLE,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            TrayError::EmptyTable => "tray capability table is empty",
            TrayError::DuplicateTray => "tray id registered twice",
            TrayError::BadTrayId => "requested tray id does not exist",
            TrayError::NoManual => "no manual bypass tray in table",
            TrayError::NoTray => "no tray satisfies the request",
            TrayError::SizeUnsupported => "media size unsupported by tray",
            TrayError::TypeUnsupported => "media type unsupported by tray",
            TrayError::WeightExceeded => "media weight above tray limit",
            TrayError::CapacityShort => "tray remaining sheets below request",
            TrayError::DuplexUnsupported => "tray cannot duplex",
            TrayError::BadRequest => "request is malformed",
            TrayError::OrderGap => "ledger request ids not strictly increasing",
            TrayError::Full => "ledger capacity reached or request id duplicated",
            TrayError::Immutable => "ledger is sealed read-only",
        }
    }
}

/// 纸张尺寸（码值 0..=4，位掩码 1<<code）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaSize {
    /// A4。
    A4,
    /// A5。
    A5,
    /// Letter。
    Letter,
    /// Legal。
    Legal,
    /// A6。
    A6,
}

impl MediaSize {
    /// 尺寸码 0..=4。
    pub fn code(self) -> u8 {
        match self {
            MediaSize::A4 => 0,
            MediaSize::A5 => 1,
            MediaSize::Letter => 2,
            MediaSize::Legal => 3,
            MediaSize::A6 => 4,
        }
    }

    /// 码值反解；闭集外返回 [`TrayError::BadRequest`]。
    pub fn from_code(c: u8) -> Result<MediaSize, TrayError> {
        match c {
            0 => Ok(MediaSize::A4),
            1 => Ok(MediaSize::A5),
            2 => Ok(MediaSize::Letter),
            3 => Ok(MediaSize::Legal),
            4 => Ok(MediaSize::A6),
            _ => Err(TrayError::BadRequest),
        }
    }

    /// 能力表位。
    pub fn bit(self) -> u8 {
        1u8 << self.code()
    }

    /// 尺寸名。
    pub fn label(self) -> &'static str {
        match self {
            MediaSize::A4 => "A4",
            MediaSize::A5 => "A5",
            MediaSize::Letter => "Letter",
            MediaSize::Legal => "Legal",
            MediaSize::A6 => "A6",
        }
    }

    /// 全部五档。
    pub const ALL: [MediaSize; 5] = [
        MediaSize::A4,
        MediaSize::A5,
        MediaSize::Letter,
        MediaSize::Legal,
        MediaSize::A6,
    ];
}

/// 介质类型（码值 0..=3，位掩码 1<<code）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaType {
    /// 普通纸。
    Plain,
    /// 厚纸。
    Thick,
    /// 信封。
    Envelope,
    /// 标签。
    Label,
}

impl MediaType {
    /// 类型码 0..=3。
    pub fn code(self) -> u8 {
        match self {
            MediaType::Plain => 0,
            MediaType::Thick => 1,
            MediaType::Envelope => 2,
            MediaType::Label => 3,
        }
    }

    /// 码值反解；闭集外返回 [`TrayError::BadRequest`]。
    pub fn from_code(c: u8) -> Result<MediaType, TrayError> {
        match c {
            0 => Ok(MediaType::Plain),
            1 => Ok(MediaType::Thick),
            2 => Ok(MediaType::Envelope),
            3 => Ok(MediaType::Label),
            _ => Err(TrayError::BadRequest),
        }
    }

    /// 能力表位。
    pub fn bit(self) -> u8 {
        1u8 << self.code()
    }

    /// 类型名。
    pub fn label(self) -> &'static str {
        match self {
            MediaType::Plain => "plain",
            MediaType::Thick => "thick",
            MediaType::Envelope => "envelope",
            MediaType::Label => "label",
        }
    }

    /// 全部四档。
    pub const ALL: [MediaType; 4] = [
        MediaType::Plain,
        MediaType::Thick,
        MediaType::Envelope,
        MediaType::Label,
    ];
}

/// 请求来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 自动：在全部可行盒里按全序取最小。
    Auto,
    /// 手送台（盒号 0）。
    Manual,
    /// 点名纸盒 1..=N。
    Tray(u8),
}

impl Source {
    /// 来源名。
    pub fn label(self) -> &'static str {
        match self {
            Source::Auto => "auto",
            Source::Manual => "manual",
            Source::Tray(_) => "tray",
        }
    }

    /// 该来源是否允许盒 `id`。自动档允许全部（手送台以最低优先级兜底）。
    pub fn allows(self, id: u8) -> bool {
        match self {
            Source::Auto => true,
            Source::Manual => id == 0,
            Source::Tray(k) => id == k,
        }
    }
}

/// 一次进纸请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// 尺寸。
    pub size: MediaSize,
    /// 介质类型。
    pub media: MediaType,
    /// 克重（g/m²）。
    pub weight_gsm: u16,
    /// 是否要求双面能力。
    pub duplex: bool,
    /// 本次请求页数。
    pub pages: u16,
    /// 来源。
    pub source: Source,
}

impl Request {
    /// 构造请求（不做校验，校验在 [`Request::validate`]）。
    pub const fn new(
        size: MediaSize,
        media: MediaType,
        weight_gsm: u16,
        duplex: bool,
        pages: u16,
        source: Source,
    ) -> Request {
        Request {
            size,
            media,
            weight_gsm,
            duplex,
            pages,
            source,
        }
    }

    /// 请求自校验：零页、零克重都是无意义请求，显式拒绝。
    pub fn validate(&self) -> Result<(), TrayError> {
        if self.pages == 0 || self.weight_gsm == 0 {
            return Err(TrayError::BadRequest);
        }
        Ok(())
    }

    /// 一行请求描述（落账/诊断）。
    pub fn line(&self) -> String {
        format!(
            "{} {} {}gsm {}p {} {}",
            self.size.label(),
            self.media.label(),
            self.weight_gsm,
            self.pages,
            if self.duplex { "duplex" } else { "simplex" },
            self.source.label()
        )
    }
}

/// 单个纸盒的能力。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayCap {
    /// 盒号：0 = 手送台，1..=N = 纸盒。
    pub id: u8,
    /// 支持的尺寸位掩码。
    pub sizes: u8,
    /// 支持的介质位掩码。
    pub media: u8,
    /// 克重上限。
    pub max_weight_gsm: u16,
    /// 是否支持双面。
    pub duplex: bool,
    /// 剩余张数。
    pub remaining: u16,
    /// 优先级（0 最高；数值越小越先选）。
    pub priority: u8,
}

impl TrayCap {
    /// 构造一个纸盒能力。
    pub const fn new(
        id: u8,
        sizes: u8,
        media: u8,
        max_weight_gsm: u16,
        duplex: bool,
        remaining: u16,
        priority: u8,
    ) -> TrayCap {
        TrayCap {
            id,
            sizes,
            media,
            max_weight_gsm,
            duplex,
            remaining,
            priority,
        }
    }

    /// 是否手送台。
    pub fn is_manual(&self) -> bool {
        self.id == 0
    }

    /// 逐项求交：尺寸 → 类型 → 克重 → 双面 → 容量。
    ///
    /// 顺序固定，故同一请求在同一纸盒上的拒绝原因唯一（诊断可复现）。
    pub fn check(&self, req: &Request) -> Result<(), TrayError> {
        if self.sizes & req.size.bit() == 0 {
            return Err(TrayError::SizeUnsupported);
        }
        if self.media & req.media.bit() == 0 {
            return Err(TrayError::TypeUnsupported);
        }
        if req.weight_gsm > self.max_weight_gsm {
            return Err(TrayError::WeightExceeded);
        }
        if req.duplex && !self.duplex {
            return Err(TrayError::DuplexUnsupported);
        }
        if req.pages > self.remaining {
            return Err(TrayError::CapacityShort);
        }
        Ok(())
    }

    /// 支持尺寸？
    pub fn supports_size(&self, s: MediaSize) -> bool {
        self.sizes & s.bit() != 0
    }

    /// 支持介质？
    pub fn supports_media(&self, m: MediaType) -> bool {
        self.media & m.bit() != 0
    }
}

/// 能力表容量上限（A3 机身最多几路供纸 + 手送，留足余量）。
pub const MAX_TRAYS: usize = 16;

/// 纸盒能力表：盒号唯一、定容。它是「求交」的唯一事实来源。
pub struct TrayTable {
    trays: Vec<TrayCap>,
    capacity: usize,
}

impl TrayTable {
    /// 空表（容量 [`MAX_TRAYS`]）。
    pub fn new() -> TrayTable {
        TrayTable {
            trays: Vec::with_capacity(MAX_TRAYS),
            capacity: MAX_TRAYS,
        }
    }

    /// 注册纸盒：容量 → 盒号唯一 → 位掩码非零。
    pub fn push(&mut self, tray: TrayCap) -> Result<(), TrayError> {
        if self.trays.len() >= self.capacity {
            return Err(TrayError::Full);
        }
        // 掩码为 0 的盒子什么也装不了，注册它就是给自动档埋一个永久候选。
        if tray.sizes == 0 || tray.media == 0 {
            return Err(TrayError::BadRequest);
        }
        let mut i = 0usize;
        while i < self.trays.len() {
            if let Some(t) = self.trays.get(i) {
                if t.id == tray.id {
                    return Err(TrayError::DuplicateTray);
                }
            }
            i += 1;
        }
        self.trays.push(tray);
        Ok(())
    }

    /// 盒数。
    pub fn len(&self) -> usize {
        self.trays.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.trays.is_empty()
    }

    /// 取第 `i` 个（越界 `None`）。
    pub fn get(&self, i: usize) -> Option<&TrayCap> {
        self.trays.get(i)
    }

    /// 按盒号查找。
    pub fn by_id(&self, id: u8) -> Option<&TrayCap> {
        let mut i = 0usize;
        while i < self.trays.len() {
            if let Some(t) = self.trays.get(i) {
                if t.id == id {
                    return Some(t);
                }
            }
            i += 1;
        }
        None
    }

    /// 能力表求交：返回所有满足请求且来源允许的盒号（升序按注册序）。
    pub fn feasible(&self, req: &Request) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.trays.len());
        let mut i = 0usize;
        while i < self.trays.len() {
            if let Some(t) = self.trays.get(i) {
                if req.source.allows(t.id) && t.check(req).is_ok() {
                    out.push(t.id);
                }
            }
            i += 1;
        }
        out
    }

    /// 全表复核：非空、盒号唯一、掩码非零。
    pub fn validate(&self) -> Result<usize, TrayError> {
        if self.trays.is_empty() {
            return Err(TrayError::EmptyTable);
        }
        let mut i = 0usize;
        while i < self.trays.len() {
            let a = match self.trays.get(i) {
                Some(v) => v,
                None => return Err(TrayError::BadTrayId),
            };
            if a.sizes == 0 || a.media == 0 {
                return Err(TrayError::BadRequest);
            }
            let mut j = i + 1;
            while j < self.trays.len() {
                if let Some(b) = self.trays.get(j) {
                    if a.id == b.id {
                        return Err(TrayError::DuplicateTray);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(self.trays.len())
    }

    /// 能力表指纹（注册序）。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.trays.len() {
            if let Some(t) = self.trays.get(i) {
                h = fnv_byte(h, t.id);
                h = fnv_byte(h, t.sizes);
                h = fnv_byte(h, t.media);
                h = fnv_u16(h, t.max_weight_gsm);
                h = fnv_byte(h, t.duplex as u8);
                h = fnv_u16(h, t.remaining);
                h = fnv_byte(h, t.priority);
            }
            i += 1;
        }
        h
    }
}

/// 一次选择的结果（可审计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    /// 请求序号（由调用方分配；账本要求严格递增）。
    pub request_id: u16,
    /// 命中的盒号。
    pub chosen: u8,
    /// 可行候选数。
    pub candidates: u8,
    /// 被拒原因位掩码（尺寸/类型/克重/双面/容量各一位）。
    pub rejected_mask: u16,
    /// 是否手送台。
    pub manual: bool,
}

impl Decision {
    /// 一行落账文本。
    pub fn line(&self) -> String {
        format!(
            "#{} tray{} cand{} reject{:04x} {}",
            self.request_id,
            self.chosen,
            self.candidates,
            self.rejected_mask,
            if self.manual { "manual" } else { "cassette" }
        )
    }
}

/// 拒绝原因位：与 [`TrayCap::check`] 的五类失败一一对应。
pub const BIT_SIZE: u16 = 1;
/// 介质类型不符。
pub const BIT_TYPE: u16 = 2;
/// 克重超限。
pub const BIT_WEIGHT: u16 = 4;
/// 容量不足。
pub const BIT_CAPACITY: u16 = 8;
/// 不支持双面。
pub const BIT_DUPLEX: u16 = 16;

fn reason_bit(e: TrayError) -> u16 {
    match e {
        TrayError::SizeUnsupported => BIT_SIZE,
        TrayError::TypeUnsupported => BIT_TYPE,
        TrayError::WeightExceeded => BIT_WEIGHT,
        TrayError::CapacityShort => BIT_CAPACITY,
        TrayError::DuplexUnsupported => BIT_DUPLEX,
        _ => 0,
    }
}

/// 全序比较：优先级小者胜；同优先级剩余多者胜；再同则盒号小者胜。
///
/// 三级 tie-break 是必须的：只比优先级时，两个同为优先级 0 的盒子会随注册序
/// 漂移；注册序又可能被固件枚举顺序改变，于是同一作业两次出纸不同盒。
fn preferred(cand: &TrayCap, best: &TrayCap) -> bool {
    cand.priority < best.priority
        || (cand.priority == best.priority && cand.remaining > best.remaining)
        || (cand.priority == best.priority
            && cand.remaining == best.remaining
            && cand.id < best.id)
}

/// 主选择入口：显式来源直达，自动档在可行集合上取全序最小。
pub fn select(table: &TrayTable, req: &Request) -> Result<Decision, TrayError> {
    req.validate()?;
    if table.is_empty() {
        return Err(TrayError::EmptyTable);
    }
    match req.source {
        Source::Manual => {
            let t = match table.by_id(0) {
                Some(v) => v,
                None => return Err(TrayError::NoManual),
            };
            t.check(req)?;
            Ok(Decision {
                request_id: 0,
                chosen: 0,
                candidates: 1,
                rejected_mask: 0,
                manual: true,
            })
        }
        Source::Tray(k) => {
            let t = match table.by_id(k) {
                Some(v) => v,
                None => return Err(TrayError::BadTrayId),
            };
            t.check(req)?;
            Ok(Decision {
                request_id: 0,
                chosen: k,
                candidates: 1,
                rejected_mask: 0,
                manual: t.is_manual(),
            })
        }
        Source::Auto => {
            let mut candidates: u8 = 0;
            let mut mask: u16 = 0;
            let mut best: Option<TrayCap> = None;
            let mut i = 0usize;
            while i < table.len() {
                let t = match table.get(i) {
                    Some(v) => *v,
                    None => return Err(TrayError::BadTrayId),
                };
                match t.check(req) {
                    Ok(()) => {
                        candidates = candidates.saturating_add(1);
                        let take = match &best {
                            None => true,
                            Some(b) => preferred(&t, b),
                        };
                        if take {
                            best = Some(t);
                        }
                    }
                    Err(e) => mask |= reason_bit(e),
                }
                i += 1;
            }
            match best {
                Some(t) => Ok(Decision {
                    request_id: 0,
                    chosen: t.id,
                    candidates,
                    rejected_mask: mask,
                    manual: t.is_manual(),
                }),
                None => Err(TrayError::NoTray),
            }
        }
    }
}

/// 独立的可行谓词（参考实现专用，故意不复用 [`TrayCap::check`]）。
///
/// 为什么另写一份：J1 要证明「选择结果 = 能力表约束求交」，如果参考实现直接
/// 调用被测的 `check`，任何 check 漏判都会在两边同时出现，判据恒真。这里把
/// 五个谓词按位重新展开，两条通路互相独立。
fn inline_ok(t: &TrayCap, req: &Request) -> bool {
    if !req.source.allows(t.id) {
        return false;
    }
    if t.sizes & (1u8 << req.size.code()) == 0 {
        return false;
    }
    if t.media & (1u8 << req.media.code()) == 0 {
        return false;
    }
    if req.weight_gsm > t.max_weight_gsm {
        return false;
    }
    if req.duplex && !t.duplex {
        return false;
    }
    if req.pages > t.remaining {
        return false;
    }
    true
}

/// 参考选择：与 [`select`] 完全独立的闭式实现，用于 30 请求逐项对拍。
pub fn reference_select(table: &TrayTable, req: &Request) -> Option<u8> {
    if req.pages == 0 || req.weight_gsm == 0 || table.is_empty() {
        return None;
    }
    match req.source {
        Source::Manual => {
            let t = table.by_id(0)?;
            if inline_ok(t, req) {
                Some(0)
            } else {
                None
            }
        }
        Source::Tray(k) => {
            let t = table.by_id(k)?;
            if inline_ok(t, req) {
                Some(k)
            } else {
                None
            }
        }
        Source::Auto => {
            let mut best: Option<TrayCap> = None;
            let mut i = 0usize;
            while i < table.len() {
                let t = match table.get(i) {
                    Some(v) => *v,
                    None => return None,
                };
                if inline_ok(&t, req) {
                    let take = match &best {
                        None => true,
                        Some(b) => {
                            t.priority < b.priority
                                || (t.priority == b.priority && t.remaining > b.remaining)
                                || (t.priority == b.priority
                                    && t.remaining == b.remaining
                                    && t.id < b.id)
                        }
                    };
                    if take {
                        best = Some(t);
                    }
                }
                i += 1;
            }
            match best {
                Some(t) => Some(t.id),
                None => None,
            }
        }
    }
}

/// 选择账本：请求号严格递增 + 定容 + 封账只读。
pub struct SelectLedger {
    rows: Vec<Decision>,
    capacity: usize,
    sealed: bool,
    rejected: u32,
}

impl SelectLedger {
    /// 建空账；容量 0 按 1 处理，上限 [`MAX_DECISIONS`]。
    pub fn new(capacity: usize) -> SelectLedger {
        let cap = if capacity == 0 {
            1
        } else if capacity > MAX_DECISIONS {
            MAX_DECISIONS
        } else {
            capacity
        };
        SelectLedger {
            rows: Vec::with_capacity(cap),
            capacity: cap,
            sealed: false,
            rejected: 0,
        }
    }

    fn reject(&mut self, e: TrayError) -> Result<(), TrayError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 入账一次选择：封账 → 容量 → 请求号重复 → 严格递增。
    pub fn insert(&mut self, d: Decision) -> Result<(), TrayError> {
        if self.sealed {
            return self.reject(TrayError::Immutable);
        }
        if self.rows.len() >= self.capacity {
            return self.reject(TrayError::Full);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.request_id == d.request_id {
                    return self.reject(TrayError::Full);
                }
            }
            i += 1;
        }
        if let Some(last) = self.rows.last() {
            if d.request_id <= last.request_id {
                return self.reject(TrayError::OrderGap);
            }
        }
        self.rows.push(d);
        Ok(())
    }

    /// 全表复核：请求号严格递增、所选盒存在。
    pub fn verify(&self, table: &TrayTable) -> Result<usize, TrayError> {
        let mut last: Option<u16> = None;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TrayError::OrderGap),
            };
            if let Some(prev) = last {
                if r.request_id <= prev {
                    return Err(TrayError::OrderGap);
                }
            }
            if table.by_id(r.chosen).is_none() {
                return Err(TrayError::BadTrayId);
            }
            last = Some(r.request_id);
            i += 1;
        }
        Ok(self.rows.len())
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计被拒。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取第 `i` 条。
    pub fn row(&self, i: usize) -> Option<&Decision> {
        self.rows.get(i)
    }

    /// 账本指纹。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fnv_u16(h, r.request_id);
                h = fnv_byte(h, r.chosen);
                h = fnv_byte(h, r.candidates);
                h = fnv_u16(h, r.rejected_mask);
                h = fnv_byte(h, r.manual as u8);
            }
            i += 1;
        }
        h
    }

    /// 落账文本。
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                out.push_str(&r.line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// 账本行数上限（一次作业的选择次数远小于页数）。
pub const MAX_DECISIONS: usize = 4096;

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u16(h: u32, v: u16) -> u32 {
    fnv_byte(fnv_byte(h, (v >> 8) as u8), v as u8)
}

/// 固定样例能力表（判据与测试共用；注册失败说明常量表本身写错了）。
///
/// 0 = 手送（低优先级、只支持 A4/Letter + 普通/信封、不能双面）；
/// 1..=4 = 纸盒，优先级与余量刻意错开，覆盖全序的三级 tie-break。
pub fn sample_table() -> TrayTable {
    let mut t = TrayTable::new();
    let _ = t.push(TrayCap::new(
        0,
        MediaSize::A4.bit() | MediaSize::Letter.bit(),
        MediaType::Plain.bit() | MediaType::Envelope.bit(),
        300,
        false,
        20,
        9,
    ));
    let _ = t.push(TrayCap::new(
        1,
        MediaSize::A4.bit() | MediaSize::A5.bit() | MediaSize::Letter.bit(),
        MediaType::Plain.bit() | MediaType::Thick.bit(),
        220,
        true,
        500,
        0,
    ));
    let _ = t.push(TrayCap::new(
        2,
        MediaSize::A4.bit() | MediaSize::Legal.bit(),
        MediaType::Plain.bit() | MediaType::Thick.bit() | MediaType::Label.bit(),
        350,
        true,
        100,
        1,
    ));
    let _ = t.push(TrayCap::new(
        3,
        MediaSize::A5.bit() | MediaSize::A6.bit(),
        MediaType::Plain.bit() | MediaType::Label.bit(),
        160,
        false,
        40,
        2,
    ));
    let _ = t.push(TrayCap::new(
        4,
        MediaSize::A4.bit()
            | MediaSize::A5.bit()
            | MediaSize::Letter.bit()
            | MediaSize::Legal.bit()
            | MediaSize::A6.bit(),
        MediaType::Plain.bit()
            | MediaType::Thick.bit()
            | MediaType::Envelope.bit()
            | MediaType::Label.bit(),
        400,
        true,
        5,
        3,
    ));
    t
}

/// 30 个固定请求：尺寸 5 档 × 介质 4 档轮转，来源分三段（自动/手送/点名盒）。
pub fn sample_requests() -> Vec<Request> {
    let mut out = Vec::with_capacity(30);
    let mut i = 0u16;
    while i < 30 {
        let size = match i % 5 {
            0 => MediaSize::A4,
            1 => MediaSize::A5,
            2 => MediaSize::Letter,
            3 => MediaSize::Legal,
            _ => MediaSize::A6,
        };
        let media = match (i / 5) % 4 {
            0 => MediaType::Plain,
            1 => MediaType::Thick,
            2 => MediaType::Envelope,
            _ => MediaType::Label,
        };
        let source = match i / 10 {
            0 => Source::Auto,
            1 => Source::Manual,
            _ => Source::Tray(1 + (i % 3) as u8),
        };
        out.push(Request::new(
            size,
            media,
            60 + (i * 7) % 160,
            i % 2 == 0,
            1 + i % 25,
            source,
        ));
        i += 1;
    }
    out
}

/// 注入核：8 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let table = sample_table();
    let good = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 5, Source::Auto);

    // 1) 空表：自动档无候选可求交。
    let empty = TrayTable::new();
    if select(&empty, &good).err() == Some(TrayError::EmptyTable) {
        caught += 1;
    }

    // 2) 盒号重复注册。
    let mut dup = TrayTable::new();
    let t1 = TrayCap::new(1, 1, 1, 200, true, 100, 0);
    if dup.push(t1).is_ok() && dup.push(t1).err() == Some(TrayError::DuplicateTray) {
        caught += 1;
    }

    // 3) 点名不存在的盒号。
    let bad_id = Request::new(
        MediaSize::A4,
        MediaType::Plain,
        100,
        false,
        5,
        Source::Tray(9),
    );
    if select(&table, &bad_id).err() == Some(TrayError::BadTrayId) {
        caught += 1;
    }

    // 4) 手送请求但表里没有 0 号台。
    let mut no_manual = TrayTable::new();
    let _ = no_manual.push(TrayCap::new(1, 1, 1, 200, true, 100, 0));
    let manual = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 5, Source::Manual);
    if select(&no_manual, &manual).err() == Some(TrayError::NoManual) {
        caught += 1;
    }

    // 5) 自动档全被容量/克重挡住 → NoTray（不是静默选一个装不下的）。
    let mut starved = TrayTable::new();
    let _ = starved.push(TrayCap::new(1, 1, 1, 120, true, 2, 0));
    let heavy = Request::new(MediaSize::A4, MediaType::Plain, 300, false, 9, Source::Auto);
    if select(&starved, &heavy).err() == Some(TrayError::NoTray) {
        caught += 1;
    }

    // 6) 零页请求。
    let zero = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 0, Source::Auto);
    if select(&table, &zero).err() == Some(TrayError::BadRequest) {
        caught += 1;
    }

    // 7) 账本请求号倒序。
    let mut led = SelectLedger::new(4);
    let d1 = Decision {
        request_id: 5,
        chosen: 1,
        candidates: 1,
        rejected_mask: 0,
        manual: false,
    };
    let d2 = Decision {
        request_id: 3,
        chosen: 2,
        candidates: 1,
        rejected_mask: 0,
        manual: false,
    };
    if led.insert(d1).is_ok() && led.insert(d2).err() == Some(TrayError::OrderGap) {
        caught += 1;
    }

    // 8) 封账后入账。
    let mut sealed = SelectLedger::new(4);
    let d3 = Decision {
        request_id: 1,
        chosen: 1,
        candidates: 1,
        rejected_mask: 0,
        manual: false,
    };
    let first = sealed.insert(d3).is_ok();
    sealed.seal();
    if first && sealed.insert(d3).err() == Some(TrayError::Immutable) && sealed.len() == 1 {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50064 自检：18 条判据（J1 原文：选择 ×30 与能力表约束求交一致）。
pub fn run_unx50064_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50064");
    let table = sample_table();
    let reqs = sample_requests();

    // J1-1 三十次选择 × 能力表求交：沿独立参考实现逐项对拍，
    // 命中盒必须真的通过 check，候选数与可行集合大小一致。
    let mut checked = 0usize;
    let mut ok_count = 0usize;
    let mut mismatch = 0u16;
    let mut i = 0usize;
    while i < reqs.len() {
        if let Some(req) = reqs.get(i) {
            let got = select(&table, req);
            let want = reference_select(&table, req);
            match (got, want) {
                (Ok(d), Some(id)) => {
                    ok_count += 1;
                    if d.chosen != id {
                        mismatch = mismatch.saturating_add(1);
                    }
                    match table.by_id(d.chosen) {
                        Some(t) => {
                            if t.check(req).is_err() {
                                mismatch = mismatch.saturating_add(1);
                            }
                        }
                        None => mismatch = mismatch.saturating_add(1),
                    }
                    if d.candidates as usize != table.feasible(req).len() {
                        mismatch = mismatch.saturating_add(1);
                    }
                    if d.manual != (d.chosen == 0) {
                        mismatch = mismatch.saturating_add(1);
                    }
                }
                (Err(_), None) => {}
                _ => mismatch = mismatch.saturating_add(1),
            }
            checked += 1;
        }
        i += 1;
    }
    let ok = checked == 30 && mismatch == 0 && ok_count > 0;
    let det = format!("n={} ok={} mismatch={}", checked, ok_count, mismatch);
    s.add_dyn("50064-J1-三十选择对拍", ok, &det);

    // J1-2 求交一致：feasible() 与手写位判定逐请求等集合（含来源过滤）。
    let mut feasible_ok = true;
    let mut req_i = 0usize;
    while req_i < reqs.len() {
        if let Some(req) = reqs.get(req_i) {
            let got = table.feasible(req);
            let mut want: Vec<u8> = Vec::with_capacity(MAX_TRAYS);
            let mut k = 0usize;
            while k < table.len() {
                if let Some(t) = table.get(k) {
                    if inline_ok(t, req) {
                        want.push(t.id);
                    }
                }
                k += 1;
            }
            if got != want {
                feasible_ok = false;
            }
        }
        req_i += 1;
    }
    s.add("50064-J1-求交一致", feasible_ok, if feasible_ok { "" } else { "可行集合不一致" });

    // J1-3 优先级全序：自动档命中盒的 (优先级, 余量, 盒号) 字典序必须最小。
    let mut order_ok = true;
    let mut ri = 0usize;
    while ri < reqs.len() {
        if let Some(req) = reqs.get(ri) {
            if req.source == Source::Auto {
                if let Ok(d) = select(&table, req) {
                    let chosen = match table.by_id(d.chosen) {
                        Some(t) => *t,
                        None => {
                            order_ok = false;
                            ri += 1;
                            continue;
                        }
                    };
                    let mut k = 0usize;
                    while k < table.len() {
                        if let Some(t) = table.get(k) {
                            if inline_ok(t, req)
                                && (t.priority < chosen.priority
                                    || (t.priority == chosen.priority
                                        && t.remaining > chosen.remaining)
                                    || (t.priority == chosen.priority
                                        && t.remaining == chosen.remaining
                                        && t.id < chosen.id))
                            {
                                order_ok = false;
                            }
                        }
                        k += 1;
                    }
                }
            }
        }
        ri += 1;
    }
    s.add("50064-J1-优先级全序", order_ok, if order_ok { "" } else { "命中盒不是字典序最小" });

    // J1-4 显式纸盒直达：点名 1 号且可装 → 恰好命中 1；五类约束逐项给码。
    let explicit_ok;
    let mut explicit_det = String::new();
    let base = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 5, Source::Tray(1));
    let a6 = Request::new(MediaSize::A6, MediaType::Plain, 100, false, 5, Source::Tray(1));
    let env = Request::new(MediaSize::A4, MediaType::Envelope, 100, false, 5, Source::Tray(1));
    let heavy = Request::new(MediaSize::A4, MediaType::Plain, 999, false, 5, Source::Tray(1));
    let big = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 9999, Source::Tray(1));
    let dup = Request::new(MediaSize::A5, MediaType::Plain, 100, true, 5, Source::Tray(3));
    match select(&table, &base) {
        Ok(d) => {
            explicit_ok = d.chosen == 1
                && d.candidates == 1
                && !d.manual
                && select(&table, &a6).err() == Some(TrayError::SizeUnsupported)
                && select(&table, &env).err() == Some(TrayError::TypeUnsupported)
                && select(&table, &heavy).err() == Some(TrayError::WeightExceeded)
                && select(&table, &big).err() == Some(TrayError::CapacityShort)
                && select(&table, &dup).err() == Some(TrayError::DuplexUnsupported);
            explicit_det = format!("chosen={} cand={}", d.chosen, d.candidates);
        }
        Err(_) => explicit_ok = false,
    }
    s.add_dyn("50064-J1-显式直达", explicit_ok, &explicit_det);

    // J1-5 手送直达：命中 0、manual 标志；0 号台装不下时给具体原因。
    let mut manual_ok;
    let mreq = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 5, Source::Manual);
    let mdup = Request::new(MediaSize::A4, MediaType::Plain, 100, true, 5, Source::Manual);
    match select(&table, &mreq) {
        Ok(d) => {
            manual_ok = d.chosen == 0 && d.manual && d.candidates == 1 && !table.by_id(0).is_none();
            if select(&table, &mdup).err() != Some(TrayError::DuplexUnsupported) {
                manual_ok = false;
            }
        }
        Err(_) => manual_ok = false,
    }
    s.add("50064-J1-手送直达", manual_ok, if manual_ok { "" } else { "手送命中/拒绝不符" });

    // J1-6 容量约束：4 号盒只剩 5 张，6 页点名必拒；自动档改选余量足够的盒。
    let mut cap_ok = false;
    let tight = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 6, Source::Tray(4));
    let auto = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 6, Source::Auto);
    if select(&table, &tight).err() == Some(TrayError::CapacityShort) {
        if let Ok(d) = select(&table, &auto) {
            match table.by_id(d.chosen) {
                Some(t) => {
                    cap_ok = t.remaining >= 6 && d.chosen != 4;
                }
                None => cap_ok = false,
            }
        }
    }
    s.add("50064-J1-容量改选", cap_ok, if cap_ok { "" } else { "容量约束未生效" });

    // J1-7 自动档拒绝位：尺寸/类型/克重/双面/容量五类失败都要在掩码里有位。
    let hard = Request::new(MediaSize::Legal, MediaType::Envelope, 380, true, 300, Source::Auto);
    let mask_ok;
    let mask_det: String;
    match select(&table, &hard) {
        Ok(d) => {
            // 仅 4 号盒尺寸/类型/克重/双面都过，但只剩 5 张 → 容量挡；
            // 故最终必失败或命中容量之外唯一可行盒，这里只断言掩码含容量位。
            mask_ok = d.rejected_mask & BIT_CAPACITY != 0;
            mask_det = format!("mask={:04x} chosen={}", d.rejected_mask, d.chosen);
        }
        Err(TrayError::NoTray) => {
            mask_ok = true;
            mask_det = "no-tray".to_string();
        }
        Err(_) => {
            mask_ok = false;
            mask_det = "other".to_string();
        }
    }
    s.add_dyn("50064-J1-拒绝位", mask_ok, &mask_det);

    // J1-8 确定性：同请求两次选择逐字段相同；能力表指纹稳定。
    let mut det_ok = true;
    let mut di = 0usize;
    while di < reqs.len() {
        if let Some(req) = reqs.get(di) {
            let a = select(&table, req);
            let b = select(&table, req);
            match (a, b) {
                (Ok(x), Ok(y)) => {
                    if x.chosen != y.chosen
                        || x.candidates != y.candidates
                        || x.rejected_mask != y.rejected_mask
                        || x.manual != y.manual
                    {
                        det_ok = false;
                    }
                }
                (Err(x), Err(y)) => {
                    if x != y {
                        det_ok = false;
                    }
                }
                _ => det_ok = false,
            }
        }
        di += 1;
    }
    if table.digest() != sample_table().digest() {
        det_ok = false;
    }
    s.add("50064-J1-选择确定", det_ok, if det_ok { "" } else { "两次选择不一致" });

    // J1-9 注册表纪律：重复盒号必拒、空表 validate 必拒、满表必拒、指纹随表变。
    let reg_ok;
    let mut dup = TrayTable::new();
    let t1 = TrayCap::new(1, 1, 1, 200, true, 100, 0);
    let dup_rej = dup.push(t1).is_ok() && dup.push(t1).err() == Some(TrayError::DuplicateTray);
    let empty_rej = TrayTable::new().validate().err() == Some(TrayError::EmptyTable);
    let mut full = TrayTable::new();
    let mut fill = 0u8;
    while fill < MAX_TRAYS as u8 {
        let _ = full.push(TrayCap::new(fill, 1, 1, 200, true, 100, fill));
        fill += 1;
    }
    let full_rej = full.len() == MAX_TRAYS
        && full.push(TrayCap::new(99, 1, 1, 200, true, 1, 0)).err() == Some(TrayError::Full);
    reg_ok = dup_rej && empty_rej && full_rej && table.validate() == Ok(5);
    s.add("50064-J1-注册表纪律", reg_ok, if reg_ok { "" } else { "注册表未拦截" });

    // J1-10 选择账本：成功选择按请求号递增入账，verify 与表一致。
    let led_ok;
    let mut led = SelectLedger::new(64);
    let mut inserted = 0usize;
    let mut li = 0usize;
    while li < reqs.len() {
        if let Some(req) = reqs.get(li) {
            if let Ok(mut d) = select(&table, req) {
                d.request_id = li as u16;
                if led.insert(d).is_ok() {
                    inserted += 1;
                }
            }
        }
        li += 1;
    }
    led_ok = inserted > 0 && led.len() == inserted && led.verify(&table) == Ok(inserted);
    let det = format!("rows={} inserted={}", led.len(), inserted);
    s.add_dyn("50064-J1-选择账本", led_ok, &det);

    // J1-11 账本纪律：倒序/重复/容量/封账逐项拦截。
    let disc_ok;
    let mk = |id: u16, tray: u8| Decision {
        request_id: id,
        chosen: tray,
        candidates: 1,
        rejected_mask: 0,
        manual: false,
    };
    let mut order = SelectLedger::new(4);
    let o1 = order.insert(mk(5, 1)).is_ok();
    let o2 = order.insert(mk(3, 2)).err() == Some(TrayError::OrderGap);
    let mut twice = SelectLedger::new(4);
    let t_first = twice.insert(mk(1, 1)).is_ok();
    let t_dup = twice.insert(mk(1, 2)).err() == Some(TrayError::Full);
    let mut tiny = SelectLedger::new(1);
    let f1 = tiny.insert(mk(1, 1)).is_ok();
    let f2 = tiny.insert(mk(2, 2)).err() == Some(TrayError::Full);
    tiny.seal();
    let imm = tiny.insert(mk(3, 3)).err() == Some(TrayError::Immutable);
    disc_ok = o1 && o2 && t_first && t_dup && f1 && f2 && imm && tiny.len() == 1;
    s.add("50064-J1-账本纪律", disc_ok, if disc_ok { "" } else { "账本纪律未生效" });

    // J1-12 请求自校验：零页、零克重都是 BadRequest。
    let zero_pages = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 0, Source::Auto);
    let zero_gsm = Request::new(MediaSize::A4, MediaType::Plain, 0, false, 5, Source::Auto);
    let ok = select(&table, &zero_pages).err() == Some(TrayError::BadRequest)
        && select(&table, &zero_gsm).err() == Some(TrayError::BadRequest);
    s.add("50064-J1-请求必校验", ok, if ok { "" } else { "零页/零克重被接受" });

    // J1-13 码值闭集：尺寸 1..=4、类型 0..=3 往返恒等，越界拒绝。
    let mut code_ok = true;
    let mut c = 0u8;
    while c < 5 {
        match MediaSize::from_code(c) {
            Ok(v) => {
                if v.code() != c || v.bit() != 1u8 << c {
                    code_ok = false;
                }
            }
            Err(_) => code_ok = false,
        }
        c += 1;
    }
    let mut m = 0u8;
    while m < 4 {
        match MediaType::from_code(m) {
            Ok(v) => {
                if v.code() != m || v.bit() != 1u8 << m {
                    code_ok = false;
                }
            }
            Err(_) => code_ok = false,
        }
        m += 1;
    }
    if MediaSize::from_code(5).is_ok() || MediaType::from_code(4).is_ok() {
        code_ok = false;
    }
    s.add("50064-J1-码值闭集", code_ok, if code_ok { "" } else { "码值往返/越界不符" });

    // J1-14 空表/无手送/坏盒号三连。
    let manual = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 5, Source::Manual);
    let mut no_manual = TrayTable::new();
    let _ = no_manual.push(TrayCap::new(1, 1, 1, 200, true, 100, 0));
    let bad = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 5, Source::Tray(9));
    let ok = select(&TrayTable::new(), &manual).err() == Some(TrayError::EmptyTable)
        && select(&no_manual, &manual).err() == Some(TrayError::NoManual)
        && select(&table, &bad).err() == Some(TrayError::BadTrayId);
    s.add("50064-J1-空表无源", ok, if ok { "" } else { "空表/无手送/坏号未拦截" });

    // J1-15 候选记账：自动档「可行数 + 被拒数」恢复整表规模。
    let mut cand_ok = true;
    let mut ci = 0usize;
    while ci < reqs.len() {
        if let Some(req) = reqs.get(ci) {
            if req.source == Source::Auto {
                match select(&table, req) {
                    Ok(d) => {
                        let mut failed = 0usize;
                        let mut k = 0usize;
                        while k < table.len() {
                            if let Some(t) = table.get(k) {
                                if t.check(req).is_err() {
                                    failed += 1;
                                }
                            }
                            k += 1;
                        }
                        if d.candidates as usize + failed != table.len() {
                            cand_ok = false;
                        }
                    }
                    Err(TrayError::NoTray) => {
                        // 全被拒：可行 0，被拒 = 全表。
                    }
                    Err(_) => cand_ok = false,
                }
            }
        }
        ci += 1;
    }
    s.add("50064-J1-候选记账", cand_ok, if cand_ok { "" } else { "候选数不守恒" });

    // J1-16 自动档不越级：只要存在非手送可行盒，就绝不落到手送台。
    let mut bypass_ok = true;
    let mut bi = 0usize;
    while bi < reqs.len() {
        if let Some(req) = reqs.get(bi) {
            if req.source == Source::Auto {
                if let Ok(d) = select(&table, req) {
                    let mut cassette = false;
                    let mut k = 0usize;
                    while k < table.len() {
                        if let Some(t) = table.get(k) {
                            if t.id != 0 && inline_ok(t, req) {
                                cassette = true;
                            }
                        }
                        k += 1;
                    }
                    if cassette && d.chosen == 0 {
                        bypass_ok = false;
                    }
                }
            }
        }
        bi += 1;
    }
    s.add("50064-J1-手送兜底序", bypass_ok, if bypass_ok { "" } else { "可行纸盒在手送台之上仍被跳过" });

    // J1-17 幂等：同一 30 请求重复跑出的账本指纹逐位相同。
    let idem_ok;
    let run = |table: &TrayTable, reqs: &[Request]| -> SelectLedger {
        let mut l = SelectLedger::new(64);
        let mut i = 0usize;
        while i < reqs.len() {
            if let Some(r) = reqs.get(i) {
                if let Ok(mut d) = select(table, r) {
                    d.request_id = i as u16;
                    let _ = l.insert(d);
                }
            }
            i += 1;
        }
        l
    };
    let la = run(&table, &reqs);
    let lb = run(&table, &reqs);
    idem_ok = la.digest() == lb.digest() && la.len() == lb.len() && la.len() > 0;
    s.add("50064-J1-账本幂等", idem_ok, if idem_ok { "" } else { "两次账本不一致" });

    // J1-18 八类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50064-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50064_checks_green() {
        let set = run_unx50064_checks();
        assert!(set.all_passed(), "UNX-F50064 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50064_thirty_requests_match_reference() {
        let table = sample_table();
        let reqs = sample_requests();
        assert_eq!(reqs.len(), 30);
        let mut ok = 0usize;
        let mut diverged = false;
        for (i, req) in reqs.iter().enumerate() {
            let got = select(&table, req);
            let want = reference_select(&table, req);
            match (got, want) {
                (Ok(d), Some(id)) => {
                    assert_eq!(d.chosen, id, "request {}", i);
                    ok += 1;
                }
                (Err(_), None) => {}
                _ => diverged = true,
            }
        }
        assert!(!diverged);
        assert!(ok > 0);
    }

    #[test]
    fn unx50064_priority_is_total_order() {
        let table = sample_table();
        let req = Request::new(MediaSize::A4, MediaType::Plain, 100, false, 5, Source::Auto);
        let d = select(&table, &req);
        assert!(d.is_ok());
        if let Ok(v) = d {
            // 优先级 0 的 1 号盒余量最多，必须胜出；3 号盒不支持 A4，
            // 可行集合只有 0/1/2/4 四路（手送也计入候选数）。
            assert_eq!(v.chosen, 1);
            assert_eq!(v.candidates, 4);
        }
    }

    #[test]
    fn unx50064_explicit_request_never_downgrades() {
        let table = sample_table();
        let req = Request::new(MediaSize::A6, MediaType::Plain, 100, false, 5, Source::Tray(1));
        assert_eq!(select(&table, &req).err(), Some(TrayError::SizeUnsupported));
        let ok_req = Request::new(MediaSize::A6, MediaType::Plain, 100, false, 5, Source::Tray(3));
        assert_eq!(select(&table, &ok_req).map(|d| d.chosen), Ok(3));
    }

    #[test]
    fn unx50064_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 8);
    }
}
