//! VE-F0223 · Intel 命令流编码器（VE-B 域 · Intel 核显组 · 目标 520 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0223`
//!
//! **判据（锚点原文五条）**：MI 族与 3D 族编码结构等价对拍、代际分派正确、
//! 对齐与 patching 正确、吞吐达标、错误码全映射。
//!
//! Intel GPU 吃自有命令流：batch buffer 里是 MI 系统命令与 3D 管线状态命令
//! 的混合序列。编码器是 A 域命令抽象（F0004 `Command{opcode,arg0,arg1}`）
//! 与硬件编码字节之间的**翻译后端**——上游语义命令从这进来，硬件 dwords
//! 从这出去。
//!
//! # 地址空间契约（全模块统一口径）
//!
//! 本编码器地址空间 **32 位**——与 F0222 的 aperture 模型（256 MiB 窗口、
//! PTE 全表 4K 粒度）一致：能被 aperture 映射的地址必在 4 GiB 内。
//! 64 位入口值超界即 [`CErr::AddrBeyond4G`] 专属拒绝，**不静默截断**
//! （截断 = 把高位抹掉继续用，设备会翻译到错误页，表现为越权读写）。
//!
//! # 头注要点（每条都是判据的反面，写在这里供判据引用）
//!
//! ## 要点一：编码结构等价对拍是**判据侧独立重写**，不问被测要答案
//!
//! 「与开源参考实现编码同场景对比结构等价」在本仓的兑现口径：判据层用
//! **独立写的参照编码器**重编同一条语义命令，与被测输出**逐 dword 对拍**
//! （长度、头字段位置、载荷语义位全对齐）。被测把字段位挪一格、把长度
//! 算错一个 dword，对拍立即分叉——判据不引用被测的任何编码函数。
//!
//! ## 要点二：代际分派是**显式映射**，不是默认值
//!
//! 每代编码有真实差异（本模块契约口径：Xe 起的 3D 命令头带扩展参数位
//! 与额外的 ext dword；视口上限 Gen9 为 16、Xe 起为 32；MI_BATCH_BUFFER_
//! START 的 Scope 位 Xe2 起才有；信号量内存轮询 Xe 起才有）。分派只经
//! [`GenTier`]→[`GenVariant`] 唯一映射，**没有任何「默认当代」回退**——
//! 拿错代际的编码表，输出的命令序列在硬件上就是另一条命令。
//!
//! ## 要点三：8 字节对齐与 patching 是**结构纪律**，不是运行时检查
//!
//! batch 内 8 字节对齐：batch 长度为奇数 dword 时由 `pad_to_8` 显式补
//! MI_NOOP——补齐是**发射路径的一部分**，不是事后修补。跳转贴片
//! （patching）：地址未知的跳转目标在发射时收集为**重定位项**
//! （`Reloc{at_dword,target}`，单 dword 精确记录），finalize 时统一回填；
//! 回填写不到收集时的位置（PatchOutOfRange）或目标地址解析不出
//! （RelocUnresolved）都是**专属错误码**，无静默。
//!
//! ## 要点四：吞吐达标用**确定性操作计数**，不用墙钟
//!
//! 裸机 rdtsc 未校准且跨核不同步（F0221 同款纪律）。吞吐口径 = 编码产出
//! 的 dword 数（确定性的工作量度量）：一次 `encode_all` 会话产出 ≤
//! `OP_BUDGET_BASE + N*OP_BUDGET_PER_CMD` 个 dword（N=命令数），且空会话
//! 产出为 0（非恒真门禁的基线）。预算超了就是超了，如实报
//! [`CErr::EncBudgetExceeded`]——该口径可抓「多吐垃圾 dword」类变异。
//!
//! ## 要点五：错误码全映射在**新域独占码段 0x32xx**
//!
//! 与 F0222 的 0x2Fxx 段互不重叠；下游 F0229 错误解析按位消费。
//! 每个码都有专属 reason 串（拒绝必带专属原因，不共用占位串）。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vea04_ring::Command as RingCommand;
use super::veb21_ident::GenTier;

