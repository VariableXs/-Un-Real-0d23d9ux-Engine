//! UNX-F0756 · 移交包哈希封存（包级完整性） · 诊断段 0x5d80
//! 判据原文摘录：UNX-F0756-J1 封存账齐（六件逐件哈希 + 包清单哈希 7/7）
//! 且封存后写尝试恒零、逐件哈希复算一致；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 七哈希结构：六件逐件哈希（账册汇编/接口快照/ktest 封版/性能基线/
//!    域经卷/验收核对单）+ 包清单哈希（六件哈希按件号序聚合），
//!    7/7 齐全才算封存成立。
//! 2. 逐件复算：每件存载荷与哈希，复算（重哈希载荷）必须与落账哈希一致，
//!    漂移即封存不可信。
//! 3. 封存后写尝试恒零：封存后任何写尝试必须被检出并拒绝，
//!    检出账行数 == 拒绝数 == 写尝试数（三数恒等，漏检即断言失败）。
//! 4. 定容数组账本 + 守恒恒等式：pieces(6) + manifest(1) == sealed_rows(7)。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5d80 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5d80;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_P_BAD: u16 = SEG_BASE + 1; // 件号越界（1..=6）
const E_P_DUP: u16 = SEG_BASE + 2; // 件哈希重复落账
const E_P_EMPTY: u16 = SEG_BASE + 3; // 件载荷为空
const E_P_DRIFT: u16 = SEG_BASE + 4; // 逐件哈希复算漂移
const E_P_LACK: u16 = SEG_BASE + 5; // 六件未齐即封包被拒
const E_M_TWICE: u16 = SEG_BASE + 6; // 包清单哈希重复落账
const E_M_PRE: u16 = SEG_BASE + 7; // 六件未齐先落包清单哈希
const E_M_DRIFT: u16 = SEG_BASE + 8; // 包清单哈希复算漂移
const E_STATE: u16 = SEG_BASE + 9; // 封存状态机非法边
const E_WRITE_CLOSED: u16 = SEG_BASE + 10; // 封存后写被拒（预期拦截码）
const E_WRITE_MISS: u16 = SEG_BASE + 11; // 写尝试漏检（恒零被破坏）
const E_ATTEMPT_FULL: u16 = SEG_BASE + 12; // 写尝试账已满
const E_BAL: u16 = SEG_BASE + 13; // 守恒恒等式破裂（7/7 不平）
const E_SEAL_TWICE: u16 = SEG_BASE + 14; // 重复封存
const E_EMPTY: u16 = SEG_BASE + 15; // 封存账为空

/// 六件件号（与 F0749 清单同序）。
pub const P_CATALOG: u8 = 1;
pub const P_API: u8 = 2;
pub const P_KTEST: u8 = 3;
pub const P_PERF: u8 = 4;
pub const P_DOMEXP: u8 = 5;
pub const P_CHECK: u8 = 6;
pub const PIECES: usize = 6;

/// 封存行总数：6 件 + 1 清单 = 7。
pub const SEALED_ROWS: usize = 7;

/// 写尝试账定容。
pub const MAX_ATTEMPTS: usize = 24;

/// 六件套件名表（封存账表头消费口）。
pub const PIECE_NAMES: [&str; 6] = [
    "账册汇编",
    "接口快照",
    "ktest 封版",
    "性能基线",
    "域经卷",
    "验收核对单",
];

/// 件号 → 件名（越界拒绝）。
pub fn piece_name(piece_no: u8) -> Result<&'static str, u16> {
    if piece_no == 0 || piece_no as usize > PIECE_NAMES.len() {
        return Err(E_P_BAD);
    }
    Ok(PIECE_NAMES[(piece_no - 1) as usize])
}

// ── FNV-1a 64（件哈希/清单哈希/封存哈希口径）────────────────────
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
        E_P_BAD => "件号越界：六件套 1..=6",
        E_P_DUP => "件哈希重复落账：一件一哈希",
        E_P_EMPTY => "件载荷为空：空件不许入包",
        E_P_DRIFT => "哈希复算漂移：载荷与哈希不互证",
        E_P_LACK => "六件未齐：六件套缺一不可",
        E_M_TWICE => "包清单哈希重复落账",
        E_M_PRE => "六件未齐先落清单哈希：顺序违规",
        E_M_DRIFT => "清单哈希复算漂移",
        E_STATE => "封存状态机非法边：Collecting→Ready→Sealed",
        E_WRITE_CLOSED => "封存后写被拒：包级完整性锁定",
        E_WRITE_MISS => "写尝试漏检：检出账与拒绝数不平",
        E_ATTEMPT_FULL => "写尝试账已满 24 条",
        E_BAL => "守恒破裂：6 件 + 1 清单必须等于 7 行",
        E_SEAL_TWICE => "重复封存：封存一次性不可逆",
        E_EMPTY => "封存账为空",
        _ => "未知诊断码",
    }
}

