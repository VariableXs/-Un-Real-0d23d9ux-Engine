//! F057 IO 调度分级 · 深化件（AI-K1 深化批次三 · G-B-17）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「class 判定：**按发起进程标签（交互面进程=前台）+ 请求特征（读<64KB=交互倾向）**」 | [`ClassPolicy`] 分级判定（标签 + 请求特征双因子，冲突时的裁决规则写死） |
//! | 2 | 【设计细节】「**截止期参数：前台 50ms/后台 2s/批量无截止期**」 | [`DeadlineCalc`] 截止期计算（批量无截止期 ≠ 截止期为 0，语义不同） |
//! | 3 | 【交互设计】「监视器 IO 页**三队列实时深度条 + 各队列吞吐曲线**」 | [`QueueDepthView`] 深度投影 + [`Throughput`] 吞吐曲线（60 秒逐秒） |
//! | 4 | 【交互设计】「**下载类任务自动降级的提示出现在通知中心（透明可查）**」 | [`Notice`] 降级通知（去重节流 + 可查） |
//! | 5 | 【状态与异常】「**分级事件审计（谁被降级/为什么）入诊断快照**」 | [`DegradeAudit`] 降级审计行（任务 id + 原级/新级 + 原因 + 时刻） |
//! | 6 | 【验收判据】「「下载中打开目录」场景**目录延迟 ≤ 无下载时的 1.2 倍**」 | [`SceneMeter`] 场景对拍账（有下载 / 无下载双样本） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink, SecRing};

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 交互倾向的读大小阈值 64KB（主册【设计细节】）。
pub const INTERACTIVE_READ_BYTES: u64 = 64 * 1_024;
/// 前台截止期 50ms。
pub const DEADLINE_FG_MS: u32 = 50;
/// 后台截止期 2s。
pub const DEADLINE_BG_MS: u32 = 2_000;
/// 目录延迟放大判据 1.2 倍（千分 1200）。
pub const SCENE_RATIO_PERMILLE: u32 = 1_200;
/// 三队列深度上限（定长结构）。
pub const QUEUE_CAP: u32 = 256;
/// 降级通知节流（同一任务 60 秒内不重复提示）。
pub const NOTICE_THROTTLE_MS: u64 = 60_000;
/// 审计环容量。
pub const AUDIT_RING: usize = 32;

/// IO 分级（主册三级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoTier {
    /// 前台交互（菜单打开/搜索/缩略图）。
    Foreground = 0,
    /// 后台任务（下载/索引/构建）。
    Background = 1,
    /// 批量（备份/日志轮转）。
    Batch = 2,
}

impl IoTier {
    pub const fn name(self) -> &'static str {
        match self {
            IoTier::Foreground => "前台交互",
            IoTier::Background => "后台任务",
            IoTier::Batch => "批量",
        }
    }
    /// 是否有截止期（批量无截止期——主册原文）。
    pub const fn has_deadline(self) -> bool {
        !matches!(self, IoTier::Batch)
    }
    /// 截止期毫秒（批量返回 None，不是 0——0 会被当成「立刻超时」）。
    pub const fn deadline_ms(self) -> Option<u32> {
        match self {
            IoTier::Foreground => Some(DEADLINE_FG_MS),
            IoTier::Background => Some(DEADLINE_BG_MS),
            IoTier::Batch => None,
        }
    }
    /// 优先级数值（小者优先）。
    pub const fn rank(self) -> u8 {
        self as u8
    }
}

// ---------------------------------------------------------------------------
// 1. 分级判定（进程标签 + 请求特征）
// ---------------------------------------------------------------------------

/// 进程标签（主册「按发起进程标签（交互面进程=前台）」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcTag {
    /// 交互面进程（合成器/文件管理器/搜索等）。
    Interactive,
    /// 服务进程（索引/更新）。
    Service,
    /// 批处理进程（备份/日志）。
    BatchJob,
}

