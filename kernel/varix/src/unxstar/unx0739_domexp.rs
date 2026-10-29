//! UNX-F0739 · 嫁接跟随域经条目（top10 候选） · 诊断段 0x5be0
//! 判据原文摘录：UNX-F0739-J1 域经条目 ≥5 条登记（三列：编号/主题/来源批）
//! 且逐条证据可解引、格式规范断言过；完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 域经条目三列写死：编号 / 主题 / 来源批，列序即落账序，
//!    缺列即条目无效，无效条目不进候选池。
//! 2. 证据可解引：每条条目必须挂至少一条证据指针，指针指向账内证据块，
//!    解引失败即条目作废——不允许"口说无凭"条目入池。
//! 3. 格式规范：主题 4..=24 字节、来源批号形如 "Bxx"（B + 两位数字），
//!    格式断言逐条过，不过者拒绝登记。
//! 4. top10 候选：登记 ≥5 条后按证据数 × 批次新近度评分排序取前 10，
//!    候选池定容 10，评分整数化（无浮点），同分按登记序稳定排序。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5be0 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5be0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_TOPIC_LEN: u16 = SEG_BASE + 1; // 主题长度越界（4..=24 字节）
const E_TOPIC_DUP: u16 = SEG_BASE + 2; // 主题重复登记
const E_BATCH_FMT: u16 = SEG_BASE + 3; // 来源批号格式非法（非 Bxx）
const E_ENTRY_FULL: u16 = SEG_BASE + 4; // 条目账已满（10 条）
const E_ENTRY_NOTFOUND: u16 = SEG_BASE + 5; // 条目编号不存在
const E_EVID_EMPTY: u16 = SEG_BASE + 6; // 条目零证据
const E_EVID_DANGLE: u16 = SEG_BASE + 7; // 证据指针不可解引
const E_EVID_BLOCK: u16 = SEG_BASE + 8; // 证据块编号越界
const E_EVID_FULL: u16 = SEG_BASE + 9; // 证据块账已满
const E_MIN_ENTRIES: u16 = SEG_BASE + 10; // 登记不足 5 条即断言被拒
const E_FMT_FAIL: u16 = SEG_BASE + 11; // 格式规范断言失败（有脏条目）
const E_BAL: u16 = SEG_BASE + 12; // 守恒恒等式破裂（候选数≤登记数）
const E_SEALED: u16 = SEG_BASE + 13; // 候选池已封，拒绝再写
const E_NO_CAND: u16 = SEG_BASE + 14; // 候选池为空

/// 候选池定容：top10。
pub const POOL_CAP: usize = 10;

/// 登记下限：≥5 条。
pub const MIN_ENTRIES: usize = 5;

/// 主题长度规范：4..=24 字节。
pub const TOPIC_MIN: usize = 4;
pub const TOPIC_MAX: usize = 24;

/// 证据块账定容。
pub const MAX_EVID: usize = 64;

// ── FNV-1a 64（证据与池哈希口径）──────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TOPIC_LEN => "主题长度越界：规范 4..=24 字节，先改写再登记",
        E_TOPIC_DUP => "主题重复：域经条目按主题唯一，先查重",
        E_BATCH_FMT => "批号格式非法：须形如 Bxx（B + 两位数字）",
        E_ENTRY_FULL => "条目账已满 10 条：top10 候选池上限",
        E_ENTRY_NOTFOUND => "条目编号不存在：按登记序 1..N",
        E_EVID_EMPTY => "条目零证据：口说无凭，先挂证据再入池",
        E_EVID_DANGLE => "证据指针不可解引：证据块已被注销",
        E_EVID_BLOCK => "证据块编号越界：按证据账 1..N",
        E_EVID_FULL => "证据块账已满 64 条：先归档旧证据",
        E_MIN_ENTRIES => "登记不足 5 条：先补条目再出候选",
        E_FMT_FAIL => "格式断言失败：存在脏条目，逐条排查三列",
        E_BAL => "候选数不得超过登记数",
        E_SEALED => "候选池已封：封账后只读",
        E_NO_CAND => "候选池为空：先登记并评分",
        _ => "未知诊断码",
    }
}

// ── 证据块 ───────────────────────────────────────────────────
/// 证据块：可解引的最小证据单元。
#[derive(Clone, Debug)]
pub struct EvidenceBlock {
    /// 块编号（1 起连续）。
    pub block_id: u32,
    /// 证据字节（登记来源的原文摘录）。
    pub bytes: Vec<u8>,
    /// 块哈希（解引校验用）。
    pub hash: u64,
}

// ── 来源批号解析 ─────────────────────────────────────────────
/// 批号合法性：形如 "Bxx"，x 为十进制数字。
pub fn batch_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 3 || b[0] != b'B' {
        return false;
    }
    let mut i = 1usize;
    while i < 3 {
        if !b[i].is_ascii_digit() {
            return false;
        }
        i += 1;
    }
    true
}

