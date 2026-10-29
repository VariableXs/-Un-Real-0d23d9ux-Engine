//! UNX-F0737 · 嫁接账封账（跟随面定版） · 诊断段 0x5ba0
//! 判据原文摘录：UNX-F0737-J1 跟随面五件封账（台账/指纹/流程/拦截/报告哈希 5/5）
//! 且封账后写尝试检出恒零；完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 跟随面五件固定：台账账 / 指纹账 / 流程账 / 拦截账 / 报告账，
//!    五件各有独立哈希，封账要求五件齐 5/5，缺一件即拒绝定版。
//! 2. 封账即定版：封账动作一次性、不可逆，封账哈希 = 五件哈希按固定序聚合，
//!    任何后写尝试必须被账本检出且计数恒零容忍（检出即拦截，非仅告警）。
//! 3. 写尝试检出口径：封账后每一次 write_attempt 调用都必须返回写拒绝诊断码
//!    并落"写尝试账"一行；写尝试账行数与被拒次数守恒。
//! 4. 定容数组账本 + 守恒恒等式：五件封账位图 bits == 0b11111 才可封；
//!    写尝试检出数 = 拒绝数，恒等式在 drill 中复核。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5ba0 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5ba0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_PIECE_DUP: u16 = SEG_BASE + 1; // 同件哈希重复落账
const E_PIECE_BAD: u16 = SEG_BASE + 2; // 件号越界（1..=5）
const E_PIECE_ZERO: u16 = SEG_BASE + 3; // 件哈希为 0（未采集）
const E_NOT_ALL: u16 = SEG_BASE + 4; // 五件未齐即封账被拒
const E_STATE: u16 = SEG_BASE + 5; // 封账状态机非法边
const E_WRITE_CLOSED: u16 = SEG_BASE + 6; // 封账后写被拒（预期拦截码）
const E_WRITE_MISS: u16 = SEG_BASE + 7; // 写尝试未被检出（检出恒零被破坏）
const E_ATTEMPT_FULL: u16 = SEG_BASE + 8; // 写尝试账已满
const E_BAL: u16 = SEG_BASE + 9; // 守恒恒等式破裂（拒绝数≠检出数）
const E_SEAL_TWICE: u16 = SEG_BASE + 10; // 重复封账
const E_HASH_DRIFT: u16 = SEG_BASE + 11; // 件哈希复算漂移（封存不可信）
const E_PIECE_EMPTY: u16 = SEG_BASE + 12; // 件名为空
const E_REOPEN: u16 = SEG_BASE + 13; // 封账账本不可重开
const E_NO_WRITE: u16 = SEG_BASE + 14; // 写尝试账为空（封账后应有检出）

/// 跟随面五件：件号 1..=5。
pub const P_LEDGER: u8 = 1; // 台账账
pub const P_FINGER: u8 = 2; // 指纹账
pub const P_FLOW: u8 = 3; // 流程账
pub const P_INTERCEPT: u8 = 4; // 拦截账
pub const P_REPORT: u8 = 5; // 报告账
pub const PIECES: usize = 5;

/// 封账五件位图满值。
pub const ALL_PIECES: u8 = 0b11111;

/// 写尝试账定容。
pub const MAX_ATTEMPTS: usize = 32;

// ── FNV-1a 64（件哈希与封账哈希口径）────────────────────────────
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
        E_PIECE_DUP => "件哈希重复落账：五件各落一次",
        E_PIECE_BAD => "件号越界：合法 1..=5",
        E_PIECE_ZERO => "件哈希为 0：哈希必须真实采集，禁止零值占位",
        E_NOT_ALL => "五件未齐：台账/指纹/流程/拦截/报告须 5/5",
        E_STATE => "封账状态机非法边：Collecting→AllCollected→Sealed",
        E_WRITE_CLOSED => "封账后写被拒：定版只读，变更走勘误通道（另单）",
        E_WRITE_MISS => "写尝试未被检出：封账后每次写必须落检出账",
        E_ATTEMPT_FULL => "写尝试账已满 32 条：异常高频写，人工排查",
        E_BAL => "守恒破裂：写尝试检出数必须等于拒绝数",
        E_SEAL_TWICE => "重复封账：封账一次性不可逆",
        E_HASH_DRIFT => "件哈希复算漂移：封存件与哈希不互证",
        E_PIECE_EMPTY => "件名为空",
        E_REOPEN => "封账账本不可重开：定版即终版",
        E_NO_WRITE => "写尝试账为空：封账后写检出口径未验证",
        _ => "未知诊断码",
    }
}

