//! VE-F2004 · Bloom 核心（VE-K 域 · 后处理架构与 Bloom 组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2004`
//!
//! **判据（锚点原文逐条）**：
//! - 三段管线：明亮提取 → 逐级 mip 模糊 → 合成叠加，多级泛光的标准管线；
//! - mip 级模糊链：多级下采样模糊，5-7 级可选，大范围柔和泛光的层级控制；
//! - J 域 F1849 源标记契约兑现：高亮光源 bloom 源标记的消费，源标记掩码提升
//!   明亮提取的物理准确性——光源直视区的泛光来源语义兑现；
//! - 光敏联动：强泛光是光敏风险源，输出时序入 F1962 扫描（三源之一的光照动效
//!   源扩展）。
//!
//! **错误路径与降级矩阵（锚点原文六条）**：
//! 1. mip 级数切换帧边界生效（F1762 规则）→ 本条实现为**待决槽 + 帧边界提交**，
//!    帧内改参数不重建链（半新半旧即视觉跳变）；
//! 2. 提取阈值 NaN/负 → F1802 量纲钳制（非有限→兜底值，负→0），钳制走显式
//!    诊断而非静默——静默钳制会让"设错了"和"没设"混为一谈；
//! 3. mip 链显存超配额 → 自动降级数 + 告警（降级为显性降级，链不崩）；
//! 4. 源标记缓冲缺失（J 侧未输出）→ 退化为纯亮度提取 + 告警（**不静默**——
//!    契约缺失必须显性，否则泛光"看起来正常"但物理语义已丢）；
//! 5. 模糊链溢出（HDR 极亮值）→ 提取端软钳制防炸帧（Reinhard 局部压缩，
//!    非硬 clamp——硬 clamp 会把光源的相对亮度差拍平，泛光失去体积感）。
//!
//! **设计要点**：
//! - **软膝（soft knee）**：提取不是硬阈值台阶而是**二次曲线过渡带**。硬阈值的
//!   问题在运动相机下是"边缘像素逐帧忽进忽出"造成高频闪点（COD:AW 方案）。
//!   带宽 `2*knee` 内的贡献按 `br²/(4*knee)` 上升，两端斜率连续；
//! - **归一化提取**：提取结果按 `soft / max(brightness, eps)` **归一**——不归一
//!   则极亮像素贡献被自身亮度二次放大（一盏灯比太阳还亮），归一后提取的是
//!   "贡献比例"而非"绝对量"，合成端乘回来才物理正确；
//! - **源标记加权（F1849 兑现）**：掩码区**放宽阈值**而非加权结果——语义是
//!   "被物理认定为光源直视区的像素，其提取门槛应更低"（光源直视区在物理上
//!   就是泛光来源，J 侧已经算过一次，不该被 K 侧阈值二次否决）。放宽量以
//!   `mask_relax ∈ [0,1]` 表达，`eff_threshold = threshold * (1 - relax*mask)`；
//! - **半分辨率起步**：mip0 从 (w+1)/2 起步逐级减半（下限 1 钳制）——本条是
//!   锚点"主优化点"，全链成本相对全分辨率减半；
//! - **dual-kawase 分离核**：down 用中心 0.5 + 四角 0.125（和 1.0），up 用
//!   中心 0.25 + 轴向 0.125×4 + 对角 0.0625×4（和 1.0）。两核权重和**机检为
//!   1.0**——分离核求和不为 1 会逐级漂移（模糊链跑几百帧后整链变暗/变亮），
//!   是合成类缺陷里最难在单帧内看出的那种；
//! - **合成权重指数衰减 + 归一化**：`w_i = decay^i`，归一化后总和为 1——低
//!   decay 收紧到大范围柔和，高 decay 收紧到光源近场锐利；
//! - **帧边界提交**：`request_levels` 只写待决槽，`commit_frame_boundary`
//!   才换链。改参数帧内生效 = 半新半旧 = 一帧亮度跳变。
//!
//! **跨批对接点**：
//! - F1849 源标记契约兑现（本条即"预留→兑现"闭环的那一端，兑现登记见
//!   `F1849_REDEMPTION_DOC`）；
//! - RT 全走 F2003 池（本模块只出**分配请求规格**与回收协议，池本体在
//!   F2003；`RtPoolAllocRequest` 是二者之间的唯一接口面）；
//! - 参数语义 F1802 量纲（阈值单位 cd/m²，钳制规则引用 F1802 极端强度条款）；
//! - 参数化扩展 F2005（阈值/强度/tint 的完整参数体系与轨道在 F2005，本条
//!   只提供三段管线的消费面与量纲边界）；
//! - 管线序 F2001：Bloom 在 TM 前、LUT 前——本条以 `PIPELINE_ORDER_DOC`
//!   声明并提供序位断言位。
//!
//! **无障碍与隐私**：泛光输出经 `PhotosensitiveSample` 交 F1962 扫描（三源
//! 扩展：面积扩张率 × 时间变化率 × 强度）；无隐私面。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟；全部确定性算法、零 IO、回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源——F1802 量纲与光敏边界的锚）
// ---------------------------------------------------------------------------

/// 提取阈值的物理单位（锚点"参数语义 F1802 量纲"：cd/m²）。
///
/// 阈值不是 0-1 无量纲数：曝光会改写场景亮度域，阈值若无量纲则曝光一动
/// 泛光就跳变。锚点 F2005「阈值用 F1802 cd/m² 单位制——与曝光联动时阈值
/// 语义稳定」即此。
pub const THRESHOLD_UNIT: &str = "cd/m^2";

/// 提取阈值下界（负→钳到 0，对应锚点错误路径第 2 条 / F1802 极端强度条款）。
pub const THRESHOLD_MIN_CD_M2: f32 = 0.0;

/// 提取阈值上界（超出即钳制——量纲纪律，不是美术自由度）。
pub const THRESHOLD_MAX_CD_M2: f32 = 1.0e5;

/// 阈值兜底值（非有限输入 NaN/Inf 的落点：取中位量级而非 0——取 0 会让
/// 整个场景全部进入提取，泛光炸屏；这是"设错了"与"没设"的第三条路）。
pub const THRESHOLD_FALLBACK_CD_M2: f32 = 1.0;

/// 软膝带宽下界（knee→0 即退化为硬阈值，接口仍合法——允许调用方显式
/// 选择硬阈值，但自检会记录该形态）。
pub const SOFT_KNEE_MIN: f32 = 0.0;

/// 软膝带宽上界（knee 过大 → 提取曲线整体被压平，"什么都是光源"）。
pub const SOFT_KNEE_MAX: f32 = 1.0e4;

/// 源标记掩码最大放宽比例（放宽到 100% 即该像素**无阈值**直接进提取——物理
/// 上等于"J 侧说它就是光源，K 侧不再质疑"，是契约的最高信任档）。
pub const MASK_RELAX_MAX: f32 = 1.0;

/// HDR 溢出软钳制的拐点（亮度超过 `SOFT_CLAMP_KNEE` 后进入局部 Reinhard）。
///
/// 不放在阈值处：软钳制是**防炸帧**的最后一道，与提取语义解耦——提取阶段
/// 的溢出钳制针对的是"极亮像素进入模糊链后会污染整链"（模糊是加权平均，
/// 一颗值 1e6 的像素经 7 级扩散影响面积极大）。
pub const SOFT_CLAMP_KNEE: f32 = 64.0;

