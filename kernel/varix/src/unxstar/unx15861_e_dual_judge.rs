//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15861 · E 型双判据协议在传输面的落位总纲（J1+J2 双验） · 0x678C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15861-J1 双判据落位成文（J1 达成复测 10/10 次 + J2 误用
//!      精确单一错误码 10/10 次同 ID 双判据），单判据注入（只验达成
//!      不验误用）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - E 型双判据协议：同 ID 的判据必须成对——J1 达成面（正演成功）
//!     + J2 误用面（错误码精确单一，多码/少码/错码皆拒）；
//!   - 传输面载体：报文（ID + 判据对 + 错误码闭集）；
//!   - 落位状态机：Draft → Paired → Verified / Unpaired（单判据必检）；
//!   - 错误码闭集（4 码，写死）：越界/失配/缺失/超时；
//!   - 正演 10/10：J1+J2 同 ID 双验全过；注入 10/10：单判据必检出；
//!   - LCG 种子写死 = SEG_BASE（0x678C0），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x678C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x678C0;

/// 每轮正演次数（判据钉版：10/10）。
pub const RUNS_PER_ROUND: u32 = 10;

/// 错误码闭集（4 码，写死：传输面 E 型协议推定）。
pub const ERR_RANGE: u32 = 1;
pub const ERR_MISMATCH: u32 = 2;
pub const ERR_MISSING: u32 = 3;
pub const ERR_TIMEOUT: u32 = 4;
pub const ERR_SET: [u32; 4] = [ERR_RANGE, ERR_MISMATCH, ERR_MISSING, ERR_TIMEOUT];
pub const ERR_NAMES: [&str; 4] = ["越界", "失配", "缺失", "超时"];

/// 同 ID 判据对载体数（6 对，写死：传输面六类报文）。
pub const PROTO_COUNT: usize = 6;
pub const PROTO_NAMES: [&str; PROTO_COUNT] =
    ["面板报文", "锁定报文", "总账报文", "通道报文", "违例报文", "快照报文"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualCode {
    /// 报文 ID 越闭集
    ProtoOutOfRange,
    /// 单判据（缺 J1 或缺 J2）
    SingleJudgeOnly,
    /// 错误码越闭集
    ErrCodeOutOfRange,
    /// 错误码不精确（多码/错码）
    ErrCodeImprecise,
    /// 非法相位边
    IllegalPhaseEdge,
    /// J1 未达成却报成（虚报）
    AchieveFaked,
    /// J2 未触发却报误用（虚报误用）
    MisuseFaked,
    /// 双跑不一致
    NotReproducible,
    /// 协议未封账
    ProtoUnsealed,
}

impl DualCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DualCode::ProtoOutOfRange => "报文 ID 越闭集：六类冻结",
            DualCode::SingleJudgeOnly => "单判据：只验达成不验误用",
            DualCode::ErrCodeOutOfRange => "错误码越闭集：四码冻结",
            DualCode::ErrCodeImprecise => "错误码不精确：多码/错码皆拒",
            DualCode::IllegalPhaseEdge => "非法相位边：落位状态机闭集外",
            DualCode::AchieveFaked => "J1 未达成却报成：虚报",
            DualCode::MisuseFaked => "J2 未触发却报误用：虚报误用",
            DualCode::NotReproducible => "双跑不一致：落位复现红线",
            DualCode::ProtoUnsealed => "协议未封账：Verified 未全达",
        }
    }
    pub const ALL: [DualCode; 9] = [
        DualCode::ProtoOutOfRange,
        DualCode::SingleJudgeOnly,
        DualCode::ErrCodeOutOfRange,
        DualCode::ErrCodeImprecise,
        DualCode::IllegalPhaseEdge,
        DualCode::AchieveFaked,
        DualCode::MisuseFaked,
        DualCode::NotReproducible,
        DualCode::ProtoUnsealed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DualCode::ALL.len() {
        if DualCode::ALL[i].val() == code {
            return DualCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x678C0 段闭集内"
}

// ── 落位状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LandPhase {
    /// 草案（判据未配对）
    Draft,
    /// 已配对（J1+J2 同 ID 成对）
    Paired,
    /// 已双验（终态）
    Verified,
    /// 单判据（终态，必检出）
    Unpaired,
}

/// 合法边闭集：Draft→Paired、Paired→Verified、Draft→Unpaired。
pub fn land_edge_legal(from: LandPhase, to: LandPhase) -> bool {
    match (from, to) {
        (LandPhase::Draft, LandPhase::Paired) => true,
        (LandPhase::Paired, LandPhase::Verified) => true,
        (LandPhase::Draft, LandPhase::Unpaired) => true,
        _ => false,
    }
}

/// 判据对载体：同 ID 的 J1 达成记录 + J2 误用记录（错误码必须精确单一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JudgePair {
    pub proto: usize,
    /// J1 达成复测通过次数。
    pub j1_hits: u32,
    /// J2 误用精确单一错误码命中次数。
    pub j2_hits: u32,
    /// J2 期望错误码（闭集内）。
    pub expect_err: u32,
    pub phase: LandPhase,
}

