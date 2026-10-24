//! unx192_fsgsbase — UNX-F0192 fs/gs base 设置与 per-CPU 预备（0x6C90 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0192）：fs/gs base 设置与 per-CPU
//! 预备。判据原文（UNX-F0192-J1）：fsgsbase 路径与 legacy wrmsr 路径
//! 双支持，per-CPU 区访问抽样一致。
//!
//! 设计三线：①双路径写 base 模型（fsgsbase 指令路径需 CR4.FSGSBASE
//! 使能前置检查；legacy wrmsr 路径走 MSR FS_BASE/GS_BASE——两路径结果
//! 必须一致且前置检查不可绕）；②per-CPU 区自描述（区头 magic/自指针
//! self_ptr 指向区首——自指针错位是经典 per-CPU 搬迁 bug，抽样对拍
//! 兑现「访问抽样一致」）；③CPU 亲和账（每 CPU 一份区、base 互异、
//! 区尺寸一致三账齐）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const FSGS_VERSION: &str = "192-fsgsbase-v1";

/// 段基（0x6C90）。
pub const SEG_BASE: u16 = 0x6C90;

/// MSR 地址：FS_BASE / GS_BASE / KERNEL_GS_BASE。
pub const MSR_FS_BASE: u32 = 0xC000_0100;
pub const MSR_GS_BASE: u32 = 0xC000_0101;
pub const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;

/// CR4.FSGSBASE 位（bit 16）。
pub const CR4_FSGSBASE: u64 = 1 << 16;

/// per-CPU 区头 magic（"VPCP" LE）。
pub const PCPU_MAGIC: u32 = 0x5043_5056;

/// per-CPU 区头（区首 16 字节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcpuHeader {
    pub magic: u32,
    /// CPU 编号。
    pub cpu_id: u32,
    /// 自指针（= 区基址）。
    pub self_ptr: u64,
}

/// per-CPU 区。
#[derive(Debug, Clone)]
pub struct PcpuArea {
    pub base: u64,
    pub size: u64,
    pub header: PcpuHeader,
}

impl PcpuArea {
    /// 构造：基址 16 对齐、区 ≥ 头大小、自指针 = 基址、magic 落位。
    pub fn init(base: u64, cpu_id: u32, size: u64) -> Result<PcpuArea, u16> {
        if base == 0 || base % 16 != 0 {
            return Err(E_BASE_ALIGN);
        }
        if size < 16 {
            return Err(E_AREA_SHORT);
        }
        Ok(PcpuArea {
            base,
            size,
            header: PcpuHeader {
                magic: PCPU_MAGIC,
                cpu_id,
                self_ptr: base,
            },
        })
    }

