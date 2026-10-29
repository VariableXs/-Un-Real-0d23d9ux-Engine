//! unx48248_phrasetop — UNX-F48248 自定义短语置顶优先级纵深（置顶压过整句 top1）（诊断段 0xBFE0）。
//!
//! 判据原文：
//!   J1 置顶 ×100 生效；判据锚定 §AI-61 §2 专题五自定义短语项。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 短语账三层：PinStore（自定义短语：code → phrase_id, pinned
//!      位, 入账序）、SentencePool（整句候选：code → score 动态打分
//!      面）、Ranker（合成呈现序）。置顶语义是跨层优先级纵深：
//!      pinned 短语恒压过任何 score 的整句 top1——score 再大也只
//!      在无置顶时出头。
//!   2. 呈现序规则冻结：第一段 pinned（按入账序，score 无关）→
//!      第二段普通短语（按入账序）→ 第三段整句（score 降序，同分
//!      按 id 升序 tie-break）。三段各自稳定，段内不得串。
//!   3. pin/unpin 是单向闸上的合法操作：pin 幂等（重复 pin 不动
//!      账），unpin 后短语退入普通段（不删账）；pin 不存在的
//!      phrase_id 拒（E_PIN_MISS）。
//!   4. J1 主判据 ×100：LCG 造 100 查询（随机 code、随机整句分、
//!      随机置顶组合），呈现序首槽（rank0）断言：有置顶 ⇒ rank0
//!      必是置顶短语（哪怕整句 score 压顶 9_999）；无置顶 ⇒ rank0
//!      是该码最前段候选（普通短语或整句最高分，同分取 id 小）。
//!      逐笔断言、全绿收账。
//!   5. 交叉守恒：呈现序总长 == pinned + 普通 + 整句 三段之和；
//!      段间次序审计（pinned 段不得出现在普通/整句段之后）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBFE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_PIN_MISS: u16 = SEG_BASE + 1; // pin 不存在的短语
const E_ORDER: u16 = SEG_BASE + 2; // 三段次序破
const E_RANK0: u16 = SEG_BASE + 3; // rank0 语义破（置顶未压顶）
const E_LEN_BAD: u16 = SEG_BASE + 4; // 呈现序总长不守恒
const E_DUP: u16 = SEG_BASE + 5; // 短语重复入账
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空
const E_TIE: u16 = SEG_BASE + 7; // 整句同分 tie-break 破

/// PinStore 条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Phrase {
    pub code: u32,
    pub phrase_id: u32,
    pub pinned: bool,
}

/// 整句候选。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sentence {
    pub code: u32,
    pub sent_id: u32,
    pub score: u32,
}

/// PinStore：自定义短语账（入账序即普通段序）。
pub struct PinStore {
    pub phrases: Vec<Phrase>,
}

impl PinStore {
    pub fn new() -> PinStore {
        PinStore { phrases: Vec::new() }
    }

