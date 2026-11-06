//! UNX-F11654 版本节损坏（偏移越界）拒收判据
//!
//! `.gnu.version_r` 是一段**自指**结构：Verneed 头里的 `vn_aux` 指向自己
//! 后面 16 字节的 Vernaux，Vernaux 的 `vna_next` 又指下一条，链尾置 0；
//! `vn_file` / `vna_name` 则是**字符串表偏移**，不是节内偏移。四类偏移
//! （节内链偏移、字符串表偏移、链长、节长）任何一处越界，装载器都必须
//! 在**碰任何符号之前**拒收，否则后续版本校验会拿越界指针去读内存。
//!
//! 本卡的中心是 B22 联签分界（co-sign boundary）：
//!
//! - 结构侧（本卡）：节长、版本号、计数、aux 偏移、串偏移、串终止、链长、
//!   链尾、哈希、旗标、other 索引。任一条坏 → **拒载**，且拒载发生在
//!   `sem` 阶段之前。B22 签名落在"结构闸最后一格"与"语义闸第一格"之间。
//! - 语义侧（F11653/F11655）：结构健康后才有资格谈"版本节点在不在"。
//!   结构侧已经判死的输入，语义侧**一个字节都不该读**；反过来，结构健康
//!   但版本缺失的输入必须越过 B22 交给语义侧。
//!
//! 这条分界不是学术洁癖。真实故障里最常见的误判是：把 `vn_aux` 越界报成
//! ``version `GLIBC_2.34' not found``——文案对不上现场，排障方向被带偏；
//! 或者更糟：越界被当成"没有版本需求"而放行，装载器带着坏链继续跑。
//!
//! 结构闸的次序不可倒：
//!
//! 1. 节长：不足 16 字节连头都读不全（`file too short`）。
//! 2. 版本号：`vn_version != 1` 的节是**另一代格式**，不能用本代解析器读。
//! 3. 计数：0 或超上限的 `vn_cnt` 让 aux 链长度无意义。
//! 4. aux 偏移：节内偏移必须 4 字节对齐且 `off+16 <= blob_len`；这一格
//!    同时覆盖"偏移为 0"与"偏移越过节尾"。
//! 5. 串偏移：`vn_file` 与 `vna_name` 都对字符串表，必须 `< strtab_len`。
//! 6. 串终止：名字串必须在表内遇到 NUL；越界前没遇到就是坏串。
//! 7. 链长与链尾：aux 实际条数必须等于 `vn_cnt`，最后一条 `vna_next`
//!    必须为 0；中间断链与链尾不封口都算坏。
//! 8. 哈希：`vna_hash` 必须等于版本串的 ELF 哈希（本卡按严格模式判）。
//! 9. 旗标与索引：旗标只允许 0 / VER_FLG_WEAK；`vna_other` 非 0 且不重复。
//! 10. 语义：结构全绿后才轮到版本节点是否命中。
//!
//! 判据的六个维度：阶段、errno、越界量（off）、注解、B22 签名
//! （b22-reject/b22-pass）、状态。少 off 这一维，"越界"与"链断"只能靠
//! 注解猜；少 B22 这一维，结构拒载与语义失败会被混成一档。
//!
//! 判序：状态 → 阶段 → errno → 越界量 → 注解 → B22。
//!
//! 语料 14 槽：8 槽申报正确（健康 2、越界 3、链坏 2、语义 1），6 槽各挂
//! 一处错误申报（状态/阶段/码/越界量/注解/B22）。账本 14 格 8 成 6 败，
//! 冻结后重放同指纹。
//!
//! 闸门次序：节长 → 版本号 → 计数 → aux 偏移 → 串偏移 → 串终止 → 链长
//! → 链尾 → 哈希 → 旗标 → other → 语义 → 就绪。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11654）。
pub const SEG_BASE: u32 = 0x6ce70;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数。
pub const TIER_COUNT: usize = 4;

/// Verneed 头字节数。
pub const VERNEED_SZ: u32 = 16;

/// Vernaux 字节数。
pub const VERNAUX_SZ: u32 = 16;

/// 版本节上限（本模型）。
pub const BLOB_MAX: u32 = 0x200;

/// need 条目上限。
pub const NEED_MAX: u16 = 4;

/// 弱旗标。
pub const VER_FLG_WEAK: u16 = 0x2;

/// 字符串表上限。
pub const STR_MAX: u32 = 0x100;

/// Verneed 头字段布局（名字, 偏移, 宽度）。
pub const VERNEED_FIELDS: [(&str, u32, u32); 5] = [
    ("vn_version", 0, 2),
    ("vn_cnt", 2, 2),
    ("vn_file", 4, 4),
    ("vn_aux", 8, 4),
    ("vn_next", 12, 4),
];

/// Vernaux 字段布局（名字, 偏移, 宽度）。
pub const VERNAUX_FIELDS: [(&str, u32, u32); 5] = [
    ("vna_hash", 0, 4),
    ("vna_flags", 4, 2),
    ("vna_other", 6, 2),
    ("vna_name", 8, 4),
    ("vna_next", 12, 4),
];

// ---------------------------------------------------------------------------
// errno
// ---------------------------------------------------------------------------

/// 文件不存在。
pub const ERR_ENOENT: i32 = 2;
/// 结构不可执行。
pub const ERR_ENOEXEC: i32 = 8;
/// 偏移不可访问。
pub const ERR_EFAULT: i32 = 14;
/// 参数非法。
pub const ERR_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const VN_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const VN_ECODE: u32 = SEG_BASE + 0x02;
/// 越界量申报不符。
pub const VN_EOFF: u32 = SEG_BASE + 0x03;
/// 注解申报不符。
pub const VN_ENOTE: u32 = SEG_BASE + 0x04;
/// B22 签名申报不符。
pub const VN_ECROSS: u32 = SEG_BASE + 0x05;
/// 状态矛盾。
pub const VN_ESTATE: u32 = SEG_BASE + 0x06;
/// 计数不符。
pub const VN_ECOUNT: u32 = SEG_BASE + 0x07;
/// 槽位非法。
pub const VN_ESLOT: u32 = SEG_BASE + 0x08;
/// 指纹不符。
pub const VN_ESEAL: u32 = SEG_BASE + 0x09;
/// 无观测。
pub const VN_ENOBS: u32 = SEG_BASE + 0x0a;
/// 矩阵不平。
pub const VN_EMATRIX: u32 = SEG_BASE + 0x0b;
/// 阶段次序倒置。
pub const VN_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const VN_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 节长闸。
pub const OP_LEN: u8 = 0x01;
/// 头闸。
pub const OP_HEAD: u8 = 0x02;
/// 偏移闸。
pub const OP_OFF: u8 = 0x03;
/// 链闸。
pub const OP_CHAIN: u8 = 0x04;
/// 哈希闸。
pub const OP_HASH: u8 = 0x05;
/// 语义闸。
pub const OP_SEM: u8 = 0x06;
/// B22 分界。
pub const OP_B22: u8 = 0x07;
/// 语料。
pub const OP_CORPUS: u8 = 0x08;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut shift = 56u32;
    loop {
        v = fold_byte(v, (((w >> shift) & 0xff) as u32) as u8);
        if shift == 0 {
            break;
        }
        shift -= 8;
    }
    v
}

/// 折入一个字符串。
fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut v = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    v
}

