//! UNX-F11585 未知 applet 诊断判据（"applet not found" 文案与 B24 锚）
//!
//! 调用一个不存在的 applet 时，busybox 必须给出**可机读的诊断**：退出码 127、
//! 一行 stderr 文案、以及一条可检索的提示。诊断面看似只有一句字符串，实际
//! 要同时钉住四件事：
//!
//! ```text
//! 分类  已知 / 未知 / 选项形 / 空名 / 含斜杠 / 坏字节 / 超长，七类互斥
//! 退出  未知类 127（shell 的 command-not-found 约定）、选项形 1、已知 0
//! 文案  未知类统一 "busybox: applet not found"，不改写、不加句号
//! 锚    每条文案在 B24 批锚下有唯一编号，锚号非 24 的登记直接拒
//! ```
//!
//! 为什么锚号要严到「非 24 即拒」：诊断文案会被 `--list` 的自检脚本与
//! 打包器的回归面按锚号引用，锚号漂移等于把「断言」悄悄换成另一条断言。
//! 本模块把锚表做成**定容目录**（8 槽），每条锚带 12 字节标签与 40 字节
//! 备注，重复标签、超容、非法标签一律落错误码，不静默覆盖。
//!
//! 另附近似建议：对未知名做有上限的编辑距离（≤2）扫描，取表内最靠前的
//! 最小距离名作为 `try 'ls'` 之类的提示。距离算法用两行定容 DP，任何长度
//! 都不越界、不分配。
//!
//! 闸门次序：空名 → 长度 → 字节 → 选项形 → 斜杠 → 表查找 → 退出码 → 文案 → 锚。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf9a0;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单名容量。
pub const NAME_CAP: usize = 32;

/// 锚标签容量。
pub const TAG_CAP: usize = 12;

/// 锚备注容量。
pub const NOTE_CAP: usize = 40;

/// 锚表容量。
pub const ANCHOR_CAP: usize = 8;

/// 批量诊断容量。
pub const BATCH_CAP: usize = 8;

/// B24 批锚号（唯一合法锚号）。
pub const BATCH_ID: u16 = 24;

/// B24 批锚标签。
pub const BATCH_TAG: &str = "B24";

/// 建议距离上限。
pub const SUGGEST_CAP: u8 = 2;

/// 未知类退出码（shell command-not-found 约定）。
pub const EXIT_NOT_FOUND: u8 = 127;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 非打印字节。
pub const UA_EBYTE: u32 = SEG_BASE + 0x01;
/// 超长。
pub const UA_ELEN: u32 = SEG_BASE + 0x02;
/// 空标签。
pub const UA_EMPTY: u32 = SEG_BASE + 0x03;
/// 标签含斜杠。
pub const UA_ESLASH: u32 = SEG_BASE + 0x04;
/// 选项形标签（保留）。
pub const UA_EOPT: u32 = SEG_BASE + 0x05;
/// 锚重复。
pub const UA_EDUP: u32 = SEG_BASE + 0x06;
/// 容量越界。
pub const UA_ECAP: u32 = SEG_BASE + 0x07;
/// 锚号不是 B24。
pub const UA_EANCHOR: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const UA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const UA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const UA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const UA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const UA_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 分类。
pub const OP_CLASS: u8 = 0x01;
/// 文案合成。
pub const OP_RENDER: u8 = 0x02;
/// 近似建议。
pub const OP_SUGGEST: u8 = 0x03;
/// 锚目录。
pub const OP_CATALOG: u8 = 0x04;
/// 批量诊断。
pub const OP_BATCH: u8 = 0x05;
/// 退出码。
pub const OP_EXIT: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（每字节 8 次乘法）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut sh = 0u32;
    while sh < 64 {
        let b = ((w >> sh) & 0xff) as u8;
        v = fold_byte(v, b);
        sh += 8;
    }
    v
}

