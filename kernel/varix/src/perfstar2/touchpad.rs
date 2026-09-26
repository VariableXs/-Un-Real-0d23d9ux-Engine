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
    // i64 中间量 + i32 钳制：病态大初速不溢出（K2 缺陷账 #20——
    // debug 档乘法 panic、release 档回绕，两档都不可接受）。
    let v = (velocity_x100 as i64) * (INERTIA_DECAY_X100 as i64) / 100;
    let decayed = v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
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

// ===========================================================================
// v2 深化批（F063 · G-B-23）——掌压拒绝 / 点按状态机 / 边缘滚动带 / 惯性积分
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-23 功能定义的实装细化，非新立项）：
// 1. PalmRejector —— 掌压拒绝：压力 > 阈值 或 触点位于掌根区（y 高位）
//    或 触点面积超大 → 丢弃并计数（误触率的直接防线）。
// 2. TapClassifier —— 点按状态机：按下-抬起位移 < TAP_TRAVEL 且时长
//    < TAP_MS → 单击；两次单击间隔 < DOUBLE_MS → 双击；长按 → 无手势
//    （与拖拽区分）；移动超行程 → 拖拽（交回主判定器）。
// 3. EdgeScrollZone —— 边缘滚动带：右缘带内单指移动 → 滚动事件；
//    两指在场时让位（多指手势优先级更高——不抢判定窗）。
// 4. InertiaProfile —— 惯性位移积分：初速 ×100 定点逐拍 ×0.95 衰减，
//    累计位移直到截止速度——「滑一下走多远」的可测答案（定级报告
//    附「预计滑行距离」字段的数据源）。
// 全部零堆：定长状态 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 掌压压力阈值（HID 压力域 0-1023；>600 判掌——旋钮可调）。
pub const PALM_PRESSURE_TH: u16 = 600;
/// 掌根区起点（y 域 0..4095，y > 3379 判掌根区——触板顶部 17%）。
pub const PALM_ROOT_Y: u16 = 3379;
/// 点按最大行程（ABS 域：4095 宽 × 1.5% ≈ 60）。
pub const TAP_TRAVEL_MAX: u16 = 60;
/// 点按最大时长（ms）。
pub const TAP_MS_MAX: u32 = 200;
/// 双击最大间隔（ms）。
pub const DOUBLE_MS_MAX: u32 = 300;
/// 右缘滚动带宽（ABS 域 4095 × 2.5% ≈ 100）。
pub const EDGE_SCROLL_BAND: u16 = 100;
/// 惯性截止速度（×100 定点 counts/s——低于即停）。
pub const INERTIA_CUTOFF_X100: i32 = 800;
/// ABS 域宽度（触板报文坐标 0..4095）。
pub const ABS_DOMAIN: u16 = 4095;

// ---------------------------------------------------------------------------
// 深化一：掌压拒绝器
// ---------------------------------------------------------------------------

/// 掌压拒绝判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PalmVerdict {
    /// 正常触点（交主判定器）。
    Finger,
    /// 掌压丢弃（压力/位置/面积任一命中）。
    Palm(PalmReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PalmReason {
    Pressure,
    RootZone,
    LargeContact,
}

/// 掌压拒绝器（丢弃计数入账——误触率的构成面）。
pub struct PalmRejector {
    rejected: [u64; 3], // 三原因分账
    accepted: u64,
}

impl PalmRejector {
    pub const fn new() -> Self {
        PalmRejector {
            rejected: [0; 3],
            accepted: 0,
        }
    }

    /// 判定一个触点（压力 0 = 无压力报文——只看位置与尺寸；x 横向位置
    /// 本层不参与判定——掌根区按纵向 y 划分，x 预留给左右手习惯旋钮）。
    pub fn judge(&mut self, _x: u16, y: u16, pressure: u16, contact_area: u16) -> PalmVerdict {
        if pressure > PALM_PRESSURE_TH {
            self.rejected[0] += 1;
            return PalmVerdict::Palm(PalmReason::Pressure);
        }
        if y > PALM_ROOT_Y {
            self.rejected[1] += 1;
            return PalmVerdict::Palm(PalmReason::RootZone);
        }
        if contact_area > 900 {
            self.rejected[2] += 1;
            return PalmVerdict::Palm(PalmReason::LargeContact);
        }
        self.accepted += 1;
        PalmVerdict::Finger
    }

    /// 误触率构成（拒绝总数 / 判定总数 ×100 定点）。
    pub fn rejection_rate_x100(&self) -> u32 {
        let total = self.accepted + self.rejected[0] + self.rejected[1] + self.rejected[2];
        if total == 0 {
            return 0;
        }
        (self.rejected[0] + self.rejected[1] + self.rejected[2]) as u32 * 100 / total as u32
    }

    pub fn accepted(&self) -> u64 {
        self.accepted
    }

    pub fn rejected_by(&self, r: PalmReason) -> u64 {
        match r {
            PalmReason::Pressure => self.rejected[0],
            PalmReason::RootZone => self.rejected[1],
            PalmReason::LargeContact => self.rejected[2],
        }
    }
}

// ---------------------------------------------------------------------------
// 深化二：点按状态机
// ---------------------------------------------------------------------------

/// 点按判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapEvent {
    Single,
    Double,
    /// 超行程——是拖拽不是点按（交回主判定器）。
    Drag,
    /// 超时长——是长按不是点按。
    LongPress,
    /// 按下中（尚未抬起——无结论）。
    Pending,
}

/// 点按状态机。
pub struct TapClassifier {
    down: bool,
    down_ms: u32,
    origin_x: u16,
    origin_y: u16,
    last_up_ms: u32,
    /// 上一事件（双击判定的对侧）。
    last_was_single: bool,
    taps: u64,
    doubles: u64,
}

impl TapClassifier {
    pub const fn new() -> Self {
        TapClassifier {
            down: false,
            down_ms: 0,
            origin_x: 0,
            origin_y: 0,
            last_up_ms: 0,
            last_was_single: false,
            taps: 0,
            doubles: 0,
        }
    }

    /// 按下。
    pub fn press(&mut self, x: u16, y: u16, at_ms: u32) {
        self.down = true;
        self.down_ms = at_ms;
        self.origin_x = x;
        self.origin_y = y;
    }

