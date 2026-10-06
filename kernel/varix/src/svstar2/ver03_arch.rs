//! VE-F3602 · 创作生态总架构（VE-R 域 · 创作生态域 · R01 组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3602`
//!
//! **判据（锚点原文·六项）**：三层五段、开放格式 P0、激励单源、沙箱复述、
//! 收敛复述、判据。
//!
//! **职责定位（锚点原文）**：创作生态总架构——生态架构（创作生态三层：工具层
//! （编辑器/工坊——R02/R03）/资产层（创作资产模型——F3603）/分发层（市场分发
//! ——F3616）；生态原则（开放格式（创作资产开放格式存储——锁定格式=生态死刑
//! （开放格式红线：创作资产必须可导出可迁移（复述十四章执法）；沙箱隔离
//! （创作代码类资产（脚本/逻辑）沙箱执行——隔离复述；激励闭环（创作者署名/
//! 分成/评级——复用 F3114 模式跨域单源声明；架构五段接口（创建→编辑→验证→
//! 打包→分发——签名冻结 v1。
//!
//! **数据结构（锚点原文·家族格式）**：三层结构；五段签名；开放格式声明；
//! 激励协议引用。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：格式锁定检出→P0（红线实测
//! ——生态执法）；沙箱绕过→P0（复述）；激励分歧→对拍（复述）。
//!
//! **性能逐项分解（锚点原文·家族格式）**：五段 O(1) 接口；格式 O(1) 声明；
//! 激励 O(1) 对接。
//!
//! **跨批对接点（锚点原文·家族格式）**：F3114/F3347 双单源复用；F3204 类型
//! 扩展（资产类型注册）；Q 管线（打包分发对端）。
//!
//! **无隐私面（锚点原文）**：本项不采集创作者个人信息，只登记协议引用。
//!
//! # 〇、模块命名：`ver03_*` 的由来
//!
//! 仓内 [`ver01_arch`](super::ver01_arch) 是 **E 域** F3401 令牌运行时架构，
//! [`ver02_arch`](super::ver02_arch) 是本项前置 **VE-F3601** R 域开工与域号
//! ADR。两者序号都被占用，且本项前置就是 F3601（任务单「前置：VE-F3601」），
//! 顺着取`ver03_*`——序号递增避让，不改动他人模块名（源码只增不减不移）。
//!
//! # 一、三处歧义与本项裁决（不静默取舍）
//!
//! 锚点正文与判据之间有三处**对不上**的地方。任何一处默默填平，后人来读
//! 就再也查不到当初为什么这么定。本项逐处显式裁决：
//!
//! ## 1.1 判据说「收敛复述」，正文里找不到「收敛」二字（歧义一）
//!
//! 锚点「判据」写「收敛复述」，但「职责定位」通篇未出现「收敛」。若只按正文
//! 施工，这条判据会**无源可依**。本项的裁决依据是册内两条**原文**：
//!
//! - 本项跨批对接点写「Q 管线（打包分发对端）」；
//! - [`VE-F3616`](https://example.invalid) 锚点写「分发管线（走 Q 管线打包分发
//!   （复述收敛）」，其错误路径写「绕管线分发→立案（收敛复述）」。
//!
//! 所以「收敛复述」= **打包与分发两段必须经Q 管线，不得绕行**。判据的落点
//! 由此确定，逐项 `basis` 写在 [`ConvergencePoint::basis`]，不冒充锚点原句。
//!
//! ## 1.2 「F3114/F3347 双单源」是两个**侧面**，不是同一件事的两个来源（歧义二）
//!
//! 锚点同时写「激励闭环（创作者署名/分成/评级——复用 F3114 模式跨域单源声明）」
//! 与「跨批对接点：F3114/F3347 双单源复用」。字面上像「一个能力两个源」——
//! 若是那样，单源纪律反而被自己破坏。本项核册内实情后裁决：
//!
//! - **F3114 是生态语义侧的模式单源**（锚点原话「复用 F3114 模式跨域单源
//!   声明」）。F3114 自身锚点写「署名元数据（不可剥离哈希）」「下载与评分
//!   （评分进 F3109 评级）」。
//! - **F3347 是 SDK 接入侧的接口单源**（F3347= 管线 SDK 与 DX，其四核心接口
//!   含「注册资源类型（F3204 复用）」）。它与 F3114 管的不是一件事。
//!
//! 于是「双单源」= **生态语义单源（F3114）+ SDK 接入单源（F3347）两个不同
//! 侧面**，而非同一能力的两个来源。每项激励能力仍**有且只有一个属主**
//! （[`IncentiveClause::owner_code`]），重复即 [`E_INCENTIVE_DUP`]。三项各自
//! 的数据对端逐条写 `basis`：署名→F3114、评级→F3109（由 F3114 锚点
//! 「评分进 F3109 评级」转引）、分成→F3306（由 F3616 锚点「F3306 权限
//! （结算对端）」转引）。
//!
//! ## 1.3 五段与三层的对应关系锚点未明写（歧义三）
//!
//! 锚点把「三层」与「五段」并列写出，但没给映射。不给映射的后果是：五段接口
//! 找不到归属层，三层找不到承载段，两张表各自成立、合起来对不上。本项按
//! **段的产出物落在哪一层**推出映射（[`StageSignature::owner_layer`]）：
//!
//! | 段 | 属主层 | 取项依据 |
//! |---|---|---|
//! | 创建 Create | 工具层 | 产出物是创作中的草稿，尚未成形为可分发资产 |
//! | 编辑 Edit | 工具层 | 编辑器/工坊（F3621/F3641）里的就地修改 |
//! | 验证 Verify | 工具层 | 发布前把关（体��在 F3607，闸门属工具层流程） |
//! | 打包 Package | 资产层 | 产出物成为**资产包**，故归资产层（F3603） |
//! | 分发 Distribute | 分发层 | 上架与结算（F3616） |
//!
//! 该映射是**推导**，不是锚点原文；`basis` 逐段写明，并机检「五段各恰一属主
//! 层、无段落空、无段多主」。
//!
//! # 二、三层结构：每层有对端批次，不许是空壳
//!
//! 锚点给三层各配了对端：工具层→R02/R03（册内 R02=VE-F3621~F3640 主题编辑器
//! 内核组、R03=VE-F3641~F3660 壁纸与视觉工坊组）、资产层→F3603、分发层→F3616。
//! 本项用 [`LayerStack`] 固化，并要求每层**至少登记一个册内真实条目号**——
//! 空壳层即 [`E_LAYER_NO_PEER`]。理由：一个没有对端条目的层，说明它在册内
//! 还没有落点，接下来的五段会挂空。
//!
//! # 三、五段接口：签名冻结 v1
//!
//! 锚点写「架构五段接口（创建→编辑→验证→打包→分发——**签名冻结 v1**）」。
//! 「冻结」是要机检的——本项把每段签名写成常量（[`StageSignature::sig`]），
//! [`StageChain::check_frozen`] 逐段比对，**签名漂移即 [`E_STAGE_SIG_DRIFT`]**
//! （升版须走 ADR，不得就地改）。段序固定为创建→编辑→验证→打包→分发，
//! 倒序/缺段即 [`E_STAGE_ORDER`]。
//!
//! # 四、开放格式红线：锁定格式 = 生态死刑（判据二·P0）
//!
//! 锚点写「创作资产必须可导出可迁移（复述十四章执法）」，「锁定格式=生态死刑」，
//! 错误路径写「格式锁定检出→P0（红线实测——生态执法）」。本项的
//! [`OpenFormatRegistry`] 对每个资源类登记三项**可迁移性能力**：可导出、可
//! 迁入、开放规范。三项缺一即检出**格式锁定**，严重级 **P0**（[`P0`]）。
//!
//! 为什么是 P0 而不是 P1：格式锁定的后果不是「这个资产暂时不好用」，而是
//! **用户已有的创作成果无法带走**——生态的地基被抽掉。这类问题拖到最后
//! 再改，等于让所有创作者重做一遍。锚点原话是「生态死刑」，本项照此定为 P0。
//!
//! 覆盖范围取册内 [`F3204`](https://example.invalid) 锚点的**十类**资源类型
//! （纹理/网格/材质/音频/字体/动画/样式表/场景图/预制体/脚本数据），而非本域
//! 资产七类——因为「锁定格式」是资源容器层面的事，资产类别盖不住它。
//!
//! # 五、沙箱隔离：代码类必须沙箱，且是**复述**不是另立（判据四）
//!
//! 锚点写「创作代码类资产（脚本/逻辑）沙箱执行——隔离复述」，错误路径写
//! 「沙箱绕过→P0（复述）」。本项只**登记引用**（[`SandboxLedger`] 的
//! `reference_only` 恒真），不实现沙箱本体——沙箱在别处已实现，此处重写一份
//! 就是制造第二真相。机检两件事：
//!
//! 1. 代码类（脚本数据）必须声明沙箱引用，非代码类不得被强加沙箱；
//! 2. `reference_only` 恒真——本项一旦出现「自己实现沙箱」的痕迹即
//!    [`E_SANDBOX_REIMPLEMENTED`] 红，这是「复述」纪律的硬门。
//!
//! # 六、收敛复述：**只有两段**走管线（判据五）
//!
//! 这是本项最容易做错的一处。若把五段全部标成「经 Q 管线」，等于把创作时
//! 的本地编辑也塞进正式交付管线——那会污染正式缓存，与
//! [`VE-F3605`](https://example.invalid) 锚点「预览不走正式管线（轻量通道）」
//! 的隔离纪律直接冲突。本项按**产出物是否对外交付**划线：
//!
//! - **打包 Package / 分发 Distribute 两段：必须经 Q 管线**（对外交付物）；
//! - **创建 / 编辑 / 验证三段：本地态，不走正式管线**（检出误标即
//!   [`E_PIPELINE_LOCAL_LEAK`] 红）。
//!
//! # 七、错误路径与降级矩阵：三条各有动作与严重级
//!
//! 锚点三条错误路径方向不同，本项各给动作而不共用一套：
//!
//! | 锚点原文 | 严重级 | 降级动作 |
//! |---|---|---|
//! | 格式锁定检出→P0（红线实测——生态执法） | P0 | [`DegradeAction::BlockRelease`] |
//! | 沙箱绕过→P0（复述） | P0 | [`DegradeAction::BlockRelease`] |
//! | 激励分歧→对拍（复述） | P1 | [`DegradeAction::Reconcile`] |
//!
//! 处置方向相反的路径**不共用错误码**（记忆里的老教训：哈希「待补」是非阻断，
//! 「漂移」是必须重签，必须拆开）。激励分歧走**对拍**不是阻断——分歧要对账，
//! 直接拦下分发会误伤正常结算。
//!
//! # 八、禁扩面：本项只声明架构，不替别人做实现
//!
//! 明确不做（[`ECO_EXCLUSIONS`]）：F3603 的七要素模型字段、F3616 的上架流水与
//! 定价、F3607 的验证器实现、F3204 的类型注册实现、F3114 的生态库检索、
//! F3306 的结算实现、Q 管线本体、沙箱本体。越界即 [`E_OVERREACH`]——宁可少做，
//! 不可把别人的实现抄一遍当自己的。
//!
//! # 九、读屏替述（无障碍：本项输出的等价口述）
//!
//! 见 [`EcosystemArchitecture::narration`]：三层五段的原则、开放格式为什么是
//! 死刑、代码类为什么必须沙箱、激励为什么单源——逐条口述，不依赖图形。

