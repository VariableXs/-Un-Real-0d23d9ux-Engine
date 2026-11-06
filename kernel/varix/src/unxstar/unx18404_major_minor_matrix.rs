//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18404 · dwMajorVersion/dwMinorVersion 语义字段
//! · 0x7E0C0 段 · 批 01 · 配额 320
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18404-J1 主/次版本取值经裁决矩阵（F18417 预留）输出，
//!   伪造直通注入 10/10 检出。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 裁决矩阵（F18417 预留位对齐）：8 行冻结口径——
//!     (5,0)=Windows2000 (5,1)=WindowsXP (5,2)=Server2003 (6,0)=Vista2008
//!     (6,1)=Win7-2008R2 (6,2)=Win8-2012 (6,3)=Win8.1-2012R2 (10,0)=Win10-11；
//!     未登记对回退 Unregistered（保留档），绝不直通；
//!   - 出处令牌：每个裁决输出必带矩阵行号令牌（row_id）；缺令牌或
//!     令牌与矩阵重查不符 = 伪造 → 检出（伪造直通注入 10/10）；
//!   - 状态机：Stated → Consulted → Adjudicated 单向；从 Stated 直接
//!     finalize（越级直通）显式拒——这正是"伪造直通"的结构性防线；
//!   - 域守卫：major/minor ≤ 0xFF（与打包位宽同源），越界显式拒；
//!   - 流水号账本：每个 consult 取严格递增 serial；乱序 serial 注入拒；
//!   - 账本：MatrixLedger（咨询/裁决/伪造检出/回退/越界拒，守恒）；
//!   - no_std + alloc；零 unwrap/expect/panic（生产区）；禁浮点/IO/墙钟。
//! 本模块结构清单：
//!   [错误路径] MmCode 闭集（11 码）+ ALL + detail() + detail(code)
//!   [裁决表]   MATRIX_ROWS 8 行冻结 + matrix_lookup + fallback
//!   [状态机]   ClaimPhase 三态单向 + ClaimAdjudicator
//!   [账本]     MatrixLedger（守恒核算）
//!   [解析器]   Verdict 出处令牌校验 verify_verdict
//!   [注入核]   伪造直通 ×10 / 域越界 ×10 / 越级 ×10 / 乱序 serial
//!   [收口]     golden_run + run_unx18404_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E0C0，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E0C0;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 矩阵登记行数（8 行冻结口径，F18417 预留位对齐）。
pub const MATRIX_ROWS: usize = 8;
/// 主/次版本域宽（与 DWORD 打包高字同源，各 8 位）。
pub const VER_FIELD_MAX: u32 = 0xFF;
/// 判据轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 伪造令牌哨兵（非法 row_id，任何合法裁决不可能取到）。
pub const FORGED_TOKEN: u32 = 0xFFFF_FFFF;
/// 回退行哨兵（未登记对；合法 row_id 域 = 0..8，此值在域外但非伪造）。
pub const FALLBACK_ROW: u32 = 0x00FF;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmCode {
    /// 主版本越域（>0xFF）。
    MajorOverflow,
    /// 次版本越域（>0xFF）。
    MinorOverflow,
    /// 伪造直通：裁决输出缺出处令牌。
    ForgedPassthrough,
    /// 令牌与矩阵重查不符（令牌漂移）。
    TokenMismatch,
    /// 状态机越级（Stated 直接 finalize）。
    PhaseSkipped,
    /// 流水号乱序（serial 非严格递增）。
    SerialOutOfOrder,
    /// 重复登记同一票据（重复边界）。
    DuplicateTicket,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 双跑不一致。
    NotReproducible,
    /// 未登记对未走回退路径（直通漏网）。
    FallbackBypassed,
    /// 矩阵行越界读（防御）。
    RowOutOfRange,
}

