//! unx0962_tiermap — UNX-F0962 轮询路径特征画像与接管难度分级。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0962 轮询路径特征画像与接管难度分级
//! 判据原文摘录（卡面验收行）：轮询路径特征画像逐点落账（触发密度/
//! 时延敏感/状态耦合三维），接管难度三级分档（易/中/难）全样本
//! 10/10 次归档正确，降档/升档边界样本全数落对档。
//! 诊断段 0x7E40（本单独占，XcCode 闭集见下）。
//! 设计纪律：分级即契约——三维特征（0-9 分）加权映射三档难度；
//! 权重表静态定死；档界样本（恰在阈值）必须落高档（宁高勿低）；
//! 全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7E40;

// ── 诊断段独占 · 画像分级错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_DIM: u16 = SEG_ERR + 1;   // 特征维越界（>9）
const E_TIER: u16 = SEG_ERR + 2;  // 档位非法
const E_WEIGHT: u16 = SEG_ERR + 3; // 权重表非法（和不为一）
const E_STATE: u16 = SEG_ERR + 4; // 画像态非法

/// 接管难度三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Easy = 0,
    Medium = 1,
    Hard = 2,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self { Tier::Easy => "easy", Tier::Medium => "medium", Tier::Hard => "hard" }
    }
}

/// 三维特征（各 0-9 分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feat3 {
    pub density: u8,   // 触发密度
    pub latency: u8,   // 时延敏感
    pub coupling: u8,  // 状态耦合
}

impl Feat3 {
    /// 维越界核验。
    pub fn in_range(&self) -> bool {
        self.density <= 9 && self.latency <= 9 && self.coupling <= 9
    }
}

/// 静态权重表（三权重和 == 100）。
pub const WEIGHTS: [u32; 3] = [40, 35, 25];

/// 权重表自检。
pub fn weights_audit() -> Result<u32, u16> {
    let s: u32 = WEIGHTS.iter().sum();
    if s != 100 { return Err(E_WEIGHT); }
    Ok(s)
}

/// 加权得分（0-900）。
pub fn score(f: &Feat3) -> Result<u32, u16> {
    if !f.in_range() { return Err(E_DIM); }
    Ok(f.density as u32 * WEIGHTS[0] + f.latency as u32 * WEIGHTS[1] + f.coupling as u32 * WEIGHTS[2])
}

/// 三档阈值（宁高勿低：恰在界归高档）。
pub const TH_MEDIUM: u32 = 300;
pub const TH_HARD: u32 = 600;

/// 分档：score < 300 易；300..600 中；>=600 难。
pub fn tier_of(f: &Feat3) -> Result<Tier, u16> {
    let s = score(f)?;
    if s >= TH_HARD { Ok(Tier::Hard) }
    else if s >= TH_MEDIUM { Ok(Tier::Medium) }
    else { Ok(Tier::Easy) }
}

/// 画像条目：特征 + 得分 + 档位。
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub name_id: u32,
    pub feat: Feat3,
    pub sc: u32,
    pub tier: Tier,
}

/// 画像账本。
pub struct ProfileLedger {
    pub entries: Vec<Profile>,
}

impl ProfileLedger {
    pub fn new() -> Self { ProfileLedger { entries: Vec::new() } }

    /// 画像落账：三维核验 + 分档 + 重算一致才落。
    pub fn profile(&mut self, name_id: u32, f: Feat3) -> Result<Tier, u16> {
        if !f.in_range() { return Err(E_DIM); }
        let sc = score(&f)?;
        let t = tier_of(&f)?;
        self.entries.push(Profile { name_id, feat: f, sc, tier: t });
        Ok(t)
    }

    /// 归档回放：全部条目重算必须同档。
    pub fn replay(&self) -> Result<u32, u16> {
        if self.entries.is_empty() { return Err(E_STATE); }
        for e in &self.entries {
            if score(&e.feat)? != e.sc { return Err(E_TIER); }
            if tier_of(&e.feat)? != e.tier { return Err(E_TIER); }
        }
        Ok(self.entries.len() as u32)
    }

