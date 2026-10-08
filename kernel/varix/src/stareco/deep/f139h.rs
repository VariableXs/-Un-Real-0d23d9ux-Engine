//! 深化层五 · F139 反馈闭环通道（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：tracker 统计 → F120 诊断中心体检灯（四灯阈值）、
//! 报告入口深链数据、单条报告摘要行装配。

use super::f139g::weekly_stats;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断中心体检灯：周开/关计数 + 积压 → 四灯判定（绿/蓝/黄/红）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthLight {
    Green,
    Blue,
    Yellow,
    Red,
}

/// 判定口径：净积压 ≥20 或周开 ≥50 → 红；净积压 ≥10 → 黄；
/// 有开单但未达黄 → 蓝（有活动）；零开零关 → 绿（安静）。
pub fn health_light(opened: usize, closed: usize) -> HealthLight {
    let backlog = opened as i64 - closed as i64;
    if opened >= 50 || backlog >= 20 {
        HealthLight::Red
    } else if backlog >= 10 {
        HealthLight::Yellow
    } else if opened > 0 {
        HealthLight::Blue
    } else {
        HealthLight::Green
    }
}

/// 诊断行装配：(周, 开, 关) → 灯 + 人话。
pub fn diag_rows(weeks: &[(u32, usize, usize)]) -> alloc::vec::Vec<(u32, HealthLight, &'static str)> {
    weeks
        .iter()
        .map(|(w, o, c)| {
            let light = health_light(*o, *c);
            let label = match light {
                HealthLight::Green => "安静周",
                HealthLight::Blue => "正常活动",
                HealthLight::Yellow => "积压抬升：安排分流",
                HealthLight::Red => "过载：立即分流并公告",
            };
            (*w, light, label)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 深链接口：报告 id → 诊断中心跳转参数（确定性参数串）
// ---------------------------------------------------------------------------

pub fn deep_link(report_id: u32, month: u32, seq: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    let prefix = b"diag://tracker/";
    out[..15].copy_from_slice(prefix);
    let mut pos = 15;
    for v in [report_id, month, seq] {
        let s = alloc::format!("{}", v);
        let b = s.as_bytes();
        if pos + b.len() + 1 > 32 {
            break;
        }
        out[pos..pos + b.len()].copy_from_slice(b);
        pos += b.len();
        out[pos] = b'/';
        pos += 1;
    }
    if pos > 0 {
        out[pos - 1] = 0; // 尾分隔符收掉
    }
    out
}

// ---------------------------------------------------------------------------
// 摘要行装配：单报告 → 摘要行（严重度标签 + 状态 + 聚类数）
// ---------------------------------------------------------------------------

pub fn summary_row(
    id: u32,
    sev: u8,
    status_open: bool,
    cluster_size: u32,
) -> (&'static str, &'static str, u32) {
    let sev_label = match sev {
        1 => "轻微",
        2 => "一般",
        3 => "严重",
        4 => "致命",
        _ => "未分诊",
    };
    let st = if status_open { "处理中" } else { "已关闭" };
    let _ = id;
    (sev_label, st, cluster_size)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F139H_TAG: &str = "stareco-F139-deep5";

pub fn run_f139_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F139H_TAG);

    // 体检灯
    set.add(
        "f139h lights",
        health_light(0, 0) == HealthLight::Green
            && health_light(5, 3) == HealthLight::Blue
            && health_light(15, 5) == HealthLight::Yellow
            && health_light(50, 0) == HealthLight::Red,
        "四灯阈值",
    );
    set.add(
        "f139h backlog red",
        health_light(25, 5) == HealthLight::Red,
        "净积压 20 线",
    );

    // 诊断行
    let rows = diag_rows(&[(1, 0, 0), (2, 15, 5)]);
    set.add(
        "f139h diag rows",
        rows[0].1 == HealthLight::Green && rows[1].2.contains("分流"),
        "人话标签",
    );

    // 深链
    let link = deep_link(7, 202609, 3);
    set.add(
        "f139h deep link",
        &link[..15] == b"diag://tracker/" && link[15..17] == *b"7/",
        "参数串形制",
    );

    // 摘要行
    let (sev, st, cl) = summary_row(1, 4, true, 6);
    set.add(
        "f139h summary",
        sev == "致命" && st == "处理中" && cl == 6,
        "摘要行三要素",
    );
    let (sev2, _, _) = summary_row(2, 0, false, 1);
    set.add("f139h untriaged", sev2 == "未分诊", "未分诊如实标注");

    // 周报复用（g 层口径）
    let wk = [(100u32, 1u32, 0u8), (101, 2, 1)];
    set.add(
        "f139h weekly reuse",
        weekly_stats(&wk, 100) == (1, 1),
        "g 层周报复用",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn light_yellow_boundary() {
        // 净积压恰 10 → 黄；9 → 蓝（有活动）。
        assert_eq!(health_light(10, 0), HealthLight::Yellow);
        assert_eq!(health_light(9, 0), HealthLight::Blue);
    }

    #[test]
    fn deep_link_guards_overflow() {
        let link = deep_link(u32::MAX, u32::MAX, u32::MAX);
        // 溢出不越 32 字节缓冲（截断而非崩）。
        assert!(link.iter().position(|b| *b == 0).map(|p| p <= 32).unwrap_or(true));
    }
}
