//! UNX-F50137 B27 批向量账骨架（USB 用例集生成器 + 校验和）（0x43d40 段）
//!
//! 锚点：UNX-F50137-J1 用例带校验和落账；判据锚定 §AI-63 §5。
//!
//! 批向量账要解决的是「账上说跑过什么」与「实际跑的是什么」之间的双真值
//! 问题。做法只有一条：用例在**落账那一刻**就带上自己的校验和，封印时再把
//! 全部校验和折叠成根摘要。此后任何一条用例（哪怕只改一个字节数）被改动，
//! 重算根摘要立刻对不上——不需要第二份清单来做交叉核对。
//!
//! 为什么单包上限要写进校验器：USB 2.0 高速端点的 `wMaxPacketSize` 是硬约束
//! （控制 64 / 批量 512 / 中断 1024 / 等时 1024）。用例越过它，上机就不再是
//! 「一次传输拆 N 包」而是非法请求。这里把「字节数 ≤ 单包上限 × 8」作为硬
//! 边界（8 包是一次传输的典型上界，推导见 `UseCase::byte_cap`），越界一律
//! `E_LEN`，不在运行期偷偷截断——截断后的向量证明不了原来的意图。
//!
//! 数据结构（两类以上真实结构，非函数堆砌）：
//! * `UseCase` / `UseCaseSet` —— 用例与用例集（注册表，编号严格递增）；
//! * `VectorLedger` / `LedgerState` —— 用例账与三态状态机（Open→Sealed→Verified）；
//! * `Xorshift32` —— 确定性生成器（同种子同集合，跨机器逐位一致）。
//!
//! 错误闭集：`SEG_BASE + n`（0x43d40 段）。九个码值全部在判据或注入核里被
//! 真实构造，没有「定义了但从不发生」的死码。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43d40 独占 0x20，段内 9 个码值）。
pub const SEG_BASE: u32 = 0x43d40;

pub const E_EMPTY: u16 = 1;
pub const E_CAP: u16 = 2;
pub const E_DUPE: u16 = 3;
pub const E_RANGE: u16 = 4;
pub const E_CLASS: u16 = 5;
pub const E_LEN: u16 = 6;
pub const E_STATE: u16 = 7;
pub const E_CSUM: u16 = 8;
pub const E_ORDER: u16 = 9;

/// 向量账错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    Empty,
    Cap,
    Dupe,
    Range,
    Class,
    Len,
    State,
    Csum,
    Order,
}

impl VecError {
    /// 诊断码 = 段基址 + 序号。
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Cap => E_CAP,
            VecError::Dupe => E_DUPE,
            VecError::Range => E_RANGE,
            VecError::Class => E_CLASS,
            VecError::Len => E_LEN,
            VecError::State => E_STATE,
            VecError::Csum => E_CSUM,
            VecError::Order => E_ORDER,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本（供红项回显）。
    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "empty case set or empty ledger",
            VecError::Cap => "case capacity reached",
            VecError::Dupe => "case id repeated",
            VecError::Range => "id/endpoint outside legal range",
            VecError::Class => "transfer class violates endpoint discipline",
            VecError::Len => "payload bytes outside class packet budget",
            VecError::State => "ledger state does not allow this action",
            VecError::Csum => "ledger digest does not match detail",
            VecError::Order => "case id went backwards",
        }
    }
}

/// 一次传输的类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XferKind {
    Control,
    Bulk,
    Interrupt,
    Iso,
}

impl XferKind {
    /// 类别码（0..3），落账与摘要都按它折叠。
    pub fn code(self) -> u8 {
        match self {
            XferKind::Control => 0,
            XferKind::Bulk => 1,
            XferKind::Interrupt => 2,
            XferKind::Iso => 3,
        }
    }

    /// 码值反解；4 以上显式拒绝（不做「默认批量」的静默兜底）。
    pub fn from_code(c: u8) -> Result<XferKind, VecError> {
        match c {
            0 => Ok(XferKind::Control),
            1 => Ok(XferKind::Bulk),
            2 => Ok(XferKind::Interrupt),
            3 => Ok(XferKind::Iso),
            _ => Err(VecError::Class),
        }
    }

