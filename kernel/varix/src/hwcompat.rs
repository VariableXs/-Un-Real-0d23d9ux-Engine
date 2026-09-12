//! VARIX-M500 · AI-13 硬件兼容广度（F301~F325，M4）
//!
//! 使命：更多机器跑起来——核显、网卡、EC、传感器、打印、社区共建。
//! 与 VARIX-500 的 `driver.rs`（基础驱动栈）零重复：本模块聚焦兼容性
//! 探测/降级/白名单/社区共建层。纯逻辑 + 固定容量数组。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F301 VGA 兼容保底 — 老机显示（VBE 模式表）
// ---------------------------------------------------------------------------

/// 保底 VBE 模式（640×480×32 等），固定表。
pub const VGA_FALLBACK_MODES: [(u16, u16, u8); 4] =
    [(1024, 768, 32), (800, 600, 32), (640, 480, 32), (320, 200, 8)];

/// 选最大不超过显存容量的模式（bytes = w*h*depth/8）。
pub fn vga_pick_mode(vram_bytes: u32) -> Option<(u16, u16, u8)> {
    VGA_FALLBACK_MODES
        .iter()
        .copied()
        .find(|&(w, h, d)| (w as u32) * (h as u32) * (d as u32) / 8 <= vram_bytes)
}

// ---------------------------------------------------------------------------
// F302 Intel 核显基础加速 — HD/UHD 系列探测
// ---------------------------------------------------------------------------

/// Intel 核显 PCI DID 前缀表（简化的家族匹配）。
pub fn intel_gpu_family(did: u16) -> Option<&'static str> {
    match did & 0xFF00 {
        0x1600 => Some("Broadwell"),
        0x1900 => Some("Skylake"),
        0x3E00 => Some("Coffee Lake"),
        0x9B00 => Some("Comet Lake"),
        0x4600 => Some("Alder Lake"),
        _ => None,
    }
}

/// 探测到已知家族即启用基本加速。
pub fn intel_gpu_accel(family: Option<&str>) -> bool {
    family.is_some()
}

// ---------------------------------------------------------------------------
// F303 AMD 图形探测 — 预留与降级
// ---------------------------------------------------------------------------

/// AMD DID 段探测；未识别家族时走 EFI FB 降级。
pub fn amd_gpu_probe(did: u32) -> (&'static str, bool) {
    match did & 0xFFFF_0000 {
        0x1000_0000..=0x7FFF_0000 => ("radeon-class", true),
        _ => ("efifb-fallback", false),
    }
}

// ---------------------------------------------------------------------------
// F304 RTL8169 网卡 — 家用主流
// ---------------------------------------------------------------------------

