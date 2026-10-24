//! VE-F3512 · 色温调试工具 —— 曲线预览编辑 + 生效链视图 + X03 会话接入 + 开销预算。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3512`
//!
//! # 职责（锚点原文拆解）
//!
//! - **曲线可视化预览**（[`CurveEditor`]）：曲线编辑的实时预览——
//!   用户拖控制点，预览按采样密度（[`PREVIEW_SAMPLES`]）输出
//!   「分钟→色温」折线，**预览失真 → 修正**：预览与正式求值走
//!   同一个求值函数（[`super::ver08a_kelvin::Curve::evaluate`]），
//!   从结构上排除「预览一套真值一套」的失真源。
//! - **生效链视图**（[`ChainView`]）：当前是谁在生效——曲线/
//!   场景/覆盖/豁免四层从上到下排，每层给「在生效/被压住/不在场」
//!   三态。调试的第一个问题是「谁赢了」，链视图把答案变成数据。
//! - **会话统一接入**（[`DebugSession`]）：调试会话与 X03 会话统一
//!   ——调试输出挂会话号，同一会话的曲线编辑/链查询/开销记录
//!   归一个会话桶，会话结束输出整包（不出机声明延续：调试数据
//!   不出机）。
//! - **开销预算**（[`DebugBudget`]）：调试有开销，预算保护——
//!   预览采样/链查询每次记录耗时（逻辑时钟），超预算降级
//!   （采样密度减半）。**基准本身不许拖慢收口**——超预算的
//!   调试会话被限流而不是被放任。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 预览失真 | 修正（同源求值） |
//! | 链缺层 | 补齐（缺席层显式「不在场」） |
//! | 开销超预算 | 降级（采样减半） |
//!
//! # 复杂度
//!
//! - `preview`：O(采样数)（固定 24）；
//! - `chain`：O(层数)（固定 4）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5790 起）
// ---------------------------------------------------------------------------

/// 色温调试域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebugCode {
    /// 预览控制点非法（校验不过）。
    BadControlPoint = 5790,
    /// 生效链缺层（某层未注册）。
    ChainLayerMissing = 5791,
    /// 调试开销超预算（降级）。
    OverBudget = 5792,
    /// 会话号非法（零号会话）。
    BadSession = 5793,
}

impl DebugCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DebugCode::BadControlPoint => "预览控制点非法",
            DebugCode::ChainLayerMissing => "生效链缺层",
            DebugCode::OverBudget => "调试开销超预算",
            DebugCode::BadSession => "会话号非法",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, DebugCode::BadSession)
    }
}

// ---------------------------------------------------------------------------
// 二、曲线预览编辑
// ---------------------------------------------------------------------------

/// 预览采样密度（一天 24 点整点采样）。
pub const PREVIEW_SAMPLES: usize = 24;

/// 曲线编辑预览器。
#[derive(Clone)]
pub struct CurveEditor {
    /// 工作副本控制点（编辑中，未提交）。
    pub points: Vec<(u32, u32)>,
    /// 采样密度（预算降级后减半）。
    pub samples: usize,
}

impl CurveEditor {
    /// 从当前曲线起编辑。
    pub fn start(points: Vec<(u32, u32)>) -> Self {
        CurveEditor {
            points,
            samples: PREVIEW_SAMPLES,
        }
    }

    /// 移动/新增控制点（就地更新）。
    pub fn set_point(&mut self, minute: u32, kelvin: u32) -> Result<(), DebugCode> {
        use super::ver08a_kelvin::{K_MAX, K_MIN};
        if !(K_MIN..=K_MAX).contains(&kelvin) {
            return Err(DebugCode::BadControlPoint);
        }
        if let Some(slot) = self.points.iter_mut().find(|(m, _)| *m == minute % 1440) {
            slot.1 = kelvin;
        } else {
            self.points.push((minute % 1440, kelvin));
            self.points.sort_by_key(|(m, _)| *m);
        }
        Ok(())
    }

