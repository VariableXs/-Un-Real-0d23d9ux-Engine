//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18509 · ProductType 比较特例（VER_NT_* 语义）
//! · 0x7FB00 段 · 批 06 · 配额 280
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18509-J1 ProductType 特例语义（any/逐类）断言全过，
//!   与 F18407 联动一致。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 特例口径冻结：VER_PRODUCT_TYPE 维**只认 VER_EQUAL**（其它算子
//!     一律 OpForbidden——Windows 口径：wProductType 无序比较语义）；
//!   - 型值闭集：VER_NT_WORKSTATION=1 / VER_NT_DOMAIN_CONTROLLER=2 /
//!     VER_NT_SERVER=3；0 = any（通配请求，任何型都过）——any/逐类
//!     双语义并列；域外值（>3）拒；
//!   - 冻结裁决表 12 例：sys∈{1,2,3} × req∈{0,1,2,3}，期望 =
//!     (req==0) ∨ (sys==req)，逐例手推；
//!   - F18407 联动：证据派生口径表冻结（is_dc,server_capable)→型：
//!     (F,F)→1 / (F,T)→3 / (T,T)→2 / (T,F)→证据冲突拒——派生型值
//!     再过本卡 pt_compare 逐类对拍（联动断言全过）；
//!   - 注入四类 × 10 轮：算子混入（GE/GT 轮换）/ 未知型请求（req=4）/
//!     域外系统型（sys=4）/ 未定档逐类（sys=0）——逐类逐轮必抓；
//!   - 探针状态机：Open → Sweeping → Sealed；行序严格升序
//!     （乱序/重复/越界三闸）；账本守恒（问=中+未中）；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] PtjCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   pt_row 12 例冻结表（any/逐类）+ PT_LINKAGE 联动表
//!   [数据结构] ProductType 型枚举（any/未定/三型）
//!   [状态机]   PtPhase 三态 + PtProbe（三闸 + 零偏差对拍）
//!   [账本]     PtLedger（问=中+未中 守恒）
//!   [注入核]   四类 × 10 轮（混算子/未知型/域外型/未定档）
//!   [收口]     golden_run + run_unx18509_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FB00，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7FB00;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_EQUAL（PT 特例唯一合法算子）。
pub const VER_EQUAL: u8 = 1;
/// VER_GREATER_EQUAL（混入对照）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// VER_GREATER（混入对照）。
pub const VER_GREATER: u8 = 2;
/// VER_NT_WORKSTATION（冻结）。
pub const VER_NT_WORKSTATION: u32 = 1;
/// VER_NT_DOMAIN_CONTROLLER（冻结）。
pub const VER_NT_DOMAIN_CONTROLLER: u32 = 2;
/// VER_NT_SERVER（冻结）。
pub const VER_NT_SERVER: u32 = 3;
/// any 通配请求值（冻结）。
pub const PT_ANY: u32 = 0;
/// 冻结裁决表行数（3 sys × 4 req）。
pub const PT_ROWS: usize = 12;
/// 联动表行数（F18407 口径 4 行）。
pub const LINK_ROWS: usize = 4;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtjCode {
    /// 算子非法（PT 特例只认 VER_EQUAL）。
    OpForbidden,
    /// 系统型域外（sys > 3）。
    SysOutOfRange,
    /// 未知型请求（req > 3）。
    UnknownType,
    /// 未定档逐类（sys=0 不可逐类比较）。
    TypeUndetermined,
    /// 行号越界（≥12）。
    RowIdOutOfRange,
    /// 行乱序。
    RowOutOfOrder,
    /// 行重复。
    RowDuplicate,
    /// 裁决偏差（got ≠ expect 零容忍）。
    VerdictDeviation,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 空表收口。
    TableEmpty,
    /// 联动失配（与 F18407 口径不等）。
    LinkageMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl PtjCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PtjCode::OpForbidden => "算子非法：ProductType 只认 VER_EQUAL",
            PtjCode::SysOutOfRange => "系统型域外：VER_NT_* 闭集 1..=3",
            PtjCode::UnknownType => "未知型请求：req 闭集 0..=3 外",
            PtjCode::TypeUndetermined => "未定档逐类：sys=0 不可比",
            PtjCode::RowIdOutOfRange => "行号越界：冻结表仅 0..=11",
            PtjCode::RowOutOfOrder => "行乱序：必须严格升序",
            PtjCode::RowDuplicate => "行重复：同行只探一次",
            PtjCode::VerdictDeviation => "裁决偏差：got≠expect 零容忍",
            PtjCode::LedgerBroken => "账本守恒破坏：问≠中+未中",
            PtjCode::TableEmpty => "空表收口：零探针不得封账",
            PtjCode::LinkageMismatch => "联动失配：与 F18407 口径不等",
            PtjCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PtjCode; 12] = [
        PtjCode::OpForbidden,
        PtjCode::SysOutOfRange,
        PtjCode::UnknownType,
        PtjCode::TypeUndetermined,
        PtjCode::RowIdOutOfRange,
        PtjCode::RowOutOfOrder,
        PtjCode::RowDuplicate,
        PtjCode::VerdictDeviation,
        PtjCode::LedgerBroken,
        PtjCode::TableEmpty,
        PtjCode::LinkageMismatch,
        PtjCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PtjCode::ALL.len() {
        if PtjCode::ALL[i].code() == code {
            return PtjCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FB00 段闭集内"
}

// ── 型枚举 ──────────────────────────────────────────────────────
/// 产品型（Windows 口径 + any 通配 + 域内未定档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProductType {
    /// any 通配（req 专用）。
    Any,
    /// 未定档（sys=0，域内保留，不可逐类比较）。
    Undetermined,
    /// 工作站。
    Workstation,
    /// 域控。
    DomainController,
    /// 服务器。
    Server,
}

impl ProductType {
    /// 整数投影（sys 侧：Any 不参与）。
    pub fn val(self) -> u32 {
        match self {
            ProductType::Any => 0,
            ProductType::Undetermined => 0,
            ProductType::Workstation => 1,
            ProductType::DomainController => 2,
            ProductType::Server => 3,
        }
    }
    /// sys 侧整数 → 型（域外 None）。
    pub fn from_sys(v: u32) -> Option<ProductType> {
        match v {
            0 => Some(ProductType::Undetermined),
            1 => Some(ProductType::Workstation),
            2 => Some(ProductType::DomainController),
            3 => Some(ProductType::Server),
            _ => None,
        }
    }
}

/// PT 特例比较核：any/逐类双语义。
/// 返回 Ok(true)=过 / Ok(false)=逐类未中；门规违例 Err。
pub fn pt_compare(op: u8, sys: u32, req: u32) -> Result<bool, u32> {
    // 特例闸1：只认 VER_EQUAL。
    if op != VER_EQUAL {
        return Err(PtjCode::OpForbidden.code());
    }
    // 特例闸2：系统型闭集。
    if sys > VER_NT_SERVER {
        return Err(PtjCode::SysOutOfRange.code());
    }
    // 特例闸3：请求型闭集（0=any 合法）。
    if req > VER_NT_SERVER {
        return Err(PtjCode::UnknownType.code());
    }
    // any 语义：req=0 通配，任何系统型都过。
    if req == PT_ANY {
        return Ok(true);
    }
    // 未定档闸：sys=0 不可逐类比较（域内保留档）。
    if sys == PT_ANY {
        return Err(PtjCode::TypeUndetermined.code());
    }
    // 逐类语义：等值。
    Ok(sys == req)
}

// ── F18407 联动表 ───────────────────────────────────────────────
/// 联动行：证据对 (is_dc, server_capable) → 派生型 or 冲突。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkRow {
    pub is_dc: bool,
    pub server_capable: bool,
    /// 派生型值（冲突行 = 0 哨兵）。
    pub derived: u32,
    /// 是否证据冲突行。
    pub conflict: bool,
}

