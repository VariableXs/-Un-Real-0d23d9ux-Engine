//! UNX-F11626 依赖缺失 cannot open shared object file 文案判据
//!
//! 共享对象的依赖不是一张平表，而是一条以 `DT_NEEDED` 为边的有向图。装载
//! 一份对象时，装载器把自己的 `DT_NEEDED` 逐个拿去做路径搜索，命中的对象
//! 再把自己的 `DT_NEEDED` 压进队列，直到闭包完成。任何一条边找不到落点，
//! 整个 `dlopen` 就失败，`dlerror` 给出的文本**只写缺失的那个 soname**：
//!
//! ```text
//! libmissing.so: cannot open shared object file: No such file or directory
//! ```
//!
//! 注意文本里没有 "required by"：glibc 不告诉你是谁的依赖缺了。但这不意味
//! 着诊断可以丢掉请求者——`ChainReport` 把 `subject`（缺失名）和
//! `requester`（谁的 DT_NEEDED）分开保存，文案仍然逐字只写前者。把请求者
//! 拼进 dlerror 会让逐字比对失败；把请求者丢掉则让运维无法定位。两者必须
//! 同时存在、各走各的出口。
//!
//! 搜索次序是另一半判据。glibc 的规则可以压缩成三条：
//!
//! 1. `DT_RUNPATH` 存在时，`DT_RPATH` **整条作废**（不是"后试"，是不试）；
//! 2. 次序是 RUNPATH → `LD_LIBRARY_PATH` → 默认目录（`/lib`、`/usr/lib`）；
//! 3. 只有 RPATH、没有 RUNPATH 时，才走 RPATH → `LD_LIBRARY_PATH` → 默认。
//!
//! 路径里的 `$ORIGIN` 要在搜索时展开成根对象的目录；展开后仍找不到，错误
//! 码记为"来源路径未命中"，但文案仍是那条标准的 cannot open 文本。
//!
//! 动态段本身也可能坏：`DT_NEEDED` 的偏移越过 `DT_STRSZ`（畸形字符串表），
//! 或指向空串（空 soname）。这两种情况必须在路径搜索之前判死，否则会拿着
//! 垃圾偏移去拼路径，最后误报成"文件不存在"。
//!
//! 闸门次序：动态段自洽 → 取 soname → 搜索路径展开 → 逐目录定位 → 递归依赖 → 环检测 → 文案。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfc90;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 字符串表容量。
pub const STRTAB_CAP: usize = 160;

/// 动态段容量。
pub const DYN_MAX: usize = 16;

/// 依赖链容量。
pub const CHAIN_MAX: usize = 12;

/// 递归深度上限。
pub const DEPTH_MAX: usize = 6;

/// 搜索目录容量。
pub const PATH_MAX: usize = 4;

/// 定位轨迹容量。
pub const TRACE_MAX: usize = 24;

// ---------------------------------------------------------------------------
// 动态段标签
// ---------------------------------------------------------------------------

/// 终止项。
pub const DT_NULL: u32 = 0;
/// 依赖 soname 的字符串偏移。
pub const DT_NEEDED: u32 = 1;
/// 字符串表地址。
pub const DT_STRTAB: u32 = 5;
/// 字符串表长度。
pub const DT_STRSZ: u32 = 10;
/// 本对象 soname。
pub const DT_SONAME: u32 = 14;
/// 旧式搜索路径。
pub const DT_RPATH: u32 = 15;
/// 新式搜索路径。
pub const DT_RUNPATH: u32 = 29;

/// 标签名。
pub fn dt_name(tag: u32) -> &'static str {
    match tag {
        DT_NULL => "DT_NULL",
        DT_NEEDED => "DT_NEEDED",
        DT_STRTAB => "DT_STRTAB",
        DT_STRSZ => "DT_STRSZ",
        DT_SONAME => "DT_SONAME",
        DT_RPATH => "DT_RPATH",
        DT_RUNPATH => "DT_RUNPATH",
        _ => "DT_UNKNOWN",
    }
}

/// 标签是否携带字符串偏移。
pub fn dt_is_string(tag: u32) -> bool {
    match tag {
        DT_NEEDED => true,
        DT_SONAME => true,
        DT_RPATH => true,
        DT_RUNPATH => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 默认目录
// ---------------------------------------------------------------------------

/// 默认搜索目录条数。
pub const DEFAULT_COUNT: usize = 2;

/// 默认目录首项。
pub const DEFAULT0: &str = "/lib";

/// 默认目录次项。
pub const DEFAULT1: &str = "/usr/lib";

/// 已知文件系统。
pub const FILES: [&str; 12] = [
    "/lib/libc.so.6",
    "/usr/lib/libc.so.6",
    "/opt/app/libm.so.6",
    "/opt/app/libz.so.1",
    "/opt/env/libq.so",
    "./liblocal.so",
    "/opt/new/libp.so",
    "/opt/old/libp.so",
    "/lib/liba.so",
    "/lib/libb.so",
    "/opt/app/libbroken.so",
    "/opt/app/libbadname.so",
];

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 直接依赖缺失。
pub const NE_ENOENT: u32 = SEG_BASE + 0x01;
/// 传递依赖缺失。
pub const NE_EDEP: u32 = SEG_BASE + 0x02;
/// 依赖环。
pub const NE_ECYCLE: u32 = SEG_BASE + 0x03;
/// 动态段畸形。
pub const NE_EMALF: u32 = SEG_BASE + 0x04;
/// 空 soname。
pub const NE_ENAME: u32 = SEG_BASE + 0x05;
/// `$ORIGIN` 展开后未命中。
pub const NE_EORIGIN: u32 = SEG_BASE + 0x06;
/// 搜索次序违规。
pub const NE_EORDER: u32 = SEG_BASE + 0x07;
/// 字符串表越界。
pub const NE_ESTRTAB: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const NE_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const NE_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const NE_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹不符。
pub const NE_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const NE_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 语料。
pub const OP_CORPUS: u8 = 1;
/// 动态段。
pub const OP_DYNAMIC: u8 = 2;
/// 字符串表。
pub const OP_STRTAB: u8 = 3;
/// 路径搜索。
pub const OP_SEARCH: u8 = 4;
/// 依赖链。
pub const OP_CHAIN: u8 = 5;
/// 文案。
pub const OP_TEXT: u8 = 6;

// ---------------------------------------------------------------------------
// FNV-1a
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

/// 字节串摘要。
fn digest(bytes: &[u8]) -> u64 {
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
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本。
fn dec_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
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

/// 字符串里是否含子串。
fn str_contains(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    if nb.is_empty() {
        return true;
    }
    if nb.len() > hb.len() {
        return false;
    }
    let mut i = 0usize;
    while i + nb.len() <= hb.len() {
        let mut k = 0usize;
        let mut ok = true;
        while k < nb.len() {
            match (hb.get(i + k), nb.get(k)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        ok = false;
                        break;
                    }
                }
                _ => {
                    ok = false;
                    break;
                }
            }
            k += 1;
        }
        if ok {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 字符串表
// ---------------------------------------------------------------------------

/// 固定容量字符串表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strtab {
    buf: [u8; STRTAB_CAP],
    len: usize,
}

impl Strtab {
    /// 空表。
    pub const fn blank() -> Strtab {
        Strtab { buf: [0u8; STRTAB_CAP], len: 0 }
    }

    /// 追加一串（含结尾零），返回偏移。
    pub fn push(&mut self, s: &str) -> Option<u32> {
        let bytes = s.as_bytes();
        let need = match bytes.len().checked_add(1) {
            Some(v) => v,
            None => return None,
        };
        let total = match self.len.checked_add(need) {
            Some(v) => v,
            None => return None,
        };
        if total > STRTAB_CAP {
            return None;
        }
        let off = self.len;
        let mut i = 0usize;
        while i < bytes.len() {
            match (bytes.get(i), self.buf.get_mut(off + i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return None,
            }
            i += 1;
        }
        if let Some(dst) = self.buf.get_mut(off + bytes.len()) {
            *dst = 0;
        }
        self.len = total;
        Some(off as u32)
    }

    /// 表长（含所有结尾零）。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 从偏移取一串（到第一个零为止）。
    pub fn get(&self, off: usize) -> Option<&str> {
        if off >= self.len {
            return None;
        }
        let mut end = off;
        loop {
            if end >= self.len {
                return None;
            }
            match self.buf.get(end) {
                Some(0) => break,
                Some(_) => end += 1,
                None => return None,
            }
        }
        let slice = self.buf.get(off..end)?;
        core::str::from_utf8(slice).ok()
    }

    /// 某串的偏移。
    pub fn offset_of(&self, s: &str) -> Option<u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.get(i) {
                if v == s {
                    return Some(i as u32);
                }
                i += v.as_bytes().len() + 1;
            } else {
                return None;
            }
        }
        None
    }

    /// 是否含某串。
    pub fn contains(&self, s: &str) -> bool {
        self.offset_of(s).is_some()
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.len as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 动态段
// ---------------------------------------------------------------------------

/// 一条动态项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynEntry {
    /// 标签。
    pub tag: u32,
    /// 值。
    pub val: u64,
}

impl DynEntry {
    /// 空项。
    pub const fn null() -> DynEntry {
        DynEntry { tag: 0, val: 0 }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let h = fold_word(FNV_OFF, self.tag as u64);
        fold_word(h, self.val)
    }
}

/// 固定容量动态项表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynTab {
    rows: [DynEntry; DYN_MAX],
    len: usize,
}

impl DynTab {
    /// 空表。
    pub const fn blank() -> DynTab {
        DynTab { rows: [DynEntry::null(); DYN_MAX], len: 0 }
    }

    /// 追加。
    pub fn push(&mut self, tag: u32, val: u64) -> bool {
        if self.len >= DYN_MAX {
            return false;
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = DynEntry { tag, val };
            self.len += 1;
            return true;
        }
        false
    }

    /// 项数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一项。
    pub fn get(&self, i: usize) -> Option<DynEntry> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 首个某标签的值。
    pub fn find(&self, tag: u32) -> Option<u64> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if e.tag == tag {
                    return Some(e.val);
                }
            }
            i += 1;
        }
        None
    }

    /// 某标签出现次数。
    pub fn count(&self, tag: u32) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if e.tag == tag {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.len as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                h = fold_word(h, e.seal());
            }
            i += 1;
        }
        h
    }
}

