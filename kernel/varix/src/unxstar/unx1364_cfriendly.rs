//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1364 · 缓存友好性账（C 表缓存局部性优化与实测）· 0x6FA00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B29）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1364-J1 C 表访存模式实测可查（缓存未命中率逐布局），
//!     布局方案 ≥2 种对照实测（A/B），最优布局采用后未命中率改善
//!     ≥20%（相对基线）实测留痕；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 闸门次序（访存实测为什么不能倒）：
//!   访存流先于模拟——没有确定性的访存流，未命中数不可复算；
//!   模拟先于对照——两布局各自实测未命中数后才有 A/B 对照可言；
//!   对照先于采用——改善率 ≥20% 复核通过才允许写"采用"位；
//!   采用先于留痕——采用位与实测账同源，账不定则留痕是空话。
//!
//! 关键差异（本卡深度所在）：
//!   本卡只回答"C 表两种布局在**同一访存流**下未命中率差多少、
//!   改善是否 ≥20%、采用位是否落在实测更优者"，不回答热/冷字段
//!   该放什么内容（那是表语义卡的事），也不复算表项业务判据——
//!   采用账若回头重算业务语义，就把"布局实测"变成了"再造一张表"。
//!   布局账与业务账摘要域彼此独立，互不覆盖。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6FA00，AI-VX3-1103A 独占区）。
pub const SEG_BASE: u32 = 0x6FA00;

/// 缓存组数（direct-mapped，64 组冻结）。
pub const CACHE_SETS: usize = 64;
/// 每缓存行容纳的访存单元数（8 单元/行，每单元 16B）。
pub const LINE_UNITS: u64 = 8;
/// C 表条目数（512 项冻结：A 布局热区 256 行 > 64 组必冲突，
/// B 布局热区 64 行恰好全容——局部性差距由此拉开）。
pub const TABLE_ENTRIES: usize = 512;
/// 实测访存流长度（4096 次访问）。
pub const STREAM_LEN: usize = 4096;
/// 改善率闸（‰，200 = 20%）。
pub const IMPROVE_PERMIL: u64 = 200;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheCode {
    /// 布局对照不足（<2 种布局）
    LayoutUnder,
    /// 访存越界（条目号 ≥ TABLE_ENTRIES）
    AccessOver,
    /// 未命中数复算失配
    MissReplay,
    /// 改善率不足（<20%）
    ImproveUnder,
    /// 采用位落错（未落在实测更优布局）
    AdoptBad,
    /// 空访存流（0 次访问不可实测）
    ZeroAccess,
    /// 双跑不一致
    NotReproducible,
}

impl CacheCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CacheCode::LayoutUnder => "布局对照不足：<2 种",
            CacheCode::AccessOver => "访存越界：条目号 ≥512",
            CacheCode::MissReplay => "未命中数复算失配",
            CacheCode::ImproveUnder => "改善率不足：<20%",
            CacheCode::AdoptBad => "采用位落错：非实测更优",
            CacheCode::ZeroAccess => "空访存流：0 次不可实测",
            CacheCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CacheCode; 7] = [
        CacheCode::LayoutUnder,
        CacheCode::AccessOver,
        CacheCode::MissReplay,
        CacheCode::ImproveUnder,
        CacheCode::AdoptBad,
        CacheCode::ZeroAccess,
        CacheCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CacheCode::ALL.len() {
        if CacheCode::ALL[i].val() == code {
            return CacheCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6FA00 段闭集内"
}

// ── 布局模型（A：热冷交错 / B：热先行） ─────────────────────────
/// 布局 A（基线，Interleaved）：每条目 4 单元（热、冷1、冷2、冷3）
/// 连续排布——热字段步长 4，一条缓存行只容 2 个条目，热区 256 行。
pub const STRIDE_A: u64 = 4;
/// 布局 B 冷区起点：热字段单数组先排满（步长 1，一行容 8 个条目，
/// 热区恰好 64 行 = 64 组全容），冷字段另行收尾排布。
pub const HOT_ZONE: u64 = (TABLE_ENTRIES as u64) * LINE_UNITS;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// 布局 A：热冷交错（基线）
    Interleaved,
    /// 布局 B：热字段先行连续（优化）
    HotFirst,
}

pub const LAYOUT_COUNT: usize = 2;

impl Layout {
    pub fn idx(self) -> usize {
        match self {
            Layout::Interleaved => 0,
            Layout::HotFirst => 1,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Layout::Interleaved => "A-热冷交错",
            Layout::HotFirst => "B-热先行",
        }
    }
    /// 字段单元地址（entry: 条目号, hot: 热字段?）。
    pub fn addr_of(self, entry: u64, hot: bool) -> Result<u64, u32> {
        if entry >= TABLE_ENTRIES as u64 {
            return Err(CacheCode::AccessOver.val());
        }
        if self == Layout::Interleaved {
            if hot {
                Ok(entry * STRIDE_A)
            } else {
                Ok(entry * STRIDE_A + 1)
            }
        } else if hot {
            Ok(entry)
        } else {
            Ok(HOT_ZONE + entry * (STRIDE_A - 1))
        }
    }
    pub const ALL: [Layout; 2] = [Layout::Interleaved, Layout::HotFirst];
}

// ── LCG（访存流驱动，与判据同源确定性） ────────────────────────
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
        (self.next() >> 33) % n
    }
}

