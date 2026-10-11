//! unxu05b_vcu05_timemeter —— CGPU-F3205 基准运行器计时与采集（双源对拍）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F3205 · 基准运行器（CGPU-U 域 · 批次 U01 · U05 组 · 判据：场景调度/计时/采集三功能）
//! · 组件：计时引擎（帧分段）+ 采集账（采样守恒）+ 双源对拍 + 域快照
//! · 判据落地：计时/采集三功能之二
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U05B_VERSION: &str = "u05b-vcu05_timemeter-v1";

/// 诊断段基址。
pub const U05B_SEG_BASE: u16 = 0x7310;

// --------------------------------------------------------------------------
// 诊断码（u05b 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 计时采集诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u05bCode {
    /// 0x7310 计时漂移（帧分段与总和不符）。
    TimerDrift,
    /// 0x7311 采样丢失（帧未入账）。
    SampleLost,
    /// 0x7312 双源分歧（软计时≠硬计时）。
    SourceDiverge,
    /// 0x7313 帧号越界。
    FrameIdxOob,
    /// 0x7314 采集账破缺。
    LedgerBreak,
    /// 0x7315 采集序倒挂。
    OrderViolated,
    /// 0x7316 报告陈旧。
    ReportStale,
    /// 0x7317 守卫触发。
    GuardTripped,
    /// 0x7318 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7319 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x731a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x731b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x731c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x731d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u05bCode {
    /// 全量闭集。
    pub const ALL: [u05bCode; 14] = [
        u05bCode::TimerDrift,
        u05bCode::SampleLost,
        u05bCode::SourceDiverge,
        u05bCode::FrameIdxOob,
        u05bCode::LedgerBreak,
        u05bCode::OrderViolated,
        u05bCode::ReportStale,
        u05bCode::GuardTripped,
        u05bCode::LedgerBroken,
        u05bCode::SnapshotFingerprint,
        u05bCode::TableBroken,
        u05bCode::IndexMisaligned,
        u05bCode::DuplicateEntry,
        u05bCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u05bCode::TimerDrift => 0x7310,
            u05bCode::SampleLost => 0x7311,
            u05bCode::SourceDiverge => 0x7312,
            u05bCode::FrameIdxOob => 0x7313,
            u05bCode::LedgerBreak => 0x7314,
            u05bCode::OrderViolated => 0x7315,
            u05bCode::ReportStale => 0x7316,
            u05bCode::GuardTripped => 0x7317,
            u05bCode::LedgerBroken => 0x7318,
            u05bCode::SnapshotFingerprint => 0x7319,
            u05bCode::TableBroken => 0x731a,
            u05bCode::IndexMisaligned => 0x731b,
            u05bCode::DuplicateEntry => 0x731c,
            u05bCode::SegmentGuard => 0x731d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u05bCode::TimerDrift => "计时漂移（帧分段与总和不符）",
            u05bCode::SampleLost => "采样丢失（帧未入账）",
            u05bCode::SourceDiverge => "双源分歧（软计时≠硬计时）",
            u05bCode::FrameIdxOob => "帧号越界",
            u05bCode::LedgerBreak => "采集账破缺",
            u05bCode::OrderViolated => "采集序倒挂",
            u05bCode::ReportStale => "报告陈旧",
            u05bCode::GuardTripped => "守卫触发",
            u05bCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u05bCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u05bCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u05bCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            u05bCode::DuplicateEntry => "条目重复（同键双写）",
            u05bCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u05bCode::TimerDrift => "按分段守恒重建",
            u05bCode::SampleLost => "按采集账补登",
            u05bCode::SourceDiverge => "按对拍规程归因",
            u05bCode::FrameIdxOob => "按帧号律裁剪",
            u05bCode::LedgerBreak => "按守恒重建",
            u05bCode::OrderViolated => "按帧号序重排",
            u05bCode::ReportStale => "重渲染",
            u05bCode::GuardTripped => "检查临界区",
            u05bCode::LedgerBroken => "按守恒律重建台账后重跑",
            u05bCode::SnapshotFingerprint => "按防篡改规程重建快照",
            u05bCode::TableBroken => "按语义规则重建常量表",
            u05bCode::IndexMisaligned => "按声明序归一化后重跑",
            u05bCode::DuplicateEntry => "合并同键条目为末次生效",
            u05bCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 帧分段计时 + 采集账（四段闭段才出时长，软硬双源对拍）
// ---------------------------------------------------------------------------

/// 帧分段（四段：输入采样→提交→GPU 执行→呈现上屏）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FramePhase {
    /// 输入采样段。
    Input,
    /// 提交段。
    Submit,
    /// GPU 执行段。
    Execute,
    /// 呈现上屏段。
    Present,
}

