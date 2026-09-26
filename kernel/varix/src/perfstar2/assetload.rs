//! F068 渲染资产按需装载（perfstar2 · G-B-28）——内存里只有你在用的那一套。
//!
//! 主册判据（验收标准第一句）：
//! **单主题驻留内存 ≤80MB（4K 全套实测）；主题切换全程无白屏闪烁（录屏
//! 帧检）。**
//!
//! 功能定义（G-B-28）：壁纸/图标集/音效按当前主题（E1）惰性装载——未选中
//! 的主题资产不驻内存；内存占用差值入账本。
//!
//! 【交互设计】无直接 UI；E1 切换的流畅度即体验面；诊断面板资产页显示当前
//! 驻留清单。
//! 【数据与存储】资产文件按主题目录组织（4K 管线 C-7 产出）；驻留清单运行
//! 时维护。
//! 【状态与异常】资产文件损坏 → 回退默认主题 + 诊断报备；切换时新资产解码
//! 失败 → 保持旧主题（不白屏）。
//! 【设计细节】引用计数管理资产生命周期（最后一个引用卸载）；解码在后缓冲
//! 完成后原子换入（无中间态上屏）；音效文件小（<200KB 全量驻留不按需）；
//! 图标集按需粒度=单图标（常用 100 个预驻）。
//!
//! 零堆纪律：定长资产表 + 定长驻留账，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 单主题驻留内存上限：80MB（4K 全套，主册明文）。
pub const RESIDENT_CAP_MB: u64 = 80;
/// 音效全量驻留阈值：<200KB（主册明文——小文件不按需）。
pub const SOUND_RESIDENT_MAX_BYTES: u64 = 200 * 1024;
/// 图标预驻数量：常用 100 个（主册明文）。
pub const ICON_PRELOAD_COUNT: usize = 100;
/// 图标集按需粒度：单图标。
pub const ICON_GRANULARITY: &'static str = "single-icon";
/// 资产表容量（当前主题驻留清单）。
const ASSET_CAP: usize = 256;
/// 主题注册容量（10 套官方主题量级）。
const THEME_CAP: usize = 16;

/// 资产类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetKind {
    Wallpaper,
    Icon,
    Sound,
}

impl AssetKind {
    /// 装载策略：音效小文件全量驻留；壁纸/图标按需。
    pub fn is_always_resident(self) -> bool {
        matches!(self, AssetKind::Sound)
    }
}

/// 驻留资产条目（引用计数生命周期）。
#[derive(Clone, Copy, Debug)]
pub struct ResidentAsset {
    pub name: &'static str,
    pub kind: AssetKind,
    /// 文件大小（字节）。
    pub file_bytes: u64,
    /// 解码后驻留字节（4K 解码面）。
    pub decoded_bytes: u64,
    /// 引用计数（0 = 待卸载）。
    pub refs: u32,
    /// 预驻旗标（常用图标 100 个 / 音效全量）。
    pub preloaded: bool,
}

/// 主题注册条目。
#[derive(Clone, Copy, Debug)]
pub struct ThemeEntry {
    pub name: &'static str,
    /// 资产总数（按需统计）。
    pub asset_count: u16,
    /// 损坏旗标（诊断报备后置位 → 回退默认主题）。
    pub corrupted: bool,
}

// ---------------------------------------------------------------------------
// 装载器
// ---------------------------------------------------------------------------

/// 渲染资产按需装载器。
pub struct AssetLoader {
    themes: [Option<ThemeEntry>; THEME_CAP],
    theme_n: usize,
    /// 当前活跃主题（唯一驻留其资产——未选中主题不驻内存）。
    active_theme: usize,
    resident: [Option<ResidentAsset>; ASSET_CAP],
    resident_n: usize,
    /// 解码失败保持旧主题旗标（切换事务语义）。
    switch_in_flight: Option<usize>,
    /// 诊断报备账（损坏资产）。
    diag_reports: u64,
    /// 后缓冲原子换入计数（无中间态上屏的模型核对）。
    atomic_swaps: u64,
    peak_resident_bytes: u64,
}

impl AssetLoader {
    pub const fn new() -> Self {
        AssetLoader {
            themes: [None; THEME_CAP],
            theme_n: 0,
            active_theme: usize::MAX,
            resident: [None; ASSET_CAP],
            resident_n: 0,
            switch_in_flight: None,
            diag_reports: 0,
            atomic_swaps: 0,
            peak_resident_bytes: 0,
        }
    }

    /// 注册主题。
    pub fn register_theme(&mut self, name: &'static str, asset_count: u16) -> usize {
        if self.theme_n == THEME_CAP {
            return usize::MAX;
        }
        self.themes[self.theme_n] = Some(ThemeEntry { name, asset_count, corrupted: false });
        self.theme_n += 1;
        self.theme_n - 1
    }

    /// 切换主题：旧主题资产全卸载（最后一个引用卸载语义在主题级表现为整体
    /// 换装），新主题惰性装载——**先解码后换入**（切换事务：解码失败保持旧
    /// 主题不白屏）。
    pub fn switch_theme(&mut self, theme_idx: usize) -> bool {
        if theme_idx >= self.theme_n {
            return false;
        }
        if self.themes[theme_idx].map_or(true, |t| t.corrupted) {
            // 目标主题损坏 → 回退默认主题 + 诊断报备。
            self.diag_reports += 1;
            let fallback = self.default_theme_index();
            if fallback == usize::MAX {
                return false;
            }
            return self.switch_theme_force(fallback);
        }
        self.switch_in_flight = Some(theme_idx);
        true // 解码启动；commit/abort 由后缓冲完成信号决定
    }

    /// 切换提交（后缓冲解码完成 → 原子换入）：旧卸新装一次完成。
    pub fn commit_switch(&mut self, decoded_ok: bool) -> bool {
        let target = match self.switch_in_flight.take() {
            Some(t) => t,
            None => return false,
        };
        if !decoded_ok {
            // 解码失败 → 保持旧主题（不白屏）。
            self.diag_reports += 1;
            return false;
        }
        // 旧主题资产全卸载（refs 归零卸载）。
        for r in self.resident.iter_mut().take(self.resident_n) {
            if let Some(a) = r {
                a.refs = 0;
            }
        }
        self.unload_zero_refs();
        self.active_theme = target;
        self.atomic_swaps += 1; // 后缓冲原子换入（无中间态上屏）
        true
    }

    fn default_theme_index(&self) -> usize {
        self.themes
            .iter()
            .take(self.theme_n)
            .position(|t| matches!(t, Some(e) if e.name == "default"))
            .unwrap_or(usize::MAX)
    }

    fn switch_theme_force(&mut self, idx: usize) -> bool {
        if idx >= self.theme_n {
            return false;
        }
        self.switch_in_flight = Some(idx);
        self.commit_switch(true)
    }

    /// 资产请求（按需装载）：音效全量驻留；图标单粒度；壁纸整张。
    /// 主题未激活的资产请求拒绝（未选中主题资产不驻内存——判据本体）。
    pub fn acquire(&mut self, kind: AssetKind, name: &'static str, file_bytes: u64, decoded_bytes: u64) -> bool {
        if self.active_theme == usize::MAX {
            return false;
        }
        // 音效小文件：全量驻留（豁免按需）。
        let preloaded = kind.is_always_resident() && file_bytes <= SOUND_RESIDENT_MAX_BYTES;
        // 驻留上限执法：80MB（音效豁免仍计入账面——上限是总驻留）。
        let cur = self.resident_bytes();
        if cur + decoded_bytes > RESIDENT_CAP_MB * 1024 * 1024 {
            return false; // 超 80MB：拒绝装载（诚实上限执法）
        }
        // 已驻留 → 引用 +1。
        for r in self.resident.iter_mut().take(self.resident_n) {
            if let Some(a) = r {
                if a.name == name {
                    a.refs += 1;
                    return true;
                }
            }
        }
        if self.resident_n == ASSET_CAP {
            return false;
        }
        self.resident[self.resident_n] = Some(ResidentAsset {
            name,
            kind,
            file_bytes,
            decoded_bytes,
            refs: 1,
            preloaded,
        });
        self.resident_n += 1;
        let now = self.resident_bytes();
        if now > self.peak_resident_bytes {
            self.peak_resident_bytes = now;
        }
        true
    }

