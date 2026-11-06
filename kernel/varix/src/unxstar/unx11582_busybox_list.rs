//! UNX-F11582 busybox --list 清单判据（全量清单与基准机一致）
//!
//! `busybox --list` 输出的不是「一个装饰性列表」，而是**这台机器上可用的
//! applet 全集**：脚本用 `busybox --list | grep -qx ls` 判断能力，打包器用
//! 差集判断裁剪版缺了谁。判据因此不能只看「打印了 40 行」，必须逐条钉住：
//!
//! ```text
//! 基线  40 条，C 序（LC_ALL=C 的逐字节升序），不是 F11204 的自举序
//! 形状  每行一个名，短名、无斜杠、无空白、无重复
//! 顺序  严格递增；乱序即拒，不做「帮你排好」的宽容
//! 渲染  --list 出裸名，--list-full 出 /bin/<name> 全路径
//! 对拍  与基准机清单做归并 diff，缺项/多项各自计数并留首个样本
//! ```
//!
//! 闸门次序：字节 → 长度 → 容量 → 重名 → 字典序 → 形状 → 渲染 → 对拍。
//!
//! 为什么要区分「自举序」与「清单序」：F11204 的 40 条按自举依赖排（sh 在
//! 首位），而 `--list` 走的是 `applet_names[]` 的**字典序**（ash 在首位），
//! 两者序不同、集相同。把这两个序混为一谈，会让「清单一致」的判据在自举序
//! 下假绿——本模块的 `LB09` 专门钉这一点：自举序清单必须被判为 EORDER。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf970;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单名容量。
pub const NAME_CAP: usize = 32;

/// 清单行容量。
pub const CAP: usize = 48;

/// 基线 applet 数。
pub const BASELINE_LEN: usize = 40;

/// 全长清单的前缀。
pub const FULL_PREFIX: &str = "/bin/";

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 非打印字节。
pub const LB_EBYTE: u32 = SEG_BASE + 0x01;
/// 单名超长。
pub const LB_ELEN: u32 = SEG_BASE + 0x02;
/// 行数超容。
pub const LB_ECAP: u32 = SEG_BASE + 0x03;
/// 重名。
pub const LB_EDUP: u32 = SEG_BASE + 0x04;
/// 非字典序。
pub const LB_EORDER: u32 = SEG_BASE + 0x05;
/// 选项非法。
pub const LB_EOPT: u32 = SEG_BASE + 0x06;
/// 多余操作数。
pub const LB_EOPERAND: u32 = SEG_BASE + 0x07;
/// 形状非法（空名 / 含斜杠）。
pub const LB_ESHAPE: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const LB_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const LB_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const LB_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LB_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const LB_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 清单解析。
pub const OP_PARSE: u8 = 0x01;
/// 字典序统计。
pub const OP_SORT: u8 = 0x02;
/// 基线对拍。
pub const OP_DIFF: u8 = 0x03;
/// 渲染。
pub const OP_RENDER: u8 = 0x04;
/// 选项。
pub const OP_OPT: u8 = 0x05;
/// 形状统计。
pub const OP_SHAPE: u8 = 0x06;

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

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 可打印（0x21..=0x7e）。
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

/// 逐字节三态比较：-1 小、0 等、1 大。
fn cmp_bytes(a: &[u8], b: &[u8]) -> i8 {
    let mut i = 0usize;
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    while i < n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x < y {
            return -1;
        }
        if x > y {
            return 1;
        }
        i += 1;
    }
    if a.len() < b.len() {
        -1
    } else if a.len() > b.len() {
        1
    } else {
        0
    }
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

// ---------------------------------------------------------------------------
// 名
// ---------------------------------------------------------------------------

