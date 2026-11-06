//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18514 · 判定探针 20 例（含边界组合）
//! · 0x7FC40 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18514-J1 20 例探针（含 6.3=6.3 边界、跨代比较）与
//!   Windows 对照零偏差
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 探针面冻结：系统快照取 6.3.9600（Windows 8.1 口径：major=6 /
//!     minor=3 / build=9600 / NT 平台），20 例探针逐例发「请求 →
//!     判定 → 与 Windows 参考对拍」，零偏差红线；
//!   - 边界组合覆盖（J1 点名）：例 0 为 6.3=6.3 双维等值边界；
//!     例 5-10 为跨代比较（5.x 时代门 / 10.x 未来门对 6.3 的
//!     GT/LT/LE 三算子）；例 11-16 为 build 邻界（9200 门槛 /
//!     9601 差一 / 10240 跨代 build）；例 17-19 为双维 AND 组合；
//!   - 维度闭式：d0=minor / d1=major / d2=build / d3=platformid，
//!     数值算子闭集 1..=5（本卡无数值维落 AND/OR 面）；基值直读
//!     快照（与 F18512 矩阵口径分工：本卡为真值基线探针）；
//!   - 探针状态机 Idle→Probing→Sealed：行 id 严格升序 + 重复拒 +
//!     Sealed 再探拒；账本环 28 槽定容；
//!   - 注入三类 × 10 轮：期望篡改（取反必偏）/ 算子对调（GT↔LT
//!     非对称例必偏）/ 越维请求（dim≥4 或算子 6/7 必拒）——
//!     10/10 全抓；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] PcCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   SNAPSHOT 快照 / PC_CASES 20 例冻结探针表（手推期望）
//!   [数据结构] PcCase / PcRec / PcRing（28 槽账本环）+ 表指纹
//!   [状态机]   PcPhase 三态 + PcProbe（升序闸/越维闸/偏差闸）
//!   [账本]     PcLedger（尝试 = 过 + 拒 守恒）+ InjectLedger
//!   [注入核]   三类 × 10 轮（篡改/对调/越维）
//!   [收口]     golden_run + run_unx18514_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FC40，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FC40;

// ── 域冻结常量（winnt.h 口径，闭式重演） ────────────────────────
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
/// 维度号：d0 = MINOR。
pub const DIM_MINOR: u8 = 0;
/// 维度号：d1 = MAJOR。
pub const DIM_MAJOR: u8 = 1;
/// 维度号：d2 = BUILD。
pub const DIM_BUILD: u8 = 2;
/// 维度号：d3 = PLATFORMID。
pub const DIM_PLATFORM: u8 = 3;
/// 系统快照（6.3.9600 / NT）：minor=3。
pub const SN_MINOR: u32 = 3;
/// 系统快照：major=6。
pub const SN_MAJOR: u32 = 6;
/// 系统快照：build=9600。
pub const SN_BUILD: u32 = 9600;
/// 系统快照：platformid=2（VER_PLATFORM_WIN32_NT）。
pub const SN_PLATFORM: u32 = 2;
/// 探针表行数（J1「20 例」口径）。
pub const PC_ROWS: usize = 20;
/// 账本环容量（定容 28 槽：20 例 + 8 余量）。
pub const RING_CAP: usize = 28;
/// 注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 快照按维取基值（越界维 None）。
pub fn snap_of(dim: u8) -> Option<u32> {
    match dim {
        DIM_MINOR => Some(SN_MINOR),
        DIM_MAJOR => Some(SN_MAJOR),
        DIM_BUILD => Some(SN_BUILD),
        DIM_PLATFORM => Some(SN_PLATFORM),
        _ => None,
    }
}

/// 数值算子比较核（闭集 1..=5；6/7 与 0/>7 均拒）。
pub fn cmp_num(op: u8, basis: u32, req: u32) -> Result<bool, u32> {
    match op {
        1 => Ok(basis == req),
        2 => Ok(basis > req),
        3 => Ok(basis >= req),
        4 => Ok(basis < req),
        5 => Ok(basis <= req),
        _ => Err(PcCode::OpIllegal.code()),
    }
}

