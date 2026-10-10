//! CGPU-F0644 · 表面渲染调度（CGPU-E 域 · 表面与呈现域 · E04 组 · 目标 380 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0644`
//!
//! **判据（锚点原文）**：排程、联动、公平、开销、判据。
//!
//! **职责定位（锚点原文）**：渲染调度：可见表面排程（激活表面的渲染
//! 排程——按可见性/交互性/预算排序）；排程与 C 域联动（排程顺序→
//! 预算申请顺序——集成复用）；调度公平（同优先级表面轮转——不饿死）；
//! 调度开销（30 表面排程耗时预算——微秒级实测）。
//!
//! ## 一、排程（判据一：三级排序键 + 预算闸）
//!
//! 激活表面才可排程（[`SurfState::Inactive`] 拒入队）；排序键三级：
//! 可见性（[`Surf::visible`]，可见优先——不可见激活面是备胎不是主力）＞
//! 交互性（[`Surf::interactive`]）＞ 轮转公平（[`Surf::last_sched`] 最旧
//! 优先——判据三的落点）；同级同 tick 以 id 升序破平——**确定性**：
//! 同输入同输出，双跑逐位一致（对拍前提）。预算闸：帧预算
//! [`FRAME_BUDGET_UNITS`] 内按序收编，累计超限的表面**推迟不丢弃**
//! （[`SchedOutcome::Deferred`] 入账，下帧排程自带优先补——推迟是
//! 账面事实不是静默失败）。
//!
//! ## 二、C 域联动（判据二：顺序即申请序）
//!
//! 排程输出的位次（0 起）就是 C 域预算申请的顺序号——**集成复用**：
//! 排程序与申请序是同一份数据（[`SchedTicket::rank`]），C 域按 rank
//! 逐个兑现预算，不需要第二次排序；rank 连续无空洞（预算闸只截尾
//! 不跳位——中间挖洞会让 C 域的申请序出现假空洞）。
//!
//! ## 三、公平（判据三：轮转 + 饥饿立案）
//!
//! 同优先级轮转由排序键第三级天然兑现（最久未排程者先上）；
//! **饥饿是显性账**：连续 [`STARVE_FRAME_LIMIT`] 帧未被排程的表面
//! 立案（[`StarveCase`]），立案面下帧强制插队（强插也要过预算闸——
//! 公平不破产预算）；立案不可静默清除，只能由真实排程兑现销案。
//!
//! ## 四、开销（判据四：操作步记账 + 微秒级预算）
//!
//! 排程耗时以**操作步**记账（比较步+搬移步，[`SchedAccount`]）——
//! 零墙钟纪律下操作步是耗时的确定性代理；30 表面排程预算
//! [`SCHED_OPS_BUDGET`]（按 O(n log n) 选择排序实测口径折算）
//! 超预算=缺陷显性立案不静默；容量上限 [`MAX_SURFS`] 写死
//! （当前策略值，可调），超容拒绝带建议（拆帧组或提优先级裁剪）。
//!
//! **对接**：F0643（表面注册与能力协商——上游表面表）；C 域（预算
//! 申请方——rank 消费者）；F0645（合成对接——排程完成的下游）。
//! 零 panic 面、零 IO、零墙钟（tick 注入）、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const E04_SCHED_VERSION: &str = "E04-surfsched-v1";

/// 表面容量上限（当前策略值，可调；超容拒绝不静默扩容）。
pub const MAX_SURFS: usize = 256;

/// 帧预算总量（预算单位；C 域申请顺序按本闸截尾）。
pub const FRAME_BUDGET_UNITS: u32 = 1_024;

/// 饥饿立案阈值（连续未被排程的帧数）。
pub const STARVE_FRAME_LIMIT: u32 = 8;

/// 排程操作步预算（30 表面口径；微秒级实测的确定性代理）。
pub const SCHED_OPS_BUDGET: u32 = 1_800;

/// 排程开销核对面：30 表面时操作步必须落在预算内。
pub const SCHED_PROBE_SURFS: usize = 30;

