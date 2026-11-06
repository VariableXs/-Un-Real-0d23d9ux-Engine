//! UNX-F11637 SONAME 与文件名错位装载判据
//!
//! 判据原文：
//! UNX-F11637-J1 按路径装载不查 SONAME 判据绿，与基准机一致。
//!
//! 本模块把 `dlopen` 的“找对象”一层拆成可判定模型：请求字符串先分类成
//! 路径请求还是名字请求；路径请求直接落到文件系统，读到 `DT_SONAME` 之后
//! 才决定登记名；名字请求只查已登记表，不做任何文件系统搜索。
//!
//! 闸门次序：请求分类 → 路径表 → 打开 → 别名 → 名字表 → 就绪
//!
//! 1. 请求分类：含斜杠的是路径请求，其余是名字请求，二者永不互相回退。
//! 2. 路径表：同一路径已映射，只加引用并返回原句柄，不再读文件。
//! 3. 打开：文件缺失出 `ENOENT`，目标是目录出 `EISDIR`，两者都在读到
//!    SONAME 之前终止。
//! 4. 别名：文件带 SONAME 且该名字已登记，返回原句柄并加引用，不再登记
//!    第二个映射——“同一 SONAME 只活一份”是 B07 联签的核心。
//! 5. 名字表：名字请求命中已登记名（SONAME 优先，缺 SONAME 时用 basename）
//!    返回原句柄；未命中即 `ENOENT`，不回落到文件系统。
//! 6. 就绪：新映射以 SONAME 为登记名，无 SONAME 时以 basename 登记。
//!
//! 关键差异（本卡深度所在）：
//! - 按路径装载时，文件名只是寻找线索，登记名由 SONAME 决定；文件名与
//!   SONAME 不一致时，后续按文件名找不到、按 SONAME 找得到；
//! - 两条不同路径、同一个 SONAME，第二次装载命中的是别名表而不是路径表，
//!   阶段与注解都不同，但句柄与引用结果相同；
//! - 名字请求不做搜索路径回退，名字未命中的错误文案与路径缺失完全同文，
//!   只有阶段不同；
//! - 目录目标在读 SONAME 之前就被拒，错误码与“普通文件缺失”不同；
//! - 没有 SONAME 的文件用 basename 登记，且 `alias` 位为假，和“有 SONAME
//!   但恰好与 basename 同形”不能混为一谈。
//!
//! 本模块只做纯计算：文件系统是一张静态表，没有系统调用、没有 IO。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段基址。
pub const SEG_BASE: u32 = 0xfce0;

/// 请求分类错。
pub const E_SNREQ: u32 = SEG_BASE + 0x01;
/// 打开阶段错。
pub const E_SNOPEN: u32 = SEG_BASE + 0x02;
/// SONAME 读取错。
pub const E_SNREAD: u32 = SEG_BASE + 0x03;
/// 别名判定错。
pub const E_SNALIAS: u32 = SEG_BASE + 0x04;
/// 名字表错。
pub const E_SNLOOKUP: u32 = SEG_BASE + 0x05;
/// 映射登记错。
pub const E_SNMAP: u32 = SEG_BASE + 0x06;
/// 引用计数错。
pub const E_SNREF: u32 = SEG_BASE + 0x07;
/// 路径表错。
pub const E_SNPATH: u32 = SEG_BASE + 0x08;
/// 名字错。
pub const E_SNNAME: u32 = SEG_BASE + 0x09;
/// 目录档。
pub const E_SNDIR: u32 = SEG_BASE + 0x0a;
/// 缺失档。
pub const E_SNMISS: u32 = SEG_BASE + 0x0b;
/// 阶段错。
pub const E_SNSTAGE: u32 = SEG_BASE + 0x0c;
/// 顺序错。
pub const E_SNORDER: u32 = SEG_BASE + 0x0d;

/// 语料槽数。
pub const SAMPLES: usize = 14;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 前 8 槽为申报正确档。
pub const ACCEPT: usize = 8;

/// 后 6 槽为申报错误档。
pub const REJECT: usize = 6;

/// 档位行数：新装 / 命中 / 未命中 / 其他。
pub const TIER_COUNT: usize = 4;

/// 句柄表容量。
pub const CAP: usize = 6;

/// 文件系统表容量。
pub const FS_CAP: usize = 6;

/// errno：无此文件。
pub const ERR_ENOENT: i32 = 2;
/// errno：是目录。
pub const ERR_EISDIR: i32 = 21;

/// 操作码：请求。
pub const OP_REQ: u8 = 0x01;
/// 操作码：路径。
pub const OP_PATH: u8 = 0x02;
/// 操作码：别名。
pub const OP_ALIAS: u8 = 0x03;
/// 操作码：映射。
pub const OP_MAP: u8 = 0x04;
/// 操作码：名字。
pub const OP_NAME: u8 = 0x05;
/// 操作码：汇总。
pub const OP_CORPUS: u8 = 0x06;

/// FNV-1a 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut x = h ^ (b as u64);
    x = x.wrapping_mul(FNV_PRIME);
    x
}

/// 折入一个字（小端）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 折入一个名字。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            x = fold_byte(x, *v);
        }
        i += 1;
    }
    x
}

/// 十进制文本。
fn dec_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        return String::from("0");
    }
    while x > 0 && n < buf.len() {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
        n += 1;
    }
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
    s
}

/// 无符号文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本（负号单独处理）。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text((-(v as i64)) as u32).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 是否路径请求：含斜杠即真。
pub fn is_path(req: &str) -> bool {
    let b = req.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'/' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 末段：最后一个斜杠之后的部分；无斜杠时整串。
pub fn base_name(path: &str) -> &str {
    let b = path.as_bytes();
    let mut cut = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'/' {
                cut = i + 1;
            }
        }
        i += 1;
    }
    match path.get(cut..) {
        Some(v) => v,
        None => path,
    }
}

// ---------------------------------------------------------------------------
// 静态文件系统
// ---------------------------------------------------------------------------

/// 文件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 普通文件。
    File,
    /// 目录。
    Dir,
    /// 不存在。
    Missing,
}

impl Kind {
    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Dir => "dir",
            Kind::Missing => "missing",
        }
    }
}

/// 一个静态文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File {
    /// 路径。
    pub path: &'static str,
    /// SONAME（空串表示没有）。
    pub soname: &'static str,
    /// 是否目录。
    pub dir: bool,
}

impl File {
    /// 普通文件。
    pub const fn plain(path: &'static str, soname: &'static str) -> File {
        File { path, soname, dir: false }
    }

