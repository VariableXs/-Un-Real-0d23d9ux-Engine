//! UNX-F50138 B27 批防重 grep 执行记录（关键词族：usb/printer/bulk/1284）（0x43d60 段）
//!
//! 锚点：UNX-F50138-J1 批收口防重留痕；判据锚定 §6.1。
//!
//! 「防重」不是一句纪律，而是一份可回放的执行记录：同一个关键词在这批里
//! 只能被扫一次，四个关键词族（usb / printer / bulk / 1284）必须全部有痕迹，
//! 收口时把整本记录折叠成一个摘要。下次有人想「再扫一遍 usb」时，记录本身
//! 就会以 `E_DUP` 把他挡回去——不必依赖人的记性。
//!
//! 为什么族序也纳入约束：批收口要求两次执行能逐项对齐，族块一旦交错，
//! 「第 N 条是哪个族的」就会随扫描顺序漂移，摘要失去可比性。因此族码只许
//! 非降序推进（Usb→Printer→Bulk→Ieee1284），回退一律 `E_ORDER`。
//!
//! 数据结构（账本 + 注册表）：
//! * `Family` —— 四个关键词族及其静态词表（族表本身也是判据对象）；
//! * `GrepRecord` —— 一条执行记录（族/关键词/命中数/首末行/摘要）；
//! * `GrepLog` —— 记录账 + 已扫关键词注册表（去重）+ 收口状态。
//!
//! 错误闭集：`SEG_BASE + n`（0x43d60 段），九个码值全部被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43d60 独占 0x20）。
pub const SEG_BASE: u32 = 0x43d60;

pub const E_EMPTY: u16 = 1;
pub const E_KEYWORD: u16 = 2;
pub const E_DUP: u16 = 3;
pub const E_FAMILY: u16 = 4;
pub const E_SEQ: u16 = 5;
pub const E_ORDER: u16 = 6;
pub const E_RANGE: u16 = 7;
pub const E_DIGEST: u16 = 8;
pub const E_CAP: u16 = 9;

/// grep 记录错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrepError {
    Empty,
    Keyword,
    Dup,
    Family,
    Seq,
    Order,
    Range,
    Digest,
    Cap,
}

impl GrepError {
    /// 诊断码 = 段基址 + 序号。
    pub fn code(self) -> u32 {
        let n = match self {
            GrepError::Empty => E_EMPTY,
            GrepError::Keyword => E_KEYWORD,
            GrepError::Dup => E_DUP,
            GrepError::Family => E_FAMILY,
            GrepError::Seq => E_SEQ,
            GrepError::Order => E_ORDER,
            GrepError::Range => E_RANGE,
            GrepError::Digest => E_DIGEST,
            GrepError::Cap => E_CAP,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            GrepError::Empty => "corpus or log is empty",
            GrepError::Keyword => "keyword empty, too long or non-printable",
            GrepError::Dup => "keyword already scanned in this batch",
            GrepError::Family => "keyword family missing at close",
            GrepError::Seq => "record sequence is not dense",
            GrepError::Order => "family order went backwards or log closed",
            GrepError::Range => "corpus or line exceeds scan budget",
            GrepError::Digest => "record/log digest does not match content",
            GrepError::Cap => "log capacity reached",
        }
    }
}

/// 单个族的静态词表条数上界（族表自洽判据用它兜住空表）。
pub const MAX_KEYWORD_LEN: usize = 32;
/// 单次扫描的语料行数上限（防止一次「顺手 grep 整个仓库」把记录撑爆）。
pub const MAX_LINES: usize = 512;
/// 单行长度上限（UTF-8 字节）。
pub const MAX_LINE_BYTES: usize = 4096;
/// 记录本容量。
pub const MAX_RECORDS: usize = 32;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(FNV_PRIME)
}

fn fnv_u16(mut h: u64, v: u16) -> u64 {
    h = fnv_u8(h, (v & 0xff) as u8);
    fnv_u8(h, (v >> 8) as u8)
}

/// 关键词族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Usb,
    Printer,
    Bulk,
    Ieee1284,
}

impl Family {
    /// 族码（0..3）。族序即批收口顺序。
    pub fn code(self) -> u8 {
        match self {
            Family::Usb => 0,
            Family::Printer => 1,
            Family::Bulk => 2,
            Family::Ieee1284 => 3,
        }
    }

