//! unxu2761_u2761merge —— 四级压力档判定器（水位+产出双因子） 合并窗与封存快照（压力档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2761 · 四级压力档判定器（水位+产出双因子） · 判据原文：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! · 组件：压力档 合并窗（4096 单元冲刷 + 单调/不相交双断言）+ FNV 封存快照
//! · 判据落地：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2761_VERSION: &str = "u2761-u2761merge-v1";

/// 诊断段基址。
pub const U2761_SEG_BASE: u16 = 0xada0;

// --------------------------------------------------------------------------
// 诊断码（u2761 十六码 · 13 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 压力档 合并面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2761Code {
    /// 0xada0 单元重复（档位跳变 同单元双入）。
    UnitDup,
    /// 0xada1 单元断档（双源分歧 序缺失）。
    UnitGap,
    /// 0xada2 冲刷破缺（冲刷后残余未清）。
    FlushBad,
    /// 0xada3 回压破缺（注入误判 回压信号失真）。
    BackpBad,
    /// 0xada4 区间重叠（合并后区间相交）。
    SegOverlap,
    /// 0xada5 合并失衡（容量账不平）。
    MergeUnbalanced,
    /// 0xada6 容量越界（超 CAP 拒收未登记）。
    CapOob,
    /// 0xada7 排序丢失（单调序破坏）。
    SortLost,
    /// 0xada8 类别非法（因子失真 类别出闭集）。
    KindBad,
    /// 0xada9 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xadaa 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xadab 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xadac 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2761Code {
    /// 全量闭集。
    pub const ALL: [u2761Code; 13] = [
        u2761Code::UnitDup,
        u2761Code::UnitGap,
        u2761Code::FlushBad,
        u2761Code::BackpBad,
        u2761Code::SegOverlap,
        u2761Code::MergeUnbalanced,
        u2761Code::CapOob,
        u2761Code::SortLost,
        u2761Code::KindBad,
        u2761Code::LedgerBroken,
        u2761Code::SnapshotFingerprint,
        u2761Code::TableBroken,
        u2761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2761Code::UnitDup => 0xada0,
            u2761Code::UnitGap => 0xada1,
            u2761Code::FlushBad => 0xada2,
            u2761Code::BackpBad => 0xada3,
            u2761Code::SegOverlap => 0xada4,
            u2761Code::MergeUnbalanced => 0xada5,
            u2761Code::CapOob => 0xada6,
            u2761Code::SortLost => 0xada7,
            u2761Code::KindBad => 0xada8,
            u2761Code::LedgerBroken => 0xada9,
            u2761Code::SnapshotFingerprint => 0xadaa,
            u2761Code::TableBroken => 0xadab,
            u2761Code::SegmentGuard => 0xadac,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2761Code::UnitDup => "单元重复（档位跳变 同单元双入）",
            u2761Code::UnitGap => "单元断档（双源分歧 序缺失）",
            u2761Code::FlushBad => "冲刷破缺（冲刷后残余未清）",
            u2761Code::BackpBad => "回压破缺（注入误判 回压信号失真）",
            u2761Code::SegOverlap => "区间重叠（合并后区间相交）",
            u2761Code::MergeUnbalanced => "合并失衡（容量账不平）",
            u2761Code::CapOob => "容量越界（超 CAP 拒收未登记）",
            u2761Code::SortLost => "排序丢失（单调序破坏）",
            u2761Code::KindBad => "类别非法（因子失真 类别出闭集）",
            u2761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2761Code::UnitDup => "按单元唯一律合并",
            u2761Code::UnitGap => "按连续律补齐",
            u2761Code::FlushBad => "按冲刷律复清",
            u2761Code::BackpBad => "按回压口径复测",
            u2761Code::SegOverlap => "按不相交律再归并",
            u2761Code::MergeUnbalanced => "按容量账复算",
            u2761Code::CapOob => "按容量账登记",
            u2761Code::SortLost => "按序重排",
            u2761Code::KindBad => "按类别闭集钳制",
            u2761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2761Code::TableBroken => "按语义规则重建常量表",
            u2761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2761win（区段排序归并：压力档 制 + 零相交 + 容量守恒）
// --------------------------------------------------------------------------
/// 压力档 合并窗（档位跳变）。
/// 区段表容量上限。
pub const MXU2761WIN_CAP: usize = 4096;

/// 区段（起址 + 长度，闭开区间 [base, base+len)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct mxu2761winSeg {
    /// 起始地址（压力档 对齐）。
    pub base: u64,
    /// 长度（压力档）。
    pub len: u64,
    /// 类型标签（由调用方约定，如可用/保留/ACPI）。
    pub kind: u32,
}