    /// 目录。
    pub const fn directory(path: &'static str) -> File {
        File { path, soname: "", dir: true }
    }

    /// 种类。
    pub fn kind(&self) -> Kind {
        if self.dir {
            Kind::Dir
        } else {
            Kind::File
        }
    }
}

/// 静态文件系统。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fs {
    files: [File; FS_CAP],
    len: usize,
}

impl Fs {
    /// 空表。
    pub const fn new() -> Fs {
        Fs {
            files: [File { path: "", soname: "", dir: false }; FS_CAP],
            len: 0,
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取条目。
    pub fn get(&self, i: usize) -> Option<&File> {
        if i >= self.len {
            return None;
        }
        self.files.get(i)
    }

    /// 加一个文件。
    pub fn add(&mut self, f: File) -> Option<usize> {
        if self.len >= FS_CAP {
            return None;
        }
        if let Some(slot) = self.files.get_mut(self.len) {
            *slot = f;
        }
        let i = self.len;
        self.len += 1;
        Some(i)
    }

    /// 按路径查。
    pub fn find(&self, path: &str) -> Option<&File> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(f) = self.files.get(i) {
                if f.path == path {
                    return Some(f);
                }
            }
            i += 1;
        }
        None
    }

    /// 种类。
    pub fn kind(&self, path: &str) -> Kind {
        match self.find(path) {
            Some(f) => f.kind(),
            None => Kind::Missing,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(f) = self.files.get(i) {
                h = fold_str(h, f.path);
                h = fold_str(h, f.soname);
                h = fold_word(h, f.dir as u64);
            }
            i += 1;
        }
        fold_word(h, self.len as u64)
    }
}

/// 预置文件系统：三条可装载文件 + 一个目录。
pub fn fs() -> Fs {
    let mut f = Fs::new();
    let _ = f.add(File::plain("./libz.so", "libz.so.1"));
    let _ = f.add(File::plain("./copy.so", "libz.so.1"));
    let _ = f.add(File::plain("./plain.so", ""));
    let _ = f.add(File::plain("./libm.so", "libm.so.6"));
    let _ = f.add(File::directory("./dir"));
    f
}

// ---------------------------------------------------------------------------
// 句柄表
// ---------------------------------------------------------------------------

/// 一个已登记对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 首次装载路径。
    pub path: &'static str,
    /// SONAME（空串表示没有）。
    pub soname: &'static str,
    /// 登记名（SONAME 优先，否则 basename）。
    pub name: &'static str,
    /// 引用计数。
    pub refs: u32,
    /// 是否映射中。
    pub mapped: bool,
}

impl Entry {
    /// 空白条目。
    pub const fn blank() -> Entry {
        Entry { path: "", soname: "", name: "", refs: 0, mapped: false }
    }

    /// 由文件路径与 SONAME 造条目。
    pub fn of(path: &'static str, soname: &'static str) -> Entry {
        let name = if soname.is_empty() { base_name_static(path) } else { soname };
        Entry { path, soname, name, refs: 1, mapped: true }
    }
}

/// 静态串的 basename：只取末段。
fn base_name_static(path: &'static str) -> &'static str {
    let b = path.as_bytes();
    let mut cut = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'/' {
                cut = i + 1;
            }
        }
        i += 1;
    }
    match path.get(cut..) {
        Some(v) => v,
        None => path,
    }
}

/// 已登记对象表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    entries: [Entry; CAP],
    len: usize,
}

impl Image {
    /// 空表。
    pub const fn new() -> Image {
        Image { entries: [Entry::blank(); CAP], len: 0 }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取条目。
    pub fn get(&self, i: usize) -> Option<&Entry> {
        if i >= self.len {
            return None;
        }
        self.entries.get(i)
    }

    /// 取可变条目。
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Entry> {
        if i >= self.len {
            return None;
        }
        self.entries.get_mut(i)
    }

    /// 按路径查。
    pub fn find_path(&self, path: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                if e.path == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按登记名查。
    pub fn find_name(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                if e.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一个对象。
    pub fn push(&mut self, e: Entry) -> Option<usize> {
        if self.len >= CAP {
            return None;
        }
        let i = self.len;
        if let Some(slot) = self.entries.get_mut(i) {
            *slot = e;
        }
        self.len += 1;
        Some(i)
    }

    /// 加引用。
    pub fn hold(&mut self, i: usize) -> Option<u32> {
        match self.get_mut(i) {
            Some(e) => {
                e.refs = e.refs.saturating_add(1);
                Some(e.refs)
            }
            None => None,
        }
    }

    /// 引用数。
    pub fn refs_at(&self, i: usize) -> u32 {
        match self.get(i) {
            Some(e) => e.refs,
            None => 0,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                h = fold_str(h, e.path);
                h = fold_str(h, e.soname);
                h = fold_str(h, e.name);
                h = fold_word(h, e.refs as u64);
                h = fold_word(h, e.mapped as u64);
            }
            i += 1;
        }
        fold_word(h, self.len as u64)
    }
}

// ---------------------------------------------------------------------------
// 阶段与注解
// ---------------------------------------------------------------------------

/// 装载阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 请求分类。
    Request,
    /// 路径表。
    Path,
    /// 打开。
    Open,
    /// 别名。
    Alias,
    /// 名字表。
    Lookup,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(self) -> u8 {
        match self {
            Stage::Request => 0,
            Stage::Path => 1,
            Stage::Open => 2,
            Stage::Alias => 3,
            Stage::Lookup => 4,
            Stage::Ready => 5,
        }
    }

    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Request => "request",
            Stage::Path => "path",
            Stage::Open => "open",
            Stage::Alias => "alias",
            Stage::Lookup => "lookup",
            Stage::Ready => "ready",
        }
    }
}

/// 装载注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 新映射，登记 SONAME。
    NewMap,
    /// 新映射，无 SONAME，登记 basename。
    NoSoname,
    /// 同一路径再次装载。
    PathHit,
    /// 不同路径、同一 SONAME。
    SonameAlias,
    /// 名字请求命中。
    NameHit,
    /// 名字请求未命中。
    NameMiss,
    /// 文件缺失。
    Missing,
    /// 目标是目录。
    Dir,
}

impl Note {
    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Note::NewMap => "new-map",
            Note::NoSoname => "no-soname",
            Note::PathHit => "path-hit",
            Note::SonameAlias => "soname-alias",
            Note::NameHit => "name-hit",
            Note::NameMiss => "name-miss",
            Note::Missing => "missing",
            Note::Dir => "dir",
        }
    }
}