/// ELF 版本串哈希（`_dl_elf_hash`）。
pub fn elf_hash(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut h = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = h.rotate_left(4).wrapping_add(*b as u32);
            let g = h & 0xf000_0000;
            if g != 0 {
                h ^= g >> 24;
            }
            h &= !g;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 定宽小写十六进制文本。
fn hex_text(v: u32, digits: usize) -> String {
    let mut s = String::new();
    let mut n = digits;
    while n > 0 {
        n -= 1;
        let shift = (n as u32) * 4;
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
    }
    s
}

/// 有符号十进制文本。
fn i32_text(v: i32) -> String {
    if v == 0 {
        return String::from("0");
    }
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text(v.unsigned_abs()).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// u32 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 偏移文本（`0x` + 8 位）。
pub fn off_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 8).as_str());
    s
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 节被截断。
pub fn msg_short(obj: &str, len: u32, req: &str) -> String {
    let mut s = String::from("version section of ");
    s.push_str(obj);
    s.push_str(" truncated at ");
    s.push_str(u32_text(len).as_str());
    s.push_str(" bytes (required by ");
    s.push_str(req);
    s.push(')');
    s
}

/// 版本号不支持。
pub fn msg_bad_version(obj: &str, ver: u16, req: &str) -> String {
    let mut s = String::from("version section of ");
    s.push_str(obj);
    s.push_str(": unsupported version ");
    s.push_str(u32_text(ver as u32).as_str());
    s.push_str(" (required by ");
    s.push_str(req);
    s.push(')');
    s
}

/// 偏移越界。
pub fn msg_oob(obj: &str, what: &str, off: u32, req: &str) -> String {
    let mut s = String::from("version section of ");
    s.push_str(obj);
    s.push_str(": ");
    s.push_str(what);
    s.push_str(" offset ");
    s.push_str(off_text(off).as_str());
    s.push_str(" out of bounds (required by ");
    s.push_str(req);
    s.push(')');
    s
}

/// 链长不符。
pub fn msg_chain(obj: &str, actual: u32, cnt: u32, req: &str) -> String {
    let mut s = String::from("version section of ");
    s.push_str(obj);
    s.push_str(": aux chain length ");
    s.push_str(u32_text(actual).as_str());
    s.push_str(" != cnt ");
    s.push_str(u32_text(cnt).as_str());
    s.push_str(" (required by ");
    s.push_str(req);
    s.push(')');
    s
}

/// 哈希不符。
pub fn msg_hash(obj: &str, ver: &str, req: &str) -> String {
    let mut s = String::from("version section of ");
    s.push_str(obj);
    s.push_str(": hash mismatch for `");
    s.push_str(ver);
    s.push_str("' (required by ");
    s.push_str(req);
    s.push(')');
    s
}

/// 语义缺失。
pub fn msg_sem_missing(ver: &str, req: &str) -> String {
    let mut s = String::from("version `");
    s.push_str(ver);
    s.push_str("' not found (required by ");
    s.push_str(req);
    s.push(')');
    s
}

// ---------------------------------------------------------------------------
// 阶段与注解
// ---------------------------------------------------------------------------

/// 结构闸阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 节长。
    Len,
    /// 版本号。
    Version,
    /// 计数。
    Count,
    /// aux 偏移。
    Aux,
    /// 字符串偏移。
    Str,
    /// 串终止。
    Nul,
    /// 链长/链尾。
    Chain,
    /// 哈希。
    Hash,
    /// 旗标与索引。
    Flags,
    /// 语义。
    Sem,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Len => 0,
            Stage::Version => 1,
            Stage::Count => 2,
            Stage::Aux => 3,
            Stage::Str => 4,
            Stage::Nul => 5,
            Stage::Chain => 6,
            Stage::Hash => 7,
            Stage::Flags => 8,
            Stage::Sem => 9,
            Stage::Ready => 10,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Len => "len",
            Stage::Version => "version",
            Stage::Count => "count",
            Stage::Aux => "aux",
            Stage::Str => "str",
            Stage::Nul => "nul",
            Stage::Chain => "chain",
            Stage::Hash => "hash",
            Stage::Flags => "flags",
            Stage::Sem => "sem",
            Stage::Ready => "ready",
        }
    }

    /// 是否结构侧（B22 之前）。
    pub fn structural(&self) -> bool {
        self.order() < Stage::Sem.order()
    }
}

/// 本次判定的注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 健康。
    Healthy,
    /// 弱版本需求。
    WeakPass,
    /// 节短。
    Short,
    /// 版本号坏。
    BadVersion,
    /// 计数坏。
    BadCount,
    /// aux 偏移越界。
    BadAuxOff,
    /// 文件串偏移越界。
    BadFileOff,
    /// 名字串偏移越界。
    BadNameOff,
    /// 串未终止。
    Unterminated,
    /// 链长/链尾坏。
    ChainBroken,
    /// 哈希坏。
    BadHash,
    /// 旗标/索引坏。
    BadFlags,
    /// 语义未命中。
    SemMiss,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Healthy => "healthy",
            Note::WeakPass => "weak-pass",
            Note::Short => "short",
            Note::BadVersion => "bad-version",
            Note::BadCount => "bad-count",
            Note::BadAuxOff => "bad-aux-off",
            Note::BadFileOff => "bad-file-off",
            Note::BadNameOff => "bad-name-off",
            Note::Unterminated => "unterminated",
            Note::ChainBroken => "chain-broken",
            Note::BadHash => "bad-hash",
            Note::BadFlags => "bad-flags",
            Note::SemMiss => "sem-miss",
        }
    }

    /// 是否结构侧注解。
    pub fn structural(&self) -> bool {
        match self {
            Note::Healthy | Note::WeakPass | Note::SemMiss => false,
            _ => true,
        }
    }

    /// 是否偏移类。
    pub fn offset_class(&self) -> bool {
        match self {
            Note::BadAuxOff | Note::BadFileOff | Note::BadNameOff => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次结构判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 走到（或死在）的阶段。
    pub stage: Stage,
    /// errno（成功为 0）。
    pub code: i32,
    /// 越界量 / 链长 / 哈希。
    pub off: u32,
    /// 注解。
    pub note: Note,
}

impl Outcome {
    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否结构拒载。
    pub fn reject(&self) -> bool {
        self.failed() && self.stage.structural()
    }

    /// 是否越过 B22。
    pub fn crossed(&self) -> bool {
        !self.reject()
    }

    /// 偏移文本。
    pub fn off_hex(&self) -> String {
        off_text(self.off)
    }
}

/// B22 联签签名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crossing {
    /// 结构侧拒载。
    Reject,
    /// 越过分界，交给语义侧。
    Pass,
}

impl Crossing {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Crossing::Reject => "b22-reject",
            Crossing::Pass => "b22-pass",
        }
    }

    /// 是否拒载。
    pub fn rejected(&self) -> bool {
        match self {
            Crossing::Reject => true,
            Crossing::Pass => false,
        }
    }
}

/// B22 分界签名：结构闸最后一格与语义闸第一格之间的切换。
pub fn crossing(s: &Shape) -> Crossing {
    if inspect(s).reject() {
        Crossing::Reject
    } else {
        Crossing::Pass
    }
}

// ---------------------------------------------------------------------------
// 形状
// ---------------------------------------------------------------------------

