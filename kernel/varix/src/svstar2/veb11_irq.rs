//! VE-F0211 · virtio 中断与事件处理（VE-B 域 · GPU 驱动矩阵 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0211`
//!
//! **判据（锚点原文）**：解耦入队、去重合并、轮询兜底、未知跳过、判据。
//!
//! # 职责
//!
//! virtio-gpu 中断与事件处理：
//! 1. **isr cfg 读取消除语义**——virtio 规范的 ISR 寄存器是「读到即消费」：
//!    一次读返回全部未决位并由硬件清零，读到的就是消费掉的，故二次读必为 0。
//!    本单元以 `read_and_clear_isr` 复刻该语义（返回值与自清同一次原子动作）。
//! 2. **display events 事件队列解析**——事件队列承载扫描输出状态变化等事件，
//!    本单元提供 `parse_event`（线格式 → `Event`）与 `IngestBatch`（按 virtio
//!    事件队列「先读长度再逐条读」两段式解析批量摄入），解析侧与中断侧解耦。
//! 3. **中断与工作线程解耦**——中断上下文只做 O(1) 置位与入队，零解析、零
//!    分配、零等待；全部解析与处理在工作线程侧（`ingest` / `drain`）。
//! 4. **同类事件短窗去重合并**——同 `(类型 × 扫描输出)` 在逻辑 tick 窗口内
//!    只收首个，后续合并并计数（查表 O(1)），防事件风暴。
//! 5. **事件风暴限流**——单位 tick 内入队量超阈值即限流（额外判据，防风暴
//!    合并之外的第二道闸），超限丢弃并计数，不静默。
//! 6. **中断丢失轮询兜底**——看门狗在 `poll_fallback` 开启且超过超时窗口未
//!    见中断时触发 `PollNow`（开关可配，默认关）。
//!
//! # 时间语义
//!
//! 全部用**逻辑 tick 注入**，不依赖墙钟——保证回归确定性。
//!
//! # 性能逐项分解（锚点原文）
//!
//! - 入队 O(1)：中断侧 `raise_irq` 仅一次按位或；`enqueue_event` 仅一次槽定位。
//! - 去重 O(1) 窗口查：直接映射表，无链表无扫描。
//! - 处理线程独立：中断上下文不跑解析、不跑 `drain`、不分配。
//!
//! # 错误路径与降级矩阵（锚点原文）
//!
//! - 未知事件类型 → 记录并跳过不崩溃（`EventOutcome::UnknownSkipped` + 计数公开）。
//! - 事件风暴 → 合并限流（去重合并 + 单位 tick 限流双闸，丢弃均计数）。
//! - 中断丢失 → 看门狗触发轮询兜底（`WatchdogAction::PollNow`）。
//! - 环满 → 覆盖最旧并计数（最新优先，与光标通道同一保序哲学，不静默丢）；
//!   **覆盖必连带释放被顶事件的去重窗口**——被顶事件从未送达消费者，其窗口
//!   若留存，同键后续事件会被误合并丢弃，消费者永远感知不到该键变化。
//! - 空读 → 记`empty_reads` 而非畸形（设备报「无事件」是正常态；混入畸形计数
//!   会淹没真畸形，诊断不说谎）。
//! - 去重表探测耗尽（表满）→ 放行但**不落窗**并计 `probe_exhausted`：宁可不合并，
//!   也不覆盖他键窗口致其失去合并保护（配置错配如实暴露供诊断）。
//!
//! # 隐私
//!
//! 事件不载用户内容——`Event` 仅含类型 / 扫描输出号 / 参数 / tick，无像素无文本。

use alloc::vec::Vec;

/// ISR 未决位：配置变化（virtio-gpu 用于通告事件队列有待读事件）。
pub const ISR_CONFIG_CHANGE: u32 = 1 << 0;
/// ISR 未决位：队列变化（本单元暂不消费，仅登记位形）。
pub const ISR_QUEUE_CHANGE: u32 = 1 << 1;

/// ISR 未决位的全部已知位掩码（用于 `is_known_isr_bits` 判定）。
pub const ISR_KNOWN_MASK: u32 = ISR_CONFIG_CHANGE | ISR_QUEUE_CHANGE;

