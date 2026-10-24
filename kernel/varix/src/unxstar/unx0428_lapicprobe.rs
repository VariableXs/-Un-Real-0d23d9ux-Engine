//! unx0428_lapicprobe —— UNX-F0428 LAPIC 探测账（基址/版本/ID）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0428 · LAPIC 探测账 · 目标 240 行
//! 判据原文：UNX-F0428-J1 LAPIC 探测三要素：基址（IA32_APIC_
//!           BASE MSR 0x1B：位 11~12 基址类型，位 12=EN 启
//!           用，位 11=x2APIC 模式，默认物理基 0xFEE00000）、
//!           版本（LVR 偏移 0x30：低 8 位版本号，位 16~23 最
//!           大 LVT 项数-1，位 24~31 族号）、本地 ID（LSR 偏
//!           移 0x20：xAPIC 取位 24~31，x2APIC 全 32 位）；
//!           探测只读不改（SVR 之外一律不动）；xAPIC/x2APIC
//!           双模判别一致性：MSR 位 11 与基址类型自洽。
//! 组件：MSR 位域拆解器 + LVR/LSR 窗口模型 + 双模判别核 +
//!       探测只读闸 + LVT 项数账 + ID 宽度规整器。
//! 职责定位：LAPIC 是每核中断的「本地邮局」，接管编排要先
//!   知道每个邮局的门牌（ID）、规模（LVT 项数）和制式（
//!   xAPIC 还是 x2APIC）；读错模 ID 会错位，判错模写错寄存
//!   器宽度，所以双模判别自洽是硬判据；探测期只读是把「看
//!   一眼」和「动一手」分开的纪律。
//! 性能逐项分解：MSR 拆解 O(1)；窗口读 O(1)；双模判别 O(1)；
//!   LVT 账 O(1)。
//! 诊断码段：0x30280（0x30281~0x30289 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const LP_VERSION: &str = "unx0428_lapicprobe-v1";

/// 诊断段基址（0x30280 段 · 本模块独占）。
pub const LP_SEG_BASE: u32 = 0x30280;
pub const XC_MODE_CONFLICT: u32 = 0x30281; // 双模判别不自洽
pub const XC_BASE_UNMAPPED: u32 = 0x30282; // 基址非 4K 对齐
pub const XC_APIC_DISABLED: u32 = 0x30283; // EN 位未置
pub const XC_ID_OOR: u32 = 0x30284; // ID 越模宽
pub const XC_REG_OOR: u32 = 0x30285; // 寄存器偏移越表
pub const XC_WRITE_IN_PROBE: u32 = 0x30286; // 探测期写入
pub const XC_LVT_OOR: u32 = 0x30287; // LVT 项数越域
pub const XC_MAGIC: u32 = 0x30289; // 账魔数破

/// IA32_APIC_BASE MSR 序号与位域。
pub const MSR_APIC_BASE: u32 = 0x1B;
pub const BASE_EN_BIT: u64 = 1 << 11; // x2APIC 模式位（位 11）
pub const BASE_GLOBAL_EN: u64 = 1 << 11; // GlobalEnable（x2APIC 语义）
pub const BASE_ENABLE: u64 = 1 << 11; // 旧名：APIC Global Enable
pub const BASE_X2_EN: u64 = 1 << 10; // Enable x2APIC（位 10）
pub const BASE_ADDR_MASK: u64 = 0xFFFF_FFFF_FFFF_F000;
/// 默认物理基址。
pub const DEFAULT_BASE: u64 = 0xFEE0_0000;
/// LVR/LSR 偏移。
pub const OFF_LSR: u32 = 0x20;
pub const OFF_LVR: u32 = 0x30;
/// 寄存器步长。
pub const REG_STRIDE: u32 = 0x10;

/// 双模：xAPIC / x2APIC。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApicMode {
    XApic = 1,
    X2Apic = 2,
}

/// 从 MSR 0x1B 拆基址三元组。
pub fn parse_apic_base(msr: u64) -> Result<(u64, bool, ApicMode), u32> {
    let addr = msr & BASE_ADDR_MASK;
    let en = msr & BASE_ENABLE != 0;
    let x2 = msr & BASE_X2_EN != 0;
    if addr & 0xFFF != 0 {
        return Err(XC_BASE_UNMAPPED);
    }
    if !en {
        return Err(XC_APIC_DISABLED);
    }
    // 自洽核：x2APIC 模式必须建立在全局启用之上（EN 已核）。
    let mode = if x2 { ApicMode::X2Apic } else { ApicMode::XApic };
    Ok((addr, en, mode))
}

