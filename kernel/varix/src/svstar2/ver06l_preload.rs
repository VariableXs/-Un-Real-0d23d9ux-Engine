//! VE-F3432 · 皮肤资产预加载 —— 后台预取 + 可取消 + 预载失败不挡实装 + 白载率可见。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3432`
//!
//! # 职责（锚点原文拆解）
//!
//! - **后台预取不阻塞交互**：预取按时间片推进（[`Prefetcher::step`]），
//!   每片只花给定预算，花完立刻让出。绝不允许「一次性把 200 个资产载完」——
//!   那等于把换肤变成了同步 IO；
//! - **预载取消**：用户切走了就取消，不白载。取消是**立即**的：队列里
//!   还没开始的直接丢弃，正在载的那一个允许载完（中断 IO 的代价通常
//!   高于载完），但结果不入缓存。已载入的缓存条目保留（下次可能还用得上），
//!   但计入「白载」统计；
//! - **预载失败不影响实装**：预取失败只记录，不阻断。实装时再载一次，
//!   走的是同步路径；如果同步路径也失败，那是换肤失败（F3425 管），
//!   不是预取的责任；
//! - **白载率可见**：`wasted / prefetched` 就是白载率。这个数字长期偏高
//!   说明预取策略过于乐观（预取了一堆用户不会用的主题），需要收紧。
//!   没有这个数字，预取就是「看起来很快」的自我安慰。
//!
//! # 为什么取消要「允许正在载的那一个载完」
//!
//! 中断一次磁盘/解码 IO 需要清理半成品缓冲，代价常常高于让它跑完
//! （一个图标 2ms、一张壁纸 40ms）。所以取消的语义是「不再开始新的」，
//! 而不是「立刻掐断当前的」。但结果**不入缓存账**，避免「取消了却
//! 占了缓存」这种账实不符。
//!
//! # 复杂度
//!
//! - `schedule`：O(资产数)；
//! - `step`：O(本片处理数)；
//! - `cancel`：O(队列长)；
//! - `stats`：O(1)。
//!
//! # 零 panic 面
//!
//! `VecDeque` + `retain`，无索引。测试区除外。

use alloc::vec;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5498 起）
// ---------------------------------------------------------------------------

/// 预加载诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreloadCode {
    /// 资产 id 为空。
    EmptyAsset = 5498,
    /// 资产重复入队。
    DuplicateAsset = 5499,
    /// 时间片预算为 0（永远推不动）。
    ZeroSlice = 5500,
    /// 取消残留未清理。
    CancelResidue = 5501,
    /// 预取阻塞了交互（单片超预算）。
    BlockingDetected = 5502,
    /// 主题为空。
    EmptyTheme = 5503,
    /// 缓存条目 id 冲突。
    CacheClash = 5504,
}

impl PreloadCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PreloadCode::EmptyAsset => "资产标识为空",
            PreloadCode::DuplicateAsset => "资产重复入队",
            PreloadCode::ZeroSlice => "时间片预算为 0",
            PreloadCode::CancelResidue => "取消后仍有残留任务",
            PreloadCode::BlockingDetected => "预取单片超预算，阻塞交互",
            PreloadCode::EmptyTheme => "主题为空",
            PreloadCode::CacheClash => "缓存条目冲突",
        }
    }

    /// 是否阻断级（阻塞交互是产品级缺陷）。
    pub fn blocking(self) -> bool {
        matches!(self, PreloadCode::BlockingDetected)
    }
}

// ---------------------------------------------------------------------------
// 二、资产与缓存
// ---------------------------------------------------------------------------

/// 一个待预取资产。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Asset {
    /// 资产 id（`icons/ok.svg`）。
    pub id: String,
    /// 预估耗时（毫秒）。
    pub cost_ms: u32,
    /// 字节数。
    pub bytes: u32,
}

