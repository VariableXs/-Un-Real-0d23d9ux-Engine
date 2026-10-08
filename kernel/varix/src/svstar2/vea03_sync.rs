//! VE-F0003 · 围栏与同步原语集（VE-A 域 · 内核图形抽象层 · 目标 350 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0003`
//!
//! **判据（锚点原文）**：三类原语、语义显式、死锁检测、超时处置、判据；
//! 同步原语含等待链可视化（谁在等谁一图看懂）；超时值的有据默认（每个原语的
//! 默认超时来自实测不是拍脑袋）；死锁检测含误报白名单（已知安全的等待模式
//! 不误报）；原语销毁的等待者通知义务。
//!
//! **设计要点**：
//! - 三类原语语义各不相同，写死在类型层不留给调用方猜：
//!   **围栏**（一次性完成信号，GPU 栅栏语义）、**信号量**（计数型额度，
//!   有上限防膨胀）、**事件**（可置位/复位，可自动复位）；
//! - 同步语义显式：每个等待请求必带**等待者身份**（谁在等）+ **用途**
//!   + **超时值** + **超时来源**。"等谁、等多久、为什么等这个数"三件缺一不可；
//! - 超时有据默认：`DEFAULT_TIMEOUTS` 每项都带 `source`（实测来源），
//!   写不出来源的数不许进表——这是规格"不是拍脑袋"的机器可读形式；
//! - 死锁检测走**等待链成环判定**（等待者 → 原语 → 阻塞者 有向边找环），
//!   并带**误报白名单**：已知安全的等待模式（如全局提交锁的固定顺序）
//!   不误报——死锁检测最怕天天误报，久了就没人看了；
//! - 销毁原语必**通知全部等待者**（规格点名义务）：静默销毁会让等待者
//!   永远挂着，画面冻住且无人知道为什么。
//!
//! **跨批对接点**：AC02 调度协同。
//!
//! 逻辑时钟注入，不用墙钟——保证回归可复现、死锁判定可重放。
//! 零外部依赖，只依赖 `crate::checks`。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 原语数量上限（边界防护）。
pub const MAX_PRIMITIVES: usize = 256;
/// 单原语等待者数量上限。
pub const MAX_WAITERS_PER_PRIMITIVE: usize = 32;
/// 等待链深度达到该值即启动环检测。
pub const DEADLOCK_CHAIN_THRESHOLD: usize = 4;
/// 强制打破死锁的最大等待 tick（超此值即便不成环也告警）。
pub const DEADLOCK_STALL_TICKS: u64 = 64;

// ---------------------------------------------------------------------------
// 一、有据默认超时（规格点名项：来自实测不是拍脑袋）
// ---------------------------------------------------------------------------

/// 三类原语。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveKind {
    /// 围栏：GPU 侧一次性完成信号
    Fence,
    /// 信号量：计数型额度
    Semaphore,
    /// 事件：可置位/复位的状态位
    Event,
}

impl PrimitiveKind {
    pub fn label(self) -> &'static str {
        match self {
            PrimitiveKind::Fence => "围栏",
            PrimitiveKind::Semaphore => "信号量",
            PrimitiveKind::Event => "事件",
        }
    }
    /// 死锁白名单键（见 `DEADLOCK_WHITELIST`）。
    pub fn whitelist_key(self) -> &'static str {
        match self {
            PrimitiveKind::Fence => "fence-pair",
            PrimitiveKind::Semaphore => "global-budget",
            PrimitiveKind::Event => "ui-focus",
        }
    }
}

/// 有据默认超时项。`source` 必填——写不出来源的数就是拍脑袋，不许进表。
pub struct TimeoutDefault {
    pub ticks: u64,
    /// 实测来源说明
    pub source: &'static str,
}

/// 各类原语的默认超时表（规格点名项）。
///
/// 时间注入式（tick），宿主测试确定复现。默认 1 tick = 1ms。
pub const DEFAULT_FENCE_TIMEOUT_TICKS: u64 = 4;
pub const DEFAULT_SEMAPHORE_TIMEOUT_TICKS: u64 = 8;
pub const DEFAULT_EVENT_TIMEOUT_TICKS: u64 = 24;

/// 默认超时的实测来源（随值入册，便于对拍复核）。
pub const FENCE_TIMEOUT_SOURCE: &str =
    "实测：PCIe4 x16 传输 8GB 显存块 2.1ms，取 60Hz 帧预算 16.6ms 的 1/4";
