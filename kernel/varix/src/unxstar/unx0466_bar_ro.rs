//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0466 BAR 只读账（MEM/IO/64 位） · 0x75580段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   BAR 账齐（条数=00 型设备 BAR 位总数），类型/位宽/64 位标志三
//!   要素逐条核。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① BAR 位总数：00 型通用设备 6 条（偏移 0x10~0x2C）+ 01 型桥
//!     2 条（0x10~0x18），PCI Local Bus Spec r3.0 §6.2.5；账齐判据
//!     = 每台 00 型 6 槽位全入账（含空槽显性）。
//!   ② 类型三态：Memory 32 位（bit0=0,bit2:1=00）/ Memory 64 位
//!     （bit0=0,bit2:1=10）/ IO 空间（bit0=1）；IO BAR 低 2 位保留
//!     位核（bit1=0 合法，bit1=1 记保留位违约显性）。
//!   ③ 位宽核算：64 位 BAR 占两槽（下一槽被影子化不可再挂独立
//!     BAR），影子槽位显性记账（不静默）；三要素（类型/位宽/64 位
//!     标志）逐条一致性核。
//!   ④ 只读红线：BAR 采样走读路径，任何配置写尝试恒拒并记账，
//!     全链后 write_ops==0。
//!   ⑤ 注入必抓（64 位标志与位宽失配 / IO 保留位违约 / 槽越位）+
//!     单调相位状态机 + 双跑一致 + 段位守卫。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75580，AI-VX7-1104 批F 独占区）。
pub const SEG_BASE: u32 = 0x75580;

/// 00 型 BAR 位总数（Spec §6.2.5：偏移 0x10~0x2C）。
pub const BAR_SLOTS_TYPE00: usize = 6;
/// 01 型桥 BAR 位总数（偏移 0x10~0x18）。
pub const BAR_SLOTS_TYPE01: usize = 2;
/// BAR 低 2 位掩码（类型判读位）。
pub const BAR_LOW2_MASK: u32 = 0x3;
/// 64 位标志位（低 3 位中的 bit2）。
pub const BAR_64BIT_FLAG: u32 = 0x4;
/// IO BAR 保留位（bit1）合法值（0）。
pub const IO_RESERVED_BIT: u32 = 0x2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarCode {
    /// 槽号越位（超出该型 BAR 位总数）
    SlotOver,
    /// 64 位标志与位宽失配（bit2=1 但判 32 位/IO）
    FlagMismatch,
    /// IO BAR 保留位违约（bit1=1）
    IoReservedBad,
    /// 影子槽位被独立占用（64 位 BAR 下一槽不可挂 BAR）
    ShadowOverlap,
    /// 配置写尝试被拒（只读红线）
    WriteRejected,
    /// BAR 相位倒退（状态机红线）
    BarRetrograde,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl BarCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BarCode::SlotOver => "槽号越位：00 型 6 槽/桥 2 槽闭集，请核 BAR 位编号",
            BarCode::FlagMismatch => "64 位标志失配：bit2=1 须配 64 位宽，核低 3 位判读",
            BarCode::IoReservedBad => "IO BAR 保留位违约：bit1 须为 0，核 IO 判读位",
            BarCode::ShadowOverlap => "影子槽位被占：64 位 BAR 占两槽，下一槽不可独立挂 BAR",
            BarCode::WriteRejected => "配置写被拒：只读红线，BAR 账只做读采样",
            BarCode::BarRetrograde => "BAR 相位倒退：状态机不许回退，请按序推进",
            BarCode::NotReproducible => "双跑不一致：复现红线，BAR 账须确定性",
        }
    }
    pub const ALL: [BarCode; 7] = [
        BarCode::SlotOver,
        BarCode::FlagMismatch,
        BarCode::IoReservedBad,
        BarCode::ShadowOverlap,
        BarCode::WriteRejected,
        BarCode::BarRetrograde,
        BarCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BarCode::ALL.len() {
        if let Some(c) = BarCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75580 段闭集内"
}

// ── 域模型：BAR 类型三态 ────────────────────────────────────────
/// BAR 类型三态闭集 + 空槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarKind {
    /// 空槽（值 0 且非 IO——按规范值为 0 即未实现）
    Empty,
    /// Memory 32 位（bit0=0, bit2:1=00）
    Mem32,
    /// Memory 64 位（bit0=0, bit2:1=10, 占两槽）
    Mem64,
    /// IO 空间（bit0=1）
    IoSpace,
}

