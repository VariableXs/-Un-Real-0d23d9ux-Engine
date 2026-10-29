//! CGPU-F0732 · 场景内容基准（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0732，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：场景内容基准——按场景内容规模（元素数/字节量/类别分布）
//!    做确定性负载评分：类别加权成本模型、档位阈值判定、
//!    场景间相对分与总分守恒账。
//!
//! 设计纪律（与 F0715 的差异：F0715 以"帧周期成本+回归门"为核心，
//! 本单以"内容加权评分+档位阈值"为核心）：
// - 内容类别四类闭集：Node/Text/Texture/Script，单位成本表写死
//   （points/件，规格默认档：1/2/16/64）。
// - 加权总分：Σ(件数×单位成本)，溢出安全；归一化为千分制
//   （相对基线场景集的最重场景）。
// - 档位三闭集：Light(<300)/Medium(<700)/Heavy(其余)。
// - 总分守恒：Σ场景分 == 全场景类别计数×成本的总分（分账=总账）。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x49c0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x49c0，批内 CGPU-F0732 独占）。
pub const SEG_BASE: u16 = 0x49c0;

/// 场景重号。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 场景参数非法（空名/类别计数含负语义）。
pub const E_SCENE: u16 = SEG_BASE + 2;
/// 归一化基线非法（无场景或最大分为 0）。
pub const E_NORM: u16 = SEG_BASE + 3;
/// 总分守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 档位参数非法。
pub const E_TIER: u16 = SEG_BASE + 5;

/// 单位成本表（points/件，规格默认档）：
/// Node=1 Text=2 Texture=16 Script=64。
pub const COST_NODE: u64 = 1;
pub const COST_TEXT: u64 = 2;
pub const COST_TEX: u64 = 16;
pub const COST_SCRIPT: u64 = 64;

/// 档位阈值：Light < 300，Medium < 700，Heavy ≥ 700（千分制）。
pub const TIER_MEDIUM_PM: u32 = 300;
pub const TIER_HEAVY_PM: u32 = 700;

/// 内容类别（闭集四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentWt {
    /// 结构节点。
    Node,
    /// 文本。
    Text,
    /// 纹理。
    Texture,
    /// 脚本。
    Script,
}

impl ContentWt {
    /// 单位成本。
    pub fn cost(self) -> u64 {
        match self {
            ContentWt::Node => COST_NODE,
            ContentWt::Text => COST_TEXT,
            ContentWt::Texture => COST_TEX,
            ContentWt::Script => COST_SCRIPT,
        }
    }
    /// 类序。
    pub fn ord(self) -> usize {
        match self {
            ContentWt::Node => 0,
            ContentWt::Text => 1,
            ContentWt::Texture => 2,
            ContentWt::Script => 3,
        }
    }
}

/// 场景内容计量：四类件数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentCensus {
    /// 四类件数（按类序）。
    pub counts: [u32; 4],
}

impl ContentCensus {
    /// 加权分（溢出安全）。
    pub fn score(&self) -> Result<u64, u16> {
        let mut s: u64 = 0;
        for i in 0..4 {
            let c = match i {
                0 => ContentWt::Node,
                1 => ContentWt::Text,
                2 => ContentWt::Texture,
                _ => ContentWt::Script,
            };
            s = s
                .checked_add((self.counts[i] as u64).checked_mul(c.cost()).ok_or(E_SCENE)?)
                .ok_or(E_SCENE)?;
        }
        Ok(s)
    }
    /// 总件数。
    pub fn items(&self) -> u64 {
        self.counts.iter().map(|c| *c as u64).sum()
    }
}

/// 档位（闭集三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 轻量。
    Light,
    /// 中量。
    Medium,
    /// 重量。
    Heavy,
}

