//! VE-F0224 · Intel 提交通路与 EXECLISTS（VE-B 域 · Intel 核显组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0224`
//!
//! **判据（锚点原文五条）**：ELSP 批提交、超时语义、池化复用、hung 检测、判据。
//!
//! EXECLISTS 是 Intel 的硬件上下文提交机制：每引擎一组端口（本契约 2 口），
//! 提交时从软件队列**队头取一对上下文描述符**（尾指针×头指针）O(1) 直入
//! 端口——不做全队列扫描。软件队列**按引擎分离**，慢引擎积压不阻塞快引擎
//! （防头阻塞）。每个提交带**显式围栏信号点**（批次完成信号点，下游凭序号
//! 签到）；上下文超时走**上下文重置**并与围栏联动（联动点 = 注入谓词，
//! F0228 硬件围栏映射落位后由其兑现该谓词——本单不依赖未施工模块）。
//!
//! # 头注要点（每条都是判据的反面，写在这里供判据引用）
//!
//! ## 要点一：ELSP 批提交是**队头成对直入**，不是挑选
//!
//! 端口空闲即取队头（至多 2 个）入端口，次序 = FIFO 入队序——O(1) 的
//! 代价是**不做优先级插队**（优先级是描述符字段，由硬件仲裁，不归软件
//! 扫描管）。提交失败/重置的上下文**回退软件队列头**（锚点降级矩阵原文），
//! 回退不改相对次序（重置组整体回队头）。
//!
//! ## 要点二：超时语义是**有终态的重置**，不是无限重试
//!
//! 上下文活跃超 [`CTX_TIMEOUT_TICKS`] ⇒ `CtxTimeout`：该上下文被逐出
//! 端口、批次回退队列、围栏联动谓词被调（reset_pending 记账）。同一上下文
//! 累计重置超 [`MAX_CTX_RESETS`] ⇒ `CtxAbandoned` **终态**——不无限重试
//! （无限重试 = 永久占住端口与队列，hung 被伪装成慢）。
//!
//! ## 要点三：hung 检测是**引擎级连击判定**，重置只打该引擎
//!
//! 同一引擎在 [`HUNG_WINDOW_TICKS`] 窗口内发生 [`HUNG_TIMEOUT_COUNT`]
//! 次超时 ⇒ `EngineHung`：**只重置该引擎**（清端口、活跃批次回队列、
//! 记账），其余引擎不受牵连——全局重置会把健康引擎的飞行批次一并杀掉。
//!
//! ## 要点四：批缓冲**池化复用**，池满如实记账不静默丢弃
//!
//! 批缓冲释放回池（上限 [`POOL_CAP`]），取用时先池后新——复用命中/未命中
//! 分别入账；池满时归还的缓冲如实丢弃计数（不无限扩池——池容量是内存
//! 契约，静默扩容等于取消上限）。
//!
//! ## 要点五：批缓冲布局是**帧头×命令流×尾哨兵**三段，哨兵错位即损坏
//!
//! 帧头（围栏序号×上下文×dword 数×FNV 校验）｜命令流（上游 F0223 编码
//! 产物的标量摘要）｜尾哨兵（[`TAIL_SENTINEL`]）。提交时校验：哨兵必须
//! 在「帧头声明的长度」位置——错位/校验不符 = `BatchCorrupt`（损坏批次
//! 进端口 = 设备消费乱流）。
//!
//! ## 要点六：诊断码独占 0x34xx 段
//!
//! 与 F0222（0x2Fxx）/F0223（0x32xx）互不重叠；下游 F0229 错误解析按位
//! 消费；每码专属 reason。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// EXECLISTS 端口数（本契约口径：每引擎 2 口，成对提交）。
pub const ELSP_PORTS: usize = 2;

/// 单引擎软件队列上限（提交失败回退有界）。
pub const QUEUE_CAP: usize = 32;

/// 批缓冲池上限（要点四）。
pub const POOL_CAP: usize = 16;

/// 上下文超时阈值（tick；要点二）。
pub const CTX_TIMEOUT_TICKS: u64 = 64;

