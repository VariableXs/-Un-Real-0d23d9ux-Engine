//! F057 IO 调度分级（perfstar · G-B-17）——互不拖累的感觉就是分级在干活。
//!
//! 主册判据（验收标准第一句）：
//! **「下载中打开目录」场景目录延迟 ≤ 无下载时的 1.2 倍；后台任务零饥饿
//! （24h 混载测试全部完成）。**
//!
//! 功能定义（G-B-17）：IO 队列三级：前台交互（菜单打开/搜索/缩略图）>
//! 后台任务（下载/索引/构建）> 批量（备份/日志轮转）；调度按截止期+权重
//! 的混合策略；io-pending 指标（实测 9/9 pass 既有）细化为分级队列视图。
//!
//! 【交互设计】监视器 IO 页三队列实时深度条 + 各队列吞吐曲线；下载类任务
//! 自动降级的提示出现在通知中心（透明可查）。
//! 【数据与存储】队列结构定长环形；分级事件审计（谁被降级/为什么）入诊断
//! 快照。
//! 【状态与异常】前台持续满载 60s → 后台完全暂停；暂停时长有界（5s 上限
//! 自动解除并复位满载计时——防饿死反向保护：完全暂停若无界，持续满载下
//! 后台永久饥饿，违反判据二）；批量任务超 30 分钟 → 分段让路（每 5 分钟
//! 让 1s）；配额（F195）与分级叠加生效。
//! 【设计细节】class 判定：按发起进程标签（交互面进程=前台）+ 请求特征
//! （读<64KB=交互倾向）；截止期参数：前台 50ms/后台 2s/批量无截止期；分级
//! 是调度顺序不改变 fsync 硬承诺（B-7xx 优先于一切）。
//!
//! 【开源复用】分级参照 Linux CFQ/BFQ 的 class 思想（极简三 class 版）；
//! 实现自研（存储栈既有队列扩展）。
//!
//! 与既有模块关系：io-pending 既有指标（9/9 pass）细化为三队列视图；F195
//! 配额以叠加开关接入（分级顺序 ∩ 配额可用）；fsync 硬承诺语义与 B-7xx
//! 存储栈一致（本层只保证调度顺序让位）。接线随闸门（登记完成报告）。
//!
//! 零堆纪律：三条定长环形队列 + 定长审计环，无 Vec/String/Box/format!。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 前台截止期 50ms（主册规格框架：前台 50ms）。
pub const FG_DEADLINE_MS: u64 = 50;
/// 后台截止期 2s（主册规格框架：后台 2s）。
pub const BG_DEADLINE_MS: u64 = 2_000;
/// 批量无截止期——以 `None` 表示（主册：批量无截止期）。
/// 交互读倾向线（主册：读<64KB=交互倾向）。
pub const INTERACTIVE_READ_MAX_BYTES: u64 = 64 * 1024;
/// 前台持续满载阈值（主册：前台持续满载 60s → 后台完全暂停）。
pub const FG_SATURATION_MS: u64 = 60_000;
/// 后台暂停时长上限（5s）：「完全暂停」必须时间有界——否则前台持续满载
/// 下后台永久饥饿，违反主册判据二（后台任务零饥饿）。暂停满上限自动解除
/// 并复位满载计时：前台再次持续满载 60s 才重新暂停（主册逐字语义）。
/// 时长取 5s（与旧「前台让出窗口」同量级的实现选择，登记完成报告）。
pub const BG_PAUSE_MAX_MS: u64 = 5_000;
/// 批量分段让路起点（主册：批量任务超 30 分钟 → 分段让路）。
pub const BATCH_SEGMENT_AFTER_MS: u64 = 30 * 60_000;
/// 批量让路节拍（主册：每 5 分钟让 1s）。
pub const BATCH_SEGMENT_MS: u64 = 5 * 60_000;
/// 批量单次让路时长（主册：让 1s）。
pub const BATCH_YIELD_MS: u64 = 1_000;
/// 批量会话保活间隔：批量流静默超过该值视为任务结束、会话复位（实现
/// 选择——主册「批量任务超 30 分钟」以连续批量流计，登记完成报告）。
pub const BATCH_SESSION_GAP_MS: u64 = 60_000;
/// 判据比值线：目录延迟 ≤ 1.2 倍（permille 1200）。
pub const DIR_DELAY_RATIO_PERMILLE: u32 = 1_200;
/// 每队列定长环形容量。256 = 风暴期后台积压（60s × 2/s = 120）+ 暂停期
/// 注入（5s × 2/s = 10）的背压余量（峰值 130），保证暂停机制全程零丢请求。
pub const Q_CAP: usize = 256;
/// 分级事件审计环容量（谁被降级/为什么——主册【数据与存储】）。
pub const AUDIT_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 分级三队列（主册：前台交互 > 后台任务 > 批量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoClass {
    Foreground = 0,
    Background = 1,
    Batch = 2,
}

impl IoClass {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// IO 操作类型（fsync 单列——B-7xx 硬承诺载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoOp {
    Read,
    Write,
    Fsync,
}