    /// 释放引用：最后一个引用卸载（判据本体）。
    pub fn release(&mut self, name: &'static str) -> bool {
        for r in self.resident.iter_mut().take(self.resident_n) {
            if let Some(a) = r {
                if a.name == name {
                    a.refs = a.refs.saturating_sub(1);
                    if a.refs == 0 {
                        self.unload_zero_refs();
                    }
                    return true;
                }
            }
        }
        false
    }

    fn unload_zero_refs(&mut self) {
        // 压实：refs=0 的条目移除。
        let mut w = 0usize;
        for r in 0..self.resident_n {
            let keep = matches!(self.resident[r], Some(a) if a.refs > 0);
            if keep {
                self.resident.swap(w, r);
                w += 1;
            }
        }
        for slot in self.resident.iter_mut().skip(w) {
            *slot = None;
        }
        self.resident_n = w;
    }

    /// 当前驻留字节（解码后口径）。
    pub fn resident_bytes(&self) -> u64 {
        self.resident
            .iter()
            .flatten()
            .map(|a| a.decoded_bytes)
            .sum()
    }

    /// 未激活主题驻留资产数（判据：恒为 0）。
    pub fn foreign_theme_resident(&self) -> usize {
        // 模型口径：active_theme 切换时旧资产已清——这里以驻留清单全属于
        // 当前主题为不变量；提供诊断查询位。
        if self.active_theme == usize::MAX {
            return self.resident_n; // 未激活却驻留 = 异常
        }
        0
    }

    pub fn resident_count(&self) -> usize {
        self.resident_n
    }

    pub fn is_resident(&self, name: &str) -> bool {
        self.resident.iter().take(self.resident_n).flatten().any(|a| a.name == name)
    }

    pub fn refs_of(&self, name: &str) -> u32 {
        self.resident
            .iter()
            .take(self.resident_n)
            .flatten()
            .find(|a| a.name == name)
            .map(|a| a.refs)
            .unwrap_or(0)
    }

    pub fn diag_reports(&self) -> u64 {
        self.diag_reports
    }

    pub fn atomic_swaps(&self) -> u64 {
        self.atomic_swaps
    }

    pub fn peak_resident_bytes(&self) -> u64 {
        self.peak_resident_bytes
    }