/// 千分制定档。
pub fn tier_of(score_pm: u32) -> Result<Tier, u16> {
    if score_pm > 1000 {
        return Err(E_TIER);
    }
    Ok(if score_pm < TIER_MEDIUM_PM {
        Tier::Light
    } else if score_pm < TIER_HEAVY_PM {
        Tier::Medium
    } else {
        Tier::Heavy
    })
}

/// 基准场景。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchScene {
    /// 场景名。
    pub name: &'static str,
    /// 内容计量。
    pub census: ContentCensus,
}

/// 基准器：场景集 + 评分账。
#[derive(Clone, Debug, Default)]
pub struct ContentBench {
    scenes: Vec<BenchScene>,
}

impl ContentBench {
    pub fn new() -> Self {
        Self { scenes: Vec::new() }
    }
    /// 登记场景（不重名）。
    pub fn add(&mut self, s: BenchScene) -> Result<(), u16> {
        if s.name.is_empty() {
            return Err(E_SCENE);
        }
        if self.scenes.iter().any(|x| x.name == s.name) {
            return Err(E_DUP);
        }
        s.census.score()?; // 预检溢出
        self.scenes.push(s);
        Ok(())
    }
    /// 场景数。
    pub fn len(&self) -> usize {
        self.scenes.len()
    }
    /// 最大原始分（归一化基）。
    pub fn max_raw(&self) -> Result<u64, u16> {
        let mut m: u64 = 0;
        for s in &self.scenes {
            let sc = s.census.score()?;
            if sc > m {
                m = sc;
            }
        }
        if m == 0 {
            return Err(E_NORM);
        }
        Ok(m)
    }
    /// 逐场景归一化评分（千分制）：raw*1000/max_raw。
    pub fn scores_pm(&self) -> Result<Vec<(&'static str, u32, Tier)>, u16> {
        if self.scenes.is_empty() {
            return Err(E_NORM);
        }
        let m = self.max_raw()?;
        let mut out = Vec::with_capacity(self.scenes.len());
        for s in &self.scenes {
            let raw = s.census.score()?;
            let pm = (raw * 1000 / m) as u32;
            out.push((s.name, pm, tier_of(pm)?));
        }
        Ok(out)
    }
    /// 总分守恒：Σ场景原始分 == 全类别 Σ件数×成本（分账=总账）。
    pub fn audit(&self) -> Result<(), u16> {
        let mut ledger: u64 = 0;
        for s in &self.scenes {
            ledger = ledger
                .checked_add(s.census.score()?)
                .ok_or(E_LEDGER)?;
        }
        let mut total: u64 = 0;
        let mut tally = [0u64; 4];
        for s in &self.scenes {
            for i in 0..4 {
                tally[i] += s.census.counts[i] as u64;
            }
        }
        for i in 0..4 {
            let c = match i {
                0 => ContentWt::Node,
                1 => ContentWt::Text,
                2 => ContentWt::Texture,
                _ => ContentWt::Script,
            };
            total = total
                .checked_add(tally[i].checked_mul(c.cost()).ok_or(E_LEDGER)?)
                .ok_or(E_LEDGER)?;
        }
        if ledger != total {
            return Err(E_LEDGER);
        }
        Ok(())
    }
}

/// 评分报告行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreRow {
    /// 场景名。
    pub name: &'static str,
    /// 原始分。
    pub raw: u64,
    /// 归一化千分。
    pub pm: u32,
    /// 档位。
    pub tier: Tier,
    /// 总件数。
    pub items: u64,
}

/// 报告化（按 pm 升序）。
pub fn report(b: &ContentBench) -> Result<Vec<ScoreRow>, u16> {
    let scores = b.scores_pm()?;
    let mut rows: Vec<ScoreRow> = Vec::with_capacity(scores.len());
    for (name, pm, tier) in scores {
        let s = b.scenes.iter().find(|x| x.name == name).ok_or(E_NORM)?;
        rows.push(ScoreRow {
            name,
            raw: s.census.score()?,
            pm,
            tier,
            items: s.census.items(),
        });
    }
    for i in 1..rows.len() {
        let mut j = i;
        while j > 0 && rows[j - 1].pm > rows[j].pm {
            rows.swap(j - 1, j);
            j -= 1;
        }
    }
    Ok(rows)
}

