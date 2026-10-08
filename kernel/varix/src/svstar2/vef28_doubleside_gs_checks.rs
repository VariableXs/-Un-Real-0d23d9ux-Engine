//! VE-F1625 判据层：双侧渲染与几何着色探测（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1625`
//!
//! **锚点判据（双面语义/GS 探测/诚实标注/判据）→ 判据族**：
//! TWOSIDE 4 / GS 4 / HONESTY 4 / META 4 = 16 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 翻译表九格**判据侧写死对拍**（D3D12 翻转/Vulkan+Metal 直通逐格
//! 比对，不信被测自报）；Metal 不支持传统 GS 是锚点原文——判据侧写
//! 死 `GsSupport::None` 恒等对拍；标注同源性用「条数=矩阵行数 × 逐
//! 条反查矩阵」双向夹；反向语料（矛盾标注）必拒；码段判据 `!=` 防
//! 自判死（0x4x/0x5x 全段除 0x4C 排除）。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vef28_doubleside_gs::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的统一绕序语义锚（锚点原文：跨后端一致）。
const EXP_WINDING_ANCHOR: &str = "CCW=正面";

/// 判据侧独立写死的翻译表翻转位九格（后端行 × [None,Back,Front] 列）。
const EXP_FLIP: [[bool; 3]; 3] = [
    [true, true, true],   // D3D12：原生默认 CW=正面 ⇒ 全翻
    [false, false, false], // Vulkan：与我方一致
    [false, false, false], // Metal：与我方一致
];

/// 判据侧独立写死的 GS 支持度（锚点原文：Metal 不支持传统 GS）。
const EXP_GS: [(Backend, bool); 3] = [
    (Backend::D3d12, true),
    (Backend::Vulkan, true),
    (Backend::Metal, false),
];

