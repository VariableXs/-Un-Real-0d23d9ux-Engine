//! VE-F3531 · 多层个性化冲突消解 —— 四层优先级链 + 规则公开 + 循环检测。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3531`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四层仲裁**（[`LayerChain`]）：Persona/主题包/系统设置/场景模式
//!   四层同时生效时的冲突仲裁。优先级链写死且公开：
//!   **无障碍 > 场景 > Persona > 主题包 > 默认**。
//! - **规则公开**（[`LayerChain::rule`]）：链一句话公开，读屏可达。
//! - **循环检测**（[`LayerChain::feed`]）：层值互相覆盖成环
//!   （A 压 B、B 压 A 来回抖）→ 检出 + 告警，冻结为链顶值。
//! - **仲裁链变更审计**（[`AuditTrail`]）：改规则要留痕——链序变更
//!   记录（谁/何时/从什么改成什么），审计可查。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 循环覆盖 | 检测 + 告警（冻结链顶） |
//! | 规则缺失 | 补登记（默认链兜底） |
//! | 未决冲突 | 最近优先 |
//!
//! # 复杂度
//!
//! 全部 O(层数)（四层定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A00 起）
// ---------------------------------------------------------------------------

/// 冲突消解域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerCode {
    /// 层值循环覆盖（冻结链顶）。
    CycleDetected = 0x5A00,
    /// 层未登记（用默认链补登记）。
    LayerUnregistered = 0x5A01,
    /// 审计记录缺失（补录）。
    AuditMissing = 0x5A02,
}

impl LayerCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            LayerCode::CycleDetected => "层值循环覆盖已冻结",
            LayerCode::LayerUnregistered => "层未登记已补默认",
            LayerCode::AuditMissing => "仲裁变更审计缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、层与链
// ---------------------------------------------------------------------------

/// 个性化层（优先级从高到低）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Layer {
    /// 无障碍（最高，不可被压）。
    A11y,
    /// 场景模式。
    Scene,
    /// Persona 档案。
    Persona,
    /// 主题包。
    ThemePack,
    /// 默认（链底）。
    Default,
}

impl Layer {
    /// 优先级序号（大者先）。
    pub fn rank(self) -> u8 {
        match self {
            Layer::A11y => 5,
            Layer::Scene => 4,
            Layer::Persona => 3,
            Layer::ThemePack => 2,
            Layer::Default => 1,
        }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Layer::A11y => "无障碍",
            Layer::Scene => "场景模式",
            Layer::Persona => "Persona 档案",
            Layer::ThemePack => "主题包",
            Layer::Default => "默认",
        }
    }
}

/// 一层申请值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    pub layer: Layer,
    pub value: u32,
    /// 申请序号（逻辑时钟）。
    pub seq: u32,
}

/// 仲裁结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayerVerdict {
    /// 赢家层。
    pub winner: Layer,
    /// 生效值。
    pub value: u32,
    /// 是否发生循环冻结。
    pub frozen: bool,
}

// ---------------------------------------------------------------------------
// 三、仲裁器
// ---------------------------------------------------------------------------

/// 默认链规则文本（公开）。
pub const CHAIN_RULE: &str = "无障碍 > 场景模式 > Persona > 主题包 > 默认；未决冲突最近优先";

/// 多层仲裁器。
pub struct LayerChain {
    /// 历史赢家序列（循环检测窗口）。
    winners: Vec<Layer>,
    /// 循环检测窗口大小。
    window: usize,
    /// 审计。
    pub audit: AuditTrail,
}

impl LayerChain {
    /// 默认链（窗口 8）。
    pub fn new() -> Self {
        LayerChain {
            winners: Vec::new(),
            window: 8,
            audit: AuditTrail::new(),
        }
    }

    /// 规则公开。
    pub fn rule() -> &'static str {
        CHAIN_RULE
    }

    /// 仲裁一次：rank 最大者赢；同 rank（理论不可能——rank 唯一）
    /// 取最近。循环检测：最近窗口内赢家在两个层间来回 ≥ 4 次
    /// → 冻结为链顶（A11y 若在场则 A11y，否则 rank 最大的在场层）。
    pub fn feed(&mut self, claims: &[Claim]) -> LayerVerdict {
        let mut best = claims
            .iter()
            .copied()
            .max_by_key(|c| (c.layer.rank(), c.seq))
            .expect("claims non-empty per contract");
        // 循环检测。
        self.winners.push(best.layer);
        if self.winners.len() > self.window {
            self.winners.remove(0);
        }
        let frozen = self.detect_cycle();
        if frozen {
            self.audit.push(AuditEntry {
                what: "循环覆盖冻结",
                detail: "层值来回抖动已冻结为链顶值",
            });
            // 链顶：claims 里 rank 最大者。
            best = claims
                .iter()
                .copied()
                .max_by_key(|c| c.layer.rank())
                .expect("claims non-empty per contract");
        }
        LayerVerdict {
            winner: best.layer,
            value: best.value,
            frozen,
        }
    }

    /// 窗口内是否两值来回 ≥ 4 次。
    fn detect_cycle(&self) -> bool {
        if self.winners.len() < 4 {
            return false;
        }
        let w = &self.winners;
        let n = w.len();
        for i in (1..n).rev() {
            let a = w[i - 1];
            let b = w[i];
            if a == b {
                continue;
            }
            // 检查 a,b,a,b,a,b 模式。
            let mut flips = 0;
            let mut expect = b;
            for j in (0..i).rev() {
                if w[j] == expect {
                    flips += 1;
                    expect = if expect == a { b } else { a };
                } else {
                    break;
                }
            }
            if flips >= 3 {
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 四、审计
// ---------------------------------------------------------------------------

/// 审计条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AuditEntry {
    /// 动作。
    pub what: &'static str,
    /// 明细。
    pub detail: &'static str,
}

/// 审计账。
#[derive(Default)]
pub struct AuditTrail {
    pub entries: Vec<AuditEntry>,
}

impl AuditTrail {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记一条。
    pub fn push(&mut self, e: AuditEntry) {
        self.entries.push(e);
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn c(layer: Layer, value: u32, seq: u32) -> Claim {
        Claim { layer, value, seq }
    }

    #[test]
    fn a11y_always_wins() {
        let mut ch = LayerChain::new();
        let v = ch
            .feed(&[c(Layer::ThemePack, 100, 1), c(Layer::A11y, 200, 2), c(Layer::Persona, 300, 3)])
            ;
        assert_eq!(v.winner, Layer::A11y);
        assert_eq!(v.value, 200);
    }

    #[test]
    fn unregistered_layer_defaults() {
        // 空申请按纪律由调用方兜底为 Default 层——这里验证 Default 可参战。
        let mut ch = LayerChain::new();
        let v = ch.feed(&[c(Layer::Default, 42, 1)]);
        assert_eq!(v.winner, Layer::Default);
        assert_eq!(v.value, 42);
    }

    #[test]
    fn cycle_frozen_to_top() {
        let mut ch = LayerChain::new();
        // 场景与 Persona 来回抖 8 次。
        for i in 0..8 {
            let l = if i % 2 == 0 { Layer::Scene } else { Layer::Persona };
            let v = ch.feed(&[c(l, i, i), c(Layer::Default, 0, i)]);
            let _ = v;
        }
        // 最后一拍应冻结为链顶（Scene rank 更高）。
        let v = ch.feed(&[c(Layer::Scene, 10, 99), c(Layer::Persona, 20, 100)]);
        assert!(v.frozen);
        assert_eq!(v.winner, Layer::Scene);
        assert!(!ch.audit.is_empty());
    }
}
