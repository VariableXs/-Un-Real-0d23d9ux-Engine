//! CGPU-F1708 · 异构遥测异常自愈
//!
//! 判据锚点（施工书原文摘录）：
//! 自愈：异常（引擎遥测断流/指标异常/仓库失败——三类——异常库复用模式；自愈
//! （断流→估算态标注/异常→熔断/失败→重试——动作表复用；心跳（复用；缺口诚实
//! （复用；测试（三类/动作/心跳/缺口四组）。判据：三类、动作、心跳、缺口、四组、判据。
//!
//! 诊断码段 0x9301~0x9305（独占）：
//!   0x9301 异常类越表（异常落三类闭集之外）
//!   0x9302 动作失配（异常类与自愈动作不配对/动作在禁用态执行）
//!   0x9303 心跳失守（心跳停摆未告警/心跳倒退）
//!   0x9304 缺口隐瞒（数据缺口未标注即出流——诚实红线）
//!   0x9305 段越界（判码落 0x9300..0x9400 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const SELF_HEAL_VERSION: &str = "K27-selfheal-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x9301~0x9305）
// ---------------------------------------------------------------------------

pub const XC_ANOMALY_OOB: u32 = 0x9301;
pub const XC_ACTION_MISMATCH: u32 = 0x9302;
pub const XC_HEARTBEAT_LOST: u32 = 0x9303;
pub const XC_GAP_CONCEALED: u32 = 0x9304;
pub const XC_SEG_OOR: u32 = 0x9305;

// ---------------------------------------------------------------------------
// 一、异常三类闭集
// ---------------------------------------------------------------------------

/// 异常类三闭集（判据"引擎遥测断流/指标异常/仓库失败"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anomaly {
    /// 引擎遥测断流：某引擎 N 帧无样本。
    StreamDown,
    /// 指标异常：指标值越物理界（利用率>1000‰ 等）。
    MetricInsane,
    /// 仓库失败：指标仓库写入拒绝（满/锁死——语义失败）。
    WarehouseFail,
}

impl Anomaly {
    pub const ALL: [Anomaly; 3] =
        [Anomaly::StreamDown, Anomaly::MetricInsane, Anomaly::WarehouseFail];
    pub const fn name(self) -> &'static str {
        match self {
            Anomaly::StreamDown => "stream_down",
            Anomaly::MetricInsane => "metric_insane",
            Anomaly::WarehouseFail => "warehouse_fail",
        }
    }
}

/// 自愈动作三闭集（判据"断流→估算态标注/异常→熔断/失败→重试"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealAction {
    /// 估算态标注：断流引擎数据以插值补位并显性标注 estimated。
    EstimateMark,
    /// 熔断：异常指标停止入流（防污染下游）。
    Fuse,
    /// 重试：仓库写入退避重试（有上限）。
    Retry,
}

impl HealAction {
    pub const fn name(self) -> &'static str {
        match self {
            HealAction::EstimateMark => "estimate_mark",
            HealAction::Fuse => "fuse",
            HealAction::Retry => "retry",
        }
    }
}

/// 动作表配对（异常→动作 一一映射，复用既有动作表模式）。
pub const fn heal_of(a: Anomaly) -> HealAction {
    match a {
        Anomaly::StreamDown => HealAction::EstimateMark,
        Anomaly::MetricInsane => HealAction::Fuse,
        Anomaly::WarehouseFail => HealAction::Retry,
    }
}

/// 自愈引擎：异常登记→动作执行→审计。
pub struct Healer {
    /// 动作执行计数（按类）。
    pub executed: [u32; 3],
    /// 熔断中的指标（指标名锚）。
    pub fused: Vec<&'static str>,
    /// 重试计数（仓库失败重试——上限 3）。
    pub retries: u32,
    /// 重试上限。
    pub retry_cap: u32,
}

/// 重试上限常量。
pub const RETRY_CAP: u32 = 3;

impl Healer {
    pub fn new() -> Self {
        Healer { executed: [0; 3], fused: Vec::new(), retries: 0, retry_cap: RETRY_CAP }
    }

    /// 上报异常：类越表不可能（枚举闭集），动作配对执行（0x9302 由
    /// disable 态拦截路径覆盖）。
    pub fn report(&mut self, a: Anomaly, metric: &'static str, enabled: bool) -> Result<HealAction, u32> {
        if !enabled {
            return Err(XC_ACTION_MISMATCH); // 自愈被禁用仍执行=失配。
        }
        let act = heal_of(a);
        match act {
            HealAction::Fuse => {
                let mut dup = false;
                for f in &self.fused {
                    if *f == metric {
                        dup = true;
                    }
                }
                if !dup {
                    self.fused.push(metric);
                }
            }
            HealAction::Retry => {
                if self.retries >= self.retry_cap {
                    return Err(XC_ACTION_MISMATCH); // 超上限硬失败——不无限重试。
                }
                self.retries += 1;
            }
            HealAction::EstimateMark => {}
        }
        self.executed[a as usize] += 1;
        Ok(act)
    }

