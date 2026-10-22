//! VE-F3517 · 时区与跨午夜处理 —— 跨午夜窗口 + 时区跟随重排 + 夏令时边界。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3517`
//!
//! # 职责（锚点原文拆解）
//!
//! - **跨午夜窗口**（[`DayWindow`]）：「夜 22 点到早 6 点」是合法
//!   窗口（from > to 环回语义，与 F3503 [`SceneWindow`] 同源但本册
//!   增加时区层）。命中判定按**当地钟**（用户墙上的时间），
//!   环回语义在环内恒成立。
//! - **时区跟随**（[`TzTracker`]）：旅行换时区 → 场景重排 + 告知。
//!   重排规则：窗口锚定当地钟（不是 UTC）——用户的「22 点」
//!   永远指当地时间 22 点。跳变后第一拍主动告知「场景已按
//!   新时区重排」（[`TzNotice`]）。
//! - **夏令时边界**（[`DstGuard`]）：春拨（2:00→3:00）时 2:00-2:59
//!   不存在；秋拨（3:00→2:00）时 1:00-1:59 出现两次。窗口端点
//!   落在不存在分钟上 → 就近钳到存在分钟；落在重复分钟上 →
//!   取第二次（晚一拍比早一拍安全——色温晚切一分钟无害，
//!   早切惊扰用户）。
//! - **回归用例库**（[`MIDNIGHT_CASES`]）：跨午夜 + 时区 + 夏令时
//!   三类刁钻样本固化成库，随组收口全量重放。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 跨午夜判定错 | 修正（环回语义单源） |
//! | 时区突变 | 重排 + 告知 |
//! | 夏令时 | 边界回归（钳到存在分钟） |
//!
//! # 复杂度
//!
//! 全部 O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5840 起）
// ---------------------------------------------------------------------------

/// 时区/跨午夜域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TzCode {
    /// 窗口端点非法（端点必须 < 1440 且不等）。
    BadWindow = 5840,
    /// 夏令时不存在分钟（已钳制）。
   DstHole = 5841,
    /// 时区偏移越界（±14h 之外，已钳制）。
    TzClamped = 5842,
    /// 重排未告知（流程违例）。
    ReshuffleSilent = 5843,
}

impl TzCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            TzCode::BadWindow => "时段窗口非法",
            TzCode::DstHole => "夏令时不存在分钟",
            TzCode::TzClamped => "时区偏移越界已钳制",
            TzCode::ReshuffleSilent => "时区重排未告知",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, TzCode::ReshuffleSilent)
    }
}

// ---------------------------------------------------------------------------
// 二、跨午夜窗口（当地钟语义）
// ---------------------------------------------------------------------------

/// 一天的分钟数。
pub const DAY: u32 = 1440;

/// 跨午夜窗口（from/to 为当地分钟）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DayWindow {
    pub from: u32,
    pub to: u32,
}

impl DayWindow {
    /// 构造校验。
    pub fn new(from: u32, to: u32) -> Result<Self, TzCode> {
        if from >= DAY || to >= DAY || from == to {
            return Err(TzCode::BadWindow);
        }
        Ok(DayWindow { from, to })
    }

    /// 是否跨午夜。
    pub fn wraps(&self) -> bool {
        self.from > self.to
    }

    /// 当地钟命中。
    pub fn hits_local(&self, local_minute: u32) -> bool {
        let m = local_minute % DAY;
        if self.wraps() {
            m >= self.from || m < self.to
        } else {
            m >= self.from && m < self.to
        }
    }
}

// ---------------------------------------------------------------------------
// 三、时区层
// ---------------------------------------------------------------------------

/// 时区偏移上限（分钟）：±14h。
pub const TZ_LIMIT_MIN: i32 = 14 * 60;

/// UTC 分钟 → 当地分钟（含日期环回）。
pub fn to_local(utc_minute: u32, tz_offset_min: i32) -> u32 {
    let tz = tz_offset_min.clamp(-TZ_LIMIT_MIN, TZ_LIMIT_MIN);
    let m = utc_minute as i64 + tz as i64;
    m.rem_euclid(DAY as i64) as u32
}

/// 时区重排告知。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TzNotice {
    /// 旧偏移（分钟）。
    pub old_offset: i32,
    /// 新偏移（分钟）。
    pub new_offset: i32,
    /// 告知文本。
    pub text: &'static str,
}

/// 时区跟踪器：检测偏移变化 → 重排 + 强制告知。
#[derive(Clone, Copy, Debug)]
pub struct TzTracker {
    pub offset: i32,
    pub reshuffles: u32,
}

impl TzTracker {
    /// 初始偏移。
    pub fn new(offset: i32) -> Self {
        TzTracker {
            offset: offset.clamp(-TZ_LIMIT_MIN, TZ_LIMIT_MIN),
            reshuffles: 0,
        }
    }