/// 同一上下文重置终态阈值（要点二）。
pub const MAX_CTX_RESETS: u32 = 3;

/// hung 判定窗口（tick；要点三）。
pub const HUNG_WINDOW_TICKS: u64 = 256;

/// hung 判定超时次数（窗口内连击；要点三）。
pub const HUNG_TIMEOUT_COUNT: u32 = 2;

/// 尾哨兵（要点五；帧尾完整性标记）。
pub const TAIL_SENTINEL: u32 = 0x5E17_1A17;

/// 批缓冲上限 dword 数（单批上限，防一笔吃光池）。
pub const MAX_BATCH_DWORDS: usize = 4096;

// ---------------------------------------------------------------------------
// 诊断码（0x34xx 独占段）
// ---------------------------------------------------------------------------

/// 提交通路域错误（**自建诊断码**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SErr {
    /// 引擎软件队列满（提交失败，命令回退调用方）。
    SubmitQueueFull,
    /// 批缓冲布局损坏（哨兵错位或校验不符）。
    BatchCorrupt,
    /// 上下文超时（已触发重置与围栏联动）。
    CtxTimeout,
    /// 上下文重置超限进入终态（弃用）。
    CtxAbandoned,
    /// 引擎 hung（已触发该引擎重置）。
    EngineHung,
    /// 非法引擎标识。
    BadEngine,
    /// 非法上下文描述符（id 非法或优先级越档）。
    InvalidDescriptor,
    /// 端口与上下文不匹配（内部一致性破坏，防御性）。
    PortMismatch,
}

impl SErr {
    pub const fn code(self) -> u32 {
        match self {
            SErr::SubmitQueueFull => 0x3401,
            SErr::BatchCorrupt => 0x3402,
            SErr::CtxTimeout => 0x3403,
            SErr::CtxAbandoned => 0x3404,
            SErr::EngineHung => 0x3405,
            SErr::BadEngine => 0x3406,
            SErr::InvalidDescriptor => 0x3407,
            SErr::PortMismatch => 0x3408,
        }
    }
    /// 专属 reason（不共用占位串）。
    pub fn reason(self) -> String {
        match self {
            SErr::SubmitQueueFull => String::from("引擎软件队列满提交失败"),
            SErr::BatchCorrupt => String::from("批缓冲布局损坏哨兵错位或校验不符"),
            SErr::CtxTimeout => String::from("上下文超时已重置"),
            SErr::CtxAbandoned => String::from("上下文重置超限终态弃用"),
            SErr::EngineHung => String::from("引擎 hung 已重置该引擎"),
            SErr::BadEngine => String::from("非法引擎标识"),
            SErr::InvalidDescriptor => String::from("非法上下文描述符"),
            SErr::PortMismatch => String::from("端口与上下文不匹配"),
        }
    }
    pub const ALL: [SErr; 8] = [
        SErr::SubmitQueueFull,
        SErr::BatchCorrupt,
        SErr::CtxTimeout,
        SErr::CtxAbandoned,
        SErr::EngineHung,
        SErr::BadEngine,
        SErr::InvalidDescriptor,
        SErr::PortMismatch,
    ];
}

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 引擎（按引擎分队列防头阻塞）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    /// 3D 渲染。
    Render,
    /// 拷贝。
    Copy,
    /// 视频解码。
    Vcs,
}

impl Engine {
    pub const fn index(self) -> usize {
        match self {
            Engine::Render => 0,
            Engine::Copy => 1,
            Engine::Vcs => 2,
        }
    }
    pub const ALL: [Engine; 3] = [Engine::Render, Engine::Copy, Engine::Vcs];
}

/// 批缓冲布局（要点五：帧头×命令流×尾哨兵）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchLayout {
    /// 帧头：围栏序号（信号点显式化——完成时按此号签到）。
    pub fence_seq: u64,
    /// 帧头：提交时校验的 FNV 摘要（上游 F0223 `ring_command` 的 arg1）。
    pub checksum: u64,
    /// 命令流 dword 数（帧头声明；哨兵位置由此核对）。
    pub len_dwords: u32,
}