    /// 单包上限：USB 2.0 高速端点的 wMaxPacketSize 基线。
    pub fn max_packet(self) -> u32 {
        match self {
            XferKind::Control => 64,
            XferKind::Bulk => 512,
            XferKind::Interrupt => 1024,
            XferKind::Iso => 1024,
        }
    }

    /// 标签（诊断文本用，不参与摘要）。
    pub fn label(self) -> &'static str {
        match self {
            XferKind::Control => "control",
            XferKind::Bulk => "bulk",
            XferKind::Interrupt => "interrupt",
            XferKind::Iso => "iso",
        }
    }
}

/// 传输方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

impl Direction {
    /// 方向码（0=IN，1=OUT）。
    pub fn code(self) -> u8 {
        match self {
            Direction::In => 0,
            Direction::Out => 1,
        }
    }

    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            Direction::In => "in",
            Direction::Out => "out",
        }
    }
}

/// 单条用例的容量上限（注册表定容；超出即 `E_CAP`）。
pub const MAX_CASES: usize = 64;
/// 一次传输最多拆 8 个包——推导：高速端点的典型传输长度 ≤ 8 × wMaxPacketSize，
/// 更大的请求在上机侧会被拆成多次 URB，已经不是「一条向量」。
pub const MAX_CHUNKS: u32 = 8;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(FNV_PRIME)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut i = 0usize;
    while i < 4 {
        h = fnv_u8(h, (v >> (i * 8)) as u8);
        i += 1;
    }
    h
}

/// 一条 USB 传输用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseCase {
    pub id: u16,
    pub kind: XferKind,
    pub dir: Direction,
    pub ep: u8,
    pub bytes: u32,
}

impl UseCase {
    /// 构造（不做校验；校验在 `validate`）。
    pub const fn new(id: u16, kind: XferKind, dir: Direction, ep: u8, bytes: u32) -> UseCase {
        UseCase {
            id,
            kind,
            dir,
            ep,
            bytes,
        }
    }

    /// 单用例字节上限 = 单包上限 × 8。
    pub fn byte_cap(kind: XferKind) -> u32 {
        kind.max_packet() * MAX_CHUNKS
    }

    /// 需要拆几个包（向上取整；bytes 已保证 ≥1）。
    pub fn chunks(&self) -> u32 {
        let mp = self.kind.max_packet();
        (self.bytes + mp - 1) / mp
    }

    /// 结构校验：控制端点必须走 EP0，其余类别的端点号落在 1..=15。
    ///
    /// 为什么控制传输的端点必须是 0：USB 规范里控制管道是默认管道（EP0），
    /// 给它编 3 号端点意味着「一条根本不存在的数据通路」，用例即使跑通也
    /// 不能证明任何东西，必须在落账前拦下。
    pub fn validate(&self) -> Result<(), VecError> {
        if self.id == 0 {
            return Err(VecError::Range);
        }
        if self.bytes == 0 || self.bytes > UseCase::byte_cap(self.kind) {
            return Err(VecError::Len);
        }
        match self.kind {
            XferKind::Control => {
                if self.ep != 0 {
                    return Err(VecError::Class);
                }
            }
            _ => {
                if self.ep == 0 || self.ep > 15 {
                    return Err(VecError::Range);
                }
            }
        }
        Ok(())
    }

    /// 用例校验和：编号 ‖ 类别 ‖ 方向 ‖ 端点 ‖ 字节数。
    pub fn checksum(&self) -> u64 {
        case_checksum(self.id, self.kind, self.dir, self.ep, self.bytes)
    }

    /// 一行渲染（诊断用；不含浮点，跨机器逐字节一致）。
    pub fn render(&self) -> String {
        format!(
            "#{} {} {} ep{} {}B",
            self.id,
            self.kind.label(),
            self.dir.label(),
            self.ep,
            self.bytes
        )
    }
}

/// 用例校验和（独立函数，便于账本与判据复用同一实现）。
pub fn case_checksum(id: u16, kind: XferKind, dir: Direction, ep: u8, bytes: u32) -> u64 {
    let mut h = FNV_OFFSET;
    h = fnv_u8(h, (id & 0xff) as u8);
    h = fnv_u8(h, (id >> 8) as u8);
    h = fnv_u8(h, kind.code());
    h = fnv_u8(h, dir.code());
    h = fnv_u8(h, ep);
    h = fnv_u32(h, bytes);
    h
}

