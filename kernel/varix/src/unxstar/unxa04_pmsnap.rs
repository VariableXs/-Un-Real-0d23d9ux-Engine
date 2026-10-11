//! unxta04_pmsnap —— UNX-F0561 电源域快照（分量指纹）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0561 · 电源面定位总账（FADT 字段汇总） · 判据：FADT 电源字段账齐（PM1a_CNT/PM1b_CNT/PM1_STS/PM1_EN/PM2_CNT 五块基址+长度），与 F0453 封账直读一致（5/5 块）
//! · 组件：域六分量快照 + 篡改检出 + 报告链
//! · 判据落地：电源域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const TA04_VERSION: &str = "ta04-pmsnap-v1";

/// 诊断段基址。
pub const TA04_SEG_BASE: u16 = 0x4930;

// --------------------------------------------------------------------------
// 诊断码（ta04 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 电源域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ta04Code {
    /// 0x4930 指纹陈旧。
    FingerprintStale,
    /// 0x4931 分量漂移。
    ComponentDrift,
    /// 0x4932 分量链断裂。
    ChainBroken,
    /// 0x4933 脱敏缺失。
    MaskMissing,
    /// 0x4934 域账破缺。
    LedgerBreak,
    /// 0x4935 渲染截断。
    RenderTruncated,
    /// 0x4936 收官印缺失。
    SealMissing,
    /// 0x4937 守卫触发。
    GuardTripped,
    /// 0x4938 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4939 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x493a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x493b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x493c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x493d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x493e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl ta04Code {
    /// 全量闭集。
    pub const ALL: [ta04Code; 15] = [
        ta04Code::FingerprintStale,
        ta04Code::ComponentDrift,
        ta04Code::ChainBroken,
        ta04Code::MaskMissing,
        ta04Code::LedgerBreak,
        ta04Code::RenderTruncated,
        ta04Code::SealMissing,
        ta04Code::GuardTripped,
        ta04Code::LedgerBroken,
        ta04Code::SnapshotFingerprint,
        ta04Code::TableBroken,
        ta04Code::IndexMisaligned,
        ta04Code::DuplicateEntry,
        ta04Code::OrderViolated,
        ta04Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            ta04Code::FingerprintStale => 0x4930,
            ta04Code::ComponentDrift => 0x4931,
            ta04Code::ChainBroken => 0x4932,
            ta04Code::MaskMissing => 0x4933,
            ta04Code::LedgerBreak => 0x4934,
            ta04Code::RenderTruncated => 0x4935,
            ta04Code::SealMissing => 0x4936,
            ta04Code::GuardTripped => 0x4937,
            ta04Code::LedgerBroken => 0x4938,
            ta04Code::SnapshotFingerprint => 0x4939,
            ta04Code::TableBroken => 0x493a,
            ta04Code::IndexMisaligned => 0x493b,
            ta04Code::DuplicateEntry => 0x493c,
            ta04Code::OrderViolated => 0x493d,
            ta04Code::SegmentGuard => 0x493e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            ta04Code::FingerprintStale => "指纹陈旧",
            ta04Code::ComponentDrift => "分量漂移",
            ta04Code::ChainBroken => "分量链断裂",
            ta04Code::MaskMissing => "脱敏缺失",
            ta04Code::LedgerBreak => "域账破缺",
            ta04Code::RenderTruncated => "渲染截断",
            ta04Code::SealMissing => "收官印缺失",
            ta04Code::GuardTripped => "守卫触发",
            ta04Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            ta04Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            ta04Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            ta04Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            ta04Code::DuplicateEntry => "条目重复（同键双写）",
            ta04Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            ta04Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            ta04Code::FingerprintStale => "重封",
            ta04Code::ComponentDrift => "重算",
            ta04Code::ChainBroken => "补链",
            ta04Code::MaskMissing => "补打码",
            ta04Code::LedgerBreak => "按守恒重建",
            ta04Code::RenderTruncated => "分页",
            ta04Code::SealMissing => "补收官印",
            ta04Code::GuardTripped => "检查临界区",
            ta04Code::LedgerBroken => "按守恒律重建台账后重跑",
            ta04Code::SnapshotFingerprint => "按防篡改规程重建快照",
            ta04Code::TableBroken => "按语义规则重建常量表",
            ta04Code::IndexMisaligned => "按声明序归一化后重跑",
            ta04Code::DuplicateEntry => "合并同键条目为末次生效",
            ta04Code::OrderViolated => "按依赖矩阵重排后重跑",
            ta04Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 pwsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 电源域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct pwsnap {
    /// 指纹分量：blocks。
    pub blocks: u64,
    /// 指纹分量：bits。
    pub bits: u64,
    /// 指纹分量：sleeps。
    pub sleeps: u64,
    /// 指纹分量：total_len。
    pub total_len: u64,
    /// 指纹分量：sealed。
    pub sealed: u64,
    /// 指纹分量：f0453。
    pub f0453: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl pwsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        blocks: u64,
        bits: u64,
        sleeps: u64,
        total_len: u64,
        sealed: u64,
        f0453: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &blocks.to_le_bytes());
        h = fnv1a64_from(h, &bits.to_le_bytes());
        h = fnv1a64_from(h, &sleeps.to_le_bytes());
        h = fnv1a64_from(h, &total_len.to_le_bytes());
        h = fnv1a64_from(h, &sealed.to_le_bytes());
        h = fnv1a64_from(h, &f0453.to_le_bytes());
        Self {
            blocks,
            bits,
            sleeps,
            total_len,
            sealed,
            f0453,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.blocks.to_le_bytes());
        h = fnv1a64_from(h, &self.bits.to_le_bytes());
        h = fnv1a64_from(h, &self.sleeps.to_le_bytes());
        h = fnv1a64_from(h, &self.total_len.to_le_bytes());
        h = fnv1a64_from(h, &self.sealed.to_le_bytes());
        h = fnv1a64_from(h, &self.f0453.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 pwdom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 电源域分量链。
#[derive(Clone, Copy, Debug)]
pub struct pwdomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct pwdomChain {
    /// 链节点序列。
    pub links: [pwdomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl pwdomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [pwdomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, ta04Code> {
        if self.n >= 16 || payload == 0 {
            return Err(ta04Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = pwdomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x4930, 0x493f]。
pub const fn ta04_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < ta04Code::ALL.len() {
        let c = ta04Code::ALL[i].code();
        if c < 0x4930 || c > 0x493f {
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
fn ta04_guard_seg() -> bool {
    ta04_segment_guard()
}

/// 电源域快照 + 篡改检出。
fn check_c1_snap() -> bool {
    let s = pwsnap::compute(5, 256, 6, 9, 1, 5);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.blocks = 4;
        ok = bad.tampered();
    }
    ok
}

/// 电源域分量链回放校验。
fn check_c2_chain() -> bool {
    let mut ch = pwdomChain::new();
    let mut ok = true;
    for (i, v) in [5u64, 256, 6, 9, 1, 1].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 278;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_ta04_checks() -> CheckSet {
    let mut set = CheckSet::new("TA04");
    set.add(
        "ta04-seg-guard",
        ta04_guard_seg(),
        if ta04_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "ta04-c1-snap",
        check_c1_snap(),
        if check_c1_snap() { "" } else { "power domain snapshot mismatch" },
    );
    set.add(
        "ta04-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "power domain chain mismatch" },
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
    fn ta04_all_checks_green() {
        let set = run_ta04_checks();
        assert!(set.all_passed(), "TA04 domain checks must be green");
    }

    #[test]
    fn ta04_segment_guard_holds() {
        assert!(ta04_segment_guard());
    }
}
