//! S4 批 · AHCI 最小栈（SATA 块设备）——AI-5 内核基建长线第三件（2026-09-22）。
//!
//! **范围如实声明**：单控制器 × 单端口（首个 `PxSSTS.DET=3` 的实现端口）
//! × 命令槽 0 串行 × LBA48。命令词汇表：IDENTIFY DEVICE(0xEC) /
//! READ DMA EXT(0x25) / WRITE DMA EXT(0x35) / FLUSH CACHE EXT(0xEA)。
//! 纯轮询（GHC.IE=0、PRDT I 位=0），与 NVMe/xHCI 同一恢复口径
//! （init 超时 → 整段重初始化一次 → `DeviceReset`）。
//! **不做**：NCQ（FPDMA/SEND+RECEIVE）、Port Multiplier、热插拔中断、
//! ATAPI、加密/可信计算特性、legacy IDE 兼容模式。
//!
//! **安全设计（实机红线对齐）**：目标态探针**默认只读**（IDENTIFY +
//! 读 LBA0 取证）；写回环仅在 cmdline 显式 `ahci_selftest=1` 时执行
//! （面向 QEMU 刮擦盘；真机 SATA 盘可能承载用户数据，绝不自动写）。
//! 白名单纪律：AHCI 控制器只在作为目标盘在场时被枚举，内置 NVMe
//! 域零接触（`classify` 只认 class 0x0106，与本机内置 NVMe 0x0108 无交集）。
//!
//! **布局权威对照**：寄存器/命令列表/PRDT/CFIS 逐字段对照 AHCI 1.3.1
//! §3（HBA）、§4（命令结构）与 QEMU `hw/ide/ahci.c` 行为语义：
//! - 命令列表项 32B：DW0 = CFL[0:5) | W(6) | …；DW1:2 = CTBA（128B 对齐）；
//! - PRDT 项 16B：DBA/DBAU 64 位数据基址；DBC[0:22) = 字节数−1；
//! - H2D Register FIS 0x27：C 位在 byte0[7]，命令在 byte2，device
//!   (LBA 位) byte7=0x40，LBA48 四段在 byte4/5/6/8/9/10，扇区数 16 位
//!   在 byte12/13（EXT 双字节）；
//! - D2H FIS 0x34 落在 PxFB+0x40（RFIS）：status byte2 / error byte3。

use super::blk::{BlockDevice, BlockError};
use super::nvme::{BarAccess, DmaMem};

/// 4KiB 页（与 NVMe/xHCI 同一 DMA 帧粒度）。
pub const PAGE: u64 = 4096;
/// 单命令最大传输（8 扇区 = 4KiB，恰好一帧数据缓冲）。
pub const MAX_XFER_BYTES: u32 = 4096;
/// 轮询超时（ns）：命令/端口等待，与既有最小栈同量纲。
pub const TIMEOUT_NS: u64 = 3_000_000_000;

// ---- HBA 寄存器（AHCI 1.3.1 §3.1 逐值同构）--------------------------------
pub const REG_CAP: u16 = 0x00;
pub const REG_GHC: u16 = 0x04;
pub const REG_IS: u16 = 0x08;
pub const REG_PI: u16 = 0x0C;
pub const GHC_AE: u32 = 1 << 31;
pub const GHC_IE: u32 = 1 << 1;
pub const GHC_HR: u32 = 1 << 0;
/// 端口寄存器基址与步进（§3.1.2：P0 = 0x100，步进 0x80）。
pub const PORT_BASE: u16 = 0x100;
pub const PORT_STRIDE: u16 = 0x80;
// 端口内偏移。
pub const P_CLB: u16 = 0x00;
pub const P_CLBU: u16 = 0x04;
pub const P_FB: u16 = 0x08;
pub const P_FBU: u16 = 0x0C;
pub const P_IS: u16 = 0x10;
pub const P_CMD: u16 = 0x18;
pub const P_TFD: u16 = 0x20;
pub const P_SIG: u16 = 0x24;
pub const P_SSTS: u16 = 0x28;
pub const P_SERR: u16 = 0x30;
pub const P_CI: u16 = 0x38;
// PxCMD 位。
pub const CMD_ST: u32 = 1 << 0;
pub const CMD_FRE: u32 = 1 << 4;
pub const CMD_FR: u32 = 1 << 14;
pub const CMD_CR: u32 = 1 << 15;
// PxSSTS.DET 值。
pub const DET_PRESENT_PHY: u32 = 3;
/// SATA 签名（PxSIG）。
pub const SIG_SATA: u32 = 0x0000_0101;

// ATA 命令（LBA48 词汇表）。
pub const ATA_IDENTIFY: u8 = 0xEC;
pub const ATA_READ_DMA_EXT: u8 = 0x25;
pub const ATA_WRITE_DMA_EXT: u8 = 0x35;
pub const ATA_FLUSH_CACHE_EXT: u8 = 0xEA;

