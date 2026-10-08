//! CGPU-F0163 自检 · 拓扑排序与关键路径（CGPU-B 域）
//!
//! **锚点判据逐条对应**（`#CGPU-F0163`「排序正确性（有向无环验证）、关键
//! 路径与理论对拍、并行发射吞吐、环检测（有环即报错含环路径）、万节点
//! 性能」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 排序正确性 | `CB03-排序-*`（线性链/菱形/无波乱序不变量/覆盖守恒） |
//! | 关键路径与理论对拍 | `CB03-关键-*`（判据侧独立手算最长链逐条对账） |
//! | 并行发射吞吐 | `CB03-吞吐-*`（星形波宽/独立全并一波/波数=关键深度） |
//! | 环检测含环路径 | `CB03-环-*`（有环报错+环清单精确+无环空表双向） |
//! | 万节点性能 | `CB03-性能-*`（万链/万星/万环三语料账面守恒） |
//! | 衔接与判据元 | `CB03-衔接-*`/`CB03-判据-*`（F0162 产物直驱同计划） |
//!
//! **判据设计硬规矩**：期望值判据侧独立手算（最长链预算自己算不借实现）；
//! 不变量两头都测（无环空表+有环精确清单）；判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::vcb01_framegraph as fg;
use crate::svstar2::vcb02_depderive as dd;
use crate::svstar2::vcb03_topopath as tp;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

fn spec(kind: fg::NodeKind, budget: u32) -> fg::NodeSpec {
    fg::NodeSpec {
        kind,
        reads: vec![],
        writes: vec![],
        budget_us: budget,
        priority: 0,
        deadline_us: 0,
        queue: fg::QueueKind::Graphics,
    }
}

fn node(b: &mut fg::FrameGraphBuilder, kind: fg::NodeKind, budget: u32) -> u32 {
    b.add_node(spec(kind, budget)).unwrap_or(u32::MAX)
}

fn edge(from: u32, to: u32) -> fg::Edge {
    fg::Edge { from, to, kind: fg::EdgeKind::ReadAfterWrite }
}

/// 判据侧独立手算最长链（不借实现——理论对拍口径）。
fn hand_sum(g: &fg::FrameGraph, path: &[u32]) -> u64 {
    path.iter()
        .map(|&h| g.get_node(h).map_or(0u64, |s| s.budget_us as u64))
        .sum()
}

fn wave_index(p: &tp::LaunchPlan, h: u32) -> Option<usize> {
    p.waves.iter().position(|w| w.contains(&h))
}

// ---------------------------------------------------------------------------
// 组一：排序正确性（有向无环验证）
// ---------------------------------------------------------------------------

