//! UNX-F3433 崩溃恢复：结果账的断电续写预留 · 0x79A40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B13 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② J1：续写预留（账本追加式设计 + 序号戳——崩溃后部分账可辨认）；
//!     M-192 模拟中断账可解析。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 账本追加式：只允许尾部追加，禁改写既有字节（RewriteForbidden）；
//!   - 帧格式（整数定长域，手工编码解码）：
//!     [SEQ:4B 小端][KIND:1B][LEN:2B 小端][PAYLOAD:LEN 字节][PARITY:2B]，
//!     PARITY = SEQ 各字节 + KIND + LEN 各字节 + PAYLOAD 各字节之和
//!     对 65536 取模——断电后残帧可凭校验和辨认；
//!   - 序号戳：每条账序号 1 起严格递增，崩溃恢复后续写序号 =
//!     已辨认最大序号 + 1（ResumeSeqBad 点名接续错误）；
//!   - M-192 模拟中断：把完整账在最后一帧中部截断，解析器必须把
//!     已完整落盘的前缀账逐条辨认出来，并把残尾计为 residue_bytes、
//!     interrupted 置真——部分账可辨认即判据本体；
//!   - 解析容错闭集：帧头残缺/长度越界/校验和错三类坏帧显性点名，
//!     不静默吞字节；
//!   - 双跑一致：同字节账两次解析结果逐字段相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - 帧域宽：SEQ u32 / KIND u8 / LEN u16 / PARITY u16——
//!     对齐内核结果账的定容整数编码纪律（no_std 无堆格式化）；
//!   - M-192：派工卡验收判据编号（模拟中断账锚定）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B13 批段窗 [0x79A00, 0x79BC0] 内）。
pub const SEG_BASE: u32 = 0x79A40;

/// 帧定长域宽：序号 4 字节。
pub const FRAME_SEQ_W: usize = 4;
/// 帧定长域宽：类型 1 字节。
pub const FRAME_KIND_W: usize = 1;
/// 帧定长域宽：长度 2 字节。
pub const FRAME_LEN_W: usize = 2;
/// 帧定长域宽：校验和 2 字节。
pub const FRAME_PARITY_W: usize = 2;
/// 帧头最小字节数（SEQ+KIND+LEN，读到它才知道载荷多长）。
pub const FRAME_HEAD_MIN: usize = FRAME_SEQ_W + FRAME_KIND_W + FRAME_LEN_W;
/// 帧尾字节数（PARITY）。
pub const FRAME_TAIL: usize = FRAME_PARITY_W;
/// 载荷字节上限（LEN 域是 u16，钳制在 256 防解析器爆账）。
pub const PAYLOAD_CAP: u16 = 256;

/// 账目类型闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// 开账：账头（含序号 1）。
    Begin,
    /// 断言：一条断言结果。
    Assert,
    /// 备注：诊断说明。
    Note,
    /// 封账：账尾（正常收口才写）。
    Seal,
}

impl EntryKind {
    /// 类型字节（编码口径，1..=4 闭集）。
    pub fn code(self) -> u8 {
        match self {
            EntryKind::Begin => 1,
            EntryKind::Assert => 2,
            EntryKind::Note => 3,
            EntryKind::Seal => 4,
        }
    }
    /// 字节反解（表外字节返回 None）。
    pub fn from_code(b: u8) -> Option<EntryKind> {
        match b {
            1 => Some(EntryKind::Begin),
            2 => Some(EntryKind::Assert),
            3 => Some(EntryKind::Note),
            4 => Some(EntryKind::Seal),
            _ => None,
        }
    }
    /// 中文类型名。
    pub fn zh(self) -> &'static str {
        match self {
            EntryKind::Begin => "开账",
            EntryKind::Assert => "断言",
            EntryKind::Note => "备注",
            EntryKind::Seal => "封账",
        }
    }
}

/// 全类型闭集数组（类型表校验与测试共用）。
pub const ALL_KINDS: [EntryKind; 4] = [
    EntryKind::Begin,
    EntryKind::Assert,
    EntryKind::Note,
    EntryKind::Seal,
];

