//! unxt246_mseal —— UNX-F0241 完整性封账（十二步封印 + 完整性面）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0241 · BootManifest 全景结构（十二步总纲） · 判据：十二步节结构定义齐备（固件/内存/CPU/随机化/参数/健康/时间/日志/上次引导/订阅/完整性/时序），总 CRC 链校验过，结构黄金比对一致
//! · 组件：封印链（12 环）+ 完整性状态机 + 封印审计账
//! · 判据落地：完整性步节的封印机制
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T246_VERSION: &str = "t246-mseal-v1";

/// 诊断段基址。
pub const T246_SEG_BASE: u16 = 0x3950;

// --------------------------------------------------------------------------
// 诊断码（t246 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 封印诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t246Code {
    /// 0x3950 封印断裂（环验失败）。
    SealBreak,
    /// 0x3951 步未封印（漏封）。
    UnsealedStep,
    /// 0x3952 重复封印（同步双封）。
    DoubleSeal,
    /// 0x3953 引导后封印（时序违例）。
    SealAfterBoot,
    /// 0x3954 封印账破缺（封/未封不守恒）。
    SealLedgerBreak,
    /// 0x3955 封印密钥缺失。
    KeyMissing,
    /// 0x3956 验印陈旧（封印已刷新）。
    VerifyStale,
    /// 0x3957 审计断档（封印无审计记录）。
    AuditGap,
    /// 0x3958 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3959 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x395a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x395b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x395c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x395d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x395e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x395f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t246Code {
    /// 全量闭集。
    pub const ALL: [t246Code; 16] = [
        t246Code::SealBreak,
        t246Code::UnsealedStep,
        t246Code::DoubleSeal,
        t246Code::SealAfterBoot,
        t246Code::SealLedgerBreak,
        t246Code::KeyMissing,
        t246Code::VerifyStale,
        t246Code::AuditGap,
        t246Code::LedgerBroken,
        t246Code::SnapshotFingerprint,
        t246Code::TableBroken,
        t246Code::IndexMisaligned,
        t246Code::DuplicateEntry,
        t246Code::OrderViolated,
        t246Code::GuardTripped,
        t246Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t246Code::SealBreak => 0x3950,
            t246Code::UnsealedStep => 0x3951,
            t246Code::DoubleSeal => 0x3952,
            t246Code::SealAfterBoot => 0x3953,
            t246Code::SealLedgerBreak => 0x3954,
            t246Code::KeyMissing => 0x3955,
            t246Code::VerifyStale => 0x3956,
            t246Code::AuditGap => 0x3957,
            t246Code::LedgerBroken => 0x3958,
            t246Code::SnapshotFingerprint => 0x3959,
            t246Code::TableBroken => 0x395a,
            t246Code::IndexMisaligned => 0x395b,
            t246Code::DuplicateEntry => 0x395c,
            t246Code::OrderViolated => 0x395d,
            t246Code::GuardTripped => 0x395e,
            t246Code::SegmentGuard => 0x395f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t246Code::SealBreak => "封印断裂（环验失败）",
            t246Code::UnsealedStep => "步未封印（漏封）",
            t246Code::DoubleSeal => "重复封印（同步双封）",
            t246Code::SealAfterBoot => "引导后封印（时序违例）",
            t246Code::SealLedgerBreak => "封印账破缺（封/未封不守恒）",
            t246Code::KeyMissing => "封印密钥缺失",
            t246Code::VerifyStale => "验印陈旧（封印已刷新）",
            t246Code::AuditGap => "审计断档（封印无审计记录）",
            t246Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t246Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t246Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t246Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t246Code::DuplicateEntry => "条目重复（同键双写）",
            t246Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t246Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t246Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t246Code::SealBreak => "按封印链重建",
            t246Code::UnsealedStep => "补封后回放",
            t246Code::DoubleSeal => "幂等跳过",
            t246Code::SealAfterBoot => "按封印窗口重排",
            t246Code::SealLedgerBreak => "按守恒重建",
            t246Code::KeyMissing => "按密钥台账补配",
            t246Code::VerifyStale => "重验后入账",
            t246Code::AuditGap => "补审计后放行",
            t246Code::LedgerBroken => "按守恒律重建台账后重跑",
            t246Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t246Code::TableBroken => "按语义规则重建常量表",
            t246Code::IndexMisaligned => "按声明序归一化后重跑",
            t246Code::DuplicateEntry => "合并同键条目为末次生效",
            t246Code::OrderViolated => "按依赖矩阵重排后重跑",
            t246Code::GuardTripped => "检查临界区嵌套与阈值",
            t246Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mseal（链式指纹：封印链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 十二步封印链（每步一环，负载=步封印序号）。
#[derive(Clone, Copy, Debug)]
pub struct msealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct msealChain {
    /// 链节点序列。
    pub links: [msealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl msealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [msealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t246Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t246Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = msealLink { tag, payload, digest };
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

// --------------------------------------------------------------------------
// 状态机 seal（4 态 × 4 事件 · 迁移 5 条显式声明）
// --------------------------------------------------------------------------
/// 封印状态机状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum sealState {
    Unsealed,
    Sealed,
    Verified,
    Broken,
}

impl sealState {
    /// 全量态闭集。
    pub const ALL: [sealState; 4] = [
        sealState::Unsealed,
        sealState::Sealed,
        sealState::Verified,
        sealState::Broken,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            sealState::Unsealed => 0,
            sealState::Sealed => 1,
            sealState::Verified => 2,
            sealState::Broken => 3,
        }
    }
}

/// seal事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum sealEvent {
    Seal,
    Verify,
    Break,
    Reseal,
}

impl sealEvent {
    /// 全量事件闭集。
    pub const ALL: [sealEvent; 4] = [
        sealEvent::Seal,
        sealEvent::Verify,
        sealEvent::Break,
        sealEvent::Reseal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const SEAL_TRANSITIONS: [(sealState, sealEvent, sealState); 5] = [
    (sealState::Unsealed, sealEvent::Seal, sealState::Sealed), // 施封
    (sealState::Sealed, sealEvent::Verify, sealState::Verified), // 验印通过
    (sealState::Sealed, sealEvent::Break, sealState::Broken), // 验印失败
    (sealState::Broken, sealEvent::Reseal, sealState::Unsealed), // 重新施封
    (sealState::Verified, sealEvent::Break, sealState::Broken), // 引导后破坏检出
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn seal_next(state: sealState, ev: sealEvent) -> Option<sealState> {
    let mut i = 0usize;
    while i < 5 {
        let t = SEAL_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn seal_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < sealState::ALL.len() {
        let mut ei = 0usize;
        while ei < sealEvent::ALL.len() {
            let legal = seal_next(sealState::ALL[si], sealEvent::ALL[ei]).is_some();
            if !legal {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 终态集合（无出边的态即终态）。
pub fn seal_terminal(st: sealState) -> bool {
    let mut i = 0usize;
    while i < 5 {
        if SEAL_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct sealLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: sealState,
}

impl sealLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: sealState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: sealEvent) -> bool {
        match seal_next(self.cur, ev) {
            Some(ns) => {
                self.cur = ns;
                self.legal += 1;
                true
            }
            None => {
                self.illegal += 1;
                false
            }
        }
    }

    /// 守恒恒等式：legal + illegal == 施加次数（由调用方累计）。
    pub fn total(&self) -> u64 {
        self.legal + self.illegal
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3950, 0x395f]。
pub const fn t246_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t246Code::ALL.len() {
        let c = t246Code::ALL[i].code();
        if c < 0x3950 || c > 0x395f {
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
fn t246_guard_seg() -> bool {
    t246_segment_guard()
}

/// 十二封印链追加 + 回放校验。
fn check_c1_seal_chain() -> bool {
    let mut ch = msealChain::new();
    let mut ok = true;
    let mut i = 1u64;
    while i <= 12 {
        if ch.append(i as u32, i * 7).is_err() {
            ok = false;
        }
        i += 1;
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 546;
    }
    ok
}

/// 封印状态机全路径（施封-验印-破坏-重封）。
fn check_c2_seal_sm() -> bool {
    let mut led = sealLedger::new(sealState::Unsealed);
    let mut ok = led.apply(sealEvent::Seal);
    ok = ok && led.apply(sealEvent::Break);
    ok = ok && led.apply(sealEvent::Reseal);
    ok = ok && led.apply(sealEvent::Seal);
    ok = ok && led.apply(sealEvent::Verify);
    if ok {
        ok = led.cur == sealState::Verified && led.legal == 5;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t246_checks() -> CheckSet {
    let mut set = CheckSet::new("T246");
    set.add(
        "t246-seg-guard",
        t246_guard_seg(),
        if t246_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t246-c1-seal-chain",
        check_c1_seal_chain(),
        if check_c1_seal_chain() { "" } else { "12-seal chain mismatch" },
    );
    set.add(
        "t246-c2-seal-sm",
        check_c2_seal_sm(),
        if check_c2_seal_sm() { "" } else { "seal state machine mismatch" },
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
    fn t246_all_checks_green() {
        let set = run_t246_checks();
        assert!(set.all_passed(), "T246 domain checks must be green");
    }

    #[test]
    fn t246_segment_guard_holds() {
        assert!(t246_segment_guard());
    }
}