/// 用例集（注册表）。编号严格递增 + 去重 + 定容。
#[derive(Clone, Debug)]
pub struct UseCaseSet {
    cases: Vec<UseCase>,
}

impl UseCaseSet {
    /// 空集合。
    pub fn new() -> UseCaseSet {
        UseCaseSet {
            cases: Vec::with_capacity(MAX_CASES),
        }
    }

    /// 用例条数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 只读视图。
    pub fn cases(&self) -> &[UseCase] {
        &self.cases
    }

    /// 追加一条用例。顺序即编号顺序——乱序会让「第 N 条」在两次复算里指向
    /// 不同用例，摘要就失去可比性，所以倒退一律 `E_ORDER`。
    pub fn push(&mut self, c: UseCase) -> Result<(), VecError> {
        c.validate()?;
        if self.cases.len() >= MAX_CASES {
            return Err(VecError::Cap);
        }
        if let Some(last) = self.cases.last() {
            if c.id == last.id {
                return Err(VecError::Dupe);
            }
            if c.id < last.id {
                return Err(VecError::Order);
            }
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(x) = self.cases.get(i) {
                if x.id == c.id {
                    return Err(VecError::Dupe);
                }
            }
            i += 1;
        }
        self.cases.push(c);
        Ok(())
    }

    /// 类别覆盖位图（bit0 控制 / bit1 批量 / bit2 中断 / bit3 等时）。
    pub fn coverage(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(x) = self.cases.get(i) {
                mask |= 1 << x.kind.code();
            }
            i += 1;
        }
        mask
    }

    /// 合计（条数, 字节数）。现算不缓存——缓存的合计一旦与明细脱钩就是账实不符。
    pub fn totals(&self) -> (usize, u32) {
        let mut n = 0usize;
        let mut bytes = 0u32;
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(x) = self.cases.get(i) {
                n += 1;
                bytes = bytes.saturating_add(x.bytes);
            }
            i += 1;
        }
        (n, bytes)
    }

    /// 集合摘要（用例序 + 每条校验和）。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(x) = self.cases.get(i) {
                h ^= x.checksum();
                h = h.wrapping_mul(FNV_PRIME);
                h = fnv_u8(h, x.kind.code());
            }
            i += 1;
        }
        h
    }
}

/// 确定性伪随机源。选 xorshift32 而不是 LCG 是因为它只有移位/异或，没有任何
/// 乘法溢出点，在 debug（溢出即 panic）与 release 下逐位一致。
struct Xorshift32 {
    s: u32,
}

impl Xorshift32 {
    fn new(seed: u32) -> Xorshift32 {
        Xorshift32 {
            s: if seed == 0 { 0x9e37_79b9 } else { seed },
        }
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.s = x;
        x
    }

    fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

/// 生成 `count` 条用例（类别按 4 循环，保证覆盖；其余字段由种子决定）。
///
/// 为什么要「按 4 循环」而不是纯随机：随机类别在 count=4 时常缺档，而覆盖
/// 四类是封印的前置条件。循环打底 + 随机化其余字段，既保证结构又保留差异。
pub fn generate(seed: u32, count: u16) -> Result<UseCaseSet, VecError> {
    if count == 0 {
        return Err(VecError::Empty);
    }
    if count as usize > MAX_CASES {
        return Err(VecError::Cap);
    }
    let mut rng = Xorshift32::new(seed);
    let mut set = UseCaseSet::new();
    let mut i = 0u16;
    while i < count {
        let kind = XferKind::from_code((i % 4) as u8)?;
        let dir = if rng.below(2) == 0 {
            Direction::In
        } else {
            Direction::Out
        };
        let ep = match kind {
            XferKind::Control => 0u8,
            _ => 1 + rng.below(15) as u8,
        };
        let bytes = 1 + rng.below(UseCase::byte_cap(kind));
        set.push(UseCase::new(5000 + i, kind, dir, ep, bytes))?;
        i += 1;
    }
    Ok(set)
}

/// 账本三态。只允许单向推进：Open → Sealed → Verified。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerState {
    Open,
    Sealed,
    Verified,
}

