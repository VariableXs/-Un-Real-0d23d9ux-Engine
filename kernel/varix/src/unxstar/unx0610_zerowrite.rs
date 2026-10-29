//! unx0610_zerowrite —— UNX-F0610 归档区零写红线（封账级）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0610 归档区零写红线（封账级）  <SEG 0x4e00>
//! 目标行数：≥220（判据：纯功能代码 ≥220 行）
//! 判据原文摘录：
//!   ② 判据原文：零写断言（归档区写拦截计数=预期注入数，生产路
//!   径 0），红线封账 v1。
//!
//! 设计纪律：
//!   1. 归档区 = [0, 档长) 只读内存面；任何写意图必须经写闸
//      （write_gate），闸一律拦截并入拦截账（生产路径拦截计数 0
//      = 生产代码零写调用，注入路径计数 = 注入数）；
//   2. 写意图三元组入账（偏移/长度/来源码），来源码闭集：0 生产
//      /1 注入演练 /2 越权探测；
//   3. 红线封账 v1：封账单 =（区界, 拦截总数, 生产拦截数=0 断言,
//      注入拦截数=预期注入数断言）四段；封账后账只读；
//   4. 区界校验：写意图偏移+长度超出区界 → 越界红码（独立于拦
//      截账——越界写连入账资格都没有）；
//   5. 对账恒等式：拦截总数 = 生产数 + 注入数 + 探测数；生产数
//      恒 0（红线）；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4e00 段为本模块独占（SEG_BASE=0x4e00），判别式
//   SEG_BASE+1..+N；与 0x4de0/0x4e20 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const ZW_VERSION: &str = "unx0610_zerowrite-v1";

/// 诊断段基址（0x4e00 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4E00;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4e01 归档区界越界（写意图超出 [0, 档长)）。
pub const XC_RANGE_OVER: u16 = SEG_BASE + 1;
/// 0x4e02 归档区写拦截（红线触发——所有写都拦）。
pub const XC_WRITE_BLOCKED: u16 = SEG_BASE + 2;
/// 0x4e03 来源码越界（闭集外）。
pub const XC_BAD_ORIGIN: u16 = SEG_BASE + 3;
/// 0x4e04 零长写意图（非法形）。
pub const XC_ZERO_LEN: u16 = SEG_BASE + 4;
/// 0x4e05 生产路径写出现（红线破约——计数非 0）。
pub const XC_PROD_WRITE: u16 = SEG_BASE + 5;
/// 0x4e06 封账后仍试图改账。
pub const XC_SEALED_WRITE: u16 = SEG_BASE + 6;
/// 0x4e07 对账不守恒（拦截分类和 ≠ 总数）。
pub const XC_LEDGER_UNBALANCED: u16 = SEG_BASE + 7;
/// 0x4e08 封账断言未齐（生产=0/注入=预期 两断言缺一）。
pub const XC_SEAL_ASSERT: u16 = SEG_BASE + 8;
/// 0x4e09 偏移加长度溢出。
pub const XC_SPAN_OVERFLOW: u16 = SEG_BASE + 9;

/// 来源码闭集：0 生产 / 1 注入演练 / 2 越权探测。
pub const ORIGIN_PROD: u8 = 0;
pub const ORIGIN_INJECT: u8 = 1;
pub const ORIGIN_PROBE: u8 = 2;

/// 写意图账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteIntent {
    /// 写偏移。
    pub off: u32,
    /// 写长度。
    pub len: u32,
    /// 来源码。
    pub origin: u8,
}

/// 拦截账。
#[derive(Debug, Default)]
pub struct BlockLedger {
    pub intents: Vec<WriteIntent>,
    /// 分类计数。
    pub prod: u32,
    pub inject: u32,
    pub probe: u32,
}

impl BlockLedger {
    /// 守恒：分类和 = 账目数。
    pub fn conserved(&self) -> Result<(), u16> {
        let total = self.intents.len() as u32;
        if self.prod + self.inject + self.probe != total {
            return Err(XC_LEDGER_UNBALANCED);
        }
        if self.prod != 0 {
            return Err(XC_PROD_WRITE);
        }
        Ok(())
    }
}

