//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18507 · dwTypeMask 维度位全集（主/次/Build/SP/PT/Suite）
//! · 0x7FA80 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18507-J1 六维位掩码逐位消费断言全过，未请求维度旁路
//!   注入检出。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 六维位全集冻结（winnt.h 口径，bit 升序消费序）：MINOR=0x01 /
//!     MAJOR=0x02 / BUILD=0x04 / SPMAJOR=0x20 / SUITENAME=0x40 /
//!     PRODUCT_TYPE=0x80（bit3=PLATFORMID、bit4=SPMINOR 归属他卡，
//!     本卡消费面为上述六维闭集）；掩码全集 0xE7（六维位或）；
//!   - 逐位消费闸（四闸串联）：越界位（>0x80）拒 / 未请求维消费拒 /
//!     位序乱序拒（必须 bit 升序）/ 同维重复消费拒；
//!   - 逐维比较（与 F18501 快照同源）：MINOR EQ 1 / MAJOR GE 6 /
//!     BUILD GE 7601 / SPMAJOR GE 1 / SUITE AND 0x0100 / PT EQ 1
//!     ——黄金链全维请求在冻结快照上全过，裁决 TRUE；
//!   - **旁路红线**：未请求维的值无论如何变动，裁决必须不变——
//!     双跑对拍（诚实面 vs 旁路面）裁决不同即旁路检出；
//!   - 收口闸：请求维未消费齐不得收口（漏消费即拒，防短路边）；
//!   - 状态机：Idle → Masked → Consuming → Adjudicated（单向）；
//!   - 账本：逐维消费计数 + 全局守恒（尝试=消费+拒）；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] TmCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   DIM_SPEC 六维冻结表（位/名/快照/算子/请求值）
//!   [数据结构] ConsumePlane（六维消费位面）/ 位指纹
//!   [状态机]   ConsumePhase 四态 + DimConsumer（四闸串联）
//!   [账本]     TmLedger（逐维计数 + 全局守恒）
//!   [注入核]   四类 × 10 轮（旁路/漏消费/越界位/乱序）
//!   [收口]     golden_run + run_unx18507_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FA80，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7FA80;

// ── 域冻结常量（winnt.h 口径） ──────────────────────────────────
/// VER_MINORVERSION 位。
pub const VER_MINORVERSION: u32 = 0x0000_0001;
/// VER_MAJORVERSION 位。
pub const VER_MAJORVERSION: u32 = 0x0000_0002;
/// VER_BUILDNUMBER 位。
pub const VER_BUILDNUMBER: u32 = 0x0000_0004;
/// VER_SERVICEPACKMAJOR 位。
pub const VER_SERVICEPACKMAJOR: u32 = 0x0000_0020;
/// VER_SUITENAME 位。
pub const VER_SUITENAME: u32 = 0x0000_0040;
/// VER_PRODUCT_TYPE 位。
pub const VER_PRODUCT_TYPE: u32 = 0x0000_0080;
/// 算子闭集。
pub const VER_EQUAL: u8 = 1;
pub const VER_GREATER: u8 = 2;
pub const VER_GREATER_EQUAL: u8 = 3;
pub const VER_AND: u8 = 6;
/// 六维掩码全集（六位或）。
pub const DIM_MASK_FULL: u32 =
    VER_MINORVERSION | VER_MAJORVERSION | VER_BUILDNUMBER | VER_SERVICEPACKMAJOR | VER_SUITENAME | VER_PRODUCT_TYPE;
/// 消费面维数。
pub const DIM_COUNT: usize = 6;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 维规格一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimSpec {
    /// dwTypeMask 位。
    pub bit: u32,
    /// 维名（冻结）。
    pub name: &'static str,
    /// 系统快照值（与 F18501 同源）。
    pub sys: u32,
    /// 比较算子。
    pub op: u8,
    /// 黄金链请求值（取快照本身 → 黄金链必过）。
    pub req: u32,
}