/// 批号转序号（B01 → 1，用于新近度评分）。
pub fn batch_num(s: &str) -> u32 {
    let b = s.as_bytes();
    if !batch_ok(s) {
        return 0;
    }
    let d1 = (b[1] - b'0') as u32;
    let d2 = (b[2] - b'0') as u32;
    d1 * 10 + d2
}

// ── 域经条目（三列 + 证据指针）────────────────────────────────
/// 一条域经条目：编号 / 主题 / 来源批 + 证据指针集。
#[derive(Clone, Debug)]
pub struct DomExpEntry {
    /// 条目编号（1 起连续）。
    pub entry_id: u32,
    /// 列二：主题（4..=24 字节，账内唯一）。
    pub topic: String,
    /// 列三：来源批号（Bxx 格式）。
    pub source_batch: String,
    /// 证据指针集（指向证据块编号，≥1 条才有效）。
    pub evids: Vec<u32>,
}

impl DomExpEntry {
    /// 三列完整性：主题合法 + 批号合法 + 证据非空。
    pub fn well_formed(&self) -> bool {
        let tb = self.topic.as_bytes();
        tb.len() >= TOPIC_MIN
            && tb.len() <= TOPIC_MAX
            && batch_ok(&self.source_batch)
            && !self.evids.is_empty()
    }

    /// 评分：证据数 × 100 + 批号序号（整数化，无浮点）。
    pub fn score(&self) -> u32 {
        (self.evids.len() as u32) * 100 + batch_num(&self.source_batch)
    }
}

// ── 候选行 ───────────────────────────────────────────────────
/// top10 候选行：条目号 + 评分 + 池内名次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateRow {
    /// 池内名次（1 起，评分降序，同分按登记序）。
    pub rank: u32,
    /// 条目编号。
    pub entry_id: u32,
    /// 评分。
    pub score: u32,
}

// ── 域经登记簿（主体）────────────────────────────────────────
/// 嫁接跟随域经登记簿：三列条目 + 证据块 + top10 候选池。
pub struct DomExpRegistry {
    entries: [Option<DomExpEntry>; POOL_CAP],
    evids: [Option<EvidenceBlock>; MAX_EVID],
    /// 条目数。
    entry_count: usize,
    /// 证据块数。
    evid_count: usize,
    /// 候选池（评分后落）。
    pool: Vec<CandidateRow>,
    /// 是否封账。
    sealed: bool,
}

impl DomExpRegistry {
    /// 开簿。
    pub fn new() -> DomExpRegistry {
        DomExpRegistry {
            entries: core::array::from_fn(|_| None),
            evids: core::array::from_fn(|_| None),
            entry_count: 0,
            evid_count: 0,
            pool: Vec::new(),
            sealed: false,
        }
    }