/// 装载结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 阶段。
    pub stage: Stage,
    /// errno（零为成功）。
    pub code: i32,
    /// 句柄（一基）。
    pub handle: u32,
    /// 条目引用数。
    pub refs: u32,
    /// 是否带 SONAME。
    pub alias: bool,
    /// 登记名。
    pub name: &'static str,
    /// 注解。
    pub note: Note,
}

impl Outcome {
    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Request,
            code: 0,
            handle: 0,
            refs: 0,
            alias: false,
            name: "",
            note: Note::Missing,
        }
    }
}

/// 名字请求：只查登记表。
fn open_name(img: &mut Image, req: &'static str) -> Outcome {
    match img.find_name(req) {
        Some(i) => {
            let refs = img.hold(i).unwrap_or(0);
            let alias = match img.get(i) {
                Some(e) => !e.soname.is_empty(),
                None => false,
            };
            let name = match img.get(i) {
                Some(e) => e.name,
                None => "",
            };
            Outcome {
                stage: Stage::Lookup,
                code: 0,
                handle: (i + 1) as u32,
                refs,
                alias,
                name,
                note: Note::NameHit,
            }
        }
        None => Outcome {
            stage: Stage::Lookup,
            code: ERR_ENOENT,
            handle: 0,
            refs: 0,
            alias: false,
            name: "",
            note: Note::NameMiss,
        },
    }
}

/// 路径请求：路径表 → 打开 → 别名 → 登记。
fn open_path(img: &mut Image, table: &Fs, req: &'static str) -> Outcome {
    if let Some(i) = img.find_path(req) {
        let refs = img.hold(i).unwrap_or(0);
        let alias = match img.get(i) {
            Some(e) => !e.soname.is_empty(),
            None => false,
        };
        let name = match img.get(i) {
            Some(e) => e.name,
            None => "",
        };
        return Outcome {
            stage: Stage::Path,
            code: 0,
            handle: (i + 1) as u32,
            refs,
            alias,
            name,
            note: Note::PathHit,
        };
    }
    let file = match table.find(req) {
        Some(f) => *f,
        None => {
            return Outcome {
                stage: Stage::Open,
                code: ERR_ENOENT,
                handle: 0,
                refs: 0,
                alias: false,
                name: "",
                note: Note::Missing,
            }
        }
    };
    if file.dir {
        return Outcome {
            stage: Stage::Open,
            code: ERR_EISDIR,
            handle: 0,
            refs: 0,
            alias: false,
            name: "",
            note: Note::Dir,
        };
    }
    if !file.soname.is_empty() {
        if let Some(i) = img.find_name(file.soname) {
            let refs = img.hold(i).unwrap_or(0);
            let name = match img.get(i) {
                Some(e) => e.name,
                None => file.soname,
            };
            return Outcome {
                stage: Stage::Alias,
                code: 0,
                handle: (i + 1) as u32,
                refs,
                alias: true,
                name,
                note: Note::SonameAlias,
            };
        }
    }
    let entry = Entry::of(req, file.soname);
    let alias = !entry.soname.is_empty();
    let name = entry.name;
    match img.push(entry) {
        Some(i) => Outcome {
            stage: Stage::Ready,
            code: 0,
            handle: (i + 1) as u32,
            refs: 1,
            alias,
            name,
            note: if alias { Note::NewMap } else { Note::NoSoname },
        },
        None => Outcome {
            stage: Stage::Ready,
            code: ERR_ENOENT,
            handle: 0,
            refs: 0,
            alias: false,
            name: "",
            note: Note::Missing,
        },
    }
}

/// 装载入口：先分类，再走各自通道。
pub fn dlopen(img: &mut Image, table: &Fs, req: &'static str) -> Outcome {
    if is_path(req) {
        open_path(img, table, req)
    } else {
        open_name(img, req)
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 错误文案：缺失与名字未命中完全同文，只有阶段不同。
pub fn message(o: &Outcome, req: &str) -> String {
    if o.ok() {
        return String::new();
    }
    let mut s = String::from(req);
    s.push_str(": ");
    match o.note {
        Note::Dir => s.push_str("cannot read file data: Is a directory"),
        Note::Missing | Note::NameMiss => {
            s.push_str("cannot open shared object file: No such file or directory")
        }
        _ => s.push_str("cannot open shared object file"),
    }
    s.push('\n');
    s
}

/// 成功路径释义。
pub fn explain(o: &Outcome) -> String {
    let mut s = String::new();
    match o.note {
        Note::NewMap => {
            s.push_str("mapped, registered as ");
            s.push_str(o.name);
        }
        Note::NoSoname => {
            s.push_str("mapped, no soname, registered as ");
            s.push_str(o.name);
        }
        Note::PathHit => {
            s.push_str("same path, handle ");
            s.push_str(u32_text(o.handle).as_str());
        }
        Note::SonameAlias => {
            s.push_str("soname alias of handle ");
            s.push_str(u32_text(o.handle).as_str());
        }
        Note::NameHit => {
            s.push_str("name hit, handle ");
            s.push_str(u32_text(o.handle).as_str());
        }
        Note::NameMiss => s.push_str("name not found"),
        Note::Missing => s.push_str("path not found"),
        Note::Dir => s.push_str("path is a directory"),
    }
    s
}

// ---------------------------------------------------------------------------
// 档位与缺陷
// ---------------------------------------------------------------------------

/// 四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 新装档。
    New,
    /// 命中档。
    Hit,
    /// 未命中档。
    Miss,
    /// 其他档（目录）。
    Other,
}

impl Tier {
    /// 行下标。
    pub fn index(self) -> usize {
        match self {
            Tier::New => 0,
            Tier::Hit => 1,
            Tier::Miss => 2,
            Tier::Other => 3,
        }
    }

    /// 由注解归行。
    pub fn of(n: Note) -> Tier {
        match n {
            Note::NewMap | Note::NoSoname => Tier::New,
            Note::PathHit | Note::SonameAlias | Note::NameHit => Tier::Hit,
            Note::NameMiss | Note::Missing => Tier::Miss,
            Note::Dir => Tier::Other,
        }
    }

    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Tier::New => "new",
            Tier::Hit => "hit",
            Tier::Miss => "miss",
            Tier::Other => "other",
        }
    }
}

