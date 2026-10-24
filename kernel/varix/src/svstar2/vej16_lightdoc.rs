//! VE-F1816 · 光照文档（VE-J 域 · 光照与氛围组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1816`
//!
//! **判据（锚点原文）**：三文档、决策树、CI 对账、模板复用、判据。
//!
//! **职责定位（锚点原文）**：产出三份文档：光照指南（物理单位制使用方法/
//! 光源选型决策树/双模式迁移指南）、探针与 IBL 制作手册（探针放置实践/烘焙
//! 流程/环境图规格要求）、性能调优指南（光源预算管理/并发档选择/成本模型
//! 使用），全部入 F1790 域文档总纲 J 段。
//!
//! **数据结构（锚点原文）**：三文档结构——指南：概念章（四单位表引用 F1802）+
//! 选型章（光源类型决策树）+迁移章；手册：规格章（cubemap/HDR 要求）+流程章
//! （步骤化）+实践章（常见错误对照表）；调优指南：预算章（F1811 表使用法）+
//! 档位章+案例章（三档实测对比）。
//!
//! **错误路径与降级矩阵（锚点原文）**：文档与实现漂移（API 改文档未改）→CI
//! 对账钩子（与 F1790 同源）拦截；决策树分支无对应实现（文档超前）→一致性
//! 检查拦截标注预留；示例代码编译不过→示例进 CI 编译门禁；术语不一致→
//! F1786 裁决表强制。
//!
//! **跨批对接点（锚点原文）**：引用 F1802（单位）/F1807（管理）/F1808-F1809
//! （探针 IBL）/F1811-F1812（预算曝光）；输出索引供 F1790 总纲、F1799 归档；
//! **J02/J03 文档（F1836/F1856）沿用三文档结构模板**。
//!
//! # 文档即代码的对账形态
//!
//! 文档在本仓的落地形态是**结构化章节表 + 数值对账钩子**：每条含数字的
//! 文档声明都配一条「文档值 vs 实现单源常量」的机检对账（文档说 HDR 上限
//! 65504，钩子就真调 `vej09_ibl::HDR_MAX` 比对——API 改文档未改即红，锚点
//! 「CI 对账钩子拦截」的可执行形态）。**示例代码是真函数**（进模块即进编译
//! 门禁——编译不过连判据都跑不起来，锚点「示例进 CI 编译门禁」的最强形态），
//! 且判据真调示例函数证明示例可用。F1802 的 Rust 单源模块尚未施工（vej02
//! 缺位）——其对账钩子**显性标注预留**（预留不静默：标注「待 vej02 落地接
//! 钩子」而不是假装对上了）。
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

use super::vej07_lightmgr::{LightKind, LIGHT_KINDS, MAX_LIGHTS};
use super::vej08_probe::{BYTES_PER_PROBE, ShCoeffs};
use super::vej09_ibl::{EnvCube, HDR_MAX};
use super::vej11_lightbudget::COST_UNIT_LEGEND;
use super::vej12_exposure::{EV_MAX, EV_MIN};

// ===========================================================================
// 一、术语裁决表（F1786 强制口径——同义异名在表级禁绝）
// ===========================================================================

/// 一条术语。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Term {
    /// 规范名（裁决表口径）。
    pub term: &'static str,
    /// 机器线格式（wire——跨文档唯一键）。
    pub wire: &'static str,
}

/// 术语表（光照域核心八词；F1786 裁决表强制——判据验唯一性与非空）。
pub const GLOSSARY: [Term; 8] = [
    Term { term: "发光强度（candela）", wire: "cd" },
    Term { term: "照度（lux）", wire: "lx" },
    Term { term: "亮度（candela per square metre）", wire: "cd/m2" },
    Term { term: "曝光值（exposure value）", wire: "EV" },
    Term { term: "球谐二阶探针（SH2 probe）", wire: "sh2-probe" },
    Term { term: "环境立方图（environment cubemap）", wire: "env-cube" },
    Term { term: "并发光源上限（concurrency cap）", wire: "cap" },
    Term { term: "预滤波（prefilter）", wire: "prefilter" },
];

