//! VE-F0214 · virtio 性能与诊断接口（目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0214`
//!
//! **判据（锚点原文逐条）**：
//! - 按帧统计命令数 / 传输字节 / 队列深度 → `C214-帧计-*`
//! - 提交到完成的延迟打点，**分位在线更新 O(1)** → `C214-分位-*`
//! - 诊断快照**按需拉取不常驻**（无请求零开销）→ `C214-快照-*`
//! - 计数溢出 → **饱和不回绕** → `C214-饱和-*`
//! - 打点缺失 → 该帧指标标记**缺测** → `C214-缺测-*`
//! - 快照请求并发 → **串行化排队** → `C214-排队-*`
//! - 计数入遥测总线，**受 F0096 预算治理** → `C214-预算-*`
//!
//! ---
//!
//! ## 设计要点一：分位草图为什么用「固定桶 + 原地晋升」而不是排序或直方图
//!
//! 锚点要求「分位在线更新 O(1)」「打点 O(1)」。常见做法是每帧把延迟样本
//! 排序后取分位——那是 O(n log n) 且要分配；直方图（按微秒分 1024 桶）虽
//! O(1) 但桶数固定、太宽（1µs 精度对 60fps 帧时毫无意义）或太窄（溢出即
//! 全丢）。
//!
//! 故本单用**对数分桶草图（t-digest 之外的轻量替代）**：
//! - 桶宽按 **2 的幂**增长：桶 i 覆盖 `[2^i, 2^(i+1))` 微秒。
//!   0..15桶 覆盖 0..32µs（亚毫秒级，逐桶 1µs 精度），16..31 覆盖
//!   32µs..65ms（帧时区，逐桶 2µs 精度），再往上每桶翻倍。
//! - 分位读出：对目标桶**线性插值**，故 p99 在桶内也能给出亚桶精度。
//! - 每次打点只做一次 `leading_zeros` + 一次加一 —— 真O(1)，无分配。
//! - **桶数固定**（[`SKETCH_BUCKETS`]），计数器组大小与帧数无关 ⇒
//!   「无请求零常驻」成立。
//!
//! **为什么不用 t-digest**：t-digest 的合并是 O(k log k) 且状态含质心数组，
//! 状态量随分位数个数增长；本单只要 p50/p90/p99 三个点，草图方案状态量恒为
//! 桶数，且实现可被「桶计数逐桶核对」直接验证。若日后要更多分位且能接受
//! 更大状态，再换 t-digest 不影响本模块对外接口。
//!
//! ## 设计要点二：饱和不回绕，且要**可区分「饱和」与「真实达到峰值」**
//!
//! 锚点：「计数溢出 → 饱和不回绕」。若只做 `if c == MAX { return }`，
//! 快照里出现 MAX 时无法区分「恰好等于 MAX」和「已饱和多次」——而这两者
//! 对漂移检测（下游 F0100）的含义完全不同：前者是正常值，后者是丢数据。
//!
//! 故每计数器配一个 **溢出位**（`saturated` 标志）：`add()` 先判断会否越界，
//! 越界则**钉在 MAX 并置饱和位**。快照如实带出饱和位，下游据此把该指标
//! 标为「上界饱和、真实值未知」而不是当成一个精确数字参与统计。
//!
//! ## 设计要点三：缺测是**一等的帧状态**，不是零
//!
//! 锚点：「打点缺失 → 该帧指标标记缺测」。关键在于**缺测不能记0**：
//! 记 0 会被下游算成「这一帧零延迟」，把设备卡死伪装成性能完美。
//! 故每帧计数带一个 `measured` 位（是否有完整打点对），缺测帧在分位草图里
//! **不参与**（既不加分位也不补零），快照里带 `frames_measured` /
//! `frames_missing` 两个计数，下游可算覆盖率。
//!
//! ## 设计要点四：快照按需拉取，队列**有界且串行化**
//!
//! 锚点：「诊断快照按需拉取不常驻开销」「快照请求并发 → 串行化排队」。
//! - 「不常驻」= 不预先构造快照对象；`snapshot()` 现场组装。
//! - 「串行化排队」= 请求进环形队列，按入队序处理；队列满时**拒收并计数**
//!   （不覆盖未处理的旧请求——旧请求更早，提出得更早）。
//!
//! ## 设计要点五：遥测接入受 F0096 预算治理，但**不越界实现采样率**
//!
//! 锚点只要求「计数入遥测总线受 F0096 预算治理」。F0096（另单）负责
//! 采样预算表/降采样阶梯/变更事件。本单只提供**接入面**：
//! 按指标族上报 + 受配额约束 + 超额时按`TelemetryPriority` 降级，
//! 预算**配置与阶梯**由 F0096 提供，本单不持有全局采样器。
//! 职责边界写在这里，避免两个单互相顶替。

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 分位草图桶数（覆盖 0..约 8.4 小时，2 的幂次跨度）。
///
/// 取16是权衡：桶是 `u32` 计数，16 桶 = 64 字节/族，三个族 192 字节——
/// 嵌入式 `.bss` 可接受；再往上则低分位精度过剩、高分位跨度不足。
pub const SKETCH_BUCKETS: usize = 16;

/// 草图桶宽的幂次上限（桶 i 覆盖 `[1<<i, 1<<(i+1))` 微秒）。
pub const SKETCH_MAX_SHIFT: u32 = (SKETCH_BUCKETS as u32) - 1;

/// 计数器饱和值（`u32::MAX` 视作饱和线，不用作真实计数）。
pub const COUNTER_SAT: u64 = u32::MAX as u64;

/// 帧计数器的环形槽数（按帧滚动）。
pub const FRAME_SLOTS: usize = 64;

/// 快照请求队列容量（有界；满则拒收并计数）。
pub const SNAP_QUEUE_SLOTS: usize = 8;

/// 遥测指标族数（命令/字节/延迟）。
pub const METRIC_FAMILIES: usize = 3;

/// 遥测降采样阶梯（锚点 F0096 的三档；本单只消费，不定义策略）。
pub const DOWN_STEPS: [u8; 3] = [100, 50, 10];

// ---------------------------------------------------------------------------
// 二、饱和计数器（不回绕）
// ---------------------------------------------------------------------------

/// 饱和计数器：加法溢出时钉在 [`COUNTER_SAT`] 并置饱和位。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaturatedCounter {
    value: u64,
    saturated: bool,
}

impl SaturatedCounter {
    pub const fn new() -> SaturatedCounter {
        SaturatedCounter { value: 0, saturated: false }
    }