#![allow(clippy::needless_range_loop)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 架构版本（锚点：签名冻结 v1）。
pub const ARCH_VERSION: &str = "R01-eco-v1";
/// 五段接口签名版本（锚点原文：签名冻结 v1）。
pub const STAGE_SIG_VERSION: &str = "stage-sig-v1";
/// 开放格式契约版本。
pub const FORMAT_VERSION: &str = "open-format-v1";
/// 激励协议引用版本。
pub const INCENTIVE_VERSION: &str = "incentive-v1";

/// 三层（锚点原文：创作生态三层）。
pub const LAYER_COUNT: usize = 3;
/// 五段（锚点原文：创建→编辑→验证→打包→分发）。
pub const STAGE_COUNT: usize = 5;
/// F3204 十类资源类型。
pub const RESOURCE_CLASS_COUNT: usize = 10;
/// 激励三项（署名/分成/评级）。
pub const INCENTIVE_COUNT: usize = 3;
/// 判据六项。
pub const CRITERION_COUNT: usize = 6;
/// 错误路径三条。
pub const DEGRADE_PATH_COUNT: usize = 3;
/// 禁扩面条数。
pub const EXCLUSION_COUNT: usize = 7;
/// 收敛点（**恰两段**：打包/分发——判据五的收敛线，见头注 §6）。
pub const CONVERGENCE_POINT_COUNT: usize = 2;

/// 每层最多登记的对端条目数（工具层要装 R02+R03 两个批次段）。
pub const MAX_PEERS_PER_LAYER: usize = 4;
/// 开放格式契约条目上限。
pub const MAX_FORMATS: usize = 16;
/// 激励条款上限。
pub const MAX_INCENTIVES: usize = 8;

// --- 错误码（异常零静默：每条降级路径各有其码）---

/// 层无对端条目（空壳层）。
pub const E_LAYER_NO_PEER: &str = "E_LAYER_NO_PEER";
/// 层数不为三。
pub const E_LAYER_COUNT: &str = "E_LAYER_COUNT";
/// 层级顺序错（工具→资产→分发）。
pub const E_LAYER_ORDER: &str = "E_LAYER_ORDER";
/// 层码往返失真。
pub const E_LAYER_CODE_ROUNDTRIP: &str = "E_LAYER_CODE_ROUNDTRIP";
/// 层无职责说明。
pub const E_LAYER_NO_DUTY: &str = "E_LAYER_NO_DUTY";
/// 对端条目号越界（不在 R 域号段内）。
pub const E_PEER_OUT_OF_BAND: &str = "E_PEER_OUT_OF_BAND";
/// 对端条目号重复登记。
pub const E_PEER_DUP: &str = "E_PEER_DUP";

/// 段数不为五。
pub const E_STAGE_COUNT: &str = "E_STAGE_COUNT";
/// 段序错（创建→编辑→验证→打包→分发）。
pub const E_STAGE_ORDER: &str = "E_STAGE_ORDER";
/// 签名漂移（冻结 v1，改签名须走 ADR）。
pub const E_STAGE_SIG_DRIFT: &str = "E_STAGE_SIG_DRIFT";
/// 段无签名。
pub const E_STAGE_NO_SIG: &str = "E_STAGE_NO_SIG";
/// 段无参数字面。
pub const E_STAGE_NO_PARAM: &str = "E_STAGE_NO_PARAM";
/// 段无结果字面。
pub const E_STAGE_NO_RESULT: &str = "E_STAGE_NO_RESULT";
/// 段无取项依据。
pub const E_STAGE_NO_BASIS: &str = "E_STAGE_NO_BASIS";

/// 段未挂属主层。
pub const E_STAGE_NO_LAYER: &str = "E_STAGE_NO_LAYER";
/// 段多主（一个段挂了多个属主层）。
pub const E_STAGE_MULTI_OWNER: &str = "E_STAGE_MULTI_OWNER";
/// 层无承载段（空壳层的反向检出）。
pub const E_LAYER_NO_STAGE: &str = "E_LAYER_NO_STAGE";

/// 资源类数不为十（F3204 十类）。
pub const E_CLASS_COUNT: &str = "E_CLASS_COUNT";
/// 资源类漏登记。
pub const E_CLASS_MISSING: &str = "E_CLASS_MISSING";
/// 格式锁定（不可导出 / 不可迁入 / 无开放规范）。
pub const E_FORMAT_LOCKED: &str = "E_FORMAT_LOCKED";
/// 开放规范无版本号。
pub const E_FORMAT_NO_SPEC: &str = "E_FORMAT_NO_SPEC";
/// 格式契约缺可迁移性声明项。
pub const E_FORMAT_NO_CAPABILITY: &str = "E_FORMAT_NO_CAPABILITY";

/// 代码类未声明沙箱引用。
pub const E_SANDBOX_MISSING: &str = "E_SANDBOX_MISSING";
/// 非代码类被强加沙箱（无谓开销）。
pub const E_SANDBOX_UNNEEDED: &str = "E_SANDBOX_UNNEEDED";
/// 沙箱绕过检出。
pub const E_SANDBOX_BYPASS: &str = "E_SANDBOX_BYPASS";
/// 沙箱被重实现（本项只许复述）。
pub const E_SANDBOX_REIMPLEMENTED: &str = "E_SANDBOX_REIMPLEMENTED";
/// 沙箱引用无出处。
pub const E_SANDBOX_NO_CITE: &str = "E_SANDBOX_NO_CITE";

/// 激励项数不为三。
pub const E_INCENTIVE_COUNT: &str = "E_INCENTIVE_COUNT";
/// 激励能力多主（单源纪律破）。
pub const E_INCENTIVE_DUP: &str = "E_INCENTIVE_DUP";
/// 激励能力无主。
pub const E_INCENTIVE_NO_OWNER: &str = "E_INCENTIVE_NO_OWNER";
/// 激励项无取项依据。
pub const E_INCENTIVE_NO_BASIS: &str = "E_INCENTIVE_NO_BASIS";
/// 署名不可剥（锚点：署名不可剥——盗包红线）。
pub const E_ATTRIBUTION_STRIPPABLE: &str = "E_ATTRIBUTION_STRIPPABLE";
/// SDK 接入单源被第二处声明。
pub const E_SDK_SOURCE_SPLIT: &str = "E_SDK_SOURCE_SPLIT";

/// 交付段绕Q 管线。
pub const E_PIPELINE_BYPASS: &str = "E_PIPELINE_BYPASS";
/// 本地段误标走正式管线。
pub const E_PIPELINE_LOCAL_LEAK: &str = "E_PIPELINE_LOCAL_LEAK";
/// 收敛点缺取项依据。
pub const E_CONVERGENCE_NO_BASIS: &str = "E_CONVERGENCE_NO_BASIS";

/// 降级路径缺失。
pub const E_DEGRADE_MISSING: &str = "E_DEGRADE_MISSING";
/// 降级动作与严重级不匹配。
pub const E_DEGRADE_MISMATCH: &str = "E_DEGRADE_MISMATCH";

/// 判据条目缺失。
pub const E_CRITERION_MISSING: &str = "E_CRITERION_MISSING";
/// 判据无锚点依据。
pub const E_CRITERION_NO_BASIS: &str = "E_CRITERION_NO_BASIS";

/// 越界（替别人实现）。
pub const E_OVERREACH: &str = "E_OVERREACH";

/// 严重级：P0 阻断（红线实测类）。
pub const P0: &str = "P0";
/// 严重级：P1 对拍/立案类。
pub const P1: &str = "P1";
/// 严重级：P2 观察类。
pub const P2: &str = "P2";

/// 契约问题（收集式，供机检一次性枚举）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcoIssue {
    /// 错误码（[`E_*`]之一）。
    pub code: &'static str,
    /// 定位键（层码/段码/类码/激励码……）。
    pub subject: String,
    /// 人可读说明。
    pub detail: String,
}

impl EcoIssue {
    /// 构造一条契约问题。
    pub fn new(code: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> EcoIssue {
        EcoIssue { code, subject: subject.into(), detail: detail.into() }
    }

    /// 是否属于「锁定/绕过」这类必须阻断（红线实测）的性质。
    pub fn is_redline(&self) -> bool {
        matches!(self.code, E_FORMAT_LOCKED | E_SANDBOX_BYPASS | E_PIPELINE_BYPASS)
    }
}

/// 生态契约错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcoError {
    /// 错误码。
    pub code: &'static str,
    /// 定位键。
    pub subject: String,
    /// 说明。
    pub detail: String,
}

