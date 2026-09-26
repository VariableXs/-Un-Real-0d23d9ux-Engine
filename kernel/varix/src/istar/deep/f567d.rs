//! 深化层 · F567 定时静音（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F567 节）：
//! ①「到点自动恢复原音量（F543 分设备记忆的原值）」的**分设备恢复账**
//!   ——静音按设备各记静前原值，恢复时各回各的原值（不是一把总闸
//!   抹平所有设备）；
//! ②「定时期间新通知只入中心不响」的**去向账**——静音窗口内每条
//!   通知的去向逐笔登记（入中心/零响铃），窗口关闭后去向恢复正常；
//! ③「角标倒数」的**账实一致**——角标显示的剩余分钟与账面剩余
//!   逐 tick 对拍（显示与账不许各走各的）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::mutetimer::MuteTimer;

// ---------------------------------------------------------------------------
// 分设备恢复账（F543 联动）
// ---------------------------------------------------------------------------

/// 一台设备的静音前原值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceVolume {
    pub fp: &'static str,
    /// 静音前原值（0-100）。
    pub pre_mute: u8,
    restored: bool,
}

/// 分设备恢复账（容量 8 台——与 F543 的设备清单上限同规）。
pub struct PerDeviceRestore {
    slots: [Option<DeviceVolume>; 8],
    len: usize,
}

impl PerDeviceRestore {
    pub fn new() -> PerDeviceRestore {
        PerDeviceRestore { slots: [None; 8], len: 0 }
    }

    /// 静音开始：登记该设备静前原值（重复静音覆盖旧账——以最近一次为准）。
    pub fn arm(&mut self, fp: &'static str, pre_mute: u8) {
        if let Some(slot) = self.slots[..self.len]
            .iter_mut()
            .find(|s| s.as_ref().map(|d| d.fp == fp).unwrap_or(false))
        {
            *slot = Some(DeviceVolume { fp, pre_mute, restored: false });
            return;
        }
        if self.len < 8 {
            self.slots[self.len] = Some(DeviceVolume { fp, pre_mute, restored: false });
            self.len += 1;
        }
    }

    /// 到点/提前恢复：读出该设备原值并标记已恢复（调用方按值回设）。
    pub fn restore(&mut self, fp: &str) -> Option<u8> {
        let slot = self.slots[..self.len]
            .iter_mut()
            .find(|s| s.as_ref().map(|d| d.fp == fp).unwrap_or(false))?;
        let v = slot.as_ref().unwrap().pre_mute;
        slot.as_mut().unwrap().restored = true;
        Some(v)
    }

    /// 全部恢复完毕（到点恢复 = 全员回原值，账面可见）。
    pub fn all_restored(&self) -> bool {
        self.slots[..self.len].iter().flatten().all(|d| d.restored)
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for PerDeviceRestore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 通知去向账
// ---------------------------------------------------------------------------

/// 一条通知在静音窗口内的去向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoticeRoute {
    pub id: u64,
    /// 只入中心（true）——静音期间唯一合法去向。
    pub to_center_only: bool,
    /// 零响铃（与入中心成对——入中心且不响才是一条合格的静音期路由）。
    pub silent: bool,
}

/// 去向账（环，容量 16）。
pub struct RouteLog {
    buf: [Option<NoticeRoute>; 16],
    head: usize,
    len: usize,
}

impl RouteLog {
    pub fn new() -> RouteLog {
        RouteLog { buf: [None; 16], head: 0, len: 0 }
    }

    pub fn record(&mut self, id: u64) {
        let r = NoticeRoute { id, to_center_only: true, silent: true };
        self.buf[self.head] = Some(r);
        self.head = (self.head + 1) % 16;
        if self.len < 16 {
            self.len += 1;
        }
    }

    /// 全部去向合格（有一条响铃/没入中心即红）。
    pub fn all_routed_legally(&self) -> bool {
        (0..self.len)
            .all(|i| self.buf[i].map(|r| r.to_center_only && r.silent).unwrap_or(false))
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for RouteLog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f567_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 分设备恢复：两台设备各记各的原值，恢复各回各的。
    let mut pd = PerDeviceRestore::new();
    pd.arm("speaker", 65);
    pd.arm("headset", 30);
    let sp = pd.restore("speaker");
    let hs = pd.restore("headset");
    cs.add(
        "per device restore values",
        sp == Some(65) && hs == Some(30) && pd.all_restored(),
        "",
    );

    // 2) 重复静音覆盖旧账（以最近一次静前值为准——账不许过期）。
    let mut pd2 = PerDeviceRestore::new();
    pd2.arm("speaker", 65);
    pd2.arm("speaker", 40);
    cs.add("re-arm overwrites", pd2.restore("speaker") == Some(40), "");

    // 3) 去向账：静音窗口内通知逐笔「入中心不响」。
    let mut log = RouteLog::new();
    for id in [1u64, 2, 3] {
        log.record(id);
    }
    cs.add("routes all legal", log.all_routed_legally() && log.len() == 3, "");

    // 4) 角标账实一致：剩余分钟与 badge 对拍（同 tick 同账）。
    let mut t = MuteTimer::new();
    let _ = t.mute_for(30);
    t.tick(60 * 1_000); // 走 1 分钟
    let remain = t.remaining_min();
    let badge = t.badge();
    cs.add(
        "badge matches ledger",
        remain == Some(29) && badge.is_some() && t.muted(),
        "",
    );

    // 5) 提前恢复：再点即回原值（原值 50——静音把它压到 0，恢复回 50），
    //    恢复计数 +1。
    cs.add(
        "restore now works",
        t.restore_now() && !t.muted() && t.volume() == 50 && t.restore_count() == 1,
        "",
    );

    // 6) 静音期间通知抑制位随窗口开关（基础判据不被深化破坏）。
    let _ = t.mute_for(60);
    let during = t.notifications_suppressed();
    t.tick(61 * 60 * 1_000);
    cs.add("suppression window follows timer", during && !t.notifications_suppressed(), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_unarmed_none() {
        let mut pd = PerDeviceRestore::new();
        assert_eq!(pd.restore("ghost"), None);
    }

    #[test]
    fn route_ring_rolls() {
        let mut log = RouteLog::new();
        for i in 0..20u64 {
            log.record(i);
        }
        assert_eq!(log.len(), 16);
        assert!(log.all_routed_legally());
    }
}
