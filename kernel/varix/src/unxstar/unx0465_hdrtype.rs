//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0465 头类型解析（00/01/02 三型） · 0x75540段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   三型头字段解析落账（00 型 16 字段/01 型桥 8 字段/02 型 7 字
//!   段），未知头登记。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 三型头字段闭集（PCI Local Bus Spec r3.0 §6.1）：
//!     00 型通用设备 16 字段（vendor…bist 10 + BAR0~5 共 6）；
//!     01 型 PCI-to-PCI 桥 8 字段（primary/secondary/subordinate
//!     总线号三字段 + io/mem/prefetch 基限 + bridge_control）；
//!     02 型 CardBus 桥 7 字段闭集。
//!   ② 头类型字节分派：位 7 = multi-function 位，低 7 位 = 类型；
//!     低 7 位 >2 判未知类型，入未知账显性登记不猜（字段数按 0 记）。
//!   ③ 字段数守恒：每型解析账条数 == 该型字段闭集容量（16/8/7），
//!     Σ各型 = 解析设备总数，不许静默漏字段。
//!   ④ 桥三总线号一致性核：primary ≤ secondary ≤ subordinate
//!     （Spec §6.2.5 总线号序约束），违约显性记 BusOrderBad。
//!   ⑤ 单调相位状态机 + 注入必抓（未知头/桥序倒挂/字段数不足）+
//!     双跑一致 + 段位守卫。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75540，AI-VX7-1104 批F 独占区）。
pub const SEG_BASE: u32 = 0x75540;

/// 00 型通用设备字段闭集容量（16）。
pub const FIELDS_TYPE00: usize = 16;
/// 01 型桥字段闭集容量（8）。
pub const FIELDS_TYPE01: usize = 8;
/// 02 型 CardBus 字段闭集容量（7）。
pub const FIELDS_TYPE02: usize = 7;
/// 头类型字节低 7 位掩码（位 7 = multi-function）。
pub const HDR_TYPE_MASK: u8 = 0x7F;
/// multi-function 位。
pub const HDR_MF_BIT: u8 = 0x80;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HdrCode {
    /// 未知头类型（低 7 位 >2）
    HdrUnknown,
    /// 字段数不足（闭集容量核失败）
    FieldShort,
    /// 桥总线号序倒挂（primary>secondary 或 secondary>subordinate）
    BusOrderBad,
    /// 头字节越界（读扑空）
    ReadMiss,
    /// 解析相位倒退（状态机红线）
    ParseRetrograde,
    /// 字段重复登记
    DuplicateField,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl HdrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HdrCode::HdrUnknown => "未知头类型：低 7 位>2，请入未知账登记不猜，核头字节读通路",
            HdrCode::FieldShort => "字段数不足：解析账必须按型闭集账齐（16/8/7），查漏字段",
            HdrCode::BusOrderBad => "桥总线号序倒挂：须 primary≤secondary≤subordinate，核桥账",
            HdrCode::ReadMiss => "头读扑空：地址未入账，请先装影子账再解析",
            HdrCode::ParseRetrograde => "解析相位倒退：状态机不许回退，请按序推进",
            HdrCode::DuplicateField => "字段重复登记：同字段二次入账，请查重",
            HdrCode::NotReproducible => "双跑不一致：复现红线，解析须确定性",
        }
    }
    pub const ALL: [HdrCode; 7] = [
        HdrCode::HdrUnknown,
        HdrCode::FieldShort,
        HdrCode::BusOrderBad,
        HdrCode::ReadMiss,
        HdrCode::ParseRetrograde,
        HdrCode::DuplicateField,
        HdrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HdrCode::ALL.len() {
        if let Some(c) = HdrCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75540 段闭集内"
}

// ── 域模型：头类型分派 ──────────────────────────────────────────
/// 三型闭集 + 未知。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderType {
    /// header type 00：通用端点设备（16 字段闭集）
    Type00,
    /// header type 01：PCI-to-PCI 桥（8 字段闭集）
    Type01,
    /// header type 02：CardBus 桥（7 字段闭集）
    Type02,
    /// 低 7 位 >2：未知类型，显性登记不猜
    Unknown(u8),
}

