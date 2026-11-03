//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0464 设备身份账（vendor/device/revision） · 0x75500段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   有效设备身份三元组账齐 100% 覆盖，值域核过（vendor≠0xFFFF）。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 身份三元组（vendor 2B / device 2B / revision 1B）逐设备账齐：
//!     配置偏移 0x00（vendor+device）与 0x08（revision+progIF），
//!     PCI Local Bus Spec r3.0 §6.2.1。
//!   ② vendor 值域三分法：0x0000 无效（规范禁用值）入无效账、
//!     0xFFFF 空槽显性归档（不是错误）、其余有效入身份账。
//!   ③ 100% 覆盖守恒：扫描范围内每个 (bus,slot,func) 地址必须落
//!     在「有效账 ∪ 无效账 ∪ 缺席账」三账之一，Σ三账 = 地址总数，
//!     不许静默漏账。
//!   ④ 已知 vendor 冻结对照表（0x8086 Intel / 0x1022 AMD /
//!     0x10DE NVIDIA / 0x1B36 RedHat / 0x1AF4 virtio），未知 vendor
//!     入账但标记 unlisted（显性不静默）。
//!   ⑤ 注入轮（LCG 种子固定）：无效身份/空槽注入必被对应账收走，
//!     不静默、不报错；双跑一致 + 段位守卫硬判据。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75500，AI-VX7-1104 批F 独占区）。
pub const SEG_BASE: u32 = 0x75500;

/// 扫描总线闭集容量（0~255）。
pub const BUS_CLOSED: usize = 256;
/// 每总线槽数。
pub const SLOTS_PER_BUS: usize = 32;
/// 每设备功能数。
pub const FUNCS_PER_DEV: usize = 8;
/// 空槽 vendor（规范定义：读到 0xFFFF 即功能缺席）。
pub const VENDOR_ABSENT: u16 = 0xFFFF;
/// 无效 vendor（规范禁用值）。
pub const VENDOR_INVALID: u16 = 0x0000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdCode {
    /// 地址越出 (bus,slot,func) 闭集
    AddrOver,
    /// vendor 无效（0x0000 规范禁用）
    VendorInvalid,
    /// 覆盖缺口：地址未落三账之一
    CoverageHole,
    /// 重复登记
    DuplicateId,
    /// 三元组失配（复算不一致）
    TripleMismatch,
    /// 扫描相位倒退（状态机红线）
    PhaseRetrograde,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl IdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IdCode::AddrOver => "地址越界：slot<32 func<8 bus≤255，请核扫描键",
            IdCode::VendorInvalid => "vendor=0x0000 规范禁用：入无效账，请核配置读通路",
            IdCode::CoverageHole => "覆盖缺口：地址未落有效/无效/缺席三账之一，扫描必须账齐",
            IdCode::DuplicateId => "重复登记：同地址二次入账，请查重后再收",
            IdCode::TripleMismatch => "三元组失配：vendor/device/revision 复算不一致",
            IdCode::PhaseRetrograde => "扫描相位倒退：状态机不许回退，请按序推进",
            IdCode::NotReproducible => "双跑不一致：复现红线，身份账须确定性",
        }
    }
    pub const ALL: [IdCode; 7] = [
        IdCode::AddrOver,
        IdCode::VendorInvalid,
        IdCode::CoverageHole,
        IdCode::DuplicateId,
        IdCode::TripleMismatch,
        IdCode::PhaseRetrograde,
        IdCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IdCode::ALL.len() {
        if let Some(c) = IdCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75500 段闭集内"
}

// ── 已知 vendor 冻结对照表（PCI SIG 注册库抽样） ────────────────
/// 已知 vendor 对照表条目。
pub struct VendorRow {
    pub id: u16,
    pub name: &'static str,
}

/// 冻结表：5 家已知厂商（表来源：PCI ID Repository 抽样）。
pub const VENDOR_TABLE: [VendorRow; 5] = [
    VendorRow { id: 0x8086, name: "Intel" },
    VendorRow { id: 0x1022, name: "AMD" },
    VendorRow { id: 0x10DE, name: "NVIDIA" },
    VendorRow { id: 0x1B36, name: "RedHat" },
    VendorRow { id: 0x1AF4, name: "virtio" },
];

/// vendor 是否在已知表内（未知回 false——入账但不静默，标记 unlisted）。
pub fn vendor_listed(v: u16) -> bool {
    let mut i = 0;
    while i < VENDOR_TABLE.len() {
        if let Some(r) = VENDOR_TABLE.get(i) {
            if r.id == v {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// vendor 名查表（未知回 "unlisted"）。
pub fn vendor_name(v: u16) -> &'static str {
    let mut i = 0;
    while i < VENDOR_TABLE.len() {
        if let Some(r) = VENDOR_TABLE.get(i) {
            if r.id == v {
                return r.name;
            }
        }
        i += 1;
    }
    "unlisted"
}

// ── 域模型：地址键 + 身份三元组 ────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevAddr {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
}

impl DevAddr {
    /// 地址闭集核：slot<32 func<8 bus≤255。
    pub fn in_range(self) -> bool {
        (self.slot as usize) < SLOTS_PER_BUS
            && (self.func as usize) < FUNCS_PER_DEV
            && (self.bus as u16) <= (BUS_CLOSED as u16 - 1)
    }
}

/// 身份三元组（vendor 2B / device 2B / revision 1B）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevId {
    pub vendor: u16,
    pub device: u16,
    pub revision: u8,
}

/// vendor 值域三分法（判据核心谓词）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdClass {
    /// 0x0000：规范禁用，入无效账
    Invalid,
    /// 0xFFFF：空槽，入缺席账（显性归档，不是错误）
    Absent,
    /// 其余：有效身份，入身份账
    Valid,
}

/// 三分法分派。
pub fn classify(vendor: u16) -> IdClass {
    if vendor == VENDOR_INVALID {
        IdClass::Invalid
    } else if vendor == VENDOR_ABSENT {
        IdClass::Absent
    } else {
        IdClass::Valid
    }
}

/// 身份账条目（带 unlisted 标记的显性账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdEntry {
    pub addr: DevAddr,
    pub id: DevId,
    pub unlisted: bool,
}