/// applet 名（定容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    bytes: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 空白。
    pub const fn blank() -> Name {
        Name { bytes: [0u8; NAME_CAP], len: 0 }
    }

    /// 构造：空 → `ESHAPE`，超长 → `ELEN`，非打印 → `EBYTE`，含斜杠 → `ESHAPE`。
    pub fn new(s: &[u8]) -> Result<Name, u32> {
        if s.is_empty() {
            return Err(LB_ESHAPE);
        }
        if s.len() > NAME_CAP {
            return Err(LB_ELEN);
        }
        if !printable(s) {
            return Err(LB_EBYTE);
        }
        let mut i = 0usize;
        while i < s.len() {
            if let Some(b) = s.get(i) {
                if *b == b'/' || *b == b'\\' {
                    return Err(LB_ESHAPE);
                }
            }
            i += 1;
        }
        let mut n = Name::blank();
        let mut k = 0usize;
        while k < s.len() {
            match (s.get(k), n.bytes.get_mut(k)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(LB_ELEN),
            }
            k += 1;
        }
        n.len = s.len();
        Ok(n)
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(0..self.len).unwrap_or(&[])
    }

    /// 文本。
    pub fn text(&self) -> String {
        text_of(self.bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否同名。
    pub fn named(&self, s: &[u8]) -> bool {
        eq_bytes(self.bytes(), s)
    }
}

// ---------------------------------------------------------------------------
// 基线：C 序 40 条（与基准机 --list 输出同序）
// ---------------------------------------------------------------------------

/// 基准机 `busybox --list` 的逐行内容（严格 C 序）。
pub const BASELINE: [&str; BASELINE_LEN] = [
    "ash", "awk", "cat", "chmod", "chown", "cp", "cut", "date", "dd", "df",
    "du", "echo", "expr", "find", "grep", "head", "kill", "ln", "ls", "mkdir",
    "mount", "mv", "printf", "ps", "rm", "rmdir", "sed", "sh", "sleep", "sort",
    "stat", "tail", "tar", "tee", "test", "touch", "tr", "umount", "uniq", "wc",
];

// ---------------------------------------------------------------------------
// 清单
// ---------------------------------------------------------------------------

/// 解析后的 `--list` 清单。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Manifest {
    rows: [Name; CAP],
    len: usize,
}

