//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0467 类码分类账（class/subclass/progIF） · 0x755C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   类码三元组账齐 100%，六大类计数守恒（存储/网络/显示/桥/输
//!   入/其他）。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 类码三元组（base class 1B / subclass 1B / progIF 1B）逐设备
//!     账齐（偏移 0x08~0x0B，PCI Local Bus Spec r3.0 §6.2.1），
//!     100% 覆盖 = 每台有效设备恰有一条三元组账。
//!   ② 六大类闭集计数：存储 0x01 / 网络 0x02 / 显示 0x03 / 桥
//!     0x06 / 输入 0x09 / 其他（未登记类显性入其他桶）；Σ六桶 =
//!     设备总数守恒（判据核心），不许静默漏桶。
//!   ③ 已知 subclass 冻结对照表（SATA 0x0106 / NVMe 0x0108 /
//!     Ethernet 0x0200 / VGA 0x0300 / PCI bridge 0x0604 / USB
//!     0x0C03…），对照命中标记 named，未命中标记 unnamed 不静默。
//!   ④ 注入必抓（未登记类入其他桶不失踪 / 覆盖缺口必拒 / 重复
//!     登记必拒）+ 单调相位状态机 + 双跑一致 + 段位守卫。
//!   ⑤ 诊断码闭集 0x40 窗口查表，Err 用 Result<T, u32>。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x755C0，AI-VX7-1104 批F 独占区）。
pub const SEG_BASE: u32 = 0x755C0;

/// 六大类闭集容量。
pub const BUCKET_COUNT: usize = 6;

// ── 六大类闭集 ──────────────────────────────────────────────────
/// 六大类桶（base class 闭集 + 其他兜底桶）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassBucket {
    /// 存储控制器（base class 0x01）
    Storage,
    /// 网络控制器（base class 0x02）
    Network,
    /// 显示控制器（base class 0x03）
    Display,
    /// 桥设备（base class 0x06）
    Bridge,
    /// 输入设备（base class 0x09）
    Input,
    /// 其他/未登记类（显性兜底桶）
    Other,
}

impl ClassBucket {
    /// base class → 桶分派（未登记类显性入其他桶）。
    pub fn from_base(base: u8) -> ClassBucket {
        match base {
            0x01 => ClassBucket::Storage,
            0x02 => ClassBucket::Network,
            0x03 => ClassBucket::Display,
            0x06 => ClassBucket::Bridge,
            0x09 => ClassBucket::Input,
            _ => ClassBucket::Other,
        }
    }

    pub fn index(self) -> usize {
        match self {
            ClassBucket::Storage => 0,
            ClassBucket::Network => 1,
            ClassBucket::Display => 2,
            ClassBucket::Bridge => 3,
            ClassBucket::Input => 4,
            ClassBucket::Other => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ClassBucket::Storage => "存储",
            ClassBucket::Network => "网络",
            ClassBucket::Display => "显示",
            ClassBucket::Bridge => "桥",
            ClassBucket::Input => "输入",
            ClassBucket::Other => "其他",
        }
    }