/// 缺陷位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 无缺陷。
    Ok,
    /// 成败不符。
    Estate,
    /// 阶段不符。
    Stage,
    /// errno 不符。
    Code,
    /// 句柄不符。
    Handle,
    /// 引用不符。
    Refs,
    /// SONAME 位不符。
    Alias,
    /// 注解不符。
    Note,
}

impl Fault {
    /// 缺陷码。
    pub fn code(self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => E_SNREQ,
            Fault::Stage => E_SNSTAGE,
            Fault::Code => E_SNREAD,
            Fault::Handle => E_SNMAP,
            Fault::Refs => E_SNREF,
            Fault::Alias => E_SNALIAS,
            Fault::Note => E_SNNAME,
        }
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Estate => "estate",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Handle => "handle",
        Fault::Refs => "refs",
        Fault::Alias => "alias",
        Fault::Note => "note",
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 阶段。
    pub stage: Stage,
    /// errno。
    pub code: i32,
    /// 句柄。
    pub handle: u32,
    /// 引用数。
    pub refs: u32,
    /// SONAME 位。
    pub alias: bool,
    /// 注解。
    pub note: Note,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Request, code: 0, handle: 0, refs: 0, alias: false, note: Note::Missing }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.code == 0
    }
}

/// 预置表构造器。
pub type Setup = fn() -> Image;

/// 一个语料形状。
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// 槽标签。
    pub tag: &'static str,
    /// 请求串。
    pub req: &'static str,
    /// 预置句柄表。
    pub setup: Setup,
}

/// 形状等值：只比标签与请求串，函数指针不参与比较。
impl PartialEq for Shape {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag && self.req == other.req
    }
}

impl Eq for Shape {}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape { tag: "", req: "", setup: setup_empty }
    }

    /// 实际结果（每次新表、新文件系统）。
    pub fn actual(&self) -> Outcome {
        let mut img = (self.setup)();
        let table = fs();
        dlopen(&mut img, &table, self.req)
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

    /// 请求串。
    pub fn req(&self) -> &'static str {
        self.shape.req
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
// 预置表
// ---------------------------------------------------------------------------

/// 空表。
fn setup_empty() -> Image {
    Image::new()
}

/// 已装入 `./libz.so`（SONAME `libz.so.1`）。
fn setup_libz() -> Image {
    let mut im = Image::new();
    let _ = im.push(Entry::of("./libz.so", "libz.so.1"));
    im
}

/// 已装入 `./libz.so` 且引用为二。
fn setup_libz2() -> Image {
    let mut im = Image::new();
    let _ = im.push(Entry { refs: 2, ..Entry::of("./libz.so", "libz.so.1") });
    im
}

/// 8 个预置构造器总表。
pub const SETUPS: [Setup; 3] = [setup_empty, setup_libz, setup_libz2];

/// 14 个形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape { tag: "path-new", req: "./libz.so", setup: setup_empty },
    Shape { tag: "path-repeat", req: "./libz.so", setup: setup_libz },
    Shape { tag: "soname-alias", req: "./copy.so", setup: setup_libz },
    Shape { tag: "name-hit", req: "libz.so.1", setup: setup_libz },
    Shape { tag: "name-miss", req: "nope.so", setup: setup_empty },
    Shape { tag: "path-no-soname", req: "./plain.so", setup: setup_empty },
    Shape { tag: "path-missing", req: "./gone.so", setup: setup_empty },
    Shape { tag: "path-dir", req: "./dir", setup: setup_empty },
    Shape { tag: "path-as-name", req: "./libz.so", setup: setup_empty },
    Shape { tag: "repeat-as-new", req: "./libz.so", setup: setup_libz },
    Shape { tag: "alias-as-newmap", req: "./copy.so", setup: setup_libz },
    Shape { tag: "name-hit-as-miss", req: "libz.so.1", setup: setup_libz },
    Shape { tag: "nosoname-as-soname", req: "./plain.so", setup: setup_empty },
    Shape { tag: "dir-as-enoent", req: "./dir", setup: setup_empty },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0 按路径新装：登记名由 SONAME 决定。
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 1, alias: true, note: Note::NewMap },
    // 1 同一路径再装：路径表命中，只加引用。
    Claim { stage: Stage::Path, code: 0, handle: 1, refs: 2, alias: true, note: Note::PathHit },
    // 2 不同路径同 SONAME：别名表命中。
    Claim { stage: Stage::Alias, code: 0, handle: 1, refs: 2, alias: true, note: Note::SonameAlias },
    // 3 名字请求命中登记名。
    Claim { stage: Stage::Lookup, code: 0, handle: 1, refs: 2, alias: true, note: Note::NameHit },
    // 4 名字请求未命中：不回退文件系统。
    Claim { stage: Stage::Lookup, code: ERR_ENOENT, handle: 0, refs: 0, alias: false, note: Note::NameMiss },
    // 5 无 SONAME：按 basename 登记。
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 1, alias: false, note: Note::NoSoname },
    // 6 路径缺失。
    Claim { stage: Stage::Open, code: ERR_ENOENT, handle: 0, refs: 0, alias: false, note: Note::Missing },
    // 7 路径是目录：读 SONAME 之前拒收。
    Claim { stage: Stage::Open, code: ERR_EISDIR, handle: 0, refs: 0, alias: false, note: Note::Dir },
    // 8 错判：把路径请求当名字请求。
    Claim { stage: Stage::Lookup, code: ERR_ENOENT, handle: 0, refs: 0, alias: false, note: Note::NameMiss },
    // 9 错判：同一路径再装被当成新映射。
    Claim { stage: Stage::Path, code: 0, handle: 2, refs: 1, alias: true, note: Note::PathHit },
    // 10 错判：别名命中被当成新映射。
    Claim { stage: Stage::Alias, code: 0, handle: 2, refs: 1, alias: true, note: Note::SonameAlias },
    // 11 错判：名字命中被当成未命中。
    Claim { stage: Stage::Lookup, code: ERR_ENOENT, handle: 0, refs: 0, alias: false, note: Note::NameMiss },
    // 12 错判：无 SONAME 被当成有 SONAME。
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 1, alias: true, note: Note::NoSoname },
    // 13 错判：目录被当成缺失。
    Claim { stage: Stage::Open, code: ERR_ENOENT, handle: 0, refs: 0, alias: false, note: Note::Dir },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Handle, Fault::Handle, Fault::Estate, Fault::Alias, Fault::Code];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] =
    [E_SNREQ, E_SNMAP, E_SNMAP, E_SNREQ, E_SNALIAS, E_SNREAD];

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
        Err(E_SNREQ)
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

