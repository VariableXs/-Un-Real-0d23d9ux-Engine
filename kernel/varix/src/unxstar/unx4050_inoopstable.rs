//! unx4050_inoopstable — UNX-F4050 inode_operations 表锚（B08 展开前置）（诊断段 0x88A0）。
//!
//! 判据原文：
//!   J1 i_op 挂接/替换/空表三类场景行为账全过；
//!   J2 空表调用统一返回 ENOSYS 且不空指针崩溃。
//! 设计纪律：
//!   1. 操作表模型：i_op 以表位（挂接/未挂接）+ 表内五槽
//!      （create/mkdir/mknod/rename/getattr）建模；空表调用
//!      统一走 ENOSYS 通道（判据 J2），不空指针解引。
//!   2. 表状态机：NoTable（空表）→ TableSet（挂接）→ TableSet
//!      （替换：旧表序号入替换账）；卸表回 NoTable。
//!   3. 槽位闭集：五槽各带实现位，未实现槽位调用同样走
//!      ENOSYS——表挂接但槽空 = 空表语义的槽级推广。
//!   4. 行为账：挂接/替换/卸表/调用四类事件逐笔记账；调用账
//!      = 分派数 = ENOSYS 数 + 槽实现数（守恒）。
//!   5. 边界：重复挂接（须先卸）、卸空表、闭集外槽位、空表期
//!      调用——全部显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x88A0;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_NO_TABLE: u16 = SEG_BASE + 1;   // 空表调用/卸表（ENOSYS 语义）
const E_SLOT_NOSYS: u16 = SEG_BASE + 2; // 槽未实现（ENOSYS 语义）
const E_ALREADY_SET: u16 = SEG_BASE + 3; // 已挂接再挂接（须先卸）
const E_BAD_SLOT: u16 = SEG_BASE + 4;   // 闭集外槽位
const E_DISPATCH_MISS: u16 = SEG_BASE + 5; // 分派账与调用账不平
const E_BAL: u16 = SEG_BASE + 6;        // 表事件账不平

/// 操作表五槽闭集。
pub const SLOT_CREATE: u8 = 1;  // create 槽
pub const SLOT_MKDIR: u8 = 2;   // mkdir 槽
pub const SLOT_MKNOD: u8 = 3;   // mknod 槽
pub const SLOT_RENAME: u8 = 4;  // rename 槽
pub const SLOT_GETATTR: u8 = 5; // getattr 槽
pub const SLOT_COUNT: usize = 5;

/// 操作表状态机：NoTable ↔ TableSet（替换走 TableSet 内账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TablePhase {
    NoTable,  // 空表（i_op 未挂接）
    TableSet, // 已挂接（五槽实现位有效）
}

/// 单张 inode_operations 表：五槽实现位。
pub struct InoOpsTable {
    pub table_id: u32,        // 表序号（替换账用）
    pub impl_bits: [bool; SLOT_COUNT], // 槽实现位
}

/// inode_operations 表锚。
pub struct InoOpsAnchor {
    pub phase: TablePhase,
    pub current: Option<InoOpsTable>, // 当前表
    pub attach_total: u32,  // 挂接事件数
    pub replace_total: u32, // 替换事件数
    pub detach_total: u32,  // 卸表事件数
    pub dispatch_total: u32,// 调用分派总数
    pub nosys_total: u32,   // ENOSYS 返回数（空表 + 空槽）
    pub slot_ok_total: u32, // 槽实现命中数
    pub rejects: u32,       // 错误账
}

impl InoOpsAnchor {
    pub fn new() -> InoOpsAnchor {
        InoOpsAnchor {
            phase: TablePhase::NoTable,
            current: None,
            attach_total: 0,
            replace_total: 0,
            detach_total: 0,
            dispatch_total: 0,
            nosys_total: 0,
            slot_ok_total: 0,
            rejects: 0,
        }
    }

    /// 槽位闭集映射到下标。
    fn slot_idx(slot: u8) -> Option<usize> {
        match slot {
            SLOT_CREATE => Some(0),
            SLOT_MKDIR => Some(1),
            SLOT_MKNOD => Some(2),
            SLOT_RENAME => Some(3),
            SLOT_GETATTR => Some(4),
            _ => None,
        }
    }

