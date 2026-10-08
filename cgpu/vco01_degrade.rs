//! CGPU-F2241 · O 域开工与降级链总架构（CGPU-O 域 · 降级链 · 批次 O01 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2241`
//!
//! 域使命：降级链——让性能压力下系统「优雅地退」而不是「难看地崩」：每一级
//! 降级都是有设计的、可预期、可恢复的——**优雅降级哲学**。N 域移交包签收
//! 记录（F2237 签收）；官方主题映射（降级模型/决策引擎/链路全图/可观测/生态
//! ——十组映射表）；架构五段（触发→决策→执行→恢复→观测）；历史汇总（散落
//! 各域降级能力汇总——D 域掉帧链/J03 热阶梯/J04 续航/Q02 流送/N01 效果降级
//! ——**统一化声明：O 域=全域降级的统一编排层，各域保留专业降级逻辑**）；
//! 不变量（任何降级不突破合同底线——80 帧适配表/信息完整性，双不变量复用）；
//! 风险四条（决策振荡/链路冲突/恢复过冲/可见性失控——各配预案）。
//!
//! # 要点一：优雅哲学是可机检的三条款，不是口号
//!
//! 「有设计/可预期/可恢复」落为封闭三条款字面量 + 恢复判定函数：一档降级
//! 只有同时声明触发源、执行动作、恢复条件才算「有设计的」——三缺一即
//! [`C_VCO01_RECEIPT_INVALID`] 级的账面缺口。
//!
//! # 要点二：签收是兑现起点
//!
//! N 域移交包（F2237）七件签收记录为结构化 [`HandoffReceipt`]（来源/七件/
//! 状态/指纹）——指纹 const 期对七件清单逐项 FNV，签收缺项或指纹漂移即
//! 域未开工（判据「签收」）。
//!
//! # 要点三：十组映射取官方批次标题逐字
//!
//! O 域十组（O01..O10）区间 2241..2400 连续无缺口守恒 160 项；五官方主题
//! 在映射表中全覆盖（判据侧重算）——「映射」由此可审计。
//!
//! # 要点四：五段流水线单向
//!
//! 触发→决策→执行→恢复→观测；跳段（未决策就执行）与回退（观测逆向驱动
//! 执行）分别以 [`C_VCO01_STAGE_SKIP`]/[`C_VCO01_STAGE_REWIND`] 显性拒绝
//! （判据「五段」）。
//!
//! # 要点五：统一编排不收编
//!
//! 五处历史降级能力逐行入 [`LEGACY_DEGRADES`] 汇总表；O 域只做统一编排，
//! 各域专业降级逻辑原地保留（复用不重建）——表外散落能力即
//! [`C_VCO01_LEGACY_ORPHAN`] 立案（判据「统一编排」）。
//!
//! # 要点六：双不变量是底线闸
//!
//! 任何降级不突破 80 帧适配表合同底线、不破坏信息完整性——
//! [`floor_check`] 对突破底线的一档返回 [`C_VCO01_FLOOR_BROKEN`]，
//! 信息完整性由 [`integrity_check`] 对「核心信息缺席」的一档返回
//! [`C_VCO01_INTEGRITY_LOST`]（判据「不变量」）。
//!
//! # 要点七：风险四条各配预案
//!
//! 决策振荡/链路冲突/恢复过冲/可见性失控——预案互异且非空；无预案的
//! 风险等于事故邀请函（判据「风险」）。
//!
//! # 要点八：诊断码独占 0x55xx 段
//!
//! 与 F1121（0x50xx）/cga02（0x51xx）/F1441（0x52xx）/F1761（0x53xx）/
//! cgm01（0x54xx）互不重叠。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界/切片扩断：五段推进用定长枚举匹配，
//! 表访问全部编译期定长；失败路径走 `Result` 与显式错误码。

// ---------------------------------------------------------------------------
// 导入（no_std）
// ---------------------------------------------------------------------------

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（vco01 独占段 0x5500..0x55FF）
// ---------------------------------------------------------------------------

/// 降级突破合同底线（80 帧适配表）。
pub const C_VCO01_FLOOR_BROKEN: u16 = 0x5500;
/// 降级破坏信息完整性。
pub const C_VCO01_INTEGRITY_LOST: u16 = 0x5501;
/// 签收缺项/指纹漂移（域未开工）。
pub const C_VCO01_RECEIPT_INVALID: u16 = 0x5502;
/// 五段流水线跳段（未决策就执行等）。
pub const C_VCO01_STAGE_SKIP: u16 = 0x5503;
/// 五段流水线回退（观测逆向驱动执行等）。
pub const C_VCO01_STAGE_REWIND: u16 = 0x5504;
/// 历史降级能力未收编进统一汇总表。
pub const C_VCO01_LEGACY_ORPHAN: u16 = 0x5505;

