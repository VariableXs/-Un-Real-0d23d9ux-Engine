//! 任务16 · PCI/PCIe 枚举——MCFG 表解析 + ECAM 配置空间扫描。
//!
//! 目标态通路：Limine RSDP → XSDT → 签名 `MCFG` 的表 → segment 条目
//! 给出 ECAM 基址 → 按公式 `base | bus<<20 | dev<<15 | fn<<12 | off`
//! 读配置空间 → 按 class code 识别 NVMe（01-08-02）与 AHCI（01-06-01）。
//!
//! 宿主可测性设计：解析层只吃字节切片（[`McfgSegment::parse`] /
//! [`find_mcfg_in_xsdt`]），扫描层只吃 [`EcamAccess`] 抽象——宿主注入
//! 合成 ECAM，目标态用 MMIO 窗口（`pfh::map_mmio`，PCD|PWT）惰性逐页
//! 映射后 volatile 读。合成表/合成设备与真 ACPI 布局逐字节同构。
//!
//! 枚举边界如实声明：bus 上限 [`MAX_SCAN_BUSES`]（q35 设备全部落
//! bus 0，上限只防 ECAM 全域扫满页表），segment 只取 MCFG 声明的
//! `[start_bus, end_bus] ∩ [0, MAX_SCAN_BUSES]`。


/// MCFG 表体头到第一个 segment 条目的偏移（表头 44 字节）。
pub const MCFG_BODY_OFF: usize = 44;
/// 每个 segment 条目 16 字节。
pub const SEG_ENTRY_LEN: usize = 16;
/// bus 枚举上限（QEMU q35 全部设备在 bus 0；上限只防异常固件）。
pub const MAX_SCAN_BUSES: u8 = 16;

/// MCFG 单条 segment：ECAM 基址与总线区间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct McfgSegment {
    pub base: u64,
    pub start_bus: u8,
    pub end_bus: u8,
}

impl McfgSegment {
    /// 解析一个 16 字节条目。
    pub fn parse(b: &[u8]) -> Option<McfgSegment> {
        if b.len() < SEG_ENTRY_LEN {
            return None;
        }
        let base = u64::from_le_bytes(b[0..8].try_into().ok()?);
        // 条目 [8..10] 是 PCI segment group——单段域（QEMU/真机常见）足够，
        // 多 segment 域如实跳过（base 0 不可信，防御拒绝）。
        let seg_group = u16::from_le_bytes(b[8..10].try_into().ok()?);
        if seg_group != 0 || base == 0 {
            return None;
        }
        Some(McfgSegment {
            base,
            start_bus: b[10],
            end_bus: b[11],
        })
    }
}

/// ECAM 配置空间 32 位读地址（对齐校验由访问层负责）。
pub fn ecam_addr(seg: &McfgSegment, bus: u8, dev: u8, func: u8, off: u16) -> u64 {
    seg.base
        | ((bus as u64) << 20)
        | ((dev as u64) << 15)
        | ((func as u64) << 12)
        | ((off as u64) & 0xFFF)
}

/// 配置空间访问抽象：目标态=MMIO 窗口，宿主态=合成字节。
pub trait EcamAccess {
    fn read32(&mut self, addr: u64) -> u32;
}

/// 识别出的控制器。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub kind: PciKind,
    /// BAR0 拿到的 MMIO 物理基址（64 位）。
    pub bar0: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PciKind {
    Nvme,
    Ahci,
    /// USB xHCI 主机控制器（class 0x0C03，prog-if 0x30）——S4.1（AI-5）。
    Xhci,
    Other,
}

/// class code 读取：offset 0x08 = [31:24] base, [23:16] sub, [15:8] prog-if。
fn classify(ecam: &mut dyn EcamAccess, addr: u64) -> PciKind {
    let cc = ecam.read32(addr + 0x08);
    match (cc >> 16) & 0xFFFF {
        0x0108 => match (cc >> 8) & 0xFF {
            0x02 => PciKind::Nvme,
            _ => PciKind::Other,
        },
        // SATA 控制器 AHCI 1.0（class 0x0106，prog-if 0x01）——S4 批（AI-5）。
        // legacy IDE（prog-if 0x00/0x8x）如实归 Other，绝不冒认。
        0x0106 => match (cc >> 8) & 0xFF {
            0x01 => PciKind::Ahci,
            _ => PciKind::Other,
        },
        // Serial bus / USB / xHCI：prog-if 0x30 才是 xHCI（0x00=UHCI 等
        // 如实归 Other，绝不冒认）。
        0x0C03 => match (cc >> 8) & 0xFF {
            0x30 => PciKind::Xhci,
            _ => PciKind::Other,
        },
        _ => PciKind::Other,
    }
}

