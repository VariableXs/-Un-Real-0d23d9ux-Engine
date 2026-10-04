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

// ===========================================================================
// 深化 v7（F456）：卷健康审计 / 用量趋势环 / 安全弹出状态机 /
// 双语六宫格对账 / 持久化通道 v7（W7P1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 阈值审计——F268 联动（黄 800‰/红 900‰）的不变量固化：红 > 黄 >
//    0、双双不越千分位上限——阈值漂移在检查面现形。
// 2. 用量趋势环——容量条只有瞬时值看不出「快满了」：定长环记用量采样，
//    增长速率告警（预算内将满 = 提前提示，不是满了一脸懵）。
// 3. 安全弹出状态机——设备区「接入即显」的另一端：弹出要走
//    Mounted→Ejecting→Ejected 全程，失败有重试冷却（400ms——与
//    F496 磁贴冷却同源量纲），重试计数诚实。
// 4. 双语对账——v1 英文 pinned 表与 v2 中文表逐位对齐（同源双语：
//    索引漂移 = 界面两处说不一样的话）。
// 5. 持久化——六宫格选择掩码 + 设备标签落盘 v7 通道（W7P1 + FNV 尾）。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 阈值审计（F268 联动不变量）
// ---------------------------------------------------------------------------

/// 阈值不变量：0 < 黄 < 红 ≤ 1000（一处一事实的检查面固化）。
pub fn threshold_audit() -> bool {
    WARN_PERMILLE > 0 && RED_PERMILLE > WARN_PERMILLE && RED_PERMILLE <= 1_000
}

/// 卷数据自洽（v1 used_permille 有钳制，这里把「used > total = 传感器
/// 在说谎」显性化——不静默信）。
pub fn volume_sane(used_mb: u64, total_mb: u64) -> bool {
    used_mb <= total_mb
}

// ---------------------------------------------------------------------------
// 用量趋势环（增长速率告警）
// ---------------------------------------------------------------------------

/// 趋势环容量（16 采样）。
pub const VOLUME_TREND_CAP: usize = 16;
/// 增长告警阈值（窗口内涨幅 ≥ 50‰ = 快满预警——预算面可调常量）。
pub const GROWTH_ALERT_PERMILLE: u32 = 50;

/// 用量趋势环：采样 + 窗口涨幅 + 告警。
pub struct VolumeTrend {
    ring: [u32; VOLUME_TREND_CAP], // used_permille 采样
    head: usize,
    n: usize,
}

impl VolumeTrend {
    pub const fn new() -> Self {
        VolumeTrend { ring: [0; VOLUME_TREND_CAP], head: 0, n: 0 }
    }

    pub fn push(&mut self, used_permille: u32) {
        self.ring[self.head] = used_permille.min(1_000);
        self.head = (self.head + 1) % VOLUME_TREND_CAP;
        self.n = (self.n + 1).min(VOLUME_TREND_CAP);
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 窗口涨幅（最新 − 最旧；账面不足 2 采样诚实 0）。
    pub fn growth_permille(&self) -> u32 {
        if self.n < 2 {
            return 0;
        }
        let oldest = (self.head + VOLUME_TREND_CAP - self.n) % VOLUME_TREND_CAP;
        let newest = (self.head + VOLUME_TREND_CAP - 1) % VOLUME_TREND_CAP;
        self.ring[newest].saturating_sub(self.ring[oldest])
    }

    /// 告警（窗口涨幅 ≥ 阈值；负增长不告警——清理是好事不吓人）。
    pub fn growth_alert(&self) -> bool {
        self.growth_permille() >= GROWTH_ALERT_PERMILLE
    }
}

// ---------------------------------------------------------------------------
// 安全弹出状态机（Mounted → Ejecting → Ejected；失败重试有冷却）
// ---------------------------------------------------------------------------

/// 弹出重试冷却（ms——与 F496 磁贴触发冷却同源量纲）。
pub const EJECT_RETRY_COOLDOWN_MS: u64 = 400;

/// 弹出状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EjectState {
    /// 已挂载（正常）。
    Mounted,
    /// 弹出中（句柄已关闭、缓冲冲刷中）。
    Ejecting,
    /// 已弹出（安全拔出——设备区行消失）。
    Ejected,
    /// 弹出失败（被占用——用户可重试，有冷却）。
    Failed,
}

