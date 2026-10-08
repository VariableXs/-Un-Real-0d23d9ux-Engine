//! VE-F0801 · 文字渲染域总架构（VE-E 域 · 文字渲染引擎 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0801`
//!
//! **判据（锚点原文五条）**：补写显性、四段架构、三向兑现、复用声明、1.5ms 预算。
//! 逐条落位：
//! - **补写显性**：[`SUPPLEMENT_MANIFEST`] + [`TextPipeline::signoff_s117`]——
//!   S117 审计发现（VE-E 的 F0801-F1000 区段因字母占用冲突从未落盘）被显性签收，
//!   签收动作进审计账；未签收不许开工（`E_SUPPLEMENT_NOT_SIGNED`）。
//! - **四段架构**：[`SEG_GLYPH`]/[`SEG_SHAPING`]/[`SEG_FONT`]/[`SEG_RENDER`] 四段
//!   以单向数据流衔接（[`FlowEdge`]），段间接口冻结（[`InterfaceSpec::frozen`]）。
//! - **三向兑现**：[`Commitment`] 三轴（字形向=光栅化质量 / 排版向=多语言正确 /
//!   渲染向=性能预算），每轴的判据逐条挂证据，全绿才 [`TextPipeline::domain_ready`]。
//! - **复用声明**：[`ReuseDeclaration`]——开工样板复用 X01 开工范式，六件套体例同构。
//! - **1.5ms 预算**：[`FRAME_BUDGET_US`] = 1500µs，六段分解见 [`BUDGET_SLICES`]，
//!   三水位（绿/黄/红）与降级序见 [`WATER_GREEN_MAX_US`] / [`degradation_plan`]。
//!
//! **错误路径与降级矩阵**（零静默，全进 [`TextPipeline::errors`]）：
//! - 段间接口未冻结 → 禁止开工下游（`E_UPSTREAM_NOT_FROZEN`）；
//! - 绕过上游直喂（数据流跳段）→ 拒绝并记账（`E_FLOW_BYPASS`）；
//! - 逆流（下游喂上游）→ 拒绝（`E_FLOW_BACKWARD`）；
//! - 已冻结接口被改 → 必须先有已接受 ADR（`E_ADR_REQUIRED`）；
//! - 帧文字开销超预算 → 记账 + 出降级序建议（`E_BUDGET_BREACH`），不静默截断；
//! - 降级序触碰无障碍路径 → 断言拦截（`E_A11Y_PROTECTED`）。
//!
//! **数据流 vs 供给边（架构显式决策）**：四段按锚点列序为 0→1→2→3 严格单向
//! *数据流*（[`FlowEdge`]，只容许紧邻上游，跳段与逆流皆违规）；字体管理对字形段
//! 的表供给是*供给边*（[`SupplyEdge`]，容许跨段但方向受限且必须显式声明）——
//! 两者混为一谈会让「单向」判据形同虚设，故分型建模并各自立判据。
//!
//! **跨批对接点**：Eb01 组内 F0802-F0820（解码/轮廓/光栅/Hinting/图集/图元/度量/
//! 遥测/安全/文档/测试/示例/fuzz/性能/扩展/API/N 域契约/联调/收口）；Eb02 字体管理
//! 组 F0821-F0840；Eb09 域自查与 AE10 F6396 三向预告呼应。
//!
//! 逻辑 tick 注入、零墙钟；零 IO；只用 `alloc` 容器（无 HashMap——内核无 hasher
//! 依赖，线性扫描并诚实标注复杂度）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 四段架构的段数。
pub const STAGE_COUNT: usize = 4;

/// 段 0：编码字形（解码 → 字形解析 → 轮廓 → 光栅化）。
pub const SEG_GLYPH: usize = 0;

/// 段 1：排版 Shaping（整形 / 双向 /换行）。
pub const SEG_SHAPING: usize = 1;

/// 段 2：字体管理（解析 / 回退 / 子集化）。
pub const SEG_FONT: usize = 2;

/// 段 3：渲染输出（图集 / 图元 / 管线）。
pub const SEG_RENDER: usize = 3;

/// 段名（索引即段号）。
pub const STAGE_NAMES: [&str; STAGE_COUNT] = ["编码字形", "排版Shaping", "字体管理", "渲染输出"];

/// 每帧文字渲染架构级预算：**1.5ms**（P07 预算范式）。
pub const FRAME_BUDGET_US: u32 = 1500;

/// 预算六段分解（µs）——解码 50 / 光栅 500 / 整形 300 / 图元 400 / 遥测 20 / 余量 230。
pub const BUDGET_SLICES: [(&str, u32); 6] = [
    ("解码", 50),
    ("光栅化", 500),
    ("整形", 300),
    ("图元渲染", 400),
    ("遥测", 20),
    ("余量", 230),
];

/// 绿水位上限（余量 ≥80% ⇒ 已用 ≤20% 预算 = 300µs）。
pub const WATER_GREEN_MAX_US: u32 = FRAME_BUDGET_US / 5;

/// 黄水位上限（余量 50%~80% ⇒ 已用 300~750µs，即预算的一半）。
pub const WATER_YELLOW_MAX_US: u32 = FRAME_BUDGET_US / 2;

/// 遥测自身预算（µs/帧，锚点 F0809 口径 ≤0.02ms）。
pub const TELEMETRY_SLICE_US: u32 = BUDGET_SLICES[4].1;

/// 补写显性：S117 审计发现的区段与补写批次登记。
pub const SUPPLEMENT_MANIFEST: &str = "\
补写显性登记（VE-F0801）：S117 审计发现 VE-E 区段 F0801-F1000 因字母占用冲突\
从未落盘；S117 审计显性立项，S129 起以 Eb01-Eb10 补写（每批 20 项）。\
本模块为 Eb01 批首项，补写签收动作入审计账——未签收不许开工。";

/// 复用声明：开工样板复用 X01 开工范式。
pub const X01_REUSE_DOC: &str = "\
复用声明（VE-F0801）：本域开工样板复用 X01 开工范式，六件套体例与 X01 同构——\
定位 / 架构 / 契约 / 预算 / 风险 / 里程碑，缺一不予开工。\
同构不同参：X01 的段间数据为几何图元，本域为字形与排版簇。";

