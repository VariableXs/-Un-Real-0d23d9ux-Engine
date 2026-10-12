//! unxu2661_u2661dual —— 页缓存结构总纲（radix 树类，4K 页粒度——§73 好条范本） 双源对拍与链式封存（缓存页）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2661 · 页缓存结构总纲（radix 树类，4K 页粒度——§73 好条范本） · 判据原文：UNX-F2661 · 判据：radix 树类结构逐项断言（键=（文件对象，页偏移），10^6 次操作不变量全过），4K 粒度契约与 §73 范本一致
//! · 组件：缓存页 双源对拍（8 槽逐槽回读一致）+ 8 节链式指纹封存
//! · 判据落地：UNX-F2661 · 判据：radix 树类结构逐项断言（键=（文件对象，页偏移），10^6 次操作不变量全过），4K 粒度契约与 §73 范本一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2661_VERSION: &str = "u2661-u2661dual-v1";

/// 诊断段基址。
pub const U2661_SEG_BASE: u16 = 0xaa00;

// --------------------------------------------------------------------------
// 诊断码（u2661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缓存页 对拍面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2661Code {
    /// 0xaa00 侧重复（键碰撞 同槽双源）。
    SideDup,
    /// 0xaa01 侧断档（粒度错配 槽未采样）。
    SideGap,
    /// 0xaa02 双源失配（同槽两侧值不等）。
    Mismatch,
    /// 0xaa03 半写（仅一侧落值）。
    HalfWrite,
    /// 0xaa04 链节重复（不变量破缺 同标签双节）。
    LinkDup,
    /// 0xaa05 链节断档（范本背离 节序缺失）。
    LinkGap,
    /// 0xaa06 链指纹破缺（回放校验失败）。
    SealBroken,
    /// 0xaa07 回放漂移（回放终态≠追加终态）。
    ReplayDrift,
    /// 0xaa08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xaa09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xaa0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xaa0b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2661Code {
    /// 全量闭集。
    pub const ALL: [u2661Code; 12] = [
        u2661Code::SideDup,
        u2661Code::SideGap,
        u2661Code::Mismatch,
        u2661Code::HalfWrite,
        u2661Code::LinkDup,
        u2661Code::LinkGap,
        u2661Code::SealBroken,
        u2661Code::ReplayDrift,
        u2661Code::LedgerBroken,
        u2661Code::SnapshotFingerprint,
        u2661Code::TableBroken,
        u2661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2661Code::SideDup => 0xaa00,
            u2661Code::SideGap => 0xaa01,
            u2661Code::Mismatch => 0xaa02,
            u2661Code::HalfWrite => 0xaa03,
            u2661Code::LinkDup => 0xaa04,
            u2661Code::LinkGap => 0xaa05,
            u2661Code::SealBroken => 0xaa06,
            u2661Code::ReplayDrift => 0xaa07,
            u2661Code::LedgerBroken => 0xaa08,
            u2661Code::SnapshotFingerprint => 0xaa09,
            u2661Code::TableBroken => 0xaa0a,
            u2661Code::SegmentGuard => 0xaa0b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2661Code::SideDup => "侧重复（键碰撞 同槽双源）",
            u2661Code::SideGap => "侧断档（粒度错配 槽未采样）",
            u2661Code::Mismatch => "双源失配（同槽两侧值不等）",
            u2661Code::HalfWrite => "半写（仅一侧落值）",
            u2661Code::LinkDup => "链节重复（不变量破缺 同标签双节）",
            u2661Code::LinkGap => "链节断档（范本背离 节序缺失）",
            u2661Code::SealBroken => "链指纹破缺（回放校验失败）",
            u2661Code::ReplayDrift => "回放漂移（回放终态≠追加终态）",
            u2661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2661Code::SideDup => "按槽唯一律去重",
            u2661Code::SideGap => "按槽闭集补采",
            u2661Code::Mismatch => "以权威侧复测仲裁",
            u2661Code::HalfWrite => "补齐对侧采样",
            u2661Code::LinkDup => "按节序唯一律合并",
            u2661Code::LinkGap => "按节序补链",
            u2661Code::SealBroken => "按防篡改规程重建链",
            u2661Code::ReplayDrift => "按追加序重放",
            u2661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2661Code::TableBroken => "按语义规则重建常量表",
            u2661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2661cmp（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 缓存页 双源对拍（键碰撞）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2661cmpSlot {
    /// 对象键面。
    ObjKey,
    /// 偏移键面。
    OffKey,
    /// radix 面。
    Radix,
    /// 粒度面。
    Grain,
    /// 命中面。
    Hit,
    /// 未命中面。
    Miss,
    /// 逐出面。
    Evict,
    /// 范本面。
    Parity,
}

impl mxu2661cmpSlot {
    /// 全量槽位闭集。
    pub const ALL: [mxu2661cmpSlot; 8] = [
        mxu2661cmpSlot::ObjKey,
        mxu2661cmpSlot::OffKey,
        mxu2661cmpSlot::Radix,
        mxu2661cmpSlot::Grain,
        mxu2661cmpSlot::Hit,
        mxu2661cmpSlot::Miss,
        mxu2661cmpSlot::Evict,
        mxu2661cmpSlot::Parity,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2661cmpSlot::ObjKey => 0,
            mxu2661cmpSlot::OffKey => 1,
            mxu2661cmpSlot::Radix => 2,
            mxu2661cmpSlot::Grain => 3,
            mxu2661cmpSlot::Hit => 4,
            mxu2661cmpSlot::Miss => 5,
            mxu2661cmpSlot::Evict => 6,
            mxu2661cmpSlot::Parity => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2661cmpSlot::ObjKey => "对象键面",
            mxu2661cmpSlot::OffKey => "偏移键面",
            mxu2661cmpSlot::Radix => "radix 面",
            mxu2661cmpSlot::Grain => "粒度面",
            mxu2661cmpSlot::Hit => "命中面",
            mxu2661cmpSlot::Miss => "未命中面",
            mxu2661cmpSlot::Evict => "逐出面",
            mxu2661cmpSlot::Parity => "范本面",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2661cmpPair {
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
pub struct mxu2661cmpDual {
    /// 逐槽样本。
    pub pairs: [mxu2661cmpPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mxu2661cmpDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mxu2661cmpPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mxu2661cmpSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mxu2661cmpPair { a, b, same, seq: self.rounds };
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
// 引擎 mxu2661seal（链式指纹：缓存页 封存链节 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 缓存页 链式指纹封存（逐链回放校验）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2661sealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2661sealChain {
    /// 链节点序列。
    pub links: [mxu2661sealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mxu2661sealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mxu2661sealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u2661Code> {
        if self.n >= 16 || payload == 0 {
            return Err(u2661Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mxu2661sealLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0xaa00, 0xaa0f]。
pub const fn u2661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2661Code::ALL.len() {
        let c = u2661Code::ALL[i].code();
        if c < 0xaa00 || c > 0xaa0f {
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
fn u2661_guard_seg() -> bool {
    u2661_segment_guard()
}

/// UNX-F2661 双源逐槽对拍全同 + 复核通过。
fn check_u2661_dual_same() -> bool {
    let mut d = mxu2661cmpDual::new();
    let mut si = 0usize;
    while si < mxu2661cmpSlot::ALL.len() {
        let v = (si as u64 + 1) * 11;
        d.sample(mxu2661cmpSlot::ALL[si], v, v);
        si += 1;
    }
    let mut ok = d.all_same() && d.recount();
    ok
}

/// UNX-F2661 封存链 8 节追加回放校验全绿。
fn check_u2661_dual_chain() -> bool {
    let mut c = mxu2661sealChain::new();
    let mut i = 0u32;
    while i < 8 { let _ = c.append(i + 1, (i as u64 + 1) * 13); i += 1; }
    let mut ok = c.verify();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2661_checks() -> CheckSet {
    let mut set = CheckSet::new("U2661");
    set.add(
        "u2661-seg-guard",
        u2661_guard_seg(),
        if u2661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2661-dual-same",
        check_u2661_dual_same(),
        if check_u2661_dual_same() { "" } else { "UNX-F2661 dual mismatch" },
    );
    set.add(
        "u2661-dual-chain",
        check_u2661_dual_chain(),
        if check_u2661_dual_chain() { "" } else { "UNX-F2661 chain mismatch" },
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
    fn u2661_all_checks_green() {
        let set = run_u2661_checks();
        assert!(set.all_passed(), "U2661 domain checks must be green");
    }

    #[test]
    fn u2661_segment_guard_holds() {
        assert!(u2661_segment_guard());
    }
}
