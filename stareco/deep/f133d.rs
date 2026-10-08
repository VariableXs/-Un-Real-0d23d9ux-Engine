//! 深化层 · F133 第三方图标包规范（2026-09-26 回炉补深化）。
//!
//! 补深：包目录树遍历校验（真实文件清单模拟）、类别四族清单表逐族
//! 必备图标、重采样计划执行器（输出明确的重采样工单）、校验报告
//! 本地化文案、manifest 与目录一致性对拍。

use crate::checks::CheckSet;
use crate::stareco::iconpack::{validate, Manifest, PackReport, CATEGORIES, MANDATORY_SIZE, SIZE_STEPS};

// ---------------------------------------------------------------------------
// 类别四族清单表（每族必备图标——清单表固定）
// ---------------------------------------------------------------------------

/// 每族必备图标名（缺失 = 缺图标警告级，不阻断导入）。
pub const FAMILY_REQUIRED: [(&str, [&str; 2]); 4] = [
    ("system", ["system-gear", "system-power"]),
    ("folder", ["folder-open", "folder-closed"]),
    ("filetype", ["file-generic", "file-image"]),
    ("tray", ["tray-net", "tray-volume"]),
];

/// 逐族清点：缺失的必备图标名清单。
pub fn family_gaps(icon_names: &[&str]) -> alloc::vec::Vec<(&'static str, &'static str)> {
    let mut out = alloc::vec::Vec::new();
    for (family, required) in FAMILY_REQUIRED.iter() {
        for r in required.iter() {
            if !icon_names.contains(r) {
                out.push((*family, *r));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 重采样计划执行器：缺失档 → 明确工单（源档/目标档/方式）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResampleOrder {
    pub target: u16,
    pub source: u16,
    /// downscale（高→低，品质优）/ upscale（低→高，标注降级）。
    pub upscale: bool,
}

/// 把 PackReport 的重采样建议展开为执行工单（含方向标注）。
pub fn resample_orders(report: &PackReport) -> alloc::vec::Vec<ResampleOrder> {
    report
        .resample
        .iter()
        .map(|&(target, source)| ResampleOrder {
            target,
            source,
            // 低→高为放大（品质降，须标注）；高→低为缩小（品质优）
            upscale: source < target,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// manifest ↔ 目录一致性对拍
// ---------------------------------------------------------------------------

/// manifest 声明的类别位图与实际提供的图标类别前缀对拍。
/// 图标命名惯例：`<类别>-<名>`，类别必须是四族之一。
pub fn manifest_dir_consistent(m: &Manifest, icon_names: &[&str]) -> Result<(), &'static str> {
    // 实际出现的类别集合
    let mut seen: u8 = 0;
    for n in icon_names {
        let mut matched = false;
        for (i, cat) in CATEGORIES.iter().enumerate() {
            if n.starts_with(cat) {
                seen |= 1 << i;
                matched = true;
                break;
            }
        }
        if !matched {
            return Err("图标名不含合法类别前缀（system/folder/filetype/tray）");
        }
    }
    // 声明了但目录里没有的类别 = manifest 撒谎
    if m.categories & seen != m.categories {
        return Err("manifest 声明了目录中不存在的类别");
    }
    // 档位声明与目录对拍由尺寸子目录层负责（本层管类别一致性）。
    Ok(())
}

// ---------------------------------------------------------------------------
// 本地化报错（校验报告 → 中文工单）
// ---------------------------------------------------------------------------

pub fn localize_report(report: &PackReport) -> alloc::vec::Vec<alloc::string::String> {
    report
        .issues
        .iter()
        .map(|i| alloc::format!("问题[{:?}]：{}", i.code as u32, i.detail))
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F133D_TAG: &str = "stareco-F133-deep";

pub fn run_f133_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F133D_TAG);

    // 族清单表
    let full = ["system-gear", "system-power", "folder-open", "folder-closed", "file-generic", "file-image", "tray-net", "tray-volume"];
    set.add("f133d no family gaps", family_gaps(&full).is_empty(), "4×2 all present");
    let gaps = family_gaps(&["system-gear"]);
    set.add("f133d gaps listed", gaps.len() == 7 && gaps[0].0 == "system", "缺失逐条列出");

    // manifest↔目录一致性
    let m = Manifest {
        name_ok: true,
        author_set: true,
        license_set: true,
        version_set: true,
        sizes: 0b1111,
        states: 0b111,
        categories: 0b0011, // system+folder
        depth32: true,
    };
    set.add(
        "f133d consistent pack",
        manifest_dir_consistent(&m, &["system-gear", "folder-open", "folder-closed"]).is_ok(),
        "declared==seen",
    );
    set.add(
        "f133d lying manifest caught",
        manifest_dir_consistent(&m, &["system-gear"]).is_err(),
        "declared folder missing",
    );
    set.add(
        "f133d bad prefix caught",
        manifest_dir_consistent(&m, &["myicon-x"]).is_err(),
        "no legal category prefix",
    );

    // 尺寸阶梯不变式（深化层复诵：256 永在阶梯顶）
    set.add("f133d 256 is top", SIZE_STEPS[3] == MANDATORY_SIZE, "4k ladder");

    // 重采样工单：方向标注（放大须标降级）
    let partial = Manifest { sizes: 0b1000, ..m };
    let orders = resample_orders(&validate(&partial, &["system-gear", "folder-open"]));
    set.add(
        "f133d resample orders with direction",
        orders.len() == 3 && orders.iter().all(|o| !o.upscale) && orders[0].source == MANDATORY_SIZE,
        "256→低档为缩小（品质优）；低→高才标放大降级",
    );

    // 本地化报错
    let broken = Manifest {
        name_ok: false,
        author_set: false,
        license_set: false,
        version_set: false,
        sizes: 0,
        states: 0,
        categories: 0,
        depth32: false,
    };
    let report = validate(&broken, &[]);
    let localized = localize_report(&report);
    set.add(
        "f133d localized tickets",
        localized.len() >= 8 && localized.iter().all(|s| s.starts_with("问题[")),
        "逐条中文工单",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn family_table_complete() {
        assert_eq!(FAMILY_REQUIRED.len(), 4);
        for (f, req) in FAMILY_REQUIRED.iter() {
            assert!(CATEGORIES.contains(f));
            assert_eq!(req.len(), 2);
        }
    }
}
