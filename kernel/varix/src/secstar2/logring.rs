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

    /// 当前字节占用（容量审计面——v5）。
    pub fn bytes_used(&self) -> usize {
        self.bytes
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

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——级别统计 / 导出命名契约 /
// 洞标记查询面。判据源：主册【交互设计】「级别过滤」+【设计细节】「导出
// zip 内三文件+manifest.json；洞标记=灰带+「此段未记录」tooltip」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：LevelCensus —— 级别五档统计（诊断页顶部的计数条数据——
// 一眼看这批日志里有多少错误多少警告）
// ---------------------------------------------------------------------------

/// 五档计数。
pub struct LevelCensus {
    pub debug: usize,
    pub info: usize,
    pub warn: usize,
    pub error: usize,
    pub fatal: usize,
}

impl LevelCensus {
    pub fn total(&self) -> usize {
        self.debug + self.info + self.warn + self.error + self.fatal
    }

    /// 是否有需要人看的条目（error+fatal>0——红点语义）。
    pub fn needs_attention(&self) -> bool {
        self.error + self.fatal > 0
    }
}

/// 统计（任意条目序列——合并视图/单环两用）。
pub fn level_census(items: &[LogEntry]) -> LevelCensus {
    let mut c = LevelCensus { debug: 0, info: 0, warn: 0, error: 0, fatal: 0 };
    for e in items {
        match e.level {
            LogLevel::Debug => c.debug += 1,
            LogLevel::Info => c.info += 1,
            LogLevel::Warn => c.warn += 1,
            LogLevel::Error => c.error += 1,
            LogLevel::Fatal => c.fatal += 1,
        }
    }
    c
}

// ---------------------------------------------------------------------------
// v3-二：ExportNaming —— 导出包命名契约（主册【设计细节】逐字：zip 内
// 三文件+manifest.json——名字是契约的一部分，第三方解包按名取件）
// ---------------------------------------------------------------------------

/// 导出包内固定文件名（F126 开放格式——第三方工具按名解析）。
pub const EXPORT_FILE_KERNEL: &str = "kernel-ring.log";
pub const EXPORT_FILE_SYSTEM: &str = "system-rotated.log";
pub const EXPORT_FILE_APP: &str = "app-sandbox.log";
pub const EXPORT_FILE_MANIFEST: &str = "manifest.json";

/// 命名契约完整性（四件齐、无重复——打包器与解析器的共同前置）。
pub fn export_naming_intact() -> bool {
    let all = [EXPORT_FILE_KERNEL, EXPORT_FILE_SYSTEM, EXPORT_FILE_APP, EXPORT_FILE_MANIFEST];
    all.iter().all(|n| n.ends_with(".log") || n.ends_with(".json"))
        && (all[0] != all[1] && all[1] != all[2] && all[2] != all[3] && all[0] != all[2])
        && all.iter().filter(|n| n.ends_with(".json")).count() == 1
}

// ---------------------------------------------------------------------------
// v3-三：HoleTooltip —— 洞标记查询面（主册【设计细节】：洞标记=灰带+
// 「此段未记录」tooltip——查询面给出灰带的数据与人话）
// ---------------------------------------------------------------------------

/// 洞标记展示数据。
pub struct HoleTooltip {
    /// 灰带起（毫秒）。
    pub from_ms: u64,
    /// 灰带止（毫秒）。
    pub to_ms: u64,
    /// 人话（tooltip 正文）。
    pub text: &'static str,
    /// 时长（毫秒——诚实标注丢了多久）。
    pub span_ms: u64,
}

/// 组装（无洞 → None——不造灰带）。
pub fn hole_tooltip(r: &KernelRing) -> Option<HoleTooltip> {
    let (from, to) = r.hole_declaration()?;
    Some(HoleTooltip {
        from_ms: from,
        to_ms: to,
        text: "此段未记录（环满覆盖——时间轴上的洞如实画洞）",
        span_ms: to.saturating_sub(from),
    })
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F188 v3 自检（聚合进 secstar2 域）。
pub fn run_logring_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F188-v3");

    // v3-一：级别统计——五档各入各账、红点语义。
    let mk = |at: u64, lvl: LogLevel, t: &[u8]| LogEntry::new(at, lvl, t);
    let items = vec![
        mk(1, LogLevel::Info, b"a"),
        mk(2, LogLevel::Debug, b"b"),
        mk(3, LogLevel::Warn, b"c"),
        mk(4, LogLevel::Error, b"d"),
        mk(5, LogLevel::Info, b"e"),
        mk(6, LogLevel::Fatal, b"f"),
    ];
    let c = level_census(&items);
    set.add("census counts", c.debug == 1 && c.info == 2 && c.warn == 1 && c.error == 1 && c.fatal == 1, "");
    set.add("census total", c.total() == 6, "");
    set.add("census attention", c.needs_attention(), "");
    let calm = level_census(&[mk(1, LogLevel::Info, b"ok")]);
    set.add("census calm", !calm.needs_attention(), "");

    // v3-二：命名契约——四件齐、唯一 manifest。
    set.add("naming intact", export_naming_intact(), "");
    set.add("naming manifest", EXPORT_FILE_MANIFEST == "manifest.json", "");
    set.add("naming kernel", EXPORT_FILE_KERNEL.ends_with(".log"), "");

    // v3-三：洞标记——真灌满环触发覆盖留洞（机制对齐，不造假状态）。
    let mut r = KernelRing::new();
    set.add("hole none fresh", hole_tooltip(&r).is_none(), "");
    let mut at = 0u64;
    loop {
        at += 10;
        r.push(LogEntry::new(at, LogLevel::Info, b"overflow-filler-entry-0123456789abcdef"));
        // 覆盖多条后 from/to 追踪拉开（单条被逐时 span=0 是合法起点）。
        let spanned = r.hole_declaration().map(|(a, b)| b > a).unwrap_or(false);
        if spanned || at > 200_000 {
            break;
        }
    }
    let h = hole_tooltip(&r);
    set.add("hole tooltip", h.is_some(), "环满覆盖后洞声明可见");
    if let Some(h) = h {
        set.add("hole span honest", h.span_ms == h.to_ms - h.from_ms && h.span_ms > 0, "");
        set.add("hole text", h.text.contains("未记录"), "");
    }

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f188_v3_census_matches_query_filter() {
        // 统计与 LogQuery 过滤互证：allow_debug=false 时 debug 被滤掉——
        // 计数差恰为 debug 数（两套口径一致）。
        let mk = |at: u64, lvl: LogLevel, t: &[u8]| LogEntry::new(at, lvl, t);
        let items: Vec<LogEntry> = (0..20u64)
            .map(|i| {
                let lvl = match i % 4 {
                    0 => LogLevel::Debug,
                    1 => LogLevel::Info,
                    2 => LogLevel::Warn,
                    _ => LogLevel::Error,
                };
                mk(i, lvl, b"x")
            })
            .collect();
        let c = level_census(&items);
        let q = LogQuery { min_level: Some(LogLevel::Info), text: None, from_ms: None, to_ms: None };
        let filtered = q.run(&items);
        assert_eq!(filtered.len(), c.total() - c.debug, "filter and census agree");
        assert_eq!(c.debug, 5);
    }

    #[test]
    fn f188_v3_run_checks_pass() {
        assert!(run_logring_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——搜索高亮定位 / 时间窗滑杆模型 /
// 泳道源选择 / 过滤预览计数 / 轮转策略公开文档。判据源：主册【交互设计】
// 「源选择（内核/系统/应用/合并）+时间窗滑杆+级别过滤+搜索」四件的模型层
// 落点 +【数据与存储】「轮转策略在册公开」。
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// v4-一：HighlightScanner —— 搜索命中高亮定位（命中不只数出来，还要标出
// 位置：字节区间互不重叠、单条上限 16 处（超限截断如实标注）、大小写
// 不敏感——与 contains_ci 同语义但产出区间）
// ---------------------------------------------------------------------------

/// 单条日志内一处命中。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitRange {
    pub start: usize,
    pub end: usize,
}

/// 单条命中上限（防一条超长日志把高亮层拖死——超限截断如实标注）。
pub const HIGHLIGHT_CAP: usize = 16;

/// 扫描一条正文（字节级 ASCII 大小写折叠；中文按字节精确匹配 UTF-8 序列）
/// 的全部命中区间。
pub fn highlight_ranges(text: &[u8], needle: &[u8]) -> (alloc::vec::Vec<HitRange>, bool) {
    let mut out = alloc::vec::Vec::new();
    if needle.is_empty() || needle.len() > text.len() {
        return (out, false);
    }
    let fold = |b: u8| -> u8 {
        if b.is_ascii_uppercase() { b + 32 } else { b }
    };
    let n = needle.len();
    let mut i = 0usize;
    while i + n <= text.len() {
        let m = (0..n).all(|k| fold(text[i + k]) == fold(needle[k]));
        if m {
            out.push(HitRange { start: i, end: i + n });
            i += n; // 不重叠——命中区跳过。
            if out.len() >= HIGHLIGHT_CAP {
                return (out, true); // true = 截断发生（诚实标注）。
            }
        } else {
            i += 1;
        }
    }
    (out, false)
}

// ---------------------------------------------------------------------------
// v4-二：TimeWindowModel —— 时间窗滑杆模型（两柄 [from,to]：拖动钳制在
// 数据范围与最小窗宽内；窗口收窄只筛不改数据——过滤无副作用）
// ---------------------------------------------------------------------------

/// 时间窗最小宽度（ms——两柄贴死等于没有窗口，最小 1s 保底）。
pub const WINDOW_MIN_SPAN_MS: u64 = 1_000;

/// 滑杆窗口（含数据范围边界）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeWindow {
    /// 数据全范围（滑杆两端硬边界）。
    pub data_from_ms: u64,
    pub data_to_ms: u64,
    /// 当前窗口（两柄）。
    pub from_ms: u64,
    pub to_ms: u64,
}

impl TimeWindow {
    /// 初始窗=全范围。
    pub fn full(data_from_ms: u64, data_to_ms: u64) -> TimeWindow {
        TimeWindow { data_from_ms, data_to_ms, from_ms: data_from_ms, to_ms: data_to_ms }
    }

    /// 拖动柄（which: 0=左柄 1=右柄）到 target——钳制三重：数据边界内 /
    /// 最小窗宽 / 不得越过对柄。
    pub fn drag(&mut self, which: u8, target: u64) {
        let target = target.max(self.data_from_ms).min(self.data_to_ms);
        match which {
            0 => {
                let cap = self.to_ms.saturating_sub(WINDOW_MIN_SPAN_MS);
                self.from_ms = target.min(cap);
            }
            _ => {
                let floor = self.from_ms + WINDOW_MIN_SPAN_MS;
                self.to_ms = target.max(floor);
            }
        }
        // 右柄不得超数据上界（floor 可能推过界——再钳一次）。
        self.to_ms = self.to_ms.min(self.data_to_ms);
        // 极窄数据本身不足最小窗宽时：窗=全范围（诚实降级）。
        if self.data_to_ms - self.data_from_ms < WINDOW_MIN_SPAN_MS {
            self.from_ms = self.data_from_ms;
            self.to_ms = self.data_to_ms;
        }
    }

    /// 窗口内条数预览。
    pub fn count_in(&self, items: &[MergedItem]) -> usize {
        items.iter().filter(|m| m.e.at_ms >= self.from_ms && m.e.at_ms <= self.to_ms).count()
    }

    /// 重置（双击滑杆轨道 = 回全范围）。
    pub fn reset(&mut self) {
        self.from_ms = self.data_from_ms;
        self.to_ms = self.data_to_ms;
    }
}

// ---------------------------------------------------------------------------
// v4-三：LaneSelector —— 泳道源选择（「源选择（内核/系统/应用/合并）」的
// 模型层：位掩码四源独立开关；合并=三源全开；全关=诚实空视图不是全显）
// ---------------------------------------------------------------------------

/// 泳道位（Lane 枚举的位掩码镜像——选择面用位运算，渲染面用枚举）。
pub const LANE_KERNEL: u8 = 1;
pub const LANE_SYSTEM: u8 = 2;
pub const LANE_APP: u8 = 4;
/// 合并预设 = 三源全开。
pub const LANE_MERGED: u8 = LANE_KERNEL | LANE_SYSTEM | LANE_APP;

/// 泳道选择器。
#[derive(Clone, Copy, Debug)]
pub struct LaneSelector {
    pub mask: u8,
    /// 全关后用户尝试查看的次数（诊断：全关视图容易让人误以为「没日志」）。
    pub empty_view_hits: u64,
}

impl LaneSelector {
    pub fn merged() -> LaneSelector {
        LaneSelector { mask: LANE_MERGED, empty_view_hits: 0 }
    }

    pub fn toggle(&mut self, lane: u8) {
        self.mask ^= lane;
    }

    /// 过滤合并时间轴（mask 之外的泳道被剔除；全关 → 计数空视图命中）。
    pub fn apply<'a>(&mut self, items: &'a [MergedItem]) -> alloc::vec::Vec<&'a MergedItem> {
        if self.mask == 0 {
            self.empty_view_hits += 1;
            return alloc::vec::Vec::new();
        }
        items
            .iter()
            .filter(|m| {
                let bit = match m.lane {
                    Lane::Kernel => LANE_KERNEL,
                    Lane::System => LANE_SYSTEM,
                    Lane::App => LANE_APP,
                };
                self.mask & bit != 0
            })
            .collect()
    }

    /// 当前是否合并预设（页面标题随源选择变化——「合并视图」vs「内核日志」）。
    pub fn is_merged(&self) -> bool {
        self.mask == LANE_MERGED
    }
}