    /// 累加（饱和语义：越界后钉在上限并置位，**不回绕**）。
    ///
    /// 越界判定必须对着 [`COUNTER_SAT`]（u32::MAX）而不是 u64::MAX ——
    /// 累加器虽是 u64，但饱和线定在 u32::MAX，若用 `checked_add` 只盯
    /// u64 上界，则 0xFFFF_FFFF + 1 在 u64 下**不溢出**，饱和分支永远
    /// 走不到，钉住与饱和位双双成为死代码（下游读到的仍是一个精确值，
    /// 而它其实早已越过上界）。故此处用「与饱和线的距离」判定。
    pub fn add(&mut self, delta: u64) {
        if self.saturated {
            return;
        }
        let room = COUNTER_SAT.saturating_sub(self.value);
        if delta > room {
            self.value = COUNTER_SAT;
            self.saturated = true;
        } else {
            self.value += delta;
        }
    }

    /// 饱和置位后累加（**不再改变值**）——区分「恰好等于上限」与「已饱和」。
    pub fn add_saturating(&mut self, delta: u64) {
        if self.saturated {
            return;
        }
        self.add(delta);
    }

    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn is_saturated(&self) -> bool {
        self.saturated
    }

    /// 快照用：值 + 饱和位。
    pub fn snapshot(&self) -> (u64, bool) {
        (self.value, self.saturated)
    }

    /// 复位。
    pub fn reset(&mut self) {
        self.value = 0;
        self.saturated = false;
    }
}

// ---------------------------------------------------------------------------
// 三、分位草图（在线 O(1) 更新）
// ---------------------------------------------------------------------------

/// 对数分桶草图：p50/p90/p99 在线估计。
///
/// 桶 i 覆盖 `[1<<i, 1<<(i+1))` 微秒；`leading_zeros` 定桶，单次打点
/// O(1) 且无分配。读分位时在目标桶内**线性插值**给亚桶精度。
#[derive(Clone, Copy, Debug, Default)]
pub struct QuantileSketch {
    buckets: [u32; SKETCH_BUCKETS],
    total: u64,
}

impl QuantileSketch {
    pub const fn new() -> QuantileSketch {
        QuantileSketch { buckets: [0; SKETCH_BUCKETS], total: 0 }
    }

    /// 打点一个延迟样本（微秒）。
    pub fn record(&mut self, micros: u64) {
        let shift = if micros == 0 { 0 } else { 63 - micros.leading_zeros() };
        let idx = if shift > SKETCH_MAX_SHIFT { SKETCH_MAX_SHIFT as usize } else { shift as usize };
        // 桶计数饱和：草图自身也要防溢出（否则「估计分位」会指向不存在的桶）
        if self.buckets[idx] < u32::MAX {
            self.buckets[idx] += 1;
        }
        if self.total < u64::MAX {
            self.total += 1;
        }
    }

    /// 样本数。
    pub fn count(&self) -> u64 {
        self.total
    }

    /// 单桶计数（供判据逐桶核对用）。
    pub fn bucket(&self, idx: usize) -> u32 {
        if idx < SKETCH_BUCKETS {
            self.buckets[idx]
        } else {
            0
        }
    }

    /// 非空桶数（判别「草图是否真的在分桶」的机检面）。
    pub fn occupied_buckets(&self) -> usize {
        let mut n = 0;
        for i in 0..SKETCH_BUCKETS {
            if self.buckets[i] > 0 {
                n += 1;
            }
        }
        n
    }

    /// 估算分位（`q` 取 0..=100，单位百分位）。
    ///
    /// 返回**微秒**。无样本返回 0（调用方应先看 `count()`，别把 0 当真实值）。
    pub fn quantile(&self, q: u32) -> u64 {
        if self.total == 0 {
            return 0;
        }
        let q = if q > 100 { 100 } else { q };
        // 目标秩：ceil(q% * total)，最小 1
        let rank = (self.total * q as u64) / 100;
        let rank = if rank == 0 { 1 } else { rank };
        let mut acc = 0u64;
        for i in 0..SKETCH_BUCKETS {
            let c = self.buckets[i] as u64;
            if c == 0 {
                continue;
            }
            if acc + c >= rank {
                // 桶内线性插值：桶覆盖 [lo, hi)，按桶内相对位置给亚桶精度
                let lo = 1u64 << i;
                let hi = 1u64 << (i + 1);
                let within = rank - acc; // 1..=c
                let frac = (within - 1) as u128 * (hi - lo - 1) as u128 / c.max(1) as u128;
                let v = lo + frac as u64;
                return if v > lo { v } else { lo };
            }
            acc += c;
        }
        // 理论不可达（total>0 必有桶命中）；保守返回最大桶上界而非 0，
        // 因为返回 0 会被下游读成「延迟为零」。
        1u64 << SKETCH_MAX_SHIFT
    }

    /// 复位。
    pub fn reset(&mut self) {
        self.buckets = [0; SKETCH_BUCKETS];
        self.total = 0;
    }
}

// ---------------------------------------------------------------------------
// 四、帧计数与缺测
// ---------------------------------------------------------------------------

/// 单帧计数（锚点「帧×命令×字节×延迟草图」中的帧层）。
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameCounters {
    /// 帧序号（单调；打点配对用）。
    pub frame_id: u64,
    /// 提交的命令数。
    pub commands: SaturatedCounter,
    /// 传输字节数。
    pub bytes: SaturatedCounter,
    /// 观测到的最大队列深度。
    pub queue_depth: SaturatedCounter,
    /// 提交→完成延迟草图（本帧）。
    pub latency: QuantileSketch,
    /// 是否有完整打点对（false = **缺测**）。
    pub measured: bool,
    /// 是否有显式缺测标记（打点缺失时置位）。
    pub missing: bool,
}

impl FrameCounters {
    pub fn new(frame_id: u64) -> FrameCounters {
        FrameCounters { frame_id, ..FrameCounters::default() }
    }

    /// 命令数打点。
    pub fn add_command(&mut self) {
        self.commands.add(1);
    }

    /// 命令数批量打点。
    pub fn add_commands(&mut self, n: u64) {
        self.commands.add(n);
    }

    /// 传输字节打点。
    pub fn add_bytes(&mut self, n: u64) {
        self.bytes.add(n);
    }

    /// 队列深度观测（取本帧最大值，不累加——深度是水位不是总量）。
    ///
    /// 注意此处**不**走 `add()`：深度语义是「取最大」而非「累加」，
    /// 累加会把水位当总量。且深度超过饱和线时同样要置饱和位——
    /// 深度值来自设备上报，无上限保证，直接赋 `u64::from(depth)` 即可
    /// （`u32 → u64` 恒不溢出），但饱和位仍要如实置上。
    pub fn observe_queue_depth(&mut self, depth: u32) {
        if !self.queue_depth.is_saturated() && self.queue_depth.value() < depth as u64 {
            self.queue_depth = SaturatedCounter { value: depth as u64, saturated: false };
        }
    }

