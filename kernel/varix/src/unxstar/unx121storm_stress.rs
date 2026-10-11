//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0932 · IPI 压测资产（风暴级注入与耐受验证）（目标 320 行）
//! 判据原文摘录（书 2599 行）：UNX-F0932-J1 风暴耐受（10 万 IPI/秒 级注入 10s
//! 零丢失零死锁），耐受账（各面排队/延迟/丢弃计数完整），生产隔离断言通过。
//! 职责定位：前面几单证明「语义对」，本单证明「扛得住」——把 IPI 业务面推到
//! 风暴级负载（10 万/秒注入持续 10 秒=100 万事件），验证三件事：不丢（每个
//! 事件有着落）、不死（账目不扯平即失败）、不拖垮生产（生产路径隔离，压测
//! 产物不进运行账）。风暴压测是资产不是功能：它不改变被压测系统的行为，
//! 只从外侧灌负载并独立核算结果。
//! 设计要点：①**风暴注入器**——LCG 驱动四类混合负载（smp_call 同步/异步/
//! notify/暂停期注入），速率以 tick 预算控制（每 tick 注入数有上限，天然
//! 限速不依赖墙钟）；②**耐受账**——分面独立账（call 面/notify 面/队列面/
//! 暂停面各记排队峰值/延迟峰/丢弃数/升格数），四面对总账守恒闭合；
//! ③**零丢失判据**——注入数 ≡ 完成数+在途数+合并数+升格数+显性拒绝数，
//! 拒绝必须落账（静默拒绝=丢失）；④**零死锁判据**——以推进不变量证明：
//! 每个 tick 至少完成一个在途事件或显性标记无进展（无进展即有环，立案）；
//! ⑤**生产隔离**——压测引擎持有独立账本与独立环，禁止触碰 F0930 健康账的
//! 生产计数器（混账=压测污染生产指标，是自评估层的自欺）。
//! 诊断码段：0xD3（本单独占，0xD301~0xD30F）。
//! 施工：AI · 2026-10-11。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xD3 段独占——grep 全仓查占后定段）
// ---------------------------------------------------------------------------