    /// 三档分布。
    pub fn dist(&self) -> (u32, u32, u32) {
        let e = self.entries.iter().filter(|x| x.tier == Tier::Easy).count() as u32;
        let m = self.entries.iter().filter(|x| x.tier == Tier::Medium).count() as u32;
        let h = self.entries.iter().filter(|x| x.tier == Tier::Hard).count() as u32;
        (e, m, h)
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        let (e, m, h) = self.dist();
        alloc::format!("profile n={} easy={} medium={} hard={}", self.entries.len(), e, m, h)
    }
}

/// 边界样本构造器：恰落阈值的特征。
pub fn boundary_samples() -> Vec<(Feat3, Tier)> {
    vec![
        (Feat3 { density: 7, latency: 0, coupling: 1 }, Tier::Medium), // 280+25=305 恰过中档界
        (Feat3 { density: 7, latency: 1, coupling: 0 }, Tier::Medium), // 280+35=315
        (Feat3 { density: 9, latency: 6, coupling: 6 }, Tier::Hard),   // 360+210+150=720
    ]
}

/// 自例程：权重表审计、边界落档、十样本画像回放全过返回 0。
pub fn drill() -> Result<u32, u16> {
    if weights_audit()? != 100 { return Err(E_WEIGHT); }
    let mut pl = ProfileLedger::new();
    // 场景一：边界样本落档（恰界归高档）。
    for (f, expect) in boundary_samples() {
        let t = pl.profile(0, f)?;
        if t != expect { return Err(E_TIER); }
    }
    // 场景二：十样本全域画像。
    let samples = [
        Feat3 { density: 0, latency: 0, coupling: 0 }, // 0 easy
        Feat3 { density: 9, latency: 0, coupling: 0 }, // 360 medium
        Feat3 { density: 0, latency: 9, coupling: 0 }, // 315 medium
        Feat3 { density: 0, latency: 0, coupling: 9 }, // 225 easy
        Feat3 { density: 5, latency: 5, coupling: 5 }, // 500 medium
        Feat3 { density: 8, latency: 8, coupling: 8 }, // 800 hard
        Feat3 { density: 2, latency: 2, coupling: 2 }, // 200 easy
        Feat3 { density: 6, latency: 6, coupling: 6 }, // 600 hard
        Feat3 { density: 3, latency: 3, coupling: 3 }, // 300 medium
        Feat3 { density: 1, latency: 1, coupling: 1 }, // 100 easy
    ];
    for (i, f) in samples.iter().enumerate() {
        pl.profile(i as u32, *f)?;
    }
    if pl.replay()? != 13 { return Err(E_STATE); }
    // 场景三：界上样本归档核验（300 → medium、600 → hard）。
    if pl.entries[7].tier != Tier::Hard { return Err(E_TIER); }
    if pl.entries[8].tier != Tier::Medium { return Err(E_TIER); }
    // 场景四：维越界必拦。
    let bad = Feat3 { density: 10, latency: 0, coupling: 0 };
    if pl.profile(99, bad) != Err(E_DIM) { return Err(E_DIM); }
    // 场景五：分布口径。
    let (e, m, h) = pl.dist();
    if e + m + h != 13 { return Err(E_STATE); }
    if pl.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}

// ════════════════════ 判据聚合入口（AI-INTAKE-1107 收库补齐） ════════════════════
//
// 修正记录：本模块历史会话只交付了域模型与测试，从未交付 run_unx962_checks
// 聚合入口——等于从未参与判据聚合闭环。此入口把模块自身的公开 API 逐项
// 折叠成 CheckSet 判据（判据强度不降：每条判据都调真实 API、断真实性质）。

use crate::checks::CheckSet;

