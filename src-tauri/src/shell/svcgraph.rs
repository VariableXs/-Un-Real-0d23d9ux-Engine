//! AI-15 开放工具组（化境域）：V-87 服务依赖图。
//!
//! 只读：枚举本机服务（HKLM\SYSTEM\CurrentControlSet\Services），
//! 读取 DependOnService（REG_MULTI_SZ）构建依赖图；提供：
//! - 全量图（节点 + 依赖边）
//! - 单服务依赖树（其依赖了谁，递归）
//! - 影响分析（谁依赖它，递归 —— 停止/禁用前的破坏面）
//! - 拓扑排序（并行启动顺序建议）
//!
//! 红线：仅读注册表，不改任何服务状态；影响分析只提示，不自动执行。

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::{AppError, CmdResult};

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 服务节点（只保留安全可公开的字段）。
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SvcNode {
    /// 服务内部名（注册表键名）
    pub name: String,
    /// 显示名（DisplayName，缺省回退内部名）
    pub display: String,
    /// "auto" | "demand" | "disabled" | "boot" | "system" | "unknown"
    pub start: String,
    /// "own" | "share" | "kernel" | "interact" | "unknown"
    pub process: String,
    /// 直接依赖（DependOnService 解析去重后）
    pub depends_on: Vec<String>,
}

/// 影响分析结果（对某个服务执行停止/禁用前的破坏面）。
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SvcImpact {
    pub target: String,
    /// 直接依赖它的服务
    pub direct_dependents: Vec<String>,
    /// 传递依赖它的服务（不含直接）
    pub transitive_dependents: Vec<String>,
    /// 目标自身的依赖（停止它不影响，但重启它需要这些先启动）
    pub target_depends_on: Vec<String>,
    /// 建议的停止顺序（依赖它的先停）与启动顺序（它依赖的先启）
    pub suggested_stop_order: Vec<String>,
    pub suggested_start_order: Vec<String>,
}

/// 拓扑排序结果（层级：同层可并行启动）。
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SvcTopo {
    /// 层级列表：第 0 层 = 不依赖任何服务的；第 N 层依赖第 N-1 层的
    pub layers: Vec<Vec<String>>,
    /// 依赖了图外/未知服务而无法归层的（环或缺失依赖）
    pub unresolved: Vec<String>,
}

// ---------------------------------------------------------------------------
// 注册表读取（仅 Windows；其他平台返回空图 + 错误说明）
// ---------------------------------------------------------------------------

/// 读取 HKLM 服务表 → 节点列表（含直接依赖边）。
#[cfg(windows)]
fn read_services() -> CmdResult<Vec<SvcNode>> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let root = hklm
        .open_subkey("SYSTEM\\CurrentControlSet\\Services")
        .map_err(|e| AppError::io(format!("打开服务注册表失败: {e}")))?;
    let mut nodes = Vec::new();
    for name in root.enum_keys().map_while(|k| k.ok()) {
        let Ok(sk) = root.open_subkey(&name) else { continue };
        let display: Option<String> = sk.get_value("DisplayName").ok();
        // 过滤：无 ImagePath 的键（驱动参数键等）不算用户可见服务
        let image: Option<String> = sk.get_value("ImagePath").ok();
        if image.is_none() {
            continue;
        }
        let start: Option<u32> = sk.get_value("Start").ok();
        let start_str = match start {
            Some(0) => "boot",
            Some(1) => "system",
            Some(2) => "auto",
            Some(3) => "demand",
            Some(4) => "disabled",
            _ => "unknown",
        };
        let ptype: Option<u32> = sk.get_value("Type").ok();
        let process_str = match ptype {
            Some(t) if t & 0x1 != 0 => {
                if t & 0x100 != 0 { "own" } else { "share" }
            }
            Some(t) if t & 0x2 != 0 => "kernel",
            Some(_) => "unknown",
            None => "unknown",
        };
        let deps: Vec<String> = sk
            .get_value::<Vec<String>, _>("DependOnService")
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty() && d.as_str() != "+")
            .collect();
        nodes.push(SvcNode {
            display: display.filter(|d| !d.is_empty()).unwrap_or_else(|| name.clone()),
            name: name.clone(),
            start: start_str.into(),
            process: process_str.into(),
            depends_on: deps,
        });
    }
    nodes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(nodes)
}