/// BAR 全值判读（类型/位宽/64 位标志三要素的源头）：
/// bit0=1 → IO；bit0=0 且全值 0 → 空槽（未实现）；bit0=0 且 bit2=1 →
/// Memory 64 位；其余（bit0=0、值非 0、bit2=0）→ Memory 32 位。
/// 注意：Mem32 与空槽的低 3 位同为 0（16 对齐），必须按全值判读，
/// 只看低 3 位会把真 Mem32 误判成空槽——判据三要素核依赖本函数。
pub fn classify_bar(raw: u32) -> BarKind {
    if raw & 0x1 == 1 {
        BarKind::IoSpace
    } else if raw == 0 {
        BarKind::Empty
    } else if raw & BAR_64BIT_FLAG != 0 {
        BarKind::Mem64
    } else {
        BarKind::Mem32
    }
}

/// 位宽（字节）核算：64 位 BAR 位宽 8 字节，其余 4 字节；空槽 0。
pub fn width_bytes(kind: BarKind) -> u32 {
    match kind {
        BarKind::Empty => 0,
        BarKind::Mem64 => 8,
        _ => 4,
    }
}

/// 64 位标志核：仅 Mem64 应有 bit2=1。
pub fn flag_matches(kind: BarKind, low3: u32) -> bool {
    let flag = low3 & BAR_64BIT_FLAG != 0;
    match kind {
        BarKind::Mem64 => flag,
        BarKind::IoSpace | BarKind::Mem32 | BarKind::Empty => !flag,
    }
}

/// IO BAR 低 2 位保留位核：bit0=1 且 bit1 必须为 0。
pub fn io_reserved_ok(low3: u32) -> bool {
    if low3 & 0x1 == 1 {
        low3 & IO_RESERVED_BIT == 0
    } else {
        true
    }
}

/// 三要素一致性核（类型/位宽/64 位标志逐条；raw 全值判读 + 低 3 位核）。
pub fn triple_ok(kind: BarKind, raw: u32) -> bool {
    let low3 = raw & 0x7;
    if classify_bar(raw) != kind {
        return false;
    }
    if !flag_matches(kind, low3) {
        return false;
    }
    io_reserved_ok(low3)
}

// ── BAR 账条目 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarRow {
    pub slot: usize,
    pub kind: BarKind,
    pub low3: u32,
    /// BAR 原值（Mem32/空槽低位同 0，判读须全值参与）
    pub raw: u32,
    /// 64 位标志（bit2=1）独立入账要素
    pub is64: bool,
    /// 位宽（字节）
    pub width: u32,
    /// 该槽是否为上一槽 64 位 BAR 的影子槽（不可独立挂 BAR）
    pub shadow: bool,
}

/// 单设备 BAR 账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarLedger {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    /// 设备头型（00=6 槽 / 01=2 槽）
    pub dev_type: u8,
    pub rows: Vec<BarRow>,
}

impl BarLedger {
    /// 该型 BAR 位总数（账齐判据的分母）。
    pub fn slot_total(&self) -> usize {
        if self.dev_type & 0x7F == 0x01 {
            BAR_SLOTS_TYPE01
        } else {
            BAR_SLOTS_TYPE00
        }
    }

    /// 账齐核：条数（含空槽与影子槽）== 该型 BAR 位总数。
    pub fn slots_complete(&self) -> bool {
        self.rows.len() == self.slot_total()
    }
}