/// 单向数据流契约文档。
pub const UNIDIRECTIONAL_FLOW_DOC: &str = "\
单向数据流契约（VE-F0801 · v1）：四段按 编码字形 → 排版Shaping → 字体管理 → 渲染输出\
严格单向衔接，数据流只容许紧邻上游供给下游；跳段（E_FLOW_BYPASS）与逆流\
（E_FLOW_BACKWARD）一律拒绝并记账。字体管理对字形段的表供给属供给边（SupplyEdge），\
方向受限且必须显式声明——供给边不是数据流，不得借供给边开旁路。";

/// 域开工六件套的槽位名（顺序即X01 体例）。
pub const OPEN_PACK_SLOTS: [&str; 6] = ["定位", "架构", "契约", "预算", "风险", "里程碑"];

// ---------------------------------------------------------------------------
// 二、数据结构
// ---------------------------------------------------------------------------

/// 段间接口规格（冻结对象——冻结是本域开工的硬闸）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceSpec {
    /// 接口名（如 `GlyphSink`）。
    pub name: String,
    /// 契约版本（十年不变承诺的载体，追加字段才允许升版）。
    pub version: u32,
    /// 接口字段清单（顺序即序列化序）。
    pub fields: Vec<String>,
    /// 冻结时刻的逻辑 tick；`None` = 未冻结。
    pub frozen_at: Option<u64>,
    /// 冻结时登记的 ADR 号（首次冻结为 `None`，改接口必填）。
    pub adr: Option<u64>,
}

impl InterfaceSpec {
    /// 新建未冻结接口。
    pub fn new(name: &str, version: u32, fields: &[&str]) -> Self {
        InterfaceSpec {
            name: name.to_string(),
            version,
            fields: fields.iter().map(|f| f.to_string()).collect(),
            frozen_at: None,
            adr: None,
        }
    }

    /// 是否已冻结。
    pub fn frozen(&self) -> bool {
        self.frozen_at.is_some()
    }

    /// 契约指纹（字段名 + 版本的有序拼接）——改字段必改指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h ^= self.version as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
        for f in &self.fields {
            for b in f.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
            h ^= 0x2c;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }
}

/// 数据流边（严格单向、只容许紧邻上游）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowEdge {
    /// 供给段。
    pub from: usize,
    /// 消费段。
    pub to: usize,
}

/// 供给边（跨段但方向受限，必须显式声明；不是数据流）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupplyEdge {
    /// 供给段。
    pub from: usize,
    /// 接受段。
    pub to: usize,
    /// 供给内容（如 `FontTables`）。
    pub payload: String,
}

/// 段开工状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageState {
    /// 未开工（段间接口未冻结 / 前置未就位）。
    NotStarted,
    /// 已开工。
    Open,
}

/// 三向兑现的轴。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// 字形向：光栅化质量。
    Glyph,
    /// 排版向：多语言正确。
    Layout,
    /// 渲染向：性能预算。
    Render,
}

impl Axis {
    /// 轴的稳定序号（记账与台账用）。
    pub fn index(self) -> usize {
        match self {
            Axis::Glyph => 0,
            Axis::Layout => 1,
            Axis::Render => 2,
        }
    }

    /// 轴名。
    pub fn name(self) -> &'static str {
        match self {
            Axis::Glyph => "字形向",
            Axis::Layout => "排版向",
            Axis::Render => "渲染向",
        }
    }
}

/// 一条判据（承诺的最小可核验单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Criterion {
    /// 判据文本。
    pub text: String,
    /// 挂上的证据（空 = 未兑现；证据非空且校验通过才算兑现）。
    pub evidence: Vec<String>,
}

/// 一向承诺（轴 + 承诺 + 判据集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commitment {
    /// 所属轴。
    pub axis: Axis,
    /// 承诺文本。
    pub promise: String,
    /// 判据集（逐条挂证据）。
    pub criteria: Vec<Criterion>,
}

impl Commitment {
    /// 本轴是否全部兑现（每条判据都有证据）。
    pub fn fulfilled(&self) -> bool {
        !self.criteria.is_empty() && self.criteria.iter().all(|c| !c.evidence.is_empty())
    }

    /// 未兑现的判据数。
    pub fn pending(&self) -> usize {
        self.criteria.iter().filter(|c| c.evidence.is_empty()).count()
    }
}

/// 开工六件套槽（X01 体例同构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenSlot {
    /// 槽名。
    pub name: String,
    /// 正文（空 = 未填）。
    pub body: String,
}

/// 六件套载体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPack {
    slots: Vec<OpenSlot>,
}

impl OpenPack {
    /// 新建空六件套。
    pub fn new() -> Self {
        OpenPack {
            slots: OPEN_PACK_SLOTS
                .iter()
                .map(|n| OpenSlot { name: n.to_string(), body: String::new() })
                .collect(),
        }
    }

    /// 填槽（槽名不存在 → 返回 false，不静默新建）。
    pub fn fill(&mut self, slot: &str, body: &str) -> bool {
        match self.slots.iter_mut().find(|s| s.name == slot) {
            Some(s) => {
                s.body = body.to_string();
                true
            }
            None => false,
        }
    }

    /// 未填槽名清单。
    pub fn missing(&self) -> Vec<String> {
        self.slots
            .iter()
            .filter(|s| s.body.is_empty())
            .map(|s| s.name.clone())
            .collect()
    }

    /// 六件套是否齐备。
    pub fn complete(&self) -> bool {
        self.missing().is_empty()
    }
}

impl Default for OpenPack {
    fn default() -> Self {
        Self::new()
    }
}

/// ADR（架构变更记录）状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdrState {
    /// 提案中（未生效——不能据此改冻结接口）。
    Proposed,
    /// 已接受（可据此改冻结接口）。
    Accepted,
    /// 已否决。
    Rejected,
}

/// ADR 记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adr {
    /// 编号（单调递增，1 起）。
    pub id: u64,
    /// 标题。
    pub title: String,
    /// 理由（为什么必须改）。
    pub reason: String,
    /// 影响的段。
    pub stages: Vec<usize>,
    /// 状态。
    pub state: AdrState,
}

/// 水位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Water {
    /// 绿：余量 ≥80%。
    Green,
    /// 黄：余量 50%~80%。
    Yellow,
    /// 红：余量 <50%（即降级触发）。
    Red,
}

impl Water {
    /// 判水位（入参为已用µs）。
    pub fn judge(used_us: u32) -> Water {
        if used_us <= WATER_GREEN_MAX_US {
            Water::Green
        } else if used_us <= WATER_YELLOW_MAX_US {
            Water::Yellow
        } else {
            Water::Red
        }
    }

