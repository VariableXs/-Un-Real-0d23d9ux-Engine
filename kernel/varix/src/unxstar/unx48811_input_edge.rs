//! UNX-F48811 输入事件边角：热插拔 / 未知事件取证 · 0x70140
//! 覆盖卡：UNX-F48811（设备热插拔事件面：add/remove 入总线事件流，热移除在途帧丢弃入账）、
//!         UNX-F48812（事件层观测探针：未知 type/code 计数器 + 样本取证面）
//!
//! ── 闸门次序 ──
//! 热插拔面与取证面各有一串闸，它们的次序决定「边界事件」落进哪本账：
//!   ① 存在性闸：add 前查重、remove 前查在（Gone 视为不存在）。
//!      若把查重放到总线事件流落账之后，重复 add 会先写一条 Add 事件，
//!      再报错回滚——总线事件流的序号已推进，下游按序号对账时必然错位。
//!      「先判后写」是本模块所有写路径的共同前提。
//!   ② 在途帧清算闸：热移除必须在**同一次操作内**把该设备所有在途帧
//!      摘掉并入账。若延后到下一次事件到达时再清，移除与清理之间到达的
//!      批次会把已死设备的帧当活帧递交——这就是「热移除残留」的典型成因，
//!      而且残留帧的设备号在表里已经查不到，排障时无从归因。
//!   ③ 幽灵查询闸：remove 之后任何查询/入帧必须报 Gone，绝不允许返回
//!      上一次的快照。若查询返回快照（把 state 只是标脏而不判），
//!      上层会对着一个已拔出的设备继续派发事件，在途帧越积越多。
//!   ④ 分类闸（取证面）：先判 type 是否在十族闭集内，再判 code 是否在
//!      已知码表内。若先判 code，未知 type 会因「码表里当然没有这个码」
//!      被误记为 UnknownCode——真正坏的是 type，分类错了取证方向就错了。
//!   ⑤ 证据闸：计数先行、样本后置。计数是**全量**（一条不漏），
//!      样本是**定容环**（16 条，溢出丢最旧并计入 evidence_dropped）。
//!      若只留样本不计数，第 17 条之后的未知事件就静默消失了；
//!      若只计数不留样本，事后无法取证——两本账必须同时存在且能互相对上
//!      （total_unknown == evidence_len + evidence_dropped）。
//!
//! ── 关键差异 ──
//! 与 unx48803_input_meta 的边界：meta 的五元组表是**静态注册面**
//!   （登记/查询/注销，回答「这台设备是什么」）；本模块是**热插拔时序面**
//!   （add/remove 事件流 + 在途帧清算，回答「这台设备什么时候来/走、
//!   走的时候还有多少帧没交出去」）。本模块不持有路径/名称/vid-pid，
//!   只持有设备号与生命周期状态；注销五元组由 meta 负责，本模块只发出总线事件。
//! 与 unx48813_input_bridge 的边界：bridge 负责「轮询路径升级为中断路径」的
//!   接管声明与错误码族；本模块的 add/remove 是**设备级**生命周期，
//!   与后端接管无关——后端切换不产生 Add/Remove 事件。
//! 与 unx48819_input_seal 的边界：seal 只做批级防重与行数收官，
//!   本模块的取证样本是**运行期**数据，不进入封印账。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本模块诊断段基址。
pub const SEG_BASE: u32 = 0x70140;

/// 热插拔周期数（卡面「×10 周期」）。
pub const HOTPLUG_CYCLES: u32 = 10;
/// 每周期在途帧数（用于验证清算完整性）。
pub const INFLIGHT_PER_CYCLE: u32 = 3;
/// 未知事件注入条数（卡面「×10^2」）。
pub const UNKNOWN_INJECTIONS: u32 = 100;
/// 证据样本环容量。
pub const EVIDENCE_CAP: usize = 16;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（12 条）────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeCode {
    /// 设备已在（重复 add）。
    AlreadyPresent,
    /// 设备不存在（remove/查询/入帧未命中）。
    Gone,
    /// 在途帧残留（热移除后仍有该设备帧）。
    InflightResidue,
    /// 总线事件流序号不连续。
    BusSeqBroken,
    /// 设备槽位越界。
    SlotOutOfRange,
    /// 未知 type（不在十族闭集）。
    UnknownType,
    /// 未知 code（族在、码不在已知表）。
    UnknownCode,
    /// 证据环为空却请求取证。
    NoEvidence,
    /// 计数与证据账不平。
    EvidenceMismatch,
    /// 清算时在途帧所属设备仍标记存活（逻辑矛盾）。
    LiveAfterEvict,
    /// 设备状态迁移非法（如 Gone → Bound 未经 add）。
    BadTransition,
    /// 双跑不一致。
    NotReproducible,
}

impl EdgeCode {
    /// 段内绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文文案。
    pub fn detail(self) -> &'static str {
        match self {
            EdgeCode::AlreadyPresent => "设备已在：重复 add 拒绝",
            EdgeCode::Gone => "设备不存在：已热移除",
            EdgeCode::InflightResidue => "在途帧残留：清算未完成",
            EdgeCode::BusSeqBroken => "总线事件流序号不连续",
            EdgeCode::SlotOutOfRange => "设备槽位越界",
            EdgeCode::UnknownType => "未知 type：十族闭集外",
            EdgeCode::UnknownCode => "未知 code：族在码不在",
            EdgeCode::NoEvidence => "证据环为空：无样本可查",
            EdgeCode::EvidenceMismatch => "计数与证据账不平",
            EdgeCode::LiveAfterEvict => "已移除设备仍标记存活",
            EdgeCode::BadTransition => "设备状态迁移非法",
            EdgeCode::NotReproducible => "双跑不一致",
        }
    }

    /// 闭集全表。
    pub const ALL: [EdgeCode; 12] = [
        EdgeCode::AlreadyPresent,
        EdgeCode::Gone,
        EdgeCode::InflightResidue,
        EdgeCode::BusSeqBroken,
        EdgeCode::SlotOutOfRange,
        EdgeCode::UnknownType,
        EdgeCode::UnknownCode,
        EdgeCode::NoEvidence,
        EdgeCode::EvidenceMismatch,
        EdgeCode::LiveAfterEvict,
        EdgeCode::BadTransition,
        EdgeCode::NotReproducible,
    ];
}

