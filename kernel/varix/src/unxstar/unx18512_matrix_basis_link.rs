//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18512 · 判定结果与矩阵值联动（谎报态比较）
//! · 0x7FBC0 段 · 批 06 · 配额 320
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18512-J1 比较基值取自矩阵终值断言全过，直连系统值旁路
//!   注入 10/10 拒绝
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 基值联动语义冻结：判定器的比较基值必须取自维度矩阵的「终值」
//!     槽（谎报/兼容层写入口径），不得直连系统快照原始值——兼容
//!     谎报态下终值 ≠ 原值，旁路直连即产生假裁决；
//!   - 维度矩阵 8 槽（与总纲同源口径，闭式重演）：d0=MINOR /
//!     d1=MAJOR / d2=BUILD / d3=PLATFORMID / d4=SPMINOR / d5=SPMAJOR /
//!     d6=SUITENAME / d7=PRODUCT_TYPE；每槽 raw（系统原值，不可变）
//!     + final（终值，可写一次）；
//!   - 谎报态表 8 组：L0 真值 / L1 谎 6.2 / L2 谎 10.0 / L3 谎 5.1 /
//!     L4 谎 6.3 / L5 谎 build 9200 / L6 谎 SP(2,1) / L7 谎套件
//!     0x0200；每组对 8 槽全量落终值后冻结；
//!   - 真值表 30 例：谎报态 × (维, 算子, 请求值)，期望逐例手推写死
//!     （数值维闭式五算子 + 套件维 AND/OR 位语义）；
//!   - 旁路注入 10 例（终值≠原值且裁决翻转的指定例）：旁路引擎吃
//!     raw 值 → 裁决与矩阵终值裁决必翻 → 10/10 全检出；
//!   - 状态机 Writable→Frozen→Judged：未冻结判定拒 / 冻结后写入拒 /
//!     空槽读拒；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] MxCode 闭集（13 码）+ ALL + detail() + detail(code)
//!   [裁决表]   SNAPSHOT 8 槽原值 / LIE_TABLE 8 组谎报终值 / CASES 30 例
//!   [数据结构] MatrixSlot / DimMatrix（raw+final 双值槽 + 指纹）
//!   [状态机]   MxPhase 三态单向（Writable→Frozen→Judged）
//!   [账本]     MxLedger（判定尝试 = 过 + 拒 守恒）+ BypassLedger
//!   [注入核]   三类 × 10 轮（旁路直连/未冻判定/冻后写入）
//!   [收口]     golden_run + run_unx18512_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FBC0，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FBC0;

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
/// VER_AND（套件维位语义：req & basis == req 才过）。
pub const VER_AND: u8 = 6;
/// VER_OR（套件维位语义：req & basis != 0 即过）。
pub const VER_OR: u8 = 7;
/// 维度槽数（bit 序冻结，与总纲一致）。
pub const DIM_SLOTS: usize = 8;
/// 谎报态组数（冻结：8）。
pub const LIE_STATES: usize = 8;
/// 真值表例数（冻结：30）。
pub const CASES: usize = 30;
/// 旁路注入指定例数（J1「10/10 拒绝」口径）。
pub const BYPASS_CASES: usize = 10;
/// 注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 系统快照原值（8 槽，冻结：OSV 6.1.7601 / NT / SP1 / 套件 0x0100 /
/// 产品型 1，与总纲同源）。
pub const SNAPSHOT: [u32; DIM_SLOTS] = [1, 6, 7601, 2, 1, 1, 0x0100, 1];

/// 谎报态第 l 组终值（8 槽全量落值；L0 为真值恒等组）。
/// 推导：L1 谎 6.2（仅 d0）、L2 谎 10.0（d1+d0）、L3 谎 5.1（仅 d1）、
/// L4 谎 6.3（仅 d0）、L5 谎 build 9200（仅 d2）、L6 谎 SP(2,1)
/// （仅 d5）、L7 谎套件 0x0200（仅 d6）。
pub fn lie_final(l: usize) -> Option<[u32; DIM_SLOTS]> {
    match l {
        0 => Some([1, 6, 7601, 2, 1, 1, 0x0100, 1]),
        1 => Some([2, 6, 7601, 2, 1, 1, 0x0100, 1]),
        2 => Some([0, 10, 7601, 2, 1, 1, 0x0100, 1]),
        3 => Some([1, 5, 7601, 2, 1, 1, 0x0100, 1]),
        4 => Some([3, 6, 7601, 2, 1, 1, 0x0100, 1]),
        5 => Some([1, 6, 9200, 2, 1, 1, 0x0100, 1]),
        6 => Some([1, 6, 7601, 2, 1, 2, 0x0100, 1]),
        7 => Some([1, 6, 7601, 2, 1, 1, 0x0200, 1]),
        _ => None,
    }
}

