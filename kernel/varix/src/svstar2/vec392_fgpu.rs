//! VE-F0792 滤镜 GPU 实现计划
//! 职责：把滤镜算子翻译为 GPU 分发计划 —— uniform 打包、tile 划分、
//!       workgroup 尺寸推导；只产出可校验的计划数据，不做真实绘制。
//! 契约：tile 数 × tile 尺寸 ≥ 位图尺寸；uniform 按槽位布局。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::FBitmap;

/// uniform 槽位（std430 风格 4 字节对齐）
#[derive(Clone, Debug, PartialEq)]
pub struct UniformPack {
    /// 槽位名 → (偏移字节, 值)
    pub slots: Vec<(&'static str, usize, f32)>,
}

impl UniformPack {
    pub fn new() -> UniformPack {
        UniformPack { slots: alloc::vec![] }
    }
    pub fn push(&mut self, name: &'static str, v: f32) {
        let off = self.slots.len() * 4;
        self.slots.push((name, off, v));
    }
    pub fn total_bytes(&self) -> usize {
        self.slots.len() * 4
    }
    pub fn find(&self, name: &str) -> Option<(usize, f32)> {
        self.slots.iter().find(|(n, _, _)| *n == name).map(|&(_, o, v)| (o, v))
    }
}

impl Default for UniformPack {
    fn default() -> UniformPack {
        UniformPack::new()
    }
}

/// tile 分发计划
#[derive(Clone, Debug, PartialEq)]
pub struct TilePlan {
    pub w: usize,
    pub h: usize,
    /// tile 尺寸（正方形）
    pub tile: usize,
}

impl TilePlan {
    pub fn for_bitmap(b: &FBitmap, tile: usize) -> TilePlan {
        TilePlan { w: b.w, h: b.h, tile: tile.max(1) }
    }
    pub fn tiles_x(&self) -> usize {
        self.w.div_ceil(self.tile)
    }
    pub fn tiles_y(&self) -> usize {
        self.h.div_ceil(self.tile)
    }
    pub fn total_tiles(&self) -> usize {
        self.tiles_x() * self.tiles_y()
    }
    /// 覆盖率校验：tile 网格面积 ≥ 位图面积
    pub fn covers(&self) -> bool {
        self.tiles_x() * self.tile >= self.w && self.tiles_y() * self.tile >= self.h
    }
    /// workgroup (x, y)：tile 网格按 8×8 分组
    pub fn workgroups(&self) -> (usize, usize) {
        (self.tiles_x().div_ceil(8), self.tiles_y().div_ceil(8))
    }
}

/// 算子 → uniform 打包（按滤镜名分派；未识别 → 空 pack）
pub fn pack_uniforms(filter: &str, radius: f32, strength: f32) -> UniformPack {
    let mut u = UniformPack::new();
    match filter {
        "blur" => {
            u.push("u_radius", radius);
            u.push("u_texel", 1.0);
        }
        "shadow" => {
            u.push("u_dx", radius);
            u.push("u_dy", strength);
            u.push("u_alpha", 0.6);
        }
        "glow" => {
            u.push("u_radius", radius);
            u.push("u_gain", strength);
        }
        _ => {}
    }
    u
}

pub fn run_vec392_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec392");
    // 族一：uniform 打包
    let mut u = UniformPack::new();
    u.push("a", 1.0);
    u.push("b", 2.0);
    s.add("gpu-槽位偏移", u.find("b") == Some((4, 2.0)), "offset slot1");
    s.add("gpu-缺槽", u.find("c").is_none(), "missing slot");
    s.add("gpu-总字节", u.total_bytes() == 8, "total bytes");
    // 族二：tile 覆盖
    let bm = FBitmap::new(50, 33);
    let tp = TilePlan::for_bitmap(&bm, 16);
    s.add("gpu-tilesX", tp.tiles_x() == 4, "50/16→4");
    s.add("gpu-tilesY", tp.tiles_y() == 3, "33/16→3");
    s.add("gpu-总数", tp.total_tiles() == 12, "total");
    s.add("gpu-覆盖校验", tp.covers(), "covers");
    // 族三：整除场景
    let sq = FBitmap::new(32, 32);
    let tq = TilePlan::for_bitmap(&sq, 16);
    s.add("gpu-整除", tq.tiles_x() == 2 && tq.covers(), "exact divide");
    s.add("gpu-零浪费", tq.total_tiles() == 4, "no waste");
    // 族四：tile 尺寸钳制
    let t1 = TilePlan::for_bitmap(&bm, 0);
    s.add("gpu-tile钳1", t1.tile == 1, "tile min 1");
    // 族五：workgroup
    let wg = tp.workgroups();
    s.add("gpu-workgroup", wg == (1, 1), "8x8 group small");
    let big = TilePlan { w: 512, h: 512, tile: 16 };
    s.add("gpu-workgroup大图", big.workgroups() == (4, 4), "512/16=32 tiles→4wg");
    // 族六：uniform 分派
    let ub = pack_uniforms("blur", 4.0, 0.0);
    s.add("gpu-blur槽", ub.find("u_radius") == Some((0, 4.0)), "blur radius");
    let us = pack_uniforms("shadow", 3.0, 5.0);
    s.add("gpu-shadow槽", us.find("u_dy") == Some((4, 5.0)), "shadow dy");
    let ug = pack_uniforms("glow", 2.0, 1.5);
    s.add("gpu-glow槽数", ug.slots.len() == 2, "glow slots");
    s.add("gpu-未识别空", pack_uniforms("other", 1.0, 1.0).slots.is_empty(), "unknown empty");
    // 族七：计划等值
    s.add("gpu-计划等值", TilePlan::for_bitmap(&sq, 16) == tq, "plan eq");
    s
}