impl LedgerState {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            LedgerState::Open => "open",
            LedgerState::Sealed => "sealed",
            LedgerState::Verified => "verified",
        }
    }
}

/// 账本明细（用例落账的原子项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u16,
    pub kind: XferKind,
    pub bytes: u32,
    pub csum: u64,
}

/// 用例账：逐条落账（带校验和）→ 封印（折叠根摘要）→ 复核（重算比对）。
#[derive(Clone, Debug)]
pub struct VectorLedger {
    entries: Vec<Entry>,
    state: LedgerState,
    root: u64,
    replays: u16,
}

impl VectorLedger {
    /// 空账（Open）。
    pub fn new() -> VectorLedger {
        VectorLedger {
            entries: Vec::with_capacity(MAX_CASES),
            state: LedgerState::Open,
            root: 0,
            replays: 0,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> LedgerState {
        self.state
    }

    /// 明细条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 复核重放次数（幂等验证的显性痕迹）。
    pub fn replays(&self) -> u16 {
        self.replays
    }

    /// 封印根摘要（未封印时为 0）。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 明细只读视图。
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// 取第 `idx` 条明细。
    pub fn entry(&self, idx: usize) -> Option<&Entry> {
        self.entries.get(idx)
    }

    /// 落账一条用例。校验和在此刻定格——后续任何字段变化都会让重算失配。
    pub fn record(&mut self, c: &UseCase) -> Result<(), VecError> {
        if self.state != LedgerState::Open {
            return Err(VecError::State);
        }
        c.validate()?;
        if self.entries.len() >= MAX_CASES {
            return Err(VecError::Cap);
        }
        if let Some(last) = self.entries.last() {
            if last.id == c.id {
                return Err(VecError::Dupe);
            }
            if c.id < last.id {
                return Err(VecError::Order);
            }
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(x) = self.entries.get(i) {
                if x.id == c.id {
                    return Err(VecError::Dupe);
                }
            }
            i += 1;
        }
        self.entries.push(Entry {
            id: c.id,
            kind: c.kind,
            bytes: c.bytes,
            csum: c.checksum(),
        });
        Ok(())
    }

    /// 明细合计（条数, 字节数）。
    pub fn totals(&self) -> (usize, u32) {
        let mut n = 0usize;
        let mut bytes = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                n += 1;
                bytes = bytes.saturating_add(e.bytes);
            }
            i += 1;
        }
        (n, bytes)
    }

    /// 类别覆盖位图（按明细的类别列重算）。
    pub fn coverage(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                mask |= 1 << e.kind.code();
            }
            i += 1;
        }
        mask
    }

    /// 重算根摘要（顺序敏感：条目序不同则结果不同）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_u8(h, e.kind.code());
                h = fnv_u8(h, (e.id & 0xff) as u8);
                h = fnv_u8(h, (e.id >> 8) as u8);
                h = fnv_u32(h, e.bytes);
                h ^= e.csum;
                h = h.wrapping_mul(FNV_PRIME);
            }
            i += 1;
        }
        h
    }

    /// 封印：Open → Sealed。要求非空且四类覆盖齐全。
    ///
    /// 覆盖检查放在封印而不是落账，是因为生成器可能只产出一个类别子集
    /// （例如只做批量回归）；「能不能封」是账的语义，不是单条用例的语义。
    pub fn seal(&mut self) -> Result<u64, VecError> {
        if self.state != LedgerState::Open {
            return Err(VecError::State);
        }
        if self.entries.is_empty() {
            return Err(VecError::Empty);
        }
        if self.coverage() != 0x0f {
            return Err(VecError::Class);
        }
        let r = self.recompute();
        self.root = r;
        self.state = LedgerState::Sealed;
        Ok(r)
    }

    /// 复核：重算与封印值比对。重复复核幂等（只加一次重放痕迹）。
    pub fn verify(&mut self) -> Result<u64, VecError> {
        if self.state == LedgerState::Open {
            return Err(VecError::State);
        }
        let r = self.recompute();
        if r != self.root {
            return Err(VecError::Csum);
        }
        if self.state == LedgerState::Verified {
            self.replays += 1;
        } else {
            self.state = LedgerState::Verified;
        }
        Ok(r)
    }

    /// 审计注入口：只改明细、不更新封印（模拟「账实不符」）。仅用于验证
    /// `verify` 必抓，不参与正常路径。
    pub fn forge_bytes(&mut self, idx: usize, bytes: u32) -> Result<(), VecError> {
        if self.state == LedgerState::Open {
            return Err(VecError::State);
        }
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.bytes = bytes;
                Ok(())
            }
            None => Err(VecError::Range),
        }
    }

    /// 账本渲染（诊断用）。
    pub fn render(&self) -> String {
        let (n, bytes) = self.totals();
        format!(
            "ledger {} n={} bytes={} cov={:x} root={:016x} replays={}",
            self.state.label(),
            n,
            bytes,
            self.coverage(),
            self.root,
            self.replays
        )
    }
}

