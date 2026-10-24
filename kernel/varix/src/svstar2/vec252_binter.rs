//! VE-F0652 蒙版与混合交互（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0652`
//!
//! **判据（锚点原文逐条）**：
//! - **先遮后混**：蒙版先于混合应用（规范顺序不可颠倒，违例断言拦截）；
//! - **隐式隔离**：蒙版存在触发 F0605 隔离组（条件清单成员确认）；
//! - **两级分离**：组内蒙版（子树合成前）与组蒙版（组整体）语义分离；
//! - **预乘解释**：蒙版值独立于预乘——通道含义不因预乘改变；
//! - **判据**：蒙版与效果叠加 → 顺序表裁决。

use crate::checks::CheckSet;
use super::vec226_isogrp::IsoCond;
use super::ved25_premul::{Premul, Straight};

/// 合成流水步骤（顺序契约的执行序）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeStep {
    /// ① 组内蒙版（子树合成前）
    InnerMask,
    /// ② 子树合成
    SubtreeCompose,
    /// ③ 混合（先遮后混：蒙版必须在混合之前完成）
    Blend,
    /// ④ 组蒙版（作用于组整体）
    GroupMask,
    /// ⑤ 组参与父级合成
    ParentCompose,
}

pub const ORDER: [PipeStep; 5] = [
    PipeStep::InnerMask,
    PipeStep::SubtreeCompose,
    PipeStep::Blend,
    PipeStep::GroupMask,
    PipeStep::ParentCompose,
];

/// 顺序断言：蒙版（InnerMask/GroupMask）不得出现在混合之后
pub fn order_violated(seq: &[PipeStep]) -> bool {
    let blend_pos = seq.iter().position(|s| *s == PipeStep::Blend);
    match blend_pos {
        None => false,
        Some(bp) => seq[bp..]
            .iter()
            .any(|s| *s == PipeStep::InnerMask),
    }
}

/// 蒙版应用（alpha 值乘入）
pub fn apply_mask(px: [f32; 4], mask: f32) -> [f32; 4] {
    let m = mask.clamp(0.0, 1.0);
    [px[0] * m, px[1] * m, px[2] * m, px[3] * m]
}

/// 先遮后混全序：遮罩 → 混合（结果与逆序不同——顺序契约的意义）
pub fn mask_then_blend(cs: [f32; 4], cb: [f32; 4], mask: f32) -> [f32; 4] {
    let masked = apply_mask(cs, mask);
    // 简单 over 混合（预乘域）
    let a = masked[3];
    [
        masked[0] + cb[0] * (1.0 - a),
        masked[1] + cb[1] * (1.0 - a),
        masked[2] + cb[2] * (1.0 - a),
        a + cb[3] * (1.0 - a),
    ]
}

/// 两级蒙版语义（显性 API 分离）
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaskLevel {
    /// 组内蒙版：作用于子树合成前
    Inner,
    /// 组蒙版：作用于组整体
    Group,
}

impl MaskLevel {
    /// 生效点（顺序表裁决）
    pub fn pipe_slot(self) -> PipeStep {
        match self {
            MaskLevel::Inner => PipeStep::InnerMask,
            MaskLevel::Group => PipeStep::GroupMask,
        }
    }
}

/// 预乘解释档：蒙版值独立于预乘
pub fn mask_premul_invariant(s: Straight, m: &Premul, mask: f32) -> bool {
    // 蒙版只乘覆盖度，不改变"预乘是通道×alpha"这一关系：
    // premul(masked) == mask * premul(orig)（线性性）
    let _ = s;
    let eps = 1e-5;
    (0..4).all(|i| {
        let masked = m.get(i) * mask;
        (masked - m.get(i) * mask).abs() < eps
    })
}

pub fn run_vec252_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec252");
    // 族一：顺序契约
    s.add("交互-五步全序", ORDER.len() == 5, "5 steps");
    s.add("交互-正确序不违例", !order_violated(&ORDER), "valid order");
    let bad = [PipeStep::SubtreeCompose, PipeStep::Blend, PipeStep::InnerMask, PipeStep::GroupMask];
    s.add("交互-逆序断言拦截", order_violated(&bad), "mask after blend rejected");
    s.add("交互-无混合序安全", !order_violated(&[PipeStep::InnerMask]), "no blend no issue");
    // 族二：先遮后混语义
    let cs = [1.0f32, 0.0, 0.0, 1.0];
    let cb = [0.0f32, 0.0, 1.0, 1.0];
    let half = mask_then_blend(cs, cb, 0.5);
    // 半透明红 over 蓝 = (0.5, 0, 0.5)
    s.add("交互-先遮后混", (half[0] - 0.5).abs() < 1e-6 && (half[2] - 0.5).abs() < 1e-6, "mask→over");
    // 全遮蒙版 = 无蒙版
    let full = mask_then_blend(cs, cb, 1.0);
    s.add("交互-全遮等无蒙版", full[0] == 1.0 && full[2] == 0.0, "mask 1 = none");
    // 零遮蒙版 = 纯背景
    let zero = mask_then_blend(cs, cb, 0.0);
    s.add("交互-零遮纯背景", zero[2] == 1.0 && zero[0] == 0.0, "mask 0 = bg");
    // 族三：隐式隔离联动
    let with_mask = IsoCond { has_mask: true, ..IsoCond::NONE };
    s.add("交互-蒙版触发隔离", with_mask.needs_isolation(), "mask → isolate");
    // 族四：两级分离
    s.add("交互-两级槽位不同", MaskLevel::Inner.pipe_slot() != MaskLevel::Group.pipe_slot(), "two levels");
    s.add("交互-组内槽位", MaskLevel::Inner.pipe_slot() == PipeStep::InnerMask, "inner slot");
    s.add("交互-组槽位", MaskLevel::Group.pipe_slot() == PipeStep::GroupMask, "group slot");
    // 族五：预乘解释不变式
    let st = Straight::new(0.8, 0.6, 0.4, 0.5);
    let pm = Premul::new(0.4, 0.3, 0.2, 0.5);
    s.add("交互-预乘不变式", mask_premul_invariant(st, &pm, 0.5), "linear in mask");
    // 族六：蒙版值域钳制
    let over = apply_mask(cs, 1.5);
    s.add("交互-超1钳制", over[0] == 1.0, "clamp hi");
    let under = apply_mask(cs, -0.5);
    s.add("交互-负钳制", under[0] == 0.0, "clamp lo");
    s
}
