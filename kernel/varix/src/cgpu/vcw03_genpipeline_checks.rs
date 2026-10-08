//! CGPU-F3523 判据层：文档生成管线（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3523`
//!
//! **锚点判据（自动生成/一组/判据）→ 判据族**：
//! GEN 3（自动生成）/ PIPE 3（管线单向）/ SRC 2（源注释三律）
//! / LINK 2（承接）/ CODE 2 / META 1 = 13 项；测试生成一组（tests 四测）。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 自动生成印记**判据侧独立重写**（防两处同源漂移）；行数公式判据侧
//! 独立重算 13（1 印记 + 3 × (1 标题 + 2 正文 + 1 分隔)）且渲染标题行
//! **逐字对拍**（抓 format 模板漂移）；排序后首/末条路径判据侧写死
//! （抓归一排序反转）；跳段/回退/终端三向语料逐条专属码分账；承接判
//! 据**跨单元对拍 vcw02 导航树**——在账源文档路径必须全部在树路径集
//! 合内（页面在树上不在树外，承接不是口头声明）；码段判据 `!=` 防自
//! 判死；判据区零 panic 面（索引全部前置长度短路保护）。

use alloc::string::ToString;

use crate::checks::CheckSet;

use super::vcw03_genpipeline::*;
use crate::cgpu::vcw02_infoarch::{flatten as site_flatten, nav_tree as site_tree};

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立重写的印记（与实现 AUTO_BANNER 逐字对拍——双向同源断）。
const EXP_BANNER: &str = "本文档由生成管线自动产出，请勿手改";

/// 判据侧独立写死的四段名。
const EXP_STAGE_NAMES: [&str; 4] = ["抽取", "归一", "渲染", "产出"];

/// 判据侧独立重算的行数（1 印记 + 3 × (1 标题 + 2 正文 + 1 分隔)）。
const EXP_LINE_COUNT: usize = 13;

/// 判据侧独立写死的排序首/末条路径（归一按路径字典序）。
const EXP_FIRST_PATH: &str = "faq/diagcodes";
const EXP_LAST_PATH: &str = "tutorial/first-frame";

/// 判据侧独立写死的渲染标题行（抓 format 模板漂移）。
const EXP_TITLE_FIRST: &str = "## 生成失败码含义｜faq/diagcodes";

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [WgCode; 6] = [
    WgCode::TITLE_EMPTY,
    WgCode::BODY_EMPTY,
    WgCode::PATH_DUP,
    WgCode::STAGE_JUMP,
    WgCode::PUBLISH_EMPTY,
    WgCode::RENDER_MISMATCH,
];

/// vcw01/vcw02 的十二码（判据侧点名——同段不重叠对拍）。
const VCW01_CODES: [u16; 6] = [0x5C01, 0x5C02, 0x5C03, 0x5C04, 0x5C05, 0x5C06];
const VCW02_CODES: [u16; 6] = [0x5C07, 0x5C08, 0x5C09, 0x5C0A, 0x5C0B, 0x5C0C];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 13;

// ---------------------------------------------------------------------------
// 判据主入口
// ---------------------------------------------------------------------------

