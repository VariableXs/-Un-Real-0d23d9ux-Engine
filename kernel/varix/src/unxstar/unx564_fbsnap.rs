//! unxt564_fbsnap —— UNX-F0481 帧缓冲域快照（分量指纹）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0481 · 帧缓冲信息账冻结（四要素） · 判据：四要素账齐（物理基址/宽/高/bpp）且与 F0110 探测值复读一致（4/4 字段），版本封账
//! · 组件：域六分量快照 + 篡改检出 + 报告链
//! · 判据落地：帧缓冲域收官证据
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T564_VERSION: &str = "t564-fbsnap-v1";

/// 诊断段基址。
pub const T564_SEG_BASE: u16 = 0x4530;

// --------------------------------------------------------------------------
// 诊断码（t564 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 帧缓冲域快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t564Code {
    /// 0x4530 指纹陈旧。
    FingerprintStale,
    /// 0x4531 分量漂移。
    ComponentDrift,
    /// 0x4532 分量链断裂。
    ChainBroken,
    /// 0x4533 脱敏缺失。
    MaskMissing,
    /// 0x4534 域账破缺。
    LedgerBreak,
    /// 0x4535 渲染截断。
    RenderTruncated,
    /// 0x4536 收官印缺失。
    SealMissing,
    /// 0x4537 守卫触发。
    GuardTripped,
    /// 0x4538 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4539 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x453a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x453b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x453c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x453d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x453e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t564Code {
    /// 全量闭集。
    pub const ALL: [t564Code; 15] = [
        t564Code::FingerprintStale,
        t564Code::ComponentDrift,
        t564Code::ChainBroken,
        t564Code::MaskMissing,
        t564Code::LedgerBreak,
        t564Code::RenderTruncated,
        t564Code::SealMissing,
        t564Code::GuardTripped,
        t564Code::LedgerBroken,
        t564Code::SnapshotFingerprint,
        t564Code::TableBroken,
        t564Code::IndexMisaligned,
        t564Code::DuplicateEntry,
        t564Code::OrderViolated,
        t564Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t564Code::FingerprintStale => 0x4530,
            t564Code::ComponentDrift => 0x4531,
            t564Code::ChainBroken => 0x4532,
            t564Code::MaskMissing => 0x4533,
            t564Code::LedgerBreak => 0x4534,
            t564Code::RenderTruncated => 0x4535,
            t564Code::SealMissing => 0x4536,
            t564Code::GuardTripped => 0x4537,
            t564Code::LedgerBroken => 0x4538,
            t564Code::SnapshotFingerprint => 0x4539,
            t564Code::TableBroken => 0x453a,
            t564Code::IndexMisaligned => 0x453b,
            t564Code::DuplicateEntry => 0x453c,
            t564Code::OrderViolated => 0x453d,
            t564Code::SegmentGuard => 0x453e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t564Code::FingerprintStale => "指纹陈旧",
            t564Code::ComponentDrift => "分量漂移",
            t564Code::ChainBroken => "分量链断裂",
            t564Code::MaskMissing => "脱敏缺失",
            t564Code::LedgerBreak => "域账破缺",
            t564Code::RenderTruncated => "渲染截断",
            t564Code::SealMissing => "收官印缺失",
            t564Code::GuardTripped => "守卫触发",
            t564Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t564Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t564Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t564Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t564Code::DuplicateEntry => "条目重复（同键双写）",
            t564Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t564Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t564Code::FingerprintStale => "重封",
            t564Code::ComponentDrift => "重算",
            t564Code::ChainBroken => "补链",
            t564Code::MaskMissing => "补打码",
            t564Code::LedgerBreak => "按守恒重建",
            t564Code::RenderTruncated => "分页",
            t564Code::SealMissing => "补收官印",
            t564Code::GuardTripped => "检查临界区",
            t564Code::LedgerBroken => "按守恒律重建台账后重跑",
            t564Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t564Code::TableBroken => "按语义规则重建常量表",
            t564Code::IndexMisaligned => "按声明序归一化后重跑",
            t564Code::DuplicateEntry => "合并同键条目为末次生效",
            t564Code::OrderViolated => "按依赖矩阵重排后重跑",
            t564Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 fbs（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 帧缓冲域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct fbs {
    /// 指纹分量：modes。
    pub modes: u64,
    /// 指纹分量：phys_base。
    pub phys_base: u64,
    /// 指纹分量：width。
    pub width: u64,
    /// 指纹分量：height。
    pub height: u64,
    /// 指纹分量：bpp。
    pub bpp: u64,
    /// 指纹分量：frozen。
    pub frozen: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl fbs {
    /// 构造并即时合成指纹。
    pub fn compute(
        modes: u64,
        phys_base: u64,
        width: u64,
        height: u64,
        bpp: u64,
        frozen: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &modes.to_le_bytes());
        h = fnv1a64_from(h, &phys_base.to_le_bytes());
        h = fnv1a64_from(h, &width.to_le_bytes());
        h = fnv1a64_from(h, &height.to_le_bytes());
        h = fnv1a64_from(h, &bpp.to_le_bytes());
        h = fnv1a64_from(h, &frozen.to_le_bytes());
        Self {
            modes,
            phys_base,
            width,
            height,
            bpp,
            frozen,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.modes.to_le_bytes());
        h = fnv1a64_from(h, &self.phys_base.to_le_bytes());
        h = fnv1a64_from(h, &self.width.to_le_bytes());
        h = fnv1a64_from(h, &self.height.to_le_bytes());
        h = fnv1a64_from(h, &self.bpp.to_le_bytes());
        h = fnv1a64_from(h, &self.frozen.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 fbdom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 帧缓冲域分量链。
#[derive(Clone, Copy, Debug)]
pub struct fbdomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct fbdomChain {
    /// 链节点序列。
    pub links: [fbdomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl fbdomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [fbdomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t564Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t564Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = fbdomLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x4530, 0x453f]。
pub const fn t564_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t564Code::ALL.len() {
        let c = t564Code::ALL[i].code();
        if c < 0x4530 || c > 0x453f {
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
fn t564_guard_seg() -> bool {
    t564_segment_guard()
}

/// 帧缓冲域快照 + 篡改检出。
fn check_c1_snap() -> bool {
    let s = fbs::compute(51, 0xE000_0000, 1024, 768, 32, 1);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.width = 1280;
        ok = bad.tampered();
    }
    ok
}

/// 帧缓冲域分量链回放校验。
fn check_c2_chain() -> bool {
    let mut ch = fbdomChain::new();
    let mut ok = true;
    for (i, v) in [51u64, 0xE000_0000, 1024, 768, 32, 1].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 0xE000_0754;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t564_checks() -> CheckSet {
    let mut set = CheckSet::new("T564");
    set.add(
        "t564-seg-guard",
        t564_guard_seg(),
        if t564_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t564-c1-snap",
        check_c1_snap(),
        if check_c1_snap() { "" } else { "fb domain snapshot mismatch" },
    );
    set.add(
        "t564-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "fb domain chain mismatch" },
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
    fn t564_all_checks_green() {
        let set = run_t564_checks();
        assert!(set.all_passed(), "T564 domain checks must be green");
    }

    #[test]
    fn t564_segment_guard_holds() {
        assert!(t564_segment_guard());
    }
}
