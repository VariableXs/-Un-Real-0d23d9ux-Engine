//! UNX-F50134 真机 USB 打印判据登记位（T1 档 ×2 台随闸门补测槽位）（0x43ce0 段）
//!
//! 锚点：UNX-F50134-J1 槽位登记不虚报；判据锚定 §AI-63 §5 诚实三态。
//!
//! 真机判据最容易被写虚：槽位「登记了」被当成「测过了」，「测过了」又被当成
//! 「通过了」。本模块把三件事拆成三个可审计的概念：
//! * 槽位（Slot）：占位与档位，登记即 Armed，只表示「这个判据有人认领」；
//! * 证据（Evidence）：页数、字节数、witness 号，三者自洽且带 CRC 才算数；
//! * 判据结果：只有带有效证据的 `Passed` / `Failed` 才进结算，`Armed` 永远
//!   落在 pending 侧——**pending 绝不并入 passed**，这就是「不虚报」。
//!
//! T1 档恰好两台机器（`T1_MACHINES = 2`），第三台一律拒绝：T1 的口径是
//! 「两台真机各跑一遍」，多登记一台会让「×2 台」这句话失去计量意义。
//!
//! 闸门（gate）：两台机器都登记且各自至少有一个 T1 槽位后闸门才可开。闸门
//! 关闭期间不得登记补测槽（那等于绕过闸门先跑）；闸门开启后不得再登记机器或
//! 基座 T1 槽（那等于事后追认）。补测槽以 `Tier::Gate` 登记，天然落在 pending
//! 侧，必须同样拿出证据才能结算。
//!
//! 数据结构：
//! * `Tier` —— 两档（T1 基座 / Gate 补测）；
//! * `SlotState` —— 三态状态机（Armed → Passed | Failed，终态不可重写）；
//! * `Evidence` —— 证据记录（页/字节/witness/CRC，含 pass 与 fail 两套判据）；
//! * `Machine` / `Slot` —— 真机与其判据位（定容）；
//! * `T1Gate` —— 登记表 + 闸门状态机 + 结算（`Tally`）。
//!
//! 错误路径：`T1Error::code()` 返回 `SEG_BASE + n`，段内闭集；全部码值都在判据
//! 或注入核里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43ce0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43ce0;

pub const E_EMPTY: u16 = 1;
pub const E_CAPACITY: u16 = 2;
pub const E_DUP_MACHINE: u16 = 3;
pub const E_DUP_SLOT: u16 = 4;
pub const E_GATE_SHUT: u16 = 5;
pub const E_GATE_OPEN: u16 = 6;
pub const E_UNKNOWN_MACHINE: u16 = 7;
pub const E_UNKNOWN_SLOT: u16 = 8;
pub const E_TIER: u16 = 9;
pub const E_EVIDENCE: u16 = 10;
pub const E_CRC: u16 = 11;
pub const E_PENDING: u16 = 12;
pub const E_LOCKED: u16 = 13;
pub const E_MACHINE_ABSENT: u16 = 14;
pub const E_COUNT_MISMATCH: u16 = 15;

/// T1 档机器台数（「×2 台」是判据口径，不是建议值）。
pub const T1_MACHINES: usize = 2;

/// 单机判据槽位上限（T1 基座 + 闸门补测 + 余量）。
pub const MAX_SLOTS: usize = 8;

/// 每页最少字节数。取 64：向量账里最短的有效页载荷就是 64 字节
/// （`unx50133` 的 bulk-in 用例），比它更少说明统计到的不是真页。
pub const MIN_PAGE_BYTES: u32 = 64;

/// T1 判据错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum T1Error {
    Empty,
    Capacity,
    DupMachine,
    DupSlot,
    GateShut,
    GateOpen,
    UnknownMachine,
    UnknownSlot,
    Tier,
    Evidence,
    Crc,
    Pending,
    Locked,
    MachineAbsent,
    CountMismatch,
}

impl T1Error {
    /// 诊断码：段基址 + 段内序号（闭集，全仓唯一）。
    pub fn code(self) -> u32 {
        let n = match self {
            T1Error::Empty => E_EMPTY,
            T1Error::Capacity => E_CAPACITY,
            T1Error::DupMachine => E_DUP_MACHINE,
            T1Error::DupSlot => E_DUP_SLOT,
            T1Error::GateShut => E_GATE_SHUT,
            T1Error::GateOpen => E_GATE_OPEN,
            T1Error::UnknownMachine => E_UNKNOWN_MACHINE,
            T1Error::UnknownSlot => E_UNKNOWN_SLOT,
            T1Error::Tier => E_TIER,
            T1Error::Evidence => E_EVIDENCE,
            T1Error::Crc => E_CRC,
            T1Error::Pending => E_PENDING,
            T1Error::Locked => E_LOCKED,
            T1Error::MachineAbsent => E_MACHINE_ABSENT,
            T1Error::CountMismatch => E_COUNT_MISMATCH,
        };
        SEG_BASE + n as u32
    }

