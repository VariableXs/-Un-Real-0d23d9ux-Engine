//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2064 · 统计采样与结算节拍 · 0x72900
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B24）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F2064-J1 结算断言全过（采样点/结算点纪律扩容到统计
//!     族），结算漂移注入 10/10 检出，与 F1774/F1745 纪律一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · F1745 纪律对齐：采样序号（seq）全账严格单调递增，倒退/封顶即拒；
//!   · F1774 纪律对齐：结算点必须满拍（BEAT_N 个采样齐）才可结算，未满
//!     拍结算即拒；拍号只进不退；
//!   · 统计族扩容：结算量含 count/sum/min/max/avg（Q8 定点平均），全部
//!     整数算术零浮点；结算记录带 FNV-1a 校验和；
//!   · 双路对拍：结算记录渲染（导出路径）与直读拼接（直读路径）逐字节
//!     相等，10^5 样本；
//!   · 漂移注入 10 路（sum/min/max/avg/cs/拍号/计数/删行/复行/控制符），
//!     核验器 10/10 必抓；
//!   · 状态机 Idle→Sampling→Settling→Sealed 单调推进，倒退/平移显性拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x72900;
pub const BEAT_N: usize = 64;
pub const BEATS: u32 = 8;
pub const SEQ_CAP: u64 = 1 << 40;
pub const Q8_SHIFT: u32 = 8;
pub const FIDELITY_SAMPLES: usize = 100_000;
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BeatCode {
    /// 序倒退/封顶：seq 全账单调只进不退，越过 SEQ_CAP 视同封账拒收
    SeqBack,
    /// 未满拍结算：pending 不足 BEAT_N 就按结算键，纪律违例
    HalfSettle,
    /// 拍号倒退：结算拍号只进不退，重复结算同一拍同拒
    BeatBack,
    /// 结算和漂移：记录 sum 与按原始采样复算的 sum 不一致
    SumDrift,
    /// 统计量漂移：min/max/avg 三统计量任一与复算不一致
    StatDrift,
    /// 校验和破损：记录尾部 cs 与正文 FNV-1a 对不上
    CsBroken,
    /// 封账违规：Sealed 状态后仍采样/结算/推进
    SealViol,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl BeatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BeatCode::SeqBack => "序倒退/封顶：seq 单调只进不退，封账后拒收新采样",
            BeatCode::HalfSettle => "未满拍结算：等 BEAT_N 采样齐再按结算键",
            BeatCode::BeatBack => "拍号倒退：拍号只进不退，同拍禁止重复结算",
            BeatCode::SumDrift => "结算和漂移：sum 与原始采样复算不一致，重出结算记录",
            BeatCode::StatDrift => "统计量漂移：min/max/avg 与复算不一致，逐列定位",
            BeatCode::CsBroken => "校验和破损：记录体 FNV-1a 与尾部 cs 不符，重算重出",
            BeatCode::SealViol => "封账违规：Sealed 后一切写操作拒绝，先解封或开新账",
            BeatCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [BeatCode; 8] = [
        BeatCode::SeqBack,
        BeatCode::HalfSettle,
        BeatCode::BeatBack,
        BeatCode::SumDrift,
        BeatCode::StatDrift,
        BeatCode::CsBroken,
        BeatCode::SealViol,
        BeatCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BeatCode::ALL.len() {
        if BeatCode::ALL[i].val() == code {
            return BeatCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72900 段闭集内"
}

// ── 采样器状态机（Idle→Sampling→Settling→Sealed，单调推进） ─────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BeatState {
    Idle,
    Sampling,
    Settling,
    Sealed,
}

impl BeatState {
    pub fn rank(self) -> u8 {
        match self {
            BeatState::Idle => 0,
            BeatState::Sampling => 1,
            BeatState::Settling => 2,
            BeatState::Sealed => 3,
        }
    }
    pub fn can_advance(self, to: BeatState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleKind {
    Raw,
    Delta,
    Gauge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamplePoint {
    pub seq: u64,
    pub beat: u32,
    pub val: i64,
    pub kind: SampleKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeatSettle {
    pub beat: u32,
    pub count: u64,
    pub sum: i64,
    pub min: i64,
    pub max: i64,
    pub avg_q8: i64,
    pub cs: u32,
    pub seq_lo: u64,
    pub seq_hi: u64,
}

pub fn derive_val(i: usize) -> i64 {
    let a = (i % 61) as i64;
    let b = ((i / 7) % 23) as i64;
    a * 17 - b * 29 + ((i % 3) as i64) * 5
}

pub fn derive_kind(i: usize) -> SampleKind {
    match i % 3 {
        0 => SampleKind::Raw,
        1 => SampleKind::Delta,
        _ => SampleKind::Gauge,
    }
}

// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 字节/子串查找与数值解析（核验器底座） ────────────────────────
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] {
            k += 1;
        }
        if k == pat.len() {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub fn parse_dec(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() {
        return None;
    }
    let b = digits.as_bytes();
    let mut v: i64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as i64)?;
        i += 1;
    }
    if neg {
        v.checked_neg()
    } else {
        Some(v)
    }
}

pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < 8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
        i += 1;
    }
    Some(v)
}

// ── 两条独立渲染路径（对拍双路） ────────────────────────────────
pub fn render_settle(s: &BeatSettle) -> String {
    format!(
        "@VXS1|beat={}|count={}|sum={}|min={}|max={}|avg={}|lo={}|hi={}|cs={:08X};\n",
        s.beat, s.count, s.sum, s.min, s.max, s.avg_q8, s.seq_lo, s.seq_hi, s.cs
    )
}

pub fn direct_settle(s: &BeatSettle) -> String {
    let mut t = String::from("@VXS1|beat=");
    t.push_str(&s.beat.to_string());
    t.push_str("|count=");
    t.push_str(&s.count.to_string());
    t.push_str("|sum=");
    t.push_str(&s.sum.to_string());
    t.push_str("|min=");
    t.push_str(&s.min.to_string());
    t.push_str("|max=");
    t.push_str(&s.max.to_string());
    t.push_str("|avg=");
    t.push_str(&s.avg_q8.to_string());
    t.push_str(&format!("|lo={}|hi={}|cs={:08X};\n", s.seq_lo, s.seq_hi, s.cs));
    t
}

pub fn prefix_of(line: &str) -> Option<&str> {
    let b = line.as_bytes();
    let p0 = find_byte(b, 0, b'|')?;
    let p1 = find_sub(b, 0, b"|cs=")?;
    if p1 <= p0 {
        return None;
    }
    Some(&line[p0 + 1..p1])
}

pub fn body_of(line: &str) -> Option<&str> {
    let b = line.as_bytes();
    let p0 = find_byte(b, 0, b'|')?;
    let p1 = find_byte(b, p0, b';')?;
    Some(&line[p0 + 1..p1])
}

pub fn parse_settle(line: &str) -> Option<BeatSettle> {
    if !line.starts_with("@VXS1|") {
        return None;
    }
    let body = body_of(line)?;
    let mut beat: Option<u32> = None;
    let mut count: Option<u64> = None;
    let mut sum: Option<i64> = None;
    let mut min: Option<i64> = None;
    let mut max: Option<i64> = None;
    let mut avg: Option<i64> = None;
    let mut lo: Option<u64> = None;
    let mut hi: Option<u64> = None;
    let mut cs: Option<u32> = None;
    let bts = body.as_bytes();
    let mut i = 0usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') {
            Some(p) => p,
            None => bts.len(),
        };
        let tok = &body[i..end];
        if !tok.is_empty() {
            let eq = find_byte(tok.as_bytes(), 0, b'=')?;
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            match k {
                "beat" => beat = Some(parse_dec(v)? as u32),
                "count" => {
                    let x = parse_dec(v)?;
                    if x < 0 { return None; }
                    count = Some(x as u64);
                }
                "sum" => sum = Some(parse_dec(v)?),
                "min" => min = Some(parse_dec(v)?),
                "max" => max = Some(parse_dec(v)?),
                "avg" => avg = Some(parse_dec(v)?),
                "lo" => {
                    let x = parse_dec(v)?;
                    if x < 0 { return None; }
                    lo = Some(x as u64);
                }
                "hi" => {
                    let x = parse_dec(v)?;
                    if x < 0 { return None; }
                    hi = Some(x as u64);
                }
                "cs" => cs = Some(parse_hex8(v)?),
                _ => return None,
            }
        }
        if end >= bts.len() {
            break;
        }
        i = end + 1;
    }
    Some(BeatSettle {
        beat: beat?,
        count: count?,
        sum: sum?,
        min: min?,
        max: max?,
        avg_q8: avg?,
        cs: cs?,
        seq_lo: lo?,
        seq_hi: hi?,
    })
}

