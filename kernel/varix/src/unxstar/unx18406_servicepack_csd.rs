//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18406 · wServicePackMajor/Minor 语义
//! · 0x7E140 段 · 批 01 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18406-J1 服务包号与注册表 CSD 值（AI-18 接口）联动一致
//!   断言全过，hive 缺键回退路径覆盖。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - CSD 语法冻结："Service Pack M"（Windows 原生）与 "Service Pack M.N"
//!     （本域扩展，M=主包 N=次包）两种合法形态；其余形态一律解析拒；
//!   - 三态 hive：Present(串) / Empty（空串=无 SP，合法）/
//!     Missing（键缺，走回退）——回退路径派生 (0,0) 且留出处；
//!   - 联动闸：struct (wServicePackMajor/Minor) 与 hive 派生对必须逐字段
//!     相等；失配/回退违写/空键违写全部显式拒（联动一致红线）；
//!   - 往返：format_csd(m,n) → parse → (m,n) 逐例还原（0..=4 主包 ×
//!     {0,1} 次包 全谱）；
//!   - 状态机：HiveGate 两态 Open → Sealed 单向；封印后联动被拒；
//!   - 注入核：联动失配 / 畸形 CSD / 回退违写 / 空键违写 / 封印后联动
//!     五类 × 10 轮全抓；
//!   - no_std + alloc；零 unwrap/expect/panic（生产区）；禁浮点/IO/墙钟。
//! 本模块结构清单：
//!   [错误路径] SpCode 闭集（11 码）+ ALL + detail() + detail(code)
//!   [解析器]   parse_csd / format_csd（CSD 语法闭集 + 往返）
//!   [数据结构] HiveValue 三态 / SpProfile / LinkOutcome（出处留痕）
//!   [状态机]   GatePhase 两态单向 + HiveGate
//!   [账本]     LinkLedger（联动/回退/拒，守恒）
//!   [注入核]   五类违约 ×10 轮（失配/畸形/回退违写/空键违写/封印后）
//!   [收口]     golden_run + run_unx18406_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E140，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E140;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// SP 号域宽（冻结：半 WORD = 0xFF，实际发行谱 0..=4）。
pub const SP_MAX: u32 = 0xFF;
/// CSD 串字节上限（推导：szCSDVersion 128 WCHAR 容量内，UTF-8 最长
/// "Service Pack M.N" 形态 ≤ 32 字节，取 64 留裕量）。
pub const CSD_STR_MAX: usize = 64;
/// 合法主包谱上界（冻结：Windows 发行史最高 SP4，谱 0..=4）。
pub const SP_ISSUED_MAX: u16 = 4;
/// 判据轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 畸形 CSD 注入变体数。
pub const MALFORMED_VARIANTS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpCode {
    /// CSD 前缀错（非 "Service Pack " 字面）。
    CsdPrefix,
    /// CSD 包号缺（前缀后无数字）。
    CsdNoNumber,
    /// CSD 尾随杂（数字段后有额外字符）。
    CsdTrailing,
    /// SP 号越域（>0xFF）。
    SpOverflow,
    /// CSD 串超长（>64 字节）。
    CsdTooLong,
    /// 联动失配（hive 派生 ≠ struct 字段）。
    LinkMismatch,
    /// 回退违写（键缺但 struct 非 (0,0)）。
    FallbackViolation,
    /// 空键违写（空串但 struct 非 (0,0)）。
    EmptyViolation,
    /// 封印后联动被拒。
    SealedLink,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 双跑不一致。
    NotReproducible,
}

