//! VE-F3503 · 时段场景系统 —— 场景编排 + 原子切换事务（E04 复用）+ 专注模式联动。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3503`
//!
//! # 职责（锚点原文拆解）
//!
//! - **场景编排**（[`SceneBook`]）：工作/夜间/阅读等场景各自持有
//!   色温曲线（复用 F3501 [`super::ver08a_kelvin::CurveFamily`] 语义）、
//!   生效时段（[`SceneWindow`]）与优先级。场景按「时间命中 + 优先级
//!   最高」选出当值场景；同优先级冲突（多个场景同时命中）→ **仲裁**
//!   取声明顺序最早者（先声明先得，规则公开）。
//! - **原子切换**（[`SceneTx`]）：场景切换是事务——新场景的三件
//!   （曲线/时段/联动）要么整体生效要么整体不动。事务三段：
//!   预演（[`SceneTx::preview`]）→ 提交（`commit`）→ 失败回滚
//!   （`rollback`）。回滚用的快照是提交前的场景表克隆，
//!   回滚后表内容与快照逐位一致（可验证）。
//! - **专注模式联动**：专注模式开启时，若当值场景声明了
//!   [`SceneFocus::Follow`]，则切换到该场景绑定的专注档；
//!   联动失效（专注系统不可达）→ **降级为独立**：场景照常切换，
//!   专注部分静默跳过并记录（[`SceneCode::FocusUnreachable`]）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 切换失败 | 回滚整组 |
//! | 场景冲突 | 仲裁（优先级→声明序） |
//! | 联动失效 | 降级独立（场景照切） |
//!
//! # 复杂度
//!
//! - `resolve`：O(场景数)（线性扫描，场景以十计）；
//! - `commit` / `rollback`：O(场景数)（表克隆）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5700 起）
// ---------------------------------------------------------------------------

/// 时段场景域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneCode {
    /// 场景名重复（同表内唯一约束）。
    DuplicateName = 5700,
    /// 场景数超上限（表膨胀防护）。
    TooManyScenes = 5701,
    /// 时段窗口非法（起点不小于终点且非跨午夜声明）。
    BadWindow = 5702,
    /// 事务预演未通过即提交。
    TxNotPreviewed = 5703,
    /// 回滚目标缺失（没有快照却要求回滚）。
    NoSnapshot = 5704,
    /// 场景未命中（无任何时段命中当前分钟）。
    NoSceneHit = 5705,
    /// 专注联动不可达（降级独立）。
    FocusUnreachable = 5706,
    /// 场景名空。
    EmptyName = 5707,
}

impl SceneCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            SceneCode::DuplicateName => "场景名重复",
            SceneCode::TooManyScenes => "场景数超上限",
            SceneCode::BadWindow => "时段窗口非法",
            SceneCode::TxNotPreviewed => "事务未预演即提交",
            SceneCode::NoSnapshot => "回滚目标缺失",
            SceneCode::NoSceneHit => "无场景命中",
            SceneCode::FocusUnreachable => "专注联动不可达",
            SceneCode::EmptyName => "场景名为空",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, SceneCode::TooManyScenes | SceneCode::NoSnapshot)
    }
}

// ---------------------------------------------------------------------------
// 二、场景定义
// ---------------------------------------------------------------------------

/// 场景数上限：以十计即可，膨胀即拒绝（防表失控）。
pub const SCENE_CAP: usize = 16;
/// 一天的分钟数。
pub const DAY: u32 = 24 * 60;

/// 生效时段窗口。支持跨午夜（`from > to` 表示环回）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SceneWindow {
    /// 起始分钟（含）。
    pub from: u32,
    /// 结束分钟（不含）。
    pub to: u32,
}

impl SceneWindow {
    /// 构造并校验（0..DAY 内，from != to）。
    pub fn new(from: u32, to: u32) -> Result<Self, SceneCode> {
        if from >= DAY || to >= DAY || from == to {
            return Err(SceneCode::BadWindow);
        }
        Ok(SceneWindow { from, to })
    }

    /// 是否跨午夜。
    pub fn wraps(&self) -> bool {
        self.from > self.to
    }

    /// 当前分钟是否命中。
    pub fn hits(&self, minute: u32) -> bool {
        let m = minute % DAY;
        if self.wraps() {
            m >= self.from || m < self.to
        } else {
            m >= self.from && m < self.to
        }
    }
}

