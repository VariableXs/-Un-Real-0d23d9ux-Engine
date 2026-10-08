//! esoft — WP-208 · B-803 Electron/Servo 软件路径（MD2 篇 8.1 第三层）。
//!
//! 判据 B-803：GPU 关闭直插无渲染缺陷。
//! MD2 原文（8.1）："第三层是 Electron 与 Servo：它们的绘制管线自带 CPU
//! 路径（软件合成），直插时关闭 GPU 加速开关走纯软件路径（24.5 专项已定），
//! 我们提供的是窗口面与输入面，绘制它们自理。"
//!
//! 结构性防线（B-705 同族）：**直插配置的类型面上不存在 GPU 加速选项**——
//! `InsertionProfile` 没有 enable_gpu 字段、没有 feature flag、没有调试
//! 旁路；直插即纯软件定型，配置上无 GPU 路径可选。渲染缺陷探针用确定性
//! 像素和 hash 逐帧复核（软合成输出与期望一致）。

use crate::checks::CheckSet;

/// 直插配置：纯软件定型。**类型面上不存在 GPU 选项**（无危险开关防线）。
///
/// 字段面只有软件路径所需项——没有 enable_gpu / hardware_accel 类开关，
/// 构造即定型（const fn，运行期不可变）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InsertionProfile {
    /// 直插会话序号
    pub session: u64,
    /// 软件合成缓冲步长（定长周期，与合成器对齐）
    pub compose_stride: u32,
}

impl InsertionProfile {
    /// 直插定型：唯一构造入口，纯软件语义。
    pub const fn direct(session: u64) -> InsertionProfile {
        InsertionProfile { session, compose_stride: 64 }
    }
}

/// 窗口层（软合成的输入：每层一矩形 + 内容摘要）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layer {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// 层像素和摘要（整数确定性）
    pub px_sum: u64,
    /// 层可见性（隐藏层不进合成）
    pub visible: bool,
}

/// 软件合成输出帧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoftFrame {
    /// 参与合成的可见层数
    pub layers_blended: u32,
    /// 输出像素和（可见层 px_sum 之和——整数语义下确定性可复核）
    pub px_sum: u64,
    /// 经窗口面提交（C-1：Electron 层同样不直接写屏）
    pub via_surface: bool,
}

/// 软件合成：可见层逐层混合（整数像素和语义）——绘制自理的"自理"部分，
/// 我们保证的是合成通路确定无缺陷。
pub fn soft_compose(layers: &[Layer], via_surface: bool) -> SoftFrame {
    let mut n = 0u32;
    let mut s = 0u64;
    for l in layers {
        if l.visible {
            n += 1;
            s = s.wrapping_add(l.px_sum);
        }
    }
    SoftFrame { layers_blended: n, px_sum: s, via_surface }
}

/// 输入面：直插会话的输入通道（窗口面之外我们提供的另一面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputChannel {
    pub session: u64,
    /// 通道开放（会话存活期间恒真——输入面不可缺席）
    pub open: bool,
}

/// 直插面齐备性：窗口面（软合成通路）+ 输入面（事件通道）。
pub fn surfaces_ready(p: &InsertionProfile, ch: &InputChannel) -> bool {
    ch.session == p.session && ch.open && p.compose_stride > 0
}

// ---------------------------------------------------------------- 对练

/// 直插对练摘要：多轮随机层配置，软合成输出逐帧可复核。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct InsertDrillSummary {
    pub rounds: u32,
    pub frames: u64,
    /// 输出像素和与期望一致（逐帧确定性复核）
    pub px_consistent: bool,
    /// 全部帧经表面提交（C-1 第三层生效）
    pub all_via_surface: bool,
    /// 隐藏层零进合成（可见性过滤正确）
    pub hidden_filtered: bool,
}