impl SpCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SpCode::CsdPrefix => "CSD 前缀错：非 \"Service Pack \" 字面",
            SpCode::CsdNoNumber => "CSD 包号缺：前缀后无数字",
            SpCode::CsdTrailing => "CSD 尾随杂：数字段后有额外字符",
            SpCode::SpOverflow => "SP 号越域：0xFF 之外",
            SpCode::CsdTooLong => "CSD 串超长：64 字节上限外",
            SpCode::LinkMismatch => "联动失配：hive 派生与 struct 不一致",
            SpCode::FallbackViolation => "回退违写：键缺但 struct 非零",
            SpCode::EmptyViolation => "空键违写：空串但 struct 非零",
            SpCode::SealedLink => "封印后联动被拒",
            SpCode::LedgerBroken => "账本守恒破坏",
            SpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SpCode; 11] = [
        SpCode::CsdPrefix,
        SpCode::CsdNoNumber,
        SpCode::CsdTrailing,
        SpCode::SpOverflow,
        SpCode::CsdTooLong,
        SpCode::LinkMismatch,
        SpCode::FallbackViolation,
        SpCode::EmptyViolation,
        SpCode::SealedLink,
        SpCode::LedgerBroken,
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
    "未知诊断码：不在 0x7E140 段闭集内"
}

// ── CSD 解析器（语法闭集） ──────────────────────────────────────
/// CSD 前缀字面（冻结，含尾随空格：完整合法形态的最长公共前缀）。
pub const CSD_PREFIX: &str = "Service Pack ";
/// CSD 核心字面（冻结：不含尾随空格的 12 字节核心——裸核后无包号
/// 报「包号缺」而非「前缀错」，使拒因粒度对齐语义）。
pub const CSD_CORE: &str = "Service Pack";

/// 解析 CSD 串 → (主包, 次包)。
/// 合法形态："Service Pack M" / "Service Pack M.N"（M,N 十进制 0..=0xFF）。
/// 其余形态显式拒（前缀错/包号缺/尾随杂/越域/超长）。
/// 判定次序：前缀错（核心字面失配）→ 包号缺（核心过但无数字）→
/// 越域 → 尾随杂。
pub fn parse_csd(s: &str) -> Result<(u16, u16), u32> {
    let b = s.as_bytes();
    if b.len() > CSD_STR_MAX {
        return Err(SpCode::CsdTooLong.code());
    }
    // 核心 12 字节先对齐（修正记录：旧式按 13 字节含空格前缀整段比对，
    // "Service Pack" 裸核被误判 CsdPrefix；语义上裸核=「前缀对但包号缺」，
    // 应报 CsdNoNumber——测试闭集 test_parse_closed_set 炸出）。
    let core = CSD_CORE.as_bytes();
    if b.len() < core.len() {
        return Err(SpCode::CsdPrefix.code());
    }
    let mut i = 0;
    while i < core.len() {
        if b[i] != core[i] {
            return Err(SpCode::CsdPrefix.code());
        }
        i += 1;
    }
    // 可选空格：裸核（无空格无数字）→ 包号缺；带空格无数字 → 包号缺。
    if i < b.len() && b[i] == b' ' {
        i += 1;
    }
    // 主包数字段（至少一位；逐位累加带溢出闸）。
    let mut major: u32 = 0;
    let mut any = false;
    while i < b.len() && b[i] >= b'0' && b[i] <= b'9' {
        major = major * 10 + (b[i] - b'0') as u32;
        if major > SP_MAX {
            return Err(SpCode::SpOverflow.code());
        }
        any = true;
        i += 1;
    }
    if !any {
        return Err(SpCode::CsdNoNumber.code());
    }
    // 次包段：可选 ".N"。
    let mut minor: u32 = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let mut any2 = false;
        while i < b.len() && b[i] >= b'0' && b[i] <= b'9' {
            minor = minor * 10 + (b[i] - b'0') as u32;
            if minor > SP_MAX {
                return Err(SpCode::SpOverflow.code());
            }
            any2 = true;
            i += 1;
        }
        if !any2 {
            return Err(SpCode::CsdNoNumber.code());
        }
    }
    // 尾随闸：必须到串尾（无空格无杂字符）。
    if i != b.len() {
        return Err(SpCode::CsdTrailing.code());
    }
    Ok((major as u16, minor as u16))
}

/// 反向格式化：(主包, 次包) → CSD 串。次包为 0 时省略 ".N"。
pub fn format_csd(major: u16, minor: u16) -> Result<String, u32> {
    if major as u32 > SP_MAX || minor as u32 > SP_MAX {
        return Err(SpCode::SpOverflow.code());
    }
    if minor == 0 {
        Ok(format!("{}{}", CSD_PREFIX, major))
    } else {
        Ok(format!("{}.{}", format!("{}{}", CSD_PREFIX, major), minor))
    }
}