// ---------------------------------------------------------------------------
// 常量与命令头契约
// ---------------------------------------------------------------------------

/// 8 字节对齐（batch 起址与 BB_START 目标的对齐纪律，要点三）。
pub const ALIGN_8: u64 = 8;

/// 地址空间上限（要点「地址空间契约」：32 位）。
pub const ADDR_MAX: u64 = 0xFFFF_FFFF;

/// MI_BATCH_BUFFER_START 的单 batch 上限（dword 数）。
pub const MAX_BATCH_DWORDS: usize = 4096;

/// 重定位项上限（超出即 [`CErr::TooManyRelocs`]——如实拒绝不吞）。
pub const MAX_RELOCS: usize = 64;

/// 吞吐预算（要点四）：会话基数（dword 数）。
pub const OP_BUDGET_BASE: u64 = 16;
/// 吞吐预算（要点四）：每条命令允许的 dword 数。
pub const OP_BUDGET_PER_CMD: u64 = 8;

/// MI 系命令头（DW0[31:29]=0 类型 + [26:23] 操作码 + [8:0] 长度）。
pub const MI_TYPE: u32 = 0x0;
/// 3D 系命令头（DW0[31:29]=3）。
pub const PIPELINE_3D: u32 = 0x3;
/// 3D 系的子类型（DW0[28:27]=3 → 3DSTATE）。
pub const SUBTYPE_3DSTATE: u32 = 0x3;

/// MI_BATCH_BUFFER_START 操作码（0x31 << 23）。
pub const MI_OP_BB_START: u32 = 0x31 << 23;
/// MI_SEMAPHORE_WAIT 操作码（0x1C << 23）。
pub const MI_OP_SEM_WAIT: u32 = 0x1C << 23;
/// MI_STORE_DATA_IMM 操作码（0x20 << 23）。
pub const MI_OP_STORE_DATA: u32 = 0x20 << 23;
/// MI_NOOP 操作码（0x00）。
pub const MI_OP_NOOP: u32 = 0x00;

/// 3DSTATE 子操作码（DW0[23:16]）：VF（顶点取指）。
pub const SUBOP_VF: u32 = 0x78;
/// 3DSTATE 子操作码：视口。
pub const SUBOP_VIEWPORT: u32 = 0x74;
/// 3DSTATE 子操作码：混合。
pub const SUBOP_BLEND: u32 = 0x4D;
/// 3DSTATE 子操作码：深度模板。
pub const SUBOP_DEPTH: u32 = 0x4E;
/// 3DSTATE 子操作码：渲染目标集合。
pub const SUBOP_RT_SET: u32 = 0x4B;

// ---------------------------------------------------------------------------
// 代际分派（要点二：显式映射，无默认回退）
// ---------------------------------------------------------------------------

/// 编码器代际变体（本模块契约口径；差异点全部显式编码，判据对拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenVariant {
    /// Gen9 基线（Gen9/9.5/11 共用基线档）。
    Gen9,
    /// Xe 标准。
    Xe,
    /// Xe 最新。
    Xe2,
}

impl GenVariant {
    /// [`GenTier`]→变体唯一映射（分派正确性判据的对象）。
    pub const fn for_tier(t: GenTier) -> GenVariant {
        match t {
            GenTier::Baseline => GenVariant::Gen9,
            GenTier::XeStandard => GenVariant::Xe,
            GenTier::XeLatest => GenVariant::Xe2,
        }
    }
    /// 视口数量上限（Gen9=16，Xe 起=32）。
    pub const fn max_viewports(self) -> u8 {
        match self {
            GenVariant::Gen9 => 16,
            GenVariant::Xe | GenVariant::Xe2 => 32,
        }
    }
    /// 3DSTATE 头是否带扩展参数位（Xe 起）。
    pub const fn has_3d_ext(self) -> bool {
        matches!(self, GenVariant::Xe | GenVariant::Xe2)
    }
    /// MI_BATCH_BUFFER_START 是否支持 Scope 位（Xe2 起）。
    pub const fn has_bb_scope(self) -> bool {
        matches!(self, GenVariant::Xe2)
    }
    /// MI_SEMAPHORE_WAIT 是否支持内存轮询模式（Xe 起）。
    pub const fn has_sem_mem_poll(self) -> bool {
        matches!(self, GenVariant::Xe | GenVariant::Xe2)
    }
    pub const ALL: [GenVariant; 3] = [GenVariant::Gen9, GenVariant::Xe, GenVariant::Xe2];
}

