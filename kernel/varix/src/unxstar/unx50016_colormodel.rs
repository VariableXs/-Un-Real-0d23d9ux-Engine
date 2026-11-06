//! UNX-F50016 色彩能力协商（打印机 ColorModel 求交）（0x42e20 段）
//!
//! 锚点：UNX-F50016-J1 求交 ×50 与能力表一致；判据锚定 §AI-63 §2 专题十二。
//!
//! 数据段 `SEG_BASE = 0x42e20`；诊断码闭集 = `SEG_BASE + n`：
//!   +1 Empty     能力集为空 / 标签为空
//!   +2 CapFull   能力表或查询容量满
//!   +3 Duplicate 能力表 id 重复
//!   +4 Unknown   出现未定义的色模位
//!   +5 Order     能力表 id 非严格递增
//!   +6 Zero      能力表 id 为 0（保留值）
//!   +7 NoCommon  求交结果为空（打印机与客户端没有共同色模）
//!   +8 State     协商状态机不允许该迁移
//!   +9 Width     DPI 越界（72..=4800）
//!
//! 求交语义：ColorModel 用位图表达（7 个色模 → 7 bit），协商就是按位与。
//! 位图求交必须与「逐色模成员判定」完全等价——这是 J1 的全部内容：
//! 对 50 组随机（打印机, 客户端）组合，逐位结果要与逐一枚举的集合相等，
//! 且结果同时是两侧子集。位运算的等价性不能靠肉眼，只能靠枚举对照。
//!
//! 结构清单：
//! - 数据类型：`Model`（7 个色模）、`ModelSet`（位图集合）、`CapRow`
//!   （一行能力声明）、`CapTable`（定容能力表）、`Negotiator`（三态协商机）
//! - 状态机：`NegState` = Idle → Offered → Intersected → Agreed / Rejected
//! - 解析/校验：`ModelSet::new`（拒绝未定义位）、`CapRow::new`（id/标签/DPI）
//! - 错误面：`CapError::code()` 落 0x42e20 段闭集
//! - 注入核：`injection_core` 七类坏样本（空能力集/重复 id/未定义位/
//!   id=0/乱序/容量满/无共同色模）

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x42e20 独占 0x20）。
pub const SEG_BASE: u32 = 0x42e20;

pub const E_EMPTY: u16 = 1;
pub const E_CAPFULL: u16 = 2;
pub const E_DUPLICATE: u16 = 3;
pub const E_UNKNOWN: u16 = 4;
pub const E_ORDER: u16 = 5;
pub const E_ZERO: u16 = 6;
pub const E_NOCOMMON: u16 = 7;
pub const E_STATE: u16 = 8;
pub const E_WIDTH: u16 = 9;

/// 能力协商错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapError {
    Empty,
    CapFull,
    Duplicate,
    Unknown,
    Order,
    Zero,
    NoCommon,
    State,
    Width,
}

impl CapError {
    /// 段内诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            CapError::Empty => E_EMPTY,
            CapError::CapFull => E_CAPFULL,
            CapError::Duplicate => E_DUPLICATE,
            CapError::Unknown => E_UNKNOWN,
            CapError::Order => E_ORDER,
            CapError::Zero => E_ZERO,
            CapError::NoCommon => E_NOCOMMON,
            CapError::State => E_STATE,
            CapError::Width => E_WIDTH,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            CapError::Empty => "empty color model set or label",
            CapError::CapFull => "capability table full",
            CapError::Duplicate => "duplicate capability id",
            CapError::Unknown => "undefined color model bit",
            CapError::Order => "capability ids not increasing",
            CapError::Zero => "capability id zero reserved",
            CapError::NoCommon => "no common color model",
            CapError::State => "negotiation state transition not allowed",
            CapError::Width => "dpi out of range",
        }
    }
}

/// 打印机色彩模型（ColorModel）。位序固定，供位图与枚举双向映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Gray,
    Rgb,
    Cmyk,
    DeviceN,
    Lab,
    Indexed,
    Spot,
}