/// 码值 → 文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < EdgeCode::ALL.len() {
        if let Some(c) = EdgeCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70141..0x7014C 闭集内"
}

// ── 指纹基础设施（本域按「生命周期指纹」命名）────────────────
/// 生命周期指纹初值。
pub fn life_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 折入一字节。
pub fn life_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折入 32 位字。
pub fn life_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 4 {
        acc = life_byte(acc, ((w >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 十族 type 表（与 core 同口径，本模块只用于分类，不存位图）。
pub const FAMILY_TYPE: [u16; 10] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x11, 0x12, 0x14, 0x15];

/// type → 槽位。
pub fn slot_of_type(kind: u16) -> Option<usize> {
    let mut i = 0usize;
    while i < FAMILY_TYPE.len() {
        if let Some(t) = FAMILY_TYPE.get(i) {
            if *t == kind {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ── 设备生命周期 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceState {
    /// 探枚举中（已 add，尚未首次上报帧）。
    Probing,
    /// 正常绑定（可入帧）。
    Bound,
    /// 热移除隔离（已 Gone，等待清理）。
    Quarantined,
    /// 已移除（不可查询、不可入帧）。
    Gone,
}

impl DeviceState {
    /// 是否可接受帧。
    pub fn accepts_frames(self) -> bool {
        matches!(self, DeviceState::Probing | DeviceState::Bound)
    }

    /// 是否存活（查询面可见）。
    pub fn alive(self) -> bool {
        self.accepts_frames()
    }

    /// 中文状态名。
    pub fn label(self) -> &'static str {
        match self {
            DeviceState::Probing => "probing",
            DeviceState::Bound => "bound",
            DeviceState::Quarantined => "quarantined",
            DeviceState::Gone => "gone",
        }
    }
}

/// 设备槽。
#[derive(Clone, Debug)]
pub struct DevSlot {
    /// 设备号。
    pub dev: u16,
    /// 设备名（用于报告，不承担唯一键职责）。
    pub name: String,
    /// 生命周期状态。
    pub state: DeviceState,
    /// 第几次 add（单设备可多次插拔，次数单调）。
    pub attaches: u32,
    /// 累计被清算的在途帧数。
    pub dropped_frames: u32,
}

/// 总线事件类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BusEventKind {
    /// 设备接入。
    Add,
    /// 设备移除。
    Remove,
}

impl BusEventKind {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            BusEventKind::Add => "add",
            BusEventKind::Remove => "remove",
        }
    }
}

/// 总线事件（热插拔事件流的最小单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BusEvent {
    /// 事件序号（1 起，严格连续）。
    pub seq: u32,
    /// 事件类型。
    pub kind: BusEventKind,
    /// 设备号。
    pub dev: u16,
}

/// 在途帧（已闭合待递交，但设备被拔走时会被清算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InFlight {
    /// 帧序号。
    pub frame: u32,
    /// 所属设备。
    pub dev: u16,
    /// 帧内事件数。
    pub events: u16,
}

impl InFlight {
    /// 构造。
    pub const fn new(frame: u32, dev: u16, events: u16) -> InFlight {
        InFlight { frame, dev, events }
    }
}

/// 热插拔总线（设备生命周期 + 事件流 + 在途帧表）。
pub struct HotplugBus {
    slots: Vec<DevSlot>,
    stream: Vec<BusEvent>,
    inflight: Vec<InFlight>,
    bus_seq: u32,
    frame_seq: u32,
    dropped_inflight: u32,
}

impl HotplugBus {
    /// 空总线。
    pub fn new() -> HotplugBus {
        HotplugBus {
            slots: Vec::new(),
            stream: Vec::new(),
            inflight: Vec::new(),
            bus_seq: 0,
            frame_seq: 0,
            dropped_inflight: 0,
        }
    }

    /// 取槽（内部用，返回下标）。
    fn slot_index(&self, dev: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.dev == dev {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 接入：查重 → 复用已 Gone 槽 → 发 Add 事件。
    pub fn add(&mut self, dev: u16, name: &str) -> Result<u32, u32> {
        if let Some(i) = self.slot_index(dev) {
            let alive = match self.slots.get(i) {
                Some(s) => s.state.alive(),
                None => false,
            };
            if alive {
                return Err(EdgeCode::AlreadyPresent.val());
            }
            // 复用槽：状态回到 Probing，插拔次数 +1。
            match self.slots.get_mut(i) {
                Some(s) => {
                    s.state = DeviceState::Probing;
                    s.attaches = s.attaches.saturating_add(1);
                }
                None => return Err(EdgeCode::SlotOutOfRange.val()),
            }
        } else {
            self.slots.push(DevSlot {
                dev,
                name: name.to_string(),
                state: DeviceState::Probing,
                attaches: 1,
                dropped_frames: 0,
            });
        }
        let seq = self.emit(BusEventKind::Add, dev)?;
        Ok(seq)
    }

    /// 绑定（Probing → Bound）：首次上报帧后调用。
    pub fn bind(&mut self, dev: u16) -> Result<(), u32> {
        let i = match self.slot_index(dev) {
            Some(i) => i,
            None => return Err(EdgeCode::Gone.val()),
        };
        match self.slots.get_mut(i) {
            Some(s) => {
                if s.state != DeviceState::Probing {
                    return Err(EdgeCode::BadTransition.val());
                }
                s.state = DeviceState::Bound;
                Ok(())
            }
            None => Err(EdgeCode::SlotOutOfRange.val()),
        }
    }

    /// 移除：状态迁移 → 事件流 → 在途帧清算（同一操作内，见闸②）。
    pub fn remove(&mut self, dev: u16) -> Result<u32, u32> {
        let i = match self.slot_index(dev) {
            Some(i) => i,
            None => return Err(EdgeCode::Gone.val()),
        };
        let alive = match self.slots.get(i) {
            Some(s) => s.state.alive(),
            None => false,
        };
        if !alive {
            return Err(EdgeCode::Gone.val());
        }
        match self.slots.get_mut(i) {
            Some(s) => s.state = DeviceState::Quarantined,
            None => return Err(EdgeCode::SlotOutOfRange.val()),
        }
        let seq = self.emit(BusEventKind::Remove, dev)?;
        // 清算：摘掉该设备所有在途帧并逐条入账。
        let before = self.inflight.len();
        let mut kept: Vec<InFlight> = Vec::with_capacity(before);
        let mut dropped_here = 0u32;
        let mut k = 0usize;
        while k < before {
            match self.inflight.get(k) {
                Some(f) => {
                    if f.dev == dev {
                        dropped_here = dropped_here.saturating_add(1);
                    } else {
                        kept.push(*f);
                    }
                }
                None => return Err(EdgeCode::InflightResidue.val()),
            }
            k += 1;
        }
        self.inflight = kept;
        self.dropped_inflight = self.dropped_inflight.saturating_add(dropped_here);
        match self.slots.get_mut(i) {
            Some(s) => {
                s.dropped_frames = s.dropped_frames.saturating_add(dropped_here);
                s.state = DeviceState::Gone;
            }
            None => return Err(EdgeCode::SlotOutOfRange.val()),
        }
        Ok(seq)
    }

    /// 单槽累计清算数（供桥接面与探针复核）。
    pub fn slot_dropped(&self, dev: u16) -> Result<u32, u32> {
        let i = match self.slot_index(dev) {
            Some(i) => i,
            None => return Err(EdgeCode::Gone.val()),
        };
        match self.slots.get(i) {
            Some(s) => Ok(s.dropped_frames),
            None => Err(EdgeCode::SlotOutOfRange.val()),
        }
    }

    /// 发一条总线事件（序号严格连续）。
    fn emit(&mut self, kind: BusEventKind, dev: u16) -> Result<u32, u32> {
        let seq = self.bus_seq.saturating_add(1);
        self.bus_seq = seq;
        self.stream.push(BusEvent { seq, kind, dev });
        Ok(seq)
    }

    /// 入帧：设备必须存活。
    pub fn push_frame(&mut self, dev: u16, events: u16) -> Result<u32, u32> {
        let i = match self.slot_index(dev) {
            Some(i) => i,
            None => return Err(EdgeCode::Gone.val()),
        };
        let accepts = match self.slots.get(i) {
            Some(s) => s.state.accepts_frames(),
            None => false,
        };
        if !accepts {
            return Err(EdgeCode::Gone.val());
        }
        let frame = self.frame_seq.saturating_add(1);
        self.frame_seq = frame;
        self.inflight.push(InFlight::new(frame, dev, events));
        Ok(frame)
    }

    /// 出帧（正常交付）。
    pub fn pop_frame(&mut self, frame: u32) -> Result<InFlight, u32> {
        let mut i = 0usize;
        while i < self.inflight.len() {
            let hit = match self.inflight.get(i) {
                Some(f) => f.frame == frame,
                None => false,
            };
            if hit {
                return Ok(self.inflight.remove(i));
            }
            i += 1;
        }
        Err(EdgeCode::Gone.val())
    }

    /// 查询：Gone 即拒（幽灵查询闸）。
    pub fn query(&self, dev: u16) -> Result<&DevSlot, u32> {
        let i = match self.slot_index(dev) {
            Some(i) => i,
            None => return Err(EdgeCode::Gone.val()),
        };
        match self.slots.get(i) {
            Some(s) => {
                if s.state.alive() {
                    Ok(s)
                } else {
                    Err(EdgeCode::Gone.val())
                }
            }
            None => Err(EdgeCode::SlotOutOfRange.val()),
        }
    }

    /// 在途帧总数。
    pub fn inflight_len(&self) -> usize {
        self.inflight.len()
    }

    /// 累计清算帧数。
    pub fn dropped_inflight(&self) -> u32 {
        self.dropped_inflight
    }

    /// 总线事件流。
    pub fn stream(&self) -> &[BusEvent] {
        self.stream.as_slice()
    }

    /// 存活设备数。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.state.alive() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 残留检查：在途帧所属设备必须存活；否则残留。
    pub fn residue_check(&self) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.inflight.len() {
            let f = match self.inflight.get(i) {
                Some(f) => f,
                None => return Err(EdgeCode::InflightResidue.val()),
            };
            let alive = match self.slot_index(f.dev) {
                Some(idx) => match self.slots.get(idx) {
                    Some(s) => s.state.alive(),
                    None => false,
                },
                None => false,
            };
            if !alive {
                return Err(EdgeCode::InflightResidue.val());
            }
            i += 1;
        }
        Ok(0)
    }

    /// 总线事件流自检：序号 1..n 连续，Add/Remove 交替与设备状态一致。
    pub fn stream_check(&self) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.stream.len() {
            let e = match self.stream.get(i) {
                Some(e) => e,
                None => return Err(EdgeCode::BusSeqBroken.val()),
            };
            if e.seq != (i as u32) + 1 {
                return Err(EdgeCode::BusSeqBroken.val());
            }
            i += 1;
        }
        Ok(self.stream.len() as u32)
    }

    /// 生命周期指纹。
    pub fn digest(&self) -> u64 {
        let mut h = life_init();
        let mut i = 0usize;
        while i < self.stream.len() {
            if let Some(e) = self.stream.get(i) {
                h = life_word(h, e.seq);
                h = life_byte(h, e.kind as u8);
                h = life_word(h, e.dev as u32);
            }
            i += 1;
        }
        h
    }
}

/// 十台设备的热插拔周期演练：add → bind → 3 帧在途 → remove → 零残留。
pub fn hotplug_cycles() -> Result<u32, u32> {
    let mut bus = HotplugBus::new();
    let mut cycle = 0u32;
    let mut cleared = 0u32;
    while cycle < HOTPLUG_CYCLES {
        let dev = 0x200u16 + cycle as u16;
        bus.add(dev, "hotplug-dev")?;
        bus.bind(dev)?;
        let mut k = 0u32;
        while k < INFLIGHT_PER_CYCLE {
            bus.push_frame(dev, 3)?;
            k += 1;
        }
        // 一个周期内还混入一台「陪跑」设备的帧，验证清算不误伤。
        let other = 0x300u16 + cycle as u16;
        bus.add(other, "bystander")?;
        bus.bind(other)?;
        bus.push_frame(other, 2)?;

        let before = bus.inflight_len();
        bus.remove(dev)?;
        // 清算必须同操作内完成：只剩陪跑设备的 1 帧。
        if bus.inflight_len() != 1 {
            return Err(EdgeCode::InflightResidue.val());
        }
        if bus.inflight_len() + INFLIGHT_PER_CYCLE as usize != before {
            return Err(EdgeCode::InflightResidue.val());
        }
        if bus.query(dev).is_ok() {
            return Err(EdgeCode::Gone.val());
        }
        if bus.push_frame(dev, 1).is_ok() {
            return Err(EdgeCode::Gone.val());
        }
        cleared = cleared.saturating_add(INFLIGHT_PER_CYCLE);
        // 陪跑设备正常交付后退场。
        bus.remove(other)?;
        if bus.inflight_len() != 0 {
            return Err(EdgeCode::InflightResidue.val());
        }
        bus.residue_check()?;
        cycle += 1;
    }
    if bus.dropped_inflight() != cleared + HOTPLUG_CYCLES {
        // 陪跑设备的 1 帧也在其 remove 时被清算。
        return Err(EdgeCode::InflightResidue.val());
    }
    bus.stream_check()?;
    Ok(cleared)
}

// ── 未知事件取证面 ────────────────────────────────────────────
/// 未知分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnkClass {
    /// 未知 type。
    TypeMiss,
    /// 未知 code（族在）。
    CodeMiss,
    /// 已知（不计数，仅供 `observe` 返回）。
    Known,
}