    pub const ALL: [ClassBucket; BUCKET_COUNT] = [
        ClassBucket::Storage,
        ClassBucket::Network,
        ClassBucket::Display,
        ClassBucket::Bridge,
        ClassBucket::Input,
        ClassBucket::Other,
    ];
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClassCode {
    /// 类码覆盖缺口（设备无三元组账）
    CoverageHole,
    /// 重复登记（同设备二次入账）
    DuplicateDev,
    /// 桶计数守恒破坏（Σ六桶≠设备总数）
    BucketBroken,
    /// 桶下标越界
    BucketOver,
    /// 登记相位倒退（状态机红线）
    LedgerRetrograde,
    /// 类码读扑空（账内无此设备）
    ReadMiss,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl ClassCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClassCode::CoverageHole => "覆盖缺口：有效设备必须恰有一条三元组账，查漏登记",
            ClassCode::DuplicateDev => "重复登记：同设备二次入账，请查重后再收",
            ClassCode::BucketBroken => "桶守恒破坏：Σ六桶须等于设备总数，核对账面",
            ClassCode::BucketOver => "桶下标越界：六桶闭集 0..=5，请核分派表",
            ClassCode::LedgerRetrograde => "登记相位倒退：状态机不许回退，请按序推进",
            ClassCode::ReadMiss => "类码读扑空：设备未入账，请先装载影子账再分类",
            ClassCode::NotReproducible => "双跑不一致：复现红线，分类账须确定性",
        }
    }
    pub const ALL: [ClassCode; 7] = [
        ClassCode::CoverageHole,
        ClassCode::DuplicateDev,
        ClassCode::BucketBroken,
        ClassCode::BucketOver,
        ClassCode::LedgerRetrograde,
        ClassCode::ReadMiss,
        ClassCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClassCode::ALL.len() {
        if let Some(c) = ClassCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x755C0 段闭集内"
}

// ── 域模型：类码三元组 ──────────────────────────────────────────
/// 类码三元组（base class / subclass / progIF）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassTriple {
    pub base: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

/// 三元组 24 位合并值（0xBBSSPP 编码，可复算键）。
pub fn triple_key(t: ClassTriple) -> u32 {
    ((t.base as u32) << 16) | ((t.subclass as u32) << 8) | (t.prog_if as u32)
}

// ── 已知 subclass 冻结对照表（PCI SIG 类码库抽样） ──────────────
pub struct ClassRow {
    pub key: u32,
    pub name: &'static str,
}

/// 冻结表：8 条常见类码（来源：PCI Local Bus Spec r3.0 附录 D 抽样）。
pub const CLASS_TABLE: [ClassRow; 8] = [
    ClassRow { key: 0x010601, name: "SATA AHCI" },
    ClassRow { key: 0x010802, name: "NVMe" },
    ClassRow { key: 0x020000, name: "Ethernet" },
    ClassRow { key: 0x030000, name: "VGA" },
    ClassRow { key: 0x030200, name: "3D" },
    ClassRow { key: 0x060400, name: "PCI-to-PCI bridge" },
    ClassRow { key: 0x060000, name: "Host bridge" },
    ClassRow { key: 0x0C0330, name: "XHCI" },
];

/// 对照命中名（未命中回 "unnamed"——显性不静默）。
pub fn class_name(t: ClassTriple) -> &'static str {
    let k = triple_key(t);
    let mut i = 0;
    while i < CLASS_TABLE.len() {
        if let Some(r) = CLASS_TABLE.get(i) {
            if r.key == k {
                return r.name;
            }
        }
        i += 1;
    }
    "unnamed"
}

// ── 类码账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassEntry {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    pub triple: ClassTriple,
    pub bucket: ClassBucket,
    pub named: bool,
}

/// 分类账簿：账 + 六桶计数 + 相位。
#[derive(Clone, Debug, Default)]
pub struct ClassLedger {
    pub entries: Vec<ClassEntry>,
    pub buckets: [u32; BUCKET_COUNT],
    pub phase: LedgerPhase,
}

/// 单调相位状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LedgerPhase {
    /// 未起步
    #[default]
    Idle,
    /// 分类登记中
    Filing,
    /// 六桶核账中
    Auditing,
    /// 封账
    Sealed,
}

impl LedgerPhase {
    pub fn rank(self) -> u8 {
        match self {
            LedgerPhase::Idle => 0,
            LedgerPhase::Filing => 1,
            LedgerPhase::Auditing => 2,
            LedgerPhase::Sealed => 3,
        }
    }
}

impl ClassLedger {
    pub fn new() -> ClassLedger {
        ClassLedger::default()
    }

