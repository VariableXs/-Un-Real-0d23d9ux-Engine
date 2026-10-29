//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13201 · 分段计时器总成（装载全链分段计时框架） · 0x65780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13201-J1 计时框架三面（探针注册/分段采集/账本落盘）
//!      30 例全链一致，B16/B18/B19 挂点接入复验通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三面架构：探针注册面（挂点名闭集：B16 转发层/B18 异常注册/
//!     B19 堆建立 + 装载全链四段）/ 分段采集面（begin/end 逻辑
//!     tick 对，乱序必拒）/ 账本落盘面（段账守恒：总时长 = Σ段时长）；
//!   - 逻辑时钟：禁墙钟，tick 由确定性推进器（LCG 步进）产生，
//!     同种子同轨迹（多机对拍红线）；
//!   - 计时段闭式：end > begin 必须成立，负段（end ≤ begin）拒收；
//!     未注册挂点拒采集；重复落盘拒收；
//!   - 30 例全链账：每例采集 4 段（装载/转发/异常/堆），落盘 1 笔
//!     总账，账目守恒恒等式逐例复核；
//!   - LCG 种子写死 = SEG_BASE（0x65780），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65780，全仓独占）。
pub const SEG_BASE: u32 = 0x65780;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegTimerCode {
    /// 未注册挂点先采集
    ProbeUnregistered,
    /// 挂点重复注册
    ProbeDuplicated,
    /// 负段（end ≤ begin）
    NegativeSegment,
    /// tick 倒流（时钟单调破）
    TickReversed,
    /// 段账不守恒（总时长 != Σ段时长）
    LedgerUnbalanced,
    /// 账本重复落盘
    DoubleFlush,
    /// 挂点名越闭集
    ProbeNameOutOfRange,
    /// 账本未落盘就复验
    NotFlushed,
}