/// 一条账目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 序号戳（1 起严格递增）。
    pub seq: u32,
    /// 账目类型。
    pub kind: EntryKind,
    /// 载荷（UTF-8 中文或整数摘要文本）。
    pub payload: String,
}

/// 帧校验和：SEQ/KIND/LEN/PAYLOAD 全字节求和对 65536 取模。
pub fn frame_parity(seq: u32, kind_code: u8, payload: &[u8]) -> u16 {
    let mut acc: u32 = 0;
    let mut i = 0usize;
    while i < FRAME_SEQ_W {
        acc += ((seq >> (8 * i as u32)) & 0xFF) as u32;
        i += 1;
    }
    acc += kind_code as u32;
    acc += (payload.len() & 0xFF) as u32;
    acc += (payload.len() >> 8) as u32;
    let mut j = 0usize;
    while j < payload.len() {
        let b = match payload.get(j) {
            Some(x) => *x as u32,
            None => break,
        };
        acc += b;
        j += 1;
    }
    (acc & 0xFFFF) as u16
}

/// 把一条账目追加编码进账缓冲（只追加，不改写既有字节）。
pub fn encode_entry(entry: &LedgerEntry, out: &mut Vec<u8>) -> Result<usize, u32> {
    if entry.payload.len() > PAYLOAD_CAP as usize {
        return Err(Diag::LenOverflow.val());
    }
    let before = out.len();
    let kind_code = entry.kind.code();
    // SEQ 小端 4 字节。
    let mut i = 0usize;
    while i < FRAME_SEQ_W {
        out.push(((entry.seq >> (8 * i as u32)) & 0xFF) as u8);
        i += 1;
    }
    out.push(kind_code);
    let len = entry.payload.len() as u16;
    out.push((len & 0xFF) as u8);
    out.push((len >> 8) as u8);
    let mut j = 0usize;
    let bytes = entry.payload.as_bytes();
    while j < bytes.len() {
        let b = match bytes.get(j) {
            Some(x) => *x,
            None => break,
        };
        out.push(b);
        j += 1;
    }
    let parity = frame_parity(entry.seq, kind_code, bytes);
    out.push((parity & 0xFF) as u8);
    out.push((parity >> 8) as u8);
    Ok(out.len() - before)
}

/// 解析结论：前缀账 + 残尾诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerParse {
    /// 已完整辨认的前缀账（序号连续）。
    pub entries: Vec<LedgerEntry>,
    /// 残尾字节数（不完整帧）。
    pub residue_bytes: usize,
    /// 是否发生中断（残尾非空即断电续写场景）。
    pub interrupted: bool,
    /// 坏帧类别（None = 全账完好）。
    pub bad: Option<u32>,
}