/// 分级判定策略（双因子 + 冲突裁决）。
pub struct ClassPolicy;

impl ClassPolicy {
    /// 判定：`tag` 为进程标签，`bytes` 为请求字节，`is_read` 为是否读请求。
    ///
    /// 裁决规则（写死，不靠调用侧解释）：
    /// - 批处理进程 → 批量级（不看请求特征：备份进程偶尔读小文件也不该插队）；
    /// - 交互进程 → 前台级；
    /// - 服务进程 → 读且 <64KB 视为交互倾向（缩略图/元数据），否则后台级。
    pub fn classify(tag: ProcTag, bytes: u64, is_read: bool) -> IoTier {
        match tag {
            ProcTag::Interactive => IoTier::Foreground,
            ProcTag::BatchJob => IoTier::Batch,
            ProcTag::Service => {
                if is_read && bytes < INTERACTIVE_READ_BYTES {
                    IoTier::Foreground
                } else {
                    IoTier::Background
                }
            }
        }
    }
    /// 服务进程的大读请求不升级为前台（防「大文件伪装成交互」）。
    pub const fn foreground_capable(tag: ProcTag) -> bool {
        !matches!(tag, ProcTag::BatchJob)
    }
    /// 判定说明（审计行要能说清为什么）。
    pub fn why(tag: ProcTag, bytes: u64, is_read: bool) -> &'static str {
        match tag {
            ProcTag::Interactive => "发起者是交互面进程",
            ProcTag::BatchJob => "发起者是批处理进程（不参与前台抢占）",
            ProcTag::Service => {
                if is_read && bytes < INTERACTIVE_READ_BYTES {
                    "服务进程的小读请求（元/缩略图倾向）"
                } else {
                    "服务进程的大块或非读请求"
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 2. 截止期计算
// ---------------------------------------------------------------------------

/// 截止期裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deadline {
    /// 有截止期（给出毫秒）。
    At(u32),
    /// 无截止期（批量：不参与截止期排序，只按权重）。
    None,
}

/// 截止期计算器（主册「截止期 + 权重的混合策略」）。
pub struct DeadlineCalc;

impl DeadlineCalc {
    /// 该级的截止期。
    pub const fn of(t: IoTier) -> Deadline {
        match t.deadline_ms() {
            Some(ms) => Deadline::At(ms),
            None => Deadline::None,
        }
    }
    /// 剩余预算（已等待 `waited_ms`）。无截止期恒返回 None——
    /// 不把「无截止期」折算成 0 或无穷大（两者都会让排序逻辑失真）。
    pub fn remaining(t: IoTier, waited_ms: u32) -> Option<u32> {
        t.deadline_ms().map(|d| d.saturating_sub(waited_ms))
    }
    /// 是否逾期。
    pub fn overdue(t: IoTier, waited_ms: u32) -> bool {
        match t.deadline_ms() {
            Some(d) => waited_ms > d,
            None => false, // 无截止期谈不上逾期
        }
    }
    /// 两级比较（逾期者先；同级按等待时长降序——先来先服务不饿死）。
    pub fn precedes(a: (IoTier, u32), b: (IoTier, u32)) -> bool {
        let (ta, wa) = a;
        let (tb, wb) = b;
        if ta.rank() != tb.rank() {
            return ta.rank() < tb.rank();
        }
        // 同级：逾期的先（避免队头长期不动），其次等待久的先
        let oa = Self::overdue(ta, wa);
        let ob = Self::overdue(tb, wb);
        if oa != ob {
            return oa;
        }
        wa >= wb
    }
}

// ---------------------------------------------------------------------------
// 3. 队列深度投影 + 吞吐曲线（交互设计数据面）
// ---------------------------------------------------------------------------

/// 三队列深度投影（监视器深度条消费）。
#[derive(Clone, Copy, Debug)]
pub struct QueueDepthView {
    pub depth: [u32; 3],
    pub cap: u32,
}

impl QueueDepthView {
    pub fn of(depth: [u32; 3], cap: u32) -> Self {
        QueueDepthView { depth, cap }
    }
    /// 某队列深度千分（深度条长度）。
    pub fn permille(&self, t: IoTier) -> u32 {
        let i = t as usize;
        if self.cap == 0 || self.depth[i] == 0 {
            return 0;
        }
        ((self.depth[i] as u64 * 1000) / self.cap as u64) as u32
    }
    /// 是否接近满载（>90% 提示背压）。
    pub fn near_full(&self, t: IoTier) -> bool {
        self.permille(t) > 900
    }
    /// 总深度千分（整体背压）。
    pub fn total_permille(&self) -> u32 {
        let sum: u64 = self.depth.iter().map(|&d| d as u64).sum();
        if self.cap == 0 {
            return 0;
        }
        ((sum * 1000) / (self.cap as u64 * 3)) as u32
    }
}

/// 三队列吞吐曲线（60 秒逐秒，各队列独立）。
pub struct Throughput {
    fg: SecRing,
    bg: SecRing,
    batch: SecRing,
}

impl Throughput {
    pub const fn new() -> Self {
        Throughput { fg: SecRing::new(), bg: SecRing::new(), batch: SecRing::new() }
    }
    pub fn note(&mut self, t: IoTier, now_ms: u64, bytes: u64) {
        match t {
            IoTier::Foreground => self.fg.note(now_ms, bytes),
            IoTier::Background => self.bg.note(now_ms, bytes),
            IoTier::Batch => self.batch.note(now_ms, bytes),
        }
    }
    pub fn series(&self, t: IoTier, out: &mut [u64]) -> usize {
        match t {
            IoTier::Foreground => self.fg.series(out),
            IoTier::Background => self.bg.series(out),
            IoTier::Batch => self.batch.series(out),
        }
    }
    /// 当前秒吞吐。
    pub fn current(&self, t: IoTier) -> u64 {
        match t {
            IoTier::Foreground => self.fg.current(),
            IoTier::Background => self.bg.current(),
            IoTier::Batch => self.batch.current(),
        }
    }
    /// 后台是否饿死（后台 60 秒吞吐为 0 但队列有积压 = 饿死信号）。
    pub fn bg_starved(&self, bg_depth: u32) -> bool {
        bg_depth > 0 && self.bg.sum() == 0
    }
}

// ---------------------------------------------------------------------------
// 4. 降级通知（透明可查 + 节流）
// ---------------------------------------------------------------------------

/// 降级通知（主册「下载类任务自动降级的提示出现在通知中心」）。
#[derive(Clone, Copy, Debug)]
pub struct Notice {
    /// 上次通知时刻（按任务 id 存，定长）。
    last_ms: [Option<u64>; 8],
    /// 任务 id 槽。
    ids: [u32; 8],
    n: usize,
    /// 已发通知数。
    pub sent: u32,
    /// 被节流抑制的通知数。
    pub suppressed: u32,
}

impl Notice {
    pub const fn new() -> Self {
        Notice { last_ms: [None; 8], ids: [0; 8], n: 0, sent: 0, suppressed: 0 }
    }
    fn slot(&mut self, task_id: u32) -> usize {
        for i in 0..self.n {
            if self.ids[i] == task_id {
                return i;
            }
        }
        if self.n < 8 {
            self.ids[self.n] = task_id;
            self.n += 1;
            return self.n - 1;
        }
        // 槽满复用最旧（第 0 槽）——通知不是关键数据，可覆盖
        0
    }
    /// 请求发一条降级通知（同任务 60 秒内不重复）。
    pub fn request(&mut self, task_id: u32, now_ms: u64) -> bool {
        let i = self.slot(task_id);
        if let Some(last) = self.last_ms[i] {
            if now_ms.saturating_sub(last) < NOTICE_THROTTLE_MS {
                self.suppressed += 1;
                return false;
            }
        }
        self.last_ms[i] = Some(now_ms);
        self.sent += 1;
        true
    }
    /// 通知文案（用户看得懂：为什么变慢了）。
    pub const fn text() -> &'static str {
        "有任务正在后台传输，为保证前台操作流畅，它已被自动降速（可在通知中心查看）"
    }
}

// ---------------------------------------------------------------------------
// 5. 降级审计（谁被降级/为什么，入诊断快照）
// ---------------------------------------------------------------------------

/// 降级原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeCause {
    /// 前台持续满载（反向保护）。
    ForegroundSaturated,
    /// 配额限制（F195 叠加）。
    QuotaLimited,
    /// 批量让路窗口（每 5 分钟让 1s）。
    BatchYieldWindow,
    /// 任务自身声明为可降级。
    SelfDeclared,
}

impl DegradeCause {
    pub fn text(&self) -> &'static str {
        match self {
            DegradeCause::ForegroundSaturated => "前台持续满载，后台让路",
            DegradeCause::QuotaLimited => "触发资源配额（F195 叠加）",
            DegradeCause::BatchYieldWindow => "批量任务让路窗口（每 5 分钟让 1 秒）",
            DegradeCause::SelfDeclared => "任务自身声明可降速",
        }
    }
}