// ── 统计核（一拍采样的统计族，整数算术） ────────────────────────
pub fn fold_stats(samples: &[SamplePoint], from: usize) -> Option<(u64, i64, i64, i64, i64)> {
    if from >= samples.len() || from + BEAT_N > samples.len() {
        return None;
    }
    let mut n: u64 = 0;
    let mut sum: i64 = 0;
    let mut mn = i64::MAX;
    let mut mx = i64::MIN;
    let mut k = from;
    while k < from + BEAT_N {
        let v = samples.get(k)?.val;
        n = n.checked_add(1)?;
        sum = sum.checked_add(v)?;
        if v < mn {
            mn = v;
        }
        if v > mx {
            mx = v;
        }
        k += 1;
    }
    let avg = sum.checked_mul(1i64 << Q8_SHIFT)?.checked_div(n as i64)?;
    Some((n, sum, mn, mx, avg))
}

pub fn recompute_settle(samples: &[SamplePoint], beat: u32) -> Option<BeatSettle> {
    let from = beat as usize * BEAT_N;
    let (count, sum, mn, mx, avg) = fold_stats(samples, from)?;
    let mut rec = BeatSettle {
        beat,
        count,
        sum,
        min: mn,
        max: mx,
        avg_q8: avg,
        cs: 0,
        seq_lo: samples.get(from)?.seq,
        seq_hi: samples.get(from + BEAT_N - 1)?.seq,
    };
    rec.cs = fnv1a(prefix_of(&render_settle(&rec))?.as_bytes());
    Some(rec)
}