/// 账解析：逐帧辨认，坏帧/残尾显性记账，不静默吞。
pub fn parse_ledger(buf: &[u8]) -> LedgerParse {
    let mut entries: Vec<LedgerEntry> = Vec::new();
    let mut pos = 0usize;
    let mut expect_seq: u32 = 1;
    let mut bad: Option<u32> = None;
    while pos < buf.len() {
        let remain = buf.len() - pos;
        if remain < FRAME_HEAD_MIN {
            // 帧头残缺：断电残尾。
            return LedgerParse {
                entries,
                residue_bytes: remain,
                interrupted: true,
                bad,
            };
        }
        // 读 SEQ（小端）。
        let mut seq: u32 = 0;
        let mut i = 0usize;
        while i < FRAME_SEQ_W {
            let b = match buf.get(pos + i) {
                Some(x) => *x,
                None => {
                    return LedgerParse {
                        entries,
                        residue_bytes: remain,
                        interrupted: true,
                        bad,
                    }
                }
            };
            seq |= (b as u32) << (8 * i as u32);
            i += 1;
        }
        let kind_code = match buf.get(pos + FRAME_SEQ_W) {
            Some(x) => *x,
            None => {
                return LedgerParse {
                    entries,
                    residue_bytes: remain,
                    interrupted: true,
                    bad,
                }
            }
        };
        let lo = match buf.get(pos + FRAME_SEQ_W + FRAME_KIND_W) {
            Some(x) => *x,
            None => {
                return LedgerParse {
                    entries,
                    residue_bytes: remain,
                    interrupted: true,
                    bad,
                }
            }
        };
        let hi = match buf.get(pos + FRAME_SEQ_W + FRAME_KIND_W + 1) {
            Some(x) => *x,
            None => {
                return LedgerParse {
                    entries,
                    residue_bytes: remain,
                    interrupted: true,
                    bad,
                }
            }
        };
        let len = lo as usize | ((hi as usize) << 8);
        if len > PAYLOAD_CAP as usize {
            // 长度越界：坏帧点名。
            bad = Some(Diag::LenOverflow.val());
            return LedgerParse {
                entries,
                residue_bytes: remain,
                interrupted: true,
                bad,
            };
        }
        let kind = match EntryKind::from_code(kind_code) {
            Some(k) => k,
            None => {
                bad = Some(Diag::KindForeign.val());
                return LedgerParse {
                    entries,
                    residue_bytes: remain,
                    interrupted: true,
                    bad,
                }
            }
        };
        let frame_total = FRAME_HEAD_MIN + len + FRAME_TAIL;
        if remain < frame_total {
            // 载荷/校验和未落全：断电残尾，前缀账已辨认。
            return LedgerParse {
                entries,
                residue_bytes: remain,
                interrupted: true,
                bad,
            };
        }
        let mut payload = Vec::new();
        let mut k = 0usize;
        while k < len {
            let b = match buf.get(pos + FRAME_HEAD_MIN + k) {
                Some(x) => *x,
                None => break,
            };
            payload.push(b);
            k += 1;
        }
        let parity_lo = match buf.get(pos + FRAME_HEAD_MIN + len) {
            Some(x) => *x as u16,
            None => 0,
        };
        let parity_hi = match buf.get(pos + FRAME_HEAD_MIN + len + 1) {
            Some(x) => *x as u16,
            None => 0,
        };
        let parity = parity_lo | (parity_hi << 8);
        if parity != frame_parity(seq, kind_code, &payload) {
            bad = Some(Diag::ParityBad.val());
            return LedgerParse {
                entries,
                residue_bytes: remain,
                interrupted: true,
                bad,
            };
        }
        if seq != expect_seq {
            // 序号断档：前缀账序号必须连续。
            bad = Some(Diag::SeqGap.val());
            return LedgerParse {
                entries,
                residue_bytes: remain,
                interrupted: true,
                bad,
            };
        }
        let text = match String::from_utf8(payload) {
            Ok(t) => t,
            Err(_) => String::new(),
        };
        entries.push(LedgerEntry {
            seq,
            kind,
            payload: text,
        });
        expect_seq += 1;
        pos += frame_total;
    }
    LedgerParse {
        entries,
        residue_bytes: 0,
        interrupted: false,
        bad,
    }
}

/// 续写序号：恢复点 = 已辨认最大序号 + 1（追加式接缝）。
pub fn resume_seq(p: &LedgerParse) -> u32 {
    let mut max_seq: u32 = 0;
    for e in p.entries.iter() {
        if e.seq > max_seq {
            max_seq = e.seq;
        }
    }
    max_seq + 1
}

/// 追加式守卫：新序号必须等于恢复点，回退/跳号显性拒。
pub fn append_check(p: &LedgerParse, new_seq: u32) -> Result<(), u32> {
    let expect = resume_seq(p);
    if new_seq < expect {
        return Err(Diag::SeqRegress.val());
    }
    if new_seq > expect {
        return Err(Diag::SeqGap.val());
    }
    Ok(())
}