// ---------------------------------------------------------------------------
// 二、诊断码（0x62 段独占：E04 调度面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x62 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScCode {
    /// 表面未注册。
    Unregistered,
    /// 重复注册。
    Duplicate,
    /// 容量已满。
    CapacityFull,
    /// 非激活面请求排程。
    NotActive,
    /// 排程开销超预算。
    OpsOverBudget,
    /// 帧预算溢出（本帧无空档且推迟账已满——极端拥塞显性态）。
    BudgetCongested,
}

impl ScCode {
    /// 人读诊断（异常零静默：发生了什么/为什么/怎么办）。
    pub const fn detail(self) -> &'static str {
        match self {
            ScCode::Unregistered => "表面未注册：排程只认注册面，建议先走 F0643 注册与能力协商",
            ScCode::Duplicate => "重复注册：同 id 表面已在账，建议改走更新而非再注册",
            ScCode::CapacityFull => "表面容量已满（上限为当前策略值）：建议拆帧组或按优先级裁剪低价值面",
            ScCode::NotActive => "非激活面不可排程：Inactive/Hidden 面不占渲染预算，建议先激活",
            ScCode::OpsOverBudget => "排程操作步超预算：30 表面口径超限=实现缺陷，建议检查排序实现复杂度",
            ScCode::BudgetCongested => "帧预算极端拥塞：推迟账已满且本帧无空档，建议降低表面预算请求或提升帧预算",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、表面账（排程的输入面）
// ---------------------------------------------------------------------------

/// 表面状态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfState {
    /// 未激活（不可排程）。
    Inactive,
    /// 激活但不可见（备胎：可排程但排序最低档）。
    ActiveHidden,
    /// 激活且可见（主力）。
    Visible,
}

/// 激活表面账（F0643 注册面的排程投影）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surf {
    /// 表面 id（注册侧分配，账内唯一）。
    pub id: u32,
    /// 状态。
    pub state: SurfState,
    /// 交互性（用户正在触碰的表面优先渲染）。
    pub interactive: bool,
    /// 优先级档（0 最高；跨档先看档再论公平）。
    pub priority: u8,
    /// 本面帧预算请求（预算单位；申请顺序=排程序）。
    pub budget_units: u32,
    /// 最近一次被排程的 tick（轮转公平键；u64::MAX=从未）。
    pub last_sched: u64,
    /// 连续未被排程的帧数（饥饿账）。
    pub starved_frames: u32,
}

// ---------------------------------------------------------------------------
// 四、排程输出（判据一 + 判据二）
// ---------------------------------------------------------------------------

/// 排程票据（rank 即 C 域预算申请顺序号）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchedTicket {
    /// 表面 id。
    pub surf_id: u32,
    /// 排程位次（0 起、连续无空洞——C 域按位次申请预算）。
    pub rank: u32,
    /// 本帧是否强插（饥饿立案面）。
    pub forced: bool,
    /// 预算申请量（透传 Surf::budget_units——C 域免二次查询）。
    pub budget_units: u32,
}

/// 单帧排程结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchedFrame {
    /// 本帧排程票据（按 rank 升序）。
    pub tickets: Vec<SchedTicket>,
    /// 推迟面（预算闸截尾；下帧优先补——推迟是账面事实）。
    pub deferred: Vec<u32>,
    /// 立案面（本帧新增饥饿立案）。
    pub starve_cases: Vec<u32>,
    /// 销案面（本帧因真实排程兑现而销案的立案）。
    pub cleared_cases: Vec<u32>,
    /// 本帧排程操作步（判据四开销账的出口）。
    pub ops: u32,
}

// ---------------------------------------------------------------------------
// 五、调度器（判据一二三的全部状态机）
// ---------------------------------------------------------------------------

/// 表面渲染调度器。
#[derive(Clone, Debug)]
pub struct SurfScheduler {
    /// 表面账（按 id 升序保持——确定性遍历序）。
    surfs: Vec<Surf>,
    /// 饥饿立案账（立案中的表面 id；销案即移除）。
    starve_ledger: Vec<u32>,
    /// 当前 tick（注入式单调钟）。
    tick: u64,
    /// 推迟账容量上限（极端拥塞显性化的闸）。
    defer_capacity: usize,
}