fn chk_order(s: &mut CheckSet) {
    // CB03-排序-01：线性链 A→B→C 波次序恰为 [A],[B],[C]。
    let mut b = fg::FrameGraphBuilder::for_frame(1);
    let a = node(&mut b, fg::NodeKind::Upload, 100);
    let bb = node(&mut b, fg::NodeKind::Compute, 200);
    let c = node(&mut b, fg::NodeKind::Present, 300);
    let _ = b.add_edge(edge(a, bb));
    let _ = b.add_edge(edge(bb, c));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-排序-01", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-排序-01", false, "无环图被拒");
            return;
        }
    };
    let ok = p.waves == vec![vec![a], vec![bb], vec![c]]
        && p.wave_count() == 3
        && p.total_nodes() == 3;
    s.add("CB03-排序-01", ok, "线性链三波序正确且覆盖守恒");

    // CB03-排序-02：菱形 A→{B,C}→D 中波恰 [B,C]（并行集）。
    let mut b = fg::FrameGraphBuilder::for_frame(2);
    let a = node(&mut b, fg::NodeKind::Upload, 10);
    let l = node(&mut b, fg::NodeKind::Compute, 10);
    let r = node(&mut b, fg::NodeKind::Compute, 10);
    let d = node(&mut b, fg::NodeKind::Present, 10);
    let _ = b.add_edge(edge(a, l));
    let _ = b.add_edge(edge(a, r));
    let _ = b.add_edge(edge(l, d));
    let _ = b.add_edge(edge(r, d));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-排序-02", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-排序-02", false, "无环图被拒");
            return;
        }
    };
    let mid_ok = p.waves.len() == 3
        && p.waves[0] == vec![a]
        && p.waves[1].len() == 2
        && p.waves[1].contains(&l)
        && p.waves[1].contains(&r)
        && p.waves[2] == vec![d];
    s.add("CB03-排序-02", mid_ok, "菱形中波是并行集且首尾波正确");

    // CB03-排序-03：无边双节点同波（互不依赖即可全并行）。
    let mut b = fg::FrameGraphBuilder::for_frame(3);
    let x = node(&mut b, fg::NodeKind::Copy, 10);
    let y = node(&mut b, fg::NodeKind::Compose, 10);
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-排序-03", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-排序-03", false, "无环图被拒");
            return;
        }
    };
    let ok = p.wave_count() == 1 && p.waves[0].len() == 2 && p.max_parallel() == 2;
    s.add("CB03-排序-03", ok, "独立节点同一波全并行发射");

    // CB03-排序-04：跨边波次不变量——任一后继波号严格大于前驱。
    // （排序正确性的普适断言：对组一全语料逐边对账。）
    let mut ok = true;
    {
        // 语料 1：线性链（重放图）。
        let mut b = fg::FrameGraphBuilder::for_frame(11);
        let a = node(&mut b, fg::NodeKind::Upload, 100);
        let bb = node(&mut b, fg::NodeKind::Compute, 200);
        let c = node(&mut b, fg::NodeKind::Present, 300);
        let _ = b.add_edge(edge(a, bb));
        let _ = b.add_edge(edge(bb, c));
        if let Ok(g) = b.into_snapshot() {
            if let Ok(p) = tp::plan(&g) {
                for e in g.edges_of().iter() {
                    let wf = wave_index(&p, e.from).unwrap_or(usize::MAX);
                    let wt = wave_index(&p, e.to).unwrap_or(0);
                    if wf >= wt {
                        ok = false;
                    }
                }
            } else {
                ok = false;
            }
        } else {
            ok = false;
        }
    }
    s.add("CB03-排序-04", ok, "跨边波次号后继严格递增（链语料）");

    // CB03-排序-05：菱形语料同款跨边不变量。
    let mut b = fg::FrameGraphBuilder::for_frame(12);
    let a = node(&mut b, fg::NodeKind::Upload, 100);
    let l = node(&mut b, fg::NodeKind::Compute, 500);
    let r = node(&mut b, fg::NodeKind::Compute, 50);
    let d = node(&mut b, fg::NodeKind::Present, 100);
    let _ = b.add_edge(edge(a, l));
    let _ = b.add_edge(edge(a, r));
    let _ = b.add_edge(edge(l, d));
    let _ = b.add_edge(edge(r, d));
    let mut ok = false;
    if let Ok(g) = b.into_snapshot() {
        if let Ok(p) = tp::plan(&g) {
            ok = true;
            for e in g.edges_of().iter() {
                let wf = wave_index(&p, e.from).unwrap_or(usize::MAX);
                let wt = wave_index(&p, e.to).unwrap_or(0);
                if wf >= wt {
                    ok = false;
                }
            }
        }
    }
    s.add("CB03-排序-05", ok, "跨边波次号后继严格递增（菱形语料）");

    // CB03-排序-06：单节点图单波且关键路径=自身。
    let mut b = fg::FrameGraphBuilder::for_frame(13);
    let only = node(&mut b, fg::NodeKind::Graphics, 777);
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-排序-06", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-排序-06", false, "单节点图被拒");
            return;
        }
    };
    let ok = p.waves == vec![vec![only]]
        && p.critical_path == vec![only]
        && p.critical_total_us == 777
        && p.is_critical(only);
    s.add("CB03-排序-06", ok, "单节点图单波且关键路径=自身全预算");
}

// ---------------------------------------------------------------------------
// 组二：空图与环检测（有环即报错含环路径）
// ---------------------------------------------------------------------------

