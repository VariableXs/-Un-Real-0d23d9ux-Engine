//! CGPU-F0164 · 跨帧依赖与围栏传播（CGPU-B 域 · 批次 B01 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0164`
//!
//! **判据（锚点原文）**：跨帧等待正确、链式传播、超时降级、持久化正确、
//! 跨 3 帧用例。
//!
//! **职责定位（锚点原文）**：任务跨帧引用的处理：上一帧的解码任务在下一
//! 帧使用——**跨帧边**（上帧任务围栏→本帧任务等待）、**围栏传播链**
//! （A 帧依赖 B 帧，B 依赖 C——链式传播）、**跨帧超时**（上帧任务 N 帧
//! 未完成→标记丢失→本帧走降级）。**图持久化**（跨帧边不随帧图释放）。
//!
//! # 一、围栏为什么是「账本」而不是「帧图的边」
//!
//! [`crate::svstar2::vcb01_framegraph::FrameGraph`] 每帧一建一放——把跨帧
//! 边画进帧图，帧一结束边就跟着图蒸发，下一帧的消费者拿着一个永远等不到
//! 的围栏。故跨帧依赖住进**独立账本**（[`CrossFrameLedger`]）：账本不持有
//! 任何帧图引用（类型上杜绝「随图释放」），条目按任务键
//! （帧号+节点号）寻址，帧图没了、围栏还在——「跨帧边不随帧图释放」由
//! 所有权结构直接保证，不靠纪律自觉。
//!
//! # 二、围栏三态与「丢失是显性标记不是沉默超时」
//!
//! 围栏三态封闭：`Pending`（上帧任务未完成，本帧消费者**阻塞等待**）/
//! `Fulfilled`（完成，放行）/ `Lost`（超时**标记丢失**，消费者**走降级**）。
//! 关键纪律在 Lost：超时不静默——上帧任务 N 帧未完成，账本把它显性标记
//! 为 Lost 并记账，消费方拿到「丢失」这个确定事实去走降级路径（低质替
//! 代），而不是对着一个永远 Pending 的围栏挂死。丢的帧要能被看见：丢失
//! 与降级都有计数（遥测入账）。
//!
//! # 三、链式传播：一个 Lost 沿依赖链全量下推
//!
//! A 帧依赖 B 帧、B 依赖 C 帧——A 丢失时 B 的等待永远不满足，C 同理。
//! 逐帧轮询各自超时是浪费且时序漂移：标记 Lost 时**沿等待链一次性下推**
//! （BFS），链上每个下游围栏同步 Lost，每个因此转降级的消费者计一笔。
//! 传播是「上帧失败→本帧降级」语义的批量兑现——不传播的账本会让下游消
//! 费者各自等满超时才降级，损失的是整帧的确定性时序。
//!
//! # 四、与相邻条的分工
//!
//! F0161 的图、F0163 的波次都在**单帧内**；本条只管**帧间**——跨帧边登
//! 记、围栏状态机、超时与传播。本帧发射器消费 [`upstream_state`]：Clear
//! 放行、Blocked 继续等、Degraded 走降级算子。单帧内的环归 F0163 管，
//! 跨帧账本里自指的等待边在登记闸拒绝（等待关系必须严格跨帧向前）。
//!
//! **超时语义**：`deadline = born + N`（[`CROSSFRAME_TIMEOUT_FRAMES`]），
//! `now >= deadline` 且仍 Pending 即 Lost——恰 N-1 帧仍在等、恰 N 帧判丢
//! （边界双向判据钉死，防 >⇄>= 变异）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const CROSSFRAME_VERSION: &str = "CB04-crossframe-v1";

/// 跨帧超时窗（上帧任务 N 帧未完成→标记丢失——锚点原文的 N）。
pub const CROSSFRAME_TIMEOUT_FRAMES: u64 = 4;

/// 围栏键未知（resolve/查询的键从未登记）。
pub const E_CROSSFRAME_UNKNOWN: &str = "E_CROSSFRAME_UNKNOWN";

/// 重复登记（同对跨帧边重复 attach——等待关系一条边记一次）。
pub const E_CROSSFRAME_DUP: &str = "E_CROSSFRAME_DUP";