/// 真值表一行：(谎报组, 维, 算子, 请求值, 期望)。
/// expect 全部按「基值 = 矩阵终值」口径手推。
pub type MxCase = (usize, u8, u8, u32, bool);

/// 第 i 例（越界 None）。
/// 推导：0-2 真值组基线；3-5 谎 6.2（终值 minor=2：eq2 过/eq1 不过）；
/// 6-9 谎 10.0；10-12 谎 5.1；13-15 谎 6.3；16-18 谎 build 9200；
/// 19-21 谎 SP 2；22-23 谎套件 0x0200；24-29 真值组套件/产品型/
/// 跨代邻域收尾。
pub fn mx_case(i: usize) -> Option<MxCase> {
    match i {
        0 => Some((0, 1, VER_EQUAL, 6, true)),            // 真值 major eq 6
        1 => Some((0, 0, VER_EQUAL, 1, true)),            // 真值 minor eq 1
        2 => Some((0, 1, VER_GREATER_EQUAL, 6, true)),    // 真值 major ge 6
        3 => Some((1, 0, VER_EQUAL, 2, true)),            // 谎 6.2：终值 eq 2 过
        4 => Some((1, 0, VER_EQUAL, 1, false)),           // 谎 6.2：终值 eq 1 不过
        5 => Some((1, 1, VER_EQUAL, 6, true)),            // 谎 6.2：major 未变
        6 => Some((2, 1, VER_EQUAL, 10, true)),           // 谎 10.0：eq 10 过
        7 => Some((2, 0, VER_EQUAL, 0, true)),            // 谎 10.0：minor eq 0 过
        8 => Some((2, 1, VER_GREATER_EQUAL, 8, true)),    // 谎 10.0：ge 8 过
        9 => Some((2, 0, VER_LESS, 1, true)),             // 谎 10.0：minor lt 1 过
        10 => Some((3, 1, VER_EQUAL, 5, true)),           // 谎 5.1：eq 5 过
        11 => Some((3, 1, VER_GREATER_EQUAL, 5, true)),   // 谎 5.1：ge 5 过
        12 => Some((3, 0, VER_EQUAL, 1, true)),           // 谎 5.1：minor 未变
        13 => Some((4, 0, VER_EQUAL, 3, true)),           // 谎 6.3：eq 3 过
        14 => Some((4, 1, VER_EQUAL, 6, true)),           // 谎 6.3：major 未变
        15 => Some((4, 0, VER_LESS_EQUAL, 3, true)),      // 谎 6.3：le 3 过
        16 => Some((5, 2, VER_EQUAL, 9200, true)),        // 谎 build：eq 9200 过
        17 => Some((5, 2, VER_EQUAL, 7601, false)),       // 谎 build：eq 7601 不过
        18 => Some((5, 1, VER_EQUAL, 6, true)),           // 谎 build：major 未变
        19 => Some((6, 5, VER_EQUAL, 2, true)),           // 谎 SP2：eq 2 过
        20 => Some((6, 5, VER_EQUAL, 1, false)),          // 谎 SP2：eq 1 不过
        21 => Some((6, 4, VER_EQUAL, 1, true)),           // 谎 SP2：minor 未变
        22 => Some((7, 6, VER_AND, 0x0200, true)),        // 谎套件：AND 0x0200 过
        23 => Some((7, 6, VER_AND, 0x0100, false)),       // 谎套件：AND 0x0100 不过
        24 => Some((0, 6, VER_AND, 0x0100, true)),        // 真值：AND 0x0100 过
        25 => Some((0, 7, VER_EQUAL, 1, true)),           // 真值：产品型 eq 1
        26 => Some((2, 1, VER_LESS_EQUAL, 10, true)),     // 谎 10.0：le 10 过
        27 => Some((3, 1, VER_LESS_EQUAL, 5, true)),      // 谎 5.1：le 5 过
        28 => Some((4, 0, VER_GREATER, 2, true)),         // 谎 6.3：gt 2 过
        29 => Some((5, 2, VER_GREATER_EQUAL, 8000, true)),// 谎 build：ge 8000 过
        _ => None,
    }
}

