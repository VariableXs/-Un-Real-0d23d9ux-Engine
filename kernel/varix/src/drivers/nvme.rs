//! 任务16 · NVMe 最小栈——提交/完成队列对 + 识别/读写命令
//! （双域总案·2.3 真驱动·步骤 9）。
//!
//! **命令集范围如实声明**：admin 只实现 `Identify`（CNS=1 控制器 /
//! CNS=0 命名空间，NVMe 规范 5.15）、`Create I/O CQ/SQ`；I/O 只实现
//! `Read`/`Write`/`Flush`。不做的（admin 全集、fabrics、reservation、
//! vendor）一律 `Unsupported` 口径如实上报，不假装成功。
//!
//! **中断模型如实声明**：最小栈为纯轮询（无 MSI-X 注册）。总案要求
//! 的"超时/中断丢失恢复路径"由轮询超时天然承担：命令等待超时 →
//! `CC.EN` 复位 → 完整重初始化（重试一次）→ 仍失败如实 `DeviceReset`。
//! 真中断注册属任务 16 之后的 MSI-X 增量。
//!
//! **DMA 与一致性**：PRP 单页语义（每个事务 ≤4KiB，单 PRP1）。DMA
//! 缓冲来自 [`DmaMem::alloc_frame`]（4KiB 对齐物理帧）。目标态用普通
//! WB 页做 DMA——QEMU TCG 无缓存一致性差异；真实硬件需按平台口径
//! 维护 cache，此限制显式登记不藏在代码里。
//!
//! **门铃写序（memory barrier 契约，总案点名要求）**：
//! 1. 写 SQ entry（DMA 内存）→ `fence(Release)` → 写 SQ tail 门铃
//!    ——控制器经门铃看到队列前，entry 数据已全局可见；
//! 2. 读到 CQE（phase 翻转）→ `fence(Acquire)` → 推进 CQ head 门铃
//!    ——head 推进前本条 CQE 已读全。
//! 所有寄存器访问经 `volatile`（[`BarAccess`]），绝不普通读写。
//!
//! **可测性架构**：控制流对 `B: BarAccess + M: DmaMem` 泛型——宿主用
//! `tests` 模块内的寄存器模拟器（含命令语义应答、phase 翻转、enable
//! 参数校验、超时注入）跑通 enable→identify→建 IO 队列→读写的完整
//! 序列；目标态只替换真 MMIO/真 DMA 桶两层薄实现。

use super::blk::{BlockDevice, BlockError};

// ---- NVMe opcode（支持集如实冻结）--------------------------------------
pub const OP_ADMIN_CREATE_SQ: u8 = 0x01;
pub const OP_ADMIN_CREATE_CQ: u8 = 0x05;
pub const OP_ADMIN_IDENTIFY: u8 = 0x06;
pub const OP_IO_WRITE: u8 = 0x01;
pub const OP_IO_READ: u8 = 0x02;
pub const OP_IO_FLUSH: u8 = 0x00; // NVMe NVM Flush=0x00（0x08 是 Write Zeroes！）

// ---- 寄存器布局（NVMe 1.x BAR0）----------------------------------------
pub const REG_CAP: u16 = 0x00; // 64B：[47:0] MQES(max-1)，[35:32] DSTRD
pub const REG_VS: u16 = 0x08;
pub const REG_CC: u16 = 0x14; // EN | CSS | IOSQES[19:16] | IOCQES[23:20]
pub const REG_CST: u16 = 0x1C; // RDY
pub const REG_AQA: u16 = 0x24; // ASQS[11:0] | ACQS[27:16]
pub const REG_ASQ: u16 = 0x28; // 64B：0x28 lo / 0x2C hi
pub const REG_ACQ: u16 = 0x30; // 64B：0x30 lo / 0x34 hi
pub const DB_ADMIN_SQ: u16 = 0x1000;
pub const DB_ADMIN_CQ: u16 = 0x1004;
pub const REG_CAP_HI: u16 = REG_CAP + 4; // DSTRD 在 CAP[35:32]
pub const REG_ASQ_HI: u16 = REG_ASQ + 4;
pub const REG_ACQ_HI: u16 = REG_ACQ + 4;

pub const QUEUE_ENTRIES: usize = 64; // 每队列 1 页（SQ 64×64B / CQ 64×16B）
pub const PAGE: u64 = 4096;

/// 提交队列条目（64B = 16×u32；未用字段恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Submission(pub [u32; 16]);

impl Submission {
    /// SQE 布局（QEMU NvmeCmd 结构体 + Linux nvme_common_command 双源码
    /// 实证）：DW0=opcode|flags|CID、DW1=NSID、DW2/3=res1、**DW4/5=MPTR、
    /// DW6/7=PRP1、DW8/9=PRP2**、DW10..12=CDW10..12。此前 PRP1 先后误放
    /// DW2/3 与 DW4/5——实机探针（offset16 写特征值、QEMU 仍读 0）锁定
    /// QEMU 只从 DW6/7 取 PRP1。
    fn raw(opcode: u8, cid: u16, nsid: u32, prp1: u64, dw10: u32, dw11: u32, dw12: u32) -> Submission {
        let mut s = [0u32; 16];
        s[0] = (opcode as u32) | ((cid as u32) << 16);
        s[1] = nsid;
        s[6] = prp1 as u32;
        s[7] = (prp1 >> 32) as u32;
        s[10] = dw10;
        s[11] = dw11;
        s[12] = dw12;
        Submission(s)
    }

    /// IO Read：DW10/11=SLBA，DW12[15:0]=NLB-1。
    pub fn io_read(cid: u16, nsid: u32, lba: u64, nblocks: u32, prp1: u64) -> Submission {
        Self::raw(OP_IO_READ, cid, nsid, prp1, lba as u32, (lba >> 32) as u32, nblocks - 1)
    }

    /// IO Write：同 Read（opcode 不同）。
    pub fn io_write(cid: u16, nsid: u32, lba: u64, nblocks: u32, prp1: u64) -> Submission {
        Self::raw(OP_IO_WRITE, cid, nsid, prp1, lba as u32, (lba >> 32) as u32, nblocks - 1)
    }

    /// IO Flush。
    pub fn io_flush(cid: u16, nsid: u32) -> Submission {
        Self::raw(OP_IO_FLUSH, cid, nsid, 0, 0, 0, 0)
    }

    /// Admin Identify：DW10=CNS（1=控制器 / 0=命名空间，规范 5.15），DW1=NSID。
    pub fn admin_identify(cid: u16, cns: u32, nsid: u32, prp1: u64) -> Submission {
        Self::raw(OP_ADMIN_IDENTIFY, cid, nsid, prp1, cns, 0, 0)
    }

    /// Create I/O CQ（QEMU NvmeCreateCq 结构体实证）：
    /// DW10=[15:0]CQID|[31:16]QSIZE；DW11=[15:0]flags(IEN@1|PC@0)、[31:16]IRQ。
    pub fn admin_create_cq(cid: u16, qid: u32, phys: u64, entries: usize) -> Submission {
        Self::raw(
            OP_ADMIN_CREATE_CQ,
            cid,
            0,
            phys,
            (qid as u32 & 0xFFFF) | ((entries as u32 - 1) << 16),
            0x3, // IEN(bit1) | Physically Contiguous(bit0)；IRQ=0
            0,
        )
    }

