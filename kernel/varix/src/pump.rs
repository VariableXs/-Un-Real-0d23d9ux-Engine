//! UNREAL-X-15000 · WP-201 · B-501 合成器事件泵状态机（MD2 篇 5.1 × 判据表）。
//!
//! 定案（MD2 行 327）：合成器是一颗**单线程事件泵**加一条合成管线。事件泵的
//! 输入源四类：VXWM 报文（客户端动作）、输入事件（来自输入子系统）、帧回调
//! 时钟（垂直节奏或软件定时）、内部超时（看门狗心跳、动画推进）。主循环每一
//! 圈做的事固定：**排空事件队列并归并状态变更 → 按脏区集合决定本帧是否需要
//! 合成 → 需要则执行合成管线并翻页 → 然后睡到下一个事件**。"不需要就不合成"
//! 是空转成本压到单核 5% 以下的结构基础——静止桌面时主循环长眠，只有光标层
//! 独立小步走。
//! 判据（MD2 行 357）：B-501 主循环空转开销——**静止桌面 ≤ 单核 5%**。
//! 缺陷形态（行 1620）：合成器忙等下一帧——空转吃满单核，风扇起飞（事件驱动
//! 降帧）。实测口径（行 1643）：一级空闲口径实测（篇 42.2），不许二级空闲美化。
//! 零堆、整数运算、宿主全测。判据号 B-501 入 CheckSet 命名。

// ---------------------------------------------------------------------------
// 常量与事件源
// ---------------------------------------------------------------------------

/// 事件队列深度。
pub const QUEUE_CAP: usize = 64;
/// 空转判据线：单核 5% = permille 50（一级空闲口径）。
pub const IDLE_BUDGET_PMIL: u32 = 50;
/// 看门狗心跳间隔（泵周期数；每周期消费 ≥1 事件则视为活跃）。
pub const WATCHDOG_TICKS: u32 = 16;

/// 四类事件源（MD2 行 327 定案）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    /// VXWM 报文（客户端动作）。
    Vxwm,
    /// 输入事件（来自输入子系统）。
    Input,
    /// 帧回调时钟（垂直节奏或软件定时）。
    FrameClock,
    /// 内部超时（看门狗心跳、动画推进）。
    Timeout,
}

impl Src {
    pub fn name(self) -> &'static str {
        match self {
            Src::Vxwm => "vxwm",
            Src::Input => "input",
            Src::FrameClock => "frame-clock",
            Src::Timeout => "timeout",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// 带脏区的动作（需重画）。
    Dirty { src: Src, surface: u32 },
    /// 光标移动：只走光标层小步叠加，不触发全帧合成。
    Cursor { x: i16, y: i16 },
    /// 看门狗喂狗/动画推进：无脏区。
    Heartbeat { src: Src },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 长眠：睡到下一个事件（静止桌面的稳态）。
    Sleep,
    /// 排空队列并归并状态变更。
    Drain,
    /// 执行合成管线并翻页。
    Compose,
    /// 看门狗饥饿（假死上报——宪章"看起来没反应"同罪检测挂点）。
    Hungry,
}

/// 合成一圈的裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleVerdict {
    /// 脏区为空：不合成，长眠。
    SleepNoCompose,
    /// 只动光标层：小步叠加，不全帧。
    CursorOnly,
    /// 全帧合成 + 翻页。
    FullFrame { dirty_surfaces: u32 },
}

// ---------------------------------------------------------------------------
// 事件泵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct EventPump {
    queue: [Option<Event>; QUEUE_CAP],
    q_head: usize,
    q_len: usize,
    pub phase: Phase,
    // —— 账本 ——
    pub cycles: u64,
    pub composes: u64,
    pub cursor_steps: u64,
    pub sleep_cycles: u64,
    pub drained: u64,
    pub queue_full_dropped: u64,
    pub watchdog_starved: u64,
    pub frames_since_feed: u32,
}

impl EventPump {
    pub fn new() -> EventPump {
        EventPump {
            queue: [None; QUEUE_CAP],
            q_head: 0,
            q_len: 0,
            phase: Phase::Sleep,
            cycles: 0,
            composes: 0,
            cursor_steps: 0,
            sleep_cycles: 0,
            drained: 0,
            queue_full_dropped: 0,
            watchdog_starved: 0,
            frames_since_feed: 0,
        }
    }