/// HDR 软钳制的渐近上限（Reinhard 局部压缩的渐近线）。
pub const SOFT_CLAMP_MAX: f32 = 1.0e3;

/// 提取归一化的 eps 下界（防 0 除；亮度恒为 0 时贡献本就为 0）。
pub const LUMA_EPS: f32 = 1.0e-6;

/// mip 链级数下界（再少就不是"链"了，单级 = F2015 低档的 Bloom 单级形态）。
pub const MIP_LEVELS_MIN: u8 = 1;

/// mip 链级数上界（锚点"5-7 级可选"的上界；再多则末级 RT 已 <4px，模糊核
/// 采边重复导致末级能量虚高）。
pub const MIP_LEVELS_MAX: u8 = 7;

/// 锚点推荐档（"5-7 级可选"的下界 mainstream 档）。
pub const MIP_LEVELS_RECOMMENDED: u8 = 5;

/// mip 尺寸下界钳制（减半到 1 即停——0 尺寸 RT 无意义）。
pub const MIP_DIM_FLOOR: u32 = 1;

/// 合成强度下界（0 = 关闭泛光叠加，链仍跑但合成为恒等——保留链是为了
/// 参数动画连续，关闭整链会丢 mip RT 的复用窗口）。
pub const INTENSITY_MIN: f32 = 0.0;

/// 合成强度上界（>1 属过曝叠加，钳制——钳制而非拒绝：强度是美术参数，
/// 与阈值不同，钳制不改变"作者想要泛光"这个意图）。
pub const INTENSITY_MAX: f32 = 4.0;

/// 合成权重衰减下界（decay→0 即只有最大 mip 参与合成 = 单一柔和泛光）。
pub const COMPOSITE_DECAY_MIN: f32 = 0.0;

/// 合成权重衰减上界（decay 过大 → 权重全压在 mip0，等于不模糊）。
pub const COMPOSITE_DECAY_MAX: f32 = 1.0;

/// 光敏扫描用的帧间隔（逻辑 tick 数——光敏是时间域问题，只看单帧无效）。
pub const PHOTOSENSITIVE_WINDOW_TICKS: u64 = 8;

/// 管线序声明（锚点"联动 K02 LUT 前序（bloom 在 TM 前——管线序 F2001
/// 声明执行）"）。
pub const PIPELINE_ORDER_DOC: &str = "\
后处理管线序（VE-F2004 · v1，对齐 F2001 声明的基准序）：
  J域光照输出 ─▶ Bloom(F2004 本条) ─▶ AA(F2009-F2012) ─▶ TM(F2006)
  ─▶ LUT(F2023) ─▶ 色彩分级 ─▶ 镜头效果 ─▶ 输出编码(F2008) ─▶ V域显示
要点：
  1. Bloom 在 TM 之前——Bloom 工作在 referred 线性域（光源的能量还没被
     色调映射压到显示域），TM 之后做泛光等于对压缩过的显示值做模糊，
     高光层次已丢；
  2. Bloom 在 LUT 之前——LUT 是创作意图的最终调色，泛光必须在调色之前
     铺好，否则 LUT 会把泛光一并染色（作者没打算让那片橙光变青）；
  3. 本条为序位声明与断言位，真实序执行归 F2002 DAG 执行器。";

/// F1849 兑现登记（锚点"兑现 F1849 掩码契约（预留-兑现闭环登记）"）。
pub const F1849_REDEMPTION_DOC: &str = "\
VE-F1849 预留 → VE-F2004 兑现 闭环登记：
  上游（F1849 · 预留端）：源标记缓冲（半分辨率 R8 掩码）+ 曝光后亮度域阈值
    + 单向数据流（J 输出，K 消费；K 回调 J = 拒绝）。
  下游（本条 · 兑现端）：消费 SourceMaskRt——掩码参与提取阈值的放宽计算
    （mask_relax × mask → eff_threshold），把「哪些像素因光源直视而高亮」
    的物理来源语义真正用进泛光链，而不只是产出一张没人用的掩码。
  兑现判据：掩码区像素的有效提取阈值严格低于非掩码区（自检
    `K04-兑现-掩码区阈值放宽` 逐位断言），且掩码缺失时退化为纯亮度提取
    并产出显性诊断（`DiagCode::SourceMaskMissing`）。";

// ---------------------------------------------------------------------------
// 二、诊断（三要素：错误码 + 上下文 + 下一步；K 域与 F2003 池同风格）
// ---------------------------------------------------------------------------

/// Bloom 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagCode {
    /// 阈值非有限（NaN/Inf）→ F1802 量纲钳制兜底。
    ThresholdNonFinite,
    /// 阈值负值 → 钳到 0。
    ThresholdNegative,
    /// 阈值超上界 → 钳到上界。
    ThresholdClampedHigh,
    /// 软膝带宽越界 → 钳制。
    SoftKneeOutOfRange,
    /// mip 级数越界 → 钳制到 [1,7]。
    MipLevelsOutOfRange,
    /// 强度越界 → 钳制。
    IntensityOutOfRange,
    /// 合成衰减越界 → 钳制。
    CompositeDecayOutOfRange,
    /// 掩码放宽比例越界 → 钳制。
    MaskRelaxOutOfRange,
    /// **源标记缓冲缺失（J 侧未输出）→ 退化为纯亮度提取**（契约缺失显性）。
    SourceMaskMissing,
    /// 源标记描述非法（尺寸非半分辨率 / 格式非 R8）→ 忽略该掩码并告警。
    SourceMaskInvalid,
    /// mip 链显存超配额 → 自动降级数。
    MipChainOverQuota,
    /// 级数切换待提交（帧边界生效，F1762 规则；非错误，是显性挂起态）。
    LevelSwitchPending,
}