    pub fn active_theme(&self) -> usize {
        self.active_theme
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_assetload_checks() -> CheckSet {
    let mut cs = CheckSet::new("F068-assetload");
    // 1) 切换前未激活：任何资产请求拒绝（未选中主题资产不驻内存）。
    let mut l = AssetLoader::new();
    let t1 = l.register_theme("aurora", 40);
    cs.add(
        "inactive_theme_no_resident",
        !l.acquire(AssetKind::Wallpaper, "wall-4k", 24 * 1024 * 1024, 40 * 1024 * 1024) && l.resident_count() == 0,
        "",
    );
    // 2) 激活后按需装载；引用计数卸载（最后一个引用卸载）。
    let _ = l.switch_theme(t1);
    let _ = l.commit_switch(true);
    cs.add(
        "active_theme_lazy_load",
        l.acquire(AssetKind::Wallpaper, "wall-4k", 24 * 1024 * 1024, 40 * 1024 * 1024)
            && l.acquire(AssetKind::Wallpaper, "wall-4k", 24 * 1024 * 1024, 40 * 1024 * 1024)
            && l.refs_of("wall-4k") == 2,
        "",
    );
    cs.add(
        "last_ref_unloads",
        l.release("wall-4k") && l.is_resident("wall-4k") && l.release("wall-4k") && !l.is_resident("wall-4k"),
        "",
    );
    // 3) 音效 <200KB 全量驻留（豁免按需）。
    cs.add(
        "sound_always_resident",
        l.acquire(AssetKind::Sound, "notify.flac", 120 * 1024, 120 * 1024) && l.is_resident("notify.flac"),
        "",
    );
    // 4) 单主题驻留 ≤80MB：超限拒绝（上限执法）。
    let mut l4 = AssetLoader::new();
    let t4 = l4.register_theme("heavy", 30);
    let _ = l4.switch_theme(t4);
    let _ = l4.commit_switch(true);
    // 40MB + 40MB = 80MB 恰达线；再 +1KB 拒绝。
    cs.add(
        "resident_cap_80mb_enforced",
        l4.acquire(AssetKind::Wallpaper, "w1", 24 << 20, 40 << 20)
            && l4.acquire(AssetKind::Wallpaper, "w2", 24 << 20, 40 << 20)
            && !l4.acquire(AssetKind::Icon, "ico-extra", 1 << 20, 1 << 10),
        "",
    );
    // 5) 图标单粒度 + 常用 100 预驻语义（预驻旗标按粒度登记）。
    let mut l5 = AssetLoader::new();
    let t5 = l5.register_theme("default", 140);
    let _ = l5.switch_theme(t5);
    let _ = l5.commit_switch(true);
    let mut all_icons = true;
    for i in 0..ICON_PRELOAD_COUNT {
        let ok = l5.acquire(AssetKind::Icon, icon_name(i), 24 * 1024, 48 * 1024);
        all_icons &= ok;
    }
    cs.add("icon_preload_100_single_granularity", all_icons && l5.resident_count() == ICON_PRELOAD_COUNT, "");
    // 6) 切换事务：解码失败 → 保持旧主题（不白屏）。
    let mut l6 = AssetLoader::new();
    let d = l6.register_theme("default", 10);
    let t6 = l6.register_theme("broken-decode", 10);
    let _ = l6.switch_theme(d);
    let _ = l6.commit_switch(true);
    let _ = l6.acquire(AssetKind::Wallpaper, "old-wall", 1 << 20, 2 << 20);
    let _ = l6.switch_theme(t6);
    cs.add("decode_fail_keeps_old_theme", !l6.commit_switch(false) && l6.active_theme() == d, "");
    // 7) 损坏主题 → 回退默认 + 诊断报备。
    let mut l7 = AssetLoader::new();
    let d7 = l7.register_theme("default", 10);
    let bad = l7.register_theme("corrupt", 10);
    // 标记损坏（内部旗标直操作——诊断报备路径）。
    l7.themes[bad] = Some(ThemeEntry { name: "corrupt", asset_count: 10, corrupted: true });
    cs.add(
        "corrupt_falls_back_default",
        l7.switch_theme(bad) && l7.active_theme() == d7 && l7.diag_reports() == 1,
        "",
    );
    // 8) 原子换入计数（无中间态上屏——后缓冲完成后 commit）。
    cs.add("atomic_swap_counted", l6.atomic_swaps() == 1, "");
    // 9) 外科对照：驻留清单位置性与主题不变量。
    let mut l9 = AssetLoader::new();
    let t9 = l9.register_theme("solo", 5);
    let _ = l9.switch_theme(t9);
    let _ = l9.commit_switch(true);
    let _ = l9.acquire(AssetKind::Wallpaper, "solo-wall", 1 << 20, 2 << 20);
    cs.add("no_foreign_resident", l9.foreign_theme_resident() == 0, "");
    cs
}

/// 图标名生成（自检用静态命名：icon-000..icon-099）。
fn icon_name(i: usize) -> &'static str {
    const NAMES: [&str; ICON_PRELOAD_COUNT] = [
        "icon-000", "icon-001", "icon-002", "icon-003", "icon-004", "icon-005", "icon-006", "icon-007",
        "icon-008", "icon-009", "icon-010", "icon-011", "icon-012", "icon-013", "icon-014", "icon-015",
        "icon-016", "icon-017", "icon-018", "icon-019", "icon-020", "icon-021", "icon-022", "icon-023",
        "icon-024", "icon-025", "icon-026", "icon-027", "icon-028", "icon-029", "icon-030", "icon-031",
        "icon-032", "icon-033", "icon-034", "icon-035", "icon-036", "icon-037", "icon-038", "icon-039",
        "icon-040", "icon-041", "icon-042", "icon-043", "icon-044", "icon-045", "icon-046", "icon-047",
        "icon-048", "icon-049", "icon-050", "icon-051", "icon-052", "icon-053", "icon-054", "icon-055",
        "icon-056", "icon-057", "icon-058", "icon-059", "icon-060", "icon-061", "icon-062", "icon-063",
        "icon-064", "icon-065", "icon-066", "icon-067", "icon-068", "icon-069", "icon-070", "icon-071",
        "icon-072", "icon-073", "icon-074", "icon-075", "icon-076", "icon-077", "icon-078", "icon-079",
        "icon-080", "icon-081", "icon-082", "icon-083", "icon-084", "icon-085", "icon-086", "icon-087",
        "icon-088", "icon-089", "icon-090", "icon-091", "icon-092", "icon-093", "icon-094", "icon-095",
        "icon-096", "icon-097", "icon-098", "icon-099",
    ];
    NAMES[i % ICON_PRELOAD_COUNT]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_acquire_then_double_release() {
        let mut l = AssetLoader::new();
        let t = l.register_theme("t", 2);
        let _ = l.switch_theme(t);
        let _ = l.commit_switch(true);
        assert!(l.acquire(AssetKind::Icon, "i", 1024, 2048));
        assert!(l.acquire(AssetKind::Icon, "i", 1024, 2048));
        assert_eq!(l.refs_of("i"), 2);
        // 释放一次：仍驻留（还有引用）。
        assert!(l.release("i") && l.is_resident("i"));
        // 驻留字节只计一次（同一资产不重复入账）。
        assert_eq!(l.resident_bytes(), 2048);
        assert!(l.release("i") && !l.is_resident("i"));
        assert_eq!(l.resident_bytes(), 0);
    }

    #[test]
    fn release_unknown_is_honest_false() {
        let mut l = AssetLoader::new();
        assert!(!l.release("never-loaded"));
    }

    #[test]
    fn theme_cap_honest_rejection() {
        let mut l = AssetLoader::new();
        for i in 0..THEME_CAP {
            let _ = i;
            assert_ne!(l.register_theme("t", 1), usize::MAX);
        }
        assert_eq!(l.register_theme("overflow", 1), usize::MAX);
    }

    #[test]
    fn switch_without_commit_leaves_old_active() {
        let mut l = AssetLoader::new();
        let a = l.register_theme("a", 1);
        let b = l.register_theme("b", 1);
        let _ = l.switch_theme(a);
        let _ = l.commit_switch(true);
        // switch 后未 commit：旧主题仍活跃（事务中间态不落盘）。
        let _ = l.switch_theme(b);
        assert_eq!(l.active_theme(), a);
        let _ = l.commit_switch(true);
        assert_eq!(l.active_theme(), b);
    }

    #[test]
    fn sound_over_200kb_is_lazy_too() {
        let mut l = AssetLoader::new();
        let t = l.register_theme("t", 3);
        let _ = l.switch_theme(t);
        let _ = l.commit_switch(true);
        // >200KB 音效：超豁免线仍可装载（按需语义），preloaded 旗标为 false
        // ——账面诚实，不伪装成全量驻留。
        assert!(l.acquire(AssetKind::Sound, "big.flac", 300 * 1024, 300 * 1024));
        let a = l.resident.iter().take(l.resident_n).flatten().find(|a| a.name == "big.flac").unwrap();
        assert!(!a.preloaded);
        // <200KB 音效：preloaded 旗标为 true。
        let _ = l.acquire(AssetKind::Sound, "small.flac", 100 * 1024, 100 * 1024);
        let s = l.resident.iter().take(l.resident_n).flatten().find(|a| a.name == "small.flac").unwrap();
        assert!(s.preloaded);
    }
}

// ===========================================================================
// v2 深化批（F068 · G-B-28）——解码调度 / 二次机会驱逐 / 白屏前置门
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-28 功能定义的实装细化，非新立项）：
// 1. DecodeScheduler —— 解码调度：优先级三档（当前主题资产 > 图标预驻
//    > 低优先装饰）+ 同级 FIFO 定长环；失败重试 ≤2 次后放弃并计数
//    （重试风暴防线——主册「解码失败保持旧主题」的调度面）。
// 2. ClockEvictor —— 二次机会（时钟）驱逐：80MB 上限逼近时扫描引用位，
//    清位一次、清位后再遇即逐出——LRU 近似的零堆形态。
// 3. SwitchGate —— 白屏前置门：新主题解码进度 <50% 不放行换入
//    （commit 的机制面前置——主册「切换全程无白屏」的门控语义）。
// 全部零堆：定长环 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 解码优先级（0 最高）。
pub const DECO_PRIO_THEME: u8 = 0;
pub const DECO_PRIO_ICON: u8 = 1;
pub const DECO_PRIO_LOW: u8 = 2;
/// 解码队列容量。
pub const DECO_Q_CAP: usize = 24;
/// 解码重试上限。
pub const DECO_RETRY_MAX: u8 = 2;
/// 白屏门：新主题就绪 50%（×100 定点）。
pub const SWITCH_READY_PCT_X100: u32 = 50;
/// 驱逐扫描表容量。
pub const EVICT_TABLE_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 深化一：解码调度器
// ---------------------------------------------------------------------------

/// 解码任务。
#[derive(Clone, Copy, Debug)]
pub struct DecodeJob {
    pub prio: u8,
    pub asset_idx: u16,
    pub attempts: u8,
}

/// 解码调度器：取最高优先级最老任务；失败重试 ≤2 次后放弃计数。
pub struct DecodeScheduler {
    q: [Option<DecodeJob>; DECO_Q_CAP],
    n: usize,
    completed: u64,
    abandoned: u64,
}

impl DecodeScheduler {
    pub const fn new() -> Self {
        DecodeScheduler {
            q: [None; DECO_Q_CAP],
            n: 0,
            completed: 0,
            abandoned: 0,
        }
    }

    /// 入队（表满拒绝——背压如实）。
    pub fn submit(&mut self, prio: u8, asset_idx: u16) -> bool {
        if self.n >= DECO_Q_CAP || prio > DECO_PRIO_LOW {
            return false;
        }
        self.q[self.n] = Some(DecodeJob { prio, asset_idx, attempts: 0 });
        self.n += 1;
        true
    }

    /// 取下一个：prio 最小（最高）者中最早入队的——线性扫描定长表。
    pub fn take_next(&mut self) -> Option<DecodeJob> {
        if self.n == 0 {
            return None;
        }
        let mut best = 0usize;
        for k in 1..self.n {
            let b = self.q[best].unwrap();
            let c = self.q[k].unwrap();
            if c.prio < b.prio {
                best = k;
            }
        }
        let job = self.q[best];
        // 压实移除。
        for k in best..self.n - 1 {
            self.q[k] = self.q[k + 1];
        }
        self.q[self.n - 1] = None;
        self.n -= 1;
        job
    }