/// 探针表一行（单/双维请求 + 期望）。
/// dual=false 时 dim_b/op_b/req_b 不参与（置 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcCase {
    /// 维 A。
    pub dim_a: u8,
    /// 维 A 算子。
    pub op_a: u8,
    /// 维 A 请求值。
    pub req_a: u32,
    /// 维 B（dual 时参与 AND 聚合）。
    pub dim_b: u8,
    /// 维 B 算子。
    pub op_b: u8,
    /// 维 B 请求值。
    pub req_b: u32,
    /// 双维标志。
    pub dual: bool,
    /// Windows 参考期望（手推写死）。
    pub expect: bool,
}

/// 第 i 例（越界 None）。
/// 推导：例 0 双维等值（6.3=6.3 边界）；1-4 双维 minor 邻界；
/// 5-10 major 三算子跨代；11-16 build 邻界与跨代；17-19 双维组合
/// 收尾。expect 全部按快照 6.3.9600 手推。
pub fn pc_case(i: usize) -> Option<PcCase> {
    match i {
        0 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_EQUAL, req_a: 6,
            dim_b: DIM_MINOR, op_b: VER_EQUAL, req_b: 3,
            dual: true, expect: true,
        }), // 6.3=6.3 边界
        1 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_EQUAL, req_a: 6,
            dim_b: DIM_MINOR, op_b: VER_LESS_EQUAL, req_b: 3,
            dual: true, expect: true,
        }),
        2 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_EQUAL, req_a: 6,
            dim_b: DIM_MINOR, op_b: VER_LESS_EQUAL, req_b: 2,
            dual: true, expect: false,
        }), // minor 差一
        3 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_GREATER_EQUAL, req_a: 6,
            dim_b: DIM_MINOR, op_b: VER_GREATER_EQUAL, req_b: 3,
            dual: true, expect: true,
        }),
        4 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_GREATER_EQUAL, req_a: 6,
            dim_b: DIM_MINOR, op_b: VER_GREATER_EQUAL, req_b: 4,
            dual: true, expect: false,
        }), // minor 差一
        5 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_GREATER, req_a: 5,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: true,
        }), // 跨代：5.x 门
        6 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_GREATER, req_a: 6,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: false,
        }),
        7 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_LESS, req_a: 10,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: true,
        }), // 跨代：10.x 未来门
        8 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_LESS, req_a: 6,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: false,
        }),
        9 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_LESS_EQUAL, req_a: 6,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: true,
        }),
        10 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_LESS_EQUAL, req_a: 5,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: false,
        }),
        11 => Some(PcCase {
            dim_a: DIM_BUILD, op_a: VER_EQUAL, req_a: 9600,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: true,
        }),
        12 => Some(PcCase {
            dim_a: DIM_BUILD, op_a: VER_EQUAL, req_a: 9601,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: false,
        }), // build 差一
        13 => Some(PcCase {
            dim_a: DIM_BUILD, op_a: VER_GREATER_EQUAL, req_a: 9200,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: true,
        }), // 8.1 build 门槛
        14 => Some(PcCase {
            dim_a: DIM_BUILD, op_a: VER_GREATER_EQUAL, req_a: 9700,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: false,
        }),
        15 => Some(PcCase {
            dim_a: DIM_BUILD, op_a: VER_LESS_EQUAL, req_a: 9600,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: true,
        }),
        16 => Some(PcCase {
            dim_a: DIM_BUILD, op_a: VER_LESS, req_a: 10240,
            dim_b: 0, op_b: 0, req_b: 0,
            dual: false, expect: true,
        }), // 跨代 build（10 的 build）
        17 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_GREATER_EQUAL, req_a: 6,
            dim_b: DIM_MINOR, op_b: VER_GREATER_EQUAL, req_b: 1,
            dual: true, expect: true,
        }), // 8.1 最低门
        18 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_GREATER_EQUAL, req_a: 10,
            dim_b: DIM_MINOR, op_b: VER_GREATER_EQUAL, req_b: 0,
            dual: true, expect: false,
        }), // 跨代拒
        19 => Some(PcCase {
            dim_a: DIM_MAJOR, op_a: VER_EQUAL, req_a: 6,
            dim_b: DIM_BUILD, op_b: VER_GREATER_EQUAL, req_b: 9200,
            dual: true, expect: true,
        }), // major+build 组合
        _ => None,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcCode {
    /// 行 id 越界（≥ 20）。
    RowIdOutOfRange,
    /// 探针乱序（id 未严格升序）。
    RowOutOfOrder,
    /// 探针重复（同 id 二探）。
    RowDuplicate,
    /// 越维请求（dim ≥ 4）。
    DimOutOfRange,
    /// 算子越界（0/6/7/>7 落数值维）。
    OpIllegal,
    /// 探针偏差（got ≠ expect，零偏差红线）。
    ProbeDeviation,
    /// 账本环满（28 槽耗尽）。
    LedgerFull,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 表指纹失配。
    FingerprintMismatch,
    /// 空表收口（零探针即封账）。
    TableEmpty,
    /// 阶段非法（Sealed 后再探）。
    PhaseInvalid,
    /// 双跑不一致。
    NotReproducible,
}