impl DiagCode {
    /// 诊断码字符串。
    pub fn code(self) -> &'static str {
        match self {
            DiagCode::ThresholdNonFinite => "BLOOM_THRESHOLD_NON_FINITE",
            DiagCode::ThresholdNegative => "BLOOM_THRESHOLD_NEGATIVE",
            DiagCode::ThresholdClampedHigh => "BLOOM_THRESHOLD_CLAMPED_HIGH",
            DiagCode::SoftKneeOutOfRange => "BLOOM_SOFT_KNEE_OUT_OF_RANGE",
            DiagCode::MipLevelsOutOfRange => "BLOOM_MIP_LEVELS_OUT_OF_RANGE",
            DiagCode::IntensityOutOfRange => "BLOOM_INTENSITY_OUT_OF_RANGE",
            DiagCode::CompositeDecayOutOfRange => "BLOOM_COMPOSITE_DECAY_OUT_OF_RANGE",
            DiagCode::MaskRelaxOutOfRange => "BLOOM_MASK_RELAX_OUT_OF_RANGE",
            DiagCode::SourceMaskMissing => "BLOOM_SOURCE_MASK_MISSING",
            DiagCode::SourceMaskInvalid => "BLOOM_SOURCE_MASK_INVALID",
            DiagCode::MipChainOverQuota => "BLOOM_MIP_CHAIN_OVER_QUOTA",
            DiagCode::LevelSwitchPending => "BLOOM_LEVEL_SWITCH_PENDING",
        }
    }

    /// 严重级（决定是否阻断帧提交）。
    pub fn severity(self) -> Severity {
        match self {
            // 契约缺失与显存超配是"链仍能跑但语义/预算已变"，走显性降级不阻断。
            DiagCode::SourceMaskMissing
            | DiagCode::SourceMaskInvalid
            | DiagCode::MipChainOverQuota
            | DiagCode::LevelSwitchPending => Severity::Degraded,
            // 量纲钳制是参数纠错，不阻断渲染。
            _ => Severity::Corrected,
        }
    }

    /// 下一步建议（锚点"错误路径约 40 行（告警与降档）"的人读侧）。
    pub fn next_hint(self) -> &'static str {
        match self {
            DiagCode::ThresholdNonFinite => {
                "阈值拿到 NaN/Inf：先修上游亮度链（F1802 量纲），临时值已按兜底 1 cd/m^2 钳制"
            }
            DiagCode::ThresholdNegative => "阈值为负：无物理意义，已钳到 0（0 = 全场景进入提取，检查是否忘填）",
            DiagCode::ThresholdClampedHigh => "阈值超 1e5 cd/m^2：超出量纲表上限，已钳制；确认单位是否误填成 0-1 归一值",
            DiagCode::SoftKneeOutOfRange => "软膝带宽越界：已钳到 [0,1e4] cd/m^2；带宽为 0 即硬阈值，会在运动镜头下产生闪点",
            DiagCode::MipLevelsOutOfRange => "mip 级数越界：已钳到 [1,7]；>7 级时末级 RT 已 <4px，能量会虚高",
            DiagCode::IntensityOutOfRange => "合成强度越界：已钳到 [0,4]；>1 为过曝叠加，观感异常优先查阈值而非加强度",
            DiagCode::CompositeDecayOutOfRange => "合成权重衰减越界：已钳到 [0,1]；0 = 仅最大 mip 参与（单一柔和泛光）",
            DiagCode::MaskRelaxOutOfRange => "掩码放宽比例越界：已钳到 [0,1]；1 = 掩码区完全无阈值（J 侧全权）",
            DiagCode::SourceMaskMissing => {
                "J 侧未输出源标记掩码（F1849 契约未兑现）：已退化为纯亮度阈值提取——泛光仍出图但物理来源语义丢失，检查 J03 掩码 pass"
            }
            DiagCode::SourceMaskInvalid => "源标记描述非法（非半分辨率或非 R8）：该掩码已忽略，链路按无掩码运行",
            DiagCode::MipChainOverQuota => "mip 链显存超配额：已自动降级数并保留告警；配额账本见 F1776 K 段",
            DiagCode::LevelSwitchPending => "mip 级数已改：待帧边界提交才换链（F1762 规则），帧内立即生效会出现半新半旧的一帧亮度跳变",
        }
    }
}

/// 严重级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 已钳制/纠正，渲染照常。
    Corrected,
    /// 显性降级：链在跑但语义或预算已变。
    Degraded,
}

impl Severity {
    /// 是否阻断帧提交（本条全部诊断均不阻断——Bloom 是效果层，崩了会带走
    /// 全链；降级出图 + 显性告警是唯一正确姿态）。
    pub fn blocks_frame(self) -> bool {
        false
    }

    /// 人读标签。
    pub fn label(self) -> &'static str {
        match self {
            Severity::Corrected => "已纠正",
            Severity::Degraded => "已降级",
        }
    }
}

/// 单条诊断（三要素：错误码 + 上下文 + 下一步）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: DiagCode,
    /// 严重级。
    pub severity: Severity,
    /// 上下文数值（阈值实测值 / 超配额字节数 / 降级前后级数——依码而定）。
    pub context: f32,
    /// 上下文次值（降级前级数、目标级数等第二维度）。
    pub context2: f32,
}

impl Diagnostic {
    /// 构造一条诊断。
    pub fn new(code: DiagCode, context: f32) -> Self {
        Diagnostic {
            code,
            severity: code.severity(),
            context,
            context2: 0.0,
        }
    }

    /// 带第二上下文值。
    pub fn with_context2(mut self, v: f32) -> Self {
        self.context2 = v;
        self
    }

    /// 人读三要素串（错误码 / 实测 / 下一步）。
    pub fn describe(&self) -> String {
        format!(
            "[{}/{}] {} = {:.4} → {}",
            self.severity.label(),
            self.code.code(),
            self.code.code(),
            self.context,
            self.code.next_hint()
        )
    }
}