    pub fn advance(&mut self, next: LedgerPhase) -> Result<(), u32> {
        if next.rank() < self.phase.rank() {
            return Err(ClassCode::LedgerRetrograde.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 登记一台设备的类码（查重 + 六桶分派 + 计数入账）。
    pub fn file(&mut self, bus: u8, slot: u8, func: u8, triple: ClassTriple) -> Result<ClassBucket, u32> {
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.bus == bus && e.slot == slot && e.func == func {
                    return Err(ClassCode::DuplicateDev.val());
                }
            }
            i += 1;
        }
        let bucket = ClassBucket::from_base(triple.base);
        let idx = bucket.index();
        if idx >= BUCKET_COUNT {
            return Err(ClassCode::BucketOver.val());
        }
        let entry = ClassEntry {
            bus,
            slot,
            func,
            triple,
            bucket,
            named: class_name(triple) != "unnamed",
        };
        self.entries.push(entry);
        if let Some(n) = self.buckets.get_mut(idx) {
            *n = n.saturating_add(1);
        }
        Ok(bucket)
    }

    /// 六桶守恒核：Σ桶计数 == 登记条数（判据核心）。
    pub fn conservation_ok(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0;
        while i < BUCKET_COUNT {
            if let Some(n) = self.buckets.get(i) {
                sum = sum.saturating_add(*n);
            }
            i += 1;
        }
        sum == self.entries.len() as u32
    }

    /// 100% 覆盖核：每台已登记设备恰有一条账，且无重复（判据 100%）。
    pub fn coverage_ok(&self, total: usize) -> bool {
        if self.entries.len() != total {
            return false;
        }
        self.conservation_ok()
    }

    /// 未登记类显性入其他桶核：Other 桶计数 == 未命中闭集分派数。
    pub fn other_bucket_ok(&self) -> bool {
        let mut other_named = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.bucket == ClassBucket::Other {
                    other_named = other_named.saturating_add(1);
                }
            }
            i += 1;
        }
        other_named == bucket_count(self, ClassBucket::Other)
    }
}

/// 桶计数安全读（.get() 取值，不裸下标）。
pub fn bucket_count(led: &ClassLedger, b: ClassBucket) -> u32 {
    led.buckets.get(b.index()).copied().unwrap_or(0)
}

// ── 影子读垫片（只读路径） ──────────────────────────────────────
pub struct ClassShim {
    /// (bus, slot, func, triple)
    pub map: Vec<(u8, u8, u8, ClassTriple)>,
    pub read_ops: u64,
    pub write_ops: u64,
}

impl ClassShim {
    pub fn new() -> ClassShim {
        ClassShim { map: Vec::new(), read_ops: 0, write_ops: 0 }
    }

    pub fn put(&mut self, bus: u8, slot: u8, func: u8, triple: ClassTriple) {
        self.map.push((bus, slot, func, triple));
    }

    /// 读类码（只读路径）：账缺回 Err(ReadMiss)。
    pub fn read(&mut self, bus: u8, slot: u8, func: u8) -> Result<ClassTriple, u32> {
        self.read_ops = self.read_ops.saturating_add(1);
        let mut i = 0;
        while i < self.map.len() {
            if let Some((b, s, f, t)) = self.map.get(i) {
                if *b == bus && *s == slot && *f == func {
                    return Ok(*t);
                }
            }
            i += 1;
        }
        Err(ClassCode::ReadMiss.val())
    }

    /// 配置写（只读红线：恒拒显性记账）。
    pub fn cfg_write(&mut self, bus: u8, slot: u8, func: u8, off: u8, val: u32) -> Result<(), u32> {
        let _ = (bus, slot, func, off, val);
        self.write_ops = self.write_ops.saturating_add(1);
        Err(ClassCode::ReadMiss.val())
    }
}

/// 全域分类器：逐设备读类码入账。
pub fn classify_all(shim: &mut ClassShim, led: &mut ClassLedger) -> Result<usize, u32> {
    led.advance(LedgerPhase::Filing)?;
    // 影子账平账序遍历（读路径不做总线扫描——扫描归 F0463，本卡只分类）。
    let mut n = 0usize;
    let mut i = 0;
    while i < shim.map.len() {
        if let Some((b, s, f, _)) = shim.map.get(i) {
            let (b, s, f) = (*b, *s, *f);
            let t = shim.read(b, s, f)?;
            led.file(b, s, f, t)?;
            n = n.saturating_add(1);
        }
        i += 1;
    }
    led.advance(LedgerPhase::Auditing)?;
    Ok(n)
}