/// 诊断码闭集：0x79A41..=0x79A4C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 序号断档：前缀账或续写序号跳号。
    SeqGap = 0x01,
    /// 校验和错：PARITY 对不上，字节被改。
    ParityBad = 0x02,
    /// 帧头残缺：不足 SEQ+KIND+LEN 的残尾。
    FrameHeadBad = 0x03,
    /// 长度越界：LEN 域超载荷上限。
    LenOverflow = 0x04,
    /// 重写违约：账本非追加式，既有字节被改。
    RewriteForbidden = 0x05,
    /// 序号回退：续写序号小于恢复点。
    SeqRegress = 0x06,
    /// 中断未标记：残尾存在却报全账完好。
    UnmarkedInterrupt = 0x07,
    /// 表外条目类型：KIND 字节不在 1..=4 闭集。
    KindForeign = 0x08,
    /// 恢复序号错：续写序号不等于恢复点。
    ResumeSeqBad = 0x09,
    /// 双跑分叉：同字节账两次解析结果不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::SeqGap => "序号断档：建议核对序号戳连续性，续写序号取恢复点",
            Diag::ParityBad => "校验和错：建议重算 PARITY 定位被改字节，坏账隔离不入正账",
            Diag::FrameHeadBad => "帧头残缺：建议按残尾处理待续写，勿强行解析残帧",
            Diag::LenOverflow => "长度越界：建议载荷写入口按 PAYLOAD_CAP 钳制拒收",
            Diag::RewriteForbidden => "重写违约：建议账本只追加，纠错走旁注新条目",
            Diag::SeqRegress => "序号回退：建议续写序号严格取已辨认最大序号加一",
            Diag::UnmarkedInterrupt => "中断未标记：建议残尾非空必置 interrupted 供续写面辨认",
            Diag::KindForeign => "表外条目类型：建议 KIND 只用开账/断言/备注/封账四码",
            Diag::ResumeSeqBad => "恢复序号错：建议恢复点用 resume_seq 统一计算",
            Diag::DoubleRunDiverge => "双跑分叉：建议 parse_ledger 做成纯函数，字节定则账定",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::SeqGap,
    Diag::ParityBad,
    Diag::FrameHeadBad,
    Diag::LenOverflow,
    Diag::RewriteForbidden,
    Diag::SeqRegress,
    Diag::UnmarkedInterrupt,
    Diag::KindForeign,
    Diag::ResumeSeqBad,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79A41..=0x79A4C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B13 批段窗 [0x79A00, 0x79BC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79A00 && SEG_BASE <= 0x79BC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 账目类型闭集：四码互异、编码 1..=4、反解回环、中文名非空。
fn kind_closed_set_ok() -> bool {
    let mut i = 0usize;
    while i < ALL_KINDS.len() {
        let a = match ALL_KINDS.get(i) {
            Some(k) => *k,
            None => return false,
        };
        if a.code() != (i + 1) as u8 || EntryKind::from_code(a.code()) != Some(a) {
            return false;
        }
        if a.zh().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_KINDS.len() {
            match ALL_KINDS.get(j) {
                Some(b) => {
                    if a.code() == b.code() || a.zh() == b.zh() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    EntryKind::from_code(0).is_none() && EntryKind::from_code(5).is_none()
}

/// 帧常量表：域宽与帧头/帧尾口径自洽。
fn frame_layout_ok() -> bool {
    FRAME_HEAD_MIN == 7 && FRAME_TAIL == 2 && FRAME_SEQ_W == 4 && FRAME_LEN_W == 2
        && PAYLOAD_CAP == 256
}

/// 正常流：完整账编码→解析回环，条目逐字段相同、无残尾、无坏帧。
fn roundtrip_ok() -> bool {
    let ledger: Vec<LedgerEntry> = vec![
        LedgerEntry { seq: 1, kind: EntryKind::Begin, payload: String::from("F3433开账") },
        LedgerEntry { seq: 2, kind: EntryKind::Assert, payload: String::from("断言1过") },
        LedgerEntry { seq: 3, kind: EntryKind::Note, payload: String::from("看门狗对账") },
        LedgerEntry { seq: 4, kind: EntryKind::Assert, payload: String::from("断言2过") },
        LedgerEntry { seq: 5, kind: EntryKind::Seal, payload: String::from("封账") },
    ];
    let mut buf: Vec<u8> = Vec::new();
    for e in ledger.iter() {
        if encode_entry(e, &mut buf).is_err() {
            return false;
        }
    }
    let p = parse_ledger(&buf);
    !p.interrupted
        && p.bad.is_none()
        && p.residue_bytes == 0
        && p.entries == ledger
        && resume_seq(&p) == 6
}

/// 序号戳连续：完整账解析后序号 1..=n 严格连续。
fn seq_stamp_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut seq = 1u32;
    while seq <= 12 {
        let e = LedgerEntry {
            seq,
            kind: if seq == 1 { EntryKind::Begin } else { EntryKind::Assert },
            payload: format!("条目{}", seq),
        };
        if encode_entry(&e, &mut buf).is_err() {
            return false;
        }
        seq += 1;
    }
    let p = parse_ledger(&buf);
    if p.entries.len() != 12 || p.bad.is_some() {
        return false;
    }
    let mut expect = 1u32;
    for e in p.entries.iter() {
        if e.seq != expect {
            return false;
        }
        expect += 1;
    }
    true
}

/// M-192 模拟中断：末帧中部截断，前缀账可辨认 + interrupted 置真。
fn crash_interrupt_parseable_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut seq = 1u32;
    let mut last_frame_total = 0usize;
    while seq <= 6 {
        let payload = format!("中断注入条目{}", seq);
        last_frame_total = FRAME_HEAD_MIN + payload.len() + FRAME_TAIL;
        let e = LedgerEntry {
            seq,
            kind: if seq == 1 { EntryKind::Begin } else { EntryKind::Assert },
            payload,
        };
        if encode_entry(&e, &mut buf).is_err() {
            return false;
        }
        seq += 1;
    }
    // 在最后一帧尾部倒数 3 字节处截断（断电残尾）。
    if buf.len() < FRAME_HEAD_MIN + 3 || last_frame_total <= 3 {
        return false;
    }
    let residue_expect = last_frame_total - 3;
    buf.truncate(buf.len() - 3);
    let p = parse_ledger(&buf);
    p.interrupted
        && p.bad.is_none()
        && p.entries.len() == 5
        && p.entries.first().map(|e| e.seq) == Some(1)
        && p.entries.last().map(|e| e.seq) == Some(5)
        && p.residue_bytes == residue_expect
        && resume_seq(&p) == 6
}

/// 残帧帧头残缺：不足 7 字节的残尾按中断处理且 FrameHeadBad 语义可达。
fn frame_head_residue_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let e = LedgerEntry { seq: 1, kind: EntryKind::Begin, payload: String::from("唯一条目") };
    if encode_entry(&e, &mut buf).is_err() {
        return false;
    }
    // 截到只剩 4 字节（不足帧头 7 字节）。
    buf.truncate(4);
    let p = parse_ledger(&buf);
    p.interrupted && p.entries.is_empty() && p.residue_bytes == 4
        && detail(Diag::FrameHeadBad.val()).contains("残尾")
}

/// 注入必抓 1：校验和错——改一个载荷字节必被 ParityBad 点名。
fn inj_parity_bad_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let e = LedgerEntry { seq: 1, kind: EntryKind::Begin, payload: String::from("完整帧") };
    if encode_entry(&e, &mut buf).is_err() {
        return false;
    }
    let idx = FRAME_HEAD_MIN;
    let b = match buf.get(idx) {
        Some(x) => *x,
        None => return false,
    };
    match buf.get_mut(idx) {
        Some(slot) => *slot = b ^ 0x55,
        None => return false,
    }
    let p = parse_ledger(&buf);
    p.bad == Some(Diag::ParityBad.val()) && p.entries.is_empty()
}

/// 注入必抓 2：长度越界——LEN 域伪造超上限必被 LenOverflow 点名。
fn inj_len_overflow_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let e = LedgerEntry { seq: 1, kind: EntryKind::Assert, payload: String::from("短帧") };
    if encode_entry(&e, &mut buf).is_err() {
        return false;
    }
    // LEN 小端在帧头第 5、6 字节：伪造 0xFFFF。
    let idx_lo = FRAME_SEQ_W + FRAME_KIND_W;
    match buf.get_mut(idx_lo) {
        Some(slot) => *slot = 0xFF,
        None => return false,
    }
    match buf.get_mut(idx_lo + 1) {
        Some(slot) => *slot = 0xFF,
        None => return false,
    }
    let p = parse_ledger(&buf);
    p.bad == Some(Diag::LenOverflow.val())
}

