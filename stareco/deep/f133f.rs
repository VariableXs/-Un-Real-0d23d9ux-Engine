//! 深化层三 · F133 第三方图标包规范（2026-09-26 深化批次三）。
//!
//! 补深工程化工具面（主册 G-D-08）：图标包差异对比器（两包增删改）、
//! 重采样计划核对（升采样警示/降采样放行）、命名冲突检测、覆盖矩阵
//! 报表（4 尺寸 × 3 状态逐格）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 包差异对比器：两份清单 → 增/删/改
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct IconMeta {
    pub name: &'static str,
    /// 尺寸档位标记（32/48/96/256 → 0..3 的档序号）。
    pub size_class: u8,
    /// 状态位图：bit0 normal / bit1 pressed / bit2 disabled。
    pub states: u8,
}

/// 对比结论：新增（b 有 a 无）、缺失（a 有 b 无）、变化（同名义尺寸档或状态位不同）。
pub struct PackDiff {
    pub added: alloc::vec::Vec<&'static str>,
    pub removed: alloc::vec::Vec<&'static str>,
    pub changed: alloc::vec::Vec<&'static str>,
}

pub fn diff_packs(base: &[IconMeta], next: &[IconMeta]) -> PackDiff {
    let mut d = PackDiff {
        added: alloc::vec::Vec::new(),
        removed: alloc::vec::Vec::new(),
        changed: alloc::vec::Vec::new(),
    };
    for b in base {
        match next.iter().find(|n| n.name == b.name) {
            None => d.removed.push(b.name),
            Some(n) => {
                if n.size_class != b.size_class || n.states != b.states {
                    d.changed.push(b.name);
                }
            }
        }
    }
    for n in next {
        if !base.iter().any(|b| b.name == n.name) {
            d.added.push(n.name);
        }
    }
    d
}

// ---------------------------------------------------------------------------
// 重采样计划核对：缺档时邻近档补位的机器面（宽容导入诚实标注）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResampleKind {
    /// 同档直接引用。
    Identity,
    /// 降采样：面积平均，视觉安全。
    Downscale,
    /// 升采样：必须诚实标注（放大发糊——4K 红线）。
    Upscale,
}

/// 依据源档与目标档判定重采样类别。档序：0=32, 1=48, 2=96, 3=256。
pub fn resample_kind(from: u8, to: u8) -> Result<ResampleKind, &'static str> {
    if from > 3 || to > 3 {
        return Err("档序越界");
    }
    match from.cmp(&to) {
        core::cmp::Ordering::Equal => Ok(ResampleKind::Identity),
        core::cmp::Ordering::Greater => Ok(ResampleKind::Downscale),
        core::cmp::Ordering::Less => Ok(ResampleKind::Upscale),
    }
}

/// 升采样必须打标注（诚实线）：返回是否需要「来源档」说明。
pub fn needs_source_note(k: ResampleKind) -> bool {
    k == ResampleKind::Upscale
}

// ---------------------------------------------------------------------------
// 命名冲突检测：重名 + 保留前缀 + 合法字符（小写/数字/单中划线）
// ---------------------------------------------------------------------------

pub const RESERVED_PREFIXES: [&str; 3] = ["varix-", "star-", "sys-"];