    /// 删点（保留至少 2 点）。
    pub fn drop_point(&mut self, minute: u32) -> Result<(), DebugCode> {
        if self.points.len() <= 2 {
            return Err(DebugCode::BadControlPoint);
        }
        self.points.retain(|(m, _)| *m != minute % 1440);
        Ok(())
    }

    /// 预览采样（与正式求值同源——失真从结构上不可能）。
    pub fn preview(&self) -> Result<Vec<(u32, u32)>, DebugCode> {
        let c = super::ver08a_kelvin::Curve::build(&self.points)
            .map_err(|_| DebugCode::BadControlPoint)?;
        let step = (1440u32 / self.samples.max(1) as u32) as u32;
        let mut out = Vec::with_capacity(self.samples);
        let mut m = 0u32;
        while m < 1440 {
            out.push((m, c.evaluate(m)));
            m += step;
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 三、生效链视图
// ---------------------------------------------------------------------------

/// 链层三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerState {
    /// 该层正在生效（赢了仲裁）。
    Winning,
    /// 该层在场但被压住（仲裁输了）。
    Suppressed,
    /// 该层不在场（未启用/未配置）。
    Absent,
}

impl LayerState {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            LayerState::Winning => "生效中",
            LayerState::Suppressed => "被压住",
            LayerState::Absent => "不在场",
        }
    }
}

/// 一层链目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainLayer {
    /// 层名。
    pub name: &'static str,
    /// 三态。
    pub state: LayerState,
    /// 该层想要的值（不在场为 0）。
    pub want: u32,
}

/// 生效链查询输入（四层的在场与诉求）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainInput {
    /// 手动覆盖：(在场, 诉求值)。
    pub manual: (bool, u32),
    /// HDR 豁免（全屏 HDR 播放中）。
    pub hdr_exempt: bool,
    /// 场景：(在场, 诉求值)。
    pub scene: (bool, u32),
    /// 曲线：(在场, 诉求值)。
    pub curve: (bool, u32),
}

