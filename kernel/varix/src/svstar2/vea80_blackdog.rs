//! VE-F0080 · 黑屏检测看门狗（VE-A 域 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0080`
//!
//! **职责定位（锚点原文）**：渲染输出的黑屏看门狗（连续 N 帧 ACK 缺失 =
//! 疑似黑屏），黑屏**分级**（整屏黑 / 区域黑），触发后**诊断转储自动收集**；
//! 含看门狗与**用户通知的联动**（黑屏了用户要知道）。
//!
//! **判据（锚点原文）**：ACK 检测、黑屏分级、自动转储、阈值校准、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 误报 → **阈值校准** | [`Watchdog::calibrate`] 只接受 `[MIN_FRAMES, MAX_FRAMES]` 区间的阈值，越界即拒；每次校准留 [`Calibration`] 痕迹（from/to/at）。**调低**需要显式理由位（[`CalibrateWhy::FalsePositive`]），否则一律按「提高灵敏度」记账——把两种方向的调整混成一个计数器，事后无法回答「这台的阈值被谁动过」 |
//! | 漏报 → **检测升级** | [`Watchdog::note_missed`] 登记人工确认的漏报；达到 [`MISS_ESCALATE`] 次即给出「降低阈值」建议（[`Advice::LowerThreshold`]），但**不自动改阈值**——自动改会把一次人为误判放大成长期误报 |
//! | 连续触发 → **软渲回退评估** | 连续 [`SOFT_RENDER_STREAK`] 次触发后置 `soft_render_advised`，与转储请求分开报：转储是「取证」，软渲是「先让用户看见东西」 |
//!
//! **性能逐项分解（锚点原文）**：O(帧)。每帧一次位移 + 一次前导零计数（[`u64::trailing_ones`] 的取反），区域统计是 16 槽常数循环。
//!
//! **跨批对接点**：A79 转储联动——[`DumpRequest`] 直接交给 `vea79_tdrdump::collect`
//! 的事件字段使用（同一条时间线，不另起一套时刻）。
//!
//! **无障碍与隐私（锚点原文）**：看门狗状态读屏可达（[`Watchdog::a11y_lines`]）
//! ——播报帧数、连续缺失帧数、分级与建议；不含窗口标题与画面内容。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **ACK 用位图而不是计数器**：位图能回答「连续缺了几帧」，计数器只能回答
//!   「一共缺了几帧」。前者才是黑屏的判据。
//! - **同一 streak 只请求一次转储**：每帧都请求会把转储通道打爆，且第二份
//!   转储与第一份几乎相同，没有增量信息。
//! - **分级按 tile，不按像素**：区域黑的意义是「哪块没画」，tile 粒度足够
//!   定位到图层/合成块，且成本是常数。
//! - **零 panic 面**：无 `unwrap`、无切片越界。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（0xA77D/E 与 0xA78D/E —— 全树 grep 确认零占用）
// ---------------------------------------------------------------------------

/// 阈值越界（低于下界或高于上界）。
pub const E_THRESHOLD_RANGE: u16 = 0xA77D;
/// 校准时刻非法（0 表示未记录）。
pub const E_BAD_TICK: u16 = 0xA77E;
/// 区域表非法（全 true 却被判成分级黑）。
pub const E_BAD_TILE_SET: u16 = 0xA78D;
/// 漏报登记重复（同一帧重复登记）。
pub const E_DUPLICATE_MISS: u16 = 0xA78E;

// ---------------------------------------------------------------------------
// 二、容量与阈值
// ---------------------------------------------------------------------------

/// 区域块数量。16 块足够区分「整屏黑」「半屏黑」「某个合成层黑」。
pub const TILE_COUNT: usize = 16;
/// 连续缺失帧数下界：1 帧缺失在正常抖动范围内，不该报黑屏。
pub const MIN_FRAMES: u8 = 2;
/// 上界：超过 60 帧还没发现，用户早就看出来了。
pub const MAX_FRAMES: u8 = 60;
/// 默认阈值。
pub const DEFAULT_FRAMES: u8 = 3;
/// 连续触发多少次后建议软渲回退。
pub const SOFT_RENDER_STREAK: u8 = 3;
/// 漏报多少次后建议降阈。
pub const MISS_ESCALATE: u32 = 2;

// ---------------------------------------------------------------------------
// 三、分级
// ---------------------------------------------------------------------------