/// 事件类别（virtio-gpu event queue 事件码形态；未知码保留原值不猜）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 扫描输出状态变化（display events 主事件）。
    Display,
    /// 光标事件。
    Cursor,
    /// 未知类型（记录并跳过——判据「未知跳过」）。
    Unknown(u32),
}

impl EventKind {
    /// 类别 → 线上事件码。
    pub fn code(self) -> u32 {
        match self {
            EventKind::Display => 0x0100,
            EventKind::Cursor => 0x0101,
            EventKind::Unknown(c) => c,
        }
    }

    /// 线上事件码 → 类别（未知码保留原值，不映射到已知类别）。
    pub fn of_code(code: u32) -> EventKind {
        match code {
            0x0100 => EventKind::Display,
            0x0101 => EventKind::Cursor,
            other => EventKind::Unknown(other),
        }
    }

    /// 是否为已知类别（未知跳过判据的快路径）。
    pub fn is_known(self) -> bool {
        !matches!(self, EventKind::Unknown(_))
    }

    pub fn label(self) -> &'static str {
        match self {
            EventKind::Display => "DISPLAY",
            EventKind::Cursor => "CURSOR",
            EventKind::Unknown(_) => "UNKNOWN",
        }
    }
}

/// 事件参数（锚点「事件队列（类型×输出×参数）」的第三维）。
///
/// 线格式上 display/cursor 事件首字为参数；本单元保留其原值不做语义猜测，
/// 由上层（呈现/光标通道）解释。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: EventKind,
    /// 扫描输出号（0 起；跨头语义见 F0213）。
    pub scanout: u32,
    /// 事件参数（线格式首字，原值透传）。
    pub param: u32,
    /// 摄入时的逻辑 tick。
    pub tick: u64,
}

/// 事件入队结果（每条路径都有显式返回值，不靠计数反推）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventOutcome {
    /// 接收入队。
    Accepted,
    /// 短窗内同类合并（丢弃本条，已计数）。
    Deduped,
    /// 未知事件类型：记录并跳过（不崩溃、不入环）。
    UnknownSkipped,
    /// 风暴限流丢弃（单位 tick 入队超阈值）。
    RateLimited,
    /// 扫描输出号越界（非法输入防护，不入环）。
    RejectedBadScanout,
}

/// 事件队列解析结果（批量摄入用；解析侧与中断侧解耦的证据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IngestBatch {
    /// 本批解析出的事件总数（按线格式长度字段）。
    pub parsed: u32,
    /// 实际入队数（含合并/限流/跳过的最终留环数）。
    pub enqueued: u32,
    /// 因未知类型跳过数。
    pub unknown: u32,
    /// 长度字段越界被判废的批次数（边界防护证据）。
    pub malformed: u32,
    /// 空读次数（设备报「无事件」的正常态）——与 `malformed` 分开计数，
    /// 否则正常空读会掺进畸形计数里，掩盖真畸形（诊断不说谎纪律）。
    pub empty: u32,
}

/// 看门狗动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogAction {
    /// 无动作（未开兜底或未超时）。
    None,
    /// ISR 超时无中断 → 本 tick 应走轮询兜底。
    PollNow,
}

/// 去重窗口槽数（直接映射 + 线性探测；探测步数有界 → 最坏仍 O(1)——锚点
/// 「去重 O(1) 窗口查」）。
///
/// 槽数取 64：virtio-gpu 最多 16 个扫描输出 × 2 个已知事件类别 = 32 个键，
/// 留一倍余量使常态下探测步数为 0~1。
const DEDUP_SLOTS: usize = 64;

/// 线性探测最大步数（超过即判表满——此时保守放行并计数，宁可少合并不可卡死）。
const DEDUP_MAX_PROBE: usize = 8;

/// 单位 tick 入队风暴阈值（合并之外的第二道闸）。
const STORM_LIMIT_PER_TICK: u32 = 16;

/// 去重表「从未接收」哨兵（全 1，与任何真实 tick 值不相撞）。
const DEDUP_NONE: u64 = u64::MAX;