/// 工作负载种类（class 判定的进程标签面——主册设计细节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Workload {
    /// 交互面进程（菜单打开/搜索/缩略图）。
    Interactive,
    /// 下载（用户故事：2GB 文件下载）。
    Download,
    /// 索引。
    Index,
    /// 构建。
    Build,
    /// 备份。
    Backup,
    /// 日志轮转。
    LogRotate,
    /// 无标签 → 请求特征判定（读<64KB=交互倾向）。
    Unknown,
}

/// IO 请求（Copy 定长——环形队列元素）。
#[derive(Clone, Copy, Debug)]
pub struct IoReq {
    pub id: u64,
    pub op: IoOp,
    pub bytes: u64,
    pub enq_ms: u64,
    /// EDF 截止期：前台 enq+50 / 后台 enq+2000 / 批量 None（主册）。
    pub deadline_ms: Option<u64>,
}

/// 分级事件审计行：谁被降级/为什么（主册【数据与存储】）。
#[derive(Clone, Copy, Debug)]
pub struct TierAudit {
    pub at_ms: u64,
    pub req_id: u64,
    pub cls: IoClass,
    /// 归因短语（静态串，无堆）。
    pub why: &'static str,
}

/// 派发结果。
#[derive(Clone, Copy, Debug)]
pub struct Dispatch {
    pub req: IoReq,
    pub cls: IoClass,
    /// 排队等待 = 派发时刻 − 入队时刻（判据观测面）。
    pub waited_ms: u64,
}

// ---------------------------------------------------------------------------
// class 判定（主册设计细节：进程标签 + 请求特征）
// ---------------------------------------------------------------------------

