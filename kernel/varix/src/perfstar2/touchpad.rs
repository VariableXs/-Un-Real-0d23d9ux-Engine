//! F063 触控板手势前瞻（perfstar2 · G-B-23）——数据面不锁死，承诺面按实测定级。
//!
//! 主册判据（验收标准第一句）：
//! **评估报告产出（报文格式/多指识别可行性/延迟实测三节）即本项达标——
//! 评估件以报告为交付。**
//!
//! 功能定义（G-B-23）：Y7000 触控板多指手势评估项——HID 报文采集（双指
//! 滚动/三指切换/捏合缩放的原始数据面先行），定级后决定实现批次。
//!
//! 【交互设计】定级通过后：设置中心「蓝牙和其他设备-触控板」页：手势开关
//! 集 + 灵敏度滑杆 + 手势演示动画；不通过则页面显示「本机触控板评估中」
//! 诚实标注。
//! 【数据与存储】采集数据仅本机诊断用（不上传）；手势配置存配置层。
//! 【状态与异常】报文解析失败 → 降级为单点触摸（基础可用）；手势与输入法
//! 组合冲突 → 手势让位（打字优先）。
//! 【设计细节】采集维度：触点数/坐标轨迹/压力（若报文有）/时间戳；惯性
//! 滚动模型评估（速度衰减指数 0.95 档）；手势判定窗口 80ms；定级标准：
//! 延迟 <40ms 且误触率 <1% 才可承诺。
//!
//! 零堆纪律：定长轨迹环 + 定长报告结构，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 手势判定窗口：80ms（主册明文）。
pub const GESTURE_WINDOW_MS: u64 = 80;
/// 定级线①：端到端延迟 <40ms 才可承诺。
pub const GRADE_LATENCY_LIMIT_MS: u32 = 40;
/// 定级线②：误触率 <1% 才可承诺。
pub const GRADE_FALSE_TOUCH_PCT_X100: u32 = 100; // 1% ×100 定点
/// 惯性滚动速度衰减指数 0.95 档（×100 定点 = 95）。
pub const INERTIA_DECAY_X100: u32 = 95;
/// 三指上滑切换位移阈值（触板高 % ×10 定点：30% 触发行程）。
pub const THREE_FINGER_TRAVEL_PCT_X10: u32 = 300;
/// 捏合缩放触发距离变化比（×100 定点：15%）。
pub const PINCH_RATIO_X100: u32 = 15;
/// 轨迹环容量（判定窗内采样点数上限，125Hz × 80ms ≈ 10 点，取 2 倍余量）。
pub const TRACE_CAP: usize = 24;
/// 评估样本量（延迟实测/误触统计的最小样本——报告可信度门槛）。
pub const REPORT_MIN_SAMPLES: usize = 100;

/// HID 报文帧（采集维度：触点数/坐标/压力/时间戳；双触点报文含第二触点
/// 坐标——捏合判别需要点间距，单点聚合模型无法区分滚动与捏合）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HidFrame {
    /// 触点数（0-5）。
    pub touches: u8,
    /// 触点 1 x/y（0..=4095 ABS 域）。
    pub x: u16,
    pub y: u16,
    /// 触点 2 x/y（touches < 2 时为 0）。
    pub x2: u16,
    pub y2: u16,
    /// 压力（0..=255；报文无压力域 = None 语义 0）。
    pub pressure: u8,
    /// 报文时间戳（ms）。
    pub at_ms: u32,
    /// 报文完整性（CRC 通过）。
    pub crc_ok: bool,
}


/// 报文构造辅助（单点/双点重合——滚动语义；第二触点坐标默认与第一点重合，
/// 间距 0：spread_old=0 时 spread_ratio=0 → 不触发捏合）。
#[allow(dead_code)]
fn frame(touches: u8, x: u16, y: u16, pressure: u8, at_ms: u32) -> HidFrame {
    HidFrame { touches, x, y, x2: x, y2: y, pressure, at_ms, crc_ok: true }
}

/// 报文构造辅助（双点分离——捏合语义）。
#[allow(dead_code)]
fn frame2(touches: u8, x: u16, y: u16, x2: u16, y2: u16, pressure: u8, at_ms: u32) -> HidFrame {
    HidFrame { touches, x, y, x2, y2, pressure, at_ms, crc_ok: true }
}