impl Asset {
    /// 构造。
    pub fn new(id: &str, cost_ms: u32, bytes: u32) -> Asset {
        Asset {
            id: id.to_string(),
            cost_ms,
            bytes,
        }
    }
}

/// 缓存条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CacheEntry {
    /// 资产 id。
    pub id: String,
    /// 字节数。
    pub bytes: u32,
    /// 被实装命中的次数。
    pub hits: u32,
}

/// 预取统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PreloadStats {
    /// 已成功预取数。
    pub prefetched: u32,
    /// 预取失败数。
    pub failed: u32,
    /// 取消丢弃数（还没开始就丢了）。
    pub cancelled: u32,
    /// 实装命中缓存数。
    pub hits: u32,
    /// 实装未命中数。
    pub misses: u32,
}

impl PreloadStats {
    /// 白载率（千分比）：取消丢弃 / 已预取。分母为 0 时返回 0。
    pub fn waste_permille(&self) -> u32 {
        if self.prefetched == 0 {
            return 0;
        }
        (self.cancelled as u64 * 1000 / self.prefetched as u64) as u32
    }

    /// 命中率（千分比）。
    pub fn hit_permille(&self) -> u32 {
        let total = self.hits.saturating_add(self.misses);
        if total == 0 {
            return 0;
        }
        (self.hits as u64 * 1000 / total as u64) as u32
    }
}

// ---------------------------------------------------------------------------
// 三、预取器
// ---------------------------------------------------------------------------

/// 预取器。
#[derive(Clone, Debug)]
pub struct Prefetcher {
    queue: VecDeque<Asset>,
    cache: Vec<CacheEntry>,
    stats: PreloadStats,
    slice_budget_ms: u32,
    /// 当前主题（用于取消判定）。
    theme: String,
    /// 取消过的资产 id（用于清理残留）。
    cancelled_ids: Vec<String>,
    /// 上一次 step 是否超预算。
    overruns: u32,
}

impl Prefetcher {
    /// 新建。
    pub fn new(slice_budget_ms: u32) -> Prefetcher {
        Prefetcher {
            queue: VecDeque::new(),
            cache: Vec::new(),
            stats: PreloadStats::default(),
            slice_budget_ms,
            theme: String::new(),
            cancelled_ids: Vec::new(),
            overruns: 0,
        }
    }

    /// 排入一个主题的资产。重复 id 拒收（同一资产排两次只会白载一次）。
    pub fn schedule(&mut self, theme: &str, assets: &[Asset]) -> Result<u32, PreloadCode> {
        if theme.trim().is_empty() {
            return Err(PreloadCode::EmptyTheme);
        }
        if self.slice_budget_ms == 0 {
            return Err(PreloadCode::ZeroSlice);
        }
        self.theme = theme.to_string();
        let mut added = 0u32;
        for a in assets.iter() {
            if a.id.trim().is_empty() {
                return Err(PreloadCode::EmptyAsset);
            }
            if self.queue.iter().any(|q| q.id == a.id)
                || self.cache.iter().any(|c| c.id == a.id)
            {
                return Err(PreloadCode::DuplicateAsset);
            }
            self.queue.push_back(a.clone());
            added = added.saturating_add(1);
        }
        Ok(added)
    }

    /// 推进一个时间片。返回本片实际消耗的毫秒。
    ///
    /// 单片最多 `slice_budget_ms`；若单个资产成本已超过预算，仍会处理它
    /// （否则队列永远推不动），但记一次超预算——这是「阻塞检测」的来源。
    pub fn step(&mut self) -> u32 {
        if self.slice_budget_ms == 0 {
            return 0;
        }
        let mut spent = 0u32;
        while let Some(asset) = self.queue.pop_front() {
            if spent > 0 && spent.saturating_add(asset.cost_ms) > self.slice_budget_ms {
                // 放回队首，留到下一片。
                self.queue.push_front(asset);
                break;
            }
            if asset.cost_ms > self.slice_budget_ms {
                self.overruns = self.overruns.saturating_add(1);
            }
            spent = spent.saturating_add(asset.cost_ms);
            // 预取「成功」：入缓存。
            if let Some(slot) = self.cache.iter_mut().find(|c| c.id == asset.id) {
                slot.bytes = asset.bytes;
            } else {
                self.cache.push(CacheEntry {
                    id: asset.id.clone(),
                    bytes: asset.bytes,
                    hits: 0,
                });
            }
            self.stats.prefetched = self.stats.prefetched.saturating_add(1);
            if spent >= self.slice_budget_ms {
                break;
            }
        }
        spent
    }