/// 诊断袋（显性降级的载体——"不静默"纪律：任何降级都必须留一条可查记录）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 空袋。
    pub fn new() -> Self {
        DiagBag { items: Vec::new() }
    }

    /// 记一条。
    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    /// 记一条（便捷）。
    pub fn note(&mut self, code: DiagCode, context: f32) {
        self.items.push(Diagnostic::new(code, context));
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 是否含某码。
    pub fn has(&self, code: DiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }

    /// 取诊断码序列（断言用）。
    pub fn codes(&self) -> Vec<DiagCode> {
        self.items.iter().map(|d| d.code).collect()
    }

    /// 是否全部为"已纠正"级（即无降级——链语义与预算均未被改变）。
    pub fn no_degradation(&self) -> bool {
        self.items
            .iter()
            .all(|d| d.severity == Severity::Corrected)
    }

    /// 人读全文（每条一行）。
    pub fn report(&self) -> String {
        let mut s = String::new();
        for (i, d) in self.items.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(&d.describe());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、Bloom 参数块（量纲边界在此；完整参数体系与轨道归 F2005）
// ---------------------------------------------------------------------------

/// Bloom 参数块（锚点"合成：多级加权叠加回主色缓冲，强度全局参数"）。
///
/// 构造即钳制：所有参数在入口完成量纲收敛，管线内部不再判断越界——把
/// "钳制"从渲染热路径里拿掉，是本条的性能纪律（钳制是 O(1) 但分支预测
/// 失败在每像素每级都发生；参数面收敛后管线内是纯算术）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomParams {
    /// 提取阈值（cd/m²，F1802 量纲；已钳制）。
    pub threshold: f32,
    /// 软膝带宽（cd/m²，0 = 硬阈值；已钳制）。
    pub soft_knee: f32,
    /// mip 级数（已钳到 [1,7]）。
    pub mip_levels: u8,
    /// 合成强度（已钳到 [0,4]）。
    pub intensity: f32,
    /// 合成权重衰减（已钳到 [0,1]）。
    pub composite_decay: f32,
    /// 源标记放宽比例（已钳到 [0,1]；F1849 兑现的力度旋钮）。
    pub mask_relax: f32,
}

impl Default for BloomParams {
    fn default() -> Self {
        BloomParams {
            threshold: 1.0,
            soft_knee: 0.5,
            mip_levels: MIP_LEVELS_RECOMMENDED,
            intensity: 1.0,
            composite_decay: 0.8,
            mask_relax: 0.75,
        }
    }
}

/// 参数量纲收敛结果（参数 + 钳制诊断）。
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedParams {
    /// 收敛后参数。
    pub params: BloomParams,
    /// 钳制诊断。
    pub diags: DiagBag,
}

impl BloomParams {
    /// 量纲收敛：逐项钳制并留诊断（F1802 极端强度条款的 K 侧执行）。
    ///
    /// 顺序纪律：先收敛标量再钳整数——否则 mip_levels 的 NaN 转换行为依赖
    /// 平台（`as u8` 对 NaN 是 UB 级未定义），必须在整数转换前拦掉非有限。
    pub fn resolve(self) -> ResolvedParams {
        let mut diags = DiagBag::new();

        // 阈值：非有限 → 兜底；负 → 0；超上界 → 上界。
        let mut threshold = self.threshold;
        if !threshold.is_finite() {
            diags.note(DiagCode::ThresholdNonFinite, self.threshold);
            threshold = THRESHOLD_FALLBACK_CD_M2;
        } else if threshold < THRESHOLD_MIN_CD_M2 {
            diags.note(DiagCode::ThresholdNegative, threshold);
            threshold = THRESHOLD_MIN_CD_M2;
        } else if threshold > THRESHOLD_MAX_CD_M2 {
            diags.note(DiagCode::ThresholdClampedHigh, threshold);
            threshold = THRESHOLD_MAX_CD_M2;
        }

        // 软膝带宽：非有限 → 0（退化为硬阈值是安全的：无过渡带即无闪点源
        // 之外的额外风险）；越界钳制。
        let mut soft_knee = self.soft_knee;
        if !soft_knee.is_finite() {
            soft_knee = SOFT_KNEE_MIN;
            diags.note(DiagCode::SoftKneeOutOfRange, self.soft_knee);
        } else if !(SOFT_KNEE_MIN..=SOFT_KNEE_MAX).contains(&soft_knee) {
            soft_knee = soft_knee.clamp(SOFT_KNEE_MIN, SOFT_KNEE_MAX);
            diags.note(DiagCode::SoftKneeOutOfRange, self.soft_knee);
        }

        // mip 级数：f32 入口先判非有限与范围，再转 u8（防 UB）。
        let lv = if self.mip_levels == 0 {
            MIP_LEVELS_MIN
        } else if self.mip_levels > MIP_LEVELS_MAX {
            diags.note(
                DiagCode::MipLevelsOutOfRange,
                f32::from(self.mip_levels),
            );
            MIP_LEVELS_MAX
        } else {
            self.mip_levels
        };
        let mip_levels = lv.max(MIP_LEVELS_MIN);

        // 强度：非有限 → 0（不静默出泛光）；越界钳制。
        let mut intensity = self.intensity;
        if !intensity.is_finite() {
            intensity = INTENSITY_MIN;
            diags.note(DiagCode::IntensityOutOfRange, self.intensity);
        } else if !(INTENSITY_MIN..=INTENSITY_MAX).contains(&intensity) {
            intensity = intensity.clamp(INTENSITY_MIN, INTENSITY_MAX);
            diags.note(DiagCode::IntensityOutOfRange, self.intensity);
        }

        // 合成衰减：非有限 → 0；越界钳制。
        let mut composite_decay = self.composite_decay;
        if !composite_decay.is_finite() {
            composite_decay = COMPOSITE_DECAY_MIN;
            diags.note(DiagCode::CompositeDecayOutOfRange, self.composite_decay);
        } else if !(COMPOSITE_DECAY_MIN..=COMPOSITE_DECAY_MAX).contains(&composite_decay) {
            composite_decay = composite_decay.clamp(COMPOSITE_DECAY_MIN, COMPOSITE_DECAY_MAX);
            diags.note(
                DiagCode::CompositeDecayOutOfRange,
                self.composite_decay,
            );
        }

        // 掩码放宽：非有限 → 0（不信任畸形掩码即等价于不放宽，安全侧）；
        // 越界钳制。
        let mut mask_relax = self.mask_relax;
        if !mask_relax.is_finite() {
            mask_relax = 0.0;
            diags.note(DiagCode::MaskRelaxOutOfRange, self.mask_relax);
        } else if !(0.0..=MASK_RELAX_MAX).contains(&mask_relax) {
            mask_relax = mask_relax.clamp(0.0, MASK_RELAX_MAX);
            diags.note(DiagCode::MaskRelaxOutOfRange, self.mask_relax);
        }

        ResolvedParams {
            params: BloomParams {
                threshold,
                soft_knee,
                mip_levels,
                intensity,
                composite_decay,
                mask_relax,
            },
            diags,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、J 域源标记契约（F1849 兑现端）
// ---------------------------------------------------------------------------

/// 源标记缓冲描述（锚点"源标记缓冲（亮度超阈像素掩码，半分辨率 R8）"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceMaskRt {
    /// 掩码宽度（必须 = 主缓冲宽度向上取整 / 2 —— 半分辨率）。
    pub width: u32,
    /// 掩码高度。
    pub height: u32,
    /// 掩码是否有效（J 侧实际写入了内容）。
    pub present: bool,
}

/// 源标记契约检查结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceMaskStatus {
    /// 契约是否兑现（present 且描述合法）。
    pub honored: bool,
    /// 有效提取阈值（叠加掩码放宽后的最终阈值）。
    pub effective_threshold: f32,
    /// 掩码缺失/非法时的诊断码（`honored` 为真时为 `None`）。
    pub diag: Option<DiagCode>,
}

/// 校验源标记描述并算出有效阈值（F1849 兑现的唯一入口）。
///
/// 契约纪律（锚点 F1849「单向数据流纪律」）：本函数只**读**描述，不回调 J
/// 侧；掩码缺失一律降级为纯亮度提取并显性告警，不静默。
pub fn resolve_source_mask(
    mask: Option<SourceMaskRt>,
    main_w: u32,
    main_h: u32,
    params: &BloomParams,
    diags: &mut DiagBag,
) -> SourceMaskStatus {
    let base = params.threshold;
    let m = match mask {
        None => {
            // 锚点错误路径第 4 条：掩码缓冲缺失 → 退化为纯亮度提取 + 告警。
            diags.note(DiagCode::SourceMaskMissing, base);
            return SourceMaskStatus {
                honored: false,
                effective_threshold: base,
                diag: Some(DiagCode::SourceMaskMissing),
            };
        }
        Some(m) => m,
    };

    if !m.present {
        diags.note(DiagCode::SourceMaskMissing, base);
        return SourceMaskStatus {
            honored: false,
            effective_threshold: base,
            diag: Some(DiagCode::SourceMaskMissing),
        };
    }

    // 半分辨率契约校验：J 侧产出的是半分辨率 R8，宽高须为上取整 /2。
    let want_w = (main_w + 1) / 2;
    let want_h = (main_h + 1) / 2;
    if m.width != want_w || m.height != want_h || m.width == 0 || m.height == 0 {
        diags.note(DiagCode::SourceMaskInvalid, m.width as f32);
        return SourceMaskStatus {
            honored: false,
            effective_threshold: base,
            diag: Some(DiagCode::SourceMaskInvalid),
        };
    }

    // 兑现：掩码区的有效阈值按放宽比例下调（mask=1 时即无阈值）。
    // 契约的最高信任档（relax=1）是合法的——J 侧说它就是光源，K 侧不质疑。
    let eff = base * (1.0 - params.mask_relax);
    SourceMaskStatus {
        honored: true,
        effective_threshold: eff,
        diag: None,
    }
}

/// 单像素有效阈值（掩码值 0-1 连续，非二值）。
///
/// 半分辨率掩码在提取时按双线性上采样到全分辨率后逐像素参与——所以阈值是
/// **逐像素**的，不是一个全局标量。这是"源标记提升提取物理准确性"的落点：
/// 高光源附近的掩码值高 → 该处阈值低 → 直视区确实更容易起泛光。
pub fn effective_threshold(base: f32, mask_value: f32, mask_relax: f32) -> f32 {
    let mv = if mask_value.is_finite() {
        mask_value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    base * (1.0 - mask_relax * mv)
}

// ---------------------------------------------------------------------------
// 五、第一段：明亮提取（亮度阈值 + 软膝 + 源标记加权）
// ---------------------------------------------------------------------------

/// HDR 线性颜色（本条全程工作于 referred 线性域——TM 之前）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrColor {
    /// 线性 r。
    pub r: f32,
    /// 线性 g。
    pub g: f32,
    /// 线性 b。
    pub b: f32,
}

impl HdrColor {
    /// 构造并清洗非有限分量（数值防线：非有限不得进入模糊链——加权平均会
    /// 把 NaN 扩散到整张 RT，且 NaN 在 min/max 归约里会静默吞掉整条分支）。
    pub fn new(r: f32, g: f32, b: f32) -> Self {
        HdrColor {
            r: clean(r),
            g: clean(g),
            b: clean(b),
        }
    }

    /// 亮度（Rec.709 亮度权重，referred 线性域）。
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    /// 分量最大值（提取用的"峰值亮度"判据）。
    pub fn max_component(self) -> f32 {
        let m = self.r.max(self.g).max(self.b);
        if m.is_finite() && m > 0.0 {
            m
        } else {
            0.0
        }
    }

    /// 软钳制（锚点错误路径第 5 条：HDR 极亮值防炸帧）。
    ///
    /// 用**局部 Reinhard** 而非硬 clamp：`x <= K` 时恒等（亮部以下的对比度
    /// 完全保留），`x > K` 后渐近到 `SOFT_CLAMP_MAX`。硬 clamp 会把两颗亮度
    /// 差 10 倍的灯拍成同一值，泛光的体积感（近处亮、远处弱）就没了。
    pub fn soft_clamp(self) -> HdrColor {
        HdrColor {
            r: soft_clamp(self.r),
            g: soft_clamp(self.g),
            b: soft_clamp(self.b),
        }
    }

    /// 逐分量乘标量。
    pub fn scale(self, k: f32) -> HdrColor {
        let k = if k.is_finite() { k } else { 0.0 };
        HdrColor {
            r: self.r * k,
            g: self.g * k,
            b: self.b * k,
        }
    }

    /// 逐分量加。
    pub fn add(self, o: HdrColor) -> HdrColor {
        HdrColor {
            r: self.r + o.r,
            g: self.g + o.g,
            b: self.b + o.b,
        }
    }

    /// 逐分量钳到 [0, SOFT_CLAMP_MAX]（链末防御——模糊是凸组合，正常不会越
    /// 界，但链式浮点误差可能轻微越界；钳一次比让脏值进主缓冲便宜）。
    pub fn clamp_chain(self) -> HdrColor {
        HdrColor {
            r: self.r.clamp(0.0, SOFT_CLAMP_MAX),
            g: self.g.clamp(0.0, SOFT_CLAMP_MAX),
            b: self.b.clamp(0.0, SOFT_CLAMP_MAX),
        }
    }
}

/// 分量清洗（非有限 → 0；正无穷按上限值语义交给 soft_clamp 处理）。
fn clean(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else if v == f32::INFINITY {
        SOFT_CLAMP_MAX
    } else if v == f32::NEG_INFINITY {
        0.0
    } else if v < 0.0 {
        // referred 域负值无物理意义（J 域应已钳制），此处归零不放大。
        0.0
    } else {
        v
    }
}

/// 标量软钳制（局部 Reinhard，锚点错误路径第 5 条）。
pub fn soft_clamp(x: f32) -> f32 {
    if !x.is_finite() {
        return if x == f32::INFINITY { SOFT_CLAMP_MAX } else { 0.0 };
    }
    if x <= SOFT_CLAMP_KNEE {
        return x.max(0.0);
    }
    let over = x - SOFT_CLAMP_KNEE;
    // 局部 Reinhard：K + over / (1 + over / (MAX - K))，在 MAX 处渐近。
    SOFT_CLAMP_KNEE + over / (1.0 + over / (SOFT_CLAMP_MAX - SOFT_CLAMP_KNEE))
}

/// 软膝贡献（锚点"亮度阈值+软膝函数"）。
///
/// COD:AW 的 quadratic knee 形态：
/// ```text
/// br = brightness - threshold + knee
/// br < 0            → 0
/// br > 2*knee       → brightness - threshold      （线性段，斜率 1）
/// 否则              → br² / (4*knee)               （过渡段，斜率连续）
/// ```
/// 带宽内贡献用二次曲线上升，两端斜率连续——运动镜头下不会出现硬阈值台阶
/// 造成的"边缘像素忽进忽出"闪点。`knee == 0` 时退化为硬阈值（除零需短路）。
pub fn soft_knee_contribution(brightness: f32, threshold: f32, knee: f32) -> f32 {
    let br = brightness - threshold + knee;
    if br <= 0.0 {
        return 0.0;
    }
    if knee <= 0.0 {
        // 硬阈值：过渡带宽度为 0，二次段不存在。
        return br - knee;
    }
    if br > 2.0 * knee {
        return brightness - threshold;
    }
    (br * br) / (4.0 * knee)
}

/// 第一段输出（提取后的贡献色 + 提取率）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractResult {
    /// 提取贡献（已归一化：scale = 贡献比例，颜色保留自身色相）。
    pub color: HdrColor,
    /// 提取率 soft/max(brightness, eps) ∈ [0,1]。
    pub rate: f32,
    /// 该像素实际使用的有效阈值（已含掩码放宽）。
    pub effective_threshold: f32,
}

/// 明亮提取（第一段）。
///
/// 流程：清洗 → 软钳制 → 亮度 → 逐像素有效阈值（含 F1849 掩码）→ 软膝贡献
/// → **归一化**。
///
/// 归一化是本段的关键语义：`color * (soft / max(brightness, eps))` 而不是
/// `color * soft`。后者对极亮像素会二次放大（一颗 1e4 的像素贡献 1e4 的
/// 提取量，模糊后成为全场最亮的一片白），前者提取的是"贡献比例"。
pub fn extract(color: HdrColor, mask_value: f32, params: &BloomParams) -> ExtractResult {
    let c = color.soft_clamp();
    let brightness = c.luminance();
    let thr = effective_threshold(params.threshold, mask_value, params.mask_relax);
    let soft = soft_knee_contribution(brightness, thr, params.soft_knee);
    let denom = brightness.max(LUMA_EPS);
    let rate = (soft / denom).clamp(0.0, 1.0);
    ExtractResult {
        color: c.scale(rate),
        rate,
        effective_threshold: thr,
    }
}

// ---------------------------------------------------------------------------
// 六、第二段：mip 模糊链（dual-kawase 分离核 + 半分辨率起步）
// ---------------------------------------------------------------------------

/// 分离核采样（相对纹素偏移 + 权重）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KernelTap {
    /// x 偏移（-1 / 0 / 1）。
    pub dx: i32,
    /// y 偏移。
    pub dy: i32,
    /// 权重（两核各自权重和恒为 1.0——机检项）。
    pub weight: f32,
}