/// 一条降级审计行（主册「谁被降级/为什么」）。
#[derive(Clone, Copy, Debug)]
pub struct DegradeRow {
    pub at_ms: u64,
    pub task_id: u32,
    pub from: IoTier,
    pub to: IoTier,
    pub cause: DegradeCause,
}

/// 降级审计环（定长 32，覆盖最旧；导出进 F174 诊断快照）。
pub struct DegradeAudit {
    ring: [Option<DegradeRow>; AUDIT_RING],
    head: usize,
    filled: usize,
    /// 记录总数（含被覆盖）。
    pub total: u64,
}

impl DegradeAudit {
    pub const fn new() -> Self {
        DegradeAudit { ring: [None; AUDIT_RING], head: 0, filled: 0, total: 0 }
    }
    /// 记一次降级（`sink` 非空时同步进诊断报备）。
    pub fn push(&mut self, row: DegradeRow, sink: Option<&mut DiagSink>) {
        self.ring[self.head] = Some(row);
        self.head = (self.head + 1) % AUDIT_RING;
        self.filled = (self.filled + 1).min(AUDIT_RING);
        self.total += 1;
        if let Some(s) = sink {
            s.push("F057", 1, row.at_ms, DiagSev::Info, row.task_id as u64, row.cause as u64, b"io tier degraded");
        }
    }
    /// 快照导出（时间升序）。
    pub fn snapshot(&self, out: &mut [DegradeRow]) -> usize {
        let n = self.filled.min(out.len());
        let start = (self.head + AUDIT_RING - n) % AUDIT_RING;
        for i in 0..n {
            if let Some(r) = self.ring[(start + i) % AUDIT_RING] {
                out[i] = r;
            }
        }
        n
    }
    /// 按任务查询降级次数（可回答「这个任务被降了多少次」）。
    pub fn count_for(&self, task_id: u32) -> u32 {
        let mut n = 0;
        for i in 0..self.filled {
            let idx = (self.head + AUDIT_RING - self.filled + i) % AUDIT_RING;
            if let Some(r) = self.ring[idx] {
                if r.task_id == task_id {
                    n += 1;
                }
            }
        }
        n
    }
    pub fn len(&self) -> usize {
        self.filled
    }
}