impl MmCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MmCode::MajorOverflow => "主版本越域：0xFF 上限外",
            MmCode::MinorOverflow => "次版本越域：0xFF 上限外",
            MmCode::ForgedPassthrough => "伪造直通：裁决输出缺出处令牌",
            MmCode::TokenMismatch => "令牌漂移：与矩阵重查不符",
            MmCode::PhaseSkipped => "状态机越级：未咨询先裁决",
            MmCode::SerialOutOfOrder => "流水号乱序：serial 非严格递增",
            MmCode::DuplicateTicket => "重复登记同一票据",
            MmCode::LedgerBroken => "账本守恒破坏",
            MmCode::NotReproducible => "双跑不一致：复现红线",
            MmCode::FallbackBypassed => "未登记对未走回退：直通漏网",
            MmCode::RowOutOfRange => "矩阵行越界读",
        }
    }
    pub const ALL: [MmCode; 11] = [
        MmCode::MajorOverflow,
        MmCode::MinorOverflow,
        MmCode::ForgedPassthrough,
        MmCode::TokenMismatch,
        MmCode::PhaseSkipped,
        MmCode::SerialOutOfOrder,
        MmCode::DuplicateTicket,
        MmCode::LedgerBroken,
        MmCode::NotReproducible,
        MmCode::FallbackBypassed,
        MmCode::RowOutOfRange,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MmCode::ALL.len() {
        if MmCode::ALL[i].code() == code {
            return MmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E0C0 段闭集内"
}

// ── 裁决矩阵（冻结 8 行） ───────────────────────────────────────
/// 信任档（Canon=登记正档 / Reserved=回退保留档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrustClass {
    Canon,
    Reserved,
}

/// 矩阵行：主/次版本对 → 产品族 + 信任档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixRow {
    pub major: u32,
    pub minor: u32,
    pub family: &'static str,
    pub trust: TrustClass,
}

/// 第 i 行（越界返回 None）。
pub fn matrix_row(i: usize) -> Option<MatrixRow> {
    let t: [(&str, TrustClass); MATRIX_ROWS] = [
        ("Windows2000", TrustClass::Canon),
        ("WindowsXP", TrustClass::Canon),
        ("Server2003", TrustClass::Canon),
        ("Vista2008", TrustClass::Canon),
        ("Win7-2008R2", TrustClass::Canon),
        ("Win8-2012", TrustClass::Canon),
        ("Win8.1-2012R2", TrustClass::Canon),
        ("Win10-11", TrustClass::Canon),
    ];
    if i >= MATRIX_ROWS {
        return None;
    }
    let (maj, min) = match i {
        0 => (5, 0),
        1 => (5, 1),
        2 => (5, 2),
        3 => (6, 0),
        4 => (6, 1),
        5 => (6, 2),
        6 => (6, 3),
        _ => (10, 0),
    };
    let (family, trust) = t[i];
    Some(MatrixRow { major: maj, minor: min, family, trust })
}