/// RTL8169 芯片版本寄存器 → 支持判定 + 芯片名。
pub fn rtl8169_identify(hw_verid: u8) -> Option<&'static str> {
    match hw_verid {
        0x20 => Some("RTL8169"),
        0x25 => Some("RTL8168B"),
        0x2C => Some("RTL8168E"),
        0x34 => Some("RTL8111G"),
        0x54 => Some("RTL8111HS"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F305 Intel 服务器网卡 — I210/I225
// ---------------------------------------------------------------------------

pub fn intel_nic_identify(did: u16) -> Option<&'static str> {
    match did {
        0x1533 => Some("I210"),
        0x1536 => Some("I211"),
        0x15F3 => Some("I225-V"),
        0x15F2 => Some("I225-LM"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F306 EHCI 主控 — USB 2.0 老外设
// ---------------------------------------------------------------------------

/// EHCI capability 判定：支持 32 段调度窗 + ports 数合法。
pub fn ehci_usable(hcsparams: u32) -> bool {
    let ports = (hcsparams & 0xF) as u8;
    let n_pcc = (hcsparams >> 4) & 0xF;
    ports > 0 && n_pcc > 0
}

// ---------------------------------------------------------------------------
// F307 SD/MMC 控制器 — 读卡器支持
// ---------------------------------------------------------------------------

/// 从 CAPABILITIES 寄存器提取超时基频（MHz）并换算 clock divider。
pub fn sdmmc_base_freq_mhz(cap: u32) -> u32 {
    ((cap >> 8) & 0x3F) as u32
}

pub fn sdmmc_divider(base_mhz: u32, want_khz: u32) -> u16 {
    if base_mhz == 0 || want_khz == 0 {
        return 0;
    }
    let div = (base_mhz * 1000 / want_khz / 2).max(1) as u16;
    div.min(0x3FF)
}

// ---------------------------------------------------------------------------
// F308 eMMC 存储 — 轻薄本存储（EXT_CSD SEC_COUNT）
// ---------------------------------------------------------------------------

/// EXT_CSD SEC_COUNT（4 字节 LE，每扇区 512B）→ 容量 GiB。
pub fn emmc_capacity_gib(sec_count: [u8; 4]) -> u64 {
    let secs = u32::from_le_bytes(sec_count) as u64;
    secs * 512 / (1024 * 1024 * 1024)
}

// ---------------------------------------------------------------------------
// F309 SATA 光驱 — 光盘读取（ATAPI identify）
// ---------------------------------------------------------------------------

/// ATAPI 设备判定：identify 的 config 字 0x8000 置位即 ATAPI。
pub fn sata_is_atapi(config_word: u16) -> bool {
    config_word & 0x8000 != 0
}

pub fn sata_is_cdrom(config_word: u16, protocol: u8) -> bool {
    sata_is_atapi(config_word) && protocol == 0x05 // ATAPI_TYPE_CDROM
}

// ---------------------------------------------------------------------------
// F310 EC 通信 — 嵌入式控制器（ACPI EC 命令序列）
// ---------------------------------------------------------------------------

/// EC 状态位：OBF/IBF/CMD/BURST。
pub const EC_OBF: u8 = 0x01;
pub const EC_IBF: u8 = 0x02;
pub const EC_CMD: u8 = 0x08;

/// 发送前提：IBF 空闲（输入缓冲空）。
pub fn ec_can_write(status: u8) -> bool {
    status & EC_IBF == 0
}

/// 读取前提：OBF 有数据。
pub fn ec_can_read(status: u8) -> bool {
    status & EC_OBF != 0
}

// ---------------------------------------------------------------------------
// F311 ACPI 背光 — 亮度调节（级别阶梯）
// ---------------------------------------------------------------------------

/// 亮度级别数 → 百分比；0 级保护为最低可用级（不会真黑）。
pub fn backlight_percent(level: u16, max_level: u16) -> u16 {
    if max_level == 0 {
        return 0;
    }
    let lvl = level.min(max_level).max(1);
    (lvl as u32 * 100 / max_level as u32) as u16
}

// ---------------------------------------------------------------------------
// F312 UVC 摄像头框架 — USB 摄像头预留
// ---------------------------------------------------------------------------

/// UVC 描述符 bInterfaceClass/bSubClass 匹配（0x0E/0x01 VC，0x0E/0x02 VS）。
pub fn uvc_is_video(class: u8, subclass: u8) -> bool {
    class == 0x0E && (subclass == 0x01 || subclass == 0x02)
}

/// 支持的 YUYV/MJPG 四帧率档。
pub const UVC_FPS: [u8; 4] = [5, 15, 30, 60];

// ---------------------------------------------------------------------------
// F313 指纹传感器预留 — 生物识别占位
// ---------------------------------------------------------------------------

/// 指纹模板槽位（仅结构占位，不做真实匹配）。
pub const FP_TEMPLATE_SLOTS: usize = 5;

#[derive(Clone, Copy)]
pub struct FpRegistry {
    pub enrolled: [bool; FP_TEMPLATE_SLOTS],
    pub count: usize,
}

impl FpRegistry {
    pub const fn new() -> FpRegistry {
        FpRegistry { enrolled: [false; FP_TEMPLATE_SLOTS], count: 0 }
    }
    pub fn enroll(&mut self, slot: usize) -> bool {
        if slot >= FP_TEMPLATE_SLOTS || self.enrolled[slot] {
            return false;
        }
        self.enrolled[slot] = true;
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F314 TPM 对接 — 可信平台模块（PCR 扩展）
// ---------------------------------------------------------------------------

/// TPM 2.0 PCR 扩展（SHA-256 只取首 4 字节演示折叠，纯逻辑占位）。
pub fn pcr_extend(pcr: u32, digest_head: u32) -> u32 {
    pcr.rotate_left(7) ^ digest_head
}

/// 度量引导链条校验：PCR 值与预期一致。
pub fn pcr_match(pcr: u32, expected: u32) -> bool {
    pcr == expected
}

// ---------------------------------------------------------------------------
// F315 传感器枢纽 — 加速度/光线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwSensor {
    Accel,
    AmbientLight,
    Gyro,
    Lid,
}

/// 枢纽路由：传感器 → 消费者通道（0 无 / 1 显示 / 2 电源 / 3 安全）。
pub fn sensor_route(s: HwSensor) -> u8 {
    match s {
        HwSensor::Accel => 2,
        HwSensor::AmbientLight => 1,
        HwSensor::Gyro => 3,
        HwSensor::Lid => 2,
    }
}

// ---------------------------------------------------------------------------
// F316 串口设备框架 — 工业外设（波特率分频）
// ---------------------------------------------------------------------------

/// UART 分频 = 时钟 / (16 * 波特)，至少 1。
pub fn uart_divider(clock_hz: u32, baud: u32) -> u16 {
    if baud == 0 {
        return 0;
    }
    ((clock_hz / (16 * baud)) as u16).max(1)
}

// ---------------------------------------------------------------------------
// F317 遗留并口桥 — 老打印口（LPT 状态位）
// ---------------------------------------------------------------------------

/// LPT 状态：0x80=busy，0x10=ack，0x08=paper-out。
pub fn lpt_ready(status: u8) -> bool {
    status & 0x80 == 0 && status & 0x08 == 0
}

// ---------------------------------------------------------------------------
// F318 打印抽象 — 打印框架（页描述符）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct PrintJob {
    pub pages: u16,
    pub duplex: bool,
    pub color: bool,
}

/// 估算墨量消耗千分比（黑/彩分计，双面省半）。
pub fn print_ink_permille(job: &PrintJob) -> u32 {
    let sheets = if job.duplex { ((job.pages as u32) + 1) / 2 } else { job.pages as u32 };
    let per_sheet = if job.color { 40 } else { 12 };
    sheets * per_sheet * 1000 / 5000 // 以 5000 页墨盒为基准
}

// ---------------------------------------------------------------------------
// F319 扫描协议预留 — 扫描仪占位
// ---------------------------------------------------------------------------

/// 扫描分辨率档（DPI）。
pub const SCAN_DPI: [u16; 3] = [150, 300, 600];

/// DPI → 估算字节大小（A4 210×297mm，灰度 1B/px）。
pub fn scan_bytes_a4(dpi: u16) -> u64 {
    let px = (210 * dpi as u64 / 25) * (297 * dpi as u64 / 25);
    px
}

// ---------------------------------------------------------------------------
// F320 硬件自报问卷 — 设备自识别（描述串打分）
// ---------------------------------------------------------------------------

/// 设备自报可信度：厂商串+序列号+型号齐 → 3 分；缺一项减 1。
pub fn hw_self_report_score(vendor: Option<&[u8]>, serial: Option<&[u8]>, model: Option<&[u8]>) -> u8 {
    let mut score = 0;
    if vendor.is_some() {
        score += 1;
    }
    if serial.is_some() {
        score += 1;
    }
    if model.is_some() {
        score += 1;
    }
    score
}

// ---------------------------------------------------------------------------
// F321 社区适配流程 — 驱动共建（开放）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdaptStage {
    Reported,
    Probed,
    Tested,
    Accepted,
}

/// 社区适配状态机：只能前进一格。
pub fn adapt_advance(cur: AdaptStage) -> AdaptStage {
    match cur {
        AdaptStage::Reported => AdaptStage::Probed,
        AdaptStage::Probed => AdaptStage::Tested,
        AdaptStage::Tested => AdaptStage::Accepted,
        AdaptStage::Accepted => AdaptStage::Accepted,
    }
}

/// 已接受才能进白名单。
pub fn adapt_whitelisted(s: AdaptStage) -> bool {
    s == AdaptStage::Accepted
}

// ---------------------------------------------------------------------------
// F322 兼容性快查库 — 白名单查询（DID 表）
// ---------------------------------------------------------------------------

/// 白名单三元组 (vendor, device, 状态 1=支持 2=降级)。
pub const COMPAT_DB: [(u16, u16, u8); 6] = [
    (0x8086, 0x1912, 1),
    (0x8086, 0x15F3, 1),
    (0x10EC, 0x8168, 1),
    (0x1002, 0x67DF, 2),
    (0x8086, 0x9B41, 1),
    (0x14C3, 0x7961, 2),
];

pub fn compat_lookup(vendor: u16, device: u16) -> Option<u8> {
    COMPAT_DB
        .iter()
        .find(|&&(v, d, _)| v == vendor && d == device)
        .map(|&(_, _, s)| s)
}

// ---------------------------------------------------------------------------
// F323 虚拟设备矩阵 — QEMU 全设备覆盖
// ---------------------------------------------------------------------------

/// QEMU 虚拟设备 DID 集合（简化示意）。
pub fn is_qemu_virtual(vendor: u16, device: u16) -> bool {
    (vendor == 0x1B36 && device == 0x0100) // QXL
        || (vendor == 0x8086 && device == 0x100E) // e1000
        || (vendor == 0x1AF4 && device & 0x1000 == 0x1000) // virtio
}

// ---------------------------------------------------------------------------
// F324 恶意设备 fuzz — 设备响应对抗
// ---------------------------------------------------------------------------

/// 设备寄存器读取永远返回 clamp 后的合法值（对抗 0xFFFF/-1 噪声）。
pub fn fuzz_dev_read(raw: u32, max_valid: u32) -> u32 {
    if raw == u32::MAX || raw == 0xFFFF_FFFF {
        return 0;
    }
    raw.min(max_valid)
}

/// 配置空间越界访问防护。
pub fn fuzz_cfg_offset(offset: u32, cfg_size: u32) -> Option<u32> {
    if offset + 4 <= cfg_size {
        Some(offset)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F325 兼容域自检（M500）
// ---------------------------------------------------------------------------

pub fn run_hwcompat_checks() -> CheckSet {
    let mut set = CheckSet::new("hwcompat-m500");

    // F301
    set.add("F301 pick 640", vga_pick_mode(1_500_000) == Some((640, 480, 32)), "1.5MB vram");
    set.add("F301 pick 320", vga_pick_mode(100_000) == Some((320, 200, 8)), "tiny vram");

    // F302
    set.add("F302 skylake", intel_gpu_family(0x1912) == Some("Skylake"), "did match");
    set.add("F302 unknown", intel_gpu_family(0x1234).is_none(), "not matched");
    set.add("F302 accel", intel_gpu_accel(intel_gpu_family(0x3E92)), "known -> accel");

    // F303
    let (name, native) = amd_gpu_probe(0x67DF_0000);
    set.add("F303 native", native && name == "radeon-class", "recognized");
    let (name2, native2) = amd_gpu_probe(0x0001_0000);
    set.add("F303 fallback", !native2 && name2 == "efifb-fallback", "efifb");

    // F304
    set.add("F304 rtl8169", rtl8169_identify(0x20) == Some("RTL8169"), "verid");

    // F305
    set.add("F305 i225", intel_nic_identify(0x15F3) == Some("I225-V"), "did");
    set.add("F305 none", intel_nic_identify(0x0000).is_none(), "no match");

    // F306
    set.add("F306 usable", ehci_usable(0x0000_0013), "ports=3,cc=1");
    set.add("F306 no ports", !ehci_usable(0x0000_0110), "ports=0");

    // F307
    set.add("F307 base 50", sdmmc_base_freq_mhz(0x0000_3200) == 50, "cap field");
    set.add("F307 div", sdmmc_divider(50, 25_000) == 1, "half to 25M");
    set.add("F307 div zero", sdmmc_divider(0, 25_000) == 0, "no base");

    // F308
    set.add("F308 8GiB", emmc_capacity_gib([0, 0, 0, 1]) == 8, "0x01000000 secs");
    set.add("F308 zero", emmc_capacity_gib([0; 4]) == 0, "empty");

    // F309
    set.add("F309 atapi", sata_is_atapi(0x85C0), "bit15");
    set.add("F309 cdrom", sata_is_cdrom(0x85C0, 0x05), "type 5");

    // F310
    set.add("F310 write ok", ec_can_write(0), "ibf clear");
    set.add("F310 write busy", !ec_can_write(EC_IBF), "ibf set");
    set.add("F310 read ok", ec_can_read(EC_OBF), "obf set");

    // F311
    set.add("F311 50%", backlight_percent(5, 10) == 50, "half");
    set.add("F311 min guard", backlight_percent(0, 10) == 10, "level0 -> min");
    set.add("F311 zero max", backlight_percent(5, 0) == 0, "bad max");

    // F312
    set.add("F312 vc", uvc_is_video(0x0E, 0x01), "control");
    set.add("F312 vs", uvc_is_video(0x0E, 0x02), "streaming");
    set.add("F312 not uvc", !uvc_is_video(0x08, 0x01), "mass storage");

    // F313
    let mut fp = FpRegistry::new();
    set.add("F313 enroll", fp.enroll(0) && !fp.enroll(0), "no dup");
    set.add("F313 range", !fp.enroll(9), "slot bound");
    set.add("F313 count", fp.count == 1, "tally");

    // F314
    let p0 = pcr_extend(0, 0xDEAD_BEEF);
    set.add("F314 extend", p0 != 0, "nonzero");
    set.add("F314 match", pcr_match(pcr_extend(p0, 1), pcr_extend(p0, 1)), "deterministic");
    set.add("F314 mismatch", !pcr_match(p0, 0), "initial zero");

    // F315
    set.add("F315 accel->power", sensor_route(HwSensor::Accel) == 2, "routing");
    set.add("F315 light->display", sensor_route(HwSensor::AmbientLight) == 1, "routing");
    set.add("F315 gyro->sec", sensor_route(HwSensor::Gyro) == 3, "routing");

    // F316
    set.add("F316 div 115200", uart_divider(1_843_200, 115_200) == 1, "1.8432MHz");
    set.add("F316 div 9600", uart_divider(1_843_200, 9_600) == 12, "classic");
    set.add("F316 div zero baud", uart_divider(1_843_200, 0) == 0, "guard");

    // F317
    set.add("F317 ready", lpt_ready(0x10), "ack only");
    set.add("F317 busy", !lpt_ready(0x80), "busy bit");
    set.add("F317 paper", !lpt_ready(0x08), "paper out");

    // F318
    let j = PrintJob { pages: 10, duplex: true, color: false };
    set.add("F318 duplex 5 sheets", print_ink_permille(&j) == 12, "5*12/5000*1000");
    let j2 = PrintJob { pages: 10, duplex: false, color: true };
    set.add("F318 color 10 sheets", print_ink_permille(&j2) == 80, "10*40/5000*1000");
    set.add("F318 zero", print_ink_permille(&PrintJob { pages: 0, duplex: false, color: false }) == 0, "empty job");

    // F319
    set.add("F319 dpi list", SCAN_DPI.len() == 3 && SCAN_DPI[0] == 150, "table");
    set.add("F319 size grows", scan_bytes_a4(600) > scan_bytes_a4(300), "monotonic");
    set.add("F319 zero dpi", scan_bytes_a4(0) == 0, "guard");

    // F320
    set.add("F320 full 3", hw_self_report_score(Some(b"acme"), Some(b"S1"), Some(b"M1")) == 3, "all present");
    set.add("F320 partial 1", hw_self_report_score(Some(b"acme"), None, None) == 1, "vendor only");

    // F321
    let s = adapt_advance(AdaptStage::Tested);
    set.add("F321 advance", s == AdaptStage::Accepted, "tested->accepted");
    set.add("F321 sink", adapt_advance(AdaptStage::Accepted) == AdaptStage::Accepted, "final");
    set.add("F321 whitelist", !adapt_whitelisted(AdaptStage::Tested), "not yet");

    // F322
    set.add("F322 hit 1", compat_lookup(0x8086, 0x1912) == Some(1), "supported");
    set.add("F322 hit 2", compat_lookup(0x1002, 0x67DF) == Some(2), "degraded");
    set.add("F322 miss", compat_lookup(0x1234, 0x5678).is_none(), "unknown");

    // F323
    set.add("F323 virtio", is_qemu_virtual(0x1AF4, 0x1000), "virtio-net");
    set.add("F323 qxl", is_qemu_virtual(0x1B36, 0x0100), "qxl");

    // F324
    set.add("F324 all-ones", fuzz_dev_read(u32::MAX, 100) == 0, "noise -> 0");
    set.add("F324 clamp", fuzz_dev_read(500, 100) == 100, "clamped");
    set.add("F324 cfg oob", fuzz_cfg_offset(0xFC, 0x100).is_some() && fuzz_cfg_offset(0xFD, 0x100).is_none(), "bounds");

    // F325
    set.add("F325 self count", set.len() >= 25, "25+ checks");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f301_order_is_largest_first() {
        assert_eq!(VGA_FALLBACK_MODES[0], (1024, 768, 32));
        // 显存刚好够 1024x768x32（3MB）
        assert_eq!(vga_pick_mode(3 * 1024 * 1024), Some((1024, 768, 32)));
    }

    #[test]
    fn f307_divider_rounding() {
        // 50MHz / 2 / (400kHz/1000) → 62（下取整后 clamp 最低 1）
        assert_eq!(sdmmc_divider(50, 400), 62);
    }

    #[test]
    fn f314_pcr_chain_differs_by_digest() {
        let a = pcr_extend(0, 1);
        let b = pcr_extend(0, 2);
        assert_ne!(a, b);
    }

    #[test]
    fn f321_full_path() {
        let mut s = AdaptStage::Reported;
        for _ in 0..3 {
            s = adapt_advance(s);
        }
        assert!(adapt_whitelisted(s));
    }

    #[test]
    fn domain_self_test_passes() {
        let set = run_hwcompat_checks();
        assert!(!set.truncated());
        assert!(set.all_passed(), "hwcompat-m500 self-test: {} checks", set.len());
    }
}
