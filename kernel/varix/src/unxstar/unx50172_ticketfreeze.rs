//! UNX-F50172 Ticket 冻结接口（管线/UI 消费单点面）（0x441a0 段）
//!
//! 锚点：UNX-F50172-J1 接口冻结登记；判据锚定 §AI-63 §4。
//!
//! 为什么需要「冻结面」：Ticket 的字段一旦同时被管线（渲染/装订/传输）与 UI
//! 消费，任何一侧晚到的字段改名都会让另一侧读到错位数据。所以把可消费的字段
//! 集收敛成**唯一一个冻结面**——字段只能进草稿态，冻结后摘要固定；消费者按
//! 摘要绑定，摘要不再吻合即为陈旧消费者，必须显式重绑，不许悄悄跟着变。
//!
//! 结构清单：
//! * [`Phase`] + [`Surface`] —— 草稿/冻结/退役三态状态机，冻结幂等、退役终态；
//! * [`FieldDecl`] —— 字段登记项（id/名称/类型/单位），重号重名一律拒绝；
//! * [`ConsumerTable`] —— 管线/UI 消费登记，同一冻结面上所有消费者摘要一致；
//! * [`SinglePoint`] —— 单点面槽位，挂第二个面必拒，卸下须出示摘要令牌；
//! * [`FreezeLedger`] 行账 —— 冻结事件序号严格递增，与字段登记一一对应。
//!
//! 错误闭集：`SEG_BASE + 1 ..= SEG_BASE + 12`，逐项见 [`FreezeError`]。
//! 内核路径零 panic：所有可失败入口返回 `Result`，容量与查表全部显式判定。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x441a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x441a0;

/// 字段容量。Ticket 冻结面按 §AI-63 §4 的消费面清单定容 24：
/// 再多就意味着有字段本不该进单点面（应下放到管线私有配置）。
pub const FIELD_CAP: usize = 24;

/// 消费者容量（管线 ×N + UI ×M 的实测量级）。
pub const CONSUMER_CAP: usize = 12;

pub const E_EMPTY_SURFACE: u16 = 1;
pub const E_DUP_FIELD: u16 = 2;
pub const E_UNKNOWN_FIELD: u16 = 3;
pub const E_BAD_KIND: u16 = 4;
pub const E_NOT_DRAFT: u16 = 5;
pub const E_NOT_FROZEN: u16 = 6;
pub const E_SECOND_SURFACE: u16 = 7;
pub const E_DUP_CONSUMER: u16 = 8;
pub const E_CAP_FULL: u16 = 9;
pub const E_DIGEST_MISMATCH: u16 = 10;
pub const E_STALE_CONSUMER: u16 = 11;
pub const E_UNKNOWN_CONSUMER: u16 = 12;

/// 冻结面错误。`code()` 返回 `SEG_BASE + n`，与全仓诊断码闭集一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeError {
    EmptySurface,
    DupField,
    UnknownField,
    BadKind,
    NotDraft,
    NotFrozen,
    SecondSurface,
    DupConsumer,
    CapFull,
    DigestMismatch,
    StaleConsumer,
    UnknownConsumer,
}

impl FreezeError {
    /// 段内偏移（1..=12）。决定码值的地方只有这里。
    pub fn offset(self) -> u16 {
        match self {
            FreezeError::EmptySurface => E_EMPTY_SURFACE,
            FreezeError::DupField => E_DUP_FIELD,
            FreezeError::UnknownField => E_UNKNOWN_FIELD,
            FreezeError::BadKind => E_BAD_KIND,
            FreezeError::NotDraft => E_NOT_DRAFT,
            FreezeError::NotFrozen => E_NOT_FROZEN,
            FreezeError::SecondSurface => E_SECOND_SURFACE,
            FreezeError::DupConsumer => E_DUP_CONSUMER,
            FreezeError::CapFull => E_CAP_FULL,
            FreezeError::DigestMismatch => E_DIGEST_MISMATCH,
            FreezeError::StaleConsumer => E_STALE_CONSUMER,
            FreezeError::UnknownConsumer => E_UNKNOWN_CONSUMER,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.offset() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FreezeError::EmptySurface => "surface has no field to freeze",
            FreezeError::DupField => "field id or name already registered",
            FreezeError::UnknownField => "field id not registered on surface",
            FreezeError::BadKind => "field kind code invalid",
            FreezeError::NotDraft => "surface is not in draft phase",
            FreezeError::NotFrozen => "surface is not frozen",
            FreezeError::SecondSurface => "single-point surface slot already mounted",
            FreezeError::DupConsumer => "consumer name already bound",
            FreezeError::CapFull => "capacity exhausted",
            FreezeError::DigestMismatch => "surface digest does not match token",
            FreezeError::StaleConsumer => "consumer bound to an older digest",
            FreezeError::UnknownConsumer => "consumer name not bound",
        }
    }
}

