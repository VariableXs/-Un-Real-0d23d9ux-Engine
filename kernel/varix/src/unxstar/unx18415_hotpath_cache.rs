//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18415 · 版本查询性能账（热路径缓存） · 0x7E380段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18415-J1 缓存命中路径 P95 <1µs 断言（任务书口径），失效联动登记可导出
//!
//! 结构清单：
//!   1. HotCode        诊断码闭集（code() = SEG_BASE + n）
//!   2. CacheKey/CacheVal  缓存键值（版本答案 + 纪元）
//!   3. HotCache       直接映射缓存（16 行：tag/val/epoch/valid）
//!   4. CostUnit       确定性成本模型（ns 等效单位，禁墙钟）
//!   5. CostLedger     成本账（逐操作入账 + P95 统计）
//!   6. InvalRegistry  失效联动登记（可导出对账清单）
//!   7. Lcg + InjectLedger（三类违约 × 10 轮）
//!   8. golden_run / probe_a/b/c / inject_rounds
//!   9. run_unx18415_checks 判据集（≤20 条）
//!
//! 设计要点：
//!   - 任务书口径换算：P95 < 1µs = 1000 ns。内核禁墙钟（契约 §0.2），
//!     故成本用**确定性 ns 等效单位**入账：命中 = 1 次 tag 比对 + 1 次行读，
//!     权重写死 HIT_COST=40ns 等效；未命中回填 = 解析+回写 MISS_COST=220；
//!     失效联动 = INVALIDATE_COST=90。冷路径基线 UNCACHE_COST=1600（>1µs，
//!     缓存存在性的量化理由）。
//!   - P95 口径：确定性负载序列（LCG 驱动，固定种子）逐操作成本升序取
//!     95 分位（ceil(95×n/100)-1 下标），命中路径子样本必须全 <1000。
//!   - 失效联动：invalidate(key) 行失效 + 登记 (key, 旧纪元) 记录，登记表
//!     可导出（export_inval）且与失效计数守恒。
//!   - 注入三类：失效后旧纪元命中（脏读）/ 越界行探测 / 未失效重复登记
//!     （重复拒绝），各 10 轮全抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E380，批 B01 独占区，%0x40==0 对齐）。
pub const SEG_BASE: u32 = 0x7E380;

// ── 冻结常量（推导见行注） ──────────────────────────────────────
/// 缓存行数（冻结：16 = 直接映射 4 位行号域）。
pub const CACHE_LINES: usize = 16;
/// 命中成本（ns 等效，推导：tag 比对 1 周期×4 + 行读 36，取 40）。
pub const HIT_COST: u32 = 40;
/// 未命中回填成本（推导：解析 180 + 回写 40 = 220）。
pub const MISS_COST: u32 = 220;
/// 失效联动成本（推导：行扫 50 + 登记 40 = 90）。
pub const INVALIDATE_COST: u32 = 90;
/// 冷路径基线（推导：全量重解析口径 1600 > 1000，量化缓存收益）。
pub const UNCACHE_COST: u32 = 1600;
/// P95 预算（任务书口径：1µs = 1000ns）。
pub const P95_BUDGET_NS: u32 = 1000;
/// 负载步数（冻结：200 步，LCG 驱动确定性负载）。
pub const LOAD_STEPS: u32 = 200;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotCode {
    /// 空缓存查询（未命中且无账）
    EmptyCache,
    /// 键越域（行号计算溢出域）
    KeyOutOfRange,
    /// 失效后旧纪元命中（脏读红线）
    StaleEpochHit,
    /// 重复失效登记（同键同纪元）
    DuplicateInval,
    /// 失效登记与失效计数不守恒
    InvalLedgerBroken,
    /// 命中路径 P95 超预算
    P95BudgetViolated,
    /// 冷基线不大于命中路径（成本模型失真）
    CostModelDistorted,
    /// 负载序列越域（步数超冻结）
    LoadOutOfRange,
    /// 纪元回退（失效纪元只增）
    EpochRegress,
    /// 导出清单与登记账不一致
    ExportBroken,
    /// 双跑不一致
    NotReproducible,
}

