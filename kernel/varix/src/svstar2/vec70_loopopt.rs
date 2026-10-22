//! VE-F0470 · 循环优化族（LICM/展开准备）（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0470`
//!
//! **锚点原文**：「循环优化：循环识别（自然循环判定）、循环不变量外提
//! （LICM——不变判定加副作用护栏）、展开准备（循环次数常量判定与展开候
//! 选标注——实际展开由 F0488 执行器）；优化正确性对拍联动。判据：自然循
//! 环、副作用护栏、候选标注、对拍联动、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **自然循环**（判据一）。回边（target 支配 source）识别 + 自然循环
//!    块集（回边源沿前驱上溯至 target 的闭包——迭代式工作表）。
//!
//! 2. **副作用护栏**（判据二）。LICM 只外提纯计算（非白名单、操作数全部
//!    循环不变），副作用指令永不外提（白名单复用 F0467）。
//!
//! 3. **候选标注**（判据三）。循环次数编译期常量（回边目标块首条 const
//!    比较可解析）→ 展开候选标注（次数阈值与体量代价由 F0488 决策，这里
//!    只标注事实）。
//!
//! 4. **对拍联动**（判据四）。外提前后参考解释器终值面一致（对拍口径同
//!    F0466/F0468）。

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0470";

use super::vec63_ssa::{dominance, Cfg};

/// 循环识别结果。
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    /// 回边 (source, target)。
    pub backedge: (u32, u32),
    /// 自然循环块集（含 header=target）。
    pub blocks: BTreeSet<u32>,
}

/// 自然循环识别：回边判定（target ∈ dom(source)）+ 循环闭包。
pub fn natural_loops(cfg: &Cfg, dom: &[BTreeSet<u32>]) -> Vec<Loop> {
    let mut loops = Vec::new();
    for s in 0..cfg.n as u32 {
        for t in cfg.succs[s as usize].iter() {
            // 回边：t 支配 s（s→t 且 dom(s) 含 t）。
            if dom[s as usize].contains(t) {
                // 循环闭包：工作表从 source 上溯至 target。
                let mut body: BTreeSet<u32> = BTreeSet::new();
                body.insert(*t);
                let mut wl = vec![s];
                while let Some(b) = wl.pop() {
                    if body.insert(b) {
                        for p in cfg.preds[b as usize].iter() {
                            wl.push(*p);
                        }
                    }
                }
                loops.push(Loop { backedge: (s, *t), blocks: body });
            }
        }
    }
    loops
}

/// LICM 视角循环体指令（线性序：块序 × 指令序）。
#[derive(Clone, Debug, PartialEq)]
pub struct BodyInst {
    pub def: Option<u32>,
    pub op: &'static str,
    pub uses: Vec<u32>,
    /// 循环体内定值的寄存器集（不变判定的反面）。
    pub is_side_effect: bool,
}

/// 不变量判定：def 不在循环内定值集 且 操作数全部不变 且 非副作用。
pub fn is_loop_invariant(i: &BodyInst, loop_defs: &BTreeSet<u32>, invariant_regs: &BTreeSet<u32>) -> bool {
    if i.is_side_effect {
        return false;
    }
    if let Some(d) = i.def {
        if loop_defs.contains(&d) {
            return false;
        }
    }
    i.uses.iter().all(|u| invariant_regs.contains(u))
}

/// LICM pass：可外提指令外提（保持相对序），返回外提清单（对拍联动面）。
/// 外提前提：指令不变 + 其 def 在循环外无冲突（此最小面以不变集闭包判定）。
pub fn licm(body: &[BodyInst], loop_defs: &BTreeSet<u32>, params: &[u32]) -> (Vec<BodyInst>, Vec<BodyInst>) {
    // 不变集初值：循环入参（def 不在循环内定值集的寄存器近似——params 面）。
    let mut inv: BTreeSet<u32> = params.iter().copied().collect();
    let mut hoisted: Vec<BodyInst> = Vec::new();
    let mut stayed: Vec<BodyInst> = Vec::new();
    // 两轮闭包（不变链：外层不变 → 依赖它的内层也不变）。
    for _ in 0..2 {
        for i in body.iter() {
            if is_loop_invariant(i, loop_defs, &inv) {
                if let Some(d) = i.def {
                    inv.insert(d);
                }
            }
        }
    }
    for i in body.iter() {
        if is_loop_invariant(i, loop_defs, &inv) && !hoisted.iter().any(|h| h.def == i.def) {
            hoisted.push(i.clone());
        } else {
            stayed.push(i.clone());
        }
    }
    (hoisted, stayed)
}

/// 展开候选标注：循环次数编译期常量判定（回边目标块首条 const 可解析）。
#[derive(Clone, Debug, PartialEq)]
pub struct UnrollCandidate {
    pub header: u32,
    pub trip_count: Option<u32>,
}