/// 缺席账条目（空槽显性归档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbsentEntry {
    pub addr: DevAddr,
}

// ── 单调相位状态机（倒退显性拒） ────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScanPhase {
    /// 未起步
    #[default]
    Idle,
    /// 三重循环扫描中
    Sweeping,
    /// 收束核账中
    Auditing,
    /// 账齐封账
    Sealed,
}

impl ScanPhase {
    pub fn rank(self) -> u8 {
        match self {
            ScanPhase::Idle => 0,
            ScanPhase::Sweeping => 1,
            ScanPhase::Auditing => 2,
            ScanPhase::Sealed => 3,
        }
    }
}

/// 身份账簿：三账 + 相位 + 覆盖核账。
#[derive(Clone, Debug, Default)]
pub struct IdLedger {
    pub valids: Vec<IdEntry>,
    pub invalids: Vec<DevAddr>,
    pub absents: Vec<AbsentEntry>,
    pub phase: ScanPhase,
}

impl IdLedger {
    pub fn new() -> IdLedger {
        IdLedger::default()
    }

    /// 相位推进（同相允许，倒退显性拒）。
    pub fn advance(&mut self, next: ScanPhase) -> Result<(), u32> {
        if next.rank() < self.phase.rank() {
            return Err(IdCode::PhaseRetrograde.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 收一条身份（三分法入账；重复登记拒；无效入无效账不算 Err）。
    pub fn record(&mut self, addr: DevAddr, id: DevId) -> Result<IdClass, u32> {
        if !addr.in_range() {
            return Err(IdCode::AddrOver.val());
        }
        // 查重：三账任一命中同地址即重复。
        let mut i = 0;
        while i < self.valids.len() {
            if let Some(e) = self.valids.get(i) {
                if e.addr.bus == addr.bus && e.addr.slot == addr.slot && e.addr.func == addr.func {
                    return Err(IdCode::DuplicateId.val());
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.invalids.len() {
            if let Some(a) = self.invalids.get(j) {
                if a.bus == addr.bus && a.slot == addr.slot && a.func == addr.func {
                    return Err(IdCode::DuplicateId.val());
                }
            }
            j += 1;
        }
        let mut k = 0;
        while k < self.absents.len() {
            if let Some(a) = self.absents.get(k) {
                if a.addr.bus == addr.bus && a.addr.slot == addr.slot && a.addr.func == addr.func {
                    return Err(IdCode::DuplicateId.val());
                }
            }
            k += 1;
        }
        let cls = classify(id.vendor);
        match cls {
            IdClass::Valid => {
                let entry = IdEntry { addr, id, unlisted: !vendor_listed(id.vendor) };
                self.valids.push(entry);
            }
            IdClass::Invalid => {
                self.invalids.push(addr);
            }
            IdClass::Absent => {
                self.absents.push(AbsentEntry { addr });
            }
        }
        Ok(cls)
    }

    /// 地址是否已落三账之一。
    pub fn covered(&self, addr: DevAddr) -> bool {
        let mut i = 0;
        while i < self.valids.len() {
            if let Some(e) = self.valids.get(i) {
                if e.addr.bus == addr.bus && e.addr.slot == addr.slot && e.addr.func == addr.func {
                    return true;
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.invalids.len() {
            if let Some(a) = self.invalids.get(j) {
                if a.bus == addr.bus && a.slot == addr.slot && a.func == addr.func {
                    return true;
                }
            }
            j += 1;
        }
        let mut k = 0;
        while k < self.absents.len() {
            if let Some(a) = self.absents.get(k) {
                if a.addr.bus == addr.bus && a.addr.slot == addr.slot && a.addr.func == addr.func {
                    return true;
                }
            }
            k += 1;
        }
        false
    }

    /// 覆盖守恒核：Σ三账 == 地址总数（判据 100% 覆盖）。
    pub fn coverage_ok(&self, total: usize) -> bool {
        let sum = self
            .valids
            .len()
            .saturating_add(self.invalids.len())
            .saturating_add(self.absents.len());
        sum == total
    }

    /// 有效三元组值域核：vendor∉{0x0000,0xFFFF}。
    pub fn valids_range_ok(&self) -> bool {
        let mut i = 0;
        while i < self.valids.len() {
            if let Some(e) = self.valids.get(i) {
                let c = classify(e.id.vendor);
                if c != IdClass::Valid {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── 配置读垫片（身份账专用影子读） ──────────────────────────────
/// 身份读源：给定地址回三元组（None=空槽），种子固定不随机。
pub struct IdShim {
    /// 平账：(addr, Option<DevId>)，None 表示读到 0xFFFF 空槽
    pub map: Vec<(DevAddr, Option<DevId>)>,
    pub read_ops: u64,
}

impl IdShim {
    pub fn new() -> IdShim {
        IdShim { map: Vec::new(), read_ops: 0 }
    }

    /// 登记一条影子读账（None 即空槽注入）。
    pub fn put(&mut self, addr: DevAddr, id: Option<DevId>) {
        self.map.push((addr, id));
    }

    /// 读身份：账缺地址回 Ok(None)（空槽语义），越界回 Err。
    pub fn read(&mut self, addr: DevAddr) -> Result<Option<DevId>, u32> {
        if !addr.in_range() {
            return Err(IdCode::AddrOver.val());
        }
        self.read_ops = self.read_ops.saturating_add(1);
        let mut i = 0;
        while i < self.map.len() {
            if let Some((a, id)) = self.map.get(i) {
                if a.bus == addr.bus && a.slot == addr.slot && a.func == addr.func {
                    return Ok(*id);
                }
            }
            i += 1;
        }
        Ok(None)
    }
}

// ── 三重循环扫描器 ──────────────────────────────────────────────
/// 全域扫描账（扫描范围 = 总线 0..=BUS_SCAN_MAX 的全部 32×8 功能）。
pub struct IdScanner {
    /// 扫描总线数上限（真机账：4 总线；闭集核另设 256 全域账）。
    pub bus_max: u8,
}

impl IdScanner {
    pub fn new(bus_max: u8) -> IdScanner {
        IdScanner { bus_max }
    }

    /// 三重循环扫描：每个 (bus,slot,func) 读一次，三分法入账。
    /// 回 (地址总数, 扫描读次数)。
    pub fn sweep(&self, shim: &mut IdShim, led: &mut IdLedger) -> Result<(usize, u64), u32> {
        led.advance(ScanPhase::Sweeping)?;
        let mut total = 0usize;
        let mut bus = 0u8;
        loop {
            let mut slot = 0u8;
            while (slot as usize) < SLOTS_PER_BUS {
                let mut func = 0u8;
                while (func as usize) < FUNCS_PER_DEV {
                    let addr = DevAddr { bus, slot, func };
                    let got = shim.read(addr)?;
                    match got {
                        Some(id) => {
                            let _ = led.record(addr, id)?;
                        }
                        None => {
                            // 账内未登记的地址按规范语义=0xFFFF 空槽。
                            let _ = led.record(
                                addr,
                                DevId { vendor: VENDOR_ABSENT, device: VENDOR_ABSENT, revision: 0 },
                            )?;
                        }
                    }
                    total = total.saturating_add(1);
                    func = func.saturating_add(1);
                }
                slot = slot.saturating_add(1);
            }
            if bus >= self.bus_max {
                break;
            }
            bus = bus.saturating_add(1);
        }
        led.advance(ScanPhase::Auditing)?;
        Ok((total, shim.read_ops))
    }
}

// ── 真机身份账（冻结：4 总线，5 设备 3 空槽显性 + 1 无效注入） ──
/// 真机影子账：bus0 三设备 + bus1 一设备 + bus2 一设备，其余空槽。
pub fn machine_shim() -> IdShim {
    let mut shim = IdShim::new();
    shim.put(DevAddr { bus: 0, slot: 0, func: 0 }, Some(DevId { vendor: 0x8086, device: 0x1237, revision: 0x02 }));
    shim.put(DevAddr { bus: 0, slot: 2, func: 0 }, Some(DevId { vendor: 0x8086, device: 0x15D8, revision: 0x05 }));
    shim.put(DevAddr { bus: 0, slot: 31, func: 0 }, Some(DevId { vendor: 0x8086, device: 0x2922, revision: 0x02 }));
    shim.put(DevAddr { bus: 1, slot: 0, func: 0 }, Some(DevId { vendor: 0x1AF4, device: 0x104D, revision: 0x01 }));
    shim.put(DevAddr { bus: 2, slot: 0, func: 0 }, Some(DevId { vendor: 0x10DE, device: 0x1B80, revision: 0xA1 }));
    shim
}

/// 无效身份注入账：bus3 slot0 func0 vendor=0x0000（规范禁用值必入无效账）。
pub fn inject_invalid_shim() -> IdShim {
    let mut shim = machine_shim();
    shim.put(DevAddr { bus: 3, slot: 0, func: 0 }, Some(DevId { vendor: VENDOR_INVALID, device: 0xBEEF, revision: 0x00 }));
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
/// 主账：真机扫描账齐，有效 5 条、覆盖守恒、值域核过。
fn main_ledger_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = IdLedger::new();
    let sc = IdScanner::new(3);
    match sc.sweep(&mut shim, &mut led) {
        Ok((total, reads)) => {
            if total != 4 * SLOTS_PER_BUS * FUNCS_PER_DEV {
                return false;
            }
            if reads != total as u64 {
                return false;
            }
            if led.valids.len() != 5 {
                return false;
            }
            if !led.coverage_ok(total) {
                return false;
            }
            if !led.valids_range_ok() {
                return false;
            }
            if led.invalids.len() != 0 {
                return false;
            }
            led.advance(ScanPhase::Sealed).is_ok()
        }
        Err(_) => false,
    }
}

/// 值域三分法逐点核：0x0000→Invalid / 0xFFFF→Absent / 0x8086→Valid。
fn classify_ok() -> bool {
    if classify(VENDOR_INVALID) != IdClass::Invalid {
        return false;
    }
    if classify(VENDOR_ABSENT) != IdClass::Absent {
        return false;
    }
    if classify(0x8086) != IdClass::Valid {
        return false;
    }
    if classify(0x10DE) != IdClass::Valid {
        return false;
    }
    true
}

/// 缺席账显性归档：空槽数 = total − 有效数，缺席账非空且不是错误路径。
fn absent_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = IdLedger::new();
    let sc = IdScanner::new(3);
    let (total, _) = match sc.sweep(&mut shim, &mut led) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let want_absent = total - 5;
    if led.absents.len() != want_absent {
        return false;
    }
    // 缺席条目地址全部在扫描闭集内（显性归档不越界）。
    let mut i = 0;
    while i < led.absents.len() {
        if let Some(a) = led.absents.get(i) {
            if !a.addr.in_range() {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 注入-无效身份必抓：vendor=0x0000 入无效账（不静默、不误入有效账）。
fn inject_invalid_ok() -> bool {
    let mut shim = inject_invalid_shim();
    let mut led = IdLedger::new();
    let sc = IdScanner::new(3);
    match sc.sweep(&mut shim, &mut led) {
        Ok((total, _)) => {
            if led.invalids.len() != 1 {
                return false;
            }
            if led.valids.len() != 5 {
                return false;
            }
            led.coverage_ok(total)
        }
        Err(_) => false,
    }
}

/// 注入-越界地址必抓：slot=32 回 Err(AddrOver)。
fn inject_over_ok() -> bool {
    let mut shim = machine_shim();
    match shim.read(DevAddr { bus: 0, slot: 32, func: 0 }) {
        Err(e) if e == IdCode::AddrOver.val() => {}
        _ => return false,
    }
    let mut led = IdLedger::new();
    match led.record(DevAddr { bus: 0, slot: 33, func: 0 },
        DevId { vendor: 0x8086, device: 1, revision: 1 }) {
        Err(e) if e == IdCode::AddrOver.val() => {}
        _ => return false,
    }
    true
}

/// 注入-重复登记必抓：同地址二次入账回 Err(DuplicateId)。
fn inject_dup_ok() -> bool {
    let mut led = IdLedger::new();
    let a = DevAddr { bus: 0, slot: 1, func: 0 };
    let id = DevId { vendor: 0x1022, device: 0x2000, revision: 0x01 };
    if led.record(a, id).is_err() {
        return false;
    }
    match led.record(a, id) {
        Err(e) if e == IdCode::DuplicateId.val() => {}
        _ => return false,
    }
    true
}

/// vendor 对照表核：5 家已知命中，未知 unlisted 不静默。
fn vendor_table_ok() -> bool {
    let mut i = 0;
    while i < VENDOR_TABLE.len() {
        if let Some(r) = VENDOR_TABLE.get(i) {
            if !vendor_listed(r.id) || vendor_name(r.id) != r.name {
                return false;
            }
        }
        i += 1;
    }
    if vendor_listed(0x9999) || vendor_name(0x9999) != "unlisted" {
        return false;
    }
    true
}

/// 相位状态机：Idle→Sweeping→Auditing→Sealed 可推进，倒退拒。
fn phase_ok() -> bool {
    let mut led = IdLedger::new();
    if led.advance(ScanPhase::Sweeping).is_err() {
        return false;
    }
    if led.advance(ScanPhase::Auditing).is_err() {
        return false;
    }
    if led.advance(ScanPhase::Sealed).is_err() {
        return false;
    }
    if led.advance(ScanPhase::Idle).is_ok() {
        return false;
    }
    true
}

/// LCG 注入轮：随机 vendor 值三分法守恒（种子固定，双跑一致）。
fn lcg_rounds_ok() -> bool {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut n = 0u32;
    while n < 64 {
        let rnd = rng.next();
        let vendor = (rnd & 0xFFFF) as u16;
        let addr = DevAddr { bus: 0, slot: 2, func: 0 };
        // 每轮新账簿只收单条（三分法自证）。
        let mut one = IdLedger::new();
        let id = DevId { vendor, device: 0xC0DE, revision: 0x01 };
        match one.record(addr, id) {
            Ok(cls) => {
                if cls != classify(vendor) {
                    return false;
                }
                if one.valids.len().saturating_add(one.invalids.len()).saturating_add(one.absents.len()) != 1 {
                    return false;
                }
            }
            Err(_) => return false,
        }
        n = n.saturating_add(1);
    }
    true
}

/// 双跑一致（复现红线）。
fn repro_ok() -> bool {
    let mut a_shim = machine_shim();
    let mut a_led = IdLedger::new();
    let a = IdScanner::new(3).sweep(&mut a_shim, &mut a_led);
    let mut b_shim = machine_shim();
    let mut b_led = IdLedger::new();
    let b = IdScanner::new(3).sweep(&mut b_shim, &mut b_led);
    a.is_ok() && b.is_ok() && a_led.valids == b_led.valids && a_led.absents == b_led.absents
}

/// 段位守卫。
fn segment_guard_ok() -> bool {
    let mut i = 0;
    while i < IdCode::ALL.len() {
        if let Some(c) = IdCode::ALL.get(i) {
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
    while i < IdCode::ALL.len() {
        if let Some(c) = IdCode::ALL.get(i) {
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
    v.push(("F0464-J1-段位守卫", segment_guard_ok(),
        format!("SEG_BASE=0x{:X} 窗口 0x40", SEG_BASE)));
    v.push(("F0464-J1-身份主账", main_ledger_ok(),
        format!("有效 5 条覆盖守恒，判定 {}", main_ledger_ok())));
    v.push(("F0464-J1-值域三分法", classify_ok(),
        format!("0x0000/0xFFFF/0x8086 三分，判定 {}", classify_ok())));
    v.push(("F0464-J1-缺席显性归档", absent_ok(),
        format!("空槽入缺席账非错误路径，判定 {}", absent_ok())));
    v.push(("F0464-J1-注入无效身份必抓", inject_invalid_ok(),
        format!("vendor=0x0000 入无效账，判定 {}", inject_invalid_ok())));
    v.push(("F0464-J1-注入越界地址必抓", inject_over_ok(),
        format!("slot=32 回 AddrOver，判定 {}", inject_over_ok())));
    v.push(("F0464-J1-注入重复登记必抓", inject_dup_ok(),
        format!("同地址二次入账拒，判定 {}", inject_dup_ok())));
    v.push(("F0464-J1-vendor对照表", vendor_table_ok(),
        format!("5 家已知命中未知 unlisted，判定 {}", vendor_table_ok())));
    v.push(("F0464-J1-相位状态机", phase_ok(),
        format!("倒退显性拒，判定 {}", phase_ok())));
    v.push(("F0464-J1-LCG注入轮", lcg_rounds_ok(),
        format!("64 轮随机 vendor 三分守恒，判定 {}", lcg_rounds_ok())));
    v.push(("F0464-J2-双跑一致", repro_ok(),
        format!("两轮账全等，判定 {}", repro_ok())));
    v.push(("F0464-J1-诊断码闭集", code_table_ok(),
        format!("7 码查表可达，判定 {}", code_table_ok())));
    v
}

pub fn run_unx0464_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0464 设备身份账");
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
    fn main_ledger_counts() {
        let mut shim = machine_shim();
        let mut led = IdLedger::new();
        let (total, reads) = IdScanner::new(3).sweep(&mut shim, &mut led).unwrap();
        assert_eq!(total, 4 * SLOTS_PER_BUS * FUNCS_PER_DEV);
        assert_eq!(reads, total as u64);
        assert_eq!(led.valids.len(), 5);
        assert!(led.invalids.is_empty());
        assert!(led.coverage_ok(total));
    }

    #[test]
    fn classify_three_way() {
        assert_eq!(classify(0x0000), IdClass::Invalid);
        assert_eq!(classify(0xFFFF), IdClass::Absent);
        assert_eq!(classify(0x8086), IdClass::Valid);
        assert_eq!(classify(0x1234), IdClass::Valid);
    }

    #[test]
    fn invalid_injection_lands_in_invalid_ledger() {
        let mut shim = inject_invalid_shim();
        let mut led = IdLedger::new();
        let (total, _) = IdScanner::new(3).sweep(&mut shim, &mut led).unwrap();
        assert_eq!(led.invalids.len(), 1);
        assert_eq!(led.valids.len(), 5);
        assert!(led.coverage_ok(total));
    }

    #[test]
    fn absent_slots_explicit() {
        let mut shim = machine_shim();
        let mut led = IdLedger::new();
        let (total, _) = IdScanner::new(3).sweep(&mut shim, &mut led).unwrap();
        assert_eq!(led.absents.len(), total - 5);
    }

    #[test]
    fn duplicate_and_overflow_rejected() {
        let mut led = IdLedger::new();
        let a = DevAddr { bus: 0, slot: 1, func: 0 };
        let id = DevId { vendor: 0x1022, device: 1, revision: 1 };
        led.record(a, id).unwrap();
        assert_eq!(led.record(a, id).unwrap_err(), IdCode::DuplicateId.val());
        assert_eq!(
            led.record(DevAddr { bus: 0, slot: 32, func: 0 }, id).unwrap_err(),
            IdCode::AddrOver.val()
        );
    }

    #[test]
    fn phase_monotonic() {
        let mut led = IdLedger::new();
        led.advance(ScanPhase::Sweeping).unwrap();
        led.advance(ScanPhase::Auditing).unwrap();
        led.advance(ScanPhase::Sealed).unwrap();
        assert_eq!(led.advance(ScanPhase::Sweeping).unwrap_err(), IdCode::PhaseRetrograde.val());
    }

    #[test]
    fn vendor_table_closed() {
        assert!(vendor_listed(0x8086));
        assert_eq!(vendor_name(0x1AF4), "virtio");
        assert!(!vendor_listed(0x9999));
        assert_eq!(vendor_name(0x9999), "unlisted");
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut i = 0;
        while i < 16 {
            assert_eq!(a.next(), b.next());
            i += 1;
        }
    }

    #[test]
    fn reproducible_double_run() {
        assert!(repro_ok());
        let s1 = run_unx0464_checks();
        let s2 = run_unx0464_checks();
        assert!(s1.all_passed());
        assert_eq!(s1.all_passed(), s2.all_passed());
    }

    #[test]
    fn guard_and_code_table() {
        assert!(segment_guard_ok());
        assert!(code_table_ok());
        assert_eq!(detail(0x123_4567), "未知诊断码：不在 0x75500 段闭集内");
    }

    #[test]
    fn coverage_hole_detected() {
        // 手工构造覆盖缺口：只收 1 条但 total 记 2。
        let mut led = IdLedger::new();
        led.record(DevAddr { bus: 0, slot: 0, func: 0 },
            DevId { vendor: 0x8086, device: 1, revision: 1 }).unwrap();
        assert!(!led.coverage_ok(2));
        assert!(led.coverage_ok(1));
    }

    #[test]
    fn shim_absent_semantics() {
        let mut shim = IdShim::new();
        // 未登记地址读到 None（空槽语义）。
        assert_eq!(shim.read(DevAddr { bus: 7, slot: 3, func: 1 }).unwrap(), None);
    }
}