/// BAR0 解析：读 offset 0x10（低 4 位定性）。
/// - `0b00`：32 位 MMIO，值即基址；
/// - `0b10`：64 位 MMIO，BAR1（0x14）给高 32 位；
/// - IO BAR（bit0=1）/ 非 MMIO：如实 None（NVMe/AHCI 必须 MMIO）。
pub fn parse_bar0_mmio(ecam: &mut dyn EcamAccess, addr: u64) -> Option<u64> {
    let low = ecam.read32(addr + 0x10);
    if low & 1 != 0 {
        return None; // IO BAR——NVMe/AHCI 必须 MMIO，如实拒绝。
    }
    // bit2:1 = 0b10 表示 64 位 MMIO（BAR1 给高 32 位）。
    let base = if (low >> 1) & 0b11 == 0b10 {
        let high = ecam.read32(addr + 0x14);
        ((high as u64) << 32) | (low as u64 & 0xFFFF_FFF0)
    } else {
        low as u64 & 0xFFFF_FFF0
    };
    if base == 0 {
        return None;
    }
    Some(base & !0xF)
}

/// 扫描一个 segment：返回第一个 NVMe（None=该 segment 无 NVMe）。
/// 多设备命中顺序 bus→dev→func 稳定可复现。
/// 枚举全部 NVMe 控制器（任务18：第二控制器挂 SHARED exFAT 卷）。
pub fn scan_nvme_all(ecam: &mut dyn EcamAccess, seg: &McfgSegment) -> alloc_crate_vec::Vec<PciDevice> {
    let end_bus = seg.end_bus.min(MAX_SCAN_BUSES);
    let mut hits = alloc_crate_vec::Vec::new();
    let mut bus = seg.start_bus;
    loop {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let addr = ecam_addr(seg, bus, dev, func, 0);
                if ecam.read32(addr) & 0xFFFF == 0xFFFF {
                    continue;
                }
                let kind = classify(ecam, addr);
                if kind != PciKind::Nvme {
                    continue;
                }
                match parse_bar0_mmio(ecam, addr) {
                    Some(bar0) => hits.push(PciDevice { bus, dev, func, kind, bar0 }),
                    None => {
                        #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                        crate::kinfo!("pci-scan: nvme at {:02x}:{:02x}.{} BAR parse FAILED", bus, dev, func);
                    }
                }
            }
        }
        if bus >= end_bus {
            break;
        }
        bus += 1;
    }
    hits
}

/// 供 scan_nvme_all 的返回类型别名（no_std 下显式 alloc 路径）。
pub(crate) mod alloc_crate_vec {
    pub use alloc::vec::Vec;
}

/// 枚举全部 xHCI 控制器（S4.1·AI-5）：多控制器命中顺序 bus→dev→func/// 稳定可复现；prog-if ≠0x30 的 USB 控制器（UHCI/EHCI）如实不收。
pub fn scan_xhci_all(ecam: &mut dyn EcamAccess, seg: &McfgSegment) -> alloc_crate_vec::Vec<PciDevice> {
    let end_bus = seg.end_bus.min(MAX_SCAN_BUSES);
    let mut hits = alloc_crate_vec::Vec::new();
    let mut bus = seg.start_bus;
    loop {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let addr = ecam_addr(seg, bus, dev, func, 0);
                if ecam.read32(addr) & 0xFFFF == 0xFFFF {
                    continue;
                }
                if classify(ecam, addr) != PciKind::Xhci {
                    continue;
                }
                match parse_bar0_mmio(ecam, addr) {
                    Some(bar0) => hits.push(PciDevice { bus, dev, func, kind: PciKind::Xhci, bar0 }),
                    None => {
                        #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                        crate::kinfo!("pci-scan: xhci at {:02x}:{:02x}.{} BAR parse FAILED", bus, dev, func);
                    }
                }
            }
        }
        if bus >= end_bus {
            break;
        }
        bus += 1;
    }
    hits
}

