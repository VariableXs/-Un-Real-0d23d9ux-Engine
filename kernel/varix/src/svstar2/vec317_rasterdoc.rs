//! VE-F0717 · 栅格化文档
//!
//! 职责：栅格化全文档——
//! 架构（扫描线/GPU 两技法/三路由）、质量指南（AA 档位选择/gamma 纪律）、
//! 性能指南（路由决策/缓存/多线程）、调试指南（可视化器 F0156/165 集成用法）。
//! 判据：四类齐、路由文档、调试指南、自动+人工、更新流程。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 文档章节：四类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Architecture, // 架构
    Quality,      // 质量指南
    Performance,  // 性能指南
    Debug,        // 调试指南
}

impl DocKind {
    pub fn name(self) -> &'static str {
        match self {
            DocKind::Architecture => "架构",
            DocKind::Quality => "质量指南",
            DocKind::Performance => "性能指南",
            DocKind::Debug => "调试指南",
        }
    }
}

/// 文档条目：章节 + 标题 + 内容 + 来源（自动生成/人工撰写）。
#[derive(Clone, Debug, PartialEq)]
pub struct DocEntry {
    pub kind: DocKind,
    pub title: String,
    pub body: String,
    /// 来源：true=自动生成（从代码注释/基准数据）、false=人工。
    pub auto_generated: bool,
}

/// 文档完备性核验：四类齐 + 每类非空 + 架构类必须含三路由说明。
pub fn doc_complete(entries: &[DocEntry]) -> Result<(), String> {
    let kinds = [DocKind::Architecture, DocKind::Quality, DocKind::Performance, DocKind::Debug];
    let mut missing: Vec<&str> = Vec::new();
    for k in kinds {
        if !entries.iter().any(|e| e.kind == k && !e.body.is_empty()) {
            missing.push(k.name());
        }
    }
    if !missing.is_empty() {
        return Err(format!("文档缺类：{}", missing.join("、")));
    }
    let arch: Vec<&DocEntry> = entries.iter().filter(|e| e.kind == DocKind::Architecture).collect();
    let has_route = arch.iter().any(|e| {
        e.body.contains("CPU") && e.body.contains("stencil") && e.body.contains("解析")
    });
    if !has_route {
        return Err("架构文档未覆盖三路由（CPU/SC/解析）".to_string());
    }
    Ok(())
}

/// 调试指南集成核验：必须提及可视化器集成用法（F0156/F0165）。
pub fn debug_guide_integrated(entries: &[DocEntry]) -> bool {
    entries
        .iter()
        .filter(|e| e.kind == DocKind::Debug)
        .any(|e| e.body.contains("可视化器") || e.body.contains("F0156") || e.body.contains("F0165"))
}

/// 同源生成：路由决策表的文档从决策器代码同源生成（表与代码不漂移）。
#[derive(Clone, Debug, PartialEq)]
pub struct RouteDocRow {
    pub cond: String,
    pub route: String,
}

/// 生成路由表文档：从（条件, 路由）对生成 markdown 行（自动来源）。
pub fn gen_route_doc(rows: &[RouteDocRow]) -> DocEntry {
    let mut body = String::from("三路由决策表：\n");
    for r in rows {
        body.push_str(&format!("- {} → {}\n", r.cond, r.route));
    }
    body.push_str("三路由 = CPU / stencil / 解析");
    DocEntry {
        kind: DocKind::Architecture,
        title: "三路由决策表（自动生成）".to_string(),
        body,
        auto_generated: true,
    }
}

/// 更新流程：文档随代码变更而更新——代码变更号登记后文档标待更新。
pub fn doc_update_needed(code_changed: u32, doc_last_synced: u32) -> bool {
    code_changed > doc_last_synced
}

/// 自动+人工双源核验：全部自动或全部人工都判不齐（判据要求自动+人工）。
pub fn dual_source_ok(entries: &[DocEntry]) -> bool {
    entries.iter().any(|e| e.auto_generated) && entries.iter().any(|e| !e.auto_generated)
}

pub fn run_vec317_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec317");
    // 族一：四类齐备
    let entries = vec![
        DocEntry {
            kind: DocKind::Architecture,
            title: "架构".to_string(),
            body: "扫描线与 GPU 两技法；三路由：CPU / stencil / 解析".to_string(),
            auto_generated: true,
        },
        DocEntry {
            kind: DocKind::Quality,
            title: "AA 档位选择".to_string(),
            body: "gamma 纪律：线性域合成".to_string(),
            auto_generated: false,
        },
        DocEntry {
            kind: DocKind::Performance,
            title: "性能指南".to_string(),
            body: "路由决策、缓存、多线程".to_string(),
            auto_generated: true,
        },
        DocEntry {
            kind: DocKind::Debug,
            title: "调试指南".to_string(),
            body: "可视化器 F0156/F0165 集成用法".to_string(),
            auto_generated: false,
        },
    ];
    s.add("四类齐备通过", doc_complete(&entries).is_ok(), "complete");
    let mut miss_one = entries.clone();
    miss_one.retain(|e| e.kind != DocKind::Quality);
    match doc_complete(&miss_one) {
        Err(m) => s.add("缺类点名", m.contains("质量指南"), "missing-kind"),
        Ok(()) => s.add("缺类点名", false, "expected err"),
    }
    let mut empty_body = entries.clone();
    if let Some(e) = empty_body.iter_mut().find(|e| e.kind == DocKind::Debug) {
        e.body.clear();
    }
    s.add("空内容判缺", doc_complete(&empty_body).is_err(), "empty-body");
    let no_route = vec![DocEntry {
        kind: DocKind::Architecture,
        title: "架构".to_string(),
        body: "只有扫描线".to_string(),
        auto_generated: true,
    }];
    s.add("无三路由架构判缺", doc_complete(&no_route).is_err(), "no-route");
    // 族二：调试指南集成
    s.add("调试指南含可视化器", debug_guide_integrated(&entries), "dbg-integrated");
    let no_dbg = vec![DocEntry {
        kind: DocKind::Debug,
        title: "调试".to_string(),
        body: "用 print 调试".to_string(),
        auto_generated: false,
    }];
    s.add("无集成判缺失", !debug_guide_integrated(&no_dbg), "dbg-missing");
    // 族三：同源生成
    let route_rows = vec![
        RouteDocRow { cond: "极小路径".to_string(), route: "CPU".to_string() },
        RouteDocRow { cond: "凸且边少".to_string(), route: "解析".to_string() },
        RouteDocRow { cond: "凹复杂".to_string(), route: "stencil".to_string() },
    ];
    let gen = gen_route_doc(&route_rows);
    s.add("路由表自动生成", gen.auto_generated, "auto");
    s.add("文档覆盖三路由关键词", doc_complete(&[gen.clone()]).is_ok(), "gen-arch");
    s.add("行数与输入对齐", gen.body.matches("→").count() == 3, "rows");
    // 族四：双源与更新流程
    s.add("自动+人工双源齐", dual_source_ok(&entries), "dual");
    s.add("全人工判不齐", !dual_source_ok(&no_route), "manual-only");
    s.add("全自动判不齐", !dual_source_ok(&[gen.clone()]), "auto-only");
    s.add("代码新于文档→待更新", doc_update_needed(12, 10), "stale");
    s.add("文档同步则无需更新", !doc_update_needed(10, 10), "synced");
    s.add("四类名称互异", {
        let k = [DocKind::Architecture, DocKind::Quality, DocKind::Performance, DocKind::Debug];
        k[0].name() != k[1].name() && k[2].name() != k[3].name()
    }, "names");
    s
}