/// 识别出的手势。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gesture {
    None,
    /// 双指滚动（dy 正负向）。
    Scroll(i16),
    /// 三指上滑 → 任务视图。
    ThreeUp,
    /// 三指下滑 → 回桌面。
    ThreeDown,
    /// 捏合缩放（距离变化幅度 ×100 定点；单点聚合模型只记幅度，
    /// 方向判定需双触点轨迹——评估报告登记项）。
    Pinch(i16),
}

/// 评估报告三节（评估件以报告为交付——判据唯一交付物）。
#[derive(Clone, Copy, Debug)]
pub struct GradeReport {
    /// 第一节：报文格式（触点数上限/压力域存在性/CRC 纪律）。
    pub proto_max_touches: u8,
    pub proto_has_pressure: bool,
    pub proto_crc_ok: bool,
    /// 第二节：多指识别可行性（判定窗内可分相处数）。
    pub multi_touch_feasible: bool,
    /// 第三节：延迟实测与误触率。
    pub latency_ms: u32,
    pub false_touch_pct_x100: u32,
    /// 定级结论：两项都达标才可承诺。
    pub committable: bool,
    pub sample_count: usize,
}

// ---------------------------------------------------------------------------
// 采集与识别
// ---------------------------------------------------------------------------

/// 触控板采集探针（评估件主体）。
pub struct TouchpadProbe {
    /// 判定窗内轨迹环。
    trace: [HidFrame; TRACE_CAP],
    trace_n: usize,
    trace_head: usize,
    /// 降级态：报文解析失败 → 单点触摸（基础可用）。
    degraded_single_touch: bool,
    /// 输入法组合态（手势让位——打字优先）。
    ime_active: bool,
    /// 延迟实测账（报文时刻 → 上屏动作时刻差）。
    lat_ring: [u16; 128],
    lat_n: usize,
    /// 误触统计：误触/总样本。
    false_touches: u32,
    total_samples: u32,
    /// 手势事件计数（可行性统计）。
    gesture_events: u32,
    now_ms: u64,
}

impl TouchpadProbe {
    pub const fn new() -> Self {
        TouchpadProbe {
            trace: [HidFrame { touches: 0, x: 0, y: 0, x2: 0, y2: 0, pressure: 0, at_ms: 0, crc_ok: true }; TRACE_CAP],
            trace_n: 0,
            trace_head: 0,
            degraded_single_touch: false,
            ime_active: false,
            lat_ring: [0; 128],
            lat_n: 0,
            false_touches: 0,
            total_samples: 0,
            gesture_events: 0,
            now_ms: 0,
        }
    }

    /// 报文入口：CRC 失败 → 降级单点触摸（主册状态与异常）。
    pub fn feed(&mut self, f: HidFrame) -> Gesture {
        self.now_ms = f.at_ms as u64;
        if !f.crc_ok {
            self.degraded_single_touch = true;
            self.clear_trace();
            return Gesture::None;
        }
        self.degraded_single_touch = false;
        // 输入法组合态：手势让位（打字优先——主册纪律）。
        if self.ime_active {
            self.clear_trace();
            return Gesture::None;
        }
        // 入轨迹环（窗口裁剪：80ms 外旧点清出）。
        self.trace[self.trace_head] = f;
        self.trace_head = (self.trace_head + 1) % TRACE_CAP;
        self.trace_n = (self.trace_n + 1).min(TRACE_CAP);
        self.trim_window();
        self.total_samples = self.total_samples.saturating_add(1);
        self.recognize()
    }

    /// 窗口裁剪：只留判定窗内样本。
    fn trim_window(&mut self) {
        let cutoff = self.now_ms.saturating_sub(GESTURE_WINDOW_MS);
        while self.trace_n > 0 {
            let oldest_idx = (self.trace_head + TRACE_CAP - self.trace_n) % TRACE_CAP;
            if (self.trace[oldest_idx].at_ms as u64) < cutoff {
                self.trace_n -= 1;
            } else {
                break;
            }
        }
    }

    fn clear_trace(&mut self) {
        self.trace_n = 0;
        self.trace_head = 0;
    }