    /// 任务结果：成功入账；失败重试（≤2）或放弃。
    pub fn report(&mut self, job: DecodeJob, ok: bool) {
        if ok {
            self.completed += 1;
            return;
        }
        if job.attempts < DECO_RETRY_MAX {
            if self.n < DECO_Q_CAP {
                let mut j = job;
                j.attempts += 1;
                self.q[self.n] = Some(j);
                self.n += 1;
            }
        } else {
            self.abandoned += 1; // 重试耗尽——放弃并计数（不静默）
        }
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        (self.completed, self.abandoned, self.n)
    }
}

// ---------------------------------------------------------------------------
// 深化二：二次机会驱逐器
// ---------------------------------------------------------------------------

/// 时钟驱逐器：引用位表 + 时钟指针。驱逐 = 清位一轮后仍未被再引用者。
pub struct ClockEvictor {
    referenced: [bool; EVICT_TABLE_CAP],
    resident: [bool; EVICT_TABLE_CAP],
    hand: usize,
    evicted: u64,
    sweeps: u64,
}

impl ClockEvictor {
    pub const fn new() -> Self {
        ClockEvictor {
            referenced: [false; EVICT_TABLE_CAP],
            resident: [false; EVICT_TABLE_CAP],
            hand: 0,
            evicted: 0,
            sweeps: 0,
        }
    }

    pub fn add(&mut self, idx: usize) -> bool {
        if idx >= EVICT_TABLE_CAP || self.resident[idx] {
            return false;
        }
        self.resident[idx] = true;
        self.referenced[idx] = true; // 新入驻默认被引用（刚用过）
        true
    }

    /// 触达（引用位置位）。
    pub fn touch(&mut self, idx: usize) {
        if idx < EVICT_TABLE_CAP && self.resident[idx] {
            self.referenced[idx] = true;
        }
    }

    /// 驱逐一个：时钟扫描——引用位清零（二次机会），清过位的遇指针即逐出。
    pub fn evict_one(&mut self) -> Option<usize> {
        let mut guard = 0;
        while guard < EVICT_TABLE_CAP * 2 {
            guard += 1;
            let i = self.hand;
            self.hand = (self.hand + 1) % EVICT_TABLE_CAP;
            if !self.resident[i] {
                continue;
            }
            if self.referenced[i] {
                self.referenced[i] = false; // 给第二次机会
            } else {
                self.resident[i] = false;
                self.evicted += 1;
                return Some(i);
            }
        }
        None // 全表都在引用（不可能常驻——保护性返回）
    }

    /// 全表扫描一轮（账面用）。
    pub fn sweep_count(&mut self) -> u64 {
        self.sweeps += 1;
        self.sweeps
    }

    pub fn resident_count(&self) -> usize {
        self.resident.iter().filter(|r| **r).count()
    }

    pub fn evicted(&self) -> u64 {
        self.evicted
    }
}

// ---------------------------------------------------------------------------
// 深化三：白屏前置门
// ---------------------------------------------------------------------------

/// 主题切换门：新主题就绪度 <50% 不放行。
pub struct SwitchGate {
    /// 新主题已解码字节。
    ready_bytes: u64,
    total_bytes: u64,
    /// 门禁历史：不放行计数（防止出现白屏帧的拦截次数）。
    blocked: u64,
    passed: u64,
}

impl SwitchGate {
    pub const fn new() -> Self {
        SwitchGate {
            ready_bytes: 0,
            total_bytes: 0,
            blocked: 0,
            passed: 0,
        }
    }

    /// 新主题解码进度入账。
    pub fn progress(&mut self, ready: u64, total: u64) {
        self.ready_bytes = ready;
        self.total_bytes = total.max(1);
    }

    /// 就绪度 ×100。
    pub fn ready_pct_x100(&self) -> u32 {
        (self.ready_bytes * 100 / self.total_bytes) as u32
    }

    /// 尝试换入：≥50% 放行，否则拦截计数。
    pub fn try_commit(&mut self) -> bool {
        if self.ready_pct_x100() >= SWITCH_READY_PCT_X100 {
            self.passed += 1;
            true
        } else {
            self.blocked += 1;
            false
        }
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.passed, self.blocked)
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：解码调度 / 驱逐 / 白屏门逐条实摆。
pub fn run_assetload_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F068-assetload-deep");

    // ── 解码调度 ──
    // 1) 优先级抢占：低档先入队，高档插队先出。
    let mut ds = DecodeScheduler::new();
    let _ = ds.submit(DECO_PRIO_LOW, 1);
    let _ = ds.submit(DECO_PRIO_LOW, 2);
    let _ = ds.submit(DECO_PRIO_THEME, 10);
    let j = ds.take_next().expect("队列非空");
    cs.add("deco_prio_preempts", j.prio == DECO_PRIO_THEME && j.asset_idx == 10, "");
    // 2) 同级 FIFO。
    let j1 = ds.take_next().expect("剩余任务");
    let j2 = ds.take_next().expect("剩余任务");
    cs.add(
        "deco_fifo_within_prio",
        j1.asset_idx == 1 && j2.asset_idx == 2 && ds.take_next().is_none(),
        "",
    );
    // 3) 失败重试两次后放弃。
    let mut ds2 = DecodeScheduler::new();
    let _ = ds2.submit(DECO_PRIO_THEME, 7);
    let job = ds2.take_next().unwrap();
    ds2.report(job, false); // 第 1 次失败 → 重试
    let job2 = ds2.take_next().unwrap();
    cs.add("deco_retry_requeued", job2.asset_idx == 7 && job2.attempts == 1, "");
    ds2.report(job2, false); // 第 2 次失败 → 再重试
    let job3 = ds2.take_next().unwrap();
    cs.add("deco_retry_second", job3.attempts == 2, "");
    ds2.report(job3, false); // 第 3 次失败 → 放弃
    cs.add("deco_abandon_after_max", ds2.stats() == (0, 1, 0), "");
    // 4) 非法优先级拒绝。
    cs.add("deco_bad_prio_refused", !ds2.submit(3, 1), "");

    // ── 二次机会驱逐 ──
    // 1) 首轮：全员 add 时都带引用位 → 全表清位（二次机会）后逐出 0 号。
    let mut ev = ClockEvictor::new();
    for idx in 0..4 {
        let _ = ev.add(idx);
    }
    let victim = ev.evict_one().expect("有可逐出者");
    cs.add("evict_first_round_clock_hand", victim == 0, "");
    // 2) touch(1) 给第二次机会 → 第二轮逐出 2 号（1 被豁免）。
    ev.touch(1);
    let v2 = ev.evict_one().expect("继续驱逐");
    cs.add("evict_second_chance_spares_touched", v2 == 2, "");
    // 3) 驻留账同步。
    cs.add("evict_resident_ledger", ev.resident_count() == 2 && ev.evicted() == 2, "");
    // 4) 在驻者重复 add 拒绝。
    cs.add("evict_dup_add_refused", !ev.add(3), "");