// ── 五件状态机 ───────────────────────────────────────────────
/// 封账账本生命周期：合法边 Collecting→AllCollected→Sealed，一步不跳。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealPhase {
    /// 采集中：允许落件哈希。
    Collecting,
    /// 五件齐备：等待封账。
    AllCollected,
    /// 已封账：只读 + 写拦截。
    Sealed,
}

impl SealPhase {
    /// 编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            SealPhase::Collecting => 1,
            SealPhase::AllCollected => 2,
            SealPhase::Sealed => 3,
        }
    }
}

// ── 件哈希行 ─────────────────────────────────────────────────
/// 五件之一的封账行：件号 + 件名 + 哈希 + 复算载荷。
#[derive(Clone, Debug)]
pub struct PieceRow {
    /// 件号（1..=5）。
    pub piece_no: u8,
    /// 件名（台账/指纹/流程/拦截/报告）。
    pub piece_name: String,
    /// 件哈希（FNV-1a 64）。
    pub hash: u64,
    /// 复算载荷（封账后复算哈希的原始字节）。
    pub payload: Vec<u8>,
}

impl PieceRow {
    /// 复算哈希：载荷重哈希必须与落账哈希一致。
    pub fn recompute_ok(&self) -> bool {
        fnv1a64(&self.payload) == self.hash
    }
}

// ── 写尝试行 ─────────────────────────────────────────────────
/// 封账后一次写尝试的检出账行。
#[derive(Clone, Debug)]
pub struct WriteAttempt {
    /// 尝试序号（1 起连续）。
    pub attempt_no: u32,
    /// 尝试写的件号。
    pub piece_no: u8,
    /// 尝试写入的载荷长度。
    pub bytes: u32,
    /// 是否被拒（封账后恒为 true）。
    pub rejected: bool,
}

// ── 件级状态（盘点用）────────────────────────────────────────
/// 单件的盘点状态：封账前后各有含义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceStatus {
    /// 未落账。
    Missing,
    /// 已落账未封账。
    Collected,
    /// 已封账且复算过。
    SealedOk,
    /// 已落账但复算漂移（异常态）。
    Drift,
}

impl PieceStatus {
    /// 编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            PieceStatus::Missing => 0,
            PieceStatus::Collected => 1,
            PieceStatus::SealedOk => 2,
            PieceStatus::Drift => 3,
        }
    }

    /// 中文态名（盘点行渲染用）。
    pub fn zh(self) -> &'static str {
        match self {
            PieceStatus::Missing => "缺件",
            PieceStatus::Collected => "已采",
            PieceStatus::SealedOk => "封存",
            PieceStatus::Drift => "漂移",
        }
    }
}

// ── 嫁接封账账（主体）────────────────────────────────────────
/// 嫁接账封账账本：五件齐 → 封账 → 写拦截 → 检出账守恒。
pub struct GraftSeal {
    pieces: [Option<PieceRow>; PIECES],
    attempts: [Option<WriteAttempt>; MAX_ATTEMPTS],
    /// 五件位图。
    piece_bits: u8,
    /// 件数（= 位图 popcount，双账互证）。
    piece_count: usize,
    /// 写尝试账行数。
    attempt_count: usize,
    /// 被拒次数。
    rejected_count: u32,
    /// 漏检次数（恒应 0）。
    missed_count: u32,
    /// 状态。
    phase: SealPhase,
    /// 封账哈希（0=未封）。
    sealed_hash: u64,
    /// 件级哈希链：chain[i] = fnv(chain[i-1] ‖ piece[i].hash)，chain[-1]=0。
    /// 封账后用于逐环复算，任何一件被篡改都会断链。
    chain: [u64; PIECES],
}

impl GraftSeal {
    /// 开账。
    pub fn new() -> GraftSeal {
        GraftSeal {
            pieces: core::array::from_fn(|_| None),
            attempts: core::array::from_fn(|_| None),
            piece_bits: 0,
            piece_count: 0,
            attempt_count: 0,
            rejected_count: 0,
            missed_count: 0,
            phase: SealPhase::Collecting,
            sealed_hash: 0,
            chain: [0; PIECES],
        }
    }