/// 首个 AHCI 控制器（S4 批·AI-5）：class 0x0106 prog-if 0x01；命中顺序
/// bus→dev→func 稳定可复现。legacy IDE（0x0106 非 0x01 prog-if 或
/// 0x0101）一律不收。
pub fn scan_ahci(ecam: &mut dyn EcamAccess, seg: &McfgSegment) -> Option<PciDevice> {
    let end_bus = seg.end_bus.min(MAX_SCAN_BUSES);
    let mut bus = seg.start_bus;
    loop {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let addr = ecam_addr(seg, bus, dev, func, 0);
                if ecam.read32(addr) & 0xFFFF == 0xFFFF {
                    continue;
                }
                if classify(ecam, addr) != PciKind::Ahci {
                    continue;
                }
                let bar0 = parse_bar0_mmio(ecam, addr)?;
                return Some(PciDevice { bus, dev, func, kind: PciKind::Ahci, bar0 });
            }
        }
        if bus >= end_bus {
            return None;
        }
        bus += 1;
    }
}

pub fn scan_nvme(ecam: &mut dyn EcamAccess, seg: &McfgSegment) -> Option<PciDevice> {
    let end_bus = seg.end_bus.min(MAX_SCAN_BUSES);
    let mut bus = seg.start_bus;
    loop {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let addr = ecam_addr(seg, bus, dev, func, 0);
                // 空槽：vendor id 0xFFFF。
                if ecam.read32(addr) & 0xFFFF == 0xFFFF {
                    continue;
                }
                let kind = classify(ecam, addr);
                if kind != PciKind::Nvme {
                    continue;
                }
                let bar0 = parse_bar0_mmio(ecam, addr)?;
                return Some(PciDevice {
                    bus,
                    dev,
                    func,
                    kind,
                    bar0,
                });
            }
        }
        if bus >= end_bus {
            return None;
        }
        bus += 1;
    }
}