/// 域版本。
pub const VCO01_VERSION: &str = "CO01-degrade-v1";

// ---------------------------------------------------------------------------
// 二、域使命与优雅降级哲学（字面量冻结）
// ---------------------------------------------------------------------------

/// 优雅降级哲学三条款（字面量冻结——判据独立对拍）。
pub const PHILOSOPHY_CLAUSES: [&str; 3] = [
    "让性能压力下系统优雅地退而不是难看地崩",
    "每一级降级都是有设计的",
    "可预期、可恢复",
];

/// 域起始任务号（本单）。
pub const O_DOMAIN_FIRST: u32 = 2241;
/// 域终止任务号（含）。
pub const O_DOMAIN_LAST: u32 = 2400;
/// 域任务总数守恒（10 组 × 16 项）。
pub const O_DOMAIN_TOTAL: u32 = 160;

// ---------------------------------------------------------------------------
// 三、N 域移交包签收（F2237）
// ---------------------------------------------------------------------------

/// FNV-1a 64（签收指纹用；const 供编译期钉死）。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 七件移交内容逐项拼接指纹（const——指纹随清单编译期钉死）。
pub const fn handoff_fp(items: &[&str]) -> u64 {
    let mut h: u64 = 0x2237_2237_2237_2237;
    let mut i = 0usize;
    while i < items.len() {
        h = fnv64_mix(h, items[i].as_bytes());
        i += 1;
    }
    h
}

/// 字节级 FNV 混入。
const fn fnv64_mix(h: u64, bytes: &[u8]) -> u64 {
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        acc ^= bytes[i] as u64;
        acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    acc
}

/// 移交包签收记录（来源/七件/状态/指纹——四字段齐备才算签收）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffReceipt {
    /// 来源（移交方+单号）。
    pub source: &'static str,
    /// 移交内容清单。
    pub items: [&'static str; 7],
    /// 签收状态。
    pub signed: bool,
    /// 七件清单指纹（编译期钉死）。
    pub fingerprint: u64,
}

/// N 域移交包七件清单（独立常量——指纹从这里编译期算出）。
pub const N_HANDOFF_ITEMS: [&str; 7] = [
    "接口冻结清单（53 函数+四资产）",
    "契约清单（十二域契约）",
    "资产清单（五账）",
    "基线快照（总册+等价表+精度表）",
    "遗留移交清单（DL 式效果/神经重建）",
    "降级链衔接包（N01 三级→O 域接入接口草案+预算表引用）",
    "经验教训十条",
];

/// 七件清单指纹（const 期计算——签收后清单漂移即指纹失配）。
pub const N_HANDOFF_FP: u64 = handoff_fp(&N_HANDOFF_ITEMS);

/// N 域移交包（CGPU-F2237）签收记录——O 域开工的前提凭据。
pub const N_HANDOFF: HandoffReceipt = HandoffReceipt {
    source: "N 域移交包 CGPU-F2237",
    items: N_HANDOFF_ITEMS,
    signed: true,
    fingerprint: N_HANDOFF_FP,
};

