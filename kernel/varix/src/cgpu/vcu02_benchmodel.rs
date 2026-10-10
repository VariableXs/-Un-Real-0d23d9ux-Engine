//! CGPU-F3202 · 基准测试模型（CGPU-U 域 · 基准与 Bench 域 · U02 组 · 目标 320 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3202`
//!
//! **判据（锚点原文）**：三维、模型表、一组、判据。
//!
//! **职责定位（锚点原文）**：模型：基准测试模型（场景×配置×指标三维
//! ——模型实现——模型表；测试（模型一组）。
//!
//! ## 一、三维模型（判据一：场景×配置×指标）
//!
//! 基准考卷的每一格是三元组（[`CellKey`]）：场景维（考哪个场景）×
//! 配置维（用什么配置跑）×指标维（量哪个指标）——三维齐一格才成立；
//! 缺维/表外维显性拒（[`BmCode`]）。指标维直接复用 F3201
//! [`super::vcu01_bencharch::BENCH_ITEMS`] 五项闭集——**集成复用**：
//! 指标口径单源，U 域不出现第二份基准项表。
//!
//! ## 二、模型表（判据二：稀疏账 + 状态机 + 守恒）
//!
//! 全跑满三维是天文数字——模型表是**稀疏账**（[`BenchModel`] 只存
//! 已登记格），登记即建格：Empty→Queued→Running→Scored→Sealed
//! （[`CellPhase`] 状态机单向，回退显性拒）；**封存不可改**：Sealed
//! 格重测走版本升级（同键新版本格，历史分保留——分数是档案不是
//! 覆盖写）；三维键唯一（重复登记显性拒）；表守恒：登记计数=状态
//! 计数积分（[`BenchModel::conserved`]——账实相符的可断言面）。
//!
//! ## 三、测试一组（判据三：模型一组的可执行面）
//!
//! 模型探针组（[`model_probe_group`]）：三维齐格登记/表外维拒绝/
//! 状态单向/封存不可改/版本升级留档/守恒六探——模型一组的锚点
//! 「测试（模型一组）」的功能面（判据押后模式下以可调用探针落位，
//! 宿主回归随闸门补测）。
//!
//! **对接**：F3201（U01 开工——指标闭集与四段流水线上游）；F3203
//! （场景分类——场景维的语义分册）；F3204（计分体系——Scored 格的
//! 分数语义下游）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const U02_MODEL_VERSION: &str = "U02-benchmodel-v1";

/// 场景维容量（当前策略值，可调）。
pub const MAX_SCENES: u32 = 256;

/// 配置维容量（当前策略值，可调）。
pub const MAX_CONFIGS: u32 = 128;

/// 单元容量上限（稀疏账上限；当前策略值，可调）。
pub const MAX_CELLS: usize = 4_096;

// ---------------------------------------------------------------------------
// 二、诊断码（0x6D 段独占：U02 模型面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x6D 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmCode {
    /// 场景维表外。
    SceneUnknown,
    /// 配置维表外。
    ConfigUnknown,
    /// 指标维表外（指标闭集唯 F3201 五项）。
    MetricUnknown,
    /// 格已存在（三维键重复登记）。
    CellDuplicate,
    /// 状态回退（单向流水线违例）。
    PhaseRegression,
    /// 封存格被改写。
    SealedRewrite,
    /// 单元容量满。
    CapacityFull,
    /// 格未登记。
    CellMissing,
}