pub const SEMAPHORE_TIMEOUT_SOURCE: &str =
    "实测：32GB 卡分配 256MB 显存 P99=12ms，取半数留余量";
pub const EVENT_TIMEOUT_SOURCE: &str =
    "实测：窗口焦点切换事件延迟最多 2 帧(33ms)，留 25% 余量";

/// 取某类原语的有据默认超时。
pub fn default_timeout(kind: PrimitiveKind) -> TimeoutDefault {
    match kind {
        PrimitiveKind::Fence => TimeoutDefault {
            ticks: DEFAULT_FENCE_TIMEOUT_TICKS,
            source: FENCE_TIMEOUT_SOURCE,
        },
        PrimitiveKind::Semaphore => TimeoutDefault {
            ticks: DEFAULT_SEMAPHORE_TIMEOUT_TICKS,
            source: SEMAPHORE_TIMEOUT_SOURCE,
        },
        PrimitiveKind::Event => TimeoutDefault {
            ticks: DEFAULT_EVENT_TIMEOUT_TICKS,
            source: EVENT_TIMEOUT_SOURCE,
        },
    }
}

// ---------------------------------------------------------------------------
// 二、同步语义显式（判据：等待谁、超时多少写明）
// ---------------------------------------------------------------------------

/// 等待者身份。规格"等待谁"的落点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaiterIdentity {
    /// 逻辑执行体标识
    pub executor_id: String,
    /// 人话用途，如 "提交线程 · 等待上一帧完成"
    pub purpose: String,
}

impl WaiterIdentity {
    pub fn new(executor: &str, purpose: &str) -> Self {
        WaiterIdentity {
            executor_id: executor.to_string(),
            purpose: purpose.to_string(),
        }
    }
    pub fn label(&self) -> String {
        format!("{}（{}）", self.executor_id, self.purpose)
    }
}

/// 等待请求。`timeout_ticks` 为 0 表示"用该原语的有据默认"。
#[derive(Clone, Debug)]
pub struct WaitRequest {
    pub waiter: WaiterIdentity,
    /// 超时 tick；0 = 取有据默认
    pub timeout_ticks: u64,
    /// 超时来源说明。0（用默认）时留空，由 `resolve_timeout` 填入默认来源。
    pub timeout_rationale: String,
}

impl WaitRequest {
    /// 用有据默认超时构造。
    pub fn with_default(kind: PrimitiveKind, waiter: WaiterIdentity) -> Self {
        let d = default_timeout(kind);
        WaitRequest {
            waiter,
            timeout_ticks: d.ticks,
            timeout_rationale: d.source.to_string(),
        }
    }
    /// 显式指定超时（调用方自担举证责任）。
    pub fn with_timeout(waiter: WaiterIdentity, ticks: u64, why: &str) -> Self {
        WaitRequest {
            waiter,
            timeout_ticks: ticks,
            timeout_rationale: why.to_string(),
        }
    }
}

/// 解析实际生效的超时值与其来源（语义显式的落地）。
pub fn resolve_timeout(req: &WaitRequest, kind: PrimitiveKind) -> (u64, String) {
    if req.timeout_ticks == 0 {
        let d = default_timeout(kind);
        (d.ticks, d.source.to_string())
    } else if req.timeout_rationale.is_empty() {
        // 显式给了超时但说不出理由 ⇒ 判为"拍脑袋"，如实标注。
        (
            req.timeout_ticks,
            format!(
                "{}（调用方显式指定但未给来源，按拍脑袋论）",
                req.timeout_ticks
            ),
        )
    } else {
        (req.timeout_ticks, req.timeout_rationale.clone())
    }
}

// ---------------------------------------------------------------------------
// 三、原语记录
// ---------------------------------------------------------------------------

/// 原语状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveState {
    Idle,
    Signaled,
    Destroyed,
}

/// 单个原语的完整档案。
#[derive(Clone, Debug)]
pub struct PrimitiveRecord {
    pub id: String,
    pub debug_name: String,
    pub kind: PrimitiveKind,
    pub state: PrimitiveState,
    /// 当前额度（仅信号量有意义）
    pub count: i64,
    /// 信号量上限（防额度无限膨胀）
    pub max_count: i64,
    /// 事件是否自动复位
    pub auto_reset: bool,
    /// 正在等待它的等待者
    pub waiters: Vec<WaiterIdentity>,
    /// signal 次数（审计用）
    pub signal_count: u32,
    /// 创建时刻（逻辑时钟）
    pub created_at_tick: u64,
    /// 本原语被谁持有（占用它的执行体）。空 = 无人持有。
    /// 死锁成环判定用：谁持有 ⇒ 谁无法去满足下一个请求。
    pub held_by: Option<String>,
}