/// 安全弹出器。
pub struct SafeEjector {
    pub state: EjectState,
    last_attempt_ms: Option<u64>,
    pub retry_count: usize,
}

impl SafeEjector {
    pub const fn new() -> Self {
        SafeEjector { state: EjectState::Mounted, last_attempt_ms: None, retry_count: 0 }
    }

    /// 开始弹出（只有 Mounted 能发起——Ejecting 中重复请求不重复冲刷）。
    pub fn begin(&mut self) -> bool {
        if self.state != EjectState::Mounted {
            return false;
        }
        self.state = EjectState::Ejecting;
        true
    }

    /// 弹出失败（Ejecting 中才可能失败；记冷却锚）。
    pub fn fail(&mut self, at_ms: u64) -> bool {
        if self.state != EjectState::Ejecting {
            return false;
        }
        self.state = EjectState::Failed;
        self.last_attempt_ms = Some(at_ms);
        self.retry_count += 1;
        true
    }

    /// 重试（冷却未过拒绝——400ms 内狂点不重复冲刷；只从 Failed 发起）。
    pub fn retry(&mut self, at_ms: u64) -> bool {
        if self.state != EjectState::Failed {
            return false;
        }
        if let Some(t) = self.last_attempt_ms {
            if at_ms.saturating_sub(t) < EJECT_RETRY_COOLDOWN_MS {
                return false;
            }
        }
        self.state = EjectState::Ejecting;
        true
    }

    /// 弹出完成（Ejecting → Ejected——单向：Ejected 后只能重新挂载，
    /// 由调用方重建 SafeEjector——不假装「弹出了又还在」）。
    pub fn complete(&mut self) -> bool {
        if self.state != EjectState::Ejecting {
            return false;
        }
        self.state = EjectState::Ejected;
        true
    }

    /// 已安全弹出（设备区行消失的判据）。
    pub fn safely_ejected(&self) -> bool {
        self.state == EjectState::Ejected
    }
}

// ---------------------------------------------------------------------------
// 双语六宫格对账（v1 英文表 × v2 中文表逐位对齐）
// ---------------------------------------------------------------------------

/// 双语对齐审计：两表等长且每对 (英, 中) 非空（同源双语——索引漂移
/// = 界面两处说不一样的话，属缺陷）。
pub fn pinned_bilingual_aligned() -> bool {
    // v1 ThisPcPage::new() 的 pinned 表与 v2 PINNED_TABLE 逐位配对。
    let page = ThisPcPage::new();
    let en = page.pinned_folders();
    en.len() == PINNED_TABLE.len()
        && (0..PINNED_N).all(|i| !en[i].is_empty() && !PINNED_TABLE[i].is_empty())
        // 语义锚：S: 共享必在两表同位（主册点名项）。
        && en[3].starts_with("S:")
        && PINNED_TABLE[3].starts_with("S:")
}

// ---------------------------------------------------------------------------
// 持久化通道 v7（W7P1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7P 族）。
pub const PCPAGE_V7_MAGIC: [u8; 4] = *b"W7P1";
/// 长度：魔标(4) + 版本(1) + pinned 掩码(1) + 设备数(1) + 设备标签(4) +
/// FNV(4) = 16。
pub const PCPAGE_V7_LEN: usize = 16;
pub const PCPAGE_V7_VERSION: u8 = 1;

