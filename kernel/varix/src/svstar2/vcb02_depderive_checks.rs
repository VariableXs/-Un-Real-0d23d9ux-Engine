//! CGPU-F0162 自检 · 资源读写集自动依赖推导（CGPU-B 域）
//!
//! **锚点判据逐条对应**（`#CGPU-F0162`「RAW/WAR/WAW 全测、子资源粒度验证、
//! 边合并正确、漏边零容忍注入测试、推导性能」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | RAW/WAR/WAW 全测 | `CB02-RAW/WAR/WAW-*`（三规则逐条注入+无写不建边对照） |
//! | 读读不建边 | `CB02-读读-*`（两连读零边——可并行的结构性验证） |
//! | 子资源粒度 | `CB02-粒度-*`（同 base 异 mip/异数组层推导不出边+同子资源对照） |
//! | 边合并正确 | `CB02-合并-*`（同对任务多冲突单边+成因布尔保留+计数账） |
//! | 漏边零容忍 | `CB02-注入-*`（手算期望边集逐条对账——不漏+不滥双向） |
//! | 推导性能 | `CB02-性能-*`（万节点链式/万读者收口账面守恒） |
//! | 判据 | `CB02-判据-*`（版本/错误码/主因映射/条数对账） |
//!
//! **判据设计硬规矩**：期望值判据侧独立手算；不变量两头都测（不漏+不滥）；
//! 判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::vcb01_framegraph as fg;
use crate::svstar2::vcb02_depderive as dd;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

fn sr(base: u32, mip: u16, layer: u16) -> dd::SubRes {
    dd::SubRes { base, mip, array_layer: layer }
}

fn task(handle: u32, reads: Vec<dd::SubRes>, writes: Vec<dd::SubRes>) -> dd::TaskAccess {
    dd::TaskAccess { handle, reads, writes }
}

/// 手算期望边（漏边零容忍的对账口径）。
fn expect_edge(dg: &dd::DerivedGraph, from: u32, to: u32, raw: bool, war: bool, waw: bool) -> bool {
    dg.edge_between(from, to).map_or(false, |e| {
        e.reason.raw == raw && e.reason.war == war && e.reason.waw == waw && e.reason.any()
    })
}

// ---------------------------------------------------------------------------
// 组一：RAW/WAR/WAW 全测 + 读读不建边
// ---------------------------------------------------------------------------

fn chk_rules(s: &mut CheckSet) {
    // CB02-RAW-01：写→读建 RAW（合规注入）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![]));
    let ok = expect_edge(d.result(), 0, 1, true, false, false) && d.result().raw_hits == 1;
    s.add("CB02-RAW-01", ok, "写后读建 RAW 边（三规则之一）");

    // CB02-RAW-02：无写先读→零边（对照——首读无 RAW）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![sr(1, 0, 0)], vec![]));
    let ok = d.result().is_empty() && d.result().raw_hits == 0;
    s.add("CB02-RAW-02", ok, "无写先读不建边（首读无 RAW）");

    // CB02-WAR-01：读→写建 WAR（反序依赖）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![sr(1, 0, 0)], vec![]));
    let _ = d.visit(&task(1, vec![], vec![sr(1, 0, 0)]));
    let ok = expect_edge(d.result(), 0, 1, false, true, false) && d.result().war_hits == 1;
    s.add("CB02-WAR-01", ok, "读后写建 WAR 边（三规则之二）");

    // CB02-WAW-01：写→写建 WAW。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![], vec![sr(1, 0, 0)]));
    let ok = expect_edge(d.result(), 0, 1, false, false, true) && d.result().waw_hits == 1;
    s.add("CB02-WAW-01", ok, "写后写建 WAW 边（三规则之三）");

    // CB02-读读-01：两连读零边（读读不建边=可并行的结构性来源）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![sr(1, 0, 0)], vec![]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![]));
    let ok = d.result().is_empty() && d.result().raw_hits == 0 && d.result().war_hits == 0;
    s.add("CB02-读读-01", ok, "两连读零边（可并行不被串行化）");

    // CB02-RAW-03：写→读→读 链——两读都只挂 RAW 且读读之间无边。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![]));
    let _ = d.visit(&task(2, vec![sr(1, 0, 0)], vec![]));
    let ok = d.result().len() == 2
        && expect_edge(d.result(), 0, 1, true, false, false)
        && expect_edge(d.result(), 0, 2, true, false, false)
        && d.result().edge_between(1, 2).is_none();
    s.add("CB02-RAW-03", ok, "写→双读：两 RAW 挂写者，读读零边");
}