    /// 移动（按下中报告当前位置——超行程即时反馈 Drag）。
    pub fn move_to(&mut self, x: u16, y: u16, at_ms: u32) -> TapEvent {
        if !self.down {
            return TapEvent::Pending;
        }
        let travel = self.origin_x.abs_diff(x).max(self.origin_y.abs_diff(y));
        if travel > TAP_TRAVEL_MAX {
            self.down = false; // 拖拽接管——点按状态退出
            self.last_was_single = false;
            return TapEvent::Drag;
        }
        if at_ms.saturating_sub(self.down_ms) > TAP_MS_MAX {
            self.down = false;
            self.last_was_single = false;
            return TapEvent::LongPress;
        }
        TapEvent::Pending
    }

    /// 抬起。
    pub fn release(&mut self, at_ms: u32) -> TapEvent {
        if !self.down {
            return TapEvent::Pending;
        }
        self.down = false;
        let dur = at_ms.saturating_sub(self.down_ms);
        if dur > TAP_MS_MAX {
            self.last_was_single = false;
            return TapEvent::LongPress;
        }
        if self.last_was_single && at_ms.saturating_sub(self.last_up_ms) <= DOUBLE_MS_MAX {
            self.last_was_single = false;
            self.doubles += 1;
            return TapEvent::Double;
        }
        self.last_was_single = true;
        self.last_up_ms = at_ms;
        self.taps += 1;
        TapEvent::Single
    }

    /// 窗口超时清算（双击窗过期——单击坐实，防"孤悬的双击预期"）。
    pub fn expire_double_window(&mut self, at_ms: u32) -> bool {
        if self.last_was_single && at_ms.saturating_sub(self.last_up_ms) > DOUBLE_MS_MAX {
            self.last_was_single = false;
            return true; // 单击坐实
        }
        false
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.taps, self.doubles)
    }
}

// ---------------------------------------------------------------------------
// 深化三：边缘滚动带
// ---------------------------------------------------------------------------

/// 边缘滚动判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeVerdict {
    /// 带内滚动（dy 正负 = 方向）。
    Scroll(i16),
    /// 不在带内 / 多指在场——交主判定器。
    Pass,
}

/// 右缘滚动带（单指专用；两指在场即让位）。
pub struct EdgeScrollZone {
    events: u64,
    /// 让位计数（多指在场时带内移动也不滚——判定窗归属清晰）。
    yielded: u64,
}

impl EdgeScrollZone {
    pub const fn new() -> Self {
        EdgeScrollZone { events: 0, yielded: 0 }
    }

    pub fn judge(&mut self, x: u16, y_prev: u16, y_now: u16, touches: u8) -> EdgeVerdict {
        if touches != 1 {
            if x > ABS_DOMAIN - EDGE_SCROLL_BAND {
                self.yielded += 1;
            }
            return EdgeVerdict::Pass;
        }
        if x <= ABS_DOMAIN - EDGE_SCROLL_BAND {
            return EdgeVerdict::Pass;
        }
        let dy = y_now as i32 - y_prev as i32;
        if dy == 0 {
            return EdgeVerdict::Pass;
        }
        self.events += 1;
        EdgeVerdict::Scroll(dy.clamp(-32767, 32767) as i16)
    }

    pub fn events(&self) -> u64 {
        self.events
    }

    pub fn yielded(&self) -> u64 {
        self.yielded
    }
}

// ---------------------------------------------------------------------------
// 深化四：惯性位移积分
// ---------------------------------------------------------------------------