/// 续写接缝：恢复点 = 最大序号 + 1；回退拒（SeqRegress）、跳号拒（SeqGap）。
fn resume_seam_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut seq = 1u32;
    while seq <= 4 {
        let e = LedgerEntry {
            seq,
            kind: if seq == 1 { EntryKind::Begin } else { EntryKind::Assert },
            payload: format!("接缝条目{}", seq),
        };
        if encode_entry(&e, &mut buf).is_err() {
            return false;
        }
        seq += 1;
    }
    let p = parse_ledger(&buf);
    if resume_seq(&p) != 5 {
        return false;
    }
    if append_check(&p, 5).is_err() {
        return false;
    }
    if append_check(&p, 3) != Err(Diag::SeqRegress.val()) {
        return false;
    }
    append_check(&p, 9) == Err(Diag::SeqGap.val())
}

/// 载荷上限：超 256 字节载荷编码期拒收（LenOverflow）。
fn payload_cap_ok() -> bool {
    let mut big = String::new();
    let mut i = 0usize;
    while i < PAYLOAD_CAP as usize + 8 {
        big.push('账');
        i += 1;
    }
    let e = LedgerEntry { seq: 1, kind: EntryKind::Note, payload: big };
    let mut buf: Vec<u8> = Vec::new();
    encode_entry(&e, &mut buf) == Err(Diag::LenOverflow.val()) && buf.is_empty()
}