impl Model {
    /// 全枚举表。顺序即位序，`from_bit` 依赖它做一致性反查。
    pub const ALL: [Model; 7] = [
        Model::Gray,
        Model::Rgb,
        Model::Cmyk,
        Model::DeviceN,
        Model::Lab,
        Model::Indexed,
        Model::Spot,
    ];

    /// 位掩码（1 << n，n 为枚举位序）。
    pub fn bit(self) -> u16 {
        match self {
            Model::Gray => 1 << 0,
            Model::Rgb => 1 << 1,
            Model::Cmyk => 1 << 2,
            Model::DeviceN => 1 << 3,
            Model::Lab => 1 << 4,
            Model::Indexed => 1 << 5,
            Model::Spot => 1 << 6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Model::Gray => "Gray",
            Model::Rgb => "RGB",
            Model::Cmyk => "CMYK",
            Model::DeviceN => "DeviceN",
            Model::Lab => "Lab",
            Model::Indexed => "Indexed",
            Model::Spot => "Separation",
        }
    }

    /// 位 → 枚举。未定义的位显式报 Unknown（不猜「最接近的模型」）。
    pub fn from_bit(bit: u16) -> Result<Model, CapError> {
        if bit == 0 || bit & (bit - 1) != 0 {
            // 0 位或多位同时置位都不是「一个模型」。
            return Err(CapError::Unknown);
        }
        let mut i = 0usize;
        while i < Model::ALL.len() {
            if let Some(m) = Model::ALL.get(i) {
                if m.bit() == bit {
                    return Ok(*m);
                }
            }
            i += 1;
        }
        Err(CapError::Unknown)
    }
}

/// 全部合法位（7 个模型）。
pub const ALL_MASK: u16 = 0x7f;

/// 色模集合（位图）。任何进入集合的位都必须已定义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelSet {
    pub mask: u16,
}

impl ModelSet {
    /// 构造并校验：出现未定义位即 Unknown。这是唯一的外部入口。
    pub fn new(mask: u16) -> Result<ModelSet, CapError> {
        if mask & !ALL_MASK != 0 {
            return Err(CapError::Unknown);
        }
        Ok(ModelSet { mask })
    }

    /// 由已知合法的位直接构造（求交/并集的结果天然是子集，无需再校验）。
    /// 仍对入参做一次 `& ALL_MASK`，保证结构体不变量在任何调用点都成立。
    pub fn raw(mask: u16) -> ModelSet {
        ModelSet { mask: mask & ALL_MASK }
    }

    pub fn empty() -> ModelSet {
        ModelSet { mask: 0 }
    }

    pub fn full() -> ModelSet {
        ModelSet { mask: ALL_MASK }
    }

    pub fn single(m: Model) -> ModelSet {
        ModelSet { mask: m.bit() }
    }

    pub fn is_empty(self) -> bool {
        self.mask == 0
    }

    pub fn contains(self, m: Model) -> bool {
        self.mask & m.bit() != 0
    }

    /// 并集。
    pub fn union(self, o: ModelSet) -> ModelSet {
        ModelSet::raw(self.mask | o.mask)
    }

    /// 求交（协商核心）。
    pub fn intersect(self, o: ModelSet) -> ModelSet {
        ModelSet::raw(self.mask & o.mask)
    }

    /// 基数（逐位计数，不依赖 `count_ones` 的可用性假设）。
    pub fn count(self) -> usize {
        let mut n = 0usize;
        let mut v = self.mask;
        while v != 0 {
            n += (v & 1) as usize;
            v >>= 1;
        }
        n
    }

    /// 枚举成员（位序稳定，结果可直接比对）。
    pub fn models(self) -> Vec<Model> {
        let mut out = Vec::with_capacity(self.count());
        let mut i = 0usize;
        while i < Model::ALL.len() {
            if let Some(m) = Model::ALL.get(i) {
                if self.contains(*m) {
                    out.push(*m);
                }
            }
            i += 1;
        }
        out
    }
}

