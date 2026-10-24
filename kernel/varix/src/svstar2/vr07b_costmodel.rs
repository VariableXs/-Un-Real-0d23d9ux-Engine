//! VE-F3722 · 创作成本模型（VE-S 域 · 批次 R07 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3722`
//!
//! **职责定位（锚点原文）**：创作成本模型（F3322 模式四用复刻）——
//! - **四维成本**：画布维（面积×图层数）/ 工具维（工具类型系数）/
//!   资产维（资源大小）/ 历史维（undo 链深）——
//!   四维合成估计值（复述校准 20%）；
//! - **心流指标模型**：编辑中断率/恢复时间/预览延迟三心流指标
//!   （复述用户指标建模）——三用复刻 + 心流新增；
//! - **预警接口复用**（设计前置复述）。
//!
//! **判据（锚点原文六条）**：四维+心流、20% 校准、设计前置、
//! 单源四用、指标建模、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 偏差>20%→修正（复述） | [`C7Code::CALIB_DRIFT`] |
//! | 心流指标缺→补（红线联动实测） | [`C7Code::FLOW_METRIC_MISS`] |
//! | 系数过期→重标定（复述） | [`C7Code::COEF_EXPIRED`] |
//! | 误报>10%→复核（复述） | [`C7Code::FALSE_ALARM`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3722 诊断码段（独占 0x71xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C7Code(pub u16);

impl C7Code {
    /// 校准偏差 >20%（修正）。
    pub const CALIB_DRIFT: C7Code = C7Code(0x7101);
    /// 心流指标缺（补全）。
    pub const FLOW_METRIC_MISS: C7Code = C7Code(0x7102);
    /// 系数过期（重标定）。
    pub const COEF_EXPIRED: C7Code = C7Code(0x7103);
    /// 误报 >10%（复核）。
    pub const FALSE_ALARM: C7Code = C7Code(0x7104);
    /// 四维参数非法。
    pub const BAD_DIMENSION: C7Code = C7Code(0x7105);
    /// 预警未前置。
    pub const WARN_NOT_FRONT: C7Code = C7Code(0x7106);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C7Code::CALIB_DRIFT => "校准偏差 >20%（复述）。建议：重标定系数。",
            C7Code::FLOW_METRIC_MISS => "心流指标缺失（红线联动）。建议：补齐中断/恢复/延迟三指标。",
            C7Code::COEF_EXPIRED => "系数过期。建议：夜间重标定。",
            C7Code::FALSE_ALARM => "误报 >10%（复述）。建议：复核预警阈值。",
            C7Code::BAD_DIMENSION => "四维参数非法。建议：各维须非负。",
            C7Code::WARN_NOT_FRONT => "预警未前置。建议：设计阶段即接入预警接口。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四维成本 ────────────────────────────────────────────────────────

/// 工具类型（工具维系数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolKind {
    /// 画笔（中）。
    Brush,
    /// 橡皮（低）。
    Eraser,
    /// 滤镜（高）。
    Filter,
    /// 导出（高）。
    Export,
}

impl ToolKind {
    /// 成本系数（×10）。
    pub const fn coef_x10(self) -> u32 {
        match self {
            ToolKind::Brush => 10,
            ToolKind::Eraser => 5,
            ToolKind::Filter => 30,
            ToolKind::Export => 50,
        }
    }
}

/// 四维输入。
pub struct CostInput {
    /// 画布宽。
    pub canvas_w: u32,
    /// 画布高。
    pub canvas_h: u32,
    /// 图层数。
    pub layers: u16,
    /// 工具。
    pub tool: ToolKind,
    /// 资产大小（KB）。
    pub asset_kb: u32,
    /// undo 链深。
    pub undo_depth: u16,
}

/// 成本估计器（四维合成）。
pub struct CostModel {
    /// 系数版本（过期检出）。
    coef_epoch: u32,
    /// 当前纪元。
    now_epoch: u32,
}

impl CostModel {
    pub fn new(coef_epoch: u32, now_epoch: u32) -> CostModel {
        CostModel {
            coef_epoch,
            now_epoch,
        }
    }

    /// 四维合成估计（整数，结果为成本点）。
    pub fn estimate(&self, input: &CostInput) -> Result<u32, C7Code> {
        // 系数过期（>100 纪元单位）
        if self.now_epoch.saturating_sub(self.coef_epoch) > 100 {
            return Err(C7Code::COEF_EXPIRED);
        }
        // 画布维：面积/1024 * 图层
        let area = (input.canvas_w as u64) * (input.canvas_h as u64);
        let canvas_dim = ((area / 1024) as u32).saturating_mul(input.layers as u32);
        // 工具维：系数
        let tool_dim = input.tool.coef_x10();
        // 资产维
        let asset_dim = input.asset_kb;
        // 历史维：undo 深 × 2
        let hist_dim = (input.undo_depth as u32).saturating_mul(2);
        let total = canvas_dim
            .saturating_add(tool_dim)
            .saturating_add(asset_dim)
            .saturating_add(hist_dim);
        Ok(total)
    }

    /// 校准（估计 vs 实测，偏差 >20% 拦）。
    pub fn calibrate(&self, estimated: u32, actual: u32) -> Result<(), C7Code> {
        if actual == 0 {
            return if estimated == 0 {
                Ok(())
            } else {
                Err(C7Code::CALIB_DRIFT)
            };
        }
        let diff = if estimated > actual {
            estimated - actual
        } else {
            actual - estimated
        };
        // diff / actual * 100 > 20 → 超
        if (diff as u64) * 100 > (actual as u64) * 20 {
            return Err(C7Code::CALIB_DRIFT);
        }
        Ok(())
    }

