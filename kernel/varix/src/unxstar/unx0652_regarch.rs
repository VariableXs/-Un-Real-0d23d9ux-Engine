//! UNX-F0652 · 回归产物归档（报告/日志/账三件）· 诊断段 0x5300（本单独占）
//!
//! 目标行数：≥200（纯功能代码）
//! 判据原文摘录：UNX-F0652-J1 每轮归档三件（报告/日志/账）齐备，10 轮归档 30 件零缺。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性输出。
//!  2. 归档件按（轮号 × 三件套）二维账组织，守恒恒等式：件数 = 轮数 × 3。
//!  3. 每件落账即计算 FNV-1a64 摘要，账内可解引、可复核。
//!  4. 重复归档同一（轮,类）拦截；空产物拦截；缺件逐轮点名。
//!  5. 诊断段 0x5300 为本模块独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::format;

pub const SEG_BASE: u16 = 0x5300;

// ── 诊断码闭集（SEG_BASE+1 .. +N）────────────────────────────
pub const E_EMPTY: u16 = SEG_BASE + 1; // 归档件字节为空
pub const E_DUP: u16 = SEG_BASE + 2;   // 同（轮,类）重复归档
pub const E_RANGE: u16 = SEG_BASE + 3; // 轮号越界（0 或超过容量）
pub const E_MISSING: u16 = SEG_BASE + 4; // 逐轮点名发现缺件
pub const E_SEALED: u16 = SEG_BASE + 5; // 归档簿已封账
pub const E_HASH: u16 = SEG_BASE + 6;  // 摘要复核不平

/// 归档件三类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    /// 轮报告：程序化生成的结构化结论。
    Report,
    /// 轮日志：逐判据执行流水。
    Log,
    /// 轮账：本轮产物登记账行。
    Ledger,
}

impl PieceKind {
    pub fn name(self) -> &'static str {
        match self {
            PieceKind::Report => "报告",
            PieceKind::Log => "日志",
            PieceKind::Ledger => "账",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            PieceKind::Report => 1,
            PieceKind::Log => 2,
            PieceKind::Ledger => 3,
        }
    }

    pub fn from_code(c: u8) -> Option<PieceKind> {
        match c {
            1 => Some(PieceKind::Report),
            2 => Some(PieceKind::Log),
            3 => Some(PieceKind::Ledger),
            _ => None,
        }
    }
}

/// FNV-1a64 摘要（确定性，整数算术）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 单件归档记录。
#[derive(Clone, Debug)]
pub struct ArchivePiece {
    /// 所属轮号（1 起）。
    pub round: u16,
    /// 件类。
    pub kind: PieceKind,
    /// 件名（确定性生成，不含手抄字段）。
    pub name: String,
    /// 件字节体。
    pub body: Vec<u8>,
    /// FNV-1a64 摘要。
    pub hash: u64,
}

/// 归档簿：容量 = 轮容量 × 三件套。
pub struct ArchiveBook {
    /// 轮容量上限（判据固定 10 轮）。
    pub round_capacity: u16,
    /// 已落账的归档件（追加式，禁止删除——零抹除纪律）。
    pub pieces: Vec<ArchivePiece>,
    /// 是否已封账。
    pub sealed: bool,
}

impl ArchiveBook {
    pub fn new(round_capacity: u16) -> ArchiveBook {
        ArchiveBook {
            round_capacity,
            pieces: Vec::new(),
            sealed: false,
        }
    }

    /// 归档一轮三件。三件任一为空即拦截。
    pub fn archive_round(
        &mut self,
        round: u16,
        report: &[u8],
        log: &[u8],
        ledger: &[u8],
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if round == 0 || round > self.round_capacity {
            return Err(E_RANGE);
        }
        if report.is_empty() || log.is_empty() || ledger.is_empty() {
            return Err(E_EMPTY);
        }
        let triple = [
            (PieceKind::Report, report),
            (PieceKind::Log, log),
            (PieceKind::Ledger, ledger),
        ];
        for (kind, body) in triple {
            if self.piece_of(round, kind).is_some() {
                return Err(E_DUP);
            }
        }
        for (kind, body) in triple {
            let name = make_name(round, kind);
            let hash = fnv1a64(body);
            self.pieces.push(ArchivePiece {
                round,
                kind,
                name,
                body: body.to_vec(),
                hash,
            });
        }
        Ok(())
    }

    /// 封账：封账前必须逐轮点名零缺。
    pub fn seal(&mut self) -> Result<u16, u16> {
        self.audit()?;
        self.sealed = true;
        Ok(self.pieces.len() as u16)
    }

    /// 逐轮点名审计：守恒恒等式 件数 = 轮数 × 3，缺一件即报 E_MISSING。
    pub fn audit(&self) -> Result<(), u16> {
        for r in 1..=self.round_capacity {
            for k in [PieceKind::Report, PieceKind::Log, PieceKind::Ledger] {
                if self.piece_of(r, k).is_none() {
                    return Err(E_MISSING);
                }
            }
        }
        Ok(())
    }