/// LAPIC 软模型。
pub struct Lapic {
    /// 物理基址。
    pub base: u64,
    /// 模式。
    pub mode: ApicMode,
    /// 版本号（LVR 低 8 位）。
    pub version: u8,
    /// 最大 LVT 项数-1（LVR 位 16~23）。
    pub max_lvt: u8,
    /// 族号（LVR 位 24~31）。
    pub family: u8,
    /// 本地 ID。
    pub id: u32,
    /// 寄存器窗口（偏移 → 值，只读探测面）。
    pub regs: [(u32, u32); 4],
    /// 探测闸。
    pub probing: bool,
}

impl Lapic {
    /// 造一个 xAPIC 核。
    pub fn xapic(id: u8, version: u8, max_lvt: u8, family: u8) -> Self {
        Lapic {
            base: DEFAULT_BASE,
            mode: ApicMode::XApic,
            version,
            max_lvt,
            family,
            id: id as u32,
            regs: [
                (OFF_LSR, (id as u32) << 24),
                (OFF_LVR, (version as u32) | ((max_lvt as u32) << 16) | ((family as u32) << 24)),
                (0xF0, 0x1FF), // SVR：假 Spurious 向量 0xFF + 使能位
                (0x80, 0),     // TPR
            ],
            probing: false,
        }
    }

    /// 造一个 x2APIC 核（ID 全 32 位，MSR 面）。
    pub fn x2apic(id: u32, version: u8, max_lvt: u8, family: u8) -> Self {
        Lapic {
            base: DEFAULT_BASE,
            mode: ApicMode::X2Apic,
            version,
            max_lvt,
            family,
            id,
            regs: [
                (OFF_LSR, id),
                (OFF_LVR, (version as u32) | ((max_lvt as u32) << 16) | ((family as u32) << 24)),
                (0xF0, 0x1FF),
                (0x80, 0),
            ],
            probing: false,
        }
    }

    /// 只读读窗口寄存器（偏移须 16 对齐且在表内）。
    pub fn read_reg(&self, off: u32) -> Result<u32, u32> {
        if off % REG_STRIDE != 0 {
            return Err(XC_REG_OOR);
        }
        for (o, v) in self.regs.iter() {
            if *o == off {
                return Ok(*v);
            }
        }
        Err(XC_REG_OOR)
    }

    /// 探测期写入拒。
    pub fn write_reg(&mut self, off: u32, val: u32) -> Result<(), u32> {
        if self.probing {
            return Err(XC_WRITE_IN_PROBE);
        }
        if off % REG_STRIDE != 0 {
            return Err(XC_REG_OOR);
        }
        let _ = val;
        Ok(())
    }

    /// LVR 拆解：(版本, max_lvt+1, 族)。
    pub fn lvr_triple(&self) -> (u8, u8, u8) {
        (self.version, self.max_lvt + 1, self.family)
    }

    /// ID 规整：xAPIC 取位 24~31；x2APIC 全 32 位。
    pub fn id_canon(&self) -> Result<u32, u32> {
        match self.mode {
            ApicMode::XApic => {
                let hi = (self.id >> 24) & 0xFF;
                if hi == 0 && self.id != 0 {
                    // xAPIC 下 ID 只能在位 24~31：低位有值即越模宽。
                    return Err(XC_ID_OOR);
                }
                Ok(hi)
            }
            ApicMode::X2Apic => {
                // x2APIC 全 32 位：允许任意值；这里只核非零。
                if self.id == 0 {
                    return Err(XC_ID_OOR);
                }
                Ok(self.id)
            }
        }
    }

    /// LVT 项数账：合法上界 6（0~5 项差一编码，典型 5/6）。
    pub fn lvt_ok(&self) -> Result<(), u32> {
        if self.max_lvt > 5 {
            return Err(XC_LVT_OOR);
        }
        Ok(())
    }

    /// 双模判别自洽核：MSR 面（en/x2 位）与窗口面（模式）一致。
    pub fn mode_selfcheck(&self, msr: u64) -> Result<ApicMode, u32> {
        let (_, _, m) = parse_apic_base(msr)?;
        if m != self.mode {
            return Err(XC_MODE_CONFLICT);
        }
        Ok(m)
    }
}

/// 合成 MSR 0x1B 值（夹具用）。
pub fn build_msr(base: u64, en: bool, x2: bool) -> u64 {
    let mut v = base & BASE_ADDR_MASK;
    if en {
        v |= BASE_ENABLE;
    }
    if x2 {
        v |= BASE_X2_EN;
    }
    v
}

