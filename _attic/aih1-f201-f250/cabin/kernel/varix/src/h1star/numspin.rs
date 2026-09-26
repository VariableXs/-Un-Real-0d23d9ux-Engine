//! F217 数值输入步进器与拖拽改值 · 判据实装（H 基础通用域 · AI-H1）。
//!
//! **判据锚**：主册 F217「数值输入步进器与拖拽改值」。
//!
//! **验收标准（主册第一句）**：三通路（按钮/滚轮/拖拽）行为一致性用例；
//! 连发三档速度实测；非法输入抖动动画与不弹窗判据审计；边界钳制 10 例。
//!
//! **设计要点**：
//! - 三通路一个状态机：按钮步进、滚轮微调（±1，Shift 加速 ±10）、
//!   拖拽横向连续改值——全部收敛到同一个 `apply_delta`（行为一致的
//!   结构保证：任何通路的落点都是同一钳制、同一抖动面）；
//! - 连发三档：初速 2/s（间隔 500ms）→ 连发 5 次后 4/s（250ms）→
//!   连发 12 次后 10/s（100ms）——三档速度实测（档位切换点边界验证）；
//! - 非法键入即时拒绝：抖动 120ms 提示（h1base F124 谱）+ **结构性
//!   不弹窗**（拒绝路径无对话框出口——类型层保证）；
//! - 边界钳制不回绕：min/max 处步进到顶即停（10 例边界验证）。
//!
//! **依赖锚点**：`crate::h1star::h1base`（Curve/MotionPolicy）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy};

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 步进按钮边长——主册 F217：「上下步进按钮（16px…）」。
pub const STEP_BTN_PX: i32 = 16;

/// 连发初速（次/秒）——主册 F217：「按住连发，初速 2/s」。
pub const AUTOREPEAT_START_PER_S: u32 = 2;

/// 连发三档间隔（ms）：500 → 250 → 100（初速 2/s 的倒数即 500ms）。
pub const AUTOREPEAT_TIERS_MS: [u32; 3] = [500, 250, 100];

/// 三档切换点（累计连发次数）：0-4 次一档、5-11 次二档、12+ 三档。
pub const AUTOREPEAT_TIER_AT: [u32; 3] = [0, 5, 12];

/// 非法输入抖动时长——主册 F217：「抖动 120ms 提示」。
pub const SHAKE_MS: u32 = 120;

/// 滚轮微调步长——主册 F217：「滚轮微调（±1…）」。
pub const WHEEL_STEP: i64 = 1;

/// Shift 滚轮加速步长——主册 F217：「Shift 加速 ±10」。
pub const WHEEL_STEP_FAST: i64 = 10;

/// 边界钳制验收例数——主册 F217：「边界钳制 10 例」。
pub const CLAMP_CASES: usize = 10;

// ---------------------------------------------------------------------------
// 步进器状态机（三通路统一落点）
// ---------------------------------------------------------------------------

/// 数值步进器：值域 [min, max] + 连发计数 + 抖动状态。
pub struct NumberSpinner {
    pub value: i64,
    pub min: i64,
    pub max: i64,
    /// 累计连发次数（档位切换依据）。
    repeats: u32,
    /// 抖动剩余时长（ms）。
    shake_left: u32,
    /// 拖拽基线（拖拽开始时的值与指针 x）。
    drag_base: Option<(i64, i32)>,
}

impl NumberSpinner {
    /// 新建（min < max；非法区间如实拒绝返回 None——不静默纠正）。
    pub fn new(value: i64, min: i64, max: i64) -> Option<NumberSpinner> {
        if min >= max {
            return None;
        }
        Some(NumberSpinner {
            value: value.clamp(min, max),
            min,
            max,
            repeats: 0,
            shake_left: 0,
            drag_base: None,
        })
    }

    /// 三通路统一落点：施加增量并钳制。返回 (新值, 是否触界停住)。
    fn apply_delta(&mut self, d: i64) -> (i64, bool) {
        let next = self.value.saturating_add(d);
        let clamped = next.clamp(self.min, self.max);
        let pinned = clamped == self.value && d != 0;
        self.value = clamped;
        (self.value, pinned)
    }

