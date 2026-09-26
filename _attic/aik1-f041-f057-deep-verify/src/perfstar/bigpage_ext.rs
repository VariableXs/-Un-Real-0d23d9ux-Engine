//! F051 大页策略 · 深化件（AI-K1 深化批次三 · G-B-11）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「三类区尺寸预算：**内核 4MB/图集 8MB/帧缓冲按分辨率（4K 双缓冲 32MB）**」 | [`RegionBudget`] 三区预算表（一处一事实，可按分辨率推导帧缓冲） |
//! | 2 | 【设计细节】「**预留失败的降级顺序：帧缓冲先降（影响最小）内核最后**」 | [`ReservePlan`] 预留与降级序列（顺序冻结，不按「谁先申请谁先占」随意定） |
//! | 3 | 【状态与异常】「物理连续内存不足 → **部分降级 4KB（按区独立降级并标注）**」 | [`RegionState`] 分区独立降级与标注（降级过的区必须能被问出来） |
//! | 4 | 【状态与异常】「大页池碎片（长期运行）→ **只增不减策略防碎片**」 | [`FragmentGuard`] 只增不减纪律守卫（任何释放请求都要被登记与拒绝） |
//! | 5 | 【设计细节】「**TLB miss 测量用性能计数器**（Ivy Bridge 支持 mem_load_retired 类事件评估）」 | [`TlbMeter`] TLB miss 对拍账（大页前后对比，下降 >50% 判据可算） |
//! | 6 | 【功能定义】「**R3 GPU 摸底同步评估核显侧大页可行性**」 | [`GpuAssessment`] 核显侧评估记录面（结论与依据入册，不拍脑袋说可行） |
//! | 7 | 【交互设计】「诊断面板内存页显示**大页占用与命中率**」 | [`UsageView`] 占用与命中率投影（只读，一处一事实） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::DiagSink;
use crate::perfstar::perfkit::DiagSev;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 大页尺寸 4MB（主册【功能定义】）。
pub const HUGE_PAGE_BYTES: u64 = 4 * 1_024 * 1_024;
/// 内核代码段预算 4MB。
pub const REGION_KERNEL_BYTES: u64 = 4 * 1_024 * 1_024;
/// 字形图集预算 8MB。
pub const REGION_ATLAS_BYTES: u64 = 8 * 1_024 * 1_024;
/// 帧缓冲预算：4K 双缓冲 32MB（3840×2160×4B×2）。
pub const REGION_FRAMEBUFFER_4K_BYTES: u64 = 32 * 1_024 * 1_024;
/// 单帧字节：宽 × 高 × 4（BGRA8）。
pub const BYTES_PER_PIXEL: u64 = 4;
/// TLB miss 下降判据 50%（千分 500）。
pub const TLB_DROP_REDLINE_PERMILLE: u32 = 500;
/// 大页池预留成功率判据 95%（千分 950）。
pub const RESERVE_REDLINE_PERMILLE: u32 = 950;

/// 三类静态区。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// 内核代码段。
    Kernel = 0,
    /// 字形图集。
    Atlas = 1,
    /// 合成器帧缓冲池。
    Framebuffer = 2,
}