/// 一段版本节的形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 被检查的对象（版本节属主）。
    pub obj: &'static str,
    /// 提出需求的对象。
    pub req: &'static str,
    /// 依赖名。
    pub dep: &'static str,
    /// 实际读到的节字节数。
    pub blob_len: u32,
    /// Verneed 头：版本号。
    pub vn_version: u16,
    /// Verneed 头：aux 条数。
    pub vn_cnt: u16,
    /// Verneed 头：文件串偏移。
    pub vn_file: u32,
    /// Verneed 头：首条 aux 偏移。
    pub vn_aux: u32,
    /// Verneed 头：下一条 Verneed 偏移。
    pub vn_next: u32,
    /// 首条 aux：版本串偏移。
    pub aux_name: u32,
    /// 首条 aux：下一条 aux 偏移。
    pub aux_next: u32,
    /// 首条 aux：版本哈希。
    pub aux_hash: u32,
    /// 首条 aux：旗标。
    pub aux_flags: u16,
    /// 首条 aux：版本索引。
    pub aux_other: u16,
    /// 字符串表长度。
    pub strtab_len: u32,
    /// 版本串是否在表内以 NUL 结束。
    pub trailing_nul: bool,
    /// aux 链实际条数（0 表示与 cnt 相同）。
    pub extra_aux: u16,
    /// 哈希是否与版本串一致。
    pub hash_ok: bool,
    /// 版本索引是否唯一非零。
    pub other_ok: bool,
    /// 结构健康后语义侧是否命中。
    pub semantic_ok: bool,
    /// 需要的版本名（文案用）。
    pub need_ver: &'static str,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            obj: "",
            req: "",
            dep: "",
            blob_len: 0,
            vn_version: 0,
            vn_cnt: 0,
            vn_file: 0,
            vn_aux: 0,
            vn_next: 0,
            aux_name: 0,
            aux_next: 0,
            aux_hash: 0,
            aux_flags: 0,
            aux_other: 0,
            strtab_len: 0,
            trailing_nul: false,
            extra_aux: 0,
            hash_ok: false,
            other_ok: false,
            semantic_ok: false,
            need_ver: "",
        }
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        inspect(self)
    }

    /// 名字指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.obj);
        h = fold_str(h, self.dep);
        h = fold_word(h, self.blob_len as u64);
        h = fold_word(h, self.vn_aux as u64);
        h = fold_word(h, self.aux_name as u64);
        h = fold_word(h, self.vn_cnt as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 结构判定
// ---------------------------------------------------------------------------

/// 结构闸：节长 → 版本号 → 计数 → aux 偏移 → 串偏移 → 串终止 → 链长
/// → 链尾 → 哈希 → 旗标 → other → 语义 → 就绪。
pub fn inspect(s: &Shape) -> Outcome {
    if s.blob_len < VERNEED_SZ {
        return Outcome {
            stage: Stage::Len,
            code: ERR_ENOEXEC,
            off: s.blob_len,
            note: Note::Short,
        };
    }
    if s.vn_version != 1 {
        return Outcome {
            stage: Stage::Version,
            code: ERR_ENOEXEC,
            off: s.vn_version as u32,
            note: Note::BadVersion,
        };
    }
    if s.vn_cnt == 0 || s.vn_cnt > NEED_MAX {
        return Outcome {
            stage: Stage::Count,
            code: ERR_EINVAL,
            off: s.vn_cnt as u32,
            note: Note::BadCount,
        };
    }
    if s.vn_aux < VERNEED_SZ || (s.vn_aux & 3) != 0 {
        return Outcome {
            stage: Stage::Aux,
            code: ERR_EFAULT,
            off: s.vn_aux,
            note: Note::BadAuxOff,
        };
    }
    let aux_end = match s.vn_aux.checked_add(VERNAUX_SZ) {
        Some(v) => v,
        None => {
            return Outcome {
                stage: Stage::Aux,
                code: ERR_EFAULT,
                off: s.vn_aux,
                note: Note::BadAuxOff,
            }
        }
    };
    if aux_end > s.blob_len {
        return Outcome {
            stage: Stage::Aux,
            code: ERR_EFAULT,
            off: s.vn_aux,
            note: Note::BadAuxOff,
        };
    }
    if s.vn_file >= s.strtab_len {
        return Outcome {
            stage: Stage::Str,
            code: ERR_EFAULT,
            off: s.vn_file,
            note: Note::BadFileOff,
        };
    }
    if s.aux_name >= s.strtab_len {
        return Outcome {
            stage: Stage::Str,
            code: ERR_EFAULT,
            off: s.aux_name,
            note: Note::BadNameOff,
        };
    }
    if !s.trailing_nul {
        return Outcome {
            stage: Stage::Nul,
            code: ERR_EINVAL,
            off: s.aux_name,
            note: Note::Unterminated,
        };
    }
    let actual = if s.extra_aux == 0 { s.vn_cnt as u32 } else { s.extra_aux as u32 };
    if actual != s.vn_cnt as u32 {
        return Outcome {
            stage: Stage::Chain,
            code: ERR_EINVAL,
            off: actual,
            note: Note::ChainBroken,
        };
    }
    if s.vn_next != 0 {
        return Outcome {
            stage: Stage::Chain,
            code: ERR_EINVAL,
            off: s.vn_next,
            note: Note::ChainBroken,
        };
    }
    if s.vn_cnt == 1 && s.aux_next != 0 {
        return Outcome {
            stage: Stage::Chain,
            code: ERR_EINVAL,
            off: s.aux_next,
            note: Note::ChainBroken,
        };
    }
    if s.vn_cnt > 1 && s.aux_next == 0 {
        return Outcome {
            stage: Stage::Chain,
            code: ERR_EINVAL,
            off: 0,
            note: Note::ChainBroken,
        };
    }
    if !s.hash_ok {
        return Outcome {
            stage: Stage::Hash,
            code: ERR_EINVAL,
            off: s.aux_hash,
            note: Note::BadHash,
        };
    }
    if s.aux_flags != 0 && s.aux_flags != VER_FLG_WEAK {
        return Outcome {
            stage: Stage::Flags,
            code: ERR_EINVAL,
            off: s.aux_flags as u32,
            note: Note::BadFlags,
        };
    }
    if !s.other_ok || s.aux_other == 0 {
        return Outcome {
            stage: Stage::Flags,
            code: ERR_EINVAL,
            off: s.aux_other as u32,
            note: Note::BadFlags,
        };
    }
    if !s.semantic_ok {
        return Outcome {
            stage: Stage::Sem,
            code: ERR_EINVAL,
            off: 0,
            note: Note::SemMiss,
        };
    }
    let note = if s.aux_flags == VER_FLG_WEAK { Note::WeakPass } else { Note::Healthy };
    Outcome { stage: Stage::Ready, code: 0, off: 0, note }
}

/// 单行文案。
pub fn message(s: &Shape) -> String {
    let o = inspect(s);
    match o.note {
        Note::Short => msg_short(s.dep, s.blob_len, s.req),
        Note::BadVersion => msg_bad_version(s.dep, s.vn_version, s.req),
        Note::BadCount => {
            let mut t = String::from("version section of ");
            t.push_str(s.dep);
            t.push_str(": bad vn_cnt ");
            t.push_str(u32_text(s.vn_cnt as u32).as_str());
            t.push_str(" (required by ");
            t.push_str(s.req);
            t.push(')');
            t
        }
        Note::BadAuxOff => msg_oob(s.dep, "vn_aux", s.vn_aux, s.req),
        Note::BadFileOff => msg_oob(s.dep, "vn_file", s.vn_file, s.req),
        Note::BadNameOff => msg_oob(s.dep, "vna_name", s.aux_name, s.req),
        Note::Unterminated => {
            let mut t = String::from("version section of ");
            t.push_str(s.dep);
            t.push_str(": unterminated string at ");
            t.push_str(off_text(s.aux_name).as_str());
            t.push_str(" (required by ");
            t.push_str(s.req);
            t.push(')');
            t
        }
        Note::ChainBroken => {
            let actual = if s.extra_aux == 0 { s.vn_cnt } else { s.extra_aux };
            msg_chain(s.dep, actual as u32, s.vn_cnt as u32, s.req)
        }
        Note::BadHash => msg_hash(s.dep, s.need_ver, s.req),
        Note::BadFlags => {
            let mut t = String::from("version section of ");
            t.push_str(s.dep);
            t.push_str(": bad vna_flags ");
            t.push_str(off_text(s.aux_flags as u32).as_str());
            t.push_str(" (required by ");
            t.push_str(s.req);
            t.push(')');
            t
        }
        Note::SemMiss => msg_sem_missing(s.need_ver, s.req),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 判据档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 健康档。
    Ready,
    /// 偏移越界档。
    Oob,
    /// 链坏档。
    Chain,
    /// 语义档。
    Sem,
}

impl Tier {
    /// 由结果归位。
    pub fn of(o: &Outcome) -> Tier {
        if o.ok() {
            return Tier::Ready;
        }
        if o.note.offset_class() || o.note == Note::Short {
            return Tier::Oob;
        }
        if o.stage == Stage::Chain {
            return Tier::Chain;
        }
        if o.stage == Stage::Sem {
            return Tier::Sem;
        }
        // 其余结构失败（版本号/计数/串终止/哈希/旗标）并入链坏档：
        // 它们同属"结构签名不一致"，与"偏移越界"分档。
        Tier::Chain
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Ready => 0,
            Tier::Oob => 1,
            Tier::Chain => 2,
            Tier::Sem => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Ready => "ready",
            Tier::Oob => "oob",
            Tier::Chain => "chain",
            Tier::Sem => "sem",
        }
    }
}

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 申报与实际比对得到的缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// errno 错。
    Code,
    /// 越界量错。
    Off,
    /// 注解错。
    Note,
    /// B22 签名错。
    Cross,
    /// 成功/失败互报。
    Estate,
    /// 计数错。
    Count,
    /// 槽位错。
    Slot,
    /// 指纹错。
    Seal,
    /// 无观测。
    Nobs,
    /// 矩阵不平。
    Matrix,
    /// 次序倒置。
    Order,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 缺陷码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Stage => VN_ESTAGE,
            Fault::Code => VN_ECODE,
            Fault::Off => VN_EOFF,
            Fault::Note => VN_ENOTE,
            Fault::Cross => VN_ECROSS,
            Fault::Estate => VN_ESTATE,
            Fault::Count => VN_ECOUNT,
            Fault::Slot => VN_ESLOT,
            Fault::Seal => VN_ESEAL,
            Fault::Nobs => VN_ENOBS,
            Fault::Matrix => VN_EMATRIX,
            Fault::Order => VN_EORDER,
            Fault::Bal => VN_EBAL,
        }
    }

    /// 是否非绿。
    pub fn alive(&self) -> bool {
        match self {
            Fault::Ok => false,
            _ => true,
        }
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Off => "off",
        Fault::Note => "note",
        Fault::Cross => "cross",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Order => "order",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 申报
// ---------------------------------------------------------------------------

/// 一条申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报阶段。
    pub stage: Stage,
    /// 申报 errno。
    pub code: i32,
    /// 申报越界量。
    pub off: u32,
    /// 申报注解。
    pub note: Note,
    /// 申报 B22 签名。
    pub cross: Crossing,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Len,
            code: 0,
            off: 0,
            note: Note::Healthy,
            cross: Crossing::Pass,
        }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }
}