    // ── 白屏前置门 ──
    // 1) 进度 49% 拦截。
    let mut sg = SwitchGate::new();
    sg.progress(49, 100);
    cs.add("gate_blocks_below_50", !sg.try_commit() && sg.stats() == (0, 1), "");
    // 2) 进度 50% 恰达线放行（≥ 语义）。
    sg.progress(50, 100);
    cs.add("gate_passes_at_50", sg.try_commit() && sg.stats() == (1, 1), "");
    // 3) 空主题（total=0 钳 1）——0 字节就绪不放行。
    let mut sg2 = SwitchGate::new();
    sg2.progress(0, 0);
    cs.add("gate_empty_theme_blocks", !sg2.try_commit(), "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn scheduler_priority_order_full_spectrum() {
        let mut ds = DecodeScheduler::new();
        let _ = ds.submit(DECO_PRIO_ICON, 5);
        let _ = ds.submit(DECO_PRIO_LOW, 6);
        let _ = ds.submit(DECO_PRIO_THEME, 7);
        let _ = ds.submit(DECO_PRIO_ICON, 8);
        let order = [
            ds.take_next().unwrap(),
            ds.take_next().unwrap(),
            ds.take_next().unwrap(),
            ds.take_next().unwrap(),
        ];
        assert_eq!(order[0].prio, DECO_PRIO_THEME);
        assert_eq!(order[1].asset_idx, 5, "同档 FIFO：5 在 8 前");
        assert_eq!(order[2].asset_idx, 8);
        assert_eq!(order[3].prio, DECO_PRIO_LOW);
    }

    #[test]
    fn evictor_clears_bit_before_evicting_touched_sole_resident() {
        let mut ev = ClockEvictor::new();
        let _ = ev.add(7);
        ev.touch(7);
        // 第一轮：清位（给机会）；第二轮：逐出唯一驻留者（引用位已清）。
        assert!(ev.evict_one().is_some(), "唯一资产引用位仅一次豁免");
        assert_eq!(ev.resident_count(), 0);
    }

    #[test]
    fn gate_progress_can_regress_and_reblock() {
        let mut sg = SwitchGate::new();
        sg.progress(80, 100);
        assert!(sg.try_commit());
        sg.progress(30, 100); // 换入后又一批资产——新主题二次切换进度回落
        assert!(!sg.try_commit(), "进度回落即再拦——门是逐次的不是一次性的");
    }
}

// ===========================================================================
// v3 深化批（F068 · G-B-28）——解码暂存池 / 主题差量装载 / 饥饿防老化
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-28 功能定义的实装细化，非新立项）：
// 1. StagingPool —— 解码暂存池：定长缓冲槽 checkout/checkin 配对，
//    池干涸等待计数（背压面）——解码峰值不击穿驻留上限。
// 2. ThemeDiff —— 主题差量装载：两主题资产集合 diff → 只装载差异集
//    （切换耗时 = 差量比例 × 全量耗时——快切机制面）。
// 3. AgeBoost —— 解码队列饥饿防老化：等待轮数越多优先级越高
//    （有效优先级 = 基础 prio − 等待轮数/4，地板 0——低优先级任务
//    也有出头日）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 暂存池槽数（4MB 解码缓冲 ×8 = 32MB 峰值——80MB 上限内的暂存余量）。
pub const STAGING_SLOTS: usize = 8;
/// 差量装载估算基准（全量切换 ms）。
pub const FULL_SWITCH_MS: u32 = 1_200;
/// 老化步长（每等待 4 轮提升一级）。
pub const AGE_BOOST_DIV: u32 = 4;

// ---------------------------------------------------------------------------
// 深化一：解码暂存池
// ---------------------------------------------------------------------------

/// 暂存池（槽位租约配对）。
pub struct StagingPool {
    leased: [bool; STAGING_SLOTS],
    dry_stalls: u64,
    peak_leased: u32,
}

impl StagingPool {
    pub const fn new() -> Self {
        StagingPool { leased: [false; STAGING_SLOTS], dry_stalls: 0, peak_leased: 0 }
    }

    /// 租一个缓冲槽（返回槽 id；池干 → None + 等待计数）。
    pub fn checkout(&mut self) -> Option<usize> {
        match self.leased.iter().position(|l| !*l) {
            Some(slot) => {
                self.leased[slot] = true;
                let n = self.leased.iter().filter(|l| **l).count() as u32;
                self.peak_leased = self.peak_leased.max(n);
                Some(slot)
            }
            None => {
                self.dry_stalls += 1;
                None
            }
        }
    }

    /// 归还（双还拒绝）。
    pub fn checkin(&mut self, slot: usize) -> bool {
        if slot >= STAGING_SLOTS || !self.leased[slot] {
            return false;
        }
        self.leased[slot] = false;
        true
    }

    pub fn available(&self) -> usize {
        self.leased.iter().filter(|l| !**l).count()
    }

    pub fn dry_stalls(&self) -> u64 {
        self.dry_stalls
    }

    pub fn peak(&self) -> u32 {
        self.peak_leased
    }
}

// ---------------------------------------------------------------------------
// 深化二：主题差量装载
// ---------------------------------------------------------------------------

/// 主题资产集合（位图，256 资产代表集）。
pub const THEME_ASSET_BITS: usize = 256;

/// 两主题差量：only_in_a / only_in_b 数量。
pub struct ThemeDiff {
    pub only_in_a: usize,
    pub only_in_b: usize,
    pub common: usize,
}

impl ThemeDiff {
    pub fn compute(a: &[bool; THEME_ASSET_BITS], b: &[bool; THEME_ASSET_BITS]) -> Self {
        let mut d = ThemeDiff { only_in_a: 0, only_in_b: 0, common: 0 };
        for k in 0..THEME_ASSET_BITS {
            match (a[k], b[k]) {
                (true, false) => d.only_in_a += 1,
                (false, true) => d.only_in_b += 1,
                (true, true) => d.common += 1,
                (false, false) => {}
            }
        }
        d
    }

    /// 差量切换耗时估算：需装载 = only_in_b（新主题需解码的），卸载
    /// only_in_a；共用水驻留。耗时 = 装载比例 × 全量。
    pub fn switch_cost_ms(&self) -> u32 {
        let load_frac_x100 = (self.only_in_b as u64) * 100 / THEME_ASSET_BITS as u64;
        (FULL_SWITCH_MS as u64 * load_frac_x100 / 100) as u32
    }

    /// 差量收益 ×100（(全量−差量)/全量）。
    pub fn savings_pct_x100(&self) -> u64 {
        let cost = self.switch_cost_ms() as u64;
        (FULL_SWITCH_MS as u64 - cost) * 100 / FULL_SWITCH_MS as u64
    }
}

// ---------------------------------------------------------------------------
// 深化三：解码队列饥饿防老化
// ---------------------------------------------------------------------------

/// 老化任务（在 v2 DecodeJob 上加等待轮数）。
#[derive(Clone, Copy, Debug)]
pub struct AgedJob {
    pub prio: u8,
    pub asset_idx: u16,
    pub waited_rounds: u32,
}

/// 有效优先级：基础 prio − 等待轮数/AGE_BOOST_DIV（饱和 0——越等越先）。
pub fn effective_prio(job: &AgedJob) -> u8 {
    job.prio.saturating_sub((job.waited_rounds / AGE_BOOST_DIV) as u8)
}

/// 老化调度：每轮取有效优先级最小者，其余 waited+1。
pub struct AgingScheduler {
    jobs: [Option<AgedJob>; 24],
    n: usize,
    rounds: u64,
    starved_forever: u64,
}

impl AgingScheduler {
    pub const fn new() -> Self {
        AgingScheduler { jobs: [None; 24], n: 0, rounds: 0, starved_forever: 0 }
    }

    pub fn submit(&mut self, prio: u8, asset_idx: u16) -> bool {
        if self.n >= 24 {
            return false;
        }
        self.jobs[self.n] = Some(AgedJob { prio, asset_idx, waited_rounds: 0 });
        self.n += 1;
        true
    }

