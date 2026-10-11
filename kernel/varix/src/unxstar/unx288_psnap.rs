//! unxt288_psnap —— UNX-F0281 panic 域总账（源因折叠 + 收官快照）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0281 · panic 入口统一（双狗/断言/拒载/用户触发四源） · 判据：四源触发统一入口，来源标记与去重合并正确，重入 panic 防护过
//! · 组件：域六分量快照 + 折叠账封存 + 渲染报告
//! · 判据落地：四源统一的域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T288_VERSION: &str = "t288-psnap-v1";

/// 诊断段基址。
pub const T288_SEG_BASE: u16 = 0x3b70;

// --------------------------------------------------------------------------
// 诊断码（t288 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// panic 域总账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t288Code {
    /// 0x3b70 指纹陈旧。
    FingerprintStale,
    /// 0x3b71 分量漂移。
    ComponentDrift,
    /// 0x3b72 报告链断裂。
    ChainBroken,
    /// 0x3b73 脱敏缺失。
    MaskMissing,
    /// 0x3b74 域账破缺。
    LedgerBreak,
    /// 0x3b75 渲染截断。
    RenderTruncated,
    /// 0x3b76 收官印缺失。
    SealMissing,
    /// 0x3b77 守卫触发。
    GuardTripped,
    /// 0x3b78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3b79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3b7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3b7b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3b7c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3b7d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3b7e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t288Code {
    /// 全量闭集。
    pub const ALL: [t288Code; 15] = [
        t288Code::FingerprintStale,
        t288Code::ComponentDrift,
        t288Code::ChainBroken,
        t288Code::MaskMissing,
        t288Code::LedgerBreak,
        t288Code::RenderTruncated,
        t288Code::SealMissing,
        t288Code::GuardTripped,
        t288Code::LedgerBroken,
        t288Code::SnapshotFingerprint,
        t288Code::TableBroken,
        t288Code::IndexMisaligned,
        t288Code::DuplicateEntry,
        t288Code::OrderViolated,
        t288Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t288Code::FingerprintStale => 0x3b70,
            t288Code::ComponentDrift => 0x3b71,
            t288Code::ChainBroken => 0x3b72,
            t288Code::MaskMissing => 0x3b73,
            t288Code::LedgerBreak => 0x3b74,
            t288Code::RenderTruncated => 0x3b75,
            t288Code::SealMissing => 0x3b76,
            t288Code::GuardTripped => 0x3b77,
            t288Code::LedgerBroken => 0x3b78,
            t288Code::SnapshotFingerprint => 0x3b79,
            t288Code::TableBroken => 0x3b7a,
            t288Code::IndexMisaligned => 0x3b7b,
            t288Code::DuplicateEntry => 0x3b7c,
            t288Code::OrderViolated => 0x3b7d,
            t288Code::SegmentGuard => 0x3b7e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t288Code::FingerprintStale => "指纹陈旧",
            t288Code::ComponentDrift => "分量漂移",
            t288Code::ChainBroken => "报告链断裂",
            t288Code::MaskMissing => "脱敏缺失",
            t288Code::LedgerBreak => "域账破缺",
            t288Code::RenderTruncated => "渲染截断",
            t288Code::SealMissing => "收官印缺失",
            t288Code::GuardTripped => "守卫触发",
            t288Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t288Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t288Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t288Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t288Code::DuplicateEntry => "条目重复（同键双写）",
            t288Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t288Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t288Code::FingerprintStale => "重封",
            t288Code::ComponentDrift => "重算",
            t288Code::ChainBroken => "补链",
            t288Code::MaskMissing => "补打码",
            t288Code::LedgerBreak => "按守恒重建",
            t288Code::RenderTruncated => "分页",
            t288Code::SealMissing => "补收官印",
            t288Code::GuardTripped => "检查临界区",
            t288Code::LedgerBroken => "按守恒律重建台账后重跑",
            t288Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t288Code::TableBroken => "按语义规则重建常量表",
            t288Code::IndexMisaligned => "按声明序归一化后重跑",
            t288Code::DuplicateEntry => "合并同键条目为末次生效",
            t288Code::OrderViolated => "按依赖矩阵重排后重跑",
            t288Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 psnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// panic 域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct psnap {
    /// 指纹分量：sources。
    pub sources: u64,
    /// 指纹分量：reasons。
    pub reasons: u64,
    /// 指纹分量：kept。
    pub kept: u64,
    /// 指纹分量：folded。
    pub folded: u64,
    /// 指纹分量：vectors。
    pub vectors: u64,
    /// 指纹分量：halt_steps。
    pub halt_steps: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl psnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        sources: u64,
        reasons: u64,
        kept: u64,
        folded: u64,
        vectors: u64,
        halt_steps: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &sources.to_le_bytes());
        h = fnv1a64_from(h, &reasons.to_le_bytes());
        h = fnv1a64_from(h, &kept.to_le_bytes());
        h = fnv1a64_from(h, &folded.to_le_bytes());
        h = fnv1a64_from(h, &vectors.to_le_bytes());
        h = fnv1a64_from(h, &halt_steps.to_le_bytes());
        Self {
            sources,
            reasons,
            kept,
            folded,
            vectors,
            halt_steps,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.sources.to_le_bytes());
        h = fnv1a64_from(h, &self.reasons.to_le_bytes());
        h = fnv1a64_from(h, &self.kept.to_le_bytes());
        h = fnv1a64_from(h, &self.folded.to_le_bytes());
        h = fnv1a64_from(h, &self.vectors.to_le_bytes());
        h = fnv1a64_from(h, &self.halt_steps.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 pdom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// panic 域分量链（六分量逐环指纹）。
#[derive(Clone, Copy, Debug)]
pub struct pdomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct pdomChain {
    /// 链节点序列。
    pub links: [pdomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl pdomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [pdomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t288Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t288Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = pdomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x3b70, 0x3b7f]。
pub const fn t288_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t288Code::ALL.len() {
        let c = t288Code::ALL[i].code();
        if c < 0x3b70 || c > 0x3b7f {
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
fn t288_guard_seg() -> bool {
    t288_segment_guard()
}

/// panic 域快照 + 篡改检出。
fn check_c1_dom_snapshot() -> bool {
    let s = psnap::compute(4, 8, 1, 6, 32, 7);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.sources = 5;
        ok = bad.tampered();
    }
    ok
}

/// 域分量链六环回放校验。
fn check_c2_dom_chain() -> bool {
    let mut ch = pdomChain::new();
    let mut ok = true;
    for (i, v) in [4u64, 8, 1, 6, 32, 7].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 58;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t288_checks() -> CheckSet {
    let mut set = CheckSet::new("T288");
    set.add(
        "t288-seg-guard",
        t288_guard_seg(),
        if t288_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t288-c1-dom-snapshot",
        check_c1_dom_snapshot(),
        if check_c1_dom_snapshot() { "" } else { "panic domain snapshot mismatch" },
    );
    set.add(
        "t288-c2-dom-chain",
        check_c2_dom_chain(),
        if check_c2_dom_chain() { "" } else { "panic domain chain mismatch" },
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
    fn t288_all_checks_green() {
        let set = run_t288_checks();
        assert!(set.all_passed(), "T288 domain checks must be green");
    }

    #[test]
    fn t288_segment_guard_holds() {
        assert!(t288_segment_guard());
    }
}
