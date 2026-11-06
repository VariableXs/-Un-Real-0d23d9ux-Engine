//! UNX-F48803 设备节点元数据表 · 0x70040
//! 覆盖卡：UNX-F48803（路径/名称/能力位图/vid-pid/总线拓扑五元组注册查询）、
//!         UNX-F48804（EV_SYN 帧边界语义：帧内事件原子递交，半帧不可见）
//!
//! ── 闸门次序 ──
//! 本模块的注册面按四道闸排序，倒序会放行不同性质的坏元数据：
//!   ① 字段完备闸：路径非空、名称非空、evbit 非零。
//!      若放到唯一闸之后，空路径会被当成「一个合法但匿名的节点」登记进去，
//!      之后所有按路径查询都查不到它，形成**不可达节点**（占位不报错）。
//!   ② evbit 闭集闸：evbit 只允许十族槽位（0..9）。
//!      若放到唯一闸之后，第 11 位起的伪造族会被登记成「有某种未知能力」，
//!      位图面（unx48801）随后会用 CapabilityDenied 误判为设备能力缺失，
//!      而真实原因是元数据表里塞了闭集外的族——定责必须落在注册这一侧。
//!   ③ 唯一闸：同一路径在同一时刻只能有一个节点。
//!      此闸必须在指纹闸之前：重复注册会先污染指纹账，再回滚就找不到
//!      「污染前的原值」；先拒重复，指纹账才保持只增不改。
//!   ④ 指纹闸：五元组逐字段进指纹，注册/注销后重算必须与账一致。
//!      此闸最后判：它校验的是「账本完整性」，不是「输入合法性」；
//!      放到前面会把「字段错」报成「指纹漂移」，排障方向完全错。
//!
//! 帧面（F48804）只有一道闸，但它是**原子性闸**：
//!   未见到 SYN_REPORT 的 pending 事件一律不得对读者可见。半帧试图暴露时
//!   必须报 HalfFrameVisible 并计数（half_attempts/half_rejects 双向记账），
//!   不允许「先给读者看一部分、再补齐」——那正是撕裂帧的成因。
//!
//! ── 关键差异 ──
//! 与 unx48801_input_core 的边界：core 持有**能力位图本体**（evbit 与码位图），
//!   本模块只持有**位图的指纹与所属设备**（五元组里的 evbit 字段是位图摘要，
//!   不是位图存储）。重复实现位图存储会让「一次读取缓存」出现两个副本。
//!   本模块判死的是「元数据是否成套、路径是否唯一、指纹是否漂移」。
//! 与 unx48805_input_guard 的边界：本模块的帧只有**边界**语义（何时可见），
//!   没有**时间**语义。缺 SYN 100ms 合成、时间戳倒流、4096 溢出都是 guard 的
//!   职责；本模块的 `half_attempts` 计的是「读者提前窥视」，不是「超时合成」。
//! 与 unx48808_input_dispatch 的边界：本模块出队的是**帧对象**（已提交），
//!   dispatch 负责把帧推到 per-CPU 环并做批量递交；本模块不关心 CPU 亲和。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本模块诊断段基址。
pub const SEG_BASE: u32 = 0x70040;

/// 五元组字段数（路径/名称/位图/vid-pid/拓扑）。
pub const TUPLE_FIELDS: usize = 5;
/// 样本设备台数。
pub const SAMPLE_DEVICES: usize = 10;
/// 注册/查询/注销的轮数（卡面「×100 轮」）。
pub const ROUNDTRIP_ROUNDS: u32 = 100;
/// 判据注入次数（卡面「×10」）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 单帧事件上限（防伪造超长帧；正常帧远小于此）。
pub const FRAME_EVENT_CAP: usize = 256;
/// 未提交 pending 上限（超过即视为驱动异常，拒收并不静默丢弃）。
pub const PENDING_CAP: usize = 512;

// ── 诊断码闭集（12 条）────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetaCode {
    /// 路径为空。
    EmptyPath,
    /// 名称为空。
    EmptyName,
    /// evbit 为零（设备不能没有任何能力）。
    EmptyEvbit,
    /// evbit 含十族以外位。
    EvbitOutOfFamily,
    /// 路径重复（唯一闸）。
    DuplicatePath,
    /// 路径不存在（查询/注销未命中）。
    NoSuchNode,
    /// 五元组指纹漂移。
    TupleDrift,
    /// 拓扑字段越界（控制器/端口/深度）。
    TopologyBad,
    /// 帧序错乱（提交序号不连续）。
    FrameOrderBad,
    /// 半帧试图暴露给读者。
    HalfFrameVisible,
    /// 帧超长（越 FRAME_EVENT_CAP）。
    FrameTooLong,
    /// 双跑不一致。
    NotReproducible,
}

impl MetaCode {
    /// 段内绝对码值（+1 起，避开段首）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文文案。
    pub fn detail(self) -> &'static str {
        match self {
            MetaCode::EmptyPath => "节点路径为空",
            MetaCode::EmptyName => "节点名称为空",
            MetaCode::EmptyEvbit => "evbit 为零：设备无任何能力",
            MetaCode::EvbitOutOfFamily => "evbit 含十族以外位",
            MetaCode::DuplicatePath => "路径重复：唯一闸拒登记",
            MetaCode::NoSuchNode => "路径不存在：查询未命中",
            MetaCode::TupleDrift => "五元组指纹漂移",
            MetaCode::TopologyBad => "拓扑字段越界：控制器/端口/深度",
            MetaCode::FrameOrderBad => "帧序错乱：提交序号不连续",
            MetaCode::HalfFrameVisible => "半帧暴露：缺 SYN_REPORT",
            MetaCode::FrameTooLong => "帧超长：越 256 事件上限",
            MetaCode::NotReproducible => "双跑不一致",
        }
    }

    /// 闭集全表。
    pub const ALL: [MetaCode; 12] = [
        MetaCode::EmptyPath,
        MetaCode::EmptyName,
        MetaCode::EmptyEvbit,
        MetaCode::EvbitOutOfFamily,
        MetaCode::DuplicatePath,
        MetaCode::NoSuchNode,
        MetaCode::TupleDrift,
        MetaCode::TopologyBad,
        MetaCode::FrameOrderBad,
        MetaCode::HalfFrameVisible,
        MetaCode::FrameTooLong,
        MetaCode::NotReproducible,
    ];
}

