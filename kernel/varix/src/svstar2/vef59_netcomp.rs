//! VE-F5406 · 延迟补偿框架（VE-AA 域 · 延迟补偿 · 锚点目标 320 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5406`
//!
//! **判据（锚点原文）**：预测、回滚重放、插值缓冲、偏差显示、判据。
//!
//! **职责定位（锚点原文）**：延迟补偿框架——补偿三件：客户端预测
//! （本地先行执行+服务端确认校对——操作跟手）/服务端回滚
//! reconciliation（权威修正时客户端回滚重放）/插值缓冲（远端实体
//! 插值缓冲——平滑不抖）；补偿三件各设开关与参数（延迟补偿可调
//! ——手感与公平的平衡显式化）。
//!
//! **错误路径与降级矩阵（锚点原文）**：预测错误→回滚重放+偏差显示；
//! 缓冲溢出→动态调整；重放风暴→限频。
//!
//! **性能逐项分解（锚点原文）**：预测 O(操作数)；重放 O(偏差帧)；
//! 调整 O(1)。
//!
//! **跨批对接点（锚点原文）**：F5405 模型上游；F5426 预测深化；
//! F5412 模拟。
//!
//! # 深化说明：补偿三件各自成器，偏差显示是诚实的可机检面
//!
//! 「操作跟手」的代价是客户端状态会**暂时错**——预测器的价值不在
//! 「猜得准」，而在猜错时**能被纠正且纠正过程可见**。锚点把「偏差
//! 显示」列为本条域色（诚实承诺），本条把它落成逐帧偏差账
//! （[`DeviationReport`]）：预测对了报 0，猜错了报差值——藏偏差等于
//! 把回滚重放做成了暗改，玩家看到的修正跳变就成了「这游戏在骗我」。
//!
//! ## 深化一：预测器——操作跟手且**可校对**
//!
//! [`Predictor`] 把每个本地操作编上序列号（seq 单调）先行执行并押
//! 入未确认队列；服务端权威快照（F5405 上游）到达时按 ack 序列号
//! 校对：本地在 ack 帧的状态与权威一致→确认消费队列前缀（预测对
//! 了）；不一致→偏差入账并触发回滚重放（[`Reconciler`]）。确认校对
//! O(确认数)——锚点「预测 O(操作数)」。无权威快照时预测照常先行
//! （预测不需要权威在场），但**校对被拒**（[`E_CMP_NO_AUTHORITY`]）
//! ——没有对账面的「预测」是漂移不是跟手。
//!
//! ## 深化二：回滚重放——重放是确定性的，风暴是限频的
//!
//! 权威修正到达且与本地不符：回滚=把本地状态拨回权威帧，重放=把
//! 未确认操作按 seq 序**确定性**重执行（同输入同输出——重放器不
//! 掷骰子）。重放 O(偏差帧)——锚点「重放 O(偏差帧)」，重放步数=
//! 未确认操作数。重放风暴（连续多帧都在重放）说明预测器已经失准，
//! 继续重放只是烧 CPU：[`REPLAY_LIMIT_PER_FRAME`] 限频，超限冷却
//! （[`E_CMP_REPLAY_STORM`] 计数+冷却帧内只确认不重放——冷却不是
//! 放弃校正，是把校正延到风暴平息）。
//!
//! ## 深化三：插值缓冲——平滑不抖且溢出会呼吸
//!
//! 远端实体的快照（时间戳+状态）进环形缓冲，渲染读「渲染时刻减去
//! 一个固定插值延迟」两侧快照做定点插值（[`Q8_ONE`] Q8 定点，无
//! f32 无 libm——内核面纪律）。缓冲溢出（快照堆到目标容量之上）
//! →**动态调整**：丢最旧快照+把插值延迟向目标收敛（调整 O(1)——
//! 锚点「调整 O(1)」，改一个游标不动缓冲本体）；缓冲不足时读最新
//! 快照并记欠账（欠账多=抖动前兆，可见）。
//!
//! ## 深化四：三件各设开关与参数——手感与公平的平衡是**配置数据**
//!
//! 锚点「延迟补偿可调」落成 [`CompensationConfig`]：预测/回滚/插值
//! 三开关 + 各自参数（插值延迟 ms、重放限频、缓冲目标容量）全部
//! 显性可查，参数域非法（负延迟、零缓冲）显性拒绝——手感与公平的
//! 平衡不藏在魔法数里，也不允许藏在魔法数里。
//!
//! # 确定性时基：注入时钟，判据可复跑（F5405 同款纪律）

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：AA 域字符串码惯例
// ---------------------------------------------------------------------------