/// 惯性滑行积分：初速 → 累计位移 + 拍数（截止速度即停）。
/// 返回 (总位移 counts, 拍数)；拍率 60Hz（×100 速度单位每拍衰减 95%）。
pub fn inertia_glide(velocity_x100: i32) -> (i64, u32) {
    let mut v = velocity_x100;
    let mut dist: i64 = 0;
    let mut ticks = 0u32;
    while v.abs() > INERTIA_CUTOFF_X100 && ticks < 600 {
        // 每拍位移 = v/100 × (1/60)s → counts = v/6000。
        dist += v as i64 / 6000;
        v = inertia_step(v);
        ticks += 1;
    }
    (dist, ticks)
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：掌压 / 点按 / 边缘带 / 惯性逐条实摆。
pub fn run_touchpad_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F063-touchpad-deep");

    // ── 掌压拒绝 ──
    let mut pr = PalmRejector::new();
    cs.add(
        "palm_normal_finger",
        pr.judge(2000, 1500, 300, 200) == PalmVerdict::Finger,
        "",
    );
    cs.add(
        "palm_pressure_reject",
        pr.judge(2000, 1500, PALM_PRESSURE_TH + 1, 200)
            == PalmVerdict::Palm(PalmReason::Pressure),
        "",
    );
    cs.add(
        "palm_root_zone_reject",
        pr.judge(2000, PALM_ROOT_Y + 1, 300, 200) == PalmVerdict::Palm(PalmReason::RootZone),
        "",
    );
    cs.add(
        "palm_area_reject",
        pr.judge(2000, 1500, 300, 901) == PalmVerdict::Palm(PalmReason::LargeContact),
        "",
    );
    cs.add(
        "palm_rate_ledger",
        pr.accepted() == 1 && pr.rejection_rate_x100() == 75, // 3 拒 1 收
        "",
    );
    // 原因分账逐笔对得上。
    cs.add(
        "palm_reason_breakdown",
        pr.rejected_by(PalmReason::Pressure) == 1
            && pr.rejected_by(PalmReason::RootZone) == 1
            && pr.rejected_by(PalmReason::LargeContact) == 1,
        "",
    );

    // ── 点按状态机 ──
    // 1) 标准单击（100ms 内、零行程）。
    let mut tc = TapClassifier::new();
    tc.press(1000, 1000, 1_000);
    cs.add("tap_pending_while_down", tc.move_to(1000, 1000, 1_050) == TapEvent::Pending, "");
    cs.add("tap_single", tc.release(1_080) == TapEvent::Single, "");
    // 2) 双击（300ms 内第二次抬起——release(1260) 即双击：距上次抬起 180ms）。
    tc.press(1000, 1000, 1_200);
    cs.add("tap_double", tc.release(1_260) == TapEvent::Double, "");
    // 3) 超行程 → Drag（点按状态即时退出）。
    let mut tc2 = TapClassifier::new();
    tc2.press(1000, 1000, 2_000);
    cs.add("tap_travel_becomes_drag", tc2.move_to(1100, 1000, 2_050) == TapEvent::Drag, "");
    cs.add("tap_drag_closes_state", tc2.release(2_100) == TapEvent::Pending, "");
    // 4) 超时长 → LongPress。
    let mut tc3 = TapClassifier::new();
    tc3.press(1000, 1000, 3_000);
    cs.add("tap_longpress", tc3.release(3_300) == TapEvent::LongPress, "");
    // 5) 双击窗过期 → 单击坐实（不悬着）。
    let mut tc4 = TapClassifier::new();
    tc4.press(1000, 1000, 4_000);
    let _ = tc4.release(4_050);
    cs.add("tap_window_expires", tc4.expire_double_window(4_400), "");
    cs.add(
        "tap_stats_ledger",
        tc4.stats() == (1, 0) && tc.stats() == (1, 1),
        "",
    );

    // ── 边缘滚动带 ──
    // 1) 右缘带内单指移动 → 滚动。
    let mut ez = EdgeScrollZone::new();
    cs.add(
        "edge_scroll_event",
        ez.judge(ABS_DOMAIN - 10, 1000, 1100, 1) == EdgeVerdict::Scroll(100),
        "",
    );
    // 2) 带外 → Pass。
    cs.add(
        "edge_outside_pass",
        ez.judge(ABS_DOMAIN - 200, 1000, 1100, 1) == EdgeVerdict::Pass,
        "",
    );
    // 3) 两指在场 → 让位（不抢多指手势）。
    cs.add(
        "edge_yields_to_multi",
        ez.judge(ABS_DOMAIN - 10, 1000, 1100, 2) == EdgeVerdict::Pass && ez.yielded() == 1,
        "",
    );
    cs.add("edge_event_count", ez.events() == 1, "");

    // ── 惯性积分 ──
    // 1) 初速越大滑得越远（单调性）。
    let (d1, t1) = inertia_glide(300_000);
    let (d2, _t2) = inertia_glide(600_000);
    cs.add("inertia_monotonic", d2 > d1 && d1 > 0 && t1 > 0, "");
    // 2) 低速直接停（低于截止速度 → 零滑行）。
    let (d3, t3) = inertia_glide(INERTIA_CUTOFF_X100);
    cs.add("inertia_cutoff_stops", d3 == 0 && t3 == 0, "");
    // 3) 负速度向负方向滑（方向保真）。
    let (d4, _) = inertia_glide(-300_000);
    cs.add("inertia_direction_kept", d4 < 0, "");
    // 4) 拍数有上限（600 拍硬顶——病态初速不挂死）。
    let (_d5, t5) = inertia_glide(i32::MAX / 2);
    cs.add("inertia_bounded_ticks", t5 <= 600, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn palm_boundary_values_are_finger() {
        let mut pr = PalmRejector::new();
        // 恰在阈值上：仍是手指（严格大于才拒——边界不翻转）。
        assert_eq!(pr.judge(0, 0, PALM_PRESSURE_TH, 900), PalmVerdict::Finger);
        assert_eq!(pr.judge(0, PALM_ROOT_Y, 0, 900), PalmVerdict::Finger);
    }

    #[test]
    fn tap_travel_boundary_exact() {
        let mut tc = TapClassifier::new();
        tc.press(1000, 1000, 0);
        // 恰好 TAP_TRAVEL_MAX 位移 = 点按（<=），再 +1 = 拖拽。
        assert_eq!(tc.move_to(1000 + TAP_TRAVEL_MAX, 1000, 50), TapEvent::Pending);
        let mut tc2 = TapClassifier::new();
        tc2.press(1000, 1000, 0);
        assert_eq!(
            tc2.move_to(1000 + TAP_TRAVEL_MAX + 1, 1000, 50),
            TapEvent::Drag
        );
    }

    #[test]
    fn double_requires_gap_under_limit() {
        let mut tc = TapClassifier::new();
        tc.press(1000, 1000, 0);
        assert_eq!(tc.release(100), TapEvent::Single);
        tc.press(1000, 1000, 500); // 距上次抬起 400ms > 300ms
        assert_eq!(tc.release(560), TapEvent::Single, "间隔超限 → 仍单击");
        assert_eq!(tc.stats(), (2, 0));
    }

    #[test]
    fn inertia_glide_math_closed_form_rough() {
        // 等比级数和首项 ≈ v/6000 × 1/(1−0.95) = v/6000 × 20。
        let (d, _) = inertia_glide(300_000);
        // 首拍 50 counts，级数上限 50×20 = 1000（离散衰减实际略小）。
        assert!(d > 600 && d <= 1000, "滑行距离落在等比级数闭合解附近 d={d}");
    }

    #[test]
    fn edge_scroll_direction_bijective() {
        let mut ez = EdgeScrollZone::new();
        assert_eq!(ez.judge(ABS_DOMAIN - 5, 2000, 1900, 1), EdgeVerdict::Scroll(-100));
        assert_eq!(ez.judge(ABS_DOMAIN - 5, 1900, 2000, 1), EdgeVerdict::Scroll(100));
    }
}

// ===========================================================================
// v3 深化批（F063 · G-B-23）——捏合缩放 FSM / 三指滑动 / 点按分区
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-23 功能定义的实装细化，非新立项）：
// 1. PinchZoomFSM —— 双指捏合缩放状态机：点间距变化比累积 → 超阈值
//     emit 缩放事件（放大/缩小方向 + 幅度 ×100 定点）；间距回中即复位。
// 2. ThreeFingerSwipe —— 三指上/下滑：三触点同向行程超阈值 → 切换
//    事件（方向判定 = 多数点位移方向——个别点抖动不改判）。
// 3. TapZones —— 角分区：触板右下角区点按 → 右键事件（触板无实体
//    键的分区替代面）；分区外照常左键。
// 全部零堆：定长状态 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 捏合触发比（×100 定点 = 15%——复用主检 PINCH_RATIO_X100 口径）。
pub const PINCH_EMIT_STEP_X100: u32 = 25;
/// 三指滑动触发行程（ABS 域）。
pub const SWIPE_TRAVEL_MIN: u16 = 800;
/// 右下角区（x > 3200 且 y > 3200）。
pub const TAP_ZONE_EDGE: u16 = 3200;

// ---------------------------------------------------------------------------
// 深化一：捏合缩放状态机
// ---------------------------------------------------------------------------

/// 缩放事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomEvent {
    /// 放大（张开）幅度 ×100。
    ZoomIn(u32),
    /// 缩小（捏合）幅度 ×100。
    ZoomOut(u32),
    /// 无事件。
    None,
}