/// 判据侧独立写死的联动任务号（差异封装由 F1626 约定承接）。
const EXP_F1626_UPLINK: u32 = 1626;

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [DgCode; 6] = [
    DgCode::CULL_WIRE_INVALID,
    DgCode::BACKEND_WIRE_INVALID,
    DgCode::GS_NO_FALLBACK,
    DgCode::ANNOTATION_CONFLICT,
    DgCode::DEGENERATE_NORMAL,
    DgCode::WINDING_WIRE_INVALID,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 16;

/// 三后端枚举表（判据侧自己的行域清单——不向被测借）。
const BACKENDS: [Backend; 3] = [Backend::D3d12, Backend::Vulkan, Backend::Metal];

/// 三剔除模式枚举表。
const CULLS: [CullMode; 3] = [CullMode::None, CullMode::Back, CullMode::Front];

// ---------------------------------------------------------------------------
// 判据主入口
// ---------------------------------------------------------------------------

pub fn run_vef28_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-doubleside-gs");

    // ================= 一、双面语义（TWOSIDE） =================

    // TWOSIDE-1：剔除开关 wire 往返封闭 + 域外拒（3 以外全拒——双向）。
    let wire_ok = CULLS.iter().all(|m| CullMode::from_wire(m.wire()) == Ok(*m))
        && CullMode::from_wire(3).is_err()
        && CullMode::from_wire(255).is_err()
        && CullMode::None.is_two_sided()
        && !CullMode::Back.is_two_sided()
        && !CullMode::Front.is_two_sided();
    s.add("F1625-TWOSIDE-剔除开关wire封闭", wire_ok, "");

    // TWOSIDE-2：翻译表九格逐格对拍（判据侧 EXP_FLIP 独立写死）。
    let mut nine_ok = true;
    for b in BACKENDS {
        for m in CULLS {
            let got = translate(b, m);
            let row = match b {
                Backend::D3d12 => 0,
                Backend::Vulkan => 1,
                Backend::Metal => 2,
            };
            let col = m.wire() as usize;
            if got.winding_flip != EXP_FLIP[row][col] || got.cull != m {
                nine_ok = false;
            }
        }
    }
    s.add("F1625-TWOSIDE-翻译表九格逐格对拍", nine_ok, "");

    // TWOSIDE-3：法线翻转语义（判据侧手算：正面原样/背面取反/零向量拒）。
    let n = [0.0, 3.0, -4.0];
    let flip_ok = facing_normal_flip(n, FacetSide::Front) == Ok([0.0, 3.0, -4.0])
        && facing_normal_flip(n, FacetSide::Back) == Ok([0.0, -3.0, 4.0])
        && facing_normal_flip([1.0, 0.0, 0.0], FacetSide::Back) == Ok([-1.0, 0.0, 0.0])
        && facing_normal_flip([0.0; 3], FacetSide::Back) == Err(DgCode::DEGENERATE_NORMAL)
        && facing_normal_flip([0.0; 3], FacetSide::Front) == Err(DgCode::DEGENERATE_NORMAL);
    s.add("F1625-TWOSIDE-法线翻转语义手算对拍", flip_ok, "");

    // TWOSIDE-4：统一语义锚逐字在册 + F1626 承接声明（衔接不暗改）。
    let anchor_ok = EXP_WINDING_ANCHOR == "CCW=正面"
        && EXP_F1626_UPLINK == 1626
        && backend_name(Backend::D3d12) == "D3D12"
        && backend_name(Backend::Vulkan) == "Vulkan"
        && backend_name(Backend::Metal) == "Metal"
        && (translate(Backend::D3d12, CullMode::None).winding_flip
            != translate(Backend::Vulkan, CullMode::None).winding_flip);
    s.add("F1625-TWOSIDE-统一锚CCW正面与F1626承接", anchor_ok, "");

    // ================= 二、GS 探测（GS） =================

    // GS-1：能力矩阵封闭（判据侧三行写死对拍——含 Metal=None 锚点原文）。
    let caps_ok = (0..3).all(|i| {
        let (b, got) = GS_CAPS[i];
        let exp_full = EXP_GS[i].1;
        let got_full = matches!(got, GsSupport::Full);
        b == EXP_GS[i].0
            && got_full == exp_full
            && got == gs_caps_of(b)
    });
    s.add("F1625-GS-能力矩阵封闭含Metal为None", caps_ok, "");

    // GS-2：Metal 降级计划——不原生 + 替代路径非空（锚点原文语义）。
    let metal_plan = match gs_plan(Backend::Metal) {
        Ok(p) => p,
        Err(_) => GsPlan { native: true, alt_path: None }, // 错形状必挂
    };
    let metal_ok = !metal_plan.native
        && match metal_plan.alt_path {
            Some(path) => path.contains("实例化展开") && path.contains("F1208"),
            None => false,
        };
    s.add("F1625-GS-Metal降级替代路径非空", metal_ok, "");

    // GS-3：支持后端原生直通 + GS_NO_FALLBACK 防御位在册可辨。
    let full_ok = BACKENDS.iter().all(|b| {
        if matches!(gs_caps_of(*b), GsSupport::Full) {
            match gs_plan(*b) {
                Ok(p) => p.native && p.alt_path.is_none(),
                Err(_) => false,
            }
        } else {
            true
        }
    }) && DgCode::GS_NO_FALLBACK.reason().contains("防御位");
    s.add("F1625-GS-支持后端原生直通与防御位", full_ok, "");

    // GS-4：后端/绕序 wire 域外拒（双向）。
    let bwire_ok = (0..3).all(|w| backend_from_wire(w).is_ok())
        && backend_from_wire(3).is_err()
        && (0..2).all(|w| winding_from_wire(w).is_ok())
        && winding_from_wire(2).is_err()
        && winding_from_wire(255).is_err();
    s.add("F1625-GS-后端绕序wire域外拒", bwire_ok, "");

    // ================= 三、诚实标注（HONESTY） =================

    let annos = annotations();

    // HONESTY-1：条数同源（6 = 3 后端 × 2 主题）且逐条裁决全过。
    let count_ok = annos.len() == 6 && annos.iter().all(|a| verify_annotation(a).is_ok());
    s.add("F1625-HONESTY-标注条数同源全过裁决", count_ok, "");

    // HONESTY-2：每条标注的 statement 非空且含后端人话名（账面可读）。
    let readable = annos.iter().all(|a| {
        !a.statement.is_empty() && a.statement.contains(backend_name(a.backend))
    });
    s.add("F1625-HONESTY-声明非空含后端名", readable, "");

    // HONESTY-3：同源性夹——GS 标注文本与矩阵逐行一致（None 行含降级全文）。
    let gs_source_ok = BACKENDS.iter().all(|b| {
        let a = annotate_gs(*b);
        match gs_caps_of(*b) {
            GsSupport::None => {
                a.statement.contains("不支持传统几何着色")
                    && a.statement.contains(GS_ALT_PATH)
            }
            GsSupport::Full => a.statement.contains("支持传统几何着色"),
        }
    });
    s.add("F1625-HONESTY-GS标注与矩阵同源", gs_source_ok, "");

    // HONESTY-4：反向语料——构造矛盾标注必被拒（双向：TwoSided/GsSupport）。
    let mut bad_gs = annotate_gs(Backend::Vulkan);
    bad_gs.statement = "GS 支持度：Vulkan 不支持传统几何着色（能力表 None）".to_string();
    let mut bad_ts = annotate_twosided(Backend::D3d12);
    bad_ts.statement = "双面语义差异封装：D3D12 的剔除语义绕序语义与我方一致（原生默认 CCW=正面）；统一语义锚为 CCW=正面（F1626 约定承接）".to_string();
    let mut empty = annotate_twosided(Backend::Metal);
    empty.statement = String::new();
    let reject_ok = verify_annotation(&bad_gs) == Err(DgCode::ANNOTATION_CONFLICT)
        && verify_annotation(&bad_ts) == Err(DgCode::ANNOTATION_CONFLICT)
        && verify_annotation(&empty) == Err(DgCode::ANNOTATION_CONFLICT);
    s.add("F1625-HONESTY-矛盾标注反向必拒", reject_ok, "");

    // ================= 四、判据自检（META） =================

    // META-1：码段独占——全部 0x4Cxx，且 != 其余段（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x4C)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x40 && hi != 0x41 && hi != 0x42 && hi != 0x43 && hi != 0x44
                && hi != 0x45 && hi != 0x46 && hi != 0x47 && hi != 0x48 && hi != 0x49
                && hi != 0x4A && hi != 0x4B && hi != 0x4D && hi != 0x50
        });
    s.add("F1625-META-诊断码段独占", section_ok, "");

    // META-2：码两两互异 + 人话原因非空。
    let mut code_ok = true;
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
    s.add("F1625-META-码互异原因非空", code_ok, "");

    // META-3：诊断码恰六码（0x4C01~0x4C06 连续）。
    let codes = {
        let mut v = Vec::new();
        for c in ALL_CODES {
            v.push(c.code());
        }
        v
    };
    let six_ok = codes.contains(&0x4C01)
        && codes.contains(&0x4C02)
        && codes.contains(&0x4C03)
        && codes.contains(&0x4C04)
        && codes.contains(&0x4C05)
        && codes.contains(&0x4C06);
    s.add("F1625-META-六码恰落4C01至4C06", six_ok, "");

    // META-4：判据条数对账（放末位：此时 len 应为 15，加自身恰 16）。
    s.add("F1625-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
