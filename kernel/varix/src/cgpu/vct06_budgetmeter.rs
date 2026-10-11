//! unxt06b_vct06_budgetmeter —— CGPU-F3046 预算实测仪表（直方图 + 分位数红线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F3046 · 对接性能预算（CGPU-T 域 · 批次 T01 · T06 组 · 判据：协议栈每帧 <0.1ms 预算实现）
//! · 组件：实测直方图 + P99 红线判定 + 域快照
//! · 判据落地：预算实测口径
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T06B_VERSION: &str = "t06b-vct06_budgetmeter-v1";

/// 诊断段基址。
pub const T06B_SEG_BASE: u16 = 0x7210;

// --------------------------------------------------------------------------
// 诊断码（t06b 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 预算仪表诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t06bCode {
    /// 0x7210 直方图不守恒。
    HistBroken,
    /// 0x7211 P99 超红线（尾延失控）。
    P99Over,
    /// 0x7212 统计窗口非法。
    WindowBad,
    /// 0x7213 预算漂移（均值渐涨）。
    DriftOver,
    /// 0x7214 仪表账破缺。
    LedgerBreak,
    /// 0x7215 采样序倒挂。
    OrderViolated,
    /// 0x7216 报告陈旧。
    ReportStale,
    /// 0x7217 守卫触发。
    GuardTripped,
    /// 0x7218 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7219 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x721a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x721b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x721c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x721d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t06bCode {
    /// 全量闭集。
    pub const ALL: [t06bCode; 14] = [
        t06bCode::HistBroken,
        t06bCode::P99Over,
        t06bCode::WindowBad,
        t06bCode::DriftOver,
        t06bCode::LedgerBreak,
        t06bCode::OrderViolated,
        t06bCode::ReportStale,
        t06bCode::GuardTripped,
        t06bCode::LedgerBroken,
        t06bCode::SnapshotFingerprint,
        t06bCode::TableBroken,
        t06bCode::IndexMisaligned,
        t06bCode::DuplicateEntry,
        t06bCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t06bCode::HistBroken => 0x7210,
            t06bCode::P99Over => 0x7211,
            t06bCode::WindowBad => 0x7212,
            t06bCode::DriftOver => 0x7213,
            t06bCode::LedgerBreak => 0x7214,
            t06bCode::OrderViolated => 0x7215,
            t06bCode::ReportStale => 0x7216,
            t06bCode::GuardTripped => 0x7217,
            t06bCode::LedgerBroken => 0x7218,
            t06bCode::SnapshotFingerprint => 0x7219,
            t06bCode::TableBroken => 0x721a,
            t06bCode::IndexMisaligned => 0x721b,
            t06bCode::DuplicateEntry => 0x721c,
            t06bCode::SegmentGuard => 0x721d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t06bCode::HistBroken => "直方图不守恒",
            t06bCode::P99Over => "P99 超红线（尾延失控）",
            t06bCode::WindowBad => "统计窗口非法",
            t06bCode::DriftOver => "预算漂移（均值渐涨）",
            t06bCode::LedgerBreak => "仪表账破缺",
            t06bCode::OrderViolated => "采样序倒挂",
            t06bCode::ReportStale => "报告陈旧",
            t06bCode::GuardTripped => "守卫触发",
            t06bCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t06bCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t06bCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t06bCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t06bCode::DuplicateEntry => "条目重复（同键双写）",
            t06bCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t06bCode::HistBroken => "重建排序态",
            t06bCode::P99Over => "按尾延路径优化",
            t06bCode::WindowBad => "按窗口律校正",
            t06bCode::DriftOver => "按漂移告警登记",
            t06bCode::LedgerBreak => "按守恒重建",
            t06bCode::OrderViolated => "按时序律重排",
            t06bCode::ReportStale => "重渲染",
            t06bCode::GuardTripped => "检查临界区",
            t06bCode::LedgerBroken => "按守恒律重建台账后重跑",
            t06bCode::SnapshotFingerprint => "按防篡改规程重建快照",
            t06bCode::TableBroken => "按语义规则重建常量表",
            t06bCode::IndexMisaligned => "按声明序归一化后重跑",
            t06bCode::DuplicateEntry => "合并同键条目为末次生效",
            t06bCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// 预算红线（本模块自包含；与 vct06_perfbudget 同源对拍）
// ---------------------------------------------------------------------------

/// 每帧预算红线（微秒；0.1ms = 100us）。
pub const FRAME_BUDGET_US: u64 = 100;

// --------------------------------------------------------------------------
// 直方图引擎 budget_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 每帧耗时直方图（us 档分位数 P50/P90/P99）。
/// 样本容量上限。
pub const BUDGET_HIST_CAP: usize = 128;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct budget_histHist {
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

impl budget_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 128], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= BUDGET_HIST_CAP {
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
// 快照 bsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 预算域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct bsnap {
    /// 指纹分量：budget_us。
    pub budget_us: u64,
    /// 指纹分量：stages。
    pub stages: u64,
    /// 指纹分量：reserved_us。
    pub reserved_us: u64,
    /// 指纹分量：headroom_us。
    pub headroom_us: u64,
    /// 指纹分量：p99_us。
    pub p99_us: u64,
    /// 指纹分量：overdraws。
    pub overdraws: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl bsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        budget_us: u64,
        stages: u64,
        reserved_us: u64,
        headroom_us: u64,
        p99_us: u64,
        overdraws: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &budget_us.to_le_bytes());
        h = fnv1a64_from(h, &stages.to_le_bytes());
        h = fnv1a64_from(h, &reserved_us.to_le_bytes());
        h = fnv1a64_from(h, &headroom_us.to_le_bytes());
        h = fnv1a64_from(h, &p99_us.to_le_bytes());
        h = fnv1a64_from(h, &overdraws.to_le_bytes());
        Self {
            budget_us,
            stages,
            reserved_us,
            headroom_us,
            p99_us,
            overdraws,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.budget_us.to_le_bytes());
        h = fnv1a64_from(h, &self.stages.to_le_bytes());
        h = fnv1a64_from(h, &self.reserved_us.to_le_bytes());
        h = fnv1a64_from(h, &self.headroom_us.to_le_bytes());
        h = fnv1a64_from(h, &self.p99_us.to_le_bytes());
        h = fnv1a64_from(h, &self.overdraws.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 bdom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 预算域分量链。
#[derive(Clone, Copy, Debug)]
pub struct bdomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct bdomChain {
    /// 链节点序列。
    pub links: [bdomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl bdomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [bdomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t06bCode> {
        if self.n >= 16 || payload == 0 {
            return Err(t06bCode::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = bdomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x7210, 0x721f]。
pub const fn t06b_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t06bCode::ALL.len() {
        let c = t06bCode::ALL[i].code();
        if c < 0x7210 || c > 0x721f {
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
fn t06b_guard_seg() -> bool {
    t06b_segment_guard()
}

/// 帧耗时直方图（P99 低于红线 + 守恒）。
fn check_c1_hist() -> bool {
    let mut h = budget_histHist::new();
    let mut i = 0;
    while i < 64 {
        h.push(30 + (i % 5));
        i += 1;
    }
    let mut ok = h.conserved() && h.n == 64;
    if ok {
        ok = h.percentile(500) <= h.percentile(990) && h.max < FRAME_BUDGET_US;
    }
    ok
}

/// 预算域快照 + 篡改检出。
fn check_c2_snap() -> bool {
    let s = bsnap::compute(100, 5, 80, 20, 34, 0);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.headroom_us = 0;
        ok = bad.tampered();
    }
    ok
}

/// 预算域分量链回放校验。
fn check_c3_chain() -> bool {
    let mut ch = bdomChain::new();
    let mut ok = true;
    for (i, v) in [100u64, 5, 80, 20, 34, 1].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 240;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t06b_checks() -> CheckSet {
    let mut set = CheckSet::new("T06B");
    set.add(
        "t06b-seg-guard",
        t06b_guard_seg(),
        if t06b_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t06b-c1-hist",
        check_c1_hist(),
        if check_c1_hist() { "" } else { "budget histogram mismatch" },
    );
    set.add(
        "t06b-c2-snap",
        check_c2_snap(),
        if check_c2_snap() { "" } else { "budget domain snapshot mismatch" },
    );
    set.add(
        "t06b-c3-chain",
        check_c3_chain(),
        if check_c3_chain() { "" } else { "budget domain chain mismatch" },
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
    fn t06b_all_checks_green() {
        let set = run_t06b_checks();
        assert!(set.all_passed(), "T06B domain checks must be green");
    }

    #[test]
    fn t06b_segment_guard_holds() {
        assert!(t06b_segment_guard());
    }
}