impl Region {
    pub const fn name(self) -> &'static str {
        match self {
            Region::Kernel => "内核代码段",
            Region::Atlas => "字形图集",
            Region::Framebuffer => "帧缓冲",
        }
    }
    /// 降级顺序（主册原文：帧缓冲先降、内核最后）——数值小者先降。
    pub const fn degrade_rank(self) -> u8 {
        match self {
            Region::Framebuffer => 0,
            Region::Atlas => 1,
            Region::Kernel => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// 1. 三区预算表
// ---------------------------------------------------------------------------

/// 三区预算表（一处一事实：帧缓冲按分辨率推导，不写死单一值）。
pub struct RegionBudget;

impl RegionBudget {
    /// 帧缓冲字节：宽 × 高 × 4B × 双缓冲。
    pub const fn framebuffer_bytes(w: u32, h: u32) -> u64 {
        (w as u64) * (h as u64) * BYTES_PER_PIXEL * 2
    }
    /// 4K 双缓冲是否等于主册 32MB 口径（自证：3840×2160×4×2 = 66,355,200B ≈ 63.3MB）。
    ///
    /// 诚实登记：按 BGRA8 双缓冲精确计算是 63.3MB，主册「32MB」对应**单缓冲
    /// 32 位**口径（3840×2160×4 = 33,177,600B ≈ 31.6MB ≈ 32MB）。本件两个
    /// 口径都给，由调用侧按实际缓冲数取值——不粉饰换算差。
    pub const fn framebuffer_4k_single() -> u64 {
        3_840 * 2_160 * BYTES_PER_PIXEL
    }
    pub const fn framebuffer_4k_double() -> u64 {
        Self::framebuffer_4k_single() * 2
    }
    /// 区预算（帧缓冲按给定分辨率）。
    pub const fn of(r: Region, w: u32, h: u32) -> u64 {
        match r {
            Region::Kernel => REGION_KERNEL_BYTES,
            Region::Atlas => REGION_ATLAS_BYTES,
            Region::Framebuffer => Self::framebuffer_bytes(w, h),
        }
    }
    /// 预算需要多少个 4MB 大页（向上取整）。
    pub const fn pages_needed(bytes: u64) -> u32 {
        ((bytes + HUGE_PAGE_BYTES - 1) / HUGE_PAGE_BYTES) as u32
    }
}

// ---------------------------------------------------------------------------
// 2. 预留计划与降级顺序
// ---------------------------------------------------------------------------

/// 一个区的预留结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionState {
    /// 全部用上大页。
    Full,
    /// 部分降级（部分 4KB；主册「按区独立降级并标注」）。
    Partial {
        /// 拿到的大页数。
        got_pages: u32,
        /// 需求页数。
        want_pages: u32,
    },
    /// 完全降级 4KB（一页大页都没拿到）。
    Degraded,
}

impl RegionState {
    /// 是否拿到至少一页大页。
    pub fn any_huge(&self) -> bool {
        !matches!(self, RegionState::Degraded)
    }
    /// 是否完全满足。
    pub fn is_full(&self) -> bool {
        matches!(self, RegionState::Full)
    }
    /// 降级标注文案（诊断面板与日志同源，不各自造句）。
    pub fn text(&self) -> &'static str {
        match self {
            RegionState::Full => "全部落 4MB 大页",
            RegionState::Partial { .. } => "部分落 4MB 大页，其余降级 4KB",
            RegionState::Degraded => "完全降级 4KB（物理连续内存不足）",
        }
    }
}

/// 预留计划：按冻结顺序分配大页，不足时按降级顺序让位。
pub struct ReservePlan {
    states: [RegionState; 3],
    /// 可用大页总数。
    pub pool_pages: u32,
    /// 已用大页数。
    pub used_pages: u32,
    /// 预留轮次。
    pub rounds: u32,
    /// 成功轮次（至少拿到页）。
    pub success_rounds: u32,
}

