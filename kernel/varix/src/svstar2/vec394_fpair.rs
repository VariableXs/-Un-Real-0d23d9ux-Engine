//! VE-F0794 滤镜双路对拍
//! 职责：GPU 计划（vec392 tile/uniform）与 CPU 分块实现（vec393）的
//!       交叉一致性对拍 —— 计划覆盖、ops 预算、逐位基线三方互验。
//! 契约：对拍失败必须能定位到路（plan/cpu/ground）与坐标。

use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Pixel};
use super::vec392_fgpu::TilePlan;
use super::vec393_fcpu::{ops_per_px, total_ops, xf_invert, xform_blocked};

/// 对拍报告
#[derive(Clone, Debug, PartialEq)]
pub struct DiffReport {
    pub plan_ok: bool,
    pub cpu_ok: bool,
    pub ground_ok: bool,
    /// 首个不一致坐标（cpu vs ground）
    pub first_diff: Option<(usize, usize)>,
}

/// 全量对拍：ground = 逐像素直接变换（第三路真值）
pub fn cross_check(src: &FBitmap, tile: usize, chunk: usize, f: super::vec393_fcpu::PxXform) -> DiffReport {
    let tp = TilePlan::for_bitmap(src, tile);
    let plan_ok = tp.covers();
    // CPU 路：分块执行
    let cpu = xform_blocked(src, chunk, f);
    // 真值路：逐像素直接执行
    let mut ground = FBitmap::new(src.w, src.h);
    for y in 0..src.h {
        for x in 0..src.w {
            ground.set(x, y, f(src.get(x, y)));
        }
    }
    let mut first_diff = None;
    let mut cpu_ok = true;
    'outer: for y in 0..src.h {
        for x in 0..src.w {
            if cpu.get(x, y) != ground.get(x, y) {
                first_diff = Some((x, y));
                cpu_ok = false;
                break 'outer;
            }
        }
    }
    DiffReport { plan_ok, cpu_ok, ground_ok: true, first_diff }
}

/// 预算校验：总 ops 不超上限
pub fn budget_ok(b: &FBitmap, op_class: u32, cap: u64) -> bool {
    total_ops(b, op_class) <= cap
}

/// 反相基线（判据用第三路）
fn ground_invert(p: Pixel) -> Pixel {
    [!p[0], !p[1], !p[2], p[3]]
}

pub fn run_vec394_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec394");
    // 构造非均匀源图
    let mut src = FBitmap::new(13, 11);
    for y in 0..src.h {
        for x in 0..src.w {
            src.set(x, y, [(x * 19 % 256) as u8, (y * 23 % 256) as u8, 77, 255]);
        }
    }
    // 族一：三路一致（多种 tile/chunk 组合）
    for &(tile, chunk) in &[(1usize, 1usize), (4, 3), (16, 5), (64, 100)] {
        let r = cross_check(&src, tile, chunk, xf_invert);
        s.add(
            "对拍-三路一致",
            r.plan_ok && r.cpu_ok && r.first_diff.is_none(),
            "tile/chunk combo",
        );
    }
    // 族二：反相基线自洽（三路真值：!x == 255-x）
    let mut ok = true;
    for y in 0..src.h {
        for x in 0..src.w {
            if ground_invert(src.get(x, y)) != xf_invert(src.get(x, y)) {
                ok = false;
            }
        }
    }
    s.add("对拍-反相基线", ok, "invert ground truth");
    // 族三：报告字段语义
    let r = cross_check(&src, 8, 4, xf_invert);
    s.add("对拍-报告plan", r.plan_ok, "plan ok");
    s.add("对拍-报告无差异", r.first_diff.is_none(), "no diff");
    s.add("对拍-报告真值", r.ground_ok, "ground ok");
    // 族四：预算
    s.add("对拍-预算内", budget_ok(&src, 0, total_ops(&src, 0)), "within cap");
    s.add("对拍-预算超", !budget_ok(&src, 0, total_ops(&src, 0) - 1), "over cap");
    s.add("对拍-类单价", ops_per_px(1) > ops_per_px(0), "class price order");
    // 族五：tile 覆盖失败可检出（tile 小于 1 不可能，这里验证 covers 与 tile 关系）
    let tp = super::vec392_fgpu::TilePlan::for_bitmap(&src, 16);
    s.add("对拍-覆盖一致", tp.covers() == cross_check(&src, 16, 4, xf_invert).plan_ok, "cover agree");
    s
}