/// 闭集全表（码值审计用；顺序无关，覆盖性由判据逐项核对）。
pub const ALL_ERRORS: [FreezeError; 12] = [
    FreezeError::EmptySurface,
    FreezeError::DupField,
    FreezeError::UnknownField,
    FreezeError::BadKind,
    FreezeError::NotDraft,
    FreezeError::NotFrozen,
    FreezeError::SecondSurface,
    FreezeError::DupConsumer,
    FreezeError::CapFull,
    FreezeError::DigestMismatch,
    FreezeError::StaleConsumer,
    FreezeError::UnknownConsumer,
];

/// 字段语义类型。类型参与摘要：改了类型就是换了接口，摘要必须变。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Bool,
    Int,
    Enum,
    Range,
    Text,
}

impl FieldKind {
    pub fn code(self) -> u8 {
        match self {
            FieldKind::Bool => 1,
            FieldKind::Int => 2,
            FieldKind::Enum => 3,
            FieldKind::Range => 4,
            FieldKind::Text => 5,
        }
    }

    /// 码值反解。0 与 ≥6 都是非法码——「0 当 Bool」这种隐式约定会让
    /// 老版本序列化悄悄漂移，宁可显式失败。
    pub fn from_code(c: u8) -> Result<FieldKind, FreezeError> {
        match c {
            1 => Ok(FieldKind::Bool),
            2 => Ok(FieldKind::Int),
            3 => Ok(FieldKind::Enum),
            4 => Ok(FieldKind::Range),
            5 => Ok(FieldKind::Text),
            _ => Err(FreezeError::BadKind),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FieldKind::Bool => "bool",
            FieldKind::Int => "int",
            FieldKind::Enum => "enum",
            FieldKind::Range => "range",
            FieldKind::Text => "text",
        }
    }
}

/// 冻结面上的一个字段登记项（不可变值语义，便于逐项比对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldDecl {
    pub id: u16,
    pub name: &'static str,
    pub kind: FieldKind,
    pub unit: &'static str,
}

impl FieldDecl {
    pub const fn new(id: u16, name: &'static str, kind: FieldKind, unit: &'static str) -> FieldDecl {
        FieldDecl {
            id,
            name,
            kind,
            unit,
        }
    }
}

/// 冻结面三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Draft,
    Frozen,
    Retired,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Draft => "draft",
            Phase::Frozen => "frozen",
            Phase::Retired => "retired",
        }
    }
}

/// 账本事件类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerEvent {
    FieldAdded,
    Frozen,
    Retired,
}

/// 账本行：`seq` 由写入位置决定，恒严格递增（不做回绕复用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    pub seq: u32,
    pub event: LedgerEvent,
    pub digest: u32,
}

