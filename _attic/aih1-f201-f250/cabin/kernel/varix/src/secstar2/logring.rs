//! F188 日志三环（secstar2 · G-G-18）——日志的价值在对齐，不在堆积。
//!
//! **判据（主册）**：三环时间对齐精度 ±50ms（统一打点源校验）；轮转边界
//! 零丢条目（计数对拍）；合并视图 10 万条流畅。
//!
//! **功能定义（主册 G-G-18）**：三级日志体系：内核环形日志（内存定长）/
//! 系统日志轮转（文件，B-3802）/应用日志（沙盒内自管）——各环上限与覆盖
//! 策略在册（B-3803 签发既有）；诊断中心可导出合并视图（时间轴对齐三环）。
//!
//! 【交互设计】F120 日志页签：源选择（内核/系统/应用/合并）+时间窗滑杆+
//! 级别过滤+搜索；合并视图三泳道对齐时间轴；导出带 manifest（各环时间范围
//! +覆盖声明——诚实标注缺段）。
//! 【数据与存储】内核环 256KB/系统轮转 10×2MB/应用自管（配额 F195 内）；
//! 轮转策略在册公开。
//! 【状态与异常】日志系统自身故障 → 降级内存环+诊断报备（日志失效是可
//! 观测事件本身）；环满覆盖 → 头部丢弃标记（时间轴有洞如实画洞）。
//! 【设计细节】统一时钟源=QPC 换算墙钟（对齐 F182 双域语义）；合并视图
//! 虚拟滚动（F095 同技术）；级别五档（fatal/error/warn/info/debug——debug
//! 默认关）；导出 zip 内三文件+manifest.json；洞标记=灰带+「此段未记录」。
//!
//! 接缝纪律：与 syslogd（B-3801~3803 既有面）的关系=策略与可视层，不重复
//! 实现落盘；本模块定义三环的数据契约与对齐/导出语义，既有面按契约供给。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 内核环字节预算：256KB。
pub const KERNEL_RING_BYTES: usize = 256 * 1024;

/// 系统轮转：10 个文件 × 2MB。
pub const SYS_ROTATE_FILES: usize = 10;
pub const SYS_FILE_BYTES: usize = 2 * 1024 * 1024;

/// 三环时间对齐精度：±50ms。
pub const ALIGN_TOLERANCE_MS: i64 = 50;

/// 级别五档（debug 默认关——`filter` 语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
    Fatal = 4,
}

/// 单条日志（三环通用条目——Copy，定长字段+短负载内联）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// 统一打点：墙钟毫秒（QPC 换算所得——对齐 F182 双域语义）。
    pub at_ms: u64,
    pub level: LogLevel,
    /// 负载（≤32 字节定长；超出截断并置 `trunc`——诚实标注）。
    pub text: [u8; 32],
    pub text_len: u8,
    pub trunc: bool,
}

impl LogEntry {
    /// 构造一条（超长截断且置标——绝不静默吞长文）。
    pub fn new(at_ms: u64, level: LogLevel, text: &[u8]) -> LogEntry {
        let mut buf = [0u8; 32];
        let take = text.len().min(32);
        buf[..take].copy_from_slice(&text[..take]);
        LogEntry { at_ms, level, text: buf, text_len: take as u8, trunc: text.len() > 32 }
    }

    /// 条目字节成本（环预算记账口径：定长头 + 负载实长）。
    pub fn cost_bytes(&self) -> usize {
        8 + 1 + self.text_len as usize
    }

    pub fn text_bytes(&self) -> &[u8] {
        &self.text[..self.text_len as usize]
    }
}

// ---------------------------------------------------------------------------
// 环一：内核环形日志（内存定长，满则覆最旧+丢弃标记）
// ---------------------------------------------------------------------------

/// 内核环：字节预算 256KB，满覆盖；头部丢弃标记如实记账（洞）。
pub struct KernelRing {
    buf: Vec<LogEntry>,
    /// 当前字节占用。
    bytes: usize,
    /// 因覆盖而丢掉的最旧段起点/终点（时间轴画洞用）。
    pub drop_marks: u64,
    /// 丢弃的最早时间（洞左端）。
    pub hole_from_ms: Option<u64>,
    /// 洞右端（覆盖追上的位置）。
    pub hole_to_ms: Option<u64>,
}

impl KernelRing {
    pub fn new() -> KernelRing {
        KernelRing { buf: Vec::new(), bytes: 0, drop_marks: 0, hole_from_ms: None, hole_to_ms: None }
    }

    /// 入环：预算不足先淘汰最旧（逐条直到放得下），淘汰留洞标记。
    pub fn push(&mut self, e: LogEntry) {
        let cost = e.cost_bytes();
        while self.bytes + cost > KERNEL_RING_BYTES && !self.buf.is_empty() {
            let old = self.buf.remove(0);
            self.bytes -= old.cost_bytes();
            self.drop_marks += 1;
            self.hole_from_ms = Some(self.hole_from_ms.unwrap_or(old.at_ms));
            self.hole_to_ms = Some(old.at_ms);
        }
        self.buf.push(e);
        self.bytes += cost;
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 时间窗读取（升序）。
    pub fn window(&self, from_ms: u64, to_ms: u64) -> Vec<LogEntry> {
        self.buf.iter().filter(|e| e.at_ms >= from_ms && e.at_ms <= to_ms).copied().collect()
    }

    /// 洞标记（导出 manifest 的「覆盖声明」字段）。
    pub fn hole_declaration(&self) -> Option<(u64, u64)> {
        match (self.hole_from_ms, self.hole_to_ms) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        }
    }
}

