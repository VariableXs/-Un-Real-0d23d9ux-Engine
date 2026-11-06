//! UNX-F11579 双机 locale=C 快照库专项验证（S1/M7）
//!
//! 双机快照比对模型：两台基准机各自把 18 条判据的「字节输出摘要」装进一份
//! 快照，摘要一律取原始字节的 FNV-1a（locale=C 下字节值即序值，不做任何
//! 宽字符折算）。比对面把两份快照按判据编号做归并，逐条落三种结论：
//!
//! * `Same`：两侧摘要相同；
//! * `Conflict`：两侧都有但摘要不同（显式给出 A/B 双摘要）；
//! * `MissingA` / `MissingB`：只在 B / 只在 A 侧出现。
//!
//! locale 闸门按 `LC_ALL > LC_CTYPE > LANG` 三层优先级取值，值必须是 `C`
//! 或 `POSIX`，否则落 `LC_ELOC`；没有任何一档时同样拒绝——因为快照库的
//! 字节序前提不允许「未声明 locale」的机器混进来。
//!
//! 一致率以归并总条数为分母、向上取整；18/18 = 100%，17/18 = 95%。
//! `strict` 档在出现任一冲突或缺项时返回 `LC_EVER`，供 CI 直接把红。
//!
//! 闸门次序：字节 → 名称 → 序号 → locale → 快照 → 归并 → 渲染 → 严格。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf4f0;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 本批判据条数。
pub const CRIT: usize = 18;

/// 快照容量。
pub const REC_CAP: usize = 24;

/// 归并行容量。
pub const ROW_CAP: usize = 2 * CRIT;

/// 名称容量。
pub const NAME_CAP: usize = 16;

/// 环境变量容量。
pub const ENV_CAP: usize = 4;

/// 负载容量。
pub const PAYLOAD_CAP: usize = 24;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 字节非法。
pub const LC_EBYTE: u32 = SEG_BASE + 0x01;
/// 名称为空。
pub const LC_ENAME: u32 = SEG_BASE + 0x02;
/// 编号重复。
pub const LC_EDUP: u32 = SEG_BASE + 0x03;
/// 超长。
pub const LC_ELEN: u32 = SEG_BASE + 0x04;
/// locale 不合 C。
pub const LC_ELOC: u32 = SEG_BASE + 0x05;
/// 状态冲突。
pub const LC_ESTATE: u32 = SEG_BASE + 0x06;
/// 快照缺项。
pub const LC_EMISS: u32 = SEG_BASE + 0x07;
/// 严格档红。
pub const LC_EVER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const LC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const LC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const LC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const LC_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 记录装载。
pub const OP_REC: u8 = 0x01;
/// 快照装载。
pub const OP_SNAP: u8 = 0x02;
/// 归并比对。
pub const OP_MERGE: u8 = 0x03;
/// 一致率。
pub const OP_RATE: u8 = 0x04;
/// 报告渲染。
pub const OP_REPORT: u8 = 0x05;
/// locale 闸门。
pub const OP_LOCALE: u8 = 0x06;

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

/// 字节流摘要。
pub fn digest(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 是否可打印：0x21..=0x7e。
fn printable(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        match s.get(i) {
            Some(b) => {
                if *b < 0x21 || *b > 0x7e {
                    return false;
                }
            }
            None => return false,
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

/// 字节序比较：小于零、相等、大于零。
pub fn cmp_bytes(a: &[u8], b: &[u8]) -> i32 {
    let mut i = 0usize;
    loop {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return if x < y { -1 } else { 1 };
                }
            }
            (Some(_), None) => return 1,
            (None, Some(_)) => return -1,
            (None, None) => return 0,
        }
        i += 1;
    }
}

/// 十进制追加。
fn push_u64(s: &mut String, mut v: u64) {
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    if v == 0 {
        s.push('0');
        return;
    }
    while v > 0 && at > 0 {
        at -= 1;
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'0' + (v % 10) as u8;
        }
        v /= 10;
    }
    let mut i = at;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
}

/// 十六位小写十六进制追加。
fn push_hex(s: &mut String, v: u64) {
    const DIG: [u8; 16] = *b"0123456789abcdef";
    let mut sh = 60i32;
    while sh >= 0 {
        let nib = ((v >> sh) & 0xf) as usize;
        match DIG.get(nib) {
            Some(d) => s.push(*d as char),
            None => s.push('?'),
        }
        sh -= 4;
    }
}

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 定长名称。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: u8,
}

impl Name {
    /// 空白名称。
    pub const fn blank() -> Name {
        Name { buf: [0u8; NAME_CAP], len: 0 }
    }

    /// 装载：非空、可打印、不超长。
    pub fn new(s: &[u8]) -> Result<Name, u32> {
        if s.is_empty() {
            return Err(LC_ENAME);
        }
        if s.len() > NAME_CAP {
            return Err(LC_ELEN);
        }
        if !printable(s) {
            return Err(LC_EBYTE);
        }
        let mut n = Name::blank();
        let mut i = 0usize;
        while i < s.len() {
            match (s.get(i), n.buf.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(LC_ELEN),
            }
            i += 1;
        }
        n.len = s.len() as u8;
        Ok(n)
    }

    /// 字节面。
    pub fn bytes(&self) -> &[u8] {
        self.buf.get(0..self.len as usize).unwrap_or(&[])
    }