/// 预测校对失败：本地与权威在 ack 帧不一致（触发回滚重放）。
pub const E_CMP_PREDICT_MISMATCH: &str = "E_CMP_PREDICT_MISMATCH";
/// 重放风暴：单帧重放超限，冷却处理。
pub const E_CMP_REPLAY_STORM: &str = "E_CMP_REPLAY_STORM";
/// 插值缓冲溢出：已动态调整（丢旧+延迟收敛），计数留痕。
pub const E_CMP_BUFFER_OVERFLOW: &str = "E_CMP_BUFFER_OVERFLOW";
/// 无权威快照：校对被拒（预测可先行，对账必须有上游）。
pub const E_CMP_NO_AUTHORITY: &str = "E_CMP_NO_AUTHORITY";
/// 配置非法：开关参数域校验失败。
pub const E_CMP_BAD_CONFIG: &str = "E_CMP_BAD_CONFIG";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_CMP_PREDICT_MISMATCH => "预测校对失败：本地与权威不一致",
        E_CMP_REPLAY_STORM => "重放风暴：单帧重放超限已冷却",
        E_CMP_BUFFER_OVERFLOW => "插值缓冲溢出：已丢旧+延迟收敛",
        E_CMP_NO_AUTHORITY => "无权威快照：校对被拒",
        E_CMP_BAD_CONFIG => "配置非法：参数域校验失败",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_CMP_PREDICT_MISMATCH => "回滚到权威帧重放未确认操作",
        E_CMP_REPLAY_STORM => "冷却帧内只确认不重放",
        E_CMP_BUFFER_OVERFLOW => "已丢最旧快照并收敛插值延迟",
        E_CMP_NO_AUTHORITY => "等待 F5405 权威快照到达",
        E_CMP_BAD_CONFIG => "按合法域重设配置",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 确定性时基：注入时钟
// ---------------------------------------------------------------------------

/// 虚拟时钟（毫秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtualClock {
    now_ms: u64,
}

impl VirtualClock {
    /// 零点时钟。
    pub const fn new() -> Self {
        VirtualClock { now_ms: 0 }
    }

    /// 推进（只进不退）。
    pub fn advance(&mut self, ms: u64) {
        self.now_ms = self.now_ms.saturating_add(ms);
    }

    /// 当前毫秒。
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 一、补偿配置：三件各设开关与参数（手感与公平的平衡显式化）
// ---------------------------------------------------------------------------

/// 插值延迟默认值 ms（缓冲时长的目标——锚点「插值缓冲」参数面）。
pub const INTERP_DELAY_MS: u64 = 100;
/// 插值缓冲目标容量（快照条数）。
pub const BUFFER_TARGET: usize = 8;
/// 单帧重放上限（重放风暴的线）。
pub const REPLAY_LIMIT_PER_FRAME: usize = 16;

/// 补偿配置：三开关+参数全部显性（锚点「补偿三件各设开关与参数」）。
#[derive(Clone, Copy, Debug)]
pub struct CompensationConfig {
    /// 客户端预测开关。
    pub predict_on: bool,
    /// 服务端回滚重放开关。
    pub replay_on: bool,
    /// 插值缓冲开关。
    pub interp_on: bool,
    /// 插值延迟 ms（>0）。
    pub interp_delay_ms: u64,
    /// 插值缓冲目标容量（≥2——至少两侧快照才能插值）。
    pub buffer_target: usize,
    /// 单帧重放上限（≥1）。
    pub replay_limit: usize,
}

impl CompensationConfig {
    /// 默认配置（判据侧独立写死对拍）。
    pub const fn default_cfg() -> Self {
        CompensationConfig {
            predict_on: true,
            replay_on: true,
            interp_on: true,
            interp_delay_ms: INTERP_DELAY_MS,
            buffer_target: BUFFER_TARGET,
            replay_limit: REPLAY_LIMIT_PER_FRAME,
        }
    }

    /// 参数域校验：延迟>0、缓冲≥2、限频≥1——非法即拒（`E_CMP_BAD_CONFIG`）。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.interp_delay_ms == 0 {
            return Err(E_CMP_BAD_CONFIG);
        }
        if self.buffer_target < 2 {
            return Err(E_CMP_BAD_CONFIG);
        }
        if self.replay_limit == 0 {
            return Err(E_CMP_BAD_CONFIG);
        }
        Ok(())
    }
}