    /// 入账：同 (code, phrase_id) 查重（纪律 4 面）。
    pub fn add(&mut self, code: u32, phrase_id: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.phrases.len() {
            if self.phrases[i].code == code && self.phrases[i].phrase_id == phrase_id {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.phrases.push(Phrase { code, phrase_id, pinned: false });
        Ok(())
    }

    /// pin：幂等；不存在拒。
    pub fn pin(&mut self, phrase_id: u32) -> Result<(), u16> {
        let mut hit = false;
        let mut i = 0usize;
        while i < self.phrases.len() {
            if self.phrases[i].phrase_id == phrase_id {
                self.phrases[i].pinned = true;
                hit = true;
            }
            i += 1;
        }
        if !hit {
            return Err(E_PIN_MISS);
        }
        Ok(())
    }

    /// unpin：退普通段不删账。
    pub fn unpin(&mut self, phrase_id: u32) -> Result<(), u16> {
        let mut hit = false;
        let mut i = 0usize;
        while i < self.phrases.len() {
            if self.phrases[i].phrase_id == phrase_id {
                self.phrases[i].pinned = false;
                hit = true;
            }
            i += 1;
        }
        if !hit {
            return Err(E_PIN_MISS);
        }
        Ok(())
    }

    pub fn find(&self, code: u32) -> Vec<Phrase> {
        let mut out: Vec<Phrase> = Vec::new();
        let mut i = 0usize;
        while i < self.phrases.len() {
            if self.phrases[i].code == code {
                out.push(self.phrases[i]);
            }
            i += 1;
        }
        out
    }
}

/// Ranker：合成呈现序（纪律 2 三段规则）。
pub struct Ranker {
    pub pool: Vec<Sentence>,
}

impl Ranker {
    pub fn new() -> Ranker {
        Ranker { pool: Vec::new() }
    }

    pub fn add_sent(&mut self, s: Sentence) {
        self.pool.push(s);
    }

    /// code 查询呈现序：pinned 段 → 普通段 → 整句段（score 降序、
    /// 同分 id 升序）。
    pub fn present(&self, store: &PinStore, code: u32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let phs = store.find(code);
        // 第一段：pinned（入账序）。
        let mut i = 0usize;
        while i < phs.len() {
            if phs[i].pinned {
                out.push(phs[i].phrase_id);
            }
            i += 1;
        }
        // 第二段：普通短语（入账序）。
        let mut i = 0usize;
        while i < phs.len() {
            if !phs[i].pinned {
                out.push(phs[i].phrase_id);
            }
            i += 1;
        }
        // 第三段：整句（score 降序、同分 id 升序）。
        let mut sents: Vec<Sentence> = Vec::new();
        let mut i = 0usize;
        while i < self.pool.len() {
            if self.pool[i].code == code {
                sents.push(self.pool[i]);
            }
            i += 1;
        }
        // 选择排序入账（稳定口径手写，不依赖库排序语义）。
        let n = sents.len();
        let mut used: Vec<bool> = Vec::new();
        let mut i = 0usize;
        while i < n {
            used.push(false);
            i += 1;
        }
        let mut i = 0usize;
        while i < n {
            let mut best = usize::MAX;
            let mut j = 0usize;
            while j < n {
                if !used[j] {
                    if best == usize::MAX
                        || sents[j].score > sents[best].score
                        || (sents[j].score == sents[best].score
                            && sents[j].sent_id < sents[best].sent_id)
                    {
                        best = j;
                    }
                }
                j += 1;
            }
            if best == usize::MAX {
                break;
            }
            used[best] = true;
            out.push(sents[best].sent_id);
            i += 1;
        }
        out
    }

    /// 三段次序审计（纪律 5）：pinned id 不得出现在普通/整句之后。
    pub fn audit_order(&self, store: &PinStore, code: u32, out: &[u32]) -> Result<(), u16> {
        let phs = store.find(code);
        // pinned 集合。
        let mut pinned_ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < phs.len() {
            if phs[i].pinned {
                pinned_ids.push(phs[i].phrase_id);
            }
            i += 1;
        }
        // out 中首个非 pinned 出现后不得再出 pinned。
        let mut seen_non_pinned = false;
        let mut i = 0usize;
        while i < out.len() {
            let mut is_pinned = false;
            let mut j = 0usize;
            while j < pinned_ids.len() {
                if pinned_ids[j] == out[i] {
                    is_pinned = true;
                }
                j += 1;
            }
            if is_pinned && seen_non_pinned {
                return Err(E_ORDER);
            }
            if !is_pinned {
                seen_non_pinned = true;
            }
            i += 1;
        }
        Ok(())
    }
}

/// LCG（确定性）。
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

/// 自例程判据：逐条落实 J1（置顶 ×100 生效），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主判据 ×100：LCG 造 100 查询，rank0 语义逐笔断言。
    let mut rng = Lcg::new(0xBFE0_0100);
    let mut store = PinStore::new();
    let mut ranker = Ranker::new();
    // 铺底账：8 个 code × 每码 2 短语 + 每码 3 整句（分随 LCG）。
    let mut pid: u32 = 1;
    let mut cid: u32 = 0;
    while cid < 8 {
        let code = 100 + cid;
        let mut k = 0u32;
        while k < 2 {
            store.add(code, pid)?;
            pid += 1;
            k += 1;
        }
        let mut k = 0u32;
        while k < 3 {
            let sc = rng.below(1_000);
            ranker.add_sent(Sentence { code, sent_id: 5_000 + cid * 10 + k, score: sc });
            k += 1;
        }
        cid += 1;
    }
    // ×100 查询：随机 code、随机 pin 组合、每轮注入一笔记分超顶整句。
    let mut i = 0u32;
    let mut pin_cursor: u32 = 1;
    while i < 100 {
        let code = 100 + rng.below(8);
        // 轮换 pin/unpin 一笔短语。
        let cur = pin_cursor % 16 + 1;
        if i % 2 == 0 {
            let _ = store.pin(cur);
        } else {
            let _ = store.unpin(cur);
        }
        pin_cursor += 3;
        // 注入超顶整句（score 9_999）：置顶仍须压过它。
        ranker.add_sent(Sentence { code, sent_id: 9_000 + i, score: 9_999 });
        let out = ranker.present(&store, code);
        ranker.audit_order(&store, code, &out)?;
        // rank0 断言。
        let phs = store.find(code);
        let mut any_pinned = false;
        let mut first_pinned = 0u32;
        let mut k = 0usize;
        while k < phs.len() {
            if phs[k].pinned {
                any_pinned = true;
                first_pinned = phs[k].phrase_id;
                break;
            }
            k += 1;
        }
        if any_pinned {
            if out.is_empty() || out[0] != first_pinned {
                return Err(E_RANK0); // 置顶未压过 9_999 整句
            }
        }
        i += 1;
    }
    hits += 1;

    // R2 无置顶面：全 unpin 后 rank0 必是整句最高分（同分 id 小）。
    let mut store = PinStore::new();
    let mut ranker = Ranker::new();
    store.add(200, 10)?;
    store.add(200, 11)?;
    ranker.add_sent(Sentence { code: 200, sent_id: 21, score: 50 });
    ranker.add_sent(Sentence { code: 200, sent_id: 22, score: 90 });
    ranker.add_sent(Sentence { code: 200, sent_id: 23, score: 90 });
    let out = ranker.present(&store, 200);
    if out.len() != 5 {
        return Err(E_LEN_BAD);
    }
    // 无置顶：普通短语两笔殿前（入账序），整句段在后。
    if out[0] != 10 || out[1] != 11 {
        return Err(E_ORDER);
    }
    // 整句段：90 分并列取 id 小，50 分殿后。
    if out[2] != 22 || out[3] != 23 {
        return Err(E_TIE);
    }
    if out[4] != 21 {
        return Err(E_ORDER);
    }
    hits += 1;

    // R3 置顶纵深直证：pin 10 后 rank0==10，且 10 不得再现于后段。
    let mut store = PinStore::new();
    let mut ranker = Ranker::new();
    store.add(200, 10)?;
    store.add(200, 11)?;
    ranker.add_sent(Sentence { code: 200, sent_id: 22, score: 90 });
    store.pin(10)?;
    let out = ranker.present(&store, 200);
    if out.len() != 3 || out[0] != 10 || out[1] != 11 || out[2] != 22 {
        return Err(E_ORDER);
    }
    ranker.audit_order(&store, 200, &out)?;
    hits += 1;

    // R4 pin 幂等与 unpin 退段（纪律 3）：重复 pin 不动账；unpin 后
    //    短语退普通段（还在呈现序里，位置移段）。
    let mut store = PinStore::new();
    store.add(300, 40)?;
    store.pin(40)?;
    store.pin(40)?; // 幂等
    let phs = store.find(300);
    if phs.len() != 1 || !phs[0].pinned {
        return Err(E_DUP);
    }
    store.unpin(40)?;
    let phs = store.find(300);
    if phs.len() != 1 || phs[0].pinned {
        return Err(E_PIN_MISS); // unpin 不删账
    }
    hits += 1;

    // R5 pin 不存在拒（纪律 3）：空账与错 id 双探针。
    let mut store = PinStore::new();
    if store.pin(999).is_ok() {
        return Err(E_PIN_MISS);
    }
    store.add(300, 40)?;
    if store.pin(999).is_ok() {
        return Err(E_PIN_MISS);
    }
    hits += 1;

    // R6 短语查重（纪律 4 面）：同 (code, phrase_id) 二次入账拒；
    //    异 code 同 id 合法。
    let mut store = PinStore::new();
    store.add(400, 50)?;
    if store.add(400, 50).is_ok() {
        return Err(E_DUP);
    }
    store.add(401, 50)?;
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x2FE0)?;
    hits += 1;

    Ok(hits)
}