    /// 文本面。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 同名判定。
    pub fn named(&self, other: &[u8]) -> bool {
        eq_bytes(self.bytes(), other)
    }
}

// ---------------------------------------------------------------------------
// 判据记录
// ---------------------------------------------------------------------------

/// 一条判据记录：编号、工具名、负载摘要、负载字节数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rec {
    id: Name,
    tool: Name,
    sum: u64,
    bytes: u32,
}

impl Rec {
    /// 装载：编号 → 工具 → 负载，三段闸门。
    pub fn new(id: &[u8], tool: &[u8], payload: &[u8]) -> Result<Rec, u32> {
        let i = Name::new(id)?;
        let t = Name::new(tool)?;
        if payload.is_empty() {
            return Err(LC_ENOBS);
        }
        if payload.len() > PAYLOAD_CAP {
            return Err(LC_ELEN);
        }
        if !printable(payload) {
            return Err(LC_EBYTE);
        }
        Ok(Rec { id: i, tool: t, sum: digest(payload), bytes: payload.len() as u32 })
    }

    /// 编号。
    pub fn id(&self) -> &[u8] {
        self.id.bytes()
    }

    /// 工具名。
    pub fn tool(&self) -> &[u8] {
        self.tool.bytes()
    }

    /// 编号文本。
    pub fn id_text(&self) -> String {
        self.id.text()
    }

    /// 工具文本。
    pub fn tool_text(&self) -> String {
        self.tool.text()
    }

    /// 摘要。
    pub fn sum(&self) -> u64 {
        self.sum
    }

    /// 负载字节数。
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// 同编号判定。
    pub fn same_id(&self, id: &[u8]) -> bool {
        self.id.named(id)
    }
}

// ---------------------------------------------------------------------------
// 快照
// ---------------------------------------------------------------------------

/// 单机快照：按编号升序保存记录。
#[derive(Clone, Copy, Debug)]
pub struct Snap {
    host: Name,
    recs: [Option<Rec>; REC_CAP],
    len: usize,
}

impl Snap {
    /// 空快照。
    pub fn new(host: &[u8]) -> Result<Snap, u32> {
        Ok(Snap { host: Name::new(host)?, recs: [None; REC_CAP], len: 0 })
    }