impl Manifest {
    /// 空清单。
    pub const fn new() -> Manifest {
        Manifest { rows: [Name::blank(); CAP], len: 0 }
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
    pub fn row(&self, i: usize) -> Option<Name> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 末行。
    pub fn last(&self) -> Option<Name> {
        if self.is_empty() {
            None
        } else {
            self.row(self.len.saturating_sub(1))
        }
    }

    /// 追加一行：形状 → 长度 → 重名 → 顺序 → 容量，五闸依次。
    pub fn push(&mut self, s: &[u8]) -> Result<usize, u32> {
        let n = Name::new(s)?;
        if self.contains(n.bytes()) {
            return Err(LB_EDUP);
        }
        if let Some(prev) = self.last() {
            if cmp_bytes(prev.bytes(), n.bytes()) >= 0 {
                return Err(LB_EORDER);
            }
        }
        if self.len >= CAP {
            return Err(LB_ECAP);
        }
        let idx = self.len;
        if let Some(dst) = self.rows.get_mut(idx) {
            *dst = n;
        }
        self.len += 1;
        Ok(idx)
    }

    /// 按文本解析：`\n` 分隔，允许末尾换行，不允许空行。
    pub fn parse(text: &[u8]) -> Result<Manifest, u32> {
        let mut m = Manifest::new();
        let mut start = 0usize;
        let mut i = 0usize;
        while i < text.len() {
            if let Some(b) = text.get(i) {
                if *b == b'\n' {
                    let line = text.get(start..i).unwrap_or(&[]);
                    m.push(line)?;
                    start = i.saturating_add(1);
                }
            }
            i += 1;
        }
        if start < text.len() {
            let line = text.get(start..text.len()).unwrap_or(&[]);
            m.push(line)?;
        } else if start == text.len() && text.is_empty() {
            return Ok(m);
        }
        Ok(m)
    }

    /// 由字符串表装配（须已按 C 序）。
    pub fn from_strs(list: &[&str]) -> Result<Manifest, u32> {
        let mut m = Manifest::new();
        let mut i = 0usize;
        while i < list.len() {
            match list.get(i) {
                Some(s) => {
                    m.push(s.as_bytes())?;
                }
                None => return Err(LB_ESHAPE),
            }
            i += 1;
        }
        Ok(m)
    }

    /// 基准机清单。
    pub fn baseline() -> Result<Manifest, u32> {
        Manifest::from_strs(&BASELINE)
    }

    /// 是否含某名。
    pub fn contains(&self, s: &[u8]) -> bool {
        self.index_of(s).is_some()
    }

    /// 名下标（线性；清单 ≤ 48 条）。
    pub fn index_of(&self, s: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(n) = self.rows.get(i) {
                if n.named(s) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 前缀计数。
    pub fn count_prefix(&self, p: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if let Some(b) = r.bytes().get(0) {
                    if *b == p {
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        n
    }

    /// 顺序违例计数（相邻逆序对）。
    pub fn inversions(&self) -> usize {
        let mut n = 0usize;
        let mut i = 1usize;
        while i < self.len {
            match (self.rows.get(i - 1), self.rows.get(i)) {
                (Some(a), Some(b)) => {
                    if cmp_bytes(a.bytes(), b.bytes()) >= 0 {
                        n += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        n
    }

    /// 名字总字节数（不含分隔符）。
    pub fn name_bytes(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.len());
            }
            i += 1;
        }
        n
    }

    /// 渲染：`full` 时每行加 `/bin/` 前缀。无行时为空串。
    pub fn render(&self, full: bool) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if full {
                    s.push_str(FULL_PREFIX);
                }
                s.push_str(r.text().as_str());
                if i.saturating_add(1) < self.len {
                    s.push('\n');
                }
            }
            i += 1;
        }
        s
    }

    /// 与基线归并对拍。
    pub fn diff_against(&self, baseline: &[&str]) -> Diff {
        let mut d = Diff::blank();
        let mut i = 0usize;
        let mut j = 0usize;
        while i < self.len || j < baseline.len() {
            let a = self.row(i);
            let b = if j < baseline.len() {
                baseline.get(j).copied()
            } else {
                None
            };
            match (a, b) {
                (Some(an), Some(bs)) => {
                    let c = cmp_bytes(an.bytes(), bs.as_bytes());
                    if c == 0 {
                        i += 1;
                        j += 1;
                    } else if c < 0 {
                        if d.extra == 0 {
                            d.first_extra = an.text();
                        }
                        d.extra += 1;
                        i += 1;
                    } else {
                        if d.missing == 0 {
                            d.first_missing = String::from(bs);
                        }
                        d.missing += 1;
                        j += 1;
                    }
                }
                (Some(an), None) => {
                    if d.extra == 0 {
                        d.first_extra = an.text();
                    }
                    d.extra += 1;
                    i += 1;
                }
                (None, Some(bs)) => {
                    if d.missing == 0 {
                        d.first_missing = String::from(bs);
                    }
                    d.missing += 1;
                    j += 1;
                }
                (None, None) => break,
            }
        }
        d.names = self.len;
        d
    }
}

/// 对拍结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    /// 基线有、清单无的条数。
    pub missing: usize,
    /// 清单有、基线无的条数。
    pub extra: usize,
    /// 首个缺项。
    pub first_missing: String,
    /// 首个多项。
    pub first_extra: String,
    /// 参与对拍的清单行数。
    pub names: usize,
}

impl Diff {
    /// 空白。
    pub fn blank() -> Diff {
        Diff {
            missing: 0,
            extra: 0,
            first_missing: String::new(),
            first_extra: String::new(),
            names: 0,
        }
    }

    /// 是否完全一致（且条数一致）。
    pub fn exact(&self) -> bool {
        self.missing == 0 && self.extra == 0 && self.names == BASELINE_LEN
    }

    /// 一行文本：`missing=0 extra=0`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("missing=");
        push_u64(&mut s, self.missing as u64);
        s.push_str(" extra=");
        push_u64(&mut s, self.extra as u64);
        s
    }
}

// ---------------------------------------------------------------------------
// 选项
// ---------------------------------------------------------------------------

/// `--list` 选项面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opt {
    /// 是否全长路径模式。
    pub full: bool,
}

impl Opt {
    /// 缺省。
    pub const fn new() -> Opt {
        Opt { full: false }
    }

    /// 文本。
    pub fn text(&self) -> &'static str {
        if self.full {
            "list-full"
        } else {
            "list"
        }
    }
}

/// 解析：只接受一个清单选项，不接受任何操作数。
pub fn parse_argv(args: &[&[u8]]) -> Result<Opt, u32> {
    let mut o = Opt::new();
    let mut seen = false;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(LB_EOPT),
        };
        if eq_bytes(a, b"--list") {
            if seen {
                return Err(LB_EOPT);
            }
            o.full = false;
            seen = true;
        } else if eq_bytes(a, b"--list-full") {
            if seen {
                return Err(LB_EOPT);
            }
            o.full = true;
            seen = true;
        } else if eq_bytes(a, b"--") {
            if i.saturating_add(1) < args.len() {
                return Err(LB_EOPERAND);
            }
        } else if a.len() > 0 {
            if let Some(f) = a.get(0) {
                if *f == b'-' {
                    return Err(LB_EOPT);
                }
            }
            return Err(LB_EOPERAND);
        } else {
            return Err(LB_EOPERAND);
        }
        i += 1;
    }
    Ok(o)
}