/// 序列化（pinned 掩码 bit i = 六宫格第 i 格用户置顶态；设备标签 4B）。
pub fn save_page_v7(pinned_mask: u8, dev_tags: [u8; 4], out: &mut [u8]) -> Option<usize> {
    if out.len() < PCPAGE_V7_LEN || pinned_mask >= (1u8 << PINNED_N) {
        return None; // 高位脏 = 坏掩码拒收（六格之外不许有假格）
    }
    out[..4].copy_from_slice(&PCPAGE_V7_MAGIC);
    out[4] = PCPAGE_V7_VERSION;
    out[5] = pinned_mask;
    out[6] = 0; // 保留
    out[7] = 0; // 保留
    out[8..12].copy_from_slice(&dev_tags);
    let h = fnv1a(&out[..12]);
    out[12] = (h & 0xff) as u8;
    out[13] = ((h >> 8) & 0xff) as u8;
    out[14] = ((h >> 16) & 0xff) as u8;
    out[15] = ((h >> 24) & 0xff) as u8;
    Some(PCPAGE_V7_LEN)
}

/// 反序列化（版本/掩码值域/保留位/FNV 四重守卫）。
pub fn load_page_v7(buf: &[u8]) -> Option<(u8, [u8; 4])> {
    if buf.len() < PCPAGE_V7_LEN || buf[..4] != PCPAGE_V7_MAGIC {
        return None;
    }
    if buf[4] != PCPAGE_V7_VERSION || buf[6] != 0 || buf[7] != 0 {
        return None;
    }
    if buf[5] >= (1u8 << PINNED_N) {
        return None;
    }
    let expect = fnv1a(&buf[..12]);
    let got = buf[12] as u32
        | ((buf[13] as u32) << 8)
        | ((buf[14] as u32) << 16)
        | ((buf[15] as u32) << 24);
    if expect != got {
        return None;
    }
    let mut tags = [0u8; 4];
    tags.copy_from_slice(&buf[8..12]);
    Some((buf[5], tags))
}

// ---------------------------------------------------------------------------
// 域自检（F456 v7）
// ---------------------------------------------------------------------------

