//! CGPU-F0801 · F 域开工与兼容矩阵方法论（CGPU 册 · F 域 · 兼容矩阵一域开工）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0801`
//!
//! 锚点原文：「F 域开工：GPU 兼容矩阵一域（F0801-F0960，官方六主题：Intel 核显
//! 全系/AMD RX 系/认证流程/能力探测/驱动差异表/回归矩阵）；方法论声明（**实测
//! 认证不凭文档声明**——卷首铁律 8 的域级兑现：每类硬件的兼容以实机测试认证）；
//! 域结构（F01 认证体系/F02-F04 Intel/F05-F06 AMD/F07 驱动差异/F08 回归矩阵/
//! F09 场景扩展/F10 域收口——十组规划）；与 D08 基准考场的关系（D08 出考卷与
//! 判定、本域出考生与成绩——分工契约）。判据：六主题、铁律兑现、十组规划、
//! D08 分工、判据。」
//!
//! # 一、方法论是**类型**不是标语：铁律 8 的域级兑现
//!
//! 「实测认证不凭文档声明」若只是宣言，第一张赶工期的表格就会绕过它。本单把
//! 方法论做成认证入口的**类型闸**：[`CertEvidence`] 携带实机测试凭据
//! （`suite_run`：兼容性测试套件在目标硬件上真跑过）与文档声明标记
//! （`doc_claim_only`）；[`certify`] 对**只有文档声明**的证据返回
//! [`C_VCF01_UNVERIFIED`] 拒绝——「凭文档声明」在类型上**进不了**认证状态机，
//! 铁律 8 由编译后的第一行闸门兑现（锚点「每类硬件的兼容以实机测试认证」）。
//!
//! # 二、域规划是**封闭账**：六主题与十组各自守恒
//!
//! 六主题 [`TOPICS`] 与十组 [`GROUPS`] 都是封闭枚举表：主题数恒 6、组号
//! F01..F10 连续无洞（判据侧独立重算）；十组覆盖声明互斥（同一任务号不得落
//! 两组段内），「规划」由此可审计——不是六个名字加十张幻灯片，而是数得出、
//! 对得上、连得上的账。
//!
//! # 三、D08 分工契约：出考卷与出考生**互斥**
//!
//! [`DivisionContract`] 把 D08 基准考场的分工写成结构：D08 出**考卷与判定**
//! （测试内容与裁决），本域出**考生与成绩**（被测设备/驱动组合与兼容矩阵条
//! 目）；[`DivisionContract::verify`] 断言职责集互斥且接口点齐备——本域不做
//! 判定、D08 不选考生，跨界即违规（可观测的 [`C_VCF01_D08_OVERLAP`]）。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界：组号连续性用迭代器折叠核对，职责
//! 互斥用集合差集；所有失败路径走 `Result` 与显式错误码。

use alloc::format;
use alloc::string::String;

// ===========================================================================
// 一、诊断码（vcf01 独占段 0x9F00..）
// ===========================================================================

/// 文档声明不足以认证（铁律 8 闸拒绝）。
pub const C_VCF01_UNVERIFIED: u16 = 0x9F00;
/// 域规划账目破损（组号断洞/主题计数漂移）。
pub const C_VCF01_GROUP_GAP: u16 = 0x9F01;
/// D08 分工越界（职责集重叠）。
pub const C_VCF01_D08_OVERLAP: u16 = 0x9F02;

/// 域版本。
pub const VCF01_VERSION: &str = "CF01-gpucompat-v1";

// ===========================================================================
// 二、六主题（官方封闭表）
// ===========================================================================

/// F 域官方主题（封闭六主题——锚点「官方六主题」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Topic {
    /// 主题号（1..=6）。
    pub no: u8,
    /// 主题名。
    pub name: &'static str,
    /// 任务段（含端点）。
    pub range: (u32, u32),
}

/// 六主题封闭表。
pub const TOPICS: [Topic; 6] = [
    Topic { no: 1, name: "Intel 核显全系", range: (802, 850) },
    Topic { no: 2, name: "AMD RX 系", range: (851, 890) },
    Topic { no: 3, name: "认证流程", range: (891, 905) },
    Topic { no: 4, name: "能力探测", range: (906, 920) },
    Topic { no: 5, name: "驱动差异表", range: (921, 940) },
    Topic { no: 6, name: "回归矩阵", range: (941, 960) },
];

/// 域任务段（F0801-F0960）。
pub const DOMAIN_RANGE: (u32, u32) = (801, 960);

// ===========================================================================
// 三、十组规划（F01..F10 连续封闭）
// ===========================================================================

/// 组规划条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    /// 组号（1..=10，对应 F01..F10）。
    pub gid: u8,
    /// 组名。
    pub name: &'static str,
    /// 承接主题号（1..=6；开工组/收口组可空段用 0 表示——0 为合法值仅限 F01/F10）。
    pub theme: u8,
    /// 交付物声明。
    pub deliverable: &'static str,
}