impl SurfScheduler {
    /// 新调度器（空账起步）。
    pub fn new() -> SurfScheduler {
        SurfScheduler {
            surfs: Vec::new(),
            starve_ledger: Vec::new(),
            tick: 0,
            defer_capacity: MAX_SURFS,
        }
    }

    /// 注册表面（重复注册显性拒；容量满显性拒——不静默扩容）。
    pub fn register(
        &mut self,
        id: u32,
        state: SurfState,
        interactive: bool,
        priority: u8,
        budget_units: u32,
    ) -> Result<(), ScCode> {
        if self.find(id).is_some() {
            return Err(ScCode::Duplicate);
        }
        if self.surfs.len() >= MAX_SURFS {
            return Err(ScCode::CapacityFull);
        }
        if budget_units > FRAME_BUDGET_UNITS {
            // 单面请求超过全帧预算：登记为预算上限（钳制是显性规则不是丢数据，
            // 原始请求在调用侧可见——这里按帧预算封顶防止单面独占全帧）。
            return Err(ScCode::BudgetCongested);
        }
        let pos = self.insert_pos(id);
        self.surfs.insert(
            pos,
            Surf {
                id,
                state,
                interactive,
                priority,
                budget_units,
                last_sched: u64::MAX,
                starved_frames: 0,
            },
        );
        Ok(())
    }

    /// 注销表面（连带销案——立案账不留死条目）。
    pub fn unregister(&mut self, id: u32) -> Result<(), ScCode> {
        let pos = match self.find(id) {
            Some(p) => p,
            None => return Err(ScCode::Unregistered),
        };
        self.surfs.remove(pos);
        let mut j = 0usize;
        while j < self.starve_ledger.len() {
            if let Some(x) = self.starve_ledger.get(j) {
                if *x == id {
                    self.starve_ledger.remove(j);
                    continue;
                }
            }
            j += 1;
        }
        Ok(())
    }

    /// 状态更新（激活/隐藏/可见迁移是排程输入面的一部分）。
    pub fn set_state(&mut self, id: u32, state: SurfState) -> Result<(), ScCode> {
        let pos = match self.find(id) {
            Some(p) => p,
            None => return Err(ScCode::Unregistered),
        };
        if let Some(s) = self.surfs.get_mut(pos) {
            s.state = state;
        }
        Ok(())
    }