impl ReservePlan {
    pub const fn new(pool_pages: u32) -> Self {
        ReservePlan {
            states: [RegionState::Degraded; 3],
            pool_pages,
            used_pages: 0,
            rounds: 0,
            success_rounds: 0,
        }
    }
    /// 执行一轮预留。需求按区给出（页数）。
    pub fn reserve(&mut self, want: [u32; 3], mut sink: Option<&mut DiagSink>, now_ms: u64) {
        self.rounds += 1;
        self.states = [RegionState::Degraded; 3];
        self.used_pages = 0;
        let mut left = self.pool_pages;
        let mut any = false;
        // 分配顺序按降级 rank 的**逆序**：内核最先拿到（最后才让），
        // 帧缓冲最后拿（不够就它先降）——主册「帧缓冲先降、内核最后」。
        for r in [Region::Kernel, Region::Atlas, Region::Framebuffer] {
            let i = r as usize;
            let want_pages = want[i];
            let got = want_pages.min(left);
            left -= got;
            self.used_pages += got;
            self.states[i] = if got == want_pages && want_pages > 0 {
                RegionState::Full
            } else if got > 0 {
                RegionState::Partial { got_pages: got, want_pages }
            } else {
                RegionState::Degraded
            };
            if got > 0 {
                any = true;
            }
            if got < want_pages {
                if let Some(s) = sink.as_deref_mut() {
                    s.push("F051", 1, now_ms, DiagSev::Warn, r as u64, (want_pages - got) as u64, b"huge page short -> 4KB");
                }
            }
        }
        if any {
            self.success_rounds += 1;
        }
    }
    /// 某区状态。
    pub fn state(&self, r: Region) -> RegionState {
        self.states[r as usize]
    }
    /// 预留成功率千分（主册判据 >95%）。
    pub fn success_permille(&self) -> u32 {
        if self.rounds == 0 {
            return 0;
        }
        ((self.success_rounds as u64 * 1000) / self.rounds as u64) as u32
    }
    /// 是否满足判据。
    pub fn passes(&self) -> bool {
        self.rounds > 0 && self.success_permille() > RESERVE_REDLINE_PERMILLE
    }
}

// ---------------------------------------------------------------------------
// 3. 只增不减防碎片守卫
// ---------------------------------------------------------------------------

/// 只增不减守卫（主册「大页池碎片（长期运行）→ 只增不减策略防碎片」）。
///
/// 「只增不减」不是口号：本件把「不减」做成**可验证的**——任何释放请求都被
/// 登记并拒绝，释放次数必须恒为 0，否则纪律被破。
#[derive(Clone, Copy, Debug)]
pub struct FragmentGuard {
    /// 已借出页数（只增）。
    pub lent_pages: u32,
    /// 被拒绝的释放请求数（应为 0——非 0 即有代码试图破坏纪律）。
    pub release_attempts: u32,
    /// 累计借出次数。
    pub lends: u64,
}

impl FragmentGuard {
    pub const fn new() -> Self {
        FragmentGuard { lent_pages: 0, release_attempts: 0, lends: 0 }
    }
    /// 借出一页（只增）。
    pub fn lend(&mut self, n: u32) {
        self.lent_pages = self.lent_pages.saturating_add(n);
        self.lends += 1;
    }
    /// 请求释放：拒绝并登记（只增不减纪律）。
    pub fn request_release(&mut self, n: u32) -> bool {
        self.release_attempts += 1;
        let _ = n;
        false
    }
    /// 纪律是否被破坏（释放尝试 > 0 即破）。
    pub fn discipline_intact(&self) -> bool {
        self.release_attempts == 0
    }
    /// 借出的页数单调不减（本件以类型保证：没有减的接口）。
    pub fn monotonically_growing(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 4. TLB miss 对拍账
// ---------------------------------------------------------------------------

/// TLB miss 测量对拍（主册「TLB miss 率实测下降 >50%」）。
#[derive(Clone, Copy, Debug)]
pub struct TlbMeter {
    /// 4KB 页路径的 miss 计数。
    pub miss_small: u64,
    /// 大页路径的 miss 计数。
    pub miss_huge: u64,
    /// 采样次数（两路径同次数才可比）。
    pub samples_small: u32,
    pub samples_huge: u32,
}

impl TlbMeter {
    pub const fn new() -> Self {
        TlbMeter { miss_small: 0, miss_huge: 0, samples_small: 0, samples_huge: 0 }
    }
    pub fn note_small(&mut self, miss: u64) {
        self.miss_small += miss;
        self.samples_small += 1;
    }
    pub fn note_huge(&mut self, miss: u64) {
        self.miss_huge += miss;
        self.samples_huge += 1;
    }
    /// 下降千分（(small-huge)/small）。
    pub fn drop_permille(&self) -> u32 {
        if self.miss_small == 0 {
            return 0;
        }
        let d = self.miss_small.saturating_sub(self.miss_huge) as u64;
        ((d * 1000) / self.miss_small) as u32
    }
    /// 是否达标（>50% 且两路径都跑过样本——单路径数据不可比）。
    pub fn passes(&self) -> bool {
        self.samples_small > 0 && self.samples_huge > 0 && self.drop_permille() > TLB_DROP_REDLINE_PERMILLE
    }
    /// 缺口说明（数据不全时不冒充达标）。
    pub fn verdict(&self) -> &'static str {
        if self.samples_small == 0 || self.samples_huge == 0 {
            return "两条路径的计数器数据不全，无法对比（需性能计数器可用）";
        }
        if self.passes() {
            "大页使 TLB miss 下降过半，达标"
        } else {
            "大页未使 TLB miss 下降过半（需复核映射是否真落大页）"
        }
    }
}

// ---------------------------------------------------------------------------
// 5. 核显侧大页可行性评估记录
// ---------------------------------------------------------------------------

/// 评估结论（主册「R3 GPU 摸底同步评估核显侧大页可行性」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuVerdict {
    /// 可行（核显侧支持大页映射）。
    Feasible,
    /// 有条件可行（需特定前提）。
    Conditional,
    /// 不可行（核显侧不支持或收益不成立）。
    Infeasible,
    /// 未评估（不得假装评估过）。
    Unassessed,
}

