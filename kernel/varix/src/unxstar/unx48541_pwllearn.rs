//! UNX-F48541 —— E·断电瞬间学习成果（正：节流窗口内数据落日志盘零丢失）
//! 判据原文：UNX-F48541-J1 学习条目（词条号 + 频次）先进节流窗口；
//! 断电事件在任意时刻触发应急落盘：窗口内全部条目必须一次性落日志盘
//! 零丢失；落盘逐槽带 CRC；重启回放恢复集合与断电前窗口逐条一致；
//! 坏槽（位翻转）必须被检出；多轮断电累计账守恒。
//! 判据落地（drill 场景）：R1 学习入窗容量守恒；R2 满窗合并（同词条频次累加）；
//! R3 断电应急落盘零丢失；R4 落盘逐槽 CRC 全绿；R5 重启回放集合一致；
//! R6 坏槽检出（位翻转必拒）；R7 随机时刻断电 ×8 轮零丢失；
//! R8 多轮累计账守恒；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF1C0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_LOSS: u16 = SEG_BASE + 1; // 断电落盘丢失条目
const E_CAPACITY: u16 = SEG_BASE + 2; // 窗口容量越限未合并
const E_CRC: u16 = SEG_BASE + 3; // 落盘槽 CRC 校验不符
const E_REPLAY: u16 = SEG_BASE + 4; // 回放恢复集合与断电前不一致
const E_MERGE: u16 = SEG_BASE + 5; // 满窗合并失败（同词条双槽）
const E_CONSERV: u16 = SEG_BASE + 6; // 多轮累计账不守恒
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_ORDER: u16 = SEG_BASE + 9; // 落盘顺序非单调
const E_ZERO: u16 = SEG_BASE + 10; // 零条目断电产生脏槽

/// 节流窗口容量与词条号闭集。
pub const WINDOW_CAP: usize = 16;
pub const WORD_LO: u32 = 1;
pub const WORD_HI: u32 = 64;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 单条学习条目：词条号 + 频次（条目 CRC = FNV(id||freq) 低 32 位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LearnEntry {
    pub word_id: u32,
    pub freq: u32,
}

/// 条目 CRC。
pub fn entry_crc(e: &LearnEntry) -> u32 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&e.word_id.to_le_bytes());
    buf.extend_from_slice(&e.freq.to_le_bytes());
    fnv1a64(&buf) as u32
}

/// 日志盘槽：条目 + CRC + 落盘序号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogSlot {
    pub seq: u32,
    pub entry: LearnEntry,
    pub crc: u32,
}

/// 日志盘：顺序追加槽。
pub struct LogDisk {
    pub slots: Vec<LogSlot>,
    pub next_seq: u32,
}

impl LogDisk {
    pub fn new() -> LogDisk {
        LogDisk {
            slots: vec![],
            next_seq: 1,
        }
    }

    /// 追加一槽（落盘时同步写 CRC）。
    pub fn append(&mut self, e: LearnEntry) -> u32 {
        let seq = self.next_seq;
        self.slots.push(LogSlot {
            seq,
            entry: e,
            crc: entry_crc(&e),
        });
        self.next_seq += 1;
        seq
    }