/// virtio-gpu 中断与事件处理单元。
#[derive(Debug)]
pub struct VirtGpuIrq {
    /// ISR 未决位（读后自清）。
    isr_pending: u32,
    /// 事件环形队列（中断上下文入队 / 工作线程 drain）。
    ring: Vec<Option<Event>>,
    ring_head: usize,
    ring_tail: usize,
    ring_len: usize,
    /// 去重窗口表：`[(类别码, 扫描输出, 上次接收 tick)]`，0 tick 哨兵表示从未接收。
    dedup: [(u32, u32, u64); DEDUP_SLOTS],
    /// 去重窗口宽度（逻辑 tick）。
    window: u64,
    /// 看门狗超时（逻辑 tick）。
    watchdog_timeout: u64,
    /// 轮询兜底开关（判据「可开」；默认关——开了才有）。
    poll_fallback: bool,
    /// 最近一次中断 tick（看门狗基准）。
    last_irq_tick: u64,
    /// 最近一次兜底轮询 tick（避免兜底自身把看门狗基准顶上去而永不再触发）。
    last_poll_tick: u64,
    /// 上一个入队 tick（风暴限流的单位 tick 计数基准）。
    last_ingest_tick: u64,
    /// 当前 tick 内已入队数（风暴限流计数）。
    ingest_in_tick: u32,
    /// 设备声明的扫描输出数（扫描输出号合法性边界）。
    scanout_count: u32,
    // ---- 计数（遥测面：全部 pub，不静默丢） ----
    /// 入队成功累计。
    pub accepted: u64,
    /// 短窗合并累计。
    pub deduped: u64,
    /// 未知类型跳过累计。
    pub unknown_skipped: u64,
    /// 风暴限流丢弃累计。
    pub rate_limited: u64,
    /// 环满覆盖最旧累计。
    pub overwritten: u64,
    /// 被覆盖事件连带释放去重窗口的累计（覆盖必释窗——否则该键后续事件
    /// 会被「已见未处理」的窗口误合并丢弃，消费者永不感知该键变化）。
    pub window_released: u64,
    /// 去重表探测步数耗尽（表满）累计——该次放行不落窗，缺合并保护，
    /// 显性计数而非静默（表满是配置错配，如实暴露供诊断）。
    pub probe_exhausted: u64,
    /// 非法扫描输出号拒收累计。
    pub rejected_scanout: u64,
    /// 畸形批次拒收累计。
    pub malformed_batches: u64,
    /// 空读批次累计（设备报「无事件」的正常态；与畸形分列）。
    pub empty_reads: u64,
    /// 轮询兜底触发累计。
    pub poll_fallback_count: u64,
}

impl VirtGpuIrq {
    /// 构造。`ring_cap` 为事件环容量（0 视作 1）；`scanout_count` 为设备声明的
    /// 扫描输出数（0 表示不校验，由 `set_scanout_count` 后置设定）。
    pub fn new(ring_cap: usize, scanout_count: u32) -> VirtGpuIrq {
        VirtGpuIrq {
            isr_pending: 0,
            ring: (0..ring_cap.max(1)).map(|_| None).collect(),
            ring_head: 0,
            ring_tail: 0,
            ring_len: 0,
            dedup: [(0u32, 0u32, DEDUP_NONE); DEDUP_SLOTS],
            window: 2,
            watchdog_timeout: 16,
            poll_fallback: false,
            last_irq_tick: 0,
            last_poll_tick: 0,
            last_ingest_tick: 0,
            ingest_in_tick: 0,
            scanout_count,
            accepted: 0,
            deduped: 0,
            unknown_skipped: 0,
            rate_limited: 0,
            overwritten: 0,
            window_released: 0,
            probe_exhausted: 0,
            rejected_scanout: 0,
            malformed_batches: 0,
            empty_reads: 0,
            poll_fallback_count: 0,
        }
    }

    // ---- 配置面（工作线程侧调用，不在中断上下文） ----

    /// 设备 cfg 声明扫描输出数后置设定（扫描输出号合法性边界）。
    pub fn set_scanout_count(&mut self, n: u32) {
        self.scanout_count = n;
    }

    /// 设定去重窗口宽度（0 钳制为 1——窗口为 0 无意义且会让除零语义歧义）。
    pub fn set_window(&mut self, window: u64) {
        self.window = window.max(1);
    }

    pub fn window(&self) -> u64 {
        self.window
    }

    /// 设定看门狗超时（0 钳制为 1，与窗口同一口径）。
    pub fn set_watchdog_timeout(&mut self, t: u64) {
        self.watchdog_timeout = t.max(1);
    }