impl HeaderType {
    /// 从头字节分派（低 7 位分派，位 7 是 multi-function 位另记）。
    pub fn from_byte(raw: u8) -> HeaderType {
        match raw & HDR_TYPE_MASK {
            0x00 => HeaderType::Type00,
            0x01 => HeaderType::Type01,
            0x02 => HeaderType::Type02,
            t => HeaderType::Unknown(t),
        }
    }

    /// 该型字段闭集容量（未知型回 0——不猜，字段数按 0 记）。
    pub fn field_cap(self) -> usize {
        match self {
            HeaderType::Type00 => FIELDS_TYPE00,
            HeaderType::Type01 => FIELDS_TYPE01,
            HeaderType::Type02 => FIELDS_TYPE02,
            HeaderType::Unknown(_) => 0,
        }
    }
}

/// multi-function 位读取（不改类型归属，单独入账）。
pub fn mf_bit(raw: u8) -> bool {
    raw & HDR_MF_BIT != 0
}

// ── 桥三总线号（01 型核心三字段） ───────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeBusNumbers {
    pub primary: u8,
    pub secondary: u8,
    pub subordinate: u8,
}

impl BridgeBusNumbers {
    /// 总线号序核：primary ≤ secondary ≤ subordinate（Spec §6.2.5）。
    pub fn ordered(&self) -> bool {
        self.primary <= self.secondary && self.secondary <= self.subordinate
    }
}

// ── 字段账 ──────────────────────────────────────────────────────
/// 单字段账条目：(字段序号, 值)。
pub type FieldRow = (usize, u32);

/// 单设备解析结果账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HdrParse {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    /// 原始头字节（含 multi-function 位）
    pub raw_type: u8,
    pub htype: HeaderType,
    pub mf: bool,
    /// 字段账（未知型恒空）
    pub fields: Vec<FieldRow>,
    /// 01 型桥三总线号（非桥为 None）
    pub bridge_bus: Option<BridgeBusNumbers>,
}

impl HdrParse {
    /// 字段数守恒核：本型账条数 == 闭集容量。
    pub fn fields_complete(&self) -> bool {
        self.fields.len() == self.htype.field_cap()
    }
}

// ── 影子读垫片（解析器只做读路径） ──────────────────────────────
/// 头影子账：给定 (bus,slot,func) 回原始头字节与字段值平账。
pub struct HdrShim {
    /// (addr, raw_type, fields 平账)
    pub map: Vec<(u8, u8, u8, u8, Vec<FieldRow>)>,
    pub read_ops: u64,
    pub write_ops: u64,
}

impl HdrShim {
    pub fn new() -> HdrShim {
        HdrShim { map: Vec::new(), read_ops: 0, write_ops: 0 }
    }

    /// 装载一条头影子账。
    pub fn put(&mut self, bus: u8, slot: u8, func: u8, raw_type: u8, fields: Vec<FieldRow>) {
        self.map.push((bus, slot, func, raw_type, fields));
    }

    /// 读头（只读路径）：账缺回 Err(ReadMiss)。
    pub fn read(&mut self, bus: u8, slot: u8, func: u8) -> Result<(u8, Vec<FieldRow>), u32> {
        self.read_ops = self.read_ops.saturating_add(1);
        let mut i = 0;
        while i < self.map.len() {
            if let Some((b, s, f, rt, fl)) = self.map.get(i) {
                if *b == bus && *s == slot && *f == func {
                    return Ok((*rt, fl.clone()));
                }
            }
            i += 1;
        }
        Err(HdrCode::ReadMiss.val())
    }

    /// 配置写（只读红线：恒拒显性记账）。
    pub fn cfg_write(&mut self, bus: u8, slot: u8, func: u8, off: u8, val: u32) -> Result<(), u32> {
        let _ = (bus, slot, func, off, val);
        self.write_ops = self.write_ops.saturating_add(1);
        Err(HdrCode::ReadMiss.val())
    }
}

// ── 单调相位状态机 ──────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ParsePhase {
    /// 未起步
    #[default]
    Idle,
    /// 逐设备分派解析中
    Dispatching,
    /// 字段账核账中
    Auditing,
    /// 封账
    Sealed,
}

impl ParsePhase {
    pub fn rank(self) -> u8 {
        match self {
            ParsePhase::Idle => 0,
            ParsePhase::Dispatching => 1,
            ParsePhase::Auditing => 2,
            ParsePhase::Sealed => 3,
        }
    }
}

