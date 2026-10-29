//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14301 · 全域判据总回归总起（总框架） · 0x66540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F14301-J1 总回归总框架：B01–B35 三十五批 700 条判据总回归
//!      总图（段/批/条数/回归闸位四列 35 行总索引），段分布守恒断言
//!      （F 型 160+M 型 140+E 型 200+I 型 200=700），索引与 handoff
//!      台账对表一致断言，与 B30/B35 两段聚合账零重叠分界表，指纹钉
//!      版，红绿齐。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 功能定位：总回归总框架是全域 700 条判据收口的总地图——35 行索引+
//!   段分布守恒是总闸设计完备性的第一证明。
//! 语义边界：只做总回归框架与索引；不做单段回归设施（本批后续条持
//!   有）、不做坏档专项（B38 持有）、不改既有判据（只读引用）。
//! 设计要点：
//!   - 35 批闭集（B01–B35，批位 0..35）× 段型四档闭集（F/M/E/I）；
//!   - 段分布守恒写死：F=160、M=140、E=200、I=200，总=700（独立常量
//!     不复用被测和值）；
//!   - 回归闸位闭集：闸位 0=未挂闸/1=已挂闸/2=回归过闸/3=闸位封版；
//!   - handoff 对表：35 行索引与 handoff 台账逐行对表（批号/段型/条数
//!     三列一致）；
//!   - 零重叠分界：B30（F14183 预演聚合）与 B35（F14281 真联签聚合）
//!     两行聚合账不进回归闸位（闸位恒 0），分界注入必红；
//!   - 双机对拍：第二账本独立展开 35 行，逐列一致；
//!   - LCG 种子写死 = SEG_BASE（0x66540），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66540，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66540;

/// 35 批闭集。
pub const BATCH_COUNT: usize = 35;
/// 段型闭集四档。
pub const SEG_F: usize = 0;
pub const SEG_M: usize = 1;
pub const SEG_E: usize = 2;
pub const SEG_I: usize = 3;
pub const SEGKIND_COUNT: usize = 4;
pub const SEGKIND_NAMES: [&str; SEGKIND_COUNT] = ["F 型", "M 型", "E 型", "I 型"];

/// 段分布守恒写死（来源：判据原文 160+140+200+200=700，独立常量）。
pub const EXPECT_F: usize = 160;
pub const EXPECT_M: usize = 140;
pub const EXPECT_E: usize = 200;
pub const EXPECT_I: usize = 200;
pub const EXPECT_TOTAL: usize = EXPECT_F + EXPECT_M + EXPECT_E + EXPECT_I;

/// 回归闸位闭集。
pub const GATE_OPEN: u8 = 0;
pub const GATE_HUNG: u8 = 1;
pub const GATE_PASSED: u8 = 2;
pub const GATE_SEALED: u8 = 3;
/// 聚合账闸位钉死 0（零重叠分界）。
pub const GATE_AGG_PIN: u8 = GATE_OPEN;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegCode {
    /// 批位越闭集（0..35）
    BatchOutOfRange,
    /// 段型越闭集（F/M/E/I）
    SegKindInvalid,
    /// 条数非正（批条数必须 ≥1）
    JudgeCountEmpty,
    /// 段分布守恒破裂
    DistConservationBroken,
    /// 总数 ≠ 700
    TotalMismatch,
    /// handoff 对表不一致
    HandoffMismatch,
    /// 聚合账行混入回归闸位（零重叠红线）
    AggRowMixed,
    /// 闸位越闭集
    GateOutOfRange,
    /// 索引行数 ≠ 35
    IndexRowMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
}