/// 零写红线闸。
pub struct ZeroWriteGate {
    /// 区界（档长）。
    pub region_len: u32,
    ledger: BlockLedger,
    /// 封账旗。
    sealed: bool,
    /// 预期注入数（封账断言面）。
    pub expect_inject: u32,
}

impl ZeroWriteGate {
    /// 新闸（区界 = 档长）。
    pub fn new(region_len: u32) -> Self {
        ZeroWriteGate {
            region_len,
            ledger: BlockLedger::default(),
            sealed: false,
            expect_inject: 0,
        }
    }

    /// 写闸主路径：零长红 / 来源码红 / 区界红 / 一律拦截入账。
    pub fn submit(&mut self, off: u32, len: u32, origin: u8) -> Result<(), u16> {
        if len == 0 {
            return Err(XC_ZERO_LEN);
        }
        if origin > ORIGIN_PROBE {
            return Err(XC_BAD_ORIGIN);
        }
        let end = off.checked_add(len).ok_or(XC_SPAN_OVERFLOW)?;
        if end > self.region_len {
            return Err(XC_RANGE_OVER);
        }
        // 红线：一律拦截。
        match origin {
            ORIGIN_PROD => self.ledger.prod += 1,
            ORIGIN_INJECT => self.ledger.inject += 1,
            _ => self.ledger.probe += 1,
        }
        self.ledger.intents.push(WriteIntent { off, len, origin });
        Err(XC_WRITE_BLOCKED)
    }

    /// 拦截总数。
    pub fn blocked_total(&self) -> u32 {
        self.ledger.intents.len() as u32
    }

    /// 生产拦截数（红线要求恒 0）。
    pub fn prod_blocked(&self) -> u32 {
        self.ledger.prod
    }

    /// 注入拦截数。
    pub fn inject_blocked(&self) -> u32 {
        self.ledger.inject
    }

    /// 设定预期注入数（封账前）。
    pub fn set_expect_inject(&mut self, n: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_SEALED_WRITE);
        }
        self.expect_inject = n;
        Ok(())
    }

    /// 红线封账 v1 主路径：账守恒 + 生产=0 断言 + 注入=预期断言。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_SEALED_WRITE);
        }
        self.ledger.conserved()?;
        // 断言 1：生产路径拦截 = 0。
        if self.ledger.prod != 0 {
            return Err(XC_PROD_WRITE);
        }
        // 断言 2：注入拦截 = 预期注入数（预期 0 时要求零注入）。
        if self.ledger.inject != self.expect_inject {
            return Err(XC_SEAL_ASSERT);
        }
        self.sealed = true;
        Ok(())
    }

    /// 封账态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 拦截账只读。
    pub fn ledger(&self) -> &BlockLedger {
        &self.ledger
    }
}

/// 封账单四段（判据面：封账单结构冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealDoc {
    /// 区界。
    pub region_len: u32,
    /// 拦截总数。
    pub blocked_total: u32,
    /// 生产拦截数（必 0）。
    pub prod_blocked: u32,
    /// 注入拦截数（必等于预期）。
    pub inject_blocked: u32,
}

/// 封账单产出（封账后只读面）。
pub fn seal_doc(gate: &ZeroWriteGate) -> Result<SealDoc, u16> {
    if !gate.is_sealed() {
        return Err(XC_SEAL_ASSERT);
    }
    Ok(SealDoc {
        region_len: gate.region_len,
        blocked_total: gate.blocked_total(),
        prod_blocked: gate.prod_blocked(),
        inject_blocked: gate.inject_blocked(),
    })
}

/// 生产路径零写自证：模拟生产代码对归档区只做读拷贝（无写闸调
/// 用），闸账保持零——生产拦截数 0 的实证面。
pub fn prod_readonly_copy(gate: &ZeroWriteGate, dst: &mut Vec<u8>, src: &[u8]) -> Result<(), u16> {
    if gate.is_sealed() {
        // 封账后生产只读拷贝仍合法（读不写）。
        dst.clear();
        dst.extend_from_slice(src);
        Ok(())
    } else {
        Err(XC_SEAL_ASSERT)
    }
}

