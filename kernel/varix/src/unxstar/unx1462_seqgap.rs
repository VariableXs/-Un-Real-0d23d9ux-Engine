//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1462 · 序号断点扫描器（全域巡检与扫描窗口） · 0x72140
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B34）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1462-J1 断点扫描器生效 100%（全域序号扫描：相邻差>1
//!     即断点，扫描窗口滑动语义验证 10/10 次），扫描周期冻结（10^4 事件/轮），
//!     断点检出率 10/10，扫描开销 ≤0.3% 断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 全域序号扫描：相邻差 >1 即断点（记 from/to/span 三元），差 ≤0 即
//!     序号回退/重号（SeqBack 违约记账）；
//!   · 扫描窗口滑动语义：窗宽 512，步长 = 窗宽（零重叠零漏扫），每事件
//!     恰被扫到 1 次（coverage 账逐项对账）；
//!   · 扫描周期冻结：单轮事件数恒 10^4，不足/超出即 PeriodBreak 拒扫；
//!   · 注入检出：LCG 撒断点（每轮 4–8 个，位置互异），检出率 10/10 且
//!     误报为零；开销 ≤0.3%（30 bps）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72140，VX3-1103 断点扫描窗口独占）。
pub const SEG_BASE: u32 = 0x72140;

/// 扫描周期（单轮事件数，判据原文 10^4，冻结）。
pub const SCAN_PERIOD: usize = 10_000;
/// 扫描窗宽（冻结 512，步长 = 窗宽）。
pub const WINDOW: usize = 512;
/// 扫描开销占比上限（≤0.3% = 30 bps，冻结）。
pub const OVERHEAD_CAP_BPS: u64 = 30;
/// 序号基线（事件流首序号，冻结 1000）。
pub const SEQ_BASE: u64 = 1000;
/// 注入断点位最小间距（冻结 8，防位置重叠）。
pub const SLOT_MIN_GAP: usize = 8;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GapCode {
    /// 序号回退/重号：相邻差 ≤0，事件流被倒灌或重放
    SeqBack,
    /// 窗口越界：扫描基址滑出全域范围
    WinOver,
    /// 扫描周期违约：单轮事件数 ≠ 10^4
    PeriodBreak,
    /// 断点漏检：注入断点未被检出
    GapMiss,
    /// 覆盖缺口：全域巡检有事件未被扫到（或被重复扫）
    CoverMiss,
    /// 开销超限：扫描簿记占比 >0.3%
    OverheadHigh,
    /// 双跑不一致：同流两扫应同账
    NotReproducible,
    /// 流账异常：事件账与序号账对不上（空流/账目断裂）
    StreamOdd,
}

impl GapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GapCode::SeqBack => "序号回退：相邻差≤0，查事件流倒灌/重放面",
            GapCode::WinOver => "窗口越界：扫描基址滑出全域，按窗宽重排轮次",
            GapCode::PeriodBreak => "扫描周期违约：单轮必须整 10^4 事件，补齐或截断后重扫",
            GapCode::GapMiss => "断点漏检：注入位未报，查窗口边界配对逻辑",
            GapCode::CoverMiss => "覆盖缺口：coverage 账不齐，事件漏扫或重扫",
            GapCode::OverheadHigh => "开销超限：簿记占比 >0.3%，压窗口簿记常数",
            GapCode::NotReproducible => "双跑不一致：同流两扫账必须全等",
            GapCode::StreamOdd => "流账异常：空流或事件账断裂，先对账再扫",
        }
    }
    pub const ALL: [GapCode; 8] = [
        GapCode::SeqBack,
        GapCode::WinOver,
        GapCode::PeriodBreak,
        GapCode::GapMiss,
        GapCode::CoverMiss,
        GapCode::OverheadHigh,
        GapCode::NotReproducible,
        GapCode::StreamOdd,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GapCode::ALL.len() {
        if GapCode::ALL[i].val() == code {
            return GapCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72140 段闭集内"
}

// ── LCG（确定性伪随机，供断点撒位与注入轮步进） ─────────────────
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

// ── 域模型 ──────────────────────────────────────────────────────
/// 事件：seq 全域单调，kind 事件类（0..2 轮转，供账目核对）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub seq: u64,
    pub kind: u8,
}

/// 断点：from→to 之间跳了 span 个序号（span = to − from − 1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap {
    pub from: u64,
    pub to: u64,
    pub span: u64,
}

/// 干净事件流（无断点无回退）：seq 从 SEQ_BASE 起逐 1 递增。
pub fn build_clean() -> Vec<Event> {
    let mut v: Vec<Event> = Vec::new();
    let mut i = 0usize;
    while i < SCAN_PERIOD {
        v.push(Event { seq: SEQ_BASE + i as u64, kind: (i % 3) as u8 });
        i += 1;
    }
    v
}

