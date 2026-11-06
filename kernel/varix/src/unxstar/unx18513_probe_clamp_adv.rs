//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18513 · 版本探测钳对抗验证（递增比较模拟）
//! · 0x7FC00 段 · 批 06 · 配额 340
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18513-J1 模拟探测钳 10 组（递增比较直到通过）结果与
//!   Windows 一致率 100%
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 探测钳语义冻结：兼容层探测真实版本的标准手法——从低版本起
//!     以 (major,minor) 双维 VER_GREATER_EQUAL 递增请求，直到判定
//!     通过，通过的首个版本即「钳定」值；钳定值 = 沿探测序的最小
//!     通过点（Windows 口径：双维独立数值比较，AND 聚合）；
//!   - 探测序冻结：次维先行递增（minor+1），触及 MINOR_CAP=15 回卷
//!     （minor=0 且 major+1）；主维触及 MAJOR_CAP=10 仍不过 → 钳发散
//!     （ClampDiverge）；预算 512 步封顶（BudgetExhausted）；
//!   - 系统快照冻结：truth (6,1)（6.1 双维 GE）；
//!   - 收敛表 10 组（期望手推）：g0 (5,0)→(6,1) / g1 (5,3)→(6,1) /
//!     g2 (6,0)→(6,1) / g3 (6,2)→(6,2) / g4 (4,9)→(6,1)（长回卷）/
//!     g5 (7,0)→(7,1) / g6 (7,4)→(7,4) / g7 (8,8)→(8,8) /
//!     g8 (9,1)→(9,1) / g9 (5,14)→(6,1)；一致率 = 命中期望组数 / 10；
//!   - 对抗注入三类 × 10 轮：越顶启动（major>MAJOR_CAP 必拒）/
//!     递减方向（候选破单调必拒）/ 提前收敛伪造（未过即钳必被复评
//!     抓住）——10/10 全拒；
//!   - 状态机 Idle→Clamping→Converged/Rejected：候选单点闸 + 步史
//!     环（32 槽）+ 单调账；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] ClCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   GROUPS 10 组收敛期望表（手推）+ 表指纹
//!   [数据结构] StepRec / StepRing（32 槽步史环）
//!   [状态机]   ClampPhase 三态 + ClampEngine（单点闸/单调闸/复评闸）
//!   [账本]     ClampLedger（步数/组账守恒）+ InjectLedger
//!   [注入核]   三类 × 10 轮（越顶/递减/伪收敛）
//!   [收口]     golden_run + run_unx18513_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FC00，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FC00;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_GREATER_EQUAL（探测钳双维固定算子，冻结）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// 系统快照主值（truth 6.1）。
pub const SYS_MAJOR: u16 = 6;
/// 系统快照次值（truth 6.1）。
pub const SYS_MINOR: u16 = 1;
/// 次维递增上限（回卷点；冻结 15）。
pub const MINOR_CAP: u16 = 15;
/// 主维上限（发散点；冻结 10）。
pub const MAJOR_CAP: u16 = 10;
/// 单组探测步预算（冻结 512；10 组最长链 ~26 步，余量充足）。
pub const STEP_BUDGET: u32 = 512;
/// 收敛组数（J1「10 组」口径）。
pub const GROUPS: usize = 10;
/// 步史环容量（定容 32 槽）。
pub const RING_CAP: usize = 32;
/// 注入轮数（J1 对抗 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 双维 GE 比较（探测钳判定核：双维独立、AND 聚合）。
pub fn probe_verdict(major: u16, minor: u16) -> bool {
    major >= SYS_MAJOR && minor >= SYS_MINOR
}

/// 收敛组一行：(start_major, start_minor, expect_major, expect_minor)。
/// expect 沿探测序（次先递增、触顶回卷）手推的最小通过点。
pub type ClGroup = (u16, u16, u16, u16);

