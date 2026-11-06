//! UNX-F50111 直通 fuzz 面（坏 PDF/坏 PS 注入不崩）（0x43a00 段）
//!
//! 锚点：UNX-F50111-J1 fuzz ×10^4 零崩；判据锚定 §AI-63 §5。
//!
//! 直通判定器吃的是**不可信字节**，因此它自己在设计上就不允许 panic：
//! 全部读取走 `get()`，深度/长度先于结构解释，扫描器对任何输入都必须返回
//! 「接受」或「带具体码的拒绝」。fuzz 面用确定性 PRNG 在 7 类变异里跑 10^4 次，
//! 只要有一次 panic 整条判据即红——「不崩」不是靠运气，是靠无 panic 写法。
//!
//! 数据段：`SEG_BASE = 0x43a00`；诊断码闭集 = `SEG_BASE + 1..=9`。
//!
//! 结构清单：
//! * `Rng64` —— xorshift64* 确定性伪随机（同种子逐位可复算）；
//! * `MutationKind` + `mutate` —— 7 类变异算子（位翻转/截断/拼接/坏头/长度/深度/尾巴）；
//! * `scan` —— 无 panic 的硬化扫描器（PDF/PS 双格式 + 结构闸）；
//! * `FuzzLedger` / `FuzzReport` —— 结果分桶、码值分布与 FNV 摘要；
//! * `injection_core` —— 9 类坏入件逐类断码。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（本模块独占 0x43a00）。
pub const SEG_BASE: u32 = 0x43a00;

pub const E_EMPTY: u16 = 1;
pub const E_NOHDR: u16 = 2;
pub const E_EOF: u16 = 3;
pub const E_XREF: u16 = 4;
pub const E_DEPTH: u16 = 5;
pub const E_LENGTH: u16 = 6;
pub const E_BINARY: u16 = 7;
pub const E_OVERSIZE: u16 = 8;
pub const E_TRAILING: u16 = 9;

/// 单件扫描上限（256 KiB）。变异器与扫描器共用同一上限，
/// 保证「造出来的样本」和「能扫的样本」是同一个域。
pub const MAX_INPUT: usize = 256 * 1024;

/// `<<` 嵌套上限：超过即判深度炸弹（递归下降解析器的栈保护）。
pub const MAX_DEPTH: u16 = 32;

/// fuzz 面错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtFuzzError {
    /// 空入件。
    Empty,
    /// 头不在 `%PDF-` / `%!PS` 白名单。
    NoHeader,
    /// 缺 `%%EOF`。
    Eof,
    /// PDF 缺 `startxref`。
    Xref,
    /// 字典嵌套超上限。
    Depth,
    /// `/Length` 声称的长度超过实际字节。
    Length,
    /// 前 512 字节里不可打印字节占比过高（二进制垃圾）。
    Binary,
    /// 超过单件上限。
    Oversize,
    /// `%%EOF` 之后仍有非空白字节。
    Trailing,
}

impl PtFuzzError {
    pub fn n(self) -> u16 {
        match self {
            PtFuzzError::Empty => E_EMPTY,
            PtFuzzError::NoHeader => E_NOHDR,
            PtFuzzError::Eof => E_EOF,
            PtFuzzError::Xref => E_XREF,
            PtFuzzError::Depth => E_DEPTH,
            PtFuzzError::Length => E_LENGTH,
            PtFuzzError::Binary => E_BINARY,
            PtFuzzError::Oversize => E_OVERSIZE,
            PtFuzzError::Trailing => E_TRAILING,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.n() as u32
    }

    pub fn index(self) -> usize {
        (self.n() - 1) as usize
    }

    pub fn from_code(code: u32) -> Option<PtFuzzError> {
        if code <= SEG_BASE {
            return None;
        }
        match (code - SEG_BASE) as u16 {
            E_EMPTY => Some(PtFuzzError::Empty),
            E_NOHDR => Some(PtFuzzError::NoHeader),
            E_EOF => Some(PtFuzzError::Eof),
            E_XREF => Some(PtFuzzError::Xref),
            E_DEPTH => Some(PtFuzzError::Depth),
            E_LENGTH => Some(PtFuzzError::Length),
            E_BINARY => Some(PtFuzzError::Binary),
            E_OVERSIZE => Some(PtFuzzError::Oversize),
            E_TRAILING => Some(PtFuzzError::Trailing),
            _ => None,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            PtFuzzError::Empty => "empty input",
            PtFuzzError::NoHeader => "unknown header",
            PtFuzzError::Eof => "missing %%EOF",
            PtFuzzError::Xref => "missing startxref",
            PtFuzzError::Depth => "nesting depth exceeded",
            PtFuzzError::Length => "declared length exceeds input",
            PtFuzzError::Binary => "binary garbage",
            PtFuzzError::Oversize => "input exceeds scan limit",
            PtFuzzError::Trailing => "non-space bytes after %%EOF",
        }
    }
}

/// 扫描通过后的轮廓。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanOk {
    pub pdf: bool,
    pub bytes: usize,
    pub max_depth: u16,
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t' | b'\x0c' | b'\x00')
}