/// 生成 `nNN` 形式的占位名（容量测试用）。
fn gen_name(i: usize) -> Name {
    let mut buf = [b'n', b'0', b'0'];
    if let Some(d) = buf.get_mut(1) {
        *d = b'0' + ((i / 10) % 10) as u8;
    }
    if let Some(d) = buf.get_mut(2) {
        *d = b'0' + (i % 10) as u8;
    }
    Name::new(&buf).unwrap_or(Name::blank())
}

/// 超容路径：49 条递增名。
fn over_cap() -> Result<u32, u32> {
    let mut m = Manifest::new();
    let mut i = 0usize;
    while i <= CAP {
        let n = gen_name(i);
        m.push(n.bytes())?;
        i += 1;
    }
    Ok(m.len() as u32)
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
            return Err(LB_ESLOT);
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
            return Err(LB_EBAL);
        }
        const VALS: [u32; 8] = [40, 3, 18, 39, 1, 179, 379, 5];
        const CODES: [u32; 6] = [
            LB_EBYTE,
            LB_ELEN,
            LB_ECAP,
            LB_EDUP,
            LB_EORDER,
            LB_ESHAPE,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LB_EBAL),
            };
            if s.slot as usize != i {
                return Err(LB_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LB_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LB_EVALUE);
                        }
                    }
                    None => return Err(LB_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LB_EVALUE);
                        }
                    }
                    None => return Err(LB_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LB_EBAL);
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
            return Err(LB_EBAL);
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
            Err(LB_ESEAL)
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