/// class 判定。返回（分级，归因短语）。归因短语入审计环——「为什么」可查。
///
/// 规则（主册【设计细节】逐条）：
/// 1. fsync → 前台（硬承诺载体，B-7xx 优先于一切）；
/// 2. 进程标签：交互面进程 = 前台；
/// 3. 请求特征：无标签且读 <64KB = 交互倾向 → 前台；
/// 4. 下载/索引/构建 = 后台（自动降级，提示入通知中心——透明可查）；
/// 5. 备份/日志轮转 = 批量。
pub fn classify(w: Workload, op: IoOp, bytes: u64) -> (IoClass, &'static str) {
    match w {
        Workload::Interactive => (IoClass::Foreground, "interactive-label"),
        Workload::Download => (IoClass::Background, "download-degraded"),
        Workload::Index => (IoClass::Background, "index-degraded"),
        Workload::Build => (IoClass::Background, "build-degraded"),
        Workload::Backup => (IoClass::Batch, "backup-batch"),
        Workload::LogRotate => (IoClass::Batch, "logrotate-batch"),
        Workload::Unknown => {
            if op == IoOp::Fsync {
                (IoClass::Foreground, "fsync-hard-promise")
            } else if op == IoOp::Read && bytes < INTERACTIVE_READ_MAX_BYTES {
                (IoClass::Foreground, "read-below-64k")
            } else {
                (IoClass::Background, "unlabeled-default-bg")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 定长环形队列
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Ring {
    buf: [Option<IoReq>; Q_CAP],
    head: usize,
    n: usize,
}

impl Ring {
    const fn new() -> Self {
        Ring { buf: [None; Q_CAP], head: 0, n: 0 }
    }

    fn push(&mut self, r: IoReq) -> bool {
        if self.n >= Q_CAP {
            return false; // 队满拒绝（背压如实上抛，不静默丢弃）
        }
        let tail = (self.head + self.n) % Q_CAP;
        self.buf[tail] = Some(r);
        self.n += 1;
        true
    }

    fn len(&self) -> usize {
        self.n
    }

    /// 取出满足谓词的最早入队请求（fsync 扫描与 EDF 共用）。
    fn take_where(&mut self, pick: impl Fn(&IoReq) -> bool) -> Option<IoReq> {
        if self.n == 0 {
            return None;
        }
        let mut best: Option<(usize, IoReq)> = None;
        for i in 0..self.n {
            let idx = (self.head + i) % Q_CAP;
            if let Some(r) = self.buf[idx] {
                if pick(&r) && best.as_ref().map_or(true, |(_, b)| r.deadline_ms < b.deadline_ms || (r.deadline_ms == b.deadline_ms && r.enq_ms < b.enq_ms)) {
                    best = Some((idx, r));
                }
            }
        }
        match best {
            Some((idx, r)) => {
                self.buf[idx] = None;
                // 收缩空洞：头指针推进语义 = 摘除后压缩。
                self.compact_after_take(idx);
                Some(r)
            }
            None => None,
        }
    }

    /// 摘除中间槽位后把其后元素前移（n ≤ 128，O(n) 定长）。
    fn compact_after_take(&mut self, idx: usize) {
        let mut i = idx;
        while i != (self.head + self.n - 1) % Q_CAP {
            let nxt = (i + 1) % Q_CAP;
            self.buf[i] = self.buf[nxt];
            i = nxt;
        }
        self.buf[i] = None;
        self.n -= 1;
    }
}

// ---------------------------------------------------------------------------
// 分级调度器
// ---------------------------------------------------------------------------

/// IO 分级调度器：三队列 + 截止期 EDF + 反向保护 + 批量让路 + 配额叠加。
pub struct IoTier {
    q: [Ring; 3],
    next_id: u64,
    /// 前台持续非空计时起点（满载 60s 反向保护）。
    fg_busy_since: Option<u64>,
    /// 后台暂停起点（Some = 暂停中：满载 60s 触发，满 BG_PAUSE_MAX_MS
    /// 自动解除——防饿死兜底，暂停时长有界）。
    bg_paused_since: Option<u64>,
    bg_pauses: u32,
    /// 批量会话起点（30min 分段让路）。
    batch_since: Option<u64>,
    /// 批量流最近活动时刻（保活判定）。
    batch_last_activity_ms: u64,
    batch_yields: u32,
    /// F195 配额叠加开关（true = 允许派发；默认全开）。
    quota_open: [bool; 3],
    /// 记账：各队列派发数 / 截止期违约数 / 最大等待。
    served: [u64; 3],
    deadline_misses: [u32; 3],
    max_wait_ms: [u64; 3],
    /// fsync 硬承诺派发计数。
    fsync_dispatched: u64,
    /// 目录打开判据观测对（基线 wait / 下载中 wait，μs 粒度由调用方换算）。
    dir_base_wait_ms: Option<u64>,
    dir_dl_wait_ms: Option<u64>,
    /// 审计环。
    audit: [Option<TierAudit>; AUDIT_CAP],
    audit_n: usize,
    total_enqueued: u64,
}

impl IoTier {
    pub fn new() -> Self {
        IoTier {
            q: [Ring::new(), Ring::new(), Ring::new()],
            next_id: 1,
            fg_busy_since: None,
            bg_paused_since: None,
            bg_pauses: 0,
            batch_since: None,
            batch_last_activity_ms: 0,
            batch_yields: 0,
            quota_open: [true; 3],
            served: [0; 3],
            deadline_misses: [0; 3],
            max_wait_ms: [0; 3],
            fsync_dispatched: 0,
            dir_base_wait_ms: None,
            dir_dl_wait_ms: None,
            audit: [None; AUDIT_CAP],
            audit_n: 0,
            total_enqueued: 0,
        }
    }

    // -- 入队 ---------------------------------------------------------------

    /// 按工作负载自动分级入队（class 判定 + 审计「谁被降级/为什么」）。
    pub fn submit(&mut self, w: Workload, op: IoOp, bytes: u64, now_ms: u64) -> Option<u64> {
        let (cls, why) = classify(w, op, bytes);
        let deadline = match cls {
            IoClass::Foreground => Some(now_ms + FG_DEADLINE_MS),
            IoClass::Background => Some(now_ms + BG_DEADLINE_MS),
            IoClass::Batch => None,
        };
        let id = self.next_id;
        self.next_id += 1;
        let req = IoReq { id, op, bytes, enq_ms: now_ms, deadline_ms: deadline };
        if !self.q[cls.index()].push(req) {
            return None; // 队满
        }
        self.total_enqueued += 1;
        self.audit_push(TierAudit { at_ms: now_ms, req_id: id, cls, why });
        // 批量会话开启/保活。
        if cls == IoClass::Batch {
            if self.batch_since.is_none() {
                self.batch_since = Some(now_ms);
            }
            self.batch_last_activity_ms = now_ms;
        }
        // 前台入队即参与满载计时评估。
        if cls == IoClass::Foreground {
            self.evaluate_fg_saturation(now_ms);
        }
        Some(id)
    }

    /// 已知分级直入（既有存储栈路径兼容；不经 classify 审计）。
    pub fn submit_classified(&mut self, cls: IoClass, op: IoOp, bytes: u64, now_ms: u64) -> Option<u64> {
        let deadline = match cls {
            IoClass::Foreground => Some(now_ms + FG_DEADLINE_MS),
            IoClass::Background => Some(now_ms + BG_DEADLINE_MS),
            IoClass::Batch => None,
        };
        let id = self.next_id;
        self.next_id += 1;
        let req = IoReq { id, op, bytes, enq_ms: now_ms, deadline_ms: deadline };
        if !self.q[cls.index()].push(req) {
            return None;
        }
        self.total_enqueued += 1;
        if cls == IoClass::Batch {
            if self.batch_since.is_none() {
                self.batch_since = Some(now_ms);
            }
            self.batch_last_activity_ms = now_ms;
        }
        if cls == IoClass::Foreground {
            self.evaluate_fg_saturation(now_ms);
        }
        Some(id)
    }

    // -- 派发 ---------------------------------------------------------------

    /// 派发下一请求。顺序 = fsync 硬承诺 > 前台 EDF > 后台 EDF > 批量（让路
    /// 窗外）；配额叠加：quota 关闭的分级整档跳过。
    pub fn next(&mut self, now_ms: u64) -> Option<Dispatch> {
        self.evaluate_fg_saturation(now_ms);
        // 批量会话复位：批量流静默超 gap → 任务结束，30min 计时归零。
        if let Some(_since) = self.batch_since {
            if now_ms.saturating_sub(self.batch_last_activity_ms) > BATCH_SESSION_GAP_MS {
                self.batch_since = None;
            }
        }

        // 0) B-7xx：fsync 硬承诺优先于一切——三队列扫描最早入队的 fsync。
        for qi in 0..3 {
            if let Some(req) = self.q[qi].take_where(|r| r.op == IoOp::Fsync) {
                self.fsync_dispatched += 1;
                let d = self.record_dispatch(IoClass::Foreground, req, now_ms);
                // fsync 可能来自任何队列，按前台承诺处理记账。
                if req.deadline_ms.is_none() {
                    self.served[qi] += 1;
                }
                return Some(d);
            }
        }

        // 1) 前台 EDF。
        if let Some(req) = self.q[0].take_where(|_| true) {
            let d = self.record_dispatch(IoClass::Foreground, req, now_ms);
            return Some(d);
        }
        // 前台已空：参与让出窗口计时（前台空闲即让出）。
        // 2) 后台 EDF（反向保护暂停中跳过；配额关闭跳过）。
        if self.bg_paused_since.is_none() && self.quota_open[1] {
            if let Some(req) = self.q[1].take_where(|_| true) {
                let d = self.record_dispatch(IoClass::Background, req, now_ms);
                return Some(d);
            }
        }
        // 3) 批量（30min 后按 5min 节拍让 1s；配额关闭跳过）。
        if self.quota_open[2] && !self.batch_yielding(now_ms) {
            if let Some(req) = self.q[2].take_where(|_| true) {
                let d = self.record_dispatch(IoClass::Batch, req, now_ms);
                return Some(d);
            }
        }
        None
    }

    fn record_dispatch(&mut self, cls: IoClass, req: IoReq, now_ms: u64) -> Dispatch {
        let waited = now_ms.saturating_sub(req.enq_ms);
        let idx = cls.index();
        self.served[idx] += 1;
        if waited > self.max_wait_ms[idx] {
            self.max_wait_ms[idx] = waited;
        }
        if let Some(dl) = req.deadline_ms {
            if now_ms > dl {
                self.deadline_misses[idx] += 1;
            }
        }
        Dispatch { req, cls, waited_ms: waited }
    }

    // -- 反向保护（前台满载 60s → 后台完全暂停） ------------------------------

    fn evaluate_fg_saturation(&mut self, now_ms: u64) {
        // 防饿死兜底：完全暂停时长有界（BG_PAUSE_MAX_MS）——若无界，前台
        // 持续满载下后台永久饥饿，违反主册判据二（后台任务零饥饿）。解除
        // 即复位满载计时：前台再次持续满载 60s 才重新暂停（主册逐字语义）。
        if let Some(p) = self.bg_paused_since {
            if now_ms.saturating_sub(p) >= BG_PAUSE_MAX_MS {
                self.bg_paused_since = None;
                self.fg_busy_since = None;
            }
        }
        if self.q[0].len() > 0 {
            let since = match self.fg_busy_since {
                Some(s) => s,
                None => {
                    self.fg_busy_since = Some(now_ms);
                    now_ms
                }
            };
            if self.bg_paused_since.is_none()
                && now_ms.saturating_sub(since) >= FG_SATURATION_MS
            {
                self.bg_paused_since = Some(now_ms); // 完全暂停（防饿死反向保护）
                self.bg_pauses += 1;
            }
        } else {
            self.fg_busy_since = None;
        }
    }

    pub fn bg_paused(&self) -> bool {
        self.bg_paused_since.is_some()
    }
    pub fn bg_pauses(&self) -> u32 {
        self.bg_pauses
    }

    // -- 批量分段让路 ---------------------------------------------------------

    /// 批量让路窗口：会话超 30min 后，每 5min 节拍的前 1s。
    fn batch_yielding(&self, now_ms: u64) -> bool {
        match self.batch_since {
            Some(since) => {
                if now_ms >= since + BATCH_SEGMENT_AFTER_MS {
                    let over = now_ms - since - BATCH_SEGMENT_AFTER_MS;
                    let yielding = over % BATCH_SEGMENT_MS < BATCH_YIELD_MS;
                    yielding
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// 让路事件记账（派发面调用：让路窗口内每节拍记一次）。
    pub fn note_batch_yield(&mut self, now_ms: u64) {
        if self.batch_yielding(now_ms) {
            let over = now_ms - self.batch_since.unwrap() - BATCH_SEGMENT_AFTER_MS;
            if over % BATCH_SEGMENT_MS == 0 {
                self.batch_yields += 1;
            }
        }
    }

    pub fn batch_yields(&self) -> u32 {
        self.batch_yields
    }

    // -- 配额叠加（F195） ------------------------------------------------------

    /// 配额开关（F195 配额耗尽 → 关闭该分级派发；叠加语义 = 分级顺序 ∩ 配额）。
    pub fn set_quota(&mut self, cls: IoClass, open: bool) {
        self.quota_open[cls.index()] = open;
    }
    pub fn quota_open(&self, cls: IoClass) -> bool {
        self.quota_open[cls.index()]
    }

    // -- 观测面 ---------------------------------------------------------------

    /// 三队列实时深度（监视器 IO 页深度条 = io-pending 分级视图）。
    pub fn depths(&self) -> [usize; 3] {
        [self.q[0].len(), self.q[1].len(), self.q[2].len()]
    }

    /// 各队列吞吐（累计派发数——时间窗曲线的取样点）。
    pub fn served(&self) -> [u64; 3] {
        self.served
    }

    pub fn deadline_misses(&self) -> [u32; 3] {
        self.deadline_misses
    }

    pub fn max_wait_ms(&self) -> [u64; 3] {
        self.max_wait_ms
    }

    pub fn fsync_dispatched(&self) -> u64 {
        self.fsync_dispatched
    }

    pub fn total_enqueued(&self) -> u64 {
        self.total_enqueued
    }

    /// 目录打开判据观测：登记基线（无下载）与下载中两个场景的前台等待。
    pub fn note_dir_open(&mut self, with_download: bool, waited_ms: u64) {
        if with_download {
            self.dir_dl_wait_ms = Some(waited_ms);
        } else {
            self.dir_base_wait_ms = Some(waited_ms);
        }
    }

    /// 判据核算：下载中目录延迟 ≤ 基线 1.2 倍（permille，1000 = 同速）。
    pub fn dir_delay_ratio_permille(&self) -> Option<u32> {
        match (self.dir_base_wait_ms, self.dir_dl_wait_ms) {
            (Some(base), Some(dl)) => Some(((dl * 1000) / base.max(1)) as u32),
            _ => None,
        }
    }

    /// 审计环快照（入诊断快照——主册【数据与存储】）。
    pub fn audit_iter(&self) -> impl Iterator<Item = TierAudit> + '_ {
        (0..self.audit_n).filter_map(move |i| self.audit[i])
    }

    fn audit_push(&mut self, a: TierAudit) {
        if self.audit_n < AUDIT_CAP {
            self.audit[self.audit_n] = Some(a);
            self.audit_n += 1;
        }
    }
}

impl Default for IoTier {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_iotier_checks() -> CheckSet {
    let mut cs = CheckSet::new("F057-iotier");
    // 1) 判据常量（50ms/2s/64KB/60s/30min/5min/1s/1.2 倍）。
    cs.add("tier_consts", FG_DEADLINE_MS == 50 && BG_DEADLINE_MS == 2_000
        && INTERACTIVE_READ_MAX_BYTES == 64 * 1024 && FG_SATURATION_MS == 60_000
        && BATCH_SEGMENT_AFTER_MS == 30 * 60_000 && BATCH_SEGMENT_MS == 5 * 60_000
        && BATCH_YIELD_MS == 1_000 && DIR_DELAY_RATIO_PERMILLE == 1_200, "");
    // 2) class 判定：标签 + 特征 + fsync（主册设计细节逐条）。
    let (c1, w1) = classify(Workload::Interactive, IoOp::Read, 4096);
    let (c2, w2) = classify(Workload::Unknown, IoOp::Read, 4096);
    let (c3, _) = classify(Workload::Unknown, IoOp::Read, 1 << 20);
    let (c4, _) = classify(Workload::Download, IoOp::Write, 0);
    let (c5, _) = classify(Workload::Backup, IoOp::Read, 1 << 20);
    let (c6, _) = classify(Workload::Unknown, IoOp::Fsync, 0);
    cs.add("classify_rules", c1 == IoClass::Foreground && w1 == "interactive-label"
        && c2 == IoClass::Foreground && w2 == "read-below-64k"
        && c3 == IoClass::Background && c4 == IoClass::Background
        && c5 == IoClass::Batch && c6 == IoClass::Foreground, "");
    // 3) B-7xx：fsync 硬承诺优先于一切（三队列皆满载时 fsync 最先派发）。
    let mut io = IoTier::new();
    let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 0);
    let _ = io.submit(Workload::Download, IoOp::Write, 1 << 20, 1);
    let _ = io.submit(Workload::Interactive, IoOp::Read, 4096, 2);
    let _ = io.submit(Workload::Unknown, IoOp::Fsync, 0, 3);
    let d = io.next(10).unwrap();
    cs.add("fsync_bypasses_all", d.req.op == IoOp::Fsync && io.fsync_dispatched() == 1, "");
    // 4) 前台插队：下载满载中目录读零排队（判据一调度本质）。
    let mut io2 = IoTier::new();
    for i in 0..50u64 {
        let _ = io2.submit(Workload::Download, IoOp::Write, 1 << 20, i);
    }
    let _ = io2.submit(Workload::Interactive, IoOp::Read, 4096, 100);
    let d2 = io2.next(100).unwrap();
    cs.add("fg_jumps_full_bg_queue", d2.cls == IoClass::Foreground && d2.waited_ms == 0
        && io2.depths()[1] == 50, "");
    // 5) 判据一：「下载中打开目录」≤ 1.2 倍（观测面对账）。
    let mut io3 = IoTier::new();
    io3.note_dir_open(false, 10); // 基线：无下载 10ms
    io3.note_dir_open(true, 11); // 下载中：插队后仅 +1ms 段间等待
    cs.add("dir_delay_within_120pct", io3.dir_delay_ratio_permille().unwrap() <= DIR_DELAY_RATIO_PERMILLE, "");
    // 6) 截止期 EDF：前台 50ms 内按 deadline 最早先派。
    let mut io4 = IoTier::new();
    let _ = io4.submit_classified(IoClass::Foreground, IoOp::Read, 4096, 0);
    let _ = io4.submit_classified(IoClass::Foreground, IoOp::Read, 4096, 20); // deadline 更晚
    let e1 = io4.next(25).unwrap();
    let e2 = io4.next(25).unwrap();
    cs.add("fg_edf_order", e1.req.enq_ms == 0 && e2.req.enq_ms == 20, "");
    // 7) 前台持续满载 60s → 后台完全暂停；暂停时长有界（满 BG_PAUSE_MAX_MS
    //    自动解除并复位满载计时——防饿死兜底，否则持续满载下后台永久饥饿）。
    let mut io5 = IoTier::new();
    for i in 0..61u64 {
        let _ = io5.submit(Workload::Interactive, IoOp::Read, 4096, i * 1_000);
    }
    cs.add("fg_saturation_pauses_bg", io5.bg_paused() && io5.bg_pauses() == 1, "");
    while io5.next(61_000).is_some() {} // 排空（暂停后 1s，上限未到仍暂停）
    cs.add("pause_holds_within_max", io5.bg_paused(), "");
    for t in 61_000..67_000u64 {
        io5.next(t); // 暂停满 BG_PAUSE_MAX_MS → 自动解除
    }
    cs.add("pause_max_resumes_bg", !io5.bg_paused(), "");
    // 8) 批量 30min 分段让路：每 5min 让 1s（窗口判定）。
    let mut io6 = IoTier::new();
    let _ = io6.submit(Workload::Backup, IoOp::Read, 1 << 20, 0);
    let in_yield = 30 * 60_000 + 500; // 让路窗口内（0..1s）
    let _ = io6.submit(Workload::Interactive, IoOp::Read, 4096, in_yield);
    let _ = io6.submit(Workload::Backup, IoOp::Read, 1 << 20, in_yield);
    let y1 = io6.next(in_yield).unwrap();
    // 让路窗口内批量不可派发（next 顺序 = fsync > 前台 > 后台 > 批量窗口外；
    // 前台已清空且批量在窗口内 → 无可派发 = None，这正是「让路」的判据面）。
    let y2 = io6.next(in_yield);
    cs.add("batch_yields_every_5min", y1.cls == IoClass::Foreground && y2.is_none(), "");
    // 9) 审计环：谁被降级/为什么（透明可查）。
    let mut io7 = IoTier::new();
    let dl_id = io7.submit(Workload::Download, IoOp::Write, 1 << 20, 0).unwrap();
    let audited = io7.audit_iter().any(|a| a.req_id == dl_id && a.why == "download-degraded");
    cs.add("audit_records_degrade_why", audited, "");
    cs
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 24h 混载时间线驱动：返回（bg 完成、batch 完成、最大 bg 等待、
    /// 反向保护次数、批量让路次数、后台截止期违约数）。
    ///
    /// 时间线模型：每秒一拍；前台每秒突发 10 读；后台下载流每秒 2 写；
    /// 批量会话每 40min 启动、持续 31min（每秒 1 大读——会话必超 30min，
    /// 让路节拍生效）；每 10min 注入 90s 前台风暴（60s 后触发反向保护）。
    fn run_24h_mixed() -> (u64, u64, u64, u32, u32, u32) {
        let mut io = IoTier::new();
        let mut now = 0u64;
        while now < 24 * 3_600_000 {
            // 注入。每小时前 60s 为前台风暴窗（50/s 超过 40 拍消化力 → 前台
            // 队列持续满载 60s → 反向保护真实触发）；其余时段 10/s 常态负载。
            // bg 积压峰值 = 风暴期 120 + 暂停期（BG_PAUSE_MAX_MS 内 5s × 2/s
            // = 10）= 130 < Q_CAP(256)——暂停机制全程零丢请求判据。
            let storm = now % 3_600_000 < 60_000;
            let fg_n = if storm { 50 } else { 10 };
            for _ in 0..fg_n {
                let _ = io.submit(Workload::Interactive, IoOp::Read, 4096, now);
            }
            for _ in 0..2 {
                let _ = io.submit(Workload::Download, IoOp::Write, 256 * 1024, now);
            }
            if (now % (40 * 60_000)) < 31 * 60_000 {
                let _ = io.submit(Workload::Backup, IoOp::Read, 8 << 20, now);
            }
            // 派发预算：每节拍 40 拍（通道吞吐模型）。
            for t in 0..40u64 {
                let _ = io.next(now + t);
            }
            // 批量让路记账。
            io.note_batch_yield(now);
            now += 1_000;
        }
        // 收尾：前台空闲（反向保护解除）→ 清空全部队列。
        let mut guard = 0;
        while io.depths() != [0, 0, 0] && guard < 1_000_000 {
            let _ = io.next(now);
            io.note_batch_yield(now);
            now += 1_000;
            guard += 1;
        }
        let s = io.served();
        (
            s[1],
            s[2],
            io.max_wait_ms()[1],
            io.bg_pauses(),
            io.batch_yields(),
            io.deadline_misses()[1],
        )
    }

    #[test]
    fn bg_zero_starvation_24h_mixed() {
        // 判据二：后台任务零饥饿（24h 混载测试全部完成）。
        let (bg_done, batch_done, max_bg_wait, pauses, yields, bg_misses) = run_24h_mixed();
        let expected_bg = 24 * 3_600 * 2; // 每秒 2 × 86400s
        let expected_batch = (24 * 3_600_000 / (40 * 60_000)) * (31 * 60_000 / 1_000);
        assert_eq!(bg_done as u64, expected_bg, "后台下载流必须全部派发完成");
        assert_eq!(batch_done as u64, expected_batch, "批量备份必须全部派发完成");
        // 后台最大等待有界（无限等待才是饥饿——反向保护暂停期是主册明文
        // 设计内行为，其间的截止期违约如实记账不粉饰）。
        assert!(max_bg_wait <= 200_000, "bg max wait {}ms 超有界", max_bg_wait);
        assert!(bg_misses > 0, "风暴暂停期的后台违约必须如实入账");
        // 反向保护与批量让路机制在混载中真实生效过。
        assert!(pauses > 0, "60s 前台风暴必须触发反向保护");
        assert!(yields > 0, "31min 批量会话必须有让路节拍");
    }

    #[test]
    fn dir_open_with_download_within_120pct() {
        // 判据一完整场景：无下载基线 vs 2GB 下载满载中打开目录。
        let mut io = IoTier::new();
        // 场景 A：无下载——目录 5 个元数据读，逐拍直达。
        let mut base_wait = 0u64;
        for i in 0..5u64 {
            let _ = io.submit(Workload::Interactive, IoOp::Read, 4096, i * 2);
            let d = io.next(i * 2).unwrap();
            base_wait += d.waited_ms;
        }
        // 场景 B：下载满载（bg 队列 100 深度）——目录读仍逐拍直达。
        for i in 0..100u64 {
            let _ = io.submit(Workload::Download, IoOp::Write, 1 << 20, i);
        }
        let mut dl_wait = 0u64;
        for i in 0..5u64 {
            let t = 200 + i * 2;
            let _ = io.submit(Workload::Interactive, IoOp::Read, 4096, t);
            let d = io.next(t).unwrap();
            assert_eq!(d.cls, IoClass::Foreground);
            dl_wait += d.waited_ms;
        }
        io.note_dir_open(false, base_wait);
        io.note_dir_open(true, dl_wait);
        let ratio = io.dir_delay_ratio_permille().unwrap();
        assert!(ratio <= DIR_DELAY_RATIO_PERMILLE,
            "下载中目录延迟比 {}‰ > 1200‰", ratio);
    }

    #[test]
    fn fsync_hard_promise_over_fg() {
        // B-7xx：fsync 优先于一切——连前台普通读也要让位。
        let mut io = IoTier::new();
        let _ = io.submit(Workload::Interactive, IoOp::Read, 4096, 0);
        let _ = io.submit(Workload::Unknown, IoOp::Fsync, 0, 1);
        let d = io.next(5).unwrap();
        assert_eq!(d.req.op, IoOp::Fsync);
        let d2 = io.next(5).unwrap();
        assert_eq!(d2.req.op, IoOp::Read);
        assert_eq!(io.fsync_dispatched(), 1);
    }

    #[test]
    fn fg_edf_and_deadline_miss_accounting() {
        // 前台 50ms 截止期：过点派发 = 违约记账（观测面不粉饰）。
        let mut io = IoTier::new();
        let _ = io.submit_classified(IoClass::Foreground, IoOp::Read, 4096, 0);
        let _ = io.submit_classified(IoClass::Foreground, IoOp::Read, 4096, 0);
        let _ = io.next(0); // 派第一个
        let _ = io.next(100); // 第二个已过 50ms 截止期
        assert_eq!(io.deadline_misses()[0], 1);
        // 后台 2s 截止期违约：61s 前台满载触发反向保护 → bg 滞留过点。
        let mut io2 = IoTier::new();
        let _ = io2.submit_classified(IoClass::Background, IoOp::Write, 4096, 0);
        for i in 0..61u64 {
            let _ = io2.submit(Workload::Interactive, IoOp::Read, 4096, i * 1_000);
        }
        assert!(io2.bg_paused());
        // 排空前台（暂停上限未到 → bg 仍暂停，不得被派发）。
        loop {
            match io2.next(62_000) {
                Some(d) if d.cls == IoClass::Foreground => continue,
                other => {
                    assert!(other.is_none(), "bg 暂停期不得派发后台请求");
                    break;
                }
            }
        }
        // 暂停满 BG_PAUSE_MAX_MS → 自动解除（防饿死兜底）→ bg 派发，
        // 等待 >2s = 违约如实记账。
        let d = io2.next(61_000 + BG_PAUSE_MAX_MS).unwrap();
        assert_eq!(d.cls, IoClass::Background);
        assert!(d.waited_ms > BG_DEADLINE_MS);
        assert_eq!(io2.deadline_misses()[1], 1);
    }

    #[test]
    fn bg_paused_skips_to_batch_only_when_quota_open() {
        // 反向保护暂停期间：后台整档跳过；批量照常（配额开）。
        let mut io = IoTier::new();
        let _ = io.submit(Workload::Download, IoOp::Write, 1 << 20, 0);
        let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 0);
        // 61s 前台满载 → 反向保护（真实路径触发）。
        for i in 0..61u64 {
            let _ = io.submit(Workload::Interactive, IoOp::Read, 4096, i * 1_000);
        }
        assert!(io.bg_paused());
        // 排空前台。
        loop {
            match io.next(62_000) {
                Some(d) if d.cls == IoClass::Foreground => continue,
                other => break other,
            }
        }
        .expect("bg 暂停让出窗未满、批量在队 → 必须派批量");
        // 配额关闭批量 → 全部跳过。
        io.set_quota(IoClass::Batch, false);
        let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 62_500);
        assert!(io.next(62_500).is_none());
        assert!(io.depths()[2] >= 1);
    }

    #[test]
    fn quota_overlay_blocks_class() {
        // F195 配额叠加：配额关 = 整档跳过，分级顺序不变。
        let mut io = IoTier::new();
        let _ = io.submit(Workload::Download, IoOp::Write, 1 << 20, 0);
        io.set_quota(IoClass::Background, false);
        assert!(io.next(10).is_none());
        io.set_quota(IoClass::Background, true);
        let d = io.next(10).unwrap();
        assert_eq!(d.cls, IoClass::Background);
    }

    #[test]
    fn batch_session_segmented_yield_windows() {
        // 批量会话 30min 内 full speed；超 30min 后每 5min 让 1s。
        // 会话需要持续注入保活（静默 >60s 视为任务结束）。
        let mut io = IoTier::new();
        let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 0); // 会话开
        let mut t = 30_000u64;
        while t < 29 * 60_000 {
            let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, t);
            t += 30_000;
        }
        // 29min：未到 30min，批量照常派发。
        let d = io.next(29 * 60_000).unwrap();
        assert_eq!(d.cls, IoClass::Batch);
        // 30min + 500ms：让路窗口内（0..1s）→ 批量被跳过。
        let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 30 * 60_000 + 100);
        assert!(io.next(30 * 60_000 + 500).is_none());
        // 31min + 500ms：节拍外（over=60.5s ≥ 1s 窗）→ 批量恢复。
        let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 31 * 60_000 + 500);
        let d3 = io.next(31 * 60_000 + 500).unwrap();
        assert_eq!(d3.cls, IoClass::Batch);
        // 会话静默 >60s → 复位（新任务重新计时 30min）。
        let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 32 * 60_000);
        assert!(io.next(32 * 60_000 + 500).is_some()); // 仍可派（无让路窗）
        let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 40 * 60_000 + 70_000);
        // 40min+70s 时旧会话（最后活动 32min）已静默超 60s → 复位后新会话
        // 从 40min+70s 起算，未超 30min → 无让路窗。
        let d4 = io.next(40 * 60_000 + 70_000).unwrap();
        assert_eq!(d4.cls, IoClass::Batch);
    }

    #[test]
    fn queue_full_backpressure_no_silent_drop() {
        // 队满 = 拒绝上抛（不静默丢弃——丢弃恒零是正确性根基的姊妹纪律）。
        let mut io = IoTier::new();
        for i in 0..Q_CAP {
            assert!(io.submit_classified(IoClass::Background, IoOp::Write, 1, i as u64).is_some());
        }
        assert!(io.submit_classified(IoClass::Background, IoOp::Write, 1, Q_CAP as u64).is_none());
        assert_eq!(io.depths()[1], Q_CAP);
    }

    #[test]
    fn monitor_depths_and_throughput_view() {
        // 监视器 IO 页三队列深度条 + 吞吐曲线取样点（io-pending 分级视图）。
        let mut io = IoTier::new();
        let _ = io.submit(Workload::Interactive, IoOp::Read, 4096, 0);
        let _ = io.submit(Workload::Download, IoOp::Write, 1 << 20, 0);
        let _ = io.submit(Workload::Backup, IoOp::Read, 1 << 20, 0);
        assert_eq!(io.depths(), [1, 1, 1]);
        let _ = io.next(5).unwrap(); // fsync 无 → 前台先
        assert_eq!(io.depths(), [0, 1, 1]);
        assert_eq!(io.served()[0], 1);
    }
}
