//! UNX-F48694 —— I35 异常呈现三要素（更新失败用户可见可重试）
//! 判据原文：UNX-F48694-J1 注入 ×10 呈现齐；判据锚定 体验十三·补。
//! 判据落地（drill 场景）：R1 四要素齐；R2 注入 ×10 全齐；R3 码域；
//! R4 文案非空且用户可读；R5 可见+可重试位语义；R6 未知类别拒收；
//! R7 码唯一；R8 板计数守恒；R9 呈现行；R10 指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：更新失败呈现四要素 = 码 + 用户可读文案 + 可见位 +
//! 可重试位；用户可见可重试是判据双锚，缺一即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0E20;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_CODE: u16 = SEG_BASE + 1; // 码为 0 或越出段窗
const E_TEXT: u16 = SEG_BASE + 2; // 文案为空或不可读
const E_VISIBLE: u16 = SEG_BASE + 3; // 可见位语义不符
const E_RETRY: u16 = SEG_BASE + 4; // 可重试位语义不符
const E_KIND: u16 = SEG_BASE + 5; // 失败类别越界
const E_BOARD: u16 = SEG_BASE + 6; // 呈现板计数不守恒
const E_DUPCODE: u16 = SEG_BASE + 7; // 呈现码重复
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_INJ: u16 = SEG_BASE + 10; // 注入轮次不足 10

/// 更新失败类别闭集。
pub const FK_SIGN: u8 = 1; // 签名失败（可见、不可重试：需新包）
pub const FK_SWAP: u8 = 2; // 换装失败（可见、可重试）
pub const FK_NET: u8 = 3; // 通道失败（可见、可重试）
pub const FK_DISK: u8 = 4; // 落盘失败（可见、可重试）
/// 注入轮次。
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

/// 呈现四要素。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RevealQuad {
    pub code: u16,
    pub text: String,
    pub visible: bool,
    pub retryable: bool,
}

impl RevealQuad {
    /// 完整性：码窗内非零、文案非空、可见位恒真（更新失败必须用户可见）。
    pub fn complete(&self) -> Result<(), u16> {
        if self.code == 0 || self.code <= SEG_BASE || self.code > SEG_BASE + 0x1F {
            return Err(E_CODE);
        }
        if self.text.is_empty() || self.text.len() < 8 {
            return Err(E_TEXT);
        }
        if !self.visible {
            return Err(E_VISIBLE);
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.code.to_le_bytes());
        buf.extend_from_slice(self.text.as_bytes());
        buf.push(self.visible as u8);
        buf.push(self.retryable as u8);
        fnv1a64(&buf)
    }
}

/// 可见+可重试语义映射：签名失败不可重试，其余可重试；可见全真。
pub fn semantics_of(kind: u8) -> Result<(bool, bool), u16> {
    match kind {
        FK_SIGN => Ok((true, false)),
        FK_SWAP => Ok((true, true)),
        FK_NET => Ok((true, true)),
        FK_DISK => Ok((true, true)),
        _ => Err(E_KIND),
    }
}

/// 用户可读文案模板。
pub fn reveal_text(kind: u8, stage: u32) -> String {
    let base = match kind {
        FK_SIGN => "update signature check failed",
        FK_SWAP => "update swap failed, retry available",
        FK_NET => "update channel unreachable, retry available",
        _ => "update write failed, retry available",
    };
    let mut out = String::new();
    out.push_str(base);
    out.push_str(" #");
    out.push_str(&stage.to_string());
    out
}

/// 呈现板。
pub struct RevealBoard {
    pub items: Vec<RevealQuad>,
}

impl RevealBoard {
    pub fn new() -> RevealBoard {
        RevealBoard { items: vec![] }
    }

    /// 注入一笔失败并呈现。
    pub fn inject(&mut self, kind: u8, stage: u32) -> Result<(), u16> {
        let (visible, retryable) = semantics_of(kind)?;
        let q = RevealQuad {
            code: SEG_BASE + 0x10 + kind as u16,
            text: reveal_text(kind, stage),
            visible,
            retryable,
        };
        q.complete()?;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == q.code && self.items[i].text == q.text {
                return Err(E_DUPCODE);
            }
            i += 1;
        }
        self.items.push(q);
        Ok(())
    }

    /// 计数守恒。
    pub fn conserved(&self, registered: u32) -> bool {
        self.items.len() as u32 == registered
    }
}

/// 呈现行：tri=<条数> retry=<可重试数> vis=<可见数>。
pub fn board_line(b: &RevealBoard) -> String {
    let mut retry: u32 = 0;
    let mut vis: u32 = 0;
    let mut i = 0usize;
    while i < b.items.len() {
        if b.items[i].retryable {
            retry += 1;
        }
        if b.items[i].visible {
            vis += 1;
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str("tri=");
    out.push_str(&b.items.len().to_string());
    out.push_str(" retry=");
    out.push_str(&retry.to_string());
    out.push_str(" vis=");
    out.push_str(&vis.to_string());
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

    // R1 四要素齐。
    let q0 = RevealQuad {
        code: SEG_BASE + 0x11,
        text: reveal_text(FK_SIGN, 1),
        visible: true,
        retryable: false,
    };
    q0.complete()?;
    if q0.retryable {
        return Err(E_RETRY);
    }
    hits += 1;

    // R2 注入 ×10 全齐。
    let mut board = RevealBoard::new();
    let mut round: u32 = 0;
    while round < INJ_N {
        let kind = (1 + (round % 4)) as u8;
        board.inject(kind, round + 1)?;
        round += 1;
    }
    if board.items.len() as u32 != INJ_N {
        return Err(E_INJ);
    }
    hits += 1;

    // R3 码域。
    let mut i = 0usize;
    while i < board.items.len() {
        if board.items[i].code == 0 || board.items[i].code <= SEG_BASE {
            return Err(E_CODE);
        }
        i += 1;
    }
    hits += 1;

    // R4 文案非空且含 retry/failed 可读关键词。
    let mut i = 0usize;
    while i < board.items.len() {
        let t = board.items[i].text.as_bytes();
        if !(has_kw(t, b"failed") || has_kw(t, b"retry")) {
            return Err(E_TEXT);
        }
        i += 1;
    }
    hits += 1;

    // R5 可见+可重试位语义：可见全真、映射全对。
    let mut i = 0usize;
    while i < board.items.len() {
        let k = board.items[i].code - SEG_BASE - 0x10;
        let (v, r) = semantics_of(k as u8)?;
        if board.items[i].visible != v || board.items[i].retryable != r {
            return Err(E_VISIBLE);
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
    match probe.inject(9, 1) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_KIND),
    }

    // R7 码唯一：同类别同阶段重复呈现被拒。
    probe.inject(FK_NET, 5)?;
    match probe.inject(FK_NET, 5) {
        Err(E_DUPCODE) => hits += 1,
        _ => return Err(E_DUPCODE),
    }

    // R8 板计数守恒。
    if !board.conserved(INJ_N) || !probe.conserved(1) {
        return Err(E_BOARD);
    }
    hits += 1;

    // R9 呈现行：tri=10 vis=10。
    let line = board_line(&board);
    if !has_kw(line.as_bytes(), b"tri=10") || !has_kw(line.as_bytes(), b"vis=10") {
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
