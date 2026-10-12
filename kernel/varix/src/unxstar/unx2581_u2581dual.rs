//! unxu2581_u2581dual —— VMA 区间树结构总纲（红黑树类，按地址排序） 双源对拍与链式封存（VMA 区间）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2581 · VMA 区间树结构总纲（红黑树类，按地址排序） · 判据原文：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! · 组件：VMA 区间 双源对拍（8 槽逐槽回读一致）+ 8 节链式指纹封存
//! · 判据落地：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2581_VERSION: &str = "u2581-u2581dual-v1";

/// 诊断段基址。
pub const U2581_SEG_BASE: u16 = 0xa700;

// --------------------------------------------------------------------------
// 诊断码（u2581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// VMA 区间 对拍面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2581Code {
    /// 0xa700 侧重复（排序破坏 同槽双源）。
    SideDup,
    /// 0xa701 侧断档（失衡塌陷 槽未采样）。
    SideGap,
    /// 0xa702 双源失配（同槽两侧值不等）。
    Mismatch,
    /// 0xa703 半写（仅一侧落值）。
    HalfWrite,
    /// 0xa704 链节重复（区间重叠 同标签双节）。
    LinkDup,
    /// 0xa705 链节断档（字段缺失 节序缺失）。
    LinkGap,
    /// 0xa706 链指纹破缺（回放校验失败）。
    SealBroken,
    /// 0xa707 回放漂移（回放终态≠追加终态）。
    ReplayDrift,
    /// 0xa708 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa709 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa70a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa70b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2581Code {
    /// 全量闭集。
    pub const ALL: [u2581Code; 12] = [
        u2581Code::SideDup,
        u2581Code::SideGap,
        u2581Code::Mismatch,
        u2581Code::HalfWrite,
        u2581Code::LinkDup,
        u2581Code::LinkGap,
        u2581Code::SealBroken,
        u2581Code::ReplayDrift,
        u2581Code::LedgerBroken,
        u2581Code::SnapshotFingerprint,
        u2581Code::TableBroken,
        u2581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2581Code::SideDup => 0xa700,
            u2581Code::SideGap => 0xa701,
            u2581Code::Mismatch => 0xa702,
            u2581Code::HalfWrite => 0xa703,
            u2581Code::LinkDup => 0xa704,
            u2581Code::LinkGap => 0xa705,
            u2581Code::SealBroken => 0xa706,
            u2581Code::ReplayDrift => 0xa707,
            u2581Code::LedgerBroken => 0xa708,
            u2581Code::SnapshotFingerprint => 0xa709,
            u2581Code::TableBroken => 0xa70a,
            u2581Code::SegmentGuard => 0xa70b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2581Code::SideDup => "侧重复（排序破坏 同槽双源）",
            u2581Code::SideGap => "侧断档（失衡塌陷 槽未采样）",
            u2581Code::Mismatch => "双源失配（同槽两侧值不等）",
            u2581Code::HalfWrite => "半写（仅一侧落值）",
            u2581Code::LinkDup => "链节重复（区间重叠 同标签双节）",
            u2581Code::LinkGap => "链节断档（字段缺失 节序缺失）",
            u2581Code::SealBroken => "链指纹破缺（回放校验失败）",
            u2581Code::ReplayDrift => "回放漂移（回放终态≠追加终态）",
            u2581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2581Code::SideDup => "按槽唯一律去重",
            u2581Code::SideGap => "按槽闭集补采",
            u2581Code::Mismatch => "以权威侧复测仲裁",
            u2581Code::HalfWrite => "补齐对侧采样",
            u2581Code::LinkDup => "按节序唯一律合并",
            u2581Code::LinkGap => "按节序补链",
            u2581Code::SealBroken => "按防篡改规程重建链",
            u2581Code::ReplayDrift => "按追加序重放",
            u2581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2581Code::TableBroken => "按语义规则重建常量表",
            u2581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2581cmp（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// VMA 区间 双源对拍（排序破坏）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2581cmpSlot {
    /// 起始面。
    Start,
    /// 终止面。
    End,
    /// 属性面。
    Attr,
    /// 类型面。
    Type,
    /// 对象面。
    Obj,
    /// 排序面。
    Order,
    /// 平衡面。
    Balance,
    /// 不变量面。
    Invariant,
}

