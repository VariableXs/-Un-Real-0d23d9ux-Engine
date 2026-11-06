//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18506 · VER_CONDITION 掩码位构造（VerSetConditionMask 联动）
//! · 0x7FA40 段 · 批 06 · 配额 340
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18506-J1 掩码位与条件算子配对断言全过，位错位注入
//!   10/10 检出。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 位域口径冻结：**每维 3 位域**（shift = 3·dim，u64 条件掩码）。
//!     推导：算子闭集 1..=7（VER_OR=7=111b）需 3 位——2 位域装不下
//!     7；64 位容器容纳 21 维（3·21+3 = 66 > 63，故 dim ≤ 20 安全，
//!     本域 dwTypeMask 全集仅 8 维 0..=7，24 位足够）；域间互不重叠、
//!     与 Windows「每维独立位域、逐位配对」的 ULONGLONG 口径同构；
//!   - 构造核 set_cond：cond | (op << 3·dim)；同维同算子重设 = 幂等
//!     （返回同值、记 idem 不记冲突）；同维异算子重设 = 冲突拒；
//!   - 配对断言：decode(cond, dim) == 期望算子逐维全等 + **杂散位闸**
//!     （cond 在未请求维的位域内不得有任何残留位）；
//!   - 五条冻结链（掩码/算子表写死）：零维、双维、三维、四维、全八维
//!     ——逐链 构造→验证→消费 三段过；
//!   - 位错位注入三类 × 10 轮：移位错位（shift+1 落入邻域）/ 截位
//!     （op&3 高位丢失，算子 5..7 必错）/ 漏设（请求维域空）——
//!     验证段/消费段必抓，漏检即红；
//!   - 状态机：Empty → Building → Validated → Consumed（单向）；
//!   - 账本：set_tries = set_ok + conflicts + 幂等，守恒核算；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] CmCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   FROZEN_CHAINS 五条冻结链（掩码+算子表）
//!   [数据结构] CondMaskPlane（8 维 3 位域位面）/ 链记录
//!   [状态机]   BuildPhase 四态 + MaskBuilder（配对验证 + 杂散位闸）
//!   [账本]     CmLedger（set 守恒 + 注入守恒）
//!   [注入核]   三类位错位 × 10 轮（移位/截位/漏设）
//!   [收口]     golden_run + run_unx18506_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FA40，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7FA40;

// ── 域冻结常量 ──────────────────────────────────────────────────
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
/// VER_AND（冻结）。
pub const VER_AND: u8 = 6;
/// VER_OR（冻结）。
pub const VER_OR: u8 = 7;
/// 维数（dwTypeMask 有效位 0..=7）。
pub const DIM_COUNT: usize = 8;
/// 位域宽（3 位/维，推导见头部）。
pub const FIELD_BITS: u32 = 3;
/// 位域掩码（0b111）。
pub const FIELD_MASK: u64 = 0b111;
/// 容器安全维上界（3·21+3 = 66 > 63，故 21 维封顶）。
pub const MAX_DIM: u8 = 21;
/// 冻结链数。
pub const CHAIN_COUNT: usize = 5;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 冻结链（掩码 u32 + 算子表 [u8;8]，未请求维算子写 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenChain {
    /// 请求掩码（dwTypeMask）。
    pub mask: u32,
    /// 逐维算子（未请求维 0）。
    pub ops: [u8; DIM_COUNT],
}

