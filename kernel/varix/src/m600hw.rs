//! m600hw — VARIX-M600 AI-20 硬件广度域 (F476~F500)
//!
//! 驱动兼容矩阵/GPU 广谱支持/触控屏全集/指纹读卡器/摄像头广谱/
//! 打印生态/扫描仪协议/蓝牙全档案/USB-C 现代化/雷电热插拔/
//! SD 卡舱/键盘灯效/触摸板手势广谱/传感器融合/电池广谱/
//! HDMI/DP 花式/声卡广谱/网卡固件舱/外接显卡实验/虚拟机检测优雅/
//! 双系统共存礼仪/U 盘引导工坊/ARM 移植侦察/RISC-V 预研/硬件年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F476 — 驱动兼容矩阵：设备 × 驱动状态总账
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    Bound,
    Fallback,
    Missing,
}

#[derive(Clone, Copy)]
pub struct DriverEntry {
    pub vendor_id: u16,
    pub device_id: u16,
    pub status: DriverStatus,
}

/// 矩阵合格：没有任何设备处于 Missing（Fallback 允许但计损）。
pub fn driver_matrix_ok(devs: &[DriverEntry]) -> bool {
    devs.iter().all(|d| d.status != DriverStatus::Missing)
}

pub fn driver_coverage_permille(devs: &[DriverEntry]) -> u16 {
    if devs.is_empty() {
        return 0;
    }
    let bound = devs.iter().filter(|d| d.status == DriverStatus::Bound).count();
    (bound * 1000 / devs.len()) as u16
}

// ===========================================================================
// F477 — GPU 广谱支持：核心特性掩码 + 无头自检
// ===========================================================================

pub const GPU_FEAT_BLIT: u32 = 1 << 0;
pub const GPU_FEAT_RECT: u32 = 1 << 1;
pub const GPU_FEAT_COMPOSITE: u32 = 1 << 2;
pub const GPU_FEAT_YUV: u32 = 1 << 3;

pub const GPU_CORE_MASK: u32 =
    GPU_FEAT_BLIT | GPU_FEAT_RECT | GPU_FEAT_COMPOSITE | GPU_FEAT_YUV;

pub fn gpu_ok(feature_mask: u32, headless_selftest: bool) -> bool {
    feature_mask & GPU_CORE_MASK == GPU_CORE_MASK && headless_selftest
}

// ===========================================================================
// F478 — 触控屏全集：多点 + 防误触
// ===========================================================================

pub const TOUCH_MIN_POINTS: u8 = 5;

pub fn touchscreen_ok(max_points: u8, palm_reject: bool) -> bool {
    max_points >= TOUCH_MIN_POINTS && palm_reject
}

// ===========================================================================
// F479 — 指纹读卡器：特征点数量 + 误识率上限
// ===========================================================================

pub const FINGERPRINT_MIN_MINUTIAE: u16 = 12;
pub const FINGERPRINT_MAX_FAR_PERMILLE: u16 = 1; // 0.1%

pub fn fingerprint_ok(minutiae: u16, far_permille: u16) -> bool {
    minutiae >= FINGERPRINT_MIN_MINUTIAE && far_permille <= FINGERPRINT_MAX_FAR_PERMILLE
}

// ===========================================================================
// F480 — 摄像头广谱：分辨率下限 + 格式数
// ===========================================================================

pub const CAMERA_MIN_W: u32 = 1280;
pub const CAMERA_MIN_H: u32 = 720;
pub const CAMERA_MIN_FORMATS: u8 = 2;

pub fn camera_broad_ok(max_w: u32, max_h: u32, formats: u8) -> bool {
    max_w >= CAMERA_MIN_W && max_h >= CAMERA_MIN_H && formats >= CAMERA_MIN_FORMATS
}

// ===========================================================================
// F481 — 打印生态：IPP 标准 + 页型覆盖
// ===========================================================================

pub const PRINTER_MIN_PAGE_SIZES: u8 = 3;

pub fn printer_ok(ipp: bool, page_sizes: u8) -> bool {
    ipp && page_sizes >= PRINTER_MIN_PAGE_SIZES
}

// ===========================================================================
// F482 — 扫描仪协议：DPI 区间覆盖
// ===========================================================================

pub const SCANNER_MIN_DPI_MAX: u16 = 300;
pub const SCANNER_MAX_DPI_MIN: u16 = 600;

pub fn scanner_ok(min_dpi: u16, max_dpi: u16) -> bool {
    min_dpi <= SCANNER_MIN_DPI_MAX && max_dpi >= SCANNER_MAX_DPI_MIN
}

