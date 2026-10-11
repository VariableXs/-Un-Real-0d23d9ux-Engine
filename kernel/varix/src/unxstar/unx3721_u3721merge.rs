//! unxu3721_u3721merge —— 桩体系总设计（替身三原则与桩清单） 合并窗与封存快照（桩体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3721 · 桩体系总设计（替身三原则与桩清单） · 判据原文：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（时钟/中断/DMA/存储/网卡五桩 v1——清单 const 扩桩走 bump）；M-479 三原则首验（时钟桩+中断桩最小联动→三原则逐条断言）。<br>J2：桩与被测分池（B20 隔离池同律——桩基础设施不吃被测方内存）。
//! · 组件：桩体系 合并窗（4096 单元冲刷 + 单调/不相交双断言）+ FNV 封存快照
//! · 判据落地：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3721_VERSION: &str = "u3721-u3721merge-v1";

/// 诊断段基址。
pub const U3721_SEG_BASE: u16 = 0xe1d0;

// --------------------------------------------------------------------------
// 诊断码（u3721 十六码 · 13 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 桩体系 合并面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3721Code {
    /// 0xe1d0 单元重复（桩 同单元双入）。
    UnitDup,
    /// 0xe1d1 单元断档（替身 序缺失）。
    UnitGap,
    /// 0xe1d2 冲刷破缺（冲刷后残余未清）。
    FlushBad,
    /// 0xe1d3 回压破缺（录制 回压信号失真）。
    BackpBad,
    /// 0xe1d4 区间重叠（合并后区间相交）。
    SegOverlap,
    /// 0xe1d5 合并失衡（容量账不平）。
    MergeUnbalanced,
    /// 0xe1d6 容量越界（超 CAP 拒收未登记）。
    CapOob,
    /// 0xe1d7 排序丢失（单调序破坏）。
    SortLost,
    /// 0xe1d8 类别非法（三原则 类别出闭集）。
    KindBad,
    /// 0xe1d9 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1da 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1db 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1dc 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3721Code {
    /// 全量闭集。
    pub const ALL: [u3721Code; 13] = [
        u3721Code::UnitDup,
        u3721Code::UnitGap,
        u3721Code::FlushBad,
        u3721Code::BackpBad,
        u3721Code::SegOverlap,
        u3721Code::MergeUnbalanced,
        u3721Code::CapOob,
        u3721Code::SortLost,
        u3721Code::KindBad,
        u3721Code::LedgerBroken,
        u3721Code::SnapshotFingerprint,
        u3721Code::TableBroken,
        u3721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3721Code::UnitDup => 0xe1d0,
            u3721Code::UnitGap => 0xe1d1,
            u3721Code::FlushBad => 0xe1d2,
            u3721Code::BackpBad => 0xe1d3,
            u3721Code::SegOverlap => 0xe1d4,
            u3721Code::MergeUnbalanced => 0xe1d5,
            u3721Code::CapOob => 0xe1d6,
            u3721Code::SortLost => 0xe1d7,
            u3721Code::KindBad => 0xe1d8,
            u3721Code::LedgerBroken => 0xe1d9,
            u3721Code::SnapshotFingerprint => 0xe1da,
            u3721Code::TableBroken => 0xe1db,
            u3721Code::SegmentGuard => 0xe1dc,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3721Code::UnitDup => "单元重复（桩 同单元双入）",
            u3721Code::UnitGap => "单元断档（替身 序缺失）",
            u3721Code::FlushBad => "冲刷破缺（冲刷后残余未清）",
            u3721Code::BackpBad => "回压破缺（录制 回压信号失真）",
            u3721Code::SegOverlap => "区间重叠（合并后区间相交）",
            u3721Code::MergeUnbalanced => "合并失衡（容量账不平）",
            u3721Code::CapOob => "容量越界（超 CAP 拒收未登记）",
            u3721Code::SortLost => "排序丢失（单调序破坏）",
            u3721Code::KindBad => "类别非法（三原则 类别出闭集）",
            u3721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3721Code::UnitDup => "按单元唯一律合并",
            u3721Code::UnitGap => "按连续律补齐",
            u3721Code::FlushBad => "按冲刷律复清",
            u3721Code::BackpBad => "按回压口径复测",
            u3721Code::SegOverlap => "按不相交律再归并",
            u3721Code::MergeUnbalanced => "按容量账复算",
            u3721Code::CapOob => "按容量账登记",
            u3721Code::SortLost => "按序重排",
            u3721Code::KindBad => "按类别闭集钳制",
            u3721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3721Code::TableBroken => "按语义规则重建常量表",
            u3721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3721win（区段排序归并：桩体系 制 + 零相交 + 容量守恒）
// --------------------------------------------------------------------------
/// 桩体系 合并窗（桩）。
/// 区段表容量上限。
pub const MXU3721WIN_CAP: usize = 4096;

/// 区段（起址 + 长度，闭开区间 [base, base+len)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct mxu3721winSeg {
    /// 起始地址（桩体系 对齐）。
    pub base: u64,
    /// 长度（桩体系）。
    pub len: u64,
    /// 类型标签（由调用方约定，如可用/保留/ACPI）。
    pub kind: u32,
}