/// 五条冻结链（手写定值）。
/// 推导：链0 零维（空链退化为纯验证）/ 链1 双维（d0 EQ + d1 GE）/
/// 链2 三维（d0 LE + d2 LE_EQUAL + d5 GE）/ 链3 四维（d1 GT + d2 EQ
/// + d3 LESS + d4 GE）/ 链4 全八维（套件 AND + 产品型 EQ 收尾）。
pub const FROZEN_CHAINS: [FrozenChain; CHAIN_COUNT] = [
    FrozenChain { mask: 0x0000_0000, ops: [0, 0, 0, 0, 0, 0, 0, 0] },
    FrozenChain {
        mask: 0x0000_0003,
        ops: [VER_EQUAL, VER_GREATER_EQUAL, 0, 0, 0, 0, 0, 0],
    },
    FrozenChain {
        mask: 0x0000_0025,
        ops: [VER_LESS, 0, VER_LESS_EQUAL, 0, 0, VER_GREATER_EQUAL, 0, 0],
    },
    FrozenChain {
        mask: 0x0000_001E,
        ops: [0, VER_GREATER, VER_EQUAL, VER_LESS, VER_GREATER_EQUAL, 0, 0, 0],
    },
    FrozenChain {
        mask: 0x0000_00FF,
        ops: [
            VER_EQUAL,
            VER_GREATER_EQUAL,
            VER_GREATER_EQUAL,
            VER_EQUAL,
            VER_LESS_EQUAL,
            VER_GREATER_EQUAL,
            VER_AND,
            VER_EQUAL,
        ],
    },
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmCode {
    /// 空掩码构造（mask==0 且要求非空）。
    MaskEmpty,
    /// 维越界（dim > 7 或 > 21）。
    DimOutOfRange,
    /// 算子越界（0 或 > 7）。
    OpOutOfRange,
    /// 同维异算子重设（冲突）。
    CondRewriteConflict,
    /// 移位溢出（3·dim 越出 64 位容器）。
    ShiftOverflow,
    /// 配对失配（decode ≠ 期望算子）。
    PairMismatch,
    /// 杂散位（未请求维位域有残留）。
    StrayBits,
    /// 域空（请求维未设算子）。
    FieldMissing,
    /// 阶段非法（乱段消费）。
    PhaseInvalid,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 链指纹失配（冻结链被改）。
    ChainFingerprint,
    /// 双跑不一致。
    NotReproducible,
}

impl CmCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CmCode::MaskEmpty => "空掩码：mask==0 不得构造",
            CmCode::DimOutOfRange => "维越界：dwTypeMask 仅 bit 0..=7",
            CmCode::OpOutOfRange => "算子越界：闭集 1..=7 外",
            CmCode::CondRewriteConflict => "同维重设冲突：异算子不得覆写",
            CmCode::ShiftOverflow => "移位溢出：3·dim 越出 64 位容器",
            CmCode::PairMismatch => "配对失配：解码算子≠期望",
            CmCode::StrayBits => "杂散位：未请求维位域有残留",
            CmCode::FieldMissing => "域空：请求维未设算子",
            CmCode::PhaseInvalid => "阶段非法：乱段消费",
            CmCode::LedgerBroken => "账本守恒破坏",
            CmCode::ChainFingerprint => "链指纹失配：冻结链被改",
            CmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CmCode; 12] = [
        CmCode::MaskEmpty,
        CmCode::DimOutOfRange,
        CmCode::OpOutOfRange,
        CmCode::CondRewriteConflict,
        CmCode::ShiftOverflow,
        CmCode::PairMismatch,
        CmCode::StrayBits,
        CmCode::FieldMissing,
        CmCode::PhaseInvalid,
        CmCode::LedgerBroken,
        CmCode::ChainFingerprint,
        CmCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CmCode::ALL.len() {
        if CmCode::ALL[i].code() == code {
            return CmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FA40 段闭集内"
}

// ── 构造核 ──────────────────────────────────────────────────────
/// 单位域设值（VerSetConditionMask 同构）：cond | (op << 3·dim)。
/// 重复设同算子 = 幂等（返回同值）；重复设异算子 = 冲突拒。
pub fn set_cond(cond: u64, dim: u8, op: u8) -> Result<u64, u32> {
    // 算子闭集闸。
    if op == 0 || op > VER_OR {
        return Err(CmCode::OpOutOfRange.code());
    }
    // 维越界闸（域全集 0..=7）。
    if dim as usize >= DIM_COUNT {
        return Err(CmCode::DimOutOfRange.code());
    }
    // 容器安全闸（3·dim+3 ≤ 64）。
    if dim > MAX_DIM || (dim as u32) * FIELD_BITS + FIELD_BITS > 64 {
        return Err(CmCode::ShiftOverflow.code());
    }
    // 同维重设闸：已有非零域且异算子 → 冲突。
    let shift = (dim as u32) * FIELD_BITS;
    let cur = (cond >> shift) & FIELD_MASK;
    if cur != 0 && cur != op as u64 {
        return Err(CmCode::CondRewriteConflict.code());
    }
    Ok(cond | ((op as u64) << shift))
}

/// 位域解码：(cond >> 3·dim) & 0b111（dim 越界返回 0）。
pub fn decode_op(cond: u64, dim: u8) -> u8 {
    if dim as usize >= DIM_COUNT {
        return 0;
    }
    let shift = (dim as u32) * FIELD_BITS;
    ((cond >> shift) & FIELD_MASK) as u8
}

/// 期望位面占位：掩码内各维域的并集（杂散位闸基准）。
pub fn expected_bits(mask: u32) -> u64 {
    let mut bits = 0u64;
    let mut d = 0u8;
    while (d as usize) < DIM_COUNT {
        if mask & (1u32 << d) != 0 {
            bits |= FIELD_MASK << ((d as u32) * FIELD_BITS);
        }
        d += 1;
    }
    bits
}

/// 位错位注入变体①：移位错位（shift = 3·dim + 1，落入本域高位）。
pub fn set_cond_shift_bug(cond: u64, dim: u8, op: u8) -> Result<u64, u32> {
    if op == 0 || op > VER_OR {
        return Err(CmCode::OpOutOfRange.code());
    }
    if dim as usize >= DIM_COUNT {
        return Err(CmCode::DimOutOfRange.code());
    }
    let shift = (dim as u32) * FIELD_BITS + 1;
    Ok(cond | ((op as u64) << shift))
}

/// 位错位注入变体②：截位（op & 0b11，算子 ≥4 高位丢失）。
pub fn set_cond_trunc_bug(cond: u64, dim: u8, op: u8) -> Result<u64, u32> {
    if op == 0 || op > VER_OR {
        return Err(CmCode::OpOutOfRange.code());
    }
    if dim as usize >= DIM_COUNT {
        return Err(CmCode::DimOutOfRange.code());
    }
    let shift = (dim as u32) * FIELD_BITS;
    Ok(cond | (((op & 0b11) as u64) << shift))
}

// ── 位面（8 维 3 位域镜像） ─────────────────────────────────────
/// 条件位面镜像（验证/消费段的读侧结构）。
#[derive(Clone, Copy, Debug)]
pub struct CondMaskPlane {
    /// 请求旗（来自 dwTypeMask）。
    pub requested: [bool; DIM_COUNT],
    /// 解码算子（0=域空）。
    pub ops: [u8; DIM_COUNT],
}

impl CondMaskPlane {
    pub fn new(mask: u32) -> CondMaskPlane {
        let mut requested = [false; DIM_COUNT];
        let mut d = 0usize;
        while d < DIM_COUNT {
            requested[d] = mask & (1u32 << d) != 0;
            d += 1;
        }
        CondMaskPlane { requested, ops: [0; DIM_COUNT] }
    }
    /// 从 u64 解码整面。
    pub fn from_cond(mask: u32, cond: u64) -> CondMaskPlane {
        let mut p = CondMaskPlane::new(mask);
        let mut d = 0u8;
        while (d as usize) < DIM_COUNT {
            p.ops[d as usize] = decode_op(cond, d);
            d += 1;
        }
        p
    }
    /// 位面指纹（算子 + 请求旗敏感折叠）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18506;
        let mut i = 0;
        while i < DIM_COUNT {
            acc = acc.wrapping_mul(31).wrapping_add(if self.requested[i] { 1 } else { 0 });
            acc = acc.wrapping_mul(31).wrapping_add(self.ops[i] as u32);
            i += 1;
        }
        acc
    }
}

// ── 账本 ────────────────────────────────────────────────────────
/// 构造账（set 守恒：tries = ok + conflicts + idems + rejects）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmLedger {
    pub set_tries: u32,
    pub set_ok: u32,
    pub conflicts: u32,
    pub idems: u32,
    pub rejects: u32,
    /// 验证段配对失配检出数。
    pub pair_caught: u32,
    /// 消费段域空检出数。
    pub missing_caught: u32,
}

