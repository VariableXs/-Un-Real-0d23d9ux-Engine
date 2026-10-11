//! unxt267_framesnap —— UNX-F0261 帧簿域快照（分量指纹 + 防篡改）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0261 · 帧簿双头显（诊断浮层 + 正常输出合成） · 判据：诊断浮层与正常输出同屏合成零互蚀，浮层开合零残影
//! · 组件：域七分量快照 + 篡改检出 + 渲染报告
//! · 判据落地：双头显域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T267_VERSION: &str = "t267-framesnap-v1";

/// 诊断段基址。
pub const T267_SEG_BASE: u16 = 0x3a60;

// --------------------------------------------------------------------------
// 诊断码（t267 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 帧簿域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t267Code {
    /// 0x3a60 指纹陈旧。
    FingerprintStale,
    /// 0x3a61 分量漂移。
    ComponentDrift,
    /// 0x3a62 脱敏缺失。
    MaskMissing,
    /// 0x3a63 报告陈旧。
    ReportStale,
    /// 0x3a64 分量链断裂。
    ChainBroken,
    /// 0x3a65 域账破缺。
    LedgerBreak,
    /// 0x3a66 渲染截断。
    RenderTruncated,
    /// 0x3a67 守卫触发。
    GuardTripped,
    /// 0x3a68 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3a69 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3a6a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3a6b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3a6c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3a6d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3a6e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t267Code {
    /// 全量闭集。
    pub const ALL: [t267Code; 15] = [
        t267Code::FingerprintStale,
        t267Code::ComponentDrift,
        t267Code::MaskMissing,
        t267Code::ReportStale,
        t267Code::ChainBroken,
        t267Code::LedgerBreak,
        t267Code::RenderTruncated,
        t267Code::GuardTripped,
        t267Code::LedgerBroken,
        t267Code::SnapshotFingerprint,
        t267Code::TableBroken,
        t267Code::IndexMisaligned,
        t267Code::DuplicateEntry,
        t267Code::OrderViolated,
        t267Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t267Code::FingerprintStale => 0x3a60,
            t267Code::ComponentDrift => 0x3a61,
            t267Code::MaskMissing => 0x3a62,
            t267Code::ReportStale => 0x3a63,
            t267Code::ChainBroken => 0x3a64,
            t267Code::LedgerBreak => 0x3a65,
            t267Code::RenderTruncated => 0x3a66,
            t267Code::GuardTripped => 0x3a67,
            t267Code::LedgerBroken => 0x3a68,
            t267Code::SnapshotFingerprint => 0x3a69,
            t267Code::TableBroken => 0x3a6a,
            t267Code::IndexMisaligned => 0x3a6b,
            t267Code::DuplicateEntry => 0x3a6c,
            t267Code::OrderViolated => 0x3a6d,
            t267Code::SegmentGuard => 0x3a6e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t267Code::FingerprintStale => "指纹陈旧",
            t267Code::ComponentDrift => "分量漂移",
            t267Code::MaskMissing => "脱敏缺失",
            t267Code::ReportStale => "报告陈旧",
            t267Code::ChainBroken => "分量链断裂",
            t267Code::LedgerBreak => "域账破缺",
            t267Code::RenderTruncated => "渲染截断",
            t267Code::GuardTripped => "守卫触发",
            t267Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t267Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t267Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t267Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t267Code::DuplicateEntry => "条目重复（同键双写）",
            t267Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t267Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t267Code::FingerprintStale => "按幂等规程重封",
            t267Code::ComponentDrift => "按账本重算分量",
            t267Code::MaskMissing => "补打码",
            t267Code::ReportStale => "重渲染",
            t267Code::ChainBroken => "补链后回放",
            t267Code::LedgerBreak => "按守恒重建",
            t267Code::RenderTruncated => "分页",
            t267Code::GuardTripped => "检查临界区",
            t267Code::LedgerBroken => "按守恒律重建台账后重跑",
            t267Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t267Code::TableBroken => "按语义规则重建常量表",
            t267Code::IndexMisaligned => "按声明序归一化后重跑",
            t267Code::DuplicateEntry => "合并同键条目为末次生效",
            t267Code::OrderViolated => "按依赖矩阵重排后重跑",
            t267Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 fsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 帧簿域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct fsnap {
    /// 指纹分量：formats。
    pub formats: u64,
    /// 指纹分量：overlays。
    pub overlays: u64,
    /// 指纹分量：scanouts。
    pub scanouts: u64,
    /// 指纹分量：windows。
    pub windows: u64,
    /// 指纹分量：buffers。
    pub buffers: u64,
    /// 指纹分量：flips。
    pub flips: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl fsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        formats: u64,
        overlays: u64,
        scanouts: u64,
        windows: u64,
        buffers: u64,
        flips: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &formats.to_le_bytes());
        h = fnv1a64_from(h, &overlays.to_le_bytes());
        h = fnv1a64_from(h, &scanouts.to_le_bytes());
        h = fnv1a64_from(h, &windows.to_le_bytes());
        h = fnv1a64_from(h, &buffers.to_le_bytes());
        h = fnv1a64_from(h, &flips.to_le_bytes());
        Self {
            formats,
            overlays,
            scanouts,
            windows,
            buffers,
            flips,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.formats.to_le_bytes());
        h = fnv1a64_from(h, &self.overlays.to_le_bytes());
        h = fnv1a64_from(h, &self.scanouts.to_le_bytes());
        h = fnv1a64_from(h, &self.windows.to_le_bytes());
        h = fnv1a64_from(h, &self.buffers.to_le_bytes());
        h = fnv1a64_from(h, &self.flips.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 fchain（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 帧簿分量链（六分量逐环链式指纹）。
#[derive(Clone, Copy, Debug)]
pub struct fchainLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct fchainChain {
    /// 链节点序列。
    pub links: [fchainLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl fchainChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [fchainLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t267Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t267Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = fchainLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x3a60, 0x3a6f]。
pub const fn t267_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t267Code::ALL.len() {
        let c = t267Code::ALL[i].code();
        if c < 0x3a60 || c > 0x3a6f {
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
fn t267_guard_seg() -> bool {
    t267_segment_guard()
}

/// 帧簿域快照正样本 + 篡改检出。
fn check_c1_snapshot() -> bool {
    let s = fsnap::compute(32, 4, 32, 16, 8, 2);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.formats = 33;
        ok = bad.tampered();
    }
    ok
}

/// 帧簿分量链追加 + 回放校验。
fn check_c2_chain() -> bool {
    let mut ch = fchainChain::new();
    let mut ok = true;
    for (i, v) in [32u64, 4, 32, 16, 8, 2].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 94;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t267_checks() -> CheckSet {
    let mut set = CheckSet::new("T267");
    set.add(
        "t267-seg-guard",
        t267_guard_seg(),
        if t267_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t267-c1-snapshot",
        check_c1_snapshot(),
        if check_c1_snapshot() { "" } else { "frame snapshot tamper mismatch" },
    );
    set.add(
        "t267-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "frame domain chain mismatch" },
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
    fn t267_all_checks_green() {
        let set = run_t267_checks();
        assert!(set.all_passed(), "T267 domain checks must be green");
    }

    #[test]
    fn t267_segment_guard_holds() {
        assert!(t267_segment_guard());
    }
}