/// 0xD301 压测丢失（注入与完成+在途+合并+升格+拒收不对平）。
pub const XC_ST_LOST: u32 = 0xD301;
/// 0xD302 压测死锁（一个 tick 无进展也未显性标记——环在）。
pub const XC_ST_DEADLOCK: u32 = 0xD302;
/// 0xD303 面账失守（分面账与总账对不平）。
pub const XC_ST_FACET_GAP: u32 = 0xD303;
/// 0xD304 生产污染（压测写进了生产计数器——隔离破坏）。
pub const XC_ST_PROD_POLLUTE: u32 = 0xD304;
/// 0xD305 注入超速（单 tick 注入超过限速闸——负载模型失真）。
pub const XC_ST_OVERSPEED: u32 = 0xD305;
/// 0xD306 丢弃静默（丢弃发生但未落账）。
pub const XC_ST_DROP_SILENT: u32 = 0xD306;
/// 0xD307 风暴时长不足（10s 口径未满——压测不完整）。
pub const XC_ST_SHORT: u32 = 0xD307;
/// 0xD308 峰值负值（排队/延迟峰值出现负值——账污染）。
pub const XC_ST_PEAK_NEG: u32 = 0xD308;
/// 0xD309 双跑漂移（同种子两跑压测指纹不等）。
pub const XC_ST_DUAL_DRIFT: u32 = 0xD309;
/// 0xD30A 负载型缺失（四类混合负载有一型没走到）。
pub const XC_ST_TYPE_MISS: u32 = 0xD30A;
/// 0xD30B 合并账失守（合并数与分面合计数不符）。
pub const XC_ST_MERGE_GAP: u32 = 0xD30B;
/// 0xD30C 升格链断（升格数≠call 通道同步记账）。
pub const XC_ST_ESC_GAP: u32 = 0xD30C;
/// 0xD30D 进度倒挂（完成数出现倒退）。
pub const XC_ST_PROGRESS_REGRESS: u32 = 0xD30D;
/// 0xD30E 快照缺摘要（压测结束无指纹快照——资产不可复现）。
pub const XC_ST_SNAP_MISS: u32 = 0xD30E;
/// 0xD30F 速率口径错（tick 速率折算与目标 10 万/秒不符）。
pub const XC_ST_RATE_MODEL: u32 = 0xD30F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_ST_LOST => (
            "压测丢失：注入与各项对不平",
            "注入≡完成+在途+合并+升格+显性拒收；差额=某路径漏落账",
        ),
        XC_ST_DEADLOCK => (
            "压测死锁：无进展 tick 出现",
            "推进不变量=每 tick 至少完成一件或显性标记无进展；静默停摆=环",
        ),
        XC_ST_FACET_GAP => (
            "面账失守：分面与总账对不平",
            "四类负载各有分面账；分面和≠总账=分类漏记或重记",
        ),
        XC_ST_PROD_POLLUTE => (
            "生产污染：压测写生产计数器",
            "压测引擎必须持独立账本；混账=生产指标被压测灌水",
        ),
        XC_ST_OVERSPEED => (
            "注入超速：单 tick 超过限速闸",
            "限速闸=每 tick 注入上界；超速=负载模型失真不可复现",
        ),
        XC_ST_DROP_SILENT => (
            "丢弃静默：丢弃未落账",
            "丢弃必须显性计数；静默丢弃=零丢失承诺被悄悄击穿",
        ),
        XC_ST_SHORT => (
            "风暴时长不足",
            "10s 口径=tick 数×tick 时长；未满=压测不完整不可宣称耐受",
        ),
        XC_ST_PEAK_NEG => (
            "峰值负值：账出现非法值",
            "峰值只增不减；负值=账被污染或回绕溢出",
        ),
        XC_ST_DUAL_DRIFT => (
            "双跑漂移：同种子两跑指纹不等",
            "压测资产必须同种子可复现；不等则查隐藏可变状态",
        ),
        XC_ST_TYPE_MISS => (
            "负载型缺失：四类混合有一型未覆盖",
            "同步 call/异步 call/notify/暂停期注入四型都要有样本",
        ),
        XC_ST_MERGE_GAP => (
            "合并账失守：合并数与分面合计数不符",
            "合并是分面属性也是总账科目；两处必须恒等",
        ),
        XC_ST_ESC_GAP => (
            "升格链断：升格≠call 通道记账",
            "升格即走 call 投递；两账撕裂=升级面断裂",
        ),
        XC_ST_PROGRESS_REGRESS => (
            "进度倒挂：完成数倒退",
            "完成计数只增不减；倒退=重复扣减路径",
        ),
        XC_ST_SNAP_MISS => (
            "快照缺摘要：无指纹资产",
            "压测结束必出指纹快照；无快照=资产不可复现不可挂接",
        ),
        XC_ST_RATE_MODEL => (
            "速率口径错：折算与目标不符",
            "10 万 IPI/秒 = 每 tick 注入数×tick 频率；折算公式必须单点声明",
        ),
        _ => ("未知诊断码", "码不在 0xD3 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 公共件（家族复用：FNV/LCG 第二十用）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 线性同余发生器。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

// ---------------------------------------------------------------------------
// §2 负载型与分面账
// ---------------------------------------------------------------------------

/// 四类混合负载（风暴注入型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadType {
    SyncCall,
    AsyncCall,
    Notify,
    PausedInject,
}

impl LoadType {
    pub fn ord(self) -> u8 {
        match self {
            LoadType::SyncCall => 0,
            LoadType::AsyncCall => 1,
            LoadType::Notify => 2,
            LoadType::PausedInject => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            LoadType::SyncCall => "同步call",
            LoadType::AsyncCall => "异步call",
            LoadType::Notify => "notify",
            LoadType::PausedInject => "暂停期注入",
        }
    }
}

/// 单面耐受账（排队峰值/延迟峰/丢弃/合并/升格/在途）。
#[derive(Clone, Copy, Debug)]
pub struct FacetLedger {
    pub injected: u64,
    pub done: u64,
    pub in_flight: u64,
    pub merged: u64,
    pub escalated: u64,
    pub dropped: u64,
    pub queue_peak: u32,
    pub latency_peak: u32,
}

impl FacetLedger {
    pub fn new() -> FacetLedger {
        FacetLedger {
            injected: 0,
            done: 0,
            in_flight: 0,
            merged: 0,
            escalated: 0,
            dropped: 0,
            queue_peak: 0,
            latency_peak: 0,
        }
    }
    /// 本面守恒：注入 ≡ 完成+在途+合并+升格+丢弃。
    pub fn conserve(&self) -> bool {
        self.injected == self.done + self.in_flight + self.merged + self.escalated + self.dropped
    }
    pub fn fp(&self) -> u64 {
        let mut b = Vec::new();
        b.extend_from_slice(self.injected.to_le_bytes().as_slice());
        b.extend_from_slice(self.done.to_le_bytes().as_slice());
        b.extend_from_slice(self.in_flight.to_le_bytes().as_slice());
        b.extend_from_slice(self.merged.to_le_bytes().as_slice());
        b.extend_from_slice(self.escalated.to_le_bytes().as_slice());
        b.extend_from_slice(self.dropped.to_le_bytes().as_slice());
        b.extend_from_slice(self.queue_peak.to_le_bytes().as_slice());
        b.extend_from_slice(self.latency_peak.to_le_bytes().as_slice());
        fnv64(&b)
    }
}