/// 自例程：主路径（3 场景→评分→档位→守恒）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut b = ContentBench::new();
    // 场景 1：轻量（800 节点 + 200 文本 = 800+400=1200）。
    b.add(BenchScene {
        name: "home",
        census: ContentCensus { counts: [800, 200, 0, 0] },
    })?;
    // 场景 2：中量（2000 节点 + 500 文本 + 50 纹理 = 2000+1000+800=3800）。
    b.add(BenchScene {
        name: "feed",
        census: ContentCensus { counts: [2000, 500, 50, 0] },
    })?;
    // 场景 3：重量（3000 节点 + 1000 文本 + 100 纹理 + 20 脚本
    //   = 3000+2000+1600+1280=7880，最重 → 基线）。
    b.add(BenchScene {
        name: "app",
        census: ContentCensus { counts: [3000, 1000, 100, 20] },
    })?;
    if b.len() != 3 {
        return Err(E_SCENE);
    }
    b.audit()?;
    // 归一化：基线 7880。home = 1200*1000/7880 = 152（Light）；
    // feed = 3800*1000/7880 = 482（Medium）；app = 1000（Heavy）。
    let rows = report(&b)?;
    if rows.len() != 3 {
        return Err(E_NORM);
    }
    if rows[0].name != "home" || rows[0].pm != 152 || rows[0].tier != Tier::Light {
        return Err(E_LEDGER);
    }
    if rows[1].name != "feed" || rows[1].pm != 482 || rows[1].tier != Tier::Medium {
        return Err(E_LEDGER);
    }
    if rows[2].name != "app" || rows[2].pm != 1000 || rows[2].tier != Tier::Heavy {
        return Err(E_LEDGER);
    }
    // 件数：home 1000；feed 2550；app 4120。
    if rows[0].items != 1000 || rows[1].items != 2550 || rows[2].items != 4120 {
        return Err(E_LEDGER);
    }
    // 档位边界：299 Light，300 Medium，699 Medium，700 Heavy。
    if tier_of(299)? != Tier::Light || tier_of(300)? != Tier::Medium
        || tier_of(699)? != Tier::Medium || tier_of(700)? != Tier::Heavy
    {
        return Err(E_TIER);
    }
    // 错误路径 0：重名。
    if b.add(BenchScene {
        name: "home",
        census: ContentCensus { counts: [1, 0, 0, 0] },
    }) != Err(E_DUP)
    {
        return Err(E_DUP);
    }
    // 错误路径 1：空名。
    if b.add(BenchScene {
        name: "",
        census: ContentCensus { counts: [1, 0, 0, 0] },
    }) != Err(E_SCENE)
    {
        return Err(E_SCENE);
    }
    // 错误路径 2：空基准归一化。
    let b0 = ContentBench::new();
    if b0.scores_pm() != Err(E_NORM) {
        return Err(E_NORM);
    }
    // 错误路径 3：全零场景集 → max_raw 0 → E_NORM。
    let mut bz = ContentBench::new();
    bz.add(BenchScene {
        name: "zero",
        census: ContentCensus { counts: [0, 0, 0, 0] },
    })?;
    if bz.scores_pm() != Err(E_NORM) {
        return Err(E_NORM);
    }
    // 错误路径 4：档位越界。
    if tier_of(1001) != Err(E_TIER) {
        return Err(E_TIER);
    }
    // 单位成本闭集：1/2/16/64。
    if ContentWt::Node.cost() != 1
        || ContentWt::Text.cost() != 2
        || ContentWt::Texture.cost() != 16
        || ContentWt::Script.cost() != 64
    {
        return Err(E_SCENE);
    }
    Ok(0)
}
