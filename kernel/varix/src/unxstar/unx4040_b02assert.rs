//! unx4040_b02assert — UNX-F4040 B02 批判据聚合断言集（诊断段 0x8780）。
//!
//! 判据原文：
//!   J1 本批 19 条判据全部聚合入 ktest vfs B02 面；
//!   J2 聚合报告 19/19 无缺项；
//!   J3 通过率 100% 才算绿。
//! 设计纪律：
//!   1. 批级聚合账：内嵌登记表逐卡收（任务号, 段位, 验收性质数）
//!      三元组——账只登记不执行（本卡为聚合账，非测试执行器）。
//!   2. 段位闭集互异：批内全部段位两两不等（0x85E0..0x8780 逐对
//!      核对），任一重段即断言失守。
//!   3. 编号账：任务号严格递增且落在批号闭集（4027..4040）内，
//!      缺号/倒序/越批号一律拒收。
//!   4. 守恒式：卡数 = 表长；各卡性质数之和 = 批声明总数 19；
//!      断言通过数 + 断言缺失数 = 总数（判据 J2 的无缺项账面）。
//!   5. 绿灯判据（J3）：通过率 = 通过数/总数（千分比整数化），
//!      1000‰ 且无缺项才算绿；否则红。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8780;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_SEG_DUP: u16 = SEG_BASE + 1;     // 段位重复（闭集互异失守）
const E_ID_DUP: u16 = SEG_BASE + 2;      // 任务号重复
const E_ID_ORDER: u16 = SEG_BASE + 3;    // 任务号倒序/越批号
const E_CRIT_ZERO: u16 = SEG_BASE + 4;   // 验收性质数为 0 非法
const E_BATCH_MISMATCH: u16 = SEG_BASE + 5; // 聚合总数与批声明不符
const E_ASSERT_MISS: u16 = SEG_BASE + 6; // 断言缺项（19/19 不满）
const E_NOT_GREEN: u16 = SEG_BASE + 7;   // 通过率不足 100%（非绿灯）
const E_BAL: u16 = SEG_BASE + 8;         // 批账守恒失守

/// B02 批声明判据总数（验收口径：19/19）。
pub const BATCH_CRIT_TOTAL: u32 = 19;
/// B02 批卡数（本批施工单卡数）。
pub const BATCH_CARDS: usize = 14;
/// 批号闭集。
pub const ID_LO: u32 = 4027;
pub const ID_HI: u32 = 4040;
/// 通过率千分比基准。
pub const GREEN_BASE: u32 = 1000;

/// 单卡聚合条目：任务号 + 段位 + 验收性质数 + 通过账。
#[derive(Clone, Copy)]
pub struct BatchCard {
    pub id: u32,     // 任务号（UNX-F4NNN 的 NNN）
    pub seg: u16,    // 诊断段位
    pub crits: u32,  // 验收性质数
    pub passed: u32, // 已通过性质数（聚合账面）
}

/// B02 批聚合账枢纽。
pub struct B02AssertSet {
    pub cards: Vec<BatchCard>,
    pub crit_total: u32,   // 聚合性质总数（= 各卡 crits 和）
    pub passed_total: u32, // 通过性质总数
    pub reg_calls: u32,    // 批登记调用数（含被拒）
    pub reg_ok: u32,       // 批登记成功次数（≤1）
    pub rejects: u32,      // 错误账
}

impl B02AssertSet {
    pub fn new() -> B02AssertSet {
        B02AssertSet {
            cards: Vec::new(),
            crit_total: 0,
            passed_total: 0,
            reg_calls: 0,
            reg_ok: 0,
            rejects: 0,
        }
    }

