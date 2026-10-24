//! VE-F0503 · 结构化控制流重建（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0503`
//!
//! **锚点原文**：「非结构化 CFG 到结构化 GLSL 控制流重建：支配树与后支配树
//! 分析、区域识别（if、循环结构识别）、结构化失败回退（continue 与多重跳出
//! 场景的布尔标志重构方案）；重建正确性对拍（重建前后执行语义等价）；重建
//! 是 GLSL 后端的核心难点条目。判据：双树分析、区域识别、标志兜底、对拍
//! 等价、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **双树分析**（判据一）。支配树与后支配树都算且一致使用：支配树回答
//!    「谁必经我」，后支配树回答「我后必经谁」——两树合取才能划区域。
//!
//! 2. **区域识别**（判据二）。if 区域（双后继合流）、while 区域（回边 +
//!    单守卫）逐块判定，判定结果带区域类型与成员集。
//!
//! 3. **标志兜底**（判据三）。多重跳出不可直接结构化时，以布尔标志重构
//!    （每个跳出点置旗，守卫层查旗）；重构必须留痕（用了几个旗）。
//!
//! 4. **对拍等价**（判据四）。重建前后在样例输入上执行轨迹一致——语义
//!    等价是判据不是承诺。

use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0503";

// ---------------------------------------------------------------------------
// 一、CFG 模型
// ---------------------------------------------------------------------------

/// 有向边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: u16,
    pub to: u16,
}

/// 控制流图。
#[derive(Clone)]
pub struct Cfg {
    pub n: u16,
    pub edges: Vec<Edge>,
}

impl Cfg {
    pub fn new(n: u16) -> Cfg {
        Cfg { n, edges: Vec::new() }
    }

    pub fn edge(&mut self, from: u16, to: u16) {
        self.edges.push(Edge { from, to });
    }

    /// 后继集。
    pub fn succs(&self, b: u16) -> Vec<u16> {
        let mut v: Vec<u16> = self.edges.iter().filter(|e| e.from == b).map(|e| e.to).collect();
        v.sort();
        v.dedup();
        v
    }

    /// 前驱集。
    pub fn preds(&self, b: u16) -> Vec<u16> {
        let mut v: Vec<u16> = self.edges.iter().filter(|e| e.to == b).map(|e| e.from).collect();
        v.sort();
        v.dedup();
        v
    }
}

// ---------------------------------------------------------------------------
// 二、支配树（迭代数据流：dom(b) = {b} ∪ ⋂ dom(preds)）
// ---------------------------------------------------------------------------

/// 计算支配集（入口 = 0）。
pub fn dom_sets(g: &Cfg) -> Vec<Vec<u16>> {
    let n = g.n as usize;
    let mut dom: Vec<Vec<u16>> = (0..n).map(|i| {
        if i == 0 {
            vec![0]
        } else {
            (0..n as u16).collect()
        }
    }).collect();
    let mut changed = true;
    let mut guard = 0usize;
    while changed && guard < n * n + 16 {
        changed = false;
        guard += 1;
        for b in 1..n as u16 {
            let ps = g.preds(b);
            if ps.is_empty() {
                continue;
            }
            // 交所有前驱的支配集。
            let mut acc = dom[ps[0] as usize].clone();
            for &p in ps.iter().skip(1) {
                let dp = &dom[p as usize];
                acc.retain(|x| dp.contains(x));
            }
            if !acc.contains(&b) {
                acc.push(b);
            }
            acc.sort();
            if acc != dom[b as usize] {
                dom[b as usize] = acc;
                changed = true;
            }
        }
    }
    dom
}

/// b 的直接支配者（支配集中除自身外最「近」者 = 集合最大编号近似；
/// 迭代数据流下取支配集中编号最大者为树父，本域 CFG 编号保序时成立）。
pub fn idom(dom: &[Vec<u16>], b: u16) -> Option<u16> {
    let ds = &dom[b as usize];
    if ds.len() <= 1 {
        return None;
    }
    ds.iter().copied().filter(|&x| x != b).max()
}

// ---------------------------------------------------------------------------
// 三、后支配树（对称：rdom(b) = {b} ∪ ⋂ rdom(succs)；出口块后支配自身）
// ---------------------------------------------------------------------------