impl CmLedger {
    pub const fn new() -> CmLedger {
        CmLedger {
            set_tries: 0, set_ok: 0, conflicts: 0, idems: 0, rejects: 0,
            pair_caught: 0, missing_caught: 0,
        }
    }
    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.set_tries == self.set_ok + self.conflicts + self.idems + self.rejects
    }
}

// ── 状态机（Empty → Building → Validated → Consumed） ───────────
/// 构造阶段（四态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildPhase {
    /// 未构造。
    Empty,
    /// 构造中。
    Building,
    /// 已验证（可消费）。
    Validated,
    /// 已消费。
    Consumed,
}

/// 掩码构造器：构造 → 配对验证 → 消费 三段。
pub struct MaskBuilder {
    pub phase: BuildPhase,
    pub mask: u32,
    pub cond: u64,
    pub ledger: CmLedger,
}

impl MaskBuilder {
    pub fn new() -> MaskBuilder {
        MaskBuilder {
            phase: BuildPhase::Empty,
            mask: 0,
            cond: 0,
            ledger: CmLedger::new(),
        }
    }

    /// 带掩码开建（空掩码拒；Building 态）。
    pub fn begin(&mut self, mask: u32) -> Result<(), u32> {
        if self.phase != BuildPhase::Empty {
            return Err(CmCode::PhaseInvalid.code());
        }
        if mask == 0 {
            return Err(CmCode::MaskEmpty.code());
        }
        if mask >> DIM_COUNT != 0 {
            return Err(CmCode::DimOutOfRange.code());
        }
        self.mask = mask;
        self.phase = BuildPhase::Building;
        Ok(())
    }