pub fn run_pcpage_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F456-v7");
    // 1) 阈值审计 + 卷自洽。
    cs.add("threshold_audit", threshold_audit(), "");
    cs.add("volume_sane", volume_sane(850, 1_000) && !volume_sane(1_100, 1_000), "");
    cs.add("sane_permille_honest", {
        // used > total 的卷：千分比钳 1000 但 sane 拒——两层各司其职。
        let bad = Volume::new("BAD", 1_200, 1_000);
        bad.used_permille() == 1_000 && !volume_sane(bad.used_mb, bad.total_mb)
    }, "");
    // 2) 趋势环：涨幅计算 + 告警 + 负增长不告警 + 环上限。
    cs.add("trend_growth_alert", {
        let mut t = VolumeTrend::new();
        for v in [500u32, 520, 540, 560] {
            t.push(v);
        }
        t.growth_permille() == 60 && t.growth_alert()
    }, "");
    cs.add("trend_shrink_no_alert", {
        let mut t = VolumeTrend::new();
        for v in [800u32, 600, 400] {
            t.push(v);
        }
        !t.growth_alert() // 清理了空间——不吓人
    }, "");
    cs.add("trend_insufficient_honest", {
        let mut t = VolumeTrend::new();
        t.push(700);
        t.growth_permille() == 0 && !t.growth_alert()
    }, "");
    cs.add("trend_ring_cap", {
        let mut t = VolumeTrend::new();
        for i in 0..(VOLUME_TREND_CAP * 2) {
            t.push((i % 1_001) as u32);
        }
        t.count() == VOLUME_TREND_CAP
    }, "");
    cs.add("trend_clamped_input", {
        let mut t = VolumeTrend::new();
        t.push(2_000); // >1000‰ 钳制
        t.growth_permille() == 0 && t.count() == 1
    }, "");
    // 3) 安全弹出：全链 + 重复请求不重复冲刷 + 冷却 + 单向门。
    cs.add("eject_full_chain", {
        let mut e = SafeEjector::new();
        e.begin() && e.complete() && e.safely_ejected()
    }, "");
    cs.add("eject_double_begin_reject", {
        let mut e = SafeEjector::new();
        e.begin() && !e.begin() // Ejecting 中再 begin = false
    }, "");
    cs.add("eject_fail_then_cooldown", {
        let mut e = SafeEjector::new();
        let _ = e.begin();
        let _ = e.fail(1_000);
        !e.retry(1_200) // 200ms < 400ms 冷却——拒绝
            && e.retry(1_401) // 401ms 后放行
            && e.retry_count == 1
    }, "");
    cs.add("eject_state_transitions_strict", {
        let mut e = SafeEjector::new();
        !e.complete() // Mounted 直接 complete = false
            && {
                let mut e2 = SafeEjector::new();
                let _ = e2.begin();
                let _ = e2.fail(0);
                !e2.complete() // Failed 直接 complete = false（必须先 retry）
            }
    }, "");
    // 4) 双语六宫格对账。
    cs.add("pinned_bilingual_aligned", pinned_bilingual_aligned(), "");
    // 5) 持久化通道：round-trip + 掩码值域 + 篡改 + 保留位。
    let mut buf = [0u8; PCPAGE_V7_LEN];
    cs.add("persist_roundtrip", {
        let n = save_page_v7(0b00_1010, [1, 2, 3, 4], &mut buf).unwrap_or(0);
        load_page_v7(&buf[..n]) == Some((0b00_1010, [1, 2, 3, 4]))
    }, "");
    cs.add("persist_mask_over_range", save_page_v7(0b0100_0000, [0; 4], &mut buf).is_none(), "");
    cs.add("persist_tamper", {
        let n = save_page_v7(0b00_0011, [9, 9, 9, 9], &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[8] ^= 0x01;
        load_page_v7(&bad[..n]).is_none()
    }, "");
    cs.add("persist_reserved_set", {
        let mut bad = [0u8; PCPAGE_V7_LEN];
        let _ = save_page_v7(0, [0; 4], &mut bad);
        bad[6] = 1;
        load_page_v7(&bad).is_none()
    }, "");
    // 6) 页打开预算回归锚（v2 分解账的 v7 复核）。
    cs.add("budget_regression", budget_sum() == PAGE_OPEN_BUDGET_MS, "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn trend_oldest_is_true_oldest_after_wrap() {
        // 环回绕后「最旧」取自正确槽位（时间序不是索引序）。
        let mut t = VolumeTrend::new();
        for i in 0..VOLUME_TREND_CAP {
            t.push(i as u32 * 10); // 0,10,...,150
        }
        t.push(1_000); // 挤掉最旧 0
        // 窗口 = 10..1000 → 涨幅 990。
        assert_eq!(t.growth_permille(), 990);
    }

    #[test]
    fn ejector_retry_never_explodes() {
        let mut e = SafeEjector::new();
        let _ = e.begin();
        let _ = e.fail(0);
        // 冷却内狂点全部拒绝（计数诚实）。
        let rejected = (0..10u64).filter(|&i| !e.retry(i * 30)).count();
        assert_eq!(rejected, 10); // 0..270ms 全在冷却内
        assert!(e.retry(EJECT_RETRY_COOLDOWN_MS + 1));
    }

    #[test]
    fn persist_dev_tags_roundtrip() {
        let mut buf = [0u8; PCPAGE_V7_LEN];
        let n = save_page_v7(0, [7, 8, 9, 10], &mut buf).unwrap();
        let (_, tags) = load_page_v7(&buf[..n]).unwrap();
        assert_eq!(tags, [7, 8, 9, 10]);
    }

    #[test]
    fn threshold_ladder_still_f268() {
        // v1 判据回归：800/900 阶梯在 v7 检查面仍绿。
        assert_eq!(bar_level(800), BarLevel::Warn);
        assert_eq!(bar_level(900), BarLevel::Red);
    }
}
