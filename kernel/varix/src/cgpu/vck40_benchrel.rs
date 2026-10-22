//! CGPU-F1721 · 异构基准发布
//!
//! 判据锚点（施工书原文摘录）：
//! 基准：基准套件（异构基准（CGPU-Bench 异构段（推理/协同/能效基准——基准定义
//! （可复现（环境标注复用——复现规范；发布（基准结果发布制度（透明——发布纪律
//! （数据诚实复用；对比（与行业对比口径（诚实对比（不挑基准——对比诚实红线；
//! 测试（定义/复现/发布/对比四组）。判据：基准定义、复现、发布、诚实对比、四组、判据。
//!
//! 诊断码段 0xA401~0xA405（独占）：
//!   0xA401 基准定义缺项（三基准任一未定义/负载指标缺）
//!   0xA402 复现失范（环境标注四要素缺一）
//!   0xA403 发布失信（原始数据不在册/修订不留痕）
//!   0xA404 对比挑基准（选择性省略/对手数据不自报）
//!   0xA405 段越界（判码落 0xA400..0xA500 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const BENCH_PUB_VERSION: &str = "K40-bench-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA401~0xA405）
// ---------------------------------------------------------------------------

pub const XC_BM_DEF_MISSING: u32 = 0xA401;
pub const XC_BM_NOT_REPRODUCIBLE: u32 = 0xA402;
pub const XC_BM_PUBLISH_DISHONEST: u32 = 0xA403;
pub const XC_BM_COMPARE_CHERRY: u32 = 0xA404;
pub const XC_BM_SEG_OOR: u32 = 0xA405;

// ---------------------------------------------------------------------------
// 一、基准套件定义（CGPU-Bench 异构段：推理/协同/能效）
// ---------------------------------------------------------------------------

/// 基准类别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchKind {
    /// 推理基准：NPU/iGPU 单引擎吞吐与时延。
    Infer,
    /// 协同基准：多引擎切分端到端。
    CoWork,
    /// 能效基准：每瓦性能（J07 口径复用）。
    Efficiency,
}

impl BenchKind {
    pub const ALL: [BenchKind; 3] =
        [BenchKind::Infer, BenchKind::CoWork, BenchKind::Efficiency];

    pub const fn name(self) -> &'static str {
        match self {
            BenchKind::Infer => "infer",
            BenchKind::CoWork => "cowork",
            BenchKind::Efficiency => "efficiency",
        }
    }
}

/// 单条基准定义：类别/工作负载/主指标/迭代数（可复现下界 1000 迭代）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BenchDef {
    pub kind: BenchKind,
    /// 工作负载标识（模型/流水名）。
    pub workload: &'static str,
    /// 主指标名（tokens/s、帧/转、perf/W）。
    pub metric: &'static str,
    /// 迭代数（下界 1000——短跑不入基准）。
    pub iters: u32,
}

impl BenchDef {
    /// 定义齐判：负载/指标非空 + 迭代达下界。
    pub fn sound(&self) -> Result<(), u32> {
        if self.workload.is_empty() || self.metric.is_empty() {
            return Err(XC_BM_DEF_MISSING);
        }
        if self.iters < 1000 {
            return Err(XC_BM_DEF_MISSING);
        }
        Ok(())
    }
}

/// 基准套件：三类别全定义。
pub struct BenchSuite {
    pub defs: [BenchDef; 3],
}

impl BenchSuite {
    /// 在册三基准（判据点名）。
    pub fn build() -> Self {
        BenchSuite {
            defs: [
                BenchDef { kind: BenchKind::Infer, workload: "llm-7b-token", metric: "tokens/s", iters: 2000 },
                BenchDef { kind: BenchKind::CoWork, workload: "render+asr-pipe", metric: "ms/frame", iters: 1500 },
                BenchDef { kind: BenchKind::Efficiency, workload: "video-call-30min", metric: "perf/W", iters: 1000 },
            ],
        }
    }

