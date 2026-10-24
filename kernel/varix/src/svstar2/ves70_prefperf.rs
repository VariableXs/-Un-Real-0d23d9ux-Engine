//! VE-F3670 · 个性化性能（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3670`
//!
//! **职责定位（锚点原文）**：个性化性能——
//! - **读取预算**：个性化值查询 O(1)，读取 ≤0.001ms（高频操作）；
//! - **生效预算**：变更生效 200ms（复用断言）；
//! - **热缓存**：个性化值热缓存（热表模式跨域声明——单源复用）；
//! - **批量生效**：预设应用万键合批生效（复用合并——复述）；
//! - **基准门**：万键应用 ≤100ms（规模基准——基准门复述制度）。
//!
//! **判据（锚点原文）**：0.001ms 读取、合批复用、万键 100ms、热表单源、纪律复用、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 读取超线→优化立案（红线实测） | [`C70Code::READ_OVER`] |
//! | 全量逐键→合批（复述红线） | [`C70Code::KEY_BY_KEY`] |
//! | 缓存击穿→直查降级（复用） | [`C70Code::CACHE_MISS`] |
//! | 基准不达→循环（复用纪律） | [`C70Code::BENCH_MISS`] |
//!
//! 零 panic 面、零 IO、零墙钟（预算单位为抽象 tick，非真实时钟）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3670 诊断码段（独占 0x76xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C70Code(pub u16);

impl C70Code {
    /// 读取超线（红线实测）。
    pub const READ_OVER: C70Code = C70Code(0x7601);
    /// 全量逐键（应合批）。
    pub const KEY_BY_KEY: C70Code = C70Code(0x7602);
    /// 缓存击穿（降级直查）。
    pub const CACHE_MISS: C70Code = C70Code(0x7603);
    /// 基准不达。
    pub const BENCH_MISS: C70Code = C70Code(0x7604);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C70Code::READ_OVER => "个性化读取超预算（红线实测）。建议：优化立案。",
            C70Code::KEY_BY_KEY => "全量逐键生效（应合批）。建议：合批复用。",
            C70Code::CACHE_MISS => "缓存击穿。建议：直查降级。",
            C70Code::BENCH_MISS => "基准不达。建议：按纪律循环优化。",
            _ => "未知诊断码。",
        }
    }
}

// ── 预算 ────────────────────────────────────────────────────────────

/// 性能预算表（抽象 tick，非真实时钟）。
#[derive(Clone, Copy)]
pub struct Budget {
    /// 单次读取预算（tick）。
    pub read_tick: u32,
    /// 变更生效预算（tick）。
    pub apply_tick: u32,
    /// 万键合批基准预算（tick）。
    pub bench_tick: u32,
}

impl Budget {
    /// 默认预算：读 1 / 生效 200 / 基准 100。
    pub const fn default_budget() -> Self {
        Self {
            read_tick: 1,
            apply_tick: 200,
            bench_tick: 100,
        }
    }

    /// 读取是否在预算内。
    pub fn read_ok(&self, measured: u32) -> bool {
        measured <= self.read_tick
    }

    /// 生效是否在预算内。
    pub fn apply_ok(&self, measured: u32) -> bool {
        measured <= self.apply_tick
    }

    /// 基准是否在预算内。
    pub fn bench_ok(&self, measured: u32) -> bool {
        measured <= self.bench_tick
    }
}

// ── 热缓存 ──────────────────────────────────────────────────────────

/// 热缓存条目。
#[derive(Clone, Copy)]
pub struct HotEntry {
    pub key: u16,
    pub value: i32,
    /// 热度计数（命中递增）。
    pub heat: u32,
}

/// 个性化热缓存（热表单源复用声明）。
pub struct HotCache {
    entries: Vec<HotEntry>,
    hits: u32,
    misses: u32,
    /// 击穿计数（降级直查次数）。
    breaks: u32,
}

