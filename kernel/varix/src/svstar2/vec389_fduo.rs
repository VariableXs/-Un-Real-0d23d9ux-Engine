//! VE-F0789 双色调滤镜
//! 职责：以亮度为权重在暗端/亮端两色间插值，支持中点与硬度（斜率）。
//! 契约：亮度 0 → 暗端色、255 → 亮端色；alpha 通道直通。

use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};

/// 双色调配置
#[derive(Clone, Debug, PartialEq)]
pub struct DuoCfg {
    pub dark: Pixel,
    pub light: Pixel,
    /// 中点亮度（0~255）
    pub mid: f64,
    /// 对比强度（1.0 = 线性；>1 = 增强对比；<1 = 压缩）
    pub strength: f64,
}

impl Default for DuoCfg {
    fn default() -> DuoCfg {
        DuoCfg { dark: [0, 0, 0, 255], light: [255, 255, 255, 255], mid: 128.0, strength: 1.0 }
    }
}

/// 亮度（Rec.601 加权，0~1）
pub fn luma(p: Pixel) -> f64 {
    (p[0] as f64 * 0.299 + p[1] as f64 * 0.587 + p[2] as f64 * 0.114) / 255.0
}

/// 对比整形：以 mid 为枢轴放大/缩小距离
pub fn shape(l: f64, mid: f64, strength: f64) -> f64 {
    let t = (l - mid / 255.0) * strength + mid / 255.0;
    t.clamp(0.0, 1.0)
}

/// 双通道插值（各分量独立线性）
pub fn mix(a: Pixel, b: Pixel, t: f64) -> Pixel {
    let f = t.clamp(0.0, 1.0);
    [
        (a[0] as f64 + (b[0] as f64 - a[0] as f64) * f).round() as u8,
        (a[1] as f64 + (b[1] as f64 - a[1] as f64) * f).round() as u8,
        (a[2] as f64 + (b[2] as f64 - a[2] as f64) * f).round() as u8,
        a[3],
    ]
}

/// 单像素双色调
pub fn duo_px(p: Pixel, cfg: &DuoCfg) -> Pixel {
    let t = shape(luma(p), cfg.mid, cfg.strength);
    mix(cfg.dark, cfg.light, t)
}

/// 全图双色调
pub fn duotone(src: &FBitmap, cfg: &DuoCfg) -> FBitmap {
    let mut out = FBitmap::new(src.w, src.h);
    for y in 0..src.h {
        for x in 0..src.w {
            out.set(x, y, duo_px(src.get(x, y), cfg));
        }
    }
    out
}

/// 算子包装：dark_r..light_b 六参数 + mid + strength
pub fn op_duo(b: &FBitmap, p: &Params) -> Result<FBitmap, &'static str> {
    let g = |k: &str, d: f64| p.get_or(k, d) as u8;
    let cfg = DuoCfg {
        dark: [g("dark_r", 0.0), g("dark_g", 0.0), g("dark_b", 0.0), 255],
        light: [g("light_r", 255.0), g("light_g", 255.0), g("light_b", 255.0), 255],
        mid: p.get_or("mid", 128.0),
        strength: p.get_or("strength", 1.0),
    };
    Ok(duotone(b, &cfg))
}

pub fn run_vec389_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec389");
    // 族一：亮度
    s.add("双调-黑亮度0", luma([0, 0, 0, 255]) == 0.0, "black");
    s.add("双调-白亮度1", (luma([255, 255, 255, 255]) - 1.0).abs() < 1e-9, "white");
    s.add("双调-绿主导", luma([0, 255, 0, 255]) > 0.5, "green heavy");
    // 族二：shape 整形
    s.add("双调-枢轴中点", (shape(0.5, 128.0, 2.0) - 0.5).abs() < 1e-9, "pivot");
    s.add("双调-增强外推", shape(0.6, 128.0, 2.0) > 0.6, "expand");
    s.add("双调-压缩内缩", shape(0.6, 128.0, 0.5) < 0.6, "compress");
    s.add("双调-夹取", shape(0.99, 128.0, 10.0) <= 1.0, "clamp hi");
    // 族三：mix 插值
    s.add("双调-t0取暗", mix([10, 20, 30, 9], [200, 210, 220, 9], 0.0) == [10, 20, 30, 9], "t=0");
    s.add("双调-t1取亮", mix([10, 20, 30, 9], [200, 210, 220, 9], 1.0) == [200, 210, 220, 9], "t=1");
    s.add("双调-t半", mix([0, 0, 0, 9], [100, 100, 100, 9], 0.5) == [50, 50, 50, 9], "t=.5");
    s.add("双调-t越界夹取", mix([0, 0, 0, 9], [100, 0, 0, 9], 1.7) == [100, 0, 0, 9], "t clamp");
    // 族四：全图端点行为
    let cfg = DuoCfg::default();
    let dark_bmp = FBitmap::solid(2, 2, [0, 0, 0, 255]);
    let light_bmp = FBitmap::solid(2, 2, [255, 255, 255, 255]);
    let od = duotone(&dark_bmp, &cfg);
    let ol = duotone(&light_bmp, &cfg);
    s.add("双调-黑图→暗端", od.get(0, 0) == cfg.dark, "black map");
    s.add("双调-白图→亮端", ol.get(0, 0) == cfg.light, "white map");
    // 族五：alpha 直通
    let a42 = FBitmap::solid(1, 1, [128, 128, 128, 42]);
    let oa = duotone(&a42, &cfg);
    s.add("双调-alpha直通", oa.get(0, 0)[3] == 42, "alpha keep");
    // 族六：算子包装
    let mut p = Params::new();
    p.set("dark_r", 255.0).set("dark_g", 0.0).set("dark_b", 0.0);
    p.set("light_r", 0.0).set("light_g", 0.0).set("light_b", 255.0);
    let half = FBitmap::solid(2, 2, [128, 128, 128, 255]);
    let od2 = op_duo(&half, &p).expect("op");
    s.add("双调-算子红蓝端", od2.get(0, 0)[0] > 0 && od2.get(0, 0)[2] > 0, "op mix");
    // 族七：尺寸守恒
    s.add("双调-尺寸不变", od2.w == 2 && od2.h == 2, "size keep");
    s
}