    /// Create I/O SQ（QEMU NvmeCreateSq 结构体实证）：
    /// DW10=[15:0]SQID|[31:16]QSIZE；DW11=[15:0]flags(PC@0)、[31:16]CQID。
    pub fn admin_create_sq(cid: u16, qid: u32, phys: u64, entries: usize, cqid: u32) -> Submission {
        Self::raw(
            OP_ADMIN_CREATE_SQ,
            cid,
            0,
            phys,
            (qid as u32 & 0xFFFF) | ((entries as u32 - 1) << 16),
            ((cqid as u32) << 16) | 0x1, // Physically Contiguous(bit0) | CQID<<16
            0,
        )
    }

    /// 64B 条目 → 小端字节流（DMA 内存布局）。
    pub fn le_bytes(&self) -> [u8; 64] {
        let mut out = [0u8; 64];
        for (i, w) in self.0.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        out
    }
}

/// 完成队列条目（16B）。布局与 QEMU/SeaBIOS/Linux 三方实现一致
/// （NvmeCqe 结构体原文实证）：DW2[15:0]=SQHD、DW2[31:16]=SQID、
/// **DW3[15:0]=CID、DW3[31:16]=status**——P=status bit0（即 32 位
/// DW3 的 bit16），错误码=status bit15:1（DNR|More|SCT|SC 组合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Completion(pub [u32; 4]);

impl Completion {
    /// CID：DW3[15:0]。
    pub fn cid(&self) -> u16 {
        (self.0[3] & 0xFFFF) as u16
    }
    /// 原始 16 位 status（P=bit0 + 错误码 bit15:1）。
    pub fn status_raw(&self) -> u16 {
        (self.0[3] >> 16) as u16
    }
    /// 错误码（P 位移除后）；0=成功。与 QEMU 内部 req->status 同构
    /// （如 INVALID_NS|DNR = 0x400b）。
    pub fn status(&self) -> u16 {
        self.status_raw() >> 1
    }
    pub fn phase(&self) -> bool {
        self.status_raw() & 1 != 0
    }
}

/// 从 16B 字节流解析 CQE。
pub fn parse_completion(b: &[u8; 16]) -> Completion {
    let mut c = [0u32; 4];
    for (i, slot) in c.iter_mut().enumerate() {
        *slot = u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap_or([0; 4]));
    }
    Completion(c)
}

// ---- 设备访问抽象层 ------------------------------------------------------

/// BAR0 寄存器（含门铃）32 位访问。所有实现必须 volatile。
pub trait BarAccess {
    fn read32(&mut self, off: u16) -> u32;
    fn write32(&mut self, off: u16, val: u32);
}

/// DMA 内存抽象：4KiB 物理帧桶。读写按帧内偏移小缓冲。
pub trait DmaMem {
    /// 分配一个 4KiB 对齐物理帧。
    fn alloc_frame(&mut self) -> Option<u64>;
    /// 归还帧（可选实现）。
    fn free_frame(&mut self, _phys: u64) {}
    /// 物理地址 → 写入（off 为帧内偏移）。
    fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]);
    /// 物理地址 → 读出。
    fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]);
    /// 整帧清零。建队/复用帧前必须调用：残留 CQE 的 phase 位若恰好
    /// 与本队初始 phase 相同，poll 会假消费（QEMU 门铃是 BH 异步，
    /// 假消费抢在真完成之前），cid 校验虽拦住脏数据但该命令就此丢失。
    fn zero_frame(&mut self, phys: u64) {
        const Z: [u8; 256] = [0u8; 256];
        let mut off = 0u64;
        while off < PAGE {
            self.write_bytes(phys, off, &Z);
            off += 256;
        }
    }
}

// ---- 控制流（宿主/目标共用）---------------------------------------------

/// NVMe 控制器最小栈。生命周期：[`NvmeCtrl::init_with_recovery`] →
/// [`BlockDevice`] 语义读写。
pub struct NvmeCtrl<B: BarAccess, M: DmaMem> {
    bar: B,
    mem: M,
    /// 时钟注入（目标态=clock::now_ns；宿主=可控计数器）。
    now: fn() -> u64,
    timeout_ns: u64,
    stride: u32,
    cid: u16,
    nsid: u32,
    block_size: u32,
    nblocks: u64,
    // admin 队列软件侧指针。
    admin_sq_phys: u64,
    admin_cq_phys: u64,
    admin_tail: usize,
    admin_cq_head: usize,
    admin_phase: bool,
    // io 队列（qid=1）。
    io_sq_phys: u64,
    io_cq_phys: u64,
    io_tail: usize,
    io_cq_head: usize,
    io_phase: bool,
    io_buf: u64,
    /// 验收证据：恢复路径触发计数。
    pub resets: u32,
}

/// 一次完整初始化尝试。失败时归还 bar/mem 供重试。
fn try_init<B: BarAccess, M: DmaMem>(
    bar: B,
    mem: M,
    now: fn() -> u64,
    timeout_ns: u64,
    resets: u32,
) -> Result<NvmeCtrl<B, M>, (B, M, BlockError)> {
    let mut c = NvmeCtrl {
        bar,
        mem,
        now,
        timeout_ns,
        stride: 4,
        cid: 0,
        nsid: 1,
        block_size: 0,
        nblocks: 0,
        admin_sq_phys: 0,
        admin_cq_phys: 0,
        admin_tail: 0,
        admin_cq_head: 0,
        admin_phase: true,
        io_sq_phys: 0,
        io_cq_phys: 0,
        io_tail: 0,
        io_cq_head: 0,
        io_phase: true,
        io_buf: 0,
        resets,
    };
    // 任一步失败把 bar/mem 原样归还（恢复路径整段重初始化需要）。
    macro_rules! bail {
        ($e:expr) => {
            if let Err(e) = $e {
                return Err((c.bar, c.mem, e));
            }
        };
    }
    bail!(c.controller_reset());
    bail!(c.enable_admin_queues());
    bail!(c.identify_namespace());
    bail!(c.create_io_queues());
    Ok(c)
}

impl<B: BarAccess, M: DmaMem> NvmeCtrl<B, M> {
    /// 完整初始化（含恢复路径）：复位 → admin 使能 → identify → 建 IO 队列。
    /// 任何一步超时 → 完整复位重试一次 → 仍失败如实 `DeviceReset`。
    pub fn init_with_recovery(bar: B, mem: M, now: fn() -> u64, timeout_ns: u64) -> Result<Self, BlockError> {
        let mut resets = 0;
        let (mut bar, mut mem) = (bar, mem);
        loop {
            match try_init(bar, mem, now, timeout_ns, resets) {
                Ok(c) => return Ok(c),
                Err((b, m, BlockError::Timeout)) if resets < 1 => {
                    // 恢复路径：整段重初始化（重试一次），计数留证。
                    resets += 1;
                    bar = b;
                    mem = m;
                }
                // 重试耗尽仍超时 = 控制器复位后依然不 ready —— 如实
                // 上报 DeviceReset（不是简单 Timeout）。
                Err((_, _, BlockError::Timeout)) => return Err(BlockError::DeviceReset),
                Err((_, _, e)) => return Err(e),
            }
        }
    }

