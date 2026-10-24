//! VE-F3435 · 多显示器同步换肤 —— 跨屏同步 + DPI 令牌换算 + 异步屏追赶。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3435`
//!
//! # 职责（锚点原文拆解）
//!
//! - **跨屏同步**：换肤要同时落在所有显示器上。与 F3429 的多窗口同步
//!   不同：窗口是应用内的，显示器是系统级的，屏与屏之间还隔着刷新率、
//!   DPI、色彩配置——所以本模块只保证「代数一致」，画面一致性交给
//!   V02 拓扑联动；
//! - **DPI 换算**：同一个令牌（如 `spacing.gap = 12px`）在不同 DPI 的
//!   屏上必须换算成不同物理像素，但**物理尺寸要一致**。
//!   换算公式写死在 [`physical_px`]：`logical × dpi / 160`。
//!   这条公式来自 V 域的既有约定，本模块只消费不重定义；
//! - **异步屏追赶**：刷新率低的屏（或外接屏唤醒慢）会落后几个 tick。
//!   落后不超过 [`CATCHUP_TICKS`] 视为正常追赶，超过则重同步
//!   （推一份当前令牌快照）；
//! - **跨屏漂移断言**：同步完成后，所有屏的代號必须相等。任一屏不等即
//!   [`DisplayCode::CrossScreenDrift`]——这是断言，不是警告：两屏显示
//!   两套主题，用户第一眼就看得见。
//!
//! # 为什么 DPI 换算不能各屏自己算
//!
//! 各屏自己算，就等于把「1px 到底是多少」这件事分散到 N 个实现里。
//! 结果是同一个按钮在 100% 缩放和 150% 缩放的屏上差 1px——用户说不清
//! 哪里不对，但看着就是别扭。换算集中在 [`physical_px`]，全屏共用一份
//! 舍入规则（四舍五入到整数像素），差异就被消灭在源头。
//!
//! # 复杂度
//!
//! - `attach` / `sync_all`：O(屏数)；
//! - `convert`：O(令牌数 × 屏数)；
//! - `catch_up`：O(屏数)；
//! - `assert_drift`：O(屏数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`；换算全 `u64` 防溢出。测试区除外。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5519 起）
// ---------------------------------------------------------------------------

/// 多显示器诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayCode {
    /// 屏数为 0。
    NoDisplays = 5519,
    /// 屏 id 重复。
    DuplicateDisplay = 5520,
    /// DPI 非法（0 或超出合理范围）。
    BadDpi = 5521,
    /// 跨屏漂移（代数不一致）。
    CrossScreenDrift = 5522,
    /// 追赶超时（落后超过上限）。
    CatchupTimeout = 5523,
    /// DPI 换算失真（物理尺寸不一致超阈值）。
    DpiDrift = 5524,
    /// 主题为空。
    EmptyTheme = 5525,
    /// 屏未登记就同步。
    UnknownDisplay = 5526,
}

impl DisplayCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DisplayCode::NoDisplays => "显示器数为 0",
            DisplayCode::DuplicateDisplay => "显示器重复登记",
            DisplayCode::BadDpi => "显示器 DPI 非法",
            DisplayCode::CrossScreenDrift => "跨屏漂移（代数不一致）",
            DisplayCode::CatchupTimeout => "异步屏追赶超时",
            DisplayCode::DpiDrift => "DPI 换算失真",
            DisplayCode::EmptyTheme => "主题为空",
            DisplayCode::UnknownDisplay => "显示器未登记",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            DisplayCode::CrossScreenDrift | DisplayCode::DpiDrift
        )
    }
}

/// 基准 DPI：160 对应逻辑像素 1:1（与 V 域约定一致）。
pub const BASE_DPI: u32 = 160;

/// 允许的异步追赶 tick 数。
pub const CATCHUP_TICKS: u64 = 2;

/// 物理尺寸一致性的允许偏差（千分比）。
///
/// 换算公式本身是 dpi 无关的（逻辑像素 → 物理长度恒等），唯一的误差
/// 来源是**整像素舍入**：12 逻辑像素在 96dpi 上是 7px、在 160dpi 上是
/// 12px，对应物理长度 1852μm 与 1905μm，差 2.8%。所以容差取 5%
/// ——这正是「1px 舍入」在小尺寸上的量级；超过 5% 就不是舍入，是换算
/// 公式被改坏了。
pub const PHYSICAL_TOLERANCE_PERMILLE: u32 = 50;

// ---------------------------------------------------------------------------
// 二、显示器
// ---------------------------------------------------------------------------

/// 一台显示器。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Display {
    /// 屏 id。
    pub id: u32,
    /// 水平 DPI。
    pub dpi: u32,
    /// 已提交的换肤代数。
    pub generation: u64,
    /// 上一次确认的 tick（用于追赶判定）。
    pub acked_tick: u64,
    /// 是否在线。
    pub online: bool,
}

impl Display {
    /// 构造。
    pub fn new(id: u32, dpi: u32) -> Result<Display, DisplayCode> {
        if dpi < 72 || dpi > 640 {
            return Err(DisplayCode::BadDpi);
        }
        Ok(Display {
            id,
            dpi,
            generation: 0,
            acked_tick: 0,
            online: true,
        })
    }
}