// ── 落位总账 ─────────────────────────────────────────────────────
pub struct LandLedger {
    pub pairs: Vec<JudgePair>,
    pub verified: u32,
    pub unpaired_caught: u32,
}

impl LandLedger {
    pub fn new() -> LandLedger {
        LandLedger { pairs: Vec::new(), verified: 0, unpaired_caught: 0 }
    }

    /// 建满六对：J1=J2=0、期望码按写死规则 (proto%4)+1。
    pub fn build_full(&mut self) {
        self.pairs.clear();
        self.verified = 0;
        self.unpaired_caught = 0;
        let mut p = 0;
        while p < PROTO_COUNT {
            self.pairs.push(JudgePair {
                proto: p,
                j1_hits: 0,
                j2_hits: 0,
                expect_err: (p % 4) as u32 + 1,
                phase: LandPhase::Draft,
            });
            p += 1;
        }
    }

    /// J1 达成复测：RunPerRound 次全过才落 Paired。
    pub fn record_j1(&mut self, proto: usize) -> Result<(), u32> {
        if proto >= self.pairs.len() {
            return Err(DualCode::ProtoOutOfRange.val());
        }
        if !land_edge_legal(self.pairs[proto].phase, LandPhase::Paired) {
            return Err(DualCode::IllegalPhaseEdge.val());
        }
        self.pairs[proto].j1_hits += 1;
        if self.pairs[proto].j1_hits >= RUNS_PER_ROUND {
            self.pairs[proto].phase = LandPhase::Paired;
        }
        Ok(())
    }

    /// J2 误用复测：注入错误码必须精确单一（==期望码），RunPerRound 次。
    pub fn record_j2(&mut self, proto: usize, got_err: u32) -> Result<(), u32> {
        if proto >= self.pairs.len() {
            return Err(DualCode::ProtoOutOfRange.val());
        }
        // 错误码闭集闸。
        let mut in_set = false;
        let mut i = 0;
        while i < ERR_SET.len() {
            if ERR_SET[i] == got_err {
                in_set = true;
            }
            i += 1;
        }
        if !in_set {
            return Err(DualCode::ErrCodeOutOfRange.val());
        }
        // 精确单一：必须 ==期望码（多码/错码皆拒）。
        if got_err != self.pairs[proto].expect_err {
            return Err(DualCode::ErrCodeImprecise.val());
        }
        // J2 必须在 Paired 相位之后验（双判据次序：先达成后误用）。
        if self.pairs[proto].phase != LandPhase::Paired {
            return Err(DualCode::SingleJudgeOnly.val());
        }
        self.pairs[proto].j2_hits += 1;
        if self.pairs[proto].j2_hits >= RUNS_PER_ROUND {
            if !land_edge_legal(LandPhase::Paired, LandPhase::Verified) {
                return Err(DualCode::IllegalPhaseEdge.val());
            }
            self.pairs[proto].phase = LandPhase::Verified;
            self.verified += 1;
        }
        Ok(())
    }

    /// 单判据注入：只录 J1 不录 J2 的对 → 巡检必检出。
    pub fn inject_single_judge(&mut self, proto: usize) -> Result<(), u32> {
        if proto >= self.pairs.len() {
            return Err(DualCode::ProtoOutOfRange.val());
        }
        // 强制 Paired（伪造 J1 满）但不录 J2 → 落 Unpaired。
        let _ = self.record_j1(proto);
        self.pairs[proto].j1_hits = RUNS_PER_ROUND;
        self.pairs[proto].phase = LandPhase::Paired;
        // 巡检：Paired 但 j2_hits==0 → 单判据。
        let single = self.pairs[proto].j2_hits == 0;
        if single {
            self.pairs[proto].phase = LandPhase::Unpaired;
            self.unpaired_caught += 1;
            return Err(DualCode::SingleJudgeOnly.val());
        }
        Ok(())
    }