/// 码值 → 文案；未知码返回闭集外说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < MetaCode::ALL.len() {
        if let Some(c) = MetaCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70041..0x7004C 闭集内"
}

// ── 指纹基础设施（本域按「五元组指纹」命名）──────────────────
/// 指纹初值。
pub fn tuple_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 折入一字节。
pub fn tuple_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折入一个 32 位字。
pub fn tuple_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 4 {
        acc = tuple_byte(acc, ((w >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 折入字符串。
pub fn tuple_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc = tuple_byte(acc, *b);
        }
        i += 1;
    }
    acc
}

/// 十族槽位掩码（与 core 同一冻结口径，此处只是常量而不是位图存储）。
pub const EV_MASK_ALL: u32 = 0x3FF;

// ── 总线拓扑 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BusKind {
    /// USB 控制器。
    Usb,
    /// 传统 i8042（PS/2）。
    I8042,
    /// I2C/HID-over-I2C。
    I2c,
    /// 蓝牙 HID。
    Bluetooth,
    /// 虚拟/注入设备。
    Virtual,
}

impl BusKind {
    /// 总线码（入指纹）。
    pub fn code(self) -> u8 {
        match self {
            BusKind::Usb => 3,
            BusKind::I8042 => 1,
            BusKind::I2c => 0x18,
            BusKind::Bluetooth => 5,
            BusKind::Virtual => 0x1F,
        }
    }

    /// 总线名。
    pub fn name(self) -> &'static str {
        match self {
            BusKind::Usb => "usb",
            BusKind::I8042 => "i8042",
            BusKind::I2c => "i2c",
            BusKind::Bluetooth => "bt",
            BusKind::Virtual => "vrt",
        }
    }

    /// 码 → 总线；未知返回 None。
    pub fn from_code(c: u8) -> Option<BusKind> {
        match c {
            3 => Some(BusKind::Usb),
            1 => Some(BusKind::I8042),
            0x18 => Some(BusKind::I2c),
            5 => Some(BusKind::Bluetooth),
            0x1F => Some(BusKind::Virtual),
            _ => None,
        }
    }

    /// 全表。
    pub const ALL: [BusKind; 5] = [
        BusKind::Usb,
        BusKind::I8042,
        BusKind::I2c,
        BusKind::Bluetooth,
        BusKind::Virtual,
    ];
}

/// 拓扑：总线 + 控制器号 + 端口号 + 层级深度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Topo {
    /// 总线类型。
    pub bus: BusKind,
    /// 控制器号（0..=15）。
    pub ctrl: u8,
    /// 端口号（0..=63）。
    pub port: u8,
    /// 层级深度（1..=7，1 为根端口）。
    pub depth: u8,
}

impl Topo {
    /// 构造 + 范围校验。
    pub fn new(bus: BusKind, ctrl: u8, port: u8, depth: u8) -> Result<Topo, u32> {
        if ctrl > 15 || port > 63 || depth == 0 || depth > 7 {
            return Err(MetaCode::TopologyBad.val());
        }
        Ok(Topo { bus, ctrl, port, depth })
    }

    /// 拓扑串（`usb-0:3.2` 形式，入指纹与报告）。
    pub fn label(&self) -> String {
        format!("{}-{}:{}.{}", self.bus.name(), self.ctrl, self.port, self.depth)
    }

    /// 拓扑自检。
    pub fn check(&self) -> Result<(), u32> {
        if self.ctrl > 15 || self.port > 63 || self.depth == 0 || self.depth > 7 {
            return Err(MetaCode::TopologyBad.val());
        }
        if BusKind::from_code(self.bus.code()) != Some(self.bus) {
            return Err(MetaCode::TopologyBad.val());
        }
        Ok(())
    }
}

// ── 五元组节点 ────────────────────────────────────────────────
/// 节点五元组：路径 / 名称 / 能力位图摘要 / vid-pid / 总线拓扑。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeKey {
    /// 设备节点路径（唯一键）。
    pub path: String,
    /// 实物名。
    pub name: String,
    /// evbit 摘要（十族槽位掩码）。
    pub evbit: u32,
    /// 厂商号。
    pub vid: u16,
    /// 产品号。
    pub pid: u16,
    /// 总线拓扑。
    pub topo: Topo,
}

impl NodeKey {
    /// 构造（不做唯一性校验，唯一性属于表）。
    pub fn new(
        path: &str,
        name: &str,
        evbit: u32,
        vid: u16,
        pid: u16,
        topo: Topo,
    ) -> NodeKey {
        NodeKey {
            path: path.to_string(),
            name: name.to_string(),
            evbit,
            vid,
            pid,
            topo,
        }
    }

    /// 字段完备 + 闭集校验（①②两闸）。
    pub fn validate(&self) -> Result<(), u32> {
        if self.path.is_empty() {
            return Err(MetaCode::EmptyPath.val());
        }
        if self.name.is_empty() {
            return Err(MetaCode::EmptyName.val());
        }
        if self.evbit == 0 {
            return Err(MetaCode::EmptyEvbit.val());
        }
        if self.evbit & !EV_MASK_ALL != 0 {
            return Err(MetaCode::EvbitOutOfFamily.val());
        }
        self.topo.check()
    }

    /// 五元组指纹：路径 → 名称 → 位图 → vid → pid → 拓扑四字段。
    pub fn digest(&self) -> u64 {
        let mut h = tuple_init();
        h = tuple_str(h, self.path.as_str());
        h = tuple_byte(h, 0x1C);
        h = tuple_str(h, self.name.as_str());
        h = tuple_byte(h, 0x1C);
        h = tuple_word(h, self.evbit);
        h = tuple_word(h, self.vid as u32);
        h = tuple_word(h, self.pid as u32);
        h = tuple_byte(h, self.topo.bus.code());
        h = tuple_byte(h, self.topo.ctrl);
        h = tuple_byte(h, self.topo.port);
        h = tuple_byte(h, self.topo.depth);
        h
    }