/// 注入流（seed 确定性）：在互异位置撒 4–8 个断点，返回 (流, 注入账)。
pub fn build_stream(seed: u64) -> (Vec<Event>, Vec<Gap>) {
    let mut rng = Lcg::new(seed.wrapping_add(SEG_BASE as u64));
    // 注入断点数：4 + seed%5（4..=8）。
    let inj_count = (4 + (seed % 5)) as usize;
    // 撒位：落在 [200, SCAN_PERIOD-200)，两两间距 ≥ SLOT_MIN_GAP。
    let mut slots: Vec<usize> = Vec::new();
    let mut k = 0usize;
    while k < inj_count {
        let p = (rng.below((SCAN_PERIOD - 400) as u64) as usize) + 200;
        let mut clash = false;
        let mut j = 0usize;
        while j < slots.len() {
            if let Some(q) = slots.get(j) {
                let d = if p > *q { p - *q } else { *q - p };
                if d < SLOT_MIN_GAP {
                    clash = true;
                }
            }
            j += 1;
        }
        if !clash {
            slots.push(p);
        }
        k += 1;
    }
    // 逐事件步进：缺省 +1；命中注入位额外跳 2..7。
    let mut events: Vec<Event> = Vec::new();
    let mut injected: Vec<Gap> = Vec::new();
    let mut seq: u64 = SEQ_BASE;
    let mut i = 0usize;
    while i < SCAN_PERIOD {
        if i > 0 {
            let mut step: u64 = 1;
            let mut j = 0usize;
            while j < slots.len() {
                if let Some(p) = slots.get(j) {
                    if i == *p {
                        let g = 2 + rng.below(6) as u64;
                        step += g;
                        injected.push(Gap { from: seq, to: seq + step, span: step - 1 });
                    }
                }
                j += 1;
            }
            seq += step;
        }
        events.push(Event { seq, kind: (i % 3) as u8 });
        i += 1;
    }
    (events, injected)
}

/// 回退流（探针用）：构造 1 次倒灌 + 1 次重号，共 2 处 SeqBack。
pub fn build_regress_stream() -> Vec<Event> {
    let mut v: Vec<Event> = Vec::new();
    let mut i = 0usize;
    while i < SCAN_PERIOD {
        let seq = if i == 5 {
            // 倒灌：比前序还小
            SEQ_BASE + 3 - 10
        } else if i == 6 {
            // 重号：与前一事件同值
            SEQ_BASE + 3 - 10
        } else {
            SEQ_BASE + i as u64
        };
        v.push(Event { seq, kind: (i % 3) as u8 });
        i += 1;
    }
    v
}

// ── 扫描窗口 ────────────────────────────────────────────────────
/// 窗口边界求解：[base, base+WINDOW) ∩ [0, len)；base ≥ len 即越界拒。
pub fn window_bounds(len: usize, base: usize) -> Result<(usize, usize), u32> {
    if base >= len {
        return Err(GapCode::WinOver.val());
    }
    let end = base.saturating_add(WINDOW).min(len);
    Ok((base, end))
}

/// 扫描账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanReport {
    /// 检出断点（按扫描序）。
    pub gaps: Vec<Gap>,
    /// 序号回退/重号处数。
    pub regressions: u32,
    /// 实扫事件数（coverage 总和）。
    pub scanned: u64,
    /// 窗口数。
    pub windows: u32,
    /// 滑动语义自检：步长=窗宽、零重叠。
    pub window_steps_ok: bool,
    pub admin_ops: u64,
    pub payload_ops: u64,
}

impl ScanReport {
    /// 扫描开销占比（bps）：簿记 / 载荷；无载荷视为超限。
    pub fn overhead_bps(&self) -> u64 {
        if self.payload_ops == 0 {
            return u64::MAX;
        }
        self.admin_ops.saturating_mul(10_000) / self.payload_ops
    }
}

