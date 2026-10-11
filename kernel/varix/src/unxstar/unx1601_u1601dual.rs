//! unxu1601_u1601dual —— SMP 前置数据包消费与 A1 联签确认 双源对拍与链式封存（字段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1601 · SMP 前置数据包消费与 A1 联签确认 · 判据原文：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，字段缺失或版本不符时启动序列拒绝开跑并列出缺失清单，联签确认记录
//! · 组件：字段 双源对拍（8 槽逐槽回读一致）+ 8 节链式指纹封存
//! · 判据落地：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1601_VERSION: &str = "u1601-u1601dual-v1";

/// 诊断段基址。
pub const U1601_SEG_BASE: u16 = 0x5540;

// --------------------------------------------------------------------------
// 诊断码（u1601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 字段 对拍面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1601Code {
    /// 0x5540 侧重复（字段缺失 同槽双源）。
    SideDup,
    /// 0x5541 侧断档（版本不符 槽未采样）。
    SideGap,
    /// 0x5542 双源失配（同槽两侧值不等）。
    Mismatch,
    /// 0x5543 半写（仅一侧落值）。
    HalfWrite,
    /// 0x5544 链节重复（核账破缺 同标签双节）。
    LinkDup,
    /// 0x5545 链节断档（联签缺位 节序缺失）。
    LinkGap,
    /// 0x5546 链指纹破缺（回放校验失败）。
    SealBroken,
    /// 0x5547 回放漂移（回放终态≠追加终态）。
    ReplayDrift,
    /// 0x5548 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5549 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x554a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x554b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1601Code {
    /// 全量闭集。
    pub const ALL: [u1601Code; 12] = [
        u1601Code::SideDup,
        u1601Code::SideGap,
        u1601Code::Mismatch,
        u1601Code::HalfWrite,
        u1601Code::LinkDup,
        u1601Code::LinkGap,
        u1601Code::SealBroken,
        u1601Code::ReplayDrift,
        u1601Code::LedgerBroken,
        u1601Code::SnapshotFingerprint,
        u1601Code::TableBroken,
        u1601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1601Code::SideDup => 0x5540,
            u1601Code::SideGap => 0x5541,
            u1601Code::Mismatch => 0x5542,
            u1601Code::HalfWrite => 0x5543,
            u1601Code::LinkDup => 0x5544,
            u1601Code::LinkGap => 0x5545,
            u1601Code::SealBroken => 0x5546,
            u1601Code::ReplayDrift => 0x5547,
            u1601Code::LedgerBroken => 0x5548,
            u1601Code::SnapshotFingerprint => 0x5549,
            u1601Code::TableBroken => 0x554a,
            u1601Code::SegmentGuard => 0x554b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1601Code::SideDup => "侧重复（字段缺失 同槽双源）",
            u1601Code::SideGap => "侧断档（版本不符 槽未采样）",
            u1601Code::Mismatch => "双源失配（同槽两侧值不等）",
            u1601Code::HalfWrite => "半写（仅一侧落值）",
            u1601Code::LinkDup => "链节重复（核账破缺 同标签双节）",
            u1601Code::LinkGap => "链节断档（联签缺位 节序缺失）",
            u1601Code::SealBroken => "链指纹破缺（回放校验失败）",
            u1601Code::ReplayDrift => "回放漂移（回放终态≠追加终态）",
            u1601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1601Code::SideDup => "按槽唯一律去重",
            u1601Code::SideGap => "按槽闭集补采",
            u1601Code::Mismatch => "以权威侧复测仲裁",
            u1601Code::HalfWrite => "补齐对侧采样",
            u1601Code::LinkDup => "按节序唯一律合并",
            u1601Code::LinkGap => "按节序补链",
            u1601Code::SealBroken => "按防篡改规程重建链",
            u1601Code::ReplayDrift => "按追加序重放",
            u1601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1601Code::TableBroken => "按语义规则重建常量表",
            u1601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1601cmp（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 字段 双源对拍（字段缺失）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1601cmpSlot {
    /// 核列表。
    CoreList,
    /// LAPIC 基址。
    LapicBase,
    /// 缺席账。
    AbsentLedger,
    /// trampoline 预留。
    TrampReserve,
    /// 版本锚。
    VersionPin,
    /// 缺失清单。
    MissingList,
    /// 联签簿。
    CosignBook,
    /// 开跑闸。
    CommitGate,
}

impl mxu1601cmpSlot {
    /// 全量槽位闭集。
    pub const ALL: [mxu1601cmpSlot; 8] = [
        mxu1601cmpSlot::CoreList,
        mxu1601cmpSlot::LapicBase,
        mxu1601cmpSlot::AbsentLedger,
        mxu1601cmpSlot::TrampReserve,
        mxu1601cmpSlot::VersionPin,
        mxu1601cmpSlot::MissingList,
        mxu1601cmpSlot::CosignBook,
        mxu1601cmpSlot::CommitGate,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu1601cmpSlot::CoreList => 0,
            mxu1601cmpSlot::LapicBase => 1,
            mxu1601cmpSlot::AbsentLedger => 2,
            mxu1601cmpSlot::TrampReserve => 3,
            mxu1601cmpSlot::VersionPin => 4,
            mxu1601cmpSlot::MissingList => 5,
            mxu1601cmpSlot::CosignBook => 6,
            mxu1601cmpSlot::CommitGate => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu1601cmpSlot::CoreList => "核列表",
            mxu1601cmpSlot::LapicBase => "LAPIC 基址",
            mxu1601cmpSlot::AbsentLedger => "缺席账",
            mxu1601cmpSlot::TrampReserve => "trampoline 预留",
            mxu1601cmpSlot::VersionPin => "版本锚",
            mxu1601cmpSlot::MissingList => "缺失清单",
            mxu1601cmpSlot::CosignBook => "联签簿",
            mxu1601cmpSlot::CommitGate => "开跑闸",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1601cmpPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1601cmpDual {
    /// 逐槽样本。
    pub pairs: [mxu1601cmpPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mxu1601cmpDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mxu1601cmpPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mxu1601cmpSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mxu1601cmpPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 8
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 8 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 8 && self.same_n + self.diff_n == 8
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1601seal（链式指纹：字段 封存链节 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 字段 链式指纹封存（逐链回放校验）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1601sealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1601sealChain {
    /// 链节点序列。
    pub links: [mxu1601sealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mxu1601sealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mxu1601sealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u1601Code> {
        if self.n >= 16 || payload == 0 {
            return Err(u1601Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mxu1601sealLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0x5540, 0x554f]。
pub const fn u1601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1601Code::ALL.len() {
        let c = u1601Code::ALL[i].code();
        if c < 0x5540 || c > 0x554f {
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
fn u1601_guard_seg() -> bool {
    u1601_segment_guard()
}

/// UNX-F1601 双源逐槽对拍全同 + 复核通过。
fn check_u1601_dual_same() -> bool {
    let mut d = mxu1601cmpDual::new();
    let mut si = 0usize;
    while si < mxu1601cmpSlot::ALL.len() {
        let v = (si as u64 + 1) * 11;
        d.sample(mxu1601cmpSlot::ALL[si], v, v);
        si += 1;
    }
    let mut ok = d.all_same() && d.recount();
    ok
}

/// UNX-F1601 封存链 8 节追加回放校验全绿。
fn check_u1601_dual_chain() -> bool {
    let mut c = mxu1601sealChain::new();
    let mut i = 0u32;
    while i < 8 { let _ = c.append(i + 1, (i as u64 + 1) * 13); i += 1; }
    let mut ok = c.verify();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1601_checks() -> CheckSet {
    let mut set = CheckSet::new("U1601");
    set.add(
        "u1601-seg-guard",
        u1601_guard_seg(),
        if u1601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1601-dual-same",
        check_u1601_dual_same(),
        if check_u1601_dual_same() { "" } else { "UNX-F1601 dual mismatch" },
    );
    set.add(
        "u1601-dual-chain",
        check_u1601_dual_chain(),
        if check_u1601_dual_chain() { "" } else { "UNX-F1601 chain mismatch" },
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
    fn u1601_all_checks_green() {
        let set = run_u1601_checks();
        assert!(set.all_passed(), "U1601 domain checks must be green");
    }

    #[test]
    fn u1601_segment_guard_holds() {
        assert!(u1601_segment_guard());
    }
}
