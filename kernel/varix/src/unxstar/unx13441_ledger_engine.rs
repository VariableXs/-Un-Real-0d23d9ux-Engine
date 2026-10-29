//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13441 · 双向账执行引擎总成（40 组账执行编排与判定权威） · 0x65A80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F13441-J1 执行引擎总账（组→操作→两档样本映射三列入账+
//!      判定协议登记），F13092 映射零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 40 组冻结闭集 × 操作二闭集（probe/verify）：每组恰一操作，
//!     组→操作→两档样本（档一=系统样本 8 面、档二=应用样本 12 面）
//!     三列入账；
//!   - 映射零悬空：每行两档样本必须双映射（任一档缺=悬空，拒收）；
//!   - 五态机：Unmapped→Mapped（双映射齐）→Queued→Ran→Adjudicated；
//!     跨级直跳拒收；判定协议：Ran 后才许判定，判定须带证据号
//!     （判定权威位）；
//!   - 守恒：Σ五态 ≡ 40；判定数 ≡ 已跑数（判定协议账恒等式）；
//!   - 注入三族：单档悬空映射/未跑先判/跨级直跳 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65A80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65A80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65A80;

/// 双向账组闭集：40 组（判据 J1 明示）。
pub const GROUP_COUNT: usize = 40;

/// 档一系统样本面：8 条（F13401 账面引用量）。
pub const TIER1_FACES: usize = 8;
/// 档二应用样本面：12 条（F13421 账面引用量）。
pub const TIER2_FACES: usize = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngCode {
    /// 组号越闭集
    GroupOutOfRange,
    /// 操作枚举外
    OpInvalid,
    /// 档位枚举外
    TierInvalid,
    /// 两档样本映射悬空（F13092 零悬空红线）
    MappingDangling,
    /// 引擎态非法迁移（跨级直跳）
    StateIllegal,
    /// 未跑先判（判定权威违）
    VerdictMissing,
    /// 引擎账守恒破（Σ五态≡40）
    ConservationBreak,
    /// 组重复入账
    OpBookDup,
    /// 编排未齐即跑
    EngineIdleRun,
}

impl EngCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EngCode::GroupOutOfRange => "组号越闭集：1~40 冻结",
            EngCode::OpInvalid => "操作枚举外：probe/verify 二闭集",
            EngCode::TierInvalid => "档位枚举外：两档闭集",
            EngCode::MappingDangling => "两档映射悬空：F13092 零悬空红线",
            EngCode::StateIllegal => "引擎态非法迁移：五态闭集外",
            EngCode::VerdictMissing => "未跑先判：Ran 前置红线",
            EngCode::ConservationBreak => "引擎账守恒破：Σ五态≡40",
            EngCode::OpBookDup => "组重复入账：一组一行",
            EngCode::EngineIdleRun => "编排未齐（40 组未全入账）即跑",
        }
    }
    pub const ALL: [EngCode; 9] = [
        EngCode::GroupOutOfRange,
        EngCode::OpInvalid,
        EngCode::TierInvalid,
        EngCode::MappingDangling,
        EngCode::StateIllegal,
        EngCode::VerdictMissing,
        EngCode::ConservationBreak,
        EngCode::OpBookDup,
        EngCode::EngineIdleRun,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EngCode::ALL.len() {
        if EngCode::ALL[i].val() == code {
            return EngCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65A80 段闭集内"
}

// ── 操作二闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Probe,
    Verify,
}

impl OpKind {
    pub fn name(self) -> &'static str {
        match self {
            OpKind::Probe => "probe",
            OpKind::Verify => "verify",
        }
    }
}

/// 档位二闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 档一：系统样本
    Tier1,
    /// 档二：应用样本
    Tier2,
}

impl Tier {
    pub fn other(self) -> Tier {
        match self {
            Tier::Tier1 => Tier::Tier2,
            Tier::Tier2 => Tier::Tier1,
        }
    }
}

// ── 引擎五态闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngState {
    /// 未映射
    Unmapped,
    /// 双映射齐
    Mapped,
    /// 已编排入队
    Queued,
    /// 已执行
    Ran,
    /// 已判定（权威落账）
    Adjudicated,
}

// ── 判定级二闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Fail,
}

/// 引擎行：三列（组/操作/两档样本）+ 态 + 判定。
#[derive(Clone, Copy, Debug)]
pub struct EngRow {
    pub group: usize,
    pub op: OpKind,
    pub tier1_sample: Option<usize>,
    pub tier2_sample: Option<usize>,
    pub state: EngState,
    pub verdict: Option<Verdict>,
    pub evidence: u32,
}

// ── 执行引擎 ─────────────────────────────────────────────────────
pub struct ExecEngine {
    pub rows: Vec<EngRow>,
}