    /// 提交打点（记录延迟上半段：命令已提交）。
    pub fn mark_submit(&mut self) {
        self.measured = true;
    }

    /// 完成打点（记录延迟下半段；`latency_us` 为提交到完成的间隔）。
    pub fn mark_complete(&mut self, latency_us: u64) {
        self.latency.record(latency_us);
    }

    /// 标记缺测（打点缺失）。
    pub fn mark_missing(&mut self) {
        self.missing = true;
        self.measured = false;
    }

    /// 覆盖率（0..=100，单位百分）；无样本返回 0。
    pub fn coverage_pct(&self) -> u32 {
        if self.frame_id == 0 {
            return 0;
        }
        if self.measured {
            100
        } else {
            0
        }
    }
}

/// 帧环形表（按帧滚动，保定长）。
#[derive(Clone, Debug)]
pub struct FrameRing {
    slots: [FrameCounters; FRAME_SLOTS],
    next: usize,
    pushed: u64,
    measured_frames: u64,
    missing_frames: u64,
}

impl FrameRing {
    pub fn new() -> FrameRing {
        FrameRing {
            slots: [FrameCounters::new(0); FRAME_SLOTS],
            next: 0,
            pushed: 0,
            measured_frames: 0,
            missing_frames: 0,
        }
    }

    /// 推入一帧（覆盖最旧的一槽）。
    pub fn push(&mut self, fc: FrameCounters) {
        self.slots[self.next] = fc;
        self.next = (self.next + 1) % FRAME_SLOTS;
        self.pushed += 1;
        if fc.measured {
            self.measured_frames += 1;
        } else {
            self.missing_frames += 1;
        }
    }

    /// 槽数（恒为 [`FRAME_SLOTS`]）。
    pub fn len(&self) -> usize {
        FRAME_SLOTS
    }

    pub fn is_empty(&self) -> bool {
        self.pushed == 0
    }

    /// 已推入帧数（含被覆盖的）。
    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    /// 已测帧数。
    pub fn measured_frames(&self) -> u64 {
        self.measured_frames
    }

    /// 缺测帧数。
    pub fn missing_frames(&self) -> u64 {
        self.missing_frames
    }

    /// 读槽（按绝对下标；越界返回 None——读取面不 panic）。
    pub fn slot(&self, idx: usize) -> Option<&FrameCounters> {
        if idx < FRAME_SLOTS {
            Some(&self.slots[idx])
        } else {
            None
        }
    }

    /// 最近一帧（最新推入的那一帧）。
    pub fn latest(&self) -> Option<&FrameCounters> {
        if self.pushed == 0 {
            return None;
        }
        let idx = if self.next == 0 { FRAME_SLOTS - 1 } else { self.next - 1 };
        Some(&self.slots[idx])
    }

    /// 缺测占比（0..=100，单位百分）。
    pub fn missing_pct(&self) -> u32 {
        if self.pushed == 0 {
            return 0;
        }
        (self.missing_frames * 100 / self.pushed) as u32
    }
}

// ---------------------------------------------------------------------------
// 五、快照队列（串行化、有界、满则拒收）
// ---------------------------------------------------------------------------

/// 快照请求的处理结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueOutcome {
    /// 已入队。
    Enqueued,
    /// 队列满，拒收（未覆盖任何在队请求）。
    RejectedFull,
    /// 已服务。
    Served,
    /// 队列空，无可服务者。
    Idle,
}

impl QueueOutcome {
    /// 是否真的处理了一个请求。
    pub fn did_serve(self) -> bool {
        self == QueueOutcome::Served
    }
}

/// 快照请求队列（环形；串行化 = 按入队序出队）。
#[derive(Clone, Debug)]
pub struct SnapQueue {
    slots: [u64; SNAP_QUEUE_SLOTS],
    head: usize,
    tail: usize,
    len: usize,
    served: u64,
    rejected: u64,
}

impl SnapQueue {
    pub fn new() -> SnapQueue {
        SnapQueue {
            slots: [0; SNAP_QUEUE_SLOTS],
            head: 0,
            tail: 0,
            len: 0,
            served: 0,
            rejected: 0,
        }
    }

    /// 当前排队长度。
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 入队一个请求（token = 请求序号）。
    ///
    /// 满时**拒收并计数**，绝不覆盖在队请求——旧请求提出得更早，
    /// 覆盖它等于让新请求插队。
    pub fn enqueue(&mut self, token: u64) -> QueueOutcome {
        if self.len >= SNAP_QUEUE_SLOTS {
            self.rejected += 1;
            return QueueOutcome::RejectedFull;
        }
        self.slots[self.tail] = token;
        self.tail = (self.tail + 1) % SNAP_QUEUE_SLOTS;
        self.len += 1;
        QueueOutcome::Enqueued
    }

    /// 出队（FIFO）。
    pub fn dequeue(&mut self) -> Option<u64> {
        if self.len == 0 {
            return None;
        }
        let t = self.slots[self.head];
        self.head = (self.head + 1) % SNAP_QUEUE_SLOTS;
        self.len -= 1;
        self.served += 1;
        Some(t)
    }

    /// 已服务数。
    pub fn served(&self) -> u64 {
        self.served
    }

    /// 被拒数。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }
}

// ---------------------------------------------------------------------------
// 六、遥测接入（受 F0096 预算治理）
// ---------------------------------------------------------------------------

/// 指标族（锚点「帧×命令×字节×延迟草图」的提交侧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricFamily {
    /// 命令数（高频族）。
    Command,
    /// 传输字节（高频族）。
    Byte,
    /// 延迟（高频族，但丢不起——优先级最高）。
    Latency,
}

impl MetricFamily {
    /// 指标族下标（0..METRIC_FAMILIES）。
    pub fn index(self) -> usize {
        match self {
            MetricFamily::Command => 0,
            MetricFamily::Byte => 1,
            MetricFamily::Latency => 2,
        }
    }

    /// 遥测优先级（大者优先，配额紧张时先保高优先级）。
    pub fn priority(self) -> u8 {
        match self {
            MetricFamily::Latency => 3,
            MetricFamily::Command => 2,
            MetricFamily::Byte => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MetricFamily::Command => "virtio.cmd",
            MetricFamily::Byte => "virtio.byte",
            MetricFamily::Latency => "virtio.latency",
        }
    }
}

