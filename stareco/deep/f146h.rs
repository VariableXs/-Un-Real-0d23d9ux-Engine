//! 深化层五 · F146 插件化星图后端（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：源池健康 → 前端数据源选择页行（F101 天气件等
//! 消费）、当前生效源标注、全降级诚实行、插件管理页行。

use super::f146g::{degrade, DegradedLevel};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 数据源选择页行：源池 → (源名, 健康, 是否生效)
// ---------------------------------------------------------------------------

pub struct SourceRow {
    pub name: &'static str,
    pub healthy: bool,
    pub active: bool,
}

/// 装配：生效源 = 降级决策树当前落点（Primary 或 Mirror 中首个健康者）。
pub fn source_rows(
    names: &[&'static str],
    health: &[bool],
) -> alloc::vec::Vec<SourceRow> {
    let primary_ok = health.first().copied().unwrap_or(false);
    let any_ok = health.iter().any(|h| *h);
    let cache = false; // 选择页不显示缓存档——缓存是后端内部事实
    let level = degrade(primary_ok, any_ok && !primary_ok, cache);
    let active_idx = match level {
        DegradedLevel::Primary => Some(0usize),
        DegradedLevel::Mirror => health.iter().position(|h| *h).filter(|i| *i > 0),
        _ => None,
    };
    names
        .iter()
        .enumerate()
        .map(|(i, n)| SourceRow {
            name: n,
            healthy: health.get(i).copied().unwrap_or(false),
            active: active_idx == Some(i),
        })
        .collect()
}

/// 全降级诚实行：所有源坏 → 单行说明（不显示假健康）。
pub fn offline_rows(names: &[&'static str]) -> alloc::vec::Vec<(&'static str, &'static str)> {
    names
        .iter()
        .map(|n| (*n, "不可达：数据走本地缓存（只读）"))
        .collect()
}

// ---------------------------------------------------------------------------
// 插件管理页行：插件登记 → (名, 阶段人话, 可禁用)
// ---------------------------------------------------------------------------

pub struct PluginPageRow {
    pub name: &'static str,
    pub stage_label: &'static str,
    pub can_disable: bool,
}

pub fn plugin_rows(
    items: &[(u32, &'static str, u8)],
) -> alloc::vec::Vec<PluginPageRow> {
    items
        .iter()
        .map(|(id, name, stage)| {
            let _ = id;
            let (label, can_disable) = match stage {
                0 => ("已发现", false),
                1 => ("已验证", true),
                2 => ("已注册", true),
                3 => ("运行中", true),
                4 => ("已检疫", false), // 检疫件不可禁用只可移除——防误启用
                _ => ("未知", false),
            };
            PluginPageRow { name, stage_label: label, can_disable }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F146H_TAG: &str = "stareco-F146-deep5";

pub fn run_f146_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F146H_TAG);

    // 数据源行
    let names = ["official", "mirror-a", "mirror-b"];
    let health = [true, false, true];
    let rows = source_rows(&names, &health);
    set.add(
        "f146h primary active",
        rows[0].active && rows[0].healthy && !rows[1].active && !rows[2].active,
        "主源健康时生效标注唯一",
    );
    let degraded = source_rows(&names, &[false, true, false]);
    set.add(
        "f146h mirror takeover",
        degraded[1].active && degraded[1].healthy && !degraded[0].active,
        "镜像接棒生效",
    );
    set.add(
        "f146h none active",
        source_rows(&names, &[false, false, false])
            .iter()
            .all(|r| !r.active),
        "全坏时无生效标注",
    );

    // 离线诚实行
    let off = offline_rows(&names);
    set.add(
        "f146h offline rows",
        off.len() == 3 && off[0].1.contains("本地缓存"),
        "全降级诚实行",
    );

    // 插件页
    let items = [
        (1u32, "weather", 3u8),
        (2, "bad-actor", 4),
        (3, "fresh", 0),
        (4, "mystery", 9),
    ];
    let rows2 = plugin_rows(&items);
    set.add(
        "f146h plugin labels",
        rows2[0].stage_label == "运行中" && rows2[2].stage_label == "已发现",
        "阶段人话",
    );
    set.add(
        "f146h quarantine guard",
        !rows2[1].can_disable && rows2[0].can_disable,
        "检疫件只可移除",
    );
    set.add(
        "f146h unknown stage",
        rows2[3].stage_label == "未知" && !rows2[3].can_disable,
        "未知阶段保守禁改",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn single_source_pool() {
        let rows = source_rows(&["only"], &[true]);
        assert!(rows[0].active && rows[0].healthy);
        let down = source_rows(&["only"], &[false]);
        assert!(!down[0].active && !down[0].healthy);
    }
}
