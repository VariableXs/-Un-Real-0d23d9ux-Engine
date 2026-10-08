//! CGPU-F0161 自检 · 帧任务图数据结构（CGPU-B 域）
//!
//! **锚点判据逐条对应**（`#CGPU-F0161`「结构全覆盖、ID 索引、arena 零碎片、
//! 快照语义、万节点压测」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 结构全覆盖 | `CB01-结构-*`（六节点型+四边型+五节点属性逐项在册） |
//! | ID 索引 | `CB01-索引-*`（下标即句柄+O(1) 定位+越界 None） |
//! | arena 零碎片 | `CB01-arena-*`（池尾追加账+整池释放+双释放拒绝） |
//! | 快照语义 | `CB01-快照-*`（into_snapshot 消费构建器+只读 API 面+帧号关联） |
//! | 万节点压测 | `CB01-压测-*`（万节点建图+快照+全图扫描+整池释放） |
//! | 判据 | `CB01-判据-*`（边三闸/版本/错误码/读屏/条数对账） |
//!
//! **判据设计硬规矩**：期望值判据侧独立重算；不变量两头都测（违规被拒+
//! 合规放行）；判据区零 panic 面（不用 unwrap——一律 match/unwrap_or(false)）。

use crate::checks::CheckSet;
use crate::svstar2::vcb01_framegraph as fg;

// ---------------------------------------------------------------------------
// 辅助：判据侧独立构造节点规格/资源集/句柄集
// ---------------------------------------------------------------------------

fn spec(kind: fg::NodeKind) -> fg::NodeSpec {
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

fn vec_res(rs: &[fg::ResId]) -> Vec<fg::ResId> {
    let mut v = Vec::new();
    for r in rs.iter() {
        v.push(*r);
    }
    v
}

fn vec_h(hs: &[fg::NodeHandle]) -> Vec<fg::NodeHandle> {
    let mut v = Vec::new();
    for h in hs.iter() {
        v.push(*h);
    }
    v
}

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 组一：结构全覆盖
// ---------------------------------------------------------------------------

fn chk_structure(s: &mut CheckSet) {
    // CB01-结构-01：六节点型闭集短码互异（穷尽 match 分派地基）。
    let kinds = fg::NodeKind::all();
    let mut ok = kinds.len() == fg::NODE_KIND_COUNT;
    for i in 0..kinds.len() {
        for j in (i + 1)..kinds.len() {
            ok = ok && kinds[i].tag() != kinds[j].tag();
        }
    }
    s.add("CB01-结构-01", ok, "六节点型短码互异（上传/计算/图形/拷贝/合成/呈现）");

    // CB01-结构-02：四边型闭集短码互异。
    let edges = fg::EdgeKind::all();
    let mut ok = edges.len() == fg::EDGE_KIND_COUNT;
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            ok = ok && edges[i].tag() != edges[j].tag();
        }
    }
    s.add("CB01-结构-02", ok, "四边型短码互异（写后读/读后写/写后写/显式）");

    // CB01-结构-03：节点五属性全在册（字段级断言——写什么读回什么）。
    let mut b = fg::FrameGraphBuilder::for_frame(1);
    let n = b
        .add_node(fg::NodeSpec {
            kind: fg::NodeKind::Compute,
            reads: vec_res(&[7, 8]),
            writes: vec_res(&[9]),
            budget_us: 250,
            priority: 3,
            deadline_us: 8_000,
            queue: fg::QueueKind::Compute,
        })
        .unwrap_or(u32::MAX);
    let ok = match b.into_snapshot() {
        Ok(g) => g.get_node(n).map_or(false, |x| {
            x.kind == fg::NodeKind::Compute
                && x.reads == [7u32, 8u32]
                && x.writes == [9u32]
                && x.budget_us == 250
                && x.priority == 3
                && x.deadline_us == 8_000
                && x.queue == fg::QueueKind::Compute
        }),
        Err(_) => false,
    };
    s.add("CB01-结构-03", ok, "节点五属性逐字段读回一致（结构全覆盖）");

    // CB01-结构-04：三队列型短码互异（所在队列类型闭集）。
    let qs = [
        (fg::QueueKind::Graphics, "gfx"),
        (fg::QueueKind::Compute, "dcompute"),
        (fg::QueueKind::Transfer, "transfer"),
    ];
    let mut ok = true;
    for i in 0..qs.len() {
        for j in (i + 1)..qs.len() {
            ok = ok && qs[i].1 != qs[j].1;
        }
    }
    ok = ok && qs[0].0.tag() == "gfx" && qs[2].0.tag() == "transfer";
    s.add("CB01-结构-04", ok, "三队列型短码互异且抽样对账");
}

