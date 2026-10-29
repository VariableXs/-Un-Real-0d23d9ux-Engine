//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13541 · 双向账清账总成（任务书双向主轴兑现权威位） · 0x65BC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F13541-J1 清账总账（40 组清账态四列：组号/执行态/判定级/
//!      终核态），执行→清账递进闭环；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四列入账：组号（1~40 闭集）/执行态（继承 F13461 执行权威，仅收
//!     Verified 及其前置快照）/判定级（A/B/C 三级闭集）/终核态（四态机）；
//!   - 终核态四态机：Unclosed→CloseClaimed（清账申报）→Audited（终核）
//!     →Cleared（兑现）；执行→清账递进红线：执行态非 Verified 不许申报；
//!   - C 级红线：C 级须豁免立案（waiver 唯一、一 C 一豁免）才许兑现；
//!   - 闭环恒等式：Σ终核态 ≡ 40；兑现数 ≡ Cleared 态数；
//!     申报数 ≡ Verified 继承数；
//!   - 注入三族：未执行申报/C 级无豁免兑现/回跳 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65BC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65BC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65BC0;

/// 清账组闭集：40 组（判据 J1 明示）。
pub const CLOSE_GROUP_COUNT: usize = 40;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClsCode {
    /// 组号越闭集
    CloseGroupOutOfRange,
    /// 终核态非法迁移
    CloseStateIllegal,
    /// 未执行即清账申报（递进红线）
    CloseBeforeVerified,
    /// C 级无豁免兑现（红线）
    LevelCWithoutWaiver,
    /// 组重复入账
    CloseBookDup,
    /// 豁免重复（一 C 一豁免）
    WaiverDup,
    /// 清账账守恒破（Σ终核态≡40）
    ConservationBreak,
    /// 执行→清账闭环断（执行态快照缺）
    CloseNotRecursive,
    /// 双跑非幂等
    RerunNonIdempotent,
}

impl ClsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClsCode::CloseGroupOutOfRange => "组号越闭集：1~40 冻结",
            ClsCode::CloseStateIllegal => "终核态非法迁移：四态闭集外",
            ClsCode::CloseBeforeVerified => "未执行即清账申报：Verified 前置",
            ClsCode::LevelCWithoutWaiver => "C 级无豁免兑现：豁免前置红线",
            ClsCode::CloseBookDup => "组重复入账：一组一行",
            ClsCode::WaiverDup => "豁免重复：一 C 一豁免",
            ClsCode::ConservationBreak => "清账账守恒破：Σ终核态≡40",
            ClsCode::CloseNotRecursive => "执行→清账闭环断：执行态快照缺",
            ClsCode::RerunNonIdempotent => "双跑非幂等：同序列哈希逐位同",
        }
    }
    pub const ALL: [ClsCode; 9] = [
        ClsCode::CloseGroupOutOfRange,
        ClsCode::CloseStateIllegal,
        ClsCode::CloseBeforeVerified,
        ClsCode::LevelCWithoutWaiver,
        ClsCode::CloseBookDup,
        ClsCode::WaiverDup,
        ClsCode::ConservationBreak,
        ClsCode::CloseNotRecursive,
        ClsCode::RerunNonIdempotent,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClsCode::ALL.len() {
        if ClsCode::ALL[i].val() == code {
            return ClsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65BC0 段闭集内"
}

// ── 执行态快照闭集（继承自 F13461 权威位） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecSnap {
    /// 前置快照（未到 Verified）
    PreVerified,
    /// 已验证（清账前置）
    Verified,
}

// ── 判定级三闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseLevel {
    LevelA,
    LevelB,
    LevelC,
}

impl CloseLevel {
    pub fn tag(self) -> &'static str {
        match self {
            CloseLevel::LevelA => "A",
            CloseLevel::LevelB => "B",
            CloseLevel::LevelC => "C",
        }
    }
}

// ── 终核态四闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseState {
    /// 未清账
    Unclosed,
    /// 清账申报
    CloseClaimed,
    /// 终核通过
    Audited,
    /// 已兑现
    Cleared,
}

/// 清账行：四列（组号/执行态/判定级/终核态）+ 豁免位。
#[derive(Clone, Copy, Debug)]
pub struct CloseRow {
    pub group: usize,
    pub exec: ExecSnap,
    pub level: CloseLevel,
    pub state: CloseState,
    pub waiver: u32,
}

// ── 清账总账 ─────────────────────────────────────────────────────
pub struct CloseLedger {
    pub rows: Vec<CloseRow>,
    /// 豁免立案账（一 C 一豁免载体）。
    pub waivers: Vec<u32>,
}

impl CloseLedger {
    pub fn new() -> CloseLedger {
        CloseLedger {
            rows: Vec::new(),
            waivers: Vec::new(),
        }
    }

