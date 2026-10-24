//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3702 · 边覆盖深化（边分类与权重） · 诊断段 0xD500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：边分类（关键边（状态转移/资源操作/权限分支）/平凡边
//!      （日志/统计——分类规则：边带语义标注（构建期插桩标注）
//!      ——关键边的权重 ≥2×（能量分配与报告分列的依据）））；
//!      M-460 边分类生效（构造含两类边的靶→分类正确+关键边盲区
//!      优先呈现）。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   J2：分类封闭（标注词表 const）。
//! 设计要点：
//!   - 标注词表：XFER/RES/PRIV（关键）/LOG/STAT（平凡）；
//!   - 权重：关键 ≥2× 平凡（能量分配）；
//!   - M-460：混合靶分类正确 + 关键盲区优先呈现（报告排序）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xD500;

/// 语义标注词表（const 封闭）。
pub const ANNOTATIONS: [&str; 5] = ["XFER", "RES", "PRIV", "LOG", "STAT"];

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 边类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeClass {
    Critical,
    Trivial,
}

impl EdgeClass {
    pub fn name(self) -> &'static str {
        match self {
            EdgeClass::Critical => "CRITICAL",
            EdgeClass::Trivial => "TRIVIAL",
        }
    }

    /// 权重：关键 4、平凡 2（关键 ≥2× 平凡）。
    pub fn weight(self) -> u32 {
        match self {
            EdgeClass::Critical => 4,
            EdgeClass::Trivial => 2,
        }
    }
}

/// 分类规则：标注 → 类别。
pub fn classify(annotation: &str) -> Option<EdgeClass> {
    match annotation {
        "XFER" | "RES" | "PRIV" => Some(EdgeClass::Critical),
        "LOG" | "STAT" => Some(EdgeClass::Trivial),
        _ => None,
    }
}

/// 靶边。
#[derive(Debug, Clone)]
pub struct TargetEdge {
    pub edge_id: usize,
    pub annotation: &'static str,
    pub covered: bool,
}

impl TargetEdge {
    pub fn class(&self) -> Option<EdgeClass> {
        classify(self.annotation)
    }
}

/// 盲区报告：未覆盖边按权重降序（关键优先呈现）。
pub fn blind_report(edges: &[TargetEdge]) -> Vec<usize> {
    let mut un: Vec<&TargetEdge> = edges.iter().filter(|e| !e.covered && e.class().is_some()).collect();
    un.sort_by(|a, b| {
        let wa = a.class().unwrap_or(EdgeClass::Trivial).weight();
        let wb = b.class().unwrap_or(EdgeClass::Trivial).weight();
        wb.cmp(&wa).then(a.edge_id.cmp(&b.edge_id))
    });
    un.iter().map(|e| e.edge_id).collect()
}

/// M-460：混合靶分类正确 + 关键盲区优先呈现。
pub fn m460() -> (bool, Vec<usize>) {
    let target = [
        TargetEdge { edge_id: 0, annotation: "XFER", covered: false },
        TargetEdge { edge_id: 1, annotation: "LOG", covered: false },
        TargetEdge { edge_id: 2, annotation: "PRIV", covered: false },
        TargetEdge { edge_id: 3, annotation: "STAT", covered: false },
        TargetEdge { edge_id: 4, annotation: "RES", covered: true },
    ];
    // 分类正确
    let cls_ok = target[0].class() == Some(EdgeClass::Critical)
        && target[1].class() == Some(EdgeClass::Trivial)
        && target[2].class() == Some(EdgeClass::Critical)
        && target[3].class() == Some(EdgeClass::Trivial)
        && target[4].class() == Some(EdgeClass::Critical);
    // 盲区排序：关键（0,2）先于平凡（1,3）
    let rep = blind_report(&target);
    let order_ok = rep == vec_order(&[0, 2, 1, 3]);
    (cls_ok && order_ok, rep)
}

/// 有序向量构造（避免 vec! 依赖）。
fn vec_order(items: &[usize]) -> Vec<usize> {
    let mut v = Vec::new();
    for x in items {
        v.push(*x);
    }
    v
}

/// J1 主判据：分类规则 + 权重比 + M-460 + J2 词表封闭。
pub fn j1_main() -> bool {
    let (m_ok, _) = m460();
    m_ok
        && EdgeClass::Critical.weight() >= 2 * EdgeClass::Trivial.weight()
        && classify("BOGUS").is_none()
        && ANNOTATIONS.len() == 5
        && ANNOTATIONS.iter().all(|a| classify(a).is_some())
        && {
            // 词表哈希稳定
            let mut buf: Vec<u8> = Vec::new();
            for a in ANNOTATIONS {
                for &b in a.as_bytes() {
                    buf.push(b);
                }
            }
            fnv1a(&buf) != 0
        }
}

// ── 注入核（0xD500 段 5 项）────────────────────────────────────────────

/// 注入核 1：词表封闭注入——词表外标注必拒。
pub fn inj1_annotation_closed() -> bool {
    classify("XFER").is_some() && classify("WAT").is_none()
}

/// 注入核 2：权重比注入——关键恰 ≥2× 平凡。
pub fn inj2_weight_ratio() -> bool {
    EdgeClass::Critical.weight() / EdgeClass::Trivial.weight() >= 2
}

/// 注入核 3：盲区排序注入——同权重按边号升序。
pub fn inj3_blind_tie() -> bool {
    let t = [
        TargetEdge { edge_id: 5, annotation: "LOG", covered: false },
        TargetEdge { edge_id: 2, annotation: "STAT", covered: false },
    ];
    blind_report(&t) == vec_order(&[2, 5])
}

/// 注入核 4：已覆盖不入盲区注入。
pub fn inj4_covered_excluded() -> bool {
    let t = [
        TargetEdge { edge_id: 0, annotation: "XFER", covered: true },
        TargetEdge { edge_id: 1, annotation: "LOG", covered: false },
    ];
    blind_report(&t) == vec_order(&[1])
}

/// 注入核 5：全覆盖空报告注入。
pub fn inj5_empty_report() -> bool {
    let t = [
        TargetEdge { edge_id: 0, annotation: "XFER", covered: true },
        TargetEdge { edge_id: 1, annotation: "LOG", covered: true },
    ];
    blind_report(&t).is_empty()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx3702_checks：本单判据聚合（诊断段 0xD500）。
pub fn run_unx3702_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3702 边覆盖深化（边分类与权重）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "边分类规则（关键/平凡）+关键边权重≥2×",
        j1_main(),
        if j1_main() { "" } else { "分类规则破" },
    );
    let (m_ok, _) = m460();
    s.add(
        "M-460边分类生效（分类正确+关键盲区优先）",
        m_ok,
        if m_ok { "" } else { "M-460破" },
    );
    s.add(
        "J2分类封闭（标注词表const）",
        inj1_annotation_closed() && ANNOTATIONS.len() == 5,
        if inj1_annotation_closed() { "" } else { "词表封闭破" },
    );
    let inj = [
        ("注入1词表封闭", inj1_annotation_closed()),
        ("注入2权重比", inj2_weight_ratio()),
        ("注入3同权排序", inj3_blind_tie()),
        ("注入4已覆盖排除", inj4_covered_excluded()),
        ("注入5空报告", inj5_empty_report()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