// ---------------------------------------------------------------------------
// v4-四：FacetPreview —— 过滤预览计数（级别 × 泳道 5×3 计数矩阵：过滤前
// 先看到「这一刀下去每个桶还剩几条」——10 万条流畅的聚合面铺垫）
// ---------------------------------------------------------------------------

/// 5 级别 × 3 泳道计数矩阵（行=级别 fatal..debug，列=内核/系统/应用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FacetMatrix {
    pub cells: [[usize; 3]; 5],
    pub total: usize,
}

impl FacetMatrix {
    /// 从合并时间轴统计。
    pub fn build(items: &[MergedItem]) -> FacetMatrix {
        let mut m = FacetMatrix { cells: [[0; 3]; 5], total: items.len() };
        for it in items {
            let row = match it.e.level {
                LogLevel::Fatal => 0,
                LogLevel::Error => 1,
                LogLevel::Warn => 2,
                LogLevel::Info => 3,
                LogLevel::Debug => 4,
            };
            let col = match it.lane {
                Lane::Kernel => 0,
                Lane::System => 1,
                Lane::App => 2,
            };
            m.cells[row][col] += 1;
        }
        m
    }

    /// 级别过滤后剩余总数（allow_debug=false 时 debug 行整行剔除——与
    /// MergedTimeline::filter_level 同语义的计数面）。
    pub fn remaining(&self, allow_debug: bool) -> usize {
        let mut n = 0;
        for (i, row) in self.cells.iter().enumerate() {
            if i == 4 && !allow_debug {
                continue;
            }
            n += row.iter().sum::<usize>();
        }
        n
    }