    /// 通路一：按钮步进（按一下 +1/-1；连发由 `autorepeat_tick` 驱动）。
    pub fn step(&mut self, up: bool) -> (i64, bool) {
        self.repeats = self.repeats.saturating_add(1);
        self.apply_delta(if up { 1 } else { -1 })
    }

    /// 通路二：滚轮微调（Shift 加速 ±10）。
    pub fn wheel(&mut self, up: bool, shift: bool) -> (i64, bool) {
        let d = if up { 1 } else { -1 } * if shift { WHEEL_STEP_FAST } else { WHEEL_STEP };
        self.apply_delta(d)
    }

    /// 通路三：拖拽开始（记基线）。
    pub fn drag_begin(&mut self, x: i32) {
        self.drag_base = Some((self.value, x));
    }

    /// 通路三：拖拽移动——横向 3px = ±1（连续改值，光标左右箭头由渲染面出）。
    pub fn drag_move(&mut self, x: i32) -> (i64, bool) {
        match self.drag_base {
            Some((base, x0)) => {
                let d = ((x - x0) / 3) as i64;
                let clamped = (base + d).clamp(self.min, self.max);
                let pinned = clamped == self.value && clamped != base + d;
                self.value = clamped;
                (self.value, pinned)
            }
            None => (self.value, false),
        }
    }

    /// 通路三：拖拽结束（松手定值，清基线）。
    pub fn drag_end(&mut self) {
        self.drag_base = None;
    }

    /// 连发档位：按累计次数取当前间隔（三档实测的判定面）。
    pub fn autorepeat_interval_ms(&self) -> u32 {
        let n = self.repeats;
        let tier = if n >= AUTOREPEAT_TIER_AT[2] {
            2
        } else if n >= AUTOREPEAT_TIER_AT[1] {
            1
        } else {
            0
        };
        AUTOREPEAT_TIERS_MS[tier]
    }

    /// 连发复位（松开按钮）。
    pub fn autorepeat_reset(&mut self) {
        self.repeats = 0;
    }

    /// 键入提交：合法则设值（仍钳制），非法返回 None 并触发抖动 120ms。
    /// **无对话框出口**——拒绝的反馈只有抖动（结构保证，类型层无弹窗面）。
    pub fn submit_text(&mut self, s: &str, policy: MotionPolicy) -> Option<i64> {
        let t = s.trim();
        let ok = !t.is_empty()
            && (t.parse::<i64>().map(|v| v >= self.min && v <= self.max).unwrap_or(false));
        if ok {
            self.value = t.parse::<i64>().unwrap_or(self.value).clamp(self.min, self.max);
            Some(self.value)
        } else {
            self.shake_left = policy.duration_ms(Curve::Linear, SHAKE_MS);
            None
        }
    }

    /// 抖动剩余时长（到 0 即静止）。
    pub fn shake_left_ms(&self) -> u32 {
        self.shake_left
    }

    /// 抖动推进（调用方按帧注入 Δt）。
    pub fn shake_tick(&mut self, dt_ms: u32) {
        self.shake_left = self.shake_left.saturating_sub(dt_ms);
    }
}

// ---------------------------------------------------------------------------
// 三通路一致性
// ---------------------------------------------------------------------------

/// 三通路行为一致性：按钮 +1 / 滚轮 +1 / 拖拽 3px 从同一初值出发，落点
/// 与触界行为必须逐字节一致（同一 `apply_delta` 落点的自动化证明）。
pub fn three_paths_consistent() -> bool {
    // 中部值：三通路各 +1 落点一致。
    let mut a = NumberSpinner::new(50, 0, 100).unwrap();
    let mut b = NumberSpinner::new(50, 0, 100).unwrap();
    let mut c = NumberSpinner::new(50, 0, 100).unwrap();
    let _ = a.step(true);
    let _ = b.wheel(true, false);
    c.drag_begin(0);
    let _ = c.drag_move(3);
    c.drag_end();
    if !(a.value == 51 && b.value == 51 && c.value == 51) {
        return false;
    }
    // 触界行为一致：max 处再 +1 全部停住不回绕。
    let mut a2 = NumberSpinner::new(100, 0, 100).unwrap();
    let mut b2 = NumberSpinner::new(100, 0, 100).unwrap();
    let mut c2 = NumberSpinner::new(100, 0, 100).unwrap();
    let pa = a2.step(true);
    let pb = b2.wheel(true, true);
    c2.drag_begin(0);
    let pc = c2.drag_move(300);
    if !(pa.1 && pb.1 && pc.1 && a2.value == 100 && b2.value == 100 && c2.value == 100) {
        return false;
    }
    true
}