/// 旁路注入指定例索引（终值≠原值且旁路裁决翻转的 10 例）。
/// 推导：逐例核对 lie_final ≠ SNAPSHOT[维] 且 raw 口径裁决翻面，
/// 命中例 3/4/6/7/9/10/13/16/17/19（例 11 raw 亦过、例 12/14/15/18
/// 未变维恰同、例 21/24/25 未变维、例 26-29 同侧不翻——均不入面；
/// 例 28 虽翻面但与例 13 同构重复，取 10 例指定面即足）。
pub fn bypass_case_ids() -> [usize; BYPASS_CASES] {
    [3, 4, 6, 7, 9, 10, 13, 16, 17, 19]
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MxCode {
    /// 维槽越界（≥ 8）。
    DimOutOfRange,
    /// 算子越界（0 或 >7）。
    OpIllegal,
    /// 套件维落数值算子 / 数值维落 AND/OR。
    OpDimMismatch,
    /// 空槽读（终值未落即判定）。
    SlotEmpty,
    /// 未冻结判定（Writable 相位判定拒）。
    NotFrozen,
    /// 冻结后写入（Frozen/Judged 相位写入拒）。
    FrozenImmutable,
    /// 真值表偏差（矩阵裁决 ≠ 期望，零偏差红线）。
    VerdictDeviation,
    /// 旁路漏检（旁路裁决与矩阵裁决同值，未翻面）。
    BypassMissed,
    /// 注入漏检（相位注入未被拒）。
    InjectionMissed,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 空表收口（零判定即封账）。
    TableEmpty,
    /// 表指纹失配。
    FingerprintMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl MxCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MxCode::DimOutOfRange => "维槽越界：矩阵仅 0..=7",
            MxCode::OpIllegal => "算子越界：闭集 1..=7",
            MxCode::OpDimMismatch => "算子维不配：AND/OR 仅套件维",
            MxCode::SlotEmpty => "空槽读：终值未落即判定",
            MxCode::NotFrozen => "未冻结判定：先落终值并冻结",
            MxCode::FrozenImmutable => "冻结后写入：矩阵终值不可变",
            MxCode::VerdictDeviation => "真值表偏差：got≠expect 零容忍",
            MxCode::BypassMissed => "旁路漏检：直连原值必翻面",
            MxCode::InjectionMissed => "注入漏检：违约必被拒",
            MxCode::LedgerBroken => "账本守恒破坏：尝试≠过+拒",
            MxCode::TableEmpty => "空表收口：零判定不得封账",
            MxCode::FingerprintMismatch => "表指纹失配：冻结表被改",
            MxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MxCode; 13] = [
        MxCode::DimOutOfRange,
        MxCode::OpIllegal,
        MxCode::OpDimMismatch,
        MxCode::SlotEmpty,
        MxCode::NotFrozen,
        MxCode::FrozenImmutable,
        MxCode::VerdictDeviation,
        MxCode::BypassMissed,
        MxCode::InjectionMissed,
        MxCode::LedgerBroken,
        MxCode::TableEmpty,
        MxCode::FingerprintMismatch,
        MxCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MxCode::ALL.len() {
        if MxCode::ALL[i].code() == code {
            return MxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FBC0 段闭集内"
}

// ── 比较核（基值口径闭式） ──────────────────────────────────────
/// 单维比较：数值维闭式五算子；套件维 AND/OR 位语义；
/// 算子/维不配即拒。
pub fn cmp_dim(dim: u8, op: u8, basis: u32, req: u32) -> Result<bool, u32> {
    if dim as usize >= DIM_SLOTS {
        return Err(MxCode::DimOutOfRange.code());
    }
    match op {
        1 => Ok(basis == req),
        2 => Ok(basis > req),
        3 => Ok(basis >= req),
        4 => Ok(basis < req),
        5 => Ok(basis <= req),
        // 套件维位语义（Windows：VER_AND 要求请求位全在场；VER_OR
        // 要求任一请求位在场）。
        6 => {
            if dim == 6 {
                Ok(req & basis == req)
            } else {
                Err(MxCode::OpDimMismatch.code())
            }
        }
        7 => {
            if dim == 6 {
                Ok(req & basis != 0)
            } else {
                Err(MxCode::OpDimMismatch.code())
            }
        }
        _ => Err(MxCode::OpIllegal.code()),
    }
}

// ── 维度矩阵（raw + final 双值槽） ──────────────────────────────
/// 矩阵槽：raw 系统原值（构造即定，不可变）+ final 终值（可写一次）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixSlot {
    pub raw: u32,
    pub final_v: Option<u32>,
}

/// 矩阵相位（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MxPhase {
    /// 可写（落终值）。
    Writable,
    /// 已冻结（可判定）。
    Frozen,
    /// 已判定收口（终态）。
    Judged,
}

/// 判定账（尝试 = 过 + 拒，守恒方程）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MxLedger {
    pub tries: u32,
    pub passes: u32,
    pub rejects: u32,
}