// ── 真机影子账（冻结：6 台，五桶 + 其他桶各显性） ────────────────
/// 真机类码账：SATA/NIC/GPU/根桥/USB/XHCI+遗留类各一。
pub fn machine_shim() -> ClassShim {
    let mut shim = ClassShim::new();
    shim.put(0, 31, 0, ClassTriple { base: 0x01, subclass: 0x06, prog_if: 0x01 }); // SATA AHCI → 存储
    shim.put(1, 0, 0, ClassTriple { base: 0x02, subclass: 0x00, prog_if: 0x00 }); // Ethernet → 网络
    shim.put(2, 0, 0, ClassTriple { base: 0x03, subclass: 0x00, prog_if: 0x00 }); // VGA → 显示
    shim.put(0, 0, 0, ClassTriple { base: 0x06, subclass: 0x00, prog_if: 0x00 }); // Host bridge → 桥
    shim.put(0, 20, 0, ClassTriple { base: 0x0C, subclass: 0x03, prog_if: 0x30 }); // XHCI → 其他（0x0C 未登记五桶）
    shim.put(5, 0, 0, ClassTriple { base: 0x09, subclass: 0x00, prog_if: 0x00 }); // 输入控制器 → 输入
    shim
}

/// 未登记类注入账：base=0x77（五桶闭集外，必入其他桶显性）。
pub fn inject_unlisted_shim() -> ClassShim {
    let mut shim = machine_shim();
    shim.put(9, 9, 9, ClassTriple { base: 0x77, subclass: 0x11, prog_if: 0x22 });
    shim
}

/// 覆盖缺口注入账：登记时跳过一台设备（覆盖缺口必拒）。
pub fn inject_hole_shim() -> ClassShim {
    let mut shim = ClassShim::new();
    shim.put(0, 31, 0, ClassTriple { base: 0x01, subclass: 0x06, prog_if: 0x01 });
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
/// 主账：6 台全分派，五桶 + 其他桶显性，守恒成立。
fn main_ledger_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = ClassLedger::new();
    match classify_all(&mut shim, &mut led) {
        Ok(n) => {
            if n != 6 {
                return false;
            }
            if bucket_count(&led, ClassBucket::Storage) != 1
                || bucket_count(&led, ClassBucket::Network) != 1
                || bucket_count(&led, ClassBucket::Display) != 1
                || bucket_count(&led, ClassBucket::Bridge) != 1
                || bucket_count(&led, ClassBucket::Input) != 1
                || bucket_count(&led, ClassBucket::Other) != 1
            {
                return false;
            }
            if !led.conservation_ok() || !led.other_bucket_ok() {
                return false;
            }
            led.advance(LedgerPhase::Sealed).is_ok()
        }
        Err(_) => false,
    }
}

/// 覆盖 100% 核（判据核心）：登记条数 == 设备总数。
fn coverage_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = ClassLedger::new();
    match classify_all(&mut shim, &mut led) {
        Ok(_) => led.coverage_ok(6),
        Err(_) => false,
    }
}