/// 逐成员判定的求交（对照实现）。位运算与它在所有输入上必须等价，
/// 否则说明位序表与枚举表脱节。
pub fn naive_intersect(a: ModelSet, b: ModelSet) -> ModelSet {
    let mut mask = 0u16;
    let mut i = 0usize;
    while i < Model::ALL.len() {
        if let Some(m) = Model::ALL.get(i) {
            if a.contains(*m) && b.contains(*m) {
                mask |= m.bit();
            }
        }
        i += 1;
    }
    ModelSet::raw(mask)
}

/// 一行能力声明：打印机档 id + 标签 + 支持的色模集 + 分辨率。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapRow {
    pub id: u8,
    pub label: String,
    pub set: ModelSet,
    pub dpi: u16,
}

impl CapRow {
    /// 构造一行能力。id=0 / 空标签 / 空集合 / DPI 越界都显式拒绝。
    pub fn new(id: u8, label: &str, set: ModelSet, dpi: u16) -> Result<CapRow, CapError> {
        if id == 0 {
            return Err(CapError::Zero);
        }
        if label.is_empty() {
            return Err(CapError::Empty);
        }
        if set.is_empty() {
            return Err(CapError::Empty);
        }
        if dpi < 72 || dpi > 4800 {
            return Err(CapError::Width);
        }
        Ok(CapRow {
            id,
            label: label.to_string(),
            set,
            dpi,
        })
    }
}

/// 定容能力表：id 严格递增，重号/乱序/超容显式拒绝。
#[derive(Clone, Debug)]
pub struct CapTable {
    cap: usize,
    rows: Vec<CapRow>,
}

impl CapTable {
    pub fn new(cap: usize) -> CapTable {
        CapTable {
            cap,
            rows: Vec::new(),
        }
    }

    /// 登记能力行。顺序：容量 → 重号 → 递增（诊断指向根因）。
    pub fn register(&mut self, row: CapRow) -> Result<usize, CapError> {
        if self.rows.len() >= self.cap {
            return Err(CapError::CapFull);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.id == row.id {
                    return Err(CapError::Duplicate);
                }
            }
            i += 1;
        }
        if let Some(last) = self.rows.last() {
            if last.id >= row.id {
                return Err(CapError::Order);
            }
        }
        self.rows.push(row);
        Ok(self.rows.len() - 1)
    }

    pub fn find(&self, id: u8) -> Option<&CapRow> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.id == id {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    pub fn row_at(&self, i: usize) -> Option<&CapRow> {
        self.rows.get(i)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 表内全部打印机能力的并集（用于「客户端要求是否至少被某档满足」）。
    pub fn printer_union(&self) -> ModelSet {
        let mut acc = ModelSet::empty();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                acc = acc.union(r.set);
            }
            i += 1;
        }
        acc
    }

    /// 以某一行能力发起完整协商（offer → require → settle）。
    pub fn negotiate(&self, id: u8, client: ModelSet) -> Result<Negotiator, CapError> {
        let row = match self.find(id) {
            Some(r) => r,
            None => return Err(CapError::Unknown),
        };
        let mut n = Negotiator::new();
        n.offer(row.set)?;
        n.require(client)?;
        n.settle()?;
        Ok(n)
    }
}

/// 协商状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NegState {
    Idle,
    Offered,
    Intersected,
    Agreed,
    Rejected,
}

impl NegState {
    pub fn label(self) -> &'static str {
        match self {
            NegState::Idle => "idle",
            NegState::Offered => "offered",
            NegState::Intersected => "intersected",
            NegState::Agreed => "agreed",
            NegState::Rejected => "rejected",
        }
    }
}

/// 三态协商机：Idle → Offered（打印机报价）→ Intersected（客户端要求）
/// → Agreed / Rejected（定案）。每步都查前驱状态，跳步即 State 错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Negotiator {
    state: NegState,
    printer: ModelSet,
    client: ModelSet,
    agreed: ModelSet,
}