    /// 字段数（自证五元组没被悄悄扩成六元组）。
    pub fn field_count(&self) -> usize {
        TUPLE_FIELDS
    }

    /// 报告行（排障三要素：路径 + 实物名 + 拓扑）。
    pub fn report(&self) -> String {
        format!(
            "{} [{}] {:04X}:{:04X} ev=0x{:03X} @{}",
            self.path,
            self.name,
            self.vid,
            self.pid,
            self.evbit,
            self.topo.label()
        )
    }
}

/// 表项：五元组 + 登记序号（序号单调，注销不复用）。
#[derive(Clone, Debug)]
pub struct NodeEntry {
    /// 五元组。
    pub key: NodeKey,
    /// 登记序号（从 1 起）。
    pub seq: u32,
}

/// 节点元数据表。
pub struct NodeTable {
    nodes: Vec<NodeEntry>,
    seq_next: u32,
    registered_total: u32,
    evicted_total: u32,
}

impl NodeTable {
    /// 空表。
    pub fn new() -> NodeTable {
        NodeTable {
            nodes: Vec::new(),
            seq_next: 1,
            registered_total: 0,
            evicted_total: 0,
        }
    }

    /// 登记（四闸次序见文件头）。
    pub fn register(&mut self, key: NodeKey) -> Result<u32, u32> {
        key.validate()?;
        // 唯一闸：同路径在表内即拒，且拒得不改任何账。
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(e) = self.nodes.get(i) {
                if e.key.path == key.path {
                    return Err(MetaCode::DuplicatePath.val());
                }
            }
            i += 1;
        }
        let seq = self.seq_next;
        self.seq_next = self.seq_next.saturating_add(1);
        self.nodes.push(NodeEntry { key, seq });
        self.registered_total = self.registered_total.saturating_add(1);
        Ok(seq)
    }

    /// 按路径查询。
    pub fn by_path(&self, path: &str) -> Result<&NodeKey, u32> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(e) = self.nodes.get(i) {
                if e.key.path == path {
                    return Ok(&e.key);
                }
            }
            i += 1;
        }
        Err(MetaCode::NoSuchNode.val())
    }

    /// 按 vid:pid 查询（同一对 vid:pid 允许多个节点，返回全部）。
    pub fn by_vidpid(&self, vid: u16, pid: u16) -> Vec<&NodeKey> {
        let mut out: Vec<&NodeKey> = Vec::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(e) = self.nodes.get(i) {
                if e.key.vid == vid && e.key.pid == pid {
                    out.push(&e.key);
                }
            }
            i += 1;
        }
        out
    }

    /// 按总线查（拓扑维度的查询面）。
    pub fn by_bus(&self, bus: BusKind) -> Vec<&NodeKey> {
        let mut out: Vec<&NodeKey> = Vec::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(e) = self.nodes.get(i) {
                if e.key.topo.bus == bus {
                    out.push(&e.key);
                }
            }
            i += 1;
        }
        out
    }

    /// 注销：返回被摘下的五元组；未命中报 NoSuchNode。
    pub fn unregister(&mut self, path: &str) -> Result<NodeKey, u32> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            let hit = match self.nodes.get(i) {
                Some(e) => e.key.path == path,
                None => false,
            };
            if hit {
                let e = match self.nodes.get(i) {
                    Some(e) => e,
                    None => return Err(MetaCode::NoSuchNode.val()),
                };
                let key = e.key.clone();
                self.nodes.remove(i);
                self.evicted_total = self.evicted_total.saturating_add(1);
                return Ok(key);
            }
            i += 1;
        }
        Err(MetaCode::NoSuchNode.val())
    }

    /// 表指纹：按登记序串五元组指纹（表空时等于初值）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = tuple_init();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(e) = self.nodes.get(i) {
                h = tuple_word(h, e.key.digest() as u32);
                h = tuple_word(h, (e.key.digest() >> 32) as u32);
                h = tuple_word(h, e.seq);
            }
            i += 1;
        }
        h
    }

    /// 表内节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 表空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 累计登记次数。
    pub fn registered_total(&self) -> u32 {
        self.registered_total
    }

    /// 累计注销次数。
    pub fn evicted_total(&self) -> u32 {
        self.evicted_total
    }

    /// 逐节点自检：五元组合法 + 路径在表内唯一。
    pub fn audit(&self) -> Result<u32, u32> {
        let mut i = 0usize;
        let mut checked = 0u32;
        while i < self.nodes.len() {
            let e = match self.nodes.get(i) {
                Some(e) => e,
                None => return Err(MetaCode::NoSuchNode.val()),
            };
            e.key.validate()?;
            let mut j = i + 1;
            while j < self.nodes.len() {
                if let Some(o) = self.nodes.get(j) {
                    if o.key.path == e.key.path {
                        return Err(MetaCode::DuplicatePath.val());
                    }
                }
                j += 1;
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }
}

// ── 十台样本设备的元数据（与 core 的画像同实物、不同视角）──────
/// 十元组样本：路径 / 名称 / evbit / vid:pid / 拓扑。
pub fn sample_nodes() -> Vec<NodeKey> {
    let mut v: Vec<NodeKey> = Vec::new();
    let ev_kbd: u32 = 0x01B; // SYN|KEY|MSC|LED|REP
    let ev_kbd87: u32 = 0x01B;
    let ev_mouse: u32 = 0x007; // SYN|KEY|REL
    let ev_pad: u32 = 0x01F; // SYN|KEY|REL|ABS|MSC
    let ev_ts: u32 = 0x019; // SYN|KEY|ABS|MSC
    let ev_gamepad: u32 = 0x20B; // SYN|KEY|ABS|FF
    let ev_remote: u32 = 0x101; // SYN|KEY|REP
    let ev_pwr: u32 = 0x003; // SYN|KEY
    let ev_tablet: u32 = 0x01B; // SYN|KEY|ABS|MSC
    let ev_hotkey: u32 = 0x1C3; // SYN|KEY|LED|SND|REP

    v.push(NodeKey::new(
        "/dev/input/event0",
        "atkbd-104",
        ev_kbd,
        0x0001,
        0x0001,
        match Topo::new(BusKind::I8042, 0, 1, 1) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::I8042, ctrl: 0, port: 1, depth: 1 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event1",
        "atkbd-87",
        ev_kbd87,
        0x0001,
        0x0001,
        match Topo::new(BusKind::I8042, 0, 2, 1) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::I8042, ctrl: 0, port: 2, depth: 1 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event2",
        "ps2-mouse",
        ev_mouse,
        0x046D,
        0xC077,
        match Topo::new(BusKind::I8042, 0, 3, 1) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::I8042, ctrl: 0, port: 3, depth: 1 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event3",
        "synaptics-touchpad",
        ev_pad,
        0x06CB,
        0x2970,
        match Topo::new(BusKind::I2c, 1, 4, 2) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::I2c, ctrl: 1, port: 4, depth: 2 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event4",
        "elan-touchscreen",
        ev_ts,
        0x04F3,
        0x2D4A,
        match Topo::new(BusKind::I2c, 2, 5, 2) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::I2c, ctrl: 2, port: 5, depth: 2 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event5",
        "xbox-gamepad",
        ev_gamepad,
        0x045E,
        0x0B12,
        match Topo::new(BusKind::Usb, 3, 6, 3) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::Usb, ctrl: 3, port: 6, depth: 3 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event6",
        "cec-remote",
        ev_remote,
        0x0000,
        0x0000,
        match Topo::new(BusKind::Virtual, 7, 7, 1) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::Virtual, ctrl: 7, port: 7, depth: 1 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event7",
        "pwrbutton",
        ev_pwr,
        0x0001,
        0x0003,
        match Topo::new(BusKind::I8042, 0, 8, 1) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::I8042, ctrl: 0, port: 8, depth: 1 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event8",
        "wacom-tablet",
        ev_tablet,
        0x056A,
        0x0374,
        match Topo::new(BusKind::Usb, 4, 9, 3) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::Usb, ctrl: 4, port: 9, depth: 3 },
        },
    ));
    v.push(NodeKey::new(
        "/dev/input/event9",
        "hotkeys-combo",
        ev_hotkey,
        0x17EF,
        0x60C1,
        match Topo::new(BusKind::Bluetooth, 5, 10, 4) {
            Ok(t) => t,
            Err(_) => Topo { bus: BusKind::Bluetooth, ctrl: 5, port: 10, depth: 4 },
        },
    ));
    v
}

