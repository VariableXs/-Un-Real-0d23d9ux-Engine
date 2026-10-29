//! UNX-F48401 —— E·焦点切换千次零丢（正：随机切换 1000 次零丢 commit）
//! 判据原文：UNX-F48401-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥359 行；
//!   J1-2 焦点事件流建模：切换帧（SWITCH_TICKS 拍内过渡）+ 落定后 commit 路由；
//!   J1-3 正向判据：LCG 随机目标切换 1000 次，每次落定后 commit 必达焦点窗，
//!        丢键账本全程为空（零丢）；
//!   J1-4 路由正确性：每次落定后 focused == 本轮目标窗（route_ok == 1000）；
//!   J1-5 状态机合法性：窗号恒在 0..NWIN，落定拍数恒为 SWITCH_TICKS；
//!   J1-6 复现性：同种子两次千次序列指纹一致；越窗/自窗切换判据齐备。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD420; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_WIN: u16 = SEG_BASE + 1;     // 窗号越界
const E_ROUTE: u16 = SEG_BASE + 2;   // 落定后焦点 ≠ 本轮目标：路由破坏
const E_LOST: u16 = SEG_BASE + 3;    // commit 落入丢键账本：零丢判据破坏
const E_STATE: u16 = SEG_BASE + 4;   // 状态机非法：帧残留/拍数偏差
const E_FINGER: u16 = SEG_BASE + 5;  // 序列指纹不稳定
const E_TICKS: u16 = SEG_BASE + 6;   // 落定拍数与 SWITCH_TICKS 不符
const E_NOOP: u16 = SEG_BASE + 7;    // 同窗切换非 no-op：状态被误改
const E_COUNT: u16 = SEG_BASE + 8;   // 千次计数不符
const E_SEED: u16 = SEG_BASE + 9;    // 不同种子序列无差异（疑似常量退化）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_WIN => "窗号越界：0..NWIN 之外拒绝",
        E_ROUTE => "落定后焦点不等于本轮目标：commit 路由破坏",
        E_LOST => "commit 落入丢键账本：千次零丢判据破坏",
        E_STATE => "状态机非法：帧残留或焦点越界",
        E_FINGER => "序列指纹不稳定：确定性被破坏",
        E_TICKS => "落定拍数与切换帧时长不符",
        E_NOOP => "同窗切换非 no-op：焦点状态被误改",
        E_COUNT => "千次计数不符：切换/路由轮次缺失",
        E_SEED => "不同种子序列无差异：随机源退化",
        _ => "未知诊断码",
    }
}

/// 确定性 LCG。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 数据模型 ────────────────────────────────────────────────
/// 窗口总数。
pub const NWIN: u8 = 8;
/// 切换帧时长（拍）：帧内 commit 一律入丢键账本。
pub const SWITCH_TICKS: u8 = 2;
/// 千次轮数。
pub const ROUNDS: u32 = 1000;

/// 切换帧：from → to 的过渡态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchFrame {
    pub from: u8,
    pub to: u8,
    pub ticks_left: u8,
}

/// 丢键账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LostKey {
    pub key: u32,
    pub frame_no: u64,
}

/// 焦点状态机。
pub struct FocusState {
    pub focused: u8,
    pub tick: u64,
    pub switches: u32,
    pub frame: Option<SwitchFrame>,
    pub frame_no: u64,
    pub routed: u32,
    pub lost: Vec<LostKey>,
}

impl FocusState {
    pub fn new(start: u8) -> Result<FocusState, u16> {
        if start >= NWIN {
            return Err(E_WIN);
        }
        Ok(FocusState {
            focused: start,
            tick: 0,
            switches: 0,
            frame: None,
            frame_no: 0,
            routed: 0,
            lost: Vec::new(),
        })
    }

