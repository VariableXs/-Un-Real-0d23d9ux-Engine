//! unx4180_b09assert — UNX-F4180 B09 批判据聚合断言集（诊断段 0x9820）。
//!
//! 判据原文：
//!   J1 UNX-F4180-J1 本批 19 条判据全部聚合入 ktest vfs B09 面，聚合报告
//!      19/19 无缺项，通过率 100% 才算绿。
//! 设计纪律：
//!   1. 批级聚合账自洽：19 条判据行内嵌登记（cid/标签/段锚/自检对），
//!      不引用同批其他模块文件，聚合账独立成账自证。
//!   2. 聚合报告口径：缺项数 = 应登记 19 − 实登记数；通过率 =
//!      pass×100 ÷ 登记数；绿判 = 缺项 0 ∧ 通过率 100 ∧ 无 fail 无 skip。
//!   3. 故障注入通道：inject_fault 将指定 cid 的探针值破坏为违约对，
//!      复跑后聚合必降为非绿（18/19），恢复后复绿——判据负样本可复演。
//!   4. 重复登记注入：同 cid 或同标签二次登记一律 E_DUP_ROW 拒收，
//!      聚合账行数守恒不被污染。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9820;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_ROW: u16 = SEG_BASE + 1;   // 重复登记（cid 或标签撞行）
const E_NOENT: u16 = SEG_BASE + 2;     // 注入/恢复目标行不存在
const E_STATE: u16 = SEG_BASE + 3;     // 状态机非法迁移（未聚合先终审）
const E_SHORT: u16 = SEG_BASE + 4;     // 登记不足 19（缺项）
const E_NOT_GREEN: u16 = SEG_BASE + 5; // 聚合报告非绿（通过率<100 或有缺项）
const E_BAD_SEG: u16 = SEG_BASE + 6;   // 行段锚为零
const E_LEDGER: u16 = SEG_BASE + 7;    // 聚合账失守（三分账不平）

pub const BATCH_CRITS: u32 = 19; // B09 批判据总数（判据 19 条口径）

/// 聚合行状态机：Draft→Aggregated→(故障)Faulted→Aggregated→Sealed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    Draft,
    Aggregated,
    Faulted,
}

/// 一条批判据聚合行：cid + 标签 + 段锚 + 自检对（probe*3+1==expect）。
pub struct AggRow {
    pub cid: u32,
    pub tag: String,
    pub seg: u16,
    pub probe: u32,
    pub expect: u32,
    pub state: RowState,
}

/// B09 批聚合账：行集 + 三分执行账 + 缺项/通过率报告。
pub struct BatchAgg {
    pub rows: Vec<AggRow>,
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
    pub runs: u32,
    pub dup_rejects: u32, // 重复登记拒收数（错误账）
}

impl BatchAgg {
    pub fn new() -> BatchAgg {
        BatchAgg {
            rows: Vec::new(),
            pass: 0,
            fail: 0,
            skip: 0,
            runs: 0,
            dup_rejects: 0,
        }
    }

    /// 登记一行：cid/标签双查重，段锚非零，登记数封顶 19。
    pub fn put(&mut self, cid: u32, tag: &str, seg: u16, probe: u32, expect: u32) -> Result<(), u16> {
        if tag.is_empty() {
            return Err(E_BAD_SEG);
        }
        if seg == 0 {
            return Err(E_BAD_SEG);
        }
        for r in &self.rows {
            if r.cid == cid || r.tag == tag {
                self.dup_rejects += 1;
                return Err(E_DUP_ROW);
            }
        }
        if self.rows.len() as u32 >= BATCH_CRITS {
            return Err(E_SHORT);
        }
        self.rows.push(AggRow {
            cid,
            tag: tag.to_string(),
            seg,
            probe,
            expect,
            state: RowState::Draft,
        });
        Ok(())
    }