    /// 码值反解。
    pub fn from_code(c: u8) -> Result<Family, GrepError> {
        match c {
            0 => Ok(Family::Usb),
            1 => Ok(Family::Printer),
            2 => Ok(Family::Bulk),
            3 => Ok(Family::Ieee1284),
            _ => Err(GrepError::Family),
        }
    }

    /// 族标签。
    pub fn label(self) -> &'static str {
        match self {
            Family::Usb => "usb",
            Family::Printer => "printer",
            Family::Bulk => "bulk",
            Family::Ieee1284 => "1284",
        }
    }

    /// 静态词表（全部小写 ASCII；带空格的关键词也是合法模式）。
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            Family::Usb => &["usb", "xhci", "device descriptor"],
            Family::Printer => &["printer", "ipp", "print job"],
            Family::Bulk => &["bulk", "bulk-in", "bulk-out"],
            Family::Ieee1284 => &["1284", "ieee 1284", "parallel port"],
        }
    }

    /// 四族全表（按族序）。
    pub fn all() -> [Family; 4] {
        [Family::Usb, Family::Printer, Family::Bulk, Family::Ieee1284]
    }
}

fn ascii_lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

/// 关键词合法性：1..=32 字节、全部可打印 ASCII。
pub fn keyword_ok(kw: &str) -> bool {
    let bytes = kw.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_KEYWORD_LEN {
        return false;
    }
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if b < 0x20 || b > 0x7e {
            return false;
        }
        i += 1;
    }
    true
}

/// 归一化：ASCII 小写。「USB」与「usb」在防重语义里是同一个模式。
pub fn normalize(kw: &str) -> String {
    let mut out = String::with_capacity(kw.len());
    let bytes = kw.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            out.push(ascii_lower(*b) as char);
        }
        i += 1;
    }
    out
}

/// 统计 `line` 中 `needle`（已小写）的非重叠出现次数。
///
/// 为什么非重叠：grep 的语义是「行内命中次数」，重叠计数会让 "aaaa" 里的
/// "aa" 变成 3 次；批次统计一旦与工具语义不一致，留痕就不可复核。
pub fn count_occurrences(line: &str, needle: &str) -> u16 {
    let hay = line.as_bytes();
    let pat = needle.as_bytes();
    if pat.is_empty() || pat.len() > hay.len() {
        return 0;
    }
    let mut count = 0u16;
    let mut i = 0usize;
    while i + pat.len() <= hay.len() {
        let mut k = 0usize;
        let mut hit = true;
        while k < pat.len() {
            match (hay.get(i + k), pat.get(k)) {
                (Some(a), Some(b)) => {
                    if ascii_lower(*a) != *b {
                        hit = false;
                        break;
                    }
                }
                _ => {
                    hit = false;
                    break;
                }
            }
            k += 1;
        }
        if hit {
            count = count.saturating_add(1);
            i += pat.len();
        } else {
            i += 1;
        }
    }
    count
}

/// 一条 grep 执行记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepRecord {
    pub seq: u16,
    pub family: Family,
    pub keyword: String,
    pub hits: u16,
    pub first_line: u16,
    pub last_line: u16,
    pub digest: u64,
}

impl GrepRecord {
    /// 一行渲染（诊断用）。
    pub fn render(&self) -> String {
        format!(
            "{}:{} seq={} hits={} lines={}..{}",
            self.family.label(),
            self.keyword,
            self.seq,
            self.hits,
            self.first_line,
            self.last_line
        )
    }
}

/// 记录摘要：族 ‖ 关键词（小写）‖ 命中 ‖ 首末行 ‖ 序号。
pub fn record_digest(
    seq: u16,
    family: Family,
    keyword: &str,
    hits: u16,
    first: u16,
    last: u16,
) -> u64 {
    let mut h = FNV_OFFSET;
    h = fnv_u8(h, family.code());
    let kw = normalize(keyword);
    let bytes = kw.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fnv_u8(h, *b);
        }
        i += 1;
    }
    h = fnv_u16(h, hits);
    h = fnv_u16(h, first);
    h = fnv_u16(h, last);
    fnv_u16(h, seq)
}

/// 执行记录本（账本）+ 已扫关键词注册表 + 收口状态。
#[derive(Clone, Debug)]
pub struct GrepLog {
    records: Vec<GrepRecord>,
    seen: Vec<String>,
    closed: bool,
    closed_digest: u64,
}

impl GrepLog {
    /// 空记录本。
    pub fn new() -> GrepLog {
        GrepLog {
            records: Vec::with_capacity(MAX_RECORDS),
            seen: Vec::with_capacity(MAX_RECORDS),
            closed: false,
            closed_digest: 0,
        }
    }