// ---------------------------------------------------------------------------
// 诊断码（新域独占码段 0x32xx；下游 F0229 按位消费）
// ---------------------------------------------------------------------------

/// 编码器域错误（**自建诊断码**，不扩下游封闭枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CErr {
    /// batch 起址非 8 字节对齐。
    BadBatchAlign,
    /// batch dword 数超上限。
    BatchTooLarge,
    /// 地址超 32 位地址空间契约。
    AddrBeyond4G,
    /// 本代际不支持该命令。
    UnsupportedForGen,
    /// 视口数量越代际上限。
    BadViewportCount,
    /// 渲染目标数量非法（1..=8）。
    BadRtCount,
    /// 信号量地址非 8 字节对齐。
    BadSemaphoreAlign,
    /// 数据写入地址非 8 字节对齐。
    BadStoreAlign,
    /// 重定位目标解析不出（finalize 时）。
    RelocUnresolved,
    /// 回填写不到收集时的位置。
    PatchOutOfRange,
    /// 重定位项超上限。
    TooManyRelocs,
    /// 索引元素尺寸非法（0/1/2 之外）。
    BadIndexElementSize,
    /// 编码会话超吞吐预算（要点四）。
    EncBudgetExceeded,
}

impl CErr {
    pub const fn code(self) -> u32 {
        match self {
            CErr::BadBatchAlign => 0x3201,
            CErr::BatchTooLarge => 0x3202,
            CErr::AddrBeyond4G => 0x3203,
            CErr::UnsupportedForGen => 0x3204,
            CErr::BadViewportCount => 0x3205,
            CErr::BadRtCount => 0x3206,
            CErr::BadSemaphoreAlign => 0x3207,
            CErr::BadStoreAlign => 0x3208,
            CErr::RelocUnresolved => 0x3209,
            CErr::PatchOutOfRange => 0x320A,
            CErr::TooManyRelocs => 0x320B,
            CErr::BadIndexElementSize => 0x320C,
            CErr::EncBudgetExceeded => 0x320D,
        }
    }
    /// 人类可读说明（专属 reason，不共用占位串）。
    pub fn reason(self) -> String {
        match self {
            CErr::BadBatchAlign => String::from("batch 起址非 8 字节对齐"),
            CErr::BatchTooLarge => String::from("batch dword 数超上限"),
            CErr::AddrBeyond4G => String::from("地址超 32 位地址空间契约"),
            CErr::UnsupportedForGen => String::from("当前代际不支持该命令"),
            CErr::BadViewportCount => String::from("视口数量越代际上限"),
            CErr::BadRtCount => String::from("渲染目标数量非法"),
            CErr::BadSemaphoreAlign => String::from("信号量地址非 8 字节对齐"),
            CErr::BadStoreAlign => String::from("数据写入地址非 8 字节对齐"),
            CErr::RelocUnresolved => String::from("重定位目标地址解析不出"),
            CErr::PatchOutOfRange => String::from("回填位置超出收集时的记录"),
            CErr::TooManyRelocs => String::from("重定位项超上限"),
            CErr::BadIndexElementSize => String::from("索引元素尺寸非法"),
            CErr::EncBudgetExceeded => String::from("编码会话超吞吐预算"),
        }
    }
    /// 全集（判据遍历互异性与码段归属）。
    pub const ALL: [CErr; 13] = [
        CErr::BadBatchAlign,
        CErr::BatchTooLarge,
        CErr::AddrBeyond4G,
        CErr::UnsupportedForGen,
        CErr::BadViewportCount,
        CErr::BadRtCount,
        CErr::BadSemaphoreAlign,
        CErr::BadStoreAlign,
        CErr::RelocUnresolved,
        CErr::PatchOutOfRange,
        CErr::TooManyRelocs,
        CErr::BadIndexElementSize,
        CErr::EncBudgetExceeded,
    ];
}