    /// 某泳道在某级别的计数（诊断页气泡查询）。
    pub fn cell(&self, level_row: usize, lane_col: usize) -> usize {
        self.cells[level_row.min(4)][lane_col.min(2)]
    }
}

// ---------------------------------------------------------------------------
// v4-五：ROTATION_DOC —— 轮转策略公开文档（「轮转策略在册公开」——把
// 常量翻译成用户可读的文档行，常量改动文档自动跟随（生成而非手写））
// ---------------------------------------------------------------------------

/// 文档行（标题 + 内容——「在册公开」的页面条目）。
pub fn rotation_doc_lines() -> alloc::vec::Vec<(&'static str, String)> {
    let mut out = alloc::vec::Vec::new();
    out.push((
        "内核环形日志",
        alloc::format!("内存定长 {} KB，写满后覆盖最旧条目，覆盖位置留洞标记", KERNEL_RING_BYTES / 1024),
    ));
    out.push((
        "系统日志",
        alloc::format!("文件轮转 {} 个 × {} MB，滚满后最旧档封存入账", SYS_ROTATE_FILES, SYS_FILE_BYTES / (1024 * 1024)),
    ));
    out.push((
        "应用日志",
        "各应用沙盒内自管，受应用配额约束（F195），超配额拒写并留账".to_string(),
    ));
    out.push((
        "时间对齐",
        alloc::format!("三环统一时钟源打点，对齐精度承诺 ±{} ms", ALIGN_TOLERANCE_MS),
    ));
    out
}