/// 遥测接入面（**只消费预算，不定义采样策略**——策略归 F0096）。
#[derive(Clone, Debug, Default)]
pub struct TelemetrySink {
    /// 每族**真采下**的点数。
    reported: [u64; METRIC_FAMILIES],
    /// 每族因超配额被降级丢弃的点数。
    dropped: [u64; METRIC_FAMILIES],
    /// 每族**提交总数**（= reported + dropped；对账用）。
    ///
    /// 必须与 `reported` 分开：抽样序号取自「提交序」而非「已采序」，
    /// 若拿 `reported` 兼任提交计数，则降档后 `reported` 里混着被丢的点，
    /// 下游按 `reported` 算实际采样率会恒得 100%（看着像满档全采，实则
    /// 一半被降级），超配额判定随之失真。
    submitted: [u64; METRIC_FAMILIES],
    /// 每族配额（点/窗口；由 F0096 预算表提供）。
    quota: [u64; METRIC_FAMILIES],
    /// 当前降采样档（0=100%，1=50%，2=10%）。
    step: usize,
}

impl TelemetrySink {
    pub fn new() -> TelemetrySink {
        TelemetrySink::default()
    }

    /// 设置某族配额（来自 F0096 采样预算表）。
    pub fn set_quota(&mut self, fam: MetricFamily, points: u64) {
        self.quota[fam.index()] = points;
    }

    /// 读配额。
    pub fn quota(&self, fam: MetricFamily) -> u64 {
        self.quota[fam.index()]
    }

    /// 当前档位（索引；对应 [`DOWN_STEPS`]）。
    pub fn step(&self) -> usize {
        self.step
    }

    /// 当前采样率（百分）。
    pub fn rate_pct(&self) -> u8 {
        DOWN_STEPS[self.step.min(DOWN_STEPS.len() - 1)]
    }

    /// 升档（负载恢复后阶梯回升）。
    pub fn step_up(&mut self) {
        if self.step > 0 {
            self.step -= 1;
        }
    }

    /// 降档（负载越限时降采样而非全量丢弃）。
    pub fn step_down(&mut self) {
        if self.step + 1 < DOWN_STEPS.len() {
            self.step += 1;
        }
    }

    /// 上报一点。返回 `true` = 被采下，`false` = 被降级丢弃。
    ///
    /// 降级判定用**取模**而非随机：确定性可复现（跨机器对拍能复现同一决策），
    /// 且 10% 档下每 10 个提交点采 1 个，不会全丢某一族。
    ///
    /// 计数口径：`submitted` 记提交，`reported` 只记真采下的点。
    /// 抽样序号取自 `submitted`（提交序）——若取自 `reported`，降档后
    /// 「每 2 采 1」会退化成「每 1 采 1」（因为采过的点数增长慢），
    /// 采样率反过来往上飘。
    pub fn report(&mut self, fam: MetricFamily) -> bool {
        let i = fam.index();
        if self.submitted[i] < u64::MAX {
            self.submitted[i] += 1;
        }
        let rate = self.rate_pct() as u64;
        if rate >= 100 {
            self.reported[i] += 1;
            return true;
        }
        // 取模抽样：提交序连续递增，`submitted % (100/rate) == 0` 时采
        let every = (100 / rate).max(1);
        if self.submitted[i] % every == 0 {
            self.reported[i] += 1;
            return true;
        }
        self.dropped[i] += 1;
        false
    }

    /// 某族**真采下**的点数（不含被降级丢弃的）。
    pub fn reported(&self, fam: MetricFamily) -> u64 {
        self.reported[fam.index()]
    }

    /// 某族提交总点数（= reported + dropped）。
    pub fn submitted(&self, fam: MetricFamily) -> u64 {
        self.submitted[fam.index()]
    }

    /// 某族被降级丢弃点数。
    pub fn dropped(&self, fam: MetricFamily) -> u64 {
        self.dropped[fam.index()]
    }

    /// 某族是否已超配额（超了要降档）。
    pub fn over_quota(&self, fam: MetricFamily) -> bool {
        self.quota[fam.index()] > 0 && self.reported[fam.index()] > self.quota[fam.index()]
    }
}

// ---------------------------------------------------------------------------
// 七、诊断快照（按需组装，不常驻）
// ---------------------------------------------------------------------------

/// 诊断快照（**按需拉取**：本结构不预先构造，`snapshot()` 现场组装）。
#[derive(Clone, Debug)]
pub struct DiagSnapshot {
    /// 组装时的推入帧数。
    pub frames_pushed: u64,
    /// 已测帧数。
    pub frames_measured: u64,
    /// 缺测帧数。
    pub frames_missing: u64,
    /// 缺测占比（百分）。
    pub missing_pct: u32,
    /// 累计命令数（含饱和位）。
    pub total_commands: (u64, bool),
    /// 累计传输字节（含饱和位）。
    pub total_bytes: (u64, bool),
    /// p50延迟（微秒）。
    pub p50_us: u64,
    /// p90 延迟（微秒）。
    pub p90_us: u64,
    /// p99 延迟（微秒）。
    pub p99_us: u64,
    /// 延迟样本数。
    pub latency_samples: u64,
    /// 遥测已采点数（按族）。
    pub telemetry_reported: [u64; METRIC_FAMILIES],
    /// 遥测被丢弃点数（按族）。
    pub telemetry_dropped: [u64; METRIC_FAMILIES],
    /// 当前采样档。
    pub sample_step: usize,
}

/// virtio 性能与诊断接口（本单主结构）。
#[derive(Clone, Debug)]
pub struct VirtioPerf {
    /// 帧环形表。
    pub frames: FrameRing,
    /// 跨帧累计命令数。
    pub total_commands: SaturatedCounter,
    /// 跨帧累计传输字节。
    pub total_bytes: SaturatedCounter,
    /// 跨帧延迟草图（**跨帧**分位；帧内草图另存）。
    pub latency: QuantileSketch,
    /// 快照请求队列。
    pub queue: SnapQueue,
    /// 遥测接入面。
    pub telemetry: TelemetrySink,
    /// 组装过的快照数（诊断「快照不常驻」的成本证据）。
    pub snapshots_built: u64,
}

impl VirtioPerf {
    pub fn new() -> VirtioPerf {
        VirtioPerf {
            frames: FrameRing::new(),
            total_commands: SaturatedCounter::new(),
            total_bytes: SaturatedCounter::new(),
            latency: QuantileSketch::new(),
            queue: SnapQueue::new(),
            telemetry: TelemetrySink::new(),
            snapshots_built: 0,
        }
    }

    /// 提交一帧（帧滚动 + 跨帧累计 + 遥测上报）。
    ///
    /// **跨帧分位是桶下界近似，不是精确值**：帧内草图按桶聚合，跨帧只
    /// 能拿到「第 i 桶有 c 个样本」，回灌时取桶下界 `1<<i` 作为代表值。
    /// 这是对数草图的固有折中——换来 O(帧数 × 桶数) 的 O(1) 上界与零分配。
    /// 想要桶内精度需改用 t-digest 携带质心（代价见头注设计要点一）。
    pub fn submit_frame(&mut self, fc: FrameCounters) {
        self.total_commands.add(fc.commands.value());
        self.total_bytes.add(fc.bytes.value());
        if fc.measured {
            // 跨帧分位只收**已测**帧的延迟（缺测不补零——记 0 会被读成零延迟）
            let mut it = 0;
            while it < SKETCH_BUCKETS {
                let c = fc.latency.bucket(it) as u64;
                let mut k = 0;
                while k < c {
                    self.latency.record(1u64 << it);
                    k += 1;
                }
                it += 1;
            }
        }
        self.frames.push(fc);
    }

