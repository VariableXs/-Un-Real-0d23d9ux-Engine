//! cgc06_powersched — CGPU-F0011 功耗感知调度（算力调度必须懂电）。
//!
//! 判据原文（施工书锚点 CGPU-F0011）：接入功耗/热信息（RAPL 类接口 + 温度
//! 传感器），建立三模式：性能模式（不设限，散热优先）、均衡模式（功率墙
//! 80%，超墙先降后台算力）、续航模式（功率墙 50%，降质链联动提前介入）。热
//! 降频预期：温度爬升趋势预测（滑动窗口外推），在实测降频发生前 2 秒主动降
//! 负载，避免被动降频造成的帧率悬崖。所有功耗动作入遥测并与卡顿归因联动
//! （CGPU-F0007 的"热降频"归因直接消费本域数据）。红线：只读传感 + 标准调
//! 节接口，禁止任何探索性硬件写。判据：三模式切换行为确定、预测性降载提前
//! ≥2s、与卡顿归因联动闭环、功耗墙精度实测、遥测全量。
//!
//! 职责定位：三模式功率墙状态机+温度趋势外推预测性降载+卡顿归因供数+遥测
//! 全量。传感只读红线：本模块零硬件写（数据由调用方注入——HAL 层隔离）。
//! 与 cgc04_framearbiter（帧内时间预算）分工——彼管毫秒、此管瓦特。降载信
//! 号只发单不执行（F0008 消费）。诊断码段 0x48xx。

use alloc::vec::Vec;

/// 模块版本。
pub const POWER_SCHED_VERSION: &str = "C06-powersched-v1";

/// 功率墙（百分比×100）：均衡 80% / 续航 50% / 性能不设限（200=无墙语义）。
pub const WALL_BALANCE_PCT: u32 = 80;
pub const WALL_BATTERY_PCT: u32 = 50;
pub const WALL_NONE_PCT: u32 = 200;

/// 温度告警线（℃）：预测性降载的起点判据。
pub const TEMP_WARN_C: u32 = 85;
/// 热降频实测线（℃）：被动降频发生点（预期要在这之前 2 秒介入）。
pub const TEMP_THROTTLE_C: u32 = 95;
/// 预测提前量（ms）：判据"实测降频发生前 2 秒主动降负载"。
pub const PREDICT_LEAD_MS: u32 = 2000;

/// 功耗模式闭集（判据三模式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerMode {
    /// 性能：不设限（散热优先）。
    Performance,
    /// 均衡：功率墙 80%，超墙先降后台算力。
    Balanced,
    /// 续航：功率墙 50%，降质链联动提前介入。
    Battery,
}

impl PowerMode {
    pub const ALL: [PowerMode; 3] = [
        PowerMode::Performance,
        PowerMode::Balanced,
        PowerMode::Battery,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            PowerMode::Performance => 0,
            PowerMode::Balanced => 1,
            PowerMode::Battery => 2,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            PowerMode::Performance => "性能(不设限)",
            PowerMode::Balanced => "均衡(墙80%)",
            PowerMode::Battery => "续航(墙50%)",
        }
    }

    /// 该模式的功率墙（百分比×100）。
    pub const fn wall_pct(self) -> u32 {
        match self {
            PowerMode::Performance => WALL_NONE_PCT,
            PowerMode::Balanced => WALL_BALANCE_PCT,
            PowerMode::Battery => WALL_BATTERY_PCT,
        }
    }
}