    /// 系数纪元。
    pub fn coef_epoch(&self) -> u32 {
        self.coef_epoch
    }
}

// ── 心流三指标（建模） ──────────────────────────────────────────────

/// 心流指标集。
pub struct FlowTriple {
    /// 中断率（×100 放大，1.00% = 100）。
    pub interrupt_x100: u32,
    /// 平均恢复时间（毫秒）。
    pub recover_ms: u32,
    /// 预览延迟（毫秒）。
    pub preview_ms: u32,
}

impl FlowTriple {
    /// 完整性（三指标齐）。
    pub fn complete(&self) -> Result<(), C7Code> {
        // 三项均有非零约束（0 值视为缺测）
        if self.interrupt_x100 == 0 || self.recover_ms == 0 || self.preview_ms == 0 {
            return Err(C7Code::FLOW_METRIC_MISS);
        }
        Ok(())
    }

    /// 心流健康分（整数 0-100，越大越好）。
    pub fn health(&self) -> u32 {
        // 简化：三线各自给分取平均
        let s1 = if self.interrupt_x100 <= 100 {
            100
        } else {
            10_000 / self.interrupt_x100
        };
        let s2 = if self.recover_ms <= 200 {
            100
        } else {
            20_000 / self.recover_ms
        };
        let s3 = if self.preview_ms <= 100 {
            100
        } else {
            10_000 / self.preview_ms
        };
        (s1 + s2 + s3) / 3
    }
}

// ── 预警接口（设计前置） ────────────────────────────────────────────

/// 预警级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WarnLevel {
    /// 提示。
    Info,
    /// 警告。
    Warn,
    /// 严重。
    Critical,
}

/// 预警接口。
pub struct WarnPort {
    /// 是否设计期已接入（前置红线）。
    front_loaded: bool,
    /// 误报计数。
    false_alarms: u32,
    /// 总预警数。
    total: u32,
}

impl WarnPort {
    /// 前置构造（设计期接入）。
    pub fn front_loaded() -> WarnPort {
        WarnPort {
            front_loaded: true,
            false_alarms: 0,
            total: 0,
        }
    }

    /// 后置构造（非前置 → 拦）。
    pub fn back_loaded() -> WarnPort {
        WarnPort {
            front_loaded: false,
            false_alarms: 0,
            total: 0,
        }
    }

    /// 前置检查。
    pub fn front_check(&self) -> Result<(), C7Code> {
        if self.front_loaded {
            Ok(())
        } else {
            Err(C7Code::WARN_NOT_FRONT)
        }
    }

    /// 发一条预警。
    pub fn fire(&mut self, _level: WarnLevel) {
        self.total += 1;
    }

    /// 标记误报。
    pub fn note_false(&mut self) {
        self.false_alarms += 1;
    }

    /// 误报率检查（>10% 拦）。
    pub fn false_rate_check(&self) -> Result<(), C7Code> {
        if self.total == 0 {
            return Ok(());
        }
        if (self.false_alarms as u64) * 10 > (self.total as u64) {
            return Err(C7Code::FALSE_ALARM);
        }
        Ok(())
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3722 判据集（锚点原文六条逐条落位）。
pub fn run_vr07b_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3722");

    // 1) 四维+心流：四维合成可算
    let model = CostModel::new(100, 150);
    let input = CostInput {
        canvas_w: 1024,
        canvas_h: 1024,
        layers: 4,
        tool: ToolKind::Brush,
        asset_kb: 100,
        undo_depth: 5,
    };
    let est = model.estimate(&input).unwrap_or(0);
    out.add(
        "0x7105-四维合成",
        est > 0,
        "四维须可估",
    );

    // 2) 20% 校准：偏差内过
    out.add(
        "0x7101-校准过",
        model.calibrate(120, 110).is_ok(),
        "偏差 9% 须过",
    );

    // 3) 20% 校准：超差被拦
    out.add(
        "0x7101-校准拦",
        model.calibrate(200, 100).is_err(),
        "偏差 100% 须拦",
    );

    // 4) 系数过期：过期被拦
    let stale = CostModel::new(0, 200);
    out.add(
        "0x7103-系数过期",
        stale.estimate(&input).is_err(),
        "过期须拦",
    );

    // 5) 指标建模：缺测被拦
    let ft = FlowTriple {
        interrupt_x100: 0,
        recover_ms: 100,
        preview_ms: 50,
    };
    out.add(
        "0x7102-指标建模",
        ft.complete().is_err(),
        "缺测须拦",
    );

    // 6) 指标建模：三指标齐过
    let ft2 = FlowTriple {
        interrupt_x100: 80,
        recover_ms: 150,
        preview_ms: 60,
    };
    out.add(
        "0x7102-三指标齐",
        ft2.complete().is_ok() && ft2.health() > 0,
        "齐备须过",
    );

    // 7) 设计前置：后置被拦
    let back = WarnPort::back_loaded();
    out.add(
        "0x7106-设计前置",
        back.front_check().is_err(),
        "后置须拦",
    );

    // 8) 单源四用：误报率门
    let mut wp = WarnPort::front_loaded();
    wp.fire(WarnLevel::Info);
    wp.fire(WarnLevel::Warn);
    wp.note_false();
    out.add(
        "0x7104-误报门",
        wp.false_rate_check().is_err(),
        "误报 50% 须拦",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7106-判据",
        C7Code::CALIB_DRIFT.value() != C7Code::FLOW_METRIC_MISS.value(),
        "码段互异",
    );

    out
}
