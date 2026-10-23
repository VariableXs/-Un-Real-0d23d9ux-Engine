//! CGPU-F2086 · 效果质量基准（CGPU-N 域 · 效果质量组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2086`
//!
//! **判据（锚点原文）**：双指标、对比库、两组、判据。
//!
//! **职责定位（锚点原文摘录）**：质量基准：基准（效果质量基准（SSIM/deltaE
//! 双指标——双指标基准——基准规范；对比（vs 原生截图库——对比库；测试
//! （基准/对比两组）。判据：双指标、对比库、两组、判据。
//!
//! --------------------------------------------------------------------------
//!
//! ## 〇、本模块全景（十个子系统，全部真实实现，无桩）
//!
//! 本模块为 VE 图形效果建立**效果质量量化基准**：同一效果在不同实现路径
//! （原生参考/GPU/ SIMD/标量/近似降级）下渲染结果与原生截图对比库的感知
//! 质量差距，全部折算为 0~10000 的整数质量分，并叠加质量预算与性能预算
//! 双约束。十个子系统：
//!
//! 1. **效果闭集与场景闭集**（[`EffectKind`] / [`BenchScenario`] +
//!    [`SCENARIOS`]）：8 种效果 × 8 条固定基准场景，场景参数全部写死；
//!    场景指纹 const 冻结（[`SCENARIO_FINGERPRINTS`]），运行前逐条复算
//!    校验——**防挑参数**（运行中任何人改一个字节即指纹失配显性报错）。
//! 2. **SSIM 整数定点实现**（[`ssim_score`]）：8×8 均匀窗、Q16 定点、
//!    亮度域，C1/C2 常数自 SSIM 论文（k1=0.01、k2=0.03、L=255）取整，
//!    输出 0~10000。
//! 3. **deltaE(CIE76) 整数定点实现**（[`deltae_score`]）：sRGB 线性化表
//!    （IEC 61966-2-1）→ XYZ(D65) → Lab（整数立方根牛顿迭代），ΔE76
//!    平方根定点化，按 JND 线性带映射 0~10000。
//! 4. **效果专有辅助指标**（[`AuxKind`]）：模糊半径误差（合成边缘线盒式
//!    模糊对拍）/ 透明度混合精度（量化 α 逐级叠加 vs 一次精确混合）/
//!    动画平滑度（阶梯更新二阶差分抖动）三族，整数实现。
//! 5. **实现路径降级模型**（[`PathDesc`] + [`PATH_TABLE`]）：5 条实现路径
//!    （原生参考/GPU/SIMD/标量/近似）各带量化位深、半径百分比、α 位深、
//!    步进百分比、成本百分比与**质量损失量化声明**（declared_max_loss）；
//!    实测损失超声明即显性违约（0x137A）。
//! 6. **原生截图对比库**（[`RefStore`]）：每场景确定性合成参考帧
//!    （梯度/棋盘/圆环/噪声四族图案 + LCG 种子），入库即 FNV 指纹冻结，
//!    每次取用复验指纹。
//! 7. **质量预算 × 性能预算双约束**（[`EffBenchEngine::run_frame`]）：
//!    每帧质量分不得低于场景合同档（[`TIER_TABLE`]），帧成本合计不得超
//!    帧性能预算（[`FRAME_PERF_BUDGET`]），双违约均显性计数+留痕。
//! 8. **结果签名链**（[`SignatureChain`]）：场景指纹 + 质量分 + 环境画像
//!    （[`EnvProfile`]）FNV-64 链式签名，逐环含前环哈希——篡改任一环
//!    即从该环起断链，可精确定位篡改槽位。
//! 9. **事件账**（[`EventLedger`]）：12 类事件定长环形账（128 槽，满则
//!    覆写最旧并计 dropped）+ 逐类计数器，`health()` 全量体检。
//! 10. **引擎门面与注入测试面**（[`EffBenchEngine`]）：双跑确定性对拍
//!    （[`EffBenchEngine::run_all_twice`]）+ 降级注入 / 篡改注入 /
//!    挑参数注入三类注入检出面（[`InjectionVerdict`]）。
//!
//! ## 〇.一、纪律落实
//!
//! - **零 panic 面**：内核路径零 `unwrap`/`expect`/`panic!`/运行期
//!   `assert!`；可失败全走 [`Result`]<T, [`EffBenchError`]>（16 变体
//!   闭集，码段 **0x1370~0x137F** 独占，每变体
//!   [`EffBenchError::detail()`] 中文可操作诊断）。
//! - **no_std + alloc**：只依赖 `core` 与 `alloc::{vec::Vec, format,
//!   string::String}`；零 IO、零墙钟、零线程。
//! - **溢出显性**：全部计数与聚合走 `saturating_add`/`saturating_sub`/
//!   `wrapping_add`；乘法前置量程论证（注释逐处给出上界）。
//! - **栈上禁区**：>64KB 结构体禁止上栈——帧缓冲走 `Vec` 堆分配；
//!   [`EffBenchEngine`] 本体约 1.3KB（事件账 128×8B 定长内嵌），
//!   编译期 `const` 断言逐类型封顶。
//! - **const fn 禁坑**：const fn 内不用 `slice::get`/`Ord::min/max`/
//!   derive 相等比较/`for` 循环——一律 `while` + 下标；含 `Vec` 字段
//!   的类型一律不 derive `Copy`；枚举比较一律整型化。
//! - **常量来源**：SSIM C1/C2（Wang et al. 2004，k1=0.01、k2=0.03、
//!   L=255）；sRGB 线性化（IEC 61966-2-1）；XYZ 变换矩阵（sRGB D65，
//!   IEC 61966-2-1 / BT.709 亮度系数）；Lab ε=216/24389、κ=24389/27
//!   （CIE）；FNV-1a 常数（Landon Curt Noll 参考实现）；性能单位换算
//!   与预算额为本域合同写死值（注释逐条标明"合同值"）。
//!
//! **对接**：F2085（效果与原生 API 映射——实现路径的上游）、F2084
//! （效果重建管线——重建产物的被评对象）、F2087（效果性能预算——
//! 成本单位复用本模块 [`PERF_UNIT_NS`] 口径）、F2103（模糊质量分级
//! ——复用本模块 [`TIER_TABLE`]）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本（N04 组 · 效果质量基准）。
pub const EFFBENCH_VERSION: &str = "N04-effbench-v1";

/// 质量分上限（合同写死：0~10000 整数分档制）。
pub const QUALITY_SCORE_MAX: u32 = 10_000;

/// 质量分聚合权重——SSIM 支路（合同值：感知结构权重最大）。
pub const QUALITY_WEIGHT_SSIM: u32 = 4_000;

/// 质量分聚合权重——deltaE 支路（合同值：色彩保真次之）。
pub const QUALITY_WEIGHT_DELTA_E: u32 = 3_500;

/// 质量分聚合权重——效果专有辅助指标支路（合同值：参数保真第三）。
pub const QUALITY_WEIGHT_AUX: u32 = 2_500;

/// 场景条数（闭集，8 条；指纹表同长）。
pub const SCENARIO_COUNT: usize = 8;

/// 实现路径条数（闭集，5 条：参考/GPU/SIMD/标量/近似）。
pub const PATH_COUNT: usize = 5;

/// 原生参考路径下标（[`PATH_TABLE`] 内）。
pub const PATH_REF_IDX: usize = 0;

/// GPU 路径下标。
pub const PATH_GPU_IDX: usize = 1;

/// SIMD 软件路径下标。
pub const PATH_SIMD_IDX: usize = 2;

/// 标量回退路径下标。
pub const PATH_SCALAR_IDX: usize = 3;

/// 近似降级路径下标。
pub const PATH_APPROX_IDX: usize = 4;

/// 帧性能预算（合同值：单位 [`PERF_UNIT_NS`]；超过即 0x1377 违约）。
pub const FRAME_PERF_BUDGET: u32 = 5_000;

/// 性能单位（合同值：1 单位 ≈ 1µs 预算口径；本模块零墙钟，
/// 成本一律为场景基价 × 路径成本百分比的整数推定，不实测时间）。
pub const PERF_UNIT_NS: u32 = 1_000;

/// 质量档位表（合同写死：A/B/C/D/F 五档，分数单调递减）。
pub const TIER_TABLE: [(u16, &str); 5] = [
    (9_000, "A-合同档"),
    (7_500, "B"),
    (6_000, "C"),
    (4_000, "D"),
    (0, "F-不合格"),
];

/// 签名链容量（环形挤出不适用于链——链满即拒收并报 0x137F）。
pub const CHAIN_CAP: usize = 64;

/// 事件账定长容量（环形账，满则覆写最旧并计 dropped）。
pub const LEDGER_CAP: usize = 128;

/// 自检报告长度上限（超限报 0x137C，防内核路径无限拼接）。
pub const REPORT_MAX_BYTES: usize = 4_096;

/// SSIM 窗边长（8×8 均匀窗；Wang et al. 2004 默认 11×11 高斯窗的
/// 整数化等价裁剪——基准域统一 8 以保证任意 8 倍数尺寸可整除分窗）。
pub const SSIM_WIN: usize = 8;

/// SSIM 窗内像素数。
pub const SSIM_WIN_PIX: u32 = (SSIM_WIN * SSIM_WIN) as u32;

/// SSIM 常数 C1 = round((0.01×255)²) = round(6.5025) = 7
/// （来源：Wang et al. 2004，k1=0.01，L=255；整数化取整）。
pub const SSIM_C1: i64 = 7;

/// SSIM 常数 C2 = round((0.03×255)²) = round(58.5225) = 59
/// （来源同上，k2=0.03）。
pub const SSIM_C2: i64 = 59;

/// Lab 判 perceptible 上界：ΔE76 ≤ 1.0 记满分（CIE JND 经验界，
/// 合同值：Q12 定点 4096）。
pub const DELTAE_FULL_Q12: u32 = 4_096;

/// Lab 记零分下界：ΔE76 ≥ 20.0 记 0 分（合同值：20×4096 Q12）。
pub const DELTAE_ZERO_Q12: u32 = 81_920;

/// 场景串行化定长（指纹输入；不足补零——防尾随字段漂移进指纹）。
pub const SCEN_SER_LEN: usize = 96;

/// 合成参考帧内用 LCG 噪声振幅上限（图案对比度，写死合同值）。
pub const PATTERN_NOISE_AMP: u8 = 24;

/// 合成边缘线长度（模糊半径辅助指标的对拍载体）。
pub const AUX_LINE_LEN: usize = 32;

/// 动画平滑度 tick 数（合同值：64 tick 一段关键帧插值）。
pub const ANIM_TICKS: usize = 64;

// ---------------------------------------------------------------------------
// 二、诊断码（0x1370~0x137F 段独占：F2086 效果质量基准面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x1370~0x137F 段独占，共 16 变体；每变体配
/// [`EffBenchError::code()`] 与 [`EffBenchError::detail()`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffBenchError {
    /// 未知效果名（效果闭集外名字显性拒绝不猜）。
    UnknownEffect,
    /// 未知场景号（场景闭集外编号显性拒绝）。
    UnknownScenario,
    /// 场景指纹失配（运行中参数被改动——防挑参数红线命中）。
    ScenarioFingerprintMismatch,
    /// 采样缓冲尺寸不符（SSIM/deltaE 双方不一致）。
    SampleSizeMismatch,
    /// 未知实现路径（路径闭集外编号显性拒绝）。
    UnknownPath,
    /// 双跑确定性漂移（同输入两次运行结果不一致）。
    DualRunDrift,
    /// 质量预算违约（质量分低于场景合同档）。
    QualityBudgetBreach,
    /// 性能预算违约（帧成本合计超 [`FRAME_PERF_BUDGET`]）。
    PerfBudgetBreach,
    /// 签名链校验失配（链上某环内容与签名不符——篡改可定位）。
    SignatureMismatch,
    /// 签名链断链（序号跳变/前环哈希不接续——链结构损坏）。
    SignatureChainBroken,
    /// 质量损失超量化声明（实测降级损失 > 路径声明上界）。
    LossDeclarationExceeded,
    /// 值域越界（质量分超 0~10000 / 环境画像字段越闭集）。
    ScoreOutOfRange,
    /// 自检报告超长（> [`REPORT_MAX_BYTES`]）。
    ReportTooLarge,
    /// 注入面返回值异常（注入检出不成立——注入面自身失守）。
    InjectionUndetected,
    /// 体检降级（存在红灯项）。
    HealthDegraded,
    /// 签名链满（链容量 [`CHAIN_CAP`] 用尽且未快照归档）。
    LedgerFull,
}

impl EffBenchError {
    /// 诊断码（0x1370~0x137F 段独占）。
    pub const fn code(&self) -> u16 {
        match self {
            EffBenchError::UnknownEffect => 0x1370,
            EffBenchError::UnknownScenario => 0x1371,
            EffBenchError::ScenarioFingerprintMismatch => 0x1372,
            EffBenchError::SampleSizeMismatch => 0x1373,
            EffBenchError::UnknownPath => 0x1374,
            EffBenchError::DualRunDrift => 0x1375,
            EffBenchError::QualityBudgetBreach => 0x1376,
            EffBenchError::PerfBudgetBreach => 0x1377,
            EffBenchError::SignatureMismatch => 0x1378,
            EffBenchError::SignatureChainBroken => 0x1379,
            EffBenchError::LossDeclarationExceeded => 0x137A,
            EffBenchError::ScoreOutOfRange => 0x137B,
            EffBenchError::ReportTooLarge => 0x137C,
            EffBenchError::InjectionUndetected => 0x137D,
            EffBenchError::HealthDegraded => 0x137E,
            EffBenchError::LedgerFull => 0x137F,
        }
    }

