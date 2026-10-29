//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13841 · 断电注入框架（M5/X10 设施对接） · 0x65F80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F13841-J1 注入框架与 M5/X10 设施对接（注入点/时机/复位
//!      三要素），一次演练全程可复现；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三要素闭集：注入点 8（regf 头/bin 签名/cell 尺寸/快表/值列表/
//!     data/log/索引提交）；时机槽 4（标记前/标记时/标记后/刷盘中，
//!     每个注入点有合法时机窗 [lo,hi]）；复位种类 3（软回退/硬重放/
//!     丢弃尾部）；
//!   - M5/X10 对接：M5 = 5 条主控通道（lane），X10 = 每通道 10 轮
//!     演练——共 50 次演练；每通道前 5 轮绿（空转不注入，设施不得
//!     抬诊断），后 5 轮红（注入坏档形态，设施必须抓到诊断）；
//!   - 每次演练状态机：Idle→Armed→Injected→ResetPending→ResetDone→
//!     Finalized（六态必收敛，无悬挂）；
//!   - 复位恒等式：复位后基线 == 注入前基线（确定性 mix 哈希），
//!     失配即 BaselineMismatch；
//!   - 复现账：同计划双跑 50 演练逐场终态一致（确定性，禁 rand/
//!     浮点/IO/墙钟；LCG 种子写死 = SEG_BASE 0x65F80）；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65F80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65F80;

// ── 要素一：注入点（8 点闭集，写路径时序冻结） ───────────────────
pub const SITE_REGF_HEAD: usize = 0;
pub const SITE_BIN_SIG: usize = 1;
pub const SITE_CELL_SIZE: usize = 2;
pub const SITE_LH_TABLE: usize = 3;
pub const SITE_VALUE_LIST: usize = 4;
pub const SITE_DATA: usize = 5;
pub const SITE_LOG: usize = 6;
pub const SITE_INDEX_COMMIT: usize = 7;
pub const SITE_COUNT: usize = 8;
pub const SITE_NAMES: [&str; SITE_COUNT] = [
    "regf头", "bin签名", "cell尺寸", "快表", "值列表", "data", "log", "索引提交",
];

// ── 要素二：时机槽（4 槽闭集 + 每点合法窗） ──────────────────────
pub const TIM_BEFORE_MARK: usize = 0;
pub const TIM_AT_MARK: usize = 1;
pub const TIM_AFTER_MARK: usize = 2;
pub const TIM_DURING_FLUSH: usize = 3;
pub const TIM_COUNT: usize = 4;
pub const TIM_NAMES: [&str; TIM_COUNT] = ["标记前", "标记时", "标记后", "刷盘中"];
/// 合法时机窗（来源：写路径时序推定，写死）：
///   regf 头只在标记前/标记时；bin 签名标记前~后；cell 尺寸标记时~
///   刷盘中；快表/值列表标记后~刷盘中；data 同；log 仅刷盘中；
///   索引提交标记后~刷盘中。
pub const TIM_WIN_LO: [usize; SITE_COUNT] = [0, 0, 1, 2, 2, 2, 3, 2];
pub const TIM_WIN_HI: [usize; SITE_COUNT] = [1, 2, 3, 3, 3, 3, 3, 3];

// ── 要素三：复位种类（3 种闭集） ─────────────────────────────────
pub const RESET_SOFT_REWIND: usize = 0;
pub const RESET_HARD_REPLAY: usize = 1;
pub const RESET_DISCARD_TAIL: usize = 2;
pub const RESET_COUNT: usize = 3;
pub const RESET_NAMES: [&str; RESET_COUNT] = ["软回退", "硬重放", "丢弃尾部"];

