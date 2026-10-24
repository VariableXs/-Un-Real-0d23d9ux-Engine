//! CGPU-F2087 · 效果性能预算（CGPU-N 域 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2087`
//!
//! **判据（锚点原文）**：预算表、处置复用、两组、判据。
//!
//! **职责定位（锚点原文摘录）**：性能预算：预算（效果性能预算（每效果帧成本
//! ——预算表（预算复用家族——预算复用；超标（超标处置复用——处置复用；
//! 测试（预算/超标两组）。
//!
//! --------------------------------------------------------------------------
//!
//! ## 〇、本模块全景（十一个子系统，全部真实实现，无桩）
//!
//! 本模块为 VE 图形效果建立**每效果帧成本预算**闭环：预算表（双刻度）→
//! 逐帧记账（守恒）→ 超标处置（三级复用族）→ 预算修正建议（只建议）→
//! 会话聚合（整数分位）。全部整数运算，零浮点；全部可失败路径走
//! `Result`。十一个子系统：
//!
//! 1. **效果闭集与预算复用家族**（[`EffectKind`] 16 种 / [`BudgetFamily`]
//!    4 族 + [`FAMILY_REGISTRY`]）：每效果归属一个预算复用家族（帧档/
//!    像素档/固定档/摊销档），家族注册表带单效果封顶与帧效果份额内的
//!    家族合计份额——**与 F2726 帧预算口径对齐镜像**（本地镜像，const
//!    注明来源，零 `use crate`）。
//! 2. **每效果帧成本预算表**（[`COST_TABLE`] + [`EffectCostEntry`]）：
//!    16 行 const 预算行，**µs 档 / 像素档双刻度**——帧档锚定合同预算，
//!    像素档随分辨率折算，预算取两腿保守大值；双刻度自洽性（参考分辨率
//!    下两腿差 ≤10%；固定档像素腿落在 20%~30% 带）编译期逐行断言。
//! 3. **表指纹冻结**（[`TABLE_FINGERPRINT`]）：16 行预算表 const 串行化
//!    后 FNV-1a 64 位指纹在编译期冻结，运行期每次校验复算——**预算表
//!    篡改一行一字节即指纹失配显性报错**（0x1DB4）。
//! 4. **每帧记账**（[`BudgetLedger`]）：600 帧环形账 + 逐效果分列累计；
//!    **守恒红线**——`Σ效果分项实耗 ≡ 帧总实耗`（0x1DB3），且未激活
//!    效果分项必须为零；帧序号单调递增（回退即拒，0x1DB6）。
//! 5. **超支率 ppm 记量**：帧级与效果级超支率一律百万分比整数
//!    （`(实际−预算)×1e6/预算`，饱和算术），无浮点。
//! 6. **超标处置复用**（[`OverrunHandler`]）：处置族**降档→跳帧→熔断**
//!    三级（留存 60%/50%/0%），**与 F0008 掉帧降质链同构**（本地镜像
//!    [`F0008_MIRROR`] + 对齐注记）；连击升级 / 滞回回落 / 熔断冷却后
//!    半开探测；外部处置申请必须"逐级 + 有据"（越权跨级 0x1DB7、无据
//!    0x1DB8）；**处置动作全量留痕**（[`DispAction`] 轨迹环）。
//! 7. **预算修正建议**（[`SuggestionEngine`]）：实测 P95 连续 3 个观测窗
//!    超预算 25% 以上 → 出**建议单**（新预算值 = P95×1.08 按 50µs 步进
//!    取整 + 家族封顶钳制标记 + 依据栏）；**只建议不自动改表**——表冻结
//!    红线，任何"自动应用"调用一律 0x1DBA 拒绝。
//! 8. **会话聚合**（[`SessionDigest`]）：P50/P95/P99 整数分位（ceil 索引
//!    法）、预算达成率（‱ 基点）、超标 Top 效果稳定排序（均值超支率降序
//!    + 效果号升序破平——全序确定）。
//! 9. **事件账**（[`EventLedger`]）：16 类事件定长环形账（128 槽，满则
//!    覆写最旧并计 dropped）+ 逐类计数器，环形扫描与计数器一致性体检。
//! 10. **health() 全量体检**（[`EffBudgetEngine::health`]）：表指纹复算 /
//!     记账守恒零错 / 环头尾一致 / 事件账自洽 / 处置轨迹逐级合法 / 建议
//!     单只读，六项检查逐条返回布尔清单。
//! 11. **门面与注入测试面**（[`EffBudgetEngine`]）：`run_frame` 帧闭环、
//!     双跑确定性对拍（`run_script_det`，指纹不一致即 0x1DBF）+ 三类
//!     注入检出面——**预算表篡改注入 / 处置越权注入 / 守恒破坏注入**
//!     （[`InjectionVerdict`]）。
//!
//! ## 〇.一、纪律落实
//!
//! - **零 panic 面**：内核路径零 `unwrap`/`expect`/`panic!`/运行期
//!   `assert!`；可失败全走 `Result`<T, [`EffBudgetError`]>（16 变体
//!   闭集，码段 **0x1DB0~0x1DBF** 独占，每变体 [`EffBudgetError::detail`]
//!   中文可操作诊断）。
//! - **码段复核留痕**：任务书首选 0x1CF0~0x1CFF 段——开工 grep 复核发现
//!   `unxstar/unx2622_upcidbook.rs` 已声明占用整段 `0x1CC0..0x1CFF`
//!   （其 `SEG_BASE=0x1CC0, SEG_MASK=0x003F`），按任务规则顺延改用
//!   **0x1DB0~0x1DBF**：grep `0x1DB` 全源零命中（邻段 0x1D80..0x1DAB
//!   被 unx2662 占、0x1D90..0x1D9F 被 cgm10 占，均不覆盖本段）。
//! - **no_std + alloc**：只依赖 `core` 与 `alloc::{vec::Vec, format,
//!   string::String}`；零 IO、零墙钟、零线程；完全自包含零 `use crate`。
//! - **溢出显性**：全部计数与聚合走 `saturating_add`/`saturating_sub`/
//!   `wrapping_*`；乘法前置量程论证（注释逐处给出上界）。
//! - **栈上禁区**：>64KB 结构体禁止上栈——600 帧记账环（62_400B）、
//!   事件环、处置轨迹全部 `Vec` 堆分配；编译期 `const` 断言逐类型封顶。
//! - **const fn 禁坑**：const fn 内不用 `slice::get`/`Ord::min/max`/
//!   derive 相等比较/`for` 循环——一律 `while` + 下标；枚举比较一律
//!   `as_u8()` 整型化；含 `Vec` 字段的类型一律不 derive `Copy`。
//! - **常量来源**：帧口径镜像 F2726（80 帧合同 → 12.5ms → 效果份额
//!   30% → 3.75ms）；成本单位镜像 F2086 `PERF_UNIT_NS = 1000ns`
//!   （vcn06_effbench）；处置族镜像 F0008 掉帧降质链（cgc09_degradechain
//!   L0 全质→L1 关效果→L2 参数减半→L3 禁用）；FNV-1a 常数（Landon
//!   Curt Noll 参考实现）；其余预算数值为本域合同写死值（注释逐条标明
//!   "合同值"）。
//!
//! **对接**：F2086（效果质量基准——成本单位 `PERF_UNIT_NS` 口径同源）、
//! F2085（效果与原生 API 映射——效果闭集上游）、F2088（效果降级链——
//! 处置族的降级执行侧）、F2726（安全渲染 80 帧合同——帧份额口径上游）。
//! 零 panic 面、零 IO、零墙钟、no_std。
//!
//! **测试两组**（锚点"测试（预算/超标两组）"）：预算组（预算表/指纹/
//! 双刻度/家族/记账守恒/建议单）与超标组（处置族/越权/注入检出/双跑
//! 一致），见文末 `mod tests`。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（全部合同写死值，逐条注明来源）
// ---------------------------------------------------------------------------

/// 本项版本（CGPU-N 域 · 效果性能预算）。
pub const EFFBUDGET_VERSION: &str = "CGPU-N-effbudget-v1";

/// 成本基准单位：纳秒（**镜像 F2086 `PERF_UNIT_NS = 1000ns` 口径**——
/// vcn06_effbench 同源；本模块直接以 ns 记账，µs 档表值 ×1000 入账）。
pub const PERF_UNIT_NS: u64 = 1_000;

/// 目标帧率：80 FPS（**镜像 F2726「安全渲染与 80 帧合同」**，合同值）。
pub const FRAME_RATE_TARGET: u32 = 80;

/// 帧时间预算：1e9/80 = 12.5ms（来源：F2726 帧合同镜像，合同值）。
pub const FRAME_TIME_NS: u64 = 12_500_000;

/// 效果总份额：帧时间预算的 30%（合同值：F2726 口径下效果子系统
/// 不得侵占超过三成帧时间）。
pub const EFFECT_SHARE_PERMILLE: u32 = 300;

/// 效果帧份额预算：12_500_000 × 300 / 1000 = 3_750_000ns = 3.75ms
/// （**F2726 帧预算口径对齐镜像锚点**；活跃效果预算合计的守门线）。
pub const EFFECT_SHARE_NS: u64 = 3_750_000;

/// 参考分辨率宽：1920（合同基准：1080p）。
pub const REF_WIDTH: u32 = 1920;

/// 参考分辨率高：1080。
pub const REF_HEIGHT: u32 = 1080;

/// 参考分辨率像素数：1920×1080 = 2_073_600。
pub const REF_PIXELS: u64 = 2_073_600;

/// 参考分辨率兆像素千分比：2_073_600/1e6 = 2.0736 MPx → 2074‰
/// （四舍五入取整；分辨率档一律用 ‰ 兆像素整数刻度，零浮点）。
pub const REF_MPX_PERMILLE: u32 = 2074;

/// 分辨率档下界：100‰ = 0.1MPx（小窗特效下限，合同值）。
pub const MPX_PERMILLE_MIN: u32 = 100;

/// 分辨率档上界：8294‰ = 8.294MPx（4K 3840×2160 = 8_294_400px，
/// 合同上界——超此档显性拒绝）。
pub const MPX_PERMILLE_MAX: u32 = 8294;

/// 记账环形账容量：600 帧（约 7.5 秒 @80FPS 回看窗；合同值）。
/// 600 × sizeof(FrameRecord=104B) = 62_400B ≤ 64KB，且整环在堆上。
pub const RING_CAP: usize = 600;

/// 单效果单帧实耗上限：2_000_000ns = 2ms（合同值：单效果不得吃掉超过
/// 单帧的 16%——超过即记账显性拒绝，防异常值污染统计）。
pub const COST_VALUE_MAX: u32 = 2_000_000;

/// 帧总实耗上限：16 × 2_000_000 = 32_000_000ns（量程上界论证：
/// Σ分项至多 16×COST_VALUE_MAX，u64 精确可表示，无溢出）。
pub const TOTAL_COST_MAX: u64 = 32_000_000;

/// 处置升级连击阈值：连续超支 3 帧 → L1 降档（合同值）。
pub const STREAK_TO_L1: u16 = 3;

/// 处置升级连击阈值：连续超支 6 帧 → L2 跳帧（合同值）。
pub const STREAK_TO_L2: u16 = 6;

/// 处置升级连击阈值：连续超支 10 帧 → L3 熔断（合同值）。
pub const STREAK_TO_L3: u16 = 10;

/// 滞回回落连击：连续预算内 4 帧 → 处置降一级（合同值：防抖滞回）。
pub const RECOVER_STREAK: u16 = 4;

/// 熔断冷却帧数：20 帧（合同值：熔断后冷却，随后半开探测）。
pub const BREAKER_FRAMES: u16 = 20;

/// 摊销家族摊销窗：16 帧（合同值：静态资源重建每 16 帧尖峰一次）。
pub const AMORT_WINDOW: u64 = 16;

/// 处置留存表（‰）：L0 全量 1000 / L1 降档 600（来源：F0008 L2
/// "模糊半径减半"约折成本 ×0.6 的整数化合同值）/ L2 跳帧 500（隔帧
/// 执行）/ L3 熔断 0（禁用）。const 断言单调不增。
pub const DISP_CUT_PERMILLE: [u32; 4] = [1_000, 600, 500, 0];

/// 建议单观测窗：100 帧（合同值）。
pub const SUGGEST_WINDOW: u64 = 100;

/// 建议单最小样本数：窗内活跃帧 ≥50 才计 P95（合同值）。
pub const SUGGEST_MIN_SAMPLES: usize = 50;

/// 建议单触发线：P95 超预算 25%（125_000 ppm，合同值）。
pub const SUGGEST_TRIGGER_PPM: u64 = 125_000;

/// 建议单连续热窗数：连续 3 窗过触发线才出单（合同值）。
pub const SUGGEST_HOT_WINDOWS: u8 = 3;

/// 建议单新预算余量：P95 基础上 +8%（80‰，合同值）。
pub const SUGGEST_HEADROOM_PERMILLE: u64 = 80;

/// 预算步进粒度：50µs（合同值：预算表与新预算单都按 50µs 对齐，
/// 家族封顶值亦为步进整数倍——const 断言）。
pub const BUDGET_STEP_NS: u64 = 50_000;

/// 处置轨迹容量：512 条（环形覆写 + dropped 计数；合同值）。
pub const TRAIL_CAP: usize = 512;

/// 事件账容量：128 槽（环形覆写 + dropped 计数；合同值）。
pub const EVENT_CAP: usize = 128;

/// 效果闭集大小。
pub const EFF_KIND_COUNT: usize = 16;

/// 场景闭集大小（镜像 F2095 N01 场景闭集口径的本地副本）。
pub const SCENE_COUNT: usize = 8;

// ---------------------------------------------------------------------------
// 二、诊断码（0x1DB0~0x1DBF 段独占：F2087 效果性能预算面）
// ---------------------------------------------------------------------------

/// 诊断码段基址（0x1DB0 段：开工 grep 复核空闲——0x1CF0~0x1CFF 段被
/// unx2622 声明占用后按任务规则顺延至此）。
pub const SEG_BASE: u16 = 0x1DB0;

/// 诊断码段掩码（`SEG_BASE | SEG_MASK = 0x1DBF` 为本段上界）。
pub const SEG_MASK: u16 = 0x000F;

/// 诊断码闭集（0x1DB0~0x1DBF 共 16 变体；每变体配
/// [`EffBudgetError::code`] 与 [`EffBudgetError::detail`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffBudgetError {
    /// 效果下标越界（闭集 0..16 之外显性拒绝）。
    EffectIndexRange,
    /// 成本数值越界（单效果 >2ms 或帧总 >32ms 的记账上限）。
    CostValueRange,
    /// 分辨率档参数越界（100‰..8294‰ 之外显性拒绝）。
    PixelScaleRange,
    /// 帧守恒破坏（Σ分项≠总账，或未激活效果分项非零）。
    FrameConservation,
    /// 预算表指纹失配（运行中预算表被篡改——表冻结红线命中）。
    TableFingerprintMismatch,
    /// 预算表冻结后非法改表（除注入面自愈外无写路径）。
    TableFrozen,
    /// 帧序号回退（记账序号必须严格单调递增——确定性守门）。
    FrameSeqRegression,
    /// 处置越权（跨级申请，如 L0 直跳 L3 熔断）。
    DispositionOverreach,
    /// 处置无据（对象未处于足够超支态——连击/累计依据不足）。
    DispositionNoGround,
    /// 处置级别非法（同级申请、方向非法或级别值越界）。
    DispositionLevelInvalid,
    /// 建议单只读红线（预算修正只建议不自动改表）。
    SuggestionApplyForbidden,
    /// 建议单重复（同效果已有未决建议单时重复出单）。
    SuggestionDuplicate,
    /// 分位窗口无样本（空窗取分位显性拒绝，不猜零）。
    WindowEmpty,
    /// 分位千分比越界（q 必须 0..=1000）。
    QuantilePermilleRange,
    /// 事件种类越界（事件闭集 0..16 之外显性拒绝）。
    EventKindRange,
    /// 双跑确定性漂移（同输入两次运行指纹不一致）。
    ReplayMismatch,
}

impl EffBudgetError {
    /// 变体序号（0..=15，段内偏移）。
    pub const fn index(self) -> u16 {
        match self {
            EffBudgetError::EffectIndexRange => 0,
            EffBudgetError::CostValueRange => 1,
            EffBudgetError::PixelScaleRange => 2,
            EffBudgetError::FrameConservation => 3,
            EffBudgetError::TableFingerprintMismatch => 4,
            EffBudgetError::TableFrozen => 5,
            EffBudgetError::FrameSeqRegression => 6,
            EffBudgetError::DispositionOverreach => 7,
            EffBudgetError::DispositionNoGround => 8,
            EffBudgetError::DispositionLevelInvalid => 9,
            EffBudgetError::SuggestionApplyForbidden => 10,
            EffBudgetError::SuggestionDuplicate => 11,
            EffBudgetError::WindowEmpty => 12,
            EffBudgetError::QuantilePermilleRange => 13,
            EffBudgetError::EventKindRange => 14,
            EffBudgetError::ReplayMismatch => 15,
        }
    }

    /// 诊断码（`SEG_BASE | index`，全段 0x1DB0..0x1DBF 一码一变体）。
    pub const fn code(self) -> u16 {
        SEG_BASE | (self.index() & SEG_MASK)
    }

    /// 变体名（进日志 / 导出串）。
    pub const fn name(self) -> &'static str {
        match self {
            EffBudgetError::EffectIndexRange => "EFFECT_INDEX_RANGE",
            EffBudgetError::CostValueRange => "COST_VALUE_RANGE",
            EffBudgetError::PixelScaleRange => "PIXEL_SCALE_RANGE",
            EffBudgetError::FrameConservation => "FRAME_CONSERVATION",
            EffBudgetError::TableFingerprintMismatch => "TABLE_FINGERPRINT_MISMATCH",
            EffBudgetError::TableFrozen => "TABLE_FROZEN",
            EffBudgetError::FrameSeqRegression => "FRAME_SEQ_REGRESSION",
            EffBudgetError::DispositionOverreach => "DISPOSITION_OVERREACH",
            EffBudgetError::DispositionNoGround => "DISPOSITION_NO_GROUND",
            EffBudgetError::DispositionLevelInvalid => "DISPOSITION_LEVEL_INVALID",
            EffBudgetError::SuggestionApplyForbidden => "SUGGESTION_APPLY_FORBIDDEN",
            EffBudgetError::SuggestionDuplicate => "SUGGESTION_DUPLICATE",
            EffBudgetError::WindowEmpty => "WINDOW_EMPTY",
            EffBudgetError::QuantilePermilleRange => "QUANTILE_PERMILLE_RANGE",
            EffBudgetError::EventKindRange => "EVENT_KIND_RANGE",
            EffBudgetError::ReplayMismatch => "REPLAY_MISMATCH",
        }
    }

    /// 中文可操作诊断（每变体一条：现象 → 定位 → 处置建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            EffBudgetError::EffectIndexRange => {
                "效果下标越界：效果闭集为 0..16，检查调用方掩码/下标来源，\
                 闭集外编号一律显性拒绝不做猜测映射"
            }
            EffBudgetError::CostValueRange => {
                "成本数值越界：单效果单帧实耗上限 2ms、帧总实耗上限 32ms，\
                 先核对采样单位是否为纳秒，异常值应在入账前由上游剔除"
            }
            EffBudgetError::PixelScaleRange => {
                "分辨率档越界：兆像素千分比必须在 100..=8294（0.1MPx..4K），\
                 检查分辨率换算是否误用像素原值或浮点残留"
            }
            EffBudgetError::FrameConservation => {
                "帧守恒破坏：Σ效果分项实耗必须恰等于帧总实耗，且未激活效果\
                 分项必须为零——检查采样汇聚处是否漏记/重记/错算总账"
            }
            EffBudgetError::TableFingerprintMismatch => {
                "预算表指纹失配：运行期预算表与编译期冻结指纹不一致，疑似\
                 篡改——冻结表后唯一合法写路径是注入面自愈，请审计写点"
            }
            EffBudgetError::TableFrozen => {
                "预算表已冻结：预算行只读，预算调整必须走建议单流程人工\
                 确认后重编表，禁止运行期热改"
            }
            EffBudgetError::FrameSeqRegression => {
                "帧序号回退：记账序号必须严格单调递增，检查多生产者是否\
                 乱序入账或重放旧帧"
            }
            EffBudgetError::DispositionOverreach => {
                "处置越权：处置族只允许逐级升级（降档→跳帧→熔断），跨级\
                 申请被拒——请按处置族状态机逐级申请"
            }
            EffBudgetError::DispositionNoGround => {
                "处置无据：申请升级时效果未处于足够超支态（连击或累计超支\
                 依据不足），先确认遥测样本归属正确"
            }
            EffBudgetError::DispositionLevelInvalid => {
                "处置级别非法：目标级别必须比当前级别高一级且在 L0..L3 \
                 内，同级/降级/越界申请一律拒绝"
            }
            EffBudgetError::SuggestionApplyForbidden => {
                "建议单只读红线：预算修正建议单只建议不自动改表，新预算\
                 需人工评审后重编预算表并重冻结指纹"
            }
            EffBudgetError::SuggestionDuplicate => {
                "建议单重复：同效果已有未决建议单，先确认（ack）旧单或等\
                 热窗条件失效后再出单"
            }
            EffBudgetError::WindowEmpty => {
                "分位窗口无样本：空窗取分位无意义，先确认效果在观测窗内\
                 至少活跃过一帧"
            }
            EffBudgetError::QuantilePermilleRange => {
                "分位千分比越界：q 必须在 0..=1000（如 500=P50、950=P95），\
                 检查是否误传百分比或 ppm"
            }
            EffBudgetError::EventKindRange => {
                "事件种类越界：事件闭集为 0..16，检查事件码来源是否被外部\
                 输入污染"
            }
            EffBudgetError::ReplayMismatch => {
                "双跑确定性漂移：同输入两次运行指纹不一致，说明存在隐式\
                 状态或未初始化读——禁止发布，逐子系统对拍定位"
            }
        }
    }
}

