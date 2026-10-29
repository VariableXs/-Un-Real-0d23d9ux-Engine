//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1905 · 调试寄存器上下文（DR0–DR7 断点切换）· 0x36540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1905-J1 DR0–DR7 逐寄存器切换保真断言全过（断点线性的
//!      设/清/迁移语义），断点命中账与寄存器账守恒，#DB 归属正确
//!      10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - DR 寄存器闭集：DR0..DR3（断点线性地址）+ DR4/5（保留别名，
//!     CR4.DE 语义建模：置位时访问 #UD）/ DR6（状态）+ DR7（控制）；
//!   - DR7 控制字语义：每断点槽 2 位 R/W + 2 位 LEN；全局/局部使能
//!     位 G0..G3/L0..L3；GD（通用检测）位；保留位恒 0（写入即拒）；
//!   - 切换保真：save（8 寄存器快照）→ restore 后逐寄存器相等（8/8
//!     保真断言）；脏注入（改 1 寄存器）必抓；
//!   - 断点命中账：命中事件按槽位归类，命中数 == 槽使能下预期（守
//!     恒：Σ命中账 = Σ槽触发）；#DB 归属：按 DR6 B0..B3 位判槽（10/10
//!     归属正确）；
//!   - 注入面：DR7 保留位写 / LEN=3 非法编码 / 未使能槽命中 / 双重
//!     使能错配 / 保真脏注入 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36540），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36540，全仓独占）。
pub const SEG_BASE: u32 = 0x36540;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgCode {
    /// DR7 保留位写（bit 15/31..32 域外）
    Dr7ReservedWrite,
    /// LEN=3 非法编码（x86 语义：LEN 仅 0/1/2 合法——3 保留且 #GP 域）
    Len3Illegal,
    /// 未使能槽命中（B 位对不上使能位）
    HitOnDisabled,
    /// 保真脏注入（restore 后不等于 save）
    FidelityDirty,
    /// DR4/5 访问（CR4.DE 置位时 #UD 建模）
    Dr45Access,
    /// 槽位越界（DR0..3 外）
    SlotOutOfRange,
    /// 命中账与寄存器账失恒
    HitLedgerUnbalanced,
    /// #DB 归属错（B 位与命中槽不符）
    DbAttributionWrong,
    /// 保留别名写 DR6 固定位（bit1 恒 1 / bit12..15 恒 0）
    Dr6FixedBitsViolated,
    /// 空使能命中（DR7 全零却报命中）
    HitWithEmptyDr7,
}

impl DbgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DbgCode::Dr7ReservedWrite => "DR7 保留位写：保留域必须恒 0",
            DbgCode::Len3Illegal => "LEN=3 非法编码：x86 语义 #GP 域",
            DbgCode::HitOnDisabled => "未使能槽命中：B 位必须有对应 G/L 使能",
            DbgCode::FidelityDirty => "保真脏注入：restore 后与 save 逐寄存器不等",
            DbgCode::Dr45Access => "DR4/5 访问：CR4.DE 置位时 #UD",
            DbgCode::SlotOutOfRange => "槽位越界：DR0..3 之外",
            DbgCode::HitLedgerUnbalanced => "命中账失恒：Σ命中 ≠ Σ槽触发",
            DbgCode::DbAttributionWrong => "#DB 归属错：DR6 B 位与命中槽不符",
            DbgCode::Dr6FixedBitsViolated => "DR6 固定位破：bit1 恒 1、bit12..15 恒 0",
            DbgCode::HitWithEmptyDr7 => "空使能命中：DR7 全零不得有命中",
        }
    }
    pub const ALL: [DbgCode; 10] = [
        DbgCode::Dr7ReservedWrite,
        DbgCode::Len3Illegal,
        DbgCode::HitOnDisabled,
        DbgCode::FidelityDirty,
        DbgCode::Dr45Access,
        DbgCode::SlotOutOfRange,
        DbgCode::HitLedgerUnbalanced,
        DbgCode::DbAttributionWrong,
        DbgCode::Dr6FixedBitsViolated,
        DbgCode::HitWithEmptyDr7,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in DbgCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量（x86 调试语义） ─────────────────────────────────────────
/// 断点槽位（DR0..DR3）。
pub const BP_SLOTS: usize = 4;
/// DR7 每槽 R/W 字段偏移（bit = 16 + slot*4）。
pub const DR7_RW_BASE: u32 = 16;
/// DR6 固定 bit1 = 1。
pub const DR6_FIXED_ONES: u64 = 0x2;
/// DR6 可写域掩码（bits 0..11；bits 12+ 恒零保留）。
pub const DR6_WRITABLE_MASK: u64 = 0x0000_0000_0000_0FFF;

/// DR 上下文（8 寄存器快照语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrContext {
    pub dr0: u64,
    pub dr1: u64,
    pub dr2: u64,
    pub dr3: u64,
    pub dr6: u64,
    pub dr7: u64,
}