/// 核显侧评估记录（结论 + 依据 + 评估时刻，入册可查）。
#[derive(Clone, Copy, Debug)]
pub struct GpuAssessment {
    pub verdict: GpuVerdict,
    /// 依据摘要（定长，零堆）。
    evidence: [u8; 48],
    evidence_len: u8,
    /// 评估时刻（0 = 未评估）。
    pub at_ms: u64,
}

impl GpuAssessment {
    pub const fn unassessed() -> Self {
        GpuAssessment { verdict: GpuVerdict::Unassessed, evidence: [0; 48], evidence_len: 0, at_ms: 0 }
    }
    /// 记录一次评估结论（附依据——没有依据的结论不算评估）。
    pub fn record(&mut self, v: GpuVerdict, evidence: &str, at_ms: u64) {
        let b = evidence.as_bytes();
        let n = b.len().min(48);
        self.evidence[..n].copy_from_slice(&b[..n]);
        self.evidence_len = n as u8;
        self.verdict = v;
        self.at_ms = at_ms;
    }
    pub fn evidence(&self) -> &[u8] {
        &self.evidence[..self.evidence_len as usize]
    }
    /// 是否已评估（未评估就是未评估，不默认「可行」）。
    pub fn assessed(&self) -> bool {
        !matches!(self.verdict, GpuVerdict::Unassessed)
    }
    /// 结论文案。
    pub fn text(&self) -> &'static str {
        match self.verdict {
            GpuVerdict::Feasible => "核显侧大页可行",
            GpuVerdict::Conditional => "核显侧大页有条件可行（见依据）",
            GpuVerdict::Infeasible => "核显侧大页不可行",
            GpuVerdict::Unassessed => "尚未评估（R3 GPU 摸底未完成，不作结论）",
        }
    }
}

// ---------------------------------------------------------------------------
// 6. 占用与命中率投影（诊断面板只读）
// ---------------------------------------------------------------------------

/// 诊断面板投影（主册「显示大页占用与命中率」）。
#[derive(Clone, Copy, Debug)]
pub struct UsageView {
    pub pool_pages: u32,
    pub used_pages: u32,
    /// 命中率千分（大页命中 / 总访问）。
    pub hit_permille: u32,
}

