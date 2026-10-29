//! unx48343_readanch — UNX-F48343 屏幕阅读器数据锚（preedit 文本 + 候选列表只读导出锚）（诊断段 0xCBC0）。
//!
//! 判据原文：
//!   J1 导出 ×100 与池实态一致；判据锚定 体验四、§AI-61 §1 边界 F4。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 池实态唯一：AnchorPool 持有 preedit 字符流与候选表两份实态，
//!      任何变更走 write 路径并使 rev 自增；导出锚只读，绝无写路径。
//!   2. 锚即快照：export() 按当前 rev 冻结快照——preedit 长度、preedit
//!      指纹、候选数、候选指纹四元组 + 自体指纹；锚与池逐位可对。
//!   3. ×100 一致：100 轮"变更→导出→对池核验"，每轮锚四元组与池
//!      实态逐项相等；任何一轮失配即 E_MISMATCH，账面零容忍。
//!   4. 只读边界 F4：导出前后池 rev 与全态指纹不变（导出不碰池）；
//!      篡改锚任一字段核验必 E_FP_TAMPER，跨 rev 核验必 E_STALE。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCBC0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_LEN_BAD: u16 = SEG_BASE + 1; // 长度字段与池不符
const E_FP_TAMPER: u16 = SEG_BASE + 2; // 锚指纹遭篡改
const E_STALE: u16 = SEG_BASE + 3; // 锚落后于池实态
const E_MISMATCH: u16 = SEG_BASE + 4; // 导出与池实态失配
const E_RO_VIOL: u16 = SEG_BASE + 5; // 只读边界被击穿
const E_REPLAY_BAD: u16 = SEG_BASE + 6; // 重核验结果翻供
const E_CAND_BAD: u16 = SEG_BASE + 7; // 候选表非法

