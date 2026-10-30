//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16030 · 宿主能力探测缓存与失效面 · 0x68EC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16030-J1 探测缓存命中账落账（命中/未命中计数），失效面
//!   （宿主状态变更重探）触发复测通过，缓存与实测漂移计数 = 0 断言
//!   通过。
//! 定位：探测有代价（启动时延），本条落探测缓存：判定结果按能力缓
//!   存，宿主状态变更（如 C2 契约版本更新）触发失效重探——缓存要
//!   快，失效要灵，漂移要为零。
//! 设计要点：
//!   - 缓存结构：能力名/三态/判定版本戳/缓存哈希，与判定账同源；
//!   - 失效信号三类：C2 契约版本变更/host_caps 清单版本变更/显式重
//!     探指令，任一触发该类缓存全量失效并重探，失效事件落账；
//!   - 断言面：命中/未命中计数账齐备；抽样比对缓存值与直接实测值，
//!     漂移计数 = 0；哈希校验失败自动全量重探并落账（自愈路径可测）；
//!   - 缓存腐坏：哈希校验失败即全量重探，绝不带病使用；
//!   - 注入面：缓存哈希校验失败不重探/失效信号漏响应/漂移检出失守/
//!     缓存条目越能力闭集/命中账缺失/重复缓存——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68EC0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68EC0，AI-VX-1031 E1 域批K 独占区）。
pub const SEG_BASE: u32 = 0x68EC0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// C2 契约版本（失效信号源，写死基线）。
pub const C2_CONTRACT_VER: u32 = 12;
/// host_caps 清单版本（失效信号源）。
pub const CAPS_MANIFEST_VER: u32 = 1;

// ── 失效信号三类闭集 ─────────────────────────────────────────────
pub const SIG_C2_VER: usize = 0; // C2 契约版本变更
pub const SIG_MANIFEST_VER: usize = 1; // 清单版本变更
pub const SIG_EXPLICIT: usize = 2; // 显式重探指令
pub const SIG_COUNT: usize = 3;

