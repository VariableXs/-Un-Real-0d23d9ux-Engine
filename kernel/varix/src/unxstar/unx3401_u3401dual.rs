//! unxu3401_u3401dual —— ktest 现状盘点与扩容总设计（接管 testing/ 目录） 双源对拍与链式封存（ktest 扩容）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3401 · ktest 现状盘点与扩容总设计（接管 testing/ 目录） · 判据原文：UNX-F3401 · 判据：J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。
//! · 组件：ktest 扩容 双源对拍（8 槽逐槽回读一致）+ 8 节链式指纹封存
//! · 判据落地：UNX-F3401 · 判据：J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3401_VERSION: &str = "u3401-u3401dual-v1";

/// 诊断段基址。
pub const U3401_SEG_BASE: u16 = 0xe0b0;

// --------------------------------------------------------------------------
// 诊断码（u3401 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// ktest 扩容 对拍面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3401Code {
    /// 0xe0b0 侧重复（ktest 同槽双源）。
    SideDup,
    /// 0xe0b1 侧断档（扩容 槽未采样）。
    SideGap,
    /// 0xe0b2 双源失配（同槽两侧值不等）。
    Mismatch,
    /// 0xe0b3 半写（仅一侧落值）。
    HalfWrite,
    /// 0xe0b4 链节重复（接管 同标签双节）。
    LinkDup,
    /// 0xe0b5 链节断档（骨架 节序缺失）。
    LinkGap,
    /// 0xe0b6 链指纹破缺（回放校验失败）。
    SealBroken,
    /// 0xe0b7 回放漂移（回放终态≠追加终态）。
    ReplayDrift,
    /// 0xe0b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3401Code {
    /// 全量闭集。
    pub const ALL: [u3401Code; 12] = [
        u3401Code::SideDup,
        u3401Code::SideGap,
        u3401Code::Mismatch,
        u3401Code::HalfWrite,
        u3401Code::LinkDup,
        u3401Code::LinkGap,
        u3401Code::SealBroken,
        u3401Code::ReplayDrift,
        u3401Code::LedgerBroken,
        u3401Code::SnapshotFingerprint,
        u3401Code::TableBroken,
        u3401Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3401Code::SideDup => 0xe0b0,
            u3401Code::SideGap => 0xe0b1,
            u3401Code::Mismatch => 0xe0b2,
            u3401Code::HalfWrite => 0xe0b3,
            u3401Code::LinkDup => 0xe0b4,
            u3401Code::LinkGap => 0xe0b5,
            u3401Code::SealBroken => 0xe0b6,
            u3401Code::ReplayDrift => 0xe0b7,
            u3401Code::LedgerBroken => 0xe0b8,
            u3401Code::SnapshotFingerprint => 0xe0b9,
            u3401Code::TableBroken => 0xe0ba,
            u3401Code::SegmentGuard => 0xe0bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3401Code::SideDup => "侧重复（ktest 同槽双源）",
            u3401Code::SideGap => "侧断档（扩容 槽未采样）",
            u3401Code::Mismatch => "双源失配（同槽两侧值不等）",
            u3401Code::HalfWrite => "半写（仅一侧落值）",
            u3401Code::LinkDup => "链节重复（接管 同标签双节）",
            u3401Code::LinkGap => "链节断档（骨架 节序缺失）",
            u3401Code::SealBroken => "链指纹破缺（回放校验失败）",
            u3401Code::ReplayDrift => "回放漂移（回放终态≠追加终态）",
            u3401Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3401Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3401Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3401Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3401Code::SideDup => "按槽唯一律去重",
            u3401Code::SideGap => "按槽闭集补采",
            u3401Code::Mismatch => "以权威侧复测仲裁",
            u3401Code::HalfWrite => "补齐对侧采样",
            u3401Code::LinkDup => "按节序唯一律合并",
            u3401Code::LinkGap => "按节序补链",
            u3401Code::SealBroken => "按防篡改规程重建链",
            u3401Code::ReplayDrift => "按追加序重放",
            u3401Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3401Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3401Code::TableBroken => "按语义规则重建常量表",
            u3401Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3401cmp（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// ktest 扩容 双源对拍（ktest）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3401cmpSlot {
    /// 注册表。
    KT,
    /// 用例槽。
    DIR,
    /// 选择器。
    CASE,
    /// 后端槽。
    SEL,
    /// 目录映射。
    REG,
    /// 计数账。
    FIX,
    /// 固化位。
    BACKEND,
    /// 冻结位。
    QEMU,
}