    /// 逐卡登记：编号账 + 段位互异 + 性质数校验。
    pub fn card_add(&mut self, id: u32, seg: u16, crits: u32) -> Result<(), u16> {
        if crits == 0 {
            self.rejects += 1;
            return Err(E_CRIT_ZERO);
        }
        if id < ID_LO || id > ID_HI {
            self.rejects += 1;
            return Err(E_ID_ORDER);
        }
        let mut i = 0;
        while i < self.cards.len() {
            if i < self.cards.len() {
                // 编号账：严格递增（倒序/重复均拒）。
                if self.cards[i].id == id {
                    self.rejects += 1;
                    return Err(E_ID_DUP);
                }
                if self.cards[i].id > id {
                    self.rejects += 1;
                    return Err(E_ID_ORDER);
                }
                // 段位闭集互异（判据 J1 的前置不变式）。
                if self.cards[i].seg == seg {
                    self.rejects += 1;
                    return Err(E_SEG_DUP);
                }
            }
            i += 1;
        }
        self.cards.push(BatchCard { id, seg, crits, passed: 0 });
        self.crit_total += crits;
        Ok(())
    }

    /// 批登记：一次性灌入批内 14 卡（段位按各模块 SEG_BASE 定编）。
    pub fn batch_register(&mut self) -> Result<u32, u16> {
        self.reg_calls += 1;
        if self.reg_ok > 0 {
            self.rejects += 1;
            return Err(E_BAL);
        }
        // （任务号, 段位, 性质数）定编表——段位与施工单逐一对应。
        const TBL: [(u32, u16, u32); BATCH_CARDS] = [
            (4027, 0x85E0, 2),
            (4028, 0x8600, 2),
            (4029, 0x8620, 1),
            (4030, 0x8640, 1),
            (4031, 0x8660, 1),
            (4032, 0x8680, 1),
            (4033, 0x86A0, 2),
            (4034, 0x86C0, 2),
            (4035, 0x86E0, 2),
            (4036, 0x8700, 1),
            (4037, 0x8720, 1),
            (4038, 0x8740, 1),
            (4039, 0x8760, 1),
            (4040, 0x8780, 1),
        ];
        let mut k = 0;
        while k < BATCH_CARDS {
            let (id, seg, crits) = TBL[k];
            self.card_add(id, seg, crits)?;
            k += 1;
        }
        // 判据 J1：聚合总数必须等于批声明总数（19）。
        if self.crit_total != BATCH_CRIT_TOTAL {
            return Err(E_BATCH_MISMATCH);
        }
        self.reg_ok += 1;
        Ok(self.crit_total)
    }

    /// 断言通过登记（逐性质）：卡内通过账推进，不超性质数。
    pub fn pass_one(&mut self, id: u32) -> Result<u32, u16> {
        let mut i = 0;
        while i < self.cards.len() {
            if i < self.cards.len() && self.cards[i].id == id {
                if self.cards[i].passed >= self.cards[i].crits {
                    self.rejects += 1;
                    return Err(E_ASSERT_MISS);
                }
                self.cards[i].passed += 1;
                self.passed_total += 1;
                return Ok(self.cards[i].passed);
            }
            i += 1;
        }
        self.rejects += 1;
        Err(E_BAL)
    }

    /// 无缺项断言（判据 J2）：每卡通过数 = 性质数，无缺项。
    pub fn assert_full(&self) -> Result<(), u16> {
        if self.cards.len() != BATCH_CARDS {
            return Err(E_ASSERT_MISS);
        }
        let mut i = 0;
        while i < self.cards.len() {
            if i < self.cards.len() && self.cards[i].passed != self.cards[i].crits {
                return Err(E_ASSERT_MISS);
            }
            i += 1;
        }
        if self.passed_total != self.crit_total {
            return Err(E_ASSERT_MISS);
        }
        Ok(())
    }

    /// 绿灯判定（判据 J3）：通过率 1000‰ 且无缺项。
    pub fn green(&self) -> Result<u32, u16> {
        self.assert_full()?;
        let rate = if self.crit_total == 0 {
            0
        } else {
            self.passed_total * GREEN_BASE / self.crit_total
        };
        if rate != GREEN_BASE {
            return Err(E_NOT_GREEN);
        }
        Ok(rate)
    }

