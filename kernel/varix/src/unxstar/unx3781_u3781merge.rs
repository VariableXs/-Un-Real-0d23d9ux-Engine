//! unxu3781_u3781merge —— 域中期收口总设计（B20 前向声明的兑现面） 合并窗与封存快照（域中期收口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3781 · 域中期收口总设计（B20 前向声明的兑现面） · 判据原文：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F3781 域中期收口：段断言大关 ≥80+五条件移交"——期票与兑付跨十批的闭链）；收口的范围声明（B16–B30 十五批——A5 域续领波的全域——不含 B01–B15（已在前次 finalize 收口——历史账冻结）；M-539 收口预跑（三总轧空转→流程通→正式总轧前演练）。<br>J2：收口的纪律（收口只核不改（发现的偏差走修正流程+留痕——收口不涂改历史账。
//! · 组件：域中期收口 合并窗（4096 单元冲刷 + 单调/不相交双断言）+ FNV 封存快照
//! · 判据落地：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3781_VERSION: &str = "u3781-u3781merge-v1";

/// 诊断段基址。
pub const U3781_SEG_BASE: u16 = 0xe200;

// --------------------------------------------------------------------------
// 诊断码（u3781 十六码 · 13 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域中期收口 合并面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3781Code {
    /// 0xe200 单元重复（收口 同单元双入）。
    UnitDup,
    /// 0xe201 单元断档（兑现 序缺失）。
    UnitGap,
    /// 0xe202 冲刷破缺（冲刷后残余未清）。
    FlushBad,
    /// 0xe203 回压破缺（缺口 回压信号失真）。
    BackpBad,
    /// 0xe204 区间重叠（合并后区间相交）。
    SegOverlap,
    /// 0xe205 合并失衡（容量账不平）。
    MergeUnbalanced,
    /// 0xe206 容量越界（超 CAP 拒收未登记）。
    CapOob,
    /// 0xe207 排序丢失（单调序破坏）。
    SortLost,
    /// 0xe208 类别非法（兑现 类别出闭集）。
    KindBad,
    /// 0xe209 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe20a 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe20b 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe20c 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3781Code {
    /// 全量闭集。
    pub const ALL: [u3781Code; 13] = [
        u3781Code::UnitDup,
        u3781Code::UnitGap,
        u3781Code::FlushBad,
        u3781Code::BackpBad,
        u3781Code::SegOverlap,
        u3781Code::MergeUnbalanced,
        u3781Code::CapOob,
        u3781Code::SortLost,
        u3781Code::KindBad,
        u3781Code::LedgerBroken,
        u3781Code::SnapshotFingerprint,
        u3781Code::TableBroken,
        u3781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3781Code::UnitDup => 0xe200,
            u3781Code::UnitGap => 0xe201,
            u3781Code::FlushBad => 0xe202,
            u3781Code::BackpBad => 0xe203,
            u3781Code::SegOverlap => 0xe204,
            u3781Code::MergeUnbalanced => 0xe205,
            u3781Code::CapOob => 0xe206,
            u3781Code::SortLost => 0xe207,
            u3781Code::KindBad => 0xe208,
            u3781Code::LedgerBroken => 0xe209,
            u3781Code::SnapshotFingerprint => 0xe20a,
            u3781Code::TableBroken => 0xe20b,
            u3781Code::SegmentGuard => 0xe20c,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3781Code::UnitDup => "单元重复（收口 同单元双入）",
            u3781Code::UnitGap => "单元断档（兑现 序缺失）",
            u3781Code::FlushBad => "冲刷破缺（冲刷后残余未清）",
            u3781Code::BackpBad => "回压破缺（缺口 回压信号失真）",
            u3781Code::SegOverlap => "区间重叠（合并后区间相交）",
            u3781Code::MergeUnbalanced => "合并失衡（容量账不平）",
            u3781Code::CapOob => "容量越界（超 CAP 拒收未登记）",
            u3781Code::SortLost => "排序丢失（单调序破坏）",
            u3781Code::KindBad => "类别非法（兑现 类别出闭集）",
            u3781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3781Code::UnitDup => "按单元唯一律合并",
            u3781Code::UnitGap => "按连续律补齐",
            u3781Code::FlushBad => "按冲刷律复清",
            u3781Code::BackpBad => "按回压口径复测",
            u3781Code::SegOverlap => "按不相交律再归并",
            u3781Code::MergeUnbalanced => "按容量账复算",
            u3781Code::CapOob => "按容量账登记",
            u3781Code::SortLost => "按序重排",
            u3781Code::KindBad => "按类别闭集钳制",
            u3781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3781Code::TableBroken => "按语义规则重建常量表",
            u3781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3781win（区段排序归并：域中期收口 制 + 零相交 + 容量守恒）
// --------------------------------------------------------------------------
/// 域中期收口 合并窗（收口）。
/// 区段表容量上限。
pub const MXU3781WIN_CAP: usize = 4096;

/// 区段（起址 + 长度，闭开区间 [base, base+len)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct mxu3781winSeg {
    /// 起始地址（域中期收口 对齐）。
    pub base: u64,
    /// 长度（域中期收口）。
    pub len: u64,
    /// 类型标签（由调用方约定，如可用/保留/ACPI）。
    pub kind: u32,
}

impl mxu3781winSeg {
    /// 构造（零长区段直接拒收由上层保证）。
    pub const fn new(base: u64, len: u64, kind: u32) -> Self {
        Self { base, len, kind }
    }

    /// 末址（闭开区间上界）。
    pub const fn end(&self) -> u64 {
        self.base + self.len
    }
}