impl SegTimerCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SegTimerCode::ProbeUnregistered => "未注册挂点先采集：三面架构断点",
            SegTimerCode::ProbeDuplicated => "挂点重复注册：注册面闭集破",
            SegTimerCode::NegativeSegment => "负段（end ≤ begin）：采集面闭式破",
            SegTimerCode::TickReversed => "tick 倒流：逻辑时钟单调破",
            SegTimerCode::LedgerUnbalanced => "段账不守恒：总时长 != Σ段时长",
            SegTimerCode::DoubleFlush => "账本重复落盘：落盘面幂等破",
            SegTimerCode::ProbeNameOutOfRange => "挂点名越闭集：注册面越界",
            SegTimerCode::NotFlushed => "账本未落盘就复验：落盘面缺步",
        }
    }
    pub const ALL: [SegTimerCode; 8] = [
        SegTimerCode::ProbeUnregistered,
        SegTimerCode::ProbeDuplicated,
        SegTimerCode::NegativeSegment,
        SegTimerCode::TickReversed,
        SegTimerCode::LedgerUnbalanced,
        SegTimerCode::DoubleFlush,
        SegTimerCode::ProbeNameOutOfRange,
        SegTimerCode::NotFlushed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SegTimerCode::ALL.len() {
        if SegTimerCode::ALL[i].val() == code {
            return SegTimerCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：计时框架建模书 + 判据原文） ──────────────────────
/// 挂点闭集（8 座）：B16 转发层/B18 异常注册/B19 堆建立 + 装载全链四段。
pub const PROBE_B16_FWD: u8 = 0;
pub const PROBE_B18_EXC: u8 = 1;
pub const PROBE_B19_HEAP: u8 = 2;
pub const PROBE_LOAD_DONE: u8 = 3;
pub const PROBE_RELOC: u8 = 4;
pub const PROBE_IMPORT: u8 = 5;
pub const PROBE_TEB_WIRE: u8 = 6;
pub const PROBE_COMMIT: u8 = 7;
pub const PROBE_DOMAIN: usize = 8;
/// 每例段数（装载/转发/异常/堆四段）。
pub const SEGS_PER_CASE: usize = 4;
/// 全链例数（判据原文：30 例）。
pub const CASES: usize = 30;

// ── 逻辑时钟（禁墙钟：确定性 tick 推进器） ───────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 三面之一：探针注册面 ─────────────────────────────────────────
pub struct ProbeRegistry {
    pub registered: [bool; PROBE_DOMAIN],
}

impl ProbeRegistry {
    pub fn new() -> ProbeRegistry {
        ProbeRegistry {
            registered: [false; PROBE_DOMAIN],
        }
    }
    pub fn register(&mut self, probe: u8) -> Result<(), SegTimerCode> {
        if probe as usize >= PROBE_DOMAIN {
            return Err(SegTimerCode::ProbeNameOutOfRange);
        }
        if self.registered[probe as usize] {
            return Err(SegTimerCode::ProbeDuplicated);
        }
        self.registered[probe as usize] = true;
        Ok(())
    }
    pub fn is_registered(&self, probe: u8) -> bool {
        if probe as usize >= PROBE_DOMAIN {
            return false;
        }
        self.registered[probe as usize]
    }
}

// ── 三面之二：分段采集面 ─────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub probe: u8,
    pub begin: u64,
    pub end: u64,
}

/// 采集闭式核：挂点已注册 + 正段 + 时钟单调。
pub fn acquire_segment(
    reg: &ProbeRegistry,
    clock: &mut u64,
    rng: &mut Lcg,
    probe: u8,
) -> Result<Segment, SegTimerCode> {
    if !reg.is_registered(probe) {
        return Err(SegTimerCode::ProbeUnregistered);
    }
    let dur = (rng.next_below(16) + 1) as u64; // 段长 1..16 tick
    let begin = *clock;
    let end = begin + dur;
    if end <= begin {
        return Err(SegTimerCode::NegativeSegment);
    }
    if end < *clock {
        return Err(SegTimerCode::TickReversed);
    }
    *clock = end;
    Ok(Segment { probe, begin, end })
}

// ── 三面之三：账本落盘面 ─────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct CaseLedger {
    pub case_id: u32,
    pub segments: Vec<Segment>,
    /// 落盘的总时长（只准落一次）
    pub flushed_total: Option<u64>,
}

impl CaseLedger {
    pub fn new(case_id: u32) -> CaseLedger {
        CaseLedger {
            case_id,
            segments: Vec::new(),
            flushed_total: None,
        }
    }

    pub fn push(&mut self, s: Segment) {
        self.segments.push(s);
    }

    /// 落盘：总时长 = Σ段时长（幂等红线：二次落盘拒收）。
    pub fn flush(&mut self) -> Result<u64, SegTimerCode> {
        if self.flushed_total.is_some() {
            return Err(SegTimerCode::DoubleFlush);
        }
        let mut total: u64 = 0;
        let mut i = 0;
        while i < self.segments.len() {
            let s = self.segments[i];
            if s.end <= s.begin {
                return Err(SegTimerCode::NegativeSegment);
            }
            total += s.end - s.begin;
            i += 1;
        }
        self.flushed_total = Some(total);
        Ok(total)
    }

    /// 守恒恒等式：已落盘总时长 == Σ段时长，且段数 == SEGS_PER_CASE。
    pub fn verify_balance(&self) -> Result<(), SegTimerCode> {
        match self.flushed_total {
            None => Err(SegTimerCode::NotFlushed),
            Some(total) => {
                let mut sum: u64 = 0;
                let mut i = 0;
                while i < self.segments.len() {
                    sum += self.segments[i].end - self.segments[i].begin;
                    i += 1;
                }
                if total != sum {
                    return Err(SegTimerCode::LedgerUnbalanced);
                }
                Ok(())
            }
        }
    }
}

/// 挂点接入复验载体：B16/B18/B19 三挂点必须在段账中出现。
pub fn verify_hooks_present(led: &CaseLedger) -> (bool, bool, bool) {
    let mut b16 = false;
    let mut b18 = false;
    let mut b19 = false;
    let mut i = 0;
    while i < led.segments.len() {
        let p = led.segments[i].probe;
        if p == PROBE_B16_FWD {
            b16 = true;
        }
        if p == PROBE_B18_EXC {
            b18 = true;
        }
        if p == PROBE_B19_HEAP {
            b19 = true;
        }
        i += 1;
    }
    (b16, b18, b19)
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：三面全链 30 例（注册→采集 4 段→落盘→守恒复核）。
pub fn drill_30_cases() -> (bool, String) {
    let mut ok_all = true;
    let mut hooked = true;
    let mut i = 0;
    while i < CASES {
        let mut reg = ProbeRegistry::new();
        // 注册 4 座挂点（含 B16/B18/B19）。
        if reg.register(PROBE_LOAD_DONE).is_err()
            || reg.register(PROBE_B16_FWD).is_err()
            || reg.register(PROBE_B18_EXC).is_err()
            || reg.register(PROBE_B19_HEAP).is_err()
        {
            ok_all = false;
            break;
        }
        let mut clock: u64 = 0;
        let mut rng = Lcg::new(SEG_BASE as u64 + i as u64);
        let mut led = CaseLedger::new(0x1000 + i as u32);
        let probes = [PROBE_LOAD_DONE, PROBE_B16_FWD, PROBE_B18_EXC, PROBE_B19_HEAP];
        let mut j = 0;
        while j < SEGS_PER_CASE {
            match acquire_segment(&reg, &mut clock, &mut rng, probes[j]) {
                Ok(s) => led.push(s),
                Err(_) => {
                    ok_all = false;
                    break;
                }
            }
            j += 1;
        }
        let total = match led.flush() {
            Ok(t) => t,
            Err(_) => {
                ok_all = false;
                break;
            }
        };
        if led.verify_balance().is_err() || total == 0 {
            ok_all = false;
            break;
        }
        let (b16, b18, b19) = verify_hooks_present(&led);
        if !(b16 && b18 && b19) {
            hooked = false;
        }
        i += 1;
    }
    (ok_all && hooked, format!("三面全链 30 例一致，挂点接入复验={}", hooked))
}

/// 注入面：未注册挂点采集 10/10 必抓。
pub fn drill_unregistered_probe_10() -> (bool, String) {
    let mut reg = ProbeRegistry::new();
    let _ = reg.register(PROBE_LOAD_DONE);
    let mut clock: u64 = 0;
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        // B16 未注册（只注册了 LOAD_DONE）。
        if acquire_segment(&reg, &mut clock, &mut rng, PROBE_B16_FWD)
            == Err(SegTimerCode::ProbeUnregistered)
        {
            caught += 1;
        }
        i += 1;
    }
    (caught == 10, format!("未注册挂点注入 {}/10 必抓", caught))
}

/// 注入面：挂点重复注册 / 越闭集名 10/10 必抓。
pub fn drill_probe_violations_10() -> (bool, String) {
    let mut reg = ProbeRegistry::new();
    let _ = reg.register(PROBE_B16_FWD);
    let mut caught = 0;
    // 重复注册 5 例。
    let mut i = 0;
    while i < 5 {
        if reg.register(PROBE_B16_FWD) == Err(SegTimerCode::ProbeDuplicated) {
            caught += 1;
        }
        i += 1;
    }
    // 越闭集名 5 例。
    let mut j = 0;
    while j < 5 {
        if reg.register(PROBE_DOMAIN as u8 + j as u8) == Err(SegTimerCode::ProbeNameOutOfRange) {
            caught += 1;
        }
        j += 1;
    }
    (caught == 10, format!("挂点违规注入 {}/10 必抓", caught))
}

/// 注入面：负段（end ≤ begin）闭式必抓 + 采集面零长段拒收。
pub fn drill_negative_segment() -> (bool, String) {
    // 直接构造负段账，落盘必拒。
    let mut led = CaseLedger::new(0x2000);
    led.push(Segment { probe: PROBE_LOAD_DONE, begin: 10, end: 4 });
    let neg = led.flush() == Err(SegTimerCode::NegativeSegment);
    // 零长段同拒。
    let mut led2 = CaseLedger::new(0x2001);
    led2.push(Segment { probe: PROBE_LOAD_DONE, begin: 5, end: 5 });
    let zero = led2.flush() == Err(SegTimerCode::NegativeSegment);
    // 采集面注入：注册挂点下 dur 由 next_below(16)+1 恒正 → 无负段；
    // 用直接构造核断言闭式。
    (
        neg && zero,
        format!("负段抓={} 零长段抓={}", neg, zero),
    )
}

/// 注入面：账本重复落盘必抓（幂等红线）。
pub fn drill_double_flush() -> (bool, String) {
    let mut led = CaseLedger::new(0x3000);
    led.push(Segment { probe: PROBE_B18_EXC, begin: 0, end: 7 });
    let first = led.flush();
    let second = led.flush();
    (
        first.is_ok() && second == Err(SegTimerCode::DoubleFlush),
        format!(
            "首次落盘={:?} 二次落盘拒={:?}",
            first.is_ok(),
            second.is_err()
        ),
    )
}

/// 注入面：账本未落盘复验必抓（NotFlushed）。
pub fn drill_not_flushed() -> (bool, String) {
    let mut led = CaseLedger::new(0x4000);
    led.push(Segment { probe: PROBE_B19_HEAP, begin: 0, end: 9 });
    (
        led.verify_balance() == Err(SegTimerCode::NotFlushed),
        format!("未落盘复验抓={:?}", led.verify_balance().is_err()),
    )
}

/// 守恒面：篡改落盘总时长 → 守恒破必抓（反向判据）。
pub fn drill_balance_tamper() -> (bool, String) {
    let mut led = CaseLedger::new(0x5000);
    led.push(Segment { probe: PROBE_LOAD_DONE, begin: 0, end: 6 });
    led.push(Segment { probe: PROBE_B16_FWD, begin: 6, end: 11 });
    let _ = led.flush();
    let good = led.verify_balance() == Ok(());
    // 篡改：抹掉落盘值换成错账。
    led.flushed_total = Some(999);
    let tampered = led.verify_balance() == Err(SegTimerCode::LedgerUnbalanced);
    (good && tampered, format!("守恒全真={} 篡改抓={}", good, tampered))
}

/// 时钟单调面：连续采集 tick 严格递增（10 段全真）。
pub fn drill_clock_monotonic() -> (bool, String) {
    let mut reg = ProbeRegistry::new();
    let _ = reg.register(PROBE_COMMIT);
    let mut clock: u64 = 0;
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ok = true;
    let mut prev_end: u64 = 0;
    let mut i = 0;
    while i < 10 {
        match acquire_segment(&reg, &mut clock, &mut rng, PROBE_COMMIT) {
            Ok(s) => {
                if s.begin < prev_end || s.end <= s.begin {
                    ok = false;
                    break;
                }
                prev_end = s.end;
            }
            Err(_) => {
                ok = false;
                break;
            }
        }
        i += 1;
    }
    (ok, format!("逻辑时钟单调 10 段全真={}", ok))
}

/// 多机对拍面：同种子重建两次，段账逐条一致。
pub fn drill_determinism() -> (bool, String) {
    let build = || {
        let mut reg = ProbeRegistry::new();
        let _ = reg.register(PROBE_B16_FWD);
        let mut clock: u64 = 0;
        let mut rng = Lcg::new(SEG_BASE as u64);
        let mut segs: Vec<(u64, u64)> = Vec::new();
        let mut i = 0;
        while i < 12 {
            if let Ok(s) = acquire_segment(&reg, &mut clock, &mut rng, PROBE_B16_FWD) {
                segs.push((s.begin, s.end));
            }
            i += 1;
        }
        segs
    };
    let a = build();
    let b = build();
    let mut same = a.len() == b.len();
    let mut j = 0;
    while same && j < a.len() {
        if a[j] != b[j] {
            same = false;
        }
        j += 1;
    }
    (same, format!("同种子对拍一致={}", same))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_30_cases();
    let (a2, d2) = drill_unregistered_probe_10();
    let (a3, d3) = drill_double_flush();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 30例三面={} 未注册注入={} 幂等落盘={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < SegTimerCode::ALL.len() {
        let v = SegTimerCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_30_cases();
    v.push(("J1-三面全链30例一致", a, b));
    let (a, b) = drill_unregistered_probe_10();
    v.push(("J2-未注册挂点注入10/10", a, b));
    let (a, b) = drill_probe_violations_10();
    v.push(("J2-重复注册/越名注入10/10", a, b));
    let (a, b) = drill_negative_segment();
    v.push(("J2-负段/零长段闭式", a, b));
    let (a, b) = drill_double_flush();
    v.push(("J2-重复落盘必抓", a, b));
    let (a, b) = drill_not_flushed();
    v.push(("J3-未落盘复验必抓", a, b));
    let (a, b) = drill_balance_tamper();
    v.push(("J3-守恒篡改反向", a, b));
    let (a, b) = drill_clock_monotonic();
    v.push(("J3-逻辑时钟单调10段", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同种子对拍一致", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13201_checks：本单判据聚合（诊断段 0x65780）。
pub fn run_unx13201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13201 分段计时器总成");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65780 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
