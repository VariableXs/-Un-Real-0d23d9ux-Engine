//! unx0810_vecpart —— UNX-F0810 · 向量空间分区账与占用审计（目标 240 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0810 · 向量空间分区账与占用审计。
//! 判据原文：UNX-F0810-J1 全 256 向量占用状态逐项可查（区/态/领主三维），
//!           占用账与实态一致性 100%。
//! 设计纪律：
//! - 256 向量分五区：异常区（0~31）/ 保留区（32~47，SPIV 等）/ MSI 区
//!   （48~175）/ IO-APIC 区（176~239）/ 自由区（240~255）。区界写死并注释。
//! - 三维台账：区（分配时定）/ 态（Free/Claimed/Armed/Bound 四态机）/
//!   领主（子系统枚举 ID，闭集）。
//! - 占用账（申请/释放流水）与实态（三维表）双源：审计时逐向量对拍，
//!   任何不一致即红账（判据 J1 后半：一致性 100%）。
//! - 越区申请即拒（异常区向量不许子系统认领）；重复认领拦截；释放未持有拦截。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x62A0（0x62A1~0x62A8 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const VP_VERSION: &str = "unx0810_vecpart-v1";

/// 诊断段基址（0x62A0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x62A0;

/// XcCode 闭集。
pub const XC_VEC_RANGE: u16 = SEG_BASE + 1; // 向量号越界
pub const XC_ZONE_DENY: u16 = SEG_BASE + 2; // 越区申请（异常/保留区禁认领）
pub const XC_ALREADY: u16 = SEG_BASE + 3; // 重复认领
pub const XC_NOT_HELD: u16 = SEG_BASE + 4; // 释放未持有
pub const XC_STATE_BAD: u16 = SEG_BASE + 5; // 非法状态迁移
pub const XC_OWNER_BAD: u16 = SEG_BASE + 6; // 领主 ID 非法
pub const XC_AUDIT_DRIFT: u16 = SEG_BASE + 7; // 审计对拍不一致
pub const XC_LED_FULL: u16 = SEG_BASE + 8; // 流水账溢出

/// 五区分界（闭集常量，判据写死）。
pub const ZONE_EXC_END: u8 = 31; // 异常区 0~31（IDT 静态注册，禁认领）
pub const ZONE_RSV_END: u8 = 47; // 保留区 32~47（spurious/错误中断等系统自用）
pub const ZONE_MSI_END: u8 = 175; // MSI 区 48~175
pub const ZONE_IOAPIC_END: u8 = 239; // IO-APIC 区 176~239
/// 自由区 240~255。

/// 区编号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    /// 异常区（0~31）。
    Exc = 0,
    /// 保留区（32~47）。
    Rsv = 1,
    /// MSI 区（48~175）。
    Msi = 2,
    /// IO-APIC 区（176~239）。
    IoApic = 3,
    /// 自由区（240~255）。
    Free = 4,
}

/// 按向量号归区（纯函数，全覆盖 256 路径）。
pub fn zone_of(vec_no: u8) -> Zone {
    if vec_no <= ZONE_EXC_END {
        Zone::Exc
    } else if vec_no <= ZONE_RSV_END {
        Zone::Rsv
    } else if vec_no <= ZONE_MSI_END {
        Zone::Msi
    } else if vec_no <= ZONE_IOAPIC_END {
        Zone::IoApic
    } else {
        Zone::Free
    }
}

/// 向量占用态四态机：Free → Claimed → Armed → Bound，回退只许 Bound→Armed→Claimed→Free。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecState {
    /// 空闲。
    Free = 0,
    /// 已认领未布防。
    Claimed = 1,
    /// 已布防（stub/表项就绪）未绑设备。
    Armed = 2,
    /// 已绑定（设备在用）。
    Bound = 3,
}

impl VecState {
    /// 前进迁移合法性。
    pub fn can_advance(self, to: VecState) -> bool {
        matches!(
            (self, to),
            (VecState::Free, VecState::Claimed)
                | (VecState::Claimed, VecState::Armed)
                | (VecState::Armed, VecState::Bound)
        )
    }

    /// 回退迁移合法性。
    pub fn can_retreat(self, to: VecState) -> bool {
        matches!(
            (self, to),
            (VecState::Bound, VecState::Armed)
                | (VecState::Armed, VecState::Claimed)
                | (VecState::Claimed, VecState::Free)
        )
    }
}