impl FramePhase {
    /// 全量闭集（四段）。
    pub const ALL: [FramePhase; 4] = [
        FramePhase::Input,
        FramePhase::Submit,
        FramePhase::Execute,
        FramePhase::Present,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            FramePhase::Input => 0,
            FramePhase::Submit => 1,
            FramePhase::Execute => 2,
            FramePhase::Present => 3,
        }
    }
}

/// 单帧计时（微秒四段 + 总计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTiming {
    /// 帧号。
    pub frame: u32,
    /// 四段耗时（us）。
    pub seg_us: [u64; 4],
    /// 是否四段齐备（闭段才出总时长）。
    pub closed: bool,
}

impl FrameTiming {
    /// 开帧（帧号入账，四段清零）。
    pub const fn open(frame: u32) -> Self {
        Self { frame, seg_us: [0; 4], closed: false }
    }

    /// 记一段（段内多次记取历史最大）。
    pub fn record(&mut self, phase: FramePhase, us: u64) -> bool {
        if phase.index() >= 4 {
            return false;
        }
        if us > self.seg_us[phase.index()] {
            self.seg_us[phase.index()] = us;
        }
        true
    }

    /// 收帧（四段齐备判据：输入+提交+执行+呈现均 >0）。
    pub fn close(&mut self) -> bool {
        self.closed = self.seg_us.iter().all(|v| *v > 0);
        self.closed
    }

    /// 总时长（未闭段返回 None）。
    pub fn total_us(&self) -> Option<u64> {
        if !self.closed {
            return None;
        }
        Some(self.seg_us[0] + self.seg_us[1] + self.seg_us[2] + self.seg_us[3])
    }
}

/// 采集账（采样守恒 + 软硬双源对拍）。
#[derive(Clone, Copy, Debug)]
pub struct MeterBook {
    /// 已采集帧数。
    pub sampled: u64,
    /// 被拒帧数（未闭段）。
    pub rejected: u64,
    /// 软计时总和。
    pub soft_us: u64,
    /// 硬计时总和。
    pub hard_us: u64,
    /// 分歧帧数。
    pub divergent: u64,
}

impl MeterBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { sampled: 0, rejected: 0, soft_us: 0, hard_us: 0, divergent: 0 }
    }

    /// 采集一帧（双源差值超 1us 记分歧）。
    pub fn sample(&mut self, t: &FrameTiming, hard_total_us: u64) -> bool {
        match t.total_us() {
            Some(v) => {
                self.sampled += 1;
                self.soft_us += v;
                self.hard_us += hard_total_us;
                let diff = if v > hard_total_us { v - hard_total_us } else { hard_total_us - v };
                if diff > 1 {
                    self.divergent += 1;
                }
                true
            }
            None => {
                self.rejected += 1;
                false
            }
        }
    }

    /// 守恒恒等式（sampled + rejected == 提供帧数由调用方累计）。
    pub fn conserved(&self, offered: u64) -> bool {
        self.sampled + self.rejected == offered
    }
}

// --------------------------------------------------------------------------
// 直方图引擎 frame_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 帧总时长直方图（us 档分位数）。
/// 样本容量上限。
pub const FRAME_HIST_CAP: usize = 128;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct frame_histHist {
    /// 样本数组（插入排序维护有序）。
    pub samples: [u64; 128],
    /// 样本数。
    pub n: usize,
    /// 溢出丢弃计数。
    pub drops: u64,
    /// 最小值（空时为 0）。
    pub min: u64,
    /// 最大值。
    pub max: u64,
    /// 累加和。
    pub sum: u64,
}

impl frame_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 128], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= FRAME_HIST_CAP {
            self.drops += 1;
            return;
        }
        let mut j = self.n;
        while j > 0 && self.samples[j - 1] > v {
            self.samples[j] = self.samples[j - 1];
            j -= 1;
        }
        self.samples[j] = v;
        self.n += 1;
        self.sum = self.sum.wrapping_add(v);
        if self.n == 1 || v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
    }

    /// 分位数（permille 口径：q500=P50/q900=P90/q990=P99；最近秩法）。
    pub fn percentile(&self, permille: u64) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let rank = (permille * self.n as u64 + 999) / 1000;
        let idx = if rank == 0 { 1 } else { rank };
        let i = if idx as usize > self.n { self.n - 1 } else { idx as usize - 1 };
        self.samples[i]
    }

    /// 均值（整数除，余数入账）。
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.sum / self.n as u64
    }

    /// 守恒：sum == 各样本和（复算）。
    pub fn conserved(&self) -> bool {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.n {
            s = s.wrapping_add(self.samples[i]);
            i += 1;
        }
        s == self.sum
    }
}