fn starts_with(hay: &[u8], needle: &[u8]) -> bool {
    if needle.len() > hay.len() {
        return false;
    }
    let mut i = 0usize;
    while i < needle.len() {
        match (hay.get(i), needle.get(i)) {
            (Some(a), Some(b)) if a == b => {}
            _ => return false,
        }
        i += 1;
    }
    true
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    let mut i = 0usize;
    while i <= last {
        let mut k = 0usize;
        let mut hit = true;
        while k < needle.len() {
            match (hay.get(i + k), needle.get(k)) {
                (Some(a), Some(b)) if a == b => {}
                _ => {
                    hit = false;
                    break;
                }
            }
            k += 1;
        }
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 最后一个 `%%EOF` 之后的结束位置（含 `%%EOF` 本身）。
fn last_eof_end(doc: &[u8]) -> Option<usize> {
    let pat = b"%%EOF";
    let mut last: Option<usize> = None;
    let mut i = 0usize;
    while i.saturating_add(pat.len()) <= doc.len() {
        let mut k = 0usize;
        let mut hit = true;
        while k < pat.len() {
            match (doc.get(i + k), pat.get(k)) {
                (Some(a), Some(b)) if a == b => {}
                _ => {
                    hit = false;
                    break;
                }
            }
            k += 1;
        }
        if hit {
            last = Some(i + pat.len());
        }
        i += 1;
    }
    last
}

fn non_printable_ratio(doc: &[u8]) -> usize {
    let probe = if doc.len() > 512 { 512 } else { doc.len() };
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < probe {
        match doc.get(i) {
            Some(&c) => {
                let printable = c == b'\t' || c == b'\n' || c == b'\r' || (c >= 0x20 && c <= 0x7e);
                if !printable {
                    bad += 1;
                }
            }
            None => break,
        }
        i += 1;
    }
    if probe == 0 {
        0
    } else {
        bad * 100 / probe
    }
}

fn scan_depth(doc: &[u8]) -> Result<u16, PtFuzzError> {
    let mut depth = 0u16;
    let mut max = 0u16;
    let mut i = 0usize;
    while i < doc.len() {
        let c = match doc.get(i) {
            Some(&v) => v,
            None => break,
        };
        if c == b'<' {
            if let Some(&b'<') = doc.get(i + 1) {
                depth = depth.saturating_add(1);
                if depth > MAX_DEPTH {
                    return Err(PtFuzzError::Depth);
                }
                if depth > max {
                    max = depth;
                }
                i += 2;
                continue;
            }
        } else if c == b'>' {
            if let Some(&b'>') = doc.get(i + 1) {
                depth = depth.saturating_sub(1);
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    Ok(max)
}

fn declared_length(doc: &[u8]) -> Option<usize> {
    let at = find(doc, b"/Length")?;
    let mut i = at + 7;
    while i < doc.len() {
        match doc.get(i) {
            Some(&c) if is_space(c) => i += 1,
            _ => break,
        }
    }
    let mut v = 0usize;
    let mut digits = 0usize;
    while i < doc.len() {
        let c = match doc.get(i) {
            Some(&v) => v,
            None => break,
        };
        if c < b'0' || c > b'9' {
            break;
        }
        v = v.saturating_mul(10).saturating_add((c - b'0') as usize);
        digits += 1;
        i += 1;
    }
    if digits == 0 {
        None
    } else {
        Some(v)
    }
}

/// 硬化扫描器：对任意字节串返回接受或**带具体码**的拒绝，绝不 panic。
pub fn scan(doc: &[u8]) -> Result<ScanOk, PtFuzzError> {
    if doc.is_empty() {
        return Err(PtFuzzError::Empty);
    }
    if doc.len() > MAX_INPUT {
        return Err(PtFuzzError::Oversize);
    }
    let pdf = if starts_with(doc, b"%PDF-") {
        true
    } else if starts_with(doc, b"%!PS") {
        false
    } else {
        return Err(PtFuzzError::NoHeader);
    };
    if non_printable_ratio(doc) > 20 {
        return Err(PtFuzzError::Binary);
    }
    if pdf && find(doc, b"startxref").is_none() {
        return Err(PtFuzzError::Xref);
    }
    let eof_end = match last_eof_end(doc) {
        Some(p) => p,
        None => return Err(PtFuzzError::Eof),
    };
    let max_depth = scan_depth(doc)?;
    if let Some(len) = declared_length(doc) {
        if len > doc.len() {
            return Err(PtFuzzError::Length);
        }
    }
    let mut i = eof_end;
    while i < doc.len() {
        match doc.get(i) {
            Some(&c) if is_space(c) || c == b'\x00' => i += 1,
            Some(_) => return Err(PtFuzzError::Trailing),
            None => break,
        }
    }
    Ok(ScanOk {
        pdf,
        bytes: doc.len(),
        max_depth,
    })
}

// ---------------------------------------------------------------------------
// 确定性 PRNG 与变异算子
// ---------------------------------------------------------------------------

/// xorshift64*：同种子逐位可复算，不依赖任何外部状态。
pub struct Rng64 {
    state: u64,
}

impl Rng64 {
    pub fn new(seed: u64) -> Rng64 {
        Rng64 {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 取 `[0, n)` 的下标（n == 0 时返回 0，不 panic）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    pub fn byte(&mut self) -> u8 {
        (self.next_u64() & 0xFF) as u8
    }
}

/// 7 类变异算子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationKind {
    Flip,
    Truncate,
    Splice,
    BadHeader,
    LengthBomb,
    DepthBomb,
    TailJunk,
}

impl MutationKind {
    pub fn code(self) -> u8 {
        match self {
            MutationKind::Flip => 0,
            MutationKind::Truncate => 1,
            MutationKind::Splice => 2,
            MutationKind::BadHeader => 3,
            MutationKind::LengthBomb => 4,
            MutationKind::DepthBomb => 5,
            MutationKind::TailJunk => 6,
        }
    }

    pub fn index(self) -> usize {
        self.code() as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            MutationKind::Flip => "flip",
            MutationKind::Truncate => "truncate",
            MutationKind::Splice => "splice",
            MutationKind::BadHeader => "bad-header",
            MutationKind::LengthBomb => "length-bomb",
            MutationKind::DepthBomb => "depth-bomb",
            MutationKind::TailJunk => "tail-junk",
        }
    }

    pub fn from_index(i: usize) -> MutationKind {
        match i % 7 {
            0 => MutationKind::Flip,
            1 => MutationKind::Truncate,
            2 => MutationKind::Splice,
            3 => MutationKind::BadHeader,
            4 => MutationKind::LengthBomb,
            5 => MutationKind::DepthBomb,
            _ => MutationKind::TailJunk,
        }
    }
}

fn append_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    let mut i = 0usize;
    while i < bytes.len() {
        if out.len() >= MAX_INPUT {
            return;
        }
        if let Some(&b) = bytes.get(i) {
            out.push(b);
        }
        i += 1;
    }
}

/// 施加一次变异。返回值长度恒 ≤ `MAX_INPUT`（超限即截至上限）。
pub fn mutate(seed: &[u8], kind: MutationKind, rng: &mut Rng64) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(seed.len().min(MAX_INPUT) + 16);
    append_bytes(&mut out, seed);
    match kind {
        MutationKind::Flip => {
            if !out.is_empty() {
                let pos = rng.below(out.len());
                let v = rng.byte();
                if let Some(slot) = out.get_mut(pos) {
                    *slot = v;
                }
            }
        }
        MutationKind::Truncate => {
            if !out.is_empty() {
                let cut = rng.below(out.len());
                out.truncate(cut);
            }
        }
        MutationKind::Splice => {
            if !seed.is_empty() {
                let at = rng.below(seed.len());
                let take = 1 + rng.below(8);
                let mut chunk: Vec<u8> = Vec::with_capacity(take);
                let mut k = 0usize;
                while k < take {
                    let idx = (at + k) % seed.len();
                    if let Some(&b) = seed.get(idx) {
                        chunk.push(b);
                    }
                    k += 1;
                }
                append_bytes(&mut out, &chunk);
            }
        }
        MutationKind::BadHeader => {
            let junk = [0x00u8, 0xFF, b'X', b'%', b'!', b'\n', 0x7F, b'Z'];
            let mut i = 0usize;
            while i < junk.len() {
                if let Some(slot) = out.get_mut(i) {
                    if let Some(&b) = junk.get(i) {
                        *slot = b;
                    }
                }
                i += 1;
            }
        }
        MutationKind::LengthBomb => {
            append_bytes(&mut out, b"\n/Length 999999\n");
        }
        MutationKind::DepthBomb => {
            let mut i = 0usize;
            while i < 40 {
                append_bytes(&mut out, b"<<");
                i += 1;
            }
        }
        MutationKind::TailJunk => {
            let n = 1 + rng.below(16);
            let mut i = 0usize;
            while i < n {
                let b = rng.byte();
                append_bytes(&mut out, &[b]);
                i += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// fuzz 账本
// ---------------------------------------------------------------------------

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 结果报告（可比较，用于确定性判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    pub iterations: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub digest: u64,
    pub kinds: [u32; 7],
    pub codes: [u32; 9],
}

/// 分桶账本：接受/拒绝 + 每码 + 每变异算子。
pub struct FuzzLedger {
    iterations: u32,
    accepted: u32,
    rejected: u32,
    codes: [u32; 9],
    kinds: [u32; 7],
    digest: u64,
}

impl FuzzLedger {
    pub fn new() -> FuzzLedger {
        FuzzLedger {
            iterations: 0,
            accepted: 0,
            rejected: 0,
            codes: [0u32; 9],
            kinds: [0u32; 7],
            digest: FNV_OFFSET,
        }
    }

    fn mix(&mut self, tag: u8, len: usize) {
        let mut h = self.digest;
        h ^= tag as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= (len as u64) & 0xFFFF_FFFF;
        h = h.wrapping_mul(FNV_PRIME);
        self.digest = h;
    }

    /// 记一次 fuzz 结果（含变异算子归属，便于证明 7 类都真的跑过）。
    pub fn record(&mut self, kind: MutationKind, len: usize, res: &Result<ScanOk, PtFuzzError>) {
        self.iterations = self.iterations.saturating_add(1);
        match res {
            Ok(_) => {
                self.accepted = self.accepted.saturating_add(1);
                self.mix(0, len);
            }
            Err(e) => {
                self.rejected = self.rejected.saturating_add(1);
                self.mix(e.n() as u8, len);
                if let Some(c) = self.codes.get_mut(e.index()) {
                    *c = c.saturating_add(1);
                }
            }
        }
        if let Some(k) = self.kinds.get_mut(kind.index()) {
            *k = k.saturating_add(1);
        }
    }

    pub fn report(&self) -> FuzzReport {
        FuzzReport {
            iterations: self.iterations,
            accepted: self.accepted,
            rejected: self.rejected,
            digest: self.digest,
            kinds: self.kinds,
            codes: self.codes,
        }
    }

    /// 码值分布求和（必须等于 rejected）。
    pub fn code_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.codes.len() {
            if let Some(&c) = self.codes.get(i) {
                sum = sum.saturating_add(c);
            }
            i += 1;
        }
        sum
    }

    /// 变异算子分布求和（必须等于 iterations）。
    pub fn kind_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.kinds.len() {
            if let Some(&c) = self.kinds.get(i) {
                sum = sum.saturating_add(c);
            }
            i += 1;
        }
        sum
    }
}

const SEED_PDF: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\nstartxref\n0\n%%EOF\n";
const SEED_PDF2: &[u8] = b"%PDF-1.4\n<< /Length 5 >>\nstream\nabcde\nendstream\nstartxref\n9\n%%EOF\n";
const SEED_PS: &[u8] = b"%!PS-Adobe-3.0\nshowpage\n%%EOF\n";

/// 种子语料（三件，覆盖 PDF 结构树与 PS 平铺两种形态）。
pub fn seed_corpus() -> Vec<&'static [u8]> {
    let mut v: Vec<&'static [u8]> = Vec::new();
    v.push(SEED_PDF);
    v.push(SEED_PDF2);
    v.push(SEED_PS);
    v
}

/// 跑 `iters` 轮 fuzz：算子按轮次取模轮转（保证 7 类都出现），
/// 随机性只用于「选种子 + 变异位置」，因此同种子结果逐位一致。
pub fn fuzz_run(seed: u64, iters: u32) -> FuzzReport {
    let corpus = seed_corpus();
    let mut rng = Rng64::new(seed);
    let mut ledger = FuzzLedger::new();
    let mut i = 0u32;
    while i < iters {
        let kind = MutationKind::from_index(i as usize);
        let base = match corpus.get(rng.below(corpus.len())) {
            Some(&d) => d,
            None => SEED_PDF,
        };
        let doc = mutate(base, kind, &mut rng);
        let res = scan(&doc);
        ledger.record(kind, doc.len(), &res);
        i += 1;
    }
    ledger.report()
}

// ---------------------------------------------------------------------------
// 注入核：9 类坏入件
// ---------------------------------------------------------------------------

fn deep_bomb() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    append_bytes(&mut v, b"%PDF-1.7\n");
    let mut i = 0usize;
    while i < 40 {
        append_bytes(&mut v, b"<<");
        i += 1;
    }
    append_bytes(&mut v, b"\nstartxref\n0\n%%EOF\n");
    v
}

fn binary_bomb() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    append_bytes(&mut v, b"%PDF-1.7\n");
    let mut i = 0usize;
    while i < 96 {
        append_bytes(&mut v, &[0x00]);
        i += 1;
    }
    append_bytes(&mut v, b"startxref\n0\n%%EOF\n");
    v
}

fn oversize_bomb() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(MAX_INPUT + 1);
    append_bytes(&mut v, b"%PDF-1.7\n");
    v.resize(MAX_INPUT + 1, b' ');
    v
}