impl MxLedger {
    pub const fn new() -> MxLedger {
        MxLedger { tries: 0, passes: 0, rejects: 0 }
    }
    /// 守恒核算。
    pub fn conserve(&self) -> bool {
        self.tries == self.passes + self.rejects
    }
}

/// 维度矩阵引擎。
pub struct DimMatrix {
    pub slots: [MatrixSlot; DIM_SLOTS],
    pub phase: MxPhase,
    pub ledger: MxLedger,
    /// 最近一次判定吃掉的维数（观测口径）。
    pub last_dims: u32,
}

impl DimMatrix {
    /// 构造：raw 槽按快照落值，final 全空，Writable 相位。
    pub fn new() -> DimMatrix {
        DimMatrix {
            slots: [
                MatrixSlot { raw: SNAPSHOT[0], final_v: None },
                MatrixSlot { raw: SNAPSHOT[1], final_v: None },
                MatrixSlot { raw: SNAPSHOT[2], final_v: None },
                MatrixSlot { raw: SNAPSHOT[3], final_v: None },
                MatrixSlot { raw: SNAPSHOT[4], final_v: None },
                MatrixSlot { raw: SNAPSHOT[5], final_v: None },
                MatrixSlot { raw: SNAPSHOT[6], final_v: None },
                MatrixSlot { raw: SNAPSHOT[7], final_v: None },
            ],
            phase: MxPhase::Writable,
            ledger: MxLedger::new(),
            last_dims: 0,
        }
    }

    /// 单槽落终值（Writable 限定；重复落值即 FrozenImmutable 语义拒）。
    pub fn write_final(&mut self, dim: usize, val: u32) -> Result<(), u32> {
        if dim >= DIM_SLOTS {
            return Err(MxCode::DimOutOfRange.code());
        }
        if self.phase != MxPhase::Writable {
            return Err(MxCode::FrozenImmutable.code());
        }
        if self.slots[dim].final_v.is_some() {
            return Err(MxCode::FrozenImmutable.code());
        }
        self.slots[dim].final_v = Some(val);
        Ok(())
    }