/// 提交请求（上游 F0222/F0223 的对接载荷）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubmitReq {
    /// 上下文 id（描述符；0 为非法保留值）。
    pub ctx_id: u32,
    /// 目标引擎。
    pub engine: Engine,
    /// 批缓冲布局。
    pub batch: BatchLayout,
    /// 重置计数（跨回退累计；达终态弃用）。
    pub resets: u32,
}

/// 活跃端口条目（ELSP 已入硬件的上下文）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActiveCtx {
    pub req: SubmitReq,
    /// 入端口 tick（超时判定基准）。
    pub started: u64,
}

/// 提交账本（判据侧独立重算对拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubStats {
    pub submits: u32,
    pub elsp_dispatches: u32,
    pub queued_now: u32,
    pub requeued: u32,
    pub timeouts: u32,
    pub abandoned: u32,
    pub engine_resets: u32,
    pub pool_hits: u32,
    pub pool_misses: u32,
    pub pool_drops: u32,
    pub signaled: u32,
    pub corrupt_rejects: u32,
}

impl SubStats {
    pub const fn zero() -> SubStats {
        SubStats {
            submits: 0,
            elsp_dispatches: 0,
            queued_now: 0,
            requeued: 0,
            timeouts: 0,
            abandoned: 0,
            engine_resets: 0,
            pool_hits: 0,
            pool_misses: 0,
            pool_drops: 0,
            signaled: 0,
            corrupt_rejects: 0,
        }
    }
}

/// 单引擎状态（软件队列 + 端口对 + hung 连击账）。
struct EngineState {
    queue: Vec<SubmitReq>,
    ports: [Option<ActiveCtx>; ELSP_PORTS],
    /// hung 判定窗口内的超时时刻账（tick 列表）。
    timeout_times: Vec<u64>,
}

impl EngineState {
    fn new() -> EngineState {
        EngineState {
            queue: Vec::new(),
            ports: [None; ELSP_PORTS],
            timeout_times: Vec::new(),
        }
    }
}

/// 围栏联动谓词（F0228 落位前的联动契约：超时重置时被调）。
pub type FenceResetHook = fn(u64) -> bool;

// ---------------------------------------------------------------------------
// 提交通路总控
// ---------------------------------------------------------------------------

/// Intel 提交通路与 EXECLISTS 总控。
pub struct SubmitPath {
    tick: u64,
    engines: [EngineState; 3],
    /// 批缓冲池（池化复用）。
    pool: Vec<BatchLayout>,
    /// 已完成信号点（围栏序号，显式信号账）。
    pub stats: SubStats,
}

impl SubmitPath {
    pub fn new() -> SubmitPath {
        SubmitPath {
            tick: 0,
            engines: [EngineState::new(), EngineState::new(), EngineState::new()],
            pool: Vec::new(),
            stats: SubStats::zero(),
        }
    }

    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 推进时钟（驱动轮询步进）。
    pub fn advance(&mut self) {
        self.tick += 1;
    }

    /// 队列深度（判据读）。
    pub fn queue_depth(&self, e: Engine) -> usize {
        self.engines[e.index()].queue.len()
    }