fn chk_cycle(s: &mut CheckSet) {
    // CB03-环-01：空图两级显性拒——vcb01 快照层 E_POOL_EMPTY 拦截为常态，
    //  本域 E_TOPO_EMPTY 是 plan 的二级防御；两级任一触发即绿（两头都测）。
    let mut b = fg::FrameGraphBuilder::for_frame(21);
    let ok = match b.into_snapshot() {
        Ok(g) => tp::plan(&g) == Err(tp::E_TOPO_EMPTY),
        Err(e) => e == fg::E_POOL_EMPTY,
    };
    s.add("CB03-环-01", ok, "空图显性拒绝（快照层或 plan 层）");

    // CB03-环-02：三节点环 plan 拒绝 E_GRAPH_CYCLE。
    let mut b = fg::FrameGraphBuilder::for_frame(22);
    let a = node(&mut b, fg::NodeKind::Compute, 10);
    let bb = node(&mut b, fg::NodeKind::Compute, 10);
    let c = node(&mut b, fg::NodeKind::Compute, 10);
    let _ = b.add_edge(edge(a, bb));
    let _ = b.add_edge(edge(bb, c));
    let _ = b.add_edge(edge(c, a));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-环-02", false, "图构造失败");
            return;
        }
    };
    let ok = tp::plan(&g) == Err(tp::E_GRAPH_CYCLE);
    s.add("CB03-环-02", ok, "有环图 plan 返回 E_GRAPH_CYCLE");

    // CB03-环-03：环清单恰为环上三节点且升序（可定位取证）。
    let mut cy = tp::cycle_nodes(&g);
    cy.sort();
    let ok = cy == vec![a, bb, c];
    s.add("CB03-环-03", ok, "cycle_nodes 精确列出环上全部节点");

    // CB03-环-04：环外节点不入清单（环图再加孤立 D）。
    let mut b = fg::FrameGraphBuilder::for_frame(23);
    let a = node(&mut b, fg::NodeKind::Compute, 10);
    let bb = node(&mut b, fg::NodeKind::Compute, 10);
    let c = node(&mut b, fg::NodeKind::Compute, 10);
    let d = node(&mut b, fg::NodeKind::Graphics, 10);
    let _ = b.add_edge(edge(a, bb));
    let _ = b.add_edge(edge(bb, c));
    let _ = b.add_edge(edge(c, a));
    let mut cy = Vec::new();
    if let Ok(g) = b.into_snapshot() {
        cy = tp::cycle_nodes(&g);
        cy.sort();
    }
    let ok = cy == vec![a, bb, c] && !cy.contains(&d);
    s.add("CB03-环-04", ok, "环外孤立节点不入环清单");

    // CB03-环-05：无环图环清单空表（不变量另一头——防恒真门禁）。
    let mut b = fg::FrameGraphBuilder::for_frame(24);
    let a = node(&mut b, fg::NodeKind::Upload, 10);
    let bb = node(&mut b, fg::NodeKind::Present, 10);
    let _ = b.add_edge(edge(a, bb));
    let ok = match b.into_snapshot() {
        Ok(g) => tp::cycle_nodes(&g).is_empty() && tp::plan(&g).is_ok(),
        Err(_) => false,
    };
    s.add("CB03-环-05", ok, "无环图环清单空且 plan 放行（双向）");

    // CB03-环-06：错误码互异非空（三码封闭）。
    let ok = !tp::E_GRAPH_CYCLE.is_empty()
        && !tp::E_TOPO_EMPTY.is_empty()
        && tp::E_GRAPH_CYCLE != tp::E_TOPO_EMPTY;
    s.add("CB03-环-06", ok, "错误码非空互异");
}

// ---------------------------------------------------------------------------
// 组三：关键路径与理论对拍
// ---------------------------------------------------------------------------

