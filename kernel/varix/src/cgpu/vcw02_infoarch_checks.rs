//! CGPU-F3522 判据层：文档信息架构（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3522`
//!
//! **锚点判据（四分区/导航表/一组/判据）→ 判据族**：
//! ARCH-SECTION 2 / ARCH-TREE 3 / ARCH-TABLE 3 / ARCH-LINK 2 / CODE 2
//! / META 1 = 13 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 四分区名**判据侧写死对拍**；展开行数判据侧独立重算（4 根 + 4 子 =
//! 8）且首/中/末行路径深度逐行写死对拍（抓 DFS 顺序反转）；树律反向
//! 语料（孤儿/空标题/路径重复/空分区）逐条专属码分账；承接判据**跨
//! 单元引用 vcw01 HANDOVER_ITEMS**（站点结构图第七件在册——承接不是
//! 口头声明）；码段判据 `!=` 防自判死。

use crate::checks::CheckSet;

use super::vcw02_infoarch::*;
use crate::cgpu::vcv01_realverify::V_DOMAIN_TOTAL;
use crate::cgpu::vcw01_sdkdoc::HANDOVER_ITEMS as V10_ITEMS;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的四分区名。
const EXP_SECTIONS: [&str; 4] = ["参考", "教程", "指南", "FAQ"];

/// 判据侧独立写死的展开行数（4 根 + 5 子：参考区 2 子）。
const EXP_ROW_COUNT: usize = 9;

/// 判据侧独立写死的首/中/末行（路径 + 深度——抓 DFS 顺序反转）。
const EXP_FIRST: (&str, usize) = ("reference", 0);
const EXP_MID: (&str, usize) = ("tutorial/first-frame", 1);
const EXP_LAST: (&str, usize) = ("faq/diagcodes", 1);

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [WiCode; 6] = [
    WiCode::SECTION_INVALID,
    WiCode::PATH_EMPTY,
    WiCode::PATH_ORPHAN,
    WiCode::PATH_DUP,
    WiCode::TITLE_EMPTY,
    WiCode::SECTION_EMPTY,
];

/// vcw01 的六码（判据侧点名——同段不重叠对拍）。
const VCW01_CODES: [u16; 6] = [0x5C01, 0x5C02, 0x5C03, 0x5C04, 0x5C05, 0x5C06];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 13;

// ---------------------------------------------------------------------------
// 判据主入口
// ---------------------------------------------------------------------------

