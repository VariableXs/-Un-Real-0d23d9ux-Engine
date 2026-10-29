//! unx48324_miscommit — UNX-F48324 误上屏纠错回退（次优候选一键换词并重算后续）（诊断段 0xC960）。
//!
//! 判据原文：
//!   J1 换词 ×100 后续重算正确；判据锚定 体验六 输入。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 上屏账：提交序列 = 已上屏词账（cand_id + 词长 + 上屏前游标），
//!      误上屏回退 = 对已上屏位一键换词——旧词出账、次优候选入账、
//!      新词长差向下游传播（后续词游标整体平移 len_delta）。
//!   2. 后续重算口径（J1 ×100）：每次换词后，全序列游标位与派生
//!      校验链（前词→本词 XOR 折叠）必须与"从零重放同序列"的独立
//!      推演逐位一致——重算不是打补丁，是整链重导出。
//!   3. 边界闭集：回退位越界拒收；换同词为 no-op（delta=0 下游不动
//!      但账目照记）；已回退位可再换（二次纠错合法）。
//!   4. 长度守恒：任一时刻 Σ词长 == 序列总长；换词只改分布不改
//!      守恒式，破坏守恒即账烂。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC960;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_NO_WORD: u16 = SEG_BASE + 1;  // 词 id 越闭集
const E_POS_BAD: u16 = SEG_BASE + 2;  // 回退位越界
const E_CHAIN_BAD: u16 = SEG_BASE + 3;// 派生校验链断裂
const E_SHIFT_BAD: u16 = SEG_BASE + 4;// 下游游标平移错误
const E_LEDGER_BAD: u16 = SEG_BASE + 5;// 长度守恒账不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 独立重放不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;// 写回摘要为空

/// 候选词表：12 词，词长 1..=4（闭集）。
pub const WORD_N: usize = 12;
pub const WORD_LEN: [u8; WORD_N] = [2, 1, 4, 2, 3, 1, 2, 4, 1, 3, 2, 1];
/// 提交序列容量。
pub const SEQ_CAP: usize = 16;
/// 换词轮数（J1）。
pub const SWAP_N: u32 = 100;

/// 已上屏词账槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Committed {
    pub word_id: u8,
    pub len: u8,
    pub cursor_before: u32, // 上屏前游标
    pub chain: u64,         // 派生校验码（前词折叠入链）
}

/// 误上屏回退面：提交账 + 一键换词 + 全链重算。
pub struct MisCommit {
    seq: Vec<Committed>,
    pub swap_total: u32,
    pub noop_total: u32,   // 换同词 no-op 账
    pub reject_total: u32,
}

impl MisCommit {
    pub fn new() -> MisCommit {
        MisCommit { seq: Vec::new(), swap_total: 0, noop_total: 0, reject_total: 0 }
    }