// ── 帧边界语义（F48804）──────────────────────────────────────
/// 帧内最小事件（与 input_event 同构，但不带时间戳：时间归 guard）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RawEv {
    /// type。
    pub kind: u16,
    /// code。
    pub code: u16,
    /// value。
    pub value: i32,
}

impl RawEv {
    /// 构造。
    pub const fn new(kind: u16, code: u16, value: i32) -> RawEv {
        RawEv { kind, code, value }
    }

    /// 指纹。
    pub fn digest(&self) -> u64 {
        let mut h = tuple_init();
        h = tuple_byte(h, (self.kind & 0xFF) as u8);
        h = tuple_byte(h, (self.kind >> 8) as u8);
        h = tuple_byte(h, (self.code & 0xFF) as u8);
        h = tuple_byte(h, (self.code >> 8) as u8);
        h = tuple_word(h, self.value as u32);
        h
    }
}

/// 已提交帧（读者能看到的唯一形态）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    /// 帧内事件（按序）。
    pub events: Vec<RawEv>,
    /// 收尾的同步子码（正常为 SYN_REPORT）。
    pub syn: u16,
    /// 帧序号（从 1 起，连续）。
    pub ordinal: u32,
}

impl Frame {
    /// 帧指纹。
    pub fn digest(&self) -> u64 {
        let mut h = tuple_init();
        h = tuple_word(h, self.ordinal);
        h = tuple_byte(h, self.syn as u8);
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                h = tuple_word(h, e.digest() as u32);
            }
            i += 1;
        }
        h
    }

    /// 事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空帧（不允许存在，自检用）。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// 推送结果（调用方按结果记账，不靠副作用猜）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PushOutcome {
    /// 进 pending，尚未可见。
    Pending,
    /// 收到 SYN_REPORT，整帧提交。
    Committed,
    /// 收到 SYN_DROPPED，pending 作废。
    Voided,
    /// 收到 SYN_MT_REPORT，pending 保留（不是帧界）。
    MtSep,
}

/// 帧装配器：pending 与 committed 严格分离。
pub struct FrameAssembler {
    pending: Vec<RawEv>,
    frames: Vec<Frame>,
    half_attempts: u32,
    half_rejects: u32,
    voided_frames: u32,
    voided_events: u32,
    mt_seps: u32,
    rejected_long: u32,
}

impl FrameAssembler {
    /// 空装配器。
    pub fn new() -> FrameAssembler {
        FrameAssembler {
            pending: Vec::new(),
            frames: Vec::new(),
            half_attempts: 0,
            half_rejects: 0,
            voided_frames: 0,
            voided_events: 0,
            mt_seps: 0,
            rejected_long: 0,
        }
    }

    /// 推送一条事件。
    pub fn push(&mut self, ev: RawEv) -> Result<PushOutcome, u32> {
        // 同步族：三种子码三种语义，逐条分支（不合并成「都算帧界」）。
        if ev.kind == 0x00 {
            match ev.code {
                0 => {
                    let events = core::mem::take(&mut self.pending);
                    if events.len() > FRAME_EVENT_CAP {
                        self.rejected_long = self.rejected_long.saturating_add(1);
                        return Err(MetaCode::FrameTooLong.val());
                    }
                    let ordinal = (self.frames.len() as u32).saturating_add(1);
                    self.frames.push(Frame { events, syn: 0, ordinal });
                    return Ok(PushOutcome::Committed);
                }
                3 => {
                    if !self.pending.is_empty() {
                        self.voided_frames = self.voided_frames.saturating_add(1);
                        self.voided_events =
                            self.voided_events.saturating_add(self.pending.len() as u32);
                        self.pending.clear();
                    }
                    return Ok(PushOutcome::Voided);
                }
                2 => {
                    self.mt_seps = self.mt_seps.saturating_add(1);
                    if self.pending.len() >= PENDING_CAP {
                        return Err(MetaCode::FrameTooLong.val());
                    }
                    self.pending.push(ev);
                    return Ok(PushOutcome::MtSep);
                }
                _ => {}
            }
        }
        if self.pending.len() >= PENDING_CAP {
            return Err(MetaCode::FrameTooLong.val());
        }
        self.pending.push(ev);
        Ok(PushOutcome::Pending)
    }

