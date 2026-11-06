//! UNX-F50131 USB 模拟器模型账（QEMU USB printer 模型对接档）（0x43c80 段）
//!
//! 锚点：UNX-F50131-J1 模拟器对接用例可复跑（模拟器档登记）；判据锚定 §AI-63 §5。
//!
//! 为什么模拟器也要记账：真机之前，打印链路只能对着 QEMU 的 USB 打印机模型跑。
//! 模型与用例一旦不留档，「昨天全过、今天全红」就无从归因——可能是模型档案改了，
//! 也可能是用例本身变了。因此本模块把三件事绑在一起：
//!
//! * `ModelSpec` —— 模拟器模型档案（代数/最大包长/端点/quirks/用例数）。quirks
//!   是模型与真实设备的行为差异（缓冲满时 NAK 还是 STALL、复位是否自恢复、
//!   是否支持零长度包），必须显式登记而不是散落在用例里；
//! * `VirtualDevice` —— 模型状态机（Unconfigured→Configured⇄Halted→Disconnected）
//!   与缓冲区账（在途字节 ≤ mps×8），把 SetConfig/BulkOut/BulkIn/ClearHalt/Reset/
//!   Unplug 的语义定死；
//! * `CaseSpec` + `run_case` —— 固定用例（操作序列 + 期望终态 + 期望末状态码），
//!   结果带摘要；复跑要求摘要逐位一致；
//! * `ReplayLedger` —— 用例登记账本（模型+用例号去重、只收通过项、定容），
//!   `replay_all()` 重跑全部登记用例并比对摘要，漂移报 `Replay`。
//!
//! 「可复跑」的判据是三条硬账：同用例两次摘要一致、账本重放与登记摘要一致、
//! 全部用例 pass 且终态/末状态码与档案逐项相符。
//!
//! 错误纪律：段内闭集 `SEG_BASE + n`；缓冲区账用饱和运算，所有入口
//! `Result<_, QemuError>`；内核路径零 panic。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43c80 独占 0x20）。
pub const SEG_BASE: u32 = 0x43c80;

pub const E_MODEL: u16 = 1;
pub const E_DUP: u16 = 2;
pub const E_CAPACITY: u16 = 3;
pub const E_CASE: u16 = 4;
pub const E_STEP: u16 = 5;
pub const E_BUF: u16 = 6;
pub const E_STATE: u16 = 7;
pub const E_REPLAY: u16 = 8;
pub const E_EMPTY: u16 = 9;
pub const E_QUIRK: u16 = 10;
pub const E_EP: u16 = 11;
pub const E_MPS: u16 = 12;

/// 模拟器对接错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QemuError {
    Model,
    Dup,
    Capacity,
    Case,
    Step,
    Buf,
    State,
    Replay,
    Empty,
    Quirk,
    Ep,
    Mps,
}

impl QemuError {
    pub fn code(self) -> u32 {
        let n = match self {
            QemuError::Model => E_MODEL,
            QemuError::Dup => E_DUP,
            QemuError::Capacity => E_CAPACITY,
            QemuError::Case => E_CASE,
            QemuError::Step => E_STEP,
            QemuError::Buf => E_BUF,
            QemuError::State => E_STATE,
            QemuError::Replay => E_REPLAY,
            QemuError::Empty => E_EMPTY,
            QemuError::Quirk => E_QUIRK,
            QemuError::Ep => E_EP,
            QemuError::Mps => E_MPS,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            QemuError::Model => "model spec invalid or unknown id",
            QemuError::Dup => "model or case registered twice",
            QemuError::Capacity => "registry/ledger capacity exhausted",
            QemuError::Case => "case spec invalid or failed its expectations",
            QemuError::Step => "case op count above the step budget",
            QemuError::Buf => "device buffer overflow without a quirk",
            QemuError::State => "operation illegal in the current device state",
            QemuError::Replay => "replay digest drifted from the recorded one",
            QemuError::Empty => "empty op list or empty ledger",
            QemuError::Quirk => "unknown quirk bit set",
            QemuError::Ep => "endpoint address invalid for its direction",
            QemuError::Mps => "max packet size not a power of two in 8..=1024",
        }
    }
}

// ---------------------------------------------------------------------------
// 模型档案
// ---------------------------------------------------------------------------

pub const Q_NAK_WHEN_FULL: u8 = 1;
pub const Q_STALL_ON_OVERFLOW: u8 = 2;
pub const Q_SELF_RESET: u8 = 4;
pub const Q_NO_ZLP: u8 = 8;
pub const QUIRK_MASK: u8 = Q_NAK_WHEN_FULL | Q_STALL_ON_OVERFLOW | Q_SELF_RESET | Q_NO_ZLP;

/// 模拟器模型档案。`quirks` 是模型与真实设备的差异位，必须逐位登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelSpec {
    pub id: u16,
    pub name: &'static str,
    pub usb_gen: u8,
    pub mps: u16,
    pub ep_out: u8,
    pub ep_in: u8,
    pub quirks: u8,
    pub expected_cases: u8,
}

impl ModelSpec {
    /// 档案自检：代数 2/3、包长 2 的幂、端点方向位正确、quirk 位在闭集内。
    pub fn validate(&self) -> Result<(), QemuError> {
        if self.id == 0 || self.name.is_empty() {
            return Err(QemuError::Model);
        }
        if self.usb_gen != 2 && self.usb_gen != 3 {
            return Err(QemuError::Model);
        }
        let mps = self.mps as u32;
        if mps < 8 || mps > 1024 || !mps.is_power_of_two() {
            return Err(QemuError::Mps);
        }
        if self.ep_out == 0 || self.ep_out & 0x80 != 0 || self.ep_out & 0x70 != 0 {
            return Err(QemuError::Ep);
        }
        if self.ep_in & 0x80 == 0 || self.ep_in & 0x70 != 0 || self.ep_in == self.ep_out {
            return Err(QemuError::Ep);
        }
        if self.quirks & !QUIRK_MASK != 0 {
            return Err(QemuError::Quirk);
        }
        if self.expected_cases == 0 || self.expected_cases > 8 {
            return Err(QemuError::Model);
        }
        Ok(())
    }