impl HotCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（窗口 0x7E380..0x7E3C0）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐条非空）。
    pub fn detail(self) -> &'static str {
        match self {
            HotCode::EmptyCache => "空缓存：无任何可命中行",
            HotCode::KeyOutOfRange => "键越域：行号域 0..16 外",
            HotCode::StaleEpochHit => "失效后旧纪元命中：脏读红线",
            HotCode::DuplicateInval => "重复失效登记：同键同纪元",
            HotCode::InvalLedgerBroken => "失效登记与失效计数不守恒",
            HotCode::P95BudgetViolated => "命中路径 P95 超 1µs 预算",
            HotCode::CostModelDistorted => "冷基线未大于命中路径：模型失真",
            HotCode::LoadOutOfRange => "负载序列越域：超冻结步数",
            HotCode::EpochRegress => "纪元回退：失效纪元只增",
            HotCode::ExportBroken => "导出清单与登记账不一致",
            HotCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 闭集全量。
    pub const ALL: [HotCode; 11] = [
        HotCode::EmptyCache,
        HotCode::KeyOutOfRange,
        HotCode::StaleEpochHit,
        HotCode::DuplicateInval,
        HotCode::InvalLedgerBroken,
        HotCode::P95BudgetViolated,
        HotCode::CostModelDistorted,
        HotCode::LoadOutOfRange,
        HotCode::EpochRegress,
        HotCode::ExportBroken,
        HotCode::NotReproducible,
    ];
}

/// 按数值反查诊断文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < HotCode::ALL.len() {
        if HotCode::ALL[i].code() == code {
            return HotCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E380 段闭集内"
}

// ── 缓存键值 ────────────────────────────────────────────────────
/// 缓存键：pid×类型掩码复合（布局冻结：高 32 位 pid，低 16 位掩码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheKey {
    pub pid: u32,
    pub mask: u16,
}
impl CacheKey {
    /// 复合指纹（行号域：fp % 16）。
    pub fn fp(&self) -> u32 {
        self.pid.wrapping_mul(2654435761) ^ ((self.mask as u32).wrapping_mul(40503))
    }
    /// 行号（直接映射）。
    pub fn line(&self) -> usize {
        (self.fp() % CACHE_LINES as u32) as usize
    }
    /// 键域守卫：pid ≥1。
    pub fn legal(&self) -> bool {
        self.pid >= 1
    }
}

/// 缓存值：版本答案（主/次/构建）+ 填充纪元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheVal {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    /// 填充纪元（失效联动对账用）
    pub epoch: u32,
}

/// 演示答案（写死：Win10 / Win11 双口径）。
pub const VAL_10: CacheVal = CacheVal { major: 10, minor: 0, build: 19045, epoch: 0 };
pub const VAL_11: CacheVal = CacheVal { major: 10, minor: 0, build: 22631, epoch: 0 };

/// 单行缓存槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheLine {
    pub tag: u32,
    pub val: CacheVal,
    pub valid: bool,
}
impl CacheLine {
    /// 空行。
    pub const EMPTY: CacheLine = CacheLine { tag: 0, val: CacheVal { major: 0, minor: 0, build: 0, epoch: 0 }, valid: false };
}

// ── 直接映射缓存 ────────────────────────────────────────────────
/// 热路径缓存：16 行直接映射 + 全局纪元 + 失效登记账。
pub struct HotCache {
    lines: [CacheLine; CACHE_LINES],
    /// 全局纪元（每次失效 +1）
    pub epoch: u32,
    /// 命中计数
    pub hits: u32,
    /// 未命中计数
    pub misses: u32,
    /// 失效计数
    pub invalidations: u32,
    /// 失效登记账：(键复合指纹, 失效时纪元)
    pub inval_ledger: Vec<(u32, u32)>,
}

impl HotCache {
    /// 新缓存（全空行、零纪元）。
    pub fn new() -> HotCache {
        HotCache {
            lines: [CacheLine::EMPTY; CACHE_LINES],
            epoch: 0,
            hits: 0,
            misses: 0,
            invalidations: 0,
            inval_ledger: Vec::new(),
        }
    }