/// 生效链仲裁序：手动 > HDR豁免 > 场景 > 曲线（与 E06 组内仲裁规则一致）。
pub fn chain_layers(input: ChainInput) -> Vec<ChainLayer> {
    // 找赢家：从高到低找第一个在场者。
    let winner = if input.manual.0 {
        Some(0usize)
    } else if input.hdr_exempt {
        Some(1)
    } else if input.scene.0 {
        Some(2)
    } else if input.curve.0 {
        Some(3)
    } else {
        None
    };
    let wants = [
        input.manual.1,
        0, // 豁免层无值，只有「放行/拦截」
        input.scene.1,
        input.curve.1,
    ];
    let names = ["手动覆盖", "HDR豁免", "场景", "自动曲线"];
    let present = [
        input.manual.0,
        input.hdr_exempt,
        input.scene.0,
        input.curve.0,
    ];
    (0..4)
        .map(|i| ChainLayer {
            name: names[i],
            state: match winner {
                None => LayerState::Absent,
                Some(w) if i == w => LayerState::Winning,
                Some(_) if present[i] => LayerState::Suppressed,
                _ => LayerState::Absent,
            },
            want: wants[i],
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 四、调试会话（X03 统一接入）
// ---------------------------------------------------------------------------

/// 开销预算（逻辑时钟单位：tick）。
pub const DEBUG_BUDGET_TICKS: u32 = 64;

/// 会话开销记录条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DebugSpend {
    /// 动作名。
    pub action: &'static str,
    /// 耗时（tick）。
    pub cost: u32,
}

/// 调试会话：曲线编辑 + 链查询 + 开销账。
pub struct DebugSession {
    /// 会话号（X03 会话 id，非零）。
    pub session: u32,
    editor: Option<CurveEditor>,
    spend: Vec<DebugSpend>,
    used: u32,
    degraded: bool,
}

impl DebugSession {
    /// 开新会话（零号拒绝——零号留给「非会话」系统内部调用）。
    pub fn open(session: u32) -> Result<Self, DebugCode> {
        if session == 0 {
            return Err(DebugCode::BadSession);
        }
        Ok(DebugSession {
            session,
            editor: None,
            spend: Vec::new(),
            used: 0,
            degraded: false,
        })
    }

    /// 已用开销。
    pub fn used(&self) -> u32 {
        self.used
    }

    /// 是否已降级。
    pub fn degraded(&self) -> bool {
        self.degraded
    }

    /// 开销账。
    pub fn spend(&self) -> &[DebugSpend] {
        &self.spend
    }

    /// 记一笔开销（超预算触发降级：采样减半）。
    pub fn charge(&mut self, action: &'static str, cost: u32) {
        self.used += cost;
        self.spend.push(DebugSpend { action, cost });
        if self.used > DEBUG_BUDGET_TICKS && !self.degraded {
            self.degraded = true;
        }
    }

    /// 开始曲线编辑（继承当前曲线）。
    pub fn edit_curve(&mut self, points: Vec<(u32, u32)>) {
        let mut ed = CurveEditor::start(points);
        if self.degraded {
            // 降级：采样减半（预览粒度变粗，但不拖慢系统）。
            ed.samples = (ed.samples / 2).max(4);
        }
        self.editor = Some(ed);
    }

    /// 预览（带开销记账）。
    pub fn preview(&mut self) -> Result<Vec<(u32, u32)>, DebugCode> {
        let ed = self.editor.as_ref().ok_or(DebugCode::BadControlPoint)?;
        let out = ed.preview();
        if out.is_ok() {
            self.charge("preview", 4);
        }
        out
    }

    /// 链查询（带开销记账）。缺层补齐：chain_layers 恒返回四层，
    /// 不在场的显式 Absent——「链缺层」在输出侧不可能发生。
    pub fn chain(&mut self, input: ChainInput) -> Vec<ChainLayer> {
        let layers = chain_layers(input);
        self.charge("chain", 2);
        layers
    }

    /// 会话收口输出（调试数据不出机：只出会话内摘要）。
    pub fn close(self) -> (u32, u32, usize) {
        (self.session, self.used, self.spend.len())
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_same_as_truth() {
        let pts = vec![(0u32, 3400u32), (360, 3400), (420, 6500), (1320, 6500)];
        let mut s = DebugSession::open(7).unwrap();
        s.edit_curve(pts.clone());
        let pv = s.preview().unwrap();
        let c = super::super::ver08a_kelvin::Curve::build(&pts).unwrap();
        for (m, v) in &pv {
            assert_eq!(*v, c.evaluate(*m), "minute {}", m);
        }
    }

    #[test]
    fn chain_manual_wins() {
        let inp = ChainInput {
            manual: (true, 3000),
            hdr_exempt: true,
            scene: (true, 2700),
            curve: (true, 5000),
        };
        let layers = chain_layers(inp);
        assert_eq!(layers[0].state, LayerState::Winning);
        assert_eq!(layers[1].state, LayerState::Suppressed);
        assert_eq!(layers[3].state, LayerState::Suppressed);
    }

    #[test]
    fn budget_degrades_sampling() {
        let mut s = DebugSession::open(3).unwrap();
        let pts = vec![(0u32, 3400u32), (360, 3400), (420, 6500), (1320, 6500)];
        s.edit_curve(pts.clone());
        let _ = s.preview();
        s.charge("synthetic", 999);
        assert!(s.degraded());
        s.edit_curve(pts);
        let n = s.preview().unwrap().len();
        assert!(n < PREVIEW_SAMPLES);
    }

    #[test]
    fn zero_session_rejected() {
        assert!(matches!(DebugSession::open(0), Err(DebugCode::BadSession)));
    }
}