    /// 设备侧缓冲区：按最大包长 × 8 定容（一个微帧内的突发量，够触发背压）。
    pub fn buffer_cap(&self) -> u32 {
        (self.mps as u32).saturating_mul(8)
    }

    pub fn quirk_label(&self) -> String {
        let mut out = String::new();
        if self.quirks & Q_NAK_WHEN_FULL != 0 {
            out.push_str("nak-full ");
        }
        if self.quirks & Q_STALL_ON_OVERFLOW != 0 {
            out.push_str("stall-overflow ");
        }
        if self.quirks & Q_SELF_RESET != 0 {
            out.push_str("self-reset ");
        }
        if self.quirks & Q_NO_ZLP != 0 {
            out.push_str("no-zlp");
        }
        if out.is_empty() {
            out.push_str("none");
        }
        out
    }
}

/// 三个模拟器档。id/名称是本仓库的登记键，不冒充任何厂商标识。
pub const MODELS: [ModelSpec; 3] = [
    ModelSpec {
        id: 1,
        name: "qemu-printer-usb2",
        usb_gen: 2,
        mps: 64,
        ep_out: 0x01,
        ep_in: 0x81,
        quirks: Q_NAK_WHEN_FULL,
        expected_cases: 2,
    },
    ModelSpec {
        id: 2,
        name: "qemu-printer-hs",
        usb_gen: 2,
        mps: 512,
        ep_out: 0x01,
        ep_in: 0x81,
        quirks: Q_STALL_ON_OVERFLOW | Q_NO_ZLP,
        expected_cases: 2,
    },
    ModelSpec {
        id: 3,
        name: "qemu-stub-usb3",
        usb_gen: 3,
        mps: 1024,
        ep_out: 0x01,
        ep_in: 0x81,
        quirks: Q_SELF_RESET,
        expected_cases: 2,
    },
];

/// 按 id 查模型。未知 id 报 `Model`，不回退默认档（回退会让对表失去意义）。
pub fn model_of(id: u16) -> Result<ModelSpec, QemuError> {
    let mut i = 0usize;
    while i < MODELS.len() {
        if let Some(m) = MODELS.get(i) {
            if m.id == id {
                return Ok(*m);
            }
        }
        i += 1;
    }
    Err(QemuError::Model)
}

/// 模型注册表：定容、id/名称双键去重、可摘要。
pub struct ModelRegistry {
    entries: Vec<ModelSpec>,
    cap: usize,
}

impl ModelRegistry {
    pub fn new(cap: usize) -> Result<ModelRegistry, QemuError> {
        if cap == 0 || cap > 8 {
            return Err(QemuError::Capacity);
        }
        Ok(ModelRegistry {
            entries: Vec::with_capacity(cap),
            cap,
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&ModelSpec> {
        self.entries.get(i)
    }

    /// 登记：先校验档案，再查 id/名称重复，最后查容量。
    pub fn register(&mut self, spec: ModelSpec) -> Result<usize, QemuError> {
        spec.validate()?;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == spec.id || e.name == spec.name {
                    return Err(QemuError::Dup);
                }
            }
            i += 1;
        }
        if self.entries.len() >= self.cap {
            return Err(QemuError::Capacity);
        }
        self.entries.push(spec);
        Ok(self.entries.len() - 1)
    }

    /// 注册表摘要（FNV-1a 64，按 id/代数/包长/端点/quirks 固定字段序）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (self.entries.len() as u64);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(m) = self.entries.get(i) {
                h = fnv_u32(h, m.id as u32);
                h = fnv_u8(h, m.usb_gen);
                h = fnv_u32(h, m.mps as u32);
                h = fnv_u8(h, m.ep_out);
                h = fnv_u8(h, m.ep_in);
                h = fnv_u8(h, m.quirks);
                h = fnv_u8(h, m.expected_cases);
            }
            i += 1;
        }
        h
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fnv_u8(x, ((v >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

// ---------------------------------------------------------------------------
// 设备状态机
// ---------------------------------------------------------------------------

/// 设备四态。Halted 是 Configured 的故障子态：端点停住但配置仍在。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevState {
    Unconfigured,
    Configured,
    Halted,
    Disconnected,
}

impl DevState {
    pub fn wire(self) -> u8 {
        match self {
            DevState::Unconfigured => 0,
            DevState::Configured => 1,
            DevState::Halted => 2,
            DevState::Disconnected => 3,
        }
    }

    pub fn from_wire(c: u8) -> Result<DevState, QemuError> {
        match c {
            0 => Ok(DevState::Unconfigured),
            1 => Ok(DevState::Configured),
            2 => Ok(DevState::Halted),
            3 => Ok(DevState::Disconnected),
            _ => Err(QemuError::State),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DevState::Unconfigured => "unconfigured",
            DevState::Configured => "configured",
            DevState::Halted => "halted",
            DevState::Disconnected => "disconnected",
        }
    }
}

/// 状态码（用例期望值口径）：0 OK / 1 NAK / 2 STALL / 3 REJECT。
pub const STATUS_OK: u16 = 0;
pub const STATUS_NAK: u16 = 1;
pub const STATUS_STALL: u16 = 2;

/// 模型操作。长度参数只对 BulkOut/BulkIn 有意义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    GetDescriptor,
    SetConfig,
    BulkOut(u32),
    BulkIn(u32),
    ClearHaltOut,
    ClearHaltIn,
    Reset,
    Unplug,
}

impl Op {
    pub fn code(self) -> u8 {
        match self {
            Op::GetDescriptor => 1,
            Op::SetConfig => 2,
            Op::BulkOut(_) => 3,
            Op::BulkIn(_) => 4,
            Op::ClearHaltOut => 5,
            Op::ClearHaltIn => 6,
            Op::Reset => 7,
            Op::Unplug => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Op::GetDescriptor => "get-descriptor",
            Op::SetConfig => "set-config",
            Op::BulkOut(_) => "bulk-out",
            Op::BulkIn(_) => "bulk-in",
            Op::ClearHaltOut => "clear-halt-out",
            Op::ClearHaltIn => "clear-halt-in",
            Op::Reset => "reset",
            Op::Unplug => "unplug",
        }
    }

    pub fn bytes(self) -> u32 {
        match self {
            Op::BulkOut(n) | Op::BulkIn(n) => n,
            _ => 0,
        }
    }
}

/// 一次操作的结果。status 与 moved 是账本口径，state 是操作后的状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpOutcome {
    pub status: u16,
    pub moved: u32,
    pub state: DevState,
    pub halted: bool,
}