/// 段上界（`SEG_BASE | SEG_MASK`；错误码全落 0x1DB0..=0x1DBF）。
pub const ERR_SEG_END: u16 = SEG_BASE | SEG_MASK;

// ---------------------------------------------------------------------------
// 三、FNV-1a 与整数工具（const fn 安全子集：while + 下标，无 get/min/max）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位（来源：Landon Curt Noll FNV 参考实现；
/// offset 0xCBF29CE484222325 / prime 0x100000001B3）。const fn 逐字节。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 运行期增量 FNV-1a 64（指纹链：逐字段续喂，等价于拼接后一次哈希）。
#[derive(Debug, Clone)]
pub struct Fnv64 {
    h: u64,
}

impl Fnv64 {
    /// 新建（offset basis）。
    pub const fn new() -> Self {
        Fnv64 { h: 0xCBF2_9CE4_8422_2325 }
    }

    /// 续喂字节串。
    pub fn update(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            self.h ^= bytes[i] as u64;
            self.h = self.h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
    }

    /// 续喂一个 u64 字段（LE 定长 8 字节展开——确定性串行化）。
    pub fn u64_field(&mut self, v: u64) {
        let mut b = [0u8; 8];
        let mut j = 0;
        while j < 8 {
            b[j] = ((v >> (8 * j)) & 0xFF) as u8;
            j += 1;
        }
        self.update(&b);
    }

    /// 收尾输出指纹。
    pub const fn finish(self) -> u64 {
        self.h
    }
}

/// u64 数组 LE 打包进 64 字节定长缓冲（不足补零、超长截断——防尾随
/// 字段漂移进指纹；const fn，while + 下标）。
pub const fn pack_u64(fields: &[u64]) -> [u8; 64] {
    let mut buf = [0u8; 64];
    let mut off = 0;
    let mut i = 0;
    while i < fields.len() && off + 8 <= 64 {
        let v = fields[i];
        let mut j = 0;
        while j < 8 {
            buf[off + j] = ((v >> (8 * j)) & 0xFF) as u8;
            j += 1;
        }
        off += 8;
        i += 1;
    }
    buf
}

/// 整数分位（升序已排序切片；ceil 索引法：idx = ceil(n×q/1000) − 1，
/// 并压回 n−1——P95 语义为"至少 95% 样本 ≤ 该值"）。空窗/越界 q 显性
/// 拒绝，零浮点。
pub fn quantile_u64(sorted: &[u64], permille: u32) -> Result<u64, EffBudgetError> {
    if sorted.is_empty() {
        return Err(EffBudgetError::WindowEmpty);
    }
    if permille > 1000 {
        return Err(EffBudgetError::QuantilePermilleRange);
    }
    let n = sorted.len() as u64;
    // 量程：n ≤ 600、permille ≤ 1000 → 乘积 ≤ 600_000，u64 无溢出。
    let mut idx = (n * permille as u64 + 999) / 1000;
    if idx == 0 {
        idx = 0;
    } else {
        idx -= 1;
    }
    if idx >= n {
        idx = n - 1;
    }
    Ok(sorted[idx as usize])
}

/// 超支率 ppm：`(actual − budget) × 1e6 / budget`（actual ≤ budget 时
/// 为 0；budget == 0 且 actual > 0 时饱和 u32::MAX——"无预算却花钱"
/// 视为无穷超支；零浮点、零除零）。
pub fn overrun_ppm(actual: u64, budget: u64) -> u32 {
    if actual <= budget {
        return 0;
    }
    if budget == 0 {
        return u32::MAX;
    }
    let diff = actual - budget;
    // 量程：diff ≤ 2_000_000（单效果上限）或 ≤ 32_000_000（帧总），
    // ×1e6 ≤ 3.2e13 < 2^64，无溢出。
    let ppm = diff * 1_000_000 / budget;
    if ppm > u32::MAX as u64 {
        u32::MAX
    } else {
        ppm as u32
    }
}

// ---------------------------------------------------------------------------
// 四、效果闭集（16 种；与 F2085/F2086 效果闭集同口径的本地独立副本）
// ---------------------------------------------------------------------------

/// 效果闭集（16 种；完全自包含副本——名字表逐字比对，表外即拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    /// 高斯模糊（半径驱动，像素型成本）。
    GaussBlur,
    /// 盒式模糊（两次分离 pass，像素型）。
    BoxBlur,
    /// 泛光（亮部提取 + 多级模糊合成，帧档型）。
    Bloom,
    /// 投影（SDF 阴影，像素型）。
    DropShadow,
    /// 折射（背景采样偏移，帧档型）。
    Refraction,
    /// 亚克力（模糊 + 噪声 + 着色合成，像素型）。
    Acrylic,
    /// 云母（静态壁纸纹理拉伸——摊销型：重建尖峰帧间复用）。
    Mica,
    /// 色调映射（LUT 采样，像素型）。
    ToneMap,
    /// 色度上采样（4:2:0 → 4:4:4，像素型）。
    ChromaUpsample,
    /// 抖动（有序抖动矩阵，固定档）。
    Dither,
    /// 锐化（反锐化掩模，像素型）。
    Sharpen,
    /// 辉光（单级软光晕，帧档型）。
    Glow,
    /// 运动模糊（速度缓冲采样，像素型）。
    MotionBlur,
    /// 景深（CoC 加权合成，像素型）。
    DoF,
    /// 抗锯齿（后处理形态学 AA，像素型）。
    AntiAlias,
    /// 调色（全局色彩变换，固定档）。
    ColorGrade,
}

impl EffectKind {
    /// 闭集大小。
    pub const COUNT: usize = EFF_KIND_COUNT;

    /// 整型化编码（const fn 比较用）。
    pub const fn as_u8(self) -> u8 {
        match self {
            EffectKind::GaussBlur => 0,
            EffectKind::BoxBlur => 1,
            EffectKind::Bloom => 2,
            EffectKind::DropShadow => 3,
            EffectKind::Refraction => 4,
            EffectKind::Acrylic => 5,
            EffectKind::Mica => 6,
            EffectKind::ToneMap => 7,
            EffectKind::ChromaUpsample => 8,
            EffectKind::Dither => 9,
            EffectKind::Sharpen => 10,
            EffectKind::Glow => 11,
            EffectKind::MotionBlur => 12,
            EffectKind::DoF => 13,
            EffectKind::AntiAlias => 14,
            EffectKind::ColorGrade => 15,
        }
    }

    /// 中文名（汇报/事件注记）。
    pub const fn name(self) -> &'static str {
        match self {
            EffectKind::GaussBlur => "高斯模糊",
            EffectKind::BoxBlur => "盒式模糊",
            EffectKind::Bloom => "泛光",
            EffectKind::DropShadow => "投影",
            EffectKind::Refraction => "折射",
            EffectKind::Acrylic => "亚克力",
            EffectKind::Mica => "云母",
            EffectKind::ToneMap => "色调映射",
            EffectKind::ChromaUpsample => "色度上采样",
            EffectKind::Dither => "抖动",
            EffectKind::Sharpen => "锐化",
            EffectKind::Glow => "辉光",
            EffectKind::MotionBlur => "运动模糊",
            EffectKind::DoF => "景深",
            EffectKind::AntiAlias => "抗锯齿",
            EffectKind::ColorGrade => "调色",
        }
    }

    /// 闭集内反查（表外编号显性拒绝，不猜）。
    pub const fn from_u8(v: u8) -> Result<EffectKind, EffBudgetError> {
        if v < EFF_KIND_COUNT as u8 {
            Ok(match v {
                0 => EffectKind::GaussBlur,
                1 => EffectKind::BoxBlur,
                2 => EffectKind::Bloom,
                3 => EffectKind::DropShadow,
                4 => EffectKind::Refraction,
                5 => EffectKind::Acrylic,
                6 => EffectKind::Mica,
                7 => EffectKind::ToneMap,
                8 => EffectKind::ChromaUpsample,
                9 => EffectKind::Dither,
                10 => EffectKind::Sharpen,
                11 => EffectKind::Glow,
                12 => EffectKind::MotionBlur,
                13 => EffectKind::DoF,
                14 => EffectKind::AntiAlias,
                _ => EffectKind::ColorGrade,
            })
        } else {
            Err(EffBudgetError::EffectIndexRange)
        }
    }

    /// 活跃掩码位（bit i = 效果 i）。
    pub const fn bit(self) -> u16 {
        1u16 << (self.as_u8() as u32)
    }

    /// 由下标查名字（汇报用；越界返回 "?"——汇报路径不可失败）。
    pub fn name_of(idx: usize) -> &'static str {
        if idx >= EFF_KIND_COUNT {
            return "?";
        }
        match EffectKind::from_u8(idx as u8) {
            Ok(k) => k.name(),
            Err(_) => "?",
        }
    }
}

// ---------------------------------------------------------------------------
// 五、预算复用家族（4 族；F2726 帧预算口径对齐镜像——本地镜像零依赖）
// ---------------------------------------------------------------------------

/// 预算复用家族（每效果恰属一族；族决定封顶/份额/折算形状）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetFamily {
    /// 帧档家族：预算锚定帧合同份额，随分辨率线性折算。
    Frame,
    /// 像素家族：成本随像素数线性。
    Pixel,
    /// 固定家族：帧档不随分辨率折算（像素腿兜底防 4K+ 失控）。
    Fixed,
    /// 摊销家族：稳态预算为摊销值，每 16 帧一次尖峰（封顶钳制）。
    Amortized,
}

/// 家族整型编码（const fn 比较用）。
pub const FAMILY_CODE_FRAME: u8 = 0;
/// 家族整型编码：像素。
pub const FAMILY_CODE_PIXEL: u8 = 1;
/// 家族整型编码：固定。
pub const FAMILY_CODE_FIXED: u8 = 2;
/// 家族整型编码：摊销。
pub const FAMILY_CODE_AMORT: u8 = 3;

impl BudgetFamily {
    /// 整型化编码。
    pub const fn as_u8(self) -> u8 {
        match self {
            BudgetFamily::Frame => FAMILY_CODE_FRAME,
            BudgetFamily::Pixel => FAMILY_CODE_PIXEL,
            BudgetFamily::Fixed => FAMILY_CODE_FIXED,
            BudgetFamily::Amortized => FAMILY_CODE_AMORT,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            BudgetFamily::Frame => "帧档家族",
            BudgetFamily::Pixel => "像素家族",
            BudgetFamily::Fixed => "固定家族",
            BudgetFamily::Amortized => "摊销家族",
        }
    }

    /// 闭集内反查（表外拒绝）。
    pub const fn from_u8(v: u8) -> Result<BudgetFamily, EffBudgetError> {
        if v <= FAMILY_CODE_AMORT {
            Ok(match v {
                FAMILY_CODE_FRAME => BudgetFamily::Frame,
                FAMILY_CODE_PIXEL => BudgetFamily::Pixel,
                FAMILY_CODE_FIXED => BudgetFamily::Fixed,
                _ => BudgetFamily::Amortized,
            })
        } else {
            Err(EffBudgetError::EffectIndexRange)
        }
    }
}

/// 家族注册表行（预算复用家族声明）。
#[derive(Debug, Clone, Copy)]
pub struct FamilyDecl {
    /// 家族。
    pub family: BudgetFamily,
    /// 单效果预算封顶（ns；步进 50µs 对齐——const 断言）。
    pub cap_ns: u64,
    /// 帧效果份额内该家族合计份额（‰；四族 Σ=1000——const 断言）。
    pub share_permille: u32,
    /// 家族口径镜像注记（来源声明，F2726 对齐）。
    pub mirror: &'static str,
}

/// 预算复用家族注册表（**F2726 帧预算口径对齐镜像**：份额基数一律为
/// `EFFECT_SHARE_NS = 3.75ms = F2726 帧合同 12.5ms × 30%`；本表为本地
/// 镜像副本，与帧合同上游零编译依赖）。
pub const FAMILY_REGISTRY: [FamilyDecl; 4] = [
    FamilyDecl {
        family: BudgetFamily::Frame,
        cap_ns: 1_200_000,
        share_permille: 300,
        mirror: "F2726 镜像：效果份额 3.75ms×30%=1125µs 家族合计上限",
    },
    FamilyDecl {
        family: BudgetFamily::Pixel,
        cap_ns: 1_500_000,
        share_permille: 500,
        mirror: "F2726 镜像：效果份额 3.75ms×50%=1875µs 家族合计上限",
    },
    FamilyDecl {
        family: BudgetFamily::Fixed,
        cap_ns: 300_000,
        share_permille: 80,
        mirror: "F2726 镜像：效果份额 3.75ms×8%=300µs 家族合计上限",
    },
    FamilyDecl {
        family: BudgetFamily::Amortized,
        cap_ns: 2_400_000,
        share_permille: 120,
        mirror: "F2726 镜像：效果份额 3.75ms×12%=450µs 稳态合计上限\
                 （尖峰帧单效果封顶 2.4ms）",
    },
];

/// 预算复用声明清单（"预算复用家族——预算复用"锚点的落地声明：三处
/// 口径镜像 + 本地零依赖）。
pub const BUDGET_REUSE_DECL: [&str; 3] = [
    "家族份额口径镜像 F2726 帧合同：80FPS→12.5ms→效果份额 30%→3.75ms",
    "成本单位口径镜像 F2086 PERF_UNIT_NS=1000ns（vcn06_effbench 同源）",
    "处置族口径镜像 F0008 掉帧降质链：三级递进+熔断归零+回升滞回",
];

/// 按家族查注册表行（闭集内 while 查找；找不到返回 Frame 行兜底并
/// 由调用方上下文保证不触发——家族编码闭集 0..=3）。
pub const fn family_decl(f: BudgetFamily) -> FamilyDecl {
    let code = f.as_u8();
    let mut i = 0;
    while i < FAMILY_REGISTRY.len() {
        if FAMILY_REGISTRY[i].family.as_u8() == code {
            return FAMILY_REGISTRY[i];
        }
        i += 1;
    }
    FAMILY_REGISTRY[0]
}

/// 家族封顶（ns）。
pub const fn family_cap_ns(f: BudgetFamily) -> u64 {
    family_decl(f).cap_ns
}

/// 家族份额（‰）。
pub const fn family_share_permille(f: BudgetFamily) -> u32 {
    family_decl(f).share_permille
}

// ---------------------------------------------------------------------------
// 六、每效果帧成本预算表（16 行 const · µs 档/像素档双刻度 · 指纹冻结）
// ---------------------------------------------------------------------------

/// 预算表行（µs 档合同预算 + 像素档成本系数双刻度；全部 const 写死）。
#[derive(Debug, Clone, Copy)]
pub struct EffectCostEntry {
    /// 效果种类。
    pub kind: EffectKind,
    /// µs 档预算：参考分辨率（1080p=2074‰兆像素）下该效果每帧预算
    /// （µs；入账 ×1000 折 ns）。
    pub budget_us: u32,
    /// 像素档成本系数：每兆像素纳秒（分辨率折算腿）。
    pub per_mpx_ns: u32,
    /// 预算复用家族。
    pub family: BudgetFamily,
    /// 处置降档提示（处置复用提示位：1=降档敏感 / 2=跳帧敏感；
    /// 仅供处置族参考，不参与指纹外决策）。
    pub degrade_hint: u8,
}

/// 每效果帧成本预算表（16 行 const，合同写死值；行序=效果闭集序）。
///
/// 双刻度自洽（参考分辨率下）：
/// - 帧/像素/摊销族：`|budget_us×1000 − per_mpx_ns×2074/1000| ≤ 帧档/10`
///   （两腿互证，漂移 >10% 即编译期失败）；
/// - 固定族：像素腿刻意压到帧档的 20%~30% 带（成本不随像素走，
///   但 4K+ 档像素腿接管兜底）。
pub const COST_TABLE: [EffectCostEntry; EFF_KIND_COUNT] = [
    EffectCostEntry {
        kind: EffectKind::GaussBlur,
        budget_us: 420,
        per_mpx_ns: 202_507,
        family: BudgetFamily::Pixel,
        degrade_hint: 1,
    },
    EffectCostEntry {
        kind: EffectKind::BoxBlur,
        budget_us: 210,
        per_mpx_ns: 101_254,
        family: BudgetFamily::Pixel,
        degrade_hint: 1,
    },
    EffectCostEntry {
        kind: EffectKind::Bloom,
        budget_us: 350,
        per_mpx_ns: 168_756,
        family: BudgetFamily::Frame,
        degrade_hint: 2,
    },
    EffectCostEntry {
        kind: EffectKind::DropShadow,
        budget_us: 260,
        per_mpx_ns: 125_362,
        family: BudgetFamily::Pixel,
        degrade_hint: 1,
    },
    EffectCostEntry {
        kind: EffectKind::Refraction,
        budget_us: 480,
        per_mpx_ns: 231_437,
        family: BudgetFamily::Frame,
        degrade_hint: 2,
    },
    EffectCostEntry {
        kind: EffectKind::Acrylic,
        budget_us: 560,
        per_mpx_ns: 269_624,
        family: BudgetFamily::Pixel,
        degrade_hint: 1,
    },
    EffectCostEntry {
        kind: EffectKind::Mica,
        budget_us: 380,
        per_mpx_ns: 183_220,
        family: BudgetFamily::Amortized,
        degrade_hint: 1,
    },
    EffectCostEntry {
        kind: EffectKind::ToneMap,
        budget_us: 140,
        per_mpx_ns: 67_502,
        family: BudgetFamily::Pixel,
        degrade_hint: 1,
    },
    EffectCostEntry {
        kind: EffectKind::ChromaUpsample,
        budget_us: 120,
        per_mpx_ns: 57_860,
        family: BudgetFamily::Pixel,
        degrade_hint: 2,
    },
    EffectCostEntry {
        kind: EffectKind::Dither,
        budget_us: 60,
        per_mpx_ns: 7_233,
        family: BudgetFamily::Fixed,
        degrade_hint: 2,
    },
    EffectCostEntry {
        kind: EffectKind::Sharpen,
        budget_us: 150,
        per_mpx_ns: 72_304,
        family: BudgetFamily::Pixel,
        degrade_hint: 1,
    },
    EffectCostEntry {
        kind: EffectKind::Glow,
        budget_us: 240,
        per_mpx_ns: 115_720,
        family: BudgetFamily::Frame,
        degrade_hint: 1,
    },
    EffectCostEntry {
        kind: EffectKind::MotionBlur,
        budget_us: 330,
        per_mpx_ns: 159_115,
        family: BudgetFamily::Pixel,
        degrade_hint: 2,
    },
    EffectCostEntry {
        kind: EffectKind::DoF,
        budget_us: 520,
        per_mpx_ns: 250_724,
        family: BudgetFamily::Pixel,
        degrade_hint: 2,
    },
    EffectCostEntry {
        kind: EffectKind::AntiAlias,
        budget_us: 450,
        per_mpx_ns: 216_972,
        family: BudgetFamily::Pixel,
        degrade_hint: 2,
    },
    EffectCostEntry {
        kind: EffectKind::ColorGrade,
        budget_us: 90,
        per_mpx_ns: 10_849,
        family: BudgetFamily::Fixed,
        degrade_hint: 1,
    },
];