impl EcoError {
    /// 构造错误。
    pub fn new(code: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> EcoError {
        EcoError { code, subject: subject.into(), detail: detail.into() }
    }

    /// 建议降级动作。
    pub fn action(&self) -> DegradeAction {
        match self.code {
            E_FORMAT_LOCKED | E_SANDBOX_BYPASS | E_SANDBOX_MISSING | E_PIPELINE_BYPASS
            | E_PIPELINE_LOCAL_LEAK => DegradeAction::BlockRelease,
            E_INCENTIVE_DUP | E_INCENTIVE_NO_OWNER | E_ATTRIBUTION_STRIPPABLE
            | E_SDK_SOURCE_SPLIT => DegradeAction::Reconcile,
            _ => DegradeAction::FileCase,
        }
    }

    /// 建议严重级。
    pub fn severity(&self) -> &'static str {
        match self.code {
            E_FORMAT_LOCKED | E_SANDBOX_BYPASS | E_SANDBOX_MISSING | E_PIPELINE_BYPASS
            | E_PIPELINE_LOCAL_LEAK | E_SANDBOX_REIMPLEMENTED => P0,
            E_INCENTIVE_DUP | E_INCENTIVE_NO_OWNER | E_ATTRIBUTION_STRIPPABLE
            | E_SDK_SOURCE_SPLIT => P1,
            _ => P2,
        }
    }

    /// 转成契约问题（供机检枚举）。
    pub fn to_issue(&self) -> EcoIssue {
        EcoIssue::new(self.code, self.subject.clone(), self.detail.clone())
    }
}

impl core::fmt::Display for EcoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.subject, self.detail)
    }
}

/// FNV-1a64（确定性能力码派生用——要的是稳定，不是抗攻击）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// FNV-1a64 十六进制（读屏与日志用）。
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}// ---------------------------------------------------------------------------
// 一、三层结构
// ---------------------------------------------------------------------------

/// 生态三层（锚点原文：工具层/资产层/分发层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcoLayer {
    /// 工具层：编辑器/工坊（对端R02/R03）。
    Tool,
    /// 资产层：创作资产模型（对端 F3603）。
    Asset,
    /// 分发层：市场分发（对端 F3616）。
    Distribution,
}

impl EcoLayer {
    /// 全部层（定长兑现O(1) 遍历）。
    pub const ALL: [EcoLayer; LAYER_COUNT] =
        [EcoLayer::Tool, EcoLayer::Asset, EcoLayer::Distribution];

    /// 层级序（锚点：工具→资产→分发，产物由内向外）。
    pub const fn order(self) -> usize {
        match self {
            EcoLayer::Tool => 0,
            EcoLayer::Asset => 1,
            EcoLayer::Distribution => 2,
        }
    }

    /// 层码（稳定字面）。
    pub const fn code(self) -> &'static str {
        match self {
            EcoLayer::Tool => "L1-TOOL",
            EcoLayer::Asset => "L2-ASSET",
            EcoLayer::Distribution => "L3-DIST",
        }
    }

    /// 由码反查层（往返须无损）。
    pub fn from_code(code: &str) -> Option<EcoLayer> {
        match code {
            "L1-TOOL" => Some(EcoLayer::Tool),
            "L2-ASSET" => Some(EcoLayer::Asset),
            "L3-DIST" => Some(EcoLayer::Distribution),
            _ => None,
        }
    }

    /// 层名（中文，读屏用）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            EcoLayer::Tool => "工具层",
            EcoLayer::Asset => "资产层",
            EcoLayer::Distribution => "分发层",
        }
    }

    /// 该层职责（锚点原文摘录）。
    pub const fn duty(self) -> &'static str {
        match self {
            EcoLayer::Tool => "编辑器与工坊：创作动作的发起地，产物是草稿",
            EcoLayer::Asset => "创作资产模型：草稿成形为可分发资产包",
            EcoLayer::Distribution => "市场上架与结算：资产包的对外交付",
        }
    }
}

/// 层对端条目（册内真实条目号——空壳层的反检出依据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerPeer {
    /// 册内条目号（如 3603、3616）。
    pub item: u32,
    /// 该条目的作用说明。
    pub note: &'static str,
}

/// R 域号段（承前置 [`VE-F3601`](super::ver02_arch) 的裁决：R 自 F3601 起）。
pub const R_BAND_LO: u32 = 3601;
/// R 域号段上界（承 F3601：F3601-F3800 为 R 域段，S 自 F3801 起）。
pub const R_BAND_HI: u32 = 3800;

/// 一层架构登记。
#[derive(Clone, Copy, Debug)]
pub struct LayerSpec {
    /// 层。
    pub layer: EcoLayer,
    /// 职责（锚点摘录，非空）。
    pub duty: &'static str,
    /// 对端条目（至少一个，见 [`E_LAYER_NO_PEER`]）。
    pub peers: [LayerPeer; MAX_PEERS_PER_LAYER],
    /// 对端实际个数。
    pub peer_count: usize,
}

impl LayerSpec {
    /// 构造一层（对端为空壳——反例用）。
    pub const fn empty(layer: EcoLayer) -> LayerSpec {
        LayerSpec {
            layer,
            duty: "",
            peers: [LayerPeer { item: 0, note: "" }; MAX_PEERS_PER_LAYER],
            peer_count: 0,
        }
    }

    /// 登记一个对端条目。
    pub fn push_peer(&mut self, item: u32, note: &'static str) -> Result<(), EcoError> {
        if self.peer_count >= MAX_PEERS_PER_LAYER {
            return Err(EcoError::new(
                "E_PEER_CAP",
                self.layer.code(),
                format!("对端条目超过上限 {}", MAX_PEERS_PER_LAYER),
            ));
        }
        if self.peers[..self.peer_count].iter().any(|p| p.item == item) {
            return Err(EcoError::new(
                E_PEER_DUP,
                self.layer.code(),
                format!("对端条目 {} 重复登记", item),
            ));
        }
        self.peers[self.peer_count] = LayerPeer { item, note };
        self.peer_count += 1;
        Ok(())
    }

    /// 是否含某条目号。
    pub fn has_peer(&self, item: u32) -> bool {
        self.peers[..self.peer_count].iter().any(|p| p.item == item)
    }

    /// 层身份指纹（层码 + 职责 + 对端清单——任一改动即指纹变）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = String::new();
        buf.push_str(self.layer.code());
        buf.push('|');
        buf.push_str(self.duty);
        for p in &self.peers[..self.peer_count] {
            buf.push('|');
            buf.push_str(&alloc::format!("{}", p.item));
        }
        fnv1a64(buf.as_bytes())
    }
}

/// 三层架构总表。
#[derive(Clone, Copy, Debug)]
pub struct LayerStack {
    /// 三层（定长）。
    pub layers: [LayerSpec; LAYER_COUNT],
}

impl LayerStack {
    /// 空总表（反例用）。
    pub const fn empty() -> LayerStack {
        LayerStack { layers: [LayerSpec::empty(EcoLayer::Tool), LayerSpec::empty(EcoLayer::Asset), LayerSpec::empty(EcoLayer::Distribution)] }
    }

    /// 查层。
    pub fn get(&self, layer: EcoLayer) -> &LayerSpec {
        &self.layers[layer.order()]
    }

    /// 可变查层。
    pub fn get_mut(&mut self, layer: EcoLayer) -> &mut LayerSpec {
        &mut self.layers[layer.order()]
    }

    /// 逐层校验（空壳/越段/职责缺失）。
    pub fn audit(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        // 逐层报空壳——不只报第一个（漏报其余两层等于让两层悄悄无对端）。
        for s in self.layers.iter() {
            if s.peer_count == 0 {
                out.push(EcoIssue::new(
                    E_LAYER_NO_PEER,
                    s.layer.code(),
                    "层无册内对端条目——空壳层会让五段挂空",
                ));
            }
        }
        for s in self.layers.iter() {
            if s.duty.trim().is_empty() {
                out.push(EcoIssue::new(
                    E_LAYER_NO_DUTY,
                    s.layer.code(),
                    "层无职责说明",
                ));
            }
            for p in &s.peers[..s.peer_count] {
                if p.item < R_BAND_LO || p.item > R_BAND_HI {
                    out.push(EcoIssue::new(
                        E_PEER_OUT_OF_BAND,
                        s.layer.code(),
                        format!(
                            "对端条目 F{} 不在 R 域号段 F{}-F{} 内（承F3601 跳段裁决）",
                            p.item, R_BAND_LO, R_BAND_HI
                        ),
                    ));
                }
                if p.note.trim().is_empty() {
                    out.push(EcoIssue::new(
                        E_PEER_OUT_OF_BAND,
                        s.layer.code(),
                        format!("对端条目 F{} 无作用说明", p.item),
                    ));
                }
            }
        }
        out
    }

    /// 层码往返无损（改一层码必须同步改 from_code）。
    pub fn code_roundtrip(&self) -> bool {
        EcoLayer::ALL
            .iter()
            .all(|l| EcoLayer::from_code(l.code()) == Some(*l))
    }

    /// 层级序严守工具(0)→资产(1)→分发(2)。
    pub fn order_ok(&self) -> bool {
        self.layers
            .iter()
            .enumerate()
            .all(|(i, s)| s.layer.order() == i)
    }
}

// ---------------------------------------------------------------------------
// 二、五段接口（签名冻结 v1）
// ---------------------------------------------------------------------------

/// 架构五段（锚点原文：创建→编辑→验证→打包→分发）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcoStage {
    /// 创建。
    Create,
    /// 编辑。
    Edit,
    /// 验证。
    Verify,
    /// 打包。
    Package,
    /// 分发。
    Distribute,
}

impl EcoStage {
    /// 全部段（顺序即段序）。
    pub const ALL: [EcoStage; STAGE_COUNT] = [
        EcoStage::Create,
        EcoStage::Edit,
        EcoStage::Verify,
        EcoStage::Package,
        EcoStage::Distribute,
    ];

    /// 段序。
    pub const fn order(self) -> usize {
        match self {
            EcoStage::Create => 0,
            EcoStage::Edit => 1,
            EcoStage::Verify => 2,
            EcoStage::Package => 3,
            EcoStage::Distribute => 4,
        }
    }

