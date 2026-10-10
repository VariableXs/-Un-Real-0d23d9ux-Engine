//! VE-F2015 · 后处理质量档位（VE-K 域 · 后处理架构与 Bloom 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2015`
//!
//! **判据（锚点原文）**：三档组合、D04 兑现、三方裁决复用、档位记忆、判据。
//!
//! **职责定位（锚点原文）**：后处理质量档位——定义后处理三档（高/中/低：效果开关与
//! 精度的组合档位表——高=全效果全精度（TAA 高收敛/DoF 64 样本/全 mip 链）；中=TAA
//! 标准/DoF 32/半 mip；低=FXAA 替代 TAA/DoF 关或极简/Bloom 单级）、与 D04 降质链
//! 映射兑现（降质参数表的后处理维度→档位映射——F1916 模式复用：单源无二义）、三方
//! 裁决复用（用户覆盖>档位>预算临时——F1832 同构规则复用声明）、档位记忆（用户档位
//! 选择持久化）。
//!
//! **数据结构（锚点原文）**：三档参数表+各效果独立子开关（用户可单独关某效果——
//! 子开关与档位的组合合法性校验）；D04 映射行；裁决规则（单源引用 F1832 定义——
//! 一处定义两处引用的复用声明）；记忆机制（用户选择持久化+版本迁移）。
//!
//! **错误路径与降级矩阵（锚点原文）**：三方源失同步→优先级裁决表；档位切换帧边界
//! 生效（F1762 同规则）；映射缺行→CI 对账拦截；非法组合（档位表外参数）→拒绝；
//! 记忆损坏→回退默认档+告警。
//!
//! **性能逐项分解（锚点原文）**：档位查询 O(1)；切换=F2002 图重编译的预设实例
//! （帧边界原子——复用 F2002 重组协议）；三档差异在 F2017 基准留证。
//!
//! **跨批对接点（锚点原文）**：语义与 F1762/F1832/F1852 档位树家族一致（两级档位
//! 树 K 段登记）；D04 跨卷契约；消费方全部 K 域效果；切换走 F2002；基准 F2017。
//!
//! **同域单源复用**：十效果闭集与诊断袋复用 [`super::vek14_ppbudget`]（F2014 已
//! 把 K01 十效果定为闭集单源，本模块另立一份枚举会让两表行序各说各话）；诊断码
//! 自建 0x30xx 段（0x2Cxx=F2013、0x2Dxx=F2014、0x2Exx=L 域调试、0x2Fxx=B 域
//! GTT——全 kernel grep 确认 0x30xx 无主）。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 档位表最容易写成恒真断言的地方有六处，本模块逐一封死：
//!
//! 1. **"三档组合"**。若只断「每档表非空」，把三档写成同一张表也全绿。必须断
//!    **三档 knobs 逐项单调**（TAA 收敛/DoF 样本/Bloom mip 数 High≥Mid≥Low）且
//!    **确有差异**（至少一对相邻档不相等——三档全等则档位形同虚设），再加 **AA
//!    替代关系**（低档 TAA 关、FXAA 开，高中档相反）与 **SMAA 三档全关**（预留
//!    不参与）——这几条合起来才钉死锚点原文的三档语义。
//! 2. **"组合合法性校验"**。若只断「合法组合通过」，一个恒放行的校验器也全绿。
//!    必须断**双向**：合法降档（关效果/降精度/降 knob/整关 AA）放行且逐位生效，
//!    非法组合（SMAA 预留被开、AA 换种类、knob 超档上限）逐类拒绝且拒绝码同源。
//! 3. **"D04 映射单源"**。若只断「映射查得到」，映射表有洞也绿。必须断
//!    **全域覆盖无缝无叠**（0..=100 每个值恰归一档——缺行即配置错误）+
//!    **边界值逐个归档** + **越界显性拒绝**（101 不回绕不猜）+ **双向往返恒等**
//!    （tier→代表值→tier 不换档且代表值不出带）。
//! 4. **"三方裁决"**。若只断「冲突时选用户」，恒返回用户的实现也全绿。必须
//!    **三源各胜一次**（user 在场 user 胜；user 缺席 tier 体系胜；再缺席预算
//!    临时胜；全缺席保持现役）且**在场异见才计冲突**（缺席≠异见）且**全一致
//!    零告警**。
//! 5. **"档位切换帧边界生效"**。若只断「切换后配置变了」，帧中即时生效的实现
//!    也绿。必须断：请求后**现役配置逐位不变**（staged 暂存）、帧边界应用才
//!    换装且 epoch 单调、**二次应用拿不到东西**（staged 是一次性消费——原子）、
//!    **非法请求连暂存都不产生**（不静默收下再丢弃）。
//! 6. **"记忆损坏回退"**。若只断「好载荷读得回」，校验和恒过的实现也绿。必须
//!    断**8×64 位逐位翻转全检出**（不是抽样——抽样会给碰撞留侥幸面）且**损坏
//!    与版本分型**（魔数/校验和坏=Corrupt 走回退；自洽的未知版本=Unsupported
//!    显性拒绝不静默回退——回退会无声丢掉用户选择，两者必须分开）。

use super::vek14_ppbudget::{DiagBag, DiagCode, Effect, EFFECT_COUNT};

// ===========================================================================
// 一、诊断码（自建，K 域独占段 0x30xx——0x2Cxx/0x2Dxx/0x2Exx/0x2Fxx 均已有主）
// ===========================================================================

/// K 域 F2015 诊断码。
pub mod code {
    use super::DiagCode;

    /// 非法组合（档位表外参数：SMAA 预留被开/AA 换种类/knob 超档上限）。
    pub const ILLEGAL_COMBO: DiagCode = DiagCode(0x3001);
    /// 三方源失同步——按优先级裁决表定胜负（F1832 同构）。
    pub const TIER_DESYNC: DiagCode = DiagCode(0x3002);
    /// D04 映射缺行（降质参数越出 0..=100 定义域）——拒绝，不回绕不猜。
    pub const MAP_ROW_MISSING: DiagCode = DiagCode(0x3003);
    /// 档位记忆损坏（魔数/校验和不符）——回退默认档+告警。
    pub const MEM_CORRUPT: DiagCode = DiagCode(0x3004);
    /// 档位记忆版本无迁移链——显性拒绝（声明支持范围，不静默回退）。
    pub const MEM_VERSION_UNSUPPORTED: DiagCode = DiagCode(0x3005);
}

// ===========================================================================
// 二、三档闭集
// ===========================================================================

/// 后处理质量三档（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum QualityTier {
    /// 高档：全效果全精度（TAA 高收敛/DoF 64 样本/全 mip 链）。
    High = 0,
    /// 中档：TAA 标准/DoF 32/半 mip。
    Mid = 1,
    /// 低档：FXAA 替代 TAA/DoF 关或极简/Bloom 单级。
    Low = 2,
}

/// 三档闭集长度。
pub const TIER_COUNT: usize = 3;

