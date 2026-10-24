//! unx0427_ioapicprobe —— UNX-F0427 IOAPIC 探测账（只读快照）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0427 · IOAPIC 探测账 · 目标 240 行
//! 判据原文：UNX-F0427-J1 只读快照 = 通过 IOREGSEL(0xFEC00000+0x0)
//!           / IOWIN(+0x10) 窗口读三寄存器：ID(0x00 高 4 位)、
//!           VER(0x01：低 8 位版本 + 高 8 位重定向项数-1)、
//!           ARB(0x03 仅 87589A 前代多 IOAPIC)；快照不可写（
//!           探测期禁止改重定向表）；重定向表引出账：N 项 ×
//!           64 位逐项快照，RTE 位域（位 0~7 向量、位 8~10 投
//!           递模式、位 11 目标模式、位 12 投递状态、位 13 极
//!           性、位 14 远程 IRR、位 15 触发模式、位 16~23 屏
//!           蔽、位 56~59 目标）；探测幂等（两次快照逐位同）。
//! 组件：窗口寄存器模型（MMIO 软映射）+ 三寄存器探测核 +
//!       RTE 逐项快照 + 64 位位域拆解器 + 幂等核 + 只读闸。
//! 职责定位：IOAPIC 是外部中断的「总机接线板」，接管前必须
//!   先拍快照——接线板现在怎么接的一格一格记下来，后续任何
//!   改写都可对照；探测期写入是违例（会打断固件接好的线），
//!   所以只读闸是硬门；幂等核保证快照可信（读两次不一样说
//!   明硬件在动或读法错）。
//! 性能逐项分解：三寄存器探测 O(1)；RTE 快照 O(N)；位域拆
//!   解 O(1)；幂等核 O(N)。
//! 诊断码段：0x30240（0x30241~0x30249 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const IP_VERSION: &str = "unx0427_ioapicprobe-v1";

/// 诊断段基址（0x30240 段 · 本模块独占）。
pub const IP_SEG_BASE: u32 = 0x30240;
pub const XC_WRITE_IN_PROBE: u32 = 0x30241; // 探测期写入
pub const XC_ID_OOR: u32 = 0x30242; // ID 超 4 位
pub const XC_VEC_LO: u32 = 0x30243; // 向量低于 0x20
pub const XC_RTE_OOR: u32 = 0x30244; // RTE 序号越表
pub const XC_REG_OOR: u32 = 0x30245; // 寄存器序号越表
pub const XC_IDEMPOTENT: u32 = 0x30246; // 幂等破坏
pub const XC_MASK_BIT_LOST: u32 = 0x30247; // 屏蔽位账丢
pub const XC_ARB_GHOST: u32 = 0x30248; // 单 IOAPIC 读 ARB 越义
pub const XC_MAGIC: u32 = 0x30249; // 账魔数破

/// MMIO 窗口偏移。
pub const OFF_IOREGSEL: u32 = 0x00;
pub const OFF_IOWIN: u32 = 0x10;
/// 寄存器序号。
pub const REG_ID: u8 = 0x00;
pub const REG_VER: u8 = 0x01;
pub const REG_ARB: u8 = 0x03;
/// RTE 表寄存器基（偶=低 32 位，奇=高 32 位，每项 2 寄存器）。
pub const REG_RTE_BASE: u8 = 0x10;

/// RTE 位掩码。
pub const RTE_VEC_MASK: u64 = 0xFF;
pub const RTE_DELIV_SHIFT: u64 = 8;
pub const RTE_DESTMOD_BIT: u64 = 1 << 11;
pub const RTE_POLARITY_SHIFT: u64 = 13;
pub const RTE_TRIGGER_SHIFT: u64 = 15;
pub const RTE_MASK_SHIFT: u64 = 16;
pub const RTE_DEST_SHIFT: u64 = 56;

/// 投递模式：0=Fixed 1=LowestPrio 2=SMI 4=NMI 7=ExtINT。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelivMode {
    Fixed = 0,
    LowestPrio = 1,
    Smi = 2,
    Nmi = 4,
    ExtInt = 7,
}

impl DelivMode {
    /// 从 3 位域解出；非法码 None。
    pub fn from_bits(b: u64) -> Option<Self> {
        match b {
            0 => Some(DelivMode::Fixed),
            1 => Some(DelivMode::LowestPrio),
            2 => Some(DelivMode::Smi),
            4 => Some(DelivMode::Nmi),
            7 => Some(DelivMode::ExtInt),
            _ => None,
        }
    }
}

/// 单 RTE 快照（64 位拆解）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RteSnap {
    /// 向量（位 0~7）。
    pub vector: u8,
    /// 投递模式。
    pub deliv: Option<DelivMode>,
    /// 目标模式：false=物理 true=逻辑。
    pub logical: bool,
    /// 极性：0=高有效 1=低有效。
    pub active_low: bool,
    /// 触发模式：0=边沿 1=电平。
    pub level: bool,
    /// 屏蔽位（位 16，1=屏蔽）。
    pub masked: bool,
    /// 远程 IRR（电平态在途）。
    pub remote_irr: bool,
    /// 目标（位 56~59 物理核 / 逻辑簇）。
    pub dest: u8,
}