/// 一个语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 空白槽。
    pub const fn blank() -> Case {
        Case { shape: Shape::blank(), claim: Claim::blank() }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        self.shape.tag
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        self.shape.actual()
    }

    /// 对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 是否一致。
    pub fn ok(&self) -> bool {
        judge(self) == Fault::Ok
    }
}

// ---------------------------------------------------------------------------
// 语料（14 槽）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的形状 + 6 个复刻形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "healthy-single",
        obj: "libdep.so",
        req: "./a.so",
        dep: "libdep.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "healthy-weak",
        obj: "libdep.so",
        req: "./b.so",
        dep: "libdep.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: VER_FLG_WEAK,
        aux_other: 4,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_4",
    },
    Shape {
        tag: "oob-aux",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x100,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "oob-name",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x200,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "oob-file",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x88,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "chain-short",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 2,
        vn_file: 0x01,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0x20,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 1,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "chain-run",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0x20,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "sem-miss",
        obj: "libdep.so",
        req: "./a.so",
        dep: "libdep.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: false,
        need_ver: "VER_9",
    },
    Shape {
        tag: "oob-as-pass",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x100,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "oob-stage-swap",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x100,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "oob-code-swap",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x100,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "oob-off-swap",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x100,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "chain-note-swap",
        obj: "libbad.so",
        req: "./a.so",
        dep: "libbad.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 2,
        vn_file: 0x01,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0x20,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 1,
        hash_ok: true,
        other_ok: true,
        semantic_ok: true,
        need_ver: "VER_3",
    },
    Shape {
        tag: "sem-cross-swap",
        obj: "libdep.so",
        req: "./a.so",
        dep: "libdep.so",
        blob_len: 0x40,
        vn_version: 1,
        vn_cnt: 1,
        vn_file: 0x01,
        vn_aux: 0x10,
        vn_next: 0,
        aux_name: 0x0c,
        aux_next: 0,
        aux_hash: 0x0a3f_2d1b,
        aux_flags: 0,
        aux_other: 3,
        strtab_len: 0x30,
        trailing_nul: true,
        extra_aux: 0,
        hash_ok: true,
        other_ok: true,
        semantic_ok: false,
        need_ver: "VER_9",
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Ready, code: 0, off: 0, note: Note::Healthy, cross: Crossing::Pass },
    Claim { stage: Stage::Ready, code: 0, off: 0, note: Note::WeakPass, cross: Crossing::Pass },
    Claim { stage: Stage::Aux, code: ERR_EFAULT, off: 0x100, note: Note::BadAuxOff, cross: Crossing::Reject },
    Claim { stage: Stage::Str, code: ERR_EFAULT, off: 0x200, note: Note::BadNameOff, cross: Crossing::Reject },
    Claim { stage: Stage::Str, code: ERR_EFAULT, off: 0x88, note: Note::BadFileOff, cross: Crossing::Reject },
    Claim { stage: Stage::Chain, code: ERR_EINVAL, off: 1, note: Note::ChainBroken, cross: Crossing::Reject },
    Claim { stage: Stage::Chain, code: ERR_EINVAL, off: 0x20, note: Note::ChainBroken, cross: Crossing::Reject },
    Claim { stage: Stage::Sem, code: ERR_EINVAL, off: 0, note: Note::SemMiss, cross: Crossing::Pass },
    Claim { stage: Stage::Ready, code: 0, off: 0, note: Note::Healthy, cross: Crossing::Pass },
    Claim { stage: Stage::Chain, code: ERR_EFAULT, off: 0x100, note: Note::BadAuxOff, cross: Crossing::Reject },
    Claim { stage: Stage::Aux, code: ERR_EINVAL, off: 0x100, note: Note::BadAuxOff, cross: Crossing::Reject },
    Claim { stage: Stage::Aux, code: ERR_EFAULT, off: 0x80, note: Note::BadAuxOff, cross: Crossing::Reject },
    Claim { stage: Stage::Chain, code: ERR_EINVAL, off: 1, note: Note::BadCount, cross: Crossing::Reject },
    Claim { stage: Stage::Sem, code: ERR_EINVAL, off: 0, note: Note::SemMiss, cross: Crossing::Reject },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Off, Fault::Note, Fault::Cross];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    VN_ESTATE,
    VN_ESTAGE,
    VN_ECODE,
    VN_EOFF,
    VN_ENOTE,
    VN_ECROSS,
];

