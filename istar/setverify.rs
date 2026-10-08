//! F599 升级后设置校验 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：关键设置清单覆盖；快照比对准确；漂移通知与一键恢复；
//! 逐项查看；全绿静默判据。
//!
//! **设计要点（主册）**：
//! - 系统更新（F122/F574 链路）完成后的设置完整性校验：升级首登后台
//!   静默比对用户关键设置（主题 F151/快捷键表 F244/默认应用 F345/壁纸/
//!   任务栏布局）与升级前快照——全部一致静默通过；
//! - 发现漂移（设置被重置）即时通知（「检测到 N 项设置在更新后变化
//!   ——一键恢复」）；恢复动作逐项可查；
//! - 升级不该偷走用户的设置——这是对「升级即重配」老毛病的结构性拒绝。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 关键设置清单（枚举即覆盖清单——升级前快照按此五类全量采集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySetting {
    /// 主题（F151 令牌档）。
    Theme,
    /// 快捷键表（F244 用户改键）。
    Hotkeys,
    /// 默认应用（F345 关联）。
    DefaultApps,
    /// 壁纸（深浅双槽）。
    Wallpaper,
    /// 任务栏布局。
    Taskbar,
}

pub const KEY_SETTINGS: [KeySetting; 5] = [
    KeySetting::Theme,
    KeySetting::Hotkeys,
    KeySetting::DefaultApps,
    KeySetting::Wallpaper,
    KeySetting::Taskbar,
];

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一类设置的快照值（哈希指纹——内容变更即指纹变）。
pub type Fingerprint = u64;

/// FNV-1a 指纹（设置串→64 位——快照比对唯一算法）。
pub fn fingerprint(value: &str) -> Fingerprint {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in value.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// 校验器。
pub struct SetVerify {
    /// 升级前快照（类 → 指纹）。
    snapshot: [(KeySetting, Fingerprint); 5],
    snap_len: usize,
    /// 升级后实测（类 → 指纹）。
    observed: [(KeySetting, Fingerprint); 5],
    obs_len: usize,
    /// 漂移清单（比对产出——一键恢复的输入）。
    drifted: Vec<KeySetting>,
    /// 恢复动作账（逐项可查）。
    restored: Vec<KeySetting>,
    /// 用户是否点了「不再提醒本批漂移」（尊重用户选择——但恢复入口仍在）。
    acknowledged: bool,
}

impl SetVerify {
    pub fn new() -> SetVerify {
        SetVerify {
            snapshot: [(); 5].map(|_| (KeySetting::Theme, 0)),
            snap_len: 0,
            observed: [(); 5].map(|_| (KeySetting::Theme, 0)),
            obs_len: 0,
            drifted: Vec::new(),
            restored: Vec::new(),
            acknowledged: false,
        }
    }

    /// 升级前快照采集（五类全覆盖——清单覆盖判据）。
    pub fn snapshot(&mut self, kind: KeySetting, value: &str) -> bool {
        if self.snap_len >= 5 || value.is_empty() {
            return false;
        }
        self.snapshot[self.snap_len] = (kind, fingerprint(value));
        self.snap_len += 1;
        true
    }

    /// 升级后实测采集。
    pub fn observe(&mut self, kind: KeySetting, value: &str) -> bool {
        if self.obs_len >= 5 || value.is_empty() {
            return false;
        }
        self.observed[self.obs_len] = (kind, fingerprint(value));
        self.obs_len += 1;
        true
    }

    /// 快照比对（升级首登后台静默跑——指纹逐类对照）。
    ///
    /// 返回漂移类清单；空 = 全绿（静默通过，用户零感知）。
    pub fn compare(&mut self) -> Vec<KeySetting> {
        self.drifted.clear();
        for i in 0..self.snap_len {
            let (kind, want) = self.snapshot[i];
            let got = self.observed[..self.obs_len]
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, v)| *v);
            match got {
                Some(v) if v != want => self.drifted.push(kind),
                None => self.drifted.push(kind), // 实测缺类 = 漂移（诚实比对）
                _ => {}
            }
        }
        self.drifted.clone()
    }

    /// 全绿静默判据：零漂移且快照五类全采（缺类快照不签发全绿——
    /// 防漏采假静默）。
    pub fn silent_pass(&self) -> bool {
        self.drifted.is_empty() && self.snapshot_complete()
    }

    /// 漂移通知文案（三要素：发生了什么/影响/下一步）。
    pub fn notice_text(&self) -> Option<String> {
        if self.drifted.is_empty() || self.acknowledged {
            return None;
        }
        Some(alloc::format!(
            "检测到 {} 项设置在更新后变化——一键恢复",
            self.drifted.len()
        ))
    }

    /// 一键恢复（全部漂移项回快照值——恢复动作逐项入账）。
    pub fn restore_all(&mut self) -> usize {
        let n = self.drifted.len();
        for k in self.drifted.drain(..) {
            self.restored.push(k);
        }
        n
    }

    /// 逐项查看：某类设置是否被恢复过（恢复动作可查账）。
    pub fn was_restored(&self, kind: KeySetting) -> bool {
        self.restored.contains(&kind)
    }

    /// 快照覆盖审计：五类全采才能比对（缺类快照不签发全绿——防漏采
    /// 假静默）。
    pub fn snapshot_complete(&self) -> bool {
        self.snap_len == KEY_SETTINGS.len()
    }
}