impl BmCode {
    /// 人读诊断（发生了什么/为什么/怎么办）。
    pub const fn detail(self) -> &'static str {
        match self {
            BmCode::SceneUnknown => "场景维表外：场景须先入 F3203 分类册，建议先登记场景再建格",
            BmCode::ConfigUnknown => "配置维表外：配置须先入配置册，建议核对配置 id",
            BmCode::MetricUnknown => "指标维表外：指标闭集唯 F3201 BENCH_ITEMS 五项，建议按闭集取用",
            BmCode::CellDuplicate => "三维键重复：同（场景×配置×指标）格已在账，建议改走版本升级",
            BmCode::PhaseRegression => "状态回退：Empty→Queued→Running→Scored→Sealed 单向，回退显性拒",
            BmCode::SealedRewrite => "封存格不可改写：重测请走版本升级（新版本格留档历史分）",
            BmCode::CapacityFull => "模型表容量已满（当前策略值）：建议归档旧版本格或扩容评审",
            BmCode::CellMissing => "格未登记：先登记（建格）再推进状态",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、三维键（判据一的载体）
// ---------------------------------------------------------------------------

/// 三维键（场景×配置×指标；三维齐一格才成立）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CellKey {
    /// 场景维 id（F3203 分类册登记号）。
    pub scene: u32,
    /// 配置维 id（配置册登记号）。
    pub config: u32,
    /// 指标维（F3201 BENCH_ITEMS 下标 0..5）。
    pub metric: u8,
    /// 版本（同键重测升级；1 起步）。
    pub version: u32,
}

/// 指标维核验（闭集唯 F3201 五项——集成复用口径单源）。
pub fn metric_ok(metric: u8) -> Result<(), BmCode> {
    if (metric as usize) < super::vcu01_bencharch::BENCH_ITEMS.len() {
        Ok(())
    } else {
        Err(BmCode::MetricUnknown)
    }
}

/// 三维键核验（容量界 + 指标闭集）。
pub fn key_ok(k: &CellKey) -> Result<(), BmCode> {
    if k.scene == 0 || k.scene > MAX_SCENES {
        return Err(BmCode::SceneUnknown);
    }
    if k.config == 0 || k.config > MAX_CONFIGS {
        return Err(BmCode::ConfigUnknown);
    }
    metric_ok(k.metric)
}

// ---------------------------------------------------------------------------
// 四、单元状态机（判据二：单向五态）
// ---------------------------------------------------------------------------

/// 单元相位（单向：Empty→Queued→Running→Scored→Sealed）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellPhase {
    /// 已建格未入队。
    Empty,
    /// 已入队待跑。
    Queued,
    /// 运行中。
    Running,
    /// 已出分（分数在账）。
    Scored,
    /// 已封存（档案态，不可改写）。
    Sealed,
}

impl CellPhase {
    /// 相位序数（单向性判据的载体）。
    pub const fn ordinal(self) -> u8 {
        match self {
            CellPhase::Empty => 0,
            CellPhase::Queued => 1,
            CellPhase::Running => 2,
            CellPhase::Scored => 3,
            CellPhase::Sealed => 4,
        }
    }

    /// 下一相位（Sealed 的下一相位是自己——终态）。
    pub const fn next(self) -> CellPhase {
        match self {
            CellPhase::Empty => CellPhase::Queued,
            CellPhase::Queued => CellPhase::Running,
            CellPhase::Running => CellPhase::Scored,
            CellPhase::Scored => CellPhase::Sealed,
            CellPhase::Sealed => CellPhase::Sealed,
        }
    }
}

/// 模型单元（一格格账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchCell {
    /// 三维键（含版本）。
    pub key: CellKey,
    /// 相位。
    pub phase: CellPhase,
    /// 分数（Scored/Sealed 相位在账；口径归 F3204 计分体系）。
    pub score: u64,
    /// 出分时间戳（注入 tick；档案追溯面）。
    pub scored_at: u64,
}

// ---------------------------------------------------------------------------
// 五、模型表（判据二：稀疏账 + 守恒）
// ---------------------------------------------------------------------------

/// 基准测试模型表（稀疏账：只存已登记格）。
#[derive(Clone, Debug)]
pub struct BenchModel {
    /// 单元账（按键升序保持——确定性遍历）。
    cells: Vec<BenchCell>,
}

impl BenchModel {
    /// 新模型表。
    pub fn new() -> BenchModel {
        BenchModel { cells: Vec::new() }
    }

    /// 建格（三维核验 + 键唯一 + 容量闸）。
    pub fn declare(&mut self, k: CellKey) -> Result<(), BmCode> {
        key_ok(&k)?;
        if self.cells.len() >= MAX_CELLS {
            return Err(BmCode::CapacityFull);
        }
        if self.pos_of(&k).is_some() {
            return Err(BmCode::CellDuplicate);
        }
        let pos = self.insert_pos(&k);
        self.cells.insert(
            pos,
            BenchCell {
                key: k,
                phase: CellPhase::Empty,
                score: 0,
                scored_at: 0,
            },
        );
        Ok(())
    }