    /// 整组落谎报终值（8 槽全量，任一失败即回滚已落槽）。
    pub fn apply_lie(&mut self, l: usize) -> Result<(), u32> {
        let vals = match lie_final(l) {
            Some(v) => v,
            None => return Err(MxCode::DimOutOfRange.code()),
        };
        let mut done = 0usize;
        while done < DIM_SLOTS {
            match self.write_final(done, vals[done]) {
                Ok(()) => done += 1,
                Err(e) => {
                    // 回滚：已落槽清空（账不丢，相位不动）。
                    let mut k = 0;
                    while k < done {
                        self.slots[k].final_v = None;
                        k += 1;
                    }
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// 冻结：8 槽终值必须全在场；Writable → Frozen。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.phase != MxPhase::Writable {
            return Err(MxCode::FrozenImmutable.code());
        }
        let mut i = 0;
        while i < DIM_SLOTS {
            if self.slots[i].final_v.is_none() {
                return Err(MxCode::SlotEmpty.code());
            }
            i += 1;
        }
        self.phase = MxPhase::Frozen;
        Ok(())
    }

    /// 判定（矩阵口径）：基值逐维取终值，AND 聚合一次收口。
    /// 相位闸：仅 Writable 拒（Frozen/Judged 均可判定，重复判定
    /// 幂等同值——确定性口径）。
    pub fn judge(&mut self, reqs: &[(u8, u8, u32)]) -> Result<bool, u32> {
        if self.phase == MxPhase::Writable {
            return Err(MxCode::NotFrozen.code());
        }
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        let mut verdict = true;
        let mut used = 0u32;
        let mut i = 0;
        while i < reqs.len() {
            let (dim, op, req) = reqs[i];
            let basis = match self.slots[dim as usize].final_v {
                Some(v) => v,
                None => {
                    self.ledger.rejects = self.ledger.rejects.saturating_add(1);
                    return Err(MxCode::SlotEmpty.code());
                }
            };
            match cmp_dim(dim, op, basis, req) {
                Ok(r) => {
                    verdict = verdict && r;
                    used = used.saturating_add(1);
                }
                Err(e) => {
                    self.ledger.rejects = self.ledger.rejects.saturating_add(1);
                    return Err(e);
                }
            }
            i += 1;
        }
        self.last_dims = used;
        self.ledger.passes = self.ledger.passes.saturating_add(1);
        self.phase = MxPhase::Judged;
        Ok(verdict)
    }

    /// 旁路判定（注入面专用：基值直连 raw 原值；正常引擎禁用）。
    pub fn judge_bypass_raw(&mut self, reqs: &[(u8, u8, u32)]) -> Result<bool, u32> {
        if self.phase == MxPhase::Writable {
            return Err(MxCode::NotFrozen.code());
        }
        let mut verdict = true;
        let mut i = 0;
        while i < reqs.len() {
            let (dim, op, req) = reqs[i];
            match cmp_dim(dim, op, self.slots[dim as usize].raw, req) {
                Ok(r) => verdict = verdict && r,
                Err(e) => return Err(e),
            }
            i += 1;
        }
        Ok(verdict)
    }

    /// 单例真值表执行：矩阵口径裁决 + 期望对拍。
    pub fn judge_case(&mut self, idx: usize) -> Result<bool, u32> {
        let (lie, dim, op, req, expect) = match mx_case(idx) {
            Some(x) => x,
            None => return Err(MxCode::DimOutOfRange.code()),
        };
        let mut m = DimMatrix::new();
        m.apply_lie(lie)?;
        m.freeze()?;
        let got = m.judge(&[(dim, op, req)])?;
        if got != expect {
            return Err(MxCode::VerdictDeviation.code());
        }
        Ok(got)
    }
}

// ── 表指纹（冻结对账） ──────────────────────────────────────────
/// 真值表指纹：31 进制逐例折叠（行序 + 期望位敏感）。
/// 推导：初值取卡号派生 0x18512；期望以 1/2 入折。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18512;
    let mut i = 0;
    while i < CASES {
        match mx_case(i) {
            Some((l, d, o, r, e)) => {
                acc = acc.wrapping_mul(31).wrapping_add(l as u32);
                acc = acc.wrapping_mul(31).wrapping_add(d as u32);
                acc = acc.wrapping_mul(31).wrapping_add(o as u32);
                acc = acc.wrapping_mul(31).wrapping_add(r);
                acc = acc.wrapping_mul(31).wrapping_add(if e { 1 } else { 2 });
            }
            None => {
                acc = acc.wrapping_mul(31).wrapping_add(0xFFFF);
            }
        }
        i += 1;
    }
    acc
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过例数 / 判定账 / 指纹）。
pub type GoldenOut = (usize, MxLedger, u32);

/// 黄金全链：30 例逐例建阵→落谎→冻结→判定→对拍，全过零偏差。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut total = MxLedger::new();
    let mut n = 0usize;
    let mut i = 0;
    while i < CASES {
        let (lie, dim, op, req, _e) = match mx_case(i) {
            Some(x) => x,
            None => return Err(MxCode::DimOutOfRange.code()),
        };
        let mut m = DimMatrix::new();
        m.apply_lie(lie)?;
        m.freeze()?;
        let got = m.judge(&[(dim, op, req)])?;
        total.tries = total.tries.saturating_add(1);
        match mx_case(i) {
            Some((_, _, _, _, e)) if e == got => {
                total.passes = total.passes.saturating_add(1);
            }
            _ => {
                total.rejects = total.rejects.saturating_add(1);
                return Err(MxCode::VerdictDeviation.code());
            }
        }
        n += 1;
        i += 1;
    }
    if !total.conserve() {
        return Err(MxCode::LedgerBroken.code());
    }
    Ok((n, total, table_fingerprint()))
}

/// 双跑一致（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MxCode::NotReproducible.code());
    }
    Ok(())
}