#[cfg(not(windows))]
fn read_services() -> CmdResult<Vec<SvcNode>> {
    Err(AppError::validation("仅支持 Windows"))
}

// ---------------------------------------------------------------------------
// 纯函数图算法（供测试）
// ---------------------------------------------------------------------------

/// 反向索引：name → 直接依赖它的服务列表。
pub fn build_dependents(nodes: &[SvcNode]) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in nodes {
        for d in &n.depends_on {
            m.entry(d.clone()).or_default().push(n.name.clone());
        }
        for v in m.values_mut() {
            v.sort();
            v.dedup();
        }
    }
    m
}

/// 递归收集：从 `start` 沿 `next` 闭包可达的节点集合（不含 start，含环保护）。
fn reachable(start: &str, next: &dyn Fn(&str) -> Vec<String>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = next(start);
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        for nx in next(&cur) {
            if !seen.contains(&nx) {
                stack.push(nx);
            }
        }
    }
    seen
}

/// 单服务依赖树：它依赖了谁（递归）。
fn dep_tree(name: &str, nodes: &BTreeMap<String, SvcNode>) -> Vec<String> {
    let next = |n: &str| -> Vec<String> {
        nodes.get(n).map(|s| s.depends_on.clone()).unwrap_or_default()
    };
    reachable(name, &next).into_iter().collect()
}

/// 影响分析（纯函数，供测试）：目标 → 受影响服务集合与建议顺序。
pub fn impact_of(target: &str, nodes: &[SvcNode]) -> SvcImpact {
    let by_name: BTreeMap<String, SvcNode> =
        nodes.iter().map(|n| (n.name.clone(), n.clone())).collect();
    let dependents = build_dependents(nodes);
    let back = |n: &str| -> Vec<String> { dependents.get(n).cloned().unwrap_or_default() };
    let all_affected: Vec<String> = reachable(target, &back).into_iter().collect();
    let direct: Vec<String> = dependents.get(target).cloned().unwrap_or_default();
    let transitive: Vec<String> = all_affected
        .iter()
        .filter(|n| !direct.contains(n))
        .cloned()
        .collect();
    // 停止顺序：受影响的按「依赖深度」浅者先停（近似拓扑逆序）
    let mut stop_order = all_affected.clone();
    stop_order.sort_by_key(|n| {
        by_name
            .get(n)
            .map(|s| s.depends_on.len())
            .unwrap_or(usize::MAX)
    });
    // 启动顺序：它依赖的按依赖数少者先启（近似）
    let mut start_deps = dep_tree(target, &by_name);
    start_deps.sort_by_key(|n| {
        by_name
            .get(n)
            .map(|s| s.depends_on.len())
            .unwrap_or(usize::MAX)
    });
    SvcImpact {
        target: target.to_string(),
        direct_dependents: direct,
        transitive_dependents: transitive,
        target_depends_on: by_name
            .get(target)
            .map(|s| s.depends_on.clone())
            .unwrap_or_default(),
        suggested_stop_order: stop_order,
        suggested_start_order: start_deps,
    }
}

