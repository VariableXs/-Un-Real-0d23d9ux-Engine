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

// ===========================================================================
// v2 深化批（F058 · G-B-18）——压缩管线四闸 + 冷龄分类器
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-18 功能定义的实装细化，非新立项）：
// 1. LzSizeEstimator —— 压缩比预筛闸：LZSS 式匹配（3 字节序列 12 位哈希
//    链 + 逐候选字节比对），估出 stored 尺寸 → 比值 ×10 定点；低于
//    RATIO_FLOOR_X10（1.5）的页在进池前拒收——不可压页不浪费池预算
//    （stats_rejected_ratio 的前置闸，与主册「压缩比 <1.5 拒收」同源）。
// 2. ZeroPageGate —— 零页去重闸：全零 4K 页不经压缩直接归 8 字节哨兵
//    （比任何压缩输出都小）；一字节之差即非零（8 字节步进扫描）。
// 3. PageAger —— 冷龄分类器：cold→warm→hot 三级（触达升温、扫描降温），
//    与 PageKind（盘上副本属性）正交——「时间维度 × 副本维度」两维联合
//    决定入池次序：冷 + 文件备份页最先压。
// 4. DecompBudget —— 200μs 解压预算批分摊：批量解压按页逐个扣减预算，
//    超预算页滚入下一批（deferred 显式计数，不静默丢弃）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// LZ 最短匹配 3 字节（低于此值匹配令牌不划算）。
pub const LZ_MIN_MATCH: usize = 3;
/// LZ 单次匹配上限 64 字节（令牌 len 域 6 位）。
pub const LZ_MAX_MATCH: usize = 64;
/// 哈希链最大回溯 8 候选（估算器预算——超过即按字面量处理）。
pub const LZ_MAX_CHAIN: usize = 8;
/// 匹配令牌 3 字节（dist 12 位 ≤4095 + len 6 位 ≤63 → 18 位 → 3 字节如实计）。
pub const LZ_TOKEN_BYTES: u32 = 3;
/// 零页哨兵 8 字节（比任何压缩输出都小的归零形态）。
pub const ZERO_SENTINEL_BYTES: u32 = 8;
/// 冷龄表代表页数（判据实装层样本集，实机随闸门扩到全物理页）。
pub const AGE_TABLE_CAP: usize = 256;
/// 解压批页数上限（单批预算分摊的批规模）。
pub const DECOMP_BATCH_MAX: usize = 16;

// ---------------------------------------------------------------------------
// 闸一：LZSS 式压缩比预估器（哈希链 + 字节级验证）
// ---------------------------------------------------------------------------

/// 单页压缩预估结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LzEstimate {
    /// 字面量字节数。
    pub literals: u32,
    /// 匹配令牌数。
    pub matches: u32,
    /// 匹配覆盖字节数（压缩有效载荷）。
    pub matched_bytes: u32,
    /// 预估压缩后字节数 = literals + matches × LZ_TOKEN_BYTES。
    pub stored_bytes: u32,
    /// 压缩比 ×10（4096×10 / stored；stored=0 不可能——下限 1 字面量）。
    pub ratio_x10: u32,
}

impl LzEstimate {
    /// 进池闸：比值达到主册下限（×10 定点）才允许占用池预算。
    pub fn admissible(&self) -> bool {
        self.ratio_x10 >= RATIO_FLOOR_X10
    }
}

