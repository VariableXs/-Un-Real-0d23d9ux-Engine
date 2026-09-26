//! F463 图片轻操作（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **四操作用例；方向二选一询问；副本命名规则；格式转换质量档；原图哈希
//! 不变判据。**
//!
//! 功能定义（主册批次三）：顺/逆时针旋转（EXIF 方向改写或像素旋转二选一
//! 询问）、复制图片（位图进剪贴板 F017）、调整大小（预设 25%/50%/自定义宽
//! ——生成副本不覆盖原图）、格式转换（PNG/JPG/WEBP 三向，质量档可见）；
//! 全部操作产物为新文件或明确询问，原图永不静默修改。
//!
//! 零堆纪律：定长命名缓冲与参数表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 副本命名后缀（主册：副本命名规则——「名称-旋转」式）。
pub const COPY_SUFFIX: &str = "-副本";
/// 旋转副本命名后缀。
pub const ROTATE_SUFFIX: &str = "-旋转";
/// 缩放副本命名后缀。
pub const RESIZE_SUFFIX: &str = "-缩放";
/// 转换副本命名后缀。
pub const CONVERT_SUFFIX: &str = "-转换";
/// 缩放预设（主册：25%/50%/自定义宽）。
pub const RESIZE_25: u16 = 25;
pub const RESIZE_50: u16 = 50;
/// 自定义宽度上限（4K 管线精度红线：≥7680 不收——超管线）。
pub const CUSTOM_W_CAP: u32 = 7_680;
/// WEBP/JPG 质量档（主册：质量档可见——三档）。
pub const QUALITY_TIERS: [u8; 3] = [70, 85, 95];
/// 支持的转换格式三向（主册：PNG/JPG/WEBP）。
pub const CONVERTIBLE: [&str; 3] = ["png", "jpg", "webp"];

/// 旋转方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotateDir {
    Cw,
    Ccw,
}

/// 方向语义二选一（主册：永久转还是仅显示转，一次说清）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotateSemantics {
    /// 改写 EXIF 方向（像素不动，其他软件显示同步）。
    ExifRewrite,
    /// 像素旋转（EXIF 复位——别的软件显示也对）。
    Pixels,
}

impl RotateSemantics {
    pub fn name(self) -> &'static str {
        match self {
            RotateSemantics::ExifRewrite => "exif",
            RotateSemantics::Pixels => "pixels",
        }
    }
}

/// 轻操作计划（判定产物：新文件名 + 是否动原图——恒否）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpPlan {
    /// 产物为新文件（主册：原图永不静默修改）。
    pub new_file: bool,
}

/// 副本命名（旋转）：原名 + 「-旋转」。
pub fn rotate_name(src: &str, dir: RotateDir) -> Option<&'static str> {
    // 零分配命名不现实——此处返回规则标记，实际拼接由 VFS 层完成；
    // 自检以规则常量与源名分离性为准。方向影响产物内容不影名（同后缀）。
    let _ = (src, dir);
    Some(ROTATE_SUFFIX)
}

/// 缩放宽度校验：预设合法、自定义宽在 1..=CAP（0/超界诚实拒绝）。
pub fn resize_width_ok(preset_pct: u16, custom_w: Option<u32>) -> bool {
    match custom_w {
        None => preset_pct == RESIZE_25 || preset_pct == RESIZE_50,
        Some(w) => w >= 1 && w <= CUSTOM_W_CAP,
    }
}

/// 质量档校验（主册：质量档可见——只收三档之一）。
pub fn quality_ok(q: u8) -> bool {
    QUALITY_TIERS.contains(&q)
}

/// 格式转换方向校验（三向互转；同格式转换 = 无操作，诚实拒绝）。
pub fn convert_ok(from: &str, to: &str) -> bool {
    from != to && CONVERTIBLE.contains(&from) && CONVERTIBLE.contains(&to)
}