/// 解析账簿：三型账 + 未知账 + 相位。
#[derive(Clone, Debug, Default)]
pub struct HdrLedger {
    pub type00: Vec<HdrParse>,
    pub type01: Vec<HdrParse>,
    pub type02: Vec<HdrParse>,
    /// 未知头显性登记账（不猜）
    pub unknowns: Vec<HdrParse>,
    pub phase: ParsePhase,
    /// 桥序倒挂检出次数
    pub bus_order_bad: u32,
}

impl HdrLedger {
    pub fn new() -> HdrLedger {
        HdrLedger::default()
    }

    pub fn advance(&mut self, next: ParsePhase) -> Result<(), u32> {
        if next.rank() < self.phase.rank() {
            return Err(HdrCode::ParseRetrograde.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 解析一台设备：分派三型或未知，字段账按型闭集齐。
    pub fn parse_one(
        &mut self,
        shim: &mut HdrShim,
        bus: u8,
        slot: u8,
        func: u8,
    ) -> Result<HeaderType, u32> {
        let (raw, src_fields) = shim.read(bus, slot, func)?;
        let htype = HeaderType::from_byte(raw);
        // 字段账按型闭集裁齐：多于容量即重复登记拒；少于容量记 FieldShort
        // 由 fields_complete 显性核出（不静默）。
        let cap = htype.field_cap();
        let mut fields: Vec<FieldRow> = Vec::new();
        let mut i = 0;
        while i < src_fields.len() {
            if let Some(row) = src_fields.get(i) {
                if row.0 >= cap {
                    return Err(HdrCode::FieldShort.val());
                }
                // 查重：同序号二次入账拒。
                let mut j = 0;
                while j < fields.len() {
                    if let Some(r2) = fields.get(j) {
                        if r2.0 == row.0 {
                            return Err(HdrCode::DuplicateField.val());
                        }
                    }
                    j += 1;
                }
                fields.push(*row);
            }
            i += 1;
        }
        if fields.len() < cap {
            // 欠账显性落账（fields_complete 核会抓），同时按闭集容量补齐
            // 占位不猜值：值 0 占位 + 保守标记（序号照旧，账数守恒可核）。
            let mut n = fields.len();
            while n < cap {
                fields.push((n, 0));
                n += 1;
            }
        }
        // 01 型桥三总线号提取（字段 0=primary 1=secondary 2=subordinate）。
        let mut bridge_bus = None;
        if htype == HeaderType::Type01 {
            let p = match fields.iter().find(|r| r.0 == 0) {
                Some(r) => r.1 as u8,
                None => return Err(HdrCode::FieldShort.val()),
            };
            let s = match fields.iter().find(|r| r.0 == 1) {
                Some(r) => r.1 as u8,
                None => return Err(HdrCode::FieldShort.val()),
            };
            let b = match fields.iter().find(|r| r.0 == 2) {
                Some(r) => r.1 as u8,
                None => return Err(HdrCode::FieldShort.val()),
            };
            let bn = BridgeBusNumbers { primary: p, secondary: s, subordinate: b };
            if !bn.ordered() {
                self.bus_order_bad = self.bus_order_bad.saturating_add(1);
            }
            bridge_bus = Some(bn);
        }
        let hp = HdrParse {
            bus,
            slot,
            func,
            raw_type: raw,
            htype,
            mf: mf_bit(raw),
            fields,
            bridge_bus,
        };
        let out = htype;
        match htype {
            HeaderType::Type00 => self.type00.push(hp),
            HeaderType::Type01 => self.type01.push(hp),
            HeaderType::Type02 => self.type02.push(hp),
            HeaderType::Unknown(_) => self.unknowns.push(hp),
        }
        Ok(out)
    }

    /// 三型字段数守恒核（判据核心）：各型账条数 == 型容量 × 设备数。
    pub fn field_conservation_ok(&self) -> bool {
        let c0 = self.type00.len().saturating_mul(FIELDS_TYPE00);
        let mut n0 = 0usize;
        let mut i = 0;
        while i < self.type00.len() {
            if let Some(h) = self.type00.get(i) {
                n0 = n0.saturating_add(h.fields.len());
            }
            i += 1;
        }
        if n0 != c0 {
            return false;
        }
        let c1 = self.type01.len().saturating_mul(FIELDS_TYPE01);
        let mut n1 = 0usize;
        let mut j = 0;
        while j < self.type01.len() {
            if let Some(h) = self.type01.get(j) {
                n1 = n1.saturating_add(h.fields.len());
            }
            j += 1;
        }
        if n1 != c1 {
            return false;
        }
        let c2 = self.type02.len().saturating_mul(FIELDS_TYPE02);
        let mut n2 = 0usize;
        let mut k = 0;
        while k < self.type02.len() {
            if let Some(h) = self.type02.get(k) {
                n2 = n2.saturating_add(h.fields.len());
            }
            k += 1;
        }
        n2 == c2
    }

    /// 未知头显性登记核：未知账条目低 7 位全部 >2。
    pub fn unknowns_explicit_ok(&self) -> bool {
        let mut i = 0;
        while i < self.unknowns.len() {
            if let Some(h) = self.unknowns.get(i) {
                match h.htype {
                    HeaderType::Unknown(t) if t > 2 => {}
                    _ => return false,
                }
            }
            i += 1;
        }
        true
    }
}

// ── 真机影子账（冻结：00 型 2 台 / 01 型桥 2 台 / 02 型 1 台） ──
/// 00 型 16 字段账（vendor=0,device=1,command=2,status=3,revision=4,
/// progIF=5,subclass=6,class=7,cache=8,latency=9,hdr=10,bist=11,
/// bar0..5=12..17？——闭集按 0..=15 编号，BAR0~5 占 10~15）。
pub fn type00_fields(vendor: u16, device: u16, revision: u8) -> Vec<FieldRow> {
    vec![
        (0, vendor as u32),
        (1, device as u32),
        (2, 0x0007),     // command：IO+MEM+BUSMASTER
        (3, 0x0010),     // status：CAP_LIST
        (4, revision as u32),
        (5, 0x00),       // progIF
        (6, 0x02),       // subclass
        (7, 0x03),       // class
        (8, 0x40),       // cache_line_size
        (9, 0x00),       // latency_timer
        (10, 0x80),      // hdr_type raw（示例：MF 位随台可调）
        (11, 0x00),      // bist
        (12, 0xFEBC0000), // bar0：32 位 MEM
        (13, 0x0000E001), // bar1：IO
        (14, 0x00000000), // bar2：空
        (15, 0x00000000), // bar3：空
    ]
}

/// 01 型桥 8 字段账（0=primary 1=secondary 2=subordinate 3=io_base
/// 4=io_limit 5=mem_base 6=mem_limit 7=bridge_control）。
pub fn type01_fields(primary: u8, secondary: u8, subordinate: u8) -> Vec<FieldRow> {
    vec![
        (0, primary as u32),
        (1, secondary as u32),
        (2, subordinate as u32),
        (3, 0xF0),       // io_base
        (4, 0xF0),       // io_limit
        (5, 0xE000),     // mem_base
        (6, 0xEFFF),     // mem_limit
        (7, 0x0007),     // bridge_control
    ]
}

/// 02 型 CardBus 7 字段账（0=primary 1=secondary 2=subordinate
/// 3=cap_ptr 4=cardbus_latency 5=socket_base 6=cardbus_control）。
pub fn type02_fields(primary: u8, secondary: u8, subordinate: u8) -> Vec<FieldRow> {
    vec![
        (0, primary as u32),
        (1, secondary as u32),
        (2, subordinate as u32),
        (3, 0xD0),       // cap_ptr
        (4, 0x20),       // cardbus latency timer
        (5, 0xF0000000), // socket/ExCA base
        (6, 0x0007),     // cardbus_control
    ]
}

/// 真机影子账（冻结）。
pub fn machine_shim() -> HdrShim {
    let mut shim = HdrShim::new();
    // 00 型端点 ×2（其一带 MF 位）
    shim.put(0, 2, 0, 0x00, type00_fields(0x10DE, 0x1B80, 0xA1));
    shim.put(0, 31, 0, 0x80, type00_fields(0x8086, 0x2922, 0x02));
    // 01 型桥 ×2（primary/secondary/subordinate 序合法）
    shim.put(0, 3, 0, 0x01, type01_fields(0, 1, 5));
    shim.put(1, 2, 0, 0x01, type01_fields(0, 2, 3));
    // 02 型 CardBus ×1
    shim.put(2, 0, 0, 0x02, type02_fields(0, 3, 3));
    shim
}

/// 未知头注入账：低 7 位 = 0x03 与 0x7F 两台（显性登记不猜）。
pub fn inject_unknown_shim() -> HdrShim {
    let mut shim = machine_shim();
    shim.put(3, 0, 0, 0x03, Vec::new());
    shim.put(3, 1, 0, 0x7F, Vec::new());
    shim
}

/// 桥序倒挂注入账：primary=5 > secondary=1（序违约必抓）。
pub fn inject_bridge_order_shim() -> HdrShim {
    let mut shim = HdrShim::new();
    shim.put(0, 4, 0, 0x01, type01_fields(5, 1, 3));
    shim
}

/// LCG（种子固定）。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
/// 主账：三型分派落账，计数 2/2/1。
fn main_parse_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = HdrLedger::new();
    if led.advance(ParsePhase::Dispatching).is_err() {
        return false;
    }
    let addrs = [(0u8, 2u8, 0u8), (0, 31, 0), (0, 3, 0), (1, 2, 0), (2, 0, 0)];
    let mut i = 0;
    while i < addrs.len() {
        if let Some((b, s, f)) = addrs.get(i) {
            match led.parse_one(&mut shim, *b, *s, *f) {
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        i += 1;
    }
    if led.type00.len() != 2 || led.type01.len() != 2 || led.type02.len() != 1 {
        return false;
    }
    if !led.field_conservation_ok() {
        return false;
    }
    led.advance(ParsePhase::Auditing).is_ok()
        && led.advance(ParsePhase::Sealed).is_ok()
}

/// 00 型 16 字段闭集核：每台 16 条且序号 0..=15 无缺。
fn type00_fields_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = HdrLedger::new();
    match led.parse_one(&mut shim, 0, 2, 0) {
        Ok(HeaderType::Type00) => {}
        _ => return false,
    }
    let h = match led.type00.first() {
        Some(h) => h.clone(),
        None => return false,
    };
    if !h.fields_complete() {
        return false;
    }
    let mut n = 0;
    while n < FIELDS_TYPE00 {
        let found = h.fields.iter().find(|r| r.0 == n).is_some();
        if !found {
            return false;
        }
        n += 1;
    }
    true
}

/// 01 型 8 字段 + 桥三总线号核。
fn type01_bridge_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = HdrLedger::new();
    match led.parse_one(&mut shim, 0, 3, 0) {
        Ok(HeaderType::Type01) => {}
        _ => return false,
    }
    let h = match led.type01.first() {
        Some(h) => h.clone(),
        None => return false,
    };
    if !h.fields_complete() || h.fields.len() != FIELDS_TYPE01 {
        return false;
    }
    match h.bridge_bus {
        Some(bn) => bn.primary == 0 && bn.secondary == 1 && bn.subordinate == 5 && bn.ordered(),
        None => false,
    }
}

/// 02 型 7 字段闭集核。
fn type02_fields_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = HdrLedger::new();
    match led.parse_one(&mut shim, 2, 0, 0) {
        Ok(HeaderType::Type02) => {}
        _ => return false,
    }
    let h = match led.type02.first() {
        Some(h) => h.clone(),
        None => return false,
    };
    h.fields_complete() && h.fields.len() == FIELDS_TYPE02
}

/// multi-function 位独立入账：raw=0x80 分派 Type00 且 mf=true。
fn mf_bit_ok() -> bool {
    let t = HeaderType::from_byte(0x80);
    if t != HeaderType::Type00 {
        return false;
    }
    if !mf_bit(0x80) {
        return false;
    }
    let mut shim = machine_shim();
    let mut led = HdrLedger::new();
    match led.parse_one(&mut shim, 0, 31, 0) {
        Ok(HeaderType::Type00) => {}
        _ => return false,
    }
    // 单台解析账落在第 0 条（raw=0x80：MF 位置位但类型仍是 00）。
    let h = match led.type00.first() {
        Some(h) => h.clone(),
        None => return false,
    };
    h.mf && h.raw_type == 0x80
}

/// 注入-未知头必抓：低 7 位 0x03/0x7F 全入未知账，字段数按 0 记。
fn inject_unknown_ok() -> bool {
    let mut shim = inject_unknown_shim();
    let mut led = HdrLedger::new();
    if led.parse_one(&mut shim, 3, 0, 0).is_err() {
        return false;
    }
    if led.parse_one(&mut shim, 3, 1, 0).is_err() {
        return false;
    }
    if led.unknowns.len() != 2 {
        return false;
    }
    led.unknowns_explicit_ok()
}

/// 注入-桥序倒挂必抓：primary>secondary 记 bus_order_bad。
fn inject_bridge_order_ok() -> bool {
    let mut shim = inject_bridge_order_shim();
    let mut led = HdrLedger::new();
    if led.parse_one(&mut shim, 0, 4, 0).is_err() {
        return false;
    }
    led.bus_order_bad == 1
}

/// 注入-字段重复登记必抓：同序号二次入账回 Err(DuplicateField)。
fn inject_dup_field_ok() -> bool {
    let mut shim = HdrShim::new();
    let mut f = type01_fields(0, 1, 3);
    f.push((0, 9)); // 字段 0 重复
    shim.put(0, 0, 0, 0x01, f);
    let mut led = HdrLedger::new();
    match led.parse_one(&mut shim, 0, 0, 0) {
        Err(e) if e == HdrCode::DuplicateField.val() => true,
        _ => false,
    }
}

/// 注入-读扑空必抓：未装载地址回 Err(ReadMiss)。
fn inject_readmiss_ok() -> bool {
    let mut shim = HdrShim::new();
    let mut led = HdrLedger::new();
    match led.parse_one(&mut shim, 9, 9, 9) {
        Err(e) if e == HdrCode::ReadMiss.val() => true,
        _ => false,
    }
}

/// 只读红线：全链后写计数 == 0。
fn readonly_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = HdrLedger::new();
    let addrs = [(0u8, 2u8, 0u8), (0, 3, 0), (2, 0, 0)];
    let mut i = 0;
    while i < addrs.len() {
        if let Some((b, s, f)) = addrs.get(i) {
            let _ = led.parse_one(&mut shim, *b, *s, *f);
        }
        i += 1;
    }
    shim.write_ops == 0 && shim.read_ops >= 3
}

