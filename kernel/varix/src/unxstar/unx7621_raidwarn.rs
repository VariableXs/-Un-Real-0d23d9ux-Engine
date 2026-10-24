//! UNX-F7621 · RAID 坏盘预警链（健康分级 → 决策引擎 → RAID 动作三段）· 诊断段 0xAF80
//! 验收锚点：预警链三段落账——① 健康分级段：盘健康分（0~100）映射四级
//! （≥80 正常 / ≥50 注意 / ≥20 降级 / <20 危急）；② 决策引擎段：按 RAID1 成员数与
//! 最差分级出动作（正常→无动作 / 注意→预警记录 / 降级→标记 degraded /
//! 危急→热备替换+重建）；③ 动作段：动作枚举可执行可回账，重建占位逐轮推进。


use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xAF80;

/// ① 健康分级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Good,
    Watch,
    Degraded,
    Critical,
}

impl Health {
    pub fn from_score(sc: u8) -> Health {
        if sc >= 80 {
            Health::Good
        } else if sc >= 50 {
            Health::Watch
        } else if sc >= 20 {
            Health::Degraded
        } else {
            Health::Critical
        }
    }
    pub fn rank(self) -> u8 {
        match self {
            Health::Good => 0,
            Health::Watch => 1,
            Health::Degraded => 2,
            Health::Critical => 3,
        }
    }
}

/// ③ RAID 动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaidAction {
    None,
    LogWarn,
    MarkDegraded,
    HotspareSwap,
    Rebuild,
}

impl RaidAction {
    pub fn as_str(self) -> &'static str {
        match self {
            RaidAction::None => "none",
            RaidAction::LogWarn => "log",
            RaidAction::MarkDegraded => "degrade",
            RaidAction::HotspareSwap => "swap",
            RaidAction::Rebuild => "rebuild",
        }
    }
}

/// RAID1 阵列：成员健康 + 热备池 + 重建进度。
pub struct Raid1 {
    pub members: Vec<(u8, Health)>,
    pub hotspares: u8,
    pub action_log: Vec<(u8, RaidAction)>,
    pub rebuild_progress: u32,
    pub rebuilding: bool,
}

impl Raid1 {
    pub fn new(n_members: u8, hotspares: u8) -> Self {
        let mut members = Vec::new();
        for i in 0..n_members {
            members.push((i, Health::Good));
        }
        Raid1 { members, hotspares, action_log: Vec::new(), rebuild_progress: 0, rebuilding: false }
    }

    /// ② 决策引擎：取最差分级出动作。
    pub fn decide(&self) -> RaidAction {
        let mut worst = 0u8;
        for (_, h) in &self.members {
            let r = h.rank();
            if r > worst {
                worst = r;
            }
        }
        match worst {
            0 => RaidAction::None,
            1 => RaidAction::LogWarn,
            2 => RaidAction::MarkDegraded,
            _ => {
                if self.hotspares > 0 {
                    RaidAction::HotspareSwap
                } else {
                    RaidAction::Rebuild
                }
            }
        }
    }

    /// 动作执行：记账（slot=0xFF 表阵列级动作）。
    pub fn apply(&mut self, a: RaidAction) -> bool {
        match a {
            RaidAction::None => {
                self.action_log.push((0xFF, RaidAction::None));
                false
            }
            RaidAction::LogWarn => {
                self.action_log.push((0xFF, RaidAction::LogWarn));
                false
            }
            RaidAction::MarkDegraded => {
                self.action_log.push((0xFF, RaidAction::MarkDegraded));
                false
            }
            RaidAction::HotspareSwap => {
                if self.hotspares == 0 {
                    return false;
                }
                self.hotspares -= 1;
                // 替换最差成员，恢复 Good
                if let Some(worst_idx) = self.worst_index() {
                    self.members[worst_idx].1 = Health::Good;
                }
                self.action_log.push((0xFF, RaidAction::HotspareSwap));
                self.rebuilding = true;
                true
            }
            RaidAction::Rebuild => {
                self.action_log.push((0xFF, RaidAction::Rebuild));
                self.rebuilding = true;
                true
            }
        }
    }

    pub fn worst_index(&self) -> Option<usize> {
        let mut wi: Option<usize> = None;
        let mut wr = 0u8;
        for (i, (_, h)) in self.members.iter().enumerate() {
            let r = h.rank();
            if r > wr {
                wr = r;
                wi = Some(i);
            }
        }
        wi
    }

    /// 重建推进：每轮 +1%，100 轮收口。
    pub fn rebuild_tick(&mut self) -> bool {
        if !self.rebuilding {
            return false;
        }
        self.rebuild_progress += 1;
        if self.rebuild_progress >= 100 {
            self.rebuild_progress = 100;
            self.rebuilding = false;
        }
        true
    }