    /// 一行可读原因（诊断/日志共用）。
    pub fn message(self) -> &'static str {
        match self {
            T1Error::Empty => "empty machine id or slot name",
            T1Error::Capacity => "machine or slot capacity exhausted",
            T1Error::DupMachine => "machine already registered",
            T1Error::DupSlot => "slot already declared on this machine",
            T1Error::GateShut => "supplementary slot before gate opened",
            T1Error::GateOpen => "base registration after gate opened",
            T1Error::UnknownMachine => "machine index out of range",
            T1Error::UnknownSlot => "slot name not declared on machine",
            T1Error::Tier => "tier code outside 1..=2",
            T1Error::Evidence => "evidence does not satisfy the tier criterion",
            T1Error::Crc => "evidence crc mismatch",
            T1Error::Pending => "slot still armed: no verdict yet",
            T1Error::Locked => "slot already decided",
            T1Error::MachineAbsent => "fewer than two T1 machines",
            T1Error::CountMismatch => "registry claims a verdict without evidence",
        }
    }
}

/// 两档：T1 基座（闸门关闭期登记）与 Gate 补测（闸门开启后登记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    T1,
    Gate,
}

impl Tier {
    pub fn code(self) -> u8 {
        match self {
            Tier::T1 => 1,
            Tier::Gate => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::T1 => "T1",
            Tier::Gate => "gate",
        }
    }

    /// 码值 → 档位。0 与 >2 都是保留值。
    pub fn from_code(c: u8) -> Result<Tier, T1Error> {
        match c {
            1 => Ok(Tier::T1),
            2 => Ok(Tier::Gate),
            _ => Err(T1Error::Tier),
        }
    }
}

/// 槽位三态。`Armed` 只表示占位；`Passed` / `Failed` 是终态，且必须带证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Armed,
    Passed,
    Failed,
}

/// 真机证据。`pages` 是打印页数，`bytes` 是页载荷字节数，`witness` 是现场
/// 见证号（作业号/时间戳低位，禁止为 0——0 表示「没有见证」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub pages: u16,
    pub bytes: u32,
    pub witness: u32,
    pub crc: u16,
}

fn fnv32_byte(mut h: u32, b: u8) -> u32 {
    h ^= b as u32;
    h.wrapping_mul(0x0100_0193)
}

impl Evidence {
    /// 16 位 CRC：折叠页数、字节数、witness。结果恒为非零——
    /// 0 保留给「未签发的证据」，任何真签发的证据都必须带非零 CRC。
    fn compute(&self) -> u16 {
        let mut h: u32 = 0x811c_9dc5;
        h = fnv32_byte(h, (self.pages & 0xff) as u8);
        h = fnv32_byte(h, (self.pages >> 8) as u8);
        let mut i = 0usize;
        while i < 4 {
            h = fnv32_byte(h, ((self.bytes >> (8 * i)) & 0xff) as u8);
            i += 1;
        }
        let mut k = 0usize;
        while k < 4 {
            h = fnv32_byte(h, ((self.witness >> (8 * k)) & 0xff) as u8);
            k += 1;
        }
        let v = (h ^ (h >> 16)) as u16;
        if v == 0 {
            1
        } else {
            v
        }
    }

    /// 构造一条**通过侧**证据：页数 > 0、witness 非 0、且每页至少 64 字节。
    pub fn pass(pages: u16, bytes: u32, witness: u32) -> Result<Evidence, T1Error> {
        let mut e = Evidence {
            pages,
            bytes,
            witness,
            crc: 0,
        };
        match e.range_defect_pass() {
            Some(err) => Err(err),
            None => {
                e.crc = e.compute();
                Ok(e)
            }
        }
    }

    /// 构造一条**失败侧**证据：允许 0 页（失败时往往一页都没出），
    /// 但 witness 必须非 0——「失败了」同样要有人签字。
    pub fn fail(pages: u16, bytes: u32, witness: u32) -> Result<Evidence, T1Error> {
        let mut e = Evidence {
            pages,
            bytes,
            witness,
            crc: 0,
        };
        match e.range_defect_fail() {
            Some(err) => Err(err),
            None => {
                e.crc = e.compute();
                Ok(e)
            }
        }
    }

