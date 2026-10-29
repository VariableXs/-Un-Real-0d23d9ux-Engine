//! unx48320_b16seal — UNX-F48320 B16 批收官印（20 条冻结/累计 96,000 行）（诊断段 0xC8E0）。
//!
//! 判据原文：
//!   J1 批印入册：域账 96,000/240,000；判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 批条账：B16 批 20 条，每条 (id, 行数, 冻结位, 印号)；批总行数
//!      必须恰 96,000——建批即对账，差一行不建批。
//!   2. 域账口径：批行数对域预算 240,000 的占用以千分比记账
//!      （96,000/240,000 = 400‰），收官印必须携带该域账口径。
//!   3. 冻结语义：盖章即冻结——冻结条行数不可再改（改则 E_FROZEN）、
//!      不可重复盖章（E_RESEAL）；收官印只进不退。
//!   4. 批印闭合：批印 = 20 条全盖章 + 总行数复核 96,000 + 印号一致；
//!      缺条、缺账、印号漂移均不达收官位。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC8E0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SUM_OFF: u16 = SEG_BASE + 1; // 批总行数 ≠ 96,000
const E_BUDGET_OVER: u16 = SEG_BASE + 2; // 域预算越界
const E_FROZEN: u16 = SEG_BASE + 3; // 冻结条被改
const E_RESEAL: u16 = SEG_BASE + 4; // 重复盖章
const E_ITEM_MISS: u16 = SEG_BASE + 5; // 批条缺号
const E_STATE_BAD: u16 = SEG_BASE + 6; // 印器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 批口径：20 条 / 96,000 行 / 域预算 240,000 行。
pub const BATCH_ITEMS: u32 = 20;
pub const BATCH_LINES: u32 = 96_000;
pub const DOMAIN_BUDGET: u32 = 240_000;

/// B16 批 20 条行数表（首十年递减盘 51,000、次十年平盘 45,000）。
pub const ITEM_LINES: [u32; 20] = [
    6000, 5800, 5600, 5400, 5200, 5000, 4800, 4600, 4400, 4200, // 51,000
    4500, 4500, 4500, 4500, 4500, 4500, 4500, 4500, 4500, 4500, // 45,000
];

/// 域账占用千分比：lines * 1000 / budget。
pub fn permille(lines: u32, budget: u32) -> u32 {
    (lines as u64 * 1000 / budget as u64) as u32
}

/// 批条一条。
#[derive(Clone, Copy, Debug)]
pub struct SealItem {
    pub id: u16,
    pub lines: u32,
    pub sealed: bool,
    pub stamp: u32,
}

/// 收官印器。
pub struct SealPress {
    items: Vec<SealItem>,
    pub batch_no: u32,
    pub stamped: u32,
    pub sealed_lines: u32,
    pub tamper_blocked: u32,
    pub reseal_blocked: u32,
    pub press_closed: bool,
}

impl SealPress {
    /// 建批：条数恰 20、行数恰 96,000、id 连续——三查过方建批。
    pub fn build(lines: &[u32; 20]) -> Result<SealPress, u16> {
        let mut total = 0u32;
        let mut i = 0usize;
        while i < lines.len() {
            total += lines[i];
            i += 1;
        }
        if total != BATCH_LINES {
            return Err(E_SUM_OFF);
        }
        let mut items = Vec::new();
        let mut id = 0u16;
        while (id as usize) < lines.len() {
            items.push(SealItem { id, lines: lines[id as usize], sealed: false, stamp: 0 });
            id += 1;
        }
        Ok(SealPress {
            items,
            batch_no: 0,
            stamped: 0,
            sealed_lines: 0,
            tamper_blocked: 0,
            reseal_blocked: 0,
            press_closed: false,
        })
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn total_lines(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            t += self.items[i].lines;
            i += 1;
        }
        t
    }

    fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 域账口径检查：批行数不得越域预算。
    pub fn budget_check(&self) -> Result<u32, u16> {
        let t = self.total_lines();
        if t > DOMAIN_BUDGET {
            return Err(E_BUDGET_OVER);
        }
        Ok(permille(t, DOMAIN_BUDGET))
    }