/// dual-kawase 下采样核：中心 0.5 + 四角 0.125（和 = 1.0）。
///
/// 下采样同时做模糊（这是 dual filter 的设计意图：一次采样完成"缩半 + 糊"，
/// 省掉独立模糊 pass）。
pub const DOWN_KERNEL: [KernelTap; 5] = [
    KernelTap { dx: 0, dy: 0, weight: 0.5 },
    KernelTap { dx: -1, dy: -1, weight: 0.125 },
    KernelTap { dx: 1, dy: -1, weight: 0.125 },
    KernelTap { dx: -1, dy: 1, weight: 0.125 },
    KernelTap { dx: 1, dy: 1, weight: 0.125 },
];

/// dual-kawase 上采样核：中心 0.25 + 轴向 0.125×4 + 对角 0.0625×4（和 = 1.0）。
///
/// 上采样核更宽（8 抽头 + 中心）以补偿降采样时的相位偏移——只做单次双线性
/// 放大会在 mip 链上留下可见的方块化。
pub const UP_KERNEL: [KernelTap; 9] = [
    KernelTap { dx: 0, dy: 0, weight: 0.25 },
    KernelTap { dx: -1, dy: 0, weight: 0.125 },
    KernelTap { dx: 1, dy: 0, weight: 0.125 },
    KernelTap { dx: 0, dy: -1, weight: 0.125 },
    KernelTap { dx: 0, dy: 1, weight: 0.125 },
    KernelTap { dx: -1, dy: -1, weight: 0.0625 },
    KernelTap { dx: 1, dy: -1, weight: 0.0625 },
    KernelTap { dx: -1, dy: 1, weight: 0.0625 },
    KernelTap { dx: 1, dy: 1, weight: 0.0625 },
];