/// 双指间距 → 缩放累积状态机。
pub struct PinchZoomFSM {
    /// 基准间距（两指落下时锚定）。
    base_dist: u32,
    /// 累积变化（×100 定点，正 = 张开）。
    accum_x100: i32,
    /// 已 emit 的累积量（回中判定的参照）。
    emitted_x100: i32,
    active: bool,
    events: u64,
}

impl PinchZoomFSM {
    pub const fn new() -> Self {
        PinchZoomFSM {
            base_dist: 0,
            accum_x100: 0,
            emitted_x100: 0,
            active: false,
            events: 0,
        }
    }

    fn dist(x1: u16, y1: u16, x2: u16, y2: u16) -> u32 {
        let dx = x1.abs_diff(x2) as u64;
        let dy = y1.abs_diff(y2) as u64;
        // 整数距离近似（max + min×3/8——经典 3% 误差近似，无平方根）。
        (dx.max(dy) + dx.min(dy) * 3 / 8) as u32
    }

    /// 两指落下：锚定基准间距。
    pub fn begin(&mut self, x1: u16, y1: u16, x2: u16, y2: u16) {
        self.base_dist = Self::dist(x1, y1, x2, y2).max(1);
        self.accum_x100 = 0;
        self.emitted_x100 = 0;
        self.active = true;
    }

    /// 两指移动：更新累积并按步长 emit。
    pub fn update(&mut self, x1: u16, y1: u16, x2: u16, y2: u16) -> ZoomEvent {
        if !self.active || self.base_dist == 0 {
            return ZoomEvent::None;
        }
        let d = Self::dist(x1, y1, x2, y2);
        self.accum_x100 = ((d as i64 - self.base_dist as i64) * 100
            / self.base_dist as i64) as i32;
        let delta = self.accum_x100 - self.emitted_x100;
        if delta >= PINCH_EMIT_STEP_X100 as i32 {
            self.emitted_x100 = self.accum_x100;
            self.events += 1;
            return ZoomEvent::ZoomIn(delta as u32);
        }
        if delta <= -(PINCH_EMIT_STEP_X100 as i32) {
            self.emitted_x100 = self.accum_x100;
            self.events += 1;
            return ZoomEvent::ZoomOut((-delta) as u32);
        }
        ZoomEvent::None
    }

    /// 两指抬起：复位（基准失效——下一次 begin 重锚）。
    pub fn end(&mut self) {
        self.active = false;
        self.base_dist = 0;
        self.accum_x100 = 0;
        self.emitted_x100 = 0;
    }

    pub fn events(&self) -> u64 {
        self.events
    }
}

// ---------------------------------------------------------------------------
// 深化二：三指滑动
// ---------------------------------------------------------------------------

/// 三指滑动判定器：三触点多数方向位移。
pub struct ThreeFingerSwipe {
    /// 上一帧三触点 y（0 = 无基准）。
    prev_y: [u16; 3],
    has_prev: bool,
    events: u64,
}

impl ThreeFingerSwipe {
    pub const fn new() -> Self {
        ThreeFingerSwipe { prev_y: [0; 3], has_prev: false, events: 0 }
    }

    /// 喂三触点 y（x 无关——纵向切换）。
    pub fn update(&mut self, ys: &[u16; 3]) -> i8 {
        if !self.has_prev {
            self.prev_y = *ys;
            self.has_prev = true;
            return 0;
        }
        // 多数方向：三点位移同号计数。
        let mut up = 0u32;
        let mut down = 0u32;
        for k in 0..3 {
            let d = ys[k] as i32 - self.prev_y[k] as i32;
            if d < -(SWIPE_TRAVEL_MIN as i32) {
                up += 1;
            } else if d > SWIPE_TRAVEL_MIN as i32 {
                down += 1;
            }
        }
        self.prev_y = *ys;
        if up >= 2 {
            self.events += 1;
            -1 // 上滑 → 切上一个
        } else if down >= 2 {
            self.events += 1;
            1 // 下滑 → 切下一个
        } else {
            0
        }
    }

    pub fn end(&mut self) {
        self.has_prev = false;
    }

    pub fn events(&self) -> u64 {
        self.events
    }
}

// ---------------------------------------------------------------------------
// 深化三：点按分区
// ---------------------------------------------------------------------------

/// 分区判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneClick {
    RightClick,
    LeftClick,
}