/// 账本 8 个成功格的值（总数、成、败、健康、越界、链坏、语义、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 3, 2, 1, 929];

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            out[i] = Case { shape: *s, claim: *c };
        }
        i += 1;
    }
    out
}

/// 取第 i 槽。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    cases().get(i).copied()
}

/// 覆盖校验。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(VN_ENOBS)
    }
}

/// 第 i 槽的设计期望。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        Some(Fault::Ok)
    } else {
        REJECT_FAULTS.get(i - ACCEPT).copied()
    }
}

/// 对判：状态 → 阶段 → errno → 越界量 → 注解 → B22。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let claim = c.claim;
    if claim.success() != a.ok() {
        return Fault::Estate;
    }
    if claim.stage != a.stage {
        return Fault::Stage;
    }
    if claim.code != a.code {
        return Fault::Code;
    }
    if claim.off != a.off {
        return Fault::Off;
    }
    if claim.note != a.note {
        return Fault::Note;
    }
    if claim.cross != crossing(&c.shape) {
        return Fault::Cross;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 释义
// ---------------------------------------------------------------------------

/// 单行释义。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(c.shape.obj);
    s.push_str(" dep=");
    s.push_str(c.shape.dep);
    s.push_str(" len=");
    s.push_str(u32_text(c.shape.blob_len).as_str());
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(a.code).as_str());
    s.push_str(" off=");
    s.push_str(a.off_hex().as_str());
    s.push_str(" note=");
    s.push_str(a.note.name());
    s.push_str(" cross=");
    s.push_str(crossing(&c.shape).name());
    let m = message(&c.shape);
    if !m.is_empty() {
        s.push_str(" msg=\"");
        s.push_str(m.as_str());
        s.push('"');
    }
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 矩阵
// ---------------------------------------------------------------------------

/// 一行统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档槽数。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
}

impl Row {
    /// 全对。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }

    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }
}

/// 四档矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（ready/oob/chain/sem）。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix { rows: [Row::blank(); TIER_COUNT] }
    }

    /// 由语料统计。
    pub fn of() -> Matrix {
        let mut m = Matrix::blank();
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                let t = Tier::of(&c.actual()).index();
                if t < TIER_COUNT {
                    let mut row = match m.rows.get(t) {
                        Some(r) => *r,
                        None => Row::blank(),
                    };
                    row.total += 1;
                    if judge(c) == Fault::Ok {
                        row.good += 1;
                    }
                    m.rows[t] = row;
                }
            }
            i += 1;
        }
        m
    }

    /// 总槽数。
    pub fn total(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.total;
            }
            i += 1;
        }
        n
    }

    /// 总正确数。
    pub fn good(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.good;
            }
            i += 1;
        }
        n
    }

    /// 是否平。
    pub fn balanced(&self) -> bool {
        self.total() == CORPUS && self.good() == ACCEPT
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        self.rows.get(i).copied()
    }

    /// 全对行数。
    pub fn perfect_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.perfect() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.total as u64);
                h = fold_word(h, r.good as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 健康档正确数。
    pub ready: u32,
    /// 越界档正确数。
    pub oob: u32,
    /// 链坏档正确数。
    pub chain: u32,
    /// 语义档正确数。
    pub sem: u32,
    /// 对判命中数。
    pub matched: u32,
}

