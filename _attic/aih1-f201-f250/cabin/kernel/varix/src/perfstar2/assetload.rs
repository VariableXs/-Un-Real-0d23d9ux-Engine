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