// ---- ATA 命令构造（纯函数，宿主逐字节锁定）--------------------------------

/// 构造 20 字节 H2D Register FIS。
/// `count` = 扇区数（16 位，EXT 语义）；`lba` = 48 位逻辑块地址。
pub fn h2d_fis(cmd: u8, lba: u64, count: u16) -> [u8; 20] {
    let mut f = [0u8; 20];
    f[0] = 0x27 | 0x80; // H2D + Command Update
    f[1] = 0x00; // I=0（纯轮询），PM=0
    f[2] = cmd;
    f[7] = 0x40; // Device：LBA 模式
    f[4] = (lba & 0xFF) as u8;
    f[5] = ((lba >> 8) & 0xFF) as u8;
    f[6] = ((lba >> 16) & 0xFF) as u8;
    f[8] = ((lba >> 24) & 0xFF) as u8;
    f[9] = ((lba >> 32) & 0xFF) as u8;
    f[10] = ((lba >> 40) & 0xFF) as u8;
    f[12] = (count & 0xFF) as u8;
    f[13] = ((count >> 8) & 0xFF) as u8;
    f
}

/// 命令列表项 DW0：CFL=5（20B FIS）+ W 位（写方向）。
pub fn cmd_list_dw0(write: bool) -> u32 {
    5 | if write { 1 << 6 } else { 0 }
}

/// PRDT 项字节：DBC = 字节数 − 1（零基），I=0（纯轮询）。
pub fn prdt_bytes(buf_phys: u64, byte_count: u32) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0..4].copy_from_slice(&(buf_phys as u32).to_le_bytes());
    p[4..8].copy_from_slice(&((buf_phys >> 32) as u32).to_le_bytes());
    let dbc = (byte_count - 1) & 0x3F_FFFF;
    p[8..12].copy_from_slice(&dbc.to_le_bytes());
    p
}

// ---- 控制器最小栈 ---------------------------------------------------------

pub struct AhciCtrl<B: BarAccess, M: DmaMem> {
    bar: B,
    mem: M,
    now: fn() -> u64,
    timeout_ns: u64,
    /// 命中的端口号（0 基）。
    port: u16,
    block_size: u32,
    nblocks: u64,
    /// 帧分配：frame0 = CLB(1KiB)+FB(1KiB)+CT(256B)；frame1 = 数据缓冲。
    clb_phys: u64,
    fb_phys: u64,
    ct_phys: u64,
    data_buf: u64,
    /// IDENTIFY 证据（vendor 串 20..40 截取 8 字节，取证用）。
    pub model: [u8; 8],
    /// 验收证据：恢复路径触发计数。
    pub resets: u32,
}

fn try_init<B: BarAccess, M: DmaMem>(
    bar: B,
    mut mem: M,
    now: fn() -> u64,
    timeout_ns: u64,
    resets: u32,
) -> Result<AhciCtrl<B, M>, (B, M, BlockError)> {
    let mut c = AhciCtrl {
        bar,
        mem,
        now,
        timeout_ns,
        port: 0,
        block_size: 0,
        nblocks: 0,
        clb_phys: 0,
        fb_phys: 0,
        ct_phys: 0,
        data_buf: 0,
        model: [0x20; 8],
        resets,
    };
    macro_rules! bail {
        ($e:expr) => {
            if let Err(e) = $e {
                return Err((c.bar, c.mem, e));
            }
        };
    }
    bail!(c.hba_reset());
    // 帧分配（清零——复用帧残留命令位会假消费，NVMe 同款戒律）。
    macro_rules! alloc {
        ($what:expr) => {
            match c.mem.alloc_frame() {
                Some(f) => {
                    c.mem.zero_frame(f);
                    f
                }
                None => {
                    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                    crate::kwarn!("ahci: dma frame exhausted at {}", $what);
                    return Err((c.bar, c.mem, BlockError::Io));
                }
            }
        };
    }
    let clb = alloc!("clb");
    let data = alloc!("data buf");
    c.clb_phys = clb;
    c.fb_phys = clb + 1024; // 同帧内 1KiB 对齐
    c.ct_phys = clb + 2048; // 128B 对齐
    c.data_buf = data;
    // 端口选择：PI 位图内首个 DET=3。
    bail!(c.pick_port());
    // 编程 CLB/FB（64 位写序：低后高——QEMU 在高写时装载，NVMe 同款）。
    let p = c.port_off();
    let clb_bus = c.mem.bus_addr(c.clb_phys);
    let fb_bus = c.mem.bus_addr(c.fb_phys);
    c.bar.write32(p + P_CLB, clb_bus as u32);
    c.bar.write32(p + P_CLBU, (clb_bus >> 32) as u32);
    c.bar.write32(p + P_FB, fb_bus as u32);
    c.bar.write32(p + P_FBU, (fb_bus >> 32) as u32);
    // SERR 清光（累积错误位 W1C），再 FRE + ST。
    c.bar.write32(p + P_SERR, 0xFFFF_FFFF);
    let cmd = c.bar.read32(p + P_CMD);
    c.bar.write32(p + P_CMD, cmd | CMD_FRE | CMD_ST);
    bail!(c.identify());
    Ok(c)
}

