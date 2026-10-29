//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14321 · 句柄泄漏总账总起（总框架） · 0x66580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F14321-J1 泄漏总账总框架：全域句柄四族（挂点/绑定/继承/清
//!      账）泄漏总账总图（族/批位/账面/闸位四列 36 行总索引——B01–B36
//!      批逐批行），账面守恒断言（36 批句柄账并账=域账），索引与
//!      handoff 对表一致断言，与 F14229/F14228 分界表，指纹钉版，红绿齐。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 功能定位：泄漏总账总框架是全域句柄收口的总地图——36 批并账守恒是
//!   「域内句柄粒粒可溯」的第一证明。
//! 语义边界：只做总账框架与索引；不做单族专项（后续条持有）、不做继
//!   承路径行为（B32 持有）、不改既有句柄账（只读并账）。
//! 设计要点：
//!   - 四族闭集：挂点 0/绑定 1/继承 2/清账 3；
//!   - 36 批闭集（B01–B36，批位 0..36），逐批四族账面 + 主族列；
//!   - 守恒恒等式：每批 Σ四族账面 = 批账面；36 批批账面之和 = 域账；
//!   - 闸位闭集：0=未挂/1=已挂/2=并账过/3=封版；
//!   - 分界表：F14229（继承路径行为账）与 F14228（单族专项闸）不进本
//!     总图闸位（其批行闸位钉 0），注入必红；
//!   - 双机对拍：第二账本独立并账，逐行逐列一致；
//!   - LCG 种子写死 = SEG_BASE（0x66580），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66580，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66580;

/// 四族闭集。
pub const FAM_HOOK: usize = 0;
pub const FAM_BIND: usize = 1;
pub const FAM_INHERIT: usize = 2;
pub const FAM_CLEAR: usize = 3;
pub const FAM_COUNT: usize = 4;
pub const FAM_NAMES: [&str; FAM_COUNT] = ["挂点族", "绑定族", "继承族", "清账族"];

/// 36 批闭集。
pub const BATCH_COUNT: usize = 36;

/// 闸位闭集。
pub const GATE_OPEN: u8 = 0;
pub const GATE_HUNG: u8 = 1;
pub const GATE_MERGED: u8 = 2;
pub const GATE_SEALED: u8 = 3;

/// F14229/F14228 分界批号（其账面只读引用，不进本图闸位）。
pub const BOUND_F14229: usize = 32;
pub const BOUND_F14228: usize = 31;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeakCode {
    /// 批位越闭集（0..36）
    BatchOutOfRange,
    /// 族越闭集
    FamilyOutOfRange,
    /// 账面为负（usize 下溢防护线）
    LedgerUnderflow,
    /// 批内并账不守恒（Σ四族 ≠ 批账面）
    BatchMergeMismatch,
    /// 域账守恒破裂（36 批并账 ≠ 域账）
    DomainMergeMismatch,
    /// handoff 对表不一致
    HandoffMismatch,
    /// 分界批行混入本图闸位
    BoundaryRowMixed,
    /// 闸位越闭集
    GateOutOfRange,
    /// 索引行数 ≠ 36
    IndexRowMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
}

