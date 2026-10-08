//! 深化层五 · F147 跨设备主题同步（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：同步状态 → 设置中心同步页行（设备/状态/冲突
//! 数）、冲突解决入口行（人工策略的下发数据）、同步关闭诚实行。

use super::f147g::Strategy;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 同步页行：设备状态 → (设备名, 状态人话, 冲突数角标)
// ---------------------------------------------------------------------------

pub struct SyncPageRow {
    pub device: &'static str,
    pub state_label: &'static str,
    pub conflicts: u32,
}

/// 状态：0=已同步 1=同步中 2=有冲突 3=离线；冲突数非零必显角标。
pub fn sync_rows(devices: &[(u32, &'static str, u8)]) -> alloc::vec::Vec<SyncPageRow> {
    devices
        .iter()
        .map(|(id, name, st)| {
            let _ = id;
            SyncPageRow {
                device: name,
                state_label: match st {
                    0 => "已同步",
                    1 => "同步中",
                    2 => "有冲突",
                    3 => "离线",
                    _ => "状态未知",
                },
                conflicts: if *st == 2 { 1 } else { 0 },
            }
        })
        .collect()
}

/// 冲突角标纪律：状态非冲突但角标非零 = 账面矛盾（机器面拦截）。
pub fn badge_consistent(rows: &[SyncPageRow]) -> bool {
    rows.iter().all(|r| (r.state_label == "有冲突") == (r.conflicts > 0))
}

// ---------------------------------------------------------------------------
// 冲突解决入口行：冲突字段 → 策略人话 + 是否需要用户出面
// ---------------------------------------------------------------------------

pub struct ConflictEntryRow {
    pub field: &'static str,
    pub strategy_label: &'static str,
    pub needs_user: bool,
}

pub fn conflict_entries(fields: &[&'static str]) -> alloc::vec::Vec<ConflictEntryRow> {
    fields
        .iter()
        .map(|f| {
            let (label, needs_user) = match super::f147g::strategy_for(f) {
                Strategy::LastWrite => ("自动合并（最新写入优先）", false),
                Strategy::TwoPhase => ("两段式：先预览再应用", false),
                Strategy::Manual => ("需要你选择保留哪边", true),
            };
            ConflictEntryRow { field: f, strategy_label: label, needs_user }
        })
        .collect()
}

/// 待用户出面清单（同步页红点数据源——只有 Manual 类型计数）。
pub fn pending_user_choices(entries: &[ConflictEntryRow]) -> usize {
    entries.iter().filter(|e| e.needs_user).count()
}

// ---------------------------------------------------------------------------
// 同步关闭诚实行：同步关掉 → 各设备行给「不同步」状态（不留空白）
// ---------------------------------------------------------------------------

pub fn sync_off_rows(devices: &[&'static str]) -> alloc::vec::Vec<(&'static str, &'static str)> {
    devices
        .iter()
        .map(|d| (*d, "同步已关闭：本机主题保持独立"))
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F147H_TAG: &str = "stareco-F147-deep5";

pub fn run_f147_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F147H_TAG);

    // 同步页行
    let rows = sync_rows(&[
        (1u32, "phone", 0u8),
        (2, "pad", 2),
        (3, "pc", 3),
        (4, "watch", 9),
    ]);
    set.add(
        "f147h states",
        rows[0].state_label == "已同步"
            && rows[1].state_label == "有冲突"
            && rows[3].state_label == "状态未知",
        "状态人话+未知不猜",
    );
    set.add(
        "f147h badges",
        rows[1].conflicts == 1 && rows[0].conflicts == 0,
        "冲突角标",
    );
    set.add("f147h badge consistency", badge_consistent(&rows), "账面一致性门");

    // 冲突入口
    let entries = conflict_entries(&["accent_color", "desktop_icon_layout", "notes"]);
    set.add(
        "f147h entries",
        !entries[0].needs_user && entries[1].needs_user && entries[2].needs_user,
        "自动/人工分派",
    );
    set.add(
        "f147h entry labels",
        entries[0].strategy_label.contains("最新写入") && entries[1].strategy_label.contains("选择"),
        "策略人话",
    );
    set.add("f147h pending count", pending_user_choices(&entries) == 2, "待出面计数");

    // 关闭诚实行
    let off = sync_off_rows(&["phone", "pc"]);
    set.add(
        "f147h off rows",
        off.len() == 2 && off[0].1.contains("保持独立"),
        "关闭不留空白",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn badge_inconsistent_detected() {
        let rows = [SyncPageRow { device: "x", state_label: "已同步", conflicts: 3 }];
        assert!(!badge_consistent(&rows));
    }

    #[test]
    fn unknown_field_defaults_manual() {
        let e = conflict_entries(&["never_seen_field"]);
        assert!(e[0].needs_user);
    }
}