    pub fn watchdog_timeout(&self) -> u64 {
        self.watchdog_timeout
    }

    /// 轮询兜底开关（判据「轮询兜底可开」）。
    pub fn enable_poll_fallback(&mut self, on: bool) {
        self.poll_fallback = on;
        if on {
            // 开闸瞬间把基准对齐当前 tick，否则下一 tick 立即误触发兜底。
            self.last_poll_tick = self.last_irq_tick;
        }
    }

    pub fn poll_fallback_enabled(&self) -> bool {
        self.poll_fallback
    }

    // ---- 中断上下文面（只置位/入队，零解析零分配） ----

    /// 硬件置中断位（解耦纪律：只按位或 + 记 tick，O(1)）。
    pub fn raise_irq(&mut self, bit: u32, tick: u64) {
        self.isr_pending |= bit;
        self.last_irq_tick = tick;
    }

    /// isr cfg 读取消除（virtio 语义：读到即消费——返回未决位并清零，二次读必 0）。
    pub fn read_and_clear_isr(&mut self) -> u32 {
        core::mem::replace(&mut self.isr_pending, 0)
    }

    /// ISR 未决位是否含配置变化（决定是否需要去读事件队列）。
    pub fn isr_has(&self, mask: u32) -> bool {
        self.isr_pending & mask != 0
    }

    /// 中断上下文入队（O(1)：一次槽定位 + 一次窗口查 + 至多一次环写）。
    ///
    /// 顺序刻意为「未知 → 扫描输出合法性 → 去重窗口 → 风暴限流 → 入环」：
    /// 越廉价的判据越前置，中断上下文不做无用功。
    pub fn enqueue_event(&mut self, ev: Event) -> EventOutcome {
        // 1) 未知类型：记录并跳过（判据「未知跳过」），不入环不崩。
        if !ev.kind.is_known() {
            self.unknown_skipped += 1;
            return EventOutcome::UnknownSkipped;
        }
        // 2) 扫描输出号越界：拒绝（边界防护；cfg 未声明数量时 0 = 不校验）。
        if self.scanout_count != 0 && ev.scanout >= self.scanout_count {
            self.rejected_scanout += 1;
            return EventOutcome::RejectedBadScanout;
        }
        // 3) 短窗去重合并：同 (类型码, 扫描输出) 在窗口内只收首个（查表 O(1)）。
        //    探测而非直接映射——避免槽冲突把某键的窗口记录顶掉致漏合并。
        let code = ev.kind.code();
        // 落窗槽：Hit/Free 可落窗；Exhausted（表满）不落窗。
        let mut land = None;
        match dedup_probe(&self.dedup, code, ev.scanout) {
            DedupSlot::Hit(slot) => {
                let (_, _, ktick) = self.dedup[slot];
                // 窗口内（tick 差 < window）即合并。tick 回退由 saturating_sub 兜住。
                if ev.tick.saturating_sub(ktick) < self.window {
                    self.deduped += 1;
                    return EventOutcome::Deduped;
                }
                land = Some(slot);
            }
            DedupSlot::Free(slot) => land = Some(slot),
            DedupSlot::Exhausted => {
                // 探测步数耗尽（表满）：如实计数后放行，但**不落窗**——落窗等于
                // 覆盖 start 槽的原键记录，那会让原键失去合并保护（风暴下重复
                // 事件漏合并）。此处宁可不合并也不夺他键窗口。
                self.probe_exhausted += 1;
            }
        }
        // 4) 风暴限流：单位 tick 入队超阈值即丢弃（合并之外的第二道闸）。
        if ev.tick != self.last_ingest_tick {
            self.last_ingest_tick = ev.tick;
            self.ingest_in_tick = 0;
        }
        if self.ingest_in_tick >= STORM_LIMIT_PER_TICK {
            self.rate_limited += 1;
            return EventOutcome::RateLimited;
        }
        self.ingest_in_tick += 1;
        // 5) 入环形槽（O(1)）；环满覆盖最旧并计数（不静默丢）。
        if self.ring_len == self.ring.len() {
            // 覆盖前必须释放被顶掉事件的去重窗口：那条事件从未到达消费者，
            // 若其窗口仍在，同键的后续事件会被误判「已见未处理」而合并丢弃，
            // 消费者就永远感知不到该键的变化（事件静默丢失）。
            if let Some(old) = self.ring[self.ring_head] {
                if dedup_forget(&mut self.dedup, old.kind.code(), old.scanout, old.tick) {
                    self.window_released += 1;
                }
            }
            self.ring[self.ring_head] = None;
            self.ring_head = (self.ring_head + 1) % self.ring.len();
            self.ring_len -= 1;
            self.overwritten += 1;
        }
        if let Some(slot) = land {
            self.dedup[slot] = (code, ev.scanout, ev.tick);
        }
        self.ring[self.ring_tail] = Some(ev);
        self.ring_tail = (self.ring_tail + 1) % self.ring.len();
        self.ring_len += 1;
        self.accepted += 1;
        EventOutcome::Accepted
    }