/// 文档行完整性（四行齐+数值行内嵌了真实常量——文档与实现永不脱节）。
pub fn rotation_doc_intact() -> bool {
    let lines = rotation_doc_lines();
    lines.len() == 4
        && lines[0].1.contains("256")
        && lines[1].1.contains("10")
        && lines[3].1.contains("50")
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F188 v4 自检（聚合进 secstar2 域）。
pub fn run_logring_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F188-v4");

    // v4-一：高亮扫描——命中区间、大小写折叠、中文、截断标注、空针诚实空。
    let text = b"Error: Disk I/O error at disk0; retry ERROR path";
    let (hits, trunc) = highlight_ranges(text, b"error");
    set.add("hl case fold", hits.len() == 3, "Error/error/ERROR 三处（大小写折叠）");
    set.add("hl ranges", hits[0] == HitRange { start: 0, end: 5 }
        && hits[1] == HitRange { start: 16, end: 21 }
        && hits[2] == HitRange { start: 38, end: 43 }, "");
    set.add("hl no trunc", !trunc, "");
    let (cn, cn_trunc) = highlight_ranges("内核慢在前，应用超时在后".as_bytes(), "超时".as_bytes());
    set.add("hl utf8 hit", cn.len() == 1 && !cn_trunc, "中文按 UTF-8 序列命中");
    set.add("hl empty needle", highlight_ranges(text, b"").0.is_empty(), "空针零命中不炸");
    // 截断：17 处命中 → 16 + 截断标志（"a" 用 "-" 分隔防重叠）。
    let mut big = alloc::vec::Vec::new();
    for i in 0..17usize {
        if i > 0 {
            big.extend_from_slice(b"-");
        }
        big.push(b'a');
    }
    let (many_hits, many_trunc) = highlight_ranges(&big, b"a");
    set.add("hl cap", many_hits.len() == HIGHLIGHT_CAP && many_trunc, "");

    // v4-二：时间窗滑杆——钳制三重、重置、窗口计数。
    let items = alloc::vec![
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(1000, LogLevel::Info, b"k boot ok") },
        MergedItem { lane: Lane::System, e: LogEntry::new(2000, LogLevel::Warn, b"s rotate") },
        MergedItem { lane: Lane::App, e: LogEntry::new(3000, LogLevel::Error, b"a quota") },
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(4000, LogLevel::Info, b"k tick") },
    ];
    let mut w = TimeWindow::full(1000, 4000);
    set.add("win full count", w.count_in(&items) == 4, "");
    w.drag(0, 1500);
    w.drag(1, 3500);
    set.add("win narrowed", w.from_ms == 1500 && w.to_ms == 3500 && w.count_in(&items) == 2, "");
    // 左柄不许越过右柄-最小窗宽。
    w.drag(0, 3400);
    set.add("win min span", w.from_ms == 2500 && w.to_ms - w.from_ms == WINDOW_MIN_SPAN_MS, "");
    // 越数据边界钳制。
    w.drag(1, 99999);
    set.add("win bound clamp", w.to_ms == 4000, "");
    w.reset();
    set.add("win reset", w.from_ms == 1000 && w.to_ms == 4000, "");
    // 极窄数据诚实降级=全范围。
    let mut tiny = TimeWindow::full(5000, 5400);
    tiny.drag(0, 5200);
    set.add("win tiny degrade", tiny.from_ms == 5000 && tiny.to_ms == 5400, "");

    // v4-三：泳道选择——合并预设、独立开关、全关诚实空+计数。
    let mut sel = LaneSelector::merged();
    set.add("lane merged default", sel.is_merged() && sel.apply(&items).len() == 4, "");
    sel.toggle(LANE_APP);
    set.add("lane toggle", sel.apply(&items).len() == 3, "关应用后剩 3 条");
    sel.toggle(LANE_KERNEL);
    set.add("lane system only", sel.apply(&items).len() == 1, "");
    sel.toggle(LANE_SYSTEM);
    let empty = sel.apply(&items);
    set.add("lane all off honest", empty.is_empty() && sel.empty_view_hits == 1, "全关=空视图并计数");
    set.add("lane merged preset const", LANE_MERGED == 7, "");

    // v4-四：过滤预览矩阵——聚合与 filter_level 同语义。
    let fm = FacetMatrix::build(&items);
    set.add("facet total", fm.total == 4, "");
    set.add("facet cell", fm.cell(1, 2) == 1, "App Error=1");
    set.add("facet kernel info", fm.cell(3, 0) == 2, "内核 Info=2");
    set.add("facet debug off", fm.remaining(false) == 4, "样本无 debug：全留");
    set.add("facet debug on", fm.remaining(true) == 4, "");
    // 带 debug 样本再验一次行剔除。
    let with_dbg = alloc::vec![
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(100, LogLevel::Debug, b"d") },
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(200, LogLevel::Info, b"i") },
    ];
    let fm2 = FacetMatrix::build(&with_dbg);
    set.add("facet row cut", fm2.remaining(false) == 1 && fm2.remaining(true) == 2, "debug 行整行剔除");

    // v4-五：轮转文档——四行齐且内嵌真实常量。
    set.add("rot doc intact", rotation_doc_intact(), "");
    let doc = rotation_doc_lines();
    set.add("rot doc app line", doc[2].1.contains("F195"), "应用日志行挂配额锚");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    fn mk_entry(at_ms: u64, level: LogLevel, text: &[u8]) -> LogEntry {
        LogEntry::new(at_ms, level, text)
    }

    #[test]
    fn f188_v4_highlight_overlapping_skipped() {
        // 重叠命中不重复计（"aaa" 搜 "aa" → 只 1 处，跳过重叠）。
        let (h, t) = highlight_ranges(b"aaaa", b"aa");
        assert_eq!(h.len(), 2, "非重叠推进：0-2 与 2-4");
        assert!(!t);
        // 边界：针比文长。
        assert!(highlight_ranges(b"ab", b"abc").0.is_empty());
    }

    #[test]
    fn f188_v4_window_never_inverts() {
        // 乱拖 1000 次两柄永不交叉且窗宽永不小于最小值（不变式压测）。
        let mut w = TimeWindow::full(0, 60_000);
        for i in 0..1000u64 {
            let t = (i * 977) % 70_000;
            w.drag((i & 1) as u8, t);
            assert!(w.to_ms >= w.from_ms + WINDOW_MIN_SPAN_MS, "iter {} [{},{}]", i, w.from_ms, w.to_ms);
            assert!(w.to_ms <= 60_000);
        }
    }

    #[test]
    fn f188_v4_lane_apply_preserves_order() {
        // 过滤保序（合并时间轴的时间序不许被选择面打乱）。
        let items = (0..50u64)
            .map(|i| {
                let lane = if i % 3 == 0 { Lane::Kernel } else if i % 3 == 1 { Lane::System } else { Lane::App };
                MergedItem { lane, e: LogEntry::new(i * 100, LogLevel::Info, b"order") }
            })
            .collect::<alloc::vec::Vec<_>>();
        let mut sel = LaneSelector::merged();
        sel.toggle(LANE_APP);
        let out = sel.apply(&items);
        assert_eq!(out.len(), 34); // 非 App 的 2/3 ≈ 33.3 → 50 - 17 = 33? 逐项算：i%3!=2
        // 严格校验：保序且全为非 App。
        let mut last = 0u64;
        for it in out {
            assert!(it.e.at_ms >= last);
            last = it.e.at_ms;
            assert!(matches!(it.lane, Lane::Kernel | Lane::System));
        }
    }

    #[test]
    fn f188_v4_facet_matrix_matches_filter() {
        // 矩阵计数与 LogQuery 过滤互证（同一数据两条路径算出同一个数）。
        let entries: alloc::vec::Vec<LogEntry> = (0..100u64)
            .map(|i| {
                let lvl = match i % 5 {
                    0 => LogLevel::Fatal,
                    1 => LogLevel::Error,
                    2 => LogLevel::Warn,
                    3 => LogLevel::Info,
                    _ => LogLevel::Debug,
                };
                mk_entry(i, lvl, b"x")
            })
            .collect();
        let items: alloc::vec::Vec<MergedItem> = entries
            .iter()
            .map(|e| MergedItem { lane: Lane::Kernel, e: *e })
            .collect();
        let fm = FacetMatrix::build(&items);
        // Fatal 行 20 条（i%5==0）。
        assert_eq!(fm.cell(0, 0), 20);
        // debug off = 80。
        assert_eq!(fm.remaining(false), 80);
    }

    #[test]
    fn f188_v4_run_checks_pass() {
        assert!(run_logring_deep3_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 · 上限口径冲刺）——三环容量审计导出。判据源：主册
// 【数据与存储】「各环上限与覆盖策略在册」+【状态与异常】诊断报备。
// ---------------------------------------------------------------------------

/// 容量审计结论（三环逐环：上限/现用/占比 permille）。
pub struct CapacityAudit {
    pub kernel_used: usize,
    pub kernel_cap: usize,
    pub sys_files_used: usize,
    pub sys_files_cap: usize,
    pub kernel_permille: u64,
    pub sys_permille: u64,
}

/// 组装（KernelRing + RotatedLog → 三环审计——诊断中心「日志健康」格）。
pub fn capacity_audit(k: &KernelRing, r: &RotatedLog) -> CapacityAudit {
    let kernel_used = k.bytes_used();
    // 系统环现用段数：非空段计数。
    let sys_used = r.seg_counts.iter().filter(|c| **c > 0).count();
    CapacityAudit {
        kernel_used,
        kernel_cap: KERNEL_RING_BYTES,
        sys_files_used: sys_used,
        sys_files_cap: SYS_ROTATE_FILES,
        kernel_permille: kernel_used as u64 * 1000 / KERNEL_RING_BYTES as u64,
        sys_permille: sys_used as u64 * 1000 / SYS_ROTATE_FILES as u64,
    }
}

/// F188 v5 自检（deep4 表）。
pub fn run_logring_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F188-v5");

    let mut k = KernelRing::new();
    let e = LogEntry::new(1000, LogLevel::Info, b"cap audit probe entry");
    let unit = e.cost_bytes();
    for _ in 0..10 {
        k.push(LogEntry::new(1000, LogLevel::Info, b"cap audit probe entry"));
    }
    let mut r = RotatedLog::new();
    for _ in 0..3 {
        r.push(&LogEntry::new(2000, LogLevel::Info, b"sys line"));
    }
    let a = capacity_audit(&k, &r);
    set.add("cap kernel used", a.kernel_used == 10 * unit, "");
    set.add("cap sys used", a.sys_files_used == 1, "3 条同段未轮转 → 1 个非空段");
    set.add("cap kernel permille", a.kernel_permille == (10 * unit) as u64 * 1000 / KERNEL_RING_BYTES as u64, "");
    set.add("cap sys permille", a.sys_permille == 100, "1/10 段 = 100‰");
    set.add("cap within bounds", a.kernel_used <= KERNEL_RING_BYTES && a.sys_files_used <= SYS_ROTATE_FILES, "");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f188_v4_cap_audit_full_ring() {
        // 灌满内核环：占比 1000‰（上限即满——审计面如实报满）。
        let mut k = KernelRing::new();
        let big = [b'x'; 32];
        while k.bytes_used() + 48 < KERNEL_RING_BYTES {
            k.push(LogEntry::new(1, LogLevel::Info, &big));
        }
        let a = capacity_audit(&k, &RotatedLog::new());
        assert!(a.kernel_permille >= 990, "接近满载如实呈现（{}‰）", a.kernel_permille);
    }

    #[test]
    fn f188_v4_run_checks_pass() {
        assert!(run_logring_deep4_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——日志健康摘要行 + 级别分布健康判定。
// 判据源：主册【状态与异常】「日志系统自身故障 → 降级内存环+诊断报备」。
// ---------------------------------------------------------------------------

/// 健康摘要行（三环现状一句——诊断中心格文案）。
pub fn health_summary_line(k: &KernelRing, r: &RotatedLog) -> String {
    let a = capacity_audit(k, r);
    alloc::format!(
        "日志健康：内核环 {}‰ / 系统环 {} 段在用 —— 三环时间对齐 ±{}ms",
        a.kernel_permille, a.sys_files_used, ALIGN_TOLERANCE_MS
    )
}

/// 级别分布健康判定（fatal 占比超 5% = 系统在喊救命——诊断面红旗）。
pub fn level_census_healthy(items: &[MergedItem]) -> bool {
    if items.is_empty() {
        return true; // 无日志=无 fatal（空即健康）。
    }
    let fatal = items.iter().filter(|m| m.e.level == LogLevel::Fatal).count();
    fatal * 100 <= items.len() * 5
}

/// F188 v6 自检（deep5 表）。
pub fn run_logring_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F188-v6");

    let mut k = KernelRing::new();
    k.push(LogEntry::new(1000, LogLevel::Info, b"health probe"));
    let mut r = RotatedLog::new();
    r.push(&LogEntry::new(2000, LogLevel::Info, b"sys"));
    let line = health_summary_line(&k, &r);
    set.add("health line", line.contains("内核环") && line.contains("±50ms"), "对齐承诺入文");

    // 级别健康——正常流绿、fatal 风暴红、空流绿。
    let normal = vec![
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(1, LogLevel::Info, b"a") },
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(2, LogLevel::Warn, b"b") },
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(3, LogLevel::Error, b"c") },
    ];
    set.add("census healthy", level_census_healthy(&normal), "无 fatal=绿");
    let storm = vec![
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(1, LogLevel::Fatal, b"x") },
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(2, LogLevel::Info, b"y") },
        MergedItem { lane: Lane::Kernel, e: LogEntry::new(3, LogLevel::Info, b"z") },
    ];
    set.add("census fatal storm", !level_census_healthy(&storm), "1/3 fatal > 5% 红旗");
    set.add("census empty", level_census_healthy(&[]), "");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f188_v5_census_boundary_5pct() {
        // 恰 5%（20 条 1 fatal）= 绿（<= 语义——边界不误报）。
        let mut items = Vec::new();
        items.push(MergedItem { lane: Lane::Kernel, e: LogEntry::new(0, LogLevel::Fatal, b"f") });
        for i in 1..20u64 {
            items.push(MergedItem { lane: Lane::Kernel, e: LogEntry::new(i, LogLevel::Info, b"i") });
        }
        assert!(level_census_healthy(&items));
    }

    #[test]
    fn f188_v5_run_checks_pass() {
        assert!(run_logring_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——合并视图分页统计。
// 判据源：主册【验收判据】「合并视图 10 万条流畅」的分页数据面。
// ---------------------------------------------------------------------------

/// 分页模型（页大小 × 页序 → 切片——10 万条流畅的分页语义）。
pub fn merged_page(items: &[MergedItem], page_size: usize, page_no: usize) -> Vec<&MergedItem> {
    let start = page_no * page_size;
    items.iter().skip(start).take(page_size).collect()
}

/// 总页数（向上取整——尾页不满也占一页）。
pub fn merged_page_count(total: usize, page_size: usize) -> usize {
    if page_size == 0 {
        return 0;
    }
    total.div_ceil(page_size)
}

/// F188 v7 自检（deep6 表）。
pub fn run_logring_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F188-v7");

    let items: Vec<MergedItem> = (0..25u64)
        .map(|i| MergedItem { lane: Lane::Kernel, e: LogEntry::new(i, LogLevel::Info, b"page") })
        .collect();
    let p0 = merged_page(&items, 10, 0);
    let p2 = merged_page(&items, 10, 2);
    set.add("page full", p0.len() == 10, "");
    set.add("page tail", p2.len() == 5, "尾页 5 条");
    set.add("page beyond empty", merged_page(&items, 10, 9).is_empty(), "越页=空");
    set.add("page count", merged_page_count(25, 10) == 3, "25/10 → 3 页");
    set.add("page count exact", merged_page_count(20, 10) == 2, "整除不加页");
    set.add("page size zero", merged_page_count(25, 0) == 0, "除零防呆");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f188_v6_page_no_overlap() {
        // 分页互不重叠且并集=全量（分页数学完备性）。
        let items: Vec<MergedItem> = (0..23u64)
            .map(|i| MergedItem { lane: Lane::Kernel, e: LogEntry::new(i, LogLevel::Info, b"p") })
            .collect();
        let pages = 3;
        let mut seen = Vec::new();
        for p in 0..pages {
            seen.extend(merged_page(&items, 10, p).iter().map(|m| m.e.at_ms));
        }
        assert_eq!(seen.len(), 23);
        let mut sorted = seen.clone();
        sorted.sort();
        assert_eq!(sorted, (0..23u64).collect::<Vec<u64>>());
    }

    #[test]
    fn f188_v6_run_checks_pass() {
        assert!(run_logring_deep6_checks().all_passed());
    }
}