/// CSD 往返一致性：format → parse 逐例还原。
pub fn csd_roundtrip(major: u16, minor: u16) -> Result<(), u32> {
    let s = format_csd(major, minor)?;
    let back = parse_csd(&s)?;
    if back != (major, minor) {
        return Err(SpCode::LinkMismatch.code());
    }
    Ok(())
}

// ── hive 三态与联动 ─────────────────────────────────────────────
/// hive 键值三态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HiveValue {
    /// 有键有值（CSD 串）。
    Present(String),
    /// 有键空串（无 SP，合法）。
    Empty,
    /// 键缺（走回退路径）。
    Missing,
}

/// 联动出处（派生来源留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpProvenance {
    /// 从 hive 串解析派生。
    HiveString,
    /// 空串派生（无 SP）。
    EmptyNoSp,
    /// 键缺回退派生。
    MissingFallback,
}

/// 联动结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkOutcome {
    pub derived: (u16, u16),
    pub provenance: SpProvenance,
}

/// 联动档案（AI-18 接口投影：hive 值 + struct 双字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpProfile {
    pub hive: HiveValue,
    pub sp_major: u16,
    pub sp_minor: u16,
}

/// 联动闸：hive 派生对与 struct 双字段逐字段相等。
/// - Present：解析 → 对拍，解析错/失配即拒；
/// - Empty：struct 必须 (0,0)，否则空键违写；
/// - Missing：struct 必须 (0,0)，否则回退违写（派生 (0,0) 留出处）。
pub fn link_check(p: &SpProfile) -> Result<LinkOutcome, u32> {
    match p.hive {
        HiveValue::Present(ref s) => {
            let d = parse_csd(s)?;
            if d != (p.sp_major, p.sp_minor) {
                return Err(SpCode::LinkMismatch.code());
            }
            Ok(LinkOutcome { derived: d, provenance: SpProvenance::HiveString })
        }
        HiveValue::Empty => {
            if p.sp_major != 0 || p.sp_minor != 0 {
                return Err(SpCode::EmptyViolation.code());
            }
            Ok(LinkOutcome {
                derived: (0, 0),
                provenance: SpProvenance::EmptyNoSp,
            })
        }
        HiveValue::Missing => {
            if p.sp_major != 0 || p.sp_minor != 0 {
                return Err(SpCode::FallbackViolation.code());
            }
            Ok(LinkOutcome {
                derived: (0, 0),
                provenance: SpProvenance::MissingFallback,
            })
        }
    }
}

// ── 账本 ────────────────────────────────────────────────────────
/// 联动账本（守恒：联动过 = hive 派生 + 空键 + 回退）。
#[derive(Clone, Copy, Debug)]
pub struct LinkLedger {
    pub tries: u32,
    pub hive_ok: u32,
    pub empty_ok: u32,
    pub fallback_ok: u32,
    pub rejected: u32,
}

impl LinkLedger {
    pub fn new() -> LinkLedger {
        LinkLedger { tries: 0, hive_ok: 0, empty_ok: 0, fallback_ok: 0, rejected: 0 }
    }
    /// 守恒：尝试 = 通过三轨合计 + 拒。
    pub fn conserve(&self) -> bool {
        self.tries == self.hive_ok + self.empty_ok + self.fallback_ok + self.rejected
    }
}

// ── 状态机（HiveGate：Open → Sealed 单向） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatePhase {
    /// 开放联动。
    Open,
    /// 已封印（只读）。
    Sealed,
}

/// hive 联动闸门：开放期收联动账，封印后一切联动被拒。
pub struct HiveGate {
    phase: GatePhase,
    pub ledger: LinkLedger,
    /// 已通过的联动结果账（按序，供导出审计）。
    pub outcomes: Vec<LinkOutcome>,
}