    /// 水位名。
    pub fn name(self) -> &'static str {
        match self {
            Water::Green => "绿",
            Water::Yellow => "黄",
            Water::Red => "红",
        }
    }
}

/// 降级一步。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeStep {
    /// 序位（0 = 先做）。
    pub order: usize,
    /// 步骤名。
    pub name: String,
    /// 是否触碰无障碍路径（放大 / 高对比）——红线保护序恒为 false。
    pub touches_a11y: bool,
}

/// 复用声明体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReuseDeclaration {
    /// 复用的样板名。
    pub paradigm: String,
    /// 复用项（逐条）。
    pub items: Vec<String>,
    /// 同构不同参的差异说明。
    pub divergence: String,
}

/// 错误账本条目：（谁、错误码、原因 + 建议动作）。
pub type ErrEntry = (String, &'static str, String);

// ---------------------------------------------------------------------------
// 三、主结构：域总架构
// ---------------------------------------------------------------------------

/// 文字渲染域总架构（四段 + 单向流+ 冻结接口 + 三向兑现 + 六件套 + 预算）。
pub struct TextPipeline {
    /// 段间接口（`STAGE_COUNT - 1` 条：段 i → 段 i+1）。
    ifaces: Vec<InterfaceSpec>,
    /// 各段开工状态。
    states: [StageState; STAGE_COUNT],
    /// 已声明的数据流边（紧邻上游链）。
    flows: Vec<FlowEdge>,
    /// 已声明的供给边。
    supplies: Vec<SupplyEdge>,
    /// 三向承诺（顺序即台账序：字形向 / 排版向 / 渲染向）。
    commitments: Vec<Commitment>,
    /// 六件套。
    open_pack: OpenPack,
    /// 复用声明（`None` = 未出具）。
    reuse: Option<ReuseDeclaration>,
    /// ADR 台账。
    adrs: Vec<Adr>,
    /// 本帧各预算段的已用 µs。
    frame_used_us: [u32; BUDGET_SLICES.len()],
    /// 累计超预算次数（观测面）。
    budget_breaches: u32,
    /// 补写签收（`false` = S117 发现未签收，不许开工）。
    supplement_signed: bool,
    /// 审计留痕。
    audits: Vec<String>,
    /// 告警账。
    warnings: Vec<String>,
    /// 错误账本（零静默）。
    errors: Vec<ErrEntry>,
    tick: u64,
}

impl TextPipeline {
    /// 新建域骨架：四段齐备、全部未开工、接口未冻结、补写未签收。
    ///
    /// 三向承诺的判据在此**一次写死**（参数唯一源，避免各批 AI 各写一套口径）。
    pub fn new() -> Self {
        TextPipeline {
            ifaces: vec![
                // 段间接口 = 4 段之间的 3 条边（0→1、1→2、2→3），不多不少。
                // 槽 i 是段 i 交给段 i+1 的契约；段 0 无上游，故无入口契约。
                InterfaceSpec::new(
                    "GlyphSink",
                    1,
                    &["glyph_id", "outline_ref", "advance", "bearing_x", "bearing_y"],
                ),
                InterfaceSpec::new(
                    "ShapedRun",
                    1,
                    &["text", "font_instance", "clusters", "advance_total", "baseline", "script"],
                ),
                InterfaceSpec::new(
                    "QuadBatch",
                    1,
                    &["atlas_page", "uv_rect", "material_variant", "blend_mode", "effect_params"],
                ),
            ],
            states: [StageState::NotStarted; STAGE_COUNT],
            flows: Vec::new(),
            supplies: Vec::new(),
            commitments: vec![
                Commitment {
                    axis: Axis::Glyph,
                    promise: "光栅化质量（字形向）".to_string(),
                    criteria: vec![
                        mk_criterion("边缘平滑无锯齿台阶（8bit 覆盖率 + Gamma 感知混合）"),
                        mk_criterion("粗细四档（细/常规/半粗/粗）与合成口径一致"),
                        mk_criterion("小字号（≤12px）可辨；超大字号（>256px）分块不越界"),
                    ],
                },
                Commitment {
                    axis: Axis::Layout,
                    promise: "多语言正确（排版向）".to_string(),
                    criteria: vec![
                        mk_criterion("双向文本（RTL/LTR 混排）视觉序正确"),
                        mk_criterion("换行断点合法（不断字形、不丢簇映射）"),
                        mk_criterion("缺字五级回退链可达，不出豆腐"),
                    ],
                },
                Commitment {
                    axis: Axis::Render,
                    promise: "性能预算（渲染向）".to_string(),
                    criteria: vec![
                        mk_criterion("每帧文字渲染 ≤1.5ms（P07 范式，双通道计时）"),
                        mk_criterion("图集命中率常态 ≥97%"),
                        mk_criterion("文本层绘制调用 ≤2 次/帧（≥2,000 quads 合批）"),
                    ],
                },
            ],
            open_pack: OpenPack::new(),
            reuse: None,
            adrs: Vec::new(),
            frame_used_us: [0u32; 6],
            budget_breaches: 0,
            supplement_signed: false,
            audits: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            tick: 0,
        }
    }

    // -- 只读观测面 -----------------------------------------------------------

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// 某段的段名。
    pub fn stage_name(&self, stage: usize) -> &'static str {
        STAGE_NAMES.get(stage).copied().unwrap_or("越界段")
    }

    /// 某段开工状态。
    pub fn stage_state(&self, stage: usize) -> StageState {
        self.states.get(stage).copied().unwrap_or(StageState::NotStarted)
    }

    /// 已开工段数。
    pub fn open_stage_count(&self) -> usize {
        self.states
            .iter()
            .filter(|s| **s == StageState::Open)
            .count()
    }

    /// 取段间接口（`slot` = 0..2，对应段 slot→slot+1）。
    pub fn iface(&self, slot: usize) -> Option<&InterfaceSpec> {
        self.ifaces.get(slot)
    }

    /// 全部接口的冻结态快照。
    pub fn frozen_flags(&self) -> Vec<bool> {
        self.ifaces.iter().map(|i| i.frozen()).collect()
    }

    /// 全部数据流边。
    pub fn flows(&self) -> &[FlowEdge] {
        &self.flows
    }

    /// 全部供给边。
    pub fn supplies(&self) -> &[SupplyEdge] {
        &self.supplies
    }

    /// 三向承诺。
    pub fn commitments(&self) -> &[Commitment] {
        &self.commitments
    }

    /// 按轴取承诺（线性扫描 O(3)——三轴常量级，诚实标注）。
    pub fn commitment(&self, axis: Axis) -> Option<&Commitment> {
        self.commitments.iter().find(|c| c.axis == axis)
    }

    /// 六件套。
    pub fn open_pack(&self) -> &OpenPack {
        &self.open_pack
    }

    /// 复用声明。
    pub fn reuse(&self) -> Option<&ReuseDeclaration> {
        self.reuse.as_ref()
    }

    /// ADR 台账。
    pub fn adrs(&self) -> &[Adr] {
        &self.adrs
    }

    /// 本帧各段已用 µs。
    pub fn frame_used_us(&self) -> &[u32; BUDGET_SLICES.len()] {
        &self.frame_used_us
    }

    /// 本帧文字渲染总开销。
    pub fn frame_total_us(&self) -> u32 {
        self.frame_used_us.iter().sum()
    }

    /// 累计超预算次数。
    pub fn budget_breaches(&self) -> u32 {
        self.budget_breaches
    }

    /// 补写是否已签收。
    pub fn supplement_signed(&self) -> bool {
        self.supplement_signed
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 告警账。
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[ErrEntry] {
        &self.errors
    }

    fn record_error(&mut self, who: String, code: &'static str, detail: String) {
        self.errors.push((who, code, detail));
    }

    // -- 补写显性（S117 签收）-------------------------------------------------

    /// 签收 S117 审计发现（补写显性判据的执行面）。
    ///
    /// 未签收即调 [`TextPipeline::open_stage`] 会被 `E_SUPPLEMENT_NOT_SIGNED` 拦。
    pub fn signoff_s117(&mut self) {
        self.supplement_signed = true;
        self.audits.push(format!(
            "tick{} 签收 S117 审计发现：{}",
            self.tick,
            "VE-E 区段 F0801-F1000 因字母占用冲突从未落盘，S129 起 Eb01-Eb10 补写"
        ));
    }

    // -- 四段架构：接口冻结与开工 ---------------------------------------------

    /// 冻结段间接口（`slot` = 0..2，对应段 slot → 段 slot+1）。
    ///
    /// 幂等：重复冻结返回既有token 不报错（幂等是契约，不是宽容）。
    pub fn freeze_interface(&mut self, slot: usize) -> Result<u64, &'static str> {
        let spec = match self.ifaces.get_mut(slot) {
            Some(s) => s,
            None => {
                self.record_error(
                    format!("freeze_interface(slot={slot})"),
                    "E_SLOT_OUT_OF_RANGE",
                    format!("段间接口槽位越界（合法 0..{}）", STAGE_COUNT - 1),
                );
                return Err("段间接口槽位越界");
            }
        };
        if spec.frozen() {
            return Ok(spec.fingerprint());
        }
        spec.frozen_at = Some(self.tick);
        let fp = spec.fingerprint();
        self.audits.push(format!(
            "tick{} 冻结接口 {} v{}（指纹 {fp:#x}）——下游段解锁",
            self.tick, spec.name, spec.version
        ));
        Ok(fp)
    }

    /// 开工某段：硬闸为「补写已签收 + 全部上游接口冻结」。
    pub fn open_stage(&mut self, stage: usize) -> Result<(), &'static str> {
        if stage >= STAGE_COUNT {
            self.record_error(
                format!("open_stage({stage})"),
                "E_STAGE_OUT_OF_RANGE",
                format!("段号越界（合法 0..{STAGE_COUNT}）"),
            );
            return Err("段号越界");
        }
        if !self.supplement_signed {
            self.record_error(
                format!("open_stage({stage})"),
                "E_SUPPLEMENT_NOT_SIGNED",
                "S117 补写发现未签收——补写显性判据要求先签收再开工".to_string(),
            );
            return Err("补写未签收");
        }
        // 上游闸：段 stage 的全部上游接口（slot 0..stage-1）必须已冻结。
        for slot in 0..stage {
            let ok = self.ifaces.get(slot).map(|i| i.frozen()).unwrap_or(false);
            if !ok {
                self.record_error(
                    format!("open_stage({stage})"),
                    "E_UPSTREAM_NOT_FROZEN",
                    format!(
                        "段 {} 的上游接口（{} → {}）未冻结——段间接口未冻结禁止开工下游",
                        stage,
                        self.stage_name(slot),
                        self.stage_name(slot + 1)
                    ),
                );
                return Err("上游接口未冻结");
            }
        }
        self.states[stage] = StageState::Open;
        self.audits.push(format!(
            "tick{} 开工段 {}（{}）——上游接口全冻结",
            self.tick,
            stage,
            self.stage_name(stage)
        ));
        Ok(())
    }

    // -- 单向数据流：跳段与逆流拦截 -------------------------------------------

    /// 声明数据流边：只容许紧邻上游（`to == from + 1`）。
    pub fn declare_flow(&mut self, from: usize, to: usize) -> Result<(), &'static str> {
        if from >= STAGE_COUNT || to >= STAGE_COUNT {
            self.record_error(
                format!("declare_flow({from},{to})"),
                "E_STAGE_OUT_OF_RANGE",
                format!("段号越界（合法 0..{STAGE_COUNT}）"),
            );
            return Err("段号越界");
        }
        if to != from + 1 {
            let code = if to <= from { "E_FLOW_BACKWARD" } else { "E_FLOW_BYPASS" };
            let why = if to <= from {
                "数据流逆流（下游喂上游）"
            } else {
                "数据流跳段（绕过中间段）"
            };
            self.record_error(
                format!("declare_flow({from},{to})"),
                code,
                format!(
                    "{why}：数据流只容许紧邻上游（{} → {}），{why}一律拒绝",
                    self.stage_name(from),
                    self.stage_name(to)
                ),
            );
            return Err("非法数据流边");
        }
        self.flows.push(FlowEdge { from, to });
        self.audits.push(format!(
            "tick{} 声明数据流 {} → {}（单向紧邻）",
            self.tick,
            self.stage_name(from),
            self.stage_name(to)
        ));
        Ok(())
    }

    /// 声明供给边：跨段容许、方向受限（`to > from` 或显式登记的定向供给），
    /// 且**不得与数据流边重复登记**（防止借供给边开旁路）。
    pub fn declare_supply(
        &mut self,
        from: usize,
        to: usize,
        payload: &str,
    ) -> Result<(), &'static str> {
        if from >= STAGE_COUNT || to >= STAGE_COUNT {
            self.record_error(
                format!("declare_supply({from},{to})"),
                "E_STAGE_OUT_OF_RANGE",
                format!("段号越界（合法 0..{STAGE_COUNT}）"),
            );
            return Err("段号越界");
        }
        if to == from {
            self.record_error(
                format!("declare_supply({from},{to})"),
                "E_SUPPLY_SELF_LOOP",
                "供给边不得自环——自环是段内循环，不跨段".to_string(),
            );
            return Err("供给边自环");
        }
        if self.flows.iter().any(|f| f.from == from && f.to == to) {
            self.record_error(
                format!("declare_supply({from},{to})"),
                "E_SUPPLY_SHADOWS_FLOW",
                "该方向已登记为数据流边，不得再登记为供给边".to_string(),
            );
            return Err("供给边与数据流边重复");
        }
        self.supplies.push(SupplyEdge {
            from,
            to,
            payload: payload.to_string(),
        });
        self.audits.push(format!(
            "tick{} 声明供给边 {} → {}（{payload}）——供给边不是数据流",
            self.tick,
            self.stage_name(from),
            self.stage_name(to)
        ));
        Ok(())
    }

    /// 单向数据流不变式核验：链完整、无跳段、无逆流。
    pub fn verify_flow(&self) -> bool {
        for slot in 0..(STAGE_COUNT - 1) {
            let found = self
                .flows
                .iter()
                .any(|f| f.from == slot && f.to == slot + 1);
            if !found {
                return false;
            }
        }
        self.flows.iter().all(|f| f.to == f.from + 1)
    }

    // -- 三向兑现 --------------------------------------------------------------

    /// 给某轴的某条判据挂证据（证据文本非空才算兑现）。
    pub fn attach_evidence(&mut self, axis: Axis, criterion_text: &str, evidence: &str) -> bool {
        if evidence.is_empty() {
            self.warnings.push(format!(
                "W_EMPTY_EVIDENCE：{} /「{criterion_text}」的空证据被拒",
                axis.name()
            ));
            return false;
        }
        match self
            .commitments
            .iter_mut()
            .find(|c| c.axis == axis)
            .and_then(|c| c.criteria.iter_mut().find(|k| k.text == criterion_text))
        {
            Some(k) => {
                k.evidence.push(evidence.to_string());
                self.audits.push(format!(
                    "tick{} {} 判据「{criterion_text}」挂证据：{evidence}",
                    self.tick,
                    axis.name()
                ));
                true
            }
            None => {
                self.record_error(
                    format!("attach_evidence({}, {criterion_text})", axis.name()),
                    "E_CRITERION_NOT_FOUND",
                    "判据文本不在册——判据集为参数唯一源，不接受临时新增".to_string(),
                );
                false
            }
        }
    }

    /// 三轴兑现全景（顺序固定：字形向 / 排版向 / 渲染向）。
    pub fn three_way(&self) -> Vec<(Axis, bool, usize)> {
        self.commitments
            .iter()
            .map(|c| (c.axis, c.fulfilled(), c.pending()))
            .collect()
    }

    /// 三向是否全部兑现。
    pub fn three_way_fulfilled(&self) -> bool {
        !self.commitments.is_empty() && self.commitments.iter().all(|c| c.fulfilled())
    }

    // -- 六件套与复用声明 ------------------------------------------------------

    /// 填六件套槽。
    pub fn fill_open_slot(&mut self, slot: &str, body: &str) -> bool {
        let ok = self.open_pack.fill(slot, body);
        if !ok {
            self.record_error(
                format!("fill_open_slot({slot})"),
                "E_OPEN_SLOT_UNKNOWN",
                "六件套槽名不在册——X01 体例只认定位/架构/契约/预算/风险/里程碑".to_string(),
            );
        }
        ok
    }

    /// 出具复用声明（复用 X01 开工范式）。
    pub fn issue_reuse_declaration(&mut self) {
        self.reuse = Some(ReuseDeclaration {
            paradigm: "X01 开工范式".to_string(),
            items: OPEN_PACK_SLOTS.iter().map(|s| format!("六件套槽：{s}")).collect(),
            divergence: "同构不同参：X01 段间数据为几何图元，本域段间数据为字形与排版簇".to_string(),
        });
        self.audits.push(format!(
            "tick{} 出具复用声明：复用 {}，六件套体例同构",
            self.tick, "X01 开工范式"
        ));
    }

    /// 六件套是否齐备（复用声明已出具 + 无缺槽）。
    pub fn open_pack_ready(&self) -> bool {
        self.reuse.is_some() && self.open_pack.complete()
    }

    // -- ADR：架构变更走 ADR ---------------------------------------------------

    /// 提ADR（提案态——不可据此改冻结接口）。
    pub fn propose_adr(&mut self, title: &str, reason: &str, stages: &[usize]) -> u64 {
        let id = self.adrs.len() as u64 + 1;
        self.adrs.push(Adr {
            id,
            title: title.to_string(),
            reason: reason.to_string(),
            stages: stages.to_vec(),
            state: AdrState::Proposed,
        });
        self.audits.push(format!("tick{} 提ADR #{id}：{title}（提案态）", self.tick));
        id
    }

    /// 接受 ADR。
    pub fn accept_adr(&mut self, id: u64) -> Result<(), &'static str> {
        match self.adrs.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                a.state = AdrState::Accepted;
                self.audits.push(format!("tick{} 接受 ADR #{id}：{}", self.tick, a.title));
                Ok(())
            }
            None => {
                self.record_error(
                    format!("accept_adr({id})"),
                    "E_ADR_NOT_FOUND",
                    "ADR 编号不在册".to_string(),
                );
                Err("ADR 不存在")
            }
        }
    }

    /// 否决 ADR。
    pub fn reject_adr(&mut self, id: u64) -> Result<(), &'static str> {
        match self.adrs.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                a.state = AdrState::Rejected;
                Ok(())
            }
            None => {
                self.record_error(
                    format!("reject_adr({id})"),
                    "E_ADR_NOT_FOUND",
                    "ADR 编号不在册".to_string(),
                );
                Err("ADR 不存在")
            }
        }
    }

    /// 改段间接口：已冻结者必先有**已接受**的 ADR（架构变更走 ADR 判据）。
    pub fn revise_interface(
        &mut self,
        slot: usize,
        new_spec: InterfaceSpec,
        adr_id: u64,
    ) -> Result<u64, &'static str> {
        let accepted = self
            .adrs
            .iter()
            .any(|a| a.id == adr_id && a.state == AdrState::Accepted);
        if !accepted {
            self.record_error(
                format!("revise_interface(slot={slot}, adr={adr_id})"),
                "E_ADR_REQUIRED",
                "改已冻结接口须先有已接受 ADR——提案态/否决态/不存在均不可".to_string(),
            );
            return Err("无已接受 ADR");
        }
        let spec = match self.ifaces.get_mut(slot) {
            Some(s) => s,
            None => {
                self.record_error(
                    format!("revise_interface(slot={slot})"),
                    "E_SLOT_OUT_OF_RANGE",
                    "段间接口槽位越界".to_string(),
                );
                return Err("槽位越界");
            }
        };
        let mut next = new_spec;
        next.frozen_at = Some(self.tick);
        next.adr = Some(adr_id);
        *spec = next;
        let fp = spec.fingerprint();
        self.audits.push(format!(
            "tick{} 按 ADR #{adr_id} 改接口 {}（指纹 {fp:#x}）",
            self.tick, spec.name
        ));
        Ok(fp)
    }

    // -- 预算：1.5ms 分解 + 三水位 + 降级序 -------------------------------------

    /// 记某预算段的本帧开销（µs），越段配额记账并出告警。
    pub fn charge(&mut self, slice: usize, us: u32) -> Result<(), &'static str> {
        if slice >= BUDGET_SLICES.len() {
            self.record_error(
                format!("charge({slice})"),
                "E_SLICE_OUT_OF_RANGE",
                format!("预算段越界（合法 0..{}）", BUDGET_SLICES.len()),
            );
            return Err("预算段越界");
        }
        let cap = BUDGET_SLICES[slice].1;
        self.frame_used_us[slice] = self.frame_used_us[slice].saturating_add(us);
        if self.frame_used_us[slice] > cap {
            self.warnings.push(format!(
                "W_SLICE_OVER：段「{}」已用 {}µs 超配额 {cap}µs",
                BUDGET_SLICES[slice].0,
                self.frame_used_us[slice]
            ));
        }
        Ok(())
    }

    /// 帧末结算：总开销与水位；超预算记账（不静默截断）。
    pub fn settle_frame(&mut self) -> Water {
        let total = self.frame_total_us();
        let w = Water::judge(total);
        if total > FRAME_BUDGET_US {
            self.budget_breaches += 1;
            self.record_error(
                format!("settle_frame(total={total}µs)"),
                "E_BUDGET_BREACH",
                format!(
                    "本帧文字渲染 {total}µs 超架构预算 {FRAME_BUDGET_US}µs（1.5ms）——\
按降级序处置：{}",
                    self.degradation_plan()
                        .iter()
                        .map(|s| s.name.clone())
                        .collect::<Vec<_>>()
                        .join(" → ")
                ),
            );
        } else {
            self.audits.push(format!(
                "tick{} 帧结算：{total}µs / {FRAME_BUDGET_US}µs，水位{}",
                self.tick,
                w.name()
            ));
        }
        self.frame_used_us = [0u32; 6];
        w
    }

    /// 降级序：先降 Hinting 档 → 再关亚像素相位 → 最后减动字号采样率。
    ///
    /// 无障碍路径（放大 / 高对比）**永不在降级序内**——红线保护序，
    /// 有人塞进来即断言拦截并记账（`E_A11Y_PROTECTED`）。
    pub fn degradation_plan(&self) -> Vec<DegradeStep> {
        [
            ("降 Hinting 档（全 Hinting → 轻 Hinting → 无 Hinting）", false),
            ("关亚像素定位相位（1/4 相位 → 整数像素）", false),
            ("减动字号采样率（动态字号降采样）", false),
        ]
        .iter()
        .enumerate()
        .map(|(i, (n, a11y))| DegradeStep {
            order: i,
            name: n.to_string(),
            touches_a11y: *a11y,
        })
        .collect()
    }

    /// 申报降级步（校验无障碍红线；越红线即拒）。
    pub fn declare_degrade_step(&mut self, order: usize, name: &str, touches_a11y: bool) -> Result<(), &'static str> {
        if touches_a11y {
            self.record_error(
                format!("declare_degrade_step({order}, {name})"),
                "E_A11Y_PROTECTED",
                "降级序不得触碰无障碍路径（放大/高对比）——红线保护序凌驾性能预算".to_string(),
            );
            return Err("无障碍路径受保护");
        }
        self.audits.push(format!("tick{} 申报降级步 #{order}：{name}", self.tick));
        Ok(())
    }

    /// 预算分解自洽核验（六段之和 == 1.5ms；遥测段 == 20µs）。
    pub fn budget_consistent(&self) -> bool {
        let sum: u32 = BUDGET_SLICES.iter().map(|(_, v)| *v).sum();
        sum == FRAME_BUDGET_US && TELEMETRY_SLICE_US == 20 && FRAME_BUDGET_US == 1500
    }

    // -- 域开工闸 --------------------------------------------------------------

    /// 域是否具备开工条件（补写已签收 + 六件套齐备 + 三向已立）。
    pub fn open_gates_ready(&self) -> bool {
        self.supplement_signed && self.open_pack_ready() && !self.commitments.is_empty()
    }

    /// 域就绪（开工闸全绿 + 四段全开工 + 单向流成立 + 三向全兑现 + 预算自洽）。
    pub fn domain_ready(&self) -> bool {
        self.open_gates_ready()
            && self.open_stage_count() == STAGE_COUNT
            && self.verify_flow()
            && self.three_way_fulfilled()
            && self.budget_consistent()
    }

    /// 未就绪原因清单（人话可读，零静默）。
    pub fn not_ready_reasons(&self) -> Vec<String> {
        let mut r = Vec::new();
        if !self.supplement_signed {
            r.push("补写未签收（S117 发现）".to_string());
        }
        if self.reuse.is_none() {
            r.push("复用声明未出具".to_string());
        }
        for m in self.open_pack.missing() {
            r.push(format!("六件套缺槽：{m}"));
        }
        if self.open_stage_count() != STAGE_COUNT {
            r.push(format!(
                "已开工 {}/4 段",
                self.open_stage_count()
            ));
        }
        if !self.verify_flow() {
            r.push("单向数据流链不完整".to_string());
        }
        for (axis, ok, pending) in self.three_way() {
            if !ok {
                r.push(format!("{}尚缺 {pending} 条判据证据", axis.name()));
            }
        }
        r
    }

    /// 域总账（一次性出报告文本，进文档账）。
    pub fn domain_report(&self) -> String {
        let mut s = String::new();
        s.push_str("文字渲染域总架构（VE-E / VE-F0801）\n");
        s.push_str(&format!("  补写签收：{}\n", if self.supplement_signed { "是" } else { "否" }));
        for st in 0..STAGE_COUNT {
            s.push_str(&format!(
                "  段 {st} {}：{:?}（接口 {}）\n",
                self.stage_name(st),
                self.stage_state(st),
                if st < STAGE_COUNT - 1 {
                    let i = &self.ifaces[st];
                    format!(
                        "{} v{}{}",
                        i.name,
                        i.version,
                        if i.frozen() { " 已冻结" } else { " 未冻结" }
                    )
                } else {
                    "—".to_string()
                }
            ));
        }
        for (axis, ok, pending) in self.three_way() {
            s.push_str(&format!(
                "  {}：{}（缺 {pending} 条证据）\n",
                axis.name(),
                if ok { "已兑现" } else { "未兑现" }
            ));
        }
        s.push_str(&format!(
            "  预算：{FRAME_BUDGET_US}µs/帧，水位阈值 绿≤{WATER_GREEN_MAX_US} / 黄≤{WATER_YELLOW_MAX_US}\n"
        ));
        s.push_str(&format!(
            "  域就绪：{}\n",
            if self.domain_ready() { "是" } else { "否" }
        ));
        s
    }

    // -- 自检注册入口 -----------------------------------------------------------

    /// VE-F0801 域自检（判据逐条映射见 `vee01_checks.rs`）。
    pub fn run_vee01_checks() -> CheckSet {
        super::vee01_checks::run_vee01_checks()
    }
}