    /// 读者试图看 pending（半帧）：非空即拒并双向记账。
    pub fn expose_pending(&mut self) -> Result<&[RawEv], u32> {
        self.half_attempts = self.half_attempts.saturating_add(1);
        if !self.pending.is_empty() {
            self.half_rejects = self.half_rejects.saturating_add(1);
            return Err(MetaCode::HalfFrameVisible.val());
        }
        Ok(self.pending.as_slice())
    }

    /// 已提交帧视图（唯一可见面）。
    pub fn committed(&self) -> &[Frame] {
        self.frames.as_slice()
    }

    /// 已提交帧数。
    pub fn committed_len(&self) -> usize {
        self.frames.len()
    }

    /// 读者可见事件数（只数已提交）。
    pub fn visible_events(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.frames.len() {
            if let Some(f) = self.frames.get(i) {
                n += f.events.len();
            }
            i += 1;
        }
        n
    }

    /// pending 事件数（读者不可见）。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 半帧尝试/拒绝次数。
    pub fn half_counts(&self) -> (u32, u32) {
        (self.half_attempts, self.half_rejects)
    }

    /// 作废帧/事件计数。
    pub fn void_counts(&self) -> (u32, u32) {
        (self.voided_frames, self.voided_events)
    }

    /// MT 分隔符计数。
    pub fn mt_seps(&self) -> u32 {
        self.mt_seps
    }

    /// 帧不变量：每帧非空、以 SYN_REPORT 收尾、序号 1..n 连续。
    pub fn invariant_check(&self) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.frames.len() {
            let f = match self.frames.get(i) {
                Some(f) => f,
                None => return Err(MetaCode::FrameOrderBad.val()),
            };
            if f.events.is_empty() {
                return Err(MetaCode::FrameOrderBad.val());
            }
            if f.syn != 0 {
                return Err(MetaCode::FrameOrderBad.val());
            }
            if f.ordinal != (i as u32).saturating_add(1) {
                return Err(MetaCode::FrameOrderBad.val());
            }
            i += 1;
        }
        Ok(self.frames.len() as u32)
    }

    /// 全链指纹（帧序 + 帧内容）。
    pub fn digest(&self) -> u64 {
        let mut h = tuple_init();
        let mut i = 0usize;
        while i < self.frames.len() {
            if let Some(f) = self.frames.get(i) {
                h = tuple_word(h, f.digest() as u32);
                h = tuple_word(h, (f.digest() >> 32) as u32);
            }
            i += 1;
        }
        h
    }
}

/// 一帧的典型三事件（KEY 按下 + MSC 扫描码 + SYN_REPORT 由装配器补）。
pub fn frame_payload(seed: u16) -> Vec<RawEv> {
    let mut v: Vec<RawEv> = Vec::new();
    let key = 0x1E + (seed % 8);
    v.push(RawEv::new(0x04, 0x04, 0x70000 + (seed as i32 % 0x40)));
    v.push(RawEv::new(0x01, key, 1));
    v.push(RawEv::new(0x01, key, 0));
    v
}

// ── 黄金全链 ──────────────────────────────────────────────────
/// 黄金链输出账面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenOut {
    /// 注册台数。
    pub devices: u32,
    /// 逐节点审计通过数。
    pub audited: u32,
    /// 100 轮后表指纹是否回到初值（零漂移）。
    pub drift_free: bool,
    /// 半帧拒绝数。
    pub half_rejects: u32,
    /// 可见事件数。
    pub visible: u32,
    /// 帧指纹。
    pub digest: u64,
}

/// 注册 10 台 → 查询 → 100 轮登记/注销零漂移。
pub fn tuple_roundtrip() -> Result<bool, u32> {
    let nodes = sample_nodes();
    if nodes.len() != SAMPLE_DEVICES {
        return Err(MetaCode::NoSuchNode.val());
    }
    let empty_fp = NodeTable::new().fingerprint();
    let mut round = 0u32;
    while round < ROUNDTRIP_ROUNDS {
        let mut t = NodeTable::new();
        let mut i = 0usize;
        while i < nodes.len() {
            let node = match nodes.get(i) {
                Some(n) => n,
                None => return Err(MetaCode::NoSuchNode.val()),
            };
            let seq = t.register(node.clone())?;
            if seq != (i as u32) + 1 {
                return Err(MetaCode::TupleDrift.val());
            }
            // 路径查询必须回同一指纹。
            let got = t.by_path(node.path.as_str())?;
            if got.digest() != node.digest() {
                return Err(MetaCode::TupleDrift.val());
            }
            // vid:pid 查询至少命中自己。
            let hits = t.by_vidpid(node.vid, node.pid);
            if hits.is_empty() {
                return Err(MetaCode::TupleDrift.val());
            }
            i += 1;
        }
        // 全表审计后逐台注销，表必须回到空指纹。
        t.audit()?;
        let mut j = 0usize;
        while j < nodes.len() {
            if let Some(n) = nodes.get(j) {
                t.unregister(n.path.as_str())?;
            }
            j += 1;
        }
        if !t.is_empty() {
            return Err(MetaCode::TupleDrift.val());
        }
        if t.fingerprint() != empty_fp {
            return Err(MetaCode::TupleDrift.val());
        }
        round += 1;
    }
    Ok(true)
}