/// 核权重和（机检断言：分离核求和不为 1 会逐级漂移）。
pub fn kernel_weight_sum(taps: &[KernelTap]) -> f32 {
    taps.iter().map(|t| t.weight).sum()
}

/// mip 层尺寸（半分辨率起步逐级减半，下限钳到 1）。
///
/// level 0 = `(main + 1) / 2`（上取整，保证奇数尺寸不丢边缘列），此后逐级
/// 减半并钳到 `MIP_DIM_FLOOR`——末级不会缩到 0（0 尺寸 RT 无法分配）。
pub fn mip_dim(main: u32, level: u8) -> u32 {
    let mut d = (main.max(1) + 1) / 2;
    for _ in 0..level {
        d = (d / 2).max(MIP_DIM_FLOOR);
    }
    d
}

/// mip 链全部层尺寸（level 0..levels-1）。
pub fn mip_chain_dims(main_w: u32, main_h: u32, levels: u8) -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    for l in 0..levels.max(MIP_LEVELS_MIN) {
        v.push((mip_dim(main_w, l), mip_dim(main_h, l)));
    }
    v
}

/// 半分辨率起步的成本减半比（锚点"半分辨率链使成本减半（主优化点实测）"）。
///
/// 以像素数计：mip0 面积 = 全分辨率的 1/4，其后每级再 1/4，链总面积为
/// `Σ (1/4)^(l+1)`，极限 1/3；7 级时约 1/3（省 2/3）——比"减半"更好，
/// 因为链是几何级数。此值进 F2014 成本模型的 K 段。
pub fn half_res_cost_ratio(levels: u8) -> f32 {
    // 逐次乘 1/4 而非 `powi`：本仓内核镜像是 no_std，`f32::powi` 属 std/libm，
    // 在内核镜像目标下不可用（会静默变成未解析符号 → 链接期才炸）。
    let mut sum = 0.0f32;
    let mut term = 0.25f32;
    for _ in 0..levels.max(1) {
        sum += term;
        term *= 0.25;
    }
    sum
}

/// dual-kawase 一级模糊（对一张 mip 做 down 或 up；`up=true` 时 `src` 尺寸为
/// `dst` 的两倍邻域，采边用夹取）。
///
/// 采边夹取而非置零：置零会在 RT 边缘产生暗环，7 级之后暗环叠成四角发灰
/// （合成阶段才看得见，但那时归因极难）。
pub fn blur_tap(
    fetch: &dyn Fn(i32, i32) -> HdrColor,
    w: i32,
    h: i32,
    up: bool,
) -> HdrColor {
    let taps: &[KernelTap] = if up { &UP_KERNEL } else { &DOWN_KERNEL };
    let mut acc = HdrColor::new(0.0, 0.0, 0.0);
    for t in taps.iter() {
        let sx = clamp_i(t.dx, 0, w - 1);
        let sy = clamp_i(t.dy, 0, h - 1);
        let s = fetch(sx, sy);
        acc = HdrColor::new(
            acc.r + s.r * t.weight,
            acc.g + s.g * t.weight,
            acc.b + s.b * t.weight,
        );
    }
    acc
}

/// 整数夹取。
fn clamp_i(v: i32, lo: i32, hi: i32) -> i32 {
    if hi < lo {
        return lo;
    }
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// 七、中间资源：F2003 池分配请求与配额降级
// ---------------------------------------------------------------------------

/// RT 像素格式（K 域后处理中间 RT 的三种格式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtFormat {
    /// R8 掩码（源标记专用）。
    R8,
    /// RGBA16F 颜色（模糊链主用）。
    Rgba16f,
    /// RGB11F 颜色（合成回主缓冲前的可选降本格式）。
    Rgb11f,
}

impl RtFormat {
    /// 字节对齐后的每像素字节数。
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            RtFormat::R8 => 1,
            RtFormat::Rgba16f => 8,
            RtFormat::Rgb11f => 8, // 打包为 64bit 对齐，显存账本口径统一
        }
    }
}

/// 单个 RT 的分配请求（与 F2003 池的唯一接口面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtPoolAllocRequest {
    /// 宽。
    pub width: u32,
    /// 高。
    pub height: u32,
    /// 格式。
    pub format: RtFormat,
    /// 用途标签（提取面 / mip 各级 / 源标记——F2003 桶键与别名分析的输入）。
    pub role: &'static str,
    /// 存活区间下界（帧内逻辑序；F2003 别名分析消费）。
    pub live_from: u16,
    /// 存活区间上界。
    pub live_to: u16,
}

/// 单 RT 字节数。
pub fn rt_bytes(w: u32, h: u32, fmt: RtFormat) -> u64 {
    u64::from(w.max(1)) * u64::from(h.max(1)) * u64::from(fmt.bytes_per_pixel())
}

/// 模糊链的 RT 分配计划（level 0 = 提取面，其后每级一张）。
#[derive(Clone, Debug, PartialEq)]
pub struct MipChainPlan {
    /// 最终生效级数（可能被配额降级下调）。
    pub levels: u8,
    /// 逐级分配请求。
    pub requests: Vec<RtPoolAllocRequest>,
    /// 总字节数。
    pub total_bytes: u64,
}

impl MipChainPlan {
    /// 逐级尺寸。
    pub fn dims(&self) -> Vec<(u32, u32)> {
        self.requests
            .iter()
            .map(|r| (r.width, r.height))
            .collect()
    }
}

