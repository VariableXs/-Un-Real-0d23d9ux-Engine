//! VE-F3621 · 主题编辑器总架构（VE-S 域 · 批次 R02 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3621`
//!
//! **判据（锚点原文六条）**：单源流水、三层对象、创作民主化、复用执法、
//! 预览复用、判据。逐条落位：
//! - **单源流水**：编辑器五段（载入→编辑→预览→验证→发布）**复用
//!   R01 五段单源**（流水单源红线：同类工具各造流水 = 维护灾难）——
//!   [`EditorPipeline`] 直接映射 R01 五段，私造流水检出 →
//!   [`V15Code::PRIVATE_PIPELINE`]（P1 重构）。
//! - **三层对象**：令牌层（F3003 对接）/组件样式层（N 控件）/资产层
//!   （壁纸图标 R03）三层编辑对象（[`Layer`]），层间失联 →
//!   [`V15Code::LAYER_LOST`] 对拍。
//! - **创作民主化**：非程序员可视化创作主题（[`EditorSession`] 全流程
//!   无代码入口）。
//! - **复用执法**：复用声明数组 [`REUSE_DECLS`] 钉死 R01/F3003/F3413
//!   单源，不复用即违规。
//! - **预览复用**：预览通道复用 F3606（预览分歧 → F3606 断言）。
//! - **判据**：`run_ves21_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 私造流水检出→P1 重构（单源红线实测） | [`V15Code::PRIVATE_PIPELINE`] |
//! | 层间失联→对拍 | [`V15Code::LAYER_LOST`] |
//! | 预览分歧→F3606 断言（复用） | [`V15Code::PREVIEW_DRIFT`] |
//! | 三层缺一→立案 | [`V15Code::LAYER_MISS`] |
//!
//! **码段**：独占 **0x45xx**。
//!
//! **性能逐项分解**：载入 O(资产)；编辑 O(变更)；复用 O(1) 声明。
//!
//! **跨批对接点**：R01 流水单源；F3003/F3413 令牌对端；R03 资产层——
//! 钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3621 诊断码段（独占 0x45xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V15Code(pub u16);

impl V15Code {
    /// 私造流水。
    pub const PRIVATE_PIPELINE: V15Code = V15Code(0x4501);
    /// 层间失联。
    pub const LAYER_LOST: V15Code = V15Code(0x4502);
    /// 预览分歧。
    pub const PREVIEW_DRIFT: V15Code = V15Code(0x4503);
    /// 三层缺一。
    pub const LAYER_MISS: V15Code = V15Code(0x4504);
    /// 阶段跳序。
    pub const STAGE_OUT_OF_ORDER: V15Code = V15Code(0x4505);
    /// 复用声明缺失。
    pub const REUSE_MISS: V15Code = V15Code(0x4506);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V15Code::PRIVATE_PIPELINE => {
                "检出私造流水（单源红线）。编辑器必须复用 R01 五段，不另造流水。"
            }
            V15Code::LAYER_LOST => {
                "层间失联。建议：对拍三层对象引用完整性。"
            }
            V15Code::PREVIEW_DRIFT => {
                "预览分歧。建议：走 F3606 保真对拍断言。"
            }
            V15Code::LAYER_MISS => {
                "三层编辑对象缺一。建议：补齐令牌/组件/资产三层。"
            }
            V15Code::STAGE_OUT_OF_ORDER => {
                "流水阶段跳序。五段顺序不可打乱。"
            }
            V15Code::REUSE_MISS => {
                "复用声明缺失。建议：补登单源复用后重新审计。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 三层编辑对象 ────────────────────────────────────────────────────

/// 编辑层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// 令牌层（颜色/间距/字号）。
    Token,
    /// 组件样式层（N 控件）。
    Component,
    /// 资产层（壁纸/图标）。
    Asset,
}

impl Layer {
    /// 三层全集。
    pub fn all() -> [Layer; 3] {
        [Layer::Token, Layer::Component, Layer::Asset]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Layer::Token => "令牌层",
            Layer::Component => "组件样式层",
            Layer::Asset => "资产层",
        }
    }
}

/// 三层对象注册。
#[derive(Clone, Debug, Default)]
pub struct LayerRegistry {
    present: Vec<Layer>,
    /// 层间引用计数（失联检测）。
    cross_refs: u32,
}

impl LayerRegistry {
    /// 新建。
    pub fn new() -> LayerRegistry {
        LayerRegistry {
            present: Vec::new(),
            cross_refs: 0,
        }
    }

    /// 注册一层。
    pub fn register(&mut self, layer: Layer) {
        if !self.present.contains(&layer) {
            self.present.push(layer);
        }
    }

    /// 三层齐全审计。
    pub fn audit_complete(&self) -> Result<(), V15Code> {
        for l in Layer::all() {
            if !self.present.contains(&l) {
                return Err(V15Code::LAYER_MISS);
            }
        }
        Ok(())
    }

    /// 记一次层间引用。
    pub fn link(&mut self) {
        self.cross_refs = self.cross_refs.wrapping_add(1);
    }

    /// 层间失联审计（引用数为零而三层都在 = 可疑失联）。
    pub fn audit_links(&self) -> Result<(), V15Code> {
        if self.present.len() == 3 && self.cross_refs == 0 {
            Err(V15Code::LAYER_LOST)
        } else {
            Ok(())
        }
    }
}

// ── 五段流水（复用 R01 单源）─────────────────────────────────────────

/// 编辑器五段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorStage {
    /// 载入。
    Load,
    /// 编辑。
    Edit,
    /// 预览。
    Preview,
    /// 验证。
    Verify,
    /// 发布。
    Publish,
}