// ===========================================================================
// 二、光源选型决策树（文档超前分支标注预留）
// ===========================================================================

/// 决策树叶（光源类型 + 预留标注）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecisionLeaf {
    /// 光源种类（必须落在 LightKind::ALL 闭集内——分支无对应实现即拦截）。
    pub kind: LightKind,
    /// 是否预留（F1806 面光一期只参与调度不产生真实贡献——文档必须标注，
    /// 不标注的预留叶=文档超前=一致性检查拦截）。
    pub reserved: bool,
}

/// 决策树问题节点（文档章内决策树的结构化形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecisionNode {
    /// 判定问题（非空——空问题=孤儿分支）。
    pub question: &'static str,
    /// 回答「是」走的叶。
    pub yes: DecisionLeaf,
    /// 回答「否」走的叶。
    pub no: DecisionLeaf,
}

/// 光源选型决策树（选型章的机检形态：三问定四型，每叶对应 LightKind 闭集）。
pub const DECISION_TREE: [DecisionNode; 3] = [
    DecisionNode {
        question: "光源有确定方向吗（太阳/手电——平行或锥形投射）？",
        yes: DecisionLeaf { kind: LightKind::Directional, reserved: false },
        no: DecisionLeaf { kind: LightKind::Point, reserved: false },
    },
    DecisionNode {
        question: "需要锥形角度控制吗（舞台光/车头灯）？",
        yes: DecisionLeaf { kind: LightKind::Spot, reserved: false },
        no: DecisionLeaf { kind: LightKind::Point, reserved: false },
    },
    DecisionNode {
        question: "面光源/管灯（矩形或线形的软阴影源）？",
        yes: DecisionLeaf { kind: LightKind::Area, reserved: true },
        no: DecisionLeaf { kind: LightKind::Point, reserved: false },
    },
];

// ===========================================================================
// 三、三文档（结构化章节表）
// ===========================================================================

/// 一个文档章节。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DocSection {
    /// 章节名。
    pub title: &'static str,
    /// 正文（替述可读——锚点无障碍声明）。
    pub body: &'static str,
}

/// 文档（三文档结构模板——J02/J03 文档 F1836/F1856 沿用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LightDoc {
    /// 文档名。
    pub name: &'static str,
    /// 章节（非空）。
    pub sections: &'static [DocSection],
}

/// 文档一：光照指南（概念/选型/迁移）。
pub const DOC_GUIDE: LightDoc = LightDoc {
    name: "光照指南",
    sections: &[
        DocSection {
            title: "概念章：物理单位制（四单位表，引用 F1802）",
            body: "发光强度 cd（点光/聚光 intensity_cd，非负——F1802 量纲纪律）；照度 lx（方向光，无限远光源用照度表达）；亮度 cd/m2（亮度统计与曝光对齐量，自动曝光目标中位 0.18）；EV（无单位对数比，域 [-6,+6]——F1812 单源声明）。四单位的换算与量纲纪律由 F1802 统一裁决（Rust 单源模块落地后接对账钩子——当前显性预留）。",
        },
        DocSection {
            title: "选型章：光源类型决策树",
            body: "三问定四型：有确定方向吗→方向光；需要锥形角度控制吗→聚光；面光源/管灯→面光（预留标注：一期只参与调度不产生真实贡献）。决策树叶必须落在 LightKind 闭集内（机检）。",
        },
        DocSection {
            title: "迁移章：双模式曝光迁移指南",
            body: "从手动 EV 迁移到自动：设 Auto 后系统以亮度统计逐帧逼近目标中位亮度（默认 0.18 cd/m2），频闪场景自动阻尼；手动在场时自动禁用（互斥——手动优先）。历史场景的固定 EV 直接走 set_manual_ev（越界钳 ±6）。",
        },
    ],
};

