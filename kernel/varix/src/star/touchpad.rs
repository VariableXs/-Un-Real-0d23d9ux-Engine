//! F063 触控板手势前瞻 · 完整设计（STAR I 主册 G-B-23）。
//!
//! **判据（主册）**：评估报告产出（报文格式 / 多指识别可行性 / 延迟实测
//! 三节）即本项达标——**评估件以报告为交付**。
//!
//! **定位纪律（主册原文）**：Y7000 触控板多指手势评估项——数据面不锁死，
//! 承诺面按实测定级。定级标准：延迟 <40ms 且误触率 <1% 才可承诺；不通过
//! 则设置页诚实标注「本机触控板评估中」。
//!
//! **设计要点（主册）**：
//! - 采集维度：触点数 / 坐标轨迹 / 压力（若报文有）/ 时间戳；
//! - 手势判定窗口 80ms（窗口内轨迹合并判定）；
//! - 惯性滚动模型评估：200ms ease-out、速度衰减指数 0.95 档；
//! - 报文解析失败 → 降级为单点触摸（基础可用）；
//! - 手势与输入法组合冲突 → 手势让位（打字优先）；
//! - 采集数据仅本机诊断用（不上传）。
//!
//! 误触率口径（评估诚实性关键）：`judged` 分母只计**多指窗口**（触点数
//! ≥2）——单指光标流量既不是手势也不是误触，混进分母会把误触率稀释成
//! 假绿。多指窗口分类失败（None）或 IME 让位各计独立计数，报告如实分列。
//!
//! 手势识别思路参照 libinput gesture 状态机；采集自研（input-probe 扩展）。
//! 一切时间注入式（微秒戳），宿主测试确定复现。

use crate::checks::CheckSet;
use crate::star::sbase::{pct_near, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 手势判定窗口（μs）——窗口内轨迹合并判定。
pub const GESTURE_WINDOW_US: u64 = 80_000;

/// 惯性滚动时长（ms）——ease-out。
pub const INERTIA_MS: u32 = 200;

/// 惯性速度衰减指数（0.95 档，定点 ×1000）。
pub const INERTIA_DECAY_MU: u32 = 950;

/// 定级延迟门（μs）：P95 < 40ms 才可承诺。
pub const GRADE_DELAY_P95_LIMIT_US: u64 = 40_000;

/// 定级误触门（万分比）：误触率 <1% 才可承诺。
pub const GRADE_FALSE_PPT_LIMIT: u32 = 100;

/// 定级最小样本量：延迟实测不足 100 个样本不下结论（数据诚实）。
pub const GRADE_MIN_DELAY_SAMPLES: usize = 100;

/// 判定窗口容量：一次手势窗口最多缓存 64 个样本。
const WIN_CAP: usize = 64;

/// 延迟环容量：评估期滚动保留最近 1024 个延迟实测值。
const DELAY_RING_CAP: usize = 1024;

/// 两触点间距变化判捏合的阈值（1/256 mm 粒度）。
const PINCH_DELTA_THRESH: i32 = 64;

/// 单轴位移判滚动/切换的阈值（1/256 mm 粒度）。
pub const SWIPE_DELTA_THRESH: i32 = 128;

// ---------------------------------------------------------------------------
// HID 报文采集数据面
// ---------------------------------------------------------------------------

/// 单帧触控样本（采集维度四项：触点数/坐标/压力/时间戳）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchSample {
    /// 采集时间戳（μs，注入式）。
    pub ts_us: u64,
    /// 触点数（0-5）。
    pub contacts: u8,
    /// 主触点 X（1/256 mm）。
    pub x: i32,
    /// 主触点 Y（1/256 mm）。
    pub y: i32,
    /// 压力（0=报文未携带或面板无压力传感器——覆盖率入报告第一节）。
    pub pressure: u16,
    /// 次触点 X（单指帧与主触点同值，间距 0）。
    pub x2: i32,
    /// 次触点 Y。
    pub y2: i32,
}

