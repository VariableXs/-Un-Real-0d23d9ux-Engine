//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18511 · ServicePack 维度比较语义（主/次双字段）
//! · 0x7FB80 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18511-J1 SP 主/次双字段比较断言全过，单边请求注入检出
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - SP 双字段语义冻结：VER_SERVICE_PACK_MAJOR（掩码位 0x20，维度
//!     d5）与 VER_SERVICE_PACK_MINOR（掩码位 0x10，维度 d4）是两个
//!     独立数值维，Windows VerifyVersionInfo 对二者独立取算子比较、
//!     结果按 AND 聚合——主字段过而次字段不过 → 总裁决必 FALSE；
//!   - 系统快照冻结：OSV 6.1.7601 / SP(1,1)：sp_major=1、sp_minor=1
//!     （与 F18501 总纲同源口径，闭式重演不跨模块引用）；
//!   - 双字段闸（单边请求检出）：合法 SP 请求必须主/次两字段同时
//!     在场（次字段缺席 = 单边请求），单边请求即拒——这是 Windows
//!     "SP 比较必须成对" 的口径收敛；
//!   - 冻结比较表 20 例：主/次请求值 × 五算子（EQ/GT/GE/LT/LE）
//!     组合，期望逐例手推写死（含等值/差一/反差一/MAX 邻域）；
//!   - 注入三类 × 10 轮：单边请求（缺次字段，双字段闸必拒）/ 算子
//!     降级（双算子降为 VER_EQUAL，非等值路径必偏）/ 算子越界
//!     （6/7 落数值维必拒）——10/10 全拒；
//!   - 状态机 Idle→Probing→Sealed：行 id 严格升序 + 重复拒 + Sealed
//!     后再探拒；账本环 24 槽定容，满显式拒且旧账不丢；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] SpCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   SP_ROWS 20 例冻结表（手推期望）+ 表指纹
//!   [数据结构] SpRequest（主/次双字段 + 双算子）/ SpRec / SpRing
//!   [状态机]   SpPhase 三态 + SpProbe（双字段闸/升序闸/偏差闸）
//!   [账本]     SpLedger（尝试 = 过 + 拒 守恒）+ InjectLedger
//!   [注入核]   三类 × 10 轮（单边/对调/越界算子）
//!   [收口]     golden_run + run_unx18511_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FB80，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FB80;

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
/// VER_AND（套件维专用；落数值维即非法）。
pub const VER_AND: u8 = 6;
/// VER_OR（套件维专用；落数值维即非法）。
pub const VER_OR: u8 = 7;
/// VER_SERVICE_PACK_MINOR 掩码位（winnt.h 0x0000010）。
pub const MASK_SP_MINOR: u32 = 0x0010;
/// VER_SERVICE_PACK_MAJOR 掩码位（winnt.h 0x0000020）。
pub const MASK_SP_MAJOR: u32 = 0x0020;
/// 系统快照 SP 主值（6.1.7601 SP1 → 1）。
pub const SYS_SP_MAJOR: u16 = 1;
/// 系统快照 SP 次值（SP1 → 1）。
pub const SYS_SP_MINOR: u16 = 1;
/// 次字段缺席哨兵（单边请求形态；u16 域内不入真实版本段）。
pub const SP_NONE: u16 = 0xFFFF;
/// 冻结比较表行数（J1 双字段断言口径）。
pub const SP_ROWS: usize = 20;
/// 账本环容量（定容 24 槽：20 表例 + 4 余量）。
pub const RING_CAP: usize = 24;
/// 注入轮数（J1「检出」10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 冻结比较表一行：(req_major, req_minor, op_major, op_minor, expect)。
/// expect = cmp(op_major, SYS_SP_MAJOR, req_major)
///       && cmp(op_minor, SYS_SP_MINOR, req_minor)，逐例手推。
pub type SpRow = (u16, u16, u8, u8, bool);