impl PcCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PcCode::RowIdOutOfRange => "行 id 越界：探针表仅 0..=19",
            PcCode::RowOutOfOrder => "探针乱序：id 必须严格升序",
            PcCode::RowDuplicate => "探针重复：同 id 只探一次",
            PcCode::DimOutOfRange => "越维请求：维度闭集 0..=3",
            PcCode::OpIllegal => "算子越界：数值维闭集 1..=5",
            PcCode::ProbeDeviation => "探针偏差：got≠expect 零容忍",
            PcCode::LedgerFull => "账本环满：28 槽耗尽",
            PcCode::LedgerBroken => "账本守恒破坏：尝试≠过+拒",
            PcCode::FingerprintMismatch => "表指纹失配：冻结表被改",
            PcCode::TableEmpty => "空表收口：零探针不得封账",
            PcCode::PhaseInvalid => "阶段非法：Sealed 后再探",
            PcCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PcCode; 12] = [
        PcCode::RowIdOutOfRange,
        PcCode::RowOutOfOrder,
        PcCode::RowDuplicate,
        PcCode::DimOutOfRange,
        PcCode::OpIllegal,
        PcCode::ProbeDeviation,
        PcCode::LedgerFull,
        PcCode::LedgerBroken,
        PcCode::FingerprintMismatch,
        PcCode::TableEmpty,
        PcCode::PhaseInvalid,
        PcCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PcCode::ALL.len() {
        if PcCode::ALL[i].code() == code {
            return PcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FC40 段闭集内"
}

// ── 账本环（28 槽定容） ─────────────────────────────────────────
/// 探针记录（Copy）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcRec {
    /// 行 id。
    pub id: u8,
    /// 引擎评值。
    pub got: bool,
    /// Windows 参考期望。
    pub expect: bool,
}

/// 28 槽账本环：定容、显式满、账不丢。
#[derive(Clone, Copy, Debug)]
pub struct PcRing {
    slots: [Option<PcRec>; RING_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl PcRing {
    pub fn new() -> PcRing {
        PcRing { slots: [None; RING_CAP], len: 0, full_rejects: 0 }
    }
    /// 入环（满则显式拒并计数，旧账不动）。
    pub fn push(&mut self, rec: PcRec) -> Result<(), u32> {
        if self.len >= RING_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(PcCode::LedgerFull.code());
        }
        self.slots[self.len] = Some(rec);
        self.len += 1;
        Ok(())
    }
    /// 按槽位取记录（越界 None）。
    pub fn rec_at(&self, idx: usize) -> Option<PcRec> {
        if idx >= self.len {
            return None;
        }
        self.slots[idx]
    }
    /// 清环（幂等）。
    pub fn reset(&mut self) {
        let mut i = 0;
        while i < RING_CAP {
            self.slots[i] = None;
            i += 1;
        }
        self.len = 0;
    }
}

// ── 表指纹（冻结对账） ──────────────────────────────────────────
/// 探针表指纹：31 进制逐例折叠（行序 + 双维 + 期望位敏感）。
/// 推导：初值取卡号派生 0x18514；期望以 1/2 入折。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18514;
    let mut i = 0;
    while i < PC_ROWS {
        match pc_case(i) {
            Some(c) => {
                acc = acc.wrapping_mul(31).wrapping_add(c.dim_a as u32);
                acc = acc.wrapping_mul(31).wrapping_add(c.op_a as u32);
                acc = acc.wrapping_mul(31).wrapping_add(c.req_a);
                acc = acc.wrapping_mul(31).wrapping_add(c.dim_b as u32);
                acc = acc.wrapping_mul(31).wrapping_add(c.op_b as u32);
                acc = acc.wrapping_mul(31).wrapping_add(c.req_b);
                acc = acc.wrapping_mul(31).wrapping_add(if c.dual { 3 } else { 4 });
                acc = acc.wrapping_mul(31).wrapping_add(if c.expect { 1 } else { 2 });
            }
            None => {
                acc = acc.wrapping_mul(31).wrapping_add(0xFFFF);
            }
        }
        i += 1;
    }
    acc
}

// ── 探针状态机（Idle → Probing → Sealed） ───────────────────────
/// 探针阶段（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcPhase {
    /// 未开探。
    Idle,
    /// 探测中。
    Probing,
    /// 已封账（终态）。
    Sealed,
}