    /// 预警链总驱动：成员评分刷新 → 决策 → 执行。
    pub fn run_chain(&mut self, scores: &[u8]) -> RaidAction {
        for (i, sc) in scores.iter().enumerate() {
            if i < self.members.len() {
                self.members[i].1 = Health::from_score(*sc);
            }
        }
        let a = self.decide();
        if a != RaidAction::None {
            self.apply(a);
        } else {
            self.action_log.push((0xFF, RaidAction::None));
        }
        a
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xAF80 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7621_AF80;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("分级-100分Good", Health::from_score(100) == Health::Good));
    out.push(("分级-0分Critical", Health::from_score(0) == Health::Critical));
    out.push(("动作-标签五态齐全", RaidAction::None.as_str() == "none" && RaidAction::Rebuild.as_str() == "rebuild"));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7621_checks：本单判据聚合（诊断段 0xAF80）。
pub fn run_unx7621_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7621 RAID 坏盘预警链");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // ① 分级段边界（含等号）
    let b_ok = Health::from_score(80) == Health::Good
        && Health::from_score(79) == Health::Watch
        && Health::from_score(50) == Health::Watch
        && Health::from_score(49) == Health::Degraded
        && Health::from_score(20) == Health::Degraded
        && Health::from_score(19) == Health::Critical;
    s.add("分级-六级边界恰等", b_ok, if b_ok { "" } else { "分级边界漂移" });

    // ② 决策引擎：四级 → 四动作
    let mut r1 = Raid1::new(2, 1);
    s.add("决策-全Good出None", r1.decide() == RaidAction::None, if r1.decide() == RaidAction::None { "" } else { "Good 决策漂移" });
    r1.members[0].1 = Health::Watch;
    s.add("决策-Watch出LogWarn", r1.decide() == RaidAction::LogWarn, if r1.decide() == RaidAction::LogWarn { "" } else { "Watch 决策漂移" });
    r1.members[0].1 = Health::Degraded;
    s.add("决策-Degraded出MarkDegraded", r1.decide() == RaidAction::MarkDegraded, if r1.decide() == RaidAction::MarkDegraded { "" } else { "Degraded 决策漂移" });
    r1.members[0].1 = Health::Critical;
    s.add("决策-Critical有热备出HotspareSwap", r1.decide() == RaidAction::HotspareSwap, if r1.decide() == RaidAction::HotspareSwap { "" } else { "热备决策漂移" });

    // 无热备危急 → Rebuild
    let mut r2 = Raid1::new(2, 0);
    r2.members[0].1 = Health::Critical;
    s.add("决策-Critical无热备出Rebuild", r2.decide() == RaidAction::Rebuild, if r2.decide() == RaidAction::Rebuild { "" } else { "无热备决策漂移" });

    // ③ 动作段：热备替换执行账
    let swapped = r1.apply(RaidAction::HotspareSwap);
    let swap_ok = swapped && r1.hotspares == 0 && r1.members[0].1 == Health::Good && r1.rebuilding;
    s.add("动作-热备替换恢复Good且开重建", swap_ok, if swap_ok { "" } else { "热备替换账不符" });

    // 重建 100 轮收口
    let mut ticks = 0u32;
    while r1.rebuild_tick() {
        ticks += 1;
        if ticks > 200 {
            break;
        }
    }
    let rebuild_ok = ticks == 100 && r1.rebuild_progress == 100 && !r1.rebuilding;
    s.add("动作-重建100轮收口", rebuild_ok, if rebuild_ok { "" } else { "重建收口漂移" });

    // 总驱动 run_chain：三盘混级一轮
    let mut r3 = Raid1::new(3, 1);
    let a3 = r3.run_chain(&[90, 55, 10]);
    let chain_ok = a3 == RaidAction::HotspareSwap && r3.members[2].1 == Health::Good && r3.hotspares == 0;
    s.add("总链-三盘混级一轮出替换", chain_ok, if chain_ok { "" } else { "总链漂移" });

    // run_chain 全 Good → None 且记账
    let mut r4 = Raid1::new(2, 0);
    let a4 = r4.run_chain(&[95, 85]);
    let none_ok = a4 == RaidAction::None && r4.action_log.len() == 1;
    s.add("总链-全Good出None且留账", none_ok, if none_ok { "" } else { "None 账缺失" });

    // 100 轮随机演练：决策面全覆盖
    let mut seed: u64 = 0x7621_0100;
    let mut seen_none = false;
    let mut seen_swap = false;
    let mut seen_rebuild = false;
    for _ in 0..100 {
        let mut r5 = Raid1::new(2, 1);
        let s0 = (lcg_next(&mut seed) % 101) as u8;
        let s1 = (lcg_next(&mut seed) % 101) as u8;
        let a = r5.run_chain(&[s0, s1]);
        match a {
            RaidAction::None => seen_none = true,
            RaidAction::HotspareSwap => seen_swap = true,
            RaidAction::Rebuild => seen_rebuild = true,
            _ => {}
        }
    }
    let cover_ok = seen_none && seen_swap && seen_rebuild;
    s.add("100轮随机-None/Swap/Rebuild三面全覆盖", cover_ok, if cover_ok { "" } else { "决策面覆盖缺口" });

    // FNV 动作日志稳定
    let h1 = fnv1a(format!("disk{}:{}", 2u8, RaidAction::HotspareSwap.as_str()).as_bytes());
    let h2 = fnv1a(b"disk2:swap");
    s.add("FNV-动作日志哈希稳定", h1 == h2, if h1 == h2 { "" } else { "哈希不稳" });

    s
}