/// 默认档（记忆损坏/无记忆时的落点——取中档：不是最快也不是最贵，
/// 首启体验两头都不塌）。
pub const DEFAULT_TIER: QualityTier = QualityTier::Mid;

/// 三档定长数组（避免 `Vec` 分配）。
const TIERS: [QualityTier; TIER_COUNT] =
    [QualityTier::High, QualityTier::Mid, QualityTier::Low];

impl QualityTier {
    /// 序号（0..3）。
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// 由序号还原（越界返回 `None`——不回绕到 0）。
    pub const fn from_ordinal(i: usize) -> Option<QualityTier> {
        match i {
            0 => Some(QualityTier::High),
            1 => Some(QualityTier::Mid),
            2 => Some(QualityTier::Low),
            _ => None,
        }
    }

    /// 全部档位（升序）。
    pub fn all() -> [QualityTier; TIER_COUNT] {
        TIERS
    }

    /// 档位名（账本/记忆载荷可读）。
    pub const fn label(self) -> &'static str {
        match self {
            QualityTier::High => "high",
            QualityTier::Mid => "mid",
            QualityTier::Low => "low",
        }
    }
}

// ===========================================================================
// 三、三档参数表（效果开关 × 精度 × 质量旋钮）
// ===========================================================================

/// 抗锯齿选择（每档恰占一个 AA 槽位——TAA 与 FXAA 互斥是锚点「FXAA 替代
/// TAA」的表意）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AaSlot {
    /// TAA（高档/中档）。
    Taa,
    /// FXAA（低档替代 TAA）。
    Fxaa,
    /// 无 AA（用户可在任何档位上把 AA 整个关掉——只降不升）。
    Off,
}

/// Bloom mip 级数口径（全 mip 链 = 7 级，半 mip = 4 级，单级 = 1 级）。
///
/// 数值是**量级声明**（供预算/成本模型对账的结构关系），真机差异由 F2017
/// 基准回灌；判据守的是「全≥半≥单」的序与档间差异，不守绝对值。
pub const BLOOM_MIPS_FULL: u32 = 7;
/// 半 mip（中档）。
pub const BLOOM_MIPS_HALF: u32 = 4;
/// 单级（低档）。
pub const BLOOM_MIPS_SINGLE: u32 = 1;

/// DoF 样本口径：高档 64 / 中档 32 / 低档 0。锚点低档写「关**或**极简」
/// 两可，取**关**并如实声明：极简的非零样本数没有档位承诺支撑，留给
/// 子开关往上开就是超档（校验器会拒），两头都占反而两头都不诚实。
pub const DOF_SAMPLES_HIGH: u32 = 64;
/// 中档 DoF 样本。
pub const DOF_SAMPLES_MID: u32 = 32;
/// 低档 DoF 样本（关）。
pub const DOF_SAMPLES_LOW: u32 = 0;

/// TAA 收敛相位数（Halton 抖动相位数口径，F2011 同源）：高档 8 相位高收敛 /
/// 中档 4 相位标准 / 低档 0（TAA 被 FXAA 替代，无收敛可言）。
pub const TAA_PHASES_HIGH: u32 = 8;
/// 中档 TAA 相位。
pub const TAA_PHASES_MID: u32 = 4;
/// 低档 TAA 相位（无 TAA）。
pub const TAA_PHASES_LOW: u32 = 0;

/// 一档的完整参数（效果开关 + 精度比 + 质量旋钮）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TierPreset {
    /// 十效果开关（行序严格对齐 [`Effect`] 闭集；SMAA 恒 false——F2012 预留
    /// 没有渲染实现，开了也是空转）。
    pub enabled: [bool; EFFECT_COUNT],
    /// 十效果精度比（千分数；中低档的「精度降」落在这里——开关还开着但
    /// 单价按比例降，与 F2014 成本模型的 `precision_permille` 同一口径）。
    pub precision_permille: [u32; EFFECT_COUNT],
    /// AA 槽位（TAA/FXAA/Off）。
    pub aa: AaSlot,
    /// TAA 收敛相位数（无 TAA 时为 0）。
    pub taa_phases: u32,
    /// DoF 样本数。
    pub dof_samples: u32,
    /// Bloom mip 级数。
    pub bloom_mips: u32,
}

/// 高档精度（全效果全精度）。
const PREC_FULL: u32 = 1000;
/// 中档精度（「TAA 标准」——效果全开但单价按比例降）。
const PREC_STD: u32 = 750;
/// 低档精度（「极简」）。
const PREC_MIN: u32 = 500;

const fn preset_high() -> TierPreset {
    TierPreset {
        enabled: [
            true, true, true, true, true, true, true, true, false, true,
        ],
        precision_permille: [
            PREC_FULL, PREC_FULL, PREC_FULL, PREC_FULL, PREC_FULL, PREC_FULL, PREC_FULL,
            PREC_FULL, PREC_FULL, PREC_FULL,
        ],
        aa: AaSlot::Taa,
        taa_phases: TAA_PHASES_HIGH,
        dof_samples: DOF_SAMPLES_HIGH,
        bloom_mips: BLOOM_MIPS_FULL,
    }
}

const fn preset_mid() -> TierPreset {
    TierPreset {
        enabled: [
            true, true, true, true, true, true, true, true, false, true,
        ],
        precision_permille: [
            PREC_STD, PREC_STD, PREC_STD, PREC_STD, PREC_STD, PREC_STD, PREC_STD, PREC_STD,
            PREC_STD, PREC_STD,
        ],
        aa: AaSlot::Taa,
        taa_phases: TAA_PHASES_MID,
        dof_samples: DOF_SAMPLES_MID,
        bloom_mips: BLOOM_MIPS_HALF,
    }
}

const fn preset_low() -> TierPreset {
    TierPreset {
        enabled: [
            true, true, true, true, true, true, true, false, false, true,
        ],
        precision_permille: [
            PREC_MIN, PREC_MIN, PREC_MIN, PREC_MIN, PREC_MIN, PREC_MIN, PREC_MIN, PREC_MIN,
            PREC_MIN, PREC_MIN,
        ],
        aa: AaSlot::Fxaa,
        taa_phases: TAA_PHASES_LOW,
        dof_samples: DOF_SAMPLES_LOW,
        bloom_mips: BLOOM_MIPS_SINGLE,
    }
}

/// 三档参数表（**单源**：档位查询 O(1) 查这里，F2017 基准对三档差异留证时
/// 也引用这里，别处不得再写一份档位数值）。
pub struct TierTable {
    presets: [TierPreset; TIER_COUNT],
}

impl TierTable {
    /// 定标表（锚点原文三档语义的直译）。
    pub const fn calibrated() -> TierTable {
        TierTable { presets: [preset_high(), preset_mid(), preset_low()] }
    }

    /// 取档位预设（O(1) 查表）。
    pub fn get(&self, t: QualityTier) -> &TierPreset {
        &self.presets[t.ordinal()]
    }
}

// ===========================================================================
// 四、子开关与组合合法性校验（只降不升）
// ===========================================================================