    /// 落一件哈希：件号合法、名非空、哈希非零、载荷可复算。
    pub fn put_piece(&mut self, piece_no: u8, name: &str, payload: &[u8]) -> Result<u64, u16> {
        if self.phase == SealPhase::Sealed {
            return Err(E_WRITE_CLOSED);
        }
        if self.phase != SealPhase::Collecting && self.phase != SealPhase::AllCollected {
            return Err(E_STATE);
        }
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_PIECE_BAD);
        }
        if name.is_empty() {
            return Err(E_PIECE_EMPTY);
        }
        if payload.is_empty() {
            return Err(E_PIECE_ZERO);
        }
        let idx = (piece_no - 1) as usize;
        if self.pieces[idx].is_some() {
            return Err(E_PIECE_DUP);
        }
        let h = fnv1a64(payload);
        if h == 0 {
            return Err(E_PIECE_ZERO);
        }
        self.pieces[idx] = Some(PieceRow {
            piece_no,
            piece_name: name.to_string(),
            hash: h,
            payload: Vec::from(payload),
        });
        self.piece_bits |= 1u8 << (piece_no - 1);
        self.piece_count += 1;
        if self.piece_bits == ALL_PIECES && self.phase == SealPhase::Collecting {
            self.phase = SealPhase::AllCollected;
        }
        Ok(h)
    }

    /// 五件齐备判定：位图满且件数同为 5（双账互证）。
    pub fn pieces_complete(&self) -> bool {
        self.piece_bits == ALL_PIECES && self.piece_count == PIECES
    }

    /// 封账：五件齐 + 每件复算过 → 聚合哈希定版。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.phase == SealPhase::Sealed {
            return Err(E_SEAL_TWICE);
        }
        if self.phase != SealPhase::AllCollected {
            return Err(E_STATE);
        }
        if !self.pieces_complete() {
            return Err(E_NOT_ALL);
        }
        // 逐件复算：封存件与哈希必须互证。
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    if !p.recompute_ok() {
                        return Err(E_HASH_DRIFT);
                    }
                }
                None => return Err(E_NOT_ALL),
            }
            i += 1;
        }
        let mut buf: Vec<u8> = Vec::new();
        // 固定序聚合：件号 1..5。
        i = 0;
        while i < PIECES {
            if let Some(p) = &self.pieces[i] {
                buf.push(p.piece_no);
                buf.extend_from_slice(p.hash.to_le_bytes().as_slice());
            }
            i += 1;
        }
        let h = fnv1a64(&buf);
        // 件级哈希链逐环落账：chain[i] = fnv(chain[i-1] ‖ hash_i)。
        let mut prev: u64 = 0;
        i = 0;
        while i < PIECES {
            if let Some(p) = &self.pieces[i] {
                let mut ib: Vec<u8> = Vec::new();
                ib.extend_from_slice(prev.to_le_bytes().as_slice());
                ib.extend_from_slice(p.hash.to_le_bytes().as_slice());
                prev = fnv1a64(&ib);
            }
            self.chain[i] = prev;
            i += 1;
        }
        self.sealed_hash = h;
        self.phase = SealPhase::Sealed;
        Ok(h)
    }

    /// 件级哈希链复算：从五件载荷重新逐环推导，与封账时链值逐环比对。
    /// 任一环不等即断链（件被篡改或链账损坏）。
    pub fn chain_verify(&self) -> Result<u64, u16> {
        if self.phase != SealPhase::Sealed {
            return Err(E_STATE);
        }
        let mut prev: u64 = 0;
        let mut i = 0usize;
        while i < PIECES {
            let p = self.pieces[i].as_ref().ok_or(E_NOT_ALL)?;
            if !p.recompute_ok() {
                return Err(E_HASH_DRIFT);
            }
            let mut ib: Vec<u8> = Vec::new();
            ib.extend_from_slice(prev.to_le_bytes().as_slice());
            ib.extend_from_slice(p.hash.to_le_bytes().as_slice());
            prev = fnv1a64(&ib);
            if prev != self.chain[i] {
                return Err(E_HASH_DRIFT);
            }
            i += 1;
        }
        Ok(prev)
    }

    /// 链尾哈希（= chain[4]，封账后恒等）。
    pub fn chain_tail(&self) -> u64 {
        self.chain[PIECES - 1]
    }

    /// 件数。
    pub fn piece_count(&self) -> usize {
        self.piece_count
    }

    /// 写尝试账行数。
    pub fn attempt_count(&self) -> usize {
        self.attempt_count
    }

    /// 封账哈希读取。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 当前阶段。
    pub fn phase(&self) -> SealPhase {
        self.phase
    }

    /// 封账后写尝试：必须被拒且落检出账，漏检即断言失败。
    pub fn write_attempt(&mut self, piece_no: u8, bytes: u32) -> Result<(), u16> {
        if self.attempt_count >= MAX_ATTEMPTS {
            return Err(E_ATTEMPT_FULL);
        }
        if self.phase == SealPhase::Sealed {
            // 检出并拒绝。
            self.attempt_count += 1;
            let n = self.attempt_count as u32;
            self.attempts[self.attempt_count - 1] = Some(WriteAttempt {
                attempt_no: n,
                piece_no,
                bytes,
                rejected: true,
            });
            self.rejected_count += 1;
            return Err(E_WRITE_CLOSED);
        }
        // 未封账期的写走 put_piece，此口一律视为漏检。
        self.missed_count += 1;
        Err(E_WRITE_MISS)
    }

    /// 检出账守恒：检出数 == 拒绝数，漏检必须为 0。
    pub fn attempts_conserved(&self) -> bool {
        self.rejected_count == self.attempt_count as u32 && self.missed_count == 0
    }

    /// 封账后写尝试检出恒零断言：attempts 非空时必须全被拒且守恒。
    pub fn zero_leak_assert(&self) -> Result<(), u16> {
        if !self.attempts_conserved() {
            return Err(E_BAL);
        }
        if self.missed_count != 0 {
            return Err(E_WRITE_MISS);
        }
        let mut i = 0usize;
        while i < self.attempt_count {
            match &self.attempts[i] {
                Some(a) if !a.rejected => return Err(E_WRITE_MISS),
                Some(_) => {}
                None => {}
            }
            i += 1;
        }
        Ok(())
    }

    /// 件只读快照。
    pub fn get_piece(&self, piece_no: u8) -> Result<(String, u64), u16> {
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_PIECE_BAD);
        }
        match &self.pieces[(piece_no - 1) as usize] {
            Some(p) => Ok((p.piece_name.clone(), p.hash)),
            None => Err(E_PIECE_BAD),
        }
    }

    /// 单件盘点：按封账阶段与复算结果给状态。
    pub fn piece_status(&self, piece_no: u8) -> Result<PieceStatus, u16> {
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_PIECE_BAD);
        }
        let idx = (piece_no - 1) as usize;
        match &self.pieces[idx] {
            None => Ok(PieceStatus::Missing),
            Some(p) => {
                if self.phase == SealPhase::Sealed {
                    if p.recompute_ok() {
                        Ok(PieceStatus::SealedOk)
                    } else {
                        Ok(PieceStatus::Drift)
                    }
                } else {
                    Ok(PieceStatus::Collected)
                }
            }
        }
    }

    /// 五件全量盘点：状态码聚合（盘点断言的取数口）。
    pub fn status_tally(&self) -> [u32; 4] {
        let mut tally = [0u32; 4];
        let mut n = 1u8;
        while n as usize <= PIECES {
            if let Ok(st) = self.piece_status(n) {
                tally[st.code() as usize] += 1;
            }
            n += 1;
        }
        tally
    }

    /// 封账定版审计行：逐件"件号|件名|哈希|复算"。
    pub fn audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 嫁接账五件封账审计 ===".to_string());
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    let mut line = String::from("P");
                    line.push_str(&itoa(p.piece_no as u64));
                    line.push('|');
                    line.push_str(&p.piece_name);
                    line.push('|');
                    line.push_str(&hex64(p.hash));
                    line.push_str("|复算=");
                    line.push_str(if p.recompute_ok() { "过" } else { "漂" });
                    lines.push(line);
                }
                None => lines.push(String::from("P?|缺件")),
            }
            i += 1;
        }
        lines.push("链尾: ".to_string() + &hex64(self.chain_tail()));
        lines
    }

    /// 写尝试账行渲染。
    pub fn attempt_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 封账后写尝试检出账 ===".to_string());
        let mut i = 0usize;
        while i < self.attempt_count {
            if let Some(a) = &self.attempts[i] {
                let mut line = String::from("W");
                line.push_str(&itoa(a.attempt_no as u64));
                line.push_str("|件");
                line.push_str(&itoa(a.piece_no as u64));
                line.push_str("|");
                line.push_str(&itoa(a.bytes as u64));
                line.push_str("B|");
                line.push_str(if a.rejected { "拒" } else { "漏" });
                lines.push(line);
            }
            i += 1;
        }
        lines
    }
}