/// 预算表串行化定长（指纹输入：每行 1+4+4+1+1 = 11 字节 × 16 行）。
pub const TABLE_SER_ROW: usize = 11;

/// 预算表串行化总长。
pub const TABLE_SER_LEN: usize = TABLE_SER_ROW * EFF_KIND_COUNT;

/// 预算表行串行化（kind/budget_us/per_mpx_ns/family/hint 五字段 LE 定长
/// 拼接；const fn，while + 下标，无越界——off 逐行推进恰 ≤ 总长）。
pub const fn serialize_row(row: &EffectCostEntry, out: &mut [u8; TABLE_SER_LEN], slot: usize) {
    let base = slot * TABLE_SER_ROW;
    out[base] = row.kind.as_u8();
    let bu = row.budget_us as u64;
    let pm = row.per_mpx_ns as u64;
    let mut j = 0;
    while j < 4 {
        out[base + 1 + j] = ((bu >> (8 * j)) & 0xFF) as u8;
        out[base + 5 + j] = ((pm >> (8 * j)) & 0xFF) as u8;
        j += 1;
    }
    out[base + 9] = row.family.as_u8();
    out[base + 10] = row.degrade_hint;
}

/// const 表全量串行化（编译期执行）。
pub const fn serialize_cost_table() -> [u8; TABLE_SER_LEN] {
    let mut out = [0u8; TABLE_SER_LEN];
    let mut i = 0;
    while i < EFF_KIND_COUNT {
        serialize_row(&COST_TABLE[i], &mut out, i);
        i += 1;
    }
    out
}

/// 运行期行集串行化（与 [`serialize_cost_table`] 同构——布局常量共享，
/// 交叉校验测试对拍两实现输出一致）。
pub fn serialize_rows(rows: &[EffectCostEntry]) -> [u8; TABLE_SER_LEN] {
    let mut out = [0u8; TABLE_SER_LEN];
    let mut i = 0;
    while i < rows.len() && i < EFF_KIND_COUNT {
        serialize_row(&rows[i], &mut out, i);
        i += 1;
    }
    out
}

/// **表指纹冻结**：16 行预算表串行化后 FNV-1a 64 位指纹，编译期算定。
/// 运行期任何一行一字节被改，复算即失配（0x1DB4）。
pub const TABLE_FINGERPRINT: u64 = fnv1a64(&serialize_cost_table());

/// 工作预算表（运行期只读副本 + 冻结指纹；唯一写路径是注入面自愈）。
#[derive(Debug, Clone)]
pub struct BudgetTable {
    rows: Vec<EffectCostEntry>,
    frozen_fp: u64,
    tamper_marks: u64,
}

impl BudgetTable {
    /// 由 const 表构建工作副本并冻结指纹。
    pub fn new() -> Self {
        let mut rows = Vec::new();
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            rows.push(COST_TABLE[i]);
            i += 1;
        }
        BudgetTable { rows, frozen_fp: TABLE_FINGERPRINT, tamper_marks: 0 }
    }

    /// 复算指纹并与冻结值比对（失配 = 篡改，0x1DB4）。
    pub fn verify(&self) -> Result<u64, EffBudgetError> {
        let fp = fnv1a64(&serialize_rows(&self.rows));
        if fp == self.frozen_fp {
            Ok(fp)
        } else {
            Err(EffBudgetError::TableFingerprintMismatch)
        }
    }

    /// 冻结指纹值（汇报用）。
    pub const fn frozen_fingerprint(&self) -> u64 {
        self.frozen_fp
    }

    /// 历史篡改标记数（注入面写入后自愈恢复，标记只增不减——审计留痕）。
    pub const fn tamper_marks(&self) -> u64 {
        self.tamper_marks
    }

    /// 取行（闭集内；越界显性拒绝）。
    pub fn row(&self, idx: usize) -> Result<&EffectCostEntry, EffBudgetError> {
        if idx >= self.rows.len() || idx >= EFF_KIND_COUNT {
            return Err(EffBudgetError::EffectIndexRange);
        }
        Ok(&self.rows[idx])
    }

    /// 行数（恒 16）。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空（恒非空——const 表 16 行）。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 单效果预算（ns）——**双刻度保守取大**：
    /// - 帧档腿：帧/像素/摊销族随分辨率 `mpx‰/2074‰` 线性折算；固定族
    ///   不折算（合同预算即帧档）；
    /// - 像素档腿：`per_mpx_ns × mpx‰ / 1000` 恒随分辨率；
    /// - 摊销族尖峰帧（`seq % 16 == 0` 且 `seq > 0`）：稳态预算 ×16 并
    ///   家族封顶钳制（Mica：380µs×16=6.08ms → 钳 2.4ms，const 断言）。
    ///
    /// 量程：budget_us ≤ 560 → 帧档腿 ≤ 560_000×8294/2074 ≈ 2.24e6，
    /// saturating_mul 后除法，u64 无溢出。
    pub fn budget_ns_at(
        &self,
        idx: usize,
        mpx_permille: u32,
        seq: u64,
    ) -> Result<u64, EffBudgetError> {
        if mpx_permille < MPX_PERMILLE_MIN || mpx_permille > MPX_PERMILLE_MAX {
            return Err(EffBudgetError::PixelScaleRange);
        }
        let row = self.row(idx)?;
        let base = row.budget_us as u64 * 1_000;
        let frame_leg = if row.family.as_u8() == FAMILY_CODE_FIXED {
            base
        } else {
            base.saturating_mul(mpx_permille as u64) / REF_MPX_PERMILLE as u64
        };
        let pixel_leg = row.per_mpx_ns as u64 * mpx_permille as u64 / 1_000;
        let mut budget = if frame_leg > pixel_leg { frame_leg } else { pixel_leg };
        if row.family.as_u8() == FAMILY_CODE_AMORT && seq > 0 && seq % AMORT_WINDOW == 0 {
            let spike = budget.saturating_mul(AMORT_WINDOW);
            let cap = family_cap_ns(row.family);
            if spike > cap {
                budget = cap;
            } else {
                budget = spike;
            }
        }
        Ok(budget)
    }

    /// 活跃掩码的帧预算合计（ns）+ 份额越限标志（Σ活跃预算 >
    /// `EFFECT_SHARE_NS` 即份额越限——合法超载态，交处置族处理而非报错）。
    pub fn frame_budget(
        &self,
        active_mask: u16,
        mpx_permille: u32,
        seq: u64,
    ) -> Result<(u64, bool), EffBudgetError> {
        let mut total: u64 = 0;
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            if (active_mask >> i) & 1 == 1 {
                total = total.saturating_add(self.budget_ns_at(i, mpx_permille, seq)?);
            }
            i += 1;
        }
        let over = total > EFFECT_SHARE_NS;
        Ok((total, over))
    }

    /// 活跃掩码内某家族预算合计（µs，参考分辨率口径；份额体检用）。
    pub fn family_budget_us(&self, active_mask: u16, f: BudgetFamily) -> u64 {
        let mut total: u64 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if (active_mask >> i) & 1 == 1 && self.rows[i].family.as_u8() == f.as_u8() {
                total += self.rows[i].budget_us as u64;
            }
            i += 1;
        }
        total
    }

    /// 注入面专用写行（**唯一写路径**：预算表篡改注入的落笔点 + 自愈
    /// 恢复点；写入后 `verify()` 必失配——这就是 0x1DB4 检出机制）。
    pub fn debug_write_row(
        &mut self,
        idx: usize,
        fake_budget_us: u32,
    ) -> Result<(), EffBudgetError> {
        if idx >= self.rows.len() {
            return Err(EffBudgetError::EffectIndexRange);
        }
        self.rows[idx].budget_us = fake_budget_us;
        self.tamper_marks = self.tamper_marks.saturating_add(1);
        Ok(())
    }

    /// 注入面自愈：整表恢复 const 原表（并复核指纹必须回到冻结值）。
    pub fn heal_from_const(&mut self) -> Result<u64, EffBudgetError> {
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            self.rows[i] = COST_TABLE[i];
            i += 1;
        }
        self.verify()
    }

    /// 预算表文本汇报（µs 档/像素档/家族/处置提示逐行 + 指纹；紧凑定界）。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("== CGPU-F2087 效果性能预算表 ==\n");
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            s.push_str(&format!(
                "{:02} {:<14} 预算 {:>4}µs 像素档 {:>7}ns/MPx {:<6} 提示{}\n",
                i,
                EffectKind::name_of(i),
                r.budget_us,
                r.per_mpx_ns,
                r.family.name(),
                r.degrade_hint
            ));
            i += 1;
        }
        s.push_str(&format!(
            "指纹 0x{:016X}（冻结）份额 {}µs 级联 {}\n",
            self.frozen_fp,
            EFFECT_SHARE_NS / 1_000,
            EFFBUDGET_VERSION
        ));
        s
    }
}

// ---------------------------------------------------------------------------
// 七、场景闭集（8 条；F2095 N01 场景口径本地副本）+ 编译期断言群
// ---------------------------------------------------------------------------

/// 场景掩码闭集（8 条合同场景；bit i = 效果 i 激活）。
pub const SCENE_MASKS: [u16; SCENE_COUNT] = [
    EffectKind::Dither.bit() | EffectKind::ColorGrade.bit(),
    EffectKind::Acrylic.bit()
        | EffectKind::GaussBlur.bit()
        | EffectKind::Dither.bit()
        | EffectKind::ColorGrade.bit(),
    EffectKind::Mica.bit()
        | EffectKind::ToneMap.bit()
        | EffectKind::Dither.bit()
        | EffectKind::ColorGrade.bit(),
    EffectKind::Bloom.bit()
        | EffectKind::Glow.bit()
        | EffectKind::ToneMap.bit()
        | EffectKind::Dither.bit(),
    EffectKind::MotionBlur.bit()
        | EffectKind::AntiAlias.bit()
        | EffectKind::ToneMap.bit()
        | EffectKind::Dither.bit()
        | EffectKind::ColorGrade.bit(),
    EffectKind::DoF.bit()
        | EffectKind::AntiAlias.bit()
        | EffectKind::GaussBlur.bit()
        | EffectKind::ToneMap.bit()
        | EffectKind::Dither.bit(),
    EffectKind::DropShadow.bit()
        | EffectKind::Sharpen.bit()
        | EffectKind::ChromaUpsample.bit()
        | EffectKind::Dither.bit()
        | EffectKind::ColorGrade.bit(),
    EffectKind::Refraction.bit()
        | EffectKind::GaussBlur.bit()
        | EffectKind::Bloom.bit()
        | EffectKind::Dither.bit()
        | EffectKind::ColorGrade.bit(),
];

/// 场景名（与 [`SCENE_MASKS`] 同序）。
pub const SCENE_NAMES: [&str; SCENE_COUNT] = [
    "S0 桌面静置",
    "S1 亚克力窗口",
    "S2 云母窗口",
    "S3 泛光高亮",
    "S4 运动场景",
    "S5 景深特写",
    "S6 投影层叠",
    "S7 折射玻璃",
];

/// const fn：场景活跃预算合计（µs，参考分辨率口径）。
const fn scene_budget_us(mask: u16) -> u64 {
    let mut total: u64 = 0;
    let mut i = 0;
    while i < EFF_KIND_COUNT {
        if (mask >> i) & 1 == 1 {
            total += COST_TABLE[i].budget_us as u64;
        }
        i += 1;
    }
    total
}

/// const fn：场景内某家族预算合计（µs）。
const fn scene_family_budget_us(mask: u16, fam_code: u8) -> u64 {
    let mut total: u64 = 0;
    let mut i = 0;
    while i < EFF_KIND_COUNT {
        if (mask >> i) & 1 == 1 && COST_TABLE[i].family.as_u8() == fam_code {
            total += COST_TABLE[i].budget_us as u64;
        }
        i += 1;
    }
    total
}

/// const fn：全表预算合计（µs）——"全效果同帧全开"的对照值。
const fn full_blast_budget_us() -> u64 {
    scene_budget_us(0xFFFF)
}