pub fn run_vcw02_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-infoarch");

    // ================= 一、四分区（ARCH-SECTION） =================

    // SECTION-1：四分区闭集名逐字对拍（判据侧写死）。
    let sec_ok = Section::ALL.len() == 4
        && Section::ALL[0].name() == EXP_SECTIONS[0]
        && Section::ALL[1].name() == EXP_SECTIONS[1]
        && Section::ALL[2].name() == EXP_SECTIONS[2]
        && Section::ALL[3].name() == EXP_SECTIONS[3];
    s.add("W2-SECTION-四分区闭集名对拍", sec_ok, "");

    // SECTION-2：在账树四区根覆盖 + 表外防御位在册。
    let t = nav_tree();
    let cover_ok = sections_covered(&t) == Ok(())
        && WiCode::SECTION_INVALID.reason().contains("四分区闭集");
    s.add("W2-SECTION-四区根覆盖与防御位", cover_ok, "");

    // ================= 二、导航树（ARCH-TREE） =================

    // TREE-1：在账树五律全过（结构/前缀/唯一/非空标题/覆盖）。
    let t_ok = validate_tree(&t) == Ok(()) && paths_unique(&flatten(&t));
    s.add("W2-TREE-在账树五律全过", t_ok, "");

    // TREE-2：反向语料——孤儿路径拒（前缀律精确到分隔符）。
    let mut orphan = nav_tree();
    orphan[0].children[0].path = "tutorial/wrong";
    let t2 = nav_tree();
    // 前缀律正向对照：合法子路径 reference/render 必须以 reference/ 开头
    let prefix_ok = orphan[0].children[0].path.starts_with("reference") == false
        && t2[0].children[0].path.starts_with("reference/");
    let orphan_ok = validate_tree(&orphan) == Err(WiCode::PATH_ORPHAN) && prefix_ok;
    s.add("W2-TREE-孤儿路径反向必拒", orphan_ok, "");

    // TREE-3：反向语料——空标题/空路径逐条专属码分账。
    let mut empty_title = nav_tree();
    empty_title[1].title = "";
    let mut empty_path = nav_tree();
    empty_path[2].path = "";
    let inv_ok = validate_tree(&empty_title) == Err(WiCode::TITLE_EMPTY)
        && validate_tree(&empty_path) == Err(WiCode::PATH_EMPTY);
    s.add("W2-TREE-空标题空路径专属码分账", inv_ok, "");

    // ================= 三、导航表（ARCH-TABLE） =================

    // TABLE-1：行数同源（判据侧独立重算 8 = 4 根 + 4 子）。
    let t = nav_tree();
    let rows = flatten(&t);
    let mut root_count = 0usize;
    for n in t.iter() {
        root_count += 1 + n.children.len();
    }
    let count_ok = rows.len() == EXP_ROW_COUNT && root_count == EXP_ROW_COUNT;
    s.add("W2-TABLE-行数与节点数同源", count_ok, "");

    // TABLE-2：DFS 前序逐行对拍（首/中/末行路径+深度判据侧写死）。
    let order_ok = rows[0].path == EXP_FIRST.0
        && rows[0].depth == EXP_FIRST.1
        && rows[0].section == "参考"
        && rows[4].path == EXP_MID.0
        && rows[4].depth == EXP_MID.1
        && rows[4].section == "教程"
        && rows[8].path == EXP_LAST.0
        && rows[8].depth == EXP_LAST.1
        && rows[8].section == "FAQ";
    s.add("W2-TABLE-DFS前序首中末行对拍", order_ok, "");

    // TABLE-3：展开确定性（两次展开逐行相同）。
    let det_ok = flatten_deterministic(&t) && flatten(&t) == rows;
    s.add("W2-TABLE-两次展开逐行相同", det_ok, "");

    // ================= 四、承接（ARCH-LINK） =================

    // LINK-1：F3521 承接——站点结构图在 V10 移交包七件在册（跨单元）。
    let link_ok = F3521_LINK.0 == &3521u32
        && F3521_LINK.1 == "站点结构图"
        && V10_ITEMS.iter().any(|it| *it == "站点结构图");
    s.add("W2-LINK-站点结构图跨单元在册", link_ok, "");

    // LINK-2：域守恒与 vcw01 同域对账（同一 160）。
    let domain_ok = W_DOMAIN_TOTAL == 160 && V_DOMAIN_TOTAL == W_DOMAIN_TOTAL;
    s.add("W2-LINK-域守恒跨单元对账", domain_ok, "");

    // ================= 五、码段（CODE） =================

    // CODE-1：续段恰 0x5C07~0x5C0C 连续，且与 vcw01 六码不重叠。
    let mut section_ok = ALL_CODES.len() == 6;
    for i in 0..6 {
        let c = ALL_CODES[i].code();
        if c != 0x5C07 + i as u16 {
            section_ok = false;
        }
        for v in VCW01_CODES.iter() {
            if c == *v {
                section_ok = false;
            }
        }
    }
    s.add("W2-CODE-续段连续与vcw01不重叠", section_ok, "");

    // CODE-2：码互异 + 原因非空 + 段高字节仍 0x5C（防自判死）。
    let mut code_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x5C);
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("W2-CODE-码互异原因非空", code_ok, "");

    // ================= 六、判据自检（META） =================

    // META-1：判据条数对账（放末位：此时 len 应为 12，加自身恰 13）。
    s.add("W2-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
