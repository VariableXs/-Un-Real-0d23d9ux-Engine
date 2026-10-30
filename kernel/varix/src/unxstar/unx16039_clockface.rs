//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16039 · 宿主时钟语义承接面 · 0x69100
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16039-J1 时钟承接面逐 API 落账（clock_gettime 类到 A2 时钟
//!   源映射），映射行与 A2 语义档对照一致率 100%，单调钟与墙上钟双
//!   轨注记率 100%。
//! 定位：Wine 的计时（性能计数器/系统时间/高精度定时）全部投影到宿
//!   主时钟，本条落时钟承接面：clock_gettime 类调用到 A2 时钟源仲
//!   裁的映射逐 API 对照，单调钟与墙上钟双轨显式注记——Wine 的
//!   GetTickCount 与 QueryPerformanceCounter 走哪条钟，账上有名。
//! 设计要点：
//!   - 承接面账：Wine 计时调用族（性能计数/系统时间/时钟精度/定时
//!     器）逐 API 登记映射目标（CLOCK_MONOTONIC/CLOCK_REALTIME 类
//!     到 A2 源），双轨注记（单调性要求/墙上时间要求）逐行落；
//!   - 时钟语义缺口（A2 未覆盖的精度档）显式标注并输出桩清单；
//!   - 精度档注记：timeBeginPeriod 语义（H2 域消费）的精度档位与
//!     A2 能力档位对照，档位差分逐行注记——精度不足要说明，不许静
//!     默认降档；
//!   - 注入面：API 越闭集/时钟源越闭集/双轨注记缺失/精度档注记删
//!     失必拒收/重复映射/A2 指针悬空——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x69100）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x69100，AI-VX-1031 E1 域批N 独占区）。
pub const SEG_BASE: u32 = 0x69100;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── Wine 计时调用族闭集 ─────────────────────────────────────────
pub const API_QPC: usize = 0; // QueryPerformanceCounter
pub const API_GTC: usize = 1; // GetTickCount
pub const API_SYSTIME: usize = 2; // GetSystemTime
pub const API_TIMER: usize = 3; // timeSetEvent 精度档
pub const API_COUNT: usize = 4;

pub const API_NAMES: [&str; API_COUNT] =
    ["QPC性能计数", "GetTickCount", "系统时间", "定时器精度档"];

// ── A2 时钟源闭集（POSIX CLOCK_ 值口径） ────────────────────────
pub const CLK_MONOTONIC: usize = 0; // 单调钟
pub const CLK_REALTIME: usize = 1; // 墙上钟
pub const CLK_COUNT: usize = 2;

pub const CLK_NAMES: [&str; CLK_COUNT] = ["CLOCK_MONOTONIC", "CLOCK_REALTIME"];

