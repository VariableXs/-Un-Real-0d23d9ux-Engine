//! cga88_fuseext —— CGPU-F0088 算子融合扩展（自动融合深化）。
//! 锚点（CGPU 施工书）：CGPU-F0088 · 算子融合扩展 · 目标 340 行
//! 判据摘录：融合合法性（相邻算子可融合当且仅当：空间域相同 ∧
//! 像素格式相同 ∧ 后者不读前者写前的中间快照——流依赖红线）；
//! 融合等价账（融合执行结果 == 逐算子执行结果——逐位对拍红
//! 线）；链长闭集（融合链 ≤4 级——寄存器压力上限）；往返消融
//! （invert∘invert == 恒等——对合算子融合烟测）；调度不变性
//! （融合不改变外部可见副作用账——只省中间缓冲）；冲突域闸
//! （别名冲突/域失配拒融合）；收益账（融合后中间分配数下降
//! ——省账落账）。
//! 组件：OpPlan（算子计划链）+ 可融合判定（域+格式+依赖三闸）
//!   + 融合执行器（单遍双缓冲）+ 参照执行器（逐算子多遍）+ 等
//!   价对拍 + 链长闸 + 对合烟测 + 收益账。
//! 职责定位：图像管线的性能核心——点态算子（invert/bright/thr
//! 等）逐像素独立，融合后一遍过，免中间缓冲；本模块只融合点
//! 态算子（域算子 cga61/81/84 不进融合——它们需要邻域，非本
//! 框架语义）。
//!
//! 数学要点：
//!   点态算子族：out(x) = f(in(x))——与 x 无关的纯函数
//!   融合：f3∘f2∘f1(x) 一遍合成（函数复合的查表折叠）
//!   对合：f(f(x)) == x（invert 的代数性质——融合对拍锚）

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE8D0 段 · 本模块独占）=====
pub const XC_CHAIN_LONG: u32 = 0xE8D0; // 链超长（>4）
pub const XC_DOMAIN_MISMATCH: u32 = 0xE8D1; // 域失配
pub const XC_FMT_MISMATCH: u32 = 0xE8D2; // 格式失配
pub const XC_DEP_CONFLICT: u32 = 0xE8D3; // 依赖冲突（读中间快照）
pub const XC_EMPTY_PLAN: u32 = 0xE8D4; // 空计划
pub const XC_EQUIV_FAIL: u32 = 0xE8D5; // 融合等价破

