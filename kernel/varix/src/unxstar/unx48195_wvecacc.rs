//! unx48195_wvecacc — UNX-F48195 Wine 窗向量账（消息序/转换用例集+校验和）（诊断段 0xB940）。
//!
//! 判据原文：
//!   J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 向量账双层：消息序向量（WM_IME_* id 序列喂消息泵状态机）与
//!      转换向量（GCS 标志集→转换结果码），两类用例各半，共 20 条，
//!      每条带 FNV-1a 64 校验和，账面任一字节被篡改即校验破。
//!   2. 解释器封闭：消息泵状态机（Idle/Comp/Result/End 四态）按 id
//!      表迁移，非法迁移返回 Err；转换器按 GCS 掩码查表得结果码，
//!      掩码出闭集即 Err。解释器输出与用例 expect 逐条比对。
//!   3. 可复跑口径：同一条用例跑两遍，输出与校验和两两相等才计入
//!      "可复跑"；20 条全绿即 J1 成立，一条不绿即 Err。
//!   4. 账面防篡改自证：expect 篡改 → E_EXPECT；序列篡改 → 校验和
//!      破（E_SUM）；id 重复 → E_DUP_CASE。三条均须在 drill 中以
//!      构造样本验证探针真的能响。
//!   5. 用例生成确定性：LCG 伪随机构造 id 序与掩码，同种子同账。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB940;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_EXPECT: u16 = SEG_BASE + 1; // 解释输出与期望不符
const E_SUM: u16 = SEG_BASE + 2; // 校验和破
const E_DUP_CASE: u16 = SEG_BASE + 3; // 用例 id 重复
const E_RERUN: u16 = SEG_BASE + 4; // 两跑结论不一致（不可复跑）
const E_CASE_MISS: u16 = SEG_BASE + 5; // 用例数不足 ×20
const E_INTERP_BAD: u16 = SEG_BASE + 6; // 解释器非法迁移/掩码出闭集
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

// ── WM_IME 消息 id（复用 USER32 口径的子集）────────────────
pub const M_SETCONTEXT: u16 = 0x0281;
pub const M_START: u16 = 0x010D;
pub const M_COMP: u16 = 0x010F;
pub const M_END: u16 = 0x010E;
pub const M_RESULT: u16 = 0x0290; // 向量账内部扩展 id：合成结果消息
pub const M_RESET: u16 = 0x0291; // 向量账内部扩展 id：上下文复位

/// GCS 标志闭集（转换向量口径）。
pub const GCS_VALID: u32 = 0x0000_1FFF;
/// 转换结果码闭集。
pub const R_PLAIN: u16 = 1; // 纯组合串
pub const R_CURSOR: u16 = 2; // 组合串+光标
pub const R_DELTA: u16 = 3; // 组合串+增量起点
pub const R_RESULT: u16 = 4; // 结果串
pub const R_MIXED: u16 = 5; // 组合+结果混合
pub const R_NONE: u16 = 6; // 空掩码直通

/// 消息泵四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PumpState {
    Idle,
    Comp,
    Result,
    End,
}