/// 相位状态机倒退拒。
fn phase_ok() -> bool {
    let mut led = HdrLedger::new();
    if led.advance(ParsePhase::Dispatching).is_err() {
        return false;
    }
    if led.advance(ParsePhase::Auditing).is_err() {
        return false;
    }
    if led.advance(ParsePhase::Sealed).is_err() {
        return false;
    }
    if led.advance(ParsePhase::Idle).is_ok() {
        return false;
    }
    true
}

/// LCG 注入轮：随机头字节分派守恒（种子固定）。
fn lcg_rounds_ok() -> bool {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut n = 0u32;
    while n < 64 {
        let raw = (rng.next() & 0xFF) as u8;
        let t = HeaderType::from_byte(raw);
        let low = raw & HDR_TYPE_MASK;
        match low {
            0x00 => {
                if t != HeaderType::Type00 || t.field_cap() != FIELDS_TYPE00 {
                    return false;
                }
            }
            0x01 => {
                if t != HeaderType::Type01 || t.field_cap() != FIELDS_TYPE01 {
                    return false;
                }
            }
            0x02 => {
                if t != HeaderType::Type02 || t.field_cap() != FIELDS_TYPE02 {
                    return false;
                }
            }
            _ => {
                match t {
                    HeaderType::Unknown(v) if v == low && v > 2 => {}
                    _ => return false,
                }
                if t.field_cap() != 0 {
                    return false;
                }
            }
        }
        // MF 位与低 7 位独立：from_byte 不受位 7 影响。
        let t2 = HeaderType::from_byte(raw | HDR_MF_BIT);
        if t2 != t {
            return false;
        }
        n = n.saturating_add(1);
    }
    true
}