impl Report {
    /// 千分表示（四舍五入）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(VN_ECOUNT)
        }
    }

    /// B22 拒载槽数。
    pub fn b22_rejects(&self) -> u32 {
        let cs = cases();
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                if crossing(&c.shape).rejected() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("verneed-oob corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" ready=");
        s.push_str(u32_text(self.ready).as_str());
        s.push_str(" oob=");
        s.push_str(u32_text(self.oob).as_str());
        s.push_str(" chain=");
        s.push_str(u32_text(self.chain).as_str());
        s.push_str(" sem=");
        s.push_str(u32_text(self.sem).as_str());
        s.push_str(" b22=");
        s.push_str(u32_text(self.b22_rejects()).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(VN_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut ready = 0u32;
    let mut oob = 0u32;
    let mut chain = 0u32;
    let mut sem = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(VN_ENOBS),
        };
        let a = c.actual();
        let mut f = judge(&c);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let want = match expected(i) {
            Some(v) => v,
            None => return Err(VN_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Ready => ready += 1,
                Tier::Oob => oob += 1,
                Tier::Chain => chain += 1,
                Tier::Sem => sem += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, ready, oob, chain, sem, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    last: i32,
    steps: u32,
    closed: bool,
}

impl Gate {
    /// 新闸门。
    pub const fn new() -> Gate {
        Gate { last: -1, steps: 0, closed: false }
    }

    /// 走一段。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            return Err(VN_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(VN_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(VN_ESTATE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 上一段。
    pub const fn last(&self) -> i32 {
        self.last
    }

    /// 是否已关。
    pub const fn closed(&self) -> bool {
        self.closed
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(VN_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(VN_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(VN_EBAL),
            };
            if s.slot as usize != i {
                return Err(VN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(VN_EOFF);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(VN_EOFF);
                        }
                    }
                    None => return Err(VN_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(VN_EOFF);
                        }
                    }
                    None => return Err(VN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(VN_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(VN_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(VN_ESEAL)
        }
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// `Result<u32, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_B22, r.map(|x| x.ready)));
    let _ = l.register(count_cell(4, OP_OFF, r.map(|x| x.oob)));
    let _ = l.register(count_cell(5, OP_CHAIN, r.map(|x| x.chain)));
    let _ = l.register(count_cell(6, OP_SEM, r.map(|x| x.sem)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_HASH, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_HEAD,
                ok: false,
                code: f.code(),
                value: 0,
            });
        }
        i += 1;
    }
    l
}

// ---------------------------------------------------------------------------
// 注入
// ---------------------------------------------------------------------------

/// 四路注入：表、判、渲染、账本。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && header_ok() && shape_ok();
    out[1] = inspect_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && reds_ok() && detail_ok() && slots_ok() && b22_ok();
    out[3] = gate_ok()
        && ledger_ok()
        && {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            a && b
        };
    out
}

// ---------------------------------------------------------------------------
// 判据基元
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let codes = [
        VN_ESTAGE, VN_ECODE, VN_EOFF, VN_ENOTE, VN_ECROSS, VN_ESTATE, VN_ECOUNT, VN_ESLOT,
        VN_ESEAL, VN_ENOBS, VN_EMATRIX, VN_EORDER, VN_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let a = SEG_BASE == 0x6ce70;
    let b = VN_ESTAGE == SEG_BASE + 0x01 && VN_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = VERNEED_SZ == 16 && VERNAUX_SZ == 16 && VER_FLG_WEAK == 2;
    a && b && c && d
}

/// 头字段布局。
fn header_ok() -> bool {
    let a = VERNEED_FIELDS.len() == 5;
    let mut i = 0usize;
    let mut sum = 0u32;
    while i < VERNEED_FIELDS.len() {
        if let Some((name, off, w)) = VERNEED_FIELDS.get(i) {
            if name.is_empty() || *w == 0 {
                return false;
            }
            if *off + *w > VERNEED_SZ {
                return false;
            }
            sum += *w;
        }
        i += 1;
    }
    let b = sum == VERNEED_SZ;
    let c = match VERNEED_FIELDS.get(3) {
        Some((n, o, w)) => *n == "vn_aux" && *o == 8 && *w == 4,
        None => false,
    };
    let d = match VERNAUX_FIELDS.get(4) {
        Some((n, o, w)) => *n == "vna_next" && *o == 12 && *w == 4,
        None => false,
    };
    let e = elf_hash("VER_3") != 0 && elf_hash("VER_3") == elf_hash("VER_3");
    let f = elf_hash("VER_3") != elf_hash("VER_4");
    let g = elf_hash("") == 0;
    a && b && c && d && e && f && g
}

/// 形状与 B22。
fn shape_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    let mut uniq = true;
    while i < CORPUS {
        let a = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.tag().is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = cs.get(j) {
                if p.tag() == a.tag() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let d0 = match cs.get(0) {
        Some(c) => crossing(&c.shape) == Crossing::Pass,
        None => false,
    };
    let d2 = match cs.get(2) {
        Some(c) => crossing(&c.shape) == Crossing::Reject,
        None => false,
    };
    let d7 = match cs.get(7) {
        Some(c) => crossing(&c.shape) == Crossing::Pass,
        None => false,
    };
    let d8 = match cs.get(8) {
        Some(c) => crossing(&c.shape) == Crossing::Reject,
        None => false,
    };
    let seal_a = match cs.get(0) {
        Some(c) => c.shape.seal() != 0,
        None => false,
    };
    let seal_b = match (cs.get(0), cs.get(2)) {
        (Some(a), Some(b)) => a.shape.seal() != b.shape.seal(),
        _ => false,
    };
    uniq && d0 && d2 && d7 && d8 && seal_a && seal_b
}

/// 结构闸纵线。
fn inspect_ok() -> bool {
    let a = {
        let o = inspect(&SHAPES[0]);
        o.ok() && o.note == Note::Healthy && o.stage == Stage::Ready
    };
    let b = {
        let o = inspect(&SHAPES[1]);
        o.ok() && o.note == Note::WeakPass
    };
    let c = {
        let o = inspect(&SHAPES[2]);
        o.stage == Stage::Aux && o.code == ERR_EFAULT && o.off == 0x100 && o.note == Note::BadAuxOff
    };
    let d = {
        let o = inspect(&SHAPES[3]);
        o.stage == Stage::Str && o.code == ERR_EFAULT && o.off == 0x200 && o.note == Note::BadNameOff
    };
    let e = {
        let o = inspect(&SHAPES[4]);
        o.stage == Stage::Str && o.off == 0x88 && o.note == Note::BadFileOff
    };
    let f = {
        let o = inspect(&SHAPES[5]);
        o.stage == Stage::Chain && o.off == 1 && o.note == Note::ChainBroken
    };
    let g = {
        let o = inspect(&SHAPES[6]);
        o.stage == Stage::Chain && o.off == 0x20 && o.note == Note::ChainBroken
    };
    let h = {
        let o = inspect(&SHAPES[7]);
        o.stage == Stage::Sem && o.note == Note::SemMiss && o.crossed()
    };
    // 短节优先于一切。
    let mut short = SHAPES[2];
    short.blob_len = 8;
    let i = {
        let o = inspect(&short);
        o.stage == Stage::Len && o.note == Note::Short && o.off == 8
    };
    // 版本号优先于计数。
    let mut badver = SHAPES[2];
    badver.vn_version = 2;
    badver.vn_cnt = 0;
    let j = {
        let o = inspect(&badver);
        o.stage == Stage::Version && o.note == Note::BadVersion
    };
    // 计数优先于偏移。
    let mut badcnt = SHAPES[2];
    badcnt.vn_cnt = 0;
    let k = inspect(&badcnt).stage == Stage::Count;
    // 未终止优先于链。
    let mut unterm = SHAPES[5];
    unterm.trailing_nul = false;
    let l = inspect(&unterm).stage == Stage::Nul;
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// B22 分界。
fn b22_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    let mut structural_rejects = 0usize;
    let mut semantic_pass = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        match crossing(&c.shape) {
            Crossing::Reject => {
                if !o.failed() || !o.stage.structural() {
                    return false;
                }
                structural_rejects += 1;
            }
            Crossing::Pass => {
                if o.reject() {
                    return false;
                }
                if o.stage == Stage::Sem {
                    semantic_pass += 1;
                }
            }
        }
        i += 1;
    }
    let a = structural_rejects == 10;
    let b = semantic_pass == 2;
    let c = Crossing::Reject.rejected() && !Crossing::Pass.rejected();
    let d = Crossing::Reject.name() == "b22-reject" && Crossing::Pass.name() == "b22-pass";
    a && b && c && d
}

/// 申报对判。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        if let Some(c) = cs.get(i) {
            if judge(c) != Fault::Ok {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let c = match cs.get(j + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let want = match REJECT_FAULTS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        let code = match REJECT_CODES.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        if judge(&c).code() != code {
            return false;
        }
        j += 1;
    }
    true
}

/// 矩阵。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    m.row(0) == Some(Row { total: 2, good: 2 })
        && m.row(1) == Some(Row { total: 7, good: 3 })
        && m.row(2) == Some(Row { total: 3, good: 2 })
        && m.row(3) == Some(Row { total: 2, good: 1 })
        && m.row(4).is_none()
        && m.total() == CORPUS
        && m.good() == ACCEPT
        && m.balanced()
        && m.perfect_rows() == 1
        && m.seal() == Matrix::of().seal()
        && m.seal() != Matrix::blank().seal()
}

/// 标签唯一。
fn tags_unique() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.tag().is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = cs.get(j) {
                if p.tag() == c.tag() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 逐槽注解合规。
fn notes_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if a.ok() && a.note.structural() {
            return false;
        }
        if a.reject() && !a.note.structural() {
            return false;
        }
        if a.reject() && !a.stage.structural() {
            return false;
        }
        if a.ok() && a.code != 0 {
            return false;
        }
        if detail(&c).as_str().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐槽对判表。
fn table_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let f = judge(&c);
        if i < ACCEPT {
            if f != Fault::Ok {
                return false;
            }
        } else if f == Fault::Ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 语料与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.ready == 2 && r.oob == 3 && r.chain == 2 && r.sem == 1;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = tags_unique() && notes_ok() && table_ok();
    let e = r.b22_rejects() == 10;
    a && b && c && d && e
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(VN_ECOUNT) && !r.green();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(VN_ESLOT);
    a && b && c
}

/// 逐字文案。
const DETAIL_OK: &str =
    "libdep.so dep=libdep.so len=64 stage=ready code=0 off=0x00000000 note=healthy cross=b22-pass\n";
/// 越界释义。
const DETAIL_OOB: &str =
    "libbad.so dep=libbad.so len=64 stage=aux code=14 off=0x00000100 note=bad-aux-off cross=b22-reject msg=\"version section of libbad.so: vn_aux offset 0x00000100 out of bounds (required by ./a.so)\"\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_OK,
        None => false,
    };
    let b = match cs.get(2) {
        Some(c) => detail(c).as_str() == DETAIL_OOB,
        None => false,
    };
    let c = match cs.get(7) {
        Some(c) => {
            message(&c.shape).as_str() == "version `VER_9' not found (required by ./a.so)"
        }
        None => false,
    };
    let d = msg_short("libbad.so", 8, "./a.so").as_str()
        == "version section of libbad.so truncated at 8 bytes (required by ./a.so)";
    let e = msg_bad_version("libbad.so", 2, "./a.so").as_str()
        == "version section of libbad.so: unsupported version 2 (required by ./a.so)";
    let f = msg_chain("libbad.so", 1, 2, "./a.so").as_str()
        == "version section of libbad.so: aux chain length 1 != cnt 2 (required by ./a.so)";
    let g = msg_hash("libbad.so", "VER_3", "./a.so").as_str()
        == "version section of libbad.so: hash mismatch for `VER_3' (required by ./a.so)";
    a && b && c && d && e && f && g
}

/// 槽号与档位。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => c.tag() == "healthy-single",
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "sem-cross-swap",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(VN_ENOBS);
    let e = Stage::Len.order() == 0
        && Stage::Aux.order() == 3
        && Stage::Chain.order() == 6
        && Stage::Sem.order() == 9
        && Stage::Ready.order() == 10;
    let f = Stage::Aux.structural() && !Stage::Sem.structural() && !Stage::Ready.structural();
    let g = Tier::Ready.index() == 0
        && Tier::Oob.index() == 1
        && Tier::Chain.index() == 2
        && Tier::Sem.index() == 3;
    let h = Tier::Ready.name() == "ready"
        && Tier::Oob.name() == "oob"
        && Tier::Chain.name() == "chain"
        && Tier::Sem.name() == "sem";
    let i = fault_short(Fault::Off) == "off"
        && fault_short(Fault::Cross) == "cross"
        && fault_short(Fault::Ok) == "ok";
    let j = Note::BadAuxOff.offset_class()
        && Note::BadFileOff.offset_class()
        && Note::BadNameOff.offset_class()
        && !Note::ChainBroken.offset_class();
    let k = Note::BadAuxOff.structural()
        && Note::SemMiss.structural() == false
        && Note::Healthy.structural() == false;
    a && b && c && d && e && f && g && h && i && j && k
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Version) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Len) == Err(VN_EORDER) && g.steps() == 1;
    let d = g.step(Stage::Aux) == Ok(()) && g.last() == 3 && g.steps() == 2;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(VN_ESTATE);
    let h = g.step(Stage::Ready) == Err(VN_ESTATE);
    a && b && c && d && e && f && h
}