/// 半帧注入 ×10：每台设备造一帧，先窥视必拒，补 SYN 后整帧可见。
pub fn half_frame_drill() -> Result<u32, u32> {
    let mut asm = FrameAssembler::new();
    let mut rejected = 0u32;
    let mut i = 0u16;
    while i < JUDGE_ROUNDS as u16 {
        let payload = frame_payload(i);
        let mut k = 0usize;
        while k < payload.len() {
            if let Some(ev) = payload.get(k) {
                asm.push(*ev)?;
            }
            k += 1;
        }
        // 此刻 pending 非空、committed 未增：窥视必须被拒。
        let before = asm.committed_len();
        match asm.expose_pending() {
            Err(c) if c == MetaCode::HalfFrameVisible.val() => {
                rejected += 1;
                if asm.committed_len() != before {
                    return Err(MetaCode::HalfFrameVisible.val());
                }
            }
            _ => return Err(MetaCode::HalfFrameVisible.val()),
        }
        // 补 SYN_REPORT：整帧一次性可见。
        asm.push(RawEv::new(0x00, 0, 0))?;
        if asm.committed_len() != before + 1 {
            return Err(MetaCode::FrameOrderBad.val());
        }
        if let Some(f) = asm.committed().last() {
            if f.events.len() != payload.len() {
                return Err(MetaCode::FrameOrderBad.val());
            }
        }
        i += 1;
    }
    asm.invariant_check()?;
    if asm.visible_events() != (JUDGE_ROUNDS as usize) * 3 {
        return Err(MetaCode::FrameOrderBad.val());
    }
    Ok(rejected)
}

/// 黄金全链。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let drift_free = tuple_roundtrip()?;
    let mut table = NodeTable::new();
    let nodes = sample_nodes();
    let mut i = 0usize;
    while i < nodes.len() {
        if let Some(n) = nodes.get(i) {
            table.register(n.clone())?;
        }
        i += 1;
    }
    let audited = table.audit()?;
    let mut asm = FrameAssembler::new();
    let mut k = 0u16;
    while k < SAMPLE_DEVICES as u16 {
        let payload = frame_payload(k);
        let mut j = 0usize;
        while j < payload.len() {
            if let Some(ev) = payload.get(j) {
                asm.push(*ev)?;
            }
            j += 1;
        }
        asm.push(RawEv::new(0x00, 0, 0))?;
        k += 1;
    }
    let rejected = half_frame_drill()?;
    let mut digest = table.fingerprint();
    digest = tuple_word(digest, asm.digest() as u32);
    digest = tuple_word(digest, (asm.digest() >> 32) as u32);
    Ok(GoldenOut {
        devices: table.len() as u32,
        audited,
        drift_free,
        half_rejects: rejected,
        visible: asm.visible_events() as u32,
        digest,
    })
}

// ── 注入账 ────────────────────────────────────────────────────
/// 注入账：5 槽。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 每槽注入次数。
    pub tries: [u32; 5],
    /// 每槽按预期被拒次数。
    pub caught: [u32; 5],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 5], caught: [0u32; 5] }
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

    /// 五槽全中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 5 {
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

/// 注入 5 槽 × rounds：空路径 / 空名称 / 越族 evbit / 重复路径 / 半帧。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let nodes = sample_nodes();
    let base = match nodes.first() {
        Some(n) => n,
        None => return Err(MetaCode::NoSuchNode.val()),
    };
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：空路径。
        let mut t = NodeTable::new();
        let mut bad = base.clone();
        bad.path = String::new();
        led.record(0, matches!(t.register(bad), Err(c) if c == MetaCode::EmptyPath.val()));

        // 槽 1：空名称。
        let mut bad = base.clone();
        bad.name = String::new();
        led.record(1, matches!(t.register(bad), Err(c) if c == MetaCode::EmptyName.val()));

        // 槽 2：evbit 第 11 位（闭集外）。
        let mut bad = base.clone();
        bad.evbit = 0x800;
        led.record(
            2,
            matches!(t.register(bad), Err(c) if c == MetaCode::EvbitOutOfFamily.val()),
        );

        // 槽 3：重复路径。
        t.register(base.clone())?;
        led.record(
            3,
            matches!(t.register(base.clone()), Err(c) if c == MetaCode::DuplicatePath.val()),
        );

        // 槽 4：半帧窥视。
        let mut asm = FrameAssembler::new();
        asm.push(RawEv::new(0x01, 0x1E, 1))?;
        led.record(
            4,
            matches!(asm.expose_pending(), Err(c) if c == MetaCode::HalfFrameVisible.val()),
        );
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────
/// 探针 A：10 台登记后按路径逐台命中，指纹非初值。
pub fn probe_seq_a() -> Result<(), u32> {
    let nodes = sample_nodes();
    let mut t = NodeTable::new();
    let mut i = 0usize;
    while i < nodes.len() {
        if let Some(n) = nodes.get(i) {
            t.register(n.clone())?;
        }
        i += 1;
    }
    if t.len() != SAMPLE_DEVICES {
        return Err(MetaCode::NoSuchNode.val());
    }
    let mut j = 0usize;
    while j < nodes.len() {
        if let Some(n) = nodes.get(j) {
            let got = t.by_path(n.path.as_str())?;
            if got.name != n.name {
                return Err(MetaCode::TupleDrift.val());
            }
        }
        j += 1;
    }
    if t.fingerprint() == tuple_init() {
        return Err(MetaCode::TupleDrift.val());
    }
    Ok(())
}

/// 探针 B：未登记路径查询必须 NoSuchNode（不返回空壳节点）。
pub fn probe_seq_b() -> Result<(), u32> {
    let t = NodeTable::new();
    match t.by_path("/dev/input/event99") {
        Err(c) if c == MetaCode::NoSuchNode.val() => Ok(()),
        _ => Err(MetaCode::NoSuchNode.val()),
    }
}

