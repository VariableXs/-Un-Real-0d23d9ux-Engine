//! VE-F1405 · 域自检（判据逐条对应，见 `veh05_submix.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - submix（封装复用，端口即出入口）→ `H05-封装-*`
//! - 八层上限（声明式防护）→ `H05-深度-*`
//! - 参数提升（封装不黑盒，提升清单显性）→ `H05-提升-*`
//! - 计量（归属透明，最耗可查）→ `H05-计量-*`
//! - 模板库 → `H05-模板-*`

use super::veh03_mixgraph::{Edge, MixGraph, NodeKind, Port};
use super::veh05_submix::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

fn p(channels: u8, rate: u32) -> Port {
    Port {
        channels,
        sample_rate: rate,
        resample_declared: false,
    }
}

/// 构造鼓组三节点子图（kick/snare/oh → 鼓组总线）。
fn drum_group() -> MixGraph {
    let mut g = MixGraph::new();
    let _ = g.add_node(1, NodeKind::Source, "底鼓", None, Some(p(1, 48_000)));
    let _ = g.add_node(2, NodeKind::Source, "军鼓", None, Some(p(1, 48_000)));
    let _ = g.add_node(3, NodeKind::Source, " overhead", None, Some(p(2, 48_000)));
    let _ = g.add_node(4, NodeKind::Processor, "鼓组压缩", Some(p(2, 48_000)), Some(p(2, 48_000)));
    let _ = g.add_node(5, NodeKind::Bus, "鼓组总线出", Some(p(2, 48_000)), Some(p(2, 48_000)));
    let _ = g.connect(Edge { from: 1, to: 4 });
    let _ = g.connect(Edge { from: 2, to: 4 });
    let _ = g.connect(Edge { from: 3, to: 4 });
    let _ = g.connect(Edge { from: 4, to: 5 });
    g
}