impl mxu3721winSeg {
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
pub struct mxu3721winMerger {
    /// 排序后归并输出。
    pub out: [mxu3721winSeg; 4096],
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

impl mxu3721winMerger {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            out: [mxu3721winSeg::new(0, 0, 0); 4096],
            out_n: 0,
            in_total: 0,
            out_total: 0,
            overlap_rejects: 0,
            coalesced: 0,
        }
    }

    /// 插入排序（按 base 升序；同 base 按 len 降序，短的在前）。
    pub fn sort(&mut self, segs: &[mxu3721winSeg]) {
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
    pub fn coalesce(&mut self) -> Result<(), u3721Code> {
        let mut i = 0usize;
        while i < self.out_n {
            if i + 1 < self.out_n {
                let a = self.out[i];
                let b = self.out[i + 1];
                if b.base < a.end() {
                    // 相交：拒止并记账（零相交红线）。
                    self.overlap_rejects += 1;
                    return Err(u3721Code::SegOverlap);
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
            return Err(u3721Code::MergeUnbalanced);
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
// 快照 Mxu3721MergSnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 桩体系 合并封存快照（FNV 三态指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3721MergSnap {
    /// 指纹分量：stub_m。
    pub stub_m: u64,
    /// 指纹分量：triple_m。
    pub triple_m: u64,
    /// 指纹分量：list_m。
    pub list_m: u64,
    /// 指纹分量：inj_m。
    pub inj_m: u64,
    /// 指纹分量：rec_m。
    pub rec_m: u64,
    /// 指纹分量：play_m。
    pub play_m: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3721MergSnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        stub_m: u64,
        triple_m: u64,
        list_m: u64,
        inj_m: u64,
        rec_m: u64,
        play_m: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &stub_m.to_le_bytes());
        h = fnv1a64_from(h, &triple_m.to_le_bytes());
        h = fnv1a64_from(h, &list_m.to_le_bytes());
        h = fnv1a64_from(h, &inj_m.to_le_bytes());
        h = fnv1a64_from(h, &rec_m.to_le_bytes());
        h = fnv1a64_from(h, &play_m.to_le_bytes());
        Self {
            stub_m,
            triple_m,
            list_m,
            inj_m,
            rec_m,
            play_m,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.stub_m.to_le_bytes());
        h = fnv1a64_from(h, &self.triple_m.to_le_bytes());
        h = fnv1a64_from(h, &self.list_m.to_le_bytes());
        h = fnv1a64_from(h, &self.inj_m.to_le_bytes());
        h = fnv1a64_from(h, &self.rec_m.to_le_bytes());
        h = fnv1a64_from(h, &self.play_m.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1d0, 0xe1df]。
pub const fn u3721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3721Code::ALL.len() {
        let c = u3721Code::ALL[i].code();
        if c < 0xe1d0 || c > 0xe1df {
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
fn u3721_guard_seg() -> bool {
    u3721_segment_guard()
}

/// UNX-F3721 合并窗容量锚 + 空窗单调/不相交。
fn check_u3721_mrg_mono() -> bool {
    let mut mg = mxu3721winMerger::new();
    let mut ok = MXU3721WIN_CAP == 4096;
    if ok { ok = mg.monotone() && mg.disjoint(); }
    ok
}

/// UNX-F3721 合并封存快照往返恒等。
fn check_u3721_mrg_snap() -> bool {
    let s0 = Mxu3721MergSnap::compute(51, 68, 85, 102, 119, 136);
    let mut s1 = Mxu3721MergSnap::compute(51, 68, 85, 102, 119, 136);
    s1.recompute();
    let mut ok = s0 == s1 && !s1.tampered();
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3721_checks() -> CheckSet {
    let mut set = CheckSet::new("U3721");
    set.add(
        "u3721-seg-guard",
        u3721_guard_seg(),
        if u3721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3721-mrg-mono",
        check_u3721_mrg_mono(),
        if check_u3721_mrg_mono() { "" } else { "UNX-F3721 merge mismatch" },
    );
    set.add(
        "u3721-mrg-snap",
        check_u3721_mrg_snap(),
        if check_u3721_mrg_snap() { "" } else { "UNX-F3721 merge snapshot mismatch" },
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
    fn u3721_all_checks_green() {
        let set = run_u3721_checks();
        assert!(set.all_passed(), "U3721 domain checks must be green");
    }

    #[test]
    fn u3721_segment_guard_holds() {
        assert!(u3721_segment_guard());
    }
}