impl HiveGate {
    pub fn new() -> HiveGate {
        HiveGate { phase: GatePhase::Open, ledger: LinkLedger::new(), outcomes: Vec::new() }
    }
    pub fn phase(&self) -> GatePhase {
        self.phase
    }
    /// 联动一笔（Open 态；Sealed 态拒）。
    pub fn link(&mut self, p: &SpProfile) -> Result<LinkOutcome, u32> {
        if self.phase != GatePhase::Open {
            self.ledger.tries += 1;
            self.ledger.rejected += 1;
            return Err(SpCode::SealedLink.code());
        }
        self.ledger.tries += 1;
        match link_check(p) {
            Ok(o) => {
                match o.provenance {
                    SpProvenance::HiveString => self.ledger.hive_ok += 1,
                    SpProvenance::EmptyNoSp => self.ledger.empty_ok += 1,
                    SpProvenance::MissingFallback => self.ledger.fallback_ok += 1,
                }
                self.outcomes.push(o);
                Ok(o)
            }
            Err(c) => {
                self.ledger.rejected += 1;
                Err(c)
            }
        }
    }
    /// 封印（Open → Sealed 单向）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != GatePhase::Open {
            return Err(SpCode::SealedLink.code());
        }
        if !self.ledger.conserve() {
            return Err(SpCode::LedgerBroken.code());
        }
        self.phase = GatePhase::Sealed;
        Ok(())
    }
}

// ── 畸形 CSD 注入变体（10 例冻结） ──────────────────────────────
/// 10 个畸形 CSD 串（冻结表：前缀错/包号缺/尾随杂/越域/超长 各形态）。
pub const MALFORMED_CSD: [&str; MALFORMED_VARIANTS] = [
    "service pack 2",       // 前缀小写
    "ServicePack 2",        // 前缀缺空格
    "Service Pack",         // 包号缺
    "Service Pack A",       // 包号非数字
    "Service Pack 2 3",     // 尾随空格+杂
    "Service Pack 2 extra", // 尾随杂
    "Service Pack 300",     // 主包越域（>0xFF）
    "Service Pack 2.500",   // 次包越域
    "Service Pack 2.",      // 次包缺数字
    "SP2",                  // 完全错形
];

/// 畸形注入 rounds 轮：10 变体逐轮必拒且报解析类码。
pub fn malformed_inject_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rejected = 0u32;
    let mut r = 0;
    while r < rounds {
        let mut k = 0;
        while k < MALFORMED_VARIANTS {
            match parse_csd(MALFORMED_CSD[k]) {
                Err(_) => rejected += 1,
                Ok(_) => return Err(SpCode::CsdPrefix.code()),
            }
            k += 1;
        }
        r += 1;
    }
    Ok(rejected)
}