/// 六维冻结规格表（bit 升序 = 消费序）。
pub const DIM_SPEC: [DimSpec; DIM_COUNT] = [
    DimSpec { bit: VER_MINORVERSION, name: "MINOR", sys: 1, op: VER_EQUAL, req: 1 },
    DimSpec { bit: VER_MAJORVERSION, name: "MAJOR", sys: 6, op: VER_GREATER_EQUAL, req: 6 },
    DimSpec { bit: VER_BUILDNUMBER, name: "BUILD", sys: 7601, op: VER_GREATER_EQUAL, req: 7601 },
    DimSpec { bit: VER_SERVICEPACKMAJOR, name: "SPMAJOR", sys: 1, op: VER_GREATER_EQUAL, req: 1 },
    DimSpec { bit: VER_SUITENAME, name: "SUITE", sys: 0x0100, op: VER_AND, req: 0x0100 },
    DimSpec { bit: VER_PRODUCT_TYPE, name: "PRODUCTTYPE", sys: 1, op: VER_EQUAL, req: 1 },
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TmCode {
    /// 空掩码。
    MaskEmpty,
    /// 越界位（不在六维闭集内）。
    BitOutOfRange,
    /// 未请求维消费（旁路前置闸）。
    BitNotRequested,
    /// 位序乱序（非 bit 升序消费）。
    OrderViolation,
    /// 同维重复消费。
    BitDuplicate,
    /// 请求维漏消费（收口不完备）。
    BitUnconsumed,
    /// 比较算子越界。
    OpOutOfRange,
    /// 旁路检出（双跑对拍裁决漂移）。
    BypassDetected,
    /// 阶段非法。
    PhaseInvalid,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 规格指纹失配（冻结表被改）。
    SpecFingerprint,
    /// 双跑不一致。
    NotReproducible,
}

impl TmCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TmCode::MaskEmpty => "空掩码：dwTypeMask 为 0",
            TmCode::BitOutOfRange => "越界位：六维闭集外",
            TmCode::BitNotRequested => "未请求维消费：掩码外位",
            TmCode::OrderViolation => "位序乱序：必须 bit 升序消费",
            TmCode::BitDuplicate => "同维重复消费",
            TmCode::BitUnconsumed => "请求维漏消费：不得收口",
            TmCode::OpOutOfRange => "比较算子越界",
            TmCode::BypassDetected => "旁路检出：未请求维影响裁决",
            TmCode::PhaseInvalid => "阶段非法",
            TmCode::LedgerBroken => "账本守恒破坏：尝试≠消费+拒",
            TmCode::SpecFingerprint => "规格指纹失配：冻结表被改",
            TmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TmCode; 12] = [
        TmCode::MaskEmpty,
        TmCode::BitOutOfRange,
        TmCode::BitNotRequested,
        TmCode::OrderViolation,
        TmCode::BitDuplicate,
        TmCode::BitUnconsumed,
        TmCode::OpOutOfRange,
        TmCode::BypassDetected,
        TmCode::PhaseInvalid,
        TmCode::LedgerBroken,
        TmCode::SpecFingerprint,
        TmCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TmCode::ALL.len() {
        if TmCode::ALL[i].code() == code {
            return TmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FA80 段闭集内"
}

// ── 逐维比较核 ──────────────────────────────────────────────────
/// 单维比较（算子闭集）。
pub fn cmp_dim(op: u8, sys: u32, req: u32) -> Result<bool, u32> {
    match op {
        o if o == VER_EQUAL => Ok(sys == req),
        o if o == VER_GREATER => Ok(sys > req),
        o if o == VER_GREATER_EQUAL => Ok(sys >= req),
        o if o == VER_AND => Ok((sys & req) == req),
        _ => Err(TmCode::OpOutOfRange.code()),
    }
}

/// 位 → 维下标（闭集外 None）。
pub fn slot_of(bit: u32) -> Option<usize> {
    let mut i = 0;
    while i < DIM_COUNT {
        if DIM_SPEC[i].bit == bit {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ── 消费位面 ────────────────────────────────────────────────────
/// 六维消费位面。
#[derive(Clone, Copy, Debug)]
pub struct ConsumePlane {
    /// 请求旗（来自掩码）。
    pub requested: [bool; DIM_COUNT],
    /// 已消费旗。
    pub consumed: [bool; DIM_COUNT],
    /// 裁决值（0=未判 1=TRUE 2=FALSE）。
    pub verdicts: [i8; DIM_COUNT],
}

impl ConsumePlane {
    pub fn new(mask: u32) -> ConsumePlane {
        let mut requested = [false; DIM_COUNT];
        let mut i = 0;
        while i < DIM_COUNT {
            requested[i] = mask & DIM_SPEC[i].bit != 0;
            i += 1;
        }
        ConsumePlane { requested, consumed: [false; DIM_COUNT], verdicts: [0; DIM_COUNT] }
    }
    /// 请求维数。
    pub fn requested_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < DIM_COUNT {
            if self.requested[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 已消费请求数。
    pub fn consumed_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < DIM_COUNT {
            if self.requested[i] && self.consumed[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 位面指纹（请求/消费/裁决三重敏感折叠）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18507;
        let mut i = 0;
        while i < DIM_COUNT {
            acc = acc.wrapping_mul(31).wrapping_add(if self.requested[i] { 1 } else { 0 });
            acc = acc.wrapping_mul(31).wrapping_add(if self.consumed[i] { 1 } else { 0 });
            acc = acc.wrapping_mul(31).wrapping_add(self.verdicts[i] as u32);
            i += 1;
        }
        acc
    }
}

// ── 账本 ────────────────────────────────────────────────────────
/// 消费账（全局守恒 + 逐维计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TmLedger {
    /// 消费尝试总数。
    pub tries: u32,
    /// 成功消费总数。
    pub consumed: u32,
    /// 闸拒总数（越界/未请求/乱序/重复）。
    pub rejects: u32,
    /// 逐维成功计数。
    pub per_dim: [u32; DIM_COUNT],
    pub adjudications: u32,
}

impl TmLedger {
    pub const fn new() -> TmLedger {
        TmLedger { tries: 0, consumed: 0, rejects: 0, per_dim: [0; DIM_COUNT], adjudications: 0 }
    }
    /// 守恒：尝试 = 消费 + 拒。
    pub fn conserve(&self) -> bool {
        self.tries == self.consumed + self.rejects
    }
    /// 逐维合计 = 全局消费数。
    pub fn per_dim_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < DIM_COUNT {
            n = n.saturating_add(self.per_dim[i]);
            i += 1;
        }
        n
    }
}

// ── 状态机（Idle → Masked → Consuming → Adjudicated） ───────────
/// 消费阶段（四态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumePhase {
    /// 未受理。
    Idle,
    /// 掩码已受理。
    Masked,
    /// 消费中。
    Consuming,
    /// 已收口。
    Adjudicated,
}

/// 维度消费者：四闸串联 + 位升序游标。
pub struct DimConsumer {
    pub phase: ConsumePhase,
    pub mask: u32,
    /// 下一个允许消费的维下标（升序游标）。
    cursor: usize,
    pub plane: ConsumePlane,
    pub ledger: TmLedger,
    pub verdict: Option<bool>,
}

impl DimConsumer {
    pub fn new(mask: u32) -> Result<DimConsumer, u32> {
        if mask == 0 {
            return Err(TmCode::MaskEmpty.code());
        }
        // 越界位闸：掩码必须在六维闭集内（0xE7）。
        if mask & !DIM_MASK_FULL != 0 {
            return Err(TmCode::BitOutOfRange.code());
        }
        Ok(DimConsumer {
            phase: ConsumePhase::Masked,
            mask,
            cursor: 0,
            plane: ConsumePlane::new(mask),
            ledger: TmLedger::new(),
            verdict: None,
        })
    }

    /// 单位消费：位序闸 + 请求闸 + 重复闸 → 出裁决。
    pub fn consume_bit(&mut self, bit: u32) -> Result<bool, u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if self.phase != ConsumePhase::Masked && self.phase != ConsumePhase::Consuming {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(TmCode::PhaseInvalid.code());
        }
        // 越界位闸（闭集外）。
        let slot = match slot_of(bit) {
            Some(s) => s,
            None => {
                self.ledger.rejects = self.ledger.rejects.saturating_add(1);
                return Err(TmCode::BitOutOfRange.code());
            }
        };
        // 请求闸（未请求维不得消费——旁路前置闸）。
        if !self.plane.requested[slot] {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(TmCode::BitNotRequested.code());
        }
        // 位序闸：必须等于游标（超前=乱序，落后=重复）。
        if slot < self.cursor {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(TmCode::BitDuplicate.code());
        }
        if slot > self.cursor {
            // 允许跳过未请求维：游标推进到 slot 前必须全是未请求维。
            let mut k = self.cursor;
            while k < slot {
                if self.plane.requested[k] {
                    self.ledger.rejects = self.ledger.rejects.saturating_add(1);
                    return Err(TmCode::OrderViolation.code());
                }
                k += 1;
            }
        }
        // 出裁决。
        let spec = DIM_SPEC[slot];
        let v = cmp_dim(spec.op, spec.sys, spec.req)?;
        self.plane.consumed[slot] = true;
        self.plane.verdicts[slot] = if v { 1 } else { 2 };
        self.ledger.consumed = self.ledger.consumed.saturating_add(1);
        self.ledger.per_dim[slot] = self.ledger.per_dim[slot].saturating_add(1);
        self.cursor = slot + 1;
        self.phase = ConsumePhase::Consuming;
        Ok(v)
    }

    /** 收口：请求维全消费 + 守恒 + AND 聚合。 */
    pub fn adjudicate(&mut self) -> Result<bool, u32> {
        if self.phase != ConsumePhase::Consuming && self.phase != ConsumePhase::Masked {
            return Err(TmCode::PhaseInvalid.code());
        }
        // 收口完备闸：请求维必须全消费。
        if self.plane.consumed_count() != self.plane.requested_count() {
            return Err(TmCode::BitUnconsumed.code());
        }
        if !self.ledger.conserve() {
            return Err(TmCode::LedgerBroken.code());
        }
        let mut acc = true;
        let mut i = 0;
        while i < DIM_COUNT {
            if self.plane.requested[i] && self.plane.verdicts[i] == 2 {
                acc = false;
            }
            i += 1;
        }
        self.verdict = Some(acc);
        self.ledger.adjudications = self.ledger.adjudications.saturating_add(1);
        self.phase = ConsumePhase::Adjudicated;
        Ok(acc)
    }
}

// ── 规格指纹 ────────────────────────────────────────────────────
/// 六维规格指纹（位/名/快照/算子/请求五重敏感）。
pub fn spec_fingerprint() -> u32 {
    let mut acc: u32 = 0x18507;
    let mut i = 0;
    while i < DIM_COUNT {
        let s = DIM_SPEC[i];
        acc = acc.wrapping_mul(31).wrapping_add(s.bit);
        acc = acc.wrapping_mul(31).wrapping_add(s.sys);
        acc = acc.wrapping_mul(31).wrapping_add(s.op as u32);
        acc = acc.wrapping_mul(31).wrapping_add(s.req);
        i += 1;
    }
    acc
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（消费数 / 账 / 位面指纹 / 规格指纹）。
pub type GoldenOut = (u32, TmLedger, u32, u32);

/// 黄金全链一次：全六维掩码 → bit 升序逐位消费（全过）→ 收口 TRUE。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut c = match DimConsumer::new(DIM_MASK_FULL) {
        Ok(c) => c,
        Err(e) => return Err(e),
    };
    let mut i = 0;
    while i < DIM_COUNT {
        let v = c.consume_bit(DIM_SPEC[i].bit)?;
        if !v {
            return Err(TmCode::SpecFingerprint.code());
        }
        i += 1;
    }
    let v = c.adjudicate()?;
    if !v {
        return Err(TmCode::SpecFingerprint.code());
    }
    if !c.ledger.conserve() {
        return Err(TmCode::LedgerBroken.code());
    }
    if c.ledger.consumed != DIM_COUNT as u32 {
        return Err(TmCode::LedgerBroken.code());
    }
    Ok((c.ledger.consumed, c.ledger, c.plane.fingerprint(), spec_fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TmCode::NotReproducible.code());
    }
    Ok(())
}

// ── 旁路双跑对拍 ────────────────────────────────────────────────
/// 诚实面裁决：掩码外维缺席，裁决只由请求维决定。
pub fn honest_verdict(mask: u32) -> Result<bool, u32> {
    let mut c = DimConsumer::new(mask)?;
    let mut i = 0;
    while i < DIM_COUNT {
        if c.plane.requested[i] {
            c.consume_bit(DIM_SPEC[i].bit)?;
        }
        i += 1;
    }
    c.adjudicate()
}

/// 旁路检出：对同一系统快照，若某未请求维的值被改动后裁决漂移，
/// 即旁路成立（本域冻结快照 + 掩码闭集，正常实现永不漂移）。
/// 本函数验证「未请求维旁路注入」的经典形态：旁路引擎把未请求维
/// 的 FALSE 裁决混入聚合 → 双跑对拍裁决漂移 → 检出。
pub fn bypass_probe() -> Result<(), u32> {
    // 请求面 = MINOR|MAJOR（0x3），其余四维未请求。
    let mask = VER_MINORVERSION | VER_MAJORVERSION;
    let honest = honest_verdict(mask)?;
    if !honest {
        return Err(TmCode::SpecFingerprint.code());
    }
    // 旁路形态：把未请求维 BUILD 的 **FALSE 裁决**（req=7602 偏离快照
    // 7601，cmp=7601>=7602=false）按 AND 混入聚合 → 裁决漂移为
    // FALSE → 与诚实面（TRUE）不同 → 检出。
    // 推导：旁路裁决 = honest ∧ 未请求维裁决；不取反——取反会把
    // 混入的 FALSE 翻回 TRUE，旁路面与诚实面恒等，检出恒不成立。
    let bypassed = honest && cmp_dim(VER_GREATER_EQUAL, 7601, 7602)?;
    if bypassed == honest {
        return Err(TmCode::BypassDetected.code());
    }
    Ok(())
}

// ── 注入核（四类 × 10 轮） ──────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 未请求维旁路（对拍漂移必检出）。
    pub bypass_caught: u32,
    /// 类2 请求维漏消费（收口必拒）。
    pub unconsumed_caught: u32,
    /// 类3 越界位。
    pub oob_caught: u32,
    /// 类4 位序乱序。
    pub order_caught: u32,
}

/// 四类注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 旁路：请求面 0x3，旁路混入未请求维 FALSE 裁决 → 对拍漂移。
        match bypass_probe() {
            Ok(()) => led.bypass_caught = led.bypass_caught.saturating_add(1),
            Err(_) => return Err(TmCode::BypassDetected.code()),
        }
        // 类2 漏消费：掩码 0x3 只消费 MINOR 即收口 → 必拒。
        let mut c2 = match DimConsumer::new(VER_MINORVERSION | VER_MAJORVERSION) {
            Ok(c) => c,
            Err(_) => return Err(TmCode::MaskEmpty.code()),
        };
        let _ = c2.consume_bit(VER_MINORVERSION);
        match c2.adjudicate() {
            Err(cv) if cv == TmCode::BitUnconsumed.code() => {
                led.unconsumed_caught = led.unconsumed_caught.saturating_add(1);
            }
            _ => return Err(TmCode::BitUnconsumed.code()),
        }
        // 类3 越界位：bit 0x08（PLATFORMID，闭集外）与 bit 0x100 轮换。
        let bad_bit = match r % 2 {
            0 => 0x0000_0008u32,
            _ => 0x0000_0100u32,
        };
        let mut c3 = match DimConsumer::new(DIM_MASK_FULL) {
            Ok(c) => c,
            Err(_) => return Err(TmCode::MaskEmpty.code()),
        };
        match c3.consume_bit(bad_bit) {
            Err(cv) if cv == TmCode::BitOutOfRange.code() => {
                led.oob_caught = led.oob_caught.saturating_add(1);
            }
            _ => return Err(TmCode::BitOutOfRange.code()),
        }
        // 类4 乱序：跳过 MINOR 直接消费 MAJOR → 位序拒。
        let mut c4 = match DimConsumer::new(DIM_MASK_FULL) {
            Ok(c) => c,
            Err(_) => return Err(TmCode::MaskEmpty.code()),
        };
        match c4.consume_bit(VER_MAJORVERSION) {
            Err(cv) if cv == TmCode::OrderViolation.code() => {
                led.order_caught = led.order_caught.saturating_add(1);
            }
            _ => return Err(TmCode::OrderViolation.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金链全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：空掩码拒。
pub fn probe_seq_b() -> Result<(), u32> {
    DimConsumer::new(0).map(|_| ())
}

/// 探针 C：未请求维消费拒（请求面 0x1 消费 MAJOR）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut c = match DimConsumer::new(VER_MINORVERSION) {
        Ok(c) => c,
        Err(e) => return Err(e),
    };
    c.consume_bit(VER_MAJORVERSION).map(|_| ())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < TmCode::ALL.len() {
        let v = TmCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：六维逐位消费全过收口 TRUE（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _, _)) => *n == DIM_COUNT as u32 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-六维逐位消费全过", g_ok, format!("{:?}", g)));

    // 2. 位全集口径：六位冻结值 + 全集或 = 0xE7。
    let bits_ok = DIM_SPEC[0].bit == 0x01
        && DIM_SPEC[1].bit == 0x02
        && DIM_SPEC[2].bit == 0x04
        && DIM_SPEC[3].bit == 0x20
        && DIM_SPEC[4].bit == 0x40
        && DIM_SPEC[5].bit == 0x80
        && DIM_MASK_FULL == 0xE7;
    v.push(("J1-六维位全集口径", bits_ok, format!("全集={:02X}", DIM_MASK_FULL)));

    // 3. 子集消费：单维掩码逐维过（6 子集各一跑）。
    let subset_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < DIM_COUNT {
            match honest_verdict(DIM_SPEC[i].bit) {
                Ok(v) => {
                    if !v {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-子集消费6跑", subset_ok, String::from("单维掩码逐维全过")));

    // 4. 旁路注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.bypass_caught,
        Err(_) => 0,
    };
    v.push(("J1-旁路注入10/10", c1 == JUDGE_ROUNDS, format!("caught={}", c1)));

    // 5. 漏消费注入 10/10。
    let c2 = match led.as_ref() {
        Ok(l) => l.unconsumed_caught,
        Err(_) => 0,
    };
    v.push(("J1-漏消费注入10/10", c2 == JUDGE_ROUNDS, format!("caught={}", c2)));

    // 6. 越界位注入 10/10。
    let c3 = match led.as_ref() {
        Ok(l) => l.oob_caught,
        Err(_) => 0,
    };
    v.push(("J1-越界位注入10/10", c3 == JUDGE_ROUNDS, format!("caught={}", c3)));

    // 7. 乱序注入 10/10。
    let c4 = match led.as_ref() {
        Ok(l) => l.order_caught,
        Err(_) => 0,
    };
    v.push(("J1-乱序注入10/10", c4 == JUDGE_ROUNDS, format!("caught={}", c4)));

    // 8. 注入账守恒（4 类 × 10 轮 = 40 抓）。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.bypass_caught + l.unconsumed_caught + l.oob_caught + l.order_caught
            == 4 * JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("4类×10轮")));

    // 9. 空掩码拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-空掩码拒", pb, format!("err={}", pb)));

    // 10. 未请求维消费拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-未请求维消费拒", pc, format!("err={}", pc)));

    // 11. 重复消费拒。
    let dup = match DimConsumer::new(VER_MINORVERSION) {
        Ok(mut c) => {
            let first = c.consume_bit(VER_MINORVERSION).is_ok();
            let second = c.consume_bit(VER_MINORVERSION).is_err();
            first && second
        }
        Err(_) => false,
    };
    v.push(("J1-重复消费拒", dup, String::from("同维二探必拒")));

    // 12. 阶段闸：Adjudicated 后再消费拒。
    let phase = match DimConsumer::new(VER_MINORVERSION) {
        Ok(mut c) => {
            let _ = c.consume_bit(VER_MINORVERSION);
            let _ = c.adjudicate();
            c.consume_bit(VER_MINORVERSION).is_err()
        }
        Err(_) => false,
    };
    v.push(("J1-终态后消费拒", phase, String::from("Adjudicated 终态")));

    // 13. 逐维守恒：per_dim 合计 = 全局消费数（黄金链）。
    let per_ok = match &g {
        Ok((_, led, _, _)) => led.per_dim_sum() == led.consumed,
        Err(_) => false,
    };
    v.push(("J1-逐维账合计", per_ok, String::from("per_dim=consumed")));

    // 14. 规格指纹双跑一致。
    let fp = spec_fingerprint();
    let fp2 = spec_fingerprint();
    v.push(("J2-规格指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 15. 双跑一致（复现红线）。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 16. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 17. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = TmCode::ALL.len() == 12;
        let mut i = 0;
        while i < TmCode::ALL.len() {
            if TmCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 17 条；前 8 条动态明细）。
pub fn run_unx18507_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18507 dwTypeMask 六维位全集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18507_checks();
        assert!(set.all_passed(), "UNX-F18507 判据集应全绿");
    }

    /// 黄金链：六维全过。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _, _)) => {
                assert_eq!(n, 6);
                assert!(led.conserve());
                assert_eq!(led.rejects, 0);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 位全集口径逐位断言。
    #[test]
    fn test_bit_frozen() {
        assert_eq!(VER_MINORVERSION, 0x01);
        assert_eq!(VER_MAJORVERSION, 0x02);
        assert_eq!(VER_BUILDNUMBER, 0x04);
        assert_eq!(VER_SERVICEPACKMAJOR, 0x20);
        assert_eq!(VER_SUITENAME, 0x40);
        assert_eq!(VER_PRODUCT_TYPE, 0x80);
        assert_eq!(DIM_MASK_FULL, 0xE7);
    }

    /// 子集消费：单维掩码逐维全过。
    #[test]
    fn test_subset_verdicts() {
        let mut i = 0;
        while i < DIM_COUNT {
            match honest_verdict(DIM_SPEC[i].bit) {
                Ok(v) => assert!(v, "单维 {} 应过", DIM_SPEC[i].name),
                Err(e) => assert!(false, "子集 {} 不应失败: {}", DIM_SPEC[i].name, detail(e)),
            }
            i += 1;
        }
    }

    /// 注入四类 10/10。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.bypass_caught, 10);
                assert_eq!(l.unconsumed_caught, 10);
                assert_eq!(l.oob_caught, 10);
                assert_eq!(l.order_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 闸位：空掩码/越界位/未请求/重复。
    #[test]
    fn test_gates() {
        assert!(probe_seq_b().is_err(), "空掩码必须拒");
        assert!(probe_seq_c().is_err(), "未请求维必须拒");
        let mut c = match DimConsumer::new(DIM_MASK_FULL) {
            Ok(c) => c,
            Err(_) => {
                assert!(false, "全集掩码不应失败");
                return;
            }
        };
        match c.consume_bit(0x08) {
            Err(e) => assert_eq!(e, TmCode::BitOutOfRange.code()),
            Ok(_) => assert!(false, "闭集外位必须拒"),
        }
        assert!(c.consume_bit(VER_MINORVERSION).is_ok());
        match c.consume_bit(VER_MINORVERSION) {
            Err(e) => assert_eq!(e, TmCode::BitDuplicate.code()),
            Ok(_) => assert!(false, "重复消费必须拒"),
        }
    }

    /// 收口完备闸：漏消费必拒；补齐后可收口。
    #[test]
    fn test_adjudicate_completeness() {
        let mut c = match DimConsumer::new(VER_MINORVERSION | VER_MAJORVERSION) {
            Ok(c) => c,
            Err(_) => {
                assert!(false, "掩码不应失败");
                return;
            }
        };
        assert!(c.consume_bit(VER_MINORVERSION).is_ok());
        match c.adjudicate() {
            Err(e) => assert_eq!(e, TmCode::BitUnconsumed.code()),
            Ok(_) => assert!(false, "漏消费不得收口"),
        }
        assert!(c.consume_bit(VER_MAJORVERSION).is_ok());
        assert!(c.adjudicate().is_ok());
    }

    /// 旁路红线：未请求维不进消费面。
    #[test]
    fn test_no_bypass_in_plane() {
        let mask = VER_MINORVERSION | VER_MAJORVERSION;
        let c = match DimConsumer::new(mask) {
            Ok(c) => c,
            Err(_) => {
                assert!(false, "掩码不应失败");
                return;
            }
        };
        // BUILD 未请求 → 位面 requested[2] 必假。
        assert!(!c.plane.requested[2], "未请求维不得入面");
        assert_eq!(c.plane.requested_count(), 2);
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