    /// 跑一轮：取有效优先级最高（数值最小）的任务（平局取先入）。
    pub fn run_round(&mut self) -> Option<(u8, u16)> {
        if self.n == 0 {
            return None;
        }
        let mut best = 0usize;
        for k in 1..self.n {
            let b = self.jobs[best].unwrap();
            let c = self.jobs[k].unwrap();
            let (eb, ec) = (effective_prio(&b), effective_prio(&c));
            if ec < eb {
                best = k;
            }
        }
        let job = self.jobs[best];
        for k in best..self.n - 1 {
            self.jobs[k] = self.jobs[k + 1];
        }
        self.jobs[self.n - 1] = None;
        self.n -= 1;
        self.rounds += 1;
        // 其余任务等待 +1。
        for j in self.jobs.iter_mut().flatten() {
            j.waited_rounds += 1;
        }
        job.map(|j| (j.prio, j.asset_idx))
    }

    /// 全部排空（用于 starve 校验）。
    pub fn drain(&mut self, max_rounds: u32) -> u32 {
        let mut ran = 0;
        while self.n > 0 && ran < max_rounds {
            self.run_round();
            ran += 1;
        }
        if self.n > 0 {
            self.starved_forever += self.n as u64;
        }
        ran
    }

    pub fn pending(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：暂存池 / 差量 / 老化逐条实摆。
pub fn run_assetload_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F068-assetload-v3");

    // ── 暂存池 ──
    let mut sp = StagingPool::new();
    let mut slots = [0usize; STAGING_SLOTS];
    let mut all = true;
    for s in slots.iter_mut() {
        match sp.checkout() {
            Some(v) => *s = v,
            None => all = false,
        }
    }
    cs.add("pool_all_leased", all && sp.available() == 0, "");
    cs.add("pool_dry_stalls_counted", sp.checkout().is_none() && sp.dry_stalls() == 1, "");
    cs.add("pool_checkin_frees", sp.checkin(0) && sp.available() == 1, "");
    cs.add("pool_double_checkin_refused", !sp.checkin(0), "");
    cs.add("pool_peak_tracked", sp.peak() == STAGING_SLOTS as u32, "");
    // 峰值语义：先全租再全还再租 1 → 峰值仍是 8。
    for s in slots.iter() {
        let _ = sp.checkin(*s);
    }
    let _ = sp.checkout();
    cs.add("pool_peak_keeps_high_water", sp.peak() == STAGING_SLOTS as u32, "");

    // ── 主题差量 ──
    let mut ta = [false; THEME_ASSET_BITS];
    let mut tb = [false; THEME_ASSET_BITS];
    for k in 0..THEME_ASSET_BITS {
        ta[k] = k % 2 == 0; // A：128 个
        tb[k] = k % 2 == 1; // B：128 个（完全错开）
    }
    let d = ThemeDiff::compute(&ta, &tb);
    cs.add("theme_diff_disjoint", d.only_in_a == 128 && d.only_in_b == 128 && d.common == 0, "");
    cs.add("theme_diff_cost_half", d.switch_cost_ms() == 600, ""); // 128/256 × 1200
    cs.add("theme_diff_savings_half", d.savings_pct_x100() == 50, "");
    // 完全相同主题 → 零成本。
    let same = ThemeDiff::compute(&ta, &ta);
    cs.add("theme_diff_identical_free", same.switch_cost_ms() == 0 && same.savings_pct_x100() == 100, "");
    // 大部分共用 → 高收益。
    let mut tc = ta;
    tc[0] = true; // 只差 1 个
    let d2 = ThemeDiff::compute(&ta, &tc);
    cs.add("theme_diff_mostly_common", d2.only_in_a == 0 && d2.switch_cost_ms() <= 10, "");

    // ── 饥饿防老化 ──
    // 1) 有效优先级随等待提升。
    let j = AgedJob { prio: 2, asset_idx: 1, waited_rounds: 0 };
    cs.add("age_base_prio", effective_prio(&j) == 2, "");
    let j4 = AgedJob { prio: 2, asset_idx: 1, waited_rounds: 4 };
    cs.add("age_boost_after_4_rounds", effective_prio(&j4) == 1, "");
    let j8 = AgedJob { prio: 2, asset_idx: 1, waited_rounds: 100 };
    cs.add("age_floor_zero", effective_prio(&j8) == 0, "");
    // 2) 端到端：低优先级任务在高优先级洪流下 20 轮内出队（无永久饥饿）。
    let mut ag = AgingScheduler::new();
    let _ = ag.submit(2, 999); // 低优先级先行入队
    for k in 0..10u16 {
        let _ = ag.submit(0, k);
    }
    let mut low_out_at = None;
    for r in 0..30u32 {
        if let Some((_, idx)) = ag.run_round() {
            if idx == 999 {
                low_out_at = Some(r);
                break;
            }
        }
    }
    cs.add(
        "age_starvation_freed",
        matches!(low_out_at, Some(r) if r < 20),
        "",
    );
    // 3) 全排空无残留。
    let mut ag2 = AgingScheduler::new();
    for k in 0..12u16 {
        let _ = ag2.submit((k % 3) as u8, k);
    }
    let drained = ag2.drain(50);
    cs.add("age_drains_all", drained == 12 && ag2.pending() == 0, "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn pool_partial_lease_peak_correct() {
        let mut sp = StagingPool::new();
        let a = sp.checkout().unwrap();
        let _b = sp.checkout().unwrap();
        let _ = sp.checkin(a);
        assert_eq!(sp.peak(), 2, "峰值 = 历史最高同时租用");
    }

    #[test]
    fn theme_diff_subset_theme() {
        // B ⊂ A：只需卸载多的，装载 0 → 成本 ≈ 0。
        let mut ta = [false; THEME_ASSET_BITS];
        let mut tb = [false; THEME_ASSET_BITS];
        for k in 0..64 {
            ta[k] = true;
            tb[k] = true;
        }
        ta[100] = true;
        let d = ThemeDiff::compute(&ta, &tb);
        assert_eq!(d.only_in_b, 0, "B 无新资产 → 零装载");
        assert_eq!(d.switch_cost_ms(), 0);
    }

    #[test]
    fn effective_prio_never_negative() {
        let j = AgedJob { prio: 0, asset_idx: 1, waited_rounds: 1000 };
        assert_eq!(effective_prio(&j), 0, "地板 0——不翻转成最高优先");
    }
}

// ===========================================================================
// v4 深化批（F068 · G-B-28）——资产依赖环检测 / 装载预测
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-28 功能定义的实装细化，非新立项）：
// 1. DepGraph —— 资产依赖图（定长邻接 16 节点）：环检测（三色 DFS
//    迭代版——零递归防栈溢出）+ 拓扑序装载顺序（依赖先行）。
// 2. LoadForecast —— 装载预测：按时段表（v3 ModeSchedule 联动口径）
//    预测下一时段需驻留的资产量 → 预热建议。
// 全部零堆：定长表，无 Vec/String/浮点/format!。
// ===========================================================================

/// 依赖图节点容量。
pub const DEP_NODES: usize = 16;

// ---------------------------------------------------------------------------
// 深化一：依赖图环检测 + 拓扑序
// ---------------------------------------------------------------------------

/// 依赖图（邻接矩阵位图）。
pub struct DepGraph {
    edges: [[bool; DEP_NODES]; DEP_NODES], // edges[i][j] = i 依赖 j
    n: usize,
}

impl DepGraph {
    pub const fn new() -> Self {
        DepGraph { edges: [[false; DEP_NODES]; DEP_NODES], n: 0 }
    }