/// 拓扑分层（纯函数，供测试）：Kahn 分层；环内节点进 unresolved。
pub fn topo_layers(nodes: &[SvcNode]) -> SvcTopo {
    let names: BTreeSet<String> = nodes.iter().map(|n| n.name.clone()).collect();
    // 只保留指向已知节点的边（图外依赖忽略，如驱动组）
    let deps: BTreeMap<String, BTreeSet<String>> = nodes
        .iter()
        .map(|n| {
            (
                n.name.clone(),
                n.depends_on.iter().filter(|d| names.contains(*d)).cloned().collect(),
            )
        })
        .collect();
    let mut resolved: BTreeSet<String> = BTreeSet::new();
    let mut layers: Vec<Vec<String>> = Vec::new();
    loop {
        let layer: Vec<String> = deps
            .iter()
            .filter(|(n, ds)| {
                !resolved.contains(*n) && ds.iter().all(|d| resolved.contains(d))
            })
            .map(|(n, _)| n.clone())
            .collect();
        if layer.is_empty() {
            break;
        }
        for n in &layer {
            resolved.insert(n.clone());
        }
        layers.push(layer);
        if resolved.len() == deps.len() {
            break;
        }
    }
    let unresolved: Vec<String> =
        deps.keys().filter(|n| !resolved.contains(*n)).cloned().collect();
    SvcTopo { layers, unresolved }
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------

/// V-87：全量服务依赖图。
#[tauri::command]
pub fn svc_graph() -> CmdResult<Vec<SvcNode>> {
    read_services()
}

/// V-87：单服务影响分析（停止/禁用前的破坏面提示）。
#[tauri::command]
pub fn svc_impact(target: String) -> CmdResult<SvcImpact> {
    let nodes = read_services()?;
    if !nodes.iter().any(|n| n.name.eq_ignore_ascii_case(&target)) {
        return Err(AppError::validation(format!("服务不存在: {target}")));
    }
    // 大小写归一到注册表键名
    let real = nodes
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case(&target))
        .map(|n| n.name.clone())
        .unwrap();
    Ok(impact_of(&real, &nodes))
}

/// V-87：拓扑分层（并行启动顺序建议）。
#[tauri::command]
pub fn svc_topo() -> CmdResult<SvcTopo> {
    Ok(topo_layers(&read_services()?))
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, deps: &[&str]) -> SvcNode {
        SvcNode {
            name: name.into(),
            display: name.into(),
            start: "demand".into(),
            process: "own".into(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn dependents_index_and_impact() {
        let nodes = vec![
            node("A", &["B"]),
            node("B", &["C"]),
            node("C", &[]),
            node("D", &["B"]),
        ];
        let dep = build_dependents(&nodes);
        assert_eq!(dep.get("C"), Some(&vec!["B".to_string()]));
        assert_eq!(dep.get("B"), Some(&vec!["A".to_string(), "D".to_string()]));
        let imp = impact_of("C", &nodes);
        assert!(imp.direct_dependents.contains(&"B".to_string()));
        assert!(imp.transitive_dependents.contains(&"A".to_string()));
        assert!(imp.transitive_dependents.contains(&"D".to_string()));
    }

    #[test]
    fn impact_cycle_safe() {
        // 环：X→Y→X（构造数据级环，算法不得死循环）
        let nodes = vec![node("X", &["Y"]), node("Y", &["X"]), node("Z", &["X"])];
        let imp = impact_of("Z", &nodes);
        assert!(imp.direct_dependents.is_empty());
        let imp_x = impact_of("X", &nodes);
        assert!(imp_x.direct_dependents.contains(&"Z".to_string()));
    }

    #[test]
    fn topo_orders_by_dependency() {
        let nodes = vec![
            node("base", &[]),
            node("mid", &["base"]),
            node("top", &["mid", "base"]),
        ];
        let t = topo_layers(&nodes);
        assert_eq!(t.layers[0], vec!["base".to_string()]);
        assert_eq!(t.layers[1], vec!["mid".to_string()]);
        assert_eq!(t.layers[2], vec!["top".to_string()]);
        assert!(t.unresolved.is_empty());
    }

    #[test]
    fn topo_unresolved_on_cycle() {
        let nodes = vec![node("a", &["b"]), node("b", &["a"])];
        let t = topo_layers(&nodes);
        assert!(t.layers.is_empty());
        assert_eq!(t.unresolved.len(), 2);
    }
}