// ===========================================================================
// F483 — 蓝牙全档案：四大必需档案齐备
// ===========================================================================

pub const BT_A2DP: u8 = 0;
pub const BT_HID: u8 = 1;
pub const BT_HFP: u8 = 2;
pub const BT_LE: u8 = 3;
pub const BT_PAN: u8 = 4;

pub fn bt_profiles_ok(profiles: &[u8]) -> bool {
    [BT_A2DP, BT_HID, BT_HFP, BT_LE].iter().all(|p| profiles.contains(p))
}

// ===========================================================================
// F484 — USB-C 现代化：DRP 双角色 + PD 功率 + 数据速率
// ===========================================================================

pub const USBC_MIN_PD_WATTS: u16 = 60;
pub const USBC_MIN_GBPS: u8 = 5;

pub fn usbc_ok(drp: bool, pd_watts: u16, data_gbps: u8) -> bool {
    drp && pd_watts >= USBC_MIN_PD_WATTS && data_gbps >= USBC_MIN_GBPS
}

// ===========================================================================
// F485 — 雷电热插拔：3 秒内上线 + 授权策略
// ===========================================================================

pub const TB_HOTPLUG_MAX_MS: u32 = 3000;

pub fn tb_hotplug_ok(plug_ms: u32, user_auth: bool) -> bool {
    plug_ms <= TB_HOTPLUG_MAX_MS && user_auth
}

// ===========================================================================
// F486 — SD 卡舱：速度等级 + 容量区间
// ===========================================================================

pub const SD_MIN_CLASS: u8 = 10;
pub const SD_MAX_CAPACITY_GB: u32 = 2048;

pub fn sd_ok(speed_class: u8, capacity_gb: u32) -> bool {
    speed_class >= SD_MIN_CLASS && capacity_gb >= 1 && capacity_gb <= SD_MAX_CAPACITY_GB
}

// ===========================================================================
// F487 — 键盘灯效：级数 + 亮度记忆
// ===========================================================================

pub const KBD_LIGHT_MIN_LEVELS: u8 = 8;

pub fn kbd_light_ok(levels: u8, brightness_recall: bool) -> bool {
    levels >= KBD_LIGHT_MIN_LEVELS && brightness_recall
}

// ===========================================================================
// F488 — 触摸板手势广谱：必备手势掩码
// ===========================================================================

pub const GEST_SCROLL2: u32 = 1 << 0;
pub const GEST_PINCH: u32 = 1 << 1;
pub const GEST_SWIPE3: u32 = 1 << 2;
pub const GEST_TAP: u32 = 1 << 3;

pub const GESTURE_REQUIRED: u32 = GEST_SCROLL2 | GEST_PINCH | GEST_SWIPE3;

pub fn touchpad_gestures_ok(mask: u32) -> bool {
    mask & GESTURE_REQUIRED == GESTURE_REQUIRED
}

// ===========================================================================
// F489 — 传感器融合：双高频源 + 融合输出
// ===========================================================================

pub const FUSION_MIN_HZ: u16 = 100;

pub fn fusion_ok(accel_hz: u16, gyro_hz: u16, fused_output: bool) -> bool {
    fused_output && accel_hz >= FUSION_MIN_HZ && gyro_hz >= FUSION_MIN_HZ
}

// ===========================================================================
// F490 — 电池广谱：健康度 + 循环寿命
// ===========================================================================

pub const BATTERY_MIN_HEALTH_PERMILLE: u16 = 800;
pub const BATTERY_MAX_CYCLES: u16 = 1000;

pub fn battery_ok(health_permille: u16, cycles: u16) -> bool {
    health_permille >= BATTERY_MIN_HEALTH_PERMILLE && cycles <= BATTERY_MAX_CYCLES
}

// ===========================================================================
// F491 — HDMI/DP 花式：整数带宽判定（单位 0.1 Gbps）
// ===========================================================================

/// need = pixel_mhz × bits_per_px / 100_000（0.1 Gbps）
/// have = lanes × gbps_tenths_per_lane × 8 / 10（8b/10b 编码效率）
pub fn display_link_ok(pixel_mhz: u32, bits_per_px: u32, lanes: u8, gbps_tenths_per_lane: u32) -> bool {
    let need = pixel_mhz * bits_per_px / 100_000;
    let have = lanes as u32 * gbps_tenths_per_lane * 8 / 10;
    have >= need
}

// ===========================================================================
// F492 — 声卡广谱：采样率/声道/位深下限
// ===========================================================================

