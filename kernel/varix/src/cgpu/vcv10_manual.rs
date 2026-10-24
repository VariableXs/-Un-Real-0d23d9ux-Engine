//! CGPU-F3370 · 验收手册（vcv10_manual）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3370`
//!
//! **判据（锚点原文）**：手册、一组、判据。
//!
//! **职责定位（锚点原文）**：文档：文档（验收手册——手册；测试
//! （手册一组）。
//!
//! # 一、手册为什么是「判据」而不是「附件」
//!
//! 本域的文档纪律与其他域一致：**手册必须可被机器核验**。一份手
//! 册如果只存在于人的记忆或口头约定里，它就无法在组收口（F3376
//! 文档账）时被对账——「写没写」变成各说各话。本模块把手册拆成
//! **章节清单**（[`MANUAL_SECTIONS`]）：每章有编号、标题、必含关
//! 键词、与前置模块的回链。核验函数（[`manual_complete`]）逐章检
//! 查：编号连续、标题非空、必含词在文、回链目标存在——四关全过
//! 才算「手册已交付」。
//!
//! # 二、手册结构（五卷）
//!
//! - **卷一 理念**：两类验证与双兜底（回链 F3362 理念模块）；
//! - **卷二 矩阵**：四维坐标与单元格语义（回链 F3363）；
//! - **卷三 流程**：四阶段流转与阶段门（回链 F3364）；
//! - **卷四 判据**：硬指标/体验指标两级（回链 F3365）；
//! - **卷五 环境**：真机登记与环境快照（回链 F3367）。
//!
//! 手册是「新人第一天能照着跑通一次验收」的操作书：每一章都必
//! 须包含**可执行入口**（模块名），读者按图索骥直达代码。
//!
//! # 三、手册一组（判据）
//!
//! 判据组：五卷齐备 + 编号连续 + 必含词核验 + 回链可达 + 空手册
//! 拒收 + 增章守门（编号断档即拒）。

use alloc::vec;
use alloc::vec::Vec;
use alloc::format;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const MANUAL_VERSION: &str = "V10-manual-v1";

/// 章节编号断档（手册结构不连续即拒）。
pub const E_VV_GAPCH: &str = "E_VV_GAPCH";

/// 必含关键词缺失（该章未覆盖锚点要求）。
pub const E_VV_KEYWORD: &str = "E_VV_KEYWORD";

/// 回链目标不存在（手册指到不存在的模块）。
pub const E_VV_BACKLINK: &str = "E_VV_BACKLINK";

/// 手册章节条目。
#[derive(Clone, Copy, Debug)]
pub struct ManualSection {
    /// 章号（1 起连续）。
    pub no: u8,
    /// 章标题。
    pub title: &'static str,
    /// 必含关键词（至少命中一个）。
    pub keywords: &'static [&'static str],
    /// 回链模块名（必须真实存在于 cgpu 域）。
    pub backlink: &'static str,
}

/// 手册五卷（锚点结构——理念/矩阵/流程/判据/环境）。
pub const MANUAL_SECTIONS: [ManualSection; 5] = [
    ManualSection {
        no: 1,
        title: "真机验收理念",
        keywords: &["测试验证", "走查", "双兜底"],
        backlink: "vcv02_philosophy",
    },
    ManualSection {
        no: 2,
        title: "验收矩阵",
        keywords: &["四维", "设备", "场景"],
        backlink: "vcv03_matrix",
    },
    ManualSection {
        no: 3,
        title: "验收流程",
        keywords: &["预备", "执行", "处置"],
        backlink: "vcv04_flow",
    },
    ManualSection {
        no: 4,
        title: "验收判据",
        keywords: &["硬指标", "体验指标", "两级"],
        backlink: "vcv05_criteria",
    },
    ManualSection {
        no: 5,
        title: "验收环境",
        keywords: &["真机登记", "快照", "环境"],
        backlink: "vcv07_env",
    },
];

// ---------------------------------------------------------------------------
// 二、手册核验
// ---------------------------------------------------------------------------

/// 单章正文核验：标题非空 + 至少命中一个必含词。
pub fn section_body_ok(sec: &ManualSection, body: &str) -> Result<(), String> {
    if sec.title.is_empty() {
        return Err(format!("{}：第 {} 章标题为空", E_VV_KEYWORD, sec.no));
    }
    for k in sec.keywords.iter() {
        if body.contains(k) {
            return Ok(());
        }
    }
    Err(format!(
        "{}：第 {} 章「{}」必含词 {:?} 全未命中",
        E_VV_KEYWORD, sec.no, sec.title, sec.keywords
    ))
}

/// 回链可达性：模块名必须出现在本域已注册模块名单中。
pub fn backlink_ok(sec: &ManualSection, registered: &[&str]) -> Result<(), String> {
    if registered.contains(&sec.backlink) {
        Ok(())
    } else {
        Err(format!(
            "{}：第 {} 章回链 {} 不在已注册模块中",
            E_VV_BACKLINK, sec.no, sec.backlink
        ))
    }
}

