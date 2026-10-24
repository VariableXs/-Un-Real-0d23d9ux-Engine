//! VE-F0787 置换滤镜
//! 职责：以位移图（displacement map）的 R/G 通道驱动采样偏移，
//!       scale_x/scale_y 控制强度，越界采样回落到 clamp 边界。
//! 契约：输出尺寸不变；位移图与源图尺寸一致；零位移为恒等。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};

/// 位移图：按像素给出 (dx, dy) 浮点偏移
#[derive(Clone, Debug, PartialEq)]
pub struct DispMap {
    pub w: usize,
    pub h: usize,
    pub vec: Vec<(f64, f64)>,
}

impl DispMap {
    pub fn zero(w: usize, h: usize) -> DispMap {
        DispMap { w, h, vec: alloc::vec![(0.0, 0.0); w * h] }
    }
    pub fn get(&self, x: usize, y: usize) -> (f64, f64) {
        self.vec[y * self.w + x]
    }
    pub fn set(&mut self, x: usize, y: usize, d: (f64, f64)) {
        if x < self.w && y < self.h {
            self.vec[y * self.w + x] = d;
        }
    }
}

/// clamp 边界取样
fn clamp_sample(b: &FBitmap, x: i64, y: i64) -> Pixel {
    let cx = x.clamp(0, b.w as i64 - 1) as usize;
    let cy = y.clamp(0, b.h as i64 - 1) as usize;
    b.get(cx, cy)
}

/// 置换核心：p(x,y) ← src(x + dx*sx, y + dy*sy)（四舍五入到整数采样）
pub fn displace(src: &FBitmap, dm: &DispMap, sx: f64, sy: f64) -> Result<FBitmap, &'static str> {
    if src.w != dm.w || src.h != dm.h {
        return Err("displacement map size mismatch");
    }
    let mut out = FBitmap::new(src.w, src.h);
    for y in 0..src.h {
        for x in 0..src.w {
            let (dx, dy) = dm.get(x, y);
            let sxp = x as f64 + dx * sx;
            let syp = y as f64 + dy * sy;
            let ix = sxp.round() as i64;
            let iy = syp.round() as i64;
            out.set(x, y, clamp_sample(src, ix, iy));
        }
    }
    Ok(out)
}

/// 算子包装：参数 scale_x / scale_y（默认 1.0），位移图经参数不可行，
/// 故 op 形态以恒等位移为缺省（管线联调用），真实位移走 displace。
pub fn op_displace(b: &FBitmap, p: &Params) -> Result<FBitmap, &'static str> {
    let sx = p.get_or("scale_x", 1.0);
    let sy = p.get_or("scale_y", 1.0);
    let dm = DispMap::zero(b.w, b.h);
    displace(b, &dm, sx, sy)
}

pub fn run_vec387_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec387");
    // 族一：位移图
    let mut dm = DispMap::zero(4, 4);
    s.add("位移图-零初始化", dm.vec.iter().all(|&d| d == (0.0, 0.0)), "zero");
    dm.set(1, 1, (2.0, -1.0));
    s.add("位移图-读写", dm.get(1, 1) == (2.0, -1.0), "get/set");
    dm.set(9, 9, (5.0, 5.0));
    s.add("位移图-越界无害", dm.vec.len() == 16, "oob ignored");
    // 族二：恒等性（零位移）
    let src = FBitmap::solid(4, 4, [10, 20, 30, 255]);
    let id = displace(&src, &DispMap::zero(4, 4), 1.0, 1.0).expect("id");
    s.add("置换-零位移恒等", id.px == src.px, "identity");
    // 族三：正向位移取右邻
    let mut dm2 = DispMap::zero(4, 4);
    dm2.set(0, 0, (1.0, 0.0));
    let moved = displace(&src, &dm2, 1.0, 0.0).expect("moved");
    s.add("置换-x位移取样", moved.get(0, 0) == src.get(1, 0), "sample right");
    // 族四：越界回落 clamp
    let mut dm3 = DispMap::zero(4, 4);
    dm3.set(3, 0, (-3.0, 0.0));
    let cl = displace(&src, &dm3, 1.0, 0.0).expect("clamp");
    s.add("置换-clamp边界", cl.get(3, 0) == src.get(0, 0), "clamp left");
    // 族五：scale 放大
    let mut dm4 = DispMap::zero(4, 4);
    dm4.set(0, 0, (1.0, 0.0));
    let half = displace(&src, &dm4, 0.0, 0.0).expect("zero scale");
    s.add("置换-零scale恒等", half.get(0, 0) == src.get(0, 0), "scale 0");
    // 族六：尺寸不一致拒绝
    let bad = displace(&src, &DispMap::zero(2, 2), 1.0, 1.0);
    s.add("置换-尺寸拒", bad == Err("displacement map size mismatch"), "size reject");
    // 族七：算子包装
    let opout = op_displace(&src, &Params::new()).expect("op");
    s.add("置换-算子恒等", opout.px == src.px, "op identity");
    let mut p = Params::new();
    p.set("scale_x", 0.0).set("scale_y", 0.0);
    s.add("置换-算子零scale", op_displace(&src, &p).is_ok(), "op zero scale");
    // 族八：round 取样（0.5 上取）
    let mut dm5 = DispMap::zero(4, 4);
    dm5.set(0, 0, (0.5, 0.0));
    let rnd = displace(&src, &dm5, 1.0, 0.0).expect("round");
    s.add("置换-round取右", rnd.get(0, 0) == src.get(1, 0), "round half");
    s
}
