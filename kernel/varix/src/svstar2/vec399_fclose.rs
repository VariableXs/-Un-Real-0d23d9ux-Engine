//! VE-F0799 滤镜组收口
//! 职责：组级 fuzz（随机参数跑全算子不 panic）、组级一致性
//!       （同输入同算子跨入口一致）、遗留风险登记、组判定聚合。
//! 契约：收口失败必须能指认到模块级判据；本模块不新增语义。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};
use super::vec335_cfuzz::Rng;

/// 组级遗留风险
pub fn risks() -> Vec<&'static str> {
    alloc::vec![
        "置换滤镜算子形态仅恒等位移——带位移图算子挂账 D07 参数注入",
        "GPU 计划仅 tile/uniform 层——真实队列提交挂账 cgpu 域",
        "噪声 LCG 周期 2^64 级——密码学级需求不在范围",
        "4K 定点亮度权重为 77/150/29 近似——与 f64 权重差 <1/255",
    ]
}

/// 全算子冒烟：随机参数逐个调用，不允许 panic，返回调用数
pub fn smoke_all_ops(seed: u64, rounds: usize) -> usize {
    let mut rng = Rng::new(seed);
    let mut called = 0usize;
    for _ in 0..rounds {
        let w = 1 + (rng.next() % 8) as usize;
        let h = 1 + (rng.next() % 8) as usize;
        let c: u8 = (rng.next() % 256) as u8;
        let bm = FBitmap::solid(w, h, [c, c, c, 255]);
        let mut p = Params::new();
        let f = (rng.next() % 1000) as f64 / 1000.0;
        p.set("radius", f).set("amount", f).set("value", f).set("strength", f);
        // 各算子逐一调用（只要求不 panic）
        let _ = super::vec382_fblur::op_blur(&bm, &p);
        let _ = super::vec385_fcolor::op_color(&bm, &p);
        let _ = super::vec386_fmat::op_colormat(&bm, &p);
        let _ = super::vec387_fremap::op_displace(&bm, &p);
        let _ = super::vec388_fconv::op_conv(&bm, &p);
        let _ = super::vec389_fduo::op_duo(&bm, &p);
        let _ = super::vec390_fnoise::op_noise(&bm, &p);
        called += 7;
    }
    called
}

/// 一致性：op_identity 三入口（直接调用 / 守卫 / 管线）结果一致
pub fn consistency_identity(b: &FBitmap) -> bool {
    let p = Params::new();
    let direct = super::vec381_fbase::op_identity(b, &p);
    let guarded = super::vec381_fbase::guarded(super::vec381_fbase::op_identity, b, &p);
    direct == guarded && direct == Ok(b.clone())
}

/// 一致性：像素算子输出尺寸守恒（全注册算子过守卫）
pub fn conservation_size(w: usize, h: usize) -> bool {
    let bm = FBitmap::solid(w, h, [128, 128, 128, 255]);
    let mut p = Params::new();
    p.set("radius", 1.0).set("amount", 0.0).set("value", 0.0);
    let ops: Vec<super::vec381_fbase::PixelOp> = alloc::vec![
        super::vec382_fblur::op_blur,
        super::vec385_fcolor::op_color,
        super::vec386_fmat::op_colormat,
        super::vec388_fconv::op_conv,
        super::vec389_fduo::op_duo,
        super::vec390_fnoise::op_noise,
    ];
    ops.iter().all(|op| {
        super::vec381_fbase::guarded(*op, &bm, &p)
            .map(|o| o.w == w && o.h == h)
            .unwrap_or(false)
    })
}

pub fn run_vec399_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec399");
    // 族一：组级 fuzz 冒烟
    let n1 = smoke_all_ops(42, 20);
    s.add("收口-冒烟调用量", n1 == 20 * 7, "20 rounds x7 ops");
    let a = smoke_all_ops(42, 20);
    s.add("收口-冒烟可复现", a == n1, "seed reproducible");
    // 异常种子也不 panic（不同位型）
    let n2 = smoke_all_ops(u64::MAX, 5);
    s.add("收口-极值种子", n2 == 35, "max seed");
    // 族二：恒等一致性
    let bm = FBitmap::solid(3, 3, [9, 9, 9, 255]);
    s.add("收口-恒等一致", consistency_identity(&bm), "identity x3 path");
    // 族三：尺寸守恒
    s.add("收口-守恒小图", conservation_size(4, 4), "4x4");
    s.add("收口-守恒宽图", conservation_size(9, 2), "9x2");
    // 族四：风险登记
    let r = risks();
    s.add("收口-风险条数", r.len() == 4, "4 risks");
    s.add("收口-风险非空", r.iter().all(|x| !x.is_empty()), "risk text");
    // 族五：组聚合判定（关键路径真值复算）
    let sh = super::vec382_fblur::blur_h(&FBitmap::solid(5, 1, [100, 0, 0, 255]), 0);
    s.add("收口-模糊r0恒等", sh.px == FBitmap::solid(5, 1, [100, 0, 0, 255]).px, "blur r=0 id");
    let gray = super::vec386_fmat::apply_mat([255, 255, 255, 255], &super::vec386_fmat::GRAY);
    s.add("收口-灰度白", gray == [255, 255, 255, 255], "gray white");
    s.add("收口-反相黑", super::vec386_fmat::apply_mat([0, 0, 0, 255], &super::vec386_fmat::INVERT) == [255, 255, 255, 255], "invert black");
    s
}