/// `Result<usize, u32>`（或 u32）→ 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn cell32(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let base = Manifest::baseline();

    let _ = l.register(cell(0, OP_PARSE, base.map(|m| m.len())));

    let _ = l.register(cell(
        1,
        OP_PARSE,
        Manifest::baseline().and_then(|m| match m.row(0) {
            Some(n) => Ok(n.len()),
            None => Err(LB_ESHAPE),
        }),
    ));

    let _ = l.register(cell(
        2,
        OP_SORT,
        Manifest::baseline().and_then(|m| m.index_of(b"ls").ok_or(LB_ESHAPE)),
    ));

    let _ = l.register(cell(
        3,
        OP_SORT,
        Manifest::baseline().and_then(|m| m.index_of(b"wc").ok_or(LB_ESHAPE)),
    ));

    let _ = l.register(cell(
        4,
        OP_DIFF,
        Manifest::baseline().map(|m| {
            let d = m.diff_against(&BASELINE);
            if d.exact() {
                1usize
            } else {
                0usize
            }
        }),
    ));

    let _ = l.register(cell(
        5,
        OP_RENDER,
        Manifest::baseline().map(|m| m.render(false).as_bytes().len()),
    ));

    let _ = l.register(cell(
        6,
        OP_RENDER,
        Manifest::baseline().map(|m| m.render(true).as_bytes().len()),
    ));

    let _ = l.register(cell(
        7,
        OP_SHAPE,
        Manifest::baseline().map(|m| m.count_prefix(b's')),
    ));

    let _ = l.register(cell(
        8,
        OP_PARSE,
        Manifest::baseline().and_then(|mut m| m.push(&[0x01u8])),
    ));

    let _ = l.register(cell(
        9,
        OP_PARSE,
        Manifest::baseline().and_then(|mut m| m.push(&[b'z'; 33])),
    ));

    let _ = l.register(cell32(10, OP_PARSE, over_cap()));

    let _ = l.register(cell(
        11,
        OP_PARSE,
        Manifest::from_strs(&["ls", "ls"]).map(|m| m.len()),
    ));

    let _ = l.register(cell(
        12,
        OP_PARSE,
        Manifest::from_strs(&["ls", "ash"]).map(|m| m.len()),
    ));

    let _ = l.register(cell(
        13,
        OP_PARSE,
        Manifest::from_strs(&[""]).map(|m| m.len()),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 基线装载。
fn baseline_ok() -> bool {
    let m = match Manifest::baseline() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if m.len() != BASELINE_LEN || m.is_empty() {
        return false;
    }
    if m.index_of(b"ash") != Some(0) || m.index_of(b"wc") != Some(39) {
        return false;
    }
    if m.index_of(b"ls") != Some(18) || m.index_of(b"sh") != Some(27) {
        return false;
    }
    if m.inversions() != 0 {
        return false;
    }
    if m.count_prefix(b's') != 5 {
        return false;
    }
    if m.count_prefix(b'a') != 2 {
        return false;
    }
    let mut i = 1usize;
    while i < m.len() {
        match (m.row(i - 1), m.row(i)) {
            (Some(a), Some(b)) => {
                if cmp_bytes(a.bytes(), b.bytes()) >= 0 {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 字节与长度闸。
fn byte_ok() -> bool {
    let a = Manifest::parse(b"ls").map(|m| m.len()) == Ok(1);
    let b = Manifest::parse(b"ls\n").map(|m| m.len()) == Ok(1);
    let c = Manifest::parse(&[0x7f]).err() == Some(LB_EBYTE);
    let d = Manifest::parse(&[b'z'; 33]).err() == Some(LB_ELEN);
    let e = Manifest::parse(&[b'z'; 32]).map(|m| m.len()) == Ok(1);
    let f = Manifest::parse(b"a/b").err() == Some(LB_ESHAPE);
    let g = Manifest::parse(&[]).map(|m| m.len()) == Ok(0);
    a && b && c && d && e && f && g
}

/// 字典序面。
fn order_ok() -> bool {
    let a = Manifest::parse(b"ash\nawk\ncat\n").map(|m| m.inversions()) == Ok(0);
    let b = Manifest::parse(b"awk\nash\n").err() == Some(LB_EORDER);
    let c = Manifest::parse(b"ls\nls\n").err() == Some(LB_EDUP);
    let d = Manifest::parse(b"ls\n").and_then(|mut m| m.push(b"ls")).err() == Some(LB_EDUP);
    let e = Manifest::parse(b"ls\n").and_then(|mut m| m.push(b"ash")).err() == Some(LB_EORDER);
    let f = Manifest::parse(b"ls\nmkdir\n").map(|m| m.index_of(b"mkdir")) == Ok(Some(1));
    let g = cmp_bytes(b"abc", b"abd") < 0
        && cmp_bytes(b"abc", b"abc") == 0
        && cmp_bytes(b"abcd", b"abc") > 0
        && cmp_bytes(b"", b"a") < 0;
    a && b && c && d && e && f && g
}

/// 形状面。
fn shape_ok() -> bool {
    let a = Manifest::parse(b"\n").err() == Some(LB_ESHAPE);
    let b = Manifest::parse(b"ls\n\n").err() == Some(LB_ESHAPE);
    let c = Manifest::parse(b"/bin/ls").err() == Some(LB_ESHAPE);
    let d = Manifest::parse(b"ls -l").err() == Some(LB_EBYTE);
    let e = Manifest::parse(b"ls\nwc").map(|m| m.len()) == Ok(2);
    let f = Manifest::parse(b"ls\nwc\n").map(|m| m.len()) == Ok(2);
    let g = Name::new(b"").err() == Some(LB_ESHAPE);
    let h = Name::new(b"a\\b").err() == Some(LB_ESHAPE);
    a && b && c && d && e && f && g && h
}

/// 容量面。
fn cap_ok() -> bool {
    let a = over_cap().err() == Some(LB_ECAP);
    let mut m = Manifest::new();
    let mut i = 0usize;
    let mut ok = true;
    while i < CAP {
        let n = gen_name(i);
        if m.push(n.bytes()).is_err() {
            ok = false;
        }
        i += 1;
    }
    a && ok && m.len() == CAP
}

/// 渲染面。
fn render_ok() -> bool {
    let m = match Manifest::from_strs(&["ash", "ls", "wc"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if m.render(false).as_str() != "ash\nls\nwc" {
        return false;
    }
    if m.render(true).as_str() != "/bin/ash\n/bin/ls\n/bin/wc" {
        return false;
    }
    if m.render(false).as_bytes().len() != 9 {
        return false;
    }
    if m.render(true).as_bytes().len() != 24 {
        return false;
    }
    if Manifest::new().render(false).as_str() != "" {
        return false;
    }
    if m.name_bytes() != 7 {
        return false;
    }
    let base = match Manifest::baseline() {
        Ok(v) => v,
        Err(_) => return false,
    };
    base.render(false).as_bytes().len() == 179 && base.render(true).as_bytes().len() == 379
}

/// 对拍面。
fn diff_ok() -> bool {
    let base = match Manifest::baseline() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = base.diff_against(&BASELINE);
    if !d.exact() || d.missing != 0 || d.extra != 0 {
        return false;
    }
    if d.text().as_str() != "missing=0 extra=0" {
        return false;
    }
    let miss = match Manifest::from_strs(&[
        "awk", "cat", "chmod", "chown", "cp", "cut", "date", "dd", "df", "du", "echo", "expr",
        "find", "grep", "head", "kill", "ln", "ls", "mkdir", "mount", "mv", "printf", "ps", "rm",
        "rmdir", "sed", "sh", "sleep", "sort", "stat", "tail", "tar", "tee", "test", "touch",
        "tr", "umount", "uniq", "wc",
    ]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let dm = miss.diff_against(&BASELINE);
    if dm.missing != 1 || dm.extra != 0 || dm.first_missing.as_str() != "ash" {
        return false;
    }
    let extra = match Manifest::from_strs(&[
        "ash", "awk", "cat", "chmod", "chown", "cp", "cut", "date", "dd", "df", "du", "echo",
        "expr", "find", "grep", "head", "kill", "ln", "ls", "mkdir", "mount", "mv", "printf",
        "ps", "rm", "rmdir", "sed", "sh", "sleep", "sort", "stat", "tail", "tar", "tee", "test",
        "touch", "tr", "umount", "uniq", "wc", "xz",
    ]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let de = extra.diff_against(&BASELINE);
    de.missing == 0 && de.extra == 1 && de.first_extra.as_str() == "xz" && !de.exact()
}

/// 选项面。
fn option_ok() -> bool {
    let a = parse_argv(&[]).map(|o| o.full) == Ok(false);
    let b = parse_argv(&[b"--list"]).map(|o| o.full) == Ok(false);
    let c = parse_argv(&[b"--list-full"]).map(|o| o.full) == Ok(true);
    let d = parse_argv(&[b"--help"]).err() == Some(LB_EOPT);
    let e = parse_argv(&[b"--list", b"x"]).err() == Some(LB_EOPERAND);
    let f = parse_argv(&[b"--list", b"--list-full"]).err() == Some(LB_EOPT);
    let g = parse_argv(&[b"--"]).map(|o| o.full) == Ok(false);
    let h = parse_argv(&[b"--", b"x"]).err() == Some(LB_EOPERAND);
    let i = parse_argv(&[b"-l"]).err() == Some(LB_EOPT);
    let j = Opt::new().text() == "list" && parse_argv(&[b"--list-full"]).map(|o| o.text()) == Ok("list-full");
    a && b && c && d && e && f && g && h && i && j
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
    if l2.register(Sample::blank()) != Err(LB_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = baseline_ok() && byte_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = order_ok() && shape_ok() && cap_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = render_ok() && diff_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = option_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11582_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11582.list");

    s.add("LB01-基线清单装载", baseline_ok(), "static");
    s.add("LB02-字节与长度闸", byte_ok(), "static");
    s.add("LB03-字典序", order_ok(), "static");
    s.add("LB04-重名拒绝", order_ok(), "static");
    s.add("LB05-空行与斜杠", shape_ok(), "static");
    s.add("LB06-容量边界", cap_ok(), "static");
    s.add("LB07-清单渲染", render_ok(), "static");
    s.add("LB08-全长渲染", render_ok(), "static");
    s.add("LB09-基准机对拍", diff_ok(), "static");
    s.add("LB10-缺项检测", diff_ok(), "static");
    s.add("LB11-选项解析", option_ok(), "static");
    s.add("LB12-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "LB13-冻结重放四注入",
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
    fn baseline_shape() {
        let m = Manifest::baseline().expect("m");
        assert_eq!(m.len(), 40);
        assert_eq!(m.row(0).map(|n| n.text().as_str() == "ash"), Some(true));
        assert_eq!(m.row(39).map(|n| n.text().as_str() == "wc"), Some(true));
        assert_eq!(m.index_of(b"ls"), Some(18));
        assert_eq!(m.inversions(), 0);
    }

    #[test]
    fn parse_lines() {
        let m = Manifest::parse(b"ash\nawk\ncat\n").expect("m");
        assert_eq!(m.len(), 3);
        assert_eq!(m.index_of(b"cat"), Some(2));
        assert_eq!(Manifest::parse(b"ash\r\n").err(), Some(LB_EBYTE));
        assert_eq!(Manifest::parse(b"ash\n\n").err(), Some(LB_ESHAPE));
        assert_eq!(Manifest::parse(&[]).map(|x| x.len()), Ok(0));
    }

    #[test]
    fn order_is_strict() {
        assert_eq!(Manifest::parse(b"awk\nash\n").err(), Some(LB_EORDER));
        assert_eq!(Manifest::parse(b"ls\nls\n").err(), Some(LB_EDUP));
        assert_eq!(
            Manifest::parse(b"ls\n").and_then(|mut m| m.push(b"mkdir")).map(|_| 1),
            Ok(1)
        );
        assert_eq!(
            Manifest::parse(b"ls\n").and_then(|mut m| m.push(b"a")).err(),
            Some(LB_EORDER)
        );
    }

    #[test]
    fn name_gates() {
        assert_eq!(Name::new(b"").err(), Some(LB_ESHAPE));
        assert_eq!(Name::new(b"/bin/ls").err(), Some(LB_ESHAPE));
        assert_eq!(Name::new(&[b'a'; 33]).err(), Some(LB_ELEN));
        assert_eq!(Name::new(b"a b").err(), Some(LB_EBYTE));
        assert_eq!(Name::new(b"ls").map(|n| n.len()), Ok(2));
    }

    #[test]
    fn capacity_is_48() {
        assert_eq!(over_cap().err(), Some(LB_ECAP));
        let mut m = Manifest::new();
        let mut i = 0usize;
        while i < CAP {
            let n = gen_name(i);
            assert_eq!(m.push(n.bytes()), Ok(i));
            i += 1;
        }
        assert_eq!(m.len(), CAP);
    }

    #[test]
    fn render_both_modes() {
        let m = Manifest::from_strs(&["ash", "ls", "wc"]).expect("m");
        assert_eq!(m.render(false).as_str(), "ash\nls\nwc");
        assert_eq!(m.render(true).as_str(), "/bin/ash\n/bin/ls\n/bin/wc");
        assert_eq!(m.render(true).as_bytes().len(), 24);
        assert_eq!(m.name_bytes(), 7);
    }

    #[test]
    fn diff_exact_and_perturbed() {
        let m = Manifest::baseline().expect("m");
        let d = m.diff_against(&BASELINE);
        assert!(d.exact());
        assert_eq!(d.text().as_str(), "missing=0 extra=0");
        let cut = Manifest::from_strs(&["ash", "awk"]).expect("cut");
        let d2 = cut.diff_against(&BASELINE);
        assert_eq!(d2.missing, 38);
        assert_eq!(d2.extra, 0);
        assert_eq!(d2.first_missing.as_str(), "cat");
    }

    #[test]
    fn options_parse() {
        assert_eq!(parse_argv(&[]).map(|o| o.full), Ok(false));
        assert_eq!(parse_argv(&[b"--list"]).map(|o| o.full), Ok(false));
        assert_eq!(parse_argv(&[b"--list-full"]).map(|o| o.full), Ok(true));
        assert_eq!(parse_argv(&[b"--help"]).err(), Some(LB_EOPT));
        assert_eq!(parse_argv(&[b"--list", b"x"]).err(), Some(LB_EOPERAND));
        assert_eq!(parse_argv(&[b"--list", b"--list-full"]).err(), Some(LB_EOPT));
    }

    #[test]
    fn diff_reports_missing_first() {
        let miss = Manifest::from_strs(&[
            "awk", "cat", "chmod", "chown", "cp", "cut", "date", "dd", "df", "du", "echo", "expr",
        ])
        .expect("m");
        let d = miss.diff_against(&BASELINE);
        assert_eq!(d.missing, 28);
        assert_eq!(d.extra, 0);
        assert_eq!(d.first_missing.as_str(), "ash");
        assert!(!d.exact());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(40));
        assert_eq!(l.sample(6).map(|s| s.value), Some(379));
        assert_eq!(l.sample(8).map(|s| s.code), Some(LB_EBYTE));
        assert_eq!(l.sample(13).map(|s| s.code), Some(LB_ESHAPE));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(LB_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(LB_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
    }

    #[test]
    fn injections_all_true() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn check_count_is_thirteen() {
        assert_eq!(run_unx11582_checks().len(), 13);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11582_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}