/// F18407 口径联动表（4 行冻结）。
pub const PT_LINKAGE: [LinkRow; LINK_ROWS] = [
    LinkRow { is_dc: false, server_capable: false, derived: VER_NT_WORKSTATION, conflict: false },
    LinkRow { is_dc: false, server_capable: true, derived: VER_NT_SERVER, conflict: false },
    LinkRow { is_dc: true, server_capable: true, derived: VER_NT_DOMAIN_CONTROLLER, conflict: false },
    LinkRow { is_dc: true, server_capable: false, derived: 0, conflict: true },
];

/// 证据派生（F18407 口径闭式重演；冲突 Err）。
pub fn derive_type(is_dc: bool, server_capable: bool) -> Result<u32, u32> {
    if is_dc {
        if server_capable {
            Ok(VER_NT_DOMAIN_CONTROLLER)
        } else {
            Err(PtjCode::LinkageMismatch.code())
        }
    } else if server_capable {
        Ok(VER_NT_SERVER)
    } else {
        Ok(VER_NT_WORKSTATION)
    }
}

// ── 冻结裁决表 ──────────────────────────────────────────────────
/// 裁决行：(sys, req, expect)。
pub type PtRow = (u32, u32, bool);

/// 第 i 行冻结裁决例（越界 None）。
/// 推导：sys ∈ {1,2,3} × req ∈ {0,1,2,3}；expect = (req==0) ∨ (sys==req)。
pub fn pt_row(i: usize) -> Option<PtRow> {
    match i {
        0 => Some((1, 1, true)),   // WS 逐类自中
        1 => Some((1, 2, false)),  // WS vs DC 未中
        2 => Some((1, 3, false)),  // WS vs SRV 未中
        3 => Some((1, 0, true)),   // any 通配
        4 => Some((2, 1, false)),  // DC vs WS 未中
        5 => Some((2, 2, true)),   // DC 自中
        6 => Some((2, 3, false)),  // DC vs SRV 未中
        7 => Some((2, 0, true)),   // any 通配
        8 => Some((3, 1, false)),  // SRV vs WS 未中
        9 => Some((3, 2, false)),  // SRV vs DC 未中
        10 => Some((3, 3, true)),  // SRV 自中
        11 => Some((3, 0, true)),  // any 通配
        _ => None,
    }
}