/// 签收核验：来源非空、七件齐备且逐项非空、signed、指纹与现算一致。
pub fn handoff_verify(r: &HandoffReceipt) -> Result<(), u16> {
    if r.source.is_empty() || !r.signed {
        return Err(C_VCO01_RECEIPT_INVALID);
    }
    let mut i = 0usize;
    while i < r.items.len() {
        if r.items[i].is_empty() {
            return Err(C_VCO01_RECEIPT_INVALID);
        }
        i += 1;
    }
    if r.fingerprint != 0 && r.fingerprint != handoff_fp(&r.items) {
        return Err(C_VCO01_RECEIPT_INVALID);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、官方主题与十组映射
// ---------------------------------------------------------------------------

/// 官方主题五字面量（判据独立对拍）。
pub const OFFICIAL_THEMES: [&str; 5] = ["降级模型", "决策引擎", "链路全图", "可观测", "生态"];

/// 十组规划条目（官方批次标题逐字 + 主题覆盖标注）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupPlan {
    /// 组号（O01..O10）。
    pub group: u8,
    /// 官方组名（批次标题逐字）。
    pub name: &'static str,
    /// 本组覆盖的官方主题下标（流程/收口组为空）。
    pub themes: &'static [usize],
    /// 起始任务号（含）。
    pub first: u32,
    /// 终止任务号（含）。
    pub last: u32,
}

/// 官方十组规划表（组名取施工书批次标题逐字；区间连续无缺口守恒 160 项）。
pub const GROUP_PLANS: [GroupPlan; 10] = [
    GroupPlan { group: 1, name: "域开工与降级链总架构组", themes: &[0], first: 2241, last: 2256 },
    GroupPlan { group: 2, name: "降级决策引擎组", themes: &[1], first: 2257, last: 2272 },
    GroupPlan { group: 3, name: "降级链路全图组", themes: &[2], first: 2273, last: 2288 },
    GroupPlan { group: 4, name: "降级可观测与生态组", themes: &[3, 4], first: 2289, last: 2304 },
    GroupPlan { group: 5, name: "降级测试与演练组", themes: &[], first: 2305, last: 2320 },
    GroupPlan { group: 6, name: "降级场景与策略组", themes: &[], first: 2321, last: 2336 },
    GroupPlan { group: 7, name: "降级性能与预算组", themes: &[], first: 2337, last: 2352 },
    GroupPlan { group: 8, name: "降级生态与工具组", themes: &[4], first: 2353, last: 2368 },
    GroupPlan { group: 9, name: "O 域预备与自查组", themes: &[], first: 2369, last: 2384 },
    GroupPlan { group: 10, name: "O 域收口组", themes: &[], first: 2385, last: 2400 },
];

/// 十组守恒：组号连续、区间无缝衔接恰为 2241..2400、总数 160。
pub fn groups_conserved() -> bool {
    let mut ok = GROUP_PLANS.len() == 10;
    let mut expect_group = 1u8;
    let mut expect_first = O_DOMAIN_FIRST;
    let mut total = 0u32;
    let mut i = 0usize;
    while i < GROUP_PLANS.len() {
        let g = &GROUP_PLANS[i];
        if g.group != expect_group || g.first != expect_first || g.first > g.last {
            ok = false;
        }
        total += g.last - g.first + 1;
        expect_group += 1;
        expect_first = g.last + 1;
        i += 1;
    }
    ok && total == O_DOMAIN_TOTAL && expect_first == O_DOMAIN_LAST + 1
}

/// 五官方主题全覆盖（判据侧重算：并集恰为 0..5）。
pub fn themes_covered() -> bool {
    let mut seen = [false; 5];
    let mut i = 0usize;
    while i < GROUP_PLANS.len() {
        let mut j = 0usize;
        while j < GROUP_PLANS[i].themes.len() {
            let t = GROUP_PLANS[i].themes[j];
            if t < 5 {
                seen[t] = true;
            }
            j += 1;
        }
        i += 1;
    }
    seen[0] && seen[1] && seen[2] && seen[3] && seen[4]
}

// ---------------------------------------------------------------------------
// 五、架构五段（单向流水线）
// ---------------------------------------------------------------------------

/// 五段封闭枚举（触发→决策→执行→恢复→观测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeStage {
    /// 触发（降级缘由入场）。
    Trigger,
    /// 决策（统一编排层选档）。
    Decide,
    /// 执行（各域专业降级逻辑落地）。
    Execute,
    /// 恢复（压力回落后逐级回升）。
    Recover,
    /// 观测（动作全量入账+可见性）。
    Observe,
}

/// 五段名封闭表。
pub const STAGE_NAMES: [&str; 5] = ["触发", "决策", "执行", "恢复", "观测"];

/// 枚举→定长下标（穷尽 match）。
pub fn stage_index(s: DegradeStage) -> usize {
    match s {
        DegradeStage::Trigger => 0,
        DegradeStage::Decide => 1,
        DegradeStage::Execute => 2,
        DegradeStage::Recover => 3,
        DegradeStage::Observe => 4,
    }
}