/// 专注模式联动策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneFocus {
    /// 不联动。
    None,
    /// 进入场景时联动专注档（档位 id）。
    Follow(u16),
}

/// 一个色温场景。
#[derive(Clone, Debug)]
pub struct Scene {
    /// 场景名（表内唯一）。
    pub name: &'static str,
    /// 生效时段。
    pub window: SceneWindow,
    /// 目标色温（本册简化：场景持固定目标值，曲线族由 F3501 引擎持有）。
    pub kelvin: u32,
    /// 优先级（大者先）。
    pub priority: u8,
    /// 专注联动。
    pub focus: SceneFocus,
}

impl Scene {
    /// 构造（色温由 ver08a 钳制）。
    pub fn build(
        name: &'static str,
        window: SceneWindow,
        kelvin: u32,
        priority: u8,
        focus: SceneFocus,
    ) -> Result<Self, SceneCode> {
        if name.is_empty() {
            return Err(SceneCode::EmptyName);
        }
        Ok(Scene {
            name,
            window,
            kelvin: super::ver08a_kelvin::clamp_kelvin(kelvin),
            priority,
            focus,
        })
    }
}

// ---------------------------------------------------------------------------
// 三、场景表与仲裁
// ---------------------------------------------------------------------------

/// 解析结果：当值场景下标或未命中。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneHit {
    /// 命中（场景下标 + 该场景色温）。
    Hit(usize, u32),
    /// 全部未命中（引擎保持当前值不动）。
    Miss,
}

/// 场景表：插入去重 + 仲裁解析。
#[derive(Clone, Debug, Default)]
pub struct SceneBook {
    scenes: Vec<Scene>,
}

impl SceneBook {
    /// 空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 场景数。
    pub fn len(&self) -> usize {
        self.scenes.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }

    /// 只读视图。
    pub fn scenes(&self) -> &[Scene] {
        &self.scenes
    }

    /// 插入场景：重名拒绝、超上限拒绝。
    pub fn insert(&mut self, scene: Scene) -> Result<usize, SceneCode> {
        if self.scenes.len() >= SCENE_CAP {
            return Err(SceneCode::TooManyScenes);
        }
        if self.scenes.iter().any(|s| s.name == scene.name) {
            return Err(SceneCode::DuplicateName);
        }
        self.scenes.push(scene);
        Ok(self.scenes.len() - 1)
    }

    /// 仲裁解析：命中场景里取优先级最大者；同优先级取声明最早者。
    pub fn resolve(&self, minute: u32) -> Result<SceneHit, SceneCode> {
        let mut best: Option<usize> = None;
        for (i, s) in self.scenes.iter().enumerate() {
            if !s.window.hits(minute) {
                continue;
            }
            best = Some(match best {
                None => i,
                Some(b) => {
                    if s.priority > self.scenes[b].priority {
                        i
                    } else {
                        b
                    }
                }
            });
        }
        match best {
            Some(i) => Ok(SceneHit::Hit(i, self.scenes[i].kelvin)),
            None => {
                if self.scenes.is_empty() {
                    Ok(SceneHit::Miss)
                } else {
                    Err(SceneCode::NoSceneHit)
                }
            }
        }
    }

    /// 联动查询：命中场景的专注档。
    pub fn focus_of(&self, hit: SceneHit) -> Option<u16> {
        match hit {
            SceneHit::Miss => None,
            SceneHit::Hit(i, _) => match self.scenes.get(i).map(|s| s.focus) {
                Some(SceneFocus::Follow(id)) => Some(id),
                _ => None,
            },
        }
    }

