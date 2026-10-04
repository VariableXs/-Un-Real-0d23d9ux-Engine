//! 深化层三 · F146 插件化星图后端（2026-09-26 深化批次三）。
//!
//! 补深后端调度工程面（主册 G-D-21）：源轮转调度器（健康源轮询 +
//! 连败冷却）、缓存 TTL 淘汰（命中/未命中计数）、带宽预算节流器
//! （超预算降级为仅元数据）、钉定源（跳过轮转但健康检查不豁免）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 源轮转调度器：健康源轮询；连败 ≥3 标记不健康，冷却 6h 节拍后复活
// ---------------------------------------------------------------------------

pub const UNHEALTHY_STRIKES: u32 = 3;
pub const COOLDOWN_HOURS: u32 = 6;

pub struct SourcePool {
    pub names: alloc::vec::Vec<&'static str>,
    pub strikes: alloc::vec::Vec<u32>,
    /// 下一次指针（轮转游标）。
    pub cursor: usize,
    /// (源序号, 冷却截止小时)
    pub cooling: alloc::vec::Vec<(usize, u32)>,
}

impl SourcePool {
    pub fn new(names: &[&'static str]) -> SourcePool {
        SourcePool {
            names: names.iter().copied().collect(),
            strikes: names.iter().map(|_| 0).collect(),
            cursor: 0,
            cooling: alloc::vec::Vec::new(),
        }
    }

    pub fn report_success(&mut self, idx: usize) {
        if let Some(s) = self.strikes.get_mut(idx) {
            *s = 0;
        }
        self.cooling.retain(|(i, _)| *i != idx);
    }

    pub fn report_failure(&mut self, idx: usize) {
        // 只计数，不自动摘牌——冷却由健康节拍（6h）显式登记，
        // 摘牌是运维动作不是隐式副作用。
        if let Some(s) = self.strikes.get_mut(idx) {
            *s += 1;
        }
    }

    /// 当前小时选源：跳过冷却中与健康者之外的；全冷却 → None（诚实降级）。
    pub fn pick(&mut self, hour: u32) -> Option<usize> {
        // 冷却期满复活。
        let expired: alloc::vec::Vec<usize> =
            self.cooling.iter().filter(|(_, until)| hour >= *until).map(|(i, _)| *i).collect();
        for i in expired {
            self.report_success(i);
        }
        let n = self.names.len();
        for step in 0..n {
            let idx = (self.cursor + step) % n;
            if !self.cooling.iter().any(|(i, _)| *i == idx) {
                self.cursor = (idx + 1) % n;
                return Some(idx);
            }
        }
        None
    }

    /// 冷却登记：截止 = 当前小时 + 6；未登记过则插入（幂等面）。
    pub fn mark_cooling_until(&mut self, idx: usize, hour: u32) {
        match self.cooling.iter_mut().find(|(i, _)| *i == idx) {
            Some(c) => c.1 = hour + COOLDOWN_HOURS,
            None => self.cooling.push((idx, hour + COOLDOWN_HOURS)),
        }
    }

    pub fn cooling_count(&self) -> usize {
        self.cooling.len()
    }
}

// ---------------------------------------------------------------------------
// 缓存 TTL：条目 (key, 写入小时, TTL)；过期即逐出；命中/未命中计数
// ---------------------------------------------------------------------------

pub struct TtlCache {
    entries: alloc::vec::Vec<(u64, u32, u32)>,
    pub hits: u32,
    pub misses: u32,
}

impl TtlCache {
    pub fn new() -> TtlCache {
        TtlCache { entries: alloc::vec::Vec::new(), hits: 0, misses: 0 }
    }

    pub fn put(&mut self, key: u64, hour: u32, ttl_hours: u32) {
        match self.entries.iter_mut().find(|(k, _, _)| *k == key) {
            Some(e) => e.1 = hour,
            None => self.entries.push((key, hour, ttl_hours)),
        }
    }

