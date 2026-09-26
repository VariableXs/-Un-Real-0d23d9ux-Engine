//! F456 「此机」总览页（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **三区内容清单；容量条与 F268 阈值联动；悬停明细准确性；设备区实时性；
//! 页打开 <1.5s（F404 判据复用）。**
//!
//! 功能定义（主册批次三）：卷区（每卷图标+容量条 F366 同源配色——黄红预警
//! 联动 F268）+常用文件夹六宫格（桌面/文档/下载/S: 共享/图片/最近）+设备区
//! （可移动介质接入即显 F293 联动）；卷容量条悬停显示明细（已用/可用/大目录
//! Top3）。
//!
//! 零堆纪律：定长卷表与设备表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 页打开时限（主册：<1.5s，F404 判据复用）。
pub const PAGE_OPEN_BUDGET_MS: u64 = 1_500;
/// 容量预警阈值（F268 联动同源：黄 ≥80%，红 ≥90%）。
pub const WARN_PERMILLE: u32 = 800;
pub const RED_PERMILLE: u32 = 900;
/// 常用文件夹六宫格（主册原文六格）。
pub const PINNED_N: usize = 6;
/// 悬停明细大目录 Top3（主册原文）。
pub const HOVER_TOP_N: usize = 3;
/// 卷表容量（此机页卷区行数上限）。
pub const VOLUME_CAP: usize = 12;

/// 容量条预警色（F268/F366 同源三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarLevel {
    Normal,
    Warn,
    Red,
}

pub fn bar_level(used_permille: u32) -> BarLevel {
    if used_permille >= RED_PERMILLE {
        BarLevel::Red
    } else if used_permille >= WARN_PERMILLE {
        BarLevel::Warn
    } else {
        BarLevel::Normal
    }
}

/// 一卷的此机页数据。
#[derive(Clone, Copy, Debug)]
pub struct Volume {
    pub label: [u8; 12],
    pub label_n: usize,
    pub used_mb: u64,
    pub total_mb: u64,
    /// 悬停明细：大目录 Top3（字节数）。
    pub top_dirs_mb: [u64; HOVER_TOP_N],
}

impl Volume {
    pub fn new(label: &str, used_mb: u64, total_mb: u64) -> Volume {
        let mut v = Volume {
            label: [0; 12],
            label_n: 0,
            used_mb,
            total_mb,
            top_dirs_mb: [0; HOVER_TOP_N],
        };
        for (i, b) in label.as_bytes().iter().enumerate().take(12) {
            v.label[i] = *b;
            v.label_n += 1;
        }
        v
    }

    /// 已用千分比（总量 0 → 诚实 0，不除零）。
    pub fn used_permille(&self) -> u32 {
        if self.total_mb == 0 {
            return 0;
        }
        (self.used_mb * 1_000 / self.total_mb).min(1_000) as u32
    }

    pub fn free_mb(&self) -> u64 {
        self.total_mb.saturating_sub(self.used_mb)
    }

    /// 悬停明细：已用/可用/大目录 Top3——数值自洽审计（Top3 之和不超已用）。
    pub fn hover_consistent(&self) -> bool {
        let top: u64 = self.top_dirs_mb.iter().sum();
        top <= self.used_mb
    }

    /// 悬停明细准确：Top3 降序排列（明细呈现规范）。
    pub fn top_dirs_sorted_desc(&self) -> bool {
        (0..HOVER_TOP_N - 1).all(|i| self.top_dirs_mb[i] >= self.top_dirs_mb[i + 1])
    }
}

/// 可移动介质设备区（F293 联动：接入即显——实时性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovableDev {
    pub present: bool,
    pub name_tag: u8,
}

/// 此机页装配器：三区内容清单。
pub struct ThisPcPage {
    volumes: [Option<Volume>; VOLUME_CAP],
    vol_n: usize,
    pinned: [&'static str; PINNED_N],
    devices: [RemovableDev; 4],
    dev_n: usize,
}

impl ThisPcPage {
    pub const fn new() -> Self {
        ThisPcPage {
            volumes: [None; VOLUME_CAP],
            vol_n: 0,
            pinned: ["Desktop", "Documents", "Downloads", "S:", "Pictures", "Recent"],
            devices: [RemovableDev { present: false, name_tag: 0 }; 4],
            dev_n: 0,
        }
    }