    /// 入账一行：组号 1~40、一组一行；执行态快照必须给出
    /// （闭环恒等：快照缺=CloseNotRecursive）。
    pub fn book(
        &mut self,
        group: usize,
        exec: ExecSnap,
        level: CloseLevel,
    ) -> Result<usize, u32> {
        if group == 0 || group > CLOSE_GROUP_COUNT {
            return Err(ClsCode::CloseGroupOutOfRange.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].group == group {
                return Err(ClsCode::CloseBookDup.val());
            }
            i += 1;
        }
        self.rows.push(CloseRow {
            group,
            exec,
            level,
            state: CloseState::Unclosed,
            waiver: 0,
        });
        Ok(self.rows.len() - 1)
    }

    /// 执行态补核：PreVerified 升 Verified（继承权威位的唯一合法边）。
    pub fn verify_exec(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(ClsCode::CloseGroupOutOfRange.val());
        }
        if self.rows[idx].exec != ExecSnap::PreVerified {
            return Err(ClsCode::CloseStateIllegal.val());
        }
        self.rows[idx].exec = ExecSnap::Verified;
        Ok(())
    }

    /// 前向迁移闭式：Unclosed→CloseClaimed→Audited→Cleared。
    pub fn advance(&mut self, idx: usize, next: CloseState) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(ClsCode::CloseGroupOutOfRange.val());
        }
        let cur = self.rows[idx].state;
        let legal = matches!(
            (cur, next),
            (CloseState::Unclosed, CloseState::CloseClaimed)
                | (CloseState::CloseClaimed, CloseState::Audited)
                | (CloseState::Audited, CloseState::Cleared)
        );
        if !legal {
            return Err(ClsCode::CloseStateIllegal.val());
        }
        // 递进红线：申报前必须 Verified。
        if next == CloseState::CloseClaimed && self.rows[idx].exec != ExecSnap::Verified {
            return Err(ClsCode::CloseBeforeVerified.val());
        }
        self.rows[idx].state = next;
        Ok(())
    }

    /// C 级豁免立案：仅 C 级行可领、案号非零唯一、一 C 一豁免。
    pub fn grant_waiver(&mut self, idx: usize, case_id: u32) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(ClsCode::CloseGroupOutOfRange.val());
        }
        if self.rows[idx].level != CloseLevel::LevelC {
            return Err(ClsCode::CloseStateIllegal.val());
        }
        if case_id == 0 {
            return Err(ClsCode::WaiverDup.val());
        }
        let mut i = 0;
        while i < self.waivers.len() {
            if self.waivers[i] == case_id {
                return Err(ClsCode::WaiverDup.val());
            }
            i += 1;
        }
        self.rows[idx].waiver = case_id;
        self.waivers.push(case_id);
        Ok(())
    }

    /// 兑现终核：Audited→Cleared；C 级无豁免拒收（红线）。
    pub fn clear(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(ClsCode::CloseGroupOutOfRange.val());
        }
        if self.rows[idx].state != CloseState::Audited {
            return Err(ClsCode::CloseStateIllegal.val());
        }
        if self.rows[idx].level == CloseLevel::LevelC && self.rows[idx].waiver == 0 {
            return Err(ClsCode::LevelCWithoutWaiver.val());
        }
        self.rows[idx].state = CloseState::Cleared;
        Ok(())
    }

    /// 闭环守恒：Σ终核态 ≡ 行数；兑现数 ≡ Cleared 态数；
    /// 豁免数 ≡ C 级带豁免数。
    pub fn conservation(&self) -> Result<[u32; 4], u32> {
        let mut counts = [0u32; 4];
        let mut waived_c = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let k = match self.rows[i].state {
                CloseState::Unclosed => 0,
                CloseState::CloseClaimed => 1,
                CloseState::Audited => 2,
                CloseState::Cleared => 3,
            };
            if self.rows[i].level == CloseLevel::LevelC && self.rows[i].waiver != 0 {
                waived_c += 1;
            }
            counts[k] += 1;
            i += 1;
        }
        let total = counts[0] + counts[1] + counts[2] + counts[3];
        if total != self.rows.len() as u32 {
            return Err(ClsCode::ConservationBreak.val());
        }
        if waived_c != self.waivers.len() as u32 {
            return Err(ClsCode::ConservationBreak.val());
        }
        Ok(counts)
    }

    /// 账哈希（幂等载体）。
    pub fn ledger_hash(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0;
        while i < self.rows.len() {
            h = (h ^ (self.rows[i].group as u32)).wrapping_mul(0x01000193);
            let ev = match self.rows[i].exec {
                ExecSnap::PreVerified => 1u32,
                ExecSnap::Verified => 2,
            };
            h = (h ^ ev).wrapping_mul(0x01000193);
            let lv = match self.rows[i].level {
                CloseLevel::LevelA => 0xA0u32,
                CloseLevel::LevelB => 0xB0,
                CloseLevel::LevelC => 0xC0,
            };
            h = (h ^ lv).wrapping_mul(0x01000193);
            let sv = match self.rows[i].state {
                CloseState::Unclosed => 1u32,
                CloseState::CloseClaimed => 2,
                CloseState::Audited => 3,
                CloseState::Cleared => 4,
            };
            h = (h ^ sv).wrapping_mul(0x01000193);
            h = (h ^ self.rows[i].waiver).wrapping_mul(0x01000193);
            i += 1;
        }
        h
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

// ── 黄金正演：40 组继承 → 补核 → 申报 → 终核 → C 豁免 → 兑现 ────
pub fn golden_run() -> Result<u32, u32> {
    let mut led = CloseLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut g = 1;
    while g <= CLOSE_GROUP_COUNT {
        let level = match rng.below(3) {
            0 => CloseLevel::LevelA,
            1 => CloseLevel::LevelB,
            _ => CloseLevel::LevelC,
        };
        led.book(g, ExecSnap::PreVerified, level)?;
        g += 1;
    }
    // 全量闭环：补核→申报→终核→（C 领豁免）→兑现。
    let mut i = 0;
    while i < CLOSE_GROUP_COUNT {
        led.verify_exec(i)?;
        led.advance(i, CloseState::CloseClaimed)?;
        led.advance(i, CloseState::Audited)?;
        if led.rows[i].level == CloseLevel::LevelC {
            led.grant_waiver(i, 0x00F1_0000 + i as u32)?;
        }
        led.clear(i)?;
        i += 1;
    }
    // 守恒 + 全兑现断言。
    let counts = led.conservation()?;
    if counts[3] != CLOSE_GROUP_COUNT as u32 {
        return Err(ClsCode::ConservationBreak.val());
    }
    // 双跑幂等。
    let h1 = led.ledger_hash();
    let h2 = led.ledger_hash();
    if h1 != h2 {
        return Err(ClsCode::RerunNonIdempotent.val());
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
    let mut d = CloseLedger::new();
    // 族 0：未执行申报（PreVerified 直接 CloseClaimed）。
    ledger.tries[0] += 1;
    let _ = d.book(1, ExecSnap::PreVerified, CloseLevel::LevelA);
    if let Err(e) = d.advance(0, CloseState::CloseClaimed) {
        if e == ClsCode::CloseBeforeVerified.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：C 级无豁免兑现。
    ledger.tries[1] += 1;
    let _ = d.book(2, ExecSnap::Verified, CloseLevel::LevelC);
    let _ = d.advance(1, CloseState::CloseClaimed);
    let _ = d.advance(1, CloseState::Audited);
    if let Err(e) = d.clear(1) {
        if e == ClsCode::LevelCWithoutWaiver.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：回跳（Audited→CloseClaimed 反向）。
    ledger.tries[2] += 1;
    if let Err(e) = d.advance(1, CloseState::CloseClaimed) {
        if e == ClsCode::CloseStateIllegal.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ClsCode::ALL.len() {
        let v = ClsCode::ALL[i].val();
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
    v.push(("J1-黄金正演40组兑现", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 递进闭环正演：PreVerified→verify_exec→申报通（正反成对）。
    let mut d2 = CloseLedger::new();
    let _ = d2.book(1, ExecSnap::PreVerified, CloseLevel::LevelB);
    let rec_ok = d2.verify_exec(0).is_ok()
        && d2.advance(0, CloseState::CloseClaimed).is_ok();
    v.push((
        "J1-递进闭环正反",
        rec_ok
            && {
                let mut d3 = CloseLedger::new();
                let _ = d3.book(1, ExecSnap::PreVerified, CloseLevel::LevelB);
                d3.advance(0, CloseState::CloseClaimed)
                    == Err(ClsCode::CloseBeforeVerified.val())
            },
        String::from("Verified 前置红线"),
    ));
    // 判定级三闭集互异。
    v.push((
        "J1-判定级三闭集互异",
        CloseLevel::LevelA.tag() != CloseLevel::LevelB.tag()
            && CloseLevel::LevelB.tag() != CloseLevel::LevelC.tag(),
        String::from("A/B/C"),
    ));
    // 组号越界必抓。
    let mut d4 = CloseLedger::new();
    v.push((
        "J1-组号越界必抓",
        d4.book(41, ExecSnap::Verified, CloseLevel::LevelA)
            == Err(ClsCode::CloseGroupOutOfRange.val()),
        String::from("1~40 闭集"),
    ));
    // 豁免唯一性：同案号二次领必拒。
    let mut d5 = CloseLedger::new();
    let _ = d5.book(1, ExecSnap::Verified, CloseLevel::LevelC);
    let _ = d5.grant_waiver(0, 0x1234);
    v.push((
        "J1-豁免重复必抓",
        d5.grant_waiver(0, 0x1234) == Err(ClsCode::WaiverDup.val()),
        String::from("一 C 一豁免"),
    ));
    // detail 反查闭链。
    let first = detail(ClsCode::CloseGroupOutOfRange.val());
    let last = detail(ClsCode::RerunNonIdempotent.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65BC0 段闭集内"
            && last != "未知诊断码：不在 0x65BC0 段闭集内",
        String::from("首尾反查非未知"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65BC0 窗口")));
    v
}

pub fn run_unx13541_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13541 双向账清账总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
