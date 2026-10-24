//! VE-F3429 · 多窗口同步换肤 —— 主窗发起全窗跟随 + 同步原子性 + 离群检测重同步。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3429`
//!
//! # 职责（锚点原文拆解）
//!
//! - **全窗口跟随**：主窗口发起换肤，其余窗口跟随。跟随不是「各自换」——
//!   每个窗口用同一个代号（[`Generation`]）提交，代号相同才算同一次换肤；
//! - **同步原子性**：不存在「半新半旧」的多窗口状态。做法是两阶段：
//!   先让所有窗口进入 `Prepared`（准备好、尚未生效），全部就绪后才统一
//!   `Commit`。任一窗口准备失败或超时 → 全体回滚（[`SyncCode::AtomicityBroken`]）。
//!   先提交再补漏的「最终一致」在换肤场景里不可接受：用户会同时看到
//!   两套主题，且不知道哪套是真的；
//! - **离群窗口检测**：某窗口的主题代号落后于当前代号即离群
//!   （[`WindowState::Orphan`]）。离群原因通常是窗口刚创建、或上次同步
//!   丢了一条消息。检测到就重同步（把当前窗口的令牌快照推给它），
//!   而不是让它自己再拉一遍；
//! - **回滚全体**：原子性被破坏时，已提交的窗口也要退回上一代。
//!   只回滚一部分等于把「半新半旧」变成「半新半旧 + 一次闪烁」。
//!
//! # 为什么用「代号」而不是「主题名」判断同步
//!
//! 主题名相同不代表同一次换肤：用户可能切到 A、切回 B、再切到 A。
//! 主题名一路是 A，但中间那次 B 的窗口没跟上，它的渲染缓存就脏了。
//! 代号单调递增，只增不改，等价于「这是第几次换肤」——用它判定，
//! 离群窗口一个都跑不掉。
//!
//! # 复杂度
//!
//! - `prepare` / `commit`：O(窗口数)；
//! - `detect_orphans`：O(窗口数)；
//! - `resync`：O(1)（推快照，不重算）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5474 起）
// ---------------------------------------------------------------------------

/// 多窗口同步诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncCode {
    /// 窗口集为空。
    NoWindows = 5474,
    /// 代号回退（新一代必须大于当前代）。
    GenerationBackwards = 5475,
    /// 有窗口未准备就绪就提交。
    NotPrepared = 5476,
    /// 原子性被破坏（部分窗口已提交）。
    AtomicityBroken = 5477,
    /// 窗口 id 重复。
    DuplicateWindow = 5478,
    /// 无窗口可回滚。
    NothingToRollback = 5479,
    /// 主窗口不在窗口集中。
    MasterMissing = 5480,
    /// 重同步目标不存在。
    NoSuchWindow = 5481,
    /// 主题为空。
    EmptyTheme = 5482,
}

impl SyncCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            SyncCode::NoWindows => "窗口集为空",
            SyncCode::GenerationBackwards => "换肤代号回退",
            SyncCode::NotPrepared => "窗口未准备就绪就提交",
            SyncCode::AtomicityBroken => "同步原子性被破坏",
            SyncCode::DuplicateWindow => "窗口重复登记",
            SyncCode::NothingToRollback => "无窗口可回滚",
            SyncCode::MasterMissing => "主窗口不在窗口集",
            SyncCode::NoSuchWindow => "窗口不存在",
            SyncCode::EmptyTheme => "主题为空",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, SyncCode::AtomicityBroken)
    }
}

/// 换肤代号：单调递增，等价于「第几次换肤」。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Generation(pub u64);

/// 窗口状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowState {
    /// 跟随中（已提交当前代号）。
    InSync,
    /// 已准备、未提交。
    Prepared,
    /// 离群（代号落后）。
    Orphan,
    /// 提交失败。
    Failed,
}

impl WindowState {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            WindowState::InSync => "已同步",
            WindowState::Prepared => "已准备",
            WindowState::Orphan => "离群",
            WindowState::Failed => "失败",
        }
    }
}

/// 一个窗口。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Window {
    /// 窗口 id。
    pub id: u32,
    /// 当前已提交代号。
    pub committed: Generation,
    /// 正在准备的目标代号。
    pub preparing: Option<Generation>,
    /// 状态。
    pub state: WindowState,
    /// 最近一次同步 tick。
    pub synced_at: u64,
}

impl Window {
    /// 构造。
    pub fn new(id: u32, generation: Generation) -> Window {
        Window {
            id,
            committed: generation,
            preparing: None,
            state: WindowState::InSync,
            synced_at: 0,
        }
    }
}

/// 同步结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SyncOutcome {
    /// 目标代号。
    pub generation: Generation,
    /// 是否成功提交。
    pub committed: bool,
    /// 参与的窗口数。
    pub windows: u32,
    /// 是否发生全体回滚。
    pub rolled_back: bool,
}