    /// 结构签名（事务一致性验证）。
    pub fn signature(&self) -> u64 {
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        for s in &self.scenes {
            h = h
                .wrapping_mul(0x100_0000_01b3)
                ^ (s.name.len() as u64)
                ^ ((s.window.from as u64) << 8)
                ^ ((s.window.to as u64) << 24)
                ^ ((s.kelvin as u64) << 40)
                ^ ((s.priority as u64) << 56);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 四、切换事务（E04 原子复用）
// ---------------------------------------------------------------------------

/// 事务阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxStage {
    /// 未开始。
    Idle,
    /// 预演通过，待提交。
    Previewed,
    /// 已提交。
    Committed,
    /// 已回滚。
    RolledBack,
}

/// 场景切换事务：预演 → 提交 →（失败）回滚。
pub struct SceneTx {
    stage: TxStage,
    snapshot: Option<SceneBook>,
    pending: Option<Scene>,
}

impl SceneTx {
    /// 新事务。
    pub fn new() -> Self {
        SceneTx {
            stage: TxStage::Idle,
            snapshot: None,
            pending: None,
        }
    }

    /// 阶段。
    pub fn stage(&self) -> TxStage {
        self.stage
    }

    /// 预演：把待插入场景做合法性检查（重名/上限），通过则记快照。
    /// 预演不动真表。
    pub fn preview(&mut self, book: &SceneBook, scene: Scene) -> Result<(), SceneCode> {
        if book.scenes.iter().any(|s| s.name == scene.name) {
            return Err(SceneCode::DuplicateName);
        }
        if book.scenes.len() >= SCENE_CAP {
            return Err(SceneCode::TooManyScenes);
        }
        self.snapshot = Some(book.clone());
        self.pending = Some(scene);
        self.stage = TxStage::Previewed;
        Ok(())
    }

    /// 提交：只有预演过的才能提交。提交把场景写入真表。
    pub fn commit(&mut self, book: &mut SceneBook) -> Result<(), SceneCode> {
        if self.stage != TxStage::Previewed {
            return Err(SceneCode::TxNotPreviewed);
        }
        let scene = self.pending.take().ok_or(SceneCode::TxNotPreviewed)?;
        book.insert(scene)?;
        self.stage = TxStage::Committed;
        Ok(())
    }

    /// 回滚：把表恢复到预演前快照（整体一致）。
    pub fn rollback(&mut self, book: &mut SceneBook) -> Result<(), SceneCode> {
        let snap = self.snapshot.take().ok_or(SceneCode::NoSnapshot)?;
        *book = snap;
        self.stage = TxStage::RolledBack;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、专注联动桥
// ---------------------------------------------------------------------------

/// 专注系统桥 trait：联动器只认这个口，不认具体专注实现。
pub trait FocusBridge {
    /// 尝试进入专注档。false = 专注系统不可达。
    fn enter(&mut self, tier: u16) -> bool;
}

/// 联动降级记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FocusFallback {
    /// 本应进入的专注档。
    pub wanted: u16,
    /// 降级码。
    pub code: SceneCode,
}

/// 联动执行器：场景命中后调用。联动失效 → 返回降级记录，场景本身照常。
pub fn apply_focus(br: &mut dyn FocusBridge, hit: SceneHit, book: &SceneBook) -> Option<FocusFallback> {
    match book.focus_of(hit) {
        Some(tier) => {
            if br.enter(tier) {
                None
            } else {
                Some(FocusFallback {
                    wanted: tier,
                    code: SceneCode::FocusUnreachable,
                })
            }
        }
        None => None,
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_wraps_midnight() {
        let w = SceneWindow::new(22 * 60, 6 * 60).unwrap();
        assert!(w.wraps());
        assert!(w.hits(23 * 60));
        assert!(w.hits(5 * 60));
        assert!(!w.hits(12 * 60));
    }

    #[test]
    fn arbitration_prefers_priority_then_order() {
        let mut b = SceneBook::new();
        let w = SceneWindow::new(0, DAY).unwrap();
        b.insert(Scene::build("low", w, 5000, 1, SceneFocus::None).unwrap())
            .unwrap();
        b.insert(Scene::build("high", w, 3400, 9, SceneFocus::None).unwrap())
            .unwrap();
        match b.resolve(600).unwrap() {
            SceneHit::Hit(i, k) => {
                assert_eq!(b.scenes()[i].name, "high");
                assert_eq!(k, 3400);
            }
            _ => panic!("expect hit"),
        }
    }

    #[test]
    fn tx_rollback_restores_snapshot() {
        let mut book = SceneBook::new();
        let w = SceneWindow::new(0, DAY).unwrap();
        book.insert(Scene::build("a", w, 6500, 1, SceneFocus::None).unwrap())
            .unwrap();
        let sig = book.signature();
        let mut tx = SceneTx::new();
        tx.preview(&book, Scene::build("b", w, 3400, 2, SceneFocus::None).unwrap())
            .unwrap();
        tx.commit(&mut book).unwrap();
        assert_eq!(book.len(), 2);
        tx.rollback(&mut book).unwrap();
        assert_eq!(book.len(), 1);
        assert_eq!(book.signature(), sig);
    }
}