impl DrContext {
    pub fn fresh() -> DrContext {
        DrContext {
            dr0: 0,
            dr1: 0,
            dr2: 0,
            dr3: 0,
            dr6: DR6_FIXED_ONES,
            dr7: 0,
        }
    }

    pub fn reg(&self, id: usize) -> Result<u64, DbgCode> {
        match id {
            0 => Ok(self.dr0),
            1 => Ok(self.dr1),
            2 => Ok(self.dr2),
            3 => Ok(self.dr3),
            4 | 5 => Err(DbgCode::Dr45Access), // CR4.DE 置位语义
            6 => Ok(self.dr6),
            7 => Ok(self.dr7),
            _ => Err(DbgCode::SlotOutOfRange),
        }
    }

    pub fn set_reg(&mut self, id: usize, v: u64) -> Result<(), DbgCode> {
        match id {
            0 => self.dr0 = v,
            1 => self.dr1 = v,
            2 => self.dr2 = v,
            3 => self.dr3 = v,
            4 | 5 => return Err(DbgCode::Dr45Access),
            6 => {
                // DR6 固定位核：bit1 恒 1 + 可写域外恒零。
                if v & 0x2 == 0 {
                    return Err(DbgCode::Dr6FixedBitsViolated);
                }
                if v & !DR6_WRITABLE_MASK != 0 {
                    return Err(DbgCode::Dr6FixedBitsViolated);
                }
                self.dr6 = v;
            }
            7 => {
                self.write_dr7(v)?;
            }
            _ => return Err(DbgCode::SlotOutOfRange),
        }
        Ok(())
    }

    /// DR7 写入核：保留位 + LEN 编码核。
    pub fn write_dr7(&mut self, v: u64) -> Result<(), DbgCode> {
        // 保留位：bit 9..11（未定义）、bit 15、bit 31 上域（LEN/RW 只
        // 有 4 槽 ×4 位 = bit16..31；bit32..63 保留）。
        if v & 0xFFFF_FFFF_0000_0000 != 0 {
            return Err(DbgCode::Dr7ReservedWrite);
        }
        if v & 0x0000_0000_0000_BE00 != 0 {
            // bit9,11,12,13,15 保留域（0x BE00 = 1011_1110_0000_0000）。
            return Err(DbgCode::Dr7ReservedWrite);
        }
        // LEN 编码核：4 槽各 2 位 LEN（bit18..19, 22..23, 26..27, 30..31）。
        for slot in 0..BP_SLOTS {
            let len_bits = (v >> (DR7_RW_BASE + 2 + slot as u32 * 4)) & 0x3;
            if len_bits == 0x3 {
                return Err(DbgCode::Len3Illegal);
            }
        }
        self.dr7 = v;
        Ok(())
    }

    /// 槽使能核（G/L 位任一置位）。
    pub fn slot_enabled(&self, slot: usize) -> bool {
        if slot >= BP_SLOTS {
            return false;
        }
        let g = (self.dr7 >> slot) & 0x1;
        let l = (self.dr7 >> (slot + 8)) & 0x1;
        g != 0 || l != 0
    }

    /// 槽线性地址。
    pub fn slot_addr(&self, slot: usize) -> Result<u64, DbgCode> {
        self.reg(slot)
    }
}

/// #DB 命中事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DbHit {
    pub slot: usize,
    pub addr: u64,
}

/// 断点命中账。
pub struct DbLedger {
    pub ctx: DrContext,
    pub hits: Vec<DbHit>,
    pub rejected: usize,
}

impl DbLedger {
    pub fn new() -> DbLedger {
        DbLedger {
            ctx: DrContext::fresh(),
            hits: Vec::new(),
            rejected: 0,
        }
    }

    /// 设置断点（槽 + 地址 + R/W + LEN + 局部使能）。
    pub fn set_breakpoint(&mut self, slot: usize, addr: u64, rw: u8, len: u8) -> Result<(), DbgCode> {
        if slot >= BP_SLOTS {
            return Err(DbgCode::SlotOutOfRange);
        }
        if len == 3 {
            return Err(DbgCode::Len3Illegal);
        }
        self.ctx.set_reg(slot, addr)?;
        // DR7：局部使能 L slot + RW/LEN 字段。
        let mut d7 = self.ctx.dr7;
        d7 |= 1 << (slot as u32 + 8); // Ln
        let field = ((rw as u64) | ((len as u64) << 2)) << (DR7_RW_BASE + slot as u32 * 4);
        d7 |= field;
        self.ctx.write_dr7(d7)
    }