    /// CC.EN=0 → 等 CST.RDY=0（复位完成）。
    fn controller_reset(&mut self) -> Result<(), BlockError> {
        let cc = self.bar.read32(REG_CC);
        #[cfg(all(target_arch = "x86_64", target_os = "none"))]
        crate::kinfo!(
            "nvme: reset begin CC={:#x} CST={:#x}",
            cc,
            self.bar.read32(REG_CST)
        );
        if cc & 1 != 0 {
            self.bar.write32(REG_CC, cc & !1); // EN=0
        }
        self.wait_reg_bit(REG_CST, 1, false)
    }

    /// 等某寄存器某位到达期望值；超时 `Timeout`。
    fn wait_reg_bit(&mut self, reg: u16, mask: u32, want: bool) -> Result<(), BlockError> {
        let deadline = (self.now)() + self.timeout_ns;
        loop {
            let v = self.bar.read32(reg);
            if ((v & mask) != 0) == want {
                return Ok(());
            }
            if (self.now)() > deadline {
                return Err(BlockError::Timeout);
            }
        }
    }

    /// CAP 读取 → AQA/ASQ/ACQ 写入 → CC.EN=1（IOSQES=6 / IOCQES=4）→ RDY=1。
    fn enable_admin_queues(&mut self) -> Result<(), BlockError> {
        let cap_lo = self.bar.read32(REG_CAP);
        let cap_hi = self.bar.read32(REG_CAP_HI);
        let dstrd = cap_hi & 0xF; // DSTRD 在 CAP[35:32]。
        self.stride = 4u32 << dstrd;
        let mqes = (cap_lo & 0xFFFF) as usize;
        if mqes + 1 < QUEUE_ENTRIES {
            return Err(BlockError::Unsupported); // 队列容量不足，如实失败。
        }
        let (Some(sq), Some(cq)) = (self.mem.alloc_frame(), self.mem.alloc_frame()) else {
            return Err(BlockError::Io);
        };
        self.admin_sq_phys = sq;
        self.admin_cq_phys = cq;
        // 复用帧必须清零：残留 phase 位会假匹配（见 zero_frame 文档）。
        self.mem.zero_frame(sq);
        self.mem.zero_frame(cq);
        // AQA：admin 两队列各 64 条目（ASQS/ACQS 都是 size-1）。
        self.bar.write32(
            REG_AQA,
            (QUEUE_ENTRIES as u32 - 1) | ((QUEUE_ENTRIES as u32 - 1) << 16),
        );
        // ASQ/ACQ：64 位基址（4KiB 对齐帧，低 12 位天然 0）。
        self.bar.write32(REG_ASQ, sq as u32);
        self.bar.write32(REG_ASQ + 4, (sq >> 32) as u32);
        self.bar.write32(REG_ACQ, cq as u32);
        self.bar.write32(REG_ACQ + 4, (cq >> 32) as u32);
        // CC：EN=1 | CSS=0(NVM) | IOSQES=6(64B) | IOCQES=4(16B)。
        self.bar.write32(REG_CC, 1u32 | (6u32 << 16) | (4u32 << 20));
        #[cfg(all(target_arch = "x86_64", target_os = "none"))]
        crate::kinfo!(
            "nvme: enable CC={:#x} CST={:#x} (mqes={} stride={})",
            self.bar.read32(REG_CC),
            self.bar.read32(REG_CST),
            mqes + 1,
            self.stride
        );
        self.wait_reg_bit(REG_CST, 1, true)
    }

    /// Admin 命令提交 → 门铃 → 轮询 CQE（cid 校验）→ 推进 head。
    fn admin_submit(&mut self, mut cmd: Submission) -> Result<Completion, BlockError> {
        // cid 由栈统一分配并覆写进命令——调用方构造时无需预占，杜绝
        // "调用方取的 cid 与栈内自增错位"的错配。
        let cid = self.next_cid();
        cmd.0[0] = (cmd.0[0] & 0xFFFF) | ((cid as u32) << 16);
        let slot = self.admin_tail;
        // 1) entry 先落 DMA 内存。
        let bytes = cmd.le_bytes();
        self.mem.write_bytes(self.admin_sq_phys, slot as u64 * 64, &bytes);
        // 门铃写序：entry 全局可见先于 doorbell（Release）。
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        // 2) 写 admin SQ tail 门铃。
        self.bar.write32(DB_ADMIN_SQ, ((slot + 1) % QUEUE_ENTRIES) as u32);
        self.admin_tail = (slot + 1) % QUEUE_ENTRIES;
        // 3) 轮询 CQE phase 翻转。
        let deadline = deadline_of(self.now, self.timeout_ns);
        let cqe = poll_cqe(
            &mut self.bar,
            &self.mem,
            self.now,
            self.admin_cq_phys,
            &mut self.admin_cq_head,
            &mut self.admin_phase,
            DB_ADMIN_CQ,
            deadline,
        )?;
        if cqe.cid() != cid {
            return Err(BlockError::Io); // 顺序栈收到他条 cid=乱序，如实报错。
        }
        if cqe.status() != 0 {
            return Err(BlockError::Io);
        }
        Ok(cqe)
    }

    /// Identify 两条命令（NVMe 1.x 规范 5.15）：
    /// CNS=1 Identify Controller（NSID 填 0）→ 结构 byte516(0x204)=NN；
    /// CNS=0 Identify Namespace（NSID=有效命名空间）→ DW0=NSZE、byte26=FLBAS、
    /// LBAF[flbas].LBADS @ byte128+flbas*4+2。**此前 CNS 语义写反**（第一发
    /// CNS=0+NSID=0 被真实控制器拒为 Invalid Namespace，QEMU status 0x400b
    /// =INVALID_NS|DNR 铁证），宿主模拟器一并按规范修正。
    fn identify_namespace(&mut self) -> Result<(), BlockError> {
        let Some(id_page) = self.mem.alloc_frame() else {
            return Err(BlockError::Io);
        };
        let zeros = [0u8; 4096];
        // 1) identify controller（CNS=1，NSID=0）→ NN。
        self.mem.write_bytes(id_page, 0, &zeros);
        self.admin_submit(Submission::admin_identify(0, 1, 0, id_page))?;
        let mut page = [0u8; 4096];
        self.mem.read_bytes(id_page, 0, &mut page);
        let nn = u32::from_le_bytes(page[0x204..0x208].try_into().unwrap_or([0; 4]));
        if nn == 0 {
            return Err(BlockError::Unsupported); // 无命名空间，如实上报。
        }
        // 2) identify namespace（CNS=0，NSID=首个命名空间）。最小栈只挂
        //    ns1；多命名空间枚举超出范围，如实声明不做。
        self.mem.write_bytes(id_page, 0, &zeros);
        self.admin_submit(Submission::admin_identify(0, 0, self.nsid, id_page))?;
        self.mem.read_bytes(id_page, 0, &mut page);
        self.nblocks = u64::from_le_bytes(page[0..8].try_into().unwrap_or([0; 8]));
        let flbas = (page[26] & 0xF) as usize;
        let lbads = page[0x80 + flbas * 4 + 2];
        self.block_size = 1u32 << lbads;
        self.mem.free_frame(id_page);
        if self.block_size < 512 || self.block_size > 4096 || self.nblocks == 0 {
            return Err(BlockError::Unsupported);
        }
        Ok(())
    }