    /// 抽样一致性读（判据主体）：读偏移处 header 字段与期望一致。
    pub fn sample_check(&self) -> Result<(), u16> {
        if self.header.magic != PCPU_MAGIC {
            return Err(E_MAGIC);
        }
        if self.header.self_ptr != self.base {
            return Err(E_SELF_PTR);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 双路径写 base
// ---------------------------------------------------------------------------

/// 写 base 路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasePath {
    /// fsgsbase 指令（需 CR4.FSGSBASE）。
    RdwrFsGs,
    /// legacy wrmsr。
    WrMsr,
}

/// 模拟 CR4 与两枚 MSR 的机器态。
#[derive(Debug, Clone)]
pub struct CpuRegs {
    pub cr4: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub kernel_gs_base: u64,
}

impl CpuRegs {
    pub fn fresh() -> Self {
        CpuRegs {
            cr4: 0,
            fs_base: 0,
            gs_base: 0,
            kernel_gs_base: 0,
        }
    }
}

/// fsgsbase 路径写 base：CR4.FSGSBASE 未使能即拒（前置检查不可绕）。
pub fn write_base_fsgs(r: &mut CpuRegs, target_fs: bool, v: u64) -> Result<(), u16> {
    if r.cr4 & CR4_FSGSBASE == 0 {
        return Err(E_FSGS_DISABLED);
    }
    if v == 0 {
        return Err(E_BASE_ZERO);
    }
    if target_fs {
        r.fs_base = v;
    } else {
        r.gs_base = v;
    }
    Ok(())
}

/// legacy wrmsr 路径写 base：按 MSR 地址路由（FS/GS/KERNEL_GS 三口）。
pub fn write_base_wrmsr(r: &mut CpuRegs, msr: u32, v: u64) -> Result<(), u16> {
    if v == 0 {
        return Err(E_BASE_ZERO);
    }
    match msr {
        MSR_FS_BASE => r.fs_base = v,
        MSR_GS_BASE => r.gs_base = v,
        MSR_KERNEL_GS_BASE => r.kernel_gs_base = v,
        _ => return Err(E_MSR_UNKNOWN),
    }
    Ok(())
}

/// 双路径一致性对拍（判据主体）：同值经两路径写入后寄存器实值一致。
pub fn dual_path_agree(v: u64) -> Result<bool, u16> {
    let mut a = CpuRegs::fresh();
    a.cr4 = CR4_FSGSBASE;
    let mut b = CpuRegs::fresh();
    write_base_fsgs(&mut a, false, v)?;
    write_base_wrmsr(&mut b, MSR_GS_BASE, v)?;
    Ok(a.gs_base == b.gs_base && b.gs_base == v)
}

// ---------------------------------------------------------------------------
// CPU 亲和账
// ---------------------------------------------------------------------------

/// 每 CPU 区账。
#[derive(Debug, Default)]
pub struct PcpuBoard {
    areas: Vec<PcpuArea>,
}

impl PcpuBoard {
    /// 挂区（同 CPU 重复挂拒）。
    pub fn attach(&mut self, base: u64, cpu_id: u32, size: u64) -> Result<(), u16> {
        if self.areas.iter().any(|a| a.header.cpu_id == cpu_id) {
            return Err(E_DUP_CPU);
        }
        let a = PcpuArea::init(base, cpu_id, size)?;
        a.sample_check()?;
        self.areas.push(a);
        Ok(())
    }

    /// 亲和审计：base 互异、尺寸一致、CPU 号连续 0..n。
    pub fn affinity_audit(&self) -> Result<(), u16> {
        let n = self.areas.len();
        if n == 0 {
            return Err(E_NO_AREA);
        }
        let size0 = self.areas[0].size;
        for (i, a) in self.areas.iter().enumerate() {
            if a.header.cpu_id != i as u32 {
                return Err(E_CPU_GAP);
            }
            if a.size != size0 {
                return Err(E_SIZE_UNEVEN);
            }
            for b in self.areas.iter().skip(i + 1) {
                if a.base == b.base {
                    return Err(E_BASE_DUP);
                }
            }
        }
        Ok(())
    }

    /// 按抽样的 base 查区（访问抽样一致的第二层：base → 区往返）。
    pub fn lookup_by_base(&self, base: u64) -> Result<u32, u16> {
        self.areas
            .iter()
            .find(|a| a.base == base)
            .map(|a| a.header.cpu_id)
            .ok_or(E_NO_AREA)
    }

    /// 区数。
    pub fn len(&self) -> usize {
        self.areas.len()
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x6C90 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum FsGsErr {
    /// CR4.FSGSBASE 未使能。
    FsgsDisabled = SEG_BASE + 1,
    /// base 零值。
    BaseZero = SEG_BASE + 2,
    /// 未知 MSR。
    MsrUnknown = SEG_BASE + 3,
    /// 区基址未对齐。
    BaseAlign = SEG_BASE + 4,
    /// 区过小。
    AreaShort = SEG_BASE + 5,
    /// magic 不符。
    Magic = SEG_BASE + 6,
    /// 自指针错位。
    SelfPtr = SEG_BASE + 7,
    /// 同 CPU 重复挂区。
    DupCpu = SEG_BASE + 8,
    /// CPU 号缺口。
    CpuGap = SEG_BASE + 9,
    /// 区尺寸不齐。
    SizeUneven = SEG_BASE + 10,
    /// base 重复。
    BaseDup = SEG_BASE + 11,
    /// 区缺失。
    NoArea = SEG_BASE + 12,
}

impl FsGsErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            FsGsErr::FsgsDisabled => "CR4.FSGSBASE 未使能",
            FsGsErr::BaseZero => "base 零值非法",
            FsGsErr::MsrUnknown => "MSR 地址不在 FS/GS/KERNEL_GS 三口",
            FsGsErr::BaseAlign => "per-CPU 区基址未 16 对齐",
            FsGsErr::AreaShort => "区不足 16 字节头",
            FsGsErr::Magic => "区头 magic 不符",
            FsGsErr::SelfPtr => "自指针 ≠ 区基址（错位）",
            FsGsErr::DupCpu => "同 CPU 重复挂区",
            FsGsErr::CpuGap => "CPU 号不连续",
            FsGsErr::SizeUneven => "区尺寸不一致",
            FsGsErr::BaseDup => "区基址重复",
            FsGsErr::NoArea => "区缺失",
        }
    }
}

const E_FSGS_DISABLED: u16 = FsGsErr::FsgsDisabled.code();
const E_BASE_ZERO: u16 = FsGsErr::BaseZero.code();
const E_MSR_UNKNOWN: u16 = FsGsErr::MsrUnknown.code();
const E_BASE_ALIGN: u16 = FsGsErr::BaseAlign.code();
const E_AREA_SHORT: u16 = FsGsErr::AreaShort.code();
const E_MAGIC: u16 = FsGsErr::Magic.code();
const E_SELF_PTR: u16 = FsGsErr::SelfPtr.code();
const E_DUP_CPU: u16 = FsGsErr::DupCpu.code();
const E_CPU_GAP: u16 = FsGsErr::CpuGap.code();
const E_SIZE_UNEVEN: u16 = FsGsErr::SizeUneven.code();
const E_BASE_DUP: u16 = FsGsErr::BaseDup.code();
const E_NO_AREA: u16 = FsGsErr::NoArea.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据集（UNX-F0192-J1 + 结构红线）。自证六段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：双路径同值写入后实值一致。
    if !dual_path_agree(0x7F00_0000)? {
        return Err(E_BASE_ZERO);
    }
    // 二、前置检查不可绕：CR4.FSGSBASE 未使能时 fsgsbase 路径必拒，
    // 而 legacy 路径照常可用（双支持语义的正反两面）。
    let mut r = CpuRegs::fresh();
    if write_base_fsgs(&mut r, true, 0x1000).is_ok() {
        return Err(E_FSGS_DISABLED);
    }
    write_base_wrmsr(&mut r, MSR_FS_BASE, 0x1000)?;
    if r.fs_base != 0x1000 {
        return Err(E_BASE_ZERO);
    }
    // 三、三口路由与未知 MSR 拒。
    let mut r2 = CpuRegs::fresh();
    write_base_wrmsr(&mut r2, MSR_KERNEL_GS_BASE, 0x2000)?;
    if r2.kernel_gs_base != 0x2000 {
        return Err(E_MSR_UNKNOWN);
    }
    if write_base_wrmsr(&mut r2, 0xDEAD, 0x1).is_ok() {
        return Err(E_MSR_UNKNOWN);
    }
    if write_base_wrmsr(&mut r2, MSR_GS_BASE, 0).is_ok() {
        return Err(E_BASE_ZERO);
    }
    // 四、per-CPU 抽样一致：挂双区、抽样过、base 往返命中。
    let mut board = PcpuBoard::default();
    board.attach(0x7F00_0000, 0, 4096)?;
    board.attach(0x7F00_1000, 1, 4096)?;
    board.affinity_audit()?;
    if board.lookup_by_base(0x7F00_1000)? != 1 {
        return Err(E_SELF_PTR);
    }
    // 五、红线逐项：自指针错位 / magic 毁 / 同 CPU 重挂 / CPU 缺口 /
    // 尺寸不齐 / base 重复合 / 基址未对齐 各自拒。
    let mut wrong = PcpuArea::init(0x7F00_2000, 9, 4096)?;
    wrong.header.self_ptr = 0x7F00_2008;
    if wrong.sample_check().is_ok() {
        return Err(E_SELF_PTR);
    }
    let mut dead = PcpuArea::init(0x7F00_3000, 10, 4096)?;
    dead.header.magic = 0xDEAD_BEEF;
    if dead.sample_check().is_ok() {
        return Err(E_MAGIC);
    }
    let mut board2 = PcpuBoard::default();
    board2.attach(0x7F00_0000, 0, 4096)?;
    if board2.attach(0x7F00_1000, 0, 4096).is_ok() {
        return Err(E_DUP_CPU);
    }
    if board2.attach(0x7F00_1004, 1, 4096).is_ok() {
        return Err(E_BASE_ALIGN);
    }
    if board2.attach(0x7F00_1000, 1, 8).is_ok() {
        return Err(E_AREA_SHORT);
    }
    let mut board3 = PcpuBoard::default();
    board3.attach(0x7F00_0000, 0, 4096)?;
    board3.attach(0x7F00_1000, 2, 4096)?;
    if board3.affinity_audit().is_ok() {
        return Err(E_CPU_GAP);
    }
    let mut board4 = PcpuBoard::default();
    board4.attach(0x7F00_0000, 0, 4096)?;
    board4.attach(0x7F00_1000, 1, 8192)?;
    if board4.affinity_audit().is_ok() {
        return Err(E_SIZE_UNEVEN);
    }
    // 六、判别式段位对拍。
    if E_NO_AREA != SEG_BASE + 12 || E_FSGS_DISABLED != SEG_BASE + 1 {
        return Err(E_NO_AREA);
    }
    Ok(())
}