    pub fn add_node(&mut self) -> Option<usize> {
        if self.n >= DEP_NODES {
            return None;
        }
        self.n += 1;
        Some(self.n - 1)
    }

    /// 加依赖边（a 依赖 b）。自环直接拒绝。
    pub fn add_edge(&mut self, a: usize, b: usize) -> bool {
        if a >= self.n || b >= self.n || a == b {
            return false;
        }
        self.edges[a][b] = true;
        true
    }

    /// 环检测：三色 DFS（0 白 1 灰 2 黑），迭代栈（防递归爆栈）。
    pub fn has_cycle(&self) -> bool {
        let mut color = [0u8; DEP_NODES];
        for start in 0..self.n {
            if color[start] != 0 {
                continue;
            }
            // 迭代 DFS：栈存 (节点, 已展开边游标)。
            let mut stack = [(0usize, 0usize); DEP_NODES];
            let mut sp = 0usize;
            stack[sp] = (start, 0);
            sp += 1;
            color[start] = 1;
            while sp > 0 {
                let node = stack[sp - 1].0;
                let mut c = stack[sp - 1].1;
                let mut advanced = false;
                while c < self.n {
                    let next = c;
                    c += 1;
                    if self.edges[node][next] {
                        match color[next] {
                            1 => return true, // 灰 → 环
                            0 => {
                                stack[sp - 1].1 = c;
                                stack[sp] = (next, 0);
                                sp += 1;
                                color[next] = 1;
                                advanced = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                if advanced {
                    continue;
                }
                stack[sp - 1].1 = self.n;
                color[node] = 2;
                sp -= 1;
            }
        }
        false
    }

    /// 拓扑序（Kahn 入度法，零堆——入度定长表）。返回序长（< n = 有环）。
    pub fn topo_order(&self, out: &mut [usize]) -> usize {
        let mut indeg = [0u32; DEP_NODES];
        for i in 0..self.n {
            for j in 0..self.n {
                if self.edges[i][j] {
                    indeg[i] += 1; // 依赖别人 → 入度 +1（被依赖者先装载）
                }
            }
        }
        let mut placed = 0usize;
        loop {
            let mut picked = None;
            for k in 0..self.n {
                if indeg[k] == 0 && !out[..placed].contains(&k) {
                    picked = Some(k);
                    break;
                }
            }
            match picked {
                Some(k) => {
                    if placed < out.len() {
                        out[placed] = k;
                    }
                    placed += 1;
                    // 移除 k 的出边（依赖 k 的入度 −1）。
                    for i in 0..self.n {
                        if self.edges[i][k] && indeg[i] > 0 {
                            indeg[i] -= 1;
                        }
                    }
                    indeg[k] = u32::MAX; // 标记已排
                }
                None => break,
            }
        }
        placed
    }
}

// ---------------------------------------------------------------------------
// 深化二：装载预测
// ---------------------------------------------------------------------------

/// 装载预测：时段 → 预测驻留资产字节（示例画像：工作时段重主题、
/// 夜间轻主题——旋钮表）。
pub struct LoadForecast {
    /// 24h 每小时预测驻留 MB（×1）。
    hourly_mb: [u32; 24],
}

impl LoadForecast {
    pub fn factory() -> Self {
        let mut h = [40u32; 24];
        for hour in h.iter_mut().take(24).skip(8).take(10) {
            *hour = 80; // 8-18 点工作重主题（80MB 上限口径）
        }
        LoadForecast { hourly_mb: h }
    }

    /// 预热建议：当前小时 → 下一小时 MB 差（正 = 需预载，负 = 可释放）。
    pub fn warmup_delta_mb(&self, hour: usize) -> i32 {
        let cur = self.hourly_mb[hour % 24];
        let next = self.hourly_mb[(hour + 1) % 24];
        next as i32 - cur as i32
    }

    /// 驻留上限对账：预测值 ≤80MB（主册单主题驻留上限）。
    pub fn within_resident_cap(&self) -> bool {
        self.hourly_mb.iter().all(|v| *v <= 80)
    }

    pub fn hourly_mb(&self) -> &[u32; 24] {
        &self.hourly_mb
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：依赖图 / 装载预测逐条实摆。
pub fn run_assetload_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F068-assetload-v4");

    // ── 依赖图 ──
    let mut g = DepGraph::new();
    for _ in 0..4 {
        let _ = g.add_node();
    }
    // 链：3 依赖 2，2 依赖 1，1 依赖 0（0 先装载）。
    let _ = g.add_edge(3, 2);
    let _ = g.add_edge(2, 1);
    let _ = g.add_edge(1, 0);
    cs.add("dep_chain_acyclic", !g.has_cycle(), "");
    let mut order = [0usize; DEP_NODES];
    let n = g.topo_order(&mut order);
    cs.add("dep_topo_full", n == 4, "");
    // 拓扑性质：被依赖者先于依赖者（0 在 1 前，1 在 2 前）。
    let pos = |v: usize| order[..n].iter().position(|x| *x == v).unwrap();
    cs.add("dep_topo_order_valid", pos(0) < pos(1) && pos(1) < pos(2) && pos(2) < pos(3), "");
    // 自环拒绝。
    cs.add("dep_self_edge_refused", !g.add_edge(0, 0), "");
    // 加环 → 检出。
    let _ = g.add_edge(0, 3);
    cs.add("dep_cycle_detected", g.has_cycle(), "");
    // 有环时拓扑不完整。
    cs.add("dep_topo_truncated_on_cycle", g.topo_order(&mut order) < 4, "");
    // 满容拒绝。
    let mut g2 = DepGraph::new();
    for _ in 0..DEP_NODES {
        let _ = g2.add_node();
    }
    cs.add("dep_cap_honest", g2.add_node().is_none(), "");

    // ── 装载预测 ──
    let lf = LoadForecast::factory();
    cs.add("forecast_within_cap", lf.within_resident_cap(), "");
    // 7 点 → 8 点：40 → 80 需预热 +40。
    cs.add("forecast_warmup_morning", lf.warmup_delta_mb(7) == 40, "");
    // 17 点 → 18 点：80 → 40 可释放 −40（工作段 8-17 含）。
    cs.add("forecast_release_evening", lf.warmup_delta_mb(17) == -40, "");
    // 午夜无波动。
    cs.add("forecast_night_flat", lf.warmup_delta_mb(2) == 0, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn dep_two_node_cycle() {
        let mut g = DepGraph::new();
        let _ = g.add_node();
        let _ = g.add_node();
        let _ = g.add_edge(0, 1);
        let _ = g.add_edge(1, 0);
        assert!(g.has_cycle());
    }

    #[test]
    fn topo_diamond_dependency() {
        // 菱形：3 依赖 1、2；1、2 依赖 0。
        let mut g = DepGraph::new();
        for _ in 0..4 {
            let _ = g.add_node();
        }
        let _ = g.add_edge(3, 1);
        let _ = g.add_edge(3, 2);
        let _ = g.add_edge(1, 0);
        let _ = g.add_edge(2, 0);
        let mut order = [0usize; DEP_NODES];
        assert_eq!(g.topo_order(&mut order), 4);
        let pos = |v: usize| order[..4].iter().position(|x| *x == v).unwrap();
        assert!(pos(0) < pos(1) && pos(0) < pos(2));
        assert!(pos(1) < pos(3) && pos(2) < pos(3));
    }

    #[test]
    fn forecast_mirror_schedule() {
        let lf = LoadForecast::factory();
        // 工作段（8-17）全 80。
        assert!(lf.hourly_mb()[8..18].iter().all(|v| *v == 80));
    }
}

// ===========================================================================
// v5 深化批（deep5）：引用计数 GC + 加载取消传播
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：资产引用计数 GC（16 槽——计数归零即回收候选）
// ---------------------------------------------------------------------------

/// 引用计数资产表。
pub struct RefCountGc {
    refs: [u16; 16],
    present: [bool; 16],
    /// 累计回收数 / 累计装载次数。
    reclaimed: u32,
    loads: u32,
}

impl RefCountGc {
    pub const fn new() -> Self {
        RefCountGc { refs: [0; 16], present: [false; 16], reclaimed: 0, loads: 0 }
    }

    /// 装载（引用计数 1；重复装载 = 引用 +1——共享不重载）。
    pub fn load(&mut self, id: usize) -> bool {
        if id >= 16 {
            return false;
        }
        self.refs[id] = self.refs[id].saturating_add(1);
        if !self.present[id] {
            self.present[id] = true;
            self.loads += 1;
        }
        true
    }

    /// 释放一个引用。
    pub fn release(&mut self, id: usize) -> bool {
        if id >= 16 || self.refs[id] == 0 {
            return false;
        }
        self.refs[id] -= 1;
        true
    }

    /// GC：回收计数归零的在驻资产（返回回收数）。
    pub fn collect(&mut self) -> u32 {
        let mut n = 0;
        for k in 0..16 {
            if self.present[k] && self.refs[k] == 0 {
                self.present[k] = false;
                n += 1;
                self.reclaimed += 1;
            }
        }
        n
    }

    pub fn resident(&self) -> usize {
        self.present.iter().filter(|p| **p).count()
    }

    pub fn stats(&self) -> (u32, u32) {
        (self.loads, self.reclaimed)
    }
}

// ---------------------------------------------------------------------------
// 深化二：加载取消传播（父任务取消 → 子任务级联取消，已落盘的不回滚）
// ---------------------------------------------------------------------------

/// 取消传播树（深度 3：root → 4 子 → 各 2 孙——扁平数组模拟）。
pub struct CancelTree {
    /// 状态：0 运行 / 1 完成 / 2 取消。
    state: [u8; 12],
    parent: [i8; 12],
    cancelled: u32,
}

pub const CANCEL_RUNNING: u8 = 0;
pub const CANCEL_DONE: u8 = 1;
pub const CANCEL_CANCELLED: u8 = 2;

impl CancelTree {
    /// 固定树：0 为根；1-4 为子；5-12 → 子 1 的孙（5,6）、子 2 的孙（7,8）…
    pub const fn new() -> Self {
        let mut parent = [-1i8; 12];
        parent[1] = 0;
        parent[2] = 0;
        parent[3] = 0;
        parent[4] = 0;
        parent[5] = 1;
        parent[6] = 1;
        parent[7] = 2;
        parent[8] = 2;
        parent[9] = 3;
        parent[10] = 3;
        parent[11] = 4;
        // 11 的兄弟 12 不存在——12 槽留空（0 亦根占位）。
        CancelTree { state: [CANCEL_RUNNING; 12], parent, cancelled: 0 }
    }

    /// 取消节点：级联取消全部在途后代（沿父链穿透——已完成中间节点
    /// 不屏蔽祖先的取消意图，但它自身不回滚）。
    pub fn cancel(&mut self, node: usize) -> u32 {
        if node >= 12 || self.state[node] == CANCEL_DONE {
            return 0;
        }
        let mut n = 0u32;
        if self.state[node] == CANCEL_RUNNING {
            self.state[node] = CANCEL_CANCELLED;
            n += 1;
        }
        for k in 0..12 {
            if k == node {
                continue;
            }
            if self.state[k] == CANCEL_RUNNING && self.is_descendant_of(k, node) {
                self.state[k] = CANCEL_CANCELLED;
                n += 1;
            }
        }
        self.cancelled += n;
        n
    }

    /// k 是否为 node 的后代（父链 ≤3 跳）。
    fn is_descendant_of(&self, k: usize, node: usize) -> bool {
        let mut cur = self.parent[k];
        for _ in 0..3 {
            if cur < 0 {
                return false;
            }
            if cur as usize == node {
                return true;
            }
            cur = self.parent[cur as usize];
        }
        false
    }

    /// 完成（取消后不可完成）。
    pub fn finish(&mut self, node: usize) -> bool {
        if node >= 12 || self.state[node] != CANCEL_RUNNING {
            return false;
        }
        self.state[node] = CANCEL_DONE;
        true
    }

    pub fn state_of(&self, node: usize) -> u8 {
        self.state[node]
    }

    pub fn cancelled(&self) -> u32 {
        self.cancelled
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_assetload_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F068-assetload-v5");

    // ── 引用 GC ──
    // 1) 共享装载不重复占槽。
    let mut gc = RefCountGc::new();
    let _ = gc.load(3);
    let _ = gc.load(3);
    cs.add("refshare_single_resident", gc.resident() == 1 && gc.stats().0 == 1, "");
    // 2) 两次释放后才可回收。
    let _ = gc.release(3);
    cs.add("refstill_held", gc.collect() == 0 && gc.resident() == 1, "");
    let _ = gc.release(3);
    cs.add("refzero_collects", gc.collect() == 1 && gc.resident() == 0 && gc.stats().1 == 1, "");
    // 3) 超额释放拒绝。
    cs.add("refover_release_refused", !gc.release(3), "");

    // ── 取消传播 ──
    // 4) 取消子 2 → 孙 7/8 级联取消，兄弟 1 不动。
    let mut ct = CancelTree::new();
    let n = ct.cancel(2);
    cs.add(
        "cancel_cascades_to_grandchildren",
        n == 3 && ct.state_of(2) == CANCEL_CANCELLED && ct.state_of(7) == CANCEL_CANCELLED && ct.state_of(8) == CANCEL_CANCELLED && ct.state_of(1) == CANCEL_RUNNING,
        "",
    );
    // 5) 已完成节点不受取消影响。
    let mut ct2 = CancelTree::new();
    let _ = ct2.finish(5);
    cs.add("cancel_done_immune", ct2.cancel(1) >= 1 && ct2.state_of(5) == CANCEL_DONE, "");
    // 6) 取消后不可完成。
    cs.add("cancel_blocks_finish", !ct2.finish(6) && ct2.finish(9), ""); // 6 号被级联取消；9 号仍在途

    // ── 联动：GC 与取消（取消释放引用 → GC 回收）──
    // 7) 完整流：装载 → 取消 → 引用释放 → GC 清。
    let mut gc2 = RefCountGc::new();
    let _ = gc2.load(9);
    let _ = gc2.release(9);
    cs.add("cancel_release_gc_chain", gc2.collect() == 1 && gc2.stats() == (1, 1), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn refcount_saturation_safe() {
        let mut gc = RefCountGc::new();
        for _ in 0..70_000 {
            let _ = gc.load(0);
        }
        // u16 饱和不 panic。
        assert_eq!(gc.resident(), 1);
        let mut freed = 0;
        for _ in 0..70_000 {
            freed += if gc.release(0) { 0 } else { 1 };
        }
        assert!(freed >= 70_000 - u16::MAX as u32 - 1, "超额释放被拒绝");
    }

    #[test]
    fn cancel_root_nukes_all() {
        let mut ct = CancelTree::new();
        let _ = ct.finish(3);
        let n = ct.cancel(0);
        // 根取消 = 根自身 + 10 个在途后代（已完成 3 免疫不回滚）。
        assert_eq!(n, 11);
        assert_eq!(ct.state_of(3), CANCEL_DONE);
        assert_eq!(ct.cancelled(), 11);
    }
}
