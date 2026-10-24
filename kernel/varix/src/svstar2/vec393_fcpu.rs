//! VE-F0793 滤镜 CPU 实现
//! 职责：行分块（row chunk）执行模型 —— 把大图按行块切片逐块处理，
//!       支持进度回调与中断；提供每行 ops 计数（预算对账用）。
//! 契约：分块结果与全图一次性结果逐位一致。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Pixel};

/// 像素级变换（无邻域依赖的算子抽象）
pub type PxXform = fn(Pixel) -> Pixel;

/// 行块切片：返回 (起始行, 行数) 序列
pub fn row_chunks(h: usize, chunk: usize) -> Vec<(usize, usize)> {
    let c = chunk.max(1);
    let mut out = alloc::vec![];
    let mut y = 0;
    while y < h {
        let n = (h - y).min(c);
        out.push((y, n));
        y += n;
    }
    out
}

/// 行块逐块执行（无中断路径）
pub fn xform_blocked(src: &FBitmap, chunk: usize, f: PxXform) -> FBitmap {
    let mut out = FBitmap::new(src.w, src.h);
    for (y0, n) in row_chunks(src.h, chunk) {
        for y in y0..y0 + n {
            for x in 0..src.w {
                out.set(x, y, f(src.get(x, y)));
            }
        }
    }
    out
}

/// 带中断的执行：progress 已完成行数；返回 Err(None) 表示中断于第几行
pub fn xform_interruptible(
    src: &FBitmap,
    chunk: usize,
    stop_after: usize,
    f: PxXform,
) -> Result<FBitmap, usize> {
    let mut out = FBitmap::new(src.w, src.h);
    let mut done = 0usize;
    for (y0, n) in row_chunks(src.h, chunk) {
        if done >= stop_after {
            return Err(done);
        }
        for y in y0..y0 + n {
            for x in 0..src.w {
                out.set(x, y, f(src.get(x, y)));
            }
        }
        done += n;
        if done >= stop_after && y0 + n < src.h {
            return Err(done);
        }
    }
    Ok(out)
}

/// 每像素 ops 计数（按变换复杂度分级）
pub fn ops_per_px(op_class: u32) -> u64 {
    match op_class {
        0 => 4,   // 亮度类：3 加 + 1 钳
        1 => 16,  // 矩阵类：4 通道 × 4 乘加
        2 => 45,  // 卷积类：9 邻域 × 3 通道 + 钳
        _ => 4,
    }
}

/// 全图 ops 总量
pub fn total_ops(b: &FBitmap, op_class: u32) -> u64 {
    (b.w * b.h) as u64 * ops_per_px(op_class)
}

/// 示例变换：反相（供判据使用）
pub fn xf_invert(p: Pixel) -> Pixel {
    [255 - p[0], 255 - p[1], 255 - p[2], p[3]]
}

pub fn run_vec393_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec393");
    // 族一：行块切片
    let ch = row_chunks(10, 4);
    s.add("cpu-切片数", ch == alloc::vec![(0, 4), (4, 4), (8, 2)], "10/4 chunks");
    s.add("cpu-整除", row_chunks(8, 4) == alloc::vec![(0, 4), (4, 4)], "exact");
    s.add("cpu-单块", row_chunks(5, 100) == alloc::vec![(0, 5)], "one chunk");
    s.add("cpu-零钳1", row_chunks(3, 0) == alloc::vec![(0, 1), (1, 1), (2, 1)], "chunk min 1");
    s.add("cpu-空图", row_chunks(0, 4).is_empty(), "empty");
    // 族二：分块一致性（不同 chunk 尺寸结果逐位一致）
    let mut src = FBitmap::new(7, 9);
    for i in 0..src.px.len() {
        src.px[i] = [(i % 256) as u8, 10, 20, 255];
    }
    let a = xform_blocked(&src, 1, xf_invert);
    let b = xform_blocked(&src, 3, xf_invert);
    let c = xform_blocked(&src, 100, xf_invert);
    s.add("cpu-分块一致", a.px == b.px && b.px == c.px, "chunk invariance");
    // 族三：变换正确性
    let inv = xform_blocked(&src, 2, xf_invert);
    s.add("cpu-反相值", inv.get(0, 0) == [255 - src.get(0, 0)[0], 245, 235, 255], "invert val");
    // 族四：中断语义
    let part = xform_interruptible(&src, 4, 5, xf_invert);
    match part {
        Err(done) => s.add("cpu-中断行数", done == 5, "stopped at 5"),
        Ok(_) => s.add("cpu-中断行数", false, "should interrupt"),
    }
    // stop_after ≥ h → 完成
    let full = xform_interruptible(&src, 4, 100, xf_invert);
    s.add("cpu-不中断完成", full.is_ok() && full.expect("f").px == inv.px, "no interrupt");
    // stop_after = 0 → 立即中断
    let zero = xform_interruptible(&src, 4, 0, xf_invert);
    s.add("cpu-零即停", zero == Err(0), "stop at 0");
    // 族五：ops 计数
    s.add("cpu-亮度ops", ops_per_px(0) == 4, "class 0");
    s.add("cpu-卷积ops", ops_per_px(2) == 45, "class 2");
    s.add("cpu-未知归亮度", ops_per_px(9) == 4, "fallback");
    let bm = FBitmap::new(10, 10);
    s.add("cpu-总ops", total_ops(&bm, 0) == 400, "total 10x10x4");
    s
}
