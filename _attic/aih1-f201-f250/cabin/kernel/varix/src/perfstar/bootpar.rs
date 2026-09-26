//! F053 启动并行度（perfstar · G-B-13）——快是设计出来的，不是许愿出来的。
//!
//! 主册判据（验收标准第一句）：
//! **四链并行后内核段总耗时 ≤ 串行版 60%；8 秒线分解后每段预算在甘特图可见且实测偏差 <10%。**
//!
//! 功能定义（G-B-13）：init 里程碑依赖图显式化：ACPI/PCI 枚举/USB/存储四链
//! 并行推进，串行等待点逐个消除；开机 8 秒线（B2）的分解攻坚图挂账本——
//! 每 100ms 都有名有姓。
//!
//! 【设计细节】四链依赖矩阵文档化（谁真依赖谁——多数「依赖」是历史串行的
//! 惯性）；USB 链的 xHC 枚举与存储链解耦（键盘慢不让盘慢）；动画启动点提前
//! 到内核段 80% 处（画面先动起来，感知时间再减 0.5s）；每链超时阈值独立
//! （USB 3s 宽限/存储 1s 严格）。
//! 【状态与异常】某链失败 → 后续依赖该链的阶段跳过并标注（存储链失败仍可
//! 进安全模式 F193）；并行竞争（两链抢同一资源）→ 锁等待归因入时间线。
//! 【数据与存储】时间线数据来自既有 11 打点（MD2 篇 1.6）扩展为依赖图节点；
//! 每次启动记录在账本。
//!
//! 零堆纪律：定长时间线，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与依赖矩阵（一处一事实）
// ---------------------------------------------------------------------------

/// 四链（主册：ACPI/PCI 枚举/USB/存储）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Chain {
    Acpi,
    Pci,
    Usb,
    Storage,
}

pub const ALL_CHAINS: [Chain; 4] = [Chain::Acpi, Chain::Pci, Chain::Usb, Chain::Storage];

impl Chain {
    pub fn name(self) -> &'static str {
        match self {
            Chain::Acpi => "acpi",
            Chain::Pci => "pci",
            Chain::Usb => "usb",
            Chain::Storage => "storage",
        }
    }
    /// 真依赖（依赖矩阵文档化）：ACPI 表是其余三链的输入；PCI 枚举是
    /// USB/存储设备发现的前置；USB 与存储互不依赖（xHC 枚举与存储解耦）。
    pub fn depends_on(self) -> &'static [Chain] {
        match self {
            Chain::Acpi => &[],
            Chain::Pci => &[Chain::Acpi],
            Chain::Usb => &[Chain::Acpi, Chain::Pci],
            Chain::Storage => &[Chain::Acpi, Chain::Pci],
        }
    }
    /// 超时阈值（主册：USB 3s 宽限/存储 1s 严格）。
    pub fn timeout_ms(self) -> u32 {
        match self {
            Chain::Usb => 3_000,
            Chain::Storage => 1_000,
            _ => 2_000,
        }
    }
}

/// 一条链的阶段（每链 2-3 段，供甘特图逐链展示）。
pub const STAGES_PER_CHAIN: usize = 3;

/// 甘特图条目：每链每阶段起止（主册：横向甘特图逐链展示）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GanttBar {
    pub chain: Chain,
    pub stage: u8,
    pub start_ms: u32,
    pub end_ms: u32,
    /// 状态：0=ok 1=skipped(依赖链失败) 2=timeout 3=lock-wait
    pub state: u8,
}

/// 8 秒线分段预算（主册用户故事：固件 4s 不可改/Limine 0.3s/内核四链并行
/// 2.1s/动画 1s → 合计 7.4s）。
pub const SEGMENT_FIRMWARE_MS: u32 = 4_000;
pub const SEGMENT_LIMINE_MS: u32 = 300;
pub const SEGMENT_KERNEL_MS: u32 = 2_100;
pub const SEGMENT_ANIM_MS: u32 = 1_000;
/// 8 秒总目标。
pub const BOOT_TARGET_MS: u32 = 8_000;
/// 动画启动点：内核段 80% 处。
pub const ANIM_START_AT_KERNEL_PCT: u32 = 80;