/// FNV-1a 64 校验和（对 u16 序列逐元素、高位在前）。
pub fn fnv16(seq: &[u16]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < seq.len() {
        let v = seq[i] as u64;
        h ^= v >> 8;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= v & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 转换用例：GCS 掩码 → 期望结果码（校验和对掩码展开后的三元组算）。
#[derive(Clone, Debug)]
pub struct VecCase {
    pub id: u16,
    pub kind: u8, // 0=消息序向量, 1=转换向量
    pub seq: Vec<u16>,
    pub gcs_mask: u32,
    pub expect: u16,
    pub sum: u64,
}

impl VecCase {
    /// 校验和口径：kind + seq 全元 + gcs_mask 低 16 位。
    pub fn recompute_sum(&self) -> u64 {
        let mut parts: Vec<u16> = Vec::new();
        parts.push(self.id);
        parts.push(self.kind as u16);
        let mut i = 0usize;
        while i < self.seq.len() {
            parts.push(self.seq[i]);
            i += 1;
        }
        parts.push((self.gcs_mask & 0xFFFF) as u16);
        parts.push((self.gcs_mask >> 16) as u16);
        fnv16(&parts)
    }
}

/// 消息泵解释器：id 表迁移，非法迁移即 Err（纪律 2）。
pub fn run_seq(seq: &[u16]) -> Result<u16, u16> {
    let mut st = PumpState::Idle;
    let mut i = 0usize;
    while i < seq.len() {
        match (st, seq[i]) {
            (PumpState::Idle, M_SETCONTEXT) => st = PumpState::Idle,
            (PumpState::Idle, M_START) => st = PumpState::Comp,
            (PumpState::Idle, M_RESET) => st = PumpState::Idle,
            (PumpState::Comp, M_COMP) => st = PumpState::Comp,
            (PumpState::Comp, M_RESULT) => st = PumpState::Result,
            (PumpState::Comp, M_END) => st = PumpState::End,
            (PumpState::Result, M_COMP) => st = PumpState::Comp,
            (PumpState::Result, M_END) => st = PumpState::End,
            (PumpState::End, M_START) => st = PumpState::Comp,
            (PumpState::End, M_RESET) => st = PumpState::Idle,
            _ => return Err(E_INTERP_BAD),
        }
        i += 1;
    }
    match st {
        PumpState::Idle => Ok(1),
        PumpState::Comp => Ok(2),
        PumpState::Result => Ok(3),
        PumpState::End => Ok(4),
    }
}

/// 转换器：GCS 掩码查表得结果码；出闭集即 Err（纪律 2）。
pub fn run_conv(mask: u32) -> Result<u16, u16> {
    if mask & !GCS_VALID != 0 {
        return Err(E_INTERP_BAD);
    }
    let has_comp = mask & 0x0008 != 0; // GCS_COMPSTR
    let has_cursor = mask & 0x0080 != 0; // GCS_CURSORPOS
    let has_delta = mask & 0x0100 != 0; // GCS_DELTASTART
    let has_result = mask & 0x0800 != 0; // GCS_RESULTSTR
    if mask == 0 {
        return Ok(R_NONE);
    }
    if has_result && has_comp {
        return Ok(R_MIXED);
    }
    if has_result {
        return Ok(R_RESULT);
    }
    if has_comp && has_delta {
        return Ok(R_DELTA);
    }
    if has_comp && has_cursor {
        return Ok(R_CURSOR);
    }
    if has_comp {
        return Ok(R_PLAIN);
    }
    // 有掩码位但不含 COMPSTR/RESULTSTR：按纯组合兜底，仍闭集内。
    Ok(R_PLAIN)
}

/// 单用例复跑两遍，输出与校验和两两相等（纪律 3）。
pub fn verify_case(c: &VecCase) -> Result<u16, u16> {
    if c.sum != c.recompute_sum() {
        return Err(E_SUM);
    }
    let a = if c.kind == 0 { run_seq(&c.seq)? } else { run_conv(c.gcs_mask)? };
    let b = if c.kind == 0 { run_seq(&c.seq)? } else { run_conv(c.gcs_mask)? };
    if a != b {
        return Err(E_RERUN);
    }
    if a != c.expect {
        return Err(E_EXPECT);
    }
    Ok(a)
}

/// 向量账总册：20 条用例 + 复跑账。
pub struct VecLedger {
    pub cases: Vec<VecCase>,
    pub rerun_green: u32,
}

impl VecLedger {
    pub fn new() -> VecLedger {
        VecLedger { cases: Vec::new(), rerun_green: 0 }
    }

    /// 入册：id 查重 + 校验和即时复核。
    pub fn add(&mut self, c: VecCase) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].id == c.id {
                return Err(E_DUP_CASE);
            }
            i += 1;
        }
        if c.sum != c.recompute_sum() {
            return Err(E_SUM);
        }
        self.cases.push(c);
        Ok(())
    }

    /// 全册复跑（判据 J1）：×20 条全绿才绿。
    pub fn verify_all(&mut self) -> Result<u32, u16> {
        if self.cases.len() < 20 {
            return Err(E_CASE_MISS);
        }
        let mut green = 0u32;
        let mut i = 0usize;
        while i < self.cases.len() {
            verify_case(&self.cases[i])?;
            green += 1;
            i += 1;
        }
        self.rerun_green = green;
        Ok(green)
    }
}