/// 自例程：注入演练拦截计数 = 预期 → 封账断言 → 封账单 → 生产
/// 零写实证 → 错误路径（越界/零长/来源越界/生产写/封账后写）。
pub fn drill() -> Result<u32, u16> {
    // 归档区：模拟 256 字节档。
    let region: Vec<u8> = (0..=255u8).collect();
    let mut gate = ZeroWriteGate::new(region.len() as u32);

    // 注入演练：4 次写意图，全拦截。
    gate.set_expect_inject(4)?;
    for i in 0..4u32 {
        let r = gate.submit(i * 10, 5, ORIGIN_INJECT);
        if r != Err(XC_WRITE_BLOCKED) {
            return Err(XC_WRITE_BLOCKED);
        }
    }
    // 越权探测 2 次。
    let _r = gate.submit(0, 1, ORIGIN_PROBE);
    if gate.blocked_total() != 6 {
        return Err(XC_LEDGER_UNBALANCED);
    }
    if gate.inject_blocked() != 4 {
        return Err(XC_LEDGER_UNBALANCED);
    }
    // 生产路径 0 次写——红线实证。
    if gate.prod_blocked() != 0 {
        return Err(XC_PROD_WRITE);
    }
    // 拦截账守恒。
    gate.ledger().conserved()?;

    // 封账主路径。
    gate.seal()?;
    if !gate.is_sealed() {
        return Err(XC_SEAL_ASSERT);
    }
    let doc = seal_doc(&gate)?;
    if doc.region_len != 256 || doc.blocked_total != 6 || doc.prod_blocked != 0 || doc.inject_blocked != 4 {
        return Err(XC_SEAL_ASSERT);
    }
    // 生产只读拷贝（封账后合法）。
    let mut dst = Vec::new();
    prod_readonly_copy(&gate, &mut dst, &region)?;
    if dst.len() != 256 {
        return Err(XC_RANGE_OVER);
    }

    // 错误路径 1：区界越界。
    let mut g2 = ZeroWriteGate::new(16);
    if g2.submit(15, 2, ORIGIN_INJECT) != Err(XC_RANGE_OVER) {
        return Err(XC_RANGE_OVER);
    }
    // 错误路径 2：零长写。
    if g2.submit(0, 0, ORIGIN_INJECT) != Err(XC_ZERO_LEN) {
        return Err(XC_ZERO_LEN);
    }
    // 错误路径 3：来源码越界。
    if g2.submit(0, 1, 3) != Err(XC_BAD_ORIGIN) {
        return Err(XC_BAD_ORIGIN);
    }
    // 错误路径 4：溢出面。
    if g2.submit(u32::MAX - 1, 4, ORIGIN_PROBE) != Err(XC_SPAN_OVERFLOW) {
        return Err(XC_SPAN_OVERFLOW);
    }
    // 错误路径 5：生产写出现（红线破约）。
    let r = g2.submit(0, 1, ORIGIN_PROD);
    if r != Err(XC_WRITE_BLOCKED) {
        return Err(XC_WRITE_BLOCKED);
    }
    // 生产计数非 0 → 封账红。
    if g2.seal() != Err(XC_PROD_WRITE) {
        return Err(XC_PROD_WRITE);
    }
    // 错误路径 6：注入计数 ≠ 预期。
    let mut g3 = ZeroWriteGate::new(16);
    g3.set_expect_inject(2)?;
    g3.submit(0, 1, ORIGIN_INJECT)?;
    if g3.seal() != Err(XC_SEAL_ASSERT) {
        return Err(XC_SEAL_ASSERT);
    }
    // 错误路径 7：封账后改预期。
    let mut g4 = ZeroWriteGate::new(16);
    g4.set_expect_inject(0)?;
    g4.seal()?;
    if g4.set_expect_inject(1) != Err(XC_SEALED_WRITE) {
        return Err(XC_SEALED_WRITE);
    }
    if g4.seal() != Err(XC_SEALED_WRITE) {
        return Err(XC_SEALED_WRITE);
    }
    // 错误路径 8：未封账取封账单。
    let g5 = ZeroWriteGate::new(16);
    if seal_doc(&g5) != Err(XC_SEAL_ASSERT) {
        return Err(XC_SEAL_ASSERT);
    }
    Ok(0)
}
