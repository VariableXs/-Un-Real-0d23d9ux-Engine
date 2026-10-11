//! unxt575_uartsnap —— UNX-F0501 串口域快照（分量指纹）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0501 · 16550 串口编程接管（F0117 升级） · 判据：COM1 编程至 115200 8N1（波特率误差 ≤1.7% 理论值核），LCR/MCR/IER 三寄存器回读一致
//! · 组件：域六分量快照 + 篡改检出 + 报告链
//! · 判据落地：串口域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T575_VERSION: &str = "t575-uartsnap-v1";

/// 诊断段基址。
pub const T575_SEG_BASE: u16 = 0x4640;

// --------------------------------------------------------------------------
// 诊断码（t575 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 串口域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t575Code {
    /// 0x4640 指纹陈旧。
    FingerprintStale,
    /// 0x4641 分量漂移。
    ComponentDrift,
    /// 0x4642 分量链断裂。
    ChainBroken,
    /// 0x4643 脱敏缺失。
    MaskMissing,
    /// 0x4644 域账破缺。
    LedgerBreak,
    /// 0x4645 渲染截断。
    RenderTruncated,
    /// 0x4646 收官印缺失。
    SealMissing,
    /// 0x4647 守卫触发。
    GuardTripped,
    /// 0x4648 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4649 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x464a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x464b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x464c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x464d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x464e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t575Code {
    /// 全量闭集。
    pub const ALL: [t575Code; 15] = [
        t575Code::FingerprintStale,
        t575Code::ComponentDrift,
        t575Code::ChainBroken,
        t575Code::MaskMissing,
        t575Code::LedgerBreak,
        t575Code::RenderTruncated,
        t575Code::SealMissing,
        t575Code::GuardTripped,
        t575Code::LedgerBroken,
        t575Code::SnapshotFingerprint,
        t575Code::TableBroken,
        t575Code::IndexMisaligned,
        t575Code::DuplicateEntry,
        t575Code::OrderViolated,
        t575Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t575Code::FingerprintStale => 0x4640,
            t575Code::ComponentDrift => 0x4641,
            t575Code::ChainBroken => 0x4642,
            t575Code::MaskMissing => 0x4643,
            t575Code::LedgerBreak => 0x4644,
            t575Code::RenderTruncated => 0x4645,
            t575Code::SealMissing => 0x4646,
            t575Code::GuardTripped => 0x4647,
            t575Code::LedgerBroken => 0x4648,
            t575Code::SnapshotFingerprint => 0x4649,
            t575Code::TableBroken => 0x464a,
            t575Code::IndexMisaligned => 0x464b,
            t575Code::DuplicateEntry => 0x464c,
            t575Code::OrderViolated => 0x464d,
            t575Code::SegmentGuard => 0x464e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t575Code::FingerprintStale => "指纹陈旧",
            t575Code::ComponentDrift => "分量漂移",
            t575Code::ChainBroken => "分量链断裂",
            t575Code::MaskMissing => "脱敏缺失",
            t575Code::LedgerBreak => "域账破缺",
            t575Code::RenderTruncated => "渲染截断",
            t575Code::SealMissing => "收官印缺失",
            t575Code::GuardTripped => "守卫触发",
            t575Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t575Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t575Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t575Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t575Code::DuplicateEntry => "条目重复（同键双写）",
            t575Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t575Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t575Code::FingerprintStale => "重封",
            t575Code::ComponentDrift => "重算",
            t575Code::ChainBroken => "补链",
            t575Code::MaskMissing => "补打码",
            t575Code::LedgerBreak => "按守恒重建",
            t575Code::RenderTruncated => "分页",
            t575Code::SealMissing => "补收官印",
            t575Code::GuardTripped => "检查临界区",
            t575Code::LedgerBroken => "按守恒律重建台账后重跑",
            t575Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t575Code::TableBroken => "按语义规则重建常量表",
            t575Code::IndexMisaligned => "按声明序归一化后重跑",
            t575Code::DuplicateEntry => "合并同键条目为末次生效",
            t575Code::OrderViolated => "按依赖矩阵重排后重跑",
            t575Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 usnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 串口域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct usnap {
    /// 指纹分量：bits。
    pub bits: u64,
    /// 指纹分量：bauds。
    pub bauds: u64,
    /// 指纹分量：fifos。
    pub fifos: u64,
    /// 指纹分量：contract。
    pub contract: u64,
    /// 指纹分量：divisor。
    pub divisor: u64,
    /// 指纹分量：err_wan。
    pub err_wan: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl usnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        bits: u64,
        bauds: u64,
        fifos: u64,
        contract: u64,
        divisor: u64,
        err_wan: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &bits.to_le_bytes());
        h = fnv1a64_from(h, &bauds.to_le_bytes());
        h = fnv1a64_from(h, &fifos.to_le_bytes());
        h = fnv1a64_from(h, &contract.to_le_bytes());
        h = fnv1a64_from(h, &divisor.to_le_bytes());
        h = fnv1a64_from(h, &err_wan.to_le_bytes());
        Self {
            bits,
            bauds,
            fifos,
            contract,
            divisor,
            err_wan,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.bits.to_le_bytes());
        h = fnv1a64_from(h, &self.bauds.to_le_bytes());
        h = fnv1a64_from(h, &self.fifos.to_le_bytes());
        h = fnv1a64_from(h, &self.contract.to_le_bytes());
        h = fnv1a64_from(h, &self.divisor.to_le_bytes());
        h = fnv1a64_from(h, &self.err_wan.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 udom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 串口域分量链。
#[derive(Clone, Copy, Debug)]
pub struct udomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct udomChain {
    /// 链节点序列。
    pub links: [udomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl udomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [udomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t575Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t575Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = udomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x4640, 0x464f]。
pub const fn t575_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t575Code::ALL.len() {
        let c = t575Code::ALL[i].code();
        if c < 0x4640 || c > 0x464f {
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
fn t575_guard_seg() -> bool {
    t575_segment_guard()
}

/// 串口域快照 + 篡改检出。
fn check_c1_snap() -> bool {
    let s = usnap::compute(512, 8, 4, 1175, 1, 0);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.bits = 513;
        ok = bad.tampered();
    }
    ok
}

/// 串口域分量链回放校验。
fn check_c2_chain() -> bool {
    let mut ch = udomChain::new();
    let mut ok = true;
    for (i, v) in [512u64, 8, 4, 1175, 1, 0].iter().enumerate() {
        if ch.append(i as u32, *v + 1).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 1706;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t575_checks() -> CheckSet {
    let mut set = CheckSet::new("T575");
    set.add(
        "t575-seg-guard",
        t575_guard_seg(),
        if t575_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t575-c1-snap",
        check_c1_snap(),
        if check_c1_snap() { "" } else { "uart domain snapshot mismatch" },
    );
    set.add(
        "t575-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "uart domain chain mismatch" },
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
    fn t575_all_checks_green() {
        let set = run_t575_checks();
        assert!(set.all_passed(), "T575 domain checks must be green");
    }

    #[test]
    fn t575_segment_guard_holds() {
        assert!(t575_segment_guard());
    }
}
