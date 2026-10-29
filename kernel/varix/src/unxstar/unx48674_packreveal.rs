//! UNX-F48674 —— I34 异常呈现三要素（打包失败显性化）
//! 判据原文：UNX-F48674-J1 注入 ×10 呈现齐；判据锚定 体验十三·补。
//! 判据落地（drill 场景）：R1 三要素齐；R2 注入 ×10 全齐；R3 码域；
//! R4 文案非空；R5 重试位语义；R6 未知类别拒收；R7 码唯一；
//! R8 板计数守恒；R9 呈现行；R10 指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0BA0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_CODE: u16 = SEG_BASE + 1; // 码为 0 或越出段窗
const E_TEXT: u16 = SEG_BASE + 2; // 文案为空
const E_RETRY: u16 = SEG_BASE + 3; // 可重试位语义不符
const E_KIND: u16 = SEG_BASE + 4; // 失败类别越界
const E_BOARD: u16 = SEG_BASE + 5; // 呈现板计数不守恒
const E_DUPCODE: u16 = SEG_BASE + 6; // 呈现码重复
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_INJ: u16 = SEG_BASE + 9; // 注入轮次不足 10

/// 打包失败类别闭集。
pub const FK_FIELD: u8 = 1; // 缺字段（可重试：补字段重打包）
pub const FK_RANGE: u8 = 2; // 数值越域（可重试：修正数值）
pub const FK_CSUM: u8 = 3; // 校验和漂移（不可重试：账面已损）
/// 判据注入轮次。
pub const INJ_N: u32 = 10;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
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

/// 呈现三要素。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RevealTriple {
    pub code: u16,
    pub text: String,
    pub retryable: bool,
}

impl RevealTriple {
    /// 完整性：码在段窗内非零、文案非空。
    pub fn complete(&self) -> Result<(), u16> {
        if self.code == 0 || self.code <= SEG_BASE || self.code > SEG_BASE + 0x1F {
            return Err(E_CODE);
        }
        if self.text.is_empty() {
            return Err(E_TEXT);
        }
        Ok(())
    }

    /// 单例指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.code.to_le_bytes());
        buf.extend_from_slice(self.text.as_bytes());
        buf.push(self.retryable as u8);
        fnv1a64(&buf)
    }
}

/// 可重试位语义映射：缺字段/数值越域可重试，校验和漂移不可重试。
pub fn retryable_of(kind: u8) -> Result<bool, u16> {
    match kind {
        FK_FIELD => Ok(true),
        FK_RANGE => Ok(true),
        FK_CSUM => Ok(false),
        _ => Err(E_KIND),
    }
}

/// 文案模板：类别名 + 阶段号。
pub fn reveal_text(kind: u8, stage: u32) -> String {
    let base = match kind {
        FK_FIELD => "pack missing field",
        FK_RANGE => "pack value out of range",
        _ => "pack checksum drift",
    };
    let mut out = String::new();
    out.push_str(base);
    out.push_str(" stage=");
    out.push_str(&stage.to_string());
    out
}

/// 呈现板。
pub struct RevealBoard {
    pub items: Vec<RevealTriple>,
}

impl RevealBoard {
    pub fn new() -> RevealBoard {
        RevealBoard { items: vec![] }
    }

    /// 注入一笔失败并呈现。
    pub fn inject(&mut self, kind: u8, stage: u32) -> Result<(), u16> {
        let retryable = retryable_of(kind)?;
        let t = RevealTriple {
            code: SEG_BASE + 0x10 + kind as u16,
            text: reveal_text(kind, stage),
            retryable,
        };
        t.complete()?;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == t.code && self.items[i].text == t.text {
                return Err(E_DUPCODE);
            }
            i += 1;
        }
        self.items.push(t);
        Ok(())
    }

    /// 计数守恒。
    pub fn conserved(&self, registered: u32) -> bool {
        self.items.len() as u32 == registered
    }
}

/// 呈现行：tri=<条数> retry=<可重试数> fix=<不可重试数>。
pub fn board_line(b: &RevealBoard) -> String {
    let mut retry: u32 = 0;
    let mut fix: u32 = 0;
    let mut i = 0usize;
    while i < b.items.len() {
        if b.items[i].retryable {
            retry += 1;
        } else {
            fix += 1;
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str("tri=");
    out.push_str(&b.items.len().to_string());
    out.push_str(" retry=");
    out.push_str(&retry.to_string());
    out.push_str(" fix=");
    out.push_str(&fix.to_string());
    out
}

/// 全板指纹。
pub fn board_fp(b: &RevealBoard) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < b.items.len() {
        buf.extend_from_slice(&b.items[i].fp().to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 子串判定（朴素扫描）。
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三要素齐。
    let t0 = RevealTriple {
        code: SEG_BASE + 0x11,
        text: reveal_text(FK_CSUM, 1),
        retryable: false,
    };
    t0.complete()?;
    if t0.retryable {
        return Err(E_RETRY);
    }
    hits += 1;

    // R2 注入 ×10 全齐。
    let mut board = RevealBoard::new();
    let mut round: u32 = 0;
    while round < INJ_N {
        let kind = (1 + (round % 3)) as u8;
        board.inject(kind, round + 1)?;
        round += 1;
    }
    if board.items.len() as u32 != INJ_N {
        return Err(E_INJ);
    }
    hits += 1;

    // R3 码域：全部码在窗内非零。
    let mut i = 0usize;
    while i < board.items.len() {
        if board.items[i].code == 0 || board.items[i].code <= SEG_BASE {
            return Err(E_CODE);
        }
        i += 1;
    }
    hits += 1;

    // R4 文案非空且含关键词。
    let mut i = 0usize;
    while i < board.items.len() {
        if board.items[i].text.is_empty() {
            return Err(E_TEXT);
        }
        if !has_kw(board.items[i].text.as_bytes(), b"pack") {
            return Err(E_TEXT);
        }
        i += 1;
    }
    hits += 1;

    // R5 重试位语义映射全对。
    let mut i = 0usize;
    while i < board.items.len() {
        let k = board.items[i].code - SEG_BASE - 0x10;
        if board.items[i].retryable != retryable_of(k as u8)? {
            return Err(E_RETRY);
        }
        i += 1;
    }
    hits += 1;

    // R6 未知类别拒收。
    let mut probe = RevealBoard::new();
    match probe.inject(0, 1) {
        Err(E_KIND) => {}
        _ => return Err(E_KIND),
    }
    match probe.inject(8, 1) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_KIND),
    }

    // R7 码唯一：同类别同阶段重复呈现被拒。
    probe.inject(FK_FIELD, 5)?;
    match probe.inject(FK_FIELD, 5) {
        Err(E_DUPCODE) => hits += 1,
        _ => return Err(E_DUPCODE),
    }

    // R8 板计数守恒。
    if !board.conserved(INJ_N) || !probe.conserved(1) {
        return Err(E_BOARD);
    }
    hits += 1;

    // R9 呈现行：tri=10。
    let line = board_line(&board);
    if !has_kw(line.as_bytes(), b"tri=10") || !has_kw(line.as_bytes(), b"retry=") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 指纹稳定。
    let f1 = board_fp(&board);
    let f2 = board_fp(&board);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}
