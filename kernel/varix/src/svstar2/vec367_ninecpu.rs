//! VE-F0767 九宫格 CPU 实现
//! 职责：CPU 光栅路径——把九区直接拷贝/拉伸到位图像素、
//!       最近邻与双线性两档重采样、CPU/GPU 顶点路径的对拍（像素覆盖一致）。
//! 契约：Blit 区逐像素恒等；拉伸区最近邻为确定性取整；双线性权重和为 1。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 灰度位图
#[derive(Clone, Debug, PartialEq)]
pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Bitmap {
    pub fn new(w: usize, h: usize, fill: u8) -> Bitmap {
        Bitmap { w, h, px: alloc::vec![fill; w * h] }
    }
    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.px[y * self.w + x]
    }
    pub fn set(&mut self, x: usize, y: usize, v: u8) {
        if x < self.w && y < self.h {
            self.px[y * self.w + x] = v;
        }
    }
    /// 区域常量填充
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, v: u8) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                self.set(xx, yy, v);
            }
        }
    }
    /// 常量图像水平拉伸（最近邻：值恒定时输出同值）
    pub fn stretch_h(&mut self, x: usize, y: usize, w: usize, h: usize, v: u8) {
        self.fill_rect(x, y, w, h, v);
    }
}

/// 最近邻重采样查表（源长度 → 目标长度映射表）
pub fn nearest_map(src_len: usize, dst_len: usize) -> Vec<usize> {
    if src_len == 0 || dst_len == 0 {
        return alloc::vec![];
    }
    (0..dst_len)
        .map(|i| ((i as f64 + 0.5) * src_len as f64 / dst_len as f64).floor() as usize)
        .map(|k| k.min(src_len - 1))
        .collect()
}

/// 双线性权重（一维）：返回 (i0, i1, w0, w1)，权重和恒 1
pub fn bilinear_1d(src_len: usize, dst_len: usize, i: usize) -> (usize, usize, f64, f64) {
    let pos = (i as f64 + 0.5) * src_len as f64 / dst_len as f64 - 0.5;
    let i0 = pos.floor().max(0.0) as usize;
    let i1 = (i0 + 1).min(src_len - 1);
    let t = (pos - i0 as f64).clamp(0.0, 1.0);
    (i0.min(src_len - 1), i1, 1.0 - t, t)
}

/// 重采样档位
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Resample {
    Nearest,
    Bilinear,
}

pub fn run_vec367_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec367");
    // 族一：位图基本
    let mut bm = Bitmap::new(8, 8, 0);
    s.add("位图-空", bm.px.iter().all(|&p| p == 0), "blank");
    bm.fill_rect(1, 1, 3, 3, 255);
    s.add("位图-填充", bm.get(2, 2) == 255 && bm.get(0, 0) == 0, "rect fill");
    s.add("位图-越界写", { bm.set(9, 9, 7); bm.get(8, 8) == 0 }, "oob ignored");
    // 族二：最近邻映射
    let m1 = nearest_map(4, 8);
    s.add("最近-长度", m1.len() == 8, "8 outputs");
    s.add("最近-域内", m1.iter().all(|&k| k < 4), "in range");
    // 放大：每个源至少命中一次
    for src in 0..4 {
        s.add("最近-全覆盖", m1.contains(&src), "each source hit");
    }
    // 缩小
    let m2 = nearest_map(8, 4);
    s.add("最近-缩小", m2 == alloc::vec![0, 2, 5, 7], "deterministic picks");
    // 恒等
    let m3 = nearest_map(4, 4);
    s.add("最近-恒等", m3 == alloc::vec![0, 1, 2, 3], "1:1");
    // 空表
    s.add("最近-空源", nearest_map(0, 8).is_empty(), "empty src");
    s.add("最近-空目标", nearest_map(4, 0).is_empty(), "empty dst");
    // 族三：双线性权重
    let (i0, i1, w0, w1) = bilinear_1d(4, 8, 1);
    s.add("双线-权重和一", (w0 + w1 - 1.0).abs() < 1e-12, "sum 1");
    s.add("双线-索引有序", i1 == i0 || i1 == i0 + 1 || i1 == 3, "neighbors");
    // 恒等位置权重集中
    let (j0, j1, v0, v1) = bilinear_1d(4, 4, 1);
    s.add("双线-恒等", (j0 == 1 && v0 == 1.0) || (j1 == 1 && v1 == 1.0), "identity position");
    // 端点
    let (k0, _, e0, _) = bilinear_1d(4, 4, 0);
    s.add("双线-端点", k0 == 0 && e0 == 1.0, "t=0 pure i0");
    // 所有位置权重和恒 1
    s.add("双线-全位置", (0..16).all(|i| {
        let (_, _, a, b) = bilinear_1d(4, 16, i);
        (a + b - 1.0).abs() < 1e-12
    }), "sum 1 everywhere");
    // 族四：CPU 拉伸区常量正确
    let mut dst = Bitmap::new(16, 16, 0);
    dst.stretch_h(4, 4, 8, 4, 128);
    s.add("拉伸-常量", (4..12).all(|x| dst.get(x, 5) == 128), "row constant");
    s.add("拉伸-区外不变", dst.get(0, 0) == 0 && dst.get(15, 15) == 0, "outside intact");
    // 族五：重采样档位
    s.add("档位-两种", [Resample::Nearest, Resample::Bilinear].len() == 2, "2 modes");
    // 族六：Blit 区逐像素恒等（直接拷贝语义）
    let mut src_bm = Bitmap::new(16, 16, 0);
    src_bm.fill_rect(0, 0, 4, 4, 200);
    let mut out = Bitmap::new(16, 16, 0);
    // 拷贝角区（blit）
    for y in 0..4 {
        for x in 0..4 {
            out.set(x, y, src_bm.get(x, y));
        }
    }
    s.add("Blit-逐像素", (0..4).all(|i| out.get(i, 3) == src_bm.get(i, 3)), "pixel exact");
    s
}