/// u64 转 16 位十六进制字符串（确定性小写）。
fn hex64(v: u64) -> String {
    let digits = b"0123456789abcdef";
    let mut s = String::new();
    let mut i = 0usize;
    while i < 16 {
        let shift = 60 - (i as u32) * 4;
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(digits[nib] as char);
        i += 1;
    }
    s
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
/// 演练：五件齐 5/5 封账 + 封账后写尝试全检出 + 恒零断言。
pub fn drill() -> Result<u32, u16> {
    let mut gs = GraftSeal::new();
    // 五件依序落账。
    let h1 = gs.put_piece(P_LEDGER, "台账账", b"ledger payload b37")?;
    let h2 = gs.put_piece(P_FINGER, "指纹账", b"fingerprint payload b37")?;
    let h3 = gs.put_piece(P_FLOW, "流程账", b"flow payload b37")?;
    let h4 = gs.put_piece(P_INTERCEPT, "拦截账", b"intercept payload b37")?;
    let h5 = gs.put_piece(P_REPORT, "报告账", b"report payload b37")?;
    if h1 == 0 || h2 == 0 || h3 == 0 || h4 == 0 || h5 == 0 {
        return Err(SEG_BASE);
    }
    if !gs.pieces_complete() || gs.phase() != SealPhase::AllCollected {
        return Err(SEG_BASE);
    }
    let sealed = gs.seal()?;
    if sealed == 0 || gs.phase() != SealPhase::Sealed {
        return Err(SEG_BASE);
    }
    // 链复算：链尾必须与封账链尾一致。
    let tail = gs.chain_verify()?;
    if tail != gs.chain_tail() || tail == 0 {
        return Err(SEG_BASE);
    }
    // 审计行必须覆盖五件。
    let audit = gs.audit_lines();
    if audit.len() < 7 {
        return Err(SEG_BASE);
    }
    // 五件盘点：封账后必须全为封存态。
    let tally = gs.status_tally();
    if tally[PieceStatus::SealedOk.code() as usize] != 5
        || tally[PieceStatus::Missing.code() as usize] != 0
    {
        return Err(SEG_BASE);
    }
    // 封账后写尝试：三连写全部必须被拒并落检出账。
    let mut hits = 0u32;
    let mut k = 0u32;
    while k < 3 {
        match gs.write_attempt(P_LEDGER, 64 + k) {
            Err(c) if c == E_WRITE_CLOSED => hits += 1,
            _ => return Err(SEG_BASE),
        }
        k += 1;
    }
    if hits != 3 {
        return Err(SEG_BASE);
    }
    gs.zero_leak_assert()?;
    if !gs.attempts_conserved() {
        return Err(SEG_BASE);
    }
    // 重复封账拒绝。
    match gs.seal() {
        Err(c) if c == E_SEAL_TWICE => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：缺件封账被拒。
    let mut gs2 = GraftSeal::new();
    let _ = gs2.put_piece(P_LEDGER, "台账账", b"only one")?;
    match gs2.seal() {
        Err(c) if c == E_STATE || c == E_NOT_ALL => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：未封账期 write_attempt 必须记漏检。
    let mut gs3 = GraftSeal::new();
    match gs3.write_attempt(P_LEDGER, 8) {
        Err(c) if c == E_WRITE_MISS => {}
        _ => return Err(SEG_BASE),
    }
    match gs3.zero_leak_assert() {
        Err(c) if c == E_WRITE_MISS => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