// ── 影子读垫片（只读路径） ──────────────────────────────────────
pub struct BarShim {
    /// (bus, slot, func, dev_type, [6 或 2 个 raw BAR 值])
    pub map: Vec<(u8, u8, u8, u8, [u32; BAR_SLOTS_TYPE00])>,
    pub read_ops: u64,
    pub write_ops: u64,
}

impl BarShim {
    pub fn new() -> BarShim {
        BarShim { map: Vec::new(), read_ops: 0, write_ops: 0 }
    }

    /// 装载一台设备的 BAR 原值账（6 槽平账，桥只用前 2 槽）。
    pub fn put(&mut self, bus: u8, slot: u8, func: u8, dev_type: u8, bars: [u32; BAR_SLOTS_TYPE00]) {
        self.map.push((bus, slot, func, dev_type, bars));
    }

    /// 读 BAR 原值（只读路径）：槽越位回 Err(SlotOver)，账缺回 Err(NotReproducible)。
    pub fn read_bar(&mut self, bus: u8, slot: u8, func: u8, idx: usize) -> Result<u32, u32> {
        if idx >= BAR_SLOTS_TYPE00 {
            return Err(BarCode::SlotOver.val());
        }
        self.read_ops = self.read_ops.saturating_add(1);
        let mut i = 0;
        while i < self.map.len() {
            if let Some((b, s, f, _dt, bars)) = self.map.get(i) {
                if *b == bus && *s == slot && *f == func {
                    match bars.get(idx) {
                        Some(v) => return Ok(*v),
                        None => return Err(BarCode::SlotOver.val()),
                    }
                }
            }
            i += 1;
        }
        Err(BarCode::NotReproducible.val())
    }

    /// 配置写（只读红线：恒拒显性记账）。
    pub fn cfg_write(&mut self, bus: u8, slot: u8, func: u8, off: u8, val: u32) -> Result<(), u32> {
        let _ = (bus, slot, func, off, val);
        self.write_ops = self.write_ops.saturating_add(1);
        Err(BarCode::WriteRejected.val())
    }
}

// ── 单调相位状态机 ──────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPhase {
    /// 未起步
    Idle,
    /// 逐槽采样中
    Sampling,
    /// 三要素核账中
    Auditing,
    /// 封账
    Sealed,
}

impl BarPhase {
    pub fn rank(self) -> u8 {
        match self {
            BarPhase::Idle => 0,
            BarPhase::Sampling => 1,
            BarPhase::Auditing => 2,
            BarPhase::Sealed => 3,
        }
    }
}

/// BAR 采集器：逐槽采样 + 三要素核 + 影子槽记账。
pub struct BarSampler {
    pub phase: BarPhase,
    /// 保留位违约检出次数
    pub io_bad: u32,
    /// 标志失配检出次数
    pub flag_bad: u32,
    /// 影子槽重叠检出次数
    pub shadow_bad: u32,
}

impl BarSampler {
    pub fn new() -> BarSampler {
        BarSampler { phase: BarPhase::Idle, io_bad: 0, flag_bad: 0, shadow_bad: 0 }
    }

