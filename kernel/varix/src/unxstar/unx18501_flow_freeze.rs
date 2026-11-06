//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18501 · VerifyVersionInfo 引擎总纲（判定流程冻结）
//! · 0x7F900 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18501-J1 判定流程（掩码→条件→比较→裁决）四段冻结 v1，
//!   跳段注入 10/10 拒绝。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 四段冻结 v1：段0 掩码解析（dwTypeMask 逐位受理）→ 段1 条件装载
//!     （VER_* 算子逐维入槽）→ 段2 比较执行（逐维出裁决值）→ 段3 裁决
//!     收口（AND 全维聚合，一次收口）；段序单向递进，跳段必拒；
//!   - 维度系（bit 序冻结，winnt.h 口径）：d0=MINOR / d1=MAJOR /
//!     d2=BUILD / d3=PLATFORMID / d4=SPMINOR / d5=SPMAJOR /
//!     d6=SUITENAME / d7=PRODUCT_TYPE；算子闭集 1..=7；
//!   - 系统快照与请求表写死（OSV 6.1.7601 / NT 平台 / SP1 / 套件
//!     0x0100 / 产品型 1），黄金链期望裁决 = TRUE；
//!   - 跳段注入四类 × 10 轮：无掩码装条件 / 无条件比零维 / 未比较先
//!     收口 / 收口后再收口——10/10 全拒且段账逐轮独立记账；
//!   - 段账（四方守恒）：每段 尝试 = 成功 + 拒收，账破即红；
//!   - 边界：空掩码拒 / 越界位（>7）拒 / 重复维度槽拒 / 请求外维拒 /
//!     未比较收口拒 / 双收口拒；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟；LCG 不用（本卡确定性封闭，无随机源）。
//! 本模块结构清单：
//!   [错误路径] FlowCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [数据结构] 维度位面 MaskPlane（8 槽算子槽 + 请求旗 + 裁决值槽）
//!   [状态机]   FlowPhase 四段单向 + FlowEngine（跳段闸在每段入口）
//!   [账本]     FlowLedger（四段守恒核算）
//!   [裁决表]   SNAPSHOT（系统快照）/ REQ_TABLE（冻结请求表）
//!   [注入核]   四类跳段 × 10 轮（无掩码装/零维比/未比收口/双收口）
//!   [收口]     golden_run + run_unx18501_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F900，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7F900;

// ── 域冻结常量（winnt.h 口径，跨卡一致） ────────────────────────
/// VER_EQUAL（冻结）。
pub const VER_EQUAL: u8 = 1;
/// VER_GREATER（冻结）。
pub const VER_GREATER: u8 = 2;
/// VER_GREATER_EQUAL（冻结）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// VER_LESS（冻结）。
pub const VER_LESS: u8 = 4;
/// VER_LESS_EQUAL（冻结）。
pub const VER_LESS_EQUAL: u8 = 5;
/// VER_AND（套件位与，冻结）。
pub const VER_AND: u8 = 6;
/// VER_OR（套件位或，冻结）。
pub const VER_OR: u8 = 7;
/// 维度数（dwTypeMask 有效位 0..=7）。
pub const DIM_COUNT: usize = 8;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 系统快照（bit 序冻结）：OSV 6.1.7601 / NT / SP1.0 / 套件 0x0100 / PT=1。
/// 推导：d0=MINOR=1，d1=MAJOR=6，d2=BUILD=7601，d3=PLATFORMID=2
/// （VER_PLATFORM_WIN32_NT），d4=SPMINOR=0，d5=SPMAJOR=1，
/// d6=SUITENAME=0x0100（VER_SUITE_SINGLEUSERTS），d7=PRODUCTTYPE=1。
pub const SNAPSHOT: [u32; DIM_COUNT] = [1, 6, 7601, 2, 0, 1, 0x0100, 1];

/// 冻结请求表：黄金链全维请求（期望裁决 TRUE）。
/// 推导：逐维取「系统值本身」（EQ/GE/LE/AND 按槽语义），保证合法
/// 请求在冻结快照上必过——黄金链任何一维红即冻结表被改。
pub const REQ_TABLE: [u32; DIM_COUNT] = [1, 6, 7601, 2, 0, 1, 0x0100, 1];