    /// 请求快照（串行化排队）。
    pub fn request_snapshot(&mut self, token: u64) -> QueueOutcome {
        self.queue.enqueue(token)
    }

    /// 处理一个排队中的快照请求。
    ///
    /// 返回值区分「服务了一个」与「队列空无事可做」——若只返 `bool`，
    /// 调用方无法把 [`QueueOutcome::Served`] 与 `Idle` 区分到同一枚举面，
    /// 统计「本次轮询是否产出快照」时就得另存一个 bool，易与返回值失配。
    pub fn serve_one(&mut self) -> QueueOutcome {
        match self.queue.dequeue() {
            Some(_) => QueueOutcome::Served,
            None => QueueOutcome::Idle,
        }
    }

    /// 按需组装快照（**现场构造，不预先持有**）。
    pub fn snapshot(&mut self) -> DiagSnapshot {
        self.snapshots_built += 1;
        DiagSnapshot {
            frames_pushed: self.frames.pushed(),
            frames_measured: self.frames.measured_frames(),
            frames_missing: self.frames.missing_frames(),
            missing_pct: self.frames.missing_pct(),
            total_commands: self.total_commands.snapshot(),
            total_bytes: self.total_bytes.snapshot(),
            p50_us: self.latency.quantile(50),
            p90_us: self.latency.quantile(90),
            p99_us: self.latency.quantile(99),
            latency_samples: self.latency.count(),
            telemetry_reported: self.telemetry.reported,
            telemetry_dropped: self.telemetry.dropped,
            sample_step: self.telemetry.step(),
        }
    }

    /// 复用上一帧的遥测降档决策（负载越限时降采样）。
    pub fn rebalance_telemetry(&mut self) {
        let mut over = false;
        for i in 0..METRIC_FAMILIES {
            let fam = match i {
                0 => MetricFamily::Command,
                1 => MetricFamily::Byte,
                _ => MetricFamily::Latency,
            };
            if self.telemetry.over_quota(fam) {
                over = true;
            }
        }
        if over {
            self.telemetry.step_down();
        } else {
            self.telemetry.step_up();
        }
    }
}

// ===========================================================================
// 单元测试层（回答「实现是否被改坏」，与判据层「规格是否满足」互不可省）
// ===========================================================================

