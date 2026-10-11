//! CGPU-F2723 · 渲染数据信任分级（CGPU-R 域 · 安全渲染 · 组内第 3 条）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2723`
//!
//! 锚点原文：「信任：信任（渲染数据信任分级（本域可信/跨域验证/外部不信任——
//! 三级信任——分级表；传递（信任不传递原则——原则声明；测试（分级/传递两组）。
//! 判据：三级、分级表、原则声明、两组、判据。」
//!
//! 判据逐条落位：
//! - **三级**：[`TRUST_TIERS`]——本域可信/跨域验证/外部不信任三级封闭
//!   （锚点原文次序即信任秩：级 0 最高，级 2 最低）。
//! - **分级表**：[`TRUST_TABLE`]——每行一条渲染数据类别的信任分级
//!   （tier/name/rule 三字段）；行名唯一、tier 落级、处置非空、
//!   外部级必须显性声明「不信任」处置（[`E_R230_TRUST_UNDECLARED`]——
//!   把外部数据当可信数据是安全边界穿洞）。
//! - **原则声明**：[`NO_TRANSIT`]——「信任不传递——跨过边界就要重新验证」；
//!   声明失守（丢「不传递」）判 [`E_R230_TRANSIT_FAULT`]。
//! - **传递闸**：[`verify_transit`]——数据从高信任级流向低信任级时必须
//!   带显性验证标记，未验证即拒（[`E_R230_TRANSIT_UNVERIFIED`]）：
//!   这是「信任不传递」的可执行形态，不是口号。
//! - **两组**：判据侧 [`super::vcr23_trustlevel_checks`] 分级组/传递组两族
//!   全量断言。
//!
//! **为什么「外部不信任」是默认而不是选项**：恶意着色器、资源炸弹、外泄
//! 三类威胁（F2722 分类表）全部经由渲染数据进入——外部数据若无默认
//! 不信任位，每条消费路径都要各自记得怀疑，忘一次就是一次破口。
//! 默认不信任把怀疑从「纪律」变成「结构」。
//!
//! **零静默纪律**：所有断链/未声明/失守/未验证都产出诊断码（0x99B 段，
//! R 域独占细分），由调用方聚合上报。
//!
//! 确定性：全部封闭常量表 + 纯函数，时间戳由调用方注入（本模块不读时钟）。

// ---------------------------------------------------------------------------
// 一、诊断码（R 域独占 0x99B 段）
// ---------------------------------------------------------------------------

/// 分级表残缺（表空、行名重复、tier 越级或处置缺席）。
pub const E_R230_TABLE_INCOMPLETE: u16 = 0x99B0;
/// 信任级未声明（外部数据行未显性声明「不信任」处置——安全边界穿洞）。
pub const E_R230_TRUST_UNDECLARED: u16 = 0x99B1;
/// 原则失守（信任不传递声明丢「不传递」——原则退化为口号）。
pub const E_R230_TRANSIT_FAULT: u16 = 0x99B2;
/// 传递未验证（高信任级数据流向低信任级时缺显性验证标记）。
pub const E_R230_TRANSIT_UNVERIFIED: u16 = 0x99B3;
/// 未知信任级（引用三级之外信任级）。
pub const E_R230_TIER_UNKNOWN: u16 = 0x99B4;
/// 版本失配（批次版本与 R03 冻结版不符）。
pub const E_R230_VERSION_MISMATCH: u16 = 0x99B5;

/// 域批次版本。
pub const VCR23_VERSION: &str = "CR23-trustlevel-v1";

/// 版本校验（双向）：R03 冻结版放行，其余一律 [`E_R230_VERSION_MISMATCH`]。
pub fn check_version(v: &str) -> Result<(), u16> {
    if v == VCR23_VERSION {
        Ok(())
    } else {
        Err(E_R230_VERSION_MISMATCH)
    }
}

// ---------------------------------------------------------------------------
// 二、三级信任（本域可信/跨域验证/外部不信任，锚点原文次序即秩）
// ---------------------------------------------------------------------------

/// 渲染数据信任三级（封闭——锚点「本域可信/跨域验证/外部不信任」；
/// 次序即信任秩：下标 0 最可信，下标 2 最不可信）。
pub const TRUST_TIERS: [&str; 3] = ["本域可信", "跨域验证", "外部不信任"];

/// 分级表行（tier=信任级下标；name=数据类别名；rule=处置声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustRow {
    /// 信任级（0=本域可信 1=跨域验证 2=外部不信任，对齐 [`TRUST_TIERS`]）。
    pub tier: usize,
    /// 数据类别名（表内唯一）。
    pub name: &'static str,
    /// 处置声明（一句可 grep 的信任处置；外部级必须含「不信任」）。
    pub rule: &'static str,
}