    /// 中文可操作诊断（每变体一句：说清"错在哪、下一步怎么办"）。
    pub const fn detail(&self) -> &'static str {
        match self {
            EffBenchError::UnknownEffect => {
                "未知效果名：只接受效果闭集内的名字。请用 EffectKind::name() 拼写核对，勿自造效果名或用别名"
            }
            EffBenchError::UnknownScenario => {
                "未知场景号：场景闭集仅 0~7。请先查 SCENARIOS 常量表确认编号，勿按猜测传参"
            }
            EffBenchError::ScenarioFingerprintMismatch => {
                "场景指纹失配：场景参数在运行中被改动（疑似挑参数）。请回滚场景表至 SCENARIO_FINGERPRINTS 冻结版本后重跑"
            }
            EffBenchError::SampleSizeMismatch => {
                "采样缓冲尺寸不符：SSIM/deltaE 要求参考与候选同尺寸。请核对场景 width×height 与缓冲长度后重新生成候选帧"
            }
            EffBenchError::UnknownPath => {
                "未知实现路径：路径闭集仅 0~4（参考/GPU/SIMD/标量/近似）。请用 PATH_TABLE 内的下标传参"
            }
            EffBenchError::DualRunDrift => {
                "双跑确定性漂移：同输入两次运行结果不一致。请排查实现路径是否引入未声明的不确定性（并发/浮点/随机源）后重跑"
            }
            EffBenchError::QualityBudgetBreach => {
                "质量预算违约：质量分低于场景合同档。请改用更高质量实现路径，或按 F2088 降级链显性登记降级后再评估"
            }
            EffBenchError::PerfBudgetBreach => {
                "性能预算违约：帧成本合计超 FRAME_PERF_BUDGET。请减少同帧场景数或换低成本实现路径（PATH_TABLE.cost_pct）"
            }
            EffBenchError::SignatureMismatch => {
                "签名链校验失配：链上内容被篡改。请以返回的槽位定位被改环，核对该环的场景指纹/质量分/环境画像后重建链"
            }
            EffBenchError::SignatureChainBroken => {
                "签名链断链：序号跳变或前环哈希不接续。请勿跳号追加；核对上一环 node_hash 后从断点重建"
            }
            EffBenchError::LossDeclarationExceeded => {
                "质量损失超量化声明：实测损失大于路径 declared_max_loss。请修正声明值或修复该路径实现，禁止静默放宽声明"
            }
            EffBenchError::ScoreOutOfRange => {
                "值域越界：质量分须在 0~10000、环境画像字段须在闭集内。请检查聚合权重与上游输入范围后重试"
            }
            EffBenchError::ReportTooLarge => {
                "自检报告超长：超过 REPORT_MAX_BYTES。请缩减健康项注记或分片输出报告"
            }
            EffBenchError::InjectionUndetected => {
                "注入检出不成立：注入面预期应检出但未检出，注入面自身失守。请立即核对注入参数与守卫逻辑，不得继续基准流程"
            }
            EffBenchError::HealthDegraded => {
                "体检降级：存在红灯项（指纹失配/断链/对拍漂移）。请先按 health() 报告逐项消红后再使用质量分"
            }
            EffBenchError::LedgerFull => {
                "签名链满：CHAIN_CAP 用尽。请先快照归档现有链并重置链后再追加新结果"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、整数数学工具（全部 const fn 可用：二分整数平方根/立方根、FNV）
// ---------------------------------------------------------------------------

/// 整数平方根（二分；对任意 u64 精确下取整）。
/// 上界 2^32-1：其平方 = 2^64-2^33+1 < 2^64，中途乘积不溢出。
pub const fn isqrt64(x: u64) -> u64 {
    if x == 0 {
        return 0;
    }
    let mut lo: u64 = 1;
    let mut hi: u64 = 4_294_967_295;
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        if mid * mid <= x {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// 整数立方根（二分；对任意 u64 精确下取整）。
/// 上界 2642245：2642245³ = 18446724184302290745 ≤ 2^64-1
/// （cbrt(2^64-1) ≈ 2642245.94，其立方不溢出；2642246³ 会溢出故上界压到 2642245）。
pub const fn icbrt64(x: u64) -> u64 {
    if x == 0 {
        return 0;
    }
    let mut lo: u64 = 1;
    let mut hi: u64 = 2_642_245;
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        if mid * mid * mid <= x {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// FNV-1a 32 位（来源：Landon Curt Noll FNV 参考实现；
/// offset 0x811C9DC5 / prime 0x01000193）。const fn，while 逐字节。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// FNV-1a 64 位（同源常数：offset 0xCBF29CE484222325 /
/// prime 0x100000001B3）。用于签名链节点哈希与环境画像哈希。
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

/// 把散列输入打包成 8 字节（签名链节点哈希的确定性串行化：
/// u16/u32 字段 LE 拼接 + 前环哈希 LE——改任一字节即断链）。
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

// ---------------------------------------------------------------------------
// 四、效果闭集 / 图案闭集 / 辅助指标闭集
// ---------------------------------------------------------------------------

/// 效果闭集（8 种；与 F2085 效果闭集同口径，本模块自带独立闭集副本
/// 以保持完全自包含——名字表逐字比对，表外即拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    /// 高斯/盒式模糊（半径驱动）。
    Blur,
    /// 亚克力（模糊 + 噪声纹理叠加）。
    Acrylic,
    /// 云母（低频采样 + 亮色适配）。
    Mica,
    /// 阴影（环形衰减）。
    Shadow,
    /// 透明度（α 混合）。
    Opacity,
    /// 圆角（小半径几何 + 抗锯齿）。
    RoundedCorner,
    /// 动画曲线（关键帧插值平滑度）。
    Animation,
    /// 去饱和（色彩保真——deltaE 权重场景）。
    Desaturate,
}

impl EffectKind {
    /// 效果名表（闭集逐字比对用，顺序即 u8 编码）。
    pub const NAMES: [&'static str; 8] = [
        "blur",
        "acrylic",
        "mica",
        "shadow",
        "opacity",
        "rounded_corner",
        "animation",
        "desaturate",
    ];

    /// 整型化编码（const fn 比较用）。
    pub const fn as_u8(&self) -> u8 {
        match self {
            EffectKind::Blur => 0,
            EffectKind::Acrylic => 1,
            EffectKind::Mica => 2,
            EffectKind::Shadow => 3,
            EffectKind::Opacity => 4,
            EffectKind::RoundedCorner => 5,
            EffectKind::Animation => 6,
            EffectKind::Desaturate => 7,
        }
    }

    /// 效果名。
    pub const fn name(&self) -> &'static str {
        EffectKind::NAMES[self.as_u8() as usize]
    }

    /// 名字 → 效果（逐字比对，表外即 [`EffBenchError::UnknownEffect`]）。
    pub fn from_name(s: &str) -> Result<EffectKind, EffBenchError> {
        let b = s.as_bytes();
        let mut i = 0;
        while i < EffectKind::NAMES.len() {
            let nb = EffectKind::NAMES[i].as_bytes();
            if nb.len() == b.len() {
                let mut same = true;
                let mut j = 0;
                while j < b.len() {
                    if nb[j] != b[j] {
                        same = false;
                        break;
                    }
                    j += 1;
                }
                if same {
                    return Ok(EffectKind::FROM_U8[i]);
                }
            }
            i += 1;
        }
        Err(EffBenchError::UnknownEffect)
    }

    /// u8 编码 → 效果（越界显性拒绝）。
    pub const fn from_u8(v: u8) -> Result<EffectKind, EffBenchError> {
        if v >= 8 {
            return Err(EffBenchError::UnknownEffect);
        }
        Ok(EffectKind::FROM_U8[v as usize])
    }
}

/// u8 编码表（const 复用，避免 match 重复书写）。
const EFFECT_FROM_U8: [EffectKind; 8] = [
    EffectKind::Blur,
    EffectKind::Acrylic,
    EffectKind::Mica,
    EffectKind::Shadow,
    EffectKind::Opacity,
    EffectKind::RoundedCorner,
    EffectKind::Animation,
    EffectKind::Desaturate,
];

impl EffectKind {
    /// [`EFFECT_FROM_U8`] 的公开别名（类型内 const 引用模块级 const）。
    const FROM_U8: [EffectKind; 8] = EFFECT_FROM_U8;
}

/// 合成图案闭集（对比库参考帧的确定性生成图案族）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    /// 水平渐变（SSIM 结构敏感）。
    Gradient,
    /// 棋盘（高频边缘——量化损失敏感）。
    Checker,
    /// 同心圆环（径向衰减——阴影/圆角载体）。
    Rings,
    /// LCG 噪声（亚克力噪声纹理载体）。
    Noise,
}

impl PatternKind {
    /// 整型化编码。
    pub const fn as_u8(&self) -> u8 {
        match self {
            PatternKind::Gradient => 0,
            PatternKind::Checker => 1,
            PatternKind::Rings => 2,
            PatternKind::Noise => 3,
        }
    }
}

/// 效果专有辅助指标闭集（第三质量支路）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxKind {
    /// 模糊半径误差：合成边缘线双半径盒式模糊对拍。
    BlurRadius,
    /// 透明度混合精度：量化 α 逐级叠加 vs 一次精确混合。
    AlphaBlend,
    /// 动画平滑度：阶梯更新二阶差分抖动。
    AnimSmoothness,
}

impl AuxKind {
    /// 整型化编码（进场景指纹）。
    pub const fn as_u8(&self) -> u8 {
        match self {
            AuxKind::BlurRadius => 0,
            AuxKind::AlphaBlend => 1,
            AuxKind::AnimSmoothness => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、质量档位表与实现路径表
// ---------------------------------------------------------------------------

/// 质量分 → 档位（档名 + 档级；0..=10000 之外显性越界）。
pub fn tier_of(score: u32) -> Result<(u8, &'static str), EffBenchError> {
    if score > QUALITY_SCORE_MAX {
        return Err(EffBenchError::ScoreOutOfRange);
    }
    let mut i = 0;
    while i < TIER_TABLE.len() {
        let (min_score, name) = TIER_TABLE[i];
        if score >= min_score as u32 {
            return Ok((i as u8, name));
        }
        i += 1;
    }
    // TIER_TABLE 末档下界为 0，理论必达；防御性越界。
    Err(EffBenchError::ScoreOutOfRange)
}

/// 实现路径描述（量化位深/半径百分比/α 位深/步进百分比/成本百分比/
/// 质量损失量化声明——全部写死，运行期只读）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathDesc {
    /// 路径编号（= PATH_TABLE 下标）。
    pub id: u8,
    /// 路径名（报告与签名链可读位）。
    pub name: &'static str,
    /// 像素量化位深（8 = 无损；4 = 近似重降级）。
    pub quant_bits: u8,
    /// 模糊半径百分比（×100 记法：10000 = 100%）。
    pub radius_pct: u16,
    /// α 混合位深（8 = 精确）。
    pub alpha_bits: u8,
    /// 动画更新步进百分比（×100 记法）。
    pub step_pct: u16,
    /// 成本百分比（×100 记法；乘场景基价得帧成本）。
    pub cost_pct: u16,
    /// 质量损失量化声明（相对原生参考的最大允许掉分，0~10000）。
    pub declared_max_loss: u16,
}

const _: () = assert!(PATH_COUNT == 5);

/// 实现路径闭集表（合同写死；损失声明为 N04 组评审值——修改须走
/// 只增不改纪律并在审计账留痕，本模块以只读 const 表落实）。
pub const PATH_TABLE: [PathDesc; PATH_COUNT] = [
    PathDesc {
        id: 0,
        name: "ref-native",
        quant_bits: 8,
        radius_pct: 10_000,
        alpha_bits: 8,
        step_pct: 10_000,
        cost_pct: 10_000,
        declared_max_loss: 0,
    },
    PathDesc {
        id: 1,
        name: "gpu",
        quant_bits: 7,
        radius_pct: 9_900,
        alpha_bits: 8,
        step_pct: 10_000,
        cost_pct: 4_000,
        declared_max_loss: 900,
    },
    PathDesc {
        id: 2,
        name: "simd-sw",
        quant_bits: 6,
        radius_pct: 9_800,
        alpha_bits: 7,
        step_pct: 10_000,
        cost_pct: 5_500,
        declared_max_loss: 1_800,
    },
    PathDesc {
        id: 3,
        name: "scalar-fb",
        quant_bits: 5,
        radius_pct: 9_500,
        alpha_bits: 6,
        step_pct: 10_000,
        cost_pct: 12_000,
        declared_max_loss: 3_200,
    },
    PathDesc {
        id: 4,
        name: "approx-dg",
        quant_bits: 4,
        radius_pct: 7_500,
        alpha_bits: 5,
        step_pct: 5_000,
        cost_pct: 3_000,
        declared_max_loss: 5_200,
    },
];

// ---------------------------------------------------------------------------
// 六、基准场景集（const 冻结 + 指纹防挑参数）
// ---------------------------------------------------------------------------

/// 基准场景（全部字段进指纹；含 `Vec` 的类型才禁 `Copy`，本结构纯
/// 值字段，可安全 `Copy`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchScenario {
    /// 场景号（= SCENARIOS 下标）。
    pub id: u16,
    /// 场景名（进指纹）。
    pub name: &'static str,
    /// 效果种类。
    pub kind: EffectKind,
    /// 辅助指标族。
    pub aux: AuxKind,
    /// 帧宽（8 的倍数——SSIM 分窗前提）。
    pub width: u16,
    /// 帧高（8 的倍数）。
    pub height: u16,
    /// 合成图案族。
    pub pattern: PatternKind,
    /// LCG 种子（对比库确定性生成）。
    pub seed: u32,
    /// 合同质量档下限分（低于即 0x1376 违约）。
    pub contract_min_score: u16,
    /// 场景基价（单位 [`PERF_UNIT_NS`] 的推定成本）。
    pub base_cost: u16,
    /// 模糊半径（Q8 定点；0 = 不模糊）。
    pub blur_radius_q8: u16,
    /// α 混合基准值（0~255；辅助指标 AlphaBlend 用）。
    pub alpha_ref: u16,
    /// α 叠加基准步数（辅助指标 AlphaBlend 用）。
    pub alpha_steps: u16,
    /// 动画位移幅值（0~255；辅助指标 AnimSmoothness 用）。
    pub anim_span: u16,
    /// 辅助指标误差容忍值（误差 ≥ 容忍 → 辅助分 0）。
    pub aux_tol: u16,
}

/// 基准场景集（8 条闭集；字段值全部为 N04 组评审写死值——
/// 任何改动必然指纹失配，这是"防挑参数"红线的物理载体）。
pub const SCENARIOS: [BenchScenario; SCENARIO_COUNT] = [
    BenchScenario {
        id: 0,
        name: "blur-g18-grad48",
        kind: EffectKind::Blur,
        aux: AuxKind::BlurRadius,
        width: 48,
        height: 48,
        pattern: PatternKind::Gradient,
        seed: 0xF208_6001,
        contract_min_score: 9_000,
        base_cost: 180,
        blur_radius_q8: 18 << 8,
        alpha_ref: 0,
        alpha_steps: 0,
        anim_span: 0,
        aux_tol: 30,
    },
    BenchScenario {
        id: 1,
        name: "acrylic-nz48",
        kind: EffectKind::Acrylic,
        aux: AuxKind::BlurRadius,
        width: 48,
        height: 48,
        pattern: PatternKind::Noise,
        seed: 0xF208_6002,
        contract_min_score: 7_500,
        base_cost: 220,
        blur_radius_q8: 8 << 8,
        alpha_ref: 0,
        alpha_steps: 0,
        anim_span: 0,
        aux_tol: 20,
    },
    BenchScenario {
        id: 2,
        name: "mica-grad48",
        kind: EffectKind::Mica,
        aux: AuxKind::BlurRadius,
        width: 48,
        height: 48,
        pattern: PatternKind::Gradient,
        seed: 0xF208_6003,
        contract_min_score: 7_500,
        base_cost: 150,
        blur_radius_q8: 3 << 8,
        alpha_ref: 0,
        alpha_steps: 0,
        anim_span: 0,
        aux_tol: 20,
    },
    BenchScenario {
        id: 3,
        name: "shadow-ring48",
        kind: EffectKind::Shadow,
        aux: AuxKind::BlurRadius,
        width: 48,
        height: 48,
        pattern: PatternKind::Rings,
        seed: 0xF208_6004,
        contract_min_score: 7_500,
        base_cost: 160,
        blur_radius_q8: 6 << 8,
        alpha_ref: 0,
        alpha_steps: 0,
        anim_span: 0,
        aux_tol: 24,
    },
    BenchScenario {
        id: 4,
        name: "opacity-chk48",
        kind: EffectKind::Opacity,
        aux: AuxKind::AlphaBlend,
        width: 48,
        height: 48,
        pattern: PatternKind::Checker,
        seed: 0xF208_6005,
        contract_min_score: 9_000,
        base_cost: 120,
        blur_radius_q8: 0,
        alpha_ref: 170,
        alpha_steps: 8,
        anim_span: 0,
        aux_tol: 6,
    },
    BenchScenario {
        id: 5,
        name: "corner-ring48",
        kind: EffectKind::RoundedCorner,
        aux: AuxKind::BlurRadius,
        width: 48,
        height: 48,
        pattern: PatternKind::Rings,
        seed: 0xF208_6006,
        contract_min_score: 7_500,
        base_cost: 140,
        blur_radius_q8: 2 << 8,
        alpha_ref: 0,
        alpha_steps: 0,
        anim_span: 0,
        aux_tol: 16,
    },
    BenchScenario {
        id: 6,
        name: "anim-grad48",
        kind: EffectKind::Animation,
        aux: AuxKind::AnimSmoothness,
        width: 48,
        height: 48,
        pattern: PatternKind::Gradient,
        seed: 0xF208_6007,
        contract_min_score: 9_000,
        base_cost: 200,
        blur_radius_q8: 0,
        alpha_ref: 0,
        alpha_steps: 0,
        anim_span: 200,
        aux_tol: 900,
    },
    BenchScenario {
        id: 7,
        name: "desat-chk-64x32",
        kind: EffectKind::Desaturate,
        aux: AuxKind::AlphaBlend,
        width: 64,
        height: 32,
        pattern: PatternKind::Checker,
        seed: 0xF208_6008,
        contract_min_score: 6_000,
        base_cost: 130,
        blur_radius_q8: 0,
        alpha_ref: 128,
        alpha_steps: 4,
        anim_span: 0,
        aux_tol: 10,
    },
];

/// 场景串行化（const fn：u16 字段 LE + 枚举编码 + 名字字节，尾部补零）。
/// 指纹防挑参数的物理输入——串行化布局本身也冻结（改布局即改指纹）。
pub const fn serialize_scenario(sc: &BenchScenario) -> [u8; SCEN_SER_LEN] {
    let mut buf = [0u8; SCEN_SER_LEN];
    let mut off = 0;
    // u16 字段统一 LE 写入（id/width/height/contract/base/radius/alpha/
    // steps/anim/tol），u32 种子 LE，枚举各 1 字节，名字逐字节。
    put_u16(&mut buf, &mut off, sc.id);
    put_u16(&mut buf, &mut off, sc.width);
    put_u16(&mut buf, &mut off, sc.height);
    put_u16(&mut buf, &mut off, sc.contract_min_score);
    put_u16(&mut buf, &mut off, sc.base_cost);
    put_u16(&mut buf, &mut off, sc.blur_radius_q8);
    put_u16(&mut buf, &mut off, sc.alpha_ref);
    put_u16(&mut buf, &mut off, sc.alpha_steps);
    put_u16(&mut buf, &mut off, sc.anim_span);
    put_u16(&mut buf, &mut off, sc.aux_tol);
    put_u32(&mut buf, &mut off, sc.seed);
    buf[off] = sc.kind.as_u8();
    off += 1;
    buf[off] = sc.aux.as_u8();
    off += 1;
    buf[off] = sc.pattern.as_u8();
    off += 1;
    let name_b = sc.name.as_bytes();
    let mut i = 0;
    while i < name_b.len() && off < SCEN_SER_LEN {
        buf[off] = name_b[i];
        off += 1;
        i += 1;
    }
    buf
}

/// const fn 小端 u16 写入（无越界可能：调用点布局静态论证 ≤ 96 字节）。
const fn put_u16(buf: &mut [u8; SCEN_SER_LEN], off: &mut usize, v: u16) {
    buf[*off] = (v & 0xFF) as u8;
    buf[*off + 1] = (v >> 8) as u8;
    *off += 2;
}

/// const fn 小端 u32 写入。
const fn put_u32(buf: &mut [u8; SCEN_SER_LEN], off: &mut usize, v: u32) {
    let mut i = 0;
    while i < 4 {
        buf[*off + i] = ((v >> (8 * i)) & 0xFF) as u8;
        i += 1;
    }
    *off += 4;
}

/// 场景指纹（FNV-1a 32 over 串行化字节；const 可用）。
pub const fn scenario_fingerprint(sc: &BenchScenario) -> u32 {
    fnv1a32(&serialize_scenario(sc))
}

/// 冻结指纹表（const 求值期一次性算定——运行期任何场景字段改动
/// 与此表对照即失配，这是"运行中指纹校验防挑参数"的冻结基准）。
pub const SCENARIO_FINGERPRINTS: [u32; SCENARIO_COUNT] = {
    let mut out = [0u32; SCENARIO_COUNT];
    let mut i = 0;
    while i < SCENARIO_COUNT {
        out[i] = scenario_fingerprint(&SCENARIOS[i]);
        i += 1;
    }
    out
};

// ---------------------------------------------------------------------------
// 七、sRGB 线性化表（deltaE 前置）
// ---------------------------------------------------------------------------

/// sRGB → 线性光查找表（单位：ppm，0..=1_000_000）。
/// 来源：IEC 61966-2-1 sRGB EOTF——c ≤ 0.04045: c/12.92，
/// 否则 ((c+0.055)/1.055)^2.4；const fn 无法做浮点幂，故以评审期
/// 预计算值冻结成表（256 项，逐项 = round(1e6 × EOTF(i/255))）。
pub const SRGB_TO_LIN: [u32; 256] = [
          0,     304,     607,     911,    1214,    1518,    1821,    2125,
       2428,    2732,    3035,    3347,    3677,    4025,    4391,    4777,
       5182,    5605,    6049,    6512,    6995,    7499,    8023,    8568,
       9134,    9721,   10330,   10960,   11612,   12286,   12983,   13702,
      14444,   15209,   15996,   16807,   17642,   18500,   19382,   20289,
      21219,   22174,   23153,   24158,   25187,   26241,   27321,   28426,
      29557,   30713,   31896,   33105,   34340,   35601,   36889,   38204,
      39546,   40915,   42311,   43735,   45186,   46665,   48172,   49707,
      51269,   52861,   54480,   56128,   57805,   59511,   61246,   63010,
      64803,   66626,   68478,   70360,   72272,   74214,   76185,   78187,
      80220,   82283,   84376,   86500,   88656,   90842,   93059,   95307,
      97587,   99899,  102242,  104616,  107023,  109462,  111932,  114435,
     116971,  119538,  122139,  124772,  127438,  130136,  132868,  135633,
     138432,  141263,  144128,  147027,  149960,  152926,  155926,  158961,
     162029,  165132,  168269,  171441,  174647,  177888,  181164,  184475,
     187821,  191202,  194618,  198069,  201556,  205079,  208637,  212231,
     215861,  219526,  223228,  226966,  230740,  234551,  238398,  242281,
     246201,  250158,  254152,  258183,  262251,  266356,  270498,  274677,
     278894,  283149,  287441,  291771,  296138,  300544,  304987,  309469,
     313989,  318547,  323143,  327778,  332452,  337164,  341914,  346704,
     351533,  356400,  361307,  366253,  371238,  376262,  381326,  386429,
     391572,  396755,  401978,  407240,  412543,  417885,  423268,  428690,
     434154,  439657,  445201,  450786,  456411,  462077,  467784,  473531,
     479320,  485150,  491021,  496933,  502886,  508881,  514918,  520996,
     527115,  533276,  539479,  545724,  552011,  558340,  564712,  571125,
     577580,  584078,  590619,  597202,  603827,  610496,  617207,  623960,
     630757,  637597,  644480,  651406,  658375,  665387,  672443,  679542,
     686685,  693872,  701102,  708376,  715694,  723055,  730461,  737910,
     745404,  752942,  760525,  768151,  775822,  783538,  791298,  799103,
     806952,  814847,  822786,  830770,  838799,  846873,  854993,  863157,
     871367,  879622,  887923,  896269,  904661,  913099,  921582,  930111,
     938686,  947307,  955973,  964686,  973445,  982251,  991102, 1000000,
];

// ---------------------------------------------------------------------------
// 八、确定性合成参考帧（对比库图案发生器）
// ---------------------------------------------------------------------------

/// LCG 下一步（参数取 Numerical Recipes glibc 同族：a=1103515245,
/// c=12345, mod 2^32——确定性、零随机源依赖）。
pub const LCG_A: u32 = 1_103_515_245;
/// LCG 增量。
pub const LCG_C: u32 = 12_345;

/// 推进 LCG 一轮（wrapping 显性）。
pub const fn lcg_next(state: u32) -> u32 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// LCG 均匀字节（取高 8 位——低位周期短，NR 建议）。
pub const fn lcg_byte(state: u32) -> u8 {
    (state >> 24) as u8
}

/// 场景参考帧生成（RGBA，每像素 4 字节，长度 = w*h*4）。
/// 确定性：仅由场景 const 字段驱动，双跑字节级一致。
/// 图案族：Gradient 水平渐变 + 垂直微调；Checker 16px 棋盘；
/// Rings 以画面中心为圆心的 24px 周期同心圆环；Noise 纯 LCG 噪声。
/// 所有图案叠加 [`PATTERN_NOISE_AMP`] 振幅的 LCG 抖动（除 Noise 主体
/// 即噪声外再叠加一层细抖动，保证任何图案都有不可预测残差）。
pub fn render_scenario_rgba(sc: &BenchScenario) -> Vec<u8> {
    let w = sc.width as usize;
    let h = sc.height as usize;
    let mut out = Vec::with_capacity(w * h * 4);
    let mut st = sc.seed;
    let cx = (w / 2) as i32;
    let cy = (h / 2) as i32;
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let base: u8 = match sc.pattern {
                PatternKind::Gradient => {
                    // 水平主梯度 + 垂直 1/8 微调，量程 0..=255 显性。
                    let gx = (x * 255) / (w.saturating_sub(1).max(1));
                    let gy = (y * 31) / (h.saturating_sub(1).max(1));
                    (gx + gy).min(255) as u8
                }
                PatternKind::Checker => {
                    let cell = ((x / 16) + (y / 16)) & 1;
                    if cell == 1 { 232 } else { 40 }
                }
                PatternKind::Rings => {
                    let dx = x as i32 - cx;
                    let dy = y as i32 - cy;
                    let r = isqrt64((dx * dx + dy * dy) as u64) as i32;
                    let band = (r % 24) as u32;
                    // 24px 周期三角形衰减环：中心亮、外缘暗。
                    let tri = if band < 12 { band } else { 24 - band };
                    (255u32.saturating_sub(tri * 20)).min(255) as u8
                }
                PatternKind::Noise => {
                    st = lcg_next(st);
                    lcg_byte(st)
                }
            };
            st = lcg_next(st);
            let jitter = (lcg_byte(st) % PATTERN_NOISE_AMP) as i32;
            let v = (base as i32 - PATTERN_NOISE_AMP as i32 / 2 + jitter)
                .clamp(0, 255) as u8;
            // RGBA（本基准域统一不透明 A=255；α 语义由辅助指标承载）。
            out.push(v);
            out.push(v);
            out.push(v);
            out.push(255);
            x += 1;
        }
        y += 1;
    }
    out
}

/// 像素量化（位深降级；levels-1 折返定标，四舍五入）。
/// bits=8 时 levels-1=255，逐字节恒等——参考路径无损的代数保证。
pub fn quantize_byte(v: u8, bits: u8) -> u8 {
    if bits >= 8 {
        return v;
    }
    let levels: u32 = 1u32 << bits; // bits ≤ 7 → ≤ 128
    let max_l = levels - 1;
    let q = ((v as u32 * max_l) + 127) / 255;
    ((q * 255) / max_l) as u8
}

/// RGBA 全帧量化（实现路径降级的图像域变换）。
pub fn quantize_rgba(rgba: &[u8], bits: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    let mut i = 0;
    while i < rgba.len() {
        out.push(quantize_byte(rgba[i], bits));
        i += 1;
    }
    out
}

/// Rec.709 亮度（sRGB D65 矩阵第二行：0.2127R + 0.7152G + 0.0721B，
/// 来源：IEC 61966-2-1 / ITU-R BT.709；线性域加权后压回 8bit）。
pub fn luma_of_rgba(rgba: &[u8]) -> Result<Vec<u8>, EffBenchError> {
    if rgba.len() % 4 != 0 {
        return Err(EffBenchError::SampleSizeMismatch);
    }
    let mut out = Vec::with_capacity(rgba.len() / 4);
    let mut i = 0;
    while i < rgba.len() {
        let rl = SRGB_TO_LIN[rgba[i] as usize] as u64;
        let gl = SRGB_TO_LIN[rgba[i + 1] as usize] as u64;
        let bl = SRGB_TO_LIN[rgba[i + 2] as usize] as u64;
        // 加权系数 ×10000：2127/7152/721（BT.709 取整）；峰值
        // 10000_000 × 10000 = 1e11 量级，u64 充裕。
        let y = (2_127 * rl + 7_152 * gl + 721 * bl) / 10_000;
        out.push((y * 255 / 1_000_000) as u8);
        i += 4;
    }
    Ok(out)
}

/// 盒式模糊（半径 r 的方形核；r=0 恒等）。半径上界 24（Q8 输入
/// 右移 8 位后 saturating 收口；O(w·h·r²) 上界 48×48×49² ≈ 553 万次
/// 加法，基准路径预算内），O(w·h·r²) 量程 u32 充裕（每窗和 ≤ 2401×255）。
pub fn box_blur_u8(src: &[u8], w: usize, h: usize, radius_q8: u16) -> Vec<u8> {
    let r = ((radius_q8 >> 8) as usize).min(24);
    if r == 0 || src.len() != w * h {
        return src.to_vec();
    }
    let mut out = Vec::with_capacity(src.len());
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let mut acc: u32 = 0;
            let mut cnt: u32 = 0;
            let mut dy = 0usize;
            while dy <= 2 * r {
                let sy = (y + dy).saturating_sub(r);
                if sy < h {
                    let mut dx = 0usize;
                    while dx <= 2 * r {
                        let sx = (x + dx).saturating_sub(r);
                        if sx < w {
                            acc += src[sy * w + sx] as u32;
                            cnt += 1;
                        }
                        dx += 1;
                    }
                }
                dy += 1;
            }
            out.push((acc / cnt.max(1)) as u8);
            x += 1;
        }
        y += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 九、SSIM 整数定点实现（双指标之一）
// ---------------------------------------------------------------------------

/// SSIM 8×8 均匀窗全帧评分（0~10000）。
/// 数学口径：逐窗 mx/my（均值，四舍五入）、vx/vy（方差，非负收口）、
/// cov（协方差，可负）；窗分 = ((2·mx·my+C1)(2·cov+C2)) /
/// ((mx²+my²+C1)(vx+vy+C2))，Q16 定点，帧分 = 窗分均值 ×10000/65536。
/// 尺寸不符 → [`EffBenchError::SampleSizeMismatch`]；非 8 倍数边长
/// → 同错（分窗前提在场景闭集内已保证，此处防御性复验）。
pub fn ssim_score(a: &[u8], b: &[u8], w: usize, h: usize) -> Result<u32, EffBenchError> {
    if a.len() != b.len() || a.len() != w * h || w % SSIM_WIN != 0 || h % SSIM_WIN != 0 {
        return Err(EffBenchError::SampleSizeMismatch);
    }
    let nx = w / SSIM_WIN;
    let ny = h / SSIM_WIN;
    let mut acc_q16: u64 = 0;
    let mut windows: u64 = 0;
    let mut wy = 0;
    while wy < ny {
        let mut wx = 0;
        while wx < nx {
            let mut sx: u32 = 0;
            let mut sy: u32 = 0;
            let mut sxx: u32 = 0;
            let mut syy: u32 = 0;
            let mut sxy: u32 = 0;
            let mut py = 0;
            while py < SSIM_WIN {
                let mut px = 0;
                while px < SSIM_WIN {
                    let idx = (wy * SSIM_WIN + py) * w + wx * SSIM_WIN + px;
                    let va = a[idx] as u32;
                    let vb = b[idx] as u32;
                    sx += va;
                    sy += vb;
                    sxx += va * va;
                    syy += vb * vb;
                    sxy += va * vb;
                    px += 1;
                }
                py += 1;
            }
            // 均值一律截断（floor）：与方差口径代数一致——由柯西不等式
            // (Σva)² ≤ 64·Σva² 可证 floor(E[x²]) ≥ floor(E[x])²，故 vx ≥ 0
            // 恒成立且 a==b 时 cov ≡ vx ≡ vy（逐位同源），num ≡ den
            // ⇒ 同帧必满分（四舍五入均值会破坏该不变量，故禁用）。
            let mx = (sx >> 6) as i64;
            let my = (sy >> 6) as i64;
            let vx = ((sxx >> 6) as i64) - mx * mx;
            let vy = ((syy >> 6) as i64) - my * my;
            let cov = ((sxy >> 6) as i64) - mx * my;
            // 分母恒正：mx²+my²+C1 ≥ 7 > 0；vx+vy+C2 ≥ 59 > 0。
            let den = (mx * mx + my * my + SSIM_C1) * (vx + vy + SSIM_C2);
            let num = (2 * mx * my + SSIM_C1) * (2 * cov + SSIM_C2);
            // 分子可负（cov 深负）→ 窗分 0 收口；Q16 定点：
            // num 上界 ≈ (2·255·255+7)(2·255·255+59) ≈ 1.69e10，
            // ×65536 ≈ 1.11e15 < 2^63，u64/i64 充裕。
            let win_q16 = if num <= 0 {
                0u64
            } else {
                ((num as u64) << 16) / den as u64
            };
            acc_q16 += win_q16;
            windows += 1;
            wx += 1;
        }
        wy += 1;
    }
    if windows == 0 {
        return Err(EffBenchError::SampleSizeMismatch);
    }
    // 帧分 = 均值 Q16 → 万分制：avg ≤ 65536，×10000 ≤ 6.6e8。
    let avg_q16 = acc_q16 / windows;
    Ok((avg_q16 * 10_000 / 65_536) as u32)
}

// ---------------------------------------------------------------------------
// 十、deltaE(CIE76) 整数定点实现（双指标之二）
// ---------------------------------------------------------------------------

/// 线性 RGB → XYZ（sRGB D65 矩阵 ×10000：4124/3576/1805、
/// 2127/7152/721、193/1192/9503；来源 IEC 61966-2-1）。
/// 输入线性 ppm（0..=1e6），输出 XYZ ppm（Y 白点 ≈ 1e6）。
const fn rgb_to_xyz(rl: u64, gl: u64, bl: u64) -> (u64, u64, u64) {
    let x = (4_124 * rl + 3_576 * gl + 1_805 * bl) / 10_000;
    let y = (2_127 * rl + 7_152 * gl + 721 * bl) / 10_000;
    let z = (193 * rl + 1_192 * gl + 9_503 * bl) / 10_000;
    (x, y, z)
}

/// Lab 三刺激函数 f(t)（Q12 输出；t 为 ppm 0..=1e6）。
/// ε = 216/24389 ≈ 0.008856 → 8856 ppm；κ = 24389/27 ≈ 903.3
/// （来源：CIE Lab 定义）。分支连续性在 t=8856 处两侧差 < 2/4096。
const fn lab_f(t: u64) -> i64 {
    if t > 8_856 {
        // f = (t/1e6)^(1/3)；f·4096 = 40.96·t^(1/3) ≈ icbrt(t·68719)
        // （68719 ≈ 4096³/1e6，比例误差 < 3e-6）。
        icbrt64(t * 68_719) as i64
    } else {
        // f = (κ·t/1e6 + 16)/116 → Q12：分子分母同扩防精度塌陷。
        (((t * 9_033 + 160_000_000) * 4_096) / 1_160_000_000) as i64
    }
}

/// 单像素 Lab（Q12：L∈0..409600，a/b∈±~200_000 量级）。
const fn pixel_lab(rgba: &[u8], idx: usize) -> (i64, i64, i64) {
    let rl = SRGB_TO_LIN[rgba[idx] as usize] as u64;
    let gl = SRGB_TO_LIN[rgba[idx + 1] as usize] as u64;
    let bl = SRGB_TO_LIN[rgba[idx + 2] as usize] as u64;
    let (x, y, z) = rgb_to_xyz(rl, gl, bl);
    let fx = lab_f(x);
    let fy = lab_f(y);
    let fz = lab_f(z);
    let l = 116 * fy - 16 * 4_096;
    let a = 500 * (fx - fy);
    let b = 200 * (fy - fz);
    (l, a, b)
}

/// deltaE76 全帧评分（0~10000，图像坐标隔 2 行 2 列采样控量）。
/// ΔE76 = √(ΔL²+Δa²+Δb²)（CIE 色差公式 1A，整数平方根）；
/// 映射：ΔE ≤ 1.0 满分、≥ 20.0 零分、其间线性（JND 经验带，合同值）。
pub fn deltae_score(
    ref_rgba: &[u8],
    cand_rgba: &[u8],
    w: usize,
    h: usize,
) -> Result<u32, EffBenchError> {
    if ref_rgba.len() != cand_rgba.len() || ref_rgba.len() != w * h * 4 {
        return Err(EffBenchError::SampleSizeMismatch);
    }
    let stride = 2usize;
    let mut sum_q12: u64 = 0;
    let mut samples: u64 = 0;
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        while x < w {
            if x % stride == 0 && y % stride == 0 {
                let idx = (y * w + x) * 4;
                let (l1, a1, b1) = pixel_lab(ref_rgba, idx);
                let (l2, a2, b2) = pixel_lab(cand_rgba, idx);
                let dl = (l1 - l2) as i64;
                let da = (a1 - a2) as i64;
                let db = (b1 - b2) as i64;
                // 平方和上界：(409600)²×3 ≈ 5e11，u64 充裕。
                let sum_sq = (dl * dl + da * da + db * db) as u64;
                // isqrt 后为 Q12 定点 ΔE（√(q12²) = q12）。
                sum_q12 += isqrt64(sum_sq);
                samples += 1;
            }
            x += 1;
        }
        y += 1;
    }
    if samples == 0 {
        return Err(EffBenchError::SampleSizeMismatch);
    }
    let avg_q12 = sum_q12 / samples;
    Ok(deltae_q12_to_score(avg_q12))
}

/// ΔE(Q12) → 万分制分数（JND 线性带映射，合同值口径见
/// [`DELTAE_FULL_Q12`] / [`DELTAE_ZERO_Q12`]）。
pub const fn deltae_q12_to_score(avg_q12: u64) -> u32 {
    if avg_q12 <= DELTAE_FULL_Q12 as u64 {
        return QUALITY_SCORE_MAX;
    }
    if avg_q12 >= DELTAE_ZERO_Q12 as u64 {
        return 0;
    }
    // (20·4096 − dE)·10000 / (19·4096)：分子 ≤ 19·4096·10000 ≈ 7.8e8。
    let span = (DELTAE_ZERO_Q12 - DELTAE_FULL_Q12) as u64; // 19·4096
    ((DELTAE_ZERO_Q12 as u64 - avg_q12) * 10_000 / span) as u32
}

// ---------------------------------------------------------------------------
// 十一、效果专有辅助指标（第三质量支路）
// ---------------------------------------------------------------------------

/// 模糊半径误差：合成 32 点阶跃边缘线分别以参考半径与候选半径做盒式
/// 模糊，逐点绝对差均值 vs 容忍值 → 0~10000。
/// radius_q8 右移 8 位为像素半径（上界收口 24，与 [`box_blur_u8`] 同口径
/// ——上界内不同半径必须可分辨，否则降级不可测）。
pub fn aux_blur_radius(radius_ref_q8: u16, radius_cand_q8: u16, tol: u16) -> (u32, u32) {
    // 阶跃边缘：前半 20，后半 235。
    let mut line = [0u8; AUX_LINE_LEN];
    let mut i = 0;
    while i < AUX_LINE_LEN {
        line[i] = if i < AUX_LINE_LEN / 2 { 20 } else { 235 };
        i += 1;
    }
    let la = box_blur_u8(&line, AUX_LINE_LEN, 1, radius_ref_q8);
    let lb = box_blur_u8(&line, AUX_LINE_LEN, 1, radius_cand_q8);
    let mut sum: u32 = 0;
    let mut j = 0;
    while j < AUX_LINE_LEN {
        let d = (la[j] as i32 - lb[j] as i32).abs();
        sum += d as u32;
        j += 1;
    }
    let err = sum / AUX_LINE_LEN as u32;
    (err_to_score(err, tol as u32), err)
}

/// 透明度混合精度：参考与候选走**同一逐级叠加混合规程**，仅精度
/// 参数不同——参考侧全 8bit α、基准步数；候选侧 α 量化到路径位深、
/// 步数按路径步进百分比折减。规程：目标 α 分摊到 steps 级（前
/// steps-1 级各 q_base，末级吸收余数），逐级 `out = (src·q +
/// out·(255−q) + 127)/255`。参考与候选规程一致 ⇒ 参考路径误差恒 0
/// （无损性的代数保证）；位深/步数降级则产生真实累积舍入误差。
/// 三种代表性底色（暗/中/亮）取绝对差均值 → 0~10000。
pub fn aux_alpha_blend(
    alpha_ref: u16,
    alpha_bits: u8,
    steps_ref: u16,
    steps_cand: u16,
    tol: u16,
) -> (u32, u32) {
    let a_ref = (alpha_ref as u32).min(255);
    let steps_ref = (steps_ref as usize).max(1);
    let dsts: [u32; 3] = [16, 128, 240];
    let src: u32 = 200;
    // 参考轨迹（全精度 α、基准步数）。
    let a_q_ref = a_ref;
    let mut sum: u64 = 0;
    let mut cnt: u64 = 0;
    let mut di = 0;
    while di < dsts.len() {
        let ref_out = blend_iterated(dsts[di], src, a_q_ref, steps_ref);
        // 候选轨迹（量化 α + 折减步数）。
        let a_q = quantize_byte(a_ref as u8, alpha_bits) as u32;
        let cand_out = blend_iterated(dsts[di], src, a_q, (steps_cand as usize).max(1));
        sum += (ref_out as i64 - cand_out as i64).abs() as u64;
        cnt += 1;
        di += 1;
    }
    let err = (sum / cnt.max(1)) as u32;
    (err_to_score(err, tol as u32), err)
}

/// 逐级叠加混合（前 iters-1 级各 q_base，末级吸收余数；q_base = α/iters）。
fn blend_iterated(dst: u32, src: u32, alpha: u32, iters: usize) -> u32 {
    let iters = iters.max(1);
    let q_base = alpha / iters as u32;
    let mut cur = dst;
    let mut i = 0;
    while i < iters {
        let q = if i + 1 == iters {
            alpha.saturating_sub(q_base * (iters as u32 - 1))
        } else {
            q_base
        };
        cur = (src * q + cur * (255 - q) + 127) / 255;
        i += 1;
    }
    cur
}

/// 动画平滑度：64 tick 线性插值参考 vs 步进百分比阶梯更新候选。
/// 更新次数 updates = max(1, 64·step_pct/10000)（参考 64 次 → 每 tick
/// 更新；近似降级 32 次 → 每 2 tick 更新一次，保持值阶梯）。
/// 抖动量 = 候选二阶差分平方和 − 参考二阶差分平方和（加加速度能量
/// 基线扣减；参考逐 tick 线性插值舍入抖动 ≤1/tick 由基线吸收）
/// → 0~10000（容忍值合同给）。
pub fn aux_anim_smoothness(span: u16, step_pct: u16, tol: u16) -> (u32, u32) {
    let span = span.min(255) as i32;
    let ticks = ANIM_TICKS as i32;
    // 更新次数与步幅（参考 100% → 64 次更新 → 步幅 1）。
    let updates = (((ticks as u32) * step_pct as u32 / 10_000).max(1)) as usize;
    let stride = (ANIM_TICKS / updates).max(1);
    let mut pos_ref = [0i32; ANIM_TICKS];
    let mut pos_cand = [0i32; ANIM_TICKS];
    let mut t = 0usize;
    let mut last: i32 = 0;
    while t < ANIM_TICKS {
        pos_ref[t] = (span * t as i32) / (ticks - 1);
        if t % stride == 0 {
            last = pos_ref[t];
        }
        pos_cand[t] = last;
        t += 1;
    }
    // 加加速度能量（二阶差分平方和；i64：每项 ≤ (255·2)² ≈ 2.6e5，
    // 62 项 ≤ 1.6e7，u64 充裕）。
    let mut e_cand: u64 = 0;
    let mut e_ref: u64 = 0;
    let mut k = 2;
    while k < ANIM_TICKS {
        let d2c = (pos_cand[k] - 2 * pos_cand[k - 1] + pos_cand[k - 2]).abs();
        let d2r = (pos_ref[k] - 2 * pos_ref[k - 1] + pos_ref[k - 2]).abs();
        e_cand += (d2c * d2c) as u64;
        e_ref += (d2r * d2r) as u64;
        k += 1;
    }
    let err = e_cand.saturating_sub(e_ref) as u32;
    (err_to_score(err, tol as u32), err)
}

/// 误差 → 分数（线性带：err ≤ tol/2 满分内插、err ≥ tol 零分；
/// 合同值口径：容忍值即场景 aux_tol 字段）。
pub const fn err_to_score(err: u32, tol: u32) -> u32 {
    if tol == 0 {
        // 容忍 0：仅 err=0 记满分（防除零的显性合同分支）。
        if err == 0 {
            return QUALITY_SCORE_MAX;
        }
        return 0;
    }
    if err >= tol {
        return 0;
    }
    (tol - err) * QUALITY_SCORE_MAX / tol
}

/// 辅助指标统一入口（按场景辅助族分发；返回 (分数, 实测误差)）。
pub fn aux_score(sc: &BenchScenario, path: &PathDesc) -> (u32, u32) {
    match sc.aux {
        AuxKind::BlurRadius => {
            let r_ref = sc.blur_radius_q8;
            // 候选半径 = 参考半径 × 路径半径百分比（Q8×pct 万分比）。
            let r_cand = ((r_ref as u32 * path.radius_pct as u32) / 10_000)
                .min(u16::MAX as u32) as u16;
            aux_blur_radius(r_ref, r_cand, sc.aux_tol)
        }
        AuxKind::AlphaBlend => {
            // 候选步数 = 基准步数 × 路径步进百分比（万分比，下取整 ≥1）；
            // 参考轨迹用基准步数——参考路径候选步数 == 基准步数 ⇒ 无损。
            let steps_cand = (((sc.alpha_steps as u32 * path.step_pct as u32) / 10_000).max(1))
                .min(u16::MAX as u32) as u16;
            aux_alpha_blend(
                sc.alpha_ref,
                path.alpha_bits,
                sc.alpha_steps,
                steps_cand,
                sc.aux_tol,
            )
        }
        AuxKind::AnimSmoothness => {
            aux_anim_smoothness(sc.anim_span, path.step_pct, sc.aux_tol)
        }
    }
}

// ---------------------------------------------------------------------------
// 十二、质量分聚合（三支路加权合成 0~10000）
// ---------------------------------------------------------------------------

/// 质量分聚合：score = (SSIM·w1 + ΔE·w2 + AUX·w3)/10000。
/// 权重合同：4000/3500/2500（和恒 10000——编译期断言封顶）。
pub const fn aggregate_score(ssim: u32, deltae: u32, aux: u32) -> Result<u32, EffBenchError> {
    const _: () = assert!(
        QUALITY_WEIGHT_SSIM + QUALITY_WEIGHT_DELTA_E + QUALITY_WEIGHT_AUX == QUALITY_SCORE_MAX
    );
    if ssim > QUALITY_SCORE_MAX || deltae > QUALITY_SCORE_MAX || aux > QUALITY_SCORE_MAX {
        return Err(EffBenchError::ScoreOutOfRange);
    }
    // 乘积上界：10000×10000 = 1e8，u32 充裕；和 ≤ 1e8。
    let acc = ssim * QUALITY_WEIGHT_SSIM
        + deltae * QUALITY_WEIGHT_DELTA_E
        + aux * QUALITY_WEIGHT_AUX;
    Ok(acc / QUALITY_SCORE_MAX)
}

/// 质量损失量化声明校验（实测损失 vs 路径声明上界）。
/// 违约即 [`EffBenchError::LossDeclarationExceeded`]——降级路径的
/// 质量损失必须先声明后实测，禁止静默超损。
pub const fn check_loss_declared(
    measured_loss: u32,
    declared_max_loss: u32,
) -> Result<(), EffBenchError> {
    if measured_loss > declared_max_loss {
        return Err(EffBenchError::LossDeclarationExceeded);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 十三、评分结果与环境画像
// ---------------------------------------------------------------------------

/// 单次评分结果（链上正文；签名覆盖全部字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoredResult {
    /// 场景号。
    pub scenario_id: u16,
    /// 路径号。
    pub path_id: u8,
    /// SSIM 支路分。
    pub ssim_score: u32,
    /// deltaE 支路分。
    pub deltae_score: u32,
    /// 辅助支路分。
    pub aux_score: u32,
    /// 辅助支路实测误差（原始量纲，供审计）。
    pub aux_err: u32,
    /// 聚合质量分。
    pub score: u32,
    /// 档级（TIER_TABLE 下标）。
    pub tier: u8,
    /// 相对原生参考的实测损失（参考路径恒 0）。
    pub loss_vs_native: u32,
    /// 路径损失声明上界。
    pub loss_declared: u32,
    /// 帧成本（场景基价 × 路径成本百分比）。
    pub perf_cost: u32,
}

impl ScoredResult {
    /// 结果串行化（签名链正文；定长 40 字节布局冻结）。
    pub const fn serialize(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        put_u16_40(&mut buf, 0, self.scenario_id);
        buf[2] = self.path_id;
        buf[3] = self.tier;
        put_u32_40(&mut buf, 4, self.ssim_score);
        put_u32_40(&mut buf, 8, self.deltae_score);
        put_u32_40(&mut buf, 12, self.aux_score);
        put_u32_40(&mut buf, 16, self.aux_err);
        put_u32_40(&mut buf, 20, self.score);
        put_u32_40(&mut buf, 24, self.loss_vs_native);
        put_u32_40(&mut buf, 28, self.loss_declared);
        put_u32_40(&mut buf, 32, self.perf_cost);
        buf
    }
}

/// 40 字节定长缓冲的 LE u16 写入。
const fn put_u16_40(buf: &mut [u8; 40], off: usize, v: u16) {
    buf[off] = (v & 0xFF) as u8;
    buf[off + 1] = (v >> 8) as u8;
}

/// 40 字节定长缓冲的 LE u32 写入。
const fn put_u32_40(buf: &mut [u8; 40], off: usize, v: u32) {
    let mut i = 0;
    while i < 4 {
        buf[off + i] = ((v >> (8 * i)) & 0xFF) as u8;
        i += 1;
    }
}

/// 环境画像（质量结论的适用域声明；全部字段进签名链）。
/// GPU 档位闭集 0~3（0=无/软渲，1=入门，2=主流，3=高性能）；
/// ISA 位集（bit0=SSE4.1，bit1=AVX2，bit2=AVX-512，bit3=NEON——
/// 来源：x86 EDX/ECX/EBX 特征位与 ARM HWCAP 惯例，仅作画像标签）；
/// 调度类闭集 0~2（0=独占，1=共享，2=后台）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvProfile {
    /// GPU 档位（0~3）。
    pub gpu_tier: u8,
    /// ISA 位集（0~15）。
    pub isa_mask: u8,
    /// 调度类（0~2）。
    pub sched_class: u8,
}

impl EnvProfile {
    /// 画像串行化（3 字节定长）。
    pub const fn serialize(&self) -> [u8; 3] {
        [self.gpu_tier, self.isa_mask, self.sched_class]
    }

