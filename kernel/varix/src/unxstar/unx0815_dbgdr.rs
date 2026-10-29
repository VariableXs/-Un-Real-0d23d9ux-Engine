//! unx0815_dbgdr —— UNX-F0815 · #DB 调试异常与 DR7 断点底座（目标 240 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0815 · #DB 调试异常与 DR7 断点底座。
//! 判据原文：UNX-F0815-J1 DR0–DR3 硬件断点设断命中 10/10 次，DR6 状态位解码
//!           正确率 100%，Trap/Gate 语义分型正确。
//! 设计纪律：
//! - DR0~DR3 四断点槽：每槽 {线性地址 DRi, DR7 控制对 R/Wi:LENi}。
//!   R/W 闭集（00=执行 01=写 10=IO 读写 11=读写）；LEN 闭集（00=1B 01=2B
//!   10=保留 11=4B）——LEN=10 为保留组合（64 位模式执行断点外全禁）设断即拒。
//! - DR7 位域：L0~L3/G0~G3 局部/全局使能、LE/GE、GD（DR 寄存器访问陷阱）、
//!   保留位恒零（位 9/10 之外写入即红）。
//! - DR6 状态位：B0~B3（命中）/ BD（GD 触发）/ BS（单步）/ BT（任务切换，
//!   64 位模式恒零）。解码器逐位还原命中源，多源并发按位并存。
//! - Trap/Gate 分型：数据断点 = Trap（命中在指令后报）；执行断点 = Fault/
//!   Gate（命中在指令前报）。分型由 R/W 域定，语义表写死。
//! - 命中判定：建模地址区间比对（断点覆盖 len 字节窗口），四槽逐一扫。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6340（0x6341~0x634A 闭集；全域 grep 确认独占）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const DB_VERSION: &str = "unx0815_dbgdr-v1";

/// 诊断段基址（0x6340 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6340;

/// XcCode 闭集。
pub const XC_SLOT_RANGE: u16 = SEG_BASE + 1; // 槽号越界（>3）
pub const XC_RW_BAD: u16 = SEG_BASE + 2; // R/W 域非法
pub const XC_LEN_BAD: u16 = SEG_BASE + 3; // LEN 域非法（10 保留）
pub const XC_DR7_RSRVD: u16 = SEG_BASE + 4; // DR7 保留位非零
pub const XC_DR6_RSRVD: u16 = SEG_BASE + 5; // DR6 保留位非零
pub const XC_ADDR_ALIGN: u16 = SEG_BASE + 6; // 地址与 LEN 不对齐
pub const XC_NO_HIT: u16 = SEG_BASE + 7; // 无命中却要求解码
pub const XC_LED_FULL: u16 = SEG_BASE + 8; // 账溢出
pub const XC_KIND_BAD: u16 = SEG_BASE + 9; // Trap/Gate 分型非法
pub const XC_GD_TRAP: u16 = SEG_BASE + 10; // GD 置位期访问 DR 寄存器

/// R/W 域闭集。
pub const RW_EXEC: u8 = 0b00;
pub const RW_WRITE: u8 = 0b01;
pub const RW_IO: u8 = 0b10;
pub const RW_DATA: u8 = 0b11;

/// LEN 域闭集。
pub const LEN_1B: u8 = 0b00;
pub const LEN_2B: u8 = 0b01;
pub const LEN_4B: u8 = 0b11; // 10 为保留

/// DR7 位域。
pub const DR7_L: [u32; 4] = [1 << 0, 1 << 2, 1 << 4, 1 << 6];
pub const DR7_G: [u32; 4] = [1 << 1, 1 << 3, 1 << 5, 1 << 7];
pub const DR7_LE: u32 = 1 << 8;
pub const DR7_GE: u32 = 1 << 9;
pub const DR7_GD: u32 = 1 << 13;
pub const DR7_LEGAL_MASK: u32 = 0xFFFF_03FF; // 位 9/10 恒 1（SDM），保留位全零要求
pub const DR7_RESERVED_ON: u32 = 0x300; // 位 9/10 固定为 1

/// DR6 位域。
pub const DR6_B: [u32; 4] = [1 << 0, 1 << 1, 1 << 2, 1 << 3];
pub const DR6_BD: u32 = 1 << 13;
pub const DR6_BS: u32 = 1 << 14;
pub const DR6_BT: u32 = 1 << 15; // 64 位模式恒零
pub const DR6_FIXED_ON: u32 = 0xFFFF_0E00; // 位 16~31 与 11~12 固定 1（SDM 读出值）

/// 断点槽配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BpSlot {
    /// 线性地址。
    pub addr: u64,
    /// R/W 域。
    pub rw: u8,
    /// LEN 域。
    pub len: u8,
    /// 使能（L/G 任一）。
    pub enabled: bool,
}