    // ---- 工作线程面（解析与处理，解耦于此） ----

    /// 事件队列解析：线格式 `[code_le32, scanout_le32, param_le32] * n` 逐条转 `Event`。
    ///
    /// 畸形处置（边界防护，不 panic 不崩溃）：
    /// - 长度为 0 或非 3 的倍数 → 整批判废，`malformed_batches` 递增，返回全零批；
    /// - 单条扫描输出号越界 → 该条拒收（不拖累同批其余条目）；
    /// - 单条未知码 → 该条跳过计数（判据「未知跳过」在解析侧同样生效）。
    ///
    /// 此函数在中断侧**不**调用（解耦判据）：中断只置位，工作线程才解析。
    pub fn parse_event(&self, bytes: &[u8]) -> Event {
        let le32 = |off: usize| -> u32 {
            let mut b = [0u8; 4];
            // 越界读零（调用方已保证 off+4 <= len，此处仅防御）
            if off + 4 <= bytes.len() {
                b.copy_from_slice(&bytes[off..off + 4]);
            }
            u32::from_le_bytes(b)
        };
        Event {
            kind: EventKind::of_code(le32(0)),
            scanout: le32(4),
            param: le32(8),
            tick: self.last_irq_tick,
        }
    }

    /// 按 virtio 事件队列两段式（先读长度，再逐条读）批量摄入。
    ///
    /// `len_bytes` 为长度字段的线格式（u32 LE）——`None` 表示长度字段不可信。
    pub fn ingest(&mut self, len_bytes: Option<&[u8]>, body: &[u8], tick: u64) -> IngestBatch {
        let mut batch = IngestBatch {
            parsed: 0,
            enqueued: 0,
            unknown: 0,
            malformed: 0,
            empty: 0,
        };
        // 长度字段不可信或越界 → 整批判废。
        let declared = match len_bytes {
            None => {
                batch.malformed = 1;
                self.malformed_batches += 1;
                return batch;
            }
            Some(b) if b.len() < 4 => {
                batch.malformed = 1;
                self.malformed_batches += 1;
                return batch;
            }
            Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize,
        };
        const REC: usize = 12; // 每条事件线格式字节数
                               // 声明长度为 0 = 设备报「无事件」，是正常空读（不记畸形）。
        if declared == 0 {
            self.empty_reads += 1;
            batch.empty = 1;
            return batch;
        }
        if declared % REC != 0 || declared > body.len() {
            batch.malformed = 1;
            self.malformed_batches += 1;
            return batch;
        }
        // 双道防线：即便上面的长度校验被绕过，也按实际缓冲长度夹紧条数——
        // 单道校验一旦失效就会越界切片 panic（内核里等于宕机），故此处不依赖它。
        let count = (declared / REC).min(body.len() / REC);
        batch.parsed = count as u32;
        for i in 0..count {
            let off = i * REC;
            let mut ev = self.parse_event(&body[off..off + REC]);
            ev.tick = tick;
            match self.enqueue_event(ev) {
                EventOutcome::Accepted => batch.enqueued += 1,
                EventOutcome::UnknownSkipped => batch.unknown += 1,
                EventOutcome::RejectedBadScanout => {}
                EventOutcome::Deduped | EventOutcome::RateLimited => {}
            }
        }
        batch
    }