/// 探针账（尝试 = 过 + 拒，守恒方程）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcLedger {
    pub tries: u32,
    pub passes: u32,
    pub rejects: u32,
}

impl PcLedger {
    pub const fn new() -> PcLedger {
        PcLedger { tries: 0, passes: 0, rejects: 0 }
    }
    /// 守恒核算。
    pub fn conserve(&self) -> bool {
        self.tries == self.passes + self.rejects
    }
}

/// 探针引擎：升序闸 + 越维闸 + 偏差闸 三闸串联。
pub struct PcProbe {
    pub phase: PcPhase,
    /// 下一个允许的 id 下界（严格升序约束）。
    next_min: usize,
    /// 上一探 id（重复判定）。
    last_probed: Option<usize>,
    pub ring: PcRing,
    pub ledger: PcLedger,
    /// 评值偏置（注入面：正常恒 None；1=期望取反 2=算子对调）。
    bias: Option<u8>,
}

impl PcProbe {
    pub fn new() -> PcProbe {
        PcProbe {
            phase: PcPhase::Idle,
            next_min: 0,
            last_probed: None,
            ring: PcRing::new(),
            ledger: PcLedger::new(),
            bias: None,
        }
    }

    /// 注入偏置（测试/注入核专用）。
    pub fn set_bias(&mut self, b: Option<u8>) {
        self.bias = b;
    }

    /// 单例评值：维闸 → 算子核 →（双维）AND 聚合；偏置面见注入核。
    fn eval(&self, id: usize) -> Result<bool, u32> {
        let c = match pc_case(id) {
            Some(x) => x,
            None => return Err(PcCode::RowIdOutOfRange.code()),
        };
        // 越维闸。
        if snap_of(c.dim_a).is_none() || (c.dual && snap_of(c.dim_b).is_none()) {
            return Err(PcCode::DimOutOfRange.code());
        }
        let ba = match snap_of(c.dim_a) {
            Some(v) => v,
            None => return Err(PcCode::DimOutOfRange.code()),
        };
        let ra = match (self.bias, c.op_a) {
            // 偏置 2：算子对调（GT↔LT / GE↔LE 成对镜像）；正常面不翻。
            (Some(2), 2) => VER_LESS,
            (Some(2), 3) => VER_LESS_EQUAL,
            (Some(2), 4) => VER_GREATER,
            (Some(2), 5) => VER_GREATER_EQUAL,
            (_, x) => x,
        };
        let got_a = cmp_num(ra, ba, c.req_a)?;
        if !c.dual {
            return Ok(got_a);
        }
        let bb = match snap_of(c.dim_b) {
            Some(v) => v,
            None => return Err(PcCode::DimOutOfRange.code()),
        };
        let got_b = cmp_num(c.op_b, bb, c.req_b)?;
        Ok(got_a && got_b)
    }