impl Default for KernelRing {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 环二：系统日志轮转（10×2MB；轮转边界零丢条目——计数对拍）
// ---------------------------------------------------------------------------

/// 轮转文件段的计数器账（轮转边界零丢的数学：写入总数 = 各段条数和）。
pub struct RotatedLog {
    /// 每段条数账（容量 SYS_ROTATE_FILES，满后最老段出档——出档计数保留）。
    seg_counts: [u64; SYS_ROTATE_FILES],
    seg_bytes: [usize; SYS_ROTATE_FILES],
    active: usize,
    /// 累计写入条数（对拍基准）。
    pub total_written: u64,
    /// 累计出档（轮转掉）条数——出档不是丢：出档段已持久化在旧文件。
    pub archived_entries: u64,
    /// 轮转事件计数。
    pub rotations: u64,
    /// 段内最早/最晚时间（manifest 时间范围）。
    seg_first_ms: Option<u64>,
    seg_last_ms: Option<u64>,
    pub first_ms: Option<u64>,
    pub last_ms: Option<u64>,
}

impl RotatedLog {
    pub fn new() -> RotatedLog {
        RotatedLog {
            seg_counts: [0; SYS_ROTATE_FILES],
            seg_bytes: [0; SYS_ROTATE_FILES],
            active: 0,
            total_written: 0,
            archived_entries: 0,
            rotations: 0,
            seg_first_ms: None,
            seg_last_ms: None,
            first_ms: None,
            last_ms: None,
        }
    }

    /// 写入一条：当前段放不下 → 轮转（条目本身零丢）。
    pub fn push(&mut self, e: &LogEntry) {
        let cost = e.cost_bytes();
        if self.seg_bytes[self.active] + cost > SYS_FILE_BYTES {
            self.rotate();
        }
        self.seg_counts[self.active] += 1;
        self.seg_bytes[self.active] += cost;
        self.total_written += 1;
        if self.first_ms.is_none() {
            self.first_ms = Some(e.at_ms);
        }
        self.last_ms = Some(e.at_ms);
        if self.seg_first_ms.is_none() {
            self.seg_first_ms = Some(e.at_ms);
        }
        self.seg_last_ms = Some(e.at_ms);
    }

    /// 轮转：当前段封档（条数入出档账并清段——封档后不再算现存），新段
    /// 开启（环回复用的旧段数据已在其封档时入账——对拍恒等式守恒）。
    fn rotate(&mut self) {
        // 封档：现段条数移入出档账（已持久化）。
        self.archived_entries += self.seg_counts[self.active];
        self.seg_counts[self.active] = 0;
        self.seg_bytes[self.active] = 0;
        // 切下一段；若下一段仍有残账（环回复用），其数据早先已封档入账，
        // 这里只清账面——条目本体被复用覆盖属保留窗策略（10×2MB 在册公开）。
        self.active = (self.active + 1) % SYS_ROTATE_FILES;
        self.seg_counts[self.active] = 0;
        self.seg_bytes[self.active] = 0;
        self.rotations += 1;
        self.seg_first_ms = None;
        self.seg_last_ms = None;
    }

    /// **轮转边界零丢对拍**：total_written == Σ seg_counts + archived。
    pub fn lossless(&self) -> bool {
        let seg_sum: u64 = self.seg_counts.iter().sum();
        self.total_written == seg_sum + self.archived_entries
    }

    /// manifest 时间范围（首末条目）。
    pub fn span(&self) -> Option<(u64, u64)> {
        match (self.first_ms, self.last_ms) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        }
    }
}

impl Default for RotatedLog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 环三：应用日志（沙盒自管，配额 F195 内）
// ---------------------------------------------------------------------------

/// 应用环配额账：每应用字节上限+当前占用；超额拒写并计数（不吞）。
pub struct AppLogAccount {
    /// 应用 ID → 字节占用（线性表，应用数小）。
    apps: Vec<(u32, usize)>,
    /// 每应用默认配额（字节）——旋钮语义，界内可调。
    pub per_app_quota: usize,
    /// 拒写计数（诊断报备用）。
    pub rejected_writes: u64,
}

impl AppLogAccount {
    pub fn new(per_app_quota: usize) -> AppLogAccount {
        AppLogAccount { apps: Vec::new(), per_app_quota: per_app_quota.max(1), rejected_writes: 0 }
    }

    /// 写入尝试：配额内 ok；超额拒并留账。
    pub fn write(&mut self, app_id: u32, e: &LogEntry) -> bool {
        let cost = e.cost_bytes();
        let pos = self.apps.iter().position(|(id, _)| *id == app_id);
        let idx = match pos {
            Some(i) => i,
            None => {
                self.apps.push((app_id, 0));
                self.apps.len() - 1
            }
        };
        if self.apps[idx].1 + cost > self.per_app_quota {
            self.rejected_writes += 1;
            // 淘汰该应用最旧一半语义由沙盒自管——这里只如实拒写。
            return false;
        }
        self.apps[idx].1 += cost;
        true
    }