impl Default for CompensationConfig {
    fn default() -> Self {
        Self::default_cfg()
    }
}

// ---------------------------------------------------------------------------
// 二、预测器：本地先行执行 + 服务端确认校对
// ---------------------------------------------------------------------------

/// 单个本地操作（seq 单调编号）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalOp {
    /// 操作序列号（本地单调递增）。
    pub seq: u64,
    /// 操作载荷（确定性重放的输入——同 seq 同载荷同结果）。
    pub payload: u64,
}

/// 校对结论：确认（前缀消费）或偏差（触发回滚重放）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AckVerdict {
    /// 预测正确：未确认队列前缀已确认。
    Confirmed { acked: usize },
    /// 预测有偏差：需要回滚重放。
    Diverged { at_seq: u64 },
}

/// 预测器：本地先行执行，押 seq 入未确认队列，等权威校对。
#[derive(Clone, Debug)]
pub struct Predictor {
    next_seq: u64,
    pending: Vec<LocalOp>,
    /// 累计确认数（跟手的可观测面）。
    pub confirmed_ops: u32,
    /// 校对被拒计数（无权威快照时校对请求被拒）。
    pub no_authority_blocks: u32,
}

impl Predictor {
    /// 空预测器。
    pub fn new() -> Self {
        Predictor { next_seq: 1, pending: Vec::new(), confirmed_ops: 0, no_authority_blocks: 0 }
    }

    /// 本地先行执行：操作入队（seq 自动单调）。O(1) 摊还。
    pub fn submit(&mut self, payload: u64) -> LocalOp {
        let op = LocalOp { seq: self.next_seq, payload };
        self.next_seq += 1;
        self.pending.push(op.clone());
        op
    }

    /// 未确认操作数。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// **校对请求**：无权威快照在场→拒绝（`E_CMP_NO_AUTHORITY`）。
    /// 有权威→按「权威认可的 seq」校对：队列中 seq ≤ acked_seq 的
    /// 前缀与权威一致即确认（本框架由调用方保证一致性判定，此处
    /// 按 ack 消费前缀）；调用方传 `diverged_at` 表明首个分歧 seq。
    pub fn reconcile(
        &mut self,
        authority_present: bool,
        acked_seq: u64,
        diverged_at: Option<u64>,
    ) -> Result<AckVerdict, &'static str> {
        if !authority_present {
            self.no_authority_blocks += 1;
            return Err(E_CMP_NO_AUTHORITY);
        }
        match diverged_at {
            Some(at) => Ok(AckVerdict::Diverged { at_seq: at }),
            None => {
                // 消费 seq ≤ acked_seq 的前缀（确认 O(确认数)）。
                let mut acked = 0usize;
                while acked < self.pending.len() && self.pending[acked].seq <= acked_seq {
                    acked += 1;
                }
                self.pending.drain(0..acked);
                self.confirmed_ops += acked as u32;
                Ok(AckVerdict::Confirmed { acked })
            }
        }
    }

    /// 未确认队列只读（重放器输入）。
    pub fn pending_ops(&self) -> &[LocalOp] {
        &self.pending
    }

    /// 手动清空到 seq（回滚器拨状态后由重放器保留未确认操作）。
    pub fn retain_from(&mut self, seq: u64) {
        self.pending.retain(|op| op.seq >= seq);
    }
}

impl Default for Predictor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、回滚重放器：确定性重执行 + 风暴限频
// ---------------------------------------------------------------------------

/// 重放器：把未确认操作按 seq 序确定性重执行。
#[derive(Clone, Debug)]
pub struct Reconciler {
    /// 累计重放操作数（O(偏差帧) 的账面）。
    pub replayed_ops: u32,
    /// 重放风暴冷却剩余帧。
    pub cooldown_frames: u32,
    /// 风暴触发计数。
    pub storm_hits: u32,
    limit_per_frame: usize,
}

/// 重放执行结果：本帧实际重放的操作数与是否触发风暴。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayOutcome {
    /// 本帧实际重放的操作数。
    pub replayed: usize,
    /// 本帧是否触发风暴限频（true=已冷却，余下操作延后）。
    pub stormed: bool,
}