fn build_ledger(set: &UseCaseSet) -> Result<VectorLedger, VecError> {
    let mut l = VectorLedger::new();
    let mut i = 0usize;
    while i < set.len() {
        if let Some(c) = set.cases().get(i) {
            l.record(c)?;
        }
        i += 1;
    }
    Ok(l)
}

fn bulk_ledger(n: u16) -> Result<VectorLedger, VecError> {
    let mut l = VectorLedger::new();
    let mut i = 0u16;
    while i < n {
        l.record(&UseCase::new(
            1 + i,
            XferKind::Bulk,
            Direction::Out,
            1,
            512,
        ))?;
        i += 1;
    }
    Ok(l)
}

fn quad_ledger() -> Result<VectorLedger, VecError> {
    let mut l = VectorLedger::new();
    l.record(&UseCase::new(1, XferKind::Control, Direction::Out, 0, 8))?;
    l.record(&UseCase::new(2, XferKind::Bulk, Direction::In, 1, 512))?;
    l.record(&UseCase::new(3, XferKind::Interrupt, Direction::In, 2, 64))?;
    l.record(&UseCase::new(4, XferKind::Iso, Direction::Out, 3, 1024))?;
    Ok(l)
}

/// 注入核：七类坏账，逐类断言专属诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1 编号 0
    if UseCase::new(0, XferKind::Bulk, Direction::Out, 1, 512)
        .validate()
        .err()
        == Some(VecError::Range)
    {
        caught += 1;
    }

    // 2 控制传输走非 0 端点
    if UseCase::new(7, XferKind::Control, Direction::In, 3, 64)
        .validate()
        .err()
        == Some(VecError::Class)
    {
        caught += 1;
    }

    // 3 超过单包预算
    if UseCase::new(8, XferKind::Bulk, Direction::In, 2, 4097)
        .validate()
        .err()
        == Some(VecError::Len)
    {
        caught += 1;
    }

    // 4 重复编号
    let mut dup = UseCaseSet::new();
    let ok = dup
        .push(UseCase::new(9, XferKind::Bulk, Direction::In, 1, 512))
        .is_ok();
    if ok
        && dup
            .push(UseCase::new(9, XferKind::Bulk, Direction::In, 1, 512))
            .err()
            == Some(VecError::Dupe)
    {
        caught += 1;
    }

    // 5 编号倒退
    let mut ord = UseCaseSet::new();
    let ok = ord
        .push(UseCase::new(10, XferKind::Bulk, Direction::In, 1, 512))
        .is_ok();
    if ok
        && ord
            .push(UseCase::new(9, XferKind::Bulk, Direction::In, 1, 512))
            .err()
            == Some(VecError::Order)
    {
        caught += 1;
    }

    // 6 空账封印
    if VectorLedger::new().seal().err() == Some(VecError::Empty) {
        caught += 1;
    }

    // 7 封印后改明细但不改根
    let mut forged = match quad_ledger() {
        Ok(mut l) => {
            if l.seal().is_ok() {
                if l.forge_bytes(0, 1).is_ok() {
                    Some(l)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Err(_) => None,
    };
    if let Some(l) = forged.as_mut() {
        if l.verify().err() == Some(VecError::Csum) {
            caught += 1;
        }
    }

    (caught, total)
}

/// 判据集（14 条）。
pub fn run_unx50137_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50137");

    // J1-1 类别码闭集与单包上限
    let mut ok = true;
    let mut c = 0u8;
    while c < 4 {
        match XferKind::from_code(c) {
            Ok(k) => {
                if k.code() != c {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        c += 1;
    }
    let ok = ok
        && XferKind::from_code(4).err() == Some(VecError::Class)
        && XferKind::Control.max_packet() == 64
        && XferKind::Bulk.max_packet() == 512
        && XferKind::Interrupt.max_packet() == 1024
        && XferKind::Iso.max_packet() == 1024
        && UseCase::byte_cap(XferKind::Bulk) == 4096;
    s.add("50137-J1-类别闭集", ok, if ok { "" } else { "类别码或包长表漂移" });

    // J1-2 校验和：同构相等、单字段改变必变
    let base = UseCase::new(5001, XferKind::Bulk, Direction::Out, 1, 512);
    let ok = base.checksum() == case_checksum(5001, XferKind::Bulk, Direction::Out, 1, 512)
        && base.checksum() != UseCase::new(5002, XferKind::Bulk, Direction::Out, 1, 512).checksum()
        && base.checksum() != UseCase::new(5001, XferKind::Control, Direction::Out, 1, 512).checksum()
        && base.checksum() != UseCase::new(5001, XferKind::Bulk, Direction::In, 1, 512).checksum()
        && base.checksum() != UseCase::new(5001, XferKind::Bulk, Direction::Out, 2, 512).checksum()
        && base.checksum() != UseCase::new(5001, XferKind::Bulk, Direction::Out, 1, 513).checksum();
    s.add("50137-J1-校验和敏感", ok, if ok { "" } else { "校验和漏字段" });

    // J1-3 生成确定性：同种子两次逐位一致
    let a = generate(0x1234, 24);
    let b = generate(0x1234, 24);
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => {
            x.digest() == y.digest() && x.totals() == y.totals() && x.len() == 24 && y.len() == 24
        }
        _ => false,
    };
    s.add("50137-J1-生成确定", ok, if ok { "" } else { "同种子集合不一致" });

    // J1-4 不同种子必产生不同集合
    let ok = match (generate(1, 24), generate(2, 24)) {
        (Ok(x), Ok(y)) => x.digest() != y.digest(),
        _ => false,
    };
    s.add("50137-J1-种子区分", ok, if ok { "" } else { "异种子同摘要" });

    // J1-5 生成覆盖四类 + 端点纪律
    let mut ok = true;
    let mut det = String::new();
    match generate(0x5eed, 32) {
        Ok(set) => {
            let mut i = 0usize;
            while i < set.len() {
                if let Some(x) = set.cases().get(i) {
                    if x.validate().is_err() {
                        ok = false;
                    }
                    if x.kind == XferKind::Control && x.ep != 0 {
                        ok = false;
                    }
                    if x.kind != XferKind::Control && x.ep == 0 {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok = ok && set.coverage() == 0x0f;
            det = format!("cov={:x} n={}", set.coverage(), set.len());
        }
        Err(_) => ok = false,
    }
    s.add_dyn("50137-J1-四类覆盖", ok, &det);

    // J1-6 分包守恒：chunks 是上界且不可再减一包
    let mut ok = true;
    match generate(0x77, 32) {
        Ok(set) => {
            let mut i = 0usize;
            while i < set.len() {
                if let Some(x) = set.cases().get(i) {
                    let mp = x.kind.max_packet();
                    let ch = x.chunks();
                    if ch == 0 || ch * mp < x.bytes || (ch - 1) * mp >= x.bytes {
                        ok = false;
                    }
                }
                i += 1;
            }
        }
        Err(_) => ok = false,
    }
    s.add("50137-J1-分包守恒", ok, if ok { "" } else { "chunks 不是最小上界" });

    // J1-7 注册表顺序纪律：重复/倒退/越界
    let mut ok = false;
    if let Ok(set) = generate(3, 8) {
        let mut dup = set.clone();
        if let Some(last) = set.cases().last().copied() {
            ok = dup.push(last).err() == Some(VecError::Dupe);
        }
        let mut back = set.clone();
        ok = ok
            && back
                .push(UseCase::new(1, XferKind::Bulk, Direction::In, 1, 512))
                .err()
                == Some(VecError::Order)
            && UseCase::new(0, XferKind::Bulk, Direction::In, 1, 512)
                .validate()
                .err()
                == Some(VecError::Range);
    }
    s.add("50137-J1-顺序纪律", ok, if ok { "" } else { "注册表顺序未守" });

    // J1-8 落账守恒：明细合计与集合合计逐项相等
    let mut ok = false;
    let mut det = String::new();
    if let Ok(set) = generate(0xabc, 32) {
        if let Ok(l) = build_ledger(&set) {
            let st = set.totals();
            let lt = l.totals();
            ok = st.0 == lt.0 && st.1 == lt.1 && l.coverage() == set.coverage() && l.len() == set.len();
            det = format!("set={:?} led={:?}", st, lt);
        }
    }
    s.add_dyn("50137-J1-落账守恒", ok, &det);

    // J1-9 封印门槛：空账空、单类不封
    let empty_ok = VectorLedger::new().seal().err() == Some(VecError::Empty);
    let one_class_ok = match bulk_ledger(4) {
        Ok(mut l) => l.seal().err() == Some(VecError::Class),
        Err(_) => false,
    };
    let ok = empty_ok && one_class_ok;
    s.add("50137-J1-封印门槛", ok, if ok { "" } else { "空账/单类被放行" });

    // J1-10 封印后拒绝再封
    let mut ok = false;
    if let Ok(set) = generate(0x88, 16) {
        if let Ok(mut l) = build_ledger(&set) {
            ok = l.seal().is_ok() && l.seal().err() == Some(VecError::State);
        }
    }
    s.add("50137-J1-二次封印拒", ok, if ok { "" } else { "可重复封印" });

    // J1-11 复核幂等：同一根、状态 Verified、重放 +1
    let mut ok = false;
    let mut det = String::new();
    if let Ok(set) = generate(0x99, 16) {
        if let Ok(mut l) = build_ledger(&set) {
            if let Ok(r1) = l.seal() {
                let v1 = l.verify();
                let v2 = l.verify();
                ok = v1 == Ok(r1) && v2 == Ok(r1) && l.state() == LedgerState::Verified && l.replays() == 1;
                det = format!("root={:016x} replays={}", r1, l.replays());
            }
        }
    }
    s.add_dyn("50137-J1-复核幂等", ok, &det);

    // J1-12 篡改必抓：改明细不改根 → E_CSUM，且根不变
    let mut ok = false;
    if let Ok(set) = generate(0xaa, 16) {
        if let Ok(mut l) = build_ledger(&set) {
            if let Ok(root) = l.seal() {
                if let Some(e) = l.entry(3) {
                    let tampered = e.bytes.wrapping_add(1);
                    if l.forge_bytes(3, tampered).is_ok() {
                        ok = l.verify().err() == Some(VecError::Csum) && l.root() == root;
                    }
                }
            }
        }
    }
    s.add("50137-J1-篡改必抓", ok, if ok { "" } else { "改明细未被发现" });

    // J1-13 容量：64 条可收，第 65 条 E_CAP
    let mut ok = false;
    let mut det = String::new();
    if let Ok(mut l) = bulk_ledger(64) {
        let full = l.len();
        let r = l.record(&UseCase::new(65, XferKind::Bulk, Direction::In, 1, 512));
        ok = full == 64 && r.err() == Some(VecError::Cap);
        det = format!("n={} err={:?}", full, r.err().map(|x| x.code()));
    }
    s.add_dyn("50137-J1-容量封顶", ok, &det);

    // J1-14 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50137-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50137_checks_green() {
        let set = run_unx50137_checks();
        assert!(set.all_passed(), "UNX-F50137 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50137_generator_is_deterministic() {
        let a = generate(0x1234, 40);
        let b = generate(0x1234, 40);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(x), Ok(y)) = (a, b) {
            assert_eq!(x.digest(), y.digest());
            assert_eq!(x.totals(), y.totals());
            assert_eq!(x.coverage(), 0x0f);
        }
    }

    #[test]
    fn unx50137_ledger_seal_verify_cycle() {
        let set = match generate(0x42, 16) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut l = VectorLedger::new();
        for i in 0..set.len() {
            if let Some(c) = set.cases().get(i) {
                assert!(l.record(c).is_ok());
            }
        }
        let root = l.seal();
        assert!(root.is_ok());
        assert_eq!(l.verify().ok(), root.ok());
    }
}