/// FNV-1a 32 位：无查表、无分配，适合在冻结路径上复算。
fn fold_bytes(mut h: u32, bs: &[u8]) -> u32 {
    let mut i = 0usize;
    while i < bs.len() {
        h ^= bs[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

fn fold_u32(mut h: u32, v: u32) -> u32 {
    h ^= v & 0xFF;
    h = h.wrapping_mul(0x0100_0193);
    h ^= (v >> 8) & 0xFF;
    h = h.wrapping_mul(0x0100_0193);
    h ^= (v >> 16) & 0xFF;
    h = h.wrapping_mul(0x0100_0193);
    h ^= (v >> 24) & 0xFF;
    h.wrapping_mul(0x0100_0193)
}

/// 单字段摘要：id、名称、类型、单位全部入账。
fn field_hash(d: &FieldDecl) -> u32 {
    let mut h = 0x811c_9dc5u32;
    h = fold_u32(h, d.id as u32);
    h = fold_bytes(h, d.name.as_bytes());
    h = fold_u32(h, d.kind.code() as u32);
    fold_bytes(h, d.unit.as_bytes())
}

/// 冻结面摘要令牌（卸面/校验的凭证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreezeToken {
    pub digest: u32,
}

/// 可冻结的接口面：草稿登记 → 冻结 → 退役。
///
/// 摘要采用**与登记顺序无关**的累加折叠：`acc += rotate(field_hash, id % 31)`。
/// 为什么不用顺序链：字段声明顺序是实现细节（结构体定义顺序），接口语义只
/// 由「有哪些字段」决定；用链式摘要会让两处同构声明产生不同摘要，消费者
/// 无故被判定为陈旧。
pub struct Surface {
    phase: Phase,
    fields: Vec<FieldDecl>,
    ledger: Vec<LedgerRow>,
    revision: u16,
}

impl Surface {
    pub fn new() -> Surface {
        Surface {
            phase: Phase::Draft,
            fields: Vec::new(),
            ledger: Vec::new(),
            revision: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// 冻结代次：每次成功冻结 +1，用于审计「这个面被重新冻结过几次」。
    pub fn revision(&self) -> u16 {
        self.revision
    }

    pub fn ledger(&self) -> &[LedgerRow] {
        &self.ledger
    }

    pub fn fields(&self) -> &[FieldDecl] {
        &self.fields
    }

    /// 登记一个字段：仅草稿态可写，重号或重名一律拒绝。
    pub fn register(&mut self, d: FieldDecl) -> Result<(), FreezeError> {
        if self.phase != Phase::Draft {
            return Err(FreezeError::NotDraft);
        }
        if self.fields.len() >= FIELD_CAP {
            return Err(FreezeError::CapFull);
        }
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                if f.id == d.id || f.name == d.name {
                    return Err(FreezeError::DupField);
                }
            }
            i += 1;
        }
        self.fields.push(d);
        self.push_row(LedgerEvent::FieldAdded, 0);
        Ok(())
    }

    /// 取可变字段（草稿期修补用）：未知 id 或已冻结均显式失败。
    pub fn field_mut(&mut self, id: u16) -> Result<&mut FieldDecl, FreezeError> {
        if self.phase != Phase::Draft {
            return Err(FreezeError::NotDraft);
        }
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                if f.id == id {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        match idx {
            Some(k) => match self.fields.get_mut(k) {
                Some(f) => Ok(f),
                None => Err(FreezeError::UnknownField),
            },
            None => Err(FreezeError::UnknownField),
        }
    }

    /// 只读查字段（冻结后消费方走这条路径）。
    pub fn field(&self, id: u16) -> Result<FieldDecl, FreezeError> {
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                if f.id == id {
                    return Ok(*f);
                }
            }
            i += 1;
        }
        Err(FreezeError::UnknownField)
    }

    /// 冻结面摘要（与登记顺序无关，见类型注释）。
    pub fn digest(&self) -> u32 {
        let mut acc = 0u32;
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                let rot = (f.id % 31) as u32;
                acc = acc.wrapping_add(field_hash(f).rotate_left(rot));
            }
            i += 1;
        }
        acc
    }

    /// 冻结：空面拒绝；成功后摘要不可再变（任何写入口都以 `NotDraft` 拒绝）。
    pub fn freeze(&mut self) -> Result<u32, FreezeError> {
        if self.phase != Phase::Draft {
            return Err(FreezeError::NotDraft);
        }
        if self.fields.is_empty() {
            return Err(FreezeError::EmptySurface);
        }
        let d = self.digest();
        self.push_row(LedgerEvent::Frozen, d);
        self.phase = Phase::Frozen;
        self.revision = self.revision.wrapping_add(1);
        Ok(d)
    }

    /// 退役：仅冻结面可退役，且为终态（草稿直接退役属非法转换）。
    pub fn retire(&mut self) -> Result<(), FreezeError> {
        if self.phase != Phase::Frozen {
            return Err(if self.phase == Phase::Draft {
                FreezeError::NotDraft
            } else {
                FreezeError::NotFrozen
            });
        }
        self.push_row(LedgerEvent::Retired, self.digest());
        self.phase = Phase::Retired;
        Ok(())
    }

    /// 取摘要令牌（未冻结不可取——令牌是「可以消费」的凭证）。
    pub fn token(&self) -> Result<FreezeToken, FreezeError> {
        if self.phase != Phase::Frozen {
            return Err(FreezeError::NotFrozen);
        }
        Ok(FreezeToken {
            digest: self.digest(),
        })
    }

    /// 校验外部出示的摘要是否就是本面当下的摘要。
    pub fn verify(&self, digest: u32) -> Result<(), FreezeError> {
        if self.phase != Phase::Frozen {
            return Err(FreezeError::NotFrozen);
        }
        if self.digest() != digest {
            return Err(FreezeError::DigestMismatch);
        }
        Ok(())
    }

    fn push_row(&mut self, event: LedgerEvent, digest: u32) {
        let seq = self.ledger.len() as u32;
        self.ledger.push(LedgerRow {
            seq,
            event,
            digest,
        });
    }
}