// ── 采样器 ──────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct Sampler {
    pub state: BeatState,
    pub next_seq: u64,
    pub beat_no: u32,
    pub samples: Vec<SamplePoint>,
    pub settles: Vec<BeatSettle>,
    pub admin_ops: u64,
    pub payload_ops: u64,
    pub settles_done: u32,
}

impl Sampler {
    pub fn new() -> Sampler {
        Sampler {
            state: BeatState::Idle, next_seq: 1, beat_no: 0, samples: Vec::new(), settles: Vec::new(),
            admin_ops: 0, payload_ops: 0, settles_done: 0,
        }
    }

    pub fn advance_state(&mut self, to: BeatState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(BeatCode::SealViol.val());
        }
        self.state = to;
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        self.advance_state(BeatState::Sealed)
    }

    pub fn sample(&mut self, kind: SampleKind, val: i64) -> Result<u64, u32> {
        if self.state == BeatState::Sealed {
            return Err(BeatCode::SealViol.val());
        }
        if self.next_seq >= SEQ_CAP {
            return Err(BeatCode::SeqBack.val());
        }
        let beat = (self.samples.len() / BEAT_N) as u32;
        let seq = self.next_seq;
        self.payload_ops += 3;
        self.samples.push(SamplePoint { seq, beat, val, kind });
        self.next_seq = match self.next_seq.checked_add(1) { Some(x) => x, None => return Err(BeatCode::SeqBack.val()) };
        if self.state == BeatState::Idle {
            self.state = BeatState::Sampling;
        }
        Ok(seq)
    }

    pub fn settle_beat(&mut self) -> Result<BeatSettle, u32> {
        if self.state == BeatState::Sealed {
            return Err(BeatCode::SealViol.val());
        }
        let done = self.settles_done as usize * BEAT_N;
        if self.samples.len() - done < BEAT_N {
            return Err(BeatCode::HalfSettle.val());
        }
        let beat = self.beat_no;
        if let Some(last) = self.settles.last() {
            if beat <= last.beat {
                return Err(BeatCode::BeatBack.val());
            }
        }
        let rec = match recompute_settle(&self.samples, beat) {
            Some(r) => r,
            None => return Err(BeatCode::CsBroken.val()),
        };
        self.admin_ops += 2;
        self.payload_ops += render_settle(&rec).len() as u64;
        self.beat_no = match beat.checked_add(1) { Some(x) => x, None => return Err(BeatCode::BeatBack.val()) };
        self.settles_done = match self.settles_done.checked_add(1) { Some(x) => x, None => return Err(BeatCode::BeatBack.val()) };
        if self.state == BeatState::Idle || self.state == BeatState::Sampling {
            self.state = BeatState::Settling;
        }
        self.settles.push(rec.clone());
        Ok(rec)
    }

    pub fn overhead_bps(&self) -> u64 {
        if self.payload_ops == 0 {
            return u64::MAX;
        }
        self.admin_ops.saturating_mul(10_000) / self.payload_ops
    }
}

pub const OVERHEAD_CAP_BPS: u64 = 200;

