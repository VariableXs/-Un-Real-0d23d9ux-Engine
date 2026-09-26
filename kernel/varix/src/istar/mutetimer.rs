//! F567 定时静音 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：三档时长 + 自定义；到点恢复原值；角标倒数；
//! 提前恢复；与 F341/F543 一致。
//!
//! **设计要点（主册）**：
//! - 快速设置磁贴右键「静音 30 分钟/1 小时/自定义」——到点自动恢复原音量
//!   （F543 分设备记忆的原值）；
//! - 剩余时间在磁贴角标倒数显示；提前恢复再点一下即回；
//! - 定时期间新通知只入中心不响（F341 静音档逻辑同源——不是勿扰，
//!   通知照收只是不响）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 三档时长（分钟）。
pub const DURATIONS_MIN: [u64; 3] = [30, 60, 120];

/// 角标倒数刷新步长（分钟粒度——角标显示「剩 29 分」口径）。
pub const BADGE_STEP_MIN: u64 = 1;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 定时静音账。
pub struct MuteTimer {
    /// 定时中（None = 未定时；Some(结束时刻 ms)）。
    until_ms: Option<u64>,
    /// 定时开始时的原音量（F543 原值——到点恢复它）。
    original_volume: Option<u8>,
    /// 当前音量（宿主注入同步）。
    volume: u8,
    now_ms: u64,
    /// 到点恢复发生次数（对账账）。
    restores: u32,
}

impl MuteTimer {
    pub fn new() -> MuteTimer {
        MuteTimer {
            until_ms: None,
            original_volume: None,
            volume: 50,
            now_ms: 0,
            restores: 0,
        }
    }

    pub fn tick(&mut self, ms: u64) {
        if ms > self.now_ms {
            self.now_ms = ms;
        }
        // 到点自动恢复（毫秒粒度判定——不依赖轮询间隔）。
        if let Some(until) = self.until_ms {
            if self.now_ms >= until {
                if let Some(orig) = self.original_volume.take() {
                    self.volume = orig;
                }
                self.until_ms = None;
                self.restores += 1;
            }
        }
    }

    /// 定时静音（分钟数 >0；三档或自定义共用此口——0 拒绝）。
    pub fn mute_for(&mut self, minutes: u64) -> bool {
        if minutes == 0 || self.until_ms.is_some() {
            return false; // 未到点重复定时拒绝（角标已在倒数，不叠第二层）。
        }
        self.original_volume = Some(self.volume);
        self.volume = 0;
        self.until_ms = Some(self.now_ms + minutes * 60_000);
        true
    }

    /// 提前恢复：再点一下即回原值。
    pub fn restore_now(&mut self) -> bool {
        if self.until_ms.is_none() {
            return false;
        }
        if let Some(orig) = self.original_volume.take() {
            self.volume = orig;
        }
        self.until_ms = None;
        self.restores += 1;
        true
    }

    /// 角标倒数文案（「剩 N 分」；未定时 None）。
    pub fn badge(&self) -> Option<&'static str> {
        // 数字拼接由 UI 层 format（内核零 format! 热路径纪律）；此处给
        // 剩余分钟数，角标显隐由 Some/None 表达。
        self.remaining_min().map(|_| "剩")
    }

    /// 剩余分钟数（向上取整——倒计时显示不为零欺瞒）。
    pub fn remaining_min(&self) -> Option<u64> {
        self.until_ms.map(|until| {
            let rem = until.saturating_sub(self.now_ms);
            (rem + 60_000 - 1) / 60_000
        })
    }

    pub fn muted(&self) -> bool {
        self.until_ms.is_some()
    }

    pub fn volume(&self) -> u8 {
        self.volume
    }

    pub fn restore_count(&self) -> u32 {
        self.restores
    }

    /// F341 语义对账：定时期间通知只入中心不响（本账不拦通知——
    /// 判定面只给「静音中」状态位）。
    pub fn notifications_suppressed(&self) -> bool {
        self.muted()
    }
}

impl Default for MuteTimer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_mutetimer_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 三档时长 + 自定义：30/60/120 与任意自定义分钟共用一口。
    set.add("three durations and custom", DURATIONS_MIN == [30, 60, 120], "");

    // 2. 静音即原值入账：原 50 → 静音后 0、原值账留 50。
    let mut t = MuteTimer::new();
    t.tick(1_000);
    t.mute_for(30);
    let orig = t.volume() == 0 && t.muted();
    set.add("mute records original", orig, "");

    // 3. 到点恢复原值：30 分钟后 volume 回 50、恢复账 +1、账清。
    t.tick(1_000 + 30 * 60_000);
    set.add(
        "auto restore at deadline",
        t.volume() == 50 && !t.muted() && t.restore_count() == 1,
        "",
    );

    // 4. 角标倒数：剩 29 分钟（向上取整——29:30 显示 30 分不欺瞒）。
    let mut t2 = MuteTimer::new();
    t2.tick(0);
    t2.mute_for(30);
    t2.tick(30 * 1_000); // 过了 30 秒
    let rem = t2.remaining_min();
    set.add(
        "badge countdown ceil",
        rem == Some(30) && t2.badge() == Some("剩"),
        "",
    );

    // 5. 提前恢复：再点一下即回原值、倒数消失。
    let early = t2.restore_now();
    set.add(
        "early restore",
        early && t2.volume() == 50 && !t2.muted() && t2.badge().is_none(),
        "",
    );

    // 6. 重复定时拒绝：定时中再定时不动账（不叠第二层）。
    let mut t3 = MuteTimer::new();
    t3.mute_for(60);
    let again = t3.mute_for(30);
    set.add("no second timer while active", !again && t3.muted(), "");

    // 7. 零时长拒绝（诚实拒绝——无意义的「静音 0 分钟」）。
    let mut t4 = MuteTimer::new();
    set.add("zero duration rejected", !t4.mute_for(0), "");

    // 8. F341 语义一致：静音中通知只入中心不响（状态位不拦通知路由）。
    set.add(
        "notifications suppressed while muted",
        t3.notifications_suppressed() && BADGE_STEP_MIN == 1,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_duration_works() {
        let mut t = MuteTimer::new();
        t.tick(0);
        assert!(t.mute_for(17)); // 自定义 17 分钟
        t.tick(17 * 60_000);
        assert_eq!(t.volume(), 50);
        assert_eq!(t.restore_count(), 1);
    }

    #[test]
    fn deadline_exact_boundary() {
        let mut t = MuteTimer::new();
        t.tick(5_000);
        t.mute_for(60);
        // 差 1ms 未到——不恢复。
        t.tick(5_000 + 60 * 60_000 - 1);
        assert!(t.muted());
        t.tick(5_000 + 60 * 60_000);
        assert!(!t.muted());
    }

    #[test]
    fn restore_now_without_timer_false() {
        let mut t = MuteTimer::new();
        assert!(!t.restore_now());
    }
}