impl TouchSample {
    /// 两触点间距（1/256 mm，整数勾股 + 牛顿整平方根——core 无 f64）。
    pub fn spread(&self) -> i32 {
        let (dx, dy) = ((self.x - self.x2) as i64, (self.y - self.y2) as i64);
        let sq = (dx * dx + dy * dy) as u64;
        if sq == 0 {
            return 0;
        }
        let mut r = sq;
        let mut g = (sq >> 1).max(1);
        while g < r {
            r = g;
            g = (g + sq / g) >> 1;
        }
        r as i32
    }
}

/// 解析结果：样本、或显性拒绝原因（畸形输入不 panic、不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseOutcome {
    /// 正常解析出一帧样本。
    Sample(TouchSample),
    /// 报文长度或报告 ID 不符 input-probe 描述符 → 丢弃。
    BadLength,
    /// 触点数字段越界（>5 或保留位非零）→ 丢弃。
    BadContacts,
}

/// HID 报文解析器（input-probe 采集约定，12 字节定长）：
///
/// | 字节 | 含义 |
/// | --- | --- |
/// | 0 | 报告 ID（0x01） |
/// | 1 | bit3-0 触点数；bit7-4 保留（必须 0） |
/// | 2-3 | 主触点 X（i16 LE，1/256 mm） |
/// | 4-5 | 主触点 Y |
/// | 6-7 | 次触点 X（单指帧与主触点同值） |
/// | 8-9 | 次触点 Y |
/// | 10-11 | 压力（u16 LE，0=无） |
pub struct ReportParser {
    /// 压力字段覆盖统计（携带非零压力的帧 / 成功帧）。
    pressure_seen: u32,
    pressure_frames: u32,
    /// 格式异常计数（两类分开记，报告第一节如实呈现）。
    pub bad_length: u32,
    pub bad_contacts: u32,
    pub ok_frames: u32,
}

impl ReportParser {
    pub fn new() -> ReportParser {
        ReportParser { pressure_seen: 0, pressure_frames: 0, bad_length: 0, bad_contacts: 0, ok_frames: 0 }
    }

    /// 解析一帧 12 字节 HID 报告。
    pub fn parse(&mut self, raw: &[u8], ts_us: u64) -> ParseOutcome {
        if raw.len() != 12 || raw[0] != 0x01 {
            self.bad_length += 1;
            return ParseOutcome::BadLength;
        }
        let contacts = raw[1] & 0x0F;
        if contacts > 5 || raw[1] >> 4 != 0 {
            self.bad_contacts += 1;
            return ParseOutcome::BadContacts;
        }
        let x = i16::from_le_bytes([raw[2], raw[3]]) as i32;
        let y = i16::from_le_bytes([raw[4], raw[5]]) as i32;
        let x2 = i16::from_le_bytes([raw[6], raw[7]]) as i32;
        let y2 = i16::from_le_bytes([raw[8], raw[9]]) as i32;
        let pressure = u16::from_le_bytes([raw[10], raw[11]]);
        self.ok_frames += 1;
        self.pressure_frames += 1;
        if pressure != 0 {
            self.pressure_seen += 1;
        }
        ParseOutcome::Sample(TouchSample { ts_us, contacts, x, y, pressure, x2, y2 })
    }

    /// 压力字段覆盖率（‰，0=报文完全不带压力）。
    pub fn pressure_coverage_ppt(&self) -> u32 {
        if self.pressure_frames == 0 {
            return 0;
        }
        self.pressure_seen * 1000 / self.pressure_frames
    }
}

impl Default for ReportParser {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 手势判定状态机（80ms 窗口）
// ---------------------------------------------------------------------------

/// 窗口内判定的手势类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// 双指滚动。
    TwoFingerScroll,
    /// 三指切换（上滑进任务视图 F081 前瞻）。
    ThreeFingerSwipe,
    /// 捏合缩放。
    PinchZoom,
    /// 窗口内无可判手势（光标移动/轻点/未分类多指）。
    None,
}

/// 手势判定窗口状态机：样本入窗，窗口闭合（80ms 边界或触点归零）时出裁决。
pub struct GestureJudge {
    win: [Option<TouchSample>; WIN_CAP],
    win_len: usize,
    win_open_us: u64,
    /// 多指窗口判定总数（分母——单指光标流量不进手势统计）。
    pub judged: u32,
    /// 多指窗口未分类数（误触率分子之一）。
    pub misfires: u32,
    /// 多指窗口因 IME 让位数（打字优先纪律的执行痕迹）。
    pub yielded: u32,
}