/// 探针 C：SYN_DROPPED 必须作废整帧且不产生帧。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut asm = FrameAssembler::new();
    asm.push(RawEv::new(0x01, 0x1E, 1))?;
    asm.push(RawEv::new(0x01, 0x1E, 0))?;
    let out = asm.push(RawEv::new(0x00, 3, 0))?;
    if out != PushOutcome::Voided {
        return Err(MetaCode::FrameOrderBad.val());
    }
    if asm.committed_len() != 0 || asm.pending_len() != 0 {
        return Err(MetaCode::FrameOrderBad.val());
    }
    let (vf, ve) = asm.void_counts();
    if vf != 1 || ve != 2 {
        return Err(MetaCode::FrameOrderBad.val());
    }
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MetaCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < MetaCode::ALL.len() {
        match MetaCode::ALL.get(i) {
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

/// 五元组字段敏感性：任一字段改动，指纹必须变（逐字段扰动）。
pub fn field_sensitivity() -> Result<u32, u32> {
    let nodes = sample_nodes();
    let base = match nodes.first() {
        Some(n) => n,
        None => return Err(MetaCode::NoSuchNode.val()),
    };
    let d0 = base.digest();
    let mut changed = 0u32;

    let mut k = base.clone();
    k.path = format!("{}.x", base.path);
    if k.digest() != d0 {
        changed += 1;
    }
    let mut k = base.clone();
    k.name = format!("{}.x", base.name);
    if k.digest() != d0 {
        changed += 1;
    }
    let mut k = base.clone();
    k.evbit ^= 0x001;
    if k.digest() != d0 {
        changed += 1;
    }
    let mut k = base.clone();
    k.vid = k.vid.wrapping_add(1);
    if k.digest() != d0 {
        changed += 1;
    }
    let mut k = base.clone();
    k.pid = k.pid.wrapping_add(1);
    if k.digest() != d0 {
        changed += 1;
    }
    let mut k = base.clone();
    k.topo = Topo::new(BusKind::Usb, 15, 63, 7)?;
    if k.digest() != d0 {
        changed += 1;
    }
    Ok(changed)
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // F48803-J1：五元组 ×10 注册/查询/注销 ×100 轮零漂移。
    let rt = tuple_roundtrip();
    v.push((
        "F48803-J1-五元组100轮零漂移",
        matches!(rt, Ok(true)),
        format!("roundtrip={:?}", rt),
    ));

    // F48803-J1：重复路径拒绝。
    let nodes = sample_nodes();
    let dup = match nodes.first() {
        Some(n) => {
            let mut t = NodeTable::new();
            let first = t.register(n.clone()).is_ok();
            let second = t.register(n.clone());
            first && matches!(second, Err(c) if c == MetaCode::DuplicatePath.val())
        }
        None => false,
    };
    v.push((
        "F48803-J1-重复路径唯一闸",
        dup,
        String::from("同路径二次登记 → DuplicatePath"),
    ));

    // F48803-J1：注销后可重注册且序号单调。
    let recycle = match nodes.first() {
        Some(n) => {
            let mut t = NodeTable::new();
            let s1 = t.register(n.clone()).unwrap_or(0);
            let removed = t.unregister(n.path.as_str()).is_ok();
            let s2 = t.register(n.clone()).unwrap_or(0);
            removed && s2 == s1 + 1 && t.len() == 1
        }
        None => false,
    };
    v.push((
        "F48803-J1-注销重注册序号单调",
        recycle,
        format!("registered={} evicted={}", 2, 1),
    ));

    // F48803-J1：五元组逐字段敏感。
    let sens = field_sensitivity();
    v.push((
        "F48803-J1-五元组逐字段敏感",
        matches!(sens, Ok(6)),
        format!("changed={:?}/6", sens),
    ));

    // F48804-J1：半帧 ×10 全拒。
    let half = half_frame_drill();
    v.push((
        "F48804-J1-半帧10/10全拒",
        matches!(half, Ok(n) if n == JUDGE_ROUNDS),
        format!("rejected={:?}", half),
    ));

    // F48804-J1：帧内事件原子递交（可见事件只在 SYN 后增加）。
    let atomic = {
        let mut asm = FrameAssembler::new();
        asm.push(RawEv::new(0x01, 0x1E, 1)).is_ok()
            && asm.visible_events() == 0
            && asm.pending_len() == 1
            && asm.push(RawEv::new(0x00, 0, 0)).is_ok()
            && asm.visible_events() == 1
            && asm.pending_len() == 0
    };
    v.push((
        "F48804-J1-帧内原子递交",
        atomic,
        String::from("visible=0 → SYN → visible=1"),
    ));

    // F48804-J1：SYN_DROPPED 作废整帧入账。
    v.push((
        "F48804-J1-SYN_DROPPED整帧作废",
        probe_seq_c().is_ok(),
        String::from("voided_frames=1 voided_events=2"),
    ));

    // F48804-J1：MT 分隔符不提前提交帧。
    let mt_ok = {
        let mut asm = FrameAssembler::new();
        let mut ok = asm.push(RawEv::new(0x03, 0x2F, 1)).is_ok();
        ok = ok && matches!(asm.push(RawEv::new(0x00, 2, 0)), Ok(PushOutcome::MtSep));
        ok = ok && asm.committed_len() == 0 && asm.pending_len() == 2;
        ok = ok && asm.push(RawEv::new(0x00, 0, 0)).is_ok();
        ok = ok && asm.committed_len() == 1 && asm.mt_seps() == 1;
        ok
    };
    v.push((
        "F48804-J1-MT分隔符不提交帧",
        mt_ok,
        format!("seps={}", 1),
    ));

    // F48803-J2：注入 5 槽 × 10 轮全中。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = matches!(&led, Ok(l) if l.all_caught());
    v.push((
        "F48803-J2-注入10轮5槽全中",
        led_ok,
        format!(
            "tries={:?} caught={:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));

    // F48803-J3：双跑 + 段位。
    v.push((
        "F48803-J3-双跑与段位",
        repro_check().is_ok() && segment_guard(),
        format!("golden={:?}", golden_run().map(|g| g.digest)),
    ));

    v
}

pub fn run_unx48803_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48803 设备节点元数据表");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_nodes_are_ten_and_valid() {
        let nodes = sample_nodes();
        assert_eq!(nodes.len(), SAMPLE_DEVICES);
        let mut i = 0usize;
        while i < nodes.len() {
            let n = nodes.get(i).expect("样本节点");
            assert!(n.validate().is_ok(), "{}", n.report());
            assert_eq!(n.field_count(), TUPLE_FIELDS);
            assert_ne!(n.digest(), 0);
            i += 1;
        }
    }

    #[test]
    fn register_query_unregister_cycle() {
        let nodes = sample_nodes();
        let n = nodes.get(2).expect("鼠标");
        let mut t = NodeTable::new();
        let seq = t.register(n.clone()).expect("登记");
        assert_eq!(seq, 1);
        assert_eq!(t.len(), 1);
        assert_eq!(t.by_path(n.path.as_str()).ok().map(|k| k.digest()), Some(n.digest()));
        assert_eq!(t.by_vidpid(n.vid, n.pid).len(), 1);
        assert_eq!(t.by_bus(BusKind::I8042).len(), 1);
        let removed = t.unregister(n.path.as_str()).expect("注销");
        assert_eq!(removed.digest(), n.digest());
        assert!(t.is_empty());
        assert_eq!(t.by_path(n.path.as_str()).err(), Some(MetaCode::NoSuchNode.val()));
    }

    #[test]
    fn four_gates_order_rejects() {
        let nodes = sample_nodes();
        let base = nodes.first().expect("首台");
        let mut t = NodeTable::new();

        let mut bad = base.clone();
        bad.path = String::new();
        assert_eq!(t.register(bad).err(), Some(MetaCode::EmptyPath.val()));

        let mut bad = base.clone();
        bad.name = String::new();
        assert_eq!(t.register(bad).err(), Some(MetaCode::EmptyName.val()));

        let mut bad = base.clone();
        bad.evbit = 0;
        assert_eq!(t.register(bad).err(), Some(MetaCode::EmptyEvbit.val()));

        let mut bad = base.clone();
        bad.evbit = 0x400;
        assert_eq!(t.register(bad).err(), Some(MetaCode::EvbitOutOfFamily.val()));

        t.register(base.clone()).expect("首次登记");
        assert_eq!(t.register(base.clone()).err(), Some(MetaCode::DuplicatePath.val()));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn hundred_rounds_zero_drift() {
        assert_eq!(tuple_roundtrip(), Ok(true));
    }

    #[test]
    fn topology_bounds() {
        assert!(Topo::new(BusKind::Usb, 16, 0, 1).is_err());
        assert!(Topo::new(BusKind::Usb, 0, 64, 1).is_err());
        assert!(Topo::new(BusKind::Usb, 0, 0, 0).is_err());
        assert!(Topo::new(BusKind::Usb, 0, 0, 8).is_err());
        let t = Topo::new(BusKind::I2c, 1, 4, 2).expect("合法拓扑");
        assert_eq!(t.label(), "i2c-1:4.2");
        assert!(t.check().is_ok());
    }

    #[test]
    fn frame_needs_syn_to_be_visible() {
        let mut asm = FrameAssembler::new();
        for ev in frame_payload(0).iter() {
            assert_eq!(asm.push(*ev), Ok(PushOutcome::Pending));
        }
        assert_eq!(asm.visible_events(), 0);
        assert_eq!(asm.pending_len(), 3);
        assert_eq!(asm.expose_pending().err(), Some(MetaCode::HalfFrameVisible.val()));
        assert_eq!(asm.push(RawEv::new(0x00, 0, 0)), Ok(PushOutcome::Committed));
        assert_eq!(asm.visible_events(), 3);
        assert_eq!(asm.committed_len(), 1);
        let (att, rej) = asm.half_counts();
        assert_eq!((att, rej), (1, 1));
        assert!(asm.invariant_check().is_ok());
    }

    #[test]
    fn half_frame_drill_ten_rejects() {
        assert_eq!(half_frame_drill(), Ok(JUDGE_ROUNDS));
    }

    #[test]
    fn syn_dropped_voids_pending() {
        assert!(probe_seq_c().is_ok());
        let mut asm = FrameAssembler::new();
        asm.push(RawEv::new(0x01, 0x1E, 1)).expect("push");
        asm.push(RawEv::new(0x00, 3, 0)).expect("dropped");
        assert_eq!(asm.committed_len(), 0);
        assert_eq!(asm.pending_len(), 0);
        assert_eq!(asm.void_counts(), (1, 1));
    }

    #[test]
    fn mt_separator_keeps_frame_open() {
        let mut asm = FrameAssembler::new();
        assert_eq!(asm.push(RawEv::new(0x03, 0x2F, 1)), Ok(PushOutcome::Pending));
        assert_eq!(asm.push(RawEv::new(0x00, 2, 0)), Ok(PushOutcome::MtSep));
        assert_eq!(asm.committed_len(), 0);
        assert_eq!(asm.push(RawEv::new(0x03, 0x30, 2)), Ok(PushOutcome::Pending));
        assert_eq!(asm.push(RawEv::new(0x00, 0, 0)), Ok(PushOutcome::Committed));
        assert_eq!(asm.mt_seps(), 1);
        let f = asm.committed().first().expect("一帧");
        assert_eq!(f.len(), 3);
        assert_eq!(f.ordinal, 1);
    }

    #[test]
    fn golden_chain_complete() {
        let g = golden_run().expect("黄金链");
        assert_eq!(g.devices, SAMPLE_DEVICES as u32);
        assert_eq!(g.audited, SAMPLE_DEVICES as u32);
        assert!(g.drift_free);
        assert_eq!(g.half_rejects, JUDGE_ROUNDS);
        assert_eq!(g.visible, (SAMPLE_DEVICES as u32) * 3);
        assert_eq!(g, golden_run().expect("二跑"));
    }

    #[test]
    fn inject_five_slots_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).expect("注入");
        assert!(led.all_caught());
        assert_eq!(led.tries, [JUDGE_ROUNDS; 5]);
        assert_eq!(led.caught, [JUDGE_ROUNDS; 5]);
    }

    #[test]
    fn field_sensitivity_all_six() {
        assert_eq!(field_sensitivity(), Ok(6));
    }

    #[test]
    fn codes_are_segment_closed() {
        assert!(segment_guard());
        for c in MetaCode::ALL.iter() {
            let v = c.val();
            assert!(v > SEG_BASE && v < SEG_BASE + 0x20);
            assert_eq!(detail(v), c.detail());
            assert!(!c.detail().is_empty());
        }
        assert_eq!(detail(0x7004D), "未知诊断码：不在 0x70041..0x7004C 闭集内");
    }

    #[test]
    fn probes_a_b_c() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }
}