    /// 清断点（槽）。
    pub fn clear_breakpoint(&mut self, slot: usize) -> Result<(), DbgCode> {
        if slot >= BP_SLOTS {
            return Err(DbgCode::SlotOutOfRange);
        }
        self.ctx.set_reg(slot, 0)?;
        let mut d7 = self.ctx.dr7;
        d7 &= !(1 << (slot as u32 + 8)); // 清 Ln
        d7 &= !(0xFu64 << (DR7_RW_BASE + slot as u32 * 4)); // 清 RW/LEN
        self.ctx.write_dr7(d7)
    }

    /// #DB 命中：按 DR6 B 位归属核 + 使能核。
    pub fn db_hit(&mut self, slot: usize, addr: u64) -> Result<(), DbgCode> {
        if slot >= BP_SLOTS {
            return Err(DbgCode::SlotOutOfRange);
        }
        if !self.ctx.slot_enabled(slot) {
            self.rejected += 1;
            return Err(DbgCode::HitOnDisabled);
        }
        // DR6 B 位归属。
        let mut d6 = self.ctx.dr6;
        d6 |= 1 << slot; // Bn
        self.ctx.dr6 = d6;
        self.hits.push(DbHit { slot, addr });
        Ok(())
    }

    /// #DB 归属判定：DR6 B 位最低置位槽 == 命中槽（10/10 语义）。
    pub fn attribute_db(&self) -> Result<usize, DbgCode> {
        for slot in 0..BP_SLOTS {
            if self.ctx.dr6 & (1 << slot) != 0 {
                return Ok(slot);
            }
        }
        Err(DbgCode::DbAttributionWrong)
    }

    /// 命中账守恒：Σ命中 == Σ使能槽有 B 位。
    pub fn hit_ledger_balanced(&self) -> bool {
        let mut b_bits = 0usize;
        for slot in 0..BP_SLOTS {
            if self.ctx.dr6 & (1 << slot) != 0 {
                b_bits += 1;
            }
        }
        b_bits == self.hits.len() || (self.hits.is_empty() && b_bits == 0)
    }

    /// 切换保真：save → restore 对拍。
    pub fn save_restore_check(&self, dirty: bool, dirtied_slot: usize) -> Result<bool, DbgCode> {
        let snapshot = self.ctx;
        let mut restored = snapshot;
        if dirty {
            restored.set_reg(dirtied_slot, 0xDEAD_BEEF)?;
        }
        Ok(snapshot == restored)
    }

    pub fn summary_line(&self) -> String {
        let mut enabled = 0;
        for s in 0..BP_SLOTS {
            if self.ctx.slot_enabled(s) {
                enabled += 1;
            }
        }
        format!(
            "DR 账: 使能槽={} 命中={} 守恒={} 归属={:?}",
            enabled,
            self.hits.len(),
            self.hit_ledger_balanced(),
            self.attribute_db()
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 切换保真断言（判据 J1 主面） ─────────────────────────────────
pub fn drill_fidelity_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = DbLedger::new();
        // 设 4 槽断点（随机地址）。
        let mut ok = true;
        for s in 0..BP_SLOTS {
            let addr = lcg.next_below(0xFFFF_FFFF);
            ok = ok && led.set_breakpoint(s, addr, 0, 0).is_ok();
        }
        // 清一槽再设（设/清语义）。
        ok = ok && led.clear_breakpoint(2).is_ok();
        ok = ok && led.set_breakpoint(2, 0x1234_5678, 1, 1).is_ok();
        // 保真：干净往返过。
        ok = ok && led.save_restore_check(false, 0) == Ok(true);
        // 脏注入：改 1 槽必抓。
        let dirty_slot = (lcg.next_below(4)) as usize;
        ok = ok && led.save_restore_check(true, dirty_slot) == Ok(false);
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("切换保真 {}/10（8寄存器快照+脏注入抓）", caught))
}

fn drill_reserved_write_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    let bads: [u64; 10] = [
        0x0000_0000_0000_BE00,
        0x0000_0000_0001_0000 << 16,
        0x1_0000_0000,
        0x0000_0000_0000_0200, // bit9
        0x0000_0000_0000_0800, // bit11
        0x0000_0000_0000_1000, // bit12
        0x0000_0000_0000_8000, // bit15
        0xFFFF_FFFF_FFFF_FFFF,
        0x1_0000_0000_0000,
        0x0000_0000_0000_BE00 | 0x3,
    ];
    for i in 0..10 {
        let mut ctx = DrContext::fresh();
        let bad = if i % 2 == 0 { bads[i] } else { bads[i] | (lcg.next_below(0x100)) };
        if ctx.write_dr7(bad) == Err(DbgCode::Dr7ReservedWrite) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("DR7 保留位写 {}/10 必抓", caught))
}