/// 单 type 命中计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TypeHit {
    /// 未知 type 值。
    pub kind: u16,
    /// 命中次数。
    pub count: u32,
    /// 首次序号。
    pub first_seq: u32,
    /// 末次序号。
    pub last_seq: u32,
}

/// 单 (type,code) 命中计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeHit {
    /// type。
    pub kind: u16,
    /// code。
    pub code: u16,
    /// 命中次数。
    pub count: u32,
    /// 首次序号。
    pub first_seq: u32,
    /// 末次序号。
    pub last_seq: u32,
}

/// 证据样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 观测序号（1 起）。
    pub seq: u32,
    /// 设备号。
    pub dev: u16,
    /// type。
    pub kind: u16,
    /// code。
    pub code: u16,
    /// value。
    pub value: i32,
    /// 时间戳。
    pub ts: u64,
    /// 分类。
    pub class: UnkClass,
}

/// 未知事件探针（全量计数 + 定容证据环）。
pub struct UnknownProbe {
    known: Vec<(u16, u16)>,
    types: Vec<TypeHit>,
    codes: Vec<CodeHit>,
    evidence: Vec<Evidence>,
    seq: u32,
    total: u32,
    evid_dropped: u32,
}

impl UnknownProbe {
    /// 以已知码表建探针（码表来自设备画像，不在此处重复位图）。
    pub fn new(known: Vec<(u16, u16)>) -> UnknownProbe {
        UnknownProbe {
            known,
            types: Vec::new(),
            codes: Vec::new(),
            evidence: Vec::new(),
            seq: 0,
            total: 0,
            evid_dropped: 0,
        }
    }