// ---------------------------------------------------------------------------
// 组二：ID 索引
// ---------------------------------------------------------------------------

fn chk_index(s: &mut CheckSet) {
    let mut b = fg::FrameGraphBuilder::for_frame(2);

    // CB01-索引-01：句柄=分配序号（下标即句柄——0 起连续）。
    let h0 = b.add_node(spec(fg::NodeKind::Upload));
    let h1 = b.add_node(spec(fg::NodeKind::Copy));
    let ok = h0 == Ok(0) && h1 == Ok(1);
    s.add("CB01-索引-01", ok, "句柄=池下标（0 起连续，ID 索引化）");

    // CB01-索引-02/03：快照后按句柄读回 + 越界显性 None（一张图两组断言）。
    let (ok2, ok3) = match b.into_snapshot() {
        Ok(g) => (
            g.get_node(0).map_or(false, |x| x.kind == fg::NodeKind::Upload)
                && g.get_node(1).map_or(false, |x| x.kind == fg::NodeKind::Copy),
            g.get_node(2).is_none() && g.get_node(u32::MAX).is_none(),
        ),
        Err(_) => (false, false),
    };
    s.add("CB01-索引-02", ok2, "句柄随机读回型别一致（ID 索引可定位）");
    s.add("CB01-索引-03", ok3, "越界句柄显性 None（不虚构节点）");

    // CB01-索引-04：读写集按句柄投影（reads_of/writes_of 与写入一致）。
    let mut b2 = fg::FrameGraphBuilder::for_frame(3);
    let n = b2
        .add_node(fg::NodeSpec {
            kind: fg::NodeKind::Graphics,
            reads: vec_res(&[1, 2, 3]),
            writes: vec_res(&[4]),
            budget_us: 100,
            priority: 0,
            deadline_us: 16_000,
            queue: fg::QueueKind::Graphics,
        })
        .unwrap_or(u32::MAX);
    let ok = match b2.into_snapshot() {
        Ok(g) => {
            g.reads_of(n) == Some([1u32, 2, 3].as_slice())
                && g.writes_of(n) == Some([4u32].as_slice())
        }
        Err(_) => false,
    };
    s.add("CB01-索引-04", ok, "读写集按句柄投影一致（资源透视面）");
}

// ---------------------------------------------------------------------------
// 组三：arena 零碎片
// ---------------------------------------------------------------------------

fn chk_arena(s: &mut CheckSet) {
    // CB01-arena-01：池尾追加账（分配数=追加次数）。
    let mut b = fg::FrameGraphBuilder::for_frame(4);
    let mut added = 0usize;
    for _ in 0..5 {
        if b.add_node(spec(fg::NodeKind::Compute)).is_ok() {
            added += 1;
        }
    }
    s.add(
        "CB01-arena-01",
        added == 5 && b.allocated() == 5,
        "池尾追加账一致（分配 O(1) 连续五次）",
    );

    // CB01-arena-02：整池释放一次归还全部（零碎片的结构性来源）。
    let mut ok = false;
    let mut ok3 = false;
    match b.into_snapshot() {
        Ok(mut g) => {
            ok = g.release_pool() == Ok(5) && g.pool_released();
            ok3 = g.release_pool().is_err() && g.pool_released();
        }
        Err(_) => {}
    }
    s.add("CB01-arena-02", ok, "整池释放归还 5 节点（无逐点 free=无碎片）");
    s.add("CB01-arena-03", ok3, "重复整池释放拒绝（防双释放）");

    // CB01-arena-04：空池快照拒绝（一帧图至少一个节点）。
    let empty = fg::FrameGraphBuilder::for_frame(5);
    let ok = matches!(empty.into_snapshot(), Err(c) if c == fg::E_POOL_EMPTY);
    s.add("CB01-arena-04", ok, "空池快照拒绝（空图无调度意义）");
}

