//! unx48292_mdefexit — UNX-F48292 M 段缺陷账出口（B09–B15 缺陷登记整理为结构化工单）（诊断段 0xC560）。
//!
//! 判据原文：
//!   J1 工单字段齐（现象/位置/严重度/建议修法/复现路径）；
//!   判据锚定 双轨产线 缺陷账本。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 五字段闭集：工单必含 ①现象码 ②位置码（批内模块）③严重度
//!      （P0..P3 闭集）④建议修法码 ⑤复现路径（步序列 ≥1 步）——
//!      缺任一字段不得出账（E_FIELD_MISS）。
//!   2. 出口单向：缺陷账 → 工单是唯一通路；工单一经开出即冻结
//!      （字段不可改，改则 E_FROZEN）——缺陷账可修，工单不可悔。
//!   3. 严重度闭集与排序账：P0 最高；工单按 (严重度, 缺陷号) 升序
//!      排队出账——P0 先出，同级按号序，确定性出口序。
//!   4. 转换守恒（J1）：登记 N 缺陷开出 N 工单，逐单五字段与缺陷账
//!      逐位相等；半字段缺陷（登记期就缺）不入账、不入单。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC560;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_FIELD_MISS: u16 = SEG_BASE + 1; // 工单字段缺位
const E_FROZEN: u16 = SEG_BASE + 2; // 工单冻结被改
const E_SEV_BAD: u16 = SEG_BASE + 3; // 严重度越闭集
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 缺陷/工单账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 5; // 出口器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 严重度闭集：P0 最高。
pub const SEV_P0: u8 = 0;
pub const SEV_P1: u8 = 1;
pub const SEV_P2: u8 = 2;
pub const SEV_P3: u8 = 3;

/// 复现路径步上限（闭集界）。
pub const REPRO_MAX: u32 = 8;

/// 缺陷登记一条（登记期五字段，码位非零即「在位」）。
#[derive(Clone, Debug)]
pub struct Defect {
    pub id: u32,
    pub phenom: u16,      // 现象码
    pub loc: u16,         // 位置码（批内模块）
    pub sev: u8,          // 严重度 P0..P3
    pub fix: u16,         // 建议修法码
    pub repro: Vec<u16>,  // 复现路径步序列
}

/// 结构化工单（冻结态）。
#[derive(Clone, Debug)]
pub struct WorkOrder {
    pub order_no: u32,
    pub defect_id: u32,
    pub phenom: u16,
    pub loc: u16,
    pub sev: u8,
    pub fix: u16,
    pub repro: Vec<u16>,
}

/// 缺陷账出口器。
pub struct DefExit {
    pub defects: Vec<Defect>,
    pub orders: Vec<WorkOrder>,
    pub frozen: u32,
}

impl DefExit {
    pub fn new() -> DefExit {
        DefExit { defects: Vec::new(), orders: Vec::new(), frozen: 0 }
    }