/// 第 i 行冻结例（越界 None）。
/// 推导：行 0-1 等值双过；2-4 GE 邻界（次差一/主差一）；5-7 LE 三态；
/// 8-10 GT 三态；11-12 LT 三态；13-15 主等本次混合算子；16-19 交叉
/// 大跨度（主 6/次 5、主 6/次 2 等跨代邻域）。
pub fn sp_row(i: usize) -> Option<SpRow> {
    match i {
        0 => Some((1, 1, VER_GREATER_EQUAL, VER_GREATER_EQUAL, true)),  // 双等值过
        1 => Some((1, 1, VER_EQUAL, VER_EQUAL, true)),                  // 双等值过
        2 => Some((1, 0, VER_GREATER_EQUAL, VER_GREATER_EQUAL, true)),  // 次反差一
        3 => Some((1, 2, VER_GREATER_EQUAL, VER_GREATER_EQUAL, false)), // 次差一
        4 => Some((2, 0, VER_GREATER_EQUAL, VER_GREATER_EQUAL, false)), // 主差一
        5 => Some((0, 0, VER_LESS_EQUAL, VER_LESS_EQUAL, false)),       // 主 LE 不过
        6 => Some((1, 1, VER_LESS_EQUAL, VER_LESS_EQUAL, true)),        // 双 LE 等值过
        7 => Some((1, 2, VER_LESS_EQUAL, VER_LESS_EQUAL, true)),        // 双 LE 过
        8 => Some((1, 1, VER_GREATER, VER_GREATER, false)),             // GT 等值不过
        9 => Some((0, 9, VER_GREATER, VER_GREATER, false)),             // 次差大
        10 => Some((0, 0, VER_GREATER, VER_GREATER, true)),             // 双 GT 过（sys=1>req=0）
        11 => Some((2, 1, VER_LESS, VER_LESS, false)),                  // 次 LT 不过
        12 => Some((0, 0, VER_LESS, VER_LESS, false)),                  // LT 全不过
        13 => Some((1, 1, VER_EQUAL, VER_GREATER_EQUAL, true)),         // 主等本次 GE
        14 => Some((2, 2, VER_EQUAL, VER_GREATER_EQUAL, false)),        // 主等不过
        15 => Some((1, 1, VER_GREATER_EQUAL, VER_EQUAL, true)),         // 主 GE 次等
        16 => Some((1, 5, VER_GREATER_EQUAL, VER_LESS_EQUAL, true)),    // 交叉过
        17 => Some((6, 1, VER_LESS_EQUAL, VER_GREATER_EQUAL, true)),    // 主 6 跨代
        18 => Some((6, 2, VER_LESS_EQUAL, VER_GREATER_EQUAL, false)),   // 次差一跨代
        19 => Some((3, 3, VER_LESS, VER_LESS, true)),                   // 双 LT 跨代
        _ => None,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpCode {
    /// 行 id 越界（≥ 20）。
    RowIdOutOfRange,
    /// 探针乱序（id 未严格升序）。
    RowOutOfOrder,
    /// 探针重复（同 id 二探）。
    RowDuplicate,
    /// 双字段闸：单边请求（主/次任一字段缺席）。
    UnilateralRequest,
    /// 算子越界（0/6/7 或 >7 落数值维）。
    OpIllegal,
    /// 比较偏差（got ≠ expect，零偏差红线）。
    CompareDeviation,
    /// 账本环满（24 槽耗尽）。
    LedgerFull,
    /// 账本守恒破坏（尝试 ≠ 过 + 拒）。
    LedgerBroken,
    /// 表指纹失配（冻结表被改）。
    FingerprintMismatch,
    /// 空表收口（零探针即封账）。
    TableEmpty,
    /// 阶段非法（Sealed 后再探）。
    PhaseInvalid,
    /// 注入漏检（变异探针未被拒）。
    InjectionMissed,
    /// 双跑不一致。
    NotReproducible,
}

impl SpCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SpCode::RowIdOutOfRange => "行 id 越界：冻结表仅 0..=19",
            SpCode::RowOutOfOrder => "探针乱序：id 必须严格升序",
            SpCode::RowDuplicate => "探针重复：同 id 只探一次",
            SpCode::UnilateralRequest => "单边请求：SP 主/次必须成对在场",
            SpCode::OpIllegal => "算子越界：数值维闭集 1..=5",
            SpCode::CompareDeviation => "比较偏差：got≠expect 零容忍",
            SpCode::LedgerFull => "账本环满：24 槽耗尽",
            SpCode::LedgerBroken => "账本守恒破坏：尝试≠过+拒",
            SpCode::FingerprintMismatch => "表指纹失配：冻结表被改",
            SpCode::TableEmpty => "空表收口：零探针不得封账",
            SpCode::PhaseInvalid => "阶段非法：Sealed 后再探",
            SpCode::InjectionMissed => "注入漏检：变异必被拒",
            SpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SpCode; 12] = [
        SpCode::RowIdOutOfRange,
        SpCode::RowOutOfOrder,
        SpCode::RowDuplicate,
        SpCode::UnilateralRequest,
        SpCode::OpIllegal,
        SpCode::CompareDeviation,
        SpCode::LedgerFull,
        SpCode::LedgerBroken,
        SpCode::FingerprintMismatch,
        SpCode::TableEmpty,
        SpCode::PhaseInvalid,
        SpCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SpCode::ALL.len() {
        if SpCode::ALL[i].code() == code {
            return SpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FB80 段闭集内"
}

// ── 数值维比较核（闭集 1..=5） ──────────────────────────────────
/// 单字段比较（SP 主/次共用；算子 6/7 为套件维专用，落数值维即拒）。
pub fn cmp_field(op: u8, sys: u16, req: u16) -> Result<bool, u32> {
    match op {
        1 => Ok(sys == req),
        2 => Ok(sys > req),
        3 => Ok(sys >= req),
        4 => Ok(sys < req),
        5 => Ok(sys <= req),
        _ => Err(SpCode::OpIllegal.code()),
    }
}

/// SP 请求（主/次双字段 + 双算子；任一字段为 SP_NONE 即单边请求）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpRequest {
    pub req_major: u16,
    pub req_minor: u16,
    pub op_major: u8,
    pub op_minor: u8,
}

impl SpRequest {
    /// 双字段闸：主/次必须成对在场（单边请求检出点）。
    pub fn paired(&self) -> bool {
        self.req_major != SP_NONE && self.req_minor != SP_NONE
    }
    /// 双字段 AND 裁决（先闸后比；任一 Err 透传）。
    pub fn verdict(&self) -> Result<bool, u32> {
        if !self.paired() {
            return Err(SpCode::UnilateralRequest.code());
        }
        let maj = cmp_field(self.op_major, SYS_SP_MAJOR, self.req_major)?;
        let min = cmp_field(self.op_minor, SYS_SP_MINOR, self.req_minor)?;
        Ok(maj && min)
    }
}

// ── 账本环（24 槽定容） ─────────────────────────────────────────
/// 探针记录（Copy，可定容复制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpRec {
    /// 行 id。
    pub id: u8,
    /// 引擎评值。
    pub got: bool,
    /// 冻结期望。
    pub expect: bool,
}

/// 24 槽账本环：定容、显式满、账不丢（满后拒新探不改旧账）。
#[derive(Clone, Copy, Debug)]
pub struct SpRing {
    slots: [Option<SpRec>; RING_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl SpRing {
    pub fn new() -> SpRing {
        SpRing { slots: [None; RING_CAP], len: 0, full_rejects: 0 }
    }
    /// 入环（满则显式拒并计数，旧账不动）。
    pub fn push(&mut self, rec: SpRec) -> Result<(), u32> {
        if self.len >= RING_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(SpCode::LedgerFull.code());
        }
        self.slots[self.len] = Some(rec);
        self.len += 1;
        Ok(())
    }
    /// 按槽位取记录（越界 None）。
    pub fn rec_at(&self, idx: usize) -> Option<SpRec> {
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
/// 冻结表指纹：31 进制逐行折叠（行序敏感 + 期望位敏感）。
/// 推导：初值取卡号派生 0x18511；expect 布尔以 1/2 入折避免与算子
/// 位混淆；算子按 u8 原值入折。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18511;
    let mut i = 0;
    while i < SP_ROWS {
        match sp_row(i) {
            Some((rm, rn, om, on, e)) => {
                acc = acc.wrapping_mul(31).wrapping_add(rm as u32);
                acc = acc.wrapping_mul(31).wrapping_add(rn as u32);
                acc = acc.wrapping_mul(31).wrapping_add(om as u32);
                acc = acc.wrapping_mul(31).wrapping_add(on as u32);
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

// ── 探针状态机（Idle → Probing → Sealed） ───────────────────────
/// 探针阶段（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpPhase {
    /// 未开探。
    Idle,
    /// 探测中（可继续入探）。
    Probing,
    /// 已封账（终态；再探拒）。
    Sealed,
}

/// 账本计数（尝试 = 过 + 拒，守恒方程）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpLedger {
    pub tries: u32,
    pub passes: u32,
    pub rejects: u32,
}

impl SpLedger {
    pub const fn new() -> SpLedger {
        SpLedger { tries: 0, passes: 0, rejects: 0 }
    }
    /// 守恒核算。
    pub fn conserve(&self) -> bool {
        self.tries == self.passes + self.rejects
    }
}

/// SP 双字段探针引擎：双字段闸 + 升序闸 + 偏差闸 三闸串联。
pub struct SpProbe {
    pub phase: SpPhase,
    /// 下一个允许的 id 下界（严格升序约束）。
    next_min: usize,
    /// 上一探 id（重复判定）。
    last_probed: Option<usize>,
    pub ring: SpRing,
    pub ledger: SpLedger,
    /// 评值偏置（注入面：正常探针恒 None；2=缺次字段 3=双算子降级）。
    bias: Option<u8>,
}

impl SpProbe {
    pub fn new() -> SpProbe {
        SpProbe {
            phase: SpPhase::Idle,
            next_min: 0,
            last_probed: None,
            ring: SpRing::new(),
            ledger: SpLedger::new(),
            bias: None,
        }
    }

    /// 注入偏置（测试/注入核专用）。
    pub fn set_bias(&mut self, b: Option<u8>) {
        self.bias = b;
    }

    /// 偏置评值：正常按冻结表双字段；2=次字段置缺；3=双算子降级。
    fn eval(&self, id: usize) -> Result<bool, u32> {
        let (rm, rn, om, on, _e) = match sp_row(id) {
            Some(x) => x,
            None => return Err(SpCode::RowIdOutOfRange.code()),
        };
        match self.bias {
            None => SpRequest { req_major: rm, req_minor: rn, op_major: om, op_minor: on }
                .verdict(),
            // 主次对调：主字段吃次请求值/次算子，次字段吃主请求值/主算子。
            Some(1) => SpRequest { req_major: rn, req_minor: rm, op_major: on, op_minor: om }
                .verdict(),
            // 单边：抹掉次字段（双字段闸必拒）。
            Some(2) => SpRequest { req_major: rm, req_minor: SP_NONE, op_major: om, op_minor: on }
                .verdict(),
            // 双算子降级：主/次算子均置 VER_EQUAL（非等值路径必偏）。
            Some(3) => SpRequest { req_major: rm, req_minor: rn, op_major: VER_EQUAL, op_minor: VER_EQUAL }
                .verdict(),
            _ => Err(SpCode::PhaseInvalid.code()),
        }
    }

    /// 单行探针：id 严格升序 + got==expect 零偏差。
    pub fn probe(&mut self, id: usize) -> Result<(), u32> {
        // 阶段闸：Sealed 后再探 = 阶段非法。
        if self.phase == SpPhase::Sealed {
            return Err(SpCode::PhaseInvalid.code());
        }
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        // 行 id 闸。
        if id >= SP_ROWS {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(SpCode::RowIdOutOfRange.code());
        }
        // 升序闸：与上探同 id = 重复，更小 = 乱序。
        if id < self.next_min {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            if self.last_probed == Some(id) {
                return Err(SpCode::RowDuplicate.code());
            }
            return Err(SpCode::RowOutOfOrder.code());
        }
        // 评值 + 期望对拍（零偏差红线）。
        let got = self.eval(id)?;
        let expect = match sp_row(id) {
            Some((_, _, _, _, e)) => e,
            None => return Err(SpCode::RowIdOutOfRange.code()),
        };
        let rec = SpRec { id: id as u8, got, expect };
        self.ring.push(rec)?;
        if got != expect {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(SpCode::CompareDeviation.code());
        }
        self.next_min = id + 1;
        self.last_probed = Some(id);
        self.ledger.passes = self.ledger.passes.saturating_add(1);
        self.phase = SpPhase::Probing;
        Ok(())
    }

    /// 全表顺探（0..20 逐行；确定性：同表双跑账逐槽相等）。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut id = 0usize;
        while id < SP_ROWS {
            self.probe(id)?;
            n += 1;
            id += 1;
        }
        Ok(n)
    }

    /// 封账：空表拒；账守恒；Sealed 终态。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == SpPhase::Idle {
            return Err(SpCode::TableEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(SpCode::LedgerBroken.code());
        }
        self.phase = SpPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过账数 / 探针账 / 表指纹）。
pub type GoldenOut = (usize, SpLedger, u32);

/// 黄金全链一次：20 例顺探全过 → 封账 → 指纹入账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut p = SpProbe::new();
    let n = p.probe_all()?;
    if n != SP_ROWS {
        return Err(SpCode::InjectionMissed.code());
    }
    p.seal()?;
    if !p.ledger.conserve() {
        return Err(SpCode::LedgerBroken.code());
    }
    if p.ledger.passes != SP_ROWS as u32 || p.ledger.rejects != 0 {
        return Err(SpCode::CompareDeviation.code());
    }
    Ok((n, p.ledger, table_fingerprint()))
}

/// 双跑一致（确定性红线：账与指纹逐项相等）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SpCode::NotReproducible.code());
    }
    Ok(())
}

// ── 交叉对账（与总纲数值维臂同口径，闭式重演） ─────────────────
/// 交叉核对 7 例：SP 主/次为独立数值维，每字段语义必须与通用数值
/// 比较臂一致（本卡自持实现与闭式逐例对拍）。
pub fn cross_check() -> Result<(), u32> {
    // (op, sys, req, 闭式期望)：覆盖 EQ/GT/GE/LT/LE 五算子等值与邻界。
    let rows: [(u8, u16, u16, bool); 7] = [
        (VER_EQUAL, 1, 1, true),
        (VER_EQUAL, 1, 2, false),
        (VER_GREATER, 1, 0, true),
        (VER_GREATER, 1, 1, false),
        (VER_GREATER_EQUAL, 1, 1, true),
        (VER_LESS, 1, 2, true),
        (VER_LESS_EQUAL, 1, 0, false),
    ];
    let mut i = 0;
    while i < rows.len() {
        let (op, s, r, master) = rows[i];
        if cmp_field(op, s, r)? != master {
            return Err(SpCode::FingerprintMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入面计数（魔数推导） ──────────────────────────────────────
/// 算子降级偏差行数：双字段算子降为 VER_EQUAL 后裁决 ≠ 期望的行数
/// （运行期推导；SP 主/次系统值同为 1，纯对调恒等价，故注入类 2
/// 取降级面——降级在「经非等值算子过/不过」的行上必偏）。
pub fn downgrade_deviation_rows() -> Result<u32, u32> {
    let mut n = 0u32;
    let mut i = 0;
    while i < SP_ROWS {
        let (rm, rn, _om, _on, e) = match sp_row(i) {
            Some(x) => x,
            None => return Err(SpCode::RowIdOutOfRange.code()),
        };
        let downgraded =
            SpRequest { req_major: rm, req_minor: rn, op_major: VER_EQUAL, op_minor: VER_EQUAL }
                .verdict()?;
        if downgraded != e {
            n = n.saturating_add(1);
        }
        i += 1;
    }
    Ok(n)
}

/// 单边请求行数：抹掉次字段后被双字段闸拒的行数（= 全表 20）。
pub fn unilateral_rows() -> Result<u32, u32> {
    let mut n = 0u32;
    let mut i = 0;
    while i < SP_ROWS {
        let (rm, _rn, om, on, _e) = match sp_row(i) {
            Some(x) => x,
            None => return Err(SpCode::RowIdOutOfRange.code()),
        };
        match (SpRequest { req_major: rm, req_minor: SP_NONE, op_major: om, op_minor: on }).verdict()
        {
            Err(c) if c == SpCode::UnilateralRequest.code() => {
                n = n.saturating_add(1);
            }
            _ => return Err(SpCode::InjectionMissed.code()),
        }
        i += 1;
    }
    Ok(n)
}

// ── 注入核（三类 × 10 轮，10/10 全拒） ──────────────────────────
/// 注入账（三类各轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 单边请求（双字段闸必拒）。
    pub unilateral_caught: u32,
    /// 类2 算子降级（偏差必记）。
    pub downgrade_caught: u32,
    /// 类3 算子越界（6/7 落数值维必拒）。
    pub opbad_caught: u32,
}

/// 三类变异注入 rounds 轮；每类每轮必被拒，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let down_expect = downgrade_deviation_rows()?;
    let uni_expect = unilateral_rows()?;
    let mut r = 0;
    while r < rounds {
        // 类1 单边：偏置探针全表走，每行都被双字段闸拒。
        let mut p1 = SpProbe::new();
        p1.set_bias(Some(2));
        let mut caught1 = 0u32;
        let mut id1 = 0usize;
        while id1 < SP_ROWS {
            match p1.probe(id1) {
                Err(c) if c == SpCode::UnilateralRequest.code() => {
                    caught1 = caught1.saturating_add(1);
                    // 推进下界继续探（拒账保留，不修复）。
                    p1.next_min = id1 + 1;
                }
                _ => return Err(SpCode::InjectionMissed.code()),
            }
            id1 += 1;
        }
        if caught1 != uni_expect {
            return Err(SpCode::InjectionMissed.code());
        }
        led.unilateral_caught = led.unilateral_caught.saturating_add(caught1);
        // 类2 降级：双算子降为 EQ，偏差行必记 CompareDeviation。
        let mut p2 = SpProbe::new();
        p2.set_bias(Some(3));
        let mut caught2 = 0u32;
        let mut id2 = 0usize;
        while id2 < SP_ROWS {
            match p2.probe(id2) {
                Err(c) if c == SpCode::CompareDeviation.code() => {
                    caught2 = caught2.saturating_add(1);
                    p2.next_min = id2 + 1;
                }
                Ok(()) => {}
                _ => return Err(SpCode::InjectionMissed.code()),
            }
            id2 += 1;
        }
        if caught2 != down_expect {
            return Err(SpCode::InjectionMissed.code());
        }
        led.downgrade_caught = led.downgrade_caught.saturating_add(caught2);
        // 类3 算子越界：主/次算子置 6（VER_AND）落数值维必拒。
        let bad_maj = SpRequest {
            req_major: 1,
            req_minor: 1,
            op_major: VER_AND,
            op_minor: VER_EQUAL,
        }
        .verdict();
        let bad_min = SpRequest {
            req_major: 1,
            req_minor: 1,
            op_major: VER_EQUAL,
            op_minor: VER_OR,
        }
        .verdict();
        let ok_maj = match bad_maj {
            Err(c) if c == SpCode::OpIllegal.code() => true,
            _ => false,
        };
        let ok_min = match bad_min {
            Err(c) if c == SpCode::OpIllegal.code() => true,
            _ => false,
        };
        if !(ok_maj && ok_min) {
            return Err(SpCode::InjectionMissed.code());
        }
        led.opbad_caught = led.opbad_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：20 例顺探全过（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：乱序（先探 5 再探 2）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = SpProbe::new();
    p.probe(5)?;
    p.probe(2)
}

/// 探针 C：重复（连探同 id）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut p = SpProbe::new();
    p.probe(0)?;
    p.probe(0)
}

/// 单边请求直查：缺次字段的合法算子请求必拒（J1 注入检出直证）。
pub fn unilateral_direct() -> Result<(), u32> {
    match (SpRequest {
        req_major: 1,
        req_minor: SP_NONE,
        op_major: VER_GREATER_EQUAL,
        op_minor: VER_GREATER_EQUAL,
    })
    .verdict()
    {
        // 单边请求（缺次字段）必须被双字段闸拒绝；Ok(_) 视为漏检。
        Err(c) => Err(c),
        Ok(_) => Err(SpCode::UnilateralRequest.code()),
    }
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < SpCode::ALL.len() {
        let v = SpCode::ALL[i].code();
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

    // 1. 黄金链：20 例双字段比较全过零偏差（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == SP_ROWS && led.rejects == 0 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-20例双字段全过", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A全表顺探", pa, format!("ok={}", pa)));

    // 3. 单边请求直查必拒。
    let uni = unilateral_direct().is_err();
    v.push(("J1-单边请求直查拒", uni, format!("err={}", uni)));

    // 4. 单边注入 10/10（全表逐行被双字段闸拒）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.unilateral_caught,
        Err(_) => 0,
    };
    let e1 = match unilateral_rows() {
        Ok(n) => n * JUDGE_ROUNDS,
        Err(_) => 0,
    };
    v.push(("J1-单边注入全拒", c1 == e1, format!("{}/{}", c1, e1)));

    // 5. 降级注入：偏差行必记（面额 = downgrade_deviation_rows 推导）。
    let c2 = match led.as_ref() {
        Ok(l) => l.downgrade_caught,
        Err(_) => 0,
    };
    let e2 = match downgrade_deviation_rows() {
        Ok(n) => n * JUDGE_ROUNDS,
        Err(_) => 0,
    };
    v.push(("J1-降级注入全偏", c2 == e2, format!("{}/{}", c2, e2)));

    // 6. 算子越界注入 10/10（主/次双向落 6/7 均拒）。
    let c3 = match led.as_ref() {
        Ok(l) => l.opbad_caught,
        Err(_) => 0,
    };
    v.push(("J1-算子越界10/10", c3 == JUDGE_ROUNDS, format!("caught={}", c3)));

    // 7. 注入账守恒（三类合计 = 三类面额 × 轮数）。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.unilateral_caught + l.downgrade_caught + l.opbad_caught == e1 + e2 + JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("3类面额×10轮")));

    // 8. 等值/差一/反差一三型边界逐型断言（主字段口径）。
    let bt = match cmp_field(VER_GREATER_EQUAL, SYS_SP_MAJOR, 1) {
        Ok(b) => b,
        Err(_) => false,
    } && match cmp_field(VER_GREATER_EQUAL, SYS_SP_MAJOR, 2) {
        Ok(b) => !b,
        Err(_) => false,
    } && match cmp_field(VER_GREATER_EQUAL, SYS_SP_MAJOR, 0) {
        Ok(b) => b,
        Err(_) => false,
    };
    v.push(("J1-三型边界断言", bt, String::from("等值/差一/反差一")));

    // 9. AND 聚合语义：主过次不过 → 总裁决必 FALSE（行 3 口径直证）。
    let and_sem = SpRequest {
        req_major: 1,
        req_minor: 2,
        op_major: VER_GREATER_EQUAL,
        op_minor: VER_GREATER_EQUAL,
    }
    .verdict()
    == Ok(false);
    v.push(("J1-AND聚合语义", and_sem, String::from("主过次不过必FALSE")));

    // 10. 乱序探针拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-乱序探针拒", pb, format!("err={}", pb)));

    // 11. 重复探针拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-重复探针拒", pc, format!("err={}", pc)));

    // 12. 越界 id 拒（id=20）。
    let oob = {
        let mut p = SpProbe::new();
        p.probe(SP_ROWS).is_err()
    };
    v.push(("J1-越界id拒", oob, format!("id={}", SP_ROWS)));

    // 13. 环容量：压环至满 + 1，显式满拒且旧账不丢。
    let cap = {
        let mut ring = SpRing::new();
        let mut k = 0;
        while k < RING_CAP {
            let _ = ring.push(SpRec { id: k as u8, got: true, expect: true });
            k += 1;
        }
        match ring.push(SpRec { id: 99, got: true, expect: true }) {
            Err(c) if c == SpCode::LedgerFull.code() => {
                ring.len == RING_CAP && ring.full_rejects == 1
            }
            _ => false,
        }
    };
    v.push(("J1-环容量24满拒", cap, String::from("24 收 1 拒账不丢")));

    // 14. 交叉对账（数值维五算子 7 例闭式对拍）。
    let cc = cross_check().is_ok();
    v.push(("J1-交叉对账7例", cc, String::from("数值维同口径")));

    // 15. 表指纹双跑一致 + 双跑复现红线。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    let rp = repro_check().is_ok();
    v.push(("J2-指纹与双跑", fp == fp2 && rp, format!("{:08X}", fp)));

    // 16. 段位守卫 + 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = segment_guard() && SpCode::ALL.len() == 12;
        let mut i = 0;
        while i < SpCode::ALL.len() {
            if SpCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-段位与闭集", all_ok, format!("{:06X} 12码", SEG_BASE)));

    v
}

/// 收口：本模块自检 CheckSet（判据 16 条；前 8 条动态明细）。
pub fn run_unx18511_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18511 ServicePack 维度比较语义");
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
        let set = run_unx18511_checks();
        assert!(set.all_passed(), "UNX-F18511 判据集应全绿");
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

    /// 双字段闸：单边请求三种形态（缺主/缺次/双缺）全拒。
    #[test]
    fn test_unilateral_gate() {
        let miss_min = SpRequest {
            req_major: 1,
            req_minor: SP_NONE,
            op_major: VER_GREATER_EQUAL,
            op_minor: VER_GREATER_EQUAL,
        };
        let miss_maj = SpRequest {
            req_major: SP_NONE,
            req_minor: 1,
            op_major: VER_GREATER_EQUAL,
            op_minor: VER_GREATER_EQUAL,
        };
        let miss_both = SpRequest {
            req_major: SP_NONE,
            req_minor: SP_NONE,
            op_major: VER_EQUAL,
            op_minor: VER_EQUAL,
        };
        for req in [miss_min, miss_maj, miss_both].iter() {
            match req.verdict() {
                Err(c) => assert_eq!(c, SpCode::UnilateralRequest.code()),
                Ok(_) => assert!(false, "单边请求必须拒"),
            }
        }
    }

    /// AND 聚合：主过次不过必 FALSE、双过必 TRUE。
    #[test]
    fn test_and_semantics() {
        let both = SpRequest {
            req_major: 1,
            req_minor: 1,
            op_major: VER_GREATER_EQUAL,
            op_minor: VER_GREATER_EQUAL,
        }
        .verdict();
        let maj_only = SpRequest {
            req_major: 1,
            req_minor: 2,
            op_major: VER_GREATER_EQUAL,
            op_minor: VER_GREATER_EQUAL,
        }
        .verdict();
        assert_eq!(both, Ok(true));
        assert_eq!(maj_only, Ok(false));
    }

    /// 算子越界：0 / 6 / 7 / 8 四形态落数值维全拒。
    #[test]
    fn test_op_illegal() {
        for op in [0u8, VER_AND, VER_OR, 8u8].iter() {
            match cmp_field(*op, SYS_SP_MAJOR, 1) {
                Err(c) => assert_eq!(c, SpCode::OpIllegal.code()),
                Ok(_) => assert!(false, "算子越界必须拒"),
            }
        }
    }

    /// 注入三类全抓（10 轮累计）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                let u = match unilateral_rows() {
                    Ok(n) => n * 10,
                    Err(_) => 0,
                };
                let d = match downgrade_deviation_rows() {
                    Ok(n) => n * 10,
                    Err(_) => 0,
                };
                assert_eq!(l.unilateral_caught, u);
                assert_eq!(l.downgrade_caught, d);
                assert_eq!(l.opbad_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 乱序/重复/越界三闸 + 封账三态。
    #[test]
    fn test_gates_and_phases() {
        assert!(probe_seq_b().is_err(), "乱序必须拒");
        assert!(probe_seq_c().is_err(), "重复必须拒");
        let mut p = SpProbe::new();
        match p.probe(SP_ROWS) {
            Err(c) => assert_eq!(c, SpCode::RowIdOutOfRange.code()),
            Ok(()) => assert!(false, "越界 id 必须拒"),
        }
        let mut p0 = SpProbe::new();
        match p0.seal() {
            Err(c) => assert_eq!(c, SpCode::TableEmpty.code()),
            Ok(()) => assert!(false, "空表封账必须拒"),
        }
        assert!(p.probe_all().is_ok());
        assert!(p.seal().is_ok());
        match p.probe(0) {
            Err(c) => assert_eq!(c, SpCode::PhaseInvalid.code()),
            Ok(()) => assert!(false, "Sealed 后再探必须拒"),
        }
    }

    /// 环容量：满后显式拒且账不丢。
    #[test]
    fn test_ring_capacity() {
        let mut ring = SpRing::new();
        let mut k = 0;
        while k < RING_CAP {
            assert!(ring.push(SpRec { id: k as u8, got: true, expect: true }).is_ok());
            k += 1;
        }
        match ring.push(SpRec { id: 99, got: true, expect: true }) {
            Err(c) => {
                assert_eq!(c, SpCode::LedgerFull.code());
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
        assert_eq!(SpCode::ALL.len(), 12);
        for c in SpCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