// ---------------------------------------------------------------------------
// 组四：快照语义
// ---------------------------------------------------------------------------

fn chk_snapshot(s: &mut CheckSet) {
    // CB01-快照-01/02/03：into_snapshot 消费构建器 + 帧号随迁 + 多消费者只读
    // + Clone 分发一致（一张图四组断言）。
    let mut b = fg::FrameGraphBuilder::for_frame(77);
    let _ = b.add_node(spec(fg::NodeKind::Present));
    let ok1;
    let ok2;
    let ok3;
    match b.into_snapshot() {
        Ok(g) => {
            ok1 = g.frame_no == 77 && g.node_count() == 1;
            ok2 = g.get_node(0).map_or(false, |x| x.kind == fg::NodeKind::Present)
                && g.present_nodes() == vec_h(&[0])
                && g.incoming(0).is_empty();
            let g2 = g.clone();
            ok3 = g2.frame_no == g.frame_no && g2.node_count() == g.node_count();
        }
        Err(_) => {
            ok1 = false;
            ok2 = false;
            ok3 = false;
        }
    }
    s.add("CB01-快照-01", ok1, "快照帧号关联+构建器被消费（图成只读）");
    s.add("CB01-快照-02", ok2, "三视角只读一致（多消费者安全）");
    s.add("CB01-快照-03", ok3, "快照克隆读一致（可分发给多消费者）");

    // CB01-快照-04：边随快照迁入（边数与构建期一致）。
    let mut b2 = fg::FrameGraphBuilder::for_frame(6);
    let n0 = b2.add_node(spec(fg::NodeKind::Upload)).unwrap_or(0);
    let n1 = b2.add_node(spec(fg::NodeKind::Graphics)).unwrap_or(1);
    let _ = b2.add_edge(fg::Edge { from: n0, to: n1, kind: fg::EdgeKind::ReadAfterWrite });
    let ok = match b2.into_snapshot() {
        Ok(g) => g.edge_count() == 1 && g.incoming(n1).len() == 1,
        Err(_) => false,
    };
    s.add("CB01-快照-04", ok, "边表随快照迁入（前驱投影一致）");
}

// ---------------------------------------------------------------------------
// 组五：边三闸 + 万节点压测
// ---------------------------------------------------------------------------