/// 折入一段字节。
fn fold_bytes(mut h: u64, s: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 严格可打印（0x21..=0x7e，不含空格）。
fn printable(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            if *b < 0x21 || *b > 0x7e {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 可打印含空格（0x20..=0x7e，备注用）。
fn printable_note(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            if *b < 0x20 || *b > 0x7e {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 逐字节相等。
fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 推入无符号十进制。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    while v > 0 {
        let d = (v % 10) as u8;
        v /= 10;
        at -= 1;
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'0' + d;
        }
    }
    let mut i = at;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
}

/// 字节拼 String。
fn text_of(s: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            out.push(*b as char);
        }
        i += 1;
    }
    out
}

/// 是否含斜杠。
fn has_slash(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            if *b == b'/' {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 名表（20 条，C 序）
// ---------------------------------------------------------------------------

/// 诊断面覆盖的 applet 名（C 序 20 条）。
pub const TABLE: [&str; 20] = [
    "ash", "awk", "cat", "cp", "cut", "date", "dd", "df", "du", "echo",
    "find", "grep", "head", "kill", "ln", "ls", "mkdir", "mv", "printf", "ps",
];

/// 表内查找。
pub fn find_applet(name: &[u8]) -> Option<u8> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(t) = TABLE.get(i) {
            if eq_bytes(t.as_bytes(), name) {
                return Some(i as u8);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 分类
// ---------------------------------------------------------------------------

/// 诊断分类（七类互斥）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 已知 applet（带表内序号）。
    Known(u8),
    /// 未知。
    Unknown,
    /// 选项形（以 `-` 开头）。
    Option,
    /// 空名。
    Empty,
    /// 含斜杠。
    Slash,
    /// 坏字节。
    BadByte,
    /// 超长。
    TooLong,
}

impl Verdict {
    /// 分类标签。
    pub fn tag(&self) -> &'static str {
        match self {
            Verdict::Known(_) => "known",
            Verdict::Unknown => "unknown",
            Verdict::Option => "option",
            Verdict::Empty => "empty",
            Verdict::Slash => "slash",
            Verdict::BadByte => "byte",
            Verdict::TooLong => "long",
        }
    }

    /// 退出码。
    pub fn exit(&self) -> u8 {
        match self {
            Verdict::Known(_) => 0,
            Verdict::Unknown => EXIT_NOT_FOUND,
            Verdict::Option => 1,
            Verdict::Empty => EXIT_NOT_FOUND,
            Verdict::Slash => EXIT_NOT_FOUND,
            Verdict::BadByte => EXIT_NOT_FOUND,
            Verdict::TooLong => EXIT_NOT_FOUND,
        }
    }

    /// 是否已知。
    pub fn is_known(&self) -> bool {
        matches!(self, Verdict::Known(_))
    }

    /// 是否「找不到」族（127）。
    pub fn is_missing(&self) -> bool {
        self.exit() == EXIT_NOT_FOUND
    }
}

/// 分类：闸门顺序固定为 空 → 长 → 字节 → 选项 → 斜杠 → 表查找。
pub fn classify(name: &[u8]) -> Verdict {
    if name.is_empty() {
        return Verdict::Empty;
    }
    if name.len() > NAME_CAP {
        return Verdict::TooLong;
    }
    if !printable(name) {
        return Verdict::BadByte;
    }
    if let Some(f) = name.get(0) {
        if *f == b'-' {
            return Verdict::Option;
        }
    }
    if has_slash(name) {
        return Verdict::Slash;
    }
    match find_applet(name) {
        Some(id) => Verdict::Known(id),
        None => Verdict::Unknown,
    }
}

// ---------------------------------------------------------------------------
// 编辑距离与建议
// ---------------------------------------------------------------------------

/// 带上限的编辑距离（Levenshtein，插入/删除/替换各计 1）。
///
/// 两行定容 DP：长度超 `NAME_CAP` 的输入已在分类闸被拒，这里再夹一次，
/// 保证数组下标恒在界内。超过 `cap` 时返回 `cap + 1`（不精确值，只用于
/// 「够不够近」的判定）。
pub fn distance(a: &[u8], b: &[u8], cap: u8) -> u8 {
    let la = a.len();
    let lb = b.len();
    if la > NAME_CAP || lb > NAME_CAP {
        return cap.saturating_add(1);
    }
    let diff = if la > lb { la - lb } else { lb - la };
    if diff > cap as usize {
        return cap.saturating_add(1);
    }
    let mut prev = [0u8; NAME_CAP + 1];
    let mut cur = [0u8; NAME_CAP + 1];
    let mut j = 0usize;
    while j <= lb {
        if let Some(dst) = prev.get_mut(j) {
            *dst = j as u8;
        }
        j += 1;
    }
    let mut i = 1usize;
    while i <= la {
        if let Some(dst) = cur.get_mut(0) {
            *dst = i as u8;
        }
        let mut k = 1usize;
        while k <= lb {
            let ac = a.get(i.saturating_sub(1)).copied().unwrap_or(0);
            let bc = b.get(k.saturating_sub(1)).copied().unwrap_or(0);
            let sub = if ac == bc { 0u8 } else { 1u8 };
            let up = prev.get(k).copied().unwrap_or(cap).saturating_add(1);
            let left = cur.get(k.saturating_sub(1)).copied().unwrap_or(cap).saturating_add(1);
            let diag = prev
                .get(k.saturating_sub(1))
                .copied()
                .unwrap_or(cap)
                .saturating_add(sub);
            let mut best = up;
            if left < best {
                best = left;
            }
            if diag < best {
                best = diag;
            }
            if let Some(dst) = cur.get_mut(k) {
                *dst = best;
            }
            k += 1;
        }
        let tmp = prev;
        prev = cur;
        cur = tmp;
        i += 1;
    }
    prev.get(lb).copied().unwrap_or(cap)
}

/// 近似建议：仅对 Unknown 生效；取表内距离最小且最靠前的名。
pub fn suggest(name: &[u8]) -> Option<&'static str> {
    if !matches!(classify(name), Verdict::Unknown) {
        return None;
    }
    let mut best = SUGGEST_CAP.saturating_add(1);
    let mut found: Option<&'static str> = None;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(t) = TABLE.get(i) {
            let d = distance(name, t.as_bytes(), SUGGEST_CAP);
            if d < best {
                best = d;
                found = Some(t);
            }
        }
        i += 1;
    }
    if best <= SUGGEST_CAP {
        found
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 锚目录
// ---------------------------------------------------------------------------

/// 一条诊断锚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    id: u16,
    tag: [u8; TAG_CAP],
    tag_len: usize,
    note: [u8; NOTE_CAP],
    note_len: usize,
}

impl Anchor {
    /// 空白。
    pub const fn blank() -> Anchor {
        Anchor { id: 0, tag: [0u8; TAG_CAP], tag_len: 0, note: [0u8; NOTE_CAP], note_len: 0 }
    }

    /// 构造。闸门：锚号 → 标签空 → 标签长 → 标签字节 → 标签斜杠 →
    /// 选项形标签 → 备注长 → 备注字节。
    pub fn new(id: u16, tag: &[u8], note: &[u8]) -> Result<Anchor, u32> {
        if id != BATCH_ID {
            return Err(UA_EANCHOR);
        }
        if tag.is_empty() {
            return Err(UA_EMPTY);
        }
        if tag.len() > TAG_CAP || note.len() > NOTE_CAP {
            return Err(UA_ELEN);
        }
        if !printable(tag) {
            return Err(UA_EBYTE);
        }
        if has_slash(tag) {
            return Err(UA_ESLASH);
        }
        if let Some(f) = tag.get(0) {
            if *f == b'-' {
                return Err(UA_EOPT);
            }
        }
        if !printable_note(note) {
            return Err(UA_EBYTE);
        }
        let mut a = Anchor::blank();
        a.id = id;
        let mut i = 0usize;
        while i < tag.len() {
            match (tag.get(i), a.tag.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(UA_ELEN),
            }
            i += 1;
        }
        a.tag_len = tag.len();
        let mut k = 0usize;
        while k < note.len() {
            match (note.get(k), a.note.get_mut(k)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(UA_ELEN),
            }
            k += 1;
        }
        a.note_len = note.len();
        Ok(a)
    }

    /// 锚号。
    pub fn id(&self) -> u16 {
        self.id
    }

    /// 标签字节。
    pub fn tag(&self) -> &[u8] {
        self.tag.get(0..self.tag_len).unwrap_or(&[])
    }

    /// 标签文本。
    pub fn tag_text(&self) -> String {
        text_of(self.tag())
    }

    /// 备注字节。
    pub fn note(&self) -> &[u8] {
        self.note.get(0..self.note_len).unwrap_or(&[])
    }

    /// 备注文本。
    pub fn note_text(&self) -> String {
        text_of(self.note())
    }

    /// 是否同标签。
    pub fn tagged(&self, t: &[u8]) -> bool {
        eq_bytes(self.tag(), t)
    }

    /// 一行文本：`B24-U1: ...`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str(self.tag_text().as_str());
        s.push_str(": ");
        s.push_str(self.note_text().as_str());
        s
    }
}