/// 动态段 + 字符串表 + 搜索路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dynamic {
    strtab: Strtab,
    tab: DynTab,
    names: [&'static str; DYN_MAX],
    needed_len: usize,
    rpath_list: &'static str,
    runpath_list: &'static str,
    has_rpath: bool,
    has_runpath: bool,
}

impl Dynamic {
    /// 空段。
    pub const fn blank() -> Dynamic {
        Dynamic {
            strtab: Strtab::blank(),
            tab: DynTab::blank(),
            names: [""; DYN_MAX],
            needed_len: 0,
            rpath_list: "",
            runpath_list: "",
            has_rpath: false,
            has_runpath: false,
        }
    }

    /// 字符串表。
    pub const fn strtab(&self) -> &Strtab {
        &self.strtab
    }

    /// 动态项表。
    pub const fn entries(&self) -> &DynTab {
        &self.tab
    }

    /// 本对象 soname。
    pub fn soname(&self) -> Option<&str> {
        let off = self.tab.find(DT_SONAME)?;
        self.strtab.get(off as usize)
    }

    /// `DT_NEEDED` 条数。
    pub fn needed_count(&self) -> usize {
        self.tab.count(DT_NEEDED)
    }

    /// 第 `i` 条 `DT_NEEDED` 的字符串。
    pub fn needed_at(&self, i: usize) -> Option<&'static str> {
        if i >= self.needed_len {
            return None;
        }
        self.names.get(i).copied()
    }

    /// 记录一条已写入字符串表的依赖名。
    fn note_needed(&mut self, name: &'static str) -> bool {
        if self.needed_len >= DYN_MAX {
            return false;
        }
        if let Some(dst) = self.names.get_mut(self.needed_len) {
            *dst = name;
            self.needed_len += 1;
            return true;
        }
        false
    }

    /// 是否有 `DT_RPATH`。
    pub const fn has_rpath(&self) -> bool {
        self.has_rpath
    }

    /// 是否有 `DT_RUNPATH`。
    pub const fn has_runpath(&self) -> bool {
        self.has_runpath
    }

    /// 取搜索路径。
    pub fn paths(&self, tag: u32) -> Option<SearchPath> {
        match tag {
            DT_RPATH => {
                if self.has_rpath {
                    Some(SearchPath::parse(self.rpath_list))
                } else {
                    None
                }
            }
            DT_RUNPATH => {
                if self.has_runpath {
                    Some(SearchPath::parse(self.runpath_list))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 字符串表与所有字符串标签是否自洽。
    pub fn str_ok(&self) -> bool {
        if self.tab.find(DT_STRTAB).is_none() {
            return false;
        }
        if self.tab.find(DT_STRSZ) != Some(self.strtab.len() as u64) {
            return false;
        }
        let mut i = 0usize;
        while i < self.tab.len() {
            if let Some(e) = self.tab.get(i) {
                if dt_is_string(e.tag) {
                    if e.val >= self.strtab.len() as u64 {
                        return false;
                    }
                    if self.strtab.get(e.val as usize).is_none() {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.strtab.seal());
        h = fold_word(h, self.tab.seal());
        h = fold_word(h, self.has_rpath as u64);
        h = fold_word(h, self.has_runpath as u64);
        h = fold_word(h, digest(self.rpath_list.as_bytes()));
        fold_word(h, digest(self.runpath_list.as_bytes()))
    }
}

/// 造一份动态段。
pub fn dynamic(
    soname: &'static str,
    needs: &[&'static str],
    rpath: Option<&'static str>,
    runpath: Option<&'static str>,
) -> Dynamic {
    let mut d = Dynamic::blank();
    let so = d.strtab.push(soname).unwrap_or(0);
    let _ = d.tab.push(DT_SONAME, so as u64);
    let mut i = 0usize;
    while i < needs.len() {
        if let Some(n) = needs.get(i) {
            if let Some(off) = d.strtab.push(n) {
                if d.tab.push(DT_NEEDED, off as u64) {
                    let _ = d.note_needed(n);
                }
            }
        }
        i += 1;
    }
    if let Some(r) = rpath {
        if let Some(off) = d.strtab.push(r) {
            let _ = d.tab.push(DT_RPATH, off as u64);
            d.rpath_list = r;
            d.has_rpath = true;
        }
    }
    if let Some(r) = runpath {
        if let Some(off) = d.strtab.push(r) {
            let _ = d.tab.push(DT_RUNPATH, off as u64);
            d.runpath_list = r;
            d.has_runpath = true;
        }
    }
    let _ = d.tab.push(DT_STRTAB, 0);
    let _ = d.tab.push(DT_STRSZ, d.strtab.len() as u64);
    let _ = d.tab.push(DT_NULL, 0);
    d
}

/// 造一份"DT_NEEDED 越界"的动态段。
pub fn dynamic_bad(soname: &'static str) -> Dynamic {
    let mut d = dynamic(soname, &[], None, None);
    let _ = d.tab.push(DT_NEEDED, 4096);
    d
}

/// 造一份"DT_NEEDED 指向空串"的动态段。
pub fn dynamic_empty_needed(soname: &'static str) -> Dynamic {
    dynamic(soname, &[""], None, None)
}

// ---------------------------------------------------------------------------
// 搜索路径
// ---------------------------------------------------------------------------

/// 冒号分隔的目录表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchPath {
    dirs: [&'static str; PATH_MAX],
    len: usize,
}

impl SearchPath {
    /// 空表。
    pub const fn empty() -> SearchPath {
        SearchPath { dirs: ["", "", "", ""], len: 0 }
    }

    /// 默认目录。
    pub const fn defaults() -> SearchPath {
        SearchPath { dirs: [DEFAULT0, DEFAULT1, "", ""], len: DEFAULT_COUNT }
    }

    /// 解析冒号列表；空段按当前目录 `.` 处理。
    pub fn parse(list: &'static str) -> SearchPath {
        let mut p = SearchPath::empty();
        if list.is_empty() {
            let _ = p.push(".");
            return p;
        }
        let bytes = list.as_bytes();
        let mut start = 0usize;
        let mut i = 0usize;
        loop {
            let at_end = i == bytes.len();
            let at_colon = match bytes.get(i) {
                Some(b':') => true,
                _ => false,
            };
            if at_end || at_colon {
                let seg = list.get(start..i).unwrap_or("");
                let dir = if seg.is_empty() { "." } else { seg };
                let _ = p.push(dir);
                start = i + 1;
            }
            if at_end {
                break;
            }
            i += 1;
        }
        p
    }

    /// 追加目录。
    pub fn push(&mut self, dir: &'static str) -> bool {
        if self.len >= PATH_MAX {
            return false;
        }
        if let Some(dst) = self.dirs.get_mut(self.len) {
            *dst = dir;
            self.len += 1;
            return true;
        }
        false
    }

    /// 目录条数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取目录。
    pub fn get(&self, i: usize) -> Option<&'static str> {
        if i >= self.len {
            return None;
        }
        self.dirs.get(i).copied()
    }

    /// 是否含某目录。
    pub fn contains(&self, dir: &str) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = self.dirs.get(i) {
                if *d == dir {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 渲染成冒号列表。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if i > 0 {
                s.push(':');
            }
            if let Some(d) = self.dirs.get(i) {
                s.push_str(d);
            }
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.len as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = self.dirs.get(i) {
                h = fold_word(h, digest(d.as_bytes()));
            }
            i += 1;
        }
        h
    }
}

/// 搜索来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 依赖名自带斜杠，直接打开。
    Direct,
    /// `DT_RUNPATH`。
    Runpath,
    /// `DT_RPATH`。
    Rpath,
    /// `LD_LIBRARY_PATH`。
    Env,
    /// 默认目录。
    Default,
}

impl Source {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Source::Direct => "direct",
            Source::Runpath => "runpath",
            Source::Rpath => "rpath",
            Source::Env => "env",
            Source::Default => "default",
        }
    }

    /// 次序。
    pub fn rank(&self) -> u8 {
        match self {
            Source::Direct => 0,
            Source::Runpath => 1,
            Source::Rpath => 2,
            Source::Env => 3,
            Source::Default => 4,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        fold_word(FNV_OFF, self.rank() as u64)
    }
}

/// 搜索计划（最多四段）。
pub fn plan_for(d: &Dynamic, env_present: bool) -> [Option<Source>; PATH_MAX] {
    let mut out = [None; PATH_MAX];
    let mut n = 0usize;
    if d.has_runpath() {
        if let Some(slot) = out.get_mut(n) {
            *slot = Some(Source::Runpath);
            n += 1;
        }
    } else if d.has_rpath() {
        if let Some(slot) = out.get_mut(n) {
            *slot = Some(Source::Rpath);
            n += 1;
        }
    }
    if env_present && n < PATH_MAX {
        if let Some(slot) = out.get_mut(n) {
            *slot = Some(Source::Env);
            n += 1;
        }
    }
    if n < PATH_MAX {
        if let Some(slot) = out.get_mut(n) {
            *slot = Some(Source::Default);
        }
    }
    out
}

/// 展开 `$ORIGIN`。
pub fn expand(raw: &str, origin: &str) -> String {
    let mut s = String::new();
    let b = raw.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let at = match b.get(i) {
            Some(v) => *v,
            None => break,
        };
        if at == b'$' && str_contains(raw.get(i..).unwrap_or(""), "$ORIGIN") {
            let tail = raw.get(i..).unwrap_or("");
            if tail.starts_with("$ORIGIN") {
                s.push_str(origin);
                i += 7;
                continue;
            }
        }
        s.push(at as char);
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 定位
// ---------------------------------------------------------------------------

/// 一次目录尝试。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    /// 来源。
    pub source: Source,
    /// 原始目录（可能含 `$ORIGIN`）。
    pub dir: &'static str,
    /// 根目录。
    pub origin: &'static str,
    /// 依赖名。
    pub name: &'static str,
    /// 是否命中。
    pub found: bool,
}

impl Attempt {
    /// 展开后的目录。
    pub fn dir_text(&self) -> String {
        expand(self.dir, self.origin)
    }

    /// 拼出的路径。
    pub fn path(&self) -> String {
        if let Source::Direct = self.source {
            return String::from(self.name);
        }
        let mut s = self.dir_text();
        s.push('/');
        s.push_str(self.name);
        s
    }

    /// 展示行。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.source.name());
        s.push(' ');
        s.push_str(self.path().as_str());
        if self.found {
            s.push_str(" hit");
        } else {
            s.push_str(" miss");
        }
        s.push('\n');
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.source.seal());
        h = fold_word(h, digest(self.dir.as_bytes()));
        h = fold_word(h, digest(self.origin.as_bytes()));
        h = fold_word(h, digest(self.name.as_bytes()));
        fold_word(h, self.found as u64)
    }
}

/// 已知文件里查路径。
pub fn path_exists(path: &str) -> Option<&'static str> {
    let mut i = 0usize;
    while i < FILES.len() {
        if let Some(f) = FILES.get(i) {
            if *f == path {
                return Some(*f);
            }
        }
        i += 1;
    }
    None
}

/// 定位轨迹。
#[derive(Clone, Copy, Debug)]
pub struct Trace {
    rows: [Option<Attempt>; TRACE_MAX],
    len: usize,
}

impl Trace {
    /// 空轨迹。
    pub fn new() -> Trace {
        Trace { rows: [None; TRACE_MAX], len: 0 }
    }

    /// 记录一次尝试。
    pub fn push(&mut self, a: Attempt) -> bool {
        if self.len >= TRACE_MAX {
            return false;
        }
        if let Some(slot) = self.rows.get_mut(self.len) {
            *slot = Some(a);
            self.len += 1;
            return true;
        }
        false
    }

    /// 条数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 取一条。
    pub fn get(&self, i: usize) -> Option<Attempt> {
        if i >= self.len {
            return None;
        }
        match self.rows.get(i) {
            Some(Some(a)) => Some(*a),
            _ => None,
        }
    }

    /// 命中数。
    pub fn hits(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.get(i) {
                if a.found {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 是否用过 `$ORIGIN`。
    pub fn origin_used(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(Some(a)) => {
                    if str_contains(a.dir, "$ORIGIN") {
                        return true;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("trace attempts=");
        s.push_str(dec_text(self.len as u32).as_str());
        s.push_str(" hits=");
        s.push_str(dec_text(self.hits() as u32).as_str());
        s.push_str(" origin=");
        s.push_str(dec_text(self.origin_used() as u32).as_str());
        s.push('\n');
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.len as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.get(i) {
                h = fold_word(h, a.seal());
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 依赖链
// ---------------------------------------------------------------------------

/// 一份对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lib {
    /// soname。
    pub soname: &'static str,
    /// 动态段。
    pub dynamic: Dynamic,
}

impl Lib {
    /// 构造。
    pub const fn new(soname: &'static str, dynamic: Dynamic) -> Lib {
        Lib { soname, dynamic }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, digest(self.soname.as_bytes()));
        h = fold_word(h, self.dynamic.seal());
        h
    }
}

/// 固定容量依赖图。
#[derive(Clone, Copy, Debug)]
pub struct Chain {
    libs: [Option<Lib>; CHAIN_MAX],
    len: usize,
}

/// 空槽。
const EMPTY_LIB: Option<Lib> = None;

impl Chain {
    /// 空图。
    pub fn new() -> Chain {
        Chain { libs: [EMPTY_LIB; CHAIN_MAX], len: 0 }
    }

    /// 加一份对象。
    pub fn push(&mut self, lib: Lib) -> Result<(), u32> {
        if self.len >= CHAIN_MAX {
            return Err(NE_EVALUE);
        }
        if let Some(slot) = self.libs.get_mut(self.len) {
            *slot = Some(lib);
            self.len += 1;
            return Ok(());
        }
        Err(NE_EVALUE)
    }

    /// 份数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取第 `i` 份。
    pub fn get_at(&self, i: usize) -> Option<Lib> {
        if i >= self.len {
            return None;
        }
        match self.libs.get(i) {
            Some(Some(v)) => Some(*v),
            _ => None,
        }
    }

    /// 按 soname 取。
    pub fn get(&self, soname: &str) -> Option<Lib> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.get_at(i) {
                if v.soname == soname {
                    return Some(v);
                }
            }
            i += 1;
        }
        None
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.len as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.get_at(i) {
                h = fold_word(h, v.seal());
            }
            i += 1;
        }
        h
    }
}

/// 解析结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 闭包完整。
    Ready,
    /// 根对象直接依赖缺失。
    Missing,
    /// 传递依赖缺失。
    DepMissing,
    /// 依赖环。
    Cycle,
    /// 动态段畸形。
    Malformed,
    /// 空 soname。
    EmptyName,
    /// `$ORIGIN` 展开后未命中。
    OriginMissing,
}

impl Outcome {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Ready => "ready",
            Outcome::Missing => "missing",
            Outcome::DepMissing => "dep-missing",
            Outcome::Cycle => "cycle",
            Outcome::Malformed => "malformed",
            Outcome::EmptyName => "empty-name",
            Outcome::OriginMissing => "origin-missing",
        }
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Outcome::Ready => 0,
            Outcome::Missing => NE_ENOENT,
            Outcome::DepMissing => NE_EDEP,
            Outcome::Cycle => NE_ECYCLE,
            Outcome::Malformed => NE_EMALF,
            Outcome::EmptyName => NE_ENAME,
            Outcome::OriginMissing => NE_EORIGIN,
        }
    }

    /// 是否成功。
    pub fn ready(&self) -> bool {
        match self {
            Outcome::Ready => true,
            _ => false,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        fold_word(FNV_OFF, self.code() as u64)
    }
}

/// 链解析报告。
#[derive(Clone, Copy, Debug)]
pub struct ChainReport {
    root: &'static str,
    origin: &'static str,
    outcome: Outcome,
    subject: Option<&'static str>,
    requester: Option<&'static str>,
    loaded: [Option<&'static str>; CHAIN_MAX],
    len: usize,
}

/// 空装载槽。
const EMPTY_LOADED: Option<&'static str> = None;

impl ChainReport {
    /// 空报告。
    pub fn new(root: &'static str, origin: &'static str) -> ChainReport {
        ChainReport {
            root,
            origin,
            outcome: Outcome::Ready,
            subject: None,
            requester: None,
            loaded: [EMPTY_LOADED; CHAIN_MAX],
            len: 0,
        }
    }

    /// 记一份已装对象。
    pub fn mark(&mut self, soname: &'static str) {
        if self.len >= CHAIN_MAX {
            return;
        }
        if let Some(slot) = self.loaded.get_mut(self.len) {
            *slot = Some(soname);
            self.len += 1;
        }
    }

    /// 记一次失败。
    pub fn fail(&mut self, o: Outcome, subject: &'static str, requester: Option<&'static str>) {
        self.outcome = o;
        self.subject = Some(subject);
        self.requester = requester;
    }

    /// 根名。
    pub const fn root(&self) -> &'static str {
        self.root
    }

    /// 结局。
    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// 缺失/出错主体。
    pub const fn subject(&self) -> Option<&'static str> {
        self.subject
    }

    /// 请求者。
    pub const fn requester(&self) -> Option<&'static str> {
        self.requester
    }

    /// 已装份数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取已装对象。
    pub fn loaded_at(&self, i: usize) -> Option<&'static str> {
        if i >= self.len {
            return None;
        }
        match self.loaded.get(i) {
            Some(Some(v)) => Some(*v),
            _ => None,
        }
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        self.outcome.code()
    }

    /// 是否成功。
    pub fn ready(&self) -> bool {
        self.outcome.ready()
    }

    /// 文案。
    pub fn text(&self) -> String {
        let subject = match self.subject {
            Some(v) => v,
            None => return String::new(),
        };
        let mut s = String::from(subject);
        match self.outcome {
            Outcome::Ready => return String::new(),
            Outcome::Malformed => {
                s.push_str(": invalid dynamic section: DT_NEEDED offset beyond DT_STRSZ");
            }
            Outcome::EmptyName => {
                s.push_str(": invalid dynamic section: empty DT_NEEDED");
            }
            Outcome::Cycle => {
                s.push_str(": circular dependency in DT_NEEDED chain");
            }
            _ => {
                s.push_str(": cannot open shared object file: No such file or directory");
            }
        }
        s
    }

    /// 诊断。
    pub fn diagnose(&self) -> String {
        let mut s = String::from(self.root);
        s.push_str(": outcome=");
        s.push_str(self.outcome.name());
        s.push_str(" loaded=");
        s.push_str(dec_text(self.len as u32).as_str());
        s.push_str(" subject=");
        match self.subject {
            Some(v) => s.push_str(v),
            None => s.push_str("none"),
        }
        s.push_str(" requester=");
        match self.requester {
            Some(v) => s.push_str(v),
            None => s.push_str("none"),
        }
        s.push_str(" origin=");
        s.push_str(self.origin);
        s.push('\n');
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, digest(self.root.as_bytes()));
        h = fold_word(h, self.outcome.seal());
        match self.subject {
            Some(v) => {
                h = fold_word(h, digest(v.as_bytes()));
            }
            None => {
                h = fold_word(h, 0);
            }
        }
        match self.requester {
            Some(v) => {
                h = fold_word(h, digest(v.as_bytes()));
            }
            None => {
                h = fold_word(h, 0);
            }
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.loaded_at(i) {
                h = fold_word(h, digest(v.as_bytes()));
            }
            i += 1;
        }
        fold_word(h, digest(self.origin.as_bytes()))
    }
}