    /// 单条盖章：冻结位落章、行数入冻结账、印号入条。
    pub fn press_one(&mut self, id: u16, batch_no: u32) -> Result<(), u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_ITEM_MISS),
        };
        if self.items[idx].sealed {
            self.reseal_blocked += 1;
            return Err(E_RESEAL);
        }
        self.items[idx].sealed = true;
        self.items[idx].stamp = batch_no;
        self.stamped += 1;
        self.sealed_lines += self.items[idx].lines;
        Ok(())
    }

    /// 冻结条改行数：必拒（E_FROZEN），篡改计数留痕。
    pub fn amend_lines(&mut self, id: u16, new_lines: u32) -> Result<(), u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_ITEM_MISS),
        };
        if self.items[idx].sealed {
            self.tamper_blocked += 1;
            return Err(E_FROZEN);
        }
        self.items[idx].lines = new_lines;
        Ok(())
    }

    /// 批印闭合（J1 主体）：20 条全盖章 + 总行数复核 + 印号一致。
    pub fn press_all(&mut self, batch_no: u32) -> Result<u32, u16> {
        if self.item_count() as u32 != BATCH_ITEMS {
            return Err(E_ITEM_MISS);
        }
        if self.total_lines() != BATCH_LINES {
            return Err(E_SUM_OFF);
        }
        let mut id = 0u16;
        while (id as u32) < BATCH_ITEMS {
            self.press_one(id, batch_no)?;
            id += 1;
        }
        if self.stamped != BATCH_ITEMS {
            return Err(E_STATE_BAD);
        }
        if self.sealed_lines != BATCH_LINES {
            return Err(E_SUM_OFF);
        }
        self.batch_no = batch_no;
        self.press_closed = true;
        self.budget_check()
    }

    /// 批印账审计：盖章数、冻结行数、印号、冻结位四方闭合。
    pub fn audit(&self) -> Result<(), u16> {
        if self.stamped as usize != self.items.len() {
            return Err(E_ITEM_MISS);
        }
        if self.sealed_lines != BATCH_LINES {
            return Err(E_SUM_OFF);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            if !it.sealed || it.stamp != self.batch_no {
                return Err(E_STATE_BAD);
            }
            i += 1;
        }
        if !self.press_closed {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：批印闭合并 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.press_closed {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（批印入册：域账 96,000/240,000），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 建批三查：20 条、总行数恰 96,000、id 连续；域账口径 400‰。
    let sp = SealPress::build(&ITEM_LINES)?;
    if sp.item_count() != BATCH_ITEMS as usize {
        return Err(E_ITEM_MISS);
    }
    if sp.total_lines() != BATCH_LINES {
        return Err(E_SUM_OFF);
    }
    let pm = sp.budget_check()?;
    if pm != 400 {
        return Err(E_BUDGET_OVER); // 96,000/240,000 = 400‰
    }
    hits += 1;

    // R2 行数表自洽：常量表逐项求和恰 96,000（表与账双口径一致）。
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < ITEM_LINES.len() {
        sum += ITEM_LINES[i];
        i += 1;
    }
    if sum != 96_000 {
        return Err(E_SUM_OFF);
    }
    hits += 1;

    // R3 批印闭合（J1 主体）：20 条全盖章、冻结行数 96,000、印号一致。
    let mut sp = SealPress::build(&ITEM_LINES)?;
    let pm2 = sp.press_all(0xB16C)?;
    if pm2 != 400 {
        return Err(E_BUDGET_OVER);
    }
    if sp.stamped != 20 || sp.sealed_lines != BATCH_LINES || sp.batch_no != 0xB16C {
        return Err(E_STATE_BAD);
    }
    sp.audit()?;
    hits += 1;

    // R4 冻结防线：盖章后改行数必拒（E_FROZEN）、篡改计数留痕。
    let mut sp2 = SealPress::build(&ITEM_LINES)?;
    sp2.press_all(0xB16D)?;
    if sp2.amend_lines(0, 9999).is_ok() {
        return Err(E_FROZEN);
    }
    if sp2.tamper_blocked != 1 || sp2.sealed_lines != BATCH_LINES {
        return Err(E_STATE_BAD);
    }
    sp2.audit()?;
    hits += 1;

    // R5 重复盖章拒收：同条二次盖章必拒（E_RESEAL）、计数留痕。
    let mut sp3 = SealPress::build(&ITEM_LINES)?;
    sp3.press_one(3, 0xB16E)?;
    if sp3.press_one(3, 0xB16E).is_ok() {
        return Err(E_RESEAL);
    }
    if sp3.reseal_blocked != 1 || sp3.stamped != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 错账不收官：行数表人为 +100 → 建批必拒（E_SUM_OFF）；
    //    未冻结改一行（合法路径）后批印必拒（行数漂移）。
    let mut bad = ITEM_LINES;
    bad[7] = bad[7] + 100;
    if SealPress::build(&bad).is_ok() {
        return Err(E_SUM_OFF);
    }
    let mut sp5 = SealPress::build(&ITEM_LINES)?;
    sp5.amend_lines(19, 9_999)?;
    if sp5.press_all(0xB16F).is_ok() {
        return Err(E_SUM_OFF);
    }
    hits += 1;

    // R7 冻结不可逆终判：批印闭合后 20 条冻结位全真、印号全同、
    //    冻结账 = 批账（只进不退）。
    sp.audit()?;
    if sp.sealed_lines != sp.total_lines() {
        return Err(E_SUM_OFF);
    }
    if sp.batch_no != 0xB16C {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：批印未闭不写回、空摘要拒、闭合并+非空摘要放行。
    let mut sp6 = SealPress::build(&ITEM_LINES)?;
    if sp6.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    sp6.press_all(0xB170)?;
    if sp6.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sp6.writeback(true, 0x30)?;
    hits += 1;

    Ok(hits)
}
