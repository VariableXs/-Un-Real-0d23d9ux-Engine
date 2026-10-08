//! F183 存储健康监测 · 批次三深化（secstar · G-G-13）。
//!
//! 批次三功能面（主册判据「对拍 ±2% / 掉电续计 / 三段阈值」的实现纵深）：
//! - [`parse_smart_attr`]：SMART 属性原始字节解析——12B 属性帧（ID/标志/
//!   值/最差/阈值/原始 6B）逐字段拆解，坏帧诚实拒（不在垃圾上做判断）；
//! - [`WearEstimator`]：磨损估计——TBW 累计 / 额定写入寿命 → 剩余寿命
//!   千分比（TBW_MODEL_CITATION 引用面直算）；
//! - [`TempHistory`]：温度环账——最近 N 次读数 + 峰值/均值（趋势可见，
//!   不只报瞬时）；
//! - [`ReminderEscalator`]：提醒升级器——黄段月频提醒连续 3 次未处理 →
//!   升级为强提醒（绿段零打扰红线不破）。
//!
//! 零堆纪律：定长环 + 定长帧，无 alloc。

use super::diskhealth::{band_of, Band, TBW_MODEL_CITATION};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// SMART 属性解析（12B 帧逐字段）
// ---------------------------------------------------------------------------

/// SMART 属性帧长度（标准 12 字节布局）。
pub const ATTR_FRAME_LEN: usize = 12;

/// 解析后的属性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartAttr {
    pub id: u8,
    pub value: u8,
    pub worst: u8,
    pub threshold: u8,
    /// 原始 6 字节小端（不同属性语义不同——保留原始，解释交给调用方）。
    pub raw: [u8; 6],
}

/// 解析 12B 属性帧。坏帧（全零/阈值>100）诚实拒——ATTR 0x00 不是属性。
pub fn parse_smart_attr(frame: &[u8; ATTR_FRAME_LEN]) -> Option<SmartAttr> {
    if frame[0] == 0 {
        return None;
    }
    let threshold = frame[5];
    if threshold > 100 {
        return None;
    }
    let mut raw = [0u8; 6];
    raw.copy_from_slice(&frame[6..12]);
    Some(SmartAttr { id: frame[0], value: frame[3], worst: frame[4], threshold, raw })
}

/// 属性健康判断：value 低于 threshold → 越线（ATTR 语义：越高越好）。
pub fn attr_breached(a: &SmartAttr) -> bool {
    a.value < a.threshold
}

// ---------------------------------------------------------------------------
// 磨损估计
// ---------------------------------------------------------------------------

/// 磨损估计器：额定寿命写入量（TB——来自型号标称，TBW_MODEL_CITATION）
/// 对照累计写入（WriteAccountant 的 bytes_total——一处一事实）。
#[derive(Clone, Copy, Debug)]
pub struct WearEstimator {
    /// 额定寿命写入量（KiB——定点点对齐整数纪律）。
    pub rated_kib: u64,
}

impl WearEstimator {
    /// 剩余寿命千分比（1000=全新；下限钳 0——负寿命不出现）。
    pub fn remaining_permille(&self, written_bytes: u64) -> u32 {
        let written_kib = written_bytes / 1024;
        if self.rated_kib == 0 {
            return 0;
        }
        let used_permille = (written_kib.saturating_mul(1_000)) / self.rated_kib.max(1);
        1_000u32.saturating_sub(used_permille as u32)
    }

    /// 剩余寿命落段（复用主层三段阈值——主层尺量的是**已用**占比，
    /// 这里把剩余换算成已用再过尺，同一把尺不另造线）。
    pub fn band(&self, written_bytes: u64) -> Band {
        band_of(1_000 - self.remaining_permille(written_bytes))
    }

    /// 预计可用天数（按日均写入速率线性外推——趋势诚实，不承诺精确）。
    pub fn projected_days(&self, written_bytes: u64, daily_bytes: u64) -> Option<u64> {
        if daily_bytes == 0 {
            return None;
        }
        let remaining_bytes = self.rated_kib.saturating_sub(written_bytes / 1024).saturating_mul(1024);
        Some(remaining_bytes / daily_bytes)
    }
}

// ---------------------------------------------------------------------------
// 温度环账
// ---------------------------------------------------------------------------

pub const TEMP_RING_CAP: usize = 16;

/// 温度读数环（摄氏度整数——主控分辨率的诚实上限）。
pub struct TempHistory {
    ring: [i16; TEMP_RING_CAP],
    n: usize,
    head: usize,
}

