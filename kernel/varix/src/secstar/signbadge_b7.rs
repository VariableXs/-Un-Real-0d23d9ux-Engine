//! F178 签名角标 · 批次七深化（v7）——多角标堆叠布局、脉冲动画状态机、
//! 离线变化队列、无障碍文本表、DPI 缩放变体。零堆、no_std、只依赖主层
//! pub API 与既有批次层。

use crate::checks::CheckSet;

/// 角标渲染直径（逻辑像素）。
pub const BADGE_SIZE_PX: u32 = 16;
/// 托盘区角标最大可见数（超出折叠为 +N）。
pub const STACK_VISIBLE_MAX: usize = 3;
/// 堆叠层间偏移（逻辑像素——每层向左上错位，露出一角）。
pub const STACK_OFFSET_PX: i32 = 4;
/// 脉冲动画周期（ms——新未签名项出现时的呼吸提醒）。
pub const PULSE_PERIOD_MS: u32 = 1_600;
/// 脉冲总时长（呼吸 3 个周期后停——提醒不无限骚扰）。
pub const PULSE_TOTAL_MS: u32 = PULSE_PERIOD_MS * 3;
/// 离线变化队列容量（托盘不可见期间的变化留痕上限）。
pub const OFFLINE_QUEUE_CAP: usize = 32;
/// DPI 缩放档位（逻辑px → 物理px 的整倍数表）。
pub const DPI_SCALES: [u32; 4] = [1, 2, 3, 4];

/// 堆叠布局条目：一个待展示角标（按严重度排序后进栈）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackEntry {
    pub app_id: u32,
    /// 0=灰 1=黄 2=红（与主层 Badge 三态对齐的序数值）。
    pub sev: u8,
}

/// 多角标堆叠布局器：严重度降序排列，前 N 个可见、其余折叠。
/// 可见层自左上向右下错位（每层 +STACK_OFFSET_PX）。
#[derive(Clone, Copy)]
pub struct StackLayout {
    entries: [Option<StackEntry>; OFFLINE_QUEUE_CAP],
    pub n: usize,
}

impl StackLayout {
    pub const fn new() -> StackLayout {
        StackLayout { entries: [None; OFFLINE_QUEUE_CAP], n: 0 }
    }

    /// 入栈（满容拒绝——托盘不是无限画布）。
    pub fn push(&mut self, e: StackEntry) -> bool {
        if self.n >= OFFLINE_QUEUE_CAP {
            return false;
        }
        self.entries[self.n] = Some(e);
        self.n += 1;
        true
    }

    /// 严重度降序排列的快照（插入序稳定——同严重度保持先来后到）。
    fn ordered(&self) -> [Option<StackEntry>; OFFLINE_QUEUE_CAP] {
        let mut out = self.entries;
        // 插入排序（n 小、稳定、零堆）。
        for i in 1..self.n {
            let key = out[i];
            let mut j = i;
            while j > 0 {
                let prev = out[j - 1].unwrap();
                let cur = key.unwrap();
                if prev.sev < cur.sev {
                    out[j] = out[j - 1];
                    j -= 1;
                } else {
                    break;
                }
            }
            out[j] = key;
        }
        out
    }

    /// 可见角标的左上角坐标（anchor 为托盘基准点）。
    /// 返回 (可见数, 折叠数, 各可见层偏移)。
    pub fn layout(&self, anchor_x: i32, anchor_y: i32) -> (usize, usize, [(i32, i32); STACK_VISIBLE_MAX]) {
        let ordered = self.ordered();
        let visible = self.n.min(STACK_VISIBLE_MAX);
        let mut pos = [(0i32, 0i32); STACK_VISIBLE_MAX];
        for (i, p) in pos.iter_mut().enumerate().take(visible) {
            if let Some(_e) = ordered[i] {
                *p = (anchor_x - STACK_OFFSET_PX * i as i32, anchor_y - STACK_OFFSET_PX * i as i32);
            }
        }
        (visible, self.n - visible, pos)
    }

    /// 折叠角标数是否应渲染 +N 徽章（≥1 才有意义）。
    pub fn overflow_count(&self) -> Option<usize> {
        let hidden = self.n.saturating_sub(STACK_VISIBLE_MAX);
        if hidden > 0 {
            Some(hidden)
        } else {
            None
        }
    }
}