    /// 段码。
    pub const fn code(self) -> &'static str {
        match self {
            EcoStage::Create => "S1-CREATE",
            EcoStage::Edit => "S2-EDIT",
            EcoStage::Verify => "S3-VERIFY",
            EcoStage::Package => "S4-PACKAGE",
            EcoStage::Distribute => "S5-DIST",
        }
    }

    /// 由码反查段。
    pub fn from_code(code: &str) -> Option<EcoStage> {
        match code {
            "S1-CREATE" => Some(EcoStage::Create),
            "S2-EDIT" => Some(EcoStage::Edit),
            "S3-VERIFY" => Some(EcoStage::Verify),
            "S4-PACKAGE" => Some(EcoStage::Package),
            "S5-DIST" => Some(EcoStage::Distribute),
            _ => None,
        }
    }

    /// 段名（中文）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            EcoStage::Create => "创建",
            EcoStage::Edit => "编辑",
            EcoStage::Verify => "验证",
            EcoStage::Package => "打包",
            EcoStage::Distribute => "分发",
        }
    }

    /// 冻结签名（v1，锚点原文「签名冻结 v1」——改此值即须走 ADR）。
    pub const fn sig(self) -> u32 {
        match self {
            EcoStage::Create => 0x11A7_0001,
            EcoStage::Edit => 0x11A7_0002,
            EcoStage::Verify => 0x11A7_0003,
            EcoStage::Package => 0x11A7_0004,
            EcoStage::Distribute => 0x11A7_0005,
        }
    }

    /// 入参字面。
    pub const fn param(self) -> &'static str {
        match self {
            EcoStage::Create => "asset_draft",
            EcoStage::Edit => "asset_draft+edit_op",
            EcoStage::Verify => "edited_draft+verify_req",
            EcoStage::Package => "verified_asset",
            EcoStage::Distribute => "asset_pkg+listing",
        }
    }

    /// 产出字面。
    pub const fn result(self) -> &'static str {
        match self {
            EcoStage::Create => "asset_draft",
            EcoStage::Edit => "edited_draft",
            EcoStage::Verify => "verified_asset",
            EcoStage::Package => "asset_pkg",
            EcoStage::Distribute => "distribution_receipt",
        }
    }

    /// 属主层（**推导**，见头注 §1.3）。
    pub const fn owner_layer(self) -> EcoLayer {
        match self {
            EcoStage::Create | EcoStage::Edit | EcoStage::Verify => EcoLayer::Tool,
            EcoStage::Package => EcoLayer::Asset,
            EcoStage::Distribute => EcoLayer::Distribution,
        }
    }

    /// 取项依据（写明这是推导而非锚点原句）。
    pub const fn basis(self) -> &'static str {
        match self {
            EcoStage::Create => "推导：产出物 asset_draft 是创作中的草稿，尚未成形为资产包→工具层",
            EcoStage::Edit => "推导：就地修改发生在编辑器/工坊内（F3621/F3641）→工具层",
            EcoStage::Verify => "推导：发布前把关是工具层流程动作（闸门本体在 F3607）→工具层",
            EcoStage::Package => "推导：产出物 asset_pkg 成为资产包，归资产层（F3603）",
            EcoStage::Distribute => "锚点直陈：分发层对端即市场分发（F3616）",
        }
    }

    /// 产出物是否对外交付（划收敛线的依据，见头注 §6）。
    pub const fn delivers_externally(self) -> bool {
        matches!(self, EcoStage::Package | EcoStage::Distribute)
    }
}

/// 五段签名登记（一条段 = 一份冻结签名）。
#[derive(Clone, Copy, Debug)]
pub struct StageSignature {
    /// 段。
    pub stage: EcoStage,
    /// 冻结签名。
    pub sig: u32,
    /// 入参字面。
    pub param: &'static str,
    /// 产出字面。
    pub result: &'static str,
    /// 属主层（可被覆写以构造反例）。
    pub owner: EcoLayer,
    /// 取项依据。
    pub basis: &'static str,
}

impl StageSignature {
    /// 按段序构造标准签名册。
    pub fn standard() -> [StageSignature; STAGE_COUNT] {
        EcoStage::ALL.map(|s| StageSignature {
            stage: s,
            sig: s.sig(),
            param: s.param(),
            result: s.result(),
            owner: s.owner_layer(),
            basis: s.basis(),
        })
    }

    /// 契约指纹（段码 + 签名 + 参产 + 属主）。
    pub fn fingerprint(&self) -> u64 {
        let buf = format!(
            "{}|{:08x}|{}|{}|{}",
            self.stage.code(),
            self.sig,
            self.param,
            self.result,
            self.owner.code()
        );
        fnv1a64(buf.as_bytes())
    }
}

/// 五段链。
#[derive(Clone, Copy, Debug)]
pub struct StageChain {
    /// 五段签名。
    pub stages: [StageSignature; STAGE_COUNT],
}

impl StageChain {
    /// 标准链。
    pub fn standard() -> StageChain {
        StageChain { stages: StageSignature::standard() }
    }

    /// 空链（反例用：签名全零）。
    pub fn empty() -> StageChain {
        StageChain {
            stages: EcoStage::ALL.map(|s| StageSignature {
                stage: s,
                sig: 0,
                param: "",
                result: "",
                owner: EcoLayer::Tool,
                basis: "",
            }),
        }
    }

    /// 查段。
    pub fn get(&self, stage: EcoStage) -> &StageSignature {
        &self.stages[stage.order()]
    }

    /// 可变查段。
    pub fn get_mut(&mut self, stage: EcoStage) -> &mut StageSignature {
        &mut self.stages[stage.order()]
    }

    /// 签名冻结校验（漂移即 [`E_STAGE_SIG_DRIFT`]）。
    pub fn check_frozen(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        for s in self.stages.iter() {
            if s.sig == 0 {
                out.push(EcoIssue::new(
                    E_STAGE_NO_SIG,
                    s.stage.code(),
                    "段签名缺失",
                ));
            } else if s.sig != s.stage.sig() {
                out.push(EcoIssue::new(
                    E_STAGE_SIG_DRIFT,
                    s.stage.code(),
                    format!(
                        "签名 {:08x} ≠冻结值 {:08x}（v1冻结，升版须走 ADR）",
                        s.sig,
                        s.stage.sig()
                    ),
                ));
            }
        }
        out
    }

    /// 契约完整性（段序/参产/依据）。
    pub fn audit(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        for (i, s) in self.stages.iter().enumerate() {
            if s.stage.order() != i {
                out.push(EcoIssue::new(
                    E_STAGE_ORDER,
                    s.stage.code(),
                    format!("段序错：位置 {} 应为序{}", i, s.stage.order()),
                ));
            }
            if s.param.trim().is_empty() {
                out.push(EcoIssue::new(
                    E_STAGE_NO_PARAM,
                    s.stage.code(),
                    "段无入参字面",
                ));
            }
            if s.result.trim().is_empty() {
                out.push(EcoIssue::new(
                    E_STAGE_NO_RESULT,
                    s.stage.code(),
                    "段无产出字面",
                ));
            }
            if s.basis.trim().is_empty() {
                out.push(EcoIssue::new(
                    E_STAGE_NO_BASIS,
                    s.stage.code(),
                    "段无取项依据（推导也须写明依据，不许冒充锚点原句）",
                ));
            }
        }
        out
    }

    /// 段码往返无损。
    pub fn code_roundtrip(&self) -> bool {
        self.stages
            .iter()
            .all(|s| EcoStage::from_code(s.stage.code()) == Some(s.stage))
    }

    /// 产出衔接：上一段产出即下一段入参（首段除外）。
    pub fn chain_continuous(&self) -> bool {
        self.stages
            .iter()
            .enumerate()
            .all(|(i, s)| i == 0 || s.param.split('+').next() == Some(self.stages[i - 1].result))
    }
}// ---------------------------------------------------------------------------
// 三、五段 × 三层映射（歧义一/三的落点，见头注 §1.3）
// ---------------------------------------------------------------------------

/// 段→层归属登记（每段恰一属主，见 [`E_STAGE_MULTI_OWNER`]）。
#[derive(Clone, Copy, Debug)]
pub struct StageLayerMap {
    /// 五段的属主层。
    pub owner: [EcoLayer; STAGE_COUNT],
}

impl StageLayerMap {
    /// 标准映射（段序即数组序）。
    pub fn standard() -> StageLayerMap {
        StageLayerMap {
            owner: EcoStage::ALL.map(|s| s.owner_layer()),
        }
    }

    /// 空映射（反例：全堆工具层）。
    pub fn all_tool() -> StageLayerMap {
        StageLayerMap { owner: [EcoLayer::Tool; STAGE_COUNT] }
    }

    /// 查某段属主。
    pub fn owner_of(&self, stage: EcoStage) -> EcoLayer {
        self.owner[stage.order()]
    }

    /// 校验：五段各恰一属主 + 三层各至少承载一段（双向无空壳）。
    pub fn audit(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        // 每段恰一主：本项用定长一槽表达，改成Vec 后此处即须检出多主。
        for (i, l) in self.owner.iter().enumerate() {
            let want = EcoStage::ALL[i].owner_layer();
            if *l != want {
                out.push(EcoIssue::new(
                    E_STAGE_NO_LAYER,
                    EcoStage::ALL[i].code(),
                    format!(
                        "段属主层为 {}，推导应为 {}（依据：{}）",
                        l.name_cn(),
                        want.name_cn(),
                        EcoStage::ALL[i].basis()
                    ),
                ));
            }
        }
        // 每层至少承载一段：三层齐备而无段者即空壳层。
        for l in EcoLayer::ALL.iter() {
            if !self.owner.iter().any(|o| o == l) {
                out.push(EcoIssue::new(
                    E_LAYER_NO_STAGE,
                    l.code(),
                    format!("{} 无承载段——空壳层", l.name_cn()),
                ));
            }
        }
        out
    }

    /// 某层承载的段数。
    pub fn load_of(&self, layer: EcoLayer) -> usize {
        self.owner.iter().filter(|o| **o == layer).count()
    }
}

// ---------------------------------------------------------------------------
// 四、开放格式红线（判据二 · P0）
// ---------------------------------------------------------------------------

