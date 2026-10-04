//! 深化层 · F143 星徽与品牌资产包（2026-09-26 回炉补深化）。
//!
//! 补深：资产版本清单（包随品牌版本更新）、LOGO 用法图解规范
//! （间距/底色常量）、滥用分类、资产包导出清单（vxtheme 容器清单）。

use crate::checks::CheckSet;
use crate::stareco::brandkit::{BrandKit, MonoVariant, LOGO_SIZES, NATIVE_4K_MIN_W};

// ---------------------------------------------------------------------------
// 资产版本清单
// ---------------------------------------------------------------------------

pub struct AssetVersion {
    pub brand_ver: u32,
    pub asset_count: u32,
    pub sha_note: &'static str,
}

/// 版本递增且资产数不减（品牌更新不许缩水）。
pub fn version_ladder_ok(versions: &[AssetVersion]) -> bool {
    versions.windows(2).all(|w| w[1].brand_ver > w[0].brand_ver && w[1].asset_count >= w[0].asset_count)
}

// ---------------------------------------------------------------------------
// LOGO 用法图解规范（间距/底色——品牌一致性的量化面）
// ---------------------------------------------------------------------------

/// 最小净空 = LOGO 高度的 25%（四周）。
pub fn min_clearspace(logo_h: u16) -> u16 {
    logo_h / 4
}

/// 允许的底色：白、黑、VARIX 强调色（十六进制）；其余底色拒用。
pub fn background_allowed(hex: u32) -> bool {
    matches!(hex, 0xFFFFFF | 0x000000 | 0x1E6BEB)
}

// ---------------------------------------------------------------------------
// 滥用分类（先礼后兵的分类学）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Misuse {
    /// 篡改主色/比例。
    Recolor,
    /// 暗示官方背书。
    Endorsement,
    /// 无关产品宣传。
    OffProduct,
}

/// 分类 → 沟通话术等级（Endorsement 最重——先礼的话也最正式）。
pub fn misuse_severity(m: Misuse) -> u8 {
    match m {
        Misuse::Recolor => 1,
        Misuse::OffProduct => 2,
        Misuse::Endorsement => 3,
    }
}

// ---------------------------------------------------------------------------
// 资产包导出清单（vxtheme 容器清单）
// ---------------------------------------------------------------------------

/// 导出清单：LOGO 五尺寸 + 单色三变体 + 壁纸 N 张 4K 的文件名清单。
pub fn export_manifest(kit: &BrandKit, wallpapers: u16) -> alloc::vec::Vec<alloc::string::String> {
    let _ = kit; // 形态齐套性由 BrandKit::complete 守门，本层只列清单
    let mut out = alloc::vec::Vec::new();
    for s in LOGO_SIZES.iter() {
        out.push(alloc::format!("logo/varix-{}.png", s));
    }
    for (v, name) in [(MonoVariant::White, "white"), (MonoVariant::Black, "black"), (MonoVariant::Accent, "accent")] {
        let _ = v;
        out.push(alloc::format!("logo/mono/varix-mono-{}.png", name));
    }
    for i in 1..=wallpapers {
        out.push(alloc::format!("wallpaper/wp-{}-{}x{}.png", i, NATIVE_4K_MIN_W, NATIVE_4K_MIN_W / 2 * 9 / 8));
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F143D_TAG: &str = "stareco-F143-deep";

pub fn run_f143_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F143D_TAG);

    // 版本阶梯
    let vs = [
        AssetVersion { brand_ver: 1, asset_count: 18, sha_note: "a" },
        AssetVersion { brand_ver: 2, asset_count: 21, sha_note: "b" },
    ];
    set.add(
        "f143d version ladder ok",
        version_ladder_ok(&vs),
        "递增且不缩水",
    );
    let shrink = [
        AssetVersion { brand_ver: 1, asset_count: 21, sha_note: "a" },
        AssetVersion { brand_ver: 2, asset_count: 18, sha_note: "b" },
    ];
    set.add("f143d shrink caught", !version_ladder_ok(&shrink), "品牌更新不许缩水");

    // 用法图解
    set.add(
        "f143d clearspace 25%",
        min_clearspace(128) == 32 && min_clearspace(64) == 16,
        "高度四分之一",
    );
    set.add(
        "f143d background whitelist",
        background_allowed(0xFFFFFF) && background_allowed(0x1E6BEB) && !background_allowed(0x00FF00),
        "三底色白名单",
    );

    // 滥用分级
    set.add(
        "f143d misuse severity ladder",
        misuse_severity(Misuse::Recolor) < misuse_severity(Misuse::OffProduct)
            && misuse_severity(Misuse::OffProduct) < misuse_severity(Misuse::Endorsement),
        "背书最重",
    );

    // 导出清单
    let mut kit = BrandKit::new();
    for s in LOGO_SIZES.iter() {
        kit.add_logo_size(*s).ok();
    }
    for v in [MonoVariant::White, MonoVariant::Black, MonoVariant::Accent] {
        kit.add_mono(v);
    }
    let manifest = export_manifest(&kit, 10);
    set.add(
        "f143d export manifest count",
        manifest.len() == 5 + 3 + 10,
        "5 logo + 3 mono + 10 壁纸",
    );
    set.add(
        "f143d wallpaper 4k name",
        manifest.iter().any(|f| f.contains("3840x2160")),
        "4K 原生命名",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn clearspace_zero_guard() {
        assert_eq!(min_clearspace(3), 0); // 极小资产净空为 0——合法下限
    }
}