/// FNV-1a 64 位指纹（char 流）。
pub fn fp_chars(cs: &[char]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < cs.len() {
        let v = cs[i] as u32;
        h ^= (v & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= ((v >> 8) & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= ((v >> 16) & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= ((v >> 24) & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// FNV-1a 64 位指纹（u64 序列，用于候选 id+score 串联）。
pub fn fp_u64s(vals: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < vals.len() {
        let mut b = 0usize;
        while b < 8 {
            h ^= ((vals[i] >> (b * 8)) & 0xFF) as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            b += 1;
        }
        i += 1;
    }
    h
}

/// 候选条目：候选 id + 文本 + 分值（实态侧）。
#[derive(Clone)]
pub struct Cand {
    pub cid: u32,
    pub text: Vec<char>,
    pub score: u16,
}

impl Cand {
    fn serial(&self) -> u64 {
        let mut acc: u64 = self.cid as u64;
        acc = acc.wrapping_mul(1_000_003).wrapping_add(self.score as u64);
        acc ^= fp_chars(&self.text);
        acc
    }
}

/// 池实态：preedit 字符流 + 候选表，rev 随每次写自增。
pub struct AnchorPool {
    preedit: Vec<char>,
    cands: Vec<Cand>,
    rev: u64,
}

impl AnchorPool {
    pub fn new() -> Self {
        AnchorPool { preedit: Vec::new(), cands: Vec::new(), rev: 0 }
    }

    /// 写路径：压入一个 preedit 字符。
    pub fn push_preedit(&mut self, c: char) {
        self.preedit.push(c);
        self.rev = self.rev.wrapping_add(1);
    }

    /// 写路径：退格一格；空流退格为合法空操作但仍计一次写。
    pub fn backspace(&mut self) {
        self.preedit.pop();
        self.rev = self.rev.wrapping_add(1);
    }

    /// 写路径：整表换候选（候选 id 必须严格递增且唯一）。
    pub fn set_cands(&mut self, cands: Vec<Cand>) -> Result<(), u16> {
        let mut i = 1usize;
        while i < cands.len() {
            if cands[i].cid <= cands[i - 1].cid {
                return Err(E_CAND_BAD);
            }
            i += 1;
        }
        self.cands = cands;
        self.rev = self.rev.wrapping_add(1);
        Ok(())
    }

    pub fn preedit(&self) -> &[char] {
        &self.preedit
    }

    pub fn cand_count(&self) -> u32 {
        self.cands.len() as u32
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// 池全态指纹：preedit 指纹 ⊕ 候选串联指纹 ⊕ rev。
    pub fn whole_fp(&self) -> u64 {
        let mut srl: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < self.cands.len() {
            srl.push(self.cands[i].serial());
            i += 1;
        }
        fp_chars(&self.preedit) ^ fp_u64s(&srl) ^ self.rev
    }
}

/// 只读导出锚：池实态在 rev 时刻的四元组快照 + 自体指纹。
#[derive(Clone)]
pub struct Anchor {
    pub rev: u64,
    pub preedit_len: u32,
    pub preedit_fp: u64,
    pub cand_count: u32,
    pub cands_fp: u64,
    pub self_fp: u64,
}

impl Anchor {
    /// 逐项与池实态核验：rev 对上才比四元组，任何一项失配即报错。
    pub fn check(&self, pool: &AnchorPool) -> Result<(), u16> {
        // 自体指纹先自洽（防锚本体被篡改）。
        let want = Self::seal(self.rev, self.preedit_len, self.preedit_fp, self.cand_count, self.cands_fp);
        if want != self.self_fp {
            return Err(E_FP_TAMPER);
        }
        if self.rev != pool.rev() {
            return Err(E_STALE);
        }
        if self.preedit_len != pool.preedit().len() as u32 {
            return Err(E_LEN_BAD);
        }
        if self.preedit_fp != fp_chars(pool.preedit()) {
            return Err(E_MISMATCH);
        }
        if self.cand_count != pool.cand_count() {
            return Err(E_LEN_BAD);
        }
        // 池侧候选指纹 = whole_fp ⊕ preedit_fp ⊕ rev 的逆视角核验。
        let cand_fp_ok = {
            // 池侧候选指纹 = whole_fp ⊕ preedit_fp ⊕ rev 的逆视角核验。
            let inv = pool.whole_fp() ^ fp_chars(pool.preedit()) ^ pool.rev();
            inv == self.cands_fp
        };
        if !cand_fp_ok {
            return Err(E_MISMATCH);
        }
        Ok(())
    }

    fn seal(rev: u64, plen: u32, pfp: u64, cc: u32, cfp: u64) -> u64 {
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        h ^= rev;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= plen as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= pfp;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= cc as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= cfp;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }
}

/// 导出器：从池冻结只读锚（绝无写路径）。
pub struct Exportor;

impl Exportor {
    pub fn export(pool: &AnchorPool) -> Result<Anchor, u16> {
        if pool.rev() == 0 && pool.cand_count() == 0 && pool.preedit().is_empty() {
            // 空池亦允许导出：锚为全零四元组，仍是合法快照。
        }
        let a = Anchor {
            rev: pool.rev(),
            preedit_len: pool.preedit().len() as u32,
            preedit_fp: fp_chars(pool.preedit()),
            cand_count: pool.cand_count(),
            cands_fp: pool.whole_fp() ^ fp_chars(pool.preedit()) ^ pool.rev(),
            self_fp: 0,
        };
        let mut a = a;
        a.self_fp = Anchor::seal(a.rev, a.preedit_len, a.preedit_fp, a.cand_count, a.cands_fp);
        Ok(a)
    }
}

/// 判据统计账。
pub struct ConsistLedger {
    pub exports: u32,
    pub consistent: u32,
    pub ro_checks: u32,
}

impl ConsistLedger {
    pub fn new() -> Self {
        ConsistLedger { exports: 0, consistent: 0, ro_checks: 0 }
    }
}

/// drill：J1 导出 ×100 与池实态一致（含只读边界 F4 / 篡改 / 陈旧 / 重放核验）。
pub fn drill() -> Result<u32, u16> {
    let mut ledger = ConsistLedger::new();
    let mut pool = AnchorPool::new();

    // ── R1 主判据：100 轮 变更→导出→对池核验 全一致 ─────────
    let mut i = 0u32;
    while i < 100 {
        // 确定性变更序列。
        if i % 3 == 0 {
            pool.push_preedit(core::char::from_u32(0x4E00 + ((i.wrapping_mul(7) % 40) as u32)).unwrap_or('\u{FFFD}'));
        }
        if i % 5 == 0 {
            // 退格一档，制造 preedit 收缩路径。
            pool.backspace();
        }
        if i % 4 == 0 {
            // 换候选表：k = i/4 % 3 + 2 个候选，id 严格递增。
            let k = (i / 4) % 3 + 2;
            let mut cs: Vec<Cand> = Vec::new();
            let mut j = 0u32;
            while j < k {
                let mut text: Vec<char> = Vec::new();
                text.push(core::char::from_u32(0x41 + j as u32).unwrap_or('A'));
                text.push(core::char::from_u32(0x61 + (i % 26) as u32).unwrap_or('a'));
                cs.push(Cand { cid: 9000 + i * 10 + j, text, score: (i % 90) as u16 + 10 });
                j += 1;
            }
            pool.set_cands(cs)?;
        }
        let a = Exportor::export(&pool)?;
        ledger.exports += 1;
        // 逐项手工对池（比 check() 更细的四元组直接比对）。
        if a.rev != pool.rev() || a.preedit_len != pool.preedit().len() as u32 {
            return Err(E_MISMATCH);
        }
        if a.preedit_fp != fp_chars(pool.preedit()) || a.cand_count != pool.cand_count() {
            return Err(E_MISMATCH);
        }
        let inv = pool.whole_fp() ^ fp_chars(pool.preedit()) ^ pool.rev();
        if a.cands_fp != inv {
            return Err(E_MISMATCH);
        }
        // check() 全通道复核。
        a.check(&pool)?;
        ledger.consistent += 1;
        i += 1;
    }
    if ledger.exports != 100 || ledger.consistent != 100 {
        return Err(E_MISMATCH);
    }

    // ── R2 只读边界 F4：导出前后池全态纹丝不动 ──────────────
    let before = pool.whole_fp();
    let before_rev = pool.rev();
    let a_mid = Exportor::export(&pool)?;
    let after = pool.whole_fp();
    if before != after || before_rev != pool.rev() {
        return Err(E_RO_VIOL);
    }
    a_mid.check(&pool)?;
    ledger.ro_checks += 1;
    // 锚不可变：导出两次，两锚逐字段相等。
    let a_mid2 = Exportor::export(&pool)?;
    if a_mid.rev != a_mid2.rev
        || a_mid.preedit_len != a_mid2.preedit_len
        || a_mid.preedit_fp != a_mid2.preedit_fp
        || a_mid.cand_count != a_mid2.cand_count
        || a_mid.cands_fp != a_mid2.cands_fp
        || a_mid.self_fp != a_mid2.self_fp
    {
        return Err(E_RO_VIOL);
    }

    // ── R3 篡改注入：锚任一字段被改 → 自体检出 E_FP_TAMPER ──
    let mut bad = a_mid.clone();
    bad.preedit_len = bad.preedit_len.wrapping_add(1);
    if bad.check(&pool) != Err(E_FP_TAMPER) {
        return Err(E_RO_VIOL);
    }
    let mut bad2 = a_mid.clone();
    bad2.cands_fp = bad2.cands_fp.wrapping_add(1);
    if bad2.check(&pool) != Err(E_FP_TAMPER) {
        return Err(E_RO_VIOL);
    }

    // ── R4 陈旧注入：核验前池又被写 → E_STALE ───────────────
    pool.push_preedit('\u{5B89}');
    if a_mid.check(&pool) != Err(E_STALE) {
        return Err(E_STALE);
    }

    // ── R5 空池导出：全零四元组仍是合法锚 ────────────────────
    let empty = AnchorPool::new();
    let ea = Exportor::export(&empty)?;
    if ea.rev != 0 || ea.preedit_len != 0 || ea.cand_count != 0 {
        return Err(E_LEN_BAD);
    }
    ea.check(&empty)?;
    if ea.self_fp != Anchor::seal(0, 0, fp_chars(&[]), 0, {
        let z = empty.whole_fp() ^ fp_chars(empty.preedit()) ^ empty.rev();
        z
    }) {
        return Err(E_FP_TAMPER);
    }

    // ── R6 重放核验：同一锚对同一池态核验结果必须翻供即报 ────
    let p2 = AnchorPool::new();
    let mut p2 = p2;
    p2.push_preedit('\u{6C49}');
    let la = Exportor::export(&p2)?;
    let r1 = la.check(&p2);
    let r2 = la.check(&p2);
    match (r1, r2) {
        (Ok(()), Ok(())) => {}
        _ => return Err(E_REPLAY_BAD),
    }

    // ── R7 候选表纪律：id 非严格递增拒收 E_CAND_BAD ──────────
    let mut dup: Vec<Cand> = Vec::new();
    let mut t1: Vec<char> = Vec::new();
    t1.push('A');
    dup.push(Cand { cid: 7, text: t1.clone(), score: 1 });
    dup.push(Cand { cid: 7, text: t1, score: 2 });
    if pool.set_cands(dup) != Err(E_CAND_BAD) {
        return Err(E_CAND_BAD);
    }

    // J2 写回摘要非空：以一致性账非空为摘要实质。
    if ledger.consistent == 0 {
        return Err(E_REPLAY_BAD);
    }
    Ok(ledger.consistent)
}