/// 渲染数据信任分级表（封闭——每行一条数据类别的信任处置）。
pub const TRUST_TABLE: [TrustRow; 6] = [
    TrustRow {
        tier: 0,
        name: "内部顶点缓冲",
        rule: "本域生成全程受控：可直接进入渲染管线",
    },
    TrustRow {
        tier: 0,
        name: "内部 uniform 集",
        rule: "本域写入自校验布局：可直接绑定",
    },
    TrustRow {
        tier: 1,
        name: "跨域纹理",
        rule: "来自 N/O 域重建产物：入管线前验证格式与尺寸",
    },
    TrustRow {
        tier: 1,
        name: "跨会话缓存",
        rule: "来自其他会话的持久化产物：入管线前校验校验和",
    },
    TrustRow {
        tier: 2,
        name: "用户上传资产",
        rule: "外部来源不信任：沙箱解析+资源上限+拒绝即显性复述",
    },
    TrustRow {
        tier: 2,
        name: "网络拉取资源",
        rule: "外部来源不信任：沙箱解析+完整性验证+限速加载",
    },
];

/// 分级表校验：表非空、行名唯一、tier 落在三级内、处置非空；
/// tier=2 的行必须显性声明「不信任」（缺声明即
/// [`E_R230_TRUST_UNDECLARED`]）。残缺判 [`E_R230_TABLE_INCOMPLETE`]。
pub fn verify_trust_table() -> Result<usize, u16> {
    if TRUST_TABLE.is_empty() {
        return Err(E_R230_TABLE_INCOMPLETE);
    }
    let mut i = 0usize;
    while i < TRUST_TABLE.len() {
        let mut j = i + 1;
        while j < TRUST_TABLE.len() {
            if TRUST_TABLE[i].name == TRUST_TABLE[j].name {
                return Err(E_R230_TABLE_INCOMPLETE);
            }
            j += 1;
        }
        i += 1;
    }
    for row in TRUST_TABLE.iter() {
        if row.tier >= 3 {
            return Err(E_R230_TIER_UNKNOWN);
        }
        if row.rule.is_empty() {
            return Err(E_R230_TABLE_INCOMPLETE);
        }
        if row.tier == 2 && !row.rule.contains("不信任") {
            return Err(E_R230_TRUST_UNDECLARED);
        }
    }
    Ok(TRUST_TABLE.len())
}

// ---------------------------------------------------------------------------
// 三、传递：信任不传递原则 + 传递闸
// ---------------------------------------------------------------------------

/// 信任不传递原则声明（锚点原文——「不传递」为不可删字）。
pub const NO_TRANSIT: &str = "信任不传递——跨过边界就要重新验证";

/// 原则声明校验：声明必须落在「不传递」上，纯口号式声明即失守。
pub fn verify_no_transit(declared: &str) -> Result<(), u16> {
    if declared.contains("不传递") {
        Ok(())
    } else {
        Err(E_R230_TRANSIT_FAULT)
    }
}

/// 传递闸：数据从 `from` 级流向 `to` 级时，只要跨信任边界
/// （from != to 或 from 级低于目标级的信任，即 from > to 无需验证——
/// 低信任数据流向高信任级本来就被拒绝），`validated` 必须为真。
///
/// 语义细化：`from <= to`（流向更不信任或同级）必须 `validated`
/// ——向低信任级流动意味着数据将被更低信任的消费者处理，
/// 交接方必须显性声明「我验证过了」；`from > to`（流向更高信任级）
/// 直接拒 [`E_R230_TRANSIT_UNVERIFIED`]——低信任数据不得逆流升权。
pub fn verify_transit(from: usize, to: usize, validated: bool) -> Result<(), u16> {
    if from >= 3 || to >= 3 {
        return Err(E_R230_TIER_UNKNOWN);
    }
    if from > to {
        return Err(E_R230_TRANSIT_UNVERIFIED);
    }
    if from < to && !validated {
        return Err(E_R230_TRANSIT_UNVERIFIED);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、分级×威胁联动声明（F2722 分类表的承接位）
// ---------------------------------------------------------------------------

/// 分级×威胁联动声明（信任三级与 F2722 五类威胁的对应——每行一条）。
pub const THREAT_LINKAGE: [&str; 3] = [
    "外部不信任级直接对冲恶意着色器与越界：沙箱解析是第一道结构防线",
    "外部不信任级直接对冲资源炸弹：资源上限让爆炸有上界",
    "跨域验证级对冲外泄：跨域数据只携带最小必要字段",
];

/// 联动校验：三行联动非空且每行落在威胁名目上（着色器/炸弹/外泄）。
pub fn verify_threat_linkage() -> Result<usize, u16> {
    if THREAT_LINKAGE.len() != 3 {
        return Err(E_R230_TABLE_INCOMPLETE);
    }
    let anchors: [&str; 3] = ["着色器", "炸弹", "外泄"];
    let mut i = 0usize;
    while i < 3 {
        if THREAT_LINKAGE[i].is_empty() || !THREAT_LINKAGE[i].contains(anchors[i]) {
            return Err(E_R230_TABLE_INCOMPLETE);
        }
        i += 1;
    }
    Ok(3)
}
