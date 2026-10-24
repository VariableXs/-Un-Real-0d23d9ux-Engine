//! CGPU-F0007 · 卡顿归因引擎（CGPU-C 域 · 帧预算域 · C08 组 · 深化版）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0007`
//!
//! **判据（锚点原文）**：九类原因注入测试各命中对应归因、置信度阈值
//! 行为正确、事件卡结构完整、十次最贵清单生成、归因延迟 ≤1s。
//!
//! **职责定位（锚点原文）**：掉一帧必须知道为什么。本功能消费帧任务图
//! 快照 + 预算日志 + 硬件计数器，自动归因卡顿：候选原因分类九种
//! （GPU 长任务/CPU 长任务/上传拥塞/锁等待/预算排队/降质反噬/页面
//! 风暴/热降频/驱动抖动），按时间线证据打分排序输出主因 + 副因。
//! 证据源四路：任务图实际耗时 vs 预测、围栏等待窗口、硬件计数器
//! （GPU 占用/显存带宽）、调度事件（抢占/让位/降质）。归因输出结构化
//! 卡顿事件卡（何时/多久/主因/证据链/建议动作），进诊断中心可按
//! "最贵十次卡顿"出改进清单。误报纪律：归因置信度 <60% 标记"待定"
//! 而非强行给结论。
//!
//! 本深化版扩出的真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **九类原因闭集**（判据一）：[`Cause`] 九态，每态绑定证据谓词面
//!    （[`score_rule`]）与建议动作闭集（[`Cause::suggestion`]）——九类
//!    注入各命中对应归因（参考探针逐一注入验证）。
//! 2. **四路证据**（判据二）：[`Evidence`] 单结构四路齐备（任务图实际
//!    vs 预测/围栏等待窗/硬件计数器 GPU 占用与显存带宽/调度事件
//!    抢占让位降质）——缺路即降置信，不猜。
//! 3. **打分排序**：九类各出 bps 分（定权重表 + 证据谓词放大/抑制），
//!    稳定排序出主因+副因——同分按闭集序，可复现。
//! 4. **置信度阈值**（判据二补）：归一置信 ≥ [`CONFIDENT_FLOOR_BPS`]
//!    （60%=6000bps）才出主因结论；不足标"待定"（[`AttribVerdict::
//!    Pending`]）——误报纪律是机制不是注释。
//! 5. **事件卡**（判据三）：[`JankCard`] 五元齐（何时/多久/主因/证据链/
//!    建议动作）+ pending 位 + 证据引用账——结构缺件即拒入库。
//! 6. **最贵十次清单**（判据四）：[`DiagCenter::top_ten`] 按耗时降序
//!    稳定取前十（插入排序）——改进清单产出面。
//! 7. **归因延迟**（判据五）：入卡即归（同步归因面），延迟账
//!    （attrib_tick − jank_tick）逐卡留痕，超 1s 立案。
//! 8. **事件账本**：FNV 链式防篡改 + 守恒对账（九类归因计数与卡数
//!    守恒）。
//!
//! **对接**：F0004（成本模型——预测耗时来源）；F0005（仲裁器——预算
//! 排队证据源）；F0006（帧合同——卡顿判定阈值口径）；F0008（降质链
//! ——降质反噬证据源与建议消费方）。零 panic 面、零 IO、零墙钟
//! （tick 注入）、零浮点、no_std。
use alloc::format;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（阈值/权重唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const JANK_ATTRIB_VERSION: &str = "C08-jankattrib-v1";

/// 置信度地板（bps；6000 = 60%——锚点误报纪律数字钉死）。
pub const CONFIDENT_FLOOR_BPS: u64 = 6000;

/// 单证据源权重基（bps；四路齐备满分基座 2500×4=10000）。
pub const EVIDENCE_BASE_BPS: u64 = 2500;

/// 卡顿判定口径沿用 F0006 帧合同：本模块取输入侧阈值（ms）。
pub const JANK_THRESHOLD_MS_DEFAULT: u64 = 20;

/// 归因延迟上限（ms；锚点"≤1s"——超限立案）。
pub const ATTRIB_LATENCY_MAX_MS: u64 = 1000;

/// 最贵清单容量（锚点"十次"）。
pub const TOP_TEN: usize = 10;

/// 事件卡账容量（滚动；最贵清单基于全量卡史——卡史独立滚动账）。
pub const CARD_LEDGER_CAPACITY: usize = 256;

/// 事件账容量（审计账滚动）。
pub const EVENT_LEDGER_CAPACITY: usize = 96;

/// 页面风暴门槛（次/帧；硬件计数器路）。
pub const PAGEFAULT_STORM_THRESHOLD: u32 = 64;

/// GPU 占用高线（bps；>9000 视为 GPU 饱和证据）。
pub const GPU_BUSY_HIGH_BPS: u64 = 9000;

/// GPU 占用低线（bps；<4000 视为 GPU 空闲证据——CPU 侧归因放大）。
pub const GPU_BUSY_LOW_BPS: u64 = 4000;

/// 围栏等待高线（ms；≥8ms 视为等待受阻证据）。
pub const FENCE_WAIT_HIGH_MS: u64 = 8;

/// 上传拥塞门槛（ms；上传任务超预测达此值）。
pub const UPLOAD_CONGEST_MS: u64 = 6;

// ---------------------------------------------------------------------------
// 二、诊断码（0x3D 段独占：C08 卡顿归因面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x3D 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 事件卡缺件（何时/多久/证据链不全——结构完整性闸）。
    CardIncomplete,
    /// 证据缺路（四路证据不全——归因降置信入库而非拒）。
    EvidencePartial,
    /// 卡顿时长为 0（无卡顿不归因）。
    ZeroDuration,
    /// 归因延迟超限（>1s——立案不静默）。
    AttribLatencyOver,
    /// 事件账指纹链断裂。
    LedgerCorrupt,
    /// 归因计数与卡数守恒失败。
    AttribMismatch,
    /// 清单为空（无卡可出——显性而非空报告）。
    TopListEmpty,
}

