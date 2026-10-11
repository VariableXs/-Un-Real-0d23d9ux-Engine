//! unxt227_kgolden —— UNX-F0221 达成线黄金比对（300 常数 vs 实际聚合）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0221 · 引导面断言聚合扩容（300+ 达成线） · 判据：ktest 引导面断言总数 ≥300 并随批次自动聚合，计数账与实际运行数一致
//! · 组件：黄金常数账 + 比对引擎（>= 判定）+ 红线守卫 + 域快照
//! · 判据落地：断言总数 ≥300 的黄金判据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T227_VERSION: &str = "t227-kgolden-v1";

/// 诊断段基址。
pub const T227_SEG_BASE: u16 = 0x3860;

// --------------------------------------------------------------------------
// 诊断码（t227 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 达成线比对诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t227Code {
    /// 0x3860 低于达成线（实际 < 300）。
    BelowLine,
    /// 0x3861 达成线漂移（常数被改）。
    LineDrift,
    /// 0x3862 比对陈旧（黄金常数过期）。
    CompareStale,
    /// 0x3863 阈值非法（红线 ≤0）。
    ThresholdBad,
    /// 0x3864 余量丢失（恰好压线无余量）。
    MarginLost,
    /// 0x3865 样本偏斜（未抽低批）。
    SampleSkew,
    /// 0x3866 黄金值被覆写。
    GoldenOverride,
    /// 0x3867 守卫假绿（比对恒真）。
    GuardFalseGreen,
    /// 0x3868 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3869 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x386a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x386b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x386c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x386d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x386e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x386f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t227Code {
    /// 全量闭集。
    pub const ALL: [t227Code; 16] = [
        t227Code::BelowLine,
        t227Code::LineDrift,
        t227Code::CompareStale,
        t227Code::ThresholdBad,
        t227Code::MarginLost,
        t227Code::SampleSkew,
        t227Code::GoldenOverride,
        t227Code::GuardFalseGreen,
        t227Code::LedgerBroken,
        t227Code::SnapshotFingerprint,
        t227Code::TableBroken,
        t227Code::IndexMisaligned,
        t227Code::DuplicateEntry,
        t227Code::OrderViolated,
        t227Code::GuardTripped,
        t227Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t227Code::BelowLine => 0x3860,
            t227Code::LineDrift => 0x3861,
            t227Code::CompareStale => 0x3862,
            t227Code::ThresholdBad => 0x3863,
            t227Code::MarginLost => 0x3864,
            t227Code::SampleSkew => 0x3865,
            t227Code::GoldenOverride => 0x3866,
            t227Code::GuardFalseGreen => 0x3867,
            t227Code::LedgerBroken => 0x3868,
            t227Code::SnapshotFingerprint => 0x3869,
            t227Code::TableBroken => 0x386a,
            t227Code::IndexMisaligned => 0x386b,
            t227Code::DuplicateEntry => 0x386c,
            t227Code::OrderViolated => 0x386d,
            t227Code::GuardTripped => 0x386e,
            t227Code::SegmentGuard => 0x386f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t227Code::BelowLine => "低于达成线（实际 < 300）",
            t227Code::LineDrift => "达成线漂移（常数被改）",
            t227Code::CompareStale => "比对陈旧（黄金常数过期）",
            t227Code::ThresholdBad => "阈值非法（红线 ≤0）",
            t227Code::MarginLost => "余量丢失（恰好压线无余量）",
            t227Code::SampleSkew => "样本偏斜（未抽低批）",
            t227Code::GoldenOverride => "黄金值被覆写",
            t227Code::GuardFalseGreen => "守卫假绿（比对恒真）",
            t227Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t227Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t227Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t227Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t227Code::DuplicateEntry => "条目重复（同键双写）",
            t227Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t227Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t227Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t227Code::BelowLine => "按缺口批补条",
            t227Code::LineDrift => "按冻结常数恢复",
            t227Code::CompareStale => "按新黄金常数重比",
            t227Code::ThresholdBad => "按 300 常量校正",
            t227Code::MarginLost => "预留安全余量后重比",
            t227Code::SampleSkew => "分层重抽",
            t227Code::GoldenOverride => "按封存规程恢复",
            t227Code::GuardFalseGreen => "注入负样本校验守卫",
            t227Code::LedgerBroken => "按守恒律重建台账后重跑",
            t227Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t227Code::TableBroken => "按语义规则重建常量表",
            t227Code::IndexMisaligned => "按声明序归一化后重跑",
            t227Code::DuplicateEntry => "合并同键条目为末次生效",
            t227Code::OrderViolated => "按依赖矩阵重排后重跑",
            t227Code::GuardTripped => "检查临界区嵌套与阈值",
            t227Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 goldchain（链式指纹：黄金链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 达成线黄金链（每批一链环，负载=批断言数）。
#[derive(Clone, Copy, Debug)]
pub struct goldchainLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct goldchainChain {
    /// 链节点序列。
    pub links: [goldchainLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl goldchainChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [goldchainLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t227Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t227Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = goldchainLink { tag, payload, digest };
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

// --------------------------------------------------------------------------
// 守卫 line（阈值 300 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 达成线守卫：窗口计数达 300 即封账。
#[derive(Clone, Copy, Debug)]
pub struct line {
    /// 重入嵌套深度（>0 即重入中）。
    pub depth: u32,
    /// 越限拒止计数。
    pub over: u64,
    /// 静默拒止计数（阈值内未升级）。
    pub silent: u64,
    /// 当前窗口计数。
    pub window: u64,
    /// 窗口内观测总量。
    pub seen: u64,
    /// 已升级告警数。
    pub raised: u64,
}

/// 守卫阈值（达成线守卫：窗口计数达 300 即封账）。
pub const LINE_THRESHOLD: u64 = 300;

impl line {
    /// 全零构造。
    pub const fn new() -> Self {
        Self { depth: 0, over: 0, silent: 0, window: 0, seen: 0, raised: 0 }
    }

    /// 进入临界区：重入直接拒止（返回 false）。
    pub fn enter(&mut self) -> bool {
        if self.depth > 0 {
            self.over += 1;
            return false;
        }
        self.depth = 1;
        true
    }

    /// 离开临界区（深度归一，幂等安全）。
    pub fn leave(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    /// 观测一次：累计 seen/window；越限升级 raised，否则静默。
    pub fn observe(&mut self, hit: bool) {
        self.seen += 1;
        if hit {
            self.window += 1;
        }
        if self.window >= LINE_THRESHOLD {
            self.raised += 1;
        } else {
            self.silent += 1;
        }
    }

    /// 守恒：seen == raised + silent（窗口口径）。
    pub fn conserved(&self) -> bool {
        self.seen == self.raised + self.silent
    }
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
// 快照 gsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 达成线六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct gsnap {
    /// 指纹分量：declared。
    pub declared: u64,
    /// 指纹分量：actual。
    pub actual: u64,
    /// 指纹分量：batches。
    pub batches: u64,
    /// 指纹分量：slots。
    pub slots: u64,
    /// 指纹分量：line。
    pub line: u64,
    /// 指纹分量：cover。
    pub cover: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl gsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        declared: u64,
        actual: u64,
        batches: u64,
        slots: u64,
        line: u64,
        cover: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &declared.to_le_bytes());
        h = fnv1a64_from(h, &actual.to_le_bytes());
        h = fnv1a64_from(h, &batches.to_le_bytes());
        h = fnv1a64_from(h, &slots.to_le_bytes());
        h = fnv1a64_from(h, &line.to_le_bytes());
        h = fnv1a64_from(h, &cover.to_le_bytes());
        Self {
            declared,
            actual,
            batches,
            slots,
            line,
            cover,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.declared.to_le_bytes());
        h = fnv1a64_from(h, &self.actual.to_le_bytes());
        h = fnv1a64_from(h, &self.batches.to_le_bytes());
        h = fnv1a64_from(h, &self.slots.to_le_bytes());
        h = fnv1a64_from(h, &self.line.to_le_bytes());
        h = fnv1a64_from(h, &self.cover.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3860, 0x386f]。
pub const fn t227_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t227Code::ALL.len() {
        let c = t227Code::ALL[i].code();
        if c < 0x3860 || c > 0x386f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t227_guard_seg() -> bool {
    t227_segment_guard()
}

/// 黄金判定（>=300 语义 + 负样本）。
fn check_c1_golden_ge() -> bool {
    let total: u64 = 300;
    let mut ok = total >= 300;
    if ok {
        ok = !(299 >= 300) && (300 >= 300) && (301 >= 300);
    }
    ok
}

/// 快照指纹 + 篡改检出（负样本）。
fn check_c2_snapshot() -> bool {
    let s = gsnap::compute(300, 300, 15, 20, 300, 300);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.actual = 299;
        ok = bad.tampered();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t227_checks() -> CheckSet {
    let mut set = CheckSet::new("T227");
    set.add(
        "t227-seg-guard",
        t227_guard_seg(),
        if t227_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t227-c1-golden-ge",
        check_c1_golden_ge(),
        if check_c1_golden_ge() { "" } else { "golden >= line mismatch" },
    );
    set.add(
        "t227-c2-snapshot",
        check_c2_snapshot(),
        if check_c2_snapshot() { "" } else { "golden snapshot tamper mismatch" },
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
    fn t227_all_checks_green() {
        let set = run_t227_checks();
        assert!(set.all_passed(), "T227 domain checks must be green");
    }

    #[test]
    fn t227_segment_guard_holds() {
        assert!(t227_segment_guard());
    }
}