/// 对判：成败 → 阶段 → errno → 句柄 → 引用 → SONAME 位 → 注解。
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
    if claim.handle != a.handle {
        return Fault::Handle;
    }
    if claim.refs != a.refs {
        return Fault::Refs;
    }
    if claim.alias != a.alias {
        return Fault::Alias;
    }
    if claim.note != a.note {
        return Fault::Note;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 释义
// ---------------------------------------------------------------------------

/// 单行释义：
/// `<req> stage=<s> code=<c> handle=<h> refs=<n> name=<name> note=<n>`。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(c.shape.req);
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(a.code).as_str());
    s.push_str(" handle=");
    s.push_str(u32_text(a.handle).as_str());
    s.push_str(" refs=");
    s.push_str(u32_text(a.refs).as_str());
    s.push_str(" name=");
    s.push_str(a.name);
    s.push_str(" note=");
    s.push_str(a.note.name());
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
    /// 行（新装 / 命中 / 未命中 / 其他）。
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
                let t = Tier::of(c.actual().note).index();
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
    /// 新装档正确数。
    pub new: u32,
    /// 命中档正确数。
    pub hit: u32,
    /// 未命中档正确数。
    pub miss: u32,
    /// 其他档正确数。
    pub other: u32,
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
            Err(E_SNREQ)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("soname corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" new=");
        s.push_str(u32_text(self.new).as_str());
        s.push_str(" hit=");
        s.push_str(u32_text(self.hit).as_str());
        s.push_str(" miss=");
        s.push_str(u32_text(self.miss).as_str());
        s.push_str(" other=");
        s.push_str(u32_text(self.other).as_str());
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
            return Err(E_SNREQ);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut new = 0u32;
    let mut hit = 0u32;
    let mut miss = 0u32;
    let mut other = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(E_SNREQ),
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
            None => return Err(E_SNREQ),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(a.note) {
                Tier::New => new += 1,
                Tier::Hit => hit += 1,
                Tier::Miss => miss += 1,
                Tier::Other => other += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, new, hit, miss, other, matched })
}

// ---------------------------------------------------------------------------
// 闸门
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
            return Err(E_SNSTAGE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(E_SNORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(E_SNSTAGE);
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
            return Err(E_SNREQ);
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
            return Err(E_SNSTAGE);
        }
        const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 3, 2, 1, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(E_SNSTAGE),
            };
            if s.slot as usize != i {
                return Err(E_SNREQ);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(E_SNREQ);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(E_SNREQ);
                        }
                    }
                    None => return Err(E_SNSTAGE),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(E_SNREQ);
                        }
                    }
                    None => return Err(E_SNSTAGE),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(E_SNSTAGE);
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
            return Err(E_SNSTAGE);
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
            Err(E_SNSTAGE)
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
    let _ = l.register(count_cell(3, OP_MAP, r.map(|x| x.new)));
    let _ = l.register(count_cell(4, OP_PATH, r.map(|x| x.hit)));
    let _ = l.register(count_cell(5, OP_NAME, r.map(|x| x.miss)));
    let _ = l.register(count_cell(6, OP_REQ, r.map(|x| x.other)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_ALIAS, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_REQ,
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

/// 四路注入：请求、路径、别名、账。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && req_ok() && fs_ok();
    out[1] = path_ok() && alias_ok() && lookup_ok();
    out[2] = judge_ok() && matrix_ok() && corpus_ok() && reds_ok();
    out[3] = slots_ok()
        && message_ok()
        && detail_ok()
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

/// 第 i 槽的期望阶段。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Ready,
    Stage::Path,
    Stage::Alias,
    Stage::Lookup,
    Stage::Lookup,
    Stage::Ready,
    Stage::Open,
    Stage::Open,
    Stage::Ready,
    Stage::Path,
    Stage::Alias,
    Stage::Lookup,
    Stage::Ready,
    Stage::Open,
];

/// 第 i 槽的期望 errno。
const WANT_CODE: [i32; CORPUS] = [
    0, 0, 0, 0, ERR_ENOENT, 0, ERR_ENOENT, ERR_EISDIR, 0, 0, 0, 0, 0, ERR_EISDIR,
];

/// 第 i 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::NewMap,
    Note::PathHit,
    Note::SonameAlias,
    Note::NameHit,
    Note::NameMiss,
    Note::NoSoname,
    Note::Missing,
    Note::Dir,
    Note::NewMap,
    Note::PathHit,
    Note::SonameAlias,
    Note::NameHit,
    Note::NoSoname,
    Note::Dir,
];

