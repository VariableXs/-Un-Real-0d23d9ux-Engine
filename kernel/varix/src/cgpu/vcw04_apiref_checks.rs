//! CGPU-F3524 判据层：API 参考自动生成（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3524`
//!
//! **锚点判据（双步/两组/判据）→ 判据族**：
//! STEP 2（双步）/ EXTRACT 3（注释抽取）/ RENDER 3（页面渲染）
//! / LINK 2（承接）/ CODE 2 / META 1 = 13 项；测试抽取/渲染两组
//! （tests 四测两组）。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 双步名**判据侧写死对拍**；首条目逐字段（模块/函数名/参数数/返回）
//! 判据侧独立写死对拍；行数公式判据侧独立重算 14（1 印记 + 5 + 4 + 4）
//! 三方对拍；渲染标题行逐字（抓 format 模板漂移）；印记**跨单元引用
//! vcw03 同一常量**（不重写——同源）；冻结条款兑现**跨单元对拍
//! vcw01 POSITION_CLAUSES**（「冻结签名才进参考文档」在册——兑现不
//! 是口头声明）；码段判据 `!=` 防自判死；判据区零 panic 面。

use crate::checks::CheckSet;

use super::vcw04_apiref::*;
use crate::cgpu::vcw01_sdkdoc::POSITION_CLAUSES;
use crate::cgpu::vcw03_genpipeline::AUTO_BANNER as PIPE_BANNER;
use crate::cgpu::vcw03_genpipeline::F3522_LINK as PREV_LINK;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的双步名。
const EXP_STEP_NAMES: [&str; 2] = ["抽取", "渲染"];

/// 判据侧独立写死的首条目字段（在账冻结签名集首条）。
const EXP_FIRST: (&str, &str, usize, &str) = ("render", "render_frame", 2, "bool");

/// 判据侧独立重算的行数（1 印记 + (3+2) + (3+1) + (3+1)）。
const EXP_LINE_COUNT: usize = 14;

/// 判据侧独立写死的渲染标题行（抓 format 模板漂移）。
const EXP_TITLE_FIRST: &str = "### render::render_frame";

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [WqCode; 6] = [
    WqCode::NOT_FROZEN,
    WqCode::NAME_EMPTY,
    WqCode::PARAM_BAD,
    WqCode::RET_EMPTY,
    WqCode::STEP_JUMP,
    WqCode::RENDER_MISMATCH,
];

/// 前三单的十八码（判据侧点名——同段不重叠对拍）。
const VCW01_CODES: [u16; 6] = [0x5C01, 0x5C02, 0x5C03, 0x5C04, 0x5C05, 0x5C06];
const VCW02_CODES: [u16; 6] = [0x5C07, 0x5C08, 0x5C09, 0x5C0A, 0x5C0B, 0x5C0C];
const VCW03_CODES: [u16; 6] = [0x5C0D, 0x5C0E, 0x5C0F, 0x5C10, 0x5C11, 0x5C12];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 13;

// ---------------------------------------------------------------------------
// 判据主入口
// ---------------------------------------------------------------------------

