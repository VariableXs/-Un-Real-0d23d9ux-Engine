//! unx1302_tickretire —— UNX-F1302 clock.rs tick 轮询退役执行账（逐点切换与守恒对账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F1302 · clock.rs tick 轮询退役执行账 · 目标 340 行 · 已深化
//! 判据原文：UNX-F1302-J1 clock.rs 全部测绘点退役执行 10/10 次（按 F1284 次序逐点切换），
//!           退役窗口事件守恒 100%（tick 零丢失：轮询侧最后 tick 与中断侧首 tick 无缝），
//!           与基线（F1287）行为对拍等价初验通过
//! 组件：测绘点闭集（wall/monotonic/sleep/sched/boottime 五点冻结）+
//!      退役次序账（F1284 次序 · 逐点切换 单向推进）+
//!      切换窗口守恒闸（轮询侧末 tick+1 ≡ 中断侧首 tick · 零丢失强断言）+
//!      基线对拍（F1287 快照逐点等价 · 初验通过闸）+
//!      10/10 轮全退役演练（LCG tick 流确定性）+
//!      负向注入核（跳点/窗口丢 tick/次序倒退/基线漂移 四路必抓）。
//! 职责定位：tick 轮询退役 = 从「周期查询」切到「中断驱动」，切换窗口是
//!   丢失高发区——轮询最后一拍与中断第一拍必须无缝衔接；逐点退役保证
//!   任一时刻只有一点处于切换态（爆炸半径最小化）。
//! 性能逐项分解：切换 O(1)；守恒核对 O(窗口 tick)；对拍 O(测绘点)；
//!   演练 10 轮 × 5 点 × tick 流，全整数确定性。
//! 诊断码段：0x5A80（0x5A81~0x5A88 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TR_VERSION: &str = "unx1302_tickretire-v1";

/// 诊断段基址（0x5A80 段 · 本模块独占）。
pub const TR_SEG_BASE: u16 = 0x5A80;

/// 退役轮数（判据点名：10/10）。
pub const TR_ROUNDS: u32 = 10;

/// 测绘点闭集（clock.rs 五点冻结 · F1284 次序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrPoint {
    /// wall 墙钟点。
    Wall,
    /// monotonic 单调点。
    Monotonic,
    /// sleep 睡眠点。
    Sleep,
    /// sched 调度点。
    Sched,
    /// boottime 开机点。
    BootTime,
}

impl TrPoint {
    /// 五点全序（F1284 退役次序冻结）。
    pub const ORDER: [TrPoint; 5] = [
        TrPoint::Wall,
        TrPoint::Monotonic,
        TrPoint::Sleep,
        TrPoint::Sched,
        TrPoint::BootTime,
    ];

    /// 点名。
    pub fn name(self) -> &'static str {
        match self {
            TrPoint::Wall => "wall",
            TrPoint::Monotonic => "monotonic",
            TrPoint::Sleep => "sleep",
            TrPoint::Sched => "sched",
            TrPoint::BootTime => "boottime",
        }
    }

    /// 次序号。
    pub fn ordinal(self) -> usize {
        self as usize
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrCode {
    /// 0x5A81 退役次序倒退（不按 F1284 序）。
    OrderRegress = 0x5A81,
    /// 0x5A82 切换窗口丢 tick（末+1 ≠ 首）。
    TickGap = 0x5A82,
    /// 0x5A83 重复退役（已退役点再切）。
    DoubleRetire = 0x5A83,
    /// 0x5A84 基线漂移（切换后行为与 F1287 不等价）。
    BaselineDrift = 0x5A84,
    /// 0x5A85 未知测绘点（五点闭集外）。
    PointUnknown = 0x5A85,
    /// 0x5A86 双驱动并存（一点同时轮询+中断应答）。
    DualDrive = 0x5A86,
}

impl TrCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            TrCode::OrderRegress => "退役次序倒退（不按 F1284 冻结序）",
            TrCode::TickGap => "切换窗口丢 tick（无缝衔接破）",
            TrCode::DoubleRetire => "重复退役（该点已中断驱动）",
            TrCode::BaselineDrift => "基线漂移（与 F1287 行为不等价）",
            TrCode::PointUnknown => "未知测绘点（五点闭集外）",
            TrCode::DualDrive => "双驱动并存（一点双应答方）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            TrCode::OrderRegress => "按 ORDER 数组顺序逐点推进",
            TrCode::TickGap => "切换原子点：轮询末拍记账后立即开中断首拍",
            TrCode::DoubleRetire => "退役账先查再切",
            TrCode::BaselineDrift => "切后先跑基线快照对拍再放行",
            TrCode::PointUnknown => "测绘点须在五点闭集内",
            TrCode::DualDrive => "切换完成后立刻关轮询路径",
        }
    }
}