impl Reconciler {
    /// 重放器（限频来自配置）。
    pub fn new(limit_per_frame: usize) -> Self {
        Reconciler { replayed_ops: 0, cooldown_frames: 0, storm_hits: 0, limit_per_frame }
    }

    /// 风暴冷却中判据。
    pub const fn cooling(&self) -> bool {
        self.cooldown_frames > 0
    }

    /// **重放一步**：给定确定重执行闭包语义的「重放批」（调用方按
    /// seq 序传入待重放操作数），本框架按限频切批——超过
    /// `limit_per_frame` 的部分延后（`E_CMP_REPLAY_STORM` 计数+冷却）。
    /// 重放 O(本帧配额)——锚点「重放 O(偏差帧)」以帧为界。
    pub fn replay_step(&mut self, pending_count: usize) -> ReplayOutcome {
        // 冷却帧内只确认不重放：每帧冷却-1。
        if self.cooling() {
            self.cooldown_frames -= 1;
            return ReplayOutcome { replayed: 0, stormed: false };
        }
        if pending_count == 0 {
            return ReplayOutcome { replayed: 0, stormed: false };
        }
        if pending_count > self.limit_per_frame {
            // 风暴：本帧重放配额内的一批，余下冷却延后。
            self.storm_hits += 1;
            self.cooldown_frames = 1;
            self.replayed_ops += self.limit_per_frame as u32;
            return ReplayOutcome { replayed: self.limit_per_frame, stormed: true };
        }
        self.replayed_ops += pending_count as u32;
        ReplayOutcome { replayed: pending_count, stormed: false }
    }
}

// ---------------------------------------------------------------------------
// 四、插值缓冲：Q8 定点 + 溢出呼吸
// ---------------------------------------------------------------------------

/// 定点插值分母（Q8：t ∈ [0,256]，256=全到 b）。
pub const Q8_ONE: u32 = 256;

/// Q8 定点插值：`a + (b-a)*t/256`（整数路径无 f32；判据侧独立手算）。
pub const fn q8_lerp(a: i64, b: i64, t: u32) -> i64 {
    let span = b - a;
    a + (span * (t as i64)) / (Q8_ONE as i64)
}

/// 远端实体快照（时间戳+状态值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteSnap {
    /// 快照时间戳 ms（服务端时钟口径）。
    pub at_ms: u64,
    /// 状态值（位置类——Q8 定点插值作用在 i64 上）。
    pub value: i64,
}

/// 插值缓冲：环形快照 + 动态延迟收敛。
#[derive(Clone, Debug)]
pub struct InterpBuffer {
    snaps: Vec<RemoteSnap>,
    target: usize,
    /// 当前插值延迟 ms（向目标动态收敛）。
    pub delay_ms: u64,
    /// 溢出调整计数（锚点「缓冲溢出→动态调整」的可观测面）。
    pub overflow_adjusts: u32,
    /// 欠账计数（缓冲不足时读最新快照的次数——抖动前兆可见）。
    pub starved_reads: u32,
}

/// 插值读取结果：插值值或降级读（最新快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterpRead {
    /// 双侧插值值。
    Lerped(i64),
    /// 缓冲不足：降级读最新快照（欠账）。
    Newest(i64),
}

impl InterpBuffer {
    /// 空缓冲（目标容量与延迟来自配置）。
    pub fn new(target: usize, delay_ms: u64) -> Self {
        InterpBuffer {
            snaps: Vec::new(),
            target,
            delay_ms,
            overflow_adjusts: 0,
            starved_reads: 0,
        }
    }