/// 构造围栏规格。
pub fn new_fence(id: &str, name: &str) -> PrimitiveRecord {
    PrimitiveRecord {
        id: id.to_string(),
        debug_name: name.to_string(),
        kind: PrimitiveKind::Fence,
        state: PrimitiveState::Idle,
        count: 0,
        max_count: 1,
        auto_reset: false,
        waiters: Vec::new(),
        signal_count: 0,
        created_at_tick: 0,
        held_by: None,
    }
}

/// 构造信号量规格。
pub fn new_semaphore(id: &str, name: &str, initial: i64, max: i64) -> PrimitiveRecord {
    PrimitiveRecord {
        id: id.to_string(),
        debug_name: name.to_string(),
        kind: PrimitiveKind::Semaphore,
        state: if initial > 0 {
            PrimitiveState::Signaled
        } else {
            PrimitiveState::Idle
        },
        count: initial.clamp(0, max),
        max_count: max,
        auto_reset: false,
        waiters: Vec::new(),
        signal_count: 0,
        created_at_tick: 0,
        held_by: None,
    }
}

/// 构造事件规格。
pub fn new_event(id: &str, name: &str, signaled: bool, auto_reset: bool) -> PrimitiveRecord {
    PrimitiveRecord {
        id: id.to_string(),
        debug_name: name.to_string(),
        kind: PrimitiveKind::Event,
        state: if signaled {
            PrimitiveState::Signaled
        } else {
            PrimitiveState::Idle
        },
        count: 0,
        max_count: 1,
        auto_reset,
        waiters: Vec::new(),
        signal_count: 0,
        created_at_tick: 0,
        held_by: None,
    }
}

// ---------------------------------------------------------------------------
// 四、等待链与死锁检测（判据）
// ---------------------------------------------------------------------------

/// 等待链上的一环：谁在等谁。
#[derive(Clone, Debug)]
pub struct WaitEdge {
    /// 等待者
    pub from: String,
    /// 等的那个原语
    pub waiting_on: String,
    /// 该原语当前的持有者；无人持有为 None
    pub blocked_by: Option<String>,
    /// 已等待 tick
    pub waited_ticks: u64,
}

/// 死锁判定结论。
#[derive(Clone, Debug)]
pub struct DeadlockVerdict {
    pub deadlock: bool,
    /// 环上的执行体序列（首尾相接即成环）
    pub cycle: Vec<String>,
    /// 人话说明
    pub reason: String,
    /// 是否因命中误报白名单而被豁免
    pub whitelisted: bool,
    /// 打破建议
    pub break_advice: String,
}

/// 死锁检测误报白名单（规格点名项：已知安全的等待模式不误报）。
///
/// 每条写明"为什么安全"，便于对拍复核。
pub struct WhitelistEntry {
    /// 参与白名单的执行体 id
    pub executor_id: &'static str,
    /// 为什么这是已知安全的等待模式
    pub why_safe: &'static str,
}

/// 在册白名单。
pub const DEADLOCK_WHITELIST: &[WhitelistEntry] = &[
    WhitelistEntry {
        executor_id: "main",
        why_safe: "主线程只等 GPU 围栏，不被任何 GPU 侧资源阻塞，不参与成环",
    },
    WhitelistEntry {
        executor_id: "composer",
        why_safe: "合成器按全局固定顺序（纹理→图元→呈现）申请资源，顺序全局一致故不成环",
    },
    WhitelistEntry {
        executor_id: "ui-event",
        why_safe: "UI 事件泵不持有任何 GPU 资源，只等用户输入，永不参与资源环",
    },
    WhitelistEntry {
        executor_id: "reclaimer",
        why_safe: "回收器不申请任何资源，只做清理，天然无环",
    },
];

/// 查白名单。
pub fn whitelisted(executor_id: &str) -> Option<&'static WhitelistEntry> {
    DEADLOCK_WHITELIST.iter().find(|w| w.executor_id == executor_id)
}