pub fn name_clash(names: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    let mut bad: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for (i, n) in names.iter().enumerate() {
        let mut problem = false;
        // 重名
        for (j, m) in names.iter().enumerate() {
            if j > i && n == m {
                problem = true;
            }
        }
        // 保留前缀
        for p in RESERVED_PREFIXES.iter() {
            if n.starts_with(p) {
                problem = true;
            }
        }
        // 字符法：小写字母/数字/中划线，且不以中划线开头结尾
        if n.is_empty()
            || n.starts_with('-')
            || n.ends_with('-')
            || !n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            problem = true;
        }
        if problem && !bad.contains(n) {
            bad.push(n);
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// 覆盖矩阵：4 尺寸 × 3 状态逐格核对 → 缺格清单
// ---------------------------------------------------------------------------

/// 完整包 = 每图标 4 尺寸档 × 3 状态位全在。登记条目为
/// 「图标 × 尺寸档」粒度——按图标名分组核对，缺席档记 0xFF 哨兵。
pub fn coverage_gaps(metas: &[IconMeta]) -> alloc::vec::Vec<(&'static str, u8, u8)> {
    let mut gaps: alloc::vec::Vec<(&'static str, u8, u8)> = alloc::vec::Vec::new();
    let mut names: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for m in metas {
        if !names.contains(&m.name) {
            names.push(m.name);
        }
    }
    for name in names {
        for sc in 0..4u8 {
            match metas.iter().find(|m| m.name == name && m.size_class == sc) {
                None => gaps.push((name, sc, 0xFF)),
                Some(m) => {
                    for st in 0..3u8 {
                        if m.states & (1 << st) == 0 {
                            gaps.push((name, sc, st));
                        }
                    }
                }
            }
        }
    }
    gaps
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F133F_TAG: &str = "stareco-F133-deep3";

pub fn run_f133_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F133F_TAG);

    let base = [
        IconMeta { name: "folder", size_class: 2, states: 0b011 },
        IconMeta { name: "file", size_class: 1, states: 0b001 },
    ];
    let next = [
        IconMeta { name: "folder", size_class: 3, states: 0b011 },
        IconMeta { name: "drive", size_class: 2, states: 0b001 },
    ];

    // 包差异
    let d = diff_packs(&base, &next);
    set.add("f133f added", d.added == alloc::vec!["drive"], "新增检出");
    set.add("f133f removed", d.removed == alloc::vec!["file"], "缺失检出");
    set.add("f133f changed", d.changed == alloc::vec!["folder"], "升档变化检出");
    set.add("f133f noop", diff_packs(&base, &base).changed.is_empty(), "同包零差异");

    // 重采样判定
    set.add("f133f identity", resample_kind(2, 2) == Ok(ResampleKind::Identity), "同档直引");
    set.add("f133f down", resample_kind(3, 1) == Ok(ResampleKind::Downscale), "降采样放行");
    set.add("f133f up", resample_kind(0, 3) == Ok(ResampleKind::Upscale), "升采样识别");
    set.add("f133f out of range", resample_kind(4, 0).is_err(), "档序越界拒绝");
    set.add("f133f note rule", needs_source_note(ResampleKind::Upscale) && !needs_source_note(ResampleKind::Downscale), "升采样必须标注");

    // 命名
    let names = ["folder", "folder", "varix-logo", "GoodIcon", "-lead", "trail-", "ok-name-2"];
    let bad = name_clash(&names);
    set.add("f133f clash dup", bad.contains(&"folder"), "重名检出");
    set.add("f133f clash reserved", bad.contains(&"varix-logo"), "保留前缀检出");
    set.add("f133f clash case", bad.contains(&"GoodIcon"), "大写检出");
    set.add("f133f clash edge", bad.contains(&"-lead") && bad.contains(&"trail-"), "边界中划线检出");
    set.add("f133f clash clean", !bad.contains(&"ok-name-2"), "合法名不误伤");

    // 覆盖矩阵（满格包 = 同图标四档尺寸 × 全状态）
    let full = [
        IconMeta { name: "ok", size_class: 0, states: 0b111 },
        IconMeta { name: "ok", size_class: 1, states: 0b111 },
        IconMeta { name: "ok", size_class: 2, states: 0b111 },
        IconMeta { name: "ok", size_class: 3, states: 0b111 },
    ];
    set.add("f133f cov full", coverage_gaps(&full).is_empty(), "满格零缺");
    let gaps = coverage_gaps(&base);
    set.add("f133f cov size gap", gaps.contains(&("file", 3, 0xFF)), "缺档哨兵登记");
    set.add("f133f cov state gap", gaps.contains(&("file", 1, 1)), "缺状态检出");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn diff_and_resample() {
        let a = [IconMeta { name: "x", size_class: 1, states: 1 }];
        let b = [IconMeta { name: "x", size_class: 1, states: 7 }];
        let d = diff_packs(&a, &b);
        assert!(d.added.is_empty() && d.removed.is_empty());
        assert_eq!(d.changed, alloc::vec!["x"]);
        assert_eq!(resample_kind(1, 1), Ok(ResampleKind::Identity));
        assert!(resample_kind(7, 7).is_err());
    }

    #[test]
    fn coverage_sentinel_shape() {
        let m = [IconMeta { name: "a", size_class: 3, states: 0b000 }];
        let g = coverage_gaps(&m);
        // 档 3：状态 3 缺；档 0/1/2：整体缺哨兵。
        assert_eq!(g.len(), 6);
        assert!(g.contains(&("a", 3, 0)));
        assert!(g.contains(&("a", 3, 2)));
        assert!(g.contains(&("a", 0, 0xFF)));
        assert!(g.contains(&("a", 2, 0xFF)));
    }
}