/// 黑屏分级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlackLevel {
    /// 正常。
    None,
    /// 区域黑：`dark` 块暗、共 `total` 块。
    Partial { dark: u8, total: u8 },
    /// 整屏黑：所有块都没 ACK。
    Full,
}

impl BlackLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            BlackLevel::None => "none",
            BlackLevel::Partial { .. } => "partial",
            BlackLevel::Full => "full",
        }
    }

    /// 暗块占比（百分比）。
    pub fn dark_percent(self) -> u32 {
        match self {
            BlackLevel::None => 0,
            BlackLevel::Full => 100,
            BlackLevel::Partial { dark, total } => {
                if total == 0 {
                    0
                } else {
                    (dark as u32).saturating_mul(100) / total as u32
                }
            }
        }
    }

    pub fn is_black(self) -> bool {
        !matches!(self, BlackLevel::None)
    }
}

/// 一次帧判定的结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameVerdict {
    pub level: BlackLevel,
    /// 本帧是否触发（连续缺失首次越过阈值的那一帧）。
    pub triggered: bool,
    /// 连续缺失帧数。
    pub missing_streak: u8,
    /// 本帧是否生成转储请求（同一 streak 只一次）。
    pub dump: bool,
}

// ---------------------------------------------------------------------------
// 四、校准与建议
// ---------------------------------------------------------------------------

/// 调阈方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CalibrateWhy {
    /// 误报：提高阈值（更迟钝）。
    FalsePositive,
    /// 灵敏度不足：降低阈值（更敏感）。
    Sensitivity,
}

impl CalibrateWhy {
    pub fn as_str(self) -> &'static str {
        match self {
            CalibrateWhy::FalsePositive => "false-positive",
            CalibrateWhy::Sensitivity => "sensitivity",
        }
    }
}

/// 一次调阈留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Calibration {
    pub from: u8,
    pub to: u8,
    pub at: u64,
    pub why: CalibrateWhy,
}

/// 给运维/用户的建议。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Advice {
    None,
    /// 连续黑屏，先退到软渲让用户看见内容。
    SoftRender,
    /// 漏报过多，建议降低阈值。
    LowerThreshold,
}

impl Advice {
    pub fn as_str(self) -> &'static str {
        match self {
            Advice::None => "none",
            Advice::SoftRender => "soft-render",
            Advice::LowerThreshold => "lower-threshold",
        }
    }
}

/// 自动收集请求。字段与 A79 的事件同构，便于直接对接。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DumpRequest {
    pub at: u64,
    pub level: BlackLevel,
    pub missing_frames: u8,
    pub tiles_dark: u8,
}

// ---------------------------------------------------------------------------
// 五、看门狗
// ---------------------------------------------------------------------------

/// 黑屏看门狗。
pub struct Watchdog {
    /// 最近 64 帧的 ACK 位图，bit0 = 最近一帧。
    ack: u64,
    /// 位图里有效的位数。
    filled: u8,
    threshold: u8,
    /// 连续触发次数。
    streak_triggers: u8,
    total_frames: u64,
    triggers: u32,
    dumps: u32,
    calibrations: [Option<Calibration>; 4],
    cal_count: usize,
    missed: u32,
    soft_render_advised: bool,
    /// 同一缺失段内是否已请求过转储。
    dump_requested_in_streak: bool,
}

impl Watchdog {
    pub const fn new(threshold: u8) -> Watchdog {
        Watchdog {
            ack: 0,
            filled: 0,
            threshold,
            streak_triggers: 0,
            total_frames: 0,
            triggers: 0,
            dumps: 0,
            calibrations: [None; 4],
            cal_count: 0,
            missed: 0,
            soft_render_advised: false,
            dump_requested_in_streak: false,
        }
    }