    /// 记录条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 是否已收口。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 只读视图。
    pub fn records(&self) -> &[GrepRecord] {
        &self.records
    }

    /// 取第 `idx` 条记录。
    pub fn record(&self, idx: usize) -> Option<&GrepRecord> {
        self.records.get(idx)
    }

    /// 族覆盖位图。
    pub fn coverage(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                mask |= 1 << r.family.code();
            }
            i += 1;
        }
        mask
    }

    fn already_scanned(&self, norm: &str) -> bool {
        let mut i = 0usize;
        while i < self.seen.len() {
            if let Some(k) = self.seen.get(i) {
                if k.as_str() == norm {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 执行一次扫描并落账，返回记录序号。
    ///
    /// 命中为零也照样留痕——「这批里没有 1284」本身就是结论；不留痕会让
    /// 下次重复扫描无从判断。防重靠的是关键词注册表，不是命中数。
    pub fn run(
        &mut self,
        family: Family,
        keyword: &str,
        corpus: &[&str],
    ) -> Result<u16, GrepError> {
        if self.closed {
            return Err(GrepError::Order);
        }
        if corpus.is_empty() {
            return Err(GrepError::Empty);
        }
        if corpus.len() > MAX_LINES {
            return Err(GrepError::Range);
        }
        if !keyword_ok(keyword) {
            return Err(GrepError::Keyword);
        }
        if self.records.len() >= MAX_RECORDS {
            return Err(GrepError::Cap);
        }
        let norm = normalize(keyword);
        if self.already_scanned(&norm) {
            return Err(GrepError::Dup);
        }
        if let Some(last) = self.records.last() {
            if family.code() < last.family.code() {
                return Err(GrepError::Order);
            }
        }
        let mut hits = 0u16;
        let mut first = 0u16;
        let mut last = 0u16;
        let mut i = 0usize;
        while i < corpus.len() {
            let line = match corpus.get(i) {
                Some(l) => *l,
                None => break,
            };
            if line.len() > MAX_LINE_BYTES {
                return Err(GrepError::Range);
            }
            let c = count_occurrences(line, &norm);
            if c > 0 {
                hits = hits.saturating_add(c);
                let ln = 1 + i as u16;
                if first == 0 {
                    first = ln;
                }
                last = ln;
            }
            i += 1;
        }
        let seq = self.records.len() as u16;
        let digest = record_digest(seq, family, &norm, hits, first, last);
        self.records.push(GrepRecord {
            seq,
            family,
            keyword: norm.clone(),
            hits,
            first_line: first,
            last_line: last,
            digest,
        });
        self.seen.push(norm);
        Ok(seq)
    }

    /// 回放一条持久化记录（跨进程合并用）。先校验摘要，再校验序号与去重。
    pub fn replay(&mut self, rec: &GrepRecord) -> Result<(), GrepError> {
        if self.closed {
            return Err(GrepError::Order);
        }
        if rec.seq != self.records.len() as u16 {
            return Err(GrepError::Seq);
        }
        if !keyword_ok(&rec.keyword) {
            return Err(GrepError::Keyword);
        }
        let norm = normalize(&rec.keyword);
        if record_digest(rec.seq, rec.family, &norm, rec.hits, rec.first_line, rec.last_line)
            != rec.digest
        {
            return Err(GrepError::Digest);
        }
        if self.already_scanned(&norm) {
            return Err(GrepError::Dup);
        }
        if let Some(last) = self.records.last() {
            if rec.family.code() < last.family.code() {
                return Err(GrepError::Order);
            }
        }
        self.records.push(GrepRecord {
            seq: rec.seq,
            family: rec.family,
            keyword: norm.clone(),
            hits: rec.hits,
            first_line: rec.first_line,
            last_line: rec.last_line,
            digest: rec.digest,
        });
        self.seen.push(norm);
        Ok(())
    }

    /// 整本摘要（收口摘要与实时摘要共用同一折叠式）。
    ///
    /// 每条都从**明细字段**重算摘要而不是复用落账时存下的 `digest`：
    /// 留痕被改动时（哪怕是命中数）摘要必须跟着变，否则复核形同虚设。
    pub fn log_digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                let d = record_digest(r.seq, r.family, &r.keyword, r.hits, r.first_line, r.last_line);
                h ^= d.rotate_left(11);
                h = h.wrapping_mul(FNV_PRIME);
                h = fnv_u16(h, r.seq);
                h = fnv_u8(h, r.family.code());
            }
            i += 1;
        }
        h
    }

    /// 收口：要求四族齐全。收口后拒绝任何追加/回放。
    pub fn close(&mut self) -> Result<u64, GrepError> {
        if self.closed {
            return Err(GrepError::Order);
        }
        if self.records.is_empty() {
            return Err(GrepError::Empty);
        }
        if self.coverage() != 0x0f {
            return Err(GrepError::Family);
        }
        self.closed = true;
        self.closed_digest = self.log_digest();
        Ok(self.closed_digest)
    }

    /// 复核：重算整本摘要与收口值比对。
    pub fn verify(&self) -> Result<u64, GrepError> {
        if !self.closed {
            return Err(GrepError::Order);
        }
        let h = self.log_digest();
        if h != self.closed_digest {
            return Err(GrepError::Digest);
        }
        Ok(h)
    }

    /// 审计注入口：只改命中数、不更新摘要（模拟留痕被篡改）。
    pub fn tamper_hits(&mut self, idx: usize, hits: u16) -> Result<(), GrepError> {
        match self.records.get_mut(idx) {
            Some(r) => {
                r.hits = hits;
                Ok(())
            }
            None => Err(GrepError::Range),
        }
    }
}

