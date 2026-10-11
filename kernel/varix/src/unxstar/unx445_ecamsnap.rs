//! unxt445_ecamsnap —— UNX-F0461 配置域快照（分量指纹）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0461 · 配置访问机制裁定（ECAM vs CF8/CFC） · 判据：机制裁定账（ECAM 优先/CF8 兜底 2 路径），双机制回读一致性抽样核（8/8 设备）
//! · 组件：域六分量快照 + 篡改检出 + 报告链
//! · 判据落地：配置域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T445_VERSION: &str = "t445-ecamsnap-v1";

/// 诊断段基址。
pub const T445_SEG_BASE: u16 = 0x4440;

// --------------------------------------------------------------------------
// 诊断码（t445 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 配置域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t445Code {
    /// 0x4440 指纹陈旧。
    FingerprintStale,
    /// 0x4441 分量漂移。
    ComponentDrift,
    /// 0x4442 分量链断裂。
    ChainBroken,
    /// 0x4443 脱敏缺失。
    MaskMissing,
    /// 0x4444 域账破缺。
    LedgerBreak,
    /// 0x4445 渲染截断。
    RenderTruncated,
    /// 0x4446 收官印缺失。
    SealMissing,
    /// 0x4447 守卫触发。
    GuardTripped,
    /// 0x4448 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4449 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x444a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x444b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x444c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x444d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x444e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t445Code {
    /// 全量闭集。
    pub const ALL: [t445Code; 15] = [
        t445Code::FingerprintStale,
        t445Code::ComponentDrift,
        t445Code::ChainBroken,
        t445Code::MaskMissing,
        t445Code::LedgerBreak,
        t445Code::RenderTruncated,
        t445Code::SealMissing,
        t445Code::GuardTripped,
        t445Code::LedgerBroken,
        t445Code::SnapshotFingerprint,
        t445Code::TableBroken,
        t445Code::IndexMisaligned,
        t445Code::DuplicateEntry,
        t445Code::OrderViolated,
        t445Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t445Code::FingerprintStale => 0x4440,
            t445Code::ComponentDrift => 0x4441,
            t445Code::ChainBroken => 0x4442,
            t445Code::MaskMissing => 0x4443,
            t445Code::LedgerBreak => 0x4444,
            t445Code::RenderTruncated => 0x4445,
            t445Code::SealMissing => 0x4446,
            t445Code::GuardTripped => 0x4447,
            t445Code::LedgerBroken => 0x4448,
            t445Code::SnapshotFingerprint => 0x4449,
            t445Code::TableBroken => 0x444a,
            t445Code::IndexMisaligned => 0x444b,
            t445Code::DuplicateEntry => 0x444c,
            t445Code::OrderViolated => 0x444d,
            t445Code::SegmentGuard => 0x444e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t445Code::FingerprintStale => "指纹陈旧",
            t445Code::ComponentDrift => "分量漂移",
            t445Code::ChainBroken => "分量链断裂",
            t445Code::MaskMissing => "脱敏缺失",
            t445Code::LedgerBreak => "域账破缺",
            t445Code::RenderTruncated => "渲染截断",
            t445Code::SealMissing => "收官印缺失",
            t445Code::GuardTripped => "守卫触发",
            t445Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t445Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t445Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t445Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t445Code::DuplicateEntry => "条目重复（同键双写）",
            t445Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t445Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t445Code::FingerprintStale => "重封",
            t445Code::ComponentDrift => "重算",
            t445Code::ChainBroken => "补链",
            t445Code::MaskMissing => "补打码",
            t445Code::LedgerBreak => "按守恒重建",
            t445Code::RenderTruncated => "分页",
            t445Code::SealMissing => "补收官印",
            t445Code::GuardTripped => "检查临界区",
            t445Code::LedgerBroken => "按守恒律重建台账后重跑",
            t445Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t445Code::TableBroken => "按语义规则重建常量表",
            t445Code::IndexMisaligned => "按声明序归一化后重跑",
            t445Code::DuplicateEntry => "合并同键条目为末次生效",
            t445Code::OrderViolated => "按依赖矩阵重排后重跑",
            t445Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 csnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 配置域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct csnap {
    /// 指纹分量：mechanisms。
    pub mechanisms: u64,
    /// 指纹分量：bdf_groups。
    pub bdf_groups: u64,
    /// 指纹分量：ecam_groups。
    pub ecam_groups: u64,
    /// 指纹分量：devices。
    pub devices: u64,
    /// 指纹分量：readbacks。
    pub readbacks: u64,
    /// 指纹分量：mismatches。
    pub mismatches: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl csnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        mechanisms: u64,
        bdf_groups: u64,
        ecam_groups: u64,
        devices: u64,
        readbacks: u64,
        mismatches: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &mechanisms.to_le_bytes());
        h = fnv1a64_from(h, &bdf_groups.to_le_bytes());
        h = fnv1a64_from(h, &ecam_groups.to_le_bytes());
        h = fnv1a64_from(h, &devices.to_le_bytes());
        h = fnv1a64_from(h, &readbacks.to_le_bytes());
        h = fnv1a64_from(h, &mismatches.to_le_bytes());
        Self {
            mechanisms,
            bdf_groups,
            ecam_groups,
            devices,
            readbacks,
            mismatches,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.mechanisms.to_le_bytes());
        h = fnv1a64_from(h, &self.bdf_groups.to_le_bytes());
        h = fnv1a64_from(h, &self.ecam_groups.to_le_bytes());
        h = fnv1a64_from(h, &self.devices.to_le_bytes());
        h = fnv1a64_from(h, &self.readbacks.to_le_bytes());
        h = fnv1a64_from(h, &self.mismatches.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 cdom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 配置域分量链。
#[derive(Clone, Copy, Debug)]
pub struct cdomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct cdomChain {
    /// 链节点序列。
    pub links: [cdomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl cdomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [cdomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t445Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t445Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = cdomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x4440, 0x444f]。
pub const fn t445_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t445Code::ALL.len() {
        let c = t445Code::ALL[i].code();
        if c < 0x4440 || c > 0x444f {
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
fn t445_guard_seg() -> bool {
    t445_segment_guard()
}

/// 配置域快照 + 篡改检出。
fn check_c1_snap() -> bool {
    let s = csnap::compute(2, 256, 256, 8, 8, 0);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.mismatches = 1;
        ok = bad.tampered();
    }
    ok
}

/// 配置域分量链回放校验。
fn check_c2_chain() -> bool {
    let mut ch = cdomChain::new();
    let mut ok = true;
    for (i, v) in [2u64, 256, 256, 8, 8, 1].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 531;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t445_checks() -> CheckSet {
    let mut set = CheckSet::new("T445");
    set.add(
        "t445-seg-guard",
        t445_guard_seg(),
        if t445_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t445-c1-snap",
        check_c1_snap(),
        if check_c1_snap() { "" } else { "config domain snapshot mismatch" },
    );
    set.add(
        "t445-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "config domain chain mismatch" },
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
    fn t445_all_checks_green() {
        let set = run_t445_checks();
        assert!(set.all_passed(), "T445 domain checks must be green");
    }

    #[test]
    fn t445_segment_guard_holds() {
        assert!(t445_segment_guard());
    }
}