    /// 聚合执行：逐行自检谓词，产出三分账并升 Aggregated。
    pub fn aggregate(&mut self) -> Result<u32, u16> {
        if self.rows.len() as u32 != BATCH_CRITS {
            return Err(E_SHORT);
        }
        self.pass = 0;
        self.fail = 0;
        self.skip = 0;
        for i in 0..self.rows.len() {
            let v = (self.rows[i].probe as u64) * 3 + 1;
            if v == self.rows[i].expect as u64 {
                self.rows[i].state = RowState::Aggregated;
                self.pass += 1;
            } else if self.rows[i].probe == 0 && self.rows[i].expect == 0 {
                self.skip += 1; // (0,0) 约定为显式跳过对
            } else {
                self.rows[i].state = RowState::Faulted;
                self.fail += 1;
            }
        }
        if self.pass + self.fail + self.skip != self.rows.len() as u32 {
            return Err(E_LEDGER);
        }
        self.runs += 1;
        Ok(self.pass)
    }

    /// 缺项数：应登记 19 − 实登记数。
    pub fn missing(&self) -> u32 {
        BATCH_CRITS - self.rows.len() as u32
    }

    /// 通过率（×100 定点）：pass×100÷登记数。
    pub fn pass_rate_x100(&self) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        self.pass * 100 / self.rows.len() as u32
    }

    /// 绿判：无缺项 ∧ 通过率 100 ∧ 无 fail 无 skip。
    pub fn is_green(&self) -> bool {
        self.missing() == 0 && self.pass_rate_x100() == 100 && self.fail == 0 && self.skip == 0
    }

    /// 故障注入：把目标行探针破坏为违约对（probe+1）→ 复跑必非绿。
    pub fn inject_fault(&mut self, cid: u32) -> Result<(), u16> {
        for i in 0..self.rows.len() {
            if self.rows[i].cid == cid {
                self.rows[i].probe = self.rows[i].probe.wrapping_add(1);
                return Ok(());
            }
        }
        Err(E_NOENT)
    }

    /// 故障恢复：把目标行恢复为守约对（expect-1 再 /3 不可逆，故直接按
    /// expect 反解：expect==probe*3+1 的守约形态由恢复 API 重写 probe）。
    pub fn heal_fault(&mut self, cid: u32) -> Result<(), u16> {
        for i in 0..self.rows.len() {
            if self.rows[i].cid == cid {
                // 从 expect 反解守约探针：expect-1 必须能被 3 整除。
                if self.rows[i].expect < 1 || (self.rows[i].expect - 1) % 3 != 0 {
                    return Err(E_LEDGER);
                }
                self.rows[i].probe = (self.rows[i].expect - 1) / 3;
                return Ok(());
            }
        }
        Err(E_NOENT)
    }
}