    /// 切换：同窗 no-op 返回 Ok(false)；异窗开切换帧返回 Ok(true)。
    pub fn switch_to(&mut self, to: u8) -> Result<bool, u16> {
        if to >= NWIN {
            return Err(E_WIN);
        }
        if to == self.focused {
            return Ok(false);
        }
        self.frame_no += 1;
        self.frame = Some(SwitchFrame {
            from: self.focused,
            to,
            ticks_left: SWITCH_TICKS,
        });
        self.switches += 1;
        Ok(true)
    }

    /// 推进一拍：帧计时递减，归零即落定。
    pub fn advance_tick(&mut self) {
        self.tick += 1;
        if let Some(f) = self.frame {
            if f.ticks_left <= 1 {
                self.focused = f.to;
                self.frame = None;
            } else {
                self.frame = Some(SwitchFrame {
                    ticks_left: f.ticks_left - 1,
                    ..f
                });
            }
        }
    }

    /// 已落定（无切换帧残留）。
    pub fn settled(&self) -> bool {
        self.frame.is_none()
    }

    /// commit：落定态路由到焦点窗返回 Ok(true)；
    /// 帧内 commit 一律捕获入丢键账本返回 Ok(false)。
    pub fn commit(&mut self, key: u32) -> Result<bool, u16> {
        if self.frame.is_some() {
            self.lost.push(LostKey {
                key,
                frame_no: self.frame_no,
            });
            return Ok(false);
        }
        if self.focused >= NWIN {
            return Err(E_STATE);
        }
        self.routed += 1;
        Ok(true)
    }

    /// 状态合法性闭包。
    pub fn legal(&self) -> bool {
        self.focused < NWIN && self.settled()
    }

    /// 状态指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![0xD4u8, 0x20];
        buf.push(self.focused);
        buf.extend_from_slice(&self.tick.to_le_bytes());
        buf.extend_from_slice(&self.switches.to_le_bytes());
        buf.extend_from_slice(&self.routed.to_le_bytes());
        buf.extend_from_slice(&self.lost.len().to_le_bytes());
        buf.extend_from_slice(&self.frame_no.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 千次运行报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub switches: u32,
    pub routed: u32,
    pub route_ok: u32,
    pub lost: usize,
    pub last_target: u8,
    pub focused_end: u8,
    pub tick_end: u64,
    pub fp: u64,
}

/// 千次随机切换：LCG 逐轮选异窗目标，落定即 commit。
pub fn run_thousand(seed: u32) -> Result<RunReport, u16> {
    let mut st = FocusState::new(0)?;
    let mut state: u32 = seed;
    let mut route_ok: u32 = 0;
    let mut last_target: u8 = 0;
    for _ in 0..ROUNDS {
        let mut target = (lcg_next(&mut state) % (NWIN as u32)) as u8;
        while target == st.focused {
            target = (lcg_next(&mut state) % (NWIN as u32)) as u8;
        }
        st.switch_to(target)?;
        while !st.settled() {
            st.advance_tick();
        }
        if st.focused != target {
            return Err(E_ROUTE);
        }
        route_ok += 1;
        let key = lcg_next(&mut state);
        if !st.commit(key)? {
            return Err(E_LOST);
        }
        last_target = target;
    }
    let mut buf: Vec<u8> = vec![0xD4u8, 0x21];
    buf.extend_from_slice(&st.switches.to_le_bytes());
    buf.extend_from_slice(&st.routed.to_le_bytes());
    buf.extend_from_slice(&st.lost.len().to_le_bytes());
    buf.push(last_target);
    buf.push(st.focused);
    Ok(RunReport {
        switches: st.switches,
        routed: st.routed,
        route_ok,
        lost: st.lost.len(),
        last_target,
        focused_end: st.focused,
        tick_end: st.tick,
        fp: fnv1a64(&buf),
    })
}

// ── 主巡检入口 ──────────────────────────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：千次切换零丢：switches == routed == 1000，丢键账本空。
    let r1 = run_thousand(0xD420_0001)?;
    if r1.switches != ROUNDS || r1.routed != ROUNDS || r1.lost != 0 {
        return Err(E_LOST);
    }
    hits += 1;