/// 虚拟设备：模型档案 + 状态 + 缓冲账 + 双向 halt 位。
pub struct VirtualDevice {
    spec: ModelSpec,
    state: DevState,
    pending: u32,
    cap: u32,
    halted_out: bool,
    halted_in: bool,
    ops_applied: u32,
}

impl VirtualDevice {
    /// 构造即校验档案；非法档案不允许变成「一台随便的模拟器」。
    pub fn new(spec: ModelSpec) -> Result<VirtualDevice, QemuError> {
        spec.validate()?;
        Ok(VirtualDevice {
            spec,
            state: DevState::Unconfigured,
            pending: 0,
            cap: spec.buffer_cap(),
            halted_out: false,
            halted_in: false,
            ops_applied: 0,
        })
    }

    pub fn state(&self) -> DevState {
        self.state
    }

    pub fn pending(&self) -> u32 {
        self.pending
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }

    pub fn spec(&self) -> ModelSpec {
        self.spec
    }

    pub fn ops_applied(&self) -> u32 {
        self.ops_applied
    }

    fn outcome(&self, status: u16, moved: u32) -> OpOutcome {
        OpOutcome {
            status,
            moved,
            state: self.state,
            halted: self.halted_out || self.halted_in,
        }
    }

    /// 施加一个操作。拔出后任何操作都是 `State`（而不是静默成功）。
    /// 缓冲满时按 quirks 分派：NAK（背压，数据可重发）/ STALL（端点停）/
    /// 无 quirk → `Buf`（模型不支持该行为，属于用例错误）。
    pub fn apply(&mut self, op: Op) -> Result<OpOutcome, QemuError> {
        if self.state == DevState::Disconnected {
            return Err(QemuError::State);
        }
        self.ops_applied = self.ops_applied.saturating_add(1);
        match op {
            Op::Unplug => {
                self.state = DevState::Disconnected;
                self.pending = 0;
                self.halted_out = false;
                self.halted_in = false;
                Ok(self.outcome(STATUS_OK, 0))
            }
            Op::Reset => {
                self.pending = 0;
                self.halted_out = false;
                self.halted_in = false;
                // 自复位 quirk：复位后自动回到 Configured，无需主机重新 SetConfig。
                self.state = if self.spec.quirks & Q_SELF_RESET != 0 {
                    DevState::Configured
                } else {
                    DevState::Unconfigured
                };
                Ok(self.outcome(STATUS_OK, 0))
            }
            // 取描述符不改变配置状态（枚举阶段先读设备/配置描述符，再 SetConfig）。
            Op::GetDescriptor => Ok(self.outcome(STATUS_OK, 0)),
            // 只有未配置态才能配置；重复 SetConfig 是主机侧错误，显式拒绝。
            Op::SetConfig => {
                if self.state != DevState::Unconfigured {
                    return Err(QemuError::State);
                }
                self.state = DevState::Configured;
                Ok(self.outcome(STATUS_OK, 0))
            }
            Op::ClearHaltOut => {
                if self.state == DevState::Unconfigured {
                    return Err(QemuError::State);
                }
                // 幂等：未挂起时清 halt 也返回 OK（主机复位流程会无脑清一遍）。
                self.halted_out = false;
                if !self.halted_in {
                    self.state = DevState::Configured;
                }
                Ok(self.outcome(STATUS_OK, 0))
            }
            Op::ClearHaltIn => {
                if self.state == DevState::Unconfigured {
                    return Err(QemuError::State);
                }
                self.halted_in = false;
                if !self.halted_out {
                    self.state = DevState::Configured;
                }
                Ok(self.outcome(STATUS_OK, 0))
            }
            Op::BulkOut(n) => {
                if self.state == DevState::Unconfigured {
                    return Err(QemuError::State);
                }
                if n == 0 {
                    return Err(QemuError::Buf);
                }
                if self.halted_out {
                    return Ok(self.outcome(STATUS_STALL, 0));
                }
                if self.pending.saturating_add(n) > self.cap {
                    if self.spec.quirks & Q_NAK_WHEN_FULL != 0 {
                        return Ok(self.outcome(STATUS_NAK, 0));
                    }
                    if self.spec.quirks & Q_STALL_ON_OVERFLOW != 0 {
                        self.halted_out = true;
                        self.state = DevState::Halted;
                        return Ok(self.outcome(STATUS_STALL, 0));
                    }
                    return Err(QemuError::Buf);
                }
                self.pending = self.pending.saturating_add(n);
                Ok(self.outcome(STATUS_OK, n))
            }
            Op::BulkIn(n) => {
                if self.state == DevState::Unconfigured {
                    return Err(QemuError::State);
                }
                if n == 0 {
                    return Err(QemuError::Buf);
                }
                if self.halted_in {
                    return Ok(self.outcome(STATUS_STALL, 0));
                }
                if self.pending == 0 {
                    return Ok(self.outcome(STATUS_NAK, 0));
                }
                let take = if n < self.pending { n } else { self.pending };
                self.pending -= take;
                Ok(self.outcome(STATUS_OK, take))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 对接用例
// ---------------------------------------------------------------------------

/// 用例档案：固定操作序列 + 期望终态 + 期望末状态码。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    pub id: u16,
    pub model: u16,
    pub name: &'static str,
    pub ops: &'static [Op],
    pub expect_state: DevState,
    pub expect_last: u16,
}

const CASE_PRINT_BASIC: [Op; 5] = [
    Op::GetDescriptor,
    Op::SetConfig,
    Op::BulkOut(64),
    Op::BulkIn(64),
    Op::Unplug,
];

const CASE_HALT_RECOVER: [Op; 5] = [
    Op::GetDescriptor,
    Op::SetConfig,
    Op::BulkOut(512),
    Op::BulkOut(64),
    Op::Unplug,
];

const CASE_OVERFLOW_STALL: [Op; 8] = [
    Op::GetDescriptor,
    Op::SetConfig,
    Op::BulkOut(4096),
    Op::BulkOut(64),
    Op::ClearHaltOut,
    Op::BulkIn(4096),
    Op::BulkOut(8),
    Op::Unplug,
];

const CASE_NAK_READ: [Op; 4] = [Op::GetDescriptor, Op::SetConfig, Op::BulkIn(64), Op::Unplug];

const CASE_SELF_RESET: [Op; 5] = [
    Op::GetDescriptor,
    Op::SetConfig,
    Op::Reset,
    Op::BulkOut(1024),
    Op::Unplug,
];

const CASE_UNPLUG_BARE: [Op; 3] = [Op::GetDescriptor, Op::SetConfig, Op::Unplug];

/// 六条用例（每模型两条），期望值与 `VirtualDevice` 的语义一一对应。
pub const CASES: [CaseSpec; 6] = [
    CaseSpec {
        id: 10,
        model: 1,
        name: "print-basic",
        ops: &CASE_PRINT_BASIC,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    },
    CaseSpec {
        id: 11,
        model: 1,
        name: "nak-backpressure",
        ops: &CASE_HALT_RECOVER,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    },
    CaseSpec {
        id: 20,
        model: 2,
        name: "overflow-stall-recover",
        ops: &CASE_OVERFLOW_STALL,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    },
    CaseSpec {
        id: 21,
        model: 2,
        name: "read-nak",
        ops: &CASE_NAK_READ,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    },
    CaseSpec {
        id: 30,
        model: 3,
        name: "self-reset",
        ops: &CASE_SELF_RESET,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    },
    CaseSpec {
        id: 31,
        model: 3,
        name: "unplug-bare",
        ops: &CASE_UNPLUG_BARE,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    },
];

/// 按用例号查档案。
pub fn case_of(id: u16) -> Result<CaseSpec, QemuError> {
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            if c.id == id {
                return Ok(*c);
            }
        }
        i += 1;
    }
    Err(QemuError::Case)
}

/// 一步用例的执行结果（供逐案对表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseResult {
    pub case_id: u16,
    pub model: u16,
    pub steps: u16,
    pub moved: u32,
    pub last_status: u16,
    pub final_state: DevState,
    pub pass: bool,
    pub digest: u64,
}

