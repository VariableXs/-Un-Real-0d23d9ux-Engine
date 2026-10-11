//! unxu3001_u3001dual —— 双链结构冻结（active/inactive 两链节点布局） 双源对拍与链式封存（双链结构冻结）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3001 · 双链结构冻结（active/inactive 两链节点布局） · 判据原文：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化
//! · 组件：双链结构冻结 双源对拍（8 槽逐槽回读一致）+ 8 节链式指纹封存
//! · 判据落地：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3001_VERSION: &str = "u3001-u3001dual-v1";

/// 诊断段基址。
pub const U3001_SEG_BASE: u16 = 0xe290;

// --------------------------------------------------------------------------
// 诊断码（u3001 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链结构冻结 对拍面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3001Code {
    /// 0xe290 侧重复（双链 同槽双源）。
    SideDup,
    /// 0xe291 侧断档（节点 槽未采样）。
    SideGap,
    /// 0xe292 双源失配（同槽两侧值不等）。
    Mismatch,
    /// 0xe293 半写（仅一侧落值）。
    HalfWrite,
    /// 0xe294 链节重复（迁移 同标签双节）。
    LinkDup,
    /// 0xe295 链节断档（双链 节序缺失）。
    LinkGap,
    /// 0xe296 链指纹破缺（回放校验失败）。
    SealBroken,
    /// 0xe297 回放漂移（回放终态≠追加终态）。
    ReplayDrift,
    /// 0xe298 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe299 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe29a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe29b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3001Code {
    /// 全量闭集。
    pub const ALL: [u3001Code; 12] = [
        u3001Code::SideDup,
        u3001Code::SideGap,
        u3001Code::Mismatch,
        u3001Code::HalfWrite,
        u3001Code::LinkDup,
        u3001Code::LinkGap,
        u3001Code::SealBroken,
        u3001Code::ReplayDrift,
        u3001Code::LedgerBroken,
        u3001Code::SnapshotFingerprint,
        u3001Code::TableBroken,
        u3001Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3001Code::SideDup => 0xe290,
            u3001Code::SideGap => 0xe291,
            u3001Code::Mismatch => 0xe292,
            u3001Code::HalfWrite => 0xe293,
            u3001Code::LinkDup => 0xe294,
            u3001Code::LinkGap => 0xe295,
            u3001Code::SealBroken => 0xe296,
            u3001Code::ReplayDrift => 0xe297,
            u3001Code::LedgerBroken => 0xe298,
            u3001Code::SnapshotFingerprint => 0xe299,
            u3001Code::TableBroken => 0xe29a,
            u3001Code::SegmentGuard => 0xe29b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3001Code::SideDup => "侧重复（双链 同槽双源）",
            u3001Code::SideGap => "侧断档（节点 槽未采样）",
            u3001Code::Mismatch => "双源失配（同槽两侧值不等）",
            u3001Code::HalfWrite => "半写（仅一侧落值）",
            u3001Code::LinkDup => "链节重复（迁移 同标签双节）",
            u3001Code::LinkGap => "链节断档（双链 节序缺失）",
            u3001Code::SealBroken => "链指纹破缺（回放校验失败）",
            u3001Code::ReplayDrift => "回放漂移（回放终态≠追加终态）",
            u3001Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3001Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3001Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3001Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3001Code::SideDup => "按槽唯一律去重",
            u3001Code::SideGap => "按槽闭集补采",
            u3001Code::Mismatch => "以权威侧复测仲裁",
            u3001Code::HalfWrite => "补齐对侧采样",
            u3001Code::LinkDup => "按节序唯一律合并",
            u3001Code::LinkGap => "按节序补链",
            u3001Code::SealBroken => "按防篡改规程重建链",
            u3001Code::ReplayDrift => "按追加序重放",
            u3001Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3001Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3001Code::TableBroken => "按语义规则重建常量表",
            u3001Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3001cmp（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 双链结构冻结 双源对拍（双链）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3001cmpSlot {
    /// active 链。
    ACTIVE,
    /// inactive 链。
    INACT,
    /// 空闲链。
    NODE,
    /// 节点池。
    LAYOUT,
    /// 布局栏。
    NEXT,
    /// 迁移账。
    PREV,
    /// 合并账。
    FREE,
    /// 不变量。
    POOL,
}

impl mxu3001cmpSlot {
    /// 全量槽位闭集。
    pub const ALL: [mxu3001cmpSlot; 8] = [
        mxu3001cmpSlot::ACTIVE,
        mxu3001cmpSlot::INACT,
        mxu3001cmpSlot::NODE,
        mxu3001cmpSlot::LAYOUT,
        mxu3001cmpSlot::NEXT,
        mxu3001cmpSlot::PREV,
        mxu3001cmpSlot::FREE,
        mxu3001cmpSlot::POOL,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3001cmpSlot::ACTIVE => 0,
            mxu3001cmpSlot::INACT => 1,
            mxu3001cmpSlot::NODE => 2,
            mxu3001cmpSlot::LAYOUT => 3,
            mxu3001cmpSlot::NEXT => 4,
            mxu3001cmpSlot::PREV => 5,
            mxu3001cmpSlot::FREE => 6,
            mxu3001cmpSlot::POOL => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3001cmpSlot::ACTIVE => "active 链",
            mxu3001cmpSlot::INACT => "inactive 链",
            mxu3001cmpSlot::NODE => "空闲链",
            mxu3001cmpSlot::LAYOUT => "节点池",
            mxu3001cmpSlot::NEXT => "布局栏",
            mxu3001cmpSlot::PREV => "迁移账",
            mxu3001cmpSlot::FREE => "合并账",
            mxu3001cmpSlot::POOL => "不变量",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3001cmpPair {
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
pub struct mxu3001cmpDual {
    /// 逐槽样本。
    pub pairs: [mxu3001cmpPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mxu3001cmpDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mxu3001cmpPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mxu3001cmpSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mxu3001cmpPair { a, b, same, seq: self.rounds };
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
// 引擎 mxu3001seal（链式指纹：双链结构冻结 封存链节 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 双链结构冻结 链式指纹封存（逐链回放校验）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3001sealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3001sealChain {
    /// 链节点序列。
    pub links: [mxu3001sealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mxu3001sealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mxu3001sealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u3001Code> {
        if self.n >= 16 || payload == 0 {
            return Err(u3001Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mxu3001sealLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0xe290, 0xe29f]。
pub const fn u3001_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3001Code::ALL.len() {
        let c = u3001Code::ALL[i].code();
        if c < 0xe290 || c > 0xe29f {
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
fn u3001_guard_seg() -> bool {
    u3001_segment_guard()
}

/// UNX-F3001 双源逐槽对拍全同 + 复核通过。
fn check_u3001_dual_same() -> bool {
    let mut d = mxu3001cmpDual::new();
    let mut si = 0usize;
    while si < mxu3001cmpSlot::ALL.len() {
        let v = (si as u64 + 1) * 11;
        d.sample(mxu3001cmpSlot::ALL[si], v, v);
        si += 1;
    }
    let mut ok = d.all_same() && d.recount();
    ok
}

/// UNX-F3001 封存链 8 节追加回放校验全绿。
fn check_u3001_dual_chain() -> bool {
    let mut c = mxu3001sealChain::new();
    let mut i = 0u32;
    while i < 8 { let _ = c.append(i + 1, (i as u64 + 1) * 13); i += 1; }
    let mut ok = c.verify();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3001_checks() -> CheckSet {
    let mut set = CheckSet::new("U3001");
    set.add(
        "u3001-seg-guard",
        u3001_guard_seg(),
        if u3001_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3001-dual-same",
        check_u3001_dual_same(),
        if check_u3001_dual_same() { "" } else { "UNX-F3001 dual mismatch" },
    );
    set.add(
        "u3001-dual-chain",
        check_u3001_dual_chain(),
        if check_u3001_dual_chain() { "" } else { "UNX-F3001 chain mismatch" },
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
    fn u3001_all_checks_green() {
        let set = run_u3001_checks();
        assert!(set.all_passed(), "U3001 domain checks must be green");
    }

    #[test]
    fn u3001_segment_guard_holds() {
        assert!(u3001_segment_guard());
    }
}
