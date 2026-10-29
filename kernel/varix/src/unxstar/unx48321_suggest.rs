//! unx48321_suggest — UNX-F48321 整句联想续接（上屏后按 bigram 给出续接候选，可关）（诊断段 0xC900）。
//!
//! 判据原文：
//!   J1 续接 ×100 语义合理样张账、开关即时生效；判据锚定 §AI-61 §2 专题一 bigram 延伸。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. bigram 模型：本地二元组续接表 (前词, 后词, 分) 常量账，8 个
//!      前词各 3 个后词共 24 条；分值闭域 [0,10]，低于门槛（MIN_SCORE）
//!      的续接不出候选——语义合理 = 分值达线 + 排序确定。
//!   2. 引擎-表双轨：引擎用倒排桶索引（前词 → 表项号桶）取候选，
//!      排序（分降序、同分词号升序）；样张账用线性扫表独立 oracle
//!      逐条比对——双算法互为哨兵，杜绝单套自证。
//!   3. 开关即时生效（J1 硬线）：关闭态续接候选立即清空（不吐旧账），
//!      重开后立即对当前尾词出候选（不等下一次上屏）；开关事件逐次
//!      留痕（轮次、状态）。
//!   4. 样张账（J1 主体）：×100 续接查询逐条落账（查询号、前词、
//!      首候选、首分值），引擎结果与 oracle 逐位相等；账本守恒
//!      （样张数 = 查询数）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC900;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_IDX_MISS: u16 = SEG_BASE + 1; // 倒排桶查无此前词
const E_ORACLE_OFF: u16 = SEG_BASE + 2; // 引擎与 oracle 对照失守
const E_TOGGLE_STATE: u16 = SEG_BASE + 3; // 开关状态非法
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 样张账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 5; // 引擎状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// bigram 表：8 前词 × 3 后词 = 24 条，(前词, 后词, 分)。
pub const BIGRAM: [(u16, u16, u8); 24] = [
    (0x0101, 0x0201, 9), (0x0101, 0x0202, 7), (0x0101, 0x0203, 5),
    (0x0102, 0x0204, 8), (0x0102, 0x0201, 8), (0x0102, 0x0205, 6), // 含同分对（排序歧义面）
    (0x0103, 0x0206, 10), (0x0103, 0x0202, 4), (0x0103, 0x0207, 4), // 门槛面：4 分对存疑
    (0x0104, 0x0203, 6), (0x0104, 0x0206, 6), (0x0104, 0x0208, 2),
    (0x0105, 0x0209, 7), (0x0105, 0x0204, 7), (0x0105, 0x0201, 3),
    (0x0106, 0x0205, 9), (0x0106, 0x0209, 5), (0x0106, 0x0202, 5),
    (0x0107, 0x0207, 8), (0x0107, 0x0208, 8), (0x0107, 0x0206, 8), // 三连同分
    (0x0108, 0x0201, 6), (0x0108, 0x0205, 6), (0x0108, 0x0209, 6),
];

/// 门槛与候选位宽。
pub const MIN_SCORE: u8 = 5;
pub const TOP_K: usize = 3;

/// oracle：线性扫表取前词候选（分降序、同分词号升序、门槛过滤）。
pub fn oracle_suggest(prev: u16) -> Vec<(u16, u8)> {
    let mut cands: Vec<(u16, u8)> = Vec::new();
    let mut i = 0usize;
    while i < BIGRAM.len() {
        if BIGRAM[i].0 == prev && BIGRAM[i].2 >= MIN_SCORE {
            cands.push((BIGRAM[i].1, BIGRAM[i].2));
        }
        i += 1;
    }
    // 选择排序：分降序、同分词号升序。
    let mut a = 0usize;
    while a < cands.len() {
        let mut b = a + 1;
        while b < cands.len() {
            let worse = cands[b].1 > cands[a].1
                || (cands[b].1 == cands[a].1 && cands[b].0 < cands[a].0);
            if worse {
                let t = cands[a];
                cands[a] = cands[b];
                cands[b] = t;
            }
            b += 1;
        }
        a += 1;
    }
    let n = if cands.len() > TOP_K { TOP_K } else { cands.len() };
    let mut out: Vec<(u16, u8)> = Vec::new();
    let mut k = 0usize;
    while k < n {
        out.push(cands[k]);
        k += 1;
    }
    out
}

/// 联想引擎。
pub struct SuggestEngine {
    pub enabled: bool,
    pub last_word: Option<u16>,
    pub history: Vec<u16>,
    idx: Vec<(u16, Vec<usize>)>, // 倒排桶：前词 → 表项号
    pub toggle_log: Vec<(u32, bool)>, // (事件号, 开后状态)
    pub sample_ledger: Vec<(u32, u16, u16, u8)>, // (查询号, 前词, 首候选, 首分值)
    pub queries: u32,
}