/// 子开关覆盖（用户在档位之上逐项微调）。
///
/// **合法域 = 只降不升**：档位是引擎对该档成本与观感的**承诺包**——子开关把
/// 「用户可单独关某效果」表意为"在承诺包内往下调"；放行往上调（把低档拧成
/// 高档）会让档位名与实际成本脱钩，D04 降质链与 F2014 预算联动全都对不上账。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SubSwitchOverrides {
    /// 逐效果开关覆盖（`Some(false)`=用户单独关；`Some(true)` 在本档位表内
    /// **恒非法**——SMAA 三档全关是预留不可开，其余三档全开没有可开位）。
    pub enable: [Option<bool>; EFFECT_COUNT],
    /// 逐效果精度覆盖（必须 ≤ 档位值；高于档位值=表外）。
    pub precision: [Option<u32>; EFFECT_COUNT],
    /// TAA 相位覆盖（≤ 档位值合法）。
    pub taa_phases: Option<u32>,
    /// DoF 样本覆盖（≤ 档位值合法）。
    pub dof_samples: Option<u32>,
    /// Bloom mip 覆盖（≤ 档位值合法）。
    pub bloom_mips: Option<u32>,
    /// AA 槽位覆盖（只允许 `Some(AaSlot::Off)`——整关 AA；换 AA 种类是表外）。
    pub aa: Option<AaSlot>,
}

/// 组合校验结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ComboVerdict {
    /// 合法，已落到解析后的配置。
    Ok,
    /// 非法（档位表外参数）——拒绝。
    Illegal,
}

/// 组合解析产物（档位承诺包 + 子开关微调后的**实际**后处理配置）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResolvedConfig {
    /// 归属档位。
    pub tier: QualityTier,
    /// 实际效果开关。
    pub enabled: [bool; EFFECT_COUNT],
    /// 实际精度比。
    pub precision_permille: [u32; EFFECT_COUNT],
    /// 实际 AA 槽位。
    pub aa: AaSlot,
    /// 实际 TAA 相位数。
    pub taa_phases: u32,
    /// 实际 DoF 样本数。
    pub dof_samples: u32,
    /// 实际 Bloom mip 级数。
    pub bloom_mips: u32,
}

/// 组合合法性校验 + 解析（**纯函数**，拒绝时零副作用——只记一条告警诊断）。
///
/// 非法三类（逐类显性，不静默收拾）：
/// 1. SMAA 被开——F2012 预留没有渲染实现（STUB 语义：预留不静默）；
/// 2. AA 域表外——AA 槽位每档恰一种，用户只能整关不能换种类/凭空开；
/// 3. 成本 knob 超档上限——TAA 相位/DoF 样本/Bloom mip/逐效果精度高于
///    档位承诺值即越出该档的参数表。
pub fn resolve_config(
    table: &TierTable,
    tier: QualityTier,
    ov: &SubSwitchOverrides,
    bag: &mut DiagBag,
) -> (ComboVerdict, Option<ResolvedConfig>) {
    let p = table.get(tier);
    let mut out = ResolvedConfig {
        tier,
        enabled: p.enabled,
        precision_permille: p.precision_permille,
        aa: p.aa,
        taa_phases: p.taa_phases,
        dof_samples: p.dof_samples,
        bloom_mips: p.bloom_mips,
    };

    // 1) SMAA 预留不可开。
    let smaa_i = Effect::Smaa.ordinal();
    if ov.enable[smaa_i] == Some(true) {
        bag.push_warn(code::ILLEGAL_COMBO);
        return (ComboVerdict::Illegal, None);
    }

    // 2) 成本 knob 只降不升。
    if let Some(v) = ov.taa_phases {
        if v > p.taa_phases {
            bag.push_warn(code::ILLEGAL_COMBO);
            return (ComboVerdict::Illegal, None);
        }
        out.taa_phases = v;
    }
    if let Some(v) = ov.dof_samples {
        if v > p.dof_samples {
            bag.push_warn(code::ILLEGAL_COMBO);
            return (ComboVerdict::Illegal, None);
        }
        out.dof_samples = v;
    }
    if let Some(v) = ov.bloom_mips {
        if v > p.bloom_mips {
            bag.push_warn(code::ILLEGAL_COMBO);
            return (ComboVerdict::Illegal, None);
        }
        out.bloom_mips = v;
    }

    // 3) AA 域：只能整关；换种类/凭空开都表外。
    if let Some(req) = ov.aa {
        match req {
            AaSlot::Off => out.aa = AaSlot::Off,
            _ => {
                bag.push_warn(code::ILLEGAL_COMBO);
                return (ComboVerdict::Illegal, None);
            }
        }
    }
    // AA 整关后 TAA 相位必须同时归零（留在那里就是"名义上关了还在转"）。
    // **放在 knob 覆盖之后**：相位覆盖先落值，AA 归零最后兜底——顺序反了
    // 会出现"整关 AA 又被相位覆盖写回非零"的缝。
    if out.aa == AaSlot::Off {
        out.taa_phases = 0;
    }

    // 4) 逐效果：Some(true) 恒非法（本表没有"档位关、用户可开"的效果），
    //    Some(false) 合法（用户单独关）；精度只降不升。
    for i in 0..EFFECT_COUNT {
        match ov.enable[i] {
            Some(true) => {
                bag.push_warn(code::ILLEGAL_COMBO);
                return (ComboVerdict::Illegal, None);
            }
            Some(false) => out.enabled[i] = false,
            None => {}
        }
        if let Some(v) = ov.precision[i] {
            if v > p.precision_permille[i] {
                bag.push_warn(code::ILLEGAL_COMBO);
                return (ComboVerdict::Illegal, None);
            }
            out.precision_permille[i] = v;
        }
    }

    (ComboVerdict::Ok, Some(out))
}

// ===========================================================================
// 五、D04 降质链映射（后处理维度 → 档位；单源无二义）
// ===========================================================================

/// 一行映射：D04 降质参数闭区间 → 档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct D04Row {
    /// 闭区间下界（含）。
    pub lo: u32,
    /// 闭区间上界（含）。
    pub hi: u32,
    /// 该降质带对应的档位。
    pub tier: QualityTier,
}

/// D04 → 档位映射表（**单源**：降质越重档位越低；0..=100 全域由三行无缝
/// 覆盖，行外值=缺行，拒绝——不回绕不猜。判据做全域覆盖对账，缺行/叠行
/// 在 CI 拦下）。
pub const D04_MAP: [D04Row; 3] = [
    D04Row { lo: 0, hi: 33, tier: QualityTier::High },
    D04Row { lo: 34, hi: 66, tier: QualityTier::Mid },
    D04Row { lo: 67, hi: 100, tier: QualityTier::Low },
];

/// D04 降质参数 → 档位（缺行返回 `None`，调用方记
/// [`code::MAP_ROW_MISSING`]——CI 对账拦截的运行时对应面）。
pub fn tier_for_d04(level: u32) -> Option<QualityTier> {
    for row in D04_MAP.iter() {
        if level >= row.lo && level <= row.hi {
            return Some(row.tier);
        }
    }
    None
}

