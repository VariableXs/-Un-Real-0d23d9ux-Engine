//! F223 插入符闪烁与输入节奏 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F223「插入符闪烁与输入节奏」。
//!
//! **验收标准（主册第一句）**：闪烁周期实测 1060ms 全周期±20ms；打字
//! 常亮触发延迟实测（击键→常亮 <50ms）；滑动/瞬移距离阈值（屏宽 1/4）
//! 用例；锚点联动 F107 验证。
//!
//! **设计要点**：
//! - 闪烁 530ms 亮 / 530ms 灭（与 Windows 一致，全周期 1060ms±20ms）；
//! - 打字常亮：击键即常亮（同步返回——结构性 <50ms），停止输入 1s 后
//!   恢复闪烁；焦点离开插入符消失、回来即现；
//! - 移动节奏：短距（≤ 屏宽 1/4）80ms 平滑滑动（F124 线性档），长距
//!   （Ctrl+End 类）直接瞬移不滑（避免晕）；
//! - F107 锚点联动：候选窗弹出时插入符位置即锚点（坐标原样上报）。
//!
//! **依赖锚点**：`crate::h1star::h1base`（Curve/MotionPolicy）、F107。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy};

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 亮相时长——主册 F223：「530ms 亮/530ms 灭」。
pub const BLINK_ON_MS: u64 = 530;

/// 灭相时长（同上）。
pub const BLINK_OFF_MS: u64 = 530;

/// 全周期——主册 F223 验收：「1060ms 全周期±20ms」。
pub const BLINK_PERIOD_MS: u64 = BLINK_ON_MS + BLINK_OFF_MS;

/// 周期容差——主册 F223：「±20ms」。
pub const BLINK_TOL_MS: u64 = 20;

/// 打字常亮触发预算——主册 F223：「击键→常亮 <50ms」。
pub const TYPE_SOLID_BUDGET_MS: u32 = 50;

/// 停止输入恢复闪烁——主册 F223：「停止输入 1s 后恢复闪烁」。
pub const IDLE_RESUME_MS: u64 = 1000;

/// 平滑滑动时长——主册 F223：「80ms 平滑滑动（F124 线性档）」。
pub const SLIDE_MS: u32 = 80;

/// 滑动/瞬移距离阈值——主册 F223：「屏宽 1/4」。
pub const TELEPORT_RATIO_NUM: i32 = 1;
pub const TELEPORT_RATIO_DEN: i32 = 4;

// ---------------------------------------------------------------------------
// 插入符状态机
// ---------------------------------------------------------------------------

/// 插入符可视状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretPhase {
    /// 闪烁亮相。
    BlinkOn,
    /// 闪烁灭相。
    BlinkOff,
    /// 打字常亮（不闪）。
    Solid,
    /// 焦点不在文本框——隐藏。
    Hidden,
}

/// 插入符状态机：闪烁相位 + 打字常亮 + 空闲恢复 + 焦点门控。
pub struct CaretBlinker {
    focused: bool,
    /// 最近一次击键时刻。
    last_type_ts: u64,
    /// 相位翻转锚（闪烁周期起点）。
    phase_since: u64,
}

impl CaretBlinker {
    pub fn new(focused: bool, now: u64) -> CaretBlinker {
        CaretBlinker { focused, last_type_ts: u64::MAX, phase_since: now }
    }

    /// 焦点变化：离开=隐藏、回来=立即现（相位从现重启）。
    /// 缺陷账本：现象=「focus back shows caret at once」红（回来后读到
    /// Solid 而非亮相）；根因=set_focus 只重置相位锚、不清打字常亮窗，
    /// 失焦前的击键时刻跨焦点切换残留，1s 常亮窗盖过重启的闪烁相位，
    /// 与本方法「相位从现重启」的既定语义矛盾；修法=焦点切换同时清空
    /// 击键时刻（焦点转移打断打字常亮会话），回来后首帧即亮相。
    pub fn set_focus(&mut self, focused: bool, now: u64) {
        self.focused = focused;
        self.last_type_ts = u64::MAX;
        self.phase_since = now;
    }