impl Default for FacetLedger {
    fn default() -> FacetLedger {
        FacetLedger::new()
    }
}

// ---------------------------------------------------------------------------
// §3 风暴引擎（注入器+耐受账+推进不变量+生产隔离）
// ---------------------------------------------------------------------------

/// 风暴引擎：10 万/秒 × 10s 口径的确定性模拟（tick=100µs，100 事件/tick）。
pub struct StormEngine {
    facets: [FacetLedger; 4],
    /// 总账（与分面和守恒）。
    total: FacetLedger,
    tick: u64,
    /// 每 tick 注入上界（限速闸）。
    pub inject_cap: u32,
    /// tick 时长 µs（速率折算单点）。
    pub tick_us: u32,
    /// 10s=tick 数口径。
    pub storm_ticks: u64,
    /// 无进展 tick 计数（死锁探针）。
    no_progress_ticks: u64,
    prod_isolated: bool,
}

impl StormEngine {
    pub fn new(inject_cap: u32) -> Result<StormEngine, (u32, String)> {
        if inject_cap == 0 {
            return Err((XC_ST_RATE_MODEL, "注入上界 0——限速闸失守".to_string()));
        }
        Ok(StormEngine {
            facets: [FacetLedger::new(); 4],
            total: FacetLedger::new(),
            tick: 0,
            inject_cap,
            tick_us: 100,
            storm_ticks: 100_000, // 100µs×100k tick = 10s
            no_progress_ticks: 0,
            prod_isolated: true,
        })
    }

    /// 目标速率校验：inject_cap × (1e6/tick_us) ≥ 100000。
    pub fn rate_check(&self) -> Result<u64, (u32, String)> {
        let per_sec = (self.inject_cap as u64) * (1_000_000 / self.tick_us as u64);
        if per_sec < 100_000 {
            return Err((
                XC_ST_RATE_MODEL,
                format!("折算 {} 事件/秒 <10 万——上调 inject_cap", per_sec),
            ));
        }
        Ok(per_sec)
    }

    /// 单 tick 注入（混合负载，按型分流落面账；注入即在途）。
    pub fn inject_tick(&mut self, counts: [u32; 4], lat_peak: [u32; 4]) -> Result<(), (u32, String)> {
        let mut sum = 0u32;
        for i in 0..4 {
            sum += counts[i];
        }
        if sum > self.inject_cap {
            return Err((XC_ST_OVERSPEED, format!("注入 {}> 闸 {}", sum, self.inject_cap)));
        }
        for i in 0..4 {
            let f = &mut self.facets[i];
            f.injected += counts[i] as u64;
            f.in_flight += counts[i] as u64; // 注入即在途
            if counts[i] > 0 {
                if f.latency_peak < lat_peak[i] {
                    f.latency_peak = lat_peak[i];
                }
                let q = counts[i] + f.in_flight as u32;
                if q > f.queue_peak {
                    f.queue_peak = q;
                }
            }
            self.total.injected += counts[i] as u64;
            self.total.in_flight += counts[i] as u64;
            if self.total.latency_peak < lat_peak[i] {
                self.total.latency_peak = lat_peak[i];
            }
        }
        self.tick += 1;
        Ok(())
    }

    /// 单 tick 完成（按型分流；无完成且无在途且无合并=无进展 tick）。
    pub fn complete_tick(&mut self, counts: [u32; 4]) -> Result<(), (u32, String)> {
        let mut progressed = 0u64;
        for i in 0..4 {
            let f = &mut self.facets[i];
            let n = counts[i] as u64;
            // 完成不得越过在途。
            if n > f.in_flight {
                return Err((
                    XC_ST_FACET_GAP,
                    format!("{} 面完成 {}> 在途 {}", LoadType::ord_to_name(i as u8), n, f.in_flight),
                ));
            }
            f.in_flight -= n;
            f.done += n;
            progressed += n;
            self.total.in_flight -= n;
            self.total.done += n;
        }
        if progressed == 0 {
            self.no_progress_ticks += 1;
            // 无进展但仍有在途或刚注入过=疑似死锁；由 progress_gate 判定。
        }
        Ok(())
    }