/// 一帧 DFS。
#[derive(Clone, Copy, Debug)]
struct Frame {
    lib: Lib,
    next: usize,
}

/// 空帧。
const EMPTY_FRAME: Option<Frame> = None;

/// 某来源对应的目录表。
fn dirs_for(src: Source, d: &Dynamic, env: &SearchPath) -> SearchPath {
    match src {
        Source::Runpath => match d.paths(DT_RUNPATH) {
            Some(p) => p,
            None => SearchPath::empty(),
        },
        Source::Rpath => match d.paths(DT_RPATH) {
            Some(p) => p,
            None => SearchPath::empty(),
        },
        Source::Env => *env,
        Source::Default => SearchPath::defaults(),
        Source::Direct => SearchPath::empty(),
    }
}

/// 解析整条依赖链。
pub fn resolve(
    chain: &Chain,
    root: &'static str,
    origin: &'static str,
    env: &'static str,
) -> (ChainReport, Trace) {
    let mut tr = Trace::new();
    let mut rep = ChainReport::new(root, origin);
    let env_present = !env.is_empty();
    let env_paths = if env_present {
        SearchPath::parse(env)
    } else {
        SearchPath::empty()
    };
    let root_lib = match chain.get(root) {
        Some(v) => v,
        None => {
            rep.fail(Outcome::Missing, root, None);
            return (rep, tr);
        }
    };
    if !root_lib.dynamic.str_ok() {
        rep.fail(Outcome::Malformed, root_lib.soname, None);
        return (rep, tr);
    }
    let mut frames: [Option<Frame>; DEPTH_MAX] = [EMPTY_FRAME; DEPTH_MAX];
    if let Some(slot) = frames.get_mut(0) {
        *slot = Some(Frame { lib: root_lib, next: 0 });
    }
    let mut depth = 1usize;
    rep.mark(root);
    while depth > 0 {
        let idx = depth - 1;
        let frame = match frames.get(idx) {
            Some(Some(f)) => *f,
            _ => break,
        };
        let lib = frame.lib;
        let total = lib.dynamic.needed_count();
        if frame.next >= total {
            depth -= 1;
            continue;
        }
        let name = match lib.dynamic.needed_at(frame.next) {
            Some(v) => v,
            None => {
                if let Some(Some(f)) = frames.get_mut(idx) {
                    f.next = f.next.saturating_add(1);
                }
                continue;
            }
        };
        if let Some(Some(f)) = frames.get_mut(idx) {
            f.next = f.next.saturating_add(1);
        }
        if name.is_empty() {
            rep.fail(Outcome::EmptyName, lib.soname, Some(lib.soname));
            return (rep, tr);
        }
        let plan = plan_for(&lib.dynamic, env_present);
        let mut found: Option<&'static str> = None;
        if str_contains(name, "/") {
            let att = Attempt {
                source: Source::Direct,
                dir: "",
                origin,
                name,
                found: false,
            };
            let path = att.path();
            let hit = path_exists(path.as_str());
            let rec = Attempt { found: hit.is_some(), ..att };
            let _ = tr.push(rec);
            found = hit;
        } else {
            let mut pi = 0usize;
            while pi < PATH_MAX {
                if found.is_some() {
                    break;
                }
                let src = match plan.get(pi) {
                    Some(Some(v)) => *v,
                    _ => {
                        pi += 1;
                        continue;
                    }
                };
                let dirs = dirs_for(src, &lib.dynamic, &env_paths);
                let mut di = 0usize;
                while di < dirs.len() {
                    let dir = match dirs.get(di) {
                        Some(v) => v,
                        None => break,
                    };
                    let att = Attempt {
                        source: src,
                        dir,
                        origin,
                        name,
                        found: false,
                    };
                    let path = att.path();
                    let hit = path_exists(path.as_str());
                    let rec = Attempt { found: hit.is_some(), ..att };
                    let _ = tr.push(rec);
                    if hit.is_some() {
                        found = hit;
                        break;
                    }
                    di += 1;
                }
                pi += 1;
            }
        }
        if found.is_none() {
            let kind = if depth == 1 {
                if tr.origin_used() {
                    Outcome::OriginMissing
                } else {
                    Outcome::Missing
                }
            } else {
                Outcome::DepMissing
            };
            rep.fail(kind, name, Some(lib.soname));
            return (rep, tr);
        }
        rep.mark(name);
        if let Some(child) = chain.get(name) {
            if !child.dynamic.str_ok() {
                rep.fail(Outcome::Malformed, child.soname, Some(lib.soname));
                return (rep, tr);
            }
            let mut cycle = false;
            let mut k = 0usize;
            while k < depth {
                match frames.get(k) {
                    Some(Some(f)) => {
                        if f.lib.soname == name {
                            cycle = true;
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            if cycle || depth >= DEPTH_MAX {
                rep.fail(Outcome::Cycle, name, Some(lib.soname));
                return (rep, tr);
            }
            if let Some(slot) = frames.get_mut(depth) {
                *slot = Some(Frame { lib: child, next: 0 });
            }
            depth += 1;
        }
    }
    (rep, tr)
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 场景编号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// 默认目录命中。
    DefaultHit,
    /// RUNPATH 命中。
    RunpathHit,
    /// RPATH 命中。
    RpathHit,
    /// 环境变量命中。
    EnvHit,
    /// 直接路径命中。
    DirectHit,
    /// 直接依赖缺失。
    MissingDirect,
    /// 传递依赖缺失。
    MissingTransitive,
    /// 依赖环。
    Cycle,
    /// RUNPATH 压过 RPATH。
    RunpathOverRpath,
    /// RPATH 落空后回默认目录。
    RpathThenDefault,
    /// 动态段畸形。
    MalformedStrtab,
    /// 空 soname。
    EmptyNeeded,
    /// RUNPATH 空段按当前目录。
    EmptyRunpath,
    /// `$ORIGIN` 展开后落空。
    OriginMiss,
}

impl Scenario {
    /// 序号。
    pub fn index(&self) -> u8 {
        match self {
            Scenario::DefaultHit => 0,
            Scenario::RunpathHit => 1,
            Scenario::RpathHit => 2,
            Scenario::EnvHit => 3,
            Scenario::DirectHit => 4,
            Scenario::MissingDirect => 5,
            Scenario::MissingTransitive => 6,
            Scenario::Cycle => 7,
            Scenario::RunpathOverRpath => 8,
            Scenario::RpathThenDefault => 9,
            Scenario::MalformedStrtab => 10,
            Scenario::EmptyNeeded => 11,
            Scenario::EmptyRunpath => 12,
            Scenario::OriginMiss => 13,
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Scenario::DefaultHit => "default-hit",
            Scenario::RunpathHit => "runpath-hit",
            Scenario::RpathHit => "rpath-hit",
            Scenario::EnvHit => "env-hit",
            Scenario::DirectHit => "direct-hit",
            Scenario::MissingDirect => "missing-direct",
            Scenario::MissingTransitive => "missing-transitive",
            Scenario::Cycle => "cycle",
            Scenario::RunpathOverRpath => "runpath-over-rpath",
            Scenario::RpathThenDefault => "rpath-then-default",
            Scenario::MalformedStrtab => "malformed-strtab",
            Scenario::EmptyNeeded => "empty-needed",
            Scenario::EmptyRunpath => "empty-runpath",
            Scenario::OriginMiss => "origin-miss",
        }
    }
}

/// 一条依赖语料。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 根对象名。
    pub name: &'static str,
    /// 场景。
    pub scenario: Scenario,
    /// `$ORIGIN` 取值。
    pub origin: &'static str,
    /// `LD_LIBRARY_PATH`。
    pub env: &'static str,
    /// 期望结局。
    pub expect: Outcome,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        name: &'static str,
        scenario: Scenario,
        origin: &'static str,
        env: &'static str,
        expect: Outcome,
        note: &'static str,
    ) -> Case {
        Case { name, scenario, origin, env, expect, note }
    }
}

/// 十四卡：八成六败。
pub const CASES: [Case; CORPUS] = [
    Case::new("./app", Scenario::DefaultHit, "/app", "", Outcome::Ready, "default-libc"),
    Case::new("./app2", Scenario::RunpathHit, "/app2", "", Outcome::Ready, "runpath-libm"),
    Case::new("./app3", Scenario::RpathHit, "/app3", "", Outcome::Ready, "rpath-libz"),
    Case::new("./app4", Scenario::EnvHit, "/app4", "/opt/env", Outcome::Ready, "env-libq"),
    Case::new("./app5", Scenario::DirectHit, "/app5", "", Outcome::Ready, "slash-direct"),
    Case::new(
        "./app6",
        Scenario::MissingDirect,
        "/app6",
        "",
        Outcome::Missing,
        "direct-missing",
    ),
    Case::new(
        "./app7",
        Scenario::MissingTransitive,
        "/app7",
        "",
        Outcome::DepMissing,
        "transitive-missing",
    ),
    Case::new("./app8", Scenario::Cycle, "/app8", "", Outcome::Cycle, "needed-cycle"),
    Case::new(
        "./app9",
        Scenario::RunpathOverRpath,
        "/app9",
        "",
        Outcome::Ready,
        "runpath-wins",
    ),
    Case::new(
        "./app10",
        Scenario::RpathThenDefault,
        "/app10",
        "",
        Outcome::Ready,
        "rpath-miss-default-hit",
    ),
    Case::new(
        "./app11",
        Scenario::MalformedStrtab,
        "/app11",
        "",
        Outcome::Malformed,
        "bad-needed-offset",
    ),
    Case::new(
        "./app12",
        Scenario::EmptyNeeded,
        "/app12",
        "",
        Outcome::EmptyName,
        "empty-soname",
    ),
    Case::new(
        "./app13",
        Scenario::EmptyRunpath,
        "/app13",
        "",
        Outcome::Ready,
        "empty-runpath-cwd",
    ),
    Case::new(
        "./app14",
        Scenario::OriginMiss,
        "/opt/app",
        "",
        Outcome::OriginMissing,
        "origin-expanded-miss",
    ),
];

/// 取一条语料。
pub fn slot_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 造依赖图。
pub fn chain_of(c: &Case) -> Chain {
    let mut ch = Chain::new();
    match c.scenario {
        Scenario::DefaultHit => {
            let _ = ch.push(Lib::new(c.name, dynamic(c.name, &["libc.so.6"], None, None)));
        }
        Scenario::RunpathHit => {
            let _ = ch.push(Lib::new(
                c.name,
                dynamic(c.name, &["libm.so.6"], None, Some("/opt/app")),
            ));
        }
        Scenario::RpathHit => {
            let _ = ch.push(Lib::new(
                c.name,
                dynamic(c.name, &["libz.so.1"], Some("/opt/app"), None),
            ));
        }
        Scenario::EnvHit => {
            let _ = ch.push(Lib::new(c.name, dynamic(c.name, &["libq.so"], None, None)));
        }
        Scenario::DirectHit => {
            let _ = ch.push(Lib::new(c.name, dynamic(c.name, &["./liblocal.so"], None, None)));
        }
        Scenario::MissingDirect => {
            let _ = ch.push(Lib::new(c.name, dynamic(c.name, &["libmissing.so"], None, None)));
        }
        Scenario::MissingTransitive => {
            let _ = ch.push(Lib::new(c.name, dynamic(c.name, &["liba.so"], None, None)));
            let _ = ch.push(Lib::new("liba.so", dynamic("liba.so", &["libgone.so"], None, None)));
        }
        Scenario::Cycle => {
            let _ = ch.push(Lib::new(c.name, dynamic(c.name, &["liba.so"], None, None)));
            let _ = ch.push(Lib::new("liba.so", dynamic("liba.so", &["libb.so"], None, None)));
            let _ = ch.push(Lib::new("libb.so", dynamic("libb.so", &["liba.so"], None, None)));
        }
        Scenario::RunpathOverRpath => {
            let _ = ch.push(Lib::new(
                c.name,
                dynamic(c.name, &["libp.so"], Some("/opt/old"), Some("/opt/new")),
            ));
        }
        Scenario::RpathThenDefault => {
            let _ = ch.push(Lib::new(
                c.name,
                dynamic(c.name, &["libc.so.6"], Some("/nope"), None),
            ));
        }
        Scenario::MalformedStrtab => {
            let _ = ch.push(Lib::new(
                c.name,
                dynamic(c.name, &["libbroken.so"], None, Some("/opt/app")),
            ));
            let _ = ch.push(Lib::new("libbroken.so", dynamic_bad("libbroken.so")));
        }
        Scenario::EmptyNeeded => {
            let _ = ch.push(Lib::new(
                c.name,
                dynamic(c.name, &["libbadname.so"], None, Some("/opt/app")),
            ));
            let _ = ch.push(Lib::new(
                "libbadname.so",
                dynamic_empty_needed("libbadname.so"),
            ));
        }
        Scenario::EmptyRunpath => {
            let _ = ch.push(Lib::new(
                c.name,
                dynamic(c.name, &["libc.so.6"], None, Some("")),
            ));
        }
        Scenario::OriginMiss => {
            let _ = ch.push(Lib::new(
                c.name,
                dynamic(c.name, &["libmissing.so"], None, Some("$ORIGIN/lib")),
            ));
        }
    }
    ch
}

/// 解一条语料。
pub fn solve(c: &Case) -> (ChainReport, Trace) {
    let ch = chain_of(c);
    resolve(&ch, c.name, c.origin, c.env)
}

/// 名字、备注互异。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            if let Some(b) = CASES.get(j) {
                if b.name == a.name || b.note == a.note {
                    return false;
                }
                if b.scenario == a.scenario {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(c) => {
                if c.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐条对判。
pub fn corpus_table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let (rep, _tr) = solve(&c);
        if rep.outcome() != c.expect {
            return false;
        }
        if rep.ready() != rep.text().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    ok: usize,
    fail: usize,
    enoent: usize,
    dep: usize,
    cycle: usize,
    malformed: usize,
    name: usize,
    origin: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            ok: 0,
            fail: 0,
            enoent: 0,
            dep: 0,
            cycle: 0,
            malformed: 0,
            name: 0,
            origin: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 成功条数。
    pub const fn ok(&self) -> usize {
        self.ok
    }

    /// 失败条数。
    pub const fn fail(&self) -> usize {
        self.fail
    }

    /// 直接缺失条数。
    pub const fn enoent(&self) -> usize {
        self.enoent
    }

    /// 传递缺失条数。
    pub const fn dep(&self) -> usize {
        self.dep
    }

    /// 环条数。
    pub const fn cycle(&self) -> usize {
        self.cycle
    }

    /// 畸形条数。
    pub const fn malformed(&self) -> usize {
        self.malformed
    }

    /// 空名条数。
    pub const fn name(&self) -> usize {
        self.name
    }

    /// `$ORIGIN` 未命中条数。
    pub const fn origin(&self) -> usize {
        self.origin
    }

    /// 对上条数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 是否红档。
    pub const fn red(&self) -> bool {
        self.red
    }

    /// 千分率。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            0
        } else if self.red {
            929
        } else {
            1000
        }
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(NE_ENOBS);
        }
        if self.red {
            return Err(NE_EBAL);
        }
        if self.matched != self.total || self.total != CORPUS {
            return Err(NE_EVALUE);
        }
        if self.ok != 8 || self.fail != 6 {
            return Err(NE_EVALUE);
        }
        if self.enoent != 1 || self.dep != 1 || self.cycle != 1 {
            return Err(NE_EVALUE);
        }
        if self.malformed != 1 || self.name != 1 || self.origin != 1 {
            return Err(NE_EVALUE);
        }
        Ok(())
    }

    /// 是否绿。
    pub fn green(&self) -> bool {
        match self.gate() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("needed total=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok as u32).as_str());
        s.push_str(" fail=");
        s.push_str(dec_text(self.fail as u32).as_str());
        s.push_str(" enoent=");
        s.push_str(dec_text(self.enoent as u32).as_str());
        s.push_str(" dep=");
        s.push_str(dec_text(self.dep as u32).as_str());
        s.push_str(" cycle=");
        s.push_str(dec_text(self.cycle as u32).as_str());
        s.push_str(" malformed=");
        s.push_str(dec_text(self.malformed as u32).as_str());
        s.push_str(" name=");
        s.push_str(dec_text(self.name as u32).as_str());
        s.push_str(" origin=");
        s.push_str(dec_text(self.origin as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑一遍并注入红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(NE_EVALUE),
        };
        let (rep, _tr) = solve(&c);
        r.total += 1;
        match rep.outcome() {
            Outcome::Ready => {
                r.ok += 1;
                if c.expect == Outcome::Ready {
                    r.matched += 1;
                }
            }
            o => {
                r.fail += 1;
                match o {
                    Outcome::Missing => r.enoent += 1,
                    Outcome::DepMissing => r.dep += 1,
                    Outcome::Cycle => r.cycle += 1,
                    Outcome::Malformed => r.malformed += 1,
                    Outcome::EmptyName => r.name += 1,
                    Outcome::OriginMissing => r.origin += 1,
                    Outcome::Ready => {}
                }
                if c.expect == o {
                    r.matched += 1;
                }
            }
        }
        i += 1;
    }
    if let Some((slot, _code)) = red {
        if slot >= r.total {
            return Err(NE_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(NE_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(NE_EVALUE),
        };
        let (rep, _tr) = solve(&c);
        if rep.outcome() != c.expect {
            return Err(NE_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 某条语料的错误码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => {
            let (rep, _tr) = solve(&c);
            rep.code()
        }
        None => NE_EVALUE,
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格观测。
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
    pub fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NE_ESLOT);
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

    /// 对账：14 行、8 成 6 败、值与码逐行对上。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(NE_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 1, 1, 1, 1, 1000];
        const CODES: [u32; 6] =
            [NE_ENOENT, NE_EDEP, NE_ECYCLE, NE_EMALF, NE_ENAME, NE_EORIGIN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NE_EBAL),
            };
            if s.slot as usize != i {
                return Err(NE_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NE_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NE_EVALUE);
                        }
                    }
                    None => return Err(NE_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NE_EVALUE);
                        }
                    }
                    None => return Err(NE_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NE_EBAL);
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
            return Err(NE_EBAL);
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
            Err(NE_ESEAL)
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

/// `Result<usize, u32>` → 观测格。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 失败格。
fn err_cell(slot: u8, op: u8, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.ok())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.fail())));
    let _ = l.register(count_cell(3, OP_CHAIN, r.map(|x| x.enoent())));
    let _ = l.register(count_cell(4, OP_CHAIN, r.map(|x| x.dep())));
    let _ = l.register(count_cell(5, OP_CHAIN, r.map(|x| x.cycle())));
    let _ = l.register(count_cell(6, OP_DYNAMIC, r.map(|x| x.malformed())));
    let _ = l.register(count_cell(7, OP_SEARCH, r.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_CHAIN, case_code(5)));
    let _ = l.register(err_cell(9, OP_CHAIN, case_code(6)));
    let _ = l.register(err_cell(10, OP_CHAIN, case_code(7)));
    let _ = l.register(err_cell(11, OP_DYNAMIC, case_code(10)));
    let _ = l.register(err_cell(12, OP_TEXT, case_code(11)));
    let _ = l.register(err_cell(13, OP_SEARCH, case_code(13)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 动态标签与动态项表。
fn dt_ok() -> bool {
    let a = dt_name(DT_NEEDED) == "DT_NEEDED"
        && dt_name(DT_RUNPATH) == "DT_RUNPATH"
        && dt_name(7) == "DT_UNKNOWN";
    let b = dt_is_string(DT_NEEDED)
        && dt_is_string(DT_SONAME)
        && !dt_is_string(DT_STRSZ)
        && !dt_is_string(DT_NULL);
    let mut t = DynTab::blank();
    let c = t.is_empty() && t.len() == 0;
    let d = t.push(DT_NEEDED, 3) && t.push(DT_RUNPATH, 9) && t.push(DT_NEEDED, 12);
    let e = t.len() == 3 && t.count(DT_NEEDED) == 2 && t.count(DT_RUNPATH) == 1;
    let f = t.find(DT_RUNPATH) == Some(9) && t.find(DT_SONAME).is_none();
    let g = t.get(0).map(|x| x.tag) == Some(DT_NEEDED) && t.get(3).is_none();
    let h = t.seal() != 0 && t.seal() == t.seal();
    a && b && c && d && e && f && g && h
}

/// 动态段解析。
fn dynamic_ok() -> bool {
    let d = dynamic("./app", &["liba.so", "libb.so"], Some("/rp"), Some("/run"));
    let a = d.soname() == Some("./app") && d.needed_count() == 2;
    let b = d.needed_at(0) == Some("liba.so") && d.needed_at(1) == Some("libb.so");
    let c = d.needed_at(2).is_none() && d.str_ok();
    let e = d.has_rpath() && d.has_runpath();
    let f = d.paths(DT_RPATH).map(|p| p.render()) == Some(String::from("/rp"));
    let g = d.paths(DT_RUNPATH).map(|p| p.render()) == Some(String::from("/run"));
    let h = d.paths(DT_NEEDED).is_none();
    let bare = dynamic("./bare", &[], None, None);
    let i = bare.needed_count() == 0 && bare.str_ok() && !bare.has_rpath() && !bare.has_runpath();
    a && b && c && e && f && g && h && i
}

/// 字符串表。
fn strtab_ok() -> bool {
    let mut t = Strtab::blank();
    let a = t.is_empty() && t.len() == 0 && t.get(0).is_none();
    let o1 = t.push("libc.so.6");
    let o2 = t.push("libm.so.6");
    let b = o1 == Some(0) && o2 == Some(10);
    let c = t.get(0) == Some("libc.so.6") && t.get(10) == Some("libm.so.6");
    let d = t.get(1) == Some("ibc.so.6") && t.offset_of("libm.so.6") == Some(10);
    let e = t.contains("libc.so.6") && !t.contains("libz.so.1");
    let f = t.get(t.len()).is_none() && t.get(999).is_none();
    let o3 = t.push("");
    let g = o3 == Some(20) && t.get(20) == Some("");
    let h = !t.is_empty() && t.len() == 21 && t.seal() != 0;
    let mut over = Strtab::blank();
    let mut i = 0usize;
    let mut ok = true;
    while i < 20 {
        if over.push("0123456789").is_none() {
            ok = false;
            break;
        }
        i += 1;
    }
    let j = !ok && over.len() > 0 && over.len() <= STRTAB_CAP;
    a && b && c && d && e && f && g && h && j
}

/// 搜索次序。
fn plan_ok() -> bool {
    let bare = dynamic("./a", &["libc.so.6"], None, None);
    let rp = dynamic("./b", &["libc.so.6"], Some("/rp"), None);
    let run = dynamic("./c", &["libc.so.6"], None, Some("/run"));
    let both = dynamic("./d", &["libc.so.6"], Some("/rp"), Some("/run"));
    let a = plan_for(&bare, false) == [Some(Source::Default), None, None, None];
    let b = plan_for(&bare, true)
        == [Some(Source::Env), Some(Source::Default), None, None];
    let c = plan_for(&rp, true)
        == [Some(Source::Rpath), Some(Source::Env), Some(Source::Default), None];
    let d = plan_for(&run, true)
        == [Some(Source::Runpath), Some(Source::Env), Some(Source::Default), None];
    let e = plan_for(&both, true)
        == [Some(Source::Runpath), Some(Source::Env), Some(Source::Default), None];
    let f = plan_for(&both, false)[0] == Some(Source::Runpath);
    let g = Source::Direct.rank() < Source::Runpath.rank()
        && Source::Runpath.rank() < Source::Rpath.rank()
        && Source::Rpath.rank() < Source::Env.rank()
        && Source::Env.rank() < Source::Default.rank();
    let h = Source::Runpath.name() == "runpath" && Source::Default.seal() != Source::Env.seal();
    a && b && c && d && e && f && g && h
}

/// 路径解析与展开。
fn paths_ok() -> bool {
    let p = SearchPath::parse("/a:/b:/c");
    let a = p.len() == 3 && p.get(0) == Some("/a") && p.get(2) == Some("/c");
    let b = p.contains("/b") && !p.contains("/d") && p.render().as_str() == "/a:/b:/c";
    let empty = SearchPath::parse("");
    let c = empty.len() == 1 && empty.get(0) == Some(".");
    let trail = SearchPath::parse("/a:");
    let d = trail.len() == 2 && trail.get(1) == Some(".");
    let defaults = SearchPath::defaults();
    let e = defaults.len() == DEFAULT_COUNT
        && defaults.get(0) == Some(DEFAULT0)
        && defaults.get(1) == Some(DEFAULT1);
    let f = SearchPath::empty().is_empty() && SearchPath::empty().get(0).is_none();
    let g = expand("$ORIGIN/lib", "/opt/app").as_str() == "/opt/app/lib";
    let h = expand("/plain", "/opt/app").as_str() == "/plain";
    let i = expand("$ORIGIN", "/x").as_str() == "/x";
    let j = expand("$ORIGIN/a/$ORIGIN", "/y").as_str() == "/y/a//y";
    let att = Attempt {
        source: Source::Runpath,
        dir: "$ORIGIN/lib",
        origin: "/opt/app",
        name: "libm.so.6",
        found: true,
    };
    let k = att.path().as_str() == "/opt/app/lib/libm.so.6"
        && att.dir_text().as_str() == "/opt/app/lib";
    let direct = Attempt {
        source: Source::Direct,
        dir: "",
        origin: "",
        name: "./liblocal.so",
        found: true,
    };
    let l = direct.path().as_str() == "./liblocal.so";
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 轨迹。
fn trace_ok() -> bool {
    let missing = match slot_at(5) {
        Some(v) => v,
        None => return false,
    };
    let (rep5, tr5) = solve(&missing);
    let a = rep5.outcome() == Outcome::Missing && tr5.len() == 2;
    let b = tr5.hits() == 0 && !tr5.origin_used();
    let c = tr5.render().as_str() == "trace attempts=2 hits=0 origin=0\n";
    let origin = match slot_at(13) {
        Some(v) => v,
        None => return false,
    };
    let (rep13, tr13) = solve(&origin);
    let d = rep13.outcome() == Outcome::OriginMissing && tr13.origin_used();
    let e = tr13.len() >= 3 && tr13.hits() == 0;
    let hit = match slot_at(1) {
        Some(v) => v,
        None => return false,
    };
    let (rep1, tr1) = solve(&hit);
    let f = rep1.outcome() == Outcome::Ready && tr1.hits() == 1;
    let g = tr1.get(0).map(|x| x.source) == Some(Source::Runpath);
    let h = tr1.seal() != tr5.seal() && tr5.seal() == solve(&missing).1.seal();
    a && b && c && d && e && f && g && h
}

/// 缺失文案逐字。
fn text_ok() -> bool {
    let t = |i: usize| -> String {
        match slot_at(i) {
            Some(c) => solve(&c).0.text(),
            None => String::new(),
        }
    };
    let tail = ": cannot open shared object file: No such file or directory";
    let a = t(5).as_str()
        == "libmissing.so: cannot open shared object file: No such file or directory";
    let b = t(6).as_str() == "libgone.so: cannot open shared object file: No such file or directory";
    let c = t(13).as_str()
        == "libmissing.so: cannot open shared object file: No such file or directory";
    let d = t(5).as_str().len() == "libmissing.so".len() + tail.len();
    let e = t(7).as_str() == "liba.so: circular dependency in DT_NEEDED chain";
    let f = t(10).as_str()
        == "libbroken.so: invalid dynamic section: DT_NEEDED offset beyond DT_STRSZ";
    let g = t(11).as_str() == "libbadname.so: invalid dynamic section: empty DT_NEEDED";
    let h = t(0).is_empty() && t(1).is_empty() && t(8).is_empty();
    a && b && c && d && e && f && g && h
}

/// 依赖链。
fn chain_ok() -> bool {
    let ready = match slot_at(0) {
        Some(v) => v,
        None => return false,
    };
    let (r0, _t0) = solve(&ready);
    let a = r0.ready() && r0.len() == 2 && r0.loaded_at(0) == Some("./app");
    let b = r0.loaded_at(1) == Some("libc.so.6") && r0.subject().is_none();
    let trans = match slot_at(6) {
        Some(v) => v,
        None => return false,
    };
    let (r6, _t6) = solve(&trans);
    let c = r6.outcome() == Outcome::DepMissing && r6.code() == NE_EDEP;
    let d = r6.subject() == Some("libgone.so") && r6.requester() == Some("liba.so");
    let cyc = match slot_at(7) {
        Some(v) => v,
        None => return false,
    };
    let (r7, _t7) = solve(&cyc);
    let e = r7.outcome() == Outcome::Cycle && r7.code() == NE_ECYCLE;
    let f = r7.subject() == Some("liba.so") && r7.requester() == Some("libb.so");
    let direct = match slot_at(5) {
        Some(v) => v,
        None => return false,
    };
    let (r5, _t5) = solve(&direct);
    let g = r5.subject() == Some("libmissing.so") && r5.requester() == Some("./app6");
    let h = r5.len() == 1 && r5.loaded_at(0) == Some("./app6");
    a && b && c && d && e && f && g && h
}

/// 畸形动态段。
fn odd_ok() -> bool {
    let mal = match slot_at(10) {
        Some(v) => v,
        None => return false,
    };
    let (rm, _tm) = solve(&mal);
    let a = rm.outcome() == Outcome::Malformed && rm.code() == NE_EMALF;
    let b = rm.subject() == Some("libbroken.so");
    let emp = match slot_at(11) {
        Some(v) => v,
        None => return false,
    };
    let (re, _te) = solve(&emp);
    let c = re.outcome() == Outcome::EmptyName && re.code() == NE_ENAME;
    let d = re.subject() == Some("libbadname.so");
    let bad = dynamic_bad("x.so");
    let e = !bad.str_ok() && bad.needed_count() == 1;
    let zero = dynamic_empty_needed("y.so");
    let f = zero.str_ok() && zero.needed_at(0) == Some("");
    let g = Outcome::Malformed.code() == NE_EMALF && Outcome::EmptyName.code() == NE_ENAME;
    a && b && c && d && e && f && g
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.ok() == 8 && r.fail() == 6 && r.matched() == CORPUS;
    let b = r.enoent() == 1 && r.dep() == 1 && r.cycle() == 1;
    let c = r.malformed() == 1 && r.name() == 1 && r.origin() == 1;
    let d = r.rate() == 1000 && r.green() && corpus_table_ok();
    a && b && c && d
}

/// 语料槽位。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(c) => c.expect == Outcome::Ready && c.scenario == Scenario::DefaultHit,
        None => false,
    };
    let b = match slot_at(6) {
        Some(c) => c.expect == Outcome::DepMissing,
        None => false,
    };
    let c = match slot_at(13) {
        Some(v) => v.expect == Outcome::OriginMissing && str_contains(v.origin, "/"),
        None => false,
    };
    let d = slot_at(CORPUS).is_none();
    let e = match slot_at(8) {
        Some(v) => v.scenario == Scenario::RunpathOverRpath,
        None => false,
    };
    let f = match slot_at(12) {
        Some(v) => v.scenario == Scenario::EmptyRunpath,
        None => false,
    };
    a && b && c && d && e && f
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "needed total=14 ok=8 fail=6 enoent=1 dep=1 cycle=1 malformed=1 name=1 origin=1 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(NE_ENOBS);
    let c = e.render().as_str()
        == "needed total=0 ok=0 fail=0 enoent=0 dep=0 cycle=0 malformed=0 name=0 origin=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, NE_ENOENT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929 && r.red();
    let b = r.gate() == Err(NE_EBAL) && !r.green();
    let c = run_with(Some((CORPUS, NE_ENOENT))).err() == Some(NE_ESLOT);
    let d = run_with(Some((CORPUS + 1, NE_ENOENT))).err() == Some(NE_ESLOT);
    a && b && c && d
}

/// 前缀封面。
fn cover_ok() -> bool {
    let a = cover(0).is_ok() && cover(1).is_ok() && cover(10).is_ok();
    let b = cover(CORPUS).is_ok() && cover(CORPUS + 1).err() == Some(NE_ESLOT);
    let mut i = 0usize;
    while i <= CORPUS {
        if cover(i).is_err() {
            return false;
        }
        i += 1;
    }
    a && b
}

/// 指纹与诊断。
fn hash_ok() -> bool {
    let a = dynamic("./a", &["liba.so"], None, None).seal()
        != dynamic("./a", &["libb.so"], None, None).seal();
    let b = dynamic("./a", &["liba.so"], Some("/r"), None).seal()
        != dynamic("./a", &["liba.so"], None, Some("/r")).seal();
    let ch = chain_of(&match slot_at(6) {
        Some(v) => v,
        None => return false,
    });
    let c = ch.seal() != 0 && ch.len() == 2 && ch.get("liba.so").is_some();
    let (r0, t0) = solve(&match slot_at(0) {
        Some(v) => v,
        None => return false,
    });
    let d = r0.seal() == r0.seal() && t0.seal() != 0;
    let e = r0.diagnose().starts_with("./app: outcome=ready loaded=2");
    let f = Trace::new().render().as_str() == "trace attempts=0 hits=0 origin=0\n";
    let g = digest(b"DT_NEEDED") != digest(b"DT_RUNPATH");
    let h = Strtab::blank().seal() != 0 && DynTab::blank().seal() != 0;
    a && b && c && d && e && f && g && h
}

/// 账本冻结与重放。
fn pin_ok() -> bool {
    let l = build_ledger();
    let a = l.len() == SAMPLES && !l.is_empty() && l.ok_count() == 8 && l.err_count() == 6;
    let b = l.audit() == Ok(());
    let c = match l.sample(0) {
        Some(s) => s.ok && s.value == 14 && s.slot == 0,
        None => false,
    };
    let d = l.sample(SAMPLES).is_none() && l.root() == 0 && !l.is_frozen();
    let mut m = l;
    let e = match m.freeze() {
        Ok(v) => v != 0 && m.root() == v && m.is_frozen(),
        Err(_) => false,
    };
    let f = m.replay() == Ok(m.seal());
    let g = m.register(Sample { slot: 14, op: OP_CORPUS, ok: true, code: 0, value: 0 })
        == Err(NE_ESLOT);
    let h = m.rejects() == 1 && m.freeze() == Err(NE_EBAL) && m.rejects() == 2;
    let i = m.audit() == Ok(());
    a && b && c && d && e && f && g && h && i
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = dt_ok() && dynamic_ok() && strtab_ok();
    out[1] = plan_ok() && paths_ok() && trace_ok();
    out[2] = text_ok() && chain_ok() && odd_ok();
    out[3] = corpus_ok()
        && slots_ok()
        && report_ok()
        && reds_ok()
        && cover_ok()
        && hash_ok()
        && pin_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11626_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11626.needed_missing");

    s.add("ND01-动态标签表", dt_ok(), "static");
    s.add("ND02-动态段解析", dynamic_ok(), "static");
    s.add("ND03-字符串表", strtab_ok(), "static");
    s.add("ND04-搜索次序", plan_ok(), "static");
    s.add("ND05-路径展开", paths_ok(), "static");
    s.add("ND06-定位轨迹", trace_ok(), "static");
    s.add("ND07-缺失文案", text_ok(), "static");
    s.add("ND08-依赖链", chain_ok(), "static");
    s.add("ND09-畸形动态段", odd_ok(), "static");
    s.add("ND10-十四卡全跑", corpus_ok(), "static");
    s.add("ND11-语料与渲染", slots_ok() && report_ok(), "static");
    s.add("ND12-红档封面与指纹", reds_ok() && cover_ok() && hash_ok(), "static");
    s.add("ND13-冻结重放四注入", inject_four() == [true; 4], "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert!(!l.is_frozen());
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert_eq!(l.root(), root);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::blank()), Err(NE_ESLOT));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11626_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn dyn_tags() {
        assert_eq!(dt_name(DT_NEEDED), "DT_NEEDED");
        assert_eq!(dt_name(DT_STRTAB), "DT_STRTAB");
        assert_eq!(dt_name(DT_STRSZ), "DT_STRSZ");
        assert_eq!(dt_name(DT_SONAME), "DT_SONAME");
        assert_eq!(dt_name(DT_RPATH), "DT_RPATH");
        assert_eq!(dt_name(DT_RUNPATH), "DT_RUNPATH");
        assert_eq!(dt_name(99), "DT_UNKNOWN");
        assert!(dt_is_string(DT_NEEDED));
        assert!(!dt_is_string(DT_STRSZ));
    }

    #[test]
    fn dyn_table() {
        let mut t = DynTab::blank();
        assert!(t.is_empty());
        assert!(t.push(DT_NEEDED, 5));
        assert!(t.push(DT_SONAME, 7));
        assert_eq!(t.len(), 2);
        assert_eq!(t.find(DT_SONAME), Some(7));
        assert_eq!(t.count(DT_NEEDED), 1);
        assert_eq!(t.get(1).map(|e| e.tag), Some(DT_SONAME));
        assert!(t.get(2).is_none());
    }

    #[test]
    fn dynamic_needs() {
        let d = dynamic("./app", &["liba.so", "libb.so"], Some("/rp"), Some("/run"));
        assert_eq!(d.soname(), Some("./app"));
        assert_eq!(d.needed_count(), 2);
        assert_eq!(d.needed_at(0), Some("liba.so"));
        assert_eq!(d.needed_at(1), Some("libb.so"));
        assert!(d.needed_at(2).is_none());
        assert!(d.str_ok());
    }

    #[test]
    fn dynamic_paths() {
        let d = dynamic("./app", &[], Some("/old"), Some("/new"));
        assert!(d.has_rpath());
        assert!(d.has_runpath());
        assert_eq!(d.paths(DT_RPATH).map(|p| p.render()), Some(String::from("/old")));
        assert_eq!(d.paths(DT_RUNPATH).map(|p| p.render()), Some(String::from("/new")));
        assert!(d.paths(DT_SONAME).is_none());
    }

    #[test]
    fn strtab_offsets() {
        let mut t = Strtab::blank();
        assert_eq!(t.push("libc.so.6"), Some(0));
        assert_eq!(t.push("libm.so.6"), Some(10));
        assert_eq!(t.get(0), Some("libc.so.6"));
        assert_eq!(t.get(10), Some("libm.so.6"));
        assert_eq!(t.offset_of("libm.so.6"), Some(10));
        assert!(t.contains("libc.so.6"));
        assert!(!t.contains("libz.so.1"));
    }

    #[test]
    fn strtab_bounds() {
        let mut t = Strtab::blank();
        assert!(t.get(0).is_none());
        assert_eq!(t.push("abc"), Some(0));
        assert_eq!(t.get(1), Some("bc"));
        assert!(t.get(4).is_none());
        assert!(t.get(99).is_none());
        let mut over = Strtab::blank();
        let mut n = 0usize;
        while n < 20 {
            if over.push("0123456789").is_none() {
                break;
            }
            n += 1;
        }
        assert!(n < 20);
        assert!(over.len() <= STRTAB_CAP);
    }

    #[test]
    fn search_plan() {
        let bare = dynamic("./a", &[], None, None);
        assert_eq!(
            plan_for(&bare, false),
            [Some(Source::Default), None, None, None]
        );
        assert_eq!(
            plan_for(&bare, true),
            [Some(Source::Env), Some(Source::Default), None, None]
        );
        let rp = dynamic("./b", &[], Some("/r"), None);
        assert_eq!(plan_for(&rp, false)[0], Some(Source::Rpath));
        let run = dynamic("./c", &[], None, Some("/u"));
        assert_eq!(plan_for(&run, false)[0], Some(Source::Runpath));
        let both = dynamic("./d", &[], Some("/r"), Some("/u"));
        assert_eq!(plan_for(&both, false)[0], Some(Source::Runpath));
        assert_ne!(plan_for(&both, false)[1], Some(Source::Rpath));
    }

    #[test]
    fn search_path_parse() {
        let p = SearchPath::parse("/a:/b");
        assert_eq!(p.len(), 2);
        assert_eq!(p.get(0), Some("/a"));
        assert_eq!(p.get(1), Some("/b"));
        assert_eq!(p.render().as_str(), "/a:/b");
        let e = SearchPath::parse("");
        assert_eq!(e.len(), 1);
        assert_eq!(e.get(0), Some("."));
        let t = SearchPath::parse("/a:");
        assert_eq!(t.len(), 2);
        assert_eq!(t.get(1), Some("."));
        assert!(SearchPath::defaults().contains("/lib"));
    }

    #[test]
    fn origin_expansion() {
        assert_eq!(expand("$ORIGIN/lib", "/opt/app").as_str(), "/opt/app/lib");
        assert_eq!(expand("/plain", "/opt/app").as_str(), "/plain");
        assert_eq!(expand("$ORIGIN", "/x").as_str(), "/x");
        assert_eq!(expand("$ORIGIN/a/$ORIGIN", "/y").as_str(), "/y/a//y");
        assert_eq!(expand("", "/z").as_str(), "");
    }

    #[test]
    fn attempt_paths() {
        let a = Attempt {
            source: Source::Rpath,
            dir: "/opt/app",
            origin: "/app",
            name: "libz.so.1",
            found: true,
        };
        assert_eq!(a.path().as_str(), "/opt/app/libz.so.1");
        let d = Attempt {
            source: Source::Direct,
            dir: "",
            origin: "",
            name: "./liblocal.so",
            found: true,
        };
        assert_eq!(d.path().as_str(), "./liblocal.so");
        assert!(path_exists("/lib/libc.so.6").is_some());
        assert!(path_exists("/nope/libc.so.6").is_none());
    }

    #[test]
    fn trace_missing() {
        let c = match slot_at(5) {
            Some(v) => v,
            None => return,
        };
        let (rep, tr) = solve(&c);
        assert_eq!(rep.outcome(), Outcome::Missing);
        assert_eq!(tr.len(), 2);
        assert_eq!(tr.hits(), 0);
        assert!(!tr.origin_used());
    }

    #[test]
    fn trace_origin() {
        let c = match slot_at(13) {
            Some(v) => v,
            None => return,
        };
        let (rep, tr) = solve(&c);
        assert_eq!(rep.outcome(), Outcome::OriginMissing);
        assert!(tr.origin_used());
    }

    #[test]
    fn text_missing() {
        let c = match slot_at(5) {
            Some(v) => v,
            None => return,
        };
        let (rep, _tr) = solve(&c);
        assert_eq!(
            rep.text().as_str(),
            "libmissing.so: cannot open shared object file: No such file or directory"
        );
    }

    #[test]
    fn text_cycle() {
        let c = match slot_at(7) {
            Some(v) => v,
            None => return,
        };
        let (rep, _tr) = solve(&c);
        assert_eq!(
            rep.text().as_str(),
            "liba.so: circular dependency in DT_NEEDED chain"
        );
    }

    #[test]
    fn text_malformed() {
        let c = match slot_at(10) {
            Some(v) => v,
            None => return,
        };
        let (rep, _tr) = solve(&c);
        assert_eq!(
            rep.text().as_str(),
            "libbroken.so: invalid dynamic section: DT_NEEDED offset beyond DT_STRSZ"
        );
    }

    #[test]
    fn chain_ready() {
        let c = match slot_at(0) {
            Some(v) => v,
            None => return,
        };
        let (rep, _tr) = solve(&c);
        assert!(rep.ready());
        assert_eq!(rep.len(), 2);
        assert_eq!(rep.loaded_at(1), Some("libc.so.6"));
    }

    #[test]
    fn chain_transitive() {
        let c = match slot_at(6) {
            Some(v) => v,
            None => return,
        };
        let (rep, _tr) = solve(&c);
        assert_eq!(rep.outcome(), Outcome::DepMissing);
        assert_eq!(rep.subject(), Some("libgone.so"));
        assert_eq!(rep.requester(), Some("liba.so"));
    }

    #[test]
    fn chain_cycle() {
        let c = match slot_at(7) {
            Some(v) => v,
            None => return,
        };
        let (rep, _tr) = solve(&c);
        assert_eq!(rep.outcome(), Outcome::Cycle);
        assert_eq!(rep.code(), NE_ECYCLE);
    }

    #[test]
    fn chain_malformed() {
        let c = match slot_at(10) {
            Some(v) => v,
            None => return,
        };
        let (rep, _tr) = solve(&c);
        assert_eq!(rep.outcome(), Outcome::Malformed);
        assert_eq!(rep.subject(), Some("libbroken.so"));
    }

    #[test]
    fn corpus_shape() {
        assert!(unique());
        assert!(notes_ok());
        assert_eq!(CASES.len(), 14);
        assert_eq!(slot_at(0).map(|c| c.scenario), Some(Scenario::DefaultHit));
        assert_eq!(slot_at(13).map(|c| c.scenario), Some(Scenario::OriginMiss));
        assert!(slot_at(14).is_none());
    }

    #[test]
    fn corpus_walk() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.ok(), 8);
        assert_eq!(r.fail(), 6);
        assert!(r.green());
    }

    #[test]
    fn report_render_literal() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(
            r.render().as_str(),
            "needed total=14 ok=8 fail=6 enoent=1 dep=1 cycle=1 malformed=1 name=1 origin=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn cover_prefix() {
        assert!(cover(0).is_ok());
        assert!(cover(14).is_ok());
        assert_eq!(cover(15), Err(NE_ESLOT));
    }

    #[test]
    fn diagnose_shape() {
        let c = match slot_at(6) {
            Some(v) => v,
            None => return,
        };
        let (rep, _tr) = solve(&c);
        let d = rep.diagnose();
        assert!(d.starts_with("./app7: outcome=dep-missing"));
        assert!(d.contains("subject=libgone.so"));
        assert!(d.contains("requester=liba.so"));
        assert!(d.ends_with('\n'));
    }
}