/// 名称与段基址。
fn name_ok() -> bool {
    let codes = [
        E_SNREQ, E_SNOPEN, E_SNREAD, E_SNALIAS, E_SNLOOKUP, E_SNMAP, E_SNREF, E_SNPATH, E_SNNAME,
        E_SNDIR, E_SNMISS, E_SNSTAGE, E_SNORDER,
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
    let a = SEG_BASE == 0xfce0;
    let b = E_SNREQ == SEG_BASE + 0x01 && E_SNORDER == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = CAP == 6 && FS_CAP == 6 && TIER_COUNT == 4;
    let e = ERR_ENOENT == 2 && ERR_EISDIR == 21;
    a && b && c && d && e
}

/// 请求分类与末段。
fn req_ok() -> bool {
    let a = is_path("./libz.so") && is_path("/usr/lib/libz.so") && is_path("a/b");
    let b = !is_path("libz.so.1") && !is_path("nope.so") && !is_path("");
    let c = base_name("./libz.so") == "libz.so";
    let d = base_name("/usr/lib/libz.so.1") == "libz.so.1";
    let e = base_name("plain.so") == "plain.so";
    let f = base_name("./dir/") == "";
    a && b && c && d && e && f
}

/// 静态文件系统。
fn fs_ok() -> bool {
    let f = fs();
    let a = f.len() == 5 && !f.is_empty();
    let b = f.kind("./libz.so") == Kind::File && f.kind("./dir") == Kind::Dir;
    let c = f.kind("./gone.so") == Kind::Missing;
    let d = match f.find("./plain.so") {
        Some(x) => x.soname.is_empty(),
        None => false,
    };
    let e = match f.find("./copy.so") {
        Some(x) => x.soname == "libz.so.1",
        None => false,
    };
    let g = f.seal() != 0 && f.get(5).is_none();
    a && b && c && d && e && g
}

/// 路径表语义。
fn path_ok() -> bool {
    let mut im = Image::new();
    let t = fs();
    let a = dlopen(&mut im, &t, "./libz.so");
    let ok1 = a.stage == Stage::Ready
        && a.ok()
        && a.handle == 1
        && a.refs == 1
        && a.alias
        && a.name == "libz.so.1"
        && a.note == Note::NewMap;
    let b = dlopen(&mut im, &t, "./libz.so");
    let ok2 = b.stage == Stage::Path
        && b.handle == 1
        && b.refs == 2
        && b.note == Note::PathHit;
    let c = dlopen(&mut im, &t, "./plain.so");
    let ok3 = c.ok() && !c.alias && c.name == "plain.so" && c.note == Note::NoSoname;
    let d = dlopen(&mut im, &t, "./gone.so");
    let ok4 = d.stage == Stage::Open && d.code == ERR_ENOENT && d.note == Note::Missing;
    let e = dlopen(&mut im, &t, "./dir");
    let ok5 = e.stage == Stage::Open && e.code == ERR_EISDIR && e.note == Note::Dir;
    ok1 && ok2 && ok3 && ok4 && ok5
}

/// 别名表语义。
fn alias_ok() -> bool {
    let mut im = setup_libz();
    let t = fs();
    let a = dlopen(&mut im, &t, "./copy.so");
    let ok1 = a.stage == Stage::Alias
        && a.ok()
        && a.handle == 1
        && a.refs == 2
        && a.alias
        && a.note == Note::SonameAlias;
    let ok2 = im.len() == 1 && im.refs_at(0) == 2;
    let b = dlopen(&mut im, &t, "./libz.so");
    let ok3 = b.stage == Stage::Path && b.refs == 3;
    let c = dlopen(&mut im, &t, "./libm.so");
    let ok4 = c.note == Note::NewMap && c.handle == 2 && c.name == "libm.so.6";
    let d = dlopen(&mut im, &t, "./copy.so");
    let ok5 = d.stage == Stage::Alias && d.refs == 4;
    ok1 && ok2 && ok3 && ok4 && ok5
}

/// 名字表语义。
fn lookup_ok() -> bool {
    let mut im = setup_libz();
    let t = fs();
    let a = dlopen(&mut im, &t, "libz.so.1");
    let ok1 = a.stage == Stage::Lookup
        && a.ok()
        && a.handle == 1
        && a.refs == 2
        && a.alias
        && a.note == Note::NameHit;
    let b = dlopen(&mut im, &t, "libz.so");
    let ok2 = !b.ok() && b.stage == Stage::Lookup && b.code == ERR_ENOENT && b.note == Note::NameMiss;
    let c = dlopen(&mut im, &t, "plain.so");
    let ok3 = !c.ok() && c.note == Note::NameMiss;
    let mut im2 = Image::new();
    let d = dlopen(&mut im2, &t, "./plain.so");
    let ok4 = d.ok() && d.name == "plain.so";
    let e = dlopen(&mut im2, &t, "plain.so");
    let ok5 = e.ok() && e.stage == Stage::Lookup && e.refs == 2;
    ok1 && ok2 && ok3 && ok4 && ok5
}

/// 申报对判。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        i += 1;
    }
    let mut b = Case::blank();
    b.shape = Shape { tag: "x", req: "./libz.so", setup: setup_empty };
    b.claim = CLAIMS[0];
    let c = b.judge() == Fault::Ok;
    b.claim.handle = 2;
    let d = b.judge() == Fault::Handle;
    c && d
}

/// 矩阵统计。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    let a = m.total() == CORPUS && m.good() == ACCEPT && m.balanced();
    let b = m.row(0) == Some(Row { total: 4, good: 2 });
    let c = m.row(1) == Some(Row { total: 6, good: 3 });
    let d = m.row(2) == Some(Row { total: 2, good: 2 });
    let e = m.row(3) == Some(Row { total: 2, good: 1 });
    let f = m.perfect_rows() == 1;
    let g = m.row(9).is_none() && m.seal() != 0;
    a && b && c && d && e && f && g
}

/// 语料与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let want =
        "soname corpus=14 accept=8 reject=6 new=2 hit=3 miss=2 other=1 match=14 rate=1000ppm\n";
    r.total == 14
        && r.accept == 8
        && r.reject == 6
        && r.new == 2
        && r.hit == 3
        && r.miss == 2
        && r.other == 1
        && r.matched == 14
        && r.rate() == 1000
        && r.green()
        && r.gate() == Ok(())
        && r.render().as_str() == want
        && cover(CORPUS) == Ok(())
        && cover(13) == Err(E_SNREQ)
}

/// 红档显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929 && !r.green() && r.gate() == Err(E_SNREQ);
    let b = r.accept == 7 && r.reject == 7;
    let c = run_with(Some((14, Fault::Code))).is_err();
    let d = run_with(Some((8, Fault::Note))).map(|x| x.matched) == Ok(13);
    a && b && c && d
}

/// 文案逐字。
fn message_ok() -> bool {
    let mut im = Image::new();
    let t = fs();
    let a = dlopen(&mut im, &t, "nope.so");
    let ok1 = message(&a, "nope.so").as_str()
        == "nope.so: cannot open shared object file: No such file or directory\n";
    let b = dlopen(&mut im, &t, "./gone.so");
    let ok2 = message(&b, "./gone.so").as_str()
        == "./gone.so: cannot open shared object file: No such file or directory\n";
    let c = dlopen(&mut im, &t, "./dir");
    let ok3 = message(&c, "./dir").as_str() == "./dir: cannot read file data: Is a directory\n";
    let d = dlopen(&mut im, &t, "./libz.so");
    let ok4 = message(&d, "./libz.so").is_empty() && explain(&d) == "mapped, registered as libz.so.1";
    let e = dlopen(&mut im, &t, "libz.so.1");
    let ok5 = explain(&e) == "name hit, handle 1";
    let f = dlopen(&mut im, &t, "zz.so");
    let ok6 = explain(&f) == "name not found";
    ok1 && ok2 && ok3 && ok4 && ok5 && ok6
}