impl TempHistory {
    pub const fn new() -> TempHistory {
        TempHistory { ring: [0; TEMP_RING_CAP], n: 0, head: 0 }
    }

    pub fn push(&mut self, celsius: i16) {
        self.ring[self.head] = celsius;
        self.head = (self.head + 1) % TEMP_RING_CAP;
        if self.n < TEMP_RING_CAP {
            self.n += 1;
        }
    }

    /// 峰值（趋势上探——散热事件可见）。
    pub fn peak(&self) -> Option<i16> {
        (0..self.n).map(|i| self.ring[i]).max()
    }

    /// 均值（整除口径——i16 环内平均，趋势量级足够）。
    pub fn mean(&self) -> Option<i16> {
        if self.n == 0 {
            return None;
        }
        let sum: i32 = (0..self.n).map(|i| self.ring[i] as i32).sum();
        Some((sum / self.n as i32) as i16)
    }

    /// 最新读数。
    pub fn latest(&self) -> Option<i16> {
        if self.n == 0 {
            return None;
        }
        Some(self.ring[(self.head + TEMP_RING_CAP - 1) % TEMP_RING_CAP])
    }
}

// ---------------------------------------------------------------------------
// 提醒升级器（黄段月频的升级线）
// ---------------------------------------------------------------------------

/// 升级阈值：黄段提醒连续 3 次被关掉 → 升强提醒。
pub const ESCALATE_AFTER: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReminderLevel {
    /// 常规（月频 toast——既有层）。
    Normal,
    /// 强提醒（横幅+设置页红点——升级后）。
    Escalated,
}

pub struct ReminderEscalator {
    dismissed_streak: u8,
    pub level: ReminderLevel,
}

impl ReminderEscalator {
    pub const fn new() -> ReminderEscalator {
        ReminderEscalator { dismissed_streak: 0, level: ReminderLevel::Normal }
    }

    /// 用户关掉一次提醒（绿段调用方不该调——升级器只管黄/红段）。
    pub fn on_dismiss(&mut self) {
        self.dismissed_streak += 1;
        if self.dismissed_streak >= ESCALATE_AFTER {
            self.level = ReminderLevel::Escalated;
        }
    }

    /// 用户处理了（进入设置页查看健康页）——连击清零，级别保留降级口。
    pub fn on_addressed(&mut self) {
        self.dismissed_streak = 0;
        self.level = ReminderLevel::Normal;
    }