    pub fn add_volume(&mut self, v: Volume) -> bool {
        if self.vol_n >= VOLUME_CAP {
            return false;
        }
        self.volumes[self.vol_n] = Some(v);
        self.vol_n += 1;
        true
    }

    pub fn volume(&self, i: usize) -> Option<&Volume> {
        self.volumes.get(i).and_then(|v| v.as_ref())
    }

    pub fn volume_count(&self) -> usize {
        self.vol_n
    }

    /// 六宫格常驻（主册原文六格）。
    pub fn pinned_folders(&self) -> [&'static str; PINNED_N] {
        self.pinned
    }

    /// 设备接入即显（实时性：接入事件同步进设备区）。
    pub fn plug_device(&mut self, tag: u8) -> bool {
        if self.dev_n >= 4 {
            return false;
        }
        self.devices[self.dev_n] = RemovableDev { present: true, name_tag: tag };
        self.dev_n += 1;
        true
    }

    pub fn unplug_device(&mut self, tag: u8) -> bool {
        for i in 0..self.dev_n {
            if self.devices[i].name_tag == tag && self.devices[i].present {
                self.devices[i].present = false;
                return true;
            }
        }
        false
    }

    pub fn present_devices(&self) -> usize {
        (0..self.dev_n).filter(|&i| self.devices[i].present).count()
    }