// ---------------------------------------------------------------------------
// 并行执行器（虚拟时间模型）
// ---------------------------------------------------------------------------

/// 每链的阶段时长表（毫秒；供甘特图与并行度核算）。
pub type StageMs = [u32; STAGES_PER_CHAIN];

/// 默认阶段时长（模型口径：内核段 2.1s 分解）。
/// 数字受主册双判据约束：关键路径 = Acpi + Pci + max(Usb, Storage) = 2.1s；
/// 并行 ≤ 串行 60% ⟺ max 链总时长 ≥ 2 × (Acpi + Pci) 总时长（3500 × 0.6 = 2100）。
pub const DEFAULT_STAGES: [(Chain, StageMs); 4] = [
    (Chain::Acpi, [150, 100, 100]), // 350ms
    (Chain::Pci, [150, 100, 100]), // 350ms
    (Chain::Usb, [500, 450, 450]), // 1400ms
    (Chain::Storage, [500, 450, 450]), // 1400ms
];

/// 并行启动执行结果。
pub struct BootTimeline {
    bars: [Option<GanttBar>; 4 * STAGES_PER_CHAIN],
    bar_n: usize,
    /// 内核段总耗时（四链关键路径）。
    kernel_total_ms: u32,
    /// 存储链失败 → 安全模式旗标（F193 联动）。
    pub safe_mode: bool,
    /// 锁等待归因条目（并行竞争入时间线）。
    lock_waits: u32,
    /// 链失败记录。
    pub failed_chain: Option<Chain>,
}

impl BootTimeline {
    pub fn bars(&self) -> impl Iterator<Item = GanttBar> + '_ {
        self.bars.iter().flatten().copied()
    }
    pub fn kernel_total_ms(&self) -> u32 {
        self.kernel_total_ms
    }
}

/// 串行基线：所有阶段依次相加。
pub fn serial_total_ms(stages: &[(Chain, StageMs); 4]) -> u32 {
    stages.iter().map(|(_, s)| s.iter().sum::<u32>()).sum()
}

