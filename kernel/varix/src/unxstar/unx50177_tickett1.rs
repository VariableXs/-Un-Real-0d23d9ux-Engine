//! UNX-F50177 Ticket 真机判据登记位（T1 档 ×2 台随闸门补测槽位）（0x44240 段）
//!
//! 锚点：UNX-F50177-J1 槽位登记不虚报；判据锚定 §AI-63 §5 诚实三态。
//!
//! 「登记」最容易出的错不是漏登，而是**把没测过的槽位写成通过**。真机判据
//! 要跨两台机器、按闸门分批补测，登记簿因此必须先解决「结论有没有证据」这个
//! 问题，再谈结论本身。本模块把诚实三态（Pass / Fail / Untested）做成类型级
//! 约束，而不是靠调用方自觉：
//!
//! * 任何 Pass/Fail 判定必须携带 ≥1 条证据，否则 `register` 直接拒绝（Overclaim）；
//! * 没有证据的槽位只能是 Untested；带证据却写 Untested 同样拒绝（台账自相矛盾）；
//! * 闸门状态由 A/B 两台设备的槽位**合取**得出：缺台、缺测、任一台 Fail，
//!   都不会得到 Pass——这是「×2 台」在语义上的唯一正确实现；
//! * 补测（supplement）只能把 Untested 推进一步，已判定的槽位封存（Sealed），
//!   杜绝「先记通过、回头再悄悄改」的翻账路径。
//!
//! 数据结构（两类以上真实结构）：
//! * `Tri` —— 诚实三态 + 合取代数（Fail 支配 / Untested 不升级 / 幂等交换）；
//! * `Device` / `SlotKey` / `Slot` —— 2 台设备 × 8 道闸门的定容槽位记录；
//! * `SlotRegistry` —— 登记簿：重复、虚报、证据矛盾、越界、容量满全部显式报错；
//! * `audit()` —— 虚报审计（任何判定槽位必须带证据，返回值为 0 才算干净）。
//!
//! 错误路径：段内闭集 `SEG_BASE + n`，全部码值在注入核/判据里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x44240 独占 0x20）。
pub const SEG_BASE: u32 = 0x44240;

pub const E_DEVICE: u16 = 1;
pub const E_DUP: u16 = 2;
pub const E_OVERCLAIM: u16 = 3;
pub const E_EVIDENCE: u16 = 4;
pub const E_FULL: u16 = 5;
pub const E_MISSING: u16 = 6;
pub const E_SEALED: u16 = 7;
pub const E_NOOP: u16 = 8;
pub const E_GATE: u16 = 9;
pub const E_EMPTY: u16 = 10;
pub const E_TRI: u16 = 11;

/// 槽位登记错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickError {
    Device,
    Dup,
    Overclaim,
    Evidence,
    Full,
    Missing,
    Sealed,
    Noop,
    Gate,
    Empty,
    Tri,
}

impl TickError {
    pub fn code(self) -> u32 {
        let n = match self {
            TickError::Device => E_DEVICE,
            TickError::Dup => E_DUP,
            TickError::Overclaim => E_OVERCLAIM,
            TickError::Evidence => E_EVIDENCE,
            TickError::Full => E_FULL,
            TickError::Missing => E_MISSING,
            TickError::Sealed => E_SEALED,
            TickError::Noop => E_NOOP,
            TickError::Gate => E_GATE,
            TickError::Empty => E_EMPTY,
            TickError::Tri => E_TRI,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            TickError::Device => "device code not in {A,B}",
            TickError::Dup => "slot already registered",
            TickError::Overclaim => "verdict recorded without evidence",
            TickError::Evidence => "evidence field contradicts slot state",
            TickError::Full => "slot registry capacity reached",
            TickError::Missing => "slot not registered",
            TickError::Sealed => "slot already carries a verdict",
            TickError::Noop => "supplement must move off Untested",
            TickError::Gate => "gate index outside 0..8",
            TickError::Empty => "registry capacity must be >= 1",
            TickError::Tri => "tri-state code outside {0,1,2}",
        }
    }
}

/// 诚实三态。编码固定：0 = 未测（唯一可缺证据的状态），1 = 通过，2 = 失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    Untested,
    Pass,
    Fail,
}

impl Tri {
    pub const ALL: [Tri; 3] = [Tri::Untested, Tri::Pass, Tri::Fail];