    /// 页打开时限核算（F404 复用）：装配+渲染耗时不超 1.5s。
    pub fn open_within_budget(elapsed_ms: u64) -> bool {
        elapsed_ms < PAGE_OPEN_BUDGET_MS
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_pcpage_checks() -> CheckSet {
    let mut cs = CheckSet::new("F456-pcpage");
    // 1) 三区内容清单：卷区+六宫格+设备区。
    let mut p = ThisPcPage::new();
    cs.add("pinned_six", p.pinned_folders().len() == PINNED_N, "");
    cs.add("pinned_has_s_share", p.pinned_folders().contains(&"S:"), "");
    // 2) 容量条与 F268 阈值联动（黄 800‰ / 红 900‰）。
    cs.add("bar_normal", bar_level(500) == BarLevel::Normal, "");
    cs.add("bar_warn", bar_level(850) == BarLevel::Warn, "");
    cs.add("bar_red", bar_level(950) == BarLevel::Red, "");
    // 3) 悬停明细准确：数值自洽 + Top3 降序。
    let mut v = Volume::new("C:", 850, 1_000);
    v.top_dirs_mb = [400, 300, 100];
    cs.add("hover_consistent", v.hover_consistent() && v.top_dirs_sorted_desc(), "");
    cs.add("hover_free_math", v.free_mb() == 150, "");
    let mut v2 = Volume::new("D:", 100, 1_000);
    v2.top_dirs_mb = [10, 50, 30];
    cs.add("hover_unsorted_caught", !v2.top_dirs_sorted_desc(), "");
    // 4) 千分比与除零边界。
    let bad = Volume::new("X:", 0, 0);
    cs.add("zero_total_honest", bad.used_permille() == 0 && bar_level(bad.used_permille()) == BarLevel::Normal, "");
    cs.add("permille_clamped", Volume::new("Y:", 2_000, 1_000).used_permille() == 1_000, "");
    // 5) 设备区实时性：接入即显、拔出即隐。
    cs.add("plug_instant", p.plug_device(7) && p.present_devices() == 1, "");
    cs.add("unplug_instant", p.unplug_device(7) && p.present_devices() == 0, "");
    cs.add("unplug_unknown_honest", !p.unplug_device(99), "");
    // 6) 页打开 <1.5s 判据（F404 复用）。
    cs.add("open_budget", ThisPcPage::open_within_budget(1_499) && !ThisPcPage::open_within_budget(1_500), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_area_lists_all_volumes() {
        let mut p = ThisPcPage::new();
        assert!(p.add_volume(Volume::new("C:", 400, 1_000)));
        assert!(p.add_volume(Volume::new("D:", 100, 2_000)));
        assert_eq!(p.volume_count(), 2);
        assert_eq!(p.volume(0).unwrap().free_mb(), 600);
    }

    #[test]
    fn warn_thresholds_match_f268() {
        // 黄红预警与资源管理器顶部横幅同一真相（同源阈值）。
        assert_eq!(bar_level(799), BarLevel::Normal);
        assert_eq!(bar_level(800), BarLevel::Warn);
        assert_eq!(bar_level(899), BarLevel::Warn);
        assert_eq!(bar_level(900), BarLevel::Red);
    }

    #[test]
    fn device_area_tracks_presence() {
        let mut p = ThisPcPage::new();
        p.plug_device(1);
        p.plug_device(2);
        assert_eq!(p.present_devices(), 2);
        p.unplug_device(1);
        assert_eq!(p.present_devices(), 1);
    }
}

// ===========================================================================
// 深化 v2（F456）：打开预算分解 / 悬停明细 Top3 自洽对账 / 设备区事件流 /
// 六宫格目录表 / 卷区容量状态跃迁
// ===========================================================================

/// 页打开预算分解（主册「页打开 <1.5s」的阶段预算——总预算硬线不变，
/// 阶段预算让人话说清楚时间花在哪；实测对账走各阶段打点）。
pub const OPEN_BUDGET_STAGES: [(&str, u64); 4] = [
    ("volume-enum", 400),
    ("pinned-grid", 300),
    ("device-scan", 200),
    ("render", 600),
];

/// 阶段预算之和 = 总预算（一处一事实：分解不虚增）。
pub fn budget_sum() -> u64 {
    OPEN_BUDGET_STAGES.iter().map(|(_, ms)| *ms).sum()
}

/// 六宫格目录表（主册六项：桌面/文档/下载/S: 共享/图片/最近——
/// 定长表与 v1 PINNED_N=6 同源）。
pub const PINNED_TABLE: [&str; PINNED_N] = [
    "桌面", "文档", "下载", "S: 共享", "图片", "最近",
];

/// 卷区容量状态跃迁（v1 BarLevel 三态的运行面：用量变化过阈 → 状态
/// 跃迁 + 悬停明细跟着变——「同一真相」在状态机里保持）。
pub fn level_transition(old: BarLevel, used_permille: u32) -> Option<BarLevel> {
    let new = bar_level(used_permille);
    if new == old {
        None
    } else {
        Some(new)
    }
}

/// 悬停明细 Top3 数值自洽对账（主册「已用/可用/大目录 Top3」——
/// Top3 大小之和 ≤ 已用量：超过即明细在说谎）。
pub fn hover_top3_consistent(used_bytes: u64, top3: [u64; 3]) -> bool {
    let sum = top3.iter().fold(0u64, |acc, &x| acc.saturating_add(x));
    sum <= used_bytes
}

/// 设备区事件流（接入即显 F293 联动的运行面：接入/拔出事件定长环——
/// 拔出事件让设备区行即时消失，接入事件让行出现）。
pub const DEVICE_EVENT_CAP: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceEvent {
    PluggedIn,
    Unplugged,
}

pub struct DeviceEventLog {
    ring: [(u64, DeviceEvent); DEVICE_EVENT_CAP],
    head: usize,
    n: usize,
    pub present: bool,
}

impl DeviceEventLog {
    pub const fn new() -> Self {
        DeviceEventLog {
            ring: [(0, DeviceEvent::Unplugged); DEVICE_EVENT_CAP],
            head: 0,
            n: 0,
            present: false,
        }
    }

    pub fn push(&mut self, at_ms: u64, ev: DeviceEvent) {
        self.ring[self.head] = (at_ms, ev);
        self.head = (self.head + 1) % DEVICE_EVENT_CAP;
        self.n = (self.n + 1).min(DEVICE_EVENT_CAP);
        self.present = match ev {
            DeviceEvent::PluggedIn => true,
            DeviceEvent::Unplugged => false,
        };
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn latest(&self) -> Option<(u64, DeviceEvent)> {
        if self.n == 0 {
            return None;
        }
        let idx = (self.head + DEVICE_EVENT_CAP - 1) % DEVICE_EVENT_CAP;
        Some(self.ring[idx])
    }
}

// ---------------------------------------------------------------------------
// 深化自检（F456 v2）
// ---------------------------------------------------------------------------

pub fn run_pcpage_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F456-v2");
    // 1) 预算分解：四阶段和 = 1500ms（不多不少）。
    cs.add("budget_sum_exact", budget_sum() == PAGE_OPEN_BUDGET_MS, "");
    cs.add("budget_table_named", OPEN_BUDGET_STAGES.len() == 4 && OPEN_BUDGET_STAGES[0].0 == "volume-enum", "");
    // 2) 六宫格表完整（六项与主册清单一致）。
    cs.add("pinned_table", PINNED_TABLE.len() == PINNED_N && PINNED_TABLE[3] == "S: 共享", "");
    // 3) 状态跃迁：过阈即报新态、未过阈不空报。
    cs.add("level_transition_on", level_transition(BarLevel::Normal, 850) == Some(BarLevel::Warn), "");
    cs.add("level_transition_none", level_transition(BarLevel::Normal, 500).is_none(), "");
    cs.add("level_transition_down", level_transition(BarLevel::Red, 700) == Some(BarLevel::Normal), "");
    // 4) 悬停明细自洽：Top3 和 ≤ 已用。
    cs.add("hover_consistent", hover_top3_consistent(1000, [400, 300, 200]), "");
    cs.add("hover_lie_detected", !hover_top3_consistent(1000, [600, 500, 0]), "");
    // 5) 设备事件流：接入即显、拔出即隐、环上限。
    let mut log = DeviceEventLog::new();
    cs.add("dev_empty_honest", log.latest().is_none() && !log.present, "");
    log.push(100, DeviceEvent::PluggedIn);
    cs.add("dev_plugged", log.present && log.latest() == Some((100, DeviceEvent::PluggedIn)), "");
    log.push(200, DeviceEvent::Unplugged);
    cs.add("dev_unplugged", !log.present && log.len() == 2, "");
    cs.add("dev_ring_cap", {
        let mut l2 = DeviceEventLog::new();
        for i in 0..(DEVICE_EVENT_CAP * 2) {
            l2.push(i as u64, if i % 2 == 0 { DeviceEvent::PluggedIn } else { DeviceEvent::Unplugged });
        }
        l2.len() == DEVICE_EVENT_CAP
    }, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn budget_stages_no_gaps() {
        // 分解账不虚增不缩水（阶段和恰等于总预算）。
        assert_eq!(OPEN_BUDGET_STAGES.iter().map(|(_, ms)| ms).sum::<u64>(), 1500);
    }

    #[test]
    fn level_ladder_monotonic() {
        // 阈值单调性：用量升 → 状态不回退（同用量点比较）。
        assert_eq!(bar_level(799), BarLevel::Normal);
        assert_eq!(bar_level(800), BarLevel::Warn);
        assert_eq!(bar_level(899), BarLevel::Warn);
        assert_eq!(bar_level(900), BarLevel::Red);
    }

    #[test]
    fn device_log_wraps_without_loss() {
        let mut log = DeviceEventLog::new();
        for i in 0..DEVICE_EVENT_CAP + 3 {
            log.push(i as u64, if i % 2 == 0 { DeviceEvent::PluggedIn } else { DeviceEvent::Unplugged });
        }
        // 环满后 latest 仍准确（最近事件）。
        let last_idx = DEVICE_EVENT_CAP + 2;
        let expect = if last_idx % 2 == 0 { DeviceEvent::PluggedIn } else { DeviceEvent::Unplugged };
        assert_eq!(log.latest().map(|(_, e)| e), Some(expect));
    }

    #[test]
    fn hover_consistency_saturating() {
        // 极端值不 panic（饱和加法）。
        assert!(!hover_top3_consistent(100, [u64::MAX, u64::MAX, 1]));
        assert!(hover_top3_consistent(u64::MAX, [u64::MAX, 0, 0]));
    }
}