impl SuggestEngine {
    pub fn new() -> SuggestEngine {
        // 建倒排桶（引擎侧独立数据面）。
        let mut idx: Vec<(u16, Vec<usize>)> = Vec::new();
        let mut i = 0usize;
        while i < BIGRAM.len() {
            let prev = BIGRAM[i].0;
            let mut hit = false;
            let mut j = 0usize;
            while j < idx.len() {
                if idx[j].0 == prev {
                    idx[j].1.push(i);
                    hit = true;
                    break;
                }
                j += 1;
            }
            if !hit {
                let mut bucket: Vec<usize> = Vec::new();
                bucket.push(i);
                idx.push((prev, bucket));
            }
            i += 1;
        }
        SuggestEngine {
            enabled: true,
            last_word: None,
            history: Vec::new(),
            idx,
            toggle_log: Vec::new(),
            sample_ledger: Vec::new(),
            queries: 0,
        }
    }

    /// 上屏：词入历史、尾词更新。
    pub fn commit(&mut self, word: u16) {
        self.history.push(word);
        self.last_word = Some(word);
    }

    /// 续接候选：引擎走倒排桶 + 同律排序；关闭态立即空（不吐旧账）。
    pub fn suggest(&mut self) -> Result<Vec<(u16, u8)>, u16> {
        if !self.enabled {
            // 关闭态：立即空候选（即时生效硬线）。
            return Ok(Vec::new());
        }
        let prev = match self.last_word {
            Some(w) => w,
            None => return Ok(Vec::new()), // 无上屏词无候选
        };
        // 倒排桶取表项号。
        let mut bucket: Option<Vec<usize>> = None;
        let mut j = 0usize;
        while j < self.idx.len() {
            if self.idx[j].0 == prev {
                bucket = Some(self.idx[j].1.clone());
                break;
            }
            j += 1;
        }
        let b = match bucket {
            Some(x) => x,
            None => return Err(E_IDX_MISS),
        };
        let mut cands: Vec<(u16, u8)> = Vec::new();
        let mut k = 0usize;
        while k < b.len() {
            let e = BIGRAM[b[k]];
            if e.2 >= MIN_SCORE {
                cands.push((e.1, e.2));
            }
            k += 1;
        }
        // 同律排序：分降序、同分词号升序。
        let mut a = 0usize;
        while a < cands.len() {
            let mut m = a + 1;
            while m < cands.len() {
                let worse = cands[m].1 > cands[a].1
                    || (cands[m].1 == cands[a].1 && cands[m].0 < cands[a].0);
                if worse {
                    let t = cands[a];
                    cands[a] = cands[m];
                    cands[m] = t;
                }
                m += 1;
            }
            a += 1;
        }
        let n = if cands.len() > TOP_K { TOP_K } else { cands.len() };
        let mut out: Vec<(u16, u8)> = Vec::new();
        let mut k = 0usize;
        while k < n {
            out.push(cands[k]);
            k += 1;
        }
        self.queries += 1;
        if out.len() > 0 {
            self.sample_ledger.push((self.queries, prev, out[0].0, out[0].1));
        }
        Ok(out)
    }

    /// 开关（即时生效语义）：状态翻转留痕；重开后立即对当前尾词出候选。
    pub fn toggle(&mut self, on: bool) -> Result<(), u16> {
        if self.enabled == on {
            return Err(E_TOGGLE_STATE); // 同态翻转无效
        }
        self.enabled = on;
        self.toggle_log.push((self.toggle_log.len() as u32 + 1, on));
        Ok(())
    }