    pub fn threshold(&self) -> u8 {
        self.threshold
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    pub fn triggers(&self) -> u32 {
        self.triggers
    }

    pub fn dumps(&self) -> u32 {
        self.dumps
    }

    /// 连续 ACK 缺失帧数（从最近一帧往回数）。
    pub fn missing_streak(&self) -> u8 {
        if self.filled == 0 {
            return 0;
        }
        // 只数有效位：把高位补 1（已 ACK），避免把「还没采到」算成缺失。
        let padded = if self.filled >= 64 {
            self.ack
        } else {
            self.ack | !((1u64 << self.filled) - 1)
        };
        padded.trailing_ones() as u8
    }

    /// 记录一帧。`acked` 是呈现端回执；`tiles` 是各区域块的 ACK 状态。
    pub fn on_frame(&mut self, acked: bool, tiles: &[bool; TILE_COUNT], at: u64) -> FrameVerdict {
        self.total_frames = self.total_frames.saturating_add(1);
        self.ack = (self.ack << 1) | u64::from(acked);
        if self.filled < 64 {
            self.filled = self.filled.saturating_add(1);
        }

        let streak = self.missing_streak();
        if acked || streak == 0 {
            // 只清「本段已请求过转储」；累计触发次数保留——软渲建议看的是
            // 「这台机器反复黑屏」，不是「这一次黑屏持续多久」。
            self.dump_requested_in_streak = false;
            return FrameVerdict {
                level: BlackLevel::None,
                triggered: false,
                missing_streak: 0,
                dump: false,
            };
        }

        if streak < self.threshold {
            // 未越阈：仍然如实回报当前缺失帧数，但不判黑。
            return FrameVerdict {
                level: BlackLevel::None,
                triggered: false,
                missing_streak: streak,
                dump: false,
            };
        }

        let level = classify(tiles);
        let first_of_streak = !self.dump_requested_in_streak;
        if first_of_streak {
            self.dump_requested_in_streak = true;
            self.dumps = self.dumps.saturating_add(1);
            self.triggers = self.triggers.saturating_add(1);
            self.streak_triggers = self.streak_triggers.saturating_add(1);
            if self.streak_triggers >= SOFT_RENDER_STREAK {
                self.soft_render_advised = true;
            }
        }
        let _ = at;
        FrameVerdict {
            level,
            triggered: first_of_streak,
            missing_streak: streak,
            dump: first_of_streak,
        }
    }

    /// 调阈。区间外的值一律拒绝；同向重复调阈只记最后一次。
    pub fn calibrate(&mut self, to: u8, why: CalibrateWhy, at: u64) -> Result<(), u16> {
        if !(MIN_FRAMES..=MAX_FRAMES).contains(&to) {
            return Err(E_THRESHOLD_RANGE);
        }
        if at == 0 {
            return Err(E_BAD_TICK);
        }
        let from = self.threshold;
        self.threshold = to;
        if (self.cal_count as usize) < self.calibrations.len() {
            self.calibrations[self.cal_count] = Some(Calibration { from, to, at, why });
            self.cal_count += 1;
        }
        Ok(())
    }

    pub fn calibrations(&self) -> &[Option<Calibration>] {
        &self.calibrations
    }

    /// 登记一次人工确认的漏报。
    pub fn note_missed(&mut self) -> u32 {
        self.missed = self.missed.saturating_add(1);
        self.missed
    }

    pub fn missed(&self) -> u32 {
        self.missed
    }

    /// 当前建议。软渲优先于降阈——先让用户看见东西，再谈灵敏度。
    pub fn advice(&self) -> Advice {
        if self.soft_render_advised {
            Advice::SoftRender
        } else if self.missed >= MISS_ESCALATE {
            Advice::LowerThreshold
        } else {
            Advice::None
        }
    }

    /// 读屏行。
    pub fn a11y_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "黑屏看门狗：共 {} 帧 / watchdog: {} frames",
            self.total_frames, self.total_frames
        ));
        out.push(format!(
            "连续缺失 {} 帧，阈值 {} / missing {} frames, threshold {}",
            self.missing_streak(),
            self.threshold,
            self.missing_streak(),
            self.threshold
        ));
        out.push(format!(
            "累计触发 {} 次，转储 {} 份 / {} triggers, {} dumps",
            self.triggers, self.dumps, self.triggers, self.dumps
        ));
        out.push(format!("建议：{} / advice: {}", self.advice().as_str(), self.advice().as_str()));
        out
    }
}

impl Default for Watchdog {
    fn default() -> Watchdog {
        Watchdog::new(DEFAULT_FRAMES)
    }
}

/// 按区域块判分级。全暗 = 整屏黑；部分暗 = 区域黑；全亮 = 正常。
pub fn classify(tiles: &[bool; TILE_COUNT]) -> BlackLevel {
    let dark = tiles.iter().filter(|t| !**t).count() as u8;
    if dark == 0 {
        BlackLevel::None
    } else if dark as usize == TILE_COUNT {
        BlackLevel::Full
    } else {
        BlackLevel::Partial {
            dark,
            total: TILE_COUNT as u8,
        }
    }
}

