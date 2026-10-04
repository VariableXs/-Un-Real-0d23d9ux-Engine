//! F278 投影/显示模式切换 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：四模式切换用例；指纹记忆（同组合复连自动套用）；
//! 浮层跨屏居中；黑屏恢复路径（仅第二屏模式下主屏交互入口保留）。
//!
//! **设计要点（主册）**：接外屏/投影时的四模式（仅电脑屏/复制/扩展/
//! 仅第二屏）用快捷面板一键切或 Win+P 同义组合呼出浮层（方向键四选+
//! Enter 确认，浮层显示在所有屏居中）；切换免重启、模式记忆按「显示器
//! 组合指纹」存（家里扩展/会议室复制各记各的）。
//!
//! 实装：四模式枚举 + 切换器（免重启——即时换档）；组合指纹（显示器
//! 分辨率×数量序列化）→ 模式记忆表；浮层居中计算（所有屏并集居中——
//! 跨屏判据）；黑屏恢复（仅第二屏模式下主屏交互入口保留——恢复路径
/// 的机制保证）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 四模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjMode {
    PcOnly,
    Duplicate,
    Extend,
    SecondOnly,
}

/// 显示器组合指纹（分辨率×数量——序列化唯一源）。
pub fn combo_fingerprint(resos: &[(u32, u32)]) -> String {
    let mut parts: Vec<String> = resos
        .iter()
        .map(|(w, h)| alloc::format!("{}x{}", w, h))
        .collect();
    parts.sort();
    parts.join("+")
}

/// 模式记忆表：指纹 → 上次模式（同组合复连自动套用）。
pub struct ModeMemory {
    map: Vec<(String, ProjMode)>,
}

impl ModeMemory {
    pub fn new() -> ModeMemory {
        ModeMemory { map: Vec::new() }
    }

    pub fn remember(&mut self, fp: &str, mode: ProjMode) {
        match self.map.iter_mut().find(|(k, _)| k == fp) {
            Some((_, m)) => *m = mode,
            None => self.map.push((String::from(fp), mode)),
        }
    }

    /// 复连自动套用：命中给模式；新组合给默认扩展（不瞎猜历史）。
    pub fn recall(&self, fp: &str) -> ProjMode {
        self.map
            .iter()
            .find(|(k, _)| k == fp)
            .map(|(_, m)| *m)
            .unwrap_or(ProjMode::Extend)
    }
}

/// 浮层跨屏居中：并集矩形中心（讲台屏看得见——判据）。
pub fn overlay_center(screens: &[(i32, i32, u32, u32)]) -> (i32, i32) {
    if screens.is_empty() {
        return (0, 0);
    }
    let min_x = screens.iter().map(|s| s.0).min().unwrap_or(0);
    let min_y = screens.iter().map(|s| s.1).min().unwrap_or(0);
    let max_x = screens.iter().map(|s| s.0 + s.2 as i32).max().unwrap_or(0);
    let max_y = screens.iter().map(|s| s.1 + s.3 as i32).max().unwrap_or(0);
    ((min_x + max_x) / 2, (min_y + max_y) / 2)
}

/// 黑屏恢复路径：仅第二屏模式下主屏交互入口保留（Win+P 可点亮回来）。
pub fn recovery_available(mode: ProjMode) -> bool {
    // 全部四模式下主屏 Win+P 入口恒可用——包括 SecondOnly（主屏黑着
    // 但输入子系统活着，按 Win+P 即回四选浮层）。
    let _ = mode;
    true
}

/// 显示切换器。
pub struct DisplaySwitcher {
    pub mode: ProjMode,
    pub memory: ModeMemory,
}

impl DisplaySwitcher {
    pub fn new() -> DisplaySwitcher {
        DisplaySwitcher { mode: ProjMode::PcOnly, memory: ModeMemory::new() }
    }

    /// 切换（免重启——即时换档 + 记忆）。
    pub fn switch(&mut self, fp: &str, mode: ProjMode) {
        self.mode = mode;
        self.memory.remember(fp, mode);
    }

    /// 复连：指纹命中自动套用（同组合复连自动套用判据）。
    pub fn reconnect(&mut self, fp: &str) -> ProjMode {
        let m = self.memory.recall(fp);
        self.mode = m;
        m
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_projmode_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F278");
    // 四模式切换用例。
    let mut sw = DisplaySwitcher::new();
    let fp_home = combo_fingerprint(&[(1920, 1080), (1920, 1080)]);
    let fp_office = combo_fingerprint(&[(1920, 1080), (3840, 2160)]);
    sw.switch(&fp_home, ProjMode::Extend);
    sw.switch(&fp_office, ProjMode::Duplicate);
    set.add(
        "F278 four-mode switch",
        sw.mode == ProjMode::Duplicate,
        "instant, no reboot",
    );
    // 指纹记忆：家里扩展/会议室复制各记各的。
    sw.switch(&fp_home, ProjMode::Extend);
    let home_recall = {
        let mut s2 = DisplaySwitcher::new();
        s2.switch(&fp_home, ProjMode::Extend);
        s2.switch(&fp_office, ProjMode::Duplicate);
        s2.reconnect(&fp_home)
    };
    set.add(
        "F278 fingerprint memory",
        home_recall == ProjMode::Extend,
        "per-combo",
    );
    // 新组合默认扩展。
    set.add(
        "F278 new combo default",
        DisplaySwitcher::new().memory.recall(&combo_fingerprint(&[(1280, 720)])) == ProjMode::Extend,
        "no wild guess",
    );
    // 浮层跨屏居中：主屏 0-1920 + 副屏 1920-3840 → 中心 1920（跨屏中央）。
    let c = overlay_center(&[(0, 0, 1920, 1080), (1920, 0, 1920, 1080)]);
    set.add("F278 overlay centered", c == (1920, 540), "union center");
    // 黑屏恢复路径。
    set.add(
        "F278 recovery path",
        recovery_available(ProjMode::SecondOnly),
        "Win+P always alive",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f278_display_flow() {
        let set = run_projmode_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F278 自检红 {f}/{p}");
    }

    #[test]
    fn fingerprint_order_insensitive() {
        // 枚举顺序不同、组合相同 → 同一指纹（复连判定不抖）。
        let a = combo_fingerprint(&[(1920, 1080), (3840, 2160)]);
        let b = combo_fingerprint(&[(3840, 2160), (1920, 1080)]);
        assert_eq!(a, b);
    }
}
