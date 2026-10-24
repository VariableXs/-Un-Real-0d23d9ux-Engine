//! VE-F3544 · persona 冲突全表机检 —— 覆盖链逐项扫描 + 冲突可视化 + 仲裁规则公开。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3544`
//!
//! # 职责（锚点原文拆解）
//!
//! - **全表机检**（[`ConflictMatrix`]）：Persona 包 vs 官方主题 vs
//!   用户微调三层，每个令牌位逐项机检——不是抽查是全表。
//! - **覆盖链扫描**（[`OverrideChain`]）：三层谁覆盖谁有明确
//!   链序（用户 > Persona > 官方），冲突=同令牌位多层取值不一致
//!   且不是链序内的正常覆盖。
//! - **可视化**（[`ConflictView`]）：冲突位渲染为可读视图
//!   （每冲突一行：令牌名/三层值/胜出方），读屏可达。
//! - **规则公开**（[`ARBITRATION_RULES`]）：仲裁规则是公开常量
//!   文本，不是埋在代码里的暗规则。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 冲突未检 | 兜底全表再扫 |
//! | 可视化缺失 | 补齐（每冲突必有行） |
//! | 仲裁不公 | 规则修正（规则版本化） |
//!
//! # 复杂度
//!
//! O(冲突数)（只枚举冲突位）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6060 起）
// ---------------------------------------------------------------------------

/// 冲突机检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictCode {
    /// 检出冲突。
    ConflictFound = 6060,
    /// 兜底扫描触发（首扫漏检自纠）。
    FallbackScan = 6061,
    /// 视图缺行（可视化不全）。
    ViewIncomplete = 6062,
    /// 仲裁规则版本漂移。
    RuleDrift = 6063,
    /// 链序非法。
    ChainBroken = 6064,
}

impl ConflictCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ConflictCode::ConflictFound => "检出令牌冲突",
            ConflictCode::FallbackScan => "兜底扫描已触发",
            ConflictCode::ViewIncomplete => "冲突视图缺行",
            ConflictCode::RuleDrift => "仲裁规则版本漂移",
            ConflictCode::ChainBroken => "覆盖链序断裂",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, ConflictCode::ChainBroken | ConflictCode::RuleDrift)
    }
}

// ---------------------------------------------------------------------------
// 二、规则公开
// ---------------------------------------------------------------------------

/// 仲裁规则版本。
pub const RULE_VERSION: u32 = 1;

/// 仲裁规则公开文本（用户可读、可引用）。
pub const ARBITRATION_RULES: &str = "\
规则1：链序优先级 用户微调 > Persona 包 > 官方主题。\
规则2：同层同令牌多值 = 真冲突，须人工裁决，不自动覆盖。\
规则3：高优先层对低优先层的覆盖不算冲突（属正常链序）。\
规则4：用户微调层的任何值永不被自动改写。";

// ---------------------------------------------------------------------------
// 三、令牌位与三层值
// ---------------------------------------------------------------------------

/// 令牌层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenLayer {
    /// 官方主题。
    Official,
    /// Persona 包。
    Persona,
    /// 用户微调。
    User,
}

impl TokenLayer {
    /// 链序权重（大者胜）。
    pub const fn rank(self) -> u8 {
        match self {
            TokenLayer::Official => 0,
            TokenLayer::Persona => 1,
            TokenLayer::User => 2,
        }
    }

    /// 层短名。
    pub const fn label(self) -> &'static str {
        match self {
            TokenLayer::Official => "官方",
            TokenLayer::Persona => "Persona",
            TokenLayer::User => "用户",
        }
    }
}

/// 单令牌位的三层取值（None = 该层未设置）。
#[derive(Clone, Copy, Debug)]
pub struct TokenSlot {
    /// 令牌名 id。
    pub token: u32,
    /// 官方值。
    pub official: Option<u32>,
    /// Persona 值。
    pub persona: Option<u32>,
    /// 用户值。
    pub user: Option<u32>,
}

impl TokenSlot {
    /// 取指定层的值。
    pub fn value_at(&self, layer: TokenLayer) -> Option<u32> {
        match layer {
            TokenLayer::Official => self.official,
            TokenLayer::Persona => self.persona,
            TokenLayer::User => self.user,
        }
    }

    /// 胜出层（链序最高且有值的层）。
    pub fn winner(&self) -> Option<TokenLayer> {
        if self.user.is_some() {
            Some(TokenLayer::User)
        } else if self.persona.is_some() {
            Some(TokenLayer::Persona)
        } else {
            self.official.map(|_| TokenLayer::Official)
        }
    }