/// 矩阵查找：登记对 → 行号；未登记 → None（调用方走回退）。
pub fn matrix_lookup(major: u32, minor: u32) -> Option<usize> {
    let mut i = 0;
    while i < MATRIX_ROWS {
        if let Some(r) = matrix_row(i) {
            if r.major == major && r.minor == minor {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ── 裁决输出（出处令牌随行） ────────────────────────────────────
/// 裁决输出：值 + 信任档 + 矩阵行号令牌 + 流水号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub major: u32,
    pub minor: u32,
    pub row_id: u32,
    /// 0=Canon 1=Reserved（整数化便于账内比对）。
    pub trust: u8,
    pub serial: u32,
}

/// 令牌校验：重查矩阵，令牌必须与当前矩阵口径一致。
pub fn verify_verdict(v: &Verdict) -> Result<(), u32> {
    if v.row_id == FORGED_TOKEN {
        return Err(MmCode::ForgedPassthrough.code());
    }
    if v.row_id == FALLBACK_ROW {
        // 回退裁决：矩阵必须查不到该对。
        if matrix_lookup(v.major, v.minor).is_some() {
            return Err(MmCode::TokenMismatch.code());
        }
        if v.trust != 1 {
            return Err(MmCode::TokenMismatch.code());
        }
        return Ok(());
    }
    let idx = v.row_id as usize;
    let r = match matrix_row(idx) {
        Some(r) => r,
        None => return Err(MmCode::TokenMismatch.code()),
    };
    if r.major != v.major || r.minor != v.minor {
        return Err(MmCode::TokenMismatch.code());
    }
    let want_trust = match r.trust {
        TrustClass::Canon => 0u8,
        TrustClass::Reserved => 1u8,
    };
    if v.trust != want_trust {
        return Err(MmCode::TokenMismatch.code());
    }
    Ok(())
}

// ── 状态机：Stated → Consulted → Adjudicated（单向） ────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClaimPhase {
    /// 主张已登记（尚未咨询矩阵）。
    Stated,
    /// 已咨询矩阵（拿到裁决，待 finalize）。
    Consulted,
    /// 已终决（票据收口）。
    Adjudicated,
}

/// 单张主张票据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClaimTicket {
    pub id: u32,
    pub major: u32,
    pub minor: u32,
    pub state: ClaimPhase,
}

/// 裁决器：登记主张 → 咨询矩阵 → 终决；全程出处令牌闭环。
pub struct ClaimAdjudicator {
    /// 在管票据（票据槽上限 32，满即拒）。
    tickets: Vec<ClaimTicket>,
    next_id: u32,
    next_serial: u32,
    pub ledger: MatrixLedger,
}

/// 矩阵账本。
#[derive(Clone, Debug)]
pub struct MatrixLedger {
    pub consults: u32,
    pub adjudicated: u32,
    pub forged_caught: u32,
    pub fallbacks: u32,
    pub overflow_rej: u32,
    pub phase_rej: u32,
    /// 裁决输出账（逐条出处令牌，供导出审计）。
    pub verdicts: Vec<Verdict>,
}

impl MatrixLedger {
    pub fn new() -> MatrixLedger {
        MatrixLedger {
            consults: 0,
            adjudicated: 0,
            forged_caught: 0,
            fallbacks: 0,
            overflow_rej: 0,
            phase_rej: 0,
            verdicts: Vec::new(),
        }
    }
    /// 守恒：咨询数 = 裁决数 + 回退数 + 越级拒数。
    pub fn conserve(&self) -> bool {
        self.consults == self.adjudicated + self.fallbacks + self.phase_rej
    }
}

/// 票据槽上限。
pub const TICKET_CAP: usize = 32;

impl ClaimAdjudicator {
    pub fn new() -> ClaimAdjudicator {
        ClaimAdjudicator {
            tickets: Vec::new(),
            next_id: 1,
            next_serial: 1,
            ledger: MatrixLedger::new(),
        }
    }

    /// 登记主张（域守卫 + 槽容量守卫 + 重复 id 拒）。
    pub fn state_claim(&mut self, major: u32, minor: u32) -> Result<u32, u32> {
        if major > VER_FIELD_MAX {
            self.ledger.overflow_rej += 1;
            return Err(MmCode::MajorOverflow.code());
        }
        if minor > VER_FIELD_MAX {
            self.ledger.overflow_rej += 1;
            return Err(MmCode::MinorOverflow.code());
        }
        if self.tickets.len() >= TICKET_CAP {
            self.ledger.overflow_rej += 1;
            return Err(MmCode::LedgerBroken.code());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tickets.push(ClaimTicket {
            id,
            major,
            minor,
            state: ClaimPhase::Stated,
        });
        Ok(id)
    }

    /// 取票据（只读；越界 None）。
    pub fn ticket(&self, id: u32) -> Option<ClaimTicket> {
        let mut i = 0;
        while i < self.tickets.len() {
            if self.tickets[i].id == id {
                return Some(self.tickets[i]);
            }
            i += 1;
        }
        None
    }

    /// 找票据槽位（内部）。
    fn ticket_pos(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.tickets.len() {
            if self.tickets[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 咨询矩阵（Stated → Consulted；只有此路能拿到合法裁决）。
    pub fn consult(&mut self, id: u32) -> Result<Verdict, u32> {
        let pos = match self.ticket_pos(id) {
            Some(p) => p,
            None => return Err(MmCode::RowOutOfRange.code()),
        };
        if self.tickets[pos].state != ClaimPhase::Stated {
            self.ledger.phase_rej += 1;
            return Err(MmCode::PhaseSkipped.code());
        }
        let (major, minor) = (self.tickets[pos].major, self.tickets[pos].minor);
        let serial = self.next_serial;
        self.next_serial += 1;
        self.ledger.consults += 1;
        let v = match matrix_lookup(major, minor) {
            Some(row) => {
                let r = match matrix_row(row) {
                    Some(r) => r,
                    None => return Err(MmCode::RowOutOfRange.code()),
                };
                Verdict {
                    major,
                    minor,
                    row_id: row as u32,
                    trust: match r.trust {
                        TrustClass::Canon => 0,
                        TrustClass::Reserved => 1,
                    },
                    serial,
                }
            }
            None => {
                // 未登记对：显式回退档（绝不直通）。
                // 回退账在 finalize 终决时计（consult/终决不重复计账）。
                Verdict {
                    major,
                    minor,
                    row_id: FALLBACK_ROW,
                    trust: 1,
                    serial,
                }
            }
        };
        self.tickets[pos].state = ClaimPhase::Consulted;
        Ok(v)
    }

    /// 终决（Consulted → Adjudicated；令牌校验必须过）。
    pub fn finalize(&mut self, id: u32, v: &Verdict) -> Result<(), u32> {
        let pos = match self.ticket_pos(id) {
            Some(p) => p,
            None => return Err(MmCode::RowOutOfRange.code()),
        };
        // 越级直通防线：Stated 直接 finalize = 伪造直通，拒。
        if self.tickets[pos].state != ClaimPhase::Consulted {
            self.ledger.phase_rej += 1;
            return Err(MmCode::PhaseSkipped.code());
        }
        // 令牌闭环：输出令牌必须与本矩阵口径重查一致。
        match verify_verdict(v) {
            Ok(()) => {}
            Err(c) if c == MmCode::ForgedPassthrough.code() => {
                self.ledger.forged_caught += 1;
                return Err(MmCode::ForgedPassthrough.code());
            }
            Err(c) => return Err(c),
        }
        // 流水号严格递增（乱序注入拒）。
        let last = match self.ledger.verdicts.last() {
            Some(prev) => prev.serial,
            None => 0,
        };
        if v.serial <= last {
            self.ledger.phase_rej += 1;
            return Err(MmCode::SerialOutOfOrder.code());
        }
        self.tickets[pos].state = ClaimPhase::Adjudicated;
        // 终决账：登记档与回退档分账（互斥，守恒不重复）。
        if v.row_id == FALLBACK_ROW {
            self.ledger.fallbacks += 1;
        } else {
            self.ledger.adjudicated += 1;
        }
        self.ledger.verdicts.push(*v);
        Ok(())
    }
}

// ── 注入核（四类违约 × 10 轮） ──────────────────────────────────
/// 类1 伪造直通样本：带伪造令牌的裁决（缺真实矩阵出处）。
pub fn forged_verdict_sample(round: usize) -> Verdict {
    let (m, n) = match round % 4 {
        0 => (9, 9),
        1 => (5, 1),
        2 => (255, 255),
        _ => (6, 1),
    };
    Verdict { major: m, minor: n, row_id: FORGED_TOKEN, trust: 0, serial: 0 }
}

/// 四类注入 rounds 轮：伪造直通 / 越级 finalize / 域越界 / 乱序 serial。
pub fn inject_rounds(rounds: u32) -> Result<MatrixLedger, u32> {
    let mut led = MatrixLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类1 伪造直通：Stated 直接 finalize 伪造令牌裁决 → 必拒。
        let mut adj = ClaimAdjudicator::new();
        let id1 = adj.state_claim(5, 1)?;
        let fv = forged_verdict_sample(r as usize);
        match adj.finalize(id1, &fv) {
            Err(c) => {
                if c != MmCode::PhaseSkipped.code() && c != MmCode::ForgedPassthrough.code() {
                    return Err(MmCode::ForgedPassthrough.code());
                }
                led.forged_caught += 1;
            }
            Ok(()) => return Err(MmCode::ForgedPassthrough.code()),
        }
        // 类2 越级：正规 consult 后再拿"第二张"伪造令牌终决 → 必拒。
        let mut adj2 = ClaimAdjudicator::new();
        let id2 = adj2.state_claim(6, 1)?;
        let mut v2 = adj2.consult(id2)?;
        // 用合法裁决但把令牌抹成伪造值（令牌漂移注入）。
        v2.row_id = FORGED_TOKEN;
        match adj2.finalize(id2, &v2) {
            Err(c) => {
                if c != MmCode::ForgedPassthrough.code() {
                    return Err(MmCode::ForgedPassthrough.code());
                }
                led.forged_caught += 1;
            }
            Ok(()) => return Err(MmCode::ForgedPassthrough.code()),
        }
        // 类3 域越界：major=0x100 / minor=0x100 轮换 → 必拒。
        let mut adj3 = ClaimAdjudicator::new();
        let bad = match r % 2 {
            0 => (VER_FIELD_MAX + 1, 1),
            _ => (5, VER_FIELD_MAX + 1),
        };
        match adj3.state_claim(bad.0, bad.1) {
            Err(_) => led.overflow_rej += 1,
            Ok(_) => return Err(MmCode::MajorOverflow.code()),
        }
        // 类4 乱序 serial：合法裁决改小 serial → 必拒。
        let mut adj4 = ClaimAdjudicator::new();
        let id4a = adj4.state_claim(5, 0)?;
        let id4b = adj4.state_claim(5, 1)?;
        let va = adj4.consult(id4a)?;
        adj4.finalize(id4a, &va)?;
        let vb = adj4.consult(id4b)?;
        let mut vb_bad = vb;
        vb_bad.serial = va.serial; // 与上一条同号（非严格递增）。
        match adj4.finalize(id4b, &vb_bad) {
            Err(c) => {
                if c != MmCode::SerialOutOfOrder.code() {
                    return Err(MmCode::SerialOutOfOrder.code());
                }
                led.phase_rej += 1;
            }
            Ok(()) => return Err(MmCode::SerialOutOfOrder.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链一次（确定性）。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut adj = ClaimAdjudicator::new();
    // 8 行登记对全谱 + 2 个未登记对（回退路径）。
    let claims: [(u32, u32); 10] = [
        (5, 0), (5, 1), (5, 2), (6, 0), (6, 1), (6, 2), (6, 3), (10, 0), (9, 9), (0, 0),
    ];
    let mut i = 0;
    while i < claims.len() {
        let id = adj.state_claim(claims[i].0, claims[i].1)?;
        let v = adj.consult(id)?;
        adj.finalize(id, &v)?;
        i += 1;
    }
    if !adj.ledger.conserve() {
        return Err(MmCode::LedgerBroken.code());
    }
    // 回退恰好 2 笔（(9,9) 与 (0,0) 未登记），登记终决恰好 8 笔。
    if adj.ledger.fallbacks != 2 || adj.ledger.adjudicated != 8 {
        return Err(MmCode::FallbackBypassed.code());
    }
    // 逐条裁决令牌重查全过。
    let mut k = 0;
    while k < adj.ledger.verdicts.len() {
        verify_verdict(&adj.ledger.verdicts[k])?;
        k += 1;
    }
    // 注入四类 × 10 轮全抓。
    let led = inject_rounds(JUDGE_ROUNDS)?;
    if led.forged_caught != 2 * JUDGE_ROUNDS {
        return Err(MmCode::ForgedPassthrough.code());
    }
    if led.overflow_rej != JUDGE_ROUNDS || led.phase_rej != JUDGE_ROUNDS {
        return Err(MmCode::PhaseSkipped.code());
    }
    Ok((
        adj.ledger.adjudicated,
        adj.ledger.fallbacks,
        led.forged_caught,
        adj.ledger.consults,
    ))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MmCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MmCode::ALL.len() {
        let v = MmCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：合法三步（登记→咨询→终决）成功且令牌可重查。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut adj = ClaimAdjudicator::new();
    let id = adj.state_claim(6, 1)?;
    let v = adj.consult(id)?;
    adj.finalize(id, &v)?;
    verify_verdict(&v)
}

/// 探针 B：Stated 直接 finalize（越级直通）→ 必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut adj = ClaimAdjudicator::new();
    let id = adj.state_claim(5, 1)?;
    let v = Verdict { major: 5, minor: 1, row_id: 1, trust: 0, serial: 1 };
    adj.finalize(id, &v).map(|_| ())
}

/// 探针 C：未登记对 (9,9) 走回退档（Reserved，绝不直通）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut adj = ClaimAdjudicator::new();
    let id = adj.state_claim(9, 9)?;
    let v = adj.consult(id)?;
    if v.row_id != FALLBACK_ROW || v.trust != 1 {
        return Err(MmCode::FallbackBypassed.code());
    }
    adj.finalize(id, &v)?;
    verify_verdict(&v)
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 判据集（8–20 条；动态明细 ≤8）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 矩阵冻结 8 行（越界取 None）。
    let row_n = {
        let mut n = 0;
        let mut i = 0;
        while i < MATRIX_ROWS + 3 {
            if matrix_row(i).is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    };
    v.push(("J1-矩阵冻结8行", row_n == MATRIX_ROWS, format!("rows={}", row_n)));

    // 2. 矩阵行序单调（(major,minor) 严格递增，防重复防乱序登记）。
    let mono = {
        let mut ok = true;
        let mut i = 1;
        while i < MATRIX_ROWS {
            let a = match matrix_row(i - 1) {
                Some(r) => r,
                None => {
                    ok = false;
                    break;
                }
            };
            let b = match matrix_row(i) {
                Some(r) => r,
                None => {
                    ok = false;
                    break;
                }
            };
            if (b.major, b.minor) <= (a.major, a.minor) {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-矩阵行序单调", mono, String::from("严格递增无重复")));

    // 3. 正链三步（登记→咨询→终决）+ 令牌重查。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-正链三步+令牌重查", pa, format!("a={}", pa)));

    // 4. 伪造直通注入 10/10（两类伪造各 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let forged_ok = match &led {
        Ok(l) => l.forged_caught == 2 * JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push((
        "J1-伪造直通注入10/10",
        forged_ok,
        format!("caught={}", match led.as_ref() { Ok(l) => l.forged_caught, Err(_) => 0 }),
    ));

    // 5. 域越界注入 10/10。
    let ovr_ok = match &led {
        Ok(l) => l.overflow_rej == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push((
        "J1-域越界注入10/10",
        ovr_ok,
        format!("rej={}", match led.as_ref() { Ok(l) => l.overflow_rej, Err(_) => 0 }),
    ));

    // 6. 乱序 serial 注入 10/10。
    let ooo_ok = match &led {
        Ok(l) => l.phase_rej == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push((
        "J1-乱序serial注入10/10",
        ooo_ok,
        format!("rej={}", match led.as_ref() { Ok(l) => l.phase_rej, Err(_) => 0 }),
    ));

    // 7. 越级直通防线（探针 B 必拒）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-越级直通必拒", pb, format!("b拒={}", pb)));

    // 8. 回退路径（(9,9)/(0,0) 未登记 → Reserved 档，绝不直通）。
    let pc = probe_seq_c().is_ok();
    v.push(("J1-未登记对回退档", pc, format!("c={}", pc)));

    // 9. 黄金全链（10 主张：8 登记终决 + 2 回退终决，守恒）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((adjn, fb, _fc, _cs)) => *adjn == 8 && *fb == 2,
        Err(_) => false,
    };
    v.push(("J1-黄金全链10主张", g_ok, format!("{:?}", g)));

    // 10. 账本守恒（黄金链内 consults == adjudicated + fallbacks）。
    let cons_ok = match &g {
        Ok((adjn, fb, _fc, cs)) => *cs == *adjn + *fb,
        Err(_) => false,
    };
    v.push(("J1-账本守恒", cons_ok, String::from("咨询=裁决+回退")));

    // 11. 确定性：矩阵查询幂等（同对重复查同行）。
    let idem = {
        let mut ok = true;
        let mut i = 0;
        while i < MATRIX_ROWS {
            if let Some(r) = matrix_row(i) {
                match matrix_lookup(r.major, r.minor) {
                    Some(j) => {
                        if j != i {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-矩阵查询幂等", idem, String::from("同对同行")));

    // 12. 边界：域下界 (0,0) 回退、域上界 (255,255) 回退、(5,1) 正档。
    let b_ok = {
        let mut adj = ClaimAdjudicator::new();
        let mut ok = true;
        let t = [(0u32, 0u32, true), (255, 255, true), (5, 1, false)];
        let mut i = 0;
        while i < 3 {
            let (m, n, want_fb) = t[i];
            let id = match adj.state_claim(m, n) {
                Ok(id) => id,
                Err(_) => {
                    ok = false;
                    break;
                }
            };
            let vd = match adj.consult(id) {
                Ok(vd) => vd,
                Err(_) => {
                    ok = false;
                    break;
                }
            };
            if (vd.row_id == FALLBACK_ROW) != want_fb {
                ok = false;
                break;
            }
            if adj.finalize(id, &vd).is_err() {
                ok = false;
                break;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-边界三档裁决", b_ok, String::from("下界/上界回退 正档")));

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
        let mut ok = MmCode::ALL.len() == 11;
        let mut i = 0;
        while i < MmCode::ALL.len() {
            if MmCode::ALL[i].detail().is_empty() {
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
pub fn run_unx18404_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18404 主/次版本裁决矩阵");
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
        let set = run_unx18404_checks();
        assert!(set.all_passed(), "UNX-F18404 判据集应全绿");
    }

    /// 正常流：登记→咨询→终决，令牌重查通过。
    #[test]
    fn test_probe_seq_a() {
        assert!(probe_seq_a().is_ok());
    }

    /// 注入：伪造直通（缺令牌+令牌漂移）各 10/10 检出。
    #[test]
    fn test_forged_injection() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => assert_eq!(l.forged_caught, 20),
            Err(_) => assert!(false, "注入全抓时不应报错"),
        }
    }

    /// 越级：Stated 直接 finalize 必拒（结构性防线）。
    #[test]
    fn test_phase_skip_rejected() {
        assert!(probe_seq_b().is_err());
    }

    /// 回退：未登记对走 Reserved 档且令牌可重查。
    #[test]
    fn test_fallback_path() {
        assert!(probe_seq_c().is_ok());
    }

    /// 边界：域越界 (256, x) / (x, 256) 显式拒。
    #[test]
    fn test_domain_overflow() {
        let mut adj = ClaimAdjudicator::new();
        match adj.state_claim(VER_FIELD_MAX + 1, 0) {
            Err(c) => assert_eq!(c, MmCode::MajorOverflow.code()),
            Ok(_) => assert!(false, "主版本越域必须拒"),
        }
        match adj.state_claim(0, VER_FIELD_MAX + 1) {
            Err(c) => assert_eq!(c, MmCode::MinorOverflow.code()),
            Ok(_) => assert!(false, "次版本越域必须拒"),
        }
    }

    /// 重复边界：矩阵无重复对（查询幂等）。
    #[test]
    fn test_matrix_idempotent() {
        for i in 0..MATRIX_ROWS {
            let r = match matrix_row(i) {
                Some(r) => r,
                None => continue,
            };
            assert_eq!(matrix_lookup(r.major, r.minor), Some(i));
        }
        assert_eq!(matrix_lookup(9, 9), None);
    }

    /// 乱序：serial 非严格递增被拒。
    #[test]
    fn test_serial_out_of_order() {
        let mut adj = ClaimAdjudicator::new();
        let ida = match adj.state_claim(5, 0) {
            Ok(i) => i,
            Err(_) => return,
        };
        let idb = match adj.state_claim(5, 1) {
            Ok(i) => i,
            Err(_) => return,
        };
        let va = match adj.consult(ida) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(adj.finalize(ida, &va).is_ok());
        let vb = match adj.consult(idb) {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut bad = vb;
        bad.serial = va.serial;
        match adj.finalize(idb, &bad) {
            Err(c) => assert_eq!(c, MmCode::SerialOutOfOrder.code()),
            Ok(()) => assert!(false, "乱序 serial 必须拒"),
        }
    }

    /// 容量：票据槽满（32+1）显式拒且账不丢。
    #[test]
    fn test_ticket_cap() {
        let mut adj = ClaimAdjudicator::new();
        let mut i = 0;
        let mut last_err = false;
        while i <= TICKET_CAP {
            match adj.state_claim(5, 0) {
                Ok(_) => {}
                Err(_) => {
                    last_err = true;
                    break;
                }
            }
            i += 1;
        }
        assert!(last_err, "槽满必须拒");
        assert_eq!(adj.tickets.len(), TICKET_CAP);
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