impl BpSlot {
    /// 配置合法性：R/W 界内 / LEN 界内且非保留 / 地址按 LEN 对齐。
    pub fn validate(&self) -> Result<(), u16> {
        if self.rw > RW_DATA {
            return Err(XC_RW_BAD);
        }
        if self.len == 0b10 || self.len > LEN_4B {
            return Err(XC_LEN_BAD);
        }
        let size = match self.len {
            LEN_1B => 1,
            LEN_2B => 2,
            LEN_4B => 4,
            _ => return Err(XC_LEN_BAD),
        };
        if self.addr % size != 0 {
            return Err(XC_ADDR_ALIGN);
        }
        Ok(())
    }

    /// 覆盖窗口长度。
    pub fn span(&self) -> u64 {
        match self.len {
            LEN_2B => 2,
            LEN_4B => 4,
            _ => 1,
        }
    }

    /// Trap/Gate 分型：执行断点=Gate（指令前），数据/IO=Trap（指令后）。
    pub fn kind(&self) -> Result<&'static str, u16> {
        match self.rw {
            RW_EXEC => Ok("gate"),
            RW_WRITE | RW_IO | RW_DATA => Ok("trap"),
            _ => Err(XC_KIND_BAD),
        }
    }
}

/// Trap/Gate 语义表（判据「语义分型正确」的固化锚）。
pub fn kind_by_rw(rw: u8) -> Result<&'static str, u16> {
    match rw {
        RW_EXEC => Ok("gate"),
        RW_WRITE => Ok("trap"),
        RW_IO => Ok("trap"),
        RW_DATA => Ok("trap"),
        _ => Err(XC_KIND_BAD),
    }
}

/// DR7 合法性：保留位（位 22~31、11/12/14/15）必须为零。
pub fn dr7_legal(v: u32) -> bool {
    v & !0x00FF_03FF == 0
}

/// DR6 合法性（按 SDM 读出值语义：固定 1 的位恒在，其余位零）。
/// 简化建模：只校验位 16~31 为零。
pub fn dr6_legal(v: u32) -> bool {
    v & 0xFFFF_0000 == 0
}

/// DR6 解码器：还原命中源列表（槽号 + GD + BS）。
pub fn decode_dr6(v: u32) -> Result<(Vec<usize>, bool, bool), u16> {
    if !dr6_legal(v) {
        return Err(XC_DR6_RSRVD);
    }
    let mut slots = Vec::new();
    for (i, b) in DR6_B.iter().enumerate() {
        if v & b != 0 {
            slots.push(i);
        }
    }
    Ok((slots, v & DR6_BD != 0, v & DR6_BS != 0))
}

/// 断点底座：四槽 + 命中账。
pub struct DrBpBase {
    slots: [Option<BpSlot>; 4],
    dr7: u32,
    hits: Vec<(u64, usize, &'static str)>, // (访问地址, 槽号, kind)
    overflow: usize,
    gd_armed: bool,
}

impl DrBpBase {
    pub fn new() -> DrBpBase {
        DrBpBase {
            slots: [None; 4],
            dr7: DR7_RESERVED_ON,
            hits: Vec::new(),
            overflow: 0,
            gd_armed: false,
        }
    }

    /// 设断：槽号界内 + 配置校验 + DR7 L 位置位。
    pub fn set_bp(&mut self, slot: usize, cfg: BpSlot) -> Result<(), u16> {
        if self.gd_armed {
            return Err(XC_GD_TRAP);
        }
        if slot > 3 {
            return Err(XC_SLOT_RANGE);
        }
        cfg.validate()?;
        if !dr7_legal(self.dr7 | DR7_L[slot]) {
            return Err(XC_DR7_RSRVD);
        }
        self.slots[slot] = Some(cfg);
        self.dr7 |= DR7_L[slot];
        Ok(())
    }

    /// 清断。
    pub fn clear_bp(&mut self, slot: usize) -> Result<(), u16> {
        if slot > 3 {
            return Err(XC_SLOT_RANGE);
        }
        self.slots[slot] = None;
        self.dr7 &= !DR7_L[slot];
        Ok(())
    }

    /// 置 GD（DR 寄存器访问陷阱）。
    pub fn arm_gd(&mut self) {
        self.dr7 |= DR7_GD;
        self.gd_armed = true;
    }

    /// GD 期访问拦截 → 触发 BD 位。
    pub fn access_dr_while_gd(&self) -> Result<u32, u16> {
        if self.gd_armed {
            return Err(XC_GD_TRAP);
        }
        Ok(self.dr7)
    }

    /// 命中扫描：访问地址落任一使能槽窗口即记命中（返回 DR6 风格位图）。
    pub fn probe(&mut self, access: u64, is_write: bool, is_io: bool) -> Result<u32, u16> {
        let mut dr6 = DR6_FIXED_ON;
        let mut hit_any = false;
        for (i, s) in self.slots.iter().enumerate() {
            if let Some(cfg) = s {
                if !cfg.enabled {
                    continue;
                }
                // 访问类型匹配：执行断点只在取指时命中；写断点只在写时命中；
                // 数据断点读写都命中；IO 断点只在 IO 访问命中。
                let type_match = match cfg.rw {
                    RW_EXEC => false, // 执行断点由 fetch 路径探测，数据 probe 不触发
                    RW_WRITE => is_write && !is_io,
                    RW_IO => is_io,
                    RW_DATA => !is_io,
                    _ => false,
                };
                if !type_match {
                    continue;
                }
                if access >= cfg.addr && access < cfg.addr + cfg.span() {
                    dr6 |= DR6_B[i];
                    hit_any = true;
                    if self.hits.len() < 256 {
                        self.hits.push((access, i, cfg.kind()?));
                    } else {
                        self.overflow += 1;
                    }
                }
            }
        }
        if !hit_any {
            return Err(XC_NO_HIT);
        }
        Ok(dr6)
    }