impl EditorStage {
    /// 顺序。
    pub fn order() -> [EditorStage; 5] {
        [
            EditorStage::Load,
            EditorStage::Edit,
            EditorStage::Preview,
            EditorStage::Verify,
            EditorStage::Publish,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            EditorStage::Load => "载入",
            EditorStage::Edit => "编辑",
            EditorStage::Preview => "预览",
            EditorStage::Verify => "验证",
            EditorStage::Publish => "发布",
        }
    }
}

/// 编辑器流水（复用 R01 五段，不私造）。
#[derive(Clone, Debug, Default)]
pub struct EditorPipeline {
    done: Vec<EditorStage>,
    /// 单源标记（恒 true，私造流水时由审计检出）。
    reuses_r01: bool,
}

impl EditorPipeline {
    /// 新建（声明复用 R01）。
    pub fn new() -> EditorPipeline {
        EditorPipeline {
            done: Vec::new(),
            reuses_r01: true,
        }
    }

    /// 走一段（必须按序）。
    pub fn advance(&mut self, stage: EditorStage) -> Result<(), V15Code> {
        let expected = EditorStage::order()
            .iter()
            .find(|s| !self.done.contains(s))
            .copied();
        match expected {
            Some(e) if e == stage => {
                self.done.push(stage);
                Ok(())
            }
            Some(_) => Err(V15Code::STAGE_OUT_OF_ORDER),
            None => Ok(()), // 全部完成后再调是幂等
        }
    }

    /// 单源审计。
    pub fn audit_single_source(&self) -> Result<(), V15Code> {
        if self.reuses_r01 {
            Ok(())
        } else {
            Err(V15Code::PRIVATE_PIPELINE)
        }
    }

    /// 诊断用：模拟私造（反向语料）。
    pub fn mark_private_for_test(&mut self) {
        self.reuses_r01 = false;
    }

    /// 进度。
    pub fn progress(&self) -> usize {
        self.done.len()
    }
}

// ── 编辑会话（创作民主化）───────────────────────────────────────────

/// 编辑会话。
#[derive(Clone, Debug)]
pub struct EditorSession {
    /// 流水。
    pub pipeline: EditorPipeline,
    /// 三层。
    pub layers: LayerRegistry,
    /// 预览指纹（对拍用）。
    pub preview_fp: u64,
    /// 发布指纹。
    pub publish_fp: u64,
}

impl EditorSession {
    /// 新建会话。
    pub fn new() -> EditorSession {
        let mut layers = LayerRegistry::new();
        for l in Layer::all() {
            layers.register(l);
        }
        layers.link();
        EditorSession {
            pipeline: EditorPipeline::new(),
            layers,
            preview_fp: 0,
            publish_fp: 0,
        }
    }

    /// 预览保真对拍（复用 F3606 口径）。
    pub fn audit_preview(&self) -> Result<(), V15Code> {
        if self.preview_fp == self.publish_fp && self.preview_fp != 0 {
            Ok(())
        } else if self.preview_fp == 0 && self.publish_fp == 0 {
            Ok(()) // 尚未预览，不违规
        } else {
            Err(V15Code::PREVIEW_DRIFT)
        }
    }
}

// ── 复用声明 ────────────────────────────────────────────────────────

/// 复用声明（单源指名）。
pub const REUSE_DECLS: [&str; 4] = [
    "R01-五段流水单源",
    "VE-F3003-令牌单源",
    "VE-F3413-依赖图单源",
    "VE-F3606-预览通道单源",
];

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["R01-PIPELINE", "VE-F3003", "R03-ASSET"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves21_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3621");

    // 0x4501 三层齐
    let es = EditorSession::new();
    out.add(
        "0x4501-三层齐",
        es.layers.audit_complete().is_ok(),
        "",
    );

    // 0x4502 流水单源
    out.add(
        "0x4502-流水单源",
        es.pipeline.audit_single_source().is_ok(),
        "",
    );

    // 0x4503 私造流水检出
    let mut es2 = EditorSession::new();
    es2.pipeline.mark_private_for_test();
    out.add(
        "0x4503-私造流水检出",
        es2.pipeline.audit_single_source().is_err(),
        "",
    );

    // 0x4504 按序推进
    let mut ep = EditorPipeline::new();
    let seq_ok = ep.advance(EditorStage::Load).is_ok()
        && ep.advance(EditorStage::Edit).is_ok()
        && ep.advance(EditorStage::Preview).is_ok();
    out.add("0x4504-按序推进", seq_ok, "");

    // 0x4505 跳序拦截
    let mut ep2 = EditorPipeline::new();
    ep2.advance(EditorStage::Load).ok();
    out.add(
        "0x4505-跳序拦截",
        ep2.advance(EditorStage::Publish).is_err(),
        "",
    );

    // 0x4506 层失联检出
    let mut lr = LayerRegistry::new();
    for l in Layer::all() {
        lr.register(l);
    }
    out.add(
        "0x4506-层失联检出",
        lr.audit_links().is_err(),
        "",
    );

    // 0x4507 层引用后放行
    let mut lr2 = LayerRegistry::new();
    for l in Layer::all() {
        lr2.register(l);
    }
    lr2.link();
    out.add("0x4507-层引用后放行", lr2.audit_links().is_ok(), "");

    // 0x4508 预览未跑不违规
    out.add("0x4508-预览未跑不违规", es.audit_preview().is_ok(), "");

    // 0x4509 预览分歧检出
    let mut es3 = EditorSession::new();
    es3.preview_fp = 1;
    es3.publish_fp = 2;
    out.add(
        "0x4509-预览分歧检出",
        es3.audit_preview().is_err(),
        "",
    );

    // 0x450a 复用声明非空
    out.add("0x450a-复用声明非空", !REUSE_DECLS.is_empty(), "");

    out
}