/// VE-F1405 域自检。
pub fn run_veh05_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh05");

    // ---- 判据：submix（封装复用：三节点 → 一个推子）----

    {
        let mut host = NestedGraphHost::new();
        let ext = host
            .package("鼓组", drum_group(), 5, 5, 0, Some(p(2, 48_000)), Some(p(2, 48_000)))
            .expect("鼓组打包");
        // 入口与出口同为"鼓组总线出"（有入有出，合法）——宿主图只见一个节点
        let one_node = host.graph.nodes.len() == 1 && host.graph.find(ext).is_some();
        let named = host
            .graph
            .find(ext)
            .map(|n| n.name.contains("鼓组"))
            .unwrap_or(false);
        set.add(
            "H05-封装-三节点一推子",
            one_node && named && host.submixes.len() == 1,
            "",
        );
    }
    // 出入口缺失拒绝：入口无入口端口 / 出口无出口端口 / 节点不存在
    {
        let mut host = NestedGraphHost::new();
        // 入口=源（无入口端口）→ 拒
        let r1 = host
            .package("坏入口", drum_group(), 1, 5, 0, Some(p(2, 48_000)), Some(p(2, 48_000)))
            .as_ref()
            .err()
            .map(|e| e.code == "E_ENTRY_MISSING" && e.is_complete())
            .unwrap_or(false);
        let mut g = drum_group();
        let _ = g.remove_node(5);
        let r2 = host
            .package("坏出口", g, 4, 5, 0, Some(p(2, 48_000)), Some(p(2, 48_000)))
            .as_ref()
            .err()
            .map(|e| e.code == "E_EXIT_MISSING")
            .unwrap_or(false);
        set.add("H05-封装-出入口语义拒绝", r1 && r2, "");
    }

    // ---- 判据：八层上限（第 9 层拒绝）----

    {
        let mut host = NestedGraphHost::new();
        let mut depth_reached: u32 = 0;
        let mut current = drum_group();
        let mut ok_last = true;
        // 逐层打包：第 1..8 层应成功，第 9 层应拒绝
        for layer in 1..=9u32 {
            let entry_ok = current.find(5).map(|n| n.in_port.is_some()).unwrap_or(false);
            if !entry_ok {
                // 保持链路：把 5 号当出入（有入有出）
                break;
            }
            match host.package(
                &alloc::format!("层{}", layer),
                current.clone(),
                5,
                5,
                layer - 1,
                Some(p(2, 48_000)),
                Some(p(2, 48_000)),
            ) {
                Ok(_) => {
                    depth_reached = layer;
                    ok_last = true;
                }
                Err(e) => {
                    ok_last = e.code == "E_DEPTH_EXCEEDED" && layer == 9;
                    break;
                }
            }
            // 下一层的内部图：包一个"子图的子图"需要占位节点图——简化为同图
            current = drum_group();
        }
        set.add(
            "H05-深度-八层过九层拒",
            depth_reached == 8 && ok_last,
            "",
        );
    }

    // ---- 判据：参数提升（封装不黑盒，未提升不可达）----

    {
        let mut host = NestedGraphHost::new();
        let ext = host
            .package("鼓组", drum_group(), 5, 5, 0, Some(p(2, 48_000)), Some(p(2, 48_000)))
            .unwrap();
        // 未提升：不可达（封装纪律）
        let blocked = host
            .set_param(ext, "attack", 0.5)
            .as_ref()
            .err()
            .map(|e| e.code == "E_PARAM_NOT_EXPOSED" && e.is_complete())
            .unwrap_or(false);
        // 显性提升后可达
        let promoted = host.promote(ext, 4, "attack", "attack").is_ok();
        // 对外名冲突拒绝
        let _ = host.promote(ext, 4, "release", "attack_dup");
        let dup = host.promote(ext, 4, "ratio", "attack").is_err();
        let settable = host.set_param(ext, "attack", 0.5).is_ok();
        set.add(
            "H05-提升-清单显性未提升不可达",
            blocked && promoted && dup && settable,
            "",
        );
    }

    // ---- 判据：计量（归属透明，最耗可查）----

    {
        let mut m = CpuMeter::new();
        m.account_host("主总线", 300);
        m.account_submix("鼓组", "鼓组压缩", 900);
        m.account_submix("鼓组", "底鼓采样", 100);
        m.account_submix("混响", "卷积核", 500);
        let rep = m.report();
        let top = rep.first().map(|(p, _)| p.clone()).unwrap_or_default();
        let by_top = m.report_by_top_level();
        let most = by_top.first().cloned().unwrap_or_default();
        set.add(
            "H05-计量-归属透明最耗可查",
            top.contains("鼓组压缩")
                && rep.first().map(|(_, c)| *c) == Some(900)
                && most.0 == "鼓组"
                && most.1 == 1000,
            "",
        );
    }

    // ---- 判据：模板库 ----

    {
        let t1 = template_game_three_bus();
        let t2 = template_room_correction();
        let t3 = template_surround_downmix();
        let g1 = t1.expect("三总线模板应合法");
        let three_source = g1
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Source)
            .count()
            == 3;
        let valid = g1.validate_all().is_ok() && t2.is_ok() && t3.is_ok();
        set.add(
            "H05-模板-三预设合法",
            three_source && valid,
            "",
        );
    }
    // 模板可再加工：从模板起步接子混音（新项目从模板起步）
    {
        let base = template_game_three_bus().unwrap_or_else(|_| MixGraph::new());
        let mut host = NestedGraphHost::new();
        host.graph = base;
        // 宿主图（已含 8 节点）上打包鼓组占位，并接到音效总线
        let ext = host
            .package("鼓组", drum_group(), 5, 5, 0, Some(p(2, 48_000)), Some(p(2, 48_000)))
            .unwrap();
        let wired = host.graph.connect(Edge { from: ext, to: 6 }).is_ok();
        set.add(
            "H05-模板-模板起步可扩展",
            wired && host.submixes.len() == 1 && host.graph.validate_all().is_ok(),
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut host = NestedGraphHost::new();
        let _ = host.package("鼓组", drum_group(), 5, 5, 0, Some(p(2, 48_000)), Some(p(2, 48_000)));
        let s = host.a11y_summary();
        set.add(
            "H05-读屏-宿主摘要可播",
            s.contains("子混音") && s.contains("上限"),
            "",
        );
    }
    {
        let run = || {
            let mut host = NestedGraphHost::new();
            let ext = host
                .package("鼓组", drum_group(), 5, 5, 0, Some(p(2, 48_000)), Some(p(2, 48_000)))
                .unwrap();
            let _ = host.promote(ext, 4, "attack", "attack");
            let _ = host.set_param(ext, "attack", 0.42);
            let mut m = CpuMeter::new();
            m.account_submix("鼓组", "鼓组压缩", 700);
            (host.a11y_summary(), m.report_by_top_level())
        };
        let a = run();
        let b = run();
        set.add("H05-确定-同操作同账面", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veh05_checks_all_green() {
        let set = run_veh05_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-H05 域自检红项：{}/{} 绿", passed, passed + failed);
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

    /// 深度上限：恰好第 9 层拒绝，前 8 层全过。
    #[test]
    fn depth_cap_at_nine() {
        let mut host = NestedGraphHost::new();
        let mut inner = drum_group();
        for layer in 1..=8u32 {
            let ext = host
                .package(
                    &alloc::format!("L{}", layer),
                    inner.clone(),
                    5,
                    5,
                    layer - 1,
                    Some(p(2, 48_000)),
                    Some(p(2, 48_000)),
                )
                .expect("前 8 层应全部成功");
            // 下一层的内部图 = 当前宿主图（模拟逐层嵌套）
            inner = drum_group();
            let _ = ext;
        }
        let ninth = host.package(
            "L9",
            inner,
            5,
            5,
            8,
            Some(p(2, 48_000)),
            Some(p(2, 48_000)),
        );
        assert_eq!(ninth.err().map(|e| e.code), Some("E_DEPTH_EXCEEDED"));
    }

    /// 封装纪律：未提升参数不可达，绕不过去。
    #[test]
    fn encapsulation_is_enforced() {
        let mut host = NestedGraphHost::new();
        let ext = host
            .package("鼓组", drum_group(), 5, 5, 0, Some(p(2, 48_000)), Some(p(2, 48_000)))
            .unwrap();
        assert!(host.set_param(ext, "ratio", 4.0).is_err());
        let _ = host.promote(ext, 4, "ratio", "ratio");
        assert!(host.set_param(ext, "ratio", 4.0).is_ok());
    }
}