/// 联动注入 rounds 轮：失配 / 回退违写 / 空键违写 / 封印后联动轮换。
pub fn link_inject_rounds(rounds: u32) -> Result<LinkLedger, u32> {
    let mut led = LinkLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类1 联动失配：hive SP2 vs struct SP3 → 必拒。
        let p1 = SpProfile {
            hive: HiveValue::Present("Service Pack 2".to_string()),
            sp_major: 3,
            sp_minor: 0,
        };
        led.tries += 1;
        match link_check(&p1) {
            Err(_) => led.rejected += 1,
            Ok(_) => return Err(SpCode::LinkMismatch.code()),
        }
        // 类2 回退违写：键缺 + struct (3,1) → 必拒。
        let p2 = SpProfile { hive: HiveValue::Missing, sp_major: 3, sp_minor: 1 };
        led.tries += 1;
        match link_check(&p2) {
            Err(_) => led.rejected += 1,
            Ok(_) => return Err(SpCode::FallbackViolation.code()),
        }
        // 类3 空键违写：空串 + struct (1,0) → 必拒。
        let p3 = SpProfile { hive: HiveValue::Empty, sp_major: 1, sp_minor: 0 };
        led.tries += 1;
        match link_check(&p3) {
            Err(_) => led.rejected += 1,
            Ok(_) => return Err(SpCode::EmptyViolation.code()),
        }
        // 类4 封印后联动：先封印再 link → 必拒。
        let mut gate = HiveGate::new();
        let _ = gate.link(&SpProfile {
            hive: HiveValue::Present("Service Pack 4".to_string()),
            sp_major: 4,
            sp_minor: 0,
        });
        if gate.seal().is_err() {
            return Err(SpCode::SealedLink.code());
        }
        led.tries += 1;
        match gate.link(&SpProfile {
            hive: HiveValue::Present("Service Pack 2".to_string()),
            sp_major: 2,
            sp_minor: 0,
        }) {
            Err(_) => led.rejected += 1,
            Ok(_) => return Err(SpCode::SealedLink.code()),
        }
        r += 1;
    }
    if !led.conserve() {
        return Err(SpCode::LedgerBroken.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链一次（确定性）：SP0..SP4 全谱联动 + 回退 + 空键 → 封印。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut gate = HiveGate::new();
    // 主谱：SP0..SP4（hive 串 ↔ struct 双字段一致）。
    let mut m: u16 = 0;
    while m <= SP_ISSUED_MAX {
        let s = format_csd(m, 0)?;
        let p = SpProfile { hive: HiveValue::Present(s), sp_major: m, sp_minor: 0 };
        let o = gate.link(&p)?;
        if o.provenance != SpProvenance::HiveString || o.derived != (m, 0) {
            return Err(SpCode::LinkMismatch.code());
        }
        m += 1;
    }
    // 次包扩展谱：SP2.1（域扩展形态，派生 (2,1)）。
    let s21 = format_csd(2, 1)?;
    let p21 = SpProfile { hive: HiveValue::Present(s21), sp_major: 2, sp_minor: 1 };
    let o21 = gate.link(&p21)?;
    if o21.derived != (2, 1) {
        return Err(SpCode::LinkMismatch.code());
    }
    // 空键（无 SP 合法）。
    let pe = SpProfile { hive: HiveValue::Empty, sp_major: 0, sp_minor: 0 };
    let oe = gate.link(&pe)?;
    if oe.provenance != SpProvenance::EmptyNoSp {
        return Err(SpCode::EmptyViolation.code());
    }
    // 键缺回退（派生 (0,0) 留出处）。
    let pm = SpProfile { hive: HiveValue::Missing, sp_major: 0, sp_minor: 0 };
    let om = gate.link(&pm)?;
    if om.provenance != SpProvenance::MissingFallback || om.derived != (0, 0) {
        return Err(SpCode::FallbackViolation.code());
    }
    // 往返全谱：0..=4 主包 × {0,1} 次包。
    let mut rm: u16 = 0;
    while rm <= SP_ISSUED_MAX {
        csd_roundtrip(rm, 0)?;
        csd_roundtrip(rm, 1)?;
        rm += 1;
    }
    // 畸形注入 10 变体 × 10 轮全拒（100）。
    let mal = malformed_inject_rounds(JUDGE_ROUNDS)?;
    if mal != MALFORMED_VARIANTS as u32 * JUDGE_ROUNDS {
        return Err(SpCode::CsdPrefix.code());
    }
    // 联动注入四类 × 10 轮全拒（40）。
    let led = link_inject_rounds(JUDGE_ROUNDS)?;
    if led.rejected != 4 * JUDGE_ROUNDS {
        return Err(SpCode::LinkMismatch.code());
    }
    // 封印 + 账守恒：通过 8 笔（5 主谱 + 1 次包 + 1 空键 + 1 回退）。
    if !gate.ledger.conserve() || gate.ledger.hive_ok != 6 {
        return Err(SpCode::LedgerBroken.code());
    }
    gate.seal()?;
    if gate.phase() != GatePhase::Sealed {
        return Err(SpCode::SealedLink.code());
    }
    Ok((
        gate.ledger.hive_ok,
        gate.ledger.fallback_ok,
        mal,
        gate.ledger.tries,
    ))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SpCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
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

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：SP3 联动正链成功且出处为 HiveString。
pub fn probe_seq_a() -> Result<(), u32> {
    let s = format_csd(3, 0)?;
    let p = SpProfile { hive: HiveValue::Present(s), sp_major: 3, sp_minor: 0 };
    let o = link_check(&p)?;
    if o.provenance != SpProvenance::HiveString || o.derived != (3, 0) {
        return Err(SpCode::LinkMismatch.code());
    }
    Ok(())
}

/// 探针 B：畸形串 "Service Pack 2 3" → 必拒（尾随杂）。
pub fn probe_seq_b() -> Result<(), u32> {
    match parse_csd("Service Pack 2 3") {
        Err(_) => Ok(()),
        Ok(_) => Err(SpCode::CsdTrailing.code()),
    }
}

/// 探针 C：键缺回退派生 (0,0) 且出处为 MissingFallback。
pub fn probe_seq_c() -> Result<(), u32> {
    let p = SpProfile { hive: HiveValue::Missing, sp_major: 0, sp_minor: 0 };
    let o = link_check(&p)?;
    if o.provenance != SpProvenance::MissingFallback || o.derived != (0, 0) {
        return Err(SpCode::FallbackViolation.code());
    }
    Ok(())
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 判据集（8–20 条；动态明细 ≤8）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. SP0..SP4 主谱联动一致（hive 串 ↔ struct 双字段逐例相等）。
    let g = golden_run();
    let main_ok = match &g {
        Ok((hive_ok, _fb, _mal, _tries)) => *hive_ok == 6,
        Err(_) => false,
    };
    v.push(("J1-SP0..4主谱联动", main_ok, format!("{:?}", g)));

    // 2. 次包扩展谱联动（SP2.1 派生 (2,1)）。
    let minor_ok = match parse_csd("Service Pack 2.1") {
        Ok(d) => d == (2, 1),
        Err(_) => false,
    };
    v.push(("J1-次包扩展谱SP2.1", minor_ok, format!("{:?}", minor_ok)));

    // 3. 往返全谱（0..=4 × {0,1} 逐例还原）。
    let rt_ok = {
        let mut ok = true;
        let mut m: u16 = 0;
        while m <= SP_ISSUED_MAX && ok {
            if csd_roundtrip(m, 0).is_err() || csd_roundtrip(m, 1).is_err() {
                ok = false;
            }
            m += 1;
        }
        ok
    };
    v.push(("J1-往返全谱10例", rt_ok, String::from("0..=4 × {0,1}")));

    // 4. 畸形注入 10 变体 × 10 轮 = 100 全拒。
    let mal = malformed_inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-畸形CSD注入100/100",
        match &mal {
            Ok(n) => *n == 100,
            Err(_) => false,
        },
        format!("{:?}", mal),
    ));

    // 5~8. 联动注入四类各 10/10（失配/回退违写/空键违写/封印后）。
    let led = link_inject_rounds(JUDGE_ROUNDS);
    let rej4 = match led.as_ref() {
        Ok(l) => l.rejected,
        Err(_) => 0,
    };
    v.push((
        "J1-联动注入4类40/40",
        match &led {
            Ok(l) => l.rejected == 4 * JUDGE_ROUNDS,
            Err(_) => false,
        },
        format!("rej={}", rej4),
    ));
    v.push((
        "J1-联动注入守恒",
        match &led {
            Ok(l) => l.conserve() && l.tries == 4 * JUDGE_ROUNDS && l.hive_ok == 0,
            Err(_) => false,
        },
        String::from("40 试 40 拒 收 0"),
    ));

    // 9. 探针三序（正链 / 畸形拒 / 回退出处）。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_ok();
    let pc = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b拒={} c={}", pa, pb, pc),
    ));

    // 10. 回退路径覆盖（键缺派生 (0,0) 且留 MissingFallback 出处）。
    let fb_ok = match &g {
        Ok((_h, fb, _m, _t)) => *fb == 1,
        Err(_) => false,
    };
    v.push(("J1-hive缺键回退覆盖", fb_ok, String::from("Missing→(0,0) 留痕")));

    // 11. 空键合法（无 SP：Empty→(0,0) EmptyNoSp 出处）。
    let pe = link_check(&SpProfile { hive: HiveValue::Empty, sp_major: 0, sp_minor: 0 });
    let empty_ok = match pe {
        Ok(o) => o.provenance == SpProvenance::EmptyNoSp,
        Err(_) => false,
    };
    v.push(("J1-空键合法无SP", empty_ok, String::from("Empty→(0,0)")));

    // 12. 封印闸（Open→Sealed 单向；封印后联动 SealedLink 拒）。
    let mut gate2 = HiveGate::new();
    let seal_ok = {
        let mut ok = gate2.seal().is_ok();
        match gate2.link(&SpProfile {
            hive: HiveValue::Present("Service Pack 1".to_string()),
            sp_major: 1,
            sp_minor: 0,
        }) {
            Err(c) => {
                if c != SpCode::SealedLink.code() {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        ok
    };
    v.push(("J1-封印后联动拒", seal_ok, String::from("SealedLink 码")));

    // 13. 双跑一致。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 15. 诊断码闭集（11 码全非空中文明细）。
    let all_ok = {
        let mut ok = SpCode::ALL.len() == 11;
        let mut i = 0;
        while i < SpCode::ALL.len() {
            if SpCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集11码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 15 条；前 8 条动态明细）。
pub fn run_unx18406_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18406 服务包与 CSD 联动");
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
        let set = run_unx18406_checks();
        assert!(set.all_passed(), "UNX-F18406 判据集应全绿");
    }

    /// 正常流：SP3 联动正链成功。
    #[test]
    fn test_probe_seq_a() {
        assert!(probe_seq_a().is_ok());
    }

    /// 畸形注入：10 变体 × 10 轮 = 100 全拒。
    #[test]
    fn test_malformed_injection() {
        assert_eq!(malformed_inject_rounds(JUDGE_ROUNDS), Ok(100));
    }

    /// 联动注入：失配/回退违写/空键违写/封印后 各 10/10。
    #[test]
    fn test_link_injection() {
        match link_inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.rejected, 40);
                assert!(l.conserve());
                assert_eq!(l.hive_ok, 0);
            }
            Err(_) => assert!(false, "全拒时不应报错"),
        }
    }

    /// 解析闭集：合法两形态过、典型畸形逐例拒。
    #[test]
    fn test_parse_closed_set() {
        assert_eq!(parse_csd("Service Pack 2"), Ok((2, 0)));
        assert_eq!(parse_csd("Service Pack 2.1"), Ok((2, 1)));
        assert_eq!(parse_csd("Service Pack 0"), Ok((0, 0)));
        match parse_csd("Service Pack") {
            Err(c) => assert_eq!(c, SpCode::CsdNoNumber.code()),
            Ok(_) => assert!(false, "包号缺必须拒"),
        }
        match parse_csd("service pack 2") {
            Err(c) => assert_eq!(c, SpCode::CsdPrefix.code()),
            Ok(_) => assert!(false, "前缀小写必须拒"),
        }
    }

    /// 往返：format→parse 全谱还原。
    #[test]
    fn test_roundtrip() {
        let mut m: u16 = 0;
        while m <= SP_ISSUED_MAX {
            assert!(csd_roundtrip(m, 0).is_ok());
            assert!(csd_roundtrip(m, 1).is_ok());
            m += 1;
        }
        assert!(csd_roundtrip(SP_MAX as u16, SP_MAX as u16).is_ok());
    }

    /// 越域：SP 0x100 解析拒（SpOverflow）。
    #[test]
    fn test_sp_overflow() {
        match parse_csd("Service Pack 300") {
            Err(c) => assert_eq!(c, SpCode::SpOverflow.code()),
            Ok(_) => assert!(false, "300 必须拒"),
        }
    }

    /// 回退路径：键缺派生 (0,0) 留出处；非零 struct 回退违写拒。
    #[test]
    fn test_fallback_path() {
        assert!(probe_seq_c().is_ok());
        let bad = SpProfile { hive: HiveValue::Missing, sp_major: 2, sp_minor: 0 };
        match link_check(&bad) {
            Err(c) => assert_eq!(c, SpCode::FallbackViolation.code()),
            Ok(_) => assert!(false, "回退违写必须拒"),
        }
    }

    /// 封印：Open→Sealed 单向，封印后联动拒。
    #[test]
    fn test_seal_gate() {
        let mut gate = HiveGate::new();
        assert!(gate.seal().is_ok());
        match gate.link(&SpProfile { hive: HiveValue::Empty, sp_major: 0, sp_minor: 0 }) {
            Err(c) => assert_eq!(c, SpCode::SealedLink.code()),
            Ok(_) => assert!(false, "封印后联动必须拒"),
        }
    }

    /// 复现红线：golden_run 双跑一致。
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