    /// 样张账审计：账本条数 ≤ 查询数、查询号单调、前词闭集、
    /// 首分值达线。
    pub fn audit_ledger(&self) -> Result<(), u16> {
        if self.sample_ledger.len() as u32 > self.queries {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 1usize;
        while i < self.sample_ledger.len() {
            if self.sample_ledger[i].0 <= self.sample_ledger[i - 1].0 {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.sample_ledger.len() {
            let (_, prev, _, score) = self.sample_ledger[i];
            if score < MIN_SCORE {
                return Err(E_LEDGER_BAD); // 首候选必须达线
            }
            if prev < 0x0101 || prev > 0x0108 {
                return Err(E_STATE_BAD); // 前词闭集
            }
            i += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：有样张 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.sample_ledger.len() == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（续接 ×100 语义合理样张账、开关即时生效），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×100：8 前词轮转 100 次续接查询，引擎（倒排桶）与
    //    oracle（线性扫表）逐条逐位相等，样张账逐条落账。
    let mut eng = SuggestEngine::new();
    let mut i = 0u32;
    while i < 100 {
        let prev = (0x0101 + (i % 8)) as u16;
        eng.commit(prev);
        let got = eng.suggest()?;
        let want = oracle_suggest(prev);
        if got.len() != want.len() {
            return Err(E_ORACLE_OFF);
        }
        let mut k = 0usize;
        while k < want.len() {
            if got[k].0 != want[k].0 || got[k].1 != want[k].1 {
                return Err(E_ORACLE_OFF);
            }
            k += 1;
        }
        i += 1;
    }
    if eng.queries != 100 {
        return Err(E_LEDGER_BAD);
    }
    if eng.sample_ledger.len() != 100 {
        return Err(E_LEDGER_BAD);
    }
    eng.audit_ledger()?;
    hits += 1;

    // R2 排序语义终判：分降序、同分词号升序、门槛过滤三律抽核——
    //    前词 0x0103（10/4/4）：首候选 0x0206 分 10，4 分对被门槛滤掉
    //    （候选只剩 1）；前词 0x0102（8/8/6）：同分对词号升序。
    eng.commit(0x0103);
    let c3 = eng.suggest()?;
    if c3.len() != 1 || c3[0].0 != 0x0206 || c3[0].1 != 10 {
        return Err(E_ORACLE_OFF);
    }
    eng.commit(0x0102);
    let c2 = eng.suggest()?;
    if c2.len() != 3 || c2[0].0 != 0x0201 || c2[0].1 != 8 || c2[1].0 != 0x0204 {
        return Err(E_ORACLE_OFF); // 同分 8：0x0201 < 0x0204 词号升序
    }
    eng.commit(0x0107);
    let c7 = eng.suggest()?;
    if c7.len() != 3 || c7[0].0 != 0x0206 || c7[2].0 != 0x0208 {
        return Err(E_ORACLE_OFF); // 三连同分：0x0206 < 0x0207 < 0x0208
    }
    hits += 1;

    // R3 开关即时生效（J1 硬线）：关 → 立即空候选；重开 → 立即对当前
    //    尾词出候选（不等下一次上屏）；同态翻转拒收。
    let mut eng2 = SuggestEngine::new();
    eng2.commit(0x0101);
    let before = eng2.suggest()?;
    if before.len() == 0 {
        return Err(E_ORACLE_OFF);
    }
    eng2.toggle(false)?;
    let off = eng2.suggest()?;
    if off.len() != 0 {
        return Err(E_TOGGLE_STATE); // 关闭态必须立即空
    }
    eng2.toggle(true)?;
    let on = eng2.suggest()?;
    if on.len() != before.len() || on[0].0 != before[0].0 {
        return Err(E_TOGGLE_STATE); // 重开立即恢复
    }
    if eng2.toggle(true).is_ok() {
        return Err(E_TOGGLE_STATE); // 同态翻转必拒
    }
    if eng2.toggle_log.len() != 2 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 关闭态不吐旧账面：关闭后 commit 不产生候选、样张账零增长；
    //    重开后首查即恢复落账。
    let led_before = eng2.sample_ledger.len();
    eng2.toggle(false)?;
    let mut i = 0u32;
    while i < 5 {
        eng2.commit((0x0101 + i) as u16);
        let off2 = eng2.suggest()?;
        if off2.len() != 0 {
            return Err(E_TOGGLE_STATE);
        }
        i += 1;
    }
    if eng2.sample_ledger.len() != led_before {
        return Err(E_LEDGER_BAD); // 关闭态零落账
    }
    eng2.toggle(true)?;
    let _ = eng2.suggest()?;
    if eng2.sample_ledger.len() != led_before + 1 {
        return Err(E_LEDGER_BAD); // 重开首查即落账
    }
    hits += 1;

    // R5 无上屏词面：空历史 suggest → 空候选（无尾词无续接）。
    let mut eng3 = SuggestEngine::new();
    let e = eng3.suggest()?;
    if e.len() != 0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 门槛律全表核：24 条表项逐条核 —— 分 < MIN_SCORE 的项绝不
    //    出现在任何前词的 oracle 候选中（语义合理 = 达线）。
    let mut p = 0x0101u16;
    while p <= 0x0108 {
        let cands = oracle_suggest(p);
        let mut k = 0usize;
        while k < cands.len() {
            if cands[k].1 < MIN_SCORE {
                return Err(E_ORACLE_OFF);
            }
            k += 1;
        }
        p += 1;
    }
    // 门槛边缘项核：0x0103→0x0202（4 分）与 0x0104→0x0208（2 分）必被滤。
    let c = oracle_suggest(0x0104);
    if c.len() != 2 {
        return Err(E_ORACLE_OFF); // 6/6 过、2 分滤
    }
    hits += 1;

    // R7 历史账与尾词闭合：×100 查询 + R3/R4 查询后历史连续、
    //    尾词恒最后上屏词。
    if eng.history.len() != 100 {
        return Err(E_LEDGER_BAD);
    }
    if eng.last_word != Some(0x0101 + 99 % 8) {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：零样张不写回、空摘要拒、有账+非空摘要放行。
    let mut eng4 = SuggestEngine::new();
    if eng4.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    eng4.commit(0x0106);
    let _ = eng4.suggest()?;
    if eng4.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    eng4.writeback(true, 0x64)?;
    hits += 1;

    Ok(hits)
}