pub const AUDIO_MIN_HZ: u32 = 48_000;
pub const AUDIO_MIN_CHANNELS: u8 = 2;
pub const AUDIO_MIN_BITS: u8 = 16;

pub fn audio_ok(max_hz: u32, channels: u8, bit_depth: u8) -> bool {
    max_hz >= AUDIO_MIN_HZ && channels >= AUDIO_MIN_CHANNELS && bit_depth >= AUDIO_MIN_BITS
}

// ===========================================================================
// F493 — 网卡固件舱：固件版本下限 + 崩溃转储
// ===========================================================================

pub fn nic_fw_ok(fw_major: u16, fw_minor: u16, min_major: u16, crash_dump: bool) -> bool {
    (fw_major * 100 + fw_minor) >= min_major * 100 && crash_dump
}

// ===========================================================================
// F494 — 外接显卡实验：链路余量 permille 门槛
// ===========================================================================

pub const EGPU_MIN_LINK_PERMILLE: u16 = 700;

pub fn egpu_feasible(link_permille: u16, experimental: bool) -> bool {
    experimental && link_permille >= EGPU_MIN_LINK_PERMILLE
}

// ===========================================================================
// F495 — 虚拟机检测优雅：识别降级但保留六成能力
// ===========================================================================

pub const VM_MIN_KEPT_PERMILLE: u16 = 600;

pub fn vm_graceful(detected_vm: bool, features_kept: u16, features_total: u16) -> bool {
    if !detected_vm {
        return true;
    }
    features_total > 0
        && (features_kept as u32 * 1000 / features_total as u32) >= VM_MIN_KEPT_PERMILLE as u32
}

// ===========================================================================
// F496 — 双系统共存礼仪：不碰别人分区 + 关快启 + 明示风险
// ===========================================================================

pub fn dualboot_safe(touches_other_os: bool, faststartup_off: bool, warned_user: bool) -> bool {
    !touches_other_os && faststartup_off && warned_user
}

// ===========================================================================
// F497 — U 盘引导工坊：写入即校验 + 引导签名
// ===========================================================================

pub fn usb_boot_ok(bytes_written: u32, bytes_verified: u32, bootable_sig: bool) -> bool {
    bootable_sig && bytes_written > 0 && bytes_verified == bytes_written
}

// ===========================================================================
// F498 — ARM 移植侦察：页宽 + 中断控制器 + 启动协议
// ===========================================================================

pub fn arm_page_supported(page_size: u32) -> bool {
    page_size == 4096 || page_size == 65536
}

pub fn arm_recon_ok(page_size: u32, gic_present: bool, uefi_boot: bool) -> bool {
    arm_page_supported(page_size) && gic_present && uefi_boot
}

// ===========================================================================
// F499 — RISC-V 预研：SBI 版本 + 必备扩展掩码
// ===========================================================================

pub const RISCV_EXT_SSTC: u32 = 1 << 0; // 超级用户定时器
pub const RISCV_EXT_SVPBMT: u32 = 1 << 1; // 页内存类型
pub const RISCV_EXT_ZICBOM: u32 = 1 << 2; // 缓存块管理

pub const RISCV_REQUIRED_MASK: u32 = RISCV_EXT_SSTC | RISCV_EXT_SVPBMT;
pub const RISCV_MIN_SBI_MAJOR: u16 = 2;

pub fn riscv_recon_ok(sbi_major: u16, ext_mask: u32) -> bool {
    sbi_major >= RISCV_MIN_SBI_MAJOR && ext_mask & RISCV_REQUIRED_MASK == RISCV_REQUIRED_MASK
}

// ===========================================================================
// F500 — 硬件年报：兼容性叙事存档
// ===========================================================================