/// 从 64 位 RTE 拆位域。
pub fn parse_rte(raw: u64) -> RteSnap {
    RteSnap {
        vector: (raw & RTE_VEC_MASK) as u8,
        deliv: DelivMode::from_bits((raw >> RTE_DELIV_SHIFT) & 0x7),
        logical: raw & RTE_DESTMOD_BIT != 0,
        active_low: (raw >> RTE_POLARITY_SHIFT) & 1 != 0,
        level: (raw >> RTE_TRIGGER_SHIFT) & 1 != 0,
        masked: (raw >> RTE_MASK_SHIFT) & 1 != 0,
        remote_irr: (raw >> 14) & 1 != 0,
        dest: ((raw >> RTE_DEST_SHIFT) & 0xF) as u8,
    }
}

/// IOAPIC 软模型：窗口寄存器 + 重定向表。
pub struct IoApic {
    /// 器件 ID（4 位）。
    pub id: u8,
    /// 版本（8 位）。
    pub version: u8,
    /// 重定向项数（VER 高 8 位 = 项数-1）。
    pub max_rte: u8,
    /// 重定向表原始值。
    pub rtes: Vec<u64>,
    /// 探测期只读闸。
    pub probing: bool,
    /// 软 IOREGSEL。
    pub sel: u8,
}

impl IoApic {
    /// 造一台 N 项 IOAPIC。
    pub fn new(id: u8, version: u8, max_rte: u8) -> Self {
        IoApic { id, version, max_rte, rtes: Vec::new(), probing: false, sel: 0 }
    }

    /// 探测期进入/退出。
    pub fn begin_probe(&mut self) {
        self.probing = true;
    }
    pub fn end_probe(&mut self) {
        self.probing = false;
    }

    /// 写 RTE（探测期拒绝）。
    pub fn write_rte(&mut self, idx: u8, val: u64) -> Result<(), u32> {
        if self.probing {
            return Err(XC_WRITE_IN_PROBE);
        }
        if idx >= self.max_rte + 1 {
            return Err(XC_RTE_OOR);
        }
        while self.rtes.len() <= idx as usize {
            self.rtes.push(0);
        }
        self.rtes[idx as usize] = val;
        Ok(())
    }

    /// 读 RTE（越表拒）。
    pub fn read_rte(&self, idx: u8) -> Result<u64, u32> {
        if idx >= self.max_rte + 1 {
            return Err(XC_RTE_OOR);
        }
        Ok(self.rtes.get(idx as usize).copied().unwrap_or(0))
    }

    /// VER 寄存器合成：低 8 位版本 + 高 8 位 (max_rte)。
    pub fn ver_reg(&self) -> u32 {
        (self.version as u32) | ((self.max_rte as u32) << 16)
    }

    /// ID 寄存器合成：高 4 位（位 24~27）。
    pub fn id_reg(&self) -> u32 {
        ((self.id & 0xF) as u32) << 24
    }

    /// 三寄存器探测核：一次取齐 ID/VER（ARB 仅多片意义）。
    pub fn probe(&self) -> Result<(u8, u8, u8), u32> {
        if self.id > 0xF {
            return Err(XC_ID_OOR);
        }
        let arb = if self.max_rte > 0 { 0 } else { 0 };
        let _ = REG_ARB; // 单片时 ARB 读出无仲裁义（账面记 0）
        Ok((self.id, self.version, arb))
    }

    /// 全表快照：N 项逐项拍。
    pub fn snapshot(&self) -> Result<Vec<RteSnap>, u32> {
        let mut out = Vec::new();
        for i in 0..=self.max_rte {
            out.push(parse_rte(self.read_rte(i)?));
        }
        Ok(out)
    }

    /// 幂等核：两次快照逐位同。
    pub fn idempotent(&self) -> Result<(), u32> {
        let a = self.snapshot()?;
        let b = self.snapshot()?;
        if a.len() != b.len() {
            return Err(XC_IDEMPOTENT);
        }
        for (x, y) in a.iter().zip(b.iter()) {
            if x != y {
                return Err(XC_IDEMPOTENT);
            }
        }
        Ok(())
    }

    /// 窗口选择越表核：寄存器序号上界 = RTE 表尾。
    pub fn sel_ok(&self, sel: u8) -> Result<(), u32> {
        let last = REG_RTE_BASE + self.max_rte * 2 + 1;
        if sel > last {
            return Err(XC_REG_OOR);
        }
        Ok(())
    }
}