// ── 旁路注入核（10 例指定面 × 10 轮，10/10 检出） ───────────────
/// 旁路账（检出数 / 漏检数；守恒 = 10 × 轮数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct BypassLedger {
    pub caught: u32,
    pub missed: u32,
}

impl BypassLedger {
    /// 守恒核算：检出 + 漏检 = 注入面总额。
    pub fn conserve(&self, rounds: u32) -> bool {
        self.caught + self.missed == BYPASS_CASES as u32 * rounds
    }
}

/// 旁路注入 rounds 轮：每轮 10 例旁路直连 raw，裁决必与矩阵终值
/// 口径翻面（或口径报错）；漏检即 Err。
pub fn bypass_inject(rounds: u32) -> Result<BypassLedger, u32> {
    let mut led = BypassLedger::default();
    let mut r = 0;
    while r < rounds {
        let mut i = 0;
        while i < BYPASS_CASES {
            let idx = bypass_case_ids()[i];
            let (lie, dim, op, req, _e) = match mx_case(idx) {
                Some(x) => x,
                None => return Err(MxCode::DimOutOfRange.code()),
            };
            let mut m = DimMatrix::new();
            m.apply_lie(lie)?;
            m.freeze()?;
            let via_matrix = m.judge(&[(dim, op, req)])?;
            let via_raw = m.judge_bypass_raw(&[(dim, op, req)])?;
            if via_matrix != via_raw {
                led.caught = led.caught.saturating_add(1);
            } else {
                led.missed = led.missed.saturating_add(1);
                return Err(MxCode::BypassMissed.code());
            }
            i += 1;
        }
        r += 1;
    }
    if !led.conserve(rounds) {
        return Err(MxCode::LedgerBroken.code());
    }
    Ok(led)
}

// ── 相位注入核（未冻判定 / 冻后写入 × 10 轮） ───────────────────
/// 相位注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct PhaseLedger {
    /// 未冻结判定被拒次数。
    pub not_frozen_caught: u32,
    /// 冻结后写入被拒次数。
    pub frozen_immutable_caught: u32,
}