    /// 是否真冲突：多层有值且值不一致且不属正常覆盖
    /// （正常覆盖 = 严格按链序取最高层；真冲突 = 用户层缺席时
    ///  Persona 与官方不一致，或用户层与下层全不一致且下层彼此也不一致）。
    pub fn is_conflict(&self) -> bool {
        match (self.official, self.persona, self.user) {
            // 三层都有值。
            (Some(o), Some(p), Some(u)) => {
                // 用户层胜出是正常覆盖；但若 Persona 与官方不一致、
                // 用户又与两者都不同，算三方分歧冲突。
                o != p && u != o && u != p
            }
            // 无用户层：Persona 与官方不一致 → 人工裁决。
            (Some(o), Some(p), None) => o != p,
            // 用户 vs 官方单挑：正常覆盖（用户优先）。
            (Some(_), None, Some(_)) => false,
            // 用户 vs Persona 单挑：正常覆盖。
            (None, Some(_), Some(_)) => false,
            // 单层或全空：无冲突。
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、冲突矩阵
// ---------------------------------------------------------------------------

/// 单条冲突。
#[derive(Clone, Copy, Debug)]
pub struct ConflictEntry {
    /// 令牌位。
    pub slot: TokenSlot,
    /// 胜出层。
    pub winner: TokenLayer,
}

/// 全表冲突矩阵。
#[derive(Default)]
pub struct ConflictMatrix {
    slots: Vec<TokenSlot>,
    /// 兜底扫描计数。
    pub fallback_scans: u32,
}

impl ConflictMatrix {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 压入令牌位。
    pub fn push(&mut self, slot: TokenSlot) {
        self.slots.push(slot);
    }

    /// 全表机检：收集所有真冲突。
    pub fn scan(&mut self) -> Vec<ConflictEntry> {
        let mut hits = Vec::new();
        for s in &self.slots {
            if s.is_conflict() {
                if let Some(w) = s.winner() {
                    hits.push(ConflictEntry { slot: *s, winner: w });
                }
            }
        }
        hits
    }

    /// 兜底扫描（首扫结果数与二次扫描不一致时自纠）。
    pub fn scan_with_fallback(&mut self) -> Vec<ConflictEntry> {
        let first = self.scan();
        let second = self.scan();
        if first.len() != second.len() {
            self.fallback_scans = self.fallback_scans.saturating_add(1);
            return second;
        }
        first
    }

    /// 令牌位总数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 链序完整性：每层至少出现一次（链断 = 覆盖序无意义）。
    pub fn chain_intact(&self) -> bool {
        let mut has_official = false;
        let mut has_persona = false;
        let mut has_user = false;
        for s in &self.slots {
            if s.official.is_some() {
                has_official = true;
            }
            if s.persona.is_some() {
                has_persona = true;
            }
            if s.user.is_some() {
                has_user = true;
            }
        }
        has_official && has_persona && has_user
    }
}

// ---------------------------------------------------------------------------
// 五、冲突可视化
// ---------------------------------------------------------------------------

/// 可视化行。
#[derive(Clone, Debug)]
pub struct ViewRow {
    /// 令牌 id。
    pub token: u32,
    /// 三层值渲染。
    pub layers_text: String,
    /// 胜出方渲染。
    pub winner_text: String,
}

/// 冲突视图（每冲突恰好一行）。
#[derive(Default)]
pub struct ConflictView {
    rows: Vec<ViewRow>,
}

impl ConflictView {
    /// 从冲突集生成视图。
    pub fn render(hits: &[ConflictEntry]) -> Self {
        let mut rows = Vec::new();
        for h in hits {
            let layers_text = alloc::format!(
                "官方={:?} Persona={:?} 用户={:?}",
                h.slot.official, h.slot.persona, h.slot.user
            );
            let winner_text = h.winner.label().into();
            rows.push(ViewRow {
                token: h.slot.token,
                layers_text,
                winner_text,
            });
        }
        Self { rows }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 行数与冲突数一致性（可视化完整性）。
    pub fn complete_for(&self, hits: usize) -> bool {
        self.rows.len() == hits
    }

    /// 首行摘要（读屏）。
    pub fn first_row_text(&self) -> Option<String> {
        self.rows.first().map(|r| {
            alloc::format!("令牌{}: {} 胜出={}", r.token, r.layers_text, r.winner_text)
        })
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// 冲突机检判据集。
pub fn run_conflict_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10d_conflictcheck");

    // 1. 三方分歧检出。
    let clash = TokenSlot {
        token: 1,
        official: Some(100),
        persona: Some(200),
        user: Some(300),
    };
    set.add("6060-三方分歧检出", clash.is_conflict(), "");

    // 2. 正常覆盖不误报（用户 > Persona）。
    let normal = TokenSlot {
        token: 2,
        official: Some(100),
        persona: Some(100),
        user: Some(250),
    };
    set.add("6061-正常覆盖不误报", !normal.is_conflict(), "");

    // 3. Persona vs 官方分歧（无用户层）检出。
    let two_layer = TokenSlot {
        token: 3,
        official: Some(1),
        persona: Some(2),
        user: None,
    };
    set.add("6062-两层分歧检出", two_layer.is_conflict(), "");

    // 4. 全表机检数量正确。
    let mut m = ConflictMatrix::new();
    m.push(clash);
    m.push(normal);
    m.push(two_layer);
    m.push(TokenSlot {
        token: 4,
        official: Some(9),
        persona: None,
        user: None,
    });
    let hits = m.scan();
    set.add("6063-全表机检数量", hits.len() == 2, "");

    // 5. 可视化行数对齐。
    let view = ConflictView::render(&hits);
    set.add("6064-视图完整性", view.complete_for(hits.len()), "");

    // 6. 胜出层正确（三方分歧时用户胜）。
    let winner_ok = hits.iter().all(|h| h.winner.rank() == TokenLayer::User.rank() || h.slot.user.is_none());
    set.add("6065-胜出链序正确", winner_ok, "");

    // 7. 链序完整性判定。
    set.add("6066-链序完整", m.chain_intact(), "");

    set
}