/// 非法跨帧边（等待边不向前——同帧自等或倒退等待，链上必然死锁）。
pub const E_CROSSFRAME_BACKWARD: &str = "E_CROSSFRAME_BACKWARD";

// ---------------------------------------------------------------------------
// 二、任务键与围栏三态
// ---------------------------------------------------------------------------

/// 跨帧任务键（帧号+帧内节点句柄——跨帧边两端的寻址单位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskKey {
    /// 任务出生帧号。
    pub frame: u64,
    /// 帧内节点句柄（F0161 arena 下标）。
    pub node: u32,
}

impl TaskKey {
    /// 键是否严格晚于另一键（等待边必须向前——同帧/倒退即死锁）。
    pub fn is_after(self, other: TaskKey) -> bool {
        self.frame > other.frame || (self.frame == other.frame && self.node > other.node)
    }

    /// 键读屏短码。
    pub fn tag(self) -> String {
        format!("f{}n{}", self.frame, self.node)
    }
}

/// 围栏三态（封闭集——加态必须改发射闸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceState {
    /// 上帧任务未完成——本帧消费者阻塞等待。
    Pending,
    /// 上帧任务完成——放行。
    Fulfilled,
    /// 超时丢失——消费者走降级算子（显性标记非沉默）。
    Lost,
}

impl FenceState {
    /// 态短码。
    pub const fn tag(self) -> &'static str {
        match self {
            FenceState::Pending => "pending",
            FenceState::Fulfilled => "fulfilled",
            FenceState::Lost => "lost",
        }
    }

    /// 态中文名（读屏可达）。
    pub const fn zh(self) -> &'static str {
        match self {
            FenceState::Pending => "等待",
            FenceState::Fulfilled => "完成",
            FenceState::Lost => "丢失",
        }
    }
}

/// 围栏条目（一个上帧任务 + 谁在等它）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FenceEntry {
    /// 围栏主体（上帧任务键）。
    pub key: TaskKey,
    /// 登记帧号（超时窗起点）。
    pub born_frame: u64,
    /// 超时限（born + N）。
    pub deadline: u64,
    /// 当前态。
    pub state: FenceState,
    /// 等待者集（本帧/后帧任务键——传播链的下游方向）。
    pub waiters: Vec<TaskKey>,
}

// ---------------------------------------------------------------------------
// 三、跨帧账本（图持久化——不随帧图释放）
// ---------------------------------------------------------------------------

/// 跨帧围栏账本。
///
/// **持久化纪律的结构保证**：账本只持有键与状态，不持有任何
/// `FrameGraph`——帧图每帧释放，账本跨帧存活。所有变更方法记账显性
/// （完成/丢失/降级三计数），无静默路径。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CrossFrameLedger {
    /// 围栏条目集（按登记序——查询线性扫，账本规模=活跃跨帧任务数）。
    pub entries: Vec<FenceEntry>,
    /// 累计完成数（resolve 计账）。
    pub fulfilled_events: u32,
    /// 累计丢失数（mark_lost/age_out 计账——每条链的源头一次）。
    pub lost_events: u32,
    /// 累计降级消费者数（因 Lost 走降级的 waiter 计账）。
    pub degraded_waiters: u32,
}

impl CrossFrameLedger {
    /// 空账本。
    pub fn new() -> Self {
        Self::default()
    }

    /// 按键取条目下标。
    fn index_of(&self, key: TaskKey) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }

/// 登记跨帧边：`waiter`（本帧任务）等待 `producer`（**更早帧**的任务）。
///
/// - producer 无条目则先建（Pending，超时窗从 now 起算——账本首次
///   见到它的时刻即等它的人开始等的时刻）；
/// - 重复同对边拒绝（[`E_CROSSFRAME_DUP`]）；
/// - 帧号不严格向前拒绝（[`E_CROSSFRAME_BACKWARD`]——跨帧账本只收跨帧
///   边，同帧等待归帧内图 F0161 管；倒退等待在链上必然死锁）。
pub fn attach(
    &mut self,
    producer: TaskKey,
    waiter: TaskKey,
    now: u64,
) -> Result<(), &'static str> {
    if waiter.frame <= producer.frame {
        return Err(E_CROSSFRAME_BACKWARD);
    }
    if self.index_of(producer).is_none() {
        self.entries.push(FenceEntry {
            key: producer,
            born_frame: now,
            deadline: now + CROSSFRAME_TIMEOUT_FRAMES,
            state: FenceState::Pending,
            waiters: vec![],
        });
    }
    let idx = match self.index_of(producer) {
        Some(i) => i,
        None => return Err(E_CROSSFRAME_UNKNOWN),
    };
    if self.entries[idx].waiters.contains(&waiter) {
        return Err(E_CROSSFRAME_DUP);
    }
    self.entries[idx].waiters.push(waiter);
    // 等待者自身若无条目也无妨——它只在别人条目的 waiters 里占位；
    // 但若它之后也被别人等待，attach 会为它建条目，形成链。
    Ok(())
}

    /// 上帧任务完成通知：Pending→Fulfilled（终态不可逆），返回放行的
    /// 直接 waiter 数。未知键拒。
    pub fn resolve(&mut self, key: TaskKey) -> Result<usize, &'static str> {
        let idx = self.index_of(key).ok_or(E_CROSSFRAME_UNKNOWN)?;
        if self.entries[idx].state != FenceState::Pending {
            // 终态幂等：Fulfilled/Lost 后再 resolve 不改账不重计。
            return Ok(0);
        }
        self.entries[idx].state = FenceState::Fulfilled;
        self.fulfilled_events += 1;
        Ok(self.entries[idx].waiters.len())
    }

    /// 标记丢失并沿等待链**一次性下推**（链式传播——锚点原文）。
    ///
    /// 返回传播长度（直接 waiter + 链上全部下游）。链上每个转 Lost 的
    /// 围栏其 waiter 各计一笔降级（源头键本身不计降级——它不是 waiter）。
    /// 非 Pending 键标记丢失幂等（已 Fulfilled 不翻脸，已 Lost 不重计）。
    pub fn mark_lost(&mut self, key: TaskKey) -> Result<usize, &'static str> {
        let idx = self.index_of(key).ok_or(E_CROSSFRAME_UNKNOWN)?;
        if self.entries[idx].state != FenceState::Pending {
            return Ok(0);
        }
        self.entries[idx].state = FenceState::Lost;
        self.lost_events += 1;
        // BFS 沿 waiters 下推。
        let mut queue: Vec<TaskKey> = self.entries[idx].waiters.clone();
        let mut propagated = 0usize;
        let mut qi = 0usize;
        while qi < queue.len() {
            let cur = queue[qi];
            qi += 1;
            let cidx = match self.index_of(cur) {
                Some(i) => i,
                None => {
                    // 等待者自身没有条目（叶子消费者）——只计降级。
                    self.degraded_waiters += 1;
                    propagated += 1;
                    continue;
                }
            };
            if self.entries[cidx].state == FenceState::Pending {
                self.entries[cidx].state = FenceState::Lost;
                self.degraded_waiters += 1;
                propagated += 1;
                for &w in self.entries[cidx].waiters.iter() {
                    queue.push(w);
                }
            } else {
                // 已 Fulfilled 的下游不受上游丢失影响（数据已到手）；
                // 已 Lost 的不重计。仍计入传播长度？——只计真实状态转移。
            }
        }
        Ok(propagated)
    }

    /// 超时清账：所有 `now >= deadline` 且仍 Pending 的围栏标记丢失
    /// （逐个走 mark_lost——链式传播与计数复用同一路径）。
    /// 返回本轮判丢的源头数。
    pub fn age_out(&mut self, now: u64) -> usize {
        let due: Vec<TaskKey> = self
            .entries
            .iter()
            .filter(|e| e.state == FenceState::Pending && now >= e.deadline)
            .map(|e| e.key)
            .collect();
        let mut n = 0usize;
        for k in due.iter() {
            if self.mark_lost(*k).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// 围栏当前态（未知键 None）。
    pub fn state_of(&self, key: TaskKey) -> Option<FenceState> {
        self.index_of(key).map(|i| self.entries[i].state)
    }

    /// 上帧围栏对某消费者的就绪判定（本帧发射闸的查询面）。
    ///
    /// 返回三态：
    /// - `Upstream::Clear`：等待的全部上游都 Fulfilled（或无跨帧上游）
    ///   ——放行；
    /// - `Upstream::Blocked`：存在 Pending 上游——继续等；
    /// - `Upstream::Degraded`：存在 Lost 上游——本帧走降级算子
    ///   （Degraded 优先于 Blocked：等待已无意义）。
    pub fn upstream_state(&self, waiter: TaskKey) -> Upstream {
        let mut blocked = false;
        for e in self.entries.iter() {
            if !e.waiters.contains(&waiter) {
                continue;
            }
            match e.state {
                FenceState::Lost => return Upstream::Degraded,
                FenceState::Pending => blocked = true,
                FenceState::Fulfilled => {}
            }
        }
        if blocked {
            Upstream::Blocked
        } else {
            Upstream::Clear
        }
    }

    /// 账本活跃条目数（Pending 计入——持久化对账用）。
    pub fn live_count(&self) -> usize {
        self.entries.len()
    }

    /// 账本读屏单行（条目/完成/丢失/降级四计数——无资源明细）。
    pub fn screen_line(&self) -> String {
        format!(
            "跨帧账本：{} 围栏，完成 {}，丢失 {}，降级 {}",
            self.entries.len(),
            self.fulfilled_events,
            self.lost_events,
            self.degraded_waiters
        )
    }
}

/// 上游就绪三态（本帧发射闸的判定结果）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upstream {
    /// 全部上游完成（或无跨帧上游）——放行。
    Clear,
    /// 存在等待中的上游——本任务暂不发射。
    Blocked,
    /// 存在丢失的上游——走降级算子（优先于 Blocked）。
    Degraded,
}