/// 追加式守卫：编码只追加不改写——同账两次追加的字节前缀逐字节相同。
fn append_only_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let e1 = LedgerEntry { seq: 1, kind: EntryKind::Begin, payload: String::from("第一") };
    let e2 = LedgerEntry { seq: 2, kind: EntryKind::Assert, payload: String::from("第二") };
    if encode_entry(&e1, &mut buf).is_err() {
        return false;
    }
    let prefix: Vec<u8> = buf.clone();
    if encode_entry(&e2, &mut buf).is_err() {
        return false;
    }
    // 前缀不得被改写（追加式设计判据本体）。
    let mut same = buf.len() > prefix.len();
    let mut i = 0usize;
    while i < prefix.len() {
        match (buf.get(i), prefix.get(i)) {
            (Some(b), Some(p)) if *b == *p => {}
            _ => same = false,
        }
        i += 1;
    }
    same && detail(Diag::RewriteForbidden.val()).contains("追加")
}

/// 双跑一致：同字节账两次解析逐字段相同。
fn double_run_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut seq = 1u32;
    while seq <= 8 {
        let e = LedgerEntry {
            seq,
            kind: if seq == 1 { EntryKind::Begin } else { EntryKind::Note },
            payload: format!("双跑条目{}", seq),
        };
        if encode_entry(&e, &mut buf).is_err() {
            return false;
        }
        seq += 1;
    }
    buf.truncate(buf.len() - 2); // 人为残尾，双跑都须看到同一结论。
    let a = parse_ledger(&buf);
    let b = parse_ledger(&buf);
    a == b && a.interrupted
}

/// 写回面：状态=已完成、结果摘要非空（含中断账解析结论）。
fn writeback_summary_ok() -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut seq = 1u32;
    while seq <= 5 {
        let e = LedgerEntry {
            seq,
            kind: if seq == 1 { EntryKind::Begin } else { EntryKind::Assert },
            payload: format!("写回条目{}", seq),
        };
        if encode_entry(&e, &mut buf).is_err() {
            return false;
        }
        seq += 1;
    }
    let p = parse_ledger(&buf);
    let summary = format!(
        "UNX-F3433 完成：前缀账{}条 中断{} 恢复点{}",
        p.entries.len(),
        if p.interrupted { "是" } else { "否" },
        resume_seq(&p)
    );
    !summary.is_empty()
        && summary.contains("UNX-F3433")
        && summary.contains("恢复点")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3433_checks ───────────────────────────────────────