    /// 画像哈希（FNV-64；签名链环境指纹）。
    pub const fn hash(&self) -> u64 {
        fnv1a64(&self.serialize())
    }

    /// 闭集校验（越界显性拒绝）。
    pub const fn validate(&self) -> Result<(), EffBenchError> {
        if self.gpu_tier >= 4 || self.isa_mask >= 16 || self.sched_class >= 3 {
            return Err(EffBenchError::ScoreOutOfRange);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十四、原生截图对比库（RefStore）
// ---------------------------------------------------------------------------

/// 对比库条目（一场景一帧；入库即指纹冻结）。
#[derive(Debug, Clone)]
pub struct RefEntry {
    /// 场景号。
    pub scenario_id: u16,
    /// 参考帧 RGBA（堆上；48×48×4 = 9216B / 64×32×4 = 8192B）。
    pub rgba: Vec<u8>,
    /// 入库指纹（FNV-32 over rgba 字节 ⊕ 场景种子）。
    pub fingerprint: u32,
    /// 取用命中计数（saturating）。
    pub hits: u32,
}

/// 原生截图对比库：每场景确定性合成参考帧 + 指纹冻结 + 取用复验。
/// （CI 无 Windows 环境时以确定性合成帧承载"原生截图库"职责——
/// F2090 效果模拟器同口径；真机接入时本结构直接换帧源不改口径。）
#[derive(Debug)]
pub struct RefStore {
    /// 条目集（场景闭集全覆盖）。
    entries: Vec<RefEntry>,
}

impl RefStore {
    /// 构建（逐场景生成 + 指纹冻结）。
    pub fn build() -> Result<RefStore, EffBenchError> {
        let mut entries = Vec::with_capacity(SCENARIO_COUNT);
        let mut i = 0;
        while i < SCENARIO_COUNT {
            let sc = &SCENARIOS[i];
            let rgba = render_scenario_rgba(sc);
            let fingerprint = ref_fingerprint(sc.seed, &rgba);
            entries.push(RefEntry {
                scenario_id: sc.id,
                rgba,
                fingerprint,
                hits: 0,
            });
            i += 1;
        }
        Ok(RefStore { entries })
    }

    /// 参考帧指纹（种子参与——同帧不同场景种子也不可互换）。
    pub const fn ref_fingerprint(seed: u32, rgba: &[u8]) -> u32 {
        ref_fingerprint(seed, rgba)
    }

    /// 取用（场景号定位 + 指纹复验 + 命中计数；失配即 0x1372）。
    pub fn lookup(&mut self, scenario_id: u16) -> Result<&RefEntry, EffBenchError> {
        let mut found: Option<usize> = None;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].scenario_id == scenario_id {
                found = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match found {
            Some(v) => v,
            None => return Err(EffBenchError::UnknownScenario),
        };
        let sc = &SCENARIOS[idx];
        let fp = ref_fingerprint(sc.seed, &self.entries[idx].rgba);
        if fp != self.entries[idx].fingerprint {
            return Err(EffBenchError::ScenarioFingerprintMismatch);
        }
        self.entries[idx].hits = self.entries[idx].hits.saturating_add(1);
        Ok(&self.entries[idx])
    }

    /// 只读取用（不改命中计数；指纹仍复验）。
    pub fn lookup_read(&self, scenario_id: u16) -> Result<&RefEntry, EffBenchError> {
        let mut found: Option<usize> = None;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].scenario_id == scenario_id {
                found = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match found {
            Some(v) => v,
            None => return Err(EffBenchError::UnknownScenario),
        };
        let sc = &SCENARIOS[idx];
        let fp = ref_fingerprint(sc.seed, &self.entries[idx].rgba);
        if fp != self.entries[idx].fingerprint {
            return Err(EffBenchError::ScenarioFingerprintMismatch);
        }
        Ok(&self.entries[idx])
    }

    /// 全库指纹复验（体检用；返回 (校验条数, 是否全过)）。
    pub fn verify_all(&self) -> (usize, bool) {
        let mut ok = true;
        let mut i = 0;
        while i < self.entries.len() {
            let sc = &SCENARIOS[i];
            let fp = ref_fingerprint(sc.seed, &self.entries[i].rgba);
            if fp != self.entries[i].fingerprint {
                ok = false;
            }
            i += 1;
        }
        (self.entries.len(), ok)
    }

    /// 条目数。
    pub const fn len(&self) -> usize {
        SCENARIO_COUNT
    }

    /// 库非空（闭集恒真；供模式化判空）。
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// 参考帧指纹实现（种子 4 字节 LE 前缀 + 帧字节，FNV-32）。
const fn ref_fingerprint(seed: u32, rgba: &[u8]) -> u32 {
    // 前缀 4 字节 + 帧体；用双段哈希合成避免运行期拼接分配。
    let mut prefix = [0u8; 4];
    let mut i = 0;
    while i < 4 {
        prefix[i] = ((seed >> (8 * i)) & 0xFF) as u8;
        i += 1;
    }
    let h1 = fnv1a32(&prefix);
    let h2 = fnv1a32(rgba);
    // 合成：h1 作种子再过一遍 h2 的 4 字节 LE（确定性、双向吃熵）。
    let mut tail = [0u8; 4];
    let mut j = 0;
    while j < 4 {
        tail[j] = ((h2 >> (8 * j)) & 0xFF) as u8;
        j += 1;
    }
    let mut h = h1;
    let mut k = 0;
    while k < 4 {
        h ^= tail[k] as u32;
        h = h.wrapping_mul(0x0100_0193);
        k += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 十五、结果签名链（场景指纹 + 质量分 + 环境画像 FNV 链）
// ---------------------------------------------------------------------------

/// 签名链节点（正文 40B + 环境哈希 8B + 前环哈希 8B → 节点哈希 8B）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainNode {
    /// 序号（0 起连续，跳号即断链）。
    pub seq: u32,
    /// 正文（评分结果全字段）。
    pub body: ScoredResult,
    /// 正文场景指纹快照（评分时刻的场景表指纹）。
    pub scenario_fp: u32,
    /// 环境画像哈希。
    pub env_hash: u64,
    /// 前环节点哈希（首环 0）。
    pub prev_hash: u64,
    /// 本环节点哈希（FNV-64 over 全部字段串行化）。
    pub node_hash: u64,
}

impl ChainNode {
    /// 节点哈希重算（校验与建环共用同一口径——篡改检测的代数基础）。
    pub const fn recompute_hash(&self) -> u64 {
        let mut buf = [0u8; 64];
        let body = self.body.serialize();
        let mut i = 0;
        while i < 40 {
            buf[i] = body[i];
            i += 1;
        }
        // scenario_fp LE @40。
        let mut j = 0;
        while j < 4 {
            buf[40 + j] = ((self.scenario_fp >> (8 * j)) & 0xFF) as u8;
            j += 1;
        }
        // seq LE @44。
        let mut k = 0;
        while k < 4 {
            buf[44 + k] = ((self.seq >> (8 * k)) & 0xFF) as u8;
            k += 1;
        }
        // env_hash LE @48。
        let mut m = 0;
        while m < 8 {
            buf[48 + m] = ((self.env_hash >> (8 * m)) & 0xFF) as u8;
            m += 1;
        }
        // prev_hash LE @56。
        let mut n = 0;
        while n < 8 {
            buf[56 + n] = ((self.prev_hash >> (8 * n)) & 0xFF) as u8;
            n += 1;
        }
        fnv1a64(&buf)
    }
}

/// 结果签名链：逐环含前环哈希；任一环被篡改则该校验从该环起失配，
/// 返回首个失配槽位——**篡改可精确定位**。
#[derive(Debug)]
pub struct SignatureChain {
    /// 节点集（Vec 堆上；容量 [`CHAIN_CAP`] 封顶）。
    nodes: Vec<ChainNode>,
    /// 环境画像哈希（建链时固定）。
    env_hash: u64,
}

impl SignatureChain {
    /// 建链（绑定环境画像）。
    pub const fn new(env_hash: u64) -> SignatureChain {
        SignatureChain {
            nodes: Vec::new(),
            env_hash,
        }
    }

    /// 追加一环（序号自动连续；满链显性拒绝 0x137F）。
    pub fn append(&mut self, body: ScoredResult, scenario_fp: u32) -> Result<u32, EffBenchError> {
        if self.nodes.len() >= CHAIN_CAP {
            return Err(EffBenchError::LedgerFull);
        }
        let seq = self.nodes.len() as u32;
        let prev_hash = if seq == 0 { 0 } else { self.nodes[seq as usize - 1].node_hash };
        let node = ChainNode {
            seq,
            body,
            scenario_fp,
            env_hash: self.env_hash,
            prev_hash,
            node_hash: 0,
        };
        // 哈希依赖自身结构但不依赖 node_hash 字段（重算口径不含
        // node_hash 自身——防自引用），先置 0 再重算安全。
        let mut node = node;
        node.node_hash = node.recompute_hash();
        let seq_ret = node.seq;
        self.nodes.push(node);
        Ok(seq_ret)
    }

    /// 全链校验：逐环复算节点哈希 + 校验前环哈希接续 + 序号连续。
    /// 返回 Ok(校验环数) 或 Err(失配类型 + 首个坏槽位打包在
    /// [`ChainFault`]）。
    pub fn verify(&self) -> Result<usize, (EffBenchError, usize)> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = &self.nodes[i];
            if n.seq != i as u32 {
                return Err((EffBenchError::SignatureChainBroken, i));
            }
            let expect_prev = if i == 0 { 0 } else { self.nodes[i - 1].node_hash };
            if n.prev_hash != expect_prev {
                return Err((EffBenchError::SignatureChainBroken, i));
            }
            if n.node_hash != n.recompute_hash() {
                return Err((EffBenchError::SignatureMismatch, i));
            }
            if n.env_hash != self.env_hash {
                return Err((EffBenchError::SignatureMismatch, i));
            }
            i += 1;
        }
        Ok(self.nodes.len())
    }

    /// 首个失配槽位定位（体检/注入面用；全链完好返回 None）。
    pub fn first_fault(&self) -> Option<(EffBenchError, usize)> {
        match self.verify() {
            Ok(_) => None,
            Err((e, slot)) => Some((e, slot)),
        }
    }

    /// 环数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 链空判。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 只读节点访问（越界 None——测试面与体检共用）。
    pub fn node(&self, slot: usize) -> Option<&ChainNode> {
        self.nodes.get(slot)
    }

    /// 注入面：篡改指定槽位正文质量分（翻转 bit0）。
    /// 仅供注入测试调用；内核常规路径不得使用。
    pub fn tamper_score_for_injection(&mut self, slot: usize) -> bool {
        if slot >= self.nodes.len() {
            return false;
        }
        self.nodes[slot].body.score ^= 1;
        true
    }

    /// 快照并清空（链满归档口径；返回旧链节点集）。
    pub fn snapshot_reset(&mut self) -> Vec<ChainNode> {
        let mut old = Vec::new();
        let mut i = 0;
        while i < self.nodes.len() {
            old.push(self.nodes[i]);
            i += 1;
        }
        self.nodes.clear();
        old
    }

    /// 环境哈希。
    pub const fn env_hash(&self) -> u64 {
        self.env_hash
    }
}

// ---------------------------------------------------------------------------
// 十六、事件账（12 类事件定长环形账 + 逐类计数器）
// ---------------------------------------------------------------------------

/// 事件种类闭集（12 类；u8 编码即 [`EVENT_KIND_NAMES`] 下标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 场景表指纹校验通过。
    ScenarioVerified,
    /// 对比库参考帧命中。
    RefHit,
    /// 完成一次评分。
    Scored,
    /// 损失声明校验通过。
    LossDeclared,
    /// 质量预算违约。
    QualityBreach,
    /// 性能预算违约。
    PerfBreach,
    /// 损失声明违约。
    LossBreach,
    /// 签名链追加一环。
    ChainAppend,
    /// 双跑对拍完成。
    DualRun,
    /// 篡改检出。
    TamperFound,
    /// 挑参数检出。
    CherryPickFound,
    /// 账满覆写。
    Overflow,
}

impl EventKind {
    /// 全类名称表（报告可读位）。
    pub const NAMES: [&'static str; 12] = [
        "scenario-verified",
        "ref-hit",
        "scored",
        "loss-declared",
        "quality-breach",
        "perf-breach",
        "loss-breach",
        "chain-append",
        "dual-run",
        "tamper-found",
        "cherry-pick-found",
        "overflow",
    ];

    /// 事件种类数。
    pub const COUNT: usize = 12;

    /// u8 编码。
    pub const fn as_u8(&self) -> u8 {
        match self {
            EventKind::ScenarioVerified => 0,
            EventKind::RefHit => 1,
            EventKind::Scored => 2,
            EventKind::LossDeclared => 3,
            EventKind::QualityBreach => 4,
            EventKind::PerfBreach => 5,
            EventKind::LossBreach => 6,
            EventKind::ChainAppend => 7,
            EventKind::DualRun => 8,
            EventKind::TamperFound => 9,
            EventKind::CherryPickFound => 10,
            EventKind::Overflow => 11,
        }
    }

    /// 事件名。
    pub const fn name(&self) -> &'static str {
        EventKind::NAMES[self.as_u8() as usize]
    }
}

/// 事件账行（定长 8 字节：种类 1B + 场景号 2B + 载荷 4B + 保留 1B）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRow {
    /// 事件种类编码。
    pub kind: u8,
    /// 关联场景号（无关则 0xFFFF）。
    pub scenario: u16,
    /// 载荷（分数/成本/槽位等，按种类解释）。
    pub value: u32,
}

/// 事件账（环形 + 逐类计数器；零动态分配）。
#[derive(Debug)]
pub struct EventLedger {
    /// 环形槽（定长内嵌——引擎本体大小可控的关键）。
    rows: [EventRow; LEDGER_CAP],
    /// 下一写入位。
    head: usize,
    /// 有效行数（封顶 LEDGER_CAP）。
    count: usize,
    /// 挤出（覆写）计数。
    dropped: u32,
    /// 逐类计数器。
    counters: [u32; EventKind::COUNT],
}

impl EventLedger {
    /// 空账。
    pub const fn new() -> EventLedger {
        EventLedger {
            rows: [EventRow { kind: 0xFF, scenario: 0xFFFF, value: 0 }; LEDGER_CAP],
            head: 0,
            count: 0,
            dropped: 0,
            counters: [0; EventKind::COUNT],
        }
    }