// --------------------------------------------------------------------------
// 引擎 frame_dual（双源对拍：4 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 计时双源对拍（软计时 vs 硬计时，逐帧）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum frame_dualSlot {
    /// 帧计数。
    FrameCount,
    /// 软计时和。
    SoftUs,
    /// 硬计时和。
    HardUs,
    /// 分歧帧数。
    Divergent,
}

impl frame_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [frame_dualSlot; 4] = [
        frame_dualSlot::FrameCount,
        frame_dualSlot::SoftUs,
        frame_dualSlot::HardUs,
        frame_dualSlot::Divergent,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            frame_dualSlot::FrameCount => 0,
            frame_dualSlot::SoftUs => 1,
            frame_dualSlot::HardUs => 2,
            frame_dualSlot::Divergent => 3,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            frame_dualSlot::FrameCount => "帧计数",
            frame_dualSlot::SoftUs => "软计时和",
            frame_dualSlot::HardUs => "硬计时和",
            frame_dualSlot::Divergent => "分歧帧数",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct frame_dualPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct frame_dualDual {
    /// 逐槽样本。
    pub pairs: [frame_dualPair; 4],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl frame_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [frame_dualPair { a: 0, b: 0, same: false, seq: 0 }; 4],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: frame_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = frame_dualPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 4
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 4 && self.same_n + self.diff_n == 4
    }
}