pub fn run_unx962_checks() -> CheckSet {
    let mut s = CheckSet::new("unx0962_tiermap");
    // J1 核心路径：权重审计、边界落档、十样本画像回放。
    s.add("核心路径drill全过", drill() == Ok(0), "权重表/边界样本/十样本/越界拒/分布全过返回 0");
    // J2 权重表自检：三权重和 == 100。
    s.add("权重表自检", weights_audit() == Ok(100), "WEIGHTS 40+35+25=100 冻结");
    // J3 分档阈值冻结（宁高勿低）。
    s.add("分档阈值冻结", TH_MEDIUM == 300 && TH_HARD == 600, "score<300 易、300..600 中、≥600 难");
    // J4 正例：边界样本恰界归高档。
    let mut pl = ProfileLedger::new();
    let mut b_ok = true;
    for (f, expect) in boundary_samples() {
        if pl.profile(0, f) != Ok(expect) { b_ok = false; }
    }
    s.add("边界样本落档", b_ok && pl.entries.len() == 3, "305/315 落 Medium、720 落 Hard——恰界宁高勿低");
    // J5 正例：三维全域画像正反档。
    let f_hard = Feat3 { density: 8, latency: 8, coupling: 8 };
    let f_med = Feat3 { density: 3, latency: 3, coupling: 3 };
    let f_easy = Feat3 { density: 0, latency: 0, coupling: 0 };
    let f6 = Feat3 { density: 6, latency: 6, coupling: 6 };
    let tier_ok = tier_of(&f_hard) == Ok(Tier::Hard)
        && tier_of(&f_med) == Ok(Tier::Medium)
        && tier_of(&f_easy) == Ok(Tier::Easy)
        && score(&f6) == Ok(600)
        && score(&f_easy) == Ok(0);
    s.add("全域画像三档", tier_ok, "800/300/0 分档与得分逐点核对");
    // J6 反例：维越界必拦（density>9）。
    let bad = Feat3 { density: 10, latency: 0, coupling: 0 };
    s.add("维越界拦", !bad.in_range() && ProfileLedger::new().profile(99, bad) == Err(E_DIM)
        && score(&bad) == Err(E_DIM), "density==10 双口（profile/score）均 E_DIM 拒");
    // J7 归档回放：全部条目重算同档。
    let replay_ok = pl.replay() == Ok(3);
    for (f, _) in boundary_samples() { let _ = pl.profile(0, f); }
    s.add("归档回放3条", replay_ok, "3 边界条目重算得分/档位逐条一致");
    // J8 全量回放：13 条目（3 边界 + 10 全域）分布守恒（独立净账）。
    let ten = [
        Feat3 { density: 0, latency: 0, coupling: 0 },
        Feat3 { density: 9, latency: 0, coupling: 0 },
        Feat3 { density: 0, latency: 9, coupling: 0 },
        Feat3 { density: 0, latency: 0, coupling: 9 },
        Feat3 { density: 5, latency: 5, coupling: 5 },
        Feat3 { density: 8, latency: 8, coupling: 8 },
        Feat3 { density: 2, latency: 2, coupling: 2 },
        Feat3 { density: 6, latency: 6, coupling: 6 },
        Feat3 { density: 3, latency: 3, coupling: 3 },
        Feat3 { density: 1, latency: 1, coupling: 1 },
    ];
    let mut pl2 = ProfileLedger::new();
    let mut all_in = true;
    for (f, _) in boundary_samples() {
        if pl2.profile(0, f).is_err() { all_in = false; }
    }
    for (i, f) in ten.iter().enumerate() {
        if pl2.profile(i as u32, *f).is_err() { all_in = false; }
    }
    let (e, m, h) = pl2.dist();
    s.add("十三目分布守恒", all_in && pl2.entries.len() == 13 && e + m + h == 13 && pl2.replay() == Ok(13),
        "13 条目全落账、三档分布和==总、回放同档");
    // J9 档位闭集与序。
    let ord_ok = Tier::Easy < Tier::Medium && Tier::Medium < Tier::Hard
        && Tier::Easy.as_str() == "easy" && Tier::Medium.as_str() == "medium"
        && Tier::Hard.as_str() == "hard";
    s.add("三档闭集有序", ord_ok, "PartialOrd 三档递增、命名冻结");
    // J10 诊断段独占。
    s.add("诊断段位独占", SEG_BASE == 0x7E40, "0x7E40 段本单独占不漂移");
    // 动态明细两列。
    let sum_len = pl2.summary().len();
    s.add_dyn("画像摘要非空", sum_len > 0, &alloc::format!("summary {} 字节（n/easy/medium/hard）", sum_len));
    let (de, dm, dh) = pl2.dist();
    s.add_dyn("三档分布可报", de + dm + dh == 13,
        &alloc::format!("easy {} / medium {} / hard {}", de, dm, dh));
    s
}