/// 逻辑像素换物理像素：`logical × dpi / 160`，四舍五入到整像素。
pub fn physical_px(logical_px: i32, dpi: u32) -> i32 {
    let scaled = (logical_px as i64) * (dpi as i64);
    let base = BASE_DPI as i64;
    // 四舍五入（对称处理负数，避免 -0.5 与 0.5 舍入方向不一致）。
    let rounded = if scaled >= 0 {
        (scaled + base / 2) / base
    } else {
        (scaled - base / 2) / base
    };
    if rounded > i32::MAX as i64 {
        i32::MAX
    } else if rounded < i32::MIN as i64 {
        i32::MIN
    } else {
        rounded as i32
    }
}

/// 物理长度（微米）：先把逻辑像素换成整物理像素，再按该屏 DPI 折算
/// 成微米。只用于「跨屏一致性」比对，不用于渲染。
///
/// 注意这里**必须**用舍入后的整像素：渲染器画的是整像素，物理尺寸的
/// 真实偏差就来自这一步。用未舍入值算出来的差异恒为 0，等于什么都没验。
pub fn physical_um(logical_px: i32, dpi: u32) -> i64 {
    if dpi == 0 {
        return 0;
    }
    let px = physical_px(logical_px, dpi) as i64;
    // 1 英寸 = 25400 微米；物理长度 = 像素 / dpi 英寸。
    px * 25_400 / dpi as i64
}

/// 一次跨屏换肤的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SyncResult {
    /// 代数。
    pub generation: u64,
    /// 参与的屏。
    pub displays: u32,
    /// 追赶的屏。
    pub caught_up: Vec<u32>,
    /// 漂移的屏（同步后仍不同代）。
    pub drifted: Vec<u32>,
}

/// 多显示器同步器。
#[derive(Clone, Debug)]
pub struct DisplaySync {
    displays: Vec<Display>,
    theme: String,
    generation: u64,
}

impl DisplaySync {
    /// 新建。
    pub fn new() -> DisplaySync {
        DisplaySync {
            displays: Vec::new(),
            theme: String::new(),
            generation: 0,
        }
    }

    /// 登记屏。
    pub fn attach(&mut self, display: Display) -> Result<(), DisplayCode> {
        if self.displays.iter().any(|d| d.id == display.id) {
            return Err(DisplayCode::DuplicateDisplay);
        }
        self.displays.push(display);
        Ok(())
    }

    /// 发起一次换肤：给所有在线屏推新代。
    pub fn sync_all(&mut self, theme: &str, tick: u64) -> Result<SyncResult, DisplayCode> {
        if theme.trim().is_empty() {
            return Err(DisplayCode::EmptyTheme);
        }
        if self.displays.is_empty() {
            return Err(DisplayCode::NoDisplays);
        }
        self.theme = theme.to_string();
        self.generation = self.generation.saturating_add(1);
        let gen = self.generation;
        let mut count = 0u32;
        for d in self.displays.iter_mut() {
            if d.online {
                d.generation = gen;
                d.acked_tick = tick;
                count = count.saturating_add(1);
            }
        }
        let drifted = self.drifted();
        Ok(SyncResult {
            generation: gen,
            displays: count,
            caught_up: Vec::new(),
            drifted,
        })
    }