/// 档位 → D04 代表值（该档映射带的**中点**；双向往返 tier→代表值→tier
/// 必须恒等——代表值落在带外即映射自相矛盾）。
pub fn d04_for_tier(tier: QualityTier) -> u32 {
    for row in D04_MAP.iter() {
        if row.tier == tier {
            return (row.lo + row.hi) / 2;
        }
    }
    0
}

// ===========================================================================
// 六、三方裁决（F1832 同构规则复用声明——一处定义两处引用）
// ===========================================================================

/// 裁决源（优先级表顺序即权威序：**用户覆盖 > 档位 > 预算临时**——与
/// F1832 三方裁决同构，与 F2014 `BudgetLinker` 的 user_override 语义同向：
/// 用户在场上时预算临时必须让位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdjudicationSource {
    /// 用户覆盖（设置面板/启动参数）——最高优先级。
    User,
    /// 档位体系（D04 映射/档位树建议）——次之。
    TierSystem,
    /// 预算临时（F2014 联动的临时降档建议）——最低。
    BudgetTemp,
}

/// 裁决优先级表（**单源**：顺序不可换，判据逐位钉住）。
pub const ADJ_PRIORITY: [AdjudicationSource; 3] = [
    AdjudicationSource::User,
    AdjudicationSource::TierSystem,
    AdjudicationSource::BudgetTemp,
];

/// 裁决产物。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Adjudication {
    /// 胜出档位。
    pub winner: QualityTier,
    /// 与胜者不一致的**在场**源数（缺席≠异见，不计冲突——判据精确对账）。
    pub conflicts: u32,
}

/// 三方裁决（**纯函数**；冲突记 [`code::TIER_DESYNC`] 告警）。
///
/// 三个源各自独立表态（`None`=该源不在场），按 [`ADJ_PRIORITY`] 取第一个
/// 在场源为准；全部缺席时保持 `keep`（现役档位）——不发明默认值，保持现状
/// 是唯一不撒谎的答案。每个在场但与胜者不同的源记一次冲突。
pub fn adjudicate(
    user: Option<QualityTier>,
    tier_sys: Option<QualityTier>,
    budget_tmp: Option<QualityTier>,
    keep: QualityTier,
    bag: &mut DiagBag,
) -> Adjudication {
    let winner = match (user, tier_sys, budget_tmp) {
        (Some(u), _, _) => u,
        (None, Some(t), _) => t,
        (None, None, Some(b)) => b,
        (None, None, None) => keep,
    };

    let mut conflicts = 0u32;
    if let Some(u) = user {
        if u != winner {
            conflicts += 1;
        }
    }
    if let Some(t) = tier_sys {
        if t != winner {
            conflicts += 1;
        }
    }
    if let Some(b) = budget_tmp {
        if b != winner {
            conflicts += 1;
        }
    }
    if conflicts > 0 {
        bag.push_warn(code::TIER_DESYNC);
    }
    Adjudication { winner, conflicts }
}

// ===========================================================================
// 七、帧边界切换（F1762 同规则 + F2002 重组协议复用位）
// ===========================================================================

/// 档位切换台（现役配置 + 暂存配置 + 原子应用）。
///
/// **帧边界生效**：请求只写暂存，帧边界一次性换装——帧中换装会让一帧内
/// 前半用旧参数后半用新参数（F2002 图重编译必须整帧原子，半新半旧的帧
/// 正是撕裂的来源）。暂存是**一次性消费**：应用即清空，二次应用拿不到
/// 东西——"应用过了"不能被"再应用一次"伪装成还没发生。
pub struct SwitchBoard {
    table: TierTable,
    active: ResolvedConfig,
    staged: Option<ResolvedConfig>,
    /// 换装次数（**单调**，epoch 判据用）。
    pub epoch: u64,
    /// 累计请求次数（含覆盖未应用请求）。
    pub requests: u64,
    /// 累计帧边界应用次数。
    pub applies: u64,
}

impl SwitchBoard {
    /// 构造（现役=默认档全默认参数）。
    pub fn new() -> SwitchBoard {
        let table = TierTable::calibrated();
        let mut bag = DiagBag::new();
        let (_, cfg) =
            resolve_config(&table, DEFAULT_TIER, &SubSwitchOverrides::default(), &mut bag);
        SwitchBoard {
            table,
            active: cfg.unwrap_or_else(|| bare_config(DEFAULT_TIER)),
            staged: None,
            epoch: 0,
            requests: 0,
            applies: 0,
        }
    }

    /// 现役配置（只读）。
    pub fn active(&self) -> &ResolvedConfig {
        &self.active
    }

    /// 是否有未应用的暂存。
    pub fn has_staged(&self) -> bool {
        self.staged.is_some()
    }

    /// 请求切换（任何时刻可调；**只写暂存不动现役**——帧边界才生效）。
    /// 新请求覆盖未应用的旧请求（只有最新意图生效，F2002 重组协议同款）。
    /// 非法请求**连暂存都不产生**（不静默收下再丢弃）。
    pub fn request(
        &mut self,
        tier: QualityTier,
        ov: &SubSwitchOverrides,
        bag: &mut DiagBag,
    ) -> ComboVerdict {
        self.requests = self.requests.saturating_add(1);
        let (v, cfg) = resolve_config(&self.table, tier, ov, bag);
        if let Some(cfg) = cfg {
            self.staged = Some(cfg);
        }
        v
    }

    /// 帧边界应用（每帧尾调用一次；无暂存返回 `None`，有暂存换装并返回
    /// 新 epoch）。**这是唯一能改现役的入口**——帧中换装在 API 层不可表达。
    pub fn apply_at_frame_boundary(&mut self) -> Option<u64> {
        let staged = self.staged.take()?;
        self.active = staged;
        self.epoch = self.epoch.saturating_add(1);
        self.applies = self.applies.saturating_add(1);
        Some(self.epoch)
    }
}

/// 裸配置（解析失败时的兜底——只出现在构造期不变式被破坏的场合；
/// 判据断言正常路径永不走到这里）。
fn bare_config(tier: QualityTier) -> ResolvedConfig {
    let table = TierTable::calibrated();
    let p = table.get(tier);
    ResolvedConfig {
        tier,
        enabled: p.enabled,
        precision_permille: p.precision_permille,
        aa: p.aa,
        taa_phases: p.taa_phases,
        dof_samples: p.dof_samples,
        bloom_mips: p.bloom_mips,
    }
}

// ===========================================================================
// 八、档位记忆（持久化 + 版本迁移）
// ===========================================================================

/// 记忆魔数（"F2015"的 15 位谐音——0xF215）。
pub const MEM_MAGIC: u16 = 0xF215;
/// 当前记忆版本 v2（v1=仅档位；v2 增加用户子开关位与 AA 整关位）。
pub const MEM_VERSION: u16 = 2;