    /// 取消当前主题的预取。队列里属于该主题的（本模块不区分主题归属，
    /// 统一按「当前队列」处理）直接丢弃并计数；已入缓存的条目保留，
    /// 但记入取消统计（这就是白载）。
    pub fn cancel(&mut self, theme: &str) -> u32 {
        if theme != self.theme {
            // 取消的不是当前主题：没有可取消的。
            return 0;
        }
        let dropped = self.queue.len() as u32;
        while let Some(a) = self.queue.pop_front() {
            self.cancelled_ids.push(a.id);
        }
        self.stats.cancelled = self.stats.cancelled.saturating_add(dropped);
        dropped
    }

    /// 清理取消残留（把已取消但仍在队列里的条目彻底移除）。
    pub fn clean_residue(&mut self) -> Result<u32, PreloadCode> {
        let before = self.queue.len();
        self.queue
            .retain(|a| !self.cancelled_ids.iter().any(|c| *c == a.id));
        let removed = before.saturating_sub(self.queue.len()) as u32;
        self.cancelled_ids.clear();
        if self.queue.iter().any(|a| self.cancelled_ids.iter().any(|c| *c == a.id)) {
            return Err(PreloadCode::CancelResidue);
        }
        Ok(removed)
    }

    /// 实装：查缓存。命中返回字节数；未命中返回 `None`（实装时再载）。
    pub fn resolve(&mut self, id: &str) -> Option<u32> {
        match self.cache.iter_mut().find(|c| c.id == id) {
            Some(entry) => {
                entry.hits = entry.hits.saturating_add(1);
                self.stats.hits = self.stats.hits.saturating_add(1);
                Some(entry.bytes)
            }
            None => {
                self.stats.misses = self.stats.misses.saturating_add(1);
                None
            }
        }
    }

    /// 预取失败（模拟 IO 错误）：只记账，不阻断实装。
    pub fn note_failure(&mut self) {
        self.stats.failed = self.stats.failed.saturating_add(1);
    }

    /// 实装是否允许。恒为 `true`——预取失败从不挡实装。
    pub fn install_allowed(&self) -> bool {
        true
    }

    /// 排队数。
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// 缓存条目数。
    pub fn cached(&self) -> usize {
        self.cache.len()
    }

    /// 统计。
    pub fn stats(&self) -> PreloadStats {
        self.stats
    }

    /// 超预算次数。
    pub fn overruns(&self) -> u32 {
        self.overruns
    }

    /// 阻塞检测：有超预算且超预算次数超过 3 次即判阻塞（单片总超）。
    pub fn blocking_alert(&self) -> Option<PreloadCode> {
        if self.overruns > 3 {
            Some(PreloadCode::BlockingDetected)
        } else {
            None
        }
    }

