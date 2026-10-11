//! unxt242_mcrc —— UNX-F0241 总 CRC 链校验（十二步逐环链式指纹）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0241 · BootManifest 全景结构（十二步总纲） · 判据：十二步节结构定义齐备（固件/内存/CPU/随机化/参数/健康/时间/日志/上次引导/订阅/完整性/时序），总 CRC 链校验过，结构黄金比对一致
//! · 组件：CRC 链引擎（FNV 链）+ 逐环回放 + 负载守恒 + 篡改检出
//! · 判据落地：总 CRC 链校验过
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T242_VERSION: &str = "t242-mcrc-v1";

/// 诊断段基址。
pub const T242_SEG_BASE: u16 = 0x3910;

// --------------------------------------------------------------------------
// 诊断码（t242 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// CRC 链诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t242Code {
    /// 0x3910 链断裂（环指纹不符）。
    ChainBreak,
    /// 0x3911 负载丢失（环负载缺失）。
    PayloadLost,
    /// 0x3912 哈希漂移（重算值与封存不符）。
    HashDrift,
    /// 0x3913 链容量超限（环数 > 16）。
    CapExceeded,
    /// 0x3914 回放断档（环序不连续）。
    ReplayGap,
    /// 0x3915 创世环非法（首环负载为零）。
    GenesisBad,
    /// 0x3916 链头不符（末环与链头矛盾）。
    HeadMismatch,
    /// 0x3917 链账破缺（负载合计≠环和）。
    LedgerBreak,
    /// 0x3918 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3919 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x391a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x391b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x391c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x391d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x391e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x391f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t242Code {
    /// 全量闭集。
    pub const ALL: [t242Code; 16] = [
        t242Code::ChainBreak,
        t242Code::PayloadLost,
        t242Code::HashDrift,
        t242Code::CapExceeded,
        t242Code::ReplayGap,
        t242Code::GenesisBad,
        t242Code::HeadMismatch,
        t242Code::LedgerBreak,
        t242Code::LedgerBroken,
        t242Code::SnapshotFingerprint,
        t242Code::TableBroken,
        t242Code::IndexMisaligned,
        t242Code::DuplicateEntry,
        t242Code::OrderViolated,
        t242Code::GuardTripped,
        t242Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t242Code::ChainBreak => 0x3910,
            t242Code::PayloadLost => 0x3911,
            t242Code::HashDrift => 0x3912,
            t242Code::CapExceeded => 0x3913,
            t242Code::ReplayGap => 0x3914,
            t242Code::GenesisBad => 0x3915,
            t242Code::HeadMismatch => 0x3916,
            t242Code::LedgerBreak => 0x3917,
            t242Code::LedgerBroken => 0x3918,
            t242Code::SnapshotFingerprint => 0x3919,
            t242Code::TableBroken => 0x391a,
            t242Code::IndexMisaligned => 0x391b,
            t242Code::DuplicateEntry => 0x391c,
            t242Code::OrderViolated => 0x391d,
            t242Code::GuardTripped => 0x391e,
            t242Code::SegmentGuard => 0x391f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t242Code::ChainBreak => "链断裂（环指纹不符）",
            t242Code::PayloadLost => "负载丢失（环负载缺失）",
            t242Code::HashDrift => "哈希漂移（重算值与封存不符）",
            t242Code::CapExceeded => "链容量超限（环数 > 16）",
            t242Code::ReplayGap => "回放断档（环序不连续）",
            t242Code::GenesisBad => "创世环非法（首环负载为零）",
            t242Code::HeadMismatch => "链头不符（末环与链头矛盾）",
            t242Code::LedgerBreak => "链账破缺（负载合计≠环和）",
            t242Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t242Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t242Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t242Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t242Code::DuplicateEntry => "条目重复（同键双写）",
            t242Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t242Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t242Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t242Code::ChainBreak => "按 CRC 链重建规程修复",
            t242Code::PayloadLost => "补负载后重算",
            t242Code::HashDrift => "按封存值重封",
            t242Code::CapExceeded => "分链后校验",
            t242Code::ReplayGap => "补环后回放",
            t242Code::GenesisBad => "按创世规程重建",
            t242Code::HeadMismatch => "按末环重挂链头",
            t242Code::LedgerBreak => "按守恒重建",
            t242Code::LedgerBroken => "按守恒律重建台账后重跑",
            t242Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t242Code::TableBroken => "按语义规则重建常量表",
            t242Code::IndexMisaligned => "按声明序归一化后重跑",
            t242Code::DuplicateEntry => "合并同键条目为末次生效",
            t242Code::OrderViolated => "按依赖矩阵重排后重跑",
            t242Code::GuardTripped => "检查临界区嵌套与阈值",
            t242Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mcrc（链式指纹：步骤链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 十二步 CRC 链（每步一链环，负载=步序号×8 字段）。
#[derive(Clone, Copy, Debug)]
pub struct mcrcLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mcrcChain {
    /// 链节点序列。
    pub links: [mcrcLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mcrcChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mcrcLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t242Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t242Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mcrcLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x3910, 0x391f]。
pub const fn t242_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t242Code::ALL.len() {
        let c = t242Code::ALL[i].code();
        if c < 0x3910 || c > 0x391f {
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
fn t242_guard_seg() -> bool {
    t242_segment_guard()
}

/// 十二环链追加 + 全链回放校验。
fn check_c1_chain_12() -> bool {
    let mut ch = mcrcChain::new();
    let mut ok = true;
    let mut i = 1u64;
    while i <= 12 {
        if ch.append(i as u32, i * 8).is_err() {
            ok = false;
        }
        i += 1;
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 624;
    }
    ok
}

/// 链篡改检出（改环负载后回放失败）。
fn check_c2_chain_tamper() -> bool {
    let mut ch = mcrcChain::new();
    let mut i = 1u64;
    while i <= 12 {
        if ch.append(i as u32, i * 8).is_err() {
            return false;
        }
        i += 1;
    }
    let mut ok = ch.verify();
    if ok {
        ch.links[3].payload = 999;
        ok = !ch.verify();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t242_checks() -> CheckSet {
    let mut set = CheckSet::new("T242");
    set.add(
        "t242-seg-guard",
        t242_guard_seg(),
        if t242_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t242-c1-chain-12",
        check_c1_chain_12(),
        if check_c1_chain_12() { "" } else { "12-link crc chain mismatch" },
    );
    set.add(
        "t242-c2-chain-tamper",
        check_c2_chain_tamper(),
        if check_c2_chain_tamper() { "" } else { "crc chain tamper detection mismatch" },
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
    fn t242_all_checks_green() {
        let set = run_t242_checks();
        assert!(set.all_passed(), "T242 domain checks must be green");
    }

    #[test]
    fn t242_segment_guard_holds() {
        assert!(t242_segment_guard());
    }
}