    /// 工作线程消费：取走全部已入队事件（解耦纪律——处理只在这里发生）。
    pub fn drain(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while self.ring_len > 0 {
            let ev = self.ring[self.ring_head].take();
            self.ring_head = (self.ring_head + 1) % self.ring.len();
            self.ring_len -= 1;
            if let Some(e) = ev {
                // 事件已送达消费者 → 释放其去重窗口。
                // 不释放的后果：消费者已处理完该键，窗口却仍在，后续同键真实
                // 状态变化会被判「窗口内重复」合并丢弃——事件静默丢失，
                // 消费者永远看不到这次变化（实测可复现）。
                if dedup_forget(&mut self.dedup, e.kind.code(), e.scanout, e.tick) {
                    self.window_released += 1;
                }
                out.push(e);
            }
        }
        out
    }

    /// 消费并逐条交给处理侧（零中间分配路径；返回处理条数）。
    pub fn drain_with<F: FnMut(&Event)>(&mut self, mut f: F) -> usize {
        let mut n = 0;
        while self.ring_len > 0 {
            let ev = self.ring[self.ring_head].take();
            self.ring_head = (self.ring_head + 1) % self.ring.len();
            self.ring_len -= 1;
            if let Some(e) = ev {
                // 同 `drain`：交付即释放窗口，否则后续同键事件会被误合并丢弃。
                if dedup_forget(&mut self.dedup, e.kind.code(), e.scanout, e.tick) {
                    self.window_released += 1;
                }
                f(&e);
                n += 1;
            }
        }
        n
    }

    // ---- 看门狗与轮询兜底 ----

    /// 看门狗 tick：开兜底且距上次中断（或上次兜底）超时 → 触发轮询兜底。
    ///
    /// 基准取 `max(last_irq_tick, last_poll_tick)`：兜底自身消费事件后必须推进
    /// 基准，否则兜底会把自己刚喂的那口气当成「中断已恢复」，形成假绿。
    pub fn watchdog_tick(&mut self, tick: u64) -> WatchdogAction {
        if !self.poll_fallback {
            return WatchdogAction::None;
        }
        let base = if self.last_poll_tick > self.last_irq_tick {
            self.last_poll_tick
        } else {
            self.last_irq_tick
        };
        if tick.saturating_sub(base) > self.watchdog_timeout {
            self.poll_fallback_count += 1;
            self.last_poll_tick = tick;
            WatchdogAction::PollNow
        } else {
            WatchdogAction::None
        }
    }

    /// 轮询兜底采集（中断已丢时的替代读事件队列路径）。
    ///
    /// 入参 `body` 为事件队列记录区（每条 12 字节），长度由 `body.len()` 自身
    /// 给出——兜底路径是「轮询式整区读」，不经过设备侧长度字段，故不走
    /// `ingest` 的两段式（否则必被判废）。
    ///
    /// 关键纪律：**不伪造中断**——`poll_once` 不改 `last_irq_tick`；是否补记
    /// 「已恢复」由调用方显式 `note_poll_recovered` 决定（真收到中断才补），
    /// 否则兜底会自证成功（自己把自己当成中断源）。
    pub fn poll_once(&mut self, body: &[u8], tick: u64) -> IngestBatch {
        // 空队列是**正常态**（无待读事件），不是畸形——记进 `malformed` 会让
        // 兜底轮询的正常空读污染畸形计数，真畸形反而被淹没（诊断不说谎）。
        if body.is_empty() {
            self.empty_reads += 1;
            return IngestBatch {
                parsed: 0,
                enqueued: 0,
                unknown: 0,
                malformed: 0,
                empty: 1,
            };
        }
        if !body.len().is_multiple_of(12) {
            self.malformed_batches += 1;
            return IngestBatch {
                parsed: 0,
                enqueued: 0,
                unknown: 0,
                malformed: 1,
                empty: 0,
            };
        }
        let lf = (body.len() as u32).to_le_bytes();
        self.ingest(Some(&lf), body, tick)
    }

    /// 兜底轮询确实取到数据后调用：推进「已恢复」基准，停止连续兜底。
    pub fn note_poll_recovered(&mut self, tick: u64) {
        self.last_poll_tick = tick;
    }

    /// ISR 未决位是否为已知位形（未知位登记而非静默）。
    pub fn is_known_isr_bits(bits: u32) -> bool {
        bits & !ISR_KNOWN_MASK == 0
    }

    pub fn ring_len(&self) -> usize {
        self.ring_len
    }