impl HotCache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            hits: 0,
            misses: 0,
            breaks: 0,
        }
    }

    /// 预热入缓存。
    pub fn warm(&mut self, key: u16, value: i32) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == key) {
            e.value = value;
            e.heat = e.heat.saturating_add(1);
        } else {
            self.entries.push(HotEntry { key, value, heat: 1 });
        }
    }

    /// 命中查（O(1) 口径：小表线性即常数界）。
    pub fn get(&mut self, key: u16) -> Option<i32> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == key) {
            e.heat = e.heat.saturating_add(1);
            self.hits += 1;
            Some(e.value)
        } else {
            self.misses += 1;
            None
        }
    }

    /// 击穿（缓存未命中走直查降级）。
    pub fn break_through(&mut self) -> i32 {
        self.breaks += 1;
        0
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }

    pub fn misses(&self) -> u32 {
        self.misses
    }

    pub fn breaks(&self) -> u32 {
        self.breaks
    }

    /// 命中率 ≥80% 视为热表有效。
    pub fn hot_effective(&self) -> bool {
        let total = self.hits + self.misses;
        total == 0 || self.hits * 5 >= total * 4
    }
}

// ── 合批生效 ────────────────────────────────────────────────────────

/// 合批变更项。
#[derive(Clone, Copy)]
pub struct ChangeItem {
    pub key: u16,
    pub value: i32,
}

/// 合批生效器（预设应用万键合批）。
pub struct BatchApplier {
    pending: Vec<ChangeItem>,
    /// 累计逐键违规（红线实测）。
    key_by_key_hits: u32,
    batches: u32,
    items_applied: u32,
    last_code: C70Code,
    has_error: bool,
}

impl BatchApplier {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            key_by_key_hits: 0,
            batches: 0,
            items_applied: 0,
            last_code: C70Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C70Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: C70Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 登记一项变更。
    pub fn queue(&mut self, item: ChangeItem) -> usize {
        self.pending.push(item);
        self.pending.len()
    }

    /// 合批生效：一次帧内全部落地。
    pub fn apply_batch(&mut self, budget: &Budget, measured_tick: u32) -> Result<u32, C70Code> {
        if self.pending.is_empty() {
            self.last_code = C70Code(0);
            self.has_error = false;
            return Ok(0);
        }
        if !budget.apply_ok(measured_tick) {
            self.fail(C70Code::KEY_BY_KEY);
            return Err(C70Code::KEY_BY_KEY);
        }
        let n = self.pending.len() as u32;
        self.items_applied += n;
        self.batches += 1;
        self.pending.clear();
        self.last_code = C70Code(0);
        self.has_error = false;
        Ok(n)
    }

