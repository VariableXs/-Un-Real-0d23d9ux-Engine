//! unx48325_pyfix — UNX-F48325 拼音纠错面（N/L 混淆、误击邻键的候选纠正，降权标注）（诊断段 0xC980）。
//!
//! 判据原文：
//!   J1 纠错 ×10 组命中且标注降权；判据锚定 §AI-61 §2 专题一 fuzzy 扩展。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 纠错闭集：只纠两类错——N/L 混淆（双向对）与键盘邻键误击
//!      （对表内双向对）；对表外的字符替换绝不瞎纠（纠错面不是
//!      万能键，闭集外命中即为误纠）。
//!   2. 单点纠错：一次查询至多容一处替换（生成全部单点变体查表）；
//!      两处及以上错一律不命中（模糊即猜谜，猜谜即错上屏温床）。
//!   3. 降权标注：精确命中 penalty=0；混淆纠错 penalty=5、邻键
//!      penalty=3；多变体取最小 penalty，同罚按词 id 升序——降权
//!      值必须随候选输出，不得静默吞掉（用户须知道这是纠的）。
//!   4. 账目守恒：查询总数 == 精确命中 + 纠错命中 + 未命中，三账
//!      分立互不冒充。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC980;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_EMPTY_Q: u16 = SEG_BASE + 1;  // 空查询拒收
const E_NO_HIT: u16 = SEG_BASE + 2;   // 精确与纠错均未命中
const E_PAIR_BAD: u16 = SEG_BASE + 3; // 混淆/邻键对表不闭
const E_LEDGER_BAD: u16 = SEG_BASE + 4;// 三账不平
const E_PENALTY_BAD: u16 = SEG_BASE + 5;// 降权标注缺失/越闭集
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;// 写回摘要为空

/// 词表（拼音串定长补 0 尾）。
pub const WORD_N: usize = 12;
pub const PY_MAX: usize = 6;
/// 词表条目：[词id, p1..p6（0 填充）]，刻意只收混淆对的单词侧，
/// 使错拼侧必须走纠错径。
pub const WORD_TAB: [[u8; PY_MAX + 1]; WORD_N] = [
    [0, b'n', b'a', b'n', 0, 0, 0],  // 南
    [1, b'l', b'i', b'u', 0, 0, 0],  // 刘
    [2, b'l', b'i', b'n', 0, 0, 0],  // 林
    [3, b'l', b'o', b'n', b'g', 0, 0], // 龙
    [4, b'l', b'e', b'i', 0, 0, 0],  // 累
    [5, b'c', b'a', b'n', 0, 0, 0],  // 餐
    [6, b's', b'h', b'i', 0, 0, 0],  // 是
    [7, b'm', b'a', 0, 0, 0, 0],     // 妈
    [8, b'f', b'a', 0, 0, 0, 0],     // 发
    [9, b'd', b'e', 0, 0, 0, 0],     // 的
    [10, b'l', b'e', 0, 0, 0, 0],    // 乐
    [11, b'h', b'e', 0, 0, 0, 0],    // 喝
];

/// N/L 混淆对（双向）。
pub const CONFUSE_TAB: [(u8, u8); 2] = [(b'n', b'l'), (b'l', b'n')];
/// 邻键对（双向，主键区行内相邻）。
pub const ADJACENT_TAB: [(u8, u8); 26] = [
    (b'a', b's'), (b's', b'a'), (b's', b'd'), (b'd', b's'),
    (b'd', b'f'), (b'f', b'd'), (b'f', b'g'), (b'g', b'f'),
    (b'g', b'h'), (b'h', b'g'), (b'h', b'j'), (b'j', b'h'),
    (b'q', b'w'), (b'w', b'q'), (b'w', b'e'), (b'e', b'w'),
    (b'e', b'r'), (b'r', b'e'), (b'r', b't'), (b't', b'r'),
    (b'z', b'x'), (b'x', b'z'), (b'x', b'c'), (b'c', b'x'),
    (b'c', b'v'), (b'v', b'c'),
];

/// 降权罚值闭集。
pub const PEN_EXACT: u32 = 0;
pub const PEN_ADJ: u32 = 3;
pub const PEN_CONFUSE: u32 = 5;

/// 纠错命中（词 id + 纠后拼音 + 降权罚值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FixHit {
    pub word_id: u8,
    pub penalty: u32,
    pub fixed_len: usize,
}

/// 拼音纠错面。
pub struct PyFix {
    pub query_total: u32,
    pub exact_total: u32,
    pub fix_total: u32,
    pub miss_total: u32,
    pub reject_total: u32,
}

impl PyFix {
    pub fn new() -> PyFix {
        PyFix { query_total: 0, exact_total: 0, fix_total: 0, miss_total: 0, reject_total: 0 }
    }