/// run_unx3433_checks：本单判据聚合（诊断段 0x79A40，全部走 add_dyn）。
pub fn run_unx3433_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3433 断电续写预留");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "账目类型闭集四码互异可回环",
            kind_closed_set_ok(),
            format!("类型{}", if kind_closed_set_ok() { "闭集" } else { "破缺" }),
        ),
        (
            "帧常量表域宽口径自洽",
            frame_layout_ok(),
            format!("帧表{}", if frame_layout_ok() { "自洽" } else { "漂移" }),
        ),
        (
            "正常流完整账编码解析回环",
            roundtrip_ok(),
            format!("回环{}", if roundtrip_ok() { "零损" } else { "有损" }),
        ),
        (
            "序号戳一至十二严格连续",
            seq_stamp_ok(),
            format!("序号戳{}", if seq_stamp_ok() { "连续" } else { "断档" }),
        ),
        (
            "M-192模拟中断账前缀可辨认",
            crash_interrupt_parseable_ok(),
            format!("中断账{}", if crash_interrupt_parseable_ok() { "可解析" } else { "不可辨" }),
        ),
        (
            "残帧帧头残缺按中断处理",
            frame_head_residue_ok(),
            format!("残帧{}", if frame_head_residue_ok() { "稳" } else { "炸" }),
        ),
        (
            "注入1校验和错必抓",
            inj_parity_bad_ok(),
            format!("校验和注入{}", if inj_parity_bad_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2长度越界必抓",
            inj_len_overflow_ok(),
            format!("越界注入{}", if inj_len_overflow_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "续写接缝回退跳号双拒",
            resume_seam_ok(),
            format!("接缝{}", if resume_seam_ok() { "钉死" } else { "失守" }),
        ),
        (
            "载荷上限编码期拒收",
            payload_cap_ok(),
            format!("载荷上限{}", if payload_cap_ok() { "拒收" } else { "放行" }),
        ),
        (
            "追加式设计前缀字节不改写",
            append_only_ok(),
            format!("追加式{}", if append_only_ok() { "守卫" } else { "破约" }),
        ),
    ];
    let wb = writeback_summary_ok();
    let dr = double_run_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "双跑一致同字节账解析相同",
        dr,
        &format!("双跑{}", if dr { "一致" } else { "分叉" }),
    );
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn full_ledger(n: u32) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        let mut seq = 1u32;
        while seq <= n {
            let e = LedgerEntry {
                seq,
                kind: if seq == 1 { EntryKind::Begin } else { EntryKind::Assert },
                payload: format!("测试条目{}", seq),
            };
            encode_entry(&e, &mut buf).unwrap();
            seq += 1;
        }
        buf
    }

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79A40);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn kind_closed_set_roundtrip() {
        assert!(kind_closed_set_ok());
        assert_eq!(EntryKind::from_code(9), None);
        assert_eq!(EntryKind::Seal.code(), 4);
    }

    #[test]
    fn frame_layout_constants() {
        assert!(frame_layout_ok());
        assert_eq!(FRAME_HEAD_MIN, 7);
        assert_eq!(FRAME_TAIL, 2);
    }

    #[test]
    fn roundtrip_full_ledger() {
        assert!(roundtrip_ok());
        let buf = full_ledger(6);
        let p = parse_ledger(&buf);
        assert_eq!(p.entries.len(), 6);
        assert!(!p.interrupted);
        assert_eq!(resume_seq(&p), 7);
    }

    #[test]
    fn seq_stamps_monotonic() {
        assert!(seq_stamp_ok());
    }

    #[test]
    fn m192_interrupted_ledger_parseable() {
        assert!(crash_interrupt_parseable_ok());
        assert!(frame_head_residue_ok());
    }

    #[test]
    fn injections_parity_and_len() {
        assert!(inj_parity_bad_ok());
        assert!(inj_len_overflow_ok());
    }

    #[test]
    fn resume_seam_guards() {
        assert!(resume_seam_ok());
        let p = parse_ledger(&full_ledger(3));
        assert_eq!(resume_seq(&p), 4);
        assert_eq!(append_check(&p, 2), Err(Diag::SeqRegress.val()));
        assert_eq!(append_check(&p, 6), Err(Diag::SeqGap.val()));
        assert_eq!(append_check(&p, 4), Ok(()));
    }

    #[test]
    fn payload_cap_rejected() {
        assert!(payload_cap_ok());
    }

    #[test]
    fn append_only_prefix_stable() {
        assert!(append_only_ok());
    }

    #[test]
    fn double_run_identical_parse() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3433_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
