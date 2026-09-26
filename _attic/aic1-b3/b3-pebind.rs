
// ---------------------------------------------------------------------------
// F003 · 深化批次三：任务管理器「导入解析：缓存命中/全量」显示面 + 内存回收
// 优先级钉值（缓存先于文件页）
//
// 主册依据（G-A-03【交互设计】）：「数据面在任务管理器（若 C-5 理解 A 拍板）
// 进程详情页显示『导入解析：缓存命中/全量』，给排查者看」；【状态与异常】
// 「内存压力下缓存先于文件页被回收」。BindTable 既有面（hits/misses/
// hit_rate_permille），本段只做消费面与回收次序钉值。
// ---------------------------------------------------------------------------

/// 解析模式判定线：命中率 ≥500‰ 记「缓存命中」模式，否则「全量」。
pub const RESOLVE_MODE_HIT_PERMILLE: u32 = 500;

/// 解析模式（任务管理器进程详情页显示面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolveMode {
    /// 缓存命中为主。
    CacheHit,
    /// 全量解析为主。
    FullResolve,
    /// 无样本（首启前）——显示面如实留空，不猜。
    NoSamples,
}

pub fn resolve_mode(hits: u64, misses: u64) -> ResolveMode {
    let total = hits + misses;
    if total == 0 {
        return ResolveMode::NoSamples;
    }
    let rate = (hits * 1000 / total) as u32;
    if rate >= RESOLVE_MODE_HIT_PERMILLE {
        ResolveMode::CacheHit
    } else {
        ResolveMode::FullResolve
    }
}

/// 显示短语（三要素之「发生了什么」——排查者可读）。
pub fn resolve_mode_label(mode: ResolveMode) -> &'static str {
    match mode {
        ResolveMode::CacheHit => "导入解析：缓存命中",
        ResolveMode::FullResolve => "导入解析：全量",
        ResolveMode::NoSamples => "导入解析：无样本",
    }
}

// 内存回收优先级（主册【状态与异常】钉死：缓存先于文件页被回收）。
pub const RECLAIM_ORDER_CACHE: u8 = 0;
pub const RECLAIM_ORDER_FILE_PAGES: u8 = 1;
pub const RECLAIM_ORDER_ANON: u8 = 2;

/// 全局回收次序（数值小者先被回收——缓存 0 < 文件页 1 < 匿名页 2）。
pub const RECLAIM_ORDER: [u8; 3] =
    [RECLAIM_ORDER_CACHE, RECLAIM_ORDER_FILE_PAGES, RECLAIM_ORDER_ANON];

/// F003 深化批次三自检。
pub fn run_pebind_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind-deep2");
    // 1) 模式判定：499/1000 命中 = 全量；500/1000 = 命中（恰达线）；零样本 = 无样本。
    cs.add(
        "resolve_mode_boundaries",
        resolve_mode(499, 501) == ResolveMode::FullResolve
            && resolve_mode(500, 500) == ResolveMode::CacheHit
            && resolve_mode(0, 0) == ResolveMode::NoSamples,
        "",
    );
    // 2) 显示短语三态非空可读（排查者面——不给裸数字）。
    cs.add(
        "resolve_mode_labels_readable",
        resolve_mode_label(ResolveMode::CacheHit) == "导入解析：缓存命中"
            && resolve_mode_label(ResolveMode::FullResolve) == "导入解析：全量"
            && resolve_mode_label(ResolveMode::NoSamples) == "导入解析：无样本",
        "",
    );
    // 3) 回收次序钉值：缓存先于文件页先于匿名页（主册【状态与异常】原文序）。
    cs.add(
        "reclaim_order_cache_first",
        RECLAIM_ORDER == [0, 1, 2]
            && RECLAIM_ORDER_CACHE < RECLAIM_ORDER_FILE_PAGES
            && RECLAIM_ORDER_FILE_PAGES < RECLAIM_ORDER_ANON,
        "",
    );
    cs
}