    /// 识别器（libinput gesture 状态机思路的判据实装）。
    fn recognize(&mut self) -> Gesture {
        if self.trace_n < 2 {
            return Gesture::None;
        }
        let newest = self.trace[(self.trace_head + TRACE_CAP - 1) % TRACE_CAP];
        let oldest_idx = (self.trace_head + TRACE_CAP - self.trace_n) % TRACE_CAP;
        let oldest = self.trace[oldest_idx];
        match newest.touches {
            // 双指：质心位移 → 滚动；点间距变化 → 捏合（先判捏合，位移
            // 伴随时以捏合优先——libinput 同款消歧）。
            2 => {
                let cy_new = (newest.y as i32 + newest.y2 as i32) / 2;
                let cy_old = (oldest.y as i32 + oldest.y2 as i32) / 2;
                let dy = cy_new - cy_old;
                let spread_new = (newest.x as i32).abs_diff(newest.x2 as i32) as i32
                    + (newest.y as i32).abs_diff(newest.y2 as i32) as i32;
                let spread_old = (oldest.x as i32).abs_diff(oldest.x2 as i32) as i32
                    + (oldest.y as i32).abs_diff(oldest.y2 as i32) as i32;
                let spread_ratio = if spread_old == 0 {
                    0
                } else {
                    // abs_diff 返回 u32——收回 i32 域（spread_old > 0 保证除法安全）。
                    let delta = spread_new.abs_diff(spread_old) as i32;
                    delta * 100 / spread_old
                };
                if spread_ratio > PINCH_RATIO_X100 as i32 {
                    self.gesture_events = self.gesture_events.saturating_add(1);
                    Gesture::Pinch(spread_ratio.clamp(-32767, 32767) as i16)
                } else if dy.abs() > 8 {
                    self.gesture_events = self.gesture_events.saturating_add(1);
                    Gesture::Scroll(dy.clamp(-32767, 32767) as i16)
                } else {
                    Gesture::None
                }
            }
            // 三指切换：上/下按质心 y 行程判向（行程过阈值 % 触板高 4096）。
            3 => {
                let dy = newest.y as i32 - oldest.y as i32;
                let travel = (THREE_FINGER_TRAVEL_PCT_X10 as i32) * 4096 / 1000;
                if dy < -travel {
                    self.gesture_events = self.gesture_events.saturating_add(1);
                    Gesture::ThreeUp
                } else if dy > travel {
                    self.gesture_events = self.gesture_events.saturating_add(1);
                    Gesture::ThreeDown
                } else {
                    Gesture::None
                }
            }
            _ => Gesture::None,
        }
    }

    /// 延迟实测打点（报文时刻 → 处理完成时刻；评估报告第三节源）。
    pub fn record_latency(&mut self, report_done_ms: u32, at_ms: u32) {
        let lat = report_done_ms.saturating_sub(at_ms);
        self.lat_ring[self.lat_n % 128] = lat.min(u16::MAX as u32) as u16;
        self.lat_n += 1;
    }

    /// 误触登记（定级统计源）。
    pub fn record_false_touch(&mut self) {
        self.false_touches = self.false_touches.saturating_add(1);
    }

    /// 输入法组合态（手势让位开关）。
    pub fn set_ime_active(&mut self, active: bool) {
        self.ime_active = active;
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded_single_touch
    }

    /// 评估报告（三节结构化——判据唯一交付物）。
    pub fn grade_report(&self, proto_max_touches: u8, proto_has_pressure: bool) -> GradeReport {
        // 延迟取 P95（延迟环有效样本）。
        let latency = self.latency_p95_ms();
        let false_pct = if self.total_samples == 0 {
            0
        } else {
            self.false_touches * 10_000 / self.total_samples
        };
        GradeReport {
            proto_max_touches,
            proto_has_pressure,
            proto_crc_ok: !self.degraded_single_touch,
            multi_touch_feasible: self.gesture_events > 0 && !self.degraded_single_touch,
            latency_ms: latency,
            false_touch_pct_x100: false_pct,
            committable: self.total_samples as usize >= REPORT_MIN_SAMPLES
                && latency < GRADE_LATENCY_LIMIT_MS
                && false_pct < GRADE_FALSE_TOUCH_PCT_X100,
            sample_count: self.total_samples as usize,
        }
    }

    fn latency_p95_ms(&self) -> u32 {
        if self.lat_n == 0 {
            return 0;
        }
        let n = self.lat_n.min(128);
        let mut buf = [0u16; 128];
        for i in 0..n {
            buf[i] = self.lat_ring[(self.lat_n as usize + 128 - n + i) % 128];
        }
        buf[..n].sort_unstable();
        let rank = (n * 95 + 99) / 100;
        buf[rank.clamp(1, n) - 1] as u32
    }
}