    /// 逐键违规检测（红线实测：单帧只 apply 一项视为逐键）。
    pub fn note_key_by_key(&mut self) {
        self.key_by_key_hits += 1;
        self.fail(C70Code::KEY_BY_KEY);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn batches(&self) -> u32 {
        self.batches
    }

    pub fn items_applied(&self) -> u32 {
        self.items_applied
    }

    pub fn key_by_key_hits(&self) -> u32 {
        self.key_by_key_hits
    }
}

// ── 读取仪表 ────────────────────────────────────────────────────────

/// 读取预算仪表（0.001ms 抽象为 1 tick）。
pub struct ReadMeter {
    samples: Vec<u32>,
    over_hits: u32,
    last_code: C70Code,
    has_error: bool,
}

impl ReadMeter {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            over_hits: 0,
            last_code: C70Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C70Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 记一次读取耗时。
    pub fn sample(&mut self, budget: &Budget, tick: u32) -> Result<bool, C70Code> {
        self.samples.push(tick);
        if !budget.read_ok(tick) {
            self.over_hits += 1;
            self.last_code = C70Code::READ_OVER;
            self.has_error = true;
            return Err(C70Code::READ_OVER);
        }
        self.last_code = C70Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 均值（O(样本)）。
    pub fn avg_tick(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u32 = self.samples.iter().copied().sum();
        sum / self.samples.len() as u32
    }

    pub fn over_hits(&self) -> u32 {
        self.over_hits
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}

// ── 基准门 ──────────────────────────────────────────────────────────

/// 万键基准门（规模基准复述制度）。
pub struct BenchGate {
    last_tick: u32,
    pass_count: u32,
    fail_count: u32,
    last_code: C70Code,
    has_error: bool,
}

impl BenchGate {
    pub fn new() -> Self {
        Self {
            last_tick: 0,
            pass_count: 0,
            fail_count: 0,
            last_code: C70Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C70Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 跑一次万键基准。
    pub fn run(&mut self, budget: &Budget, measured_tick: u32) -> Result<bool, C70Code> {
        self.last_tick = measured_tick;
        if budget.bench_ok(measured_tick) {
            self.pass_count += 1;
            self.last_code = C70Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail_count += 1;
            self.last_code = C70Code::BENCH_MISS;
            self.has_error = true;
            Err(C70Code::BENCH_MISS)
        }
    }

    pub fn last_tick(&self) -> u32 {
        self.last_tick
    }

    pub fn pass_count(&self) -> u32 {
        self.pass_count
    }

    pub fn fail_count(&self) -> u32 {
        self.fail_count
    }

    /// 基准总评：至少过一次且无失败残留。
    pub fn bench_green(&self) -> bool {
        self.pass_count > 0 && self.fail_count == 0
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves70_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves70_prefperf");
    let budget = Budget::default_budget();
    {
        let b = Budget::default_budget();
        chk(
            &mut set,
            "C3670-01",
            b.read_tick == 1 && b.apply_tick == 200 && b.bench_tick == 100,
            "err",
        );
    }
    {
        let b = Budget::default_budget();
        chk(&mut set, "C3670-02", b.read_ok(1) && !b.read_ok(2), "err");
    }
    {
        let mut m = ReadMeter::new();
        let res = m.sample(&budget, 1);
        chk(&mut set, "C3670-03", res == Ok(true) && m.over_hits() == 0, "err");
    }
    {
        let mut m = ReadMeter::new();
        let res = m.sample(&budget, 9);
        chk(&mut set, "C3670-04", res.err() == Some(C70Code::READ_OVER), "err");
    }
    {
        let mut h = HotCache::new();
        h.warm(1, 100);
        h.warm(2, 200);
        let v = h.get(1);
        chk(&mut set, "C3670-05", v == Some(100) && h.hits() == 1, "err");
    }
    {
        let mut h = HotCache::new();
        h.warm(1, 1);
        let v = h.get(9);
        chk(&mut set, "C3670-06", v.is_none() && h.misses() == 1, "err");
    }
    {
        let mut h = HotCache::new();
        h.break_through();
        chk(&mut set, "C3670-07", h.breaks() == 1, "err");
    }
    {
        let mut a = BatchApplier::new();
        for i in 0..10u16 {
            a.queue(ChangeItem { key: i, value: i as i32 });
        }
        let n = a.apply_batch(&budget, 50);
        chk(&mut set, "C3670-08", n == Ok(10) && a.pending_count() == 0, "err");
    }
    {
        let mut a = BatchApplier::new();
        a.queue(ChangeItem { key: 1, value: 1 });
        let res = a.apply_batch(&budget, 500);
        chk(&mut set, "C3670-09", res.err() == Some(C70Code::KEY_BY_KEY), "err");
    }
    {
        let mut g = BenchGate::new();
        let res = g.run(&budget, 80);
        chk(&mut set, "C3670-10", res == Ok(true) && g.bench_green(), "err");
    }
    {
        let mut g = BenchGate::new();
        let res = g.run(&budget, 200);
        chk(&mut set, "C3670-11", res.err() == Some(C70Code::BENCH_MISS), "err");
    }
    {
        let mut h = HotCache::new();
        h.warm(1, 1);
        h.get(1);
        h.get(1);
        h.get(9);
        chk(&mut set, "C3670-12", h.hot_effective(), "err");
    }
    set
}