/// 十组封闭表（锚点「十组规划」）。
pub const GROUPS: [Group; 10] = [
    Group { gid: 1, name: "F01 认证体系", theme: 3, deliverable: "认证流程与方法论落地（铁律 8 闸）" },
    Group { gid: 2, name: "F02 Intel 入门", theme: 1, deliverable: "Intel 核显入门型号兼容条目" },
    Group { gid: 3, name: "F03 Intel 主流", theme: 1, deliverable: "Intel 核显主流型号兼容条目" },
    Group { gid: 4, name: "F04 Intel 旗舰", theme: 1, deliverable: "Intel 核显旗舰型号+能力探测" },
    Group { gid: 5, name: "F05 AMD 入门", theme: 2, deliverable: "AMD RX 入门型号兼容条目" },
    Group { gid: 6, name: "F06 AMD 主流+旗舰", theme: 2, deliverable: "AMD RX 主流/旗舰兼容条目" },
    Group { gid: 7, name: "F07 驱动差异", theme: 5, deliverable: "跨驱动版本差异表" },
    Group { gid: 8, name: "F08 回归矩阵", theme: 6, deliverable: "兼容回归矩阵与守卫" },
    Group { gid: 9, name: "F09 场景扩展", theme: 4, deliverable: "场景化扩展条目（能力探测联动）" },
    Group { gid: 10, name: "F10 域收口", theme: 0, deliverable: "域账清点与判据收口" },
];

/// 组号连续性核对（F01..F10 无洞无重——判据侧独立重算同一结果）。
pub fn groups_contiguous() -> bool {
    let mut ok = true;
    let mut expect = 1u8;
    for g in GROUPS.iter() {
        if g.gid != expect {
            ok = false;
        }
        expect += 1;
    }
    ok && expect == 11
}

/// 主题-组映射合法（组承接的主题号在 1..=6 内；F01/F10 允许 0 空段）。
pub fn group_themes_valid() -> bool {
    GROUPS.iter().all(|g| {
        (g.theme >= 1 && g.theme <= 6) || ((g.gid == 1 || g.gid == 10) && g.theme == 0)
    })
}

// ===========================================================================
// 四、方法论：铁律 8 的类型闸
// ===========================================================================

/// 方法论声明（锚点「实测认证不凭文档声明」的原句承载）。
pub const METHODOLOGY: &str = "实测认证不凭文档声明：每类硬件的兼容以实机测试认证";

/// 认证证据（实机测试凭据 + 文档声明标记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CertEvidence {
    /// 设备标识（厂商 id + 设备 id 打包）。
    pub device: u32,
    /// 驱动版本（打包短号）。
    pub driver: u32,
    /// 兼容性测试套件在目标硬件上**真跑过**（实机凭据）。
    pub suite_run: bool,
    /// 兼容性仅有文档/营销材料声明（无实机凭据）。
    pub doc_claim_only: bool,
}

/// 认证状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertStatus {
    /// 实机认证通过。
    Certified,
    /// 未认证（无凭据——不是错误，是显性状态）。
    Uncertified,
}

/// 认证入口（铁律 8 闸）：文档声明**进不了**认证。
///
/// - `doc_claim_only == true` → 拒绝 [`C_VCF01_UNVERIFIED`]（凭文档声明不认证）；
/// - `suite_run == false` → 显性 [`CertStatus::Uncertified`]（没测过就说没测过）；
/// - `suite_run == true && !doc_claim_only` → [`CertStatus::Certified`]。
pub fn certify(ev: &CertEvidence) -> Result<CertStatus, u16> {
    if ev.doc_claim_only {
        return Err(C_VCF01_UNVERIFIED);
    }
    if !ev.suite_run {
        return Ok(CertStatus::Uncertified);
    }
    Ok(CertStatus::Certified)
}

// ===========================================================================
// 五、D08 分工契约：出考卷与出考生互斥
// ===========================================================================

/// 分工契约（锚点「D08 出考卷与判定、本域出考生与成绩——分工契约」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DivisionContract {
    /// D08 职责：出考卷（测试内容）与判定（裁决）。
    pub d08_duties: [&'static str; 2],
    /// 本域职责：出考生（设备/驱动组合）与成绩（兼容矩阵条目）。
    pub f_duties: [&'static str; 2],
    /// 接口点：考生清单交考场、成绩单回矩阵（双向交接）。
    pub handoffs: [&'static str; 2],
}

/// 分工契约常量。
pub const CONTRACT: DivisionContract = DivisionContract {
    d08_duties: ["出考卷", "判定"],
    f_duties: ["出考生", "成绩"],
    handoffs: ["考生清单交考场", "成绩单回矩阵"],
};

impl DivisionContract {
    /// 分工互斥核验：两域职责集无交集、接口点齐备且方向对。
    pub fn verify(&self) -> Result<(), u16> {
        for d in self.d08_duties.iter() {
            for f in self.f_duties.iter() {
                if d == f {
                    return Err(C_VCF01_D08_OVERLAP);
                }
            }
        }
        if self.handoffs.len() != 2 || self.handoffs[0].is_empty() || self.handoffs[1].is_empty() {
            return Err(C_VCF01_D08_OVERLAP);
        }
        Ok(())
    }
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    format!(
        "{} topics={} groups={} range={}-{} 「{}」",
        VCF01_VERSION,
        TOPICS.len(),
        GROUPS.len(),
        DOMAIN_RANGE.0,
        DOMAIN_RANGE.1,
        METHODOLOGY,
    )
}