/// 宿主可测版 MCFG 查找：遍历根表（XSDT 8B / RSDT 4B 条目），用
/// `sig_at` 回调查各条目物理地址处的表签名——宿主注入合成表，
/// 目标态传 `phys_read32`。返回 MCFG 表条目物理地址。
pub fn find_mcfg_in_xsdt(root: &[u8], is_xsdt: bool, sig_at: &dyn Fn(u64) -> u32) -> Option<u64> {
    if root.len() < 36 {
        return None;
    }
    let len = u32::from_le_bytes(root[4..8].try_into().ok()?) as usize;
    let entry_len = if is_xsdt { 8 } else { 4 };
    let count = len.saturating_sub(36) / entry_len;
    for i in 0..count {
        let off = 36 + i * entry_len;
        if off + entry_len > root.len() {
            break;
        }
        let addr = if is_xsdt {
            u64::from_le_bytes(root[off..off + 8].try_into().ok()?)
        } else {
            u64::from(u32::from_le_bytes(root[off..off + 4].try_into().ok()?))
        };
        if addr == 0 {
            continue;
        }
        if sig_at(addr) == u32::from_le_bytes(*b"MCFG") {
            return Some(addr);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 目标态（真 ACPI + 真 ECAM MMIO）——仅内核目标编译。
// ---------------------------------------------------------------------------
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod target {
    use super::*;

    /// 物理地址 → HHDM 虚拟地址。addr 已落在 HHDM 窗口（≥hhdm）时
    /// 视为已映射直接返回——limine 响应里的地址就是这种形态。曾漏掉
    /// 此判断对 rsdp 二次加 hhdm，非 canonical 访问实机 #GP(0)。
    fn to_virt(addr: u64, hhdm: u64) -> u64 {
        if hhdm != 0 && addr >= hhdm {
            return addr;
        }
        addr + hhdm
    }

    /// 经 HHDM 读物理内存 32 位小端。
    fn phys_read32(addr: u64, hhdm: u64) -> u32 {
        let virt = to_virt(addr, hhdm);
        // SAFETY: addr 为 ACPI 表内偏移（固件保证可读），经 HHDM 窗口访问。
        unsafe { core::ptr::read_volatile(virt as *const u32) }
    }

    /// 遍历 RSDP 根表（XSDT/RSDT 自适应），返回 MCFG 表条目物理地址。
    pub fn find_mcfg_phys(rsdp_addr: u64, hhdm: u64) -> Option<u64> {
        let rsdp_virt = to_virt(rsdp_addr, hhdm);
        // SAFETY: RSDP 头 36 字节，固件内存恒可读。
        let bytes = unsafe { core::slice::from_raw_parts(rsdp_virt as *const u8, 36) };
        let rsdp = crate::acpi::Rsdp::parse(bytes)?;
        let (root_addr, is_xsdt) = rsdp.root_table()?;
        let root_virt = to_virt(root_addr, hhdm);
        // SAFETY: SDT 头 36 字节。
        let hdr = unsafe { core::slice::from_raw_parts(root_virt as *const u8, 36) };
        let len = u32::from_le_bytes(hdr[4..8].try_into().ok()?) as usize;
        let entries = if is_xsdt { 8 } else { 4 };
        let count = len.saturating_sub(36) / entries;
        for i in 0..count {
            let slot = root_virt + 36 + (i * entries) as u64;
            let tab_addr = if is_xsdt {
                // SAFETY: XSDT 条目槽（8B 物理地址）。
                unsafe { core::ptr::read_volatile(slot as *const u64) }
            } else {
                // SAFETY: RSDT 条目槽（4B 物理地址）。
                u64::from(unsafe { core::ptr::read_volatile(slot as *const u32) })
            };
            if tab_addr == 0 {
                continue;
            }
            let sig = phys_read32(tab_addr, hhdm);
            if sig == u32::from_le_bytes(*b"MCFG") {
                return Some(tab_addr);
            }
        }
        None
    }

    /// 读 MCFG 第一个合法 segment（多段域只取首段——QEMU/消费级单段）。
    pub fn read_first_segment(mcfg_phys: u64, hhdm: u64) -> Option<McfgSegment> {
        let mut buf = [0u8; SEG_ENTRY_LEN];
        let body = mcfg_phys + MCFG_BODY_OFF as u64;
        for (i, chunk) in buf.chunks_mut(4).enumerate() {
            let v = phys_read32(body + (i * 4) as u64, hhdm);
            chunk.copy_from_slice(&v.to_le_bytes());
        }
        McfgSegment::parse(&buf)
    }

    /// 目标态 ECAM 访问：MMIO 窗口基址 [`ECAM_WINDOW`] 起，惰性逐页映射。
    pub struct EcamMmio {
        seg: McfgSegment,
    }

    /// MMIO 窗口虚拟基址：PML4[510] 顶格 1GiB 段（0xffffff0000000000），
    /// 与内核（511）和 HHDM（0xffff8…）不重叠；U=0 用户不可见。
    pub const ECAM_WINDOW: u64 = 0xffff_ff00_0000_0000;

    /// 窗口内已映射页计数（验收证据）。
    pub static ECAM_PAGES_MAPPED: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);

    impl EcamMmio {
        pub fn new(seg: McfgSegment) -> EcamMmio {
            EcamMmio { seg }
        }

        /// 确保 addr 所在 4KiB 页已映射进窗口。窗口按"页号槽位"排布：
        /// window + (phys_page - base_page) * 4096，同一页只映射一次。
        fn ensure(&mut self, addr: u64) -> Option<u64> {
            use crate::mem::pfh::PageTableOps as _;
            let page = addr & !0xFFF;
            // 窗口内偏移 = 物理页相对 MCFG base 的页序号 × 4KiB。
            let rel_pages = (page - self.seg.base) >> 12;
            let win = ECAM_WINDOW + (rel_pages << 12);
            let mut ops = crate::mem::pfh::target_ops();
            if ops.translate(win).is_none() {
                if !ops.map_mmio(win, page) {
                    return None;
                }
                ECAM_PAGES_MAPPED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
            Some(win | (addr & 0xFFF))
        }
    }

    impl EcamAccess for EcamMmio {
        fn read32(&mut self, addr: u64) -> u32 {
            let Some(virt) = self.ensure(addr & !3) else {
                return u32::MAX; // 映射失败如实当空槽。
            };
            // SAFETY: virt 已映射（PCD|PWT），4KiB 窗口内 4 字节对齐读。
            unsafe { core::ptr::read_volatile(virt as *const u32) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::blk;
    use super::*;

    /// 合成 MCFG segment 条目。
    fn seg_bytes(base: u64, s: u8, e: u8) -> [u8; SEG_ENTRY_LEN] {
        let mut b = [0u8; SEG_ENTRY_LEN];
        b[0..8].copy_from_slice(&base.to_le_bytes());
        b[10] = s;
        b[11] = e;
        b
    }

    /// 合成 XSDT：头（签名/长度）+ MCFG 条目。
    fn xsdt_with_mcfg(mcfg_body_off: usize) -> Vec<u8> {
        let mut t = vec![0u8; 36 + 8];
        t[0..4].copy_from_slice(b"XSDT");
        t[4..8].copy_from_slice(&44u32.to_le_bytes());
        // 表体放一个指针（指向"伪 MCFG 物理地址"）——解析层只做签名
        // 分发，指针值不二次校验（真表 checksum 校验在 acpi.rs 层）。
        t[36..44].copy_from_slice(&(0x1000u64 + mcfg_body_off as u64).to_le_bytes());
        t
    }

    #[test]
    fn pci_mcfg_segment_parse_fields() {
        let s = McfgSegment::parse(&seg_bytes(0xB000_0000, 0, 255)).unwrap();
        assert_eq!(s.base, 0xB000_0000);
        assert_eq!(s.start_bus, 0);
        assert_eq!(s.end_bus, 255);
        // 非法：全零 base / 多 segment group 如实拒绝。
        assert!(McfgSegment::parse(&seg_bytes(0, 0, 255)).is_none());
        let mut multi = seg_bytes(0xB000_0000, 0, 255);
        multi[8..10].copy_from_slice(&1u16.to_le_bytes());
        assert!(McfgSegment::parse(&multi).is_none());
    }

    #[test]
    fn pci_ecam_addr_formula() {
        let s = McfgSegment {
            base: 0xB000_0000,
            start_bus: 0,
            end_bus: 255,
        };
        assert_eq!(ecam_addr(&s, 0, 0, 0, 0), 0xB000_0000);
        assert_eq!(ecam_addr(&s, 2, 5, 3, 0x10), 0xB000_0000 | (2 << 20) | (5 << 15) | (3 << 12) | 0x10);
        // 偏移限制在 4KiB 内（每函数一页）。
        assert_eq!(ecam_addr(&s, 0, 0, 0, 0x1004) & 0xFFF, 4);
    }

    /// 合成 ECAM：bus0 dev0=host bridge(0600)，dev5 fn0=NVMe(010802，
    /// 64 位 BAR0=0xC0000000)，dev7 fn2=AHCI(010601)。
    struct FakeEcam {
        slots: Vec<(u8, u8, u8, [u32; 8])>,
    }

    impl FakeEcam {
        fn new() -> Self {
            // cfg [0]=VID:PID [2]=cmd:sts [3]=class:prog:sub:base
            // [4]=BAR0 [5]=BAR1
            let nvme: [u32; 8] = [
                0x1af4_0001, 0x0000_0007, 0x0108_0201, 0x0000_0000, 0xC000_0004, 0x0000_0000, 0, 0,
            ];
            let ahci: [u32; 8] = [
                0x8086_2922, 0x0000_0007, 0x0106_0100, 0xE100_0000, 0, 0, 0, 0,
            ];
            let host: [u32; 8] = [0x8086_1237, 0, 0x0600_0000, 0, 0, 0, 0, 0];
            FakeEcam {
                slots: vec![(0, 0, 0, host), (0, 5, 0, nvme), (0, 7, 2, ahci)],
            }
        }
        fn find(&self, bus: u8, dev: u8, func: u8) -> Option<&[u32; 8]> {
            self.slots
                .iter()
                .find(|(b, d, f, _)| *b == bus && *d == dev && *f == func)
                .map(|(_, _, _, cfg)| cfg)
        }
    }

    impl EcamAccess for FakeEcam {
        fn read32(&mut self, addr: u64) -> u32 {
            let seg = McfgSegment {
                base: 0xB000_0000,
                start_bus: 0,
                end_bus: 255,
            };
            let off = (addr & 0xFFF) as usize;
            let bus = ((addr >> 20) & 0xFF) as u8;
            let dev = ((addr >> 15) & 0x1F) as u8;
            let func = ((addr >> 12) & 0x7) as u8;
            let _ = seg;
            match self.find(bus, dev, func) {
                Some(cfg) => cfg[off / 4],
                None => u32::MAX, // 空槽。
            }
        }
    }

    #[test]
    fn pci_scan_finds_nvme_with_bar0() {
        let mut ecam = FakeEcam::new();
        let seg = McfgSegment {
            base: 0xB000_0000,
            start_bus: 0,
            end_bus: 255,
        };
        let hit = scan_nvme(&mut ecam, &seg).expect("合成 ECAM 必含 NVMe");
        assert_eq!(hit.bus, 0);
        assert_eq!(hit.dev, 5);
        assert_eq!(hit.func, 0);
        assert_eq!(hit.kind, PciKind::Nvme);
        // 64 位 BAR：BAR0 bit2:1=0b10，基址取高位清零后的 0xC000_0000。
        assert_eq!(hit.bar0, 0xC000_0000);
    }

    #[test]
    fn pci_scan_skips_non_mmio_bars() {
        // BAR0 为 IO BAR（bit0=1）的 NVMe：如实拒绝，继续扫。
        struct IoBarNvme;
        impl EcamAccess for IoBarNvme {
            fn read32(&mut self, addr: u64) -> u32 {
                match addr & 0xFFF {
                    0x00 => 0x1af4_0001,     // vendor 在席
                    0x08 => 0x0108_0201,     // NVMe class
                    0x10 => 0x0000_0001,     // IO BAR
                    _ => 0,
                }
            }
        }
        let mut ecam = IoBarNvme;
        let seg = McfgSegment {
            base: 0,
            start_bus: 0,
            end_bus: 0,
        };
        assert!(scan_nvme(&mut ecam, &seg).is_none());
    }

    #[test]
    fn pci_scan_stable_on_absent_device() {
        let mut ecam = FakeEcam::new();
        let seg = McfgSegment {
            base: 0xB000_0000,
            start_bus: 4, // 合成设备都在 bus0 —— 高 bus 扫描必须空手而归。
            end_bus: 6,
        };
        assert!(scan_nvme(&mut ecam, &seg).is_none());
    }

    #[test]
    fn pci_find_mcfg_in_xsdt_signature_dispatch() {
        const MCFG_SIG: u32 = u32::from_le_bytes(*b"MCFG");
        let t = xsdt_with_mcfg(0);
        // 条目指针=0x1000；sig_at 回调只在 0x1000 处报 MCFG 签名。
        let hit = find_mcfg_in_xsdt(&t, true, &|a| {
            if a == 0x1000 {
                MCFG_SIG
            } else {
                0
            }
        });
        assert_eq!(hit, Some(0x1000));
        // RSDT 模式（4 字节槽）同样可用。
        let mut t4 = vec![0u8; 36 + 4];
        t4[0..4].copy_from_slice(b"RSDT");
        t4[4..8].copy_from_slice(&40u32.to_le_bytes());
        t4[36..40].copy_from_slice(&0x2000u32.to_le_bytes());
        assert_eq!(
            find_mcfg_in_xsdt(&t4, false, &|a| if a == 0x2000 { MCFG_SIG } else { 0 }),
            Some(0x2000)
        );
        // 无 MCFG 的 XSDT：None。
        let mut empty = vec![0u8; 44];
        empty[0..4].copy_from_slice(b"XSDT");
        empty[4..8].copy_from_slice(&44u32.to_le_bytes());
        assert!(find_mcfg_in_xsdt(&empty, true, &|_| MCFG_SIG).is_none());
    }

    #[test]
    fn pci_blk_kind_roundtrip_names() {
        // 块错误串口径冻结（验收证据行用）。
        assert_eq!(blk::BlockError::Timeout.as_str(), "TIMEOUT");
        assert_eq!(blk::BlockError::DeviceReset.as_str(), "DEVICE-RESET");
    }
}