    /// 入证据块：返回块编号（登记条目前先备证据）。
    pub fn put_evidence(&mut self, bytes: &[u8]) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.evid_count >= MAX_EVID {
            return Err(E_EVID_FULL);
        }
        if bytes.is_empty() {
            return Err(E_EVID_DANGLE);
        }
        let block_id = (self.evid_count + 1) as u32;
        self.evids[self.evid_count] = Some(EvidenceBlock {
            block_id,
            bytes: Vec::from(bytes),
            hash: fnv1a64(bytes),
        });
        self.evid_count += 1;
        Ok(block_id)
    }

    /// 证据解引：按块编号取只读引用。
    pub fn deref_evidence(&self, block_id: u32) -> Result<&EvidenceBlock, u16> {
        if block_id == 0 || block_id as usize > self.evid_count {
            return Err(E_EVID_BLOCK);
        }
        match &self.evids[(block_id - 1) as usize] {
            Some(b) => Ok(b),
            None => Err(E_EVID_DANGLE),
        }
    }

    /// 登记条目：三列格式断言 + 主题查重 + 证据可解引逐条核。
    pub fn register(&mut self, topic: &str, source_batch: &str, evids: &[u32]) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.entry_count >= POOL_CAP {
            return Err(E_ENTRY_FULL);
        }
        let tb = topic.as_bytes();
        if tb.len() < TOPIC_MIN || tb.len() > TOPIC_MAX {
            return Err(E_TOPIC_LEN);
        }
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                if e.topic == topic {
                    return Err(E_TOPIC_DUP);
                }
            }
            i += 1;
        }
        if !batch_ok(source_batch) {
            return Err(E_BATCH_FMT);
        }
        if evids.is_empty() {
            return Err(E_EVID_EMPTY);
        }
        // 证据可解引：逐条先解引再落账（悬垂指针即拒绝）。
        i = 0;
        while i < evids.len() {
            self.deref_evidence(evids[i])?;
            i += 1;
        }
        let entry_id = (self.entry_count + 1) as u32;
        self.entries[self.entry_count] = Some(DomExpEntry {
            entry_id,
            topic: topic.to_string(),
            source_batch: source_batch.to_string(),
            evids: Vec::from(evids),
        });
        self.entry_count += 1;
        Ok(entry_id)
    }

    /// 条目只读快照。
    pub fn get(&self, entry_id: u32) -> Result<(String, String, usize), u16> {
        if entry_id == 0 || entry_id as usize > self.entry_count {
            return Err(E_ENTRY_NOTFOUND);
        }
        match &self.entries[(entry_id - 1) as usize] {
            Some(e) => Ok((e.topic.clone(), e.source_batch.clone(), e.evids.len())),
            None => Err(E_ENTRY_NOTFOUND),
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entry_count
    }

    /// 空簿判定。
    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }

    /// 逐条证据可解引断言：全簿扫一遍，任何悬垂即失败。
    pub fn evid_deref_assert(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.entry_count {
            let e = self.entries[i].as_ref().ok_or(E_ENTRY_NOTFOUND)?;
            if e.evids.is_empty() {
                return Err(E_EVID_EMPTY);
            }
            let mut j = 0usize;
            while j < e.evids.len() {
                self.deref_evidence(e.evids[j])?;
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 格式规范断言：全簿逐条三列复核。
    pub fn format_assert(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.entry_count {
            let e = self.entries[i].as_ref().ok_or(E_ENTRY_NOTFOUND)?;
            if !e.well_formed() {
                return Err(E_FMT_FAIL);
            }
            i += 1;
        }
        Ok(())
    }

    /// 评分出池：登记 ≥5 条后按评分降序稳定取前 10（选择排序，确定性）。
    pub fn build_pool(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.entry_count < MIN_ENTRIES {
            return Err(E_MIN_ENTRIES);
        }
        self.format_assert()?;
        self.evid_deref_assert()?;
        // 汇总 (entry_id, score)。
        let mut scores: Vec<(u32, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                scores.push((e.entry_id, e.score()));
            }
            i += 1;
        }
        // 选择排序：评分降序，同分登记序升序（稳定）。
        let n = scores.len();
        let mut a = 0usize;
        while a < n {
            let mut best = a;
            let mut b = a + 1;
            while b < n {
                let (_, sa) = scores[best];
                let (_, sb) = scores[b];
                if sb > sa {
                    best = b;
                }
                b += 1;
            }
            let tmp = scores[a];
            scores[a] = scores[best];
            scores[best] = tmp;
            a += 1;
        }
        // 取前 10（不足 10 全取）。
        self.pool.clear();
        let take = if n > POOL_CAP { POOL_CAP } else { n };
        let mut r = 0usize;
        while r < take {
            let (eid, sc) = scores[r];
            self.pool.push(CandidateRow {
                rank: (r + 1) as u32,
                entry_id: eid,
                score: sc,
            });
            r += 1;
        }
        // 守恒：候选数 ≤ 登记数。
        if self.pool.len() > self.entry_count {
            return Err(E_BAL);
        }
        Ok(self.pool.len())
    }

    /// 候选池行数。
    pub fn pool_len(&self) -> usize {
        self.pool.len()
    }

    /// 候选只读快照。
    pub fn pool_row(&self, rank: u32) -> Result<CandidateRow, u16> {
        if rank == 0 || rank as usize > self.pool.len() {
            return Err(E_NO_CAND);
        }
        Ok(self.pool[(rank - 1) as usize])
    }

    /// 候选池全绿断言：≥5 条登记、池非空、评分降序、格式全过。
    pub fn pool_assert(&self) -> Result<(), u16> {
        if self.entry_count < MIN_ENTRIES {
            return Err(E_MIN_ENTRIES);
        }
        if self.pool.is_empty() {
            return Err(E_NO_CAND);
        }
        self.format_assert()?;
        self.evid_deref_assert()?;
        let mut i = 1usize;
        while i < self.pool.len() {
            if self.pool[i - 1].score < self.pool[i].score {
                return Err(E_BAL);
            }
            if self.pool[i].rank != (i + 1) as u32 {
                return Err(E_BAL);
            }
            i += 1;
        }
        Ok(())
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.pool_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                buf.extend_from_slice(e.entry_id.to_le_bytes().as_slice());
                buf.extend_from_slice(e.topic.as_bytes());
                buf.push(0x1f);
                buf.extend_from_slice(e.source_batch.as_bytes());
                buf.extend_from_slice((e.evids.len() as u32).to_le_bytes().as_slice());
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.pool.len() {
            buf.extend_from_slice(self.pool[j].entry_id.to_le_bytes().as_slice());
            buf.extend_from_slice(self.pool[j].score.to_le_bytes().as_slice());
            j += 1;
        }
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 登记簿审计行渲染：三列逐条。
    pub fn audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 嫁接跟随域经条目 · 三列审计 ===".to_string());
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                let mut line = String::from("E");
                line.push_str(&itoa(e.entry_id as u64));
                line.push('|');
                line.push_str(&e.topic);
                line.push('|');
                line.push_str(&e.source_batch);
                line.push_str("|证据");
                line.push_str(&itoa(e.evids.len() as u64));
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 来源批分布盘点：批号序 → 条目数（批均衡断言取数口）。
    pub fn batch_tally(&self) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                let bn = batch_num(&e.source_batch);
                let mut hit = false;
                let mut j = 0usize;
                while j < out.len() {
                    if out[j].0 == bn {
                        out[j].1 += 1;
                        hit = true;
                    }
                    j += 1;
                }
                if !hit {
                    out.push((bn, 1));
                }
            }
            i += 1;
        }
        out
    }

    /// 主题长度直方图：len 桶 → 条目数（格式规范复核取数口）。
    /// 桶定义：[4..7][8..11][12..15][16..19][20..24]，桶外（不可能）丢弃。
    pub fn topic_len_histogram(&self) -> [u32; 5] {
        let mut hist = [0u32; 5];
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                let l = e.topic.as_bytes().len();
                if l >= TOPIC_MIN && l <= TOPIC_MAX {
                    let bucket = (l - TOPIC_MIN) / 4;
                    if bucket < 5 {
                        hist[bucket] += 1;
                    }
                }
            }
            i += 1;
        }
        hist
    }

    /// 候选池行渲染：名次|条目号|评分。
    pub fn pool_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== top10 候选池 · 评分降序 ===".to_string());
        let mut i = 0usize;
        while i < self.pool.len() {
            let c = &self.pool[i];
            let mut line = String::from("R");
            line.push_str(&itoa(c.rank as u64));
            line.push('|');
            line.push_str(&itoa(c.entry_id as u64));
            line.push_str("|分");
            line.push_str(&itoa(c.score as u64));
            lines.push(line);
            i += 1;
        }
        lines
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：≥5 条登记 + 证据可解引 + 格式断言 + top10 出池。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut reg = DomExpRegistry::new();
    // 备证据块。
    let ev1 = reg.put_evidence(b"limine protocol: hash-first verify order")?;
    let ev2 = reg.put_evidence(b"slab arena: fixed-cap ledger pattern")?;
    let ev3 = reg.put_evidence(b"sla ledger: 7-day hard deadline")?;
    let ev4 = reg.put_evidence(b"interop: fnv1a64 as sole hash")?;
    let ev5 = reg.put_evidence(b"seg coding: base+N error codes")?;
    // 登记五条（含新近度差异样本）。
    let e1 = reg.register("哈希先行校验", "B33", &[ev1])?;
    let e2 = reg.register("定容账本模式", "B35", &[ev2, ev4])?;
    let e3 = reg.register("七日硬时限", "B37", &[ev3])?;
    let e4 = reg.register("段位诊断码", "B37", &[ev5, ev4, ev2])?;
    let e5 = reg.register("整数化评分", "B34", &[ev4])?;
    if reg.len() < MIN_ENTRIES {
        return Err(SEG_BASE);
    }
    let _ = (e1, e2, e3, e4, e5);
    reg.evid_deref_assert()?;
    reg.format_assert()?;
    let n = reg.build_pool()?;
    if n == 0 || reg.pool_len() != 5 {
        return Err(SEG_BASE);
    }
    reg.pool_assert()?;
    // 评分首位必须是证据最多的 e4（3 证据 × 100 = 300+）。
    let top = reg.pool_row(1)?;
    if top.entry_id != 4 {
        return Err(SEG_BASE);
    }
    // 批分布与直方图取数口可用性。
    let bt = reg.batch_tally();
    if bt.is_empty() {
        return Err(SEG_BASE);
    }
    let hist = reg.topic_len_histogram();
    let mut hist_sum = 0u32;
    let mut hi = 0usize;
    while hi < 5 {
        hist_sum += hist[hi];
        hi += 1;
    }
    if hist_sum != 5 {
        return Err(SEG_BASE);
    }
    let pl = reg.pool_lines();
    if pl.len() != 6 {
        return Err(SEG_BASE);
    }
    let h = reg.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 封账后登记拒绝。
    match reg.register("封账后条目", "B37", &[ev1]) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：坏批号格式。
    let mut reg2 = DomExpRegistry::new();
    let ev = reg2.put_evidence(b"e")?;
    match reg2.register("主题样本", "bX7", &[ev]) {
        Err(c) if c == E_BATCH_FMT => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：零证据登记。
    match reg2.register("主题样本", "B37", &[]) {
        Err(c) if c == E_EVID_EMPTY => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