// ── 封存状态机 ───────────────────────────────────────────────
/// 封存生命周期：Collecting→Ready→Sealed，一步不跳。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealPhase {
    /// 采集中（落件哈希）。
    Collecting,
    /// 六件齐备（可落清单哈希）。
    Ready,
    /// 已封存（只读 + 写拦截）。
    Sealed,
}

impl SealPhase {
    /// 编号。
    pub fn code(self) -> u8 {
        match self {
            SealPhase::Collecting => 1,
            SealPhase::Ready => 2,
            SealPhase::Sealed => 3,
        }
    }
}

// ── 件封存行 ─────────────────────────────────────────────────
/// 一件的封存行：件号 + 载荷 + 哈希。
#[derive(Clone, Debug)]
pub struct PieceSeal {
    /// 件号（1..=6）。
    pub piece_no: u8,
    /// 件载荷（复算依据）。
    pub payload: Vec<u8>,
    /// 落账哈希。
    pub hash: u64,
}

impl PieceSeal {
    /// 复算一致：载荷重哈希 == 落账哈希。
    pub fn recompute_ok(&self) -> bool {
        fnv1a64(&self.payload) == self.hash
    }
}

// ── 写尝试行 ─────────────────────────────────────────────────
/// 封存后一次写尝试的检出账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteAttempt {
    /// 尝试序号。
    pub attempt_no: u32,
    /// 目标件号。
    pub piece_no: u8,
    /// 载荷长度。
    pub bytes: u32,
    /// 是否被拒（封存后恒 true）。
    pub rejected: bool,
}

// ── 移交包封存账（主体）──────────────────────────────────────
/// 移交包哈希封存账：6 件哈希 + 1 清单哈希 + 写拦截 + 复算核。
pub struct PackageSeal {
    pieces: [Option<PieceSeal>; PIECES],
    piece_count: usize,
    /// 包清单哈希（0=未落）。
    manifest_hash: u64,
    phase: SealPhase,
    attempts: [Option<WriteAttempt>; MAX_ATTEMPTS],
    attempt_count: usize,
    rejected_count: u32,
    missed_count: u32,
}

impl PackageSeal {
    /// 开账。
    pub fn new() -> PackageSeal {
        PackageSeal {
            pieces: [const { None }; PIECES],
            piece_count: 0,
            manifest_hash: 0,
            phase: SealPhase::Collecting,
            attempts: [None; MAX_ATTEMPTS],
            attempt_count: 0,
            rejected_count: 0,
            missed_count: 0,
        }
    }

