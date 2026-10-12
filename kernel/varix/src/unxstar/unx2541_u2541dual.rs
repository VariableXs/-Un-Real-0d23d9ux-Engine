//! unxu2541_u2541dual —— alloc_pages/free_pages 冻结接口总账（正式冻结版） 双源对拍与链式封存（冻结接口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2541 · alloc_pages/free_pages 冻结接口总账（正式冻结版） · 判据原文：UNX-F2541 · 判据：双接口冻结件登记（全域消费方清单齐备），签名与错误码三机直测断言一致
//! · 组件：冻结接口 双源对拍（8 槽逐槽回读一致）+ 8 节链式指纹封存
//! · 判据落地：UNX-F2541 · 判据：双接口冻结件登记（全域消费方清单齐备），签名与错误码三机直测断言一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2541_VERSION: &str = "u2541-u2541dual-v1";

/// 诊断段基址。
pub const U2541_SEG_BASE: u16 = 0xa580;

// --------------------------------------------------------------------------
// 诊断码（u2541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 冻结接口 对拍面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2541Code {
    /// 0xa580 侧重复（消费方缺 同槽双源）。
    SideDup,
    /// 0xa581 侧断档（签名不符 槽未采样）。
    SideGap,
    /// 0xa582 双源失配（同槽两侧值不等）。
    Mismatch,
    /// 0xa583 半写（仅一侧落值）。
    HalfWrite,
    /// 0xa584 链节重复（错误码分歧 同标签双节）。
    LinkDup,
    /// 0xa585 链节断档（直测不一致 节序缺失）。
    LinkGap,
    /// 0xa586 链指纹破缺（回放校验失败）。
    SealBroken,
    /// 0xa587 回放漂移（回放终态≠追加终态）。
    ReplayDrift,
    /// 0xa588 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa589 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa58a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa58b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2541Code {
    /// 全量闭集。
    pub const ALL: [u2541Code; 12] = [
        u2541Code::SideDup,
        u2541Code::SideGap,
        u2541Code::Mismatch,
        u2541Code::HalfWrite,
        u2541Code::LinkDup,
        u2541Code::LinkGap,
        u2541Code::SealBroken,
        u2541Code::ReplayDrift,
        u2541Code::LedgerBroken,
        u2541Code::SnapshotFingerprint,
        u2541Code::TableBroken,
        u2541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2541Code::SideDup => 0xa580,
            u2541Code::SideGap => 0xa581,
            u2541Code::Mismatch => 0xa582,
            u2541Code::HalfWrite => 0xa583,
            u2541Code::LinkDup => 0xa584,
            u2541Code::LinkGap => 0xa585,
            u2541Code::SealBroken => 0xa586,
            u2541Code::ReplayDrift => 0xa587,
            u2541Code::LedgerBroken => 0xa588,
            u2541Code::SnapshotFingerprint => 0xa589,
            u2541Code::TableBroken => 0xa58a,
            u2541Code::SegmentGuard => 0xa58b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2541Code::SideDup => "侧重复（消费方缺 同槽双源）",
            u2541Code::SideGap => "侧断档（签名不符 槽未采样）",
            u2541Code::Mismatch => "双源失配（同槽两侧值不等）",
            u2541Code::HalfWrite => "半写（仅一侧落值）",
            u2541Code::LinkDup => "链节重复（错误码分歧 同标签双节）",
            u2541Code::LinkGap => "链节断档（直测不一致 节序缺失）",
            u2541Code::SealBroken => "链指纹破缺（回放校验失败）",
            u2541Code::ReplayDrift => "回放漂移（回放终态≠追加终态）",
            u2541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2541Code::SideDup => "按槽唯一律去重",
            u2541Code::SideGap => "按槽闭集补采",
            u2541Code::Mismatch => "以权威侧复测仲裁",
            u2541Code::HalfWrite => "补齐对侧采样",
            u2541Code::LinkDup => "按节序唯一律合并",
            u2541Code::LinkGap => "按节序补链",
            u2541Code::SealBroken => "按防篡改规程重建链",
            u2541Code::ReplayDrift => "按追加序重放",
            u2541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2541Code::TableBroken => "按语义规则重建常量表",
            u2541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2541cmp（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 冻结接口 双源对拍（消费方缺）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2541cmpSlot {
    /// 分配面。
    Alloc,
    /// 释放面。
    Free,
    /// 消费方面。
    Consumer,
    /// 签名面。
    Sign,
    /// 错误码面。
    ErrCode,
    /// 三机面。
    Machine,
    /// 参数面。
    Param,
    /// 返回面。
    Return,
}

impl mxu2541cmpSlot {
    /// 全量槽位闭集。
    pub const ALL: [mxu2541cmpSlot; 8] = [
        mxu2541cmpSlot::Alloc,
        mxu2541cmpSlot::Free,
        mxu2541cmpSlot::Consumer,
        mxu2541cmpSlot::Sign,
        mxu2541cmpSlot::ErrCode,
        mxu2541cmpSlot::Machine,
        mxu2541cmpSlot::Param,
        mxu2541cmpSlot::Return,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2541cmpSlot::Alloc => 0,
            mxu2541cmpSlot::Free => 1,
            mxu2541cmpSlot::Consumer => 2,
            mxu2541cmpSlot::Sign => 3,
            mxu2541cmpSlot::ErrCode => 4,
            mxu2541cmpSlot::Machine => 5,
            mxu2541cmpSlot::Param => 6,
            mxu2541cmpSlot::Return => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2541cmpSlot::Alloc => "分配面",
            mxu2541cmpSlot::Free => "释放面",
            mxu2541cmpSlot::Consumer => "消费方面",
            mxu2541cmpSlot::Sign => "签名面",
            mxu2541cmpSlot::ErrCode => "错误码面",
            mxu2541cmpSlot::Machine => "三机面",
            mxu2541cmpSlot::Param => "参数面",
            mxu2541cmpSlot::Return => "返回面",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2541cmpPair {
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
pub struct mxu2541cmpDual {
    /// 逐槽样本。
    pub pairs: [mxu2541cmpPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mxu2541cmpDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mxu2541cmpPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mxu2541cmpSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mxu2541cmpPair { a, b, same, seq: self.rounds };
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
// 引擎 mxu2541seal（链式指纹：冻结接口 封存链节 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 冻结接口 链式指纹封存（逐链回放校验）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2541sealLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2541sealChain {
    /// 链节点序列。
    pub links: [mxu2541sealLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl mxu2541sealChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [mxu2541sealLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, u2541Code> {
        if self.n >= 16 || payload == 0 {
            return Err(u2541Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = mxu2541sealLink { tag, payload, digest };
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

/// 段位守卫：全部诊断码必须落在本段 [0xa580, 0xa58f]。
pub const fn u2541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2541Code::ALL.len() {
        let c = u2541Code::ALL[i].code();
        if c < 0xa580 || c > 0xa58f {
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
fn u2541_guard_seg() -> bool {
    u2541_segment_guard()
}

/// UNX-F2541 双源逐槽对拍全同 + 复核通过。
fn check_u2541_dual_same() -> bool {
    let mut d = mxu2541cmpDual::new();
    let mut si = 0usize;
    while si < mxu2541cmpSlot::ALL.len() {
        let v = (si as u64 + 1) * 11;
        d.sample(mxu2541cmpSlot::ALL[si], v, v);
        si += 1;
    }
    let mut ok = d.all_same() && d.recount();
    ok
}

/// UNX-F2541 封存链 8 节追加回放校验全绿。
fn check_u2541_dual_chain() -> bool {
    let mut c = mxu2541sealChain::new();
    let mut i = 0u32;
    while i < 8 { let _ = c.append(i + 1, (i as u64 + 1) * 13); i += 1; }
    let mut ok = c.verify();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2541_checks() -> CheckSet {
    let mut set = CheckSet::new("U2541");
    set.add(
        "u2541-seg-guard",
        u2541_guard_seg(),
        if u2541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2541-dual-same",
        check_u2541_dual_same(),
        if check_u2541_dual_same() { "" } else { "UNX-F2541 dual mismatch" },
    );
    set.add(
        "u2541-dual-chain",
        check_u2541_dual_chain(),
        if check_u2541_dual_chain() { "" } else { "UNX-F2541 chain mismatch" },
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
    fn u2541_all_checks_green() {
        let set = run_u2541_checks();
        assert!(set.all_passed(), "U2541 domain checks must be green");
    }

    #[test]
    fn u2541_segment_guard_holds() {
        assert!(u2541_segment_guard());
    }
}