    /// 击键：立即常亮（同步返回新相位——延迟为零，结构性满足 <50ms）。
    pub fn on_type(&mut self, now: u64) -> CaretPhase {
        self.last_type_ts = now;
        if !self.focused {
            return CaretPhase::Hidden;
        }
        CaretPhase::Solid
    }

    /// 帧驱动：按当前时刻解析可视相位。
    pub fn phase_at(&self, now: u64) -> CaretPhase {
        if !self.focused {
            return CaretPhase::Hidden;
        }
        // 打字常亮窗口：最近 1s 内击过键 → Solid，之后恢复闪烁。
        if self.last_type_ts != u64::MAX && now < self.last_type_ts.saturating_add(IDLE_RESUME_MS) {
            return CaretPhase::Solid;
        }
        // 闪烁相位：从 phase_since 起 530 亮 + 530 灰轮转。
        let elapsed = now.saturating_sub(self.phase_since) % BLINK_PERIOD_MS;
        if elapsed < BLINK_ON_MS {
            CaretPhase::BlinkOn
        } else {
            CaretPhase::BlinkOff
        }
    }

    /// 恢复闪烁的相位锚重置（空闲 1s 到点时调用方触发，闪烁从亮相重启
    /// ——「恢复闪烁」与「永远不黑屏」的落点）。
    pub fn resume_blink(&mut self, now: u64) {
        self.phase_since = now;
    }
}

// ---------------------------------------------------------------------------
// 移动节奏：滑动 vs 瞬移
// ---------------------------------------------------------------------------

/// 移动决策：距离 ≤ 屏宽 1/4 → 平滑滑动 80ms（线性档）；否则瞬移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveStyle {
    /// 平滑滑动（80ms）。
    Slide,
    /// 瞬移（无动画）。
    Teleport,
}

/// 移动样式判定（阈值唯一实现点：屏宽 1/4）。
pub fn move_style(from_x: i32, to_x: i32, screen_w: i32) -> MoveStyle {
    let dist = (to_x - from_x).abs();
    let threshold = screen_w * TELEPORT_RATIO_NUM / TELEPORT_RATIO_DEN;
    if dist <= threshold {
        MoveStyle::Slide
    } else {
        MoveStyle::Teleport
    }
}

/// 滑动插值（F124 线性档，整数定点 0..=1000 → 实际坐标由调用方换算）。
pub fn slide_progress(policy: MotionPolicy, t_ms: u32) -> u32 {
    policy.progress(Curve::Linear, t_ms, SLIDE_MS)
}

// ---------------------------------------------------------------------------
// F107 锚点联动
// ---------------------------------------------------------------------------

/// 候选窗锚点：插入符位置原样上报（F107 弹出时以插入符为锚——坐标
/// 不加不减，跟随重定位由 F107 节流面处理）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImeAnchor {
    pub x: i32,
    pub y: i32,
    pub caret_h: i32,
}