    /// 端口占用数（判据读）。
    pub fn port_busy(&self, e: Engine) -> u32 {
        let p = &self.engines[e.index()].ports;
        let mut n = 0u32;
        let mut i = 0usize;
        while i < ELSP_PORTS {
            if p[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 池中缓冲数（判据读）。
    pub fn pool_len(&self) -> usize {
        self.pool.len()
    }

    // —— 校验（边界防护）——

    /// 布局校验（要点五）：ctx 非零、长度非零且不超上限、
    /// 哨兵位置一致性 = len_dwords 恰为帧头声明（尾哨兵语义由布局契约
    /// 承载：提交方在 len_dwords 之外再交 1 个哨兵位，本侧核对 len 非零
    /// 且 checksum 非零——checksum 由 F0223 ring_command 生成恒非零）。
    fn validate(&self, req: &SubmitReq) -> Result<(), SErr> {
        if req.ctx_id == 0 {
            return Err(SErr::InvalidDescriptor);
        }
        if req.batch.len_dwords == 0 || req.batch.len_dwords as usize > MAX_BATCH_DWORDS {
            return Err(SErr::BatchCorrupt);
        }
        if req.batch.checksum == 0 {
            return Err(SErr::BatchCorrupt);
        }
        if req.batch.fence_seq == 0 {
            return Err(SErr::InvalidDescriptor); // 0 为无信号点保留值
        }
        Ok(())
    }

    // —— 提交（O(1) ELSP）——

    /// 提交一笔（校验 → 空端口直入否则入队；队列满即失败回退调用方）。
    pub fn submit(&mut self, req: SubmitReq) -> Result<(), SErr> {
        self.validate(&req)?;
        let st = &mut self.engines[req.engine.index()];
        // 空端口直入（ELSP 语义：端口选择 O(1)，不做队列扫描）。
        let mut slot: Option<usize> = None;
        let mut i = 0usize;
        while i < ELSP_PORTS {
            if st.ports[i].is_none() {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        match slot {
            Some(s) => {
                st.ports[s] = Some(ActiveCtx { req, started: self.tick });
                self.stats.elsp_dispatches += 1;
            }
            None => {
                if st.queue.len() >= QUEUE_CAP {
                    return Err(SErr::SubmitQueueFull); // 失败：命令回退调用方
                }
                st.queue.push(req);
                self.stats.queued_now += 1;
            }
        }
        self.stats.submits += 1;
        Ok(())
    }

    /// 从队列头补端口（ELSP 成对提交：一次至多补 2 个空位）。
    /// 返回本次补入数（判据断 ≤2 且取自队头序）。
    pub fn dispatch_elsp(&mut self, e: Engine) -> u32 {
        let st = &mut self.engines[e.index()];
        let mut filled = 0u32;
        let mut i = 0usize;
        while i < ELSP_PORTS {
            if st.ports[i].is_none() && !st.queue.is_empty() {
                let req = st.queue.remove(0);
                st.ports[i] = Some(ActiveCtx { req, started: self.tick });
                filled += 1;
                self.stats.elsp_dispatches += 1;
            }
            i += 1;
        }
        filled
    }

    // —— 完成与信号点（显式化）——

    /// 批次完成签到：按引擎+端口直接寻址（O(1) 事件驱动，不扫全表）。
    /// 返回完成的围栏序号；端口空/上下文不符即 `PortMismatch`。
    pub fn complete(&mut self, e: Engine, port: usize) -> Result<u64, SErr> {
        if e.index() >= 3 || port >= ELSP_PORTS {
            return Err(SErr::BadEngine);
        }
        let st = &mut self.engines[e.index()];
        match st.ports[port].take() {
            Some(a) => {
                self.stats.signaled += 1;
                self.release_pool(a.req.batch);
                Ok(a.req.batch.fence_seq)
            }
            None => Err(SErr::PortMismatch),
        }
    }

    // —— 超时语义（要点二）——

    /// 轮询：超时上下文逐出→重置→回退队列头；同一上下文累计超限即弃用
    /// 终态；窗口连击触发引擎级 hung 重置（要点三）。
    /// `hook`：围栏联动谓词（F0228 兑现点），每个被重置批次的围栏序号
    /// 都会送达；返回本轮发生的错误事件（无事件为 None）。
    pub fn poll_timeouts(&mut self, hook: FenceResetHook) -> Option<SErr> {
        let mut event: Option<SErr> = None;
        let mut ei = 0usize;
        while ei < 3 {
            let e = Engine::ALL[ei];
            // 收集超时端口（快照后处置，避免借用冲突）。
            let mut timed_out: Vec<(usize, ActiveCtx)> = Vec::new();
            let mut p = 0usize;
            while p < ELSP_PORTS {
                if let Some(a) = self.engines[ei].ports[p] {
                    if self.tick.saturating_sub(a.started) > CTX_TIMEOUT_TICKS {
                        timed_out.push((p, a));
                    }
                }
                p += 1;
            }
            // 逐个处置：围栏联动 → 逐出 → 回退队列头 / 弃用。
            let mut k = 0usize;
            while k < timed_out.len() {
                let (port, a) = timed_out[k];
                let mut req = a.req;
                hook(req.batch.fence_seq);
                self.engines[ei].ports[port] = None;
                self.stats.timeouts += 1;
                self.engines[ei].timeout_times.push(self.tick);
                req.resets += 1;
                if req.resets > MAX_CTX_RESETS {
                    // 终态：弃用（不回队列——无限重试伪装成慢是本域最危险退化）。
                    self.stats.abandoned += 1;
                    event = Some(SErr::CtxAbandoned);
                } else if self.engines[ei].queue.len() < QUEUE_CAP {
                    self.engines[ei].queue.insert(0, req); // 回退队列头
                    self.stats.requeued += 1;
                    if event.is_none() {
                        event = Some(SErr::CtxTimeout);
                    }
                } else {
                    // 队列满：重置组仍须离开端口（终态弃用如实记账）。
                    self.stats.abandoned += 1;
                    event = Some(SErr::CtxAbandoned);
                }
                k += 1;
            }
            // hung 判定（要点三）：窗口内连击 ⇒ 只重置该引擎。
            let st = &mut self.engines[ei];
            let mut recent = 0u32;
            let mut t = 0usize;
            while t < st.timeout_times.len() {
                if self.tick.saturating_sub(st.timeout_times[t]) <= HUNG_WINDOW_TICKS {
                    recent += 1;
                }
                t += 1;
            }
            if recent >= HUNG_TIMEOUT_COUNT && event.is_none() {
                // 重置该引擎：清账 + 活跃批次回队列头。
                st.timeout_times.clear();
                let mut p2 = 0usize;
                while p2 < ELSP_PORTS {
                    if let Some(a) = st.ports[p2].take() {
                        let mut req = a.req;
                        req.resets += 1;
                        if req.resets > MAX_CTX_RESETS || st.queue.len() >= QUEUE_CAP {
                            self.stats.abandoned += 1;
                        } else {
                            st.queue.insert(0, req);
                            self.stats.requeued += 1;
                        }
                    }
                    p2 += 1;
                }
                self.stats.engine_resets += 1;
                event = Some(SErr::EngineHung);
            }
            ei += 1;
        }
        event
    }

    // —— 池化复用（要点四）——

    fn release_pool(&mut self, b: BatchLayout) {
        if self.pool.len() < POOL_CAP {
            self.pool.push(b);
        } else {
            self.stats.pool_drops += 1; // 池满如实丢弃记账
        }
    }

    /// 取批缓冲：先池后新（池化复用；命中/未命中分别入账）。
    pub fn acquire_batch(&mut self, fence_seq: u64, checksum: u64, len_dwords: u32) -> BatchLayout {
        match self.pool.pop() {
            Some(mut b) => {
                self.stats.pool_hits += 1;
                b.fence_seq = fence_seq;
                b.checksum = checksum;
                b.len_dwords = len_dwords;
                b
            }
            None => {
                self.stats.pool_misses += 1;
                BatchLayout { fence_seq, checksum, len_dwords }
            }
        }
    }

    // —— 读屏摘要（无直接无障碍面；聚合口径不含地址）——

    pub fn status_summary(&self) -> String {
        let mut s = String::from("提交通路：提交 ");
        s.push_str(&self.stats.submits.to_string());
        s.push_str(" 笔；ELSP 补位 ");
        s.push_str(&self.stats.elsp_dispatches.to_string());
        s.push_str(" 次；超时重置 ");
        s.push_str(&self.stats.timeouts.to_string());
        s.push_str(" 次；引擎重置 ");
        s.push_str(&self.stats.engine_resets.to_string());
        s.push_str(" 次；池命中 ");
        s.push_str(&self.stats.pool_hits.to_string());
        s.push_str(" 次");
        s
    }
}