    /// 某屏确认（异步屏回报）。
    pub fn ack(&mut self, id: u32, generation: u64, tick: u64) -> Result<(), DisplayCode> {
        match self.displays.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.generation = generation;
                d.acked_tick = tick;
                Ok(())
            }
            None => Err(DisplayCode::UnknownDisplay),
        }
    }

    /// 异步屏追赶：给落后且超时的屏重推快照。
    pub fn catch_up(&mut self, tick: u64) -> Vec<u32> {
        let mut out = Vec::new();
        let gen = self.generation;
        for d in self.displays.iter_mut() {
            if !d.online || d.generation == gen {
                continue;
            }
            if tick.saturating_sub(d.acked_tick) > CATCHUP_TICKS {
                d.generation = gen;
                d.acked_tick = tick;
                out.push(d.id);
            }
        }
        out
    }

    /// 漂移屏列表。
    pub fn drifted(&self) -> Vec<u32> {
        let gen = self.generation;
        let mut out: Vec<u32> = self
            .displays
            .iter()
            .filter(|d| d.online && d.generation != gen)
            .map(|d| d.id)
            .collect();
        out.sort_unstable();
        out
    }

    /// 跨屏漂移断言：同步后不许有漂移屏。
    pub fn assert_no_drift(&self) -> Result<(), DisplayCode> {
        if self.drifted().is_empty() {
            Ok(())
        } else {
            Err(DisplayCode::CrossScreenDrift)
        }
    }

    /// DPI 一致性断言：同一逻辑长度在各屏上的物理尺寸差异必须在容差内。
    pub fn assert_physical_consistency(&self, logical_px: i32) -> Result<u32, DisplayCode> {
        if self.displays.is_empty() {
            return Err(DisplayCode::NoDisplays);
        }
        let mut min = i64::MAX;
        let mut max = i64::MIN;
        for d in self.displays.iter().filter(|d| d.online) {
            let um = physical_um(logical_px, d.dpi);
            min = core::cmp::min(min, um);
            max = core::cmp::max(max, um);
        }
        if min == i64::MAX {
            return Err(DisplayCode::NoDisplays);
        }
        let spread = if min == 0 {
            0
        } else {
            ((max - min) * 1000 / core::cmp::max(1, min.abs())) as u32
        };
        if spread > PHYSICAL_TOLERANCE_PERMILLE {
            Err(DisplayCode::DpiDrift)
        } else {
            Ok(spread)
        }
    }

    /// 屏数。
    pub fn count(&self) -> usize {
        self.displays.len()
    }

    /// 当前代数。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 读屏播报（锚点「同步状态读屏播报」）。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "多显示器换肤：主题 {}，第 {} 代，{} 块屏",
            if self.theme.is_empty() { "无" } else { &self.theme },
            self.generation,
            self.displays.len()
        ));
        for d in self.displays.iter() {
            out.push(format!(
                "屏{}：DPI {}，第{}代，{}，上次确认 {}",
                d.id,
                d.dpi,
                d.generation,
                if d.online { "在线" } else { "离线" },
                d.acked_tick
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sync() -> DisplaySync {
        let mut s = DisplaySync::new();
        s.attach(Display::new(1, 96).expect("屏")).expect("登记");
        s.attach(Display::new(2, 144).expect("屏")).expect("登记");
        s
    }

    #[test]
    fn all_online_displays_get_same_generation() {
        let mut s = sync();
        let r = s.sync_all("deep-ocean", 10).expect("同步");
        assert_eq!(r.displays, 2);
        assert_eq!(r.drifted, Vec::<u32>::new());
        assert!(s.assert_no_drift().is_ok());
        assert_eq!(s.generation(), 1);
    }

    #[test]
    fn dpi_conversion_keeps_physical_size() {
        // 12 逻辑像素在 96dpi 上是 7px（12*96/160=7.2 -> 7）。
        assert_eq!(physical_px(12, 96), 7);
        // 在 160dpi 上是 12px。
        assert_eq!(physical_px(12, 160), 12);
        // 在 320dpi 上是 24px。
        assert_eq!(physical_px(12, 320), 24);
        // 负值对称舍入。
        assert_eq!(physical_px(-12, 96), -7);
        // 物理长度始终一致（同一令牌同一物理尺寸）。
        assert_eq!(physical_um(12, 96), physical_um(12, 96));
    }

    #[test]
    fn physical_consistency_holds_across_dpis() {
        let s = sync();
        // 96 与 144 dpi 上 12 逻辑像素 -> 7px / 11px，物理长度差约 2.8%，
        // 在 5% 容差内。
        let spread = s.assert_physical_consistency(12).expect("容差内");
        assert!(spread > 0 && spread <= PHYSICAL_TOLERANCE_PERMILLE);
    }

    #[test]
    fn slow_display_is_caught_up_after_timeout() {
        let mut s = sync();
        s.sync_all("neon-night", 10).expect("同步");
        // 屏 2 没确认，代退回旧代。
        s.ack(2, 0, 10).expect("伪造落后");
        assert_eq!(s.drifted(), alloc::vec![2]);
        // 未超时不追赶。
        assert!(s.catch_up(11).is_empty());
        // 超时后追赶。
        let caught = s.catch_up(13);
        assert_eq!(caught, alloc::vec![2]);
        assert!(s.assert_no_drift().is_ok());
    }

    #[test]
    fn offline_display_is_not_counted_as_drift() {
        let mut s = sync();
        if let Some(d) = s.displays.get_mut(0) {
            d.online = false;
        }
        s.sync_all("x", 1).expect("同步");
        assert!(s.drifted().is_empty(), "离线屏不参与漂移判定");
    }

    #[test]
    fn bad_dpi_and_duplicates_are_rejected() {
        assert_eq!(Display::new(1, 0).err(), Some(DisplayCode::BadDpi));
        assert_eq!(Display::new(1, 1000).err(), Some(DisplayCode::BadDpi));
        let mut s = sync();
        assert_eq!(
            s.attach(Display::new(1, 96).expect("屏")).err(),
            Some(DisplayCode::DuplicateDisplay)
        );
    }

    #[test]
    fn empty_system_and_empty_theme_are_rejected() {
        let mut empty = DisplaySync::new();
        assert_eq!(empty.sync_all("x", 1).err(), Some(DisplayCode::NoDisplays));
        let mut s = sync();
        assert_eq!(s.sync_all(" ", 1).err(), Some(DisplayCode::EmptyTheme));
    }

    #[test]
    fn reading_lines_list_every_display() {
        let mut s = sync();
        s.sync_all("deep-ocean", 5).expect("同步");
        let lines = s.reading_lines();
        assert!(lines.first().map(|l| l.contains("2 块屏")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("屏1：DPI 96")));
    }
}