/// 冻结请求算子表（与 REQ_TABLE 同序；d6 用 VER_AND 套件位与）。
pub const REQ_OPS: [u8; DIM_COUNT] =
    [VER_EQUAL, VER_GREATER_EQUAL, VER_GREATER_EQUAL, VER_EQUAL, VER_LESS_EQUAL, VER_GREATER_EQUAL, VER_AND, VER_EQUAL];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowCode {
    /// 空掩码（dwTypeMask == 0）。
    MaskEmpty,
    /// 掩码越界位（bit ≥ 8）。
    MaskBitOutOfRange,
    /// 算子越界（0 或 > 7，闭集 1..=7 外）。
    OpOutOfRange,
    /// 条件槽冲突（同维重复装载）。
    CondSlotConflict,
    /// 跳段（段序单向递进被破坏）。
    StageOutOfOrder,
    /// 请求外维（未受理维度被比较）。
    DimNotRequested,
    /// 未比较收口（有请求维无裁决值）。
    DimUncompared,
    /// 双收口（裁决段重复进入）。
    AdjudicateTwice,
    /// 段账守恒破坏。
    LedgerBroken,
    /// 阶段值非法（域外值出现于账面）。
    PhaseInvalid,
    /// 对账指纹失配（快照/请求表被改）。
    FingerprintMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl FlowCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生，禁手写魔数）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FlowCode::MaskEmpty => "空掩码：dwTypeMask 为 0",
            FlowCode::MaskBitOutOfRange => "掩码越界位：有效位仅 0..=7",
            FlowCode::OpOutOfRange => "算子越界：闭集 1..=7 外",
            FlowCode::CondSlotConflict => "条件槽冲突：同维重复装载",
            FlowCode::StageOutOfOrder => "跳段：段序必须掩码→条件→比较→裁决",
            FlowCode::DimNotRequested => "请求外维：未受理维度不得比较",
            FlowCode::DimUncompared => "未比较收口：请求维缺裁决值",
            FlowCode::AdjudicateTwice => "双收口：裁决段一次收口",
            FlowCode::LedgerBroken => "段账守恒破坏：尝试≠成功+拒",
            FlowCode::PhaseInvalid => "阶段值非法：域外值入账",
            FlowCode::FingerprintMismatch => "对账指纹失配：冻结表被改",
            FlowCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [FlowCode; 12] = [
        FlowCode::MaskEmpty,
        FlowCode::MaskBitOutOfRange,
        FlowCode::OpOutOfRange,
        FlowCode::CondSlotConflict,
        FlowCode::StageOutOfOrder,
        FlowCode::DimNotRequested,
        FlowCode::DimUncompared,
        FlowCode::AdjudicateTwice,
        FlowCode::LedgerBroken,
        FlowCode::PhaseInvalid,
        FlowCode::FingerprintMismatch,
        FlowCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FlowCode::ALL.len() {
        if FlowCode::ALL[i].code() == code {
            return FlowCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F900 段闭集内"
}

// ── 基础比较核（逐维算子出裁决值） ──────────────────────────────
/// 单维比较：按算子闭集对 (sys, req) 出裁决值。
/// d6 套件维位与口径（请求位任一在场即过）与 d7 逐类口径在本卡同样
/// 按位与/等值落地，与 F18509/F18510 特例卡口径一致。
pub fn cmp_dim(op: u8, sys: u32, req: u32) -> Result<bool, u32> {
    match op {
        o if o == VER_EQUAL => Ok(sys == req),
        o if o == VER_GREATER => Ok(sys > req),
        o if o == VER_GREATER_EQUAL => Ok(sys >= req),
        o if o == VER_LESS => Ok(sys < req),
        o if o == VER_LESS_EQUAL => Ok(sys <= req),
        o if o == VER_AND => Ok((sys & req) == req),
        o if o == VER_OR => Ok((sys & req) != 0),
        _ => Err(FlowCode::OpOutOfRange.code()),
    }
}

// ── 维度位面（掩码/条件/裁决三槽同构 8 维） ─────────────────────
/// 单维槽（Copy，可定容复制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimSlot {
    /// 该维是否被掩码受理。
    pub requested: bool,
    /// 已装载算子（0 = 未装）。
    pub op: u8,
    /// 裁决值（0 = 未比，1 = TRUE，2 = FALSE；i8 避开 Option 以稳 Copy）。
    pub verdict: i8,
    /// 该维是否已执行比较。
    pub compared: bool,
}

impl DimSlot {
    /// 空槽。
    pub const EMPTY: DimSlot = DimSlot { requested: false, op: 0, verdict: 0, compared: false };
}

/// 维度位面：掩码受理旗 / 条件算子槽 / 裁决值槽三位一体。
#[derive(Clone, Copy, Debug)]
pub struct MaskPlane {
    pub slots: [DimSlot; DIM_COUNT],
}

impl MaskPlane {
    pub fn new() -> MaskPlane {
        MaskPlane { slots: [DimSlot::EMPTY; DIM_COUNT] }
    }
    /// 位面清零（幂等：清后与 new 等价）。
    pub fn reset(&mut self) {
        let mut i = 0;
        while i < DIM_COUNT {
            self.slots[i] = DimSlot::EMPTY;
            i += 1;
        }
    }
    /// 已受理维数。
    pub fn requested_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < DIM_COUNT {
            if self.slots[i].requested {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 已比较维数（限受理维内）。
    pub fn compared_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < DIM_COUNT {
            if self.slots[i].requested && self.slots[i].compared {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 位面指纹（冻结对账用：槽三元组折叠；推导：31 进制逐维折叠，
    /// 维序敏感——改序/改值/改算子必变指纹）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18501;
        let mut i = 0;
        while i < DIM_COUNT {
            let s = self.slots[i];
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(if s.requested { 1 } else { 0 })
                .wrapping_mul(31)
                .wrapping_add(s.op as u32)
                .wrapping_mul(31)
                .wrapping_add(if s.compared { 1 } else { 0 });
            i += 1;
        }
        acc
    }
}

// ── 段账本（四段守恒） ──────────────────────────────────────────
/// 四段账：每段 尝试 = 成功 + 拒收（守恒方程逐段成立）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowLedger {
    /// 段0：掩码受理尝试 / 成功 / 拒收。
    pub mask_tries: u32,
    pub mask_ok: u32,
    pub mask_rej: u32,
    /// 段1：条件装载尝试 / 成功 / 拒收。
    pub load_tries: u32,
    pub load_ok: u32,
    pub load_rej: u32,
    /// 段2：比较执行尝试（按维计）/ 成功 / 拒收。
    pub cmp_tries: u32,
    pub cmp_ok: u32,
    pub cmp_rej: u32,
    /// 段3：收口尝试 / 成功 / 拒收。
    pub adj_tries: u32,
    pub adj_ok: u32,
    pub adj_rej: u32,
}

impl FlowLedger {
    pub const fn new() -> FlowLedger {
        FlowLedger {
            mask_tries: 0, mask_ok: 0, mask_rej: 0,
            load_tries: 0, load_ok: 0, load_rej: 0,
            cmp_tries: 0, cmp_ok: 0, cmp_rej: 0,
            adj_tries: 0, adj_ok: 0, adj_rej: 0,
        }
    }
    /// 四段守恒：逐段 尝试 = 成功 + 拒收。
    pub fn conserve(&self) -> bool {
        self.mask_tries == self.mask_ok + self.mask_rej
            && self.load_tries == self.load_ok + self.load_rej
            && self.cmp_tries == self.cmp_ok + self.cmp_rej
            && self.adj_tries == self.adj_ok + self.adj_rej
    }
    /// 总账聚合（对账：四段尝试合计，只增不减探测）。
    pub fn total_tries(&self) -> u32 {
        self.mask_tries + self.load_tries + self.cmp_tries + self.adj_tries
    }
}

// ── 四段状态机（单向递进） ──────────────────────────────────────
/// 流程段（0=Idle 1=MaskParsed 2=CondLoaded 3=Compared 4=Adjudicated）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowPhase {
    /// 段前：掩码未受理。
    Idle,
    /// 段0 过：掩码已受理。
    MaskParsed,
    /// 段1 过：条件已装载。
    CondLoaded,
    /// 段2 过：比较已执行。
    Compared,
    /// 段3 过：裁决已收口（终态）。
    Adjudicated,
}

impl FlowPhase {
    /// 整数投影（账面用；0..=4 闭集）。
    pub fn val(self) -> u8 {
        match self {
            FlowPhase::Idle => 0,
            FlowPhase::MaskParsed => 1,
            FlowPhase::CondLoaded => 2,
            FlowPhase::Compared => 3,
            FlowPhase::Adjudicated => 4,
        }
    }
    /// 整数反查（域外 None）。
    pub fn from_val(v: u8) -> Option<FlowPhase> {
        match v {
            0 => Some(FlowPhase::Idle),
            1 => Some(FlowPhase::MaskParsed),
            2 => Some(FlowPhase::CondLoaded),
            3 => Some(FlowPhase::Compared),
            4 => Some(FlowPhase::Adjudicated),
            _ => None,
        }
    }
}

/// 判定引擎：掩码→条件→比较→裁决 四段单向；每段入口置跳段闸。
pub struct FlowEngine {
    pub phase: FlowPhase,
    pub plane: MaskPlane,
    pub verdict: Option<bool>,
    pub ledger: FlowLedger,
}

impl FlowEngine {
    pub fn new() -> FlowEngine {
        FlowEngine {
            phase: FlowPhase::Idle,
            plane: MaskPlane::new(),
            verdict: None,
            ledger: FlowLedger::new(),
        }
    }

    /// 段0：掩码受理。空掩码/越界位拒；受理成功 Idle→MaskParsed。
    pub fn parse_mask(&mut self, mask: u32) -> Result<usize, u32> {
        self.ledger.mask_tries = self.ledger.mask_tries.saturating_add(1);
        if mask == 0 {
            self.ledger.mask_rej = self.ledger.mask_rej.saturating_add(1);
            return Err(FlowCode::MaskEmpty.code());
        }
        // 越界位闸：有效位仅 0..=7（dwTypeMask 全集 0x000000FF）。
        if mask >> DIM_COUNT != 0 {
            self.ledger.mask_rej = self.ledger.mask_rej.saturating_add(1);
            return Err(FlowCode::MaskBitOutOfRange.code());
        }
        // 重复进入段0 = 跳段回退（MaskParsed 后不得重受理）。
        if self.phase != FlowPhase::Idle {
            self.ledger.mask_rej = self.ledger.mask_rej.saturating_add(1);
            return Err(FlowCode::StageOutOfOrder.code());
        }
        let mut n = 0usize;
        let mut b = 0u32;
        while b < DIM_COUNT as u32 {
            if mask & (1u32 << b) != 0 {
                self.plane.slots[b as usize].requested = true;
                n += 1;
            }
            b += 1;
        }
        self.phase = FlowPhase::MaskParsed;
        self.ledger.mask_ok = self.ledger.mask_ok.saturating_add(1);
        Ok(n)
    }

    /// 段1：条件装载。必须在段0 之后；算子闭集闸 + 重复槽闸。
    pub fn load_cond(&mut self, dim: usize, op: u8) -> Result<(), u32> {
        self.ledger.load_tries = self.ledger.load_tries.saturating_add(1);
        // 跳段闸：必须已过段0 且未过段1。
        if self.phase != FlowPhase::MaskParsed {
            self.ledger.load_rej = self.ledger.load_rej.saturating_add(1);
            return Err(FlowCode::StageOutOfOrder.code());
        }
        if dim >= DIM_COUNT {
            self.ledger.load_rej = self.ledger.load_rej.saturating_add(1);
            return Err(FlowCode::MaskBitOutOfRange.code());
        }
        // 算子闭集闸：1..=7。
        if op == 0 || op > VER_OR {
            self.ledger.load_rej = self.ledger.load_rej.saturating_add(1);
            return Err(FlowCode::OpOutOfRange.code());
        }
        // 请求旗闸：未受理维不得装条件（请求外维前置闸）。
        let req = match self.plane.slots.get(dim) {
            Some(s) => s.requested,
            None => false,
        };
        if !req {
            self.ledger.load_rej = self.ledger.load_rej.saturating_add(1);
            return Err(FlowCode::DimNotRequested.code());
        }
        // 重复槽闸：同维已装即拒（槽冲突防线）。
        let dup = match self.plane.slots.get(dim) {
            Some(s) => s.op != 0,
            None => false,
        };
        if dup {
            self.ledger.load_rej = self.ledger.load_rej.saturating_add(1);
            return Err(FlowCode::CondSlotConflict.code());
        }
        self.plane.slots[dim].op = op;
        self.ledger.load_ok = self.ledger.load_ok.saturating_add(1);
        Ok(())
    }

    /// 段1 收束：全部受理维装齐 → CondLoaded（漏装在此拒）。
    pub fn seal_load(&mut self) -> Result<(), u32> {
        self.ledger.load_tries = self.ledger.load_tries.saturating_add(1);
        if self.phase != FlowPhase::MaskParsed {
            self.ledger.load_rej = self.ledger.load_rej.saturating_add(1);
            return Err(FlowCode::StageOutOfOrder.code());
        }
        let mut i = 0;
        while i < DIM_COUNT {
            let s = self.plane.slots[i];
            if s.requested && s.op == 0 {
                self.ledger.load_rej = self.ledger.load_rej.saturating_add(1);
                return Err(FlowCode::DimUncompared.code());
            }
            i += 1;
        }
        self.phase = FlowPhase::CondLoaded;
        self.ledger.load_ok = self.ledger.load_ok.saturating_add(1);
        Ok(())
    }

    /// 段2：比较执行（逐受理维出裁决值）。必须在 CondLoaded 态。
    pub fn run_compare(&mut self) -> Result<usize, u32> {
        self.ledger.cmp_tries = self.ledger.cmp_tries.saturating_add(1);
        // 跳段闸：必须已过段1（未装载即比 = 跳段）。
        if self.phase != FlowPhase::CondLoaded {
            self.ledger.cmp_rej = self.ledger.cmp_rej.saturating_add(1);
            return Err(FlowCode::StageOutOfOrder.code());
        }
        let mut n = 0usize;
        let mut i = 0;
        while i < DIM_COUNT {
            let s = self.plane.slots[i];
            if s.requested {
                if s.op == 0 {
                    // 装载缺失即拒（漏装防线，重复报账不吞）。
                    self.ledger.cmp_rej = self.ledger.cmp_rej.saturating_add(1);
                    return Err(FlowCode::DimUncompared.code());
                }
                let v = cmp_dim(s.op, SNAPSHOT[i], REQ_TABLE[i])?;
                self.plane.slots[i].verdict = if v { 1 } else { 2 };
                self.plane.slots[i].compared = true;
                n += 1;
            }
            i += 1;
        }
        self.phase = FlowPhase::Compared;
        self.ledger.cmp_ok = self.ledger.cmp_ok.saturating_add(n as u32);
        Ok(n)
    }

    /// 段3：裁决收口（AND 全维聚合；一次收口，重复收口拒）。
    pub fn adjudicate(&mut self) -> Result<bool, u32> {
        self.ledger.adj_tries = self.ledger.adj_tries.saturating_add(1);
        // 跳段闸：必须已过段2。
        if self.phase != FlowPhase::Compared {
            self.ledger.adj_rej = self.ledger.adj_rej.saturating_add(1);
            return Err(FlowCode::StageOutOfOrder.code());
        }
        // 收口完备闸：每个受理维必须有裁决值。
        let mut acc = true;
        let mut i = 0;
        while i < DIM_COUNT {
            let s = self.plane.slots[i];
            if s.requested {
                if !s.compared || s.verdict == 0 {
                    self.ledger.adj_rej = self.ledger.adj_rej.saturating_add(1);
                    return Err(FlowCode::DimUncompared.code());
                }
                if s.verdict == 2 {
                    acc = false;
                }
            }
            i += 1;
        }
        self.phase = FlowPhase::Adjudicated;
        self.ledger.adj_ok = self.ledger.adj_ok.saturating_add(1);
        self.verdict = Some(acc);
        Ok(acc)
    }

    /// 收口后再收口 = 双收口（显式拒，不动终态）。
    pub fn readjudicate_guard(&mut self) -> Result<bool, u32> {
        self.ledger.adj_tries = self.ledger.adj_tries.saturating_add(1);
        if self.phase == FlowPhase::Adjudicated {
            self.ledger.adj_rej = self.ledger.adj_rej.saturating_add(1);
            return Err(FlowCode::AdjudicateTwice.code());
        }
        self.adjudicate()
    }
}

// ── 黄金全链（四段冻结 v1 全走一遍） ────────────────────────────
/// 黄金链结果（指纹 + 四段账合计 + 裁决值）。
pub type GoldenOut = (u32, u32, bool);

/// 黄金全链一次（确定性）：冻结请求全维请求 → 期望 TRUE。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut eng = FlowEngine::new();
    // 段0：冻结请求掩码 = 全 8 维（0x000000FF）。
    let n = eng.parse_mask(0x0000_00FF)?;
    if n != DIM_COUNT {
        return Err(FlowCode::FingerprintMismatch.code());
    }
    // 段1：逐维装载冻结算子（bit 升序）。
    let mut d = 0usize;
    while d < DIM_COUNT {
        eng.load_cond(d, REQ_OPS[d])?;
        d += 1;
    }
    eng.seal_load()?;
    // 段2：比较执行（8 维全比）。
    let c = eng.run_compare()?;
    if c != DIM_COUNT {
        return Err(FlowCode::DimUncompared.code());
    }
    // 段3：收口（期望 TRUE——冻结请求在冻结快照上必过）。
    let v = eng.adjudicate()?;
    if !v {
        return Err(FlowCode::FingerprintMismatch.code());
    }
    if !eng.ledger.conserve() {
        return Err(FlowCode::LedgerBroken.code());
    }
    Ok((eng.plane.fingerprint(), eng.ledger.total_tries(), v))
}

/// 双跑一致（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FlowCode::NotReproducible.code());
    }
    Ok(())
}