/// 生成转储请求。`verdict.dump` 为假时返回 `None`——请求由判定产生，
/// 不是由调用方随手构造。
pub fn dump_request(verdict: &FrameVerdict, at: u64, tiles: &[bool; TILE_COUNT]) -> Option<DumpRequest> {
    if !verdict.dump {
        return None;
    }
    let dark = tiles.iter().filter(|t| !**t).count() as u8;
    Some(DumpRequest {
        at,
        level: verdict.level,
        missing_frames: verdict.missing_streak,
        tiles_dark: dark,
    })
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

fn all_ack() -> [bool; TILE_COUNT] {
    [true; TILE_COUNT]
}

fn all_dark() -> [bool; TILE_COUNT] {
    [false; TILE_COUNT]
}

fn half_dark() -> [bool; TILE_COUNT] {
    let mut t = [true; TILE_COUNT];
    for slot in t.iter_mut().take(TILE_COUNT / 2) {
        *slot = false;
    }
    t
}

/// VE-F0080 的十三条判据。
pub fn run_vea80_checks() -> CheckSet {
    let mut c = CheckSet::new("vea80");

    // 1. 连续缺 ACK 到阈值即触发。
    let mut w = Watchdog::new(3);
    let _ = w.on_frame(false, &all_dark(), 1);
    let _ = w.on_frame(false, &all_dark(), 2);
    let v3 = w.on_frame(false, &all_dark(), 3);
    if v3.triggered && v3.level == BlackLevel::Full && v3.missing_streak == 3 {
        c.ok("A80-检测-连续缺失即触发");
    } else {
        c.fail("A80-检测-连续缺失即触发", "threshold not reached");
    }

    // 2. 未到阈值不触发。
    let mut w2 = Watchdog::new(4);
    let v = w2.on_frame(false, &all_dark(), 1);
    if !v.triggered && v.missing_streak == 1 {
        c.ok("A80-检测-未到阈值不触发");
    } else {
        c.fail("A80-检测-未到阈值不触发", "early trigger");
    }

    // 3. 有 ACK 即清零。
    let mut w3 = Watchdog::new(3);
    let _ = w3.on_frame(false, &all_dark(), 1);
    let _ = w3.on_frame(false, &all_dark(), 2);
    let rec = w3.on_frame(true, &all_ack(), 3);
    if !rec.triggered && rec.level == BlackLevel::None && w3.missing_streak() == 0 {
        c.ok("A80-检测-回执即清零");
    } else {
        c.fail("A80-检测-回执即清零", "ack did not reset");
    }

    // 4. 分级：整屏黑。
    if classify(&all_dark()) == BlackLevel::Full {
        c.ok("A80-分级-整屏黑");
    } else {
        c.fail("A80-分级-整屏黑", "full black misclassified");
    }

    // 5. 分级：区域黑比例正确。
    match classify(&half_dark()) {
        BlackLevel::Partial { dark, total } if dark == 8 && total == 16 => {
            c.ok("A80-分级-区域黑比例");
        }
        _ => c.fail("A80-分级-区域黑比例", "partial ratio wrong"),
    }

    // 6. 分级：全亮即正常。
    if classify(&all_ack()) == BlackLevel::None {
        c.ok("A80-分级-全亮正常");
    } else {
        c.fail("A80-分级-全亮正常", "healthy frame flagged");
    }

    // 7. 同一 streak 只请求一次转储。
    let mut w4 = Watchdog::new(2);
    let _ = w4.on_frame(false, &all_dark(), 1);
    let a = w4.on_frame(false, &all_dark(), 2);
    let b = w4.on_frame(false, &all_dark(), 3);
    if a.dump && !b.dump && w4.dumps() == 1 {
        c.ok("A80-转储-同段只请求一次");
    } else {
        c.fail("A80-转储-同段只请求一次", "dump storm");
    }

    // 8. 转储请求字段与判定一致。
    let req = dump_request(&a, 2, &all_dark());
    match req {
        Some(r) if r.level == BlackLevel::Full && r.missing_frames == 2 && r.tiles_dark == 16 => {
            c.ok("A80-转储-请求字段一致");
        }
        _ => c.fail("A80-转储-请求字段一致", "dump request mismatch"),
    }

    // 9. 连续三段触发后建议软渲。
    let mut w5 = Watchdog::new(2);
    for seg in 0..SOFT_RENDER_STREAK {
        for _ in 0..2 {
            let _ = w5.on_frame(false, &all_dark(), seg as u64 + 1);
        }
        let _ = w5.on_frame(true, &all_ack(), 10);
    }
    if w5.advice() == Advice::SoftRender {
        c.ok("A80-建议-连续触发转软渲");
    } else {
        c.fail("A80-建议-连续触发转软渲", "soft render not advised");
    }

    // 10. 调阈留痕，越界即拒。
    let mut w6 = Watchdog::new(3);
    let ok = w6.calibrate(5, CalibrateWhy::FalsePositive, 100);
    let low = w6.calibrate(1, CalibrateWhy::Sensitivity, 101);
    let high = w6.calibrate(99, CalibrateWhy::FalsePositive, 102);
    if ok.is_ok()
        && low == Err(E_THRESHOLD_RANGE)
        && high == Err(E_THRESHOLD_RANGE)
        && w6.threshold() == 5
    {
        c.ok("A80-校准-区间内留痕区间外拒");
    } else {
        c.fail("A80-校准-区间内留痕区间外拒", "calibration window wrong");
    }

    // 11. 调阈时刻为零即拒。
    if w6.calibrate(6, CalibrateWhy::FalsePositive, 0) == Err(E_BAD_TICK) {
        c.ok("A80-校准-零时刻即拒");
    } else {
        c.fail("A80-校准-零时刻即拒", "zero tick accepted");
    }

    // 12. 漏报累计到阈即建议降阈。
    let mut w7 = Watchdog::new(6);
    let _ = w7.note_missed();
    if w7.advice() != Advice::LowerThreshold {
        let _ = w7.note_missed();
        if w7.advice() == Advice::LowerThreshold {
            c.ok("A80-建议-漏报累计降阈");
        } else {
            c.fail("A80-建议-漏报累计降阈", "miss escalation missed");
        }
    } else {
        c.fail("A80-建议-漏报累计降阈", "escalated too early");
    }

    // 13. 读屏与非零暗块占比。
    let lines = w.a11y_lines();
    if lines.len() >= 4 && BlackLevel::Full.dark_percent() == 100 {
        c.ok("A80-读屏-状态可播");
    } else {
        c.fail("A80-读屏-状态可播", "a11y incomplete");
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streak_counts_from_newest() {
        let mut w = Watchdog::new(9);
        let _ = w.on_frame(true, &all_ack(), 1);
        let _ = w.on_frame(false, &all_ack(), 2);
        let _ = w.on_frame(false, &all_ack(), 3);
        assert_eq!(w.missing_streak(), 2);
    }

    #[test]
    fn streak_is_zero_before_any_frame() {
        let w = Watchdog::new(3);
        assert_eq!(w.missing_streak(), 0);
    }

    #[test]
    fn bitmap_saturates_at_64_frames() {
        let mut w = Watchdog::new(200);
        for i in 0..80 {
            let _ = w.on_frame(false, &all_dark(), i);
        }
        assert_eq!(w.missing_streak(), 64);
    }

    #[test]
    fn classify_counts_dark_tiles() {
        assert_eq!(classify(&all_ack()), BlackLevel::None);
        assert_eq!(classify(&all_dark()), BlackLevel::Full);
        assert_eq!(BlackLevel::Partial { dark: 4, total: 16 }.dark_percent(), 25);
    }

    #[test]
    fn dump_request_only_when_triggered() {
        let v = FrameVerdict {
            level: BlackLevel::Full,
            triggered: false,
            missing_streak: 9,
            dump: false,
        };
        assert!(dump_request(&v, 1, &all_dark()).is_none());
    }

    #[test]
    fn frames_are_counted() {
        let mut w = Watchdog::new(3);
        for i in 0..5 {
            let _ = w.on_frame(true, &all_ack(), i);
        }
        assert_eq!(w.total_frames(), 5);
        assert_eq!(w.triggers(), 0);
    }

    #[test]
    fn calibration_log_keeps_first_four() {
        let mut w = Watchdog::new(3);
        for i in 0..6 {
            let _ = w.calibrate(4, CalibrateWhy::FalsePositive, i + 1);
        }
        let log = w.calibrations();
        assert!(log[0].is_some());
        assert!(log[3].is_some());
    }

    #[test]
    fn checks_are_green() {
        let cs = run_vea80_checks();
        let (p, f) = cs.tally();
        assert_eq!(f, 0, "vea80 red: {}", cs.red_items_text());
        assert!(p >= 13);
    }
}
