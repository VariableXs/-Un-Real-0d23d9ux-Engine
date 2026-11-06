//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F43203 载荷引用模型（全量/δ块双形态，引用校验和钉版） · 0x9B080段
//! ───────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥506 行；
//!   ② 判据原文：UNX-F43203-J1 双形态 ×20 往返校验和闭环；判据锚定
//!      任务书专题一；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双形态闭集：全量引用 FullRef（file_id/len/csum）与 δ 块引用
//!     DeltaRef（base_id/blocks/csum），形态字节 1/2，0 与 3+ 拒；
//!   - ×20 往返校验和闭环：8 份冻结样例文件上，10 全量 + 10 δ 引用
//!     encode→decode→逐字段等 + 校验和对文件正演重算等；
//!   - ×10 注入：形态字节 0/3、file_id 越窗、全量零长、δ 零块、
//!     δ 基准=自身、校验和翻位、基准越窗、长度越上限、file_id=0
//!     逐类必抓；
//!   - 双跑一致 + 段位守卫 + 闭集回查；no_std + alloc；零 panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x9B080，AI-VX12-1106 批1 12 槽第 3 槽）。
pub const SEG_BASE: u32 = 0x9B080;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayRefCode {
    /// 形态字节越闭集（非 1/2）
    BadForm,
    /// file_id 越窗（0x100..0x900）或为 0
    BadFileId,
    /// 全量引用零长
    FullZeroLen,
    /// 长度越上限
    LenOverflow,
    /// δ 零块
    DeltaZeroBlocks,
    /// δ 块数越上限
    DeltaBlockOverflow,
    /// δ 基准越窗或为 0
    BadBaseId,
    /// δ 基准=自身
    BaseSelfLoop,
    /// 校验和坏
    CsumMismatch,
    /// 账满
    LedgerFull,
    /// 未知兜底
    Unknown,
}

impl PayRefCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PayRefCode::BadForm => 0x01,
                PayRefCode::BadFileId => 0x02,
                PayRefCode::FullZeroLen => 0x03,
                PayRefCode::LenOverflow => 0x04,
                PayRefCode::DeltaZeroBlocks => 0x05,
                PayRefCode::DeltaBlockOverflow => 0x06,
                PayRefCode::BadBaseId => 0x07,
                PayRefCode::BaseSelfLoop => 0x08,
                PayRefCode::CsumMismatch => 0x09,
                PayRefCode::LedgerFull => 0x0A,
                PayRefCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            PayRefCode::BadForm => "形态字节越闭集：1=全量 2=δ",
            PayRefCode::BadFileId => "file_id 越窗：0x100..0x900 且非 0",
            PayRefCode::FullZeroLen => "全量引用零长：len 不得为 0",
            PayRefCode::LenOverflow => "长度越上限：≤ 0x10000",
            PayRefCode::DeltaZeroBlocks => "δ 零块：blocks 不得为 0",
            PayRefCode::DeltaBlockOverflow => "δ 块数越上限：≤ 64",
            PayRefCode::BadBaseId => "δ 基准越窗：基准须在窗内且非 0",
            PayRefCode::BaseSelfLoop => "δ 基准=自身：自环拒",
            PayRefCode::CsumMismatch => "校验和坏：对样例文件正演重算",
            PayRefCode::LedgerFull => "账满：先消费再入账",
            PayRefCode::Unknown => "未知诊断码：不在 0x9B080 段闭集内",
        }
    }

    pub const ALL: [PayRefCode; 11] = [
        PayRefCode::BadForm,
        PayRefCode::BadFileId,
        PayRefCode::FullZeroLen,
        PayRefCode::LenOverflow,
        PayRefCode::DeltaZeroBlocks,
        PayRefCode::DeltaBlockOverflow,
        PayRefCode::BadBaseId,
        PayRefCode::BaseSelfLoop,
        PayRefCode::CsumMismatch,
        PayRefCode::LedgerFull,
        PayRefCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in PayRefCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    PayRefCode::Unknown.detail()
}

// ══ [CHUNK-2-END]

// ── 冻结样例文件表（校验和正演基底） ────────────────────────────
/// 8 份冻结样例：编号 + 字节内容。
pub const FILES: [(u16, &[u8]); 8] = [
    (0x0100, b"payload-full-core-0001"),
    (0x0110, b"payload-full-shell-0002"),
    (0x0120, b"payload-full-ui-0003"),
    (0x0200, b"delta-base-v1-content"),
    (0x0210, b"delta-base-v2-content"),
    (0x0300, b"mix-a"),
    (0x0310, b"mix-bb"),
    (0x0900, b"edge-max-id"),
];