    /// 守恒对账：卡数、性质总数、通过账、登记账闭合。
    pub fn audit(&self) -> Result<(), u16> {
        if self.cards.len() != BATCH_CARDS {
            return Err(E_BAL);
        }
        let mut sum = 0u32;
        let mut psum = 0u32;
        let mut i = 0;
        while i < self.cards.len() {
            if i < self.cards.len() {
                sum += self.cards[i].crits;
                psum += self.cards[i].passed;
            }
            i += 1;
        }
        if sum != self.crit_total || psum != self.passed_total {
            return Err(E_BAL);
        }
        if self.reg_ok != 1 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 聚合报告导出（判据 J2 报告面）：（总数，通过数，缺失数，绿灯）。
    pub fn report(&self) -> Result<(u32, u32, u32, bool), u16> {
        let missing = self.crit_total - self.passed_total;
        let ok = self.green().is_ok();
        Ok((self.crit_total, self.passed_total, missing, ok))
    }
}

/// 自例程：R1 批登记 → R2 互异/编号注入 → R3 逐性质通过 → R4 缺项注入 → R5 绿灯终局。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ba = B02AssertSet::new();

    // R1 批登记（判据 J1）：14 卡入账，聚合总数 = 19。
    let tot = ba.batch_register()?;
    if tot != BATCH_CRIT_TOTAL || ba.cards.len() != BATCH_CARDS {
        return Err(E_BATCH_MISMATCH);
    }
    hits += 1;

    // R2 互异/编号注入：重段、重号、倒序、越批号、零性质数分别被拒。
    let mut b2 = B02AssertSet::new();
    b2.card_add(4027, 0x85E0, 2)?;
    if b2.card_add(4028, 0x85E0, 1) != Err(E_SEG_DUP) {
        return Err(E_BAL);
    }
    if b2.card_add(4027, 0x8600, 1) != Err(E_ID_DUP) {
        return Err(E_BAL);
    }
    if b2.card_add(4026, 0x8620, 1) != Err(E_ID_ORDER) {
        return Err(E_BAL);
    }
    if b2.card_add(4041, 0x8640, 1) != Err(E_ID_ORDER) {
        return Err(E_BAL);
    }
    if b2.card_add(4029, 0x8660, 0) != Err(E_CRIT_ZERO) {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 逐性质通过：按卡灌满通过账（共 19 笔）。
    const CRIT: [u32; BATCH_CARDS] =
        [2, 2, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1];
    let mut id = ID_LO;
    let mut ci = 0;
    while ci < BATCH_CARDS {
        let mut k = 0;
        while k < CRIT[ci] {
            ba.pass_one(id)?;
            k += 1;
        }
        id += 1;
        ci += 1;
    }
    if ba.passed_total != BATCH_CRIT_TOTAL {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 缺项注入：缺一笔通过时 assert_full 必须报缺项（J2 反向）。
    let mut b3 = B02AssertSet::new();
    b3.batch_register()?;
    // 只灌 4027 与 4028 的账（共 4 笔），其余 15 笔缺项。
    let mut k = 0;
    while k < 2 {
        b3.pass_one(4027)?;
        k += 1;
    }
    let mut k = 0;
    while k < 2 {
        b3.pass_one(4028)?;
        k += 1;
    }
    if b3.assert_full() != Err(E_ASSERT_MISS) {
        return Err(E_BAL);
    }
    if b3.green() != Err(E_NOT_GREEN) {
        return Err(E_BAL);
    }
    let (_t, _p, miss, green) = b3.report()?;
    if miss != 15 || green {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 绿灯终局（判据 J3）：满账绿灯 1000‰；超灌被拒；账面闭合。
    if ba.pass_one(4027) != Err(E_ASSERT_MISS) {
        return Err(E_BAL);
    }
    if ba.green()? != GREEN_BASE {
        return Err(E_BAL);
    }
    ba.audit()?;
    let (t, p, m, g) = ba.report()?;
    if t != 19 || p != 19 || m != 0 || !g {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