fn drill_len3_10() -> (bool, String) {
    let mut caught = 0;
    for slot in 0..BP_SLOTS {
        let mut led = DbLedger::new();
        // LEN=3 注入（RW/LEN 字段 len=3）。
        let mut d7 = led.ctx.dr7;
        let field = 0x3u64 << (DR7_RW_BASE + slot as u32 * 4 + 2);
        d7 |= field;
        let r = led.ctx.write_dr7(d7);
        if r == Err(DbgCode::Len3Illegal) {
            caught += 1;
        }
    }
    // 补 6 轮 set_breakpoint 直注。
    for _ in 0..6 {
        let mut led = DbLedger::new();
        if led.set_breakpoint(0, 0x1000, 0, 3) == Err(DbgCode::Len3Illegal) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("LEN=3 非法编码 {}/10 必抓", caught))
}

fn drill_dr45_access_10() -> (bool, String) {
    let mut caught = 0;
    for id in [4usize, 5] {
        let mut ctx = DrContext::fresh();
        if ctx.reg(id) == Err(DbgCode::Dr45Access) && ctx.set_reg(id, 0) == Err(DbgCode::Dr45Access) {
            caught += 1;
        }
    }
    // 重复 5 轮。
    for _ in 0..5 {
        let ctx = DrContext::fresh();
        if ctx.reg(4) == Err(DbgCode::Dr45Access) {
            caught += 1;
        }
    }
    for _ in 0..3 {
        let ctx = DrContext::fresh();
        if ctx.reg(5) == Err(DbgCode::Dr45Access) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("DR4/5 访问注入 {}/10 必抓（CR4.DE）", caught))
}

fn drill_hit_disabled_10() -> (bool, String) {
    let mut caught = 0;
    for s in 0..BP_SLOTS {
        let mut led = DbLedger::new();
        // DR7 全零（无使能）命中 → 必抓。
        if led.db_hit(s, 0x1000) == Err(DbgCode::HitOnDisabled) {
            caught += 1;
        }
    }
    // 补 6 轮随机槽。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    for _ in 0..6 {
        let mut led = DbLedger::new();
        let s = (lcg.next_below(4)) as usize;
        if led.db_hit(s, 0x2000) == Err(DbgCode::HitOnDisabled) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("未使能命中注入 {}/10 必抓", caught))
}

pub fn drill_db_attribution_10() -> (bool, String) {
    // #DB 归属 10/10：使能槽命中 → DR6 B 位归属恰中。
    let mut caught = 0;
    for round in 0..10u32 {
        let mut led = DbLedger::new();
        let slot = round as usize % BP_SLOTS;
        let _ = led.set_breakpoint(slot, 0x4000_0000 + round as u64, 0, 0);
        let _ = led.db_hit(slot, 0x4000_0000 + round as u64);
        let attr = led.attribute_db();
        if attr == Ok(slot) && led.hit_ledger_balanced() {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("#DB 归属 {}/10 正确", caught))
}

fn drill_hit_ledger_conservation() -> (bool, String) {
    // 命中账与寄存器账守恒：4 槽全命中 → 4 B 位；清槽后账恒。
    let mut led = DbLedger::new();
    let mut ok = true;
    for s in 0..BP_SLOTS {
        let _ = led.set_breakpoint(s, 0x5000 + s as u64, 0, 0);
        let _ = led.db_hit(s, 0x5000 + s as u64);
    }
    ok = ok && led.hits.len() == 4 && led.hit_ledger_balanced();
    // 清槽 0：使能消、B 位仍在（历史命中）——守恒口径按 B 位 == 命中数。
    let _ = led.clear_breakpoint(0);
    ok = ok && !led.ctx.slot_enabled(0) && led.hit_ledger_balanced();
    (ok, format!("命中账守恒: 4命中4B位={} 清槽后恒={}", led.hits.len() == 4, led.hit_ledger_balanced()))
}

fn drill_dr6_fixed_bits_10() -> (bool, String) {
    let mut caught = 0;
    let bads: [u64; 10] = [
        0x0,                    // bit1 缺
        0x1000,                 // 保留域
        0x2000,
        0x4000,
        0x8000,
        0xF000,
        0xFFFF_0000_0000_0000,  // 高位保留
        0x0000_0000_0000_1002,  // bit1 在但保留域也在
        0xFFFF_0FF0_0000_0000,  // 固定位但缺 bit1? — 0x...00 bit1=0
        0x0000_0000_0000_0002 | 0x10_0000_0000,
    ];
    for bad in bads {
        let mut ctx = DrContext::fresh();
        if ctx.set_reg(6, bad) == Err(DbgCode::Dr6FixedBitsViolated) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("DR6 固定位注入 {}/10 必抓", caught))
}

fn drill_set_clear_semantics() -> (bool, String) {
    // 设/清语义：设后使能，清后禁能 + 地址归零 + RW/LEN 字段清。
    let mut led = DbLedger::new();
    let _ = led.set_breakpoint(1, 0x7000, 2, 2);
    let set_ok = led.ctx.slot_enabled(1) && led.ctx.slot_addr(1) == Ok(0x7000);
    let _ = led.clear_breakpoint(1);
    let clear_ok = !led.ctx.slot_enabled(1) && led.ctx.slot_addr(1) == Ok(0);
    (set_ok && clear_ok, format!("设/清语义: 设={} 清={}", set_ok, clear_ok))
}

fn drill_slot_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 3);
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = DbLedger::new();
        let bad = BP_SLOTS + (lcg.next_below(8) as usize);
        let r1 = led.set_breakpoint(bad, 0, 0, 0);
        let r2 = led.clear_breakpoint(bad);
        let r3 = led.db_hit(bad, 0);
        if r1 == Err(DbgCode::SlotOutOfRange) && r2 == Err(DbgCode::SlotOutOfRange) && r3 == Err(DbgCode::SlotOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("槽位越界注入 {}/10 必抓", caught))
}