    /// 设一位域（账本入账：ok/idem/conflict/reject 四向）。
    pub fn set(&mut self, dim: u8, op: u8) -> Result<(), u32> {
        self.ledger.set_tries = self.ledger.set_tries.saturating_add(1);
        if self.phase != BuildPhase::Building {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(CmCode::PhaseInvalid.code());
        }
        // 请求旗闸：未请求维不得设。
        if dim as usize >= DIM_COUNT || self.mask & (1u32 << dim) == 0 {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(CmCode::DimOutOfRange.code());
        }
        match set_cond(self.cond, dim, op) {
            Ok(c) => {
                // 幂等判定：域已有同算子。
                let cur = decode_op(self.cond, dim);
                if cur == op {
                    self.ledger.idems = self.ledger.idems.saturating_add(1);
                } else {
                    self.ledger.set_ok = self.ledger.set_ok.saturating_add(1);
                }
                self.cond = c;
                Ok(())
            }
            Err(c) if c == CmCode::CondRewriteConflict.code() => {
                self.ledger.conflicts = self.ledger.conflicts.saturating_add(1);
                Err(c)
            }
            Err(c) => {
                self.ledger.rejects = self.ledger.rejects.saturating_add(1);
                Err(c)
            }
        }
    }

    /// 配对验证：逐请求维 decode == 期望 + 杂散位闸。
    pub fn validate(&mut self, expect_ops: &[u8; DIM_COUNT]) -> Result<(), u32> {
        if self.phase != BuildPhase::Building {
            return Err(CmCode::PhaseInvalid.code());
        }
        // 杂散位闸。
        if self.cond & !expected_bits(self.mask) != 0 {
            return Err(CmCode::StrayBits.code());
        }
        // 逐维配对。
        let mut d = 0u8;
        while (d as usize) < DIM_COUNT {
            if self.mask & (1u32 << d) != 0 {
                let got = decode_op(self.cond, d);
                if got != expect_ops[d as usize] {
                    self.ledger.pair_caught = self.ledger.pair_caught.saturating_add(1);
                    return Err(CmCode::PairMismatch.code());
                }
            }
            d += 1;
        }
        self.phase = BuildPhase::Validated;
        Ok(())
    }

    /// 消费：请求维域必须全非零；消费后终态。
    pub fn consume(&mut self) -> Result<CondMaskPlane, u32> {
        if self.phase != BuildPhase::Validated {
            return Err(CmCode::PhaseInvalid.code());
        }
        let plane = CondMaskPlane::from_cond(self.mask, self.cond);
        let mut d = 0usize;
        while d < DIM_COUNT {
            if plane.requested[d] && plane.ops[d] == 0 {
                self.ledger.missing_caught = self.ledger.missing_caught.saturating_add(1);
                return Err(CmCode::FieldMissing.code());
            }
            d += 1;
        }
        self.phase = BuildPhase::Consumed;
        Ok(plane)
    }
}