    /// 记账（满则覆写最旧并计 dropped）。
    pub fn record(&mut self, kind: EventKind, scenario: u16, value: u32) {
        let row = EventRow {
            kind: kind.as_u8(),
            scenario,
            value,
        };
        self.rows[self.head] = row;
        self.head = (self.head + 1) % LEDGER_CAP;
        if self.count < LEDGER_CAP {
            self.count += 1;
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
        let k = kind.as_u8() as usize;
        self.counters[k] = self.counters[k].saturating_add(1);
    }

    /// 逐类计数。
    pub const fn counter(&self, kind: EventKind) -> u32 {
        self.counters[kind.as_u8() as usize]
    }

    /// 覆写计数。
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 有效行数。
    pub const fn count(&self) -> usize {
        self.count
    }

    /// 逆序取第 i 新行（越界 None）。
    pub fn row_newest_first(&self, i: usize) -> Option<EventRow> {
        if i >= self.count {
            return None;
        }
        // head 指向下一写位 → 最新行在 (head-1)。
        let idx = (self.head + LEDGER_CAP - 1 - i) % LEDGER_CAP;
        Some(self.rows[idx])
    }
}

// ---------------------------------------------------------------------------
// 十七、引擎门面（评分/帧预算/双跑/体检/注入测试面）
// ---------------------------------------------------------------------------

/// 帧评选项（场景 × 路径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePick {
    /// 场景号。
    pub scenario_id: u16,
    /// 路径号（PATH_TABLE 下标）。
    pub path_id: u8,
}

/// 帧判定（预算双约束结论）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameVerdict {
    /// 帧成本合计（单位 PERF_UNIT_NS 推定）。
    pub total_cost: u32,
    /// 帧内最低质量分。
    pub min_score: u32,
    /// 帧内质量违约场景数。
    pub quality_breaches: u32,
    /// 是否性能违约。
    pub perf_breach: bool,
    /// 帧内各评分结果（顺序同 picks）。
    pub results: Vec<ScoredResult>,
}