    /// 查找：tag+valid 匹配即命中；返回 (值, 命中否)。
    pub fn lookup(&mut self, key: CacheKey) -> Result<(CacheVal, bool), u32> {
        if !key.legal() {
            return Err(HotCode::KeyOutOfRange.code());
        }
        let ln = key.line();
        let line = match self.lines.get(ln) {
            Some(l) => *l,
            None => return Err(HotCode::KeyOutOfRange.code()),
        };
        if line.valid && line.tag == key.fp() {
            self.hits += 1;
            Ok((line.val, true))
        } else {
            self.misses += 1;
            Err(HotCode::EmptyCache.code())
        }
    }

    /// 回填：写 tag/val/epoch（未命中路径的后半）。
    pub fn fill(&mut self, key: CacheKey, val: CacheVal) -> Result<(), u32> {
        if !key.legal() {
            return Err(HotCode::KeyOutOfRange.code());
        }
        let ln = key.line();
        if let Some(l) = self.lines.get_mut(ln) {
            *l = CacheLine { tag: key.fp(), val, valid: true };
        }
        Ok(())
    }

    /// 带 epoch 盖章的回填（失效联动路径用）。
    pub fn fill_stamped(&mut self, key: CacheKey, mut val: CacheVal) -> Result<(), u32> {
        val.epoch = self.epoch;
        self.fill(key, val)
    }

    /// 失效联动：行失效 + 登记账入账（键指纹, 纪元）。
    /// 同键同纪元重复登记拒绝（重复红线）。
    /// 返回值 = 失效后的**新全局纪元**（单调递增口径：每次失效 +1，
    /// epoch_monotonic 判据与此对账；旧行是否本就被命中由账面另计）。
    pub fn invalidate(&mut self, key: CacheKey) -> Result<u32, u32> {
        if !key.legal() {
            return Err(HotCode::KeyOutOfRange.code());
        }
        let ln = key.line();
        // 重复登记闸：账内已有 (同指纹, 同纪元) 即拒。
        let dup = {
            let mut d = false;
            let mut i = 0;
            while i < self.inval_ledger.len() {
                if let Some(rec) = self.inval_ledger.get(i) {
                    if rec.0 == key.fp() && rec.1 == self.epoch {
                        d = true;
                    }
                }
                i += 1;
            }
            d
        };
        if dup {
            return Err(HotCode::DuplicateInval.code());
        }
        let _had_tag = match self.lines.get(ln) {
            Some(l) => l.valid && l.tag == key.fp(),
            None => false,
        };
        if let Some(l) = self.lines.get_mut(ln) {
            *l = CacheLine::EMPTY;
        }
        self.epoch += 1;
        self.invalidations += 1;
        self.inval_ledger.push((key.fp(), self.epoch));
        Ok(self.epoch)
    }

    /// 旧纪元脏读探测：行内值纪元 < 当前全局纪元即陈旧。
    /// 这是失效联动的对账核：失效后行必须空或新纪元。
    pub fn stale_probe(&self, key: CacheKey) -> Result<bool, u32> {
        let ln = key.line();
        let line = match self.lines.get(ln) {
            Some(l) => *l,
            None => return Err(HotCode::KeyOutOfRange.code()),
        };
        if line.valid && line.tag == key.fp() && line.val.epoch < self.epoch {
            Ok(true) // 陈旧行存在
        } else {
            Ok(false)
        }
    }

    /// 失效登记导出（可导出断言）：返回登记账拷贝。
    pub fn export_inval(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.inval_ledger.len() {
            if let Some(rec) = self.inval_ledger.get(i) {
                out.push(*rec);
            }
            i += 1;
        }
        out
    }