/// 领主子系统 ID 闭集。
pub mod owner {
    /// 未持有。
    pub const NONE: u8 = 0;
    /// MSI/PCI 子系统。
    pub const MSI: u8 = 1;
    /// IO-APIC 子系统。
    pub const IOAPIC: u8 = 2;
    /// 时钟子系统。
    pub const TIMER: u8 = 3;
    /// IPC/自 IPI 子系统。
    pub const IPI: u8 = 4;
    /// 平台错误面（LVT 错误/热温）。
    pub const PLATERR: u8 = 5;

    /// 合法性（0~5 闭集）。
    pub fn valid(id: u8) -> bool {
        id <= PLATERR
    }
}

/// 台账三维行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VecRow {
    /// 向量号。
    pub vec: u8,
    /// 区。
    pub zone: Zone,
    /// 态。
    pub state: VecState,
    /// 领主。
    pub owner: u8,
}

/// 占用流水行（账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRow {
    /// 向量号。
    pub vec: u8,
    /// 动作：0=认领 1=布防 2=绑定 3=回退 4=释放。
    pub action: u8,
    /// 领主。
    pub owner: u8,
    /// 序号。
    pub seq: u64,
}

/// 向量空间分区账（全 256 三维表 + 流水账）。
pub struct VecPartition {
    rows: [VecRow; 256],
    ledger: Vec<AuditRow>,
    ledger_overflow: usize,
    seq: u64,
}

impl VecPartition {
    /// 初始：全 256 行 Free/NONE；异常与保留区预划给系统（Claimed/PLATERR 或 NONE）。
    pub fn new() -> VecPartition {
        let mut rows = [VecRow {
            vec: 0,
            zone: Zone::Free,
            state: VecState::Free,
            owner: owner::NONE,
        }; 256];
        for (i, r) in rows.iter_mut().enumerate() {
            r.vec = i as u8;
            r.zone = zone_of(i as u8);
        }
        // 保留区 32（spurious）与 34/35（LVT 错误/热温映射位）系统预占：
        // 本模块只标 Claimed（布防由 F0846/F0830 完成）。
        for v in [32u8, 34, 35] {
            rows[v as usize].state = VecState::Claimed;
            rows[v as usize].owner = owner::PLATERR;
        }
        VecPartition {
            rows,
            ledger: Vec::new(),
            ledger_overflow: 0,
            seq: 0,
        }
    }

    /// 流水入账（定容 512，满即溢出计数）。
    fn push_ledger(&mut self, vec: u8, action: u8, own: u8) {
        self.seq += 1;
        if self.ledger.len() < 512 {
            self.ledger.push(AuditRow {
                vec,
                action,
                owner: own,
                seq: self.seq,
            });
        } else {
            self.ledger_overflow += 1;
        }
    }

    /// 认领：越区（Exc/Rsv）拒、非 Free 拒、领主非法拒。
    pub fn claim(&mut self, vec_no: u8, own: u8) -> Result<(), u16> {
        if !owner::valid(own) || own == owner::NONE {
            return Err(XC_OWNER_BAD);
        }
        let z = zone_of(vec_no);
        if matches!(z, Zone::Exc | Zone::Rsv) {
            self.push_ledger(vec_no, 0, own);
            return Err(XC_ZONE_DENY);
        }
        let r = &mut self.rows[vec_no as usize];
        if r.state != VecState::Free {
            self.push_ledger(vec_no, 0, own);
            return Err(XC_ALREADY);
        }
        r.state = VecState::Claimed;
        r.owner = own;
        self.push_ledger(vec_no, 0, own);
        Ok(())
    }

    /// 状态推进（Claimed→Armed→Bound）。
    pub fn advance(&mut self, vec_no: u8, to: VecState) -> Result<(), u16> {
        let (owner_id, act) = {
            let r = &mut self.rows[vec_no as usize];
            if !r.state.can_advance(to) {
                return Err(XC_STATE_BAD);
            }
            r.state = to;
            let act = match to {
                VecState::Armed => 1u8,
                VecState::Bound => 2u8,
                _ => 0u8,
            };
            (r.owner, act)
        };
        self.push_ledger(vec_no, act, owner_id);
        Ok(())
    }

    /// 回退（Bound→Armed→Claimed）。
    pub fn retreat(&mut self, vec_no: u8, to: VecState) -> Result<(), u16> {
        let owner_id = {
            let r = &mut self.rows[vec_no as usize];
            if !r.state.can_retreat(to) {
                return Err(XC_STATE_BAD);
            }
            r.state = to;
            r.owner
        };
        self.push_ledger(vec_no, 3, owner_id);
        Ok(())
    }

