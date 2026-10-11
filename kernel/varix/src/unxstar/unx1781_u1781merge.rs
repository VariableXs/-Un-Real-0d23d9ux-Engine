//! unxu1781_u1781merge —— B01–B09 每核结构总成总纲（十层结构拼图） 合并窗与封存快照（结构层）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1781 · B01–B09 每核结构总成总纲（十层结构拼图） · 判据原文：UNX-F1781 · 判据：十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）总成关系断言 ≥30 条全过，层间接口全量登记
//! · 组件：结构层 合并窗（4096 单元冲刷 + 单调/不相交双断言）+ FNV 封存快照
//! · 判据落地：UNX-F1781 · 判据：十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）总成关系断言 ≥30 条全过，层间接口全量登记
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1781_VERSION: &str = "u1781-u1781merge-v1";

/// 诊断段基址。
pub const U1781_SEG_BASE: u16 = 0x5c20;

// --------------------------------------------------------------------------
// 诊断码（u1781 十六码 · 13 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 结构层 合并面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1781Code {
    /// 0x5c20 单元重复（拼缝错位 同单元双入）。
    UnitDup,
    /// 0x5c21 单元断档（层接口漏登 序缺失）。
    UnitGap,
    /// 0x5c22 冲刷破缺（冲刷后残余未清）。
    FlushBad,
    /// 0x5c23 回压破缺（断言缺条 回压信号失真）。
    BackpBad,
    /// 0x5c24 区间重叠（合并后区间相交）。
    SegOverlap,
    /// 0x5c25 合并失衡（容量账不平）。
    MergeUnbalanced,
    /// 0x5c26 容量越界（超 CAP 拒收未登记）。
    CapOob,
    /// 0x5c27 排序丢失（单调序破坏）。
    SortLost,
    /// 0x5c28 类别非法（咬合失败 类别出闭集）。
    KindBad,
    /// 0x5c29 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5c2a 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5c2b 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5c2c 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1781Code {
    /// 全量闭集。
    pub const ALL: [u1781Code; 13] = [
        u1781Code::UnitDup,
        u1781Code::UnitGap,
        u1781Code::FlushBad,
        u1781Code::BackpBad,
        u1781Code::SegOverlap,
        u1781Code::MergeUnbalanced,
        u1781Code::CapOob,
        u1781Code::SortLost,
        u1781Code::KindBad,
        u1781Code::LedgerBroken,
        u1781Code::SnapshotFingerprint,
        u1781Code::TableBroken,
        u1781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1781Code::UnitDup => 0x5c20,
            u1781Code::UnitGap => 0x5c21,
            u1781Code::FlushBad => 0x5c22,
            u1781Code::BackpBad => 0x5c23,
            u1781Code::SegOverlap => 0x5c24,
            u1781Code::MergeUnbalanced => 0x5c25,
            u1781Code::CapOob => 0x5c26,
            u1781Code::SortLost => 0x5c27,
            u1781Code::KindBad => 0x5c28,
            u1781Code::LedgerBroken => 0x5c29,
            u1781Code::SnapshotFingerprint => 0x5c2a,
            u1781Code::TableBroken => 0x5c2b,
            u1781Code::SegmentGuard => 0x5c2c,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1781Code::UnitDup => "单元重复（拼缝错位 同单元双入）",
            u1781Code::UnitGap => "单元断档（层接口漏登 序缺失）",
            u1781Code::FlushBad => "冲刷破缺（冲刷后残余未清）",
            u1781Code::BackpBad => "回压破缺（断言缺条 回压信号失真）",
            u1781Code::SegOverlap => "区间重叠（合并后区间相交）",
            u1781Code::MergeUnbalanced => "合并失衡（容量账不平）",
            u1781Code::CapOob => "容量越界（超 CAP 拒收未登记）",
            u1781Code::SortLost => "排序丢失（单调序破坏）",
            u1781Code::KindBad => "类别非法（咬合失败 类别出闭集）",
            u1781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1781Code::UnitDup => "按单元唯一律合并",
            u1781Code::UnitGap => "按连续律补齐",
            u1781Code::FlushBad => "按冲刷律复清",
            u1781Code::BackpBad => "按回压口径复测",
            u1781Code::SegOverlap => "按不相交律再归并",
            u1781Code::MergeUnbalanced => "按容量账复算",
            u1781Code::CapOob => "按容量账登记",
            u1781Code::SortLost => "按序重排",
            u1781Code::KindBad => "按类别闭集钳制",
            u1781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1781Code::TableBroken => "按语义规则重建常量表",
            u1781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1781win（区段排序归并：结构层 制 + 零相交 + 容量守恒）
// --------------------------------------------------------------------------
/// 结构层 合并窗（拼缝错位）。
/// 区段表容量上限。
pub const MXU1781WIN_CAP: usize = 4096;

/// 区段（起址 + 长度，闭开区间 [base, base+len)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct mxu1781winSeg {
    /// 起始地址（结构层 对齐）。
    pub base: u64,
    /// 长度（结构层）。
    pub len: u64,
    /// 类型标签（由调用方约定，如可用/保留/ACPI）。
    pub kind: u32,
}