/// 角分区：右下角区 → 右键。
pub fn tap_zone(x: u16, y: u16) -> ZoneClick {
    if x > TAP_ZONE_EDGE && y > TAP_ZONE_EDGE {
        ZoneClick::RightClick
    } else {
        ZoneClick::LeftClick
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：捏合 / 三指 / 分区逐条实摆。
pub fn run_touchpad_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F063-touchpad-v3");

    // ── 捏合 FSM ──
    // 1) 锚定 1000 间距；张开 30% → ZoomIn（幅度 ≥25 步长）。
    let mut pz = PinchZoomFSM::new();
    pz.begin(1000, 2000, 2000, 2000); // dist = 1000
    let ev = pz.update(1000, 2000, 2300, 2000); // dist = 1300 → +30%
    cs.add("pinch_zoomin_emits", matches!(ev, ZoomEvent::ZoomIn(a) if a >= 25 && a <= 35), "");
    // 2) 继续张开 30%（累积 60%，已 emit 30%）→ 再 emit 30。
    let ev2 = pz.update(1000, 2000, 2600, 2000); // dist 1600 → +60%
    cs.add("pinch_accumulates", matches!(ev2, ZoomEvent::ZoomIn(a) if a >= 25), "");
    // 3) 微动不 emit（步长以下）。
    let ev3 = pz.update(1000, 2000, 2650, 2000); // +65% − 60% = 5% < 25%
    cs.add("pinch_deadzone_no_emit", ev3 == ZoomEvent::None, "");
    // 4) 捏回缩小。
    let ev4 = pz.update(1000, 2000, 1300, 2000); // 30% → −35% 相对 emit 65% → −95%？
    cs.add("pinch_zoomout_emits", matches!(ev4, ZoomEvent::ZoomOut(a) if a >= 25), "");
    // 5) 抬起复位。
    pz.end();
    cs.add("pinch_end_inert", pz.update(0, 0, 4095, 4095) == ZoomEvent::None, "");
    cs.add("pinch_events_ledger", pz.events() == 3, "");

    // ── 三指滑动 ──
    let mut tf = ThreeFingerSwipe::new();
    let base = [1000u16, 1100, 1200];
    let _ = tf.update(&base);
    // 三点齐上移 900 → 上滑。
    let up = [100u16, 200, 300];
    cs.add("swipe_up_detected", tf.update(&up) == -1, "");
    // 多数方向：三点从 3000 齐动，两点 −900 一点 +500 → 上滑（个别点抖动不改判）。
    let mut tf5 = ThreeFingerSwipe::new();
    let _ = tf5.update(&[3000u16, 3000, 3000]);
    cs.add("swipe_majority_wins", tf5.update(&[2100u16, 2100, 3500]) == -1, "");
    // 仅一点动 → 无事件（重置后单点位移）。
    let mut tf6 = ThreeFingerSwipe::new();
    let _ = tf6.update(&[3000u16, 3000, 3000]);
    cs.add("swipe_single_point_ignored", tf6.update(&[2100u16, 3000, 3000]) == 0, "");
    // 抬起复位后重锚。
    tf.end();
    let _ = tf.update(&base);
    cs.add("swipe_reanchors_after_end", tf.update(&up) == -1, "");

    // ── 点按分区 ──
    cs.add("zone_corner_right", tap_zone(3500, 3600) == ZoneClick::RightClick, "");
    cs.add("zone_center_left", tap_zone(2000, 2000) == ZoneClick::LeftClick, "");
    cs.add("zone_edge_x_only_left", tap_zone(3500, 2000) == ZoneClick::LeftClick, "");
    cs.add("zone_edge_y_only_left", tap_zone(2000, 3500) == ZoneClick::LeftClick, "");
    // 边界值：恰在线上 = 左键（严格大于）。
    cs.add("zone_boundary_strict", tap_zone(TAP_ZONE_EDGE, TAP_ZONE_EDGE) == ZoneClick::LeftClick, "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn pinch_distance_approx_symmetric() {
        // 近似距离对 x/y 对称（无轴偏好）。
        let d1 = PinchZoomFSM::dist(0, 0, 300, 400);
        let d2 = PinchZoomFSM::dist(0, 0, 400, 300);
        assert_eq!(d1, d2);
        assert!(d1 >= 495 && d1 <= 525, "近似值落在真值 500 附近 d1={d1}");
    }

    #[test]
    fn pinch_zero_base_inert() {
        let mut pz = PinchZoomFSM::new();
        // 未 begin 直接 update → None（无基准不判）。
        assert_eq!(pz.update(0, 0, 4095, 4095), ZoomEvent::None);
    }

    #[test]
    fn swipe_direction_reversal() {
        let mut tf = ThreeFingerSwipe::new();
        let base = [2000u16, 2000, 2000];
        let _ = tf.update(&base);
        let up = [1100u16, 1100, 1100];
        assert_eq!(tf.update(&up), -1);
        let down = [2000u16, 2000, 2000];
        assert_eq!(tf.update(&down), 1, "反向滑动立即改判（多数方向语义）");
    }
}

// ===========================================================================
// v4 深化批（F063 · G-B-23）——手势宏序列 / 指针灵敏度曲线 / 掌缘自适应
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-23 功能定义的实装细化，非新立项）：
// 1. GestureMacro —— 手势宏序列：两指下滑 + 长按 → 已录序列回放判定
//    （宏 = 事件模式序列匹配——可扩展手势的状态机底座）。
// 2. SensitivityCurve —— 指针加速度曲线：16 段整数 LUT（低速精描/
//    高速快移——指距到光标位移的映射面）。
// 3. AdaptivePalm —— 掌缘自适应：按用户误触统计动态微调掌压阈值
//    （误触偏高 → 阈值收紧，正常 → 回默认——旋钮自学习的边界面）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 灵敏度 LUT 段数。
pub const SENS_SEGMENTS: usize = 16;
/// 掌压阈值默认（复用 v2 PALM_PRESSURE_TH 口径）。
pub const ADAPTIVE_PALM_DEFAULT: u16 = 600;
/// 阈值调整步长（每轮 ±20）。
pub const ADAPTIVE_PALM_STEP: u16 = 20;
/// 阈值边界（500-700）。
pub const ADAPTIVE_PALM_LO: u16 = 500;
pub const ADAPTIVE_PALM_HI: u16 = 700;

// ---------------------------------------------------------------------------
// 深化一：手势宏序列
// ---------------------------------------------------------------------------

/// 宏事件（最小集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacroEv {
    TwoFingerSwipeDown,
    LongPress,
    ThreeFingerUp,
}

/// 宏定义（事件序列，≤4）。
pub struct GestureMacro {
    pattern: [Option<MacroEv>; 4],
    n: usize,
    /// 匹配进度。
    progress: usize,
    triggered: u64,
}

impl GestureMacro {
    pub const fn new() -> Self {
        GestureMacro { pattern: [None; 4], n: 0, progress: 0, triggered: 0 }
    }

    /// 录制宏（空序列拒绝）。
    pub fn record(&mut self, evs: &[MacroEv]) -> bool {
        if evs.is_empty() || evs.len() > 4 {
            return false;
        }
        for (k, e) in evs.iter().enumerate() {
            self.pattern[k] = Some(*e);
        }
        for s in self.pattern[evs.len()..].iter_mut() {
            *s = None;
        }
        self.n = evs.len();
        self.progress = 0;
        true
    }

    /// 喂事件：完整走完序列 → 触发（部分匹配保持进度，不匹配回退到
    /// 当前事件自身重开——KMP 简化：无部分重叠回退）。
    pub fn feed(&mut self, ev: MacroEv) -> bool {
        if self.n == 0 {
            return false;
        }
        let expect = self.pattern[self.progress];
        if expect == Some(ev) {
            self.progress += 1;
            if self.progress == self.n {
                self.progress = 0;
                self.triggered += 1;
                return true;
            }
        } else {
            // 不匹配：若当前事件恰好是序列首 → 从 1 起算，否则归零。
            self.progress = if self.pattern[0] == Some(ev) { 1 } else { 0 };
        }
        false
    }

    pub fn progress(&self) -> usize {
        self.progress
    }

    pub fn triggered(&self) -> u64 {
        self.triggered
    }
}

// ---------------------------------------------------------------------------
// 深化二：指针灵敏度曲线（16 段 LUT）
// ---------------------------------------------------------------------------

/// 灵敏度曲线（输入指速 counts/frame → 输出增益 ×100）。
/// 默认曲线：低速 80%、中速 100%、高速 180%（加速度手感）。
pub const SENS_DEFAULT_X100: [u32; SENS_SEGMENTS] = [
    80, 80, 85, 90, 95, 100, 100, 105, 110, 120, 130, 140, 150, 160, 170, 180,
];

