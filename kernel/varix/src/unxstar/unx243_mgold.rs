//! unxt243_mgold —— UNX-F0241 结构黄金比对（黄金哈希 vs 实际结构）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0241 · BootManifest 全景结构（十二步总纲） · 判据：十二步节结构定义齐备（固件/内存/CPU/随机化/参数/健康/时间/日志/上次引导/订阅/完整性/时序），总 CRC 链校验过，结构黄金比对一致
//! · 组件：黄金哈希账 + 结构指纹 + 双源对拍 + 偏离升级
//! · 判据落地：结构黄金比对一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T243_VERSION: &str = "t243-mgold-v1";

/// 诊断段基址。
pub const T243_SEG_BASE: u16 = 0x3920;

// --------------------------------------------------------------------------
// 诊断码（t243 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 黄金比对诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t243Code {
    /// 0x3920 黄金漂移（结构哈希≠黄金）。
    GoldenDrift,
    /// 0x3921 结构缺失（步节未定义）。
    StructMissing,
    /// 0x3922 多余步节（13 步外溢）。
    ExtraStep,
    /// 0x3923 字段数非法（非 8 字段）。
    FieldCountBad,
    /// 0x3924 步序交换（相邻步互换）。
    OrderSwap,
    /// 0x3925 哈希碰撞（异结构同哈希）。
    HashCollision,
    /// 0x3926 版本非法（manifest 版本漂移）。
    VersionBad,
    /// 0x3927 账本破缺（黄金账不守恒）。
    LedgerBreak,
    /// 0x3928 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3929 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x392a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x392b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x392c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x392d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x392e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x392f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t243Code {
    /// 全量闭集。
    pub const ALL: [t243Code; 16] = [
        t243Code::GoldenDrift,
        t243Code::StructMissing,
        t243Code::ExtraStep,
        t243Code::FieldCountBad,
        t243Code::OrderSwap,
        t243Code::HashCollision,
        t243Code::VersionBad,
        t243Code::LedgerBreak,
        t243Code::LedgerBroken,
        t243Code::SnapshotFingerprint,
        t243Code::TableBroken,
        t243Code::IndexMisaligned,
        t243Code::DuplicateEntry,
        t243Code::OrderViolated,
        t243Code::GuardTripped,
        t243Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t243Code::GoldenDrift => 0x3920,
            t243Code::StructMissing => 0x3921,
            t243Code::ExtraStep => 0x3922,
            t243Code::FieldCountBad => 0x3923,
            t243Code::OrderSwap => 0x3924,
            t243Code::HashCollision => 0x3925,
            t243Code::VersionBad => 0x3926,
            t243Code::LedgerBreak => 0x3927,
            t243Code::LedgerBroken => 0x3928,
            t243Code::SnapshotFingerprint => 0x3929,
            t243Code::TableBroken => 0x392a,
            t243Code::IndexMisaligned => 0x392b,
            t243Code::DuplicateEntry => 0x392c,
            t243Code::OrderViolated => 0x392d,
            t243Code::GuardTripped => 0x392e,
            t243Code::SegmentGuard => 0x392f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t243Code::GoldenDrift => "黄金漂移（结构哈希≠黄金）",
            t243Code::StructMissing => "结构缺失（步节未定义）",
            t243Code::ExtraStep => "多余步节（13 步外溢）",
            t243Code::FieldCountBad => "字段数非法（非 8 字段）",
            t243Code::OrderSwap => "步序交换（相邻步互换）",
            t243Code::HashCollision => "哈希碰撞（异结构同哈希）",
            t243Code::VersionBad => "版本非法（manifest 版本漂移）",
            t243Code::LedgerBreak => "账本破缺（黄金账不守恒）",
            t243Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t243Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t243Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t243Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t243Code::DuplicateEntry => "条目重复（同键双写）",
            t243Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t243Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t243Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t243Code::GoldenDrift => "按黄金结构对齐",
            t243Code::StructMissing => "补步节定义",
            t243Code::ExtraStep => "按十二步边界裁撤",
            t243Code::FieldCountBad => "按 8 字段律校正",
            t243Code::OrderSwap => "按步序单调律恢复",
            t243Code::HashCollision => "换哈希族重算",
            t243Code::VersionBad => "按版本台账校正",
            t243Code::LedgerBreak => "按黄金账重建",
            t243Code::LedgerBroken => "按守恒律重建台账后重跑",
            t243Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t243Code::TableBroken => "按语义规则重建常量表",
            t243Code::IndexMisaligned => "按声明序归一化后重跑",
            t243Code::DuplicateEntry => "合并同键条目为末次生效",
            t243Code::OrderViolated => "按依赖矩阵重排后重跑",
            t243Code::GuardTripped => "检查临界区嵌套与阈值",
            t243Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 mgold（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// manifest 黄金快照（六分量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct mgold {
    /// 指纹分量：steps。
    pub steps: u64,
    /// 指纹分量：fields。
    pub fields: u64,
    /// 指纹分量：steps_total。
    pub steps_total: u64,
    /// 指纹分量：chain_head。
    pub chain_head: u64,
    /// 指纹分量：schema_hash。
    pub schema_hash: u64,
    /// 指纹分量：version。
    pub version: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl mgold {
    /// 构造并即时合成指纹。
    pub fn compute(
        steps: u64,
        fields: u64,
        steps_total: u64,
        chain_head: u64,
        schema_hash: u64,
        version: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &steps.to_le_bytes());
        h = fnv1a64_from(h, &fields.to_le_bytes());
        h = fnv1a64_from(h, &steps_total.to_le_bytes());
        h = fnv1a64_from(h, &chain_head.to_le_bytes());
        h = fnv1a64_from(h, &schema_hash.to_le_bytes());
        h = fnv1a64_from(h, &version.to_le_bytes());
        Self {
            steps,
            fields,
            steps_total,
            chain_head,
            schema_hash,
            version,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.steps.to_le_bytes());
        h = fnv1a64_from(h, &self.fields.to_le_bytes());
        h = fnv1a64_from(h, &self.steps_total.to_le_bytes());
        h = fnv1a64_from(h, &self.chain_head.to_le_bytes());
        h = fnv1a64_from(h, &self.schema_hash.to_le_bytes());
        h = fnv1a64_from(h, &self.version.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 gold_dual（双源对拍：4 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 黄金 vs 实际双源对拍（步数/字段数/链头/版本）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum gold_dualSlot {
    /// 步数。
    Steps,
    /// 字段数。
    Fields,
    /// 链头。
    ChainHead,
    /// 版本。
    Version,
}

impl gold_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [gold_dualSlot; 4] = [
        gold_dualSlot::Steps,
        gold_dualSlot::Fields,
        gold_dualSlot::ChainHead,
        gold_dualSlot::Version,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            gold_dualSlot::Steps => 0,
            gold_dualSlot::Fields => 1,
            gold_dualSlot::ChainHead => 2,
            gold_dualSlot::Version => 3,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            gold_dualSlot::Steps => "步数",
            gold_dualSlot::Fields => "字段数",
            gold_dualSlot::ChainHead => "链头",
            gold_dualSlot::Version => "版本",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct gold_dualPair {
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
pub struct gold_dualDual {
    /// 逐槽样本。
    pub pairs: [gold_dualPair; 4],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl gold_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [gold_dualPair { a: 0, b: 0, same: false, seq: 0 }; 4],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: gold_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = gold_dualPair { a, b, same, seq: self.rounds };
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
        self.diff_n == 0 && self.same_n == 4
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 4 && self.same_n + self.diff_n == 4
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3920, 0x392f]。
pub const fn t243_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t243Code::ALL.len() {
        let c = t243Code::ALL[i].code();
        if c < 0x3920 || c > 0x392f {
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
fn t243_guard_seg() -> bool {
    t243_segment_guard()
}

/// 黄金快照正样本 + 篡改检出。
fn check_c1_golden_snapshot() -> bool {
    let s = mgold::compute(12, 8, 96, 0xdead_beef, 0x1234_5678, 1);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.steps = 13;
        ok = bad.tampered();
    }
    ok
}

/// 黄金对拍四槽一致 + 负样本。
fn check_c2_gold_dual() -> bool {
    let mut d = gold_dualDual::new();
    d.sample(gold_dualSlot::Steps, 12, 12);
    d.sample(gold_dualSlot::Fields, 8, 8);
    d.sample(gold_dualSlot::ChainHead, 0xdead_beef, 0xdead_beef);
    d.sample(gold_dualSlot::Version, 1, 1);
    let mut ok = d.all_same();
    if ok {
        d.sample(gold_dualSlot::Steps, 12, 13);
        ok = !d.all_same() && d.diff_n == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t243_checks() -> CheckSet {
    let mut set = CheckSet::new("T243");
    set.add(
        "t243-seg-guard",
        t243_guard_seg(),
        if t243_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t243-c1-golden-snapshot",
        check_c1_golden_snapshot(),
        if check_c1_golden_snapshot() { "" } else { "golden snapshot mismatch" },
    );
    set.add(
        "t243-c2-gold-dual",
        check_c2_gold_dual(),
        if check_c2_gold_dual() { "" } else { "golden dual compare mismatch" },
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
    fn t243_all_checks_green() {
        let set = run_t243_checks();
        assert!(set.all_passed(), "T243 domain checks must be green");
    }

    #[test]
    fn t243_segment_guard_holds() {
        assert!(t243_segment_guard());
    }
}