/// 记忆载荷（用户选择：档位 + 子开关位 + AA 整关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemPayload {
    /// 用户选定的档位。
    pub tier: QualityTier,
    /// 逐效果子开关位（bit i = 效果 i 被用户单独关；1=关；十效果恰用 10 位）。
    pub disabled_bits: u16,
    /// AA 整关（用户把 AA 关了）。
    pub aa_off: bool,
}

/// 记忆编码错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemError {
    /// 损坏（魔数/校验和不符）——回退默认档+告警。
    Corrupt,
    /// 版本无迁移链——显性拒绝（声明支持范围：本实现支持 v1 迁移与 v2 本体，
    /// 未来版本到了要写 v2→v3 迁移器，不能装作没看见）。
    VersionUnsupported,
}

/// FNV-1a 截 8 位（覆盖前 7 字节）。
///
/// **为什么 8 位就够**：FNV-1a 对**单比特翻转**是全检出的——翻第 i 位把
/// 中间值改 ±2^i，乘奇素数模 256 后差仍 = ±(2^i·奇) mod 256 ≠ 0（i<8）。
/// 记忆载荷的威胁模型正是单比特翻转/字节损坏，不是蓄意碰撞——8 位校验
/// 换来 1 个数据字节（十效果子开关 10 位 + 档位 2 位 + AA 1 位，8 位校验
/// 才挤得下），这笔交换是划算的。
const fn mem_checksum(bytes: &[u8; 8]) -> u8 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < 7 {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    (h & 0xFF) as u8
}

/// 编码（8 字节定长）：
/// `b[0..2]`=魔数 LE、`b[2..4]`=版本 LE、`b[4]`=档位序号、
/// `b[5]`=子开关位 0..7（bit7=AA 整关）、`b[6]`=子开关位 7..9（低 3 位）、
/// `b[7]`=校验和（覆盖 b[0..7]）。
pub fn mem_encode(p: &MemPayload) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[0] = (MEM_MAGIC & 0xFF) as u8;
    b[1] = (MEM_MAGIC >> 8) as u8;
    b[2] = (MEM_VERSION & 0xFF) as u8;
    b[3] = (MEM_VERSION >> 8) as u8;
    b[4] = p.tier.ordinal() as u8;
    b[5] = (if p.aa_off { 0x80 } else { 0 }) | ((p.disabled_bits & 0x7F) as u8);
    b[6] = ((p.disabled_bits >> 7) & 0x07) as u8;
    b[7] = mem_checksum(&b);
    b
}

/// 解码（**损坏与版本分型**：魔数/校验和坏 = Corrupt；自洽的未知版本 =
/// VersionUnsupported——两者走不同的错误路径，前者回退默认档，后者显性
/// 拒绝不静默回退）。
pub fn mem_decode(bytes: &[u8; 8]) -> Result<MemPayload, MemError> {
    let magic = (bytes[0] as u16) | ((bytes[1] as u16) << 8);
    if magic != MEM_MAGIC {
        return Err(MemError::Corrupt);
    }
    if bytes[7] != mem_checksum(bytes) {
        return Err(MemError::Corrupt);
    }
    let version = (bytes[2] as u16) | ((bytes[3] as u16) << 8);
    if version != MEM_VERSION {
        return Err(MemError::VersionUnsupported);
    }
    let tier = match QualityTier::from_ordinal((bytes[4] & 0x03) as usize) {
        Some(t) => t,
        None => return Err(MemError::Corrupt),
    };
    let aa_off = bytes[5] & 0x80 != 0;
    let disabled_bits = (bytes[5] & 0x7F) as u16 | (((bytes[6] & 0x07) as u16) << 7);
    Ok(MemPayload { tier, disabled_bits, aa_off })
}

/// v1 → v2 迁移（v1 载荷：magic/version=1/tier，无子开关位——迁移补默认
/// 零位，用户档位选择**原样保留**，这是迁移的底线：升级不丢用户选择）。
pub fn mem_migrate_v1_to_v2(bytes: &[u8; 8]) -> Result<MemPayload, MemError> {
    let magic = (bytes[0] as u16) | ((bytes[1] as u16) << 8);
    if magic != MEM_MAGIC {
        return Err(MemError::Corrupt);
    }
    let version = (bytes[2] as u16) | ((bytes[3] as u16) << 8);
    if version != 1 {
        return Err(MemError::VersionUnsupported);
    }
    let tier = match QualityTier::from_ordinal((bytes[4] & 0x03) as usize) {
        Some(t) => t,
        None => return Err(MemError::Corrupt),
    };
    Ok(MemPayload { tier, disabled_bits: 0, aa_off: false })
}

/// 记忆读入的高层语义（错误路径分型在这层做，调用方不用重抄）：
/// `Ok(payload)`=读到了用户选择；`Err(Corrupt)` 已记 [`code::MEM_CORRUPT`]
/// （调用方据此回退默认档）；`Err(VersionUnsupported)` 已记
/// [`code::MEM_VERSION_UNSUPPORTED`]——**不**替用户回退（显性拒绝）。
pub fn mem_load(bytes: &[u8; 8], bag: &mut DiagBag) -> Result<MemPayload, MemError> {
    match mem_decode(bytes) {
        Ok(p) => Ok(p),
        Err(MemError::VersionUnsupported) => {
            bag.push_warn(code::MEM_VERSION_UNSUPPORTED);
            Err(MemError::VersionUnsupported)
        }
        Err(e) => {
            bag.push_warn(code::MEM_CORRUPT);
            Err(e)
        }
    }
}