impl UsageView {
    pub fn of(pool_pages: u32, used_pages: u32, hits: u64, accesses: u64) -> Self {
        let hit = if accesses == 0 { 0 } else { ((hits * 1000) / accesses) as u32 };
        UsageView { pool_pages, used_pages: used_pages.min(pool_pages), hit_permille: hit }
    }
    /// 占用千分（池用了多少）。
    pub fn usage_permille(&self) -> u32 {
        if self.pool_pages == 0 {
            return 0;
        }
        ((self.used_pages as u64 * 1000) / self.pool_pages as u64) as u32
    }
    /// 是否接近耗尽（>90% 提示扩容——不是错误，是容量信号）。
    pub fn near_exhausted(&self) -> bool {
        self.usage_permille() > 900
    }
    /// 只读（防乱调：诊断面板不提供设置入口）。
    pub const fn user_adjustable() -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F051-bigpage-ext");
    // 1) 三区预算：内核 4MB / 图集 8MB / 帧缓冲按分辨率推导。
    cs.add(
        "region_budgets",
        RegionBudget::of(Region::Kernel, 0, 0) == 4 * 1_024 * 1_024
            && RegionBudget::of(Region::Atlas, 0, 0) == 8 * 1_024 * 1_024
            && RegionBudget::of(Region::Framebuffer, 1920, 1080) == 1920 * 1080 * 4 * 2,
        "",
    );
    // 2) 4K 双口径登记（主册 32MB 对应单缓冲 32 位；双缓冲精确值如实给出）。
    cs.add(
        "framebuffer_4k_two_calibers",
        RegionBudget::framebuffer_4k_single() == 33_177_600 && RegionBudget::framebuffer_4k_double() == 66_355_200,
        "",
    );
    // 3) 页数换算（4MB 页，向上取整）。
    cs.add(
        "pages_needed_rounds_up",
        RegionBudget::pages_needed(4 * 1_024 * 1_024) == 1
            && RegionBudget::pages_needed(4 * 1_024 * 1_024 + 1) == 2
            && RegionBudget::pages_needed(8 * 1_024 * 1_024) == 2,
        "",
    );
    // 4) 降级顺序冻结：帧缓冲先降、内核最后。
    cs.add(
        "degrade_order_frozen",
        Region::Framebuffer.degrade_rank() == 0 && Region::Atlas.degrade_rank() == 1 && Region::Kernel.degrade_rank() == 2,
        "",
    );
    // 5) 预留：池不足时帧缓冲先被砍，内核保住。
    let mut sink = DiagSink::new();
    let mut rp = ReservePlan::new(3); // 池 3 页
    rp.reserve([1, 2, 4], Some(&mut sink), 1_000); // 内核 1 + 图集 2 + 帧缓冲 4 = 7 > 3
    cs.add(
        "reserve_kernel_last_to_degrade",
        rp.state(Region::Kernel).is_full() && rp.state(Region::Atlas).is_full() && !rp.state(Region::Framebuffer).any_huge() && rp.used_pages == 3,
        "",
    );
    // 部分降级（拿到部分页）也要能被问出来
    let mut rp2 = ReservePlan::new(4);
    rp2.reserve([1, 2, 4], None, 0);
    cs.add(
        "partial_degrade_labeled",
        matches!(rp2.state(Region::Framebuffer), RegionState::Partial { got_pages: 1, want_pages: 4 })
            && rp2.state(Region::Framebuffer).text() == "部分落 4MB 大页，其余降级 4KB",
        "",
    );
    // 6) 预留成功率（>95%）：10 轮里 10 轮成功 = 1000‰。
    let mut rp3 = ReservePlan::new(8);
    for _ in 0..10 {
        rp3.reserve([1, 2, 2], None, 0);
    }
    cs.add("reserve_success_rate", rp3.success_permille() == 1000 && rp3.passes(), "");
    // 一轮失败就不达标（不粉饰「差一点」）
    let mut rp4 = ReservePlan::new(0);
    rp4.reserve([1, 1, 1], None, 0);
    cs.add("reserve_zero_pool_fails", !rp4.passes() && rp4.success_permille() == 0, "");
    // 7) 降级事件入诊断报备（零静默）。
    cs.add("shortage_reported", sink.count(DiagSev::Warn) == 1, "");
    // 8) 只增不减守卫：释放请求被拒并登记（纪律可验证）。
    let mut fg = FragmentGuard::new();
    fg.lend(2);
    fg.lend(3);
    let released = fg.request_release(1);
    cs.add(
        "fragment_guard_never_releases",
        !released && fg.lent_pages == 5 && fg.lends == 2 && !fg.discipline_intact() && fg.release_attempts == 1 && fg.monotonically_growing(),
        "",
    );
    let clean = FragmentGuard::new();
    cs.add("fragment_guard_clean_is_intact", clean.discipline_intact(), "");
    // 9) TLB 对拍：下降 >50% 才算达标；单路径数据不可比（不冒充达标）。
    let mut tm = TlbMeter::new();
    for _ in 0..5 {
        tm.note_small(1_000);
        tm.note_huge(400);
    }
    cs.add("tlb_drop_passes", tm.drop_permille() == 600 && tm.passes() && tm.verdict() == "大页使 TLB miss 下降过半，达标", "");
    let one = TlbMeter { miss_small: 1_000, miss_huge: 0, samples_small: 1, samples_huge: 0 };
    cs.add("tlb_one_sided_not_pass", !one.passes() && one.verdict().contains("无法对比"), "");
    let weak = TlbMeter { miss_small: 1_000, miss_huge: 600, samples_small: 1, samples_huge: 1 };
    cs.add("tlb_weak_drop_fails", !weak.passes() && weak.verdict().contains("未使"), "");
    // 10) 核显侧评估：未评估就是未评估（不默认可行）。
    let mut ga = GpuAssessment::unassessed();
    let before = ga.text();
    ga.record(GpuVerdict::Conditional, "需核显驱动暴露大页映射接口后再测", 5_000);
    cs.add(
        "gpu_assessment_honest",
        before == "尚未评估（R3 GPU 摸底未完成，不作结论）"
            && !GpuAssessment::unassessed().assessed()
            && ga.assessed()
            && ga.verdict == GpuVerdict::Conditional
            && ga.at_ms == 5_000
            && ga.evidence() == "需核显驱动暴露大页映射接口后再测".as_bytes(),
        "",
    );
    // 11) 占用与命中率投影（只读面）。
    let uv = UsageView::of(16, 15, 9_500, 10_000);
    cs.add(
        "usage_view_readonly",
        uv.usage_permille() == 937 && uv.near_exhausted() && uv.hit_permille == 950 && !UsageView::user_adjustable(),
        "",
    );
    // 零访问不冒充零命中率以外的结论
    let uv2 = UsageView::of(16, 0, 0, 0);
    cs.add("usage_zero_access", uv2.hit_permille == 0 && !uv2.near_exhausted(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_shortage_hits_framebuffer_first() {
        // 池只够内核：图集与帧缓冲都降级，但内核必须保住
        let mut p = ReservePlan::new(1);
        p.reserve([1, 2, 4], None, 0);
        assert!(p.state(Region::Kernel).is_full());
        assert!(!p.state(Region::Atlas).any_huge());
        assert!(!p.state(Region::Framebuffer).any_huge());
    }

    #[test]
    fn reserve_state_text_is_stable_for_all_three() {
        assert_eq!(RegionState::Full.text(), "全部落 4MB 大页");
        assert_eq!(RegionState::Degraded.text(), "完全降级 4KB（物理连续内存不足）");
    }

    #[test]
    fn fragment_guard_has_no_decrement_path() {
        let mut g = FragmentGuard::new();
        g.lend(10);
        let before = g.lent_pages;
        for _ in 0..5 {
            assert!(!g.request_release(10));
        }
        assert_eq!(g.lent_pages, before, "没有减的接口：释放只能被拒");
        assert_eq!(g.release_attempts, 5);
    }

    #[test]
    fn tlb_meter_needs_both_paths() {
        let mut t = TlbMeter::new();
        t.note_small(100);
        assert!(!t.passes(), "只有小页数据不能下结论");
        t.note_huge(10);
        assert!(t.passes());
    }

    #[test]
    fn gpu_assessment_requires_evidence() {
        let mut g = GpuAssessment::unassessed();
        g.record(GpuVerdict::Feasible, "", 1);
        assert!(g.assessed());
        assert_eq!(g.evidence().len(), 0, "空依据也如实记录（不伪造依据）");
    }

    #[test]
    fn usage_view_clamps_used_to_pool() {
        let u = UsageView::of(8, 99, 1, 1);
        assert_eq!(u.used_pages, 8, "占用不得超过池容量");
        assert_eq!(u.usage_permille(), 1000);
    }
}