// ---------------------------------------------------------------------------
// 语义命令 IR（上游抽象的 Intel 后端承载形态）
// ---------------------------------------------------------------------------

/// 地址空间指示（MI_BATCH_BUFFER_START 的 ASI 位语义，与 F0222 表种对接）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddrSpace {
    /// GGTT（全局）——ASI=0。
    Ggtt,
    /// ppGTT（每进程）——ASI=1。
    PpGtt,
}

impl AddrSpace {
    /// ASI 位显式编码（禁 enum as u8 直转）。
    pub const fn asi_bit(self) -> u32 {
        match self {
            AddrSpace::Ggtt => 0,
            AddrSpace::PpGtt => 1,
        }
    }
}

/// MI_SEMAPHORE_WAIT 的比较操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemCompare {
    /// 等于。
    Equal,
    /// 大于等于。
    GreaterEqual,
}

impl SemCompare {
    pub const fn bits(self) -> u32 {
        match self {
            SemCompare::Equal => 0,
            SemCompare::GreaterEqual => 1,
        }
    }
}

/// 重定位目标（符号地址；finalize 时由解析器回填）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RelocTarget {
    /// 符号 id（上游符号表键）。
    pub sym: u64,
}

/// 一条重定位项（要点三：收集于发射时，回填于 finalize）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reloc {
    /// batch 内的 dword 下标（符号地址回填到这一个 dword）。
    pub at_dword: usize,
    pub target: RelocTarget,
}

/// 语义命令（编码器的输入 IR）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemCmd {
    /// 二级 batch 启动（地址可为符号；space 与 F0222 表种对接）。
    MiBatchBufferStart {
        addr: Option<u64>,
        sym: Option<u64>,
        space: AddrSpace,
    },
    /// 信号量等待（轮询模式）。
    MiSemaphoreWait {
        addr: u64,
        value: u32,
        cmp: SemCompare,
        /// 内存轮询（Xe 起；Gen9 传 true 为 [`CErr::UnsupportedForGen`]）。
        mem_poll: bool,
    },
    /// 数据立即数写入。
    MiStoreData { addr: u64, value: u32 },
    /// 顶点取指设置。
    VfSetup {
        indexed: bool,
        /// 索引元素尺寸档（0=8bit 1=16bit 2=32bit）。
        index_elem_size: u8,
    },
    /// 视口设置。
    Viewport { count: u8 },
    /// 混合状态。
    Blend {
        write_enable: bool,
        logic_op: bool,
        color_mask: u8,
    },
    /// 深度模板状态。
    DepthStencil { depth_test: bool, depth_write: bool },
    /// 渲染目标集合。
    RenderTargets {
        /// 1..=8。
        rt_count: u8,
        /// 每目标 surface 格式码（判据侧字面量对拍）。
        fmt: u32,
    },
    /// 显式空操作（对齐补齐也用同一条编码）。
    MiNoop,
}

// ---------------------------------------------------------------------------
// 编码器
// ---------------------------------------------------------------------------