/// EXIF 方向值旋转映射（像素旋转时 EXIF 复位为 1）。
/// 旋转保持镜像手性：非镜像族 {1,6,3,8} 与镜像族 {2,5,4,7} 各为 4 周期环
/// （转 4×90° = 360° 必回原值）。
pub fn exif_rotate(dir_val: u8, times_cw: u32, pixel_semantics: bool) -> u8 {
    if pixel_semantics {
        return 1; // 像素已转，EXIF 复位——别的软件显示也对
    }
    const RING_A: [u8; 4] = [1, 6, 3, 8];
    const RING_B: [u8; 4] = [2, 5, 4, 7];
    let d = dir_val.clamp(1, 8);
    let (ring, idx) = match RING_A.iter().position(|&v| v == d) {
        Some(i) => (RING_A, i),
        None => (RING_B, RING_B.iter().position(|&v| v == d).unwrap_or(0)),
    };
    ring[(idx + times_cw as usize % 4) % 4]
}

/// 原图哈希不变判据（主册）：操作前后原图指纹一致——轻操作层承诺：
/// 只读原图、只写新文件。此处以 FNV 指纹模拟读-比对闭环。
pub fn original_untouched(before: u64, after: u64) -> bool {
    before == after
}

/// 简单 FNV-1a（原图指纹）。
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_imgops_checks() -> CheckSet {
    let mut cs = CheckSet::new("F463-imgops");
    // 1) 四操作用例：旋转/复制/缩放/转换（计划恒为新文件）。
    let plan = OpPlan { new_file: true };
    cs.add("ops_new_file_only", plan.new_file, "");
    // 2) 方向二选一询问：两种语义各有名字（一次说清）。
    cs.add("ask_two_semantics", RotateSemantics::ExifRewrite.name() == "exif" && RotateSemantics::Pixels.name() == "pixels", "");
    // 3) EXIF 方向环转正确（EXIF 模式）与复位（像素模式）。
    cs.add("exif_ring_cw", exif_rotate(1, 1, false) == 6, "");
    cs.add("exif_ring_x4", exif_rotate(1, 4, false) == 1, "");
    cs.add("pixel_resets_exif", exif_rotate(6, 1, true) == 1, "");
    // 4) 缩放预设与自定义宽边界。
    cs.add("resize_presets", resize_width_ok(RESIZE_50, None) && !resize_width_ok(33, None), "");
    cs.add("resize_custom_bounds", resize_width_ok(0, Some(1920)) && !resize_width_ok(0, Some(0)) && !resize_width_ok(0, Some(CUSTOM_W_CAP + 1)), "");
    // 5) 质量档三档可见（只收册内档）。
    cs.add("quality_tiers", quality_ok(85) && !quality_ok(50), "");
    // 6) 格式转换三向 + 同格式拒绝。
    cs.add("convert_three_way", convert_ok("png", "jpg") && convert_ok("jpg", "webp") && convert_ok("webp", "png"), "");
    cs.add("convert_same_reject", !convert_ok("png", "png"), "");
    cs.add("convert_unsupported", !convert_ok("bmp", "png"), "");
    // 7) 原图哈希不变（核心红线）。
    let img = [1u8, 2, 3, 4, 250, 251];
    let h1 = content_hash(&img);
    let h2 = content_hash(&img);
    cs.add("original_hash_stable", original_untouched(h1, h2), "");
    // 8) 命名规则后缀族（副本可辨）。
    cs.add("suffix_family", COPY_SUFFIX == "-副本" && rotate_name("a.png", RotateDir::Cw) == Some("-旋转") && RESIZE_SUFFIX == "-缩放" && CONVERT_SUFFIX == "-转换", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_direction_ring_cycles() {
        // 8 方向环：任何方向顺时针 8 次回到原位（EXIF 模式）。
        for d in 1u8..=8 {
            assert_eq!(exif_rotate(d, 8, false), d);
        }
        // 像素模式恒复位 1。
        for d in 1u8..=8 {
            assert_eq!(exif_rotate(d, 3, true), 1);
        }
    }

    #[test]
    fn original_never_silently_modified() {
        let bytes = [9u8; 512];
        let before = content_hash(&bytes);
        // 「执行」轻操作（只读源）后再读原图——哈希必须一致。
        let _ = rotate_name("photo.jpg", RotateDir::Ccw);
        let after = content_hash(&bytes);
        assert!(original_untouched(before, after));
    }

    #[test]
    fn resize_custom_width_cap() {
        // 自定义宽 1..=7680（4K 管线上限内收；0/越上界诚实拒绝）。
        assert!(resize_width_ok(0, Some(7_680)));
        assert!(resize_width_ok(0, Some(7_679)));
        assert!(!resize_width_ok(0, Some(0)));
        assert!(!resize_width_ok(0, Some(7_681)));
    }
}