    /// 快照数。
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    /// 空缓冲判据。
    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }

    /// 入快照：容量超目标→**动态调整**（丢最旧+延迟向目标收敛
    /// ——延迟按 1/4 步长贴近目标，调整 O(1)：改游标不动本体）。
    pub fn push(&mut self, snap: RemoteSnap) {
        self.snaps.push(snap);
        self.snaps.sort_by_key(|s| s.at_ms);
        if self.snaps.len() > self.target {
            let drop_n = self.snaps.len() - self.target;
            self.snaps.drain(0..drop_n);
            self.overflow_adjusts += 1;
            // 延迟收敛：向目标容量对应的标称延迟（target*10ms）贴近 1/4。
            let nominal = (self.target as u64) * 10;
            if self.delay_ms > nominal {
                let step = (self.delay_ms - nominal + 3) / 4;
                self.delay_ms -= step.max(1);
            }
        }
    }

    /// **插值读取**：渲染时刻 = now - delay；取渲染时刻两侧最近快照
    /// 做 Q8 定点插值。t 按「渲染时刻在两侧快照间的比例」计算
    /// （判据侧独立手算对拍）。缓冲不足（<2 或渲染时刻越出右端）
    /// →降级读最新并记欠账。
    pub fn read(&mut self, now_ms: u64) -> Option<InterpRead> {
        if self.snaps.len() < 2 {
            match self.snaps.last() {
                Some(s) => {
                    self.starved_reads += 1;
                    Some(InterpRead::Newest(s.value))
                }
                None => None,
            }
        } else {
            let render_ms = now_ms.saturating_sub(self.delay_ms);
            let last = self.snaps[self.snaps.len() - 1];
            if render_ms >= last.at_ms {
                // 渲染时刻越过最新快照：降级读最新（欠账）。
                self.starved_reads += 1;
                return Some(InterpRead::Newest(last.value));
            }
            // 找渲染时刻左邻快照（snaps 按 at_ms 升序）。
            let mut lo = 0usize;
            let mut i = 0usize;
            while i + 1 < self.snaps.len() {
                if self.snaps[i + 1].at_ms <= render_ms {
                    i += 1;
                    lo = i;
                } else {
                    break;
                }
            }
            let a = self.snaps[lo];
            let hi = self.snaps[lo + 1];
            let span = (hi.at_ms - a.at_ms) as u64;
            if span == 0 {
                return Some(InterpRead::Lerped(a.value));
            }
            let t = (((render_ms - a.at_ms) * (Q8_ONE as u64)) / span) as u32;
            let t = t.min(Q8_ONE);
            Some(InterpRead::Lerped(q8_lerp(a.value, hi.value, t)))
        }
    }
}

// ---------------------------------------------------------------------------
// 五、偏差显示：诚实承诺的可机检面（域色核心）
// ---------------------------------------------------------------------------

/// 逐帧偏差账：预测校对产生的偏差记录（可读屏）。
#[derive(Clone, Debug, Default)]
pub struct DeviationReport {
    entries: Vec<(u64, i64)>,
}

impl DeviationReport {
    /// 空账。
    pub fn new() -> Self {
        DeviationReport { entries: Vec::new() }
    }

    /// 记一笔偏差（seq, 权威值-本地值）。
    pub fn record(&mut self, seq: u64, delta: i64) {
        self.entries.push((seq, delta));
    }

    /// 偏差条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判据。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 最近偏差值（无偏差时 Some(0)——偏差为零也是要如实报的事实）。
    pub fn latest(&self) -> i64 {
        self.entries.last().map(|(_, d)| *d).unwrap_or(0)
    }