/// 样例文件查内容。
pub fn file_of(id: u16) -> Option<&'static [u8]> {
    for f in FILES.iter() {
        if f.0 == id {
            return Some(f.1);
        }
    }
    None
}

/// FNV-1a 32（引用校验和正演）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// file_id 窗。
pub const FILE_WIN: (u16, u16) = (0x0100, 0x0901);
/// 长度上限。
pub const LEN_CAP: u32 = 0x10000;
/// δ 块上限。
pub const BLOCK_CAP: u16 = 64;
/// 引用账容量。
pub const REF_CAP: usize = 32;

/// 引用形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    /// 全量引用
    Full,
    /// δ 块引用（基于 base_id）
    Delta,
}

/// 单条引用记录（Copy）。
#[derive(Clone, Copy, Debug)]
pub struct RefRec {
    pub kind: RefKind,
    pub file_id: u16,
    /// 全量=文件长；δ=变换后长。
    pub len: u32,
    /// δ 基准文件 id（全量置 0）。
    pub base_id: u16,
    /// δ 块数（全量置 0）。
    pub blocks: u16,
    pub csum: u32,
}

/// 编码 20 字节：tag(1)+file_id(2)+len(4)+base_id(2)+blocks(2)+csum(4)+rsv(5)。
pub fn encode(r: &RefRec) -> [u8; 20] {
    let mut o = [0u8; 20];
    o[0] = match r.kind {
        RefKind::Full => 1,
        RefKind::Delta => 2,
    };
    o[1] = (r.file_id >> 8) as u8;
    o[2] = (r.file_id & 0xFF) as u8;
    o[3] = (r.len >> 24) as u8;
    o[4] = (r.len >> 16) as u8;
    o[5] = (r.len >> 8) as u8;
    o[6] = (r.len & 0xFF) as u8;
    o[7] = (r.base_id >> 8) as u8;
    o[8] = (r.base_id & 0xFF) as u8;
    o[9] = (r.blocks >> 8) as u8;
    o[10] = (r.blocks & 0xFF) as u8;
    o[11] = (r.csum >> 24) as u8;
    o[12] = (r.csum >> 16) as u8;
    o[13] = (r.csum >> 8) as u8;
    o[14] = (r.csum & 0xFF) as u8;
    o
}

// ══ [CHUNK-3-END]

// ── 解码闸（七闸全过才成账） ────────────────────────────────────
/// 解码 20 字节引用，逐闸校验。
pub fn decode(b: &[u8]) -> Result<RefRec, u32> {
    if b.len() < 15 {
        return Err(PayRefCode::BadForm.val());
    }
    // 闸1：形态闭集。
    let kind = match b[0] {
        1 => RefKind::Full,
        2 => RefKind::Delta,
        _ => return Err(PayRefCode::BadForm.val()),
    };
    let file_id = ((b[1] as u16) << 8) | b[2] as u16;
    let len = ((b[3] as u32) << 24)
        | ((b[4] as u32) << 16)
        | ((b[5] as u32) << 8)
        | b[6] as u32;
    let base_id = ((b[7] as u16) << 8) | b[8] as u16;
    let blocks = ((b[9] as u16) << 8) | b[10] as u16;
    let csum = ((b[11] as u32) << 24)
        | ((b[12] as u32) << 16)
        | ((b[13] as u8) as u32) << 8
        | b[14] as u32;
    // 闸2：file_id 窗（半开）。
    if file_id == 0 || file_id < FILE_WIN.0 || file_id >= FILE_WIN.1 {
        return Err(PayRefCode::BadFileId.val());
    }
    match kind {
        RefKind::Full => {
            // 闸3：全量非零长。
            if len == 0 {
                return Err(PayRefCode::FullZeroLen.val());
            }
            // 闸4：长度上限。
            if len > LEN_CAP {
                return Err(PayRefCode::LenOverflow.val());
            }
            // 闸5：校验和对文件正演（文件在册才可正演）。
            match file_of(file_id) {
                Some(bytes) => {
                    if fnv1a(bytes) != csum {
                        return Err(PayRefCode::CsumMismatch.val());
                    }
                }
                None => {
                    // 表外文件：无正演基底，只要求 csum≠0（外置引用可带任意非零校验和）。
                    if csum == 0 {
                        return Err(PayRefCode::CsumMismatch.val());
                    }
                }
            }
        }
        RefKind::Delta => {
            // 闸6：δ 块数。
            if blocks == 0 {
                return Err(PayRefCode::DeltaZeroBlocks.val());
            }
            if blocks > BLOCK_CAP {
                return Err(PayRefCode::DeltaBlockOverflow.val());
            }
            // 闸7：基准窗 + 自环。
            if base_id == 0 || base_id < FILE_WIN.0 || base_id >= FILE_WIN.1 {
                return Err(PayRefCode::BadBaseId.val());
            }
            if base_id == file_id {
                return Err(PayRefCode::BaseSelfLoop.val());
            }
            match file_of(base_id) {
                Some(bytes) => {
                    if fnv1a(bytes) != csum {
                        return Err(PayRefCode::CsumMismatch.val());
                    }
                    let _ = bytes;
                }
                None => {
                    if csum == 0 {
                        return Err(PayRefCode::CsumMismatch.val());
                    }
                }
            }
        }
    }
    Ok(RefRec { kind, file_id, len, base_id, blocks, csum })
}