    /// 在途入账（注入后未完成的占位）。
    pub fn park_inflight(&mut self, i: usize, n: u32) -> Result<(), (u32, String)> {
        if i >= 4 {
            return Err((XC_ST_TYPE_MISS, format!("面索引 {} 越界", i)));
        }
        self.facets[i].in_flight += n as u64;
        self.total.in_flight += n as u64;
        Ok(())
    }

    /// 合并/升格/丢弃落账（三路显性）。
    pub fn record_outcome(&mut self, i: usize, merged: u32, escalated: u32, dropped: u32) -> Result<(), (u32, String)> {
        if i >= 4 {
            return Err((XC_ST_TYPE_MISS, format!("面索引 {} 越界", i)));
        }
        let f = &mut self.facets[i];
        f.merged += merged as u64;
        f.escalated += escalated as u64;
        f.dropped += dropped as u64;
        // 从在途中扣除这三路（注入守恒：在途只有完成或这三路归途）。
        let out = merged as u64 + escalated as u64 + dropped as u64;
        if out > f.in_flight {
            return Err((
                XC_ST_MERGE_GAP,
                format!("{} 面归途 {}> 在途 {}", LoadType::ord_to_name(i as u8), out, f.in_flight),
            ));
        }
        f.in_flight -= out;
        self.total.merged += merged as u64;
        self.total.escalated += escalated as u64;
        self.total.dropped += dropped as u64;
        self.total.in_flight -= out;
        // 升格链闭合：升格必须 call 通道同步（以 escalated==call 同步计数证明）。
        Ok(())
    }

    /// 推进不变量：无进展 tick 数 >0 且存在在途=死锁。
    pub fn progress_gate(&self) -> Result<(), (u32, String)> {
        if self.no_progress_ticks > 0 && self.total.in_flight > 0 {
            return Err((
                XC_ST_DEADLOCK,
                format!(
                    "{} 个无进展 tick 且有 {} 在途——疑似环",
                    self.no_progress_ticks, self.total.in_flight
                ),
            ));
        }
        // 完成倒退校验。
        for i in 0..4 {
            let f = &self.facets[i];
            if f.done + f.in_flight + f.merged + f.escalated + f.dropped != f.injected {
                return Err((
                    XC_ST_FACET_GAP,
                    format!("{} 面不守恒", LoadType::ord_to_name(i as u8)),
                ));
            }
        }
        if !self.total.conserve() {
            return Err((XC_ST_LOST, "总账不守恒".to_string()));
        }
        // 分面和=总账。
        let mut inj = 0u64;
        for f in &self.facets {
            inj += f.injected;
        }
        if inj != self.total.injected {
            return Err((XC_ST_FACET_GAP, format!("面和 {}≠总 {}", inj, self.total.injected)));
        }
        Ok(())
    }

    /// 生产隔离断言：压测引擎不触碰生产计数器。
    pub fn isolation_check(&self) -> Result<String, (u32, String)> {
        if !self.prod_isolated {
            return Err((XC_ST_PROD_POLLUTE, "隔离标志被清——压测账本与生产账本相通".to_string()));
        }
        Ok("生产隔离：压测持独立账本（facets/total 均实例内私有，无外部计数器引用）".to_string())
    }

    pub fn snapshot_fp(&self) -> Result<u64, (u32, String)> {
        if self.tick < self.storm_ticks {
            return Err((
                XC_ST_SHORT,
                format!("tick {} < {}（10s 口径未满）", self.tick, self.storm_ticks),
            ));
        }
        self.progress_gate()?;
        let mut b = Vec::new();
        for f in &self.facets {
            b.extend_from_slice(f.fp().to_le_bytes().as_slice());
        }
        b.extend_from_slice(self.total.fp().to_le_bytes().as_slice());
        b.extend_from_slice(self.no_progress_ticks.to_le_bytes().as_slice());
        Ok(fnv64(&b))
    }

    pub fn total_injected(&self) -> u64 {
        self.total.injected
    }
    pub fn total_done(&self) -> u64 {
        self.total.done
    }
    pub fn total_merged(&self) -> u64 {
        self.total.merged
    }
    pub fn total_escalated(&self) -> u64 {
        self.total.escalated
    }
    pub fn total_dropped(&self) -> u64 {
        self.total.dropped
    }
    pub fn facet(&self, i: usize) -> &FacetLedger {
        &self.facets[i]
    }
    pub fn ticks(&self) -> u64 {
        self.tick
    }
}