/// 编码账本（判据侧独立重算对拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EncStats {
    /// 成功发射的语义命令数。
    pub commands: u32,
    /// 发射的 dword 总数（含 NOOP 补齐）。
    pub dwords: u64,
    /// NOOP 补齐数。
    pub noop_pads: u32,
    /// 收集的重定位项数。
    pub relocs: u32,
    /// 回填成功的重定位项数。
    pub patches: u32,
    /// 最近一次会话的产出 dword 数（吞吐口径，要点四）。
    pub last_op_count: u64,
    /// 超预算拒绝次数。
    pub budget_rejects: u32,
}

impl EncStats {
    pub const fn zero() -> EncStats {
        EncStats {
            commands: 0,
            dwords: 0,
            noop_pads: 0,
            relocs: 0,
            patches: 0,
            last_op_count: 0,
            budget_rejects: 0,
        }
    }
}

/// Intel 命令流编码后端（A 域命令抽象 → Intel dwords）。
pub struct CmdEncoder {
    variant: GenVariant,
    pub stats: EncStats,
    /// 已发射 dwords（u32 每 dword——dword 是硬件事实宽度）。
    batch: Vec<u32>,
    /// 待回填重定位项。
    relocs: Vec<Reloc>,
}

/// 地址解析器（finalize 时注入：符号 → 32 位地址）。
pub type SymResolver = fn(u64) -> Option<u64>;

impl CmdEncoder {
    /// 由代际分派构造（要点二）。
    pub fn new(tier: GenTier) -> CmdEncoder {
        CmdEncoder {
            variant: GenVariant::for_tier(tier),
            stats: EncStats::zero(),
            batch: Vec::new(),
            relocs: Vec::new(),
        }
    }

    /// 当前代际变体。
    pub const fn variant(&self) -> GenVariant {
        self.variant
    }

    /// 已发射 dword 数。
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// 读一个 dword（判据对拍用；越界 None）。
    pub fn dword(&self, i: usize) -> Option<u32> {
        if i < self.batch.len() {
            Some(self.batch[i])
        } else {
            None
        }
    }

    /// 重定位项快照（判据读）。
    pub fn relocs_snapshot(&self) -> Vec<Reloc> {
        let mut out: Vec<Reloc> = Vec::new();
        let mut i = 0usize;
        while i < self.relocs.len() {
            out.push(self.relocs[i]);
            i += 1;
        }
        out
    }

    // —— 3D 命令头（代际差异的显式落点，要点二）——

    /// 3DSTATE 命令头。返回 (dw0, ext_dw)；ext_dw 在非 Xe 代为 None。
    /// 长度字段契约口径 = 头之后（含 ext）的 dword 数。
    fn head_3d(&self, subop: u32, body_dwords: usize) -> (u32, Option<u32>) {
        let ext_extra = if self.variant.has_3d_ext() { 1usize } else { 0usize };
        let len_field = (body_dwords + ext_extra) as u32;
        let dw0 = (PIPELINE_3D << 29)
            | (SUBTYPE_3DSTATE << 27)
            | (subop << 16)
            | len_field;
        let ext = if self.variant.has_3d_ext() {
            // ext dword：本契约口径承载「扩展参数存在」标记位 20。
            Some(1u32 << 20)
        } else {
            None
        };
        (dw0, ext)
    }

    fn emit(&mut self, dws: &[u32]) {
        let mut i = 0usize;
        while i < dws.len() {
            self.batch.push(dws[i]);
            i += 1;
        }
        self.stats.dwords += dws.len() as u64;
        self.stats.commands += 1;
    }

    fn check_addr(a: u64) -> Result<u32, CErr> {
        if a > ADDR_MAX {
            return Err(CErr::AddrBeyond4G);
        }
        Ok(a as u32)
    }

    // —— MI 族发射口 ——