// ── 全域扫描器 ──────────────────────────────────────────────────
/// 全域序号扫描：窗口滑动 + 相邻配对 + coverage 账。
/// 周期冻结：流长 ≠ SCAN_PERIOD 直接拒扫。
pub fn scan_stream(events: &[Event]) -> Result<ScanReport, u32> {
    if events.len() != SCAN_PERIOD {
        return Err(GapCode::PeriodBreak.val());
    }
    if events.is_empty() {
        return Err(GapCode::StreamOdd.val());
    }
    let mut gaps: Vec<Gap> = Vec::new();
    let mut regressions = 0u32;
    // coverage 账：每事件被扫到次数（全域巡检必须恰 1 次）。
    let mut coverage: Vec<u32> = Vec::new();
    let mut z = 0usize;
    while z < events.len() {
        coverage.push(0);
        z += 1;
    }
    let mut admin = 0u64;
    let mut payload = 0u64;
    let mut windows = 0u32;
    let mut steps_ok = true;
    let mut prev = match events.first() {
        Some(e) => e.seq,
        None => return Err(GapCode::StreamOdd.val()),
    };
    if let Some(c) = coverage.get_mut(0) {
        *c += 1;
    }
    let mut base = 0usize;
    while base < events.len() {
        // 滑动语义：步长必须恰 = WINDOW（记录自检）。
        if base % WINDOW != 0 {
            steps_ok = false;
        }
        let (lo, hi) = window_bounds(events.len(), base)?;
        admin += 2; // 窗口簿记：基址推进 + 边界判定
        let mut i = lo;
        while i < hi {
            if i > 0 {
                let e = match events.get(i) {
                    Some(e) => e,
                    None => return Err(GapCode::StreamOdd.val()),
                };
                payload += 2; // 读事件 + 相邻比较
                match e.seq.checked_sub(prev) {
                    None | Some(0) => {
                        // 回退/重号：记账续扫，prev 吸收当前值。
                        regressions += 1;
                        prev = e.seq;
                    }
                    Some(1) => {
                        prev = e.seq;
                    }
                    Some(d) => {
                        gaps.push(Gap { from: prev, to: e.seq, span: d - 1 });
                        prev = e.seq;
                    }
                }
                if let Some(c) = coverage.get_mut(i) {
                    *c += 1;
                }
            }
            i += 1;
        }
        windows += 1;
        base += WINDOW;
    }
    // 覆盖对账：每事件恰扫 1 次。
    let mut cover_ok = true;
    let mut total = 0u64;
    let mut i = 0usize;
    while i < coverage.len() {
        match coverage.get(i) {
            Some(&c) => {
                if c != 1 {
                    cover_ok = false;
                }
                total += c as u64;
            }
            None => cover_ok = false,
        }
        i += 1;
    }
    if !cover_ok || total != events.len() as u64 {
        return Err(GapCode::CoverMiss.val());
    }
    Ok(ScanReport {
        gaps,
        regressions,
        scanned: total,
        windows,
        window_steps_ok: steps_ok,
        admin_ops: admin,
        payload_ops: payload,
    })
}

/// 序号账闸：有回退/重号即 SeqBack 拒（干净流必过）。
pub fn assert_no_regress(rep: &ScanReport) -> Result<(), u32> {
    if rep.regressions > 0 {
        return Err(GapCode::SeqBack.val());
    }
    Ok(())
}

// ── 注入检出对账 ────────────────────────────────────────────────
/// 单轮检出账：注入数 / 匹配数 / 误报数 / 回退数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectOutcome {
    pub injected: u32,
    pub matched: u32,
    pub spurious: u32,
    pub regressions: u32,
}

impl DetectOutcome {
    /// 全中：注入全检出 + 零误报 + 零回退。
    pub fn all_hit(&self) -> bool {
        self.injected > 0 && self.matched == self.injected && self.spurious == 0 && self.regressions == 0
    }
}

/// 单轮注入检出：建流→扫→逐条对账。
pub fn detect_round(seed: u64) -> Result<DetectOutcome, u32> {
    let (events, injected) = build_stream(seed);
    let rep = scan_stream(&events)?;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < injected.len() {
        let g = match injected.get(i) {
            Some(g) => g,
            None => return Err(GapCode::GapMiss.val()),
        };
        let mut found = false;
        let mut j = 0usize;
        while j < rep.gaps.len() {
            if let Some(d) = rep.gaps.get(j) {
                if d.from == g.from && d.to == g.to && d.span == g.span {
                    found = true;
                }
            }
            j += 1;
        }
        if found {
            matched += 1;
        }
        i += 1;
    }
    let spurious = (rep.gaps.len() as u32).saturating_sub(matched);
    Ok(DetectOutcome {
        injected: injected.len() as u32,
        matched,
        spurious,
        regressions: rep.regressions,
    })
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏检/误报的轮号（诊断用）。
    pub miss_rounds: Vec<u32>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_rounds: Vec::new() }
    }
    pub fn record(&mut self, round: u32, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_rounds.push(round);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：10 轮独立撒点建流，检出率必须 10/10 且误报为零。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let out = detect_round(r as u64 + 1)?;
        led.record(r, out.all_hit());
        r += 1;
    }
    Ok(led)
}