/// 槽号与档位。
fn slots_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if a.stage != WANT_STAGE[i] || a.code != WANT_CODE[i] || a.note != WANT_NOTE[i] {
            return false;
        }
        i += 1;
    }
    let tags = [
        "path-new",
        "path-repeat",
        "soname-alias",
        "name-hit",
        "name-miss",
        "path-no-soname",
        "path-missing",
        "path-dir",
        "path-as-name",
        "repeat-as-new",
        "alias-as-newmap",
        "name-hit-as-miss",
        "nosoname-as-soname",
        "dir-as-enoent",
    ];
    let mut k = 0usize;
    while k < tags.len() {
        let mut m = k + 1;
        while m < tags.len() {
            if tags[k] == tags[m] {
                return false;
            }
            m += 1;
        }
        if let Some(c) = cs.get(k) {
            if c.tag() != tags[k] {
                return false;
            }
        }
        k += 1;
    }
    let mut t = 0usize;
    while t < REJECT {
        if let Some(c) = cs.get(ACCEPT + t) {
            if judge(c) != expected(ACCEPT + t).unwrap_or(Fault::Note) {
                return false;
            }
        }
        t += 1;
    }
    case_at(0).is_some()
        && case_at(CORPUS).is_none()
        && case_at(9).map(|c| c.tag()) == Some("repeat-as-new")
}

/// 新装释义。
const DETAIL_NEW: &str = "./libz.so stage=ready code=0 handle=1 refs=1 name=libz.so.1 note=new-map\n";

/// 别名释义。
const DETAIL_ALIAS: &str =
    "./copy.so stage=alias code=0 handle=1 refs=2 name=libz.so.1 note=soname-alias\n";