    /// 相位推进（单向 + 封存闸）。
    pub fn advance(&mut self, k: &CellKey) -> Result<CellPhase, BmCode> {
        let pos = match self.pos_of(k) {
            Some(p) => p,
            None => return Err(BmCode::CellMissing),
        };
        let cur = match self.cells.get(pos) {
            Some(c) => c.phase,
            None => return Err(BmCode::CellMissing),
        };
        if cur == CellPhase::Sealed {
            return Err(BmCode::SealedRewrite);
        }
        let nxt = cur.next();
        if let Some(c) = self.cells.get_mut(pos) {
            c.phase = nxt;
        }
        Ok(nxt)
    }

    /// 出分（Running→Scored 携带分数；相位错序显性拒）。
    pub fn record_score(&mut self, k: &CellKey, score: u64, ts: u64) -> Result<(), BmCode> {
        let pos = match self.pos_of(k) {
            Some(p) => p,
            None => return Err(BmCode::CellMissing),
        };
        let cur = match self.cells.get(pos) {
            Some(c) => c.phase,
            None => return Err(BmCode::CellMissing),
        };
        if cur != CellPhase::Running {
            return Err(BmCode::PhaseRegression);
        }
        if let Some(c) = self.cells.get_mut(pos) {
            c.phase = CellPhase::Scored;
            c.score = score;
            c.scored_at = ts;
        }
        Ok(())
    }

    /// 版本升级（封存格重测的唯一路径：同键 version+1 新格，旧格留档）。
    pub fn supersede(&mut self, k: &CellKey) -> Result<CellKey, BmCode> {
        let pos = match self.pos_of(k) {
            Some(p) => p,
            None => return Err(BmCode::CellMissing),
        };
        let sealed = match self.cells.get(pos) {
            Some(c) => c.phase == CellPhase::Sealed,
            None => false,
        };
        if !sealed {
            return Err(BmCode::PhaseRegression);
        }
        let nk = CellKey {
            scene: k.scene,
            config: k.config,
            metric: k.metric,
            version: k.version.saturating_add(1),
        };
        self.declare(nk)?;
        Ok(nk)
    }

    /// 格读取（只读档案面）。
    pub fn cell_of(&self, k: &CellKey) -> Option<BenchCell> {
        let p = self.pos_of(k)?;
        self.cells.get(p).cloned()
    }