fn run_four(log: &mut GrepLog) -> bool {
    let mut ok = true;
    if log.run(Family::Usb, "usb", &["usb bulk IN"]).is_err() {
        ok = false;
    }
    if log.run(Family::Printer, "printer", &["printer 1284"]).is_err() {
        ok = false;
    }
    if log.run(Family::Bulk, "bulk", &["bulk-out queue"]).is_err() {
        ok = false;
    }
    if log.run(Family::Ieee1284, "1284", &["ieee 1284 mode"]).is_err() {
        ok = false;
    }
    ok
}

/// 注入核：八类坏记录，逐类断言专属码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1 空语料
    let mut l1 = GrepLog::new();
    if l1.run(Family::Usb, "usb", &[]).err() == Some(GrepError::Empty) {
        caught += 1;
    }

    // 2 非法关键词（空串）
    let mut l2 = GrepLog::new();
    if l2.run(Family::Usb, "", &["usb"]).err() == Some(GrepError::Keyword) {
        caught += 1;
    }

    // 3 重复关键词（大小写归一后同形）
    let mut l3 = GrepLog::new();
    let ok = l3.run(Family::Usb, "usb", &["usb"]).is_ok();
    if ok && l3.run(Family::Usb, "USB", &["usb"]).err() == Some(GrepError::Dup) {
        caught += 1;
    }

    // 4 族序回退
    let mut l4 = GrepLog::new();
    let ok = l4.run(Family::Printer, "printer", &["printer"]).is_ok();
    if ok && l4.run(Family::Usb, "usb", &["usb"]).err() == Some(GrepError::Order) {
        caught += 1;
    }

    // 5 三族就收口
    let mut l5 = GrepLog::new();
    let _ = l5.run(Family::Usb, "usb", &["usb"]);
    let _ = l5.run(Family::Printer, "printer", &["printer"]);
    let _ = l5.run(Family::Bulk, "bulk", &["bulk"]);
    if l5.close().err() == Some(GrepError::Family) {
        caught += 1;
    }

    // 6 记录被改后复核
    let mut l6 = GrepLog::new();
    if run_four(&mut l6) {
        if l6.close().is_ok() {
            if l6.tamper_hits(0, 9).is_ok() {
                if l6.verify().err() == Some(GrepError::Digest) {
                    caught += 1;
                }
            }
        }
    }

    // 7 回放序号跳号
    let mut src = GrepLog::new();
    let _ = src.run(Family::Usb, "usb", &["usb"]);
    let mut l7 = GrepLog::new();
    if let Some(rec) = src.record(0) {
        if l7.replay(rec).is_ok() {
            if l7.replay(rec).err() == Some(GrepError::Seq) {
                caught += 1;
            }
        }
    }

    // 8 容量封顶：32 条之后第 33 条
    let mut l8 = GrepLog::new();
    let mut i = 0usize;
    let mut filled = true;
    while i < MAX_RECORDS {
        let kw = format!("k{}", i);
        if l8.run(Family::Usb, &kw, &["usb"]).is_err() {
            filled = false;
        }
        i += 1;
    }
    if filled && l8.run(Family::Usb, "z9", &["usb"]).err() == Some(GrepError::Cap) {
        caught += 1;
    }

    (caught, total)
}