impl Default for SetVerify {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_setverify_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 关键设置清单覆盖：五类齐（枚举即清单）。
    set.add(
        "five key settings covered",
        KEY_SETTINGS.len() == 5
            && KEY_SETTINGS.contains(&KeySetting::Theme)
            && KEY_SETTINGS.contains(&KeySetting::Hotkeys)
            && KEY_SETTINGS.contains(&KeySetting::DefaultApps),
        "",
    );

    // 2. 快照比对准确：全一致 → 零漂移（全绿静默）；主题被重置 → 漂移 1。
    //    全绿例：五类快照、五类实测同值。
    let mut v = SetVerify::new();
    v.snapshot(KeySetting::Theme, "dark-token-v2");
    v.snapshot(KeySetting::Hotkeys, "user-remap-table");
    v.snapshot(KeySetting::DefaultApps, "browser=edge-like");
    v.snapshot(KeySetting::Wallpaper, "dark:夜.png");
    v.snapshot(KeySetting::Taskbar, "layout-v1");
    for (k, val) in [
        (KeySetting::Theme, "dark-token-v2"),
        (KeySetting::Hotkeys, "user-remap-table"),
        (KeySetting::DefaultApps, "browser=edge-like"),
        (KeySetting::Wallpaper, "dark:夜.png"),
        (KeySetting::Taskbar, "layout-v1"),
    ] {
        v.observe(k, val);
    }
    let all_green = v.compare().is_empty() && v.silent_pass();
    // 漂移例：主题被升级重置为缺省。
    let mut v1 = SetVerify::new();
    v1.snapshot(KeySetting::Theme, "dark-token-v2");
    v1.snapshot(KeySetting::Hotkeys, "user-remap-table");
    v1.snapshot(KeySetting::DefaultApps, "browser=edge-like");
    v1.snapshot(KeySetting::Wallpaper, "dark:夜.png");
    v1.snapshot(KeySetting::Taskbar, "layout-v1");
    v1.observe(KeySetting::Theme, "default-light");
    v1.observe(KeySetting::Hotkeys, "user-remap-table");
    v1.observe(KeySetting::DefaultApps, "browser=edge-like");
    v1.observe(KeySetting::Wallpaper, "dark:夜.png");
    v1.observe(KeySetting::Taskbar, "layout-v1");
    let drifted_one = v1.compare().len() == 1 && !v1.silent_pass();
    set.add(
        "compare accurate and silent pass",
        all_green && drifted_one,
        "",
    );

    // 3. 漂移通知与一键恢复：通知文案带项数；一键恢复清空漂移账。
    let notice = v1.notice_text();
    let restored_n = v1.restore_all();
    set.add(
        "notice and one key restore",
        notice.as_ref().map(|t| t.contains("1 项")).unwrap_or(false)
            && restored_n == 1
            && v1.drifted.is_empty(),
        "",
    );

    // 4. 逐项查看：恢复动作逐项入账可查。
    set.add(
        "per item restore ledger",
        v1.was_restored(KeySetting::Theme) && !v1.was_restored(KeySetting::Taskbar),
        "",
    );

    // 5. 快照缺类不签发全绿（防漏采假静默——silent_pass 内建完整性门）。
    let mut v2 = SetVerify::new();
    v2.snapshot(KeySetting::Theme, "a");
    v2.observe(KeySetting::Theme, "a");
    let incomplete = v2.compare().is_empty() && !v2.silent_pass() && !v2.snapshot_complete();
    set.add(
        "incomplete snapshot blocks silent green",
        incomplete,
        "",
    );

    // 6. 实测缺类 = 漂移（升级吃掉设置项也算被偷）。
    let mut v3 = SetVerify::new();
    for k in KEY_SETTINGS {
        v3.snapshot(k, "val");
    }
    v3.observe(KeySetting::Theme, "val");
    v3.observe(KeySetting::Taskbar, "val");
    let missing = v3.compare().len() == 3;
    set.add(
        "missing observation counts as drift",
        missing,
        "",
    );

    // 7. 指纹算法稳定：同串同签、异串异签（比对唯一算法单源）。
    set.add(
        "fingerprint stable",
        fingerprint("abc") == fingerprint("abc") && fingerprint("abc") != fingerprint("abd"),
        "",
    );

    // 8. 全绿零通知（静默是礼仪——无漂移时 notice_text 不出）。
    let mut v4 = SetVerify::new();
    v4.snapshot(KeySetting::Theme, "x");
    v4.observe(KeySetting::Theme, "x");
    let _ = v4.compare();
    set.add(
        "all green zero notice",
        v4.notice_text().is_none(),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_value_snapshot_rejected() {
        let mut v = SetVerify::new();
        assert!(!v.snapshot(KeySetting::Theme, ""));
        assert!(!v.observe(KeySetting::Theme, ""));
    }

    #[test]
    fn restore_with_no_drift_zero() {
        let mut v = SetVerify::new();
        assert_eq!(v.restore_all(), 0);
    }

    #[test]
    fn acknowledge_suppresses_notice() {
        let mut v = SetVerify::new();
        v.snapshot(KeySetting::Theme, "a");
        v.observe(KeySetting::Theme, "b");
        let _ = v.compare();
        v.acknowledged = true;
        assert!(v.notice_text().is_none());
    }
}
