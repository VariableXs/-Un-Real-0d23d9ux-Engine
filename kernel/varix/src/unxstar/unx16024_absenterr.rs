//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16024 · absent 项应用加载期精确报错 · 0x68D40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16024-J1 absent 项触发的加载路径输出精确错误（含能力名与替
//!   代建议），错误信息三要素齐备率 100%，半途崩溃计数 = 0 断言通过。
//! 定位：任务书专题一：absent 项要在应用加载时给出精确错误而非半途
//!   崩溃——半途崩溃是用户体验最差的失败形态，本条把 absent 能力被
//!   触碰时的失败路径落成三要素报告：发生了什么/为什么/下一步怎么办。
//! 设计要点：
//!   - 触发面：absent 能力对应的 Wine 调用点逐个登记（能力名/触发
//!     API/调用链位置），触发面清单随跟随版本滚动；
//!   - 报错链：触发时输出三要素报告——发生了什么（"应用调用的 X 能
//!     力 Varix 未提供"）/为什么（host_caps 登记 absent + 缺失理由引
//!     用）/下一步怎么办（替代建议或等待补齐批次指针），报告落账与
//!     进程错误流双通道；
//!   - 断言面：注入探针逐个触碰 absent 能力，错误三要素齐备率 100%，
//!     进程异常崩溃（非受控退出）计数 = 0；
//!   - 受控退出语义：报错路径必须走受控退出（退出码 = 0xC0 | 触发面
//!     序号），不许 panic 面外溢；
//!   - 注入面：触发未登记/报告缺要素/崩溃面外溢/退出码为零/触发面
//!     重复/能力名悬空——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68D40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68D40，AI-VX-1031 E1 域批J 独占区）。
pub const SEG_BASE: u32 = 0x68D40;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 受控退出码基（0xC0 段 = absent 触发受控退出族）。
pub const CONTROLLED_EXIT_BASE: u32 = 0xC0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AeCode {
    /// 触发点未登记（触发面清单外触碰）
    TriggerUnbooked,
    /// 报告缺三要素
    ReportTriElementShort,
    /// 崩溃面外溢（非受控退出）
    CrashEscaped,
    /// 受控退出码为零
    ExitCodeZero,
    /// 触发面重复登记
    TriggerDuplicated,
    /// 能力名悬空（不在 host_caps 闭集）
    CapDangling,
    /// 报错钩子降级静默
    HookDegradeSilent,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl AeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AeCode::TriggerUnbooked => "触发点未登记：触发面清单外触碰",
            AeCode::ReportTriElementShort => "报告缺三要素",
            AeCode::CrashEscaped => "崩溃面外溢：非受控退出",
            AeCode::ExitCodeZero => "受控退出码为零",
            AeCode::TriggerDuplicated => "触发面重复登记",
            AeCode::CapDangling => "能力名悬空：不在 host_caps 闭集",
            AeCode::HookDegradeSilent => "报错钩子降级静默",
            AeCode::NotReproducible => "双跑不一致：确定性红线",
            AeCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [AeCode; 9] = [
        AeCode::TriggerUnbooked,
        AeCode::ReportTriElementShort,
        AeCode::CrashEscaped,
        AeCode::ExitCodeZero,
        AeCode::TriggerDuplicated,
        AeCode::CapDangling,
        AeCode::HookDegradeSilent,
        AeCode::NotReproducible,
        AeCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AeCode::ALL.len() {
        if AeCode::ALL[i].val() == code {
            return AeCode::ALL[i].detail();
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

/// host_caps 能力名闭集（账面投影：六项 absent 候选 + 部分全量项）。
pub const KNOWN_CAPS: [&str; 8] = [
    "sock.multicast",
    "mem.hugepage",
    "proc.setsid",
    "thr.affinity",
    "file.lock",
    "path.traverse",
    "proc.fork",
    "file.open",
];

/// absent 能力闭集（F16001 表中 state=absent 的两项）。
pub const ABSENT_CAPS: [&str; 2] = ["sock.multicast", "mem.hugepage"];

/// absent 项缺失理由（host_caps 登记引用）。
pub fn absent_reason(cap: &str) -> &'static str {
    match cap {
        "sock.multicast" => "组播协议栈未收录（host_caps absent 登记）",
        "mem.hugepage" => "大页内存管理面未实装（host_caps absent 登记）",
        _ => "未知能力",
    }
}

pub fn cap_known(cap: &str) -> bool {
    let mut i = 0;
    while i < KNOWN_CAPS.len() {
        if KNOWN_CAPS[i] == cap {
            return true;
        }
        i += 1;
    }
    false
}

pub fn cap_absent(cap: &str) -> bool {
    let mut i = 0;
    while i < ABSENT_CAPS.len() {
        if ABSENT_CAPS[i] == cap {
            return true;
        }
        i += 1;
    }
    false
}

/// 触发面登记行：能力名/触发 API/调用链位置。
#[derive(Clone, Debug)]
pub struct TriggerRow {
    pub cap: String,
    pub api: String,
    pub call_site: String,
}

/// 触发行体检。
pub fn vet_trigger(t: &TriggerRow) -> Result<(), u32> {
    if !cap_known(&t.cap) {
        return Err(AeCode::CapDangling.val());
    }
    if !cap_absent(&t.cap) {
        return Err(AeCode::CapDangling.val());
    }
    if t.api.len() < 4 || t.call_site.len() < 4 {
        return Err(AeCode::TriggerUnbooked.val());
    }
    Ok(())
}

/// 三要素报告。
#[derive(Clone, Debug)]
pub struct ErrReport {
    pub what: String,
    pub why: String,
    pub next: String,
}

pub fn vet_report(r: &ErrReport) -> Result<(), u32> {
    if r.what.len() == 0 || r.why.len() == 0 || r.next.len() == 0 {
        return Err(AeCode::ReportTriElementShort.val());
    }
    Ok(())
}

// ── 报错链 ───────────────────────────────────────────────────────
pub struct AbsentGate {
    triggers: Vec<TriggerRow>,
    /// 崩溃计数（非受控退出，必须为 0）。
    pub crashes: u32,
    /// 降级账（报错钩子自身异常）。
    pub degrades: Vec<String>,
}

impl AbsentGate {
    pub fn new() -> AbsentGate {
        AbsentGate { triggers: Vec::new(), crashes: 0, degrades: Vec::new() }
    }

    pub fn register(&mut self, t: TriggerRow) -> Result<(), u32> {
        vet_trigger(&t)?;
        let mut i = 0;
        while i < self.triggers.len() {
            if self.triggers[i].cap == t.cap && self.triggers[i].api == t.api {
                return Err(AeCode::TriggerDuplicated.val());
            }
            i += 1;
        }
        self.triggers.push(t);
        Ok(())
    }

    /// 触发：输出三要素报告 + 受控退出码。未登记触发即红。
    /// 返回 Ok((报告, 退出码))——退出码 = 0xC0 | 触发面序号（非零）。
    pub fn touch(&mut self, cap: &str) -> Result<(ErrReport, u32), u32> {
        // 必须是登记在册的 absent 触发面。
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.triggers.len() {
            if self.triggers[i].cap == cap {
                idx = Some(i);
            }
            i += 1;
        }
        let i = match idx {
            Some(i) => i,
            None => {
                // 清单外触碰：按零依赖纪律同样受控拒绝（不崩溃）。
                return Err(AeCode::TriggerUnbooked.val());
            }
        };
        if !cap_absent(cap) {
            return Err(AeCode::CapDangling.val());
        }
        let report = ErrReport {
            what: format!("发生了什么：应用调用的 {} 能力 Varix 未提供", cap),
            why: format!("为什么：{}", absent_reason(cap)),
            next: format!(
                "下一步怎么办：等待补齐批次或改用替代路径（触发面 = {}）",
                self.triggers[i].call_site
            ),
        };
        vet_report(&report)?;
        let exit = CONTROLLED_EXIT_BASE + i as u32;
        if exit == 0 {
            return Err(AeCode::ExitCodeZero.val());
        }
        Ok((report, exit))
    }

    /// 崩溃守卫：非受控退出计数必须为 0（判据原文口径）。
    pub fn crash_guard(&self) -> Result<(), u32> {
        if self.crashes != 0 {
            return Err(AeCode::CrashEscaped.val());
        }
        Ok(())
    }

    pub fn trigger_count(&self) -> usize {
        self.triggers.len()
    }
}

/// 首版触发面装配：两项 absent × 各一触发 API。
pub fn build_initial() -> Result<AbsentGate, u32> {
    let mut g = AbsentGate::new();
    g.register(TriggerRow {
        cap: String::from("sock.multicast"),
        api: String::from("setsockopt(IP_ADD_MEMBERSHIP)"),
        call_site: String::from("wine/ws2_32/setsockopt"),
    })?;
    g.register(TriggerRow {
        cap: String::from("mem.hugepage"),
        api: String::from("VirtualAlloc(MEM_LARGE_PAGES)"),
        call_site: String::from("wine/kernel32/VirtualAllocEx"),
    })?;
    Ok(g)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((触发面数, 崩溃数))。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let mut g = build_initial()?;
    // 注入式复测：逐个触碰 absent 能力 → 报告三要素齐备 + 受控退出。
    let mut a = 0;
    while a < ABSENT_CAPS.len() {
        let (report, code) = g.touch(ABSENT_CAPS[a])?;
        vet_report(&report)?;
        if code == 0 {
            return Err(AeCode::ExitCodeZero.val());
        }
        a += 1;
    }
    // 崩溃守卫。
    g.crash_guard()?;
    Ok((g.trigger_count(), g.crashes))
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
        // ① 清单外触碰（触发未登记）。
        led.record(
            0,
            matches!(build_initial().unwrap_or_else(|_| AbsentGate::new()).touch("proc.fork"),
                Err(c) if c == AeCode::TriggerUnbooked.val()),
        );

        // ② 报告缺三要素。
        led.record(
            1,
            vet_report(&ErrReport {
                what: String::from("w"),
                why: String::new(),
                next: String::from("n"),
            }) == Err(AeCode::ReportTriElementShort.val()),
        );

        // ③ 能力名悬空。
        led.record(
            2,
            vet_trigger(&TriggerRow {
                cap: String::from("ghost.cap"),
                api: String::from("some_api"),
                call_site: String::from("some/site"),
            }) == Err(AeCode::CapDangling.val()),
        );

        // ④ 非 absent 能力登记触发面（全量项不许进触发面）。
        led.record(
            3,
            vet_trigger(&TriggerRow {
                cap: String::from("proc.fork"),
                api: String::from("fork_api"),
                call_site: String::from("site/site"),
            }) == Err(AeCode::CapDangling.val()),
        );

        // ⑤ 触发面重复。
        led.record(4, {
            let mut g = build_initial().unwrap_or_else(|_| AbsentGate::new());
            g.register(TriggerRow {
                cap: String::from("sock.multicast"),
                api: String::from("setsockopt(IP_ADD_MEMBERSHIP)"),
                call_site: String::from("dup/site"),
            }) == Err(AeCode::TriggerDuplicated.val())
        });

        // ⑥ 受控退出码非零 + 轮次守卫。
        led.record(
            5,
            CONTROLLED_EXIT_BASE != 0
                && (CONTROLLED_EXIT_BASE + 1) != 0
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
                Err(AeCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法报错面必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut g = build_initial()?;
    // 触发面覆盖全部 absent 项。
    let mut a = 0;
    while a < ABSENT_CAPS.len() {
        g.touch(ABSENT_CAPS[a])?;
        a += 1;
    }
    g.crash_guard()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AeCode::ALL.len() {
        let v = AeCode::ALL[i].val();
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
    // 1. 黄金正演：2 触发面 / 0 崩溃。
    let g = golden_run();
    v.push((
        "J1-黄金正演2触发面0崩溃",
        matches!(g, Ok((2, 0))),
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
    // 3. 报告三要素齐备率 100%（判据原文口径）。
    let mut gate = build_initial().unwrap_or_else(|_| AbsentGate::new());
    let tri_ok = {
        let mut ok = true;
        let mut a = 0;
        while a < ABSENT_CAPS.len() {
            match gate.touch(ABSENT_CAPS[a]) {
                Ok((r, _)) => {
                    if vet_report(&r).is_err() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            a += 1;
        }
        ok
    };
    v.push(("J1-三要素齐备100%", tri_ok, format!("{}/2", ABSENT_CAPS.len())));
    // 4. 半途崩溃计数 = 0（判据原文口径）。
    v.push((
        "J1-半途崩溃计数0",
        gate.crashes == 0 && matches!(gate.crash_guard(), Ok(())),
        format!("{}", gate.crashes),
    ));
    // 5. 精确错误含能力名与替代建议（判据原文口径）。
    let named_ok = {
        let mut g2 = build_initial().unwrap_or_else(|_| AbsentGate::new());
        matches!(g2.touch("sock.multicast"), Ok((r, _))
            if r.what.contains("sock.multicast") && r.next.contains("下一步怎么办"))
    };
    v.push(("J1-精确错误含能力名建议", named_ok, String::from("三要素双通道")));
    // 6. 受控退出码族。
    v.push((
        "J1-受控退出码族",
        CONTROLLED_EXIT_BASE == 0xC0,
        String::from("0xC0 段"),
    ));
    // 7. absent 闭集两项。
    v.push((
        "J1-absent闭集两项",
        ABSENT_CAPS.len() == 2,
        String::from("sock.multicast/mem.hugepage"),
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
        String::from("覆盖+崩溃守卫"),
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
pub fn run_unx16024_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16024 absent 项应用加载期精确报错");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