/// 锚目录（定容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Catalog {
    items: [Anchor; ANCHOR_CAP],
    len: usize,
}

impl Catalog {
    /// 空目录。
    pub const fn new() -> Catalog {
        Catalog { items: [Anchor::blank(); ANCHOR_CAP], len: 0 }
    }

    /// 登记一条：重复标签 → `EDUP`，超容 → `ECAP`。
    pub fn add(&mut self, a: Anchor) -> Result<usize, u32> {
        if self.find(a.tag()).is_some() {
            return Err(UA_EDUP);
        }
        if self.len >= ANCHOR_CAP {
            return Err(UA_ECAP);
        }
        let idx = self.len;
        if let Some(dst) = self.items.get_mut(idx) {
            *dst = a;
        }
        self.len += 1;
        Ok(idx)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一条。
    pub fn at(&self, i: usize) -> Option<Anchor> {
        if i >= self.len {
            return None;
        }
        self.items.get(i).copied()
    }

    /// 按标签查找。
    pub fn find(&self, tag: &[u8]) -> Option<Anchor> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.items.get(i) {
                if a.tagged(tag) {
                    return Some(*a);
                }
            }
            i += 1;
        }
        None
    }

    /// 锚号是否全为 B24。
    pub fn all_b24(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.items.get(i) {
                Some(a) => {
                    if a.id() != BATCH_ID {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 整表文本（逐行，`\n` 结尾）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.items.get(i) {
                s.push_str(a.text().as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.items.get(i) {
                h = fold(h, a.id() as u64);
                h = fold_bytes(h, a.tag());
                h = fold_bytes(h, a.note());
            }
            i += 1;
        }
        h
    }
}

/// 标准锚目录（五条，全挂 B24）。
pub fn standard_catalog() -> Result<Catalog, u32> {
    let mut c = Catalog::new();
    c.add(Anchor::new(BATCH_ID, b"B24-U1", b"unknown applet exit 127")?)?;
    c.add(Anchor::new(BATCH_ID, b"B24-U2", b"empty name exit 127")?)?;
    c.add(Anchor::new(BATCH_ID, b"B24-U3", b"slash name rejected")?)?;
    c.add(Anchor::new(BATCH_ID, b"B24-U4", b"option form rejected")?)?;
    c.add(Anchor::new(BATCH_ID, b"B24-U5", b"byte and length gates")?)?;
    Ok(c)
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 一条诊断文案（body + hint），不可 Copy。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    body: String,
    hint: String,
    exit: u8,
}

impl Message {
    /// 空白。
    pub fn blank() -> Message {
        Message { body: String::new(), hint: String::new(), exit: 0 }
    }

    /// 主体。
    pub fn body(&self) -> &str {
        self.body.as_str()
    }

    /// 提示。
    pub fn hint(&self) -> &str {
        self.hint.as_str()
    }

    /// 退出码。
    pub fn exit(&self) -> u8 {
        self.exit
    }

    /// 是否空文案（已知 applet 无诊断）。
    pub fn is_empty(&self) -> bool {
        self.body.is_empty()
    }

    /// 全文：`body\nhint\n`；已知时为空串。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.body.is_empty() {
            return s;
        }
        s.push_str(self.body.as_str());
        s.push('\n');
        if !self.hint.is_empty() {
            s.push_str(self.hint.as_str());
            s.push('\n');
        }
        s
    }

    /// 字节数。
    pub fn len(&self) -> usize {
        self.text().as_bytes().len()
    }
}

/// 规范主体：`busybox: applet not found`。
pub const MSG_NOT_FOUND: &str = "busybox: applet not found";

/// 规范提示：`busybox: try 'busybox --list'`。
pub const HINT_LIST: &str = "busybox: try 'busybox --list'";

/// 合成诊断：按分类选文案骨架，`Slash` 会把原名嵌进主体。
pub fn compose(name: &[u8]) -> Message {
    let v = classify(name);
    match v {
        Verdict::Known(_) => Message::blank(),
        Verdict::Unknown => Message {
            body: String::from(MSG_NOT_FOUND),
            hint: String::from(HINT_LIST),
            exit: EXIT_NOT_FOUND,
        },
        Verdict::Empty => Message {
            body: String::from(MSG_NOT_FOUND),
            hint: String::from(HINT_LIST),
            exit: EXIT_NOT_FOUND,
        },
        Verdict::TooLong => Message {
            body: String::from("busybox: applet name too long"),
            hint: String::from(MSG_NOT_FOUND),
            exit: EXIT_NOT_FOUND,
        },
        Verdict::BadByte => Message {
            body: String::from("busybox: invalid applet name"),
            hint: String::from(MSG_NOT_FOUND),
            exit: EXIT_NOT_FOUND,
        },
        Verdict::Option => Message {
            body: {
                let mut s = String::from("busybox: unrecognized option '");
                s.push_str(text_of(name).as_str());
                s.push('\'');
                s
            },
            hint: String::from("busybox: try 'busybox --help'"),
            exit: 1,
        },
        Verdict::Slash => Message {
            body: {
                let mut s = String::from("busybox: can't run '");
                s.push_str(text_of(name).as_str());
                s.push_str("': applet not found");
                s
            },
            hint: String::from(HINT_LIST),
            exit: EXIT_NOT_FOUND,
        },
    }
}

// ---------------------------------------------------------------------------
// 批量诊断
// ---------------------------------------------------------------------------

/// 一批名的诊断统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    items: [Verdict; BATCH_CAP],
    len: usize,
}