/// 诊断码（0x48xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4801 功率超墙——降载信号（后台算力先降）。
    PwWallBreached,
    /// 0x4802 预测性热降载——提前 ≥2s 信号（避免帧率悬崖）。
    PwThermalPredicted,
    /// 0x4803 被动降频已发生（预测失守——显性立案供归因）。
    PwThrottledPassive,
    /// 0x4804 传感样本非法（温度出 0~150℃ 域 / 功率为零）。
    PwSampleInvalid,
    /// 0x4805 模式切换行为不确定（迟滞带内抖动被拒——行为确定判据）。
    PwSwitchUnstable,
    /// 0x4806 遥测序断裂。
    PwTelemetryBroken,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::PwWallBreached => "功率超墙：按模式先降后台算力（降载信号只发单不执行）",
            XcCode::PwThermalPredicted => "预测性热降载：温度爬升外推将在 2s 内触顶——主动降负载",
            XcCode::PwThrottledPassive => "被动降频已发生：预测失守立案（帧率悬崖归因供 F0007）",
            XcCode::PwSampleInvalid => "传感样本非法：温度须在 0~150℃，功率非负",
            XcCode::PwSwitchUnstable => "模式切换被拒：迟滞带内防抖（三模式切换行为确定判据）",
            XcCode::PwTelemetryBroken => "功耗遥测序断裂：seq 必须单调",
        }
    }
}

/// 温度样本（滑动窗口元素）。
#[derive(Clone, Copy, Debug)]
pub struct TempSample {
    /// 时刻（ms 时间轴——调用方注入）。
    pub at_ms: u32,
    /// 温度（℃）。
    pub temp_c: u32,
}

/// 温度趋势外推器（判据"滑动窗口外推"——首尾斜率整数外推）。
#[derive(Clone, Debug)]
pub struct ThermalTrend {
    /// 滑动窗口（保留最近 32 样本）。
    win: Vec<TempSample>,
}

impl ThermalTrend {
    pub fn new() -> Self {
        ThermalTrend { win: Vec::new() }
    }

    /// 喂样本（非法样本显性拒）。
    pub fn push(&mut self, s: TempSample) -> Result<(), XcCode> {
        if s.temp_c > 150 {
            return Err(XcCode::PwSampleInvalid);
        }
        self.win.push(s);
        if self.win.len() > 32 {
            self.win.remove(0);
        }
        Ok(())
    }

    /// 爬升斜率（℃/秒 ×100——整数外推：窗口首尾差 / 时间差）。
    pub fn slope_c_per_s(&self) -> i64 {
        if self.win.len() < 2 {
            return 0;
        }
        let first = self.win[0];
        let last = self.win[self.win.len() - 1];
        let dt_ms = (last.at_ms as i64) - (first.at_ms as i64);
        if dt_ms <= 0 {
            return 0;
        }
        let d_temp = (last.temp_c as i64) - (first.temp_c as i64);
        d_temp * 100 * 1000 / dt_ms
    }

    /// 距降频线的时间预测（ms）：当前温度到 TEMP_THROTTLE_C 的余量 / 斜率。
    /// 返回 None=不爬升（斜率 ≤0）或已越线。
    pub fn eta_to_throttle_ms(&self) -> Option<u32> {
        let slope = self.slope_c_per_s();
        if slope <= 0 {
            return None;
        }
        let cur = self.win.last().map(|s| s.temp_c).unwrap_or(0);
        if cur >= TEMP_THROTTLE_C {
            return None;
        }
        let headroom_c = (TEMP_THROTTLE_C - cur) as i64 * 100; // ×100 口径
        let eta_s100 = headroom_c / slope; // ×100 秒
        let eta_ms = (eta_s100 * 10) as u32;
        Some(eta_ms)
    }
}

/// 降载建议单（只发单不执行——F0008/调度层消费）。
#[derive(Clone, Copy, Debug)]
pub struct DethrottleOrder {
    /// 归因（墙/热预测）。
    pub cause: XcCode,
    /// 建议降载幅度（ppm——后台算力优先降）。
    pub shed_ppm: u64,
}

