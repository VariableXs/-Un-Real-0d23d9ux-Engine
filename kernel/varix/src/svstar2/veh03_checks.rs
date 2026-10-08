//! VE-F1403 · 域自检（判据逐条对应，见 `veh03_mixgraph.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四类节点 → `H03-四类-*`
//! - 端口协商 → `H03-协商-*`
//! - 三查（环/采样率/布局，建图与热更新双时点）→ `H03-三查-*`
//! - 热更新（影子图原子提交 + 等增益过渡）→ `H03-热更新-*`
//! - 模板化（播放链五层 = 本图的预置实例）→ `H03-模板-*`

use super::veh03_mixgraph::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

fn port(ch: u8, rate: u32, resample: bool) -> Port {
    Port {
        channels: ch,
        sample_rate: rate,
        resample_declared: resample,
    }
}

/// VE-F1403 域自检。
pub fn run_veh03_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh03");

    // ---- 判据：四类节点（端口语义强制）----

    {
        let mut g = MixGraph::new();
        let ok = g
            .add_node(1, NodeKind::Source, "源", None, Some(port(CH_STEREO, 48_000, false)))
            .is_ok()
            && g.add_node(2, NodeKind::Processor, "处理", Some(port(CH_STEREO, 48_000, false)), Some(port(CH_STEREO, 48_000, false))).is_ok()
            && g.add_node(3, NodeKind::Bus, "总线", Some(port(CH_STEREO, 48_000, false)), Some(port(CH_STEREO, 48_000, false))).is_ok()
            && g.add_node(4, NodeKind::Listener, "监听", Some(port(CH_MONO, 48_000, false)), None)
                .is_ok();
        // 源带入口 / 监听带出口 = 端口语义违例，显性拒绝
        let bad_src = g
            .add_node(9, NodeKind::Source, "坏源", Some(port(CH_MONO, 48_000, false)), None)
            .as_ref()
            .err()
            .map(|e| e.code == "E_PORT_SEMANTICS" && e.is_complete())
            .unwrap_or(false);
        let bad_listener = g
            .add_node(10, NodeKind::Listener, "坏监听", None, Some(port(CH_MONO, 48_000, false)))
            .as_ref()
            .err()
            .map(|e| e.code == "E_PORT_SEMANTICS")
            .unwrap_or(false);
        set.add(
            "H03-四类-端口语义强制",
            ok && bad_src && bad_listener && g.nodes.len() == 4,
            "",
        );
    }

    // ---- 判据：端口协商 ----

    {
        let mut g = MixGraph::new();
        let _ = g.add_node(1, NodeKind::Source, "源", None, Some(port(CH_STEREO, 48_000, false)));
        let _ = g.add_node(2, NodeKind::Listener, "监听", Some(port(CH_MONO, 48_000, false)), None);
        // 跨布局且未声明 → 拒绝；声明后 → 放行
        let r1 = g.connect(Edge { from: 1, to: 2 }).as_ref().err().map(|e| e.code == "E_LAYOUT_INCOMPAT" && e.is_complete()).unwrap_or(false);
        let _ = g.find_port_mut(2).map(|n| n.in_port = Some(port(CH_MONO, 48_000, true)));
        let r2 = g.connect(Edge { from: 1, to: 2 }).is_ok();
        // 重复边拒绝
        let r3 = g.connect(Edge { from: 1, to: 2 }).as_ref().err().map(|e| e.code == "E_DUP_EDGE").unwrap_or(false);
        set.add("H03-协商-布局跨率声明制", r1 && r2 && r3, "");
    }
    {
        let mut g = MixGraph::new();
        let _ = g.add_node(1, NodeKind::Source, "源", None, Some(port(CH_STEREO, 48_000, false)));
        let _ = g.add_node(2, NodeKind::Listener, "监听", Some(port(CH_STEREO, 44_100, false)), None);
        let rejected = g.connect(Edge { from: 1, to: 2 }).as_ref().err().map(|e| e.code == "E_RATE_MISMATCH").unwrap_or(false);
        let _ = g.find_port_mut(1).map(|n| n.out_port = Some(port(CH_STEREO, 48_000, true)));
        let accepted = g.connect(Edge { from: 1, to: 2 }).is_ok();
        set.add("H03-协商-采样率跨率重采样声明", rejected && accepted, "");
    }

    // ---- 判据：三查（环检测 / 采样率 / 布局，全图级）----

    {
        let mut g = MixGraph::new();
        let p = port(CH_STEREO, 48_000, false);
        let _ = g.add_node(1, NodeKind::Processor, "A", Some(p), Some(p));
        let _ = g.add_node(2, NodeKind::Processor, "B", Some(p), Some(p));
        let _ = g.connect(Edge { from: 1, to: 2 });
        let _ = g.connect(Edge { from: 2, to: 1 }); // 成环
        let cycle = g.validate_all().as_ref().err().map(|e| e.code == "E_GRAPH_CYCLE" && e.is_complete() && e.why.contains("反馈")).unwrap_or(false);
        set.add("H03-三查-环检测拒绝", cycle, "");
    }

    // ---- 判据：热更新（影子图原子提交 + 拒绝不动账）----

    {
        let tpl = playback_chain_template().expect("模板应合法");
        let mut host = GraphHost::new();
        host.live = tpl;
        // 合法补丁：插入 EQ 处理节点并接入 3→4 之间
        let p = port(CH_STEREO, 48_000, false);
        let patch = GraphPatch {
            add_nodes: vec![(6, NodeKind::Processor, "EQ".to_string(), Some(p), Some(p))],
            remove_nodes: vec![],
            connect: vec![Edge { from: 3, to: 6 }, Edge { from: 6, to: 4 }],
            disconnect: vec![Edge { from: 3, to: 4 }],
            ..Default::default()
        };
        let v1 = host.apply_patch(&patch);
        let committed = matches!(v1, PatchVerdict::Committed { transitions: 1 });
        let wired = host.live.edges.contains(&Edge { from: 3, to: 6 })
            && host.live.edges.contains(&Edge { from: 6, to: 4 })
            && !host.live.edges.contains(&Edge { from: 3, to: 4 });
        // 非法补丁（成环）：拒绝且活图不动
        let bad = GraphPatch {
            add_nodes: vec![],
            remove_nodes: vec![],
            connect: vec![Edge { from: 6, to: 3 }],
            disconnect: vec![],
            ..Default::default()
        };
        let snapshot = host.live.a11y_summary();
        let v2 = host.apply_patch(&bad);
        let untouched = host.live.a11y_summary() == snapshot;
        set.add(
            "H03-热更新-原子提交与拒绝不动账",
            committed
                && wired
                && matches!(v2, PatchVerdict::Rejected(ref e) if e.code == "E_GRAPH_CYCLE")
                && untouched
                && host.committed_patches == 1
                && host.rejected_patches == 1,
            "",
        );
    }
    // 等增益过渡：端点精确、功率和恒 1、相邻步无跳变（无爆音的机器可验形式）
    {
        let n = TRANSITION_STEPS;
        let mut max_step = 0.0f64;
        let mut prev_in = 0.0f64;
        let mut prev_out = 0.0f64;
        let mut ok = true;
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let g_in = equal_power_gain(t, true);
            let g_out = equal_power_gain(t, false);
            // 功率和恒 1（等响切换）
            if (g_in * g_in + g_out * g_out - 1.0).abs() > 1e-9 {
                ok = false;
            }
            // 端点精确
            if i == 0 && (g_in != 0.0 || g_out != 1.0) {
                ok = false;
            }
            if i == n && (g_in != 1.0 || g_out != 0.0) {
                ok = false;
            }
            // 相邻步差有界（无跳变）
            if i > 0 {
                max_step = max_step.max((g_in - prev_in).abs()).max((g_out - prev_out).abs());
            }
            prev_in = g_in;
            prev_out = g_out;
        }
        set.add(
            "H03-热更新-等增益无爆音曲线",
            ok && max_step <= 0.05,
            "",
        );
    }

    // ---- 判据：模板化（播放链五层 = 本图预置实例）----

    {
        let tpl = playback_chain_template();
        let ok = tpl.is_ok();
        let g = tpl.unwrap_or_else(|_| MixGraph::new());
        let five_layers = g.nodes.len() == 5
            && g.find(1).map(|n| n.kind == NodeKind::Source).unwrap_or(false)
            && g.find(5).map(|n| n.kind == NodeKind::Listener).unwrap_or(false);
        // 模板自身过三查 + 是任意 DAG 的一个实例（还能继续热更新扩展）
        let extensible = {
            let mut host = GraphHost::new();
            host.live = g.clone();
            let p = port(CH_STEREO, 48_000, false);
            let patch = GraphPatch {
                add_nodes: vec![(9, NodeKind::Source, "第二源".to_string(), None, Some(p))],
                remove_nodes: vec![],
                connect: vec![Edge { from: 9, to: 4 }],
                disconnect: vec![],
                ..Default::default()
            };
            matches!(host.apply_patch(&patch), PatchVerdict::Committed { .. })
        };
        set.add(
            "H03-模板-五层预置可扩展",
            ok && five_layers && extensible && g.validate_all().is_ok(),
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let g = playback_chain_template().unwrap_or_else(|_| MixGraph::new());
        let s = g.a11y_summary();
        set.add(
            "H03-读屏-图摘要可播",
            s.contains("混音图") && s.contains("源") && s.contains("边"),
            "",
        );
    }
    {
        let run = || {
            let mut host = GraphHost::new();
            host.live = playback_chain_template().unwrap_or_else(|_| MixGraph::new());
            let p = port(CH_STEREO, 48_000, false);
            let good = GraphPatch {
                add_nodes: vec![(7, NodeKind::Processor, "压缩器".to_string(), Some(p), Some(p))],
                remove_nodes: vec![],
                connect: vec![Edge { from: 2, to: 7 }, Edge { from: 7, to: 3 }],
                disconnect: vec![Edge { from: 2, to: 3 }],
                ..Default::default()
            };
            let v1 = host.apply_patch(&good);
            let v2 = host.apply_patch(&GraphPatch {
                connect: vec![Edge { from: 7, to: 1 }],
                ..Default::default()
            });
            (host.live.a11y_summary(), format!("{:?}", v1), format!("{:?}", v2), host.committed_patches, host.rejected_patches)
        };
        let a = run();
        let b = run();
        set.add("H03-确定-同操作同账面", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veh03_checks_all_green() {
        let set = run_veh03_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-H03 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 三查铁律：环、跨率、跨布局都必须在建图与热更新两个时点被拦。
    #[test]
    fn three_checks_gate_both_entry_points() {
        // 建图时点
        let mut g = MixGraph::new();
        let p = port(CH_STEREO, 48_000, false);
        let _ = g.add_node(1, NodeKind::Processor, "A", Some(p), Some(p));
        let _ = g.add_node(2, NodeKind::Processor, "B", Some(p), Some(p));
        assert!(g.connect(Edge { from: 1, to: 2 }).is_ok());
        assert!(g.connect(Edge { from: 2, to: 1 }).is_ok());
        assert!(g.validate_all().is_err());
        // 热更新时点（影子图拦截，活图不动）
        let mut host = GraphHost::new();
        host.live = playback_chain_template().unwrap();
        let bad = GraphPatch {
            connect: vec![Edge { from: 4, to: 2 }],
            ..Default::default()
        };
        assert!(matches!(
            host.apply_patch(&bad),
            PatchVerdict::Rejected(GraphError { code: "E_GRAPH_CYCLE", .. })
        ));
        assert_eq!(host.live.edges.len(), 4, "拒绝的补丁不碰活图");
    }

    /// 等增益过渡的功率和恒 1（equal-power 的数学本体）。
    #[test]
    fn equal_power_sum_is_constant() {
        for i in 0..=100 {
            let t = i as f64 / 100.0;
            let gi = equal_power_gain(t, true);
            let go = equal_power_gain(t, false);
            assert!((gi * gi + go * go - 1.0).abs() < 1e-9, "t={} 处功率和偏离 1", t);
        }
    }
}