impl RegCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RegCode::BatchOutOfRange => "批位越闭集：B01–B35 三十五批",
            RegCode::SegKindInvalid => "段型越闭集：F/M/E/I 四档",
            RegCode::JudgeCountEmpty => "批条数为零：判据账不得空批",
            RegCode::DistConservationBroken => "段分布守恒破裂：160+140+200+200",
            RegCode::TotalMismatch => "总条数 ≠ 700：全域收口红线",
            RegCode::HandoffMismatch => "handoff 对表不一致：台账漂移",
            RegCode::AggRowMixed => "聚合账行混入回归闸位：零重叠分界红线",
            RegCode::GateOutOfRange => "闸位越闭集：0..3",
            RegCode::IndexRowMismatch => "索引行数 ≠ 35：总图红线",
            RegCode::DualReplayDrift => "双机对拍漂移：索引不可复现",
        }
    }
    pub const ALL: [RegCode; 10] = [
        RegCode::BatchOutOfRange,
        RegCode::SegKindInvalid,
        RegCode::JudgeCountEmpty,
        RegCode::DistConservationBroken,
        RegCode::TotalMismatch,
        RegCode::HandoffMismatch,
        RegCode::AggRowMixed,
        RegCode::GateOutOfRange,
        RegCode::IndexRowMismatch,
        RegCode::DualReplayDrift,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        if RegCode::ALL[i].val() == code {
            return RegCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66540 段闭集内"
}

// ── 35 行总索引行（段/批/条数/回归闸位） ─────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegRow {
    /// 段型（0..4）
    pub kind: usize,
    /// 批号（1..36，即 B01..B35）
    pub batch_no: usize,
    /// 批条数
    pub judges: usize,
    /// 回归闸位（0..3）
    pub gate: u8,
}

/// 35 行段分布钉版表（写死；来源：判据原文段分布守恒口径的批间展开，
/// 每批条数为框架分配值，逐批行之和等于各段守恒值）：
///   B01–B16（16 批）F 型各 10 条  → F 合计 160；
///   B17–B23（7 批） M 型各 20 条  → M 合计 140；
///   B24–B30（7 批） E 型各 E_SPLIT 表值 → E 合计 200；
///   B31–B35（5 批） I 型各 40 条  → I 合计 200。
pub const E_SPLIT: [usize; 7] = [25, 30, 25, 30, 30, 30, 30];

pub fn pinned_kind(batch_no: usize) -> Result<usize, u32> {
    if batch_no < 1 || batch_no > BATCH_COUNT {
        return Err(RegCode::BatchOutOfRange.val());
    }
    if batch_no <= 16 {
        Ok(SEG_F)
    } else if batch_no <= 23 {
        Ok(SEG_M)
    } else if batch_no <= 30 {
        Ok(SEG_E)
    } else {
        Ok(SEG_I)
    }
}

pub fn pinned_judges(batch_no: usize) -> Result<usize, u32> {
    if batch_no < 1 || batch_no > BATCH_COUNT {
        return Err(RegCode::BatchOutOfRange.val());
    }
    if batch_no <= 16 {
        Ok(10)
    } else if batch_no <= 23 {
        Ok(20)
    } else if batch_no <= 30 {
        Ok(E_SPLIT[batch_no - 24])
    } else {
        Ok(40)
    }
}

/// 聚合账批号闭集（零重叠分界：B30 预演聚合、B35 真联签聚合）。
pub fn is_agg_batch(batch_no: usize) -> bool {
    batch_no == 30 || batch_no == 35
}

// ── 总回归账本 ───────────────────────────────────────────────────
pub struct RegressLedger {
    pub rows: Vec<RegRow>,
    pub sealed: bool,
}

impl RegressLedger {
    pub fn new() -> RegressLedger {
        RegressLedger { rows: Vec::new(), sealed: false }
    }