// ── 链指纹 ──────────────────────────────────────────────────────
/// 冻结链指纹（掩码+算子全敏感折叠）。
pub fn chains_fingerprint() -> u32 {
    let mut acc: u32 = 0x18506;
    let mut c = 0;
    while c < CHAIN_COUNT {
        let ch = FROZEN_CHAINS[c];
        acc = acc.wrapping_mul(31).wrapping_add(ch.mask);
        let mut d = 0;
        while d < DIM_COUNT {
            acc = acc.wrapping_mul(31).wrapping_add(ch.ops[d] as u32);
            d += 1;
        }
        c += 1;
    }
    acc
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过链数 / 账 / 位面指纹 / 链指纹）。
pub type GoldenOut = (usize, CmLedger, u32, u32);

/// 黄金全链一次：五条冻结链逐链 构造→验证→消费。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut done = 0usize;
    let mut total_ledger = CmLedger::new();
    let mut last_fp = 0u32;
    let mut c = 0;
    while c < CHAIN_COUNT {
        let ch = FROZEN_CHAINS[c];
        let mut b = MaskBuilder::new();
        // 链0 空掩码：begin 拒是预期（空链退化为纯拒绝路径验证）。
        if ch.mask == 0 {
            match b.begin(ch.mask) {
                Err(cv) if cv == CmCode::MaskEmpty.code() => {
                    done += 1;
                    c += 1;
                    continue;
                }
                _ => return Err(CmCode::ChainFingerprint.code()),
            }
        }
        b.begin(ch.mask)?;
        let mut d = 0u8;
        while (d as usize) < DIM_COUNT {
            if ch.mask & (1u32 << d) != 0 {
                b.set(d, ch.ops[d as usize])?;
            }
            d += 1;
        }
        b.validate(&ch.ops)?;
        let plane = b.consume()?;
        if plane.fingerprint() == 0 {
            return Err(CmCode::ChainFingerprint.code());
        }
        last_fp = plane.fingerprint();
        total_ledger.set_tries = total_ledger.set_tries.saturating_add(b.ledger.set_tries);
        total_ledger.set_ok = total_ledger.set_ok.saturating_add(b.ledger.set_ok);
        total_ledger.idems = total_ledger.idems.saturating_add(b.ledger.idems);
        done += 1;
        c += 1;
    }
    if !total_ledger.conserve() {
        return Err(CmCode::LedgerBroken.code());
    }
    Ok((done, total_ledger, last_fp, chains_fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CmCode::NotReproducible.code());
    }
    Ok(())
}

// ── 幂等/冲突探针 ───────────────────────────────────────────────
/// 幂等：同维同算子重设返回同值且记 idem。
pub fn idempotent_probe() -> Result<(), u32> {
    let mut b = MaskBuilder::new();
    b.begin(0x1)?;
    b.set(0, VER_EQUAL)?;
    let c1 = b.cond;
    b.set(0, VER_EQUAL)?;
    if b.cond != c1 {
        return Err(CmCode::CondRewriteConflict.code());
    }
    if b.ledger.idems != 1 {
        return Err(CmCode::LedgerBroken.code());
    }
    Ok(())
}

/// 冲突：同维异算子重设必拒。
pub fn conflict_probe() -> Result<(), u32> {
    let mut b = MaskBuilder::new();
    b.begin(0x1)?;
    b.set(0, VER_EQUAL)?;
    match b.set(0, VER_GREATER_EQUAL) {
        Err(c) if c == CmCode::CondRewriteConflict.code() => Ok(()),
        _ => Err(CmCode::CondRewriteConflict.code()),
    }
}

// ── 注入核（三类位错位 × 10 轮） ────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 移位错位。
    pub shift_caught: u32,
    /// 类2 截位。
    pub trunc_caught: u32,
    /// 类3 漏设（域空）。
    pub missing_caught: u32,
}