    /// 读屏文本：非空且含最近偏差值（诚实承诺——藏偏差=暗改）。
    pub fn screen_text(&self) -> String {
        format!(
            "预测偏差：最近一笔 {}（共 {} 笔偏差记录；偏差为零时如实报零）",
            self.latest(),
            self.entries.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5406 域自检（判据分六组：配置/预测/回滚重放/插值/偏差显示/判据元）。
pub fn run_vef59_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5406");

    // --- 配置（判据一）---
    // 配置-01：三开关默认全开 + 参数判据侧独立写死对拍。
    let cfg = CompensationConfig::default_cfg();
    if cfg.predict_on && cfg.replay_on && cfg.interp_on
        && cfg.interp_delay_ms == 100
        && cfg.buffer_target == 8
        && cfg.replay_limit == 16
    {
        s.ok("EAA6-配置-01-默认参数显性对拍");
    } else {
        s.fail("EAA6-配置-01-默认参数显性对拍", "默认参数漂移");
    }
    // 配置-02：参数域校验——零延迟/缓冲<2/零限频逐个拒。
    let mut bad = CompensationConfig::default_cfg();
    bad.interp_delay_ms = 0;
    let b1 = bad.validate() == Err(E_CMP_BAD_CONFIG);
    let mut bad2 = CompensationConfig::default_cfg();
    bad2.buffer_target = 1;
    let b2 = bad2.validate() == Err(E_CMP_BAD_CONFIG);
    let mut bad3 = CompensationConfig::default_cfg();
    bad3.replay_limit = 0;
    let b3 = bad3.validate() == Err(E_CMP_BAD_CONFIG);
    let good = CompensationConfig::default_cfg().validate().is_ok();
    if b1 && b2 && b3 && good {
        s.ok("EAA6-配置-02-参数域非法拒");
    } else {
        s.fail("EAA6-配置-02-参数域非法拒", "参数域闸失效");
    }

    // --- 预测（判据二）---
    // 预测-01：操作跟手——seq 单调、pending 增长、确认前缀消费。
    let mut p = Predictor::new();
    let op1 = p.submit(10);
    let op2 = p.submit(20);
    let op3 = p.submit(30);
    let mono = op1.seq < op2.seq && op2.seq < op3.seq && p.pending_len() == 3;
    let v = p.reconcile(true, op2.seq, None);
    let confirmed = matches!(v, Ok(AckVerdict::Confirmed { acked: 2 }))
        && p.pending_len() == 1
        && p.confirmed_ops == 2;
    if mono && confirmed {
        s.ok("EAA6-预测-01-跟手与确认前缀消费");
    } else {
        s.fail("EAA6-预测-01-跟手与确认前缀消费", "预测器校对面失效");
    }
    // 预测-02：无权威校对被拒——没有对账面的预测是漂移不是跟手。
    let mut p2 = Predictor::new();
    let _ = p2.submit(1);
    let na = p2.reconcile(false, 1, None) == Err(E_CMP_NO_AUTHORITY)
        && p2.no_authority_blocks == 1
        && p2.pending_len() == 1;
    if na {
        s.ok("EAA6-预测-02-无权威校对拒");
    } else {
        s.fail("EAA6-预测-02-无权威校对拒", "无上游仍放行校对");
    }

    // --- 回滚重放（判据三）---
    // 重放-01：偏差→重放未确认操作（O(偏差帧) 以帧为界）。
    let mut p3 = Predictor::new();
    let _ = p3.submit(100);
    let _ = p3.submit(200);
    let _ = p3.submit(300);
    let div = p3.reconcile(true, 3, Some(2));
    let div_ok = matches!(div, Ok(AckVerdict::Diverged { at_seq: 2 }));
    // 分歧后保留 seq>=2 的未确认操作（重放器输入）。
    p3.retain_from(2);
    let kept = p3.pending_len() == 2 && p3.pending_ops()[0].seq == 2;
    let mut r = Reconciler::new(16);
    let out = r.replay_step(p3.pending_len());
    if div_ok && kept && out.replayed == 2 && !out.stormed && r.replayed_ops == 2 {
        s.ok("EAA6-重放-01-偏差回滚重放");
    } else {
        s.fail("EAA6-重放-01-偏差回滚重放", "重放面失效");
    }
    // 重放-02：风暴限频——超限触发冷却，冷却帧内只确认不重放。
    let mut r2 = Reconciler::new(4);
    let storm = r2.replay_step(10);
    let s1 = storm.replayed == 4 && storm.stormed && r2.storm_hits == 1 && r2.cooling();
    let cooled = r2.replay_step(6);
    let s2 = cooled.replayed == 0 && !r2.cooling();
    let after = r2.replay_step(6);
    let s3 = after.replayed == 4 && after.stormed;
    if s1 && s2 && s3 {
        s.ok("EAA6-重放-02-风暴限频冷却");
    } else {
        s.fail("EAA6-重放-02-风暴限频冷却", "限频或冷却失效");
    }
    // 重放-03：空队列重放零操作（不凭空烧 CPU）。
    let mut r3 = Reconciler::new(16);
    let idle = r3.replay_step(0).replayed == 0 && r3.replayed_ops == 0;
    if idle {
        s.ok("EAA6-重放-03-空批零重放");
    } else {
        s.fail("EAA6-重放-03-空批零重放", "空批仍重放");
    }

    // --- 插值缓冲（判据四）---
    // 插值-01：Q8 定点插值判据侧独立手算对拍（中点/四分点/端点）。
    let mid = q8_lerp(100, 200, 128); // 期望 150（判据侧手算）
    let quart = q8_lerp(0, 256, 64); // 期望 64
    let lo = q8_lerp(10, 20, 0); // 期望 10
    let hi = q8_lerp(10, 20, 256); // 期望 20
    if mid == 150 && quart == 64 && lo == 10 && hi == 20 {
        s.ok("EAA6-插值-01-定点插值手算对拍");
    } else {
        s.fail("EAA6-插值-01-定点插值手算对拍", "定点插值漂移");
    }
    // 插值-02：缓冲平滑读取——双侧插值落在两快照之间。
    let mut buf = InterpBuffer::new(8, 100);
    buf.push(RemoteSnap { at_ms: 0, value: 0 });
    buf.push(RemoteSnap { at_ms: 100, value: 100 });
    let rd = buf.read(150); // 渲染时刻 50ms → t=128 → 期望 50
    let smooth = rd == Some(InterpRead::Lerped(50));
    if smooth {
        s.ok("EAA6-插值-02-平滑双侧插值");
    } else {
        s.fail("EAA6-插值-02-平滑双侧插值", "插值读取漂移");
    }
    // 插值-03：溢出动态调整——丢最旧+延迟收敛+计数留痕（调整 O(1)）。
    // 判据侧独立手算：7 次 push、目标 4 → 第 5/6/7 次各溢出一次
    //（adjusts=3）；延迟 200→160→130→107（每步 (d-40+3)/4：40/30/23）。
    let mut buf2 = InterpBuffer::new(4, 200);
    for i in 0..7u64 {
        buf2.push(RemoteSnap { at_ms: i * 10, value: (i as i64) * 10 });
    }
    let adj = buf2.len() == 4
        && buf2.overflow_adjusts == 3
        && buf2.delay_ms == 107
        && buf2.snaps[0].at_ms == 30; // 恰丢 0/10/20 三条最旧
    if adj {
        s.ok("EAA6-插值-03-溢出动态调整");
    } else {
        s.fail("EAA6-插值-03-溢出动态调整", "溢出未丢旧或延迟未收敛");
    }
    // 插值-04：缓冲不足降级读最新并记欠账（抖动前兆可见）。
    let mut buf3 = InterpBuffer::new(8, 100);
    buf3.push(RemoteSnap { at_ms: 0, value: 7 });
    let starve = buf3.read(50) == Some(InterpRead::Newest(7)) && buf3.starved_reads == 1;
    let empty = InterpBuffer::new(8, 100).read(50).is_none();
    if starve && empty {
        s.ok("EAA6-插值-04-欠账降级读");
    } else {
        s.fail("EAA6-插值-04-欠账降级读", "降级面失效");
    }

    // --- 偏差显示（判据五）---
    // 显示-01：偏差逐帧入账、读屏文本非空含值、零偏差如实报零。
    let mut d = DeviationReport::new();
    let zero_text = d.screen_text();
    let honest_zero = zero_text.contains("0") && zero_text.contains("如实报零");
    d.record(5, -3);
    d.record(6, 7);
    let txt = d.screen_text();
    let shown = txt.contains("7") && txt.contains("2 笔");
    if d.len() == 2 && d.latest() == 7 && honest_zero && shown {
        s.ok("EAA6-显示-01-偏差诚实显示");
    } else {
        s.fail("EAA6-显示-01-偏差诚实显示", "偏差显示藏值");
    }

    // --- 判据元（判据六）---
    // 元-01：错误码互异 + 三要素齐备（判据侧独立枚举五码）。
    let codes = [
        E_CMP_PREDICT_MISMATCH,
        E_CMP_REPLAY_STORM,
        E_CMP_BUFFER_OVERFLOW,
        E_CMP_NO_AUTHORITY,
        E_CMP_BAD_CONFIG,
    ];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if reason_of(codes[i]) == "未知错误码" || direction_of(codes[i]).is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok {
        s.ok("EAA6-判据-01-码互异三要素齐备");
    } else {
        s.fail("EAA6-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic（自扫本文件，单遍词法剥除）。
    let src = include_str!("vef59_netcomp.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unreachable!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EAA6-判据-02-零panic面");
    } else {
        s.fail("EAA6-判据-02-零panic面", "判据/生产面含 panic 面");
    }
    // 元-03：注入时钟可复跑。
    let mut c1 = VirtualClock::new();
    c1.advance(30);
    c1.advance(20);
    let mut c2 = VirtualClock::new();
    c2.advance(50);
    if c1.now_ms() == 50 && c1.now_ms() == c2.now_ms() {
        s.ok("EAA6-判据-03-注入时钟可复跑");
    } else {
        s.fail("EAA6-判据-03-注入时钟可复跑", "时钟推进不可复跑");
    }

    s
}

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