fn chk_critical(s: &mut CheckSet) {
    // CB03-关键-01：链 A(100)→B(200)→C(300) 关键路径恰全链、总预算 600。
    let mut b = fg::FrameGraphBuilder::for_frame(31);
    let a = node(&mut b, fg::NodeKind::Upload, 100);
    let bb = node(&mut b, fg::NodeKind::Compute, 200);
    let c = node(&mut b, fg::NodeKind::Present, 300);
    let _ = b.add_edge(edge(a, bb));
    let _ = b.add_edge(edge(bb, c));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-关键-01", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-关键-01", false, "无环图被拒");
            return;
        }
    };
    let ok = p.critical_path == vec![a, bb, c]
        && p.critical_total_us == 600
        && p.critical_total_us == hand_sum(&g, &[a, bb, c]);
    s.add("CB03-关键-01", ok, "链关键路径与判据侧手算总和对拍");

    // CB03-关键-02：菱形不等权——长支胜出，短支非关键（对拍理论最长链）。
    let mut b = fg::FrameGraphBuilder::for_frame(32);
    let a = node(&mut b, fg::NodeKind::Upload, 100);
    let long = node(&mut b, fg::NodeKind::Compute, 500);
    let short = node(&mut b, fg::NodeKind::Compute, 50);
    let d = node(&mut b, fg::NodeKind::Present, 100);
    let _ = b.add_edge(edge(a, long));
    let _ = b.add_edge(edge(a, short));
    let _ = b.add_edge(edge(long, d));
    let _ = b.add_edge(edge(short, d));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-关键-02", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-关键-02", false, "无环图被拒");
            return;
        }
    };
    let ok = p.critical_path == vec![a, long, d]
        && p.critical_total_us == 700
        && p.is_critical(short) == false
        && p.is_critical(a)
        && p.is_critical(long)
        && p.is_critical(d);
    s.add("CB03-关键-02", ok, "不等权菱形长支胜出短支非关键");

    // CB03-关键-03：换路对称验证——短支加码后关键路径翻到短支
    // （防「永远取先到支」式假实现对拍）。
    let mut b = fg::FrameGraphBuilder::for_frame(33);
    let a = node(&mut b, fg::NodeKind::Upload, 100);
    let l = node(&mut b, fg::NodeKind::Compute, 50);
    let r = node(&mut b, fg::NodeKind::Compute, 500);
    let d = node(&mut b, fg::NodeKind::Present, 100);
    let _ = b.add_edge(edge(a, l));
    let _ = b.add_edge(edge(a, r));
    let _ = b.add_edge(edge(l, d));
    let _ = b.add_edge(edge(r, d));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-关键-03", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-关键-03", false, "无环图被拒");
            return;
        }
    };
    let ok = p.critical_path == vec![a, r, d] && p.critical_total_us == 700;
    s.add("CB03-关键-03", ok, "权重对调后关键路径随之换路");

    // CB03-关键-04：关键集恰为关键路径（标注一致性——预算保障口径）。
    let ok = p.critical_set == p.critical_path;
    s.add("CB03-关键-04", ok, "critical_set 与 critical_path 逐元素相等");

    // CB03-关键-05：关键路径首尾端点——首为源（零入度）、尾为汇（零出度）。
    let ok = !p.critical_path.is_empty()
        && wave_index(&p, p.critical_path[0]) == Some(0)
        && wave_index(&p, *p.critical_path.last().unwrap_or(&u32::MAX))
            == Some(p.wave_count() - 1);
    s.add("CB03-关键-05", ok, "关键路径首波为源末波为汇");

    // CB03-关键-06：松弛方向——后继继承只增不减且必含自身预算
    // （回归钉：dist 漏加自身 bud 的实现此条必红）。
    //  链 A(1)→B(2)→C(4)：手算 total=7；漏 budC 得 3、漏 budB 得 5。
    let mut b = fg::FrameGraphBuilder::for_frame(34);
    let a = node(&mut b, fg::NodeKind::Copy, 1);
    let bb = node(&mut b, fg::NodeKind::Copy, 2);
    let c = node(&mut b, fg::NodeKind::Copy, 4);
    let _ = b.add_edge(edge(a, bb));
    let _ = b.add_edge(edge(bb, c));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-关键-06", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-关键-06", false, "无环图被拒");
            return;
        }
    };
    let ok = p.critical_total_us == 7 && p.critical_path == vec![a, bb, c];
    s.add("CB03-关键-06", ok, "总预算 1+2+4 恰等（漏任一自身预算即红）");
}

// ---------------------------------------------------------------------------
// 组四：并行发射吞吐
// ---------------------------------------------------------------------------

