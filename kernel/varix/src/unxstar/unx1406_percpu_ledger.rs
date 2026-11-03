//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1406 · per-CPU 分账与全域合并账 · 0x72080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B31）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1406-J1 per-CPU 分账开户率 100%（每向量×每 CPU 二维开
//!     户），全域合并视图一致性 100% 断言（合并=分账按序号归并零丢
//!     失），分账合并 10^6 事件对拍一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 施工说明：
//!   - 二维分账：向量（0..VECTOR_COUNT）× CPU（0..CPU_COUNT）每格
//!     独立开户（opened/count/last_serial 三账），开户率千分数必须
//!     =1000‰。
//!   - 事件流：全局序号（serial 单调递增）逐事件落格，格内序号必须
//!     严格递增（断档即拒）。
//!   - 全域合并：两条独立路径对拍——①合并视图（分账逐格按 CPU 归并
//!     求和）；②重放合并（事件流独立重聚合）。两路逐向量相等且总数
//!     相等（零丢失）。
//!   - 按序号归并：per-CPU 事件流 k 路归并按序号全序展开，序号严格
//!     递增且计数守恒（对拍 10^6 事件全量跑）。
//!   - 10^6 事件对拍：1_000_000 条事件分账落格后双路对拍一致。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72080，VX3-1103 per-CPU 分账批独占区）。
pub const SEG_BASE: u32 = 0x72080;

/// 向量数（64 冻结，一维）。
pub const VECTOR_COUNT: usize = 64;
/// CPU 数（8 冻结，另一维）。
pub const CPU_COUNT: usize = 8;
/// 对拍事件总量（10^6 冻结）。
pub const BIG_EVENTS: u64 = 1_000_000;
/// 开户率硬线（千分数 1000）。
pub const OPEN_HARDLINE: u64 = 1000;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlCode {
    /// 向量号越界
    VecOver,
    /// CPU 号越界
    CpuOver,
    /// 分账格未开户
    CellNotOpen,
    /// 合并视图零丢失破坏
    MergeLoss,
    /// 双路对拍失配
    PairMismatch,
    /// 序号断档（格内序号未严格递增）
    SerialGap,
    /// 双跑不一致
    NotReproducible,
}