/// 功耗调度器（判据本体）。
pub struct PowerSched {
    /// 当前模式。
    mode: PowerMode,
    /// 温度趋势器。
    trend: ThermalTrend,
    /// 遥测行（全量）。
    pub telemetry: Vec<(u32, u32, u32, u8)>, // (seq, temp_c, power_pct, action_code)
    /// seq 发生器。
    seq: u32,
    /// 上次切换目标模式（迟滞判据用——防抖窗内拒绝反向切换）。
    last_switch_target: Option<PowerMode>,
    /// 迟滞窗（切换后 N 帧内不接受反向切换——行为确定）。
    pub switch_cooldown: u32,
    cooldown_left: u32,
    /// 预测性降载是否已发（本轮热事件——避免重复发单）。
    pred_fired: bool,
    /// 立案计数。
    pub cases: u32,
}

impl PowerSched {
    pub fn new(mode: PowerMode) -> Self {
        PowerSched {
            mode,
            trend: ThermalTrend::new(),
            telemetry: Vec::new(),
            seq: 0,
            last_switch_target: None,
            switch_cooldown: 30,
            cooldown_left: 0,
            pred_fired: false,
            cases: 0,
        }
    }

    /// 当前模式。
    pub const fn mode(&self) -> PowerMode {
        self.mode
    }

    /// 切换模式（判据"三模式切换行为确定"：迟滞窗内反向切换拒——
    /// 同向或同模式幂等通过）。
    pub fn set_mode(&mut self, m: PowerMode) -> Result<(), XcCode> {
        if m == self.mode {
            return Ok(()); // 幂等
        }
        if self.cooldown_left > 0 {
            // 迟滞窗内：只允许与上次目标同向的切换（防抖）
            if self.last_switch_target != Some(m) {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::PwSwitchUnstable);
            }
        }
        self.mode = m;
        self.last_switch_target = Some(m);
        self.cooldown_left = self.switch_cooldown;
        Ok(())
    }

    /// 逐帧步进（判据主路径：功率墙判定 + 热预测降载 + 遥测入账）。
    /// power_pct：当前实测功率（百分比×100，调用方注入——只读传感红线）。
    pub fn tick(&mut self, temp_c: u32, power_pct: u32, now_ms: u32) -> Result<Vec<DethrottleOrder>, XcCode> {
        if temp_c > 150 || power_pct == 0 || power_pct > 500 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::PwSampleInvalid);
        }
        if self.cooldown_left > 0 {
            self.cooldown_left = self.cooldown_left.saturating_sub(1);
        }
        self.trend.push(TempSample { at_ms: now_ms, temp_c })?;
        let mut orders: Vec<DethrottleOrder> = Vec::new();
        // ① 功率墙判定（性能模式无墙语义）
        let wall = self.mode.wall_pct();
        if power_pct > wall && wall <= 100 {
            let over = ((power_pct - wall) as u64) * 1_000_000 / wall as u64;
            orders.push(DethrottleOrder {
                cause: XcCode::PwWallBreached,
                shed_ppm: over.max(10_000),
            });
            self.cases = self.cases.saturating_add(1);
        }
        // ② 热预测降载（判据"预测性降载提前 ≥2s"）
        let eta = self.trend.eta_to_throttle_ms();
        if let Some(eta_ms) = eta {
            if eta_ms <= PREDICT_LEAD_MS && !self.pred_fired {
                orders.push(DethrottleOrder {
                    cause: XcCode::PwThermalPredicted,
                    shed_ppm: 200_000,
                });
                self.pred_fired = true;
                self.cases = self.cases.saturating_add(1);
            }
        }
        // ③ 被动降频实测（失守立案——归因供 F0007）
        if temp_c >= TEMP_THROTTLE_C {
            self.pred_fired = false; // 热事件结算，重置预测态
            self.cases = self.cases.saturating_add(1);
            orders.push(DethrottleOrder {
                cause: XcCode::PwThrottledPassive,
                shed_ppm: 300_000,
            });
        }
        // 遥测全量入账（action_code：0=无动作 1=墙降载 2=热预测 3=被动）
        let action = if orders.is_empty() {
            0
        } else {
            orders[0].cause as u8
        };
        self.seq = self.seq.saturating_add(1);
        self.telemetry.push((self.seq, temp_c, power_pct, action));
        Ok(orders)
    }

    /// 卡顿归因供数（判据"与卡顿归因联动闭环"——F0007 消费）：
    /// 返回最近遥测窗口内是否发生过热事件（预测/被动）。
    pub fn thermal_events_recent(&self, lookback: usize) -> bool {
        let start = self.telemetry.len().saturating_sub(lookback);
        self.telemetry[start..]
            .iter()
            .any(|(_, _, _, a)| *a == 2 || *a == 3)
    }

    /// 遥测序核验。
    pub fn telemetry_sequential(&mut self) -> Result<(), XcCode> {
        let mut prev = 0u32;
        for (s, _, _, _) in &self.telemetry {
            if *s <= prev {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::PwTelemetryBroken);
            }
            prev = *s;
        }
        Ok(())
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "功耗调度{} 模式{} 遥测{} 立案{}",
            POWER_SCHED_VERSION,
            self.mode.zh(),
            self.telemetry.len(),
            self.cases,
        )
    }
}