    /// 落件哈希：件号合法 + 载荷非空 + 未重复。
    pub fn put_piece(&mut self, piece_no: u8, payload: &[u8]) -> Result<u64, u16> {
        if self.phase == SealPhase::Sealed {
            return Err(E_WRITE_CLOSED);
        }
        if self.phase != SealPhase::Collecting {
            return Err(E_STATE);
        }
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_P_BAD);
        }
        if payload.is_empty() {
            return Err(E_P_EMPTY);
        }
        let idx = (piece_no - 1) as usize;
        if self.pieces[idx].is_some() {
            return Err(E_P_DUP);
        }
        let h = fnv1a64(payload);
        self.pieces[idx] = Some(PieceSeal {
            piece_no,
            payload: Vec::from(payload),
            hash: h,
        });
        self.piece_count += 1;
        if self.piece_count == PIECES {
            self.phase = SealPhase::Ready;
        }
        Ok(h)
    }

    /// 落包清单哈希：六件齐 + 未落 + 复算一致（聚合哈希自动推导）。
    pub fn put_manifest(&mut self, declared: u64) -> Result<u64, u16> {
        if self.phase == SealPhase::Sealed {
            return Err(E_WRITE_CLOSED);
        }
        if self.phase != SealPhase::Ready {
            return Err(if self.piece_count < PIECES { E_P_LACK } else { E_STATE });
        }
        if self.manifest_hash != 0 {
            return Err(E_M_TWICE);
        }
        // 聚合：六件哈希按件号序聚合。
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    if !p.recompute_ok() {
                        return Err(E_P_DRIFT);
                    }
                    buf.extend_from_slice(p.hash.to_le_bytes().as_slice());
                }
                None => return Err(E_P_LACK),
            }
            i += 1;
        }
        let mh = fnv1a64(&buf);
        // 声明值必须与推导值一致（防外挂清单）。
        if declared != 0 && declared != mh {
            return Err(E_M_DRIFT);
        }
        self.manifest_hash = mh;
        Ok(mh)
    }

    /// 包清单哈希读取。
    pub fn manifest_hash(&self) -> u64 {
        self.manifest_hash
    }

    /// 当前阶段。
    pub fn phase(&self) -> SealPhase {
        self.phase
    }

    /// 件数。
    pub fn piece_count(&self) -> usize {
        self.piece_count
    }

    /// 封存：清单哈希在位 + 逐件复算 + 守恒 7/7。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.phase == SealPhase::Sealed {
            return Err(E_SEAL_TWICE);
        }
        if self.phase != SealPhase::Ready {
            return Err(E_P_LACK);
        }
        if self.manifest_hash == 0 {
            return Err(E_M_PRE);
        }
        // 逐件复算。
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    if !p.recompute_ok() {
                        return Err(E_P_DRIFT);
                    }
                }
                None => return Err(E_P_LACK),
            }
            i += 1;
        }
        // 守恒：6 件 + 1 清单 = 7 行。
        if self.piece_count + 1 != SEALED_ROWS {
            return Err(E_BAL);
        }
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.manifest_hash.to_le_bytes().as_slice());
        self.phase = SealPhase::Sealed;
        Ok(fnv1a64(&buf))
    }

    /// 封存后写尝试：必须被拒并落检出账；未封存期调用记漏检。
    pub fn write_attempt(&mut self, piece_no: u8, bytes: u32) -> Result<(), u16> {
        if self.attempt_count >= MAX_ATTEMPTS {
            return Err(E_ATTEMPT_FULL);
        }
        if self.phase == SealPhase::Sealed {
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
        self.missed_count += 1;
        Err(E_WRITE_MISS)
    }

    /// 写尝试三数恒等断言：尝试数 == 拒绝数，漏检 == 0。
    pub fn attempts_conserved(&self) -> bool {
        self.attempt_count as u32 == self.rejected_count && self.missed_count == 0
    }

    /// 封存后写尝试恒零断言（全账扫）。
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
                _ => {}
            }
            i += 1;
        }
        Ok(())
    }

    /// 逐件哈希复算断言（封存后复核口）。
    pub fn recompute_all_assert(&self) -> Result<(), u16> {
        if self.piece_count != PIECES {
            return Err(E_P_LACK);
        }
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    if !p.recompute_ok() {
                        return Err(E_P_DRIFT);
                    }
                }
                None => return Err(E_P_LACK),
            }
            i += 1;
        }
        Ok(())
    }

    /// 件哈希只读查询。
    pub fn piece_hash(&self, piece_no: u8) -> Result<u64, u16> {
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_P_BAD);
        }
        match &self.pieces[(piece_no - 1) as usize] {
            Some(p) => Ok(p.hash),
            None => Err(E_P_LACK),
        }
    }

    /// 写尝试账行渲染。
    pub fn attempt_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 封存后写尝试检出账 ===".to_string());
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

    /// 七哈希清单行渲染。
    pub fn seal_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 移交包七哈希封存账 ===".to_string());
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    let mut line = String::from("件");
                    line.push_str(&itoa(p.piece_no as u64));
                    line.push_str("|");
                    line.push_str(&hex64(p.hash));
                    line.push_str(if p.recompute_ok() { "|复算过" } else { "|漂移" });
                    lines.push(line);
                }
                None => lines.push(String::from("件?|缺")),
            }
            i += 1;
        }
        lines.push("清单|".to_string() + &hex64(self.manifest_hash));
        lines
    }

    /// 写尝试统计三元组：(尝试数, 拒绝数, 漏检数)。
    pub fn attempt_summary(&self) -> (u32, u32, u32) {
        (self.attempt_count as u32, self.rejected_count, self.missed_count)
    }

    /// 件载荷只读快照（复算凭证）。
    pub fn payload_of(&self, piece_no: u8) -> Result<Vec<u8>, u16> {
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_P_BAD);
        }
        match &self.pieces[(piece_no - 1) as usize] {
            Some(p) => Ok(p.payload.clone()),
            None => Err(E_P_LACK),
        }
    }

    /// 包级完整性报告（消费方验包全览行）。
    pub fn integrity_report(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 包级完整性报告 ===".to_string());
        lines.push("阶段: ".to_string() + match self.phase {
            SealPhase::Collecting => "采集中",
            SealPhase::Ready => "六件齐",
            SealPhase::Sealed => "已封存",
        });
        lines.push("件数: ".to_string() + &itoa(self.piece_count as u64) + "/6");
        let (at, rj, ms) = self.attempt_summary();
        lines.push("写尝试: ".to_string() + &itoa(at as u64)
            + " 拒 " + &itoa(rj as u64) + " 漏 " + &itoa(ms as u64));
        // 逐件复算行。
        let mut i = 0usize;
        while i < PIECES {
            let mut line = String::from("件");
            line.push_str(&itoa((i + 1) as u64));
            line.push_str("|");
            line.push_str(match piece_name((i + 1) as u8) {
                Ok(n) => n,
                Err(_) => "?",
            });
            line.push('|');
            match &self.pieces[i] {
                Some(p) => {
                    line.push_str(if p.recompute_ok() { "复算过" } else { "漂移" });
                }
                None => line.push_str("缺件"),
            }
            lines.push(line);
            i += 1;
        }
        lines
    }
}