/// 单轮三类注入（返回各类是否被抓；漏检 false）。
fn inject_one_round() -> (bool, bool, bool) {
    // 类1 移位错位：链1（mask 0x3，d0=EQ d1=GE）用错位变体设 d0，
    // 验证段杂散位/配对必抓。
    let mut ok1 = false;
    {
        let mut b = MaskBuilder::new();
        let _ = b.begin(0x3);
        match set_cond_shift_bug(0, 0, VER_EQUAL) {
            Ok(c) => {
                b.cond = c;
                // 验证：d0 域应为 EQ（1），错位后 d0 域=0（op 落到
                // shift1），decode=0 ≠ 1 → PairMismatch；
                // 或位溢出本域 → StrayBits。二者任一即算抓。
                match b.validate(&FROZEN_CHAINS[1].ops) {
                    Err(cv) if cv == CmCode::PairMismatch.code() => ok1 = true,
                    Err(cv) if cv == CmCode::StrayBits.code() => ok1 = true,
                    _ => ok1 = false,
                }
            }
            Err(_) => ok1 = false,
        }
    }
    // 类2 截位：链2 d2=LE_EQUAL（5=101b）截成 01b=1 → 配对必错。
    // 推导：截位变体 op&3 只对算子 ≥4 才产生偏差（1..3 截位不变），
    // 故必须打在链2 中算子为 VER_LESS_EQUAL(5) 的 d2 上——
    // 5&3=1 ≠ 5，decode(d2)=1 ≠ 期望 5 → PairMismatch；
    // d0(VER_LESS=4) 与 d5(VER_GREATER_EQUAL=3) 正常设值不动。
    let mut ok2 = false;
    {
        let mut b = MaskBuilder::new();
        let _ = b.begin(0x25);
        let mut d = 0u8;
        while (d as usize) < DIM_COUNT {
            if 0x25 & (1u32 << d) != 0 {
                let op = FROZEN_CHAINS[2].ops[d as usize];
                if d == 2 {
                    // 只对 d2 用截位变体（op=5 截 3 位后得 1）。
                    if let Ok(c) = set_cond_trunc_bug(b.cond, d, op) {
                        b.cond = c;
                    }
                } else {
                    let _ = b.set(d, op);
                }
            }
            d += 1;
        }
        match b.validate(&FROZEN_CHAINS[2].ops) {
            Err(cv) if cv == CmCode::PairMismatch.code() => ok2 = true,
            _ => ok2 = false,
        }
    }
    // 类3 漏设：链1 只设 d0 不设 d1 → 消费段域空必抓。
    let mut ok3 = false;
    {
        let mut b = MaskBuilder::new();
        let _ = b.begin(0x3);
        let _ = b.set(0, VER_EQUAL);
        // 验证会先抓配对（d1 期望 GE 解码 0）——跳过验证直接造
        // Validated 态需要走完整验证；这里用「期望表只写 d0」的
        // 部分表不可行（冻结表不可改），故消费段路径：验证用完整
        // 期望表会 PairMismatch。为专测消费段域空，先让验证用
        // 「与已设一致的部分链」——改为直接构造 cond 只含 d0，
        // mask 0x3，validate 用同款部分期望（本域允许调用方传部分
        // 表会被请求维全查逻辑拒绝吗？——validate 逐请求维查，d1
        // 期望 0 但请求位在 → decode 0 == 0 通过，随后 consume 抓
        // 域空。用部分期望表 [EQ,0,...] 可走到消费段）。
        let mut partial = [0u8; DIM_COUNT];
        partial[0] = VER_EQUAL;
        match b.validate(&partial) {
            Ok(()) => match b.consume() {
                Err(cv) if cv == CmCode::FieldMissing.code() => ok3 = true,
                _ => ok3 = false,
            },
            Err(_) => ok3 = false,
        }
    }
    (ok1, ok2, ok3)
}

/// 三类位错位注入 rounds 轮；每类每轮必抓。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        let (a, b, c) = inject_one_round();
        if !a {
            return Err(CmCode::PairMismatch.code());
        }
        if !b {
            return Err(CmCode::PairMismatch.code());
        }
        if !c {
            return Err(CmCode::FieldMissing.code());
        }
        led.shift_caught = led.shift_caught.saturating_add(1);
        led.trunc_caught = led.trunc_caught.saturating_add(1);
        led.missing_caught = led.missing_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金全链五链过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：幂等重设。
pub fn probe_seq_b() -> Result<(), u32> {
    idempotent_probe()
}