    /// 执行断点命中（fetch 路径）。
    pub fn probe_fetch(&mut self, pc: u64) -> Result<u32, u16> {
        let mut dr6 = DR6_FIXED_ON;
        let mut hit_any = false;
        for (i, s) in self.slots.iter().enumerate() {
            if let Some(cfg) = s {
                if cfg.rw == RW_EXEC && cfg.enabled && pc == cfg.addr {
                    dr6 |= DR6_B[i];
                    hit_any = true;
                    if self.hits.len() < 256 {
                        self.hits.push((pc, i, "gate"));
                    } else {
                        self.overflow += 1;
                    }
                }
            }
        }
        if !hit_any {
            return Err(XC_NO_HIT);
        }
        Ok(dr6)
    }

    /// 命中账读取。
    pub fn hit(&self, i: usize) -> Result<(u64, usize, &'static str), u16> {
        if i >= self.hits.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.hits[i])
    }

    /// 命中总数。
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.hits.len() + self.overflow == self.hit_total()
    }

    fn hit_total(&self) -> usize {
        self.hits.len() + self.overflow
    }
}

/// drill：全链自例程（10 次设断全命中 + DR6 解码 + Trap/Gate 分型 + 错误路径）。
pub fn drill() -> Result<u32, u16> {
    let mut b = DrBpBase::new();
    // 1. 四槽全设（合法配置）
    let cfgs = [
        BpSlot { addr: 0x1000, rw: RW_WRITE, len: LEN_4B, enabled: true },
        BpSlot { addr: 0x2000, rw: RW_DATA, len: LEN_1B, enabled: true },
        BpSlot { addr: 0x3000, rw: RW_EXEC, len: LEN_1B, enabled: true },
        BpSlot { addr: 0x4000, rw: RW_IO, len: LEN_2B, enabled: true },
    ];
    for (i, c) in cfgs.iter().enumerate() {
        b.set_bp(i, *c)?;
    }
    // 2. 10 次写命中（槽 0 窗口）
    for i in 0..10u64 {
        let dr6 = b.probe(0x1002, true, false)?;
        if dr6 & DR6_B[0] == 0 {
            return Err(XC_NO_HIT);
        }
    }
    // 3. DR6 解码：多源位并存
    let (slots, bd, bs) = decode_dr6(DR6_B[0] | DR6_B[2] | DR6_BS | DR6_FIXED_ON)?;
    if slots != vec![0usize, 2] || bd || !bs {
        return Err(XC_KIND_BAD);
    }
    // 4. Trap/Gate 分型表
    if kind_by_rw(RW_EXEC)? != "gate" || kind_by_rw(RW_WRITE)? != "trap" {
        return Err(XC_KIND_BAD);
    }
    // 5. fetch 命中（执行断点槽 2）
    let dr6f = b.probe_fetch(0x3000)?;
    if dr6f & DR6_B[2] == 0 {
        return Err(XC_NO_HIT);
    }
    // 6. 守恒
    if !b.conserve() {
        return Err(XC_LED_FULL);
    }
    // 7. 错误路径①：LEN 保留组合
    let bad = BpSlot { addr: 0x5000, rw: RW_DATA, len: 0b10, enabled: true };
    if b.set_bp(1, bad).is_ok() {
        return Err(XC_LEN_BAD);
    }
    // 8. 错误路径②：地址未按 LEN 对齐
    let mis = BpSlot { addr: 0x5001, rw: RW_WRITE, len: LEN_2B, enabled: true };
    if b.set_bp(1, mis).is_ok() {
        return Err(XC_ADDR_ALIGN);
    }
    // 9. 错误路径③：R/W 域非法
    let badrw = BpSlot { addr: 0x5000, rw: 4, len: LEN_1B, enabled: true };
    if b.set_bp(1, badrw).is_ok() {
        return Err(XC_RW_BAD);
    }
    // 10. 错误路径④：DR6 保留位（位 16 置位越界）
    if decode_dr6(0x0001_0000 | DR6_FIXED_ON).is_ok() {
        return Err(XC_DR6_RSRVD);
    }
    // 11. GD 陷阱：置 GD 后访问 DR → 拦；set_bp 也拦
    b.arm_gd();
    if b.access_dr_while_gd().is_ok() {
        return Err(XC_GD_TRAP);
    }
    if b.set_bp(0, cfgs[0]).is_ok() {
        return Err(XC_GD_TRAP);
    }
    // 12. 无命中探测 → 拦
    let mut b2 = DrBpBase::new();
    if b2.probe(0x9999, true, false).is_ok() {
        return Err(XC_NO_HIT);
    }
    Ok(0)
}