impl Batch {
    /// 空批。
    pub const fn new() -> Batch {
        Batch { items: [Verdict::Unknown; BATCH_CAP], len: 0 }
    }

    /// 追加一个名（只做分类，不落文案）。
    pub fn push(&mut self, name: &[u8]) -> Result<Verdict, u32> {
        if self.len >= BATCH_CAP {
            return Err(UA_ECAP);
        }
        let v = classify(name);
        if let Some(dst) = self.items.get_mut(self.len) {
            *dst = v;
        }
        self.len += 1;
        Ok(v)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一条。
    pub fn at(&self, i: usize) -> Option<Verdict> {
        if i >= self.len {
            return None;
        }
        self.items.get(i).copied()
    }

    /// 某分类计数。
    pub fn count(&self, tag: &str) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.items.get(i) {
                if v.tag() == tag {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 退出 127 的条数。
    pub fn missing(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.items.get(i) {
                if v.is_missing() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 已知条数。
    pub fn known(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.items.get(i) {
                if v.is_known() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UA_ESLOT);
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

    /// 取格。
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
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(UA_EBAL);
        }
        const VALS: [u32; 8] = [127, 15, 2, 2, 56, 5, 3, 1];
        const CODES: [u32; 6] = [
            UA_EBYTE,
            UA_ELEN,
            UA_EMPTY,
            UA_ESLASH,
            UA_EDUP,
            UA_EANCHOR,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(UA_EBAL),
            };
            if s.slot as usize != i {
                return Err(UA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(UA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(UA_EVALUE);
                        }
                    }
                    None => return Err(UA_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(UA_EVALUE);
                        }
                    }
                    None => return Err(UA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(UA_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UA_EBAL);
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
            Err(UA_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u8, u32>` → 样本。
fn cell8(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(cell8(0, OP_CLASS, Ok(classify(b"lss").exit())));

    let _ = l.register(cell8(
        1,
        OP_CLASS,
        match classify(b"ls") {
            Verdict::Known(id) => Ok(id),
            _ => Err(UA_ENOBS),
        },
    ));

    let _ = l.register(cell(
        2,
        OP_SUGGEST,
        match suggest(b"lss") {
            Some(s) => Ok(s.as_bytes().len()),
            None => Err(UA_ENOBS),
        },
    ));

    let _ = l.register(cell8(3, OP_SUGGEST, Ok(distance(b"gerp", b"grep", 2))));

    let _ = l.register(cell(4, OP_RENDER, Ok(compose(b"foo").len())));

    let _ = l.register(cell(
        5,
        OP_CATALOG,
        standard_catalog().map(|c| c.len()),
    ));

    let _ = l.register(cell(6, OP_BATCH, Ok(batch_unknown() as usize)));

    let _ = l.register(cell8(7, OP_EXIT, Ok(classify(b"-x").exit())));

    let _ = l.register(cell8(
        8,
        OP_CATALOG,
        Anchor::new(BATCH_ID, &[0x01u8], b"bad").map(|_| 0u8),
    ));

    let _ = l.register(cell8(
        9,
        OP_CATALOG,
        Anchor::new(BATCH_ID, b"B24-L1", &[b'n'; NOTE_CAP + 1]).map(|_| 0u8),
    ));

    let _ = l.register(cell8(
        10,
        OP_CATALOG,
        Anchor::new(BATCH_ID, b"", b"empty").map(|_| 0u8),
    ));

    let _ = l.register(cell8(
        11,
        OP_CATALOG,
        Anchor::new(BATCH_ID, b"B24/X", b"slash").map(|_| 0u8),
    ));

    let _ = l.register(cell8(
        12,
        OP_CATALOG,
        standard_catalog()
            .and_then(|mut c| {
                let a = Anchor::new(BATCH_ID, b"B24-U1", b"dup")?;
                c.add(a)?;
                Ok(0u8)
            }),
    ));

    let _ = l.register(cell8(
        13,
        OP_CATALOG,
        Anchor::new(BATCH_ID.saturating_sub(1), b"B24-X", b"bad anchor").map(|_| 0u8),
    ));

    l
}

/// 批内未知条数。
fn batch_unknown() -> u32 {
    let mut b = Batch::new();
    let _ = b.push(b"lss");
    let _ = b.push(b"gerp");
    let _ = b.push(b"ls");
    let _ = b.push(b"nope");
    b.count("unknown") as u32
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 分类面。
fn class_ok() -> bool {
    let a = classify(b"ls") == Verdict::Known(15);
    let b = classify(b"lss") == Verdict::Unknown;
    let c = classify(b"") == Verdict::Empty;
    let d = classify(&[0x01u8]) == Verdict::BadByte;
    let e = classify(&[b'z'; 33]) == Verdict::TooLong;
    let f = classify(b"-x") == Verdict::Option;
    let g = classify(b"/bin/ls") == Verdict::Slash;
    let h = classify(b"LS") == Verdict::Unknown;
    let i = classify(b"a b") == Verdict::BadByte;
    let j = Verdict::Unknown.tag() == "unknown" && Verdict::Known(0).tag() == "known";
    a && b && c && d && e && f && g && h && i && j
}

/// 退出码面。
fn exit_ok() -> bool {
    let a = classify(b"foo").exit() == 127;
    let b = classify(b"ls").exit() == 0;
    let c = classify(b"-x").exit() == 1;
    let d = classify(b"").exit() == 127;
    let e = classify(b"/bin/sh").exit() == 127;
    let f = classify(&[0x7f]).exit() == 127;
    let g = classify(&[b'z'; 33]).exit() == 127;
    let h = Verdict::Unknown.is_missing() && !Verdict::Known(3).is_missing();
    a && b && c && d && e && f && g && h
}

/// 文案面。
fn render_ok() -> bool {
    let m = compose(b"foo");
    if m.body() != "busybox: applet not found" {
        return false;
    }
    if m.hint() != "busybox: try 'busybox --list'" {
        return false;
    }
    if m.exit() != 127 || m.len() != 56 {
        return false;
    }
    if m.text().as_str() != "busybox: applet not found\nbusybox: try 'busybox --list'\n" {
        return false;
    }
    let known = compose(b"ls");
    if !known.is_empty() || known.exit() != 0 || !known.text().is_empty() {
        return false;
    }
    let slash = compose(b"/bin/ls");
    if !slash.body().contains("can't run '/bin/ls'") {
        return false;
    }
    let opt = compose(b"-z");
    if opt.exit() != 1 || !opt.body().contains("unrecognized option '-z'") {
        return false;
    }
    compose(b"foo").len() == compose(b"bar").len()
        && compose(b"nope").text().as_str() == m.text().as_str()
}

/// 提示与空名面。
fn hint_ok() -> bool {
    let empty = compose(b"");
    if empty.exit() != 127 || empty.body() != MSG_NOT_FOUND {
        return false;
    }
    let long = compose(&[b'q'; 33]);
    if long.exit() != 127 || !long.body().contains("too long") {
        return false;
    }
    let bad = compose(&[0x02u8]);
    if !bad.body().contains("invalid applet name") {
        return false;
    }
    let opt = compose(b"-h");
    if !opt.hint().contains("--help") {
        return false;
    }
    let slash = compose(b"a/b");
    if slash.hint() != HINT_LIST {
        return false;
    }
    empty.hint() == HINT_LIST
        && long.hint() == MSG_NOT_FOUND
        && bad.hint() == MSG_NOT_FOUND
        && compose(b"foo").hint() == HINT_LIST
}

/// 近似建议面。
fn suggest_ok() -> bool {
    let a = suggest(b"lss") == Some("ls");
    let b = suggest(b"gerp") == Some("grep");
    let c = suggest(b"Cp") == Some("cp");
    let d = suggest(b"ls").is_none();
    let e = suggest(b"").is_none();
    let f = suggest(b"-x").is_none();
    let g = suggest(b"/bin/ls").is_none();
    let h = suggest(b"zzzzzz").is_none();
    let i = suggest(b"xyz").is_none();
    a && b && c && d && e && f && g && h && i
}

/// 编辑距离面。
fn distance_ok() -> bool {
    let a = distance(b"abc", b"abc", 2) == 0;
    let b = distance(b"abc", b"abd", 2) == 1;
    let c = distance(b"abc", b"ab", 2) == 1;
    let d = distance(b"abc", b"abcd", 2) == 1;
    let e = distance(b"gerp", b"grep", 2) == 2;
    let f = distance(b"", b"abc", 2) == 3;
    let g = distance(b"a", b"b", 1) == 1;
    let h = distance(b"kitten", b"sitting", 3) == 3;
    let i = distance(&[b'x'; 40], b"ls", 2) == 3;
    a && b && c && d && e && f && g && h && i
}

/// 锚表装载面。
fn catalog_ok() -> bool {
    let c = match standard_catalog() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if c.len() != 5 || c.is_empty() {
        return false;
    }
    if !c.all_b24() {
        return false;
    }
    let a = match c.find(b"B24-U1") {
        Some(v) => v,
        None => return false,
    };
    if a.id() != 24 || a.note_text().as_str() != "unknown applet exit 127" {
        return false;
    }
    if a.tag_text().as_str() != "B24-U1" || a.text().as_str() != "B24-U1: unknown applet exit 127" {
        return false;
    }
    if c.at(5).is_some() || c.find(b"B24-U9").is_some() {
        return false;
    }
    let text = c.text();
    text.as_str().starts_with("B24-U1: unknown applet exit 127\n")
        && text.as_str().ends_with("B24-U5: byte and length gates\n")
}

/// 锚号校验面。
fn anchor_gate_ok() -> bool {
    let a = Anchor::new(23, b"B24-X", b"x").err() == Some(UA_EANCHOR);
    let b = Anchor::new(25, b"B24-X", b"x").err() == Some(UA_EANCHOR);
    let c = Anchor::new(24, b"", b"x").err() == Some(UA_EMPTY);
    let d = Anchor::new(24, &[b't'; 13], b"x").err() == Some(UA_ELEN);
    let e = Anchor::new(24, &[0x01], b"x").err() == Some(UA_EBYTE);
    let f = Anchor::new(24, b"a/b", b"x").err() == Some(UA_ESLASH);
    let g = Anchor::new(24, b"-x", b"x").err() == Some(UA_EOPT);
    let h = Anchor::new(24, b"B24-OK", &[b'n'; NOTE_CAP + 1]).err() == Some(UA_ELEN);
    let i = Anchor::new(24, b"B24-OK", &[0x01]).err() == Some(UA_EBYTE);
    let j = Anchor::new(24, b"B24-OK", b"ok note").map(|x| x.id()) == Ok(24);
    a && b && c && d && e && f && g && h && i && j
}

/// 锚表封印面。
fn seal_ok() -> bool {
    let a = match standard_catalog() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match standard_catalog() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if a.seal() != b.seal() {
        return false;
    }
    let mut c = match standard_catalog() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let extra = match Anchor::new(24, b"B24-U6", b"extra") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if c.add(extra) != Ok(5) {
        return false;
    }
    if c.seal() == a.seal() {
        return false;
    }
    if c.add(extra) != Err(UA_EDUP) {
        return false;
    }
    c.len() == 6 && a.seal() != 0
}

/// 批量诊断面。
fn batch_ok() -> bool {
    let mut b = Batch::new();
    if !b.is_empty() {
        return false;
    }
    if b.push(b"foo") != Ok(Verdict::Unknown) {
        return false;
    }
    let _ = b.push(b"ls");
    let _ = b.push(b"");
    let _ = b.push(b"-x");
    if b.len() != 4 {
        return false;
    }
    if b.known() != 1 || b.missing() != 2 || b.count("unknown") != 1 {
        return false;
    }
    if b.count("option") != 1 || b.count("empty") != 1 || b.count("slash") != 0 {
        return false;
    }
    if b.at(4).is_some() {
        return false;
    }
    let mut c = Batch::new();
    let mut i = 0usize;
    while i < BATCH_CAP {
        if c.push(b"x").is_err() {
            return false;
        }
        i += 1;
    }
    c.push(b"x").err() == Some(UA_ECAP) && c.len() == BATCH_CAP
}

/// 容量与选项面。
fn cap_option_ok() -> bool {
    let mut c = Catalog::new();
    let mut i = 0usize;
    let mut ok = true;
    while i < ANCHOR_CAP {
        let tag = anchor_tag(i);
        let a = match Anchor::new(24, tag.as_bytes(), b"n") {
            Ok(v) => v,
            Err(_) => {
                ok = false;
                break;
            }
        };
        if c.add(a).is_err() {
            ok = false;
            break;
        }
        i += 1;
    }
    if !ok || c.len() != ANCHOR_CAP {
        return false;
    }
    let ninth = match Anchor::new(24, b"B24-U9", b"n") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if c.add(ninth) != Err(UA_ECAP) {
        return false;
    }
    Anchor::new(24, b"-reserved", b"n").err() == Some(UA_EOPT)
        && Anchor::new(24, b"B24-U9", b"n").is_ok()
}

/// 生成 `B24-U{i}` 形式的标签。
fn anchor_tag(i: usize) -> String {
    let mut s = String::from("B24-U");
    push_u64(&mut s, i as u64);
    s
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    let s1 = l.seal();
    if s1 != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root != s1 || !l2.is_frozen() {
        return false;
    }
    if l2.replay() != Ok(root) {
        return false;
    }
    if l2.register(Sample::blank()) != Err(UA_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = class_ok() && exit_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = render_ok() && hint_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = suggest_ok() && distance_ok() && batch_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = catalog_ok() && anchor_gate_ok() && seal_ok() && cap_option_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11585_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11585.unknown");

    s.add("UA01-七类分类", class_ok(), "static");
    s.add("UA02-退出码表", exit_ok(), "static");
    s.add("UA03-文案渲染", render_ok(), "static");
    s.add("UA04-提示与空名", hint_ok(), "static");
    s.add("UA05-近似建议", suggest_ok(), "static");
    s.add("UA06-编辑距离", distance_ok(), "static");
    s.add("UA07-锚表装载", catalog_ok(), "static");
    s.add("UA08-锚号校验", anchor_gate_ok(), "static");
    s.add("UA09-锚表封印", seal_ok(), "static");
    s.add("UA10-批量诊断", batch_ok(), "static");
    s.add("UA11-容量与选项", cap_option_ok(), "static");
    s.add("UA12-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "UA13-冻结重放四注入",
        {
            let four = inject_four();
            let mut all = true;
            let mut i = 0usize;
            while i < four.len() {
                if let Some(v) = four.get(i) {
                    if !*v {
                        all = false;
                    }
                }
                i += 1;
            }
            all
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_matrix() {
        assert_eq!(classify(b"ls"), Verdict::Known(15));
        assert_eq!(classify(b"grep"), Verdict::Known(11));
        assert_eq!(classify(b"lss"), Verdict::Unknown);
        assert_eq!(classify(b""), Verdict::Empty);
        assert_eq!(classify(b"-x"), Verdict::Option);
        assert_eq!(classify(b"a/b"), Verdict::Slash);
        assert_eq!(classify(&[0x01]), Verdict::BadByte);
        assert_eq!(classify(&[b'z'; 33]), Verdict::TooLong);
    }

    #[test]
    fn exits_follow_shell_convention() {
        assert_eq!(classify(b"nope").exit(), 127);
        assert_eq!(classify(b"ls").exit(), 0);
        assert_eq!(classify(b"-z").exit(), 1);
        assert!(Verdict::Slash.is_missing());
        assert!(!Verdict::Option.is_missing());
    }

    #[test]
    fn message_canonical_text() {
        let m = compose(b"foo");
        assert_eq!(m.body(), "busybox: applet not found");
        assert_eq!(m.hint(), "busybox: try 'busybox --list'");
        assert_eq!(m.len(), 56);
        assert_eq!(
            m.text().as_str(),
            "busybox: applet not found\nbusybox: try 'busybox --list'\n"
        );
        assert!(compose(b"ls").text().is_empty());
    }

    #[test]
    fn slash_message_embeds_name() {
        let m = compose(b"/opt/bin/ls");
        assert!(m.body().contains("/opt/bin/ls"));
        assert!(m.body().ends_with("applet not found"));
        assert_eq!(m.exit(), 127);
    }

    #[test]
    fn suggestions_are_close() {
        assert_eq!(suggest(b"lss"), Some("ls"));
        assert_eq!(suggest(b"gerp"), Some("grep"));
        assert_eq!(suggest(b"Cp"), Some("cp"));
        assert_eq!(suggest(b"ls"), None);
        assert_eq!(suggest(b""), None);
        assert_eq!(suggest(b"-x"), None);
    }

    #[test]
    fn levenshtein_known_values() {
        assert_eq!(distance(b"", b"", 2), 0);
        assert_eq!(distance(b"a", b"", 2), 1);
        assert_eq!(distance(b"abc", b"abd", 2), 1);
        assert_eq!(distance(b"gerp", b"grep", 2), 2);
        assert_eq!(distance(b"kitten", b"sitting", 3), 3);
    }

    #[test]
    fn anchor_stores_text() {
        let a = Anchor::new(24, b"B24-U1", b"unknown applet exit 127").expect("a");
        assert_eq!(a.id(), 24);
        assert_eq!(a.tag_text().as_str(), "B24-U1");
        assert_eq!(a.note_text().as_str(), "unknown applet exit 127");
        assert_eq!(a.text().as_str(), "B24-U1: unknown applet exit 127");
        assert!(a.tagged(b"B24-U1"));
    }

    #[test]
    fn anchor_gates() {
        assert_eq!(Anchor::new(23, b"B24-X", b"x").err(), Some(UA_EANCHOR));
        assert_eq!(Anchor::new(24, b"", b"x").err(), Some(UA_EMPTY));
        assert_eq!(Anchor::new(24, &[b't'; 13], b"x").err(), Some(UA_ELEN));
        assert_eq!(Anchor::new(24, b"a/b", b"x").err(), Some(UA_ESLASH));
        assert_eq!(Anchor::new(24, b"-x", b"x").err(), Some(UA_EOPT));
        assert_eq!(
            Anchor::new(24, b"B24-OK", &[b'n'; NOTE_CAP + 1]).err(),
            Some(UA_ELEN)
        );
    }

    #[test]
    fn catalog_standard() {
        let c = standard_catalog().expect("c");
        assert_eq!(c.len(), 5);
        assert!(c.all_b24());
        assert_eq!(c.find(b"B24-U3").map(|a| a.note_text()), Some(String::from("slash name rejected")));
        assert_eq!(c.at(5), None);
        assert!(c.seal() != 0);
    }

    #[test]
    fn catalog_conflicts() {
        let mut c = standard_catalog().expect("c");
        let dup = Anchor::new(24, b"B24-U1", b"dup").expect("dup");
        assert_eq!(c.add(dup), Err(UA_EDUP));
        assert_eq!(c.len(), 5);
    }

    #[test]
    fn batch_tallies() {
        let mut b = Batch::new();
        assert!(b.is_empty());
        assert_eq!(b.push(b"foo"), Ok(Verdict::Unknown));
        let _ = b.push(b"ls");
        let _ = b.push(b"");
        let _ = b.push(b"-x");
        assert_eq!(b.len(), 4);
        assert_eq!(b.known(), 1);
        assert_eq!(b.missing(), 2);
        assert_eq!(b.count("unknown"), 1);
        assert_eq!(b.count("option"), 1);
    }

    #[test]
    fn batch_capacity() {
        let mut b = Batch::new();
        let mut i = 0usize;
        while i < BATCH_CAP {
            assert!(b.push(b"x").is_ok());
            i += 1;
        }
        assert_eq!(b.push(b"x").err(), Some(UA_ECAP));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(127));
        assert_eq!(l.sample(4).map(|s| s.value), Some(56));
        assert_eq!(l.sample(8).map(|s| s.code), Some(UA_EBYTE));
        assert_eq!(l.sample(13).map(|s| s.code), Some(UA_EANCHOR));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(UA_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(UA_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections_all_true() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11585_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