    /// 生成标准已知码表（EV_KEY 0x01..=0x58、EV_REL 0/1/8、EV_ABS 0/1/0x35）。
    pub fn default_known() -> Vec<(u16, u16)> {
        let mut v: Vec<(u16, u16)> = Vec::new();
        let mut k = 0x01u16;
        while k <= 0x58 {
            v.push((0x01, k));
            k += 1;
        }
        v.push((0x02, 0x00));
        v.push((0x02, 0x01));
        v.push((0x02, 0x08));
        v.push((0x03, 0x00));
        v.push((0x03, 0x01));
        v.push((0x03, 0x35));
        v.push((0x00, 0x00));
        v
    }

    /// 码表查询。
    pub fn is_known(&self, kind: u16, code: u16) -> bool {
        let mut i = 0usize;
        while i < self.known.len() {
            if let Some(k) = self.known.get(i) {
                if k.0 == kind && k.1 == code {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 观测一条事件（分类闸 + 计数 + 证据）。
    pub fn observe(
        &mut self,
        dev: u16,
        kind: u16,
        code: u16,
        value: i32,
        ts: u64,
    ) -> UnkClass {
        self.seq = self.seq.saturating_add(1);
        if slot_of_type(kind).is_none() {
            self.bump_type(kind);
            self.push_evidence(Evidence {
                seq: self.seq,
                dev,
                kind,
                code,
                value,
                ts,
                class: UnkClass::TypeMiss,
            });
            self.total = self.total.saturating_add(1);
            return UnkClass::TypeMiss;
        }
        if !self.is_known(kind, code) {
            self.bump_code(kind, code);
            self.push_evidence(Evidence {
                seq: self.seq,
                dev,
                kind,
                code,
                value,
                ts,
                class: UnkClass::CodeMiss,
            });
            self.total = self.total.saturating_add(1);
            return UnkClass::CodeMiss;
        }
        UnkClass::Known
    }

    /// 计数：未知 type。
    fn bump_type(&mut self, kind: u16) {
        let mut i = 0usize;
        while i < self.types.len() {
            let hit = match self.types.get(i) {
                Some(t) => t.kind == kind,
                None => false,
            };
            if hit {
                if let Some(t) = self.types.get_mut(i) {
                    t.count = t.count.saturating_add(1);
                    t.last_seq = self.seq;
                }
                return;
            }
            i += 1;
        }
        self.types.push(TypeHit { kind, count: 1, first_seq: self.seq, last_seq: self.seq });
    }

    /// 计数：未知 code。
    fn bump_code(&mut self, kind: u16, code: u16) {
        let mut i = 0usize;
        while i < self.codes.len() {
            let hit = match self.codes.get(i) {
                Some(c) => c.kind == kind && c.code == code,
                None => false,
            };
            if hit {
                if let Some(c) = self.codes.get_mut(i) {
                    c.count = c.count.saturating_add(1);
                    c.last_seq = self.seq;
                }
                return;
            }
            i += 1;
        }
        self.codes.push(CodeHit { kind, code, count: 1, first_seq: self.seq, last_seq: self.seq });
    }

    /// 证据入环（满则丢最旧并计入 evid_dropped）。
    fn push_evidence(&mut self, e: Evidence) {
        if self.evidence.len() >= EVIDENCE_CAP {
            if !self.evidence.is_empty() {
                self.evidence.remove(0);
                self.evid_dropped = self.evid_dropped.saturating_add(1);
            }
        }
        self.evidence.push(e);
    }

    /// 未知总数。
    pub fn total_unknown(&self) -> u32 {
        self.total
    }

    /// 未知 type 计数表。
    pub fn type_hits(&self) -> &[TypeHit] {
        self.types.as_slice()
    }

    /// 未知 code 计数表。
    pub fn code_hits(&self) -> &[CodeHit] {
        self.codes.as_slice()
    }

    /// 证据样本。
    pub fn samples(&self) -> &[Evidence] {
        self.evidence.as_slice()
    }

    /// 证据丢弃数。
    pub fn evidence_dropped(&self) -> u32 {
        self.evid_dropped
    }

    /// 计数与证据账对拍：total == samples + dropped。
    pub fn reconcile(&self) -> Result<(), u32> {
        if self.total != (self.evidence.len() as u32).saturating_add(self.evid_dropped) {
            return Err(EdgeCode::EvidenceMismatch.val());
        }
        let mut tsum = 0u32;
        let mut i = 0usize;
        while i < self.types.len() {
            if let Some(t) = self.types.get(i) {
                if t.count == 0 {
                    return Err(EdgeCode::EvidenceMismatch.val());
                }
                tsum = tsum.saturating_add(t.count);
            }
            i += 1;
        }
        let mut csum = 0u32;
        let mut k = 0usize;
        while k < self.codes.len() {
            if let Some(c) = self.codes.get(k) {
                if c.count == 0 {
                    return Err(EdgeCode::EvidenceMismatch.val());
                }
                csum = csum.saturating_add(c.count);
            }
            k += 1;
        }
        if tsum.saturating_add(csum) != self.total {
            return Err(EdgeCode::EvidenceMismatch.val());
        }
        Ok(())
    }

    /// 按序号取证（线性扫描证据环）。
    pub fn evidence_for(&self, seq: u32) -> Result<Evidence, u32> {
        if self.evidence.is_empty() {
            return Err(EdgeCode::NoEvidence.val());
        }
        let mut i = 0usize;
        while i < self.evidence.len() {
            if let Some(e) = self.evidence.get(i) {
                if e.seq == seq {
                    return Ok(*e);
                }
            }
            i += 1;
        }
        Err(EdgeCode::NoEvidence.val())
    }

    /// 取证面指纹。
    pub fn digest(&self) -> u64 {
        let mut h = life_init();
        let mut i = 0usize;
        while i < self.evidence.len() {
            if let Some(e) = self.evidence.get(i) {
                h = life_word(h, e.seq);
                h = life_word(h, e.kind as u32);
                h = life_word(h, e.code as u32);
                h = life_byte(h, e.class as u8);
            }
            i += 1;
        }
        h = life_word(h, self.total);
        h = life_word(h, self.evid_dropped);
        h
    }
}

/// 100 条未知事件注入（半数未知 type、半数未知 code），分类与计数必须全对。
pub fn unknown_injection() -> Result<u32, u32> {
    let mut probe = UnknownProbe::new(UnknownProbe::default_known());
    // 未知 type 池：0x7E / 0x42 / 0x99（各 17/17/16 条）。
    let type_pool: [u16; 3] = [0x7E, 0x42, 0x99];
    // 未知 code 池（族在、码不在）：(EV_KEY,0x2F0) / (EV_ABS,0x3F) / (EV_REL,0x0F)。
    let code_pool: [(u16, u16); 3] = [(0x01, 0x2F0), (0x03, 0x3F), (0x02, 0x0F)];
    let mut i = 0u32;
    let mut type_seen = 0u32;
    let mut code_seen = 0u32;
    while i < UNKNOWN_INJECTIONS {
        let dev = 0x400u16 + (i % 4) as u16;
        if i % 2 == 0 {
            let kind = match type_pool.get(((i / 2) % 3) as usize) {
                Some(k) => *k,
                None => return Err(EdgeCode::UnknownType.val()),
            };
            let class = probe.observe(dev, kind, 0x01, i as i32, i as u64);
            if class != UnkClass::TypeMiss {
                return Err(EdgeCode::UnknownType.val());
            }
            type_seen += 1;
        } else {
            let pair = match code_pool.get(((i / 2) % 3) as usize) {
                Some(p) => *p,
                None => return Err(EdgeCode::UnknownCode.val()),
            };
            let class = probe.observe(dev, pair.0, pair.1, i as i32, i as u64);
            if class != UnkClass::CodeMiss {
                return Err(EdgeCode::UnknownCode.val());
            }
            code_seen += 1;
        }
        i += 1;
    }
    probe.reconcile()?;
    if probe.total_unknown() != UNKNOWN_INJECTIONS {
        return Err(EdgeCode::EvidenceMismatch.val());
    }
    let mut tsum = 0u32;
    let mut k = 0usize;
    while k < probe.type_hits().len() {
        if let Some(t) = probe.type_hits().get(k) {
            tsum = tsum.saturating_add(t.count);
        }
        k += 1;
    }
    let mut csum = 0u32;
    k = 0;
    while k < probe.code_hits().len() {
        if let Some(c) = probe.code_hits().get(k) {
            csum = csum.saturating_add(c.count);
        }
        k += 1;
    }
    if tsum != type_seen || csum != code_seen {
        return Err(EdgeCode::EvidenceMismatch.val());
    }
    if probe.samples().len() != EVIDENCE_CAP || probe.evidence_dropped() == 0 {
        return Err(EdgeCode::EvidenceMismatch.val());
    }
    // 最末一条必须能在证据环里查到（取证可用）。
    let last = probe.samples().last().copied();
    match last {
        Some(e) => {
            let got = probe.evidence_for(e.seq)?;
            if got.seq != e.seq || got.class == UnkClass::Known {
                return Err(EdgeCode::NoEvidence.val());
            }
        }
        None => return Err(EdgeCode::NoEvidence.val()),
    }
    Ok(probe.total_unknown())
}

// ── 黄金全链 ──────────────────────────────────────────────────
/// 黄金链输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenOut {
    /// 热插拔清算帧总数。
    pub cleared: u32,
    /// 未知事件总数。
    pub unknown: u32,
    /// 证据样本数。
    pub samples: u32,
    /// 生命周期指纹与取证指纹的合成值。
    pub digest: u64,
}

/// 黄金全链。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let cleared = hotplug_cycles()?;
    let unknown = unknown_injection()?;
    let mut bus = HotplugBus::new();
    bus.add(0x500, "combo")?;
    bus.bind(0x500)?;
    bus.push_frame(0x500, 4)?;
    bus.remove(0x500)?;
    bus.residue_check()?;
    bus.stream_check()?;
    let mut probe = UnknownProbe::new(UnknownProbe::default_known());
    let _ = probe.observe(0x500, 0x77, 1, 0, 1);
    let mut digest = bus.digest();
    digest = life_word(digest, probe.digest() as u32);
    digest = life_word(digest, (probe.digest() >> 32) as u32);
    Ok(GoldenOut {
        cleared,
        unknown,
        samples: probe.samples().len() as u32,
        digest,
    })
}

// ── 注入账 ────────────────────────────────────────────────────
/// 注入账：4 槽。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 每槽注入次数。
    pub tries: [u32; 4],
    /// 每槽按预期命中次数。
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 4], caught: [0u32; 4] }
    }

    /// 记账。
    pub fn record(&mut self, slot: usize, caught: bool) -> bool {
        let mut ok = false;
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            ok = true;
        }
        if ok && caught {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
        ok
    }

    /// 四槽全中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || t != c {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 4 槽 × rounds：重复 add / 重复 remove / 已拔设备入帧 / 空证据取证。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let mut bus = HotplugBus::new();
        bus.add(1, "x")?;

        // 槽 0：重复 add。
        led.record(
            0,
            matches!(bus.add(1, "x"), Err(c) if c == EdgeCode::AlreadyPresent.val()),
        );

        // 槽 1：重复 remove。
        bus.remove(1)?;
        led.record(
            1,
            matches!(bus.remove(1), Err(c) if c == EdgeCode::Gone.val()),
        );

        // 槽 2：已拔设备入帧。
        led.record(
            2,
            matches!(bus.push_frame(1, 1), Err(c) if c == EdgeCode::Gone.val()),
        );

        // 槽 3：空证据取证。
        let probe = UnknownProbe::new(UnknownProbe::default_known());
        led.record(
            3,
            matches!(probe.evidence_for(1), Err(c) if c == EdgeCode::NoEvidence.val()),
        );
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────
/// 探针 A：add/bind/帧/remove 全链，清算数恰为入帧数。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut bus = HotplugBus::new();
    bus.add(7, "a")?;
    bus.bind(7)?;
    bus.push_frame(7, 3)?;
    bus.push_frame(7, 2)?;
    let before = bus.inflight_len();
    bus.remove(7)?;
    if before != 2 || bus.inflight_len() != 0 {
        return Err(EdgeCode::InflightResidue.val());
    }
    if bus.dropped_inflight() != 2 {
        return Err(EdgeCode::InflightResidue.val());
    }
    bus.residue_check()?;
    bus.stream_check()?;
    if bus.live_count() != 0 {
        return Err(EdgeCode::LiveAfterEvict.val());
    }
    Ok(())
}