impl SyncOutcome {
    /// 一行渲染。
    pub fn headline(&self) -> String {
        format!(
            "第{}代换肤：{}，{} 个窗口{}",
            self.generation.0,
            if self.committed { "已提交" } else { "未提交" },
            self.windows,
            if self.rolled_back { "，已全体回滚" } else { "" }
        )
    }
}

// ---------------------------------------------------------------------------
// 二、同步协议
// ---------------------------------------------------------------------------

/// 多窗口同步协议机。
#[derive(Clone, Debug)]
pub struct SyncProtocol {
    windows: Vec<Window>,
    master: u32,
    current: Generation,
    /// 上一代（回滚目标）。
    previous: Generation,
    theme: String,
}

impl SyncProtocol {
    /// 建协议。至少一个窗口，主窗口必须在集合里。
    pub fn new(master: u32, windows: &[u32], generation: Generation, theme: &str) -> Result<SyncProtocol, SyncCode> {
        if windows.is_empty() {
            return Err(SyncCode::NoWindows);
        }
        if theme.trim().is_empty() {
            return Err(SyncCode::EmptyTheme);
        }
        let mut sorted = windows.to_vec();
        sorted.sort_unstable();
        let mut i = 0;
        while i + 1 < sorted.len() {
            if sorted.get(i) == sorted.get(i + 1) {
                return Err(SyncCode::DuplicateWindow);
            }
            i += 1;
        }
        if !windows.contains(&master) {
            return Err(SyncCode::MasterMissing);
        }
        let mut list = Vec::new();
        for id in windows.iter() {
            list.push(Window::new(*id, generation));
        }
        Ok(SyncProtocol {
            windows: list,
            master,
            current: generation,
            previous: generation,
            theme: theme.to_string(),
        })
    }

    /// 第一阶段：让所有窗口准备到新一代。
    pub fn prepare(&mut self, generation: Generation, tick: u64) -> Result<u32, SyncCode> {
        if generation <= self.current {
            return Err(SyncCode::GenerationBackwards);
        }
        let mut count = 0u32;
        for w in self.windows.iter_mut() {
            w.preparing = Some(generation);
            w.state = WindowState::Prepared;
            w.synced_at = tick;
            count = count.saturating_add(1);
        }
        Ok(count)
    }

    /// 某个窗口准备失败（渲染目标不可用等）。
    pub fn fail_prepare(&mut self, id: u32, tick: u64) -> Result<(), SyncCode> {
        for w in self.windows.iter_mut() {
            if w.id == id {
                w.state = WindowState::Failed;
                w.preparing = None;
                w.synced_at = tick;
                return Ok(());
            }
        }
        Err(SyncCode::NoSuchWindow)
    }

    /// 第二阶段：全体提交。任一窗口未准备即拒（原子性闸）。
    pub fn commit(&mut self, generation: Generation, tick: u64) -> Result<SyncOutcome, SyncCode> {
        if generation <= self.current {
            return Err(SyncCode::GenerationBackwards);
        }
        let all_prepared = self
            .windows
            .iter()
            .all(|w| w.state == WindowState::Prepared && w.preparing == Some(generation));
        if !all_prepared {
            // 有窗口没准备好：一个都不许提交，并记原子性破坏。
            return Err(SyncCode::AtomicityBroken);
        }
        for w in self.windows.iter_mut() {
            w.committed = generation;
            w.preparing = None;
            w.state = WindowState::InSync;
            w.synced_at = tick;
        }
        self.previous = self.current;
        self.current = generation;
        Ok(SyncOutcome {
            generation,
            committed: true,
            windows: self.windows.len() as u32,
            rolled_back: false,
        })
    }

    /// 全体回滚到上一代（原子性被破坏后的处置）。
    pub fn rollback_all(&mut self, tick: u64) -> Result<SyncOutcome, SyncCode> {
        if self.windows.is_empty() {
            return Err(SyncCode::NothingToRollback);
        }
        for w in self.windows.iter_mut() {
            w.committed = self.previous;
            w.preparing = None;
            w.state = WindowState::InSync;
            w.synced_at = tick;
        }
        self.current = self.previous;
        Ok(SyncOutcome {
            generation: self.current,
            committed: false,
            windows: self.windows.len() as u32,
            rolled_back: true,
        })
    }

    /// 离群检测：代号落后于当前代即离群。
    pub fn detect_orphans(&mut self) -> Vec<u32> {
        let current = self.current;
        let mut out = Vec::new();
        for w in self.windows.iter_mut() {
            if w.committed < current {
                w.state = WindowState::Orphan;
                out.push(w.id);
            }
        }
        out.sort_unstable();
        out
    }

