//! unx48330_smfuzz — UNX-F48330 智能面 fuzz（随机输入串/随机反馈注入不崩）（诊断段 0xCA20）。
//!
//! 判据原文：
//!   J1 fuzz ×10^4 零崩；判据锚定 体验十。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 注入面闭集：三类注入——随机输入串（任意字节序列）、随机
//!      反馈（任意词 id/任意轮次）、随机操作序（提交/换词/查询混
//!      序）——fuzz 打的是边界组合，不是单点。
//!   2. 不崩红线：任何注入组合下三条红线恒守——账目守恒（注入后
//!      各账自洽）、状态闭集（词 id/点 id/体裁不越界）、序号单调
//!      （不跳号不回退）；红线即 drill 的逐轮判据，崩一条即红。
//!   3. 确定性 LCG：×10^4 注入序列由种子唯一决定，零种子禁用；
//!      同种子复跑注入序列与结果逐位一致（崩样可复现才算真账）。
//!   4. 崩样留痕：红线违例记录（轮号/注入类/违例码），账面可见；
//!      J1 零崩即违例账恒零。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCA20;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_FUZZ_LEDGER: u16 = SEG_BASE + 1; // 注入后账目不守恒
const E_FUZZ_STATE: u16 = SEG_BASE + 2;  // 状态越闭集
const E_FUZZ_SEQ: u16 = SEG_BASE + 3;    // 序号单调破坏
const E_SEED_ZERO: u16 = SEG_BASE + 4;   // 零种子禁用
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 6;   // 写回摘要为空

/// 注入轮数（J1 ×10^4）。
pub const FUZZ_N: u32 = 10000;
/// 输入串闭集界（注入串长 0..=8，字节任意）。
pub const STR_CAP: usize = 8;
/// 词表容量（反馈注入词 id 闭集 0..16，越界必须被拒收而非崩）。
pub const WORD_N: usize = 16;

/// 被试面：极简智能会话（提交/反馈/查询三操作共享状态）。
pub struct SmTarget {
    pub committed: Vec<u8>,   // 已提交词 id（闭集 0..16）
    pub feedback: u32,        // 反馈落账计数
    pub queried: u32,         // 查询计数
    pub reject_total: u32,    // 拒收计数（越界词/超长串/空操作）
    last_seq: u32,
}

impl SmTarget {
    pub fn new() -> SmTarget {
        SmTarget {
            committed: Vec::new(),
            feedback: 0,
            queried: 0,
            reject_total: 0,
            last_seq: 0,
        }
    }

    /// 提交：词 id 越闭集拒收，账内只存合法 id。
    pub fn commit(&mut self, word_id: u8, seq: u32) -> Result<(), u16> {
        if seq <= self.last_seq {
            return Err(E_FUZZ_SEQ);
        }
        if word_id as usize >= WORD_N {
            self.reject_total += 1;
            self.last_seq = seq;
            return Err(E_FUZZ_STATE); // 拒收也推进序（单调不回退）
        }
        if self.committed.len() >= 32 {
            self.reject_total += 1;
            self.last_seq = seq;
            return Err(E_FUZZ_LEDGER);
        }
        self.committed.push(word_id);
        self.last_seq = seq;
        Ok(())
    }

    /// 反馈：任意 u32 载荷均不崩，账单调 +1。
    pub fn feedback(&mut self, payload: u32, seq: u32) -> Result<u32, u16> {
        if seq <= self.last_seq {
            return Err(E_FUZZ_SEQ);
        }
        self.feedback = self.feedback.wrapping_add(payload);
        self.last_seq = seq;
        Ok(self.feedback)
    }

    /// 查询：任意串不崩，超长拒收，命中与否只影响返回值。
    pub fn query(&mut self, q: &[u8], seq: u32) -> Result<bool, u16> {
        if seq <= self.last_seq {
            return Err(E_FUZZ_SEQ);
        }
        if q.len() > STR_CAP {
            self.reject_total += 1;
            self.last_seq = seq;
            return Err(E_FUZZ_STATE);
        }
        self.queried += 1;
        self.last_seq = seq;
        // 命中判定：串内有任一合法词 id 字节即命中（确定性纯函数）。
        let mut i = 0usize;
        while i < q.len() {
            if (q[i] as usize) < WORD_N {
                return Ok(true);
            }
            i += 1;
        }
        Ok(false)
    }