/// 曲线查询：input counts（0-4095 域线性映射到 16 段）。
pub fn sens_lookup(input_counts: u32) -> u32 {
    let seg = (input_counts / (4096 / SENS_SEGMENTS as u32)) as usize;
    SENS_DEFAULT_X100[seg.min(SENS_SEGMENTS - 1)]
}

/// 曲线应用：位移 × 增益 / 100（饱和钳制）。
pub fn sens_apply(delta: i32, gain_x100: u32) -> i32 {
    let v = (delta as i64) * (gain_x100 as i64) / 100;
    v.clamp(-4095, 4095) as i32
}

/// 曲线单调不减校验（加速度曲线的形状纪律——只升不降）。
pub fn sens_monotonic_nondecreasing() -> bool {
    for k in 1..SENS_SEGMENTS {
        if SENS_DEFAULT_X100[k] < SENS_DEFAULT_X100[k - 1] {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 深化三：掌缘自适应
// ---------------------------------------------------------------------------

/// 掌压阈值自学习（误触统计驱动，步长钳边）。
pub struct AdaptivePalm {
    threshold: u16,
    false_touches: u32,
    clean_frames: u32,
    adjustments: u64,
}

impl AdaptivePalm {
    pub const fn new() -> Self {
        AdaptivePalm {
            threshold: ADAPTIVE_PALM_DEFAULT,
            false_touches: 0,
            clean_frames: 0,
            adjustments: 0,
        }
    }

    pub fn threshold(&self) -> u16 {
        self.threshold
    }

    /// 喂反馈：误触一次。
    pub fn false_touch(&mut self) {
        self.false_touches += 1;
        self.maybe_adjust();
    }

    /// 喂反馈：1000 帧无误触。
    pub fn clean_batch(&mut self) {
        self.clean_frames += 1;
        self.maybe_adjust();
    }

    /// 调整策略：连续 2 次误触反馈 → 收紧（降阈值）；连续 3 次 clean →
    /// 放松（升阈值回默认方向）；边界钳制。
    fn maybe_adjust(&mut self) {
        if self.false_touches >= 2 {
            self.false_touches = 0;
            self.threshold = self.threshold.saturating_sub(ADAPTIVE_PALM_STEP).max(ADAPTIVE_PALM_LO);
            self.adjustments += 1;
        } else if self.clean_frames >= 3 {
            self.clean_frames = 0;
            self.threshold = self.threshold.saturating_add(ADAPTIVE_PALM_STEP).min(ADAPTIVE_PALM_HI);
            self.adjustments += 1;
        }
    }

    pub fn adjustments(&self) -> u64 {
        self.adjustments
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：宏 / 曲线 / 自适应逐条实摆。
pub fn run_touchpad_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F063-touchpad-v4");

    // ── 手势宏 ──
    let mut gm = GestureMacro::new();
    cs.add("macro_empty_record_refused", !gm.record(&[]), "");
    let seq = [MacroEv::TwoFingerSwipeDown, MacroEv::LongPress];
    cs.add("macro_record_ok", gm.record(&seq), "");
    // 干扰事件不推进。
    cs.add("macro_noise_ignored", !gm.feed(MacroEv::ThreeFingerUp) && gm.progress() == 0, "");
    cs.add(
        "macro_first_match",
        !gm.feed(MacroEv::TwoFingerSwipeDown) && gm.progress() == 1, // 首步未完成不触发
        "",
    );
    cs.add("macro_completes", gm.feed(MacroEv::LongPress) && gm.triggered() == 1, "");
    // 不匹配回退：首事件匹配后再来噪声 → 归零。
    let _ = gm.feed(MacroEv::TwoFingerSwipeDown);
    let _ = gm.feed(MacroEv::ThreeFingerUp);
    cs.add("macro_mismatch_resets", gm.progress() == 0, "");
    // 前缀重合：序列首 = 噪声事件时从 1 续。
    let mut gm2 = GestureMacro::new();
    let _ = gm2.record(&[MacroEv::LongPress, MacroEv::ThreeFingerUp]);
    let _ = gm2.feed(MacroEv::LongPress); // 序列首匹配 → 前进
    cs.add("macro_prefix_kept", !gm2.feed(MacroEv::ThreeFingerUp) == false && gm2.progress() == 0, "");

    // ── 灵敏度曲线 ──
    cs.add("sens_monotonic_shape", sens_monotonic_nondecreasing(), "");
    cs.add("sens_low_speed_gentle", sens_lookup(10) == 80, "");
    cs.add("sens_high_speed_fast", sens_lookup(4000) == 180, "");
    cs.add("sens_mid_neutral", sens_lookup(1600) == 100, ""); // 段 6
    // 应用：低速小位移精描（×0.8）、高速快移（×1.8）。
    cs.add("sens_apply_slow", sens_apply(10, sens_lookup(10)) == 8, "");
    cs.add("sens_apply_fast", sens_apply(1000, sens_lookup(4000)) == 1800, "");
    cs.add("sens_apply_saturates", sens_apply(4095, 180) == 4095, "");

    // ── 掌缘自适应 ──
    let mut ap = AdaptivePalm::new();
    cs.add("apalm_default", ap.threshold() == ADAPTIVE_PALM_DEFAULT, "");
    ap.false_touch();
    ap.false_touch();
    cs.add("apalm_tightens", ap.threshold() == ADAPTIVE_PALM_DEFAULT - ADAPTIVE_PALM_STEP, "");
    for _ in 0..3 {
        ap.clean_batch();
    }
    cs.add("apalm_relaxes_back", ap.threshold() == ADAPTIVE_PALM_DEFAULT, "");
    // 边界钳制：连续收紧到下限。
    for _ in 0..20 {
        ap.false_touch();
        ap.false_touch();
    }
    cs.add("apalm_floor_clamped", ap.threshold() == ADAPTIVE_PALM_LO, "");
    cs.add("apalm_adjust_ledger", ap.adjustments() > 0, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn macro_overlapping_prefix_kmp_lite() {
        // 序列 [A, A, B]：喂 A A A B——第三个 A 既是序列内又重开匹配。
        let mut gm = GestureMacro::new();
        let _ = gm.record(&[MacroEv::LongPress, MacroEv::LongPress, MacroEv::ThreeFingerUp]);
        let _ = gm.feed(MacroEv::LongPress);
        let _ = gm.feed(MacroEv::LongPress);
        let _ = gm.feed(MacroEv::LongPress); // 不匹配 B → 回退但 A 首匹配 → progress 1
        assert_eq!(gm.progress(), 1, "自重叠序列回退到首匹配");
        assert!(gm.feed(MacroEv::ThreeFingerUp) == false || gm.progress() == 2);
    }

    #[test]
    fn sens_curve_covers_all_inputs() {
        for v in [0u32, 255, 512, 1000, 2048, 3000, 4095] {
            let g = sens_lookup(v);
            assert!(g >= 80 && g <= 180, "全输入域落在曲线内");
        }
    }

    #[test]
    fn apalm_ceiling_clamped() {
        let mut ap = AdaptivePalm::new();
        for _ in 0..20 {
            ap.clean_batch();
            ap.clean_batch();
            ap.clean_batch();
        }
        assert_eq!(ap.threshold(), ADAPTIVE_PALM_HI, "上限钳制");
    }
}

// ===========================================================================
// v5 深化批（deep5）：边缘手势 + 滚动加速度曲线
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：边缘手势区（从屏幕边滑入 → 系统手势，与应用手势隔离）
// ---------------------------------------------------------------------------

/// 屏幕尺寸与边缘带宽。
pub const EDGE_BAND_PX: u16 = 24;

/// 边缘手势识别：起点在边缘带内 → 系统手势（返回方向），否则透传应用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeGesture {
    None,
    FromLeft,
    FromRight,
    FromTop,
    FromBottom,
}

pub struct EdgeGestureZone {
    screen_w: u16,
    screen_h: u16,
    /// 起点是否已锁定在边缘带。
    armed: Option<EdgeGesture>,
    /// 累计触发的系统手势数（按方向分账）。
    fired: [u32; 4],
}

impl EdgeGestureZone {
    pub const fn new(screen_w: u16, screen_h: u16) -> Self {
        EdgeGestureZone { screen_w, screen_h, armed: None, fired: [0; 4] }
    }

    /// 按下（x, y）——决定是否劫持为系统手势。
    pub fn press(&mut self, x: u16, y: u16) -> bool {
        self.armed = if x < EDGE_BAND_PX {
            Some(EdgeGesture::FromLeft)
        } else if x >= self.screen_w - EDGE_BAND_PX {
            Some(EdgeGesture::FromRight)
        } else if y < EDGE_BAND_PX {
            Some(EdgeGesture::FromTop)
        } else if y >= self.screen_h - EDGE_BAND_PX {
            Some(EdgeGesture::FromBottom)
        } else {
            None
        };
        self.armed.is_some()
    }

    /// 抬起 + 位移（主轴方向）——armed 时结算系统手势。
    pub fn release(&mut self, dx: i32, dy: i32) -> EdgeGesture {
        match self.armed.take() {
            None => EdgeGesture::None,
            Some(dir) => {
                // 位移须与边一致：左边滑入 dx>0 等。
                let ok = match dir {
                    EdgeGesture::FromLeft => dx > 40,
                    EdgeGesture::FromRight => dx < -40,
                    EdgeGesture::FromTop => dy > 40,
                    EdgeGesture::FromBottom => dy < -40,
                    EdgeGesture::None => false,
                };
                if ok {
                    let slot = match dir {
                        EdgeGesture::FromLeft => 0,
                        EdgeGesture::FromRight => 1,
                        EdgeGesture::FromTop => 2,
                        _ => 3,
                    };
                    self.fired[slot] += 1;
                    dir
                } else {
                    EdgeGesture::None // 位移不足 → 还给应用（不吞）
                }
            }
        }
    }

    pub fn fired(&self, dir: usize) -> u32 {
        self.fired[dir]
    }
}

// ---------------------------------------------------------------------------
// 深化二：双指滚动加速度曲线（速度 → 加速因子 ×100 分段）
// ---------------------------------------------------------------------------

/// 加速曲线：慢速 1x（≤200）、中速线性 1x-2x（200-800）、快速 2x（>800）。
/// 返回滚动放大因子（×100）。
pub fn scroll_accel_factor_x100(speed_counts_per_s: u32) -> u32 {
    const SLOW_MAX: u32 = 200;
    const FAST_MIN: u32 = 800;
    if speed_counts_per_s <= SLOW_MAX {
        return 100;
    }
    if speed_counts_per_s >= FAST_MIN {
        return 200;
    }
    // 线性段：100 + (v-200)×100/600
    100 + (speed_counts_per_s - SLOW_MAX) * 100 / (FAST_MIN - SLOW_MAX)
}

/// 滚动积分器：累积带方向位移，加速度因子作用于速度段。
pub struct ScrollAccel {
    /// 累积位移（可为负）。
    accum: i32,
    /// 最近速度采样（counts/s）。
    last_speed: u32,
}

impl ScrollAccel {
    pub const fn new() -> Self {
        ScrollAccel { accum: 0, last_speed: 0 }
    }

    /// 一段滚轮输入：counts（有符号）+ 当前速度 → 返回放大后位移。
    pub fn feed(&mut self, counts: i32, speed: u32) -> i32 {
        self.last_speed = speed;
        let factor = scroll_accel_factor_x100(speed) as i64;
        let moved = (counts as i64 * factor / 100) as i32;
        self.accum += moved;
        moved
    }

    /// 取走累积（读后不清——读并清用 take）。
    pub fn accumulated(&self) -> i32 {
        self.accum
    }

    pub fn take(&mut self) -> i32 {
        let v = self.accum;
        self.accum = 0;
        v
    }

    pub fn last_speed(&self) -> u32 {
        self.last_speed
    }
}

// ---------------------------------------------------------------------------
// 深化三：手掌误触学习统计（大接触面 + 低压力 → 记误触；周内比率 → 阈值建议）
// ---------------------------------------------------------------------------

/// 误触学习账：滚动 64 样本，统计大面占比。
pub struct PalmLearner {
    /// (接触面, 是否误触) 环形。
    ring: [(u16, bool); 64],
    pos: usize,
    filled: usize,
    /// 当前阈值（面积 ≥ 此值判可疑）。
    area_threshold: u16,
}

/// 阈值步长与边界。
pub const PALM_AREA_STEP: u16 = 40;
pub const PALM_AREA_MIN: u16 = 400;
pub const PALM_AREA_MAX: u16 = 1200;

impl PalmLearner {
    pub const fn new() -> Self {
        PalmLearner { ring: [(0, false); 64], pos: 0, filled: 0, area_threshold: 800 }
    }

    /// 记录一个触点（面积 + 实际是否误触——由上层标注）。
    pub fn observe(&mut self, area: u16, was_palm: bool) {
        self.ring[self.pos] = (area, was_palm);
        self.pos = (self.pos + 1) % 64;
        if self.filled < 64 {
            self.filled += 1;
        }
    }

    /// 大面样本中误触比率（×100）。样本 <16 → None（不猜）。
    pub fn big_area_palm_pct(&self) -> Option<u32> {
        if self.filled < 16 {
            return None;
        }
        let mut big = 0u32;
        let mut big_palm = 0u32;
        for k in 0..self.filled {
            let (a, palm) = self.ring[k];
            if a >= self.area_threshold {
                big += 1;
                if palm {
                    big_palm += 1;
                }
            }
        }
        if big == 0 {
            return Some(0);
        }
        Some(big_palm * 100 / big)
    }

    /// 阈值建议：大面误触率 >60% → 阈值下调（抓更多可疑）；<20% → 上调（少打扰）。
    pub fn suggest_threshold(&self) -> Option<u16> {
        let pct = self.big_area_palm_pct()?;
        if pct > 60 {
            Some((self.area_threshold - PALM_AREA_STEP).max(PALM_AREA_MIN))
        } else if pct < 20 {
            Some((self.area_threshold + PALM_AREA_STEP).min(PALM_AREA_MAX))
        } else {
            Some(self.area_threshold) // 20-60% 稳态
        }
    }

    pub fn apply_suggested(&mut self) -> bool {
        match self.suggest_threshold() {
            Some(t) if t != self.area_threshold => {
                self.area_threshold = t;
                true
            }
            _ => false,
        }
    }

    pub fn threshold(&self) -> u16 {
        self.area_threshold
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_touchpad_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F063-touchpad-v5");

    // ── 边缘手势 ──
    // 1) 左缘起手 + 右滑 → FromLeft。
    let mut ez = EdgeGestureZone::new(1920, 1080);
    cs.add(
        "edge_left_swipe",
        ez.press(10, 540) && ez.release(120, 5) == EdgeGesture::FromLeft && ez.fired(0) == 1,
        "",
    );
    // 2) 中央起手 → 不劫持（透传应用）。
    let mut ez2 = EdgeGestureZone::new(1920, 1080);
    cs.add("edge_center_passthrough", !ez2.press(960, 540) && ez2.release(200, 0) == EdgeGesture::None, "");
    // 3) 边缘起手但位移不足 → 归还应用（不吞事件）。
    let mut ez3 = EdgeGestureZone::new(1920, 1080);
    cs.add("edge_short_swipe_returned", ez3.press(5, 100) && ez3.release(10, 0) == EdgeGesture::None && ez3.fired(0) == 0, "");
    // 4) 上缘起手 + 下滑（呼出通知中心方向）→ FromTop。
    let mut ez4 = EdgeGestureZone::new(1920, 1080);
    cs.add("edge_top_swipe", ez4.press(960, 3) && ez4.release(0, 150) == EdgeGesture::FromTop, "");

    // ── 滚动加速 ──
    // 5) 慢速 1x / 快速 2x。
    cs.add(
        "scroll_accel_ends",
        scroll_accel_factor_x100(150) == 100 && scroll_accel_factor_x100(900) == 200,
        "",
    );
    // 6) 中段线性：500 → 100 + 300×100/600 = 150。
    cs.add("scroll_accel_mid_linear", scroll_accel_factor_x100(500) == 150, "");
    // 7) 积分器：同样 counts，快速下位移翻倍。
    let mut sa1 = ScrollAccel::new();
    let m1 = sa1.feed(10, 150);
    let mut sa2 = ScrollAccel::new();
    let m2 = sa2.feed(10, 900);
    cs.add("scroll_accel_doubles_fast", m1 == 10 && m2 == 20, "");
    // 8) take 清账。
    let _ = sa2.feed(10, 900);
    cs.add("scroll_take_clears", sa2.take() == 40 && sa2.accumulated() == 0, "");

    // ── 掌压学习 ──
    // 9) 大面全是误触 → 建议下调阈值。
    let mut pl = PalmLearner::new();
    for k in 0..16 {
        pl.observe(if k % 2 == 0 { 900 } else { 200 }, true); // 大面全 palm
    }
    cs.add("palm_suggest_lower", pl.suggest_threshold() == Some(800 - PALM_AREA_STEP), "");
    // 10) 大面零误触 → 上调。
    let mut pl2 = PalmLearner::new();
    for k in 0..16 {
        pl2.observe(if k % 2 == 0 { 900 } else { 200 }, false);
    }
    cs.add("palm_suggest_raise", pl2.suggest_threshold() == Some(800 + PALM_AREA_STEP), "");
    // 11) 样本不足 → None（不猜）。
    let mut pl3 = PalmLearner::new();
    for _ in 0..8 {
        pl3.observe(900, true);
    }
    cs.add("palm_insufficient_none", pl3.suggest_threshold().is_none(), "");
    // 12) 阈值边界钳制：连升到顶。
    let mut pl4 = PalmLearner::new();
    for _ in 0..12 {
        for _ in 0..16 {
            pl4.observe(900, false); // 反复触发上调
        }
        pl4.apply_suggested();
    }
    cs.add("palm_threshold_clamped_max", pl4.threshold() == PALM_AREA_MAX, "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn edge_right_swipe_negative() {
        let mut ez = EdgeGestureZone::new(800, 600);
        assert!(ez.press(799, 300));
        assert_eq!(ez.release(-200, 0), EdgeGesture::FromRight);
        assert_eq!(ez.fired(1), 1);
    }

    #[test]
    fn scroll_accel_exact_midpoints() {
        assert_eq!(scroll_accel_factor_x100(200), 100, "慢速上界");
        assert_eq!(scroll_accel_factor_x100(800), 200, "快速下界");
        assert_eq!(scroll_accel_factor_x100(800 - 1), 199, "线性段末点");
    }

    #[test]
    fn palm_ring_overwrites() {
        let mut pl = PalmLearner::new();
        // 64 个大面误触 + 64 个小面 → 环全换 → 大面占比 0 → 建议上调。
        for _ in 0..64 {
            pl.observe(900, true);
        }
        for _ in 0..64 {
            pl.observe(200, false);
        }
        assert_eq!(pl.big_area_palm_pct(), Some(0));
        assert_eq!(pl.suggest_threshold(), Some(800 + PALM_AREA_STEP));
    }
}