    /// 上屏一词（正常径）。
    pub fn commit(&mut self, word_id: u8) -> Result<u32, u16> {
        if word_id as usize >= WORD_N {
            self.reject_total += 1;
            return Err(E_NO_WORD);
        }
        if self.seq.len() >= SEQ_CAP {
            self.reject_total += 1;
            return Err(E_POS_BAD);
        }
        let cursor_before = self.total_len();
        let prev_chain = match self.seq.last() {
            Some(w) => w.chain,
            None => 0xC960_C961,
        };
        let chain = prev_chain
            ^ ((word_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .rotate_left(7);
        self.seq.push(Committed {
            word_id,
            len: WORD_LEN[word_id as usize],
            cursor_before,
            chain,
        });
        Ok(cursor_before + WORD_LEN[word_id as usize] as u32)
    }

    pub fn total_len(&self) -> u32 {
        let mut t: u32 = 0;
        let mut i = 0usize;
        while i < self.seq.len() {
            t += self.seq[i].len as u32;
            i += 1;
        }
        t
    }

    pub fn seq_len(&self) -> usize {
        self.seq.len()
    }

    pub fn word_at(&self, pos: usize) -> Result<u8, u16> {
        if pos >= self.seq.len() {
            return Err(E_POS_BAD);
        }
        Ok(self.seq[pos].word_id)
    }

    /// 一键换词（误上屏回退）：pos 位旧词换 new_id，下游全部重算。
    /// 返回换词后总长。
    pub fn replace_at(&mut self, pos: usize, new_id: u8) -> Result<u32, u16> {
        if new_id as usize >= WORD_N {
            self.reject_total += 1;
            return Err(E_NO_WORD);
        }
        if pos >= self.seq.len() {
            self.reject_total += 1;
            return Err(E_POS_BAD);
        }
        let old_len = self.seq[pos].len;
        let new_len = WORD_LEN[new_id as usize];
        if self.seq[pos].word_id == new_id {
            self.noop_total += 1; // 换同词：账照记，下游不动
            return Ok(self.total_len());
        }
        self.seq[pos].word_id = new_id;
        self.seq[pos].len = new_len;
        // 下游重算：游标平移 len_delta + 派生链全链重导。
        let delta = new_len as i32 - old_len as i32;
        let mut p = pos + 1;
        while p < self.seq.len() {
            let c = &mut self.seq[p];
            if delta >= 0 {
                c.cursor_before += delta as u32;
            } else {
                c.cursor_before -= (-delta) as u32;
            }
            p += 1;
        }
        self.rechain_from(pos)?;
        self.swap_total += 1;
        Ok(self.total_len())
    }

    /// 派生校验链从 pos 起重导（链 = 前词链 XOR 本词折叠）。
    fn rechain_from(&mut self, pos: usize) -> Result<(), u16> {
        let mut p = pos;
        let mut prev_chain = if p == 0 {
            0xC960_C961
        } else {
            self.seq[p - 1].chain
        };
        while p < self.seq.len() {
            let wid = self.seq[p].word_id;
            let ch = prev_chain
                ^ ((wid as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
                    .rotate_left(7);
            self.seq[p].chain = ch;
            prev_chain = ch;
            p += 1;
        }
        Ok(())
    }

    /// 独立重放推演：从提交根序列（换词后词序）从零重建账，
    /// 与现状逐位比对——后续重算正确性的第二条独立路径。
    pub fn verify_replay(&self) -> Result<(), u16> {
        let mut cursor: u32 = 0;
        let mut prev_chain: u64 = 0xC960_C961;
        let mut i = 0usize;
        while i < self.seq.len() {
            let w = self.seq[i];
            if w.cursor_before != cursor {
                return Err(E_SHIFT_BAD);
            }
            let want_chain = prev_chain
                ^ ((w.word_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
                    .rotate_left(7);
            if w.chain != want_chain {
                return Err(E_CHAIN_BAD);
            }
            if WORD_LEN[w.word_id as usize] != w.len {
                return Err(E_LEDGER_BAD);
            }
            cursor += w.len as u32;
            prev_chain = w.chain;
            i += 1;
        }
        if cursor != self.total_len() {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 长度守恒审计：Σlen == total_len 且槽长与词表一致。
    pub fn audit(&self) -> Result<(), u16> {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < self.seq.len() {
            let w = self.seq[i];
            if w.word_id as usize >= WORD_N {
                return Err(E_NO_WORD);
            }
            if w.len != WORD_LEN[w.word_id as usize] {
                return Err(E_LEDGER_BAD);
            }
            s += w.len as u32;
            i += 1;
        }
        if s != self.total_len() {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 序列指纹（防漂移留痕）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0xC960_C962;
        let mut i = 0usize;
        while i < self.seq.len() {
            let w = self.seq[i];
            fp ^= (w.word_id as u64) << 8;
            fp ^= w.cursor_before as u64;
            fp = fp.rotate_left(9);
            i += 1;
        }
        fp
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.swap_total == 0 {
            return Err(E_LEDGER_BAD);
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
            return Err(E_NO_WORD);
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

/// 自例程判据：逐条落实 J1（换词 ×100 后续重算正确），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 换词 ×100 后续重算正确：LCG 驱动建 8 词句，×100 轮随机位
    //    随机次优候选换词，每轮换后独立重放推演逐位一致。
    let mut mc = MisCommit::new();
    let mut lcg = Lcg::new(0xC960)?;
    let mut n = 0u32;
    while n < 8 {
        mc.commit((lcg.next_below(WORD_N as u64)) as u8)?;
        n += 1;
    }
    let mut n = 0u32;
    while n < SWAP_N {
        let pos = (lcg.next_below(mc.seq_len() as u64)) as usize;
        let new_id = (lcg.next_below(WORD_N as u64)) as u8;
        mc.replace_at(pos, new_id)?;
        mc.verify_replay()?;
        mc.audit()?;
        n += 1;
    }
    if mc.swap_total + mc.noop_total != SWAP_N {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 下游平移精确：词长表已知的定点换词——0 位换长差 +1，
    //    后继每词 cursor_before 恰 +1，首位游标不变。
    let mut mc = MisCommit::new();
    mc.commit(1)?; // len 1
    mc.commit(1)?; // len 1
    mc.commit(1)?; // len 1
    mc.replace_at(0, 0)?; // len 1 → 2，delta +1
    if mc.word_at(0)? != 0 {
        return Err(E_LEDGER_BAD);
    }
    let w1 = mc.seq_get(1)?;
    let w2 = mc.seq_get(2)?;
    if w1.cursor_before != 3 || w2.cursor_before != 4 {
        return Err(E_SHIFT_BAD);
    }
    if mc.total_len() != 4 {
        return Err(E_LEDGER_BAD);
    }
    mc.verify_replay()?;
    hits += 1;

    // R3 负 delta 平移：长词换短词，下游 cursor 恰减。
    let mut mc = MisCommit::new();
    mc.commit(2)?; // len 4
    mc.commit(2)?; // len 4
    mc.replace_at(0, 1)?; // len 4 → 1，delta -3
    let w1 = mc.seq_get(1)?;
    if w1.cursor_before != 1 {
        return Err(E_SHIFT_BAD);
    }
    if mc.total_len() != 5 {
        return Err(E_LEDGER_BAD);
    }
    mc.verify_replay()?;
    hits += 1;

    // R4 边界闭集：回退位越界拒收、词 id 越闭集拒收、换同词 no-op
    //    账照记下游不动、账分立。
    let mut mc = MisCommit::new();
    mc.commit(0)?;
    if mc.replace_at(5, 1) != Err(E_POS_BAD) {
        return Err(E_POS_BAD);
    }
    if mc.replace_at(0, 99) != Err(E_NO_WORD) {
        return Err(E_NO_WORD);
    }
    if mc.commit(50) != Err(E_NO_WORD) {
        return Err(E_NO_WORD);
    }
    if mc.replace_at(0, 0).is_err() {
        return Err(E_LEDGER_BAD);
    }
    if mc.noop_total != 1 || mc.reject_total != 3 {
        return Err(E_LEDGER_BAD);
    }
    let fp_before = mc.fingerprint();
    mc.replace_at(0, 0)?;
    if mc.fingerprint() != fp_before {
        return Err(E_SHIFT_BAD); // no-op 不得动序列
    }
    hits += 1;

    // R5 二次纠错链：同位连换两次，链与游标仍全链一致；总长守恒。
    let mut mc = MisCommit::new();
    mc.commit(3)?;
    mc.commit(4)?;
    mc.commit(5)?;
    mc.replace_at(0, 7)?; // len 2→4
    mc.verify_replay()?;
    mc.replace_at(0, 1)?; // len 4→1
    mc.verify_replay()?;
    if mc.word_at(0)? != 1 || mc.total_len() != 5 {
        return Err(E_LEDGER_BAD);
    }
    if mc.swap_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 首词换词全体平移：首词 delta 传播到每个后继（含最后一个）。
    let mut mc = MisCommit::new();
    let mut n = 0u32;
    while n < 6 {
        mc.commit(9)?; // len 3 ×6 = 18
        n += 1;
    }
    mc.replace_at(0, 11)?; // len 3→1，delta -2
    let mut p = 1usize;
    while p < mc.seq_len() {
        let w = mc.seq_get(p)?;
        if w.cursor_before != (p as u32) * 3 - 2 {
            return Err(E_SHIFT_BAD);
        }
        p += 1;
    }
    if mc.total_len() != 16 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 可红演练：链断裂检测——手工重导一槽链而不重导后继，
    //    verify_replay 必红；空摘要/零换词写回必红。
    let mut mc = MisCommit::new();
    mc.commit(0)?;
    mc.commit(2)?;
    // 正常链先验绿。
    mc.verify_replay()?;
    // 破坏中段链（模拟外部篡改），重放必红。
    mc.seq[1].chain ^= 0xFF;
    if mc.verify_replay().is_ok() {
        return Err(E_CHAIN_BAD);
    }
    if mc.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    if mc.writeback(false, 8).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 ×100 复跑指纹一致与写回（判据 J2）：同种子第二轮 ×100
    //    换词，终局指纹一致（可复现）；写回过。
    let mut lcg = Lcg::new(0xC962)?;
    let mut mc = MisCommit::new();
    let mut n = 0u32;
    while n < 8 {
        mc.commit((lcg.next_below(WORD_N as u64)) as u8)?;
        n += 1;
    }
    let mut n = 0u32;
    while n < SWAP_N {
        let pos = (lcg.next_below(mc.seq_len() as u64)) as usize;
        let new_id = (lcg.next_below(WORD_N as u64)) as u8;
        mc.replace_at(pos, new_id)?;
        n += 1;
    }
    let fp1 = mc.fingerprint();
    let mut lcg2 = Lcg::new(0xC962)?;
    let mut mc2 = MisCommit::new();
    let mut n = 0u32;
    while n < 8 {
        mc2.commit((lcg2.next_below(WORD_N as u64)) as u8)?;
        n += 1;
    }
    let mut n = 0u32;
    while n < SWAP_N {
        let pos = (lcg2.next_below(mc2.seq_len() as u64)) as usize;
        let new_id = (lcg2.next_below(WORD_N as u64)) as u8;
        mc2.replace_at(pos, new_id)?;
        n += 1;
    }
    if mc2.fingerprint() != fp1 {
        return Err(E_REPLAY_DIFF);
    }
    mc.audit()?;
    if mc.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    mc.writeback(true, 0x28)?;
    hits += 1;

    Ok(hits)
}

impl MisCommit {
    /// 只读槽视图（drill 断言用）。
    pub fn seq_get(&self, pos: usize) -> Result<Committed, u16> {
        if pos >= self.seq.len() {
            return Err(E_POS_BAD);
        }
        Ok(self.seq[pos])
    }
}