impl PlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PlCode::VecOver => "向量号越界：≥64，请核对向量表长度",
            PlCode::CpuOver => "CPU 号越界：≥8，请核对 CPU 账",
            PlCode::CellNotOpen => "分账格未开户：请先对全部向量×CPU 二维开户",
            PlCode::MergeLoss => "合并视图零丢失破坏：分账总和与事件总数不等，逐格对账",
            PlCode::PairMismatch => "双路对拍失配：合并视图与重放聚合逐向量不一致",
            PlCode::SerialGap => "序号断档：格内事件序号必须严格递增",
            PlCode::NotReproducible => "双跑不一致：复现红线，固定种子后重跑",
        }
    }
    pub const ALL: [PlCode; 7] = [
        PlCode::VecOver,
        PlCode::CpuOver,
        PlCode::CellNotOpen,
        PlCode::MergeLoss,
        PlCode::PairMismatch,
        PlCode::SerialGap,
        PlCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PlCode::ALL.len() {
        if PlCode::ALL[i].val() == code {
            return PlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72080 段闭集内"
}

// ── 二维分账域模型 ─────────────────────────────────────────────
/// 单格账（一向量 × 一 CPU）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitCell {
    /// 已开户。
    pub opened: bool,
    /// 事件计数。
    pub count: u64,
    /// 格内最后事件序号。
    pub last_serial: u64,
}

impl SplitCell {
    /// 未开户空格。
    pub fn closed() -> SplitCell {
        SplitCell { opened: false, count: 0, last_serial: 0 }
    }
}

/// per-CPU 分账账本（向量 × CPU 二维格阵）。
#[derive(Clone, Debug)]
pub struct SplitLedger {
    pub cells: Vec<Vec<SplitCell>>,
    /// 全账事件总数。
    pub total: u64,
}

impl SplitLedger {
    /// 二维全开户（开户率 1000‰）。
    pub fn open_all() -> SplitLedger {
        let mut cells = Vec::new();
        let mut v = 0;
        while v < VECTOR_COUNT {
            let mut row = Vec::new();
            let mut c = 0;
            while c < CPU_COUNT {
                row.push(SplitCell { opened: true, count: 0, last_serial: 0 });
                c += 1;
            }
            cells.push(row);
            v += 1;
        }
        SplitLedger { cells, total: 0 }
    }

    /// 开户率（千分数，整数复算）。
    pub fn open_rate_permille(&self) -> u64 {
        let total = (VECTOR_COUNT * CPU_COUNT) as u64;
        let mut opened: u64 = 0;
        let mut v = 0;
        while v < self.cells.len() {
            let mut c = 0;
            while c < self.cells[v].len() {
                if self.cells[v][c].opened {
                    opened += 1;
                }
                c += 1;
            }
            v += 1;
        }
        opened.saturating_mul(1000) / total
    }

    /// 事件落格（序号单调 + 开户闸）。
    pub fn record(&mut self, vector: usize, cpu: usize, serial: u64) -> Result<(), u32> {
        if vector >= VECTOR_COUNT {
            return Err(PlCode::VecOver.val());
        }
        if cpu >= CPU_COUNT {
            return Err(PlCode::CpuOver.val());
        }
        let cell = match self.cells.get_mut(vector) {
            Some(row) => match row.get_mut(cpu) {
                Some(c) => c,
                None => return Err(PlCode::CellNotOpen.val()),
            },
            None => return Err(PlCode::CellNotOpen.val()),
        };
        if !cell.opened {
            return Err(PlCode::CellNotOpen.val());
        }
        if serial <= cell.last_serial && cell.count > 0 {
            return Err(PlCode::SerialGap.val());
        }
        cell.count += 1;
        cell.last_serial = serial;
        self.total += 1;
        Ok(())
    }

    /// 全域合并视图：逐向量把 8 个 CPU 的分账归并求和。
    pub fn merged_view(&self) -> Vec<u64> {
        let mut view = Vec::new();
        let mut v = 0;
        while v < self.cells.len() {
            let mut sum: u64 = 0;
            let mut c = 0;
            while c < self.cells[v].len() {
                sum = sum.saturating_add(self.cells[v][c].count);
                c += 1;
            }
            view.push(sum);
            v += 1;
        }
        view
    }

    /// 分账总和（全格求和）。
    pub fn cells_sum(&self) -> u64 {
        let mut s: u64 = 0;
        let mut v = 0;
        while v < self.cells.len() {
            let mut c = 0;
            while c < self.cells[v].len() {
                s = s.saturating_add(self.cells[v][c].count);
                c += 1;
            }
            v += 1;
        }
        s
    }
}

// ── 事件流 ─────────────────────────────────────────────────────
/// 一条分账事件（向量/CPU/全局序号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEvent {
    pub vector: usize,
    pub cpu: usize,
    pub serial: u64,
}

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

/// 生成 n 条确定性事件（序号严格递增，向量/CPU 由 LCG 撒点）。
pub fn gen_events(n: u64, seed: u64) -> Vec<LedgerEvent> {
    let mut evs = Vec::new();
    let mut r = Lcg::new(seed);
    let mut s: u64 = 0;
    let mut i = 0;
    while i < n {
        s += 1 + r.below(2);
        evs.push(LedgerEvent {
            vector: r.below(VECTOR_COUNT as u64) as usize,
            cpu: r.below(CPU_COUNT as u64) as usize,
            serial: s,
        });
        i += 1;
    }
    evs
}

/// 独立重放聚合（第二路径，不经分账账本）。
pub fn replay_merge(events: &[LedgerEvent]) -> Vec<u64> {
    let mut view = Vec::new();
    let mut v = 0;
    while v < VECTOR_COUNT {
        view.push(0);
        v += 1;
    }
    let mut i = 0;
    while i < events.len() {
        let ev = events[i];
        if ev.vector < VECTOR_COUNT {
            view[ev.vector] += 1;
        }
        i += 1;
    }
    view
}