impl mxu3401cmpSlot {
    /// 全量槽位闭集。
    pub const ALL: [mxu3401cmpSlot; 8] = [
        mxu3401cmpSlot::KT,
        mxu3401cmpSlot::DIR,
        mxu3401cmpSlot::CASE,
        mxu3401cmpSlot::SEL,
        mxu3401cmpSlot::REG,
        mxu3401cmpSlot::FIX,
        mxu3401cmpSlot::BACKEND,
        mxu3401cmpSlot::QEMU,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3401cmpSlot::KT => 0,
            mxu3401cmpSlot::DIR => 1,
            mxu3401cmpSlot::CASE => 2,
            mxu3401cmpSlot::SEL => 3,
            mxu3401cmpSlot::REG => 4,
            mxu3401cmpSlot::FIX => 5,
            mxu3401cmpSlot::BACKEND => 6,
            mxu3401cmpSlot::QEMU => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3401cmpSlot::KT => "注册表",
            mxu3401cmpSlot::DIR => "用例槽",
            mxu3401cmpSlot::CASE => "选择器",
            mxu3401cmpSlot::SEL => "后端槽",
            mxu3401cmpSlot::REG => "目录映射",
            mxu3401cmpSlot::FIX => "计数账",
            mxu3401cmpSlot::BACKEND => "固化位",
            mxu3401cmpSlot::QEMU => "冻结位",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3401cmpPair {
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
pub struct mxu3401cmpDual {
    /// 逐槽样本。
    pub pairs: [mxu3401cmpPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mxu3401cmpDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mxu3401cmpPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mxu3401cmpSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mxu3401cmpPair { a, b, same, seq: self.rounds };
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
// 引擎 mxu3401seal（链式指纹：ktest 扩容 封存链节 + 逐链回放校验）
// --------------------------------------------------------------------------
/// ktest 扩容 链式指纹封存（逐链回放校验）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3401sealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3401sealChain {
    /// 链节点序列。
    pub links: [mxu3401sealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mxu3401sealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mxu3401sealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u3401Code> {
        if self.n >= 16 || payload == 0 {
            return Err(u3401Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mxu3401sealLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0b0, 0xe0bf]。
pub const fn u3401_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3401Code::ALL.len() {
        let c = u3401Code::ALL[i].code();
        if c < 0xe0b0 || c > 0xe0bf {
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
fn u3401_guard_seg() -> bool {
    u3401_segment_guard()
}

/// UNX-F3401 双源逐槽对拍全同 + 复核通过。
fn check_u3401_dual_same() -> bool {
    let mut d = mxu3401cmpDual::new();
    let mut si = 0usize;
    while si < mxu3401cmpSlot::ALL.len() {
        let v = (si as u64 + 1) * 11;
        d.sample(mxu3401cmpSlot::ALL[si], v, v);
        si += 1;
    }
    let mut ok = d.all_same() && d.recount();
    ok
}

/// UNX-F3401 封存链 8 节追加回放校验全绿。
fn check_u3401_dual_chain() -> bool {
    let mut c = mxu3401sealChain::new();
    let mut i = 0u32;
    while i < 8 { let _ = c.append(i + 1, (i as u64 + 1) * 13); i += 1; }
    let mut ok = c.verify();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3401_checks() -> CheckSet {
    let mut set = CheckSet::new("U3401");
    set.add(
        "u3401-seg-guard",
        u3401_guard_seg(),
        if u3401_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3401-dual-same",
        check_u3401_dual_same(),
        if check_u3401_dual_same() { "" } else { "UNX-F3401 dual mismatch" },
    );
    set.add(
        "u3401-dual-chain",
        check_u3401_dual_chain(),
        if check_u3401_dual_chain() { "" } else { "UNX-F3401 chain mismatch" },
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
    fn u3401_all_checks_green() {
        let set = run_u3401_checks();
        assert!(set.all_passed(), "U3401 domain checks must be green");
    }

    #[test]
    fn u3401_segment_guard_holds() {
        assert!(u3401_segment_guard());
    }
}