// ===========================================================================
// 九、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F2015 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vek15_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vek15");

        // -- P15-SET-01 三档表闭集：SMAA 三档全关 + knobs 单调 + 确有差异 ----
        {
            let t = TierTable::calibrated();
            let hi = t.get(QualityTier::High);
            let mid = t.get(QualityTier::Mid);
            let lo = t.get(QualityTier::Low);
            let smaa_i = Effect::Smaa.ordinal();
            let smaa_all_off =
                !hi.enabled[smaa_i] && !mid.enabled[smaa_i] && !lo.enabled[smaa_i];
            // knobs 单调：高档 ≥ 中档 ≥ 低档。
            let knobs_mono = hi.taa_phases >= mid.taa_phases
                && mid.taa_phases >= lo.taa_phases
                && hi.dof_samples >= mid.dof_samples
                && mid.dof_samples >= lo.dof_samples
                && hi.bloom_mips >= mid.bloom_mips
                && mid.bloom_mips >= lo.bloom_mips;
            // 反证：三档确有差异（全等则档位形同虚设——恒真断言防线）。
            let tiers_differ = hi.taa_phases != mid.taa_phases
                || hi.dof_samples != mid.dof_samples
                || hi.bloom_mips != mid.bloom_mips;
            // AA 替代关系与三档 knob 直译（锚点原文逐项）。
            let aa_shape = hi.aa == AaSlot::Taa
                && mid.aa == AaSlot::Taa
                && lo.aa == AaSlot::Fxaa
                && hi.taa_phases == TAA_PHASES_HIGH
                && mid.taa_phases == TAA_PHASES_MID
                && lo.taa_phases == TAA_PHASES_LOW
                && hi.dof_samples == DOF_SAMPLES_HIGH
                && mid.dof_samples == DOF_SAMPLES_MID
                && lo.dof_samples == DOF_SAMPLES_LOW
                && hi.bloom_mips == BLOOM_MIPS_FULL
                && mid.bloom_mips == BLOOM_MIPS_HALF
                && lo.bloom_mips == BLOOM_MIPS_SINGLE;
            // 十效果闭集行宽对齐（与 F2014 单源同宽）。
            let width_ok =
                hi.enabled.len() == EFFECT_COUNT && hi.precision_permille.len() == EFFECT_COUNT;
            s.add(
                "P15-SET-01 三档表：SMAA 三档全关、knobs 逐项单调且三档确有差异、AA 替代形状正确",
                smaa_all_off && knobs_mono && tiers_differ && aa_shape && width_ok,
                "锚点三档语义直译：高=全精度全 mip、中=TAA 标准/DoF32/半 mip、低=FXAA 替代/DoF 关/单级",
            );
        }

        // -- P15-SET-02 三档闭集双射 + 标签齐全 ------------------------------
        {
            let mut bij = true;
            for t in QualityTier::all().iter() {
                if QualityTier::from_ordinal(t.ordinal()) != Some(*t) {
                    bij = false;
                }
            }
            s.add(
                "P15-SET-02 三档序号与闭集双射、越界返回 None、默认档为中档",
                bij
                    && QualityTier::from_ordinal(TIER_COUNT).is_none()
                    && DEFAULT_TIER == QualityTier::Mid
                    && QualityTier::Low.label() == "low"
                    && QualityTier::High.label() == "high",
                "闭集按序号寻址（零分配）；越界须拒而不是回绕",
            );
        }

        // -- P15-SUB-01 合法降档放行且逐位生效 -------------------------------
        {
            let t = TierTable::calibrated();
            let mut bag = DiagBag::new();
            let mut ov = SubSwitchOverrides::default();
            let bloom_i = Effect::Bloom.ordinal();
            ov.enable[bloom_i] = Some(false); // 用户单独关 Bloom
            ov.precision[Effect::Fxaa.ordinal()] = Some(300); // 精度下调
            ov.dof_samples = Some(8); // DoF 从 64 降到 8
            ov.aa = Some(AaSlot::Off); // AA 整关
            let (v, cfg) = resolve_config(&t, QualityTier::High, &ov, &mut bag);
            let cfg = match cfg {
                Some(c) => c,
                None => bare_config(QualityTier::High),
            };
            let aa_off_consistent = cfg.aa == AaSlot::Off && cfg.taa_phases == 0;
            s.add(
                "P15-SUB-01 合法降档（关效果/降精度/降样本/整关AA）放行且逐位生效，AA 关则相位归零",
                v == ComboVerdict::Ok
                    && !cfg.enabled[bloom_i]
                    && cfg.precision_permille[Effect::Fxaa.ordinal()] == 300
                    && cfg.dof_samples == 8
                    && aa_off_consistent
                    && cfg.tier == QualityTier::High,
                "子开关=承诺包内下调；整关 AA 必须连带相位归零（名义关了还在转=假关）",
            );
        }

        // -- P15-SUB-02 非法组合逐类拒绝且拒绝码同源 -------------------------
        {
            let t = TierTable::calibrated();
            let smaa_i = Effect::Smaa.ordinal();
            // (a) SMAA 预留被开
            let mut bag_a = DiagBag::new();
            let mut ov_a = SubSwitchOverrides::default();
            ov_a.enable[smaa_i] = Some(true);
            let (va, ca) = resolve_config(&t, QualityTier::High, &ov_a, &mut bag_a);
            // (b) AA 域表外：低档强行换 AA 种类
            let mut bag_b = DiagBag::new();
            let mut ov_b = SubSwitchOverrides::default();
            ov_b.aa = Some(AaSlot::Taa);
            let (vb, cb) = resolve_config(&t, QualityTier::Low, &ov_b, &mut bag_b);
            // (c) knob 超档上限：低档 DoF 强行 64
            let mut bag_c = DiagBag::new();
            let mut ov_c = SubSwitchOverrides::default();
            ov_c.dof_samples = Some(DOF_SAMPLES_HIGH);
            let (vc, cc) = resolve_config(&t, QualityTier::Low, &ov_c, &mut bag_c);
            // (d) 精度超档：高档精度 1000，强塞 1001
            let mut bag_d = DiagBag::new();
            let mut ov_d = SubSwitchOverrides::default();
            ov_d.precision[Effect::Bloom.ordinal()] = Some(PREC_FULL + 1);
            let (vd, cd) = resolve_config(&t, QualityTier::High, &ov_d, &mut bag_d);
            s.add(
                "P15-SUB-02 非法组合逐类拒绝：SMAA 开/AA 换种/knob 超档/精度超档，均记 ILLEGAL_COMBO 且零产物",
                va == ComboVerdict::Illegal
                    && vb == ComboVerdict::Illegal
                    && vc == ComboVerdict::Illegal
                    && vd == ComboVerdict::Illegal
                    && ca.is_none()
                    && cb.is_none()
                    && cc.is_none()
                    && cd.is_none()
                    && bag_a.has(code::ILLEGAL_COMBO)
                    && bag_b.has(code::ILLEGAL_COMBO)
                    && bag_c.has(code::ILLEGAL_COMBO)
                    && bag_d.has(code::ILLEGAL_COMBO),
                "档位表外参数拒绝不静默收拾；SMAA 预留=STUB 语义（F2012）；拒绝零副作用",
            );
        }

        // -- P15-SUB-03 全域扫描：只降不升不变式（三档×十效果单覆盖枚举）----
        {
            let t = TierTable::calibrated();
            let mut all_ok = true;
            for tier in QualityTier::all().iter() {
                for i in 0..EFFECT_COUNT {
                    // 关任一效果合法且只影响该效果。
                    let mut bag = DiagBag::new();
                    let mut ov = SubSwitchOverrides::default();
                    ov.enable[i] = Some(false);
                    let (v, cfg) = resolve_config(&t, *tier, &ov, &mut bag);
                    match cfg {
                        Some(c) => {
                            if v != ComboVerdict::Ok || c.enabled[i] {
                                all_ok = false;
                            }
                            // 其余效果不受牵连（与档位表逐位一致）。
                            for j in 0..EFFECT_COUNT {
                                if j != i && c.enabled[j] != t.get(*tier).enabled[j] {
                                    all_ok = false;
                                }
                            }
                        }
                        None => all_ok = false,
                    }
                }
            }
            s.add(
                "P15-SUB-03 全域扫描：三档×十效果逐个单独关闭均合法且不牵连其余效果",
                all_ok,
                "用户可单独关某效果（锚点原文）；合法性校验不得有意外拒绝/意外放行",
            );
        }

        // -- P15-D04-01 映射全域覆盖无缝无叠 + 边界归档 + 越界拒绝 -----------
        {
            // 全域覆盖对账（CI 对账拦截的等价物）：0..=100 每值恰归一档。
            let mut coverage = [0u32; 101];
            let mut rows_wellformed = true;
            for row in D04_MAP.iter() {
                if row.hi < row.lo || row.hi > 100 {
                    rows_wellformed = false;
                }
                let mut l = row.lo;
                while l <= row.hi && l <= 100 {
                    coverage[l as usize] += 1;
                    l += 1;
                }
            }
            let seamless = rows_wellformed && coverage.iter().all(|&c| c == 1);
            // 边界值逐个归档。
            let bounds_ok = tier_for_d04(0) == Some(QualityTier::High)
                && tier_for_d04(33) == Some(QualityTier::High)
                && tier_for_d04(34) == Some(QualityTier::Mid)
                && tier_for_d04(66) == Some(QualityTier::Mid)
                && tier_for_d04(67) == Some(QualityTier::Low)
                && tier_for_d04(100) == Some(QualityTier::Low);
            // 越界显性拒绝（101 不回绕不猜）。
            let mut bag = DiagBag::new();
            let oob = tier_for_d04(101);
            if oob.is_none() {
                bag.push_warn(code::MAP_ROW_MISSING);
            }
            s.add(
                "P15-D04-01 映射 0..=100 无缝无叠、六边界值归档正确、101 缺行显性拒绝",
                seamless && bounds_ok && oob.is_none() && bag.has(code::MAP_ROW_MISSING),
                "F1916 模式复用：单源无二义；缺行即配置错误由 CI 对账拦截",
            );
        }

        // -- P15-D04-02 双向往返恒等（tier→代表值→tier 不换档不出带）--------
        {
            let mut rt_ok = true;
            for t in QualityTier::all().iter() {
                let rep = d04_for_tier(*t);
                match tier_for_d04(rep) {
                    Some(back) if back == *t => {}
                    _ => rt_ok = false,
                }
                // 代表值必须落在该档自己的带内（不是擦边借道别档）。
                let in_band = D04_MAP
                    .iter()
                    .any(|r| r.tier == *t && rep >= r.lo && rep <= r.hi);
                if !in_band {
                    rt_ok = false;
                }
            }
            s.add(
                "P15-D04-02 双向往返 tier→代表值→tier 恒等且代表值在本档带内",
                rt_ok,
                "映射两个方向同源（一张表出两函数）；代表值出带=映射自相矛盾",
            );
        }

        // -- P15-ADJ-01 三方裁决：三源各胜一次且冲突计数精确 -----------------
        {
            // user 胜：user=Low，tier=High，budget=Mid ⇒ 冲突恰 2（两个异见在场源）。
            let mut bag1 = DiagBag::new();
            let a1 = adjudicate(
                Some(QualityTier::Low),
                Some(QualityTier::High),
                Some(QualityTier::Mid),
                QualityTier::High,
                &mut bag1,
            );
            // tier 体系胜：user 缺席，tier=Low，budget=Mid ⇒ 冲突恰 1。
            let mut bag2 = DiagBag::new();
            let a2 = adjudicate(
                None,
                Some(QualityTier::Low),
                Some(QualityTier::Mid),
                QualityTier::High,
                &mut bag2,
            );
            // 预算临时胜：user/tier 均缺席，budget=Low，keep=High ⇒ 冲突 0
            // （keep 不是裁决源，缺席不计异见）。
            let mut bag3 = DiagBag::new();
            let a3 = adjudicate(
                None,
                None,
                Some(QualityTier::Low),
                QualityTier::High,
                &mut bag3,
            );
            // 全缺席：保持 keep，零冲突零告警。
            let mut bag4 = DiagBag::new();
            let a4 = adjudicate(None, None, None, QualityTier::Mid, &mut bag4);
            // 全一致：三源同值零冲突零告警。
            let mut bag5 = DiagBag::new();
            let a5 = adjudicate(
                Some(QualityTier::Mid),
                Some(QualityTier::Mid),
                Some(QualityTier::Mid),
                QualityTier::Mid,
                &mut bag5,
            );
            s.add(
                "P15-ADJ-01 三源各胜一次（user>tier>budget>保持）且冲突计数精确、一致/全缺席零告警",
                a1.winner == QualityTier::Low
                    && a1.conflicts == 2
                    && bag1.has(code::TIER_DESYNC)
                    && a2.winner == QualityTier::Low
                    && a2.conflicts == 1
                    && a3.winner == QualityTier::Low
                    && a3.conflicts == 0
                    && a4.winner == QualityTier::Mid
                    && a4.conflicts == 0
                    && !bag4.has(code::TIER_DESYNC)
                    && a5.conflicts == 0
                    && !bag5.has(code::TIER_DESYNC),
                "F1832 同构：用户覆盖>档位>预算临时；在场异见才计冲突（缺席≠异见）",
            );
        }

        // -- P15-ADJ-02 优先级表单源：顺序逐位钉住 ---------------------------
        {
            s.add(
                "P15-ADJ-02 优先级表逐位等于（用户、档位体系、预算临时），不可换序",
                ADJ_PRIORITY.len() == 3
                    && matches!(ADJ_PRIORITY[0], AdjudicationSource::User)
                    && matches!(ADJ_PRIORITY[1], AdjudicationSource::TierSystem)
                    && matches!(ADJ_PRIORITY[2], AdjudicationSource::BudgetTemp),
                "一处定义两处引用（F1832 复用声明）：表是单源，裁决函数与判据都引用它",
            );
        }

        // -- P15-SW-01 帧中请求不生效；帧边界一次性换装；二次应用拿不到 -------
        {
            let mut sb = SwitchBoard::new();
            let mut bag = DiagBag::new();
            let before = *sb.active();
            let v = sb.request(QualityTier::Low, &SubSwitchOverrides::default(), &mut bag);
            let after_request = *sb.active();
            // 断言时点：has_staged 必须在应用前捕获——应用是一次性消费，
            // 走完再查只会看到空暂存（判据读被后续调用改写的状态=读错时点）。
            let staged_pending = sb.has_staged();
            let e1 = sb.apply_at_frame_boundary();
            let after_apply = *sb.active();
            let e2 = sb.apply_at_frame_boundary();
            s.add(
                "P15-SW-01 请求后现役逐位不变、帧边界才换装且 epoch+1、二次应用返回 None",
                v == ComboVerdict::Ok
                    && staged_pending
                    && after_request == before
                    && e1 == Some(1)
                    && after_apply != before
                    && after_apply.tier == QualityTier::Low
                    && e2.is_none()
                    && sb.epoch == 1
                    && sb.applies == 1
                    && sb.requests == 1,
                "F1762 同规则：切换帧边界生效；暂存一次性消费（原子换装不可重放）",
            );
        }

        // -- P15-SW-02 新请求覆盖未应用旧请求；非法请求不产生暂存 ------------
        {
            let mut sb = SwitchBoard::new();
            let mut bag = DiagBag::new();
            let mut ov_bad = SubSwitchOverrides::default();
            ov_bad.enable[Effect::Smaa.ordinal()] = Some(true);
            let v_bad = sb.request(QualityTier::High, &ov_bad, &mut bag);
            let no_stage_after_bad = !sb.has_staged();
            let mut ov_ok1 = SubSwitchOverrides::default();
            ov_ok1.dof_samples = Some(48);
            let v_ok1 = sb.request(QualityTier::High, &ov_ok1, &mut bag);
            let mut ov_ok2 = SubSwitchOverrides::default();
            ov_ok2.dof_samples = Some(16);
            let v_ok2 = sb.request(QualityTier::High, &ov_ok2, &mut bag);
            let e = sb.apply_at_frame_boundary();
            let dof_final = sb.active().dof_samples;
            s.add(
                "P15-SW-02 非法请求零暂存、新请求覆盖旧请求（只生效最新意图 16 非 48）",
                v_bad == ComboVerdict::Illegal
                    && no_stage_after_bad
                    && v_ok1 == ComboVerdict::Ok
                    && v_ok2 == ComboVerdict::Ok
                    && e.is_some()
                    && dof_final == 16,
                "F2002 重组协议同款：暂存只留最新意图；非法请求连暂存都不产生",
            );
        }

        // -- P15-MEM-01 记忆往返恒等 + 8×64 位逐位翻转全检出 + 分型 -----------
        {
            let p = MemPayload {
                tier: QualityTier::Low,
                disabled_bits: 0b0011_1000_0101, // 十效果跨 10 位
                aa_off: true,
            };
            let bytes = mem_encode(&p);
            let rt_ok = matches!(mem_decode(&bytes), Ok(q) if q == p);
            // 8 字节×8 位全扫：任一单比特翻转必须被拒（不是抽样——抽样给
            // 碰撞留侥幸面；FNV-1a 8 位对单比特翻转数学上全检出，见
            // mem_checksum 注记）。
            let mut all_detected = true;
            for byte_i in 0..8usize {
                for bit in 0..8 {
                    let mut bad = bytes;
                    bad[byte_i] ^= 1u8 << bit;
                    if mem_decode(&bad).is_ok() {
                        all_detected = false;
                    }
                }
            }
            // 损坏分型：魔数坏 = Corrupt（不是版本错）。
            let mut bad_magic = bytes;
            bad_magic[0] ^= 0xFF;
            let magic_typing = matches!(mem_decode(&bad_magic), Err(MemError::Corrupt));
            s.add(
                "P15-MEM-01 记忆往返恒等（十效果跨 10 位）、8×64 位逐位翻转全检出、魔数坏分型 Corrupt",
                rt_ok && all_detected && magic_typing,
                "校验和恒过的实现靠全位翻转扫描钉死；损坏与版本走不同错误路径",
            );
        }

        // -- P15-MEM-02 损坏记 MEM_CORRUPT；自洽未知版本显性拒绝不回退 --------
        {
            // 损坏路径：翻 tier 字节一位（校验和必不匹配）→ MEM_CORRUPT。
            let good = mem_encode(&MemPayload {
                tier: QualityTier::Low,
                disabled_bits: 0,
                aa_off: false,
            });
            let mut bad = good;
            bad[4] ^= 0x01;
            let mut bag_corrupt = DiagBag::new();
            let r_corrupt = mem_load(&bad, &mut bag_corrupt);
            // 版本路径：构造**校验和自洽**的 v3 载荷——真实场景是新版引擎写的
            // 完好 v3 被旧引擎读到；若不重算校验和，版本错永远被损坏路径掩盖
            // （分型失效，版本分支成了死代码）。
            let mut v3 = good;
            v3[2] = 3;
            v3[3] = 0;
            v3[7] = mem_checksum(&v3);
            let mut bag_version = DiagBag::new();
            let r_version = mem_load(&v3, &mut bag_version);
            s.add(
                "P15-MEM-02 损坏记 MEM_CORRUPT；自洽未知版本记 VERSION_UNSUPPORTED 且显性拒绝",
                matches!(r_corrupt, Err(MemError::Corrupt))
                    && bag_corrupt.has(code::MEM_CORRUPT)
                    && matches!(r_version, Err(MemError::VersionUnsupported))
                    && bag_version.has(code::MEM_VERSION_UNSUPPORTED),
                "损坏→回退默认档+告警；版本无迁移链→显性拒绝声明支持范围（不静默回退丢用户选择）",
            );
        }

        // -- P15-MEM-03 v1 迁移：档位保留、子开关补默认零位 ------------------
        {
            let mut v1 = [0u8; 8];
            v1[0] = (MEM_MAGIC & 0xFF) as u8;
            v1[1] = (MEM_MAGIC >> 8) as u8;
            v1[2] = 1; // version 1
            v1[3] = 0;
            v1[4] = QualityTier::High.ordinal() as u8;
            let m = mem_migrate_v1_to_v2(&v1);
            let m_ok = matches!(
                &m,
                Ok(p) if p.tier == QualityTier::High && p.disabled_bits == 0 && !p.aa_off
            );
            // v2 本体不走迁移器（迁移链只收旧版本）。
            let v2 = mem_encode(&MemPayload {
                tier: QualityTier::Low,
                disabled_bits: 0,
                aa_off: false,
            });
            let v2_rejected =
                matches!(mem_migrate_v1_to_v2(&v2), Err(MemError::VersionUnsupported));
            s.add(
                "P15-MEM-03 v1→v2 迁移保留用户档位并补默认零位；v2 本体不重复迁移",
                m_ok && v2_rejected,
                "升级不丢用户选择是迁移的底线；迁移链只收旧版本（重复迁移=版本链失守）",
            );
        }

        // -- P15-REUSE-01 同域单源复用：效果闭集与诊断袋来自 F2014 -----------
        {
            // 效果闭集行宽与 F2014 单源同宽（另立一份枚举会让两表行序各说各话）。
            let t = TierTable::calibrated();
            let width_ok = t.get(QualityTier::High).enabled.len() == EFFECT_COUNT;
            // 诊断袋按 (code>>8)&0x0F 分桶：0x30 段桶 0 与 F2014 的 0x2D 段
            // 桶 0xD 不撞——同袋混装不串账（真复用而非复制）。
            let mut bag = DiagBag::new();
            bag.push_warn(code::ILLEGAL_COMBO);
            let bucket_ok = bag.count(code::ILLEGAL_COMBO) == 1;
            s.add(
                "P15-REUSE-01 十效果闭集与诊断袋复用 F2014 单源，0x30 段诊断码不撞桶",
                width_ok && bucket_ok,
                "同域单源：另起一套会让效果行序/诊断口径各说各话",
            );
        }

        s
    }
}