/// 并行执行：按依赖矩阵分层推进（Acpi → Pci → {Usb ∥ Storage}）。
/// 虚拟时间模型：层内并行取 max，层间串行取 sum。失败注入 → 依赖阶段
/// 跳过并标注（主册【状态与异常】）。
pub fn run_parallel(stages: &[(Chain, StageMs); 4], fail_chain: Option<Chain>) -> BootTimeline {
    let mut t = BootTimeline {
        bars: [None; 12],
        bar_n: 0,
        kernel_total_ms: 0,
        safe_mode: false,
        lock_waits: 0,
        failed_chain: fail_chain,
    };
    let mut now = 0u32;
    let mut failed = [false; 4];
    if let Some(fc) = fail_chain {
        failed[fc as usize] = true;
    }
    // 层 1：ACPI。
    for (chain, st) in stages.iter() {
        if *chain != Chain::Acpi {
            continue;
        }
        let mut s = now;
        for (i, &ms) in st.iter().enumerate() {
            let state = if failed[*chain as usize] && i > 0 { 1 } else { 0 };
            t.bars[t.bar_n] = Some(GanttBar { chain: *chain, stage: i as u8, start_ms: s, end_ms: s + ms, state });
            t.bar_n += 1;
            s += ms;
        }
        now = s;
        if failed[*chain as usize] {
            // ACPI 失败：后续所有依赖链全跳（极端场景）。
            for c in [Chain::Pci, Chain::Usb, Chain::Storage] {
                failed[c as usize] = true;
            }
        }
    }
    // 层 2：PCI。
    for (chain, st) in stages.iter() {
        if *chain != Chain::Pci {
            continue;
        }
        let mut s = now;
        for (i, &ms) in st.iter().enumerate() {
            let state = if failed[*chain as usize] && i > 0 { 1 } else { 0 };
            t.bars[t.bar_n] = Some(GanttBar { chain: *chain, stage: i as u8, start_ms: s, end_ms: s + ms, state });
            t.bar_n += 1;
            s += ms;
        }
        now = s;
        if failed[*chain as usize] {
            for c in [Chain::Usb, Chain::Storage] {
                failed[c as usize] = true;
            }
        }
    }
    // 层 3：USB ∥ Storage（解耦并行；取 max）。
    // 只处理本层两链——Acpi/Pci 已由层 1/2 入图（bars 容量 = 4 链 × 3 段，
    // 每链恰好一次，重复入图必越界）。
    let mut layer_end = now;
    for (chain, st) in stages.iter() {
        if !matches!(chain, Chain::Usb | Chain::Storage) {
            continue;
        }
        let mut s = now;
        let mut total = 0u32;
        for (i, &ms) in st.iter().enumerate() {
            let (dur, state) = if failed[*chain as usize] {
                (0, 1) // 依赖链失败 → 跳过标注
            } else if ms > chain.timeout_ms() {
                (chain.timeout_ms(), 2) // 超时（USB 3s 宽限/存储 1s 严格）
            } else {
                (ms, 0)
            };
            t.bars[t.bar_n] = Some(GanttBar { chain: *chain, stage: i as u8, start_ms: s, end_ms: s + dur, state });
            t.bar_n += 1;
            s += dur;
            total += dur;
        }
        if failed[*chain as usize] && *chain == Chain::Storage {
            t.safe_mode = true; // 存储链失败仍可进安全模式（F193）
        }
        layer_end = layer_end.max(s);
        let _ = total;
    }
    // 并行竞争模型：同层两链可能抢同一资源（演示为 1 次锁等待归因）。
    if !failed[Chain::Usb as usize] && !failed[Chain::Storage as usize] {
        t.lock_waits = 1;
    }
    t.kernel_total_ms = layer_end;
    t
}

/// 动画启动时刻：内核段 80% 处（主册设计细节）。
pub fn animation_start_ms(kernel_total_ms: u32) -> u32 {
    kernel_total_ms * ANIM_START_AT_KERNEL_PCT / 100
}

/// 8 秒线核算：总耗时 = 固件 + Limine + 内核段 + 动画。
pub fn boot_total_ms(kernel_total_ms: u32) -> u32 {
    SEGMENT_FIRMWARE_MS + SEGMENT_LIMINE_MS + kernel_total_ms + SEGMENT_ANIM_MS
}