    /// 按账源解引：取（轮,类）件。
    pub fn piece_of(&self, round: u16, kind: PieceKind) -> Option<&ArchivePiece> {
        self.pieces
            .iter()
            .find(|p| p.round == round && p.kind == kind)
    }

    /// 摘要复核：账内 hash 与现算一致才平。
    pub fn verify_hashes(&self) -> Result<(), u16> {
        for p in &self.pieces {
            if fnv1a64(&p.body) != p.hash {
                return Err(E_HASH);
            }
        }
        Ok(())
    }

    /// 缺件清单（审计失败的归因明细）。
    pub fn missing_list(&self) -> Vec<(u16, PieceKind)> {
        let mut out = Vec::new();
        for r in 1..=self.round_capacity {
            for k in [PieceKind::Report, PieceKind::Log, PieceKind::Ledger] {
                if self.piece_of(r, k).is_none() {
                    out.push((r, k));
                }
            }
        }
        out
    }

    /// 汇总行：轮数、件数、守恒判定。
    pub fn summary(&self) -> String {
        let rounds = self.pieces.len() as u32 / 3;
        let conserved = self.pieces.len() == rounds as usize * 3;
        format!(
            "归档簿: 轮数={} 件数={} 守恒={}",
            rounds,
            self.pieces.len(),
            if conserved { "平" } else { "不平" }
        )
    }
}

/// 件名确定性生成：R<轮>-<类码> 形式，零手抄。
fn make_name(round: u16, kind: PieceKind) -> String {
    let mut s = String::from("R");
    s.push_str(&itoa(round as u32));
    s.push('-');
    s.push((b'0' + kind.code()) as char);
    s
}

/// 简易整数转字符串（确定性，无浮点）。
fn itoa(mut v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut s = String::new();
    for b in &buf[i..] {
        s.push(*b as char);
    }
    s
}

/// 确定性产物字节生成：以（轮,类,序）派生，供自例程使用。
pub fn synth_body(round: u16, kind: PieceKind, salt: u32) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    let seed = (round as u32)
        .wrapping_mul(0x9E37_79B9)
        ^ ((kind.code() as u32).wrapping_mul(0x85EB_0001))
        ^ salt;
    let mut st = (seed as u64) | 1;
    for b in out.iter_mut() {
        // 64 位 LCG 标准参数（Knuth MMIX 乘子/增量），u32 域装不下故全程 u64。
        st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        *b = (st >> 33) as u8;
    }
    out
}

/// 自例程：10 轮归档 30 件零缺 + 重复/空件/缺件注入全检出 → 0。
pub fn drill() -> Result<u32, u16> {
    let mut book = ArchiveBook::new(10);
    for r in 1..=10u16 {
        book.archive_round(
            r,
            &synth_body(r, PieceKind::Report, 0xA1),
            &synth_body(r, PieceKind::Log, 0xB2),
            &synth_body(r, PieceKind::Ledger, 0xC3),
        )?;
    }
    // 守恒：30 件零缺
    if book.pieces.len() != 30 {
        return Err(E_MISSING);
    }
    book.audit()?;
    book.verify_hashes()?;
    // 注入 1：重复归档同（轮,类）→ 必须拦截
    let dup = book.archive_round(
        3,
        &synth_body(3, PieceKind::Report, 0xFF),
        &synth_body(3, PieceKind::Log, 0xFF),
        &synth_body(3, PieceKind::Ledger, 0xFF),
    );
    if dup != Err(E_DUP) {
        return Err(SEG_BASE + 15);
    }
    // 注入 2：空件 → 必须拦截
    let empty = book.archive_round(4, &[], &[], &[]);
    if empty != Err(E_EMPTY) {
        return Err(SEG_BASE + 16);
    }
    // 注入 3：构造缺件簿 → audit 必须点名
    let mut bad = ArchiveBook::new(10);
    for r in 1..=10u16 {
        if r == 7 {
            continue; // 缺第 7 轮三件
        }
        bad.archive_round(
            r,
            &synth_body(r, PieceKind::Report, 1),
            &synth_body(r, PieceKind::Log, 1),
            &synth_body(r, PieceKind::Ledger, 1),
        )?;
    }
    if bad.audit() != Err(E_MISSING) || bad.missing_list().len() != 3 {
        return Err(SEG_BASE + 17);
    }
    // 封账后写尝试 → 拦截
    let n = book.seal()?;
    if n != 30 {
        return Err(SEG_BASE + 18);
    }
    let late = book.archive_round(
        10,
        &synth_body(10, PieceKind::Report, 9),
        &synth_body(10, PieceKind::Log, 9),
        &synth_body(10, PieceKind::Ledger, 9),
    );
    if late != Err(E_SEALED) {
        return Err(SEG_BASE + 19);
    }
    Ok(0)
}