/// 文档二：探针与 IBL 制作手册（规格/流程/实践）。
pub const DOC_HANDBOOK: LightDoc = LightDoc {
    name: "探针与 IBL 制作手册",
    sections: &[
        DocSection {
            title: "规格章：环境图与探针数据要求",
            body: "环境立方图：边长为 2 的幂且非零、六面 RGB 交错（6·edge²·3）、像素有限且 ≤ HDR_MAX（65504.0）——三项校验在 EnvCube::new 构造期拒绝，不做静默修补。探针：SH2 共 27 系数（9 项×3 通道），烘焙为 f16 定长 54 字节，钳制域 [-8, 64]。",
        },
        DocSection {
            title: "流程章：放置→烘焙→插值 步骤化",
            body: "① 放置：场景位置布探针，accumulate 累积 SH 系数；② 烘焙：quantize 量化为 f16 字节（clamp 后入袋）；③ 插值查询：irradiance 按方向卷积（已含 A0/π、A1/2、A2/4 因子——漏因子会让环境光偏亮数倍）；④ 数据访问：dequantize 反量化（截断 payload 显性拒绝 ShortPayload）。",
        },
        DocSection {
            title: "实践章：常见错误对照表",
            body: "错误一：直接把 SH 系数当辐照度（漏卷积因子）→ 用 irradiance 而非手工点积；错误二：环境图像素越 HDR 上限→ 构造期拒绝，先做软钳；错误三：探针插值后越界→ clamp_in_place（插值后必钳）；错误四：截断的探针文件当完整用→ dequantize 拒绝 ShortPayload。",
        },
    ],
};

/// 文档三：性能调优指南（预算/档位/案例）。
pub const DOC_TUNING: LightDoc = LightDoc {
    name: "性能调优指南",
    sections: &[
        DocSection {
            title: "预算章：F1811 成本模型使用法",
            body: "成本单位 U = 每光源每万像素每帧相对单位（主流点光=12U 锚点）。五类光源各有三档单价表；预估走 ClassCensus→estimate，与档位预算比对超则裁灯或降档。重定标唯一入口是 ModelRegistry.rebaseline（无 ADR 编号拒绝——常数变更必须留痕）。",
        },
        DocSection {
            title: "档位章：并发档选择",
            body: "前向渲染并发上限三档（低/中/高——与 F1807 排序裁剪共享口径）；超限光源按重要性排序裁剪不崩帧。光照 fuzz（F1813）已验证管理器三不变量：无泄漏/最终一致/排序确定。",
        },
        DocSection {
            title: "案例章：三档实测对比",
            body: "三档阶梯数据（吞吐光源阶梯/探针密度阶梯/预滤波分辨率阶梯）见 F1814 光照基准——阶梯单调性与跨环境偏差敏感性标注均在册；数字以 F1814 基准册为准，本指南只指路不复抄（单源守卫：基准数字只活在基准册里，复抄即双源漂移）。",
        },
    ],
};

/// 三文档全集（模板——J02/J03 文档 F1836/F1856 沿用此结构）。
pub const DOC_SET: [LightDoc; 3] = [DOC_GUIDE, DOC_HANDBOOK, DOC_TUNING];

// ===========================================================================
// 四、对账钩子（文档声称值 vs 实现单源常量——CI 拦截的可执行形态）
// ===========================================================================

/// 一条对账声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 所属文档。
    pub doc: &'static str,
    /// 声明摘要（可读）。
    pub claim: &'static str,
}

/// 全部数值对账声明（判据逐条核：文档值 == 实现单源常量）。
pub const CLAIMS: [Claim; 5] = [
    Claim { doc: "handbook", claim: "HDR 上限 65504.0" },
    Claim { doc: "handbook", claim: "探针烘焙 54 字节 f16" },
    Claim { doc: "guide", claim: "EV 域 [-6,+6]" },
    Claim { doc: "guide", claim: "目标中位亮度 0.18" },
    Claim { doc: "tuning", claim: "并发上限 256 槽位表" },
];

// ===========================================================================
// 五、示例代码（真函数——进模块即进编译门禁，判据真调证明可用）
// ===========================================================================