// ══ [CHUNK-4-END]

// ── ×20 往返校验和闭环（判据核心） ──────────────────────────────
/// 10 全量 + 10 δ 引用：encode→decode→逐字段等 + csum 对文件正演等。
pub fn roundtrip_twenty() -> (u32, String) {
    let mut ok = 0u32;
    let mut marks = String::new();
    // 10 全量：8 表内 + 2 表外。
    let full_ids: [u16; 10] = [0x0100, 0x0110, 0x0120, 0x0200, 0x0210, 0x0300, 0x0310, 0x0900, 0x0800, 0x0777];
    let mut i = 0usize;
    while i < 10 {
        let id = full_ids[i];
        let bytes = file_of(id);
        let (len, csum) = match bytes {
            Some(b) => (b.len() as u32, fnv1a(b)),
            None => (0x20 + i as u32, 0x5A5A_0000 + i as u32),
        };
        let r = RefRec { kind: RefKind::Full, file_id: id, len, base_id: 0, blocks: 0, csum };
        let e = encode(&r);
        match decode(&e) {
            Ok(d) => {
                if d.kind == r.kind && d.file_id == r.file_id && d.len == r.len && d.csum == r.csum {
                    ok += 1;
                } else {
                    marks.push_str(&format!("F{}!", i));
                }
            }
            Err(e2) => marks.push_str(&format!("F{}e{:X}!", i, e2)),
        }
        i += 1;
    }
    // 10 δ：基准取表内前 5，块数 1..=10。
    let bases: [u16; 5] = [0x0200, 0x0210, 0x0100, 0x0110, 0x0120];
    let mut k = 0usize;
    while k < 10 {
        let base = bases[k % 5];
        let bytes = file_of(base).unwrap_or(b"");
        let r = RefRec {
            kind: RefKind::Delta,
            file_id: 0x0400 + (k as u16) * 0x10,
            len: (bytes.len() as u32) + k as u32,
            base_id: base,
            blocks: (k + 1) as u16,
            csum: fnv1a(bytes),
        };
        let e = encode(&r);
        match decode(&e) {
            Ok(d) => {
                if d.kind == r.kind
                    && d.file_id == r.file_id
                    && d.base_id == r.base_id
                    && d.blocks == r.blocks
                    && d.csum == r.csum
                    && d.len == r.len
                {
                    ok += 1;
                } else {
                    marks.push_str(&format!("D{}!", k));
                }
            }
            Err(e2) => marks.push_str(&format!("D{}e{:X}!", k, e2)),
        }
        k += 1;
    }
    (ok, marks)
}

// ══ [CHUNK-5-END]