/// 执行一条用例。操作序列中途任何 `Err` 都原样上抛——那说明用例与模型档案
/// 不匹配，属于对接错误，不允许「吞掉错误继续跑」。
pub fn run_case(case: &CaseSpec, spec: &ModelSpec) -> Result<CaseResult, QemuError> {
    if case.id == 0 || case.name.is_empty() {
        return Err(QemuError::Case);
    }
    if case.ops.is_empty() {
        return Err(QemuError::Empty);
    }
    if case.model != spec.id {
        return Err(QemuError::Model);
    }
    if case.ops.len() > 64 {
        return Err(QemuError::Step);
    }
    let mut dev = VirtualDevice::new(*spec)?;
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325 ^ (case.id as u64);
    let mut moved: u32 = 0;
    let mut last: u16 = STATUS_OK;
    let mut steps: u16 = 0;
    let mut i = 0usize;
    while i < case.ops.len() {
        let op = match case.ops.get(i) {
            Some(o) => *o,
            None => break,
        };
        let out = dev.apply(op)?;
        moved = moved.saturating_add(out.moved);
        last = out.status;
        digest = fnv_u8(digest, op.code());
        digest = fnv_u32(digest, out.status as u32);
        digest = fnv_u32(digest, out.moved);
        digest = fnv_u8(digest, out.state.wire());
        digest = fnv_u8(digest, if out.halted { 1 } else { 0 });
        steps = steps.saturating_add(1);
        i += 1;
    }
    let pass = dev.state() == case.expect_state && last == case.expect_last;
    Ok(CaseResult {
        case_id: case.id,
        model: case.model,
        steps,
        moved,
        last_status: last,
        final_state: dev.state(),
        pass,
        digest,
    })
}

// ---------------------------------------------------------------------------
// 回放账本
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayRecord {
    pub model: u16,
    pub case_id: u16,
    pub digest: u64,
    pub pass: bool,
}

/// 回放账本：只登记通过的用例，模型+用例号去重，定容，可重放。
/// 登记失败项等于把红用例洗成绿，必须显式拒绝。
pub struct ReplayLedger {
    recs: Vec<ReplayRecord>,
    cap: usize,
    rejected: u16,
}

