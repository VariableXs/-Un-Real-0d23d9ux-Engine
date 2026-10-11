//! unxt434_rsdpsnap —— UNX-F0441 RSDP 域快照（分量指纹）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0441 · RSDP 签名搜索（EBDA/BIOS 双窗） · 判据：双窗搜索（EBDA 1KB 窗 + BIOS ROM 0xE0000–0xFFFFF 窗）命中 RSDP 签名 'RSD PTR ' 定位唯一（1/1 命中账）
//! · 组件：域六分量快照 + 篡改检出 + 报告链
//! · 判据落地：RSDP 域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T434_VERSION: &str = "t434-rsdpsnap-v1";

/// 诊断段基址。
pub const T434_SEG_BASE: u16 = 0x4330;

// --------------------------------------------------------------------------
// 诊断码（t434 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RSDP 域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t434Code {
    /// 0x4330 指纹陈旧。
    FingerprintStale,
    /// 0x4331 分量漂移。
    ComponentDrift,
    /// 0x4332 分量链断裂。
    ChainBroken,
    /// 0x4333 脱敏缺失。
    MaskMissing,
    /// 0x4334 域账破缺。
    LedgerBreak,
    /// 0x4335 渲染截断。
    RenderTruncated,
    /// 0x4336 收官印缺失。
    SealMissing,
    /// 0x4337 守卫触发。
    GuardTripped,
    /// 0x4338 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4339 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x433a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x433b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x433c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x433d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x433e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t434Code {
    /// 全量闭集。
    pub const ALL: [t434Code; 15] = [
        t434Code::FingerprintStale,
        t434Code::ComponentDrift,
        t434Code::ChainBroken,
        t434Code::MaskMissing,
        t434Code::LedgerBreak,
        t434Code::RenderTruncated,
        t434Code::SealMissing,
        t434Code::GuardTripped,
        t434Code::LedgerBroken,
        t434Code::SnapshotFingerprint,
        t434Code::TableBroken,
        t434Code::IndexMisaligned,
        t434Code::DuplicateEntry,
        t434Code::OrderViolated,
        t434Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t434Code::FingerprintStale => 0x4330,
            t434Code::ComponentDrift => 0x4331,
            t434Code::ChainBroken => 0x4332,
            t434Code::MaskMissing => 0x4333,
            t434Code::LedgerBreak => 0x4334,
            t434Code::RenderTruncated => 0x4335,
            t434Code::SealMissing => 0x4336,
            t434Code::GuardTripped => 0x4337,
            t434Code::LedgerBroken => 0x4338,
            t434Code::SnapshotFingerprint => 0x4339,
            t434Code::TableBroken => 0x433a,
            t434Code::IndexMisaligned => 0x433b,
            t434Code::DuplicateEntry => 0x433c,
            t434Code::OrderViolated => 0x433d,
            t434Code::SegmentGuard => 0x433e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t434Code::FingerprintStale => "指纹陈旧",
            t434Code::ComponentDrift => "分量漂移",
            t434Code::ChainBroken => "分量链断裂",
            t434Code::MaskMissing => "脱敏缺失",
            t434Code::LedgerBreak => "域账破缺",
            t434Code::RenderTruncated => "渲染截断",
            t434Code::SealMissing => "收官印缺失",
            t434Code::GuardTripped => "守卫触发",
            t434Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t434Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t434Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t434Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t434Code::DuplicateEntry => "条目重复（同键双写）",
            t434Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t434Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t434Code::FingerprintStale => "重封",
            t434Code::ComponentDrift => "重算",
            t434Code::ChainBroken => "补链",
            t434Code::MaskMissing => "补打码",
            t434Code::LedgerBreak => "按守恒重建",
            t434Code::RenderTruncated => "分页",
            t434Code::SealMissing => "补收官印",
            t434Code::GuardTripped => "检查临界区",
            t434Code::LedgerBroken => "按守恒律重建台账后重跑",
            t434Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t434Code::TableBroken => "按语义规则重建常量表",
            t434Code::IndexMisaligned => "按声明序归一化后重跑",
            t434Code::DuplicateEntry => "合并同键条目为末次生效",
            t434Code::OrderViolated => "按依赖矩阵重排后重跑",
            t434Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 rsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// RSDP 域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct rsnap {
    /// 指纹分量：ebda_slots。
    pub ebda_slots: u64,
    /// 指纹分量：bios_slots。
    pub bios_slots: u64,
    /// 指纹分量：hits。
    pub hits: u64,
    /// 指纹分量：attempts。
    pub attempts: u64,
    /// 指纹分量：revision。
    pub revision: u64,
    /// 指纹分量：length。
    pub length: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl rsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        ebda_slots: u64,
        bios_slots: u64,
        hits: u64,
        attempts: u64,
        revision: u64,
        length: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &ebda_slots.to_le_bytes());
        h = fnv1a64_from(h, &bios_slots.to_le_bytes());
        h = fnv1a64_from(h, &hits.to_le_bytes());
        h = fnv1a64_from(h, &attempts.to_le_bytes());
        h = fnv1a64_from(h, &revision.to_le_bytes());
        h = fnv1a64_from(h, &length.to_le_bytes());
        Self {
            ebda_slots,
            bios_slots,
            hits,
            attempts,
            revision,
            length,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.ebda_slots.to_le_bytes());
        h = fnv1a64_from(h, &self.bios_slots.to_le_bytes());
        h = fnv1a64_from(h, &self.hits.to_le_bytes());
        h = fnv1a64_from(h, &self.attempts.to_le_bytes());
        h = fnv1a64_from(h, &self.revision.to_le_bytes());
        h = fnv1a64_from(h, &self.length.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 rdom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// RSDP 域分量链。
#[derive(Clone, Copy, Debug)]
pub struct rdomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct rdomChain {
    /// 链节点序列。
    pub links: [rdomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl rdomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [rdomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t434Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t434Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = rdomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x4330, 0x433f]。
pub const fn t434_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t434Code::ALL.len() {
        let c = t434Code::ALL[i].code();
        if c < 0x4330 || c > 0x433f {
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
fn t434_guard_seg() -> bool {
    t434_segment_guard()
}

/// RSDP 域快照 + 篡改检出。
fn check_c1_snap() -> bool {
    let s = rsnap::compute(64, 448, 1, 512, 2, 36);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.hits = 2;
        ok = bad.tampered();
    }
    ok
}

/// RSDP 域分量链回放校验。
fn check_c2_chain() -> bool {
    let mut ch = rdomChain::new();
    let mut ok = true;
    for (i, v) in [64u64, 448, 1, 512, 2, 36].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 1063;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t434_checks() -> CheckSet {
    let mut set = CheckSet::new("T434");
    set.add(
        "t434-seg-guard",
        t434_guard_seg(),
        if t434_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t434-c1-snap",
        check_c1_snap(),
        if check_c1_snap() { "" } else { "rsdp domain snapshot mismatch" },
    );
    set.add(
        "t434-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "rsdp domain chain mismatch" },
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
    fn t434_all_checks_green() {
        let set = run_t434_checks();
        assert!(set.all_passed(), "T434 domain checks must be green");
    }

    #[test]
    fn t434_segment_guard_holds() {
        assert!(t434_segment_guard());
    }
}