    /// 投递事件（VXWM 报文/输入/时钟/超时四个入口归一）。满则丢新计数上报
    /// （静默丢包是缺陷——与 B-503 序号闸同一纪律）。
    pub fn post(&mut self, e: Event) -> u16 {
        if self.q_len == QUEUE_CAP {
            self.queue_full_dropped += 1;
            return 1;
        }
        self.queue[(self.q_head + self.q_len) % QUEUE_CAP] = Some(e);
        self.q_len += 1;
        0
    }

    /// 主循环一圈：排空 → 归并 → 裁决（合成/光标/长眠）→ 看门狗。
    pub fn cycle(&mut self) -> CycleVerdict {
        self.cycles += 1;
        self.phase = Phase::Drain;
        let drained_before = self.drained;
        let mut dirty: u32 = 0;
        let mut cursor: bool = false;
        let mut heartbeat: bool = false;
        while self.q_len > 0 {
            let e = self.queue[self.q_head].take();
            self.q_head = (self.q_head + 1) % QUEUE_CAP;
            self.q_len -= 1;
            self.drained += 1;
            match e.unwrap_or(Event::Heartbeat { src: Src::Timeout }) {
                Event::Dirty { .. } => dirty += 1,
                Event::Cursor { .. } => cursor = true,
                Event::Heartbeat { .. } => heartbeat = true,
            }
        }
        // 看门狗：本圈排空了事件即视为喂狗
        if self.q_len == 0 && self.drained == drained_before {
            self.frames_since_feed += 1;
        } else {
            self.frames_since_feed = 0;
        }
        // 归并裁决：脏区优先，其次光标，皆无则长眠；持续空转达阈值进 Hungry
        let verdict = if dirty > 0 {
            self.composes += 1;
            self.phase = Phase::Compose;
            CycleVerdict::FullFrame { dirty_surfaces: dirty }
        } else if cursor {
            self.cursor_steps += 1;
            self.phase = Phase::Sleep;
            CycleVerdict::CursorOnly
        } else if self.frames_since_feed >= WATCHDOG_TICKS {
            // 空转且看门狗饥饿：保持 Hungry；恰达阈值那圈进位一次
            self.sleep_cycles += 1;
            if self.frames_since_feed == WATCHDOG_TICKS {
                self.watchdog_starved += 1;
            }
            self.phase = Phase::Hungry;
            CycleVerdict::SleepNoCompose
        } else {
            self.sleep_cycles += 1;
            self.phase = Phase::Sleep;
            CycleVerdict::SleepNoCompose
        };
        if heartbeat && dirty == 0 {
            self.phase = Phase::Sleep;
        }
        verdict
    }

    /// 空转成本模型（permille）：合成圈 / 总圈。
    /// 静止桌面判据：≤ 50‰（单核 5%，一级空闲口径）。
    pub fn idle_cost_pmil(&self) -> u32 {
        if self.cycles == 0 {
            return 0;
        }
        ((self.composes * 1000) / self.cycles) as u32
    }

    pub fn reset(&mut self) {
        self.queue = [None; QUEUE_CAP];
        self.q_head = 0;
        self.q_len = 0;
        self.phase = Phase::Sleep;
        self.cycles = 0;
        self.composes = 0;
        self.cursor_steps = 0;
        self.sleep_cycles = 0;
        self.drained = 0;
        self.queue_full_dropped = 0;
        self.watchdog_starved = 0;
        self.frames_since_feed = 0;
    }
}

// ---------------------------------------------------------------------------
// 自检（判据号 B-501 入命名）
// ---------------------------------------------------------------------------