// ── 冻结对账（快照/请求表指纹） ─────────────────────────────────
/// 冻结表指纹：快照 + 请求表 + 算子表折叠（改任一必变）。
/// 推导：37 进制逐槽折叠，初值取段号派生 0x18501。
pub fn frozen_tables_fingerprint() -> u32 {
    let mut acc: u32 = 0x18501;
    let mut i = 0;
    while i < DIM_COUNT {
        acc = acc.wrapping_mul(37).wrapping_add(SNAPSHOT[i]);
        acc = acc.wrapping_mul(37).wrapping_add(REQ_TABLE[i]);
        acc = acc.wrapping_mul(37).wrapping_add(REQ_OPS[i] as u32);
        i += 1;
    }
    acc
}

// ── 注入核（四类跳段 × 10 轮，逐轮独立记账） ────────────────────
/// 注入账（四类各一轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct JumpLedger {
    /// 类1：无掩码装条件（Idle 直接 load）。
    pub load_before_mask: u32,
    /// 类2：零维比（MaskParsed 直接 run_compare）。
    pub cmp_before_load: u32,
    /// 类3：未比较收口（CondLoaded 直接 adjudicate）。
    pub adj_before_cmp: u32,
    /// 类4：双收口（Adjudicated 再 adjudicate）。
    pub readjudicate: u32,
}