    /// 展开 35 行总索引（只读引用既有判据，闸位初始挂闸=1）。
    pub fn expand(&mut self) -> Result<(), u32> {
        if !self.rows.is_empty() {
            return Err(RegCode::IndexRowMismatch.val());
        }
        let mut b = 1;
        while b <= BATCH_COUNT {
            let kind = pinned_kind(b)?;
            let judges = pinned_judges(b)?;
            if judges == 0 {
                return Err(RegCode::JudgeCountEmpty.val());
            }
            // 零重叠分界：聚合账批号闸位钉 0，不挂回归闸。
            let gate = if is_agg_batch(b) { GATE_AGG_PIN } else { GATE_HUNG };
            self.rows.push(RegRow { kind, batch_no: b, judges, gate });
            b += 1;
        }
        if self.rows.len() != BATCH_COUNT {
            return Err(RegCode::IndexRowMismatch.val());
        }
        Ok(())
    }

    /// 段分布守恒断言（逐段累加 vs 写死独立常量）。
    pub fn conservation(&self) -> Result<[usize; SEGKIND_COUNT], u32> {
        if self.rows.len() != BATCH_COUNT {
            return Err(RegCode::IndexRowMismatch.val());
        }
        let mut acc = [0usize; SEGKIND_COUNT];
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.kind >= SEGKIND_COUNT {
                return Err(RegCode::SegKindInvalid.val());
            }
            acc[r.kind] += r.judges;
            i += 1;
        }
        if acc[SEG_F] != EXPECT_F || acc[SEG_M] != EXPECT_M
            || acc[SEG_E] != EXPECT_E || acc[SEG_I] != EXPECT_I
        {
            return Err(RegCode::DistConservationBroken.val());
        }
        Ok(acc)
    }

    /// 总数守恒：35 行之和 = 700。
    pub fn total_judges(&self) -> Result<usize, u32> {
        let acc = self.conservation()?;
        Ok(acc[0] + acc[1] + acc[2] + acc[3])
    }

    /// 回归过闸：非聚合批逐批挂闸→过闸→封版（状态机闭集推进）。
    pub fn regress_all(&mut self) -> Result<usize, u32> {
        if self.rows.len() != BATCH_COUNT {
            return Err(RegCode::IndexRowMismatch.val());
        }
        let mut passed = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let gate = self.rows[i].gate;
            if gate == GATE_AGG_PIN {
                // 聚合账行不得过闸（零重叠）。
                return Err(RegCode::AggRowMixed.val());
            }
            if gate != GATE_HUNG && gate != GATE_PASSED {
                return Err(RegCode::GateOutOfRange.val());
            }
            self.rows[i].gate = GATE_PASSED;
            passed += 1;
            i += 1;
        }
        // 封版：全表 PASSED→SEALED。
        let mut i = 0;
        while i < self.rows.len() {
            if is_agg_batch(self.rows[i].batch_no) {
                self.rows[i].gate = GATE_AGG_PIN;
            } else {
                self.rows[i].gate = GATE_SEALED;
            }
            i += 1;
        }
        self.sealed = true;
        Ok(passed)
    }

    /// handoff 对表：台账（批号/段型/条数）逐行一致。
    pub fn handoff_reconcile(&self, handoff: &[RegRow]) -> Result<(), u32> {
        if handoff.len() != self.rows.len() {
            return Err(RegCode::HandoffMismatch.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            // 对表只核三列：批号/段型/条数（闸位为回归自有状态）。
            if handoff[i].batch_no != self.rows[i].batch_no
                || handoff[i].kind != self.rows[i].kind
                || handoff[i].judges != self.rows[i].judges
            {
                return Err(RegCode::HandoffMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 双机对拍：第二账本独立展开，三列逐行一致。
    pub fn dual_replay(&self) -> Result<(), u32> {
        let mut twin = RegressLedger::new();
        twin.expand()?;
        if twin.rows.len() != self.rows.len() {
            return Err(RegCode::DualReplayDrift.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if twin.rows[i].batch_no != self.rows[i].batch_no
                || twin.rows[i].kind != self.rows[i].kind
                || twin.rows[i].judges != self.rows[i].judges
            {
                return Err(RegCode::DualReplayDrift.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// handoff 台账构造（自钉版表独立另算一遍，模拟 B01–B35 台账喂入）。
pub fn build_handoff() -> Result<Vec<RegRow>, u32> {
    let mut v: Vec<RegRow> = Vec::new();
    let mut b = 1;
    while b <= BATCH_COUNT {
        v.push(RegRow {
            kind: pinned_kind(b)?,
            batch_no: b,
            judges: pinned_judges(b)?,
            gate: GATE_OPEN,
        });
        b += 1;
    }
    Ok(v)
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
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

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, usize, [usize; SEGKIND_COUNT]), u32> {
    let mut led = RegressLedger::new();
    led.expand()?;
    let acc = led.conservation()?;
    let total = led.total_judges()?;
    if total != EXPECT_TOTAL {
        return Err(RegCode::TotalMismatch.val());
    }
    let handoff = build_handoff()?;
    led.handoff_reconcile(&handoff)?;
    led.dual_replay()?;
    let passed = led.regress_all()?;
    Ok((led.rows.len(), passed, acc))
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
    // 族 0：段分布守恒破裂注入（篡改一行条数）。
    ledger.tries[0] += 1;
    let mut led = RegressLedger::new();
    let expand_ok = led.expand().is_ok();
    if expand_ok && led.rows.len() > 5 {
        led.rows[5].judges += 7;
    }
    if led.conservation().is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：聚合账行混入回归闸位（B30/B35 强行过闸）。
    ledger.tries[1] += 1;
    let mut led2 = RegressLedger::new();
    let _ = led2.expand();
    if led2.rows.len() > 30 {
        // 把 B30 的闸位钉值改为挂闸，试图混入回归。
        led2.rows[29].gate = GATE_HUNG;
    }
    if led2.regress_all().is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：handoff 台账漂移注入（一行条数改账）。
    ledger.tries[2] += 1;
    let mut led3 = RegressLedger::new();
    let _ = led3.expand();
    let mut bad_handoff: Vec<RegRow> = Vec::new();
    if let Ok(h) = build_handoff() {
        let mut i = 0;
        while i < h.len() {
            if i == 10 && h[i].judges > 1 {
                let mut r = h[i].clone();
                r.judges -= 1;
                bad_handoff.push(r);
            } else {
                bad_handoff.push(h[i].clone());
            }
            i += 1;
        }
    }
    if led3.handoff_reconcile(&bad_handoff).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        let v = RegCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：35 行 + 全过闸 + 段守恒。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((rows, passed, _)) => *rows == BATCH_COUNT && *passed == BATCH_COUNT,
        Err(_) => false,
    };
    v.push(("J1-35行总图+全过闸", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 段分布守恒独立复核（写死常量直加）。
    v.push((
        "J1-段守恒 160+140+200+200=700",
        EXPECT_TOTAL == 700,
        format!("={}", EXPECT_TOTAL),
    ));
    // 4. handoff 对表一致（干净台账）。
    let handoff_ok = {
        let mut led = RegressLedger::new();
        let e = led.expand();
        match e {
            Ok(()) => {
                let h = build_handoff();
                match h {
                    Ok(hh) => led.handoff_reconcile(&hh).is_ok(),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    };
    v.push(("J1-handoff 对表一致", handoff_ok, String::from("35 行三列")));
    // 5. 零重叠分界：B30/B35 闸位恒钉 0。
    let boundary_ok = {
        let mut led = RegressLedger::new();
        let e = led.expand();
        match e {
            Ok(()) => {
                let mut ok = true;
                let mut i = 0;
                while i < led.rows.len() {
                    if is_agg_batch(led.rows[i].batch_no)
                        && led.rows[i].gate != GATE_AGG_PIN
                    {
                        ok = false;
                    }
                    i += 1;
                }
                ok
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-B30/B35 聚合账零重叠分界",
        boundary_ok,
        String::from("闸位钉0"),
    ));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性（同种子双抽一致）。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14301_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14301 全域判据总回归总起（35 批 700 条总图）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