/// const fn：双刻度自洽检查（逐行；固定族走 20%~30% 带，其余族走
/// ±10% 带；返回 false 即编译期断言失败）。
const fn dual_scale_all_ok() -> bool {
    let mut i = 0;
    while i < EFF_KIND_COUNT {
        let row = COST_TABLE[i];
        let frame_leg = row.budget_us as u64 * 1_000;
        let pixel_leg = row.per_mpx_ns as u64 * REF_MPX_PERMILLE as u64 / 1_000;
        let ok = if row.family.as_u8() == FAMILY_CODE_FIXED {
            // 固定族：像素腿应在帧档 20%~30% 带（下限防"无兜底"，
            // 上限防"名不副实的固定档"）。
            pixel_leg * 10 >= frame_leg * 2 && pixel_leg * 10 <= frame_leg * 3
        } else {
            // 其余族：两腿差 ≤ 帧档/10 + 1。
            let diff = if frame_leg > pixel_leg {
                frame_leg - pixel_leg
            } else {
                pixel_leg - frame_leg
            };
            diff <= frame_leg / 10 + 1
        };
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

/// const fn：每行预算 ≤ 家族封顶（参考分辨率帧档口径）。
const fn family_caps_ok() -> bool {
    let mut i = 0;
    while i < EFF_KIND_COUNT {
        let row = COST_TABLE[i];
        if row.budget_us as u64 * 1_000 > family_cap_ns(row.family) {
            return false;
        }
        i += 1;
    }
    true
}

/// const fn：摊销族尖峰钳制检查（Mica 380µs×16 = 6.08ms → 钳 2.4ms）。
const fn amort_spike_clamped() -> bool {
    let mut i = 0;
    while i < EFF_KIND_COUNT {
        let row = COST_TABLE[i];
        if row.family.as_u8() == FAMILY_CODE_AMORT {
            let spike = row.budget_us as u64 * 1_000 * AMORT_WINDOW;
            if spike <= family_cap_ns(row.family) {
                return false; // 未触钳制说明尖峰模型失真（本域合同要求触钳）。
            }
        }
        i += 1;
    }
    true
}

/// const fn：8 条场景预算合计全部 ≤ 效果份额（3750µs）。
const fn scenes_within_share() -> bool {
    let mut s = 0;
    while s < SCENE_COUNT {
        if scene_budget_us(SCENE_MASKS[s]) * 1_000 > EFFECT_SHARE_NS {
            return false;
        }
        s += 1;
    }
    true
}

/// const fn：8 条场景的各家族合计 ≤ 家族份额（换算 µs 比较）。
const fn scene_family_share_ok() -> bool {
    let mut s = 0;
    while s < SCENE_COUNT {
        let mask = SCENE_MASKS[s];
        let mut f = 0;
        while f < FAMILY_REGISTRY.len() {
            let share_us = EFFECT_SHARE_NS * FAMILY_REGISTRY[f].share_permille as u64
                / 1_000
                / 1_000;
            if scene_family_budget_us(mask, FAMILY_REGISTRY[f].family.as_u8()) > share_us {
                return false;
            }
            f += 1;
        }
        s += 1;
    }
    true
}

/// const fn：四族份额 Σ = 1000‰。
const fn family_shares_sum_ok() -> bool {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < FAMILY_REGISTRY.len() {
        sum += FAMILY_REGISTRY[i].share_permille;
        i += 1;
    }
    sum == 1_000
}

/// const fn：家族封顶全部 50µs 步进对齐（建议单封顶对齐的前提）。
const fn family_caps_step_aligned() -> bool {
    let mut i = 0;
    while i < FAMILY_REGISTRY.len() {
        if FAMILY_REGISTRY[i].cap_ns % BUDGET_STEP_NS != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// const fn：处置留存单调不增（1000 ≥ 600 ≥ 500 ≥ 0）。
const fn disp_cut_monotonic() -> bool {
    DISP_CUT_PERMILLE[0] >= DISP_CUT_PERMILLE[1]
        && DISP_CUT_PERMILLE[1] >= DISP_CUT_PERMILLE[2]
        && DISP_CUT_PERMILLE[2] >= DISP_CUT_PERMILLE[3]
}

// 编译期断言群（失败即编译错误——合同红线前移到编译期）。
const _: () = assert!(EFF_KIND_COUNT == 16, "效果闭集恰 16 种");
const _: () = assert!(FRAME_TIME_NS == 12_500_000, "F2726 镜像：12.5ms");
const _: () = assert!(EFFECT_SHARE_NS == 3_750_000, "F2726 镜像：效果份额 3.75ms");
const _: () = assert!(REF_PIXELS == REF_WIDTH as u64 * REF_HEIGHT as u64, "参考分辨率");
const _: () = assert!(dual_scale_all_ok(), "预算表双刻度自洽失败");
const _: () = assert!(family_caps_ok(), "预算行超家族封顶");
const _: () = assert!(amort_spike_clamped(), "摊销尖峰必须触家族钳制");
const _: () = assert!(scenes_within_share(), "场景预算超效果份额");
const _: () = assert!(scene_family_share_ok(), "场景家族合计超家族份额");
const _: () = assert!(family_shares_sum_ok(), "四族份额 Σ 必须 1000‰");
const _: () = assert!(family_caps_step_aligned(), "家族封顶须 50µs 对齐");
const _: () = assert!(disp_cut_monotonic(), "处置留存必须单调不增");
const _: () = assert!(
    full_blast_budget_us() * 1_000 > EFFECT_SHARE_NS,
    "全效果全开必须为非法配置（对照断言）"
);

// ---------------------------------------------------------------------------
// 八、每帧记账（BudgetLedger：600 帧环形 + 逐效果分列 + 守恒红线）
// ---------------------------------------------------------------------------

/// 帧记账记录（104B；600 帧整环 62_400B 在堆上——见 const 断言）。
#[derive(Debug, Clone, Copy)]
pub struct FrameRecord {
    /// 帧序号（严格单调递增）。
    pub seq: u64,
    /// 分辨率档（‰ 兆像素）。
    pub mpx_permille: u32,
    /// 活跃效果掩码。
    pub active_mask: u16,
    /// 帧预算合计（ns，双刻度保守大值合计）。
    pub budget_ns: u64,
    /// 帧总实耗（ns；**守恒：= Σper_effect 活跃分项**）。
    pub actual_ns: u64,
    /// 逐效果实耗分列（ns；未激活效果恒 0）。
    pub per_effect: [u32; EFF_KIND_COUNT],
    /// 帧级超支率 ppm。
    pub over_ppm: u32,
    /// 标志位：bit0 帧超支 / bit1 份额越限 / bit2 处置作用帧。
    pub flags: u16,
}

// 记账环体量封顶（104B × 600 = 62_400B ≤ 64KB，且整环在堆上——
// 满足">64KB 结构体禁上栈"纪律的对照断言）。
const _: () = assert!(
    RING_CAP * core::mem::size_of::<FrameRecord>() <= 65_536,
    "记账环总字节须 ≤ 64KB"
);

/// 帧入账结果。
#[derive(Debug, Clone, Copy)]
pub struct CommitInfo {
    /// 帧序号。
    pub seq: u64,
    /// 帧级超支率 ppm。
    pub over_ppm: u32,
    /// 是否份额越限（Σ活跃预算 > 3.75ms）。
    pub share_exceeded: bool,
    /// 本帧是否触发环形覆写（最旧帧被挤出）。
    pub wrapped: bool,
}

/// 每帧记账账本（环形 600 帧 + 逐效果累计分列）。
#[derive(Debug, Clone)]
pub struct BudgetLedger {
    ring: Vec<FrameRecord>,
    head: u64,
    started: bool,
    last_seq: u64,
    /// 守恒红线命中次数（注入面/上游 bug 显性计数）。
    pub conservation_errs: u64,
    /// 序号回退拒绝次数。
    pub seq_rejects: u64,
    /// 逐效果累计实耗（ns，饱和加）。
    pub per_effect_cum: [u64; EFF_KIND_COUNT],
    /// 逐效果活跃帧数。
    pub per_effect_frames: [u64; EFF_KIND_COUNT],
    /// 逐效果超支帧数。
    pub per_effect_over: [u64; EFF_KIND_COUNT],
    /// 逐效果超支率 ppm 累计（均值 = 累计/超支帧数）。
    pub per_effect_ppm_sum: [u64; EFF_KIND_COUNT],
    /// 帧级超支帧数。
    pub over_frames: u64,
    /// 份额越限帧数。
    pub share_exceed_frames: u64,
    /// 历史帧预算合计（ns，饱和）。
    pub budget_cum_ns: u64,
    /// 历史帧实耗合计（ns，饱和）。
    pub actual_cum_ns: u64,
}

impl BudgetLedger {
    /// 空账本（环延迟分配，首次入账起堆上生长至 600 封顶）。
    pub const fn new() -> Self {
        BudgetLedger {
            ring: Vec::new(),
            head: 0,
            started: false,
            last_seq: 0,
            conservation_errs: 0,
            seq_rejects: 0,
            per_effect_cum: [0; EFF_KIND_COUNT],
            per_effect_frames: [0; EFF_KIND_COUNT],
            per_effect_over: [0; EFF_KIND_COUNT],
            per_effect_ppm_sum: [0; EFF_KIND_COUNT],
            over_frames: 0,
            share_exceed_frames: 0,
            budget_cum_ns: 0,
            actual_cum_ns: 0,
        }
    }

    /// 已入账帧数。
    pub const fn head(&self) -> u64 {
        self.head
    }

    /// 环内现存帧数（≤ 600）。
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// 是否空环。
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// 被挤出环的历史帧数（`head − 环内现存`）。
    pub fn dropped_frames(&self) -> u64 {
        self.head.saturating_sub(self.ring.len() as u64)
    }

    /// 帧入账（**守恒红线**在此把关）：
    /// 1. 分列长度必须 16（0x1DB0）；
    /// 2. 序号严格递增（回退 0x1DB6）；
    /// 3. 单值 ≤ 2ms、总账 ≤ 32ms（0x1DB1）；
    /// 4. 未激活分项必须为零、`Σ分项 ≡ 总账`（0x1DB3）。
    pub fn commit(
        &mut self,
        seq: u64,
        mpx_permille: u32,
        active_mask: u16,
        per_effect: &[u32],
        declared_total_ns: u64,
        budget_ns: u64,
        over_mask: u16,
        per_effect_over_ppm: &[u32; EFF_KIND_COUNT],
        share_exceeded: bool,
        disp_mask: u16,
    ) -> Result<CommitInfo, EffBudgetError> {
        if per_effect.len() != EFF_KIND_COUNT {
            return Err(EffBudgetError::EffectIndexRange);
        }
        if self.started && seq <= self.last_seq {
            self.seq_rejects = self.seq_rejects.saturating_add(1);
            return Err(EffBudgetError::FrameSeqRegression);
        }
        if declared_total_ns > TOTAL_COST_MAX {
            return Err(EffBudgetError::CostValueRange);
        }
        let mut computed: u64 = 0;
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            let v = per_effect[i];
            if v > COST_VALUE_MAX {
                return Err(EffBudgetError::CostValueRange);
            }
            if v > 0 && (active_mask >> i) & 1 == 0 {
                self.conservation_errs = self.conservation_errs.saturating_add(1);
                return Err(EffBudgetError::FrameConservation);
            }
            computed = computed.saturating_add(v as u64);
            i += 1;
        }
        if computed != declared_total_ns {
            self.conservation_errs = self.conservation_errs.saturating_add(1);
            return Err(EffBudgetError::FrameConservation);
        }
        let ppm = overrun_ppm(declared_total_ns, budget_ns);
        let mut flags: u16 = 0;
        if ppm > 0 {
            flags |= 1;
        }
        if share_exceeded {
            flags |= 2;
        }
        if disp_mask != 0 {
            flags |= 4;
        }
        let rec = FrameRecord {
            seq,
            mpx_permille,
            active_mask,
            budget_ns,
            actual_ns: declared_total_ns,
            per_effect: {
                let mut a = [0u32; EFF_KIND_COUNT];
                let mut j = 0;
                while j < EFF_KIND_COUNT {
                    a[j] = per_effect[j];
                    j += 1;
                }
                a
            },
            over_ppm: ppm,
            flags,
        };
        // 环写入：未满 push，满则覆写最旧槽（head % 600）。
        if self.ring.len() < RING_CAP {
            self.ring.push(rec);
        } else {
            let slot = (self.head % RING_CAP as u64) as usize;
            self.ring[slot] = rec;
        }
        let wrapped = self.head >= RING_CAP as u64;
        self.head = self.head.saturating_add(1);
        self.started = true;
        self.last_seq = seq;
        // 累计分列（全部饱和）。
        let mut k = 0;
        while k < EFF_KIND_COUNT {
            if (active_mask >> k) & 1 == 1 {
                self.per_effect_frames[k] = self.per_effect_frames[k].saturating_add(1);
                self.per_effect_cum[k] =
                    self.per_effect_cum[k].saturating_add(per_effect[k] as u64);
                if (over_mask >> k) & 1 == 1 {
                    self.per_effect_over[k] = self.per_effect_over[k].saturating_add(1);
                    self.per_effect_ppm_sum[k] = self
                        .per_effect_ppm_sum[k]
                        .saturating_add(per_effect_over_ppm[k] as u64);
                }
            }
            k += 1;
        }
        if ppm > 0 {
            self.over_frames = self.over_frames.saturating_add(1);
        }
        if share_exceeded {
            self.share_exceed_frames = self.share_exceed_frames.saturating_add(1);
        }
        self.budget_cum_ns = self.budget_cum_ns.saturating_add(budget_ns);
        self.actual_cum_ns = self.actual_cum_ns.saturating_add(declared_total_ns);
        Ok(CommitInfo { seq, over_ppm: ppm, share_exceeded, wrapped })
    }

    /// 时间序读取环内第 k 帧（k=0 最旧；越界 None——显式可失败读）。
    pub fn record_at(&self, k: usize) -> Option<FrameRecord> {
        if k >= self.ring.len() {
            return None;
        }
        let len = self.ring.len();
        let start = if len < RING_CAP {
            0
        } else {
            (self.head % RING_CAP as u64) as usize
        };
        Some(self.ring[(start + k) % len])
    }

    /// 效果近窗实耗样本（时间序，至多 window 条活跃帧；建议单 P95 用）。
    pub fn window_actuals(&self, effect: usize, window: usize) -> Vec<u64> {
        let mut out = Vec::new();
        if effect >= EFF_KIND_COUNT {
            return out;
        }
        let mut taken = 0;
        let mut k = self.ring.len();
        while k > 0 && taken < window {
            let rec = match self.record_at(k - 1) {
                Some(r) => r,
                None => break,
            };
            if (rec.active_mask >> effect) & 1 == 1 {
                out.push(rec.per_effect[effect] as u64);
                taken += 1;
            }
            k -= 1;
        }
        out
    }

    /// 帧总实耗近窗样本（时间序，至多 window 条）。
    pub fn window_totals(&self, window: usize) -> Vec<u64> {
        let mut out = Vec::new();
        let mut k = self.ring.len();
        while k > 0 && out.len() < window {
            if let Some(r) = self.record_at(k - 1) {
                out.push(r.actual_ns);
            }
            k -= 1;
        }
        out
    }

    /// 环扫描一致性（体检用）：重算环内超支标志与存储标志一致，且环内
    /// 超支帧数 ≤ 累计超支帧数（累计含被挤出历史帧）。
    pub fn scan_consistent(&self) -> (bool, u64) {
        let mut ok = true;
        let mut over_in_ring: u64 = 0;
        let mut k = 0;
        while k < self.ring.len() {
            if let Some(r) = self.record_at(k) {
                let ppm = overrun_ppm(r.actual_ns, r.budget_ns);
                if (ppm > 0) != ((r.flags & 1) != 0) {
                    ok = false;
                }
                if ppm > 0 {
                    over_in_ring += 1;
                }
            }
            k += 1;
        }
        if over_in_ring > self.over_frames {
            ok = false;
        }
        (ok, over_in_ring)
    }

    /// 环纯净体检：逐存储记录复验守恒（`Σ分项 ≡ 总账` 且未激活分项为
    /// 零）——被拒绝的破坏帧从未入环，环内记录必须全部纯净。
    pub fn ring_pure(&self) -> bool {
        let mut k = 0;
        while k < self.ring.len() {
            if let Some(r) = self.record_at(k) {
                let mut sum: u64 = 0;
                let mut i = 0;
                while i < EFF_KIND_COUNT {
                    if (r.active_mask >> i) & 1 == 0 && r.per_effect[i] != 0 {
                        return false;
                    }
                    sum = sum.saturating_add(r.per_effect[i] as u64);
                    i += 1;
                }
                if sum != r.actual_ns {
                    return false;
                }
            }
            k += 1;
        }
        true
    }

    /// 账本指纹（头/尾/守恒错/累计列全量入指纹——任何记账差异即变）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv64::new();
        h.u64_field(self.head);
        h.u64_field(self.conservation_errs);
        h.u64_field(self.over_frames);
        h.u64_field(self.share_exceed_frames);
        h.u64_field(self.budget_cum_ns);
        h.u64_field(self.actual_cum_ns);
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            h.u64_field(self.per_effect_cum[i]);
            h.u64_field(self.per_effect_over[i]);
            h.u64_field(self.per_effect_ppm_sum[i]);
            i += 1;
        }
        h.finish()
    }
}

// ---------------------------------------------------------------------------
// 九、超标处置复用（OverrunHandler：降档→跳帧→熔断 + 留痕 + 越权把关）
// ---------------------------------------------------------------------------

/// 处置级别（与 F0008 掉帧降质链**同构镜像**——见 [`F0008_MIRROR`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispLevel {
    /// L0 无处置（全量执行）。
    None,
    /// L1 降档（效果参数降一档，留存 60%）。
    Downgrade,
    /// L2 跳帧（隔帧执行，留存 50%）。
    FrameSkip,
    /// L3 熔断（禁用至冷却结束，留存 0%）。
    Breaker,
}

impl DispLevel {
    /// 整型化（const fn 比较用）。
    pub const fn as_u8(self) -> u8 {
        match self {
            DispLevel::None => 0,
            DispLevel::Downgrade => 1,
            DispLevel::FrameSkip => 2,
            DispLevel::Breaker => 3,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            DispLevel::None => "无处置",
            DispLevel::Downgrade => "降档",
            DispLevel::FrameSkip => "跳帧",
            DispLevel::Breaker => "熔断",
        }
    }
}

/// F0008 同构镜像表（**处置复用锚点**：本模块处置族与 F0008 掉帧降质链
/// 逐级对齐——本地镜像 + 对齐注记，零编译依赖；镜像一致性 const 断言）。
pub const F0008_MIRROR: [(u8, u8, &'static str); 4] = [
    (0, 0, "F0008-L0 全质 ↔ 本域 L0 无处置（留存 100%）"),
    (1, 1, "F0008-L1 关 Bloom/折射 ↔ 本域 L1 降档（留存 60%）"),
    (2, 2, "F0008-L2 模糊半径减半/阴影降档 ↔ 本域 L2 跳帧（留存 50%）"),
    (3, 3, "F0008-L3 动态画面帧率降档（禁用）↔ 本域 L3 熔断（留存 0%）"),
];

/// const fn：镜像表逐级对齐（我方级 == F0008 级且 0..3 连续）。
const fn mirror_aligned() -> bool {
    let mut i = 0;
    while i < F0008_MIRROR.len() {
        if F0008_MIRROR[i].0 != i as u8 || F0008_MIRROR[i].1 != i as u8 {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(mirror_aligned(), "F0008 处置镜像必须逐级对齐");

/// 处置动作理由（留痕分类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispReason {
    /// 连击升级（自动）。
    StreakEscalate,
    /// 滞回回落（自动）。
    HysteresisRecover,
    /// 熔断冷却结束 → 半开探测（自动）。
    BreakerTimeout,
    /// 外部申请核准（处置复用路径——其他子系统凭累计超支史申请）。
    ExternalConfirm,
}

impl DispReason {
    /// 中文名（留痕串）。
    pub const fn name(self) -> &'static str {
        match self {
            DispReason::StreakEscalate => "连击升级",
            DispReason::HysteresisRecover => "滞回回落",
            DispReason::BreakerTimeout => "熔断半开",
            DispReason::ExternalConfirm => "外部核准",
        }
    }
}

/// 处置动作留痕（全量入轨迹环；每动作一字不落）。
#[derive(Debug, Clone, Copy)]
pub struct DispAction {
    /// 发生帧号。
    pub frame: u64,
    /// 效果下标。
    pub effect: u8,
    /// 迁移前级别。
    pub from: u8,
    /// 迁移后级别。
    pub to: u8,
    /// 理由。
    pub reason: DispReason,
    /// 处置后成本留存‰（600/500/0——处置复用的量化声明）。
    pub keep_permille: u32,
}

/// 单效果处置状态机。
#[derive(Debug, Clone, Copy)]
struct EffDispState {
    level: u8,
    streak: u16,
    recover: u16,
    breaker_left: u16,
    cumulative_over: u64,
    ever_breaker: bool,
}

impl EffDispState {
    const fn new() -> Self {
        EffDispState {
            level: 0,
            streak: 0,
            recover: 0,
            breaker_left: 0,
            cumulative_over: 0,
            ever_breaker: false,
        }
    }
}

/// 超标处置复用处理器（16 效果并行状态机 + 轨迹环 + 越权把关）。
#[derive(Debug, Clone)]
pub struct OverrunHandler {
    states: [EffDispState; EFF_KIND_COUNT],
    trail: Vec<DispAction>,
    trail_head: u64,
    /// 升级总次数（含外部核准）。
    pub escalated_total: u64,
    /// 回落总次数。
    pub recovered_total: u64,
    /// 熔断开启总次数。
    pub breaker_total: u64,
    /// 越权/无据/非法拒绝总次数。
    pub rejected_total: u64,
}

impl OverrunHandler {
    /// 新建（全效果 L0）。
    pub const fn new() -> Self {
        OverrunHandler {
            states: [EffDispState::new(); EFF_KIND_COUNT],
            trail: Vec::new(),
            trail_head: 0,
            escalated_total: 0,
            recovered_total: 0,
            breaker_total: 0,
            rejected_total: 0,
        }
    }

    fn push_trail(&mut self, a: DispAction) {
        if self.trail.len() < TRAIL_CAP {
            self.trail.push(a);
        } else {
            let slot = (self.trail_head % TRAIL_CAP as u64) as usize;
            self.trail[slot] = a;
        }
        self.trail_head = self.trail_head.saturating_add(1);
    }

    /// 被挤出轨迹环的历史动作数。
    pub fn dropped_trail(&self) -> u64 {
        self.trail_head.saturating_sub(self.trail.len() as u64)
    }

    /// 轨迹现存条数。
    pub fn trail_len(&self) -> usize {
        self.trail.len()
    }

    /// 时间序读取轨迹第 k 条（k=0 最旧）。
    pub fn trail_at(&self, k: usize) -> Option<DispAction> {
        if k >= self.trail.len() {
            return None;
        }
        let len = self.trail.len();
        let start = if len < TRAIL_CAP {
            0
        } else {
            (self.trail_head % TRAIL_CAP as u64) as usize
        };
        Some(self.trail[(start + k) % len])
    }

    /// 效果当前级别。
    pub fn level_of(&self, effect: usize) -> u8 {
        if effect >= EFF_KIND_COUNT {
            return 0;
        }
        self.states[effect].level
    }

    /// 全效果级别快照。
    pub fn levels(&self) -> [u8; EFF_KIND_COUNT] {
        let mut out = [0u8; EFF_KIND_COUNT];
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            out[i] = self.states[i].level;
            i += 1;
        }
        out
    }

    /// 处置作用掩码（级别 ≥ L1 的效果位）。
    pub fn active_mask(&self) -> u16 {
        let mut m: u16 = 0;
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            if self.states[i].level > 0 {
                m |= 1 << i;
            }
            i += 1;
        }
        m
    }

    /// 连击阈值表（级别 → 升入该级所需连击；索引 0 占位不可用）。
    pub fn streak_threshold(level: u8) -> u16 {
        if level <= 1 {
            STREAK_TO_L1
        } else if level == 2 {
            STREAK_TO_L2
        } else {
            STREAK_TO_L3
        }
    }

    /// 处置后投影成本（`actual × keep‰ / 1000`；L3 恒 0——熔断即停）。
    pub fn projected_cost(&self, effect: usize, actual_ns: u64) -> u64 {
        let lv = self.level_of(effect);
        if lv > 3 {
            return actual_ns;
        }
        actual_ns.saturating_mul(DISP_CUT_PERMILLE[lv as usize] as u64) / 1_000
    }

    /// 帧观测（自动状态机；返回本帧产生的处置动作）。
    ///
    /// 迁移规则（与 F0008 同构：三级递进 + 熔断归零 + 回升滞回）：
    /// - L3 冷却中：倒计数，归零 → L2 半开探测（BreakerTimeout）；
    /// - 超支帧：连击 +1、滞回清零；**连续**超支达 3/6/10 → L1/L2/L3；
    /// - 预算内帧：连击清零（连胜被打断）、滞回 +1；连续 4 帧预算内
    ///   → 降一级。
    pub fn observe(&mut self, effect: usize, over: bool, frame: u64) -> Option<DispAction> {
        if effect >= EFF_KIND_COUNT {
            return None;
        }
        let st = &mut self.states[effect];
        if st.level == 3 {
            // 熔断冷却：效果已禁用，over 恒为 false（上游禁用后零成本）。
            if st.breaker_left > 0 {
                st.breaker_left -= 1;
                return None;
            }
            // 冷却结束 → 半开：L2 跳帧探测。
            let from = st.level;
            st.level = 2;
            st.streak = 0;
            st.recover = 0;
            let action = DispAction {
                frame,
                effect: effect as u8,
                from,
                to: 2,
                reason: DispReason::BreakerTimeout,
                keep_permille: DISP_CUT_PERMILLE[2],
            };
            self.push_trail(action);
            self.recovered_total = self.recovered_total.saturating_add(1);
            return Some(action);
        }
        if over {
            st.streak = st.streak.saturating_add(1);
            st.recover = 0;
            st.cumulative_over = st.cumulative_over.saturating_add(1);
            let target: u8 = if st.streak >= STREAK_TO_L3 {
                3
            } else if st.streak >= STREAK_TO_L2 {
                2
            } else if st.streak >= STREAK_TO_L1 {
                1
            } else {
                0
            };
            if target > st.level {
                let from = st.level;
                st.level = target;
                if target == 3 {
                    st.breaker_left = BREAKER_FRAMES;
                    st.ever_breaker = true;
                    st.streak = 0;
                }
                let action = DispAction {
                    frame,
                    effect: effect as u8,
                    from,
                    to: target,
                    reason: DispReason::StreakEscalate,
                    keep_permille: DISP_CUT_PERMILLE[target as usize],
                };
                self.push_trail(action);
                self.escalated_total = self.escalated_total.saturating_add(1);
                if target == 3 {
                    self.breaker_total = self.breaker_total.saturating_add(1);
                }
                return Some(action);
            }
            return None;
        }
        // 预算内帧：连击清零（超支连胜被打断）+ 滞回累积。
        st.streak = 0;
        st.recover = st.recover.saturating_add(1);
        if st.recover >= RECOVER_STREAK && st.level > 0 {
            let from = st.level;
            st.level -= 1;
            st.recover = 0;
            if st.level == 0 {
                st.streak = 0;
            }
            let action = DispAction {
                frame,
                effect: effect as u8,
                from,
                to: st.level,
                reason: DispReason::HysteresisRecover,
                keep_permille: DISP_CUT_PERMILLE[st.level as usize],
            };
            self.push_trail(action);
            self.recovered_total = self.recovered_total.saturating_add(1);
            return Some(action);
        }
        None
    }

    /// 外部处置申请（**处置复用路径**——其他子系统凭自身遥测申请同一
    /// 处置族；本方法把三道关：
    /// 1. 级别必须 = 当前 +1 且 ≤3（否则 0x1DB7 越权 / 0x1DB9 非法）；
    /// 2. 必须仍有超支态势（当前连击 ≥1，否则 0x1DB8 无据）；
    /// 3. 累计超支帧数达目标级阈值（依据充分性，否则 0x1DB8）。
    pub fn request_external(
        &mut self,
        effect: usize,
        target_level: u8,
        frame: u64,
    ) -> Result<DispAction, EffBudgetError> {
        if effect >= EFF_KIND_COUNT || target_level > 3 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            return Err(EffBudgetError::DispositionLevelInvalid);
        }
        let st = &mut self.states[effect];
        if target_level <= st.level {
            self.rejected_total = self.rejected_total.saturating_add(1);
            return Err(EffBudgetError::DispositionLevelInvalid);
        }
        if target_level > st.level + 1 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            return Err(EffBudgetError::DispositionOverreach);
        }
        if st.streak == 0 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            return Err(EffBudgetError::DispositionNoGround);
        }
        if st.cumulative_over < Self::streak_threshold(target_level) as u64 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            return Err(EffBudgetError::DispositionNoGround);
        }
        let from = st.level;
        st.level = target_level;
        st.recover = 0;
        if target_level == 3 {
            st.breaker_left = BREAKER_FRAMES;
            st.ever_breaker = true;
            st.streak = 0;
        }
        let action = DispAction {
            frame,
            effect: effect as u8,
            from,
            to: target_level,
            reason: DispReason::ExternalConfirm,
            keep_permille: DISP_CUT_PERMILLE[target_level as usize],
        };
        self.push_trail(action);
        self.escalated_total = self.escalated_total.saturating_add(1);
        if target_level == 3 {
            self.breaker_total = self.breaker_total.saturating_add(1);
        }
        Ok(action)
    }

    /// 轨迹合法性体检（逐动作 |from − to| ≤ 1——越级即不合法）。
    pub fn trail_legal(&self) -> bool {
        let mut k = 0;
        while k < self.trail.len() {
            if let Some(a) = self.trail_at(k) {
                let d = if a.from > a.to { a.from - a.to } else { a.to - a.from };
                if d > 1 {
                    return false;
                }
                if a.to > 3 || a.effect as usize >= EFF_KIND_COUNT {
                    return false;
                }
            }
            k += 1;
        }
        true
    }

    /// 效果累计超支帧数（外部申请依据的可观测镜像）。
    pub fn cumulative_over(&self, effect: usize) -> u64 {
        if effect >= EFF_KIND_COUNT {
            return 0;
        }
        self.states[effect].cumulative_over
    }
}