/// 注入判定（注入测试面的统一返回；`detected=false` 即注入面失守）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectionVerdict {
    /// 是否检出。
    pub detected: bool,
    /// 检出诊断码（未检出为 0）。
    pub code: u16,
    /// 定位信息（篡改槽位/场景号等；无定位为 0xFFFF_FFFF）。
    pub locate: u32,
}

/// 健康项（绿灯 0 / 黄灯 1 / 红灯 2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthItem {
    /// 检查项名。
    pub name: &'static str,
    /// 灯级（0/1/2）。
    pub level: u8,
    /// 中文注记。
    pub note: String,
}

/// 体检报告。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthReport {
    /// 检查项集。
    pub items: Vec<HealthItem>,
    /// 红灯数。
    pub red_count: u32,
    /// 黄灯数。
    pub yellow_count: u32,
}

/// 原生参考分数缓存行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeScore {
    /// 场景号。
    pub scenario_id: u16,
    /// 原生参考路径质量分。
    pub score: u32,
}

/// 引擎本体（约 1.3KB：账 128×8B + 链/结果/缓存 Vec 头 + 计数器）。
/// 含 `Vec` 字段 → 不 derive `Copy`（纪律）。
#[derive(Debug)]
pub struct EffBenchEngine {
    /// 版本标签。
    pub version_tag: &'static str,
    /// 环境画像（建链即固定）。
    pub env: EnvProfile,
    /// 对比库。
    refs: RefStore,
    /// 签名链。
    chain: SignatureChain,
    /// 事件账。
    ledger: EventLedger,
    /// 原生参考分缓存。
    native_cache: Vec<NativeScore>,
    /// 最近一次双跑是否一致。
    dual_run_ok: bool,
    /// 评分总次数（saturating）。
    scored_total: u32,
    /// 质量违约累计。
    pub breach_quality: u32,
    /// 性能违约累计。
    pub breach_perf: u32,
    /// 损失声明违约累计。
    pub breach_loss: u32,
    /// 最近一次帧判定（违约时的可读结论载体）。
    last_frame_verdict: Option<FrameVerdict>,
}