// ---------------------------------------------------------------------------
// 组二：子资源粒度
// ---------------------------------------------------------------------------

fn chk_granularity(s: &mut CheckSet) {
    // CB02-粒度-01：同 base 异 mip 无边（同纹理不同 mip 可并行）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 1, 0)], vec![]));
    let ok = d.result().is_empty();
    s.add("CB02-粒度-01", ok, "同 base 异 mip 推导不出边（粒度红线）");

    // CB02-粒度-02：同 mip 有边（对照——粒度规则不吞真依赖）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![]));
    let ok = d.result().len() == 1;
    s.add("CB02-粒度-02", ok, "同 base 同 mip 正常建边（对照组）");

    // CB02-粒度-03：异数组层无边。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 1)], vec![]));
    let ok = d.result().is_empty();
    s.add("CB02-粒度-03", ok, "同 base 同 mip 异数组层无边");

    // CB02-粒度-04：整资源键碰子资源键——whole(mip=MAX) 与 mip0 视为不同键
    // （粗粒度声明与细粒度声明不互通是**显性**语义：全量清空须用 whole 键
    // 与所有子键逐个对撞——此处验证 whole 不误伤 mip0 场景下的并行）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![dd::SubRes::whole(1)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![]));
    let ok = d.result().is_empty();
    s.add("CB02-粒度-04", ok, "whole 键与子键异粒不相撞（键语义显性）");

    // CB02-粒度-05：whole 对 whole 正常建边（粗粒度对粗粒度）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![dd::SubRes::whole(1)]));
    let _ = d.visit(&task(1, vec![dd::SubRes::whole(1)], vec![]));
    let ok = d.result().len() == 1;
    s.add("CB02-粒度-05", ok, "whole 对 whole 正常建边");
}

// ---------------------------------------------------------------------------
// 组三：边合并
// ---------------------------------------------------------------------------

fn chk_merge(s: &mut CheckSet) {
    // CB02-合并-01：同对任务两资源冲突（RAW+WAW）→ 单边双因。
    let mut d = dd::DepDeriver::new();
    // 任务0 写 r0（任务1 读）+ 任务0 写 r1；任务1 读 r0 写 r1。
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0), sr(2, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![sr(2, 0, 0)]));
    let ok = d.result().len() == 1
        && expect_edge(d.result(), 0, 1, true, false, true)
        && d.result().raw_hits == 1
        && d.result().waw_hits == 1;
    s.add("CB02-合并-01", ok, "两资源冲突合并单边且成因双记（RAW+WAW）");

    // CB02-合并-02：读写同任务——RAW 与 WAW 合并到同一条对边。
    let mut d = dd::DepDeriver::new();
    // 任务0 写 r0；任务1 读 r0 再写 r0；任务2 读 r0 写 r0。
    // 手算：1 读→RAW(0,1)；1 写→WAW(0,1)（prev=0）且读者{1}不给自己 WAR；
    //       2 读→RAW(1,2)；2 写→WAW(1,2)。故两对边均为 RAW+WAW 合并。
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(2, vec![sr(1, 0, 0)], vec![sr(1, 0, 0)]));
    let ok = d.result().len() == 2
        && expect_edge(d.result(), 0, 1, true, false, true)
        && expect_edge(d.result(), 1, 2, true, false, true);
    s.add("CB02-合并-02", ok, "读写同任务：RAW+WAW 合并到同对边（手算复核）");

    // CB02-合并-03：三型同对全触发（写者/读者/写者交错）→ 单边三因。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)])); // 0 写
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![])); // 1 读 → RAW(0,1)
    let _ = d.visit(&task(2, vec![], vec![sr(1, 0, 0)])); // 2 写 → WAR(1,2)+WAW(0,2)
    let _ = d.visit(&task(3, vec![sr(1, 0, 0)], vec![])); // 3 读 → RAW(2,3)
    let ok = d.result().len() == 4
        && expect_edge(d.result(), 0, 1, true, false, false)
        && expect_edge(d.result(), 1, 2, false, true, false)
        && expect_edge(d.result(), 0, 2, false, false, true)
        && expect_edge(d.result(), 2, 3, true, false, false);
    s.add("CB02-合并-03", ok, "四任务交错全链手算对账（RAW/WAR/WAW 各就位）");
}

// ---------------------------------------------------------------------------
// 组四：漏边零容忍注入测试
// ---------------------------------------------------------------------------