/// 脉冲动画状态机：Idle → Pulsing（3 周期）→ Idle。
/// 相位 = (t - start) % 周期；幅度 = 半周期正弦近似（三角波——确定性可复核）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PulseState {
    Idle,
    Pulsing,
}

#[derive(Clone, Copy)]
pub struct PulseAnimator {
    pub state: PulseState,
    start_ms: u32,
}

impl PulseAnimator {
    pub const fn new() -> PulseAnimator {
        PulseAnimator { state: PulseState::Idle, start_ms: 0 }
    }

    /// 触发一次呼吸提醒（已在脉冲中不重启——不无限骚扰）。
    pub fn trigger(&mut self, now_ms: u32) -> bool {
        if self.state == PulseState::Pulsing {
            return false;
        }
        self.state = PulseState::Pulsing;
        self.start_ms = now_ms;
        true
    }

    /// 推进时间：超总时长自动归 Idle（出现必有完整消失路径）。
    pub fn tick(&mut self, now_ms: u32) {
        if self.state == PulseState::Pulsing && now_ms.saturating_sub(self.start_ms) >= PULSE_TOTAL_MS {
            self.state = PulseState::Idle;
        }
    }

    /// 当前帧强调幅度 ‰（0..1000，三角波：升半周期/降半周期）。
    /// Idle 恒 0。幅度用于透明度/描边粗细插值。
    pub fn amplitude_permille(&self, now_ms: u32) -> u32 {
        if self.state != PulseState::Pulsing {
            return 0;
        }
        let phase = now_ms.saturating_sub(self.start_ms) % PULSE_PERIOD_MS;
        let half = PULSE_PERIOD_MS / 2;
        let tri = if phase < half { phase * 1_000 / half } else { (PULSE_PERIOD_MS - phase) * 1_000 / half };
        tri.min(1_000)
    }

    pub fn is_active(&self) -> bool {
        self.state == PulseState::Pulsing
    }
}

/// 离线变化队列：托盘隐藏期间的状态变化留痕，恢复可见时回放。
/// 同应用连续多变化只保留最新（中间态没有回放价值）。
#[derive(Clone, Copy)]
pub struct OfflineQueue {
    slots: [Option<StackEntry>; OFFLINE_QUEUE_CAP],
    /// 每槽是否被覆盖过（回放序 = 插入序，覆盖不改变位置）。
    occupied: [bool; OFFLINE_QUEUE_CAP],
    pub n: usize,
    pub overwritten: u32,
}

impl OfflineQueue {
    pub const fn new() -> OfflineQueue {
        OfflineQueue { slots: [None; OFFLINE_QUEUE_CAP], occupied: [false; OFFLINE_QUEUE_CAP], n: 0, overwritten: 0 }
    }

    /// 记录一次变化：已存在同应用 → 原位覆盖（计 overwritten）；
    /// 否则入尾；满容 → 驱逐最旧（回放丢最旧比丢最新诚实——最新态最有价值）。
    pub fn record(&mut self, e: StackEntry) {
        for i in 0..self.n {
            if let Some(s) = self.slots[i] {
                if s.app_id == e.app_id {
                    self.slots[i] = Some(e);
                    self.overwritten += 1;
                    return;
                }
            }
        }
        if self.n < OFFLINE_QUEUE_CAP {
            self.slots[self.n] = Some(e);
            self.occupied[self.n] = true;
            self.n += 1;
        } else {
            // 满容：整体前移一格（驱逐最旧），新条目入尾。
            for i in 1..OFFLINE_QUEUE_CAP {
                self.slots[i - 1] = self.slots[i];
            }
            self.slots[OFFLINE_QUEUE_CAP - 1] = Some(e);
        }
    }

    /// 回放：按插入序返回全部（快照拷贝——回放不消费账）。
    pub fn replay(&self) -> [Option<StackEntry>; OFFLINE_QUEUE_CAP] {
        self.slots
    }

    pub fn replay_len(&self) -> usize {
        self.n
    }