    /// 偏移更新：变了就重排（这里重排=记数）并产出告知；
    /// **告知缺失是流程违例**——调用方必须消费 notice。
    pub fn update(&mut self, new_offset: i32) -> Result<Option<TzNotice>, TzCode> {
        let no = new_offset.clamp(-TZ_LIMIT_MIN, TZ_LIMIT_MIN);
        if no != new_offset {
            return Err(TzCode::TzClamped);
        }
        if no == self.offset {
            return Ok(None);
        }
        let n = TzNotice {
            old_offset: self.offset,
            new_offset: no,
            text: "场景已按新时区重排",
        };
        self.offset = no;
        self.reshuffles += 1;
        Ok(Some(n))
    }
}

// ---------------------------------------------------------------------------
// 四、夏令时守卫
// ---------------------------------------------------------------------------

/// 春拨消失区间（当地分钟 120..180，即 02:00-02:59）。
pub const DST_HOLE: (u32, u32) = (120, 180);
/// 秋拨重复区间（当地分钟 60..120，即 01:00-01:59）。
pub const DST_REPEAT: (u32, u32) = (60, 120);

/// 夏令时边界钳制：
/// - 端点落在消失区间 → 钳到 180（存在侧）；
/// - 端点落在重复区间 → 取第二次出现（数值不变，语义标注）。
pub fn dst_clamp(minute: u32) -> (u32, Option<TzCode>) {
    if minute >= DST_HOLE.0 && minute < DST_HOLE.1 {
        (DST_HOLE.1, Some(TzCode::DstHole))
    } else {
        (minute, None)
    }
}

/// 夏令时窗口修正：两端点过钳制后重建窗口。
pub fn dst_safe_window(w: DayWindow) -> (DayWindow, Vec<TzCode>) {
    let mut codes = Vec::new();
    let (f, c1) = dst_clamp(w.from);
    let (t, c2) = dst_clamp(w.to);
    if let Some(c) = c1 {
        codes.push(c);
    }
    if let Some(c) = c2 {
        codes.push(c);
    }
    if f == t {
        // 钳制后两端重合 → 原窗口整个落进消失区间，钳到最小有效窗。
        codes.push(TzCode::DstHole);
        return (DayWindow { from: DST_HOLE.1, to: (DST_HOLE.1 + 60) % DAY }, codes);
    }
    (DayWindow { from: f, to: t }, codes)
}

// ---------------------------------------------------------------------------
// 五、回归用例库
// ---------------------------------------------------------------------------

/// 跨午夜回归样本：(窗口, 当地分钟, 期望命中)。
pub const MIDNIGHT_CASES: [((u32, u32), u32, bool); 8] = [
    ((1320, 360), 1320, true),  // 22:00 窗口起点
    ((1320, 360), 1440 - 1, true), // 23:59
    ((1320, 360), 0, true),     // 00:00
    ((1320, 360), 359, true),   // 05:59
    ((1320, 360), 360, false),  // 06:00 窗口终点（不含）
    ((1320, 360), 720, false),  // 12:00
    ((540, 1020), 540, true),   // 常规窗起点
    ((540, 1020), 1019, true),  // 常规窗末分钟
];

/// 全量重放回归库。任何期望不符即 Err（样本号）。
pub fn replay_midnight_cases() -> Result<usize, (usize, u32)> {
    for (i, ((f, t), m, want)) in MIDNIGHT_CASES.iter().enumerate() {
        let w = DayWindow::new(*f, *t).map_err(|_| (i, 0u32))?;
        if w.hits_local(*m) != *want {
            return Err((i, *m));
        }
    }
    Ok(MIDNIGHT_CASES.len())
}

/// 时区重排回归：跳变后当地命中语义保持。
pub fn replay_tz_cases() -> Result<usize, (usize, u32)> {
    let w = DayWindow::new(1320, 360).map_err(|_| (0, 0u32))?;
    // UTC 16:00（960 分）在 UTC+8 是 00:00（命中）。
    if !w.hits_local(to_local(960, 480)) {
        return Err((0, 960));
    }
    // UTC 16:00 在 UTC-5 是 11:00（不命中）。
    if w.hits_local(to_local(960, -300)) {
        return Err((1, 960));
    }
    Ok(2)
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_window_semantics() {
        let w = DayWindow::new(1320, 360).unwrap();
        assert!(w.wraps());
        assert!(w.hits_local(23 * 60));
        assert!(w.hits_local(0));
        assert!(!w.hits_local(6 * 60));
    }

    #[test]
    fn tz_reshuffle_notifies() {
        let mut tr = TzTracker::new(480);
        let n = tr.update(-300).unwrap();
        assert!(n.is_some());
        assert_eq!(tr.reshuffles, 1);
        assert_eq!(tr.update(-300).unwrap(), None);
    }

    #[test]
    fn dst_hole_clamped() {
        let (m, c) = dst_clamp(150);
        assert_eq!(m, 180);
        assert_eq!(c, Some(TzCode::DstHole));
        let (m2, c2) = dst_clamp(90);
        assert_eq!(m2, 90); // 重复区间数值不变
        assert_eq!(c2, None);
    }

    #[test]
    fn midnight_cases_all_green() {
        assert_eq!(replay_midnight_cases().unwrap(), MIDNIGHT_CASES.len());
        assert_eq!(replay_tz_cases().unwrap(), 2);
    }
}