    /// 期望取值（偏置 1：取反——篡改注入面）。
    fn expect_of(&self, id: usize) -> Result<bool, u32> {
        let e = match pc_case(id) {
            Some(c) => c.expect,
            None => return Err(PcCode::RowIdOutOfRange.code()),
        };
        Ok(match self.bias {
            Some(1) => !e,
            _ => e,
        })
    }

    /// 单行探针：id 严格升序 + got==expect 零偏差。
    pub fn probe(&mut self, id: usize) -> Result<(), u32> {
        if self.phase == PcPhase::Sealed {
            return Err(PcCode::PhaseInvalid.code());
        }
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if id >= PC_ROWS {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(PcCode::RowIdOutOfRange.code());
        }
        if id < self.next_min {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            if self.last_probed == Some(id) {
                return Err(PcCode::RowDuplicate.code());
            }
            return Err(PcCode::RowOutOfOrder.code());
        }
        let got = self.eval(id)?;
        let expect = self.expect_of(id)?;
        let rec = PcRec { id: id as u8, got, expect };
        self.ring.push(rec)?;
        if got != expect {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(PcCode::ProbeDeviation.code());
        }
        self.next_min = id + 1;
        self.last_probed = Some(id);
        self.ledger.passes = self.ledger.passes.saturating_add(1);
        self.phase = PcPhase::Probing;
        Ok(())
    }

    /// 全表顺探（0..20 逐例；确定性：同表双跑账逐槽相等）。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut id = 0usize;
        while id < PC_ROWS {
            self.probe(id)?;
            n += 1;
            id += 1;
        }
        Ok(n)
    }

    /// 封账：空表拒；账守恒；Sealed 终态。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == PcPhase::Idle {
            return Err(PcCode::TableEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(PcCode::LedgerBroken.code());
        }
        self.phase = PcPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过例数 / 探针账 / 指纹）。
pub type GoldenOut = (usize, PcLedger, u32);

/// 黄金全链一次：20 例顺探全过零偏差 → 封账 → 指纹入账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut p = PcProbe::new();
    let n = p.probe_all()?;
    if n != PC_ROWS {
        return Err(PcCode::LedgerBroken.code());
    }
    p.seal()?;
    if !p.ledger.conserve() {
        return Err(PcCode::LedgerBroken.code());
    }
    if p.ledger.passes != PC_ROWS as u32 || p.ledger.rejects != 0 {
        return Err(PcCode::ProbeDeviation.code());
    }
    Ok((n, p.ledger, table_fingerprint()))
}

/// 双跑一致（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PcCode::NotReproducible.code());
    }
    Ok(())
}