/// 判据集（12 条）。
pub fn run_unx50138_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50138");

    // J1-1 族表自洽：四族词表非空、关键词合法、族码恰好 0..3
    let mut ok = true;
    let mut total_kw = 0usize;
    let mut mask = 0u8;
    let fams = Family::all();
    let mut i = 0usize;
    while i < fams.len() {
        let f = fams[i];
        let kws = f.keywords();
        if kws.is_empty() {
            ok = false;
        }
        mask |= 1 << f.code();
        let mut j = 0usize;
        while j < kws.len() {
            if let Some(kw) = kws.get(j) {
                if !keyword_ok(kw) {
                    ok = false;
                }
                if normalize(kw) != *kw {
                    ok = false;
                }
                total_kw += 1;
            }
            j += 1;
        }
        i += 1;
    }
    let ok = ok
        && total_kw >= 8
        && mask == 0x0f
        && Family::from_code(4).err() == Some(GrepError::Family);
    let det = format!("kw={} mask={:x}", total_kw, mask);
    s.add_dyn("50138-J1-族表自洽", ok, &det);

    // J1-2 命中计数：语料里 usb 一次、1284 一次
    let mut log = GrepLog::new();
    let r1 = log.run(Family::Usb, "usb", &["usb bulk IN", "printer 1284", "nothing"]);
    let r2 = log.run(Family::Ieee1284, "1284", &["usb bulk IN", "printer 1284", "nothing"]);
    let ok = r1 == Ok(0)
        && r2 == Ok(1)
        && log.record(0).map(|x| x.hits == 1) == Some(true)
        && log.record(1).map(|x| x.hits == 1) == Some(true);
    s.add("50138-J1-命中计数", ok, if ok { "" } else { "命中数不对" });

    // J1-3 大小写不敏感：同语料不同大小写同命中
    let mut lo = GrepLog::new();
    let mut up = GrepLog::new();
    let _ = lo.run(Family::Usb, "usb", &["USB usb"]);
    let _ = up.run(Family::Usb, "usb", &["usb usb"]);
    let ok = match (lo.record(0), up.record(0)) {
        (Some(a), Some(b)) => a.hits == b.hits && a.hits == 2,
        _ => false,
    };
    let det = format!("{:?} vs {:?}", lo.record(0).map(|x| x.hits), up.record(0).map(|x| x.hits));
    s.add_dyn("50138-J1-大小写归一", ok, &det);

    // J1-4 非重叠计数：aaaa 里的 aa 是 2 次
    let ok = count_occurrences("aaaa", "aa") == 2
        && count_occurrences("aaa", "aa") == 1
        && count_occurrences("", "aa") == 0
        && count_occurrences("usb", "usb") == 1;
    s.add("50138-J1-非重叠计数", ok, if ok { "" } else { "重叠计数" });

    // J1-5 首末行：跨行命中取最小/最大行号；零命中不留行号
    let mut log5 = GrepLog::new();
    let _ = log5.run(Family::Usb, "usb", &["x", "usb usb", "x", "usb"]);
    let mut log5b = GrepLog::new();
    let _ = log5b.run(Family::Usb, "usb", &["x", "y"]);
    let ok = match (log5.record(0), log5b.record(0)) {
        (Some(a), Some(b)) => {
            a.hits == 3
                && a.first_line == 2
                && a.last_line == 4
                && b.hits == 0
                && b.first_line == 0
                && b.last_line == 0
        }
        _ => false,
    };
    let det = format!("h={:?}", log5.record(0).map(|x| x.hits));
    s.add_dyn("50138-J1-首末行", ok, &det);

    // J1-6 防重：同族与跨族都拒
    let mut log6 = GrepLog::new();
    let _ = log6.run(Family::Usb, "usb", &["usb"]);
    let same = log6.run(Family::Usb, "USB", &["usb"]);
    let cross = log6.run(Family::Printer, "USB", &["usb usb"]);
    let ok = same.err() == Some(GrepError::Dup) && cross.err() == Some(GrepError::Dup);
    s.add("50138-J1-防重必拒", ok, if ok { "" } else { "重复关键词被收下" });

    // J1-7 族序：回退拒、收口后追加拒
    let mut log7 = GrepLog::new();
    let _ = log7.run(Family::Printer, "printer", &["printer"]);
    let back = log7.run(Family::Usb, "usb", &["usb"]);
    let mut log7b = GrepLog::new();
    let _ = run_four(&mut log7b);
    let closed_ok = log7b.close().is_ok();
    let after = log7b.run(Family::Ieee1284, "parallel port", &["parallel port"]);
    let ok = back.err() == Some(GrepError::Order)
        && closed_ok
        && after.err() == Some(GrepError::Order);
    s.add("50138-J1-族序纪律", ok, if ok { "" } else { "族序或收口未守" });

    // J1-8 收口覆盖：三族必拒、四族可收
    let mut log8 = GrepLog::new();
    let _ = log8.run(Family::Usb, "usb", &["usb"]);
    let _ = log8.run(Family::Printer, "printer", &["printer"]);
    let _ = log8.run(Family::Bulk, "bulk", &["bulk"]);
    let short = log8.close();
    let _ = log8.run(Family::Ieee1284, "1284", &["1284"]);
    let full = log8.close();
    let ok = short.err() == Some(GrepError::Family) && full.is_ok() && log8.is_closed();
    let digest = match full {
        Ok(v) => v,
        Err(_) => 0,
    };
    let det = format!("cov={:x} digest={:016x}", log8.coverage(), digest);
    s.add_dyn("50138-J1-收口覆盖", ok, &det);

    // J1-9 摘要确定/敏感：同构同摘要，改命中即变
    let mut a = GrepLog::new();
    let mut b = GrepLog::new();
    let _ = run_four(&mut a);
    let _ = run_four(&mut b);
    let da = a.close();
    let db = b.close();
    let mut c = GrepLog::new();
    let _ = c.run(Family::Usb, "usb", &["usb usb usb"]);
    let _ = c.run(Family::Printer, "printer", &["printer 1284"]);
    let _ = c.run(Family::Bulk, "bulk", &["bulk-out queue"]);
    let _ = c.run(Family::Ieee1284, "1284", &["ieee 1284 mode"]);
    let dc = c.close();
    let ok = match (da, db, dc) {
        (Ok(x), Ok(y), Ok(z)) => x == y && x != z,
        _ => false,
    };
    s.add("50138-J1-摘要确定", ok, if ok { "" } else { "同构摘要漂移" });

    // J1-10 复核：篡改打回；回放序号/摘要校验
    let mut log10 = GrepLog::new();
    let _ = run_four(&mut log10);
    let closed = log10.close().is_ok();
    let clean = log10.verify().is_ok();
    let _ = log10.tamper_hits(1, 42);
    let dirty = log10.verify().err() == Some(GrepError::Digest);
    let mut src = GrepLog::new();
    let _ = src.run(Family::Usb, "usb", &["usb"]);
    let mut dst = GrepLog::new();
    let replay_ok = match src.record(0) {
        Some(rec) => dst.replay(rec).is_ok(),
        None => false,
    };
    let ok = closed && clean && dirty && replay_ok;
    s.add("50138-J1-复核回放", ok, if ok { "" } else { "复核或回放失守" });

    // J1-11 边界：空语料/超长关键词/超行数
    let mut log11 = GrepLog::new();
    let empty = log11.run(Family::Usb, "usb", &[]);
    let long_kw = format!("{}{}", "usb-", "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    let killer = log11.run(Family::Usb, &long_kw, &["usb"]);
    let mut lines: Vec<&str> = Vec::with_capacity(MAX_LINES + 1);
    let mut i = 0usize;
    while i <= MAX_LINES {
        lines.push("usb");
        i += 1;
    }
    let over = log11.run(Family::Usb, "usb", &lines);
    let ok = empty.err() == Some(GrepError::Empty)
        && killer.err() == Some(GrepError::Keyword)
        && over.err() == Some(GrepError::Range);
    s.add("50138-J1-边界必拒", ok, if ok { "" } else { "边界未守" });

    // J1-12 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50138-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50138_checks_green() {
        let set = run_unx50138_checks();
        assert!(set.all_passed(), "UNX-F50138 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50138_non_overlapping_counts() {
        assert_eq!(count_occurrences("aaaa", "aa"), 2);
        assert_eq!(count_occurrences("USB usb UsB", "usb"), 3);
    }

    #[test]
    fn unx50138_log_closes_with_four_families() {
        let mut l = GrepLog::new();
        assert!(run_four(&mut l));
        let d = l.close();
        assert!(d.is_ok());
        assert_eq!(l.verify().ok(), d.ok());
        assert_eq!(l.coverage(), 0x0f);
        assert_eq!(l.len(), 4);
    }
}