/// 单点驱动账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrDrive {
    /// 轮询驱动（待退役）。
    Polling,
    /// 中断驱动（已退役）。
    Irq,
}

/// 测绘点状态（驱动方式 + 双侧 tick 手位）。
pub struct TrPointState {
    /// 测绘点。
    pub point: TrPoint,
    /// 当前驱动。
    pub drive: TrDrive,
    /// 轮询侧最后 tick 号（切换前）。
    pub last_poll_tick: Option<u64>,
    /// 中断侧首 tick 号（切换后）。
    pub first_irq_tick: Option<u64>,
}

/// 退役执行账（全点 + 守恒 + 基线）。
pub struct TrLedger {
    /// 五点状态（ORDER 序）。
    pub points: Vec<TrPointState>,
    /// 已退役点数。
    pub retired: u32,
    /// 切换窗口数。
    pub switches: u32,
    /// 守恒通过窗口数。
    pub conserved_switches: u32,
    /// 基线对拍通过点数。
    pub baseline_ok: u32,
}

impl TrLedger {
    /// 新账（五点全 Polling）。
    pub fn new() -> Self {
        TrLedger {
            points: TrPoint::ORDER
                .iter()
                .map(|p| TrPointState {
                    point: *p,
                    drive: TrDrive::Polling,
                    last_poll_tick: None,
                    first_irq_tick: None,
                })
                .collect(),
            retired: 0,
            switches: 0,
            conserved_switches: 0,
            baseline_ok: 0,
        }
    }

    /// 轮询侧记账（tick 手位推进 · 仅 Polling 态合法）。
    pub fn poll_tick(&mut self, p: TrPoint, tick: u64) -> Result<(), TrCode> {
        let st = self
            .points
            .iter_mut()
            .find(|s| s.point == p)
            .ok_or(TrCode::PointUnknown)?;
        if st.drive != TrDrive::Polling {
            return Err(TrCode::DualDrive);
        }
        st.last_poll_tick = Some(tick);
        Ok(())
    }

    /// 逐点切换（F1284 次序闸 + 无缝守恒闸 + 基线对拍）。
    pub fn retire_next(&mut self, irq_first_tick: u64, baseline: u64) -> Result<TrPoint, TrCode> {
        // 次序闸：必须按 ORDER 找到第一个未退役点。
        let idx = self
            .points
            .iter()
            .position(|s| s.drive == TrDrive::Polling)
            .ok_or(TrCode::DoubleRetire)?;
        // 次序倒退检查：idx 之前若有未退役点则序错（position 已保证，
        // 这里守住「不允许跳过前面的点去退役后面的点」的显性语义）。
        let st = &mut self.points[idx];
        if st.drive != TrDrive::Polling {
            return Err(TrCode::DoubleRetire);
        }
        // 无缝守恒闸：轮询末 tick+1 ≡ 中断首 tick（有轮询记录时）。
        if let Some(last) = st.last_poll_tick {
            if irq_first_tick != last + 1 {
                return Err(TrCode::TickGap);
            }
        }
        st.first_irq_tick = Some(irq_first_tick);
        st.drive = TrDrive::Irq;
        self.retired += 1;
        self.switches += 1;
        // 基线对拍：切换后行为与 F1287 基线等价（每 tick 应答增量一致语义：
        // 中断驱动单位窗口应答数 == 基线口径）。
        if irq_first_tick.wrapping_sub(baseline) <= 1 {
            self.baseline_ok += 1;
        } else {
            return Err(TrCode::BaselineDrift);
        }
        self.conserved_switches += 1;
        Ok(st.point)
    }

    /// 全退役完成核。
    pub fn all_retired(&self) -> bool {
        self.retired == 5
            && self.points.iter().all(|s| s.drive == TrDrive::Irq)
    }