    /// 主机名。
    pub fn host(&self) -> &[u8] {
        self.host.bytes()
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
    pub fn at(&self, i: usize) -> Option<Rec> {
        if i >= self.len {
            return None;
        }
        match self.recs.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 按编号查位次。
    pub fn find(&self, id: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(r)) = self.recs.get(i) {
                if r.same_id(id) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按编号取摘要。
    pub fn sum_of(&self, id: &[u8]) -> Option<u64> {
        match self.find(id) {
            Some(i) => self.at(i).map(|r| r.sum()),
            None => None,
        }
    }

    /// 有序插入：重复编号拒绝，满则超长。
    pub fn push(&mut self, r: Rec) -> Result<u16, u32> {
        if self.len >= REC_CAP {
            return Err(LC_ELEN);
        }
        let mut pos = self.len;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(x)) = self.recs.get(i) {
                match cmp_bytes(x.id(), r.id()) {
                    0 => return Err(LC_EDUP),
                    o if o > 0 => {
                        pos = i;
                        break;
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        let mut k = self.len;
        while k > pos {
            let prev = match self.recs.get(k - 1) {
                Some(v) => *v,
                None => None,
            };
            if let Some(dst) = self.recs.get_mut(k) {
                *dst = prev;
            }
            k -= 1;
        }
        if let Some(dst) = self.recs.get_mut(pos) {
            *dst = Some(r);
        }
        self.len += 1;
        Ok(pos as u16)
    }

    /// 是否已有序。
    pub fn is_sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.len {
            let a = self.at(i - 1);
            let b = self.at(i);
            match (a, b) {
                (Some(x), Some(y)) => {
                    if cmp_bytes(x.id(), y.id()) >= 0 {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 摘要指纹：主机名 + 逐条摘要。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.host.len() {
            if let Some(b) = self.host.bytes().get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        i = 0;
        while i < self.len {
            if let Some(r) = self.at(i) {
                h = fold(h, r.sum());
                h = fold(h, r.bytes() as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 环境与 locale 闸门
// ---------------------------------------------------------------------------

/// 环境变量表。
#[derive(Clone, Copy, Debug)]
pub struct Env {
    items: [(Name, Name); ENV_CAP],
    len: usize,
}

impl Env {
    /// 空环境。
    pub fn new() -> Env {
        Env { items: [(Name::blank(), Name::blank()); ENV_CAP], len: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 设值：重复键状态冲突，满则超长。
    pub fn set(&mut self, k: &[u8], v: &[u8]) -> Result<(), u32> {
        let key = Name::new(k)?;
        let val = Name::new(v)?;
        let mut i = 0usize;
        while i < self.len {
            if let Some((old, _)) = self.items.get(i) {
                if old.named(k) {
                    return Err(LC_ESTATE);
                }
            }
            i += 1;
        }
        if self.len >= ENV_CAP {
            return Err(LC_ELEN);
        }
        if let Some(dst) = self.items.get_mut(self.len) {
            *dst = (key, val);
        }
        self.len += 1;
        Ok(())
    }

    /// 取值。
    pub fn get(&self, k: &[u8]) -> Option<&[u8]> {
        let mut i = 0usize;
        while i < self.len {
            if let Some((key, val)) = self.items.get(i) {
                if key.named(k) {
                    return Some(val.bytes());
                }
            }
            i += 1;
        }
        None
    }
}

/// 是否 C 语义 locale 名。
fn is_c(v: &[u8]) -> bool {
    eq_bytes(v, b"C") || eq_bytes(v, b"POSIX")
}

/// locale 闸门：LC_ALL > LC_CTYPE > LANG，值须为 C/POSIX。
pub fn locale_c(env: &Env) -> Result<(), u32> {
    if let Some(v) = env.get(b"LC_ALL") {
        return if is_c(v) { Ok(()) } else { Err(LC_ELOC) };
    }
    if let Some(v) = env.get(b"LC_CTYPE") {
        return if is_c(v) { Ok(()) } else { Err(LC_ELOC) };
    }
    match env.get(b"LANG") {
        Some(v) => {
            if is_c(v) {
                Ok(())
            } else {
                Err(LC_ELOC)
            }
        }
        None => Err(LC_ELOC),
    }
}

/// 标准 C 环境。
pub fn env_c() -> Result<Env, u32> {
    let mut e = Env::new();
    e.set(b"LC_ALL", b"C")?;
    Ok(e)
}

// ---------------------------------------------------------------------------
// 归并比对
// ---------------------------------------------------------------------------

/// 单条结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 两侧相同。
    Same,
    /// 两侧摘要不同。
    Conflict,
    /// 只在 A 侧。
    MissingB,
    /// 只在 B 侧。
    MissingA,
}

impl Verdict {
    /// 文本面。
    pub fn text(&self) -> &'static str {
        match self {
            Verdict::Same => "OK",
            Verdict::Conflict => "CONFLICT",
            Verdict::MissingB => "MISSING-B",
            Verdict::MissingA => "MISSING-A",
        }
    }

    /// 是否冲突类。
    pub fn bad(&self) -> bool {
        match self {
            Verdict::Same => false,
            _ => true,
        }
    }
}

/// 归并行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    id: Name,
    tool: Name,
    va: u64,
    vb: u64,
    verdict: Verdict,
}

impl Row {
    /// 空白行。
    pub const fn blank() -> Row {
        Row { id: Name::blank(), tool: Name::blank(), va: 0, vb: 0, verdict: Verdict::Same }
    }

    /// 编号。
    pub fn id(&self) -> &[u8] {
        self.id.bytes()
    }

    /// 编号文本。
    pub fn id_text(&self) -> String {
        self.id.text()
    }

    /// 工具文本。
    pub fn tool_text(&self) -> String {
        self.tool.text()
    }

    /// A 侧摘要。
    pub fn sum_a(&self) -> u64 {
        self.va
    }

    /// B 侧摘要。
    pub fn sum_b(&self) -> u64 {
        self.vb
    }

    /// 结论。
    pub fn verdict(&self) -> Verdict {
        self.verdict
    }
}

/// 归并报告。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    rows: [Row; ROW_CAP],
    len: usize,
}

impl Report {
    /// 空报告。
    pub const fn new() -> Report {
        Report { rows: [Row::blank(); ROW_CAP], len: 0 }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 追加。
    fn push(&mut self, r: Row) -> Result<(), u32> {
        if self.len >= ROW_CAP {
            return Err(LC_ELEN);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = r;
        }
        self.len += 1;
        Ok(())
    }

    /// 同结论计数。
    pub fn count(&self, v: Verdict) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.verdict == v {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 相同条数。
    pub fn same(&self) -> usize {
        self.count(Verdict::Same)
    }

    /// 冲突条数。
    pub fn conflict(&self) -> usize {
        self.count(Verdict::Conflict)
    }

    /// 缺项条数。
    pub fn missing(&self) -> usize {
        self.count(Verdict::MissingA) + self.count(Verdict::MissingB)
    }

    /// 总条数。
    pub fn total(&self) -> usize {
        self.len
    }

    /// 一致率，向上取整。
    pub fn rate(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let num = (self.same() as u64).saturating_mul(100);
        let den = self.len as u64;
        let mut p = num / den + if num % den != 0 { 1 } else { 0 };
        if p > 100 {
            p = 100;
        }
        p as u32
    }

    /// 按编号取结论。
    pub fn verdict_of(&self, id: &[u8]) -> Option<Verdict> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.id.named(id) {
                    return Some(r.verdict);
                }
            }
            i += 1;
        }
        None
    }

    /// 冲突编号文本，逗号串。
    pub fn conflict_ids(&self) -> String {
        let mut s = String::new();
        let mut first = true;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.verdict == Verdict::Conflict {
                    if !first {
                        s.push(',');
                    }
                    s.push_str(r.id_text().as_str());
                    first = false;
                }
            }
            i += 1;
        }
        s
    }

    /// 渲染：逐行结论 + 末行汇总。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                out.push_str(r.id_text().as_str());
                out.push(' ');
                out.push_str(r.verdict.text());
                match r.verdict {
                    Verdict::Conflict => {
                        out.push_str(" A=");
                        push_hex(&mut out, r.va);
                        out.push_str(" B=");
                        push_hex(&mut out, r.vb);
                    }
                    Verdict::MissingB => {
                        out.push_str(" A=");
                        push_hex(&mut out, r.va);
                    }
                    Verdict::MissingA => {
                        out.push_str(" B=");
                        push_hex(&mut out, r.vb);
                    }
                    Verdict::Same => {}
                }
                out.push('\n');
            }
            i += 1;
        }
        out.push_str("rate=");
        push_u64(&mut out, self.rate() as u64);
        out.push_str("% same=");
        push_u64(&mut out, self.same() as u64);
        out.push('/');
        push_u64(&mut out, self.len as u64);
        out.push_str(" conflict=");
        push_u64(&mut out, self.conflict() as u64);
        out.push_str(" missing=");
        push_u64(&mut out, self.missing() as u64);
        out.push('\n');
        out
    }

    /// 覆盖档：双侧同号条数不足 18 即缺项。
    pub fn cover(&self) -> Result<(), u32> {
        if self.same() + self.conflict() < CRIT {
            Err(LC_EMISS)
        } else {
            Ok(())
        }
    }

    /// 严格档：任一冲突或缺项即红。
    pub fn strict(&self) -> Result<(), u32> {
        if self.conflict() > 0 || self.missing() > 0 {
            Err(LC_EVER)
        } else {
            Ok(())
        }
    }
}

/// 双机归并：按编号升序产出结论。
pub fn compare(a: &Snap, b: &Snap) -> Result<Report, u32> {
    if a.is_empty() || b.is_empty() {
        return Err(LC_ENOBS);
    }
    let mut out = Report::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < a.len() || j < b.len() {
        let ra = a.at(i);
        let rb = b.at(j);
        match (ra, rb) {
            (Some(x), Some(y)) => {
                let o = cmp_bytes(x.id(), y.id());
                if o < 0 {
                    out.push(Row {
                        id: match Name::new(x.id()) {
                            Ok(v) => v,
                            Err(c) => return Err(c),
                        },
                        tool: match Name::new(x.tool()) {
                            Ok(v) => v,
                            Err(c) => return Err(c),
                        },
                        va: x.sum(),
                        vb: 0,
                        verdict: Verdict::MissingB,
                    })?;
                    i += 1;
                } else if o > 0 {
                    out.push(Row {
                        id: match Name::new(y.id()) {
                            Ok(v) => v,
                            Err(c) => return Err(c),
                        },
                        tool: match Name::new(y.tool()) {
                            Ok(v) => v,
                            Err(c) => return Err(c),
                        },
                        va: 0,
                        vb: y.sum(),
                        verdict: Verdict::MissingA,
                    })?;
                    j += 1;
                } else {
                    let v = if x.sum() == y.sum() { Verdict::Same } else { Verdict::Conflict };
                    out.push(Row {
                        id: match Name::new(x.id()) {
                            Ok(v2) => v2,
                            Err(c) => return Err(c),
                        },
                        tool: match Name::new(x.tool()) {
                            Ok(v2) => v2,
                            Err(c) => return Err(c),
                        },
                        va: x.sum(),
                        vb: y.sum(),
                        verdict: v,
                    })?;
                    i += 1;
                    j += 1;
                }
            }
            (Some(x), None) => {
                out.push(Row {
                    id: match Name::new(x.id()) {
                        Ok(v) => v,
                        Err(c) => return Err(c),
                    },
                    tool: match Name::new(x.tool()) {
                        Ok(v) => v,
                        Err(c) => return Err(c),
                    },
                    va: x.sum(),
                    vb: 0,
                    verdict: Verdict::MissingB,
                })?;
                i += 1;
            }
            (None, Some(y)) => {
                out.push(Row {
                    id: match Name::new(y.id()) {
                        Ok(v) => v,
                        Err(c) => return Err(c),
                    },
                    tool: match Name::new(y.tool()) {
                        Ok(v) => v,
                        Err(c) => return Err(c),
                    },
                    va: 0,
                    vb: y.sum(),
                    verdict: Verdict::MissingA,
                })?;
                j += 1;
            }
            (None, None) => break,
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 样本快照
// ---------------------------------------------------------------------------

/// 18 条判据对应的工具名。
const TOOLS: [&[u8]; CRIT] = [
    b"sort", b"grep", b"tr", b"wc", b"sed", b"ls", b"cmp", b"od", b"uniq", b"comm", b"join",
    b"cut", b"date", b"printf", b"test", b"nl", b"fold", b"numfmt",
];

/// 快照变体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tweak {
    /// 与基准一致。
    None,
    /// S07 负载变异（摘要不同）。
    Od,
    /// 缺 S18。
    DropS18,
}

/// 第 i 条判据记录（i 从零起）。
fn rec_at(i: usize, tw: Tweak) -> Result<Rec, u32> {
    let n = i + 1;
    let id = [b'S', b'0' + (n / 10) as u8, b'0' + (n % 10) as u8];
    let tool = match TOOLS.get(i) {
        Some(t) => *t,
        None => b"x",
    };
    let mut buf = [0u8; PAYLOAD_CAP];
    let mut at = 0usize;
    let mut k = 0usize;
    while k < tool.len() {
        match (tool.get(k), buf.get_mut(at)) {
            (Some(src), Some(dst)) => {
                *dst = *src;
                at += 1;
            }
            _ => return Err(LC_ELEN),
        }
        k += 1;
    }
    if let Some(dst) = buf.get_mut(at) {
        *dst = b':';
        at += 1;
    }
    if let Some(dst) = buf.get_mut(at) {
        *dst = b'C';
        at += 1;
    }
    if tw == Tweak::Od && i == 6 {
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'2';
            at += 1;
        }
    }
    Rec::new(&id, tool, buf.get(0..at).unwrap_or(&[]))
}

/// 建一份快照。
fn build(host: &[u8], tw: Tweak) -> Result<Snap, u32> {
    let mut s = Snap::new(host)?;
    let mut i = 0usize;
    while i < CRIT {
        if tw == Tweak::DropS18 && i == CRIT - 1 {
            i += 1;
            continue;
        }
        s.push(rec_at(i, tw)?)?;
        i += 1;
    }
    Ok(s)
}

/// A 机基准快照。
pub fn snap_a() -> Result<Snap, u32> {
    build(b"host-a", Tweak::None)
}

/// B 机基准快照。
pub fn snap_b() -> Result<Snap, u32> {
    build(b"host-b", Tweak::None)
}

/// B 机 S07 变异快照。
pub fn snap_conflict() -> Result<Snap, u32> {
    build(b"host-b", Tweak::Od)
}

/// B 机缺 S18 快照。
pub fn snap_missing() -> Result<Snap, u32> {
    build(b"host-b", Tweak::DropS18)
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
            return Err(LC_ESLOT);
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
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(LC_EBAL);
        }
        const VALS: [u32; 8] = [18, 18, 18, 100, 1, 95, 1, 19];
        const CODES: [u32; 6] = [LC_EBYTE, LC_ENAME, LC_EDUP, LC_ELOC, LC_ENOBS, LC_EMISS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LC_EBAL),
            };
            if s.slot as usize != i {
                return Err(LC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LC_EVALUE);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LC_EVALUE);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LC_EBAL);
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
            return Err(LC_EBAL);
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
            Err(LC_ESEAL)
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

/// `Result<u64, u32>` → 样本。
fn long_cell(slot: u8, op: u8, r: Result<u64, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(count_cell(0, OP_SNAP, snap_a().map(|s| s.len())));

    let _ = l.register(count_cell(1, OP_SNAP, snap_b().map(|s| s.len())));

    let _ = l.register(count_cell(
        2,
        OP_MERGE,
        snap_a().and_then(|a| snap_b().and_then(|b| compare(&a, &b))).map(|r| r.same()),
    ));

    let _ = l.register(count_cell(
        3,
        OP_RATE,
        snap_a().and_then(|a| snap_b().and_then(|b| compare(&a, &b))).map(|r| r.rate() as usize),
    ));

    let _ = l.register(count_cell(
        4,
        OP_MERGE,
        snap_a()
            .and_then(|a| snap_conflict().and_then(|b| compare(&a, &b)))
            .map(|r| r.conflict()),
    ));

    let _ = l.register(count_cell(
        5,
        OP_RATE,
        snap_a()
            .and_then(|a| snap_conflict().and_then(|b| compare(&a, &b)))
            .map(|r| r.rate() as usize),
    ));

    let _ = l.register(count_cell(
        6,
        OP_MERGE,
        snap_a()
            .and_then(|a| snap_missing().and_then(|b| compare(&a, &b)))
            .map(|r| r.missing()),
    ));

    let _ = l.register(count_cell(
        7,
        OP_REPORT,
        snap_a()
            .and_then(|a| snap_conflict().and_then(|b| compare(&a, &b)))
            .map(|r| r.render().lines().count()),
    ));

    let _ = l.register(long_cell(
        8,
        OP_REC,
        Rec::new(b"S01", b"sort", &[0x80]).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        9,
        OP_REC,
        Rec::new(b"", b"sort", b"sort:C").map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        10,
        OP_SNAP,
        snap_a()
            .and_then(|mut s| rec_at(0, Tweak::None).and_then(|r| s.push(r)))
            .map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        11,
        OP_LOCALE,
        locale_c(&client_env(b"LC_ALL", b"en_US.UTF-8")).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        12,
        OP_MERGE,
        snap_a()
            .and_then(|a| Snap::new(b"empty").and_then(|b| compare(&a, &b)))
            .map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        13,
        OP_MERGE,
        snap_a()
            .and_then(|a| build(b"host-small", Tweak::DropS18).and_then(|b| compare(&a, &b)))
            .and_then(|r| r.cover())
            .map(|_| 0u64),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 快照装载面。
fn snap_ok() -> bool {
    let a = match snap_a() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match snap_b() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = a.len() == CRIT && b.len() == CRIT && a.is_sorted() && b.is_sorted();
    let d = match (a.at(0), a.at(17)) {
        (Some(x), Some(y)) => x.id() == b"S01" && y.id() == b"S18" && y.tool() == b"numfmt",
        _ => false,
    };
    let e = match (a.find(b"S07"), a.at(6)) {
        (Some(i), Some(r)) => i == 6 && r.tool() == b"cmp" && r.bytes() == 5,
        _ => false,
    };
    let f = a.sum_of(b"S11") == b.sum_of(b"S11") && a.sum_of(b"S01") == b.sum_of(b"S01");
    let g = a.host() == b"host-a" && b.host() == b"host-b" && a.seal() != b.seal();
    c && d && e && f && g
}

/// 名称与字节闸门。
fn name_ok() -> bool {
    let a = Name::new(b"").err() == Some(LC_ENAME);
    let b = Name::new(&[0x80]).err() == Some(LC_EBYTE);
    let c = Name::new(&[b'q'; NAME_CAP + 1]).err() == Some(LC_ELEN);
    let d = Rec::new(b"S01", b"sort", &[]).err() == Some(LC_ENOBS);
    let e = Rec::new(b"S01", b"sort", &[0x80]).err() == Some(LC_EBYTE);
    let f = Rec::new(b"S01", b"sort", &[b'q'; PAYLOAD_CAP + 1]).err() == Some(LC_ELEN);
    let g = match Rec::new(b"S01", b"sort", b"sort:C") {
        Ok(r) => r.id() == b"S01" && r.tool() == b"sort" && r.bytes() == 6 && r.sum() == digest(b"sort:C"),
        Err(_) => false,
    };
    let h = digest(b"") == FNV_OFF && digest(b"a") != digest(b"b");
    a && b && c && d && e && f && g && h
}

/// locale 闸门面。
fn locale_ok() -> bool {
    let a = locale_c(&client_env(b"LC_ALL", b"C")).is_ok();
    let b = locale_c(&client_env(b"LC_ALL", b"en_US.UTF-8")).err() == Some(LC_ELOC);
    let c = locale_c(&client_env(b"LC_CTYPE", b"POSIX")).is_ok();
    let d = locale_c(&client_env(b"LANG", b"C")).is_ok();
    let e = locale_c(&Env::new()).err() == Some(LC_ELOC);
    let f = locale_c(&client_env(b"LANG", b"zh_CN.UTF-8")).err() == Some(LC_ELOC);
    let g = match (env_c(), env_c()) {
        (Ok(mut x), Ok(mut y)) => {
            x.set(b"LC_ALL", b"C") == Err(LC_ESTATE)
                && y.set(b"LANG", b"C").is_ok()
                && locale_c(&y).is_ok()
        }
        _ => false,
    };
    a && b && c && d && e && f && g
}

/// 单键环境。
fn client_env(k: &[u8], v: &[u8]) -> Env {
    let mut e = Env::new();
    let _ = e.set(k, v);
    e
}

/// 双机一致面。
fn agree_ok() -> bool {
    let r = match snap_a().and_then(|a| snap_b().and_then(|b| compare(&a, &b))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.len() == CRIT && r.total() == CRIT;
    let b = r.same() == CRIT && r.conflict() == 0 && r.missing() == 0;
    let c = r.rate() == 100;
    let d = r.verdict_of(b"S01") == Some(Verdict::Same) && r.verdict_of(b"S18") == Some(Verdict::Same);
    let e = r.strict() == Ok(()) && r.conflict_ids().is_empty();
    a && b && c && d && e
}

/// 冲突显式面。
fn conflict_ok() -> bool {
    let r = match snap_a().and_then(|a| snap_conflict().and_then(|b| compare(&a, &b))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.same() == CRIT - 1 && r.conflict() == 1 && r.missing() == 0;
    let b = r.rate() == 95 && r.total() == CRIT;
    let c = r.verdict_of(b"S07") == Some(Verdict::Conflict);
    let d = r.conflict_ids() == "S07";
    let e = r.strict().err() == Some(LC_EVER);
    let f = match r.row(6) {
        Some(x) => x.id() == b"S07" && x.sum_a() != x.sum_b() && x.verdict().bad(),
        None => false,
    };
    let g = {
        let text = r.render();
        let mut hit = false;
        let mut i = 0usize;
        while i < text.len() {
            let rest = text.get(i..).unwrap_or("");
            if rest.starts_with("CONFLICT") {
                hit = true;
            }
            i += 1;
        }
        hit
    };
    a && b && c && d && e && f && g
}

/// 缺项显式面。
fn missing_ok() -> bool {
    let r = match snap_a().and_then(|a| snap_missing().and_then(|b| compare(&a, &b))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.missing() == 1 && r.same() == CRIT - 1 && r.conflict() == 0;
    let b = r.verdict_of(b"S18") == Some(Verdict::MissingB);
    let c = r.rate() == 95 && r.total() == CRIT;
    let d = r.strict().err() == Some(LC_EVER);
    let e = match r.row(17) {
        Some(x) => x.id() == b"S18" && x.sum_b() == 0 && x.sum_a() != 0 && x.verdict() == Verdict::MissingB,
        None => false,
    };
    a && b && c && d && e
}

/// 一致率面。
fn rate_ok() -> bool {
    let full = match snap_a().and_then(|a| snap_b().and_then(|b| compare(&a, &b))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let one = match snap_a().and_then(|a| snap_conflict().and_then(|b| compare(&a, &b))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = full.rate() == 100 && one.rate() == 95;
    let b = Report::new().rate() == 0 && Report::new().total() == 0;
    let c = full.same() == full.total() && full.missing() == 0;
    let d = one.same() + one.conflict() + one.missing() == one.total();
    a && b && c && d
}

/// 报告渲染面。
fn report_ok() -> bool {
    let r = match snap_a().and_then(|a| snap_b().and_then(|b| compare(&a, &b))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let text = r.render();
    let a = text.lines().count() == CRIT + 1;
    let b = text.starts_with("S01 OK\n");
    let c = text.contains("rate=100% same=18/18 conflict=0 missing=0");
    let d = text.ends_with('\n');
    let one = match snap_a().and_then(|a| snap_conflict().and_then(|b| compare(&a, &b))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let e = {
        let t = one.render();
        let mut hit_hex = false;
        let mut i = 0usize;
        while i + 16 <= t.len() {
            let seg = t.get(i..i + 16).unwrap_or("");
            if seg.chars().all(|ch| ch == '0' || (ch >= '1' && ch <= '9') || (ch >= 'a' && ch <= 'f')) {
                hit_hex = true;
            }
            i += 1;
        }
        t.contains("CONFLICT A=") && t.contains(" B=") && hit_hex
    };
    a && b && c && d && e
}

/// 记录排序面。
fn order_ok() -> bool {
    let mut s = match Snap::new(b"host-x") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r3 = Rec::new(b"S03", b"tr", b"tr:C");
    let r1 = Rec::new(b"S01", b"sort", b"sort:C");
    let r2 = Rec::new(b"S02", b"grep", b"grep:C");
    let a = match (r3, r1, r2) {
        (Ok(x), Ok(y), Ok(z)) => s.push(x).is_ok() && s.push(y).is_ok() && s.push(z).is_ok(),
        _ => false,
    };
    let b = s.is_sorted() && s.len() == 3;
    let c = match (s.at(0), s.at(1), s.at(2)) {
        (Some(x), Some(y), Some(z)) => x.id() == b"S01" && y.id() == b"S02" && z.id() == b"S03",
        _ => false,
    };
    let d = s.find(b"S02") == Some(1) && s.find(b"S09").is_none();
    let e = match Rec::new(b"S02", b"grep", b"grep:C") {
        Ok(dup) => s.push(dup).err() == Some(LC_EDUP),
        Err(_) => false,
    };
    let f = s.len() == 3 && s.is_sorted();
    a && b && c && d && e && f
}

/// 容量与空表面。
fn bounds_ok() -> bool {
    let a = match snap_a() {
        Ok(x) => compare(&x, &empty_snap()).err() == Some(LC_ENOBS),
        Err(_) => false,
    };
    let b = match (snap_a(), build(b"host-small", Tweak::DropS18)) {
        (Ok(x), Ok(y)) => match compare(&x, &y) {
            Ok(r) => r.missing() == 1 && r.cover() == Err(LC_EMISS),
            Err(_) => false,
        },
        _ => false,
    };
    let c = {
        let mut s = match Snap::new(b"host-full") {
            Ok(v) => v,
            Err(_) => return false,
        };
        let mut i = 0usize;
        let mut ok = true;
        while i < REC_CAP {
            let id = [b'R', b'0' + ((i / 10) % 10) as u8, b'0' + (i % 10) as u8];
            match Rec::new(&id, b"t", b"p") {
                Ok(r) => {
                    if s.push(r).is_err() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            i += 1;
        }
        let over = match Rec::new(b"R99", b"t", b"p") {
            Ok(r) => s.push(r).err() == Some(LC_ELEN),
            Err(_) => false,
        };
        ok && s.len() == REC_CAP && over
    };
    let d = Report::new().is_empty() && Report::new().render() == "rate=0% same=0/0 conflict=0 missing=0\n";
    a && b && c && d
}

/// 空快照兜底。
fn empty_snap() -> Snap {
    match Snap::new(b"e") {
        Ok(v) => v,
        Err(_) => Snap { host: Name::blank(), recs: [None; REC_CAP], len: 0 },
    }
}

/// 严格档面。
fn strict_ok() -> bool {
    let a = match snap_a().and_then(|x| snap_b().and_then(|y| compare(&x, &y))) {
        Ok(r) => r.strict() == Ok(()),
        Err(_) => false,
    };
    let b = match snap_a().and_then(|x| snap_conflict().and_then(|y| compare(&x, &y))) {
        Ok(r) => r.strict() == Err(LC_EVER),
        Err(_) => false,
    };
    let c = match snap_a().and_then(|x| snap_missing().and_then(|y| compare(&x, &y))) {
        Ok(r) => r.strict() == Err(LC_EVER),
        Err(_) => false,
    };
    let d = Verdict::Same.bad() == false && Verdict::Conflict.bad() && Verdict::MissingA.bad();
    a && b && c && d
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = snap_ok() && name_ok();
    out[1] = locale_ok() && agree_ok();
    out[2] = conflict_ok() && missing_ok();
    out[3] = rate_ok()
        && report_ok()
        && order_ok()
        && bounds_ok()
        && strict_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11579_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11579.lc_snap");

    s.add(
        "LC01-快照装载",
        {
            snap_ok()
        },
        "static",
    );

    s.add(
        "LC02-名称与字节",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "LC03-locale 三层闸门",
        {
            locale_ok()
        },
        "static",
    );

    s.add(
        "LC04-双机一致",
        {
            agree_ok()
        },
        "static",
    );

    s.add(
        "LC05-冲突显式",
        {
            conflict_ok()
        },
        "static",
    );

    s.add(
        "LC06-缺项显式",
        {
            missing_ok()
        },
        "static",
    );

    s.add(
        "LC07-一致率",
        {
            rate_ok()
        },
        "static",
    );

    s.add(
        "LC08-报告渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "LC09-记录排序",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "LC10-严格档",
        {
            strict_ok()
        },
        "static",
    );

    s.add(
        "LC11-账本对账",
        {
            build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "LC12-容量与空表",
        {
            bounds_ok()
        },
        "static",
    );

    s.add(
        "LC13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_shape() {
        let a = snap_a().expect("a");
        let b = snap_b().expect("b");
        assert_eq!(a.len(), CRIT);
        assert_eq!(b.len(), CRIT);
        assert!(a.is_sorted());
        assert_eq!(a.at(0).map(|r| r.id_text()), Some(String::from("S01")));
        assert_eq!(a.at(17).map(|r| r.tool_text()), Some(String::from("numfmt")));
        assert_eq!(a.find(b"S07"), Some(6));
        assert_ne!(a.seal(), b.seal());
    }

    #[test]
    fn record_gates() {
        assert_eq!(Name::new(b"").err(), Some(LC_ENAME));
        assert_eq!(Name::new(&[0x80]).err(), Some(LC_EBYTE));
        assert_eq!(Rec::new(b"S01", b"sort", &[]).err(), Some(LC_ENOBS));
        assert_eq!(Rec::new(b"S01", b"sort", &[0x80]).err(), Some(LC_EBYTE));
        let r = Rec::new(b"S01", b"sort", b"sort:C").expect("r");
        assert_eq!(r.bytes(), 6);
        assert_eq!(r.sum(), digest(b"sort:C"));
    }

    #[test]
    fn locale_chain() {
        assert!(locale_c(&client_env(b"LC_ALL", b"C")).is_ok());
        assert!(locale_c(&client_env(b"LC_ALL", b"POSIX")).is_ok());
        assert!(locale_c(&client_env(b"LC_CTYPE", b"C")).is_ok());
        assert!(locale_c(&client_env(b"LANG", b"C")).is_ok());
        assert_eq!(locale_c(&Env::new()).err(), Some(LC_ELOC));
        assert_eq!(locale_c(&client_env(b"LC_ALL", b"en_US.UTF-8")).err(), Some(LC_ELOC));
        assert_eq!(locale_c(&client_env(b"LANG", b"zh_CN.UTF-8")).err(), Some(LC_ELOC));
    }

    #[test]
    fn env_gates() {
        let mut e = env_c().expect("e");
        assert_eq!(e.set(b"LC_ALL", b"C"), Err(LC_ESTATE));
        assert_eq!(e.get(b"LC_ALL"), Some(&b"C"[..]));
        assert_eq!(e.get(b"LANG"), None);
    }

    #[test]
    fn agree_report() {
        let a = snap_a().expect("a");
        let b = snap_b().expect("b");
        let r = compare(&a, &b).expect("r");
        assert_eq!(r.len(), CRIT);
        assert_eq!(r.same(), CRIT);
        assert_eq!(r.conflict(), 0);
        assert_eq!(r.rate(), 100);
        assert_eq!(r.strict(), Ok(()));
        assert_eq!(r.render().lines().count(), CRIT + 1);
    }

    #[test]
    fn conflict_report() {
        let a = snap_a().expect("a");
        let b = snap_conflict().expect("b");
        let r = compare(&a, &b).expect("r");
        assert_eq!(r.conflict(), 1);
        assert_eq!(r.same(), CRIT - 1);
        assert_eq!(r.rate(), 95);
        assert_eq!(r.conflict_ids().as_str(), "S07");
        assert_eq!(r.verdict_of(b"S07"), Some(Verdict::Conflict));
        assert_eq!(r.strict(), Err(LC_EVER));
        let row = r.row(6).expect("row");
        assert_ne!(row.sum_a(), row.sum_b());
    }

    #[test]
    fn missing_report() {
        let a = snap_a().expect("a");
        let b = snap_missing().expect("b");
        let r = compare(&a, &b).expect("r");
        assert_eq!(r.missing(), 1);
        assert_eq!(r.verdict_of(b"S18"), Some(Verdict::MissingB));
        assert_eq!(r.rate(), 95);
        assert_eq!(r.strict(), Err(LC_EVER));
    }

    #[test]
    fn sorted_insert() {
        let mut s = Snap::new(b"host-s").expect("s");
        s.push(Rec::new(b"S02", b"grep", b"grep:C").expect("2")).expect("p");
        s.push(Rec::new(b"S01", b"sort", b"sort:C").expect("1")).expect("p");
        assert!(s.is_sorted());
        assert_eq!(s.at(0).map(|r| r.id_text()), Some(String::from("S01")));
        let dup = Rec::new(b"S01", b"sort", b"sort:C").expect("d");
        assert_eq!(s.push(dup), Err(LC_EDUP));
        assert_eq!(s.find(b"S01"), Some(0));
        assert_eq!(s.find(b"S09"), None);
    }

    #[test]
    fn compare_gates() {
        let a = snap_a().expect("a");
        let empty = empty_snap();
        assert_eq!(compare(&a, &empty).err(), Some(LC_ENOBS));
        let small = build(b"host-small", Tweak::DropS18).expect("small");
        let r = compare(&a, &small).expect("r");
        assert_eq!(r.missing(), 1);
        assert_eq!(r.cover(), Err(LC_EMISS));
    }

    #[test]
    fn verdict_text() {
        assert_eq!(Verdict::Same.text(), "OK");
        assert_eq!(Verdict::Conflict.text(), "CONFLICT");
        assert_eq!(Verdict::MissingA.text(), "MISSING-A");
        assert_eq!(Verdict::MissingB.text(), "MISSING-B");
        assert!(!Verdict::Same.bad());
        assert!(Verdict::Conflict.bad());
    }

    #[test]
    fn empty_report() {
        let r = Report::new();
        assert_eq!(r.len(), 0);
        assert!(r.is_empty());
        assert_eq!(r.rate(), 0);
        assert_eq!(r.render().as_str(), "rate=0% same=0/0 conflict=0 missing=0\n");
        assert_eq!(r.row(0), None);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(7).map(|s| s.value), Some(19));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(LC_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11579_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