    pub fn code(self) -> u8 {
        match self {
            Tri::Untested => 0,
            Tri::Pass => 1,
            Tri::Fail => 2,
        }
    }

    pub fn from_code(c: u8) -> Result<Tri, TickError> {
        match c {
            0 => Ok(Tri::Untested),
            1 => Ok(Tri::Pass),
            2 => Ok(Tri::Fail),
            _ => Err(TickError::Tri),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tri::Untested => "untested",
            Tri::Pass => "pass",
            Tri::Fail => "fail",
        }
    }

    /// 是否已给出结论。给出结论就必须有证据——这是登记簿的第一道门。
    pub fn has_verdict(self) -> bool {
        !matches!(self, Tri::Untested)
    }

    /// 合取（闸门语义）：Fail 支配一切；只要有一侧未测就仍是未测。
    ///
    /// 之所以不用「多数票」而用合取：×2 台的判据本来要求两台都过，
    /// 任一台没测过就说明判据整体未被证成，此时报 Pass 就是虚报。
    pub fn combine(self, other: Tri) -> Tri {
        match (self, other) {
            (Tri::Fail, _) => Tri::Fail,
            (_, Tri::Fail) => Tri::Fail,
            (Tri::Untested, _) => Tri::Untested,
            (_, Tri::Untested) => Tri::Untested,
            _ => Tri::Pass,
        }
    }
}

/// 设备两档：A / B。编码固定 1..=2，落账后可跨版本读回。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    A,
    B,
}

impl Device {
    pub const ALL: [Device; 2] = [Device::A, Device::B];

    pub fn code(self) -> u8 {
        match self {
            Device::A => 1,
            Device::B => 2,
        }
    }

    pub fn from_code(c: u8) -> Result<Device, TickError> {
        match c {
            1 => Ok(Device::A),
            2 => Ok(Device::B),
            _ => Err(TickError::Device),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Device::A => "A",
            Device::B => "B",
        }
    }
}

/// 闸门数量。8 道闸门 × 2 台设备 = 16 个槽位，正好等于登记簿上限。
pub const GATES: u8 = 8;
/// 槽位上限：不允许多开槽位绕开「×2 台 + 8 闸门」的固定覆盖面。
pub const MAX_SLOTS: usize = 16;
/// 单槽证据条数上限。证据是计数不是结论，超出说明灌入路径有 bug。
pub const MAX_EVIDENCE: u16 = 4096;

/// 槽位键：设备 + 闸门。闸门越界只能在构造/登记时被抓住，故保留显式校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotKey {
    pub device: Device,
    pub gate: u8,
}

impl SlotKey {
    pub const fn new(device: Device, gate: u8) -> SlotKey {
        SlotKey { device, gate }
    }
}

/// 一条槽位记录。`state` 与 `evidence` 的一致性由登记簿统一把关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub key: SlotKey,
    pub state: Tri,
    pub evidence: u16,
}

/// 登记簿审计结果：`verdict_slots` 是给出结论的槽位数，`unsupported` 是其中
/// 没有证据的条数。J1「不虚报」要求 `unsupported == 0`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audit {
    pub total: usize,
    pub verdict_slots: usize,
    pub unsupported: usize,
}

/// 定容槽位登记簿。
pub struct SlotRegistry {
    slots: Vec<Slot>,
    cap: usize,
}