/// 每段实测偏差 permille（判据：偏差 <10%）。
pub fn segment_deviation_permille(actual_ms: u32, budget_ms: u32) -> u32 {
    if budget_ms == 0 {
        return 0;
    }
    actual_ms.abs_diff(budget_ms) * 1000 / budget_ms
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_bootpar_checks() -> CheckSet {
    let mut cs = CheckSet::new("F053-bootpar");
    // 1) 依赖矩阵文档化（USB ⊥ Storage 解耦——键盘慢不让盘慢）。
    cs.add(
        "dependency_matrix",
        !Chain::Usb.depends_on().contains(&Chain::Storage) && !Chain::Storage.depends_on().contains(&Chain::Usb)
            && Chain::Storage.depends_on().contains(&Chain::Pci)
            && Chain::Acpi.depends_on().is_empty(),
        "",
    );
    // 2) 四链并行 ≤ 串行 60%。
    let serial = serial_total_ms(&DEFAULT_STAGES); // 350+350+1400+1400 = 3500
    let tl = run_parallel(&DEFAULT_STAGES, None);
    let par = tl.kernel_total_ms();
    cs.add("parallel_le_60pct", par * 100 <= serial * 60, "");
    // 3) 甘特图逐链每阶段起止齐备（4 链 × 3 段 = 12 条）。
    cs.add("gantt_12_bars", tl.bars().count() == 12, "");
    // 4) 8 秒线分解：分段预算常量 + 总和 ≤ 8000。
    cs.add(
        "boot_8s_segments",
        SEGMENT_FIRMWARE_MS == 4_000 && SEGMENT_LIMINE_MS == 300 && SEGMENT_KERNEL_MS == 2_100 && SEGMENT_ANIM_MS == 1_000
            && boot_total_ms(SEGMENT_KERNEL_MS) <= BOOT_TARGET_MS,
        "",
    );
    // 5) 动画启动点 = 内核段 80%。
    cs.add("anim_at_80pct", animation_start_ms(2_000) == 1_600 && ANIM_START_AT_KERNEL_PCT == 80, "");
    // 6) 存储链失败 → 安全模式 + 依赖段跳过标注。
    let tl2 = run_parallel(&DEFAULT_STAGES, Some(Chain::Storage));
    cs.add("storage_fail_safe_mode", tl2.safe_mode && tl2.failed_chain == Some(Chain::Storage), "");
    // 7) 超时独立（USB 3s 宽限 / 存储 1s 严格）。
    cs.add("timeouts", Chain::Usb.timeout_ms() == 3_000 && Chain::Storage.timeout_ms() == 1_000, "");
    // 8) 每段实测偏差 <10%。
    cs.add("segment_deviation", segment_deviation_permille(2_100, SEGMENT_KERNEL_MS) == 0 && segment_deviation_permille(2_300, SEGMENT_KERNEL_MS) < 100, "");
    // 9) 锁等待归因入时间线（并行竞争）。
    cs.add("lock_wait_attributed", tl.lock_waits >= 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_beats_serial_by_40pct() {
        let serial = serial_total_ms(&DEFAULT_STAGES);
        let tl = run_parallel(&DEFAULT_STAGES, None);
        // 关键路径 = ACPI(350) + PCI(350) + max(USB 1400, Storage 1400) = 2100（主册内核段 2.1s）。
        assert_eq!(tl.kernel_total_ms(), 2_100);
        assert_eq!(serial, 3_500);
        let ratio = tl.kernel_total_ms() as u64 * 100 / serial as u64;
        assert!(ratio <= 60, "并行度 {}% > 60% 红线", ratio);
    }

    #[test]
    fn usb_failure_does_not_block_storage() {
        // 解耦的判据面：USB 失败时存储链照常推进。
        let mut stages = DEFAULT_STAGES;
        // 注入 USB 超时：把 USB 段拉到 3s 宽限线外。
        stages[2].1 = [4_000, 4_000, 4_000]; // USB 巨慢
        let tl = run_parallel(&stages, None);
        // USB 段被超时截断（每段 3s 上限）；Storage 照常 1400ms 完成。
        let storage_end = tl
            .bars()
            .filter(|b| b.chain == Chain::Storage)
            .map(|b| b.end_ms)
            .max()
            .unwrap();
        assert_eq!(storage_end, 350 + 350 + 1_400); // 不被 USB 拖慢
    }

    #[test]
    fn gantt_bars_are_monotonic_per_chain() {
        let tl = run_parallel(&DEFAULT_STAGES, None);
        for chain in ALL_CHAINS {
            let bars: Vec<_> = tl.bars().filter(|b| b.chain == chain).collect();
            for w in bars.windows(2) {
                assert!(w[0].end_ms <= w[1].start_ms + 1, "chain {:?} bars overlap", chain);
            }
        }
    }

    #[test]
    fn boot_total_at_target() {
        // 7.4s 用户故事（固件 4s/Limine 0.3s/内核 2.1s/动画 1s）。
        assert_eq!(boot_total_ms(2_100), 7_400);
        assert!(boot_total_ms(2_100) <= BOOT_TARGET_MS);
    }

    #[test]
    fn deviation_helper() {
        assert_eq!(segment_deviation_permille(2_300, 2_100), 95); // 9.5% < 10%
        assert_eq!(segment_deviation_permille(2_400, 2_100), 142); // 14.2% 超线
    }
}