// ---------------------------------------------------------------------------
// 十、预算修正建议（SuggestionEngine：P95 持续超预算 → 建议单，只建议）
// ---------------------------------------------------------------------------

/// 建议单状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggState {
    /// 未决（等待人工评审）。
    Proposed,
    /// 已确认（人工知悉；仍不许自动改表——改表走重编+重冻结）。
    Acknowledged,
}

/// 预算修正建议单（**只建议不自动改表**：新预算值 + 依据栏 + 家族封顶
/// 钳制标记；`apply()` 一律 0x1DBA 拒绝）。
#[derive(Debug, Clone, Copy)]
pub struct SuggestionSheet {
    /// 建议单号（全局递增）。
    pub id: u32,
    /// 效果下标。
    pub effect: u8,
    /// 现预算（ns，当前分辨率档口径）。
    pub old_budget_ns: u32,
    /// 建议新预算（ns，50µs 步进对齐 + 家族封顶钳制后）。
    pub new_budget_ns: u32,
    /// 依据：观测窗 P95 实耗（ns）。
    pub p95_ns: u64,
    /// 依据：窗内样本数。
    pub samples: u32,
    /// 依据：P95 对现预算的超支率 ppm。
    pub over_ppm: u32,
    /// 依据：连续热窗数。
    pub hot_windows: u8,
    /// 新预算是否被家族封顶钳制（钳制单同时建议处置升级）。
    pub clamped: bool,
    /// 单据状态。
    pub state: SuggState,
}

/// 预算修正建议引擎。
#[derive(Debug, Clone)]
pub struct SuggestionEngine {
    /// 未决建议单（每效果至多一张——重复出单 0x1DBB）。
    pub open: Vec<SuggestionSheet>,
    /// 已确认历史。
    pub history: Vec<SuggestionSheet>,
    window_hot: [u8; EFF_KIND_COUNT],
    last_window_seq: [u64; EFF_KIND_COUNT],
    next_id: u32,
    /// 累计出单数。
    pub proposed_total: u64,
    /// 只读红线命中数（apply 尝试被拒计数）。
    pub apply_rejected_total: u64,
}

impl SuggestionEngine {
    /// 新建。
    pub const fn new() -> Self {
        SuggestionEngine {
            open: Vec::new(),
            history: Vec::new(),
            window_hot: [0; EFF_KIND_COUNT],
            last_window_seq: [0; EFF_KIND_COUNT],
            next_id: 1,
            proposed_total: 0,
            apply_rejected_total: 0,
        }
    }

    /// 观测窗回调（引擎每 100 帧对每活跃效果调用；返回新出建议单）。
    ///
    /// 触发链：`P95 > 预算×1.25`（ppm > 125_000）记热窗；连续 3 热窗
    /// → 出单。新预算 = `ceil(P95×1.08 / 50µs)×50µs`，家族封顶钳制。
    pub fn observe_window(
        &mut self,
        effect: usize,
        budget_ns: u64,
        p95_ns: u64,
        samples: u32,
        window_seq: u64,
    ) -> Option<SuggestionSheet> {
        if effect >= EFF_KIND_COUNT || samples < SUGGEST_MIN_SAMPLES as u32 {
            // 样本不足：热窗清零（证据链断）。
            if effect < EFF_KIND_COUNT {
                self.window_hot[effect] = 0;
                self.last_window_seq[effect] = window_seq;
            }
            return None;
        }
        // 窗号不连续（效果曾离场）：热窗清零重来。
        if self.last_window_seq[effect] + 1 != window_seq {
            self.window_hot[effect] = 0;
        }
        self.last_window_seq[effect] = window_seq;
        let ppm = overrun_ppm(p95_ns, budget_ns);
        if ppm as u64 >= SUGGEST_TRIGGER_PPM {
            if self.window_hot[effect] < SUGGEST_HOT_WINDOWS {
                self.window_hot[effect] += 1;
            }
        } else {
            self.window_hot[effect] = 0;
            return None;
        }
        if self.window_hot[effect] < SUGGEST_HOT_WINDOWS {
            return None;
        }
        // 连续热窗达标：检查未决重复（同效果已有未决单 → 不重复出单，
        // 热窗保持在满格等待确认）。
        let mut k = 0;
        while k < self.open.len() {
            if self.open[k].effect as usize == effect {
                return None;
            }
            k += 1;
        }
        // 新预算：P95 × (1+80‰)，50µs 步进上取整，家族封顶钳制。
        // 量程：p95 ≤ 2e6 ×1080 ≈ 2.16e9，u64 无溢出。
        let raw = p95_ns * (1_000 + SUGGEST_HEADROOM_PERMILLE) / 1_000;
        let stepped = (raw + BUDGET_STEP_NS - 1) / BUDGET_STEP_NS * BUDGET_STEP_NS;
        let family = COST_TABLE[effect].family;
        let cap = family_cap_ns(family);
        let clamped = stepped > cap;
        let new_budget = if clamped { cap } else { stepped };
        if new_budget <= budget_ns {
            // 防御：钳制后反低于现预算（现预算已超家族封顶的病态表）——
            // 不出降预算单（降预算只能重编表）。
            return None;
        }
        let sheet = SuggestionSheet {
            id: self.next_id,
            effect: effect as u8,
            old_budget_ns: budget_ns as u32,
            new_budget_ns: new_budget as u32,
            p95_ns,
            samples,
            over_ppm: ppm,
            hot_windows: SUGGEST_HOT_WINDOWS,
            clamped,
            state: SuggState::Proposed,
        };
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.proposed_total = self.proposed_total.saturating_add(1);
        self.open.push(sheet);
        Some(sheet)
    }

    /// **只读红线**：任何"应用建议单"调用一律拒绝（0x1DBA）——预算
    /// 修正必须人工评审后重编 const 表并重冻结指纹，运行期零改表。
    pub fn apply(&mut self, _id: u32) -> Result<(), EffBudgetError> {
        self.apply_rejected_total = self.apply_rejected_total.saturating_add(1);
        Err(EffBudgetError::SuggestionApplyForbidden)
    }

    /// 同效果重复出单（未决单在架时再出 → 0x1DBB；先 ack 或等热窗失效）。
    pub fn re_propose(&self, effect: usize) -> Result<(), EffBudgetError> {
        let mut k = 0;
        while k < self.open.len() {
            if self.open[k].effect as usize == effect {
                return Err(EffBudgetError::SuggestionDuplicate);
            }
            k += 1;
        }
        Ok(())
    }

    /// 确认建议单（未决 → 已确认历史；单号不存在显性拒绝）。
    pub fn ack(&mut self, id: u32) -> Result<SuggestionSheet, EffBudgetError> {
        let mut k = 0;
        while k < self.open.len() {
            if self.open[k].id == id {
                let mut sheet = self.open[k];
                sheet.state = SuggState::Acknowledged;
                self.open.remove(k);
                self.history.push(sheet);
                // 确认后热窗清零（该效果重新累积证据）。
                let e = sheet.effect as usize;
                if e < EFF_KIND_COUNT {
                    self.window_hot[e] = 0;
                }
                return Ok(sheet);
            }
            k += 1;
        }
        Err(EffBudgetError::SuggestionDuplicate)
    }

    /// 指定效果未决单（无则 None）。
    pub fn open_of(&self, effect: usize) -> Option<SuggestionSheet> {
        let mut k = 0;
        while k < self.open.len() {
            if self.open[k].effect as usize == effect {
                return Some(self.open[k]);
            }
            k += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 十一、会话聚合（SessionDigest：整数分位/达成率/Top 超支稳定排序）
// ---------------------------------------------------------------------------

/// 超标 Top 条目（效果下标, 平均超支率 ppm）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverrunEntry {
    /// 效果下标。
    pub effect: u8,
    /// 平均超支率 ppm（累计 ppm / 超支帧数）。
    pub mean_ppm: u32,
    /// 超支帧数。
    pub over_frames: u64,
}

/// 会话聚合摘要。
#[derive(Debug, Clone)]
pub struct SessionDigest {
    /// 会话帧数（历史累计，含被挤出环的帧）。
    pub frames: u64,
    /// 帧总实耗 P50（ns）。
    pub p50_ns: u64,
    /// 帧总实耗 P95（ns）。
    pub p95_ns: u64,
    /// 帧总实耗 P99（ns）。
    pub p99_ns: u64,
    /// 帧总实耗最大（ns）。
    pub max_ns: u64,
    /// 帧总实耗最小（ns）。
    pub min_ns: u64,
    /// 预算达成率（‱ 基点：预算内帧 ×10000 / 总帧）。
    pub achievement_bp: u32,
    /// 帧级超支帧数（历史累计）。
    pub over_frames: u64,
    /// 份额越限帧数。
    pub share_exceed_frames: u64,
    /// 超标 Top5（平均超支率降序，同率按下标升序——**全序确定**）。
    pub top_overrun: Vec<OverrunEntry>,
    /// 逐效果实耗 P95（近窗 600 帧；未活跃为 0）。
    pub per_effect_p95: [u64; EFF_KIND_COUNT],
    /// 会话指纹。
    pub fingerprint: u64,
}

/// 由账本聚合会话摘要（空账显性拒绝——空会话无分位意义）。
pub fn aggregate(ledger: &BudgetLedger) -> Result<SessionDigest, EffBudgetError> {
    if ledger.head() == 0 {
        return Err(EffBudgetError::WindowEmpty);
    }
    let mut totals = ledger.window_totals(RING_CAP);
    if totals.is_empty() {
        return Err(EffBudgetError::WindowEmpty);
    }
    // 最大/最小（整数扫描）。
    let mut max_ns: u64 = 0;
    let mut min_ns: u64 = u64::MAX;
    let mut i = 0;
    while i < totals.len() {
        if totals[i] > max_ns {
            max_ns = totals[i];
        }
        if totals[i] < min_ns {
            min_ns = totals[i];
        }
        i += 1;
    }
    totals.sort_unstable();
    let p50 = quantile_u64(&totals, 500)?;
    let p95 = quantile_u64(&totals, 950)?;
    let p99 = quantile_u64(&totals, 990)?;
    // 达成率（‱）：预算内帧 = head − over_frames（历史口径）。
    let within = ledger.head().saturating_sub(ledger.over_frames);
    let achievement_bp = (within * 10_000 / ledger.head()) as u32;
    // Top 超支效果：均值超支率降序 + 下标升序破平。
    let mut entries: Vec<OverrunEntry> = Vec::new();
    let mut e = 0;
    while e < EFF_KIND_COUNT {
        if ledger.per_effect_over[e] > 0 {
            let mean = ledger.per_effect_ppm_sum[e] / ledger.per_effect_over[e];
            let mean32 = if mean > u32::MAX as u64 { u32::MAX } else { mean as u32 };
            entries.push(OverrunEntry {
                effect: e as u8,
                mean_ppm: mean32,
                over_frames: ledger.per_effect_over[e],
            });
        }
        e += 1;
    }
    entries.sort_by(|a, b| {
        if b.mean_ppm != a.mean_ppm {
            b.mean_ppm.cmp(&a.mean_ppm)
        } else if b.over_frames != a.over_frames {
            b.over_frames.cmp(&a.over_frames)
        } else {
            a.effect.cmp(&b.effect)
        }
    });
    let top_len = if entries.len() < 5 { entries.len() } else { 5 };
    let top_overrun: Vec<OverrunEntry> = entries.into_iter().take(top_len).collect();
    // 逐效果 P95（近窗活跃样本）。
    let mut per_effect_p95 = [0u64; EFF_KIND_COUNT];
    let mut j = 0;
    while j < EFF_KIND_COUNT {
        let mut w = ledger.window_actuals(j, RING_CAP);
        if !w.is_empty() {
            w.sort_unstable();
            if let Ok(v) = quantile_u64(&w, 950) {
                per_effect_p95[j] = v;
            }
        }
        j += 1;
    }
    // 会话指纹。
    let mut h = Fnv64::new();
    h.u64_field(ledger.head());
    h.u64_field(p50);
    h.u64_field(p95);
    h.u64_field(p99);
    h.u64_field(max_ns);
    h.u64_field(min_ns);
    h.u64_field(achievement_bp as u64);
    h.u64_field(ledger.over_frames);
    let mut t = 0;
    while t < top_overrun.len() {
        h.u64_field(top_overrun[t].effect as u64);
        h.u64_field(top_overrun[t].mean_ppm as u64);
        t += 1;
    }
    let mut u = 0;
    while u < EFF_KIND_COUNT {
        h.u64_field(per_effect_p95[u]);
        u += 1;
    }
    Ok(SessionDigest {
        frames: ledger.head(),
        p50_ns: p50,
        p95_ns: p95,
        p99_ns: p99,
        max_ns,
        min_ns,
        achievement_bp,
        over_frames: ledger.over_frames,
        share_exceed_frames: ledger.share_exceed_frames,
        top_overrun,
        per_effect_p95,
        fingerprint: h.finish(),
    })
}

// ---------------------------------------------------------------------------
// 十二、事件账（EventLedger：16 类事件 · 128 槽环形 · 逐类计数）
// ---------------------------------------------------------------------------

/// 事件闭集（16 类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 引擎构造（Boot）。
    Boot,
    /// 预算表指纹校验通过。
    TableVerified,
    /// 帧入账。
    FrameCommitted,
    /// 帧级超支。
    FrameOverrun,
    /// 效果级超支（arg=效果下标）。
    EffectOverrun,
    /// 份额越限（Σ活跃预算 > 3.75ms）。
    ShareExceeded,
    /// 处置升级（arg=效果下标）。
    DispEscalate,
    /// 处置回落。
    DispRecover,
    /// 熔断开启。
    BreakerOpen,
    /// 熔断冷却结束半开。
    BreakerHalfOpen,
    /// 越权申请被拒。
    OverreachRejected,
    /// 预算表篡改检出。
    TamperDetected,
    /// 守恒破坏检出。
    ConservationBreak,
    /// 建议单产生（arg=效果下标）。
    SuggestProposed,
    /// 建议单只读红线命中。
    SuggestApplyRejected,
    /// 双跑对拍通过。
    ReplayChecked,
}

impl EventKind {
    /// 事件闭集大小。
    pub const COUNT: usize = 16;

    /// 整型化。
    pub const fn as_u8(self) -> u8 {
        match self {
            EventKind::Boot => 0,
            EventKind::TableVerified => 1,
            EventKind::FrameCommitted => 2,
            EventKind::FrameOverrun => 3,
            EventKind::EffectOverrun => 4,
            EventKind::ShareExceeded => 5,
            EventKind::DispEscalate => 6,
            EventKind::DispRecover => 7,
            EventKind::BreakerOpen => 8,
            EventKind::BreakerHalfOpen => 9,
            EventKind::OverreachRejected => 10,
            EventKind::TamperDetected => 11,
            EventKind::ConservationBreak => 12,
            EventKind::SuggestProposed => 13,
            EventKind::SuggestApplyRejected => 14,
            EventKind::ReplayChecked => 15,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::Boot => "构造",
            EventKind::TableVerified => "表校验",
            EventKind::FrameCommitted => "帧入账",
            EventKind::FrameOverrun => "帧超支",
            EventKind::EffectOverrun => "效果超支",
            EventKind::ShareExceeded => "份额越限",
            EventKind::DispEscalate => "处置升级",
            EventKind::DispRecover => "处置回落",
            EventKind::BreakerOpen => "熔断开启",
            EventKind::BreakerHalfOpen => "熔断半开",
            EventKind::OverreachRejected => "越权拒绝",
            EventKind::TamperDetected => "篡改检出",
            EventKind::ConservationBreak => "守恒破坏",
            EventKind::SuggestProposed => "建议单",
            EventKind::SuggestApplyRejected => "只读红线",
            EventKind::ReplayChecked => "双跑对拍",
        }
    }