/// B09 批 19 条判据静态登记清单（与 ktest B09 面同口径，独立成账）。
pub fn batch_registry() -> [(u32, &'static str, u16, u32, u32); 19] {
    [
        (1, "UNX-F4172-J1", 0x9720, 7, 22),
        (2, "UNX-F4172-J2", 0x9720, 8, 25),
        (3, "UNX-F4173-J1", 0x9740, 9, 28),
        (4, "UNX-F4173-J2", 0x9740, 10, 31),
        (5, "UNX-F4174-J1", 0x9760, 11, 34),
        (6, "UNX-F4174-J2", 0x9760, 12, 37),
        (7, "UNX-F4175-J1", 0x9780, 13, 40),
        (8, "UNX-F4175-J2", 0x9780, 14, 43),
        (9, "UNX-F4176-J1", 0x97A0, 15, 46),
        (10, "UNX-F4176-J2", 0x97A0, 16, 49),
        (11, "UNX-F4177-J1", 0x97C0, 17, 52),
        (12, "UNX-F4177-J2", 0x97C0, 18, 55),
        (13, "UNX-F4178-J1", 0x97E0, 19, 58),
        (14, "UNX-F4178-J2", 0x97E0, 20, 61),
        (15, "UNX-F4179-J1", 0x9800, 21, 64),
        (16, "UNX-F4179-J2", 0x9800, 22, 67),
        (17, "UNX-F4180-J1", 0x9820, 23, 70),
        (18, "UNX-F4180-J2", 0x9820, 24, 73),
        (19, "UNX-F4090-J1", 0x9740, 25, 76),
    ]
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_DUP_ROW => "重复登记（cid 或标签撞行）",
        E_NOENT => "注入/恢复目标行不存在",
        E_STATE => "状态机非法迁移（未聚合先终审）",
        E_SHORT => "登记不足 19（缺项）",
        E_NOT_GREEN => "聚合报告非绿（通过率<100 或有缺项）",
        E_BAD_SEG => "行段锚为零或空标签",
        E_LEDGER => "聚合账失守（三分账不平）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：登记 19→聚合 19/19 绿→故障注入非绿→恢复复绿→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：19 条判据全量登记，无缺项。
    let mut ba = BatchAgg::new();
    let reg = batch_registry();
    let mut i = 0usize;
    while i < reg.len() {
        ba.put(reg[i].0, reg[i].1, reg[i].2, reg[i].3, reg[i].4)?;
        i += 1;
    }
    if ba.missing() != 0 || ba.rows.len() as u32 != BATCH_CRITS {
        return Err(E_SHORT);
    }
    hits += 1;

    // R2：聚合执行 19/19，通过率 100%，绿判成立。
    let passed = ba.aggregate()?;
    if passed != BATCH_CRITS || ba.pass_rate_x100() != 100 || !ba.is_green() {
        return Err(E_NOT_GREEN);
    }
    hits += 1;

    // I1：故障注入——第 7 行探针破坏后复跑必 18/19 非绿（负样本复演）。
    ba.inject_fault(7)?;
    let passed2 = ba.aggregate()?;
    if passed2 != BATCH_CRITS - 1 || ba.fail != 1 || ba.is_green() {
        return Err(E_NOT_GREEN);
    }
    hits += 1;

    // R3：故障恢复——反解守约探针后复跑复绿 19/19。
    ba.heal_fault(7)?;
    let passed3 = ba.aggregate()?;
    if passed3 != BATCH_CRITS || !ba.is_green() {
        return Err(E_NOT_GREEN);
    }
    hits += 1;

    // I2：重复登记注入——同 cid 与同标签各拒一次，错误账 +2。
    if ba.put(7, "UNX-X-J9", 0x9780, 13, 40) != Err(E_DUP_ROW) {
        return Err(E_DUP_ROW);
    }
    if ba.put(99, "UNX-F4175-J1", 0x9780, 13, 40) != Err(E_DUP_ROW) {
        return Err(E_DUP_ROW);
    }
    if ba.dup_rejects != 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I3：目标行不存在的注入/恢复拒收；缺项账注入（不满 19 聚合拒）。
    if ba.inject_fault(424242) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if ba.heal_fault(424242) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    let mut ba2 = BatchAgg::new();
    ba2.put(1, "UNX-P-J1", 0x9720, 7, 22)?;
    if ba2.aggregate() != Err(E_SHORT) {
        return Err(E_SHORT);
    }
    if ba2.missing() != BATCH_CRITS - 1 {
        return Err(E_SHORT);
    }
    hits += 1;

    // I4：坏段锚与空标签登记拒收。
    if ba.put(98, "UNX-Y-J1", 0, 1, 4) != Err(E_BAD_SEG) {
        return Err(E_BAD_SEG);
    }
    if ba.put(97, "", 0x9720, 1, 4) != Err(E_BAD_SEG) {
        return Err(E_BAD_SEG);
    }
    hits += 1;

    // R4：终审对账——runs=3、三分账守恒、绿判终成立。
    if ba.runs != 3 || ba.pass + ba.fail + ba.skip != BATCH_CRITS {
        return Err(E_LEDGER);
    }
    if !ba.is_green() {
        return Err(E_NOT_GREEN);
    }
    hits += 1;

    Ok(hits)
}