// ---------------------------------------------------------------------------
// 6. 场景对拍（「下载中打开目录」）
// ---------------------------------------------------------------------------

/// 场景对拍账（主册「目录延迟 ≤ 无下载时的 1.2 倍」）。
#[derive(Clone, Copy, Debug)]
pub struct SceneMeter {
    /// 无下载时的目录打开延迟（毫秒）。
    pub baseline_ms: Option<u32>,
    /// 有下载时的目录打开延迟。
    pub with_download_ms: Option<u32>,
    /// 样本数（两侧各自）。
    pub baseline_n: u32,
    pub download_n: u32,
}

impl SceneMeter {
    pub const fn new() -> Self {
        SceneMeter { baseline_ms: None, with_download_ms: None, baseline_n: 0, download_n: 0 }
    }
    /// 喂一个样本（取平均：抖动大的场景单点不可信）。
    pub fn note(&mut self, with_download: bool, ms: u32) {
        if with_download {
            self.with_download_ms = Some(match self.with_download_ms {
                None => ms,
                Some(v) => ((v as u64 * self.download_n as u64 + ms as u64) / (self.download_n as u64 + 1)) as u32,
            });
            self.download_n += 1;
        } else {
            self.baseline_ms = Some(match self.baseline_ms {
                None => ms,
                Some(v) => ((v as u64 * self.baseline_n as u64 + ms as u64) / (self.baseline_n as u64 + 1)) as u32,
            });
            self.baseline_n += 1;
        }
    }
    /// 放大倍率千分。
    pub fn ratio_permille(&self) -> Option<u32> {
        match (self.baseline_ms, self.with_download_ms) {
            (Some(b), Some(d)) if b > 0 => Some(((d as u64 * 1000) / b as u64) as u32),
            _ => None,
        }
    }
    /// 是否达标（≤1.2 倍；缺任一侧数据 = 无法判定，不冒充达标）。
    pub fn passes(&self) -> bool {
        matches!(self.ratio_permille(), Some(r) if r <= SCENE_RATIO_PERMILLE)
    }
    /// 结论文案。
    pub fn verdict(&self) -> &'static str {
        match self.ratio_permille() {
            None => "缺少对照样本（无下载基线未测），无法判定",
            Some(r) if r <= SCENE_RATIO_PERMILLE => "下载中打开目录的延迟未超过无下载时的 1.2 倍",
            Some(_) => "下载中打开目录明显变慢（前台插队未生效，需回炉）",
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F057-iotier-ext");
    // 1) 三级名称与截止期（批量无截止期 ≠ 0）。
    cs.add(
        "tier_deadlines",
        IoTier::Foreground.deadline_ms() == Some(50)
            && IoTier::Background.deadline_ms() == Some(2_000)
            && IoTier::Batch.deadline_ms().is_none()
            && !IoTier::Batch.has_deadline()
            && IoTier::Foreground.rank() == 0,
        "",
    );
    // 2) 分级判定：标签 + 请求特征双因子，冲突裁决写死。
    cs.add(
        "class_policy_two_factors",
        ClassPolicy::classify(ProcTag::Interactive, 10 * 1_024 * 1_024, true) == IoTier::Foreground
            && ClassPolicy::classify(ProcTag::BatchJob, 100, true) == IoTier::Batch // 批进程小读也不插队
            && ClassPolicy::classify(ProcTag::Service, 4 * 1_024, true) == IoTier::Foreground
            && ClassPolicy::classify(ProcTag::Service, 4 * 1_024 * 1_024, true) == IoTier::Background
            && ClassPolicy::classify(ProcTag::Service, 4 * 1_024, false) == IoTier::Background, // 写不享交互倾向
        "",
    );
    cs.add(
        "class_policy_why_text",
        ClassPolicy::why(ProcTag::BatchJob, 100, true) == "发起者是批处理进程（不参与前台抢占）"
            && ClassPolicy::why(ProcTag::Service, 4_096, true) == "服务进程的小读请求（元/缩略图倾向）"
            && !ClassPolicy::foreground_capable(ProcTag::BatchJob),
        "",
    );
    // 3) 截止期：剩余预算与逾期判定（无截止期不谈逾期）。
    cs.add(
        "deadline_remaining_and_overdue",
        DeadlineCalc::remaining(IoTier::Foreground, 20) == Some(30)
            && DeadlineCalc::remaining(IoTier::Batch, 999) .is_none()
            && DeadlineCalc::overdue(IoTier::Foreground, 51)
            && !DeadlineCalc::overdue(IoTier::Batch, 999_999),
        "",
    );
    // 4) 排序：前台优先；同级逾期先；同级同态等待久者先。
    cs.add(
        "deadline_precedence",
        DeadlineCalc::precedes((IoTier::Foreground, 0), (IoTier::Background, 0))
            && DeadlineCalc::precedes((IoTier::Background, 3_000), (IoTier::Background, 10))
            && DeadlineCalc::precedes((IoTier::Background, 900), (IoTier::Background, 100)),
        "",
    );
    // 5) 队列深度投影（深度条 + 背压提示）。
    let qv = QueueDepthView::of([10, 100, 240], QUEUE_CAP);
    cs.add(
        "queue_depth_view",
        qv.permille(IoTier::Foreground) == 39 && qv.near_full(IoTier::Batch) && !qv.near_full(IoTier::Foreground) && qv.total_permille() == 455,
        "",
    );
    let empty = QueueDepthView::of([0, 0, 0], QUEUE_CAP);
    cs.add("queue_depth_zero", empty.permille(IoTier::Foreground) == 0 && empty.total_permille() == 0, "");
    // 6) 吞吐曲线（三队列独立，后台饿死可检出）。
    let mut th = Throughput::new();
    th.note(IoTier::Foreground, 1_000, 500);
    th.note(IoTier::Background, 1_000, 2_000);
    th.note(IoTier::Batch, 1_000, 8_000);
    cs.add(
        "throughput_per_tier",
        th.current(IoTier::Foreground) == 500 && th.current(IoTier::Background) == 2_000 && th.current(IoTier::Batch) == 8_000,
        "",
    );
    let mut out = [0u64; 60];
    th.series(IoTier::Background, &mut out);
    cs.add("throughput_series", out[59] == 2_000, "");
    // 后台有积压却零吞吐 = 饿死信号（不静默）
    let fresh = Throughput::new();
    cs.add("bg_starvation_detected", fresh.bg_starved(10) && !fresh.bg_starved(0), "");
    // 7) 降级通知：节流 + 文案透明。
    let mut nt = Notice::new();
    let first = nt.request(7, 1_000);
    let too_soon = nt.request(7, 1_000 + 1_000);
    let later = nt.request(7, 1_000 + 61_000);
    cs.add(
        "notice_throttled",
        first && !too_soon && later && nt.sent == 2 && nt.suppressed == 1 && Notice::text().contains("自动降速"),
        "",
    );
    // 8) 降级审计：入环 + 进诊断报备 + 按任务可查。
    let mut sink = DiagSink::new();
    let mut da = DegradeAudit::new();
    da.push(
        DegradeRow { at_ms: 1_000, task_id: 7, from: IoTier::Background, to: IoTier::Batch, cause: DegradeCause::ForegroundSaturated },
        Some(&mut sink),
    );
    da.push(
        DegradeRow { at_ms: 2_000, task_id: 7, from: IoTier::Batch, to: IoTier::Background, cause: DegradeCause::QuotaLimited },
        Some(&mut sink),
    );
    da.push(
        DegradeRow { at_ms: 3_000, task_id: 9, from: IoTier::Background, to: IoTier::Batch, cause: DegradeCause::BatchYieldWindow },
        Some(&mut sink),
    );
    let mut rows = [DegradeRow { at_ms: 0, task_id: 0, from: IoTier::Background, to: IoTier::Batch, cause: DegradeCause::QuotaLimited }; 4];
    let n = da.snapshot(&mut rows);
    cs.add(
        "degrade_audit_queryable",
        n == 3 && da.count_for(7) == 2 && da.count_for(9) == 1 && sink.count(DiagSev::Info) == 3 && rows[0].cause.text() == "前台持续满载，后台让路",
        "",
    );
    // 9) 场景对拍（下载中打开目录 ≤ 1.2 倍）。
    let mut sm = SceneMeter::new();
    cs.add("scene_no_baseline", !sm.passes() && sm.verdict() == "缺少对照样本（无下载基线未测），无法判定", "");
    sm.note(false, 100);
    sm.note(false, 120); // 基线均值 110
    sm.note(true, 130);
    cs.add(
        "scene_within_1_2x",
        sm.ratio_permille() == Some(1_181) && sm.passes() && sm.verdict() == "下载中打开目录的延迟未超过无下载时的 1.2 倍",
        "",
    );
    sm.note(true, 300); // 拉高到 215
    cs.add("scene_over_line_detected", !sm.passes() && sm.verdict() == "下载中打开目录明显变慢（前台插队未生效，需回炉）", "");
    // 10) 零基线不可比（不粉饰）。
    let zero = SceneMeter { baseline_ms: Some(0), with_download_ms: Some(100), baseline_n: 1, download_n: 1 };
    cs.add("scene_zero_baseline_not_comparable", zero.ratio_permille().is_none() && !zero.passes(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_never_gains_foreground_privileges() {
        // 批处理进程即便发一个 1 字节读也不得插队（否则备份会拖慢前台）
        assert_eq!(ClassPolicy::classify(ProcTag::BatchJob, 1, true), IoTier::Batch);
        assert!(!ClassPolicy::foreground_capable(ProcTag::BatchJob));
    }

    #[test]
    fn writes_do_not_get_interactive_bias() {
        // 交互倾向只对**读**生效：写请求天生可以合并延迟，不该抢前台
        assert_eq!(ClassPolicy::classify(ProcTag::Service, 100, false), IoTier::Background);
        assert_eq!(ClassPolicy::classify(ProcTag::Service, 100, true), IoTier::Foreground);
    }

    #[test]
    fn no_deadline_is_not_zero_deadline() {
        assert!(DeadlineCalc::remaining(IoTier::Batch, 0).is_none());
        assert!(!DeadlineCalc::overdue(IoTier::Batch, u32::MAX));
        assert_eq!(DeadlineCalc::of(IoTier::Batch), Deadline::None);
    }

    #[test]
    fn throughput_keeps_tiers_separate() {
        let mut t = Throughput::new();
        t.note(IoTier::Foreground, 1_000, 10);
        t.note(IoTier::Background, 1_000, 20);
        t.note(IoTier::Batch, 1_000, 30);
        assert_eq!(t.current(IoTier::Foreground), 10);
        assert_eq!(t.current(IoTier::Background), 20);
        assert_eq!(t.current(IoTier::Batch), 30, "三队列曲线互不串账");
    }

    #[test]
    fn audit_ring_covers_oldest_and_keeps_total() {
        let mut a = DegradeAudit::new();
        for i in 0..(AUDIT_RING + 5) {
            a.push(
                DegradeRow { at_ms: i as u64, task_id: 1, from: IoTier::Background, to: IoTier::Batch, cause: DegradeCause::SelfDeclared },
                None,
            );
        }
        assert_eq!(a.len(), AUDIT_RING);
        assert_eq!(a.total as usize, AUDIT_RING + 5);
        assert_eq!(a.count_for(1), AUDIT_RING as u32);
    }

    #[test]
    fn scene_meter_averages_before_comparing() {
        let mut s = SceneMeter::new();
        for ms in [100u32, 100, 100, 100] {
            s.note(false, ms);
        }
        s.note(true, 121);
        assert_eq!(s.baseline_ms, Some(100));
        assert_eq!(s.ratio_permille(), Some(1_210));
        assert!(!s.passes(), "1.21 倍超过 1.2 倍线，如实判不达标");
    }
}