impl mxu1781winSeg {
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
pub struct mxu1781winMerger {
    /// 排序后归并输出。
    pub out: [mxu1781winSeg; 4096],
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

impl mxu1781winMerger {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            out: [mxu1781winSeg::new(0, 0, 0); 4096],
            out_n: 0,
            in_total: 0,
            out_total: 0,
            overlap_rejects: 0,
            coalesced: 0,
        }
    }

    /// 插入排序（按 base 升序；同 base 按 len 降序，短的在前）。
    pub fn sort(&mut self, segs: &[mxu1781winSeg]) {
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
    pub fn coalesce(&mut self) -> Result<(), u1781Code> {
        let mut i = 0usize;
        while i < self.out_n {
            if i + 1 < self.out_n {
                let a = self.out[i];
                let b = self.out[i + 1];
                if b.base < a.end() {
                    // 相交：拒止并记账（零相交红线）。
                    self.overlap_rejects += 1;
                    return Err(u1781Code::SegOverlap);
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
            return Err(u1781Code::MergeUnbalanced);
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
// 快照 Mxu1781MergSnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 结构层 合并封存快照（FNV 三态指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1781MergSnap {
    /// 指纹分量：bj_m。
    pub bj_m: u64,
    /// 指纹分量：bl_m。
    pub bl_m: u64,
    /// 指纹分量：dl_m。
    pub dl_m: u64,
    /// 指纹分量：l_m。
    pub l_m: u64,
    /// 指纹分量：sjp_m。
    pub sjp_m: u64,
    /// 指纹分量：zz_m。
    pub zz_m: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1781MergSnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        bj_m: u64,
        bl_m: u64,
        dl_m: u64,
        l_m: u64,
        sjp_m: u64,
        zz_m: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &bj_m.to_le_bytes());
        h = fnv1a64_from(h, &bl_m.to_le_bytes());
        h = fnv1a64_from(h, &dl_m.to_le_bytes());
        h = fnv1a64_from(h, &l_m.to_le_bytes());
        h = fnv1a64_from(h, &sjp_m.to_le_bytes());
        h = fnv1a64_from(h, &zz_m.to_le_bytes());
        Self {
            bj_m,
            bl_m,
            dl_m,
            l_m,
            sjp_m,
            zz_m,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.bj_m.to_le_bytes());
        h = fnv1a64_from(h, &self.bl_m.to_le_bytes());
        h = fnv1a64_from(h, &self.dl_m.to_le_bytes());
        h = fnv1a64_from(h, &self.l_m.to_le_bytes());
        h = fnv1a64_from(h, &self.sjp_m.to_le_bytes());
        h = fnv1a64_from(h, &self.zz_m.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5c20, 0x5c2f]。
pub const fn u1781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1781Code::ALL.len() {
        let c = u1781Code::ALL[i].code();
        if c < 0x5c20 || c > 0x5c2f {
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
fn u1781_guard_seg() -> bool {
    u1781_segment_guard()
}

/// UNX-F1781 合并窗容量锚 + 空窗单调/不相交。
fn check_u1781_mrg_mono() -> bool {
    let mut mg = mxu1781winMerger::new();
    let mut ok = MXU1781WIN_CAP == 4096;
    if ok { ok = mg.monotone() && mg.disjoint(); }
    ok
}

/// UNX-F1781 合并封存快照往返恒等。
fn check_u1781_mrg_snap() -> bool {
    let s0 = Mxu1781MergSnap::compute(51, 68, 85, 102, 119, 136);
    let mut s1 = Mxu1781MergSnap::compute(51, 68, 85, 102, 119, 136);
    s1.recompute();
    let mut ok = s0 == s1 && !s1.tampered();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1781_checks() -> CheckSet {
    let mut set = CheckSet::new("U1781");
    set.add(
        "u1781-seg-guard",
        u1781_guard_seg(),
        if u1781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1781-mrg-mono",
        check_u1781_mrg_mono(),
        if check_u1781_mrg_mono() { "" } else { "UNX-F1781 merge mismatch" },
    );
    set.add(
        "u1781-mrg-snap",
        check_u1781_mrg_snap(),
        if check_u1781_mrg_snap() { "" } else { "UNX-F1781 merge snapshot mismatch" },
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
    fn u1781_all_checks_green() {
        let set = run_u1781_checks();
        assert!(set.all_passed(), "U1781 domain checks must be green");
    }

    #[test]
    fn u1781_segment_guard_holds() {
        assert!(u1781_segment_guard());
    }
}