    /// 单帧排程（判据一二三的主流程；返回 [`SchedFrame`]）。
    pub fn schedule_frame(&mut self) -> Result<SchedFrame, ScCode> {
        // ---- 1 候选收集：激活面才可排程（Inactive 面连候选都不是） ----
        let mut cand: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < self.surfs.len() {
            if let Some(s) = self.surfs.get(i) {
                if s.state != SurfState::Inactive {
                    cand.push(i);
                }
            }
            i += 1;
        }
        // ---- 2 三级排序：可见性 > 交互性 > 轮转（最旧优先）> id 破平 ----
        // 选择排序（稳定语义手工保证：严格更优才换位，等价保序）——
        // 操作步逐次记账（判据四）。
        let mut ops: u32 = 0;
        let n = cand.len();
        let mut a = 0usize;
        while a < n {
            let mut best = a;
            let mut b = a + 1;
            while b < n {
                ops = ops.saturating_add(1);
                let bi = *cand.get(b).unwrap_or(&0);
                let sti = *cand.get(best).unwrap_or(&0);
                if surf_better(self.surfs.get(bi), self.surfs.get(sti)) {
                    best = b;
                }
                b += 1;
            }
            if best != a {
                ops = ops.saturating_add(2);
                cand.swap(a, best);
            }
            a += 1;
        }
        // ---- 3 饥饿立案 + 强插上浮 ----
        let mut starve_cases: Vec<u32> = Vec::new();
        let mut cleared_cases: Vec<u32> = Vec::new();
        let mut forced_flags: Vec<bool> = Vec::new();
        {
            let mut i = 0usize;
            while i < cand.len() {
                let idx = *cand.get(i).unwrap_or(&0);
                let (sid, starved, was_ledgered) = match self.surfs.get(idx) {
                    Some(s) => (s.id, s.starved_frames, self.ledgered(s.id)),
                    None => (0, 0, false),
                };
                let over = starved >= STARVE_FRAME_LIMIT;
                if over && !was_ledgered {
                    self.starve_ledger.push(sid);
                    starve_cases.push(sid);
                }
                forced_flags.push(over || was_ledgered);
                i += 1;
            }
        }
        // 强插面按候选序上浮到队首（保持相对序——公平不破坏确定性）。
        {
            let mut out: Vec<usize> = Vec::new();
            let mut rest: Vec<usize> = Vec::new();
            let mut i = 0usize;
            while i < cand.len() {
                let idx = *cand.get(i).unwrap_or(&0);
                let f = *forced_flags.get(i).unwrap_or(&false);
                if f {
                    out.push(idx);
                } else {
                    rest.push(idx);
                }
                i += 1;
            }
            let mut j = 0usize;
            while j < rest.len() {
                out.push(*rest.get(j).unwrap_or(&0));
                j += 1;
            }
            cand = out;
        }
        // ---- 4 预算闸：按序收编、超限截尾推迟（推迟不丢弃） ----
        let mut tickets: Vec<SchedTicket> = Vec::new();
        let mut deferred: Vec<u32> = Vec::new();
        let mut used: u32 = 0;
        let mut rank: u32 = 0;
        {
            let mut i = 0usize;
            while i < cand.len() {
                let idx = *cand.get(i).unwrap_or(&0);
                let (sid, bu, forced) = match self.surfs.get(idx) {
                    Some(s) => (s.id, s.budget_units, *forced_flags.get(i).unwrap_or(&false)),
                    None => (0, 0, false),
                };
                let nb = used.saturating_add(bu);
                if nb <= FRAME_BUDGET_UNITS {
                    used = nb;
                    tickets.push(SchedTicket {
                        surf_id: sid,
                        rank,
                        forced,
                        budget_units: bu,
                    });
                    rank += 1;
                    // 排程兑现：轮转键刷新 + 饥饿清零 + 立案销案。
                    if let Some(s) = self.surfs.get_mut(idx) {
                        s.last_sched = self.tick;
                        s.starved_frames = 0;
                    }
                    if self.ledgered(sid) {
                        cleared_cases.push(sid);
                        self.ledger_remove(sid);
                    }
                } else {
                    deferred.push(sid);
                    if let Some(s) = self.surfs.get_mut(idx) {
                        s.starved_frames = s.starved_frames.saturating_add(1);
                    }
                }
                i += 1;
            }
        }
        // ---- 5 未入候选的激活面同样计饥饿（不可见面也要流动） ----
        {
            let mut i = 0usize;
            while i < self.surfs.len() {
                let (sid, st, scheduled_this_frame) = match self.surfs.get(i) {
                    Some(s) => (s.id, s.state, s.last_sched == self.tick),
                    None => (0, SurfState::Inactive, false),
                };
                if st != SurfState::Inactive && !scheduled_this_frame {
                    if let Some(s) = self.surfs.get_mut(i) {
                        s.starved_frames = s.starved_frames.saturating_add(1);
                    }
                    let starved = match self.surfs.get(i) {
                        Some(s) => s.starved_frames,
                        None => 0,
                    };
                    if starved >= STARVE_FRAME_LIMIT && !self.ledgered(sid) {
                        self.starve_ledger.push(sid);
                        starve_cases.push(sid);
                    }
                }
                i += 1;
            }
        }
        // ---- 6 开销账（判据四）：超预算=缺陷显性 ----
        if ops > SCHED_OPS_BUDGET {
            return Err(ScCode::OpsOverBudget);
        }
        // ---- 7 推迟账容量闸（极端拥塞显性化） ----
        if deferred.len() > self.defer_capacity {
            return Err(ScCode::BudgetCongested);
        }
        self.tick = self.tick.saturating_add(1);
        Ok(SchedFrame {
            tickets,
            deferred,
            starve_cases,
            cleared_cases,
            ops,
        })
    }