fn chk_throughput(s: &mut CheckSet) {
    // CB03-吞吐-01：星形 1→8 叶最大波宽 8。
    let mut b = fg::FrameGraphBuilder::for_frame(41);
    let hub = node(&mut b, fg::NodeKind::Upload, 10);
    let mut leaves: Vec<u32> = Vec::new();
    for _ in 0..8 {
        let lf = node(&mut b, fg::NodeKind::Compute, 10);
        let _ = b.add_edge(edge(hub, lf));
        leaves.push(lf);
    }
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-吞吐-01", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-吞吐-01", false, "无环图被拒");
            return;
        }
    };
    let ok = p.max_parallel() == 8 && p.waves[1].len() == 8 && p.total_nodes() == 9;
    s.add("CB03-吞吐-01", ok, "星形八叶最大并行度恰 8");

    // CB03-吞吐-02：波次数与关键路径互洽（星形：波数 2=hub 波+叶波，
    //  关键路径 hub+一叶 2 节点、总预算 10+10）。
    let ok = p.wave_count() == 2
        && p.critical_path.len() == 2
        && p.critical_total_us == 20;
    s.add("CB03-吞吐-02", ok, "星形波数 2、关键路径 hub+一叶 2 节点 20μs");

    // CB03-吞吐-03：十独立节点全并一波（吞吐上限=节点数）。
    let mut b = fg::FrameGraphBuilder::for_frame(42);
    for i in 0..10 {
        let _ = node(&mut b, fg::NodeKind::Copy, (i as u32) + 1);
    }
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-吞吐-03", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-吞吐-03", false, "无环图被拒");
            return;
        }
    };
    let ok = p.wave_count() == 1 && p.max_parallel() == 10 && p.total_nodes() == 10;
    s.add("CB03-吞吐-03", ok, "十独立节点一波全并行");

    // CB03-吞吐-04：链状图波宽恒 1（并行度下限对账）。
    let mut b = fg::FrameGraphBuilder::for_frame(43);
    let mut prev = node(&mut b, fg::NodeKind::Upload, 10);
    for _ in 0..5 {
        let cur = node(&mut b, fg::NodeKind::Compute, 10);
        let _ = b.add_edge(edge(prev, cur));
        prev = cur;
    }
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-吞吐-04", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-吞吐-04", false, "无环图被拒");
            return;
        }
    };
    let ok = p.max_parallel() == 1 && p.wave_count() == 6 && p.total_nodes() == 6;
    s.add("CB03-吞吐-04", ok, "六链波宽恒 1 波数恰 6");
}

// ---------------------------------------------------------------------------
// 组五：万节点性能（账面守恒）
// ---------------------------------------------------------------------------

fn chk_perf(s: &mut CheckSet) {
    // CB03-性能-01：万节点链 plan 账面守恒（波数/覆盖/关键总和）。
    const N_CHAIN: u32 = 10000;
    let mut b = fg::FrameGraphBuilder::for_frame(51);
    let mut prev = node(&mut b, fg::NodeKind::Copy, 1);
    for _ in 1..N_CHAIN {
        let cur = node(&mut b, fg::NodeKind::Copy, 1);
        let _ = b.add_edge(edge(prev, cur));
        prev = cur;
    }
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-性能-01", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-性能-01", false, "万链被拒");
            return;
        }
    };
    let ok = p.wave_count() == N_CHAIN as usize
        && p.total_nodes() == N_CHAIN as usize
        && p.critical_total_us == N_CHAIN as u64
        && p.critical_path.len() == N_CHAIN as usize;
    s.add("CB03-性能-01", ok, "万节点链波数/覆盖/关键总和守恒");

    // CB03-性能-02：万节点星 plan 账面守恒（两波/波宽 9999）。
    const N_STAR: usize = 9999;
    let mut b = fg::FrameGraphBuilder::for_frame(52);
    let hub = node(&mut b, fg::NodeKind::Upload, 5);
    for _ in 0..N_STAR {
        let lf = node(&mut b, fg::NodeKind::Compute, 1);
        let _ = b.add_edge(edge(hub, lf));
    }
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-性能-02", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-性能-02", false, "万星被拒");
            return;
        }
    };
    let ok = p.wave_count() == 2
        && p.max_parallel() == N_STAR
        && p.total_nodes() == N_STAR + 1
        && p.critical_total_us == 6;
    s.add("CB03-性能-02", ok, "万星两波波宽 9999 关键路径 6μs");

    // CB03-性能-03：万节点环 cycle_nodes 全量捕获（万规模环检出）。
    const N_CYC: u32 = 10000;
    let mut b = fg::FrameGraphBuilder::for_frame(53);
    let mut first = node(&mut b, fg::NodeKind::Copy, 1);
    let mut prev = first;
    for _ in 1..N_CYC {
        let cur = node(&mut b, fg::NodeKind::Copy, 1);
        let _ = b.add_edge(edge(prev, cur));
        prev = cur;
    }
    let _ = b.add_edge(edge(prev, first));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-性能-03", false, "图构造失败");
            return;
        }
    };
    let cy = tp::cycle_nodes(&g);
    let ok = cy.len() == N_CYC as usize && tp::plan(&g) == Err(tp::E_GRAPH_CYCLE);
    s.add("CB03-性能-03", ok, "万节点环全量捕获且 plan 拒绝");
}