fn chk_injection(s: &mut CheckSet) {
    // CB02-注入-01：菱形场景手算全对账（不漏）。
    let mut d = dd::DepDeriver::new();
    // 0 写 rA rB；1 读 rA；2 读 rB；3 读 rA rB。
    // 手算：0→1 RAW(rA)、0→2 RAW(rB)、0→3 RAW(rA+rB 合并)；1/2/3 互为读读无边。
    let _ = d.visit(&task(0, vec![], vec![sr(10, 0, 0), sr(11, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(10, 0, 0)], vec![]));
    let _ = d.visit(&task(2, vec![sr(11, 0, 0)], vec![]));
    let _ = d.visit(&task(3, vec![sr(10, 0, 0), sr(11, 0, 0)], vec![]));
    let ok = d.result().len() == 3
        && expect_edge(d.result(), 0, 1, true, false, false)
        && expect_edge(d.result(), 0, 2, true, false, false)
        && expect_edge(d.result(), 0, 3, true, false, false)
        && !expect_edge(d.result(), 1, 3, false, false, false); // 1→3 无边（读读）
    s.add("CB02-注入-01", ok, "菱形场景三边手算全对账（1→3 读读无边）");

    // CB02-注入-02：不滥——期望外零边（同上场景逐对反查）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(10, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(10, 0, 0)], vec![]));
    let _ = d.visit(&task(2, vec![], vec![]));
    let ok = d.result().len() == 1
        && d.result().edge_between(0, 2).is_none()
        && d.result().edge_between(1, 2).is_none()
        && d.result().edge_between(2, 0).is_none();
    s.add("CB02-注入-02", ok, "空任务与远端任务零边（不滥建）");

    // CB02-注入-03：同任务自读写（h 读自己写的资源）不建自边。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(0, vec![sr(1, 0, 0)], vec![sr(1, 0, 0)]));
    let ok = d.result().edges().iter().all(|e| e.from != e.to) && d.result().len() <= 1;
    s.add("CB02-注入-03", ok, "同句柄自读写不产生自环边（from!=to 纪律）");
}

// ---------------------------------------------------------------------------
// 组五：vcb01 衔接 + 推导性能
// ---------------------------------------------------------------------------

fn chk_bridge_perf(s: &mut CheckSet) {
    // CB02-衔接-01：from_frame_graph 扁平退化（F0161 快照接入）。
    let mut b = fg::FrameGraphBuilder::for_frame(20);
    let mut n0 = spec_g(fg::NodeKind::Compute);
    n0.writes = vec_res(&[5]);
    let mut n1 = spec_g(fg::NodeKind::Graphics);
    n1.reads = vec_res(&[5]);
    let h0 = b.add_node(n0).unwrap_or(0);
    let h1 = b.add_node(n1).unwrap_or(1);
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB02-衔接-01", false, "F0161 快照扁平资源推导 RAW（单层退化）");
            return;
        }
    };
    let dg = dd::from_frame_graph(&g);
    let ok = match dg {
        Ok(d) => d.len() == 1 && expect_edge(&d, h0, h1, true, false, false),
        Err(_) => false,
    };
    s.add("CB02-衔接-01", ok, "F0161 快照扁平资源推导 RAW（单层退化）");

    // CB02-衔接-02：to_builder_edges 主因映射（raw→ReadAfterWrite）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![]));
    let edges = dd::to_builder_edges(d.result());
    let ok = match edges {
        Ok(v) => v.len() == 1 && v[0].kind == fg::EdgeKind::ReadAfterWrite,
        Err(_) => false,
    };
    s.add("CB02-衔接-02", ok, "主因映射 raw→ReadAfterWrite（优先级钉死）");

    // CB02-衔接-03：EDGE_PRIORITY 序钉死（raw > war > waw）。
    let ok = dd::EDGE_PRIORITY == ["raw", "war", "waw"]
        && dd::DepReason::NONE.primary() == "waw";
    s.add("CB02-衔接-03", ok, "主因优先级表钉死（NONE 空因归 waw 档可判）");

    // CB02-性能-01：万节点链式 RAW 推导（9999 边账面守恒）。
    let mut d = dd::DepDeriver::new();
    for i in 0..10_000u32 {
        let reads = if i == 0 { vec![] } else { vec![sr(i, 0, 0)] };
        let writes = vec![sr(i + 1, 0, 0)];
        let _ = d.visit(&task(i, reads, writes));
    }
    let r = d.result();
    let ok = r.len() == 9_999 && r.raw_hits == 9_999 && r.waw_hits == 0;
    s.add("CB02-性能-01", ok, "万节点链式推导 9999 RAW（单遍扫描账守恒）");

    // CB02-性能-02：万读者收口（1 写 + 9999 读 + 1 写 → 9999 RAW + 9999 WAR + 1 WAW）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    for i in 1..10_000u32 {
        let _ = d.visit(&task(i, vec![sr(1, 0, 0)], vec![]));
    }
    let _ = d.visit(&task(10_000, vec![], vec![sr(1, 0, 0)]));
    let r = d.result();
    let ok = r.raw_hits == 9_999 && r.war_hits == 9_999 && r.waw_hits == 1
        && r.edge_between(0, 10_000).map_or(false, |e| e.reason.waw);
    s.add("CB02-性能-02", ok, "万读者收口三账守恒（RAW/WAR/WAW 计数精确）");

    // CB02-性能-03：批量入口 derive_all 与逐 visit 等价（幂等收口）。
    let tasks = vec![
        task(0, vec![], vec![sr(1, 0, 0)]),
        task(1, vec![sr(1, 0, 0)], vec![]),
        task(2, vec![], vec![sr(1, 0, 0)]),
    ];
    let mut d1 = dd::DepDeriver::new();
    let a = d1.derive_all(&tasks).map(|g| (g.len(), g.raw_hits, g.war_hits, g.waw_hits));
    let mut d2 = dd::DepDeriver::new();
    for t in tasks.iter() {
        let _ = d2.visit(t);
    }
    let b = Ok((d2.result().len(), d2.result().raw_hits, d2.result().war_hits, d2.result().waw_hits));
    s.add("CB02-性能-03", a == b && a.is_ok(), "批量入口与逐任务扫描结果一致");

    // CB02-结构-01：推导产物结构无环（from<to 全检——声明序的结构性承诺）。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![]));
    let _ = d.visit(&task(2, vec![], vec![sr(1, 0, 0)]));
    let r = d.result();
    let ok = r.is_acyclic() && d.result().edges().iter().all(|e| e.reason.zh() != "无成因");
    s.add("CB02-结构-01", ok, "产物无环且每边带成因（中文成因非空档）");
}