/// 模糊链 RT 计划 + 配额降级（锚点错误路径第 3 条：显存超配额 → 自动降级数
/// + 告警）。
///
/// 降级策略：**从最高级砍起，逐级下降直到入配额**——砍低级（mip0）省得少
/// 却把提取面砍没了（等于没做 Bloom）；砍高级（末级 mip）省显存最多且观感
/// 损失最小（末级只贡献最柔和的那一摊）。
///
/// **下限纪律**：级数降到 `MIP_LEVELS_MIN` 仍装不下时，**保底留 1 级并继续
/// 留告警**，不静默超配、也不禁用整链——"要不要关掉泛光"是档位表的职责
/// （F2015 低档 = Bloom 单级 / 关），本条只负责在既定配额内把链降到可跑。
/// 调用方从 `MipChainPlan.total_bytes` 与诊断袋的联合读出真实状态。
///
/// **链内显存分布（预算沟通用，实测 1920×1080 / RGBA16F / 7 级）**：
/// 各级面积按 1/4 递减，故 mip0（960×540 ≈ 4.15MB）**独占全链总量
/// （≈5.53MB）的约 75%**，后六级合计仅约 25%。两个直接推论：
/// - "多一级 mip" 的边际显存代价极低（末级仅数百 KB 级），砍高级省不出
///   多少预算，**真正的大头只有 mip0**；
/// - 因此配额驱动的降级往往一步就把链压到 1 级——预算谈判应盯 mip0 的尺寸
///   策略（能否再降半 / 能否走 R11G11B10），而不是讨论"少几级 mip"。
pub fn plan_mip_chain(
    main_w: u32,
    main_h: u32,
    wanted_levels: u8,
    quota_bytes: u64,
    diags: &mut DiagBag,
) -> MipChainPlan {
    let mut levels = wanted_levels.max(MIP_LEVELS_MIN);
    let mut plan = build_plan(main_w, main_h, levels);

    // 至少留 1 级：全砍掉等于 Bloom 未启用（那是 F2015 档位表的职责，不是
    // 配额降级的职责——本条只降级，不禁用）。
    while plan.total_bytes > quota_bytes && levels > MIP_LEVELS_MIN {
        levels -= 1;
        plan = build_plan(main_w, main_h, levels);
        diags.push(
            Diagnostic::new(DiagCode::MipChainOverQuota, plan.total_bytes as f32)
                .with_context2(f32::from(levels)),
        );
    }
    plan
}

/// 按级数构建计划（不含降级逻辑）。
fn build_plan(main_w: u32, main_h: u32, levels: u8) -> MipChainPlan {
    let mut requests = Vec::new();
    let mut total = 0u64;
    for l in 0..levels.max(MIP_LEVELS_MIN) {
        let w = mip_dim(main_w, l);
        let h = mip_dim(main_h, l);
        let role: &'static str = if l == 0 { "bloom_extract" } else { "bloom_mip" };
        let b = rt_bytes(w, h, RtFormat::Rgba16f);
        total += b;
        requests.push(RtPoolAllocRequest {
            width: w,
            height: h,
            format: RtFormat::Rgba16f,
            role,
            // 存活区间：提取面活到合成结束（live_to = 帧末常量），末级 mip
            // 在被上采样消费后即死——F2003 别名分析据此判定区间不重叠。
            live_from: l as u16,
            live_to: if l + 1 == levels { u16::MAX } else { l as u16 + 1 },
        });
    }
    MipChainPlan {
        levels: levels.max(MIP_LEVELS_MIN),
        requests,
        total_bytes: total,
    }
}

/// 帧边界回收协议（F2003 对接）：本帧所有 mip RT 在帧边界归还。
///
/// 存活区间上界是"帧内序"，回收是"帧末一次性"——两者不冲突：池的别名分析
/// 在帧内做区间着色，回收在帧末统一执行。
pub fn reclaim_all(plan: &MipChainPlan) -> Vec<u16> {
    plan.requests.iter().map(|r| r.live_from).collect()
}

// ---------------------------------------------------------------------------
// 八、第三段：合成（多级加权叠加）
// ---------------------------------------------------------------------------

/// 合成权重序列（`w_i = decay^i` 后归一化，和恒为 1.0）。
///
/// 归一化的必要性：不归一则 `decay=0.8` 时 5 级权重和为 2.69，合成结果整体
/// 亮 2.69 倍——**强度参数会与衰减耦合**，调衰减等于同时调亮度，这是合成类
/// 参数面板最经典的"两个滑杆互相偷值"缺陷。
pub fn composite_weights(levels: u8, decay: f32) -> Vec<f32> {
    let n = levels.max(MIP_LEVELS_MIN) as usize;
    let d = if decay.is_finite() {
        decay.clamp(COMPOSITE_DECAY_MIN, COMPOSITE_DECAY_MAX)
    } else {
        COMPOSITE_DECAY_MIN
    };
    let mut w = Vec::with_capacity(n);
    // 逐次相乘而非 `powi`：no_std 内核镜像目标无 std/libm 的 `f32::powi`。
    let mut term = 1.0f32;
    for _ in 0..n {
        w.push(term);
        term *= d;
    }
    let sum: f32 = w.iter().sum();
    if sum > LUMA_EPS {
        for v in w.iter_mut() {
            *v /= sum;
        }
    } else {
        // 全零（decay=0 且 n>1 时首项为 1，不会全零；此处为防御）。
        for (i, v) in w.iter_mut().enumerate() {
            *v = if i == 0 { 1.0 } else { 0.0 };
        }
    }
    w
}

/// 合成（第三段）：多级 mip 加权叠加后乘强度，加回主色缓冲。
///
/// 叠加语义是**加法**（`scene + bloom * intensity`）而非 lerp——泛光是光源
/// 在镜头介质中的散射，物理上是能量叠加；lerp 会把泛光变成"替换"（画面
/// 整体发白而不是光晕外扩）。
pub fn composite(
    scene: HdrColor,
    mips: &[HdrColor],
    params: &BloomParams,
) -> HdrColor {
    if mips.is_empty() {
        return scene;
    }
    let w = composite_weights(mips.len() as u8, params.composite_decay);
    let mut acc = HdrColor::new(0.0, 0.0, 0.0);
    for (i, m) in mips.iter().enumerate() {
        let wi = w.get(i).copied().unwrap_or(0.0);
        acc = HdrColor::new(
            acc.r + m.r * wi,
            acc.g + m.g * wi,
            acc.b + m.b * wi,
        );
    }
    let k = params.intensity;
    scene.add(acc.scale(k)).clamp_chain()
}

// ---------------------------------------------------------------------------
// 九、帧边界提交（F1762 规则：mip 级数切换帧边界生效）
// ---------------------------------------------------------------------------

/// Bloom 执行状态（持链者；级数切换走待决槽）。
#[derive(Clone, Debug, PartialEq)]
pub struct BloomChain {
    /// 当前生效的级数。
    pub active_levels: u8,
    /// 待决级数（`None` = 无待决变更）。
    pub pending_levels: Option<u8>,
    /// 当前主缓冲尺寸。
    pub main_size: (u32, u32),
    /// 当前计划（换链后重算）。
    pub plan: MipChainPlan,
    /// 累积诊断。
    pub diags: DiagBag,
}

/// Bloom 链构造（按已收敛参数建链）。
pub fn new_chain(main_w: u32, main_h: u32, resolved: &ResolvedParams, quota: u64) -> BloomChain {
    let mut diags = resolved.diags.clone();
    let plan = plan_mip_chain(
        main_w,
        main_h,
        resolved.params.mip_levels,
        quota,
        &mut diags,
    );
    BloomChain {
        active_levels: plan.levels,
        pending_levels: None,
        main_size: (main_w, main_h),
        plan,
        diags,
    }
}