    /// MI_BATCH_BUFFER_START（跨 batch 跳转，2 dword）。
    ///
    /// `addr` 给定即时编码；`sym` 给定时收集重定位项（要点三）。
    /// 两者都给以 addr 为准；都不给是构造错误（零写入拒绝）。
    pub fn emit_mi_bb_start(
        &mut self,
        addr: Option<u64>,
        sym: Option<u64>,
        space: AddrSpace,
    ) -> Result<(), CErr> {
        if addr.is_none() && sym.is_none() {
            return Err(CErr::RelocUnresolved);
        }
        if let Some(a) = addr {
            if a % ALIGN_8 != 0 {
                return Err(CErr::BadBatchAlign);
            }
            Self::check_addr(a)?;
        }
        if self.batch.len() + 2 > MAX_BATCH_DWORDS {
            return Err(CErr::BatchTooLarge);
        }
        // DW0: opcode | 长度 1 | ASI(8) | (Xe2)Scope(9)。
        let mut dw0 = MI_OP_BB_START | 1 | (space.asi_bit() << 8);
        if self.variant.has_bb_scope() {
            dw0 |= 1 << 9;
        }
        match addr {
            Some(a) => {
                let lo = Self::check_addr(a)?;
                self.emit(&[dw0, lo]);
            }
            None => {
                // 符号目标：收集重定位（要点三），单 dword 占位。
                if self.relocs.len() >= MAX_RELOCS {
                    return Err(CErr::TooManyRelocs);
                }
                // 占位 dword 下标 = 发射后的 len+1（dw0 在 len，lo 在 len+1）。
                let at = self.batch.len() + 1;
                self.relocs.push(Reloc {
                    at_dword: at,
                    target: RelocTarget { sym: sym.unwrap_or(0) },
                });
                self.stats.relocs += 1;
                self.emit(&[dw0, 0]);
            }
        }
        Ok(())
    }

    /// MI_SEMAPHORE_WAIT（信号量等待/轮询，3 dword）。
    /// DW0：opcode | 长度 2 | WaitMode=1(14) | MemPoll(15, Xe 起) | CompareOp(12)。
    /// DW1：地址（bit[2:0] 恒 0——8 字节对齐纪律）；DW2：等待值。
    pub fn emit_mi_sem_wait(
        &mut self,
        addr: u64,
        value: u32,
        cmp: SemCompare,
        mem_poll: bool,
    ) -> Result<(), CErr> {
        if addr % ALIGN_8 != 0 {
            return Err(CErr::BadSemaphoreAlign);
        }
        if addr > ADDR_MAX {
            return Err(CErr::AddrBeyond4G);
        }
        if mem_poll && !self.variant.has_sem_mem_poll() {
            return Err(CErr::UnsupportedForGen);
        }
        let mut dw0 = MI_OP_SEM_WAIT | 2 | (1 << 14) | (cmp.bits() << 12);
        if mem_poll {
            dw0 |= 1 << 15;
        }
        let addr_dw = (addr as u32) & !0x7;
        self.emit(&[dw0, addr_dw, value]);
        Ok(())
    }

    /// MI_STORE_DATA_IMM（立即数写入内存，3 dword）。
    pub fn emit_mi_store_data(&mut self, addr: u64, value: u32) -> Result<(), CErr> {
        if addr % ALIGN_8 != 0 {
            return Err(CErr::BadStoreAlign);
        }
        if addr > ADDR_MAX {
            return Err(CErr::AddrBeyond4G);
        }
        let dw0 = MI_OP_STORE_DATA | 2;
        let addr_dw = (addr as u32) & !0x7; // bit[2:0] 恒 0
        self.emit(&[dw0, addr_dw, value]);
        Ok(())
    }

    /// MI_NOOP（对齐补齐的合法形态）。
    pub fn emit_mi_noop(&mut self) {
        self.emit(&[MI_OP_NOOP]);
    }

    /// 8 字节对齐补齐（要点三）：dword 数为奇时补一条 NOOP。
    pub fn pad_to_8(&mut self) {
        if self.batch.len() % 2 != 0 {
            self.emit_mi_noop();
            self.stats.noop_pads += 1;
        }
    }

    // —— 3D 族发射口（代际分派生效处）——