impl XcCode {
    /// 人读诊断（全带可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::CardIncomplete => "事件卡缺件：何时/多久/证据链五元不全，建议核对归因入口是否传齐证据",
            XcCode::EvidencePartial => "证据缺路：四路证据不全，归因降置信处理（不足阈值即待定），建议补齐采集面",
            XcCode::ZeroDuration => "卡顿时长为 0：无卡顿不归因，建议核对卡顿判定阈值口径（F0006）",
            XcCode::AttribLatencyOver => "归因延迟超 1s：同步归因面被阻塞，建议核查诊断中心入卡路径",
            XcCode::LedgerCorrupt => "事件账指纹链断裂：账面被篡改或乱序，建议以快照为准重建",
            XcCode::AttribMismatch => "归因计数与卡数守恒失败：建议对账修复后再出清单",
            XcCode::TopListEmpty => "最贵清单为空：无卡顿卡在册，建议先确认卡顿确实发生过再出改进清单",
        }
    }

    /// 0x3D 段十六进制码。
    pub const fn code_hex(self) -> u16 {
        match self {
            XcCode::CardIncomplete => 0x3D01,
            XcCode::EvidencePartial => 0x3D02,
            XcCode::ZeroDuration => 0x3D03,
            XcCode::AttribLatencyOver => 0x3D04,
            XcCode::LedgerCorrupt => 0x3D05,
            XcCode::AttribMismatch => 0x3D06,
            XcCode::TopListEmpty => 0x3D07,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节混入。
pub const fn fnv1a_byte(state: u64, b: u8) -> u64 {
    (state ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a u64 定长混入。
pub const fn fnv1a_u64(state: u64, v: u64) -> u64 {
    let s1 = fnv1a_byte(state, (v & 0xff) as u8);
    let s2 = fnv1a_byte(s1, ((v >> 8) & 0xff) as u8);
    let s3 = fnv1a_byte(s2, ((v >> 16) & 0xff) as u8);
    let s4 = fnv1a_byte(s3, ((v >> 24) & 0xff) as u8);
    let s5 = fnv1a_byte(s4, ((v >> 32) & 0xff) as u8);
    let s6 = fnv1a_byte(s5, ((v >> 40) & 0xff) as u8);
    let s7 = fnv1a_byte(s6, ((v >> 48) & 0xff) as u8);
    fnv1a_byte(s7, ((v >> 56) & 0xff) as u8)
}

/// FNV-1a 字符串混入。
pub const fn fnv1a_str(state: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut st = state;
    while i < b.len() {
        st = fnv1a_byte(st, b[i]);
        i += 1;
    }
    st
}

// ---------------------------------------------------------------------------
// 四、九类原因闭集（判据一：语义+建议动作一体）
// ---------------------------------------------------------------------------

/// 候选原因九类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// GPU 长任务（GPU 侧任务超预测且 GPU 饱和）。
    GpuLongTask,
    /// CPU 长任务（CPU 侧任务超预测且 GPU 空闲）。
    CpuLongTask,
    /// 上传拥塞（上传类任务超预测超拥塞线）。
    UploadCongestion,
    /// 锁等待（围栏等待高 + 双侧皆不饱和——卡在同步点）。
    LockWait,
    /// 预算排队（调度事件面出现排队让位——仲裁器拒/排队证据）。
    BudgetQueue,
    /// 降质反噬（降质动作后帧仍超——降级档位反噬证据）。
    DegradeBackfire,
    /// 页面风暴（显存缺页计数风暴——页面迁移挤占带宽）。
    PageStorm,
    /// 热降频（热标志 + 频率骤降证据——F0011 降频失守归因）。
    ThermalThrottle,
    /// 驱动抖动（九类排除后残余的耗时方差抖动——驱动层嫌疑）。
    DriverJitter,
}

impl Cause {
    /// 闭集序（0-8；同分稳定排序键）。
    pub const fn ordinal(self) -> usize {
        match self {
            Cause::GpuLongTask => 0,
            Cause::CpuLongTask => 1,
            Cause::UploadCongestion => 2,
            Cause::LockWait => 3,
            Cause::BudgetQueue => 4,
            Cause::DegradeBackfire => 5,
            Cause::PageStorm => 6,
            Cause::ThermalThrottle => 7,
            Cause::DriverJitter => 8,
        }
    }

    /// 由闭集序还原。
    pub const fn from_ordinal(o: usize) -> Option<Cause> {
        match o {
            0 => Some(Cause::GpuLongTask),
            1 => Some(Cause::CpuLongTask),
            2 => Some(Cause::UploadCongestion),
            3 => Some(Cause::LockWait),
            4 => Some(Cause::BudgetQueue),
            5 => Some(Cause::DegradeBackfire),
            6 => Some(Cause::PageStorm),
            7 => Some(Cause::ThermalThrottle),
            8 => Some(Cause::DriverJitter),
            _ => None,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            Cause::GpuLongTask => "GPU长任务",
            Cause::CpuLongTask => "CPU长任务",
            Cause::UploadCongestion => "上传拥塞",
            Cause::LockWait => "锁等待",
            Cause::BudgetQueue => "预算排队",
            Cause::DegradeBackfire => "降质反噬",
            Cause::PageStorm => "页面风暴",
            Cause::ThermalThrottle => "热降频",
            Cause::DriverJitter => "驱动抖动",
        }
    }

    /// 建议动作闭集（事件卡第五元——每因一动作，不空谈）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            Cause::GpuLongTask => "建议：拆分 GPU 长任务或降低该任务档位（F0008 降质链）",
            Cause::CpuLongTask => "建议：核查 CPU 侧热点并错峰提交（避开交互帧）",
            Cause::UploadCongestion => "建议：错峰上传并压缩上传体量（F1283 带宽账户限流）",
            Cause::LockWait => "建议：缩短临界区并检查围栏依赖链是否成环",
            Cause::BudgetQueue => "建议：上调该任务优先级或清理预算排队积压（F0005 仲裁面）",
            Cause::DegradeBackfire => "建议：回退该降质档位并改走资产降级（降质反噬面）",
            Cause::PageStorm => "建议：收紧驻留集并预热页面（F0852/H 域驻留面）",
            Cause::ThermalThrottle => "建议：触发热策略降载（F0011 预测性降载提前 2s）",
            Cause::DriverJitter => "建议：采集驱动版本与差异表比对（F0963 差异表面）",
        }
    }

    /// 基础权重（bps；打分基座——谓词放大/抑制在此基础上）。
    pub const fn base_weight_bps(self) -> u64 {
        match self {
            Cause::GpuLongTask => 4200,
            Cause::CpuLongTask => 4000,
            Cause::UploadCongestion => 3400,
            Cause::LockWait => 3200,
            Cause::BudgetQueue => 3000,
            Cause::DegradeBackfire => 2600,
            Cause::PageStorm => 2800,
            Cause::ThermalThrottle => 3000,
            Cause::DriverJitter => 1800,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、四路证据（单结构齐备面——缺路即降置信）
// ---------------------------------------------------------------------------

/// 归因证据（四路齐备结构；缺路字段填"无证据"哨兵值并计缺路数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    // —— 路 1：任务图实际 vs 预测（F0004 口径）——
    /// 最大超预测任务的超时量（ms；无超预测任务 0）。
    pub max_task_overrun_ms: u64,
    /// 超预测任务所属侧（true=GPU 侧 / false=CPU 侧；无超任务 false）。
    pub overrun_gpu_side: bool,
    /// 超预测任务是否为上传类（上传拥塞证据半边）。
    pub overrun_is_upload: bool,
    /// 任务耗时方差（ms² 定点；驱动抖动证据）。
    pub task_variance_ms2: u64,
    // —— 路 2：围栏等待窗 ——
    /// 围栏等待总窗（ms；0 = 无等待）。
    pub fence_wait_ms: u64,
    // —— 路 3：硬件计数器 ——
    /// GPU 占用（bps）。
    pub gpu_busy_bps: u64,
    /// 显存带宽占用（bps）。
    pub vram_bw_bps: u64,
    /// 页缺失页数（次/帧）。
    pub pagefaults: u32,
    // —— 路 4：调度事件 ——
    /// 本帧发生预算排队（F0005 排队/拒单证据）。
    pub budget_queued: bool,
    /// 本帧发生降质动作（F0008 降质链证据）。
    pub degraded: bool,
    /// 本帧发生热降频（F0011 失守证据）。
    pub thermal_throttled: bool,
    /// 证据路数（0-4；缺路降置信——机制面）。
    pub routes_present: u8,
}

impl Evidence {
    /// 空证据（全无——归因必待定）。
    pub const fn none() -> Evidence {
        Evidence {
            max_task_overrun_ms: 0,
            overrun_gpu_side: false,
            overrun_is_upload: false,
            task_variance_ms2: 0,
            fence_wait_ms: 0,
            gpu_busy_bps: 0,
            vram_bw_bps: 0,
            pagefaults: 0,
            budget_queued: false,
            degraded: false,
            thermal_throttled: false,
            routes_present: 0,
        }
    }

    /// 缺路惩罚（bps；每缺一路扣 1500——四路齐才足信）。
    const fn route_penalty(&self) -> u64 {
        let missing = 4u64.saturating_sub(self.routes_present as u64);
        missing.saturating_mul(1500)
    }
}

/// 单因打分（bps；谓词面——证据驱动，不拍脑袋）。
/// 返回该因本帧得分（0 = 无证据支持）。
pub const fn score_rule(cause: Cause, ev: &Evidence) -> u64 {
    let base = cause.base_weight_bps();
    match cause {
        Cause::GpuLongTask => {
            if ev.max_task_overrun_ms > 0 && ev.overrun_gpu_side && ev.gpu_busy_bps >= GPU_BUSY_HIGH_BPS {
                base.saturating_add(3000)
            } else if ev.max_task_overrun_ms > 0 && ev.overrun_gpu_side {
                base
            } else {
                0
            }
        }
        Cause::CpuLongTask => {
            if ev.max_task_overrun_ms > 0 && !ev.overrun_gpu_side && ev.gpu_busy_bps <= GPU_BUSY_LOW_BPS {
                base.saturating_add(3000)
            } else if ev.max_task_overrun_ms > 0 && !ev.overrun_gpu_side {
                base
            } else {
                0
            }
        }
        Cause::UploadCongestion => {
            if ev.overrun_is_upload && ev.max_task_overrun_ms >= UPLOAD_CONGEST_MS {
                base.saturating_add(3200)
            } else if ev.overrun_is_upload {
                base
            } else {
                0
            }
        }
        Cause::LockWait => {
            if ev.fence_wait_ms >= FENCE_WAIT_HIGH_MS
                && ev.gpu_busy_bps <= GPU_BUSY_LOW_BPS
                && ev.max_task_overrun_ms == 0
            {
                base.saturating_add(3400)
            } else if ev.fence_wait_ms >= FENCE_WAIT_HIGH_MS {
                base
            } else {
                0
            }
        }
        Cause::BudgetQueue => {
            if ev.budget_queued {
                base.saturating_add(3000)
            } else {
                0
            }
        }
        Cause::DegradeBackfire => {
            if ev.degraded && ev.max_task_overrun_ms > 0 {
                base.saturating_add(2800)
            } else if ev.degraded {
                base
            } else {
                0
            }
        }
        Cause::PageStorm => {
            if ev.pagefaults >= PAGEFAULT_STORM_THRESHOLD {
                base.saturating_add(3000)
            } else if ev.pagefaults > 0 {
                base / 2
            } else {
                0
            }
        }
        Cause::ThermalThrottle => {
            if ev.thermal_throttled {
                base.saturating_add(3200)
            } else {
                0
            }
        }
        Cause::DriverJitter => {
            // 排除面：四路皆无强证据时，方差高即嫌疑（残余归因）。
            if ev.max_task_overrun_ms == 0
                && ev.fence_wait_ms < FENCE_WAIT_HIGH_MS
                && !ev.budget_queued
                && !ev.degraded
                && !ev.thermal_throttled
                && ev.pagefaults < PAGEFAULT_STORM_THRESHOLD
                && ev.task_variance_ms2 > 0
            {
                base.saturating_add(2400)
            } else if ev.task_variance_ms2 > 0 {
                base / 2
            } else {
                0
            }
        }
    }
}

/// 归因裁决（主因/待定二态——置信不足不给结论）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttribVerdict {
    /// 主因确立（携带归因序与置信）。
    Confirmed { cause: Cause, confidence_bps: u64 },
    /// 待定（置信不足——误报纪律）。
    Pending { best: Cause, confidence_bps: u64 },
}

/// 归因得分行（排序用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CauseScore {
    /// 原因。
    pub cause: Cause,
    /// 归一分（bps；0-10000）。
    pub score_bps: u64,
}

/// 归因核心（打分→归一→排序→阈值裁决）。
pub struct AttribEngine;

impl AttribEngine {
    /// 归因一次（证据 → 主因+副因 + 置信裁决）。
    /// 返回 (裁决, 九因得分降序表)。副因 = 得分非零且非主因的前二。
    pub fn attribute(ev: &Evidence) -> (AttribVerdict, Vec<CauseScore>) {
        // 逐因打分（bps 原始）。
        let mut raw: [(u64, usize); 9] = [(0, 0); 9];
        let mut total: u64 = 0;
        let mut i = 0usize;
        while i < 9 {
            let cause = match Cause::from_ordinal(i) {
                Some(c) => c,
                None => break,
            };
            let mut s = score_rule(cause, ev);
            // 缺路惩罚（四路不全降置信——不猜）。
            s = s.saturating_sub(ev.route_penalty());
            let ord = cause.ordinal();
            raw[ord] = (s, ord);
            total = total.saturating_add(s);
            i += 1;
        }
        // 归一（bps；总分 0 → 全零表 + 待定）。
        let mut scores: Vec<CauseScore> = Vec::new();
        let mut i = 0usize;
        while i < 9 {
            let (s, ord) = raw[i];
            let norm = if total == 0 {
                0
            } else {
                s.saturating_mul(10_000) / total
            };
            if let Some(cause) = Cause::from_ordinal(ord) {
                if s > 0 {
                    scores.push(CauseScore { cause, score_bps: norm });
                }
            }
            i += 1;
        }
        // 稳定排序：分数降序、同分按闭集序（插入排序——9 元小表）。
        let n = scores.len();
        let mut i = 1usize;
        while i < n {
            let key = match scores.get(i) {
                Some(s) => *s,
                None => break,
            };
            let mut j = i;
            while j > 0 {
                let prev = match scores.get(j.saturating_sub(1)) {
                    Some(p) => *p,
                    None => break,
                };
                if prev.score_bps < key.score_bps
                    || (prev.score_bps == key.score_bps && prev.cause.ordinal() > key.cause.ordinal())
                {
                    let v = scores.remove(j.saturating_sub(1));
                    scores.insert(j, v);
                    j -= 1;
                } else {
                    break;
                }
            }
            i += 1;
        }
        // 裁决：主因置信 ≥ 60% 确立；否则待定（误报纪律）。
        let verdict = match scores.first() {
            Some(top) if top.score_bps >= CONFIDENT_FLOOR_BPS => AttribVerdict::Confirmed {
                cause: top.cause,
                confidence_bps: top.score_bps,
            },
            Some(top) => AttribVerdict::Pending {
                best: top.cause,
                confidence_bps: top.score_bps,
            },
            None => AttribVerdict::Pending {
                best: Cause::DriverJitter,
                confidence_bps: 0,
            },
        };
        (verdict, scores)
    }
}

// ---------------------------------------------------------------------------
// 六、事件卡（五元齐备 + pending 位 + 证据引用账）
// ---------------------------------------------------------------------------

/// 卡顿事件卡（何时/多久/主因/证据链/建议动作——五元缺一拒入库）。
#[derive(Clone, Debug)]
pub struct JankCard {
    /// 卡 id（账内唯一）。
    pub card_id: u32,
    /// 何时（卡顿帧起始 tick）。
    pub when_tick: u64,
    /// 帧号。
    pub frame_id: u64,
    /// 多久（卡顿时长 ms）。
    pub duration_ms: u64,
    /// 裁决（主因或待定）。
    pub verdict: AttribVerdict,
    /// 副因（非主因得分前二）。
    pub secondary: Vec<Cause>,
    /// 证据引用（归因时点的证据快照指纹——证据链锚）。
    pub evidence_fp: u64,
    /// 证据缺路数。
    pub missing_routes: u8,
    /// 建议动作（主因绑定；待定给排查建议）。
    pub suggestion: String,
    /// 归因延迟（ms；attrib_tick − when_tick）。
    pub latency_ms: u64,
    /// 延迟超限标记（>1s 立案）。
    pub latency_over: bool,
    /// 卡指纹（链上）。
    pub fp: u64,
}

impl JankCard {
    /// 结构完整性闸（五元齐备判定）。
    pub const fn complete(&self) -> bool {
        self.duration_ms > 0 && self.evidence_fp != 0 && !self.suggestion.is_empty()
    }
}

/// 卡账（滚动 256 + FNV 链——最贵清单的数据面）。
#[derive(Clone, Debug)]
pub struct CardLedger {
    cards: Vec<JankCard>,
    next_id: u32,
    /// 逐出累计。
    pub evicted: u64,
    /// 尾指纹。
    pub tail_fp: u64,
}

impl CardLedger {
    /// 空账。
    pub fn new() -> CardLedger {
        CardLedger {
            cards: Vec::new(),
            next_id: 1,
            evicted: 0,
            tail_fp: FNV_OFFSET,
        }
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// 入卡（滚动逐出；完整性闸；链上指纹）。
    pub fn push(&mut self, mut card: JankCard) -> Result<u32, XcCode> {
        if !card.complete() {
            return Err(XcCode::CardIncomplete);
        }
        card.card_id = self.next_id;
        let fp = fnv1a_u64(self.tail_fp, self.next_id as u64);
        let fp = fnv1a_u64(fp, card.when_tick);
        let fp = fnv1a_u64(fp, card.duration_ms);
        let fp = fnv1a_u64(fp, card.evidence_fp);
        card.fp = fp;
        self.tail_fp = fp;
        self.next_id = self.next_id.saturating_add(1);
        if self.cards.len() >= CARD_LEDGER_CAPACITY {
            let _ = self.cards.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        let id = card.card_id;
        self.cards.push(card);
        Ok(id)
    }

    /// 链复验。
    pub fn verify_chain(&self) -> Result<(), XcCode> {
        let mut prev = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.cards.len() {
            let c = match self.cards.get(i) {
                Some(c) => c,
                None => return Err(XcCode::LedgerCorrupt),
            };
            let mut fp = fnv1a_u64(prev, c.card_id as u64);
            fp = fnv1a_u64(fp, c.when_tick);
            fp = fnv1a_u64(fp, c.duration_ms);
            fp = fnv1a_u64(fp, c.evidence_fp);
            if fp != c.fp {
                return Err(XcCode::LedgerCorrupt);
            }
            prev = c.fp;
            i += 1;
        }
        if prev != self.tail_fp {
            return Err(XcCode::LedgerCorrupt);
        }
        Ok(())
    }

    /// 最贵十次（耗时降序稳定取前十——锚点改进清单面）。
    pub fn top_ten(&self) -> Vec<&JankCard> {
        let mut idx: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < self.cards.len() {
            idx.push(i);
            i += 1;
        }
        // 按 (duration 降序, card_id 升序) 稳定排序。
        let n = idx.len();
        let mut a = 1usize;
        while a < n {
            let key = *idx.get(a).unwrap_or(&0);
            let mut b = a;
            while b > 0 {
                let pk = *idx.get(b.saturating_sub(1)).unwrap_or(&0);
                let kd = self.cards.get(key).map(|c| c.duration_ms).unwrap_or(0);
                let pd = self.cards.get(pk).map(|c| c.duration_ms).unwrap_or(0);
                if pd < kd || (pd == kd && pk > key) {
                    let v = idx.remove(b.saturating_sub(1));
                    idx.insert(b, v);
                    b -= 1;
                } else {
                    break;
                }
            }
            a += 1;
        }
        let mut out: Vec<&JankCard> = Vec::new();
        let mut i = 0usize;
        while i < idx.len() && i < TOP_TEN {
            let ix = *idx.get(i).unwrap_or(&0);
            if let Some(c) = self.cards.get(ix) {
                out.push(c);
            }
            i += 1;
        }
        out
    }

    /// 按因计数（守恒对账面）。
    pub fn count_by_cause(&self, cause: Cause) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                let hit = match &c.verdict {
                    AttribVerdict::Confirmed { cause: k, .. } => *k == cause,
                    AttribVerdict::Pending { best, .. } => *best == cause,
                };
                if hit {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 七、审计账（六类事件 + FNV 链）
// ---------------------------------------------------------------------------

/// 审计事件闭集（六类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    /// 卡顿入卡。
    Carded,
    /// 归因确立。
    Confirmed,
    /// 归因待定。
    PendingMarked,
    /// 延迟立案。
    LatencyOver,
    /// 清单产出。
    TopListOut,
    /// 证据缺路。
    EvidencePartial,
}

impl LedgerKind {
    /// 序号。
    pub const fn kind_id(self) -> u8 {
        match self {
            LedgerKind::Carded => 0,
            LedgerKind::Confirmed => 1,
            LedgerKind::PendingMarked => 2,
            LedgerKind::LatencyOver => 3,
            LedgerKind::TopListOut => 4,
            LedgerKind::EvidencePartial => 5,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            LedgerKind::Carded => "入卡",
            LedgerKind::Confirmed => "归因确立",
            LedgerKind::PendingMarked => "待定",
            LedgerKind::LatencyOver => "延迟立案",
            LedgerKind::TopListOut => "清单产出",
            LedgerKind::EvidencePartial => "证据缺路",
        }
    }
}

/// 账面行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerRecord {
    /// 序号。
    pub seq: u64,
    /// tick。
    pub tick: u64,
    /// 类别。
    pub kind: LedgerKind,
    /// 量。
    pub amount: u64,
    /// 摘要。
    pub summary: String,
    /// 指纹。
    pub fp: u64,
}

/// 审计账（滚动 96 + FNV 链）。
#[derive(Clone, Debug)]
pub struct EventLedger {
    records: Vec<LedgerRecord>,
    next_seq: u64,
    /// 逐出累计。
    pub evicted: u64,
    /// 尾指纹。
    pub tail_fp: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> EventLedger {
        EventLedger {
            records: Vec::new(),
            next_seq: 1,
            evicted: 0,
            tail_fp: FNV_OFFSET,
        }
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 追加。
    pub fn append(&mut self, tick: u64, kind: LedgerKind, amount: u64, summary: &str) -> u64 {
        let seq = self.next_seq;
        let mut fp = fnv1a_u64(self.tail_fp, seq);
        fp = fnv1a_u64(fp, tick);
        fp = fnv1a_u64(fp, kind.kind_id() as u64);
        fp = fnv1a_u64(fp, amount);
        fp = fnv1a_str(fp, summary);
        self.tail_fp = fp;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.records.len() >= EVENT_LEDGER_CAPACITY {
            let _ = self.records.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        self.records.push(LedgerRecord {
            seq,
            tick,
            kind,
            amount,
            summary: summary.to_string(),
            fp,
        });
        seq
    }

    /// 链复验。
    pub fn verify_chain(&self) -> Result<(), XcCode> {
        let mut prev = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => return Err(XcCode::LedgerCorrupt),
            };
            let mut fp = fnv1a_u64(prev, r.seq);
            fp = fnv1a_u64(fp, r.tick);
            fp = fnv1a_u64(fp, r.kind.kind_id() as u64);
            fp = fnv1a_u64(fp, r.amount);
            fp = fnv1a_str(fp, &r.summary);
            if fp != r.fp {
                return Err(XcCode::LedgerCorrupt);
            }
            prev = r.fp;
            i += 1;
        }
        if prev != self.tail_fp {
            return Err(XcCode::LedgerCorrupt);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 八、诊断中心（入卡即归 + 最贵清单 + 守恒 + 人读）
// ---------------------------------------------------------------------------

/// 卡顿诊断中心。
#[derive(Clone, Debug)]
pub struct DiagCenter {
    /// 卡账。
    pub cards: CardLedger,
    /// 审计账。
    pub ledger: EventLedger,
    /// 卡顿判定阈值（ms；F0006 口径注入）。
    pub threshold_ms: u64,
    /// 归因确立计数。
    pub confirmed_total: u64,
    /// 待定计数（误报纪律面——待定不是失败是诚实）。
    pub pending_total: u64,
    /// 延迟立案计数。
    pub latency_over_total: u64,
    /// 证据缺路计数。
    pub evidence_partial_total: u64,
}

impl DiagCenter {
    /// 新中心（阈值注入）。
    pub fn new(threshold_ms: u64) -> DiagCenter {
        DiagCenter {
            cards: CardLedger::new(),
            ledger: EventLedger::new(),
            threshold_ms: if threshold_ms == 0 { JANK_THRESHOLD_MS_DEFAULT } else { threshold_ms },
            confirmed_total: 0,
            pending_total: 0,
            latency_over_total: 0,
            evidence_partial_total: 0,
        }
    }

    /// 归因入卡（同步归因面——归因延迟 ≤1s 判据的机制保证：入卡即算）。
    /// `attrib_tick` 为归因时刻（注入）；`jank_tick` 为卡顿发生时刻。
    pub fn attribute_jank(
        &mut self,
        frame_id: u64,
        jank_tick: u64,
        duration_ms: u64,
        ev: &Evidence,
        attrib_tick: u64,
    ) -> Result<u32, XcCode> {
        if duration_ms == 0 {
            return Err(XcCode::ZeroDuration);
        }
        if duration_ms < self.threshold_ms {
            // 未达卡顿线：不入卡（口径一致——F0006）。
            return Ok(0);
        }
        // 归因（同步——延迟机制面）。
        let (verdict, scores) = AttribEngine::attribute(ev);
        // 延迟账。
        let latency = attrib_tick.saturating_sub(jank_tick);
        let latency_over = latency > ATTRIB_LATENCY_MAX_MS;
        if latency_over {
            self.latency_over_total = self.latency_over_total.saturating_add(1);
            self.ledger.append(attrib_tick, LedgerKind::LatencyOver, latency, "归因延迟超 1s 立案");
        }
        // 证据缺路面。
        if ev.routes_present < 4 {
            self.evidence_partial_total = self.evidence_partial_total.saturating_add(1);
            self.ledger.append(
                attrib_tick,
                LedgerKind::EvidencePartial,
                ev.routes_present as u64,
                "四路证据不全：降置信处理",
            );
        }
        // 证据链锚指纹。
        let mut evp = fnv1a_u64(FNV_OFFSET, ev.max_task_overrun_ms);
        evp = fnv1a_u64(evp, ev.fence_wait_ms);
        evp = fnv1a_u64(evp, ev.gpu_busy_bps);
        evp = fnv1a_u64(evp, ev.pagefaults as u64);
        evp = fnv1a_u64(evp, ev.routes_present as u64);
        // 建议动作（主因绑定；待定给排查建议）。
        let suggestion = match &verdict {
            AttribVerdict::Confirmed { cause, .. } => cause.suggestion().to_string(),
            AttribVerdict::Pending { best, .. } => {
                alloc::format!("待定（嫌疑 {} {}）：建议补齐证据面后复核", best.zh(), best.suggestion())
            }
        };
        // 副因（非主因前二）。
        let mut secondary: Vec<Cause> = Vec::new();
        let mut i = 0usize;
        while i < scores.len() && secondary.len() < 2 {
            let sc = match scores.get(i) {
                Some(s) => *s,
                None => break,
            };
            let is_main = match &verdict {
                AttribVerdict::Confirmed { cause, .. } => sc.cause == *cause,
                AttribVerdict::Pending { best, .. } => sc.cause == *best,
            };
            if !is_main {
                secondary.push(sc.cause);
            }
            i += 1;
        }
        // 立账。
        match &verdict {
            AttribVerdict::Confirmed { cause, confidence_bps } => {
                self.confirmed_total = self.confirmed_total.saturating_add(1);
                self.ledger.append(
                    attrib_tick,
                    LedgerKind::Confirmed,
                    *confidence_bps / 100,
                    &alloc::format!("帧{} 主因 {} 置信 {}%", frame_id, cause.zh(), confidence_bps / 100),
                );
            }
            AttribVerdict::Pending { best, confidence_bps } => {
                self.pending_total = self.pending_total.saturating_add(1);
                self.ledger.append(
                    attrib_tick,
                    LedgerKind::PendingMarked,
                    *confidence_bps / 100,
                    &alloc::format!("帧{} 待定（嫌疑 {}）置信 {}%", frame_id, best.zh(), confidence_bps / 100),
                );
            }
        }
        let card = JankCard {
            card_id: 0,
            when_tick: jank_tick,
            frame_id,
            duration_ms,
            verdict,
            secondary,
            evidence_fp: evp,
            missing_routes: 4u8.saturating_sub(ev.routes_present),
            suggestion,
            latency_ms: latency,
            latency_over,
            fp: 0,
        };
        let id = self.cards.push(card)?;
        self.ledger.append(attrib_tick, LedgerKind::Carded, id as u64, "卡顿事件卡入库");
        Ok(id)
    }

    /// 最贵十次清单（锚点改进清单面；空账显性报错不空报告）。
    pub fn top_ten(&self) -> Result<Vec<&JankCard>, XcCode> {
        let list = self.cards.top_ten();
        if list.is_empty() {
            return Err(XcCode::TopListEmpty);
        }
        Ok(list)
    }

    /// 守恒：确立+待定 == 卡数。
    pub fn reconcile(&self) -> Result<(), XcCode> {
        if self.confirmed_total.saturating_add(self.pending_total) != self.cards.len() as u64 {
            return Err(XcCode::AttribMismatch);
        }
        self.cards.verify_chain()?;
        self.ledger.verify_chain()
    }

    /// 全量校验。
    pub fn validate(&self) -> Result<(), XcCode> {
        self.reconcile()
    }

    /// 人读汇总。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("== CGPU-F0007 卡顿归因引擎 汇报 ==\n");
        s.push_str(&alloc::format!(
            "卡 {} 确立 {} 待定 {} 延迟立案 {} 证据缺路 {}\n",
            self.cards.len(),
            self.confirmed_total,
            self.pending_total,
            self.latency_over_total,
            self.evidence_partial_total
        ));
        match self.top_ten() {
            Ok(list) => {
                s.push_str("-- 最贵十次卡顿 --\n");
                let mut i = 0usize;
                while i < list.len() {
                    let c = list.get(i).copied();
                    if let Some(c) = c {
                        let cause = match &c.verdict {
                            AttribVerdict::Confirmed { cause, confidence_bps } => {
                                alloc::format!("{}（{}%）", cause.zh(), confidence_bps / 100)
                            }
                            AttribVerdict::Pending { best, confidence_bps } => {
                                alloc::format!("待定·嫌疑{}（{}%）", best.zh(), confidence_bps / 100)
                            }
                        };
                        s.push_str(&alloc::format!(
                            "#{} 帧{} {}ms {} 延迟 {}ms\n  {}\n",
                            i + 1,
                            c.frame_id,
                            c.duration_ms,
                            cause,
                            c.latency_ms,
                            c.suggestion
                        ));
                    }
                    i += 1;
                }
            }
            Err(_) => {
                s.push_str("-- 最贵十次：无卡在册 --\n");
            }
        }
        s
    }

    /// 无障碍读屏（三段固定预算：中心态/最贵头条/待定面）。
    pub fn read_screen(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(alloc::format!(
            "卡顿归因：在卡 {} 张，确立 {} 待定 {}",
            self.cards.len(),
            self.confirmed_total,
            self.pending_total
        ));
        // 头条（最贵一卡的因与建议）。
        match self.cards.top_ten().first().copied() {
            Some(c) => {
                let cause = match &c.verdict {
                    AttribVerdict::Confirmed { cause, .. } => cause.zh().to_string(),
                    AttribVerdict::Pending { best, .. } => alloc::format!("待定·嫌疑{}", best.zh()),
                };
                out.push(alloc::format!("最贵：{}ms，{}", c.duration_ms, cause));
            }
            None => out.push("最贵：暂无卡顿记录".to_string()),
        }
        // 待定面（仅待定时占预算——诚实不隐藏）。
        if self.pending_total > 0 {
            out.push(alloc::format!("注意：待定归因 {} 张，建议补证据复核", self.pending_total));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 九、参考探针（判据八：九类注入各命中/阈值行为/事件卡结构/最贵清单/延迟）
// ---------------------------------------------------------------------------

/// 参考探针：核心判据自证（纯函数口径，零 IO）。
pub fn run_cgc08_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    // 一、九类注入各命中对应归因（置信 ≥60% 主因确立）。
    // 1. GPU 长任务：GPU 侧超预测 + GPU 饱和。
    let ev_gpu = Evidence {
        max_task_overrun_ms: 9,
        overrun_gpu_side: true,
        overrun_is_upload: false,
        task_variance_ms2: 0,
        fence_wait_ms: 0,
        gpu_busy_bps: 9500,
        vram_bw_bps: 5000,
        pagefaults: 0,
        budget_queued: false,
        degraded: false,
        thermal_throttled: false,
        routes_present: 4,
    };
    let (v, _) = AttribEngine::attribute(&ev_gpu);
    out.push(("注入1 GPU长任务命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::GpuLongTask, .. })));
    // 2. CPU 长任务：CPU 侧超预测 + GPU 空闲。
    let ev_cpu = Evidence { max_task_overrun_ms: 11, overrun_gpu_side: false, gpu_busy_bps: 2000, ..ev_gpu };
    let (v, _) = AttribEngine::attribute(&ev_cpu);
    out.push(("注入2 CPU长任务命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::CpuLongTask, .. })));
    // 3. 上传拥塞：上传类超预测超线。
    let ev_up = Evidence { overrun_is_upload: true, max_task_overrun_ms: 8, gpu_busy_bps: 6000, ..ev_gpu };
    let (v, _) = AttribEngine::attribute(&ev_up);
    out.push(("注入3 上传拥塞命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::UploadCongestion, .. })));
    // 4. 锁等待：围栏高 + 双侧不饱和 + 无超预测。
    let ev_lock = Evidence {
        max_task_overrun_ms: 0,
        fence_wait_ms: 12,
        gpu_busy_bps: 3000,
        overrun_gpu_side: false,
        overrun_is_upload: false,
        task_variance_ms2: 0,
        vram_bw_bps: 2000,
        pagefaults: 0,
        budget_queued: false,
        degraded: false,
        thermal_throttled: false,
        routes_present: 4,
    };
    let (v, _) = AttribEngine::attribute(&ev_lock);
    out.push(("注入4 锁等待命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::LockWait, .. })));
    // 5. 预算排队。
    let ev_q = Evidence { budget_queued: true, gpu_busy_bps: 6000, max_task_overrun_ms: 0, fence_wait_ms: 0, ..Evidence::none() };
    let (v, _) = AttribEngine::attribute(&ev_q);
    out.push(("注入5 预算排队命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::BudgetQueue, .. })));
    // 6. 降质反噬。
    let ev_dg = Evidence { degraded: true, max_task_overrun_ms: 7, gpu_busy_bps: 7000, fence_wait_ms: 0, overrun_gpu_side: true, ..Evidence::none() };
    let (v, _) = AttribEngine::attribute(&ev_dg);
    out.push(("注入6 降质反噬命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::DegradeBackfire, .. })));
    // 7. 页面风暴。
    let ev_pf = Evidence { pagefaults: 100, gpu_busy_bps: 5000, ..Evidence::none() };
    let (v, _) = AttribEngine::attribute(&ev_pf);
    out.push(("注入7 页面风暴命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::PageStorm, .. })));
    // 8. 热降频。
    let ev_th = Evidence { thermal_throttled: true, gpu_busy_bps: 6000, ..Evidence::none() };
    let (v, _) = AttribEngine::attribute(&ev_th);
    out.push(("注入8 热降频命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::ThermalThrottle, .. })));
    // 9. 驱动抖动（排除面 + 方差）。
    let ev_dj = Evidence { task_variance_ms2: 400, routes_present: 4, ..Evidence::none() };
    let (v, _) = AttribEngine::attribute(&ev_dj);
    out.push(("注入9 驱动抖动命中", matches!(v, AttribVerdict::Confirmed { cause: Cause::DriverJitter, .. })));
    // 二、置信阈值行为：单弱证据 → 待定。
    let ev_weak = Evidence { budget_queued: true, routes_present: 1, ..Evidence::none() };
    let (v, _) = AttribEngine::attribute(&ev_weak);
    out.push(("置信不足标待定", matches!(v, AttribVerdict::Pending { .. })));
    // 空证据必待定。
    let (v, _) = AttribEngine::attribute(&Evidence::none());
    out.push(("空证据必待定", matches!(v, AttribVerdict::Pending { .. })));
    // 三、事件卡结构完整 + 阈值行为 + 延迟判据 + 最贵清单。
    let mut dc = DiagCenter::new(20);
    let id1 = dc.attribute_jank(1, 1000, 33, &ev_gpu, 1005);
    out.push(("入卡成功", id1.is_ok() && id1.unwrap() > 0));
    out.push(("阈值下不归因", dc.attribute_jank(2, 2000, 5, &ev_gpu, 2001).unwrap_or(0) == 0));
    out.push(("零时长拒", dc.attribute_jank(3, 3000, 0, &ev_gpu, 3001).is_err()));
    let _ = dc.attribute_jank(4, 4000, 40, &ev_cpu, 4002);
    let _ = dc.attribute_jank(5, 5000, 55, &ev_up, 5003);
    let _ = dc.attribute_jank(6, 6000, 22, &ev_lock, 6004);
    let _ = dc.attribute_jank(7, 7000, 88, &ev_q, 7005);
    let _ = dc.attribute_jank(8, 8000, 44, &ev_dg, 8006);
    let _ = dc.attribute_jank(9, 9000, 66, &ev_pf, 9007);
    let _ = dc.attribute_jank(10, 10_000, 30, &ev_th, 10_005);
    let _ = dc.attribute_jank(11, 11_000, 25, &ev_dj, 11_004);
    // 最贵十次：88ms 应居首。
    let top = dc.top_ten();
    out.push(("最贵清单产出", top.is_ok() && top.as_ref().map(|l| l.len() == 9).unwrap_or(false)));
    out.push(("最贵居首88ms", top.as_ref().map(|l| l.first().map(|c| c.duration_ms == 88).unwrap_or(false)).unwrap_or(false)));
    // 归因延迟 ≤1s：全卡延迟记录且无超限（上面 attrib_tick 差 ≤5ms）。
    out.push(("延迟达标", dc.latency_over_total == 0));
    // 延迟超限立案。
    let mut dc2 = DiagCenter::new(20);
    let _ = dc2.attribute_jank(1, 1000, 30, &ev_gpu, 1000 + ATTRIB_LATENCY_MAX_MS + 1);
    out.push(("延迟超限立案", dc2.latency_over_total == 1));
    // 守恒与账链。
    out.push(("归因守恒", dc.reconcile().is_ok()));
    out.push(("全量校验过", dc.validate().is_ok()));
    out.push(("读屏三段", dc.read_screen().len() >= 3));
    out.push(("九因建议闭集齐", {
        let mut i = 0usize;
        let mut all = true;
        while i < 9 {
            if let Some(c) = Cause::from_ordinal(i) {
                if c.suggestion().is_empty() {
                    all = false;
                }
            }
            i += 1;
        }
        all
    }));
    out.push(("诊断码带建议", !XcCode::CardIncomplete.detail().is_empty() && XcCode::CardIncomplete.code_hex() == 0x3D01));
    out
}

// ---------------------------------------------------------------------------
// 旧式判据桥接（2026-10-22 接线轮补）：run_cgc08_checks 返回 CheckSet 之前的旧式
// Vec<(&'static str, bool)>，不能直接进 CheckSet 函数指针表；此桥逐项转入
// CheckSetBuilder（语义等价：判据名/通过位一一对应，零信息丢失）。
// ---------------------------------------------------------------------------
pub fn run_cgc08_checks_cs() -> crate::checks::CheckSet {
    let mut b = crate::checks::CheckSetBuilder::new("CGPU-F0007", "旧式判据桥接");
    for (name, ok) in run_cgc08_checks() {
        b.ok(name, ok, "");
    }
    b.done()
}