    /// 不做校验的原始构造（只供 CRC 注入用；正常路径请走 `pass`/`fail`）。
    pub fn raw(pages: u16, bytes: u32, witness: u32, crc: u16) -> Evidence {
        Evidence {
            pages,
            bytes,
            witness,
            crc,
        }
    }

    /// 通过侧**范围**校验（不含 CRC：构造期 CRC 尚未算出）。
    fn range_defect_pass(&self) -> Option<T1Error> {
        if self.pages == 0 || self.witness == 0 {
            return Some(T1Error::Evidence);
        }
        if self.bytes < self.pages as u32 * MIN_PAGE_BYTES {
            return Some(T1Error::Evidence);
        }
        None
    }

    /// 失败侧范围校验：witness 必须有，页数可为 0。
    fn range_defect_fail(&self) -> Option<T1Error> {
        if self.witness == 0 {
            return Some(T1Error::Evidence);
        }
        None
    }

    /// 通过侧全量校验：范围 + CRC。CRC 不符一律 `Crc`，不降级成「范围错误」。
    fn pass_defect(&self) -> Option<T1Error> {
        match self.range_defect_pass() {
            Some(err) => Some(err),
            None => {
                if self.crc != self.compute() {
                    Some(T1Error::Crc)
                } else {
                    None
                }
            }
        }
    }

    /// 失败侧全量校验：范围 + CRC。
    fn fail_defect(&self) -> Option<T1Error> {
        match self.range_defect_fail() {
            Some(err) => Some(err),
            None => {
                if self.crc != self.compute() {
                    Some(T1Error::Crc)
                } else {
                    None
                }
            }
        }
    }

    /// 通过侧证据是否完全自洽（页/字节/witness/CRC 四项全过）。
    pub fn pass_valid(&self) -> bool {
        self.pass_defect().is_none()
    }

    /// 失败侧证据是否完全自洽（witness/CRC 两项过，页数可为 0）。
    pub fn fail_valid(&self) -> bool {
        self.fail_defect().is_none()
    }
}

/// 一个判据槽位。
#[derive(Clone, Debug)]
pub struct Slot {
    pub name: &'static str,
    pub tier: Tier,
    pub state: SlotState,
    pub evidence: Option<Evidence>,
}

/// 一台真机。
#[derive(Clone, Debug)]
pub struct Machine {
    pub id: &'static str,
    pub slots: Vec<Slot>,
}

/// 结算结果。`pending` 是 Armed 槽位数——它永远单独成项，绝不并入 passed。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pending: usize,
    pub machines: usize,
    pub gate_open: bool,
}

impl Tally {
    /// 三态守恒：三项之和必须等于槽位总数，且没有任何一项是负数（usize 天然成立，
    /// 这里保留为显式断言口，供聚合器复用）。
    pub fn conserved(&self) -> bool {
        self.passed + self.failed + self.pending == self.total
    }
}

/// T1 登记表 + 闸门状态机。
#[derive(Clone, Debug)]
pub struct T1Gate {
    machines: Vec<Machine>,
    gate: bool,
    supplements: u16,
}

impl T1Gate {
    pub fn new() -> T1Gate {
        T1Gate {
            machines: Vec::with_capacity(T1_MACHINES),
            gate: false,
            supplements: 0,
        }
    }

    pub fn machine_count(&self) -> usize {
        self.machines.len()
    }

    pub fn gate_open(&self) -> bool {
        self.gate
    }

    pub fn supplements(&self) -> u16 {
        self.supplements
    }

    pub fn machine(&self, i: usize) -> Option<&Machine> {
        self.machines.get(i)
    }

    /// 登记一台真机。闸门开启后禁止追加——事后追认的机器不在 T1 口径内。
    pub fn register_machine(&mut self, id: &'static str) -> Result<usize, T1Error> {
        if id.is_empty() {
            return Err(T1Error::Empty);
        }
        if self.gate {
            return Err(T1Error::GateOpen);
        }
        // 先查重复再查容量：重复登记是「名字错了」，容量是「台数够了」，
        // 两者都发生在满员表上时，报错应指向真正的错因（重名）。
        let mut i = 0usize;
        while i < self.machines.len() {
            if let Some(m) = self.machines.get(i) {
                if m.id == id {
                    return Err(T1Error::DupMachine);
                }
            }
            i += 1;
        }
        if self.machines.len() >= T1_MACHINES {
            return Err(T1Error::Capacity);
        }
        self.machines.push(Machine {
            id,
            slots: Vec::with_capacity(MAX_SLOTS),
        });
        Ok(self.machines.len() - 1)
    }

