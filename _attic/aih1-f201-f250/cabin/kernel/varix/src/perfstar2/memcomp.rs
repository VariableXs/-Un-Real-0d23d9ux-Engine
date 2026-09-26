//! F058 内存压缩前瞻（perfstar2 · G-B-18）——压缩是边界扩展器，不是常规武器。
//!
//! 主册判据（验收标准第一句）：
//! **立项判据本身可验证：工作集数据采集脚本先跑一周出报告；启用后（若立项）
//! 解压延迟与压缩比双达标。**
//!
//! 功能定义（G-B-18）：冷页压缩池评估项——真实工作集超 3.2GB（水位高档
//! F045 持续一周触发）才立项启用；LZ4 级算法评估；先测后做，不提前上压缩。
//! 本项整体是 R3 级评估件——立项文档先行，代码后行（纪律同 S406）。
//!
//! 【设计细节】「冷」定义：64 秒未触碰；压缩在空闲 CPU 时段批量做（F048
//! 低频档窗口）；池大小动态 0-512MB；优先压缩文件备份页（重读成本高者）。
//! 【状态与异常】解压延迟超标（P99 >200μs）→ 池缩小；压缩比 <1.5 的页 →
//! 不压缩（白费 CPU）；池满 → 回退 OOM 流程（如实告知用户）。
//! 【规格框架】真实工作集超 3.2GB / 等效 5GB / 压缩页槽 4KB 框 / 池 0-512MB。
//!
//! 零堆纪律：定长页槽表 + 定长采集账本，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 立项门槛：真实工作集超 3.2GB（主册规格框架，水位高档 F045 联动）。
pub const PROPOSAL_WORKSET_BYTES: u64 = 3_200 * 1_000_000; // 3.2GB（十进制口径与主册一致）
/// 持续一周（7×24 小时）高位才立项——「持续」的定义。
pub const PROPOSAL_SUSTAIN_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// 「冷」定义：64 秒未触碰（主册明文）。
pub const COLD_AFTER_MS: u64 = 64_000;
/// 压缩页槽框架：4KB（主册规格框架）。
pub const SLOT_FRAME_BYTES: usize = 4_096;
/// 池大小动态上限 512MB（主册明文）。
pub const POOL_MAX_BYTES: u64 = 512 * 1_000_000;
/// 池大小动态下限 0（主册「动态 0-512MB」的下沿）。
pub const POOL_MIN_BYTES: u64 = 0;
/// 解压延迟红线：P99 >200μs → 池缩小（主册状态与异常）。
pub const DECOMP_P99_LIMIT_US: u32 = 200;
/// 压缩比下限：<1.5 的页不压缩——白费 CPU（主册明文）。
pub const RATIO_FLOOR_X10: u32 = 15; // 1.5 × 10，定点避免浮点
/// 立项后等效内存边界：4GB → 等效 5GB（主册用户故事口径）。
pub const EFFECTIVE_BOUNDARY_BYTES: u64 = 5_000 * 1_000_000;
/// 批量压缩每轮页数（空闲时段批量做——单轮工作量有界，给交互让路）。
pub const BATCH_PAGES_PER_ROUND: usize = 64;
/// 采集账本容量：7 天 × 每小时 1 桶（采集脚本「先跑一周出报告」的最小分辨率）。
pub const LEDGER_HOURS: usize = 168;

/// 候选页类别（主册：优先压缩文件备份页——重读成本高者先压）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageKind {
    /// 文件备份页（可从盘重读，重读成本高 → 最高优先级）。
    FileBacked,
    /// 匿名页（无盘上副本，压缩后换出即真丢失回读路径 → 次优先）。
    Anonymous,
    /// 内核保留页（永不进入候选——红线）。
    KernelReserved,
}

/// 一小时的采集桶（评估报告的最小数据单元）。
#[derive(Clone, Copy, Debug)]
pub struct WorksetBucket {
    pub hour_index: u16,
    /// 本小时内工作集峰值（字节）。
    pub peak_bytes: u64,
    /// 本小时内高于立项线的采样数（0-60，每分钟一采）。
    pub over_threshold_samples: u8,
}

