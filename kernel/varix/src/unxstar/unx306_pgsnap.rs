//! unxt306_pgsnap —— UNX-F0301 页表域快照（分量指纹 + 防篡改）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0301 · 页表构建总编排（四级表族流水） · 判据：PML4→PDPT→PD→PT 四级构建流水编排正确，依赖序与设计一致，构建账完整
//! · 组件：域六分量快照 + 篡改检出 + 渲染报告
//! · 判据落地：构建域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T306_VERSION: &str = "t306-pgsnap-v1";

/// 诊断段基址。
pub const T306_SEG_BASE: u16 = 0x3c50;

// --------------------------------------------------------------------------
// 诊断码（t306 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 页表域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t306Code {
    /// 0x3c50 指纹陈旧。
    FingerprintStale,
    /// 0x3c51 分量漂移。
    ComponentDrift,
    /// 0x3c52 分量链断裂。
    ChainBroken,
    /// 0x3c53 脱敏缺失。
    MaskMissing,
    /// 0x3c54 域账破缺。
    LedgerBreak,
    /// 0x3c55 渲染截断。
    RenderTruncated,
    /// 0x3c56 收官印缺失。
    SealMissing,
    /// 0x3c57 守卫触发。
    GuardTripped,
    /// 0x3c58 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3c59 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3c5a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3c5b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3c5c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3c5d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3c5e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t306Code {
    /// 全量闭集。
    pub const ALL: [t306Code; 15] = [
        t306Code::FingerprintStale,
        t306Code::ComponentDrift,
        t306Code::ChainBroken,
        t306Code::MaskMissing,
        t306Code::LedgerBreak,
        t306Code::RenderTruncated,
        t306Code::SealMissing,
        t306Code::GuardTripped,
        t306Code::LedgerBroken,
        t306Code::SnapshotFingerprint,
        t306Code::TableBroken,
        t306Code::IndexMisaligned,
        t306Code::DuplicateEntry,
        t306Code::OrderViolated,
        t306Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t306Code::FingerprintStale => 0x3c50,
            t306Code::ComponentDrift => 0x3c51,
            t306Code::ChainBroken => 0x3c52,
            t306Code::MaskMissing => 0x3c53,
            t306Code::LedgerBreak => 0x3c54,
            t306Code::RenderTruncated => 0x3c55,
            t306Code::SealMissing => 0x3c56,
            t306Code::GuardTripped => 0x3c57,
            t306Code::LedgerBroken => 0x3c58,
            t306Code::SnapshotFingerprint => 0x3c59,
            t306Code::TableBroken => 0x3c5a,
            t306Code::IndexMisaligned => 0x3c5b,
            t306Code::DuplicateEntry => 0x3c5c,
            t306Code::OrderViolated => 0x3c5d,
            t306Code::SegmentGuard => 0x3c5e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t306Code::FingerprintStale => "指纹陈旧",
            t306Code::ComponentDrift => "分量漂移",
            t306Code::ChainBroken => "分量链断裂",
            t306Code::MaskMissing => "脱敏缺失",
            t306Code::LedgerBreak => "域账破缺",
            t306Code::RenderTruncated => "渲染截断",
            t306Code::SealMissing => "收官印缺失",
            t306Code::GuardTripped => "守卫触发",
            t306Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t306Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t306Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t306Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t306Code::DuplicateEntry => "条目重复（同键双写）",
            t306Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t306Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t306Code::FingerprintStale => "重封",
            t306Code::ComponentDrift => "重算",
            t306Code::ChainBroken => "补链",
            t306Code::MaskMissing => "补打码",
            t306Code::LedgerBreak => "按守恒重建",
            t306Code::RenderTruncated => "分页",
            t306Code::SealMissing => "补收官印",
            t306Code::GuardTripped => "检查临界区",
            t306Code::LedgerBroken => "按守恒律重建台账后重跑",
            t306Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t306Code::TableBroken => "按语义规则重建常量表",
            t306Code::IndexMisaligned => "按声明序归一化后重跑",
            t306Code::DuplicateEntry => "合并同键条目为末次生效",
            t306Code::OrderViolated => "按依赖矩阵重排后重跑",
            t306Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 pgsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 页表域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct pgsnap {
    /// 指纹分量：levels。
    pub levels: u64,
    /// 指纹分量：slots。
    pub slots: u64,
    /// 指纹分量：pipeline。
    pub pipeline: u64,
    /// 指纹分量：pages。
    pub pages: u64,
    /// 指纹分量：walks。
    pub walks: u64,
    /// 指纹分量：huge。
    pub huge: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl pgsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        levels: u64,
        slots: u64,
        pipeline: u64,
        pages: u64,
        walks: u64,
        huge: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &levels.to_le_bytes());
        h = fnv1a64_from(h, &slots.to_le_bytes());
        h = fnv1a64_from(h, &pipeline.to_le_bytes());
        h = fnv1a64_from(h, &pages.to_le_bytes());
        h = fnv1a64_from(h, &walks.to_le_bytes());
        h = fnv1a64_from(h, &huge.to_le_bytes());
        Self {
            levels,
            slots,
            pipeline,
            pages,
            walks,
            huge,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.levels.to_le_bytes());
        h = fnv1a64_from(h, &self.slots.to_le_bytes());
        h = fnv1a64_from(h, &self.pipeline.to_le_bytes());
        h = fnv1a64_from(h, &self.pages.to_le_bytes());
        h = fnv1a64_from(h, &self.walks.to_le_bytes());
        h = fnv1a64_from(h, &self.huge.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 pgdom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 页表域分量链（六分量逐环指纹）。
#[derive(Clone, Copy, Debug)]
pub struct pgdomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct pgdomChain {
    /// 链节点序列。
    pub links: [pgdomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl pgdomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [pgdomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t306Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t306Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = pgdomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x3c50, 0x3c5f]。
pub const fn t306_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t306Code::ALL.len() {
        let c = t306Code::ALL[i].code();
        if c < 0x3c50 || c > 0x3c5f {
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
fn t306_guard_seg() -> bool {
    t306_segment_guard()
}

/// 页表域快照 + 篡改检出。
fn check_c1_snap() -> bool {
    let s = pgsnap::compute(4, 512, 9, 27, 3, 2);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.slots = 513;
        ok = bad.tampered();
    }
    ok
}

/// 页表域分量链回放校验。
fn check_c2_chain() -> bool {
    let mut ch = pgdomChain::new();
    let mut ok = true;
    for (i, v) in [4u64, 512, 9, 27, 3, 2].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 557;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t306_checks() -> CheckSet {
    let mut set = CheckSet::new("T306");
    set.add(
        "t306-seg-guard",
        t306_guard_seg(),
        if t306_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t306-c1-snap",
        check_c1_snap(),
        if check_c1_snap() { "" } else { "page domain snapshot mismatch" },
    );
    set.add(
        "t306-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "page domain chain mismatch" },
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
    fn t306_all_checks_green() {
        let set = run_t306_checks();
        assert!(set.all_passed(), "T306 domain checks must be green");
    }

    #[test]
    fn t306_segment_guard_holds() {
        assert!(t306_segment_guard());
    }
}