impl Negotiator {
    pub fn new() -> Negotiator {
        Negotiator {
            state: NegState::Idle,
            printer: ModelSet::empty(),
            client: ModelSet::empty(),
            agreed: ModelSet::empty(),
        }
    }

    pub fn state(self) -> NegState {
        self.state
    }

    pub fn printer(self) -> ModelSet {
        self.printer
    }

    pub fn client(self) -> ModelSet {
        self.client
    }

    pub fn agreed(self) -> ModelSet {
        self.agreed
    }

    /// 打印机报价：必须 Idle，且集合非空。
    pub fn offer(&mut self, printer: ModelSet) -> Result<(), CapError> {
        if self.state != NegState::Idle {
            return Err(CapError::State);
        }
        if printer.is_empty() {
            return Err(CapError::Empty);
        }
        self.printer = printer;
        self.state = NegState::Offered;
        Ok(())
    }

    /// 客户端要求：必须 Offered，且集合非空；此处只做求交，不定案。
    pub fn require(&mut self, client: ModelSet) -> Result<(), CapError> {
        if self.state != NegState::Offered {
            return Err(CapError::State);
        }
        if client.is_empty() {
            return Err(CapError::Empty);
        }
        self.client = client;
        self.agreed = self.printer.intersect(client);
        self.state = NegState::Intersected;
        Ok(())
    }

    /// 定案：必须 Intersected。交集为空 → Rejected + NoCommon（显式失败，
    /// 不返回「空协议」让上层当成功）。
    pub fn settle(&mut self) -> Result<ModelSet, CapError> {
        if self.state != NegState::Intersected {
            return Err(CapError::State);
        }
        if self.agreed.is_empty() {
            self.state = NegState::Rejected;
            return Err(CapError::NoCommon);
        }
        self.state = NegState::Agreed;
        Ok(self.agreed)
    }

    /// 复位到 Idle（保留已算出的值供审计读取）。
    pub fn reset(&mut self) {
        self.state = NegState::Idle;
    }

    /// 已定案时返回交集，否则 None（不伪造「默认空协议」）。
    pub fn result(self) -> Option<ModelSet> {
        if self.state == NegState::Agreed {
            Some(self.agreed)
        } else {
            None
        }
    }
}

impl Default for Negotiator {
    fn default() -> Negotiator {
        Negotiator::new()
    }
}

// ---------------------------------------------------------------------------
// 注入核：七类坏样本必须全部被抓
// ---------------------------------------------------------------------------

fn row(id: u8, label: &str, mask: u16) -> Option<CapRow> {
    match ModelSet::new(mask) {
        Ok(set) => CapRow::new(id, label, set, 600).ok(),
        Err(_) => None,
    }
}

/// 七类坏样本注入：返回（被抓数, 总数）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1 空能力集
    if CapRow::new(1, "empty", ModelSet::empty(), 600).err() == Some(CapError::Empty) {
        caught += 1;
    }
    // 2 重复 id
    let mut t = CapTable::new(4);
    let dup = match (row(3, "a", ALL_MASK), row(3, "b", 1)) {
        (Some(a), Some(b)) => {
            let _ = t.register(a);
            t.register(b).err() == Some(CapError::Duplicate)
        }
        _ => false,
    };
    if dup {
        caught += 1;
    }
    // 3 未定义位
    if ModelSet::new(1 << 15).err() == Some(CapError::Unknown)
        && Model::from_bit(1 << 9).err() == Some(CapError::Unknown)
    {
        caught += 1;
    }
    // 4 id = 0
    if CapRow::new(0, "zero", ModelSet::full(), 600).err() == Some(CapError::Zero) {
        caught += 1;
    }
    // 5 乱序登记
    let mut t2 = CapTable::new(4);
    let order = match (row(9, "a", 1), row(2, "b", 1)) {
        (Some(a), Some(b)) => {
            let _ = t2.register(a);
            t2.register(b).err() == Some(CapError::Order)
        }
        _ => false,
    };
    if order {
        caught += 1;
    }
    // 6 容量满
    let mut t3 = CapTable::new(1);
    let full = match (row(1, "a", 1), row(2, "b", 1)) {
        (Some(a), Some(b)) => {
            let _ = t3.register(a);
            t3.register(b).err() == Some(CapError::CapFull)
        }
        _ => false,
    };
    if full {
        caught += 1;
    }
    // 7 无共同色模
    let mut n = Negotiator::new();
    let _ = n.offer(ModelSet::single(Model::Cmyk));
    let _ = n.require(ModelSet::single(Model::Gray));
    if n.settle().err() == Some(CapError::NoCommon) && n.state() == NegState::Rejected {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *seed
}