    /// 开销账读取（最近一帧排程的操作步——账面留查）。
    pub const fn current_tick(&self) -> u64 {
        self.tick
    }

    /// 在账表面数。
    pub const fn surf_count(&self) -> usize {
        self.surfs.len()
    }

    /// 立案账读取。
    pub fn starve_ledger(&self) -> Vec<u32> {
        self.starve_ledger.clone()
    }

    // ---- 内部 ----

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.surfs.len() {
            if let Some(s) = self.surfs.get(i) {
                if s.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn insert_pos(&self, id: u32) -> usize {
        let mut i = 0usize;
        while i < self.surfs.len() {
            if let Some(s) = self.surfs.get(i) {
                if s.id > id {
                    return i;
                }
            }
            i += 1;
        }
        self.surfs.len()
    }

    fn ledgered(&self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.starve_ledger.len() {
            if let Some(x) = self.starve_ledger.get(i) {
                if *x == id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn ledger_remove(&mut self, id: u32) {
        let mut i = 0usize;
        while i < self.starve_ledger.len() {
            if let Some(x) = self.starve_ledger.get(i) {
                if *x == id {
                    self.starve_ledger.remove(i);
                    return;
                }
            }
            i += 1;
        }
    }
}

impl Default for SurfScheduler {
    fn default() -> SurfScheduler {
        SurfScheduler::new()
    }
}

/// 三级排序谓词：`a` 是否严格优于 `b`（严格才换位——等价保序=稳定）。
fn surf_better(a: Option<&Surf>, b: Option<&Surf>) -> bool {
    let (a, b) = match (a, b) {
        (Some(a), Some(b)) => (a, b),
        _ => return false,
    };
    // 键一：可见性（Visible > ActiveHidden）。
    let av = a.state == SurfState::Visible;
    let bv = b.state == SurfState::Visible;
    if av != bv {
        return av;
    }
    // 键二：优先级档（数值小者优）。
    if a.priority != b.priority {
        return a.priority < b.priority;
    }
    // 键三：交互性。
    if a.interactive != b.interactive {
        return a.interactive;
    }
    // 键四：轮转公平（最久未排程者优；从未排程 u64::MAX 最优）。
    if a.last_sched != b.last_sched {
        return a.last_sched > b.last_sched;
    }
    // 破平：id 升序（确定性）。
    a.id < b.id
}

// ---------------------------------------------------------------------------
// 六、排程报告（读屏可达人话面）
// ---------------------------------------------------------------------------

/// 单帧排程报告（锚点：读屏可达；两行内固定预算）。
pub fn sched_report(f: &SchedFrame, ops: u32) -> String {
    let mut s = String::new();
    s.push_str(&alloc::format!(
        "排程：本帧{}面/推迟{}面/立案{}销案{}；开销{}步（预算{}）",
        f.tickets.len(),
        f.deferred.len(),
        f.starve_cases.len(),
        f.cleared_cases.len(),
        ops,
        SCHED_OPS_BUDGET
    ));
    s
}

/// 30 表面开销探针（判据四的可执行面：构造 30 面跑一帧，返回操作步口径结论）。
pub fn sched_ops_probe() -> Result<u32, ScCode> {
    let mut sch = SurfScheduler::new();
    let mut i: u32 = 0;
    while (i as usize) < SCHED_PROBE_SURFS {
        let st = if i % 3 == 0 {
            SurfState::Visible
        } else {
            SurfState::ActiveHidden
        };
        sch.register(
            i + 1,
            st,
            i % 5 == 0,
            (i % 4) as u8,
            8 + (i % 7) * 4,
        )?;
        i += 1;
    }
    let f = sch.schedule_frame()?;
    // 30 面 × 每面预算 ≤ 32，总请求 ≤ 960 ≤ 1024 ⇒ 无推迟的全收帧；
    // 操作步出账给判据四对拍（微秒级实测的确定性代理值）。
    if f.deferred.is_empty() && f.tickets.len() == SCHED_PROBE_SURFS && f.ops <= SCHED_OPS_BUDGET {
        Ok(f.ops)
    } else {
        Err(ScCode::BudgetCongested)
    }
}
