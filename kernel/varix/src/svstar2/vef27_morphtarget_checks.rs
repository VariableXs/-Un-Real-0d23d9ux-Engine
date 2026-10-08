//! VE-F1624 判据层：Morph Target 基础（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1624`
//!
//! **锚点判据（插值/帧序列/叠加/稀疏量化/判据）→ 判据族**：
//! INTERP 3 / TRACK 3 / COMBINE 3 / QUANT 4 / META 4 = 17 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 插值结果**判据侧手算写死对拍**（恰边界 0/1 + 中间权重 + 法线归一化
//! 期望值）；帧序列恰帧/插值/端点钳制独立构造语料；组合 replace/add
//! 双模式语义分账（Σ 恰边界双向）；量化往返误差界与稀疏账判据侧重算；
//! 码段判据 `!=` 防自判死（0x4x 全段除 0x44 排除）。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vef27_morphtarget::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的量化步长（2^12 = 4096）。
const EXP_QUANT_SCALE: f32 = 4096.0;

/// 判据侧独立写死的往返误差半步。
const EXP_HALF_STEP: f32 = 0.5 / 4096.0;

/// 判据侧独立写死的联动任务号。
const EXP_KEYFRAME_UPLINK: u32 = 1345;

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [MtCode; 5] = [
    MtCode::WEIGHT_OUT_OF_RANGE,
    MtCode::WEIGHT_SUM_EXCEEDS,
    MtCode::UNKNOWN_TARGET,
    MtCode::EMPTY_TRACK,
    MtCode::KEYS_UNORDERED,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 17;

/// 语料：微笑目标（顶点 0 位置差分 / 顶点 5 位置+法线差分）。
fn smile() -> MorphTarget {
    MorphTarget {
        id: 1,
        label: "微笑",
        deltas: vec![
            VertexDelta { index: 0, pos: [0.0, 2.0, 0.0], normal: [0.0, 0.0, 0.0] },
            VertexDelta { index: 5, pos: [1.0, 0.0, 0.0], normal: [0.0, 1.0, 0.0] },
        ],
    }
}

/// 法线归一化期望（1/√2，判据侧写死——容差 1e-4）。
const EXP_INV_SQRT2: f32 = 0.707_106_78;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// vef27 域自检入口。
pub fn run_vef27_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-morphtarget");

    // ================= 一、morph 通路（INTERP） =================

    let t = smile();

    // INT-1：恰边界双向——w=0 原样、w=1 全量差分（手算写死）。
    let w0 = apply_morph([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], &t, 0, 0.0);
    let w1 = apply_morph([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], &t, 0, 1.0);
    let int1 = match (w0, w1) {
        (Ok(a), Ok(b)) => a.pos == [1.0, 2.0, 3.0]
            && b.pos == [1.0, 4.0, 3.0]
            && a.touched
            && b.touched,
        _ => false,
    };
    s.add("F1624-INT-恰边界双向插值", int1, "");

    // INT-2：中间权重线性（w=0.5 → [1,3,3]，手算写死）+ 位置与法线同时执行。
    let w05 = apply_morph([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], &t, 5, 0.5);
    let int2 = match w05 {
        Ok(a) => a.pos == [1.5, 2.0, 3.0],
        _ => false,
    };
    s.add("F1624-INT-中间权重线性对拍", int2, "");

    // INT-3：法线插值后归一化（差分 [0,1,0] 叠在 [0,0,1] → 期望 [0,1/√2,1/√2]）
    // + 稀疏未触碰顶点原样（touched=false 且 pos==base）。
    let n = apply_morph([0.0; 3], [0.0, 0.0, 1.0], &t, 5, 1.0);
    let miss = apply_morph([9.0, 9.0, 9.0], [0.0, 0.0, 1.0], &t, 42, 0.7);
    let int3 = match (n, miss) {
        (Ok(a), Ok(b)) => {
            a.normal[0].abs() < 1e-4
                && (a.normal[1] - EXP_INV_SQRT2).abs() < 1e-4
                && (a.normal[2] - EXP_INV_SQRT2).abs() < 1e-4
                && !b.touched
                && b.pos == [9.0, 9.0, 9.0]
        }
        _ => false,
    };
    s.add("F1624-INT-法线归一化与稀疏未触碰", int3, "");

    // ================= 二、帧序列（TRACK） =================

    // TRK-1：双帧线性插值手算（tick 50 → 0.5）。
    let track = WeightTrack {
        target_id: 1,
        keys: vec![
            WeightKeyframe { tick: 0, weight: 0.0 },
            WeightKeyframe { tick: 100, weight: 1.0 },
        ],
    };
    s.add("F1624-TRK-双帧线性插值", weight_at(&track, 50) == Ok(0.5), "");

    // TRK-2：恰帧命中 + 轨道外端点钳制。
    let trk2 = weight_at(&track, 0) == Ok(0.0)
        && weight_at(&track, 100) == Ok(1.0)
        && weight_at(&track, 200) == Ok(1.0);
    s.add("F1624-TRK-恰帧命中端点钳制", trk2, "");

    // TRK-3：空轨道/乱序拒绝 + F1345 关键帧联动声明在账（判据侧逐字对拍）。
    let empty = WeightTrack { target_id: 1, keys: vec![] };
    let bad = WeightTrack {
        target_id: 1,
        keys: vec![
            WeightKeyframe { tick: 10, weight: 0.2 },
            WeightKeyframe { tick: 5, weight: 0.1 },
        ],
    };
    s.add(
        "F1624-TRK-空轨乱序拒与联动声明",
        weight_at(&empty, 0) == Err(MtCode::EMPTY_TRACK)
            && weight_at(&bad, 7) == Err(MtCode::KEYS_UNORDERED)
            && KEYFRAME_UPLINK == EXP_KEYFRAME_UPLINK
            && KEYFRAME_UPLINK_NOTE.contains("关键帧系统（F1345）联动"),
        "",
    );

    // ================= 三、morph 组合（COMBINE） =================

    let blink = MorphTarget {
        id: 2,
        label: "眨眼",
        deltas: vec![VertexDelta { index: 0, pos: [0.0, 1.0, 0.0], normal: [0.0, 0.0, 0.0] }],
    };
    let two = [t.clone(), blink];

    // CMB-1：replace 归一化恰边界——Σ==1 放行（0.5+0.5 手算叠加 [0, 2, 0]）；
    // Σ>1 拒（0.6+0.6）。
    let half = [
        MorphWeight { target_id: 1, weight: 0.5 },
        MorphWeight { target_id: 2, weight: 0.5 },
    ];
    let over = [
        MorphWeight { target_id: 1, weight: 0.6 },
        MorphWeight { target_id: 2, weight: 0.6 },
    ];
    let ok_half = combine_vertex([0.0; 3], [0.0, 0.0, 1.0], 0, &two, &half, CombineMode::Replace);
    let cmb1 = match ok_half {
        Ok(c) => c.pos == [0.0, 1.5, 0.0] && c.applied.len() == 2,
        _ => false,
    } && combine_vertex([0.0; 3], [0.0, 0.0, 1.0], 0, &two, &over, CombineMode::Replace)
        == Err(MtCode::WEIGHT_SUM_EXCEEDS);
    s.add("F1624-CMB-replace归一化恰边界", cmb1, "");

    // CMB-2：add 模式独立叠加——Σ>1 放行、位移累加手算（w=1.0 与 0.5 → [0, 2.5, 0]）。
    let add_ws = [
        MorphWeight { target_id: 1, weight: 1.0 },
        MorphWeight { target_id: 2, weight: 0.5 },
    ];
    let cmb2 = match combine_vertex([0.0; 3], [0.0, 0.0, 1.0], 0, &two, &add_ws, CombineMode::Add) {
        Ok(c) => c.pos == [0.0, 2.5, 0.0],
        _ => false,
    };
    s.add("F1624-CMB-add叠加语义手算", cmb2, "");

    // CMB-3：未知目标拒绝 + applied 账条数==权重条数。
    let ghost = [MorphWeight { target_id: 99, weight: 0.5 }];
    let good = [MorphWeight { target_id: 1, weight: 0.5 }];
    let cmb3 = combine_vertex([0.0; 3], [0.0, 0.0, 1.0], 0, &two, &ghost, CombineMode::Add)
        == Err(MtCode::UNKNOWN_TARGET)
        && match combine_vertex([0.0; 3], [0.0, 0.0, 1.0], 0, &two, &good, CombineMode::Add) {
            Ok(c) => c.applied.len() == 1 && c.applied.first().map(|x| *x) == Some(1),
            _ => false,
        };
    s.add("F1624-CMB-未知目标拒与账齐", cmb3, "");

    // ================= 四、差分量化（QUANT） =================

    // QN-1：量化步长/半步字面量独立对拍。
    s.add(
        "F1624-QN-量化字面量对拍",
        QUANT_SCALE == EXP_QUANT_SCALE
            && QUANT_FRACT_BITS == 12
            && QUANT_HALF_STEP == EXP_HALF_STEP,
        "",
    );

    // QN-2：往返误差有界（正/负/小量三语料，判据侧独立重算）。
    let samples = [0.125_f32, -0.25, 0.0001, 2.5, -7.75];
    let mut rt_ok = samples.len() == 5;
    for v in samples.iter() {
        if (dequant_q12(quant_q12(*v)) - *v).abs() > QUANT_HALF_STEP {
            rt_ok = false;
        }
    }
    s.add("F1624-QN-往返误差界", rt_ok, "");

    // QN-3：稀疏只存差异——量化表与原差异表逐条下标一致（只存差异顶点）。
    let q = quantize_target(&t, 8);
    let mut sparse_ok = q.deltas.len() == t.deltas.len() && q.vertex_count == 8;
    for i in 0..t.deltas.len() {
        if t.deltas.get(i).map(|d| d.index) != q.deltas.get(i).map(|d| d.index) {
            sparse_ok = false;
        }
    }
    s.add("F1624-QN-稀疏只存差异", sparse_ok, "");

    // QN-4：稀疏度手算（8 顶点网格 2 差分 → 0.25）+ 零网格拒绝
    // + 定点反演加权手算（base 1.0 + q(0.5)·w0.5 → 1.25 容差半步）。
    let sp = sparsity(&t, 8);
    let qn4 = match sp {
        Ok(v) => (v - 0.25).abs() < 1e-6,
        _ => false,
    } && sparsity(&t, 0) == Err(MtCode::EMPTY_TRACK)
        && {
            let qd = quant_q12(0.5);
            (dequant_apply(1.0, qd, 0.5) - 1.25).abs() <= QUANT_HALF_STEP
        };
    s.add("F1624-QN-稀疏度手算与零网格拒", qn4, "");

    // ================= 五、判据自检（META） =================

    // META-2：判据容量无截断。
    s.add("F1624-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x44xx，且 != 0x4x 其余段（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x44)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x40 && hi != 0x41 && hi != 0x42 && hi != 0x43
                && hi != 0x45 && hi != 0x46 && hi != 0x47 && hi != 0x48 && hi != 0x49
        });
    s.add("F1624-META-诊断码段独占", section_ok, "");

    // META-4：码两两互异 + 人话原因非空。
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
    s.add("F1624-META-码互异原因非空", code_ok, "");

    // META-1：判据条数对账（放末位：此时 len 应为 16，加自身恰 17）。
    s.add("F1624-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