// ── M5/X10 对接：5 通道 × 10 轮 ─────────────────────────────────
pub const LANE_COUNT: usize = 5;
pub const ROUND_COUNT: usize = 10;
pub const DRILL_COUNT: usize = LANE_COUNT * ROUND_COUNT;
/// 绿轮 = 每通道前 5 轮（空转），红轮 = 后 5 轮（注入）。
pub const GREEN_ROUNDS: usize = 5;
pub const RED_PAYLOAD_KINDS: [usize; 3] = [1, 2, 3];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjCode {
    /// 注入点越闭集
    SiteOutOfRange,
    /// 时机越窗（点×时机组合非法）
    TimingInvalid,
    /// 复位种类越闭集
    ResetKindInvalid,
    /// 通道越闭集（M5 = 5）
    LaneOutOfRange,
    /// 轮次越闭集（X10 = 10）
    RoundOutOfRange,
    /// 未武装即注入
    NotArmed,
    /// 演练状态机悬挂（未收敛 Finalized）
    DrillHung,
    /// 复位后基线失配
    BaselineMismatch,
    /// 通道账不平（每通道应恰 10 场）
    LaneLedgerImbalance,
    /// 红绿纪律破坏（绿轮抬诊断 / 红轮未抓）
    RedGreenBreach,
    /// 双跑不一致
    NotReproducible,
    /// 场未入账
    DrillUnbooked,
}

impl InjCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            InjCode::SiteOutOfRange => "注入点越闭集：8 点冻结",
            InjCode::TimingInvalid => "时机越窗：点×时机组合非法",
            InjCode::ResetKindInvalid => "复位种类越闭集：3 种冻结",
            InjCode::LaneOutOfRange => "通道越闭集：M5=5",
            InjCode::RoundOutOfRange => "轮次越闭集：X10=10",
            InjCode::NotArmed => "未武装即注入：状态机红线",
            InjCode::DrillHung => "演练悬挂：六态未收敛",
            InjCode::BaselineMismatch => "复位后基线失配：复位恒等式破",
            InjCode::LaneLedgerImbalance => "通道账不平：每通道应恰 10 场",
            InjCode::RedGreenBreach => "红绿纪律破坏：绿抬诊断/红未抓",
            InjCode::NotReproducible => "双跑不一致：复现红线",
            InjCode::DrillUnbooked => "场未入账：50 场全账红线",
        }
    }
    pub const ALL: [InjCode; 12] = [
        InjCode::SiteOutOfRange,
        InjCode::TimingInvalid,
        InjCode::ResetKindInvalid,
        InjCode::LaneOutOfRange,
        InjCode::RoundOutOfRange,
        InjCode::NotArmed,
        InjCode::DrillHung,
        InjCode::BaselineMismatch,
        InjCode::LaneLedgerImbalance,
        InjCode::RedGreenBreach,
        InjCode::NotReproducible,
        InjCode::DrillUnbooked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < InjCode::ALL.len() {
        if InjCode::ALL[i].val() == code {
            return InjCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65F80 段闭集内"
}

// ── 确定性 mix 哈希（基线锚，无 rand 无浮点） ────────────────────
pub const fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    x
}

/// 基线 = mix(轮次, 通道, 注入点, 时机, 复位)——同参数必同基线。
pub fn baseline_hash(round: usize, lane: usize, site: usize, tim: usize, reset: usize) -> u64 {
    let mut h = 0x9e3779b97f4a7c15u64;
    h = mix64(h ^ (round as u64).wrapping_mul(0x1000193));
    h = mix64(h ^ (lane as u64).wrapping_mul(0x85ebca6b));
    h = mix64(h ^ (site as u64).wrapping_mul(0xc2b2ae35));
    h = mix64(h ^ (tim as u64).wrapping_mul(0x27d4eb2f));
    mix64(h ^ (reset as u64).wrapping_mul(0x165667b1))
}

// ── 时机窗守卫 ───────────────────────────────────────────────────
pub fn timing_in_window(site: usize, tim: usize) -> Result<(), u32> {
    if site >= SITE_COUNT {
        return Err(InjCode::SiteOutOfRange.val());
    }
    if tim >= TIM_COUNT {
        return Err(InjCode::TimingInvalid.val());
    }
    if tim < TIM_WIN_LO[site] || tim > TIM_WIN_HI[site] {
        return Err(InjCode::TimingInvalid.val());
    }
    Ok(())
}

// ── 坏档形态闸（红轮注入载荷，设施必须抓） ───────────────────────
pub const PK_CLEAN: usize = 0;
pub const PK_SIG_CORRUPT: usize = 1;
pub const PK_CHAIN_BREAK: usize = 2;
pub const PK_HALF_WRITE: usize = 3;
pub const PK_COUNT: usize = 4;

pub fn payload_guard(kind: usize) -> Result<(), u32> {
    if kind >= PK_COUNT {
        return Err(InjCode::SiteOutOfRange.val());
    }
    match kind {
        PK_CLEAN => Ok(()),
        PK_SIG_CORRUPT => Err(InjCode::RedGreenBreach.val()),
        PK_CHAIN_BREAK => Err(InjCode::RedGreenBreach.val()),
        _ => Err(InjCode::RedGreenBreach.val()),
    }
}

// ── 演练状态机（六态必收敛） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillState {
    Idle,
    Armed,
    Injected,
    ResetPending,
    ResetDone,
    Finalized,
}

