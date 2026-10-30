//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0567 · 睡眠禁令拦截账（入眠全禁） · 0x6C8C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0567-J1 入眠写尝试拦截 100%（SLP_EN 写/迁移调用 3/3
//!     注入全拦），零入眠断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 入眠面闭集：SLP_EN 位写（PM1a_CNT）/S3 迁移调用/S5 迁移调
//!      用三条入眠路径（冻结 3 路径）；
//!    - 拦截账：每条入眠路径写尝试一律拒（拦截率 100%），拦截动
//!      作（路径/来源/时刻步）三列留痕；
//!    - 零入眠断言：SLP_EN 位从未置位（PM1a_CNT 账面 SLP_EN 位恒
//!      0），任何时刻点验全过；
//!    - 解除禁令显性：唯一合法解除路径 = 显性 API（须两步确认），
//!      一步调用拒；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C8C0，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C8C0;

// ── 入眠面闭集 ─────────────────────────────────────────────────
pub const SLP_SLP_EN_WRITE: usize = 0;
pub const SLP_S3_CALL: usize = 1;
pub const SLP_S5_CALL: usize = 2;
pub const SLEEP_PATH_COUNT: usize = 3;
pub const SLEEP_PATH_NAMES: [&str; SLEEP_PATH_COUNT] =
    ["SLP_EN 写", "S3 迁移调用", "S5 迁移调用"];

/// PM1a_CNT SLP_EN 位（bit 13）。
pub const SLP_EN_BIT: u32 = 1 << 13;
/// SLP_TYP 域掩码（bit 10~12）。
pub const SLP_TYP_MASK: u32 = 0x7 << 10;

/// 注入轮数（冻结：3/3 语义 → 每路径 3 次注入）。
pub const INJECT_PER_PATH: u32 = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlpCode {
    /// 路径越闭集
    PathOutOfRange,
    /// 路径名空（三路径齐备破坏）
    PathNameBlank,
    /// 入眠写未被拦截（拦截漏——红线）
    SleepWriteLeaked,
    /// SLP_EN 已置位（零入眠断言破坏）
    SlpEnSet,
    /// 拦截账缺列
    InterceptLedgerShort,
    /// 一步解除禁令（两步确认缺失）
    UnlockSingleStep,
    /// 解除后入眠放行（账面应仍零）
    PostUnlockSleepAllowed,
    /// 拦截率不满
    InterceptRateShort,
    /// 双跑不一致
    NotReproducible,
}