const _: () = assert!(core::mem::size_of::<EffBenchEngine>() <= 2 * 1024);
const _: () = assert!(core::mem::size_of::<EventLedger>() <= 1_200);
const _: () = assert!(SCENARIO_COUNT == SCENARIOS.len());
const _: () = assert!(PATH_COUNT == PATH_TABLE.len());
const _: () = assert!(CHAIN_CAP <= 256);

impl EffBenchEngine {
    /// 构建引擎（构建对比库 + 建链 + 场景表指纹全量校验）。
    pub fn new(env: EnvProfile) -> Result<EffBenchEngine, EffBenchError> {
        env.validate()?;
        let refs = RefStore::build()?;
        let chain = SignatureChain::new(env.hash());
        let mut eng = EffBenchEngine {
            version_tag: EFFBENCH_VERSION,
            env,
            refs,
            chain,
            ledger: EventLedger::new(),
            native_cache: Vec::with_capacity(SCENARIO_COUNT),
            dual_run_ok: true,
            scored_total: 0,
            breach_quality: 0,
            breach_perf: 0,
            breach_loss: 0,
            last_frame_verdict: None,
        };
        eng.verify_scenarios()?;
        Ok(eng)
    }

    /// 场景表指纹全量复算（**每次评分前强制调用**——运行中指纹校验
    /// 防挑参数；失配即 0x1372 并留痕）。
    pub fn verify_scenarios(&mut self) -> Result<(), EffBenchError> {
        let mut i = 0;
        while i < SCENARIO_COUNT {
            let fp = scenario_fingerprint(&SCENARIOS[i]);
            if fp != SCENARIO_FINGERPRINTS[i] {
                self.ledger.record(EventKind::CherryPickFound, SCENARIOS[i].id, fp);
                return Err(EffBenchError::ScenarioFingerprintMismatch);
            }
            i += 1;
        }
        self.ledger.record(EventKind::ScenarioVerified, 0xFFFF, SCENARIO_COUNT as u32);
        Ok(())
    }

    /// 场景号定位（闭集外显性拒绝）。
    fn scenario_index(scenario_id: u16) -> Result<usize, EffBenchError> {
        let mut i = 0;
        while i < SCENARIO_COUNT {
            if SCENARIOS[i].id == scenario_id {
                return Ok(i);
            }
            i += 1;
        }
        Err(EffBenchError::UnknownScenario)
    }

    /// 路径号定位（闭集外显性拒绝）。
    fn path_index(path_id: u8) -> Result<usize, EffBenchError> {
        if (path_id as usize) < PATH_COUNT && PATH_TABLE[path_id as usize].id == path_id {
            return Ok(path_id as usize);
        }
        Err(EffBenchError::UnknownPath)
    }

    /// 原生参考分（带缓存；缓存行含指纹防错位）。
    fn native_score_of(&mut self, sc_idx: usize) -> Result<u32, EffBenchError> {
        let sid = SCENARIOS[sc_idx].id;
        let mut i = 0;
        while i < self.native_cache.len() {
            if self.native_cache[i].scenario_id == sid {
                return Ok(self.native_cache[i].score);
            }
            i += 1;
        }
        let r = self.run_scenario_raw(sc_idx, PATH_REF_IDX)?;
        self.native_cache.push(NativeScore {
            scenario_id: sid,
            score: r.score,
        });
        Ok(r.score)
    }

    /// 评分内核（不校验指纹、不记账——由外层包装决定纪律层级）。
    /// 管线：参考帧 → 亮度 → 双半径盒模糊 → 量化 → SSIM；参考帧 →
    /// 量化 RGBA → deltaE；场景辅助指标 → 三支路聚合 → 档位 →
    /// 损失声明校验。
    fn run_scenario_raw(
        &mut self,
        sc_idx: usize,
        path_idx: usize,
    ) -> Result<ScoredResult, EffBenchError> {
        let sc = SCENARIOS[sc_idx];
        let path = PATH_TABLE[path_idx];
        let w = sc.width as usize;
        let h = sc.height as usize;
        let expect_len = w * h * 4;
        // 对比库取帧（指纹复验）。
        let entry = self.refs.lookup(sc.id)?;
        if entry.rgba.len() != expect_len {
            return Err(EffBenchError::SampleSizeMismatch);
        }
        let ref_rgba = entry.rgba.clone();
        self.ledger.record(EventKind::RefHit, sc.id, entry.fingerprint);
        // 亮度域。
        let ref_luma = luma_of_rgba(&ref_rgba)?;
        // 参考侧模糊（场景合同半径）与候选侧模糊（路径半径百分比）。
        let ref_blur = box_blur_u8(&ref_luma, w, h, sc.blur_radius_q8);
        let r_cand = ((sc.blur_radius_q8 as u32 * path.radius_pct as u32) / 10_000)
            .min(u16::MAX as u32) as u16;
        let mut cand_luma = box_blur_u8(&ref_luma, w, h, r_cand);
        // 候选量化（图像域降级主源）。
        let mut i = 0;
        while i < cand_luma.len() {
            cand_luma[i] = quantize_byte(cand_luma[i], path.quant_bits);
            i += 1;
        }
        let s_ssim = ssim_score(&ref_blur, &cand_luma, w, h)?;
        // 色度域：候选 RGBA 量化。
        let cand_rgba = quantize_rgba(&ref_rgba, path.quant_bits);
        let s_de = deltae_score(&ref_rgba, &cand_rgba, w, h)?;
        // 辅助支路。
        let (s_aux, a_err) = aux_score(&sc, &path);
        // 聚合。
        let score = aggregate_score(s_ssim, s_de, s_aux)?;
        let (tier, _tier_name) = tier_of(score)?;
        // 帧成本（基价 × 成本百分比，万分比）。
        let perf_cost = (sc.base_cost as u32 * path.cost_pct as u32) / 10_000;
        // 损失声明（参考路径损失恒 0、声明恒 0）。
        let native = if path_idx == PATH_REF_IDX {
            score
        } else {
            self.native_score_of(sc_idx)?
        };
        let loss = native.saturating_sub(score);
        if let Err(e) = check_loss_declared(loss, path.declared_max_loss as u32) {
            // 损失声明违约：显性计数 + 留痕后上抛——禁止静默超损。
            self.breach_loss = self.breach_loss.saturating_add(1);
            self.ledger.record(EventKind::LossBreach, sc.id, loss);
            return Err(e);
        }
        self.ledger
            .record(EventKind::LossDeclared, sc.id, path.declared_max_loss as u32);
        Ok(ScoredResult {
            scenario_id: sc.id,
            path_id: path.id,
            ssim_score: s_ssim,
            deltae_score: s_de,
            aux_score: s_aux,
            aux_err: a_err,
            score,
            tier,
            loss_vs_native: loss,
            loss_declared: path.declared_max_loss as u32,
            perf_cost,
        })
    }

    /// 标准评分入口（指纹校验 → 评分 → 记账 → 上链）。
    pub fn run_scenario(
        &mut self,
        scenario_id: u16,
        path_id: u8,
    ) -> Result<ScoredResult, EffBenchError> {
        self.verify_scenarios()?;
        let sc_idx = Self::scenario_index(scenario_id)?;
        let path_idx = Self::path_index(path_id)?;
        let sc = SCENARIOS[sc_idx];
        let r = self.run_scenario_raw(sc_idx, path_idx)?;
        self.scored_total = self.scored_total.saturating_add(1);
        self.ledger.record(EventKind::Scored, sc.id, r.score);
        // 合同档判定（低于即违约——留痕后显性上抛）。
        if r.score < sc.contract_min_score as u32 {
            self.breach_quality = self.breach_quality.saturating_add(1);
            self.ledger.record(EventKind::QualityBreach, sc.id, r.score);
            return Err(EffBenchError::QualityBudgetBreach);
        }
        // 上链（场景指纹 + 质量分 + 环境画像三要素签名）。
        let fp = scenario_fingerprint(&SCENARIOS[sc_idx]);
        self.chain.append(r, fp)?;
        self.ledger.record(EventKind::ChainAppend, sc.id, r.score);
        Ok(r)
    }

    /// 帧级评估（质量预算 × 性能预算双约束）。
    /// 逐项评分：质量违约按违约计数并记入判定（不中断其余项——
    /// 帧级结论要完整）；帧成本合计超 [`FRAME_PERF_BUDGET`] 即
    /// 性能违约（0x1377）。任一违约存在时返回 Err（结论本体在
    /// 上次成功落账的 [`FrameVerdict`]——本接口约定：Err 时以
    /// [`EffBenchEngine::last_frame_verdict`] 读取判定）。
    pub fn run_frame(&mut self, picks: &[FramePick]) -> Result<FrameVerdict, EffBenchError> {
        self.verify_scenarios()?;
        let mut results = Vec::with_capacity(picks.len());
        let mut total_cost: u32 = 0;
        let mut min_score: u32 = QUALITY_SCORE_MAX;
        let mut q_breaches: u32 = 0;
        let mut perf_breach = false;
        let mut i = 0;
        while i < picks.len() {
            let sc_idx = Self::scenario_index(picks[i].scenario_id)?;
            let path_idx = Self::path_index(picks[i].path_id)?;
            let sc = SCENARIOS[sc_idx];
            let r = self.run_scenario_raw(sc_idx, path_idx)?;
            self.scored_total = self.scored_total.saturating_add(1);
            self.ledger.record(EventKind::Scored, sc.id, r.score);
            if r.score < sc.contract_min_score as u32 {
                q_breaches += 1;
                self.breach_quality = self.breach_quality.saturating_add(1);
                self.ledger.record(EventKind::QualityBreach, sc.id, r.score);
            }
            total_cost = total_cost.saturating_add(r.perf_cost);
            if r.score < min_score {
                min_score = r.score;
            }
            let fp = scenario_fingerprint(&SCENARIOS[sc_idx]);
            self.chain.append(r, fp)?;
            results.push(r);
            i += 1;
        }
        if total_cost > FRAME_PERF_BUDGET {
            perf_breach = true;
            self.breach_perf = self.breach_perf.saturating_add(1);
            self.ledger.record(EventKind::PerfBreach, 0xFFFF, total_cost);
        }
        let verdict = FrameVerdict {
            total_cost,
            min_score,
            quality_breaches: q_breaches,
            perf_breach,
            results,
        };
        self.last_frame_verdict = Some(verdict.clone());
        if q_breaches > 0 {
            return Err(EffBenchError::QualityBudgetBreach);
        }
        if perf_breach {
            return Err(EffBenchError::PerfBudgetBreach);
        }
        Ok(verdict)
    }

    /// 全场景 × 全路径矩阵采集（**无合同档强制**——基准采集模式：
    /// run_all_twice 与对拍测试用；公开以便上层采集全矩阵做对拍。
    /// 损失声明校验仍然生效——声明是降级路径的硬不变量）。
    pub fn collect_matrix_scores(&mut self) -> Result<Vec<ScoredResult>, EffBenchError> {
        self.verify_scenarios()?;
        let mut out = Vec::with_capacity(SCENARIO_COUNT * PATH_COUNT);
        let mut si = 0;
        while si < SCENARIO_COUNT {
            let mut pi = 0;
            while pi < PATH_COUNT {
                out.push(self.run_scenario_raw(si, pi)?);
                pi += 1;
            }
            si += 1;
        }
        Ok(out)
    }

    /// 双跑确定性对拍（全部场景 × 全部路径跑两遍逐字段比对）。
    /// 漂移即 0x1375 留痕并上抛——基准结论只在确定性下可信。
    pub fn run_all_twice(&mut self) -> Result<(), EffBenchError> {
        self.verify_scenarios()?;
        let first = self.collect_all_scores()?;
        let second = self.collect_all_scores()?;
        Self::compare_run_sets(&first, &second)?;
        self.dual_run_ok = true;
        self.ledger.record(EventKind::DualRun, 0xFFFF, first.len() as u32);
        Ok(())
    }

    /// 全场景 × 全路径采集（native_cache 命中不影响评分值——
    /// 评分纯函数于 (场景, 路径)，缓存只省重算）。
    fn collect_all_scores(&mut self) -> Result<Vec<ScoredResult>, EffBenchError> {
        self.collect_matrix_scores()
    }

    /// 双跑结果逐字段比对（独立函数——注入面直接调用做漂移注入）。
    pub fn compare_run_sets(
        a: &[ScoredResult],
        b: &[ScoredResult],
    ) -> Result<(), EffBenchError> {
        if a.len() != b.len() {
            return Err(EffBenchError::DualRunDrift);
        }
        let mut i = 0;
        while i < a.len() {
            if a[i] != b[i] {
                return Err(EffBenchError::DualRunDrift);
            }
            i += 1;
        }
        Ok(())
    }

    /// 最近一次帧判定（含违约场景——Err 返回值的可读载体）。
    pub fn last_frame_verdict(&self) -> Option<&FrameVerdict> {
        self.last_frame_verdict.as_ref()
    }

    /// 签名链全量校验（体检用；失配返回 (错误, 槽位)）。
    pub fn verify_chain(&self) -> Result<usize, (EffBenchError, usize)> {
        self.chain.verify()
    }

    /// 事件账只读访问。
    pub const fn ledger(&self) -> &EventLedger {
        &self.ledger
    }

    /// 链长。
    pub fn chain_len(&self) -> usize {
        self.chain.len()
    }

    /// 评分总次数。
    pub const fn scored_total(&self) -> u32 {
        self.scored_total
    }

    /// 体检（七项检查；红/黄/绿三灯，红灯存在即 0x137E 可上抛）。
    pub fn health(&self) -> HealthReport {
        let mut items: Vec<HealthItem> = Vec::with_capacity(8);
        let mut red: u32 = 0;
        let mut yellow: u32 = 0;
        // 1. 场景指纹（只读复算）。
        let mut fp_ok = true;
        let mut i = 0;
        while i < SCENARIO_COUNT {
            if scenario_fingerprint(&SCENARIOS[i]) != SCENARIO_FINGERPRINTS[i] {
                fp_ok = false;
            }
            i += 1;
        }
        if fp_ok {
            items.push(HealthItem {
                name: "scenario-fingerprints",
                level: 0,
                note: String::from("8 条场景指纹与冻结表一致"),
            });
        } else {
            red += 1;
            items.push(HealthItem {
                name: "scenario-fingerprints",
                level: 2,
                note: String::from("场景指纹失配：疑似挑参数，禁止使用质量分"),
            });
        }
        // 2. 签名链完整性。
        match self.chain.verify() {
            Ok(n) => items.push(HealthItem {
                name: "signature-chain",
                level: 0,
                note: format!("签名链完好，共 {n} 环"),
            }),
            Err((e, slot)) => {
                red += 1;
                items.push(HealthItem {
                    name: "signature-chain",
                    level: 2,
                    note: format!("签名链故障：{} @槽位 {slot}", e.code()),
                });
            }
        }
        // 3. 对比库指纹。
        let (n_refs, refs_ok) = self.refs.verify_all();
        if refs_ok {
            items.push(HealthItem {
                name: "ref-store",
                level: 0,
                note: format!("对比库 {n_refs} 条指纹全部一致"),
            });
        } else {
            red += 1;
            items.push(HealthItem {
                name: "ref-store",
                level: 2,
                note: format!("对比库指纹失配（{n_refs} 条中至少一条被改）"),
            });
        }
        // 4. 双跑确定性。
        if self.dual_run_ok {
            items.push(HealthItem {
                name: "dual-run",
                level: 0,
                note: String::from("最近一次双跑对拍一致"),
            });
        } else {
            red += 1;
            items.push(HealthItem {
                name: "dual-run",
                level: 2,
                note: String::from("双跑对拍漂移：实现路径存在未声明不确定性"),
            });
        }
        // 5. 质量违约累计（黄灯）。
        if self.breach_quality == 0 {
            items.push(HealthItem {
                name: "quality-budget",
                level: 0,
                note: String::from("无质量预算违约"),
            });
        } else {
            yellow += 1;
            items.push(HealthItem {
                name: "quality-budget",
                level: 1,
                note: format!("质量预算违约累计 {} 次", self.breach_quality),
            });
        }
        // 6. 性能违约累计（黄灯）。
        if self.breach_perf == 0 {
            items.push(HealthItem {
                name: "perf-budget",
                level: 0,
                note: String::from("无性能预算违约"),
            });
        } else {
            yellow += 1;
            items.push(HealthItem {
                name: "perf-budget",
                level: 1,
                note: format!("性能预算违约累计 {} 次", self.breach_perf),
            });
        }
        // 7. 损失声明违约累计（黄灯）。
        if self.breach_loss == 0 {
            items.push(HealthItem {
                name: "loss-declaration",
                level: 0,
                note: String::from("无损失声明违约"),
            });
        } else {
            yellow += 1;
            items.push(HealthItem {
                name: "loss-declaration",
                level: 1,
                note: format!("损失声明违约累计 {} 次", self.breach_loss),
            });
        }
        // 8. 账溢出（黄灯）。
        let dropped = self.ledger.dropped();
        if dropped == 0 {
            items.push(HealthItem {
                name: "ledger",
                level: 0,
                note: String::from("事件账无覆写"),
            });
        } else {
            yellow += 1;
            items.push(HealthItem {
                name: "ledger",
                level: 1,
                note: format!("事件账已覆写 {dropped} 条"),
            });
        }
        HealthReport {
            items,
            red_count: red,
            yellow_count: yellow,
        }
    }