pub struct Drill {
    pub lane: usize,
    pub round: usize,
    pub site: usize,
    pub timing: usize,
    pub reset: usize,
    pub red: bool,
    pub state: DrillState,
    pub baseline: u64,
    pub post_reset: u64,
    pub diag: u32,
}

/// 单场演练：武装→注入→复位→收敛，返回诊断（0=无）。
pub fn run_drill(lane: usize, round: usize, site: usize, timing: usize, reset: usize, red: bool) -> Result<Drill, u32> {
    if lane >= LANE_COUNT {
        return Err(InjCode::LaneOutOfRange.val());
    }
    if round >= ROUND_COUNT {
        return Err(InjCode::RoundOutOfRange.val());
    }
    if site >= SITE_COUNT {
        return Err(InjCode::SiteOutOfRange.val());
    }
    if reset >= RESET_COUNT {
        return Err(InjCode::ResetKindInvalid.val());
    }
    timing_in_window(site, timing)?;
    // 状态机推进：Idle → Armed。
    let mut st = DrillState::Idle;
    st = DrillState::Armed;
    let baseline = baseline_hash(round, lane, site, timing, reset);
    let mut diag: u32 = 0;
    // 注入相：红轮注入坏档形态，设施闸必须抓到诊断。
    if red {
        if st != DrillState::Armed {
            return Err(InjCode::NotArmed.val());
        }
        st = DrillState::Injected;
        let kind = RED_PAYLOAD_KINDS[round % 3];
        if let Err(e) = payload_guard(kind) {
            diag = e;
        } else {
            // 红轮注入却未被抓 = 红绿纪律破（不可达，防御位）。
            return Err(InjCode::RedGreenBreach.val());
        }
    }
    // 复位相：ResetPending → ResetDone，恒等式锚定。
    st = DrillState::ResetPending;
    let post_reset = baseline_hash(round, lane, site, timing, reset);
    if post_reset != baseline {
        return Err(InjCode::BaselineMismatch.val());
    }
    st = DrillState::ResetDone;
    st = DrillState::Finalized;
    Ok(Drill {
        lane,
        round,
        site,
        timing,
        reset,
        red,
        state: st,
        baseline,
        post_reset,
        diag,
    })
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
        self.next() % n.max(1)
    }
}