fn demo_table() -> Option<CapTable> {
    let mut t = CapTable::new(4);
    let rows = [
        (1u8, "mono-laser", Model::Gray.bit()),
        (2u8, "office-rgb", Model::Rgb.bit() | Model::Gray.bit()),
        (
            3u8,
            "press-cmyk",
            Model::Cmyk.bit() | Model::Spot.bit() | Model::Gray.bit(),
        ),
        (
            4u8,
            "proof-wide",
            Model::Rgb.bit() | Model::Cmyk.bit() | Model::Lab.bit() | Model::DeviceN.bit(),
        ),
    ];
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            match row(r.0, r.1, r.2) {
                Some(c) => {
                    if t.register(c).is_err() {
                        return None;
                    }
                }
                None => return None,
            }
        }
        i += 1;
    }
    Some(t)
}

pub fn run_unx50016_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50016");

    // J1-1 枚举/位表自洽：位唯一、并集恰为 ALL_MASK、from_bit 反查一致
    let mut union = 0u16;
    let mut ok = true;
    let mut i = 0usize;
    while i < Model::ALL.len() {
        if let Some(m) = Model::ALL.get(i) {
            let b = m.bit();
            if b & union != 0 {
                ok = false;
            }
            union |= b;
            if Model::from_bit(b) != Ok(*m) {
                ok = false;
            }
            if !ModelSet::single(*m).contains(*m) {
                ok = false;
            }
        }
        i += 1;
    }
    let ok = ok && union == ALL_MASK && ModelSet::full().count() == 7;
    let det = format!("union={:#x} count={}", union, ModelSet::full().count());
    s.add_dyn("50016-J1-位表自洽", ok, &det);

    // J1-2 未定义位必拒；且空/满集合边界成立
    let ok = ModelSet::new(0x80).err() == Some(CapError::Unknown)
        && ModelSet::new(0xffff).err() == Some(CapError::Unknown)
        && ModelSet::new(0).map(|m| m.is_empty()) == Ok(true)
        && ModelSet::new(ALL_MASK).map(|m| m.count()) == Ok(7);
    s.add("50016-J1-未定义位必拒", ok, if ok { "" } else { "越界位被接受" });

    // J1-3 求交代数性质：交换、幂等、与空集为零元、与满集为恒等
    let a = ModelSet::raw(0b0011011);
    let b = ModelSet::raw(0b0110110);
    let ok = a.intersect(b) == b.intersect(a)
        && a.intersect(a) == a
        && a.intersect(ModelSet::empty()).is_empty()
        && a.intersect(ModelSet::full()) == a
        && a.union(b).count() >= a.count();
    s.add("50016-J1-求交代数", ok, if ok { "" } else { "求交性质被破坏" });

    // J1-4 J1 核心：50 组随机组合，位运算求交 == 逐成员判定
    let mut seed = 0x0123_4567_89ab_cdefu64;
    let mut mismatch = 0usize;
    let mut n = 0usize;
    while n < 50 {
        let abits = (lcg(&mut seed) >> 40) as u16 & ALL_MASK;
        let bbits = (lcg(&mut seed) >> 40) as u16 & ALL_MASK;
        let am = ModelSet::raw(abits);
        let bm = ModelSet::raw(bbits);
        let fast = am.intersect(bm);
        let slow = naive_intersect(am, bm);
        if fast != slow {
            mismatch += 1;
        }
        // 结果必须是两侧子集，且成员数等于位计数
        let mut k = 0usize;
        while k < Model::ALL.len() {
            if let Some(m) = Model::ALL.get(k) {
                let want = am.contains(*m) && bm.contains(*m);
                if fast.contains(*m) != want {
                    mismatch += 1;
                }
            }
            k += 1;
        }
        if fast.count() != fast.models().len() {
            mismatch += 1;
        }
        n += 1;
    }
    let ok = mismatch == 0;
    let det = format!("queries=50 mismatch={}", mismatch);
    s.add_dyn("50016-J1-求交×50", ok, &det);

    // J1-5 能力表：正常登记 + 检索 + 并集
    let table = match demo_table() {
        Some(t) => t,
        None => return s,
    };
    let t_union = table.printer_union();
    let ok = table.len() == 4
        && table.find(3).map(|r| r.label == "press-cmyk") == Some(true)
        && table.find(99).is_none()
        && t_union.contains(Model::Cmyk)
        && t_union.contains(Model::Lab)
        && !t_union.contains(Model::Indexed)
        && t_union.count() == 6;
    let det = format!("rows={} union={:#x}", table.len(), t_union.mask);
    s.add_dyn("50016-J1-能力表", ok, &det);

    // J1-6 能力行校验：空集/空标签/id=0/DPI 越界
    let ok = CapRow::new(1, "e", ModelSet::empty(), 600).err() == Some(CapError::Empty)
        && CapRow::new(1, "", ModelSet::full(), 600).err() == Some(CapError::Empty)
        && CapRow::new(0, "z", ModelSet::full(), 600).err() == Some(CapError::Zero)
        && CapRow::new(1, "d", ModelSet::full(), 71).err() == Some(CapError::Width)
        && CapRow::new(1, "d", ModelSet::full(), 4801).err() == Some(CapError::Width)
        && CapRow::new(1, "d", ModelSet::full(), 72).is_ok()
        && CapRow::new(1, "d", ModelSet::full(), 4800).is_ok();
    s.add("50016-J1-能力行校验", ok, if ok { "" } else { "能力行边界错" });

    // J1-7 注册表：重号/乱序/容量（容量另起一张表验证）
    let mut t = CapTable::new(4);
    let r1 = row(5, "a", 1).map(|r| t.register(r));
    let r2 = row(5, "b", 2).map(|r| t.register(r));
    let r3 = row(7, "c", 4).map(|r| t.register(r));
    let r4 = row(6, "d", 8).map(|r| t.register(r));
    let mut tcap = CapTable::new(1);
    let c1 = row(1, "a", 1).map(|r| tcap.register(r));
    let c2 = row(2, "b", 2).map(|r| tcap.register(r));
    let ok = matches!(r1, Some(Ok(0)))
        && matches!(r2, Some(Err(CapError::Duplicate)))
        && matches!(r3, Some(Ok(1)))
        && matches!(r4, Some(Err(CapError::Order)))
        && matches!(c1, Some(Ok(0)))
        && matches!(c2, Some(Err(CapError::CapFull)));
    let det = format!("r2={:?} c2={:?}", r2, c2);
    s.add_dyn("50016-J1-注册闸", ok, &det);

    // J1-8 协商正常路径：交集 == 位与，且是双方子集
    let client = ModelSet::single(Model::Cmyk).union(ModelSet::single(Model::Gray));
    let ok = match table.negotiate(3, client) {
        Ok(n) => {
            let agreed = n.agreed();
            n.state() == NegState::Agreed
                && agreed == ModelSet::raw(Model::Cmyk.bit() | Model::Gray.bit())
                && agreed == n.printer().intersect(n.client())
                && n.result() == Some(agreed)
        }
        Err(_) => false,
    };
    s.add("50016-J1-协商定案", ok, if ok { "" } else { "协商结果与求交不符" });

    // J1-9 求交 ×50 走完整协商：结果与能力表逐项一致
    let mut seed2 = 0xdead_beef_0bad_f00du64;
    let mut bad = 0usize;
    let mut q = 0usize;
    let mut agreements = 0usize;
    while q < 50 {
        let bits = (lcg(&mut seed2) >> 40) as u16 & ALL_MASK;
        let client = ModelSet::raw(bits);
        let row_idx = (lcg(&mut seed2) % 4) as usize;
        let row = match table.row_at(row_idx) {
            Some(r) => r,
            None => return s,
        };
        let expect = row.set.intersect(client);
        match table.negotiate(row.id, client) {
            Ok(n) => {
                agreements += 1;
                if n.agreed() != expect
                    || !expect.models().iter().all(|m| row.set.contains(*m) && client.contains(*m))
                {
                    bad += 1;
                }
            }
            Err(CapError::NoCommon) => {
                if !expect.is_empty() {
                    bad += 1;
                }
            }
            Err(_) => bad += 1,
        }
        q += 1;
    }
    let ok = bad == 0 && agreements > 0;
    let det = format!("queries=50 agreed={} bad={}", agreements, bad);
    s.add_dyn("50016-J1-协商×50", ok, &det);

    // J1-10 状态机跳步必拒：require/settle 先于前驱、重复 offer
    let mut n = Negotiator::new();
    let early_require = n.require(ModelSet::full()).err() == Some(CapError::State);
    let early_settle = n.settle().err() == Some(CapError::State);
    let _ = n.offer(ModelSet::single(Model::Rgb));
    let double_offer = n.offer(ModelSet::single(Model::Gray)).err() == Some(CapError::State);
    let empty_client = n.require(ModelSet::empty()).err() == Some(CapError::Empty);
    let ok = early_require && early_settle && double_offer && empty_client;
    let det = format!("req={} set={} offer={}", early_require, early_settle, double_offer);
    s.add_dyn("50016-J1-跳步必拒", ok, &det);

    // J1-11 无交集：NoCommon 且 Rejected，result() 为 None
    let mut nc = Negotiator::new();
    let _ = nc.offer(ModelSet::single(Model::Gray));
    let _ = nc.require(ModelSet::single(Model::DeviceN));
    let ok = nc.settle().err() == Some(CapError::NoCommon)
        && nc.state() == NegState::Rejected
        && nc.result().is_none();
    s.add("50016-J1-无交集", ok, if ok { "" } else { "无交集被当成功" });

    // J1-12 确定性：同输入两次协商结果逐位一致
    let row4 = match table.find(4) {
        Some(r) => r.set,
        None => return s,
    };
    let mut n1 = Negotiator::new();
    let mut n2 = Negotiator::new();
    let _ = n1.offer(row4);
    let _ = n2.offer(row4);
    let _ = n1.require(client);
    let _ = n2.require(client);
    let _ = n1.settle();
    let _ = n2.settle();
    let ok = n1 == n2 && n1.result().is_some();
    s.add("50016-J1-协商确定", ok, if ok { "" } else { "协商非确定" });

    // J1-13 注入核七类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50016-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50016_checks_green() {
        let set = run_unx50016_checks();
        assert!(set.all_passed(), "UNX-F50016 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50016_bit_roundtrip() {
        let mut i = 0usize;
        while i < Model::ALL.len() {
            if let Some(m) = Model::ALL.get(i) {
                assert_eq!(Model::from_bit(m.bit()), Ok(*m));
                assert_eq!(ModelSet::single(*m).models().len(), 1);
            }
            i += 1;
        }
    }

    #[test]
    fn unx50016_intersect_commutes() {
        let mut seed = 0x51u64;
        let mut n = 0usize;
        while n < 50 {
            let a = ModelSet::raw((lcg(&mut seed) >> 40) as u16);
            let b = ModelSet::raw((lcg(&mut seed) >> 40) as u16);
            assert_eq!(a.intersect(b), b.intersect(a));
            assert_eq!(a.intersect(b), naive_intersect(a, b));
            n += 1;
        }
    }
}