    /// 在账格数。
    pub const fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// 守恒核验（判据二：账严格按键全序排列且无重复键——
    /// 任何乱序/重复都意味着登记闸被绕过；恒真核验是死判据，禁用）。
    pub fn conserved(&self) -> bool {
        if self.cells.is_empty() {
            return true;
        }
        let mut i = 1usize;
        while i < self.cells.len() {
            let (prev, cur) = match (self.cells.get(i - 1), self.cells.get(i)) {
                (Some(a), Some(b)) => (a, b),
                _ => return false,
            };
            let pa = (prev.key.scene, prev.key.config, prev.key.metric, prev.key.version);
            let pc = (cur.key.scene, cur.key.config, cur.key.metric, cur.key.version);
            if pa >= pc {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 某格是否封存。
    pub fn sealed(&self, k: &CellKey) -> bool {
        match self.pos_of(k) {
            Some(p) => match self.cells.get(p) {
                Some(c) => c.phase == CellPhase::Sealed,
                None => false,
            },
            None => false,
        }
    }

    fn pos_of(&self, k: &CellKey) -> Option<usize> {
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.key.scene == k.scene
                    && c.key.config == k.config
                    && c.key.metric == k.metric
                    && c.key.version == k.version
                {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn insert_pos(&self, k: &CellKey) -> usize {
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                let ck = &c.key;
                let cand = (k.scene, k.config, k.metric, k.version);
                let held = (ck.scene, ck.config, ck.metric, ck.version);
                if cand < held {
                    return i;
                }
            }
            i += 1;
        }
        self.cells.len()
    }
}

impl Default for BenchModel {
    fn default() -> BenchModel {
        BenchModel::new()
    }
}

// ---------------------------------------------------------------------------
// 六、模型探针组（判据三：测试（模型一组）的可执行面）
// ---------------------------------------------------------------------------

/// 单探结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// 探针名。
    pub name: &'static str,
    /// 通过与否。
    pub ok: bool,
}

/// 模型探针组（六探：三维齐格/表外维拒/状态单向/封存不可改/版本升级留档/守恒）。
pub fn model_probe_group() -> Vec<Probe> {
    let mut out: Vec<Probe> = Vec::new();
    // 探一：三维齐格登记 → Empty 起步。
    let mut m = BenchModel::new();
    let k1 = CellKey { scene: 1, config: 1, metric: 0, version: 1 };
    let ok1 = m.declare(k1).is_ok()
        && match m.cell_of(&k1) {
            Some(c) => c.phase == CellPhase::Empty,
            None => false,
        };
    out.push(Probe { name: "三维齐格登记", ok: ok1 });
    // 探二：表外维拒绝（指标越闭集 / 场景越容量 / 配置越容量）。
    let bad_metric = CellKey { scene: 1, config: 1, metric: 9, version: 1 };
    let bad_scene = CellKey { scene: MAX_SCENES + 1, config: 1, metric: 0, version: 1 };
    let bad_config = CellKey { scene: 1, config: MAX_CONFIGS + 1, metric: 0, version: 1 };
    let ok2 = m.declare(bad_metric) == Err(BmCode::MetricUnknown)
        && m.declare(bad_scene) == Err(BmCode::SceneUnknown)
        && m.declare(bad_config) == Err(BmCode::ConfigUnknown);
    out.push(Probe { name: "表外维拒绝", ok: ok2 });
    // 探三：状态单向推进（Empty→Queued→Running→Scored→Sealed）。
    let ok3 = m.declare(k1) == Err(BmCode::CellDuplicate)
        && m.advance(&k1) == Ok(CellPhase::Queued)
        && m.advance(&k1) == Ok(CellPhase::Running)
        && m.record_score(&k1, 9_900, 42).is_ok()
        && m.advance(&k1) == Ok(CellPhase::Sealed);
    out.push(Probe { name: "状态单向推进", ok: ok3 });
    // 探四：封存不可改（advance/record_score 双拒）。
    let ok4 = m.advance(&k1) == Err(BmCode::SealedRewrite)
        && m.record_score(&k1, 1, 1) == Err(BmCode::SealedRewrite);
    out.push(Probe { name: "封存不可改", ok: ok4 });
    // 探五：版本升级留档（新格 Empty，旧格 Sealed 分数保留）。
    let ok5 = match m.supersede(&k1) {
        Ok(nk) => {
            let fresh_ok = match m.cell_of(&nk) {
                Some(c) => c.phase == CellPhase::Empty,
                None => false,
            };
            let old_ok = match m.cell_of(&k1) {
                Some(c) => c.phase == CellPhase::Sealed && c.score == 9_900,
                None => false,
            };
            fresh_ok && old_ok
        }
        Err(_) => false,
    };
    out.push(Probe { name: "版本升级留档", ok: ok5 });
    // 探六：守恒（两格在账、计数=遍历积分）。
    out.push(Probe { name: "模型表守恒", ok: m.conserved() && m.cell_count() == 2 });
    out
}

/// 模型报告（读屏可达）。
pub fn model_report(probes: &[Probe]) -> String {
    let mut ok_n = 0usize;
    let mut i = 0usize;
    while i < probes.len() {
        if let Some(p) = probes.get(i) {
            if p.ok {
                ok_n += 1;
            }
        }
        i += 1;
    }
    alloc::format!(
        "基准测试模型：{}探{}过（指标闭集{}项/容量{}格）",
        probes.len(),
        ok_n,
        super::vcu01_bencharch::BENCH_ITEMS.len(),
        MAX_CELLS
    )
}