pub fn run_vcw04_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-apiref");

    // ================= 一、双步（STEP） =================

    // STEP-1：双步名与序判据侧写死对拍 + next 链恰一后继。
    let step_ok = RefStep::ALL.len() == 2
        && RefStep::ALL[0].name() == EXP_STEP_NAMES[0]
        && RefStep::ALL[1].name() == EXP_STEP_NAMES[1]
        && RefStep::ExtractSig.next() == Some(RefStep::RenderPage)
        && RefStep::RenderPage.next() == None;
    s.add("W4-STEP-双步名与恰一后继", step_ok, "");

    // STEP-2：违序双向拒（回退/终端再推进）+ 合法单向过。
    let jump_ok = two_step_transition(RefStep::RenderPage, RefStep::ExtractSig)
        == Err(WqCode::STEP_JUMP)
        && two_step_transition(RefStep::RenderPage, RefStep::RenderPage)
            == Err(WqCode::STEP_JUMP)
        && two_step_transition(RefStep::ExtractSig, RefStep::RenderPage).is_ok();
    s.add("W4-STEP-违序双向拒与合法过", jump_ok, "");

    // ================= 二、注释抽取（EXTRACT） =================

    // EXTRACT-1：在账冻结签名全过 + 首条目逐字段判据侧写死对拍。
    let frozen = frozen_signatures();
    let ext_ok = match extract_all(&frozen) {
        Ok(entries) => {
            entries.len() == 3
                && entries[0].module == EXP_FIRST.0
                && entries[0].name == EXP_FIRST.1
                && entries[0].params.len() == EXP_FIRST.2
                && entries[0].ret == EXP_FIRST.3
        }
        Err(_) => false,
    };
    s.add("W4-EXTRACT-冻结签名全过字段对拍", ext_ok, "");

    // EXTRACT-2：未冻结签名显性码拒（冻结闸的机检兑现——批量整体拒）。
    let mut mixed = frozen_signatures();
    mixed.extend(unfrozen_signatures());
    let gate_ok = extract_signature(&unfrozen_signatures()[0]) == Err(WqCode::NOT_FROZEN)
        && extract_all(&mixed) == Err(WqCode::NOT_FROZEN);
    s.add("W4-EXTRACT-未冻结闸单条与批量拒", gate_ok, "");

    // EXTRACT-3：反向语料——空名/参数违例/空返回逐条专属码分账。
    let mut empty_name = frozen_signatures();
    empty_name[0].name = "";
    let mut dup_param = frozen_signatures();
    dup_param[0].params = &[("frame", "&Frame"), ("frame", "&CmdList")];
    let mut empty_ret = frozen_signatures();
    empty_ret[2].ret = "";
    let inv_ok = extract_signature(&empty_name[0]) == Err(WqCode::NAME_EMPTY)
        && extract_signature(&dup_param[0]) == Err(WqCode::PARAM_BAD)
        && extract_signature(&empty_ret[2]) == Err(WqCode::RET_EMPTY);
    s.add("W4-EXTRACT-反向语料专属码分账", inv_ok, "");

    // ================= 三、页面渲染（RENDER） =================

    // RENDER-1：行数同源——判据侧独立重算 == 公式函数 == 实际输出（三方）。
    let frozen2 = frozen_signatures();
    let tri_ok = match extract_all(&frozen2) {
        Ok(entries) => {
            let lines = render_pages(&entries);
            // 判据侧独立重算：1 + 逐条 (3 + 参数行数)
            let mut manual = 1usize;
            for e in entries.iter() {
                manual += 3 + e.params.len();
            }
            manual == EXP_LINE_COUNT
                && manual == expected_line_count(&entries)
                && lines.len() == manual
                && verify_pages(&lines, &entries) == Ok(())
        }
        Err(_) => false,
    };
    s.add("W4-RENDER-行数三方同源对拍", tri_ok, "");

    // RENDER-2：确定性 + 渲染标题行逐字（抓 format 模板漂移）。
    let frozen3 = frozen_signatures();
    let det_ok = match extract_all(&frozen3) {
        Ok(entries) => {
            let lines = render_pages(&entries);
            render_deterministic(&entries) && lines[1] == EXP_TITLE_FIRST
        }
        Err(_) => false,
    };
    s.add("W4-RENDER-确定性与标题行逐字", det_ok, "");

    // RENDER-3：印记跨单元同源——实现常量 == vcw03 同一常量（不重写）。
    let banner_ok = match extract_all(&frozen_signatures()) {
        Ok(entries) => {
            let lines = render_pages(&entries);
            !lines.is_empty() && lines[0] == PIPE_BANNER && PIPE_BANNER == AUTO_BANNER
        }
        Err(_) => false,
    };
    s.add("W4-RENDER-印记与F3523管线同源", banner_ok, "");

    // ================= 四、承接（LINK） =================

    // LINK-1：vcw01 冻结条款兑现——NOT_FROZEN 原因与条款「冻结签名才
    // 进参考文档」对拍 + F3523 承接在账（跨单元非口头声明）。
    let clause = POSITION_CLAUSES[0];
    let link_ok = clause.contains("冻结签名才进参考文档")
        && WqCode::NOT_FROZEN.reason().contains("冻结签名才进参考文档")
        && F3523_LINK.0 == &3523u32
        && F3523_LINK.1 == "文档生成管线"
        && PREV_LINK.0 == &3522u32;
    s.add("W4-LINK-冻结条款兑现与承接在账", link_ok, "");

    // LINK-2：域守恒与前三单同域对账（同一 160）。
    let domain_ok = W_DOMAIN_TOTAL == 160
        && super::vcw02_infoarch::W_DOMAIN_TOTAL == W_DOMAIN_TOTAL
        && super::vcw03_genpipeline::W_DOMAIN_TOTAL == W_DOMAIN_TOTAL;
    s.add("W4-LINK-域守恒跨单元对账", domain_ok, "");

    // ================= 五、码段（CODE） =================

    // CODE-1：续段恰 0x5C13~0x5C18 连续，且与前三单十八码不重叠。
    let mut section_ok = ALL_CODES.len() == 6;
    for i in 0..6 {
        let c = ALL_CODES[i].code();
        if c != 0x5C13 + i as u16 {
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
        for v in VCW03_CODES.iter() {
            if c == *v {
                section_ok = false;
            }
        }
    }
    s.add("W4-CODE-续段连续与前十八码不重叠", section_ok, "");

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
    s.add("W4-CODE-码互异原因非空", code_ok, "");

    // ================= 六、判据自检（META） =================

    // META-1：判据条数对账（放末位：此时 len 应为 12，加自身恰 13）。
    s.add("W4-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