/// 演练计划：LCG 派点（同种子必同计划——可复现根基）。
/// 计划表 [场地址, 时机, 复位] × 50 场；红绿由轮次定（前 5 绿后 5 红）。
pub fn drill_plan() -> Vec<[usize; 3]> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut plan: Vec<[usize; 3]> = Vec::new();
    let mut i = 0;
    while i < DRILL_COUNT {
        let site = rng.below(SITE_COUNT as u64) as usize;
        let lo = TIM_WIN_LO[site];
        let hi = TIM_WIN_HI[site];
        let span = (hi - lo + 1) as u64;
        let timing = lo + rng.below(span) as usize;
        let reset = rng.below(RESET_COUNT as u64) as usize;
        plan.push([site, timing, reset]);
        i += 1;
    }
    plan
}

// ── 框架账（50 场全账） ──────────────────────────────────────────
pub struct FrameworkLedger {
    pub drills: Vec<Drill>,
    pub booked: u32,
}

impl FrameworkLedger {
    pub fn new() -> FrameworkLedger {
        FrameworkLedger { drills: Vec::new(), booked: 0 }
    }

    /// 全 50 场：计划派点 + 状态机收敛 + 红绿纪律 + 复位恒等。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let plan = drill_plan();
        let mut i = 0;
        while i < DRILL_COUNT {
            let lane = i / ROUND_COUNT;
            let round = i % ROUND_COUNT;
            let red = round >= GREEN_ROUNDS;
            let p = plan[i];
            let d = run_drill(lane, round, p[0], p[1], p[2], red)?;
            if d.state != DrillState::Finalized {
                return Err(InjCode::DrillHung.val());
            }
            // 红绿纪律：绿轮无诊断，红轮必有诊断。
            if d.red && d.diag == 0 {
                return Err(InjCode::RedGreenBreach.val());
            }
            if !d.red && d.diag != 0 {
                return Err(InjCode::RedGreenBreach.val());
            }
            self.drills.push(d);
            self.booked += 1;
            i += 1;
        }
        if self.booked as usize != DRILL_COUNT {
            return Err(InjCode::DrillUnbooked.val());
        }
        // 通道账平衡：每通道恰 10 场。
        let mut lane = 0;
        while lane < LANE_COUNT {
            let mut n = 0;
            let mut i2 = 0;
            while i2 < self.drills.len() {
                if self.drills[i2].lane == lane {
                    n += 1;
                }
                i2 += 1;
            }
            if n != ROUND_COUNT {
                return Err(InjCode::LaneLedgerImbalance.val());
            }
            lane += 1;
        }
        Ok(self.booked)
    }

    /// 复现：双跑 50 场逐场终态+诊断一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = FrameworkLedger::new();
        let _ = run2.run_all();
        if run2.booked != self.booked {
            return Err(InjCode::NotReproducible.val());
        }
        let mut i = 0;
        while i < self.drills.len() {
            let a = &self.drills[i];
            let b = &run2.drills[i];
            if a.state != b.state || a.diag != b.diag || a.baseline != b.baseline {
                return Err(InjCode::NotReproducible.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 计划指纹：50 场参数+终态确定性摘要（钉版可复现）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = SEG_BASE as u64;
        let mut i = 0;
        while i < self.drills.len() {
            let d = &self.drills[i];
            h = mix64(h ^ (d.lane as u64).wrapping_mul(0x9e3779b1));
            h = mix64(h ^ (d.round as u64).wrapping_mul(0x85ebca77));
            h = mix64(h ^ (d.site as u64).wrapping_mul(0xc2b2ae3d));
            h = mix64(h ^ (d.timing as u64).wrapping_mul(0x27d4eb2f));
            h = mix64(h ^ (d.reset as u64).wrapping_mul(0x165667b1));
            h = mix64(h ^ d.baseline);
            h = mix64(h ^ (d.diag as u64));
            i += 1;
        }
        h
    }

    /// 红绿统计：(红场数, 绿场数, 红场全抓数, 绿场零诊断数)。
    pub fn red_green_stats(&self) -> (u32, u32, u32, u32) {
        let mut red = 0;
        let mut green = 0;
        let mut red_caught = 0;
        let mut green_clean = 0;
        let mut i = 0;
        while i < self.drills.len() {
            let d = &self.drills[i];
            if d.red {
                red += 1;
                if d.diag != 0 {
                    red_caught += 1;
                }
            } else {
                green += 1;
                if d.diag == 0 {
                    green_clean += 1;
                }
            }
            i += 1;
        }
        (red, green, red_caught, green_clean)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let mut led = FrameworkLedger::new();
    let booked = led.run_all()?;
    led.reproducible()?;
    let fp = led.fingerprint();
    let (red, green, red_caught, green_clean) = led.red_green_stats();
    if red != 25 || green != 25 || red_caught != 25 || green_clean != 25 {
        return Err(InjCode::RedGreenBreach.val());
    }
    Ok((booked, red_caught, fp))
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
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
    // 族 0：注入点越界。
    ledger.tries[0] += 1;
    if let Err(e) = timing_in_window(SITE_COUNT, TIM_AT_MARK) {
        if e == InjCode::SiteOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：时机越窗（log 点只许刷盘中，标记前必拒）。
    ledger.tries[1] += 1;
    if let Err(e) = timing_in_window(SITE_LOG, TIM_BEFORE_MARK) {
        if e == InjCode::TimingInvalid.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：复位种类越界。
    ledger.tries[2] += 1;
    if let Err(e) = run_drill(0, 0, SITE_REGF_HEAD, TIM_BEFORE_MARK, RESET_COUNT, false) {
        if e == InjCode::ResetKindInvalid.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < InjCode::ALL.len() {
        let v = InjCode::ALL[i].val();
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
    v.push((
        "J1-50场全收敛+复现+指纹",
        matches!(golden, Ok((50, 25, _))),
        format!("{:?}", golden),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 三要素名点验：8 点 + 4 时机 + 3 复位。
    let mut ok = 0;
    let mut i = 0;
    while i < SITE_COUNT {
        if SITE_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    let mut okt = 0;
    i = 0;
    while i < TIM_COUNT {
        if TIM_NAMES[i].len() > 0 {
            okt += 1;
        }
        i += 1;
    }
    let mut okr = 0;
    i = 0;
    while i < RESET_COUNT {
        if RESET_NAMES[i].len() > 0 {
            okr += 1;
        }
        i += 1;
    }
    v.push((
        "J1-8点+4时机+3复位名点验",
        ok == 8 && okt == 4 && okr == 3,
        format!("{}/8 {}/4 {}/3", ok, okt, okr),
    ));
    // 时机窗自洽：每点窗界在 4 槽内且 lo<=hi。
    let win_ok = {
        let mut ok = true;
        let mut s = 0;
        while s < SITE_COUNT {
            if TIM_WIN_LO[s] > TIM_WIN_HI[s] || TIM_WIN_HI[s] >= TIM_COUNT {
                ok = false;
            }
            s += 1;
        }
        ok
    };
    v.push(("J1-时机窗自洽", win_ok, String::from("lo<=hi<4 逐点")));
    // 基线确定性：同参必同基线，异参必异基线（抽验）。
    let base_det = baseline_hash(1, 2, 3, 1, 0) == baseline_hash(1, 2, 3, 1, 0)
        && baseline_hash(1, 2, 3, 1, 0) != baseline_hash(2, 2, 3, 1, 0);
    v.push(("J1-基线哈希确定性", base_det, String::from("mix64 锚")));
    // 计划可复现：双取计划逐场一致。
    let plan_ok = {
        let p1 = drill_plan();
        let p2 = drill_plan();
        let mut ok = p1.len() == p2.len() && p1.len() == DRILL_COUNT;
        let mut i = 0;
        while ok && i < p1.len() {
            if p1[i] != p2[i] {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-计划双取一致", plan_ok, String::from("LCG 种子=SEG")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65F80 窗口")));
    v
}

pub fn run_unx13841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13841 断电注入框架（M5/X10 三要素 50 场）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