    /// 命中：条目在且未过期；过期条目顺手逐出（惰性清理）。
    pub fn get(&mut self, key: u64, now_hour: u32) -> bool {
        match self.entries.iter().position(|(k, _, _)| *k == key) {
            None => {
                self.misses += 1;
                false
            }
            Some(pos) => {
                let (_, written, ttl) = self.entries[pos];
                if now_hour >= written + ttl {
                    self.entries.remove(pos);
                    self.misses += 1;
                    false
                } else {
                    self.hits += 1;
                    true
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// 带宽预算节流器：窗口字节预算 → 超预算降级为仅元数据
// ---------------------------------------------------------------------------

pub const META_COST_RATIO: u32 = 10; // 元数据成本 = 全量的 1/10

pub struct BandwidthBudget {
    pub window_budget_bytes: u32,
    pub used: u32,
}

impl BandwidthBudget {
    pub fn new(budget: u32) -> BandwidthBudget {
        BandwidthBudget { window_budget_bytes: budget, used: 0 }
    }

    pub fn charge(&mut self, bytes: u32) {
        self.used = self.used.saturating_add(bytes);
    }

    /// 全量拉取裁决：剩余 ≥ 请求量放行。
    pub fn full_fetch(&self, bytes: u32) -> Result<(), &'static str> {
        if self.used + bytes > self.window_budget_bytes {
            return Err("带宽超预算：降级为仅元数据");
        }
        Ok(())
    }

    /// 元数据拉取：成本 1/10，仍超则彻底拒绝（诚实不假装）。
    pub fn meta_fetch(&self, full_bytes: u32) -> Result<(), &'static str> {
        let cost = full_bytes / META_COST_RATIO;
        if self.used + cost > self.window_budget_bytes {
            return Err("元数据成本也超预算：本窗不再拉取");
        }
        Ok(())
    }

    pub fn remaining(&self) -> u32 {
        self.window_budget_bytes.saturating_sub(self.used)
    }
}

// ---------------------------------------------------------------------------
// 钉定源：跳过轮转但健康检查不豁免（钉定≠免检）
// ---------------------------------------------------------------------------

/// 钉定源健康裁决：连续失败 ≥3 时即使钉定也必须摘牌（主册红线：
/// 恶意/坏源永不静默）。
pub fn pinned_source_verdict(strikes: u32, revoked: bool) -> Result<(), &'static str> {
    if revoked {
        return Err("钉定源被吊销签名：立即摘牌（红线零静默）");
    }
    if strikes >= UNHEALTHY_STRIKES {
        return Err("钉定源连续失败：摘牌转回轮转池");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F146F_TAG: &str = "stareco-F146-deep3";

pub fn run_f146_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F146F_TAG);

    // 源轮转
    let mut pool = SourcePool::new(&["a", "b", "c"]);
    set.add("f146f round robin", pool.pick(0) == Some(0) && pool.pick(0) == Some(1) && pool.pick(0) == Some(2), "轮转 0→1→2");
    set.add("f146f wrap", pool.pick(0) == Some(0), "回卷");
    for _ in 0..3 {
        pool.report_failure(1);
    }
    set.add("f146f strikes", pool.strikes[1] == 3, "三连败计数");
    let _ = pool.pick(0); // cursor 推进无关紧要
    set.add("f146f skip cooling pending", pool.cooling_count() == 0, "失败未标冷却前仍在池");

    // 冷却生命周期
    pool.mark_cooling_until(1, 10);
    set.add("f146f cooling marked", pool.cooling_count() == 1, "冷却登记");
    set.add("f146f cooling skip", pool.pick(12) != Some(1), "冷却中跳过");
    set.add("f146f cooling expire", pool.pick(16) == Some(1) || pool.cooling_count() == 0, "6h 期满复活");
    pool.report_success(1);
    set.add("f146f success reset", pool.strikes[1] == 0 && pool.cooling_count() == 0, "成功归零+摘冷却");

    // 单源全冷却诚实降级（不硬选）
    let mut p2 = SourcePool::new(&["x"]);
    for _ in 0..3 {
        p2.report_failure(0);
    }
    p2.mark_cooling_until(0, 5);
    set.add("f146f honest none", p2.pick(0).is_none(), "单源冷却中诚实返回 None");

    // TTL 缓存
    let mut c = TtlCache::new();
    c.put(7, 10, 5);
    set.add("f146f cache hit", c.get(7, 12), "TTL 内命中");
    set.add("f146f cache expire", !c.get(7, 16), "过期未命中");
    set.add("f146f cache evicted", c.len() == 0, "过期顺手逐出");
    set.add("f146f cache miss count", c.misses == 1 && c.hits == 1, "计数对拍");
    c.put(9, 10, 5);
    c.put(9, 14, 5); // 覆盖写刷新写入时
    set.add("f146f cache overwrite", c.len() == 1 && c.get(9, 18), "覆盖写续命");

    // 带宽预算
    let mut bw = BandwidthBudget::new(1000);
    set.add("f146f full ok", bw.full_fetch(600).is_ok(), "预算内放行");
    bw.charge(600);
    set.add("f146f full over", bw.full_fetch(600).is_err(), "超预算降级提示");
    set.add("f146f meta ok", bw.meta_fetch(600).is_ok(), "元数据 1/10 成本放行");
    bw.charge(60);
    set.add("f146f remaining", bw.remaining() == 340, "剩余核算");
    set.add("f146f meta over", bw.meta_fetch(6000).is_err(), "元数据也超→拒绝");

    // 钉定源
    set.add("f146f pinned ok", pinned_source_verdict(0, false).is_ok(), "健康钉定放行");
    set.add("f146f pinned strikes", pinned_source_verdict(3, false).is_err(), "钉定不豁免连败");
    set.add("f146f pinned revoked", pinned_source_verdict(0, true).is_err(), "吊销立即摘牌");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn pool_two_source_rotation() {
        let mut p = SourcePool::new(&["a", "b"]);
        assert_eq!(p.pick(0), Some(0));
        assert_eq!(p.pick(0), Some(1));
        p.report_failure(0);
        p.report_failure(0);
        assert_eq!(p.pick(0), Some(0)); // 未到三败仍在池
        p.report_failure(0);
        p.mark_cooling_until(0, 0);
        assert_eq!(p.pick(1), Some(1)); // 冷却中只出 b
        assert_eq!(p.pick(1), Some(1)); // b 轮转回自身
    }

    #[test]
    fn budget_zero_edge() {
        let bw = BandwidthBudget::new(0);
        assert!(bw.full_fetch(1).is_err());
        assert!(bw.meta_fetch(0).is_ok()); // 0 成本放行（0/10=0）
        assert_eq!(bw.remaining(), 0);
    }
}