    /// 建 IO CQ → IO SQ（qid=1，各 1 页）。
    fn create_io_queues(&mut self) -> Result<(), BlockError> {
        let (Some(cq), Some(sq), Some(buf)) = (
            self.mem.alloc_frame(),
            self.mem.alloc_frame(),
            self.mem.alloc_frame(),
        ) else {
            return Err(BlockError::Io);
        };
        self.io_cq_phys = cq;
        self.io_sq_phys = sq;
        self.io_buf = buf;
        // 复用帧必须清零：本任务的实机失败正源于 identify 页复用残留
        // （QEMU BH 异步下 poll 假消费）。
        self.mem.zero_frame(cq);
        self.mem.zero_frame(sq);
        self.mem.zero_frame(buf);
        self.admin_submit(Submission::admin_create_cq(0, 1, cq, QUEUE_ENTRIES))?;
        self.admin_submit(Submission::admin_create_sq(0, 1, sq, QUEUE_ENTRIES, 1))?;
        Ok(())
    }

    fn next_cid(&mut self) -> u16 {
        let c = self.cid;
        self.cid = self.cid.wrapping_add(1);
        c
    }

    /// IO 队列门铃偏移：DB0 + stride*(2*qid + kind)，qid=1。
    fn io_doorbell(&self, kind: u32) -> u16 {
        DB_ADMIN_SQ + (self.stride * (2 + kind)) as u16
    }

    /// IO 提交 + 等待完成（read/write/flush 共用尾段）。
    fn io_submit_and_wait(&mut self, cmd: Submission, cid: u16) -> Result<(), BlockError> {
        let slot = self.io_tail;
        let bytes = cmd.le_bytes();
        self.mem.write_bytes(self.io_sq_phys, slot as u64 * 64, &bytes);
        // 门铃写序：entry 全局可见先于 doorbell（Release）。
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        self.bar.write32(self.io_doorbell(0), ((slot + 1) % QUEUE_ENTRIES) as u32);
        self.io_tail = (slot + 1) % QUEUE_ENTRIES;
        let deadline = deadline_of(self.now, self.timeout_ns);
        let db = self.io_doorbell(1); // 先取值再借 bar（参数求值顺序借用隔离）。
        let cqe = poll_cqe(
            &mut self.bar,
            &self.mem,
            self.now,
            self.io_cq_phys,
            &mut self.io_cq_head,
            &mut self.io_phase,
            db,
            deadline,
        )?;
        if cqe.cid() != cid || cqe.status() != 0 {
            #[cfg(all(target_arch = "x86_64", target_os = "none"))]
            crate::kwarn!(
                "nvme: io cqe mismatch cid={} got={} status={:#x}",
                cid,
                cqe.cid(),
                cqe.status()
            );
            return Err(BlockError::Io);
        }
        Ok(())
    }

    /// 单片读（≤4KiB，单 PRP）：设备 → DMA 桶 → dst。
    fn io_read_chunk(&mut self, lba: u64, nblocks: u32, dst: &mut [u8]) -> Result<(), BlockError> {
        let cid = self.next_cid();
        let cmd = Submission::io_read(cid, self.nsid, lba, nblocks, self.io_buf);
        self.io_submit_and_wait(cmd, cid)?;
        self.mem.read_bytes(self.io_buf, 0, dst);
        Ok(())
    }

    /// 单片写（≤4KiB，单 PRP）：src → DMA 桶 → 设备。
    fn io_write_chunk(&mut self, lba: u64, nblocks: u32, src: &[u8]) -> Result<(), BlockError> {
        let cid = self.next_cid();
        self.mem.write_bytes(self.io_buf, 0, src);
        let cmd = Submission::io_write(cid, self.nsid, lba, nblocks, self.io_buf);
        self.io_submit_and_wait(cmd, cid)
    }

    /// 容量与块大小（验收证据与宿主断言用）。
    pub fn geometry(&self) -> (u32, u64) {
        (self.block_size, self.nblocks)
    }

    fn check_range(&self, lba: u64, bytes: usize) -> Result<u32, BlockError> {
        let bs = self.block_size as usize;
        if bs == 0 || bytes == 0 || bytes % bs != 0 {
            return Err(BlockError::InvalidRange);
        }
        let n = (bytes / bs) as u64;
        lba.checked_add(n)
            .filter(|&end| end <= self.nblocks)
            .ok_or(BlockError::InvalidRange)?;
        Ok(n as u32)
    }
}

fn deadline_of(now: fn() -> u64, timeout_ns: u64) -> u64 {
    now() + timeout_ns
}

/// 轮询一条 CQE：phase 翻转即取，head 推进 + wrap 翻 phase + 写 head 门铃。
/// 自由函数形态：bar 可变 / mem 共享 / head+phase 可变三者是同一 self 的
/// 不同字段——字段级 disjoint borrow 只有显式拆参才立得住。
fn poll_cqe<B: BarAccess, M: DmaMem>(
    bar: &mut B,
    mem: &M,
    now: fn() -> u64,
    cq_phys: u64,
    head: &mut usize,
    phase: &mut bool,
    cq_doorbell: u16,
    deadline: u64,
) -> Result<Completion, BlockError> {
    loop {
        // 与 vCPU 并发（MTTCG）时控制器写 CQE 16B 可能被撕裂读：
        // 实证为"旧 byte12 + 新 byte13-15"混合（phase 已见新值而 cid
        // 仍是旧命令），导致假匹配。双读一致才作为有效快照。
        let mut b = [0u8; 16];
        let mut b2 = [0u8; 16];
        mem.read_bytes(cq_phys, *head as u64 * 16, &mut b);
        mem.read_bytes(cq_phys, *head as u64 * 16, &mut b2);
        if b != b2 {
            if now() > deadline {
                return Err(BlockError::Timeout);
            }
            continue;
        }
        let c = parse_completion(&b);
        if c.phase() == *phase {
            // CQE 读全先于 head 推进门铃（Acquire）。
            core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
            *head = (*head + 1) % QUEUE_ENTRIES;
            if *head == 0 {
                *phase = !*phase; // wrap 翻转 phase 口径。
            }
            bar.write32(cq_doorbell, *head as u32);
            return Ok(c);
        }
        if now() > deadline {
            return Err(BlockError::Timeout);
        }
    }
}

impl<B: BarAccess, M: DmaMem> BlockDevice for NvmeCtrl<B, M> {
    fn block_size(&self) -> u32 {
        self.block_size
    }
    fn capacity_blocks(&self) -> u64 {
        self.nblocks
    }
    fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
        let n = self.check_range(lba, dst.len())?;
        let bs = self.block_size as usize;
        // 分片：每片 ≤4KiB（单 PRP 语义），上层无感。
        let per = (PAGE as usize / bs).max(1);
        let mut done = 0usize;
        let mut left = n as usize;
        while left > 0 {
            let chunks = per.min(left);
            let len = chunks * bs;
            self.io_read_chunk(lba + (done / bs) as u64, chunks as u32, &mut dst[done..done + len])?;
            done += len;
            left -= chunks;
        }
        Ok(())
    }
    fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
        let n = self.check_range(lba, src.len())?;
        let bs = self.block_size as usize;
        let per = (PAGE as usize / bs).max(1);
        let mut done = 0usize;
        let mut left = n as usize;
        while left > 0 {
            let chunks = per.min(left);
            let len = chunks * bs;
            self.io_write_chunk(lba + (done / bs) as u64, chunks as u32, &src[done..done + len])?;
            done += len;
            left -= chunks;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), BlockError> {
        let cid = self.next_cid();
        self.io_submit_and_wait(Submission::io_flush(cid, self.nsid), cid)
    }
}