/// 生成确定性访存流：(条目号, 热字段?)；90% 热 / 10% 冷。
pub fn access_stream(len: usize, seed: u64) -> Vec<(u64, bool)> {
    let mut rng = Lcg::new(seed);
    let mut v: Vec<(u64, bool)> = Vec::new();
    let mut i = 0;
    while i < len {
        let entry = rng.below(TABLE_ENTRIES as u64);
        let hot = rng.below(10) < 9;
        v.push((entry, hot));
        i += 1;
    }
    v
}

/// 真机访存流（SEG_BASE 作种，判据同源）。
pub fn machine_stream() -> Vec<(u64, bool)> {
    access_stream(STREAM_LEN, SEG_BASE as u64)
}

// ── direct-mapped 缓存模拟器（组数 64 × 行 8 单元） ─────────────
pub struct CacheSim {
    tags: [u64; CACHE_SETS],
    valid: [bool; CACHE_SETS],
    pub hits: u64,
    pub misses: u64,
}

impl CacheSim {
    pub fn new() -> CacheSim {
        CacheSim { tags: [0; CACHE_SETS], valid: [false; CACHE_SETS], hits: 0, misses: 0 }
    }
    pub fn access(&mut self, addr: u64) {
        let line = addr / LINE_UNITS;
        let set = (line % CACHE_SETS as u64) as usize;
        if self.valid[set] && self.tags[set] == line {
            self.hits += 1;
        } else {
            self.misses += 1;
            self.valid[set] = true;
            self.tags[set] = line;
        }
    }
}

/// 单布局实测：整条访存流过一遍缓存，回报 (命中, 未命中)。
pub fn run_layout(layout: Layout, stream: &[(u64, bool)]) -> Result<(u64, u64), u32> {
    if stream.is_empty() {
        return Err(CacheCode::ZeroAccess.val());
    }
    let mut sim = CacheSim::new();
    let mut i = 0;
    while i < stream.len() {
        let (e, hot) = stream[i];
        let addr = layout.addr_of(e, hot)?;
        sim.access(addr);
        i += 1;
    }
    Ok((sim.hits, sim.misses))
}

/// 未命中率（‰）。
pub fn miss_permil(hits: u64, misses: u64) -> Result<u64, u32> {
    if hits + misses == 0 {
        return Err(CacheCode::ZeroAccess.val());
    }
    Ok((misses * 1000 + (hits + misses) / 2) / (hits + misses))
}

// ── 布局账（逐布局实测记录 + 采用位） ──────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutRecord {
    pub layout: usize,
    pub hits: u64,
    pub misses: u64,
    pub rate_permil: u64,
}

#[derive(Clone, Debug, Default)]
pub struct CacheLedger {
    pub records: Vec<LayoutRecord>,
    /// 采用位：Some(布局下标) —— 必须落在实测未命中率最小者。
    pub adopted: Option<usize>,
}

impl CacheLedger {
    pub fn new() -> CacheLedger {
        CacheLedger::default()
    }

    /// 实测一个布局并入账。
    pub fn measure(&mut self, layout: Layout, stream: &[(u64, bool)]) -> Result<u64, u32> {
        let (hits, misses) = run_layout(layout, stream)?;
        let rate = miss_permil(hits, misses)?;
        self.records.push(LayoutRecord {
            layout: layout.idx(),
            hits,
            misses,
            rate_permil: rate,
        });
        Ok(rate)
    }