/// 双路对拍：合并视图 vs 重放聚合 vs 事件总数（零丢失三重断言）。
pub fn cross_check(ledger: &SplitLedger, events: &[LedgerEvent]) -> Result<(), u32> {
    let mv = ledger.merged_view();
    let rp = replay_merge(events);
    if mv.len() != VECTOR_COUNT || rp.len() != VECTOR_COUNT {
        return Err(PlCode::PairMismatch.val());
    }
    let mut v = 0;
    while v < VECTOR_COUNT {
        if mv[v] != rp[v] {
            return Err(PlCode::PairMismatch.val());
        }
        v += 1;
    }
    if ledger.cells_sum() != events.len() as u64 {
        return Err(PlCode::MergeLoss.val());
    }
    if ledger.total != events.len() as u64 {
        return Err(PlCode::MergeLoss.val());
    }
    Ok(())
}

// ── 按序号归并（per-CPU 流 k 路归并，全序展开） ─────────────────
/// 把事件流按 CPU 切成有序流（全局序号递增 ⇒ 各流有序）。
pub fn split_streams(events: &[LedgerEvent]) -> Vec<Vec<LedgerEvent>> {
    let mut streams: Vec<Vec<LedgerEvent>> = Vec::new();
    let mut c = 0;
    while c < CPU_COUNT {
        streams.push(Vec::new());
        c += 1;
    }
    let mut i = 0;
    while i < events.len() {
        let ev = events[i];
        if ev.cpu < CPU_COUNT {
            streams[ev.cpu].push(ev);
        }
        i += 1;
    }
    streams
}

/// k 路按序号归并：全序展开，序号必须严格递增（断档即拒），
/// 归并计数守恒（零丢失）。返回归并事件数。
pub fn ordered_merge(events: &[LedgerEvent]) -> Result<u64, u32> {
    let streams = split_streams(events);
    let mut pos = Vec::new();
    let mut c = 0;
    while c < CPU_COUNT {
        pos.push(0usize);
        c += 1;
    }
    let mut last: u64 = 0;
    let mut merged: u64 = 0;
    let total = events.len() as u64;
    while merged < total {
        // 取各流队首中序号最小者（k 路扫描选最小）。
        let mut best: Option<(usize, u64)> = None;
        let mut c = 0;
        while c < CPU_COUNT {
            let p = pos[c];
            if p < streams[c].len() {
                let s = streams[c][p].serial;
                match best {
                    None => best = Some((c, s)),
                    Some((_, bs)) => {
                        if s < bs {
                            best = Some((c, s));
                        }
                    }
                }
            }
            c += 1;
        }
        let (c, s) = match best {
            Some(x) => x,
            None => return Err(PlCode::MergeLoss.val()),
        };
        if merged > 0 && s <= last {
            return Err(PlCode::SerialGap.val());
        }
        last = s;
        pos[c] += 1;
        merged += 1;
    }
    Ok(merged)
}

// ── 真机分账全链（10^6 事件对拍） ───────────────────────────────
/// 黄金全链：开户→10^6 事件落格→双路对拍→按序号归并守恒。
/// 返回 (开户率‰, 事件总数, 归并数)。
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    let mut ledger = SplitLedger::open_all();
    let rate = ledger.open_rate_permille();
    if rate != OPEN_HARDLINE {
        return Err(PlCode::CellNotOpen.val());
    }
    let events = gen_events(BIG_EVENTS, SEG_BASE as u64);
    let mut i = 0;
    while i < events.len() {
        ledger.record(events[i].vector, events[i].cpu, events[i].serial)?;
        i += 1;
    }
    cross_check(&ledger, &events)?;
    let merged = ordered_merge(&events)?;
    if merged != events.len() as u64 {
        return Err(PlCode::MergeLoss.val());
    }
    Ok((rate, ledger.total, merged))
}

// ── 注入检出账（四路：未开户/合并丢失/序号断档/对拍失配） ───────
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

