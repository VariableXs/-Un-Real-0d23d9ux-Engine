//! 深化层二 · F133 第三方图标包规范（2026-09-26 深化批次二）。
//!
//! 补深主册【设计细节】位深 32bit 与类别四族清单表（主册 G-D-08）：
//! 位深校验、四族类别表（系统/文件夹/文件类型/托盘）、命名法深化
//! （版本后缀语义）、导入向导分步状态机。

use crate::checks::CheckSet;
use crate::stareco::iconpack::PackReport;

// ---------------------------------------------------------------------------
// 四族类别清单表（主册【设计细节】固定表）
// ---------------------------------------------------------------------------

pub const CATEGORY_FAMILIES: [&str; 4] = ["system", "folder", "filetype", "tray"];

/// 每族必备图标的最小集（收口判据：四族清单表逐族核）。
pub const FAMILY_MUST: [(&str, [&str; 2]); 4] = [
    ("system", ["computer", "trash"]),
    ("folder", ["folder", "folder-open"]),
    ("filetype", ["file-generic", "file-image"]),
    ("tray", ["tray-volume", "tray-network"]),
];

/// 类别归属合法性：图标名必须可归四族之一（归不了 = 不收）。
pub fn category_of(name: &str) -> Option<&'static str> {
    for (family, must) in FAMILY_MUST {
        if must.contains(&name) {
            return Some(family);
        }
    }
    None
}

/// 四族完备性：每族至少一枚必备图标在包内。
pub fn families_covered(icon_names: &[&str]) -> alloc::vec::Vec<&'static str> {
    FAMILY_MUST
        .iter()
        .filter(|(_, must)| must.iter().any(|m| icon_names.contains(m)))
        .map(|(f, _)| *f)
        .collect()
}

// ---------------------------------------------------------------------------
// 位深校验：32bit（含 alpha）硬线
// ---------------------------------------------------------------------------

/// PNG 头色型位：6 = RGBA（32bit）、2 = RGB（24bit，缺 alpha 判红）。
pub fn bit_depth_ok(color_type: u8) -> bool {
    color_type == 6
}

/// 像素 alpha 抽检：全不透明也判红？不——32bit 格式成立即可
/// （alpha 值可为 255）；格式是 24bit 才是硬红。
pub fn alpha_channel_present(color_type: u8) -> bool {
    bit_depth_ok(color_type)
}

// ---------------------------------------------------------------------------
// 命名法深化：版本后缀语义（-2x 高清双倍率档）
// ---------------------------------------------------------------------------

/// 名字解析：基础名 + 可选倍率后缀（`@2x`）。
pub fn parse_name(full: &str) -> Result<(&str, u8), &'static str> {
    if let Some(base) = full.strip_suffix("@2x") {
        if base.is_empty() {
            return Err("倍率后缀前必须有基础名");
        }
        return Ok((base, 2));
    }
    if full.is_empty() {
        return Err("空名字");
    }
    Ok((full, 1))
}

/// 双倍率配套检查：基础档存在的图标应有 @2x 档（4K 管线配套——缺失只提醒不拒）。
pub fn missing_2x<'a>(names: &[&'a str]) -> alloc::vec::Vec<&'a str> {
    names
        .iter()
        .filter(|n| {
            let (base, scale) = parse_name(n).unwrap_or(("", 1));
            scale == 1 && !names.iter().any(|o| parse_name(o).map_or(false, |(b2, s2)| s2 == 2 && b2 == base))
        })
        .copied()
        .collect()
}

// ---------------------------------------------------------------------------
// 导入向导分步状态机：选包→校验→降级确认→导入
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportStage {
    Picked,
    Validated,
    DowngradeConfirmed,
    Imported,
}

/// 推进规则：必须逐段；有降级项时必须经过确认段才能导入。
pub fn import_advance(from: ImportStage, to: ImportStage, has_downgrades: bool) -> Result<ImportStage, &'static str> {
    let legal = matches!(
        (from, to),
        (ImportStage::Picked, ImportStage::Validated)
            | (ImportStage::Validated, ImportStage::DowngradeConfirmed)
            | (ImportStage::Validated, ImportStage::Imported)
            | (ImportStage::DowngradeConfirmed, ImportStage::Imported)
    );
    if !legal {
        return Err("非法向导步骤");
    }
    if from == ImportStage::Validated && to == ImportStage::Imported && has_downgrades {
        return Err("存在降级项：跳过确认直接导入 = 静默降级（红线）");
    }
    Ok(to)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F133E_TAG: &str = "stareco-F133-deep2";