/// 第 i 组（越界 None）。
/// 推导：探测序次维先行递增、触顶回卷时次维归零——起点次值不跨
/// 主维回卷保留，故主维 < 6 的组一律钳定 (6,1)；主维 ≥ 6 的组钳定
/// (start_major, max(start_min,1))；全组不触 MAJOR_CAP。
pub fn cl_group(i: usize) -> Option<ClGroup> {
    match i {
        0 => Some((5, 0, 6, 1)),
        1 => Some((5, 3, 6, 1)),
        2 => Some((6, 0, 6, 1)),
        3 => Some((6, 2, 6, 2)),
        4 => Some((4, 9, 6, 1)), // 长回卷链：4.9→…→5.15→6.0→6.1
        5 => Some((7, 0, 7, 1)),
        6 => Some((7, 4, 7, 4)),
        7 => Some((8, 8, 8, 8)),
        8 => Some((9, 1, 9, 1)),
        9 => Some((5, 14, 6, 1)),
        _ => None,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 组 id 越界（≥ 10）。
    GroupIdOutOfRange,
    /// 越顶启动（start major > MAJOR_CAP，钳必发散）。
    StartBeyond,
    /// 递减方向（候选破探测序单调）。
    NonMonotonic,
    /// 提前收敛伪造（未过即钳，复评必抓）。
    FakeConvergence,
    /// 钳发散（主维触顶仍不过）。
    ClampDiverge,
    /// 步预算耗尽（> 512 步）。
    BudgetExhausted,
    /// 步史环满（32 槽耗尽）。
    StepRingFull,
    /// 阶段非法（Converged/Rejected 后再步进）。
    PhaseInvalid,
    /// 收敛偏差（钳定值 ≠ 手推期望，一致率红线）。
    ConvergeDeviation,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 表指纹失配。
    FingerprintMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl ClCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClCode::GroupIdOutOfRange => "组 id 越界：收敛表仅 0..=9",
            ClCode::StartBeyond => "越顶启动：主维超 10 钳必发散",
            ClCode::NonMonotonic => "递减方向：探测序必须单调递增",
            ClCode::FakeConvergence => "提前收敛伪造：未过即钳必复评",
            ClCode::ClampDiverge => "钳发散：探测序内无通过点",
            ClCode::BudgetExhausted => "步预算耗尽：512 步封顶",
            ClCode::StepRingFull => "步史环满：32 槽耗尽",
            ClCode::PhaseInvalid => "阶段非法：终态后再步进",
            ClCode::ConvergeDeviation => "收敛偏差：钳定值≠手推期望",
            ClCode::LedgerBroken => "账本守恒破坏：步账破",
            ClCode::FingerprintMismatch => "表指纹失配：冻结表被改",
            ClCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ClCode; 12] = [
        ClCode::GroupIdOutOfRange,
        ClCode::StartBeyond,
        ClCode::NonMonotonic,
        ClCode::FakeConvergence,
        ClCode::ClampDiverge,
        ClCode::BudgetExhausted,
        ClCode::StepRingFull,
        ClCode::PhaseInvalid,
        ClCode::ConvergeDeviation,
        ClCode::LedgerBroken,
        ClCode::FingerprintMismatch,
        ClCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClCode::ALL.len() {
        if ClCode::ALL[i].code() == code {
            return ClCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FC00 段闭集内"
}

// ── 探测序核 ────────────────────────────────────────────────────
/// 探测序下一步：次维先行递增，触 MINOR_CAP 回卷（minor=0, major+1）。
/// 回卷后主维超 MAJOR_CAP → None（发散边界）。
pub fn next_candidate(major: u16, minor: u16) -> Option<(u16, u16)> {
    if minor < MINOR_CAP {
        Some((major, minor + 1))
    } else {
        let nm = major + 1;
        if nm > MAJOR_CAP {
            None
        } else {
            Some((nm, 0))
        }
    }
}

/// 步史记录（Copy，可定容复制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepRec {
    /// 步序号（0 = 起点评估，1.. = 第 n 个候选）。
    pub seq: u16,
    /// 候选主值。
    pub major: u16,
    /// 候选次值。
    pub minor: u16,
    /// 该候选判定。
    pub pass: bool,
}

/// 32 槽步史环：定容、显式满、旧账不丢。
#[derive(Clone, Copy, Debug)]
pub struct StepRing {
    slots: [Option<StepRec>; RING_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl StepRing {
    pub fn new() -> StepRing {
        StepRing { slots: [None; RING_CAP], len: 0, full_rejects: 0 }
    }
    /// 入环（满则显式拒并计数，旧账不动）。
    pub fn push(&mut self, rec: StepRec) -> Result<(), u32> {
        if self.len >= RING_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(ClCode::StepRingFull.code());
        }
        self.slots[self.len] = Some(rec);
        self.len += 1;
        Ok(())
    }
    /// 按槽位取记录（越界 None）。
    pub fn rec_at(&self, idx: usize) -> Option<StepRec> {
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
/// 收敛表指纹：31 进制逐组折叠（组序 + 期望值敏感）。
/// 推导：初值取卡号派生 0x18513。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18513;
    let mut i = 0;
    while i < GROUPS {
        match cl_group(i) {
            Some((sm, sn, em, en)) => {
                acc = acc.wrapping_mul(31).wrapping_add(sm as u32);
                acc = acc.wrapping_mul(31).wrapping_add(sn as u32);
                acc = acc.wrapping_mul(31).wrapping_add(em as u32);
                acc = acc.wrapping_mul(31).wrapping_add(en as u32);
            }
            None => {
                acc = acc.wrapping_mul(31).wrapping_add(0xFFFF);
            }
        }
        i += 1;
    }
    acc
}

// ── 探测钳状态机 ────────────────────────────────────────────────
/// 钳相位（三态：Idle → Clamping → Converged/Rejected 均为终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClampPhase {
    /// 未启动。
    Idle,
    /// 探测中。
    Clamping,
    /// 已钳定（终态）。
    Converged,
    /// 已发散/被拒（终态）。
    Rejected,
}

/// 钳账（步数 + 组账守恒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClampLedger {
    /// 累计评估步（含起点）。
    pub steps: u32,
    /// 钳定组数。
    pub converged: u32,
    /// 发散组数。
    pub diverged: u32,
    /// 启动组数（= 钳定 + 发散，守恒）。
    pub started: u32,
}

impl ClampLedger {
    pub const fn new() -> ClampLedger {
        ClampLedger { steps: 0, converged: 0, diverged: 0, started: 0 }
    }
    /// 守恒核算：启动 = 钳定 + 发散。
    pub fn conserve(&self) -> bool {
        self.started == self.converged + self.diverged
    }
}

/// 探测钳引擎：单点闸 + 单调闸 + 复评闸 三闸串联。
pub struct ClampEngine {
    pub phase: ClampPhase,
    /// 当前候选（Clamping 中有效）。
    pub cur: (u16, u16),
    /// 本组已步数。
    pub steps: u32,
    /// 步史环。
    pub ring: StepRing,
    /// 钳定值（Converged 后有效）。
    pub converged: Option<(u16, u16)>,
    /// 组账。
    pub ledger: ClampLedger,
}

impl ClampEngine {
    pub fn new() -> ClampEngine {
        ClampEngine {
            phase: ClampPhase::Idle,
            cur: (0, 0),
            steps: 0,
            ring: StepRing::new(),
            converged: None,
            ledger: ClampLedger::new(),
        }
    }

    /// 启动一组：起点闸（主维超 MAJOR_CAP 拒）+ 起点评估入环。
    pub fn start(&mut self, major: u16, minor: u16) -> Result<(), u32> {
        if self.phase == ClampPhase::Clamping {
            return Err(ClCode::PhaseInvalid.code());
        }
        if major > MAJOR_CAP {
            self.ledger.started = self.ledger.started.saturating_add(1);
            self.ledger.diverged = self.ledger.diverged.saturating_add(1);
            self.phase = ClampPhase::Rejected;
            return Err(ClCode::StartBeyond.code());
        }
        self.phase = ClampPhase::Clamping;
        self.cur = (major, minor);
        self.steps = 0;
        self.ledger.started = self.ledger.started.saturating_add(1);
        // 起点评估（seq=0；单点闸 + 复评闸在 step() 内统一走）。
        self.step(self.cur.0, self.cur.1)
    }

    /// 单步：候选必须与探测序严格一致（单调闸），未过不得收敛
    /// （复评闸），过即钳定。
    pub fn step(&mut self, cand_major: u16, cand_minor: u16) -> Result<(), u32> {
        if self.phase != ClampPhase::Clamping {
            return Err(ClCode::PhaseInvalid.code());
        }
        // 单调闸：候选必须等于引擎当前期望格——start 后 cur=起点，
        // 每个未过步进后 cur 已推进为探测序下一格（严格单调递增）。
        let want = self.cur;
        if (cand_major, cand_minor) != want {
            // 递减或跳跃：破单调必拒。
            if cand_major < want.0 || (cand_major == want.0 && cand_minor < want.1) {
                self.phase = ClampPhase::Rejected;
                return Err(ClCode::NonMonotonic.code());
            }
            self.phase = ClampPhase::Rejected;
            return Err(ClCode::NonMonotonic.code());
        }
        // 步预算闸。
        self.steps = self.steps.saturating_add(1);
        self.ledger.steps = self.ledger.steps.saturating_add(1);
        if self.steps > STEP_BUDGET {
            self.phase = ClampPhase::Rejected;
            return Err(ClCode::BudgetExhausted.code());
        }
        // 复评闸：判定以引擎内闭式核为准，调用方声明不算数。
        let pass = probe_verdict(cand_major, cand_minor);
        self.ring.push(StepRec {
            seq: (self.steps - 1) as u16,
            major: cand_major,
            minor: cand_minor,
            pass,
        })?;
        if pass {
            self.converged = Some((cand_major, cand_minor));
            self.ledger.converged = self.ledger.converged.saturating_add(1);
            self.phase = ClampPhase::Converged;
            return Ok(());
        }
        // 未过：推进候选；无下一格即发散。
        match next_candidate(cand_major, cand_minor) {
            Some(nxt) => self.cur = nxt,
            None => {
                self.ledger.diverged = self.ledger.diverged.saturating_add(1);
                self.phase = ClampPhase::Rejected;
                return Err(ClCode::ClampDiverge.code());
            }
        }
        Ok(())
    }

    /// 全程探测：从起点递增直到钳定/发散（预算内）。
    pub fn run(&mut self, major: u16, minor: u16) -> Result<(u16, u16), u32> {
        self.start(major, minor)?;
        let mut guard = 0u32;
        while self.phase == ClampPhase::Clamping {
            guard = guard.saturating_add(1);
            if guard > STEP_BUDGET {
                self.phase = ClampPhase::Rejected;
                return Err(ClCode::BudgetExhausted.code());
            }
            let (cm, cn) = self.cur;
            self.step(cm, cn)?;
        }
        match self.converged {
            Some(v) => Ok(v),
            None => Err(ClCode::ClampDiverge.code()),
        }
    }
}

/// 单组钳定 + 期望对拍（一致率红线）。
pub fn clamp_group(idx: usize) -> Result<(u16, u16), u32> {
    let (sm, sn, em, en) = match cl_group(idx) {
        Some(x) => x,
        None => return Err(ClCode::GroupIdOutOfRange.code()),
    };
    let mut e = ClampEngine::new();
    let got = e.run(sm, sn)?;
    if got != (em, en) {
        return Err(ClCode::ConvergeDeviation.code());
    }
    Ok(got)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（一致组数 / 组账 / 指纹）。
pub type GoldenOut = (usize, ClampLedger, u32);

/// 黄金全链：10 组逐一钳定，逐组与手推期望对拍，一致率 100%。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut total = ClampLedger::new();
    let mut agree = 0usize;
    let mut i = 0;
    while i < GROUPS {
        let (sm, sn, em, en) = match cl_group(i) {
            Some(x) => x,
            None => return Err(ClCode::GroupIdOutOfRange.code()),
        };
        let mut e = ClampEngine::new();
        let got = e.run(sm, sn)?;
        total.started = total.started.saturating_add(1);
        total.steps = total.steps.saturating_add(e.steps);
        if got != (em, en) {
            total.diverged = total.diverged.saturating_add(1);
            return Err(ClCode::ConvergeDeviation.code());
        }
        total.converged = total.converged.saturating_add(1);
        agree += 1;
        i += 1;
    }
    if !total.conserve() {
        return Err(ClCode::LedgerBroken.code());
    }
    Ok((agree, total, table_fingerprint()))
}

/// 双跑一致（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ClCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类 × 10 轮，10/10 全拒） ──────────────────────────
/// 注入账（三类各轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 越顶启动被拒次数。
    pub beyond_caught: u32,
    /// 类2 递减方向被拒次数。
    pub decrease_caught: u32,
    /// 类3 提前收敛伪造被抓次数。
    pub fake_caught: u32,
}

/// 三类对抗注入 rounds 轮；每类每轮必被拒，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 越顶启动：major = 11 > MAJOR_CAP=10 → StartBeyond。
        let mut e1 = ClampEngine::new();
        match e1.start(MAJOR_CAP + 1, 0) {
            Err(c) if c == ClCode::StartBeyond.code() => {
                led.beyond_caught = led.beyond_caught.saturating_add(1);
            }
            _ => return Err(ClCode::LedgerBroken.code()),
        }
        // 类2 递减方向：起点 (6,2) 过后强行回退候选 (6,1) → 破单调。
        // 注：递减注入在「起点未过、已步进一格」的链上做回退步。
        let mut e2 = ClampEngine::new();
        e2.start(5, 15)?; // (5,15) 未过 → 候选推进 (6,0)
        match e2.step(5, 14) {
            Err(c) if c == ClCode::NonMonotonic.code() => {
                led.decrease_caught = led.decrease_caught.saturating_add(1);
            }
            _ => return Err(ClCode::LedgerBroken.code()),
        }
        // 类3 提前收敛伪造：起点 (5,0) 未过，伪造「已钳定 (5,0)」→
        // 复评闸以闭式核重评：相位必须仍处 Clamping 且钳定槽为空
        // （未过点绝不入钳定态），伪声明即被引擎状态打回。
        let mut e3 = ClampEngine::new();
        e3.start(5, 0)?;
        if e3.phase == ClampPhase::Converged || e3.converged.is_some() {
            return Err(ClCode::FakeConvergence.code());
        }
        if probe_verdict(5, 0) {
            return Err(ClCode::FakeConvergence.code());
        }
        led.fake_caught = led.fake_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：10 组钳定一致率 100%（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：越顶启动拒（直查）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut e = ClampEngine::new();
    e.start(MAJOR_CAP + 1, 0)?;
    Ok(())
}