impl<B: BarAccess, M: DmaMem> AhciCtrl<B, M> {
    /// 完整初始化（含恢复路径）：复位 → 端口编程 → IDENTIFY。
    pub fn init_with_recovery(bar: B, mem: M, now: fn() -> u64, timeout_ns: u64) -> Result<Self, BlockError> {
        let mut resets = 0;
        let (mut bar, mut mem) = (bar, mem);
        loop {
            match try_init(bar, mem, now, timeout_ns, resets) {
                Ok(c) => return Ok(c),
                Err((b, m, BlockError::Timeout)) if resets < 1 => {
                    resets += 1;
                    bar = b;
                    mem = m;
                }
                Err((_, _, BlockError::Timeout)) => return Err(BlockError::DeviceReset),
                Err((_, _, e)) => return Err(e),
            }
        }
    }

    fn port_off(&self) -> u16 {
        PORT_BASE + self.port * PORT_STRIDE
    }

    fn deadline(&self) -> u64 {
        (self.now)() + self.timeout_ns
    }

    fn wait_reg_bit(&mut self, reg: u16, mask: u32, want: bool) -> Result<(), BlockError> {
        let deadline = self.deadline();
        loop {
            let v = self.bar.read32(reg);
            if (v & mask != 0) == want {
                return Ok(());
            }
            if (self.now)() > deadline {
                return Err(BlockError::Timeout);
            }
        }
    }

    /// GHC.AE=1 → GHC.HR=1 → 等 HR 自清。
    fn hba_reset(&mut self) -> Result<(), BlockError> {
        let ghc = self.bar.read32(REG_GHC);
        if ghc & GHC_AE == 0 {
            self.bar.write32(REG_GHC, ghc | GHC_AE);
        }
        self.bar.write32(REG_GHC, GHC_AE | GHC_HR);
        let deadline = self.deadline();
        loop {
            if self.bar.read32(REG_GHC) & GHC_HR == 0 {
                break;
            }
            if (self.now)() > deadline {
                return Err(BlockError::Timeout);
            }
        }
        // AE 复位后可能被清，重置一次并保持 IE=0（纯轮询）。
        let ghc = self.bar.read32(REG_GHC);
        self.bar.write32(REG_GHC, ghc | GHC_AE);
        Ok(())
    }

    /// 选首个 DET=3 且在 PI 位图内的端口。
    fn pick_port(&mut self) -> Result<(), BlockError> {
        let pi = self.bar.read32(REG_PI);
        let max_ports = (self.bar.read32(REG_CAP) & 0x1F) + 1;
        for p in 0..max_ports.min(32) {
            if pi & (1 << p) == 0 {
                continue;
            }
            let off = PORT_BASE + p as u16 * PORT_STRIDE;
            let det = self.bar.read32(off + P_SSTS) & 0xF;
            if det == DET_PRESENT_PHY {
                self.port = p as u16;
                // 端口复位语义（COMRESET）：SCTL 由固件/QEMU 完成，此处
                // 只清 SERR 累积位（W1C）并读签名取证。
                self.bar.write32(off + P_SERR, 0xFFFF_FFFF);
                let sig = self.bar.read32(off + P_SIG);
                let _ = sig; // SIG_SATA 之外（ATAPI 等）如实不判死，IDENTIFY 再验。
                return Ok(());
            }
        }
        Err(BlockError::Unsupported) // 无在位 SATA 盘：如实拒绝。
    }

    /// 投放槽 0 命令并轮询完成。`write` = 主机→设备方向。
    fn issue(&mut self, cmd: u8, lba: u64, count: u16, write: bool, byte_len: u32) -> Result<(), BlockError> {
        let p = self.port_off();
        // PxTFD BSY/DRQ 必须空闲。
        let tfd = self.bar.read32(p + P_TFD);
        if tfd & (0x80 | 0x08) != 0 {
            return Err(BlockError::Io);
        }
        // 命令列表项 0：DW0 + CTBA。
        let ct_bus = self.mem.bus_addr(self.ct_phys);
        let mut dw0 = [0u8; 4];
        dw0.copy_from_slice(&cmd_list_dw0(write).to_le_bytes());
        self.mem.write_bytes(self.clb_phys, 0, &dw0);
        self.mem.write_bytes(self.clb_phys, 4, &((ct_bus as u32) & 0xFFFF_FFE0).to_le_bytes());
        self.mem.write_bytes(self.clb_phys, 8, &((ct_bus >> 32) as u32).to_le_bytes());
        // CFIS。
        let fis = h2d_fis(cmd, lba, count);
        self.mem.write_bytes(self.ct_phys, 0, &fis);
        // PRDT 0：数据缓冲（单 PRD，≤4KiB）。
        if byte_len > 0 {
            let prdt = prdt_bytes(self.mem.bus_addr(self.data_buf), byte_len);
            self.mem.write_bytes(self.ct_phys, 128, &prdt);
        }
        // 门铃：PxCI bit0。
        self.bar.write32(p + P_CI, 1);
        // 轮询完成：PxCI 清零。
        let deadline = self.deadline();
        loop {
            if self.bar.read32(p + P_CI) & 1 == 0 {
                break;
            }
            if (self.now)() > deadline {
                return Err(BlockError::Timeout);
            }
        }
        // RFIS status：BSY/DRQ 清零且无 ERR。
        let mut rfis = [0u8; 20];
        self.mem.read_bytes(self.fb_phys, 0x40, &mut rfis);
        let status = rfis[2];
        let error = rfis[3];
        if status & 0x80 != 0 || status & 0x08 != 0 || status & 0x01 != 0 {
            #[cfg(all(target_arch = "x86_64", target_os = "none"))]
            crate::kwarn!("ahci: cmd {:#04x} failed status={:#04x} error={:#04x}", cmd, status, error);
            return Err(BlockError::Io);
        }
        Ok(())
    }