/// F3204 十类资源类型（锚点 F3204 原文：纹理/网格/材质/音频/字体/动画/
/// 样式表/场景图/预制体/脚本数据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceClass {
    /// 纹理。
    Texture,
    /// 网格。
    Mesh,
    /// 材质。
    Material,
    /// 音频。
    Audio,
    /// 字体。
    Font,
    /// 动画。
    Animation,
    /// 样式表。
    StyleSheet,
    /// 场景图。
    SceneGraph,
    /// 预制体。
    Prefab,
    /// 脚本数据（**代码类**，须沙箱）。
    ScriptData,
}

impl ResourceClass {
    /// 全部类（定长兑现 O(1) 遍历）。
    pub const ALL: [ResourceClass; RESOURCE_CLASS_COUNT] = [
        ResourceClass::Texture,
        ResourceClass::Mesh,
        ResourceClass::Material,
        ResourceClass::Audio,
        ResourceClass::Font,
        ResourceClass::Animation,
        ResourceClass::StyleSheet,
        ResourceClass::SceneGraph,
        ResourceClass::Prefab,
        ResourceClass::ScriptData,
    ];

    /// 类序（定长槽位寻址——`ALL` 数组序即类序）。
    pub const fn order(self) -> usize {
        match self {
            ResourceClass::Texture => 0,
            ResourceClass::Mesh => 1,
            ResourceClass::Material => 2,
            ResourceClass::Audio => 3,
            ResourceClass::Font => 4,
            ResourceClass::Animation => 5,
            ResourceClass::StyleSheet => 6,
            ResourceClass::SceneGraph => 7,
            ResourceClass::Prefab => 8,
            ResourceClass::ScriptData => 9,
        }
    }

    /// 类码。
    pub const fn code(self) -> &'static str {
        match self {
            ResourceClass::Texture => "T-TEX",
            ResourceClass::Mesh => "T-MESH",
            ResourceClass::Material => "T-MAT",
            ResourceClass::Audio => "T-AUD",
            ResourceClass::Font => "T-FONT",
            ResourceClass::Animation => "T-ANIM",
            ResourceClass::StyleSheet => "T-SS",
            ResourceClass::SceneGraph => "T-SG",
            ResourceClass::Prefab => "T-PREFAB",
            ResourceClass::ScriptData => "T-SCRIPT",
        }
    }

    /// 由码反查类。
    pub fn from_code(code: &str) -> Option<ResourceClass> {
        ResourceClass::ALL.iter().copied().find(|c| c.code() == code)
    }

    /// 类名（中文）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            ResourceClass::Texture => "纹理",
            ResourceClass::Mesh => "网格",
            ResourceClass::Material => "材质",
            ResourceClass::Audio => "音频",
            ResourceClass::Font => "字体",
            ResourceClass::Animation => "动画",
            ResourceClass::StyleSheet => "样式表",
            ResourceClass::SceneGraph => "场景图",
            ResourceClass::Prefab => "预制体",
            ResourceClass::ScriptData => "脚本数据",
        }
    }

    /// 是否代码类（代码类必须沙箱，见 §5）。
    pub const fn is_code(self) -> bool {
        matches!(self, ResourceClass::ScriptData)
    }
}

/// 开放规范前缀（**唯一事实源**：规范名不以此前缀开头即视为专有格式）。
pub const OPEN_SPEC_PREFIX: &str = "open-spec/";

/// 开放格式契约一条（判据二：可导出可迁移）。
#[derive(Clone, Copy, Debug)]
pub struct OpenFormat {
    /// 资源类。
    pub class: ResourceClass,
    /// 可导出（创作资产必须可导出）。
    pub exportable: bool,
    /// 可迁入（用户能从别处带进来）。
    pub migratable_in: bool,
    /// 开放规范版本（非空才叫开放格式；专有格式即锁）。
    pub open_spec: &'static str,
}

impl OpenFormat {
    /// 标准契约：十类全部可导出可迁入且有开放规范版本。
    pub fn standard() -> [OpenFormat; RESOURCE_CLASS_COUNT] {
        ResourceClass::ALL.map(|c| OpenFormat {
            class: c,
            exportable: true,
            migratable_in: true,
            open_spec: match c {
                ResourceClass::Texture => "open-spec/text-v1",
                ResourceClass::Mesh => "open-spec/mesh-v1",
                ResourceClass::Material => "open-spec/mat-v1",
                ResourceClass::Audio => "open-spec/audio-v1",
                ResourceClass::Font => "open-spec/font-v1",
                ResourceClass::Animation => "open-spec/anim-v1",
                ResourceClass::StyleSheet => "open-spec/ss-v1",
                ResourceClass::SceneGraph => "open-spec/scene-v1",
                ResourceClass::Prefab => "open-spec/prefab-v1",
                ResourceClass::ScriptData => "open-spec/script-v1",
            },
        })
    }

    /// 规范名是否为**开放**规范（须以 [`OPEN_SPEC_PREFIX`] 开头）。
    ///
    /// 这一条是开放格式红线的实质：非空的专有名（`vendor-lock/prefab-v9`）
    ///照样能把用户锁死，只判空等于没检。专有格式 = 生态死刑。
    pub fn spec_is_open(&self) -> bool {
        let s = self.open_spec.trim();
        !s.is_empty() && s.starts_with(OPEN_SPEC_PREFIX)
    }

    /// 是否被锁定（可导出/可迁入缺失，或规范非开放）。
    pub fn locked(&self) -> bool {
        !self.exportable || !self.migratable_in || !self.spec_is_open()
    }

    /// 锁定原因（人可读；未锁返回空串）。
    pub fn lock_reason(&self) -> String {
        let mut why: Vec<&str> = Vec::new();
        if !self.exportable {
            why.push("不可导出");
        }
        if !self.migratable_in {
            why.push("不可迁入");
        }
        if self.open_spec.trim().is_empty() {
            why.push("无开放规范");
        } else if !self.spec_is_open() {
            why.push("专有格式（非开放规范）");
        }
        if why.is_empty() {
            String::new()
        } else {
            format!("{}：{}", self.class.name_cn(), why.join(" + "))
        }
    }
}

/// 开放格式总表。
#[derive(Clone, Copy, Debug)]
pub struct FormatRegistry {
    /// 契约条目。
    pub items: [OpenFormat; RESOURCE_CLASS_COUNT],
}

impl FormatRegistry {
    /// 标准总表。
    pub fn standard() -> FormatRegistry {
        FormatRegistry { items: OpenFormat::standard() }
    }

    /// 空总表（反例用：全未声明）。
    pub fn empty() -> FormatRegistry {
        FormatRegistry {
            items: ResourceClass::ALL.map(|c| OpenFormat {
                class: c,
                exportable: false,
                migratable_in: false,
                open_spec: "",
            }),
        }
    }

    /// 查类契约。
    pub fn get(&self, class: ResourceClass) -> &OpenFormat {
        &self.items[class.order()]
    }

    /// 可变查类契约。
    pub fn get_mut(&mut self, class: ResourceClass) -> &mut OpenFormat {
        &mut self.items[class.order()]
    }

    /// 格式锁定审计（**十类逐个核**，不看汇总值——单个类被锁不能被九个合规类淹没）。
    pub fn audit(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        for (i, f) in self.items.iter().enumerate() {
            if f.class.order() != i {
                out.push(EcoIssue::new(
                    E_CLASS_COUNT,
                    f.class.code(),
                    format!("类位错：位置 {} 应为序{}", i, f.class.order()),
                ));
            }
            if f.class.code() != ResourceClass::ALL[i].code() {
                out.push(EcoIssue::new(
                    E_CLASS_MISSING,
                    f.class.code(),
                    format!("类位 {} 登记了别的类（应为 {}）", i, ResourceClass::ALL[i].code()),
                ));
            }
            if f.locked() {
                out.push(EcoIssue::new(
                    E_FORMAT_LOCKED,
                    f.class.code(),
                    format!("{}（P0·生态死刑：用户成果带不走）", f.lock_reason()),
                ));
            }
            if f.open_spec.trim().is_empty() {
                out.push(EcoIssue::new(E_FORMAT_NO_SPEC, f.class.code(), "开放规范版本为空"));
            }
        }
        out
    }

    /// 被锁类数（机检辅助；注意这是**红项计数**，不是 CheckSet 的总数）。
    pub fn locked_count(&self) -> usize {
        self.items.iter().filter(|f| f.locked()).count()
    }

    /// 类码往返无损。
    pub fn code_roundtrip(&self) -> bool {
        ResourceClass::ALL
            .iter()
            .all(|c| ResourceClass::from_code(c.code()) == Some(*c))
    }
}

// ---------------------------------------------------------------------------
// 五、沙箱隔离（判据四·复述不另立）
// ---------------------------------------------------------------------------

/// 沙箱引用登记一条。
#[derive(Clone, Copy, Debug)]
pub struct SandboxRef {
    /// 资源类。
    pub class: ResourceClass,
    /// 沙箱出处（册内条目号/模块名——本项只引用，不实现）。
    pub cite: &'static str,
    /// 隔离级别（声明式，本项不执行）。
    pub level: &'static str,
    /// 是否被绕过（检出即 P0）。
    pub bypassed: bool,
}

impl SandboxRef {
    /// 标准登记（仅代码类有沙箱引用）。
    pub fn standard() -> [SandboxRef; RESOURCE_CLASS_COUNT] {
        ResourceClass::ALL.map(|c| SandboxRef {
            class: c,
            cite: if c.is_code() { "F4604 沙箱（vew04_sandbox）" } else { "" },
            level: if c.is_code() { "进程隔离+能力白名单" } else { "" },
            bypassed: false,
        })
    }
}

/// 沙箱总表。
#[derive(Clone, Copy, Debug)]
pub struct SandboxLedger {
    /// 登记条目。
    pub items: [SandboxRef; RESOURCE_CLASS_COUNT],
    /// 本项只许复述——恒真；一旦置假即「自己实现了沙箱」。
    pub reference_only: bool,
}