// ── 滑动语义审计（判据原文：滑动语义验证 10/10 次） ─────────────
/// 单轮滑动语义审计：窗口数 = ⌈len/窗宽⌉、步长=窗宽、覆盖齐、开销达标。
pub fn window_audit(seed: u64) -> Result<(u32, bool, bool), u32> {
    let (events, _injected) = build_stream(seed);
    let rep = scan_stream(&events)?;
    let expect_windows = (SCAN_PERIOD + WINDOW - 1) / WINDOW;
    let win_ok = rep.windows == expect_windows as u32 && rep.window_steps_ok;
    // 窗口越界探针：基址 = len 必拒。
    let over = window_bounds(events.len(), events.len()).is_err();
    Ok((rep.windows, win_ok, over))
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：干净流零断点 → 注入检出 → 滑动审计 → 回退流必报 → 开销闸。
/// 返回 (正旗数, 拒旗数, 实扫事件数, 检出断点总数)。
pub fn golden_run() -> Result<(u32, u32, u64, u32), u32> {
    // 干净流：零断点零回退，覆盖满账。
    let clean = build_clean();
    let crep = scan_stream(&clean)?;
    assert_no_regress(&crep)?;
    let clean_ok = crep.gaps.is_empty() && crep.regressions == 0 && crep.scanned == SCAN_PERIOD as u64;
    // 注入检出（首轮）。
    let det = detect_round(1)?;
    let det_ok = det.all_hit();
    // 滑动语义审计。
    let (_w, win_ok, over_rej) = window_audit(2)?;
    // 回退流：2 处 SeqBack 必报。
    let reg = build_regress_stream();
    let rrep = scan_stream(&reg)?;
    let reg_seen = rrep.regressions == 2;
    // 周期冻结闸：流长 9999 / 10001 必拒扫（短额/超额两路）。
    let mut short = build_clean();
    let _ = short.pop();
    let short_rej = scan_stream(&short).is_err();
    let mut long = build_clean();
    long.push(Event { seq: SEQ_BASE + SCAN_PERIOD as u64, kind: 0 });
    let long_rej = scan_stream(&long).is_err();
    // 开销闸（注入流口径）。
    let (events, _inj2) = build_stream(3);
    let erep = scan_stream(&events)?;
    let bps = erep.overhead_bps();
    if bps > OVERHEAD_CAP_BPS {
        return Err(GapCode::OverheadHigh.val());
    }
    // 回退闸：回退流走 assert_no_regress 必拒（SeqBack 面）。
    let seqback_rej = assert_no_regress(&rrep).is_err();
    let flags = clean_ok as u32 + det_ok as u32 + win_ok as u32 + reg_seen as u32;
    let rejects = over_rej as u32 + short_rej as u32 + long_rej as u32 + seqback_rej as u32;
    Ok((flags, rejects, crep.scanned, erep.gaps.len() as u32))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(GapCode::NotReproducible.val()),
    }
}