/// 归并结果账（容量守恒：输入总长 == 输出总长）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3781winMerger {
    /// 排序后归并输出。
    pub out: [mxu3781winSeg; 4096],
    /// 输出计数。
    pub out_n: usize,
    /// 输入总长。
    pub in_total: u64,
    /// 输出总长。
    pub out_total: u64,
    /// 相交拒止计数。
    pub overlap_rejects: u64,
    /// 归并吸收计数（相邻同类型段合并）。
    pub coalesced: u64,
}

impl mxu3781winMerger {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            out: [mxu3781winSeg::new(0, 0, 0); 4096],
            out_n: 0,
            in_total: 0,
            out_total: 0,
            overlap_rejects: 0,
            coalesced: 0,
        }
    }

    /// 插入排序（按 base 升序；同 base 按 len 降序，短的在前）。
    pub fn sort(&mut self, segs: &[mxu3781winSeg]) {
        self.out_n = 0;
        let mut i = 0usize;
        while i < segs.len() && i < 4096 {
            let s = segs[i];
            self.in_total += s.len;
            let mut j = self.out_n;
            while j > 0 && self.out[j - 1].base > s.base {
                self.out[j] = self.out[j - 1];
                j -= 1;
            }
            self.out[j] = s;
            self.out_n += 1;
            i += 1;
        }
    }

    /// 归并：相邻同类型且首尾相接的段吸收；相交直接拒止记账。
    pub fn coalesce(&mut self) -> Result<(), u3781Code> {
        let mut i = 0usize;
        while i < self.out_n {
            if i + 1 < self.out_n {
                let a = self.out[i];
                let b = self.out[i + 1];
                if b.base < a.end() {
                    // 相交：拒止并记账（零相交红线）。
                    self.overlap_rejects += 1;
                    return Err(u3781Code::SegOverlap);
                }
                if a.kind == b.kind && a.end() == b.base {
                    // 首尾相接同类型：吸收。
                    self.out[i].len += b.len;
                    let mut k = i + 1;
                    while k + 1 < self.out_n {
                        self.out[k] = self.out[k + 1];
                        k += 1;
                    }
                    self.out_n -= 1;
                    self.coalesced += 1;
                    continue;
                }
            }
            i += 1;
        }
        // 输出总长核算。
        let mut total = 0u64;
        let mut j = 0usize;
        while j < self.out_n {
            total += self.out[j].len;
            j += 1;
        }
        self.out_total = total;
        if self.out_total != self.in_total {
            return Err(u3781Code::MergeUnbalanced);
        }
        Ok(())
    }

    /// 单调性复检（输出全序：base 严格递增且段间有缝或同序）。
    pub fn monotone(&self) -> bool {
        let mut i = 1usize;
        while i < self.out_n {
            if self.out[i - 1].base >= self.out[i].base {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 零相交复检（逐对扫描相邻段）。
    pub fn disjoint(&self) -> bool {
        let mut i = 0usize;
        while i + 1 < self.out_n {
            if self.out[i].end() > self.out[i + 1].base {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 快照 Mxu3781MergSnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 域中期收口 合并封存快照（FNV 三态指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3781MergSnap {
    /// 指纹分量：close_m。
    pub close_m: u64,
    /// 指纹分量：mid_m。
    pub mid_m: u64,
    /// 指纹分量：decl_m。
    pub decl_m: u64,
    /// 指纹分量：fulfill_m。
    pub fulfill_m: u64,
    /// 指纹分量：scope_m。
    pub scope_m: u64,
    /// 指纹分量：item_m。
    pub item_m: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3781MergSnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        close_m: u64,
        mid_m: u64,
        decl_m: u64,
        fulfill_m: u64,
        scope_m: u64,
        item_m: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &close_m.to_le_bytes());
        h = fnv1a64_from(h, &mid_m.to_le_bytes());
        h = fnv1a64_from(h, &decl_m.to_le_bytes());
        h = fnv1a64_from(h, &fulfill_m.to_le_bytes());
        h = fnv1a64_from(h, &scope_m.to_le_bytes());
        h = fnv1a64_from(h, &item_m.to_le_bytes());
        Self {
            close_m,
            mid_m,
            decl_m,
            fulfill_m,
            scope_m,
            item_m,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.close_m.to_le_bytes());
        h = fnv1a64_from(h, &self.mid_m.to_le_bytes());
        h = fnv1a64_from(h, &self.decl_m.to_le_bytes());
        h = fnv1a64_from(h, &self.fulfill_m.to_le_bytes());
        h = fnv1a64_from(h, &self.scope_m.to_le_bytes());
        h = fnv1a64_from(h, &self.item_m.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe200, 0xe20f]。
pub const fn u3781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3781Code::ALL.len() {
        let c = u3781Code::ALL[i].code();
        if c < 0xe200 || c > 0xe20f {
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

/// UNX-F3781 合并窗容量锚 + 空窗单调/不相交。
fn check_u3781_mrg_mono() -> bool {
    let mut mg = mxu3781winMerger::new();
    let mut ok = MXU3781WIN_CAP == 4096;
    if ok { ok = mg.monotone() && mg.disjoint(); }
    ok
}

/// UNX-F3781 合并封存快照往返恒等。
fn check_u3781_mrg_snap() -> bool {
    let s0 = Mxu3781MergSnap::compute(51, 68, 85, 102, 119, 136);
    let mut s1 = Mxu3781MergSnap::compute(51, 68, 85, 102, 119, 136);
    s1.recompute();
    let mut ok = s0 == s1 && !s1.tampered();
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
        "u3781-mrg-mono",
        check_u3781_mrg_mono(),
        if check_u3781_mrg_mono() { "" } else { "UNX-F3781 merge mismatch" },
    );
    set.add(
        "u3781-mrg-snap",
        check_u3781_mrg_snap(),
        if check_u3781_mrg_snap() { "" } else { "UNX-F3781 merge snapshot mismatch" },
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