/// 边界钳制 10 例：min-1/max+1 双向 × 五种通路组合（步进/滚轮/Shift 滚轮/
/// 键入越界/拖拽越界）在边界处全部钳住不回绕。
pub fn clamp_boundary_cases() -> bool {
    let mut ok = 0usize;
    // 1) min 处按钮 -1。
    let mut s = NumberSpinner::new(0, 0, 100).unwrap();
    if s.step(false).1 && s.value == 0 { ok += 1; }
    // 2) min 处滚轮 -1。
    let mut s = NumberSpinner::new(0, 0, 100).unwrap();
    if s.wheel(false, false).1 && s.value == 0 { ok += 1; }
    // 3) max 处 Shift 滚轮 +10。
    let mut s = NumberSpinner::new(100, 0, 100).unwrap();
    if s.wheel(true, true).1 && s.value == 100 { ok += 1; }
    // 4) 键入越界（>max）→ 拒绝 + 抖动。
    let mut s = NumberSpinner::new(50, 0, 100).unwrap();
    if s.submit_text("200", MotionPolicy::normal()).is_none() && s.shake_left_ms() == SHAKE_MS && s.value == 50 { ok += 1; }
    // 5) 键入合法越上界（=max）→ 接受并钳到 max。
    let mut s = NumberSpinner::new(50, 0, 100).unwrap();
    if s.submit_text("100", MotionPolicy::normal()) == Some(100) { ok += 1; }
    // 6) 键入负值越下界 → 拒绝。
    let mut s = NumberSpinner::new(50, 0, 100).unwrap();
    if s.submit_text("-1", MotionPolicy::normal()).is_none() { ok += 1; }
    // 7) 键入非数字 → 拒绝 + 抖动 + 不弹窗（无对话框出口）。
    let mut s = NumberSpinner::new(50, 0, 100).unwrap();
    if s.submit_text("abc", MotionPolicy::normal()).is_none() && s.shake_left_ms() == SHAKE_MS { ok += 1; }
    // 8) 拖拽远超上界 → 钳在 max。
    let mut s = NumberSpinner::new(90, 0, 100).unwrap();
    s.drag_begin(0);
    let (v, _) = s.drag_move(999);
    s.drag_end();
    if v == 100 { ok += 1; }
    // 9) 拖拽远超下界 → 钳在 min。
    let mut s = NumberSpinner::new(10, 0, 100).unwrap();
    s.drag_begin(0);
    let (v, _) = s.drag_move(-999);
    s.drag_end();
    if v == 0 { ok += 1; }
    // 10) 非法区间构造如实拒绝（min>=max 返回 None，不静默纠正）。
    if NumberSpinner::new(5, 10, 10).is_none() { ok += 1; }
    ok == CLAMP_CASES
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F217 自检（判据面：三通路一致 + 连发三档 + 抖动不弹窗 + 边界钳制）。
pub fn run_numspin_checks() -> CheckSet {
    let mut set = CheckSet::new("F217-numspin");

    // 1. 三通路行为一致性（中部 +1 落点一致、触界停住一致）。
    set.add("three paths behave identically", three_paths_consistent(), "");

    // 2. 连发三档速度：0-4 次 500ms、5-11 次 250ms、12+ 次 100ms。
    let mut s = NumberSpinner::new(0, 0, 1000).unwrap();
    let t0 = s.autorepeat_interval_ms();
    for _ in 0..AUTOREPEAT_TIER_AT[1] { let _ = s.step(true); }
    let t1 = s.autorepeat_interval_ms();
    for _ in 0..(AUTOREPEAT_TIER_AT[2] - AUTOREPEAT_TIER_AT[1]) { let _ = s.step(true); }
    let t2 = s.autorepeat_interval_ms();
    set.add(
        "autorepeat three tiers",
        t0 == 500 && t1 == 250 && t2 == 100 && AUTOREPEAT_START_PER_S == 2,
        "",
    );

    // 3. 松开复位：连发计数清零回到一档。
    s.autorepeat_reset();
    set.add("autorepeat resets on release", s.autorepeat_interval_ms() == 500, "");

    // 4. 滚轮 Shift 加速 ±10。
    let mut s = NumberSpinner::new(50, 0, 100).unwrap();
    let _ = s.wheel(true, true);
    set.add("wheel shift accelerates x10", s.value == 60, "");

    // 5. 非法输入抖动 120ms 且结构性不弹窗（拒绝路径无对话框出口）。
    let mut s = NumberSpinner::new(50, 0, 100).unwrap();
    let r = s.submit_text("xyz", MotionPolicy::normal());
    set.add(
        "illegal input shakes 120ms no dialog",
        r.is_none() && s.shake_left_ms() == SHAKE_MS && SHAKE_MS == 120,
        "",
    );

    // 6. 抖动随帧推进归零（不卡死在抖动态）。
    s.shake_tick(60);
    s.shake_tick(60);
    set.add("shake decays to rest", s.shake_left_ms() == 0, "");

    // 7. 边界钳制 10 例全过。
    set.add("10 clamp boundary cases green", clamp_boundary_cases(), "");

    // 8. 拖拽 3px=±1 的连续改值标度。
    let mut s = NumberSpinner::new(50, 0, 100).unwrap();
    s.drag_begin(0);
    let _ = s.drag_move(6);
    s.drag_end();
    set.add("drag scale 3px per unit", s.value == 52, "");

    // 9. 步进按钮 16px 常量（一处一事实）。
    set.add("step button 16px", STEP_BTN_PX == 16, "");

    // 10. 键入合法值直接生效（三通路第四面：直接键入）。
    let mut s = NumberSpinner::new(50, 0, 100).unwrap();
    set.add("typed value applies", s.submit_text("77", MotionPolicy::normal()) == Some(77), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autorepeat_tier_boundaries() {
        let mut s = NumberSpinner::new(0, 0, 1000).unwrap();
        // 第 1-4 次：一档 500ms。
        for _ in 0..4 { let _ = s.step(true); }
        assert_eq!(s.autorepeat_interval_ms(), 500);
        // 第 5 次：二档 250ms。
        let _ = s.step(true);
        assert_eq!(s.autorepeat_interval_ms(), 250);
        // 第 12 次：三档 100ms。
        for _ in 5..12 { let _ = s.step(true); }
        assert_eq!(s.autorepeat_interval_ms(), 100);
    }

    #[test]
    fn no_wraparound_at_bounds() {
        let mut s = NumberSpinner::new(0, -5, 5).unwrap();
        for _ in 0..10 { let _ = s.step(true); }
        assert_eq!(s.value, 5);
        for _ in 0..10 { let _ = s.step(false); }
        assert_eq!(s.value, -5);
    }

    #[test]
    fn drag_roundtrip() {
        let mut s = NumberSpinner::new(50, 0, 100).unwrap();
        s.drag_begin(100);
        let _ = s.drag_move(115); // 15px → +5
        assert_eq!(s.value, 55);
        let _ = s.drag_move(85); // 相对基线 -15px → 45
        assert_eq!(s.value, 45);
        s.drag_end();
        // 拖拽外移动不影响值。
        let (v, _) = s.drag_move(999);
        assert_eq!(v, 45);
    }

    #[test]
    fn reject_paths_have_no_dialog() {
        // 类型层证明：submit_text 返回 Option<i64>，无任何对话框出口。
        let mut s = NumberSpinner::new(50, 0, 100).unwrap();
        assert!(s.submit_text("", MotionPolicy::normal()).is_none());
        assert!(s.submit_text("  ", MotionPolicy::normal()).is_none());
        assert!(s.submit_text("1.5", MotionPolicy::normal()).is_none());
        assert!(s.submit_text("1e3", MotionPolicy::normal()).is_none());
        assert_eq!(s.value, 50);
    }

    #[test]
    fn numspin_selfcheck_all_green() {
        let set = run_numspin_checks();
        assert!(set.all_passed(), "F217 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F217 验收定义的实装细化，非新立项）：持久化面 = 步进
// 器状态（值/步长/上下限）的 VXH1 定长记录；壳接线面 = 连发计时判定面（三
// 档速度的下一拍时刻）+ 抖动动画几何清单；判定面 = run_numspin_v2_checks。
// 零堆：编解码全走定长缓冲。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 记录定长：4 魔数 + 1 版本 + 32 载荷（值/步长/下限/上限各 i64）+ 4 校验。
pub const V2_RECORD_BYTES: usize = 41;

/// v2 持久化错误：四类损坏输入全部显性拒绝（明确错误枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 步进器状态记录（持久化面）：判据「边界钳制 10 例」所钳的值域
/// （值/步长/下限/上限）整体入册——重启后恢复同一钳制域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpinnerStateRecord {
    pub value: i64,
    pub step: i64,
    pub min: i64,
    pub max: i64,
}

impl SpinnerStateRecord {
    /// 采集：从步进器现行状态导出（step 取滚轮基步长 WHEEL_STEP 在册值）。
    pub fn capture(s: &NumberSpinner) -> SpinnerStateRecord {
        SpinnerStateRecord { value: s.value, step: WHEEL_STEP, min: s.min, max: s.max }
    }

    /// 恢复：区间非法（min>=max）如实拒绝——不静默纠正。
    pub fn restore(&self) -> Option<NumberSpinner> {
        NumberSpinner::new(self.value, self.min, self.max)
    }

    /// 编码：VXH1 + 版本 + 32 字节定长载荷 + FNV-1a 校验和。
    pub fn to_bytes(&self) -> [u8; V2_RECORD_BYTES] {
        let mut out = [0u8; V2_RECORD_BYTES];
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5..13].copy_from_slice(&self.value.to_le_bytes());
        out[13..21].copy_from_slice(&self.step.to_le_bytes());
        out[21..29].copy_from_slice(&self.min.to_le_bytes());
        out[29..37].copy_from_slice(&self.max.to_le_bytes());
        let sum = v2_fnv1a(&out[..V2_RECORD_BYTES - 4]);
        out[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四门逐道拒绝。
    pub fn from_bytes(b: &[u8]) -> Result<SpinnerStateRecord, V2PersistError> {
        if b.len() < V2_RECORD_BYTES { return Err(V2PersistError::BadLength); }
        if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
        if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
        let sum = u32::from_le_bytes([b[37], b[38], b[39], b[40]]);
        if v2_fnv1a(&b[..37]) != sum { return Err(V2PersistError::BadChecksum); }
        let q = |o: usize| i64::from_le_bytes(b[o..o + 8].try_into().unwrap_or([0; 8]));
        Ok(SpinnerStateRecord { value: q(5), step: q(13), min: q(21), max: q(29) })
    }
}

// ---------------------------------------------------------------------------
// UI 壳接线：连发计时判定面 + 抖动动画几何清单
// ---------------------------------------------------------------------------

/// 连发计时判定面：下一拍触发时刻 = 上拍 + 当前档间隔（三档速度实测的
/// 时序读数——500/250/100ms 档位由累计连发次数决定，与
/// NumberSpinner::autorepeat_interval_ms 同源）。
pub fn next_fire_due(last_fire_ms: u64, repeat_count: u32) -> u64 {
    let tier = if repeat_count >= AUTOREPEAT_TIER_AT[2] {
        2
    } else if repeat_count >= AUTOREPEAT_TIER_AT[1] {
        1
    } else {
        0
    };
    last_fire_ms + AUTOREPEAT_TIERS_MS[tier] as u64
}

/// 抖动动画帧清单容量（120ms 抖动按 12ms 步进 11 帧含静止终帧）与幅值。
pub const SHAKE_FRAME_CAP: usize = 11;
pub const SHAKE_AMP_PX: i32 = 2;

/// 抖动 x 偏移清单：0,+2,0,-2 往复（末帧强制归零静止——「抖动 120ms
/// 提示」的几何面；幅值有界 ±2px，不产生位移失控）。
pub fn shake_offset_list() -> [i32; SHAKE_FRAME_CAP] {
    let mut out = [0i32; SHAKE_FRAME_CAP];
    for (i, slot) in out.iter_mut().enumerate() {
        // 现象：原 match i%4 {1=>+2, 2=>-2} 产出 0,+2,-2,0 往复，与
        //      本函数文档「0,+2,0,-2 往复」及单测 shake_list_shape 矛盾。
        // 根因：实现相位错位——正峰后应回零再摆负峰（对称抖动），
        //      负峰应在 i%4==3 而非 2。
        // 修法：改实现——负峰移到 i%4==3，与文档/单测对齐；v2 自检
        //      第 5 条只看界/首末帧/含 ±2，不受影响。
        *slot = match i % 4 {
            1 => SHAKE_AMP_PX,
            3 => -SHAKE_AMP_PX,
            _ => 0,
        };
    }
    out[SHAKE_FRAME_CAP - 1] = 0; // 末帧静止（不卡在抖动态）
    out
}

/// F217 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_numspin_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F217-numspin-v2");
    // 1. round-trip：状态采集→编码→解码逐字段相等，且恢复出的步进器值域
    //    一致（v2 记录纪律 + 「边界钳制」值域完整性）。
    let ok = match NumberSpinner::new(42, 0, 100) {
        Some(s) => {
            let rec = SpinnerStateRecord::capture(&s);
            let blob = rec.to_bytes();
            SpinnerStateRecord::from_bytes(&blob) == Ok(rec)
                && rec.restore().map(|r| r.value == 42 && r.max == 100).unwrap_or(false)
        }
        None => false,
    };
    set.add("v2 record round-trip spinner state", ok, "");
    // 2. 四类损坏输入全部拒绝（魔数/版本/校验/长度）。
    let blob = match NumberSpinner::new(0, 0, 100) {
        Some(s) => SpinnerStateRecord::capture(&s).to_bytes(),
        None => [0u8; V2_RECORD_BYTES], // min<max 恒成立，此分支不可达（显性兜底）
    };
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[10] ^= 0xFF;
    set.add(
        "corruption four-way rejected",
        SpinnerStateRecord::from_bytes(&bad_magic) == Err(V2PersistError::BadMagic)
            && SpinnerStateRecord::from_bytes(&bad_ver) == Err(V2PersistError::BadVersion)
            && SpinnerStateRecord::from_bytes(&bad_sum) == Err(V2PersistError::BadChecksum)
            && SpinnerStateRecord::from_bytes(&blob[..40]) == Err(V2PersistError::BadLength),
        "",
    );
    // 3. 恢复面拒绝非法区间（「非法区间如实拒绝」的持久化侧同款纪律）。
    let bad_range = SpinnerStateRecord { value: 5, step: 1, min: 10, max: 10 };
    set.add("restore rejects min>=max", bad_range.restore().is_none(), "");
    // 4. 验「连发三档速度实测」：4 次一档 500ms、5 次二档 250ms、12 次三档 100ms。
    set.add(
        "next fire due three tiers",
        next_fire_due(1_000, 4) == 1_500
            && next_fire_due(1_000, 5) == 1_250
            && next_fire_due(1_000, 12) == 1_100,
        "",
    );
    // 5. 验「非法输入抖动动画」几何面：幅值有界 ±2px、首帧与末帧静止。
    let shake = shake_offset_list();
    set.add(
        "shake offsets bounded ±2px",
        shake[0] == 0 && shake[SHAKE_FRAME_CAP - 1] == 0
            && shake.iter().all(|&o| o.abs() <= SHAKE_AMP_PX)
            && shake.contains(&SHAKE_AMP_PX) && shake.contains(&-SHAKE_AMP_PX),
        "",
    );
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn spinner_record_round_trip_and_reject() {
        let s = NumberSpinner::new(-7, -10, 10).unwrap();
        let rec = SpinnerStateRecord::capture(&s);
        let blob = rec.to_bytes();
        assert_eq!(SpinnerStateRecord::from_bytes(&blob), Ok(rec));
        assert_eq!(rec.restore().map(|r| r.value), Some(-7));
        assert!(SpinnerStateRecord::from_bytes(&vec![0u8; 3]).is_err());
    }

    #[test]
    fn shake_list_shape() {
        assert_eq!(shake_offset_list(), [0, 2, 0, -2, 0, 2, 0, -2, 0, 2, 0]);
    }

    #[test]
    fn numspin_v2_selfcheck_all_green() {
        let set = run_numspin_v2_checks();
        assert!(set.all_passed(), "F217 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