    /// 顶点取指设置。
    pub fn emit_3d_vf(&mut self, indexed: bool, index_elem_size: u8) -> Result<(), CErr> {
        if index_elem_size > 2 {
            return Err(CErr::BadIndexElementSize);
        }
        let (dw0, ext) = self.head_3d(SUBOP_VF, 1);
        let mut body = 0u32;
        if indexed {
            body |= 1 << 9; // IndexedDrawEnable
        }
        body |= (index_elem_size as u32) << 10; // IndexElementSize(11:10)
        match ext {
            Some(e) => self.emit(&[dw0, e, body]),
            None => self.emit(&[dw0, body]),
        }
        Ok(())
    }

    /// 视口设置（代际上限分派：Gen9=16，Xe 起=32）。
    pub fn emit_3d_viewport(&mut self, count: u8) -> Result<(), CErr> {
        if count == 0 || count > self.variant.max_viewports() {
            return Err(CErr::BadViewportCount);
        }
        let (dw0, ext) = self.head_3d(SUBOP_VIEWPORT, 1);
        let body = (count as u32) << 2; // ViewportCount(6:2)
        match ext {
            Some(e) => self.emit(&[dw0, e, body]),
            None => self.emit(&[dw0, body]),
        }
        Ok(())
    }

    /// 混合状态。
    pub fn emit_3d_blend(&mut self, write_enable: bool, logic_op: bool, color_mask: u8) -> Result<(), CErr> {
        let (dw0, ext) = self.head_3d(SUBOP_BLEND, 1);
        let mut body = color_mask as u32; // ColorWriteMask(7:0)
        if write_enable {
            body |= 1 << 8;
        }
        if logic_op {
            body |= 1 << 9;
        }
        match ext {
            Some(e) => self.emit(&[dw0, e, body]),
            None => self.emit(&[dw0, body]),
        }
        Ok(())
    }

    /// 深度模板状态。
    pub fn emit_3d_depth(&mut self, depth_test: bool, depth_write: bool) -> Result<(), CErr> {
        let (dw0, ext) = self.head_3d(SUBOP_DEPTH, 1);
        let mut body = 0u32;
        if depth_test {
            body |= 1 << 0;
        }
        if depth_write {
            body |= 1 << 1;
        }
        match ext {
            Some(e) => self.emit(&[dw0, e, body]),
            None => self.emit(&[dw0, body]),
        }
        Ok(())
    }

    /// 渲染目标集合。
    pub fn emit_3d_rt_set(&mut self, rt_count: u8, fmt: u32) -> Result<(), CErr> {
        if rt_count == 0 || rt_count > 8 {
            return Err(CErr::BadRtCount);
        }
        let (dw0, ext) = self.head_3d(SUBOP_RT_SET, 2);
        let body0 = rt_count as u32; // RTCount(3:0)
        let body1 = fmt & 0x3FF; // SurfaceFormat(9:0)
        match ext {
            Some(e) => self.emit(&[dw0, e, body0, body1]),
            None => self.emit(&[dw0, body0, body1]),
        }
        Ok(())
    }

    // —— 会话编排（上游一帧语义命令 → batch）——