// ---------------------------------------------------------------------------
// 组六：派生消费面（线性序 + 松弛量）
// ---------------------------------------------------------------------------

fn chk_derived(s: &mut CheckSet) {
    // 菱形语料：A(100)→{B(500),C(50)}→D(100)，关键链 A-B-D 共 700。
    let mut b = fg::FrameGraphBuilder::for_frame(71);
    let a = node(&mut b, fg::NodeKind::Upload, 100);
    let l = node(&mut b, fg::NodeKind::Compute, 500);
    let r = node(&mut b, fg::NodeKind::Compute, 50);
    let d = node(&mut b, fg::NodeKind::Present, 100);
    let _ = b.add_edge(edge(a, l));
    let _ = b.add_edge(edge(a, r));
    let _ = b.add_edge(edge(l, d));
    let _ = b.add_edge(edge(r, d));
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-线性-01", false, "图构造失败");
            s.add("CB03-松弛-01", false, "图构造失败");
            s.add("CB03-松弛-02", false, "图构造失败");
            s.add("CB03-松弛-03", false, "图构造失败");
            return;
        }
    };
    let p = match tp::plan(&g) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-线性-01", false, "无环图被拒");
            s.add("CB03-松弛-01", false, "无环图被拒");
            s.add("CB03-松弛-02", false, "无环图被拒");
            s.add("CB03-松弛-03", false, "无环图被拒");
            return;
        }
    };

    // CB03-线性-01：topo_order 恒等于波次级联（确定性投影）。
    let mut cascade: Vec<u32> = Vec::new();
    for w in p.waves.iter() {
        for &x in w.iter() {
            cascade.push(x);
        }
    }
    let ord1 = tp::topo_order(&p);
    let ord2 = tp::topo_order(&p);
    let ok = ord1 == cascade && ord1 == ord2 && ord1.len() == 4;
    s.add("CB03-线性-01", ok, "线性序=波次级联且两次求取恒等");

    // CB03-松弛-01：松弛量表与判据侧手算对拍（短支 C 恰 450，其余 0）。
    let sk = tp::slacks(&g, &p);
    let ok = sk.len() == 4
        && sk[a as usize] == 0
        && sk[l as usize] == 0
        && sk[r as usize] == 450
        && sk[d as usize] == 0;
    s.add("CB03-松弛-01", ok, "菱形松弛量表手算对拍（短支 450）");

    // CB03-松弛-02：关键路径任务松弛恒 0 + 零松弛集包含关键路径全量，
    //  且非零松弛短支节点不入集（两头都测）。
    let zs = tp::zero_slack_set(&g, &p);
    let crit_zero = p.critical_path.iter().all(|&h| sk[h as usize] == 0);
    let superset = p.critical_path.iter().all(|h| zs.contains(h));
    let ok = crit_zero && superset && !zs.contains(&r);
    s.add("CB03-松弛-02", ok, "零松弛集⊇关键路径且不含非零松弛节点");

    // CB03-松弛-03：松弛读屏行携带零松弛计数与最大拖延量。
    let line = tp::screen_line_slack(&g, &p);
    let ok = line.contains("3 任务") && line.contains("450 微秒");
    s.add("CB03-松弛-03", ok, "松弛读屏行含零松弛 3 与 450 微秒");
}

// ---------------------------------------------------------------------------
// 组七：F0162 衔接 + 读屏 + 判据元
// ---------------------------------------------------------------------------