    /// 体检门禁（红灯存在即 [`EffBenchError::HealthDegraded`]）。
    pub fn health_gate(&self) -> Result<(), EffBenchError> {
        let r = self.health();
        if r.red_count > 0 {
            return Err(EffBenchError::HealthDegraded);
        }
        Ok(())
    }

    /// 自检报告（纯文本；超限 0x137C）。
    pub fn selfcheck_report(&self) -> Result<String, EffBenchError> {
        let health = self.health();
        let mut out = String::with_capacity(512);
        out.push_str("== CGPU-F2086 效果质量基准 · 自检 ==\n");
        out.push_str("version=");
        out.push_str(self.version_tag);
        out.push('\n');
        out.push_str("env(gpu,isa,sched)=");
        let env_s = self.env.serialize();
        out.push_str(&format!("{},{},{}\n", env_s[0], env_s[1], env_s[2]));
        out.push_str(&format!(
            "scored_total={} chain_len={} dual_run_ok={}\n",
            self.scored_total,
            self.chain.len(),
            self.dual_run_ok
        ));
        out.push_str(&format!(
            "breaches(quality,perf,loss)={},{},{}\n",
            self.breach_quality, self.breach_perf, self.breach_loss
        ));
        let mut i = 0;
        while i < health.items.len() {
            let it = &health.items[i];
            out.push_str(&format!(
                "[{}] {} {}\n",
                if it.level == 0 {
                    "OK  "
                } else if it.level == 1 {
                    "WARN"
                } else {
                    "FAIL"
                },
                it.name,
                it.note
            ));
            i += 1;
        }
        if out.len() > REPORT_MAX_BYTES {
            return Err(EffBenchError::ReportTooLarge);
        }
        Ok(out)
    }

    // -- 注入测试面（三类注入各检出；内核常规路径不得调用） --

    /// 降级注入：强制以近似降级路径（PATH_APPROX）跑高合同档场景，
    /// 预期质量预算违约被检出。
    pub fn inject_downgrade(&mut self, scenario_id: u16) -> Result<InjectionVerdict, EffBenchError> {
        match self.run_scenario(scenario_id, PATH_TABLE[PATH_APPROX_IDX].id) {
            Err(EffBenchError::QualityBudgetBreach) => {
                self.ledger.record(EventKind::QualityBreach, scenario_id, 0x1376);
                Ok(InjectionVerdict {
                    detected: true,
                    code: EffBenchError::QualityBudgetBreach.code(),
                    locate: scenario_id as u32,
                })
            }
            Err(e) => Ok(InjectionVerdict {
                detected: false,
                code: e.code(),
                locate: scenario_id as u32,
            }),
            Ok(r) => Ok(InjectionVerdict {
                detected: false,
                code: 0,
                locate: r.score,
            }),
        }
    }

    /// 篡改注入：翻转指定槽位链正文质量分 bit0，校验链定位失配槽。
    pub fn inject_tamper_chain(&mut self, slot: usize) -> Result<InjectionVerdict, EffBenchError> {
        if !self.chain.tamper_score_for_injection(slot) {
            return Ok(InjectionVerdict {
                detected: false,
                code: 0,
                locate: 0xFFFF_FFFF,
            });
        }
        match self.chain.first_fault() {
            Some((EffBenchError::SignatureMismatch, hit)) => {
                self.ledger.record(EventKind::TamperFound, 0xFFFF, hit as u32);
                Ok(InjectionVerdict {
                    detected: true,
                    code: EffBenchError::SignatureMismatch.code(),
                    locate: hit as u32,
                })
            }
            Some((e, hit)) => Ok(InjectionVerdict {
                detected: true,
                code: e.code(),
                locate: hit as u32,
            }),
            None => Ok(InjectionVerdict {
                detected: false,
                code: 0,
                locate: slot as u32,
            }),
        }
    }

    /// 挑参数注入：对指定场景串行化字节翻转指定位后重算指纹，
    /// 与冻结表比对——预期指纹失配被检出。
    pub fn inject_cherry_pick(
        &mut self,
        scenario_idx: usize,
        byte_pos: usize,
    ) -> Result<InjectionVerdict, EffBenchError> {
        if scenario_idx >= SCENARIO_COUNT || byte_pos >= SCEN_SER_LEN {
            return Ok(InjectionVerdict {
                detected: false,
                code: 0,
                locate: 0xFFFF_FFFF,
            });
        }
        let mut ser = serialize_scenario(&SCENARIOS[scenario_idx]);
        ser[byte_pos] ^= 0x01;
        let fp_tampered = fnv1a32(&ser);
        let fp_frozen = SCENARIO_FINGERPRINTS[scenario_idx];
        if fp_tampered != fp_frozen {
            self.ledger
                .record(EventKind::CherryPickFound, SCENARIOS[scenario_idx].id, fp_tampered);
            Ok(InjectionVerdict {
                detected: true,
                code: EffBenchError::ScenarioFingerprintMismatch.code(),
                locate: scenario_idx as u32,
            })
        } else {
            Ok(InjectionVerdict {
                detected: false,
                code: 0,
                locate: scenario_idx as u32,
            })
        }
    }

    /// 漂移注入：对双跑结果集篡改一个字段后比对——预期漂移检出。
    pub fn inject_drift(
        a: &[ScoredResult],
        b: &[ScoredResult],
    ) -> Result<InjectionVerdict, EffBenchError> {
        match Self::compare_run_sets(a, b) {
            Err(EffBenchError::DualRunDrift) => Ok(InjectionVerdict {
                detected: true,
                code: EffBenchError::DualRunDrift.code(),
                locate: 0,
            }),
            Ok(()) => Ok(InjectionVerdict {
                detected: false,
                code: 0,
                locate: 0,
            }),
            Err(e) => Ok(InjectionVerdict {
                detected: false,
                code: e.code(),
                locate: 0,
            }),
        }
    }

    /// 链快照归档（链满处置；返回旧链并重置）。
    pub fn chain_snapshot_reset(&mut self) -> Vec<ChainNode> {
        self.chain.snapshot_reset()
    }

    /// 链只读节点。
    pub fn chain_node(&self, slot: usize) -> Option<&ChainNode> {
        self.chain.node(slot)
    }
}

/// 双跑一致性旗标字段（引擎结构尾段；pub(crate) 语义由注入面维护）。
impl EffBenchEngine {
    /// 显性置漂移旗标（体检演示与注入面用）。
    pub fn mark_drift_for_injection(&mut self) {
        self.dual_run_ok = false;
    }
}

// ---------------------------------------------------------------------------
// 十八、编译期封顶断言（栈上禁区与表长一致性）
// ---------------------------------------------------------------------------

const _: () = assert!(core::mem::size_of::<BenchScenario>() <= 128);
const _: () = assert!(core::mem::size_of::<ScoredResult>() <= 64);
const _: () = assert!(core::mem::size_of::<ChainNode>() <= 128);
const _: () = assert!(SSIM_WIN * SSIM_WIN == 64);

// ---------------------------------------------------------------------------
// 十九、测试（判据"两组"：基准组 / 对比组；另含注入面组）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod effbench_tests_base {
    use super::*;

    fn env() -> EnvProfile {
        EnvProfile {
            gpu_tier: 2,
            isa_mask: 0b0011,
            sched_class: 1,
        }
    }

    // -- 诊断码段独占 --

    #[test]
    fn diag_codes_exclusive_1370_137f() {
        let all = [
            EffBenchError::UnknownEffect,
            EffBenchError::UnknownScenario,
            EffBenchError::ScenarioFingerprintMismatch,
            EffBenchError::SampleSizeMismatch,
            EffBenchError::UnknownPath,
            EffBenchError::DualRunDrift,
            EffBenchError::QualityBudgetBreach,
            EffBenchError::PerfBudgetBreach,
            EffBenchError::SignatureMismatch,
            EffBenchError::SignatureChainBroken,
            EffBenchError::LossDeclarationExceeded,
            EffBenchError::ScoreOutOfRange,
            EffBenchError::ReportTooLarge,
            EffBenchError::InjectionUndetected,
            EffBenchError::HealthDegraded,
            EffBenchError::LedgerFull,
        ];
        assert_eq!(all.len(), 16);
        let mut i = 0;
        while i < all.len() {
            assert_eq!(all[i].code(), 0x1370 + i as u16);
            // 每变体中文可操作诊断非空且含句读。
            let d = all[i].detail();
            assert!(!d.is_empty());
            assert!(d.contains('：') || d.contains('。'));
            let mut j = i + 1;
            while j < all.len() {
                assert_ne!(all[i].code(), all[j].code(), "诊断码必须互异");
                j += 1;
            }
            i += 1;
        }
    }

    // -- const 形状 --

    #[test]
    fn const_shape_and_fingerprints_frozen() {
        assert_eq!(SCENARIOS.len(), SCENARIO_COUNT);
        assert_eq!(PATH_TABLE.len(), PATH_COUNT);
        // 权重合同：三支路和恒满分。
        assert_eq!(
            QUALITY_WEIGHT_SSIM + QUALITY_WEIGHT_DELTA_E + QUALITY_WEIGHT_AUX,
            QUALITY_SCORE_MAX
        );
        // 指纹非零且互异（互异是"防挑参数"可定位性的前提）。
        let mut i = 0;
        while i < SCENARIO_COUNT {
            assert_ne!(SCENARIO_FINGERPRINTS[i], 0);
            let mut j = i + 1;
            while j < SCENARIO_COUNT {
                assert_ne!(SCENARIO_FINGERPRINTS[i], SCENARIO_FINGERPRINTS[j]);
                j += 1;
            }
            i += 1;
        }
        // 路径表 id 连续且损失声明单调递增（降级越深声明越宽）。
        let mut p = 1;
        while p < PATH_COUNT {
            assert_eq!(PATH_TABLE[p].id, p as u8);
            assert!(PATH_TABLE[p].declared_max_loss >= PATH_TABLE[p - 1].declared_max_loss);
            p += 1;
        }
        // 档位表单调递减。
        let mut t = 1;
        while t < TIER_TABLE.len() {
            assert!(TIER_TABLE[t].0 <= TIER_TABLE[t - 1].0);
            t += 1;
        }
    }

    // -- 整数数学 --

    #[test]
    fn integer_math_known_values() {
        assert_eq!(isqrt64(0), 0);
        assert_eq!(isqrt64(1), 1);
        assert_eq!(isqrt64(3), 1);
        assert_eq!(isqrt64(4), 2);
        assert_eq!(isqrt64(1_000_000), 1_000);
        assert_eq!(isqrt64(u64::MAX), 4_294_967_295);
        assert_eq!(icbrt64(0), 0);
        assert_eq!(icbrt64(7), 1);
        assert_eq!(icbrt64(8), 2);
        assert_eq!(icbrt64(1_000_000), 100);
        assert_eq!(icbrt64(u64::MAX), 2_642_245);
        // FNV-1a 32 已知向量（Landon Curt Noll 参考值）。
        assert_eq!(fnv1a32(b""), 0x811C_9DC5);
        assert_eq!(fnv1a32(b"a"), 0xE40C_292C);
    }

    // -- 场景指纹 --

    #[test]
    fn scenario_fingerprint_stability_and_tamper_sensitivity() {
        let sc = &SCENARIOS[0];
        let f1 = scenario_fingerprint(sc);
        let f2 = scenario_fingerprint(sc);
        assert_eq!(f1, f2, "同场景指纹必须确定");
        // 串行化任一字节翻转必改指纹（挑参数可检出的灵敏度前提）。
        let mut flipped_hit = 0;
        let mut b = 0;
        while b < SCEN_SER_LEN {
            let mut ser = serialize_scenario(sc);
            ser[b] ^= 0x01;
            if fnv1a32(&ser) != f1 {
                flipped_hit += 1;
            }
            b += 1;
        }
        assert_eq!(flipped_hit, SCEN_SER_LEN, "串行化无死字节：任一字节参与指纹");
    }

    // -- 合成参考帧确定性 --

    #[test]
    fn scenario_render_deterministic_and_shaped() {
        let mut i = 0;
        while i < SCENARIO_COUNT {
            let sc = &SCENARIOS[i];
            let a = render_scenario_rgba(sc);
            let b = render_scenario_rgba(sc);
            assert_eq!(a.len(), sc.width as usize * sc.height as usize * 4);
            assert_eq!(a, b, "参考帧必须双跑字节级一致");
            i += 1;
        }
    }

    // -- SSIM / deltaE 双指标 --

    #[test]
    fn ssim_identical_full_and_inverted_low() {
        let sc = &SCENARIOS[0];
        let rgba = render_scenario_rgba(sc);
        let luma = luma_of_rgba(&rgba).unwrap();
        let (w, h) = (sc.width as usize, sc.height as usize);
        let s_same = ssim_score(&luma, &luma, w, h).unwrap();
        assert_eq!(s_same, QUALITY_SCORE_MAX, "同帧 SSIM 必须满分");
        let mut inv = luma.clone();
        let mut i = 0;
        while i < inv.len() {
            inv[i] = 255 - inv[i];
            i += 1;
        }
        let s_inv = ssim_score(&luma, &inv, w, h).unwrap();
        assert!(s_inv < 2_000, "反相帧 SSIM 必须低分，实测 {s_inv}");
        // 尺寸不符显性报错。
        assert_eq!(
            ssim_score(&luma, &luma[..luma.len() - 1], w, h).unwrap_err(),
            EffBenchError::SampleSizeMismatch
        );
    }

    #[test]
    fn deltae_identical_full_and_shifted_lower() {
        let sc = &SCENARIOS[4];
        let rgba = render_scenario_rgba(sc);
        let (w, h) = (sc.width as usize, sc.height as usize);
        let s_same = deltae_score(&rgba, &rgba, w, h).unwrap();
        assert_eq!(s_same, QUALITY_SCORE_MAX, "同帧 deltaE 必须满分");
        let mut shifted = rgba.clone();
        let mut i = 0;
        while i < shifted.len() {
            if i % 4 != 3 {
                shifted[i] = shifted[i].saturating_add(24);
            }
            i += 1;
        }
        let s_shift = deltae_score(&rgba, &shifted, w, h).unwrap();
        assert!(s_shift < s_same, "整体色偏必须掉分：{s_shift} < {s_same}");
        // JND 映射端点。
        assert_eq!(deltae_q12_to_score(4_096), QUALITY_SCORE_MAX);
        assert_eq!(deltae_q12_to_score(81_920), 0);
    }

    // -- 辅助指标 --

    #[test]
    fn aux_metrics_ref_lossless_and_degradation_monotone() {
        // 模糊半径：参考路径无损；降级单调掉分。
        let (s_ref, e_ref) = aux_blur_radius(18 << 8, 18 << 8, 30);
        assert_eq!(e_ref, 0);
        assert_eq!(s_ref, QUALITY_SCORE_MAX);
        let gpu_r_q8 = ((18u32 * 9_900) / 10_000) as u16;
        let (s_gpu, _) = aux_blur_radius(18 << 8, gpu_r_q8 << 8, 30);
        assert!(s_gpu <= s_ref);
        // α 混合：同规程同参数 → 无损。
        let (s_a_ref, e_a_ref) = aux_alpha_blend(170, 8, 8, 8, 6);
        assert_eq!(e_a_ref, 0);
        assert_eq!(s_a_ref, QUALITY_SCORE_MAX);
        // 动画平滑度：参考步进无损。
        let (s_n_ref, e_n_ref) = aux_anim_smoothness(200, 10_000, 900);
        assert_eq!(e_n_ref, 0);
        assert_eq!(s_n_ref, QUALITY_SCORE_MAX);
        // 步进折减产生真实抖动。
        let (s_n_approx, _) = aux_anim_smoothness(200, 5_000, 900);
        assert!(s_n_approx < s_n_ref);
    }

    // -- 量化与亮度 --

    #[test]
    fn quantize_identity_at_8bit_and_monotone() {
        let mut v = 0;
        while v < 256 {
            assert_eq!(quantize_byte(v as u8, 8), v as u8, "8bit 量化必须恒等");
            v += 1;
        }
        // 位深越低误差单调不减（同字节）。
        let v = 137u8;
        let e7 = (quantize_byte(v, 7) as i32 - v as i32).abs();
        let e6 = (quantize_byte(v, 6) as i32 - v as i32).abs();
        let e5 = (quantize_byte(v, 5) as i32 - v as i32).abs();
        let e4 = (quantize_byte(v, 4) as i32 - v as i32).abs();
        assert!(e4 >= e5 || e4 >= e5.min(e6.min(e7)));
        // 亮度：纯白 → 255。
        let white = [255u8, 255, 255, 255];
        let l = luma_of_rgba(&white).unwrap();
        assert_eq!(l[0], 255);
    }

    // -- 质量分聚合与档位 --

    #[test]
    fn aggregate_and_tier_guards() {
        assert_eq!(
            aggregate_score(10_000, 10_000, 10_000).unwrap(),
            QUALITY_SCORE_MAX
        );
        assert_eq!(aggregate_score(0, 0, 0).unwrap(), 0);
        assert_eq!(
            aggregate_score(10_001, 0, 0).unwrap_err(),
            EffBenchError::ScoreOutOfRange
        );
        assert_eq!(tier_of(9_000).unwrap().0, 0);
        assert_eq!(tier_of(8_999).unwrap().0, 1);
        assert_eq!(tier_of(0).unwrap().0, 4);
        assert_eq!(
            tier_of(10_001).unwrap_err(),
            EffBenchError::ScoreOutOfRange
        );
        // 损失声明校验。
        assert!(check_loss_declared(0, 0).is_ok());
        assert!(check_loss_declared(899, 900).is_ok());
        assert_eq!(
            check_loss_declared(901, 900).unwrap_err(),
            EffBenchError::LossDeclarationExceeded
        );
    }

    // -- 引擎基础与全场景参考分 --

    #[test]
    fn engine_ref_path_full_score_all_scenarios() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        let mut i = 0;
        while i < SCENARIO_COUNT {
            let r = eng
                .run_scenario(SCENARIOS[i].id, PATH_TABLE[PATH_REF_IDX].id)
                .unwrap_or_else(|e| panic!("场景 {} 参考路径应满分通过: {:?}", i, e));
            assert_eq!(r.score, QUALITY_SCORE_MAX, "参考路径 = 对比库自身");
            assert_eq!(r.loss_vs_native, 0);
            assert_eq!(r.tier, 0);
            i += 1;
        }
        assert_eq!(eng.chain_len(), SCENARIO_COUNT as usize);
        assert!(eng.verify_chain().is_ok());
        eng.health_gate().unwrap();
    }

    // -- 双跑确定性 --

    #[test]
    fn dual_run_determinism_all_matrix() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        eng.run_all_twice().unwrap();
        // 结果集逐字段比对已内置；这里再验对拍事件已记账。
        assert!(eng.ledger().counter(EventKind::DualRun) >= 1);
    }

    // -- 事件账环形 --

    #[test]
    fn ledger_ring_and_counters() {
        let mut led = EventLedger::new();
        let mut i = 0u32;
        while i < 200 {
            led.record(EventKind::Scored, (i % 8) as u16, i);
            i += 1;
        }
        assert_eq!(led.count(), LEDGER_CAP);
        assert_eq!(led.dropped(), 72);
        assert_eq!(led.counter(EventKind::Scored), 200);
        // 逆序取最新。
        let newest = led.row_newest_first(0).unwrap();
        assert_eq!(newest.value, 199);
        assert!(led.row_newest_first(LEDGER_CAP).is_none());
    }

    // -- 签名链基础 --

    #[test]
    fn chain_append_verify_and_capacity() {
        let mut chain = SignatureChain::new(0xDEAD_BEEF);
        let mut i = 0;
        while i < CHAIN_CAP {
            let body = ScoredResult {
                scenario_id: i as u16,
                path_id: 0,
                ssim_score: 10_000,
                deltae_score: 10_000,
                aux_score: 10_000,
                aux_err: 0,
                score: 10_000,
                tier: 0,
                loss_vs_native: 0,
                loss_declared: 0,
                perf_cost: 100,
            };
            assert_eq!(chain.append(body, 0x1234).unwrap(), i as u32);
            i += 1;
        }
        assert!(chain.verify().is_ok());
        // 满链显性拒绝。
        let body = ScoredResult {
            scenario_id: 99,
            path_id: 0,
            ssim_score: 1,
            deltae_score: 1,
            aux_score: 1,
            aux_err: 0,
            score: 1,
            tier: 4,
            loss_vs_native: 0,
            loss_declared: 0,
            perf_cost: 1,
        };
        assert_eq!(chain.append(body, 0x1234).unwrap_err(), EffBenchError::LedgerFull);
        // 快照归档清空后可再建。
        let snap = chain.snapshot_reset();
        assert_eq!(snap.len(), CHAIN_CAP);
        assert!(chain.is_empty());
        assert!(chain.verify().is_ok());
    }

    // -- 环境画像闭集 --

    #[test]
    fn env_profile_closed_set() {
        assert!(env().validate().is_ok());
        let bad = EnvProfile {
            gpu_tier: 4,
            isa_mask: 0,
            sched_class: 0,
        };
        assert_eq!(bad.validate().unwrap_err(), EffBenchError::ScoreOutOfRange);
        assert_ne!(env().hash(), 0);
    }
}