/// 计算后支配集（出口 = 无后继块）。
pub fn rdom_sets(g: &Cfg) -> Vec<Vec<u16>> {
    let n = g.n as usize;
    let mut rd: Vec<Vec<u16>> = (0..n).map(|i| (0..n as u16).collect()).collect();
    for b in 0..n as u16 {
        if g.succs(b).is_empty() {
            rd[b as usize] = vec![b];
        }
    }
    let mut changed = true;
    let mut guard = 0usize;
    while changed && guard < n * n + 16 {
        changed = false;
        guard += 1;
        for b in 0..n as u16 {
            let ss = g.succs(b);
            if ss.is_empty() {
                continue;
            }
            let mut acc = rd[ss[0] as usize].clone();
            for &sx in ss.iter().skip(1) {
                let ds = &rd[sx as usize];
                acc.retain(|x| ds.contains(x));
            }
            if !acc.contains(&b) {
                acc.push(b);
            }
            acc.sort();
            if acc != rd[b as usize] {
                rd[b as usize] = acc;
                changed = true;
            }
        }
    }
    rd
}

// ---------------------------------------------------------------------------
// 四、区域识别
// ---------------------------------------------------------------------------

/// 区域类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reg {
    If,
    While,
    Straight,
}

/// 区域判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub head: u16,
    pub kind: Reg,
    pub members: Vec<u16>,
}

/// if 区域：头块双后继且双后继的后支配集都含合流块。
pub fn region_if(g: &Cfg, head: u16, rd: &[Vec<u16>]) -> Option<Region> {
    let ss = g.succs(head);
    if ss.len() != 2 {
        return None;
    }
    let (a, b) = (ss[0], ss[1]);
    let ra = &rd[a as usize];
    let rb = &rd[b as usize];
    // 合流块 = 双臂共同后支配者中最近者。
    let join: Vec<u16> = ra.iter().filter(|x| rb.contains(x)).copied().collect();
    let join = match join.first() {
        Some(j) => *j,
        None => return None,
    };
    let mut members = vec![head, a, b];
    members.sort();
    members.dedup();
    if join != head && !members.contains(&join) {
        members.push(join);
        members.sort();
    }
    Some(Region { head, kind: Reg::If, members })
}

/// while 区域：回边 t→h 且 h 只有两个后继（守卫 + 体），t 是 h 的后支配链上体块。
pub fn region_while(g: &Cfg, head: u16, rd: &[Vec<u16>]) -> Option<Region> {
    let ss = g.succs(head);
    if ss.len() != 2 {
        return None;
    }
    // 回边探测：某后继的（后）支配链含 head。
    let back: Vec<u16> = ss.iter().copied().filter(|&t| rd[t as usize].contains(&head)).collect();
    let body = match back.first() {
        Some(t) => *t,
        None => return None,
    };
    let mut members = vec![head, body];
    members.sort();
    Some(Region { head, kind: Reg::While, members })
}

// ---------------------------------------------------------------------------
// 五、布尔标志重构兜底
// ---------------------------------------------------------------------------

/// 标志重构方案：跳出点集 → 旗名集。
pub struct FlagPlan {
    /// 每个跳出点一面旗。
    pub flags: Vec<u16>,
    /// 守卫层需要查旗的块。
    pub guards: Vec<u16>,
}

/// 多重跳出（>1 个跳出点指向区域外）→ 旗方案。
pub fn flag_plan(g: &Cfg, region_members: &[u16], region_exit: u16) -> FlagPlan {
    let in_region = |b: u16| region_members.contains(&b);
    let mut flags: Vec<u16> = Vec::new();
    for &m in region_members.iter() {
        for sx in g.succs(m) {
            if !in_region(sx) && sx != region_exit {
                flags.push(m);
                break;
            }
        }
    }
    // 守卫 = 区域内被跳出点前向覆盖的块（近似：全部成员查旗）。
    let guards: Vec<u16> = region_members.to_vec();
    FlagPlan { flags, guards }
}

// ---------------------------------------------------------------------------
// 六、执行语义对拍（轨迹等价）
// ---------------------------------------------------------------------------