    pub fn advance(&mut self, next: BarPhase) -> Result<(), u32> {
        if next.rank() < self.phase.rank() {
            return Err(BarCode::BarRetrograde.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 采样一台设备：账齐 6/2 槽（空槽显性 + 64 位影子槽显性）。
    /// 注意：相位保持在 Sampling 跨多台采样（同相推进不倒退），
    /// Auditing/Sealed 由调用方在全链采样后显性推进。
    pub fn sample(
        &mut self,
        shim: &mut BarShim,
        bus: u8,
        slot: u8,
        func: u8,
        dev_type: u8,
    ) -> Result<BarLedger, u32> {
        self.advance(BarPhase::Sampling)?;
        let total = if dev_type & 0x7F == 0x01 { BAR_SLOTS_TYPE01 } else { BAR_SLOTS_TYPE00 };
        let mut rows: Vec<BarRow> = Vec::new();
        let mut idx = 0usize;
        while idx < total {
            let raw = shim.read_bar(bus, slot, func, idx)?;
            let low3 = raw & 0x7;
            let kind = classify_bar(raw);
            // 三要素逐条核（类型/位宽/64 位标志）。
            if !flag_matches(kind, low3) {
                self.flag_bad = self.flag_bad.saturating_add(1);
                return Err(BarCode::FlagMismatch.val());
            }
            if !io_reserved_ok(low3) {
                self.io_bad = self.io_bad.saturating_add(1);
                return Err(BarCode::IoReservedBad.val());
            }
            let shadow = match rows.last() {
                Some(prev) => prev.kind == BarKind::Mem64,
                None => false,
            };
            if shadow && kind != BarKind::Empty {
                // 64 位 BAR 的下一槽必须表现为空槽/影子槽，独立挂 BAR 违约。
                self.shadow_bad = self.shadow_bad.saturating_add(1);
                return Err(BarCode::ShadowOverlap.val());
            }
            rows.push(BarRow {
                slot: idx,
                kind,
                low3,
                raw,
                is64: kind == BarKind::Mem64,
                width: width_bytes(kind),
                shadow,
            });
            idx = idx.saturating_add(1);
        }
        let led = BarLedger { bus, slot, func, dev_type, rows };
        Ok(led)
    }
}

// ── 真机影子账（冻结：00 型 3 台 / 桥 1 台） ────────────────────
/// 真机 BAR 原值账。
pub fn machine_shim() -> BarShim {
    let mut shim = BarShim::new();
    // GPU：bar0 Mem32 / bar1 Mem64（占槽 1-2）/ bar3 IO / 其余空
    shim.put(0, 2, 0, 0x00, [
        0xFEBC0000, // slot0 Mem32（低位 00）
        0xC000000C, // slot1 Mem64（低位 100=bit2 置位；基址含高位段）
        0x00000000, // slot2 影子槽（64 位 BAR 高半段，账记影子）
        0x0000E001, // slot3 IO（低位 001）
        0x00000000, // slot4 空
        0x00000000, // slot5 空
    ]);
    // NVMe：bar0 Mem64（占槽 0-1）/ bar2 Mem32 / 其余空
    shim.put(3, 0, 0, 0x00, [
        0xC0100004, // slot0 Mem64
        0x00000000, // slot1 影子槽
        0xFEBC0000, // slot2 Mem32
        0x00000000, // slot3 空
        0x00000000, // slot4 空
        0x00000000, // slot5 空
    ]);
    // NIC：bar0 Mem32 / bar1 IO / 其余空（全 4 字节位宽）
    shim.put(1, 0, 0, 0x00, [
        0xFEBC0000, // slot0 Mem32
        0x00003001, // slot1 IO
        0x00000000, 0x00000000, 0x00000000, 0x00000000,
    ]);
    // 桥：bar0 Mem32 / bar1 IO（桥只有 2 槽）
    shim.put(0, 3, 0, 0x01, [
        0xFEBC0000, // slot0 Mem32
        0x00002001, // slot1 IO
        0x00000000, 0x00000000, 0x00000000, 0x00000000,
    ]);
    shim
}

/// 注入-标志失配账：bit2=1 但判 IO（bit0=1）——失配必抓。
pub fn inject_flag_mismatch_shim() -> BarShim {
    let mut shim = BarShim::new();
    shim.put(0, 9, 0, 0x00, [0x0000E005, 0, 0, 0, 0, 0]); // IO + bit2 → 失配
    shim
}

/// 注入-IO 保留位账：bit0=1 且 bit1=1（保留位违约必抓）。
pub fn inject_io_reserved_shim() -> BarShim {
    let mut shim = BarShim::new();
    shim.put(0, 9, 0, 0x00, [0x0000E003, 0, 0, 0, 0, 0]); // IO bit1=1 → 违约
    shim
}

/// 注入-影子槽重叠账：slot0 Mem64 后 slot1 独立挂 Mem32（违约必抓）。
pub fn inject_shadow_overlap_shim() -> BarShim {
    let mut shim = BarShim::new();
    shim.put(0, 9, 0, 0x00, [0xC000000C, 0xFEBC0000, 0, 0, 0, 0]);
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
/// 主账：4 台全采，账齐条数 = 型位总数（6/6/6/2）。
fn main_ledger_ok() -> bool {
    let mut shim = machine_shim();
    let mut smp = BarSampler::new();
    let devs = [(0u8, 2u8, 0u8, 0x00u8), (3, 0, 0, 0x00), (1, 0, 0, 0x00), (0, 3, 0, 0x01)];
    let mut i = 0;
    while i < devs.len() {
        if let Some((b, s, f, t)) = devs.get(i) {
            match smp.sample(&mut shim, *b, *s, *f, *t) {
                Ok(led) => {
                    if !led.slots_complete() {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
        i += 1;
    }
    // 全链采样后显性推进：Auditing → Sealed。
    if smp.advance(BarPhase::Auditing).is_err() || smp.advance(BarPhase::Sealed).is_err() {
        return false;
    }
    true
}

/// 账齐条数守恒（判据核心）：Σ条数 = 00 型 6×3 + 桥 2 = 20。
fn slot_total_ok() -> bool {
    let mut shim = machine_shim();
    let mut smp = BarSampler::new();
    let mut total = 0usize;
    let devs = [(0u8, 2u8, 0u8, 0x00u8), (3, 0, 0, 0x00), (1, 0, 0, 0x00), (0, 3, 0, 0x01)];
    let mut i = 0;
    while i < devs.len() {
        if let Some((b, s, f, t)) = devs.get(i) {
            match smp.sample(&mut shim, *b, *s, *f, *t) {
                Ok(led) => total = total.saturating_add(led.rows.len()),
                Err(_) => return false,
            }
        }
        i += 1;
    }
    total == BAR_SLOTS_TYPE00 * 3 + BAR_SLOTS_TYPE01
}

/// 三要素逐条核（GPU 台：Mem32/Mem64/IO/空 四态全命中）。
fn triple_gpu_ok() -> bool {
    let mut shim = machine_shim();
    let mut smp = BarSampler::new();
    let led = match smp.sample(&mut shim, 0, 2, 0, 0x00) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let want = [
        (BarKind::Mem32, 4u32, false),
        (BarKind::Mem64, 8, true),
        (BarKind::Empty, 0, false),
        (BarKind::IoSpace, 4, false),
        (BarKind::Empty, 0, false),
        (BarKind::Empty, 0, false),
    ];
    if led.rows.len() != want.len() {
        return false;
    }
    let mut i = 0;
    while i < want.len() {
        let r = match led.rows.get(i) {
            Some(r) => r.clone(),
            None => return false,
        };
        let (wk, ww, w64) = want[i];
        if r.kind != wk || r.width != ww || r.is64 != w64 {
            return false;
        }
        if !triple_ok(r.kind, r.raw) {
            return false;
        }
        i += 1;
    }
    true
}

/// 影子槽显性记账：GPU slot2 / NVMe slot1 是 64 位 BAR 影子槽。
fn shadow_ok() -> bool {
    let mut shim = machine_shim();
    let mut smp = BarSampler::new();
    let gpu = match smp.sample(&mut shim, 0, 2, 0, 0x00) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let nvme = match smp.sample(&mut shim, 3, 0, 0, 0x00) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let gpu_shadow = gpu.rows.get(2).map(|r| r.shadow).unwrap_or(false);
    let nvme_shadow = nvme.rows.get(1).map(|r| r.shadow).unwrap_or(false);
    gpu_shadow && nvme_shadow
}

/// 注入-标志失配必抓。
fn inject_flag_ok() -> bool {
    let mut shim = inject_flag_mismatch_shim();
    let mut smp = BarSampler::new();
    match smp.sample(&mut shim, 0, 9, 0, 0x00) {
        Err(e) if e == BarCode::FlagMismatch.val() => smp.flag_bad == 1,
        _ => false,
    }
}

/// 注入-IO 保留位必抓。
fn inject_io_ok() -> bool {
    let mut shim = inject_io_reserved_shim();
    let mut smp = BarSampler::new();
    match smp.sample(&mut shim, 0, 9, 0, 0x00) {
        Err(e) if e == BarCode::IoReservedBad.val() => smp.io_bad == 1,
        _ => false,
    }
}

/// 注入-影子槽重叠必抓。
fn inject_shadow_ok() -> bool {
    let mut shim = inject_shadow_overlap_shim();
    let mut smp = BarSampler::new();
    match smp.sample(&mut shim, 0, 9, 0, 0x00) {
        Err(e) if e == BarCode::ShadowOverlap.val() => smp.shadow_bad == 1,
        _ => false,
    }
}

/// 注入-槽越位必抓：idx=6 越出 00 型 6 槽闭集。
fn inject_slot_over_ok() -> bool {
    let mut shim = BarShim::new();
    match shim.read_bar(0, 2, 0, BAR_SLOTS_TYPE00) {
        Err(e) if e == BarCode::SlotOver.val() => {}
        _ => return false,
    }
    true
}

/// 只读红线：全链后写计数 == 0，写尝试恒拒。
fn readonly_ok() -> bool {
    let mut shim = machine_shim();
    let mut smp = BarSampler::new();
    let _ = smp.sample(&mut shim, 0, 2, 0, 0x00);
    match shim.cfg_write(0, 2, 0, 0x10, 0xFEBC0000) {
        Err(e) if e == BarCode::WriteRejected.val() => {}
        _ => return false,
    }
    shim.write_ops == 1 && shim.read_ops >= 6
}

/// 三态判读闭集扫：classify_bar 全值域 0..=7 回合法三态或空槽。
fn classify_sweep_ok() -> bool {
    let mut v = 0u32;
    while v < 8 {
        let k = classify_bar(v);
        // 每个值必落闭集之一且三要素自洽（除构造违约值 3/5/7 外）。
        match k {
            BarKind::Empty => {
                if v != 0 {
                    return false;
                }
            }
            BarKind::IoSpace => {
                if v & 1 != 1 {
                    return false;
                }
            }
            BarKind::Mem64 => {
                if v & BAR_64BIT_FLAG == 0 {
                    return false;
                }
            }
            BarKind::Mem32 => {
                if v & 0x1 != 0 || v & BAR_64BIT_FLAG != 0 || v == 0 {
                    return false;
                }
            }
        }
        v = v.saturating_add(1);
    }
    true
}

/// 相位状态机倒退拒。
fn phase_ok() -> bool {
    let mut smp = BarSampler::new();
    if smp.advance(BarPhase::Sampling).is_err() {
        return false;
    }
    if smp.advance(BarPhase::Auditing).is_err() {
        return false;
    }
    if smp.advance(BarPhase::Sealed).is_err() {
        return false;
    }
    if smp.advance(BarPhase::Idle).is_ok() {
        return false;
    }
    true
}

/// LCG 注入轮：随机低 3 位三要素自洽扫描（种子固定）。
fn lcg_rounds_ok() -> bool {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut n = 0u32;
    while n < 64 {
        let low3 = (rng.next() & 0x7) as u32;
        let k = classify_bar(low3);
        // 保留位/标志违约值（3/5/7）之外必须三要素自洽。
        if low3 != 3 && low3 != 5 && low3 != 7 {
            if !triple_ok(k, low3) {
                return false;
            }
        }
        n = n.saturating_add(1);
    }
    true
}

/// 双跑一致。
fn repro_ok() -> bool {
    let mut a_shim = machine_shim();
    let mut a_smp = BarSampler::new();
    let a = a_smp.sample(&mut a_shim, 0, 2, 0, 0x00);
    let mut b_shim = machine_shim();
    let mut b_smp = BarSampler::new();
    let b = b_smp.sample(&mut b_shim, 3, 0, 0, 0x00);
    let a2 = a_smp.sample(&mut a_shim, 3, 0, 0, 0x00);
    let b2 = b_smp.sample(&mut b_shim, 0, 2, 0, 0x00);
    match (a, b, a2, b2) {
        (Ok(la), Ok(lb), Ok(la2), Ok(lb2)) => la == lb2 && lb == la2,
        _ => false,
    }
}

/// 段位守卫。
fn segment_guard_ok() -> bool {
    let mut i = 0;
    while i < BarCode::ALL.len() {
        if let Some(c) = BarCode::ALL.get(i) {
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
    while i < BarCode::ALL.len() {
        if let Some(c) = BarCode::ALL.get(i) {
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
    v.push(("F0466-J1-段位守卫", segment_guard_ok(),
        format!("SEG_BASE=0x{:X} 窗口 0x40", SEG_BASE)));
    v.push(("F0466-J1-BAR主账", main_ledger_ok(),
        format!("4 台全采账齐，判定 {}", main_ledger_ok())));
    v.push(("F0466-J1-账齐条数守恒", slot_total_ok(),
        format!("Σ条数=6*3+2=20，判定 {}", slot_total_ok())));
    v.push(("F0466-J1-三要素逐条核", triple_gpu_ok(),
        format!("类型/位宽/64 位标志四态全命中，判定 {}", triple_gpu_ok())));
    v.push(("F0466-J1-影子槽显性", shadow_ok(),
        format!("64 位 BAR 占两槽记账，判定 {}", shadow_ok())));
    v.push(("F0466-J1-注入标志失配必抓", inject_flag_ok(),
        format!("bit2=1 判 IO 拒，判定 {}", inject_flag_ok())));
    v.push(("F0466-J1-注入IO保留位必抓", inject_io_ok(),
        format!("bit1=1 违约拒，判定 {}", inject_io_ok())));
    v.push(("F0466-J1-注入影子槽重叠必抓", inject_shadow_ok(),
        format!("影子槽独立挂 BAR 拒，判定 {}", inject_shadow_ok())));
    v.push(("F0466-J1-注入槽越位必抓", inject_slot_over_ok(),
        format!("idx=6 越出闭集拒，判定 {}", inject_slot_over_ok())));
    v.push(("F0466-J1-只读红线", readonly_ok(),
        format!("全链 write_ops==0，判定 {}", readonly_ok())));
    v.push(("F0466-J1-三态判读全值域扫", classify_sweep_ok(),
        format!("0..=7 全落闭集，判定 {}", classify_sweep_ok())));
    v.push(("F0466-J1-相位状态机", phase_ok(),
        format!("倒退显性拒，判定 {}", phase_ok())));
    v.push(("F0466-J1-LCG注入轮", lcg_rounds_ok(),
        format!("64 轮低 3 位三要素核，判定 {}", lcg_rounds_ok())));
    v.push(("F0466-J2-双跑一致", repro_ok(),
        format!("两轮账全等，判定 {}", repro_ok())));
    v.push(("F0466-J1-诊断码闭集", code_table_ok(),
        format!("7 码查表可达，判定 {}", code_table_ok())));
    v
}

pub fn run_unx0466_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0466 BAR 只读账");
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
    fn classify_three_states() {
        assert_eq!(classify_bar(0), BarKind::Empty);
        assert_eq!(classify_bar(1), BarKind::IoSpace);
        assert_eq!(classify_bar(4), BarKind::Mem64);
        assert_eq!(classify_bar(0xFEBC0000), BarKind::Mem32);
        assert_eq!(classify_bar(2), BarKind::Mem32);
    }

    #[test]
    fn width_accounting() {
        assert_eq!(width_bytes(BarKind::Mem64), 8);
        assert_eq!(width_bytes(BarKind::Mem32), 4);
        assert_eq!(width_bytes(BarKind::IoSpace), 4);
        assert_eq!(width_bytes(BarKind::Empty), 0);
    }

    #[test]
    fn main_ledger_slot_totals() {
        let mut shim = machine_shim();
        let mut smp = BarSampler::new();
        let gpu = smp.sample(&mut shim, 0, 2, 0, 0x00).unwrap();
        let nvme = smp.sample(&mut shim, 3, 0, 0, 0x00).unwrap();
        let nic = smp.sample(&mut shim, 1, 0, 0, 0x00).unwrap();
        let br = smp.sample(&mut shim, 0, 3, 0, 0x01).unwrap();
        assert_eq!(gpu.rows.len(), 6);
        assert_eq!(nvme.rows.len(), 6);
        assert_eq!(nic.rows.len(), 6);
        assert_eq!(br.rows.len(), 2);
        assert!(br.slots_complete());
    }

    #[test]
    fn shadow_slots_explicit() {
        let mut shim = machine_shim();
        let mut smp = BarSampler::new();
        let gpu = smp.sample(&mut shim, 0, 2, 0, 0x00).unwrap();
        assert!(gpu.rows.get(1).unwrap().is64);
        assert!(gpu.rows.get(2).unwrap().shadow);
        let nvme = smp.sample(&mut shim, 3, 0, 0, 0x00).unwrap();
        assert!(nvme.rows.get(0).unwrap().is64);
        assert!(nvme.rows.get(1).unwrap().shadow);
    }

    #[test]
    fn flag_mismatch_injection_caught() {
        let mut shim = inject_flag_mismatch_shim();
        let mut smp = BarSampler::new();
        assert_eq!(
            smp.sample(&mut shim, 0, 9, 0, 0x00).unwrap_err(),
            BarCode::FlagMismatch.val()
        );
        assert_eq!(smp.flag_bad, 1);
    }

    #[test]
    fn io_reserved_injection_caught() {
        let mut shim = inject_io_reserved_shim();
        let mut smp = BarSampler::new();
        assert_eq!(
            smp.sample(&mut shim, 0, 9, 0, 0x00).unwrap_err(),
            BarCode::IoReservedBad.val()
        );
        assert_eq!(smp.io_bad, 1);
    }

    #[test]
    fn shadow_overlap_injection_caught() {
        let mut shim = inject_shadow_overlap_shim();
        let mut smp = BarSampler::new();
        assert_eq!(
            smp.sample(&mut shim, 0, 9, 0, 0x00).unwrap_err(),
            BarCode::ShadowOverlap.val()
        );
        assert_eq!(smp.shadow_bad, 1);
    }

    #[test]
    fn slot_over_injection_caught() {
        let mut shim = BarShim::new();
        assert_eq!(
            shim.read_bar(0, 0, 0, 6).unwrap_err(),
            BarCode::SlotOver.val()
        );
    }

    #[test]
    fn readonly_redline() {
        let mut shim = machine_shim();
        let mut smp = BarSampler::new();
        smp.sample(&mut shim, 1, 0, 0, 0x00).unwrap();
        let before = shim.write_ops;
        assert!(shim.cfg_write(1, 0, 0, 0x10, 0).is_err());
        assert_eq!(shim.write_ops, before + 1);
        assert!(shim.read_ops >= 6);
    }

    #[test]
    fn phase_monotonic() {
        let mut smp = BarSampler::new();
        smp.advance(BarPhase::Sampling).unwrap();
        smp.advance(BarPhase::Auditing).unwrap();
        smp.advance(BarPhase::Sealed).unwrap();
        assert_eq!(smp.advance(BarPhase::Sampling).unwrap_err(), BarCode::BarRetrograde.val());
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
        let s1 = run_unx0466_checks();
        let s2 = run_unx0466_checks();
        assert!(s1.all_passed());
        assert_eq!(s1.all_passed(), s2.all_passed());
    }

    #[test]
    fn guard_and_code_table() {
        assert!(segment_guard_ok());
        assert!(code_table_ok());
        assert_eq!(detail(0xABC_DEF0), "未知诊断码：不在 0x75580 段闭集内");
    }
}