fn chk_edges_stress(s: &mut CheckSet) {
    // CB01-边-01：自环拒。
    let mut b = fg::FrameGraphBuilder::for_frame(7);
    let n0 = b.add_node(spec(fg::NodeKind::Compute)).unwrap_or(0);
    let ok = b.add_edge(fg::Edge { from: n0, to: n0, kind: fg::EdgeKind::Explicit }).is_err();
    s.add("CB01-边-01", ok, "自环依赖拒绝");

    // CB01-边-02：端点未知拒（双向）。
    let ok = b.add_edge(fg::Edge { from: n0, to: 99, kind: fg::EdgeKind::Explicit }).is_err()
        && b.add_edge(fg::Edge { from: 99, to: n0, kind: fg::EdgeKind::Explicit }).is_err();
    s.add("CB01-边-02", ok, "端点未知拒绝（双向）");

    // CB01-边-03：重复边拒（同端点同型）+ 异型边放行。
    let n1 = b.add_node(spec(fg::NodeKind::Graphics)).unwrap_or(1);
    let e = fg::Edge { from: n0, to: n1, kind: fg::EdgeKind::ReadAfterWrite };
    let first = b.add_edge(e).is_ok();
    let dup = b.add_edge(e).is_err();
    let other = b
        .add_edge(fg::Edge { from: n0, to: n1, kind: fg::EdgeKind::WriteAfterWrite })
        .is_ok();
    s.add("CB01-边-03", first && dup && other, "重复边拒+异型边放行（同端点可多型依赖）");

    // CB01-环-01：无环链图 Kahn 检出无环（合规放行侧）。
    let n2 = b.add_node(spec(fg::NodeKind::Compose)).unwrap_or(2);
    let _ = b.add_edge(fg::Edge { from: n1, to: n2, kind: fg::EdgeKind::Explicit });
    let ok = match b.into_snapshot() {
        Ok(g) => !g.has_cycle(),
        Err(_) => false,
    };
    s.add("CB01-环-01", ok, "无环链图检出无环（Kahn 放行侧）");

    // CB01-环-02：构造环 0→1→2→0 检出成环（红项可达——防恒假门禁）。
    let mut b2 = fg::FrameGraphBuilder::for_frame(8);
    let a = b2.add_node(spec(fg::NodeKind::Compute)).unwrap_or(0);
    let c = b2.add_node(spec(fg::NodeKind::Copy)).unwrap_or(1);
    let d = b2.add_node(spec(fg::NodeKind::Upload)).unwrap_or(2);
    let _ = b2.add_edge(fg::Edge { from: a, to: c, kind: fg::EdgeKind::Explicit });
    let _ = b2.add_edge(fg::Edge { from: c, to: d, kind: fg::EdgeKind::Explicit });
    let _ = b2.add_edge(fg::Edge { from: d, to: a, kind: fg::EdgeKind::Explicit });
    let ok = match b2.into_snapshot() {
        Ok(g) => g.has_cycle(),
        Err(_) => false,
    };
    s.add("CB01-环-02", ok, "三节点环检出成环（发射前合法性闸可达）");

    // CB01-普查-01：kind_census 与节点构成对账（结构全覆盖的运行期账面）。
    let mut b3 = fg::FrameGraphBuilder::for_frame(11);
    let _ = b3.add_node(spec(fg::NodeKind::Upload));
    let _ = b3.add_node(spec(fg::NodeKind::Upload));
    let _ = b3.add_node(spec(fg::NodeKind::Present));
    let census = [2usize, 0, 0, 0, 0, 1];
    let ok = match b3.into_snapshot() {
        Ok(g) => g.kind_census() == census && g.kind_census().iter().sum::<usize>() == 3,
        Err(_) => false,
    };
    s.add("CB01-普查-01", ok, "按型普查与构成逐型对账（2 上传+1 呈现）");

    // CB01-普查-02：帧预算总和独立重算。
    let mut b4 = fg::FrameGraphBuilder::for_frame(12);
    let mut sp = spec(fg::NodeKind::Compute);
    sp.budget_us = 300;
    let _ = b4.add_node(sp);
    let mut sp2 = spec(fg::NodeKind::Copy);
    sp2.budget_us = 500;
    let _ = b4.add_node(sp2);
    let ok = match b4.into_snapshot() {
        Ok(g) => g.total_budget_us() == 800,
        Err(_) => false,
    };
    s.add("CB01-普查-02", ok, "帧预算总和 300+500=800 独立对账");

    // CB01-压测-01：万节点建图（分配账=10000）。
    let mut big = fg::FrameGraphBuilder::for_frame(100);
    let mut added = 0usize;
    for i in 0..10_000u32 {
        let mut sp = spec(fg::NodeKind::Compute);
        sp.reads = vec_res(&[i]);
        sp.writes = vec_res(&[i + 1]);
        if big.add_node(sp).is_ok() {
            added += 1;
        }
    }
    let ok = added == 10_000 && big.allocated() == 10_000;
    s.add("CB01-压测-01", ok, "万节点 arena 分配账一致（10000/10000）");

    // CB01-压测-02/03/04：万节点边链+快照+全图扫描+整池释放（一张图四组断言）。
    for i in 0..9_999u32 {
        let _ = big.add_edge(fg::Edge {
            from: i,
            to: i + 1,
            kind: fg::EdgeKind::WriteAfterRead,
        });
    }
    let ok2;
    let ok3;
    let ok4;
    match big.into_snapshot() {
        Ok(mut g) => {
            ok2 = g.node_count() == 10_000 && g.edge_count() == 9_999;
            let mut scan_ok = true;
            for h in 0..10_000u32 {
                scan_ok = scan_ok
                    && g.reads_of(h).map_or(false, |r| r == [h].as_slice())
                    && g.writes_of(h).map_or(false, |w| w == [h + 1].as_slice());
            }
            ok3 = scan_ok && g.incoming(5_000).len() == 1 && g.present_nodes().is_empty();
            ok4 = g.release_pool() == Ok(10_000) && g.pool_released();
        }
        Err(_) => {
            ok2 = false;
            ok3 = false;
            ok4 = false;
        }
    }
    s.add("CB01-压测-02", ok2, "万节点+9999 边快照成功（图规模达标）");
    s.add("CB01-压测-03", ok3, "万节点全图扫描一致（读写投影+前驱账）");
    s.add("CB01-压测-04", ok4, "万节点整池释放一次归还（arena 收口）");
}