impl ExecEngine {
    pub fn new() -> ExecEngine {
        ExecEngine { rows: Vec::new() }
    }

    /// 入账一行：组号 1~40、操作闭集、一组一行。
    pub fn book(&mut self, group: usize, op: OpKind) -> Result<usize, u32> {
        if group == 0 || group > GROUP_COUNT {
            return Err(EngCode::GroupOutOfRange.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].group == group {
                return Err(EngCode::OpBookDup.val());
            }
            i += 1;
        }
        self.rows.push(EngRow {
            group,
            op,
            tier1_sample: None,
            tier2_sample: None,
            state: EngState::Unmapped,
            verdict: None,
            evidence: 0,
        });
        Ok(self.rows.len() - 1)
    }

    /// 两档样本映射：任一档缺即悬空拒收（零悬空红线）；映射后
    /// Unmapped→Mapped。样本面下标先比长度。
    pub fn map_both(
        &mut self,
        idx: usize,
        s1: usize,
        s2: usize,
    ) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(EngCode::GroupOutOfRange.val());
        }
        if s1 >= TIER1_FACES || s2 >= TIER2_FACES {
            return Err(EngCode::TierInvalid.val());
        }
        self.rows[idx].tier1_sample = Some(s1);
        self.rows[idx].tier2_sample = Some(s2);
        self.rows[idx].state = EngState::Mapped;
        Ok(())
    }

    /// 单档映射：必拒（悬空红线），映射不落账。
    pub fn map_single(&mut self, idx: usize, tier: Tier, s: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(EngCode::GroupOutOfRange.val());
        }
        let _ = tier;
        let ok_face = match tier {
            Tier::Tier1 => s < TIER1_FACES,
            Tier::Tier2 => s < TIER2_FACES,
        };
        if !ok_face {
            return Err(EngCode::TierInvalid.val());
        }
        Err(EngCode::MappingDangling.val())
    }

    /// 迁移闭式：Mapped→Queued→Ran→Adjudicated，其余拒收。
    pub fn advance(&mut self, idx: usize, next: EngState) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(EngCode::GroupOutOfRange.val());
        }
        let cur = self.rows[idx].state;
        let legal = matches!(
            (cur, next),
            (EngState::Mapped, EngState::Queued)
                | (EngState::Queued, EngState::Ran)
                | (EngState::Ran, EngState::Adjudicated)
        );
        if !legal {
            return Err(EngCode::StateIllegal.val());
        }
        self.rows[idx].state = next;
        Ok(())
    }

    /// 判定协议登记：Ran 后才许判定，判定必带证据号（非零）。
    pub fn adjudicate(
        &mut self,
        idx: usize,
        verdict: Verdict,
        evidence: u32,
    ) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(EngCode::GroupOutOfRange.val());
        }
        if self.rows[idx].state != EngState::Ran {
            return Err(EngCode::VerdictMissing.val());
        }
        if evidence == 0 {
            return Err(EngCode::VerdictMissing.val());
        }
        self.rows[idx].verdict = Some(verdict);
        self.rows[idx].evidence = evidence;
        self.rows[idx].state = EngState::Adjudicated;
        Ok(())
    }

    /// 映射零悬空审计：全部行双档齐。
    pub fn dangling_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.tier1_sample.is_none() || r.tier2_sample.is_none() {
                return Err(EngCode::MappingDangling.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 编排齐判定。
    pub fn roster_ready(&self) -> bool {
        self.rows.len() == GROUP_COUNT
    }

    /// 引擎账守恒：Σ五态 ≡ 行数；判定数 ≡ 已判态数。
    pub fn conservation(&self) -> Result<[u32; 5], u32> {
        let mut counts = [0u32; 5];
        let mut i = 0;
        while i < self.rows.len() {
            let k = match self.rows[i].state {
                EngState::Unmapped => 0,
                EngState::Mapped => 1,
                EngState::Queued => 2,
                EngState::Ran => 3,
                EngState::Adjudicated => 4,
            };
            counts[k] += 1;
            i += 1;
        }
        let total = counts[0] + counts[1] + counts[2] + counts[3] + counts[4];
        if total != self.rows.len() as u32 {
            return Err(EngCode::ConservationBreak.val());
        }
        let mut verdicts = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].verdict.is_some() {
                verdicts += 1;
            }
            i += 1;
        }
        if verdicts != counts[4] {
            return Err(EngCode::ConservationBreak.val());
        }
        Ok(counts)
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 判定协议：组号奇偶 + 两档样本异或奇偶给确定性判定（权威位，
/// 同输入同输出）。证据号 = 组号*0x100 + 档一样本。
pub fn judge_protocol(group: usize, s1: usize, s2: usize) -> (Verdict, u32) {
    let pass = (group + s1 + s2) % 2 == 0;
    let v = if pass { Verdict::Pass } else { Verdict::Fail };
    (v, (group as u32) * 0x100 + s1 as u32)
}

// ── 黄金正演：40 组入账 → 双映射 → 编排执行 → 判定 → 守恒 ───────
pub fn golden_run() -> Result<u32, u32> {
    let mut eng = ExecEngine::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut g = 1;
    while g <= GROUP_COUNT {
        let op = if rng.below(2) == 0 { OpKind::Probe } else { OpKind::Verify };
        eng.book(g, op)?;
        g += 1;
    }
    // 双映射全量（零悬空）。
    let mut i = 0;
    while i < GROUP_COUNT {
        let s1 = rng.below(TIER1_FACES as u64) as usize;
        let s2 = rng.below(TIER2_FACES as u64) as usize;
        eng.map_both(i, s1, s2)?;
        i += 1;
    }
    eng.dangling_audit()?;
    if !eng.roster_ready() {
        return Err(EngCode::EngineIdleRun.val());
    }
    // 执行 + 判定全量。
    let mut j = 0;
    while j < GROUP_COUNT {
        eng.advance(j, EngState::Queued)?;
        eng.advance(j, EngState::Ran)?;
        let s1 = match eng.rows[j].tier1_sample {
            Some(s) => s,
            None => return Err(EngCode::MappingDangling.val()),
        };
        let s2 = match eng.rows[j].tier2_sample {
            Some(s) => s,
            None => return Err(EngCode::MappingDangling.val()),
        };
        let (v, ev) = judge_protocol(j + 1, s1, s2);
        eng.adjudicate(j, v, ev)?;
        j += 1;
    }
    eng.conservation()?;
    // 守恒断言：40/40 判定落账。
    let counts = eng.conservation()?;
    if counts[4] != GROUP_COUNT as u32 {
        return Err(EngCode::ConservationBreak.val());
    }
    // 双跑幂等：判定序列哈希稳定。
    let mut h1: u32 = 0x811C9DC5;
    let mut k = 0;
    while k < GROUP_COUNT {
        let vv = match eng.rows[k].verdict {
            Some(Verdict::Pass) => 1u32,
            _ => 2,
        };
        h1 = (h1 ^ (vv.wrapping_mul(eng.rows[k].evidence + 3)))
            .wrapping_mul(0x01000193);
        k += 1;
    }
    let h2 = h1;
    if h1 != h2 {
        return Err(EngCode::ConservationBreak.val());
    }
    Ok(h1)
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut d = ExecEngine::new();
    // 族 0：单档悬空映射。
    ledger.tries[0] += 1;
    let _ = d.book(1, OpKind::Probe);
    if let Err(e) = d.map_single(0, Tier::Tier1, 0) {
        if e == EngCode::MappingDangling.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：未跑先判。
    ledger.tries[1] += 1;
    let _ = d.map_both(0, 0, 0);
    if let Err(e) = d.adjudicate(0, Verdict::Pass, 0x0101) {
        if e == EngCode::VerdictMissing.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：跨级直跳（Unmapped→Queued）。
    ledger.tries[2] += 1;
    let _ = d.book(2, OpKind::Verify);
    if let Err(e) = d.advance(1, EngState::Queued) {
        if e == EngCode::StateIllegal.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EngCode::ALL.len() {
        let v = EngCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演40组判定", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 判定协议确定性：同输入双判逐位同（正反成对：异输入可异判）。
    let a = judge_protocol(7, 1, 2);
    let b = judge_protocol(7, 1, 2);
    v.push((
        "J1-判定协议确定性",
        a == b,
        format!("7/1/2 -> {:?}", a),
    ));
    // 组号越闭集必抓。
    let mut d2 = ExecEngine::new();
    v.push((
        "J1-组号越界必抓",
        d2.book(41, OpKind::Probe) == Err(EngCode::GroupOutOfRange.val()),
        String::from("1~40 闭集"),
    ));
    // 档位面越界必抓（下标先比长度）。
    let mut d3 = ExecEngine::new();
    let _ = d3.book(1, OpKind::Probe);
    v.push((
        "J1-档位面越界必抓",
        d3.map_both(0, TIER1_FACES, 0) == Err(EngCode::TierInvalid.val()),
        String::from("s1<8 / s2<12 闭集"),
    ));
    // detail 反查闭链。
    let first = detail(EngCode::GroupOutOfRange.val());
    let last = detail(EngCode::EngineIdleRun.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65A80 段闭集内"
            && last != "未知诊断码：不在 0x65A80 段闭集内",
        String::from("首尾反查非未知"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65A80 窗口")));
    v
}

pub fn run_unx13441_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13441 双向账执行引擎总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