// --------------------------------------------------------------------------
// 快照 msnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 运行器域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct msnap {
    /// 指纹分量：scenes。
    pub scenes: u64,
    /// 指纹分量：phases。
    pub phases: u64,
    /// 指纹分量：sampled。
    pub sampled: u64,
    /// 指纹分量：rejected。
    pub rejected: u64,
    /// 指纹分量：divergent。
    pub divergent: u64,
    /// 指纹分量：hist_n。
    pub hist_n: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl msnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        scenes: u64,
        phases: u64,
        sampled: u64,
        rejected: u64,
        divergent: u64,
        hist_n: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &scenes.to_le_bytes());
        h = fnv1a64_from(h, &phases.to_le_bytes());
        h = fnv1a64_from(h, &sampled.to_le_bytes());
        h = fnv1a64_from(h, &rejected.to_le_bytes());
        h = fnv1a64_from(h, &divergent.to_le_bytes());
        h = fnv1a64_from(h, &hist_n.to_le_bytes());
        Self {
            scenes,
            phases,
            sampled,
            rejected,
            divergent,
            hist_n,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.scenes.to_le_bytes());
        h = fnv1a64_from(h, &self.phases.to_le_bytes());
        h = fnv1a64_from(h, &self.sampled.to_le_bytes());
        h = fnv1a64_from(h, &self.rejected.to_le_bytes());
        h = fnv1a64_from(h, &self.divergent.to_le_bytes());
        h = fnv1a64_from(h, &self.hist_n.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 mdom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 运行器域分量链。
#[derive(Clone, Copy, Debug)]
pub struct mdomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mdomChain {
    /// 链节点序列。
    pub links: [mdomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mdomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mdomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u05bCode> {
        if self.n >= 16 || payload == 0 {
            return Err(u05bCode::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mdomLink { tag, payload, digest };
        self.n += 1;
        self.payload_total += payload;
        self.head = digest;
        Ok(digest)
    }

    /// 全链回放校验（逐环重算指纹 + 负载守恒）。
    pub fn verify(&self) -> bool {
        let mut h = FNV_OFFSET;
        let mut total = 0u64;
        let mut i = 0usize;
        while i < self.n {
            let lk = self.links[i];
            h = fnv1a64_from(h, &lk.payload.to_le_bytes());
            if h != lk.digest {
                return false;
            }
            total += lk.payload;
            i += 1;
        }
        total == self.payload_total && h == self.head
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x7310, 0x731f]。
pub const fn u05b_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u05bCode::ALL.len() {
        let c = u05bCode::ALL[i].code();
        if c < 0x7310 || c > 0x731f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// FNV-1a 64 快照指纹（无第三方依赖的确定性哈希；跨次运行恒同）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基准（标准初值 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数（标准 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 逐字节 FNV-1a 64 递推（xor 后乘，顺序即语义）。
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 64 位值入链（小端字节序，保证跨平台构造一致）。
pub const fn fnv1a64_u64(h: u64, v: u64) -> u64 {
    let b = [
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 24) & 0xff) as u8,
        ((v >> 32) & 0xff) as u8,
        ((v >> 40) & 0xff) as u8,
        ((v >> 48) & 0xff) as u8,
        ((v >> 56) & 0xff) as u8,
    ];
    fnv1a64_from(h, &b)
}

/// 以给定初值继续递推（链式哈希原语）。
pub const fn fnv1a64_from(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn u05b_guard_seg() -> bool {
    u05b_segment_guard()
}

/// 帧分段计时（四段闭段 + 未闭拒收）。
fn check_c1_frame_timing() -> bool {
    let mut t = FrameTiming::open(1);
    let mut ok = t.total_us().is_none();
    ok = ok && t.record(FramePhase::Input, 1200);
    ok = ok && t.record(FramePhase::Submit, 800);
    ok = ok && t.record(FramePhase::Execute, 6500);
    ok = ok && t.record(FramePhase::Present, 1500);
    if ok {
        ok = t.close() && t.total_us() == Some(10000);
    }
    if ok {
        let mut open = FrameTiming::open(2);
        let _ = open.record(FramePhase::Input, 1000);
        ok = !open.close() && open.total_us().is_none();
    }
    ok
}

/// 采集账（双源一致零分歧 + 未闭拒收 + 守恒）。
fn check_c2_meter() -> bool {
    let mut mb = MeterBook::new();
    let mut t = FrameTiming::open(1);
    let _ = t.record(FramePhase::Input, 100);
    let _ = t.record(FramePhase::Submit, 100);
    let _ = t.record(FramePhase::Execute, 8000);
    let _ = t.record(FramePhase::Present, 1800);
    let _ = t.close();
    let mut ok = mb.sample(&t, 10000) && mb.divergent == 0;
    if ok {
        // 未闭帧必须拒收。
        let bad = FrameTiming::open(2);
        ok = !mb.sample(&bad, 5000) && mb.rejected == 1;
    }
    if ok {
        ok = mb.conserved(2) && mb.sampled == 1;
    }
    ok
}

/// 运行器双源对拍（四槽一致 + 负样本）。
fn check_c3_frame_dual() -> bool {
    let mut d = frame_dualDual::new();
    d.sample(frame_dualSlot::FrameCount, 1200, 1200);
    d.sample(frame_dualSlot::SoftUs, 12000000, 12000000);
    d.sample(frame_dualSlot::HardUs, 12000001, 12000001);
    d.sample(frame_dualSlot::Divergent, 0, 0);
    let mut ok = d.all_same();
    if ok {
        d.sample(frame_dualSlot::Divergent, 0, 1);
        ok = !d.all_same() && d.diff_n == 1;
    }
    ok
}

/// 运行器域快照 + 篡改检出。
fn check_c4_snap() -> bool {
    let s = msnap::compute(8, 4, 1200, 0, 0, 1200);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.divergent = 1;
        ok = bad.tampered();
    }
    ok
}

/// 运行器域分量链回放校验。
fn check_c5_chain() -> bool {
    let mut ch = mdomChain::new();
    let mut ok = true;
    for (i, v) in [8u64, 4, 1200, 1, 2, 1200].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 2415;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u05b_checks() -> CheckSet {
    let mut set = CheckSet::new("U05B");
    set.add(
        "u05b-seg-guard",
        u05b_guard_seg(),
        if u05b_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u05b-c1-frame-timing",
        check_c1_frame_timing(),
        if check_c1_frame_timing() { "" } else { "frame phase timing mismatch" },
    );
    set.add(
        "u05b-c2-meter",
        check_c2_meter(),
        if check_c2_meter() { "" } else { "meter book mismatch" },
    );
    set.add(
        "u05b-c3-frame-dual",
        check_c3_frame_dual(),
        if check_c3_frame_dual() { "" } else { "frame dual compare mismatch" },
    );
    set.add(
        "u05b-c4-snap",
        check_c4_snap(),
        if check_c4_snap() { "" } else { "runner domain snapshot mismatch" },
    );
    set.add(
        "u05b-c5-chain",
        check_c5_chain(),
        if check_c5_chain() { "" } else { "runner domain chain mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u05b_all_checks_green() {
        let set = run_u05b_checks();
        assert!(set.all_passed(), "U05B domain checks must be green");
    }

    #[test]
    fn u05b_segment_guard_holds() {
        assert!(u05b_segment_guard());
    }
}