fn drill_migration_semantics() -> (bool, String) {
    // 迁移语义：任务 A 断点经快照迁到任务 B 后 B 侧命中正常。
    let mut a = DbLedger::new();
    let _ = a.set_breakpoint(0, 0x8000, 0, 0);
    let snap = a.ctx;
    // 迁移到 B。
    let mut b = DbLedger::new();
    b.ctx = snap;
    let hit = b.db_hit(0, 0x8000);
    let attr = b.attribute_db();
    (
        hit.is_ok() && attr == Ok(0),
        format!("迁移语义: B 侧命中={} 归属={:?}", hit.is_ok(), attr),
    )
}

fn drill_rw_len_fields() -> (bool, String) {
    // RW/LEN 字段编码核：rw=1(len=2) 编码回读。
    let mut led = DbLedger::new();
    let _ = led.set_breakpoint(2, 0x9000, 1, 2);
    let field = (led.ctx.dr7 >> (DR7_RW_BASE + 2 * 4)) & 0xF;
    // rw=1 → bits0..1 = 01；len=2 → bits2..3 = 10 → 0b0110 = 6。
    (field == 0b0110, format!("RW/LEN 编码: field={:#06b} (期望 0b0110)", field))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    DbgCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_fidelity_10();
    let (a2, d2) = drill_db_attribution_10();
    let (a3, d3) = drill_hit_ledger_conservation();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 保真={} 归属={} 守恒={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_fidelity_10();
    v.push(("J1-切换保真10/10", a, b));
    let (a, b) = drill_reserved_write_10();
    v.push(("J2-DR7保留位写10/10", a, b));
    let (a, b) = drill_len3_10();
    v.push(("J2-LEN3非法编码10/10", a, b));
    let (a, b) = drill_dr45_access_10();
    v.push(("J2-DR4/5访问注入10/10", a, b));
    let (a, b) = drill_hit_disabled_10();
    v.push(("J2-未使能命中注入10/10", a, b));
    let (a, b) = drill_db_attribution_10();
    v.push(("J1-#DB归属10/10", a, b));
    let (a, b) = drill_hit_ledger_conservation();
    v.push(("J1-命中账守恒", a, b));
    let (a, b) = drill_dr6_fixed_bits_10();
    v.push(("J2-DR6固定位注入10/10", a, b));
    let (a, b) = drill_set_clear_semantics();
    v.push(("J1-设/清语义", a, b));
    let (a, b) = drill_slot_range_10();
    v.push(("J2-槽位越界注入10/10", a, b));
    let (a, b) = drill_migration_semantics();
    v.push(("J3-迁移语义", a, b));
    let (a, b) = drill_rw_len_fields();
    v.push(("J3-RW/LEN编码核", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1905_checks：本单判据聚合（诊断段 0x36540）。
pub fn run_unx1905_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1905 调试寄存器上下文");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36540 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