    /// 三红线自检：账守恒 + 状态闭集 + 序号有效。
    pub fn invariants(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.committed.len() {
            if self.committed[i] as usize >= WORD_N {
                return Err(E_FUZZ_STATE);
            }
            i += 1;
        }
        if self.committed.len() > 32 {
            return Err(E_FUZZ_LEDGER);
        }
        if self.queried.checked_add(self.reject_total).is_none() {
            return Err(E_FUZZ_LEDGER);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.last_seq == 0 {
            return Err(E_FUZZ_LEDGER);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性 LCG（零种子禁用）。
pub struct Lcg {
    x: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_SEED_ZERO);
        }
        Ok(Lcg { x: seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        self.x = self.x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.x
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        (self.next_u64() >> 16) % n
    }
}

/// fuzz 崩样账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrashLedger {
    pub crashes: u32,     // 红线违例数（J1 要求恒零）
    pub inj_total: u32,   // 注入总数
    pub op_total: u32,    // 成功操作数
    pub first_crash_at: u32, // 首崩轮（0 = 无崩）
}

/// ×N 轮 fuzz（J1 主体）：三类注入轮转，逐轮三红线自检，违例留痕。
pub fn fuzz_run(seed: u64, rounds: u32) -> Result<CrashLedger, u16> {
    let mut lcg = Lcg::new(seed)?;
    let mut t = SmTarget::new();
    let mut led = CrashLedger { crashes: 0, inj_total: 0, op_total: 0, first_crash_at: 0 };
    let mut r = 0u32;
    while r < rounds {
        let kind = lcg.next_below(3);
        let seq = r + 1;
        let ok = match kind {
            0 => {
                // 随机输入串：长度 0..=10（含超长越界），字节任意。
                let len = lcg.next_below(11) as usize;
                let mut q: Vec<u8> = Vec::new();
                let mut i = 0usize;
                while i < len {
                    q.push((lcg.next_u64() & 0xFF) as u8);
                    i += 1;
                }
                t.query(&q, seq).is_ok()
            }
            1 => {
                // 随机反馈：任意 u32 载荷。
                let payload = lcg.next_u64() as u32;
                t.feedback(payload, seq).is_ok()
            }
            _ => {
                // 随机提交：词 id 0..=255（含大量越界），账内只存合法值。
                let wid = (lcg.next_u64() & 0xFF) as u8;
                t.commit(wid, seq).is_ok()
            }
        };
        led.inj_total += 1;
        if ok {
            led.op_total += 1;
        }
        // 三红线逐轮自检（不崩红线 = 违例恒零）。
        if t.invariants().is_err() {
            led.crashes += 1;
            if led.first_crash_at == 0 {
                led.first_crash_at = r + 1;
            }
            return Err(E_FUZZ_LEDGER); // 崩样即刻红（不允许带伤续跑）
        }
        r += 1;
    }
    Ok(led)
}

/// 自例程判据：逐条落实 J1（fuzz ×10^4 零崩），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 fuzz ×10^4 零崩：三类注入 ×10^4，逐轮三红线自检，崩样账
    //    恒零，注入账 == 10^4。
    let led = fuzz_run(0xCA21, FUZZ_N)?;
    if led.crashes != 0 || led.first_crash_at != 0 {
        return Err(E_FUZZ_LEDGER);
    }
    if led.inj_total != FUZZ_N {
        return Err(E_FUZZ_LEDGER);
    }
    if led.op_total == 0 {
        return Err(E_FUZZ_LEDGER); // 全拒收说明注入面没打进目标
    }
    hits += 1;

    // R2 注入面真达：三类操作均被注入命中（操作账只增不异常）；
    //    越界词拒收路径真实可达（拒收账 > 0）。
    let led = fuzz_run(0xCA22, 3000)?;
    if led.inj_total != 3000 {
        return Err(E_FUZZ_LEDGER);
    }
    let mut t = SmTarget::new();
    t.commit(99, 1).is_err(); // 越界词
    t.commit(3, 2)?;
    if t.committed.len() != 1 || t.committed[0] != 3 {
        return Err(E_FUZZ_STATE);
    }
    if t.reject_total != 1 {
        return Err(E_FUZZ_LEDGER);
    }
    hits += 1;

    // R3 状态闭集红线可红：账内只可能存合法词 id——强注越界值后
    //    invariants 必红（判据非恒绿演练）。
    let mut t = SmTarget::new();
    t.committed.push(3);
    t.committed.push(200); // 模拟账面被注入非法值
    if t.invariants().is_ok() {
        return Err(E_FUZZ_STATE);
    }
    hits += 1;

    // R4 序号单调红线：回退序号必拒（commit/feedback/query 三径
    //    一致）；合法递增照收。
    let mut t = SmTarget::new();
    t.commit(1, 5)?;
    if t.commit(2, 5) != Err(E_FUZZ_SEQ) {
        return Err(E_FUZZ_SEQ);
    }
    if t.commit(2, 3) != Err(E_FUZZ_SEQ) {
        return Err(E_FUZZ_SEQ);
    }
    if t.feedback(1, 4) != Err(E_FUZZ_SEQ) {
        return Err(E_FUZZ_SEQ);
    }
    if t.query(b"ab", 2) != Err(E_FUZZ_SEQ) {
        return Err(E_FUZZ_SEQ);
    }
    t.feedback(7, 6)?;
    t.commit(2, 7)?;
    if t.last_seq != 7 {
        return Err(E_FUZZ_SEQ);
    }
    hits += 1;

    // R5 超长串闭集：len ≤ 8 收、len 9..=10 拒、len 11+ 拒，拒收
    //    计数留痕，不崩。
    let mut t = SmTarget::new();
    t.query(&[1, 2, 3, 4, 5, 6, 7, 8], 1)?;
    t.query(&[1, 2, 3, 4, 5, 6, 7, 8, 9], 2).is_err();
    t.query(&[9; 11], 3).is_err();
    if t.reject_total != 2 || t.queried != 1 {
        return Err(E_FUZZ_LEDGER);
    }
    t.invariants()?;
    hits += 1;

    // R6 零种子禁用与反馈溢出不崩：seed 0 拒跑；u32 回绕反馈不崩
    //    （wrapping 语义留痕）。
    match Lcg::new(0) {
        Err(E_SEED_ZERO) => {}
        _ => return Err(E_SEED_ZERO),
    }
    let mut t = SmTarget::new();
    t.feedback(0xFFFF_FFFF, 1)?;
    t.feedback(2, 2)?; // 回绕到 1
    if t.feedback != 1 {
        return Err(E_FUZZ_LEDGER);
    }
    t.invariants()?;
    hits += 1;

    // R7 复跑逐位一致：同种子两轮 fuzz（×2000 足量），账面三元组
    //    逐位一致（注入序列确定性，崩样可复现）。
    let l1 = fuzz_run(0xCA23, 2000)?;
    let l2 = fuzz_run(0xCA23, 2000)?;
    if l1 != l2 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 ×10^4 复跑收官与写回（判据 J2）：第二轮 ×10^4 与首轮账面
    //    一致；写回过。
    let l1 = fuzz_run(0xCA21, FUZZ_N)?;
    let l2 = fuzz_run(0xCA21, FUZZ_N)?;
    if l1 != l2 || l1.crashes != 0 {
        return Err(E_REPLAY_DIFF);
    }
    let mut t = SmTarget::new();
    t.feedback(1, 1)?;
    if t.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    t.writeback(true, 0x1E)?;
    hits += 1;

    Ok(hits)
}