/// 返回 `(命中数, 总类数)`；每类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let total = 9usize;
    let mut caught = 0usize;

    if scan(&[]).err() == Some(PtFuzzError::Empty) {
        caught += 1;
    }
    if scan(b"garbage-not-a-doc").err() == Some(PtFuzzError::NoHeader) {
        caught += 1;
    }
    if scan(b"%PDF-1.7\nstartxref\n0\n").err() == Some(PtFuzzError::Eof) {
        caught += 1;
    }
    if scan(b"%PDF-1.7\n%%EOF\n").err() == Some(PtFuzzError::Xref) {
        caught += 1;
    }
    let deep = deep_bomb();
    if scan(&deep).err() == Some(PtFuzzError::Depth) {
        caught += 1;
    }
    if scan(b"%PDF-1.7\n/Length 999999\nstartxref\n0\n%%EOF\n").err()
        == Some(PtFuzzError::Length)
    {
        caught += 1;
    }
    let bin = binary_bomb();
    if scan(&bin).err() == Some(PtFuzzError::Binary) {
        caught += 1;
    }
    let big = oversize_bomb();
    if scan(&big).err() == Some(PtFuzzError::Oversize) {
        caught += 1;
    }
    if scan(b"%PDF-1.7\nstartxref\n0\n%%EOF\nZZZ").err() == Some(PtFuzzError::Trailing) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50111_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50111");

    // J1-1 fuzz ×10^4 零崩：跑完即证据（panic 会让本判据所在的整轮测试直接失败）
    let rep = fuzz_run(2026, 10_000);
    let ok = rep.iterations == 10_000 && rep.accepted.saturating_add(rep.rejected) == 10_000;
    let det = format!("it={} acc={} rej={}", rep.iterations, rep.accepted, rep.rejected);
    s.add_dyn("50111-J1-万轮零崩", ok, &det);

    // J1-2 分桶守恒：每码计数和 = 拒绝数；每算子计数和 = 轮数
    let mut code_sum = 0u32;
    let mut i = 0usize;
    while i < rep.codes.len() {
        if let Some(&c) = rep.codes.get(i) {
            code_sum = code_sum.saturating_add(c);
        }
        i += 1;
    }
    let mut kind_sum = 0u32;
    let mut i = 0usize;
    while i < rep.kinds.len() {
        if let Some(&c) = rep.kinds.get(i) {
            kind_sum = kind_sum.saturating_add(c);
        }
        i += 1;
    }
    let ok = code_sum == rep.rejected && kind_sum == rep.iterations;
    let det = format!("codes={} kinds={}", code_sum, kind_sum);
    s.add_dyn("50111-J1-分桶守恒", ok, &det);

    // J1-3 7 类变异全部真实跑过（没有一类是摆设）
    let mut ok = true;
    let mut i = 0usize;
    while i < rep.kinds.len() {
        match rep.kinds.get(i) {
            Some(&c) if c > 0 => {}
            _ => ok = false,
        }
        i += 1;
    }
    let det = format!("{:?}", rep.kinds);
    s.add_dyn("50111-J1-算子覆盖", ok, &det);

    // J1-4 确定性：同种子两次跑，摘要与分桶逐位一致
    let a = fuzz_run(7, 2_000);
    let b = fuzz_run(7, 2_000);
    let ok = a == b && a.digest != 0;
    s.add("50111-J1-同种子复算", ok, if ok { "" } else { "两次跑不一致" });

    // J1-5 不同种子产生不同流（PRNG 真的在变，而不是常量）
    let c = fuzz_run(8, 2_000);
    let ok = c.digest != a.digest;
    s.add("50111-J1-种子分离", ok, if ok { "" } else { "种子不起作用" });

    // J1-6 种子语料全部可收，且格式判定正确、深度合规
    let pdf = scan(SEED_PDF);
    let pdf2 = scan(SEED_PDF2);
    let ps = scan(SEED_PS);
    let ok = match (pdf, pdf2, ps) {
        (Ok(a), Ok(b), Ok(c)) => {
            a.pdf && b.pdf && !c.pdf && a.max_depth == 1 && b.max_depth == 1 && c.max_depth == 0
        }
        _ => false,
    };
    s.add("50111-J1-种子可收", ok, if ok { "" } else { "种子被判坏" });

    // J1-7 空/超限：两个边界优先于一切结构解释
    let big = oversize_bomb();
    let ok = scan(&[]).err() == Some(PtFuzzError::Empty)
        && scan(&big).err() == Some(PtFuzzError::Oversize);
    let det = format!("big={}", big.len());
    s.add_dyn("50111-J1-边界先判", ok, &det);

    // J1-8 结构闸：坏头 / 缺 xref / 缺 EOF 各自独立成码
    let nh = scan(b"hello world");
    let xr = scan(b"%PDF-1.7\n%%EOF\n");
    let ef = scan(b"%PDF-1.7\nstartxref\n0\n");
    let ok = nh.err() == Some(PtFuzzError::NoHeader)
        && xr.err() == Some(PtFuzzError::Xref)
        && ef.err() == Some(PtFuzzError::Eof);
    s.add("50111-J1-结构三闸", ok, if ok { "" } else { "结构码混淆" });

    // J1-9 深度炸弹：40 层 `<<` 必须被 MAX_DEPTH=32 拒
    let deep = deep_bomb();
    let ok = scan(&deep).err() == Some(PtFuzzError::Depth) && MAX_DEPTH == 32;
    let det = format!("maxdepth={}", MAX_DEPTH);
    s.add_dyn("50111-J1-深度闸", ok, &det);

    // J1-10 长度炸弹：/Length 大于实际字节 → Length（不是 Eof/Xref）
    let lb = scan(b"%PDF-1.7\n/Length 999999\nstartxref\n0\n%%EOF\n");
    let ok = lb.err() == Some(PtFuzzError::Length);
    s.add("50111-J1-长度闸", ok, if ok { "" } else { "长度谎言未拦" });

    // J1-11 二进制垃圾：头合法但不可打印占比超 20% → Binary
    let bin = binary_bomb();
    let ok = scan(&bin).err() == Some(PtFuzzError::Binary);
    s.add("50111-J1-二进制闸", ok, if ok { "" } else { "二进制未被识别" });

    // J1-12 尾巴垃圾：%%EOF 之后非空白字节 → Trailing；纯空白可收
    let tj = scan(b"%PDF-1.7\nstartxref\n0\n%%EOF\nZZZ");
    let ws = scan(b"%PDF-1.7\nstartxref\n0\n%%EOF\n \n\t\n");
    let ok = tj.err() == Some(PtFuzzError::Trailing) && ws.is_ok();
    s.add("50111-J1-尾巴闸", ok, if ok { "" } else { "尾巴判定错" });

    // J1-13 幂等：同一入件两次扫描结果一致（不允许内部状态污染判定）
    let one = scan(SEED_PDF);
    let two = scan(SEED_PDF);
    let three = scan(SEED_PDF2);
    let four = scan(SEED_PDF2);
    let ok = one == two && three == four && one.is_ok();
    s.add("50111-J1-扫描幂等", ok, if ok { "" } else { "两次不一致" });

    // J1-14 变异器有界：7 类算子输出都 ≤ MAX_INPUT
    let mut rng = Rng64::new(99);
    let mut ok = true;
    let mut i = 0usize;
    while i < 7 {
        let kind = MutationKind::from_index(i);
        let out = mutate(SEED_PDF2, kind, &mut rng);
        if out.len() > MAX_INPUT {
            ok = false;
        }
        i += 1;
    }
    s.add("50111-J1-变异有界", ok, if ok { "" } else { "变异越界" });

    // J1-15 PRNG 确定性：同种子前 4 个数逐位一致；不同种子不同
    let mut r1 = Rng64::new(12345);
    let mut r2 = Rng64::new(12345);
    let mut r3 = Rng64::new(54321);
    let mut ok = true;
    let mut i = 0usize;
    while i < 4 {
        if r1.next_u64() != r2.next_u64() {
            ok = false;
        }
        i += 1;
    }
    ok = ok && r3.next_u64() != r1.next_u64();
    s.add("50111-J1-PRNG确定", ok, if ok { "" } else { "PRNG 不确定" });

    // J1-16 注入核：9 类坏入件全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50111-J1-注入必抓", ok, &det);

    // J1-17 码值零漂移：9 个码互异、连续、可反解
    let all = [
        PtFuzzError::Empty,
        PtFuzzError::NoHeader,
        PtFuzzError::Eof,
        PtFuzzError::Xref,
        PtFuzzError::Depth,
        PtFuzzError::Length,
        PtFuzzError::Binary,
        PtFuzzError::Oversize,
        PtFuzzError::Trailing,
    ];
    let mut ok = true;
    let mut seen = [false; 9];
    let mut i = 0usize;
    while i < all.len() {
        let n = all[i].n() as usize;
        if n < 1 || n > 9 || seen[n - 1] {
            ok = false;
        } else {
            seen[n - 1] = true;
        }
        if PtFuzzError::from_code(all[i].code()) != Some(all[i]) {
            ok = false;
        }
        i += 1;
    }
    ok = ok && PtFuzzError::from_code(SEG_BASE).is_none();
    s.add("50111-J1-码值不漂", ok, if ok { "" } else { "码值漂移" });

    // J1-18 接受件恒带轮廓：bytes 与max_depth 与实际一致
    let r = scan(SEED_PDF2);
    let ok = match r {
        Ok(v) => v.bytes == SEED_PDF2.len() && v.max_depth == 1,
        Err(_) => false,
    };
    let det = format!("bytes={}", SEED_PDF2.len());
    s.add_dyn("50111-J1-轮廓一致", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50111_checks_green() {
        let set = run_unx50111_checks();
        assert!(set.all_passed(), "UNX-F50111 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50111_scan_never_panics_on_mutations() {
        // 直接在测试里再跑一轮短 fuzz：panic 会让测试失败。
        let rep = fuzz_run(4242, 1_000);
        assert_eq!(rep.iterations, 1_000);
        assert_eq!(rep.accepted + rep.rejected, 1_000);
    }

    #[test]
    fn unx50111_mutator_respects_cap() {
        let mut rng = Rng64::new(7);
        let big: Vec<u8> = {
            let mut v = Vec::new();
            v.resize(MAX_INPUT, b'a');
            v
        };
        let out = mutate(&big, MutationKind::TailJunk, &mut rng);
        assert!(out.len() <= MAX_INPUT);
    }
}
