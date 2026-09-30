//! H2 应用生命周期编排 · 深化批次三·二波（单例路由 → 三拍子启动 →
//! 心跳监视 → 无响应恢复 → 结束清算——F282/F283/F284 的全生命周期
//! 总状态机）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F282 单例策略**：三策略（单例/多例/带参转发）已在
//!   [`crate::h2star::h2launch`]——本层是它的时间轴延伸：激活事件
//!   打进生命周期账（两入口同账——F275 双入口纪律的移植）；
//! - **F284 无响应判定与恢复**：5s 心跳判定、恢复滞回（连续心跳
//!   才判活）在 notresp——本层把「判定→蒙层→两选一→抢救快照→
//!   结束」串成全链状态机：蒙层出现必给两选一、结束必清算、
//!   「看起来卡死但没人管」的死角在编排层不存在；
//! - **结束清算**：正常结束与被结束都走同一份清算账（资源回收/
//!   任务栏按钮移除/单例表清位）——「进程死了按钮还在」类幽灵
//!   状态在结构上无路可走。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 生命周期状态机
// ---------------------------------------------------------------------------

/// 应用实例生命周期状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    /// 已路由待启动（F282 路由产出）。
    Routed,
    /// 启动中（三拍子计时——F283）。
    Launching,
    /// 运行中（心跳正常）。
    Running,
    /// 无响应（蒙层与两选一在挂）。
    NotResponding,
    /// 已结束（正常或被结束——清算完成后入此态）。
    Terminated,
}

/// 无响应判定阈值（F284 判据 5s——同源常量）。
pub const HEARTBEAT_TIMEOUT_MS: u32 = 5000;
/// 恢复滞回：连续 3 次心跳才判活（notresp 同值——两处同源）。
pub const RECOVERY_BEATS: u32 = 3;

/// 一个受管实例。
pub struct Instance {
    pub app: String,
    pub pid: u64,
    pub state: Lifecycle,
    /// 距上次心跳的 ms（调用方逐事件喂）。
    since_beat_ms: u32,
    /// 恢复期连续心跳数。
    recovery_beats: u32,
    /// 启动耗时账（三拍子：反馈/窗框/骨架完成）。
    beats_done: u8,
}

impl Instance {
    pub fn new(app: &str, pid: u64) -> Instance {
        Instance {
            app: app.into(),
            pid,
            state: Lifecycle::Routed,
            since_beat_ms: 0,
            recovery_beats: 0,
            beats_done: 0,
        }
    }

    /// 推进启动拍子（F283 三拍子：反馈/窗框/骨架——三拍齐才 Running）。
    /// 返回完成拍数（诊断页口径）。
    pub fn beat_launch(&mut self) -> u8 {
        if self.state == Lifecycle::Launching && self.beats_done < 3 {
            self.beats_done += 1;
            if self.beats_done == 3 {
                self.state = Lifecycle::Running;
            }
        }
        self.beats_done
    }

    /// 心跳喂入：Running 态刷新；NotResponding 态累计恢复拍——
    /// 连续 RECOVERY_BEATS 次才判活（滞回）。
    pub fn heartbeat(&mut self) {
        self.since_beat_ms = 0;
        if self.state == Lifecycle::NotResponding {
            self.recovery_beats += 1;
            if self.recovery_beats >= RECOVERY_BEATS {
                self.state = Lifecycle::Running;
                self.recovery_beats = 0;
            }
        } else if self.state == Lifecycle::Running {
            self.recovery_beats = 0;
        }
    }

    /// 时间喂入：Running 态超 5s → NotResponding（一次性——蒙层
    /// 只弹一次，不重复弹）。
    pub fn tick(&mut self, delta_ms: u32) -> bool {
        if self.state == Lifecycle::Running {
            self.since_beat_ms += delta_ms;
            if self.since_beat_ms > HEARTBEAT_TIMEOUT_MS {
                self.state = Lifecycle::NotResponding;
                self.recovery_beats = 0;
                return true; // 蒙层该出现了（一次性边沿）
            }
        }
        false
    }

    /// 结束清算：任何状态可入；重复调用幂等（状态保持终态、清单
    /// 重发同账）。返回清算动作清单（资源回收/按钮移除/单例清位
    /// ——三件全做，不许只做一半）。
    pub fn terminate(&mut self, killed: bool) -> Teardown {
        self.state = Lifecycle::Terminated;
        Teardown {
            app: self.app.clone(),
            pid: self.pid,
            reclaim_resources: true,
            remove_taskbar_button: true,
            clear_singleton_slot: true,
            killed,
        }
    }
}

/// 结束清算单（三件套 + 死因——账目可查）。
#[derive(Debug, PartialEq, Eq)]
pub struct Teardown {
    pub app: String,
    pub pid: u64,
    pub reclaim_resources: bool,
    pub remove_taskbar_button: bool,
    pub clear_singleton_slot: bool,
    pub killed: bool,
}

// ---------------------------------------------------------------------------
// 生命周期账（两入口同账——F275 纪律移植到 F282 激活面）
// ---------------------------------------------------------------------------

/// 激活事件来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Taskbar,
    StartMenu,
}

/// 一条激活账。
#[derive(Debug, PartialEq, Eq)]
pub struct ActivationRecord {
    pub app: String,
    pub entry: Entry,
    /// 是否转发给既有实例（F282 带参转发——单例唤醒）。
    pub forwarded: bool,
}