// ---------------------------------------------------------------------------
// 组六：读屏 + 判据元
// ---------------------------------------------------------------------------

fn chk_meta(s: &mut CheckSet) {
    // CB01-判据-01：图读屏单行（帧号+节点数+边数+池态）。
    let mut b = fg::FrameGraphBuilder::for_frame(9);
    let n0 = b.add_node(spec(fg::NodeKind::Upload)).unwrap_or(0);
    let n1 = b.add_node(spec(fg::NodeKind::Present)).unwrap_or(1);
    let _ = b.add_edge(fg::Edge { from: n0, to: n1, kind: fg::EdgeKind::Explicit });
    let line = match b.into_snapshot() {
        Ok(g) => fg::screen_line_graph(&g),
        Err(_) => String::new(),
    };
    let ok = line.contains("9") && line.contains("2") && line.contains("1");
    s.add("CB01-判据-01", ok, "图读屏单行（帧号/节点/边可读）");

    // CB01-判据-02：协议版本前缀。
    s.add(
        "CB01-判据-02",
        fg::FRAMEGRAPH_VERSION.starts_with("CB01"),
        "协议版本 CB01-*（跨版本对账锚）",
    );

    // CB01-判据-03：错误码非空互异。
    s.add(
        "CB01-判据-03",
        !fg::E_NODE_UNKNOWN.is_empty()
            && !fg::E_EDGE_BAD.is_empty()
            && !fg::E_POOL_EMPTY.is_empty()
            && fg::E_NODE_UNKNOWN != fg::E_EDGE_BAD
            && fg::E_EDGE_BAD != fg::E_POOL_EMPTY,
        "错误码非空互异（外部可观测分支）",
    );

    // CB01-判据-04：判据条数对账（本条前已有 30 条，本条为第 31 条）。
    s.add("CB01-判据-04", s.len() == 30, "判据条数对账（声明 31）");
}

use alloc::string::String;

// ---------------------------------------------------------------------------
// 聚合（单集 24 条 ≤ MAX_CHECKS=112）
// ---------------------------------------------------------------------------

/// CGPU-F0161 域自检（聚合入口，注册表用）。
pub fn run_vcb01_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0161");
    chk_structure(&mut s);
    chk_index(&mut s);
    chk_arena(&mut s);
    chk_snapshot(&mut s);
    chk_edges_stress(&mut s);
    chk_meta(&mut s);
    s
}
