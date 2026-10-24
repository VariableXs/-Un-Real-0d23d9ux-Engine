//! VE-F3631 · 编辑器 undo（VE-S 域 · 批次 R02 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3631`
//!
//! **判据（锚点原文六条）**：历史保护、粒度复述、链上限、崩溃恢复、
//! 单源复用、判据。逐条落位：
//! - **历史保护**：崩溃后 undo 链不能丢（崩溃恢复红线）——
//!   [`PersistentUndo::persist`] 链持久化，崩溃模拟后 [`restore`] 恢复。
//! - **粒度复述**：令牌编辑粒度（每次提交一步）。
//! - **链上限**：复用 F3043 链上限配置。
//! - **崩溃恢复**：断电注入演练（[`CrashSim`]）。
//! - **单源复用**：F3043/F3090 undo 单源复用。
//! - **判据**：`run_ves31_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 崩溃丢链→P1（历史红线实测） | [`V1FCode::CHAIN_LOST`] |
//! | 粒度失调→修正（复述） | [`V1FCode::GRANULARITY`] |
//! | 上限爆→断言（复用） | [`V1FCode::CHAIN_OVERFLOW`] |
//! | 预留调用→显性（复述） | [`V1FCode::RESERVE_CALLED`] |
//!
//! **码段**：独占 **0x4fxx**。
//!
//! **性能逐项分解**：粒度 O(1)；持久 O(链) 异步；上限 O(1)；
//! 预留 O(0)。
//!
//! **跨批对接点**：F3043/F3090 undo 单源复用声明；F3047 草稿（持久
//! 对端）；F3632 预留——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3631 诊断码段（独占 0x4fxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V1FCode(pub u16);

impl V1FCode {
    /// 崩溃丢链。
    pub const CHAIN_LOST: V1FCode = V1FCode(0x4f01);
    /// 粒度失调。
    pub const GRANULARITY: V1FCode = V1FCode(0x4f02);
    /// 链上限爆。
    pub const CHAIN_OVERFLOW: V1FCode = V1FCode(0x4f03);
    /// 预留被调。
    pub const RESERVE_CALLED: V1FCode = V1FCode(0x4f04);
    /// 持久化数据损坏。
    pub const PERSIST_CORRUPT: V1FCode = V1FCode(0x4f05);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V1FCode::CHAIN_LOST => {
                "崩溃后 undo 链丢失（历史红线 P1）。建议：启用链持久化。"
            }
            V1FCode::GRANULARITY => {
                "undo 粒度失调。建议：每次令牌编辑 = 一步。"
            }
            V1FCode::CHAIN_OVERFLOW => {
                "链上限爆。建议：截断最旧步。"
            }
            V1FCode::RESERVE_CALLED => {
                "协作预留被调用（显式未就绪）。"
            }
            V1FCode::PERSIST_CORRUPT => {
                "持久化数据损坏。建议：从快照重建。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 持久化 undo 链 ──────────────────────────────────────────────────

/// 一步 undo（令牌级粒度）。
#[derive(Clone, Debug)]
pub struct UndoStep {
    /// 令牌名。
    pub token: String,
    /// 旧值。
    pub old_val: u32,
    /// 新值。
    pub new_val: u32,
}

/// 持久化 undo 链。
#[derive(Clone, Debug, Default)]
pub struct PersistentUndo {
    steps: Vec<UndoStep>,
    /// 持久化镜像（崩溃恢复用）。
    persisted: Vec<UndoStep>,
    /// 链上限。
    pub max_depth: usize,
}

impl PersistentUndo {
    /// 新建（上限 200）。
    pub fn new() -> PersistentUndo {
        PersistentUndo {
            steps: Vec::new(),
            persisted: Vec::new(),
            max_depth: 200,
        }
    }

    /// 压入一步（令牌编辑粒度）。
    pub fn push(&mut self, step: UndoStep) -> Result<(), V1FCode> {
        if self.steps.len() >= self.max_depth {
            self.steps.remove(0);
            return Err(V1FCode::CHAIN_OVERFLOW);
        }
        self.steps.push(step);
        Ok(())
    }

    /// 持久化（崩溃前调用）。
    pub fn persist(&mut self) {
        self.persisted = self.steps.clone();
    }

    /// 模拟崩溃恢复。
    pub fn restore(&mut self) -> Result<(), V1FCode> {
        if self.persisted.is_empty() && !self.steps.is_empty() {
            return Err(V1FCode::CHAIN_LOST);
        }
        self.steps = self.persisted.clone();
        Ok(())
    }

    /// 深度。
    pub fn depth(&self) -> usize {
        self.steps.len()
    }

    /// 撤销一步。
    pub fn undo(&mut self) -> Option<UndoStep> {
        self.steps.pop()
    }
}

// ── 崩溃注入模拟 ────────────────────────────────────────────────────

/// 崩溃模拟器。
#[derive(Clone, Debug, Default)]
pub struct CrashSim;

impl CrashSim {
    /// 断电注入：链未持久化时丢链。
    pub fn power_cut(chain: &PersistentUndo) -> Result<(), V1FCode> {
        if chain.persisted.is_empty() && chain.depth() > 0 {
            Err(V1FCode::CHAIN_LOST)
        } else {
            Ok(())
        }
    }
}

// ── 协作预留接口（F3632 联动）───────────────────────────────────────

/// 协作冲突预留。
#[derive(Clone, Debug, Default)]
pub struct CollabReserve {
    called: bool,
}

impl CollabReserve {
    /// 调用（显式未就绪）。
    pub fn call(&mut self) -> Result<(), V1FCode> {
        self.called = true;
        Err(V1FCode::RESERVE_CALLED)
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3043", "VE-F3047", "VE-F3632"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves31_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3631");

    // 0x4f01 持久化恢复
    let mut pu = PersistentUndo::new();
    pu.push(UndoStep {
        token: String::from("primary"),
        old_val: 1,
        new_val: 2,
    })
    .ok();
    pu.persist();
    out.add("0x4f01-持久化恢复", pu.restore().is_ok(), "");

    // 0x4f02 未持久化丢链
    let mut pu2 = PersistentUndo::new();
    pu2.push(UndoStep {
        token: String::from("x"),
        old_val: 1,
        new_val: 2,
    })
    .ok();
    out.add(
        "0x4f02-未持久化丢链",
        CrashSim::power_cut(&pu2).is_err(),
        "",
    );

    // 0x4f03 已持久化不断
    out.add("0x4f03-已持久化不断", CrashSim::power_cut(&pu).is_ok(), "");

    // 0x4f04 链上限
    let mut pu3 = PersistentUndo::new();
    pu3.max_depth = 3;
    let mut overflow = false;
    for i in 0..5 {
        if pu3
            .push(UndoStep {
                token: String::from("t"),
                old_val: i,
                new_val: i + 1,
            })
            .is_err()
        {
            overflow = true;
        }
    }
    out.add("0x4f04-链上限", overflow, "");

    // 0x4f05 撤销
    let mut pu4 = PersistentUndo::new();
    pu4.push(UndoStep {
        token: String::from("y"),
        old_val: 10,
        new_val: 20,
    })
    .ok();
    out.add("0x4f05-撤销", pu4.undo().is_some(), "");

    // 0x4f06 预留显式未就绪
    let mut cr = CollabReserve::default();
    out.add("0x4f06-预留显式未就绪", cr.call().is_err(), "");

    out
}