    /// 挂接（判据 J1 场景一）：NoTable → TableSet；重复挂接拒收。
    pub fn attach(&mut self, table_id: u32, impl_bits: [bool; SLOT_COUNT]) -> Result<TablePhase, u16> {
        if self.phase == TablePhase::TableSet {
            self.rejects += 1;
            return Err(E_ALREADY_SET);
        }
        self.current = Some(InoOpsTable { table_id, impl_bits });
        self.phase = TablePhase::TableSet;
        self.attach_total += 1;
        Ok(self.phase)
    }

    /// 替换（判据 J1 场景二）：TableSet 内换表，旧表序号入替换账。
    pub fn replace(&mut self, table_id: u32, impl_bits: [bool; SLOT_COUNT]) -> Result<(u32, u32), u16> {
        if self.phase != TablePhase::TableSet {
            self.rejects += 1;
            return Err(E_NO_TABLE);
        }
        let old_id = match self.current {
            Some(ref t) => t.table_id,
            None => {
                self.rejects += 1;
                return Err(E_BAL);
            }
        };
        self.current = Some(InoOpsTable { table_id, impl_bits });
        self.replace_total += 1;
        Ok((old_id, table_id))
    }

    /// 卸表（判据 J1 场景前置）：TableSet → NoTable；卸空表拒收。
    pub fn detach(&mut self) -> Result<TablePhase, u16> {
        if self.phase != TablePhase::TableSet {
            self.rejects += 1;
            return Err(E_NO_TABLE);
        }
        self.current = None;
        self.phase = TablePhase::NoTable;
        self.detach_total += 1;
        Ok(self.phase)
    }