impl SlotRegistry {
    /// 新建登记簿；容量 0 没有意义（任何登记都会立刻失败），显式拒绝。
    pub fn new(cap: usize) -> Result<SlotRegistry, TickError> {
        if cap == 0 || cap > MAX_SLOTS {
            return Err(TickError::Empty);
        }
        Ok(SlotRegistry {
            slots: Vec::with_capacity(cap),
            cap,
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    fn find(&self, key: SlotKey) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记槽位。校验顺序：闸门 → 证据上界 → 状态/证据一致性 → 容量 → 重复。
    ///
    /// 一致性与容量放在重复检测之前，是为了让「坏样本」优先撞上语义错误：
    /// 重复登记一个虚报槽位时，先报 Overclaim 比先报 Dup 更能说明问题出在哪。
    pub fn register(&mut self, key: SlotKey, state: Tri, evidence: u16) -> Result<(), TickError> {
        if key.gate >= GATES {
            return Err(TickError::Gate);
        }
        if evidence > MAX_EVIDENCE {
            return Err(TickError::Evidence);
        }
        if state.has_verdict() && evidence == 0 {
            return Err(TickError::Overclaim);
        }
        if !state.has_verdict() && evidence != 0 {
            return Err(TickError::Evidence);
        }
        if self.slots.len() >= self.cap {
            return Err(TickError::Full);
        }
        if self.find(key).is_some() {
            return Err(TickError::Dup);
        }
        self.slots.push(Slot {
            key,
            state,
            evidence,
        });
        Ok(())
    }

    /// 补测：只允许把 Untested 推进一步。
    ///
    /// 已判定的槽位一律 Sealed——真机结论要改，必须新开一条槽位并留下新的
    /// 证据链；就地改写会让「历史结论」与「当前结论」不可区分。
    pub fn supplement(
        &mut self,
        key: SlotKey,
        state: Tri,
        evidence: u16,
    ) -> Result<Tri, TickError> {
        if key.gate >= GATES {
            return Err(TickError::Gate);
        }
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(TickError::Missing),
        };
        let cur = match self.slots.get(idx) {
            Some(slot) => slot.state,
            None => return Err(TickError::Missing),
        };
        if cur.has_verdict() {
            return Err(TickError::Sealed);
        }
        if !state.has_verdict() {
            return Err(TickError::Noop);
        }
        if evidence == 0 || evidence > MAX_EVIDENCE {
            return Err(TickError::Overclaim);
        }
        if let Some(slot) = self.slots.get_mut(idx) {
            slot.state = state;
            slot.evidence = evidence;
        }
        Ok(state)
    }

    pub fn get(&self, key: SlotKey) -> Option<Slot> {
        match self.find(key) {
            Some(i) => match self.slots.get(i) {
                Some(s) => Some(*s),
                None => None,
            },
            None => None,
        }
    }

    /// 单台设备在某闸门上的状态；未登记按 Untested（诚实默认，不是错误）。
    pub fn device_state(&self, device: Device, gate: u8) -> Tri {
        match self.get(SlotKey::new(device, gate)) {
            Some(s) => s.state,
            None => Tri::Untested,
        }
    }

    /// 闸门状态 = 两台设备合取。缺台不会被当成通过。
    pub fn gate_state(&self, gate: u8) -> Result<Tri, TickError> {
        if gate >= GATES {
            return Err(TickError::Gate);
        }
        let a = self.device_state(Device::A, gate);
        let b = self.device_state(Device::B, gate);
        Ok(a.combine(b))
    }

    /// 三态计数（Pass, Fail, Untested）。
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut pass = 0usize;
        let mut fail = 0usize;
        let mut untested = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                match s.state {
                    Tri::Pass => pass += 1,
                    Tri::Fail => fail += 1,
                    Tri::Untested => untested += 1,
                }
            }
            i += 1;
        }
        (pass, fail, untested)
    }

    /// 虚报审计：返回登记总数、判定槽位数、其中无证据的条数。
    pub fn audit(&self) -> Audit {
        let mut verdict_slots = 0usize;
        let mut unsupported = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.state.has_verdict() {
                    verdict_slots += 1;
                    if s.evidence == 0 {
                        unsupported += 1;
                    }
                }
            }
            i += 1;
        }
        Audit {
            total: self.slots.len(),
            verdict_slots,
            unsupported,
        }
    }

    /// 登记簿摘要：按登记序累积 (设备码, 闸门, 状态码, 证据)。
    /// 只用于「同序重放一致」，不承担跨序归一化的职责。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.slots.len() as u64;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                h = fnv_u8(h, s.key.device.code());
                h = fnv_u8(h, s.key.gate);
                h = fnv_u8(h, s.state.code());
                h = fnv_u8(h, (s.evidence >> 8) as u8);
                h = fnv_u8(h, s.evidence as u8);
            }
            i += 1;
        }
        h
    }

    /// 诊断文本：一条一行，便于在失败路径上直接读。
    pub fn render(&self) -> String {
        let (pass, fail, untested) = self.counts();
        let audit = self.audit();
        format!(
            "slots={}/{} pass={} fail={} untested={} verdict={} unsupported={}",
            self.slots.len(),
            self.cap,
            pass,
            fail,
            untested,
            audit.verdict_slots,
            audit.unsupported
        )
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

// ---------------------------------------------------------------------------
// 注入核：十类坏样本，逐类断言专属诊断码
// ---------------------------------------------------------------------------

/// 返回 (被抓数, 样本总数)。任何一类没被抓到，J1「不虚报」就失去意义。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;
    let key = SlotKey::new(Device::A, 3);

    // 1 无证据的 Pass（虚报）
    let mut r = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if r.register(key, Tri::Pass, 0).err() == Some(TickError::Overclaim) {
        caught += 1;
    }

    // 2 未测却带证据（台账自相矛盾）
    let mut r2 = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if r2.register(key, Tri::Untested, 3).err() == Some(TickError::Evidence) {
        caught += 1;
    }

    // 3 重复槽位
    let mut r3 = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if r3.register(key, Tri::Untested, 0).is_ok()
        && r3.register(key, Tri::Pass, 1).err() == Some(TickError::Dup)
    {
        caught += 1;
    }

    // 4 已判定槽位再补测（封存）
    let mut r4 = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if r4.register(key, Tri::Pass, 2).is_ok()
        && r4.supplement(key, Tri::Fail, 1).err() == Some(TickError::Sealed)
    {
        caught += 1;
    }

    // 5 补测未登记槽位
    let mut r5 = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if r5.supplement(key, Tri::Pass, 1).err() == Some(TickError::Missing) {
        caught += 1;
    }

    // 6 补测到 Untested（空动作）
    let mut r6 = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if r6.register(key, Tri::Untested, 0).is_ok()
        && r6.supplement(key, Tri::Untested, 1).err() == Some(TickError::Noop)
    {
        caught += 1;
    }

    // 7 非法设备码
    if Device::from_code(9).err() == Some(TickError::Device) {
        caught += 1;
    }

    // 8 越界闸门
    let mut r8 = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if r8
        .register(SlotKey::new(Device::B, GATES), Tri::Untested, 0)
        .err()
        == Some(TickError::Gate)
    {
        caught += 1;
    }

    // 9 零容量登记簿
    if SlotRegistry::new(0).err() == Some(TickError::Empty) {
        caught += 1;
    }

    // 10 容量满
    let mut r10 = match SlotRegistry::new(1) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if r10.register(key, Tri::Untested, 0).is_ok()
        && r10
            .register(SlotKey::new(Device::B, 3), Tri::Untested, 0)
            .err()
            == Some(TickError::Full)
    {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50177_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50177");

    // J1-1 未测不被通过升级（诚实三态的核心性质）
    let ok = Tri::Untested.combine(Tri::Pass) == Tri::Untested
        && Tri::Pass.combine(Tri::Untested) == Tri::Untested;
    s.add(
        "50177-J1-未测不上调",
        ok,
        if ok { "" } else { "Untested 被并入 Pass" },
    );

    // J1-2 Fail 支配
    let ok = Tri::Fail.combine(Tri::Pass) == Tri::Fail
        && Tri::Pass.combine(Tri::Fail) == Tri::Fail
        && Tri::Fail.combine(Tri::Untested) == Tri::Fail
        && Tri::Fail.combine(Tri::Fail) == Tri::Fail;
    s.add(
        "50177-J1-失败支配",
        ok,
        if ok { "" } else { "Fail 未支配" },
    );

    // J1-3 合取代数性质：幂等 / 交换 / 结合（3^3 全遍历）
    let mut ok = true;
    let mut i = 0usize;
    while i < Tri::ALL.len() {
        let mut j = 0usize;
        while j < Tri::ALL.len() {
            let (a, b) = match (Tri::ALL.get(i), Tri::ALL.get(j)) {
                (Some(x), Some(y)) => (*x, *y),
                _ => {
                    ok = false;
                    break;
                }
            };
            if a.combine(b) != b.combine(a) || a.combine(a) != a {
                ok = false;
            }
            let mut k = 0usize;
            while k < Tri::ALL.len() {
                if let Some(c) = Tri::ALL.get(k) {
                    if a.combine(b).combine(*c) != a.combine(b.combine(*c)) {
                        ok = false;
                    }
                }
                k += 1;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "50177-J1-合取代数",
        ok,
        if ok { "" } else { "合取非幂等/非交换/非结合" },
    );

    // J1-4 三态码值闭集（0/1/2 可解，3 必拒）
    let ok = Tri::from_code(0).ok() == Some(Tri::Untested)
        && Tri::from_code(1).ok() == Some(Tri::Pass)
        && Tri::from_code(2).ok() == Some(Tri::Fail)
        && Tri::from_code(3).err() == Some(TickError::Tri)
        && Device::from_code(1).ok() == Some(Device::A)
        && Device::from_code(2).ok() == Some(Device::B)
        && Device::from_code(0).err() == Some(TickError::Device);
    s.add(
        "50177-J1-码值闭集",
        ok,
        if ok { "" } else { "码值越界被放行" },
    );

    // J1-5 合法登记簿的虚报审计必须干净
    let mut r = match SlotRegistry::new(16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = r.register(SlotKey::new(Device::A, 0), Tri::Pass, 2);
    let _ = r.register(SlotKey::new(Device::B, 0), Tri::Fail, 1);
    let _ = r.register(SlotKey::new(Device::A, 1), Tri::Untested, 0);
    let audit = r.audit();
    let ok = audit.total == 3 && audit.verdict_slots == 2 && audit.unsupported == 0;
    let det = format!(
        "total={} verdict={} unsupported={}",
        audit.total, audit.verdict_slots, audit.unsupported
    );
    s.add_dyn("50177-J1-审计干净", ok, &det);

    // J1-6 无证据判定必拒（登记不虚报的第一道门）
    let mut r2 = match SlotRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = r2
        .register(SlotKey::new(Device::A, 2), Tri::Pass, 0)
        .err()
        == Some(TickError::Overclaim)
        && r2
            .register(SlotKey::new(Device::A, 2), Tri::Fail, 0)
            .err()
            == Some(TickError::Overclaim)
        && r2.len() == 0;
    s.add(
        "50177-J1-无据必拒",
        ok,
        if ok { "" } else { "无证据判定被放行" },
    );

    // J1-7 证据与状态矛盾必拒
    let mut r3 = match SlotRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = r3
        .register(SlotKey::new(Device::B, 2), Tri::Untested, 5)
        .err()
        == Some(TickError::Evidence)
        && r3
            .register(SlotKey::new(Device::B, 2), Tri::Pass, MAX_EVIDENCE + 1)
            .err()
            == Some(TickError::Evidence);
    s.add(
        "50177-J1-证据矛盾",
        ok,
        if ok { "" } else { "状态与证据不符仍入账" },
    );

    // J1-8 重复槽位必拒（登记簿是集合不是日志）
    let mut r4 = match SlotRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let key = SlotKey::new(Device::A, 5);
    let ok = r4.register(key, Tri::Untested, 0).is_ok()
        && r4.register(key, Tri::Pass, 1).err() == Some(TickError::Dup)
        && r4.len() == 1;
    s.add(
        "50177-J1-重复必拒",
        ok,
        if ok { "" } else { "重复槽位改变登记数" },
    );

    // J1-9 闸门需双台：只 A 通过时未测；A+B 都通过才 Pass
    let mut r5 = match SlotRegistry::new(16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = r5.register(SlotKey::new(Device::A, 3), Tri::Pass, 2);
    let only_a = r5.gate_state(3).ok();
    let _ = r5.register(SlotKey::new(Device::B, 3), Tri::Pass, 1);
    let both = r5.gate_state(3).ok();
    let ok = only_a == Some(Tri::Untested) && both == Some(Tri::Pass);
    let det = format!("only_a={:?} both={:?}", only_a, both);
    s.add_dyn("50177-J1-双台合取", ok, &det);

    // J1-10 缺台不虚报：全新登记簿里只登记 B，闸门仍是未测
    let mut r6 = match SlotRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = r6.register(SlotKey::new(Device::B, 6), Tri::Pass, 1);
    let ok = r6.gate_state(6).ok() == Some(Tri::Untested)
        && r6.device_state(Device::A, 6) == Tri::Untested;
    s.add(
        "50177-J1-缺台不虚报",
        ok,
        if ok { "" } else { "单台通过被当成闸门通过" },
    );

    // J1-11 补测只进不退：Untested → Pass 成功，再补测封存
    let mut r7 = match SlotRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let k7 = SlotKey::new(Device::A, 7);
    let ok = r7.register(k7, Tri::Untested, 0).is_ok()
        && r7.supplement(k7, Tri::Pass, 2).ok() == Some(Tri::Pass)
        && r7.supplement(k7, Tri::Fail, 1).err() == Some(TickError::Sealed)
        && r7.get(k7).map(|x| x.state) == Some(Tri::Pass);
    s.add(
        "50177-J1-补测单向",
        ok,
        if ok { "" } else { "已判定槽位被改写" },
    );

    // J1-12 补测守恒：登记数不变、证据落到槽位上
    let mut r8 = match SlotRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let k8 = SlotKey::new(Device::B, 7);
    let _ = r8.register(k8, Tri::Untested, 0);
    let before = r8.len();
    let _ = r8.supplement(k8, Tri::Fail, 3);
    let ok = r8.len() == before
        && r8.get(k8).map(|x| x.evidence) == Some(3)
        && r8.get(k8).map(|x| x.state) == Some(Tri::Fail);
    s.add(
        "50177-J1-补测守恒",
        ok,
        if ok { "" } else { "补测改变槽位数/证据丢失" },
    );

    // J1-13 空登记簿闸门恒为未测（空集不能算通过）
    let r9 = match SlotRegistry::new(2) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut ok = r9.is_empty();
    let mut g = 0u8;
    while g < GATES {
        if r9.gate_state(g).ok() != Some(Tri::Untested) {
            ok = false;
        }
        g += 1;
    }
    s.add(
        "50177-J1-空簿未测",
        ok,
        if ok { "" } else { "空登记簿产出非未测结论" },
    );

    // J1-14 越界闸门 / 非法设备必拒
    let mut r10 = match SlotRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = r10.gate_state(GATES).err() == Some(TickError::Gate)
        && r10
            .register(SlotKey::new(Device::A, GATES), Tri::Untested, 0)
            .err()
            == Some(TickError::Gate)
        && r10.supplement(SlotKey::new(Device::A, GATES), Tri::Pass, 1).err()
            == Some(TickError::Gate)
        && Device::from_code(3).err() == Some(TickError::Device);
    s.add(
        "50177-J1-越界必拒",
        ok,
        if ok { "" } else { "越界参数被接受" },
    );

    // J1-15 容量满必拒（不静默丢登记）
    let mut r11 = match SlotRegistry::new(1) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = r11
        .register(SlotKey::new(Device::A, 0), Tri::Untested, 0)
        .is_ok()
        && r11
            .register(SlotKey::new(Device::A, 1), Tri::Untested, 0)
            .err()
            == Some(TickError::Full)
        && r11.len() == 1;
    s.add(
        "50177-J1-容量满拒",
        ok,
        if ok { "" } else { "超容登记被静默吞" },
    );

    // J1-16 摘要确定性：同序重放逐位一致，且对状态变化敏感
    let mut a = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut b = match SlotRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ka = SlotKey::new(Device::A, 0);
    let kb = SlotKey::new(Device::B, 1);
    let _ = a.register(ka, Tri::Pass, 1);
    let _ = a.register(kb, Tri::Untested, 0);
    let _ = b.register(ka, Tri::Pass, 1);
    let d_before = b.digest();
    let _ = b.register(kb, Tri::Untested, 0);
    let ok = a.digest() == b.digest()
        && a.digest() != d_before
        && a.digest() == a.digest();
    let det = format!("{:x}", a.digest());
    s.add_dyn("50177-J1-摘要确定", ok, &det);

    // J1-17 注入核：十类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50177-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50177_checks_green() {
        let set = run_unx50177_checks();
        assert!(set.all_passed(), "UNX-F50177 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50177_untested_never_upgrades() {
        assert_eq!(Tri::Untested.combine(Tri::Pass), Tri::Untested);
        assert_eq!(Tri::Pass.combine(Tri::Untested), Tri::Untested);
        assert_eq!(Tri::Untested.combine(Tri::Fail), Tri::Fail);
    }

    #[test]
    fn unx50177_supplement_is_one_way() {
        let mut r = SlotRegistry::new(2);
        assert!(r.is_ok());
        if let Ok(reg) = r.as_mut() {
            let k = SlotKey::new(Device::A, 1);
            assert_eq!(reg.register(k, Tri::Untested, 0), Ok(()));
            assert_eq!(reg.supplement(k, Tri::Pass, 2), Ok(Tri::Pass));
            assert_eq!(reg.supplement(k, Tri::Fail, 1), Err(TickError::Sealed));
        }
    }
}