impl GestureJudge {
    pub fn new() -> GestureJudge {
        GestureJudge { win: [const { None }; WIN_CAP], win_len: 0, win_open_us: 0, judged: 0, misfires: 0, yielded: 0 }
    }

    /// 样本入窗。窗口开启超 80ms 或触点归零 → 先闭合旧窗。
    /// 返回 Some((手势, 闭合时刻)) 表示旧窗刚闭合出裁决。
    pub fn feed(&mut self, s: TouchSample, ime_active: bool) -> Option<(Gesture, u64)> {
        let mut closed = None;
        if self.win_len > 0
            && (s.ts_us.saturating_sub(self.win_open_us) >= GESTURE_WINDOW_US || s.contacts == 0)
        {
            closed = Some((self.close(ime_active), s.ts_us));
            self.reset();
        }
        if s.contacts == 0 {
            return closed;
        }
        if self.win_len == 0 {
            self.win_open_us = s.ts_us;
        }
        if self.win_len < WIN_CAP {
            self.win[self.win_len] = Some(s);
            self.win_len += 1;
        }
        closed
    }

    /// 显式闭合（评估收尾用）。
    pub fn close_now(&mut self, ime_active: bool) -> Gesture {
        let g = if self.win_len == 0 { Gesture::None } else { self.close(ime_active) };
        self.reset();
        g
    }

    fn close(&mut self, ime_active: bool) -> Gesture {
        if self.win_len == 0 {
            return Gesture::None;
        }
        let mut max_contacts = 0u8;
        for slot in self.win.iter().take(self.win_len) {
            if let Some(s) = *slot {
                max_contacts = max_contacts.max(s.contacts);
            }
        }
        if max_contacts < 2 {
            return Gesture::None; // 光标流量：不进手势统计。
        }
        self.judged += 1;
        // 手势让位：打字优先（独立计数，不算检测失败）。
        if ime_active {
            self.yielded += 1;
            return Gesture::None;
        }
        let first = self.win[0].unwrap();
        let last = self.win[self.win_len - 1].unwrap();
        let g = if max_contacts >= 3 {
            Gesture::ThreeFingerSwipe
        } else {
            let d_spread = last.spread() - first.spread();
            if d_spread.abs() > PINCH_DELTA_THRESH {
                Gesture::PinchZoom
            } else if (last.y - first.y).abs() > SWIPE_DELTA_THRESH {
                Gesture::TwoFingerScroll
            } else {
                Gesture::None // 多指但未过任何阈值 → 误触统计。
            }
        };
        if g == Gesture::None {
            self.misfires += 1;
        }
        g
    }

    fn reset(&mut self) {
        for slot in self.win.iter_mut() {
            *slot = None;
        }
        self.win_len = 0;
    }

    /// 误触率（万分比）：多指窗口中未分类的占比。
    pub fn misfire_ppt(&self) -> u32 {
        if self.judged == 0 {
            return 0;
        }
        self.misfires * 10_000 / self.judged
    }
}

impl Default for GestureJudge {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 惯性滚动模型（200ms ease-out · 衰减 0.95 档）
// ---------------------------------------------------------------------------

/// 惯性滚动评估器：以末速起步，按 0.95 档逐帧衰减积分位移。
pub struct InertiaModel {
    /// 单帧步长 ms（评估口径 17ms ≈ 60fps）。
    frame_ms: u32,
}

impl InertiaModel {
    pub const fn new() -> InertiaModel {
        InertiaModel { frame_ms: 17 }
    }