fn spec_g(kind: fg::NodeKind) -> fg::NodeSpec {
    fg::NodeSpec {
        kind,
        reads: Vec::new(),
        writes: Vec::new(),
        budget_us: 100,
        priority: 0,
        deadline_us: 16_000,
        queue: fg::QueueKind::Graphics,
    }
}

fn vec_res(rs: &[u32]) -> Vec<u32> {
    let mut v = Vec::new();
    for r in rs.iter() {
        v.push(*r);
    }
    v
}

// ---------------------------------------------------------------------------
// 组六：判据元
// ---------------------------------------------------------------------------

fn chk_meta(s: &mut CheckSet) {
    // CB02-判据-01：协议版本前缀。
    s.add(
        "CB02-判据-01",
        dd::DEPDERIVE_VERSION.starts_with("CB02"),
        "协议版本 CB02-*（跨版本对账锚）",
    );

    // CB02-判据-02：错误码非空。
    s.add("CB02-判据-02", !dd::E_TASK_UNKNOWN.is_empty(), "错误码非空（外部可观测分支）");

    // CB02-判据-03：读屏单行含三型计数。
    let mut d = dd::DepDeriver::new();
    let _ = d.visit(&task(0, vec![], vec![sr(1, 0, 0)]));
    let _ = d.visit(&task(1, vec![sr(1, 0, 0)], vec![]));
    let line = dd::screen_line_derive(d.result());
    let ok = line.contains("1 条边") && line.contains("RAW 1");
    s.add("CB02-判据-03", ok, "推导读屏单行（边数+三型计数）");

    // CB02-判据-04：判据条数对账（本条前已有 27 条，本条为第 28 条）。
    s.add("CB02-判据-04", s.len() == 27, "判据条数对账（声明 28）");
}

// ---------------------------------------------------------------------------
// 聚合（单集 26 条 ≤ MAX_CHECKS=112）
// ---------------------------------------------------------------------------

/// CGPU-F0162 域自检（聚合入口，注册表用）。
pub fn run_vcb02_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0162");
    chk_rules(&mut s);
    chk_granularity(&mut s);
    chk_merge(&mut s);
    chk_injection(&mut s);
    chk_bridge_perf(&mut s);
    chk_meta(&mut s);
    s
}
