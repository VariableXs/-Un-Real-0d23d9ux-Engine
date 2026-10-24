//! VE-F0798 滤镜文档与基准
//! 职责：滤镜组文档登记（名册 + 契约行）与轻量基准
//!       （固定工作量计时位，结果只断言「可完成且工作量守恒」）。
//! 契约：名册覆盖 vec381~397 全部公开算子；基准不做真实时钟依赖。

use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, FilterRegistry, Params};
use super::vec393_fcpu::{ops_per_px, total_ops, xf_invert, xform_blocked};

/// 滤镜名册行
#[derive(Clone, Debug, PartialEq)]
pub struct DocRow {
    pub module: &'static str,
    pub op_name: &'static str,
    /// 契约要点（一行）
    pub contract: &'static str,
}

/// 全组名册
pub fn roster() -> alloc::vec::Vec<DocRow> {
    alloc::vec![
        DocRow { module: "vec381", op_name: "identity", contract: "尺寸不变" },
        DocRow { module: "vec382", op_name: "blur", contract: "盒式分离两趟" },
        DocRow { module: "vec383", op_name: "shadow", contract: "掩码位移合成" },
        DocRow { module: "vec384", op_name: "glow", contract: "膨胀掩码染色" },
        DocRow { module: "vec385", op_name: "color", contract: "亮度对比饱和" },
        DocRow { module: "vec386", op_name: "colormat", contract: "4x5 矩阵" },
        DocRow { module: "vec387", op_name: "displace", contract: "位移图采样 clamp" },
        DocRow { module: "vec388", op_name: "conv", contract: "3x3 核 clamp 边界" },
        DocRow { module: "vec389", op_name: "duo", contract: "亮度双端插值" },
        DocRow { module: "vec390", op_name: "noise", contract: "LCG 确定性" },
        DocRow { module: "vec391", op_name: "chain", contract: "编译校验短路" },
        DocRow { module: "vec392", op_name: "gpu-plan", contract: "tile 覆盖 uniform" },
        DocRow { module: "vec393", op_name: "cpu-block", contract: "行块一致可中断" },
        DocRow { module: "vec394", op_name: "pair", contract: "三路对拍" },
        DocRow { module: "vec395", op_name: "anim", contract: "关键帧钳制插值" },
        DocRow { module: "vec396", op_name: "4k", contract: "Q8.8 定点" },
        DocRow { module: "vec397", op_name: "budget", contract: "成本余量降级" },
    ]
}

/// 基准工作量：位图面积 × 类单价（纯计数，无时钟）
pub fn bench_workload(w: usize, h: usize, class: u32) -> u64 {
    total_ops(&FBitmap::new(w, h), class)
}

/// 基准执行：反相全图并统计已处理像素（工作量守恒断言用）
pub fn bench_run(w: usize, h: usize) -> u64 {
    let bm = FBitmap::solid(w, h, [1, 2, 3, 255]);
    let out = xform_blocked(&bm, 64, xf_invert);
    out.px.len() as u64
}

/// 基准登记进注册表（复用 vec381 注册表唯一性）
pub fn bench_register(reg: &mut FilterRegistry) -> Result<(), &'static str> {
    fn op_bench(b: &FBitmap, _p: &Params) -> Result<FBitmap, &'static str> {
        Ok(xform_blocked(b, 64, xf_invert))
    }
    reg.register("bench_invert", 1, op_bench)
}

pub fn run_vec398_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec398");
    // 族一：名册完整性
    let r = roster();
    s.add("文档-名册条数", r.len() == 17, "17 rows");
    s.add("文档-覆盖381", r.iter().any(|x| x.module == "vec381"), "covers base");
    s.add("文档-覆盖397", r.iter().any(|x| x.module == "vec397"), "covers budget");
    s.add("文档-契约非空", r.iter().all(|x| !x.contract.is_empty()), "contract text");
    s.add("文档-模块唯一", {
        let mut u = true;
        for i in 0..r.len() {
            for j in i + 1..r.len() {
                if r[i].module == r[j].module {
                    u = false;
                }
            }
        }
        u
    }, "module unique");
    // 族二：工作量守恒
    s.add("基准-工作量公式", bench_workload(10, 10, 0) == 400, "10x10 class0");
    s.add("基准-线性于面积", bench_workload(20, 10, 0) == 2 * bench_workload(10, 10, 0), "linear area");
    s.add("基准-类单价放大", bench_workload(10, 10, 2) > bench_workload(10, 10, 0), "class scale");
    // 族三：基准执行
    s.add("基准-像素守恒", bench_run(32, 32) == 32 * 32, "pixel count");
    s.add("基准-空图零", bench_run(0, 8) == 0, "empty");
    // 族四：基准注册
    let mut reg = FilterRegistry::new();
    s.add("基准-注册成功", bench_register(&mut reg).is_ok(), "register ok");
    s.add("基准-重名拒", bench_register(&mut reg) == Err("duplicate filter name"), "dup reject");
    let out = reg.lookup("bench_invert").map(|f| (f.op)(&FBitmap::solid(2, 2, [10, 10, 10, 255]), &Params::new()));
    s.add("基准-执行反相", out == Some(Ok(FBitmap::solid(2, 2, [245, 245, 245, 255]))), "exec invert");
    s
}