    /// 给定初速（px/s），模拟到 200ms 收敛，返回累计位移（px）与帧数。
    /// ease-out 语义 = 位移增量绝对值单调不增（debug 断言护栏）。
    pub fn rollout(&self, v0_px_s: i64) -> (i64, u32) {
        let mut v = v0_px_s;
        let mut elapsed_ms = 0u32;
        let mut pos: i64 = 0;
        let mut prev_step: Option<i64> = None;
        let mut frames = 0u32;
        while elapsed_ms < INERTIA_MS {
            let step = v * self.frame_ms as i64 / 1000;
            pos += step;
            if let Some(p) = prev_step {
                debug_assert!(step.abs() <= p.abs(), "ease-out violated at {}ms", elapsed_ms);
            }
            prev_step = Some(step);
            v = v * INERTIA_DECAY_MU as i64 / 1000;
            elapsed_ms += self.frame_ms;
            frames += 1;
            if step == 0 {
                break;
            }
        }
        (pos, frames)
    }
}

impl Default for InertiaModel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 评估报告（本项交付物本体：三节 + 定级）
// ---------------------------------------------------------------------------

/// 定级结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    /// 延迟 <40ms 且误触率 <1% 且样本充足 —— 可承诺手势批次。
    Commitable,
    /// 未达定级标准或数据不足 —— 设置页诚实标注「本机触控板评估中」。
    Evaluating,
}

/// 第一节 · 报文格式。
#[derive(Clone, Copy, Debug)]
pub struct FormatSection {
    pub ok_frames: u32,
    pub bad_length: u32,
    pub bad_contacts: u32,
    /// 压力字段覆盖率（‰）。
    pub pressure_coverage_ppt: u32,
}

/// 第二节 · 多指识别可行性。
#[derive(Clone, Copy, Debug)]
pub struct MultiTouchSection {
    /// 多指窗口判定总数。
    pub judged: u32,
    pub scrolls: u32,
    pub swipes: u32,
    pub pinches: u32,
    /// 多指窗口未分类数。
    pub misfires: u32,
    /// 多指窗口因 IME 让位数。
    pub yielded: u32,
    /// 误触率（万分比）。
    pub misfire_ppt: u32,
    /// 评估期见到的最大触点数。
    pub max_contacts_seen: u8,
}

/// 第三节 · 延迟实测。
#[derive(Clone, Copy, Debug)]
pub struct LatencySection {
    /// 采集 → 上屏（μs，注入式打点差）。
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub samples: usize,
}

/// 评估报告：三节齐备 + 定级结论（交付物本体）。
#[derive(Clone, Copy, Debug)]
pub struct EvaluationReport {
    pub format: FormatSection,
    pub multitouch: MultiTouchSection,
    pub latency: LatencySection,
    pub grade: Grade,
}

/// 采集评估会话：解析器 + 判定器 + 惯性模型 + 延迟环。
pub struct TouchpadProbe {
    parser: ReportParser,
    judge: GestureJudge,
    inertia: InertiaModel,
    delay_ring: RingLog<u64, DELAY_RING_CAP>,
    max_contacts_seen: u8,
    gesture_counts: [u32; 3], // scroll / swipe / pinch
}

impl TouchpadProbe {
    pub fn new() -> TouchpadProbe {
        TouchpadProbe {
            parser: ReportParser::new(),
            judge: GestureJudge::new(),
            inertia: InertiaModel::new(),
            delay_ring: RingLog::new(),
            max_contacts_seen: 0,
            gesture_counts: [0; 3],
        }
    }

    /// 采集一帧：解析 → 入判定窗。畸形帧显性拒绝（降级单点的数据依据）。
    pub fn feed(&mut self, raw: &[u8], ts_us: u64, ime_active: bool) -> ParseOutcome {
        let out = self.parser.parse(raw, ts_us);
        if let ParseOutcome::Sample(s) = out {
            self.max_contacts_seen = self.max_contacts_seen.max(s.contacts);
            if let Some((g, _)) = self.judge.feed(s, ime_active) {
                self.tally(g);
            }
        }
        out
    }

    /// 延迟打点：采集 → 上屏的时间差（μs）入环。
    pub fn record_delay(&mut self, sample_ts_us: u64, present_ts_us: u64) {
        self.delay_ring.push(present_ts_us.saturating_sub(sample_ts_us));
    }

    /// 惯性模型评估入口（供报告附注与调参）。
    pub fn inertia_rollout(&self, v0_px_s: i64) -> (i64, u32) {
        self.inertia.rollout(v0_px_s)
    }