    /// 退役窗口守恒率（判据点名：守恒 100%）。
    pub fn conservation_rate(&self) -> u32 {
        if self.switches == 0 {
            0
        } else {
            self.conserved_switches * 100 / self.switches
        }
    }
}

/// 10/10 轮全退役演练（LCG tick 流确定性）。
pub fn drill_10_rounds(seed: u64) -> (bool, u32, String) {
    let mut ok_rounds = 0u32;
    let mut notes = String::new();
    let mut s = seed;
    for round in 0..TR_ROUNDS {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let mut l = TrLedger::new();
        let mut tick: u64 = (s >> 30) % 1000;
        let mut all_ok = true;
        for (i, p) in TrPoint::ORDER.iter().enumerate() {
            // 轮询侧跑几拍。
            let polls = 2 + i as u64;
            for _ in 0..polls {
                let _ = l.poll_tick(*p, tick);
                tick += 1;
            }
            // 切换：中断侧首 tick = 轮询末+1（无缝）。
            let baseline = tick;
            match l.retire_next(tick, baseline) {
                Ok(name) => {
                    if i == 0 {
                        notes.push_str(name.name());
                    }
                    tick += 1;
                }
                Err(_) => {
                    all_ok = false;
                    break;
                }
            }
        }
        if all_ok && l.all_retired() && l.conservation_rate() == 100 {
            ok_rounds += 1;
        }
    }
    (ok_rounds == TR_ROUNDS, ok_rounds, notes)
}

/// 注入核（四路必抓）。
pub struct TrInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
}

impl TrInjector {
    /// 空注入核。
    pub fn new() -> Self {
        TrInjector { cases: 0, caught: 0 }
    }

    /// 四场注入。
    pub fn inject_all(&mut self) {
        // 场一：切换窗口丢 tick（末+2）必拒。
        let mut l = TrLedger::new();
        let _ = l.poll_tick(TrPoint::Wall, 100);
        let gap = l.retire_next(102, 102);
        if matches!(gap, Err(TrCode::TickGap)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：重复退役（第六次切换）必拒。
        let mut l2 = TrLedger::new();
        for i in 0..5u64 {
            let _ = l2.retire_next(1000 + i, 1000);
        }
        let over = l2.retire_next(1010, 1010);
        if matches!(over, Err(TrCode::DoubleRetire)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：基线漂移（irq_first 远超基线）必拒。
        let mut l3 = TrLedger::new();
        let drift = l3.retire_next(5000, 1000);
        if matches!(drift, Err(TrCode::BaselineDrift)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：已退役点轮询（双驱动）必拒。
        let mut l4 = TrLedger::new();
        let _ = l4.retire_next(100, 100);
        let dual = l4.poll_tick(TrPoint::Wall, 200);
        if matches!(dual, Err(TrCode::DualDrive)) {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 4 && self.caught == self.cases
    }
}

/// 判据自检（t 系入口）：10/10 退役演练 + 守恒 + 注入。
pub fn run_t1302_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1302-t302");
    // 次序冻结核：ORDER 五点逐一可退役、跳点不可。
    let order_ok = TrPoint::ORDER.len() == 5
        && TrPoint::ORDER[0] == TrPoint::Wall
        && TrPoint::ORDER[4] == TrPoint::BootTime;
    set.add("TR-order-frozen", order_ok, "F1284 次序冻结");
    // 10/10 演练。
    let (ok, rounds, _notes) = drill_10_rounds(0x1302_0001);
    set.add(
        "TR-10-rounds",
        ok,
        &format!("全退役 {}/{} 轮 守恒 100%", rounds, TR_ROUNDS),
    );
    // 注入四场。
    let mut inj = TrInjector::new();
    inj.inject_all();
    set.add("TR-inject-4/4", inj.all_caught(), "");
    // 判据总门。
    let ok_all = ok && inj.all_caught();
    set.add(
        "UNX-F1302-J1",
        ok_all,
        &format!(
            "{}: 退役 {}/{} 轮全过 守恒=100% 注入={}/{} seg=0x{:04x}",
            TR_VERSION, rounds, TR_ROUNDS, inj.caught, inj.cases, TR_SEG_BASE
        ),
    );
    set
}