    /// 闭集内反查（表外拒绝）。
    pub const fn from_u8(v: u8) -> Result<EventKind, EffBudgetError> {
        if v >= EventKind::COUNT as u8 {
            return Err(EffBudgetError::EventKindRange);
        }
        Ok(match v {
            0 => EventKind::Boot,
            1 => EventKind::TableVerified,
            2 => EventKind::FrameCommitted,
            3 => EventKind::FrameOverrun,
            4 => EventKind::EffectOverrun,
            5 => EventKind::ShareExceeded,
            6 => EventKind::DispEscalate,
            7 => EventKind::DispRecover,
            8 => EventKind::BreakerOpen,
            9 => EventKind::BreakerHalfOpen,
            10 => EventKind::OverreachRejected,
            11 => EventKind::TamperDetected,
            12 => EventKind::ConservationBreak,
            13 => EventKind::SuggestProposed,
            14 => EventKind::SuggestApplyRejected,
            _ => EventKind::ReplayChecked,
        })
    }
}

/// 事件记录（定长 24B 语义）。
#[derive(Debug, Clone, Copy)]
pub struct EventRecord {
    /// 事件种类（`EventKind::as_u8`）。
    pub kind: u8,
    /// 事件序号（全局递增，wrapping）。
    pub seq: u32,
    /// 关联帧号。
    pub frame: u64,
    /// 载荷（效果下标 / 码值等）。
    pub arg: u64,
}

/// 事件账（128 槽环形 + 逐类计数 + dropped）。
#[derive(Debug, Clone)]
pub struct EventLedger {
    ring: Vec<EventRecord>,
    head: u64,
    seq: u32,
    kind_counts: [u32; EventKind::COUNT],
}

impl EventLedger {
    /// 新建。
    pub const fn new() -> Self {
        EventLedger {
            ring: Vec::new(),
            head: 0,
            seq: 0,
            kind_counts: [0; EventKind::COUNT],
        }
    }

    /// 入账一事件（闭集外种类显性拒绝）。
    pub fn push(
        &mut self,
        kind: EventKind,
        frame: u64,
        arg: u64,
    ) -> Result<(), EffBudgetError> {
        let rec = EventRecord {
            kind: kind.as_u8(),
            seq: self.seq,
            frame,
            arg,
        };
        self.seq = self.seq.wrapping_add(1);
        if self.ring.len() < EVENT_CAP {
            self.ring.push(rec);
        } else {
            let slot = (self.head % EVENT_CAP as u64) as usize;
            self.ring[slot] = rec;
        }
        self.head = self.head.saturating_add(1);
        let ki = kind.as_u8() as usize;
        if ki < EventKind::COUNT {
            self.kind_counts[ki] = self.kind_counts[ki].saturating_add(1);
        }
        Ok(())
    }

    /// 事件总数（历史累计）。
    pub const fn head(&self) -> u64 {
        self.head
    }

    /// 被挤出环的事件数。
    pub fn dropped(&self) -> u64 {
        self.head.saturating_sub(self.ring.len() as u64)
    }

    /// 环内现存条数。
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// 逐类计数快照。
    pub fn kind_counts(&self) -> [u32; EventKind::COUNT] {
        self.kind_counts
    }

    /// 时间序读取第 k 条（k=0 最旧）。
    pub fn record_at(&self, k: usize) -> Option<EventRecord> {
        if k >= self.ring.len() {
            return None;
        }
        let len = self.ring.len();
        let start = if len < EVENT_CAP {
            0
        } else {
            (self.head % EVENT_CAP as u64) as usize
        };
        Some(self.ring[(start + k) % len])
    }

    /// 自洽体检：① 总数 = 逐类计数之和；② 环内条数 = min(head, 128)；
    /// ③ dropped = head − 环内；④ 环内事件种类全部在闭集内。
    pub fn consistent(&self) -> bool {
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < EventKind::COUNT {
            sum += self.kind_counts[i] as u64;
            i += 1;
        }
        if sum != self.head {
            return false;
        }
        let expect_len = if self.head > EVENT_CAP as u64 {
            EVENT_CAP
        } else {
            self.head as usize
        };
        if self.ring.len() != expect_len {
            return false;
        }
        if self.dropped() != self.head.saturating_sub(self.ring.len() as u64) {
            return false;
        }
        let mut k = 0;
        while k < self.ring.len() {
            if let Some(r) = self.record_at(k) {
                if EventKind::from_u8(r.kind).is_err() {
                    return false;
                }
            }
            k += 1;
        }
        true
    }

    /// 事件账指纹（时间序全量入指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv64::new();
        h.u64_field(self.head);
        let mut k = 0;
        while k < self.ring.len() {
            if let Some(r) = self.record_at(k) {
                h.u64_field(r.kind as u64);
                h.u64_field(r.seq as u64);
                h.u64_field(r.frame);
                h.u64_field(r.arg);
            }
            k += 1;
        }
        h.finish()
    }
}

// ---------------------------------------------------------------------------
// 十三、health() 全量体检
// ---------------------------------------------------------------------------

/// 体检报告（逐项布尔 + 总判 + 指纹 + 文本）。
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// 检查项（名称, 通过与否）。
    pub checks: Vec<(&'static str, bool)>,
    /// 总判（全项通过才 true）。
    pub all_ok: bool,
    /// 体检时点引擎指纹。
    pub fingerprint: u64,
    /// 文本汇报。
    pub text: String,
}

// ---------------------------------------------------------------------------
// 十四、引擎门面（EffBudgetEngine：帧闭环 / 双跑对拍 / 注入测试面）
// ---------------------------------------------------------------------------

/// 帧脚本（双跑对拍与测试的确定性输入单元）。
#[derive(Debug, Clone, Copy)]
pub struct FrameScript {
    /// 分辨率档（‰ 兆像素）。
    pub mpx_permille: u32,
    /// 活跃效果掩码。
    pub active_mask: u16,
    /// 逐效果实耗（ns；未激活必须 0）。
    pub per_effect: [u32; EFF_KIND_COUNT],
    /// 申报帧总实耗（ns；必须 = Σ活跃分项——守恒由账本把关）。
    pub declared_total_ns: u64,
}

/// 单帧执行结果。
#[derive(Debug, Clone)]
pub struct FrameOutcome {
    /// 帧序号。
    pub seq: u64,
    /// 帧预算合计（ns）。
    pub budget_ns: u64,
    /// 帧总实耗（ns）。
    pub actual_ns: u64,
    /// 帧级超支率 ppm。
    pub over_ppm: u32,
    /// 份额越限（预算侧超载，交处置族）。
    pub share_exceeded: bool,
    /// 本帧处置动作（含外部核准）。
    pub actions: Vec<DispAction>,
    /// 全效果处置级别快照（帧后）。
    pub levels: [u8; EFF_KIND_COUNT],
    /// 处置后下一帧投影实耗（Σ actual × keep‰）。
    pub projected_ns: u64,
}

/// 效果性能预算引擎（门面：预算表 + 账本 + 处置 + 建议 + 事件）。
#[derive(Debug, Clone)]
pub struct EffBudgetEngine {
    table: BudgetTable,
    ledger: BudgetLedger,
    handler: OverrunHandler,
    sugg: SuggestionEngine,
    events: EventLedger,
    frame_seq: u64,
    replay_fp: Option<(u64, u64)>,
}

impl EffBudgetEngine {
    /// 构造（表指纹即时校验入事件账——Boot 即自证）。
    pub fn new() -> Self {
        let mut eng = EffBudgetEngine {
            table: BudgetTable::new(),
            ledger: BudgetLedger::new(),
            handler: OverrunHandler::new(),
            sugg: SuggestionEngine::new(),
            events: EventLedger::new(),
            frame_seq: 0,
            replay_fp: None,
        };
        // Boot 自证：const 表构建后指纹必须恰为冻结值（构建即验证）。
        let fp = match eng.table.verify() {
            Ok(v) => v,
            Err(_) => eng.table.frozen_fingerprint(),
        };
        let _ = eng.events.push(EventKind::Boot, 0, fp);
        eng
    }

    /// 预算表（只读访问）。
    pub fn table(&self) -> &BudgetTable {
        &self.table
    }

    /// 账本（只读访问）。
    pub fn ledger(&self) -> &BudgetLedger {
        &self.ledger
    }

    /// 处置器（只读访问）。
    pub fn handler(&self) -> &OverrunHandler {
        &self.handler
    }

    /// 建议引擎（只读访问）。
    pub fn suggestions(&self) -> &SuggestionEngine {
        &self.sugg
    }

    /// 事件账（只读访问）。
    pub fn events(&self) -> &EventLedger {
        &self.events
    }

    /// 校验预算表指纹（失配即 0x1DB4——篡改检出入口）。
    pub fn verify_table(&mut self) -> Result<u64, EffBudgetError> {
        match self.table.verify() {
            Ok(fp) => {
                let _ = self.events.push(EventKind::TableVerified, self.frame_seq, fp);
                Ok(fp)
            }
            Err(e) => {
                let _ = self.events.push(
                    EventKind::TamperDetected,
                    self.frame_seq,
                    e.code() as u64,
                );
                Err(e)
            }
        }
    }

    /// 单效果预算（ns；门面便捷口）。
    pub fn budget_ns_at(
        &self,
        effect: usize,
        mpx_permille: u32,
    ) -> Result<u64, EffBudgetError> {
        self.table.budget_ns_at(effect, mpx_permille, self.frame_seq)
    }

    /// 帧闭环：校验 → 预算 → 守恒入账 → 逐效果处置观测 → 建议窗回调。
    pub fn run_frame(&mut self, script: &FrameScript) -> Result<FrameOutcome, EffBudgetError> {
        if script.mpx_permille < MPX_PERMILLE_MIN
            || script.mpx_permille > MPX_PERMILLE_MAX
        {
            return Err(EffBudgetError::PixelScaleRange);
        }
        let seq = self.frame_seq.wrapping_add(1);
        // 预算侧：双刻度保守大值逐效果 + 合计 + 份额检查。
        let (frame_budget_ns, share_exceeded) =
            self.table.frame_budget(script.active_mask, script.mpx_permille, seq)?;
        if share_exceeded {
            let _ = self
                .events
                .push(EventKind::ShareExceeded, seq, frame_budget_ns);
        }
        // 逐效果预算与超支判定（超支判定用稳态预算——尖峰帧不误判）。
        let mut over_mask: u16 = 0;
        let mut ppm_arr = [0u32; EFF_KIND_COUNT];
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            if (script.active_mask >> i) & 1 == 1 {
                let b = self.table.budget_ns_at(i, script.mpx_permille, 0)?;
                let ppm = overrun_ppm(script.per_effect[i] as u64, b);
                if ppm > 0 {
                    over_mask |= 1 << i;
                    let _ = self.events.push(EventKind::EffectOverrun, seq, i as u64);
                }
                ppm_arr[i] = ppm;
            }
            i += 1;
        }
        // 守恒入账（失败路径：事件留痕后原样上抛）。
        let disp_mask = self.handler.active_mask();
        let commit = match self.ledger.commit(
            seq,
            script.mpx_permille,
            script.active_mask,
            &script.per_effect,
            script.declared_total_ns,
            frame_budget_ns,
            over_mask,
            &ppm_arr,
            share_exceeded,
            disp_mask,
        ) {
            Ok(c) => c,
            Err(e) => {
                if e == EffBudgetError::FrameConservation {
                    let _ = self.events.push(
                        EventKind::ConservationBreak,
                        seq,
                        e.code() as u64,
                    );
                }
                return Err(e);
            }
        };
        self.frame_seq = seq;
        let _ = self
            .events
            .push(EventKind::FrameCommitted, seq, script.declared_total_ns);
        if commit.over_ppm > 0 {
            let _ = self
                .events
                .push(EventKind::FrameOverrun, seq, commit.over_ppm as u64);
        }
        // 处置观测（活跃效果逐个；处置后效果实耗归零——禁用即不超支，
        // 与 F0008 同构的"降质不降帧"处置语义）。
        let mut actions: Vec<DispAction> = Vec::new();
        let mut j = 0;
        while j < EFF_KIND_COUNT {
            if (script.active_mask >> j) & 1 == 1 {
                let lv = self.handler.level_of(j);
                let effective_over = if lv == 3 {
                    false // 熔断中：效果已禁用，观测恒"预算内"驱动冷却。
                } else {
                    (over_mask >> j) & 1 == 1
                };
                if let Some(a) = self.handler.observe(j, effective_over, seq) {
                    if a.to == 3 {
                        let _ = self.events.push(EventKind::BreakerOpen, seq, j as u64);
                    } else if a.to > a.from {
                        let _ = self.events.push(EventKind::DispEscalate, seq, j as u64);
                    } else {
                        let _ = self.events.push(EventKind::DispRecover, seq, j as u64);
                        if a.reason == DispReason::BreakerTimeout {
                            let _ =
                                self.events.push(EventKind::BreakerHalfOpen, seq, j as u64);
                        }
                    }
                    actions.push(a);
                }
            }
            j += 1;
        }
        // 建议窗回调：每 100 帧边界对全效果做 P95 复核（近窗样本）。
        if seq > 0 && seq % SUGGEST_WINDOW == 0 {
            let window_seq = seq / SUGGEST_WINDOW;
            let mut e2 = 0;
            while e2 < EFF_KIND_COUNT {
                let samples = self.ledger.window_actuals(e2, SUGGEST_WINDOW as usize);
                let n = samples.len() as u32;
                if n >= SUGGEST_MIN_SAMPLES as u32 {
                    let mut sorted = samples;
                    sorted.sort_unstable();
                    if let Ok(p95) = quantile_u64(&sorted, 950) {
                        // 建议依据用稳态预算（尖峰不参与判热）。
                        if let Ok(b) = self.table.budget_ns_at(e2, script.mpx_permille, 0)
                        {
                            if self
                                .sugg
                                .observe_window(e2, b, p95, n, window_seq)
                                .is_some()
                            {
                                let _ = self.events.push(
                                    EventKind::SuggestProposed,
                                    seq,
                                    e2 as u64,
                                );
                            }
                        }
                    }
                }
                e2 += 1;
            }
        }
        // 处置后投影（下一帧口径：Σ actual × keep‰）。
        let mut projected: u64 = 0;
        let mut m = 0;
        while m < EFF_KIND_COUNT {
            if (script.active_mask >> m) & 1 == 1 {
                projected = projected
                    .saturating_add(self.handler.projected_cost(m, script.per_effect[m] as u64));
            }
            m += 1;
        }
        Ok(FrameOutcome {
            seq,
            budget_ns: frame_budget_ns,
            actual_ns: script.declared_total_ns,
            over_ppm: commit.over_ppm,
            share_exceeded,
            actions,
            levels: self.handler.levels(),
            projected_ns: projected,
        })
    }

    /// 外部处置申请（处置复用门面；拒绝路径事件留痕）。
    pub fn request_disposition(
        &mut self,
        effect: usize,
        target_level: u8,
    ) -> Result<DispAction, EffBudgetError> {
        match self.handler.request_external(effect, target_level, self.frame_seq) {
            Ok(a) => {
                if a.to == 3 {
                    let _ =
                        self.events.push(EventKind::BreakerOpen, self.frame_seq, a.effect as u64);
                } else {
                    let _ =
                        self.events.push(EventKind::DispEscalate, self.frame_seq, a.effect as u64);
                }
                Ok(a)
            }
            Err(e) => {
                let _ = self.events.push(
                    EventKind::OverreachRejected,
                    self.frame_seq,
                    e.code() as u64,
                );
                Err(e)
            }
        }
    }

    /// 建议单只读红线门面（apply 一律拒绝 + 事件留痕）。
    pub fn apply_suggestion(&mut self, id: u32) -> Result<(), EffBudgetError> {
        let r = self.sugg.apply(id);
        let _ = self.events.push(
            EventKind::SuggestApplyRejected,
            self.frame_seq,
            EffBudgetError::SuggestionApplyForbidden.code() as u64,
        );
        r
    }

    /// 确认建议单门面。
    pub fn ack_suggestion(&mut self, id: u32) -> Result<SuggestionSheet, EffBudgetError> {
        self.sugg.ack(id)
    }

    /// 会话聚合门面。
    pub fn aggregate(&self) -> Result<SessionDigest, EffBudgetError> {
        aggregate(&self.ledger)
    }

    /// 转录指纹（表指纹 + 账本 + 处置 + 建议 + 事件全链——任何子系统
    /// 任何差异即变指纹；双跑对拍的比较基准）。
    pub fn transcript_fingerprint(&self) -> u64 {
        let mut h = Fnv64::new();
        h.u64_field(self.table.frozen_fingerprint());
        h.u64_field(self.table.tamper_marks);
        h.u64_field(self.ledger.fingerprint());
        h.u64_field(self.handler.escalated_total);
        h.u64_field(self.handler.recovered_total);
        h.u64_field(self.handler.breaker_total);
        h.u64_field(self.handler.rejected_total);
        h.u64_field(self.handler.trail_head);
        h.u64_field(self.sugg.proposed_total);
        h.u64_field(self.sugg.apply_rejected_total);
        let mut k = 0;
        while k < self.handler.trail_len() {
            if let Some(a) = self.handler.trail_at(k) {
                h.u64_field(a.frame);
                h.u64_field(a.effect as u64);
                h.u64_field(a.from as u64);
                h.u64_field(a.to as u64);
                h.u64_field(a.keep_permille as u64);
            }
            k += 1;
        }
        h.u64_field(self.events.fingerprint());
        h.finish()
    }

    /// 双跑确定性对拍：两个全新引擎跑同一脚本，转录指纹必须逐位一致
    /// （不一致 = 隐式状态/未初始化读——0x1DBF 显性失败，禁止发布）。
    pub fn run_script_det(
        &mut self,
        script: &[FrameScript],
    ) -> Result<(u64, u64), EffBudgetError> {
        let mut a = EffBudgetEngine::new();
        let mut b = EffBudgetEngine::new();
        let mut i = 0;
        while i < script.len() {
            a.run_frame(&script[i])?;
            i += 1;
        }
        let mut j = 0;
        while j < script.len() {
            b.run_frame(&script[j])?;
            j += 1;
        }
        let fa = a.transcript_fingerprint();
        let fb = b.transcript_fingerprint();
        if fa != fb {
            return Err(EffBudgetError::ReplayMismatch);
        }
        self.replay_fp = Some((fa, fb));
        let _ = self.events.push(EventKind::ReplayChecked, self.frame_seq, fa);
        Ok((fa, fb))
    }

    /// 最近一次双跑指纹。
    pub const fn last_replay_fp(&self) -> Option<(u64, u64)> {
        self.replay_fp
    }

    /// health() 全量体检（六项：表指纹 / 记账环守恒纯净 / 环头尾一致 /
    /// 事件自洽 / 处置轨迹逐级合法 / 建议单只读）。
    pub fn health(&self) -> HealthReport {
        let table_ok = self.table.verify().is_ok();
        let (ring_ok, _over_in_ring) = self.ledger.scan_consistent();
        let events_ok = self.events.consistent();
        let trail_ok = self.handler.trail_legal();
        let sugg_ok = self.sugg.open.len() <= EFF_KIND_COUNT
            && self
                .sugg
                .open
                .iter()
                .all(|s| s.state == SuggState::Proposed);
        let checks: Vec<(&'static str, bool)> = vec![
            ("预算表指纹复算一致", table_ok),
            ("记账环守恒纯净", self.ledger.ring_pure()),
            ("记账环头尾一致", ring_ok),
            ("事件账自洽", events_ok),
            ("处置轨迹逐级合法", trail_ok),
            ("建议单只读未应用", sugg_ok),
        ];
        let all_ok = checks.iter().all(|(_, ok)| *ok);
        let mut text = String::from("== CGPU-F2087 效果性能预算 health ==\n");
        let mut i = 0;
        while i < checks.len() {
            text.push_str(&format!(
                "[{}] {}\n",
                if checks[i].1 { "通过" } else { "失败" },
                checks[i].0
            ));
            i += 1;
        }
        text.push_str(&format!(
            "帧 {} 处置{}回{}熔断{}拒{} 建议{}只读红线{}\n",
            self.ledger.head(),
            self.handler.escalated_total,
            self.handler.recovered_total,
            self.handler.breaker_total,
            self.handler.rejected_total,
            self.sugg.proposed_total,
            self.sugg.apply_rejected_total
        ));
        HealthReport {
            checks,
            all_ok,
            fingerprint: self.transcript_fingerprint(),
            text,
        }
    }

    /// 单行标签（管线对拍 / 收口汇报用）。
    pub fn tag(&self) -> String {
        format!(
            "single=CGPU-F2087 file=vcn07_effbudget.rs seg=0x{:04X} fp=0x{:016X}",
            SEG_BASE,
            self.table.frozen_fingerprint()
        )
    }
}