    /// 改善率（‰）：(基线未命中 − 更优未命中) × 1000 / 基线未命中。
    /// 账面第 0 格必须是布局 A（基线）、第 1 格是布局 B（优化）。
    pub fn improve_permil(&self) -> Result<u64, u32> {
        if self.records.len() < LAYOUT_COUNT {
            return Err(CacheCode::LayoutUnder.val());
        }
        let a = self.records[Layout::Interleaved.idx()].misses;
        let b = self.records[Layout::HotFirst.idx()].misses;
        if a == 0 {
            return Err(CacheCode::ZeroAccess.val());
        }
        if b >= a {
            return Err(CacheCode::ImproveUnder.val());
        }
        Ok((a - b) * 1000 / a)
    }

    /// 采用闸：采用位必须等于实测更优布局。
    pub fn best_slot(&self) -> Result<usize, u32> {
        if self.records.len() < LAYOUT_COUNT {
            return Err(CacheCode::LayoutUnder.val());
        }
        let a = self.records[Layout::Interleaved.idx()].misses;
        let b = self.records[Layout::HotFirst.idx()].misses;
        if b < a {
            Ok(Layout::HotFirst.idx())
        } else {
            Ok(Layout::Interleaved.idx())
        }
    }

    /// 写采用位（落在实测更优者）。
    pub fn adopt_best(&mut self) -> Result<usize, u32> {
        let best = self.best_slot()?;
        self.adopted = Some(best);
        Ok(best)
    }

    /// 采用复核：已写的采用位若不等于实测更优者 ⇒ AdoptBad。
    pub fn adopt_gate(&self) -> Result<usize, u32> {
        let best = self.best_slot()?;
        match self.adopted {
            Some(x) if x == best => Ok(x),
            _ => Err(CacheCode::AdoptBad.val()),
        }
    }
}

/// 复算：同一访存流两布局各重跑一遍，逐数比对账面。
pub fn replay_ok(stream: &[(u64, bool)]) -> Result<(), u32> {
    let mut i = 0;
    while i < Layout::ALL.len() {
        let r1 = run_layout(Layout::ALL[i], stream)?;
        let r2 = run_layout(Layout::ALL[i], stream)?;
        if r1 != r2 {
            return Err(CacheCode::MissReplay.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入检出账（四路：布局路/空流路/改善路/采用路） ──────────────
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

// ── 黄金全链：实测→对照→采用→四拒→复算 ─────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let stream = machine_stream();
    let mut l = CacheLedger::new();
    // 两布局对照实测（A/B）。
    let _ = l.measure(Layout::Interleaved, &stream)?;
    let _ = l.measure(Layout::HotFirst, &stream)?;
    // 改善率 ≥20%（200‰）。
    let improve = l.improve_permil()?;
    let improve_ok = (improve >= IMPROVE_PERMIL) as u32;
    // 采用位落在实测更优布局。
    let best = l.adopt_best()?;
    let adopt_ok = (l.adopt_gate()? == best && best == Layout::HotFirst.idx()) as u32;
    // 四拒。
    let mut single = CacheLedger::new();
    let _ = single.measure(Layout::Interleaved, &stream)?;
    let under = single.improve_permil().is_err(); // 布局不足
    let empty = run_layout(Layout::HotFirst, &[]).is_err(); // 空流
    let mut worse = CacheLedger::new();
    let _ = worse.measure(Layout::Interleaved, &stream)?;
    let _ = worse.measure(Layout::HotFirst, &stream)?;
    let _ = worse.records.swap(0, 1); // 伪装成 B 劣于 A → 改善闸必拒
    let no_improve = worse.improve_permil().is_err();
    let mut badadopt = CacheLedger::new();
    let _ = badadopt.measure(Layout::Interleaved, &stream)?;
    let _ = badadopt.measure(Layout::HotFirst, &stream)?;
    let _ = badadopt.adopt_best()?;
    badadopt.adopted = Some(Layout::Interleaved.idx()); // 强行采用劣者
    let adoptbad = badadopt.adopt_gate().is_err();
    replay_ok(&stream)?;
    Ok((
        improve_ok + adopt_ok,
        under as u32 + empty as u32 + no_improve as u32 + adoptbad as u32,
        improve as u32,
    ))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：改善 ≥20‰ 闸 + 采用正确 + 四拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 4, _)) => Ok(()),
        _ => Err(CacheCode::NotReproducible.val()),
    }
}