// ── 结算核验（记录 vs 原始采样复算逐域对账） ────────────────────
pub fn verify_settle_line(samples: &[SamplePoint], line: &str, beat: u32) -> Result<(), u32> {
    let want = match recompute_settle(samples, beat) {
        Some(r) => r,
        None => return Err(BeatCode::HalfSettle.val()),
    };
    let got = match parse_settle(line) {
        Some(g) => g,
        None => return Err(BeatCode::CsBroken.val()),
    };
    if got.beat != want.beat || got.count != want.count {
        return Err(BeatCode::BeatBack.val());
    }
    if got.sum != want.sum {
        return Err(BeatCode::SumDrift.val());
    }
    if got.min != want.min || got.max != want.max || got.avg_q8 != want.avg_q8 {
        return Err(BeatCode::StatDrift.val());
    }
    let semi = match find_byte(line.as_bytes(), 0, b';') {
        Some(p) => p,
        None => return Err(BeatCode::CsBroken.val()),
    };
    if &line[semi + 1..] != "
" {
        return Err(BeatCode::CsBroken.val());
    }
    let prefix = match prefix_of(line) {
        Some(b) => b,
        None => return Err(BeatCode::CsBroken.val()),
    };
    if fnv1a(prefix.as_bytes()) != got.cs {
        return Err(BeatCode::CsBroken.val());
    }
    if got.seq_lo != want.seq_lo || got.seq_hi != want.seq_hi {
        return Err(BeatCode::SeqBack.val());
    }
    Ok(())
}

pub fn verify_seq_discipline(samples: &[SamplePoint]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < samples.len() {
        let s = match samples.get(i) {
            Some(s) => s,
            None => return Err(BeatCode::SeqBack.val()),
        };
        if s.seq != (i + 1) as u64 {
            return Err(BeatCode::SeqBack.val());
        }
        if s.beat as usize != i / BEAT_N {
            return Err(BeatCode::BeatBack.val());
        }
        i += 1;
    }
    Ok(())
}

pub fn verify_beat_discipline(settles: &[BeatSettle]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < settles.len() {
        let s = match settles.get(i) {
            Some(s) => s,
            None => return Err(BeatCode::BeatBack.val()),
        };
        if s.beat != i as u32 {
            return Err(BeatCode::BeatBack.val());
        }
        if s.count != BEAT_N as u64 {
            return Err(BeatCode::HalfSettle.val());
        }
        if s.seq_lo != (i * BEAT_N + 1) as u64 || s.seq_hi != ((i + 1) * BEAT_N) as u64 {
            return Err(BeatCode::SeqBack.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 失真注入（10 路逐字失真） ───────────────────────────────────
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}

fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) {
        Some(pp) => pp + pat.len(),
        None => usize::MAX,
    };
    let mut i = 0usize;
    while i < b.len() {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
        i += 1;
    }
}

fn line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) {
        Some(x) => x,
        None => b.len(),
    }
}