    /// 编码一批语义命令（逐条分派到对应发射口；任一条失败即停止且
    /// 保留已发射前缀——前缀合法可继续用，失败者带专属码上报）。
    /// 结束时做 8 字节对齐补齐（要点三）并核对吞吐预算（要点四）。
    pub fn encode_all(&mut self, cmds: &[SemCmd]) -> Result<(), CErr> {
        let start_dwords = self.stats.dwords;
        let n = cmds.len() as u64;
        let mut i = 0usize;
        while i < cmds.len() {
            let r = match cmds[i] {
                SemCmd::MiBatchBufferStart { addr, sym, space } => {
                    self.emit_mi_bb_start(addr, sym, space)
                }
                SemCmd::MiSemaphoreWait { addr, value, cmp, mem_poll } => {
                    self.emit_mi_sem_wait(addr, value, cmp, mem_poll)
                }
                SemCmd::MiStoreData { addr, value } => self.emit_mi_store_data(addr, value),
                SemCmd::VfSetup { indexed, index_elem_size } => {
                    self.emit_3d_vf(indexed, index_elem_size)
                }
                SemCmd::Viewport { count } => self.emit_3d_viewport(count),
                SemCmd::Blend { write_enable, logic_op, color_mask } => {
                    self.emit_3d_blend(write_enable, logic_op, color_mask)
                }
                SemCmd::DepthStencil { depth_test, depth_write } => {
                    self.emit_3d_depth(depth_test, depth_write)
                }
                SemCmd::RenderTargets { rt_count, fmt } => self.emit_3d_rt_set(rt_count, fmt),
                SemCmd::MiNoop => {
                    self.emit_mi_noop();
                    Ok(())
                }
            };
            if let Err(e) = r {
                self.stats.last_op_count = self.stats.dwords - start_dwords;
                return Err(e);
            }
            // 吞吐预算逐条核对（要点四）：累计产出 ≤ 基数 + 已编命令数×每命令预算。
            let used = self.stats.dwords - start_dwords;
            if used > OP_BUDGET_BASE + ((i as u64) + 1) * OP_BUDGET_PER_CMD {
                self.stats.budget_rejects += 1;
                self.stats.last_op_count = used;
                return Err(CErr::EncBudgetExceeded);
            }
            i += 1;
        }
        self.pad_to_8();
        self.stats.last_op_count = self.stats.dwords - start_dwords;
        Ok(())
    }

    // —— finalize：patching（要点三）——

    /// finalize：以解析器回填全部重定位项。
    /// 返回回填数；任一目标解析不出即 [`CErr::RelocUnresolved`]（零回填——
    /// 与 F0222 批写纪律同构：先全解析后落笔，无半提交）。
    pub fn finalize(&mut self, resolver: SymResolver) -> Result<u32, CErr> {
        let mut resolved: Vec<(usize, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.relocs.len() {
            let r = self.relocs[i];
            match resolver(r.target.sym) {
                Some(a) => {
                    if a > ADDR_MAX {
                        return Err(CErr::AddrBeyond4G);
                    }
                    resolved.push((r.at_dword, a as u32));
                }
                None => return Err(CErr::RelocUnresolved),
            }
            i += 1;
        }
        let mut applied = 0u32;
        let mut k = 0usize;
        while k < resolved.len() {
            let (at, a) = resolved[k];
            if at >= self.batch.len() {
                return Err(CErr::PatchOutOfRange);
            }
            self.batch[at] = a;
            self.stats.patches += 1;
            applied += 1;
            k += 1;
        }
        self.relocs.clear();
        Ok(applied)
    }

    // —— A 域对接（F0004 环形分配器的传输形态）——

    /// 把已编码 batch 摘要为 F0004 `Command` 标量三元组：
    /// opcode=批标签 0x1E23，arg0=dword 数，arg1=dwords 的 FNV-1a 校验。
    /// 上游把它 enqueue 进环，消费者凭校验复核批次完整性。
    pub fn ring_command(&self) -> RingCommand {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0usize;
        while i < self.batch.len() {
            hash ^= self.batch[i] as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        RingCommand::new(0x1E23, self.batch.len() as u64, hash)
    }

    // —— 读屏摘要（隐私红线：不含地址与符号表）——

    pub fn status_summary(&self) -> String {
        let mut s = String::from("命令编码：已发射 ");
        s.push_str(&self.stats.commands.to_string());
        s.push_str(" 条 / ");
        s.push_str(&self.stats.dwords.to_string());
        s.push_str(" dword；待回填重定位 ");
        s.push_str(&self.stats.relocs.to_string());
        s.push_str(" 项；NOOP 补齐 ");
        s.push_str(&self.stats.noop_pads.to_string());
        s.push_str(" 次");
        s
    }
}