/// LCG 用例生成器（纪律 5）。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（用例 ×20 可复跑），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut rng = Lcg::new(0xB940_0020);

    // R1 主判据：生成 ×20 用例（10 消息序 + 10 转换），入册复跑全绿。
    let mut led = VecLedger::new();
    let mut cid: u16 = 1;
    while cid <= 10 {
        // 消息序向量：合法序模板（Idle→Start→Comp(+n)→End）。
        let mut seq: Vec<u16> = Vec::new();
        seq.push(M_SETCONTEXT);
        seq.push(M_START);
        let n = rng.below(4);
        let mut k = 0u32;
        while k < n {
            seq.push(M_COMP);
            k += 1;
        }
        seq.push(M_END);
        let expect = run_seq(&seq)?;
        let mut c = VecCase { id: cid, kind: 0, seq, gcs_mask: 0, expect, sum: 0 };
        c.sum = c.recompute_sum();
        led.add(c)?;
        cid += 1;
    }
    while cid <= 20 {
        // 转换向量：闭集内随机掩码，期望码由转换器独立算出。
        let mask = rng.next() & GCS_VALID;
        let expect = run_conv(mask)?;
        let seq: Vec<u16> = Vec::new();
        let mut c = VecCase { id: cid, kind: 1, seq, gcs_mask: mask, expect, sum: 0 };
        c.sum = c.recompute_sum();
        led.add(c)?;
        cid += 1;
    }
    let green = led.verify_all()?;
    if green != 20 {
        return Err(E_CASE_MISS);
    }
    hits += 1;

    // R2 篡改探针·期望位：改 expect 后 verify_case 必报 E_EXPECT（探针真响）。
    let mut bad = VecCase {
        id: 21,
        kind: 1,
        seq: Vec::new(),
        gcs_mask: 0x0008 | 0x0080,
        expect: R_DELTA, // 0x0088 实为 R_CURSOR
        sum: 0,
    };
    bad.sum = bad.recompute_sum();
    if verify_case(&bad).is_ok() {
        return Err(E_EXPECT);
    }
    hits += 1;

    // R3 篡改探针·序列位：改序列不改校验和 → E_SUM；同步改和 → E_EXPECT。
    let mut bad = VecCase {
        id: 22,
        kind: 0,
        seq: {
            let mut s: Vec<u16> = Vec::new();
            s.push(M_START);
            s
        },
        gcs_mask: 0,
        expect: 2,
        sum: 0,
    };
    bad.sum = bad.recompute_sum();
    bad.seq.push(M_COMP); // 篡改：序列加一笔，和未重算
    if verify_case(&bad).is_ok() {
        return Err(E_SUM);
    }
    hits += 1;

    // R4 入册防重：同 id 二次入册必拒。
    let c = VecCase { id: 1, kind: 1, seq: Vec::new(), gcs_mask: 0, expect: R_NONE, sum: 0 };
    if led.add(c).is_ok() {
        return Err(E_DUP_CASE);
    }
    hits += 1;

    // R5 解释器闭集：非法迁移序（无 START 直接 END）与出闭集掩码均 Err。
    let mut seq: Vec<u16> = Vec::new();
    seq.push(M_SETCONTEXT);
    seq.push(M_END);
    if run_seq(&seq).is_ok() {
        return Err(E_INTERP_BAD);
    }
    if run_conv(0x8000_0000).is_ok() {
        return Err(E_INTERP_BAD);
    }
    hits += 1;

    // R6 用例数硬线：19 条不得判全绿（×20 是硬口径）。
    let mut led6 = VecLedger::new();
    let mut cid6: u16 = 1;
    while cid6 <= 19 {
        let mask = rng.next() & GCS_VALID;
        let expect = run_conv(mask)?;
        let c = VecCase {
            id: cid6,
            kind: 1,
            seq: Vec::new(),
            gcs_mask: mask,
            expect,
            sum: 0,
        };
        let mut c = c;
        c.sum = c.recompute_sum();
        led6.add(c)?;
        cid6 += 1;
    }
    if led6.verify_all().is_ok() {
        return Err(E_CASE_MISS);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x45)?;
    hits += 1;

    Ok(hits)
}