impl SandboxLedger {
    /// 标准总表。
    pub fn standard() -> SandboxLedger {
        SandboxLedger { items: SandboxRef::standard(), reference_only: true }
    }

    /// 反例总表（重实现痕迹）。
    pub fn reimplemented() -> SandboxLedger {
        SandboxLedger { items: SandboxRef::standard(), reference_only: false }
    }

    /// 查类登记。
    pub fn get(&self, class: ResourceClass) -> &SandboxRef {
        &self.items[class.order()]
    }

    /// 可变查类登记。
    pub fn get_mut(&mut self, class: ResourceClass) -> &mut SandboxRef {
        &mut self.items[class.order()]
    }

    /// 沙箱审计（代码类必沙箱 / 非代码类不强加/ 绕过即 P0 / 只许复述）。
    pub fn audit(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        if !self.reference_only {
            out.push(EcoIssue::new(
                E_SANDBOX_REIMPLEMENTED,
                "sandbox-ledger",
                "本项只许复述沙箱隔离，出现自实现痕迹即制造第二真相",
            ));
        }
        for s in self.items.iter() {
            if s.class.is_code() {
                if s.cite.trim().is_empty() {
                    out.push(EcoIssue::new(
                        E_SANDBOX_MISSING,
                        s.class.code(),
                        "代码类资产未声明沙箱引用（P0·锚点：代码类沙箱执行）",
                    ));
                } else if s.bypassed {
                    out.push(EcoIssue::new(
                        E_SANDBOX_BYPASS,
                        s.class.code(),
                        format!("沙箱绕过检出：绕过 {}{}", s.class.name_cn(), s.cite),
                    ));
                }
            } else if !s.cite.trim().is_empty() {
                out.push(EcoIssue::new(
                    E_SANDBOX_UNNEEDED,
                    s.class.code(),
                    format!("{} 非代码类，不应挂沙箱（无谓开销）", s.class.name_cn()),
                ));
            }
            if s.bypassed && s.class.is_code() && s.cite.trim().is_empty() {
                // 绕过但无引用：既缺沙箱又检出绕过，两条都记（不静默吞其一）。
                out.push(EcoIssue::new(
                    E_SANDBOX_NO_CITE,
                    s.class.code(),
                    "绕过检出但无沙箱出处——无法定位绕过了什么",
                ));
            }
        }
        out
    }
}// ---------------------------------------------------------------------------
// 六、激励闭环（判据三·单源；歧义二的落点，见头注 §1.2）
// ---------------------------------------------------------------------------

/// 激励三项（锚点原文：创作者署名/分成/评级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncentiveKind {
    /// 署名。
    Attribution,
    /// 分成。
    RevenueShare,
    /// 评级。
    Rating,
}

impl IncentiveKind {
    /// 全部项。
    pub const ALL: [IncentiveKind; INCENTIVE_COUNT] = [
        IncentiveKind::Attribution,
        IncentiveKind::RevenueShare,
        IncentiveKind::Rating,
    ];

    /// 项序（定长槽位寻址——`ALL` 数组序即项序）。
    pub const fn order(self) -> usize {
        match self {
            IncentiveKind::Attribution => 0,
            IncentiveKind::RevenueShare => 1,
            IncentiveKind::Rating => 2,
        }
    }

    /// 项码。
    pub const fn code(self) -> &'static str {
        match self {
            IncentiveKind::Attribution => "INC-ATTR",
            IncentiveKind::RevenueShare => "INC-SHARE",
            IncentiveKind::Rating => "INC-RATE",
        }
    }

    /// 由码反查项。
    pub fn from_code(code: &str) -> Option<IncentiveKind> {
        IncentiveKind::ALL.iter().copied().find(|k| k.code() == code)
    }

    /// 项名（中文）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            IncentiveKind::Attribution => "署名",
            IncentiveKind::RevenueShare => "分成",
            IncentiveKind::Rating => "评级",
        }
    }

    /// 是否可被剥离（锚点 F3114：署名不可剥=盗包红线）。
    pub const fn strippable(self) -> bool {
        !matches!(self, IncentiveKind::Attribution)
    }
}

/// 单源侧（歧义二：两个侧面，不是一件事的两个来源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFacet {
    /// 生态语义侧（F3114）。
    Ecosystem,
    /// SDK 接入侧（F3347）。
    Sdk,
}

impl SourceFacet {
    /// 全部侧面。
    pub const FACETS: [SourceFacet; 2] = [SourceFacet::Ecosystem, SourceFacet::Sdk];

    /// 侧面名。
    pub const fn name_cn(self) -> &'static str {
        match self {
            SourceFacet::Ecosystem => "生态语义侧",
            SourceFacet::Sdk => "SDK 接入侧",
        }
    }

    /// 侧面单源条目号。
    pub const fn anchor(self) -> &'static str {
        match self {
            SourceFacet::Ecosystem => "F3114 风格生态与分享",
            SourceFacet::Sdk => "F3347 管线 SDK 与 DX",
        }
    }
}

/// 激励条款一条。
#[derive(Clone, Copy, Debug)]
pub struct IncentiveClause {
    /// 激励项。
    pub kind: IncentiveKind,
    /// 数据对端条目号（**每项恰一**，重复即 [`E_INCENTIVE_DUP`]）。
    pub owner_code: u32,
    /// 数据对端说明。
    pub owner_note: &'static str,
    /// 取项依据（署名→F3114 / 评级→F3109 / 分成→F3306，各有册内出处）。
    pub basis: &'static str,
}

impl IncentiveClause {
    /// 标准条款。
    pub fn standard() -> [IncentiveClause; INCENTIVE_COUNT] {
        [
            IncentiveClause {
                kind: IncentiveKind::Attribution,
                owner_code: 3114,
                owner_note: "F3114 署名元数据（不可剥离哈希）",
                basis: "锚点直陈：复用 F3114 模式；F3114 自身锚点写「署名元数据（不可剥离哈希）」",
            },
            IncentiveClause {
                kind: IncentiveKind::RevenueShare,
                owner_code: 3306,
                owner_note: "F3306 权限（结算对端）",
                basis: "转引：F3616 锚点跨批对接点写「F3306 权限（结算对端）」；本项只登记引用，不实现结算",
            },
            IncentiveClause {
                kind: IncentiveKind::Rating,
                owner_code: 3109,
                owner_note: "F3109 治理准入（评级）",
                basis: "转引：F3114 锚点写「评分进 F3109 评级」",
            },
        ]
    }

    /// 是否被第二处声明同一能力（单源纪律的核心检查）。
    pub fn collides_with(&self, other: &IncentiveClause) -> bool {
        self.kind == other.kind && self.owner_code != other.owner_code
    }
}

/// 激励总表。
#[derive(Clone, Copy, Debug)]
pub struct IncentiveLedger {
    /// 三项条款。
    pub clauses: [IncentiveClause; INCENTIVE_COUNT],
    /// SDK 接入侧单源条目（应只有 F3347 一处声明）。
    pub sdk_source: Option<u32>,
    /// 生态语义侧单源条目（应只有 F3114 一处声明）。
    pub eco_source: Option<u32>,
}

impl IncentiveLedger {
    /// 标准总表。
    pub fn standard() -> IncentiveLedger {
        IncentiveLedger {
            clauses: IncentiveClause::standard(),
            sdk_source: Some(3347),
            eco_source: Some(3114),
        }
    }

    /// 空总表（反例：无单源登记）。
    pub fn empty() -> IncentiveLedger {
        IncentiveLedger {
            clauses: IncentiveClause::standard(),
            sdk_source: None,
            eco_source: None,
        }
    }

    /// 查项条款。
    pub fn get(&self, kind: IncentiveKind) -> &IncentiveClause {
        &self.clauses[kind.order()]
    }

    /// 可变查项条款。
    pub fn get_mut(&mut self, kind: IncentiveClauseKindAlias) -> &mut IncentiveClause {
        &mut self.clauses[kind.order()]
    }

    /// 单源审计（每项恰一属主/ 无主检出 / 侧面单源唯一）。
    pub fn audit(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        for (i, c) in self.clauses.iter().enumerate() {
            if c.kind.order() != i {
                out.push(EcoIssue::new(
                    E_INCENTIVE_COUNT,
                    c.kind.code(),
                    format!("项位错：位置 {} 应为序{}", i, c.kind.order()),
                ));
            }
            if c.owner_code == 0 {
                out.push(EcoIssue::new(
                    E_INCENTIVE_NO_OWNER,
                    c.kind.code(),
                    format!("{} 无数据对端——激励无处兑现", c.kind.name_cn()),
                ));
            }
            if c.basis.trim().is_empty() {
                out.push(EcoIssue::new(
                    E_INCENTIVE_NO_BASIS,
                    c.kind.code(),
                    "激励项无取项依据（转引也须写明册内出处）",
                ));
            }
            // 单源：同一 kind 不得指向两个不同对端。
            for other in self.clauses.iter() {
                if c.collides_with(other) {
                    out.push(EcoIssue::new(
                        E_INCENTIVE_DUP,
                        c.kind.code(),
                        format!(
                            "{} 同时指向 F{} 与 F{}——单源纪律破，须对拍",
                            c.kind.name_cn(),
                            c.owner_code,
                            other.owner_code
                        ),
                    ));
                }
            }
            // 署名不可剥。
            if c.kind == IncentiveKind::Attribution && c.owner_code == 0 {
                out.push(EcoIssue::new(
                    E_ATTRIBUTION_STRIPPABLE,
                    c.kind.code(),
                    "署名为空即等于可剥（锚点：署名不可剥=盗包红线）",
                ));
            }
        }
        // 侧面单源：SDK 侧与生态侧各只许一处声明。
        if self.sdk_source.is_none() {
            out.push(EcoIssue::new(
                E_SDK_SOURCE_SPLIT,
                "facet-sdk",
                "SDK 接入侧单源（F3347）未登记——第三方接入将各写一份",
            ));
        }
        if self.eco_source.is_none() {
            out.push(EcoIssue::new(
                E_INCENTIVE_NO_OWNER,
                "facet-eco",
                "生态语义侧单源（F3114）未登记",
            ));
        }
        out
    }