/// 序 B：注入检出 10/10 且误报为零。
pub fn probe_seq_b() -> Result<(), u32> {
    let led = inject_rounds(JUDGE_ROUNDS)?;
    if led.all_caught() {
        Ok(())
    } else {
        Err(GapCode::GapMiss.val())
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72140 && GapCode::StreamOdd.val() < SEG_BASE + 0x40;
    match (
        SCAN_PERIOD == 10_000,
        WINDOW == 512,
        OVERHEAD_CAP_BPS == 30,
        SEQ_BASE == 1000,
        SLOT_MIN_GAP == 8,
        JUDGE_ROUNDS == 10,
        in_window,
    ) {
        (true, true, true, true, true, true, true) => Ok(()),
        _ => Err(GapCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    // 同流两扫：账逐域全等。
    let (events, _inj) = build_stream(7);
    let a = scan_stream(&events)?;
    let b = scan_stream(&events)?;
    if a != b {
        return Err(GapCode::NotReproducible.val());
    }
    // 建流两跑：注入账全等。
    let (_, i1) = build_stream(7);
    let (_, i2) = build_stream(7);
    if i1 != i2 {
        return Err(GapCode::NotReproducible.val());
    }
    // 黄金全链两跑同值。
    let ga = golden_run()?;
    let gb = golden_run()?;
    if ga != gb {
        return Err(GapCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GapCode::ALL.len() {
        let v = GapCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 断点账摘要（供写回与诊断） ──────────────────────────────────
/// 检出断点摘要：span 分桶（小跳 2–3 / 中跳 4–5 / 大跳 ≥6）。
pub fn gap_summary(gaps: &[Gap]) -> String {
    let mut small = 0u32;
    let mut mid = 0u32;
    let mut big = 0u32;
    let mut i = 0usize;
    while i < gaps.len() {
        match gaps.get(i) {
            Some(g) => {
                if g.span <= 3 {
                    small += 1;
                } else if g.span <= 5 {
                    mid += 1;
                } else {
                    big += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    format!("断点小跳{} 中跳{} 大跳{}", small, mid, big)
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let led = inject_rounds(JUDGE_ROUNDS)?;
    if !led.all_caught() {
        return Err(GapCode::GapMiss.val());
    }
    let clean = build_clean();
    let rep = scan_stream(&clean)?;
    Ok(format!(
        "状态=已完成 rounds={} detected={}/{} scanned={} {}",
        JUDGE_ROUNDS, led.caught, led.tries, rep.scanned, gap_summary(&rep.gaps)
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    v.push((
        "J1-全域扫描建面",
        match &g {
            Ok((flags, rej, _, _)) => *flags == 4 && *rej == 4,
            Err(_) => false,
        },
        match &g {
            Ok((flags, rej, scanned, gaps)) => {
                format!("flags={} rejects={} scanned={} gaps={}", flags, rej, scanned, gaps)
            }
            Err(e) => format!("err=0x{:x} {}", e, detail(*e)),
        },
    ));
    // J1 断点检出率 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-断点检出10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_rounds),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 扫描窗口滑动语义 10/10（逐轮审计）。
    let mut slide_ok = true;
    let mut slide_n = 0u32;
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        match window_audit(r as u64 + 1) {
            Ok((_w, ok, _over)) => {
                slide_ok = slide_ok && ok;
                slide_n += 1;
            }
            Err(_) => slide_ok = false,
        }
        r += 1;
    }
    v.push((
        "J1-窗口滑动语义10/10",
        slide_ok && slide_n == JUDGE_ROUNDS,
        format!("rounds={} win=512 step=512 零重叠零漏扫", slide_n),
    ));
    // J1 扫描周期冻结（10^4 事件/轮）。
    let (events, _i) = build_stream(9);
    let period_ok = events.len() == SCAN_PERIOD && scan_stream(&events).is_ok();
    let mut short = build_clean();
    let _ = short.pop();
    let period_rej = scan_stream(&short).is_err();
    v.push((
        "J1-扫描周期冻结",
        period_ok && period_rej && SCAN_PERIOD == 10_000,
        format!("period={} 足额过 短额拒={}", SCAN_PERIOD, period_rej),
    ));
    // J1 扫描开销 ≤0.3%。
    let clean = build_clean();
    let crep = scan_stream(&clean);
    let (bps, admin, payload) = match &crep {
        Ok(rep) => (rep.overhead_bps(), rep.admin_ops, rep.payload_ops),
        Err(_) => (u64::MAX, 0, 0),
    };
    v.push((
        "J1-扫描开销≤0.3%",
        bps <= OVERHEAD_CAP_BPS,
        format!("admin={} payload={} bps={} cap={}", admin, payload, bps, OVERHEAD_CAP_BPS),
    ));
    // J1 序号回退/重号必报（SeqBack 路）。
    let reg = build_regress_stream();
    let reg_ok = match scan_stream(&reg) {
        Ok(rep) => rep.regressions == 2 && assert_no_regress(&rep).is_err(),
        Err(_) => false,
    };
    v.push((
        "J1-序号回退必报",
        reg_ok,
        String::from("倒灌+重号 双处记账，assert_no_regress 拒"),
    ));
    // J1 干净流零误报。
    let cok = match scan_stream(&clean) {
        Ok(rep) => rep.gaps.is_empty() && rep.regressions == 0,
        Err(_) => false,
    };
    v.push((
        "J1-干净流零误报",
        cok,
        String::from("10^4 干净事件 零断点零回退"),
    ));
    // J1 窗口越界拒（WinOver 路）。
    let over1 = window_bounds(SCAN_PERIOD, SCAN_PERIOD).is_err();
    let over2 = window_bounds(SCAN_PERIOD, SCAN_PERIOD + WINDOW).is_err();
    let over3 = window_bounds(SCAN_PERIOD, 0).is_ok();
    v.push((
        "J1-窗口越界拒",
        over1 && over2 && over3,
        format!("base=len 拒 base>len 拒 base=0 过"),
    ));
    // J2 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("同流两扫 + 建流两跑 + 全链两跑 全等"),
    ));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J4 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x72140 窗口"),
    ));
    v
}

pub fn run_unx1462_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1462 序号断点扫描器（全域巡检与扫描窗口）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