impl BloomChain {
    /// 请求改级数（**帧内不生效**——只写待决槽并留显性挂起诊断）。
    ///
    /// 帧内立即重建链 = 半新半旧：新级数少一级时，本帧合成的 mip 数量与
    /// 权重序列长度不符，权重和不再是 1.0 → 亮度跳变一帧。
    pub fn request_levels(&mut self, levels: u8) {
        let lv = if levels == 0 {
            MIP_LEVELS_MIN
        } else {
            levels.min(MIP_LEVELS_MAX)
        };
        if lv == self.active_levels && self.pending_levels.is_none() {
            return;
        }
        self.pending_levels = Some(lv);
        self.diags.note(DiagCode::LevelSwitchPending, f32::from(lv));
    }

    /// 是否存在待决级数变更。
    pub fn has_pending(&self) -> bool {
        self.pending_levels.is_some()
    }

    /// 帧边界提交：换链或撤销（F1762 规则）。
    ///
    /// 返回是否真的换了链。分辨率同时变化时也走这里（RT 尺寸变了必须重分配）。
    pub fn commit_frame_boundary(
        &mut self,
        quota: u64,
        new_main: Option<(u32, u32)>,
    ) -> bool {
        let main_changed = match new_main {
            Some(sz) => sz != self.main_size,
            None => false,
        };
        if self.pending_levels.is_none() && !main_changed {
            return false;
        }
        let levels = self.pending_levels.unwrap_or(self.active_levels);
        self.pending_levels = None;
        if let Some(sz) = new_main {
            self.main_size = sz;
        }
        let (w, h) = self.main_size;
        let before = self.active_levels;
        self.plan = plan_mip_chain(w, h, levels, quota, &mut self.diags);
        self.active_levels = self.plan.levels;
        self.active_levels != before || main_changed
    }

    /// 当前生效计划。
    pub fn plan(&self) -> &MipChainPlan {
        &self.plan
    }

    /// 帧边界回收（交 F2003 池）。
    pub fn reclaim(&self) -> Vec<u16> {
        reclaim_all(&self.plan)
    }

    /// 合成权重（按当前生效级数）。
    pub fn weights(&self, decay: f32) -> Vec<f32> {
        composite_weights(self.active_levels, decay)
    }
}

// ---------------------------------------------------------------------------
// 十、光敏联动（F1962 扫描：三源之一的光照动效源扩展）
// ---------------------------------------------------------------------------

/// 光敏采样（泛光作为"光照动效源"进F1962 三闪扫描）。
///
/// 泛光的时间域特性与普通效果不同：泛光的**面积扩张**本身就是一种闪烁
/// （WCAG 2.3.1 关心的是每帧亮度变化的面积与频率）。故本采样给三要素：
/// 峰值强度、面积扩张率、时间变化率——三者同时超阈才判闪烁。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhotosensitiveSample {
    /// 泛光峰值强度（强度参数 × 提取峰值贡献）。
    pub intensity: f32,
    /// 面积扩张率（末级 mip 面积 / mip0 面积——链越长扩张越强）。
    pub area_spread: f32,
    /// 时间变化率（|本帧 - 上帧| / max(上帧, eps)，逻辑 tick 注入）。
    pub temporal_delta: f32,
}

impl PhotosensitiveSample {
    /// 三源联合判定（任一源超阈即标记为需扫描，不直接判闪烁——判定归
    /// F1962 的扫描器，本条只负责**把信号完整交出去**）。
    pub fn needs_scan(self, intensity_thr: f32, area_thr: f32, delta_thr: f32) -> bool {
        self.intensity > intensity_thr || self.area_spread > area_thr || self.temporal_delta > delta_thr
    }

    /// 读屏可读替述（无障碍：强泛光是光敏风险，替述须可被读屏软件读到）。
    pub fn screen_text(self) -> String {
        format!(
            "泛光：强度 {:.2}，面积扩张 {:.2}，时间变化 {:.3}{}",
            self.intensity,
            self.area_spread,
            self.temporal_delta,
            if self.needs_scan(1.5, 0.25, 0.35) {
                "（已标记进光敏扫描队列）"
            } else {
                ""
            }
        )
    }
}

/// 面积扩张率（末级面积 / mip0 面积）。
pub fn area_spread_ratio(dims: &[(u32, u32)]) -> f32 {
    if dims.len() < 2 {
        return 0.0;
    }
    let a0 = dims[0].0 as f64 * dims[0].1 as f64;
    let an = dims[dims.len() - 1].0 as f64 * dims[dims.len() - 1].1 as f64;
    if a0 <= 0.0 {
        return 0.0;
    }
    (an / a0) as f32
}

/// 构造本帧光敏采样。
pub fn photosensitive_sample(
    params: &BloomParams,
    dims: &[(u32, u32)],
    prev_peak: f32,
    cur_peak: f32,
) -> PhotosensitiveSample {
    let denom = prev_peak.abs().max(LUMA_EPS);
    PhotosensitiveSample {
        intensity: params.intensity * cur_peak.max(0.0),
        area_spread: area_spread_ratio(dims),
        temporal_delta: ((cur_peak - prev_peak).abs()) / denom,
    }
}

// ---------------------------------------------------------------------------
// 十一、管线序断言位（F2001 序声明的执行侧校验）
// ---------------------------------------------------------------------------

/// 后处理序位（本条在序中的位置）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineSlot {
    /// 光照输出后、色调映射前（本条的合法位）。
    BeforeToneMap,
    /// 色调映射后（F2006 之后——**非法**：泛光必须工作在 referred 线性域）。
    AfterToneMap,
    /// 编码后（更非法：显示值域做泛光等于对压缩值做模糊）。
    AfterEncode,
}

/// 序位断言（越位即拒绝——边界纪律，位错则泛光语义整体失效）。
///
/// 拒绝而非告警：位错时泛光"能出图但完全不对"（TM 后的高光层次已丢），
/// 静默出错误图比拒绝更难排查。
pub fn assert_slot(slot: PipelineSlot) -> Result<(), DiagCode> {
    match slot {
        PipelineSlot::BeforeToneMap => Ok(()),
        PipelineSlot::AfterToneMap => Err(DiagCode::SourceMaskInvalid),
        PipelineSlot::AfterEncode => Err(DiagCode::SourceMaskInvalid),
    }
}

// ---------------------------------------------------------------------------
// 十二、读屏摘要与自检入口
// ---------------------------------------------------------------------------

/// Bloom 状态读屏摘要（无障碍替述）。
pub fn screen_text(chain: &BloomChain, params: &BloomParams) -> String {
    let mut s = format!(
        "泛光：{} 级模糊链（半分辨率起步，阈值 {:.2} {}，软膝 {:.2}，强度 {:.2}）",
        chain.active_levels, params.threshold, THRESHOLD_UNIT, params.soft_knee, params.intensity
    );
    if chain.has_pending() {
        s.push_str(&format!(
            "；级数待切至 {}（帧边界生效）",
            chain.pending_levels.unwrap_or(0)
        ));
    }
    if let Some(d) = chain.diags.items.first() {
        s.push_str(&format!("；诊断：{}", d.code.code()));
    }
    s
}

/// VE-F2004 域自检（判据逐条映射见 `vek04_checks.rs`）。
pub fn run_vek04_checks() -> CheckSet {
    super::vek04_checks::run_vek04_checks()
}