fn chk_bridge(s: &mut CheckSet) {
    // CB03-衔接-01：F0162 推导边直驱——同一资源序（W0 写 r1 → W1 读 r1 写
    //  r2 → W2 读 r2），推导建边图与手工建边图 plan 全等（波次+关键路径）。
    //  plan 只看预算与边：手工图节点走 node()，自动图节点带同款预算，
    //  裸图节点带读写集供 F0162 推边。
    let mut manual = fg::FrameGraphBuilder::for_frame(61);
    let m0 = node(&mut manual, fg::NodeKind::Compute, 30);
    let m1 = node(&mut manual, fg::NodeKind::Compute, 40);
    let m2 = node(&mut manual, fg::NodeKind::Graphics, 50);
    let _ = manual.add_edge(edge(m0, m1));
    let _ = manual.add_edge(edge(m1, m2));
    let g_manual = match manual.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-衔接-01", false, "手工图构造失败");
            return;
        }
    };
    // 裸图：节点带读写集、无边——F0162 从中推导 RAW 边。
    let mut b = fg::FrameGraphBuilder::for_frame(62);
    let _ = b.add_node(spec_rw(fg::NodeKind::Compute, 30, vec![1], vec![1]));
    let _ = b.add_node(spec_rw(fg::NodeKind::Compute, 40, vec![1, 2], vec![2]));
    let _ = b.add_node(spec_rw(fg::NodeKind::Graphics, 50, vec![2], vec![]));
    let g_bare = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-衔接-01", false, "裸图构造失败");
            return;
        }
    };
    let derived = match dd::from_frame_graph(&g_bare) {
        Ok(d) => d,
        Err(_) => {
            s.add("CB03-衔接-01", false, "F0162 推导失败");
            return;
        }
    };
    let de = match dd::to_builder_edges(&derived) {
        Ok(e) => e,
        Err(_) => {
            s.add("CB03-衔接-01", false, "F0162 边导出失败");
            return;
        }
    };
    let mut bb = fg::FrameGraphBuilder::for_frame(63);
    let d0 = node(&mut bb, fg::NodeKind::Compute, 30);
    let d1 = node(&mut bb, fg::NodeKind::Compute, 40);
    let d2 = node(&mut bb, fg::NodeKind::Graphics, 50);
    let mut edges_ok = de.len() == 2;
    for e in de.iter() {
        if bb.add_edge(*e).is_err() {
            edges_ok = false;
        }
    }
    let g_auto = match bb.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB03-衔接-01", false, "自动图构造失败");
            return;
        }
    };
    let pm = match tp::plan(&g_manual) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-衔接-01", false, "手工图 plan 失败");
            return;
        }
    };
    let pa = match tp::plan(&g_auto) {
        Ok(p) => p,
        Err(_) => {
            s.add("CB03-衔接-01", false, "自动图 plan 失败");
            return;
        }
    };
    let hand_total = 30u64 + 40 + 50;
    let ok = edges_ok
        && pa.waves == pm.waves
        && pa.critical_path == pm.critical_path
        && pa.critical_total_us == hand_total
        && pa.critical_path == vec![d0, d1, d2];
    s.add("CB03-衔接-01", ok, "F0162 推导边直驱与手工建边计划全等");

    // CB03-判据-01：版本字面量（防漂移）。
    let ok = tp::TOPOPATH_VERSION == "CB03-topopath-v1";
    s.add("CB03-判据-01", ok, "TOPOPATH_VERSION 字面量钉死");

    // CB03-判据-02：读屏单行携带波数与帧延迟下限（短语级锚定防弱判据）。
    let mut b = fg::FrameGraphBuilder::for_frame(64);
    let a = node(&mut b, fg::NodeKind::Upload, 10);
    let bb = node(&mut b, fg::NodeKind::Present, 20);
    let _ = b.add_edge(edge(a, bb));
    let ok = match b.into_snapshot() {
        Ok(g) => match tp::plan(&g) {
            Ok(p) => {
                let line = tp::screen_line_plan(&p);
                line.contains("2 波") && line.contains("30 微秒")
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("CB03-判据-02", ok, "读屏行携带波数与帧延迟下限数字");
}

/// 带读写集与预算的规格（衔接判据用——F0162 按读写集推边）。
fn spec_rw(kind: fg::NodeKind, budget: u32, reads: Vec<u32>, writes: Vec<u32>) -> fg::NodeSpec {
    fg::NodeSpec {
        kind,
        reads,
        writes,
        budget_us: budget,
        priority: 0,
        deadline_us: 0,
        queue: fg::QueueKind::Graphics,
    }
}

// ---------------------------------------------------------------------------
// 聚合
// ---------------------------------------------------------------------------

/// CGPU-F0163 域自检入口（聚合防自调：只调组函数+自身 tally）。
pub fn run_vcb03_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0163");
    chk_order(&mut s);
    chk_cycle(&mut s);
    chk_critical(&mut s);
    chk_throughput(&mut s);
    chk_perf(&mut s);
    chk_derived(&mut s);
    chk_bridge(&mut s);
    // 条数对账：7 组 32 条（排序 6+环 6+关键 6+吞吐 4+性能 3+派生 4+衔接判据 3）。
    let (pass, fail) = s.tally();
    let ok = pass + fail == 32;
    s.add(
        "CB03-判据-03",
        ok,
        "条数对账：实挂 32 条（7 组）",
    );
    s
}