impl Default for TextPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// 构造判据条目（未挂证据）。
fn mk_criterion(text: &str) -> Criterion {
    Criterion { text: text.to_string(), evidence: Vec::new() }
}

// ---------------------------------------------------------------------------
// 四、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0801 域自检。
pub fn run_vee01_checks() -> CheckSet {
    super::vee01_checks::run_vee01_checks()
}

// ---------------------------------------------------------------------------
// 五、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一条已签收 + 四段全开工 + 流向完整的骨架。
    fn ready_skeleton() -> TextPipeline {
        let mut p = TextPipeline::new();
        p.signoff_s117();
        for slot in 0..STAGE_COUNT - 1 {
            let _ = p.freeze_interface(slot);
        }
        for stage in 0..STAGE_COUNT {
            let _ = p.open_stage(stage);
        }
        for slot in 0..STAGE_COUNT - 1 {
            let _ = p.declare_flow(slot, slot + 1);
        }
        p
    }

    #[test]
    fn vee01_four_stage_unidirectional_chain() {
        let mut p = ready_skeleton();
        assert_eq!(p.open_stage_count(), STAGE_COUNT);
        assert!(p.verify_flow(), "四段单向链完整");
        assert_eq!(p.flows().len(), 3);
        // 跳段被拒。
        assert!(p.declare_flow(0, 2).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_FLOW_BYPASS"));
        // 逆流被拒。
        assert!(p.declare_flow(3, 1).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_FLOW_BACKWARD"));
        assert!(p.verify_flow(), "非法边未污染链");
    }

    #[test]
    fn vee01_supply_edge_typed_apart_from_flow() {
        let mut p = ready_skeleton();
        // 字体管理 → 字形段的表供给（跨段反向，合法供给边）。
        assert!(p.declare_supply(SEG_FONT, SEG_GLYPH, "FontTables").is_ok());
        assert_eq!(p.supplies().len(), 1);
        assert!(p.verify_flow(), "供给边不影响数据流核验");
        // 自环拒。
        assert!(p.declare_supply(1, 1, "X").is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SUPPLY_SELF_LOOP"));
        // 与已登记数据流边重复 → 拒（防借供给边开旁路）。
        assert!(p.declare_supply(0, 1, "Shadow").is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SUPPLY_SHADOWS_FLOW"));
    }

    #[test]
    fn vee01_upstream_freeze_gates_downstream() {
        let mut p = TextPipeline::new();
        // 未签收 → 全段禁开工。
        assert!(p.open_stage(0).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SUPPLEMENT_NOT_SIGNED"));
        p.signoff_s117();
        // 签收后：段 0 可开工（无上游）。
        assert!(p.open_stage(0).is_ok());
        // 段 1 因接口未冻结被拦。
        assert!(p.open_stage(1).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_UPSTREAM_NOT_FROZEN"));
        let fp = p.freeze_interface(0).unwrap();
        assert!(p.open_stage(1).is_ok());
        // 冻结幂等。
        assert_eq!(p.freeze_interface(0).unwrap(), fp);
        assert_eq!(p.frozen_flags(), alloc::vec![true, false, false], "只冻结了槽 0；4 段之间共 3 条段间接口");
        // 段 3 无下游，故无第四条段间接口（槽 3 越界显性拒）。
        assert!(p.freeze_interface(3).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SLOT_OUT_OF_RANGE"));
    }

    #[test]
    fn vee01_three_way_commitment() {
        let mut p = ready_skeleton();
        assert!(!p.three_way_fulfilled());
        let (_, _, pending_g) = p.three_way()[0];
        assert_eq!(pending_g, 3);
        // 空证据被拒。
        assert!(!p.attach_evidence(Axis::Glyph, "边缘平滑无锯齿台阶（8bit 覆盖率 + Gamma 感知混合）", ""));
        assert!(p.warnings().iter().any(|w| w.contains("W_EMPTY_EVIDENCE")));
        // 不在册判据被拒（判据集唯一源）。
        assert!(!p.attach_evidence(Axis::Glyph, "临时新增判据", "e"));
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_CRITERION_NOT_FOUND"));
        // 逐轴挂证据。
        for c in p.commitments()[0].criteria.clone() {
            assert!(p.attach_evidence(Axis::Glyph, &c.text, "金样剖面 0.5% 内"));
        }
        for c in p.commitments()[1].criteria.clone() {
            assert!(p.attach_evidence(Axis::Layout, &c.text, "对拍集零偏差"));
        }
        assert!(!p.three_way_fulfilled(), "渲染向未兑现仍不算齐");
        for c in p.commitments()[2].criteria.clone() {
            assert!(p.attach_evidence(Axis::Render, &c.text, "双通道计时在册"));
        }
        assert!(p.three_way_fulfilled());
        assert!(p.three_way().iter().all(|(_, ok, _)| *ok));
    }

    #[test]
    fn vee01_adr_gates_interface_revision() {
        let mut p = ready_skeleton();
        let before = p.iface(0).unwrap().fingerprint();
        let new_spec = InterfaceSpec::new("GlyphSink", 2, &["glyph_id", "bitmap", "advance"]);
        // 无 ADR → 拒。
        assert!(p.revise_interface(0, new_spec.clone(), 999).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_ADR_REQUIRED"));
        // 提案态 ADR → 仍拒。
        let id = p.propose_adr("GlyphSink 精简", "去掉冗余字段", &[SEG_GLYPH]);
        assert!(p.revise_interface(0, new_spec.clone(), id).is_err());
        // 接受后 → 放行，指纹变化。
        assert!(p.accept_adr(id).is_ok());
        let after = p.revise_interface(0, new_spec, id).unwrap();
        assert_ne!(before, after, "契约指纹随字段变化");
        assert_eq!(p.iface(0).unwrap().adr, Some(id));
        assert!(p.iface(0).unwrap().frozen(), "改后仍处冻结态");
        // 否决态不可用。
        let id2 = p.propose_adr("x", "y", &[SEG_RENDER]);
        assert!(p.reject_adr(id2).is_ok());
        assert!(p.revise_interface(1, InterfaceSpec::new("X", 1, &[]), id2).is_err());
    }

    #[test]
    fn vee01_budget_decomposition_and_water() {
        let mut p = ready_skeleton();
        assert!(p.budget_consistent());
        assert_eq!(FRAME_BUDGET_US, 1500);
        // 绿水位。
        assert_eq!(Water::judge(300), Water::Green);
        assert_eq!(Water::judge(301), Water::Yellow);
        assert_eq!(Water::judge(750), Water::Yellow);
        assert_eq!(Water::judge(751), Water::Red);
        // 超配额段告警。
        assert!(p.charge(1, 600).is_ok());
        assert!(p.warnings().iter().any(|w| w.contains("W_SLICE_OVER")));
        assert_eq!(p.frame_total_us(), 600);
        // 总超预算 → 记账 + 给降级序，不静默。
        assert!(p.charge(2, 1000).is_ok());
        let w = p.settle_frame();
        assert_eq!(w, Water::Red);
        assert_eq!(p.budget_breaches(), 1);
        let e = p.errors().last().unwrap();
        assert_eq!(e.1, "E_BUDGET_BREACH");
        assert!(e.2.contains("Hinting"));
        assert_eq!(p.frame_total_us(), 0, "结算后清零");
        // 越段拒。
        assert!(p.charge(9, 1).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SLICE_OUT_OF_RANGE"));
    }

    #[test]
    fn vee01_degradation_order_and_a11y_redline() {
        let p = ready_skeleton();
        let plan = p.degradation_plan();
        assert_eq!(plan.len(), 3);
        assert!(plan[0].name.contains("Hinting"), "先降 Hinting 档");
        assert!(plan[1].name.contains("亚像素"), "再关亚像素相位");
        assert!(plan[2].name.contains("动字号"), "最后减动字号采样率");
        assert!(plan.iter().all(|s| !s.touches_a11y), "降级序不含无障碍路径");
        let mut p2 = p;
        assert!(p2.declare_degrade_step(0, "降 Hinting 档", false).is_ok());
        assert!(p2.declare_degrade_step(1, "关高对比", true).is_err());
        assert!(p2.errors().iter().any(|(_, c, _)| *c == "E_A11Y_PROTECTED"));
    }

    #[test]
    fn vee01_open_pack_and_reuse_declaration() {
        let mut p = ready_skeleton();
        assert!(!p.open_pack_ready());
        for s in OPEN_PACK_SLOTS {
            assert!(p.fill_open_slot(s, "已填"));
        }
        assert!(!p.open_pack_ready(), "复用声明未出具仍不算齐");
        assert!(!p.fill_open_slot("不存在", "x"));
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_OPEN_SLOT_UNKNOWN"));
        p.issue_reuse_declaration();
        assert!(p.open_pack_ready());
        let r = p.reuse().unwrap();
        assert_eq!(r.paradigm, "X01 开工范式");
        assert_eq!(r.items.len(), 6);
        assert!(r.divergence.contains("同构不同参"));
        assert!(X01_REUSE_DOC.contains("六件套"));
    }

    #[test]
    fn vee01_domain_ready_and_report() {
        let mut p = TextPipeline::new();
        assert!(!p.domain_ready());
        assert!(!p.not_ready_reasons().is_empty());
        p.signoff_s117();
        for slot in 0..STAGE_COUNT - 1 {
            let _ = p.freeze_interface(slot);
        }
        for stage in 0..STAGE_COUNT {
            let _ = p.open_stage(stage);
        }
        for slot in 0..STAGE_COUNT - 1 {
            let _ = p.declare_flow(slot, slot + 1);
        }
        for s in OPEN_PACK_SLOTS {
            p.fill_open_slot(s, "已填");
        }
        p.issue_reuse_declaration();
        assert!(p.open_gates_ready());
        assert!(!p.domain_ready(), "三向未兑现不得就绪");
        for c in p.commitments().to_vec() {
            let axis = c.axis;
            for k in c.criteria {
                p.attach_evidence(axis, &k.text, "证据在册");
            }
        }
        assert!(p.domain_ready(), "全闸齐绿");
        assert!(p.not_ready_reasons().is_empty());
        let rep = p.domain_report();
        assert!(rep.contains("文字渲染域总架构"));
        assert!(rep.contains("域就绪：是"));
        assert!(SUPPLEMENT_MANIFEST.contains("F0801-F1000"));
    }

    #[test]
    fn vee01_unidirectional_doc_and_no_silent_errors() {
        let p = ready_skeleton();
        assert!(UNIDIRECTIONAL_FLOW_DOC.contains("紧邻上游"));
        assert!(UNIDIRECTIONAL_FLOW_DOC.contains("供给边不是数据流"));
        // 零静默：每条错误都带可读原因与建议动作（非空 detail）。
        assert!(p.errors().is_empty());
        let mut q = TextPipeline::new();
        assert!(q.open_stage(0).is_err());
        assert!(q.declare_flow(2, 0).is_err());
        assert!(q.charge(99, 1).is_err());
        for (_, _, detail) in q.errors() {
            assert!(!detail.is_empty(), "错误必带原因与建议");
        }
    }

    #[test]
    fn vee01_checks_all_green() {
        let set = run_vee01_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0801 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}