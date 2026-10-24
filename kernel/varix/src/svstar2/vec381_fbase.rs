//! VE-F0781 滤镜基座框架
//! 职责：滤镜参数抽象（命名参数表）、像素算子签名（行进式）、
//!       滤镜注册表（名字唯一 + 版本）、应用管线（参数校验→算子→后处理）。
//! 契约：算子不得修改尺寸；参数缺失显式报错；注册名唯一。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 像素（RGBA 各 8bit）
pub type Pixel = [u8; 4];

/// 灰度位图（滤镜工作域）
#[derive(Clone, Debug, PartialEq)]
pub struct FBitmap {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Pixel>,
}

impl FBitmap {
    pub fn new(w: usize, h: usize) -> FBitmap {
        FBitmap { w, h, px: alloc::vec![[0, 0, 0, 255]; w * h] }
    }
    pub fn solid(w: usize, h: usize, c: Pixel) -> FBitmap {
        FBitmap { w, h, px: alloc::vec![c; w * h] }
    }
    pub fn get(&self, x: usize, y: usize) -> Pixel {
        self.px[y * self.w + x]
    }
    pub fn set(&mut self, x: usize, y: usize, p: Pixel) {
        if x < self.w && y < self.h {
            self.px[y * self.w + x] = p;
        }
    }
    /// 尺寸不变性校验
    pub fn same_size(&self, o: &FBitmap) -> bool {
        self.w == o.w && self.h == o.h
    }
}

/// 命名参数表
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Params {
    pub kv: Vec<(&'static str, f64)>,
}

impl Params {
    pub fn new() -> Params {
        Params::default()
    }
    pub fn set(&mut self, k: &'static str, v: f64) -> &mut Self {
        if let Some(e) = self.kv.iter_mut().find(|(kk, _)| *kk == k) {
            e.1 = v;
        } else {
            self.kv.push((k, v));
        }
        self
    }
    pub fn get(&self, k: &str) -> Option<f64> {
        self.kv.iter().find(|(kk, _)| *kk == k).map(|(_, v)| *v)
    }
    pub fn get_or(&self, k: &str, dflt: f64) -> f64 {
        self.get(k).unwrap_or(dflt)
    }
}

/// 像素算子：全图变换（尺寸不变）
pub type PixelOp = fn(&FBitmap, &Params) -> Result<FBitmap, &'static str>;

/// 滤镜登记行
#[derive(Clone, Debug, PartialEq)]
pub struct FilterReg {
    pub name: &'static str,
    pub ver: u32,
    pub op: PixelOp,
}

/// 注册表
pub struct FilterRegistry {
    pub regs: Vec<FilterReg>,
}

impl FilterRegistry {
    pub fn new() -> FilterRegistry {
        FilterRegistry { regs: alloc::vec![] }
    }
    pub fn register(&mut self, name: &'static str, ver: u32, op: PixelOp) -> Result<(), &'static str> {
        if self.regs.iter().any(|r| r.name == name) {
            return Err("duplicate filter name");
        }
        self.regs.push(FilterReg { name, ver, op });
        Ok(())
    }
    pub fn lookup(&self, name: &str) -> Option<&FilterReg> {
        self.regs.iter().find(|r| r.name == name)
    }
}

impl Default for FilterRegistry {
    fn default() -> Self {
        FilterRegistry::new()
    }
}

/// 恒等滤镜（基座自检用）
pub fn op_identity(b: &FBitmap, _p: &Params) -> Result<FBitmap, &'static str> {
    Ok(b.clone())
}

/// 尺寸守恒断言包装：算子输出必须同尺寸
pub fn guarded(op: PixelOp, b: &FBitmap, p: &Params) -> Result<FBitmap, &'static str> {
    let out = op(b, p)?;
    if !out.same_size(b) {
        return Err("operator changed size");
    }
    Ok(out)
}

pub fn run_vec381_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec381");
    // 族一：位图
    let bm = FBitmap::solid(4, 4, [255, 0, 0, 255]);
    s.add("位图-纯色", bm.px.iter().all(|&p| p == [255, 0, 0, 255]), "solid");
    s.add("位图-取值", bm.get(2, 2) == [255, 0, 0, 255], "get");
    let mut bm2 = bm.clone();
    bm2.set(0, 0, [0, 255, 0, 255]);
    s.add("位图-写读", bm2.get(0, 0) == [0, 255, 0, 255], "set/get");
    bm2.set(9, 9, [0, 0, 0, 0]);
    s.add("位图-越界无害", bm2.px.len() == 16, "oob ignored");
    s.add("位图-尺寸比较", bm.same_size(&bm2) && !bm.same_size(&FBitmap::new(8, 4)), "same_size");
    // 族二：参数表
    let mut p = Params::new();
    p.set("radius", 4.0);
    s.add("参数-读取", p.get("radius") == Some(4.0), "get");
    s.add("参数-缺省", p.get("blur").is_none(), "missing");
    s.add("参数-带默认", p.get_or("blur", 2.0) == 2.0, "default");
    s.add("参数-覆盖", { p.set("radius", 8.0); p.kv.len() == 1 && p.get("radius") == Some(8.0) }, "overwrite not dup");
    // 族三：注册表
    let mut reg = FilterRegistry::new();
    s.add("注册-成功", reg.register("identity", 1, op_identity).is_ok(), "ok");
    s.add("注册-重名拒", reg.register("identity", 2, op_identity) == Err("duplicate filter name"), "dup");
    s.add("注册-查找", reg.lookup("identity").expect("f").ver == 1, "found");
    s.add("注册-未录", reg.lookup("nope").is_none(), "missing");
    // 族四：守卫包装
    let out = guarded(op_identity, &bm, &p);
    s.add("守卫-恒等过", out == Ok(bm.clone()), "identity pass");
    // 尺寸篡改算子
    fn bad_op(b: &FBitmap, _: &Params) -> Result<FBitmap, &'static str> {
        Ok(FBitmap::new(b.w + 1, b.h))
    }
    s.add("守卫-尺寸篡改拒", guarded(bad_op, &bm, &p) == Err("operator changed size"), "size guard");
    // 报错算子
    fn err_op(_: &FBitmap, _: &Params) -> Result<FBitmap, &'static str> {
        Err("bad params")
    }
    s.add("守卫-报错透传", guarded(err_op, &bm, &p) == Err("bad params"), "error passthrough");
    // 族五：管线（注册→查找→执行）
    let found = reg.lookup("identity").expect("op");
    let piped = (found.op)(&bm, &p);
    s.add("管线-执行", piped == Ok(bm.clone()), "pipeline exec");
    // 族六：默认空表
    s.add("默认-空注册", FilterRegistry::default().regs.is_empty(), "empty default");
    s
}