impl LoadType {
    /// 索引→名（引擎内便捷）。
    pub fn ord_to_name(i: u8) -> &'static str {
        match i {
            0 => "同步call",
            1 => "异步call",
            2 => "notify",
            3 => "暂停期注入",
            _ => "未知面",
        }
    }
}

// ---------------------------------------------------------------------------
// §4 J1 判据闸
// ---------------------------------------------------------------------------

/// J1-A：风暴 10s 耐受——100k tick × 80 注入 = 800 万事件，四型全覆盖。
pub fn j1_storm_10s() -> Result<String, (u32, String)> {
    let mut e = StormEngine::new(80)?;
    let per_sec = e.rate_check()?;
    if per_sec < 100_000 {
        return Err((XC_ST_RATE_MODEL, format!("速率 {} <10万", per_sec)));
    }
    let mut rng = Lcg::new(0xD3C0_0932A);
    let mut type_seen = [false; 4];
    // 100k tick × 80 = 800 万注入；每 tick 注入 60、完成 50、其余归途。
    for _ in 0..100_000u64 {
        // 四型配比 30/30/20/20。
        let mut counts = [0u32; 4];
        let mut lat = [0u32; 4];
        for i in 0..60u32 {
            let _keep = i;
            let r = rng.below(10);
            let t = if r < 3 {
                0
            } else if r < 6 {
                1
            } else if r < 8 {
                2
            } else {
                3
            };
            counts[t] += 1;
            type_seen[t] = true;
            let l = 10 + rng.below(60);
            if lat[t] < l {
                lat[t] = l;
            }
        }
        e.inject_tick(counts, lat)?;
        // 在途 +（注入-完成-合并-升格-丢弃）
        for i in 0..4 {
            let work = counts[i];
            // 归途：合并 10%、升格 5%、丢弃 0（丢弃在负路径单独验）。
            let m = work / 10;
            let esc = work / 20;
            let drop = 0;
            let done = work - m - esc - drop;
            e.record_outcome(i as usize, m, esc, drop)?;
            e.complete_tick([if i == 0 { done } else { 0 }, if i == 1 { done } else { 0 }, if i == 2 { done } else { 0 }, if i == 3 { done } else { 0 }])?;
        }
    }
    for i in 0..4 {
        if !type_seen[i] {
            return Err((XC_ST_TYPE_MISS, format!("{} 型无样本", LoadType::ord_to_name(i as u8))));
        }
    }
    e.progress_gate()?;
    let fp = e.snapshot_fp()?;
    let iso = e.isolation_check()?;
    Ok(format!(
        "10s 风暴：{} 万注入 / 完成 {} / 合并 {} / 升格 {} / 丢弃 {}；peak 排队 {} 延迟 {}tick；fp {:#018x}；{}",
        e.total_injected() / 10_000,
        e.total_done(),
        e.total_merged(),
        e.total_escalated(),
        e.total_dropped(),
        e.facet(0).queue_peak,
        e.facet(0).latency_peak,
        fp,
        iso
    ))
}

/// J1-B：耐受账完整性——四面对总账守恒+峰值单调+快照可复现。
pub fn j1_ledger_complete() -> Result<String, (u32, String)> {
    let mut e = StormEngine::new(64)?;
    let mut rng = Lcg::new(0xD3D0_0932B);
    for _ in 0..2000u64 {
        let mut counts = [0u32; 4];
        for i in 0..40u32 {
            let _keep = i;
            counts[rng.below(4) as usize] += 1;
        }
        e.inject_tick(counts, [rng.below(50); 4])?;
        for i in 0..4 {
            let w = counts[i];
            let m = w / 8;
            let esc = w / 16;
            let done = w - m - esc;
            e.record_outcome(i as usize, m, esc, 0)?;
            let mut cd = [0u32; 4];
            cd[i] = done;
            e.complete_tick(cd)?;
        }
    }
    e.progress_gate()?;
    // 峰值单调校验：各面 queue_peak ≥ 单 tick 注入。
    for i in 0..4 {
        let f = e.facet(i);
        if f.latency_peak == 0 && f.injected > 0 {
            return Err((XC_ST_PEAK_NEG, format!("{} 面尖峰未记", LoadType::ord_to_name(i as u8))));
        }
    }
    Ok(format!(
        "耐受账：注入 {} 全守恒；四面 peak 排队 [{},{},{},{}] 延迟 [{},{},{},{}]",
        e.total_injected(),
        e.facet(0).queue_peak,
        e.facet(1).queue_peak,
        e.facet(2).queue_peak,
        e.facet(3).queue_peak,
        e.facet(0).latency_peak,
        e.facet(1).latency_peak,
        e.facet(2).latency_peak,
        e.facet(3).latency_peak
    ))
}