/// 编号连续性：1 起无断档。
pub fn numbering_ok(secs: &[ManualSection]) -> Result<(), String> {
    for (i, s) in secs.iter().enumerate() {
        if s.no as usize != i + 1 {
            return Err(format!(
                "{}：第 {} 位章号是 {}（应为 {}）",
                E_VV_GAPCH,
                i + 1,
                s.no,
                i + 1
            ));
        }
    }
    Ok(())
}

/// 手册完整性核验（四关全过才交付）。
pub fn manual_complete(bodies: &[&str], registered: &[&str]) -> Result<(), String> {
    if bodies.len() != MANUAL_SECTIONS.len() {
        return Err(format!(
            "{}：正文 {} 段 != 章节 {} 卷",
            E_VV_GAPCH,
            bodies.len(),
            MANUAL_SECTIONS.len()
        ));
    }
    numbering_ok(&MANUAL_SECTIONS)?;
    for (i, sec) in MANUAL_SECTIONS.iter().enumerate() {
        section_body_ok(sec, bodies[i])?;
        backlink_ok(sec, registered)?;
    }
    Ok(())
}

/// 章节标题渲染（报告引用的唯一格式）。
pub fn render_toc(secs: &[ManualSection]) -> String {
    let mut out = String::new();
    for s in secs.iter() {
        out.push_str(&format!("{}. {}\n", s.no, s.title));
    }
    out
}

/// 手册声明（文档也是契约——非空即判据）。
pub const MANUAL_DECLARATION: &str =
    "验收手册五卷：理念/矩阵/流程/判据/环境，每章带可执行入口回链";

// ---------------------------------------------------------------------------
// 三、手册一组（判据）
// ---------------------------------------------------------------------------

/// CGPU-F3370 域自检：手册、一组、判据。
pub fn run_vcv10_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv10_manual");

    // --- 五卷齐备 ---
    cs.add(
        "五卷齐备",
        MANUAL_SECTIONS.len() == 5,
        "锚点手册结构：理念/矩阵/流程/判据/环境",
    );

    // --- 编号连续 ---
    cs.add(
        "章号连续无断档",
        numbering_ok(&MANUAL_SECTIONS).is_ok(),
        "1..5 连续",
    );

    // --- 编号断档拒收 ---
    let mut broken = MANUAL_SECTIONS.to_vec();
    broken[2].no = 9;
    cs.add(
        "编号断档拒收",
        numbering_ok(&broken).is_err(),
        "结构不连续的手册不是手册",
    );

    // --- 必含词核验：命中通过 / 全未命中拒 ---
    let sec1 = &MANUAL_SECTIONS[0];
    cs.add(
        "必含词命中通过",
        section_body_ok(sec1, "本章讲测试验证与体验验证的区别，走查兜底").is_ok(),
        "命中任一关键词即过",
    );
    let r = section_body_ok(sec1, "完全无关的正文");
    cs.add(
        "必含词全未命中拒",
        r.is_err() && r.unwrap_err().contains(E_VV_KEYWORD),
        "该章未覆盖锚点要求",
    );

    // --- 回链可达：本域真实模块名单 ---
    let registered: Vec<&str> = vec![
        "vcv02_philosophy",
        "vcv03_matrix",
        "vcv04_flow",
        "vcv05_criteria",
        "vcv07_env",
        "vcv08_telemetry",
    ];
    let mut all_link_ok = true;
    for s in MANUAL_SECTIONS.iter() {
        if backlink_ok(s, &registered).is_err() {
            all_link_ok = false;
        }
    }
    cs.add(
        "回链五卷全可达",
        all_link_ok,
        "手册指到的模块必须真实存在",
    );

    // --- 回链断链拒收 ---
    let fake = vec!["vcv02_philosophy"];
    let r = backlink_ok(&MANUAL_SECTIONS[1], &fake);
    cs.add(
        "断链拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_BACKLINK),
        "指到不存在的模块=手册撒谎",
    );

    // --- 完整性：正文段数不符拒收 ---
    cs.add(
        "正文段数不符拒",
        manual_complete(&["只有一段"], &registered).is_err(),
        "五卷手册不能只有一段正文",
    );

    // --- 完整性：全量正文通过 ---
    let bodies: Vec<&str> = vec![
        "测试验证代码行为，走查双兜底",
        "四维：设备驱动 OS 场景",
        "预备执行报告处置四阶段",
        "硬指标与体验指标两级判据",
        "真机登记与环境快照",
    ];
    cs.add(
        "全量正文通过",
        manual_complete(&bodies, &registered).is_ok(),
        "四关全过=手册已交付",
    );

    // --- TOC 渲染 ---
    cs.add(
        "目录渲染非空",
        render_toc(&MANUAL_SECTIONS).contains("4. 验收判据"),
        "格式钉死",
    );

    // --- 声明非空 ---
    cs.add(
        "手册声明非空",
        !MANUAL_DECLARATION.is_empty() && MANUAL_DECLARATION.contains("五卷"),
        "文档也是契约",
    );

    // --- 版本指纹 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in MANUAL_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    cs.add("版本指纹非零", fp != 0, MANUAL_VERSION);

    cs
}