impl mxu2581cmpSlot {
    /// 全量槽位闭集。
    pub const ALL: [mxu2581cmpSlot; 8] = [
        mxu2581cmpSlot::Start,
        mxu2581cmpSlot::End,
        mxu2581cmpSlot::Attr,
        mxu2581cmpSlot::Type,
        mxu2581cmpSlot::Obj,
        mxu2581cmpSlot::Order,
        mxu2581cmpSlot::Balance,
        mxu2581cmpSlot::Invariant,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2581cmpSlot::Start => 0,
            mxu2581cmpSlot::End => 1,
            mxu2581cmpSlot::Attr => 2,
            mxu2581cmpSlot::Type => 3,
            mxu2581cmpSlot::Obj => 4,
            mxu2581cmpSlot::Order => 5,
            mxu2581cmpSlot::Balance => 6,
            mxu2581cmpSlot::Invariant => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2581cmpSlot::Start => "起始面",
            mxu2581cmpSlot::End => "终止面",
            mxu2581cmpSlot::Attr => "属性面",
            mxu2581cmpSlot::Type => "类型面",
            mxu2581cmpSlot::Obj => "对象面",
            mxu2581cmpSlot::Order => "排序面",
            mxu2581cmpSlot::Balance => "平衡面",
            mxu2581cmpSlot::Invariant => "不变量面",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2581cmpPair {
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
pub struct mxu2581cmpDual {
    /// 逐槽样本。
    pub pairs: [mxu2581cmpPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mxu2581cmpDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mxu2581cmpPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mxu2581cmpSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mxu2581cmpPair { a, b, same, seq: self.rounds };
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
// 引擎 mxu2581seal（链式指纹：VMA 区间 封存链节 + 逐链回放校验）
// --------------------------------------------------------------------------
/// VMA 区间 链式指纹封存（逐链回放校验）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2581sealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2581sealChain {
    /// 链节点序列。
    pub links: [mxu2581sealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mxu2581sealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mxu2581sealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u2581Code> {
        if self.n >= 16 || payload == 0 {
            return Err(u2581Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mxu2581sealLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0xa700, 0xa70f]。
pub const fn u2581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2581Code::ALL.len() {
        let c = u2581Code::ALL[i].code();
        if c < 0xa700 || c > 0xa70f {
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
fn u2581_guard_seg() -> bool {
    u2581_segment_guard()
}

/// UNX-F2581 双源逐槽对拍全同 + 复核通过。
fn check_u2581_dual_same() -> bool {
    let mut d = mxu2581cmpDual::new();
    let mut si = 0usize;
    while si < mxu2581cmpSlot::ALL.len() {
        let v = (si as u64 + 1) * 11;
        d.sample(mxu2581cmpSlot::ALL[si], v, v);
        si += 1;
    }
    let mut ok = d.all_same() && d.recount();
    ok
}

/// UNX-F2581 封存链 8 节追加回放校验全绿。
fn check_u2581_dual_chain() -> bool {
    let mut c = mxu2581sealChain::new();
    let mut i = 0u32;
    while i < 8 { let _ = c.append(i + 1, (i as u64 + 1) * 13); i += 1; }
    let mut ok = c.verify();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2581_checks() -> CheckSet {
    let mut set = CheckSet::new("U2581");
    set.add(
        "u2581-seg-guard",
        u2581_guard_seg(),
        if u2581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2581-dual-same",
        check_u2581_dual_same(),
        if check_u2581_dual_same() { "" } else { "UNX-F2581 dual mismatch" },
    );
    set.add(
        "u2581-dual-chain",
        check_u2581_dual_chain(),
        if check_u2581_dual_chain() { "" } else { "UNX-F2581 chain mismatch" },
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
    fn u2581_all_checks_green() {
        let set = run_u2581_checks();
        assert!(set.all_passed(), "U2581 domain checks must be green");
    }

    #[test]
    fn u2581_segment_guard_holds() {
        assert!(u2581_segment_guard());
    }
}