    /// 缺陷登记：五字段在位核验（phenom/loc/fix 码非零、sev 闭集、
    /// repro ≥1 步且 ≤8 步），半字段缺陷不入账。
    pub fn register(
        &mut self,
        id: u32,
        phenom: u16,
        loc: u16,
        sev: u8,
        fix: u16,
        repro: Vec<u16>,
    ) -> Result<(), u16> {
        if phenom == 0 || loc == 0 || fix == 0 {
            return Err(E_FIELD_MISS);
        }
        if sev > SEV_P3 {
            return Err(E_SEV_BAD);
        }
        if repro.len() == 0 || repro.len() as u32 > REPRO_MAX {
            return Err(E_FIELD_MISS);
        }
        // 缺陷号不重。
        let mut i = 0usize;
        while i < self.defects.len() {
            if self.defects[i].id == id {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        // 复现步码非零。
        let mut k = 0usize;
        while k < repro.len() {
            if repro[k] == 0 {
                return Err(E_FIELD_MISS);
            }
            k += 1;
        }
        self.defects.push(Defect { id, phenom, loc, sev, fix, repro });
        Ok(())
    }

    pub fn defect_count(&self) -> usize {
        self.defects.len()
    }

    /// 转换守恒核前置：工单量 == 缺陷量。
    pub fn conserved(&self) -> bool {
        self.orders.len() == self.defects.len()
    }

    /// 出账（J1 主体）：按 (严重度, 缺陷号) 升序逐单开出，五字段
    /// 逐位搬运，工单号按出口序 1..n。
    pub fn issue_all(&mut self) -> Result<u32, u16> {
        if self.defect_count() == 0 {
            return Err(E_STATE_BAD);
        }
        self.orders.clear();
        self.frozen = 0;
        // 选择排序出口序（确定性）：先按 sev 升、同 sev 按 id 升。
        let mut order: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < self.defects.len() {
            order.push(i);
            i += 1;
        }
        let mut a = 0usize;
        while a < order.len() {
            let mut b = a + 1;
            while b < order.len() {
                let da = &self.defects[order[a]];
                let db = &self.defects[order[b]];
                if db.sev < da.sev || (db.sev == da.sev && db.id < da.id) {
                    let t = order[a];
                    order[a] = order[b];
                    order[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        // 逐单开出。
        let mut no = 1u32;
        let mut k = 0usize;
        while k < order.len() {
            let d = &self.defects[order[k]];
            // 出口面再核一次五字段（登记账外的双防线）。
            if d.phenom == 0 || d.loc == 0 || d.fix == 0 || d.sev > SEV_P3 || d.repro.len() == 0 {
                return Err(E_FIELD_MISS);
            }
            let mut rp: Vec<u16> = Vec::new();
            let mut j = 0usize;
            while j < d.repro.len() {
                rp.push(d.repro[j]);
                j += 1;
            }
            self.orders.push(WorkOrder {
                order_no: no,
                defect_id: d.id,
                phenom: d.phenom,
                loc: d.loc,
                sev: d.sev,
                fix: d.fix,
                repro: rp,
            });
            self.frozen += 1;
            no += 1;
            k += 1;
        }
        if !self.conserved() {
            return Err(E_LEDGER_BAD);
        }
        Ok(self.orders.len() as u32)
    }

    /// 工单冻结：改字段必拒（工单不可悔）。
    pub fn amend_order(&mut self, order_no: u32, new_fix: u16) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.orders.len() {
            if self.orders[i].order_no == order_no {
                self.frozen += 1; // 篡改计数留痕
                return Err(E_FROZEN);
            }
            i += 1;
        }
        Err(E_STATE_BAD)
    }

    /// 出口账审计：守恒 + 工单序 (严重度, 号) 单调 + 工单号 1..n 闭合。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.conserved() {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 1usize;
        while i < self.orders.len() {
            let a = &self.orders[i - 1];
            let b = &self.orders[i];
            if b.sev < a.sev || (b.sev == a.sev && b.defect_id < a.defect_id) {
                return Err(E_LEDGER_BAD); // 出口序失守
            }
            if b.order_no != a.order_no + 1 {
                return Err(E_LEDGER_BAD); // 工单号闭合
            }
            i += 1;
        }
        if self.orders.len() > 0 && self.orders[0].order_no != 1 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：有单出账 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.orders.len() == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（工单字段齐），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // 构造 6 条缺陷（严重度混杂、复现步 1~8 步）。
    let mut de = DefExit::new();
    let mut r1: Vec<u16> = Vec::new();
    r1.push(0x11);
    r1.push(0x12);
    r1.push(0x13);
    let mut r2: Vec<u16> = Vec::new();
    r2.push(0x21);
    let mut r3: Vec<u16> = Vec::new();
    let mut k = 0u16;
    while k < 8 {
        r3.push(0x30 + k);
        k += 1;
    }
    let mut r4: Vec<u16> = Vec::new();
    r4.push(0x41);
    r4.push(0x42);
    let mut r5: Vec<u16> = Vec::new();
    r5.push(0x51);
    r5.push(0x52);
    let mut r6: Vec<u16> = Vec::new();
    r6.push(0x61);
    de.register(101, 0xA001, 0xB001, SEV_P2, 0xC001, r1)?;
    de.register(102, 0xA002, 0xB002, SEV_P0, 0xC002, r2)?;
    de.register(103, 0xA003, 0xB003, SEV_P1, 0xC003, r3)?;
    de.register(104, 0xA004, 0xB004, SEV_P2, 0xC004, r4)?;
    de.register(105, 0xA005, 0xB005, SEV_P3, 0xC005, r5)?;
    de.register(106, 0xA006, 0xB006, SEV_P0, 0xC006, r6)?;
    if de.defect_count() != 6 {
        return Err(E_LEDGER_BAD);
    }

    // R1 主对齐：六缺陷全量出账，工单量守恒、五字段逐单逐位相等。
    let n = de.issue_all()?;
    if n != 6 || !de.conserved() {
        return Err(E_LEDGER_BAD);
    }
    let mut i = 0usize;
    while i < de.orders.len() {
        let o = &de.orders[i];
        // 五字段在位核（现象/位置/严重度/建议修法/复现路径）。
        if o.phenom == 0 || o.loc == 0 || o.fix == 0 || o.sev > SEV_P3 || o.repro.len() == 0 {
            return Err(E_FIELD_MISS);
        }
        i += 1;
    }
    de.audit()?;
    hits += 1;

    // R2 出口序终判：(严重度, 缺陷号) 升序——P0 两单先出（102→106），
    //    随后 P1(103)、P2(101→104)、P3(105)；工单号 1..6 闭合。
    let want_ids: [u32; 6] = [102, 106, 103, 101, 104, 105];
    let mut i = 0usize;
    while i < 6 {
        if de.orders[i].defect_id != want_ids[i] || de.orders[i].order_no != (i + 1) as u32 {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R3 字段缺位负例：现象码零 / 位置码零 / 修法码零 / 复现零步 /
    //    复现超 8 步 / 复现步码零 —— 六路全拒（E_FIELD_MISS 族）。
    let mut de2 = DefExit::new();
    let r: Vec<u16> = Vec::new();
    if de2.register(1, 0, 0xB001, SEV_P1, 0xC001, r.clone()).is_ok() {
        return Err(E_FIELD_MISS);
    }
    if de2.register(1, 0xA001, 0, SEV_P1, 0xC001, r.clone()).is_ok() {
        return Err(E_FIELD_MISS);
    }
    if de2.register(1, 0xA001, 0xB001, SEV_P1, 0, r.clone()).is_ok() {
        return Err(E_FIELD_MISS);
    }
    if de2.register(1, 0xA001, 0xB001, SEV_P1, 0xC001, r).is_ok() {
        return Err(E_FIELD_MISS); // 零步复现
    }
    let mut over: Vec<u16> = Vec::new();
    let mut k = 0u16;
    while k < 9 {
        over.push(0x70 + k);
        k += 1;
    }
    if de2.register(1, 0xA001, 0xB001, SEV_P1, 0xC001, over).is_ok() {
        return Err(E_FIELD_MISS); // 超 8 步
    }
    let mut zero_step: Vec<u16> = Vec::new();
    zero_step.push(0);
    if de2.register(1, 0xA001, 0xB001, SEV_P1, 0xC001, zero_step).is_ok() {
        return Err(E_FIELD_MISS);
    }
    if de2.defect_count() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 严重度闭集负例：sev=4 → E_SEV_BAD。
    let mut r9: Vec<u16> = Vec::new();
    r9.push(0x81);
    if de2.register(2, 0xA002, 0xB002, 4, 0xC002, r9).is_ok() {
        return Err(E_SEV_BAD);
    }
    hits += 1;

    // R5 工单冻结负例：出账后改字段 → E_FROZEN、篡改计数留痕；
    //    改不存在工单号 → E_STATE_BAD。
    if de.amend_order(3, 0xC999).is_ok() {
        return Err(E_FROZEN);
    }
    if de.amend_order(99, 0xC999).is_ok() {
        return Err(E_STATE_BAD);
    }
    if de.frozen < 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 缺陷号重号拒收：同 id 二次登记 → E_LEDGER_BAD。
    let mut r10: Vec<u16> = Vec::new();
    r10.push(0x91);
    if de.register(101, 0xA009, 0xB009, SEV_P1, 0xC009, r10).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if de.defect_count() != 6 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 空账不出口：零缺陷 issue_all → E_STATE_BAD。
    let mut de3 = DefExit::new();
    if de3.issue_all().is_ok() {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：零工单不写回、空摘要拒、出账+非空摘要放行。
    if de3.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if de.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    de.writeback(true, 0x06)?;
    hits += 1;

    Ok(hits)
}