/// 账本形状。
fn ledger_ok() -> bool {
    let l = build_ledger();
    l.len() == SAMPLES
        && l.ok_count() == ACCEPT
        && l.err_count() == REJECT
        && l.audit() == Ok(())
        && l.sample(100).is_none()
        && !l.is_empty()
}

// ---------------------------------------------------------------------------
// 判据集
// ---------------------------------------------------------------------------

/// 13 条判据。
pub fn run_unx11654_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11654_verneed_oob");

    s.add("VN01-名称与段基址", name_ok(), "static");

    s.add("VN02-头字段布局", header_ok(), "static");

    s.add("VN03-形状与B22", shape_ok(), "static");

    s.add("VN04-结构闸纵线", inspect_ok(), "static");

    s.add("VN05-申报对判", judge_ok(), "static");

    s.add("VN06-矩阵统计", matrix_ok(), "static");

    s.add("VN07-语料与渲染", corpus_ok(), "static");

    s.add("VN08-红档显式", reds_ok(), "static");

    s.add("VN09-释义逐字", detail_ok(), "static");

    s.add("VN10-槽号与档位", slots_ok(), "static");

    s.add("VN11-阶段闸门", gate_ok(), "static");

    s.add("VN12-账本对账", ledger_ok(), "static");

    s.add(
        "VN13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> ! {
        panic!("unx11654 test fail")
    }

    #[test]
    fn consts() {
        assert_eq!(SEG_BASE, 0x6ce70);
        assert_eq!(SAMPLES, 14);
        assert_eq!(VERNEED_SZ, 16);
        assert_eq!(VERNAUX_SZ, 16);
        assert_eq!(VER_FLG_WEAK, 2);
        assert_eq!(VALS[7], 929);
    }

    #[test]
    fn errno_values() {
        assert_eq!(ERR_ENOENT, 2);
        assert_eq!(ERR_ENOEXEC, 8);
        assert_eq!(ERR_EFAULT, 14);
        assert_eq!(ERR_EINVAL, 22);
    }

    #[test]
    fn error_codes_spaced() {
        assert_eq!(VN_ESTAGE, 0x6ce71);
        assert_eq!(VN_ECROSS, 0x6ce75);
        assert_eq!(VN_EBAL, 0x6ce7d);
    }

    #[test]
    fn field_layout_sums() {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < VERNEED_FIELDS.len() {
            if let Some((_, _, w)) = VERNEED_FIELDS.get(i) {
                n += *w;
            }
            i += 1;
        }
        assert_eq!(n, VERNEED_SZ);
        let mut m = 0u32;
        let mut j = 0usize;
        while j < VERNAUX_FIELDS.len() {
            if let Some((_, _, w)) = VERNAUX_FIELDS.get(j) {
                m += *w;
            }
            j += 1;
        }
        assert_eq!(m, VERNAUX_SZ);
    }

    #[test]
    fn stage_orders() {
        assert_eq!(Stage::Len.order(), 0);
        assert_eq!(Stage::Aux.order(), 3);
        assert_eq!(Stage::Chain.order(), 6);
        assert_eq!(Stage::Sem.order(), 9);
        assert_eq!(Stage::Ready.order(), 10);
        assert!(Stage::Flags.structural());
        assert!(!Stage::Sem.structural());
    }

    #[test]
    fn note_classes() {
        assert!(Note::BadAuxOff.offset_class());
        assert!(Note::BadNameOff.offset_class());
        assert!(!Note::ChainBroken.offset_class());
        assert!(Note::Short.structural());
        assert!(!Note::SemMiss.structural());
        assert!(!Note::Healthy.structural());
    }

    #[test]
    fn healthy_single() {
        let o = inspect(&SHAPES[0]);
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.note, Note::Healthy);
        assert_eq!(o.code, 0);
        assert!(o.crossed());
    }

    #[test]
    fn healthy_weak() {
        let o = inspect(&SHAPES[1]);
        assert!(o.ok());
        assert_eq!(o.note, Note::WeakPass);
    }

    #[test]
    fn aux_oob() {
        let o = inspect(&SHAPES[2]);
        assert!(o.reject());
        assert_eq!(o.stage, Stage::Aux);
        assert_eq!(o.code, ERR_EFAULT);
        assert_eq!(o.off, 0x100);
        assert_eq!(o.note, Note::BadAuxOff);
    }

    #[test]
    fn name_oob() {
        let o = inspect(&SHAPES[3]);
        assert_eq!(o.stage, Stage::Str);
        assert_eq!(o.note, Note::BadNameOff);
        assert_eq!(o.off, 0x200);
    }

    #[test]
    fn file_oob() {
        let o = inspect(&SHAPES[4]);
        assert_eq!(o.stage, Stage::Str);
        assert_eq!(o.note, Note::BadFileOff);
        assert_eq!(o.off, 0x88);
    }

    #[test]
    fn chain_short() {
        let o = inspect(&SHAPES[5]);
        assert_eq!(o.stage, Stage::Chain);
        assert_eq!(o.note, Note::ChainBroken);
        assert_eq!(o.off, 1);
    }

    #[test]
    fn chain_run() {
        let o = inspect(&SHAPES[6]);
        assert_eq!(o.stage, Stage::Chain);
        assert_eq!(o.off, 0x20);
    }

    #[test]
    fn semantic_miss_crosses() {
        let o = inspect(&SHAPES[7]);
        assert_eq!(o.stage, Stage::Sem);
        assert_eq!(o.note, Note::SemMiss);
        assert!(o.crossed());
        assert!(!o.reject());
    }

    #[test]
    fn short_preempts_all() {
        let mut s = SHAPES[2];
        s.blob_len = 8;
        s.vn_version = 9;
        let o = inspect(&s);
        assert_eq!(o.stage, Stage::Len);
        assert_eq!(o.off, 8);
    }

    #[test]
    fn version_preempts_count() {
        let mut s = SHAPES[2];
        s.vn_version = 2;
        s.vn_cnt = 0;
        assert_eq!(inspect(&s).stage, Stage::Version);
    }

    #[test]
    fn count_preempts_aux() {
        let mut s = SHAPES[2];
        s.vn_cnt = 0;
        assert_eq!(inspect(&s).stage, Stage::Count);
    }

    #[test]
    fn aux_alignment_checked() {
        let mut s = SHAPES[0];
        s.vn_aux = 0x11;
        assert_eq!(inspect(&s).stage, Stage::Aux);
        assert_eq!(inspect(&s).code, ERR_EFAULT);
    }

    #[test]
    fn aux_zero_checked() {
        let mut s = SHAPES[0];
        s.vn_aux = 0;
        assert_eq!(inspect(&s).note, Note::BadAuxOff);
    }

    #[test]
    fn unterminated_checked() {
        let mut s = SHAPES[0];
        s.trailing_nul = false;
        let o = inspect(&s);
        assert_eq!(o.stage, Stage::Nul);
        assert_eq!(o.note, Note::Unterminated);
    }

    #[test]
    fn vn_next_must_be_zero() {
        let mut s = SHAPES[0];
        s.vn_next = 0x40;
        let o = inspect(&s);
        assert_eq!(o.stage, Stage::Chain);
        assert_eq!(o.off, 0x40);
    }

    #[test]
    fn hash_checked() {
        let mut s = SHAPES[0];
        s.hash_ok = false;
        let o = inspect(&s);
        assert_eq!(o.stage, Stage::Hash);
        assert_eq!(o.note, Note::BadHash);
    }

    #[test]
    fn flags_checked() {
        let mut s = SHAPES[0];
        s.aux_flags = 0x4;
        let o = inspect(&s);
        assert_eq!(o.stage, Stage::Flags);
        assert_eq!(o.note, Note::BadFlags);
    }

    #[test]
    fn other_checked() {
        let mut s = SHAPES[0];
        s.other_ok = false;
        let o = inspect(&s);
        assert_eq!(o.stage, Stage::Flags);
        assert_eq!(o.note, Note::BadFlags);
    }

    #[test]
    fn elf_hash_stable() {
        assert_eq!(elf_hash("VER_3"), elf_hash("VER_3"));
        assert_ne!(elf_hash("VER_3"), elf_hash("VER_4"));
        assert_eq!(elf_hash(""), 0);
        assert_ne!(elf_hash("GLIBC_2.34"), 0);
    }

    #[test]
    fn messages_exact() {
        assert_eq!(
            message(&SHAPES[2]).as_str(),
            "version section of libbad.so: vn_aux offset 0x00000100 out of bounds (required by ./a.so)"
        );
        assert_eq!(
            message(&SHAPES[5]).as_str(),
            "version section of libbad.so: aux chain length 1 != cnt 2 (required by ./a.so)"
        );
        assert_eq!(
            message(&SHAPES[7]).as_str(),
            "version `VER_9' not found (required by ./a.so)"
        );
        assert_eq!(message(&SHAPES[0]).as_str(), "");
    }

    #[test]
    fn differential_b22() {
        assert_eq!(crossing(&SHAPES[0]), Crossing::Pass);
        assert_eq!(crossing(&SHAPES[2]), Crossing::Reject);
        assert_eq!(crossing(&SHAPES[7]), Crossing::Pass);
        assert!(Crossing::Reject.rejected());
        assert!(!Crossing::Pass.rejected());
        assert_eq!(Crossing::Reject.name(), "b22-reject");
    }

    #[test]
    fn tiers() {
        assert_eq!(Tier::of(&inspect(&SHAPES[0])), Tier::Ready);
        assert_eq!(Tier::of(&inspect(&SHAPES[2])), Tier::Oob);
        assert_eq!(Tier::of(&inspect(&SHAPES[5])), Tier::Chain);
        assert_eq!(Tier::of(&inspect(&SHAPES[7])), Tier::Sem);
        // 版本号坏并入链坏档。
        let mut s = SHAPES[0];
        s.vn_version = 3;
        assert_eq!(Tier::of(&inspect(&s)), Tier::Chain);
    }

    #[test]
    fn judge_accepts() {
        let cs = cases();
        let mut i = 0usize;
        while i < ACCEPT {
            assert_eq!(judge(&cs[i]), Fault::Ok);
            i += 1;
        }
    }

    #[test]
    fn judge_rejects() {
        let cs = cases();
        let mut i = 0usize;
        while i < REJECT {
            let c = &cs[i + ACCEPT];
            assert_eq!(judge(c), REJECT_FAULTS[i]);
            assert_eq!(judge(c).code(), REJECT_CODES[i]);
            i += 1;
        }
    }

    #[test]
    fn reject_specifics() {
        let cs = cases();
        assert_eq!(judge(&cs[8]), Fault::Estate);
        assert_eq!(judge(&cs[9]), Fault::Stage);
        assert_eq!(judge(&cs[10]), Fault::Code);
        assert_eq!(judge(&cs[11]), Fault::Off);
        assert_eq!(judge(&cs[12]), Fault::Note);
        assert_eq!(judge(&cs[13]), Fault::Cross);
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.row(0), Some(Row { total: 2, good: 2 }));
        assert_eq!(m.row(1), Some(Row { total: 7, good: 3 }));
        assert_eq!(m.row(2), Some(Row { total: 3, good: 2 }));
        assert_eq!(m.row(3), Some(Row { total: 2, good: 1 }));
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.perfect_rows(), 1);
    }

    #[test]
    fn matrix_seal() {
        assert_eq!(Matrix::of().seal(), Matrix::of().seal());
        assert_ne!(Matrix::of().seal(), Matrix::blank().seal());
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, 8);
        assert_eq!(r.reject, 6);
        assert_eq!(r.ready, 2);
        assert_eq!(r.oob, 3);
        assert_eq!(r.chain, 2);
        assert_eq!(r.sem, 1);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.b22_rejects(), 10);
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "verneed-oob corpus=14 accept=8 reject=6 ready=2 oob=3 chain=2 sem=1 b22=10 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(VN_ECOUNT));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(VN_ESLOT));
    }

    #[test]
    fn detail_exact() {
        let cs = cases();
        assert_eq!(detail(&cs[0]).as_str(), DETAIL_OK);
        assert_eq!(detail(&cs[2]).as_str(), DETAIL_OOB);
    }

    #[test]
    fn slot_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(CORPUS - 1).is_some());
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(0), Err(VN_ENOBS));
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Cross), "cross");
    }

    #[test]
    fn gate_steps() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Version), Ok(()));
        assert_eq!(g.step(Stage::Len), Err(VN_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Aux), Ok(()));
        assert_eq!(g.last(), 3);
    }

    #[test]
    fn gate_close() {
        let mut g = Gate::new();
        assert!(g.close().is_ok());
        assert_eq!(g.close(), Err(VN_ESTATE));
        assert_eq!(g.step(Stage::Ready), Err(VN_ESTATE));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.register(Sample::blank()).err(), Some(VN_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(VN_EBAL));
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0");
        assert!(out[1], "inject1");
        assert!(out[2], "inject2");
        assert!(out[3], "inject3");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(off_text(0x100).as_str(), "0x00000100");
        assert_eq!(i32_text(-14).as_str(), "-14");
        assert_eq!(fold_str(FNV_OFF, "x"), fold_str(FNV_OFF, "x"));
        assert_ne!(fold_str(FNV_OFF, "x"), fold_str(FNV_OFF, "y"));
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11654_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