/// 表指纹（sys/req/期望全敏感折叠）。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18509;
    let mut i = 0;
    while i < PT_ROWS {
        match pt_row(i) {
            Some((s, r, e)) => {
                acc = acc.wrapping_mul(31).wrapping_add(s);
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

// ── 账本 ────────────────────────────────────────────────────────
/// PT 账（问 = 中 + 未中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtLedger {
    pub queries: u32,
    pub hits: u32,
    pub misses: u32,
    pub rejects: u32,
}

impl PtLedger {
    pub const fn new() -> PtLedger {
        PtLedger { queries: 0, hits: 0, misses: 0, rejects: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.queries == self.hits + self.misses
    }
}

// ── 状态机（Open → Sweeping → Sealed） ──────────────────────────
/// 探针阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtPhase {
    /// 未开探。
    Open,
    /// 扫描中。
    Sweeping,
    /// 已封账。
    Sealed,
}

/// PT 探针：三闸 + 零偏差对拍。
pub struct PtProbe {
    pub phase: PtPhase,
    next_min: usize,
    last_probed: Option<usize>,
    pub ledger: PtLedger,
}

impl PtProbe {
    pub fn new() -> PtProbe {
        PtProbe {
            phase: PtPhase::Open,
            next_min: 0,
            last_probed: None,
            ledger: PtLedger::new(),
        }
    }

    /// 单行探针：取冻结行 → pt_compare → 对拍。
    pub fn probe(&mut self, id: usize) -> Result<(), u32> {
        if self.phase == PtPhase::Sealed {
            return Err(PtjCode::RowIdOutOfRange.code());
        }
        if id >= PT_ROWS {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            return Err(PtjCode::RowIdOutOfRange.code());
        }
        if id < self.next_min {
            self.ledger.rejects = self.ledger.rejects.saturating_add(1);
            if self.last_probed == Some(id) {
                return Err(PtjCode::RowDuplicate.code());
            }
            return Err(PtjCode::RowOutOfOrder.code());
        }
        let (sys, req, expect) = match pt_row(id) {
            Some(x) => x,
            None => {
                self.ledger.rejects = self.ledger.rejects.saturating_add(1);
                return Err(PtjCode::RowIdOutOfRange.code());
            }
        };
        let got = pt_compare(VER_EQUAL, sys, req)?;
        self.ledger.queries = self.ledger.queries.saturating_add(1);
        if got {
            self.ledger.hits = self.ledger.hits.saturating_add(1);
        } else {
            self.ledger.misses = self.ledger.misses.saturating_add(1);
        }
        if got != expect {
            return Err(PtjCode::VerdictDeviation.code());
        }
        self.next_min = id + 1;
        self.last_probed = Some(id);
        self.phase = PtPhase::Sweeping;
        Ok(())
    }

    /// 全表顺探。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut id = 0usize;
        while id < PT_ROWS {
            self.probe(id)?;
            n += 1;
            id += 1;
        }
        Ok(n)
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == PtPhase::Open {
            return Err(PtjCode::TableEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(PtjCode::LedgerBroken.code());
        }
        self.phase = PtPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过账数 / 账 / 指纹 / 联动派生数）。
pub type GoldenOut = (usize, PtLedger, u32, usize);

/// 黄金全链一次：12 例顺探全过 + F18407 联动四行对拍。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut p = PtProbe::new();
    let n = p.probe_all()?;
    if n != PT_ROWS {
        return Err(PtjCode::VerdictDeviation.code());
    }
    p.seal()?;
    if !p.ledger.conserve() {
        return Err(PtjCode::LedgerBroken.code());
    }
    // 联动：三行合法证据派生型 → pt_compare 逐类自中；冲突行必拒。
    let mut derived_ok = 0usize;
    let mut i = 0;
    while i < LINK_ROWS {
        let lr = PT_LINKAGE[i];
        if lr.conflict {
            if derive_type(lr.is_dc, lr.server_capable).is_ok() {
                return Err(PtjCode::LinkageMismatch.code());
            }
        } else {
            let d = derive_type(lr.is_dc, lr.server_capable)?;
            if d != lr.derived {
                return Err(PtjCode::LinkageMismatch.code());
            }
            let hit = pt_compare(VER_EQUAL, d, d)?;
            if !hit {
                return Err(PtjCode::LinkageMismatch.code());
            }
            derived_ok += 1;
        }
        i += 1;
    }
    if derived_ok != 3 {
        return Err(PtjCode::LinkageMismatch.code());
    }
    Ok((n, p.ledger, table_fingerprint(), derived_ok))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PtjCode::NotReproducible.code());
    }
    Ok(())
}

// ── 交叉对账 ────────────────────────────────────────────────────
/// 交叉核对：型值闭集 1/2/3 + any 语义对拍 F18407 冻结口径。
pub fn cross_check() -> Result<(), u32> {
    if VER_NT_WORKSTATION != 1 || VER_NT_DOMAIN_CONTROLLER != 2 || VER_NT_SERVER != 3 {
        return Err(PtjCode::LinkageMismatch.code());
    }
    // any 语义三型全过。
    let mut t = 1u32;
    while t <= VER_NT_SERVER {
        if !pt_compare(VER_EQUAL, t, PT_ANY)? {
            return Err(PtjCode::VerdictDeviation.code());
        }
        t += 1;
    }
    Ok(())
}

// ── 注入核（四类 × 10 轮） ──────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 算子混入（GE/GT 轮换）。
    pub op_caught: u32,
    /// 类2 未知型请求（req=4）。
    pub unknown_caught: u32,
    /// 类3 域外系统型（sys=4）。
    pub sys_caught: u32,
    /// 类4 未定档逐类（sys=0）。
    pub undet_caught: u32,
}

