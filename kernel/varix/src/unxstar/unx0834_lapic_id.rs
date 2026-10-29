//! unx0834_lapic_id — UNX-F0834 LAPIC ID 寄存器与 x2APIC 全宽 ID 对账（诊断段 0x6560）。
//!
//! 判据原文：ID 读取三源一致（寄存器/初始 LAPIC ID/F0004 拓扑账），
//!           x2APIC 32 位 ID 全宽读回正确。
//! 设计纪律：
//!   1. xAPIC ID 寄存器（0x020）只有 [31:24] 是 APIC ID，其余位读零
//!      （SDM Vol.3 §10.4.6/§10.9）；初始 LAPIC ID 取自 CPUID.01H:EBX[31:24]；
//!      F0004 拓扑账为固件枚举的第三源。三源全等才出绿账。
//!   2. x2APIC 迁移（SDM §10.12）：ID 寄存器变 32 位全宽读回，迁移时
//!      低 8 位保留原 xAPIC ID、高 24 位清零起步；写回只许整 32 位。
//!   3. 写入口约束：xAPIC 档写 [23:0] 静默丢弃（只 [31:24] 可驻留），
//!      越界源（> 0xFF）在 xAPIC 档为非法。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x6560 本单独占。

pub const SEG_BASE: u16 = 0x6560;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TRISOURCE: u16 = SEG_BASE + 1; // 三源不一致
const E_XAPIC_RESV: u16 = SEG_BASE + 2;// xAPIC 档 [23:0] 写入残留（应静默归零）
const E_ID_RANGE: u16 = SEG_BASE + 3;  // ID 超出 8 位域（xAPIC 档）
const E_X2_WIDE: u16 = SEG_BASE + 4;   // x2APIC 全宽读回不符
const E_MIGRATE: u16 = SEG_BASE + 5;   // 迁移序列低 8 位未保留/高位未清
const E_TOPO: u16 = SEG_BASE + 6;      // F0004 拓扑账无此核（账目缺失）

/// ID 三源账：寄存器读出 / 初始 LAPIC ID / F0004 拓扑账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdTriSource {
    pub reg_id: u8,     // LAPIC ID 寄存器 [31:24]
    pub initial_id: u8, // CPUID.01H:EBX[31:24]
    pub topo_id: u8,    // F0004 拓扑账登记
}

impl IdTriSource {
    /// 三源全等校验（判据核心）。
    pub fn consistent(&self) -> Result<(), u16> {
        if self.reg_id == self.initial_id && self.initial_id == self.topo_id {
            Ok(())
        } else {
            Err(E_TRISOURCE)
        }
    }
}

/// F0004 拓扑账（固件枚举闭集，本模型登记 16 核）。
pub struct TopoLedger {
    pub entries: [u8; 16],
    pub count: u8,
}

impl TopoLedger {
    pub fn empty() -> TopoLedger {
        TopoLedger {
            entries: [0u8; 16],
            count: 0,
        }
    }

    pub fn add(&mut self, id: u8) -> Result<(), u16> {
        if self.count as usize >= self.entries.len() {
            return Err(E_TOPO);
        }
        for i in 0..self.count as usize {
            if self.entries[i] == id {
                return Err(E_TOPO); // 拓扑账 ID 唯一性
            }
        }
        self.entries[self.count as usize] = id;
        self.count += 1;
        Ok(())
    }

    pub fn lookup(&self, id: u8) -> Result<u8, u16> {
        for i in 0..self.count as usize {
            if self.entries[i] == id {
                return Ok(self.entries[i]);
            }
        }
        Err(E_TOPO)
    }
}

/// LAPIC ID 引擎：xAPIC/x2APIC 双模式寄存器面。
pub struct LapicId {
    x2apic: bool,
    reg: u32, // xAPIC：[31:24] 有效；x2APIC：全 32 位
}

impl LapicId {
    pub fn new(initial_id: u8) -> LapicId {
        LapicId {
            x2apic: false,
            reg: (initial_id as u32) << 24,
        }
    }

    pub fn in_x2apic(&self) -> bool {
        self.x2apic
    }

    /// xAPIC 档读：只有 [31:24]，[23:0] 恒零。
    pub fn read_xapic(&self) -> Result<u8, u16> {
        if self.x2apic {
            // x2APIC 态下读 xAPIC 布局口为体系结构越界，闭集拒
            return Err(E_X2_WIDE);
        }
        Ok((self.reg >> 24) as u8)
    }

    /// xAPIC 档写：值 > 0xFF 非法；[23:0] 段静默丢弃（写后读零）。
    pub fn write_xapic(&mut self, v: u32) -> Result<(), u16> {
        if self.x2apic {
            return Err(E_X2_WIDE);
        }
        if v > 0xFF {
            return Err(E_ID_RANGE);
        }
        self.reg = v << 24;
        Ok(())
    }

    /// x2APIC 迁移：低 8 位保留现行 xAPIC ID，高 24 位清零起步。
    pub fn migrate_to_x2apic(&mut self) -> Result<(), u16> {
        if self.x2apic {
            return Err(E_MIGRATE); // 重复迁移
        }
        let cur = (self.reg >> 24) as u8;
        self.x2apic = true;
        self.reg = cur as u32;
        Ok(())
    }

    /// x2APIC 全宽读（判据「32 位 ID 全宽读回」）。
    pub fn read_x2apic(&self) -> Result<u32, u16> {
        if !self.x2apic {
            return Err(E_X2_WIDE);
        }
        Ok(self.reg)
    }