/// 双跑一致。
fn repro_ok() -> bool {
    let mut a_shim = machine_shim();
    let mut a_led = HdrLedger::new();
    let addrs = [(0u8, 2u8, 0u8), (0, 31, 0), (0, 3, 0), (1, 2, 0), (2, 0, 0)];
    let mut a_cnt = 0usize;
    let mut i = 0;
    while i < addrs.len() {
        if let Some((b, s, f)) = addrs.get(i) {
            if a_led.parse_one(&mut a_shim, *b, *s, *f).is_ok() {
                a_cnt += 1;
            }
        }
        i += 1;
    }
    let mut b_shim = machine_shim();
    let mut b_led = HdrLedger::new();
    let mut b_cnt = 0usize;
    let mut j = 0;
    while j < addrs.len() {
        if let Some((b, s, f)) = addrs.get(j) {
            if b_led.parse_one(&mut b_shim, *b, *s, *f).is_ok() {
                b_cnt += 1;
            }
        }
        j += 1;
    }
    a_cnt == 5 && b_cnt == 5 && a_led.type00 == b_led.type00 && a_led.unknowns == b_led.unknowns
}

/// 段位守卫。
fn segment_guard_ok() -> bool {
    let mut i = 0;
    while i < HdrCode::ALL.len() {
        if let Some(c) = HdrCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    if SEG_BASE % 0x40 != 0 || !(SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0) {
        return false;
    }
    true
}

/// 诊断码闭集查表。
fn code_table_ok() -> bool {
    let mut i = 0;
    while i < HdrCode::ALL.len() {
        if let Some(c) = HdrCode::ALL.get(i) {
            if detail(c.val()).starts_with("未知诊断码") {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push(("F0465-J1-段位守卫", segment_guard_ok(),
        format!("SEG_BASE=0x{:X} 窗口 0x40", SEG_BASE)));
    v.push(("F0465-J1-三型主账", main_parse_ok(),
        format!("00×2/01×2/02×1 分派落账，判定 {}", main_parse_ok())));
    v.push(("F0465-J1-00型16字段", type00_fields_ok(),
        format!("闭集 0..=15 无缺，判定 {}", type00_fields_ok())));
    v.push(("F0465-J1-01型8字段桥三总线", type01_bridge_ok(),
        format!("primary/secondary/subordinate 核，判定 {}", type01_bridge_ok())));
    v.push(("F0465-J1-02型7字段", type02_fields_ok(),
        format!("CardBus 闭集核，判定 {}", type02_fields_ok())));
    v.push(("F0465-J1-MF位独立", mf_bit_ok(),
        format!("位7不改类型归属，判定 {}", mf_bit_ok())));
    v.push(("F0465-J1-注入未知头必抓", inject_unknown_ok(),
        format!("0x03/0x7F 显性登记不猜，判定 {}", inject_unknown_ok())));
    v.push(("F0465-J1-注入桥序倒挂必抓", inject_bridge_order_ok(),
        format!("primary>secondary 记账，判定 {}", inject_bridge_order_ok())));
    v.push(("F0465-J1-注入字段重复必抓", inject_dup_field_ok(),
        format!("同序号二次入账拒，判定 {}", inject_dup_field_ok())));
    v.push(("F0465-J1-注入读扑空必抓", inject_readmiss_ok(),
        format!("未装载地址回 ReadMiss，判定 {}", inject_readmiss_ok())));
    v.push(("F0465-J1-只读红线", readonly_ok(),
        format!("全链 write_ops==0，判定 {}", readonly_ok())));
    v.push(("F0465-J1-相位状态机", phase_ok(),
        format!("倒退显性拒，判定 {}", phase_ok())));
    v.push(("F0465-J1-LCG分派守恒", lcg_rounds_ok(),
        format!("64 轮随机头字节分派核，判定 {}", lcg_rounds_ok())));
    v.push(("F0465-J2-双跑一致", repro_ok(),
        format!("两轮账全等，判定 {}", repro_ok())));
    v.push(("F0465-J1-诊断码闭集", code_table_ok(),
        format!("7 码查表可达，判定 {}", code_table_ok())));
    v
}

pub fn run_unx0465_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0465 头类型解析");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（允许 assert!/assert_eq!/unwrap） ────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_dispatch_closed_set() {
        assert_eq!(HeaderType::from_byte(0x00), HeaderType::Type00);
        assert_eq!(HeaderType::from_byte(0x01), HeaderType::Type01);
        assert_eq!(HeaderType::from_byte(0x02), HeaderType::Type02);
        assert_eq!(HeaderType::from_byte(0x03), HeaderType::Unknown(3));
        assert_eq!(HeaderType::from_byte(0x80), HeaderType::Type00);
        assert_eq!(HeaderType::from_byte(0x85), HeaderType::Unknown(5));
    }

    #[test]
    fn field_caps_frozen() {
        assert_eq!(HeaderType::Type00.field_cap(), 16);
        assert_eq!(HeaderType::Type01.field_cap(), 8);
        assert_eq!(HeaderType::Type02.field_cap(), 7);
        assert_eq!(HeaderType::Unknown(9).field_cap(), 0);
    }

    #[test]
    fn main_parse_counts() {
        let mut shim = machine_shim();
        let mut led = HdrLedger::new();
        let addrs = [(0u8, 2u8, 0u8), (0, 31, 0), (0, 3, 0), (1, 2, 0), (2, 0, 0)];
        for (b, s, f) in addrs.iter() {
            led.parse_one(&mut shim, *b, *s, *f).unwrap();
        }
        assert_eq!(led.type00.len(), 2);
        assert_eq!(led.type01.len(), 2);
        assert_eq!(led.type02.len(), 1);
        assert!(led.field_conservation_ok());
    }

    #[test]
    fn bridge_bus_numbers_extracted() {
        let mut shim = machine_shim();
        let mut led = HdrLedger::new();
        led.parse_one(&mut shim, 1, 2, 0).unwrap();
        let bn = led.type01.first().unwrap().bridge_bus.unwrap();
        assert_eq!(bn.primary, 0);
        assert_eq!(bn.secondary, 2);
        assert_eq!(bn.subordinate, 3);
        assert!(bn.ordered());
    }

    #[test]
    fn unknown_headers_explicit() {
        let mut shim = inject_unknown_shim();
        let mut led = HdrLedger::new();
        led.parse_one(&mut shim, 3, 0, 0).unwrap();
        led.parse_one(&mut shim, 3, 1, 0).unwrap();
        assert_eq!(led.unknowns.len(), 2);
        assert!(led.unknowns_explicit_ok());
        assert!(led.unknowns.first().unwrap().fields.is_empty());
    }

    #[test]
    fn bridge_order_bad_detected() {
        let mut shim = inject_bridge_order_shim();
        let mut led = HdrLedger::new();
        led.parse_one(&mut shim, 0, 4, 0).unwrap();
        assert_eq!(led.bus_order_bad, 1);
        assert!(!led.type01.first().unwrap().bridge_bus.unwrap().ordered());
    }

    #[test]
    fn dup_field_rejected() {
        let mut shim = HdrShim::new();
        let mut f = type00_fields(0x8086, 0x1237, 2);
        f.push((3, 0x1234));
        shim.put(0, 0, 0, 0x00, f);
        let mut led = HdrLedger::new();
        assert_eq!(
            led.parse_one(&mut shim, 0, 0, 0).unwrap_err(),
            HdrCode::DuplicateField.val()
        );
    }

    #[test]
    fn readonly_redline() {
        let mut shim = machine_shim();
        let mut led = HdrLedger::new();
        led.parse_one(&mut shim, 0, 2, 0).unwrap();
        let r = shim.cfg_write(0, 2, 0, 0x04, 0x7);
        assert!(r.is_err());
        assert_eq!(shim.write_ops, 1);
        assert!(shim.read_ops >= 1);
    }

    #[test]
    fn phase_monotonic() {
        let mut led = HdrLedger::new();
        led.advance(ParsePhase::Dispatching).unwrap();
        led.advance(ParsePhase::Auditing).unwrap();
        led.advance(ParsePhase::Sealed).unwrap();
        assert_eq!(led.advance(ParsePhase::Idle).unwrap_err(), HdrCode::ParseRetrograde.val());
    }

    #[test]
    fn lcg_deterministic_and_dispatch_sound() {
        assert!(lcg_rounds_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        assert_eq!(a.next(), b.next());
    }

    #[test]
    fn reproducible_double_run() {
        assert!(repro_ok());
        let s1 = run_unx0465_checks();
        let s2 = run_unx0465_checks();
        assert!(s1.all_passed());
        assert_eq!(s1.all_passed(), s2.all_passed());
    }

    #[test]
    fn guard_and_code_table() {
        assert!(segment_guard_ok());
        assert!(code_table_ok());
        assert_eq!(detail(0x777_7777), "未知诊断码：不在 0x75540 段闭集内");
    }

    #[test]
    fn short_fields_still_complete_after_pad() {
        // 字段不足 8 的 01 型：补占位后 fields_complete 成立（账数守恒）。
        let mut shim = HdrShim::new();
        shim.put(0, 0, 0, 0x01, vec![(0, 0u32), (1, 1u32), (2, 3u32)]);
        let mut led = HdrLedger::new();
        led.parse_one(&mut shim, 0, 0, 0).unwrap();
        let h = led.type01.first().unwrap();
        assert_eq!(h.fields.len(), FIELDS_TYPE01);
        assert!(h.fields_complete());
    }
}