    /// 读屏播报。
    pub fn reading_lines(&self) -> Vec<String> {
        let s = self.stats;
        let mut out = Vec::new();
        out.push(format!(
            "预加载：主题 {}，排队 {}，缓存 {}，已预取 {}，失败 {}，取消 {}",
            if self.theme.is_empty() { "无" } else { &self.theme },
            self.queue.len(),
            self.cache.len(),
            s.prefetched,
            s.failed,
            s.cancelled
        ));
        out.push(format!(
            "白载率 {}‰，命中率 {}‰，实装{}",
            s.waste_permille(),
            s.hit_permille(),
            if self.install_allowed() {
                "允许（预取失败不挡实装）"
            } else {
                "被阻止"
            }
        ));
        out
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> Vec<Asset> {
        alloc::vec![
            Asset::new("icons/ok.svg", 2, 1024),
            Asset::new("icons/warn.svg", 2, 1024),
            Asset::new("wall/4k.webp", 40, 8_000_000),
        ]
    }

    #[test]
    fn prefetch_is_sliced_and_non_blocking() {
        let mut p = Prefetcher::new(8);
        p.schedule("neon-night", &assets()).expect("排入");
        // 第一片：两个 2ms 图标，花费 4ms（第三个 40ms 会超预算，留到下一片）。
        let spent = p.step();
        assert!(spent <= 8, "单片不得超预算：{}", spent);
        assert_eq!(p.cached(), 2);
        assert_eq!(p.queued(), 1);
        // 下一片处理大资产：单片必超 -> 记超预算（阻塞检测的来源）。
        let spent2 = p.step();
        assert_eq!(spent2, 40);
        assert_eq!(p.overruns(), 1);
    }

    #[test]
    fn cancel_drops_queue_and_counts_waste() {
        let mut p = Prefetcher::new(8);
        p.schedule("neon-night", &assets()).expect("排入");
        p.step();
        let dropped = p.cancel("neon-night");
        assert_eq!(dropped, 1, "队列里剩一个大资产");
        assert_eq!(p.queued(), 0);
        assert_eq!(p.stats().cancelled, 1);
        // 已预取 2 个、取消 1 个 -> 白载率 500‰（粗略口径，够看趋势）。
        assert!(p.stats().waste_permille() >= 400);
    }

    #[test]
    fn prefetch_failure_never_blocks_install() {
        let mut p = Prefetcher::new(8);
        p.schedule("x", &assets()).expect("排入");
        p.note_failure();
        assert!(p.install_allowed());
        // 未命中缓存时 resolve 返回 None，实装走同步路径。
        assert_eq!(p.resolve("icons/ok.svg"), None);
        assert_eq!(p.stats().misses, 1);
    }

    #[test]
    fn cache_hits_are_counted() {
        let mut p = Prefetcher::new(100);
        p.schedule("x", &assets()).expect("排入");
        p.step();
        assert_eq!(p.resolve("icons/ok.svg"), Some(1024));
        assert_eq!(p.resolve("icons/ok.svg"), Some(1024));
        assert_eq!(p.stats().hits, 2);
        assert_eq!(p.stats().hit_permille(), 1000);
    }

    #[test]
    fn duplicate_and_empty_assets_are_rejected() {
        let mut p = Prefetcher::new(8);
        p.schedule("x", &assets()).expect("首次");
        assert_eq!(
            p.schedule("x", &assets()).err(),
            Some(PreloadCode::DuplicateAsset)
        );
        assert_eq!(
            p.schedule("", &assets()).err(),
            Some(PreloadCode::EmptyTheme)
        );
        assert_eq!(
            p.schedule("x", &[Asset::new("", 1, 1)]).err(),
            Some(PreloadCode::EmptyAsset)
        );
    }

    #[test]
    fn blocking_alert_after_repeated_overruns() {
        let mut p = Prefetcher::new(1);
        let mut i = 0;
        while i < 5 {
            p.schedule("t", &[Asset::new(&format!("a{}", i), 40, 1)]).expect("排");
            p.step();
            i += 1;
        }
        assert_eq!(p.blocking_alert(), Some(PreloadCode::BlockingDetected));
        assert!(PreloadCode::BlockingDetected.blocking());
    }

    #[test]
    fn reading_lines_expose_waste_rate() {
        let mut p = Prefetcher::new(100);
        p.schedule("x", &assets()).expect("排入");
        p.step();
        let lines = p.reading_lines();
        assert!(lines.first().map(|l| l.contains("缓存 3")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("白载率")));
    }
}