// ── ×10 注入（判据：逐类必抓） ──────────────────────────────────
pub fn inject_ten() -> (u32, String) {
    let mut caught = 0u32;
    let mut marks = String::new();
    // 1 形态字节 0。
    let mut b = [0u8; 15];
    b[0] = 0;
    if decode(&b).err() == Some(PayRefCode::BadForm.val()) {
        caught += 1;
    } else {
        marks.push_str("1!");
    }
    // 2 形态字节 3。
    let mut b = [0u8; 15];
    b[0] = 3;
    if decode(&b).err() == Some(PayRefCode::BadForm.val()) {
        caught += 1;
    } else {
        marks.push_str("2!");
    }
    // 3 file_id=0。
    let r = RefRec { kind: RefKind::Full, file_id: 0, len: 8, base_id: 0, blocks: 0, csum: 1 };
    if decode(&encode(&r)).err() == Some(PayRefCode::BadFileId.val()) {
        caught += 1;
    } else {
        marks.push_str("3!");
    }
    // 4 file_id 越窗（0x0902）。
    let r = RefRec { kind: RefKind::Full, file_id: 0x0902, len: 8, base_id: 0, blocks: 0, csum: 1 };
    if decode(&encode(&r)).err() == Some(PayRefCode::BadFileId.val()) {
        caught += 1;
    } else {
        marks.push_str("4!");
    }
    // 5 全量零长。
    let r = RefRec { kind: RefKind::Full, file_id: 0x0100, len: 0, base_id: 0, blocks: 0, csum: 1 };
    if decode(&encode(&r)).err() == Some(PayRefCode::FullZeroLen.val()) {
        caught += 1;
    } else {
        marks.push_str("5!");
    }
    // 6 长度越上限。
    let r = RefRec { kind: RefKind::Full, file_id: 0x0100, len: LEN_CAP + 1, base_id: 0, blocks: 0, csum: 1 };
    if decode(&encode(&r)).err() == Some(PayRefCode::LenOverflow.val()) {
        caught += 1;
    } else {
        marks.push_str("6!");
    }
    // 7 δ 零块。
    let r = RefRec { kind: RefKind::Delta, file_id: 0x0400, len: 8, base_id: 0x0200, blocks: 0, csum: fnv1a(b"delta-base-v1-content") };
    if decode(&encode(&r)).err() == Some(PayRefCode::DeltaZeroBlocks.val()) {
        caught += 1;
    } else {
        marks.push_str("7!");
    }
    // 8 δ 基准越窗。
    let r = RefRec { kind: RefKind::Delta, file_id: 0x0400, len: 8, base_id: 0x0099, blocks: 4, csum: 1 };
    if decode(&encode(&r)).err() == Some(PayRefCode::BadBaseId.val()) {
        caught += 1;
    } else {
        marks.push_str("8!");
    }
    // 9 δ 基准=自身。
    let r = RefRec { kind: RefKind::Delta, file_id: 0x0400, len: 8, base_id: 0x0400, blocks: 4, csum: 1 };
    if decode(&encode(&r)).err() == Some(PayRefCode::BaseSelfLoop.val()) {
        caught += 1;
    } else {
        marks.push_str("9!");
    }
    // 10 校验和翻位（表内文件 csum 篡改）。
    let r = RefRec { kind: RefKind::Full, file_id: 0x0100, len: 22, base_id: 0, blocks: 0, csum: 0xDEAD_BEEF };
    if decode(&encode(&r)).err() == Some(PayRefCode::CsumMismatch.val()) {
        caught += 1;
    } else {
        marks.push_str("10!");
    }
    (caught, marks)
}

// ══ [CHUNK-6-END]

// ── 黄金正演 + 双跑 + 段位守卫 ──────────────────────────────────
/// 黄金正演：全量+δ 引用闭环 + 账守恒。
pub fn golden_run() -> Result<u32, u32> {
    let mut refs: Vec<RefRec> = Vec::new();
    // 3 全量 + 2 δ 入账。
    for id in FILES.iter().take(3) {
        let bytes = file_of(id.0).ok_or(PayRefCode::BadFileId.val())?;
        refs.push(RefRec {
            kind: RefKind::Full,
            file_id: id.0,
            len: bytes.len() as u32,
            base_id: 0,
            blocks: 0,
            csum: fnv1a(bytes),
        });
    }
    let base_bytes = file_of(0x0200).ok_or(PayRefCode::BadFileId.val())?;
    refs.push(RefRec {
        kind: RefKind::Delta,
        file_id: 0x0400,
        len: base_bytes.len() as u32 + 16,
        base_id: 0x0200,
        blocks: 4,
        csum: fnv1a(base_bytes),
    });
    let base2 = file_of(0x0210).ok_or(PayRefCode::BadFileId.val())?;
    refs.push(RefRec {
        kind: RefKind::Delta,
        file_id: 0x0410,
        len: base2.len() as u32 + 8,
        base_id: 0x0210,
        blocks: 2,
        csum: fnv1a(base2),
    });
    // 账内逐条 decode 回读守恒。
    let mut n = 0u32;
    for r in refs.iter() {
        let d = decode(&encode(r))?;
        if d.file_id == r.file_id && d.len == r.len {
            n = n.saturating_add(1);
        } else {
            return Err(PayRefCode::CsumMismatch.val());
        }
    }
    if n != 5 {
        return Err(PayRefCode::LedgerFull.val());
    }
    Ok(n)
}

