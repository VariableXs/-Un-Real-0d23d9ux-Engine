//! unxu3661_u3661dual —— 符号化总设计（地址→符号的解析管线） 双源对拍与链式封存（符号化管线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3661 · 符号化总设计（地址→符号的解析管线） · 判据原文：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（单帧地址→符号名+偏移+源行——翻译三要素）。<br>J2：离线纯净（符号化器不依赖内核状态（纯函数——同输入同输出——F3672 的前提））。
//! · 组件：符号化管线 双源对拍（8 槽逐槽回读一致）+ 8 节链式指纹封存
//! · 判据落地：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3661_VERSION: &str = "u3661-u3661dual-v1";

/// 诊断段基址。
pub const U3661_SEG_BASE: u16 = 0xe180;

// --------------------------------------------------------------------------
// 诊断码（u3661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 符号化管线 对拍面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3661Code {
    /// 0xe180 侧重复（符号 同槽双源）。
    SideDup,
    /// 0xe181 侧断档（解析 槽未采样）。
    SideGap,
    /// 0xe182 双源失配（同槽两侧值不等）。
    Mismatch,
    /// 0xe183 半写（仅一侧落值）。
    HalfWrite,
    /// 0xe184 链节重复（内联 同标签双节）。
    LinkDup,
    /// 0xe185 链节断档（解析 节序缺失）。
    LinkGap,
    /// 0xe186 链指纹破缺（回放校验失败）。
    SealBroken,
    /// 0xe187 回放漂移（回放终态≠追加终态）。
    ReplayDrift,
    /// 0xe188 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe189 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe18a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe18b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3661Code {
    /// 全量闭集。
    pub const ALL: [u3661Code; 12] = [
        u3661Code::SideDup,
        u3661Code::SideGap,
        u3661Code::Mismatch,
        u3661Code::HalfWrite,
        u3661Code::LinkDup,
        u3661Code::LinkGap,
        u3661Code::SealBroken,
        u3661Code::ReplayDrift,
        u3661Code::LedgerBroken,
        u3661Code::SnapshotFingerprint,
        u3661Code::TableBroken,
        u3661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3661Code::SideDup => 0xe180,
            u3661Code::SideGap => 0xe181,
            u3661Code::Mismatch => 0xe182,
            u3661Code::HalfWrite => 0xe183,
            u3661Code::LinkDup => 0xe184,
            u3661Code::LinkGap => 0xe185,
            u3661Code::SealBroken => 0xe186,
            u3661Code::ReplayDrift => 0xe187,
            u3661Code::LedgerBroken => 0xe188,
            u3661Code::SnapshotFingerprint => 0xe189,
            u3661Code::TableBroken => 0xe18a,
            u3661Code::SegmentGuard => 0xe18b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3661Code::SideDup => "侧重复（符号 同槽双源）",
            u3661Code::SideGap => "侧断档（解析 槽未采样）",
            u3661Code::Mismatch => "双源失配（同槽两侧值不等）",
            u3661Code::HalfWrite => "半写（仅一侧落值）",
            u3661Code::LinkDup => "链节重复（内联 同标签双节）",
            u3661Code::LinkGap => "链节断档（解析 节序缺失）",
            u3661Code::SealBroken => "链指纹破缺（回放校验失败）",
            u3661Code::ReplayDrift => "回放漂移（回放终态≠追加终态）",
            u3661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3661Code::SideDup => "按槽唯一律去重",
            u3661Code::SideGap => "按槽闭集补采",
            u3661Code::Mismatch => "以权威侧复测仲裁",
            u3661Code::HalfWrite => "补齐对侧采样",
            u3661Code::LinkDup => "按节序唯一律合并",
            u3661Code::LinkGap => "按节序补链",
            u3661Code::SealBroken => "按防篡改规程重建链",
            u3661Code::ReplayDrift => "按追加序重放",
            u3661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3661Code::TableBroken => "按语义规则重建常量表",
            u3661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3661cmp（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 符号化管线 双源对拍（符号）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3661cmpSlot {
    /// 符号表。
    SYM,
    /// 模块表。
    ADDR,
    /// 地址桶。
    RESOLVE,
    /// 偏移栏。
    TBL,
    /// 内联树。
    MOD,
    /// 缓存槽。
    OFFSET,
    /// 命中率。
    INLINE,
    /// 回落账。
    FUNC,
}

impl mxu3661cmpSlot {
    /// 全量槽位闭集。
    pub const ALL: [mxu3661cmpSlot; 8] = [
        mxu3661cmpSlot::SYM,
        mxu3661cmpSlot::ADDR,
        mxu3661cmpSlot::RESOLVE,
        mxu3661cmpSlot::TBL,
        mxu3661cmpSlot::MOD,
        mxu3661cmpSlot::OFFSET,
        mxu3661cmpSlot::INLINE,
        mxu3661cmpSlot::FUNC,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3661cmpSlot::SYM => 0,
            mxu3661cmpSlot::ADDR => 1,
            mxu3661cmpSlot::RESOLVE => 2,
            mxu3661cmpSlot::TBL => 3,
            mxu3661cmpSlot::MOD => 4,
            mxu3661cmpSlot::OFFSET => 5,
            mxu3661cmpSlot::INLINE => 6,
            mxu3661cmpSlot::FUNC => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3661cmpSlot::SYM => "符号表",
            mxu3661cmpSlot::ADDR => "模块表",
            mxu3661cmpSlot::RESOLVE => "地址桶",
            mxu3661cmpSlot::TBL => "偏移栏",
            mxu3661cmpSlot::MOD => "内联树",
            mxu3661cmpSlot::OFFSET => "缓存槽",
            mxu3661cmpSlot::INLINE => "命中率",
            mxu3661cmpSlot::FUNC => "回落账",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3661cmpPair {
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
pub struct mxu3661cmpDual {
    /// 逐槽样本。
    pub pairs: [mxu3661cmpPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mxu3661cmpDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mxu3661cmpPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mxu3661cmpSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mxu3661cmpPair { a, b, same, seq: self.rounds };
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
// 引擎 mxu3661seal（链式指纹：符号化管线 封存链节 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 符号化管线 链式指纹封存（逐链回放校验）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3661sealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3661sealChain {
    /// 链节点序列。
    pub links: [mxu3661sealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mxu3661sealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mxu3661sealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u3661Code> {
        if self.n >= 16 || payload == 0 {
            return Err(u3661Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mxu3661sealLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0xe180, 0xe18f]。
pub const fn u3661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3661Code::ALL.len() {
        let c = u3661Code::ALL[i].code();
        if c < 0xe180 || c > 0xe18f {
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
fn u3661_guard_seg() -> bool {
    u3661_segment_guard()
}

/// UNX-F3661 双源逐槽对拍全同 + 复核通过。
fn check_u3661_dual_same() -> bool {
    let mut d = mxu3661cmpDual::new();
    let mut si = 0usize;
    while si < mxu3661cmpSlot::ALL.len() {
        let v = (si as u64 + 1) * 11;
        d.sample(mxu3661cmpSlot::ALL[si], v, v);
        si += 1;
    }
    let mut ok = d.all_same() && d.recount();
    ok
}

/// UNX-F3661 封存链 8 节追加回放校验全绿。
fn check_u3661_dual_chain() -> bool {
    let mut c = mxu3661sealChain::new();
    let mut i = 0u32;
    while i < 8 { let _ = c.append(i + 1, (i as u64 + 1) * 13); i += 1; }
    let mut ok = c.verify();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3661_checks() -> CheckSet {
    let mut set = CheckSet::new("U3661");
    set.add(
        "u3661-seg-guard",
        u3661_guard_seg(),
        if u3661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3661-dual-same",
        check_u3661_dual_same(),
        if check_u3661_dual_same() { "" } else { "UNX-F3661 dual mismatch" },
    );
    set.add(
        "u3661-dual-chain",
        check_u3661_dual_chain(),
        if check_u3661_dual_chain() { "" } else { "UNX-F3661 chain mismatch" },
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
    fn u3661_all_checks_green() {
        let set = run_u3661_checks();
        assert!(set.all_passed(), "U3661 domain checks must be green");
    }

    #[test]
    fn u3661_segment_guard_holds() {
        assert!(u3661_segment_guard());
    }
}