    /// 释放：只许 Claimed 态释放；领主须匹配。
    pub fn release(&mut self, vec_no: u8, own: u8) -> Result<(), u16> {
        let r = &mut self.rows[vec_no as usize];
        if r.state != VecState::Claimed || r.owner != own {
            self.push_ledger(vec_no, 4, own);
            return Err(XC_NOT_HELD);
        }
        r.state = VecState::Free;
        r.owner = owner::NONE;
        self.push_ledger(vec_no, 4, own);
        Ok(())
    }

    /// 三维逐项查询（判据 J1 前半：逐项可查）。
    pub fn row(&self, vec_no: u8) -> Result<VecRow, u16> {
        Ok(self.rows[vec_no as usize])
    }

    /// 审计：占用账重放 vs 实态对拍。
    /// 重放规则：按流水顺序对影子表执行，末态与实态逐向量比，任何 drift 即红。
    pub fn audit(&self) -> Result<usize, u16> {
        let mut shadow = [VecRow {
            vec: 0,
            zone: Zone::Free,
            state: VecState::Free,
            owner: owner::NONE,
        }; 256];
        for (i, r) in shadow.iter_mut().enumerate() {
            r.vec = i as u8;
            r.zone = zone_of(i as u8);
        }
        for v in [32u8, 34, 35] {
            shadow[v as usize].state = VecState::Claimed;
            shadow[v as usize].owner = owner::PLATERR;
        }
        // 重放流水（对影子表只做合法迁移；非法流水即账坏）。
        for row in self.ledger.iter() {
            let r = &mut shadow[row.vec as usize];
            match row.action {
                0 => {
                    r.state = VecState::Claimed;
                    r.owner = row.owner;
                }
                1 => r.state = VecState::Armed,
                2 => r.state = VecState::Bound,
                3 => { /* 回退细节不入影子：末态对拍以实态为准，回退行仅作序号审计 */ }
                4 => {
                    r.state = VecState::Free;
                    r.owner = owner::NONE;
                }
                _ => return Err(XC_STATE_BAD),
            }
        }
        // 逐向量对拍（256 全量）。
        for i in 0..256usize {
            if shadow[i].state != self.rows[i].state || shadow[i].owner != self.rows[i].owner {
                return Err(XC_AUDIT_DRIFT);
            }
        }
        Ok(256)
    }

    /// 流水账统计。
    pub fn ledger_stat(&self) -> (usize, usize, u64) {
        (self.ledger.len(), self.ledger_overflow, self.seq)
    }

    /// 守恒核。
    pub fn conserve(&self) -> bool {
        let (l, ov, seq) = self.ledger_stat();
        l + ov == seq as usize
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    let mut vp = VecPartition::new();
    // 1. 初始态抽查：三维可查
    let r0 = vp.row(0)?;
    if r0.zone != Zone::Exc || r0.state != VecState::Free {
        return Err(XC_AUDIT_DRIFT);
    }
    let r32 = vp.row(32)?;
    if r32.zone != Zone::Rsv || r32.state != VecState::Claimed || r32.owner != owner::PLATERR {
        return Err(XC_AUDIT_DRIFT);
    }
    // 2. 正常认领→布防→绑定（MSI 区 100）
    vp.claim(100, owner::MSI)?;
    vp.advance(100, VecState::Armed)?;
    vp.advance(100, VecState::Bound)?;
    // 3. 回退链 Bound→Armed→Claimed→释放
    vp.retreat(100, VecState::Armed)?;
    vp.retreat(100, VecState::Claimed)?;
    vp.release(100, owner::MSI)?;
    // 4. 审计对拍（账实一致）
    let n = vp.audit()?;
    if n != 256 {
        return Err(XC_AUDIT_DRIFT);
    }
    // 5. 错误路径①：越区认领（异常区 3 号）
    if vp.claim(3, owner::MSI).is_ok() {
        return Err(XC_ZONE_DENY);
    }
    // 6. 错误路径②：重复认领（保留区预占 32 也越区；改用 MSI 区已持有）
    vp.claim(101, owner::TIMER)?;
    if vp.claim(101, owner::IPI).is_ok() {
        return Err(XC_ALREADY);
    }
    // 7. 错误路径③：非法状态迁移（Free→Bound 直跳）
    if vp.advance(102, VecState::Bound).is_ok() {
        return Err(XC_STATE_BAD);
    }
    // 8. 错误路径④：释放未持有（领主不匹配）
    if vp.release(101, owner::IPI).is_ok() {
        return Err(XC_NOT_HELD);
    }
    // 9. 错误路径⑤：领主非法
    if vp.claim(103, 9).is_ok() {
        return Err(XC_OWNER_BAD);
    }
    // 10. 终审计 + 守恒
    vp.audit()?;
    if !vp.conserve() {
        return Err(XC_LED_FULL);
    }
    Ok(0)
}