    /// 分派调用（判据 J2 主通道）：空表统一 E_NO_TABLE（ENOSYS），
    /// 挂接但槽未实现统一 E_SLOT_NOSYS（ENOSYS），命中则走实现。
    pub fn dispatch(&mut self, slot: u8) -> Result<(), u16> {
        let si = match Self::slot_idx(slot) {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(E_BAD_SLOT);
            }
        };
        // 闭集内调用才入分派账（守恒：分派 = ENOSYS + 槽命中）。
        self.dispatch_total += 1;
        match self.phase {
            TablePhase::NoTable => {
                // 判据 J2：空表统一 ENOSYS，不空指针崩溃。
                self.nosys_total += 1;
                Err(E_NO_TABLE)
            }
            TablePhase::TableSet => {
                let t = self.current.as_ref();
                let t = match t {
                    Some(t) => t,
                    None => {
                        // 状态与指针账不一致（防御分支，不崩）。
                        self.nosys_total += 1;
                        return Err(E_NO_TABLE);
                    }
                };
                if si >= t.impl_bits.len() {
                    self.nosys_total += 1;
                    return Err(E_NO_TABLE);
                }
                if t.impl_bits[si] {
                    self.slot_ok_total += 1;
                    Ok(())
                } else {
                    self.nosys_total += 1;
                    Err(E_SLOT_NOSYS)
                }
            }
        }
    }

    /// 三类场景行为账核验（判据 J1）：挂接/替换/空表逐段对账。
    pub fn scenario_audit(&self) -> Result<(), u16> {
        // 守恒式：分派 = ENOSYS + 槽命中。
        if self.dispatch_total != self.nosys_total + self.slot_ok_total {
            return Err(E_DISPATCH_MISS);
        }
        // 表事件账闭合：NoTable 终态时挂接 - 卸表 = 0（或持表态 +1）。
        let held = if self.phase == TablePhase::TableSet { 1u32 } else { 0 };
        if self.attach_total != self.detach_total + held {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 守恒对账。
    pub fn audit(&self) -> Result<(), u16> {
        self.scenario_audit()?;
        if self.replace_total > self.attach_total + self.replace_total {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（相位，挂接账，替换账，ENOSYS 账，槽命中账）。
    pub fn ledger(&self) -> (TablePhase, u32, u32, u32, u32) {
        (self.phase, self.attach_total, self.replace_total, self.nosys_total, self.slot_ok_total)
    }

    /// 当前表视图：返回（表序号，五槽实现位快照）。
    pub fn table_view(&self) -> Result<(u32, [bool; SLOT_COUNT]), u16> {
        match &self.current {
            Some(t) => Ok((t.table_id, t.impl_bits)),
            None => Err(E_NO_TABLE),
        }
    }

    /// 槽实现判定视图（挂接态下指定槽是否已实现）。
    pub fn slot_implemented(&self, slot: u8) -> Result<bool, u16> {
        if self.phase != TablePhase::TableSet {
            return Err(E_NO_TABLE);
        }
        let si = match Self::slot_idx(slot) {
            Some(s) => s,
            None => return Err(E_BAD_SLOT),
        };
        let t = match &self.current {
            Some(t) => t,
            None => return Err(E_NO_TABLE),
        };
        if si >= t.impl_bits.len() {
            return Err(E_NO_TABLE);
        }
        Ok(t.impl_bits[si])
    }

    /// 事件账视图：返回（挂接，替换，卸表）三账。
    pub fn event_view(&self) -> (u32, u32, u32) {
        (self.attach_total, self.replace_total, self.detach_total)
    }

    /// ENOSYS 覆盖率视图（千分比整数化）：空表/空槽统一通道占比。
    pub fn nosys_rate(&self) -> u32 {
        if self.dispatch_total == 0 {
            return 0;
        }
        self.nosys_total * 1000 / self.dispatch_total
    }
}

/// 自例程：R1 空表调用 → R2 挂接与槽分派 → R3 替换 → R4 注入拒收 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut an = InoOpsAnchor::new();

    // R1 空表调用（判据 J2）：五槽统一 ENOSYS，零崩溃，账同步。
    if an.dispatch(SLOT_CREATE) != Err(E_NO_TABLE) {
        return Err(E_BAL);
    }
    if an.dispatch(SLOT_GETATTR) != Err(E_NO_TABLE) {
        return Err(E_BAL);
    }
    if an.nosys_total != 2 || an.dispatch_total != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 挂接与槽分派（判据 J1 场景一）：三槽实现，两槽空。
    let bits = [true, true, true, false, false];
    if an.attach(1001, bits)? != TablePhase::TableSet {
        return Err(E_BAL);
    }
    an.dispatch(SLOT_CREATE)?; // 命中
    an.dispatch(SLOT_MKDIR)?;  // 命中
    if an.dispatch(SLOT_RENAME) != Err(E_SLOT_NOSYS) {
        return Err(E_BAL);
    }
    if an.slot_ok_total != 2 || an.nosys_total != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 替换（判据 J1 场景二）：换表后旧表序号入账，槽位随新表。
    let (old, _new) = an.replace(1002, [true, false, false, false, true])?;
    if old != 1001 {
        return Err(E_BAL);
    }
    an.dispatch(SLOT_GETATTR)?; // 新表命中
    if an.dispatch(SLOT_CREATE) != Err(E_SLOT_NOSYS) {
        // 新表 create 槽空。
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注入拒收：重复挂接、卸表后重挂前再挂、闭集外槽位、空表卸表。
    if an.attach(1003, bits) != Err(E_ALREADY_SET) {
        return Err(E_BAL);
    }
    an.detach()?;
    if an.detach() != Err(E_NO_TABLE) {
        return Err(E_BAL);
    }
    if an.dispatch(9) != Err(E_BAD_SLOT) {
        return Err(E_BAL);
    }
    if an.replace(1004, bits) != Err(E_NO_TABLE) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒对账：分派 = ENOSYS + 命中；表事件账闭合。
    an.attach(1005, bits)?;
    an.audit()?;
    let (ph, at, rt, nosys, ok) = an.ledger();
    if ph != TablePhase::TableSet || at != 2 || rt != 1 || nosys != 4 || ok != 3 {
        // 分派总数 7 = ENOSYS 4（空表 2 + 空槽 2）+ 槽命中 3。
        return Err(E_BAL);
    }
    // 表视图与槽实现视图：当前表 1005，create 命中 rename 空。
    let (tid, bits5) = an.table_view()?;
    if tid != 1005 || bits5[0] != true || bits5[3] != false {
        return Err(E_BAL);
    }
    if !an.slot_implemented(SLOT_CREATE)? || an.slot_implemented(SLOT_RENAME)? {
        return Err(E_BAL);
    }
    // 事件账与 ENOSYS 覆盖率：分派 7 笔中 ENOSYS 4 → 571‰。
    let (at2, rt2, dt2) = an.event_view();
    if at2 != 2 || rt2 != 1 || dt2 != 1 {
        return Err(E_BAL);
    }
    if an.nosys_rate() != 4 * 1000 / 7 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