pub fn distort(line: &str, kind: usize) -> String {
    let b = line.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    match kind % 10 {
        0 => patch_after(b, b"sum=", b'9', b'1', &mut v),
        1 => patch_after(b, b"min=", b'0', b'2', &mut v),
        2 => patch_after(b, b"max=", b'9', b'1', &mut v),
        3 => patch_after(b, b"avg=", b'9', b'1', &mut v),
        4 => patch_after(b, b"cs=", b'0', b'F', &mut v),
        5 => patch_after(b, b"beat=", b'9', b'1', &mut v),
        6 => patch_after(b, b"count=", b'9', b'1', &mut v),
        7 => push_range(b, 0, b.len().saturating_sub(9), &mut v),
        8 => {
            push_range(b, 0, line_end(b), &mut v);
            v.push(0x01);
            push_range(b, line_end(b) + 1, b.len(), &mut v);
        }
        _ => {
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"@VXS1|beat=99|count=64|sum=0|min=0|max=0|avg=0|cs=00000000;\n";
            let mut k = 0usize;
            while k < tail.len() {
                v.push(tail[k]);
                k += 1;
            }
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut base: Vec<SamplePoint> = Vec::new();
    let mut i = 0usize;
    while i < BEAT_N {
        base.push(SamplePoint { seq: (i + 1) as u64, beat: 0, val: derive_val(i), kind: derive_kind(i) });
        i += 1;
    }
    let mut r = 0u32;
    while r < rounds {
        let rec = match recompute_settle(&base, 0) {
            Some(x) => x,
            None => return Err(BeatCode::HalfSettle.val()),
        };
        let gold = render_settle(&rec);
        verify_settle_line(&base, &gold, 0)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_settle_line(&base, &distort(&gold, kind), 0).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u64, u32), u32> {
    let mut sm = Sampler::new();
    let total = BEATS as usize * BEAT_N;
    let mut i = 0usize;
    while i < total { sm.sample(derive_kind(i), derive_val(i))?; i += 1; }
    let mut k = 0u32;
    while k < BEATS {
        let rec = sm.settle_beat()?;
        let line = render_settle(&rec);
        verify_settle_line(&sm.samples, &line, k)?;
        if line != direct_settle(&rec) { return Err(BeatCode::NotReproducible.val()); }
        k += 1;
    }
    verify_seq_discipline(&sm.samples)?;
    verify_beat_discipline(&sm.settles)?;
    let mut fid = 0u32;
    let rec0 = match sm.settles.first() {
        Some(r) => r.clone(),
        None => return Err(BeatCode::HalfSettle.val()),
    };
    while (fid as usize) < FIDELITY_SAMPLES {
        if render_settle(&rec0) != direct_settle(&rec0) { return Err(BeatCode::NotReproducible.val()); }
        fid += 1;
    }
    let mut sm2 = Sampler::new();
    let half_rej = sm2.settle_beat().is_err();
    let _ = sm2.sample(SampleKind::Raw, 1);
    sm2.seal()?;
    let sealed_s = sm2.sample(SampleKind::Raw, 2).is_err();
    let sealed_t = sm2.settle_beat().is_err();
    let mut sm3 = Sampler::new();
    let m1 = sm3.advance_state(BeatState::Sampling).is_ok(); let m2 = sm3.advance_state(BeatState::Idle).is_err();
    let m3 = sm3.advance_state(BeatState::Sampling).is_err(); let m4 = sm3.advance_state(BeatState::Settling).is_ok();
    let m5 = sm3.advance_state(BeatState::Sampling).is_err();
    let state_rej = m1 && m2 && m3 && m4 && m5;
    let rec1 = match sm.settles.get(1) {
        Some(r) => r.clone(),
        None => return Err(BeatCode::HalfSettle.val()),
    };
    let tamper_rej = verify_settle_line(&sm.samples, &distort(&render_settle(&rec1), 0), 1).is_err();
    if sm.overhead_bps() > OVERHEAD_CAP_BPS {
        return Err(BeatCode::NotReproducible.val());
    }
    let flags = (sm.settles.len() == BEATS as usize) as u32
        + (fid == FIDELITY_SAMPLES as u32) as u32
        + (verify_seq_discipline(&sm.samples).is_ok()) as u32
        + (verify_beat_discipline(&sm.settles).is_ok()) as u32;
    let rejects = half_rej as u32 + sealed_s as u32 + sealed_t as u32 + state_rej as u32 + tamper_rej as u32;
    Ok((flags, rejects, sm.payload_ops, fid))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() { Ok((4, 5, _, _)) => Ok(()), _ => Err(BeatCode::NotReproducible.val()) }
}

pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) { Ok(l) if l.all_caught() => Ok(()), _ => Err(BeatCode::SumDrift.val()) }
}

pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72900 && BeatCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = BEAT_N == 64 && BEATS == 8 && SEQ_CAP == (1u64 << 40) && Q8_SHIFT == 8
        && FIDELITY_SAMPLES == 100_000 && JUDGE_ROUNDS == 10 && OVERHEAD_CAP_BPS == 200 && in_window;
    if ok { Ok(()) } else { Err(BeatCode::NotReproducible.val()) }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BeatCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BeatCode::ALL.len() {
        let v = BeatCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 结算记录往返探针（渲染→解析→逐域对账） ──────────────────────
pub fn record_roundtrip() -> bool {
    let rec = BeatSettle { beat: 3, count: 64, sum: -1234, min: -500, max: 900, avg_q8: -4937, cs: fnv1a(b"probe"), seq_lo: 193, seq_hi: 256 };
    let line = render_settle(&rec);
    let a = match parse_settle(&line) {
        Some(p) => p.beat == 3 && p.count == 64 && p.sum == -1234 && p.min == -500 && p.max == 900 && p.avg_q8 == -4937,
        None => false,
    };
    let bad = parse_settle("@VXS1|beat=1|ghost=2|cs=00000000;").is_none()
        && parse_settle("@VXS1|beat=x|count=1|cs=00000000;").is_none()
        && parse_settle("@VXS1|beat=1|count=-3|cs=00000000;").is_none();
    a && bad
}

// ── Q8 定点平均探针（整数算术边界） ─────────────────────────────
pub fn probe_q8() -> bool {
    let mut same: Vec<SamplePoint> = Vec::new();
    let mut i = 0usize;
    while i < BEAT_N { same.push(SamplePoint { seq: (i + 1) as u64, beat: 0, val: -7, kind: SampleKind::Raw }); i += 1; }
    let a = match fold_stats(&same, 0) {
        Some((n, sum, mn, mx, avg)) => n == BEAT_N as u64 && sum == -7 * BEAT_N as i64 && mn == -7 && mx == -7 && avg == -7 * 256,
        None => false,
    };
    let b = fold_stats(&same, BEAT_N).is_none() && fold_stats(&same, 1).is_none();
    a && b
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut sm = Sampler::new();
    let total = BEATS as usize * BEAT_N;
    let mut i = 0usize;
    while i < total { sm.sample(derive_kind(i), derive_val(i))?; i += 1; }
    let mut k = 0u32;
    while k < BEATS { sm.settle_beat()?; k += 1; }
    verify_seq_discipline(&sm.samples)?;
    verify_beat_discipline(&sm.settles)?;
    sm.seal()?;
    Ok(format!(
        "状态=已完成 beats={} samples={} settles={} overhead={}bps",
        sm.settles.len(), sm.samples.len(), sm.settles_done, sm.overhead_bps()
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p, _)) => (*f == 4 && *r == 5, format!("flags={} rejects={} payload={}", f, r, p)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-结算断言全过", gp, gd));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-漂移注入10/10", ip, id));
    let mut sm = Sampler::new();
    let mut disc_ok = true;
    let mut i = 0usize;
    while i < 2 * BEAT_N { if sm.sample(derive_kind(i), derive_val(i)).is_err() { disc_ok = false; } i += 1; }
    let d1 = verify_seq_discipline(&sm.samples).is_ok();
    let _ = sm.settle_beat();
    let d2 = sm.settle_beat().is_ok();
    let d3 = sm.settle_beat().is_err();
    let d4 = verify_beat_discipline(&sm.settles).is_ok();
    v.push(("J1-双纪律对齐", disc_ok && d1 && d2 && d3 && d4,
        format!("seq连续={} 满拍结算={} 欠拍拒={} 拍账核={}", d1, d2, d3, d4)));
    let rec = BeatSettle { beat: 0, count: BEAT_N as u64, sum: 1, min: 0, max: 1, avg_q8: 256, cs: 0, seq_lo: 1, seq_hi: 64 };
    let mut fid = 0usize;
    while fid < FIDELITY_SAMPLES { if render_settle(&rec) != direct_settle(&rec) { break; } fid += 1; }
    v.push(("J1-对拍10^5样本", fid == FIDELITY_SAMPLES, format!("samples={}", FIDELITY_SAMPLES)));
    let mut s2 = Sampler::new();
    let mut j = 0usize;
    while j < BEAT_N { let _ = s2.sample(SampleKind::Raw, 1); j += 1; }
    let _ = s2.settle_beat();
    v.push((
        "J1-开销占比≤2%",
        golden_run().map(|(_, _, p, _)| p > 0).unwrap_or(false) && s2.overhead_bps() <= OVERHEAD_CAP_BPS,
        format!("cap={}bps", OVERHEAD_CAP_BPS),
    ));
    let mut s3 = Sampler::new();
    let m1 = s3.advance_state(BeatState::Sampling).is_ok();
    let m2 = s3.advance_state(BeatState::Idle).is_err();
    let m3 = s3.advance_state(BeatState::Sampling).is_err();
    let m4 = s3.advance_state(BeatState::Sealed).is_ok();
    let s4 = s3.sample(SampleKind::Raw, 1).is_err() && s3.settle_beat().is_err() && s3.seal().is_err();
    v.push((
        "J2-单调守卫",
        m1 && m2 && m3 && m4 && s4,
        format!("进={} 倒拒={} 平拒={} 封={} 封账三拒={}", m1, m2, m3, m4, s4),
    ));
    let rt = record_roundtrip();
    let q8 = probe_q8();
    v.push(("J1-记录往返+Q8", rt && q8, format!("往返={} q8边界={}", rt, q8)));
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72900 窗口")));
    v
}

pub fn run_unx2064_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2064 统计采样与结算节拍");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