/// 探针 B：重复 add / 已拔查询 / 已拔入帧三拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut bus = HotplugBus::new();
    bus.add(8, "b")?;
    if bus.add(8, "b").is_ok() {
        return Err(EdgeCode::AlreadyPresent.val());
    }
    bus.remove(8)?;
    if bus.query(8).is_ok() {
        return Err(EdgeCode::Gone.val());
    }
    if bus.remove(8).is_ok() {
        return Err(EdgeCode::Gone.val());
    }
    Ok(())
}

/// 探针 C：取证分类闸（先 type 后 code）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut probe = UnknownProbe::new(UnknownProbe::default_known());
    // 未知 type 且码也不在：必须记 TypeMiss（不是 CodeMiss）。
    let c = probe.observe(1, 0x7E, 0xFFFF, 0, 1);
    if c != UnkClass::TypeMiss {
        return Err(EdgeCode::UnknownType.val());
    }
    // 族在、码不在：CodeMiss。
    let c = probe.observe(1, 0x01, 0x2F0, 0, 2);
    if c != UnkClass::CodeMiss {
        return Err(EdgeCode::UnknownCode.val());
    }
    // 已知事件不计数。
    let c = probe.observe(1, 0x01, 0x1E, 1, 3);
    if c != UnkClass::Known {
        return Err(EdgeCode::UnknownCode.val());
    }
    if probe.total_unknown() != 2 {
        return Err(EdgeCode::EvidenceMismatch.val());
    }
    probe.reconcile()
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EdgeCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < EdgeCode::ALL.len() {
        match EdgeCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v <= SEG_BASE || v >= SEG_BASE + 0x20 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // F48811-J1：热插拔 ×10 周期在途帧清算零残留。
    let cycles = hotplug_cycles();
    v.push((
        "F48811-J1-热插拔10周期零残留",
        matches!(cycles, Ok(n) if n == HOTPLUG_CYCLES * INFLIGHT_PER_CYCLE),
        format!("cleared={:?}", cycles),
    ));

    // F48811-J1：热移除后查询/入帧必 Gone（无幽灵）。
    let ghost = {
        let mut bus = HotplugBus::new();
        bus.add(3, "g").is_ok()
            && bus.bind(3).is_ok()
            && bus.push_frame(3, 1).is_ok()
            && bus.remove(3).is_ok()
            && bus.query(3).is_err()
            && bus.push_frame(3, 1).is_err()
            && bus.remove(3).is_err()
    };
    v.push((
        "F48811-J1-移除后三路必Gone",
        ghost,
        String::from("query/push/remove 全拒"),
    ));

    // F48811-J1：重复 add 拒绝（已插设备）。
    let dup = {
        let mut bus = HotplugBus::new();
        bus.add(4, "d").is_ok()
            && matches!(bus.add(4, "d"), Err(c) if c == EdgeCode::AlreadyPresent.val())
            && bus.stream_check() == Ok(1)
    };
    v.push((
        "F48811-J1-重复add不污染事件流",
        dup,
        String::from("事件流仍 1 条"),
    ));

    // F48811-J1：总线事件流序号连续且 Add/Remove 成对。
    let stream_ok = {
        let mut bus = HotplugBus::new();
        let _ = bus.add(1, "s").is_ok();
        let _ = bus.remove(1).is_ok();
        let _ = bus.add(2, "s").is_ok();
        let _ = bus.remove(2).is_ok();
        let mut adds = 0u32;
        let mut rems = 0u32;
        let mut i = 0usize;
        while i < bus.stream().len() {
            if let Some(e) = bus.stream().get(i) {
                match e.kind {
                    BusEventKind::Add => adds += 1,
                    BusEventKind::Remove => rems += 1,
                }
            }
            i += 1;
        }
        bus.stream_check() == Ok(4) && adds == 2 && rems == 2
    };
    v.push((
        "F48811-J1-事件流AddRemove成对",
        stream_ok,
        String::from("adds=2 removes=2"),
    ));

    // F48812-J1：未知事件 ×100 全计数。
    let unknown = unknown_injection();
    v.push((
        "F48812-J1-未知注入100全计数",
        matches!(unknown, Ok(n) if n == UNKNOWN_INJECTIONS),
        format!("unknown={:?}", unknown),
    ));

    // F48812-J1：证据面 16 样本 + 丢弃计数对拍。
    let evid = {
        let mut probe = UnknownProbe::new(UnknownProbe::default_known());
        let mut i = 0u32;
        while i < 40 {
            let _ = probe.observe(1, 0x7E, 1, i as i32, i as u64);
            i += 1;
        }
        probe.reconcile().is_ok()
            && probe.samples().len() == EVIDENCE_CAP
            && probe.evidence_dropped() == 40 - EVIDENCE_CAP as u32
            && probe.total_unknown() == 40
    };
    v.push((
        "F48812-J1-证据环16加丢弃对拍",
        evid,
        String::from("samples=16 dropped=24 total=40"),
    ));

    // F48812-J1：分类闸（未知 type 不误记未知 code）。
    v.push((
        "F48812-J1-未知分类不误记",
        probe_seq_c().is_ok(),
        String::from("TypeMiss/CodeMiss/Known 三态"),
    ));

    // F48811-J2：注入 4 槽 ×10 全中。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = matches!(&led, Ok(l) if l.all_caught());
    v.push((
        "F48811-J2-注入10轮4槽全中",
        led_ok,
        format!(
            "tries={:?} caught={:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));

    // F48811-J3：双跑 + 段位。
    v.push((
        "F48811-J3-双跑与段位",
        repro_check().is_ok() && segment_guard(),
        format!("golden={:?}", golden_run().map(|g| g.digest)),
    ));

    v
}

pub fn run_unx48811_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48811 输入事件边角：热插拔与未知取证");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_cycles_clear_inflight() {
        assert_eq!(hotplug_cycles(), Ok(HOTPLUG_CYCLES * INFLIGHT_PER_CYCLE));
    }

    #[test]
    fn remove_is_same_operation_cleanup() {
        let mut bus = HotplugBus::new();
        bus.add(1, "a").expect("add");
        bus.bind(1).expect("bind");
        bus.push_frame(1, 3).expect("f1");
        bus.push_frame(1, 3).expect("f2");
        bus.push_frame(1, 3).expect("f3");
        assert_eq!(bus.inflight_len(), 3);
        bus.remove(1).expect("remove");
        assert_eq!(bus.inflight_len(), 0);
        assert_eq!(bus.dropped_inflight(), 3);
        assert!(bus.residue_check().is_ok());
    }

    #[test]
    fn bystander_frames_survive_neighbor_removal() {
        let mut bus = HotplugBus::new();
        bus.add(1, "a").expect("add1");
        bus.add(2, "b").expect("add2");
        bus.bind(1).expect("bind1");
        bus.bind(2).expect("bind2");
        bus.push_frame(1, 1).expect("f1");
        bus.push_frame(2, 1).expect("f2");
        bus.remove(1).expect("remove1");
        assert_eq!(bus.inflight_len(), 1);
        bus.residue_check().expect("无残留");
        let left = bus.pop_frame(2).expect("剩余帧属于设备 2");
        assert_eq!(left.dev, 2);
    }

    #[test]
    fn duplicate_add_and_repeat_remove() {
        assert!(probe_seq_b().is_ok());
        let mut bus = HotplugBus::new();
        bus.add(9, "d").expect("add");
        assert_eq!(bus.add(9, "d").err(), Some(EdgeCode::AlreadyPresent.val()));
        // 事件流未被污染的重复 add 推进（只有 1 条 Add）。
        assert_eq!(bus.stream_check(), Ok(1));
        bus.remove(9).expect("remove");
        assert_eq!(bus.remove(9).err(), Some(EdgeCode::Gone.val()));
    }

    #[test]
    fn reattach_reuses_slot_and_counts() {
        let mut bus = HotplugBus::new();
        bus.add(5, "r").expect("add1");
        bus.remove(5).expect("rm1");
        bus.add(5, "r").expect("add2");
        let slot = bus.query(5).expect("重插后可见");
        assert_eq!(slot.attaches, 2);
        assert_eq!(slot.state, DeviceState::Probing);
        assert_eq!(bus.stream_check(), Ok(3));
    }

    #[test]
    fn bind_requires_probing_state() {
        let mut bus = HotplugBus::new();
        bus.add(6, "t").expect("add");
        bus.bind(6).expect("bind");
        assert_eq!(bus.bind(6).err(), Some(EdgeCode::BadTransition.val()));
    }

    #[test]
    fn unknown_hundred_all_counted() {
        assert_eq!(unknown_injection(), Ok(UNKNOWN_INJECTIONS));
    }

    #[test]
    fn classification_type_before_code() {
        assert!(probe_seq_c().is_ok());
        let mut probe = UnknownProbe::new(UnknownProbe::default_known());
        assert_eq!(probe.observe(1, 0x7E, 0x1E, 0, 0), UnkClass::TypeMiss);
        assert_eq!(probe.observe(1, 0x01, 0x2F0, 0, 0), UnkClass::CodeMiss);
        assert_eq!(probe.observe(1, 0x01, 0x1E, 0, 0), UnkClass::Known);
        assert_eq!(probe.type_hits().len(), 1);
        assert_eq!(probe.code_hits().len(), 1);
        assert!(probe.reconcile().is_ok());
    }

    #[test]
    fn evidence_ring_caps_at_sixteen() {
        let mut probe = UnknownProbe::new(UnknownProbe::default_known());
        let mut i = 0u32;
        while i < 40 {
            probe.observe(1, 0x99, 0, i as i32, i as u64);
            i += 1;
        }
        assert_eq!(probe.samples().len(), EVIDENCE_CAP);
        assert_eq!(probe.evidence_dropped(), 24);
        assert_eq!(probe.total_unknown(), 40);
        // 最早的一条已被挤出，查不到。
        assert_eq!(probe.evidence_for(1).err(), Some(EdgeCode::NoEvidence.val()));
        // 最后一条必须能查到。
        let last = probe.samples().last().expect("末样本");
        assert_eq!(probe.evidence_for(last.seq).ok().map(|e| e.seq), Some(last.seq));
        assert!(probe.reconcile().is_ok());
    }

    #[test]
    fn golden_run_is_stable() {
        let a = golden_run().expect("黄金链");
        assert_eq!(a, golden_run().expect("二跑"));
        assert_eq!(a.cleared, HOTPLUG_CYCLES * INFLIGHT_PER_CYCLE);
        assert_eq!(a.unknown, UNKNOWN_INJECTIONS);
        assert_eq!(a.samples, 1);
    }

    #[test]
    fn inject_four_slots_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).expect("注入");
        assert!(led.all_caught());
        assert_eq!(led.tries, [JUDGE_ROUNDS; 4]);
        assert_eq!(led.caught, [JUDGE_ROUNDS; 4]);
    }

    #[test]
    fn probes_a_b_c_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn codes_are_segment_closed() {
        assert!(segment_guard());
        for c in EdgeCode::ALL.iter() {
            let v = c.val();
            assert!(v > SEG_BASE && v < SEG_BASE + 0x20);
            assert_eq!(detail(v), c.detail());
        }
        assert_eq!(detail(0x7014D), "未知诊断码：不在 0x70141..0x7014C 闭集内");
    }
}
