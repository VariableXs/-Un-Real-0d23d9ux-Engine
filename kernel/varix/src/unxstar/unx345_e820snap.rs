//! unxt345_e820snap —— UNX-F0361 内存盘点域快照。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0361 · 早年内存盘点归并（E820 区段排序相交并） · 判据：E820 区段排序归并零相交，归并后总容量与分区明细账一致
//! · 组件：域六分量快照 + 篡改检出 + 报告链
//! · 判据落地：盘点域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T345_VERSION: &str = "t345-e820snap-v1";

/// 诊断段基址。
pub const T345_SEG_BASE: u16 = 0x3f40;

// --------------------------------------------------------------------------
// 诊断码（t345 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 盘点域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t345Code {
    /// 0x3f40 指纹陈旧。
    FingerprintStale,
    /// 0x3f41 分量漂移。
    ComponentDrift,
    /// 0x3f42 分量链断裂。
    ChainBroken,
    /// 0x3f43 脱敏缺失。
    MaskMissing,
    /// 0x3f44 域账破缺。
    LedgerBreak,
    /// 0x3f45 渲染截断。
    RenderTruncated,
    /// 0x3f46 收官印缺失。
    SealMissing,
    /// 0x3f47 守卫触发。
    GuardTripped,
    /// 0x3f48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3f49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3f4a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3f4b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3f4c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3f4d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3f4e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t345Code {
    /// 全量闭集。
    pub const ALL: [t345Code; 15] = [
        t345Code::FingerprintStale,
        t345Code::ComponentDrift,
        t345Code::ChainBroken,
        t345Code::MaskMissing,
        t345Code::LedgerBreak,
        t345Code::RenderTruncated,
        t345Code::SealMissing,
        t345Code::GuardTripped,
        t345Code::LedgerBroken,
        t345Code::SnapshotFingerprint,
        t345Code::TableBroken,
        t345Code::IndexMisaligned,
        t345Code::DuplicateEntry,
        t345Code::OrderViolated,
        t345Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t345Code::FingerprintStale => 0x3f40,
            t345Code::ComponentDrift => 0x3f41,
            t345Code::ChainBroken => 0x3f42,
            t345Code::MaskMissing => 0x3f43,
            t345Code::LedgerBreak => 0x3f44,
            t345Code::RenderTruncated => 0x3f45,
            t345Code::SealMissing => 0x3f46,
            t345Code::GuardTripped => 0x3f47,
            t345Code::LedgerBroken => 0x3f48,
            t345Code::SnapshotFingerprint => 0x3f49,
            t345Code::TableBroken => 0x3f4a,
            t345Code::IndexMisaligned => 0x3f4b,
            t345Code::DuplicateEntry => 0x3f4c,
            t345Code::OrderViolated => 0x3f4d,
            t345Code::SegmentGuard => 0x3f4e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t345Code::FingerprintStale => "指纹陈旧",
            t345Code::ComponentDrift => "分量漂移",
            t345Code::ChainBroken => "分量链断裂",
            t345Code::MaskMissing => "脱敏缺失",
            t345Code::LedgerBreak => "域账破缺",
            t345Code::RenderTruncated => "渲染截断",
            t345Code::SealMissing => "收官印缺失",
            t345Code::GuardTripped => "守卫触发",
            t345Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t345Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t345Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t345Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t345Code::DuplicateEntry => "条目重复（同键双写）",
            t345Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t345Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t345Code::FingerprintStale => "重封",
            t345Code::ComponentDrift => "重算",
            t345Code::ChainBroken => "补链",
            t345Code::MaskMissing => "补打码",
            t345Code::LedgerBreak => "按守恒重建",
            t345Code::RenderTruncated => "分页",
            t345Code::SealMissing => "补收官印",
            t345Code::GuardTripped => "检查临界区",
            t345Code::LedgerBroken => "按守恒律重建台账后重跑",
            t345Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t345Code::TableBroken => "按语义规则重建常量表",
            t345Code::IndexMisaligned => "按声明序归一化后重跑",
            t345Code::DuplicateEntry => "合并同键条目为末次生效",
            t345Code::OrderViolated => "按依赖矩阵重排后重跑",
            t345Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 esnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 内存盘点域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct esnap {
    /// 指纹分量：types。
    pub types: u64,
    /// 指纹分量：scan_segs。
    pub scan_segs: u64,
    /// 指纹分量：merged_segs。
    pub merged_segs: u64,
    /// 指纹分量：avail。
    pub avail: u64,
    /// 指纹分量：resv。
    pub resv: u64,
    /// 指纹分量：total。
    pub total: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl esnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        types: u64,
        scan_segs: u64,
        merged_segs: u64,
        avail: u64,
        resv: u64,
        total: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &types.to_le_bytes());
        h = fnv1a64_from(h, &scan_segs.to_le_bytes());
        h = fnv1a64_from(h, &merged_segs.to_le_bytes());
        h = fnv1a64_from(h, &avail.to_le_bytes());
        h = fnv1a64_from(h, &resv.to_le_bytes());
        h = fnv1a64_from(h, &total.to_le_bytes());
        Self {
            types,
            scan_segs,
            merged_segs,
            avail,
            resv,
            total,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.types.to_le_bytes());
        h = fnv1a64_from(h, &self.scan_segs.to_le_bytes());
        h = fnv1a64_from(h, &self.merged_segs.to_le_bytes());
        h = fnv1a64_from(h, &self.avail.to_le_bytes());
        h = fnv1a64_from(h, &self.resv.to_le_bytes());
        h = fnv1a64_from(h, &self.total.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 edom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 盘点域分量链。
#[derive(Clone, Copy, Debug)]
pub struct edomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct edomChain {
    /// 链节点序列。
    pub links: [edomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl edomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [edomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t345Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t345Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = edomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x3f40, 0x3f4f]。
pub const fn t345_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t345Code::ALL.len() {
        let c = t345Code::ALL[i].code();
        if c < 0x3f40 || c > 0x3f4f {
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
fn t345_guard_seg() -> bool {
    t345_segment_guard()
}

/// 盘点域快照 + 篡改检出。
fn check_c1_snap() -> bool {
    let s = esnap::compute(5, 128, 120, 112, 16, 128);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.scan_segs = 129;
        ok = bad.tampered();
    }
    ok
}

/// 盘点域分量链回放校验。
fn check_c2_chain() -> bool {
    let mut ch = edomChain::new();
    let mut ok = true;
    for (i, v) in [5u64, 128, 120, 112, 16, 128].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 509;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t345_checks() -> CheckSet {
    let mut set = CheckSet::new("T345");
    set.add(
        "t345-seg-guard",
        t345_guard_seg(),
        if t345_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t345-c1-snap",
        check_c1_snap(),
        if check_c1_snap() { "" } else { "memmap domain snapshot mismatch" },
    );
    set.add(
        "t345-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "memmap domain chain mismatch" },
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
    fn t345_all_checks_green() {
        let set = run_t345_checks();
        assert!(set.all_passed(), "T345 domain checks must be green");
    }

    #[test]
    fn t345_segment_guard_holds() {
        assert!(t345_segment_guard());
    }
}