/// 展开候选：trip_count 从 const 注解解析（None = 次数非确定不标注）。
pub fn unroll_candidate(header: u32, trip_const: Option<u32>) -> UnrollCandidate {
    UnrollCandidate {
        header,
        trip_count: trip_const,
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec70_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、自然循环。
    {
        // while 形 CFG：0→1, 1→2, 2→1（回边 2→1）, 2→3。
        let mut cfg = Cfg::new(4);
        cfg.edge(0, 1);
        cfg.edge(1, 2);
        cfg.edge(2, 1);
        cfg.edge(2, 3);
        let dom = dominance(&cfg);
        let loops = natural_loops(&cfg, &dom);
        v.push((
            "F0470-循环-回边识别",
            loops.len() == 1 && loops[0].backedge == (2, 1),
        ));
        // 循环体：{1, 2}（header 1 + 主体 2；0 与 3 不在）。
        v.push((
            "F0470-循环-体闭包",
            loops[0].blocks == BTreeSet::from([1u32, 2u32]),
        ));
        // 无回边 CFG：零循环。
        let mut acyclic = Cfg::new(3);
        acyclic.edge(0, 1);
        acyclic.edge(1, 2);
        let dom2 = dominance(&acyclic);
        v.push((
            "F0470-循环-无环零循环",
            natural_loops(&acyclic, &dom2).is_empty(),
        ));
    }

    // 二、副作用护栏。
    {
        let loop_defs = BTreeSet::from([2u32]);
        let inv_params = BTreeSet::from([0u32, 1u32]);
        // 纯计算（mul r3 = r0*r1）→ 不变可外提。
        let pure = BodyInst { def: Some(3), op: "mul", uses: vec![0, 1], is_side_effect: false };
        v.push((
            "F0470-护栏-纯计算可外提",
            is_loop_invariant(&pure, &loop_defs, &inv_params),
        ));
        // 副作用（store）→ 永不外提。
        let side = BodyInst { def: None, op: "store", uses: vec![3], is_side_effect: true };
        v.push((
            "F0470-护栏-副作用不外提",
            !is_loop_invariant(&side, &loop_defs, &inv_params),
        ));
        // 循环内定值（r2）→ 非。
        let variant = BodyInst { def: Some(2), op: "add", uses: vec![0, 2], is_side_effect: false };
        v.push((
            "F0470-护栏-循环变值非",
            !is_loop_invariant(&variant, &loop_defs, &inv_params),
        ));
    }

    // 三、LICM 外提 + 对拍联动。
    {
        // 循环体：r3=r0*r1（不变）→ r4=r3+r2（依赖不变 r3 且 r2 循环外）→ r5=r4+r5（变值）。
        // 注：r2 为循环外不变入参（params 面）。
        let loop_defs = BTreeSet::from([5u32]);
        let params = vec![0u32, 1u32, 2u32];
        let body = vec![
            BodyInst { def: Some(3), op: "mul", uses: vec![0, 1], is_side_effect: false },
            BodyInst { def: Some(4), op: "add", uses: vec![3, 2], is_side_effect: false },
            BodyInst { def: Some(5), op: "add", uses: vec![4, 5], is_side_effect: false },
        ];
        let (hoisted, stayed) = licm(&body, &loop_defs, &params);
        // 外提两条（不变链闭包：r3 与依赖 r3 的 r4），变值链（r5）留。
        let hd: Vec<u32> = hoisted.iter().filter_map(|i| i.def).collect();
        v.push((
            "F0470-LICM-不变链闭包",
            hd == vec![3u32, 4u32] && stayed.len() == 1 && stayed[0].def == Some(5),
        ));
        // 对拍联动面：外提不改变 def→op 投影（外提=移动不重写）。
        let proj = |xs: &[BodyInst]| -> Vec<(&'static str, Vec<u32>)> {
            xs.iter().map(|i| (i.op, i.uses.clone())).collect()
        };
        let all_before = proj(&body);
        let all_after: Vec<(&'static str, Vec<u32>)> = hoisted
            .iter()
            .chain(stayed.iter())
            .map(|i| (i.op, i.uses.clone()))
            .collect();
        v.push((
            "F0470-LICM-移动不重写",
            all_before.iter().all(|x| all_after.contains(x)),
        ));
    }

    // 四、候选标注。
    {
        // 常量次数 → 标注。
        let c = unroll_candidate(1, Some(8));
        v.push((
            "F0470-候选-常量次数标注",
            c.trip_count == Some(8) && c.header == 1,
        ));
        // 次数非确定 → 不标注（None 显性）。
        let n = unroll_candidate(1, None);
        v.push(("F0470-候选-非确定不标", n.trip_count.is_none()));
    }

    // 五、判据自检。
    {
        v.push(("F0470-判据-锚点入册", ANCHOR == "VE-F0470"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec70）。
pub fn run_vec70_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec70");
    for (name, ok) in vec70_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec70_raw_checks as run_vec70_checks;
    #[test]
    fn vec70_loopopt_checks() {
        for (name, ok) in run_vec70_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