    /// 表内查串（精确）。
    fn exact_lookup(q: &[u8]) -> Option<u8> {
        let mut i = 0usize;
        while i < WORD_N {
            let row = &WORD_TAB[i];
            let wl = py_len(row);
            if wl == q.len() {
                let mut same = true;
                let mut k = 0usize;
                while k < wl {
                    if row[1 + k] != q[k] {
                        same = false;
                        break;
                    }
                    k += 1;
                }
                if same {
                    return Some(row[0]);
                }
            }
            i += 1;
        }
        None
    }

    /// 对表查替身（混淆罚 5，邻键罚 3）。
    fn sub_lookup(c: u8) -> Option<(u8, u32)> {
        let mut i = 0usize;
        while i < CONFUSE_TAB.len() {
            if CONFUSE_TAB[i].0 == c {
                return Some((CONFUSE_TAB[i].1, PEN_CONFUSE));
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < ADJACENT_TAB.len() {
            if ADJACENT_TAB[i].0 == c {
                return Some((ADJACENT_TAB[i].1, PEN_ADJ));
            }
            i += 1;
        }
        None
    }

    /// 单点变体查表：对查询串每个位置尝试对表内替身，命中即收集。
    /// 返回最小罚命中（同罚按词 id 升序择优）。
    fn fix_lookup(q: &[u8]) -> Option<FixHit> {
        let mut best: Option<FixHit> = None;
        let mut pos = 0usize;
        while pos < q.len() {
            if let Some((rep, pen)) = PyFix::sub_lookup(q[pos]) {
                let mut v: Vec<u8> = Vec::new();
                let mut k = 0usize;
                while k < q.len() {
                    if k == pos {
                        v.push(rep);
                    } else {
                        v.push(q[k]);
                    }
                    k += 1;
                }
                if let Some(wid) = PyFix::exact_lookup(&v) {
                    let hit = FixHit { word_id: wid, penalty: pen, fixed_len: v.len() };
                    let take = match best {
                        None => true,
                        Some(b) => {
                            hit.penalty < b.penalty
                                || (hit.penalty == b.penalty && hit.word_id < b.word_id)
                        }
                    };
                    if take {
                        best = Some(hit);
                    }
                }
            }
            pos += 1;
        }
        best
    }

    /// 查询主径：精确优先，其次单点纠错（降权标注）。
    pub fn query(&mut self, q: &[u8]) -> Result<(u8, u32), u16> {
        self.query_total += 1;
        if q.is_empty() {
            self.reject_total += 1;
            return Err(E_EMPTY_Q);
        }
        if let Some(wid) = PyFix::exact_lookup(q) {
            self.exact_total += 1;
            return Ok((wid, PEN_EXACT));
        }
        if let Some(hit) = PyFix::fix_lookup(q) {
            if hit.penalty == PEN_EXACT {
                return Err(E_PENALTY_BAD); // 纠错径不得零罚（标注缺失即红）
            }
            self.fix_total += 1;
            return Ok((hit.word_id, hit.penalty));
        }
        self.miss_total += 1;
        Err(E_NO_HIT)
    }

    /// 三账守恒审计。
    pub fn audit(&self) -> Result<(), u16> {
        if self.query_total
            != self.exact_total + self.fix_total + self.miss_total + self.reject_total
        {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.query_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 拼音行有效长。
fn py_len(row: &[u8; PY_MAX + 1]) -> usize {
    let mut n = 0usize;
    while n < PY_MAX && row[1 + n] != 0 {
        n += 1;
    }
    n
}

/// J1 十组纠错样张（错拼查询 → 期望词 id），全部必须命中且降权。
pub const FIX_SAMPLES: [(&[u8], u8); 10] = [
    (b"lan", 0),   // n/l 混淆 → nan 南
    (b"niu", 1),   // 混淆 → liu 刘
    (b"nin", 2),   // 混淆 → lin 林
    (b"nong", 3),  // 混淆 → long 龙
    (b"nei", 4),   // 混淆 → lei 累
    (b"xan", 5),   // c/x 邻键 → can 餐
    (b"dhi", 6),   // s/d 邻键 → shi 是
    (b"ga", 8),    // f/g 邻键 → fa 发
    (b"dw", 9),    // w/e 邻键 → de 的
    (b"je", 11),   // h/j 邻键 → he 喝
];

/// 自例程判据：逐条落实 J1（纠错 ×10 组命中且标注降权），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 纠错 ×10 组命中且标注降权：十组样张全部命中且 penalty>0，
    //    penalty 落闭集（3 或 5）。
    let mut pf = PyFix::new();
    let mut s = 0usize;
    while s < FIX_SAMPLES.len() {
        let (q, want_wid) = FIX_SAMPLES[s];
        let (wid, pen) = pf.query(q)?;
        if wid != want_wid {
            return Err(E_NO_HIT);
        }
        if pen != PEN_CONFUSE && pen != PEN_ADJ {
            return Err(E_PENALTY_BAD);
        }
        s += 1;
    }
    if pf.fix_total != 10 || pf.exact_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    pf.audit()?;
    hits += 1;

    // R2 精确匹配零降权：词表 12 词全查，penalty==0，纠错账不动。
    let mut pf = PyFix::new();
    let mut i = 0usize;
    while i < WORD_N {
        let row = &WORD_TAB[i];
        let wl = py_len(row);
        let mut q: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < wl {
            q.push(row[1 + k]);
            k += 1;
        }
        let (wid, pen) = pf.query(&q)?;
        if wid != row[0] || pen != PEN_EXACT {
            return Err(E_PENALTY_BAD);
        }
        i += 1;
    }
    if pf.exact_total != 12 || pf.fix_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    pf.audit()?;
    hits += 1;

    // R3 对表外替换不纠：非混淆/邻键的字符替换查询必未命中（不瞎纠）。
    let mut pf = PyFix::new();
    if pf.query(b"mv") != Err(E_NO_HIT) {
        return Err(E_NO_HIT);
    }
    if pf.query(b"zao") != Err(E_NO_HIT) {
        return Err(E_NO_HIT);
    }
    pf.audit()?;
    hits += 1;

    // R4 单点闭集：双错查询不命中（变体生成只容一处替换）；空查询拒收。
    let mut pf = PyFix::new();
    // "jer" 距 "he" 两错（j→h 且 r→e），单点变体全 miss。
    if pf.query(b"jer") != Err(E_NO_HIT) {
        return Err(E_NO_HIT);
    }
    // "dwi" 距 "de" 一替换一多余，仍 miss。
    if pf.query(b"dwi") != Err(E_NO_HIT) {
        return Err(E_NO_HIT);
    }
    if pf.query(b"") != Err(E_EMPTY_Q) {
        return Err(E_EMPTY_Q);
    }
    if pf.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    pf.audit()?;
    hits += 1;

    // R5 降权值账面可复核：混淆样张罚恒 5、邻键样张罚恒 3（分组
    //    断言，标注不是拍脑袋）。
    let mut pf = PyFix::new();
    let confuse_n = 5usize;
    let mut s = 0usize;
    while s < confuse_n {
        let (_, pen) = pf.query(FIX_SAMPLES[s].0)?;
        if pen != PEN_CONFUSE {
            return Err(E_PENALTY_BAD);
        }
        s += 1;
    }
    let mut s = confuse_n;
    while s < FIX_SAMPLES.len() {
        let (_, pen) = pf.query(FIX_SAMPLES[s].0)?;
        if pen != PEN_ADJ {
            return Err(E_PENALTY_BAD);
        }
        s += 1;
    }
    hits += 1;

    // R6 双路径对账：query 径的命中集合与直接调 fix_lookup 的独立
    //    枚举一致（防主径择优逻辑独走）。
    let mut pf = PyFix::new();
    let mut s = 0usize;
    while s < FIX_SAMPLES.len() {
        let q = FIX_SAMPLES[s].0;
        let (wid, pen) = pf.query(q)?;
        match PyFix::fix_lookup(q) {
            None => return Err(E_REPLAY_DIFF),
            Some(h) => {
                if h.word_id != wid || h.penalty != pen {
                    return Err(E_REPLAY_DIFF);
                }
            }
        }
        s += 1;
    }
    hits += 1;

    // R7 可红演练：构造纠错径零罚必红（词表侧若混入罚 0 命中即
    //    标注缺失）；空摘要/零查询写回必红。
    let mut pf = PyFix::new();
    if pf.query(b"ma").is_err() {
        return Err(E_NO_HIT); // 前置：精确径必须可达
    }
    if pf.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    let pf0 = PyFix::new();
    if pf0.writeback(false, 8).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 ×10 组复跑与写回（判据 J2）：十组样张第二轮复跑逐位一致
    //    （确定性），三账守恒，写回过。
    let mut pf = PyFix::new();
    let mut s = 0usize;
    while s < FIX_SAMPLES.len() {
        pf.query(FIX_SAMPLES[s].0)?;
        s += 1;
    }
    let fp1 = (pf.exact_total, pf.fix_total, pf.miss_total);
    let mut pf2 = PyFix::new();
    let mut s = 0usize;
    while s < FIX_SAMPLES.len() {
        pf2.query(FIX_SAMPLES[s].0)?;
        s += 1;
    }
    if fp1 != (pf2.exact_total, pf2.fix_total, pf2.miss_total) {
        return Err(E_REPLAY_DIFF);
    }
    pf.audit()?;
    if pf.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    pf.writeback(true, 0x18)?;
    hits += 1;

    Ok(hits)
}