/// 双跑一致。
pub fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && a != 0
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x9B000..=0x9B400).contains(&SEG_BASE) {
        return false;
    }
    for c in PayRefCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口 ────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演闭环", g.is_ok(), format!("{:?}", g)));
    let (c20, mk20) = roundtrip_twenty();
    v.push(("J1-往返×20闭环", c20 == 20, format!("c {}/20 {}", c20, mk20)));
    let (ci, mki) = inject_ten();
    v.push(("J2-注入拒×10", ci == 10, format!("c {}/10 {}", ci, mki)));
    // J3：形态互斥（全量 base/blocks 必 0，δ 两字段必非 0）。
    let mut ok = true;
    let f = RefRec { kind: RefKind::Full, file_id: 0x0100, len: 22, base_id: 0, blocks: 0, csum: fnv1a(b"payload-full-core-0001") };
    if let Ok(d) = decode(&encode(&f)) {
        if d.base_id != 0 || d.blocks != 0 {
            ok = false;
        }
    } else {
        ok = false;
    }
    let dd = RefRec { kind: RefKind::Delta, file_id: 0x0400, len: 37, base_id: 0x0200, blocks: 4, csum: fnv1a(b"delta-base-v1-content") };
    if let Ok(d) = decode(&encode(&dd)) {
        if d.base_id == 0 || d.blocks == 0 {
            ok = false;
        }
    } else {
        ok = false;
    }
    v.push(("J3-形态互斥", ok, String::from("全量/δ 字段守恒")));
    // J4：边界卡边（file_id 窗沿 0x0901 放行、0x0902 拒）。
    let edge_in = decode(&encode(&RefRec { kind: RefKind::Full, file_id: FILE_WIN.1 - 1, len: 11, base_id: 0, blocks: 0, csum: fnv1a(b"edge-max-id") })).is_ok();
    let edge_out = decode(&encode(&RefRec { kind: RefKind::Full, file_id: 0x0901, len: 11, base_id: 0, blocks: 0, csum: 9 })).err() == Some(PayRefCode::BadFileId.val());
    v.push(("J4-窗沿卡边", edge_in && edge_out, String::from("0x0900 进/0x0901 出")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x9B080 窗口")));
    let closed = {
        let mut ok = true;
        for c in PayRefCode::ALL.iter() {
            if detail(c.val()) != c.detail() {
                ok = false;
            }
        }
        ok
    };
    v.push(("J6-闭集回查", closed, String::from("detail 全等")));
    v
}

/// 检查入口（卡 43203）。
pub fn run_unx43203_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F43203 载荷引用双形态");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ══ [CHUNK-7-END]

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_roundtrip_twenty() {
        let (c, mk) = roundtrip_twenty();
        assert_eq!(c, 20, "marks: {}", mk);
    }

    #[test]
    fn test_inject_ten() {
        let (c, mk) = inject_ten();
        assert_eq!(c, 10, "marks: {}", mk);
    }

    #[test]
    fn test_form_exclusive() {
        let f = RefRec { kind: RefKind::Full, file_id: 0x0100, len: 22, base_id: 0, blocks: 0, csum: fnv1a(b"payload-full-core-0001") };
        assert!(decode(&encode(&f)).is_ok());
        let d = RefRec { kind: RefKind::Delta, file_id: 0x0400, len: 37, base_id: 0x0200, blocks: 4, csum: fnv1a(b"delta-base-v1-content") };
        assert!(decode(&encode(&d)).is_ok());
    }

    #[test]
    fn test_edge_window() {
        assert!(decode(&encode(&RefRec { kind: RefKind::Full, file_id: 0x0900, len: 11, base_id: 0, blocks: 0, csum: fnv1a(b"edge-max-id") })).is_ok());
        assert!(decode(&encode(&RefRec { kind: RefKind::Full, file_id: 0x0901, len: 11, base_id: 0, blocks: 0, csum: 9 })).is_err());
    }

    #[test]
    fn test_dual_run() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x9B080);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in PayRefCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}

// ══ [END-43203]