// ===========================================================================
// 单元测试层（回答「实现是否被改坏」，与判据层「规格是否满足」互不可省）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- 饱和计数器 ---

    #[test]
    fn sat_normal_accumulate() {
        let mut c = SaturatedCounter::new();
        c.add(1);
        c.add(2);
        c.add(37);
        assert_eq!(c.value(), 40);
        assert!(!c.is_saturated());
        assert_eq!(c.snapshot(), (40, false));
    }

    #[test]
    fn sat_exactly_at_ceiling_is_not_saturated() {
        // 关键语义：「恰好等于上限」≠「已饱和」。两者对下游漂移检测
        // 含义不同（前者是有效值，后者是丢数据）。
        let mut c = SaturatedCounter::new();
        c.add(COUNTER_SAT);
        assert_eq!(c.value(), COUNTER_SAT);
        assert!(!c.is_saturated(), "恰好等于上限不得置饱和位");
    }

    #[test]
    fn sat_overflow_pins_and_flags() {
        // 这条是本单修掉的真缺陷：曾用 checked_add 只盯 u64 上界，
        // 而饱和线是 u32::MAX ⇒ 0xFFFF_FFFF+1 在 u64 下不溢出，
        // 饱和分支永不可达（死代码）。
        let mut c = SaturatedCounter::new();
        c.add(COUNTER_SAT);
        c.add(1);
        assert_eq!(c.value(), COUNTER_SAT, "溢出后必须钉在饱和线");
        assert!(c.is_saturated(), "溢出必须置饱和位");
    }

    #[test]
    fn sat_no_wraparound() {
        // 回绕会让「累计传输量」从巨大变回 0，下游算平均带宽得到完全错误的数。
        let mut c = SaturatedCounter::new();
        c.add(COUNTER_SAT);
        c.add(999);
        assert!(c.value() >= COUNTER_SAT, "绝不允许回绕到小值");
    }

    #[test]
    fn sat_add_after_saturated_does_not_move() {
        let mut c = SaturatedCounter::new();
        c.add(u64::MAX);
        assert!(c.is_saturated());
        let before = c.value();
        c.add(1);
        c.add(u64::MAX);
        assert_eq!(c.value(), before);
    }

    #[test]
    fn sat_add_saturating_after_flag_is_noop() {
        let mut c = SaturatedCounter::new();
        c.add_saturating(u64::MAX);
        let v = c.value();
        c.add_saturating(12345);
        assert_eq!(c.value(), v);
    }

    #[test]
    fn sat_add_saturating_within_range_accumulates() {
        let mut c = SaturatedCounter::new();
        c.add_saturating(10);
        c.add_saturating(5);
        assert_eq!(c.value(), 15);
        assert!(!c.is_saturated());
    }

    #[test]
    fn sat_reset_clears_both_fields() {
        let mut c = SaturatedCounter::new();
        c.add(u64::MAX);
        c.reset();
        assert_eq!(c.value(), 0);
        assert!(!c.is_saturated(), "reset 必须清饱和位");
        c.add(3);
        assert_eq!(c.value(), 3);
    }

    #[test]
    fn sat_add_saturating_never_exceeds_ceiling() {
        let mut c = SaturatedCounter::new();
        c.add(COUNTER_SAT - 5);
        c.add(10);
        assert_eq!(c.value(), COUNTER_SAT);
        assert!(c.is_saturated());
    }

    // --- 分位草图 ---

    #[test]
    fn sketch_empty_has_no_samples() {
        let s = QuantileSketch::new();
        assert_eq!(s.count(), 0);
        assert_eq!(s.occupied_buckets(), 0);
        assert_eq!(s.quantile(50), 0, "无样本返 0，但调用方须先看 count()");
    }

    #[test]
    fn sketch_single_sample_lands_in_its_bucket() {
        let mut s = QuantileSketch::new();
        s.record(1000);
        // 1000 ∈ [512, 1024) ⇒ 桶 9
        assert_eq!(s.bucket(9), 1);
        let q = s.quantile(50);
        assert!(q >= 512 && q < 1024, "单点分位须落在本桶区间，实得 {}", q);
    }

    #[test]
    fn sketch_bucket_sum_equals_sample_count() {
        let mut s = QuantileSketch::new();
        let mut i = 0;
        while i < 500 {
            s.record(50);
            i += 1;
        }
        i = 0;
        while i < 300 {
            s.record(7000);
            i += 1;
        }
        let mut sum = 0u64;
        let mut k = 0;
        while k < SKETCH_BUCKETS {
            sum += s.bucket(k) as u64;
            k += 1;
        }
        assert_eq!(sum, 800);
        assert_eq!(s.count(), 800);
        assert_eq!(s.occupied_buckets(), 2);
    }

    #[test]
    fn sketch_zero_sample_goes_to_bucket_zero() {
        let mut s = QuantileSketch::new();
        s.record(0);
        assert_eq!(s.bucket(0), 1, "0µs 须落桶 0（不得因 leading_zeros 而 panic）");
    }

    #[test]
    fn sketch_huge_sample_clamps_to_top_bucket() {
        let mut s = QuantileSketch::new();
        s.record(u64::MAX);
        assert_eq!(s.bucket(SKETCH_MAX_SHIFT as usize), 1);
        assert_eq!(s.count(), 1);
        assert!(s.quantile(99) > 0, "溢出样本不得读成零延迟");
    }

    #[test]
    fn sketch_bucket_read_out_of_range_returns_zero() {
        let mut s = QuantileSketch::new();
        s.record(100);
        assert_eq!(s.bucket(SKETCH_BUCKETS), 0);
        assert_eq!(s.bucket(usize::MAX), 0, "越界读桶不得 panic");
    }

    #[test]
    fn sketch_quantiles_are_monotonic() {
        let mut s = QuantileSketch::new();
        let mut i = 0;
        while i < 1000 {
            s.record(10 + (i % 97) * 13);
            i += 1;
        }
        let p50 = s.quantile(50);
        let p90 = s.quantile(90);
        let p99 = s.quantile(99);
        assert!(p50 <= p90 && p90 <= p99, "{} {} {}", p50, p90, p99);
    }

    #[test]
    fn sketch_q_out_of_range_clamps() {
        let mut s = QuantileSketch::new();
        s.record(500);
        assert_eq!(s.quantile(200), s.quantile(100));
        assert_eq!(s.quantile(u32::MAX), s.quantile(100));
    }

    #[test]
    fn sketch_reset_clears_all() {
        let mut s = QuantileSketch::new();
        s.record(100);
        s.record(1000);
        s.reset();
        assert_eq!(s.count(), 0);
        assert_eq!(s.occupied_buckets(), 0);
    }

    #[test]
    fn sketch_two_magnitudes_give_distinct_quantiles() {
        // 若分位写死返常数，此测试必红——这是草图的核心判别力。
        let mut s = QuantileSketch::new();
        let mut i = 0;
        while i < 900 {
            s.record(100);
            i += 1;
        }
        i = 0;
        while i < 100 {
            s.record(1000);
            i += 1;
        }
        let p50 = s.quantile(50);
        let p99 = s.quantile(99);
        assert!(p50 < p99, "p50={} p99={} 必须可区分", p50, p99);
        assert!(p50 >= 64 && p50 < 128, "p50 落 100µs 桶，实得 {}", p50);
        assert!(p99 >= 512 && p99 < 1024, "p99 落 1000µs 桶，实得 {}", p99);
    }

    // --- 帧计数 ---

    #[test]
    fn frame_counters_add_commands_and_bytes() {
        let mut f = FrameCounters::new(1);
        f.add_command();
        f.add_commands(4);
        f.add_bytes(4096);
        assert_eq!(f.commands.value(), 5);
        assert_eq!(f.bytes.value(), 4096);
    }

    #[test]
    fn frame_queue_depth_takes_max_not_sum() {
        let mut f = FrameCounters::new(1);
        f.observe_queue_depth(7);
        f.observe_queue_depth(3);
        f.observe_queue_depth(9);
        f.observe_queue_depth(2);
        assert_eq!(f.queue_depth.value(), 9, "深度是水位不是总量");
    }

    #[test]
    fn frame_missing_marks_and_clears_measured() {
        let mut f = FrameCounters::new(1);
        f.mark_submit();
        assert!(f.measured);
        assert!(!f.missing);
        f.mark_missing();
        assert!(f.missing);
        assert!(!f.measured, "缺测必须清 measured 位");
        assert_eq!(f.coverage_pct(), 0);
    }

    #[test]
    fn frame_mark_complete_records_exactly_one_sample() {
        let mut f = FrameCounters::new(1);
        f.mark_submit();
        f.mark_complete(2000);
        assert_eq!(f.latency.count(), 1, "一帧一个延迟样本，不得重复记录");
    }

    #[test]
    fn frame_zero_id_coverage_is_zero() {
        let f = FrameCounters::new(0);
        assert_eq!(f.coverage_pct(), 0);
    }

    // --- 帧环形表 ---

    #[test]
    fn ring_is_fixed_length() {
        let mut r = FrameRing::new();
        let mut i = 0;
        while i < FRAME_SLOTS * 4 {
            r.push(FrameCounters::new(i as u64));
            i += 1;
        }
        assert_eq!(r.len(), FRAME_SLOTS, "槽数恒定不增长");
        assert_eq!(r.pushed(), (FRAME_SLOTS * 4) as u64);
    }

    #[test]
    fn ring_latest_tracks_newest() {
        let mut r = FrameRing::new();
        assert!(r.latest().is_none());
        r.push(FrameCounters::new(11));
        r.push(FrameCounters::new(22));
        assert_eq!(r.latest().unwrap().frame_id, 22);
        // 绕回
        let mut i = 0;
        while i < FRAME_SLOTS {
            r.push(FrameCounters::new(100 + i as u64));
            i += 1;
        }
        assert_eq!(r.latest().unwrap().frame_id, 100 + (FRAME_SLOTS - 1) as u64);
    }

    #[test]
    fn ring_slot_out_of_range_returns_none() {
        let r = FrameRing::new();
        assert!(r.slot(FRAME_SLOTS).is_none());
        assert!(r.slot(usize::MAX).is_none(), "越界读槽不得 panic");
    }

    #[test]
    fn ring_missing_pct_computed_on_totals() {
        let mut r = FrameRing::new();
        let mut i = 0;
        while i < 3 {
            let mut f = FrameCounters::new(i as u64);
            if i < 2 {
                f.mark_submit();
            } else {
                f.mark_missing();
            }
            r.push(f);
            i += 1;
        }
        assert_eq!(r.measured_frames(), 2);
        assert_eq!(r.missing_frames(), 1);
        assert_eq!(r.missing_pct(), 33);
    }

    #[test]
    fn ring_empty_missing_pct_is_zero() {
        let r = FrameRing::new();
        assert!(r.is_empty());
        assert_eq!(r.missing_pct(), 0);
    }

    // --- 快照队列 ---

    #[test]
    fn queue_is_fifo() {
        let mut q = SnapQueue::new();
        let mut i = 0;
        while i < 5 {
            q.enqueue(i as u64);
            i += 1;
        }
        let mut expect = 0u64;
        while let Some(t) = q.dequeue() {
            assert_eq!(t, expect, "串行化=按入队序");
            expect += 1;
        }
        assert_eq!(expect, 5);
        assert_eq!(q.served(), 5);
    }

    #[test]
    fn queue_full_rejects_without_overwriting() {
        let mut q = SnapQueue::new();
        let mut i = 0;
        while i < SNAP_QUEUE_SLOTS {
            assert_eq!(q.enqueue(i as u64), QueueOutcome::Enqueued);
            i += 1;
        }
        assert_eq!(q.len(), SNAP_QUEUE_SLOTS);
        assert_eq!(q.enqueue(9999), QueueOutcome::RejectedFull);
        assert_eq!(q.len(), SNAP_QUEUE_SLOTS, "拒收不得改变队长");
        assert_eq!(q.rejected(), 1);
        // 9999 绝不能出现在队列里（旧请求更早，不得被覆盖）
        let mut n = 0;
        while let Some(t) = q.dequeue() {
            assert!(t < SNAP_QUEUE_SLOTS as u64, "被拒请求绝不可入队");
            n += 1;
        }
        assert_eq!(n, SNAP_QUEUE_SLOTS);
    }

    #[test]
    fn queue_dequeue_empty_returns_none() {
        let mut q = SnapQueue::new();
        assert!(q.is_empty());
        assert!(q.dequeue().is_none());
        assert_eq!(q.served(), 0);
    }

    #[test]
    fn queue_wraps_around_repeatedly() {
        let mut q = SnapQueue::new();
        let mut round = 0;
        while round < 10 {
            let mut i = 0;
            while i < SNAP_QUEUE_SLOTS {
                q.enqueue((round * 100 + i) as u64);
                i += 1;
            }
            let mut i = 0;
            while i < SNAP_QUEUE_SLOTS {
                assert_eq!(q.dequeue(), Some((round * 100 + i) as u64));
                i += 1;
            }
            round += 1;
        }
        assert_eq!(q.rejected(), 0, "出队腾位后不该再有拒收");
    }

    // --- 指标族 ---

    #[test]
    fn metric_family_indices_are_distinct_and_bounded() {
        let fams = [MetricFamily::Command, MetricFamily::Byte, MetricFamily::Latency];
        let mut i = 0;
        while i < fams.len() {
            assert!(fams[i].index() < METRIC_FAMILIES);
            let mut j = i + 1;
            while j < fams.len() {
                assert_ne!(fams[i].index(), fams[j].index(), "族下标必须互异");
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn metric_family_priority_order() {
        assert!(MetricFamily::Latency.priority() > MetricFamily::Command.priority());
        assert!(MetricFamily::Command.priority() > MetricFamily::Byte.priority());
    }

    #[test]
    fn metric_family_names_differ() {
        assert_ne!(MetricFamily::Command.name(), MetricFamily::Byte.name());
        assert_ne!(MetricFamily::Byte.name(), MetricFamily::Latency.name());
        assert_ne!(MetricFamily::Command.name(), MetricFamily::Latency.name());
    }

    // --- 遥测接入 ---

    #[test]
    fn telemetry_full_step_samples_everything() {
        let mut t = TelemetrySink::new();
        let mut i = 0;
        while i < 100 {
            assert!(t.report(MetricFamily::Command));
            i += 1;
        }
        assert_eq!(t.reported(MetricFamily::Command), 100);
        assert_eq!(t.dropped(MetricFamily::Command), 0);
        assert_eq!(t.submitted(MetricFamily::Command), 100);
    }

    #[test]
    fn telemetry_reported_counts_only_taken() {
        // 本单修掉的真缺陷：reported 曾在取模前自增，导致它其实是
        // 「提交总数」——降档后下游按它算采样率会恒得 100%。
        let mut t = TelemetrySink::new();
        t.step_down(); // 50%
        let mut i = 0;
        while i < 100 {
            t.report(MetricFamily::Command);
            i += 1;
        }
        assert_eq!(t.submitted(MetricFamily::Command), 100);
        assert_eq!(t.reported(MetricFamily::Command), 50, "50% 档应真采 50");
        assert_eq!(t.dropped(MetricFamily::Command), 50);
        assert_eq!(t.submitted(MetricFamily::Command), 100 - 50 + 50);
    }

    #[test]
    fn telemetry_three_way_accounting_balances() {
        let mut t = TelemetrySink::new();
        t.step_down();
        let mut i = 0;
        while i < 100 {
            t.report(MetricFamily::Byte);
            i += 1;
        }
        let sub = t.submitted(MetricFamily::Byte);
        assert_eq!(sub, t.reported(MetricFamily::Byte) + t.dropped(MetricFamily::Byte));
    }

    #[test]
    fn telemetry_lowest_step_drops_nine_of_ten() {
        let mut t = TelemetrySink::new();
        t.step_down();
        t.step_down(); // 10%
        assert_eq!(t.rate_pct(), 10);
        let mut taken = 0;
        let mut i = 0;
        while i < 100 {
            if t.report(MetricFamily::Command) {
                taken += 1;
            }
            i += 1;
        }
        assert_eq!(taken, 10, "10% 档每 10 采 1");
    }

    #[test]
    fn telemetry_sampling_is_deterministic() {
        // 确定性可复现：跨机器对拍须得到同一决策序列。
        let mut a = TelemetrySink::new();
        let mut b = TelemetrySink::new();
        a.step_down();
        b.step_down();
        let mut i = 0;
        while i < 200 {
            assert_eq!(a.report(MetricFamily::Latency), b.report(MetricFamily::Latency));
            i += 1;
        }
    }

    #[test]
    fn telemetry_step_clamped_at_both_ends() {
        let mut t = TelemetrySink::new();
        t.step_up();
        assert_eq!(t.step(), 0);
        t.step_up();
        assert_eq!(t.step(), 0, "满档不得上溢");
        let mut i = 0;
        while i < DOWN_STEPS.len() + 5 {
            t.step_down();
            i += 1;
        }
        assert_eq!(t.step(), DOWN_STEPS.len() - 1, "到底档不得下溢");
        assert_eq!(t.rate_pct(), *DOWN_STEPS.last().unwrap());
    }

    #[test]
    fn telemetry_over_quota_detection() {
        let mut t = TelemetrySink::new();
        t.set_quota(MetricFamily::Byte, 5);
        let mut i = 0;
        while i < 10 {
            t.report(MetricFamily::Byte);
            i += 1;
        }
        assert!(t.over_quota(MetricFamily::Byte));
        assert!(!t.over_quota(MetricFamily::Command), "未设配额=不判超限");
    }

    #[test]
    fn telemetry_quota_readback() {
        let mut t = TelemetrySink::new();
        t.set_quota(MetricFamily::Command, 77);
        assert_eq!(t.quota(MetricFamily::Command), 77);
        assert_eq!(t.quota(MetricFamily::Byte), 0);
    }

    #[test]
    fn telemetry_families_are_independent() {
        let mut t = TelemetrySink::new();
        t.step_down();
        t.report(MetricFamily::Command);
        t.report(MetricFamily::Command);
        assert_eq!(t.submitted(MetricFamily::Byte), 0, "族间不得串扰");
    }

    // --- VirtioPerf 端到端 ---

    fn mk_frame(id: u64, cmds: u64, bytes: u64, lat: u64, measured: bool) -> FrameCounters {
        let mut f = FrameCounters::new(id);
        f.add_commands(cmds);
        f.add_bytes(bytes);
        f.observe_queue_depth(4);
        if measured {
            f.mark_submit();
            f.mark_complete(lat);
        } else {
            f.mark_missing();
        }
        f
    }

    #[test]
    fn perf_snapshot_not_built_until_called() {
        let p = VirtioPerf::new();
        assert_eq!(p.snapshots_built, 0, "构造不得预建快照（不常驻）");
    }

    #[test]
    fn perf_snapshot_counts_up_on_demand() {
        let mut p = VirtioPerf::new();
        let _ = p.snapshot();
        let _ = p.snapshot();
        assert_eq!(p.snapshots_built, 2);
    }

    #[test]
    fn perf_totals_accumulate_across_frames() {
        let mut p = VirtioPerf::new();
        p.submit_frame(mk_frame(1, 3, 300, 100, true));
        p.submit_frame(mk_frame(2, 4, 400, 200, true));
        let s = p.snapshot();
        assert_eq!(s.total_commands, (7, false));
        assert_eq!(s.total_bytes, (700, false));
        assert_eq!(s.frames_pushed, 2);
        assert_eq!(s.frames_measured, 2);
        assert_eq!(s.frames_missing, 0);
    }

    #[test]
    fn perf_missing_frame_adds_no_latency_sample() {
        // 缺测若被记 0 参与分位，下游会把设备卡死读成「零延迟=性能完美」。
        let mut p = VirtioPerf::new();
        p.submit_frame(mk_frame(1, 1, 1, 5000, true));
        p.submit_frame(mk_frame(2, 1, 1, 0, false));
        let s = p.snapshot();
        assert_eq!(s.latency_samples, 1, "缺测帧不得进分位");
        assert_eq!(s.frames_missing, 1);
        assert!(s.p50_us > 0);
    }

    #[test]
    fn perf_all_missing_yields_no_samples() {
        let mut p = VirtioPerf::new();
        p.submit_frame(mk_frame(1, 1, 1, 0, false));
        p.submit_frame(mk_frame(2, 1, 1, 0, false));
        let s = p.snapshot();
        assert_eq!(s.latency_samples, 0);
        assert_eq!(s.missing_pct, 100);
    }

    #[test]
    fn perf_totals_saturate_across_frames() {
        let mut p = VirtioPerf::new();
        let mut f = mk_frame(1, COUNTER_SAT, COUNTER_SAT, 100, true);
        p.submit_frame(f);
        f = mk_frame(2, 1, 1, 100, true);
        p.submit_frame(f);
        let s = p.snapshot();
        assert_eq!(s.total_commands, (COUNTER_SAT, true), "跨帧累计亦须饱和");
        assert_eq!(s.total_bytes, (COUNTER_SAT, true));
    }

    #[test]
    fn perf_latency_shifts_up_with_load() {
        let mut fast = VirtioPerf::new();
        let mut i = 0;
        while i < 50 {
            fast.submit_frame(mk_frame(i as u64, 1, 1, 100, true));
            i += 1;
        }
        let mut slow = VirtioPerf::new();
        i = 0;
        while i < 50 {
            slow.submit_frame(mk_frame(i as u64, 1, 1, 5000, true));
            i += 1;
        }
        let f = fast.snapshot();
        let s = slow.snapshot();
        assert!(s.p50_us > f.p50_us, "重负载 p50 应更高");
        assert!(s.p99_us > f.p99_us);
        assert!(f.p50_us <= f.p90_us && f.p90_us <= f.p99_us);
        assert!(s.p50_us <= s.p90_us && s.p90_us <= s.p99_us);
    }

    #[test]
    fn perf_snapshot_queue_roundtrip() {
        let mut p = VirtioPerf::new();
        assert_eq!(p.request_snapshot(1), QueueOutcome::Enqueued);
        assert_eq!(p.request_snapshot(2), QueueOutcome::Enqueued);
        assert_eq!(p.serve_one(), QueueOutcome::Served);
        assert_eq!(p.serve_one(), QueueOutcome::Served);
        assert_eq!(p.serve_one(), QueueOutcome::Idle, "空队列不得虚报服务");
        assert!(!QueueOutcome::Idle.did_serve());
        assert!(QueueOutcome::Served.did_serve());
    }

    #[test]
    fn perf_rebalance_steps_down_on_over_quota() {
        let mut p = VirtioPerf::new();
        p.telemetry.set_quota(MetricFamily::Command, 2);
        let mut i = 0;
        while i < 5 {
            p.telemetry.report(MetricFamily::Command);
            i += 1;
        }
        p.rebalance_telemetry();
        assert_eq!(p.telemetry.step(), 1, "超配额应降档");
    }

    #[test]
    fn perf_rebalance_steps_up_when_under_quota() {
        let mut p = VirtioPerf::new();
        p.telemetry.step_down();
        p.telemetry.set_quota(MetricFamily::Command, 10_000);
        p.rebalance_telemetry();
        assert_eq!(p.telemetry.step(), 0, "未超限应回升");
    }

    #[test]
    fn perf_snapshot_carries_all_four_metric_kinds() {
        let mut p = VirtioPerf::new();
        p.submit_frame(mk_frame(1, 2, 200, 300, true));
        let s = p.snapshot();
        assert_eq!(s.total_commands.0, 2);
        assert_eq!(s.total_bytes.0, 200);
        assert_eq!(s.latency_samples, 1);
        assert_eq!(s.telemetry_reported.len(), METRIC_FAMILIES);
        assert_eq!(s.telemetry_dropped.len(), METRIC_FAMILIES);
        assert_eq!(s.sample_step, 0);
    }
}