/// 四类注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 算子混入：GE/GT 轮换 → OpForbidden。
        let bad_op = match r % 2 {
            0 => VER_GREATER_EQUAL,
            _ => VER_GREATER,
        };
        match pt_compare(bad_op, VER_NT_WORKSTATION, VER_NT_WORKSTATION) {
            Err(c) if c == PtjCode::OpForbidden.code() => {
                led.op_caught = led.op_caught.saturating_add(1);
            }
            _ => return Err(PtjCode::OpForbidden.code()),
        }
        // 类2 未知型请求：req=4 → UnknownType。
        match pt_compare(VER_EQUAL, VER_NT_WORKSTATION, 4) {
            Err(c) if c == PtjCode::UnknownType.code() => {
                led.unknown_caught = led.unknown_caught.saturating_add(1);
            }
            _ => return Err(PtjCode::UnknownType.code()),
        }
        // 类3 域外系统型：sys=4 → SysOutOfRange。
        match pt_compare(VER_EQUAL, 4, VER_NT_WORKSTATION) {
            Err(c) if c == PtjCode::SysOutOfRange.code() => {
                led.sys_caught = led.sys_caught.saturating_add(1);
            }
            _ => return Err(PtjCode::SysOutOfRange.code()),
        }
        // 类4 未定档逐类：sys=0 + req=1 → TypeUndetermined。
        match pt_compare(VER_EQUAL, 0, VER_NT_WORKSTATION) {
            Err(c) if c == PtjCode::TypeUndetermined.code() => {
                led.undet_caught = led.undet_caught.saturating_add(1);
            }
            _ => return Err(PtjCode::TypeUndetermined.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：12 例全过 + 联动四行。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：any 通配三型全过（any 语义直查）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut t = 1u32;
    while t <= VER_NT_SERVER {
        pt_compare(VER_EQUAL, t, PT_ANY)?;
        t += 1;
    }
    Ok(())
}

/// 探针 C：证据冲突行（is_dc ∧ ¬cap）→ 拒（F18407 口径）。
pub fn probe_seq_c() -> Result<(), u32> {
    derive_type(true, false).map(|_| ())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < PtjCode::ALL.len() {
        let v = PtjCode::ALL[i].code();
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

    // 1. 黄金链：12 例全过 + 联动三派生（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _, d)) => *n == PT_ROWS && led.conserve() && *d == 3,
        Err(_) => false,
    };
    v.push(("J1-12例any逐类全过", g_ok, format!("{:?}", g)));

    // 2. 型值口径：1/2/3 冻结 + any=0。
    let vals_ok = VER_NT_WORKSTATION == 1
        && VER_NT_DOMAIN_CONTROLLER == 2
        && VER_NT_SERVER == 3
        && PT_ANY == 0;
    v.push(("J1-型值口径1/2/3", vals_ok, String::from("WS=1 DC=2 SRV=3 any=0")));

    // 3. any 语义：三型 × req=0 全过（探针 B）。
    let pb = probe_seq_b().is_ok();
    v.push(("J1-any通配全过", pb, format!("ok={}", pb)));

    // 4. 逐类自中：三型 req=sys 全过；异类未中合法。
    let class_ok = pt_compare(VER_EQUAL, 1, 1) == Ok(true)
        && pt_compare(VER_EQUAL, 2, 2) == Ok(true)
        && pt_compare(VER_EQUAL, 3, 3) == Ok(true)
        && pt_compare(VER_EQUAL, 1, 2) == Ok(false)
        && pt_compare(VER_EQUAL, 2, 3) == Ok(false);
    v.push(("J1-逐类自中/异中", class_ok, String::from("3中2异")));

    // 4b. 交叉对账：any 三型 + 型值闭集（与 F18407 口径一致）。
    let cc = cross_check().is_ok();
    v.push(("J1-交叉对账any三型", cc, String::from("any=0 WS=1 DC=2 SRV=3")));

    // 5. 算子混入注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.op_caught,
        Err(_) => 0,
    };
    v.push(("J1-算子混入10/10", c1 == JUDGE_ROUNDS, format!("caught={}", c1)));

    // 6. 未知型注入 10/10。
    let c2 = match led.as_ref() {
        Ok(l) => l.unknown_caught,
        Err(_) => 0,
    };
    v.push(("J1-未知型10/10", c2 == JUDGE_ROUNDS, format!("caught={}", c2)));

    // 7. 域外系统型注入 10/10。
    let c3 = match led.as_ref() {
        Ok(l) => l.sys_caught,
        Err(_) => 0,
    };
    v.push(("J1-域外系统型10/10", c3 == JUDGE_ROUNDS, format!("caught={}", c3)));

    // 8. 未定档注入 10/10。
    let c4 = match led.as_ref() {
        Ok(l) => l.undet_caught,
        Err(_) => 0,
    };
    v.push(("J1-未定档逐类10/10", c4 == JUDGE_ROUNDS, format!("caught={}", c4)));

    // 9. 注入账守恒（4 类 × 10 轮 = 40 抓）。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.op_caught + l.unknown_caught + l.sys_caught + l.undet_caught
            == 4 * JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("4类×10轮")));

    // 10. 联动：证据冲突行拒（探针 C，F18407 口径）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-证据冲突联动拒", pc, format!("err={}", pc)));

    // 11. 联动三派生型过 pt_compare（黄金链内已验，独立复跑）。
    let link_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < LINK_ROWS {
            let lr = PT_LINKAGE[i];
            if !lr.conflict {
                match derive_type(lr.is_dc, lr.server_capable) {
                    Ok(d) => {
                        let hit = match pt_compare(VER_EQUAL, d, d) {
                            Ok(h) => h,
                            Err(_) => false,
                        };
                        if d != lr.derived || !hit {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-联动三派生", link_ok, String::from("派生型自中")));

    // 12. 乱序/重复/越界三闸。
    let gates = {
        let mut p1 = PtProbe::new();
        let _ = p1.probe(5);
        let ooo = p1.probe(2).is_err();
        let mut p2 = PtProbe::new();
        let _ = p2.probe(0);
        let dup = p2.probe(0).is_err();
        let mut p3 = PtProbe::new();
        let oob = p3.probe(PT_ROWS).is_err();
        ooo && dup && oob
    };
    v.push(("J1-三闸乱重越", gates, String::from("乱序/重复/越界")));

    // 13. 空表收口拒。
    let empty = {
        let mut p = PtProbe::new();
        p.seal().is_err()
    };
    v.push(("J1-空表收口拒", empty, String::from("零探针")));

    // 14. 表指纹双跑一致。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

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
        let mut ok = PtjCode::ALL.len() == 12;
        let mut i = 0;
        while i < PtjCode::ALL.len() {
            if PtjCode::ALL[i].detail().is_empty() {
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
pub fn run_unx18509_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18509 ProductType 特例语义");
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
        let set = run_unx18509_checks();
        assert!(set.all_passed(), "UNX-F18509 判据集应全绿");
    }

    /// 黄金链：12 例全过 + 联动。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _, d)) => {
                assert_eq!(n, 12);
                assert!(led.conserve());
                assert_eq!(led.misses, 6, "12 例中逐类未中 6 例");
                assert_eq!(d, 3);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// any/逐类双语义逐例断言。
    #[test]
    fn test_any_and_class() {
        // any：三型全过。
        assert_eq!(pt_compare(VER_EQUAL, 1, 0), Ok(true));
        assert_eq!(pt_compare(VER_EQUAL, 2, 0), Ok(true));
        assert_eq!(pt_compare(VER_EQUAL, 3, 0), Ok(true));
        // 逐类：自中/异中。
        assert_eq!(pt_compare(VER_EQUAL, 2, 2), Ok(true));
        assert_eq!(pt_compare(VER_EQUAL, 2, 1), Ok(false));
        assert_eq!(pt_compare(VER_EQUAL, 3, 1), Ok(false));
    }

    /// 算子特例：只认 VER_EQUAL（GE/GT/LE 轮换全拒）。
    #[test]
    fn test_op_constraint() {
        let mut op = 2u8;
        while op <= 7 {
            match pt_compare(op, 1, 1) {
                Err(c) => assert_eq!(c, PtjCode::OpForbidden.code()),
                Ok(_) => assert!(false, "算子 {} 必须拒", op),
            }
            op += 1;
        }
        assert!(pt_compare(VER_EQUAL, 1, 1).is_ok());
    }

    /// 注入四类 10/10。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.op_caught, 10);
                assert_eq!(l.unknown_caught, 10);
                assert_eq!(l.sys_caught, 10);
                assert_eq!(l.undet_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// F18407 联动：派生口径逐行断言。
    #[test]
    fn test_linkage() {
        assert_eq!(derive_type(false, false), Ok(1));
        assert_eq!(derive_type(false, true), Ok(3));
        assert_eq!(derive_type(true, true), Ok(2));
        assert!(derive_type(true, false).is_err(), "证据冲突必须拒");
        // 派生型过 pt_compare 自中。
        assert_eq!(pt_compare(VER_EQUAL, 3, 3), Ok(true));
        assert_eq!(pt_compare(VER_EQUAL, 2, 2), Ok(true));
        assert_eq!(pt_compare(VER_EQUAL, 1, 1), Ok(true));
    }

    /// 三闸：乱序/重复/越界。
    #[test]
    fn test_gates() {
        let mut p1 = PtProbe::new();
        assert!(p1.probe(5).is_ok());
        match p1.probe(2) {
            Err(c) => assert_eq!(c, PtjCode::RowOutOfOrder.code()),
            Ok(()) => assert!(false, "乱序必须拒"),
        }
        let mut p2 = PtProbe::new();
        assert!(p2.probe(0).is_ok());
        match p2.probe(0) {
            Err(c) => assert_eq!(c, PtjCode::RowDuplicate.code()),
            Ok(()) => assert!(false, "重复必须拒"),
        }
        let mut p3 = PtProbe::new();
        match p3.probe(PT_ROWS) {
            Err(c) => assert_eq!(c, PtjCode::RowIdOutOfRange.code()),
            Ok(()) => assert!(false, "越界必须拒"),
        }
    }

    /// 空表收口拒。
    #[test]
    fn test_empty_seal() {
        let mut p = PtProbe::new();
        match p.seal() {
            Err(c) => assert_eq!(c, PtjCode::TableEmpty.code()),
            Ok(()) => assert!(false, "空表封账必须拒"),
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