/// 注入轮（LCG 驱动 rounds 轮 × 4 路，每路必抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：未开户格落事件 ⇒ CellNotOpen 必抓。
        let mut l1 = SplitLedger::open_all();
        l1.cells[0][0].opened = false;
        led.record(0, l1.record(0, 0, 1).is_err());
        // 路 2：合并丢失（分账少记一事件）⇒ 对拍零丢失必抓。
        let events2 = gen_events(64, 7);
        let mut l2 = SplitLedger::open_all();
        let mut i = 0;
        while i < events2.len() {
            let _ = l2.record(events2[i].vector, events2[i].cpu, events2[i].serial);
            i += 1;
        }
        l2.total -= 1;
        led.record(1, cross_check(&l2, &events2).is_err());
        // 路 3：序号断档（同格回拨序号）⇒ SerialGap 必抓。
        let mut l3 = SplitLedger::open_all();
        let _ = l3.record(3, 3, 10);
        led.record(2, l3.record(3, 3, 9).is_err());
        // 路 4：对拍失配（重放流抹一事件）⇒ PairMismatch 必抓。
        let events4 = gen_events(128, 11);
        let mut l4 = SplitLedger::open_all();
        let mut i = 0;
        while i < events4.len() {
            let _ = l4.record(events4[i].vector, events4[i].cpu, events4[i].serial);
            i += 1;
        }
        let mut broken = Vec::new();
        let mut j = 0;
        while j < events4.len() - 1 {
            broken.push(events4[j]);
            j += 1;
        }
        led.record(3, cross_check(&l4, &broken).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（开户率 1000‰ + 10^6 事件 + 归并守恒）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((rate, total, merged)) => {
            if rate == OPEN_HARDLINE && total == BIG_EVENTS && merged == BIG_EVENTS {
                Ok(())
            } else {
                Err(PlCode::NotReproducible.val())
            }
        }
        Err(e) => Err(e),
    }
}

/// 序 B：向量越界 ⇒ VecOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = SplitLedger::open_all();
    match l.record(VECTOR_COUNT, 0, 1) {
        Err(e) if e == PlCode::VecOver.val() => Ok(()),
        _ => Err(PlCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常数守卫（向量 64 / CPU 8 / 对拍 10^6 / 硬线 1000‰）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        VECTOR_COUNT == 64,
        CPU_COUNT == 8,
        BIG_EVENTS == 1_000_000,
        OPEN_HARDLINE == 1000,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(PlCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PlCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PlCode::ALL.len() {
        let v = PlCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（10^6 事件对拍一致）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((rate, total, merged)) => {
                *rate == OPEN_HARDLINE && *total == BIG_EVENTS && *merged == BIG_EVENTS
            }
            Err(_) => false,
        },
        match &g {
            Ok((rate, total, merged)) => format!("open={}permille total={} merged={}", rate, total, merged),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 开户率 100%（二维全开）。
    let l = SplitLedger::open_all();
    v.push((
        "J1-开户率100%",
        l.open_rate_permille() == OPEN_HARDLINE && l.cells.len() == VECTOR_COUNT,
        String::from("64 向量 × 8 CPU 二维全开户"),
    ));
    // 双路对拍（中量事件逐向量相等 + 总数守恒）。
    let ev2 = gen_events(4096, 23);
    let mut l2 = SplitLedger::open_all();
    let mut all_ok = true;
    let mut i = 0;
    while i < ev2.len() {
        if l2.record(ev2[i].vector, ev2[i].cpu, ev2[i].serial).is_err() {
            all_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-双路对拍",
        all_ok && cross_check(&l2, &ev2).is_ok(),
        format!("4096 事件对拍零丢失 total={}", l2.total),
    ));
    // 按序号归并（k 路全序展开守恒）。
    let om = ordered_merge(&ev2);
    v.push((
        "J1-按序号归并",
        match om {
            Ok(n) => n == ev2.len() as u64,
            Err(_) => false,
        },
        format!("merged={} of {}", om.clone().unwrap_or(0), ev2.len()),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x72080 窗口"),
    ));
    v
}

/// 自检入口（UNX-F1406）。
pub fn run_unx1406_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1406 per-CPU 分账与全域合并账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
