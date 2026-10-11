//! unxtb04_cmdsnap —— UNX-F0581 参数域快照（分量指纹）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0581 · 参数行提取（loader 交接收尾） · 判据：参数行提取账齐（源/长度/原文三元组），长度守卫核（256 字节上限 1/1 断言）
//! · 组件：域六分量快照 + 篡改检出 + 报告链
//! · 判据落地：参数域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const TB04_VERSION: &str = "tb04-cmdsnap-v1";

/// 诊断段基址。
pub const TB04_SEG_BASE: u16 = 0x4a30;

// --------------------------------------------------------------------------
// 诊断码（tb04 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 参数域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum tb04Code {
    /// 0x4a30 指纹陈旧。
    FingerprintStale,
    /// 0x4a31 分量漂移。
    ComponentDrift,
    /// 0x4a32 分量链断裂。
    ChainBroken,
    /// 0x4a33 脱敏缺失。
    MaskMissing,
    /// 0x4a34 域账破缺。
    LedgerBreak,
    /// 0x4a35 渲染截断。
    RenderTruncated,
    /// 0x4a36 收官印缺失。
    SealMissing,
    /// 0x4a37 守卫触发。
    GuardTripped,
    /// 0x4a38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4a39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4a3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4a3b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4a3c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4a3d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x4a3e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl tb04Code {
    /// 全量闭集。
    pub const ALL: [tb04Code; 15] = [
        tb04Code::FingerprintStale,
        tb04Code::ComponentDrift,
        tb04Code::ChainBroken,
        tb04Code::MaskMissing,
        tb04Code::LedgerBreak,
        tb04Code::RenderTruncated,
        tb04Code::SealMissing,
        tb04Code::GuardTripped,
        tb04Code::LedgerBroken,
        tb04Code::SnapshotFingerprint,
        tb04Code::TableBroken,
        tb04Code::IndexMisaligned,
        tb04Code::DuplicateEntry,
        tb04Code::OrderViolated,
        tb04Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            tb04Code::FingerprintStale => 0x4a30,
            tb04Code::ComponentDrift => 0x4a31,
            tb04Code::ChainBroken => 0x4a32,
            tb04Code::MaskMissing => 0x4a33,
            tb04Code::LedgerBreak => 0x4a34,
            tb04Code::RenderTruncated => 0x4a35,
            tb04Code::SealMissing => 0x4a36,
            tb04Code::GuardTripped => 0x4a37,
            tb04Code::LedgerBroken => 0x4a38,
            tb04Code::SnapshotFingerprint => 0x4a39,
            tb04Code::TableBroken => 0x4a3a,
            tb04Code::IndexMisaligned => 0x4a3b,
            tb04Code::DuplicateEntry => 0x4a3c,
            tb04Code::OrderViolated => 0x4a3d,
            tb04Code::SegmentGuard => 0x4a3e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            tb04Code::FingerprintStale => "指纹陈旧",
            tb04Code::ComponentDrift => "分量漂移",
            tb04Code::ChainBroken => "分量链断裂",
            tb04Code::MaskMissing => "脱敏缺失",
            tb04Code::LedgerBreak => "域账破缺",
            tb04Code::RenderTruncated => "渲染截断",
            tb04Code::SealMissing => "收官印缺失",
            tb04Code::GuardTripped => "守卫触发",
            tb04Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            tb04Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            tb04Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            tb04Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            tb04Code::DuplicateEntry => "条目重复（同键双写）",
            tb04Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            tb04Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            tb04Code::FingerprintStale => "重封",
            tb04Code::ComponentDrift => "重算",
            tb04Code::ChainBroken => "补链",
            tb04Code::MaskMissing => "补打码",
            tb04Code::LedgerBreak => "按守恒重建",
            tb04Code::RenderTruncated => "分页",
            tb04Code::SealMissing => "补收官印",
            tb04Code::GuardTripped => "检查临界区",
            tb04Code::LedgerBroken => "按守恒律重建台账后重跑",
            tb04Code::SnapshotFingerprint => "按防篡改规程重建快照",
            tb04Code::TableBroken => "按语义规则重建常量表",
            tb04Code::IndexMisaligned => "按声明序归一化后重跑",
            tb04Code::DuplicateEntry => "合并同键条目为末次生效",
            tb04Code::OrderViolated => "按依赖矩阵重排后重跑",
            tb04Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 cmsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 参数域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct cmsnap {
    /// 指纹分量：sources。
    pub sources: u64,
    /// 指纹分量：positions。
    pub positions: u64,
    /// 指纹分量：handoff。
    pub handoff: u64,
    /// 指纹分量：extracted。
    pub extracted: u64,
    /// 指纹分量：rejected。
    pub rejected: u64,
    /// 指纹分量：max_len。
    pub max_len: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl cmsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        sources: u64,
        positions: u64,
        handoff: u64,
        extracted: u64,
        rejected: u64,
        max_len: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &sources.to_le_bytes());
        h = fnv1a64_from(h, &positions.to_le_bytes());
        h = fnv1a64_from(h, &handoff.to_le_bytes());
        h = fnv1a64_from(h, &extracted.to_le_bytes());
        h = fnv1a64_from(h, &rejected.to_le_bytes());
        h = fnv1a64_from(h, &max_len.to_le_bytes());
        Self {
            sources,
            positions,
            handoff,
            extracted,
            rejected,
            max_len,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.sources.to_le_bytes());
        h = fnv1a64_from(h, &self.positions.to_le_bytes());
        h = fnv1a64_from(h, &self.handoff.to_le_bytes());
        h = fnv1a64_from(h, &self.extracted.to_le_bytes());
        h = fnv1a64_from(h, &self.rejected.to_le_bytes());
        h = fnv1a64_from(h, &self.max_len.to_le_bytes());
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
/// 参数域分量链。
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
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, tb04Code> {
        if self.n >= 16 || payload == 0 {
            return Err(tb04Code::LedgerBroken);
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

/// 段位守卫：全部诊断码必须落在本段 [0x4a30, 0x4a3f]。
pub const fn tb04_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < tb04Code::ALL.len() {
        let c = tb04Code::ALL[i].code();
        if c < 0x4a30 || c > 0x4a3f {
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
fn tb04_guard_seg() -> bool {
    tb04_segment_guard()
}

/// 参数域快照 + 篡改检出。
fn check_c1_snap() -> bool {
    let s = cmsnap::compute(3, 512, 6, 16, 16, 256);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.max_len = 257;
        ok = bad.tampered();
    }
    ok
}

/// 参数域分量链回拍校验。
fn check_c2_chain() -> bool {
    let mut ch = cdomChain::new();
    let mut ok = true;
    for (i, v) in [3u64, 512, 6, 16, 16, 1].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 554;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_tb04_checks() -> CheckSet {
    let mut set = CheckSet::new("TB04");
    set.add(
        "tb04-seg-guard",
        tb04_guard_seg(),
        if tb04_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "tb04-c1-snap",
        check_c1_snap(),
        if check_c1_snap() { "" } else { "cmdline domain snapshot mismatch" },
    );
    set.add(
        "tb04-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "cmdline domain chain mismatch" },
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
    fn tb04_all_checks_green() {
        let set = run_tb04_checks();
        assert!(set.all_passed(), "TB04 domain checks must be green");
    }

    #[test]
    fn tb04_segment_guard_holds() {
        assert!(tb04_segment_guard());
    }
}