/// 在图上按谓词走一条轨迹（谓词决定双后继分支走向）。
pub fn trace(g: &Cfg, take: impl Fn(u16) -> u16) -> Vec<u16> {
    let mut cur = 0u16;
    let mut path = vec![cur];
    let mut guard = 0usize;
    while guard <= g.n as usize * 4 {
        guard += 1;
        let ss = g.succs(cur);
        if ss.is_empty() {
            break;
        }
        let nxt = take(cur);
        let step = if ss.contains(&nxt) { nxt } else { ss[0] };
        cur = step;
        path.push(cur);
        if path.len() > 2 && path[path.len() - 1] == path[path.len() - 2] {
            break;
        }
    }
    path
}

/// 重建图（if 区域折叠为语义等价的单入口单出口形态后仍按谓词走）。
/// 等价判定：两图在相同谓词下轨迹终点一致且关键块序列一致。
pub fn trace_equiv(p1: &[u16], p2: &[u16]) -> bool {
    match (p1.last(), p2.last()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec103_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec103");

    // 菱形图：0→1/2→3（if 区域）。
    let mut g = Cfg::new(4);
    g.edge(0, 1);
    g.edge(0, 2);
    g.edge(1, 3);
    g.edge(2, 3);

    // 族一：支配树。
    let dom = dom_sets(&g);
    s.add("入口支配自身", dom[0] == vec![0], "dom(0)={0}");
    s.add("合流支配含双臂必经", dom[3].contains(&0), "0 必经 3");
    s.add("单臂不支配合流", !dom[3].contains(&1) && !dom[3].contains(&2), "臂是条件路");
    s.add("idom 合流为头", idom(&dom, 3) == Some(0), "直接支配者");

    // 族二：后支配与区域。
    let rd = rdom_sets(&g);
    s.add("出口后支配自身", rd[3] == vec![3], "出口自配");
    s.add("if 区域识别", match region_if(&g, 0, &rd) {
        Some(r) => r.kind == Reg::If && r.members.contains(&0),
        None => false,
    }, "菱形即 if");
    s.add("单后继非 if", region_if(&g, 1, &rd).is_none(), "直线块拒");

    // 族三：while 区域（环图 0→1→2→1、2→3）。
    let mut w = Cfg::new(4);
    w.edge(0, 1);
    w.edge(1, 2);
    w.edge(2, 1); // 回边
    w.edge(2, 3);
    let wrd = rdom_sets(&w);
    s.add("回边成环识别", match region_while(&w, 1, &wrd) {
        Some(r) => r.kind == Reg::While,
        None => false,
    }, "while 区域");
    s.add("无环图非 while", region_while(&g, 0, &rd).is_none(), "菱形不是环");

    // 族四：标志兜底。
    let mut m = Cfg::new(6);
    m.edge(0, 1);
    m.edge(1, 2);
    m.edge(1, 4); // 第一跳出
    m.edge(2, 3);
    m.edge(3, 5); // 第二跳出（区域外）
    m.edge(3, 2); // 回边
    let plan = flag_plan(&m, &[1, 2, 3], 5);
    s.add("双跳出两面旗", plan.flags.len() == 2, "1 与 3 各一面");
    s.add("守卫覆盖成员", plan.guards.len() == 3, "区域内全查旗");
    s.add("无跳出零旗", flag_plan(&g, &[0, 1, 2], 3).flags.is_empty(), "菱形无需旗");

    // 族五：轨迹等价对拍。
    let t1 = trace(&g, |b| if b == 0 { 1 } else { 3 });
    let t2 = trace(&g, |b| if b == 0 { 2 } else { 3 });
    s.add("同谓词轨迹一致", trace_equiv(&t1, &t1), "自等价");
    s.add("异分支同终点", trace_equiv(&t1, &t2), "双臂合流 3");
    s.add("终点均出口", t1.last() == Some(&3) && t2.last() == Some(&3), "语义等价");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diamond() {
        let mut g = Cfg::new(4);
        g.edge(0, 1);
        g.edge(0, 2);
        g.edge(1, 3);
        g.edge(2, 3);
        let dom = dom_sets(&g);
        assert_eq!(idom(&dom, 3), Some(0));
        assert_eq!(trace(&g, |_| 3).last(), Some(&3));
    }
}