// ---------------------------------------------------------------------------
// 十五、注入测试面（预算表篡改 / 处置越权 / 守恒破坏 三类检出）
// ---------------------------------------------------------------------------

/// 注入判定（检出 = 注入被显性错误码拦截 + 事件留痕）。
#[derive(Debug, Clone)]
pub struct InjectionVerdict {
    /// 注入名。
    pub name: &'static str,
    /// 是否被检出（错误码正确 + 事件留痕）。
    pub detected: bool,
    /// 拦截错误码（未拦截为 0）。
    pub code: u16,
    /// 注释（含自愈说明）。
    pub note: String,
}

/// 注入 1 · 预算表篡改：写一行假预算 → `verify_table()` 必失配 0x1DB4 +
/// TamperDetected 事件 → **自愈恢复 const 表并复核指纹归位**。
pub fn inject_budget_tamper(eng: &mut EffBudgetEngine, effect: usize, fake_budget_us: u32) -> InjectionVerdict {
    let write_ok = eng.table.debug_write_row(effect, fake_budget_us).is_ok();
    let verdict = match eng.verify_table() {
        Err(e) if e.code() == EffBudgetError::TableFingerprintMismatch.code() => {
            InjectionVerdict {
                name: "预算表篡改注入",
                detected: true,
                code: e.code(),
                note: format!(
                    "写入假预算 {}µs 后指纹失配拦截（写入{}），随后自愈恢复",
                    fake_budget_us,
                    if write_ok { "成功" } else { "失败" }
                ),
            }
        }
        Err(e) => InjectionVerdict {
            name: "预算表篡改注入",
            detected: false,
            code: e.code(),
            note: format!("拦截码异常：0x{:04X}", e.code()),
        },
        Ok(_) => InjectionVerdict {
            name: "预算表篡改注入",
            detected: false,
            code: 0,
            note: String::from("未拦截：指纹未失配——冻结机制失效"),
        },
    };
    // 自愈：恢复 const 表并复核指纹（自愈失败计入 note）。
    match eng.table.heal_from_const() {
        Ok(fp) if fp == TABLE_FINGERPRINT => {}
        _ => {
            let mut v = verdict;
            v.note.push_str("；警告：自愈后指纹未归位");
            return v;
        }
    }
    let _ = eng.verify_table(); // 自愈后再校验一次，TableVerified 事件收尾。
    verdict
}

/// 注入 2 · 处置越权：L0 效果直跳 L3 熔断 → 处置族把关节拒 0x1DB7 +
/// OverreachRejected 事件（状态机不受污染）。
pub fn inject_disposition_overreach(
    eng: &mut EffBudgetEngine,
    effect: usize,
) -> InjectionVerdict {
    let before = eng.handler.levels();
    match eng.request_disposition(effect, 3) {
        Err(e) if e.code() == EffBudgetError::DispositionOverreach.code() => {
            let after = eng.handler.levels();
            let idx = if effect < EFF_KIND_COUNT { effect } else { 0 };
            InjectionVerdict {
                name: "处置越权注入",
                detected: true,
                code: e.code(),
                note: format!(
                    "L{} 直跳 L3 被拒，级别快照前后一致={}",
                    before[idx],
                    before == after
                ),
            }
        }
        Err(e) => InjectionVerdict {
            name: "处置越权注入",
            detected: false,
            code: e.code(),
            note: format!("拦截码异常：0x{:04X}（预期越权码）", e.code()),
        },
        Ok(_) => InjectionVerdict {
            name: "处置越权注入",
            detected: false,
            code: 0,
            note: String::from("未拦截：越权申请被放行——处置把关节失效"),
        },
    }
}

/// 注入 3 · 守恒破坏：申报总账 ≠ Σ分项 → 记账守恒红线拦截 0x1DB3 +
/// ConservationBreak 事件（账本状态零污染——该帧未入账）。
pub fn inject_conservation_break(
    eng: &mut EffBudgetEngine,
    mask: u16,
    per_effect: &[u32; EFF_KIND_COUNT],
    bogus_total_ns: u64,
) -> InjectionVerdict {
    let head_before = eng.ledger().head();
    let script = FrameScript {
        mpx_permille: REF_MPX_PERMILLE,
        active_mask: mask,
        per_effect: *per_effect,
        declared_total_ns: bogus_total_ns,
    };
    match eng.run_frame(&script) {
        Err(e) if e.code() == EffBudgetError::FrameConservation.code() => {
            let head_after = eng.ledger().head();
            InjectionVerdict {
                name: "守恒破坏注入",
                detected: true,
                code: e.code(),
                note: format!(
                    "假总账 {}ns 被守恒红线拦截，账本头 {}→{}（零污染）",
                    bogus_total_ns, head_before, head_after
                ),
            }
        }
        Err(e) => InjectionVerdict {
            name: "守恒破坏注入",
            detected: false,
            code: e.code(),
            note: format!("拦截码异常：0x{:04X}（预期守恒码）", e.code()),
        },
        Ok(_) => InjectionVerdict {
            name: "守恒破坏注入",
            detected: false,
            code: 0,
            note: String::from("未拦截：假总账入账——守恒红线失效"),
        },
    }
}

/// 全注入巡检（三注入各检出 + 巡检后 health 全绿）。
pub fn injection_sweep(eng: &mut EffBudgetEngine) -> Result<Vec<InjectionVerdict>, EffBudgetError> {
    let mut out = Vec::new();
    // 1) 篡改：把高斯模糊预算写成 1µs（应失配）。
    out.push(inject_budget_tamper(eng, 0, 1));
    // 2) 越权：对 L0 的盒式模糊直跳 L3。
    out.push(inject_disposition_overreach(eng, 1));
    // 3) 守恒破坏：分项和 120_000 申报 999_999。
    let mut pe = [0u32; EFF_KIND_COUNT];
    pe[9] = 120_000; // Dither 单效果。
    let real_total: u64 = 120_000;
    out.push(inject_conservation_break(
        eng,
        EffectKind::Dither.bit(),
        &pe,
        real_total + 879_999,
    ));
    Ok(out)
}