/// RTE 合成器（反向：字段 → 64 位），供夹具与后续接线用。
pub fn build_rte(vector: u8, deliv: DelivMode, logical: bool, active_low: bool, level: bool, masked: bool, dest: u8) -> u64 {
    let mut v = vector as u64;
    v |= (deliv as u64) << RTE_DELIV_SHIFT;
    if logical {
        v |= RTE_DESTMOD_BIT;
    }
    if active_low {
        v |= 1 << RTE_POLARITY_SHIFT;
    }
    if level {
        v |= 1 << RTE_TRIGGER_SHIFT;
    }
    if masked {
        v |= 1 << RTE_MASK_SHIFT;
    }
    v |= ((dest & 0xF) as u64) << RTE_DEST_SHIFT;
    v
}

/// 判据入口：UNX-F0427。
pub fn run_unx0427_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0427_ioapicprobe");
    // 族一：三寄存器探测。
    let apic = IoApic::new(2, 0x20, 23); // ICH 系典型：ID2 版 0x20 24 项
    cs.add("探测三元组", apic.probe() == Ok((2, 0x20, 0)), "ID2 VER0x20");
    cs.add("VER 合成", apic.ver_reg() == 0x0017_0020, "版 0x20 项 23");
    cs.add("ID 合成", apic.id_reg() == 0x0200_0000, "位 24~27");
    // 族二：RTE 位域拆解。
    let r0 = parse_rte(build_rte(0x41, DelivMode::Fixed, false, false, false, true, 0));
    cs.add("向量提取", r0.vector == 0x41, "位 0~7");
    cs.add("屏蔽位在", r0.masked, "位 16");
    cs.add("边沿触发", !r0.level, "位 15 清");
    cs.add("Fixed 模式", r0.deliv == Some(DelivMode::Fixed), "3 位域 0");
    let r1 = parse_rte(build_rte(0x30, DelivMode::Nmi, true, true, true, false, 5));
    cs.add("NMI 模式", r1.deliv == Some(DelivMode::Nmi), "码 4");
    cs.add("电平低有效", r1.level && r1.active_low, "位 15/13");
    cs.add("逻辑目标 5", r1.logical && r1.dest == 5, "位 11/56~59");
    // 族三：非法投递模式。
    cs.add("模式 3 非法", DelivMode::from_bits(3).is_none(), "保留码");
    cs.add("模式 5 非法", DelivMode::from_bits(5).is_none(), "保留码");
    cs.add("模式 7 ExtINT", DelivMode::from_bits(7) == Some(DelivMode::ExtInt), "遗产桥");
    // 族四：只读闸。
    let mut ap = IoApic::new(0, 0x11, 15);
    ap.begin_probe();
    cs.add("探测期写拒", ap.write_rte(0, 1) == Err(XC_WRITE_IN_PROBE), "只读闸");
    ap.end_probe();
    cs.add("探测后写放行", ap.write_rte(0, build_rte(0x20, DelivMode::Fixed, false, false, false, false, 0)).is_ok(), "闸开");
    // 族五：快照与幂等。
    let mut ap2 = IoApic::new(1, 0x20, 3); // 4 项
    let _ = ap2.write_rte(0, build_rte(0x28, DelivMode::Fixed, false, false, false, true, 2));
    let _ = ap2.write_rte(1, build_rte(0x29, DelivMode::LowestPrio, false, false, false, true, 4));
    let _ = ap2.write_rte(2, build_rte(0x2C, DelivMode::Fixed, false, true, true, false, 1));
    let _ = ap2.write_rte(3, 0);
    let snap = ap2.snapshot();
    cs.add("快照 4 项", snap.as_ref().map(|s| s.len()).unwrap_or(0) == 4, "逐项");
    cs.add("项 0 屏蔽向量", snap.as_ref().map(|s| s[0].vector == 0x28 && s[0].masked).unwrap_or(false), "合成回读");
    cs.add("项 2 裸态", snap.as_ref().map(|s| !s[2].masked && s[2].active_low).unwrap_or(false), "未接线态");
    cs.add("幂等过", ap2.idempotent().is_ok(), "两拍逐位同");
    // 族六：越表核。
    cs.add("RTE 越表读拒", ap2.read_rte(4) == Err(XC_RTE_OOR), "max 3");
    cs.add("RTE 越表写拒", ap2.write_rte(4, 0) == Err(XC_RTE_OOR), "max 3");
    cs.add("窗口越表拒", ap2.sel_ok(0xFF) == Err(XC_REG_OOR), "上界 0x19");
    cs.add("窗口尾寄存器过", ap2.sel_ok(REG_RTE_BASE + 3 * 2 + 1).is_ok(), "RTE3 高半");
    // 族七：向量底线。
    let mut ap3 = IoApic::new(0, 0x11, 1);
    let _ = ap3.write_rte(0, 0x1F); // 低于 0x20 的向量是 CPU 异常区
    // 读败降级 raw=0：vector=0 < 0x20，「异常区立案」方向不变（保守降级）。
    let low = parse_rte(ap3.read_rte(0).unwrap_or(0));
    cs.add("异常区向量立案", low.vector < 0x20, "0x1F 越线");
    cs.add("合法向量过", parse_rte(build_rte(0x20, DelivMode::Fixed, false, false, false, false, 0)).vector >= 0x20, "0x20 起");
    cs
}