/// 探针 C：同维异算子冲突拒。
pub fn probe_seq_c() -> Result<(), u32> {
    conflict_probe()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < CmCode::ALL.len() {
        let v = CmCode::ALL[i].code();
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

    // 1. 黄金全链：五链过（J1 主体：掩码位与算子配对全过）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _, _)) => *n == CHAIN_COUNT && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-五链配对全过", g_ok, format!("{:?}", g)));

    // 2. 配对往返：encode→decode 逐维还原（全八维链）。
    let rt_ok = {
        let ch = FROZEN_CHAINS[4];
        let mut cond = 0u64;
        let mut d = 0u8;
        let mut ok = true;
        while (d as usize) < DIM_COUNT {
            if ch.mask & (1u32 << d) != 0 {
                match set_cond(cond, d, ch.ops[d as usize]) {
                    Ok(c) => cond = c,
                    Err(_) => ok = false,
                }
            }
            d += 1;
        }
        let mut d2 = 0u8;
        while (d2 as usize) < DIM_COUNT {
            if ch.mask & (1u32 << d2) != 0 && decode_op(cond, d2) != ch.ops[d2 as usize] {
                ok = false;
            }
            d2 += 1;
        }
        ok
    };
    v.push(("J1-配对往返8维", rt_ok, String::from("encode→decode 逐维还原")));

    // 3. 幂等：同维同算子重设同值且记 idem（探针 B）。
    let pb = probe_seq_b().is_ok();
    v.push(("J1-同维幂等重设", pb, String::from("idem=1 值不变")));

    // 4. 冲突：同维异算子拒（探针 C：Ok = 冲突被正确抓到）。
    let pc = probe_seq_c().is_ok();
    v.push(("J1-同维异算子拒", pc, format!("caught={}", pc)));

    // 5. 移位错位注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.shift_caught,
        Err(_) => 0,
    };
    v.push(("J1-移位错位注入10/10", c1 == JUDGE_ROUNDS, format!("caught={}", c1)));

    // 6. 截位注入 10/10。
    let c2 = match led.as_ref() {
        Ok(l) => l.trunc_caught,
        Err(_) => 0,
    };
    v.push(("J1-截位注入10/10", c2 == JUDGE_ROUNDS, format!("caught={}", c2)));

    // 7. 漏设注入 10/10。
    let c3 = match led.as_ref() {
        Ok(l) => l.missing_caught,
        Err(_) => 0,
    };
    v.push(("J1-漏设注入10/10", c3 == JUDGE_ROUNDS, format!("caught={}", c3)));

    // 8. 注入账守恒（3 类 × 10 轮 = 30 抓）。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.shift_caught + l.trunc_caught + l.missing_caught == 3 * JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("3类×10轮")));

    // 9. 算子闭集：0 与 8 在构造核直接拒。
    let op_gates = set_cond(0, 0, 0).is_err() && set_cond(0, 0, 8).is_err();
    v.push(("J1-算子闭集1..=7", op_gates, String::from("0/8 双拒")));

    // 10. 维越界：dim=8 拒。
    let dim_gate = set_cond(0, 8, VER_EQUAL).is_err();
    v.push(("J1-维越界拒", dim_gate, String::from("dim=8")));

    // 11. 空掩码 begin 拒。
    let empty_mask = {
        let mut b = MaskBuilder::new();
        b.begin(0).is_err()
    };
    v.push(("J1-空掩码拒", empty_mask, String::from("mask=0")));

    // 12. 未请求维设值拒（请求旗闸）。
    let unreq = {
        let mut b = MaskBuilder::new();
        let _ = b.begin(0x1);
        b.set(3, VER_EQUAL).is_err()
    };
    v.push(("J1-未请求维设值拒", unreq, String::from("mask=0x1 设 d3")));

    // 13. 阶段闸：Empty 态直接 set 拒；Validated 前消费拒。
    let phase_gates = {
        let mut b1 = MaskBuilder::new();
        let e1 = b1.set(0, VER_EQUAL).is_err();
        let mut b2 = MaskBuilder::new();
        let _ = b2.begin(0x1);
        let e2 = b2.consume().is_err();
        e1 && e2
    };
    v.push(("J1-阶段闸双拒", phase_gates, String::from("Empty set / 未验 consume")));

    // 14. 杂散位闸：cond 混入未请求域残留 → validate 拒。
    let stray = {
        let mut b = MaskBuilder::new();
        let _ = b.begin(0x1);
        let _ = b.set(0, VER_EQUAL);
        // 手工塞入 d1 域残留位（op=7 << 3）。
        b.cond |= 0b111u64 << 3;
        b.validate(&FROZEN_CHAINS[1].ops).is_err()
    };
    v.push(("J1-杂散位闸", stray, String::from("d1 域残留必抓")));

    // 15. 链指纹双跑一致。
    let fp = chains_fingerprint();
    let fp2 = chains_fingerprint();
    v.push(("J2-链指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 16. 双跑一致（复现红线）。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 17. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 18. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = CmCode::ALL.len() == 12;
        let mut i = 0;
        while i < CmCode::ALL.len() {
            if CmCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18506_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18506 VER_CONDITION 掩码位构造");
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
        let set = run_unx18506_checks();
        assert!(set.all_passed(), "UNX-F18506 判据集应全绿");
    }

    /// 位域口径：单域设值/解码往返。
    #[test]
    fn test_field_roundtrip() {
        let mut cond = 0u64;
        let mut d = 0u8;
        while (d as usize) < DIM_COUNT {
            let op = 1 + (d % 7); // 1..=7 逐维轮换
            cond = match set_cond(cond, d, op) {
                Ok(c) => c,
                Err(_) => {
                    assert!(false, "设域不应失败 dim={}", d);
                    return;
                }
            };
            assert_eq!(decode_op(cond, d), op, "解码还原失败 dim={}", d);
            d += 1;
        }
        // 域间互不重叠：逐域孤立读出。
        assert_eq!(decode_op(cond, 0), 1);
        assert_eq!(decode_op(cond, 7), 1);
    }

    /// 黄金链：五链全过。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _, _)) => {
                assert_eq!(n, CHAIN_COUNT);
                assert!(led.conserve());
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 幂等与冲突。
    #[test]
    fn test_idem_conflict() {
        assert!(idempotent_probe().is_ok());
        assert!(conflict_probe().is_err());
        // 幂等核直接验证：cond 不变。
        let c0 = set_cond(0, 2, VER_LESS_EQUAL);
        let c1 = match c0 {
            Ok(c) => c,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        let c2 = match set_cond(c1, 2, VER_LESS_EQUAL) {
            Ok(c) => c,
            Err(_) => {
                assert!(false, "同值重设不应失败");
                return;
            }
        };
        assert_eq!(c1, c2);
    }

    /// 注入三类 10/10。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.shift_caught, 10);
                assert_eq!(l.trunc_caught, 10);
                assert_eq!(l.missing_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 截位语义：算子 5..7 截位后解码必错。
    #[test]
    fn test_trunc_semantics() {
        // op=5（101b）截成 01b=1。
        let c = match set_cond_trunc_bug(0, 0, VER_LESS_EQUAL) {
            Ok(c) => c,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        assert_eq!(decode_op(c, 0), 1);
        assert_ne!(decode_op(c, 0), VER_LESS_EQUAL);
        // op=6（110b）截成 10b=2。
        let c2 = match set_cond_trunc_bug(0, 1, VER_AND) {
            Ok(c) => c,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        assert_eq!(decode_op(c2, 1), VER_GREATER);
    }

    /// 移位错位语义：op 落到域高位，本域解码为 0。
    #[test]
    fn test_shift_bug_semantics() {
        let c = match set_cond_shift_bug(0, 0, VER_EQUAL) {
            Ok(c) => c,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        assert_eq!(decode_op(c, 0), 0, "本域应空");
        assert!(c & (1u64 << 1) != 0, "位应落入本域高位");
    }

    /// 闸位：空掩码/未请求维/阶段闸。
    #[test]
    fn test_gates() {
        let mut b0 = MaskBuilder::new();
        match b0.begin(0) {
            Err(c) => assert_eq!(c, CmCode::MaskEmpty.code()),
            Ok(()) => assert!(false, "空掩码必须拒"),
        }
        let mut b1 = MaskBuilder::new();
        assert!(b1.begin(0x1).is_ok());
        match b1.set(3, VER_EQUAL) {
            Err(c) => assert_eq!(c, CmCode::DimOutOfRange.code()),
            Ok(()) => assert!(false, "未请求维必须拒"),
        }
        let mut b2 = MaskBuilder::new();
        match b2.set(0, VER_EQUAL) {
            Err(c) => assert_eq!(c, CmCode::PhaseInvalid.code()),
            Ok(()) => assert!(false, "Empty 态 set 必须拒"),
        }
    }

    /// 杂散位闸。
    #[test]
    fn test_stray_bits() {
        let mut b = MaskBuilder::new();
        assert!(b.begin(0x1).is_ok());
        assert!(b.set(0, VER_EQUAL).is_ok());
        b.cond |= 0b111u64 << 3;
        match b.validate(&FROZEN_CHAINS[1].ops) {
            Err(c) => assert_eq!(c, CmCode::StrayBits.code()),
            Ok(()) => assert!(false, "杂散位必须抓"),
        }
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