    /// 重同步：把离群窗口拉到当前代。返回被拉回的窗口。
    pub fn resync(&mut self, tick: u64) -> Vec<u32> {
        let current = self.current;
        let mut out = Vec::new();
        for w in self.windows.iter_mut() {
            if w.committed < current {
                w.committed = current;
                w.state = WindowState::InSync;
                w.synced_at = tick;
                out.push(w.id);
            }
        }
        out
    }

    /// 当前代。
    pub fn current(&self) -> Generation {
        self.current
    }

    /// 主窗口。
    pub fn master(&self) -> u32 {
        self.master
    }

    /// 主题。
    pub fn theme(&self) -> &str {
        &self.theme
    }

    /// 窗口列表（只读）。
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// 状态读屏播报（锚点「同步状态读屏播报」）。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "多窗口同步：主题 {}，当前第 {} 代，主窗口 {}，共 {} 个窗口",
            self.theme,
            self.current.0,
            self.master,
            self.windows.len()
        ));
        for w in self.windows.iter() {
            out.push(format!(
                "窗口{}：{}，已提交第{}代{}",
                w.id,
                w.state.label(),
                w.committed.0,
                match w.preparing {
                    Some(g) => format!("，正在准备第{}代", g.0),
                    None => String::new(),
                }
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

    fn proto() -> SyncProtocol {
        SyncProtocol::new(1, &[1, 2, 3], Generation(7), "daylight").expect("协议")
    }

    #[test]
    fn all_windows_follow_master_in_one_generation() {
        let mut p = proto();
        assert_eq!(p.prepare(Generation(8), 10).ok(), Some(3));
        let out = p.commit(Generation(8), 11).expect("提交");
        assert!(out.committed);
        assert_eq!(out.windows, 3);
        assert_eq!(p.current(), Generation(8));
        assert!(p.windows().iter().all(|w| w.committed == Generation(8)));
    }

    #[test]
    fn one_failed_window_breaks_atomicity_and_nothing_commits() {
        let mut p = proto();
        p.prepare(Generation(8), 10).expect("准备");
        p.fail_prepare(2, 10).expect("失败");
        let err = p.commit(Generation(8), 11).err().expect("必拒");
        assert_eq!(err, SyncCode::AtomicityBroken);
        assert!(err.blocking());
        // 没有窗口被提交：仍是第 7 代。
        assert!(p.windows().iter().all(|w| w.committed == Generation(7)));
    }

    #[test]
    fn rollback_returns_every_window_to_previous_generation() {
        let mut p = proto();
        p.prepare(Generation(8), 10).expect("准备");
        p.commit(Generation(8), 11).expect("提交");
        let out = p.rollback_all(12).expect("回滚");
        assert!(out.rolled_back);
        assert_eq!(p.current(), Generation(7));
        assert!(p.windows().iter().all(|w| w.committed == Generation(7)));
    }

    #[test]
    fn orphan_detection_and_resync() {
        let mut p = proto();
        p.prepare(Generation(8), 10).expect("准备");
        p.commit(Generation(8), 11).expect("提交");
        // 新窗口加入时落后一代（模拟新开窗口）。
        if let Some(w) = p.windows.iter_mut().find(|w| w.id == 3) {
            w.committed = Generation(7);
        }
        let orphans = p.detect_orphans();
        assert_eq!(orphans, alloc::vec![3]);
        let resynced = p.resync(12);
        assert_eq!(resynced, alloc::vec![3]);
        assert!(p.detect_orphans().is_empty());
    }

    #[test]
    fn generation_must_move_forward() {
        let mut p = proto();
        assert_eq!(
            p.prepare(Generation(7), 1).err(),
            Some(SyncCode::GenerationBackwards)
        );
        assert_eq!(
            p.commit(Generation(6), 1).err(),
            Some(SyncCode::GenerationBackwards)
        );
    }

    #[test]
    fn duplicate_or_masterless_window_set_is_rejected() {
        assert_eq!(
            SyncProtocol::new(1, &[1, 1], Generation(1), "t").err(),
            Some(SyncCode::DuplicateWindow)
        );
        assert_eq!(
            SyncProtocol::new(9, &[1, 2], Generation(1), "t").err(),
            Some(SyncCode::MasterMissing)
        );
        assert_eq!(
            SyncProtocol::new(1, &[], Generation(1), "t").err(),
            Some(SyncCode::NoWindows)
        );
    }

    #[test]
    fn reading_lines_report_each_window() {
        let mut p = proto();
        p.prepare(Generation(8), 10).expect("准备");
        let lines = p.reading_lines();
        assert!(lines.first().map(|l| l.contains("第 7 代")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("已准备") && l.contains("第8代")));
    }
}