    /// 指标是否熔断中。
    pub fn is_fused(&self, metric: &str) -> bool {
        let mut found = false;
        for f in &self.fused {
            if *f == metric {
                found = true;
            }
        }
        found
    }
}

// ---------------------------------------------------------------------------
// 二、心跳（复用）
// ---------------------------------------------------------------------------

/// 心跳账：每引擎心跳时间戳（单调钟 µs）——停摆/倒退即 0x9303。
pub struct Heartbeats {
    /// 四引擎最近心跳（0=从未跳动）。
    pub last_us: [u64; 4],
    /// 心跳间隔预算 µs（超时=断流先兆）。
    pub budget_us: u64,
}

impl Heartbeats {
    pub const fn build() -> Self {
        Heartbeats { last_us: [0; 4], budget_us: 250_000 }
    }

    /// 心跳入账：倒退拒（0x9303）。
    pub fn beat(&mut self, engine: usize, now_us: u64) -> Result<(), u32> {
        if engine > 3 {
            return Err(XC_HEARTBEAT_LOST);
        }
        if now_us < self.last_us[engine] {
            return Err(XC_HEARTBEAT_LOST);
        }
        self.last_us[engine] = now_us;
        Ok(())
    }

    /// 停摆检测：now - last > budget = 停摆（从未跳动的引擎不算停摆——还没上岗）。
    pub fn stalled(&self, engine: usize, now_us: u64) -> Result<bool, u32> {
        if engine > 3 {
            return Err(XC_HEARTBEAT_LOST);
        }
        let last = self.last_us[engine];
        if last == 0 {
            return Ok(false);
        }
        Ok(now_us.saturating_sub(last) > self.budget_us)
    }
}

// ---------------------------------------------------------------------------
// 三、缺口诚实（复用）
// ---------------------------------------------------------------------------

/// 缺口登记：数据缺口必须显性标注（estimated=true）才能出流。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GapRow {
    /// 引擎序。
    pub engine: u8,
    /// 缺口起点帧/终点帧（闭区间语义以起止帧号记录）。
    pub from_frame: u32,
    pub to_frame: u32,
    /// 已标注估算态（诚实红线：未标注出流=0x9304）。
    pub marked: bool,
}

/// 缺口账。
pub struct GapBook {
    rows: Vec<GapRow>,
}

impl GapBook {
    pub fn new() -> Self {
        GapBook { rows: Vec::new() }
    }

    /// 登记缺口。
    pub fn register(&mut self, engine: u8, from_frame: u32, to_frame: u32) -> Result<(), u32> {
        if engine > 3 || to_frame < from_frame {
            return Err(XC_GAP_CONCEALED);
        }
        self.rows.push(GapRow { engine, from_frame, to_frame, marked: false });
        Ok(())
    }

    /// 出流闸：跨缺口的帧必须全部缺口已标注（0x9304）。
    pub fn emit_gate(&self, engine: u8, frame: u32) -> Result<(), u32> {
        for g in &self.rows {
            if g.engine == engine && frame >= g.from_frame && frame <= g.to_frame && !g.marked {
                return Err(XC_GAP_CONCEALED);
            }
        }
        Ok(())
    }