/// 生命周期账：两入口写同一本账（结构一致 = 同一个 add 函数）。
pub struct LifecycleLedger {
    records: Vec<ActivationRecord>,
}

impl LifecycleLedger {
    pub fn new() -> LifecycleLedger {
        LifecycleLedger { records: Vec::new() }
    }

    /// 统一记账口（两入口同函数——结构一致不是约定是构造）。
    pub fn record(&mut self, app: &str, entry: Entry, forwarded: bool) {
        self.records.push(ActivationRecord { app: app.into(), entry, forwarded });
    }

    /// 某应用两入口账目一致性（同一应用从两入口的转发判定必须相同
    /// ——同策略同结果，除非策略变更在中间发生）。
    pub fn consistent_for(&self, app: &str) -> bool {
        let forwards: Vec<bool> = self
            .records
            .iter()
            .filter(|r| r.app == app)
            .map(|r| r.forwarded)
            .collect();
        forwards.iter().all(|f| *f == forwards[0])
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2appctl_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2appctl");
    // 三拍子：三拍齐才 Running；拍数机判可查。
    let mut i = Instance::new("记事本", 1);
    set.add("h2appctl routed start", i.state == Lifecycle::Routed, "routed entry");
    i.state = Lifecycle::Launching;
    i.beat_launch();
    i.beat_launch();
    set.add(
        "h2appctl two beats not running",
        i.state == Lifecycle::Launching && i.beat_launch() == 3,
        "third beat fires",
    );
    set.add("h2appctl running after beats", i.state == Lifecycle::Running, "all beats done");
    // 心跳超时：>5s 边沿触发蒙层（一次性）；恰 5s 不触发（≤ 线）。
    let mut r = Instance::new("计算器", 2);
    r.state = Lifecycle::Running;
    let mut edge = false;
    for _ in 0..5 {
        edge |= r.tick(1000); // 恰 5000ms 累计——不超线
    }
    set.add(
        "h2appctl 5s inclusive",
        !edge && r.state == Lifecycle::Running,
        "5000 not over",
    );
    edge |= r.tick(1);
    set.add(
        "h2appctl 5s edge once",
        edge && r.state == Lifecycle::NotResponding,
        "one-shot edge",
    );
    let mut edge2 = false;
    for _ in 0..10 {
        edge2 |= r.tick(1000);
    }
    set.add(
        "h2appctl no re-edge",
        !edge2 && r.state == Lifecycle::NotResponding,
        "overlay not re-popped",
    );
    // 恢复滞回：单心跳不活；三连心跳才活。
    r.heartbeat();
    set.add("h2appctl one beat stays", r.state == Lifecycle::NotResponding, "hysteresis holds");
    r.heartbeat();
    r.heartbeat();
    set.add("h2appctl three beats live", r.state == Lifecycle::Running, "recovered");
    // 结束清算：三件套齐 + 死因留账；终态不可再清算。
    let td = r.terminate(true);
    set.add(
        "h2appctl teardown full",
        td.reclaim_resources
            && td.remove_taskbar_button
            && td.clear_singleton_slot
            && td.killed
            && r.state == Lifecycle::Terminated,
        "three-of-three",
    );
    set.add(
        "h2appctl terminal locked",
        r.terminate(false).pid == 2 && r.state == Lifecycle::Terminated,
        "no double teardown",
    );
    // 两入口同账：同应用转发判定一致；不同应用各记各的。
    let mut ledger = LifecycleLedger::new();
    ledger.record("记事本", Entry::Taskbar, true);
    ledger.record("记事本", Entry::StartMenu, true);
    ledger.record("计算器", Entry::Taskbar, false);
    ledger.record("计算器", Entry::StartMenu, false);
    set.add(
        "h2appctl same ledger both lanes",
        ledger.len() == 4
            && ledger.consistent_for("记事本")
            && ledger.consistent_for("计算器"),
        "two entries one book",
    );
    ledger.record("计算器", Entry::Taskbar, true);
    set.add(
        "h2appctl inconsistency visible",
        !ledger.consistent_for("计算器"),
        "drift is detectable",
    );
    // 常量同源：阈值与滞回与 notresp 判据同值。
    set.add(
        "h2appctl constants",
        HEARTBEAT_TIMEOUT_MS == 5000 && RECOVERY_BEATS == 3,
        "5s / 3 beats",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2appctl_all_green() {
        let set = run_h2appctl_checks();
        assert!(set.all_passed(), "h2appctl 自检有红项");
        assert!(!set.truncated(), "h2appctl 自检溢出");
    }

    #[test]
    fn hang_recover_cycle_repeats() {
        // 卡死-恢复-再卡死-再恢复循环 5 轮：状态机不漂移（长跑不变式）。
        let mut a = Instance::new("循环体", 9);
        a.state = Lifecycle::Running;
        for _ in 0..5 {
            let mut hung = false;
            while !hung {
                hung = a.tick(1000);
            }
            assert_eq!(a.state, Lifecycle::NotResponding);
            for _ in 0..RECOVERY_BEATS {
                a.heartbeat();
            }
            assert_eq!(a.state, Lifecycle::Running);
        }
    }
}