/// 目录释义。
const DETAIL_DIR: &str = "./dir stage=open code=21 handle=0 refs=0 name= note=dir\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_NEW,
        None => false,
    };
    let b = match cs.get(2) {
        Some(c) => detail(c).as_str() == DETAIL_ALIAS,
        None => false,
    };
    let d = match cs.get(7) {
        Some(c) => detail(c).as_str() == DETAIL_DIR,
        None => false,
    };
    let e = match cs.get(3) {
        Some(c) => {
            let o = c.actual();
            o.handle == 1 && o.refs == 2 && o.stage == Stage::Lookup
        }
        None => false,
    };
    a && b && d && e
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Path) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Request) == Err(E_SNORDER);
    let d = g.step(Stage::Open) == Ok(()) && g.last() == 2;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(E_SNSTAGE);
    a && b && c && d && e && f
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
pub fn run_unx11637_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11637_soname_path");

    s.add("SN01-名称与段基址", name_ok(), "static");

    s.add("SN02-请求分类与末段", req_ok(), "static");

    s.add("SN03-静态文件系统", fs_ok(), "static");

    s.add("SN04-路径表语义", path_ok(), "static");

    s.add("SN05-别名表语义", alias_ok(), "static");

    s.add("SN06-名字表语义", lookup_ok(), "static");

    s.add("SN07-申报对判", judge_ok(), "static");

    s.add("SN08-矩阵统计", matrix_ok(), "static");

    s.add("SN09-语料与渲染", corpus_ok(), "static");

    s.add("SN10-红档显式", reds_ok(), "static");

    s.add("SN11-文案逐字", message_ok(), "static");

    s.add("SN12-释义与账本", detail_ok() && ledger_ok() && gate_ok(), "static");

    s.add(
        "SN13-冻结重放四注入",
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

    #[test]
    fn seg_and_codes() {
        assert_eq!(SEG_BASE, 0xfce0);
        assert_eq!(E_SNREQ, SEG_BASE + 0x01);
        assert_eq!(E_SNORDER, SEG_BASE + 0x0d);
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(CAP, 6);
        assert_eq!(FS_CAP, 6);
        assert!(name_ok());
    }

    #[test]
    fn fnv_is_stable() {
        let a = fold_str(FNV_OFF, "libz.so.1");
        let b = fold_str(FNV_OFF, "libz.so.1");
        let c = fold_str(FNV_OFF, "libz.so.2");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 3));
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(i32_text(-1).as_str(), "-1");
        assert_eq!(i32_text(21).as_str(), "21");
        assert_eq!(i32_text(0).as_str(), "0");
    }

    #[test]
    fn request_classification() {
        assert!(is_path("./a.so"));
        assert!(is_path("/a"));
        assert!(!is_path("a.so"));
        assert!(!is_path(""));
        assert_eq!(base_name("./a.so"), "a.so");
        assert_eq!(base_name("a.so"), "a.so");
        assert_eq!(base_name("/x/y/z"), "z");
    }

    #[test]
    fn fs_seed_kinds() {
        let f = fs();
        assert_eq!(f.len(), 5);
        assert_eq!(f.kind("./libz.so"), Kind::File);
        assert_eq!(f.kind("./dir"), Kind::Dir);
        assert_eq!(f.kind("./none"), Kind::Missing);
        assert_eq!(f.get(5), None);
        assert_ne!(f.seal(), 0);
    }

    #[test]
    fn entry_registration_name() {
        let a = Entry::of("./libz.so", "libz.so.1");
        assert_eq!(a.name, "libz.so.1");
        assert_eq!(a.refs, 1);
        assert!(a.mapped);
        let b = Entry::of("./plain.so", "");
        assert_eq!(b.name, "plain.so");
        let c = Entry::blank();
        assert_eq!(c.refs, 0);
        assert!(!c.mapped);
    }

    #[test]
    fn image_push_and_find() {
        let mut im = Image::new();
        assert!(im.is_empty());
        assert_eq!(im.push(Entry::of("./libz.so", "libz.so.1")), Some(0));
        assert_eq!(im.find_path("./libz.so"), Some(0));
        assert_eq!(im.find_path("./other.so"), None);
        assert_eq!(im.find_name("libz.so.1"), Some(0));
        assert_eq!(im.find_name("libz.so"), None);
        assert_eq!(im.hold(0), Some(2));
        assert_eq!(im.refs_at(0), 2);
        assert_eq!(im.get(1), None);
    }

    #[test]
    fn image_capacity() {
        let mut im = Image::new();
        let mut i = 0usize;
        while i < CAP {
            assert!(im.push(Entry::of("./x.so", "x.so")).is_some());
            i += 1;
        }
        assert_eq!(im.push(Entry::of("./y.so", "y.so")), None);
        assert_eq!(im.len(), CAP);
    }

    #[test]
    fn new_map_by_path() {
        let mut im = Image::new();
        let t = fs();
        let o = dlopen(&mut im, &t, "./libz.so");
        assert_eq!(o.stage, Stage::Ready);
        assert!(o.ok());
        assert_eq!(o.handle, 1);
        assert_eq!(o.refs, 1);
        assert!(o.alias);
        assert_eq!(o.name, "libz.so.1");
        assert_eq!(o.note, Note::NewMap);
    }

    #[test]
    fn repeat_path_hits() {
        let mut im = Image::new();
        let t = fs();
        let _ = dlopen(&mut im, &t, "./libz.so");
        let o = dlopen(&mut im, &t, "./libz.so");
        assert_eq!(o.stage, Stage::Path);
        assert_eq!(o.handle, 1);
        assert_eq!(o.refs, 2);
        assert_eq!(o.note, Note::PathHit);
        assert_eq!(im.len(), 1);
    }

    #[test]
    fn soname_alias_hits() {
        let mut im = setup_libz();
        let t = fs();
        let o = dlopen(&mut im, &t, "./copy.so");
        assert_eq!(o.stage, Stage::Alias);
        assert_eq!(o.handle, 1);
        assert_eq!(o.refs, 2);
        assert!(o.alias);
        assert_eq!(o.note, Note::SonameAlias);
        assert_eq!(im.len(), 1);
    }

    #[test]
    fn name_lookup_hit_and_miss() {
        let mut im = setup_libz();
        let t = fs();
        let a = dlopen(&mut im, &t, "libz.so.1");
        assert_eq!(a.stage, Stage::Lookup);
        assert_eq!(a.handle, 1);
        assert_eq!(a.refs, 2);
        assert_eq!(a.note, Note::NameHit);
        let b = dlopen(&mut im, &t, "libz.so");
        assert!(!b.ok());
        assert_eq!(b.code, ERR_ENOENT);
        assert_eq!(b.note, Note::NameMiss);
    }

    #[test]
    fn no_soname_registers_basename() {
        let mut im = Image::new();
        let t = fs();
        let o = dlopen(&mut im, &t, "./plain.so");
        assert!(o.ok());
        assert!(!o.alias);
        assert_eq!(o.name, "plain.so");
        assert_eq!(o.note, Note::NoSoname);
        let b = dlopen(&mut im, &t, "plain.so");
        assert_eq!(b.stage, Stage::Lookup);
        assert_eq!(b.refs, 2);
    }

    #[test]
    fn missing_and_dir() {
        let mut im = Image::new();
        let t = fs();
        let a = dlopen(&mut im, &t, "./gone.so");
        assert_eq!(a.stage, Stage::Open);
        assert_eq!(a.code, ERR_ENOENT);
        assert_eq!(a.note, Note::Missing);
        let b = dlopen(&mut im, &t, "./dir");
        assert_eq!(b.stage, Stage::Open);
        assert_eq!(b.code, ERR_EISDIR);
        assert_eq!(b.note, Note::Dir);
    }

    #[test]
    fn claims_match_all_slots() {
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => panic!("missing case"),
            };
            assert_eq!(judge(&c), expected(i).unwrap_or(Fault::Note));
            i += 1;
        }
    }

    #[test]
    fn expected_table() {
        assert_eq!(expected(0), Some(Fault::Ok));
        assert_eq!(expected(7), Some(Fault::Ok));
        assert_eq!(expected(8), Some(Fault::Estate));
        assert_eq!(expected(9), Some(Fault::Handle));
        assert_eq!(expected(10), Some(Fault::Handle));
        assert_eq!(expected(11), Some(Fault::Estate));
        assert_eq!(expected(12), Some(Fault::Alias));
        assert_eq!(expected(13), Some(Fault::Code));
        assert_eq!(expected(14), None);
    }

    #[test]
    fn reject_codes_align() {
        let cs = cases();
        let mut i = 0usize;
        while i < REJECT {
            let c = match cs.get(ACCEPT + i) {
                Some(v) => *v,
                None => panic!("missing reject"),
            };
            assert_eq!(judge(&c).code(), REJECT_CODES[i]);
            i += 1;
        }
    }

    #[test]
    fn corpus_runs_green() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("run failed"),
        };
        assert_eq!(r.total, 14);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert!(r.green());
    }

    #[test]
    fn red_rate_929() {
        let r = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => panic!("run failed"),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert!(!r.green());
    }

    #[test]
    fn render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("run failed"),
        };
        assert_eq!(
            r.render().as_str(),
            "soname corpus=14 accept=8 reject=6 new=2 hit=3 miss=2 other=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn matrix_shape() {
        let m = Matrix::of();
        assert!(m.balanced());
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert_eq!(m.row(0), Some(Row { total: 4, good: 2 }));
        assert_eq!(m.row(1), Some(Row { total: 6, good: 3 }));
        assert_eq!(m.row(2), Some(Row { total: 2, good: 2 }));
        assert_eq!(m.row(3), Some(Row { total: 2, good: 1 }));
        assert_eq!(m.perfect_rows(), 1);
    }

    #[test]
    fn detail_lines_exact() {
        let a = match case_at(0) {
            Some(v) => v,
            None => panic!("missing"),
        };
        let b = match case_at(2) {
            Some(v) => v,
            None => panic!("missing"),
        };
        let c = match case_at(7) {
            Some(v) => v,
            None => panic!("missing"),
        };
        assert_eq!(detail(&a).as_str(), DETAIL_NEW);
        assert_eq!(detail(&b).as_str(), DETAIL_ALIAS);
        assert_eq!(detail(&c).as_str(), DETAIL_DIR);
    }

    #[test]
    fn message_lines_exact() {
        let mut im = Image::new();
        let t = fs();
        let a = dlopen(&mut im, &t, "./dir");
        assert_eq!(message(&a, "./dir").as_str(), "./dir: cannot read file data: Is a directory\n");
        let b = dlopen(&mut im, &t, "zz.so");
        assert_eq!(
            message(&b, "zz.so").as_str(),
            "zz.so: cannot open shared object file: No such file or directory\n"
        );
    }

    #[test]
    fn gate_order_monotonic() {
        assert!(gate_ok());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.audit(), Ok(()));
        assert!(!l.is_empty());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => panic!("freeze failed"),
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert!(l.register(Sample::blank()).is_err());
        assert!(l.rejects() > 0);
    }

    #[test]
    fn injections() {
        let inj = inject_four();
        assert!(inj[0]);
        assert!(inj[1]);
        assert!(inj[2]);
        assert!(inj[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11637_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn tags_unique() {
        let mut i = 0usize;
        while i < CORPUS {
            let a = SHAPES[i].tag;
            let mut j = i + 1;
            while j < CORPUS {
                assert_ne!(a, SHAPES[j].tag);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn cover_bounds() {
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(0), Err(E_SNREQ));
        assert!(case_at(13).is_some());
        assert!(case_at(14).is_none());
    }
}