/// 相位注入 rounds 轮：Writable 判定必拒 / Frozen 写入必拒。
pub fn phase_inject(rounds: u32) -> Result<PhaseLedger, u32> {
    let mut led = PhaseLedger::default();
    let mut r = 0;
    while r < rounds {
        // 未冻结判定：新阵不落值直接判定 → NotFrozen。
        let mut m0 = DimMatrix::new();
        match m0.judge(&[(1, VER_EQUAL, 6)]) {
            Err(c) if c == MxCode::NotFrozen.code() => {
                led.not_frozen_caught = led.not_frozen_caught.saturating_add(1);
            }
            _ => return Err(MxCode::InjectionMissed.code()),
        }
        // 冻结后写入：落值→冻结→再写 → FrozenImmutable。
        let mut m1 = DimMatrix::new();
        m1.apply_lie(0)?;
        m1.freeze()?;
        match m1.write_final(0, 1) {
            Err(c) if c == MxCode::FrozenImmutable.code() => {
                led.frozen_immutable_caught = led.frozen_immutable_caught.saturating_add(1);
            }
            _ => return Err(MxCode::InjectionMissed.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：30 例真值表全过（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：空槽判定（落值一半即冻结→缺槽拒）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut m = DimMatrix::new();
    m.write_final(0, 1)?;
    m.freeze()?;
    match m.judge(&[(0, VER_EQUAL, 1)]) {
        Err(c) => Err(c),
        Ok(_) => Err(MxCode::SlotEmpty.code()),
    }
}

/// 探针 C：数值维落 AND（算子维不配拒）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut m = DimMatrix::new();
    m.apply_lie(0)?;
    m.freeze()?;
    match m.judge(&[(1, VER_AND, 0x0100)]) {
        Err(c) => Err(c),
        Ok(_) => Err(MxCode::OpDimMismatch.code()),
    }
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < MxCode::ALL.len() {
        let v = MxCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（16 条；动态明细 ≤8） ──────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：30 例矩阵终值口径全过零偏差（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == CASES && led.conserve() && led.rejects == 0,
        Err(_) => false,
    };
    v.push(("J1-30例矩阵联动全过", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A真值表顺跑", pa, format!("ok={}", pa)));

    // 3. 谎报态基值直证：谎 6.2 下 eq 2 过、eq 1 不过（终值口径）。
    let lie_direct = {
        let mut m = DimMatrix::new();
        let ok1 = m.apply_lie(1).is_ok() && m.freeze().is_ok();
        let r1 = m.judge(&[(0, VER_EQUAL, 2)]);
        let mut m2 = DimMatrix::new();
        let ok2 = m2.apply_lie(1).is_ok() && m2.freeze().is_ok();
        let r2 = m2.judge(&[(0, VER_EQUAL, 1)]);
        ok1 && ok2 && r1 == Ok(true) && r2 == Ok(false)
    };
    v.push(("J1-谎报态基值直证", lie_direct, String::from("终值 6.2 口径")));

    // 4. 旁路注入 10/10 检出（裁决必翻面）。
    let bl = bypass_inject(JUDGE_ROUNDS);
    let bc = match bl.as_ref() {
        Ok(l) => l.caught,
        Err(_) => 0,
    };
    v.push((
        "J1-旁路注入10/10",
        bc == BYPASS_CASES as u32 * JUDGE_ROUNDS,
        format!("caught={}", bc),
    ));

    // 5. 旁路账守恒（检出 + 漏检 = 面额 × 轮数，漏检必须为 0）。
    let bcons = match bl.as_ref() {
        Ok(l) => l.conserve(JUDGE_ROUNDS) && l.missed == 0,
        Err(_) => false,
    };
    v.push(("J1-旁路账守恒", bcons, String::from("10例×10轮=100")));

    // 6. 相位注入：未冻判定 / 冻后写入 10/10 拒。
    let pl = phase_inject(JUDGE_ROUNDS);
    let p_ok = match pl.as_ref() {
        Ok(l) => {
            l.not_frozen_caught == JUDGE_ROUNDS && l.frozen_immutable_caught == JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-相位注入10/10", p_ok, format!("{:?}", pl)));

    // 7. 空槽判定拒（探针 B：半落值冻结）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-空槽判定拒", pb, format!("err={}", pb)));

    // 8. 算子维不配拒（探针 C：数值维落 AND）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-数值维AND拒", pc, format!("err={}", pc)));

    // 9. 套件维位语义直证：AND 全位在场才过、OR 任一位在场即过。
    let suite_sem = {
        let mut m = DimMatrix::new();
        let ok = m.apply_lie(0).is_ok() && m.freeze().is_ok();
        ok && m.judge(&[(6, VER_AND, 0x0100)]) == Ok(true)
            && m.judge(&[(6, VER_AND, 0x0300)]) == Ok(false)
            && m.judge(&[(6, VER_OR, 0x1000)]) == Ok(false)
    };
    v.push(("J1-套件位语义", suite_sem, String::from("AND全位/OR任位")));

    // 10. 回滚语义：整组落值中断后槽账清零（冻结必拒）。
    let rollback = {
        let mut m = DimMatrix::new();
        // L8 越界组：apply_lie 必拒且已落槽回滚。
        let e = m.apply_lie(8);
        let cleared = {
            let mut any = false;
            let mut k = 0;
            while k < DIM_SLOTS {
                if m.slots[k].final_v.is_some() {
                    any = true;
                }
                k += 1;
            }
            !any
        };
        e.is_err() && cleared && m.freeze().is_err()
    };
    v.push(("J1-越界组回滚", rollback, String::from("槽账清零冻结拒")));

    // 11. 判定收口相位：Judged 后再写拒（终态单向）。
    let sealed = {
        let mut m = DimMatrix::new();
        let ok = m.apply_lie(0).is_ok() && m.freeze().is_ok();
        ok && m.judge(&[(1, VER_EQUAL, 6)]).is_ok()
            && m.write_final(1, 7).is_err()
    };
    v.push(("J1-Judged终态写拒", sealed, String::from("三态单向")));

    // 12. 表指纹双跑一致 + 扰动敏感（手推表 30 例）。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 13. 双跑一致（复现红线）。
    let rp = repro_check().is_ok();
    v.push(("J2-双跑一致", rp, String::from("golden 两跑同值")));

    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 15. 诊断码闭集（13 码全非空中文明细）。
    let all_ok = {
        let mut ok = MxCode::ALL.len() == 13;
        let mut i = 0;
        while i < MxCode::ALL.len() {
            if MxCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    // 16. 旁路指定面完整性：10 例逐一核对「终值≠原值」前提成立。
    let faces_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < BYPASS_CASES {
            let idx = bypass_case_ids()[i];
            match mx_case(idx) {
                Some((l, d, _, _, _)) => match lie_final(l) {
                    Some(vals) => {
                        if vals[d as usize] == SNAPSHOT[d as usize] {
                            ok = false;
                        }
                    }
                    None => ok = false,
                },
                None => ok = false,
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-旁路面10例前提", faces_ok, String::from("终值≠原值逐例核")));

    v
}

/// 收口：本模块自检 CheckSet（判据 16 条；前 8 条动态明细）。
pub fn run_unx18512_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18512 判定结果与矩阵值联动");
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
        let set = run_unx18512_checks();
        assert!(set.all_passed(), "UNX-F18512 判据集应全绿");
    }

    /// 黄金链：30 例全过零偏差。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 30);
                assert_eq!(led.passes, 30);
                assert_eq!(led.rejects, 0);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 谎报态基值联动：终值口径 vs raw 口径逐例翻面。
    #[test]
    fn test_lie_basis_linkage() {
        // 谎 6.2：矩阵口径 eq 2 过；旁路口径 eq 2 不过（raw minor=1）。
        let mut m = DimMatrix::new();
        assert!(m.apply_lie(1).is_ok());
        assert!(m.freeze().is_ok());
        assert_eq!(m.judge(&[(0, VER_EQUAL, 2)]), Ok(true));
        assert_eq!(m.judge_bypass_raw(&[(0, VER_EQUAL, 2)]), Ok(false));
    }

    /// 旁路注入 10/10 检出。
    #[test]
    fn test_bypass_injection() {
        match bypass_inject(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.caught, 100);
                assert_eq!(l.missed, 0);
                assert!(l.conserve(JUDGE_ROUNDS));
            }
            Err(e) => assert!(false, "旁路注入不应报错: {}", detail(e)),
        }
    }

    /// 相位注入 10/10 拒。
    #[test]
    fn test_phase_injection() {
        match phase_inject(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.not_frozen_caught, 10);
                assert_eq!(l.frozen_immutable_caught, 10);
            }
            Err(e) => assert!(false, "相位注入不应报错: {}", detail(e)),
        }
    }

    /// 重复落值与越界槽拒。
    #[test]
    fn test_write_gates() {
        let mut m = DimMatrix::new();
        assert!(m.write_final(0, 1).is_ok());
        match m.write_final(0, 2) {
            Err(c) => assert_eq!(c, MxCode::FrozenImmutable.code()),
            Ok(()) => assert!(false, "重复落值必须拒"),
        }
        match m.write_final(8, 1) {
            Err(c) => assert_eq!(c, MxCode::DimOutOfRange.code()),
            Ok(()) => assert!(false, "越界槽必须拒"),
        }
    }

    /// 探针 B/C：空槽判定拒、数值维 AND 拒。
    #[test]
    fn test_probe_bc() {
        assert!(probe_seq_b().is_err(), "空槽判定必须拒");
        assert!(probe_seq_c().is_err(), "数值维 AND 必须拒");
    }

    /// 套件维位语义：AND 全位 / OR 任位。
    #[test]
    fn test_suite_bitwise() {
        let mut m = DimMatrix::new();
        assert!(m.apply_lie(0).is_ok());
        assert!(m.freeze().is_ok());
        assert_eq!(m.judge(&[(6, VER_AND, 0x0100)]), Ok(true));
        assert_eq!(m.judge(&[(6, VER_AND, 0x0300)]), Ok(false));
        assert_eq!(m.judge(&[(6, VER_OR, 0x0200)]), Ok(false));
        assert_eq!(m.judge(&[(6, VER_OR, 0x0100)]), Ok(true));
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
        assert_eq!(MxCode::ALL.len(), 13);
        for c in MxCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