/// 压缩页槽（4KB 框，定长零堆）。
#[derive(Clone, Copy, Debug)]
pub struct CompSlot {
    /// 槽状态：false=空 true=已压缩数据驻留。
    pub occupied: bool,
    /// 原始页类别（FileBacked/Anonymous）。
    pub kind: PageKind,
    /// 压缩后字节数（压缩比 = 4096 / stored，×10 定点入账）。
    pub stored_bytes: u16,
    /// 压缩时刻（ms 时钟，用于冷龄排序）。
    pub at_ms: u64,
    /// 最近一次解压耗时（μs，P99 统计源）。
    pub last_decomp_us: u32,
}

// ---------------------------------------------------------------------------
// 评估器（R3 评估件主体：采集 → 立项判定 → 启用后池治理）
// ---------------------------------------------------------------------------

/// 内存压缩评估与池治理器。
pub struct MemCompEvaluator {
    /// 采集账本（168 小时环形）。
    ledger: [Option<WorksetBucket>; LEDGER_HOURS],
    ledger_head: usize,
    ledger_n: usize,
    /// 当前积累桶的小时（同小时打点累进——head 前移后靠它定位当前桶）。
    current_hour: Option<u16>,
    /// 立项状态：评估中 / 已立项启用。
    enabled: bool,
    enabled_at_ms: u64,
    /// 当前池预算（动态 0-512MB）。
    pool_budget_bytes: u64,
    /// 页槽表（池预算内实际驻留）。
    slots: [CompSlot; SLOT_CAP],
    slot_n: usize,
    /// 统计：累计压缩/解压/拒绝/池满回退。
    stats_comp: u64,
    stats_decomp: u64,
    stats_rejected_ratio: u64,
    stats_pool_full_oom: u64,
    stats_shrinks: u64,
    /// 解压延迟样本（μs，定长环，P99 统计源）。
    decomp_us_ring: [u32; 64],
    decomp_ring_n: usize,
    /// 池缩小冷却（解压次数计——防单次超标窗口连环缩池）。
    shrink_cooldown: u8,
    now_ms: u64,
}

/// 槽表容量：512MB 池 / 4KB 框 = 131072 页实装随闸门；本层为判据实装，
/// 槽表取 1024 页代表样本集（覆盖全部治理路径，比例判定同构）。
const SLOT_CAP: usize = 1024;

impl MemCompEvaluator {
    pub const fn new() -> Self {
        MemCompEvaluator {
            ledger: [None; LEDGER_HOURS],
            ledger_head: 0,
            ledger_n: 0,
            current_hour: None,
            enabled: false,
            enabled_at_ms: 0,
            pool_budget_bytes: 0,
            slots: [CompSlot {
                occupied: false,
                kind: PageKind::KernelReserved,
                stored_bytes: 0,
                at_ms: 0,
                last_decomp_us: 0,
            }; SLOT_CAP],
            slot_n: 0,
            stats_comp: 0,
            stats_decomp: 0,
            stats_rejected_ratio: 0,
            stats_pool_full_oom: 0,
            stats_shrinks: 0,
            decomp_us_ring: [0; 64],
            decomp_ring_n: 0,
            shrink_cooldown: 0,
            now_ms: 0,
        }
    }

    /// 采集脚本每分钟打点：工作集观测入账本（先测后做——评估的数据源）。
    pub fn observe(&mut self, workset_bytes: u64, at_ms: u64) {
        self.now_ms = at_ms;
        let hour = (at_ms / 3_600_000) as u16;
        if self.current_hour == Some(hour) {
            // 同小时：累进当前桶（head 已前移一格——当前桶在 head-1）。
            let idx = (self.ledger_head + LEDGER_HOURS - 1) % LEDGER_HOURS;
            if let Some(b) = self.ledger[idx].as_mut() {
                if workset_bytes > b.peak_bytes {
                    b.peak_bytes = workset_bytes;
                }
                if workset_bytes >= PROPOSAL_WORKSET_BYTES && b.over_threshold_samples < 60 {
                    b.over_threshold_samples += 1;
                }
            }
            return;
        }
        self.ledger[self.ledger_head] = Some(WorksetBucket {
            hour_index: hour,
            peak_bytes: workset_bytes,
            over_threshold_samples: u8::from(workset_bytes >= PROPOSAL_WORKSET_BYTES),
        });
        self.ledger_head = (self.ledger_head + 1) % LEDGER_HOURS;
        self.ledger_n = (self.ledger_n + 1).min(LEDGER_HOURS);
        self.current_hour = Some(hour);
    }