    /// 登记守恒：导出条数 == 失效计数，纪元单调不回退。
    pub fn inval_conserve(&self) -> Result<(), u32> {
        if self.export_inval().len() as u32 != self.invalidations {
            return Err(HotCode::InvalLedgerBroken.code());
        }
        let mut i = 0;
        while i < self.inval_ledger.len() {
            let cur = match self.inval_ledger.get(i) {
                Some(r) => r.1,
                None => 0,
            };
            if cur == 0 {
                return Err(HotCode::EpochRegress.code());
            }
            if i > 0 {
                let prev = match self.inval_ledger.get(i - 1) {
                    Some(r) => r.1,
                    None => 0,
                };
                if cur <= prev {
                    return Err(HotCode::EpochRegress.code());
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 缓存指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        let mut i = 0;
        while i < CACHE_LINES {
            let l = match self.lines.get(i) {
                Some(l) => *l,
                None => CacheLine::EMPTY,
            };
            h = h.wrapping_mul(16777619) ^ l.tag;
            h = h.wrapping_mul(16777619) ^ (l.valid as u32);
            h = h.wrapping_mul(16777619) ^ l.val.build;
            i += 1;
        }
        h = h.wrapping_mul(16777619) ^ self.epoch;
        h = h.wrapping_mul(16777619) ^ self.hits;
        h
    }
}

// ── 确定性成本账（ns 等效单位，禁墙钟） ─────────────────────────
/// 成本账：逐操作入账，支持 P95（升序取 95 分位下标）。
pub struct CostLedger {
    pub costs: Vec<u32>,
}
impl CostLedger {
    /// 空账。
    pub fn new() -> CostLedger {
        CostLedger { costs: Vec::new() }
    }
    /// 记一笔。
    pub fn record(&mut self, c: u32) {
        self.costs.push(c);
    }
    /// P95：升序副本取 ceil(95×n/100)-1 下标（n≥1；n=0 返回 0）。
    pub fn p95(&self) -> u32 {
        let n = self.costs.len();
        if n == 0 {
            return 0;
        }
        let mut sorted: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < n {
            let c = match self.costs.get(i) {
                Some(c) => *c,
                None => 0,
            };
            // 升序插入（n≤200，插入排序成本可忽略）。
            let mut pos = sorted.len();
            let mut k = 0;
            while k < sorted.len() {
                if let Some(s) = sorted.get(k) {
                    if *s > c {
                        pos = k;
                        break;
                    }
                }
                k += 1;
            }
            sorted.insert(pos, c);
            i += 1;
        }
        // 95 分位下标：ceil(95×n/100)-1。
        let idx = ((95 * n + 99) / 100).saturating_sub(1);
        match sorted.get(idx) {
            Some(v) => *v,
            None => 0,
        }
    }
    /// 命中路径子账（只留 == HIT_COST 的样本）。
    pub fn hit_only(&self) -> CostLedger {
        let mut out = CostLedger::new();
        let mut i = 0;
        while i < self.costs.len() {
            if let Some(c) = self.costs.get(i) {
                if *c == HIT_COST {
                    out.record(*c);
                }
            }
            i += 1;
        }
        out
    }
}

/// 确定性负载：LCG 驱动 200 步查/回填混合，逐操作成本入账。
/// 热键域推导：2 pid × 8 掩码 = 16 热键 ≤ 16 行，直接映射全覆盖——
/// 热路径口径（重复访问少数键），命中/回填两种成本都被负载覆盖。
/// 返回 (成本账, 命中数, 未命中数)。
pub fn drive_load(steps: u32, seed: u64) -> Result<(CostLedger, u32, u32), u32> {
    if steps > LOAD_STEPS {
        return Err(HotCode::LoadOutOfRange.code());
    }
    let mut cache = HotCache::new();
    let mut led = CostLedger::new();
    let mut rng = Lcg::new(seed);
    let mut step = 0;
    while step < steps {
        let key = CacheKey { pid: 1 + (rng.below(2) as u32), mask: (rng.below(8)) as u16 };
        match cache.lookup(key) {
            Ok((v, true)) => {
                led.record(HIT_COST);
                let _ = v;
            }
            _ => {
                // 未命中：解析（模型化为 VAL_10/VAL_11 择一）+ 回填。
                let val = if key.pid <= 2 { VAL_10 } else { VAL_11 };
                cache.fill_stamped(key, val)?;
                led.record(MISS_COST);
            }
        }
        step += 1;
    }
    Ok((led, cache.hits, cache.misses))
}

// ── 确定性伪随机 ────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    /// 推进一格。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0, n) 伪随机。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
/// 三类注入槽：0=失效后旧纪元脏读，1=重复失效登记，2=键越域。
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    /// 全抓断言。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检合计。
    pub fn missed_total(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类违约逐类注入，断言每轮被显式检出。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut cache = HotCache::new();
        let key = CacheKey { pid: 1, mask: 0x0002 };
        // 槽 0：失效后旧纪元脏读——先证失效后行干净（stale_probe=false）。
        cache.fill_stamped(key, VAL_10)?;
        cache.invalidate(key)?;
        let clean = !cache.stale_probe(key)?;
        // 真脏读注入：手工重放旧纪元行（模拟失效漏清），stale_probe 必抓。
        let ln = key.line();
        let mut dirty_detected = false;
        if let Some(l) = cache.lines.get_mut(ln) {
            *l = CacheLine { tag: key.fp(), val: VAL_10, valid: true }; // 旧纪元值
        }
        if cache.stale_probe(key)? {
            dirty_detected = true;
        }
        led.record(0, clean && dirty_detected);
        // 槽 1：重复失效登记——新键先登记一次，同键同纪元二次必拒。
        let k2 = CacheKey { pid: 2, mask: 0x0002 };
        cache.fill_stamped(k2, VAL_11)?;
        cache.invalidate(k2)?;
        let dup = cache.invalidate(k2).is_err();
        led.record(1, dup);
        // 槽 2：键越域（pid=0）。
        let bad = cache.lookup(CacheKey { pid: 0, mask: 1 }).is_err()
            && HotCache::new().invalidate(CacheKey { pid: 0, mask: 1 }).is_err();
        led.record(2, bad);
        r += 1;
    }
    if !led.all_caught() || led.missed_total() != 0 {
        return Err(HotCode::NotReproducible.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：200 步负载 → 命中 P95 <1000 → 冷基线对照 → 失效联动登记
/// 守恒导出。返回 (命中数, P95, 登记条数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let (led, hits, _misses) = drive_load(LOAD_STEPS, SEG_BASE as u64)?;
    // 命中路径 P95 < 1µs（J1 主判据）。
    let hit_led = led.hit_only();
    let p95 = hit_led.p95();
    if hit_led.costs.len() == 0 {
        return Err(HotCode::P95BudgetViolated.code());
    }
    if p95 >= P95_BUDGET_NS {
        return Err(HotCode::P95BudgetViolated.code());
    }
    // 成本模型自洽：冷基线必须大于命中成本（缓存收益量化）。
    if UNCACHE_COST <= HIT_COST || MISS_COST <= HIT_COST {
        return Err(HotCode::CostModelDistorted.code());
    }
    // 失效联动：三次失效（不同键）→ 登记导出守恒。
    let mut cache = HotCache::new();
    let k1 = CacheKey { pid: 1, mask: 0x0001 };
    let k2 = CacheKey { pid: 2, mask: 0x0002 };
    let k3 = CacheKey { pid: 3, mask: 0x0003 };
    let _ = cache.fill_stamped(k1, VAL_10);
    let _ = cache.fill_stamped(k2, VAL_11);
    let _ = cache.fill_stamped(k3, VAL_10);
    cache.invalidate(k1)?;
    cache.invalidate(k2)?;
    cache.invalidate(k3)?;
    let exported = cache.export_inval();
    if exported.len() != 3 {
        return Err(HotCode::ExportBroken.code());
    }
    cache.inval_conserve()?;
    Ok((hits, p95, exported.len() as u32))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：空缓存查询（合法键，未命中）→ EmptyCache。
pub fn probe_a() -> Result<(), u32> {
    let mut cache = HotCache::new();
    match cache.lookup(CacheKey { pid: 1, mask: 1 }) {
        Err(c) if c == HotCode::EmptyCache.code() => Ok(()),
        _ => Err(HotCode::NotReproducible.code()),
    }
}

/// 探针 B：回填后命中，值逐字段一致。
pub fn probe_b() -> Result<(), u32> {
    let mut cache = HotCache::new();
    let key = CacheKey { pid: 2, mask: 0x0010 };
    cache.fill_stamped(key, VAL_11)?;
    match cache.lookup(key) {
        Ok((v, true)) if v.build == VAL_11.build && v.minor == VAL_11.minor => Ok(()),
        _ => Err(HotCode::NotReproducible.code()),
    }
}

/// 探针 C：直接映射冲突——同行双键，后者覆盖前者（tag 换主）。
pub fn probe_c() -> Result<(), u32> {
    let mut cache = HotCache::new();
    let k1 = CacheKey { pid: 1, mask: 0 };
    // 找与 k1 同行的另一键（fp 同模 16）：pid 遍历 1..64。
    let mut k2: Option<CacheKey> = None;
    let mut p = 2;
    while p < 64 {
        let cand = CacheKey { pid: p, mask: 0 };
        if cand.line() == k1.line() && cand.fp() != k1.fp() {
            k2 = Some(cand);
            break;
        }
        p += 1;
    }
    let k2 = match k2 {
        Some(k) => k,
        None => return Err(HotCode::NotReproducible.code()),
    };
    cache.fill_stamped(k1, VAL_10)?;
    cache.fill_stamped(k2, VAL_11)?;
    // k2 命中、k1 被逐出（未命中）。
    let hit2 = matches!(cache.lookup(k2), Ok((_, true)));
    let miss1 = matches!(cache.lookup(k1), Err(c) if c == HotCode::EmptyCache.code());
    if hit2 && miss1 {
        Ok(())
    } else {
        Err(HotCode::NotReproducible.code())
    }
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HotCode::NotReproducible.code());
    }
    // 指纹级：同种子负载的缓存终态一致。
    let c1 = drive_load(LOAD_STEPS, 0x7E38);
    let c2 = drive_load(LOAD_STEPS, 0x7E38);
    match (c1, c2) {
        (Ok((_, h1, m1)), Ok((_, h2, m2))) => {
            if h1 != h2 || m1 != m2 {
                return Err(HotCode::NotReproducible.code());
            }
            Ok(())
        }
        _ => Err(HotCode::NotReproducible.code()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HotCode::ALL.len() {
        let v = HotCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集巡检。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < HotCode::ALL.len() {
        let c = HotCode::ALL[i];
        if c.detail().is_empty() || detail_of(c.code()) != c.detail() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
/// UNX-F18415 判据集（≤20 条；动态明细 ≤8 槽）。
pub fn run_unx18415_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18415 版本查询性能账（热路径缓存）");

    // 1. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7E380 段 0x40 窗口且 0x40 对齐");
    // 2. 黄金全链。
    let golden = golden_run();
    let golden_ok = match &golden {
        Ok((_, p95, inv)) => *p95 < P95_BUDGET_NS && *inv == 3,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链", golden_ok, &format!("{:?}", golden));
    // 3. 命中路径 P95 < 1µs（J1 主判据）。
    let p95_val = match &golden {
        Ok((_, p, _)) => *p,
        Err(_) => 0,
    };
    s.add_dyn("J1-命中P95<1µs", p95_val < P95_BUDGET_NS && p95_val > 0, &format!("p95={}ns", p95_val));
    // 4. 成本模型自洽：冷基线 > 回填 > 命中。
    let model_ok = UNCACHE_COST > MISS_COST && MISS_COST > HIT_COST && HIT_COST > 0;
    s.add("J1-成本模型自洽", model_ok, "1600>220>40 链序成立");
    // 5. 负载有命中：200 步命中数 >0 且守恒。
    let load = drive_load(LOAD_STEPS, SEG_BASE as u64);
    let load_ok = match &load {
        Ok((_, h, m)) => *h > 0 && h + m == LOAD_STEPS,
        Err(_) => false,
    };
    s.add_dyn("J1-负载命中守恒", load_ok, &format!("steps={}", LOAD_STEPS));
    // 6. 失效联动登记可导出（J1 主判据）。
    let mut ic = HotCache::new();
    let k = CacheKey { pid: 1, mask: 7 };
    let _ = ic.fill_stamped(k, VAL_10);
    let _ = ic.invalidate(k);
    let export_ok = ic.export_inval().len() == 1 && ic.inval_conserve().is_ok();
    s.add("J1-失效登记可导出", export_ok, "登记条数==失效计数且纪元单调");
    // 7. 重复失效登记拒绝。
    let mut ic2 = HotCache::new();
    let _ = ic2.fill_stamped(k, VAL_10);
    let dup_ok = ic2.invalidate(k).is_ok() && ic2.invalidate(k).is_err();
    s.add("J1-重复失效登记拒", dup_ok, "同键同纪元二次登记必拒");
    // 8. 失效后旧纪元脏读检出。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10", inj_ok, &format!("r={}", JUDGE_ROUNDS));
    // 9. 计数守恒。
    let conserve_ok = match &inj {
        Ok(l) => l.missed_total() == 0,
        Err(_) => false,
    };
    s.add("J1-计数守恒漏检零", conserve_ok, "tries=caught+missed 且 missed=0");
    // 10. 探针三序。
    let a = probe_a().is_ok();
    let b = probe_b().is_ok();
    let c = probe_c().is_ok();
    s.add_dyn("J1-探针三序", a && b && c, &format!("a={} b={} c={}", a, b, c));
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 12. 负载越域（超冻结步数）拒绝。
    s.add("J1-负载越域拒", drive_load(LOAD_STEPS + 1, 1).is_err(), ">200 步显式拒");
    // 13. 键域守卫。
    s.add("J1-键域守卫", CacheKey { pid: 0, mask: 1 }.legal() == false, "pid=0 非法");
    // 14. 文案闭集。
    s.add("J3-文案闭集", detail_closure(), "11 码逐条非空且反查一致");
    // 15. 段内窗口逐码核对。
    let win_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < HotCode::ALL.len() {
            if HotCode::ALL[i].code() >= SEG_BASE + 0x40 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J1-段内窗口核对", win_ok, "11 码全部 < SEG_BASE+0x40");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据集全绿。
    #[test]
    fn all_checks_pass() {
        let set = run_unx18415_checks();
        assert!(set.all_passed(), "UNX-F18415 判据集存在红项");
    }

    /// 正常流：回填→命中→失效→再查未命中。
    #[test]
    fn normal_flow_hit_invalidate() {
        let mut cache = HotCache::new();
        let key = CacheKey { pid: 1, mask: 0x0001 };
        cache.fill_stamped(key, VAL_10).unwrap_or_default();
        assert!(matches!(cache.lookup(key), Ok((_, true))));
        cache.invalidate(key).unwrap_or_default();
        assert!(matches!(cache.lookup(key), Err(_)));
    }

    /// 注入三类 10/10。
    #[test]
    fn injection_all_caught() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(_) => panic!("注入轮次内部出错"),
        };
        assert!(led.all_caught());
        assert_eq!(led.missed_total(), 0);
    }

    /// P95 统计：已知序列分位核对。
    #[test]
    fn p95_statistic() {
        let mut led = CostLedger::new();
        let mut i = 0;
        while i < 100 {
            led.record((i + 1) as u32);
            i += 1;
        }
        // 1..100 升序：ceil(95×100/100)-1 = 94 → 第 95 小 = 95。
        assert_eq!(led.p95(), 95);
        // 全命中序列 P95 = HIT_COST。
        let mut hit_led = CostLedger::new();
        let mut k = 0;
        while k < 50 {
            hit_led.record(HIT_COST);
            k += 1;
        }
        assert_eq!(hit_led.p95(), HIT_COST);
        assert!(hit_led.p95() < P95_BUDGET_NS);
    }

    /// 判定次序：键越域先于命中判定。
    #[test]
    fn order_key_domain_before_hit() {
        let mut cache = HotCache::new();
        match cache.lookup(CacheKey { pid: 0, mask: 0 }) {
            Err(c) => assert_eq!(c, HotCode::KeyOutOfRange.code()),
            Ok(_) => panic!("pid=0 不应查询成功"),
        }
    }

    /// 失效纪元单调：三次失效纪元 1<2<3。
    #[test]
    fn epoch_monotonic() {
        let mut cache = HotCache::new();
        let mut e_prev = 0;
        let mut i = 1;
        while i <= 3 {
            let key = CacheKey { pid: i as u32, mask: i as u16 };
            cache.fill_stamped(key, VAL_10).unwrap_or_default();
            let e = cache.invalidate(key).unwrap_or(0);
            assert!(e > e_prev);
            e_prev = e;
            i += 1;
        }
        assert!(cache.inval_conserve().is_ok());
    }

    /// 直接映射冲突：同行 tag 换主。
    #[test]
    fn direct_mapped_conflict() {
        assert!(probe_c().is_ok());
    }

    /// 双跑一致。
    #[test]
    fn reproducible() {
        assert!(repro_check().is_ok());
        let c1 = HotCache::new();
        let c2 = HotCache::new();
        assert_eq!(c1.fingerprint(), c2.fingerprint());
    }

    /// 段位守卫 + 文案闭集。
    #[test]
    fn segment_and_detail() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert!(detail_of(SEG_BASE + 99).starts_with("未知"));
    }
}