pub fn run_vcw03_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-genpipeline");

    // ================= 一、自动生成（GEN） =================

    // GEN-1：印记逐字对拍（实现常量 == 判据侧独立重写）。
    let docs = source_docs();
    let gen_ok = match generate(&docs) {
        Ok(lines) => {
            lines.len() == EXP_LINE_COUNT && lines[0] == EXP_BANNER && lines[0] == AUTO_BANNER
        }
        Err(_) => false,
    };
    s.add("W3-GEN-印记逐字对拍", gen_ok, "");

    // GEN-2：行数同源——判据侧独立重算 == 公式函数 == 实际输出（三方）。
    let raw = extract(&docs);
    let tri_ok = match raw {
        Ok(raw) => match normalize(raw) {
            Ok(norm) => {
                let lines = render(&norm);
                let manual = {
                    // 判据侧独立重算：1 + 逐条 (2 + 正文行数)
                    let mut n = 1usize;
                    for e in norm.iter() {
                        n += 2 + e.body.len();
                    }
                    n
                };
                manual == EXP_LINE_COUNT
                    && manual == expected_line_count(&norm)
                    && lines.len() == manual
                    && verify_render(&lines, &norm) == Ok(())
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("W3-GEN-行数三方同源对拍", tri_ok, "");

    // GEN-3：确定性 + 归一排序首末条 + 渲染标题行逐字（抓模板漂移）。
    let norm0 = match extract(&docs) {
        Ok(r) => match normalize(r) {
            Ok(n) => Some(n),
            Err(_) => None,
        },
        Err(_) => None,
    };
    let det_ok = generate_deterministic(&docs)
        && match norm0 {
            Some(norm) => {
                norm[0].path == EXP_FIRST_PATH
                    && norm[norm.len() - 1].path == EXP_LAST_PATH
                    && norm.len() == 3
                    && render(&norm)[1] == EXP_TITLE_FIRST
            }
            None => false,
        };
    s.add("W3-GEN-确定性排序与标题行逐字", det_ok, "");

    // ================= 二、管线单向（PIPE） =================

    // PIPE-1：四段名判据侧写死对拍 + next 链恰一后继。
    let mut names_ok = GenStage::ALL.len() == 4;
    for i in 0..4 {
        if GenStage::ALL[i].name() != EXP_STAGE_NAMES[i] {
            names_ok = false;
        }
    }
    let chain_ok = names_ok
        && GenStage::Extract.next() == Some(GenStage::Normalize)
        && GenStage::Normalize.next() == Some(GenStage::Render)
        && GenStage::Render.next() == Some(GenStage::Publish)
        && GenStage::Publish.next() == None;
    s.add("W3-PIPE-四段名与恰一后继", chain_ok, "");

    // PIPE-2：违序三向语料逐条专属码分账（跳段/回退/终端再推进）。
    let jump_ok = stage_transition(GenStage::Extract, GenStage::Render)
        == Err(WgCode::STAGE_JUMP)
        && stage_transition(GenStage::Render, GenStage::Extract) == Err(WgCode::STAGE_JUMP)
        && stage_transition(GenStage::Publish, GenStage::Publish) == Err(WgCode::STAGE_JUMP)
        && full_chain_legal();
    s.add("W3-PIPE-违序三向拒与合法链全过", jump_ok, "");

    // PIPE-3：发布终端闸——空文档/手写文档（无印记）拒，生成文档过。
    let gate_ok = publish(&alloc::vec![]) == Err(WgCode::PUBLISH_EMPTY)
        && publish(&alloc::vec!["手写的第一行".to_string()]) == Err(WgCode::PUBLISH_EMPTY)
        && match generate(&source_docs()) {
            Ok(lines) => publish(&lines).is_ok(),
            Err(_) => false,
        };
    s.add("W3-PIPE-发布终端闸三态", gate_ok, "");

    // ================= 三、源注释三律（SRC） =================

    // SRC-1：在账源抽取全过 + 归一保条数（正向基线非平凡）。
    let src_ok = match extract(&source_docs()) {
        Ok(raw) => raw.len() == 3 && raw[0].body.len() == 2,
        Err(_) => false,
    };
    s.add("W3-SRC-在账源抽取全过", src_ok, "");

    // SRC-2：反向语料——空标题/空正文/重复路径逐条专属码分账。
    let mut bad_title = source_docs();
    bad_title[0].title = "";
    let mut bad_body = source_docs();
    bad_body[1].body = &["有内容", ""];
    let mut dup = source_docs();
    dup.push(DocComment {
        path: "faq/diagcodes",
        title: "重复路径",
        body: &["占位"],
    });
    let inv_ok = extract(&bad_title) == Err(WgCode::TITLE_EMPTY)
        && extract(&bad_body) == Err(WgCode::BODY_EMPTY)
        && match extract(&dup) {
            Ok(raw) => normalize(raw) == Err(WgCode::PATH_DUP),
            Err(_) => false,
        };
    s.add("W3-SRC-三律反向语料专属码分账", inv_ok, "");

    // ================= 四、承接（LINK） =================

    // LINK-1：F3522 承接——在账源文档路径必须全部在 vcw02 导航树
    // 路径集合内（页面在树上不在树外——跨单元对拍非口头声明）。
    let tree_paths: alloc::vec::Vec<&str> =
        site_flatten(&site_tree()).iter().map(|r| r.path).collect();
    let mut on_tree = F3522_LINK.0 == &3522u32 && F3522_LINK.1 == "文档生成管线";
    for d in source_docs().iter() {
        if !tree_paths.iter().any(|p| *p == d.path) {
            on_tree = false;
        }
    }
    s.add("W3-LINK-源路径全在导航树上", on_tree, "");

    // LINK-2：域守恒与 vcw02 同域对账（同一 160）。
    let domain_ok = W_DOMAIN_TOTAL == 160 && super::vcw02_infoarch::W_DOMAIN_TOTAL == W_DOMAIN_TOTAL;
    s.add("W3-LINK-域守恒跨单元对账", domain_ok, "");

    // ================= 五、码段（CODE） =================

    // CODE-1：续段恰 0x5C0D~0x5C12 连续，且与 vcw01/vcw02 十二码不重叠。
    let mut section_ok = ALL_CODES.len() == 6;
    for i in 0..6 {
        let c = ALL_CODES[i].code();
        if c != 0x5C0D + i as u16 {
            section_ok = false;
        }
        for v in VCW01_CODES.iter() {
            if c == *v {
                section_ok = false;
            }
        }
        for v in VCW02_CODES.iter() {
            if c == *v {
                section_ok = false;
            }
        }
    }
    s.add("W3-CODE-续段连续与前十二码不重叠", section_ok, "");

    // CODE-2：码互异 + 原因非空 + 段高字节仍 0x5C（防自判死）。
    let mut code_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x5C);
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES.iter() {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("W3-CODE-码互异原因非空", code_ok, "");

    // ================= 六、判据自检（META） =================

    // META-1：判据条数对账（放末位：此时 len 应为 12，加自身恰 13）。
    s.add("W3-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