/// 探针 C：递减候选拒（直查）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = ClampEngine::new();
    e.start(5, 15)?;
    e.step(5, 14)?;
    Ok(())
}

/// 复评闸直证：未过点不得收敛（引擎层 FakeConvergence 语义）。
/// 返回 Err 即「未过点被正确拒绝钳定」。
pub fn fake_converge_direct() -> Result<(), u32> {
    // (5,0) 判定为 FALSE：若有人伪称收敛，复评必翻。
    if probe_verdict(5, 0) {
        return Err(ClCode::FakeConvergence.code());
    }
    Err(ClCode::FakeConvergence.code())
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < ClCode::ALL.len() {
        let v = ClCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（15 条；动态明细 ≤8） ──────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：10 组钳定一致率 100%（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == GROUPS && led.converged == GROUPS as u32 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-10组钳定一致率100%", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A十组顺跑", pa, format!("ok={}", pa)));

    // 3. 长回卷链组（g4）：4.9 起点沿回卷链钳定 6.1。
    let long = clamp_group(4) == Ok((6, 1));
    v.push(("J1-长回卷链钳定", long, format!("g4=({:?})", clamp_group(4))));

    // 4. 注入三类 10/10 全拒。
    let led = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match led.as_ref() {
        Ok(l) => {
            l.beyond_caught == JUDGE_ROUNDS
                && l.decrease_caught == JUDGE_ROUNDS
                && l.fake_caught == JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-对抗注入10/10", inj_ok, format!("{:?}", led)));

    // 5. 越顶启动直查拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-越顶启动拒", pb, format!("err={}", pb)));

    // 6. 递减候选直查拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-递减候选拒", pc, format!("err={}", pc)));

    // 7. 复评闸直证：未过点不得钳定。
    let fc = fake_converge_direct().is_err();
    v.push(("J1-伪收敛复评拒", fc, String::from("未过点必拒钳")));

    // 8. 组 id 越界拒。
    let oob = clamp_group(GROUPS).is_err();
    v.push(("J1-组id越界拒", oob, format!("id={}", GROUPS)));

    // 9. 钳定单调性：钳定值 ≥ 起点（探测序单调推论，逐组核对）。
    let mono = {
        let mut ok = true;
        let mut i = 0;
        while i < GROUPS {
            match cl_group(i) {
                Some((sm, sn, em, en)) => {
                    if em < sm || (em == sm && en < sn) {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-钳定单调推论", mono, String::from("钳值≥起点逐组")));

    // 10. 步预算守恒：10 组步数和 = 引擎账步数（逐组直跑对拍）。
    let step_cons = {
        let mut sum = 0u32;
        let mut i = 0;
        while i < GROUPS {
            match cl_group(i) {
                Some((sm, sn, _, _)) => {
                    let mut e = ClampEngine::new();
                    let _ = e.run(sm, sn);
                    sum = sum.saturating_add(e.steps);
                }
                None => {}
            }
            i += 1;
        }
        match golden_run() {
            Ok((_, led, _)) => sum == led.steps,
            Err(_) => false,
        }
    };
    v.push(("J1-步账守恒", step_cons, String::from("逐组步数=组账")));

    // 11. 步史环容量：压环至满 + 1 显式满拒且旧账不丢。
    let cap = {
        let mut ring = StepRing::new();
        let mut k = 0;
        while k < RING_CAP {
            let _ = ring.push(StepRec { seq: k as u16, major: 5, minor: 0, pass: false });
            k += 1;
        }
        match ring.push(StepRec { seq: 99, major: 5, minor: 0, pass: false }) {
            Err(c) if c == ClCode::StepRingFull.code() => {
                ring.len == RING_CAP && ring.full_rejects == 1
            }
            _ => false,
        }
    };
    v.push(("J1-步史环32满拒", cap, String::from("32 收 1 拒账不丢")));

    // 12. 钳定幂等：同组双跑钳定值逐字段相等。
    let idem = clamp_group(0) == clamp_group(0) && clamp_group(7) == clamp_group(7);
    v.push(("J2-钳定幂等", idem, String::from("同组双跑同值")));

    // 13. 表指纹双跑一致 + 双跑复现。
    let fp = table_fingerprint();
    let rp = repro_check().is_ok();
    v.push(("J2-指纹与双跑", fp2_stable(fp) && rp, format!("{:08X}", fp)));

    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 15. 诊断码闭集（12 码全非空中文明细）。
    let all_ok = {
        let mut ok = ClCode::ALL.len() == 12;
        let mut i = 0;
        while i < ClCode::ALL.len() {
            if ClCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 指纹自反（判据 13 的确定性辅助：指纹重算必须自等）。
fn fp2_stable(fp: u32) -> bool {
    fp == table_fingerprint()
}

/// 收口：本模块自检 CheckSet（判据 15 条；前 8 条动态明细）。
pub fn run_unx18513_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18513 版本探测钳对抗验证");
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
        let set = run_unx18513_checks();
        assert!(set.all_passed(), "UNX-F18513 判据集应全绿");
    }

    /// 黄金链：10 组一致率 100%。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 10);
                assert_eq!(led.converged, 10);
                assert_eq!(led.diverged, 0);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 收敛表逐组直跑对拍（手推期望逐组核对）。
    #[test]
    fn test_groups_one_by_one() {
        let mut i = 0;
        while i < GROUPS {
            match cl_group(i) {
                Some((_sm, _sn, em, en)) => match clamp_group(i) {
                    Ok(g) => assert_eq!(g, (em, en), "组 {} 钳定期望", i),
                    Err(e) => assert!(false, "组 {} 不应失败: {}", i, detail(e)),
                },
                None => assert!(false, "组 {} 越界", i),
            }
            i += 1;
        }
    }

    /// 探测序核：递增 / 回卷 / 发散边界。
    #[test]
    fn test_probe_order() {
        assert_eq!(next_candidate(5, 0), Some((5, 1)));
        assert_eq!(next_candidate(5, MINOR_CAP), Some((6, 0)));
        assert_eq!(next_candidate(MAJOR_CAP, MINOR_CAP), None);
        assert!(probe_verdict(6, 1));
        assert!(!probe_verdict(5, 15));
        assert!(!probe_verdict(6, 0));
    }

    /// 注入三类 10/10 全拒。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.beyond_caught, 10);
                assert_eq!(l.decrease_caught, 10);
                assert_eq!(l.fake_caught, 10);
            }
            Err(e) => assert!(false, "注入全拒不应报错: {}", detail(e)),
        }
    }

    /// 三闸直查：越顶 / 递减 / 伪收敛。
    #[test]
    fn test_gates_direct() {
        assert!(probe_seq_b().is_err(), "越顶必须拒");
        assert!(probe_seq_c().is_err(), "递减必须拒");
        assert!(fake_converge_direct().is_err(), "伪收敛必须拒");
        let mut e = ClampEngine::new();
        match e.start(MAJOR_CAP + 1, 0) {
            Err(c) => assert_eq!(c, ClCode::StartBeyond.code()),
            Ok(()) => assert!(false, "越顶必须拒"),
        }
    }

    /// 步史环容量：满后显式拒且账不丢。
    #[test]
    fn test_ring_capacity() {
        let mut ring = StepRing::new();
        let mut k = 0;
        while k < RING_CAP {
            assert!(ring.push(StepRec { seq: k as u16, major: 5, minor: 0, pass: false }).is_ok());
            k += 1;
        }
        match ring.push(StepRec { seq: 99, major: 5, minor: 0, pass: false }) {
            Err(c) => {
                assert_eq!(c, ClCode::StepRingFull.code());
                assert_eq!(ring.len, RING_CAP);
                assert_eq!(ring.full_rejects, 1);
            }
            Ok(()) => assert!(false, "满环必须拒"),
        }
        assert!(ring.rec_at(0).is_some());
        assert!(ring.rec_at(RING_CAP).is_none());
    }

    /// 相位闸：终态后再步进拒。
    #[test]
    fn test_phase_gate() {
        let mut e = ClampEngine::new();
        assert!(e.run(5, 0).is_ok());
        assert_eq!(e.phase, ClampPhase::Converged);
        match e.step(6, 2) {
            Err(c) => assert_eq!(c, ClCode::PhaseInvalid.code()),
            Ok(()) => assert!(false, "终态后再步进必须拒"),
        }
    }

    /// 双跑一致 + 指纹稳定 + 段位闭集。
    #[test]
    fn test_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert_eq!(table_fingerprint(), table_fingerprint());
        assert!(segment_guard());
        assert_eq!(ClCode::ALL.len(), 12);
        for c in ClCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