    pub fn streak(&self) -> u8 {
        self.dismissed_streak
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_diskhealth_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F183-b3");

    // 1) SMART 解析：合法帧逐字段对（ID/值/最差/阈值/原始 6B）。
    let mut frame = [0u8; ATTR_FRAME_LEN];
    frame[0] = 5; // Reallocated Sector Count
    frame[3] = 90; // value
    frame[4] = 90; // worst
    frame[5] = 10; // threshold
    frame[6] = 0x11;
    frame[11] = 0x66;
    let a = parse_smart_attr(&frame).unwrap();
    cs.add(
        "smart_parse_fields",
        a.id == 5 && a.value == 90 && a.worst == 90 && a.threshold == 10 && a.raw[0] == 0x11 && a.raw[5] == 0x66,
        "",
    );

    // 2) 坏帧诚实拒：全零帧（无属性）与阈值>100 帧都拒。
    let zero = [0u8; ATTR_FRAME_LEN];
    let mut bad = frame;
    bad[5] = 200;
    cs.add("smart_reject_bad_frames", parse_smart_attr(&zero).is_none() && parse_smart_attr(&bad).is_none(), "");

    // 3) 越线判断：value < threshold 即越线（90<10 假 / 5<10 真）。
    let mut low = frame;
    low[3] = 5;
    let low_attr = parse_smart_attr(&low).unwrap();
    cs.add("smart_breach_detected", !attr_breached(&a) && attr_breached(&low_attr), "");

    // 4) 磨损估计：零写入=1000、半寿命=500、超寿命钳 0（三态同尺）。
    let w = WearEstimator { rated_kib: 1_000_000 };
    cs.add(
        "wear_permille_ladder",
        w.remaining_permille(0) == 1_000
            && w.remaining_permille(500_000 * 1024) == 500
            && w.remaining_permille(u64::MAX) == 0,
        "",
    );

    // 5) 磨损落段复用主层三段尺（同一把尺——不另造线）。
    let fresh = w.band(0);
    // 900 GiB 写入 ≫ 976 MiB 级额定 → 剩余 0 → 红段（磨损尽）。
    let worn = w.band(900 * 1024 * 1024 * 1024);
    cs.add("wear_band_reuses_scale", matches!(fresh, Band::Green) && matches!(worn, Band::Red), "");

    // 6) 剩余天数外推：日均速率给出行数（零速率诚实 None——不编造）。
    cs.add(
        "wear_projected_days",
        w.projected_days(0, 1024 * 1024) == Some(1_000_000 * 1024 / (1024 * 1024)) && w.projected_days(0, 0).is_none(),
        "",
    );

    // 7) 温度环：峰值/均值/最新三读数全对（趋势三面）。
    let mut t = TempHistory::new();
    for v in [40i16, 45, 52, 48] {
        t.push(v);
    }
    cs.add("temp_stats", t.peak() == Some(52) && t.mean() == Some(46) && t.latest() == Some(48), "");

    // 8) 温度环回卷：17 次推入只留最近 16（环满覆盖——内存上限纪律）。
    let mut t2 = TempHistory::new();
    for i in 0..17i16 {
        t2.push(30 + i);
    }
    cs.add("temp_ring_wraps", t2.peak() == Some(46) && t2.latest() == Some(46), "");

    // 9) 空环不猜：三读数全 None（无数据不出数）。
    let t3 = TempHistory::new();
    cs.add("temp_empty_none", t3.peak().is_none() && t3.mean().is_none() && t3.latest().is_none(), "");

    // 10) 提醒升级：连关 3 次 → 强提醒（升级线生效）。
    let mut r = ReminderEscalator::new();
    r.on_dismiss();
    r.on_dismiss();
    let still_normal = r.level == ReminderLevel::Normal;
    r.on_dismiss();
    cs.add("reminder_escalates", still_normal && r.level == ReminderLevel::Escalated && r.streak() == 3, "");

    // 11) 提醒处理复位：连击清零、级别回落（处理即Reset——不永远强提醒）。
    r.on_addressed();
    cs.add("reminder_addressed_resets", r.level == ReminderLevel::Normal && r.streak() == 0, "");

    // 12) TBW 引用面贯通：citation 在册且非空（模型来源标注不缺席）。
    cs.add("tbw_citation_present", !TBW_MODEL_CITATION.is_empty(), "");

    // 13) 主层尺贯通：band_of 边界值（主册口径——已用 <60% 绿 / <85%
    // 黄 / ≥85% 红）与主层同源（主层 band_of 逐点对拍）。
    cs.add(
        "band_edges_same_scale",
        matches!(band_of(0), Band::Green)
            && matches!(band_of(599), Band::Green)
            && matches!(band_of(600), Band::Yellow)
            && matches!(band_of(849), Band::Yellow)
            && matches!(band_of(850), Band::Red)
            && matches!(band_of(999), Band::Red),
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn smart_frame_roundtrip_parse() {
        // 12B 帧逐字段写入再读出，全字段保真（解析不丢字节）。
        let mut f = [0xA5u8; ATTR_FRAME_LEN];
        f[0] = 197;
        f[3] = 100;
        f[4] = 78;
        f[5] = 0;
        let a = parse_smart_attr(&f).unwrap();
        assert_eq!(a.id, 197);
        assert_eq!(a.worst, 78);
        assert_eq!(a.raw, [0xA5; 6]);
        // 阈值 0 → 永不越线（0 阈值语义）。
        assert!(!attr_breached(&a));
    }

    #[test]
    fn wear_projection_monotone() {
        // 写得越多剩余越少（单调性——估计器不精神分裂）。
        let w = WearEstimator { rated_kib: 1_000_000 };
        let mut prev = 1_001;
        for mib in [0u64, 100, 200, 300, 400] {
            let rem = w.remaining_permille(mib * 1024 * 1024);
            assert!(rem < prev, "mib={mib} rem={rem}");
            prev = rem;
        }
    }

    #[test]
    fn temp_ring_bounded_memory() {
        // 1000 次推入内存不涨（环账上限——长跑不膨胀）。
        let mut t = TempHistory::new();
        for i in 0..1000i16 {
            t.push(i);
        }
        assert_eq!(t.latest(), Some(999));
        assert!(t.peak().unwrap() >= 984); // 最后 16 个的最大值
    }
}