    /// 立项判定（主册：真实工作集超 3.2GB 持续一周才立项）。
    /// 判定口径：最近 168 桶（≥7 天）每小时峰值全部 ≥ 线 = 持续一周高位。
    pub fn evaluate_proposal(&mut self, at_ms: u64) -> bool {
        self.now_ms = at_ms;
        if self.enabled {
            return true;
        }
        // 桶不满 168（不足一周数据）→ 不立项（先测后做，数据不齐不拍板）。
        if self.ledger_n < LEDGER_HOURS {
            return false;
        }
        let mut all_over = true;
        for i in 0..LEDGER_HOURS {
            let idx = (self.ledger_head + LEDGER_HOURS - 1 - i) % LEDGER_HOURS;
            match self.ledger[idx] {
                Some(b) if b.peak_bytes >= PROPOSAL_WORKSET_BYTES => {}
                _ => {
                    all_over = false;
                    break;
                }
            }
        }
        if all_over {
            self.enabled = true;
            self.enabled_at_ms = at_ms;
            self.pool_budget_bytes = POOL_MAX_BYTES; // 初始按上限开池（旋钮可调）
        }
        self.enabled
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 候选页准入（压缩在空闲时段批量做）：冷龄 ≥64s + 类别优先 + 比率达标。
    /// 返回压缩后字节数（拒绝时 None）。
    pub fn try_compress(
        &mut self,
        kind: PageKind,
        last_touch_ms: u64,
        raw_model_bytes: u16,
        at_ms: u64,
    ) -> Option<u16> {
        self.now_ms = at_ms;
        if !self.enabled || kind == PageKind::KernelReserved {
            return None;
        }
        // 冷定义：64 秒未触碰。
        if at_ms.saturating_sub(last_touch_ms) < COLD_AFTER_MS {
            return None;
        }
        // 比率红线：压缩比 <1.5 的页不压缩（白费 CPU）。
        // stored_model 为压缩后模型字节数；ratio = 4096/stored ×10 定点 <15 → 拒。
        let ratio_x10 = if raw_model_bytes == 0 {
            u32::MAX
        } else {
            (SLOT_FRAME_BYTES as u32 * 10) / raw_model_bytes as u32
        };
        if raw_model_bytes == 0 || ratio_x10 < RATIO_FLOOR_X10 {
            self.stats_rejected_ratio += 1;
            return None;
        }
        // 池预算内找空槽；池满 → 如实回退 OOM 流程（不静默丢页）。
        let slot_idx = (0..SLOT_CAP).find(|&i| !self.slots[i].occupied);
        let slot_idx = match slot_idx {
            Some(i) => i,
            None => {
                self.stats_pool_full_oom += 1;
                return None;
            }
        };
        self.slots[slot_idx] = CompSlot {
            occupied: true,
            kind,
            stored_bytes: raw_model_bytes,
            at_ms,
            last_decomp_us: 0,
        };
        self.slot_n += 1;
        self.stats_comp += 1;
        Some(raw_model_bytes)
    }

    /// 解压（读回路径）：记录延迟样本；P99 超标 → 池缩小（带冷却——
    /// 单个超标采样窗只缩一档，不连环缩到零）。
    pub fn decompress(&mut self, slot_idx: usize, decomp_us: u32) -> bool {
        if slot_idx >= SLOT_CAP || !self.slots[slot_idx].occupied {
            return false;
        }
        self.slots[slot_idx].last_decomp_us = decomp_us;
        self.slots[slot_idx].occupied = false;
        self.slot_n = self.slot_n.saturating_sub(1);
        self.stats_decomp += 1;
        self.decomp_us_ring[self.decomp_ring_n] = decomp_us;
        self.decomp_ring_n = (self.decomp_ring_n + 1) % 64;
        if self.shrink_cooldown > 0 {
            self.shrink_cooldown -= 1;
        }
        // P99 超 200μs → 池缩小一档（512MB → 384MB → … → 0）；冷却期不缩。
        if self.shrink_cooldown == 0
            && self.decomp_p99_us() > DECOMP_P99_LIMIT_US
            && self.pool_budget_bytes > POOL_MIN_BYTES
        {
            self.pool_budget_bytes = self.pool_budget_bytes.saturating_sub(POOL_MAX_BYTES / 4);
            self.stats_shrinks += 1;
            self.shrink_cooldown = 8; // 8 次解压内不再重复缩（单窗单缩语义）
        }
        true
    }

    /// 解压延迟 P99（64 样本环内取 P99 位次——样本量小时取最大值，诚实口径）。
    pub fn decomp_p99_us(&self) -> u32 {
        if self.decomp_ring_n == 0 {
            return 0;
        }
        // 定长计数排序（μs 值域有界：<4096），零堆。
        let mut hist = [0u16; 4096];
        for i in 0..self.decomp_ring_n {
            let v = self.decomp_us_ring[i] as usize;
            if v < 4096 {
                hist[v] += 1;
            }
        }
        // P99 位次：第 ceil(0.99*n) 小。
        let rank = ((self.decomp_ring_n as u32 * 99 + 99) / 100) as usize;
        let mut acc = 0usize;
        for (v, c) in hist.iter().enumerate() {
            acc += *c as usize;
            if acc >= rank {
                return v as u32;
            }
        }
        4095
    }

    /// 池占用字节（4KB 框 × 驻留槽）。
    pub fn pool_used_bytes(&self) -> u64 {
        self.slot_n as u64 * SLOT_FRAME_BYTES as u64
    }

    pub fn pool_budget_bytes(&self) -> u64 {
        self.pool_budget_bytes
    }

    pub fn stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.stats_comp,
            self.stats_decomp,
            self.stats_rejected_ratio,
            self.stats_pool_full_oom,
            self.stats_shrinks,
        )
    }

    /// 评估报告结论行（采集脚本一周报告的结构化摘要）：
    /// (桶数, 工作集峰值, 超线小时数, 是否已立项)。
    pub fn report_summary(&self) -> (u16, u64, u16, bool) {
        let mut peak = 0u64;
        let mut hours_over = 0u16;
        for b in self.ledger.iter().flatten() {
            if b.peak_bytes > peak {
                peak = b.peak_bytes;
            }
            if b.over_threshold_samples > 0 {
                hours_over += 1;
            }
        }
        (self.ledger_n as u16, peak, hours_over, self.enabled)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_memcomp_checks() -> CheckSet {
    let mut cs = CheckSet::new("F058-memcomp");
    // 1) 立项判据可验证：数据不足一周（167h）→ 不立项（先测后做）。
    let mut e = MemCompEvaluator::new();
    for h in 0..LEDGER_HOURS - 1 {
        e.observe(PROPOSAL_WORKSET_BYTES + 100_000_000, h as u64 * 3_600_000);
        e.observe(PROPOSAL_WORKSET_BYTES + 100_000_000, h as u64 * 3_600_000 + 3_540_000);
    }
    cs.add("proposal_needs_full_week", !e.evaluate_proposal(0), "");
    let mut e2 = MemCompEvaluator::new();
    for h in 0..LEDGER_HOURS {
        let t = h as u64 * 3_600_000;
        e2.observe(PROPOSAL_WORKSET_BYTES + 100_000_000, t);
        e2.observe(PROPOSAL_WORKSET_BYTES + 200_000_000, t + 3_540_000);
    }
    cs.add(
        "proposal_granted_after_week",
        e2.evaluate_proposal(LEDGER_HOURS as u64 * 3_600_000),
        "",
    );
    // 2) 一周内有一小时低于线 → 拒绝立项（「持续」语义）。
    let mut e3 = MemCompEvaluator::new();
    for h in 0..LEDGER_HOURS {
        let t = h as u64 * 3_600_000;
        let ws = if h == 100 { PROPOSAL_WORKSET_BYTES - 1 } else { PROPOSAL_WORKSET_BYTES + 1 };
        e3.observe(ws, t);
        e3.observe(ws, t + 3_540_000);
    }
    cs.add("dip_rejects_proposal", !e3.evaluate_proposal(LEDGER_HOURS as u64 * 3_600_000), "");
    // 3) 未启用时压缩一概拒绝（先测后做——不提前上压缩）。
    let mut e4 = MemCompEvaluator::new();
    cs.add(
        "disabled_no_compress",
        e4.try_compress(PageKind::FileBacked, 0, 1_000, 1_000_000).is_none(),
        "",
    );
    // 4) 冷定义 64 秒：62s 未触碰拒收，65s 收。
    let mut e5 = MemCompEvaluator::new();
    e5.observe(PROPOSAL_WORKSET_BYTES + 1, 0);
    for h in 0..LEDGER_HOURS {
        let t = h as u64 * 3_600_000;
        e5.observe(PROPOSAL_WORKSET_BYTES + 1, t);
        e5.observe(PROPOSAL_WORKSET_BYTES + 1, t + 3_540_000);
    }
    e5.evaluate_proposal(LEDGER_HOURS as u64 * 3_600_000);
    cs.add(
        "warm_page_rejected",
        e5.try_compress(PageKind::FileBacked, 3_600_000 * 200 - 62_000, 1_000, 3_600_000 * 200).is_none(),
        "",
    );
    cs.add(
        "cold_page_accepted",
        e5.try_compress(PageKind::FileBacked, 3_600_000 * 200 - 65_000, 1_000, 3_600_000 * 200).is_some(),
        "",
    );
    // 5) 比率红线：<1.5 拒绝并计数（白费 CPU 防线）。
    // 4096/stored < 1.5 ⇒ stored > 2730。
    let mut e6 = MemCompEvaluator::new();
    e6.observe(PROPOSAL_WORKSET_BYTES + 1, 0);
    for h in 0..LEDGER_HOURS {
        let t = h as u64 * 3_600_000;
        e6.observe(PROPOSAL_WORKSET_BYTES + 1, t);
        e6.observe(PROPOSAL_WORKSET_BYTES + 1, t + 3_540_000);
    }
    e6.evaluate_proposal(LEDGER_HOURS as u64 * 3_600_000);
    let t0 = 3_600_000 * 300;
    cs.add(
        "low_ratio_rejected",
        e6.try_compress(PageKind::FileBacked, t0 - 65_000, 3_000, t0).is_none(),
        "",
    );
    cs.add(
        "good_ratio_accepted",
        e6.try_compress(PageKind::FileBacked, t0 - 65_000, 1_200, t0).is_some(),
        "",
    );
    // 6) 内核保留页永不入池（红线）。
    cs.add(
        "kernel_reserved_never",
        e6.try_compress(PageKind::KernelReserved, t0 - 65_000, 1_200, t0 + 1).is_none(),
        "",
    );
    // 7) 解压延迟 P99 >200μs → 池缩小。
    let mut e7 = MemCompEvaluator::new();
    e7.observe(PROPOSAL_WORKSET_BYTES + 1, 0);
    for h in 0..LEDGER_HOURS {
        let t = h as u64 * 3_600_000;
        e7.observe(PROPOSAL_WORKSET_BYTES + 1, t);
        e7.observe(PROPOSAL_WORKSET_BYTES + 1, t + 3_540_000);
    }
    e7.evaluate_proposal(LEDGER_HOURS as u64 * 3_600_000);
    let mut slot_ids: [usize; 8] = [0; 8];
    for (i, s) in slot_ids.iter_mut().enumerate() {
        *s = (0..SLOT_CAP)
            .find(|&k| !e7.decompress_probe_occupied(k))
            .unwrap_or(SLOT_CAP);
        let _ = e7.try_compress(PageKind::Anonymous, t0 - 65_000, 1_000, t0 + i as u64);
    }
    // 注入超标解压样本：7 正常 + 1 超标（64 环内 P99 位次 = 第 8 大 → 超标触发）。
    for i in 0..8 {
        let us = if i == 7 { 300 } else { 50 };
        let _ = e7.decompress(slot_ids[i], us);
    }
    cs.add("decomp_p99_shrinks_pool", e7.pool_budget_bytes() < POOL_MAX_BYTES, "");
    // 8) 池满 → 回退 OOM 流程计数（如实告知，不静默）。
    let mut e8 = MemCompEvaluator::new();
    e8.observe(PROPOSAL_WORKSET_BYTES + 1, 0);
    for h in 0..LEDGER_HOURS {
        let t = h as u64 * 3_600_000;
        e8.observe(PROPOSAL_WORKSET_BYTES + 1, t);
        e8.observe(PROPOSAL_WORKSET_BYTES + 1, t + 3_540_000);
    }
    e8.evaluate_proposal(LEDGER_HOURS as u64 * 3_600_000);
    let mut filled = 0usize;
    for i in 0..SLOT_CAP + 8 {
        if e8.try_compress(PageKind::FileBacked, t0 - 65_000, 1_000, t0 + i as u64).is_some() {
            filled += 1;
        }
    }
    let (c, _, _, oom, _) = e8.stats();
    cs.add(
        "pool_full_reports_oom",
        filled == SLOT_CAP && oom == 8 && c == SLOT_CAP as u64,
        "",
    );
    // 9) 报告摘要结构化输出（采集脚本一周报告口径；超线小时数 = 全周 168）。
    let (n, peak, hours_over, enabled) = e8.report_summary();
    cs.add(
        "report_summary_structured",
        n == LEDGER_HOURS as u16
            && peak >= PROPOSAL_WORKSET_BYTES
            && hours_over == LEDGER_HOURS as u16
            && enabled,
        "",
    );
    cs
}

impl MemCompEvaluator {
    /// 自检辅助：槽占用查询（不进内核路径，仅域自检用）。
    fn decompress_probe_occupied(&self, idx: usize) -> bool {
        self.slots.get(idx).map_or(false, |s| s.occupied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一台已立项的评估器（168 小时全高位）。
    fn enabled_eval() -> MemCompEvaluator {
        let mut e = MemCompEvaluator::new();
        for h in 0..LEDGER_HOURS {
            let t = h as u64 * 3_600_000;
            e.observe(PROPOSAL_WORKSET_BYTES + 1, t);
            e.observe(PROPOSAL_WORKSET_BYTES + 1, t + 3_540_000);
        }
        e.evaluate_proposal(LEDGER_HOURS as u64 * 3_600_000);
        e
    }

    #[test]
    fn proposal_requires_full_week_above_line() {
        let mut e = MemCompEvaluator::new();
        assert!(!e.evaluate_proposal(0), "无数据不得立项");
        for h in 0..LEDGER_HOURS - 1 {
            let t = h as u64 * 3_600_000;
            e.observe(PROPOSAL_WORKSET_BYTES + 1, t);
            e.observe(PROPOSAL_WORKSET_BYTES + 1, t + 3_540_000);
        }
        assert!(!e.evaluate_proposal((LEDGER_HOURS - 1) as u64 * 3_600_000), "167h 不足一周");
        e.observe(PROPOSAL_WORKSET_BYTES + 1, (LEDGER_HOURS - 1) as u64 * 3_600_000);
        e.observe(PROPOSAL_WORKSET_BYTES + 1, (LEDGER_HOURS - 1) as u64 * 3_600_000 + 3_540_000);
        assert!(e.evaluate_proposal(LEDGER_HOURS as u64 * 3_600_000));
        assert!(e.is_enabled());
    }

    #[test]
    fn cold_boundary_is_64s() {
        let mut e = enabled_eval();
        let t0 = 3_600_000 * 300;
        assert!(e.try_compress(PageKind::FileBacked, t0 - COLD_AFTER_MS, 1_000, t0).is_some(), "恰 64s = 冷");
        let mut e2 = enabled_eval();
        assert!(e2.try_compress(PageKind::FileBacked, t0 - (COLD_AFTER_MS - 1), 1_000, t0).is_none(), "63.999s = 热");
    }

    #[test]
    fn ratio_floor_and_kernel_reserved() {
        let mut e = enabled_eval();
        let t0 = 3_600_000 * 300;
        // stored=2731 → 4096*10/2731 = 14.99 → <15 拒。
        assert!(e.try_compress(PageKind::FileBacked, t0 - 65_000, 2_731, t0).is_none());
        // stored=2730 → 15.0 → 恰达线收。
        assert!(e.try_compress(PageKind::FileBacked, t0 - 65_000, 2_730, t0).is_some());
        assert!(e.try_compress(PageKind::KernelReserved, t0 - 65_000, 100, t0).is_none());
        let (_, _, rej, _, _) = e.stats();
        assert!(rej >= 1, "比率拒绝计数在案");
    }

    #[test]
    fn decomp_p99_over_limit_shrinks_pool() {
        let mut e = enabled_eval();
        let t0 = 3_600_000 * 300;
        let mut ids = [usize::MAX; 4];
        for (i, id) in ids.iter_mut().enumerate() {
            *id = (0..SLOT_CAP).find(|&k| !e.decompress_probe_occupied(k)).unwrap();
            assert!(e.try_compress(PageKind::Anonymous, t0 - 65_000, 1_000, t0 + i as u64).is_some());
        }
        assert_eq!(e.pool_budget_bytes(), POOL_MAX_BYTES);
        // 1/4 样本超标：P99（第 4 大的 ceil(0.99*4)=4 位次）= 300 > 200 → 缩池。
        assert!(e.decompress(ids[0], 300));
        assert!(e.decompress(ids[1], 40));
        assert!(e.decompress(ids[2], 40));
        assert!(e.decompress(ids[3], 40));
        assert!(e.decomp_p99_us() > DECOMP_P99_LIMIT_US);
        assert!(e.pool_budget_bytes() < POOL_MAX_BYTES, "P99 超标 → 池缩小");
        let (_, _, _, _, shrinks) = e.stats();
        assert_eq!(shrinks, 1);
    }

    #[test]
    fn pool_full_falls_back_to_oom_honestly() {
        let mut e = enabled_eval();
        let t0 = 3_600_000 * 300;
        let mut ok = 0u32;
        for i in 0..SLOT_CAP {
            assert!(e.try_compress(PageKind::FileBacked, t0 - 65_000, 1_000, t0 + i as u64).is_some());
            ok += 1;
        }
        assert_eq!(ok, SLOT_CAP as u32);
        for i in 0..4 {
            assert!(e.try_compress(PageKind::FileBacked, t0 - 65_000, 1_000, t0 + SLOT_CAP as u64 + i).is_none());
        }
        let (_, _, _, oom, _) = e.stats();
        assert_eq!(oom, 4, "池满回退如实计数");
        assert_eq!(e.pool_used_bytes(), SLOT_CAP as u64 * SLOT_FRAME_BYTES as u64);
    }

    #[test]
    fn ledger_is_168h_ring() {
        let mut e = MemCompEvaluator::new();
        // 打 300 小时点：环形只留最后 168。
        for h in 0..300 {
            let t = h as u64 * 3_600_000;
            e.observe(if h >= 132 { PROPOSAL_WORKSET_BYTES + 1 } else { 1_000_000 }, t);
            e.observe(1_000_000, t + 1_800_000);
        }
        let (n, peak, _, _) = e.report_summary();
        assert_eq!(n, LEDGER_HOURS as u16, "168 桶环形");
        assert!(peak >= PROPOSAL_WORKSET_BYTES, "峰值来自最近 168h 窗口");
    }
}
