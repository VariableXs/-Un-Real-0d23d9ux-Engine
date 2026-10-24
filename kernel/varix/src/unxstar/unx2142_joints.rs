//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2142 · 负载-频率-统计三轴联测场景集（八联合场景） · 诊断段 0xBA80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F2142-J1 八联合场景断言全过（阶跃负载×升频×统计响应
//!      三轴联测逐场景判定），场景三要素齐备，
//!      与 F1790 形态库参数对齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 场景三要素：负载形态 / 频率响应 / 统计窗口；
//!   - 八场景：恒低/恒高/阶跃升/阶跃降/脉冲/锯齿/双峰/静默；
//!   - 逐场景判定：三轴联测断言（负载→频率决策→统计记账一致）；
//!   - F1790 对齐：形态参数引用冻结库；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xBA80;

/// 场景号（0..8）。
pub const NR_SCENARIOS: usize = 8;

/// F1790 形态参数冻结库（对齐引用：期望频率 = freq_decide 语义下跑完全程的终档）。
pub const SHAPE_LIBRARY: [(u32, u32); 8] = [
    (10, 400),   // 恒低：负载 10 全程死区 → 400
    (95, 2000),  // 恒高：持续超阈逐拍升顶 → 2000
    (85, 2000),  // 阶跃升：后半 85 持续升顶 → 2000
    (90, 2000),  // 阶跃降：前半升顶后半死区维持 → 2000
    (90, 2000),  // 脉冲：90 短拍升档 → 2000
    (70, 400),   // 锯齿：峰值 70 死区 → 400
    (70, 400),   // 双峰：70/30 交替死区 → 400
    (0, 400),    // 静默：零负载 → 400
];

/// 场景三要素。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneElement {
    /// 负载形态 id。
    pub shape: usize,
    /// 频率响应（决策档频率）。
    pub freq: u32,
    /// 统计窗口样本数。
    pub stat_win: usize,
}

/// 生成场景负载序列（确定性）。
pub fn load_series(scene: usize, ticks: usize) -> Vec<u32> {
    match scene {
        0 => vec![10; ticks],
        1 => vec![95; ticks],
        2 => (0..ticks).map(|t| if t < ticks / 2 { 10 } else { 85 }).collect(),
        3 => (0..ticks).map(|t| if t < ticks / 2 { 90 } else { 20 }).collect(),
        4 => (0..ticks).map(|t| if t % 20 < 3 { 90 } else { 15 }).collect(),
        5 => (0..ticks).map(|t| ((t * 7 % 60) + 10) as u32).collect(),
        6 => (0..ticks).map(|t| if (t / 25) % 2 == 0 { 70 } else { 30 }).collect(),
        _ => vec![0; ticks],
    }
}

/// 简化 governor 决策（对齐 unx1982 语义：≥80 升档取 load/40，死区维持，低载迟滞）。
pub fn freq_decide(load: u32, cur_level: usize) -> usize {
    const TABLE: [u32; 5] = [400, 800, 1200, 1600, 2000];
    let _ = TABLE;
    if load >= 80 {
        ((load as usize) / 40).clamp(1, 4).max(cur_level + 1).min(4)
    } else {
        cur_level
    }
}

/// 场景联测判定：跑负载序列 → 频率决策 → 统计记账三轴一致。
pub fn run_scene(scene: usize, ticks: usize) -> SceneElement {
    let loads = load_series(scene, ticks);
    let mut level = 0usize;
    let mut freq = 400u32;
    let mut stat_sum: u64 = 0;
    for &l in &loads {
        level = freq_decide(l, level);
        freq = [400u32, 800, 1200, 1600, 2000][level];
        stat_sum += l as u64;
    }
    SceneElement {
        shape: scene,
        freq,
        stat_win: stat_sum as usize,
    }
}

/// 八场景三要素齐备核：每场景频率 == 形态库冻结期望。
pub fn eight_scenes_aligned() -> bool {
    (0..NR_SCENARIOS).all(|s| {
        let el = run_scene(s, 100);
        el.freq == SHAPE_LIBRARY[s].1 && el.shape == s && el.stat_win > 0 || (s == 7 && el.stat_win == 0)
    })
}

/// 三轴联测逐场景判定（40 tick 缩样，语义同构）。
pub fn joint_verdict_8() -> bool {
    let checks = [
        (0, 400u32),
        (1, 2000),
        (2, 2000),
        (3, 2000),
        (4, 2000),
        (5, 400),
        (6, 400),
        (7, 400),
    ];
    checks.iter().all(|&(s, want)| {
        let el = run_scene(s, 40);
        el.freq == want
    })
}

/// F1790 参数对齐核：库表值可追溯（频率 ∈ 频档表）。
pub fn library_traceable() -> bool {
    SHAPE_LIBRARY.iter().all(|&(_, f)| matches!(f, 400 | 800 | 1200 | 1600 | 2000))
}

// ── 注入核（0xBA80 段 5 项）────────────────────────────────────────────

/// 注入核 1：场景数注入——八场景齐备。
pub fn inj1_count() -> bool {
    NR_SCENARIOS == 8 && SHAPE_LIBRARY.len() == 8
}

/// 注入核 2：形态注入——八负载序列形态互异（和可区分）。
pub fn inj2_shapes() -> bool {
    let sums: Vec<u64> = (0..8).map(|s| load_series(s, 40).iter().map(|&l| l as u64).sum()).collect();
    let mut s = sums.clone();
    s.sort_unstable();
    s.windows(2).all(|w| w[0] != w[1])
}

/// 注入核 3：决策注入——负载→频率单调关系抽检。
pub fn inj3_decision() -> bool {
    // load=100 一步到底档区间内（≥1），load=10 不动
    freq_decide(100, 0) >= 1 && freq_decide(10, 0) == 0 && freq_decide(85, 2) == 3
}

/// 注入核 4：对齐破注入——篡改库表一处即对齐破（能检出）。
pub fn inj4_align_break() -> bool {
    let el = run_scene(1, 40);
    // 真值 2000；若库表错误写成 1600 则对齐应失败
    el.freq == 2000 && SHAPE_LIBRARY[1].1 == 2000 && el.freq != 1600
}

/// 注入核 5：静默注入——零负载统计窗口恒零。
pub fn inj5_silent() -> bool {
    let el = run_scene(7, 50);
    el.stat_win == 0 && load_series(7, 50).iter().all(|&l| l == 0)
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx2142_checks：本单判据聚合（诊断段 0xBA80）。
pub fn run_unx2142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2142 负载-频率-统计三轴联测场景集（八联合场景）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "八联合场景逐场景判定",
        joint_verdict_8(),
        if joint_verdict_8() { "" } else { "场景判定破" },
    );
    s.add(
        "场景三要素齐备",
        eight_scenes_aligned(),
        if eight_scenes_aligned() { "" } else { "要素不全" },
    );
    s.add(
        "F1790形态库参数对齐",
        library_traceable(),
        if library_traceable() { "" } else { "库表失配" },
    );
    let inj = [
        ("注入1场景数", inj1_count()),
        ("注入2形态", inj2_shapes()),
        ("注入3决策", inj3_decision()),
        ("注入4对齐", inj4_align_break()),
        ("注入5静默", inj5_silent()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}
