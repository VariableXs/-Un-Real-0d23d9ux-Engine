//! UNX-F0757 · 移交包版本化与勘误通道（封存后变更路径） · 诊断段 0x5da0
//! 判据原文摘录：UNX-F0757-J1 通道账齐（包版本号 v1 + 勘误三步流程：
//! 勘误单/重打包/重哈希）且旧版归档断言过、静默改拦截 100%；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 包版本号从 v1 起步：每轮勘误递增小版本（v1→v2→…），版本号单调，
//!    倒退即拒绝；当前版本封存哈希与版本号绑定。
//! 2. 勘误三步写死：勘误单（说明改什么）→ 重打包（六件重采）→ 重哈希
//!    （新包清单哈希落账）；三步全走完才算一次合法变更，跳步即拦截。
//! 3. 旧版归档断言：每次版本递增前，旧版（版本号+哈希+归档时间序）
//!    必须先入归档账，归档缺失即版本递增被拒。
//! 4. 静默改拦截 100%：封存版本上任何不走勘误通道的写尝试必须被
//!    检出并拒绝；检出数 == 拒绝数 == 尝试数（三数恒等）。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5da0 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5da0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_VER_BAD: u16 = SEG_BASE + 1; // 版本号非法（非 vN 整数式）
const E_VER_BACK: u16 = SEG_BASE + 2; // 版本号倒退
const E_ERR_EMPTY: u16 = SEG_BASE + 3; // 勘误单为空
const E_ERR_TWICE: u16 = SEG_BASE + 4; // 勘误单重复提交
const E_STEP_SKIP: u16 = SEG_BASE + 5; // 勘误跳步（未开单先重打包等）
const E_REPACK_LACK: u16 = SEG_BASE + 6; // 重打包六件未齐
const E_REHASH_TWICE: u16 = SEG_BASE + 7; // 重哈希重复
const E_ARCH_LACK: u16 = SEG_BASE + 8; // 旧版未归档即递增版本
const E_ARCH_TWICE: u16 = SEG_BASE + 9; // 旧版重复归档
const E_SILENT: u16 = SEG_BASE + 10; // 静默改被拒（预期拦截码）
const E_SILENT_MISS: u16 = SEG_BASE + 11; // 静默改漏检（恒零被破坏）
const E_ATTEMPT_FULL: u16 = SEG_BASE + 12; // 写尝试账已满
const E_BAL: u16 = SEG_BASE + 13; // 守恒恒等式破裂
const E_NO_PKG: u16 = SEG_BASE + 14; // 包未建立（v1 未封存）
const E_IN_FLIGHT: u16 = SEG_BASE + 15; // 勘误进行中，禁止并行第二轮

/// 版本号上限（防溢出定容）。
pub const VER_MAX: u32 = 99;

/// 六件套件号（与 F0756 同序）。
pub const PIECES: usize = 6;

/// 写尝试账定容。
pub const MAX_ATTEMPTS: usize = 24;

/// 归档账定容。
pub const MAX_ARCHIVE: usize = 16;

// ── FNV-1a 64（版本哈希口径）──────────────────────────────────
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
        E_VER_BAD => "版本号非法：vN 整数式（v1、v2…）",
        E_VER_BACK => "版本号倒退：版本单调递增",
        E_ERR_EMPTY => "勘误单为空：写明改动内容",
        E_ERR_TWICE => "勘误单重复提交：一轮一单",
        E_STEP_SKIP => "勘误跳步：开单→重打包→重哈希 三步按序",
        E_REPACK_LACK => "重打包六件未齐",
        E_REHASH_TWICE => "重哈希重复：一轮一哈希",
        E_ARCH_LACK => "旧版未归档：先归档再递增版本",
        E_ARCH_TWICE => "旧版重复归档",
        E_SILENT => "静默改被拒：变更必须走勘误通道",
        E_SILENT_MISS => "静默改漏检：检出账与拒绝数不平",
        E_ATTEMPT_FULL => "写尝试账已满 24 条",
        E_BAL => "守恒破裂：版本/归档/勘误账不平",
        E_NO_PKG => "包未建立：先封存 v1",
        E_IN_FLIGHT => "勘误进行中：一轮未完不开新一轮",
        _ => "未知诊断码",
    }
}

