//! unxu3781_u3781dual —— 域中期收口总设计（B20 前向声明的兑现面） 双源对拍与链式封存（域中期收口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3781 · 域中期收口总设计（B20 前向声明的兑现面） · 判据原文：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F3781 域中期收口：段断言大关 ≥80+五条件移交"——期票与兑付跨十批的闭链）；收口的范围声明（B16–B30 十五批——A5 域续领波的全域——不含 B01–B15（已在前次 finalize 收口——历史账冻结）；M-539 收口预跑（三总轧空转→流程通→正式总轧前演练）。<br>J2：收口的纪律（收口只核不改（发现的偏差走修正流程+留痕——收口不涂改历史账。
//! · 组件：域中期收口 双源对拍（8 槽逐槽回读一致）+ 8 节链式指纹封存
//! · 判据落地：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3781_VERSION: &str = "u3781-u3781dual-v1";

/// 诊断段基址。
pub const U3781_SEG_BASE: u16 = 0xe1e0;

// --------------------------------------------------------------------------
// 诊断码（u3781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域中期收口 对拍面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3781Code {
    /// 0xe1e0 侧重复（收口 同槽双源）。
    SideDup,
    /// 0xe1e1 侧断档（兑现 槽未采样）。
    SideGap,
    /// 0xe1e2 双源失配（同槽两侧值不等）。
    Mismatch,
    /// 0xe1e3 半写（仅一侧落值）。
    HalfWrite,
    /// 0xe1e4 链节重复（缺口 同标签双节）。
    LinkDup,
    /// 0xe1e5 链节断档（兑现 节序缺失）。
    LinkGap,
    /// 0xe1e6 链指纹破缺（回放校验失败）。
    SealBroken,
    /// 0xe1e7 回放漂移（回放终态≠追加终态）。
    ReplayDrift,
    /// 0xe1e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3781Code {
    /// 全量闭集。
    pub const ALL: [u3781Code; 12] = [
        u3781Code::SideDup,
        u3781Code::SideGap,
        u3781Code::Mismatch,
        u3781Code::HalfWrite,
        u3781Code::LinkDup,
        u3781Code::LinkGap,
        u3781Code::SealBroken,
        u3781Code::ReplayDrift,
        u3781Code::LedgerBroken,
        u3781Code::SnapshotFingerprint,
        u3781Code::TableBroken,
        u3781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3781Code::SideDup => 0xe1e0,
            u3781Code::SideGap => 0xe1e1,
            u3781Code::Mismatch => 0xe1e2,
            u3781Code::HalfWrite => 0xe1e3,
            u3781Code::LinkDup => 0xe1e4,
            u3781Code::LinkGap => 0xe1e5,
            u3781Code::SealBroken => 0xe1e6,
            u3781Code::ReplayDrift => 0xe1e7,
            u3781Code::LedgerBroken => 0xe1e8,
            u3781Code::SnapshotFingerprint => 0xe1e9,
            u3781Code::TableBroken => 0xe1ea,
            u3781Code::SegmentGuard => 0xe1eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3781Code::SideDup => "侧重复（收口 同槽双源）",
            u3781Code::SideGap => "侧断档（兑现 槽未采样）",
            u3781Code::Mismatch => "双源失配（同槽两侧值不等）",
            u3781Code::HalfWrite => "半写（仅一侧落值）",
            u3781Code::LinkDup => "链节重复（缺口 同标签双节）",
            u3781Code::LinkGap => "链节断档（兑现 节序缺失）",
            u3781Code::SealBroken => "链指纹破缺（回放校验失败）",
            u3781Code::ReplayDrift => "回放漂移（回放终态≠追加终态）",
            u3781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3781Code::SideDup => "按槽唯一律去重",
            u3781Code::SideGap => "按槽闭集补采",
            u3781Code::Mismatch => "以权威侧复测仲裁",
            u3781Code::HalfWrite => "补齐对侧采样",
            u3781Code::LinkDup => "按节序唯一律合并",
            u3781Code::LinkGap => "按节序补链",
            u3781Code::SealBroken => "按防篡改规程重建链",
            u3781Code::ReplayDrift => "按追加序重放",
            u3781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3781Code::TableBroken => "按语义规则重建常量表",
            u3781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3781cmp（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 域中期收口 双源对拍（收口）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3781cmpSlot {
    /// 条目表。
    CLOSE,
    /// 范围栏。
    MID,
    /// 兑现位。
    DECL,
    /// 缺口表。
    Fulfill,
    /// 证据槽。
    SCOPE,
    /// 绑定栏。
    ITEM,
    /// 核验账。
    DONE,
    /// 封口签。
    OPEN,
}

impl mxu3781cmpSlot {
    /// 全量槽位闭集。
    pub const ALL: [mxu3781cmpSlot; 8] = [
        mxu3781cmpSlot::CLOSE,
        mxu3781cmpSlot::MID,
        mxu3781cmpSlot::DECL,
        mxu3781cmpSlot::Fulfill,
        mxu3781cmpSlot::SCOPE,
        mxu3781cmpSlot::ITEM,
        mxu3781cmpSlot::DONE,
        mxu3781cmpSlot::OPEN,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3781cmpSlot::CLOSE => 0,
            mxu3781cmpSlot::MID => 1,
            mxu3781cmpSlot::DECL => 2,
            mxu3781cmpSlot::Fulfill => 3,
            mxu3781cmpSlot::SCOPE => 4,
            mxu3781cmpSlot::ITEM => 5,
            mxu3781cmpSlot::DONE => 6,
            mxu3781cmpSlot::OPEN => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3781cmpSlot::CLOSE => "条目表",
            mxu3781cmpSlot::MID => "范围栏",
            mxu3781cmpSlot::DECL => "兑现位",
            mxu3781cmpSlot::Fulfill => "缺口表",
            mxu3781cmpSlot::SCOPE => "证据槽",
            mxu3781cmpSlot::ITEM => "绑定栏",
            mxu3781cmpSlot::DONE => "核验账",
            mxu3781cmpSlot::OPEN => "封口签",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3781cmpPair {
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
pub struct mxu3781cmpDual {
    /// 逐槽样本。
    pub pairs: [mxu3781cmpPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mxu3781cmpDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mxu3781cmpPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mxu3781cmpSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mxu3781cmpPair { a, b, same, seq: self.rounds };
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
// 引擎 mxu3781seal（链式指纹：域中期收口 封存链节 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 域中期收口 链式指纹封存（逐链回放校验）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3781sealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3781sealChain {
    /// 链节点序列。
    pub links: [mxu3781sealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mxu3781sealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mxu3781sealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u3781Code> {
        if self.n >= 16 || payload == 0 {
            return Err(u3781Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mxu3781sealLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1e0, 0xe1ef]。
pub const fn u3781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3781Code::ALL.len() {
        let c = u3781Code::ALL[i].code();
        if c < 0xe1e0 || c > 0xe1ef {
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
fn u3781_guard_seg() -> bool {
    u3781_segment_guard()
}

/// UNX-F3781 双源逐槽对拍全同 + 复核通过。
fn check_u3781_dual_same() -> bool {
    let mut d = mxu3781cmpDual::new();
    let mut si = 0usize;
    while si < mxu3781cmpSlot::ALL.len() {
        let v = (si as u64 + 1) * 11;
        d.sample(mxu3781cmpSlot::ALL[si], v, v);
        si += 1;
    }
    let mut ok = d.all_same() && d.recount();
    ok
}

/// UNX-F3781 封存链 8 节追加回放校验全绿。
fn check_u3781_dual_chain() -> bool {
    let mut c = mxu3781sealChain::new();
    let mut i = 0u32;
    while i < 8 { let _ = c.append(i + 1, (i as u64 + 1) * 13); i += 1; }
    let mut ok = c.verify();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3781_checks() -> CheckSet {
    let mut set = CheckSet::new("U3781");
    set.add(
        "u3781-seg-guard",
        u3781_guard_seg(),
        if u3781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3781-dual-same",
        check_u3781_dual_same(),
        if check_u3781_dual_same() { "" } else { "UNX-F3781 dual mismatch" },
    );
    set.add(
        "u3781-dual-chain",
        check_u3781_dual_chain(),
        if check_u3781_dual_chain() { "" } else { "UNX-F3781 chain mismatch" },
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
    fn u3781_all_checks_green() {
        let set = run_u3781_checks();
        assert!(set.all_passed(), "U3781 domain checks must be green");
    }

    #[test]
    fn u3781_segment_guard_holds() {
        assert!(u3781_segment_guard());
    }
}