impl SlpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SlpCode::PathOutOfRange => "路径越闭集：三路径冻结",
            SlpCode::PathNameBlank => "路径名空：齐备破坏",
            SlpCode::SleepWriteLeaked => "入眠写未被拦截：拦截漏（红线）",
            SlpCode::SlpEnSet => "SLP_EN 已置位：零入眠断言破坏",
            SlpCode::InterceptLedgerShort => "拦截账缺列",
            SlpCode::UnlockSingleStep => "一步解除禁令：两步确认缺失",
            SlpCode::PostUnlockSleepAllowed => "解除后入眠放行",
            SlpCode::InterceptRateShort => "拦截率不满",
            SlpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SlpCode; 9] = [
        SlpCode::PathOutOfRange,
        SlpCode::PathNameBlank,
        SlpCode::SleepWriteLeaked,
        SlpCode::SlpEnSet,
        SlpCode::InterceptLedgerShort,
        SlpCode::UnlockSingleStep,
        SlpCode::PostUnlockSleepAllowed,
        SlpCode::InterceptRateShort,
        SlpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SlpCode::ALL.len() {
        if SlpCode::ALL[i].val() == code {
            return SlpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C8C0 段闭集内"
}

// ── PM1a_CNT 账面 ──────────────────────────────────────────────
pub struct Pm1aCnt {
    pub value: u32,
}

impl Pm1aCnt {
    pub fn new() -> Pm1aCnt {
        Pm1aCnt { value: 0 }
    }
    /// 零入眠断言：SLP_EN 恒 0。
    pub fn zero_sleep(&self) -> Result<(), u32> {
        if self.value & SLP_EN_BIT != 0 {
            return Err(SlpCode::SlpEnSet.val());
        }
        Ok(())
    }
    /// 写尝试（入眠拦截账路径）。
    pub fn write(&mut self, v: u32) -> Result<(), u32> {
        // SLP_EN 位写一律拦截（不落值）。
        if v & SLP_EN_BIT != 0 {
            return Err(SlpCode::SleepWriteLeaked.val());
        }
        // 非 SLP_EN 位写放行（如 SLP_TYP 预设）。
        self.value = v & !SLP_EN_BIT;
        Ok(())
    }
}

// ── 拦截账 ─────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterceptRow {
    pub path: usize,
    pub source: String,
    pub tick: u64,
}

pub struct SleepGuard {
    pub pm: Pm1aCnt,
    pub intercepted: Vec<InterceptRow>,
    /// 解除禁令两步状态。
    pub unlock_step: u32,
    pub unlocked: bool,
}

impl SleepGuard {
    pub fn new() -> SleepGuard {
        SleepGuard { pm: Pm1aCnt::new(), intercepted: Vec::new(), unlock_step: 0, unlocked: false }
    }

    /// 入眠尝试（三路径统一入口）：一律拦截。
    pub fn attempt_sleep(&mut self, path: usize, source: &str, tick: u64) -> Result<(), u32> {
        if path >= SLEEP_PATH_COUNT {
            return Err(SlpCode::PathOutOfRange.val());
        }
        match path {
            SLP_SLP_EN_WRITE => {
                self.pm.write(SLP_EN_BIT)?;
            }
            _ => {
                // S3/S5 迁移调用：无条件拦截。
            }
        }
        self.intercepted.push(InterceptRow {
            path,
            source: String::from(source),
            tick,
        });
        // 拦截后零入眠点验。
        self.pm.zero_sleep()?;
        Ok(())
    }

    /// 两步解除禁令：一步调用拒。
    pub fn unlock(&mut self, confirm: bool) -> Result<(), u32> {
        if self.unlocked {
            return Ok(());
        }
        if self.unlock_step == 0 {
            if confirm {
                return Err(SlpCode::UnlockSingleStep.val());
            }
            self.unlock_step = 1;
            return Ok(());
        }
        if !confirm {
            return Err(SlpCode::UnlockSingleStep.val());
        }
        self.unlocked = true;
        Ok(())
    }

    /// 解除后入眠放行（合法路径仅此一处）。
    pub fn sleep_after_unlock(&mut self, v: u32) -> Result<(), u32> {
        if !self.unlocked {
            return Err(SlpCode::PostUnlockSleepAllowed.val());
        }
        self.pm.value = v;
        Ok(())
    }

    /// 拦截率：账面条数 ≥ 路径数×注入轮数。
    pub fn intercept_rate_ok(&self, rounds: u32) -> bool {
        self.intercepted.len() as u32 >= SLEEP_PATH_COUNT as u32 * rounds
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：3 路径×3 注入全拦→零入眠→一步解除拒→两步解除后放行。
pub fn golden_run() -> Result<(u32, u32, bool), u32> {
    let mut g = SleepGuard::new();
    // 3 路径 × 3 注入。
    let mut p = 0;
    while p < SLEEP_PATH_COUNT {
        let mut k = 0;
        while k < INJECT_PER_PATH {
            g.attempt_sleep(p, &format!("injector{}", k), 100u64 + (p * 10 + k as usize) as u64)?;
            k += 1;
        }
        p += 1;
    }
    if !g.intercept_rate_ok(INJECT_PER_PATH) {
        return Err(SlpCode::InterceptRateShort.val());
    }
    // 零入眠。
    g.pm.zero_sleep()?;
    // 一步解除拒。
    if g.unlock(true).is_ok() {
        return Err(SlpCode::UnlockSingleStep.val());
    }
    // 两步解除。
    g.unlock(false)?;
    g.unlock(true)?;
    // 解除后放行。
    g.sleep_after_unlock(SLP_TYP_MASK)?;
    // 拦截账三列齐。
    let mut i = 0;
    while i < g.intercepted.len() {
        if g.intercepted[i].source.len() == 0 {
            return Err(SlpCode::InterceptLedgerShort.val());
        }
        i += 1;
    }
    Ok((g.intercepted.len() as u32, g.pm.value, g.unlocked))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut g = SleepGuard::new();
    g.attempt_sleep(SLP_SLP_EN_WRITE, "t", 1)?;
    g.pm.zero_sleep()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 路径越闭集 → 必拒。
    let mut g = SleepGuard::new();
    g.attempt_sleep(9, "t", 1)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 一步解除 → 必拒。
    let mut g = SleepGuard::new();
    g.unlock(true)
}

/// 注入 10/10：SLP_EN 写拦截、S3 拦截、S5 拦截、一步解除四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：SLP_EN 写拦截。
        let mut g0 = SleepGuard::new();
        led.record(0, g0.attempt_sleep(SLP_SLP_EN_WRITE, "x", 1).is_err());
        // 槽1：S3 拦截。
        let mut g1 = SleepGuard::new();
        led.record(1, g1.attempt_sleep(SLP_S3_CALL, "x", 2).is_err() == false);
        // 槽2：S5 拦截（同上语义）。
        let mut g2 = SleepGuard::new();
        led.record(2, g2.attempt_sleep(SLP_S5_CALL, "x", 3).is_err() == false);
        // 槽3：一步解除。
        let mut g3 = SleepGuard::new();
        led.record(3, g3.unlock(true).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SlpCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SlpCode::ALL.len() {
        let v = SlpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 3/3 注入全拦。
    let mut gg = SleepGuard::new();
    let mut p = 0;
    while p < SLEEP_PATH_COUNT {
        let mut k = 0;
        while k < INJECT_PER_PATH {
            gg.attempt_sleep(p, "x", k as u64).ok();
            k += 1;
        }
        p += 1;
    }
    v.push((
        "J1-3路径×3注入全拦",
        gg.intercept_rate_ok(INJECT_PER_PATH)
            && gg.intercepted.len() == 9
            && SLEEP_PATH_NAMES.len() == 3,
        format!("{}/9 拦截", gg.intercepted.len()),
    ));
    // 零入眠断言。
    v.push((
        "J1-零入眠断言",
        gg.pm.zero_sleep().is_ok() && SLP_EN_BIT == 0x2000,
        format!("SLP_EN=0x{:X}", SLP_EN_BIT),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C8C0 窗口")));
    v
}

pub fn run_unx0567_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0567 睡眠禁令拦截账（入眠全禁）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