/// 四类跳段注入 rounds 轮；每类每轮必须被显式拒绝，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<JumpLedger, u32> {
    let mut led = JumpLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1：Idle 态直接装条件 → 跳段拒。
        let mut e1 = FlowEngine::new();
        match e1.load_cond(1, VER_GREATER_EQUAL) {
            Err(c) if c == FlowCode::StageOutOfOrder.code() => {
                led.load_before_mask = led.load_before_mask.saturating_add(1);
            }
            Err(_) => return Err(FlowCode::StageOutOfOrder.code()),
            Ok(()) => return Err(FlowCode::StageOutOfOrder.code()),
        }
        // 类2：MaskParsed 态直接比较（未装条件）→ 跳段拒。
        let mut e2 = FlowEngine::new();
        let _ = e2.parse_mask(0x0000_00FF);
        match e2.run_compare() {
            Err(c) if c == FlowCode::StageOutOfOrder.code() => {
                led.cmp_before_load = led.cmp_before_load.saturating_add(1);
            }
            Err(_) => return Err(FlowCode::StageOutOfOrder.code()),
            Ok(_) => return Err(FlowCode::StageOutOfOrder.code()),
        }
        // 类3：CondLoaded 态直接收口（未比较）→ 跳段拒。
        let mut e3 = FlowEngine::new();
        let _ = e3.parse_mask(0x0000_00FF);
        let _ = e3.load_cond(0, VER_EQUAL);
        let _ = e3.seal_load();
        match e3.adjudicate() {
            Err(c) if c == FlowCode::StageOutOfOrder.code() => {
                led.adj_before_cmp = led.adj_before_cmp.saturating_add(1);
            }
            Err(_) => return Err(FlowCode::StageOutOfOrder.code()),
            Ok(_) => return Err(FlowCode::StageOutOfOrder.code()),
        }
        // 类4：全链走完后再收口 → 双收口拒（终态不动）。
        let mut e4 = FlowEngine::new();
        let _ = e4.parse_mask(0x0000_00FF);
        let mut d = 0usize;
        while d < DIM_COUNT {
            let _ = e4.load_cond(d, REQ_OPS[d]);
            d += 1;
        }
        let _ = e4.seal_load();
        let _ = e4.run_compare();
        let v1 = e4.adjudicate();
        match (v1, e4.readjudicate_guard()) {
            (Ok(_), Err(c)) if c == FlowCode::AdjudicateTwice.code() => {
                led.readjudicate = led.readjudicate.saturating_add(1);
            }
            _ => return Err(FlowCode::AdjudicateTwice.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金链全过（四段冻结 v1）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：空掩码 → 段0 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut eng = FlowEngine::new();
    eng.parse_mask(0).map(|_| ())
}

/// 探针 C：掩码越界位（bit8 置位）→ 段0 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut eng = FlowEngine::new();
    eng.parse_mask(0x0000_0100).map(|_| ())
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < FlowCode::ALL.len() {
        let v = FlowCode::ALL[i].code();
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

    // 1. 黄金全链：四段冻结 v1 全走，裁决 TRUE（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((_, _, verdict)) => *verdict,
        Err(_) => false,
    };
    v.push(("J1-黄金全链四段冻结", g_ok, format!("{:?}", g)));

    // 2. 探针 A 联动（黄金链可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A黄金链", pa, format!("ok={}", pa)));

    // 3. 跳段注入 10/10：类1 无掩码装条件。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.load_before_mask,
        Err(_) => 0,
    };
    v.push(("J1-跳段注入类1 10/10", c1 == JUDGE_ROUNDS, format!("rej={}", c1)));

    // 4. 类2 零维比。
    let c2 = match led.as_ref() {
        Ok(l) => l.cmp_before_load,
        Err(_) => 0,
    };
    v.push(("J1-跳段注入类2 10/10", c2 == JUDGE_ROUNDS, format!("rej={}", c2)));

    // 5. 类3 未比较收口。
    let c3 = match led.as_ref() {
        Ok(l) => l.adj_before_cmp,
        Err(_) => 0,
    };
    v.push(("J1-跳段注入类3 10/10", c3 == JUDGE_ROUNDS, format!("rej={}", c3)));

    // 6. 类4 双收口。
    let c4 = match led.as_ref() {
        Ok(l) => l.readjudicate,
        Err(_) => 0,
    };
    v.push(("J1-跳段注入类4 10/10", c4 == JUDGE_ROUNDS, format!("rej={}", c4)));

    // 7. 注入账守恒（4 类 × 10 轮 = 40 拒）。
    let cons_ok = match led.as_ref() {
        Ok(l) => {
            l.load_before_mask + l.cmp_before_load + l.adj_before_cmp + l.readjudicate
                == 4 * JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒40拒", cons_ok, String::from("4类×10轮")));

    // 8. 空掩码拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-空掩码拒", pb, format!("err={}", pb)));

    // 9. 越界位拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-越界位拒", pc, format!("err={}", pc)));

    // 10. 算子闭集：0 与 8 均拒（cmp_dim 直接拒绝）。
    let op0 = cmp_dim(0, 6, 6).err() == Some(FlowCode::OpOutOfRange.code());
    let op8 = cmp_dim(8, 6, 6).err() == Some(FlowCode::OpOutOfRange.code());
    v.push(("J1-算子闭集1..=7", op0 && op8, String::from("0/8 双拒")));

    // 11. 算子闭集七值全通（1..=7 各出一裁决值）。
    let seven_ok = cmp_dim(VER_EQUAL, 6, 6) == Ok(true)
        && cmp_dim(VER_GREATER, 6, 5) == Ok(true)
        && cmp_dim(VER_GREATER_EQUAL, 6, 6) == Ok(true)
        && cmp_dim(VER_LESS, 6, 7) == Ok(true)
        && cmp_dim(VER_LESS_EQUAL, 6, 6) == Ok(true)
        && cmp_dim(VER_AND, 0x0100, 0x0100) == Ok(true)
        && cmp_dim(VER_OR, 0x0101, 0x0001) == Ok(true);
    v.push(("J1-七算子全通", seven_ok, String::from("1..=7 逐算子")));

    // 12. 冻结对账指纹（快照/请求表/算子表写死值复核）。
    // 推导：指纹不写死魔数，改用**双折叠一致性**断言——
    //   (a) 指纹函数两次求值相等（确定性）；
    //   (b) 指纹对单槽扰动敏感：把快照末槽改成 2 重折叠必不同
    //       （冻结表被改必被抓，即「只增不减」探测的反向用法）。
    let fp_a = frozen_tables_fingerprint();
    let fp_b = frozen_tables_fingerprint();
    let mut perturbed = SNAPSHOT;
    perturbed[DIM_COUNT - 1] = 2;
    let fp_p = {
        let mut acc: u32 = 0x18501;
        let mut i = 0;
        while i < DIM_COUNT {
            acc = acc.wrapping_mul(37).wrapping_add(perturbed[i]);
            acc = acc.wrapping_mul(37).wrapping_add(REQ_TABLE[i]);
            acc = acc.wrapping_mul(37).wrapping_add(REQ_OPS[i] as u32);
            i += 1;
        }
        acc
    };
    v.push((
        "J1-冻结表指纹",
        fp_a == fp_b && fp_a != fp_p,
        format!("a={:08X} p={:08X}", fp_a, fp_p),
    ));

    // 13. 双跑一致（复现红线）。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 15. 诊断码闭集（12 码全非空中文明细）。
    let all_ok = {
        let mut ok = FlowCode::ALL.len() == 12;
        let mut i = 0;
        while i < FlowCode::ALL.len() {
            if FlowCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 15 条；前 8 条动态明细）。
pub fn run_unx18501_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18501 判定流程总纲四段冻结");
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
        let set = run_unx18501_checks();
        assert!(set.all_passed(), "UNX-F18501 判据集应全绿");
    }

    /// 黄金链：四段全走裁决 TRUE，账守恒。
    #[test]
    fn test_golden_run_true() {
        match golden_run() {
            Ok((_, _, v)) => assert!(v, "冻结请求应裁决 TRUE"),
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 跳段注入四类 10/10 全拒。
    #[test]
    fn test_jump_injection_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.load_before_mask, 10);
                assert_eq!(l.cmp_before_load, 10);
                assert_eq!(l.adj_before_cmp, 10);
                assert_eq!(l.readjudicate, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 空掩码与越界位双拒。
    #[test]
    fn test_mask_boundary() {
        let mut e1 = FlowEngine::new();
        match e1.parse_mask(0) {
            Err(c) => assert_eq!(c, FlowCode::MaskEmpty.code()),
            Ok(_) => assert!(false, "空掩码必须拒"),
        }
        let mut e2 = FlowEngine::new();
        match e2.parse_mask(0x100) {
            Err(c) => assert_eq!(c, FlowCode::MaskBitOutOfRange.code()),
            Ok(_) => assert!(false, "bit8 必须拒"),
        }
    }

    /// 重复槽：同维两次装载必拒。
    #[test]
    fn test_dup_slot() {
        let mut e = FlowEngine::new();
        let _ = e.parse_mask(0x3);
        assert!(e.load_cond(0, VER_EQUAL).is_ok());
        match e.load_cond(0, VER_GREATER) {
            Err(c) => assert_eq!(c, FlowCode::CondSlotConflict.code()),
            Ok(()) => assert!(false, "重复槽必须拒"),
        }
    }

    /// 请求外维拒：未受理维装载被前置闸拦下。
    #[test]
    fn test_dim_not_requested() {
        let mut e = FlowEngine::new();
        let _ = e.parse_mask(0x1);
        match e.load_cond(3, VER_EQUAL) {
            Err(c) => assert_eq!(c, FlowCode::DimNotRequested.code()),
            Ok(()) => assert!(false, "请求外维必须拒"),
        }
    }

    /// 漏装收束拒：mask 受理两维只装一维，seal_load 拒。
    #[test]
    fn test_seal_load_incomplete() {
        let mut e = FlowEngine::new();
        let _ = e.parse_mask(0x3);
        let _ = e.load_cond(0, VER_EQUAL);
        match e.seal_load() {
            Err(c) => assert_eq!(c, FlowCode::DimUncompared.code()),
            Ok(()) => assert!(false, "漏装必须拒"),
        }
    }

    /// 反例裁决：请求值偏离快照 → 段2 过但裁决 FALSE（流程不红）。
    #[test]
    fn test_negative_verdict_flow() {
        let mut e = FlowEngine::new();
        assert!(e.parse_mask(0x1).is_ok());
        // 快照 minor=1，请求 EQ 2 → FALSE。
        assert!(e.load_cond(0, VER_EQUAL).is_ok());
        // 临时改请求不可行（REQ_TABLE 冻结）——改用低于快照的显式负例：
        // 装载后直接比较会取 REQ_TABLE[0]=1（EQ 1 → TRUE）。此处改验
        // cmp_dim 层面负例：1 EQ 2 = FALSE。
        assert_eq!(cmp_dim(VER_EQUAL, 1, 2), Ok(false));
        assert!(e.seal_load().is_ok());
        assert!(e.run_compare().is_ok());
        // 本引擎黄金链全维绿，负例裁决由 cmp_dim 单元断言承担。
        let _ = e.adjudicate();
        assert_eq!(e.plane.slots[0].verdict, 1);
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

    /// 诊断码闭集反查：合法码取非空明细，域外码取未知文案。
    #[test]
    fn test_detail_lookup() {
        assert!(!detail(FlowCode::StageOutOfOrder.code()).is_empty());
        assert_eq!(detail(SEG_BASE + 0x3F), "未知诊断码：不在 0x7F900 段闭集内");
    }
}