/// 判据入口：UNX-F0428。
pub fn run_unx0428_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0428_lapicprobe");
    // 族一：MSR 基址拆解。
    let msr = build_msr(DEFAULT_BASE, true, false);
    let p = parse_apic_base(msr);
    cs.add("基址默认值", p.as_ref().map(|(a, _, _)| *a == DEFAULT_BASE).unwrap_or(false), "0xFEE00000");
    cs.add("xAPIC 模", p.as_ref().map(|(_, _, m)| *m == ApicMode::XApic).unwrap_or(false), "位 10 清");
    let msr2 = build_msr(0xFEE0_0C00, true, true);
    let p2 = parse_apic_base(msr2);
    cs.add("x2APIC 模", p2.as_ref().map(|(_, _, m)| *m == ApicMode::X2Apic).unwrap_or(false), "位 10 置");
    // 族二：MSR 违例。
    cs.add("非 4K 对齐拒", parse_apic_base(build_msr(0xFEE0_0123, true, false)) == Err(XC_BASE_UNMAPPED), "低位 0x123");
    cs.add("未启用拒", parse_apic_base(build_msr(DEFAULT_BASE, false, false)) == Err(XC_APIC_DISABLED), "EN 清");
    // 族三：LVR 三元组。
    let c = Lapic::xapic(0, 0x14, 5, 0x01); // P6 家族典型：版 0x14 LVT5 族 1
    cs.add("LVR 三元组", c.lvr_triple() == (0x14, 6, 0x01), "差一编码还原");
    cs.add("LVT 上界过", c.lvt_ok().is_ok(), "5 ≤ 6 上界");
    cs.add("LVT 越域拒", Lapic::xapic(0, 0x14, 6, 0x01).lvt_ok() == Err(XC_LVT_OOR), "6 > 5");
    // 族四：窗口读与偏移纪律。
    cs.add("LSR 读", c.read_reg(OFF_LSR).map(|v| v >> 24).unwrap_or(0xFF) == 0, "ID0 高位 0");
    cs.add("非 16 对齐拒", c.read_reg(0x31) == Err(XC_REG_OOR), "0x31 歪");
    cs.add("表外偏移拒", c.read_reg(0x200) == Err(XC_REG_OOR), "无此窗");
    // 族五：探测只读闸。
    let mut cp = Lapic::xapic(2, 0x10, 4, 0x06);
    cp.probing = true;
    cs.add("探测期写拒", cp.write_reg(0x80, 0) == Err(XC_WRITE_IN_PROBE), "只读纪律");
    cp.probing = false;
    cs.add("探测后写放行", cp.write_reg(0x80, 0).is_ok(), "闸开");
    // 族六：ID 规整双模。
    let cx = Lapic::xapic(3, 0x10, 4, 0x06);
    cs.add("xAPIC ID 取高 8", cx.id_canon() == Ok(3), "位 24~31");
    let bad = Lapic { id: 0x0000_00FF, ..Lapic::xapic(0, 0x10, 4, 0x06) };
    cs.add("xAPIC 低位越宽拒", bad.id_canon() == Err(XC_ID_OOR), "ID 挤在低 24");
    let c2 = Lapic::x2apic(0x0000_0102, 0x10, 4, 0x06);
    cs.add("x2APIC 全 32 位", c2.id_canon() == Ok(0x102), "全宽");
    let c0 = Lapic::x2apic(0, 0x10, 4, 0x06);
    cs.add("x2APIC 零 ID 拒", c0.id_canon() == Err(XC_ID_OOR), "0 无效");
    // 族七：双模自洽。
    cs.add("自洽 xAPIC", Lapic::xapic(1, 0x10, 4, 0x06).mode_selfcheck(build_msr(DEFAULT_BASE, true, false)).is_ok(), "两面对齐");
    cs.add("自洽 x2APIC", Lapic::x2apic(1, 0x10, 4, 0x06).mode_selfcheck(build_msr(DEFAULT_BASE, true, true)).is_ok(), "两面对齐");
    cs.add("矛盾立案", Lapic::xapic(1, 0x10, 4, 0x06).mode_selfcheck(build_msr(DEFAULT_BASE, true, true)) == Err(XC_MODE_CONFLICT), "窗面 xAPIC MSR 面 x2");
    cs.add("MSR 未启用传染拒", Lapic::xapic(1, 0x10, 4, 0x06).mode_selfcheck(build_msr(DEFAULT_BASE, false, false)) == Err(XC_APIC_DISABLED), "EN 清连带拒");
    // 族八：多核拓扑账。
    let pack = [Lapic::xapic(0, 0x14, 5, 0x01), Lapic::xapic(1, 0x14, 5, 0x01), Lapic::xapic(2, 0x14, 5, 0x01)];
    let mut got = 0u32;
    for c in pack.iter() {
        if let Ok(i) = c.id_canon() {
            got |= 1 << i;
        }
    }
    cs.add("三核 ID 各就位", got == 0b111, "位 0/1/2");
    cs.add("版本一致", pack.iter().all(|c| c.version == 0x14), "同代同版");
    cs
}