    /// 会话收尾：闭合判定窗并产出三节报告（本项交付物）。
    pub fn finish(&mut self, ime_active: bool) -> EvaluationReport {
        let g = self.judge.close_now(ime_active);
        self.tally(g);

        let delays: Vec<u64> = self.delay_ring.newest_first().iter().copied().collect();
        let latency = LatencySection {
            p50_us: pct_near(&delays, 50),
            p95_us: pct_near(&delays, 95),
            p99_us: pct_near(&delays, 99),
            samples: delays.len(),
        };
        let format = FormatSection {
            ok_frames: self.parser.ok_frames,
            bad_length: self.parser.bad_length,
            bad_contacts: self.parser.bad_contacts,
            pressure_coverage_ppt: self.parser.pressure_coverage_ppt(),
        };
        let multitouch = MultiTouchSection {
            judged: self.judge.judged,
            scrolls: self.gesture_counts[0],
            swipes: self.gesture_counts[1],
            pinches: self.gesture_counts[2],
            misfires: self.judge.misfires,
            yielded: self.judge.yielded,
            misfire_ppt: self.judge.misfire_ppt(),
            max_contacts_seen: self.max_contacts_seen,
        };
        let grade = if delays.len() >= GRADE_MIN_DELAY_SAMPLES
            && latency.p95_us < GRADE_DELAY_P95_LIMIT_US
            && multitouch.misfire_ppt < GRADE_FALSE_PPT_LIMIT
            && multitouch.judged > 0
        {
            Grade::Commitable
        } else {
            Grade::Evaluating
        };
        EvaluationReport { format, multitouch, latency, grade }
    }

    fn tally(&mut self, g: Gesture) {
        match g {
            Gesture::TwoFingerScroll => self.gesture_counts[0] += 1,
            Gesture::ThreeFingerSwipe => self.gesture_counts[1] += 1,
            Gesture::PinchZoom => self.gesture_counts[2] += 1,
            Gesture::None => {}
        }
    }
}