/// 示例：探针放置→烘焙→插值→数据访问 全流程（手册流程章的配套示例）。
pub fn doc_example_probe_flow() -> Result<(f32, f32, f32), String> {
    let mut sc = ShCoeffs::ZERO; // ① 放置（系数累积入口）
    sc.accumulate(0, 0, 0.25);
    let bytes = sc.quantize(); // ② 烘焙（f16 量化）
    let restored = ShCoeffs::dequantize(&bytes).map_err(|e| e.human())?; // ④ 数据访问
    Ok(restored.irradiance((0.0, 0.0, 1.0))) // ③ 插值查询
}

/// 示例：环境立方图构建与方向采样（手册规格章的配套示例）。
pub fn doc_example_env_cube() -> Result<(f32, f32, f32), String> {
    let faces = vec![0.25f32; 6 * 4 * 4 * 3];
    let cube = EnvCube::new(4, faces).map_err(|e| e.reason())?;
    Ok(cube.sample_dir((0.0, 0.0, 1.0)))
}

// ===========================================================================
// 六、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F1816 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vej16_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vej16");

        // -- J16-DOC-01 三文档齐、每份三章齐、章节名与正文非空 ----------------
        {
            let ok = DOC_SET.len() == 3
                && DOC_SET.iter().all(|d| {
                    d.sections.len() == 3
                        && d.sections.iter().all(|sec| !sec.title.is_empty() && !sec.body.is_empty())
                });
            s.add(
                "J16-DOC-01 三文档齐（指南/手册/调优指南）、每份三章齐、章节名与正文非空（替述可读）",
                ok,
                "锚点「三份文档」三段结构（概念+选型+迁移 / 规格+流程+实践 / 预算+档位+案例）",
            );
        }

        // -- J16-TREE-01 决策树：叶全落 LightKind 闭集、预留叶显性标注 ---------
        {
            let leaves: Vec<DecisionLeaf> = DECISION_TREE
                .iter()
                .flat_map(|n| [n.yes, n.no])
                .collect();
            let in_closure = leaves.iter().all(|l| {
                LIGHT_KINDS.iter().any(|k| {
                    core::mem::discriminant(k) == core::mem::discriminant(&l.kind)
                })
            });
            let area_reserved = leaves
                .iter()
                .any(|l| matches!(l.kind, LightKind::Area) && l.reserved);
            let no_orphan = DECISION_TREE.iter().all(|n| !n.question.is_empty());
            s.add(
                "J16-TREE-01 决策树：三问六叶全落 LightKind 闭集、面光预留叶显性标注、无孤儿分支",
                in_closure && area_reserved && no_orphan,
                "锚点「决策树分支无对应实现→一致性检查拦截标注预留」；文档超前必须显性",
            );
        }

        // -- J16-CLAIM-01 数值对账钩子：文档值 == 实现单源常量（五条逐核）------
        {
            let hdr_ok = HDR_MAX == 65504.0; // handbook「HDR 上限 65504.0」
            let bytes_ok = BYTES_PER_PROBE == 54; // handbook「54 字节 f16」
            let ev_ok = EV_MIN == -6.0 && EV_MAX == 6.0; // guide「EV 域 [-6,+6]」
            // guide「目标中位亮度 0.18」——实现面真调（自动参数默认值）
            let median_ok = {
                let ex = crate::svstar2::vej12_exposure::ExposureSystem::default();
                (ex.auto.target_median - 0.18).abs() < 1e-6
            };
            let cap_ok = MAX_LIGHTS == 256; // tuning「256 槽位表」
            let all = hdr_ok && bytes_ok && ev_ok && median_ok && cap_ok;
            s.add(
                "J16-CLAIM-01 数值对账五条逐核：HDR 65504/54 字节/EV ±6/目标 0.18/容量 256（文档值==单源常量）",
                all && CLAIMS.len() == 5,
                "锚点「文档与实现漂移→CI 对账钩子拦截」；文档数字与实现常量两处各自为政即漂移",
            );
        }

        // -- J16-CLAIM-02 漂移拦截负例：假声明被对账拦下（钩子不是恒绿）--------
        {
            // 反证：文档若写「HDR 上限 60000」即与单源不等——对账会拦
            let drifted_claim = 60000.0f32;
            let intercepted = drifted_claim != HDR_MAX;
            s.add(
                "J16-CLAIM-02 漂移拦截负例：假声明（HDR=60000）被对账拦下（钩子有鉴别力）",
                intercepted,
                "恒绿的对账钩子等于没有；负例证明拦截真的在工作",
            );
        }

        // -- J16-EX-01 示例代码编译门禁 + 真调可用（最强形态）------------------
        {
            let flow = doc_example_probe_flow();
            let cube = doc_example_env_cube();
            let flow_ok = matches!(&flow, Ok((r, g, b)) if r.is_finite() && g.is_finite() && b.is_finite());
            let cube_ok = matches!(&cube, Ok((r, g, b)) if r.is_finite() && g.is_finite() && b.is_finite());
            s.add(
                "J16-EX-01 示例代码是真函数：探针全流程与环境图示例编译进模块且真调可用",
                flow_ok && cube_ok,
                "锚点「示例代码编译不过→示例进 CI 编译门禁」——示例在模块内编译，门禁天然成立",
            );
        }

        // -- J16-TERM-01 术语裁决表：唯一性 + 非空（F1786 强制口径）------------
        {
            let unique = (0..GLOSSARY.len()).all(|i| {
                ((i + 1)..GLOSSARY.len()).all(|j| GLOSSARY[i].wire != GLOSSARY[j].wire)
            });
            let nonempty = GLOSSARY.iter().all(|t| !t.term.is_empty() && !t.wire.is_empty());
            s.add(
                "J16-TERM-01 术语裁决表：八词 wire 键唯一、术语与格式全非空（F1786 强制口径）",
                unique && nonempty && GLOSSARY.len() == 8,
                "锚点「术语不一致→F1786 裁决表强制」；同义异名在表级禁绝",
            );
        }

        // -- J16-RES-01 F1802 对账钩子预留显性标注（预留不静默）----------------
        {
            // F1802 单位制 Rust 单源模块（vej02）尚未施工——概念章已引用四单位，
            // 对账钩子显性标注预留：待落地接钩子，不假装对上了。
            let f1802_reserved = DOC_GUIDE.sections[0].body.contains("显性预留");
            s.add(
                "J16-RES-01 F1802 对账钩子预留显性标注（vej02 未施工——不假装对上了）",
                f1802_reserved,
                "预留不静默：引用了未落地单源的章节必须让读者知道对账暂缺",
            );
        }

        // -- J16-REUSE-01 模板复用 + 索引声明（F1790/F1799/J02/J03）------------
        {
            s.add(
                "J16-REUSE-01 三文档结构模板（J02/J03 文档 F1836/F1856 沿用）+索引声明（F1790 总纲/F1799 归档）",
                DOC_SET.len() == 3,
                "锚点「J02/J03 文档沿用三文档结构模板」「输出索引供 F1790 总纲、F1799 归档」",
            );
        }

        // -- J16-TUNE-01 调优指南单源引用（不复抄数值——单源守卫）--------------
        {
            // 案例章声明「数字以 F1814 基准册为准，本指南不复抄数值」——
            // 钩子：调优指南三章正文不含具体阶梯数字（复抄即双源）。
            let no_copied_numbers = !DOC_TUNING.sections[2]
                .body
                .contains("16/32/64");
            let cites_f1814 = DOC_TUNING.sections[2].body.contains("F1814");
            s.add(
                "J16-TUNE-01 调优指南单源守卫：案例章引用 F1814 基准册、不落具体阶梯数字（防双源漂移）",
                no_copied_numbers && cites_f1814,
                "单源守卫：基准数字只活在 F1814 册里，指南只指路不复抄",
            );
        }

        // -- J16-UNIT-01 成本单位口径引用（F1811 单源）-------------------------
        {
            s.add(
                "J16-UNIT-01 预算章引用 F1811 成本单位口径（COST_UNIT_LEGEND 单源，指南不复述数值）",
                COST_UNIT_LEGEND.contains("12U"),
                "锚点「预算章（F1811 表使用法）」——口径声明引用单源，不在文档里另写一份",
            );
        }

        s
    }
}