pub const HW_REPORT_SECTIONS: [&str; 4] = ["matrix", "blessed-list", "quirks", "learnings"];

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600hw_checks() -> CheckSet {
    let mut set = CheckSet::new("m600hw");

    // F476 驱动兼容矩阵
    let devs = [
        DriverEntry { vendor_id: 0x8086, device_id: 0x1234, status: DriverStatus::Bound },
        DriverEntry { vendor_id: 0x10EC, device_id: 0x8168, status: DriverStatus::Fallback },
        DriverEntry { vendor_id: 0x1234, device_id: 0x1111, status: DriverStatus::Missing },
    ];
    set.add("F476 matrix ok w/o missing", driver_matrix_ok(&devs[..2]), "no missing");
    set.add("F476 missing caught", !driver_matrix_ok(&devs), "missing refuses");
    let cov = driver_coverage_permille(&devs[..2]);
    set.add("F476 coverage permille", cov == 500, "bound tally");

    // F477 GPU 广谱
    set.add(
        "F477 gpu broad",
        gpu_ok(GPU_CORE_MASK | GEST_TAP, true)
            && !gpu_ok(GPU_CORE_MASK & !GPU_FEAT_YUV, true)
            && !gpu_ok(GPU_CORE_MASK, false),
        "core mask+smoke",
    );

    // F478 触控屏
    set.add(
        "F478 touchscreen",
        touchscreen_ok(10, true) && !touchscreen_ok(2, true) && !touchscreen_ok(10, false),
        "5-point+palm",
    );

    // F479 指纹
    set.add(
        "F479 fingerprint",
        fingerprint_ok(40, 1) && !fingerprint_ok(11, 1) && !fingerprint_ok(40, 2),
        "minutiae+far",
    );

    // F480 摄像头
    set.add(
        "F480 camera broad",
        camera_broad_ok(1920, 1080, 3) && !camera_broad_ok(640, 480, 3) && !camera_broad_ok(1920, 1080, 1),
        "720p+formats",
    );

    // F481 打印
    set.add(
        "F481 printer",
        printer_ok(true, 4) && !printer_ok(false, 4) && !printer_ok(true, 2),
        "ipp+pages",
    );

    // F482 扫描仪
    set.add(
        "F482 scanner",
        scanner_ok(150, 1200) && !scanner_ok(600, 1200) && !scanner_ok(150, 300),
        "dpi span",
    );

    // F483 蓝牙
    let bt = [BT_A2DP, BT_HID, BT_HFP, BT_LE, BT_PAN];
    set.add("F483 bt profiles", bt_profiles_ok(&bt), "all required");
    let bt_gap = [BT_A2DP, BT_HID, BT_LE];
    set.add("F483 bt gap caught", !bt_profiles_ok(&bt_gap), "hfp missing");

    // F484 USB-C
    set.add(
        "F484 usb-c",
        usbc_ok(true, 65, 10) && !usbc_ok(false, 65, 10) && !usbc_ok(true, 15, 10),
        "drp+pd60+5g",
    );

    // F485 雷电
    set.add(
        "F485 thunderbolt",
        tb_hotplug_ok(1800, true) && !tb_hotplug_ok(4000, true) && !tb_hotplug_ok(1800, false),
        "3s+auth",
    );

    // F486 SD 卡
    set.add(
        "F486 sd bay",
        sd_ok(10, 512) && !sd_ok(4, 512) && !sd_ok(10, 4096) && !sd_ok(10, 0),
        "class10+range",
    );

    // F487 键盘灯效
    set.add(
        "F487 keyboard light",
        kbd_light_ok(16, true) && !kbd_light_ok(4, true) && !kbd_light_ok(16, false),
        "levels+recall",
    );

    // F488 触摸板手势
    set.add(
        "F488 touchpad gestures",
        touchpad_gestures_ok(GESTURE_REQUIRED | GEST_TAP)
            && !touchpad_gestures_ok(GEST_SCROLL2 | GEST_SWIPE3),
        "required set",
    );

    // F489 传感器融合
    set.add(
        "F489 sensor fusion",
        fusion_ok(200, 200, true) && !fusion_ok(50, 200, true) && !fusion_ok(200, 200, false),
        "100hz+fused",
    );

    // F490 电池
    set.add(
        "F490 battery",
        battery_ok(920, 300) && !battery_ok(700, 300) && !battery_ok(920, 1200),
        "health+cycles",
    );

    // F491 HDMI/DP 带宽
    // 4K60: 3840×2160×60 ≈ 497.7 Mpx/s，24bpp → 119（0.1 Gbps）；HBR2×4 ≈ 172 可用
    set.add(
        "F491 display link 4k60",
        display_link_ok(497_712, 24, 4, 54) && !display_link_ok(497_712, 24, 2, 54),
        "4-lane hbr2 ok, 2-lane short",
    );
    set.add(
        "F491 display link 8k30",
        !display_link_ok(995_424, 30, 4, 54),
        "8k needs more lanes",
    );

    // F492 声卡
    set.add(
        "F492 audio broad",
        audio_ok(192_000, 8, 24) && !audio_ok(44_100, 2, 16) && !audio_ok(48_000, 1, 8),
        "48k/stereo/16bit",
    );

    // F493 网卡固件
    set.add(
        "F493 nic firmware",
        nic_fw_ok(3, 20, 2, true) && nic_fw_ok(2, 0, 2, true) && !nic_fw_ok(1, 99, 2, true) && !nic_fw_ok(3, 20, 2, false),
        "version+dum",
    );

    // F494 外接显卡
    set.add(
        "F494 egpu experiment",
        egpu_feasible(750, true) && !egpu_feasible(650, true) && !egpu_feasible(750, false),
        "permille+flag",
    );

    // F495 虚拟机检测
    set.add(
        "F495 vm graceful",
        vm_graceful(false, 0, 0)
            && vm_graceful(true, 7, 10)
            && !vm_graceful(true, 5, 10)
            && !vm_graceful(true, 7, 0),
        "keep>=600‰",
    );

    // F496 双系统共存
    set.add(
        "F496 dual boot",
        dualboot_safe(false, true, true)
            && !dualboot_safe(true, true, true)
            && !dualboot_safe(false, false, true)
            && !dualboot_safe(false, true, false),
        "hands off+warn",
    );

    // F497 U 盘引导
    set.add(
        "F497 usb boot",
        usb_boot_ok(4096, 4096, true)
            && !usb_boot_ok(4096, 4000, true)
            && !usb_boot_ok(4096, 4096, false)
            && !usb_boot_ok(0, 0, true),
        "write==verify",
    );

    // F498 ARM 侦察
    set.add(
        "F498 arm recon",
        arm_recon_ok(4096, true, true)
            && arm_recon_ok(65536, true, true)
            && !arm_recon_ok(8192, true, true)
            && !arm_recon_ok(4096, false, true)
            && !arm_recon_ok(4096, true, false),
        "page+gic+uefi",
    );

    // F499 RISC-V 预研
    set.add(
        "F499 riscv recon",
        riscv_recon_ok(2, RISCV_REQUIRED_MASK | RISCV_EXT_ZICBOM)
            && !riscv_recon_ok(1, RISCV_REQUIRED_MASK)
            && !riscv_recon_ok(2, RISCV_EXT_SSTC),
        "sbi2+ext",
    );

    // F500 硬件年报
    set.add("F500 hw report", HW_REPORT_SECTIONS.len() == 4, "archived");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f476_coverage_math() {
        let devs = [
            DriverEntry { vendor_id: 1, device_id: 1, status: DriverStatus::Bound },
            DriverEntry { vendor_id: 1, device_id: 2, status: DriverStatus::Bound },
            DriverEntry { vendor_id: 1, device_id: 3, status: DriverStatus::Fallback },
            DriverEntry { vendor_id: 1, device_id: 4, status: DriverStatus::Bound },
        ];
        assert_eq!(driver_coverage_permille(&devs), 750);
        let empty: [DriverEntry; 0] = [];
        assert_eq!(driver_coverage_permille(&empty), 0);
        assert!(driver_matrix_ok(&devs));
    }

    #[test]
    fn f483_profile_requirements() {
        let full = [BT_A2DP, BT_HID, BT_HFP, BT_LE];
        assert!(bt_profiles_ok(&full));
        assert!(!bt_profiles_ok(&[BT_A2DP]));
        assert!(!bt_profiles_ok(&[BT_A2DP, BT_HID, BT_HFP, BT_PAN])); // 缺 BT_LE
    }

    #[test]
    fn f491_bandwidth_integer_math() {
        // 4K60 24bpp 需 119（0.1 Gbps）；HBR2 单 lane 5.4 Gbps×0.8×4 = 172
        assert!(display_link_ok(497_712, 24, 4, 54));
        // 2 lane 只有 86 不足
        assert!(!display_link_ok(497_712, 24, 2, 54));
        // 1080p60 ≈ 31，单 lane 即可
        assert!(display_link_ok(124_416, 24, 1, 54));
    }

    #[test]
    fn f495_vm_degradation() {
        assert!(vm_graceful(false, 0, 0)); // 裸机不受限
        assert!(vm_graceful(true, 6, 10)); // 恰好 600‰
        assert!(!vm_graceful(true, 599, 1000)); // 599‰
        assert!(!vm_graceful(true, 1, 0)); // 除零防御
    }

    #[test]
    fn f498_f499_arch_recon() {
        assert!(arm_page_supported(4096) && arm_page_supported(65536));
        assert!(!arm_page_supported(16384));
        assert!(riscv_recon_ok(3, RISCV_EXT_ZICBOM | RISCV_EXT_SSTC | RISCV_EXT_SVPBMT));
        assert!(!riscv_recon_ok(2, RISCV_EXT_ZICBOM));
    }

    #[test]
    fn f500_domain_selfcheck_all_pass() {
        let set = run_m600hw_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