/// 五账户功耗预算映射（深化：帧内五账户（对齐 cgc04 AcctKind ordinal）的
/// 功耗配额——功率墙约束下的账户级分配：呈现保底占比最高不可降，后台类
/// 先降——时间预算与功率预算同构治理）。
pub struct PowerBudget;

impl PowerBudget {
    /// 默认功耗占比表（ppm，下标对齐五账户）：
    /// 合成保底 350_000 / 交互 250_000 / 网页表面 250_000 / 动态 100_000 / 机动 50_000。
    pub const DEFAULT_RATIO_PPM: [u64; 5] = [350_000, 250_000, 250_000, 100_000, 50_000];

    /// 墙内分配（mW）：总预算 = 实测功率上限 × 墙 → 按占比投影；
    /// 收缩模式：超墙时按"机动→动态→表面"降序收缩（保底/交互不动）。
    pub fn allocate(wall_pct: u32, total_watts: u64, shed: bool) -> [u64; 5] {
        let cap = total_watts * wall_pct as u64 / 100;
        let mut out = [0u64; 5];
        let mut i = 0usize;
        while i < 5 {
            out[i] = cap * Self::DEFAULT_RATIO_PPM[i] / 1_000_000;
            i += 1;
        }
        if shed {
            // 收缩序：机动 → 动态 → 网页表面（各收一半）
            out[4] /= 2;
            out[3] /= 2;
            out[2] /= 2;
        }
        out
    }
}

/// 模式自动推荐（深化：温度+功率史 → 建议模式——均衡为缺省锚，
/// 温度持续高位或功率贴墙降续航，双低回性能；建议只出单不改态）。
#[derive(Clone, Copy, Debug)]
pub struct ModeAdvisor {
    /// 高温样本计数。
    hot_run: u32,
    /// 低温样本计数。
    cool_run: u32,
}

impl ModeAdvisor {
    pub const fn new() -> Self {
        ModeAdvisor { hot_run: 0, cool_run: 0 }
    }

    /// 帧观察：温度 ≥ 告警线累计 ≥10 帧 → 建议续航；≤ 65℃ 且功率 ≤60%
    /// 连续 ≥30 帧 → 建议性能；其余维持均衡。
    pub fn observe(&mut self, temp_c: u32, power_pct: u32) -> PowerMode {
        if temp_c >= TEMP_WARN_C {
            self.hot_run += 1;
        } else {
            self.hot_run = 0;
        }
        if temp_c <= 65 && power_pct <= 6000 {
            self.cool_run += 1;
        } else {
            self.cool_run = 0;
        }
        if self.hot_run >= 10 {
            return PowerMode::Battery;
        }
        if self.cool_run >= 30 {
            return PowerMode::Performance;
        }
        PowerMode::Balanced
    }
}