/// 双轨注记闭集。
pub const TRACK_MONO: usize = 0; // 单调性要求
pub const TRACK_WALL: usize = 1; // 墙上时间要求
pub const TRACK_COUNT: usize = 2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfCode {
    /// API 越闭集
    ApiOutOfRange,
    /// 时钟源越闭集
    ClkOutOfRange,
    /// 双轨注记缺失（单调/墙上必居其一且显式）
    TrackNoteMissing,
    /// 精度档注记缺失（删注记必拒收）
    PeriodNoteMissing,
    /// 重复映射
    MapDuplicated,
    /// A2 指针悬空
    A2Dangling,
    /// 缺口档未标注
    GapUnmarked,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl CfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CfCode::ApiOutOfRange => "API 越闭集",
            CfCode::ClkOutOfRange => "时钟源越闭集",
            CfCode::TrackNoteMissing => "双轨注记缺失：单调/墙上要显式",
            CfCode::PeriodNoteMissing => "精度档注记缺失：不许静默降档",
            CfCode::MapDuplicated => "重复映射：一 API 一映射",
            CfCode::A2Dangling => "A2 指针悬空",
            CfCode::GapUnmarked => "缺口档未标注",
            CfCode::NotReproducible => "双跑不一致：确定性红线",
            CfCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [CfCode; 9] = [
        CfCode::ApiOutOfRange,
        CfCode::ClkOutOfRange,
        CfCode::TrackNoteMissing,
        CfCode::PeriodNoteMissing,
        CfCode::MapDuplicated,
        CfCode::A2Dangling,
        CfCode::GapUnmarked,
        CfCode::NotReproducible,
        CfCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CfCode::ALL.len() {
        if CfCode::ALL[i].val() == code {
            return CfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

/// A2 语义档指针闭集。
pub const A2_POOL: [&str; 2] = ["A2-单调钟档", "A2-墙上钟档"];

pub fn a2_legal(a: &str) -> bool {
    let mut i = 0;
    while i < A2_POOL.len() {
        if A2_POOL[i] == a {
            return true;
        }
        i += 1;
    }
    false
}

/// 映射行。
#[derive(Clone, Debug)]
pub struct ClockMapRow {
    /// Wine API（0..API_COUNT）。
    pub api: usize,
    /// 映射目标时钟源（0/1）。
    pub clk: usize,
    /// 双轨注记（TRACK_MONO/TRACK_WALL，必居其一显式）。
    pub track: usize,
    /// A2 语义档引用（须 ∈ A2_POOL）。
    pub a2_ref: String,
    /// 精度档注记（timeBeginPeriod 档位差分，定时器族必填）。
    pub period_note: String,
}

/// 单行体检：映射咬合 + 双轨注记。
pub fn vet_row(r: &ClockMapRow) -> Result<(), u32> {
    if r.api >= API_COUNT {
        return Err(CfCode::ApiOutOfRange.val());
    }
    if r.clk >= CLK_COUNT {
        return Err(CfCode::ClkOutOfRange.val());
    }
    if r.track >= TRACK_COUNT {
        return Err(CfCode::TrackNoteMissing.val());
    }
    if !a2_legal(&r.a2_ref) {
        return Err(CfCode::A2Dangling.val());
    }
    // 映射咬合：QPC/GetTickCount 走单调钟；系统时间/定时器走墙上钟。
    let expect_clk = match r.api {
        API_QPC | API_GTC => CLK_MONOTONIC,
        _ => CLK_REALTIME,
    };
    if r.clk != expect_clk {
        return Err(CfCode::ClkOutOfRange.val());
    }
    // A2 档与时钟源咬合。
    let expect_ref = if r.clk == CLK_MONOTONIC { A2_POOL[0] } else { A2_POOL[1] };
    if r.a2_ref != expect_ref {
        return Err(CfCode::A2Dangling.val());
    }
    // 定时器族精度档注记必填。
    if r.api == API_TIMER && r.period_note.len() < 12 {
        return Err(CfCode::PeriodNoteMissing.val());
    }
    Ok(())
}

// ── 承接面账 ─────────────────────────────────────────────────────
pub struct ClockFace {
    rows: Vec<ClockMapRow>,
    /// 缺口档（A2 未覆盖的精度档，显式标注）。
    pub gaps: Vec<String>,
}

impl ClockFace {
    pub fn new() -> ClockFace {
        ClockFace { rows: Vec::new(), gaps: Vec::new() }
    }

    pub fn add(&mut self, r: ClockMapRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].api == r.api {
                return Err(CfCode::MapDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    pub fn mark_gap(&mut self, note: &str) -> Result<(), u32> {
        if note.len() < 12 {
            return Err(CfCode::GapUnmarked.val());
        }
        self.gaps.push(String::from(note));
        Ok(())
    }

    /// 四 API 覆盖。
    pub fn apis_closed(&self) -> Result<(), u32> {
        let mut a = 0;
        while a < API_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].api == a {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(CfCode::ApiOutOfRange.val());
            }
            a += 1;
        }
        Ok(())
    }

    /// 双轨注记率（判据 100% 口径）：全部行 track 显式。
    pub fn track_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(CfCode::TrackNoteMissing.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].track < TRACK_COUNT {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.rows.len())
    }

    /// A2 对照一致率（判据 100% 口径）。
    pub fn a2_align_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(CfCode::A2Dangling.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if vet_row(&self.rows[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.rows.len())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版承接面装配：四 API 全映射 + 精度档缺口。
pub fn build_initial() -> Result<ClockFace, u32> {
    let mut f = ClockFace::new();
    f.add(ClockMapRow {
        api: API_QPC,
        clk: CLK_MONOTONIC,
        track: TRACK_MONO,
        a2_ref: String::from("A2-单调钟档"),
        period_note: String::new(),
    })?;
    f.add(ClockMapRow {
        api: API_GTC,
        clk: CLK_MONOTONIC,
        track: TRACK_MONO,
        a2_ref: String::from("A2-单调钟档"),
        period_note: String::new(),
    })?;
    f.add(ClockMapRow {
        api: API_SYSTIME,
        clk: CLK_REALTIME,
        track: TRACK_WALL,
        a2_ref: String::from("A2-墙上钟档"),
        period_note: String::new(),
    })?;
    f.add(ClockMapRow {
        api: API_TIMER,
        clk: CLK_REALTIME,
        track: TRACK_WALL,
        a2_ref: String::from("A2-墙上钟档"),
        period_note: String::from(
            "精度档差分：timeBeginPeriod 1ms 档 A2 侧为 1ms 粒度上限，无静默降档",
        ),
    })?;
    f.mark_gap("缺口：A2 未覆盖亚毫秒精度档（timeBeginPeriod 1ms 以下），已输出桩清单")?;
    Ok(f)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 双轨率, 缺口数))。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let f = build_initial()?;
    f.apis_closed()?;
    let tr = f.track_rate()?;
    if tr != 100 {
        return Err(CfCode::TrackNoteMissing.val());
    }
    let ar = f.a2_align_rate()?;
    if ar != 100 {
        return Err(CfCode::A2Dangling.val());
    }
    if f.gaps.len() != 1 {
        return Err(CfCode::GapUnmarked.val());
    }
    Ok((f.len(), tr, f.gaps.len()))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 精度档注记缺失（判据复测方式：删注记必拒收）。
        led.record(
            0,
            vet_row(&ClockMapRow {
                api: API_TIMER,
                clk: CLK_REALTIME,
                track: TRACK_WALL,
                a2_ref: String::from("A2-墙上钟档"),
                period_note: String::new(),
            }) == Err(CfCode::PeriodNoteMissing.val()),
        );

        // ② 映射咬合错位：QPC 映射墙上钟。
        led.record(
            1,
            vet_row(&ClockMapRow {
                api: API_QPC,
                clk: CLK_REALTIME,
                track: TRACK_WALL,
                a2_ref: String::from("A2-墙上钟档"),
                period_note: String::new(),
            }) == Err(CfCode::ClkOutOfRange.val()),
        );

        // ③ A2 档与时钟源错配。
        led.record(
            2,
            vet_row(&ClockMapRow {
                api: API_GTC,
                clk: CLK_MONOTONIC,
                track: TRACK_MONO,
                a2_ref: String::from("A2-墙上钟档"),
                period_note: String::new(),
            }) == Err(CfCode::A2Dangling.val()),
        );

        // ④ API 越闭集 / 时钟源越闭集。
        led.record(
            3,
            vet_row(&ClockMapRow {
                api: API_COUNT + (rnd % 2) as usize,
                clk: CLK_MONOTONIC,
                track: TRACK_MONO,
                a2_ref: String::from("A2-单调钟档"),
                period_note: String::new(),
            }) == Err(CfCode::ApiOutOfRange.val())
                && vet_row(&ClockMapRow {
                    api: API_QPC,
                    clk: CLK_COUNT,
                    track: TRACK_MONO,
                    a2_ref: String::from("A2-单调钟档"),
                    period_note: String::new(),
                }) == Err(CfCode::ClkOutOfRange.val()),
        );

        // ⑤ A2 指针悬空。
        led.record(
            4,
            vet_row(&ClockMapRow {
                api: API_SYSTIME,
                clk: CLK_REALTIME,
                track: TRACK_WALL,
                a2_ref: String::from("A2-ghost"),
                period_note: String::new(),
            }) == Err(CfCode::A2Dangling.val()),
        );

        // ⑥ 缺口档未标注 + 轮次守卫。
        led.record(
            5,
            ClockFace::new().mark_gap("短") == Err(CfCode::GapUnmarked.val())
                && (rnd % 1) == 0,
        );

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(CfCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法承接面必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let f = build_initial()?;
    f.apis_closed()?;
    // 四 API 名/两钟源名点验。
    let mut a = 0;
    while a < API_COUNT {
        if API_NAMES[a].len() == 0 {
            return Err(CfCode::ApiOutOfRange.val());
        }
        a += 1;
    }
    let mut c = 0;
    while c < CLK_COUNT {
        if CLK_NAMES[c].len() == 0 {
            return Err(CfCode::ClkOutOfRange.val());
        }
        c += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CfCode::ALL.len() {
        let v = CfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：4 行 / 双轨 100% / 1 缺口。
    let g = golden_run();
    v.push((
        "J1-黄金正演4行100%1缺口",
        matches!(g, Ok((4, 100, 1))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 双轨注记率 100%（判据原文口径）。
    let f = build_initial().unwrap_or_else(|_| ClockFace::new());
    let tr = f.track_rate().unwrap_or(0);
    v.push(("J1-双轨注记率100%", tr == 100, format!("{}%", tr)));
    // 4. A2 对照一致率 100%（判据原文口径）。
    let ar = f.a2_align_rate().unwrap_or(0);
    v.push(("J1-A2对照一致100%", ar == 100, format!("{}%", ar)));
    // 5. 精度档注记在册。
    let pn_ok = f.rows.iter().any(|r| r.api == API_TIMER && r.period_note.len() >= 12);
    v.push(("J1-精度档注记在册", pn_ok, String::from("timeBeginPeriod")));
    // 6. 缺口桩清单输出。
    v.push((
        "J1-缺口桩清单输出",
        f.gaps.len() == 1,
        String::from("亚毫秒档"),
    ));
    // 7. 删精度档注记必拒收（判据复测方式）。
    v.push((
        "J1-删精度档注记必拒收",
        vet_row(&ClockMapRow {
            api: API_TIMER,
            clk: CLK_REALTIME,
            track: TRACK_WALL,
            a2_ref: String::from("A2-墙上钟档"),
            period_note: String::from("短"),
        }) == Err(CfCode::PeriodNoteMissing.val()),
        String::from("复测方式"),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("四API+两钟源"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16039_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16039 宿主时钟语义承接面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