// ---------------------------------------------------------------------------
// 十六、测试（预算/超标两组 + 注入检出 + 双跑一致；cfg(test) 走 std 断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    // ---- 测试脚手架 ------------------------------------------------------

    /// 造一帧脚本：活跃效果按预算比例给实耗（mult_permille = 实耗/预算
    /// 的千分比；1200 = 超支 20%）。
    fn make_script(mask: u16, mult_permille: u64, mpx: u32) -> FrameScript {
        let table = BudgetTable::new();
        let mut pe = [0u32; EFF_KIND_COUNT];
        let mut total: u64 = 0;
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            if (mask >> i) & 1 == 1 {
                let b = table.budget_ns_at(i, mpx, 0).unwrap_or(0);
                let v = b.saturating_mul(mult_permille) / 1_000;
                let v32 = if v > u32::MAX as u64 { u32::MAX } else { v as u32 };
                pe[i] = v32;
                total += v32 as u64;
            }
            i += 1;
        }
        FrameScript {
            mpx_permille: mpx,
            active_mask: mask,
            per_effect: pe,
            declared_total_ns: total,
        }
    }

    // ---- 预算组：预算表 / 指纹 / 双刻度 / 家族 / 记账守恒 / 建议单 ----

    #[test]
    fn budget_table_shape_and_fingerprint() {
        let t = BudgetTable::new();
        assert_eq!(t.len(), 16, "预算表恰 16 行");
        assert!(!t.is_empty());
        // const 冻结指纹 == 运行期复算指纹。
        assert_eq!(t.verify().unwrap(), TABLE_FINGERPRINT);
        // const 串行化与运行期串行化同构（布局交叉对拍）。
        let a = serialize_cost_table();
        let b = serialize_rows(&{
            let mut rows = vec![];
            let mut i = 0;
            while i < EFF_KIND_COUNT {
                rows.push(COST_TABLE[i]);
                i += 1;
            }
            rows
        });
        let mut k = 0;
        while k < TABLE_SER_LEN {
            assert_eq!(a[k], b[k], "串行化双实现第 {} 字节一致", k);
            k += 1;
        }
        // 行序 = 效果闭集序。
        let mut i = 0;
        while i < EFF_KIND_COUNT {
            assert_eq!(t.row(i).unwrap().kind.as_u8(), i as u8);
            i += 1;
        }
        assert!(t.row(16).is_err(), "闭集外行下标显性拒绝");
        assert_eq!(
            t.row(16).unwrap_err().code(),
            EffBudgetError::EffectIndexRange.code()
        );
    }

    #[test]
    fn budget_dual_scale_and_resolution() {
        let t = BudgetTable::new();
        // 参考分辨率：像素族帧档腿 = budget_us×1000（保守大值恰为帧档）。
        let g = t.budget_ns_at(0, REF_MPX_PERMILLE, 0).unwrap();
        assert_eq!(g, 420_000, "高斯模糊参考档预算 420µs");
        // 固定族：4K 档像素腿接管（Dither 60µs 帧档 vs 像素腿 59_990）。
        let dither_4k = t.budget_ns_at(9, MPX_PERMILLE_MAX, 0).unwrap();
        assert!(
            dither_4k >= 59_000 && dither_4k <= 61_000,
            "Dither 4K 档双刻度交接带：{}",
            dither_4k
        );
        // 像素族预算随分辨率单调不减（双刻度取大保守）。
        let mut mpx = MPX_PERMILLE_MIN;
        let mut prev: u64 = 0;
        while mpx <= MPX_PERMILLE_MAX {
            let v = t.budget_ns_at(5, mpx, 0).unwrap(); // Acrylic 像素族。
            assert!(v >= prev, "预算随分辨率单调不减（{}‰）", mpx);
            prev = v;
            mpx += 500;
        }
        // 摊销族尖峰：Mica 稳态 380µs，尖峰帧 = min(380µs×16, 2.4ms) = 2.4ms。
        let steady = t.budget_ns_at(6, REF_MPX_PERMILLE, 3).unwrap();
        let spike = t.budget_ns_at(6, REF_MPX_PERMILLE, 16).unwrap();
        assert_eq!(steady, 380_000);
        assert_eq!(spike, 2_400_000, "摊销尖峰必须被家族封顶钳到 2.4ms");
        // 分辨率档越界显性拒绝。
        assert_eq!(
            t.budget_ns_at(0, 50, 0).unwrap_err().code(),
            EffBudgetError::PixelScaleRange.code()
        );
        assert_eq!(
            t.budget_ns_at(0, 9_000, 0).unwrap_err().code(),
            EffBudgetError::PixelScaleRange.code()
        );
    }

    #[test]
    fn budget_family_and_scene_shares() {
        let t = BudgetTable::new();
        // 场景闭集：Σ预算 ≤ 3.75ms 且家族份额合规（运行期镜像复算）。
        let mut s = 0;
        while s < SCENE_COUNT {
            let (total, over) = t
                .frame_budget(SCENE_MASKS[s], REF_MPX_PERMILLE, 1)
                .unwrap();
            assert!(!over, "{} 预算合计不越效果份额", SCENE_NAMES[s]);
            let expect_max = EFFECT_SHARE_NS;
            assert!(total <= expect_max, "{} 合计 {}ns ≤ 份额", SCENE_NAMES[s], total);
            // 家族份额（参考口径 µs 比较）。
            let mut f = 0;
            while f < FAMILY_REGISTRY.len() {
                let fam = FAMILY_REGISTRY[f].family;
                let used_us = t.family_budget_us(SCENE_MASKS[s], fam);
                let share_us = EFFECT_SHARE_NS * family_share_permille(fam) as u64
                    / 1_000
                    / 1_000;
                assert!(
                    used_us <= share_us,
                    "{} 家族 {:?} 用 {}µs > 份额 {}µs",
                    SCENE_NAMES[s],
                    fam,
                    used_us,
                    share_us
                );
                f += 1;
            }
            s += 1;
        }
        // 全效果全开必越份额（对照断言的运行期镜像）。
        let (bl_total, bl_over) = t.frame_budget(0xFFFF, REF_MPX_PERMILLE, 1).unwrap();
        assert!(bl_over, "全开 4760µs 必越 3.75ms 份额");
        assert!(bl_total > EFFECT_SHARE_NS);
        // 预算复用声明与镜像注记在架。
        assert_eq!(BUDGET_REUSE_DECL.len(), 3);
        assert_eq!(F0008_MIRROR.len(), 4);
    }

    #[test]
    fn budget_ledger_conservation_and_ring() {
        let mut led = BudgetLedger::new();
        let mask_gd = EffectKind::GaussBlur.bit() | EffectKind::Dither.bit();
        // 合法帧：Σ分项 = 总账。
        let pe = {
            let mut a = [0u32; EFF_KIND_COUNT];
            a[0] = 100_000;
            a[9] = 60_000;
            a
        };
        let ppm0 = [0u32; EFF_KIND_COUNT];
        let c = led
            .commit(1, REF_MPX_PERMILLE, mask_gd, &pe, 160_000, 500_000, 0, &ppm0, false, 0)
            .unwrap();
        assert_eq!(c.seq, 1);
        assert_eq!(c.over_ppm, 0, "预算内帧超支率 0");
        assert!(!c.wrapped);
        // 守恒破坏：总账 160_001 ≠ Σ160_000。
        let e = led
            .commit(2, REF_MPX_PERMILLE, mask_gd, &pe, 160_001, 500_000, 0, &ppm0, false, 0)
            .unwrap_err();
        assert_eq!(e.code(), EffBudgetError::FrameConservation.code());
        assert_eq!(led.conservation_errs, 1);
        // 未激活分项非零。
        let mut pe2 = pe;
        pe2[1] = 5;
        let e2 = led
            .commit(2, REF_MPX_PERMILLE, mask_gd, &pe2, 160_005, 500_000, 0, &ppm0, false, 0)
            .unwrap_err();
        assert_eq!(e2.code(), EffBudgetError::FrameConservation.code());
        assert_eq!(led.conservation_errs, 2);
        // 序号回退。
        let e3 = led
            .commit(1, REF_MPX_PERMILLE, mask_gd, &pe, 160_000, 500_000, 0, &ppm0, false, 0)
            .unwrap_err();
        assert_eq!(e3.code(), EffBudgetError::FrameSeqRegression.code());
        assert_eq!(led.seq_rejects, 1);
        assert_eq!(led.head(), 1, "失败帧不入账");
        // 单值越界。
        let mut pe3 = [0u32; EFF_KIND_COUNT];
        pe3[0] = COST_VALUE_MAX + 1;
        let e4 = led
            .commit(2, REF_MPX_PERMILLE, EffectKind::GaussBlur.bit(), &pe3, COST_VALUE_MAX as u64 + 1, 5_000_000, 0, &ppm0, false, 0)
            .unwrap_err();
        assert_eq!(e4.code(), EffBudgetError::CostValueRange.code());
        // 环形：灌 650 帧，环存 600，最旧被挤出，累计列含历史。
        let mut n = 2;
        while n <= 651 {
            let mut pem = [0u32; EFF_KIND_COUNT];
            pem[0] = 10_000;
            let mut ppm_arr = [0u32; EFF_KIND_COUNT];
            if n % 2 == 0 {
                ppm_arr[0] = 500_000;
                let _ = led.commit(n, REF_MPX_PERMILLE, EffectKind::GaussBlur.bit(), &pem, 10_000, 5_000, EffectKind::GaussBlur.bit(), &ppm_arr, false, 0);
            } else {
                let _ = led.commit(n, REF_MPX_PERMILLE, EffectKind::GaussBlur.bit(), &pem, 10_000, 20_000, 0, &ppm_arr, false, 0);
            }
            n += 1;
        }
        assert_eq!(led.head(), 651);
        assert_eq!(led.len(), RING_CAP, "环满后恰存 600");
        assert_eq!(led.dropped_frames(), 51);
        // 第 2 帧已被挤出；现存最旧应为第 52 帧。
        let oldest = led.record_at(0).unwrap();
        assert_eq!(oldest.seq, 52);
        // 累计列（含被挤出帧）：偶数帧 325 帧超支（2..651 偶数恰 325）。
        assert_eq!(led.per_effect_over[0], 325);
        assert!(led.per_effect_cum[0] >= 10_000 * 650);
        // 环扫描一致 + 环纯净。
        let (ok, _n) = led.scan_consistent();
        assert!(ok);
        assert!(led.ring_pure());
    }

    #[test]
    fn budget_suggestion_sheet_flow() {
        let mut eng = EffBudgetEngine::new();
        // 高斯模糊持续超支 40%（预算内处置外场景：连击会升级 L1——
        // 升级后投影成本变化不影响"实耗样本"口径，样本始终按实测入账）。
        let mask = EffectKind::GaussBlur.bit() | EffectKind::Dither.bit();
        let mut s = make_script(mask, 1_400, REF_MPX_PERMILLE);
        let mut f = 0;
        while f < 320 {
            let _ = eng.run_frame(&s).unwrap();
            // 帧间微扰（确定性三角波：+1000/帧，每 50 帧回落）——保 P95 稳定超 25%。
            if f % 50 != 49 {
                s.per_effect[0] += 1_000;
                s.declared_total_ns += 1_000;
            } else {
                s.per_effect[0] -= 49_000;
                s.declared_total_ns -= 49_000;
            }
            f += 1;
        }
        // 320 帧 ≥ 3 个观测窗且持续热 → 必有未决建议单。
        let sheet = eng.suggestions().open_of(0);
        assert!(sheet.is_some(), "持续超预算 40% 必出建议单");
        let sh = sheet.unwrap();
        assert_eq!(sh.effect, 0);
        assert!(sh.p95_ns > 420_000, "依据 P95 须超现预算");
        assert!(sh.over_ppm as u64 >= SUGGEST_TRIGGER_PPM);
        assert_eq!(sh.hot_windows, SUGGEST_HOT_WINDOWS);
        // 新预算 50µs 步进对齐且 > 现预算。
        assert!(sh.new_budget_ns % BUDGET_STEP_NS as u32 == 0);
        assert!(sh.new_budget_ns > sh.old_budget_ns);
        assert!(sh.new_budget_ns as u64 <= family_cap_ns(BudgetFamily::Pixel));
        // 只读红线：apply 一律拒绝。
        let e = eng.apply_suggestion(sh.id).unwrap_err();
        assert_eq!(e.code(), EffBudgetError::SuggestionApplyForbidden.code());
        assert_eq!(eng.suggestions().apply_rejected_total, 1);
        // 重复出单拒绝（未决单在架）。
        let e2 = eng.suggestions().re_propose(0).unwrap_err();
        assert_eq!(e2.code(), EffBudgetError::SuggestionDuplicate.code());
        // 确认后可重新累积证据。
        let acked = eng.ack_suggestion(sh.id).unwrap();
        assert_eq!(acked.state, SuggState::Acknowledged);
        assert!(eng.suggestions().open_of(0).is_none());
        assert_eq!(eng.suggestions().history.len(), 1);
    }

    #[test]
    fn budget_suggestion_clamped_path() {
        // Acrylic（像素族 560µs）持续超支约 220%（实耗 1_799_840ns，
        // 单帧值上限内）→ 新预算超家族封顶 1.5ms → 钳制单（clamped）。
        let mut eng = EffBudgetEngine::new();
        let mask = EffectKind::Acrylic.bit();
        let s = make_script(mask, 3_214, REF_MPX_PERMILLE);
        assert!(
            s.per_effect[5] > 1_388_888 && s.per_effect[5] <= COST_VALUE_MAX,
            "实耗 {} 须触发钳制且在上限内",
            s.per_effect[5]
        );
        let mut f = 0;
        while f < 320 {
            let _ = eng.run_frame(&s).unwrap();
            f += 1;
        }
        let sh = eng.suggestions().open_of(5).expect("持续重超支必出钳制单");
        assert!(sh.clamped, "新预算必须被家族封顶钳制");
        assert_eq!(sh.new_budget_ns as u64, family_cap_ns(BudgetFamily::Pixel));
    }

    // ---- 超标组：处置族 / 越权 / 注入检出 / 双跑一致 / 聚合 ----------

    #[test]
    fn overrun_disposition_ladder() {
        let mut h = OverrunHandler::new();
        // 连击 1、2：无动作。
        assert!(h.observe(0, true, 1).is_none());
        assert!(h.observe(0, true, 2).is_none());
        assert_eq!(h.level_of(0), 0);
        // 连击 3 → L1 降档。
        let a1 = h.observe(0, true, 3).unwrap();
        assert_eq!((a1.from, a1.to), (0, 1));
        assert_eq!(a1.reason, DispReason::StreakEscalate);
        assert_eq!(a1.keep_permille, 600);
        // 连击 6 → L2 跳帧。
        h.observe(0, true, 4);
        h.observe(0, true, 5);
        let a2 = h.observe(0, true, 6).unwrap();
        assert_eq!((a2.from, a2.to), (1, 2));
        // 连击 10 → L3 熔断（留存 0，开 20 帧冷却）。
        let mut n = 7;
        while n < 10 {
            h.observe(0, true, n);
            n += 1;
        }
        let a3 = h.observe(0, true, 10).unwrap();
        assert_eq!((a3.from, a3.to), (2, 3));
        assert_eq!(a3.keep_permille, 0);
        assert_eq!(h.level_of(0), 3);
        assert_eq!(h.breaker_total, 1);
        // 熔断期观测（false 驱动冷却倒计）：恰 20 帧倒尽。
        let mut c = 0;
        while c < 20 {
            assert!(h.observe(0, false, 11 + c).is_none());
            c += 1;
        }
        assert_eq!(h.level_of(0), 3);
        // 第 21 帧：冷却尽 → L2 半开。
        let ah = h.observe(0, false, 31).unwrap();
        assert_eq!((ah.from, ah.to), (3, 2));
        assert_eq!(ah.reason, DispReason::BreakerTimeout);
        // 滞回：连续 4 帧预算内逐级回落 2→1→0。
        let mut r = 0;
        while r < 4 {
            h.observe(0, false, 32 + r);
            r += 1;
        }
        assert_eq!(h.level_of(0), 1);
        let mut r2 = 0;
        while r2 < 4 {
            h.observe(0, false, 36 + r2);
            r2 += 1;
        }
        assert_eq!(h.level_of(0), 0, "滞回回落到位");
        // 轨迹逐级合法（无跨级）。
        assert!(h.trail_legal());
        assert_eq!(h.dropped_trail(), 0);
        // 留存投影模型（新处理器驱动到各级后验证 1000/600/500/0）。
        let mut hp = OverrunHandler::new();
        assert_eq!(hp.projected_cost(2, 100_000), 100_000, "L0 全量");
        hp.observe(2, true, 1);
        hp.observe(2, true, 2);
        hp.observe(2, true, 3);
        assert_eq!(hp.level_of(2), 1);
        assert_eq!(hp.projected_cost(2, 100_000), 60_000, "L1 降档留存 60%");
        hp.observe(2, true, 4);
        hp.observe(2, true, 5);
        hp.observe(2, true, 6);
        assert_eq!(hp.level_of(2), 2);
        assert_eq!(hp.projected_cost(2, 100_000), 50_000, "L2 跳帧留存 50%");
        let mut w = 7;
        while w < 10 {
            hp.observe(2, true, w);
            w += 1;
        }
        hp.observe(2, true, 10);
        assert_eq!(hp.level_of(2), 3);
        assert_eq!(hp.projected_cost(2, 100_000), 0, "L3 熔断归零");
    }

    #[test]
    fn overrun_external_request_gates() {
        let mut eng = EffBudgetEngine::new();
        // 无据：L0 且零超支史 → 直跳/逐级都被拒。
        let e1 = eng.request_disposition(0, 3).unwrap_err();
        assert_eq!(e1.code(), EffBudgetError::DispositionOverreach.code());
        let e2 = eng.request_disposition(0, 1).unwrap_err();
        assert_eq!(e2.code(), EffBudgetError::DispositionNoGround.code());
        assert_eq!(eng.handler().rejected_total, 2);
        // 级别非法：目标 4 / 目标 0（同级）。
        let e3 = eng.request_disposition(0, 4).unwrap_err();
        assert_eq!(e3.code(), EffBudgetError::DispositionLevelInvalid.code());
        let e4 = eng.request_disposition(0, 0).unwrap_err();
        assert_eq!(e4.code(), EffBudgetError::DispositionLevelInvalid.code());
        // 构造合法外部核准：超支/预算交替 6 轮（自动状态机停在 L0——
        // 无连击达阈），再补 1 帧超支（累计 7、连击 1，仍在 L0）——
        // 外部凭累计超支史申请 L1 核准（处置复用路径）。
        let mut h = OverrunHandler::new();
        let mut f = 1;
        while f <= 11 {
            h.observe(0, true, f);
            h.observe(0, false, f + 1);
            f += 2;
        }
        h.observe(0, true, 13);
        assert_eq!(h.level_of(0), 0, "交替超支不触发自动升级");
        assert_eq!(h.cumulative_over(0), 7);
        let ok = h.request_external(0, 1, 100).unwrap();
        assert_eq!((ok.from, ok.to), (0, 1));
        assert_eq!(ok.reason, DispReason::ExternalConfirm);
        // 外部核准后：滞回 4 帧回落 L0。
        let mut r = 0;
        while r < 4 {
            h.observe(0, false, 101 + r);
            r += 1;
        }
        assert_eq!(h.level_of(0), 0);
        assert!(h.trail_legal());
        // 无据（其二）：连击 1 但累计 1 < L1 阈值 3——依据不足。
        let mut h2 = OverrunHandler::new();
        h2.observe(0, true, 1);
        let e5 = h2.request_external(0, 1, 50).unwrap_err();
        assert_eq!(e5.code(), EffBudgetError::DispositionNoGround.code());
        // 无据（其三）：交替 3 轮止于预算内帧（连击 0）——无超支态势。
        let mut h3 = OverrunHandler::new();
        let mut g = 1;
        while g <= 5 {
            h3.observe(0, true, g);
            h3.observe(0, false, g + 1);
            g += 2;
        }
        let e6 = h3.request_external(0, 1, 60).unwrap_err();
        assert_eq!(e6.code(), EffBudgetError::DispositionNoGround.code());
    }

    #[test]
    fn overrun_engine_frame_flow() {
        let mut eng = EffBudgetEngine::new();
        // S5 景深场景 120% 负载 12 帧：帧超支 + 效果超支 + 处置升级事件。
        let mask = SCENE_MASKS[5];
        let mut f = 0;
        while f < 12 {
            let s = make_script(mask, 1_200, REF_MPX_PERMILLE);
            let out = eng.run_frame(&s).unwrap();
            if f + 1 >= 3 {
                // 连击 3 起 DoF/AntiAlias/GaussBlur/ToneMap 全体 L1+。
                assert!(!out.actions.is_empty() || out.levels.iter().any(|&l| l > 0));
            }
            assert!(out.actual_ns > out.budget_ns, "120% 负载帧必超支");
            assert!(out.over_ppm > 0);
            f += 1;
        }
        let dig = eng.aggregate().unwrap();
        assert_eq!(dig.frames, 12);
        assert!(dig.over_frames >= 12);
        assert_eq!(dig.achievement_bp, 0, "全程超支达成率 0‱");
        // 事件账自洽 + health 全绿。
        assert!(eng.events().consistent());
        let hr = eng.health();
        assert!(hr.all_ok, "超标运行后 health 仍须全绿：{}", hr.text);
    }

    #[test]
    fn overrun_injections_all_detected() {
        let mut eng = EffBudgetEngine::new();
        // 先跑 5 帧合法帧（有状态底座）。
        let mut f = 0;
        while f < 5 {
            let s = make_script(SCENE_MASKS[0], 1_000, REF_MPX_PERMILLE);
            let _ = eng.run_frame(&s).unwrap();
            f += 1;
        }
        // 三注入巡检：全部检出 + 正确错误码。
        let verdicts = injection_sweep(&mut eng).unwrap();
        assert_eq!(verdicts.len(), 3);
        assert!(verdicts[0].detected, "{}: {}", verdicts[0].name, verdicts[0].note);
        assert_eq!(verdicts[0].code, 0x1DB4);
        assert!(verdicts[1].detected, "{}: {}", verdicts[1].name, verdicts[1].note);
        assert_eq!(verdicts[1].code, 0x1DB7);
        assert!(verdicts[2].detected, "{}: {}", verdicts[2].name, verdicts[2].note);
        assert_eq!(verdicts[2].code, 0x1DB3);
        // 检出事件留痕：TamperDetected / OverreachRejected / ConservationBreak。
        let counts = eng.events().kind_counts();
        assert!(counts[EventKind::TamperDetected.as_u8() as usize] >= 1);
        assert!(counts[EventKind::OverreachRejected.as_u8() as usize] >= 1);
        assert!(counts[EventKind::ConservationBreak.as_u8() as usize] >= 1);
        // 注入自愈后：表指纹归位 + health 全绿。
        assert_eq!(eng.table().verify().unwrap(), TABLE_FINGERPRINT);
        assert!(eng.health().all_ok);
    }

    #[test]
    fn overrun_double_run_determinism() {
        // 混合脚本：合法帧 + 超支帧 + 份额越限帧（全开）——双跑对拍。
        let mut script: Vec<FrameScript> = Vec::new();
        let mut k = 0;
        while k < 40 {
            let m = match k % 4 {
                0 => SCENE_MASKS[1],
                1 => SCENE_MASKS[5],
                2 => 0xFFFF, // 全开（份额越限 + 大超支——处置族全链触发）。
                _ => SCENE_MASKS[3],
            };
            let mult = match k % 4 {
                0 => 1_000,
                1 => 1_500,
                2 => 2_000,
                _ => 700,
            };
            script.push(make_script(m, mult, REF_MPX_PERMILLE));
            k += 1;
        }
        let mut eng = EffBudgetEngine::new();
        let (fa, fb) = eng.run_script_det(&script).unwrap();
        assert_eq!(fa, fb, "双跑指纹必须逐位一致");
        // 两次独立调用同样一致（对拍本身确定性）。
        let (fa2, fb2) = eng.run_script_det(&script).unwrap();
        assert_eq!(fa, fa2);
        assert_eq!(fb, fb2);
        assert_eq!(eng.last_replay_fp(), Some((fa, fb)));
        // 事件留痕。
        let counts = eng.events().kind_counts();
        assert!(counts[EventKind::ReplayChecked.as_u8() as usize] >= 1);
        // 手工双跑（非门面）：两个全新引擎逐帧跑同一脚本，指纹一致。
        let mut a = EffBudgetEngine::new();
        let mut b = EffBudgetEngine::new();
        let mut i = 0;
        while i < script.len() {
            let _ = a.run_frame(&script[i]).unwrap();
            let _ = b.run_frame(&script[i]).unwrap();
            i += 1;
        }
        assert_eq!(a.transcript_fingerprint(), b.transcript_fingerprint());
        assert_eq!(a.transcript_fingerprint(), fa);
    }

    #[test]
    fn overrun_aggregation_top_stable_order() {
        // 两效果同超支率（420µs→630µs 与 210µs→315µs 均为 +500_000ppm），
        // Top 排序破平按下标升序（全序确定）。
        let mut led = BudgetLedger::new();
        let mask = EffectKind::GaussBlur.bit() | EffectKind::BoxBlur.bit();
        let mut n = 1;
        while n <= 10 {
            let mut pe = [0u32; EFF_KIND_COUNT];
            // 同超支率 500_000 ppm：420µs→630µs / 210µs→315µs。
            pe[0] = 630_000;
            pe[1] = 315_000;
            let mut ppm_arr = [0u32; EFF_KIND_COUNT];
            ppm_arr[0] = 500_000;
            ppm_arr[1] = 500_000;
            let total = 945_000u64;
            let _ = led.commit(
                n,
                REF_MPX_PERMILLE,
                mask,
                &pe,
                total,
                300_000,
                mask,
                &ppm_arr,
                false,
                0,
            );
            n += 1;
        }
        let dig = aggregate(&led).unwrap();
        assert_eq!(dig.top_overrun.len(), 2);
        assert_eq!(dig.top_overrun[0].effect, 0, "同 ppm 下标 0 在前");
        assert_eq!(dig.top_overrun[1].effect, 1);
        assert_eq!(dig.top_overrun[0].mean_ppm, 500_000);
        // 稳定排序：重复聚合输出一致。
        let dig2 = aggregate(&led).unwrap();
        assert_eq!(dig.fingerprint, dig2.fingerprint);
        // 分位正确性：P95 = 945_000（全同样本）。
        assert_eq!(dig.p95_ns, 945_000);
        assert_eq!(dig.p50_ns, 945_000);
        assert_eq!(dig.achievement_bp, 0);
        // 引擎路径聚合：S0 预算内 30 帧 → 达成率 10000‱。
        let mut eng2 = EffBudgetEngine::new();
        let mut f = 0;
        while f < 30 {
            let s = make_script(SCENE_MASKS[0], 1_000, REF_MPX_PERMILLE);
            let _ = eng2.run_frame(&s).unwrap();
            f += 1;
        }
        let d2 = eng2.aggregate().unwrap();
        assert_eq!(d2.achievement_bp, 10_000);
        assert_eq!(d2.over_frames, 0);
        // 空会话显性拒绝。
        let eng3 = EffBudgetEngine::new();
        assert_eq!(
            eng3.aggregate().unwrap_err().code(),
            EffBudgetError::WindowEmpty.code()
        );
    }

    #[test]
    fn budget_error_surface_closed() {
        // 16 变体一码一段互不重码（0x1DB0..0x1DBF 全覆盖）。
        let mut seen = [false; 16];
        let all = [
            EffBudgetError::EffectIndexRange,
            EffBudgetError::CostValueRange,
            EffBudgetError::PixelScaleRange,
            EffBudgetError::FrameConservation,
            EffBudgetError::TableFingerprintMismatch,
            EffBudgetError::TableFrozen,
            EffBudgetError::FrameSeqRegression,
            EffBudgetError::DispositionOverreach,
            EffBudgetError::DispositionNoGround,
            EffBudgetError::DispositionLevelInvalid,
            EffBudgetError::SuggestionApplyForbidden,
            EffBudgetError::SuggestionDuplicate,
            EffBudgetError::WindowEmpty,
            EffBudgetError::QuantilePermilleRange,
            EffBudgetError::EventKindRange,
            EffBudgetError::ReplayMismatch,
        ];
        let mut i = 0;
        while i < all.len() {
            let code = all[i].code();
            assert!(code >= SEG_BASE && code <= ERR_SEG_END);
            assert_eq!(code, SEG_BASE | i as u16);
            assert!(!seen[code as usize - SEG_BASE as usize]);
            seen[code as usize - SEG_BASE as usize] = true;
            // 每变体中文诊断非空且含可操作指引。
            assert!(all[i].detail().len() >= 10);
            assert!(!all[i].name().is_empty());
            i += 1;
        }
        // 事件闭集反查。
        assert!(EventKind::from_u8(16).is_err());
        assert_eq!(
            EventKind::from_u8(16).unwrap_err().code(),
            EffBudgetError::EventKindRange.code()
        );
        assert!(EventKind::from_u8(15).is_ok());
        // 效果闭集反查。
        assert!(EffectKind::from_u8(16).is_err());
        assert!(EffectKind::from_u8(15).is_ok());
        // 分位守门。
        assert_eq!(
            quantile_u64(&[], 500).unwrap_err().code(),
            EffBudgetError::WindowEmpty.code()
        );
        assert_eq!(
            quantile_u64(&[1, 2, 3], 1001).unwrap_err().code(),
            EffBudgetError::QuantilePermilleRange.code()
        );
        assert_eq!(quantile_u64(&[10, 20, 30, 40], 950).unwrap(), 40);
        assert_eq!(quantile_u64(&[10, 20, 30, 40], 500).unwrap(), 20);
        // 零预算超支饱和。
        assert_eq!(overrun_ppm(5, 0), u32::MAX);
        assert_eq!(overrun_ppm(0, 0), 0);
        assert_eq!(overrun_ppm(100, 100), 0);
        // 汇报与标签。
        let eng = EffBudgetEngine::new();
        assert!(eng.table().report().contains("高斯模糊"));
        assert!(eng.tag().contains("CGPU-F2087"));
        assert_eq!(eng.table().tamper_marks(), 0);
    }

    #[test]
    fn overrun_share_exceeded_and_amort_spike_in_engine() {
        let mut eng = EffBudgetEngine::new();
        // 全开 100%：份额越限帧（预算侧超载）但实耗按预算给 → 不超支。
        let s = make_script(0xFFFF, 1_000, REF_MPX_PERMILLE);
        let out = eng.run_frame(&s).unwrap();
        assert!(out.share_exceeded, "全开预算必越份额");
        assert_eq!(out.over_ppm, 0, "实耗=预算 → 帧不超支");
        let dig = eng.aggregate().unwrap();
        assert_eq!(dig.share_exceed_frames, 1);
        // Mica 尖峰帧：seq=16 时摊销预算抬到 2.4ms（钳制），预算内执行。
        let mut eng2 = EffBudgetEngine::new();
        let mica_mask = EffectKind::Mica.bit() | EffectKind::Dither.bit();
        let mut f = 0;
        while f < 16 {
            let mut s2 = make_script(mica_mask, 1_000, REF_MPX_PERMILLE);
            if f == 15 {
                // 第 16 帧（seq=16）Mica 尖峰：预算 2.4ms，给尖峰实耗
                // 1.9ms（单帧值上限 2ms 内、尖峰预算内）。
                s2.per_effect[6] = 1_900_000;
                s2.declared_total_ns = s2.per_effect[6] as u64 + s2.per_effect[9] as u64;
            }
            let out2 = eng2.run_frame(&s2).unwrap();
            if f == 15 {
                assert!(out2.budget_ns >= 2_400_000, "尖峰帧预算抬到钳制值");
                assert_eq!(out2.over_ppm, 0, "尖峰实耗在尖峰预算内");
            }
            f += 1;
        }
        // 尖峰帧不应触发建议单（稳态口径判热）。
        assert!(eng2.suggestions().open_of(6).is_none());
    }
}