pub fn run_f133_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F133E_TAG);

    // 四族清单表
    set.add("f133e families fixed", CATEGORY_FAMILIES.len() == 4, "四族固定");
    set.add(
        "f133e category map",
        category_of("computer") == Some("system") && category_of("tray-volume") == Some("tray"),
        "图标归类",
    );
    set.add("f133e category unknown", category_of("weird-icon").is_none(), "归不了族拒收");
    let names = ["computer", "folder", "file-generic", "tray-volume"];
    set.add(
        "f133e families covered",
        families_covered(&names) == CATEGORY_FAMILIES.to_vec(),
        "四族全覆盖",
    );
    set.add(
        "f133e families gap",
        families_covered(&["computer"]) == alloc::vec!["system"],
        "单族覆盖如实",
    );

    // 位深
    set.add("f133e rgba ok", bit_depth_ok(6), "RGBA 32bit 达标");
    set.add("f133e rgb reject", !bit_depth_ok(2), "RGB 24bit 缺 alpha 红线");
    set.add("f133e alpha present", alpha_channel_present(6), "alpha 抽检同源");

    // 命名倍率
    set.add("f133e name base", parse_name("trash") == Ok(("trash", 1)), "基础档解析");
    set.add("f133e name 2x", parse_name("trash@2x") == Ok(("trash", 2)), "双倍率解析");
    set.add("f133e name empty", parse_name("").is_err(), "空名拒绝");
    set.add("f133e name bare2x", parse_name("@2x").is_err(), "裸后缀拒绝");
    let pack = ["trash", "folder@2x"];
    set.add(
        "f133e missing 2x",
        missing_2x(&pack) == alloc::vec!["trash"],
        "缺 @2x 点名（提醒不拒）",
    );

    // 导入向导
    set.add("f133e step 1", import_advance(ImportStage::Picked, ImportStage::Validated, false).is_ok(), "选包→校验");
    set.add(
        "f133e step skip",
        import_advance(ImportStage::Picked, ImportStage::Imported, false).is_err(),
        "跳步拒绝",
    );
    set.add(
        "f133e downgrade gate",
        import_advance(ImportStage::Validated, ImportStage::Imported, true).is_err(),
        "有降级跳确认拒绝",
    );
    set.add(
        "f133e downgrade ok",
        import_advance(ImportStage::DowngradeConfirmed, ImportStage::Imported, true).is_ok(),
        "确认后可导",
    );
    set.add(
        "f133e clean path",
        import_advance(ImportStage::Validated, ImportStage::Imported, false).is_ok(),
        "无降级直导",
    );

    // 与基础层联动：齐套 manifest 过基础层校验（判据对账）
    let manifest = crate::stareco::iconpack::Manifest {
        name_ok: true,
        author_set: true,
        license_set: true,
        version_set: true,
        sizes: 0b1111,
        states: 0b111,
        categories: 0b1111,
        depth32: true,
    };
    let report: PackReport = crate::stareco::iconpack::validate(&manifest, &names);
    set.add("f133e official pass", report.pass(), "齐套包过基础层校验（对账）");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn family_table_shape() {
        assert_eq!(FAMILY_MUST.len(), CATEGORY_FAMILIES.len());
        for (f, must) in FAMILY_MUST {
            assert!(CATEGORY_FAMILIES.contains(&f));
            assert!(must.iter().all(|m| category_of(m) == Some(f)));
        }
    }

    #[test]
    fn bit_depth_other_types() {
        assert!(!bit_depth_ok(0)); // 灰度
        assert!(!bit_depth_ok(3)); // 调色板
        assert!(bit_depth_ok(6));
    }

    #[test]
    fn name_parse_all_2x() {
        let pack = ["a@2x", "b@2x"];
        assert!(missing_2x(&pack).is_empty()); // 全 2x 无基础档 = 不点名（各自独立档）
    }
}