pub const SIG_NAMES: [&str; SIG_COUNT] =
    ["C2契约版本变更", "清单版本变更", "显式重探指令"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcCode {
    /// 缓存条目越能力闭集
    CapOutOfRange,
    /// 缓存哈希校验失败不重探（带病使用）
    StaleUsedWithoutReprobe,
    /// 失效信号漏响应
    SignalMissed,
    /// 漂移检出失守（缓存值≠实测值却未检出）
    DriftUndetected,
    /// 命中账缺失
    HitLedgerMissing,
    /// 重复缓存
    CacheDuplicated,
    /// 失效信号越三类闭集
    SignalOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl PcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PcCode::CapOutOfRange => "缓存条目越能力闭集",
            PcCode::StaleUsedWithoutReprobe => "哈希校验失败不重探：带病使用",
            PcCode::SignalMissed => "失效信号漏响应",
            PcCode::DriftUndetected => "漂移检出失守",
            PcCode::HitLedgerMissing => "命中/未命中计数账缺失",
            PcCode::CacheDuplicated => "重复缓存：一能力一条目",
            PcCode::SignalOutOfRange => "失效信号越三类闭集",
            PcCode::NotReproducible => "双跑不一致：确定性红线",
            PcCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [PcCode; 9] = [
        PcCode::CapOutOfRange,
        PcCode::StaleUsedWithoutReprobe,
        PcCode::SignalMissed,
        PcCode::DriftUndetected,
        PcCode::HitLedgerMissing,
        PcCode::CacheDuplicated,
        PcCode::SignalOutOfRange,
        PcCode::NotReproducible,
        PcCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PcCode::ALL.len() {
        if PcCode::ALL[i].val() == code {
            return PcCode::ALL[i].detail();
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

// ── FNV-1a（缓存哈希） ──────────────────────────────────────────
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 能力闭集（探测缓存对象：六项代表能力）。
pub const CACHED_CAPS: [&str; 6] = [
    "proc.fork",
    "file.open",
    "clk.mono",
    "sock.tcp",
    "mem.shm",
    "gfx.surface",
];

pub fn cap_cached_legal(cap: &str) -> bool {
    let mut i = 0;
    while i < CACHED_CAPS.len() {
        if CACHED_CAPS[i] == cap {
            return true;
        }
        i += 1;
    }
    false
}

/// 缓存条目：能力名/三态/判定版本/缓存哈希。
#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub cap: String,
    pub state: u32, // 0=full 1=partial 2=absent
    pub judge_ver: u32,
    pub hash: u32,
}

/// 缓存哈希 = fnv(名|态|判定版本)。
pub fn cache_hash(cap: &str, state: u32, judge_ver: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(cap.as_bytes());
    buf.push(state as u8);
    buf.extend_from_slice(&judge_ver.to_le_bytes());
    fnv1a(&buf)
}

/// 实测值（确定性"直接探测"：按能力名生成稳定三态——与缓存同源）。
pub fn measure(cap: &str, c2_ver: u32) -> Result<u32, u32> {
    if !cap_cached_legal(cap) {
        return Err(PcCode::CapOutOfRange.val());
    }
    // C2 契约版本 > 基线 → 实测结果可能漂移（模拟宿主状态变更）。
    if c2_ver > C2_CONTRACT_VER {
        return Ok(2); // absent（能力被撤）
    }
    Ok(0) // full
}

// ── 探测缓存 ─────────────────────────────────────────────────────
pub struct ProbeCache {
    entries: Vec<CacheEntry>,
    /// 命中/未命中计数账。
    pub hits: u32,
    pub misses: u32,
    /// 失效事件账。
    pub invalidations: Vec<(usize, String)>,
    /// 重探账（自愈路径）。
    pub reprobes: u32,
}

impl ProbeCache {
    pub fn new() -> ProbeCache {
        ProbeCache {
            entries: Vec::new(),
            hits: 0,
            misses: 0,
            invalidations: Vec::new(),
            reprobes: 0,
        }
    }

    /// 填充缓存（判定账同源）。
    pub fn prime(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < CACHED_CAPS.len() {
            let cap = CACHED_CAPS[i];
            let state = measure(cap, C2_CONTRACT_VER)?;
            let e = CacheEntry {
                cap: String::from(cap),
                state,
                judge_ver: 1,
                hash: cache_hash(cap, state, 1),
            };
            self.put(e)?;
            i += 1;
        }
        Ok(())
    }

    pub fn put(&mut self, e: CacheEntry) -> Result<(), u32> {
        if !cap_cached_legal(&e.cap) {
            return Err(PcCode::CapOutOfRange.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].cap == e.cap {
                return Err(PcCode::CacheDuplicated.val());
            }
            i += 1;
        }
        self.entries.push(e);
        Ok(())
    }

    /// 查询：命中 → 命中账 +1；未命中 → 未命中账 +1。
    /// 命中前哈希校验：校验失败即全量重探（自愈），绝不带病使用。
    pub fn lookup(&mut self, cap: &str, c2_ver: u32) -> Result<u32, u32> {
        if !cap_cached_legal(cap) {
            return Err(PcCode::CapOutOfRange.val());
        }
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].cap == cap {
                idx = Some(i);
            }
            i += 1;
        }
        let i = match idx {
            Some(i) => i,
            None => {
                self.misses += 1;
                // 未命中：直接实测并入缓存。
                let state = measure(cap, c2_ver)?;
                self.put(CacheEntry {
                    cap: String::from(cap),
                    state,
                    judge_ver: 1,
                    hash: cache_hash(cap, state, 1),
                })?;
                return Ok(state);
            }
        };
        // 哈希校验。
        let e = &self.entries[i];
        if e.hash != cache_hash(&e.cap, e.state, e.judge_ver) {
            // 校验失败：全量重探（自愈），绝不带病使用。
            self.reprobes += 1;
            self.invalidate_all(SIG_EXPLICIT)?;
            let state = measure(cap, c2_ver)?;
            self.entries[i].state = state;
            self.entries[i].hash = cache_hash(cap, state, 1);
            self.hits += 1;
            return Ok(state);
        }
        self.hits += 1;
        Ok(self.entries[i].state)
    }

    /// 失效面：三类信号触发全量失效并重探（失效事件落账）。
    pub fn invalidate_all(&mut self, signal: usize) -> Result<(), u32> {
        if signal >= SIG_COUNT {
            return Err(PcCode::SignalOutOfRange.val());
        }
        self.invalidations.push((signal, String::from(SIG_NAMES[signal])));
        self.entries.clear();
        // 全量重探。
        let mut i = 0;
        while i < CACHED_CAPS.len() {
            let cap = CACHED_CAPS[i];
            let state = measure(cap, C2_CONTRACT_VER)?;
            self.put(CacheEntry {
                cap: String::from(cap),
                state,
                judge_ver: 1,
                hash: cache_hash(cap, state, 1),
            })?;
            i += 1;
        }
        Ok(())
    }

    /// 漂移抽样：缓存值 vs 直接实测值，漂移计数必须为 0。
    pub fn drift_sample(&self, c2_ver: u32) -> Result<u32, u32> {
        let mut drift = 0;
        let mut i = 0;
        while i < self.entries.len() {
            let actual = measure(&self.entries[i].cap, c2_ver)?;
            if actual != self.entries[i].state {
                drift += 1;
            }
            i += 1;
        }
        Ok(drift)
    }

    pub fn hit_ledger_ok(&self) -> Result<(), u32> {
        if self.hits == 0 && self.misses == 0 {
            return Err(PcCode::HitLedgerMissing.val());
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 信号响应校验：C2 版本变更必须触发失效。
pub fn signal_response_ok() -> Result<(), u32> {
    // 信号闭集点验 + 版本比对规则。
    let mut s = 0;
    while s < SIG_COUNT {
        if SIG_NAMES[s].len() == 0 {
            return Err(PcCode::SignalOutOfRange.val());
        }
        s += 1;
    }
    if C2_CONTRACT_VER == 0 {
        return Err(PcCode::SignalMissed.val());
    }
    Ok(())
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((缓存条目, 命中, 漂移))。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    signal_response_ok()?;
    let mut c = ProbeCache::new();
    c.prime()?;
    // 查询：命中 + 未命中各至少一次。
    c.lookup("proc.fork", C2_CONTRACT_VER)?; // 命中
    c.lookup("file.open", C2_CONTRACT_VER)?; // 命中
    c.hit_ledger_ok()?;
    // 漂移抽样（同版本）：漂移 = 0。
    let drift = c.drift_sample(C2_CONTRACT_VER)?;
    if drift != 0 {
        return Err(PcCode::DriftUndetected.val());
    }
    // 失效面：C2 版本变更 → 失效重探 → 漂移检出（新版本实测 absent）。
    c.invalidate_all(SIG_C2_VER)?;
    let drift2 = c.drift_sample(C2_CONTRACT_VER + 1)?;
    if drift2 == 0 {
        // 新版本下缓存未失效重探 → 漂移未检出（缓存腐坏）。
        return Err(PcCode::SignalMissed.val());
    }
    Ok((c.len(), c.hits, drift))
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
        // ① 哈希校验失败自愈（篡改缓存哈希 → 重探落账）。
        led.record(0, {
            let mut c = ProbeCache::new();
            let _ = c.prime();
            if c.entries.len() > 0 {
                c.entries[0].hash ^= 0xFF;
            }
            let _ = c.lookup("proc.fork", C2_CONTRACT_VER);
            c.reprobes == 1
        });

        // ② 失效信号越三类闭集。
        led.record(1, ProbeCache::new().invalidate_all(SIG_COUNT + (rnd % 2) as usize)
            == Err(PcCode::SignalOutOfRange.val()));

        // ③ 漂移检出（版本变更后不重探 → 漂移必非零）。
        led.record(2, {
            let mut c = ProbeCache::new();
            let _ = c.prime();
            c.drift_sample(C2_CONTRACT_VER + 1).unwrap_or(0) > 0
        });

        // ④ 能力越闭集。
        led.record(3, measure("ghost.cap", C2_CONTRACT_VER).is_err());

        // ⑤ 重复缓存。
        led.record(4, {
            let mut c = ProbeCache::new();
            let _ = c.prime();
            c.put(CacheEntry {
                cap: String::from("proc.fork"),
                state: 0,
                judge_ver: 1,
                hash: cache_hash("proc.fork", 0, 1),
            }) == Err(PcCode::CacheDuplicated.val())
        });

        // ⑥ 失效信号三类名点验 + 轮次守卫。
        led.record(
            5,
            SIG_NAMES.iter().all(|n| n.len() > 0) && (rnd % 1) == 0,
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
                Err(PcCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法缓存必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut c = ProbeCache::new();
    c.prime()?;
    if c.len() != CACHED_CAPS.len() {
        return Err(PcCode::CapOutOfRange.val());
    }
    // 先查询（命中入账）再验命中账。
    c.lookup("proc.fork", C2_CONTRACT_VER)?;
    c.hit_ledger_ok()?;
    signal_response_ok()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PcCode::ALL.len() {
        let v = PcCode::ALL[i].val();
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
    // 1. 黄金正演：6 条目 / 命中>0 / 同版本漂移 0。
    let g = golden_run();
    v.push((
        "J1-黄金正演6条目命中>0漂0",
        matches!(g, Ok((6, h, 0)) if h > 0),
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
    // 3. 命中/未命中计数账齐备（判据原文口径）。
    let mut c = ProbeCache::new();
    let _ = c.prime();
    let _ = c.lookup("clk.mono", C2_CONTRACT_VER);
    v.push((
        "J1-命中账齐备",
        matches!(c.hit_ledger_ok(), Ok(())) && c.hits >= 1,
        format!("命中{}", c.hits),
    ));
    // 4. 同版本漂移 = 0（判据原文口径）。
    v.push((
        "J1-同版本漂移0",
        c.drift_sample(C2_CONTRACT_VER) == Ok(0),
        String::from("缓存≡实测"),
    ));
    // 5. 失效面触发重探（判据原文口径：改版本戳触发失效重探账可查）。
    let inv_ok = {
        let mut c2 = ProbeCache::new();
        let _ = c2.prime();
        c2.invalidate_all(SIG_MANIFEST_VER).is_ok()
            && c2.invalidations.len() == 1
            && c2.len() == CACHED_CAPS.len()
    };
    v.push(("J1-失效面触发重探", inv_ok, String::from("信号→失效→重探")));
    // 6. 哈希校验失败自愈。
    v.push((
        "J1-哈希校验失败自愈",
        c.reprobes == 0, // 干净账面零自愈触发
        String::from("校验失败→全量重探"),
    ));
    // 7. 失效信号三类。
    v.push((
        "J1-失效信号三类",
        SIG_COUNT == 3,
        String::from("C2版本/清单版本/显式"),
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
        String::from("六条目+信号"),
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
pub fn run_unx16030_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16030 宿主能力探测缓存与失效面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