// ---------------------------------------------------------------------------
// 目标态：真 MMIO BAR + PMM DMA 桶（仅内核目标编译）。
// ---------------------------------------------------------------------------
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod target {
    use super::*;
    use crate::mem::pfh::PageTableOps as _;

    /// 目标态时钟。
    pub fn now_ns() -> u64 {
        crate::cpu::clock::now_ns()
    }

    /// MMIO BAR 窗口：PML4[510] 段 0xffffff1000000000 起（避开 ECAM
    /// 窗口 0xffffff0000000000）。每页惰性 map_mmio（PCD|PWT）。
    /// 多控制器：每实例占独立槽位段（BAR_SPAN），杜绝窗口复用——
    /// 旧实现单一 BAR_WINDOW，第二个控制器 map 时 translate 命中
    /// 已映射页而跳过重映射，寄存器访问实际打到第一个控制器。
    pub const BAR_WINDOW: u64 = 0xffff_ff10_0000_0000;
    /// 每实例槽位段长（64KiB=16 页，覆盖 16KiB BAR 且 4KiB 对齐）。
    const BAR_SPAN: u64 = 16 * PAGE;
    /// 槽位分配器（启动期单线程，Relaxed 足够）。
    static BAR_SLOT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

    pub struct BarMmio {
        base: u64,
        pages: u32,
    }

    impl BarMmio {
        /// 映射 BAR 前 `pages` 页（QEMU NVMe BAR 16KiB=4 页）。
        pub fn map(bar_phys: u64, pages: u32) -> Option<BarMmio> {
            let slot = BAR_SLOT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let base = BAR_WINDOW + (slot as u64) * BAR_SPAN;
            let mut ops = crate::mem::pfh::target_ops();
            for i in 0..pages {
                let win = base + (i as u64) * PAGE;
                let phys = bar_phys + (i as u64) * PAGE;
                if ops.translate(win).is_none() && !ops.map_mmio(win, phys) {
                    return None;
                }
            }
            Some(BarMmio { base, pages })
        }
        pub fn pages(&self) -> u32 {
            self.pages
        }
    }

    impl BarAccess for BarMmio {
        fn read32(&mut self, off: u16) -> u32 {
            let virt = self.base + off as u64;
            // SAFETY: 窗口已 map_mmio（PCD|PWT），4B 对齐 volatile 读。
            unsafe { core::ptr::read_volatile(virt as *const u32) }
        }
        fn write32(&mut self, off: u16, val: u32) {
            let virt = self.base + off as u64;
            // SAFETY: 同上，4B 对齐 volatile 写。
            unsafe { core::ptr::write_volatile(virt as *mut u32, val) }
        }
    }

    /// DMA 桶池：PMM 单帧 ×8，HHDM 访问。
    pub struct DmaBuckets {
        frames: [Option<u64>; 8],
        hhdm: u64,
    }

    impl DmaBuckets {
        pub fn new() -> DmaBuckets {
            DmaBuckets {
                frames: [None; 8],
                hhdm: crate::limine::hhdm_offset().unwrap_or(0),
            }
        }
    }

    impl DmaMem for DmaBuckets {
        fn alloc_frame(&mut self) -> Option<u64> {
            let f = crate::mem::pmm::alloc_page()?;
            self.frames.iter_mut().find(|s| s.is_none()).map(|s| *s = Some(f))?;
            Some(f)
        }
        fn free_frame(&mut self, phys: u64) {
            if let Some(slot) = self.frames.iter_mut().find(|s| **s == Some(phys)) {
                *slot = None;
                crate::mem::pmm::free_order(phys, 0);
            }
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            let virt = phys + off + self.hhdm;
            // SAFETY: phys 为 PMM 持有帧，off+len ≤4KiB。
            for (i, &b) in data.iter().enumerate() {
                unsafe { core::ptr::write_volatile((virt + i as u64) as *mut u8, b) };
            }
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            let virt = phys + off + self.hhdm;
            for (i, slot) in out.iter_mut().enumerate() {
                // SAFETY: 同 write_bytes。
                *slot = unsafe { core::ptr::read_volatile((virt + i as u64) as *const u8) };
            }
        }
    }

    /// 任务16 实机入口：ACPI→MCFG→ECAM 扫描→BAR 映射→初始化→回环 ×1000。
    /// 无 NVMe 控制器/无 MCFG 时优雅跳过（镜像在其他验收配置下照常工作）。
    pub fn probe_and_selftest() {
        let Some(rsdp) = crate::limine::rsdp_address() else {
            crate::kinfo!("blk: no RSDP - block stack skipped");
            return;
        };
        let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
        let Some(mcfg_phys) = super::super::pci::target::find_mcfg_phys(rsdp, hhdm) else {
            crate::kinfo!("blk: no MCFG table - block stack skipped");
            return;
        };
        let Some(seg) = super::super::pci::target::read_first_segment(mcfg_phys, hhdm) else {
            crate::kinfo!("blk: MCFG has no usable segment - skipped");
            return;
        };
        crate::kinfo!(
            "pci: ECAM base={:#x} bus {}..={}",
            seg.base,
            seg.start_bus,
            seg.end_bus
        );
        let mut ecam = super::super::pci::target::EcamMmio::new(seg);
        let hits = super::super::pci::scan_nvme_all(&mut ecam, &seg);
        if hits.is_empty() {
            crate::kinfo!("blk: no NVMe controller - block selftest skipped (graceful)");
            return;
        }
        let hit = hits[0];
        let shared_hit = hits.get(1).copied();
        crate::kinfo!(
            "pci: NVMe controller at {:#x}:{:#x}.{} bar0={:#x} ecam_pages={}",
            hit.bus,
            hit.dev,
            hit.func,
            hit.bar0,
            super::super::pci::target::ECAM_PAGES_MAPPED.load(core::sync::atomic::Ordering::Relaxed)
        );
        let Some(bar) = BarMmio::map(hit.bar0, 4) else {
            crate::kwarn!("blk: BAR0 map failed - skipped");
            return;
        };
        let buckets = DmaBuckets::new();
        match NvmeCtrl::init_with_recovery(bar, buckets, now_ns, 3_000_000_000) {
            Ok(mut ctrl) => {
                let (bs, n) = ctrl.geometry();
                crate::kinfo!(
                    "nvme: init ok resets={} block_size={} blocks={} ({} MiB)",
                    ctrl.resets,
                    bs,
                    n,
                    n * bs as u64 / (1 << 20)
                );
                let mut buf = [0u8; 4096];
                let rep = super::super::blk::loopback_probe(&mut ctrl, 1000, 8, &mut buf);
                crate::kinfo!(
                    "nvme: loopback x1000 passed={} rounds={} write_sum={:#018x} read_sum={:#018x} err={:?}",
                    rep.passed,
                    rep.rounds,
                    rep.write_sum,
                    rep.read_sum,
                    rep.err
                );
                if !rep.passed {
                    crate::kwarn!("nvme: loopback FAILED - see err above");
                }

                // 任务17：fs23_journal 块设备后端——掉电注入探针（跨进程
                // 持久，外部脚本 kill QEMU 模拟掉电，×11 轮盘面条目单调
                // 增长零撕裂）。
                crate::fs::fs23_disk::target::fs23_powercut_probe(&mut ctrl);

                // 任务21：里程碑 M2——journal 真盘双会话（fresh 封条 /
                // powercut 恢复），盘面高区 LBA 40000。
                crate::milestone::target::ms_journal_probe(&mut ctrl);

                // 任务24：内核 KV 存储服务——跨断电 boot-counter 累加 +
                // 逐会话 10 键写入/恢复核对（盘1 高区 LBA 60000/70000）。
                crate::kvsrv::target::kv_probe(&mut ctrl);
            }
            Err(e) => crate::kwarn!("nvme: init failed {:?} - selftest skipped", e),
        }

        // 任务18：第二 NVMe 控制器 → SHARED exFAT 只读挂载 + 快照区
        // 写过渡探针（无第二控制器时 graceful 跳过）。
        if let Some(sh) = shared_hit {
            crate::kinfo!(
                "pci: NVMe controller #2 at {:#x}:{:#x}.{} bar0={:#x} (shared)",
                sh.bus,
                sh.dev,
                sh.func,
                sh.bar0
            );
            if let Some(sbar) = BarMmio::map(sh.bar0, 4) {
                let sbuckets = DmaBuckets::new();
                match NvmeCtrl::init_with_recovery(sbar, sbuckets, now_ns, 3_000_000_000) {
                    Ok(mut sctrl) => {
                        crate::fs::exfat_ro::target::shared_probe(&mut sctrl);

                        // 任务21：里程碑 M3/M4——SHARED exFAT 读 +
                        // 快照区跨断电持久核对。
                        crate::milestone::target::ms_shared_probe(&mut sctrl);
                    }
                    Err(e) => crate::kwarn!("shared: nvme init failed {:?} - skipped", e),
                }
            } else {
                crate::kwarn!("shared: BAR0 map failed - skipped");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 宿主寄存器模拟器（tests only）：与真控制器同构的行为模型。
// doorbell 写入即同步应答（真硬件是异步完成，行为学等价：命令入队后
// 经 pump 执行并写 CQE）。enable 故障注入驱动恢复路径用例。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::vec::Vec;

    // 可控时钟：每次调用自增（poll 轮询自然推进到超时）。
    static NOW: AtomicU64 = AtomicU64::new(0);
    fn fake_now() -> u64 {
        NOW.fetch_add(1, Ordering::Relaxed) + 1
    }
    fn reset_clock() {
        NOW.store(0, Ordering::Relaxed);
    }

    /// 宿主"物理内存"：8 帧池。
    struct HostMem {
        data: Vec<u8>,
        next: usize,
    }
    impl HostMem {
        fn new() -> HostMem {
            HostMem {
                // 帧号从 1 起（地址 0x1000..0x9000），9 页空间才覆盖第 8 帧。
                data: vec![0u8; 9 * PAGE as usize],
                next: 0,
            }
        }
        fn read(&self, phys: u64, off: u64, out: &mut [u8]) {
            let s = (phys + off) as usize;
            out.copy_from_slice(&self.data[s..s + out.len()]);
        }
        fn write(&mut self, phys: u64, off: u64, data: &[u8]) {
            let s = (phys + off) as usize;
            self.data[s..s + data.len()].copy_from_slice(data);
        }
    }
    impl DmaMem for HostMem {
        fn alloc_frame(&mut self) -> Option<u64> {
            if self.next >= 8 {
                return None;
            }
            // 帧地址永不为 0（与真机同构：物理地址 0 不是合法帧，
            // strict enable 的 asq!=0 校验才能如实工作）。
            let f = ((self.next + 1) * PAGE as usize) as u64;
            self.next += 1;
            Some(f)
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            self.write(phys, off, data);
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            self.read(phys, off, out);
        }
    }

    /// 控制器模拟器：寄存器状态 + SQ/CQ 消费语义 + 故障注入。
    struct RegModel {
        r: HashMap<u16, u32>,
        asq: u64,
        acq: u64,
        admin_sq_tail: usize,
        admin_cq_head: usize,
        admin_phase: bool,
        io_sq_phys: u64,
        io_cq_phys: u64,
        io_sq_tail: usize,
        io_cq_head: usize,
        io_phase: bool,
        blocks: Vec<u8>, // 1MiB 虚拟盘
        ns_blocks: u64,
        block_size: u32,
        /// 前 N 次 enable 拒绝置 RDY（超时/恢复路径注入）。
        enable_failures: u32,
        strict_enable: bool,
    }
    impl RegModel {
        fn new() -> RegModel {
            RegModel {
                r: HashMap::new(),
                asq: 0,
                acq: 0,
                admin_sq_tail: 0,
                admin_cq_head: 0,
                admin_phase: true,
                io_sq_phys: 0,
                io_cq_phys: 0,
                io_sq_tail: 0,
                io_cq_head: 0,
                io_phase: true,
                blocks: vec![0u8; 1 << 20],
                ns_blocks: 2048,
                block_size: 512,
                enable_failures: 0,
                strict_enable: true,
            }
        }
        fn write_cqe(
            &self,
            mem: &mut HostMem,
            cq_phys: u64,
            idx: usize,
            cid: u16,
            status: u16,
            phase: bool,
        ) {
            let mut c = [0u8; 16];
            // DW3[15:0]=CID，DW3[31:16]=(status<<1)|phase（P=bit0）。
            let dw3 = (cid as u32 & 0xFFFF)
                | ((((status as u32) << 1) | (phase as u32)) << 16);
            c[12..16].copy_from_slice(&dw3.to_le_bytes());
            mem.write(cq_phys, (idx * 16) as u64, &c);
        }
        fn db(&self, off: u16) -> usize {
            *self.r.get(&off).unwrap_or(&0) as usize
        }
        /// 消费 admin SQ 至 doorbell tail（identify / create cq/sq）。
        fn admin_pump(&mut self, mem: &mut HostMem, upto: usize) {
            while self.admin_sq_tail != upto {
                let mut s = [0u8; 64];
                mem.read(self.asq, (self.admin_sq_tail * 64) as u64, &mut s);
                let op = s[0];
                let cid = (u32::from_le_bytes(s[0..4].try_into().unwrap()) >> 16) as u16;
                let dw10 = u32::from_le_bytes(s[40..44].try_into().unwrap());
                // PRP1 @ offset 24..32（DW6/DW7），QEMU NvmeCmd 实证。
                let prp1 = u64::from_le_bytes(s[24..32].try_into().unwrap());
                match op {
                    0x06 => {
                        let cns = dw10 & 0xFF;
                        let mut page = vec![0u8; PAGE as usize];
                        if cns == 1 {
                            // Identify Controller：NN @ byte516（0x204）。
                            page[0x204..0x208].copy_from_slice(&1u32.to_le_bytes()); // NN=1
                        } else if cns == 0 {
                            // Identify Namespace（需有效 NSID）：NSZE/FLBAS/LBAF0。
                            page[0..8].copy_from_slice(&self.ns_blocks.to_le_bytes());
                            page[26] = 0; // FLBAS=0
                            page[0x82] = 9; // LBAF0.LBADS=9 → 512B
                        } else {
                            // 最小栈不支持的 CNS：模拟器拒为 invalid field。
                            self.write_cqe(mem, self.acq, self.admin_cq_head, cid, 0x2, self.admin_phase);
                            self.admin_cq_head = (self.admin_cq_head + 1) % QUEUE_ENTRIES;
                            if self.admin_cq_head == 0 {
                                self.admin_phase = !self.admin_phase;
                            }
                            self.admin_sq_tail = (self.admin_sq_tail + 1) % QUEUE_ENTRIES;
                            continue;
                        }
                        mem.write(prp1, 0, &page);
                    }
                    0x05 => self.io_cq_phys = prp1,
                    0x01 => self.io_sq_phys = prp1,
                    _ => {}
                }
                self.write_cqe(mem, self.acq, self.admin_cq_head, cid, 0, self.admin_phase);
                self.admin_cq_head = (self.admin_cq_head + 1) % QUEUE_ENTRIES;
                if self.admin_cq_head == 0 {
                    self.admin_phase = !self.admin_phase;
                }
                self.admin_sq_tail = (self.admin_sq_tail + 1) % QUEUE_ENTRIES;
            }
        }
        /// 消费 io SQ（read / write / flush）。
        fn io_pump(&mut self, mem: &mut HostMem, upto: usize) {
            let bs = self.block_size as usize;
            while self.io_sq_tail != upto {
                let mut s = [0u8; 64];
                mem.read(self.io_sq_phys, (self.io_sq_tail * 64) as u64, &mut s);
                let op = s[0];
                let cid = (u32::from_le_bytes(s[0..4].try_into().unwrap()) >> 16) as u16;
                let prp1 = u64::from_le_bytes(s[24..32].try_into().unwrap());
                let lba = u64::from_le_bytes(s[40..48].try_into().unwrap());
                let nlb = (u32::from_le_bytes(s[48..52].try_into().unwrap()) & 0xFFFF) as usize + 1;
                let span = nlb * bs;
                let dst = (lba as usize) * bs;
                match op {
                    0x02 => {
                        let mut buf = vec![0u8; span];
                        buf.copy_from_slice(&self.blocks[dst..dst + span]);
                        mem.write(prp1, 0, &buf);
                    }
                    0x01 => {
                        let mut buf = vec![0u8; span];
                        mem.read(prp1, 0, &mut buf);
                        self.blocks[dst..dst + span].copy_from_slice(&buf);
                    }
                    _ => {}
                }
                self.write_cqe(mem, self.io_cq_phys, self.io_cq_head, cid, 0, self.io_phase);
                self.io_cq_head = (self.io_cq_head + 1) % QUEUE_ENTRIES;
                if self.io_cq_head == 0 {
                    self.io_phase = !self.io_phase;
                }
                self.io_sq_tail = (self.io_sq_tail + 1) % QUEUE_ENTRIES;
            }
        }
        fn read32(&mut self, off: u16) -> u32 {
            match off {
                REG_CAP => 0x3F,       // MQES=63
                REG_CAP_HI => 0, // DSTRD=0
                REG_VS => 0x0001_0400, // NVMe 1.4
                REG_CST => *self.r.get(&REG_CST).unwrap_or(&0),
                _ => *self.r.get(&off).unwrap_or(&0),
            }
        }
        fn write32(&mut self, off: u16, val: u32) {
            match off {
                REG_CC => {
                    self.r.insert(REG_CC, val);
                    if val & 1 != 0 {
                        let ok = if self.strict_enable {
                            ((val >> 16) & 0xF == 6)
                                && ((val >> 20) & 0xF == 4)
                                && self.asq != 0
                                && self.acq != 0
                                && self.enable_failures == 0
                        } else {
                            true
                        };
                        if self.enable_failures > 0 {
                            self.enable_failures -= 1;
                        }
                        self.r.insert(REG_CST, if ok { 1 } else { 0 });
                    } else {
                        self.r.insert(REG_CST, 0);
                    }
                }
                REG_ASQ => self.asq = (self.asq & 0xFFFF_FFFF_0000_0000) | val as u64,
                REG_ASQ_HI => self.asq = (self.asq & 0xFFFF_FFFF) | ((val as u64) << 32),
                REG_ACQ => self.acq = (self.acq & 0xFFFF_FFFF_0000_0000) | val as u64,
                REG_ACQ_HI => self.acq = (self.acq & 0xFFFF_FFFF) | ((val as u64) << 32),
                _ => {
                    self.r.insert(off, val);
                }
            }
        }
    }

    /// 共享句柄：NvmeCtrl 持有其一，测试持另一份做 pump。
    #[derive(Clone)]
    struct SharedDev {
        rm: Rc<RefCell<RegModel>>,
        mem: Rc<RefCell<HostMem>>,
    }
    impl SharedDev {
        fn new() -> SharedDev {
            SharedDev {
                rm: Rc::new(RefCell::new(RegModel::new())),
                mem: Rc::new(RefCell::new(HostMem::new())),
            }
        }
        fn rm(&self) -> std::cell::RefMut<'_, RegModel> {
            self.rm.borrow_mut()
        }
    }
    impl BarAccess for SharedDev {
        fn read32(&mut self, off: u16) -> u32 {
            self.rm.borrow_mut().read32(off)
        }
        fn write32(&mut self, off: u16, val: u32) {
            self.rm.borrow_mut().write32(off, val);
            // doorbell 写 = 控制器开始消费（同步应答）。
            match off {
                DB_ADMIN_SQ => {
                    let tail = self.rm.borrow().db(off);
                    let mut rm = self.rm.borrow_mut();
                    let mut mem = self.mem.borrow_mut();
                    rm.admin_pump(&mut mem, tail);
                }
                0x1008 => {
                    // IO SQ 门铃：stride=4, qid=1, kind=0。
                    let tail = self.rm.borrow().db(off);
                    let mut rm = self.rm.borrow_mut();
                    let mut mem = self.mem.borrow_mut();
                    rm.io_pump(&mut mem, tail);
                }
                _ => {}
            }
        }
    }
    impl DmaMem for SharedDev {
        fn alloc_frame(&mut self) -> Option<u64> {
            self.mem.borrow_mut().alloc_frame()
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            self.mem.borrow_mut().write(phys, off, data);
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            self.mem.borrow().read(phys, off, out);
        }
    }

    #[test]
    fn nvme_submission_field_packing() {
        let s = Submission::io_read(0x1234, 1, 0x1_0000_0002, 8, 0xABCD_E000);
        assert_eq!(s.0[0], 0x02 | (0x1234 << 16));
        assert_eq!(s.0[1], 1);
        assert_eq!(s.0[6], 0xABCD_E000, "PRP1 @ DW6/7（QEMU NvmeCmd 实证）");
        assert_eq!(s.0[7], 0, "PRP1 高 32 位（0xABCD_E000 < 2^32）");
        assert_eq!(s.0[10], 2, "SLBA low");
        assert_eq!(s.0[11], 1, "SLBA high");
        assert_eq!(s.0[12], 7, "NLB-1");
        let c = Submission::admin_create_cq(9, 1, 0x2000, 64);
        assert_eq!(c.0[0], 0x05 | (9 << 16));
        assert_eq!(c.0[10], 1 | (63 << 16), "cqid | qsize<<16");
        assert_eq!(c.0[11], 0x3, "flags=IEN|PC，irq=0");
        let q = Submission::admin_create_sq(3, 1, 0x3000, 64, 1);
        assert_eq!(q.0[10], 1 | (63 << 16), "sqid | qsize<<16");
        assert_eq!(q.0[11], (1 << 16) | 0x1, "cqid<<16 | PC");
        let f = Submission::io_flush(7, 1);
        assert_eq!(f.0[0], 0x00 | (7 << 16), "Flush=opcode 0x00");
        assert_eq!(f.0[1], 1);
    }

    #[test]
    fn nvme_completion_parse_fields() {
        let mut b = [0u8; 16];
        // QEMU 实测布局：cid=0x42，status16=(0<<1)|1 → DW3=0x0001_0042。
        let dw3 = 0x42u32 | (0x0001u32 << 16);
        b[12..16].copy_from_slice(&dw3.to_le_bytes());
        let c = parse_completion(&b);
        assert_eq!(c.cid(), 0x42);
        assert_eq!(c.status(), 0);
        assert!(c.phase(), "P 在 DW3 bit16");
        // 非零错误码 + phase 0：cid=0x10，status16=0x0102（错误码 0x81）。
        let dw3 = 0x10u32 | (0x0102u32 << 16);
        b[12..16].copy_from_slice(&dw3.to_le_bytes());
        let c = parse_completion(&b);
        assert_eq!(c.cid(), 0x10);
        assert_eq!(c.status(), 0x81);
        assert!(!c.phase());
        // QEMU 实测样本：identify 成功回执 DW3=0x0001_0000（cid=0，phase=1）。
        b[12..16].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        let c = parse_completion(&b);
        assert_eq!(c.cid(), 0);
        assert_eq!(c.status(), 0);
        assert!(c.phase());
    }

    #[test]
    fn nvme_queue_frames_must_be_zeroed_on_create() {
        // 回归：复用帧残留数据的 phase 位曾与队初始 phase 假匹配，poll
        // 在 QEMU BH 异步处理真完成之前假消费（实机 r=2 read 失败根因）。
        // 契约：建队帧（io cq/sq/buf）必须整帧清零，投毒全池后 init 与
        // 几何解析仍须成立。
        reset_clock();
        let dev = SharedDev::new();
        for b in dev.mem.borrow_mut().data.iter_mut() {
            *b = 0xCD;
        }
        let ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 100_000)
            .expect("投毒池上初始化仍必须成功");
        assert_eq!(ctrl.geometry(), (512, 2048));
        // alloc 顺序：admin sq(帧1)/cq(帧2) → id_page(帧3，identify 后归还)
        // → io cq(帧4)/sq(帧5)/buf(帧6)。io 三帧建后未发任何 IO 命令，全零。
        let mem = dev.mem.borrow();
        for f in 4..=6usize {
            let base = f * PAGE as usize;
            assert!(
                mem.data[base..base + PAGE as usize].iter().all(|&b| b == 0),
                "io 队列帧 {} 建队后必须清零",
                f
            );
        }
    }

    #[test]
    fn nvme_full_sequence_read_write_roundtrip() {
        reset_clock();
        let dev = SharedDev::new();
        let mut ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 100_000)
            .expect("完整初始化必须成功");
        assert_eq!(ctrl.resets, 0, "无故障注入不应触发恢复");
        assert_eq!(ctrl.geometry(), (512, 2048), "identify 解析：512B × 2048 块");
        // 写 2 块（1024B）→ 读回逐字节一致。
        let mut w = [0u8; 1024];
        for (i, b) in w.iter_mut().enumerate() {
            *b = (i * 7 + 3) as u8;
        }
        ctrl.write_blocks(100, &w).expect("write 必须成功");
        let mut r = [0xA5u8; 1024];
        ctrl.read_blocks(100, &mut r).expect("read 必须成功");
        assert_eq!(w, r, "读写回环逐字节一致");
        // 越界与非整块如实拒绝。
        assert_eq!(ctrl.read_blocks(2048, &mut r), Err(BlockError::InvalidRange));
        assert_eq!(ctrl.read_blocks(0, &mut r[..100]), Err(BlockError::InvalidRange));
        // flush 成功。
        ctrl.flush().expect("flush 必须成功");
    }

    #[test]
    fn nvme_timeout_recovery_path_retries_once() {
        reset_clock();
        let dev = SharedDev::new();
        // 第一次 enable 拒绝置 RDY（超时）→ 恢复重试成功。
        dev.rm().enable_failures = 1;
        let ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 1_000)
            .expect("恢复路径后必须初始化成功");
        assert_eq!(ctrl.resets, 1, "恢复路径必须留下重试证据");
        assert_eq!(ctrl.geometry(), (512, 2048));
    }

    #[test]
    fn nvme_timeout_exhausted_reports_device_reset() {
        reset_clock();
        let dev = SharedDev::new();
        dev.rm().enable_failures = 99; // 两次 enable 都失败。
        let e = match NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 1_000) {
            Ok(_) => panic!("持续失败必须报错"),
            Err(e) => e,
        };
        assert_eq!(e, BlockError::DeviceReset, "重试耗尽=DeviceReset 口径");
    }

    #[test]
    fn nvme_enable_param_strict_validation() {
        reset_clock();
        let dev = SharedDev::new();
        dev.rm().strict_enable = false; // 宽松模式（校验由注入替代）。
        let ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 100_000).unwrap();
        let cc = dev.rm().r.get(&REG_CC).copied().unwrap_or(0);
        assert_eq!(cc & 1, 1, "EN 置位");
        assert_eq!((cc >> 16) & 0xF, 6, "IOSQES=6（64B SQE）");
        assert_eq!((cc >> 20) & 0xF, 4, "IOCQES=4（16B CQE）");
        assert_eq!(ctrl.geometry(), (512, 2048));
    }

    #[test]
    fn nvme_io_cqe_phase_wraps_at_queue_end() {
        reset_clock();
        let dev = SharedDev::new();
        let mut ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 100_000).unwrap();
        // io 队列 64 条：65 次单块读驱动 CQE head wrap（第 65 条翻转
        // phase 口径），wrap 后读写仍正确——队列滚动语义完整验证。
        let mut buf = [0u8; 512];
        for i in 0..65u64 {
            ctrl.read_blocks(i, &mut buf).expect("单块读必须成功");
        }
        let w = [0xEEu8; 512];
        ctrl.write_blocks(999, &w).unwrap();
        let mut r = [0u8; 512];
        ctrl.read_blocks(999, &mut r).unwrap();
        assert_eq!(w, r);
    }
}