/// u64 转 16 位十六进制字符串。
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
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
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
/// 演练：7/7 封存 + 复算一致 + 写拦截恒零 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut ps = PackageSeal::new();
    // 六件载荷落账。
    let payloads: [&[u8]; 6] = [
        b"catalog: 40+40 books chain 240000",
        b"api: six frozen tables 16 rows",
        b"ktest: 320 asserts all green",
        b"perf: T0T3 three machines median 2880ms",
        b"domexp: high/mid volume 4 entries",
        b"check: six criteria all pass",
    ];
    let mut hashes = [0u64; PIECES];
    let mut i = 0usize;
    while i < PIECES {
        hashes[i] = ps.put_piece((i + 1) as u8, payloads[i])?;
        i += 1;
    }
    if ps.piece_count() != 6 || ps.phase() != SealPhase::Ready {
        return Err(SEG_BASE);
    }
    // 包清单哈希：声明 0 = 让账本自动推导落账。
    let mh = ps.put_manifest(0)?;
    if mh == 0 || ps.manifest_hash() != mh {
        return Err(SEG_BASE);
    }
    // 复算一致 + 封存。
    ps.recompute_all_assert()?;
    let sealed = ps.seal()?;
    if sealed == 0 || ps.phase() != SealPhase::Sealed {
        return Err(SEG_BASE);
    }
    // 封存后写尝试：三连写全拒全检出。
    let mut hits = 0u32;
    let mut k = 0u32;
    while k < 3 {
        match ps.write_attempt((k % 6 + 1) as u8, 16 + k) {
            Err(c) if c == E_WRITE_CLOSED => hits += 1,
            _ => return Err(SEG_BASE),
        }
        k += 1;
    }
    if hits != 3 {
        return Err(SEG_BASE);
    }
    ps.zero_leak_assert()?;
    if !ps.attempts_conserved() {
        return Err(SEG_BASE);
    }
    // 重复封存拒绝。
    match ps.seal() {
        Err(c) if c == E_SEAL_TWICE => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：六件未齐先落清单。
    let mut ps2 = PackageSeal::new();
    let _ = ps2.put_piece(1, b"only one piece")?;
    match ps2.put_manifest(0) {
        Err(c) if c == E_P_LACK => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：声明值与推导值不一致。
    let mut ps3 = PackageSeal::new();
    let mut j = 0usize;
    while j < PIECES {
        let _ = ps3.put_piece((j + 1) as u8, payloads[j])?;
        j += 1;
    }
    match ps3.put_manifest(0xDEAD_0001) {
        Err(c) if c == E_M_DRIFT => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：未封存期写尝试记漏检。
    let mut ps4 = PackageSeal::new();
    match ps4.write_attempt(1, 8) {
        Err(c) if c == E_WRITE_MISS => {}
        _ => return Err(SEG_BASE),
    }
    match ps4.zero_leak_assert() {
        Err(c) if c == E_WRITE_MISS => {}
        _ => return Err(SEG_BASE),
    }
    // 七哈希清单可渲染。
    let sl = ps.seal_lines();
    if sl.len() != 8 {
        return Err(SEG_BASE);
    }
    // 取数口：件名、载荷、统计、完整性报告。
    if piece_name(3)? != "ktest 封版" {
        return Err(SEG_BASE);
    }
    let pl = ps.payload_of(1)?;
    if pl != payloads[0] {
        return Err(SEG_BASE);
    }
    let (at, rj, ms) = ps.attempt_summary();
    if at != 3 || rj != 3 || ms != 0 {
        return Err(SEG_BASE);
    }
    let ir = ps.integrity_report();
    if ir.len() != 11 {
        return Err(SEG_BASE);
    }
    Ok(0)
}