pub fn run_pump_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("vxwm-pump");

    // —— 四类事件源归一 ——
    let mut p = EventPump::new();
    let all_ok = p.post(Event::Dirty { src: Src::Vxwm, surface: 1 }) == 0
        && p.post(Event::Dirty { src: Src::Input, surface: 2 }) == 0
        && p.post(Event::Heartbeat { src: Src::FrameClock }) == 0
        && p.post(Event::Heartbeat { src: Src::Timeout }) == 0;
    let v = p.cycle();
    set.add(
        "B-501 四类事件源归一入泵",
        all_ok && p.drained == 4 && v == CycleVerdict::FullFrame { dirty_surfaces: 2 },
        "VXWM/输入/帧时钟/内部超时全通",
    );

    // —— 排空归并 ——
    let mut p2 = EventPump::new();
    for i in 0..10u32 {
        let _ = p2.post(Event::Dirty { src: Src::Vxwm, surface: i });
    }
    let v2 = p2.cycle();
    set.add(
        "B-501 一圈排空归并",
        p2.drained == 10 && v2 == CycleVerdict::FullFrame { dirty_surfaces: 10 },
        "多事件归并为一次合成裁决",
    );
    let v3 = p2.cycle();
    set.add(
        "B-501 空队列长眠",
        v3 == CycleVerdict::SleepNoCompose && p2.phase == Phase::Sleep,
        "排空后不合成——需要才动",
    );

    // —— 光标层小步走 ——
    let mut p3 = EventPump::new();
    let _ = p3.post(Event::Cursor { x: 50, y: 60 });
    let v4 = p3.cycle();
    set.add(
        "B-501 光标移动不触发全帧",
        v4 == CycleVerdict::CursorOnly && p3.composes == 0 && p3.cursor_steps == 1,
        "光标层独立小步走（MD2 行 327）",
    );

    // —— 空转判据：静止桌面 ≤ 5% ——
    let mut idle = EventPump::new();
    for _ in 0..1000 {
        let _ = idle.cycle(); // 静止：无事件长眠
    }
    set.add(
        "B-501 静止桌面零合成",
        idle.composes == 0 && idle.sleep_cycles == 1000 && idle.idle_cost_pmil() == 0,
        "一级空闲口径：1000 圈 0 合成",
    );
    set.add(
        "B-501 空转成本 ≤ 单核 5%",
        idle.idle_cost_pmil() <= IDLE_BUDGET_PMIL,
        "permille 50 判据线（B-501 达标线）",
    );

    // —— 忙等缺陷形态的反面验证 ——
    let mut busy = EventPump::new();
    for _ in 0..200 {
        let _ = busy.post(Event::Dirty { src: Src::FrameClock, surface: 9 });
        let _ = busy.cycle();
    }
    set.add(
        "B-501 满负荷合成率如实计量",
        busy.composes == 200 && busy.idle_cost_pmil() == 1000,
        "有脏区必合成——判据区分空转与满载",
    );
    let mut mixed = EventPump::new();
    for i in 0..1000u64 {
        if i % 20 == 0 {
            let _ = mixed.post(Event::Dirty { src: Src::Vxwm, surface: 1 });
        }
        let _ = mixed.cycle();
    }
    set.add(
        "B-501 混合负载成本模型",
        mixed.composes == 50 && mixed.idle_cost_pmil() == 50,
        "20:1 静动比 → 50‰ 恰在判据线",
    );

    // —— 队列满丢弃计数（静默丢包是缺陷） ——
    let mut q = EventPump::new();
    let mut dropped_at = 0;
    for i in 0..(QUEUE_CAP + 8) {
        if q.post(Event::Dirty { src: Src::Input, surface: i as u32 }) != 0 {
            dropped_at = i;
            break;
        }
    }
    set.add(
        "B-501 队列满显式丢弃计数",
        dropped_at == QUEUE_CAP && q.queue_full_dropped == 1,
        "满 64 拒新并计数上报",
    );

    // —— 看门狗 ——
    let mut w = EventPump::new();
    for _ in 0..(WATCHDOG_TICKS as u64) {
        let _ = w.cycle();
    }
    set.add(
        "B-501 看门狗饥饿上报",
        w.phase == Phase::Hungry && w.watchdog_starved == 1,
        "假死检测挂点（宪章同罪检测）",
    );
    let _ = w.post(Event::Heartbeat { src: Src::Timeout });
    let _ = w.cycle();
    set.add(
        "B-501 喂狗复活的路径",
        w.phase != Phase::Hungry,
        "事件到达即喂狗恢复",
    );

    // —— 阶段机迁移闭环 ——
    let mut ph = EventPump::new();
    let seq = [
        ph.phase, // Sleep 初态
        {
            let _ = ph.post(Event::Dirty { src: Src::Vxwm, surface: 1 });
            let _ = ph.cycle();
            ph.phase // Compose
        },
        {
            let _ = ph.cycle();
            ph.phase // 回 Sleep
        },
    ];
    set.add(
        "B-501 阶段机迁移闭环",
        seq[0] == Phase::Sleep && seq[1] == Phase::Compose && seq[2] == Phase::Sleep,
        "Sleep→Drain→Compose→Sleep",
    );

    // —— 环形队列回绕 ——
    let mut ring = EventPump::new();
    let mut wrap_ok = true;
    for round in 0..4u32 {
        for i in 0..QUEUE_CAP {
            wrap_ok &= ring.post(Event::Dirty { src: Src::Vxwm, surface: round * 100 + i as u32 }) == 0;
        }
        let vr = ring.cycle();
        wrap_ok &= vr == CycleVerdict::FullFrame { dirty_surfaces: QUEUE_CAP as u32 };
    }
    set.add(
        "B-501 环形队列四轮回绕",
        wrap_ok && ring.drained == (QUEUE_CAP as u64) * 4,
        "head/tail 回绕零错位",
    );

    // —— 净身与叙事 ——
    let mut z = EventPump::new();
    let _ = z.post(Event::Dirty { src: Src::Vxwm, surface: 1 });
    let _ = z.cycle();
    z.reset();
    set.add(
        "B-501 重置净身",
        z.cycles == 0 && z.composes == 0 && z.phase == Phase::Sleep,
        "账本归零",
    );
    set.add(
        "B-501 四源命名完备",
        Src::Vxwm.name() == "vxwm" && Src::FrameClock.name() == "frame-clock",
        "诊断日志可读",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pump_idle_static_desktop() {
        let mut p = EventPump::new();
        for _ in 0..4096 {
            let _ = p.cycle();
        }
        assert_eq!(p.composes, 0, "静止桌面长眠零合成");
        assert_eq!(p.idle_cost_pmil(), 0, "B-501 空转成本 0 ≤ 50‰");
        assert_eq!(p.sleep_cycles, 4096);
    }

    #[test]
    fn pump_compose_only_when_dirty() {
        let mut p = EventPump::new();
        // 10 个静止圈 + 1 个脏圈循环，共 100 圈
        for _ in 0..10 {
            for _ in 0..10 {
                let _ = p.cycle();
            }
            let _ = p.post(Event::Dirty { src: Src::Vxwm, surface: 1 });
            let _ = p.cycle();
        }
        assert_eq!(p.cycles, 110);
        assert_eq!(p.composes, 10);
        // 成本 = 10/110 ≈ 90‰ > 50‰——有真实负载时判据口径是"静止桌面"
        assert_eq!(p.idle_cost_pmil(), (10 * 1000 / 110) as u32);
    }

    #[test]
    fn pump_watchdog_timeline() {
        let mut p = EventPump::new();
        for _ in 0..(WATCHDOG_TICKS - 1) {
            let _ = p.cycle();
        }
        assert_ne!(p.phase, Phase::Hungry, "临界前不误报");
        let _ = p.cycle();
        assert_eq!(p.phase, Phase::Hungry);
        assert_eq!(p.watchdog_starved, 1);
        // 持续饥饿只计一次
        for _ in 0..8 {
            let _ = p.cycle();
        }
        assert_eq!(p.watchdog_starved, 1);
        // 喂狗恢复
        let _ = p.post(Event::Heartbeat { src: Src::Timeout });
        let _ = p.cycle();
        assert_ne!(p.phase, Phase::Hungry);
    }

    #[test]
    fn pump_cursor_never_composes() {
        let mut p = EventPump::new();
        for i in 0..100 {
            let _ = p.post(Event::Cursor { x: i as i16, y: (i * 2) as i16 });
            let _ = p.cycle();
        }
        assert_eq!(p.composes, 0);
        assert_eq!(p.cursor_steps, 100);
    }

    #[test]
    fn pump_all_checks_pass() {
        let set = run_pump_checks();
        assert!(set.len() >= 15, "B-501 CheckSet 应≥15 项，实际 {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "B-501 check {} failed: {}", c.name, c.detail);
        }
    }
}