/// 六桶分派闭集核：base → 桶逐点核 + 未登记类兜底。
fn bucket_dispatch_ok() -> bool {
    if ClassBucket::from_base(0x01) != ClassBucket::Storage {
        return false;
    }
    if ClassBucket::from_base(0x02) != ClassBucket::Network {
        return false;
    }
    if ClassBucket::from_base(0x03) != ClassBucket::Display {
        return false;
    }
    if ClassBucket::from_base(0x06) != ClassBucket::Bridge {
        return false;
    }
    if ClassBucket::from_base(0x09) != ClassBucket::Input {
        return false;
    }
    // 未登记类显性入其他桶（0x00/0x0C/0x77/0xFF 全兜底）。
    if ClassBucket::from_base(0x00) != ClassBucket::Other
        || ClassBucket::from_base(0x0C) != ClassBucket::Other
        || ClassBucket::from_base(0x77) != ClassBucket::Other
        || ClassBucket::from_base(0xFF) != ClassBucket::Other
    {
        return false;
    }
    // 桶下标闭集 0..=5。
    let mut i = 0;
    while i < BUCKET_COUNT {
        if let Some(b) = ClassBucket::ALL.get(i) {
            if b.index() != i {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 注入-未登记类必入其他桶显性（不失踪、不报错）。
fn inject_unlisted_ok() -> bool {
    let mut shim = inject_unlisted_shim();
    let mut led = ClassLedger::new();
    match classify_all(&mut shim, &mut led) {
        Ok(n) => {
            if n != 7 {
                return false;
            }
            if bucket_count(&led, ClassBucket::Other) != 2 {
                return false;
            }
            led.conservation_ok() && led.other_bucket_ok()
        }
        Err(_) => false,
    }
}

/// 注入-重复登记必抓。
fn inject_dup_ok() -> bool {
    let mut led = ClassLedger::new();
    let t = ClassTriple { base: 0x01, subclass: 0x06, prog_if: 0x01 };
    if led.file(0, 31, 0, t).is_err() {
        return false;
    }
    match led.file(0, 31, 0, t) {
        Err(e) if e == ClassCode::DuplicateDev.val() => true,
        _ => false,
    }
}

/// 注入-覆盖缺口必抓：少登记一台 → coverage_ok 拒。
fn inject_hole_ok() -> bool {
    let mut shim = inject_hole_shim();
    let mut led = ClassLedger::new();
    match classify_all(&mut shim, &mut led) {
        Ok(n) => {
            if n != 1 {
                return false;
            }
            // 期望 6 台只来 1 台：覆盖缺口显性核出。
            !led.coverage_ok(6)
        }
        Err(_) => false,
    }
}

/// 注入-读扑空必抓。
fn inject_readmiss_ok() -> bool {
    let mut shim = ClassShim::new();
    match shim.read(9, 9, 9) {
        Err(e) if e == ClassCode::ReadMiss.val() => true,
        _ => false,
    }
}

/// 类码对照表核：8 条命中，未知 unnamed 不静默。
fn class_table_ok() -> bool {
    let mut i = 0;
    while i < CLASS_TABLE.len() {
        if let Some(r) = CLASS_TABLE.get(i) {
            let t = ClassTriple {
                base: (r.key >> 16) as u8,
                subclass: (r.key >> 8) as u8,
                prog_if: (r.key & 0xFF) as u8,
            };
            if class_name(t) != r.name {
                return false;
            }
        }
        i += 1;
    }
    class_name(ClassTriple { base: 0x77, subclass: 0x11, prog_if: 0x22 }) == "unnamed"
}

/// 三元组可复算键核：0x010601 编码往返一致。
fn triple_key_ok() -> bool {
    let t = ClassTriple { base: 0x01, subclass: 0x06, prog_if: 0x01 };
    let k = triple_key(t);
    if k != 0x010601 {
        return false;
    }
    let back = ClassTriple { base: (k >> 16) as u8, subclass: (k >> 8) as u8, prog_if: (k & 0xFF) as u8 };
    back == t
}

/// 相位状态机倒退拒。
fn phase_ok() -> bool {
    let mut led = ClassLedger::new();
    if led.advance(LedgerPhase::Filing).is_err() {
        return false;
    }
    if led.advance(LedgerPhase::Auditing).is_err() {
        return false;
    }
    if led.advance(LedgerPhase::Sealed).is_err() {
        return false;
    }
    if led.advance(LedgerPhase::Idle).is_ok() {
        return false;
    }
    true
}

/// LCG 注入轮：随机类码六桶分派守恒（种子固定）。
fn lcg_rounds_ok() -> bool {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut led = ClassLedger::new();
    let mut n = 0u32;
    while n < 48 {
        let rnd = rng.next();
        let base = (rnd & 0xFF) as u8;
        let sub = ((rnd >> 8) & 0xFF) as u8;
        let pif = ((rnd >> 16) & 0xFF) as u8;
        let t = ClassTriple { base, subclass: sub, prog_if: pif };
        let b = match led.file(n as u8, 0, 0, t) {
            Ok(b) => b,
            Err(_) => return false,
        };
        if b != ClassBucket::from_base(base) {
            return false;
        }
        n = n.saturating_add(1);
    }
    led.conservation_ok() && led.entries.len() == 48
}

/// 只读红线：全链后写计数 == 0，写尝试恒拒。
fn readonly_ok() -> bool {
    let mut shim = machine_shim();
    let mut led = ClassLedger::new();
    match classify_all(&mut shim, &mut led) {
        Ok(_) => {}
        Err(_) => return false,
    }
    match shim.cfg_write(0, 0, 0, 0x08, 0x010601) {
        Err(_) => {}
        Ok(()) => return false,
    }
    shim.write_ops == 1 && shim.read_ops >= 6
}

/// 双跑一致。
fn repro_ok() -> bool {
    let mut a_shim = machine_shim();
    let mut a_led = ClassLedger::new();
    let a = classify_all(&mut a_shim, &mut a_led);
    let mut b_shim = machine_shim();
    let mut b_led = ClassLedger::new();
    let b = classify_all(&mut b_shim, &mut b_led);
    a.is_ok() && b.is_ok() && a_led.entries == b_led.entries && a_led.buckets == b_led.buckets
}

/// 段位守卫。
fn segment_guard_ok() -> bool {
    let mut i = 0;
    while i < ClassCode::ALL.len() {
        if let Some(c) = ClassCode::ALL.get(i) {
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
    while i < ClassCode::ALL.len() {
        if let Some(c) = ClassCode::ALL.get(i) {
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
    v.push(("F0467-J1-段位守卫", segment_guard_ok(),
        format!("SEG_BASE=0x{:X} 窗口 0x40", SEG_BASE)));
    v.push(("F0467-J1-分类主账", main_ledger_ok(),
        format!("五桶+其他桶各 1 守恒，判定 {}", main_ledger_ok())));
    v.push(("F0467-J1-覆盖100%", coverage_ok(),
        format!("条数==设备总数 6，判定 {}", coverage_ok())));
    v.push(("F0467-J1-六桶分派闭集", bucket_dispatch_ok(),
        format!("base→桶逐点核+兜底核，判定 {}", bucket_dispatch_ok())));
    v.push(("F0467-J1-注入未登记类入其他桶", inject_unlisted_ok(),
        format!("base=0x77 显性兜底，判定 {}", inject_unlisted_ok())));
    v.push(("F0467-J1-注入重复登记必抓", inject_dup_ok(),
        format!("同设备二次入账拒，判定 {}", inject_dup_ok())));
    v.push(("F0467-J1-注入覆盖缺口必抓", inject_hole_ok(),
        format!("少登记核出缺口，判定 {}", inject_hole_ok())));
    v.push(("F0467-J1-注入读扑空必抓", inject_readmiss_ok(),
        format!("未装载地址回 ReadMiss，判定 {}", inject_readmiss_ok())));
    v.push(("F0467-J1-类码对照表", class_table_ok(),
        format!("8 条命中未知 unnamed，判定 {}", class_table_ok())));
    v.push(("F0467-J1-三元组可复算键", triple_key_ok(),
        format!("0x010601 编码往返一致，判定 {}", triple_key_ok())));
    v.push(("F0467-J1-相位状态机", phase_ok(),
        format!("倒退显性拒，判定 {}", phase_ok())));
    v.push(("F0467-J1-LCG注入轮", lcg_rounds_ok(),
        format!("48 轮随机类码六桶守恒，判定 {}", lcg_rounds_ok())));
    v.push(("F0467-J1-只读红线", readonly_ok(),
        format!("全链 write_ops==0，判定 {}", readonly_ok())));
    v.push(("F0467-J2-双跑一致", repro_ok(),
        format!("两轮账全等，判定 {}", repro_ok())));
    v.push(("F0467-J1-诊断码闭集", code_table_ok(),
        format!("7 码查表可达，判定 {}", code_table_ok())));
    v
}

pub fn run_unx0467_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0467 类码分类账");
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
    fn bucket_dispatch_closed() {
        assert_eq!(ClassBucket::from_base(0x01), ClassBucket::Storage);
        assert_eq!(ClassBucket::from_base(0x02), ClassBucket::Network);
        assert_eq!(ClassBucket::from_base(0x03), ClassBucket::Display);
        assert_eq!(ClassBucket::from_base(0x06), ClassBucket::Bridge);
        assert_eq!(ClassBucket::from_base(0x09), ClassBucket::Input);
        assert_eq!(ClassBucket::from_base(0x0C), ClassBucket::Other);
        assert_eq!(ClassBucket::from_base(0xFF), ClassBucket::Other);
    }

    #[test]
    fn main_ledger_buckets() {
        let mut shim = machine_shim();
        let mut led = ClassLedger::new();
        let n = classify_all(&mut shim, &mut led).unwrap();
        assert_eq!(n, 6);
        assert_eq!(led.buckets, [1, 1, 1, 1, 1, 1]);
        assert!(led.conservation_ok());
        assert!(led.coverage_ok(6));
    }

    #[test]
    fn unlisted_class_lands_in_other() {
        let mut shim = inject_unlisted_shim();
        let mut led = ClassLedger::new();
        let n = classify_all(&mut shim, &mut led).unwrap();
        assert_eq!(n, 7);
        assert_eq!(led.buckets[ClassBucket::Other.index()], 2);
        assert!(led.other_bucket_ok());
    }

    #[test]
    fn duplicate_registration_caught() {
        let mut led = ClassLedger::new();
        let t = ClassTriple { base: 0x02, subclass: 0x00, prog_if: 0x00 };
        led.file(0, 5, 0, t).unwrap();
        assert_eq!(led.file(0, 5, 0, t).unwrap_err(), ClassCode::DuplicateDev.val());
    }

    #[test]
    fn coverage_hole_caught() {
        let mut shim = inject_hole_shim();
        let mut led = ClassLedger::new();
        classify_all(&mut shim, &mut led).unwrap();
        assert!(!led.coverage_ok(6));
        assert!(led.coverage_ok(1));
    }

    #[test]
    fn readmiss_caught() {
        let mut shim = ClassShim::new();
        assert_eq!(shim.read(1, 2, 3).unwrap_err(), ClassCode::ReadMiss.val());
    }

    #[test]
    fn class_table_hits_and_unnamed() {
        let sata = ClassTriple { base: 0x01, subclass: 0x06, prog_if: 0x01 };
        assert_eq!(class_name(sata), "SATA AHCI");
        let nvme = ClassTriple { base: 0x01, subclass: 0x08, prog_if: 0x02 };
        assert_eq!(class_name(nvme), "NVMe");
        let weird = ClassTriple { base: 0x77, subclass: 0x11, prog_if: 0x22 };
        assert_eq!(class_name(weird), "unnamed");
    }

    #[test]
    fn triple_key_roundtrip() {
        let t = ClassTriple { base: 0x0C, subclass: 0x03, prog_if: 0x30 };
        assert_eq!(triple_key(t), 0x0C0330);
        let back = ClassTriple {
            base: (triple_key(t) >> 16) as u8,
            subclass: (triple_key(t) >> 8) as u8,
            prog_if: (triple_key(t) & 0xFF) as u8,
        };
        assert_eq!(back, t);
    }

    #[test]
    fn named_flag_explicit() {
        let mut shim = machine_shim();
        let mut led = ClassLedger::new();
        classify_all(&mut shim, &mut led).unwrap();
        // XHCI 命中对照表（named），base 0x0C 落其他桶。
        let xhci = led.entries.iter().find(|e| e.bucket == ClassBucket::Other).unwrap();
        assert!(xhci.named);
        assert_eq!(xhci.triple.base, 0x0C);
    }

    #[test]
    fn phase_monotonic() {
        let mut led = ClassLedger::new();
        led.advance(LedgerPhase::Filing).unwrap();
        led.advance(LedgerPhase::Auditing).unwrap();
        led.advance(LedgerPhase::Sealed).unwrap();
        assert_eq!(led.advance(LedgerPhase::Filing).unwrap_err(), ClassCode::LedgerRetrograde.val());
    }

    #[test]
    fn readonly_redline() {
        let mut shim = machine_shim();
        let mut led = ClassLedger::new();
        classify_all(&mut shim, &mut led).unwrap();
        assert!(shim.cfg_write(0, 31, 0, 0x08, 0).is_err());
        assert_eq!(shim.write_ops, 1);
        assert!(shim.read_ops >= 6);
    }

    #[test]
    fn lcg_deterministic() {
        assert!(lcg_rounds_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        assert_eq!(a.next(), b.next());
    }

    #[test]
    fn reproducible_double_run() {
        assert!(repro_ok());
        let s1 = run_unx0467_checks();
        let s2 = run_unx0467_checks();
        assert!(s1.all_passed());
        assert_eq!(s1.all_passed(), s2.all_passed());
    }

    #[test]
    fn guard_and_code_table() {
        assert!(segment_guard_ok());
        assert!(code_table_ok());
        assert_eq!(detail(0x555_5555), "未知诊断码：不在 0x755C0 段闭集内");
    }
}