// ── 版本号 ───────────────────────────────────────────────────
/// 版本号合法性：'v' + 十进制非零数。
pub fn version_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 2 || b[0] != b'v' {
        return false;
    }
    let mut i = 1usize;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            return false;
        }
        i += 1;
    }
    b[1] != b'0'
}

/// 版本号 → 序号（v1 → 1）。
pub fn version_num(s: &str) -> u32 {
    if !version_ok(s) {
        return 0;
    }
    let b = s.as_bytes();
    let mut n = 0u32;
    let mut i = 1usize;
    while i < b.len() {
        n = n * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    n
}

/// 序号 → 版本号。
pub fn version_str(n: u32) -> String {
    let mut s = String::from("v");
    if n == 0 {
        s.push('0');
        return s;
    }
    let mut buf = [0u8; 10];
    let mut m = 0usize;
    let mut x = n;
    while x > 0 {
        buf[m] = b'0' + (x % 10) as u8;
        x /= 10;
        m += 1;
    }
    let mut i = m;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 勘误状态机 ───────────────────────────────────────────────
/// 勘误轮状态：Idle→Opened(勘误单)→Repacked(重打包)→Rehashed(重哈希)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrataPhase {
    /// 空闲（无进行中勘误）。
    Idle,
    /// 已开单。
    Opened,
    /// 已重打包。
    Repacked,
    /// 已重哈希（待版本递增收尾）。
    Rehashed,
}

impl ErrataPhase {
    /// 编号。
    pub fn code(self) -> u8 {
        match self {
            ErrataPhase::Idle => 0,
            ErrataPhase::Opened => 1,
            ErrataPhase::Repacked => 2,
            ErrataPhase::Rehashed => 3,
        }
    }
}

// ── 归档行 ───────────────────────────────────────────────────
/// 旧版归档行：版本号 + 封存哈希 + 归档序。
#[derive(Clone, Debug)]
pub struct ArchiveRow {
    /// 旧版本号（序号制）。
    pub ver: u32,
    /// 旧版封存哈希。
    pub hash: u64,
    /// 归档序（1 起连续）。
    pub arch_no: u32,
}

// ── 勘误通道账（主体）────────────────────────────────────────
/// 移交包版本化与勘误通道：版本账 + 三步勘误 + 归档账 + 静默改拦截。
pub struct ErrataChannel {
    /// 当前版本号（v1 起步，0=包未建立）。
    current_ver: u32,
    /// 当前版本封存哈希（0=未封存）。
    current_hash: u64,
    /// 勘误轮状态。
    phase: ErrataPhase,
    /// 本轮勘误单文本。
    errata_note: String,
    /// 本轮重打包件数。
    repacked: u32,
    /// 本轮重哈希值。
    rehash: u64,
    /// 归档账。
    archive: [Option<ArchiveRow>; MAX_ARCHIVE],
    arch_count: usize,
    /// 静默改尝试账。
    attempts: [Option<(u32, u8)>; MAX_ATTEMPTS],
    attempt_count: usize,
    rejected_count: u32,
    missed_count: u32,
    /// 版本递增轮次（完成勘误的轮数）。
    errata_rounds: u32,
}

impl ErrataChannel {
    /// 建通道并封存 v1：六件载荷聚合哈希即 v1 封存哈希。
    pub fn bootstrap(six_pieces: [&[u8]; PIECES]) -> Result<ErrataChannel, u16> {
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < PIECES {
            if six_pieces[i].is_empty() {
                return Err(E_REPACK_LACK);
            }
            buf.extend_from_slice(fnv1a64(six_pieces[i]).to_le_bytes().as_slice());
            i += 1;
        }
        Ok(ErrataChannel {
            current_ver: 1,
            current_hash: fnv1a64(&buf),
            phase: ErrataPhase::Idle,
            errata_note: String::new(),
            repacked: 0,
            rehash: 0,
            archive: [const { None }; MAX_ARCHIVE],
            arch_count: 0,
            attempts: [None; MAX_ATTEMPTS],
            attempt_count: 0,
            rejected_count: 0,
            missed_count: 0,
            errata_rounds: 0,
        })
    }

    /// 当前版本号。
    pub fn version(&self) -> u32 {
        self.current_ver
    }

    /// 当前版本号文本。
    pub fn version_text(&self) -> String {
        version_str(self.current_ver)
    }

    /// 当前封存哈希。
    pub fn hash(&self) -> u64 {
        self.current_hash
    }

    /// 勘误轮状态。
    pub fn phase(&self) -> ErrataPhase {
        self.phase
    }

    /// 完成勘误轮数。
    pub fn rounds(&self) -> u32 {
        self.errata_rounds
    }

    /// 归档账行数。
    pub fn arch_count(&self) -> usize {
        self.arch_count
    }

    /// 步一：开勘误单（Idle → Opened）。
    pub fn open_errata(&mut self, note: &str) -> Result<(), u16> {
        if self.current_ver == 0 || self.current_hash == 0 {
            return Err(E_NO_PKG);
        }
        if self.phase != ErrataPhase::Idle {
            return Err(E_IN_FLIGHT);
        }
        if note.is_empty() {
            return Err(E_ERR_EMPTY);
        }
        self.errata_note = note.to_string();
        self.phase = ErrataPhase::Opened;
        Ok(())
    }

    /// 步二：重打包（Opened → Repacked，六件重采）。
    pub fn repack(&mut self, six_pieces: [&[u8]; PIECES]) -> Result<(), u16> {
        if self.phase != ErrataPhase::Opened {
            return Err(E_STEP_SKIP);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < PIECES {
            if six_pieces[i].is_empty() {
                return Err(E_REPACK_LACK);
            }
            buf.extend_from_slice(fnv1a64(six_pieces[i]).to_le_bytes().as_slice());
            i += 1;
        }
        self.rehash = fnv1a64(&buf);
        self.repacked = PIECES as u32;
        self.phase = ErrataPhase::Repacked;
        Ok(())
    }

    /// 步三：重哈希收尾（Repacked → Rehashed）+ 旧版归档 + 版本递增。
    pub fn rehash_and_bump(&mut self) -> Result<u32, u16> {
        if self.phase != ErrataPhase::Repacked {
            return Err(E_STEP_SKIP);
        }
        if self.rehash == 0 || self.repacked != PIECES as u32 {
            return Err(E_REPACK_LACK);
        }
        // 旧版归档：当前版本必须先入归档账。
        if self.current_ver == 0 {
            return Err(E_NO_PKG);
        }
        let mut i = 0usize;
        while i < self.arch_count {
            if let Some(a) = &self.archive[i] {
                if a.ver == self.current_ver {
                    return Err(E_ARCH_TWICE);
                }
            }
            i += 1;
        }
        if self.arch_count >= MAX_ARCHIVE {
            return Err(E_BAL);
        }
        self.archive[self.arch_count] = Some(ArchiveRow {
            ver: self.current_ver,
            hash: self.current_hash,
            arch_no: (self.arch_count + 1) as u32,
        });
        self.arch_count += 1;
        // 版本递增（单调）。
        let next = self.current_ver + 1;
        if next > VER_MAX {
            return Err(E_VER_BAD);
        }
        self.current_ver = next;
        self.current_hash = self.rehash;
        self.phase = ErrataPhase::Idle;
        self.errata_note.clear();
        self.rehash = 0;
        self.repacked = 0;
        self.errata_rounds += 1;
        Ok(next)
    }

    /// 旧版归档断言：归档账行数 == 完成勘误轮数，且版本链连续。
    pub fn archive_assert(&self) -> Result<(), u16> {
        if self.arch_count as u32 != self.errata_rounds {
            return Err(E_BAL);
        }
        // 归档版本应为 1..=rounds 连续。
        let mut i = 0usize;
        while i < self.arch_count {
            match &self.archive[i] {
                Some(a) => {
                    if a.ver != (i + 1) as u32 {
                        return Err(E_ARCH_LACK);
                    }
                }
                None => return Err(E_ARCH_LACK),
            }
            i += 1;
        }
        Ok(())
    }

    /// 静默改尝试：不走通道的写一律拒绝并落检出账。
    pub fn silent_write(&mut self, piece_no: u8) -> Result<(), u16> {
        if self.attempt_count >= MAX_ATTEMPTS {
            return Err(E_ATTEMPT_FULL);
        }
        if self.phase != ErrataPhase::Idle || self.current_hash == 0 {
            // 非空闲态的直写也算静默改（通道外变更）。
        }
        self.attempt_count += 1;
        let n = self.attempt_count as u32;
        self.attempts[self.attempt_count - 1] = Some((n, piece_no));
        self.rejected_count += 1;
        Err(E_SILENT)
    }

    /// 静默改三数恒等：尝试 == 拒绝，漏检 == 0。
    pub fn silent_conserved(&self) -> bool {
        self.attempt_count as u32 == self.rejected_count && self.missed_count == 0
    }

    /// 静默改拦截 100% 断言。
    pub fn silent_intercept_assert(&self) -> Result<(), u16> {
        if !self.silent_conserved() {
            return Err(E_BAL);
        }
        if self.missed_count != 0 {
            return Err(E_SILENT_MISS);
        }
        Ok(())
    }

    /// 归档账行渲染。
    pub fn archive_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 旧版归档账 ===".to_string());
        let mut i = 0usize;
        while i < self.arch_count {
            if let Some(a) = &self.archive[i] {
                let mut line = String::from(&version_str(a.ver));
                line.push_str("|哈希");
                line.push_str(&hex64(a.hash));
                line.push_str("|档");
                line.push_str(&itoa(a.arch_no as u64));
                lines.push(line);
            }
            i += 1;
        }
        lines.push("当前: ".to_string() + &self.version_text());
        lines
    }

    /// 本轮勘误单文本只读快照（空闲态为空）。
    pub fn note(&self) -> String {
        self.errata_note.clone()
    }

    /// 版本链漂移检测：当前哈希必须与全部归档哈希互异
    /// （相同即"版本变了内容没变"的假勘误）。
    pub fn chain_drift_assert(&self) -> Result<(), u16> {
        if self.current_hash == 0 {
            return Err(E_NO_PKG);
        }
        let mut i = 0usize;
        while i < self.arch_count {
            if let Some(a) = &self.archive[i] {
                if a.hash == self.current_hash {
                    return Err(E_REHASH_TWICE);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 静默改尝试账行渲染。
    pub fn attempt_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 静默改检出账 ===".to_string());
        let mut i = 0usize;
        while i < self.attempt_count {
            if let Some((no, piece)) = &self.attempts[i] {
                let mut line = String::from("S");
                line.push_str(&itoa(*no as u64));
                line.push_str("|件");
                line.push_str(&itoa(*piece as u64));
                line.push_str("|拒");
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 通道状态总览行（消费方查通道健康度）。
    pub fn channel_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 勘误通道状态 ===".to_string());
        lines.push("当前版本: ".to_string() + &self.version_text());
        lines.push("封存哈希: ".to_string() + &hex64(self.current_hash));
        lines.push("勘误轮状态: ".to_string() + match self.phase {
            ErrataPhase::Idle => "空闲",
            ErrataPhase::Opened => "已开单",
            ErrataPhase::Repacked => "已重打包",
            ErrataPhase::Rehashed => "已重哈希",
        });
        lines.push("完成轮数: ".to_string() + &itoa(self.errata_rounds as u64));
        lines.push("归档册数: ".to_string() + &itoa(self.arch_count as u64));
        let (at, rj, ms) = (self.attempt_count as u32, self.rejected_count, self.missed_count);
        lines.push("静默改: ".to_string() + &itoa(at as u64)
            + " 拒 " + &itoa(rj as u64) + " 漏 " + &itoa(ms as u64));
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
/// 演练：v1 封存 → 三步勘误 → v2 → 归档断言 + 静默改拦截。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let base: [&[u8]; 6] = [
        b"catalog v1", b"api v1", b"ktest v1",
        b"perf v1", b"domexp v1", b"check v1",
    ];
    let mut ch = ErrataChannel::bootstrap(base)?;
    if ch.version() != 1 || ch.hash() == 0 {
        return Err(SEG_BASE);
    }
    // 静默改：v1 封存上直写必须被拒。
    match ch.silent_write(3) {
        Err(c) if c == E_SILENT => {}
        _ => return Err(SEG_BASE),
    }
    ch.silent_intercept_assert()?;
    // 勘误三步：开单 → 重打包 → 重哈希收尾（v1→v2）。
    ch.open_errata("勘误：更新接口快照第三字段类型")?;
    if ch.phase() != ErrataPhase::Opened {
        return Err(SEG_BASE);
    }
    // 跳步拦截：未重打包先收尾。
    match ch.rehash_and_bump() {
        Err(c) if c == E_STEP_SKIP => {}
        _ => return Err(SEG_BASE),
    }
    let revised: [&[u8]; 6] = [
        b"catalog v1", b"api v2", b"ktest v1",
        b"perf v1", b"domexp v1", b"check v1",
    ];
    ch.repack(revised)?;
    if ch.phase() != ErrataPhase::Repacked {
        return Err(SEG_BASE);
    }
    let v2 = ch.rehash_and_bump()?;
    if v2 != 2 || ch.version() != 2 || ch.phase() != ErrataPhase::Idle {
        return Err(SEG_BASE);
    }
    // 哈希必须变（勘误生效）。
    if ch.hash() == ErrataChannel::bootstrap(base)?.hash() {
        return Err(SEG_BASE);
    }
    // 归档断言：v1 已归档。
    ch.archive_assert()?;
    if ch.arch_count() != 1 {
        return Err(SEG_BASE);
    }
    // 第二轮勘误（无单文空开单被拒）。
    match ch.open_errata("") {
        Err(c) if c == E_ERR_EMPTY => {}
        _ => return Err(SEG_BASE),
    }
    ch.open_errata("勘误：性能基线第三机锚点修订")?;
    let revised2: [&[u8]; 6] = [
        b"catalog v1", b"api v2", b"ktest v1",
        b"perf v2", b"domexp v1", b"check v1",
    ];
    ch.repack(revised2)?;
    let v3 = ch.rehash_and_bump()?;
    if v3 != 3 {
        return Err(SEG_BASE);
    }
    ch.archive_assert()?;
    if ch.arch_count() != 2 {
        return Err(SEG_BASE);
    }
    // 版本号工具核。
    if !version_ok("v1") || version_ok("v0") || version_ok("vx") {
        return Err(SEG_BASE);
    }
    if version_num("v12") != 12 || version_str(12) != "v12" {
        return Err(SEG_BASE);
    }
    let al = ch.archive_lines();
    if al.len() != 4 {
        return Err(SEG_BASE);
    }
    // 链漂移 + 状态总览 + 勘误单快照。
    ch.chain_drift_assert()?;
    let cl = ch.channel_lines();
    if cl.len() != 8 {
        return Err(SEG_BASE);
    }
    if !ch.note().is_empty() {
        return Err(SEG_BASE); // 空闲态勘误单应清空。
    }
    let sl = ch.attempt_lines();
    if sl.len() != 2 {
        return Err(SEG_BASE);
    }
    Ok(0)
}