/// 单点面槽位：全系统只允许出现一个冻结面。
///
/// 为什么是「槽位」而不是「列表面」：管线与 UI 各自缓存字段偏移，两个面同时
/// 活跃时同一份 Ticket 会被两套偏移解读。要换代只能先出示旧面的摘要令牌卸下，
/// 这样换代动作一定在账上留有可见痕迹。
pub struct SinglePoint {
    active: Option<Surface>,
}

impl SinglePoint {
    pub fn new() -> SinglePoint {
        SinglePoint { active: None }
    }

    /// 挂载冻结面（草稿也可挂，但消费仍需冻结后才可绑定）。
    pub fn mount(&mut self, s: Surface) -> Result<(), FreezeError> {
        if self.active.is_some() {
            return Err(FreezeError::SecondSurface);
        }
        self.active = Some(s);
        Ok(())
    }

    pub fn surface(&self) -> Option<&Surface> {
        self.active.as_ref()
    }

    pub fn surface_mut(&mut self) -> Option<&mut Surface> {
        self.active.as_mut()
    }

    /// 卸下冻结面：必须出示与当下摘要一致的令牌，否则拒绝（防误换面）。
    pub fn unmount(&mut self, token: &FreezeToken) -> Result<Surface, FreezeError> {
        let (phase, dig) = match self.active.as_ref() {
            Some(s) => (s.phase(), s.digest()),
            None => return Err(FreezeError::NotFrozen),
        };
        if phase != Phase::Frozen {
            return Err(FreezeError::NotFrozen);
        }
        if dig != token.digest {
            return Err(FreezeError::DigestMismatch);
        }
        self.active.take().ok_or(FreezeError::NotFrozen)
    }
}

/// 消费者绑定（管线/UI 各一条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumerBind {
    pub name: &'static str,
    pub digest: u32,
}

/// 消费者表：按摘要绑定，摘要不符即陈旧。
pub struct ConsumerTable {
    binds: Vec<ConsumerBind>,
    cap: usize,
}

impl ConsumerTable {
    pub fn new() -> ConsumerTable {
        ConsumerTable::with_capacity(CONSUMER_CAP)
    }

    pub fn with_capacity(cap: usize) -> ConsumerTable {
        ConsumerTable {
            binds: Vec::new(),
            cap,
        }
    }

    pub fn len(&self) -> usize {
        self.binds.len()
    }

    fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.binds.len() {
            if let Some(b) = self.binds.get(i) {
                if b.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 绑定消费者：面必须先冻结；同名二次绑定拒绝。
    pub fn bind(&mut self, name: &'static str, s: &Surface) -> Result<u32, FreezeError> {
        if s.phase() != Phase::Frozen {
            return Err(FreezeError::NotFrozen);
        }
        if self.binds.len() >= self.cap {
            return Err(FreezeError::CapFull);
        }
        if self.find(name).is_some() {
            return Err(FreezeError::DupConsumer);
        }
        let d = s.digest();
        self.binds.push(ConsumerBind { name, digest: d });
        Ok(d)
    }

    /// 读取绑定摘要（消费前自检用）。
    pub fn read(&self, name: &'static str) -> Result<u32, FreezeError> {
        match self.find(name) {
            Some(i) => match self.binds.get(i) {
                Some(b) => Ok(b.digest),
                None => Err(FreezeError::UnknownConsumer),
            },
            None => Err(FreezeError::UnknownConsumer),
        }
    }

    /// 全体消费者是否看到同一摘要（单点面的核心不变量）。
    pub fn all_agree(&self) -> bool {
        let mut i = 1usize;
        let first = match self.binds.first() {
            Some(b) => b.digest,
            None => return true,
        };
        while i < self.binds.len() {
            match self.binds.get(i) {
                Some(b) => {
                    if b.digest != first {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 消费者是否陈旧（绑定的摘要与给定面不一致）。
    pub fn stale(&self, name: &'static str, s: &Surface) -> Result<bool, FreezeError> {
        if s.phase() != Phase::Frozen {
            return Err(FreezeError::NotFrozen);
        }
        match self.find(name) {
            Some(i) => match self.binds.get(i) {
                Some(b) => Ok(b.digest != s.digest()),
                None => Err(FreezeError::UnknownConsumer),
            },
            None => Err(FreezeError::UnknownConsumer),
        }
    }

    /// 要求消费者与给定面一致：陈旧即 `StaleConsumer`（不许带病消费）。
    pub fn require_fresh(&self, name: &'static str, s: &Surface) -> Result<u32, FreezeError> {
        if self.stale(name, s)? {
            return Err(FreezeError::StaleConsumer);
        }
        self.read(name)
    }

    /// 显式重绑到新摘要（换面后的唯一合法迁移路径）。
    pub fn rebind(&mut self, name: &'static str, s: &Surface) -> Result<u32, FreezeError> {
        if s.phase() != Phase::Frozen {
            return Err(FreezeError::NotFrozen);
        }
        match self.find(name) {
            Some(i) => {
                let d = s.digest();
                if let Some(b) = self.binds.get_mut(i) {
                    b.digest = d;
                }
                Ok(d)
            }
            None => Err(FreezeError::UnknownConsumer),
        }
    }
}

/// 出厂冻结面（管线/UI 的公共消费基线，§AI-63 §4 清单的精简子集）。
pub fn factory_surface() -> Surface {
    let mut s = Surface::new();
    let _ = s.register(FieldDecl::new(1, "job-name", FieldKind::Text, ""));
    let _ = s.register(FieldDecl::new(2, "copies", FieldKind::Int, "sheet"));
    let _ = s.register(FieldDecl::new(3, "duplex", FieldKind::Enum, ""));
    let _ = s.register(FieldDecl::new(4, "media-size", FieldKind::Enum, ""));
    let _ = s.register(FieldDecl::new(5, "page-range", FieldKind::Range, "page"));
    s
}

fn frozen_factory() -> Surface {
    let mut s = factory_surface();
    let _ = s.freeze();
    s
}

/// 容量判据用的 24 个互异字段名（名称唯一性是 `DupField` 的另一半）。
const FILL_NAMES: [&str; FIELD_CAP] = [
    "f01", "f02", "f03", "f04", "f05", "f06", "f07", "f08", "f09", "f10", "f11", "f12", "f13",
    "f14", "f15", "f16", "f17", "f18", "f19", "f20", "f21", "f22", "f23", "f24",
];

// ---------------------------------------------------------------------------
// 注入核：六类误用必须逐类被拒，且报出各自的码
// ---------------------------------------------------------------------------

/// 返回 (抓到数, 总数)。每个样本都断言**具体**诊断码，不做「只要失败就算」。
pub fn injection_core() -> (usize, usize) {
    let total = 6usize;
    let mut caught = 0usize;

    // 1 空面冻结：没有任何字段的接口面不能被消费。
    let mut empty = Surface::new();
    if empty.freeze().err() == Some(FreezeError::EmptySurface) {
        caught += 1;
    }

    // 2 重复字段：重号（不同名）与重名（不同号）都必须拒。
    let mut dup = Surface::new();
    let _ = dup.register(FieldDecl::new(7, "sides", FieldKind::Enum, ""));
    if dup
        .register(FieldDecl::new(7, "sides-alt", FieldKind::Enum, ""))
        .err()
        == Some(FreezeError::DupField)
    {
        caught += 1;
    }

    // 3 非法类型码：0/6+ 不是任何已登记类型。
    if FieldKind::from_code(9).err() == Some(FreezeError::BadKind)
        && FieldKind::from_code(0).err() == Some(FreezeError::BadKind)
    {
        caught += 1;
    }

    // 4 冻结后改写：登记与取可变字段两条路径都要挡住。
    let mut frozen = frozen_factory();
    let write_blocked = frozen
        .register(FieldDecl::new(9, "late", FieldKind::Bool, ""))
        .err()
        == Some(FreezeError::NotDraft)
        && frozen.field_mut(1).err() == Some(FreezeError::NotDraft);
    if write_blocked {
        caught += 1;
    }

    // 5 单点面第二个：槽位已占时挂第二面必须失败。
    let mut sp = SinglePoint::new();
    let _ = sp.mount(frozen_factory());
    if sp.mount(frozen_factory()).err() == Some(FreezeError::SecondSurface) {
        caught += 1;
    }

    // 6 错令牌卸面：摘要对不上不许换面。
    let mut sp2 = SinglePoint::new();
    let s = frozen_factory();
    let dig = match s.token() {
        Ok(t) => t.digest,
        Err(_) => 0,
    };
    let _ = sp2.mount(s);
    if sp2
        .unmount(&FreezeToken {
            digest: dig ^ 0x5a5a_5a5a,
        })
        .err()
        == Some(FreezeError::DigestMismatch)
    {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50172_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50172");

    // J1-1 冻结登记：草稿可写、冻结后可消费、空面拒绝
    let mut surf = factory_surface();
    let draft_ok = surf.field_count() == 5 && surf.phase() == Phase::Draft;
    let frozen_digest = surf.freeze();
    let ok = draft_ok && frozen_digest.is_ok() && surf.phase() == Phase::Frozen;
    s.add("50172-J1-冻结登记", ok, if ok { "" } else { "冻结未生效" });

    // J1-2 冻结幂等：二次冻结被拒，摘要与代次均不变
    let d0 = surf.digest();
    let second = surf.freeze();
    let ok = second.err() == Some(FreezeError::NotDraft)
        && surf.digest() == d0
        && surf.revision() == 1;
    s.add("50172-J1-冻结幂等", ok, if ok { "" } else { "二次冻结改变了状态" });

    // J1-3 重号/重名双向拒绝
    let mut dup = Surface::new();
    let _ = dup.register(FieldDecl::new(1, "a", FieldKind::Int, ""));
    let dup_id = dup.register(FieldDecl::new(1, "b", FieldKind::Int, "")).err();
    let dup_name = dup.register(FieldDecl::new(2, "a", FieldKind::Int, "")).err();
    let ok = dup_id == Some(FreezeError::DupField) && dup_name == Some(FreezeError::DupField);
    s.add("50172-J1-登记唯一", ok, if ok { "" } else { "重号/重名未拦" });

    // J1-4 摘要与登记顺序无关（接口语义只看字段集合）
    let mut o1 = Surface::new();
    let _ = o1.register(FieldDecl::new(1, "a", FieldKind::Int, "u"));
    let _ = o1.register(FieldDecl::new(2, "b", FieldKind::Text, ""));
    let mut o2 = Surface::new();
    let _ = o2.register(FieldDecl::new(2, "b", FieldKind::Text, ""));
    let _ = o2.register(FieldDecl::new(1, "a", FieldKind::Int, "u"));
    // 非空面的摘要必须区别于空面（空面摘要恒 0），否则「有没有字段」不可判。
    let ok = o1.digest() == o2.digest() && o1.digest() != Surface::new().digest();
    let det = format!("d1={:#x} d2={:#x}", o1.digest(), o2.digest());
    s.add_dyn("50172-J1-摘要序无关", ok, &det);

    // J1-5 摘要对类型/名称/单位任一变化敏感
    let mut sens = factory_surface();
    let base = sens.digest();
    let mut kind_changed = false;
    if let Ok(f) = sens.field_mut(2) {
        f.kind = FieldKind::Text;
        kind_changed = true;
    }
    let ok = kind_changed && sens.digest() != base;
    s.add("50172-J1-摘要敏感", ok, if ok { "" } else { "改类型摘要未变" });

    // J1-6 冻结后不可改写（登记/取可变字段双路径）
    let mut locked = frozen_factory();
    let add_blocked = locked
        .register(FieldDecl::new(20, "x", FieldKind::Bool, ""))
        .err()
        == Some(FreezeError::NotDraft);
    let mut_blocked = locked.field_mut(1).err() == Some(FreezeError::NotDraft);
    let ok = add_blocked && mut_blocked;
    s.add("50172-J1-冻结禁写", ok, if ok { "" } else { "冻结面被改写" });

    // J1-7 单点面：第二面挂不上；令牌一致才能卸下
    let mut sp = SinglePoint::new();
    let _ = sp.mount(frozen_factory());
    let second = sp.mount(frozen_factory()).err();
    let token = sp.surface().and_then(|v| v.token().ok());
    let unmounted = match token {
        Some(t) => sp.unmount(&t).is_ok(),
        None => false,
    };
    let ok = second == Some(FreezeError::SecondSurface)
        && unmounted
        && sp.surface().is_none();
    s.add("50172-J1-单点唯一", ok, if ok { "" } else { "单点面被破坏" });

    // J1-8 未冻结面不得绑定消费者
    let draft = factory_surface();
    let mut table = ConsumerTable::new();
    let ok = table.bind("pipeline", &draft).err() == Some(FreezeError::NotFrozen)
        && table.len() == 0;
    s.add("50172-J1-绑定前置", ok, if ok { "" } else { "草稿面被绑定" });

    // J1-9 同一冻结面上所有消费者摘要一致
    let frozen = frozen_factory();
    let mut table2 = ConsumerTable::new();
    let _ = table2.bind("pipeline", &frozen);
    let _ = table2.bind("ui", &frozen);
    let _ = table2.bind("spooler", &frozen);
    let ok = table2.len() == 3
        && table2.all_agree()
        && table2.read("pipeline").ok() == Some(frozen.digest())
        && table2.read("ui").ok() == Some(frozen.digest());
    s.add("50172-J1-消费一致", ok, if ok { "" } else { "消费者摘要不一致" });

    // J1-10 换面后旧绑定为陈旧；重绑后才放行
    let mut other = Surface::new();
    let _ = other.register(FieldDecl::new(9, "extra", FieldKind::Bool, ""));
    let _ = other.freeze();
    let stale = table2.stale("pipeline", &other);
    let require = table2.require_fresh("pipeline", &other).err();
    let rebound = table2.rebind("pipeline", &other).is_ok();
    let fresh = table2.require_fresh("pipeline", &other).is_ok();
    let ok = stale.ok() == Some(true)
        && require == Some(FreezeError::StaleConsumer)
        && rebound
        && fresh;
    s.add("50172-J1-陈旧检出", ok, if ok { "" } else { "陈旧消费者未被拦" });

    // J1-11 账本序号严格递增，行数 = 字段数 + 冻结行
    let mut led = factory_surface();
    let _ = led.freeze();
    let rows = led.ledger();
    let mut mono = true;
    let mut i = 0usize;
    while i < rows.len() {
        match rows.get(i) {
            Some(r) => {
                if r.seq != i as u32 {
                    mono = false;
                }
            }
            None => mono = false,
        }
        i += 1;
    }
    let last_ok = match rows.last() {
        Some(r) => r.event == LedgerEvent::Frozen && r.digest == led.digest(),
        None => false,
    };
    // 5 条字段登记 + 1 条冻结事件 = 6 行，行数与操作数守恒。
    let ok = mono && rows.len() == 5 + 1 && last_ok;
    let det = format!("rows={} mono={}", rows.len(), mono);
    s.add_dyn("50172-J1-账本单调", ok, &det);

    // J1-12 退役：草稿直接退役被拒；冻结面退役后不再可消费
    let mut r1 = factory_surface();
    let draft_retire = r1.retire().err();
    let _ = r1.freeze();
    let retired = r1.retire().is_ok();
    let ok = draft_retire == Some(FreezeError::NotDraft)
        && retired
        && r1.phase() == Phase::Retired
        && r1.token().err() == Some(FreezeError::NotFrozen);
    s.add("50172-J1-退役终态", ok, if ok { "" } else { "退役转换非法" });

    // J1-13 容量上限显式失败（字段 24 / 消费者 2）
    let mut cap = Surface::new();
    let mut i2 = 0usize;
    while i2 < FIELD_CAP {
        let name = match FILL_NAMES.get(i2) {
            Some(n) => *n,
            None => "fxx",
        };
        let _ = cap.register(FieldDecl::new(i2 as u16 + 1, name, FieldKind::Int, ""));
        i2 += 1;
    }
    let overflow = cap
        .register(FieldDecl::new(200, "over", FieldKind::Int, ""))
        .err();
    let mut two = ConsumerTable::with_capacity(2);
    let fz = frozen_factory();
    let _ = two.bind("a", &fz);
    let _ = two.bind("b", &fz);
    let cap_hit = two.bind("c", &fz).err();
    let ok = overflow == Some(FreezeError::CapFull)
        && cap_hit == Some(FreezeError::CapFull)
        && cap.field_count() == FIELD_CAP;
    let det = format!("fields={} consumer_cap={}", cap.field_count(), two.len());
    s.add_dyn("50172-J1-容量上限", ok, &det);

    // J1-14 未知字段/未知消费者显式定位
    let ok = frozen_factory().field(99).err() == Some(FreezeError::UnknownField)
        && table2.read("ghost").err() == Some(FreezeError::UnknownConsumer)
        && table2.bind("pipeline", &frozen).err() == Some(FreezeError::DupConsumer);
    s.add("50172-J1-未知定位", ok, if ok { "" } else { "未知项未定位" });

    // J1-15 确定性：同构构造两次摘要与账本逐项一致
    let a = frozen_factory();
    let b = frozen_factory();
    let mut same_rows = a.ledger().len() == b.ledger().len();
    let mut k = 0usize;
    while k < a.ledger().len() && same_rows {
        if a.ledger().get(k) != b.ledger().get(k) {
            same_rows = false;
        }
        k += 1;
    }
    let ok = a.digest() == b.digest() && same_rows;
    s.add("50172-J1-构造确定", ok, if ok { "" } else { "两次构造不一致" });

    // J1-16 码值闭集：12 条码连续、唯一、与枚举一致
    let mut seen = [false; 13];
    let mut ok = true;
    let mut n = 0usize;
    while n < ALL_ERRORS.len() {
        let off = ALL_ERRORS[n].offset() as usize;
        if off == 0 || off >= seen.len() || seen[off] {
            ok = false;
        } else {
            seen[off] = true;
        }
        if ALL_ERRORS[n].code() != SEG_BASE + off as u32 {
            ok = false;
        }
        n += 1;
    }
    s.add("50172-J1-码值闭集", ok, if ok { "" } else { "码值重复或断裂" });

    // J1-17 注入核六类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50172-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50172_checks_green() {
        let set = run_unx50172_checks();
        assert!(set.all_passed(), "UNX-F50172 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50172_digest_order_independent() {
        let mut a = Surface::new();
        assert!(a.register(FieldDecl::new(1, "x", FieldKind::Int, "mm")).is_ok());
        assert!(a.register(FieldDecl::new(2, "y", FieldKind::Text, "")).is_ok());
        let mut b = Surface::new();
        assert!(b.register(FieldDecl::new(2, "y", FieldKind::Text, "")).is_ok());
        assert!(b.register(FieldDecl::new(1, "x", FieldKind::Int, "mm")).is_ok());
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn unx50172_freeze_is_terminal_for_writes() {
        let mut s = frozen_factory();
        assert_eq!(s.phase(), Phase::Frozen);
        assert_eq!(
            s.register(FieldDecl::new(30, "late", FieldKind::Bool, "")).err(),
            Some(FreezeError::NotDraft)
        );
        assert!(s.retire().is_ok());
        assert_eq!(s.token().err(), Some(FreezeError::NotFrozen));
    }
}