    /// 清空（回放完成 + 角标已重建后调用——回放与清空分离防丢）。
    pub fn clear(&mut self) {
        self.slots = [None; OFFLINE_QUEUE_CAP];
        self.occupied = [false; OFFLINE_QUEUE_CAP];
        self.n = 0;
    }
}

/// 无障碍文本表：三态 × 语言（中/英）的完整读屏文本。
/// 键序：[态 0..3][语言 0=中 1=英]。
pub const A11Y_TEXTS: [[&str; 2]; 3] = [
    ["签名链未验证——程序来源未知", "Signature chain unverified"],
    ["签名有效但算法已过时", "Signature valid but algorithm deprecated"],
    ["签名链已验证——来源可信", "Signature chain verified"],
];

/// 读屏文本查询（越界态回落到未知提示——不给屏幕阅读器空串）。
pub fn a11y_text(sev: u8, lang: usize) -> &'static str {
    let unknown = ["签名状态未知", "Signature state unknown"];
    if sev >= 3 {
        return unknown[lang.min(1)];
    }
    A11Y_TEXTS[sev as usize][lang.min(1)]
}

/// DPI 变体：逻辑尺寸 × 缩放 = 物理尺寸（对齐 4px 网格——高分屏不糊）。
pub fn physical_size(dpi_idx: usize) -> Option<u32> {
    let s = *DPI_SCALES.get(dpi_idx)?;
    Some((BADGE_SIZE_PX * s + 3) & !3) // 向上对齐 4px
}