    /// IDENTIFY：容量与块大小（word 100-103 LBA48；word 117-118 扇区大小）。
    fn identify(&mut self) -> Result<(), BlockError> {
        self.issue(ATA_IDENTIFY, 0, 0, false, 512)?;
        let mut id = [0u8; 512];
        self.mem.read_bytes(self.data_buf, 0, &mut id);
        let w = |i: usize| u16::from_le_bytes([id[i * 2], id[i * 2 + 1]]);
        // model 串（words 27..48，字节高低互换）取前 8 字符。
        for k in 0..8usize {
            let ch = id[54 + (k ^ 1)];
            self.model[k] = if (0x20..0x7F).contains(&ch) { ch } else { b' ' };
        }
        // LBA48 = words 100..103 按字序拼接（word100=LSW）。
        let sectors48 = ((w(103) as u64) << 48) | ((w(102) as u64) << 32) | ((w(101) as u64) << 16) | (w(100) as u64);
        let sec_size = if w(117) != 0 { w(117) as u32 } else { 512 };
        if sec_size == 0 || !sec_size.is_power_of_two() || sec_size > 4096 {
            return Err(BlockError::Unsupported);
        }
        self.block_size = sec_size;
        self.nblocks = if sectors48 != 0 { sectors48 } else { 0x0FFF_FFFF };
        if self.nblocks == 0 {
            return Err(BlockError::Unsupported);
        }
        Ok(())
    }

    /// 块读（≤MAX_XFER_BYTES 分段；block_size 对齐由 trait 契约保证）。
    fn read_dma(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
        let bs = self.block_size as usize;
        let count = (dst.len() / bs) as u16;
        self.issue(ATA_READ_DMA_EXT, lba, count, false, dst.len() as u32)?;
        self.mem.read_bytes(self.data_buf, 0, dst);
        Ok(())
    }

    fn write_dma(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
        let bs = self.block_size as usize;
        let count = (src.len() / bs) as u16;
        self.mem.write_bytes(self.data_buf, 0, src);
        self.issue(ATA_WRITE_DMA_EXT, lba, count, true, src.len() as u32)?;
        Ok(())
    }
}