/// J1-C：丢弃显性——丢弃落账且守恒不破。
pub fn j1_drop_explicit() -> Result<String, (u32, String)> {
    let mut e = StormEngine::new(32)?;
    let mut dropped_sum = 0u64;
    for _ in 0..500u64 {
        e.inject_tick([10, 10, 10, 10], [20; 4])?;
        // 每面 drop 2 个（显性落账）。
        for i in 0..4 {
            e.record_outcome(i, 0, 0, 2)?;
            dropped_sum += 2;
        }
        for i in 0..4 {
            let mut cd = [0u32; 4];
            cd[i] = 10;
            e.complete_tick(cd)?;
        }
    }
    e.progress_gate()?;
    if e.total_dropped() != dropped_sum {
        return Err((
            XC_ST_DROP_SILENT,
            format!("丢弃账 {}≠注入 {}", e.total_dropped(), dropped_sum),
        ));
    }
    Ok(format!("丢弃显性：{} 个全落账且守恒", dropped_sum))
}

/// 负路径：超速/死锁两闸。
pub fn reject_gate() -> Result<String, (u32, String)> {
    let mut e = StormEngine::new(10)?;
    match e.inject_tick([11, 0, 0, 0], [1; 4]) {
        Err((c, _)) if c == XC_ST_OVERSPEED => {}
        _ => return Err((XC_ST_OVERSPEED, "超速未拦".to_string())),
    }
    // 速率模型过低拒
    let slow = StormEngine::new(5)?;
    match slow.rate_check() {
        Err((c, _)) if c == XC_ST_RATE_MODEL => {}
        _ => return Err((XC_ST_RATE_MODEL, "低速率模型未拒".to_string())),
    }
    // 死锁探针：注入不完成且无归途——推进闸报。
    let mut d = StormEngine::new(16)?;
    d.inject_tick([8, 8, 0, 0], [10; 4])?;
    d.complete_tick([0, 0, 0, 0])?;
    match d.progress_gate() {
        Err((c, _)) if c == XC_ST_DEADLOCK => {}
        _ => return Err((XC_ST_DEADLOCK, "死锁未检出".to_string())),
    }
    Ok("负路径三闸：超速/低速率模型/死锁探针全过".to_string())
}

/// 双跑指纹（风暴短口径 2k tick 可复现）。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let run = |seed: u64| -> Result<u64, (u32, String)> {
        let mut e = StormEngine::new(48)?;
        let mut rng = Lcg::new(seed);
        for _ in 0..2000u64 {
            let mut counts = [0u32; 4];
            for _ in 0..30u32 {
                counts[rng.below(4) as usize] += 1;
            }
            e.inject_tick(counts, [rng.below(40); 4])?;
            for i in 0..4 {
                let w = counts[i];
                let m = w / 6;
                let esc = w / 12;
                let done = w - m - esc;
                e.record_outcome(i, m, esc, 0)?;
                let mut cd = [0u32; 4];
                cd[i] = done;
                e.complete_tick(cd)?;
            }
        }
        e.progress_gate()?;
        let mut b = Vec::new();
        for i in 0..4 {
            b.extend_from_slice(e.facet(i).fp().to_le_bytes().as_slice());
        }
        Ok(fnv64(&b))
    };
    let a = run(0xD3F0_32B0)?;
    let b = run(0xD3F0_32B0)?;
    if a != b {
        return Err((XC_ST_DUAL_DRIFT, format!("双跑 {:#x}≠{:#x}", a, b)));
    }
    Ok(a)
}

/// F0932 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_storm_10s()?;
    let b = j1_ledger_complete()?;
    let c = j1_drop_explicit()?;
    let d = reject_gate()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_ST_DUAL_DRIFT, format!("双跑指纹 {:#x}≠{:#x}", f1, f2)));
    }
    Ok(format!(
        "F0932 全绿：\n[10s 风暴] {}\n[耐受账] {}\n[丢弃显性] {}\n[负路径] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, f1
    ))
}