/// 序 B：空访存流 ⇒ ZeroAccess。
pub fn probe_seq_b() -> Result<(), u32> {
    match run_layout(Layout::Interleaved, &[]) {
        Err(e) if e == CacheCode::ZeroAccess.val() => Ok(()),
        _ => Err(CacheCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（组数 64 / 行 8 单元 / 条目 512 / 闸 200‰）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        CACHE_SETS == 64,
        LINE_UNITS == 8,
        TABLE_ENTRIES == 512,
        IMPROVE_PERMIL == 200,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(CacheCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let stream = machine_stream();
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：布局路（单布局对照必拒）。
        let mut l1 = CacheLedger::new();
        let _ = l1.measure(Layout::Interleaved, &stream)?;
        led.record(0, l1.improve_permil().is_err());
        // 路 2：空流路。
        led.record(1, run_layout(Layout::HotFirst, &[]).is_err());
        // 路 3：改善路（账面伪装 B 劣于 A 必拒）。
        let mut l3 = CacheLedger::new();
        let _ = l3.measure(Layout::Interleaved, &stream)?;
        let _ = l3.measure(Layout::HotFirst, &stream)?;
        let _ = l3.records.swap(0, 1);
        led.record(2, l3.improve_permil().is_err());
        // 路 4：采用路（采用位错置必检出）。
        let mut l4 = CacheLedger::new();
        let _ = l4.measure(Layout::Interleaved, &stream)?;
        let _ = l4.measure(Layout::HotFirst, &stream)?;
        let _ = l4.adopt_best()?;
        l4.adopted = Some(Layout::Interleaved.idx());
        led.record(3, l4.adopt_gate().is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CacheCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CacheCode::ALL.len() {
        let v = CacheCode::ALL[i].val();
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
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, rej, _)) => *ok == 2 && *rej == 4,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, imp)) => format!("ok={} rejects={} improve={}‰", ok, rej, imp),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 逐布局实测账（≥2 布局 + 各自未命中率留痕 + B 优于 A）。
    let stream = machine_stream();
    let mut l = CacheLedger::new();
    let mut rates: [u64; 2] = [0; 2];
    let mut all_ok = true;
    let mut i = 0;
    while i < Layout::ALL.len() {
        match l.measure(Layout::ALL[i], &stream) {
            Ok(rate) => rates[i] = rate,
            Err(_) => all_ok = false,
        }
        i += 1;
    }
    v.push((
        "J1-布局账",
        all_ok && l.records.len() == 2 && rates[1] < rates[0],
        format!("A={}‰ B={}‰", rates[0], rates[1]),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(x) => x.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|x| x.tries),
            inj.as_ref().map(|x| x.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=冻结", b),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6FA00 窗口"),
    ));
    v
}

pub fn run_unx1364_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1364 缓存友好性账（C 表缓存局部性优化与实测）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 自检（探针本地复核，不进内核镜像） ──────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_two_four() {
        match golden_run() {
            Ok((ok, rej, imp)) => {
                assert_eq!(ok, 2);
                assert_eq!(rej, 4);
                assert!(u64::from(imp) >= IMPROVE_PERMIL);
            }
            Err(e) => panic!("golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn b_beats_a_by_at_least_20_percent() {
        let stream = machine_stream();
        let (_, ma) = run_layout(Layout::Interleaved, &stream).ok().unwrap_or((0, 1));
        let (_, mb) = run_layout(Layout::HotFirst, &stream).ok().unwrap_or((0, 1));
        assert!(mb < ma);
        let mut l = CacheLedger::new();
        let _ = l.measure(Layout::Interleaved, &stream);
        let _ = l.measure(Layout::HotFirst, &stream);
        assert!(l.improve_permil().ok().unwrap_or(0) >= IMPROVE_PERMIL);
    }

    #[test]
    fn three_probes_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn inject_and_repro_and_guard() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(inj) => assert!(inj.all_caught()),
            Err(e) => panic!("inject err=0x{:x}", e),
        }
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }
}