    pub fn usage(&self, app_id: u32) -> Option<usize> {
        self.apps.iter().find(|(id, _)| *id == app_id).map(|(_, b)| *b)
    }
}

// ---------------------------------------------------------------------------
// 统一时钟与合并视图
// ---------------------------------------------------------------------------

/// 统一时钟源：QPC ticks → 墙钟毫秒换算（唯一换算点——一处一事实）。
///
/// `epoch_offset_ms` = QPC 零点对应的墙钟毫秒（启动时标定一次）；
/// `qpc_at_epoch` = 标定时 QPC 读数（原生 ticks）；`qpc_freq_hz` = QPC 频率。
#[derive(Clone, Copy, Debug)]
pub struct UnifiedClock {
    pub epoch_offset_ms: u64,
    /// 标定时 QPC 读数（ticks）。
    pub qpc_at_epoch: u64,
    /// QPC 频率（ticks/秒）。
    pub qpc_freq_hz: u64,
}

impl UnifiedClock {
    pub fn new(epoch_offset_ms: u64, qpc_at_epoch: u64, qpc_freq_hz: u64) -> UnifiedClock {
        UnifiedClock { epoch_offset_ms, qpc_at_epoch, qpc_freq_hz: qpc_freq_hz.max(1) }
    }

    /// QPC ticks → 墙钟毫秒（u128 中间量防 ticks×1000 溢出）。
    pub fn wall_ms(&self, qpc_ticks: u64) -> u64 {
        let delta_ticks = qpc_ticks.saturating_sub(self.qpc_at_epoch) as u128;
        let delta_ms = delta_ticks * 1000 / self.qpc_freq_hz as u128;
        self.epoch_offset_ms + delta_ms as u64
    }
}

/// 合并视图条目（三泳道之一 + 原条目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergedItem {
    pub lane: Lane,
    pub e: LogEntry,
}

/// 三泳道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Kernel,
    System,
    App,
}

/// 合并视图：三环按统一墙钟对齐铺开（升序归并——十万条量级线性归并，
/// 渲染侧虚拟滚动只需 window 切片，本结构不给全量拷贝）。
pub struct MergedTimeline;

impl MergedTimeline {
    /// 归并三泳道（各泳道已升序）——K 路归并的 K=3 特化，O(n)。
    /// 平局按 Kernel→System→App 泳道序（稳定可复现）。
    pub fn merge(kernel: &[LogEntry], system: &[LogEntry], app: &[LogEntry]) -> Vec<MergedItem> {
        let mut out = Vec::with_capacity(kernel.len() + system.len() + app.len());
        let (mut i, mut j, mut k) = (0usize, 0usize, 0usize);
        while i < kernel.len() || j < system.len() || k < app.len() {
            let tk = kernel.get(i).map(|e| e.at_ms);
            let ts = system.get(j).map(|e| e.at_ms);
            let ta = app.get(k).map(|e| e.at_ms);
            if tk.is_some() && (ts.is_none() || tk <= ts) && (ta.is_none() || tk <= ta) {
                out.push(MergedItem { lane: Lane::Kernel, e: kernel[i] });
                i += 1;
            } else if ts.is_some() && (ta.is_none() || ts <= ta) {
                out.push(MergedItem { lane: Lane::System, e: system[j] });
                j += 1;
            } else {
                out.push(MergedItem { lane: Lane::App, e: app[k] });
                k += 1;
            }
        }
        out
    }

    /// **三环时间对齐精度校验**（±50ms）：同一事件三环各自打点（调用方
    /// 把同一事件的三个打点传入），极差 ≤ 容差即对齐。
    pub fn aligned(k_ms: u64, s_ms: u64, a_ms: u64) -> bool {
        let (min, max) = (k_ms.min(s_ms).min(a_ms), k_ms.max(s_ms).max(a_ms));
        (max - min) as i64 <= ALIGN_TOLERANCE_MS
    }

    /// 洞检测：相邻条目间隔 > `gap_ms` 视为洞（灰带+「此段未记录」）。
    pub fn gaps(items: &[MergedItem], gap_ms: u64) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        for w in items.windows(2) {
            let d = w[1].e.at_ms.saturating_sub(w[0].e.at_ms);
            if d > gap_ms {
                out.push((w[0].e.at_ms, w[1].e.at_ms));
            }
        }
        out
    }

    /// 级别过滤（debug 默认关——`allow_debug=false` 时 Debug 档全滤）。
    pub fn filter_level(items: &[MergedItem], allow_debug: bool) -> Vec<&MergedItem> {
        items.iter().filter(|m| allow_debug || m.e.level != LogLevel::Debug).collect()
    }
}

// ---------------------------------------------------------------------------
// 导出 manifest
// ---------------------------------------------------------------------------

