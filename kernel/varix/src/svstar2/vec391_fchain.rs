//! VE-F0791 滤镜链
//! 职责：把「滤镜名 + 参数」序列编译为算子执行计划（plan），
//!       顺序执行 + 编译期校验（名字未录 / 参数非法即拒）。
//! 契约：链为空 → 恒等；执行短路报错（首个失败即停）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, FilterRegistry, Params, PixelOp};

/// 链节点：滤镜名 + 参数
#[derive(Clone, Debug, PartialEq)]
pub struct ChainNode {
    pub name: &'static str,
    pub params: Params,
}

impl ChainNode {
    pub fn of(name: &'static str) -> ChainNode {
        ChainNode { name, params: Params::new() }
    }
    pub fn with(name: &'static str, k: &'static str, v: f64) -> ChainNode {
        let mut p = Params::new();
        p.set(k, v);
        ChainNode { name, params: p }
    }
}

/// 编译后的执行计划
#[derive(Clone, Debug, Default)]
pub struct ChainPlan {
    pub steps: Vec<(&'static str, PixelOp, Params)>,
}

/// 编译：把名字解析为算子；未录名即拒
pub fn compile_chain(nodes: &[ChainNode], reg: &FilterRegistry) -> Result<ChainPlan, &'static str> {
    let mut steps = alloc::vec![];
    for n in nodes {
        let f = reg.lookup(n.name).ok_or("unknown filter")?;
        steps.push((n.name, f.op, n.params.clone()));
    }
    Ok(ChainPlan { steps })
}

/// 执行：顺序应用，短路报错
pub fn exec_chain(b: &FBitmap, plan: &ChainPlan) -> Result<FBitmap, &'static str> {
    let mut cur = b.clone();
    for (_name, op, p) in &plan.steps {
        cur = op(&cur, p)?;
    }
    Ok(cur)
}

pub fn run_vec391_checks() -> crate::checks::CheckSet {
    use super::vec381_fbase::{FBitmap as FB, op_identity};
    let mut s = CheckSet::new("svstar2-vec391");
    // 构造注册表：identity + brightness（vec385 亮度，此处以局部算子替代避免跨模块参数耦合）
    fn op_bright(b: &FB, p: &Params) -> Result<FB, &'static str> {
        let v = p.get_or("value", 0.0);
        let mut out = FB::new(b.w, b.h);
        for y in 0..b.h {
            for x in 0..b.w {
                let px = b.get(x, y);
                let c = |u: u8| (u as i32 + v as i32).clamp(0, 255) as u8;
                out.set(x, y, [c(px[0]), c(px[1]), c(px[2]), px[3]]);
            }
        }
        Ok(out)
    }
    let mut reg = FilterRegistry::new();
    reg.register("identity", 1, op_identity).expect("reg id");
    reg.register("bright", 1, op_bright).expect("reg bright");
    // 族一：空链恒等
    let empty = compile_chain(&[], &reg).expect("empty");
    let src = FBitmap::solid(2, 2, [50, 60, 70, 255]);
    s.add("链-空链恒等", exec_chain(&src, &empty).expect("e").px == src.px, "empty chain");
    // 族二：单步执行
    let one = compile_chain(&[ChainNode::with("bright", "value", 10.0)], &reg).expect("one");
    let o1 = exec_chain(&src, &one).expect("x");
    s.add("链-单步加亮", o1.get(0, 0) == [60, 70, 80, 255], "single step");
    // 族三：多步叠加（+10 再 +15）
    let two = compile_chain(&[
        ChainNode::with("bright", "value", 10.0),
        ChainNode::with("bright", "value", 15.0),
    ], &reg).expect("two");
    let o2 = exec_chain(&src, &two).expect("x2");
    s.add("链-多步叠加", o2.get(0, 0) == [75, 85, 95, 255], "chained");
    // 族四：恒等节点穿插不影响
    let mixed = compile_chain(&[
        ChainNode::of("identity"),
        ChainNode::with("bright", "value", 10.0),
        ChainNode::of("identity"),
    ], &reg).expect("mix");
    s.add("链-恒等穿插", exec_chain(&src, &mixed).expect("m").px == o1.px, "identity pass");
    // 族五：未知滤镜拒
    let bad = compile_chain(&[ChainNode::of("ghost")], &reg);
    s.add("链-未知名拒", bad.err() == Some("unknown filter"), "unknown reject");
    // 族六：编译期短路（第一个未知即停）
    let multi_bad = compile_chain(&[
        ChainNode::of("identity"),
        ChainNode::of("ghost"),
        ChainNode::of("phantom"),
    ], &reg);
    s.add("链-编译短路", multi_bad.err() == Some("unknown filter"), "compile short");
    // 族七：计划步数
    s.add("链-计划步数", two.steps.len() == 2, "plan len");
    s.add("链-空计划", empty.steps.is_empty(), "empty plan");
    // 族八：执行报错透传（构造恒报错算子）
    fn op_err(_: &FB, _: &Params) -> Result<FB, &'static str> {
        Err("boom")
    }
    reg.register("err", 1, op_err).expect("reg err");
    let eb = compile_chain(&[ChainNode::of("err")], &reg).expect("eb");
    s.add("链-执行报错", exec_chain(&src, &eb) == Err("boom"), "exec error");
    // 族九：饱和夹取链
    let sat = compile_chain(&[ChainNode::with("bright", "value", 255.0)], &reg).expect("sat");
    let os = exec_chain(&src, &sat).expect("sx");
    s.add("链-饱和夹取", os.get(0, 0) == [255, 255, 255, 255], "saturate");
    s
}