impl ReplayLedger {
    pub fn new(cap: usize) -> Result<ReplayLedger, QemuError> {
        if cap == 0 || cap > 16 {
            return Err(QemuError::Capacity);
        }
        Ok(ReplayLedger {
            recs: Vec::with_capacity(cap),
            cap,
            rejected: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn rejected(&self) -> u16 {
        self.rejected
    }

    pub fn get(&self, i: usize) -> Option<&ReplayRecord> {
        self.recs.get(i)
    }

    pub fn all_pass(&self) -> bool {
        if self.recs.is_empty() {
            return false;
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            match self.recs.get(i) {
                Some(r) => {
                    if !r.pass {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    pub fn register(&mut self, rec: ReplayRecord) -> Result<usize, QemuError> {
        if !rec.pass {
            self.rejected = self.rejected.saturating_add(1);
            return Err(QemuError::Case);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(e) = self.recs.get(i) {
                if e.model == rec.model && e.case_id == rec.case_id {
                    return Err(QemuError::Dup);
                }
            }
            i += 1;
        }
        if self.recs.len() >= self.cap {
            return Err(QemuError::Capacity);
        }
        self.recs.push(rec);
        Ok(self.recs.len() - 1)
    }

    /// 账本摘要：按登记序混入 model/case/digest。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (self.recs.len() as u64);
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                h = fnv_u32(h, r.model as u32);
                h = fnv_u32(h, r.case_id as u32);
                h = fnv_u32(h, (r.digest & 0xffff_ffff) as u32);
                h = fnv_u32(h, (r.digest >> 32) as u32);
            }
            i += 1;
        }
        h
    }

    /// 重放全部登记用例：重新执行并比对摘要，任何漂移即 `Replay`。
    /// 返回重放后的合并摘要（应与 `digest()` 相等）。
    pub fn replay_all(&self) -> Result<u64, QemuError> {
        if self.recs.is_empty() {
            return Err(QemuError::Empty);
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (self.recs.len() as u64);
        let mut i = 0usize;
        while i < self.recs.len() {
            let rec = match self.recs.get(i) {
                Some(r) => *r,
                None => return Err(QemuError::Empty),
            };
            let case = case_of(rec.case_id)?;
            let spec = model_of(rec.model)?;
            let again = run_case(&case, &spec)?;
            if again.digest != rec.digest || !again.pass {
                return Err(QemuError::Replay);
            }
            h = fnv_u32(h, again.model as u32);
            h = fnv_u32(h, again.case_id as u32);
            h = fnv_u32(h, (again.digest & 0xffff_ffff) as u32);
            h = fnv_u32(h, (again.digest >> 32) as u32);
            i += 1;
        }
        Ok(h)
    }
}

/// 摘要比对（登记值 vs 重跑值）。分开暴露是为了让「漂移」可被单独注入验证。
pub fn replay_compare(recorded: u64, actual: u64) -> Result<(), QemuError> {
    if recorded == actual {
        Ok(())
    } else {
        Err(QemuError::Replay)
    }
}

/// 渲染一行用例结果。
pub fn render(r: &CaseResult) -> String {
    format!(
        "case {} model={} steps={} moved={} last={} state={} pass={}",
        r.case_id,
        r.model,
        r.steps,
        r.moved,
        r.last_status,
        r.final_state.label(),
        r.pass
    )
}

// ---------------------------------------------------------------------------
// 注入核：十类坏样本逐类断言具体码
// ---------------------------------------------------------------------------

const LONG_OPS: [Op; 65] = [Op::GetDescriptor; 65];

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    let m1 = match model_of(1) {
        Ok(m) => m,
        Err(_) => return (caught, total),
    };
    let m3 = match model_of(3) {
        Ok(m) => m,
        Err(_) => return (caught, total),
    };

    // 1 未知模型号
    if model_of(99).err() == Some(QemuError::Model) {
        caught += 1;
    }
    // 2 档案字段非法：包长非 2 幂 / 端点方向错 / 未知 quirk 位
    let mut bad_mps = m1;
    bad_mps.mps = 300;
    let mut bad_ep = m1;
    bad_ep.ep_in = 0x01;
    let mut bad_quirk = m1;
    bad_quirk.quirks = 0x80;
    if bad_mps.validate().err() == Some(QemuError::Mps)
        && bad_ep.validate().err() == Some(QemuError::Ep)
        && bad_quirk.validate().err() == Some(QemuError::Quirk)
    {
        caught += 1;
    }
    // 3 重复登记（id 相同）
    let mut reg = match ModelRegistry::new(4) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    let r1 = reg.register(m1);
    let r2 = reg.register(m1);
    if r1 == Ok(0) && r2.err() == Some(QemuError::Dup) {
        caught += 1;
    }
    // 4 注册表容量
    let mut reg1 = match ModelRegistry::new(1) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    let a = reg1.register(m1);
    let b = reg1.register(m3);
    if a.is_ok()
        && b.err() == Some(QemuError::Capacity)
        && ModelRegistry::new(0).err() == Some(QemuError::Capacity)
    {
        caught += 1;
    }
    // 5 用例号非法 / 操作列表为空 / 步数超预算
    let bad_id = CaseSpec {
        id: 0,
        model: 1,
        name: "x",
        ops: &CASE_UNPLUG_BARE,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    };
    let empty_ops = CaseSpec {
        id: 99,
        model: 1,
        name: "empty",
        ops: &[],
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    };
    let long_ops = CaseSpec {
        id: 98,
        model: 1,
        name: "long",
        ops: &LONG_OPS,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    };
    if run_case(&bad_id, &m1).err() == Some(QemuError::Case)
        && run_case(&empty_ops, &m1).err() == Some(QemuError::Empty)
        && run_case(&long_ops, &m1).err() == Some(QemuError::Step)
    {
        caught += 1;
    }
    // 6 无溢出 quirk 的模型写超容量 → Buf
    let over = CaseSpec {
        id: 97,
        model: 3,
        name: "over",
        ops: &[Op::GetDescriptor, Op::SetConfig, Op::BulkOut(8193)],
        expect_state: DevState::Configured,
        expect_last: STATUS_OK,
    };
    if run_case(&over, &m3).err() == Some(QemuError::Buf) {
        caught += 1;
    }
    // 7 拔出后再操作 → State
    let mut dev = match VirtualDevice::new(m1) {
        Ok(d) => d,
        Err(_) => return (caught, total),
    };
    let p1 = dev.apply(Op::GetDescriptor);
    let p2 = dev.apply(Op::Unplug);
    let p3 = dev.apply(Op::BulkOut(8));
    if p1.is_ok() && p2.is_ok() && p3.err() == Some(QemuError::State) {
        caught += 1;
    }
    // 8 重放摘要漂移
    if replay_compare(0x1234, 0x1235).err() == Some(QemuError::Replay)
        && replay_compare(0x1234, 0x1234).is_ok()
    {
        caught += 1;
    }
    // 9 红用例不得登记（wash 成绿）
    let mut ledger = match ReplayLedger::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let red = ReplayRecord {
        model: 1,
        case_id: 10,
        digest: 1,
        pass: false,
    };
    let green = ReplayRecord {
        model: 1,
        case_id: 10,
        digest: 1,
        pass: true,
    };
    if ledger.register(red).err() == Some(QemuError::Case)
        && ledger.rejected() == 1
        && ledger.register(green).is_ok()
        && ledger.register(green).err() == Some(QemuError::Dup)
    {
        caught += 1;
    }
    // 10 空账本重放 / 零容量
    let empty_ledger = match ReplayLedger::new(2) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    if empty_ledger.replay_all().err() == Some(QemuError::Empty)
        && ReplayLedger::new(0).err() == Some(QemuError::Capacity)
        && case_of(12345).err() == Some(QemuError::Case)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50131_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50131");

    // J1-1 三模型档案合法、id/名称/端点唯一，代数覆盖 2 与 3
    let mut models_ok = true;
    let mut gens = [false; 4];
    let mut i = 0usize;
    while i < MODELS.len() {
        if let Some(m) = MODELS.get(i) {
            if m.validate().is_err() {
                models_ok = false;
            }
            if let Some(g) = gens.get_mut(m.usb_gen as usize) {
                *g = true;
            }
            let mut j = i + 1;
            while j < MODELS.len() {
                if let Some(n) = MODELS.get(j) {
                    // 登记键是 id 与名称；端点号允许跨模型相同（是每台设备内部的事）。
                    if m.id == n.id || m.name == n.name {
                        models_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    let ok = models_ok && gens.get(2).copied().unwrap_or(false) && gens.get(3).copied().unwrap_or(false);
    s.add("50131-J1-三模型", ok, if ok { "" } else { "模型档案非法或重复" });

    // J1-2 模型查表往返；未知号显式报 Model
    let mut lookup_ok = true;
    let mut i = 0usize;
    while i < MODELS.len() {
        if let Some(m) = MODELS.get(i) {
            if model_of(m.id).ok() != Some(*m) {
                lookup_ok = false;
            }
        }
        i += 1;
    }
    let ok = lookup_ok
        && model_of(0).err() == Some(QemuError::Model)
        && model_of(99).err() == Some(QemuError::Model);
    s.add("50131-J1-模型查表", ok, if ok { "" } else { "查表往返失败" });

    // J1-3 注册表：装 3 模型、重复拒绝、容量拒绝、摘要确定
    let mut reg = match ModelRegistry::new(4) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut reg_ok = true;
    let mut i = 0usize;
    while i < MODELS.len() {
        if let Some(m) = MODELS.get(i) {
            if reg.register(*m).is_err() {
                reg_ok = false;
            }
        }
        i += 1;
    }
    let first = match MODELS.get(0) {
        Some(m) => *m,
        None => return s,
    };
    let dup = reg.register(first);
    let d1 = reg.digest();
    let d2 = reg.digest();
    let ok = reg_ok
        && reg.len() == 3
        && dup.err() == Some(QemuError::Dup)
        && d1 == d2
        && ModelRegistry::new(0).err() == Some(QemuError::Capacity)
        && ModelRegistry::new(9).err() == Some(QemuError::Capacity);
    let det = format!("n={} d={:x}", reg.len(), d1);
    s.add_dyn("50131-J1-模型注册", ok, &det);

    // J1-4 六条用例全部跑通，终态/末状态码与档案逐项相符（J1 用例对表）
    let mut results: [Option<CaseResult>; 6] = [None, None, None, None, None, None];
    let mut all_ok = true;
    let mut i = 0usize;
    while i < CASES.len() {
        let case = match CASES.get(i) {
            Some(c) => *c,
            None => {
                all_ok = false;
                i += 1;
                continue;
            }
        };
        let spec = match model_of(case.model) {
            Ok(m) => m,
            Err(_) => {
                all_ok = false;
                i += 1;
                continue;
            }
        };
        match run_case(&case, &spec) {
            Ok(r) => {
                if !r.pass || r.final_state != case.expect_state || r.last_status != case.expect_last {
                    all_ok = false;
                }
                if let Some(cell) = results.get_mut(i) {
                    *cell = Some(r);
                }
            }
            Err(_) => all_ok = false,
        }
        i += 1;
    }
    let ok = all_ok && results.iter().all(|x| x.is_some());
    let det = format!("cases={}/6", results.iter().filter(|x| x.is_some()).count());
    s.add_dyn("50131-J1-用例对表", ok, &det);

    // J1-5 可复跑：每条用例连跑两次，摘要逐位一致（J1 核心）
    let mut replay_ok = true;
    let mut i = 0usize;
    let mut digests: [u64; 6] = [0; 6];
    while i < CASES.len() {
        if let Some(case) = CASES.get(i) {
            if let Ok(spec) = model_of(case.model) {
                let a = run_case(case, &spec);
                let b = run_case(case, &spec);
                match (a, b) {
                    (Ok(x), Ok(y)) => {
                        if x.digest != y.digest || x.pass != y.pass || x.moved != y.moved {
                            replay_ok = false;
                        }
                        if let Some(cell) = digests.get_mut(i) {
                            *cell = x.digest;
                        }
                    }
                    _ => replay_ok = false,
                }
            } else {
                replay_ok = false;
            }
        }
        i += 1;
    }
    // 六条用例的摘要两两互异：不同操作序列不能给出同一个摘要。
    let mut j = 0usize;
    while j < digests.len() {
        let mut k = j + 1;
        while k < digests.len() {
            if digests.get(j) == digests.get(k) {
                replay_ok = false;
            }
            k += 1;
        }
        j += 1;
    }
    let det = format!("{:x}/{:x}/{:x}", digests[0], digests[1], digests[2]);
    s.add_dyn("50131-J1-可复跑", replay_ok, &det);

    // J1-6 用例落账：六条登记、账本全绿、重放摘要与登记摘要一致
    let mut ledger = match ReplayLedger::new(6) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let mut reg_all = true;
    let mut i = 0usize;
    while i < results.len() {
        match results.get(i).and_then(|x| *x) {
            Some(r) => {
                let rec = ReplayRecord {
                    model: r.model,
                    case_id: r.case_id,
                    digest: r.digest,
                    pass: r.pass,
                };
                if ledger.register(rec).is_err() {
                    reg_all = false;
                }
            }
            None => reg_all = false,
        }
        i += 1;
    }
    let replay_digest = ledger.replay_all();
    let ok = reg_all
        && ledger.len() == 6
        && ledger.all_pass()
        && ledger.rejected() == 0
        && replay_digest.ok() == Some(ledger.digest())
        && re_register_dup(&mut ledger, &results);
    let det = format!("n={} replay={:?}", ledger.len(), replay_digest.is_ok());
    s.add_dyn("50131-J1-登记重放", ok, &det);

    // J1-7 状态机：未配置不可清 halt；拔出后任何操作都被拒；重复配置被拒
    let mut dev = match VirtualDevice::new(first) {
        Ok(d) => d,
        Err(_) => return s,
    };
    let clear_unconf = dev.apply(Op::ClearHaltOut);
    let cfg1 = dev.apply(Op::SetConfig);
    let cfg2 = dev.apply(Op::SetConfig);
    let drain = dev.apply(Op::BulkOut(64));
    let unplug = dev.apply(Op::Unplug);
    let after = dev.apply(Op::BulkIn(8));
    let ok = clear_unconf.err() == Some(QemuError::State)
        && cfg1.is_ok()
        && cfg2.err() == Some(QemuError::State)
        && drain.is_ok()
        && unplug.is_ok()
        && after.err() == Some(QemuError::State);
    s.add("50131-J1-状态机", ok, if ok { "" } else { "状态闸失守" });

    // J1-8 NAK 行为：model1 缓冲满后再写返回 NAK 且账不变；无数据读也 NAK
    let mut nak = match VirtualDevice::new(first) {
        Ok(d) => d,
        Err(_) => return s,
    };
    let _ = nak.apply(Op::GetDescriptor);
    let _ = nak.apply(Op::SetConfig);
    let f1 = nak.apply(Op::BulkOut(512));
    let f2 = nak.apply(Op::BulkOut(64));
    let after_nak = nak.pending();
    let drain = nak.apply(Op::BulkIn(512));
    let in_empty = nak.apply(Op::BulkIn(64));
    let ok = f1 == Ok(OpOutcome {
        status: STATUS_OK,
        moved: 512,
        state: DevState::Configured,
        halted: false,
    }) && f2.map(|o| o.status).ok() == Some(STATUS_NAK)
        && after_nak == 512
        && drain.map(|o| o.moved).ok() == Some(512)
        && in_empty.map(|o| o.status).ok() == Some(STATUS_NAK);
    s.add("50131-J1-NAK语义", ok, if ok { "" } else { "NAK 语义/缓冲账不符" });

    // J1-9 溢出：model2 溢出 STALL 并挂起，清 halt 后恢复；model3 无该 quirk → Buf
    let m2 = match model_of(2) {
        Ok(m) => m,
        Err(_) => return s,
    };
    let mut ov = match VirtualDevice::new(m2) {
        Ok(d) => d,
        Err(_) => return s,
    };
    let _ = ov.apply(Op::GetDescriptor);
    let _ = ov.apply(Op::SetConfig);
    let fill = ov.apply(Op::BulkOut(4096));
    let over = ov.apply(Op::BulkOut(64));
    let halted_state = ov.state();
    let clear = ov.apply(Op::ClearHaltOut);
    let drained = ov.apply(Op::BulkIn(4096));
    let recovered = ov.apply(Op::BulkOut(8));
    let mut m3dev = match VirtualDevice::new(m3_spec()) {
        Ok(d) => d,
        Err(_) => return s,
    };
    let _ = m3dev.apply(Op::GetDescriptor);
    let _ = m3dev.apply(Op::SetConfig);
    let m3_over = m3dev.apply(Op::BulkOut(8193));
    let ok = fill.is_ok()
        && over.map(|o| o.status).ok() == Some(STATUS_STALL)
        && halted_state == DevState::Halted
        && clear.is_ok()
        && drained.map(|o| o.moved).ok() == Some(4096)
        && recovered.is_ok()
        && m3_over.err() == Some(QemuError::Buf);
    let det = format!("halted={} m3={:?}", halted_state.label(), m3_over.err());
    s.add_dyn("50131-J1-溢出语义", ok, &det);

    // J1-10 自复位 quirk：model3 复位后仍 Configured，model1 复位后回 Unconfigured
    let mut d3 = match VirtualDevice::new(m3_spec()) {
        Ok(d) => d,
        Err(_) => return s,
    };
    let _ = d3.apply(Op::GetDescriptor);
    let _ = d3.apply(Op::SetConfig);
    let r3 = d3.apply(Op::Reset);
    let mut d1 = match VirtualDevice::new(first) {
        Ok(d) => d,
        Err(_) => return s,
    };
    let _ = d1.apply(Op::GetDescriptor);
    let _ = d1.apply(Op::SetConfig);
    let r1 = d1.apply(Op::Reset);
    let ok = r3.map(|o| o.state).ok() == Some(DevState::Configured)
        && r1.map(|o| o.state).ok() == Some(DevState::Unconfigured)
        && d3.state() == DevState::Configured
        && d1.state() == DevState::Unconfigured;
    s.add("50131-J1-自复位", ok, if ok { "" } else { "复位语义未按 quirk 分派" });

    // J1-11 用例参数闸：id 0/空名/空 ops/超步数/模型不符
    let bad_id = CaseSpec {
        id: 0,
        model: 1,
        name: "x",
        ops: &CASE_UNPLUG_BARE,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    };
    let bad_name = CaseSpec {
        id: 60,
        model: 1,
        name: "",
        ops: &CASE_UNPLUG_BARE,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    };
    let empty_ops = CaseSpec {
        id: 61,
        model: 1,
        name: "empty",
        ops: &[],
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    };
    let long_ops = CaseSpec {
        id: 62,
        model: 1,
        name: "long",
        ops: &LONG_OPS,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    };
    let wrong_model = CaseSpec {
        id: 63,
        model: 3,
        name: "wrong",
        ops: &CASE_UNPLUG_BARE,
        expect_state: DevState::Disconnected,
        expect_last: STATUS_OK,
    };
    let ok = run_case(&bad_id, &first).err() == Some(QemuError::Case)
        && run_case(&bad_name, &first).err() == Some(QemuError::Case)
        && run_case(&empty_ops, &first).err() == Some(QemuError::Empty)
        && run_case(&long_ops, &first).err() == Some(QemuError::Step)
        && run_case(&wrong_model, &first).err() == Some(QemuError::Model)
        && case_of(0).err() == Some(QemuError::Case);
    s.add("50131-J1-用例闸", ok, if ok { "" } else { "用例参数闸失守" });

    // J1-12 账本闸：红用例拒登记、重复登记拒、容量拒、空账本重放拒
    let mut lg = match ReplayLedger::new(2) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let red = ReplayRecord {
        model: 1,
        case_id: 10,
        digest: 7,
        pass: false,
    };
    let green = ReplayRecord {
        model: 1,
        case_id: 10,
        digest: 7,
        pass: true,
    };
    let green2 = ReplayRecord {
        model: 2,
        case_id: 20,
        digest: 8,
        pass: true,
    };
    let green3 = ReplayRecord {
        model: 3,
        case_id: 30,
        digest: 9,
        pass: true,
    };
    let e1 = lg.register(red);
    let e2 = lg.register(green);
    let e3 = lg.register(green);
    let e4 = lg.register(green2);
    let e5 = lg.register(green3);
    let empty_lg = match ReplayLedger::new(2) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let ok = e1.err() == Some(QemuError::Case)
        && lg.rejected() == 1
        && e2.is_ok()
        && e3.err() == Some(QemuError::Dup)
        && e4.is_ok()
        && e5.err() == Some(QemuError::Capacity)
        && empty_lg.replay_all().err() == Some(QemuError::Empty)
        && !empty_lg.all_pass()
        && ReplayLedger::new(0).err() == Some(QemuError::Capacity);
    s.add("50131-J1-账本闸", ok, if ok { "" } else { "账本闸失守" });

    // J1-13 摘要敏感性：同用例同模型恒定；同模型不同用例互异
    let c10 = case_of(10);
    let c11 = case_of(11);
    let mut digest_ok = false;
    if let (Ok(ca), Ok(cb), Ok(spec)) = (c10, c11, model_of(1)) {
        let a1 = run_case(&ca, &spec);
        let a2 = run_case(&ca, &spec);
        let b1 = run_case(&cb, &spec);
        if let (Ok(a1), Ok(a2), Ok(b1)) = (a1, a2, b1) {
            digest_ok = a1.digest == a2.digest && a1.digest != b1.digest;
        }
    }
    s.add("50131-J1-摘要敏感", digest_ok, if digest_ok { "" } else { "摘要不可区分或漂移" });

    // J1-14 注入核：十类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50131-J1-注入必抓", ok, &det);

    s
}

fn m3_spec() -> ModelSpec {
    match model_of(3) {
        Ok(m) => m,
        Err(_) => ModelSpec {
            id: 0,
            name: "invalid",
            usb_gen: 0,
            mps: 0,
            ep_out: 0,
            ep_in: 0,
            quirks: 0,
            expected_cases: 0,
        },
    }
}

/// 重复登记检查（拆成小函数，避免判据主体过长）。
fn re_register_dup(ledger: &mut ReplayLedger, results: &[Option<CaseResult>; 6]) -> bool {
    let mut found = false;
    let mut i = 0usize;
    while i < results.len() {
        if let Some(r) = results.get(i).and_then(|x| *x) {
            let rec = ReplayRecord {
                model: r.model,
                case_id: r.case_id,
                digest: r.digest,
                pass: r.pass,
            };
            if ledger.register(rec).err() == Some(QemuError::Dup) {
                found = true;
            }
            break;
        }
        i += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50131_checks_green() {
        let set = run_unx50131_checks();
        assert!(set.all_passed(), "UNX-F50131 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50131_all_cases_replay_identically() {
        let mut i = 0usize;
        while i < CASES.len() {
            if let Some(case) = CASES.get(i) {
                let spec = model_of(case.model);
                assert!(spec.is_ok());
                if let Ok(spec) = spec {
                    let a = run_case(case, &spec);
                    let b = run_case(case, &spec);
                    assert!(a.is_ok() && b.is_ok());
                    if let (Ok(a), Ok(b)) = (a, b) {
                        assert!(a.pass);
                        assert_eq!(a.digest, b.digest);
                    }
                }
            }
            i += 1;
        }
    }

    #[test]
    fn unx50131_ledger_replay_matches() {
        let mut ledger = ReplayLedger::new(6);
        assert!(ledger.is_ok());
        if let Ok(mut ledger) = ledger {
            let mut i = 0usize;
            while i < CASES.len() {
                if let Some(case) = CASES.get(i) {
                    if let Ok(spec) = model_of(case.model) {
                        if let Ok(r) = run_case(case, &spec) {
                            let rec = ReplayRecord {
                                model: r.model,
                                case_id: r.case_id,
                                digest: r.digest,
                                pass: r.pass,
                            };
                            assert!(ledger.register(rec).is_ok());
                        }
                    }
                }
                i += 1;
            }
            assert_eq!(ledger.replay_all().ok(), Some(ledger.digest()));
        }
    }

    #[test]
    fn unx50131_error_codes_in_segment() {
        assert_eq!(QemuError::Model.code(), SEG_BASE + 1);
        assert_eq!(QemuError::Mps.code(), SEG_BASE + 12);
    }
}