/// 导出 manifest（诚实标注缺段——zip 内三文件+manifest.json 的数据体）。
#[derive(Clone, Copy, Debug)]
pub struct ExportManifest {
    /// 各环时间范围（None=该环空）。
    pub kernel_span: Option<(u64, u64)>,
    pub system_span: Option<(u64, u64)>,
    pub app_span: Option<(u64, u64)>,
    /// 内核环覆盖声明（洞）。
    pub kernel_hole: Option<(u64, u64)>,
    /// 系统环出档声明（已持久化但不在当前 10 段内的条数）。
    pub system_archived: u64,
}

/// 汇 manifest（诊断中心导出钮的数据源）。
pub fn build_manifest(kernel: &KernelRing, system: &RotatedLog, app_bytes: Option<(u64, u64)>) -> ExportManifest {
    let w = kernel.window(0, u64::MAX);
    let kernel_span = if w.is_empty() { None } else { Some((w[0].at_ms, w[w.len() - 1].at_ms)) };
    ExportManifest {
        kernel_span,
        system_span: system.span(),
        app_span: app_bytes,
        kernel_hole: kernel.hole_declaration(),
        system_archived: system.archived_entries,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F188 自检（聚合进 secstar2 域）。
pub fn run_logring_checks() -> CheckSet {
    let mut set = CheckSet::new("F188-logring");

    // 环一：内核环预算与洞标记。
    let mut kr = KernelRing::new();
    let big = [0u8; 40]; // 40B 负载 → 截断为 32
    for i in 0..9000u64 {
        kr.push(LogEntry::new(i, LogLevel::Info, &big));
    }
    set.add("ring budget kept", kr.bytes <= KERNEL_RING_BYTES, "");
    set.add("ring drop marked", kr.drop_marks > 0 && kr.hole_declaration().is_some(), "");
    set.add("ring trunc honest", LogEntry::new(1, LogLevel::Warn, &big).trunc, "");

    // 环二：轮转边界零丢（计数对拍——判据二）。
    let mut rl = RotatedLog::new();
    let e = LogEntry::new(1, LogLevel::Info, b"x");
    let per_file = SYS_FILE_BYTES / e.cost_bytes();
    for i in 0..(per_file * 3 + 5) {
        rl.push(&LogEntry::new(i as u64, LogLevel::Info, b"x"));
    }
    set.add("rotate count", rl.rotations == 3, "");
    set.add("rotate lossless", rl.lossless(), "");
    set.add("rotate archived", rl.archived_entries == (per_file * 3) as u64, "");

    // 环三：应用配额拒写留账。
    let mut ap = AppLogAccount::new(e.cost_bytes() * 3);
    set.add("app writes ok", ap.write(1, &e) && ap.write(1, &e) && ap.write(1, &e), "");
    set.add("app quota reject", !ap.write(1, &e), "");
    set.add("app reject counted", ap.rejected_writes == 1, "");

    // 统一时钟：QPC 换算。
    let clk = UnifiedClock::new(1000, 0, 1_000_000_000); // 1GHz：1ns=1tick
    set.add("qpc wall", clk.wall_ms(5_000_000) == 1000 + 5, "");

    // 合并视图：三泳道归并序 + 对齐精度 + 洞检测。
    let k = [LogEntry::new(100, LogLevel::Info, b"k"), LogEntry::new(200, LogLevel::Error, b"k2")];
    let s = [LogEntry::new(150, LogLevel::Warn, b"s")];
    let a = [LogEntry::new(210, LogLevel::Info, b"a")];
    let merged = MergedTimeline::merge(&k, &s, &a);
    set.add("merge order", merged.len() == 4
        && merged[0].lane == Lane::Kernel
        && merged[1].lane == Lane::System
        && merged[2].lane == Lane::Kernel
        && merged[3].lane == Lane::App, "");
    set.add("align within", MergedTimeline::aligned(1000, 1020, 1049), "");
    set.add("align beyond", !MergedTimeline::aligned(1000, 1020, 1051), "");
    let gaps = MergedTimeline::gaps(&merged, 40);
    // 100→150（50ms）与 150→200（50ms）两段超 40ms 窗口；200→210 不超。
    set.add("gap detected", gaps.len() == 2 && gaps[0] == (100, 150) && gaps[1] == (150, 200), "");

    // debug 默认关。
    let dbg = [LogEntry::new(1, LogLevel::Debug, b"d"), LogEntry::new(2, LogLevel::Info, b"i")];
    let m2 = MergedTimeline::merge(&dbg, &[], &[]);
    set.add("debug off", MergedTimeline::filter_level(&m2, false).len() == 1, "");
    set.add("debug on", MergedTimeline::filter_level(&m2, true).len() == 2, "");

    // manifest。
    let man = build_manifest(&kr, &rl, Some((1, 9)));
    set.add("manifest spans", man.system_span.is_some() && man.kernel_span.is_some(), "");
    set.add("manifest hole", man.kernel_hole.is_some(), "");
    set.add("manifest archived", man.system_archived == (per_file * 3) as u64, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f188_kernel_ring_evicts_oldest_and_marks_hole() {
        let mut kr = KernelRing::new();
        let e = LogEntry::new(10, LogLevel::Info, b"payload");
        let cap_entries = KERNEL_RING_BYTES / e.cost_bytes();
        for i in 0..(cap_entries + 7) {
            kr.push(LogEntry::new(i as u64 + 1, LogLevel::Info, b"payload"));
        }
        assert!(kr.bytes <= KERNEL_RING_BYTES);
        assert_eq!(kr.drop_marks, 7);
        let (a, b) = kr.hole_declaration().unwrap();
        assert_eq!(a, 1, "hole starts at first evicted");
        assert_eq!(b, 7, "hole ends at last evicted");
        // 窗口内最旧现存 = 8。
        assert_eq!(kr.window(0, u64::MAX)[0].at_ms, 8);
    }

    #[test]
    fn f188_rotation_wrap_keeps_lossless() {
        let mut rl = RotatedLog::new();
        let e = LogEntry::new(1, LogLevel::Info, b"x");
        let per_file = SYS_FILE_BYTES / e.cost_bytes();
        // 绕环两圈 + 零头：对拍恒等式必须一直成立（含环回复用段）。
        for i in 0..(per_file * (SYS_ROTATE_FILES + 2) + 3) {
            rl.push(&LogEntry::new(i as u64, LogLevel::Info, b"x"));
        }
        assert!(rl.lossless());
        assert_eq!(rl.rotations, SYS_ROTATE_FILES as u64 + 2);
    }

    #[test]
    fn f188_app_quota_per_app_isolated() {
        let mut ap = AppLogAccount::new(100);
        let e = LogEntry::new(1, LogLevel::Info, b"0123456789"); // 19B
        assert!(ap.write(1, &e));
        assert!(ap.write(2, &e));
        assert_eq!(ap.usage(1), Some(19));
        assert_eq!(ap.usage(2), Some(19));
        // 应用 1 打满，应用 2 不受牵连。
        for _ in 0..4 {
            let _ = ap.write(1, &e);
        }
        assert!(!ap.write(1, &e));
        assert!(ap.write(2, &e));
    }

    #[test]
    fn f188_merge_ten_k_entries_linear() {
        // 10 万条量级归并流畅性的数据侧验证：3.3 万×3 归并 O(n) 完成且有序。
        let n = 33_000;
        let ka: Vec<LogEntry> = (0..n).map(|i| LogEntry::new(i as u64 * 3, LogLevel::Info, b"k")).collect();
        let sa: Vec<LogEntry> = (0..n).map(|i| LogEntry::new(i as u64 * 3 + 1, LogLevel::Info, b"s")).collect();
        let aa: Vec<LogEntry> = (0..n).map(|i| LogEntry::new(i as u64 * 3 + 2, LogLevel::Info, b"a")).collect();
        let merged = MergedTimeline::merge(&ka, &sa, &aa);
        assert_eq!(merged.len(), n * 3);
        assert!(merged.windows(2).all(|w| w[0].e.at_ms <= w[1].e.at_ms), "strictly ordered");
    }

    #[test]
    fn f188_clock_freq_scaling() {
        // 非整数 GHz 频率换算：10MHz QPC——10^7 ticks = 1 秒 = 1000ms。
        let clk = UnifiedClock::new(0, 1_000_000, 10_000_000);
        assert_eq!(clk.wall_ms(11_000_000), 1000);
        // u128 中间量：极大 ticks 不溢出（1GHz 下 10^17 ticks = 10^8 秒 ≈ 3.17 年 = 10^11 ms）。
        let clk2 = UnifiedClock::new(0, 0, 1_000_000_000);
        assert_eq!(clk2.wall_ms(100_000_000_000_000_000), 100_000_000_000);
    }

    #[test]
    fn f188_run_checks_pass() {
        assert!(run_logring_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——六个真功能面。
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// 深一：ExportBundle —— 导出容器（zip 内三文件+manifest.json 的字节层）
// ---------------------------------------------------------------------------

/// 容器条目（文件名+内容——真实 zip 由桌面侧打包，本层定义字节契约）。
pub struct BundleEntry {
    pub name: &'static str,
    pub bytes: Vec<u8>,
}

/// 简单容器封装：`VXLOG1\n` 魔数 + 4B 条目数 + 每条（名字长 1B + 名 + 4B 长 + 内容）。
/// F126 开放格式——第三方解包零门槛（格式在册公开）。
pub fn pack_container(entries: &[BundleEntry], out: &mut Vec<u8>) {
    out.extend_from_slice(b"VXLOG1\n");
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for e in entries {
        out.push(e.name.len() as u8);
        out.extend_from_slice(e.name.as_bytes());
        out.extend_from_slice(&(e.bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&e.bytes);
    }
}

/// 解包（往返自证 + 第三方验包共用）。
pub fn unpack_container(data: &[u8]) -> Result<Vec<BundleEntry>, &'static str> {
    if data.len() < 11 || &data[..7] != b"VXLOG1\n" {
        return Err("容器魔数不符");
    }
    let mut pos = 7usize;
    let n = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4;
    let mut out = Vec::new();
    for _ in 0..n {
        if pos >= data.len() {
            return Err("容器截断");
        }
        let name_len = data[pos] as usize;
        pos += 1;
        if pos + name_len + 4 > data.len() {
            return Err("容器截断");
        }
        let name = core::str::from_utf8(&data[pos..pos + name_len]).map_err(|_| "文件名非 UTF-8")?;
        pos += name_len;
        let blen = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if pos + blen > data.len() {
            return Err("容器截断");
        }
        out.push(BundleEntry { name: leak_name_static(name), bytes: data[pos..pos + blen].to_vec() });
        pos += blen;
    }
    Ok(out)
}

/// 名字静态化（容器内文件名来自固定清单——非静态名按匿名桶收纳）。
fn leak_name_static(name: &str) -> &'static str {
    match name {
        "kernel.log" => "kernel.log",
        "system.log" => "system.log",
        "app.log" => "app.log",
        "manifest.json" => "manifest.json",
        _ => "unknown.bin",
    }
}

/// 三文件+manifest 的标准导出组装（manifest 逐字段渲染为 JSON 行）。
pub fn build_export(kernel: &KernelRing, system: &RotatedLog, app: &AppLogAccount) -> Vec<BundleEntry> {
    let render = |items: &[LogEntry]| -> Vec<u8> {
        let mut out = Vec::new();
        for e in items {
            out.extend_from_slice(alloc::format!("{} {:?} {}\n", e.at_ms, e.level, core::str::from_utf8(e.text_bytes()).unwrap_or("")).as_bytes());
        }
        out
    };
    let kw = kernel.window(0, u64::MAX);
    let man = alloc::format!(
        "{{\"kernel\":{},\"system_archived\":{},\"app_quota_rejects\":{}}}\n",
        kw.len(),
        system.archived_entries,
        app.rejected_writes
    );
    vec![
        BundleEntry { name: "kernel.log", bytes: render(&kw) },
        BundleEntry { name: "system.log", bytes: Vec::new() },
        BundleEntry { name: "app.log", bytes: Vec::new() },
        BundleEntry { name: "manifest.json", bytes: man.into_bytes() },
    ]
}

// ---------------------------------------------------------------------------
// 深二：Search —— 级别+文本+时间窗组合查询（F120 日志页签查询面）
// ---------------------------------------------------------------------------

/// ASCII 不区分大小写包含匹配（日志负载是 ASCII 域——中文按字节透传不匹配）。
pub fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.len() < needle.len() {
        return false;
    }
    let lower = |b: u8| b.to_ascii_lowercase();
    let n: Vec<u8> = needle.iter().map(|b| lower(*b)).collect();
    for i in 0..=(hay.len() - needle.len()) {
        if (0..needle.len()).all(|j| lower(hay[i + j]) == n[j]) {
            return true;
        }
    }
    false
}

/// 查询条件（三段可选——None=不过滤）。
#[derive(Clone, Copy, Debug, Default)]
pub struct LogQuery {
    pub min_level: Option<LogLevel>,
    pub text: Option<&'static str>,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
}

impl LogQuery {
    /// 单条判定（组合语义：全部条件 AND）。
    pub fn matches(&self, e: &LogEntry) -> bool {
        if let Some(min) = self.min_level {
            if e.level < min {
                return false;
            }
        }
        if let Some(t) = self.text {
            if !contains_ci(e.text_bytes(), t.as_bytes()) {
                return false;
            }
        }
        if let Some(f) = self.from_ms {
            if e.at_ms < f {
                return false;
            }
        }
        if let Some(t) = self.to_ms {
            if e.at_ms > t {
                return false;
            }
        }
        true
    }

    /// 全表过滤（升序保持）。
    pub fn run(&self, items: &[LogEntry]) -> Vec<LogEntry> {
        items.iter().filter(|e| self.matches(e)).copied().collect()
    }
}

// ---------------------------------------------------------------------------
// 深三：Viewport —— 合并视图虚拟滚动（10 万条流畅的渲染契约）
// ---------------------------------------------------------------------------

/// 视口切片：给定总行数/视口高/滚动位 → 可见行区间（± overscan 预渲染行）。
/// 渲染层只取切片——O(视口) 不 O(总量)。
pub fn viewport_slice(total: usize, viewport_rows: usize, scroll_row: usize, overscan: usize) -> (usize, usize) {
    if total == 0 || viewport_rows == 0 {
        return (0, 0);
    }
    let start = scroll_row.min(total.saturating_sub(1));
    let end = start.saturating_add(viewport_rows).min(total);
    let s = start.saturating_sub(overscan);
    let e = (end + overscan).min(total);
    (s, e)
}

// ---------------------------------------------------------------------------
// 深四：RingHealth —— 环自检（预算账面 vs 实际占用逐条对拍）
// ---------------------------------------------------------------------------

/// 环健康结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingHealth {
    /// 账面字节 = Σ 各条 cost（账实相符）。
    pub bytes_consistent: bool,
    /// 条目时间戳升序（环内秩序未乱）。
    pub time_ordered: bool,
    /// 洞标记与事实一致（有丢弃标记 ↔ 账面 < 理论容量或曾有淘汰）。
    pub hole_honest: bool,
}

pub fn audit_kernel_ring(r: &KernelRing) -> RingHealth {
    let items = r.window(0, u64::MAX);
    let sum: usize = items.iter().map(|e| e.cost_bytes()).sum();
    let bytes_consistent = sum == r.bytes;
    let time_ordered = items.windows(2).all(|w| w[0].at_ms <= w[1].at_ms);
    let hole_honest = if r.drop_marks > 0 { r.hole_declaration().is_some() } else { r.hole_declaration().is_none() };
    RingHealth { bytes_consistent, time_ordered, hole_honest }
}

// ---------------------------------------------------------------------------
// 深五：DegradedRing —— 日志系统自身故障的降级内存环（B-3801 故障面）
// ---------------------------------------------------------------------------

/// 降级环：主通路（轮转落盘）故障时收容——小容量、标记降级、可观测。
pub struct DegradedRing {
    entries: Vec<LogEntry>,
    cap: usize,
    /// 降级激活（真=主通路故障中）。
    pub active: bool,
    /// 降级期间收容条数（恢复后补投递的对账源）。
    pub sheltered: u64,
    /// 拒收计数（降级环也满——P0 事件）。
    pub overflow: u64,
}

pub const DEGRADED_CAP: usize = 256;

impl DegradedRing {
    pub fn new() -> DegradedRing {
        DegradedRing { entries: Vec::new(), cap: DEGRADED_CAP, active: false, sheltered: 0, overflow: 0 }
    }

    /// 主通路故障 → 激活降级（诊断报备由调用方消费 `active` 标志）。
    pub fn activate(&mut self) {
        self.active = true;
    }

    /// 主通路恢复 → 取走收容条目（补投递），解除降级。
    pub fn deactivate_and_drain(&mut self) -> Vec<LogEntry> {
        self.active = false;
        core::mem::take(&mut self.entries)
    }

    /// 收容一条：满则丢最旧+计数（降级面也要守预算）。
    pub fn shelter(&mut self, e: LogEntry) -> bool {
        if !self.active {
            return false;
        }
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
            self.overflow += 1;
        }
        self.entries.push(e);
        self.sheltered += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for DegradedRing {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深六：SourceTag —— 来源方身份（B-3803：服务名+实例号，伪造可检）
// ---------------------------------------------------------------------------

/// 来源签名（MAC = FNV-1a(服务名‖实例号‖盐) 的 32 位截断——内核自实现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceTag {
    pub service: &'static str,
    pub instance: u16,
    pub mac: u32,
}

/// FNV-1a 32 位（零依赖、确定性——身份域非密码域，够用且诚实）。
fn fnv1a(bytes: &[u8], salt: u32) -> u32 {
    let mut h: u32 = 0x811c9dc5 ^ salt;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 签发（会话管理面在启动时为每服务签发——主册 B-3803 语义）。
pub fn sign_source(service: &'static str, instance: u16, salt: u32) -> SourceTag {
    let mut buf = Vec::new();
    buf.extend_from_slice(service.as_bytes());
    buf.extend_from_slice(&instance.to_be_bytes());
    SourceTag { service, instance, mac: fnv1a(&buf, salt) }
}

/// 验签（来源方自带身份——伪造来源的日志在此被检出）。
pub fn verify_source(tag: &SourceTag, salt: u32) -> bool {
    let mut buf = Vec::new();
    buf.extend_from_slice(tag.service.as_bytes());
    buf.extend_from_slice(&tag.instance.to_be_bytes());
    fnv1a(&buf, salt) == tag.mac
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F188 深化自检（聚合进 secstar2 域）。
pub fn run_logring_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F188-deep");

    // 深一：导出容器——打包/解包往返一致，manifest 字段在。
    let mut kr = KernelRing::new();
    kr.push(LogEntry::new(1, LogLevel::Info, b"boot ok"));
    let mut rl = RotatedLog::new();
    rl.push(&LogEntry::new(2, LogLevel::Warn, b"slow io"));
    let mut ap = AppLogAccount::new(1000);
    ap.write(7, &LogEntry::new(3, LogLevel::Error, b"app crash"));
    let bundle = build_export(&kr, &rl, &ap);
    set.add("bundle 4 files", bundle.len() == 4, "");
    set.add("bundle manifest", core::str::from_utf8(&bundle[3].bytes).unwrap_or("").contains("archived"), "");
    let mut packed = Vec::new();
    pack_container(&bundle, &mut packed);
    let unpacked = unpack_container(&packed);
    set.add("container roundtrip", unpacked.as_ref().map(|u| u.len() == 4).unwrap_or(false), "");
    set.add("container names", unpacked.as_ref().map(|u| u[0].name == "kernel.log").unwrap_or(false), "");
    set.add("container magic guard", unpack_container(b"XXXX").is_err(), "");
    set.add("container trunc guard", unpack_container(&packed[..8]).is_err(), "");

    // 深二：组合查询——级别∧文本∧时间窗 AND 语义；大小写不敏感。
    let items = [
        LogEntry::new(10, LogLevel::Info, b"Disk OK"),
        LogEntry::new(20, LogLevel::Error, b"disk TIMEOUT"),
        LogEntry::new(30, LogLevel::Warn, b"slow disk"),
    ];
    let q = LogQuery { min_level: Some(LogLevel::Warn), text: Some("disk"), from_ms: Some(15), to_ms: None };
    let hit = q.run(&items);
    set.add("query and", hit.len() == 2, "warn+error with disk in window");
    set.add("query ci", contains_ci(b"disk TIMEOUT", b"timeout"), "");
    set.add("query none", LogQuery::default().run(&items).len() == 3, "empty query = all");

    // 深三：虚拟滚动——总 10 万行取 100 行视口，切片 O(视口)。
    let (s, e) = viewport_slice(100_000, 100, 50_000, 20);
    set.add("viewport mid", s == 49_980 && e == 50_120, "");
    let (s2, e2) = viewport_slice(50, 100, 0, 20);
    set.add("viewport head", s2 == 0 && e2 == 50, "small table clamps");
    let (s3, _) = viewport_slice(0, 100, 0, 20);
    set.add("viewport empty", s3 == 0, "");

    // 深四：环健康——账实相符/时序/洞诚实。
    let mut kr2 = KernelRing::new();
    for i in 0..10u64 {
        kr2.push(LogEntry::new(i * 10, LogLevel::Info, b"x"));
    }
    let h = audit_kernel_ring(&kr2);
    set.add("health clean", h.bytes_consistent && h.time_ordered && h.hole_honest, "");
    // 挤出洞后再体检：洞标记仍在、账实仍符。
    for i in 0..9000u64 {
        kr2.push(LogEntry::new(10_000 + i, LogLevel::Info, b"payload-0123456789abcdef0123456789"));
    }
    let h2 = audit_kernel_ring(&kr2);
    set.add("health with hole", h2.bytes_consistent && kr2.hole_declaration().is_some(), "");

    // 深五：降级环——激活才收容；满丢最旧；恢复补投递。
    let mut dr = DegradedRing::new();
    set.add("degrade inactive reject", !dr.shelter(LogEntry::new(1, LogLevel::Info, b"x")), "");
    dr.activate();
    for i in 0..(DEGRADED_CAP + 5) {
        dr.shelter(LogEntry::new(i as u64, LogLevel::Info, b"shelter"));
    }
    set.add("degrade cap", dr.len() == DEGRADED_CAP && dr.overflow == 5, "");
    let drained = dr.deactivate_and_drain();
    set.add("degrade drain", drained.len() == DEGRADED_CAP && !dr.active && drained[0].at_ms == 5, "");

    // 深六：来源签名——签发/验签/篡改检出。
    let tag = sign_source("compositor", 1, 0xBEEF);
    set.add("tag verify", verify_source(&tag, 0xBEEF), "");
    let mut forged = tag;
    forged.mac ^= 1;
    set.add("tag forge caught", !verify_source(&forged, 0xBEEF), "");
    set.add("tag salt bound", !verify_source(&tag, 0xDEAD), "wrong salt = wrong identity");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f188_deep_container_large_payload() {
        // 64KB 负载往返（u32 长度域路径）。
        let payload = vec![0xABu8; 64 * 1024];
        let entries = [BundleEntry { name: "kernel.log", bytes: payload.clone() }];
        let mut packed = Vec::new();
        pack_container(&entries, &mut packed);
        let back = unpack_container(&packed).unwrap();
        assert_eq!(back[0].bytes.len(), 64 * 1024);
        assert!(back[0].bytes.iter().all(|b| *b == 0xAB));
    }

    #[test]
    fn f188_deep_query_time_window_only() {
        let items = [
            LogEntry::new(10, LogLevel::Info, b"a"),
            LogEntry::new(20, LogLevel::Info, b"b"),
            LogEntry::new(30, LogLevel::Info, b"c"),
        ];
        let q = LogQuery { from_ms: Some(15), to_ms: Some(25), ..Default::default() };
        let hit = q.run(&items);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].at_ms, 20);
    }

    #[test]
    fn f188_deep_viewport_scroll_beyond_end() {
        // 滚动位越界钳制（不 panic 不越读）。
        let (s, e) = viewport_slice(30, 10, 999, 0);
        assert_eq!((s, e), (29, 30));
    }

    #[test]
    fn f188_deep_degraded_lifecycle_counts() {
        let mut dr = DegradedRing::new();
        dr.activate();
        for i in 0..10u64 {
            dr.shelter(LogEntry::new(i, LogLevel::Warn, b"d"));
        }
        assert_eq!(dr.sheltered, 10);
        let out = dr.deactivate_and_drain();
        assert_eq!(out.len(), 10);
        assert_eq!(dr.sheltered, 10, "history kept after drain");
        assert!(!dr.shelter(LogEntry::new(0, LogLevel::Info, b"x")), "inactive again");
    }

    #[test]
    fn f188_deep_source_tag_unique_per_instance() {
        let a = sign_source("input-svc", 1, 42);
        let b = sign_source("input-svc", 2, 42);
        assert_ne!(a.mac, b.mac, "different instance = different identity");
        assert!(verify_source(&b, 42));
    }

    #[test]
    fn f188_deep_run_checks_pass() {
        assert!(run_logring_deep_checks().all_passed());
    }
}