// ── 交叉对账（数值维五算子闭式对拍） ───────────────────────────
/// 交叉核对 7 例：本卡 cmp_num 与通用数值比较臂（闭式）逐例相等。
pub fn cross_check() -> Result<(), u32> {
    let rows: [(u8, u32, u32, bool); 7] = [
        (VER_EQUAL, 6, 6, true),
        (VER_EQUAL, 6, 7, false),
        (VER_GREATER, 6, 5, true),
        (VER_GREATER, 6, 6, false),
        (VER_GREATER_EQUAL, 6, 6, true),
        (VER_LESS, 6, 7, true),
        (VER_LESS_EQUAL, 6, 5, false),
    ];
    let mut i = 0;
    while i < rows.len() {
        let (op, s, r, master) = rows[i];
        if cmp_num(op, s, r)? != master {
            return Err(PcCode::FingerprintMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入面计数（魔数推导） ──────────────────────────────────────
/// 算子对调偏差例数（运行期推导：对调后裁决 ≠ 期望的例数）。
pub fn swap_deviation_cases() -> Result<u32, u32> {
    let mut n = 0u32;
    let mut i = 0;
    while i < PC_ROWS {
        let mut p = PcProbe::new();
        p.set_bias(Some(2));
        // 直接走评值路径（不入环不推下界）。
        let got = p.eval(i)?;
        let e = match pc_case(i) {
            Some(c) => c.expect,
            None => return Err(PcCode::RowIdOutOfRange.code()),
        };
        if got != e {
            n = n.saturating_add(1);
        }
        i += 1;
    }
    Ok(n)
}

// ── 注入核（三类 × 10 轮，10/10 全抓） ──────────────────────────
/// 注入账（三类各轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 期望篡改（取反必偏）。
    pub tamper_caught: u32,
    /// 类2 算子对调（镜像非对称例必偏）。
    pub swap_caught: u32,
    /// 类3 越维请求（dim≥4 / 算子 6 必拒）。
    pub dimop_caught: u32,
}

/// 三类变异注入 rounds 轮；每类每轮必被抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let swap_expect = swap_deviation_cases()?;
    let mut r = 0;
    while r < rounds {
        // 类1 期望篡改：例 0 期望取反 → 必记 ProbeDeviation。
        let mut p1 = PcProbe::new();
        p1.set_bias(Some(1));
        match p1.probe(0) {
            Err(c) if c == PcCode::ProbeDeviation.code() => {
                led.tamper_caught = led.tamper_caught.saturating_add(1);
            }
            _ => return Err(PcCode::FingerprintMismatch.code()),
        }
        // 类2 算子对调：偏置探针全表走，镜像非对称例必偏。
        let mut p2 = PcProbe::new();
        p2.set_bias(Some(2));
        let mut caught2 = 0u32;
        let mut id2 = 0usize;
        while id2 < PC_ROWS {
            match p2.probe(id2) {
                Err(c) if c == PcCode::ProbeDeviation.code() => {
                    caught2 = caught2.saturating_add(1);
                    p2.next_min = id2 + 1;
                }
                Ok(()) => {}
                _ => return Err(PcCode::FingerprintMismatch.code()),
            }
            id2 += 1;
        }
        if caught2 != swap_expect {
            return Err(PcCode::FingerprintMismatch.code());
        }
        led.swap_caught = led.swap_caught.saturating_add(caught2);
        // 类3 越维请求：dim=7（越界）与 op=6（套件算子落数值维）必拒。
        // 越维：经维闸直查（dim=7 无基值、dim=3 在界内对照）。
        let dim_gate_ok = snap_of(7).is_none() && snap_of(DIM_PLATFORM).is_some();
        if !dim_gate_ok {
            return Err(PcCode::DimOutOfRange.code());
        }
        let op_gate_ok = match cmp_num(6, 9600, 9600) {
            Err(c) if c == PcCode::OpIllegal.code() => true,
            _ => false,
        };
        if !op_gate_ok {
            return Err(PcCode::OpIllegal.code());
        }
        led.dimop_caught = led.dimop_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：20 例顺探全过零偏差（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：乱序（先探 5 再探 2）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = PcProbe::new();
    p.probe(5)?;
    p.probe(2)
}

/// 探针 C：重复（连探同 id）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut p = PcProbe::new();
    p.probe(0)?;
    p.probe(0)
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < PcCode::ALL.len() {
        let v = PcCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（18 条；动态明细 ≤8） ──────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：20 例全过零偏差（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == PC_ROWS && led.conserve() && led.rejects == 0,
        Err(_) => false,
    };
    v.push(("J1-20例探针全过", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A全表顺探", pa, format!("ok={}", pa)));

    // 3. 6.3=6.3 边界直证（例 0 双维等值过；例 2 minor 差一按期望
    //    FALSE 零偏差通过——差一被正确判否即边界语义成立）。
    let bnd = {
        let mut p = PcProbe::new();
        p.probe(0).is_ok() && p.probe(1).is_ok() && p.probe(2).is_ok()
    };
    v.push(("J1-6.3=6.3边界直证", bnd, String::from("等值过/差一判否")));

    // 4. 跨代比较直证（例 5 过 / 例 18 拒）。
    let gen = {
        let mut p = PcProbe::new();
        p.probe(5).is_ok() && p.probe(18).is_ok()
    };
    v.push(("J1-跨代比较直证", gen, String::from("5.x门过/10.x门拒")));

    // 5. 期望篡改注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.tamper_caught,
        Err(_) => 0,
    };
    v.push(("J1-篡改注入10/10", c1 == JUDGE_ROUNDS, format!("caught={}", c1)));

    // 6. 算子对调注入（面额 = swap_deviation_cases 运行期推导）。
    let c2 = match led.as_ref() {
        Ok(l) => l.swap_caught,
        Err(_) => 0,
    };
    let e2 = match swap_deviation_cases() {
        Ok(n) => n * JUDGE_ROUNDS,
        Err(_) => 0,
    };
    v.push(("J1-对调注入全偏", c2 == e2, format!("{}/{}", c2, e2)));

    // 7. 越维/越算子注入 10/10。
    let c3 = match led.as_ref() {
        Ok(l) => l.dimop_caught,
        Err(_) => 0,
    };
    v.push(("J1-越维越算子10/10", c3 == JUDGE_ROUNDS, format!("caught={}", c3)));

    // 8. 注入账守恒（三类合计 = 面额 × 轮数）。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.tamper_caught + l.swap_caught + l.dimop_caught == JUDGE_ROUNDS + e2 + JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("3类面额×10轮")));

    // 9. 乱序探针拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-乱序探针拒", pb, format!("err={}", pb)));

    // 10. 重复探针拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-重复探针拒", pc, format!("err={}", pc)));

    // 11. 越界 id 拒（id=20）。
    let oob = {
        let mut p = PcProbe::new();
        p.probe(PC_ROWS).is_err()
    };
    v.push(("J1-越界id拒", oob, format!("id={}", PC_ROWS)));

    // 12. 空表封账拒 + Sealed 再探拒（三态闸）。
    let phases = {
        let mut p0 = PcProbe::new();
        let empty_rej = match p0.seal() {
            Err(c) if c == PcCode::TableEmpty.code() => true,
            _ => false,
        };
        let mut p1 = PcProbe::new();
        let _ = p1.probe_all();
        let sealed_ok = p1.seal().is_ok();
        let post_rej = match p1.probe(0) {
            Err(c) if c == PcCode::PhaseInvalid.code() => true,
            _ => false,
        };
        empty_rej && sealed_ok && post_rej
    };
    v.push(("J1-三态封账闸", phases, String::from("空拒/封过/后再探拒")));

    // 13. 环容量：压环至满 + 1 显式满拒且旧账不丢。
    let cap = {
        let mut ring = PcRing::new();
        let mut k = 0;
        while k < RING_CAP {
            let _ = ring.push(PcRec { id: k as u8, got: true, expect: true });
            k += 1;
        }
        match ring.push(PcRec { id: 99, got: true, expect: true }) {
            Err(c) if c == PcCode::LedgerFull.code() => {
                ring.len == RING_CAP && ring.full_rejects == 1
            }
            _ => false,
        }
    };
    v.push(("J1-环容量28满拒", cap, String::from("28 收 1 拒账不丢")));

    // 14. 交叉对账（数值维五算子 7 例闭式对拍）。
    let cc = cross_check().is_ok();
    v.push(("J1-交叉对账7例", cc, String::from("数值维同口径")));

    // 15. build 邻界三型直证（9600 等值过 / 9601 差一拒 / 9200 门槛过）。
    let build3 = {
        let mut p = PcProbe::new();
        p.probe(11).is_ok() && p.probe(12).is_ok() && p.probe(13).is_ok()
    };
    v.push(("J1-build邻界三型", build3, String::from("等值/差一/门槛")));

    // 16. 表指纹双跑一致。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 17. 双跑一致（复现红线）。
    let rp = repro_check().is_ok();
    v.push(("J2-双跑一致", rp, String::from("golden 两跑同值")));

    // 18. 段位守卫 + 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = segment_guard() && PcCode::ALL.len() == 12;
        let mut i = 0;
        while i < PcCode::ALL.len() {
            if PcCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-段位与闭集", all_ok, format!("{:06X} 12码", SEG_BASE)));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18514_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18514 判定探针 20 例边界组合");
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
        let set = run_unx18514_checks();
        assert!(set.all_passed(), "UNX-F18514 判据集应全绿");
    }

    /// 黄金链：20 例全过零偏差。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 20);
                assert_eq!(led.passes, 20);
                assert_eq!(led.rejects, 0);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 探针表逐例手推期望对拍（20 例逐一直评）。
    #[test]
    fn test_cases_one_by_one() {
        let mut i = 0;
        while i < PC_ROWS {
            let p = PcProbe::new();
            let got = match p.eval(i) {
                Ok(b) => b,
                Err(e) => panic!("例 {} 不应报错: {}", i, detail(e)),
            };
            let e = match pc_case(i) {
                Some(c) => c.expect,
                None => {
                    assert!(false, "例 {} 越界", i);
                    return;
                }
            };
            assert_eq!(got, e, "例 {} 与 Windows 参考零偏差", i);
            i += 1;
        }
    }

    /// 边界组合：6.3=6.3 / minor 差一 / build 差一 / 跨代。
    #[test]
    fn test_boundary_combos() {
        let p = PcProbe::new();
        // 例 0：6.3=6.3 双维等值。
        assert_eq!(p.eval(0), Ok(true));
        // 例 2：minor 差一（3 vs ≤2）。
        assert_eq!(p.eval(2), Ok(false));
        // 例 12：build 差一（9600 vs 9601）。
        assert_eq!(p.eval(12), Ok(false));
        // 例 5：5.x 跨代门（6>5）。
        assert_eq!(p.eval(5), Ok(true));
        // 例 18：10.x 跨代门（6≥10 假）。
        assert_eq!(p.eval(18), Ok(false));
    }

    /// 注入三类全抓（10 轮累计）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.tamper_caught, 10);
                let e = match swap_deviation_cases() {
                    Ok(n) => n * 10,
                    Err(_) => 0,
                };
                assert_eq!(l.swap_caught, e);
                assert_eq!(l.dimop_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 三闸 + 越维直证。
    #[test]
    fn test_gates() {
        assert!(probe_seq_b().is_err(), "乱序必须拒");
        assert!(probe_seq_c().is_err(), "重复必须拒");
        let mut p = PcProbe::new();
        match p.probe(PC_ROWS) {
            Err(c) => assert_eq!(c, PcCode::RowIdOutOfRange.code()),
            Ok(()) => assert!(false, "越界 id 必须拒"),
        }
        assert!(snap_of(7).is_none(), "dim=7 必须越界");
        match cmp_num(6, 9600, 9600) {
            Err(c) => assert_eq!(c, PcCode::OpIllegal.code()),
            Ok(_) => assert!(false, "算子 6 落数值维必须拒"),
        }
    }

    /// 环容量：满后显式拒且账不丢。
    #[test]
    fn test_ring_capacity() {
        let mut ring = PcRing::new();
        let mut k = 0;
        while k < RING_CAP {
            assert!(ring.push(PcRec { id: k as u8, got: true, expect: true }).is_ok());
            k += 1;
        }
        match ring.push(PcRec { id: 99, got: true, expect: true }) {
            Err(c) => {
                assert_eq!(c, PcCode::LedgerFull.code());
                assert_eq!(ring.len, RING_CAP);
                assert_eq!(ring.full_rejects, 1);
            }
            Ok(()) => assert!(false, "满环必须拒"),
        }
        assert!(ring.rec_at(0).is_some());
        assert!(ring.rec_at(RING_CAP).is_none());
    }

    /// 双跑一致 + 指纹稳定。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(table_fingerprint(), table_fingerprint());
    }

    /// 段位守卫 + 诊断码闭集。
    #[test]
    fn test_segment_and_codes() {
        assert!(segment_guard());
        assert_eq!(PcCode::ALL.len(), 12);
        for c in PcCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