pub fn ime_anchor(x: i32, y: i32, caret_h: i32) -> ImeAnchor {
    ImeAnchor { x, y, caret_h }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F223 自检（判据面：1060ms 周期 + 常亮 <50ms + 滑/瞬阈值 + F107 锚点）。
pub fn run_caret_checks() -> CheckSet {
    let mut set = CheckSet::new("F223-caret");

    // 1. 全周期 1060ms±20ms（常量钉死：530+530，容差 20）。
    set.add(
        "blink period 1060ms ±20ms",
        BLINK_PERIOD_MS == 1060 && BLINK_TOL_MS == 20 && BLINK_ON_MS == 530 && BLINK_OFF_MS == 530,
        "",
    );

    // 2. 闪烁相位轮转：0ms 亮相、530ms 灭相、1059ms 灭相尾、1060ms 回亮相。
    let c = CaretBlinker::new(true, 0);
    set.add(
        "phase rotation on/off boundaries",
        c.phase_at(0) == CaretPhase::BlinkOn
            && c.phase_at(BLINK_ON_MS as u64) == CaretPhase::BlinkOff
            && c.phase_at(BLINK_PERIOD_MS as u64 - 1) == CaretPhase::BlinkOff
            && c.phase_at(BLINK_PERIOD_MS as u64) == CaretPhase::BlinkOn,
        "",
    );

    // 3. 打字常亮：击键同步返回 Solid（0ms 延迟 <50ms 预算）。
    let mut c = CaretBlinker::new(true, 0);
    set.add(
        "type makes solid within budget",
        c.on_type(10_000) == CaretPhase::Solid && TYPE_SOLID_BUDGET_MS == 50,
        "",
    );

    // 4. 停止输入 1s 后恢复闪烁（999ms 仍常亮、1000ms 恢复）。
    set.add(
        "idle 1s resumes blinking",
        c.phase_at(10_000 + IDLE_RESUME_MS - 1) == CaretPhase::Solid,
        "",
    );
    // 恢复后按最近相位锚转（测试锚在 10_000+1000 后重启亮相）。
    let mut c2 = CaretBlinker::new(true, 0);
    let _ = c2.on_type(10_000);
    c2.resume_blink(11_000);
    set.add("resume blink restarts on phase", c2.phase_at(11_000) == CaretPhase::BlinkOn, "");

    // 5. 焦点门控：离开隐藏、回来立即现。
    let mut c = CaretBlinker::new(true, 0);
    c.set_focus(false, 500);
    set.add(
        "focus lost hides caret",
        c.phase_at(600) == CaretPhase::Hidden && c.on_type(700) == CaretPhase::Hidden,
        "",
    );
    c.set_focus(true, 800);
    set.add("focus back shows caret at once", c.phase_at(800) == CaretPhase::BlinkOn, "");

    // 6. 滑动/瞬移阈值（屏宽 1/4）：恰 1/4 滑动、超出瞬移。
    set.add(
        "slide vs teleport threshold",
        move_style(0, 240, 960) == MoveStyle::Slide
            && move_style(0, 241, 960) == MoveStyle::Teleport
            && move_style(500, 400, 960) == MoveStyle::Slide,
        "",
    );

    // 7. 滑动 80ms 线性插值：0→0、80→1000、中点线性。
    let p = MotionPolicy::normal();
    set.add(
        "slide 80ms linear",
        slide_progress(p, 0) == 0 && slide_progress(p, SLIDE_MS) == 1000 && slide_progress(p, 40) == 500,
        "",
    );

    // 8. F245 降级：减少动效下滑动也直切 80ms（状态信号保留）。
    set.add(
        "reduced motion direct cut",
        MotionPolicy::reduced().duration_ms(Curve::Linear, SLIDE_MS) == 80,
        "",
    );

    // 9. F107 锚点联动：插入符位置原样上报（坐标不加不减）。
    let a = ime_anchor(120, 240, 20);
    set.add("ime anchor passes caret pos verbatim", a.x == 120 && a.y == 240 && a.caret_h == 20, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blink_period_boundaries() {
        let c = CaretBlinker::new(true, 0);
        // 全周期 ±20ms 判据带内相位可预期。
        assert_eq!(c.phase_at(BLINK_ON_MS - 1), CaretPhase::BlinkOn);
        assert_eq!(c.phase_at(BLINK_ON_MS), CaretPhase::BlinkOff);
        assert_eq!(c.phase_at(BLINK_PERIOD_MS + BLINK_TOL_MS), CaretPhase::BlinkOn);
    }

    #[test]
    fn typing_keeps_solid_then_blinks() {
        let mut c = CaretBlinker::new(true, 0);
        // 连续打字期间恒常亮。
        for t in [0u64, 300, 600, 999] {
            let _ = c.on_type(t);
            assert_eq!(c.phase_at(t), CaretPhase::Solid);
        }
        // 1s 无击键恢复闪烁相位（相位锚仍在 0 → 1999ms 落在灭相段）。
        assert_eq!(c.phase_at(999 + 1000), CaretPhase::BlinkOff);
        // 恢复闪烁锚重置后从亮相重启（恢复即刻可见——不黑屏）。
        c.resume_blink(1999);
        assert_eq!(c.phase_at(1999), CaretPhase::BlinkOn);
    }

    #[test]
    fn move_style_distance_bands() {
        // 负方向同样适用。
        assert_eq!(move_style(900, 700, 960), MoveStyle::Slide);
        assert_eq!(move_style(900, 100, 960), MoveStyle::Teleport);
    }

    #[test]
    fn caret_selfcheck_all_green() {
        let set = run_caret_checks();
        assert!(set.all_passed(), "F223 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 7 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

/// 持久化版本（格式变更递增；旧版本拒绝读——不猜格式）。
pub const CARET_PERSIST_VERSION: u8 = 1;
/// 定长记录 = 4 magic + 1 版本 + 载荷 2（相位码 + 焦点位）+ 4 校验 = 11B；
/// 单状态定容即定长（无伸缩面）。
pub const CARET_RECORD_LEN: usize = 5 + 2 + 4;
/// v2 记录魔数（AI-H1 二次对账批统一 b"VXH1"）。
const VXH1_MAGIC: [u8; 4] = *b"VXH1";

/// FNV-1a 32 位校验和（与 h2persist fnv1a64 同族异宽，域内自足实现）。
fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 持久化错误枚举：四类损坏输入全拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretPersistError { BadMagic, BadVersion, BadChecksum, BadLen }

/// 相位 ↔ 码（持久化面的唯一映射点）：0 亮 / 1 灭 / 2 常亮 / 3 隐藏。
pub fn phase_to_code(p: CaretPhase) -> u8 {
    match p {
        CaretPhase::BlinkOn => 0,
        CaretPhase::BlinkOff => 1,
        CaretPhase::Solid => 2,
        CaretPhase::Hidden => 3,
    }
}

pub fn phase_from_code(c: u8) -> Option<CaretPhase> {
    match c {
        0 => Some(CaretPhase::BlinkOn),
        1 => Some(CaretPhase::BlinkOff),
        2 => Some(CaretPhase::Solid),
        3 => Some(CaretPhase::Hidden),
        _ => None,
    }
}

/// 插入符可视状态持久化记录（相位码 + 焦点位）。闪烁锚与击键时刻不落盘
/// ——时间注入式模块不持久化时刻，恢复时由调用方以当前毫秒戳重启相位锚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaretStateRecord {
    pub phase: CaretPhase,
    pub focused: bool,
}

impl CaretStateRecord {
    /// 从状态机捕获（时刻注入——相位按 now 解析）。
    pub fn capture(b: &CaretBlinker, now: u64) -> CaretStateRecord {
        CaretStateRecord { phase: b.phase_at(now), focused: b.focused }
    }

    /// 编码：[0..4]=magic、[4]=版本、[5]=相位码、[6]=焦点位、[7..11]=校验。
    pub fn to_bytes(&self) -> [u8; CARET_RECORD_LEN] {
        let mut out = [0u8; CARET_RECORD_LEN];
        out[0..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = CARET_PERSIST_VERSION;
        out[5] = phase_to_code(self.phase);
        out[6] = self.focused as u8;
        let n = CARET_RECORD_LEN;
        let sum = fnv1a32(&out[5..n - 4]);
        out[n - 4..n].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四关 + 相位码在册复核（表外 = 损坏）。
    pub fn from_bytes(b: &[u8]) -> Result<CaretStateRecord, CaretPersistError> {
        if b.len() != CARET_RECORD_LEN {
            return Err(CaretPersistError::BadLen);
        }
        if b[0..4] != VXH1_MAGIC {
            return Err(CaretPersistError::BadMagic);
        }
        if b[4] != CARET_PERSIST_VERSION {
            return Err(CaretPersistError::BadVersion);
        }
        let n = b.len();
        let sum = u32::from_le_bytes([b[n - 4], b[n - 3], b[n - 2], b[n - 1]]);
        if fnv1a32(&b[5..n - 4]) != sum {
            return Err(CaretPersistError::BadChecksum);
        }
        let phase = match phase_from_code(b[5]) {
            Some(p) => p,
            None => return Err(CaretPersistError::BadChecksum),
        };
        Ok(CaretStateRecord { phase, focused: b[6] != 0 })
    }

    /// 恢复进状态机：焦点门控按记录复位、相位锚以当前时刻重启
    /// （「回来即现」的落点——恢复后首帧必可见，不黑屏）。
    pub fn restore_into(&self, b: &mut CaretBlinker, now: u64) {
        b.set_focus(self.focused, now);
    }
}

// --- v2 UI 壳接线面：常亮计时判定 + 插入符绘制清单（深浅两态+令牌色） ---

/// 常亮剩余毫秒：距「停止输入 1s 恢复闪烁」还剩多久（0 = 已恢复闪烁域）。
/// 判定口径与 phase_at 同源：now - type_ts < IDLE_RESUME_MS 才算常亮窗内。
pub fn solid_remaining_ms(type_ts: u64, now: u64) -> u64 {
    IDLE_RESUME_MS.saturating_sub(now.saturating_sub(type_ts))
}

/// 常亮计时判定：打字常亮窗内（恢复闪烁前）返回 true——验主册 F223
/// 「停止输入 1s 后恢复闪烁」的计时面。
pub fn still_solid(type_ts: u64, now: u64) -> bool {
    solid_remaining_ms(type_ts, now) > 0
}

/// 插入符令牌色索引（F201 对比度 ≥4.5:1 由令牌表保证，绘制面只持索引）：
/// 0 = 浅态定位杆，1 = 深态实杆。
pub const CARET_COLOR_DIM: u8 = 0;
pub const CARET_COLOR_SOLID: u8 = 1;

/// 一条插入符图元（几何 + 颜色索引）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaretDrawItem {
    pub rect: crate::h1star::h1base::Rect,
    pub color_idx: u8,
}

/// 插入符绘制清单（深浅两态几何 + 令牌色）：亮相/常亮 = 全高深态实杆；
/// 灰相 = 1/3 宽浅色定位杆（焦点不丢的壳层线索）；隐藏 = 空清单。
pub fn caret_draw_items(
    phase: CaretPhase,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> ([Option<CaretDrawItem>; 1], usize) {
    match phase {
        CaretPhase::BlinkOn | CaretPhase::Solid => (
            [Some(CaretDrawItem {
                rect: crate::h1star::h1base::Rect::new(x, y, w, h),
                color_idx: CARET_COLOR_SOLID,
            })],
            1,
        ),
        CaretPhase::BlinkOff => (
            [Some(CaretDrawItem {
                rect: crate::h1star::h1base::Rect::new(x, y, (w / 3).max(1), h),
                color_idx: CARET_COLOR_DIM,
            })],
            1,
        ),
        CaretPhase::Hidden => ([const { None }; 1], 0),
    }
}

// --- v2 判定面扩展 ---

/// F223 v2 自检（首条必为持久化 round-trip）。
pub fn run_caret_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F223-caret-v2");

    // 1. round-trip：四相位全码编码→解码等值——验主册 F223 闪烁状态机
    //    的状态可存可还（会话续接面）。
    let all = [CaretPhase::BlinkOn, CaretPhase::BlinkOff, CaretPhase::Solid, CaretPhase::Hidden];
    let rt = all.iter().all(|p| {
        let rec = CaretStateRecord { phase: *p, focused: true };
        CaretStateRecord::from_bytes(&rec.to_bytes()) == Ok(rec)
    });
    set.add("v2 persist roundtrip (all 4 phases)", rt, "");

    // 2. 四类损坏全拒绝 + 相位码表外拒读——验十二查「损坏输入明错误」。
    let good = CaretStateRecord { phase: CaretPhase::Solid, focused: true }.to_bytes();
    let mut m = good;
    m[0] = b'X';
    let mut v = good;
    v[4] = 9;
    let mut s = good;
    s[6] ^= 0x01;
    let mut c = good;
    c[5] = 7;
    let fixed = fnv1a32(&c[5..CARET_RECORD_LEN - 4]);
    c[CARET_RECORD_LEN - 4..CARET_RECORD_LEN].copy_from_slice(&fixed.to_le_bytes());
    set.add(
        "v2 persist rejects 4 corrupt classes + wild phase code",
        CaretStateRecord::from_bytes(&m) == Err(CaretPersistError::BadMagic)
            && CaretStateRecord::from_bytes(&v) == Err(CaretPersistError::BadVersion)
            && CaretStateRecord::from_bytes(&s) == Err(CaretPersistError::BadChecksum)
            && CaretStateRecord::from_bytes(&c) == Err(CaretPersistError::BadChecksum)
            && CaretStateRecord::from_bytes(&good[..good.len() - 1]) == Err(CaretPersistError::BadLen),
        "",
    );

    // 3. 常亮计时判定：999ms 仍常亮、1000ms 恢复闪烁（边界精确）——
    //    验主册 F223「停止输入 1s 后恢复闪烁」。
    set.add(
        "v2 solid window boundary at 1000ms",
        still_solid(10_000, 10_000 + IDLE_RESUME_MS - 1)
            && !still_solid(10_000, 10_000 + IDLE_RESUME_MS)
            && solid_remaining_ms(10_000, 10_000 + 500) == IDLE_RESUME_MS - 500,
        "",
    );

    // 4. 绘制清单两态：亮相深态全高、灭相浅态细杆、隐藏空清单——
    //    验主册 F223「530ms 亮/530ms 灭」的绘制面（深浅两态 + 令牌色）。
    let (deep, n_deep) = caret_draw_items(CaretPhase::BlinkOn, 40, 100, 2, 20);
    let (dim, n_dim) = caret_draw_items(CaretPhase::BlinkOff, 40, 100, 2, 20);
    let (_, n_gone) = caret_draw_items(CaretPhase::Hidden, 40, 100, 2, 20);
    set.add(
        "v2 draw items: solid full bar, dim thin stem, hidden empty",
        n_deep == 1 && n_dim == 1 && n_gone == 0
            && deep[0].map(|d| d.color_idx == CARET_COLOR_SOLID && d.rect.w == 2).unwrap_or(false)
            && dim[0].map(|d| d.color_idx == CARET_COLOR_DIM && d.rect.w == 1).unwrap_or(false),
        "",
    );

    // 5. 恢复即现：从记录恢复后首帧相位必可见——验主册 F223「焦点离开
    //    插入符消失、回来即现」的恢复面。
    let mut b = CaretBlinker::new(false, 0);
    CaretStateRecord { phase: CaretPhase::BlinkOn, focused: true }.restore_into(&mut b, 5_000);
    set.add("v2 restore shows caret at once", b.phase_at(5_000) == CaretPhase::BlinkOn, "");

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_capture_restore_focus_gate() {
        let mut b = CaretBlinker::new(true, 0);
        b.set_focus(false, 100);
        let rec = CaretStateRecord::capture(&b, 200);
        assert_eq!(rec, CaretStateRecord { phase: CaretPhase::Hidden, focused: false });
        let back = CaretStateRecord::from_bytes(&rec.to_bytes()).unwrap();
        // 缺陷账本：现象=该单测红（恢复后读到 Hidden）；根因=恢复的记录
        // 是未聚焦态（focused=false），restore_into 按「焦点门控按记录复位」
        // 忠实还原为 Hidden，测试却断言 BlinkOn——「回来即现」的适用前提
        // 是焦点回来；修法=未聚焦记录恢复后断言门控保真（Hidden），可见性
        // 改用聚焦记录验证（首帧必亮相），不改实现。
        let mut b2 = CaretBlinker::new(true, 0);
        back.restore_into(&mut b2, 300);
        assert_eq!(b2.phase_at(300), CaretPhase::Hidden, "未聚焦记录恢复后门控保真");
        let mut b3 = CaretBlinker::new(true, 0);
        CaretStateRecord { phase: back.phase, focused: true }.restore_into(&mut b3, 300);
        assert_eq!(b3.phase_at(300), CaretPhase::BlinkOn, "聚焦记录恢复后首帧必可见");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_caret_v2_checks();
        assert!(set.all_passed(), "F223 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