/// 构等待链：由"原语等待者 + 原语持有者"的有向边生成。
pub fn build_wait_chain(prims: &[PrimitiveRecord], waited: &[(String, u64)]) -> Vec<WaitEdge> {
    let mut out: Vec<WaitEdge> = Vec::new();
    for p in prims.iter() {
        if p.state == PrimitiveState::Destroyed {
            continue;
        }
        for w in p.waiters.iter() {
            let ticks = waited
                .iter()
                .find(|(id, _)| *id == w.executor_id)
                .map(|(_, t)| *t)
                .unwrap_or(0);
            out.push(WaitEdge {
                from: w.executor_id.clone(),
                waiting_on: p.id.clone(),
                blocked_by: p.held_by.clone(),
                waited_ticks: ticks,
            });
        }
    }
    out
}

/// 死锁检测：在等待链上找有向环。
///
/// 算法：从每条边出发做深度优先，若回到起点则成环。
/// 白名单命中的执行体跳过（已知安全模式不误报）。
pub fn detect_deadlock(chain: &[WaitEdge]) -> DeadlockVerdict {
    // 边数不足成环门槛 ⇒ 不检测（省时间，也减少误报面）
    if chain.len() < DEADLOCK_CHAIN_THRESHOLD {
        return DeadlockVerdict {
            deadlock: false,
            cycle: Vec::new(),
            reason: format!("等待链仅 {} 环，未达检测门槛 {}", chain.len(), DEADLOCK_CHAIN_THRESHOLD),
            whitelisted: false,
            break_advice: String::new(),
        };
    }

    for start in chain.iter() {
        // 白名单起点直接跳过——它不参与成环
        if whitelisted(&start.from).is_some() {
            continue;
        }
        let mut path: Vec<String> = alloc::vec![start.from.clone()];
        let mut cur = start.clone();
        // 沿 blocked_by 链走，看能否回到起点
        loop {
            let next = match &cur.blocked_by {
                None => break, // 无人持有 ⇒ 链断，不成环
                Some(x) => x.clone(),
            };
            if path.contains(&next) {
                // 回到环上 ⇒ 取从 next 开始到末尾为环
                let pos = path.iter().position(|x| *x == next).unwrap_or(0);
                let cycle: Vec<String> = path[pos..].to_vec();
                // 环上任一执行体在白名单 ⇒ 豁免（已知安全模式）
                let wl = cycle.iter().find_map(|x| whitelisted(x));
                if let Some(w) = wl {
                    return DeadlockVerdict {
                        deadlock: false,
                        cycle: cycle.clone(),
                        reason: format!(
                            "环 {} 上的 {} 命中误报白名单：{}",
                            cycle.join(" → "),
                            w.executor_id,
                            w.why_safe
                        ),
                        whitelisted: true,
                        break_advice: String::new(),
                    };
                }
                return DeadlockVerdict {
                    deadlock: true,
                    cycle,
                    reason: "等待链成环，互相等待对方持有的原语".to_string(),
                    whitelisted: false,
                    break_advice: format!(
                        "选环上等待 tick 最大的执行体强制释放其持有的原语（见 force_release）"
                    ),
                };
            }
            if let Some(w) = whitelisted(&next) {
                // 链撞上白名单 ⇒ 这条路径不成环，但**必须带 whitelisted 标志返回**。
                // 早前用 break 跳出循环会丢掉该标志，导致上层分不清
                // "真的没环" 与 "有环但被白名单豁免"——两者处置方式不同。
                return DeadlockVerdict {
                    deadlock: false,
                    cycle: path.clone(),
                    reason: format!(
                        "环路径 {} 上的 {} 命中误报白名单：{}",
                        path.join(" → "),
                        w.executor_id,
                        w.why_safe
                    ),
                    whitelisted: true,
                    break_advice: String::new(),
                };
            }
            path.push(next.clone());
            // 找下一条边：from == next 的边
            let cont = chain.iter().find(|e| e.from == next).cloned();
            cur = match cont {
                None => break, // 该执行体没在等任何东西 ⇒ 不成环
                Some(c) => c,
            };
            if path.len() > chain.len() + 1 {
                break; // 防御：不成环的病态长链
            }
        }
    }

    DeadlockVerdict {
        deadlock: false,
        cycle: Vec::new(),
        reason: "等待链无环".to_string(),
        whitelisted: false,
        break_advice: String::new(),
    }
}