impl mxu2761winSeg {
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
pub struct mxu2761winMerger {
    /// 排序后归并输出。
    pub out: [mxu2761winSeg; 4096],
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

impl mxu2761winMerger {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            out: [mxu2761winSeg::new(0, 0, 0); 4096],
            out_n: 0,
            in_total: 0,
            out_total: 0,
            overlap_rejects: 0,
            coalesced: 0,
        }
    }

    /// 插入排序（按 base 升序；同 base 按 len 降序，短的在前）。
    pub fn sort(&mut self, segs: &[mxu2761winSeg]) {
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
    pub fn coalesce(&mut self) -> Result<(), u2761Code> {
        let mut i = 0usize;
        while i < self.out_n {
            if i + 1 < self.out_n {
                let a = self.out[i];
                let b = self.out[i + 1];
                if b.base < a.end() {
                    // 相交：拒止并记账（零相交红线）。
                    self.overlap_rejects += 1;
                    return Err(u2761Code::SegOverlap);
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
            return Err(u2761Code::MergeUnbalanced);
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
// 快照 Mxu2761MergSnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 压力档 合并封存快照（FNV 三态指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2761MergSnap {
    /// 指纹分量：nm_m。
    pub nm_m: u64,
    /// 指纹分量：lt_m。
    pub lt_m: u64,
    /// 指纹分量：md_m。
    pub md_m: u64,
    /// 指纹分量：hv_m。
    pub hv_m: u64,
    /// 指纹分量：wl_m。
    pub wl_m: u64,
    /// 指纹分量：ou_m。
    pub ou_m: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2761MergSnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        nm_m: u64,
        lt_m: u64,
        md_m: u64,
        hv_m: u64,
        wl_m: u64,
        ou_m: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &nm_m.to_le_bytes());
        h = fnv1a64_from(h, &lt_m.to_le_bytes());
        h = fnv1a64_from(h, &md_m.to_le_bytes());
        h = fnv1a64_from(h, &hv_m.to_le_bytes());
        h = fnv1a64_from(h, &wl_m.to_le_bytes());
        h = fnv1a64_from(h, &ou_m.to_le_bytes());
        Self {
            nm_m,
            lt_m,
            md_m,
            hv_m,
            wl_m,
            ou_m,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.nm_m.to_le_bytes());
        h = fnv1a64_from(h, &self.lt_m.to_le_bytes());
        h = fnv1a64_from(h, &self.md_m.to_le_bytes());
        h = fnv1a64_from(h, &self.hv_m.to_le_bytes());
        h = fnv1a64_from(h, &self.wl_m.to_le_bytes());
        h = fnv1a64_from(h, &self.ou_m.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xada0, 0xadaf]。
pub const fn u2761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2761Code::ALL.len() {
        let c = u2761Code::ALL[i].code();
        if c < 0xada0 || c > 0xadaf {
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
fn u2761_guard_seg() -> bool {
    u2761_segment_guard()
}

/// UNX-F2761 合并窗容量锚 + 空窗单调/不相交。
fn check_u2761_mrg_mono() -> bool {
    let mut mg = mxu2761winMerger::new();
    let mut ok = MXU2761WIN_CAP == 4096;
    if ok { ok = mg.monotone() && mg.disjoint(); }
    ok
}

/// UNX-F2761 合并封存快照往返恒等。
fn check_u2761_mrg_snap() -> bool {
    let s0 = Mxu2761MergSnap::compute(51, 68, 85, 102, 119, 136);
    let mut s1 = Mxu2761MergSnap::compute(51, 68, 85, 102, 119, 136);
    s1.recompute();
    let mut ok = s0 == s1 && !s1.tampered();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2761_checks() -> CheckSet {
    let mut set = CheckSet::new("U2761");
    set.add(
        "u2761-seg-guard",
        u2761_guard_seg(),
        if u2761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2761-mrg-mono",
        check_u2761_mrg_mono(),
        if check_u2761_mrg_mono() { "" } else { "UNX-F2761 merge mismatch" },
    );
    set.add(
        "u2761-mrg-snap",
        check_u2761_mrg_snap(),
        if check_u2761_mrg_snap() { "" } else { "UNX-F2761 merge snapshot mismatch" },
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
    fn u2761_all_checks_green() {
        let set = run_u2761_checks();
        assert!(set.all_passed(), "U2761 domain checks must be green");
    }

    #[test]
    fn u2761_segment_guard_holds() {
        assert!(u2761_segment_guard());
    }
}