    fn machine_index(&self, i: usize) -> Result<usize, T1Error> {
        if i < self.machines.len() {
            Ok(i)
        } else {
            Err(T1Error::UnknownMachine)
        }
    }

    fn slot_index(&self, m: usize, name: &str) -> Result<usize, T1Error> {
        let mi = self.machine_index(m)?;
        let slots = match self.machines.get(mi) {
            Some(x) => &x.slots,
            None => return Err(T1Error::UnknownMachine),
        };
        let mut i = 0usize;
        while i < slots.len() {
            if let Some(s) = slots.get(i) {
                if s.name == name {
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(T1Error::UnknownSlot)
    }

    /// 登记一个判据槽位（登记即 Armed，绝不等于「通过」）。
    ///
    /// 档位与闸门必须匹配：闸门关着不许登记补测槽（绕过闸门先跑），
    /// 闸门开着不许登记基座 T1 槽（事后追认）。
    pub fn declare(&mut self, m: usize, name: &'static str, tier: Tier) -> Result<usize, T1Error> {
        if name.is_empty() {
            return Err(T1Error::Empty);
        }
        let mi = self.machine_index(m)?;
        if !self.gate && tier == Tier::Gate {
            return Err(T1Error::GateShut);
        }
        if self.gate && tier == Tier::T1 {
            return Err(T1Error::GateOpen);
        }
        let machine = match self.machines.get_mut(mi) {
            Some(x) => x,
            None => return Err(T1Error::UnknownMachine),
        };
        if machine.slots.len() >= MAX_SLOTS {
            return Err(T1Error::Capacity);
        }
        let mut i = 0usize;
        while i < machine.slots.len() {
            if let Some(s) = machine.slots.get(i) {
                if s.name == name {
                    return Err(T1Error::DupSlot);
                }
            }
            i += 1;
        }
        machine.slots.push(Slot {
            name,
            tier,
            state: SlotState::Armed,
            evidence: None,
        });
        if tier == Tier::Gate {
            self.supplements = self.supplements.saturating_add(1);
        }
        Ok(machine.slots.len() - 1)
    }

    /// 记录通过：证据四项自洽才落 Passed，否则状态不动（不虚报）。
    pub fn record_pass(&mut self, m: usize, name: &str, ev: Evidence) -> Result<SlotState, T1Error> {
        self.record_inner(m, name, ev, true)
    }

    /// 记录失败：失败侧证据自洽才落 Failed，否则状态不动。
    pub fn record_fail(&mut self, m: usize, name: &str, ev: Evidence) -> Result<SlotState, T1Error> {
        self.record_inner(m, name, ev, false)
    }

    fn record_inner(
        &mut self,
        m: usize,
        name: &str,
        ev: Evidence,
        pass: bool,
    ) -> Result<SlotState, T1Error> {
        let si = self.slot_index(m, name)?;
        let mi = self.machine_index(m)?;
        let machine = match self.machines.get_mut(mi) {
            Some(x) => x,
            None => return Err(T1Error::UnknownMachine),
        };
        let slot = match machine.slots.get_mut(si) {
            Some(s) => s,
            None => return Err(T1Error::UnknownSlot),
        };
        // 终态锁：已判定的槽位不许被「再测一次」覆盖，否则失败会被通过悄悄挤掉。
        if slot.state != SlotState::Armed {
            return Err(T1Error::Locked);
        }
        if pass {
            if let Some(err) = ev.pass_defect() {
                return Err(err);
            }
            slot.state = SlotState::Passed;
        } else {
            if let Some(err) = ev.fail_defect() {
                return Err(err);
            }
            slot.state = SlotState::Failed;
        }
        slot.evidence = Some(ev);
        Ok(slot.state)
    }

    /// 「无证据直通」入口——故意保留，用来证明它被拒。
    ///
    /// 现场总有「我看它打出来了」的口头结论；本函数就是那句口头结论的 API 化。
    /// 它必须**永远失败**，且失败后槽位仍为 Armed。
    pub fn force_pass(&mut self, m: usize, name: &str) -> Result<SlotState, T1Error> {
        let _ = self.slot_index(m, name)?;
        Err(T1Error::Evidence)
    }

    /// 开闸：两台机器都登记、且各自至少有一个 T1 槽位。
    pub fn open_gate(&mut self) -> Result<(), T1Error> {
        if self.gate {
            // 重复开闸是幂等的，不报错：闸门是状态不是事件。
            return Ok(());
        }
        if self.machines.len() < T1_MACHINES {
            return Err(T1Error::MachineAbsent);
        }
        let mut i = 0usize;
        while i < self.machines.len() {
            let mut base = 0usize;
            if let Some(m) = self.machines.get(i) {
                let mut k = 0usize;
                while k < m.slots.len() {
                    if let Some(s) = m.slots.get(k) {
                        if s.tier == Tier::T1 {
                            base += 1;
                        }
                    }
                    k += 1;
                }
            }
            if base == 0 {
                return Err(T1Error::Pending);
            }
            i += 1;
        }
        self.gate = true;
        Ok(())
    }

    /// 结算：逐槽位复核，任何「有结论但没证据」的槽位都算登记造假。
    pub fn audit(&self) -> Result<Tally, T1Error> {
        let mut total = 0usize;
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut pending = 0usize;
        let mut i = 0usize;
        while i < self.machines.len() {
            if let Some(m) = self.machines.get(i) {
                let mut k = 0usize;
                while k < m.slots.len() {
                    if let Some(s) = m.slots.get(k) {
                        total += 1;
                        match s.state {
                            SlotState::Armed => pending += 1,
                            SlotState::Passed => match s.evidence {
                                Some(e) if e.pass_valid() => passed += 1,
                                _ => return Err(T1Error::CountMismatch),
                            },
                            SlotState::Failed => match s.evidence {
                                Some(e) if e.fail_valid() => failed += 1,
                                _ => return Err(T1Error::CountMismatch),
                            },
                        }
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        if passed + failed + pending != total {
            return Err(T1Error::CountMismatch);
        }
        Ok(Tally {
            total,
            passed,
            failed,
            pending,
            machines: self.machines.len(),
            gate_open: self.gate,
        })
    }

    /// 只读结算（不校验）：渲染/日志用，判据一律走 `audit`。
    pub fn tally(&self) -> Tally {
        let mut total = 0usize;
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut pending = 0usize;
        let mut i = 0usize;
        while i < self.machines.len() {
            if let Some(m) = self.machines.get(i) {
                let mut k = 0usize;
                while k < m.slots.len() {
                    if let Some(s) = m.slots.get(k) {
                        total += 1;
                        match s.state {
                            SlotState::Armed => pending += 1,
                            SlotState::Passed => passed += 1,
                            SlotState::Failed => failed += 1,
                        }
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        Tally {
            total,
            passed,
            failed,
            pending,
            machines: self.machines.len(),
            gate_open: self.gate,
        }
    }
}

/// 诊断渲染：一行说清「测了几台、几个槽、几过几挂几待测」。
pub fn render(g: &T1Gate) -> String {
    let t = g.tally();
    format!(
        "t1 machines={} slots={} pass={} fail={} pending={} gate={} sup={}",
        t.machines,
        t.total,
        t.passed,
        t.failed,
        t.pending,
        t.gate_open,
        g.supplements()
    )
}

// ---------------------------------------------------------------------------
// 基准登记表
// ---------------------------------------------------------------------------

/// 两台 T1 真机（型号串按真机口径取，不造「未知机型」）。
pub const T1_MACHINE_IDS: [&str; 2] = ["hp-lj-1020", "epson-l3153"];

/// 每台机器的三个基座判据槽位。
pub const T1_SLOT_NAMES: [&str; 3] = ["attach-enumerate", "pdl-submit", "status-poll"];

/// 构造两台机器 × 三槽的基座登记表（未记录任何结论 → 全 pending）。
pub fn baseline() -> Result<T1Gate, T1Error> {
    let mut g = T1Gate::new();
    let mut i = 0usize;
    while i < T1_MACHINE_IDS.len() {
        if let Some(id) = T1_MACHINE_IDS.get(i) {
            let mi = g.register_machine(id)?;
            let mut k = 0usize;
            while k < T1_SLOT_NAMES.len() {
                if let Some(n) = T1_SLOT_NAMES.get(k) {
                    g.declare(mi, n, Tier::T1)?;
                }
                k += 1;
            }
        }
        i += 1;
    }
    Ok(g)
}

// ---------------------------------------------------------------------------
// 注入核：九类坏登记必须被显式拒绝
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    // 1 空机器名
    if T1Gate::new().register_machine("").err() == Some(T1Error::Empty) {
        caught += 1;
    }
    // 2 重复机器
    let mut gm = T1Gate::new();
    let a1 = gm.register_machine("a");
    let dup_machine = if a1.is_ok() {
        gm.register_machine("a").err()
    } else {
        None
    };
    if dup_machine == Some(T1Error::DupMachine) {
        caught += 1;
    }
    // 3 第三台机器
    let mut gt = T1Gate::new();
    let b1 = gt.register_machine("a");
    let b2 = gt.register_machine("b");
    let third = if b1.is_ok() && b2.is_ok() {
        gt.register_machine("c").err()
    } else {
        None
    };
    if third == Some(T1Error::Capacity) {
        caught += 1;
    }
    let mut g = match baseline() {
        Ok(x) => x,
        Err(_) => return (caught, total),
    };
    // 4 未登记机器上的槽位
    if g.declare(7, "x", Tier::T1).err() == Some(T1Error::UnknownMachine) {
        caught += 1;
    }
    // 5 同机重复槽位
    if g.declare(0, T1_SLOT_NAMES[0], Tier::T1).err() == Some(T1Error::DupSlot) {
        caught += 1;
    }
    // 6 无证据直通
    if g.force_pass(0, T1_SLOT_NAMES[0]).err() == Some(T1Error::Evidence) {
        caught += 1;
    }
    // 7 证据 CRC 篡改
    let bad_crc = Evidence::raw(2, 256, 0x1234, 0x0000);
    if g.record_pass(0, T1_SLOT_NAMES[0], bad_crc).err() == Some(T1Error::Crc) {
        caught += 1;
    }
    // 8 闸门关闭时登记补测槽
    if g.declare(0, "extra-late", Tier::Gate).err() == Some(T1Error::GateShut) {
        caught += 1;
    }
    // 9 单机开闸
    let mut solo = T1Gate::new();
    let s1 = solo.register_machine("solo");
    let s2 = solo.declare(0, "attach-enumerate", Tier::T1);
    let one = if s1.is_ok() && s2.is_ok() {
        solo.open_gate().err()
    } else {
        None
    };
    if one == Some(T1Error::MachineAbsent) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50134_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50134");

    let mut g = match baseline() {
        Ok(x) => x,
        Err(_) => {
            s.add("50134-J1-基准登记", false, "基座登记构建失败");
            return s;
        }
    };

    // J1-1 两台机器 × 三槽：机器数恰为 2、槽位 6，第三台必拒
    let third = g.register_machine("third-printer");
    let dup = g.register_machine(T1_MACHINE_IDS[0]);
    let ok = g.machine_count() == T1_MACHINES
        && g.tally().total == 6
        && third.err() == Some(T1Error::Capacity)
        && dup.err() == Some(T1Error::DupMachine);
    let det = format!("machines={} slots={}", g.machine_count(), g.tally().total);
    s.add_dyn("50134-J1-两机六槽", ok, &det);

    // J1-2 槽位登记不虚报：新登记槽位一律 Armed，pending 涨、passed 不涨
    let before = match g.audit() {
        Ok(t) => t,
        Err(_) => return s,
    };
    let declared = g.declare(0, "gate-tbd", Tier::T1);
    let after = match g.audit() {
        Ok(t) => t,
        Err(_) => return s,
    };
    let ok = declared.is_ok()
        && after.pending == before.pending + 1
        && after.passed == 0
        && after.failed == 0
        && after.total == before.total + 1
        && after.conserved();
    let det = format!("pending {}=>{} passed={}", before.pending, after.pending, after.passed);
    s.add_dyn("50134-J1-登记不虚报", ok, &det);

    // J1-3 无证据不得通过：force_pass 必拒，且槽位仍是 Armed
    let fp = g.force_pass(0, T1_SLOT_NAMES[0]);
    let t3 = g.audit();
    let slot_state = g.machine(0).and_then(|m| m.slots.get(0)).map(|x| x.state);
    let ok = fp.err() == Some(T1Error::Evidence)
        && match (t3, slot_state) {
            (Ok(t), Some(st)) => st == SlotState::Armed && t.passed == 0,
            _ => false,
        };
    s.add("50134-J1-无证不通过", ok, if ok { "" } else { "口头结论被当成通过" });

    // J1-4 页/字节勾稽：pages 0、witness 0、字节不足 64/页 全部必拒
    let p0 = Evidence::pass(0, 128, 7);
    let w0 = Evidence::pass(2, 256, 0);
    let short = Evidence::pass(4, 4 * MIN_PAGE_BYTES - 1, 9);
    let ok = p0.err() == Some(T1Error::Evidence)
        && w0.err() == Some(T1Error::Evidence)
        && short.err() == Some(T1Error::Evidence);
    s.add("50134-J1-页字节勾稽", ok, if ok { "" } else { "证据勾稽过宽" });

    // J1-5 证据 CRC：合法证据落 Passed，篡改 CRC 只报 Crc 且状态不动
    let good = match Evidence::pass(3, 3 * MIN_PAGE_BYTES, 0xA55A) {
        Ok(e) => e,
        Err(_) => return s,
    };
    let rec_ok = g.record_pass(0, T1_SLOT_NAMES[0], good);
    let mut forged = good;
    forged.bytes += 1; // 字节被改、CRC 不变 → 必被抓
    let rec_bad = g.record_pass(0, T1_SLOT_NAMES[1], forged);
    let ok = rec_ok == Ok(SlotState::Passed) && rec_bad.err() == Some(T1Error::Crc);
    s.add("50134-J1-证据CRC", ok, if ok { "" } else { "证据改写未被识别" });

    // J1-6 三态守恒：任何操作后 passed+failed+pending == total
    let t6 = g.audit();
    let ok = match t6 {
        Ok(t) => t.conserved() && t.passed == 1 && t.failed == 0 && t.pending == t.total - 1,
        Err(_) => false,
    };
    let det = format!("t={:?}", t6.ok());
    s.add_dyn("50134-J1-三态守恒", ok, &det);

    // J1-7 终态锁：已判定（通过或失败）的槽位不得被再测覆盖
    let locked = g.record_pass(0, T1_SLOT_NAMES[0], good);
    let locked_fail = g.record_fail(0, T1_SLOT_NAMES[0], Evidence::raw(0, 0, 1, 0));
    let mut g8 = match baseline() {
        Ok(x) => x,
        Err(_) => return s,
    };
    let fail_ev = match Evidence::fail(0, 0, 0xBEEF) {
        Ok(e) => e,
        Err(_) => return s,
    };
    let failed_rec = g8.record_fail(1, T1_SLOT_NAMES[2], fail_ev);
    let fail_locked = g8.record_pass(1, T1_SLOT_NAMES[2], good);
    let ok = locked.err() == Some(T1Error::Locked)
        && locked_fail.err() == Some(T1Error::Locked)
        && failed_rec == Ok(SlotState::Failed)
        && fail_locked.err() == Some(T1Error::Locked);
    s.add("50134-J1-终态锁", ok, if ok { "" } else { "终态被覆盖" });

    // J1-8 闸门关闭时不得登记补测槽（免测通道）
    let early = g.declare(1, "supplement-early", Tier::Gate);
    let ok = early.err() == Some(T1Error::GateShut) && g.supplements() == 0;
    s.add("50134-J1-闸门前禁补测", ok, if ok { "" } else { "绕过闸门提前补测" });

    // J1-9 开闸条件：单机 / 有一台零基座槽，都不得开闸
    let mut solo = T1Gate::new();
    let r1 = solo.register_machine("solo");
    let r2 = solo.declare(0, "attach-enumerate", Tier::T1);
    let open_solo = solo.open_gate();
    let mut half = T1Gate::new();
    let h1 = half.register_machine("full");
    let h2 = half.declare(0, "attach-enumerate", Tier::T1);
    let h3 = half.register_machine("bare");
    let open_half = half.open_gate();
    let ok = open_solo.err() == Some(T1Error::MachineAbsent)
        && r1.is_ok()
        && r2.is_ok()
        && h1.is_ok()
        && h2.is_ok()
        && h3.is_ok()
        && open_half.err() == Some(T1Error::Pending)
        && !half.gate_open();
    s.add("50134-J1-开闸条件", ok, if ok { "" } else { "不满足条件却开闸" });

    // J1-10 开闸后：可登记补测槽，但不得再登记机器或基座 T1 槽
    let mut gate = match baseline() {
        Ok(x) => x,
        Err(_) => return s,
    };
    let opened = gate.open_gate();
    let late_machine = gate.register_machine("late-printer");
    let late_base = gate.declare(0, "late-base", Tier::T1);
    let supp = gate.declare(0, "supplement-1", Tier::Gate);
    let ok = opened.is_ok()
        && late_machine.err() == Some(T1Error::GateOpen)
        && late_base.err() == Some(T1Error::GateOpen)
        && supp.is_ok()
        && gate.supplements() == 1
        && gate.gate_open();
    s.add("50134-J1-闸门开启后", ok, if ok { "" } else { "闸门后登记面失守" });

    // J1-11 补测槽位：档位为 Gate、状态 Armed、计入 pending 而非 passed
    let supp_state = gate.machine(0).and_then(|m| {
        let mut i = 0usize;
        while i < m.slots.len() {
            if let Some(x) = m.slots.get(i) {
                if x.name == "supplement-1" {
                    return Some(x.state);
                }
            }
            i += 1;
        }
        None
    });
    let t11 = gate.tally();
    let ok = supp_state == Some(SlotState::Armed) && t11.passed == 0 && t11.pending == t11.total;
    let det = format!("state={:?} pending={}/{}", supp_state, t11.pending, t11.total);
    s.add_dyn("50134-J1-补测不预支", ok, &det);

    // J1-12 结算幂等：连续两次 audit 逐字段相等
    let a1 = gate.audit();
    let a2 = gate.audit();
    let ok = a1 == a2 && a1.is_ok();
    s.add("50134-J1-结算幂等", ok, if ok { "" } else { "两次结算不一致" });

    // J1-13 审计抓虚报：手工把 Armed 槽改成 Passed（无证据）必被 CountMismatch 抓
    let mut forged_reg = match baseline() {
        Ok(x) => x,
        Err(_) => return s,
    };
    if let Some(m) = forged_reg.machines.get_mut(0) {
        if let Some(slot) = m.slots.get_mut(0) {
            slot.state = SlotState::Passed;
        }
    }
    let caught = forged_reg.audit();
    let ok = caught.err() == Some(T1Error::CountMismatch);
    let det = format!("audit={:?}", caught.err());
    s.add_dyn("50134-J1-审计抓虚报", ok, &det);

    // J1-14 错误码闭集：15 个码互异且在段内；档位保留值必拒
    let codes = [
        T1Error::Empty,
        T1Error::Capacity,
        T1Error::DupMachine,
        T1Error::DupSlot,
        T1Error::GateShut,
        T1Error::GateOpen,
        T1Error::UnknownMachine,
        T1Error::UnknownSlot,
        T1Error::Tier,
        T1Error::Evidence,
        T1Error::Crc,
        T1Error::Pending,
        T1Error::Locked,
        T1Error::MachineAbsent,
        T1Error::CountMismatch,
    ];
    let mut distinct = true;
    let mut in_seg = true;
    let mut x = 0usize;
    while x < codes.len() {
        let cx = codes[x].code();
        if cx < SEG_BASE + 1 || cx > SEG_BASE + 15 {
            in_seg = false;
        }
        let mut y = x + 1;
        while y < codes.len() {
            if codes[y].code() == cx {
                distinct = false;
            }
            y += 1;
        }
        x += 1;
    }
    let reserved = Tier::from_code(0).err() == Some(T1Error::Tier)
        && Tier::from_code(3).err() == Some(T1Error::Tier);
    let ok = distinct && in_seg && reserved;
    s.add("50134-J1-错误码闭集", ok, if ok { "" } else { "码值重复、越段或档位未拒" });

    // J1-15 注入核：九类坏登记全抓
    let (caught_n, total) = injection_core();
    let ok = caught_n == total;
    let det = format!("注入 {}/{}", caught_n, total);
    s.add_dyn("50134-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50134_checks_green() {
        let set = run_unx50134_checks();
        assert!(set.all_passed(), "UNX-F50134 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50134_baseline_is_all_pending() {
        let g = baseline();
        assert!(g.is_ok());
        if let Ok(x) = g {
            let t = x.tally();
            assert_eq!(t.total, 6);
            assert_eq!(t.passed, 0);
            assert_eq!(t.pending, 6);
            assert!(t.conserved());
        }
    }

    #[test]
    fn unx50134_no_evidence_no_pass() {
        let mut g = baseline();
        assert!(g.is_ok());
        if let Ok(mut x) = g {
            assert_eq!(x.force_pass(0, T1_SLOT_NAMES[0]).err(), Some(T1Error::Evidence));
            assert_eq!(x.tally().passed, 0);
        }
    }

    #[test]
    fn unx50134_two_machines_only() {
        let mut g = T1Gate::new();
        assert!(g.register_machine("a").is_ok());
        assert!(g.register_machine("b").is_ok());
        assert_eq!(g.register_machine("c").err(), Some(T1Error::Capacity));
    }
}