    /// x2APIC 全宽写（软件仅许在 x2APIC 态整 32 位改写 ID）。
    pub fn write_x2apic(&mut self, v: u32) -> Result<(), u16> {
        if !self.x2apic {
            return Err(E_X2_WIDE);
        }
        self.reg = v;
        Ok(())
    }

    /// 迁移语义复核：迁移后全宽值低 8 位 == 迁移前 xAPIC ID，高 24 位为零。
    pub fn migration_preserved(&self, before_id: u8) -> Result<(), u16> {
        let w = self.read_x2apic()?;
        if (w & 0xFF) as u8 != before_id || w >> 8 != 0 {
            return Err(E_MIGRATE);
        }
        Ok(())
    }

    /// 三源对账总入口：寄存器读出 × 初始 ID × 拓扑账 lookup 三方合账。
    pub fn reconcile(&self, initial_id: u8, topo: &TopoLedger) -> Result<(), u16> {
        let reg_id = if self.x2apic {
            // x2APIC 档对账仍按低 8 位可比域（拓扑账为 8 位域闭集）
            (self.read_x2apic()? & 0xFF) as u8
        } else {
            self.read_xapic()?
        };
        topo.lookup(initial_id)?;
        let t = IdTriSource {
            reg_id,
            initial_id,
            topo_id: topo.lookup(initial_id)?,
        };
        t.consistent()
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_TRISOURCE => "三源不一致：寄存器/初始 LAPIC ID/F0004 拓扑账失合",
        E_XAPIC_RESV => "xAPIC 档 [23:0] 段写入残留（应静默归零）",
        E_ID_RANGE => "ID 值超出 8 位域（xAPIC 档）",
        E_X2_WIDE => "x2APIC/xAPIC 模式口错配",
        E_MIGRATE => "迁移序列破坏：低 8 位未保留或高位未清",
        E_TOPO => "F0004 拓扑账无此核或账满/重账",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三源一致 10/10 + x2APIC 全宽读回 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：16 核拓扑账登记（唯一性守恒）。
    let mut topo = TopoLedger::empty();
    for id in 0u8..16 {
        topo.add(id)?;
    }
    if topo.count != 16 {
        return Err(E_TOPO);
    }
    hits += 1;

    // R2：三源一致 10/10（判据原文）——每核建引擎对账全绿。
    for id in 0u8..10 {
        let mut lap = LapicId::new(id);
        if lap.read_xapic()? != id {
            return Err(E_TRISOURCE);
        }
        lap.reconcile(id, &topo)?;
    }
    hits += 1;

    // R3：写回改 ID 后三源账同步再合（寄存器源更新语义）。
    let mut lap = LapicId::new(5);
    lap.write_xapic(9)?;
    if lap.read_xapic()? != 9 {
        return Err(E_XAPIC_RESV);
    }
    // 改写后对账必须失败（寄存器源偏离初始/拓扑源）——负样本先行。
    if lap.reconcile(5, &topo) != Err(E_TRISOURCE) {
        return Err(E_TRISOURCE);
    }
    hits += 1;

    // R4：x2APIC 迁移：低 8 位保留、高 24 位清零、全宽读回（判据后半）。
    let mut lap2 = LapicId::new(7);
    lap2.migrate_to_x2apic()?;
    if !lap2.in_x2apic() {
        return Err(E_MIGRATE);
    }
    lap2.migration_preserved(7)?;
    let w = lap2.read_x2apic()?;
    if w != 7 {
        return Err(E_X2_WIDE);
    }
    // 全宽写：32 位 ID 直入直出。
    lap2.write_x2apic(0x0001_0203)?;
    if lap2.read_x2apic()? != 0x0001_0203 {
        return Err(E_X2_WIDE);
    }
    hits += 1;

    // R5：x2APIC 档三源对账仍按低 8 位可比域合账。
    let mut lap3 = LapicId::new(3);
    lap3.migrate_to_x2apic()?;
    lap3.reconcile(3, &topo)?;
    hits += 1;

    // I1：xAPIC 档越界写 → E_ID_RANGE。
    let mut lap4 = LapicId::new(1);
    if lap4.write_xapic(0x100) != Err(E_ID_RANGE) {
        return Err(E_ID_RANGE);
    }
    hits += 1;

    // I2：xAPIC 口读 x2APIC 态 / x2APIC 口读 xAPIC 态 → E_X2_WIDE。
    if lap4.read_x2apic() != Err(E_X2_WIDE) {
        return Err(E_X2_WIDE);
    }
    let mut lap5 = LapicId::new(2);
    lap5.migrate_to_x2apic()?;
    if lap5.read_xapic() != Err(E_X2_WIDE) {
        return Err(E_X2_WIDE);
    }
    if lap5.write_xapic(1) != Err(E_X2_WIDE) {
        return Err(E_X2_WIDE);
    }
    hits += 1;

    // I3：重复迁移 → E_MIGRATE。
    if lap5.migrate_to_x2apic() != Err(E_MIGRATE) {
        return Err(E_MIGRATE);
    }
    hits += 1;

    // I4：迁移语义破坏注入——高位污染 → E_MIGRATE。
    let mut lap6 = LapicId::new(4);
    lap6.migrate_to_x2apic()?;
    lap6.write_x2apic(0x00AA_0004)?; // 模拟高位未清
    if lap6.migration_preserved(4) != Err(E_MIGRATE) {
        return Err(E_MIGRATE);
    }
    hits += 1;

    // I5：拓扑账缺失核 → E_TOPO。
    if lap4.reconcile(200, &topo) != Err(E_TOPO) {
        return Err(E_TOPO);
    }
    // 拓扑账重账 → E_TOPO。
    if topo.add(3) != Err(E_TOPO) {
        return Err(E_TOPO);
    }
    hits += 1;

    Ok(hits)
}