    /// 侧面单源对账：两侧条目不同（若相同即两侧其实是一件事，登记有误）。
    pub fn facets_distinct(&self) -> bool {
        match (self.eco_source, self.sdk_source) {
            (Some(e), Some(s)) => e != s,
            _ => false,
        }
    }
}

/// 激励项的可变索引别名（保持 `get_mut` 签名可读）。
pub type IncentiveClauseKindAlias = IncentiveKind;

// ---------------------------------------------------------------------------
// 七、收敛复述（判据五·只有两段走管线，见头注 §6）
// ---------------------------------------------------------------------------

/// 收敛点一条（段是否经 Q 管线）。
#[derive(Clone, Copy, Debug)]
pub struct ConvergencePoint {
    /// 段。
    pub stage: EcoStage,
    /// 是否经 Q 管线。
    pub via_pipeline: bool,
    /// 取项依据（写明这是推导+册内出处）。
    pub basis: &'static str,
}

impl ConvergencePoint {
    /// 标准收敛点（**仅打包与分发两段**走管线）。
    pub fn standard() -> [ConvergencePoint; CONVERGENCE_POINT_COUNT] {
        [
            ConvergencePoint {
                stage: EcoStage::Package,
                via_pipeline: true,
                basis: "推导+转引：产出物 asset_pkg 对外交付→经 Q 管线；F3616 锚点「走 Q 管线打包分发（复述收敛）」",
            },
            ConvergencePoint {
                stage: EcoStage::Distribute,
                via_pipeline: true,
                basis: "转引：F3616 锚点错误路径写「绕管线分发→立案（收敛复述）」",
            },
        ]
    }
}

/// 收敛总表。
#[derive(Clone, Copy, Debug)]
pub struct ConvergenceLedger {
    /// 收敛点。
    pub points: [ConvergencePoint; CONVERGENCE_POINT_COUNT],
}

impl ConvergenceLedger {
    /// 标准总表。
    pub fn standard() -> ConvergenceLedger {
        ConvergenceLedger { points: ConvergencePoint::standard() }
    }

    /// 空总表（反例：无收敛点=默认谁都可不走管线）。
    pub fn empty() -> ConvergenceLedger {
        ConvergenceLedger {
            points: [
                ConvergencePoint {
                    stage: EcoStage::Package,
                    via_pipeline: false,
                    basis: "",
                },
                ConvergencePoint {
                    stage: EcoStage::Distribute,
                    via_pipeline: false,
                    basis: "",
                },
            ],
        }
    }

    /// 某段是否走管线。
    pub fn via_pipeline(&self, stage: EcoStage) -> Option<bool> {
        self.points
            .iter()
            .find(|p| p.stage == stage)
            .map(|p| p.via_pipeline)
    }

    /// 收敛审计（交付段绕管线即P0 / 本地段误走管线 / 依据缺失）。
    pub fn audit(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        // 交付段必须走管线；本地段必须不走——两向都查，防止「全走」或「全不走」。
        for s in EcoStage::ALL.iter() {
            match (self.via_pipeline(*s), s.delivers_externally()) {
                (Some(true), false) => out.push(EcoIssue::new(
                    E_PIPELINE_LOCAL_LEAK,
                    s.code(),
                    "本地段误标走正式管线——会污染正式缓存（与 F3605 预览隔离冲突）",
                )),
                (Some(false), true) => out.push(EcoIssue::new(
                    E_PIPELINE_BYPASS,
                    s.code(),
                    "交付段绕 Q 管线——收敛红线破（转引 F3616）",
                )),
                _ => {}
            }
        }
        for p in self.points.iter() {
            if p.basis.trim().is_empty() {
                out.push(EcoIssue::new(
                    E_CONVERGENCE_NO_BASIS,
                    p.stage.code(),
                    "收敛点无取项依据",
                ));
            }
        }
        out
    }
}// ---------------------------------------------------------------------------
// 八、错误路径与降级矩阵（判据：三条各有动作）
// ---------------------------------------------------------------------------

/// 降级动作（处置方向不同，不共用一套）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeAction {
    /// 阻断分发（红线实测类）。
    BlockRelease,
    /// 对拍/对账（分歧类）。
    Reconcile,
    /// 立案（观察类）。
    FileCase,
}

impl DegradeAction {
    /// 全部动作。
    pub const ALL: [DegradeAction; 3] = [
        DegradeAction::BlockRelease,
        DegradeAction::Reconcile,
        DegradeAction::FileCase,
    ];

    /// 动作码。
    pub const fn code(self) -> &'static str {
        match self {
            DegradeAction::BlockRelease => "A-BLOCK",
            DegradeAction::Reconcile => "A-RECONCILE",
            DegradeAction::FileCase => "A-FILECASE",
        }
    }

    /// 动作名（中文，读屏用）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            DegradeAction::BlockRelease => "阻断分发",
            DegradeAction::Reconcile => "对拍对账",
            DegradeAction::FileCase => "立案观察",
        }
    }

    /// 动作是否阻断。
    pub const fn blocks(self) -> bool {
        matches!(self, DegradeAction::BlockRelease)
    }
}

/// 降级路径一条（锚点错误路径矩阵逐条）。
#[derive(Clone, Copy, Debug)]
pub struct DegradePath {
    /// 触发错误码。
    pub trigger: &'static str,
    /// 严重级。
    pub severity: &'static str,
    /// 降级动作。
    pub action: DegradeAction,
    /// 锚点原文摘录。
    pub quote: &'static str,
}

impl DegradePath {
    /// 锚点三条路径（原文照录）。
    pub fn standard() -> [DegradePath; DEGRADE_PATH_COUNT] {
        [
            DegradePath {
                trigger: E_FORMAT_LOCKED,
                severity: P0,
                action: DegradeAction::BlockRelease,
                quote: "格式锁定检出→P0（红线实测——生态执法）",
            },
            DegradePath {
                trigger: E_SANDBOX_BYPASS,
                severity: P0,
                action: DegradeAction::BlockRelease,
                quote: "沙箱绕过→P0（复述）",
            },
            DegradePath {
                trigger: E_INCENTIVE_DUP,
                severity: P1,
                action: DegradeAction::Reconcile,
                quote: "激励分歧→对拍（复述）",
            },
        ]
    }

    /// 指纹（触发码 + 级+ 动作）。
    pub fn fingerprint(&self) -> u64 {
        let buf = format!("{}|{}|{}", self.trigger, self.severity, self.action.code());
        fnv1a64(buf.as_bytes())
    }
}

/// 降级矩阵。
#[derive(Clone, Copy, Debug)]
pub struct DegradeMatrix {
    /// 三条路径。
    pub paths: [DegradePath; DEGRADE_PATH_COUNT],
}

impl DegradeMatrix {
    /// 标准矩阵。
    pub fn standard() -> DegradeMatrix {
        DegradeMatrix { paths: DegradePath::standard() }
    }

    /// 查触发码对应路径。
    pub fn by_trigger(&self, trigger: &str) -> Option<&DegradePath> {
        self.paths.iter().find(|p| p.trigger == trigger)
    }