impl<B: BarAccess, M: DmaMem> BlockDevice for AhciCtrl<B, M> {
    fn block_size(&self) -> u32 {
        self.block_size
    }
    fn capacity_blocks(&self) -> u64 {
        self.nblocks
    }
    fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
        let bs = self.block_size as usize;
        if dst.is_empty() || dst.len() % bs != 0 {
            return Err(BlockError::InvalidRange);
        }
        if lba + (dst.len() / bs) as u64 > self.nblocks {
            return Err(BlockError::InvalidRange);
        }
        let mut off = 0usize;
        let mut cur = lba;
        let remaining = dst.len() / bs;
        let per_cmd = (MAX_XFER_BYTES as usize / bs) as u64;
        let mut done = 0u64;
        while done < remaining as u64 {
            let step = (remaining as u64 - done).min(per_cmd) as usize;
            let seg = step * bs;
            self.read_dma(cur, &mut dst[off..off + seg])?;
            off += seg;
            cur += step as u64;
            done += step as u64;
        }
        Ok(())
    }
    fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
        let bs = self.block_size as usize;
        if src.is_empty() || src.len() % bs != 0 {
            return Err(BlockError::InvalidRange);
        }
        if lba + (src.len() / bs) as u64 > self.nblocks {
            return Err(BlockError::InvalidRange);
        }
        let mut off = 0usize;
        let mut cur = lba;
        let remaining = src.len() / bs;
        let per_cmd = (MAX_XFER_BYTES as usize / bs) as u64;
        let mut done = 0u64;
        while done < remaining as u64 {
            let step = (remaining as u64 - done).min(per_cmd) as usize;
            let seg = step * bs;
            self.write_dma(cur, &src[off..off + seg])?;
            off += seg;
            cur += step as u64;
            done += step as u64;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), BlockError> {
        self.issue(ATA_FLUSH_CACHE_EXT, 0, 0, false, 0)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 目标态：PCI 扫描 + 只读探针（cmdline `ahci_selftest=1` 才做写回环）。
// ---------------------------------------------------------------------------
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod target {
    use super::*;
    use super::super::nvme::target::{now_ns, BarMmio};
    use crate::drivers::blk::loopback_probe;
    use alloc::vec;

    /// 全局实例（与 NVMe/xHCI 同一手工 Once 范式）。
    static mut GLOBAL: Option<AhciCtrl<BarMmio, super::super::nvme::target::DmaBuckets>> = None;

    fn global() -> Option<&'static mut AhciCtrl<BarMmio, super::super::nvme::target::DmaBuckets>> {
        let slot = &raw mut GLOBAL;
        // SAFETY: 引导期探针单线程独占（与 NVMe 同一调用序约束）。
        unsafe { (*slot).as_mut() }
    }

    /// AHCI 探针：MCFG→ECAM 扫描 class 0x0106 → BAR 映射 → 初始化 →
    /// IDENTIFY + 读 LBA0（只读取证）。无控制器优雅跳过。
    pub fn probe_and_selftest() {
        let Some(rsdp) = crate::limine::rsdp_address() else {
            crate::kinfo!("ahci: no RSDP - sata skipped");
            return;
        };
        let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
        let Some(mcfg_phys) = crate::drivers::pci::target::find_mcfg_phys(rsdp, hhdm) else {
            crate::kinfo!("ahci: no MCFG table - sata skipped");
            return;
        };
        let Some(seg) = crate::drivers::pci::target::read_first_segment(mcfg_phys, hhdm) else {
            crate::kinfo!("ahci: MCFG has no usable segment - sata skipped");
            return;
        };
        let mut ecam = crate::drivers::pci::target::EcamMmio::new(seg);
        let Some(hit) = crate::drivers::pci::scan_ahci(&mut ecam, &seg) else {
            crate::kinfo!("ahci: no controller - sata skipped (graceful)");
            return;
        };
        crate::kinfo!(
            "ahci: controller at {:#x}:{:#x}.{} bar0={:#x}",
            hit.bus,
            hit.dev,
            hit.func,
            hit.bar0
        );
        let Some(bar) = BarMmio::map(hit.bar0, 8) else {
            crate::kwarn!("ahci: BAR0 map failed - skipped");
            return;
        };
        let buckets = super::super::nvme::target::DmaBuckets::new();
        match AhciCtrl::init_with_recovery(bar, buckets, now_ns, TIMEOUT_NS) {
            Ok(mut c) => {
                // 只读取证：读 LBA0（首扇区 MBR/GPT 头，绝不写）。
                let bs = c.block_size() as usize;
                let mut s0 = vec![0u8; bs];
                match c.read_blocks(0, &mut s0) {
                    Ok(()) => {
                        let model = core::str::from_utf8(&c.model).unwrap_or("????????");
                        crate::kinfo!(
                            "ahci: read lba0 ok model={} blocks={} bs={} sig={:#04x}{:#04x}",
                            model,
                            c.capacity_blocks(),
                            bs,
                            s0[510],
                            s0[511]
                        );
                    }
                    Err(e) => crate::kwarn!("ahci: read lba0 failed {:?}", e),
                }
                // 写回环：仅当 cmdline 显式 ahci_selftest=1（QEMU 刮擦盘；
                // 真机 SATA 盘可能承载用户数据，绝不自动写）。
                if crate::cmdline::flag("ahci_selftest") {
                    crate::kinfo!("ahci: selftest=1 - write loopback on attached disk");
                    let bs = c.block_size() as usize;
                    let mut buf = vec![0u8; bs * 8];
                    let rep = loopback_probe(&mut c, 8, 8, &mut buf);
                    crate::kinfo!(
                        "ahci: loopback passed={} rounds={} err={:?}",
                        rep.passed,
                        rep.rounds,
                        rep.err
                    );
                }
                let slot = &raw mut GLOBAL;
                // SAFETY: 引导期单线程（同 NVMe 范式）。
                unsafe {
                    if (*slot).is_none() {
                        *slot = Some(c);
                        crate::kinfo!("ahci: sata channel live");
                    } else {
                        crate::kwarn!("ahci: second controller ignored (global slot taken)");
                    }
                }
            }
            Err(e) => crate::kwarn!("ahci: init failed {:?} - sata skipped", e),
        }
    }
}