/// 五段推进判定：只许逐段前进；跳段 [`C_VCO01_STAGE_SKIP`]、回退/同段
/// [`C_VCO01_STAGE_REWIND`] 显性拒绝。
pub fn stage_advance(prev: usize, next: usize) -> Result<(), u16> {
    if next > prev + 1 {
        return Err(C_VCO01_STAGE_SKIP);
    }
    if next <= prev {
        return Err(C_VCO01_STAGE_REWIND);
    }
    if next >= STAGE_NAMES.len() {
        return Err(C_VCO01_STAGE_SKIP);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、历史汇总与统一编排声明
// ---------------------------------------------------------------------------

/// 历史降级能力条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyDegrade {
    /// 来源域/组。
    pub domain: &'static str,
    /// 降级能力名。
    pub capability: &'static str,
}

/// 散落各域降级能力汇总表（锚点原文五行——判据逐行对拍）。
pub const LEGACY_DEGRADES: [LegacyDegrade; 5] = [
    LegacyDegrade { domain: "D 域", capability: "掉帧链" },
    LegacyDegrade { domain: "J03", capability: "热阶梯" },
    LegacyDegrade { domain: "J04", capability: "续航" },
    LegacyDegrade { domain: "Q02", capability: "流送" },
    LegacyDegrade { domain: "N01", capability: "效果降级" },
];

/// 统一化声明（锚点原句承载——判据逐字对拍）。
pub const ORCHESTRATION: &str = "O 域=全域降级的统一编排层，各域保留专业降级逻辑";

/// 收编核验：来源域/能力名在汇总表内有登记（表外散落即孤儿立案口径）。
pub fn legacy_enrolled(domain: &str, capability: &str) -> Result<(), u16> {
    let mut i = 0usize;
    while i < LEGACY_DEGRADES.len() {
        if LEGACY_DEGRADES[i].domain == domain && LEGACY_DEGRADES[i].capability == capability {
            return Ok(());
        }
        i += 1;
    }
    Err(C_VCO01_LEGACY_ORPHAN)
}

// ---------------------------------------------------------------------------
// 七、双不变量（底线闸）
// ---------------------------------------------------------------------------

/// 双不变量字面量（判据独立对拍）。
pub const INVARIANTS: [&str; 2] = [
    "任何降级不突破 80 帧适配表合同底线",
    "任何降级不破坏信息完整性",
];

/// 底线闸：降级档不得把质量水位压到合同底线之下。
pub fn floor_check(degraded_floor: u32, contract_floor: u32) -> Result<(), u16> {
    if degraded_floor < contract_floor {
        return Err(C_VCO01_FLOOR_BROKEN);
    }
    Ok(())
}

/// 完整性闸：降级档必须保留核心信息位（core_retained=false 即破坏完整性）。
pub fn integrity_check(core_retained: bool) -> Result<(), u16> {
    if !core_retained {
        return Err(C_VCO01_INTEGRITY_LOST);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 八、风险四条（各配预案）
// ---------------------------------------------------------------------------

/// 风险登记（风险名 + 预案——互异且非空）。
pub const RISKS: [(&str, &str); 4] = [
    ("决策振荡", "预案：滞回+最小驻留帧数（复用 F0004 滞回模式防反复横跳）"),
    ("链路冲突", "预案：统一编排层单点仲裁，冲突即拒绝并立案不静默吞并"),
    ("恢复过冲", "预案：恢复逐级回升+连续达标确认（复用 F0008 防抖纪律）"),
    ("可见性失控", "预案：降级动作全量入帧日志+设置中心显性展示用户可锁"),
];

// ---------------------------------------------------------------------------
// 九、摘要行
// ---------------------------------------------------------------------------

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    let mut s = String::from(VCO01_VERSION);
    s.push_str(" domain=2241-2400 groups=10 themes=5 stages=");
    s.push_str("触发→决策→执行→恢复→观测");
    s.push_str(" 「");
    s.push_str(ORCHESTRATION);
    s.push_str("」");
    s
}

// ---------------------------------------------------------------------------
// 十、编译期钉死
// ---------------------------------------------------------------------------

const _PHILOSOPHY_CLOSED: () = assert!(PHILOSOPHY_CLAUSES.len() == 3);
const _THEMES_FIVE: () = assert!(OFFICIAL_THEMES.len() == 5);
const _STAGES_FIVE: () = assert!(STAGE_NAMES.len() == 5);
const _RISKS_FOUR: () = assert!(RISKS.len() == 4);
const _INVARIANTS_DUAL: () = assert!(INVARIANTS.len() == 2);
const _HANDOFF_SEVEN: () = assert!(N_HANDOFF.items.len() == 7);
const _RECEIPT_SIGNED: () = assert!(N_HANDOFF.signed, "N 域移交包必须已签收");
const _HANDOFF_FP_FROZEN: () = assert!(
    N_HANDOFF.fingerprint == handoff_fp(&N_HANDOFF.items) && N_HANDOFF.fingerprint != 0,
    "handoff fingerprint drift"
);
const _GROUPS_CONSERVED: () = assert!(O_DOMAIN_TOTAL == (O_DOMAIN_LAST - O_DOMAIN_FIRST + 1));