    /// 定义齐判：三类别恰好多定义一次且每条健全。
    pub fn defined(&self) -> Result<(), u32> {
        let mut cover = [false; 3];
        for d in self.defs.iter() {
            d.sound()?;
            let mut i = 0;
            while i < 3 {
                if BenchKind::ALL[i] == d.kind {
                    if cover[i] {
                        return Err(XC_BM_DEF_MISSING);
                    }
                    cover[i] = true;
                }
                i += 1;
            }
        }
        let mut i = 0;
        while i < 3 {
            if !cover[i] {
                return Err(XC_BM_DEF_MISSING);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、复现规范（环境标注四要素）
// ---------------------------------------------------------------------------

/// 环境标注四要素（复用 F1702 环境字段惯例）：
/// 引擎版本 / 驱动版本 / 温度墙 / 电源计划。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnvStamp {
    pub engine_ver: u32,
    pub driver_ver: u32,
    /// 温度墙（℃）。
    pub thermal_wall_c: u16,
    /// 电源计划（0=性能 1=均衡 2=省电——非 0..=2 拒）。
    pub power_plan: u8,
}

impl EnvStamp {
    /// 标注齐判：版本双非零 + 温墙合理区间 + 电源计划在册。
    pub fn stamped(&self) -> Result<(), u32> {
        if self.engine_ver == 0 || self.driver_ver == 0 {
            return Err(XC_BM_NOT_REPRODUCIBLE);
        }
        if self.thermal_wall_c < 40 || self.thermal_wall_c > 120 {
            return Err(XC_BM_NOT_REPRODUCIBLE);
        }
        if self.power_plan > 2 {
            return Err(XC_BM_NOT_REPRODUCIBLE);
        }
        Ok(())
    }
}

/// 复现判：同一负载两次独立跑偏差带内（千分比）方为可复现。
pub fn reproducible(a: u64, b: u64, ppm_slack: u64) -> Result<(), u32> {
    if a == 0 || b == 0 || ppm_slack == 0 || ppm_slack > 100_000 {
        return Err(XC_BM_NOT_REPRODUCIBLE);
    }
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    // 偏差 = (hi-lo)/hi，与 slack/1e6 比较（全整数，无浮点）。
    let diff_num = (hi - lo) * 1_000_000;
    if diff_num > hi.saturating_mul(ppm_slack) {
        return Err(XC_BM_NOT_REPRODUCIBLE);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、发布纪律（透明：原始数据在册 + 修订留痕）
// ---------------------------------------------------------------------------

/// 发布账：结果发布必附原始数据指纹 + 修订链。
pub struct PublishLedger {
    /// 原始数据 FNV-1a 64 指纹（发布不可无原始）。
    pub raw_fingerprint: u64,
    /// 修订次数（0=首发；修订必须递增留痕）。
    pub revisions: u32,
    /// 修订链末指针（首发为 0）。
    pub last_rev_id: u64,
}

impl PublishLedger {
    pub fn build(raw_fingerprint: u64) -> Self {
        PublishLedger { raw_fingerprint, revisions: 0, last_rev_id: 0 }
    }

    /// 修订：版本递增 + 指针单调（不可回退）。
    pub fn revise(&mut self, new_rev_id: u64) -> Result<(), u32> {
        if new_rev_id == 0 || new_rev_id <= self.last_rev_id {
            return Err(XC_BM_PUBLISH_DISHONEST);
        }
        self.revisions += 1;
        self.last_rev_id = new_rev_id;
        Ok(())
    }

    /// 发布合法判：指纹非零 + 修订链一致。
    pub fn publishable(&self) -> Result<(), u32> {
        if self.raw_fingerprint == 0 {
            return Err(XC_BM_PUBLISH_DISHONEST);
        }
        if self.revisions > 0 && self.last_rev_id == 0 {
            return Err(XC_BM_PUBLISH_DISHONEST);
        }
        if self.revisions == 0 && self.last_rev_id != 0 {
            return Err(XC_BM_PUBLISH_DISHONEST);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、对比诚实红线（不挑基准）
// ---------------------------------------------------------------------------

/// 与行业对比口径：全基准披露 + 对手数据自报 + 选择性省略检测。
pub struct ComparePolicy {
    /// 本次披露的基准类别掩码（bit0=推理 bit1=协同 bit2=能效）。
    pub disclosed_mask: u8,
    /// 对手数据是否自报（引对方官方数，不得代报）。
    pub rival_self_reported: bool,
    /// 对比负载是否与自测同负载（跨负载对比即挑基准）。
    pub same_workload: bool,
}

impl ComparePolicy {
    pub const MASK_ALL: u8 = 0b111;

    pub fn honest(&self) -> Result<(), u32> {
        // 红线一：三基准全披露（不挑）。
        if self.disclosed_mask != Self::MASK_ALL {
            return Err(XC_BM_COMPARE_CHERRY);
        }
        // 红线二：对手数据必须自报。
        if !self.rival_self_reported {
            return Err(XC_BM_COMPARE_CHERRY);
        }
        // 红线三：同负载对比。
        if !self.same_workload {
            return Err(XC_BM_COMPARE_CHERRY);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：定义/复现/发布/对比）
// ---------------------------------------------------------------------------

/// 基准发布引擎。
pub struct BenchEngine;

impl BenchEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：三基准定义齐。
        BenchSuite::build().defined()?;
        // 反例：迭代不足拒。
        let short = BenchDef { kind: BenchKind::Infer, workload: "x", metric: "t/s", iters: 999 };
        if short.sound().is_ok() {
            return Err(XC_BM_DEF_MISSING);
        }
        // 组二：环境标注齐 + 双跑可复现（2% 带）。
        EnvStamp { engine_ver: 1, driver_ver: 1, thermal_wall_c: 95, power_plan: 0 }.stamped()?;
        reproducible(10_000, 10_050, 20_000)?;
        // 反例：缺电源计划/偏差超带拒。
        if (EnvStamp { engine_ver: 1, driver_ver: 1, thermal_wall_c: 95, power_plan: 9 })
            .stamped()
            .is_ok()
        {
            return Err(XC_BM_NOT_REPRODUCIBLE);
        }
        if reproducible(10_000, 10_900, 20_000).is_ok() {
            return Err(XC_BM_NOT_REPRODUCIBLE);
        }
        // 组三：发布链——首发 + 两次修订单调，回退拒。
        let mut led = PublishLedger::build(0xDEAD_BEEF);
        led.publishable()?;
        led.revise(1)?;
        led.revise(2)?;
        led.publishable()?;
        if led.revise(2).is_ok() {
            return Err(XC_BM_PUBLISH_DISHONEST);
        }
        // 反例：零指纹不可发布。
        if PublishLedger::build(0).publishable().is_ok() {
            return Err(XC_BM_PUBLISH_DISHONEST);
        }
        // 组四：对比诚实——挑基准/代报/跨负载三反例全拦。
        if (ComparePolicy { disclosed_mask: 0b011, rival_self_reported: true, same_workload: true })
            .honest()
            .is_ok()
        {
            return Err(XC_BM_COMPARE_CHERRY);
        }
        if (ComparePolicy { disclosed_mask: 0b111, rival_self_reported: false, same_workload: true })
            .honest()
            .is_ok()
        {
            return Err(XC_BM_COMPARE_CHERRY);
        }
        if (ComparePolicy { disclosed_mask: 0b111, rival_self_reported: true, same_workload: false })
            .honest()
            .is_ok()
        {
            return Err(XC_BM_COMPARE_CHERRY);
        }
        ComparePolicy { disclosed_mask: 0b111, rival_self_reported: true, same_workload: true }
            .honest()?;
        Ok(())
    }

    /// 段对账：五码全落 0xA400..0xA500 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_BM_DEF_MISSING,
            XC_BM_NOT_REPRODUCIBLE,
            XC_BM_PUBLISH_DISHONEST,
            XC_BM_COMPARE_CHERRY,
            XC_BM_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA400 || codes[i] >= 0xA500 {
                return Err(XC_BM_DEF_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_BM_DEF_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