    // R2：路由正确性：每轮落定后焦点 == 目标，route_ok == 1000。
    if r1.route_ok != ROUNDS {
        return Err(E_ROUTE);
    }
    hits += 1;

    // R3：状态机合法性：终态焦点在窗内、无帧残留、落定拍数 = 2×1000。
    if r1.focused_end >= NWIN {
        return Err(E_STATE);
    }
    if r1.tick_end != (SWITCH_TICKS as u64) * (ROUNDS as u64) {
        return Err(E_TICKS);
    }
    hits += 1;

    // R4：手动微程零丢：8 窗轮转各 commit 一次，账本恒空、路由恒真。
    let mut st4 = FocusState::new(0)?;
    for w in 0..NWIN {
        if w == st4.focused {
            continue;
        }
        st4.switch_to(w)?;
        while !st4.settled() {
            st4.advance_tick();
        }
        if !st4.commit(0xAA00 + w as u32)? {
            return Err(E_LOST);
        }
        if !st4.lost.is_empty() {
            return Err(E_LOST);
        }
    }
    if st4.routed != 7 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R5：复现性：同种子两次千次运行指纹一致。
    let r5a = run_thousand(0xD420_005A)?;
    let r5b = run_thousand(0xD420_005A)?;
    if r5a.fp != r5b.fp {
        return Err(E_FINGER);
    }
    hits += 1;

    // R6：边界窗口：切到 NWIN-1 与切回 0 均可落定并路由。
    let mut st6 = FocusState::new(0)?;
    st6.switch_to(NWIN - 1)?;
    while !st6.settled() {
        st6.advance_tick();
    }
    if st6.focused != NWIN - 1 || !st6.commit(1)? {
        return Err(E_ROUTE);
    }
    st6.switch_to(0)?;
    while !st6.settled() {
        st6.advance_tick();
    }
    if st6.focused != 0 || !st6.commit(2)? {
        return Err(E_ROUTE);
    }
    hits += 1;

    // R7：同窗 no-op：switch_to(当前) 返回 Ok(false) 且不改状态。
    let mut st7 = FocusState::new(3)?;
    let sw_before = st7.switches;
    if st7.switch_to(3)? {
        return Err(E_NOOP);
    }
    if st7.switches != sw_before || st7.frame.is_some() {
        return Err(E_NOOP);
    }
    hits += 1;

    // R8：越窗拒绝：switch_to(NWIN) → E_WIN；new(NWIN) → E_WIN。
    let mut st8 = FocusState::new(0)?;
    if st8.switch_to(NWIN) != Err(E_WIN) {
        return Err(E_WIN);
    }
    if FocusState::new(NWIN).is_ok() {
        return Err(E_WIN);
    }
    hits += 1;

    // R9：种子敏感：不同种子两次运行均绿，且序列指纹有差异。
    let r9a = run_thousand(0xD420_00A0)?;
    let r9b = run_thousand(0xD420_00A1)?;
    if r9a.switches != ROUNDS || r9b.switches != ROUNDS {
        return Err(E_COUNT);
    }
    if r9a.fp == r9b.fp {
        return Err(E_SEED);
    }
    hits += 1;

    // R10：终态一致：focused_end == last_target（最后一次切换目标）。
    if r1.focused_end != r1.last_target {
        return Err(E_ROUTE);
    }
    if !r1.last_target.to_string().is_empty() {
        // 报告文本拼装判据：千次账目文本非空。
        let mut s = String::new();
        s.push_str("fok1k switches=");
        s.push_str(&r1.switches.to_string());
        s.push_str(" lost=0");
        if !s.contains("1000") {
            return Err(E_COUNT);
        }
    }
    hits += 1;

    Ok(hits)
}