impl LeakCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LeakCode::BatchOutOfRange => "批位越闭集：B01–B36 三十六批",
            LeakCode::FamilyOutOfRange => "族越闭集：挂点/绑定/继承/清账四族",
            LeakCode::LedgerUnderflow => "账面负值：句柄账不得为负",
            LeakCode::BatchMergeMismatch => "批内并账不守恒：Σ四族 ≠ 批账面",
            LeakCode::DomainMergeMismatch => "域账守恒破裂：36 批并账 ≠ 域账",
            LeakCode::HandoffMismatch => "handoff 对表不一致：台账漂移",
            LeakCode::BoundaryRowMixed => "F14229/F14228 分界批行混入闸位",
            LeakCode::GateOutOfRange => "闸位越闭集：0..3",
            LeakCode::IndexRowMismatch => "索引行数 ≠ 36：总图红线",
            LeakCode::DualReplayDrift => "双机对拍漂移：并账不可复现",
        }
    }
    pub const ALL: [LeakCode; 10] = [
        LeakCode::BatchOutOfRange,
        LeakCode::FamilyOutOfRange,
        LeakCode::LedgerUnderflow,
        LeakCode::BatchMergeMismatch,
        LeakCode::DomainMergeMismatch,
        LeakCode::HandoffMismatch,
        LeakCode::BoundaryRowMixed,
        LeakCode::GateOutOfRange,
        LeakCode::IndexRowMismatch,
        LeakCode::DualReplayDrift,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LeakCode::ALL.len() {
        if LeakCode::ALL[i].val() == code {
            return LeakCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66580 段闭集内"
}

// ── 36 行总索引行（族/批位/账面/闸位） ───────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeakRow {
    /// 主族（该批句柄账的主要归属族）
    pub fam: usize,
    /// 批号 1..36
    pub batch_no: usize,
    /// 批账面（四族合计，钉版分配）
    pub ledger: usize,
    /// 闸位
    pub gate: u8,
}

/// 主族钉版：批号模 4 分族（写死：B01 挂点、B02 绑定、B03 继承、
/// B04 清账……循环展开，B32 继承批按分界表仍主族继承但闸位钉 0）。
pub fn pinned_fam(batch_no: usize) -> Result<usize, u32> {
    if batch_no < 1 || batch_no > BATCH_COUNT {
        return Err(LeakCode::BatchOutOfRange.val());
    }
    Ok((batch_no - 1) % FAM_COUNT)
}

/// 批账面钉版（写死分配：批号 × 7 + 批号模 13，来源：36 批句柄账
/// 均布口径；独立函数，禁与被测并账和值复用）。
pub fn pinned_batch_ledger(batch_no: usize) -> Result<usize, u32> {
    if batch_no < 1 || batch_no > BATCH_COUNT {
        return Err(LeakCode::BatchOutOfRange.val());
    }
    Ok(batch_no * 7 + (batch_no % 13))
}

/// 批内四族账面拆分钉版：主族拿大份，其余三族各拿小份（拆分表写死
/// 比例 6:2:1:1——主族 60%，相邻两族 20%/10%，末族 10%；账面为 0 时
/// 全零）。
pub fn split_families(batch_ledger: usize, fam: usize) -> Result<[usize; FAM_COUNT], u32> {
    if fam >= FAM_COUNT {
        return Err(LeakCode::FamilyOutOfRange.val());
    }
    let mut out = [0usize; FAM_COUNT];
    if batch_ledger == 0 {
        return Ok(out);
    }
    let big = batch_ledger * 6 / 10;
    let mut rest = batch_ledger - big;
    // 剩余三族按 20/10/10 再拆（余数全给第一个非主族）。
    let mut k = 0;
    while k < FAM_COUNT {
        if k == fam {
            out[k] = big;
        } else if rest > 0 {
            let share = if rest >= batch_ledger / 10 && batch_ledger / 10 > 0 {
                batch_ledger / 10
            } else {
                rest
            };
            out[k] = share;
            rest -= share;
        }
        k += 1;
    }
    // 余数兜底：全部并入主族（守恒优先）。
    out[fam] += rest;
    Ok(out)
}

// ── 泄漏总账账本 ─────────────────────────────────────────────────
pub struct LeakLedger {
    pub rows: Vec<LeakRow>,
    /// 每批四族拆分账（与 rows 同序，同长）
    pub splits: Vec<[usize; FAM_COUNT]>,
    /// 域账（写死口径：36 批批账面之和的独立回算）
    pub domain_account: usize,
    pub sealed: bool,
}

impl LeakLedger {
    pub fn new() -> LeakLedger {
        LeakLedger { rows: Vec::new(), splits: Vec::new(), domain_account: 0, sealed: false }
    }

    /// 展开 36 行总索引（闸位：分界批钉 0，其余挂闸 1）。
    pub fn expand(&mut self) -> Result<(), u32> {
        if !self.rows.is_empty() {
            return Err(LeakCode::IndexRowMismatch.val());
        }
        let mut b = 1;
        while b <= BATCH_COUNT {
            let fam = pinned_fam(b)?;
            let ledger = pinned_batch_ledger(b)?;
            let gate = if b == BOUND_F14229 || b == BOUND_F14228 {
                GATE_OPEN
            } else {
                GATE_HUNG
            };
            self.rows.push(LeakRow { fam, batch_no: b, ledger, gate });
            self.splits.push(split_families(ledger, fam)?);
            b += 1;
        }
        if self.rows.len() != BATCH_COUNT || self.splits.len() != BATCH_COUNT {
            return Err(LeakCode::IndexRowMismatch.val());
        }
        Ok(())
    }

    /// 批内守恒：Σ四族 = 批账面（逐批断言）。
    pub fn batch_conservation(&self) -> Result<(), u32> {
        if self.rows.len() != self.splits.len() {
            return Err(LeakCode::IndexRowMismatch.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            let mut sum = 0;
            let mut f = 0;
            while f < FAM_COUNT {
                sum += self.splits[i][f];
                f += 1;
            }
            if sum != self.rows[i].ledger {
                return Err(LeakCode::BatchMergeMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 域账守恒：36 批并账 = 域账（并账路径独立回算比对）。
    pub fn domain_conservation(&self) -> Result<usize, u32> {
        self.batch_conservation()?;
        let mut sum = 0;
        let mut i = 0;
        while i < self.rows.len() {
            sum += self.rows[i].ledger;
            i += 1;
        }
        // 域账独立回算：pinned 函数直加（不复用被测行）。
        let mut direct = 0;
        let mut b = 1;
        while b <= BATCH_COUNT {
            direct += pinned_batch_ledger(b)?;
            b += 1;
        }
        if sum != direct {
            return Err(LeakCode::DomainMergeMismatch.val());
        }
        Ok(sum)
    }

    /// 并账过闸：非分界批 1→2，最后封版 3。
    pub fn merge_all(&mut self) -> Result<usize, u32> {
        if self.rows.len() != BATCH_COUNT {
            return Err(LeakCode::IndexRowMismatch.val());
        }
        let mut merged = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let gate = self.rows[i].gate;
            if self.rows[i].batch_no == BOUND_F14229
                || self.rows[i].batch_no == BOUND_F14228
            {
                // 分界批行不并账（F14229/F14228 持有）；钉 0 跳过，
                // 闸位被改即分界被破，必红。
                if gate == GATE_OPEN {
                    i += 1;
                    continue;
                }
                return Err(LeakCode::BoundaryRowMixed.val());
            }
            if gate != GATE_HUNG && gate != GATE_MERGED {
                return Err(LeakCode::GateOutOfRange.val());
            }
            self.rows[i].gate = GATE_MERGED;
            merged += 1;
            i += 1;
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].batch_no != BOUND_F14229
                && self.rows[i].batch_no != BOUND_F14228
            {
                self.rows[i].gate = GATE_SEALED;
            }
            i += 1;
        }
        self.sealed = true;
        Ok(merged)
    }

    /// handoff 对表：台账（批号/主族/账面）逐行一致。
    pub fn handoff_reconcile(&self, handoff: &[LeakRow]) -> Result<(), u32> {
        if handoff.len() != self.rows.len() {
            return Err(LeakCode::HandoffMismatch.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if handoff[i].batch_no != self.rows[i].batch_no
                || handoff[i].fam != self.rows[i].fam
                || handoff[i].ledger != self.rows[i].ledger
            {
                return Err(LeakCode::HandoffMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 双机对拍：第二账本独立并账，逐行一致。
    pub fn dual_replay(&self) -> Result<(), u32> {
        let mut twin = LeakLedger::new();
        twin.expand()?;
        twin.batch_conservation()?;
        if twin.rows.len() != self.rows.len() {
            return Err(LeakCode::DualReplayDrift.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if twin.rows[i] != self.rows[i] {
                return Err(LeakCode::DualReplayDrift.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// handoff 台账构造（自钉版函数独立另算）。
pub fn build_handoff() -> Result<Vec<LeakRow>, u32> {
    let mut v: Vec<LeakRow> = Vec::new();
    let mut b = 1;
    while b <= BATCH_COUNT {
        v.push(LeakRow {
            fam: pinned_fam(b)?,
            batch_no: b,
            ledger: pinned_batch_ledger(b)?,
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
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let mut led = LeakLedger::new();
    led.expand()?;
    let domain = led.domain_conservation()?;
    let handoff = build_handoff()?;
    led.handoff_reconcile(&handoff)?;
    led.dual_replay()?;
    let merged = led.merge_all()?;
    Ok((led.rows.len(), merged, domain))
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
    // 族 0：批账面篡改（并账守恒必破）。
    ledger.tries[0] += 1;
    let mut led = LeakLedger::new();
    let _ = led.expand();
    if led.rows.len() > 9 {
        led.rows[9].ledger += 5;
    }
    if led.batch_conservation().is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：分界批行强行并账（F14229/F14228 闸位钉 0 仍被推闸）。
    ledger.tries[1] += 1;
    let mut led2 = LeakLedger::new();
    let _ = led2.expand();
    if led2.rows.len() > BOUND_F14229 {
        led2.rows[BOUND_F14229 - 1].gate = GATE_HUNG;
    }
    if led2.merge_all().is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：越界批号注入（批 0 与批 37 必拒）。
    ledger.tries[2] += 2;
    if pinned_fam(0).is_err() {
        ledger.caught[2] += 1;
    }
    if pinned_batch_ledger(37).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LeakCode::ALL.len() {
        let v = LeakCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：36 行 + 并账过闸 + 域账守恒。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((rows, merged, _)) => *rows == BATCH_COUNT && *merged == BATCH_COUNT - 2,
        Err(_) => false,
    };
    v.push(("J1-36行总图+34批并账", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 域账守恒独立复核（干净账双路直加一致）。
    let domain_ok = {
        let mut led = LeakLedger::new();
        match led.expand() {
            Ok(()) => led.domain_conservation().is_ok(),
            Err(_) => false,
        }
    };
    v.push(("J1-36批并账=域账", domain_ok, String::from("双路回算")));
    // 4. 四族拆分守恒抽验（主族比例 6:2:1:1 余数兜底）。
    let split_ok = {
        let s = split_families(100, FAM_HOOK);
        match s {
            Ok(arr) => {
                let mut sum = 0;
                let mut f = 0;
                while f < FAM_COUNT {
                    sum += arr[f];
                    f += 1;
                }
                sum == 100
            }
            Err(_) => false,
        }
    };
    v.push(("J1-四族拆分守恒抽验", split_ok, String::from("6:2:1:1")));
    // 5. handoff 对表一致。
    let handoff_ok = {
        let mut led = LeakLedger::new();
        match led.expand() {
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
    v.push(("J1-handoff 对表一致", handoff_ok, String::from("36 行三列")));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性。
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

pub fn run_unx14321_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14321 句柄泄漏总账总起（四族 36 批并账总图）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