impl Default for TouchpadProbe {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 构造一帧合法 12 字节双指报文。
fn frame2(x: i32, y: i32, x2: i32, y2: i32) -> [u8; 12] {
    frame_n(2, x, y, x2, y2, 0)
}

/// 构造指定触点数/压力的 12 字节报文。
fn frame_n(contacts: u8, x: i32, y: i32, x2: i32, y2: i32, pressure: u16) -> [u8; 12] {
    let mut f = [0u8; 12];
    f[0] = 0x01;
    f[1] = contacts & 0x0F;
    f[2..4].copy_from_slice(&(x as i16).to_le_bytes());
    f[4..6].copy_from_slice(&(y as i16).to_le_bytes());
    f[6..8].copy_from_slice(&(x2 as i16).to_le_bytes());
    f[8..10].copy_from_slice(&(y2 as i16).to_le_bytes());
    f[10..12].copy_from_slice(&pressure.to_le_bytes());
    f
}

/// F063 自检（判据：评估报告三节齐备即达标）。
pub fn run_touchpad_checks() -> CheckSet {
    let mut set = CheckSet::new("F063-touchpad");
    let mut p = ReportParser::new();

    // 1. 正常帧解析。
    let frame = frame2(0x1000, 0x2000, 0x1010, 0x2010);
    let ok = matches!(p.parse(&frame, 1_000), ParseOutcome::Sample(_));
    set.add("parse normal frame", ok, "");

    // 2. 畸形报文全防御（2000 轮注入：长度 0-16 / ID / 触点数 / 保留位越界，
    //    不 panic、通过的样本必须满足触点 ≤5 规格约束）。
    let mut x: u32 = 0x9E3779B9;
    let mut survived = true;
    for i in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let n = (x % 16) as usize;
        let mut raw = [0u8; 16];
        for b in raw.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = x as u8;
        }
        if let ParseOutcome::Sample(s) = p.parse(&raw[..n], i as u64 * 100) {
            if s.contacts > 5 {
                survived = false;
            }
        }
    }
    set.add("malformed fuzz 2000 rounds no panic", survived, "");

    // 3. 两类显性拒绝路径都可观测。
    let bad = p.parse(&[0x02, 0xFF], 2_000);
    set.add("bad length explicit reject", bad == ParseOutcome::BadLength, "");
    let bad_ct = p.parse(&frame_n(6, 0, 0, 0, 0, 0), 2_100);
    set.add("bad contacts explicit reject", bad_ct == ParseOutcome::BadContacts, "");

    // 4. 手势判定：80ms 窗口内双指 Y 位移 → 滚动。
    let mut judge = GestureJudge::new();
    for (i, ts) in [0u64, 20_000, 40_000, 60_000].iter().enumerate() {
        let y = 800 + (i as i32) * 60;
        let _ = judge.feed(TouchSample { ts_us: *ts, contacts: 2, x: 100, y, pressure: 0, x2: 140, y2: y }, false);
    }
    set.add("two-finger scroll judged in 80ms window", judge.close_now(false) == Gesture::TwoFingerScroll, "");

    // 5. 手势让位：IME 激活 → None，独立计数 yielded，不计误触。
    let mut judge2 = GestureJudge::new();
    for (i, ts) in [0u64, 20_000, 40_000].iter().enumerate() {
        let y = 800 + (i as i32) * 60;
        let _ = judge2.feed(TouchSample { ts_us: *ts, contacts: 2, x: 100, y, pressure: 0, x2: 140, y2: y }, true);
    }
    let g2 = judge2.close_now(true);
    set.add("gesture yields to typing (IME)", g2 == Gesture::None && judge2.yielded == 1 && judge2.misfires == 0, "");

    // 6. 捏合判定：间距显著变化。
    let mut judge3 = GestureJudge::new();
    let _ = judge3.feed(TouchSample { ts_us: 0, contacts: 2, x: 100, y: 100, pressure: 0, x2: 116, y2: 100 }, false);
    let _ = judge3.feed(TouchSample { ts_us: 40_000, contacts: 2, x: 100, y: 100, pressure: 0, x2: 400, y2: 100 }, false);
    set.add("pinch zoom judged", judge3.close_now(false) == Gesture::PinchZoom, "");

    // 7. 窗口超时闭合：80ms 边界处新样本触发旧窗先裁决。
    let mut judge4 = GestureJudge::new();
    let _ = judge4.feed(TouchSample { ts_us: 0, contacts: 2, x: 100, y: 800, pressure: 0, x2: 140, y2: 800 }, false);
    let closed = judge4.feed(
        TouchSample { ts_us: GESTURE_WINDOW_US, contacts: 2, x: 100, y: 860, pressure: 0, x2: 140, y2: 860 },
        false,
    );
    set.add("window closes at 80ms boundary", closed.is_some(), "");

    // 8. 惯性 ease-out：200ms 内收敛。
    let inertia = InertiaModel::new();
    let (pos, frames) = inertia.rollout(2000);
    set.add("inertia 200ms ease-out converges", pos > 0 && frames <= (INERTIA_MS + 16) / 17, "");

    // 9. 定级门实测（行为级）：120 个双指滚动窗全部分类成功、延迟全 10ms
    //    → Commitable。
    let mut probe_ok = TouchpadProbe::new();
    for w in 0..120u32 {
        for k in 0..4u32 {
            let y = 800 + ((w * 4 + k) as i32) * 60;
            let f = frame2(100, y, 140, y);
            let ts = (w as u64 * 100_000) + (k as u64 * 20_000);
            let _ = probe_ok.feed(&f, ts, false);
            probe_ok.record_delay(ts, ts + 10_000);
        }
    }
    let rep_ok = probe_ok.finish(false);
    set.add(
        "commitable grade end-to-end (delay ok + misfire ok)",
        rep_ok.grade == Grade::Commitable
            && rep_ok.multitouch.scrolls == 120
            && rep_ok.multitouch.misfire_ppt == 0,
        "",
    );

    // 10. 定级门实测（行为级）：延迟达标但 10% 多指窗口未分类 → 误触 10%
    //     >1% 门 → Evaluating（诚实不承诺）。
    let mut probe_bad = TouchpadProbe::new();
    for w in 0..120u32 {
        let ambiguous = w % 10 == 0; // 12/120 窗口位移不过阈值
        for k in 0..4u32 {
            let y = if ambiguous { 800 + (k as i32) * 2 } else { 800 + (k as i32) * 60 };
            let f = frame2(100, y, 140, y);
            let ts = (w as u64 * 100_000) + (k as u64 * 20_000);
            let _ = probe_bad.feed(&f, ts, false);
            probe_bad.record_delay(ts, ts + 10_000);
        }
    }
    let rep_bad = probe_bad.finish(false);
    set.add(
        "misfire gate rejects honestly",
        rep_bad.grade == Grade::Evaluating && rep_bad.multitouch.misfires == 12 && rep_bad.multitouch.misfire_ppt == 1_000,
        "",
    );

    // 11. 报告三节齐备 + 延迟 P95 实测（本项判据本体）。
    set.add(
        "report three sections complete",
        rep_ok.format.ok_frames == 480
            && rep_ok.multitouch.judged == 120
            && rep_ok.latency.samples == 480
            && rep_ok.latency.p95_us <= 10_000,
        "",
    );

    // 12. 样本不足不下结论。
    let mut probe_few = TouchpadProbe::new();
    let _ = probe_few.feed(&frame2(8, 8, 40, 8), 0, false);
    probe_few.record_delay(0, 5_000);
    set.add("grade requires 100+ delay samples", probe_few.finish(false).grade == Grade::Evaluating, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_integer_sqrt() {
        let s = TouchSample { ts_us: 0, contacts: 2, x: 0, y: 0, pressure: 0, x2: 300, y2: 400 };
        assert_eq!(s.spread(), 500);
        let one = TouchSample { ts_us: 0, contacts: 1, x: 7, y: 7, pressure: 0, x2: 7, y2: 7 };
        assert_eq!(one.spread(), 0);
        // 非整平方根向下取。
        let d = TouchSample { ts_us: 0, contacts: 2, x: 0, y: 0, pressure: 0, x2: 1, y2: 0 };
        assert_eq!(d.spread(), 1);
    }

    #[test]
    fn malformed_fuzz_never_panics() {
        let mut p = ReportParser::new();
        let mut x: u32 = 0xDEADBEEF;
        let mut bad_len = 0u32;
        let mut bad_ct = 0u32;
        for i in 0..5000u32 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let n = (x % 16) as usize;
            let mut raw = [0u8; 16];
            for b in raw.iter_mut() {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                *b = x as u8;
            }
            match p.parse(&raw[..n], i as u64) {
                ParseOutcome::BadLength => bad_len += 1,
                ParseOutcome::BadContacts => bad_ct += 1,
                ParseOutcome::Sample(_) => {}
            }
        }
        // 随机流不保证 ID/触点字段命中率——确定性补两类拒绝路径。
        if p.parse(&[0x02; 12], 9_998) == ParseOutcome::BadLength {
            bad_len += 1;
        }
        if p.parse(&[0x01, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 9_999) == ParseOutcome::BadContacts {
            bad_ct += 1;
        }
        assert!(bad_len > 0 && bad_ct > 0, "两类拒绝路径都应被击中");
        assert_eq!(p.ok_frames + p.bad_length + p.bad_contacts, 5002);
    }

    #[test]
    fn pressure_coverage_tracks() {
        let mut p = ReportParser::new();
        let _ = p.parse(&frame_n(2, 0, 0, 10, 0, 500), 0);
        let _ = p.parse(&frame_n(2, 0, 0, 10, 0, 0), 1);
        let _ = p.parse(&frame_n(2, 0, 0, 10, 0, 0), 2);
        assert_eq!(p.pressure_coverage_ppt(), 333);
    }

    #[test]
    fn three_finger_swipe_judged() {
        let mut judge = GestureJudge::new();
        for i in 0..4u32 {
            let y = 300 - (i as i32) * 80;
            let s = TouchSample { ts_us: i as u64 * 20_000, contacts: 3, x: 100, y, pressure: 0, x2: 120, y2: y - 80 };
            let _ = judge.feed(s, false);
        }
        assert_eq!(judge.close_now(false), Gesture::ThreeFingerSwipe);
    }

    #[test]
    fn single_finger_excluded_from_stats() {
        let mut judge = GestureJudge::new();
        for i in 0..4u32 {
            let s = TouchSample { ts_us: i as u64 * 20_000, contacts: 1, x: 10 + i as i32, y: 10, pressure: 0, x2: 10, y2: 10 };
            let _ = judge.feed(s, false);
        }
        assert_eq!(judge.close_now(false), Gesture::None);
        assert_eq!(judge.judged, 0, "单指光标流量不进手势统计");
        assert_eq!(judge.misfire_ppt(), 0);
    }

    #[test]
    fn ambiguous_multifinger_counts_misfire() {
        let mut judge = GestureJudge::new();
        for i in 0..4u32 {
            let s = TouchSample { ts_us: i as u64 * 20_000, contacts: 2, x: 100, y: 800, pressure: 0, x2: 140, y2: 800 };
            let _ = judge.feed(s, false);
        }
        assert_eq!(judge.close_now(false), Gesture::None);
        assert_eq!(judge.judged, 1);
        assert_eq!(judge.misfires, 1);
        assert_eq!(judge.misfire_ppt(), 10_000);
    }

    #[test]
    fn window_silence_closes_and_opens_new() {
        let mut judge = GestureJudge::new();
        let s1 = TouchSample { ts_us: 0, contacts: 2, x: 100, y: 800, pressure: 0, x2: 140, y2: 800 };
        assert!(judge.feed(s1, false).is_none());
        let s2 = TouchSample {
            ts_us: GESTURE_WINDOW_US + 1,
            contacts: 2,
            x: 100,
            y: 860,
            pressure: 0,
            x2: 140,
            y2: 860,
        };
        let closed = judge.feed(s2, false);
        assert!(closed.is_some());
        // 旧窗只有 1 个多指样本 → 未分类 → None + 误触。
        assert_eq!(closed.unwrap().0, Gesture::None);
        assert_eq!(judge.judged, 1);
    }

    #[test]
    fn ime_yield_not_misfire() {
        let mut judge = GestureJudge::new();
        for i in 0..4u32 {
            let y = 800 + (i as i32) * 60;
            let s = TouchSample { ts_us: i as u64 * 20_000, contacts: 2, x: 100, y, pressure: 0, x2: 140, y2: y };
            let _ = judge.feed(s, true);
        }
        assert_eq!(judge.close_now(true), Gesture::None);
        assert_eq!(judge.yielded, 1);
        assert_eq!(judge.misfires, 0);
    }

    #[test]
    fn inertia_monotonic_decay() {
        let inertia = InertiaModel::new();
        let (pos, frames) = inertia.rollout(3000);
        assert!(pos > 0);
        assert!(frames >= 10 && frames <= 13, "200ms / 17ms ≈ 12 帧，实测 {frames}");
        let (zero, _) = inertia.rollout(0);
        assert_eq!(zero, 0);
    }

    #[test]
    fn commitable_end_to_end() {
        let mut probe = TouchpadProbe::new();
        for w in 0..120u32 {
            for k in 0..4u32 {
                let y = 800 + ((w * 4 + k) as i32) * 60;
                let f = frame2(100, y, 140, y);
                let ts = (w as u64 * 100_000) + (k as u64 * 20_000);
                let _ = probe.feed(&f, ts, false);
                probe.record_delay(ts, ts + 10_000);
            }
        }
        let rep = probe.finish(false);
        assert_eq!(rep.format.ok_frames, 480);
        assert_eq!(rep.format.bad_length, 0);
        assert_eq!(rep.multitouch.scrolls, 120);
        assert_eq!(rep.multitouch.misfire_ppt, 0);
        assert!(rep.latency.p95_us <= 10_000);
        assert_eq!(rep.grade, Grade::Commitable, "延迟 10ms<40ms 且误触 0<1% 应可承诺");
    }

    #[test]
    fn inertia_rollout_accessory() {
        let probe = TouchpadProbe::new();
        let (pos, frames) = probe.inertia_rollout(1500);
        assert!(pos > 0 && frames >= 10);
    }
}