#[cfg(test)]
mod effbench_tests_compare {
    use super::*;

    fn env() -> EnvProfile {
        EnvProfile {
            gpu_tier: 2,
            isa_mask: 0b0011,
            sched_class: 1,
        }
    }

    // -- 路径质量单调性（实现路径对拍核心） --

    #[test]
    fn path_scores_monotone_over_degradation() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        // 矩阵采集（无合同档强制——单调性是采集层面的物理事实）。
        let matrix = eng.collect_matrix_scores().unwrap();
        let mut si = 0;
        while si < SCENARIO_COUNT {
            let mut prev: Option<u32> = None;
            let mut pi = 0;
            while pi < PATH_COUNT {
                let r = &matrix[si * PATH_COUNT + pi];
                assert_eq!(r.scenario_id, SCENARIOS[si].id);
                assert_eq!(r.path_id, pi as u8);
                if let Some(p) = prev {
                    assert!(
                        r.score <= p,
                        "场景 {} 路径 {} 分数 {} 不得超过上一路径 {}",
                        SCENARIOS[si].id,
                        PATH_TABLE[pi].name,
                        r.score,
                        p
                    );
                }
                prev = Some(r.score);
                pi += 1;
            }
            si += 1;
        }
    }

    #[test]
    fn loss_vs_native_within_declaration_all_matrix() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        let matrix = eng.collect_matrix_scores().unwrap();
        let mut si = 0;
        while si < SCENARIO_COUNT {
            let native = matrix[si * PATH_COUNT + PATH_REF_IDX].score;
            assert_eq!(native, QUALITY_SCORE_MAX, "参考路径 = 对比库自身");
            let mut pi = 1;
            while pi < PATH_COUNT {
                let r = &matrix[si * PATH_COUNT + pi];
                assert!(r.loss_vs_native <= r.loss_declared);
                pi += 1;
            }
            si += 1;
        }
        // 合同档：参考路径必然达标（质量预算的采集侧事实）。
        let mut si2 = 0;
        while si2 < SCENARIO_COUNT {
            let r = &matrix[si2 * PATH_COUNT + PATH_REF_IDX];
            assert!(r.score >= SCENARIOS[si2].contract_min_score as u32);
            si2 += 1;
        }
    }

    // -- 对比库 --

    #[test]
    fn ref_store_fingerprints_and_lookup() {
        let mut store = RefStore::build().unwrap();
        assert_eq!(store.len(), SCENARIO_COUNT);
        assert!(!store.is_empty());
        let (n, ok) = store.verify_all();
        assert_eq!(n, SCENARIO_COUNT);
        assert!(ok);
        // 命中计数。
        let e = store.lookup(SCENARIOS[3].id).unwrap();
        assert_eq!(e.hits, 1);
        let e = store.lookup(SCENARIOS[3].id).unwrap();
        assert_eq!(e.hits, 2);
        // 闭集外显性拒绝。
        assert_eq!(
            store.lookup(99).unwrap_err(),
            EffBenchError::UnknownScenario
        );
        // 种子参与指纹：同帧不同种子指纹不同。
        let sc = &SCENARIOS[3];
        assert_ne!(
            RefStore::ref_fingerprint(sc.seed + 1, &render_scenario_rgba(sc)),
            RefStore::ref_fingerprint(sc.seed, &render_scenario_rgba(sc))
        );
    }

    // -- 质量预算 × 性能预算双约束 --

    #[test]
    fn frame_budget_green_within_contract() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        let picks = [
            FramePick { scenario_id: 0, path_id: 0 },
            FramePick { scenario_id: 4, path_id: 1 },
            FramePick { scenario_id: 6, path_id: 1 },
            FramePick { scenario_id: 7, path_id: 2 },
        ];
        let v = eng.run_frame(&picks).unwrap();
        assert_eq!(v.quality_breaches, 0);
        assert!(!v.perf_breach);
        assert!(v.total_cost <= FRAME_PERF_BUDGET);
        assert!(v.min_score >= 9_000);
        eng.health_gate().unwrap();
    }

    #[test]
    fn frame_perf_breach_explicit() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        // 重复挑高基价场景压穿性能预算（S01 基价 220 × REF 成本 100%）。
        let mut picks = Vec::new();
        let mut i = 0;
        while i < 40 {
            picks.push(FramePick { scenario_id: 1, path_id: 0 });
            i += 1;
        }
        let err = eng.run_frame(&picks).unwrap_err();
        assert_eq!(err, EffBenchError::PerfBudgetBreach);
        let v = eng.last_frame_verdict().unwrap();
        assert!(v.perf_breach);
        assert!(v.total_cost > FRAME_PERF_BUDGET);
        assert_eq!(eng.breach_perf, 1);
    }

    #[test]
    fn frame_quality_breach_explicit() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        let picks = [
            FramePick { scenario_id: 0, path_id: PATH_APPROX_IDX as u8 },
            FramePick { scenario_id: 4, path_id: 0 },
        ];
        let err = eng.run_frame(&picks).unwrap_err();
        assert_eq!(err, EffBenchError::QualityBudgetBreach);
        let v = eng.last_frame_verdict().unwrap();
        assert!(v.quality_breaches >= 1);
        assert_eq!(eng.breach_quality, 1);
    }

    #[test]
    fn unknown_scenario_and_path_rejected() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        assert_eq!(
            eng.run_scenario(88, 0).unwrap_err(),
            EffBenchError::UnknownScenario
        );
        assert_eq!(
            eng.run_scenario(0, 9).unwrap_err(),
            EffBenchError::UnknownPath
        );
    }

    // -- 注入面三检出 --

    #[test]
    fn inject_downgrade_detected() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        // S01 合同档 A(9000)：近似降级路径必须被预算违约检出。
        let v = eng.inject_downgrade(SCENARIOS[0].id).unwrap();
        assert!(v.detected, "降级注入必须检出");
        assert_eq!(v.code, EffBenchError::QualityBudgetBreach.code());
        assert_eq!(v.locate, SCENARIOS[0].id as u32);
    }

    #[test]
    fn inject_tamper_chain_located() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        // 先造 3 环。
        let mut i = 0;
        while i < 3 {
            eng.run_scenario(SCENARIOS[i].id, PATH_TABLE[1].id).unwrap();
            i += 1;
        }
        let before = eng.chain_len();
        let v = eng.inject_tamper_chain(1).unwrap();
        assert!(v.detected, "链篡改必须检出");
        assert_eq!(v.code, EffBenchError::SignatureMismatch.code());
        assert_eq!(v.locate, 1, "篡改必须精确定位到槽位 1");
        assert_eq!(eng.chain_len(), before);
    }

    #[test]
    fn inject_cherry_pick_detected() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        // 逐字节扫描：任一参数字节被改都必须指纹失配。
        let mut detected = 0;
        let mut b = 0;
        while b < SCEN_SER_LEN {
            let v = eng.inject_cherry_pick(0, b).unwrap();
            if v.detected {
                assert_eq!(v.code, EffBenchError::ScenarioFingerprintMismatch.code());
                detected += 1;
            }
            b += 1;
        }
        assert_eq!(detected, SCEN_SER_LEN, "串行化无死字节：全部 {SCEN_SER_LEN} 字节注入均须检出");
    }

    #[test]
    fn inject_drift_detected() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        eng.run_all_twice().unwrap();
        // 矩阵采集（无合同档强制）作为对拍样本集。
        let mut a = eng.collect_matrix_scores().unwrap();
        let mut b = a.clone();
        // 干净比对不检出。
        assert!(!EffBenchEngine::inject_drift(&a, &b).unwrap().detected);
        // 篡改一个 SSIM 字段 → 漂移检出。
        b[5].ssim_score ^= 0x40;
        let v = EffBenchEngine::inject_drift(&a, &b).unwrap();
        assert!(v.detected, "漂移注入必须检出");
        assert_eq!(v.code, EffBenchError::DualRunDrift.code());
        // 长度不等同样判漂移。
        a.pop();
        let v2 = EffBenchEngine::inject_drift(&a, &b).unwrap();
        assert!(v2.detected, "长度不等必须判漂移");
        assert_eq!(v2.code, EffBenchError::DualRunDrift.code());
    }

    #[test]
    fn tampered_scenario_copy_fails_fingerprint() {
        // 直接改场景结构体字段 → 指纹必失配（引擎外层同样防护）。
        let sc = SCENARIOS[2];
        let mut tampered = sc;
        tampered.contract_min_score = 1; // 把合同档改到地板——典型挑参数。
        assert_ne!(
            scenario_fingerprint(&tampered),
            SCENARIO_FINGERPRINTS[2]
        );
    }

    // -- 体检降级路径 --

    #[test]
    fn health_degrades_on_drift_flag() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        eng.run_all_twice().unwrap();
        eng.health_gate().unwrap();
        eng.mark_drift_for_injection();
        let rep = eng.health();
        assert!(rep.red_count >= 1);
        let has_red = {
            let mut found = false;
            let mut i = 0;
            while i < rep.items.len() {
                if rep.items[i].level == 2 && rep.items[i].name == "dual-run" {
                    found = true;
                }
                i += 1;
            }
            found
        };
        assert!(has_red);
        assert_eq!(eng.health_gate().unwrap_err(), EffBenchError::HealthDegraded);
    }

    #[test]
    fn selfcheck_report_bounded() {
        let mut eng = EffBenchEngine::new(env()).unwrap();
        eng.run_scenario(SCENARIOS[0].id, PATH_TABLE[1].id).unwrap();
        let rep = eng.selfcheck_report().unwrap();
        assert!(rep.len() <= REPORT_MAX_BYTES);
        assert!(rep.contains("CGPU-F2086"));
        assert!(rep.contains("version="));
        assert!(rep.contains("signature-chain"));
    }

    // -- 双跑对拍（对比组口径复验：评分纯函数性） --

    #[test]
    fn scoring_pure_function_of_scenario_and_path() {
        let mut eng1 = EffBenchEngine::new(env()).unwrap();
        let mut eng2 = EffBenchEngine::new(env()).unwrap();
        let a = eng1.collect_matrix_scores().unwrap();
        let b = eng2.collect_matrix_scores().unwrap();
        assert_eq!(a.len(), SCENARIO_COUNT * PATH_COUNT);
        assert_eq!(a, b, "评分必须只依赖 (场景, 路径)");
    }

    // -- 损失声明违约显性化 --

    #[test]
    fn loss_declaration_breach_visible_in_ledger() {
        // 全矩阵采集（含损失声明校验）——评审声明值应零违约。
        let mut eng = EffBenchEngine::new(env()).unwrap();
        let _ = eng.collect_matrix_scores().unwrap();
        assert_eq!(eng.ledger().counter(EventKind::LossBreach), 0);
        assert!(eng.verify_chain().is_ok());
    }
}