    /// 矩阵审计（缺路径 / 级与动作不匹配 / 引文为空）。
    pub fn audit(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        let want = DegradePath::standard();
        for w in want.iter() {
            match self.by_trigger(w.trigger) {
                None => out.push(EcoIssue::new(
                    E_DEGRADE_MISSING,
                    w.trigger,
                    format!("锚点错误路径缺失：{}", w.quote),
                )),
                Some(p) => {
                    if p.severity != w.severity || p.action != w.action {
                        out.push(EcoIssue::new(
                            E_DEGRADE_MISMATCH,
                            w.trigger,
                            format!(
                                "级/动作为 {} / {}，锚点为 {} / {}",
                                p.severity,
                                p.action.name_cn(),
                                w.severity,
                                w.action.name_cn()
                            ),
                        ));
                    }
                    if p.quote.trim().is_empty() {
                        out.push(EcoIssue::new(
                            E_DEGRADE_MISSING,
                            w.trigger,
                            "降级路径无锚点引文",
                        ));
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 九、禁扩面（只声明架构，不替别人实现）
// ---------------------------------------------------------------------------

/// 禁扩面条目（码 → 说明）。
pub const ECO_EXCLUSIONS: [(&str, &str); EXCLUSION_COUNT] = [
    ("F3603:model", "七要素模型字段与schema 校验器归 F3603"),
    ("F3616:listing", "上架流水/定价模型/合规表归 F3616"),
    ("F3607:verify", "验证器五段实现归 F3607"),
    ("F3204:registry", "资源类型注册表实现归 F3204"),
    ("F3114:library", "生态库检索/下载/反刷归 F3114"),
    ("F3306:settle", "结算与收益明细实现归 F3306"),
    ("Q:body", "Q 管线本体与沙箱本体均不在本项实现"),
];

/// 越界建议（给定意图串，返回是否越界与建议）。
pub fn eco_scope_advice(intent: &str) -> Option<&'static str> {
    ECO_EXCLUSIONS
        .iter()
        .find(|(code, _)| intent.contains(code.split(':').next().unwrap_or("")))
        .map(|(_, note)| *note)
}

/// 越界判定（命中即 [`E_OVERREACH`]）。
pub fn check_no_overreach(intent: &str) -> Result<&'static str, EcoError> {
    match eco_scope_advice(intent) {
        Some(note) => Err(EcoError::new(
            E_OVERREACH,
            intent,
            format!("越界：{}（本项只声明架构）", note),
        )),
        None => Ok("在本项声明范围内"),
    }
}

// ---------------------------------------------------------------------------
// 十、判据（锚点原文六项）
// ---------------------------------------------------------------------------

/// 判据一条。
#[derive(Clone, Copy, Debug)]
pub struct Criterion {
    /// 判据码。
    pub code: &'static str,
    /// 判据名（锚点原文用词）。
    pub title: &'static str,
    /// 锚点依据（原文或明确的推导出处）。
    pub basis: &'static str,
    /// 机检锚点（对应自检项前缀）。
    pub probe: &'static str,
}

impl Criterion {
    /// 全部判据（锚点原文六项，逐条照录）。
    pub const ALL: [Criterion; CRITERION_COUNT] = [
        Criterion {
            code: "C1",
            title: "三层五段",
            basis: "锚点原文：创作生态三层 + 架构五段接口（创建→编辑→验证→打包→分发）",
            probe: "R01-三层五段",
        },
        Criterion {
            code: "C2",
            title: "开放格式 P0",
            basis: "锚点原文：锁定格式=生态死刑 + 格式锁定检出→P0（红线实测）",
            probe: "R01-开放格式",
        },
        Criterion {
            code: "C3",
            title: "激励单源",
            basis: "锚点原文：复用 F3114 模式跨域单源声明；跨批对接点 F3114/F3347 双单源",
            probe: "R01-激励单源",
        },
        Criterion {
            code: "C4",
            title: "沙箱复述",
            basis: "锚点原文：创作代码类资产（脚本/逻辑）沙箱执行——隔离复述",
            probe: "R01-沙箱复述",
        },
        Criterion {
            code: "C5",
            title: "收敛复述",
            basis: "判据原文「收敛复述」；正文对应「Q 管线（打包分发对端）」+ F3616「绕管线分发→立案（收敛复述）」",
            probe: "R01-收敛复述",
        },
        Criterion {
            code: "C6",
            title: "判据",
            basis: "锚点判据列表末项——判据本身须可机检、可追溯",
            probe: "R01-判据",
        },
    ];

    /// 判据指纹（六项逐条拼接）。
    pub fn ledger_fingerprint() -> u64 {
        let mut buf = String::new();
        for c in Criterion::ALL.iter() {
            buf.push_str(c.code);
            buf.push(':');
            buf.push_str(c.title);
            buf.push(';');
        }
        fnv1a64(buf.as_bytes())
    }
}

/// 判据审计（缺失 / 无依据）。
pub fn audit_criteria() -> Vec<EcoIssue> {
    let mut out = Vec::new();
    if Criterion::ALL.len() != CRITERION_COUNT {
        out.push(EcoIssue::new(
            E_CRITERION_MISSING,
            "criteria",
            format!("判据数 {} ≠ {}", Criterion::ALL.len(), CRITERION_COUNT),
        ));
    }
    for c in Criterion::ALL.iter() {
        if c.title.trim().is_empty() {
            out.push(EcoIssue::new(E_CRITERION_MISSING, c.code, "判据无标题"));
        }
        if c.basis.trim().is_empty() {
            out.push(EcoIssue::new(
                E_CRITERION_NO_BASIS,
                c.code,
                "判据无锚点依据（推导也须写明出处）",
            ));
        }
        if c.probe.trim().is_empty() {
            out.push(EcoIssue::new(E_CRITERION_NO_BASIS, c.code, "判据无机检锚点"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 十一、总纲本体
// ---------------------------------------------------------------------------

/// 创作生态总架构（总纲聚合）。
#[derive(Clone, Copy, Debug)]
pub struct EcosystemArchitecture {
    /// 三层。
    pub layers: LayerStack,
    /// 五段签名链。
    pub stages: StageChain,
    /// 段→层映射。
    pub map: StageLayerMap,
    /// 开放格式总表。
    pub formats: FormatRegistry,
    /// 沙箱总表。
    pub sandbox: SandboxLedger,
    /// 激励总表。
    pub incentives: IncentiveLedger,
    /// 收敛总表。
    pub convergence: ConvergenceLedger,
    /// 降级矩阵。
    pub degrade: DegradeMatrix,
}

impl EcosystemArchitecture {
    /// 标准总纲（**唯一正样本构造处**——自检模块不各自造好看的数据）。
    pub fn standard() -> EcosystemArchitecture {
        let mut tool = LayerSpec::empty(EcoLayer::Tool);
        // 工具层对端 R02/R03（册内：R02=VE-F3621~F3640，R03=VE-F3641~F3660）。
        let _ = tool.push_peer(3621, "R02 主题编辑器内核组（F3621-F3640）");
        let _ = tool.push_peer(3641, "R03 壁纸与视觉工坊组（F3641-F3660）");
        tool.duty = EcoLayer::Tool.duty();

        let mut asset = LayerSpec::empty(EcoLayer::Asset);
        let _ = asset.push_peer(3603, "创作资产模型（紧随本项的下一条目）");
        asset.duty = EcoLayer::Asset.duty();

        let mut dist = LayerSpec::empty(EcoLayer::Distribution);
        let _ = dist.push_peer(3616, "创作与市场分发");
        dist.duty = EcoLayer::Distribution.duty();

        EcosystemArchitecture {
            layers: LayerStack { layers: [tool, asset, dist] },
            stages: StageChain::standard(),
            map: StageLayerMap::standard(),
            formats: FormatRegistry::standard(),
            sandbox: SandboxLedger::standard(),
            incentives: IncentiveLedger::standard(),
            convergence: ConvergenceLedger::standard(),
            degrade: DegradeMatrix::standard(),
        }
    }

    /// 空总纲（反例用）。
    pub fn empty() -> EcosystemArchitecture {
        EcosystemArchitecture {
            layers: LayerStack::empty(),
            stages: StageChain::empty(),
            map: StageLayerMap::all_tool(),
            formats: FormatRegistry::empty(),
            sandbox: SandboxLedger::standard(),
            incentives: IncentiveLedger::empty(),
            convergence: ConvergenceLedger::empty(),
            degrade: DegradeMatrix::standard(),
        }
    }

    /// 开工前置校验（不通过即 `Err`，阻断开工）。
    pub fn preflight(&self) -> Result<(), EcoError> {
        let issues = self.collect_issues();
        // 取第一条阻断项（顺序即严重度序：层→段→映射→格式→沙箱→激励→收敛→降级）。
        for i in issues.iter() {
            if i.is_redline() {
                return Err(EcoError::new(i.code, i.subject.clone(), i.detail.clone()));
            }
        }
        if let Some(i) = issues.first() {
            return Err(EcoError::new(i.code, i.subject.clone(), i.detail.clone()));
        }
        Ok(())
    }

    /// 收集全部契约问题（机检一次性枚举，不逐个 early-return 掩盖后续）。
    pub fn collect_issues(&self) -> Vec<EcoIssue> {
        let mut out = Vec::new();
        out.extend(self.layers.audit());
        out.extend(self.stages.check_frozen());
        out.extend(self.stages.audit());
        out.extend(self.map.audit());
        out.extend(self.formats.audit());
        out.extend(self.sandbox.audit());
        out.extend(self.incentives.audit());
        out.extend(self.convergence.audit());
        out.extend(self.degrade.audit());
        out.extend(audit_criteria());
        out
    }

    /// 架构指纹（八张表逐表串接——任一改动即变）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = String::new();
        for l in self.layers.layers.iter() {
            buf.push_str(&alloc::format!("{:016x}", l.fingerprint()));
        }
        for s in self.stages.stages.iter() {
            buf.push_str(&alloc::format!("{:016x}", s.fingerprint()));
        }
        for o in self.map.owner.iter() {
            buf.push_str(o.code());
        }
        for f in self.formats.items.iter() {
            buf.push_str(f.class.code());
            buf.push_str(if f.locked() { "L" } else { "O" });
        }
        for s in self.sandbox.items.iter() {
            buf.push_str(s.class.code());
            buf.push_str(if s.bypassed { "X" } else { "." });
        }
        for c in self.incentives.clauses.iter() {
            buf.push_str(c.kind.code());
            buf.push_str(&alloc::format!("{}", c.owner_code));
        }
        for p in self.convergence.points.iter() {
            buf.push_str(p.stage.code());
            buf.push_str(if p.via_pipeline { "Q" } else { "-" });
        }
        for p in self.degrade.paths.iter() {
            buf.push_str(&alloc::format!("{:016x}", p.fingerprint()));
        }
        fnv1a64(buf.as_bytes())
    }

    /// 总纲自检（契约+ 六判据登记）。
    pub fn run_checks(&self) -> CheckSet {
        super::ver03_checks::run_ver03_checks()
    }

    /// 读屏替述（无障碍等价口述——不依赖图形，逐条可复述）。
    pub fn narration(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "创作生态共三层：{}。",
            EcoLayer::ALL
                .iter()
                .map(|l| format!("{}管{}", l.name_cn(), l.duty()))
                .collect::<Vec<_>>()
                .join("；")
        ));
        out.push(format!(
            "五段接口签名冻结在{}：{}。上一段的产出就是下一段的入参。",
            STAGE_SIG_VERSION,
            EcoStage::ALL
                .iter()
                .map(|s| format!("{}产出{}", s.name_cn(), s.result()))
                .collect::<Vec<_>>()
                .join("，")
        ));
        out.push(
            "开放格式是生死线：锁定格式等于生态死刑，因为用户攒下的创作成果带不走；\
             所以十类资源每类都必须可导出、可迁入、有公开规范，缺一项按 P0 阻断分发。"
                .to_string(),
        );
        out.push(
            "代码类资产（脚本数据）必须在沙箱里跑，本项只引用不另写一份沙箱；\
             非代码类不挂沙箱，避免无谓开销；绕过沙箱按 P0 处置。"
                .to_string(),
        );
        out.push(
            "激励三项各有唯一数据对端：署名归 F3114（且不可剥离，剥了就是盗包）、\
             评级归 F3109、分成归 F3306。F3114 与 F3347 不是同一件事的两个来源，\
             前者是生态语义侧单源，后者是 SDK 接入侧单源。"
                .to_string(),
        );
        out.push(
            "只有打包与分发两段经 Q 管线，因为只有这两段对外交付；创建、编辑、验证是本地态，\
             硬塞进正式管线会污染正式缓存。"
                .to_string(),
        );
        out
    }
}

/// VE-F3602 域自检（判据逐条映射见 `ver03_checks.rs`）。
///
/// 聚合表经本模块取自检（与 [`ver02_arch`](super::ver02_arch) 同一约定）：
/// 这样聚合面只认「域开工模块」，不认检查模块，检查实现挪位不影响总表。
pub fn run_ver03_checks() -> CheckSet {
    super::ver03_checks::run_ver03_checks()
}