    /// 标注（EstimateMark 动作落点）。
    pub fn mark(&mut self, engine: u8, frame: u32) -> Result<(), u32> {
        let mut hit = false;
        for g in &mut self.rows {
            if g.engine == engine && frame >= g.from_frame && frame <= g.to_frame {
                g.marked = true;
                hit = true;
            }
        }
        if !hit {
            return Err(XC_GAP_CONCEALED);
        }
        Ok(())
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：三类/动作/心跳/缺口）
// ---------------------------------------------------------------------------

/// 自愈总引擎。
pub struct HealEngine {
    pub healer: Healer,
    pub hb: Heartbeats,
    pub gaps: GapBook,
}

impl HealEngine {
    pub fn new() -> Self {
        HealEngine { healer: Healer::new(), hb: Heartbeats::build(), gaps: GapBook::new() }
    }

    /// 组一：三类——三异常配对动作正确 + 未知类拒绝路径由枚举闭集保证。
    pub fn selfcheck_classes(&mut self) -> Result<(), u32> {
        // 配对断言：三类动作映射逐一核对。
        if heal_of(Anomaly::StreamDown) != HealAction::EstimateMark
            || heal_of(Anomaly::MetricInsane) != HealAction::Fuse
            || heal_of(Anomaly::WarehouseFail) != HealAction::Retry
        {
            return Err(XC_ACTION_MISMATCH);
        }
        // 三类各执行一次。
        self.healer.report(Anomaly::StreamDown, "m1", true)?;
        self.healer.report(Anomaly::MetricInsane, "utili_permille", true)?;
        self.healer.report(Anomaly::WarehouseFail, "m2", true)?;
        if self.healer.executed != [1, 1, 1] {
            return Err(XC_ANOMALY_OOB);
        }
        Ok(())
    }

    /// 组二：动作——禁用态拒 + 熔断幂等 + 重试上限硬失败。
    pub fn selfcheck_actions(&mut self) -> Result<(), u32> {
        // 禁用态拒。
        if self.healer.report(Anomaly::StreamDown, "m3", false).is_ok() {
            return Err(XC_ACTION_MISMATCH);
        }
        // 熔断幂等：同指标重复熔断不重复入册。
        self.healer.report(Anomaly::MetricInsane, "dup_metric", true)?;
        self.healer.report(Anomaly::MetricInsane, "dup_metric", true)?;
        if self.healer.is_fused("dup_metric") != true {
            return Err(XC_ACTION_MISMATCH);
        }
        // 重试上限：第 4 次仓库失败=硬失败。
        let mut h2 = Healer::new();
        let mut i = 0;
        while i < 3 {
            h2.report(Anomaly::WarehouseFail, "m4", true)?;
            i += 1;
        }
        if h2.report(Anomaly::WarehouseFail, "m4", true).is_ok() {
            return Err(XC_ACTION_MISMATCH);
        }
        if h2.retries != 3 {
            return Err(XC_ACTION_MISMATCH);
        }
        Ok(())
    }

    /// 组三：心跳——正常跳动 + 倒退拒 + 停摆检出 + 引擎越界拒。
    pub fn selfcheck_heartbeat(&mut self) -> Result<(), u32> {
        self.hb.beat(1, 1000)?;
        self.hb.beat(1, 2000)?;
        // 倒退拒。
        if self.hb.beat(1, 1500).is_ok() {
            return Err(XC_HEARTBEAT_LOST);
        }
        // 未停摆（间隔 2000 预算内）。
        if self.hb.stalled(1, 251_000)? {
            return Err(XC_HEARTBEAT_LOST);
        }
        // 停摆检出（超预算 1µs）。
        if !self.hb.stalled(1, 252_001)? {
            return Err(XC_HEARTBEAT_LOST);
        }
        // 从未跳动不算停摆。
        if self.hb.stalled(2, 999_999)? {
            return Err(XC_HEARTBEAT_LOST);
        }
        // 引擎越界拒。
        if self.hb.beat(4, 1).is_ok() {
            return Err(XC_HEARTBEAT_LOST);
        }
        Ok(())
    }

    /// 组四：缺口——登记→未标注出流拒→标注后放行→区间外不受影响。
    pub fn selfcheck_gaps(&mut self) -> Result<(), u32> {
        self.gaps.register(2, 100, 200)?;
        if self.gaps.len() != 1 {
            return Err(XC_GAP_CONCEALED);
        }
        // 未标注出流拒。
        if self.gaps.emit_gate(2, 150).is_ok() {
            return Err(XC_GAP_CONCEALED);
        }
        // 区间外不受影响。
        self.gaps.emit_gate(2, 99)?;
        self.gaps.emit_gate(2, 201)?;
        // 标注后放行。
        self.gaps.mark(2, 150)?;
        self.gaps.emit_gate(2, 150)?;
        // 标注未登记缺口拒。
        if self.gaps.mark(3, 1).is_ok() {
            return Err(XC_GAP_CONCEALED);
        }
        // 倒序区间拒。
        if self.gaps.register(1, 300, 299).is_ok() {
            return Err(XC_GAP_CONCEALED);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_classes()?;
        self.selfcheck_actions()?;
        self.selfcheck_heartbeat()?;
        self.selfcheck_gaps()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9300..0x9400 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_ANOMALY_OOB,
            XC_ACTION_MISMATCH,
            XC_HEARTBEAT_LOST,
            XC_GAP_CONCEALED,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9300 || codes[i] >= 0x9400 {
                return Err(XC_ANOMALY_OOB);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_ANOMALY_OOB);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