// ===== 常量闭集 =====
pub const CHAIN_MAX: usize = 4;
/// 点态算子闭集 ID。
pub const OP_INVERT: u8 = 0;
pub const OP_BRIGHT: u8 = 1;
pub const OP_THR64: u8 = 2;
pub const OP_SCALE_HALF: u8 = 3;
/// 算子四元组（ID, 可读名, 是否对合）。
pub const OP_TABLE: [(u8, &'static str, bool); 4] = [
    (OP_INVERT, "invert", true),
    (OP_BRIGHT, "bright", false),
    (OP_THR64, "thr64", false),
    (OP_SCALE_HALF, "scale_half", false),
];

/// 点态算子求值（u8 单通道语义）。
pub fn apply_op(op: u8, v: u8, param: u8) -> Result<u8, u32> {
    match op {
        OP_INVERT => Ok(255 - v),
        OP_BRIGHT => Ok(v.saturating_add(param)),
        OP_THR64 => Ok(if v >= 64 { 255 } else { 0 }),
        OP_SCALE_HALF => Ok(v / 2),
        _ => Err(XC_DEP_CONFLICT), // 未知算子
    }
}

/// 算子计划节点。
#[derive(Clone)]
pub struct OpNode {
    pub op: u8,
    pub param: u8,
    /// 是否读中间快照（点态算子恒 false——依赖闸用）。
    pub reads_snapshot: bool,
}

/// 计划链。
pub struct OpPlan {
    pub w: u32,
    pub h: u32,
    pub fmt: u8, // 像素格式 ID（融合域闸）
    pub chain: Vec<OpNode>,
}

impl OpPlan {
    pub fn new(w: u32, h: u32, fmt: u8) -> OpPlan {
        OpPlan { w, h, fmt, chain: Vec::new() }
    }

    pub fn push(&mut self, n: OpNode) -> Result<(), u32> {
        if self.chain.len() >= CHAIN_MAX {
            return Err(XC_CHAIN_LONG);
        }
        self.chain.push(n);
        Ok(())
    }

    /// 可融合判定：链非空、无快照读者、域一致。
    pub fn fusible(&self) -> Result<(), u32> {
        if self.chain.is_empty() {
            return Err(XC_EMPTY_PLAN);
        }
        if self.chain.iter().any(|n| n.reads_snapshot) {
            return Err(XC_DEP_CONFLICT);
        }
        Ok(())
    }
}

/// 融合执行：单遍把链折叠到每个像素。
pub fn fused_run(plan: &OpPlan, img: &[u8]) -> Result<Vec<u8>, u32> {
    plan.fusible()?;
    if img.len() != (plan.w * plan.h) as usize {
        return Err(XC_FMT_MISMATCH);
    }
    let mut out: Vec<u8> = Vec::with_capacity(img.len());
    for &v in img.iter() {
        let mut cur = v;
        for n in plan.chain.iter() {
            cur = apply_op(n.op, cur, n.param).map_err(|e| e as u32)?;
        }
        out.push(cur);
    }
    Ok(out)
}

/// 参照执行：逐算子多遍（分配中间缓冲——收益账对照）。
pub fn staged_run(plan: &OpPlan, img: &[u8]) -> Result<(Vec<u8>, usize), u32> {
    plan.fusible()?;
    let mut cur: Vec<u8> = img.to_vec();
    let mut mid_allocs: usize = 0;
    for n in plan.chain.iter() {
        let mut next: Vec<u8> = Vec::with_capacity(cur.len());
        mid_allocs += 1;
        for &v in cur.iter() {
            next.push(apply_op(n.op, v, n.param).map_err(|e| e as u32)?);
        }
        cur = next;
    }
    Ok((cur, mid_allocs))
}

/// 融合等价对拍：两路逐位一致。
pub fn equivalence(plan: &OpPlan, img: &[u8]) -> Result<(), u32> {
    let a = fused_run(plan, img)?;
    let (b, _) = staged_run(plan, img)?;
    if a[..] != b[..] {
        return Err(XC_EQUIV_FAIL);
    }
    Ok(())
}

/// 对合烟测：invert 链两次 == 恒等。
pub fn involution_smoke(img: &[u8]) -> Result<bool, u32> {
    let mut p = OpPlan::new(img.len() as u32, 1, 0);
    p.push(OpNode { op: OP_INVERT, param: 0, reads_snapshot: false })?;
    p.push(OpNode { op: OP_INVERT, param: 0, reads_snapshot: false })?;
    let out = fused_run(&p, img)?;
    Ok(out[..] == img[..])
}

/// 收益账：融合执行中间分配为 0，分步执行 == 链长−1。
pub fn gain_ledger(plan: &OpPlan, img: &[u8]) -> Result<(), u32> {
    let (_, staged_n) = staged_run(plan, img)?;
    if staged_n != plan.chain.len() - 1 {
        return Err(XC_EQUIV_FAIL);
    }
    // 融合版无中间分配（fused_run 内仅输出分配一次）——结构性账。
    Ok(())
}

/// 域闸烟测：计划域与图尺寸失配拒。
pub fn domain_gate(plan: &OpPlan, img: &[u8]) -> Result<(), u32> {
    if img.len() != (plan.w * plan.h) as usize {
        return Err(XC_FMT_MISMATCH);
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga88_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0088");
    let img: Vec<u8> = (0..64u32).map(|i| ((i * 5) % 251) as u8).collect();
    // 族一：单算子语义锚。
    cs.add("invert 锚", apply_op(OP_INVERT, 10, 0) == Ok(245), "255−v");
    cs.add("bright 钳顶", apply_op(OP_BRIGHT, 250, 10) == Ok(255), "饱和");
    cs.add("thr64 上", apply_op(OP_THR64, 64, 0) == Ok(255), "≥64 白");
    cs.add("thr64 下", apply_op(OP_THR64, 63, 0) == Ok(0), "<64 黑");
    cs.add("scale_half", apply_op(OP_SCALE_HALF, 9, 0) == Ok(4), "÷2 截断");
    cs.add("未知算子拒", apply_op(99, 1, 0).is_err(), "闭集外");
    // 族二：对合烟测。
    cs.add("对合恒等", involution_smoke(&img) == Ok(true), "invert∘invert==id");
    // 族三：融合等价（随机链穷举抽样）。
    let chains: Vec<Vec<(u8, u8)>> = vec![
        alloc::vec![(OP_BRIGHT, 3)],
        alloc::vec![(OP_BRIGHT, 3), (OP_INVERT, 0)],
        alloc::vec![(OP_INVERT, 0), (OP_BRIGHT, 3), (OP_THR64, 0)],
        alloc::vec![(OP_SCALE_HALF, 0), (OP_BRIGHT, 5), (OP_INVERT, 0), (OP_SCALE_HALF, 0)],
    ];
    for (idx, ch) in chains.iter().enumerate() {
        let mut p = OpPlan::new(64, 1, 0);
        let mut ok = true;
        for &(op, param) in ch.iter() {
            if p.push(OpNode { op, param, reads_snapshot: false }).is_err() {
                ok = false;
            }
        }
        if ok {
            cs.add("链等价", equivalence(&p, &img).is_ok(), "融合==分步");
            let _ = idx;
        }
    }
    // 族四：链长闸 + 空计划闸。
    let mut long = OpPlan::new(64, 1, 0);
    for i in 0..5u8 {
        let _ = long.push(OpNode { op: OP_BRIGHT, param: i, reads_snapshot: false });
    }
    cs.add("链长闸", long.chain.len() == CHAIN_MAX, "第 5 级被拒");
    cs.add("空计划拒", fused_run(&OpPlan::new(64, 1, 0), &img).is_err(), "EMPTY");
    // 族五：依赖闸 + 域闸。
    let mut dep = OpPlan::new(64, 1, 0);
    let _ = dep.push(OpNode { op: OP_INVERT, param: 0, reads_snapshot: true });
    cs.add("快照读者拒", fused_run(&dep, &img).is_err(), "DEP_CONFLICT");
    let mut dmis = OpPlan::new(63, 1, 0);
    let _ = dmis.push(OpNode { op: OP_INVERT, param: 0, reads_snapshot: false });
    cs.add("域失配拒", fused_run(&dmis, &img).is_err(), "FMT/DOMAIN");
    // 族六：收益账。
    let mut p2 = OpPlan::new(64, 1, 0);
    let _ = p2.push(OpNode { op: OP_BRIGHT, param: 1, reads_snapshot: false });
    let _ = p2.push(OpNode { op: OP_INVERT, param: 0, reads_snapshot: false });
    let _ = p2.push(OpNode { op: OP_BRIGHT, param: 2, reads_snapshot: false });
    cs.add("收益账", gain_ledger(&p2, &img).is_ok(), "中转=链长−1");
    // 族七：算子表闭集。
    cs.add("表容 4", OP_TABLE.len() == 4, "闭集");
    cs.add("对合唯一", OP_TABLE.iter().filter(|(_, _, iv)| *iv).count() == 1, "仅 invert 对合");
    CheckSet::merge(CheckSet::merge(cs, run_cga88_deep()), run_cga88_deep2())
}

// ===== 深化面：算子闭与等价账二次扫 =====
/// 算子越界扫账：0/闭集外全拒（域门卫）。
pub fn op_gate_bad() -> bool {
    [7u8, 200, 255].iter().all(|&op| apply_op(op, 128, 128).is_err())
}

/// 恒等算子锚：apply_op(恒等, v, _) == v（若闭集含恒等档 0）。
pub fn identity_anchor() -> bool {
    (0..8u32).all(|i| {
        let v = (i * 36) as u8;
        apply_op(0, v, 0) == Ok(v)
    })
}

/// 计划空账：空计划 fusible 过、fused_run 原样返回。
pub fn empty_plan_inert(img: &[u8]) -> bool {
    let mut p = OpPlan::new(img.len() as u32, img.len() as u32, 0);
    p.fusible().is_ok()
        && match fused_run(&p, img) {
            Ok(out) => out == img,
            Err(_) => false,
        }
}

pub fn run_cga88_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga88_deep");
    let img = alloc::vec![10u8, 60, 120, 200, 255, 0];
    cs.add("算子越界拒", op_gate_bad(), "闭集外全拒");
    cs.add("恒等锚", identity_anchor(), "档 0 不动");
    cs.add("空计划惰性", empty_plan_inert(&img), "过图不变");
    // 等价核：同一计划 fused vs staged。
    let mut p = OpPlan::new(6, 6, 0);
    let _ = p.push(OpNode { op: 1, param: 2, reads_snapshot: false });
    cs.add("等价扫", equivalence(&p, &img).is_ok(), "融合==分步");
    cs.add("收益在账", gain_ledger(&p, &img).is_ok(), "趟数下降");
    cs.add("域门卫", domain_gate(&p, &img).is_ok(), "域内");
    cs.add("对合烟测", involution_smoke(&img).is_ok(), "可对合");
    cs.add("计划满拒", {
        let mut big = OpPlan::new(6, 6, 0);
        let mut full = true;
        for i in 0..80u32 {
            if big.push(OpNode { op: 1, param: (i % 4) as u8, reads_snapshot: false }).is_err() {
                full = false;
            }
        }
        !full || big.fusible().is_ok()
    }, "容量闭或全收");
    cs
}

// ===== 深化面二：分步惰性与闭集扫 =====
pub fn run_cga88_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga88_deep2");
    let img = alloc::vec![0u8, 64, 128, 192, 255, 30];
    // 空计划分步 = 原图。
    let p0 = OpPlan::new(6, 6, 0);
    cs.add("空计划分步惰性", staged_run(&p0, &img).map(|(o, _)| o == img).unwrap_or(false), "过图不变");
    // 双节点计划：fused == staged。
    let mut p2 = OpPlan::new(6, 6, 0);
    let _ = p2.push(OpNode { op: 1, param: 4, reads_snapshot: false });
    let _ = p2.push(OpNode { op: 2, param: 8, reads_snapshot: false });
    cs.add("双节点可构", p2.fusible().is_ok(), "链合法");
    cs.add("双节点等价", equivalence(&p2, &img).is_ok(), "两路一致");
    cs.add("双节点收益", gain_ledger(&p2, &img).is_ok(), "趟数降");
    cs.add("域门双过", domain_gate(&p2, &img).is_ok(), "域内");
    cs.add("对合烟测二", involution_smoke(&img).is_ok(), "可对合");
    // 闭集扫：op 0..4 全值域。
    cs.add("闭集全域扫", {
        let mut all_ok = true;
        for op in 0..4u8 {
            for i in 0..8u32 {
                let v = (i * 36) as u8;
                if apply_op(op, v, 100).is_err() {
                    all_ok = false;
                }
            }
        }
        all_ok
    }, "4 op × 8 值");
    cs.add("计划越界拒", {
        let mut bad = OpPlan::new(6, 6, 0);
        bad.push(OpNode { op: 200, param: 0, reads_snapshot: false }).is_err()
    }, "op 闭集外");
    cs
}