// ---------------------------------------------------------------------------
// 宿主模拟器（tests only）：与 QEMU hw/ide/ahci.c 行为同构的 HBA 模型。
// PxCI 写入即同步处理（真硬件异步完成，行为学等价）：取命令列表项 0 →
// CFIS 解码 → 内存盘 DMA（经 PRDT）→ 写 RFIS status → 清 CI。
// no_complete 注入驱动恢复路径用例。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    thread_local! {
        static CLOCK: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    }
    fn fake_now() -> u64 {
        CLOCK.with(|c| {
            let v = c.get() + 1;
            c.set(v);
            v
        })
    }
    fn reset_clock() {
        CLOCK.with(|c| c.set(0));
    }

    struct Mem {
        data: Vec<u8>,
        frames: Vec<u64>,
    }
    impl Mem {
        fn new(frames: u64) -> Mem {
            Mem {
                data: vec![0u8; (frames * PAGE) as usize],
                frames: (0..frames).map(|i| i * PAGE).collect(),
            }
        }
        fn rd(&self, phys: u64, off: u64, out: &mut [u8]) {
            let base = (phys + off) as usize;
            out.copy_from_slice(&self.data[base..base + out.len()]);
        }
        fn wr(&mut self, phys: u64, off: u64, data: &[u8]) {
            let base = (phys + off) as usize;
            self.data[base..base + data.len()].copy_from_slice(data);
        }
    }
    impl DmaMem for Mem {
        fn alloc_frame(&mut self) -> Option<u64> {
            self.frames.pop()
        }
        fn free_frame(&mut self, _k: u64) {}
        fn write_bytes(&mut self, k: u64, off: u64, d: &[u8]) {
            self.wr(k, off, d);
        }
        fn read_bytes(&self, k: u64, off: u64, out: &mut [u8]) {
            self.rd(k, off, out);
        }
        fn zero_frame(&mut self, k: u64) {
            self.wr(k, 0, &[0u8; PAGE as usize]);
        }
        fn bus_addr(&self, k: u64) -> u64 {
            k
        }
    }

    struct Port {
        clb: u64,
        fb: u64,
        cmd: u32,
        tfd: u32,
        ssts: u32,
        sig: u32,
        ci: u32,
    }

    struct Regs {
        ghc: u32,
        pi: u32,
        cap: u32,
        ports: Vec<Port>,
        disk: Vec<u8>,
        no_complete: bool,
        pub errs: Vec<String>,
    }

    impl Regs {
        fn new(blocks: usize) -> Regs {
            Regs {
                ghc: 0,
                pi: 0b11, // 端口 0/1 实现；端口 0 有盘，端口 1 空
                cap: 0x1F, // 32 端口
                ports: vec![
                    Port { clb: 0, fb: 0, cmd: 0, tfd: 0x50, ssts: 0x123, sig: SIG_SATA, ci: 0 },
                    Port { clb: 0, fb: 0, cmd: 0, tfd: 0x7F, ssts: 0, sig: 0, ci: 0 },
                ],
                disk: vec![0u8; blocks * 512],
                no_complete: false,
                errs: Vec::new(),
            }
        }

        fn identify_data(&self) -> Vec<u8> {
            let mut id = vec![0u8; 512];
            let mut w = |i: usize, v: u16| {
                id[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
            };
            let blocks = (self.disk.len() / 512) as u64;
            w(100, (blocks & 0xFFFF) as u16);
            w(101, ((blocks >> 16) & 0xFFFF) as u16);
            w(102, ((blocks >> 32) & 0xFFFF) as u16);
            w(103, ((blocks >> 48) & 0xFFFF) as u16);
            w(117, 512);
            w(118, 0);
            for (k, ch) in b"QEMUSATA".iter().enumerate() {
                id[54 + (k ^ 1)] = *ch;
            }
            id
        }

        fn process_cmd(&mut self, mem: &mut Mem, pidx: usize) {
            let p = &self.ports[pidx];
            if p.cmd & CMD_ST == 0 || self.no_complete {
                return;
            }
            // 命令列表项 0。
            let mut hdr = [0u8; 32];
            mem.rd(p.clb, 0, &mut hdr);
            let dw0 = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
            let write = dw0 & (1 << 6) != 0;
            let ctba = u64::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7], hdr[8], hdr[9], hdr[10], hdr[11]]);
            // CFIS。
            let mut fis = [0u8; 20];
            mem.rd(ctba, 0, &mut fis);
            if fis[0] & 0x7F != 0x27 || fis[0] & 0x80 == 0 {
                self.errs.push(format!("bad FIS type {:#04x}", fis[0]));
                return;
            }
            let cmd = fis[2];
            let lba = (fis[4] as u64)
                | ((fis[5] as u64) << 8)
                | ((fis[6] as u64) << 16)
                | ((fis[8] as u64) << 24)
                | ((fis[9] as u64) << 32)
                | ((fis[10] as u64) << 40);
            let count = fis[12] as u16 | ((fis[13] as u16) << 8);
            // PRDT 0。
            let mut prd = [0u8; 16];
            mem.rd(ctba, 128, &mut prd);
            let dba = u64::from_le_bytes([prd[0], prd[1], prd[2], prd[3], prd[4], prd[5], prd[6], prd[7]]);
            let dbc = (u32::from_le_bytes([prd[8], prd[9], prd[10], prd[11]]) & 0x3F_FFFF) as usize + 1;
            let status = 0x50u8; // DRDY|SEEK：BSY/DRQ/ERR 全清
            match cmd {
                ATA_IDENTIFY => {
                    let id = self.identify_data();
                    mem.wr(dba, 0, &id[..dbc]);
                }
                ATA_READ_DMA_EXT => {
                    let start = lba as usize * 512;
                    let n = count as usize * 512;
                    let seg = self.disk[start..start + n].to_vec();
                    mem.wr(dba, 0, &seg[..dbc.min(n)]);
                }
                ATA_WRITE_DMA_EXT => {
                    let start = lba as usize * 512;
                    let n = count as usize * 512;
                    let mut seg = vec![0u8; n];
                    let take = dbc.min(n);
                    mem.rd(dba, 0, &mut seg[..take]);
                    self.disk[start..start + n].copy_from_slice(&seg);
                }
                ATA_FLUSH_CACHE_EXT => {}
                other => {
                    self.errs.push(format!("unknown cmd {:#04x}", other));
                }
            }
            let _ = write;
            // 写 RFIS + 清 CI。
            let mut rfis = [0u8; 20];
            rfis[0] = 0x34;
            rfis[2] = status;
            mem.wr(p.fb, 0x40, &rfis);
            self.ports[pidx].ci &= !1;
        }

        fn do_reset(&mut self) {
            for p in self.ports.iter_mut() {
                p.ci = 0;
                p.cmd &= !(CMD_ST | CMD_FRE);
            }
            self.ghc &= !GHC_HR;
        }
    }

    // ---- 寄存器/内存共享句柄 ------------------------------------------------
    #[derive(Clone)]
    struct Dev {
        regs: Rc<RefCell<Regs>>,
        mem: Rc<RefCell<Mem>>,
    }

    impl Dev {
        fn new(blocks: usize) -> Dev {
            Dev {
                regs: Rc::new(RefCell::new(Regs::new(blocks))),
                mem: Rc::new(RefCell::new(Mem::new(8))),
            }
        }
    }

    impl BarAccess for Dev {
        fn read32(&mut self, off: u16) -> u32 {
            let r = self.regs.borrow();
            if off < PORT_BASE {
                return match off {
                    REG_CAP => r.cap,
                    REG_GHC => r.ghc,
                    REG_PI => r.pi,
                    _ => 0,
                };
            }
            let pidx = ((off - PORT_BASE) / PORT_STRIDE) as usize;
            let po = (off - PORT_BASE) % PORT_STRIDE;
            let p = &r.ports[pidx];
            match po {
                P_CLB => p.clb as u32,
                P_CLBU => (p.clb >> 32) as u32,
                P_FB => p.fb as u32,
                P_FBU => (p.fb >> 32) as u32,
                P_CMD => p.cmd,
                P_TFD => p.tfd,
                P_SIG => p.sig,
                P_SSTS => p.ssts,
                P_CI => p.ci,
                _ => 0,
            }
        }
        fn write32(&mut self, off: u16, v: u32) {
            if off < PORT_BASE {
                let mut r = self.regs.borrow_mut();
                if off == REG_GHC {
                    if v & GHC_HR != 0 {
                        r.do_reset();
                    }
                    // HR 是自清位：复位完成后寄存器读值不再含 HR。
                    r.ghc = v & (GHC_AE | GHC_IE);
                }
                return;
            }
            let pidx = ((off - PORT_BASE) / PORT_STRIDE) as usize;
            let po = (off - PORT_BASE) % PORT_STRIDE;
            let mut r = self.regs.borrow_mut();
            let p = &mut r.ports[pidx];
            match po {
                P_CLB => p.clb = (p.clb & 0xFFFF_FFFF_0000_0000) | v as u64,
                P_CLBU => p.clb = (p.clb & 0xFFFF_FFFF) | ((v as u64) << 32),
                P_FB => p.fb = (p.fb & 0xFFFF_FFFF_0000_0000) | v as u64,
                P_FBU => p.fb = (p.fb & 0xFFFF_FFFF) | ((v as u64) << 32),
                P_CMD => {
                    let old = p.cmd;
                    p.cmd = (p.cmd & !(CMD_ST | CMD_FRE)) | (v & (CMD_ST | CMD_FRE));
                    let _ = old;
                }
                P_SERR => {} // W1C：整写清光
                P_CI => {
                    p.ci |= v;
                    drop(r);
                    let mut mem = self.mem.borrow_mut();
                    let mut r2 = self.regs.borrow_mut();
                    r2.process_cmd(&mut mem, pidx);
                }
                _ => {}
            }
        }
    }

    impl DmaMem for Dev {
        fn alloc_frame(&mut self) -> Option<u64> {
            self.mem.borrow_mut().frames.pop()
        }
        fn free_frame(&mut self, phys: u64) {
            self.mem.borrow_mut().frames.push(phys);
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            self.mem.borrow_mut().wr(phys, off, data);
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            self.mem.borrow().rd(phys, off, out);
        }
        fn zero_frame(&mut self, phys: u64) {
            self.mem.borrow_mut().wr(phys, 0, &[0u8; PAGE as usize]);
        }
        fn bus_addr(&self, key: u64) -> u64 {
            key
        }
    }

    fn bringup(blocks: usize) -> (Dev, AhciCtrl<Dev, Dev>) {
        reset_clock();
        let dev = Dev::new(blocks);
        let c = AhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000)
            .expect("初始化必须成功");
        (dev, c)
    }

    // ---- 用例 ---------------------------------------------------------------

    #[test]
    fn ahci_h2d_fis_golden_bytes() {
        let f = h2d_fis(ATA_READ_DMA_EXT, 0x0123_4567_89AB, 9);
        assert_eq!(f[0], 0x27 | 0x80);
        assert_eq!(f[1], 0x00);
        assert_eq!(f[2], ATA_READ_DMA_EXT);
        assert_eq!(f[7], 0x40);
        assert_eq!(f[4], 0xAB);
        assert_eq!(f[5], 0x89);
        assert_eq!(f[6], 0x67);
        assert_eq!(f[8], 0x45);
        assert_eq!(f[9], 0x23);
        assert_eq!(f[10], 0x01);
        assert_eq!(f[12], 9);
        assert_eq!(f[13], 0);
        // 写方向命令的列表项/PRDT 编码。
        assert_eq!(cmd_list_dw0(true) & (1 << 6), 1 << 6);
        assert_eq!(cmd_list_dw0(false) & (1 << 6), 0);
        assert_eq!(cmd_list_dw0(false) & 0x1F, 5, "CFL=5");
        let p = prdt_bytes(0x1234_5000, 4096);
        assert_eq!(&p[0..4], &0x1234_5000u32.to_le_bytes());
        assert_eq!(u32::from_le_bytes(p[8..12].try_into().unwrap()), 4095);
    }

    #[test]
    fn ahci_identify_and_geometry() {
        let (_dev, c) = bringup(64);
        assert_eq!(c.block_size(), 512);
        assert_eq!(c.capacity_blocks(), 64);
        assert_eq!(&c.model, b"QEMUSATA");
        // 模拟器无错误日志。
        let _ = _dev.regs.borrow().errs.clone();
        assert!(_dev.regs.borrow().errs.is_empty());
        // 选中端口 = 端口 0（端口 1 无盘）。
        assert!(_dev.regs.borrow().ports[0].cmd & CMD_ST != 0);
        assert!(_dev.regs.borrow().ports[1].cmd & CMD_ST == 0);
    }

    #[test]
    fn ahci_read_write_loopback_and_flush() {
        let (dev, mut c) = bringup(64);
        let mut w = [0u8; 512];
        for (i, b) in w.iter_mut().enumerate() {
            *b = (i * 11 + 7) as u8;
        }
        c.write_blocks(10, &w).expect("单块写必须成功");
        let mut r = [0u8; 512];
        c.read_blocks(10, &mut r).expect("单块读必须成功");
        assert_eq!(w, r);
        // 跨命令分块（8 块/命令上限 → 20 块拆 3 条命令）。
        let big: Vec<u8> = (0..20 * 512).map(|i| (i * 3) as u8).collect();
        c.write_blocks(16, &big).expect("多块写必须成功");
        let mut back = vec![0u8; 20 * 512];
        c.read_blocks(16, &mut back).expect("多块读必须成功");
        assert_eq!(big, back);
        // 越界/非整块拒绝。
        assert_eq!(c.read_blocks(60, &mut [0u8; 10 * 512]), Err(BlockError::InvalidRange));
        assert_eq!(c.read_blocks(0, &mut [0u8; 100]), Err(BlockError::InvalidRange));
        assert!(c.flush().is_ok());
        // 模拟器无错误日志。
        assert!(dev.regs.borrow().errs.is_empty());
    }

    #[test]
    fn ahci_timeout_recovery_retries_once() {
        reset_clock();
        let dev = Dev::new(64);
        dev.regs.borrow_mut().no_complete = true;
        let e = match AhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000) {
            Ok(_) => panic!("持续失败必须报错"),
            Err(e) => e,
        };
        assert_eq!(e, BlockError::DeviceReset, "重试耗尽=DeviceReset 口径");
    }

    #[test]
    fn ahci_no_controller_reports_unsupported() {
        reset_clock();
        let dev = Dev::new(64);
        dev.regs.borrow_mut().pi = 0; // 无实现端口
        let e = match AhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000) {
            Ok(_) => panic!("无盘必须报错"),
            Err(e) => e,
        };
        assert_eq!(e, BlockError::Unsupported);
    }
}