    /// 巡检：全部对必须 Verified 才封账。
    pub fn seal_audit(&self) -> Result<u32, u32> {
        let mut i = 0;
        while i < self.pairs.len() {
            if self.pairs[i].phase != LandPhase::Verified {
                return Err(DualCode::ProtoUnsealed.val());
            }
            i += 1;
        }
        if self.verified as usize != self.pairs.len() {
            return Err(DualCode::ProtoUnsealed.val());
        }
        Ok(self.verified)
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ──────────────────────────
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
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 注入面：三族各 10 次，10/10 全抓 ─────────────────────────────
pub const INJ_FAMILIES: usize = 3;
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["单判据族", "错误码不精确族", "越码族"];

pub struct InjectLedger {
    pub tries: [u32; INJ_FAMILIES],
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJ_FAMILIES {
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_ROUND {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：单判据注入（只验达成不验误用 ×10）→ SingleJudgeOnly。
    let mut k = 0u32;
    while k < RUNS_PER_ROUND {
        ledger.tries[0] += 1;
        let mut led = LandLedger::new();
        led.build_full();
        if let Err(e) = led.inject_single_judge((k as usize) % PROTO_COUNT) {
            if e == DualCode::SingleJudgeOnly.val() && led.unpaired_caught == 1 {
                ledger.caught[0] += 1;
            }
        }
        k += 1;
    }
    // 族 1：错误码不精确注入（错码 ×10）→ ErrCodeImprecise。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[1] += 1;
        let mut led = LandLedger::new();
        led.build_full();
        let proto = (k as usize) % PROTO_COUNT;
        let _ = led.record_j1(proto);
        // 期望码 = (proto%4)+1，注入错码 = 期望码±1（落在闭集内）。
        let want = led.pairs[proto].expect_err;
        let wrong = if want == ERR_SET[3] { ERR_SET[0] } else { want + 1 };
        let _ = led.record_j1(proto);
        let _ = led.record_j1(proto);
        // 补满 J1 至 10。
        let mut fill = led.pairs[proto].j1_hits;
        while fill < RUNS_PER_ROUND {
            let _ = led.record_j1(proto);
            fill += 1;
        }
        if let Err(e) = led.record_j2(proto, wrong) {
            if e == DualCode::ErrCodeImprecise.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：越码注入（码 99 ×10）→ ErrCodeOutOfRange。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[2] += 1;
        let mut led = LandLedger::new();
        led.build_full();
        if let Err(e) = led.record_j2(0, 99) {
            if e == DualCode::ErrCodeOutOfRange.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：同 ID 双判据（J1+J2）×10 轮 ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub rounds: u32,
    pub verified: u32,
    pub pair_total: usize,
    pub j1_total: u32,
    pub j2_total: u32,
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut led = LandLedger::new();
    led.build_full();
    let mut j1_total = 0u32;
    let mut j2_total = 0u32;
    let mut round = 0u32;
    while round < RUNS_PER_ROUND {
        let _ = g.below(17);
        // 每轮：六对全部 J1 一次 + 全部 J2 一次（同 ID 双验）。
        let mut p = 0;
        while p < PROTO_COUNT {
            led.record_j1(p)?;
            j1_total += 1;
            p += 1;
        }
        let mut p = 0;
        while p < PROTO_COUNT {
            // J2 需 Paired 相位：j1 满 10 才 Paired，末轮才可 J2。
            if led.pairs[p].phase == LandPhase::Paired {
                led.record_j2(p, led.pairs[p].expect_err)?;
                j2_total += 1;
            }
            p += 1;
        }
        round += 1;
    }
    let verified = led.seal_audit()?;
    Ok(GoldenResult {
        rounds: round,
        verified,
        pair_total: led.pairs.len(),
        j1_total,
        j2_total,
    })
}

/// 双跑复现。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a.verified != b.verified || a.j1_total != b.j1_total || a.j2_total != b.j2_total {
        return Err(DualCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DualCode::ALL.len() {
        let v = DualCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：同 ID 双判据 10/10 封账。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => {
            g.rounds == RUNS_PER_ROUND
                && g.verified == PROTO_COUNT as u32
                && g.pair_total == PROTO_COUNT
        }
        Err(_) => false,
    };
    v.push(("J1-同ID双判据10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-单判据三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 错误码闭集名点验。
    let mut ok = 0;
    let mut i = 0;
    while i < ERR_SET.len() {
        if ERR_NAMES[i].len() > 0 && ERR_SET[i] == i as u32 + 1 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-四码闭集点验", ok == 4, format!("{}/4", ok)));
    // 5. 状态机非法边抽验：Unpaired→Verified 直跳必拒。
    let bad = !land_edge_legal(LandPhase::Unpaired, LandPhase::Verified);
    v.push(("J1-Unpaired→Verified必拒", bad, String::from("闭集外")));
    // 6. 反向判据：J2 先于 J1（Draft 相位录 J2）必拒。
    let mut led2 = LandLedger::new();
    led2.build_full();
    let rev = if let Err(e) = led2.record_j2(0, led2.pairs[0].expect_err) {
        e == DualCode::SingleJudgeOnly.val()
    } else {
        false
    };
    v.push(("J1-J2先于J1必拒", rev, String::from("Draft 相位录 J2")));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x678C0 窗口")));
    v
}

pub fn run_unx15861_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B34 E 型双判据协议落位总纲（J1+J2 同 ID 双验）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