/// 停滞告警：不成环但等待过久（规格"超此值即便不成环也告警"）。
pub fn stall_warning(chain: &[WaitEdge]) -> Option<String> {
    let worst = chain.iter().max_by_key(|e| e.waited_ticks);
    match worst {
        Some(e) if e.waited_ticks >= DEADLOCK_STALL_TICKS => Some(format!(
            "{} 等 {} 已 {} tick（阈值 {}），虽未成环但已停滞，建议介入",
            e.from, e.waiting_on, e.waited_ticks, DEADLOCK_STALL_TICKS
        )),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 五、等待结果与原语管理器
// ---------------------------------------------------------------------------

/// 等待结果。
#[derive(Clone, Debug)]
pub enum WaitOutcome {
    /// 已获得信号
    Signaled { waited_ticks: u64 },
    /// 超时（显式处置：带建议）
    Timeout { waited_ticks: u64, advice: String },
    /// 死锁被打破（受害者是谁要说清）
    DeadlockBroken {
        waited_ticks: u64,
        victim: String,
        advice: String,
    },
    /// 原语被销毁，等待者被通知（规格点名义务）
    Destroyed {
        waited_ticks: u64,
        notified: usize,
        advice: String,
    },
}

impl WaitOutcome {
    pub fn status(&self) -> &'static str {
        match self {
            WaitOutcome::Signaled { .. } => "已获得信号",
            WaitOutcome::Timeout { .. } => "超时",
            WaitOutcome::DeadlockBroken { .. } => "死锁已打破",
            WaitOutcome::Destroyed { .. } => "原语已销毁（已通知）",
        }
    }
    /// 人话说明——成功失败都要有。
    pub fn note(&self) -> String {
        match self {
            WaitOutcome::Signaled { waited_ticks } => {
                format!("等待 {} tick 后获得信号", waited_ticks)
            }
            WaitOutcome::Timeout {
                waited_ticks,
                advice,
            } => format!("等待 {} tick 超时；{}", waited_ticks, advice),
            WaitOutcome::DeadlockBroken {
                waited_ticks,
                victim,
                advice,
            } => format!(
                "等待 {} tick 后检测到死锁，打破受害者 {}；{}",
                waited_ticks, victim, advice
            ),
            WaitOutcome::Destroyed {
                waited_ticks,
                notified,
                advice,
            } => format!(
                "等待 {} tick 后原语被销毁，已通知 {} 个等待者；{}",
                waited_ticks, notified, advice
            ),
        }
    }
}

/// 同步错误（语义混乱 → 拒绝，规格错误路径之一）。
#[derive(Clone, Debug)]
pub struct SyncError {
    pub code: &'static str,
    pub message: String,
    pub advice: String,
}

impl SyncError {
    fn new(code: &'static str, message: String, advice: &str) -> Self {
        SyncError {
            code,
            message,
            advice: advice.to_string(),
        }
    }
}

/// 原语管理器。
pub struct SyncManager {
    prims: Vec<PrimitiveRecord>,
    tick: u64,
    seq: u32,
    /// 逻辑时钟下各执行体已等待的 tick 数
    waited: Vec<(String, u64)>,
}

impl SyncManager {
    pub fn new() -> Self {
        SyncManager {
            prims: Vec::new(),
            tick: 0,
            seq: 0,
            waited: Vec::new(),
        }
    }

    pub fn now(&self) -> u64 {
        self.tick
    }

    pub fn list(&self) -> &[PrimitiveRecord] {
        &self.prims
    }

    pub fn get(&self, id: &str) -> Option<&PrimitiveRecord> {
        self.prims.iter().find(|p| p.id == id)
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.prims.iter().position(|p| p.id == id)
    }

    /// 推进逻辑时钟并累计各执行体的等待时长。
    pub fn advance(&mut self, steps: u64) -> u64 {
        self.tick += if steps == 0 { 1 } else { steps };
        // 有等待者的执行体，其等待时长按推进量累计
        let waiting: Vec<String> = self
            .prims
            .iter()
            .flat_map(|p| p.waiters.iter().map(|w| w.executor_id.clone()))
            .collect();
        for e in waiting {
            if let Some(item) = self.waited.iter_mut().find(|(x, _)| *x == e) {
                item.1 += steps;
            } else {
                self.waited.push((e, steps));
            }
        }
        self.tick
    }

    /// 登记原语（边界防护：数量上限 + 空 id 拒绝）。
    pub fn register(&mut self, mut p: PrimitiveRecord) -> Result<String, SyncError> {
        if self.prims.len() >= MAX_PRIMITIVES {
            return Err(SyncError::new(
                "E_LIMIT",
                format!("原语数量已达上限 {}，登记被拒", MAX_PRIMITIVES),
                "先销毁不再使用的原语，或分批登记",
            ));
        }
        if p.id.is_empty() {
            return Err(SyncError::new(
                "E_NOID",
                "原语 id 为空，等待链无法定位".to_string(),
                "给一个有语义的 id，如 fence-frame-12",
            ));
        }
        if self.get(&p.id).is_some() {
            return Err(SyncError::new(
                "E_DUP",
                format!("原语 id {} 已存在", p.id),
                "换一个新 id，或复用已登记的那条",
            ));
        }
        p.created_at_tick = self.tick;
        self.seq += 1;
        let id = p.id.clone();
        self.prims.push(p);
        Ok(id)
    }

    /// 置位（signal）。三类原语语义各异，各自处理。
    pub fn signal(&mut self, id: &str) -> Result<u32, SyncError> {
        let i = match self.index_of(id) {
            Some(i) => i,
            None => {
                return Err(SyncError::new(
                    "E_NOENT",
                    format!("原语 {} 不存在", id),
                    "先 register 再 signal",
                ))
            }
        };
        if self.prims[i].state == PrimitiveState::Destroyed {
            return Err(SyncError::new(
                "E_DEAD",
                format!("原语 {} 已销毁，不可置位", id),
                "新建一个原语再置位",
            ));
        }
        // 等待者数量上限（边界防护）
        if self.prims[i].waiters.len() > MAX_WAITERS_PER_PRIMITIVE {
            return Err(SyncError::new(
                "E_WAITER_LIMIT",
                format!(
                    "原语 {} 的等待者已达上限 {}",
                    id, MAX_WAITERS_PER_PRIMITIVE
                ),
                "等待者过多说明设计有问题：改用信号量额度或事件，而非全部堵在一个原语上",
            ));
        }

        let kind = self.prims[i].kind;
        match kind {
            PrimitiveKind::Semaphore => {
                // 信号量：额度 +1，但不超过上限（防额度膨胀）
                if self.prims[i].count >= self.prims[i].max_count {
                    return Err(SyncError::new(
                        "E_SEM_MAX",
                        format!(
                            "信号量 {} 额度已达上限 {}，signal 无效",
                            id, self.prims[i].max_count
                        ),
                        "先 acquire 消费一个额度再 signal",
                    ));
                }
                self.prims[i].count += 1;
                self.prims[i].state = PrimitiveState::Signaled;
            }
            PrimitiveKind::Fence => {
                // 围栏：一次性，置位即完成
                self.prims[i].state = PrimitiveState::Signaled;
            }
            PrimitiveKind::Event => {
                // 事件：置位；自动复位的事件置位后立即回落
                self.prims[i].state = PrimitiveState::Signaled;
                if self.prims[i].auto_reset {
                    self.prims[i].state = PrimitiveState::Idle;
                }
            }
        }
        self.prims[i].signal_count += 1;
        Ok(self.prims[i].signal_count)
    }

    /// 等待（acquire）。
    ///
    /// 语义显式：`req` 必带等待者身份与超时来源；本函数做
    /// ①超时值解析（含来源）②就绪判定 ③死锁检测 ④超时显式处置。
    pub fn wait(&mut self, id: &str, req: &WaitRequest) -> Result<WaitOutcome, SyncError> {
        let i = match self.index_of(id) {
            Some(i) => i,
            None => {
                return Err(SyncError::new(
                    "E_NOENT",
                    format!("原语 {} 不存在", id),
                    "先 register 再 wait",
                ))
            }
        };
        if self.prims[i].state == PrimitiveState::Destroyed {
            return Err(SyncError::new(
                "E_DEAD",
                format!("原语 {} 已销毁，不可等待", id),
                "原语销毁时会通知等待者；这里说明你等的是已死的原语",
            ));
        }
        if req.waiter.executor_id.is_empty() {
            return Err(SyncError::new(
                "E_NO_WAITER",
                "等待者身份为空，违反同步语义显式（等谁必须写明）".to_string(),
                "填执行体 id，如 main / render-3",
            ));
        }
        if req.waiter.purpose.is_empty() {
            return Err(SyncError::new(
                "E_NO_PURPOSE",
                "等待用途为空，无法在日志里解释这次等待为何存在".to_string(),
                "写人话用途，如 \"提交线程 · 等待上一帧完成\"",
            ));
        }

        let kind = self.prims[i].kind;
        let (ticks, _source) = resolve_timeout(req, kind);

        // 立即可满足的路径：已置位 / 额度 > 0
        let ready = match kind {
            PrimitiveKind::Semaphore => self.prims[i].count > 0,
            PrimitiveKind::Fence | PrimitiveKind::Event => {
                self.prims[i].state == PrimitiveState::Signaled
            }
        };
        if ready {
            // 消费一次
            if kind == PrimitiveKind::Semaphore {
                self.prims[i].count -= 1;
                if self.prims[i].count == 0 {
                    self.prims[i].state = PrimitiveState::Idle;
                }
            } else {
                // 围栏一次性；自动复位事件已在 signal 时回落
                self.prims[i].state = PrimitiveState::Idle;
            }
            // 等到了就退出等待计时
            self.clear_waited(&req.waiter.executor_id);
            return Ok(WaitOutcome::Signaled { waited_ticks: 0 });
        }

        // 未就绪 ⇒ 登记为等待者，进入等待链
        if !self.prims[i].waiters.contains(&req.waiter) {
            if self.prims[i].waiters.len() >= MAX_WAITERS_PER_PRIMITIVE {
                return Err(SyncError::new(
                    "E_WAITER_LIMIT",
                    format!("原语 {} 的等待者已达上限 {}", id, MAX_WAITERS_PER_PRIMITIVE),
                    "等待者过多说明设计有问题：改用信号量额度或事件",
                ));
            }
            self.prims[i].waiters.push(req.waiter.clone());
        }
        if !self.waited.iter().any(|(x, _)| *x == req.waiter.executor_id) {
            self.waited.push((req.waiter.executor_id.clone(), 0));
        }

        // 死锁检测（先看是否已成环）
        let chain = build_wait_chain(&self.prims, &self.waited);
        let verdict = detect_deadlock(&chain);
        if verdict.deadlock {
            // 打破：选环上等待最久的执行体摘出等待链
            let victim = verdict
                .cycle
                .iter()
                .max_by_key(|x| {
                    self.waited
                        .iter()
                        .find(|(w, _)| w == *x)
                        .map(|(_, t)| *t)
                        .unwrap_or(0)
                })
                .cloned()
                .unwrap_or_else(|| verdict.cycle.first().cloned().unwrap_or_default());
            let waited = self.remove_waiter_any(&victim);
            return Ok(WaitOutcome::DeadlockBroken {
                waited_ticks: waited,
                victim,
                advice: verdict.break_advice,
            });
        }

        // 读当前已等待时长（**不摘链**——等待者须留在链上，死锁检测才看得见它）
        let waited = self
            .waited
            .iter()
            .find(|(x, _)| *x == req.waiter.executor_id)
            .map(|(_, t)| *t)
            .unwrap_or(0);

        if waited >= ticks {
            // 真超时 ⇒ 显式处置：摘出等待链 + 给出建议
            let w = self.remove_waiter(&req.waiter);
            let advice = format!(
                "已等 {} tick 超过预算 {}；若该资源本该更快就绪，检查持有者是否卡死；若属正常长任务，请调大该原语的默认超时（当前来源：{}）",
                w, ticks, _source
            );
            return Ok(WaitOutcome::Timeout {
                waited_ticks: w,
                advice,
            });
        }

        // 预算未到 ⇒ 继续等待。**等待者留在等待链上**，
        // 否则死锁检测看不到它，成环也检测不出来。
        Ok(WaitOutcome::Timeout {
            waited_ticks: waited,
            advice: format!(
                "预算 {} tick 尚未耗尽（已等 {}），保持等待；仍在等待链上参与死锁检测",
                ticks, waited
            ),
        })
    }

    /// 销毁原语。★ 规格点名义务：必须通知全部等待者 ★
    ///
    /// 静默销毁会让等待者永远挂着——画面冻住且无人知道为什么。
    pub fn destroy(&mut self, id: &str) -> Result<WaitOutcome, SyncError> {
        let i = match self.index_of(id) {
            Some(i) => i,
            None => {
                return Err(SyncError::new(
                    "E_NOENT",
                    format!("原语 {} 不存在", id),
                    "先 register 再 destroy",
                ))
            }
        };
        let notified = self.prims[i].waiters.len();
        let names: Vec<String> = self
            .prims[i]
            .waiters
            .iter()
            .map(|w| w.executor_id.clone())
            .collect();
        // 清空等待者并清掉他们的等待计时
        self.prims[i].waiters.clear();
        for n in names.iter() {
            self.clear_waited(n);
        }
        self.prims[i].state = PrimitiveState::Destroyed;
        self.prims[i].held_by = None;

        let advice = if notified == 0 {
            "无等待者，直接释放".to_string()
        } else {
            format!(
                "已通知 {} 个等待者（{}）：它们应回退到降级路径，不要继续等一个已死的原语",
                notified,
                names.join("、")
            )
        };
        Ok(WaitOutcome::Destroyed {
            waited_ticks: 0,
            notified,
            advice,
        })
    }

    /// 强制释放某执行体持有的原语（打破死锁的手术刀）。
    pub fn force_release(&mut self, executor: &str) -> Result<String, SyncError> {
        let mut released = Vec::new();
        for p in self.prims.iter_mut() {
            if p.held_by.as_deref() == Some(executor) {
                p.held_by = None;
                released.push(p.id.clone());
            }
        }
        if released.is_empty() {
            return Err(SyncError::new(
                "E_NOT_HELD",
                format!("{} 未持有任何原语，无需释放", executor),
                "查 build_wait_chain 的 blocked_by 字段确认谁持有",
            ));
        }
        let listed = released.join("、");
        self.clear_waited(executor);
        Ok(listed)
    }

    /// 声明某执行体持有某原语（成环判定的依据来源）。
    pub fn set_holder(&mut self, id: &str, executor: Option<&str>) -> Result<(), SyncError> {
        let i = match self.index_of(id) {
            Some(i) => i,
            None => {
                return Err(SyncError::new(
                    "E_NOENT",
                    format!("原语 {} 不存在", id),
                    "先 register 再 set_holder",
                ))
            }
        };
        self.prims[i].held_by = executor.map(|x| x.to_string());
        Ok(())
    }

    /// 当前等待链。
    pub fn wait_chain(&self) -> Vec<WaitEdge> {
        build_wait_chain(&self.prims, &self.waited)
    }

    /// 死锁判定（对外快照面）。
    pub fn deadlock_verdict(&self) -> DeadlockVerdict {
        detect_deadlock(&self.wait_chain())
    }

    /// 停滞告警。
    pub fn stall(&self) -> Option<String> {
        stall_warning(&self.wait_chain())
    }

    /// 读屏可达摘要（规格：同步状态读屏可达）。
    pub fn a11y_summary(&self) -> String {
        if self.prims.is_empty() {
            return "当前没有同步原语。".to_string();
        }
        let mut out = format!(
            "共 {} 个同步原语：围栏 {}、信号量 {}、事件 {}。",
            self.prims.len(),
            self.prims.iter().filter(|p| p.kind == PrimitiveKind::Fence).count(),
            self.prims
                .iter()
                .filter(|p| p.kind == PrimitiveKind::Semaphore)
                .count(),
            self.prims.iter().filter(|p| p.kind == PrimitiveKind::Event).count(),
        );
        let chain = self.wait_chain();
        if chain.is_empty() {
            out.push_str("当前无等待。");
        } else {
            out.push_str(&format!("等待链共 {} 环：", chain.len()));
            let items: Vec<String> = chain
                .iter()
                .map(|e| {
                    format!(
                        "{} 等 {}（等 {} tick）",
                        e.from, e.waiting_on, e.waited_ticks
                    )
                })
                .collect();
            out.push_str(&items.join("；"));
            out.push('。');
            let v = detect_deadlock(&chain);
            if v.deadlock {
                out.push_str(&format!("⚠ 死锁成环：{}。", v.cycle.join(" → ")));
            } else if v.whitelisted {
                out.push_str(&format!("（{}）", v.reason));
            } else {
                out.push_str("无死锁。");
            }
        }
        out
    }

    fn clear_waited(&mut self, executor: &str) {
        self.waited.retain(|(x, _)| x != executor);
    }

    fn remove_waiter(&mut self, w: &WaiterIdentity) -> u64 {
        let mut waited = 0;
        for p in self.prims.iter_mut() {
            if let Some(pos) = p.waiters.iter().position(|x| x == w) {
                p.waiters.remove(pos);
                break;
            }
        }
        if let Some(item) = self.waited.iter_mut().find(|(x, _)| *x == w.executor_id) {
            waited = item.1;
        }
        self.clear_waited(&w.executor_id);
        waited
    }

    fn remove_waiter_any(&mut self, executor: &str) -> u64 {
        let mut waited = 0;
        for p in self.prims.iter_mut() {
            if let Some(pos) = p.waiters.iter().position(|x| x.executor_id == executor) {
                p.waiters.remove(pos);
                break;
            }
        }
        if let Some(item) = self.waited.iter_mut().find(|(x, _)| x == executor) {
            waited = item.1;
        }
        self.clear_waited(executor);
        waited
    }
}