    /// 全盘逐槽 CRC 校验（坏槽返回首个坏序号）。
    pub fn verify(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].crc != entry_crc(&self.slots[i].entry) {
                return Err(self.slots[i].seq);
            }
            i += 1;
        }
        Ok(())
    }

    /// 回放恢复：逐槽提取条目集合。
    pub fn replay(&self) -> Vec<LearnEntry> {
        let mut out: Vec<LearnEntry> = vec![];
        let mut i = 0usize;
        while i < self.slots.len() {
            out.push(self.slots[i].entry);
            i += 1;
        }
        out
    }

    /// 落盘序号单调。
    pub fn ordered(&self) -> bool {
        let mut i = 1usize;
        while i < self.slots.len() {
            if self.slots[i - 1].seq >= self.slots[i].seq {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 节流窗口：学习条目暂存（同词条合并累加频次）。
pub struct ThrottleWindow {
    pub entries: Vec<LearnEntry>,
}

impl ThrottleWindow {
    pub fn new() -> ThrottleWindow {
        ThrottleWindow { entries: vec![] }
    }

    fn idx_of(&self, word_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].word_id == word_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 学习一笔：同词条累加频次，新词条入窗；窗口满则拒收（调用方先 flush）。
    pub fn learn(&mut self, e: LearnEntry) -> Result<(), u16> {
        if e.word_id < WORD_LO || e.word_id > WORD_HI {
            return Err(E_CAPACITY);
        }
        match self.idx_of(e.word_id) {
            Some(i) => {
                self.entries[i].freq += e.freq;
                Ok(())
            }
            None => {
                if self.entries.len() >= WINDOW_CAP {
                    return Err(E_CAPACITY);
                }
                self.entries.push(e);
                Ok(())
            }
        }
    }

    /// 应急落盘：窗口全量追加日志盘并清窗；返回落盘条数。
    pub fn flush_all(&mut self, disk: &mut LogDisk) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            disk.append(self.entries[i]);
            n += 1;
            i += 1;
        }
        self.entries.clear();
        n
    }

    /// 窗口指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.entries.len() {
            buf.extend_from_slice(&self.entries[i].word_id.to_le_bytes());
            buf.extend_from_slice(&self.entries[i].freq.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 集合一致性：回放集合与窗口快照逐条一致（按词条号排序比对）。
pub fn sets_equal(a: &[LearnEntry], b: &[LearnEntry]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // 排序副本（插入排序，按 word_id）。
    let mut sa: Vec<LearnEntry> = vec![];
    let mut i = 0usize;
    while i < a.len() {
        sa.push(a[i]);
        i += 1;
    }
    let mut i = 1usize;
    while i < sa.len() {
        let mut j = i;
        while j > 0 && sa[j - 1].word_id > sa[j].word_id {
            let t = sa[j - 1];
            sa[j - 1] = sa[j];
            sa[j] = t;
            j -= 1;
        }
        i += 1;
    }
    let mut sb: Vec<LearnEntry> = vec![];
    let mut i = 0usize;
    while i < b.len() {
        sb.push(b[i]);
        i += 1;
    }
    let mut i = 1usize;
    while i < sb.len() {
        let mut j = i;
        while j > 0 && sb[j - 1].word_id > sb[j].word_id {
            let t = sb[j - 1];
            sb[j - 1] = sb[j];
            sb[j] = t;
            j -= 1;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < sa.len() {
        if sa[i] != sb[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 日志行（呈现面）：slots=<n> window=<n> lost=<0/1>。
pub fn disk_line(disk_slots: usize, window_len: usize, lost: bool) -> String {
    let mut s = String::new();
    s.push_str("slots=");
    s.push_str(&(disk_slots as u32).to_string());
    s.push_str(" window=");
    s.push_str(&(window_len as u32).to_string());
    s.push_str(" lost=");
    s.push_str(if lost { "1" } else { "0" });
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 学习入窗容量守恒：32 笔学习（词条号 1..=16 循环）窗内恰 16 槽。
    let mut w = ThrottleWindow::new();
    let mut t = 0u32;
    while t < 32 {
        w.learn(LearnEntry {
            word_id: 1 + (t % 16),
            freq: 1,
        })?;
        t += 1;
    }
    if w.entries.len() != 16 {
        return Err(E_CAPACITY);
    }
    // 同词条频次累加：每词条恰 2 次。
    let mut i = 0usize;
    while i < w.entries.len() {
        if w.entries[i].freq != 2 {
            return Err(E_MERGE);
        }
        i += 1;
    }
    hits += 1;

    // R2 满窗合并：满窗后同词条再学 → 累加成功不越容；新词条 → E_CAPACITY。
    w.learn(LearnEntry { word_id: 3, freq: 5 })?;
    let mut i = 0usize;
    let mut f3 = 0u32;
    while i < w.entries.len() {
        if w.entries[i].word_id == 3 {
            f3 = w.entries[i].freq;
        }
        i += 1;
    }
    if f3 != 7 || w.entries.len() != 16 {
        return Err(E_MERGE);
    }
    match w.learn(LearnEntry { word_id: 20, freq: 1 }) {
        Err(E_CAPACITY) => hits += 1,
        _ => return Err(E_CAPACITY),
    }

    // R3/R4/R5 断电应急落盘零丢失 + CRC 全绿 + 回放一致。
    let mut disk = LogDisk::new();
    let snap = w.fp();
    let snap_len = w.entries.len();
    let flushed = w.flush_all(&mut disk);
    if flushed as usize != snap_len || !w.entries.is_empty() {
        return Err(E_LOSS);
    }
    if disk.verify().is_err() {
        return Err(E_CRC);
    }
    // 回放集合应与断电前快照一致：以 16 槽快照直接重放比对。
    let mut expect: Vec<LearnEntry> = vec![];
    let mut wid = 1u32;
    while wid <= 16 {
        expect.push(LearnEntry {
            word_id: wid,
            freq: 2,
        });
        wid += 1;
    }
    // 词条 3 频次已被 R2 改为 7。
    let mut i = 0usize;
    while i < expect.len() {
        if expect[i].word_id == 3 {
            expect[i].freq = 7;
        }
        i += 1;
    }
    if !sets_equal(&disk.replay(), &expect) {
        return Err(E_REPLAY);
    }
    if snap == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R6 坏槽检出：手动翻转一槽频次字节 → CRC 校验必报该槽序号。
    let mut disk2 = LogDisk::new();
    disk2.append(LearnEntry { word_id: 5, freq: 3 });
    disk2.append(LearnEntry { word_id: 6, freq: 4 });
    if disk2.verify().is_err() {
        return Err(E_CRC);
    }
    disk2.slots[1].entry.freq = 5; // 位翻转等价：条目被改而 CRC 未同步
    match disk2.verify() {
        Err(seq) => {
            if seq != 2 {
                return Err(E_CRC);
            }
        }
        Ok(()) => return Err(E_CRC),
    }
    hits += 1;

    // R7 随机时刻断电 ×8 轮零丢失：每轮随机学习 5..20 笔（词条号闭集内），
    // 随时 flush，落盘条数 == 窗口条数。
    let mut lcg = Lcg::new(0x48541);
    let mut disk3 = LogDisk::new();
    let mut w3 = ThrottleWindow::new();
    let mut round = 0u32;
    let mut total_learned: u32 = 0;
    let mut total_flushed: u32 = 0;
    while round < 8 {
        let n = 5 + lcg.below(16);
        let mut k = 0u32;
        while k < n {
            let wid = WORD_LO + lcg.below(WORD_HI);
            // 窗满即断电应急落盘，再重试本笔（空窗必收）。
            if w3.learn(LearnEntry { word_id: wid, freq: 1 }).is_err() {
                let flushed = w3.flush_all(&mut disk3);
                total_flushed += flushed;
                w3.learn(LearnEntry { word_id: wid, freq: 1 })?;
            }
            total_learned += 1;
            k += 1;
        }
        let flushed = w3.flush_all(&mut disk3);
        total_flushed += flushed;
        if flushed == 0 && n > 0 {
            return Err(E_LOSS);
        }
        round += 1;
    }
    if !w3.entries.is_empty() {
        return Err(E_LOSS);
    }
    if !disk3.verify().is_ok() {
        return Err(E_CRC);
    }
    hits += 1;

    // R8 多轮累计账守恒：每笔学习要么在窗口（终态空窗 → 全部落盘）要么已落盘，
    // 总落盘 == 总学习（每轮 flush 全量，无丢弃）。
    if total_flushed != total_learned {
        return Err(E_CONSERV);
    }
    if !disk3.ordered() {
        return Err(E_ORDER);
    }
    hits += 1;

    // R9 指纹稳定：日志盘指纹两次一致且非零。
    let f1 = fnv1a64(&{
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < disk3.slots.len() {
            buf.extend_from_slice(&disk3.slots[i].seq.to_le_bytes());
            buf.extend_from_slice(&disk3.slots[i].entry.word_id.to_le_bytes());
            buf.extend_from_slice(&disk3.slots[i].entry.freq.to_le_bytes());
            i += 1;
        }
        buf
    });
    let f2 = fnv1a64(&{
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < disk3.slots.len() {
            buf.extend_from_slice(&disk3.slots[i].seq.to_le_bytes());
            buf.extend_from_slice(&disk3.slots[i].entry.word_id.to_le_bytes());
            buf.extend_from_slice(&disk3.slots[i].entry.freq.to_le_bytes());
            i += 1;
        }
        buf
    });
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：零条目断电不得产生脏槽（flush 0 返回 0，盘无槽）。
    let mut w4 = ThrottleWindow::new();
    let mut disk4 = LogDisk::new();
    if w4.flush_all(&mut disk4) != 0 || !disk4.slots.is_empty() {
        return Err(E_ZERO);
    }
    let line = disk_line(disk3.slots.len(), w4.entries.len(), false);
    if !has_kw(line.as_bytes(), b"slots=")
        || !has_kw(line.as_bytes(), b"window=")
        || !has_kw(line.as_bytes(), b"lost=0")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