    /// 最近一次中断 tick（看门狗基准只读面——自检与诊断用）。
    pub fn last_irq_tick(&self) -> u64 {
        self.last_irq_tick
    }

    pub fn ring_cap(&self) -> usize {
        self.ring.len()
    }

    /// 待处理积压量（工作线程调度用；达阈值即该 drain）。
    pub fn backlog(&self) -> usize {
        self.ring_len
    }
}

/// 去重槽起始位：乘法散列把 (code, scanout) 摊到槽空间。
fn dedup_slot(code: u32, scanout: u32) -> usize {
    let mixed = (code ^ 0x9E37_79B9).wrapping_mul(0x85EB_CA6B) ^ scanout.wrapping_mul(0x27D4_EB2F);
    (mixed >> 4) as usize % DEDUP_SLOTS
}

/// 去重槽定位结果。
///
/// **为什么必须三态而非一个 bool**：「本键没有记录（可落窗）」与「探测步数
/// 耗尽（表满，不可落窗）」在仅有一个 `found: bool` 时都表现为 `false`，
/// 混淆二者会让「首次出现的键」也走不落窗分支——该键从此永不参与合并，
/// 风暴下去重形同虚设。故拆为三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DedupSlot {
    /// 命中同键既有记录（`usize` 为槽号，可刷新窗口）。
    Hit(usize),
    /// 探测到空槽（`usize` 为槽号），本键尚未落表，可落窗。
    Free(usize),
    /// 探测步数耗尽（表满）——无可用槽，本次放行且**不落窗**。
    Exhausted,
}

/// 去重槽定位：直接映射 + 有界线性探测，仍 O(1)。
///
/// **为什么必须探测**：直接映射下两个不同键会落同一槽，后写者会顶掉前者的
/// 窗口记录——虽不致误合并（比较的是完整 `(码, 输出)` 元组），但被顶掉的键
/// 会**丢失合并保护**（风暴下重复事件漏合并）。故探测到空槽或同键槽为止，
/// 步数上界 `DEDUP_MAX_PROBE` 保证最坏 O(1)。
fn dedup_probe(table: &[(u32, u32, u64); DEDUP_SLOTS], code: u32, scanout: u32) -> DedupSlot {
    let start = dedup_slot(code, scanout);
    let mut idx = start;
    for step in 0..DEDUP_MAX_PROBE {
        let e = table[idx];
        // 空槽（从未接收）→ 该键未落表，可落窗
        if e.2 == DEDUP_NONE {
            return DedupSlot::Free(idx);
        }
        // 同键 → 命中已有记录
        if e.0 == code && e.1 == scanout {
            return DedupSlot::Hit(idx);
        }
        idx = (start + step + 1) % DEDUP_SLOTS;
    }
    // 探测步数耗尽（表满）：不落窗——落窗会覆盖 start 槽原键的窗口。
    DedupSlot::Exhausted
}

/// 按 `(码, 输出, tick)` 三元组精确作废一条去重窗口记录。
///
/// **为什么必须三元组全等才作废**：窗口槽可能被同键的新事件刷新过（tick 不同）。
/// 若只按 `(码, 输出)` 作废，会把「已入队的新事件」的窗口一起清掉，导致该键
/// 立刻可再收一条——同一键在环里已有未消费事件时又入一条，恢复成风暴前语义。
/// 全等才保证「作废的正是那条从未送达消费者的事件的窗口」。
///
/// 返回是否真的作废了一条（未命中返回 false，不静默假装成功）。
fn dedup_forget(
    table: &mut [(u32, u32, u64); DEDUP_SLOTS],
    code: u32,
    scanout: u32,
    tick: u64,
) -> bool {
    let start = dedup_slot(code, scanout);
    let mut idx = start;
    for step in 0..DEDUP_MAX_PROBE {
        let e = table[idx];
        if e.2 == DEDUP_NONE {
            return false;
        }
        if e.0 == code && e.1 == scanout && e.2 == tick {
            table[idx] = (0, 0, DEDUP_NONE);
            return true;
        }
        idx = (start + step + 1) % DEDUP_SLOTS;
    }
    false
}

/// VE-F0211 判据自检域聚合入口（登记于 `svstar2::checks`）。
pub fn run_veb11_checks() -> crate::checks::CheckSet {
    super::veb11_checks::run_veb11_checks()
}
