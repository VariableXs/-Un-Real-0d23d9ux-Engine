//! UNX-F0212 · 回归对比（本版 vs 上版镜像）
//!
//! 锚点判据（施工书原文）：UNX-F0212-J1 跨版本报告对比表生成，
//! 字段差异全部显性列出，无静默漂移。
//!
//! 设计：两版报告（同 schema 字段表）→ 差分器（按字段序双指针
//! 对齐：相等 / 变更 / 新增 / 删除四类）→ 差异清单全显性 → 静默
//! 漂移红线：两版字段名集合的对称差必须全部出现在差异清单中
//! （漏报一个即红）。
//!
//! 错误码段：0x2C40（全仓普查后独占分配，AI-WBX1022）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0212_VERSION: &str = "F0212-regdiff-v1";

/// 诊断码段基址（UNX-F0212 独占）。
pub const SEG_BASE: u16 = 0x2C40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 字段缺失于单侧且未入清单。
    SilentDrift = SEG_BASE + 1,
    /// 版本号缺失。
    NoVersion = SEG_BASE + 2,
    /// 自比必须零差异。
    SelfDiff = SEG_BASE + 3,
    /// 空报告。
    EmptyReport = SEG_BASE + 4,
    /// 差异分类错（值同却标变更）。
    Misclassified = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 差异分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// 同名异值。
    Changed,
    /// 仅本版有。
    Added,
    /// 仅上版有。
    Removed,
}

/// 一条差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    /// 字段名。
    pub field: String,
    /// 分类。
    pub kind: DiffKind,
    /// 上版值（Added 时为空）。
    pub old: String,
    /// 本版值（Removed 时为空）。
    pub new: String,
}

/// 报告（字段表 + 版本号）。
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// 版本号。
    pub version: String,
    /// 字段（名, 值），按登记序。
    pub fields: Vec<(String, String)>,
}

impl Report {
    /// 构造。
    pub fn new(version: &str, fields: &[(&str, &str)]) -> Result<Self, u16> {
        if version.is_empty() {
            return Err(E::NoVersion.code());
        }
        if fields.is_empty() {
            return Err(E::EmptyReport.code());
        }
        Ok(Report {
            version: String::from(version),
            fields: fields
                .iter()
                .map(|(k, v)| (String::from(*k), String::from(*v)))
                .collect(),
        })
    }
}

/// 差分器。
pub struct RegressDiff;

impl RegressDiff {
    /// 生成差异清单（全部显性）。
    pub fn diff(old: &Report, new: &Report) -> Result<Vec<Diff>, u16> {
        if old.fields.is_empty() || new.fields.is_empty() {
            return Err(E::EmptyReport.code());
        }
        let mut out = Vec::new();
        // Added / Changed：以本版为基准扫。
        for (k, v) in new.fields.iter() {
            match old.fields.iter().find(|(ok, _)| ok == k) {
                Some((_, ov)) => {
                    if ov != v {
                        out.push(Diff {
                            field: k.clone(),
                            kind: DiffKind::Changed,
                            old: ov.clone(),
                            new: v.clone(),
                        });
                    }
                }
                None => out.push(Diff {
                    field: k.clone(),
                    kind: DiffKind::Added,
                    old: String::new(),
                    new: v.clone(),
                }),
            }
        }
        // Removed：以上版为基准扫。
        for (k, v) in old.fields.iter() {
            if !new.fields.iter().any(|(nk, _)| nk == k) {
                out.push(Diff {
                    field: k.clone(),
                    kind: DiffKind::Removed,
                    old: v.clone(),
                    new: String::new(),
                });
            }
        }
        // 静默漂移红线：两侧独有字段数分别 ≡ Added / Removed 条数。
        let added_count = out
            .iter()
            .filter(|d| d.kind == DiffKind::Added)
            .count();
        let removed_count = out
            .iter()
            .filter(|d| d.kind == DiffKind::Removed)
            .count();
        let only_in_new = new
            .fields
            .iter()
            .filter(|(k, _)| !old.fields.iter().any(|(ok, _)| ok == k))
            .count();
        let only_in_old = old
            .fields
            .iter()
            .filter(|(k, _)| !new.fields.iter().any(|(nk, _)| nk == k))
            .count();
        if added_count != only_in_new || removed_count != only_in_old {
            return Err(E::SilentDrift.code());
        }
        Ok(out)
    }
}

/// J1 自证（判据原文：对比表生成，字段差异全部显性，无静默漂移）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：上版 5 字段，本版改 1、加 1、删 1、同 2。
    let old = Report::new(
        "v1.0",
        &[
            ("boot-ok", "yes"),
            ("heap-size", "64MiB"),
            ("gpu-probe", "done"),
            ("fs-mount", "ok"),
            ("legacy-field", "gone"),
        ],
    )?;
    let new = Report::new(
        "v1.1",
        &[
            ("boot-ok", "yes"),
            ("heap-size", "128MiB"), // Changed
            ("gpu-probe", "done"),
            ("fs-mount", "ok"),
            ("new-field", "added"), // Added
        ],
    )?;

    // 二、J1 主体：三类差异各恰一条，字段全部显性。
    let ds = RegressDiff::diff(&old, &new)?;
    if ds.len() != 3 {
        return Err(E::SilentDrift.code());
    }
    let has = |k: &str, kind: DiffKind| {
        ds.iter()
            .any(|d| d.field == k && d.kind == kind)
    };
    if !has("heap-size", DiffKind::Changed)
        || !has("new-field", DiffKind::Added)
        || !has("legacy-field", DiffKind::Removed)
    {
        return Err(E::SilentDrift.code());
    }
    let ch = match ds.iter().find(|d| d.field == "heap-size") {
        Some(d) => d,
        None => return Err(E::Misclassified.code()),
    };
    if ch.old != "64MiB" || ch.new != "128MiB" {
        return Err(E::Misclassified.code());
    }

    // 三、自比零差异（自检红线）。
    if !RegressDiff::diff(&old, &old)?.is_empty() {
        return Err(E::SelfDiff.code());
    }

    // 四、红线逐项：版本缺失 / 空报告 / 分类错判。
    if Report::new("", &[("a", "b")]).is_ok() {
        return Err(E::NoVersion.code());
    }
    if Report::new("v", &[]).is_ok() {
        return Err(E::EmptyReport.code());
    }
    // 值相同却试图判变更 → 差分器不产该条（Misclassified 红线）。
    let same_a = Report::new("a", &[("k", "1")])?;
    let same_b = Report::new("b", &[("k", "1")])?;
    if !RegressDiff::diff(&same_a, &same_b)?.is_empty() {
        return Err(E::Misclassified.code());
    }

    // 五、判别式段位对拍。
    if E::SilentDrift.code() != SEG_BASE + 1 || E::Misclassified.code() != SEG_BASE + 5 {
        return Err(E::SilentDrift.code());
    }
    Ok(())
}