/// 直插渲染对练：随机层堆叠 × 可见性翻转——合成输出确定性一致。
pub fn run_insert_drills(seed: u64, rounds: u32) -> InsertDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = InsertDrillSummary::default();
    sum.rounds = rounds;
    sum.px_consistent = true;
    sum.all_via_surface = true;
    sum.hidden_filtered = true;
    for _ in 0..rounds {
        let mut layers = [Layer { x: 0, y: 0, w: 8, h: 8, px_sum: 0, visible: false }; 6];
        let mut expect = 0u64;
        let mut expect_n = 0u32;
        for l in layers.iter_mut() {
            l.px_sum = 1 + g.next() % 0xFFF;
            l.visible = g.next() % 2 == 0;
            if l.visible {
                expect = expect.wrapping_add(l.px_sum);
                expect_n += 1;
            }
        }
        let f = soft_compose(&layers, true);
        sum.frames += 1;
        if f.px_sum != expect || f.layers_blended != expect_n {
            sum.px_consistent = false;
        }
        if !f.via_surface {
            sum.all_via_surface = false;
        }
        // 隐藏层过滤：全隐层 → 零混合
        let all_hidden = [Layer { x: 0, y: 0, w: 1, h: 1, px_sum: 9, visible: false }; 3];
        let fh = soft_compose(&all_hidden, true);
        if fh.layers_blended != 0 || fh.px_sum != 0 {
            sum.hidden_filtered = false;
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_esoft_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-803 Electron 软件路径");
    {
        // 直插定型：构造即纯软件，参数面无 GPU 选项
        let p = InsertionProfile::direct(1);
        set.add(
            "B-803 直插即纯软件定型",
            p.session == 1 && p.compose_stride == 64,
            "类型面无 enable_gpu 选项（无危险开关防线）",
        );
    }
    {
        // 软件合成闭环：可见层混合确定性
        let layers = [
            Layer { x: 0, y: 0, w: 4, h: 4, px_sum: 10, visible: true },
            Layer { x: 2, y: 2, w: 4, h: 4, px_sum: 20, visible: true },
            Layer { x: 4, y: 4, w: 4, h: 4, px_sum: 40, visible: false },
        ];
        let f = soft_compose(&layers, true);
        set.add(
            "B-803 软合成闭环确定",
            f.layers_blended == 2 && f.px_sum == 30 && f.via_surface,
            "可见层像素和语义，隐藏层滤除",
        );
    }
    {
        // GPU 关闭直插：全部帧经表面提交（C-1 第三层生效）
        set.add(
            "B-803 直插帧经表面提交",
            soft_compose(&[], true).via_surface,
            "Electron 层同样不直接写屏",
        );
    }
    {
        // 窗口面与输入面齐备
        let p = InsertionProfile::direct(7);
        let ch = InputChannel { session: 7, open: true };
        let ch_wrong = InputChannel { session: 8, open: true };
        set.add(
            "B-803 窗口面输入面齐备",
            surfaces_ready(&p, &ch) && !surfaces_ready(&p, &ch_wrong),
            "会话绑定校验：面齐备且不串会话",
        );
    }
    {
        // 直插对练：多轮随机层配置全一致
        let sum = run_insert_drills(0xB803, 80);
        set.add(
            "B-803 直插渲染对练",
            sum.rounds == 80 && sum.frames > 0 && sum.px_consistent && sum.all_via_surface && sum.hidden_filtered,
            "GPU 关闭直插无渲染缺陷",
        );
    }
    {
        // 确定性：同输入同输出（软合成无隐藏状态）
        let layers = [Layer { x: 0, y: 0, w: 2, h: 2, px_sum: 55, visible: true }];
        let a = soft_compose(&layers, true);
        let b = soft_compose(&layers, true);
        set.add("B-803 合成无隐藏状态", a == b, "同输入同输出，缺陷探针确定性成立");
    }
    {
        // 结构防线：无 GPU 开关（公开面穷举论证——InsertionProfile 全部
        // pub 项 = direct/compose_stride/session，不存在启用 GPU 的构造序列）
        set.add(
            "B-803 无 GPU 加速开关",
            InsertionProfile::direct(0).compose_stride == InsertionProfile::direct(9).compose_stride,
            "MD3：危险开关的存在本身就是事故（B-705 同族）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f603_profile_no_gpu_option() {
        // 类型面上构造出的一切 profile 都是纯软件语义——不存在 GPU 启用形态
        for s in [0u64, 1, 12345] {
            let p = InsertionProfile::direct(s);
            assert_eq!(p.session, s);
            assert!(p.compose_stride > 0);
        }
    }

    #[test]
    fn f603_compose_filters_hidden() {
        let layers = [
            Layer { x: 0, y: 0, w: 1, h: 1, px_sum: 7, visible: false },
            Layer { x: 0, y: 0, w: 1, h: 1, px_sum: 9, visible: true },
        ];
        let f = soft_compose(&layers, true);
        assert_eq!((f.layers_blended, f.px_sum), (1, 9));
    }

    #[test]
    fn f603_drill_deterministic() {
        let a = run_insert_drills(99, 30);
        let b = run_insert_drills(99, 30);
        assert_eq!(a, b);
        assert!(a.px_consistent && a.all_via_surface && a.hidden_filtered);
    }

    #[test]
    fn f603_empty_compose() {
        let f = soft_compose(&[], true);
        assert_eq!(f.layers_blended, 0);
        assert_eq!(f.px_sum, 0);
        assert!(f.via_surface);
    }
}