/// 惯性滚动模型（速度衰减指数 0.95 档——评估节附录）。
/// 每帧速度 ×0.95；低于死区停止。
pub fn inertia_step(velocity_x100: i32) -> i32 {
    let decayed = velocity_x100 * INERTIA_DECAY_X100 as i32 / 100;
    if decayed.abs() < 5 {
        0 // 死区：速度 <0.05 停（防无限余滑）
    } else {
        decayed
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_touchpad_checks() -> CheckSet {
    let mut cs = CheckSet::new("F063-touchpad");
    // 1) 双指滚动识别（判定窗 80ms 内 y 位移）。
    let mut p = TouchpadProbe::new();
    let g1 = p.feed(frame(2, 2048, 1000, 40, 0));
    let g2 = p.feed(frame(2, 2048, 1200, 40, 40));
    cs.add(
        "two_finger_scroll",
        matches!(g1, Gesture::None) && matches!(g2, Gesture::Scroll(dy) if dy == 200),
        "",
    );
    // 2) 三指上滑 → 任务视图手势（行程 >30% 触板）。
    let mut p2 = TouchpadProbe::new();
    let _ = p2.feed(frame(3, 2048, 3000, 40, 0));
    let g3 = p2.feed(frame(3, 2048, 1500, 40, 60));
    cs.add("three_finger_up", matches!(g3, Gesture::ThreeUp), "");
    // 3) 捏合识别（双触点间距变化 >15%；质心不动、间距拉开 = 捏合放大）。
    let mut p3 = TouchpadProbe::new();
    let _ = p3.feed(frame2(2, 1000, 1000, 2000, 2000, 40, 0)); // 间距 2000
    let g4 = p3.feed(frame2(2, 1500, 1500, 1500, 1500, 40, 50)); // 间距 0：变化 100%
    cs.add("pinch_detected", matches!(g4, Gesture::Pinch(_)), "");
    // 3b) 双指同距平移（间距不变、质心动）→ 滚动而非捏合（消歧判据）。
    let mut p3b = TouchpadProbe::new();
    let _ = p3b.feed(frame2(2, 1000, 1000, 1500, 1500, 40, 0)); // 间距 1000
    let g4b = p3b.feed(frame2(2, 1200, 1300, 1700, 1800, 40, 50)); // 间距 1000 不变
    cs.add(
        "pinch_scroll_disambiguation",
        matches!(g4b, Gesture::Scroll(dy) if dy == 300),
        "",
    );
    // 4) 判定窗口 80ms：窗外旧点裁掉（100ms 间隔不构成手势）。
    let mut p4 = TouchpadProbe::new();
    let _ = p4.feed(frame(2, 2048, 1000, 40, 0));
    let g5 = p4.feed(frame(2, 2048, 2000, 40, 100));
    cs.add("gesture_window_80ms", matches!(g5, Gesture::None), "");
    // 5) 报文 CRC 失败 → 降级单点触摸 + 清轨迹（后续仍可用）。
    let mut p5 = TouchpadProbe::new();
    let _ = p5.feed(frame(2, 2048, 1000, 40, 0));
    let g6 = p5.feed(HidFrame { touches: 2, x: 2048, y: 1500, x2: 2048, y2: 1500, pressure: 40, at_ms: 30, crc_ok: false });
    cs.add(
        "crc_fail_degrades_single",
        matches!(g6, Gesture::None) && p5.is_degraded(),
        "",
    );
    let g7 = p5.feed(frame(1, 2048, 1500, 40, 60));
    cs.add(
        "degrade_recovers_single_touch",
        !p5.is_degraded() && matches!(g7, Gesture::None),
        "",
    );
    // 6) 手势与输入法冲突 → 手势让位（打字优先）。
    let mut p6 = TouchpadProbe::new();
    p6.set_ime_active(true);
    let _ = p6.feed(frame(2, 2048, 1000, 40, 0));
    let g8 = p6.feed(frame(2, 2048, 2000, 40, 40));
    cs.add("ime_takes_priority", matches!(g8, Gesture::None), "");
    // 7) 惯性模型：0.95 衰减 + 死区停。
    let v0 = 1000; // 10.00
    let v1 = inertia_step(v0);
    let v2 = inertia_step(v1);
    cs.add(
        "inertia_decay_095",
        v1 == 950 && v2 == 902 && inertia_step(4) == 0,
        "",
    );
    // 8) 评估报告：好数据（低延迟低误触）→ 可承诺。
    let mut p8 = TouchpadProbe::new();
    for i in 0..120u32 {
        let _ = p8.feed(HidFrame { touches: 2, x: 2048, y: 1000 + (i as u16) * 4, x2: 2048, y2: 1000 + (i as u16) * 4, pressure: 40, at_ms: i * 10, crc_ok: true });
        p8.record_latency(i * 10 + 12, i * 10); // 12ms 延迟
    }
    let r8 = p8.grade_report(5, true);
    cs.add(
        "grade_report_committable",
        r8.committable && r8.latency_ms < GRADE_LATENCY_LIMIT_MS && r8.proto_max_touches == 5,
        "",
    );
    // 9) 评估报告：坏数据（高误触）→ 诚实不承诺（页面显「评估中」）。
    let mut p9 = TouchpadProbe::new();
    for i in 0..120u32 {
        let _ = p9.feed(HidFrame { touches: 2, x: 2048, y: 1000 + (i as u16) * 4, x2: 2048, y2: 1000 + (i as u16) * 4, pressure: 40, at_ms: i * 10, crc_ok: true });
        p9.record_latency(i * 10 + 12, i * 10);
        if i % 50 == 0 {
            p9.record_false_touch(); // 2% 误触
        }
    }
    let r9 = p9.grade_report(5, true);
    cs.add(
        "grade_report_honest_reject",
        !r9.committable && r9.false_touch_pct_x100 >= GRADE_FALSE_TOUCH_PCT_X100,
        "",
    );
    // 10) 报告样本量门槛：样本不足不承诺（报告可信度）。
    let mut p10 = TouchpadProbe::new();
    for i in 0..30u32 {
        p10.record_latency(i * 10 + 5, i * 10);
    }
    let r10 = p10.grade_report(5, true);
    cs.add("report_min_samples", r10.sample_count < REPORT_MIN_SAMPLES && !r10.committable, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_direction_symmetry() {
        let mut p = TouchpadProbe::new();
        let _ = p.feed(frame(2, 2048, 2000, 40, 0));
        let up = p.feed(frame(2, 2048, 1800, 40, 30));
        assert!(matches!(up, Gesture::Scroll(dy) if dy == -200), "上滑负向");
    }

    #[test]
    fn three_finger_down_to_desktop() {
        let mut p = TouchpadProbe::new();
        let _ = p.feed(frame(3, 2048, 1500, 40, 0));
        let g = p.feed(frame(3, 2048, 3000, 40, 60));
        assert!(matches!(g, Gesture::ThreeDown));
    }

    #[test]
    fn window_trims_old_samples() {
        let mut p = TouchpadProbe::new();
        // 连打跨 200ms：窗内样本恒 ≤ 80ms 跨度。
        for i in 0..20u32 {
            let _ = p.feed(frame(1, 100, 100, 10, i * 10));
        }
        assert!(p.trace_n <= (GESTURE_WINDOW_MS / 10 + 1) as usize);
    }

    #[test]
    fn latency_p95_ranking() {
        let mut p = TouchpadProbe::new();
        // 100 样本：95×10ms + 5×50ms → P95 = 10ms 位次附近（95 位）。
        for i in 0..100u32 {
            let lat = if i % 20 == 19 { 50 } else { 10 };
            p.record_latency(lat, 0);
        }
        let r = p.grade_report(5, false);
        assert_eq!(r.latency_ms, 10, "P95 位次 = 第 95 小");
    }

    #[test]
    fn ime_flag_blocks_all_gestures() {
        let mut p = TouchpadProbe::new();
        p.set_ime_active(true);
        let _ = p.feed(frame(3, 2048, 3000, 40, 0));
        let g = p.feed(frame(3, 2048, 1200, 40, 50));
        assert!(matches!(g, Gesture::None), "打字优先");
        p.set_ime_active(false);
        let _ = p.feed(frame(3, 2048, 3000, 40, 60));
        let g2 = p.feed(frame(3, 2048, 1200, 40, 90));
        assert!(matches!(g2, Gesture::ThreeUp), "解除后恢复");
    }

    #[test]
    fn report_sections_complete() {
        let mut p = TouchpadProbe::new();
        for i in 0..REPORT_MIN_SAMPLES as u32 {
            let _ = p.feed(HidFrame { touches: 2, x: 2048, y: 1000 + (i as u16) * 4, x2: 2048, y2: 1000 + (i as u16) * 4, pressure: 40, at_ms: i * 10, crc_ok: true });
            p.record_latency(i * 10 + 12, i * 10);
        }
        let r = p.grade_report(5, true);
        // 三节齐备：报文格式 / 可行性 / 延迟实测。
        assert_eq!(r.proto_max_touches, 5);
        assert!(r.proto_has_pressure);
        assert!(r.proto_crc_ok);
        assert!(r.multi_touch_feasible);
        assert!(r.sample_count >= REPORT_MIN_SAMPLES);
    }
}