/// 3 字节序列 12 位哈希（FNV-1a 变体，零堆零浮点）。
fn lz_hash3(data: &[u8], i: usize) -> usize {
    let mut h: u32 = 0x811C_9DC5;
    for k in 0..LZ_MIN_MATCH {
        h ^= data[i + k] as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    ((h >> 8) & 0x0FFF) as usize // 12 位 → 4096 桶（与帧等宽，链短）
}

/// LZSS 式单页压缩尺寸预估器（4K 框定长；head/prev 各 4K 槽零堆）。
pub struct LzSizeEstimator {
    head: [u16; SLOT_FRAME_BYTES],
    prev: [u16; SLOT_FRAME_BYTES],
}

const LZ_NIL: u16 = 0xFFFF;

impl LzSizeEstimator {
    pub const fn new() -> Self {
        LzSizeEstimator {
            head: [LZ_NIL; SLOT_FRAME_BYTES],
            prev: [LZ_NIL; SLOT_FRAME_BYTES],
        }
    }

    /// 预估一帧（≤4096 字节）的压缩后尺寸。匹配候选全部逐字节比对
    /// （哈希只用于定位，正确性靠比对保证——正确性优先于速度，与
    /// F054 SIMD 判据同一立场）。
    pub fn estimate(&mut self, frame: &[u8]) -> LzEstimate {
        self.head = [LZ_NIL; SLOT_FRAME_BYTES];
        self.prev = [LZ_NIL; SLOT_FRAME_BYTES];
        let len = frame.len().min(SLOT_FRAME_BYTES);
        let mut literals: u32 = 0;
        let mut matches: u32 = 0;
        let mut matched_bytes: u32 = 0;
        let mut i = 0usize;
        while i < len {
            let mut best_len = 0usize;
            if i + LZ_MIN_MATCH <= len {
                let h = lz_hash3(frame, i);
                let mut cand = self.head[h];
                let mut chain = 0usize;
                while cand != LZ_NIL && chain < LZ_MAX_CHAIN {
                    let j = cand as usize;
                    if j >= i {
                        break; // 链只含更早位置（prev 单调插入保证）
                    }
                    let max_len = (len - i).min(LZ_MAX_MATCH);
                    let mut l = 0usize;
                    while l < max_len && frame[j + l] == frame[i + l] {
                        l += 1;
                    }
                    if l > best_len {
                        best_len = l;
                        if l >= LZ_MAX_MATCH {
                            break;
                        }
                    }
                    cand = self.prev[j];
                    chain += 1;
                }
            }
            if best_len >= LZ_MIN_MATCH {
                // 逐字节比对已保证 frame[best_pos..best_pos+best_len] ==
                // frame[i..i+best_len]——匹配语义正确性在此成立。
                matches += 1;
                matched_bytes += best_len as u32;
                // 匹配覆盖的每个位置都入哈希链（后续位置可引用它们）。
                let ins_end = (i + best_len).min(len.saturating_sub(LZ_MIN_MATCH) + 1);
                let mut k = i;
                while k < ins_end {
                    let h = lz_hash3(frame, k);
                    self.prev[k] = self.head[h];
                    self.head[h] = k as u16;
                    k += 1;
                }
                i += best_len;
            } else {
                if i + LZ_MIN_MATCH <= len {
                    let h = lz_hash3(frame, i);
                    self.prev[i] = self.head[h];
                    self.head[h] = i as u16;
                }
                literals += 1;
                i += 1;
            }
        }
        let stored = literals + matches * LZ_TOKEN_BYTES;
        let ratio_x10 = (len as u32) * 10 / stored.max(1);
        LzEstimate {
            literals,
            matches,
            matched_bytes,
            stored_bytes: stored,
            ratio_x10,
        }
    }
}

// ---------------------------------------------------------------------------
// 闸二：零页去重（8 字节哨兵 + 节省账）
// ---------------------------------------------------------------------------

/// 零页判定与节省账（全零页无需压缩——哨兵即最小形态）。
pub struct ZeroPageGate {
    zero_pages: u64,
    non_zero_pages: u64,
    /// 零页去重累计节省字节（4096 − 8 每页）。
    bytes_saved: u64,
}

impl ZeroPageGate {
    pub const fn new() -> Self {
        ZeroPageGate {
            zero_pages: 0,
            non_zero_pages: 0,
            bytes_saved: 0,
        }
    }

    /// 全零判定：8 字节步进（等价逐字节扫描——一字节之差即非零）。
    pub fn is_zero_page(frame: &[u8]) -> bool {
        let len = frame.len().min(SLOT_FRAME_BYTES);
        let mut i = 0usize;
        while i + 8 <= len {
            let acc = frame[i] as u32
                | (frame[i + 1] as u32)
                | (frame[i + 2] as u32)
                | (frame[i + 3] as u32)
                | (frame[i + 4] as u32)
                | (frame[i + 5] as u32)
                | (frame[i + 6] as u32)
                | (frame[i + 7] as u32);
            if acc != 0 {
                return false;
            }
            i += 8;
        }
        while i < len {
            if frame[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 页过闸：分类入账并返回节省字节（非零页节省 0——走压缩管线）。
    pub fn admit(&mut self, frame: &[u8]) -> u64 {
        if Self::is_zero_page(frame) {
            self.zero_pages += 1;
            self.bytes_saved += (SLOT_FRAME_BYTES as u64) - (ZERO_SENTINEL_BYTES as u64);
            (SLOT_FRAME_BYTES as u64) - (ZERO_SENTINEL_BYTES as u64)
        } else {
            self.non_zero_pages += 1;
            0
        }
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.zero_pages, self.non_zero_pages, self.bytes_saved)
    }
}

// ---------------------------------------------------------------------------
// 闸三：冷龄分类器（时间维度，与 PageKind 副本维度正交）
// ---------------------------------------------------------------------------

/// 冷龄三档（0=hot 1=warm 2=cold；数值越大越冷——排序友好）。
pub const AGE_HOT: u8 = 0;
pub const AGE_WARM: u8 = 1;
pub const AGE_COLD: u8 = 2;

/// 冷龄分类器：触达升温（−2 档，两次触达从冷到热）、扫描降温（+1 档）。
/// 初始即 cold（保守——未见过的页不当热页压）。
pub struct PageAger {
    cls: [u8; AGE_TABLE_CAP],
    touches: [u32; AGE_TABLE_CAP],
    sweeps: u64,
}

impl PageAger {
    pub const fn new() -> Self {
        PageAger {
            cls: [AGE_COLD; AGE_TABLE_CAP],
            touches: [0; AGE_TABLE_CAP],
            sweeps: 0,
        }
    }

    /// 触达：升温 2 档（饱和于 hot），计数入账。越界 idx 安全拒绝。
    pub fn touch(&mut self, idx: usize) -> bool {
        if idx >= AGE_TABLE_CAP {
            return false;
        }
        self.cls[idx] = self.cls[idx].saturating_sub(2);
        self.touches[idx] = self.touches[idx].wrapping_add(1);
        true
    }

    /// 一轮扫描：全体降温 1 档（饱和于 cold）。
    pub fn sweep(&mut self) {
        for c in self.cls.iter_mut() {
            *c = (*c + 1).min(AGE_COLD);
        }
        self.sweeps += 1;
    }

    pub fn class_of(&self, idx: usize) -> Option<u8> {
        self.cls.get(idx).copied()
    }

    pub fn touch_count(&self, idx: usize) -> u32 {
        if idx < AGE_TABLE_CAP {
            self.touches[idx]
        } else {
            0
        }
    }

    /// 压缩候选数（cold 页——只有冷页值得压）。
    pub fn cold_count(&self) -> u32 {
        self.cls.iter().filter(|c| **c == AGE_COLD).count() as u32
    }

    /// 入池次序分（×100 定点）：文件备份页 +40 / 匿名 +20；冷 +40 / 温 +20 /
    /// 热 +0——「冷且文件备份」最高分（主册：重读成本高者先压）。
    /// 内核保留页恒 0 分（红线：永不入候选——冷龄再高也不参评）。
    pub fn priority_score(&self, idx: usize, kind: PageKind) -> u32 {
        if kind == PageKind::KernelReserved {
            return 0; // 红线优先于一切评分
        }
        let age_score = match self.cls.get(idx).copied() {
            Some(AGE_COLD) => 40,
            Some(AGE_WARM) => 20,
            _ => 0,
        };
        let kind_score = match kind {
            PageKind::FileBacked => 40,
            PageKind::Anonymous => 20,
            PageKind::KernelReserved => 0, // 不可达（上方已拦截）
        };
        age_score + kind_score
    }

    pub fn sweeps(&self) -> u64 {
        self.sweeps
    }
}

// ---------------------------------------------------------------------------
// 闸四：解压预算批分摊（200μs 预算逐页扣减，超线滚入下一批）
// ---------------------------------------------------------------------------

/// 解压预算编排器：一批页请求累计不超过 DECOMP_P99_LIMIT_US。
pub struct DecompBudget {
    used_us: u32,
    admitted: usize,
    deferred: u32,
    batches: u64,
}

impl DecompBudget {
    pub const fn new() -> Self {
        DecompBudget {
            used_us: 0,
            admitted: 0,
            deferred: 0,
            batches: 0,
        }
    }

    /// 提交一页解压请求：预算内准入，超线延迟（返回 false = 滚入下批）。
    pub fn request(&mut self, decomp_us: u32) -> bool {
        if self.used_us + decomp_us <= DECOMP_P99_LIMIT_US {
            self.used_us += decomp_us;
            self.admitted += 1;
            true
        } else {
            self.deferred += 1;
            false
        }
    }

    /// 批收口：used 清零、批号递增（延迟页由调用方在下一批重新提交）。
    pub fn close_batch(&mut self) {
        self.used_us = 0;
        self.admitted = 0;
        self.batches += 1;
    }

    pub fn used_us(&self) -> u32 {
        self.used_us
    }

    pub fn admitted_now(&self) -> usize {
        self.admitted
    }

    pub fn deferred_total(&self) -> u32 {
        self.deferred
    }

    pub fn batches(&self) -> u64 {
        self.batches
    }
}

// ---------------------------------------------------------------------------
// 页路径裁决（三闸串联：零页 → 不可压 → 可压入池）
// ---------------------------------------------------------------------------

/// 页进入压缩管线的裁决路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagePath {
    /// 全零页：归 8 字节哨兵，不占压缩池。
    ZeroDedup,
    /// 可压：比值达标，入池。
    CompressAdmit,
    /// 不可压：比值低于下限，进池前拒收。
    IncompressibleReject,
}

/// 三闸串联裁决（零页闸 → LZ 预筛闸 → 准入判定）——调用方按路径分流。
pub fn route_page(est: &mut LzSizeEstimator, gate: &mut ZeroPageGate, frame: &[u8]) -> (PagePath, u32) {
    if gate.admit(frame) > 0 {
        return (PagePath::ZeroDedup, 0);
    }
    let est = est.estimate(frame);
    if est.admissible() {
        (PagePath::CompressAdmit, est.ratio_x10)
    } else {
        (PagePath::IncompressibleReject, est.ratio_x10)
    }
}

// ---------------------------------------------------------------------------
// 深化批自检（run_memcomp_deep_checks · 与主检 merge 后一行在账）
// ---------------------------------------------------------------------------

/// xorshift32 伪随机（测试/判据用确定性随机源——零依赖可复现）。
pub fn xorshift32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// 深化批自检：压缩管线四闸 + 冷龄分类器逐条实摆。
pub fn run_memcomp_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F058-memcomp-deep");

    // ── 闸一：LZSS 预估器 ──
    // 1) 高重复数据强压缩（16 字节周期 → ≥10:1，×10 定点 ≥ 100）。
    let mut est = LzSizeEstimator::new();
    let mut frame = [0u8; SLOT_FRAME_BYTES];
    for (i, b) in frame.iter_mut().enumerate() {
        *b = (i % 16) as u8;
    }
    let e1 = est.estimate(&frame);
    cs.add("lz_pattern_ratio_ge_10x", e1.ratio_x10 >= 100, "");
    // 2) 预估账自洽：stored == literals + matches×3（令牌公式逐笔对得上）。
    cs.add(
        "lz_stored_formula_consistent",
        e1.stored_bytes == e1.literals + e1.matches * LZ_TOKEN_BYTES,
        "",
    );
    // 3) 伪随机数据不可压（比值 <2:1——诚实预估，不虚报压缩收益）。
    let mut rnd: u32 = 0x1234_5678;
    let mut rand_frame = [0u8; SLOT_FRAME_BYTES];
    for chunk in rand_frame.chunks_mut(4) {
        let v = xorshift32(&mut rnd).to_le_bytes();
        for (dst, src) in chunk.iter_mut().zip(v.iter()) {
            *dst = *src;
        }
    }
    let e2 = est.estimate(&rand_frame);
    cs.add("lz_random_ratio_lt_2x", e2.ratio_x10 < 20, "");
    // 4) 准入闸：重复数据达标、随机数据拒收（比值下限 RATIO_FLOOR_X10）。
    cs.add("lz_admit_gate_two_sides", e1.admissible() && !e2.admissible(), "");
    // 5) 远距离匹配（后半 = 前半，2K 距离——匹配覆盖 ≥ 2048−64）。
    let mut half_frame = [0u8; SLOT_FRAME_BYTES];
    for i in 0..2048 {
        half_frame[i] = (i % 7) as u8;
        half_frame[2048 + i] = (i % 7) as u8;
    }
    let e3 = est.estimate(&half_frame);
    cs.add("lz_far_match_covered", e3.matched_bytes >= 2048 - 64, "");
    // 6) 空帧安全（除 1 保护——ratio 除法不崩）。
    let e4 = est.estimate(&[]);
    cs.add("lz_empty_frame_safe", e4.stored_bytes == 0 || e4.ratio_x10 >= 1, "");

    // ── 闸二：零页去重 ──
    let mut gate = ZeroPageGate::new();
    let zero_frame = [0u8; SLOT_FRAME_BYTES];
    let mut one_bit_frame = [0u8; SLOT_FRAME_BYTES];
    one_bit_frame[4095] = 1; // 一字节之差
    cs.add("zero_gate_detect", ZeroPageGate::is_zero_page(&zero_frame), "");
    cs.add("zero_gate_one_bit_reject", !ZeroPageGate::is_zero_page(&one_bit_frame), "");
    let saved = gate.admit(&zero_frame);
    cs.add(
        "zero_gate_saves_4088",
        saved == (SLOT_FRAME_BYTES as u64) - (ZERO_SENTINEL_BYTES as u64),
        "",
    );
    let _ = gate.admit(&one_bit_frame);
    let (zp, nz, tot) = gate.stats();
    cs.add("zero_gate_stats_ledger", zp == 1 && nz == 1 && tot == saved, "");

    // ── 闸三：冷龄分类器 ──
    let mut ager = PageAger::new();
    cs.add("ager_new_is_cold", ager.class_of(7) == Some(AGE_COLD), "");
    let _ = ager.touch(7);
    let _ = ager.touch(7);
    cs.add("ager_two_touches_hot", ager.class_of(7) == Some(AGE_HOT), "");
    ager.sweep();
    cs.add("ager_sweep_demotes", ager.class_of(7) == Some(AGE_WARM), "");
    cs.add("ager_cold_stays_cold", {
        ager.sweep();
        ager.sweep();
        ager.class_of(3) == Some(AGE_COLD) // 未触达页三轮后仍 cold（饱和）
    }, "");
    cs.add("ager_touch_out_of_range_safe", !ager.touch(AGE_TABLE_CAP), "");
    // 优先级分（触达+扫描后 3/7 均已冷）：冷+文件备份 = 80；冷+匿名 = 60；
    // 内核页 = 0（时间×副本两维联合 + 红线豁免）。
    cs.add(
        "ager_priority_two_dimensions",
        ager.priority_score(3, PageKind::FileBacked) == 80
            && ager.priority_score(7, PageKind::Anonymous) == 60
            && ager.priority_score(7, PageKind::KernelReserved) == 0,
        "",
    );
    cs.add("ager_cold_count_ledger", {
        let mut a2 = PageAger::new();
        let _ = a2.touch(0);
        let _ = a2.touch(1);
        a2.cold_count() == (AGE_TABLE_CAP - 2) as u32
    }, "");

    // ── 闸四：解压预算批分摊 ──
    let mut bud = DecompBudget::new();
    // 40μs × 5 页 = 200μs 恰满（边界精确——第 6 页延迟）。
    let mut all_admitted = true;
    for _ in 0..5 {
        all_admitted &= bud.request(40);
    }
    let sixth = bud.request(40);
    cs.add(
        "budget_exact_boundary",
        all_admitted && !sixth && bud.used_us() == 200 && bud.deferred_total() == 1,
        "",
    );
    bud.close_batch();
    cs.add(
        "budget_batch_reset",
        bud.used_us() == 0 && bud.batches() == 1 && bud.request(200),
        "",
    );

    // ── 三闸串联路由 ──
    let mut est2 = LzSizeEstimator::new();
    let mut gate2 = ZeroPageGate::new();
    let (p_zero, _) = route_page(&mut est2, &mut gate2, &zero_frame);
    let (p_rand, r_rand) = route_page(&mut est2, &mut gate2, &rand_frame);
    let (p_pat, r_pat) = route_page(&mut est2, &mut gate2, &frame);
    cs.add(
        "route_three_paths",
        p_zero == PagePath::ZeroDedup
            && p_rand == PagePath::IncompressibleReject
            && p_pat == PagePath::CompressAdmit,
        "",
    );
    cs.add("route_ratio_carried", r_pat >= 100 && r_rand < 20, "");
    // xorshift 复现性（确定性随机源——判据可复现）。
    let mut r1: u32 = 42;
    let a = xorshift32(&mut r1);
    let mut r2: u32 = 42;
    let b = xorshift32(&mut r2);
    cs.add("xorshift_reproducible", a == b, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn lz_estimator_matches_real_lzss_semantics() {
        let mut est = LzSizeEstimator::new();
        // ABCABC... 高周期数据：首个匹配后全帧走令牌路径。
        let mut f = [0u8; 512];
        for (i, b) in f.iter_mut().enumerate() {
            *b = b"ABC"[i % 3];
        }
        let e = est.estimate(&f);
        assert!(e.matches >= 1, "周期数据至少一个匹配");
        assert!(e.matched_bytes >= (f.len() - LZ_MIN_MATCH) as u32, "首 3 字节字面量后全匹配");
        assert_eq!(e.stored_bytes, e.literals + e.matches * LZ_TOKEN_BYTES);
    }

    #[test]
    fn lz_estimator_never_expands_beyond_frame() {
        let mut est = LzSizeEstimator::new();
        let mut rnd: u32 = 99;
        let mut f = [0u8; SLOT_FRAME_BYTES];
        for chunk in f.chunks_mut(4) {
            let v = xorshift32(&mut rnd).to_le_bytes();
            chunk.copy_from_slice(&v);
        }
        let e = est.estimate(&f);
        assert!(e.stored_bytes <= SLOT_FRAME_BYTES as u32, "随机数据最坏 = 全字面量，不膨胀");
    }

    #[test]
    fn zero_gate_step_scan_matches_byte_scan() {
        // 8 字节步进扫描与逐字节扫描结论一致（步进实现正确性）。
        let mut f = [0u8; SLOT_FRAME_BYTES];
        assert!(ZeroPageGate::is_zero_page(&f));
        f[100] = 0; // 仍全零
        assert!(ZeroPageGate::is_zero_page(&f));
        f[100] = 0x80;
        assert!(!ZeroPageGate::is_zero_page(&f));
        f[4095] = 1; // 尾部越出 8 字节步进的残余段
        assert!(!ZeroPageGate::is_zero_page(&f));
    }

    #[test]
    fn ager_promote_demote_cycle_stable() {
        let mut a = PageAger::new();
        for _ in 0..100 {
            let _ = a.touch(5);
        }
        assert_eq!(a.class_of(5), Some(AGE_HOT));
        assert_eq!(a.touch_count(5), 100);
        for _ in 0..10 {
            a.sweep();
        }
        assert_eq!(a.class_of(5), Some(AGE_COLD), "连续扫描后冷饱和");
    }

    #[test]
    fn budget_deferred_pages_are_counted_not_lost() {
        let mut b = DecompBudget::new();
        let mut deferred = 0;
        for _ in 0..20 {
            if !b.request(30) {
                deferred += 1;
            }
        }
        assert_eq!(deferred, b.deferred_total(), "延迟页账实一致");
        assert!(b.used_us() <= DECOMP_P99_LIMIT_US, "预算不被突破");
        assert!(b.admitted_now() >= 1);
    }

    #[test]
    fn route_page_zero_short_circuits_estimator() {
        // 零页短路：不过 LZ 闸（零页节省 > 任何压缩收益）。
        let mut est = LzSizeEstimator::new();
        let mut gate = ZeroPageGate::new();
        let f = [0u8; SLOT_FRAME_BYTES];
        let (path, ratio) = route_page(&mut est, &mut gate, &f);
        assert_eq!(path, PagePath::ZeroDedup);
        assert_eq!(ratio, 0, "零页不走压缩比值");
    }
}

// ===========================================================================
// v3 深化批（F058 · G-B-18）——配额治理 / 解压热点缓存 / 成本账 / 候选批次规划
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-18 功能定义的实装细化，非新立项）：
// 1. PoolQuota —— 压缩池按 PageKind 配额治理：文件备份页/匿名页各占配额
//    比例，超配额入池请求被拒（防单类页挤占全池——公平面）；
//    配额内回收按最冷龄优先。
// 2. DecompressCache —— 解压热点缓存：最近解压的页留 N 槽直查
//    （二次访问免解压——CPU 成本面），满后 LRU 逐出。
// 3. CompCostLedger —— 压缩/解压成本账：逐笔 CPU 成本（×100 定点 μs）
//    累计，摊销视图 = 总成本 / 净省字节（成本有效性可量化）。
// 4. CandidatePlanner —— 入池批次规划：按 priority_score 降序取 Top-K
//    冷页 + LZ 预筛通过者进批次单（预算内装满即停——规划不是承诺）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 配额类别（与 PageKind 对齐，KernelReserved 恒拒）。
pub const QUOTA_FILEBACKED: u8 = 0;
pub const QUOTA_ANON: u8 = 1;
/// 配额槽容量（代表集）。
pub const QUOTA_TABLE_CAP: usize = 64;
/// 解压热点缓存槽位。
pub const DECOMP_CACHE_CAP: usize = 16;
/// 批次规划单容量。
pub const PLAN_BATCH_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 深化一：压缩池配额治理
// ---------------------------------------------------------------------------

/// 配额治理器（两类别各自记账，比例配额 ×100 定点）。
pub struct PoolQuota {
    /// 各类配额比例（×100 定点百分比，合计 ≤100）。
    quota_pct: [u32; 2],
    used_pages: [u32; 2],
    total_budget: u32,
    rejected: u64,
}

impl PoolQuota {
    pub const fn new(total_budget: u32, file_pct: u32, anon_pct: u32) -> Self {
        PoolQuota {
            quota_pct: [file_pct, anon_pct],
            used_pages: [0; 2],
            total_budget,
            rejected: 0,
        }
    }

    fn limit(kind: u8) -> Option<usize> {
        match kind {
            QUOTA_FILEBACKED => Some(0),
            QUOTA_ANON => Some(1),
            _ => None,
        }
    }

    /// 申请入池：类别配额未满才放行（KernelReserved 恒拒——红线）。
    pub fn admit(&mut self, kind: u8, pages: u32) -> bool {
        let slot = match Self::limit(kind) {
            Some(s) => s,
            None => {
                self.rejected += 1;
                return false; // 内核页/未知类别——红线拒绝
            }
        } as usize;
        let limit = (self.total_budget as u64) * (self.quota_pct[slot] as u64) / 100;
        if self.used_pages[slot] as u64 + pages as u64 > limit {
            self.rejected += 1;
            return false;
        }
        self.used_pages[slot] += pages;
        true
    }

    /// 回收（解压后写回原页 → 池页释放）。
    pub fn release(&mut self, kind: u8, pages: u32) -> bool {
        match Self::limit(kind) {
            Some(slot) => {
                self.used_pages[slot] = self.used_pages[slot].saturating_sub(pages);
                true
            }
            None => false,
        }
    }

    /// 类别使用率 ×100。
    pub fn usage_pct(&self, kind: u8) -> u32 {
        match Self::limit(kind) {
            Some(slot) => {
                let limit = (self.total_budget as u64) * (self.quota_pct[slot] as u64) / 100;
                if limit == 0 {
                    return 0;
                }
                (self.used_pages[slot] as u64 * 100 / limit) as u32
            }
            None => 0,
        }
    }

    pub fn stats(&self) -> ([u32; 2], u64) {
        (self.used_pages, self.rejected)
    }
}

// ---------------------------------------------------------------------------
// 深化二：解压热点缓存（LRU N 槽）
// ---------------------------------------------------------------------------

/// 解压热点缓存：槽 id 键控（slot_idx），命中免解压。
pub struct DecompressCache {
    slots: [Option<(usize, u64)>; DECOMP_CACHE_CAP], // (slot_idx, last_touch)
    n: usize,
    clock: u64,
    hits: u64,
    misses: u64,
}

impl DecompressCache {
    pub const fn new() -> Self {
        DecompressCache {
            slots: [None; DECOMP_CACHE_CAP],
            n: 0,
            clock: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// 查缓存：命中刷新时间戳。
    pub fn lookup(&mut self, slot_idx: usize) -> bool {
        self.clock += 1;
        for e in self.slots.iter_mut().flatten() {
            if e.0 == slot_idx {
                e.1 = self.clock;
                self.hits += 1;
                return true;
            }
        }
        self.misses += 1;
        false
    }

    /// 解压完成后登记（未命中才登记——命中不重复入）。
    pub fn insert(&mut self, slot_idx: usize) {
        if self.slots.iter().flatten().any(|e| e.0 == slot_idx) {
            return; // 已在缓存（lookup miss 但他人先 insert——幂等）
        }
        self.clock += 1;
        if self.n < DECOMP_CACHE_CAP {
            self.slots[self.n] = Some((slot_idx, self.clock));
            self.n += 1;
            return;
        }
        // LRU：最旧时间戳逐出。
        let mut oldest = 0usize;
        for (k, e) in self.slots.iter().enumerate() {
            if let Some((_, t)) = e {
                let base = match self.slots[oldest] {
                    Some((_, bt)) => bt,
                    None => u64::MAX,
                };
                if *t < base {
                    oldest = k;
                }
            }
        }
        self.slots[oldest] = Some((slot_idx, self.clock));
    }

    /// 页回收（release）时同步失效——缓存不得指向已释放槽。
    pub fn invalidate(&mut self, slot_idx: usize) -> bool {
        for e in self.slots.iter_mut() {
            if let Some((idx, _)) = e {
                if *idx == slot_idx {
                    *e = None;
                    // 压实留到遍历侧（invalidate 语义：洞即跳过）。
                    return true;
                }
            }
        }
        false
    }

    /// 命中率 ×100。
    pub fn hit_rate_pct(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        (self.hits * 100 / total) as u32
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }
}

// ---------------------------------------------------------------------------
// 深化三：压缩成本账
// ---------------------------------------------------------------------------

/// 成本账：逐笔 CPU μs ×100 定点累计 + 摊销视图。
pub struct CompCostLedger {
    comp_us_x100: u64,
    decomp_us_x100: u64,
    comp_count: u64,
    decomp_count: u64,
    /// 净省字节（压缩省下的——原始 − stored）。
    bytes_saved: u64,
}

impl CompCostLedger {
    pub const fn new() -> Self {
        CompCostLedger {
            comp_us_x100: 0,
            decomp_us_x100: 0,
            comp_count: 0,
            decomp_count: 0,
            bytes_saved: 0,
        }
    }

    pub fn record_comp(&mut self, us_x100: u32, bytes_saved: u64) {
        self.comp_us_x100 += us_x100 as u64;
        self.comp_count += 1;
        self.bytes_saved += bytes_saved;
    }

    pub fn record_decomp(&mut self, us_x100: u32) {
        self.decomp_us_x100 += us_x100 as u64;
        self.decomp_count += 1;
    }

    /// 摊销成本（×100 定点 μs/KB 省）：总 CPU 成本 / 净省 KB。
    /// 返回 None 当尚未省出 1KB（样本不足不摊销——不猜）。
    pub fn amortized_us_per_kb(&self) -> Option<u64> {
        if self.bytes_saved < 1024 {
            return None;
        }
        let total = self.comp_us_x100 + self.decomp_us_x100;
        Some(total * 1024 / self.bytes_saved)
    }

    /// 解压/压缩成本比 ×100（>100 = 解压比压缩贵——池策略输入）。
    pub fn decomp_comp_ratio_x100(&self) -> u64 {
        if self.comp_us_x100 == 0 {
            return 0;
        }
        self.decomp_us_x100 * 100 / self.comp_us_x100
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.comp_count, self.decomp_count, self.bytes_saved)
    }
}

// ---------------------------------------------------------------------------
// 深化四：候选批次规划器
// ---------------------------------------------------------------------------

/// 候选页（规划输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub slot_idx: u16,
    /// priority_score（冷龄×副本两维——复用 PageAger 评分口径）。
    pub score: u32,
    pub kind_quota: u8,
}

/// 批次规划：score 降序 Top-K 进单（插排稳定），满即停。
pub struct CandidatePlanner {
    plan: [Option<Candidate>; PLAN_BATCH_CAP],
    n: usize,
    planned_rounds: u64,
}

impl CandidatePlanner {
    pub const fn new() -> Self {
        CandidatePlanner {
            plan: [None; PLAN_BATCH_CAP],
            n: 0,
            planned_rounds: 0,
        }
    }

    /// 提交候选：按分数插入序位（降序），单满低分拒绝。
    pub fn submit(&mut self, c: Candidate) -> bool {
        // 找插入位（第一个 score < c.score 的位）。
        let mut pos = self.n;
        for k in 0..self.n {
            if let Some(p) = self.plan[k] {
                if p.score < c.score {
                    pos = k;
                    break;
                }
            }
        }
        if self.n >= PLAN_BATCH_CAP {
            if pos >= PLAN_BATCH_CAP {
                return false; // 单满且分数垫底——拒绝
            }
            // 单满但更优：挤掉队尾。
            self.n = PLAN_BATCH_CAP - 1;
        }
        // 后移。
        let mut k = self.n;
        while k > pos {
            self.plan[k] = self.plan[k - 1];
            k -= 1;
        }
        self.plan[pos] = Some(c);
        self.n += 1;
        true
    }

    /// 出单（清空并计数）。
    pub fn take_batch(&mut self) -> u32 {
        let n = self.n as u32;
        for s in self.plan.iter_mut() {
            *s = None;
        }
        self.n = 0;
        if n > 0 {
            self.planned_rounds += 1;
        }
        n
    }

    /// 单内是否降序（规划质量自证）。
    pub fn is_sorted_desc(&self) -> bool {
        for k in 1..self.n {
            if let (Some(a), Some(b)) = (self.plan[k - 1], self.plan[k]) {
                if a.score < b.score {
                    return false;
                }
            }
        }
        true
    }

    pub fn pending(&self) -> usize {
        self.n
    }

    pub fn rounds(&self) -> u64 {
        self.planned_rounds
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：配额 / 热点缓存 / 成本账 / 规划器逐条实摆。
pub fn run_memcomp_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F058-memcomp-v3");

    // ── 配额治理 ──
    let mut q = PoolQuota::new(100, 70, 30);
    cs.add("quota_file_ok", q.admit(QUOTA_FILEBACKED, 60), "");
    cs.add("quota_file_over", !q.admit(QUOTA_FILEBACKED, 11), ""); // 61 > 70? 不——61≤70 过
    let _ = q.admit(QUOTA_FILEBACKED, 10); // 70 满
    cs.add("quota_file_limit", !q.admit(QUOTA_FILEBACKED, 1) && q.usage_pct(QUOTA_FILEBACKED) == 100, "");
    cs.add("quota_anon_independent", q.admit(QUOTA_ANON, 30), "文件满不牵连匿名配额");
    cs.add("quota_kernel_refused", !q.admit(2, 1), ""); // 红线类别
    cs.add("quota_release_frees", {
        let _ = q.release(QUOTA_FILEBACKED, 20);
        q.usage_pct(QUOTA_FILEBACKED) == 71 // 50/70 = 71.4 → 71
    }, "");

    // ── 解压热点缓存 ──
    let mut dc = DecompressCache::new();
    cs.add("dc_miss_then_insert", !dc.lookup(5) && {
        dc.insert(5);
        dc.lookup(5)
    }, "");
    cs.add("dc_hit_rate_ledger", dc.stats() == (1, 1) && dc.hit_rate_pct() == 50, "");
    // LRU 逐出：灌满 16 槽，刷新 0 号，再插 → 最旧（1 号）出、0 号保留。
    let mut dc2 = DecompressCache::new();
    for k in 0..DECOMP_CACHE_CAP {
        dc2.insert(k);
    }
    let _ = dc2.lookup(0); // 0 号刷新为最新
    dc2.insert(DECOMP_CACHE_CAP); // 挤掉 1 号（时间戳最旧）
    cs.add("dc_lru_evicts_oldest", !dc2.lookup(1), "");
    cs.add("dc_lru_keeps_refreshed", dc2.lookup(0), "");
    cs.add("dc_invalidate_frees_slot", {
        let mut d = DecompressCache::new();
        d.insert(3);
        d.invalidate(3) && !d.lookup(3)
    }, "");

    // ── 成本账 ──
    let mut cl = CompCostLedger::new();
    cl.record_comp(200, 3 * 1024); // 压缩 2ms 省 3KB
    cl.record_decomp(100);
    cl.record_decomp(100);
    cs.add(
        "cost_amortized",
        cl.amortized_us_per_kb() == Some((200 + 200) * 1024 / 3072), // 133
        "",
    );
    cs.add("cost_below_1kb_unknown", CompCostLedger::new().amortized_us_per_kb().is_none(), "");
    cs.add("cost_ratio_ledger", cl.decomp_comp_ratio_x100() == 100, ""); // 200/200

    // ── 候选批次规划 ──
    let mut pl = CandidatePlanner::new();
    let _ = pl.submit(Candidate { slot_idx: 1, score: 50, kind_quota: 0 });
    let _ = pl.submit(Candidate { slot_idx: 2, score: 90, kind_quota: 0 });
    let _ = pl.submit(Candidate { slot_idx: 3, score: 70, kind_quota: 1 });
    cs.add(
        "plan_sorted_desc",
        pl.is_sorted_desc() && pl.pending() == 3, // 90,70,50
        "",
    );
    cs.add("plan_takes_top_first", pl.take_batch() == 3 && pl.pending() == 0, "");
    // 单满挤尾：灌 32 个低分 + 1 个高分 → 高分入单、队尾低分出局。
    let mut pl2 = CandidatePlanner::new();
    for k in 0..PLAN_BATCH_CAP {
        let _ = pl2.submit(Candidate { slot_idx: k as u16, score: 10, kind_quota: 0 });
    }
    cs.add(
        "plan_full_evicts_tail",
        pl2.submit(Candidate { slot_idx: 99, score: 99, kind_quota: 0 })
            && pl2.is_sorted_desc()
            && pl2.pending() == PLAN_BATCH_CAP,
        "",
    );
    cs.add("plan_low_score_refused_when_full", {
        let mut p3 = CandidatePlanner::new();
        for k in 0..PLAN_BATCH_CAP {
            let _ = p3.submit(Candidate { slot_idx: k as u16, score: 50, kind_quota: 0 });
        }
        !p3.submit(Candidate { slot_idx: 98, score: 10, kind_quota: 0 })
    }, "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn quota_zero_budget_rejects_all() {
        let mut q = PoolQuota::new(0, 70, 30);
        assert!(!q.admit(QUOTA_FILEBACKED, 1));
    }

    #[test]
    fn dc_invalidate_nonexistent_is_false() {
        let mut d = DecompressCache::new();
        assert!(!d.invalidate(42));
    }

    #[test]
    fn cost_ledger_zero_comp_ratio_zero() {
        assert_eq!(CompCostLedger::new().decomp_comp_ratio_x100(), 0);
    }

    #[test]
    fn planner_preserves_order_for_ties() {
        let mut p = CandidatePlanner::new();
        let _ = p.submit(Candidate { slot_idx: 1, score: 50, kind_quota: 0 });
        let _ = p.submit(Candidate { slot_idx: 2, score: 50, kind_quota: 0 });
        // 同分稳定：先入者在前（插入位取第一个 < 而非 ≤）。
        assert!(p.is_sorted_desc());
        let _ = p.take_batch();
        assert_eq!(p.rounds(), 1);
    }
}

// ===========================================================================
// v4 深化批（F058 · G-B-18）——池完整性走查 / 池快照编解码 / 迁移模拟器
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-18 功能定义的实装细化，非新立项）：
// 1. IntegrityWalker —— 池完整性走查：逐槽校验和（分配时登记，走查时
//    比对）——压缩数据静默损坏的防线（bit-flip 即检出）。
// 2. PoolSnapshotCodec —— 池状态序列化：槽表 → 定长字节 + FNV 尾
//    （休眠/唤醒间池状态快照的机制面——roundtrip 零漂移）。
// 3. MigrationSimulator —— 页迁移模拟：池满压力下的槽位迁移演练
//    （迁出最冷 → 迁入新页 → 迁移计数与成本入账——确定性 PRNG 驱动）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 走查槽容量。
pub const WALK_SLOT_CAP: usize = 64;
/// 迁移模拟轮数上界。
pub const MIGRATE_ROUNDS_CAP: u32 = 32;

// ---------------------------------------------------------------------------
// 深化一：池完整性走查
// ---------------------------------------------------------------------------

/// 完整性走查器（分配登记校验和 → 逐槽比对）。
pub struct IntegrityWalker {
    checksums: [Option<(u16, u32)>; WALK_SLOT_CAP], // (槽 id, FNV)
    n: usize,
    registered: u64,
    corrupted: u64,
}

impl IntegrityWalker {
    pub const fn new() -> Self {
        IntegrityWalker { checksums: [None; WALK_SLOT_CAP], n: 0, registered: 0, corrupted: 0 }
    }

    /// 登记：页入池时记数据校验和（data 4 字节代表窗——全量随闸门）。
    pub fn register(&mut self, slot_idx: u16, data: &[u8; 16]) -> bool {
        if self.n >= WALK_SLOT_CAP {
            return false;
        }
        let h = fnv16(data);
        for e in self.checksums.iter_mut().flatten() {
            if e.0 == slot_idx {
                e.1 = h; // 重压缩覆盖
                self.registered += 1;
                return true;
            }
        }
        self.checksums[self.n] = Some((slot_idx, h));
        self.n += 1;
        self.registered += 1;
        true
    }

    /// 走查一槽：数据校验和 vs 登记值。
    pub fn verify_slot(&mut self, slot_idx: u16, data: &[u8; 16]) -> bool {
        let h = fnv16(data);
        for e in self.checksums.iter_mut().flatten() {
            if e.0 == slot_idx {
                if e.1 == h {
                    return true;
                }
                self.corrupted += 1;
                return false;
            }
        }
        false // 未登记的槽——走查不通过（账外数据不可信）
    }

    /// 注销（解压后写回 → 槽空）。
    pub fn unregister(&mut self, slot_idx: u16) -> bool {
        for k in 0..self.n {
            if let Some((idx, _)) = self.checksums[k] {
                if idx == slot_idx {
                    self.checksums[k] = self.checksums[self.n - 1];
                    self.checksums[self.n - 1] = None;
                    self.n -= 1;
                    return true;
                }
            }
        }
        false
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        (self.registered, self.corrupted, self.n)
    }
}

/// FNV-1a 16 字节窗（域内共享）。
pub fn fnv16(data: &[u8; 16]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---------------------------------------------------------------------------
// 深化二：池快照编解码
// ---------------------------------------------------------------------------

/// 快照条目 13B：slot(2) + stored(2) + kind(1) + at_ms(8)。
pub const POOL_SNAP_ENTRY: usize = 13;
/// 快照最大槽。
pub const POOL_SNAP_MAX: usize = 24;

/// 池快照编码器。
pub struct PoolSnapshotCodec {
    buf: [u8; 4 + POOL_SNAP_MAX * POOL_SNAP_ENTRY + 4],
    len: usize,
}

/// 快照槽条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolSnapSlot {
    pub slot_idx: u16,
    pub stored_bytes: u16,
    pub kind: u8,
    pub at_ms: u64,
}

impl PoolSnapshotCodec {
    pub fn encode(slots: &[PoolSnapSlot]) -> Self {
        let mut c = PoolSnapshotCodec {
            buf: [0; 4 + POOL_SNAP_MAX * POOL_SNAP_ENTRY + 4],
            len: 0,
        };
        let n = slots.len().min(POOL_SNAP_MAX);
        c.buf[0] = (n & 0xFF) as u8;
        c.buf[1] = (n >> 8) as u8;
        c.buf[2] = 0;
        c.buf[3] = 0;
        c.len = 4;
        for s in slots.iter().take(n) {
            let b = c.len;
            c.buf[b..b + 2].copy_from_slice(&s.slot_idx.to_le_bytes());
            c.buf[b + 2..b + 4].copy_from_slice(&s.stored_bytes.to_le_bytes());
            c.buf[b + 4] = s.kind;
            c.buf[b + 5..b + 13].copy_from_slice(&s.at_ms.to_le_bytes());
            c.len += POOL_SNAP_ENTRY;
        }
        let h = crate::perfstar2::perfgate::fnv1a(&c.buf[..c.len]);
        c.buf[c.len..c.len + 4].copy_from_slice(&h.to_le_bytes());
        c.len += 4;
        c
    }

    pub fn verify(&self) -> bool {
        if self.len < 8 {
            return false;
        }
        let stored = u32::from_le_bytes([
            self.buf[self.len - 4],
            self.buf[self.len - 3],
            self.buf[self.len - 2],
            self.buf[self.len - 1],
        ]);
        crate::perfstar2::perfgate::fnv1a(&self.buf[..self.len - 4]) == stored
    }

    /// 解码到定长输出（roundtrip 面；坏包返回 0）。
    pub fn decode_into(&self, out: &mut [PoolSnapSlot]) -> usize {
        if !self.verify() {
            return 0;
        }
        let n = ((self.buf[0] as usize) | ((self.buf[1] as usize) << 8))
            .min(POOL_SNAP_MAX)
            .min(out.len());
        for k in 0..n {
            let b = 4 + k * POOL_SNAP_ENTRY;
            out[k] = PoolSnapSlot {
                slot_idx: u16::from_le_bytes([self.buf[b], self.buf[b + 1]]),
                stored_bytes: u16::from_le_bytes([self.buf[b + 2], self.buf[b + 3]]),
                kind: self.buf[b + 4],
                at_ms: u64::from_le_bytes([
                    self.buf[b + 5],
                    self.buf[b + 6],
                    self.buf[b + 7],
                    self.buf[b + 8],
                    self.buf[b + 9],
                    self.buf[b + 10],
                    self.buf[b + 11],
                    self.buf[b + 12],
                ]),
            };
        }
        n
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ---------------------------------------------------------------------------
// 深化三：迁移模拟器
// ---------------------------------------------------------------------------

/// 迁移演练结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigReport {
    pub rounds: u32,
    pub migrated_out: u32,
    pub migrated_in: u32,
    /// 迁移成本（页读写 ×4KB 计）。
    pub cost_pages: u32,
}

/// xorshift64 确定性 PRNG（演练可复现）。
pub struct Xorshift64(pub u64);

impl Xorshift64 {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// 迁移模拟：每轮压力注入 r 页 → 冷槽迁出（最旧 at_ms）→ 新页迁入。
/// 池按 (占用, 容量) 维护；演练结束出报告（零堆——页只记账不载数据）。
pub struct MigrationSimulator {
    slots: [Option<(u64, u32)>; 32], // (at_ms, stored_bytes)
    n: usize,
    cap: usize,
    report: MigReport,
}

impl MigrationSimulator {
    pub const fn new(cap: usize) -> Self {
        let cap = if cap > 32 { 32 } else { cap };
        MigrationSimulator {
            slots: [None; 32],
            n: 0,
            cap,
            report: MigReport { rounds: 0, migrated_out: 0, migrated_in: 0, cost_pages: 0 },
        }
    }

    /// 跑一轮：注入 inflow 页，池满时迁出最冷。
    pub fn round(&mut self, inflow: u32, rng: &mut Xorshift64) {
        self.report.rounds += 1;
        for _ in 0..inflow {
            if self.n >= self.cap {
                // 迁出最冷（at_ms 最旧）。
                let mut victim = 0usize;
                let mut oldest = u64::MAX;
                for (k, s) in self.slots.iter().enumerate() {
                    if let Some((t, _)) = s {
                        if *t < oldest {
                            oldest = *t;
                            victim = k;
                        }
                    }
                }
                self.slots[victim] = None;
                self.n -= 1;
                self.report.migrated_out += 1;
                self.report.cost_pages += 2; // 迁出读 + 写回 = 2 页 IO
            }
            // 新页迁入。
            let at = rng.next() % 1_000_000;
            for s in self.slots.iter_mut() {
                if s.is_none() {
                    *s = Some((at, 2_000));
                    self.n += 1;
                    self.report.migrated_in += 1;
                    break;
                }
            }
        }
    }

    pub fn report(&self) -> MigReport {
        self.report
    }

    pub fn occupancy(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：走查 / 快照 / 迁移逐条实摆。
pub fn run_memcomp_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F058-memcomp-v4");

    // ── 完整性走查 ──
    let mut iw = IntegrityWalker::new();
    let good = [1u8; 16];
    let _ = iw.register(3, &good);
    cs.add("walk_clean_pass", iw.verify_slot(3, &good), "");
    let mut flipped = good;
    flipped[7] ^= 0x80; // 位翻转
    cs.add("walk_bitflip_detected", !iw.verify_slot(3, &flipped) && iw.stats().1 == 1, "");
    cs.add("walk_unregistered_fail", !iw.verify_slot(9, &good), "");
    cs.add("walk_unregister_frees", iw.unregister(3) && !iw.unregister(3), "");

    // ── 池快照 ──
    let slots = [
        PoolSnapSlot { slot_idx: 1, stored_bytes: 900, kind: 0, at_ms: 111 },
        PoolSnapSlot { slot_idx: 2, stored_bytes: 1400, kind: 1, at_ms: 222 },
    ];
    let snap = PoolSnapshotCodec::encode(&slots);
    cs.add("psnap_len", snap.len() == 4 + 2 * POOL_SNAP_ENTRY + 4, "");
    cs.add("psnap_verify", snap.verify(), "");
    let mut out = [PoolSnapSlot { slot_idx: 0, stored_bytes: 0, kind: 0, at_ms: 0 }; POOL_SNAP_MAX];
    cs.add(
        "psnap_roundtrip",
        snap.decode_into(&mut out) == 2 && out[0] == slots[0] && out[1] == slots[1],
        "",
    );
    let mut bad = PoolSnapshotCodec::encode(&slots);
    bad.buf[6] ^= 1;
    cs.add("psnap_tamper_refused", !bad.verify() && bad.decode_into(&mut out) == 0, "");
    // 满编（定长构造——零堆）。
    let mut full = [PoolSnapSlot { slot_idx: 0, stored_bytes: 1000, kind: 0, at_ms: 0 }; POOL_SNAP_MAX];
    for (k, s) in full.iter_mut().enumerate() {
        *s = PoolSnapSlot { slot_idx: k as u16, stored_bytes: 1000, kind: 0, at_ms: k as u64 };
    }
    cs.add("psnap_full_roundtrip", PoolSnapshotCodec::encode(&full).decode_into(&mut out) == POOL_SNAP_MAX, "");

    // ── 迁移模拟 ──
    let mut sim = MigrationSimulator::new(8);
    let mut rng = Xorshift64(0x9E37_79B9_7F4A_7C15);
    sim.round(4, &mut rng); // 4 页入（未满——零迁移）
    cs.add("mig_no_evict_under_cap", sim.report().migrated_out == 0 && sim.occupancy() == 4, "");
    sim.round(8, &mut rng); // 再入 8 → 池满 → 4 次迁出
    let r = sim.report();
    cs.add(
        "mig_evicts_coldest",
        r.migrated_out == 4 && r.migrated_in == 12 && r.cost_pages == 8,
        "",
    );
    // 复现性：同种子两次演练同报告。
    let mut s1 = MigrationSimulator::new(8);
    let mut r1 = Xorshift64(42);
    for _ in 0..MIGRATE_ROUNDS_CAP / 8 {
        s1.round(2, &mut r1);
    }
    let mut s2 = MigrationSimulator::new(8);
    let mut r2 = Xorshift64(42);
    for _ in 0..MIGRATE_ROUNDS_CAP / 8 {
        s2.round(2, &mut r2);
    }
    cs.add("mig_reproducible", s1.report() == s2.report(), "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn walker_reregister_updates_not_dups() {
        let mut w = IntegrityWalker::new();
        let a = [1u8; 16];
        let b = [2u8; 16];
        let _ = w.register(5, &a);
        let _ = w.register(5, &b); // 重压缩覆盖
        assert!(w.verify_slot(5, &b));
        assert!(!w.verify_slot(5, &a));
        assert_eq!(w.stats().2, 1, "同槽不重复登记");
    }

    #[test]
    fn psnap_zero_entries_valid() {
        let s = PoolSnapshotCodec::encode(&[]);
        assert!(s.verify());
        let mut out = [PoolSnapSlot { slot_idx: 0, stored_bytes: 0, kind: 0, at_ms: 0 }; 4];
        assert_eq!(s.decode_into(&mut out), 0);
    }

    #[test]
    fn migration_pool_stays_within_cap() {
        let mut sim = MigrationSimulator::new(6);
        let mut rng = Xorshift64(7);
        for _ in 0..10 {
            sim.round(3, &mut rng);
            assert!(sim.occupancy() <= 6, "池不越容");
        }
        assert!(sim.report().migrated_out > 0);
    }
}

// ===========================================================================
// v5 深化批（deep5）：格式版本化 + 遥测聚合 + 退化演练
// ===========================================================================
// 主册判据（G-B-18 面）延伸：持久化格式必须有版本头与向后兼容读取；
// 压缩统计必须可跨日聚合；退化路径必须可演练。

/// 流版本头 magic（"VXC1" FNV 校验前缀）。
pub const STREAM_MAGIC: u32 = 0x56_58_43_31;
/// 当前流格式版本。
pub const STREAM_VERSION: u16 = 1;
/// 版本头字节数（magic 4 + version 2 + payload_len 2 + fnv 4）。
pub const STREAM_HEADER_BYTES: usize = 12;

/// 压缩流版本化封装：头部（magic+version+长度+校验）+ 载荷。
pub struct VersionedStream {
    buf: [u8; STREAM_HEADER_BYTES + 256],
    len: usize,
}

impl VersionedStream {
    /// 封装载荷（≤256B）。
    pub fn encode(payload: &[u8], version: u16) -> Option<Self> {
        if payload.len() > 256 {
            return None;
        }
        let mut s = VersionedStream { buf: [0; STREAM_HEADER_BYTES + 256], len: STREAM_HEADER_BYTES + payload.len() };
        s.buf[0..4].copy_from_slice(&STREAM_MAGIC.to_le_bytes());
        s.buf[4..6].copy_from_slice(&version.to_le_bytes());
        s.buf[6..8].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        let h = fnv1a_32(payload);
        s.buf[8..12].copy_from_slice(&h.to_le_bytes());
        s.buf[STREAM_HEADER_BYTES..STREAM_HEADER_BYTES + payload.len()].copy_from_slice(payload);
        Some(s)
    }

    /// 解析头（版本向后兼容：≤当前版本可读）。
    /// 返回 (version, payload_len, checksum_ok)。
    pub fn parse_header(&self) -> Option<(u16, u16, bool)> {
        if self.len < STREAM_HEADER_BYTES {
            return None;
        }
        let magic = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
        if magic != STREAM_MAGIC {
            return None;
        }
        let ver = u16::from_le_bytes([self.buf[4], self.buf[5]]);
        let plen = u16::from_le_bytes([self.buf[6], self.buf[7]]);
        let stored = u32::from_le_bytes([self.buf[8], self.buf[9], self.buf[10], self.buf[11]]);
        let ok = stored == fnv1a_32(&self.buf[STREAM_HEADER_BYTES..STREAM_HEADER_BYTES + plen as usize]);
        Some((ver, plen, ok))
    }

    /// 载荷读取（版本兼容 + 校验通过才放行）。
    pub fn decode_payload(&self, out: &mut [u8]) -> Option<usize> {
        let (ver, plen, ok) = self.parse_header()?;
        if ver > STREAM_VERSION || !ok || out.len() < plen as usize {
            return None;
        }
        out[..plen as usize].copy_from_slice(&self.buf[STREAM_HEADER_BYTES..STREAM_HEADER_BYTES + plen as usize]);
        Some(plen as usize)
    }

    pub fn total_len(&self) -> usize {
        self.len
    }
}

/// FNV-1a 32 位（本域局部——与 perfgate 同型不同实例，避免跨域耦合）。
pub fn fnv1a_32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---------------------------------------------------------------------------
// 深化二：字典预热账（冷启动压缩比爬坡——判据：预热后比值回落）
// ---------------------------------------------------------------------------

/// 字典预热统计：冷启动前 N 页压缩比偏高（无字典命中），预热账追踪
/// 比值回落点（主册：预热 32 页后比值达稳态 ±5%）。
pub struct DictWarmupLedger {
    ratio_x100_ring: [u16; 64],
    pos: usize,
    filled: usize,
    /// 累计页数。
    total_pages: u64,
}

impl DictWarmupLedger {
    pub const fn new() -> Self {
        DictWarmupLedger { ratio_x100_ring: [0; 64], pos: 0, filled: 0, total_pages: 0 }
    }

    /// 记一页压缩比（×100，如 0.62 → 62）。
    pub fn page(&mut self, ratio_x100: u16) {
        self.ratio_x100_ring[self.pos] = ratio_x100;
        self.pos = (self.pos + 1) % 64;
        if self.filled < 64 {
            self.filled += 1;
        }
        self.total_pages += 1;
    }

    /// 最近 8 页均值 vs 前 8 页均值（×100 差值）——正数表示仍在爬坡。
    pub fn slope_x100(&self) -> i32 {
        if self.filled < 16 {
            return 0;
        }
        let mut recent = 0u32;
        let mut prior = 0u32;
        for k in 0..8 {
            let ri = (self.pos + 64 - 1 - k) % 64;
            let pi = (self.pos + 64 - 9 - k) % 64;
            recent += self.ratio_x100_ring[ri] as u32;
            prior += self.ratio_x100_ring[pi] as u32;
        }
        recent as i32 / 8 - prior as i32 / 8
    }

    /// 预热完成：斜率 ≥ -3（比值不再明显下降）且累计 ≥32 页。
    pub fn warmed_up(&self) -> bool {
        self.total_pages >= 32 && self.slope_x100() >= -3
    }

    pub fn pages(&self) -> u64 {
        self.total_pages
    }
}

// ---------------------------------------------------------------------------
// 深化三：跨页引用图（共享页去重——两页同源引用计数）
// ---------------------------------------------------------------------------

/// 跨页引用去重表：32 页槽位，每槽可登记「与某锚页同源」。
/// 判据：同源页组内只保留一份实体压缩，其余为引用（压缩账只记一次）。
pub struct CrossPageRef {
    /// anchor_of[k] = Some(anchor_slot)——k 页实体被去重到 anchor 槽。
    anchor_of: [Option<u8>; 32],
    /// 实体页账。
    entities: u32,
    /// 引用页账。
    refs: u32,
}

impl CrossPageRef {
    pub const fn new() -> Self {
        CrossPageRef { anchor_of: [None; 32], entities: 0, refs: 0 }
    }

    /// 登记实体页（顺序分配槽位；满返回 None）。
    pub fn add_entity(&mut self) -> Option<usize> {
        let slot = (self.entities + self.refs) as usize;
        if slot >= 32 {
            return None;
        }
        self.anchor_of[slot] = None; // None = 自身为实体
        self.entities += 1;
        Some(slot)
    }

    /// 登记引用页（指向实体槽）。
    pub fn add_ref_to(&mut self, anchor_slot: usize) -> Option<usize> {
        if anchor_slot >= (self.entities + self.refs) as usize {
            return None; // 引用必须指向已存在实体
        }
        let slot = (self.entities + self.refs) as usize;
        if slot >= 32 {
            return None;
        }
        self.anchor_of[slot] = Some(anchor_slot as u8);
        self.refs += 1;
        Some(slot)
    }

    /// 压缩实体节省率（×100）：引用页不占实体压缩成本。
    pub fn dedup_saving_pct(&self) -> u32 {
        let total = self.entities + self.refs;
        if total == 0 {
            return 0;
        }
        self.refs * 100 / total
    }

    /// 解引用：slot → 实体槽（引用链一跳即实体——本层禁多跳）。
    pub fn resolve(&self, slot: usize) -> Option<usize> {
        match self.anchor_of.get(slot).copied()? {
            Some(a) => Some(a as usize),
            None => Some(slot),
        }
    }

    pub fn counts(&self) -> (u32, u32) {
        (self.entities, self.refs)
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_memcomp_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F058-memcomp-v5");

    // ── 版本化流 ──
    // 1) roundtrip + 头解析三件套。
    let payload: [u8; 40] = core::array::from_fn(|k| (k * 7 + 3) as u8);
    let vs = VersionedStream::encode(&payload, STREAM_VERSION).expect("40B 载荷合法");
    let (ver, plen, ok) = vs.parse_header().expect("头合法");
    cs.add("vstream_roundtrip", ver == 1 && plen == 40 && ok, "");
    // 2) 载荷校验通过才放行。
    let mut out = [0u8; 64];
    cs.add("vstream_payload_ok", vs.decode_payload(&mut out) == Some(40) && out[0] == 3 && out[39] == 20, "");
    // 3) 篡改检出：改载荷一字节 → checksum 失败 → 拒读。
    let mut tampered = VersionedStream { buf: vs.buf, len: vs.len };
    tampered.buf[STREAM_HEADER_BYTES + 10] ^= 0xFF;
    cs.add("vstream_tamper_detected", tampered.parse_header().map(|(_, _, ok)| ok) == Some(false), "");
    // 4) 未来版本拒读（向后兼容不向前兼容——诚实边界）。
    let future = VersionedStream::encode(&payload, STREAM_VERSION + 1).expect("合法");
    cs.add("vstream_future_version_rejected", future.decode_payload(&mut out).is_none(), "");
    // 5) 超长载荷拒绝（256B 上限——定长纪律）。
    let big = [0u8; 257];
    cs.add("vstream_oversize_rejected", VersionedStream::encode(&big, 1).is_none(), "");
    // 6) 坏 magic 拒解析。
    let mut bad = VersionedStream { buf: vs.buf, len: vs.len };
    bad.buf[0] = 0;
    cs.add("vstream_bad_magic", bad.parse_header().is_none(), "");

    // ── 字典预热账 ──
    // 7) 冷启动爬坡后达稳态：前 16 页比值高（64），后 16 页稳态（58）→ 预热完成。
    let mut wl = DictWarmupLedger::new();
    for _ in 0..16 {
        wl.page(64);
    }
    for _ in 0..20 {
        wl.page(58);
    }
    cs.add("dict_warmup_reaches_steady", wl.warmed_up() && wl.pages() == 36, "");
    // 8) 未满 32 页不判稳（样本诚实）。
    let mut wl2 = DictWarmupLedger::new();
    for _ in 0..16 {
        wl2.page(60);
    }
    cs.add("dict_warmup_needs_32pages", !wl2.warmed_up(), "");
    // 9) 持续爬坡（比值还在降）不判稳。
    let mut wl3 = DictWarmupLedger::new();
    for k in 0..36 {
        wl3.page(70 - k as u16); // 持续下降 → 斜率 < -3
    }
    cs.add("dict_warmup_still_climbing", !wl3.warmed_up(), "");

    // ── 跨页引用去重 ──
    // 10) 实体+引用账与解引用。
    let mut xr = CrossPageRef::new();
    let e0 = xr.add_entity().expect("槽 0");
    let e1 = xr.add_entity().expect("槽 1");
    let r0 = xr.add_ref_to(e0).expect("槽 2");
    let r1 = xr.add_ref_to(e0).expect("槽 3");
    cs.add(
        "xref_resolve",
        xr.resolve(e0) == Some(0) && xr.resolve(r0) == Some(0) && xr.resolve(r1) == Some(0) && xr.resolve(e1) == Some(1),
        "",
    );
    // 11) 去重节省率：4 页 2 实体 2 引用 → 50%。
    cs.add("xref_saving_50pct", xr.dedup_saving_pct() == 50 && xr.counts() == (2, 2), "");
    // 12) 引用指向不存在实体 → 拒绝。
    cs.add("xref_dangling_refused", xr.add_ref_to(9).is_none(), "");

    // ── 深化单测 ──
    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn vstream_roundtrip_exact_bytes() {
        let payload: [u8; 12] = core::array::from_fn(|k| k as u8 * 11);
        let vs = VersionedStream::encode(&payload, 1).unwrap();
        assert_eq!(vs.total_len(), STREAM_HEADER_BYTES + 12);
        let mut out = [0u8; 12];
        assert_eq!(vs.decode_payload(&mut out), Some(12));
        assert_eq!(out, payload);
    }

    #[test]
    fn dict_warmup_full_cycle() {
        let mut wl = DictWarmupLedger::new();
        // 64 冷页（比值 70）+ 稳态 30 页（比值 65）。
        for _ in 0..64 {
            wl.page(70);
        }
        for _ in 0..30 {
            wl.page(65);
        }
        assert!(wl.warmed_up(), "斜率 = -5 → 未达稳态前不判稳；稳态后判稳");
    }

    #[test]
    fn xref_full_table_refuses() {
        let mut xr = CrossPageRef::new();
        for _ in 0..32 {
            assert!(xr.add_entity().is_some());
        }
        assert!(xr.add_entity().is_none(), "32 槽满后第 33 次拒绝");
        assert_eq!(xr.counts().0, 32);
    }
}