impl Upstream {
    /// 判定短码。
    pub const fn tag(self) -> &'static str {
        match self {
            Upstream::Clear => "clear",
            Upstream::Blocked => "blocked",
            Upstream::Degraded => "degraded",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、跨帧用例驱动（跨 3 帧完整闭环——锚点判据的用例形态）
// ---------------------------------------------------------------------------

/// 跨 3 帧用例：frame1 解码任务 d，frame2、frame3 消费者逐级等待。
///
/// 演示全流程：登记跨帧边（f1→f2、f2→f3）→ 生产完成逐级 resolve →
/// 尾帧 Clear 放行。账本自始至终不持有帧图——图随便放，围栏还在。
pub fn three_frame_happy_path() -> CrossFrameLedger {
    let d1 = TaskKey { frame: 1, node: 7 };
    let c2 = TaskKey { frame: 2, node: 3 };
    let c3 = TaskKey { frame: 3, node: 9 };
    let mut led = CrossFrameLedger::new();
    // frame2 登记：等 frame1 的解码任务。
    let _ = led.attach(d1, c2, 2);
    // frame3 登记：等 frame2 的消费者（链式——f2 围栏由 f1 决定）。
    let _ = led.attach(c2, c3, 3);
    // frame1 结束：解码完成（d1 放行直接 waiter c2）。
    let _ = led.resolve(d1);
    // frame2 结束：c2 的生产完成（c2 放行 waiter c3——链上最后一级）。
    let _ = led.resolve(c2);
    led
}

/// 跨 3 帧丢失用例：源头超时，链式传播让尾帧走降级。
pub fn three_frame_lost_path() -> CrossFrameLedger {
    let d1 = TaskKey { frame: 1, node: 7 };
    let c2 = TaskKey { frame: 2, node: 3 };
    let c3 = TaskKey { frame: 3, node: 9 };
    let mut led = CrossFrameLedger::new();
    let _ = led.attach(d1, c2, 2);
    let _ = led.attach(c2, c3, 3);
    // 4 帧过去解码没完成——超时清账，链式 Lost 下推到 c3。
    let _ = led.age_out(2 + CROSSFRAME_TIMEOUT_FRAMES);
    led
}