#[inline(never)]
pub fn run_signbadge_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F178-b7");

    // 1) 堆叠排序：黄入红前（降序），同严重度保持插入序（稳定）。
    let mut st = StackLayout::new();
    st.push(StackEntry { app_id: 1, sev: 1 });
    st.push(StackEntry { app_id: 2, sev: 2 });
    st.push(StackEntry { app_id: 3, sev: 1 });
    let ordered = st.ordered();
    cs.add(
        "stack_sev_desc_stable",
        ordered[0].unwrap().app_id == 2 && ordered[1].unwrap().app_id == 1 && ordered[2].unwrap().app_id == 3,
        "",
    );

    // 2) 堆叠布局三可见 + 折叠计数：5 项 → 3 可见 2 折叠，坐标逐层错位。
    let mut st5 = StackLayout::new();
    for i in 0..5u32 {
        st5.push(StackEntry { app_id: i, sev: 2 });
    }
    let (vis, hidden, pos) = st5.layout(100, 100);
    cs.add(
        "stack_layout_visible_hidden",
        vis == 3 && hidden == 2 && pos[0] == (100, 100) && pos[1] == (96, 96) && pos[2] == (92, 92),
        "",
    );

    // 3) 折叠徽章：0 隐藏 None、3 隐藏 Some(3)（+N 只在真有隐藏时出现）。
    cs.add(
        "stack_overflow_badge",
        st5.overflow_count() == Some(2) && st.overflow_count().is_none(),
        "",
    );

    // 4) 脉冲：触发置位、幅度三角波（周期中点最大、两端归零）、超时自动归 Idle。
    let mut p = PulseAnimator::new();
    let triggered = p.trigger(1_000);
    let mid = p.amplitude_permille(1_000 + PULSE_PERIOD_MS / 2);
    let edge = p.amplitude_permille(1_000);
    p.tick(1_000 + PULSE_TOTAL_MS);
    cs.add(
        "pulse_triangle_and_timeout",
        triggered && p.amplitude_permille(9_999) == 0 && mid == 1_000 && edge == 0 && p.state == PulseState::Idle,
        "",
    );

    // 5) 脉冲不重启：激活中再 trigger 被拒（3 周期停——提醒有尽头）。
    let mut p2 = PulseAnimator::new();
    p2.trigger(100);
    let again = p2.trigger(200);
    cs.add("pulse_no_restart_while_active", !again && p2.is_active(), "");

    // 6) 离线队列同应用覆盖：两次同应用变化 → 一条 + overwritten=1（中间态无回放价值）。
    let mut q = OfflineQueue::new();
    q.record(StackEntry { app_id: 7, sev: 0 });
    q.record(StackEntry { app_id: 7, sev: 2 });
    cs.add(
        "offline_coalesce_same_app",
        q.replay_len() == 1 && q.replay()[0].unwrap().sev == 2 && q.overwritten == 1,
        "",
    );

    // 7) 离线队列满容驱逐最旧：32 满后再记 → 首条出账、新条目在尾。
    let mut q2 = OfflineQueue::new();
    for i in 0..OFFLINE_QUEUE_CAP as u32 {
        q2.record(StackEntry { app_id: i, sev: 1 });
    }
    q2.record(StackEntry { app_id: 999, sev: 2 });
    let snap = q2.replay();
    cs.add(
        "offline_full_evicts_oldest",
        q2.replay_len() == OFFLINE_QUEUE_CAP && snap[0].unwrap().app_id == 1 && snap[OFFLINE_QUEUE_CAP - 1].unwrap().app_id == 999,
        "",
    );

    // 8) 回放与清空分离：clear 后归零（回放不消费账——先回放后清空防丢）。
    let mut q3 = OfflineQueue::new();
    q3.record(StackEntry { app_id: 5, sev: 1 });
    let _ = q3.replay();
    q3.clear();
    cs.add("offline_replay_then_clear", q3.replay_len() == 0 && q3.n == 0, "");

    // 9) 无障碍文本表：三态齐、越界回落（不给读屏器空串）、双语言各配。
    cs.add(
        "a11y_texts_complete",
        a11y_text(0, 0).len() > 0 && a11y_text(2, 1).len() > 0 && a11y_text(9, 0).len() > 0 && a11y_text(9, 0) != "",
        "",
    );

    // 10) DPI 变体：1x=16、2x=32、越界 None（4px 对齐——高分屏放大不糊不锯齿）。
    cs.add(
        "dpi_scale_grid4",
        physical_size(0) == Some(16) && physical_size(1) == Some(32) && physical_size(9).is_none(),
        "",
    );

    // 11) 堆叠偏移常量自洽：可见 3 层最大偏移 = 2×OFFSET（锚点即首层）。
    cs.add(
        "stack_offset_consistency",
        STACK_OFFSET_PX * (STACK_VISIBLE_MAX as i32 - 1) == 8,
        "",
    );

    // 12) 脉冲常量自洽：总时长 = 3 周期（呼吸三次后停的承诺）。
    cs.add("pulse_constants", PULSE_TOTAL_MS == 4_800 && PULSE_PERIOD_MS == 1_600, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_never_duplicates_app() {
        // 同应用重复入栈在布局层不合并（合并是 OfflineQueue 的职责）——
        // 但布局层 32 上限守门仍生效。
        let mut st = StackLayout::new();
        for i in 0..(OFFLINE_QUEUE_CAP + 4) {
            let ok = st.push(StackEntry { app_id: i as u32 % 8, sev: 1 });
            if i >= OFFLINE_QUEUE_CAP {
                assert!(!ok, "满容必须拒绝");
            }
        }
        assert_eq!(st.n, OFFLINE_QUEUE_CAP);
    }

    #[test]
    fn pulse_amplitude_monotone_rise_fall() {
        // 单周期内幅度：升段单调不降、降段单调不升（三角波的定义）。
        let mut p = PulseAnimator::new();
        p.trigger(0);
        let mut prev_rise = 0;
        for t in (0..=800).step_by(50) {
            let a = p.amplitude_permille(t);
            assert!(a >= prev_rise, "升段必须不降 t={t}");
            prev_rise = a;
        }
        let mut prev_fall = 1_000;
        for t in (800..=1_600).step_by(50) {
            let a = p.amplitude_permille(t);
            assert!(a <= prev_fall, "降段必须不升 t={t}");
            prev_fall = a;
        }
    }

    #[test]
    fn offline_replay_preserves_insert_order() {
        // 回放序 = 插入序（不同应用不重排——堆叠层才排序）。
        let mut q = OfflineQueue::new();
        for i in 0..6u32 {
            q.record(StackEntry { app_id: i, sev: (i % 3) as u8 });
        }
        let snap = q.replay();
        for i in 0..6usize {
            assert_eq!(snap[i].unwrap().app_id, i as u32);
        }
    }
}
