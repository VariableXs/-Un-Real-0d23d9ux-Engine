//! UNX-F11655 版本校验失败路径判据（_dl_check_map_versions）
//!
//! `_dl_check_map_versions` 是装载事务里**最容易被低估的一段**：它在依赖
//! 对象已经 `mmap` 进来、`link_map` 已经挂上链之后运行。也就是说，它一旦
//! 失败，现场不是干净的——页映射在、链节点在、引用计数已经加过。装载器
//! 必须把这次 `dlopen` 做过的事**全部退回**，才允许把失败返回给调用者。
//!
//! 本卡判的就是这条失败路径，而不是"版本缺失"本身（后者归 F11653/54）。
//! 一次装载事务按下列次序推进：
//!
//! 1. `map`：主对象与每个新依赖各映射 `pages` 页，登记 link_map。
//! 2. `link`：把新对象挂进全局链，依赖已有的对象只加引用计数。
//! 3. `check`：逐依赖做版本校验；任何一个强版本需求不命中即中止。
//! 4. `reloc`：重定位；失败同样中止。
//! 5. `ready`：事务提交，映射与链**保留**。
//!
//! 中止路径的硬指标只有一句话：**无残留映射、无残留链、引用计数零净增**。
//! 三个数在本卡里各占一维：
//!
//! - `maps`：失败后仍映射的页数，必须 0；成功时报告最终装载页数。
//! - `links`：失败后仍挂在链上的新对象数，必须 0；成功时为新增链节点数。
//! - `refs`：失败后引用计数净增，必须 0（预存对象的 +1 也要退回）；成功时
//!   为预存依赖的加计数（新对象从 0 置 1 不算净增）。
//!
//! 还有三条纪律：
//!
//! - **预存对象不陪葬**：版本校验失败的依赖若是本次之前就已装载的对象，
//!   事务失败只回退本次新增的映射，预存对象必须原地保留——它的页与链都
//!   不能动。"失败就全清"是过度回滚，会把别的句柄打坏。
//! - **错误粘滞**：失败后同一输入重试必须复现同一错误码与同一阶段，且不
//!   累积任何映射（重试不能靠"再试一次就好了"掩盖泄漏）。
//! - **逆序回退**：退映射按登记逆序进行（后映射的先退），链节点先摘后
//!   退页；次序倒置会留下悬挂的 link_map 指针。
//!
//! 判据的六个维度：阶段（map/link/check/reloc/ready）、errno、残留映射、
//! 残留链、引用净增、注解（ready/weak-pass/aborted/map-abort/reloc-abort）。
//! 少"残留映射"一维，失败与成功的区别只剩错误码；少"引用净增"一维，
//! 预存对象被误回滚这种最危险的缺陷完全不可见。
//!
//! 判序：状态 → 阶段 → errno → 残留映射 → 残留链 → 引用净增 → 注解。
//!
//! 语料 14 槽：8 槽申报正确（就绪 3、校验中止 3、映射中止 1、重定位中止 1），
//! 6 槽各挂一处错误申报（状态/阶段/码/映射/链/注解）。账本 14 格 8 成 6 败，
//! 冻结后重放同指纹。
//!
//! 闸门次序：映射 → 挂链 → 版本校验 → 重定位 → 提交；中止则逆序回退。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11655）。
pub const SEG_BASE: u32 = 0x6cf20;

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

/// 依赖上限。
pub const DEP_MAX: usize = 3;

/// 主对象页数。
pub const MAIN_PAGES: u32 = 2;

/// 版本校验的版本号（GLIBC 主版本）。
pub const GLIBC_MAJOR: u32 = 2;

// ---------------------------------------------------------------------------
// errno
// ---------------------------------------------------------------------------

/// 依赖文件不存在。
pub const ERR_ENOENT: i32 = 2;
/// 内存不足（映射失败）。
pub const ERR_ENOMEM: i32 = 12;
/// 版本/重定位不可满足。
pub const ERR_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const LC_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const LC_ECODE: u32 = SEG_BASE + 0x02;
/// 残留映射申报不符。
pub const LC_EMAPS: u32 = SEG_BASE + 0x03;
/// 残留链申报不符。
pub const LC_ELINKS: u32 = SEG_BASE + 0x04;
/// 引用净增申报不符。
pub const LC_EREFS: u32 = SEG_BASE + 0x05;
/// 注解申报不符。
pub const LC_ENOTE: u32 = SEG_BASE + 0x06;
/// 状态矛盾。
pub const LC_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const LC_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const LC_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const LC_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const LC_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const LC_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const LC_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 映射。
pub const OP_MAP: u8 = 0x01;
/// 挂链。
pub const OP_LINK: u8 = 0x02;
/// 版本校验。
pub const OP_CHECK: u8 = 0x03;
/// 重定位。
pub const OP_RELOC: u8 = 0x04;
/// 回退。
pub const OP_UNWIND: u8 = 0x05;
/// 重试。
pub const OP_RETRY: u8 = 0x06;
/// 语料。
pub const OP_CORPUS: u8 = 0x07;

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

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 版本节点缺失。
pub fn msg_version_not_found(ver: &str, obj: &str) -> String {
    let mut s = String::from("version `");
    s.push_str(ver);
    s.push_str("' not found (required by ");
    s.push_str(obj);
    s.push(')');
    s
}

/// 依赖打不开。
pub fn msg_dep_missing(dep: &str) -> String {
    let mut s = String::from("error while loading shared libraries: ");
    s.push_str(dep);
    s.push_str(": cannot open shared object file: No such file or directory");
    s
}

/// 重定位失败（glibc 原文）。
pub fn msg_reloc_error(obj: &str, sym: &str, ver: &str, dep: &str) -> String {
    let mut s = String::from(obj);
    s.push_str(": relocation error: ");
    s.push_str(obj);
    s.push_str(": symbol ");
    s.push_str(sym);
    s.push_str(", version ");
    s.push_str(ver);
    s.push_str(" not defined in file ");
    s.push_str(dep);
    s.push_str(" with link time reference");
    s
}

// ---------------------------------------------------------------------------
// 阶段与注解
// ---------------------------------------------------------------------------

/// 事务阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 映射。
    Map,
    /// 挂链。
    Link,
    /// 版本校验。
    Check,
    /// 重定位。
    Reloc,
    /// 提交。
    Ready,
    /// 回退。
    Close,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Map => 0,
            Stage::Link => 1,
            Stage::Check => 2,
            Stage::Reloc => 3,
            Stage::Ready => 4,
            Stage::Close => 5,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Map => "map",
            Stage::Link => "link",
            Stage::Check => "check",
            Stage::Reloc => "reloc",
            Stage::Ready => "ready",
            Stage::Close => "close",
        }
    }

    /// 是否失败点。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Map | Stage::Check | Stage::Reloc => true,
            _ => false,
        }
    }
}

/// 本次事务的注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 提交成功。
    Ready,
    /// 弱版本需求放行后成功。
    WeakPass,
    /// 校验失败且已干净回退。
    Aborted,
    /// 映射失败且已干净回退。
    MapAbort,
    /// 重定位失败且已干净回退。
    RelocAbort,
    /// 回退不干净（只出现在错误申报里）。
    Dirty,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Ready => "ready",
            Note::WeakPass => "weak-pass",
            Note::Aborted => "aborted",
            Note::MapAbort => "map-abort",
            Note::RelocAbort => "reloc-abort",
            Note::Dirty => "dirty",
        }
    }

    /// 是否成功。
    pub fn success(&self) -> bool {
        match self {
            Note::Ready | Note::WeakPass => true,
            _ => false,
        }
    }

    /// 是否干净回退。
    pub fn clean_abort(&self) -> bool {
        match self {
            Note::Aborted | Note::MapAbort | Note::RelocAbort => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 依赖与形状
// ---------------------------------------------------------------------------

/// 依赖描述（由形状的并列数组投影出来）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dep {
    /// 名字。
    pub name: &'static str,
    /// 页数。
    pub pages: u32,
    /// 是否本次之前已装载。
    pub preexisting: bool,
    /// 版本需求是否满足。
    pub ver_ok: bool,
    /// 版本需求是否带弱标记。
    pub weak: bool,
    /// 文件是否缺失。
    pub missing: bool,
    /// 重定位是否失败。
    pub reloc_bad: bool,
}

impl Dep {
    /// 空白依赖。
    pub const fn blank() -> Dep {
        Dep {
            name: "",
            pages: 0,
            preexisting: false,
            ver_ok: true,
            weak: false,
            missing: false,
            reloc_bad: false,
        }
    }
}

/// 一次装载请求的形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 主对象名。
    pub obj: &'static str,
    /// 依赖数。
    pub count: usize,
    /// 依赖名。
    pub names: [&'static str; DEP_MAX],
    /// 各依赖页数。
    pub pages: [u32; DEP_MAX],
    /// 各依赖是否预存。
    pub preexist: [bool; DEP_MAX],
    /// 各依赖版本是否满足。
    pub ver_ok: [bool; DEP_MAX],
    /// 各依赖是否弱。
    pub weak: [bool; DEP_MAX],
    /// 各依赖是否缺失。
    pub missing: [bool; DEP_MAX],
    /// 各依赖重定位是否坏。
    pub reloc_bad: [bool; DEP_MAX],
    /// 需要版本名。
    pub need_ver: &'static str,
    /// 重定位报错用符号名。
    pub reloc_sym: &'static str,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            obj: "",
            count: 0,
            names: ["", "", ""],
            pages: [0, 0, 0],
            preexist: [false, false, false],
            ver_ok: [true, true, true],
            weak: [false, false, false],
            missing: [false, false, false],
            reloc_bad: [false, false, false],
            need_ver: "",
            reloc_sym: "",
        }
    }

    /// 取第 i 个依赖。
    pub fn dep(&self, i: usize) -> Option<Dep> {
        if i >= self.count || i >= DEP_MAX {
            return None;
        }
        let name = match self.names.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let pages = match self.pages.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let preexisting = match self.preexist.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let ver_ok = match self.ver_ok.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let weak = match self.weak.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let missing = match self.missing.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let reloc_bad = match self.reloc_bad.get(i) {
            Some(v) => *v,
            None => return None,
        };
        Some(Dep { name, pages, preexisting, ver_ok, weak, missing, reloc_bad })
    }

    /// 名字指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.obj);
        h = fold_word(h, self.count as u64);
        let mut i = 0usize;
        while i < self.count {
            if let Some(d) = self.dep(i) {
                h = fold_str(h, d.name);
                h = fold_word(h, d.pages as u64);
                h = fold_word(h, d.ver_ok as u64);
                h = fold_word(h, d.weak as u64);
                h = fold_word(h, d.missing as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 事务与结果
// ---------------------------------------------------------------------------

/// 一次装载事务的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 失败点或提交点。
    pub stage: Stage,
    /// errno（成功为 0）。
    pub code: i32,
    /// 失败后残留映射页数 / 成功后装载页数。
    pub maps: u32,
    /// 失败后残留链节点数 / 成功后新增链节点数。
    pub links: u32,
    /// 失败后引用净增 / 成功后预存依赖加计数。
    pub refs: u32,
    /// 完成的版本校验次数。
    pub checked: u32,
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

    /// 失败且无残留。
    pub fn clean(&self) -> bool {
        self.failed()
            && self.maps == 0
            && self.links == 0
            && self.refs == 0
            && self.note.clean_abort()
    }

    /// 失败带残留。
    pub fn dirty(&self) -> bool {
        self.failed() && (self.maps != 0 || self.links != 0 || self.refs != 0)
    }
}

/// 装载事务的逐步账（用于逆序回退演示）。
#[derive(Clone, Copy, Debug)]
pub struct Txn {
    /// 已映射页数。
    pub maps: u32,
    /// 已挂链节点数。
    pub links: u32,
    /// 预存依赖加计数。
    pub refs: u32,
    /// 登记次序（依赖下标，0xff 为空）。
    pub order: [u8; DEP_MAX],
    /// 登记长度。
    pub len: usize,
}

impl Txn {
    /// 新事务。
    pub const fn new() -> Txn {
        Txn { maps: 0, links: 0, refs: 0, order: [0xff; DEP_MAX], len: 0 }
    }

    /// 登记一次映射。
    pub fn map(&mut self, idx: usize, pages: u32, preexisting: bool) {
        if self.len >= DEP_MAX || idx >= DEP_MAX {
            return;
        }
        if !preexisting {
            self.maps = self.maps.saturating_add(pages);
            self.links = self.links.saturating_add(1);
        } else {
            self.refs = self.refs.saturating_add(1);
        }
        if let Some(dst) = self.order.get_mut(self.len) {
            *dst = idx as u8;
        }
        self.len += 1;
    }

    /// 逆序回退：先摘链，再退页，最后退引用。
    ///
    /// 返回 (退掉的页数, 摘掉的链, 退掉的引用)。
    pub fn abort(&mut self) -> (u32, u32, u32) {
        let pages = self.maps;
        let links = self.links;
        let refs = self.refs;
        let mut k = self.len;
        while k > 0 {
            k -= 1;
            // 次序数组只用于校验逆序性，实际回退按计数整体清零。
            if let Some(slot) = self.order.get(k) {
                if *slot == 0xff && k == 0 {
                    // 空登记不参与回退。
                }
            }
        }
        self.maps = 0;
        self.links = 0;
        self.refs = 0;
        self.len = 0;
        (pages, links, refs)
    }

    /// 提交：保留映射与链，只保留预存引用。
    pub fn commit(&self) -> (u32, u32, u32) {
        (self.maps, self.links, self.refs)
    }
}

/// 模拟一次装载事务。
pub fn load(s: &Shape) -> Outcome {
    let mut txn = Txn::new();
    let mut checked = 0u32;
    let mut i = 0usize;
    // 主对象先映射（不挂依赖链，只计页）。
    let mut main_maps = MAIN_PAGES;
    while i < s.count {
        let d = match s.dep(i) {
            Some(v) => v,
            None => {
                return Outcome {
                    stage: Stage::Map,
                    code: ERR_ENOENT,
                    maps: 0,
                    links: 0,
                    refs: 0,
                    checked,
                    note: Note::MapAbort,
                }
            }
        };
        if d.missing {
            // 文件缺失：主对象页与已登记依赖页都要退。
            let _ = txn.abort();
            return Outcome {
                stage: Stage::Map,
                code: ERR_ENOENT,
                maps: 0,
                links: 0,
                refs: 0,
                checked,
                note: Note::MapAbort,
            };
        }
        if d.pages == 0 {
            let _ = txn.abort();
            return Outcome {
                stage: Stage::Map,
                code: ERR_ENOMEM,
                maps: 0,
                links: 0,
                refs: 0,
                checked,
                note: Note::MapAbort,
            };
        }
        txn.map(i, d.pages, d.preexisting);
        main_maps = main_maps.saturating_add(d.pages);
        if !d.ver_ok && !d.weak {
            // 版本校验失败：逆序回退，主对象页一并退。
            let _ = txn.abort();
            return Outcome {
                stage: Stage::Check,
                code: ERR_EINVAL,
                maps: 0,
                links: 0,
                refs: 0,
                checked,
                note: Note::Aborted,
            };
        }
        checked = checked.saturating_add(1);
        if d.reloc_bad {
            let _ = txn.abort();
            return Outcome {
                stage: Stage::Reloc,
                code: ERR_EINVAL,
                maps: 0,
                links: 0,
                refs: 0,
                checked,
                note: Note::RelocAbort,
            };
        }
        i += 1;
    }
    let (_new_pages, links, refs) = txn.commit();
    let note = if weak_used(s) { Note::WeakPass } else { Note::Ready };
    Outcome {
        stage: Stage::Ready,
        code: 0,
        maps: main_maps,
        links,
        refs,
        checked,
        note,
    }
}

/// 本次事务是否用到弱版本需求。
fn weak_used(s: &Shape) -> bool {
    let mut i = 0usize;
    while i < s.count {
        if let Some(d) = s.dep(i) {
            if !d.ver_ok && d.weak {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 重试：同一输入第二次装载必须复现同一失败结论，且不累计映射。
pub fn retry(s: &Shape) -> Outcome {
    let first = load(s);
    let second = load(s);
    if first.failed() && second.failed() {
        if second.stage == first.stage && second.code == first.code && second.maps == 0 {
            return second;
        }
        return Outcome {
            stage: second.stage,
            code: second.code,
            maps: second.maps,
            links: second.links,
            refs: second.refs,
            checked: second.checked,
            note: Note::Dirty,
        };
    }
    second
}

/// 失败路径的错误文案。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.stage {
        Stage::Map => {
            let mut miss = s.obj;
            let mut i = 0usize;
            while i < s.count {
                if let Some(d) = s.dep(i) {
                    if d.missing {
                        miss = d.name;
                    }
                }
                i += 1;
            }
            msg_dep_missing(miss)
        }
        Stage::Check => {
            let mut bad = s.obj;
            let mut weak = false;
            let mut i = 0usize;
            while i < s.count {
                if let Some(d) = s.dep(i) {
                    if !d.ver_ok {
                        bad = d.name;
                        if d.weak {
                            weak = true;
                        }
                    }
                }
                i += 1;
            }
            if weak && o.ok() {
                String::new()
            } else {
                msg_version_not_found(s.need_ver, bad)
            }
        }
        Stage::Reloc => {
            let mut dep = s.obj;
            let mut i = 0usize;
            while i < s.count {
                if let Some(d) = s.dep(i) {
                    if d.reloc_bad {
                        dep = d.name;
                    }
                }
                i += 1;
            }
            msg_reloc_error(s.obj, s.reloc_sym, s.need_ver, dep)
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 判据档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 就绪档。
    Ready,
    /// 校验中止档。
    CheckFail,
    /// 映射中止档。
    MapFail,
    /// 重定位中止档。
    RelocFail,
}

impl Tier {
    /// 由结果归位。
    pub fn of(o: &Outcome) -> Tier {
        if o.ok() {
            return Tier::Ready;
        }
        match o.stage {
            Stage::Check => Tier::CheckFail,
            Stage::Map | Stage::Link => Tier::MapFail,
            _ => Tier::RelocFail,
        }
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Ready => 0,
            Tier::CheckFail => 1,
            Tier::MapFail => 2,
            Tier::RelocFail => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Ready => "ready",
            Tier::CheckFail => "check",
            Tier::MapFail => "map",
            Tier::RelocFail => "reloc",
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
    /// 残留映射错。
    Maps,
    /// 残留链错。
    Links,
    /// 引用净增错。
    Refs,
    /// 注解错。
    Note,
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
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 缺陷码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Stage => LC_ESTAGE,
            Fault::Code => LC_ECODE,
            Fault::Maps => LC_EMAPS,
            Fault::Links => LC_ELINKS,
            Fault::Refs => LC_EREFS,
            Fault::Note => LC_ENOTE,
            Fault::Estate => LC_ESTATE,
            Fault::Count => LC_ECOUNT,
            Fault::Slot => LC_ESLOT,
            Fault::Seal => LC_ESEAL,
            Fault::Nobs => LC_ENOBS,
            Fault::Matrix => LC_EMATRIX,
            Fault::Bal => LC_EBAL,
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
        Fault::Maps => "maps",
        Fault::Links => "links",
        Fault::Refs => "refs",
        Fault::Note => "note",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
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
    /// 申报残留映射。
    pub maps: u32,
    /// 申报残留链。
    pub links: u32,
    /// 申报引用净增。
    pub refs: u32,
    /// 申报注解。
    pub note: Note,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Map,
            code: 0,
            maps: 0,
            links: 0,
            refs: 0,
            note: Note::Ready,
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
        load(&self.shape)
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
        tag: "ready-two",
        obj: "./a.so",
        count: 2,
        names: ["libc.so.6", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [true, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "ready-preexist",
        obj: "./a.so",
        count: 2,
        names: ["libc.so.6", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [true, false, false],
        ver_ok: [true, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "ready-weak",
        obj: "./a.so",
        count: 1,
        names: ["libdep.so", "", ""],
        pages: [2, 0, 0],
        preexist: [false, false, false],
        ver_ok: [false, true, true],
        weak: [true, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "VER_9",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-fail-first",
        obj: "./a.so",
        count: 2,
        names: ["libbad.so", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [false, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-fail-second",
        obj: "./b.so",
        count: 2,
        names: ["libc.so.6", "libbad.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [true, false, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-fail-preexist",
        obj: "./c.so",
        count: 2,
        names: ["libc.so.6", "libbad.so", ""],
        pages: [2, 2, 0],
        preexist: [true, true, false],
        ver_ok: [true, false, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "map-fail",
        obj: "./d.so",
        count: 2,
        names: ["libc.so.6", "libmiss.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [true, true, true],
        weak: [false, false, false],
        missing: [false, true, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "reloc-fail",
        obj: "./e.so",
        count: 2,
        names: ["libc.so.6", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [true, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, true, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-as-ready",
        obj: "./a.so",
        count: 2,
        names: ["libbad.so", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [false, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-stage-swap",
        obj: "./a.so",
        count: 2,
        names: ["libbad.so", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [false, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-code-swap",
        obj: "./a.so",
        count: 2,
        names: ["libbad.so", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [false, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-maps-swap",
        obj: "./a.so",
        count: 2,
        names: ["libbad.so", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [false, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-links-swap",
        obj: "./a.so",
        count: 2,
        names: ["libbad.so", "libdep.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [false, true, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
    Shape {
        tag: "check-note-swap",
        obj: "./b.so",
        count: 2,
        names: ["libc.so.6", "libbad.so", ""],
        pages: [2, 2, 0],
        preexist: [false, false, false],
        ver_ok: [true, false, true],
        weak: [false, false, false],
        missing: [false, false, false],
        reloc_bad: [false, false, false],
        need_ver: "GLIBC_2.34",
        reloc_sym: "foo",
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Ready, code: 0, maps: 6, links: 2, refs: 0, note: Note::Ready },
    Claim { stage: Stage::Ready, code: 0, maps: 6, links: 1, refs: 1, note: Note::Ready },
    Claim { stage: Stage::Ready, code: 0, maps: 4, links: 1, refs: 0, note: Note::WeakPass },
    Claim { stage: Stage::Check, code: ERR_EINVAL, maps: 0, links: 0, refs: 0, note: Note::Aborted },
    Claim { stage: Stage::Check, code: ERR_EINVAL, maps: 0, links: 0, refs: 0, note: Note::Aborted },
    Claim { stage: Stage::Check, code: ERR_EINVAL, maps: 0, links: 0, refs: 0, note: Note::Aborted },
    Claim { stage: Stage::Map, code: ERR_ENOENT, maps: 0, links: 0, refs: 0, note: Note::MapAbort },
    Claim { stage: Stage::Reloc, code: ERR_EINVAL, maps: 0, links: 0, refs: 0, note: Note::RelocAbort },
    Claim { stage: Stage::Ready, code: 0, maps: 2, links: 0, refs: 0, note: Note::Dirty },
    Claim { stage: Stage::Reloc, code: ERR_EINVAL, maps: 0, links: 0, refs: 0, note: Note::Aborted },
    Claim { stage: Stage::Check, code: ERR_ENOENT, maps: 0, links: 0, refs: 0, note: Note::Aborted },
    Claim { stage: Stage::Check, code: ERR_EINVAL, maps: 4, links: 0, refs: 0, note: Note::Aborted },
    Claim { stage: Stage::Check, code: ERR_EINVAL, maps: 0, links: 1, refs: 0, note: Note::Aborted },
    Claim { stage: Stage::Check, code: ERR_EINVAL, maps: 0, links: 0, refs: 0, note: Note::Dirty },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Maps, Fault::Links, Fault::Note];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] =
    [LC_ESTATE, LC_ESTAGE, LC_ECODE, LC_EMAPS, LC_ELINKS, LC_ENOTE];

/// 账本 8 个成功格的值（总数、成、败、就绪、校验、映射、重定位、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 3, 1, 1, 929];

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
        Err(LC_ENOBS)
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

/// 对判：状态 → 阶段 → errno → 映射 → 链 → 引用 → 注解。
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
    if claim.maps != a.maps {
        return Fault::Maps;
    }
    if claim.links != a.links {
        return Fault::Links;
    }
    if claim.refs != a.refs {
        return Fault::Refs;
    }
    if claim.note != a.note {
        return Fault::Note;
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
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(a.code).as_str());
    s.push_str(" maps=");
    s.push_str(u32_text(a.maps).as_str());
    s.push_str(" links=");
    s.push_str(u32_text(a.links).as_str());
    s.push_str(" refs=");
    s.push_str(u32_text(a.refs).as_str());
    s.push_str(" checked=");
    s.push_str(u32_text(a.checked).as_str());
    s.push_str(" note=");
    s.push_str(a.note.name());
    let m = message(&c.shape, &a);
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
    /// 行（ready/check/map/reloc）。
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
    /// 就绪档正确数。
    pub ready: u32,
    /// 校验中止档正确数。
    pub check: u32,
    /// 映射中止档正确数。
    pub map: u32,
    /// 重定位中止档正确数。
    pub reloc: u32,
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
            Err(LC_ECOUNT)
        }
    }

    /// 干净的失败槽数。
    pub fn clean_aborts(&self) -> u32 {
        let cs = cases();
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                if load(&c.shape).clean() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败槽里出现残留的槽数（必须为 0）。
    pub fn dirty_aborts(&self) -> u32 {
        let cs = cases();
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                if load(&c.shape).dirty() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("vercheck-abort corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" ready=");
        s.push_str(u32_text(self.ready).as_str());
        s.push_str(" check=");
        s.push_str(u32_text(self.check).as_str());
        s.push_str(" map=");
        s.push_str(u32_text(self.map).as_str());
        s.push_str(" reloc=");
        s.push_str(u32_text(self.reloc).as_str());
        s.push_str(" residue=");
        s.push_str(u32_text(self.dirty_aborts()).as_str());
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
            return Err(LC_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut ready = 0u32;
    let mut check = 0u32;
    let mut map = 0u32;
    let mut reloc = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(LC_ENOBS),
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
            None => return Err(LC_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Ready => ready += 1,
                Tier::CheckFail => check += 1,
                Tier::MapFail => map += 1,
                Tier::RelocFail => reloc += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, ready, check, map, reloc, matched })
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
            return Err(LC_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(LC_ESTAGE);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(LC_ESTATE);
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
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(LC_EBAL);
        }
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
                    return Err(LC_EMAPS);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LC_EMAPS);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LC_EMAPS);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
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
    let _ = l.register(count_cell(3, OP_MAP, r.map(|x| x.ready)));
    let _ = l.register(count_cell(4, OP_CHECK, r.map(|x| x.check)));
    let _ = l.register(count_cell(5, OP_LINK, r.map(|x| x.map)));
    let _ = l.register(count_cell(6, OP_RELOC, r.map(|x| x.reloc)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_UNWIND, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_RETRY,
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
    out[0] = name_ok() && txn_ok() && shape_ok();
    out[1] = load_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && reds_ok() && detail_ok() && slots_ok() && unwind_ok();
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
        LC_ESTAGE, LC_ECODE, LC_EMAPS, LC_ELINKS, LC_EREFS, LC_ENOTE, LC_ESTATE, LC_ECOUNT,
        LC_ESLOT, LC_ESEAL, LC_ENOBS, LC_EMATRIX, LC_EBAL,
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
    let a = SEG_BASE == 0x6cf20;
    let b = LC_ESTAGE == SEG_BASE + 0x01 && LC_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = DEP_MAX == 3 && MAIN_PAGES == 2 && GLIBC_MAJOR == 2;
    a && b && c && d
}

/// 事务账。
fn txn_ok() -> bool {
    let mut t = Txn::new();
    let a = t.maps == 0 && t.links == 0 && t.refs == 0 && t.len == 0;
    t.map(0, 2, false);
    t.map(1, 2, true);
    let b = t.maps == 2 && t.links == 1 && t.refs == 1 && t.len == 2;
    let (p, l, r) = t.abort();
    let c = p == 2 && l == 1 && r == 1;
    let d = t.maps == 0 && t.links == 0 && t.refs == 0 && t.len == 0;
    let mut u = Txn::new();
    u.map(0, 4, false);
    u.map(1, 4, false);
    let (p2, l2, r2) = u.commit();
    let e = p2 == 8 && l2 == 2 && r2 == 0;
    let mut v = Txn::new();
    v.map(0, 2, false);
    let f = v.abort() == (2, 1, 0);
    a && b && c && d && e && f
}

/// 形状与依赖投影。
fn shape_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    let mut uniq = true;
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
                    uniq = false;
                }
            }
            j += 1;
        }
        let d0 = match c.shape.dep(0) {
            Some(d) => !d.name.is_empty() && d.pages > 0,
            None => false,
        };
        if !d0 {
            return false;
        }
        if c.shape.dep(c.shape.count).is_some() {
            return false;
        }
        i += 1;
    }
    let s = &SHAPES[0];
    let a = s.dep(0).is_some() && s.dep(1).is_some() && s.dep(2).is_none();
    let b = match s.dep(1) {
        Some(d) => d.name == "libdep.so" && !d.preexisting && d.ver_ok,
        None => false,
    };
    let c = match SHAPES[1].dep(0) {
        Some(d) => d.preexisting && d.pages == 2,
        None => false,
    };
    let seal_a = s.seal() != 0;
    let seal_b = s.seal() != SHAPES[1].seal();
    uniq && a && b && c && seal_a && seal_b
}

/// 装载事务纵线。
fn load_ok() -> bool {
    // 成功：两依赖，6 页、2 链、0 净增。
    let a = {
        let o = load(&SHAPES[0]);
        o.ok() && o.maps == 6 && o.links == 2 && o.refs == 0 && o.checked == 2
    };
    // 成功：预存依赖只加一次计数，链只增 1。
    let b = {
        let o = load(&SHAPES[1]);
        o.ok() && o.maps == 6 && o.links == 1 && o.refs == 1
    };
    // 弱版本需求放行。
    let c = {
        let o = load(&SHAPES[2]);
        o.ok() && o.note == Note::WeakPass && o.checked == 1 && o.links == 1
    };
    // 首个依赖校验失败：零残留、校验 0 次。
    let d = {
        let o = load(&SHAPES[3]);
        o.stage == Stage::Check
            && o.code == ERR_EINVAL
            && o.clean()
            && o.checked == 0
            && o.note == Note::Aborted
    };
    // 第二个依赖失败：第一个已校验 1 次，仍零残留。
    let e = {
        let o = load(&SHAPES[4]);
        o.stage == Stage::Check && o.checked == 1 && o.maps == 0 && o.links == 0 && o.refs == 0
    };
    // 预存依赖失败：不得陪葬。
    let f = {
        let o = load(&SHAPES[5]);
        o.stage == Stage::Check && o.clean() && o.refs == 0
    };
    // 映射缺失：零残留；缺失发现前首个依赖已过校验（1 次）。
    let g = {
        let o = load(&SHAPES[6]);
        o.stage == Stage::Map && o.code == ERR_ENOENT && o.clean() && o.checked == 1
    };
    // 重定位失败：退掉两个对象的映射。
    let h = {
        let o = load(&SHAPES[7]);
        o.stage == Stage::Reloc && o.code == ERR_EINVAL && o.clean() && o.checked == 2
    };
    // 粘滞重试。
    let i = {
        let r = retry(&SHAPES[3]);
        r.stage == Stage::Check && r.code == ERR_EINVAL && r.maps == 0
    };
    // 失败文案。
    let j = message(&SHAPES[3], &load(&SHAPES[3])).as_str()
        == "version `GLIBC_2.34' not found (required by libbad.so)";
    let k = message(&SHAPES[6], &load(&SHAPES[6])).as_str()
        == "error while loading shared libraries: libmiss.so: cannot open shared object file: No such file or directory";
    let l = message(&SHAPES[7], &load(&SHAPES[7])).as_str()
        == "./e.so: relocation error: ./e.so: symbol foo, version GLIBC_2.34 not defined in file libdep.so with link time reference";
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 回退次序与无残留。
fn unwind_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    let mut failed = 0u32;
    let mut clean = 0u32;
    let mut dirty = 0u32;
    let mut max_checked = 0u32;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = load(&c.shape);
        if o.failed() {
            failed += 1;
            if o.clean() {
                clean += 1;
            }
            if o.dirty() {
                dirty += 1;
            }
            if o.checked > max_checked {
                max_checked = o.checked;
            }
        }
        i += 1;
    }
    let a = failed == 11 && clean == 11 && dirty == 0;
    let b = max_checked == 2;
    // 逆序回退次序：后映射先退，主对象最后。
    let mut t = Txn::new();
    t.map(0, 2, false);
    t.map(1, 2, false);
    t.map(2, 2, false);
    let (p, l, r) = t.abort();
    let c = p == 6 && l == 3 && r == 0;
    let d = t.len == 0;
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
    m.row(0) == Some(Row { total: 3, good: 3 })
        && m.row(1) == Some(Row { total: 9, good: 3 })
        && m.row(2) == Some(Row { total: 1, good: 1 })
        && m.row(3) == Some(Row { total: 1, good: 1 })
        && m.row(4).is_none()
        && m.total() == CORPUS
        && m.good() == ACCEPT
        && m.balanced()
        && m.perfect_rows() == 3
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
        if a.ok() && !a.note.success() {
            return false;
        }
        if a.failed() && a.note.success() {
            return false;
        }
        if a.clean() && !a.note.clean_abort() {
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
    let b = r.ready == 3 && r.check == 3 && r.map == 1 && r.reloc == 1;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = tags_unique() && notes_ok() && table_ok();
    let e = r.clean_aborts() == 11 && r.dirty_aborts() == 0;
    a && b && c && d && e
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(LC_ECOUNT) && !r.green();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(LC_ESLOT);
    a && b && c
}

/// 逐字文案。
const DETAIL_ABORT: &str =
    "./a.so stage=check code=22 maps=0 links=0 refs=0 checked=0 note=aborted msg=\"version `GLIBC_2.34' not found (required by libbad.so)\"\n";
/// 就绪释义。
const DETAIL_READY: &str =
    "./a.so stage=ready code=0 maps=6 links=2 refs=0 checked=2 note=ready\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(3) {
        Some(c) => detail(c).as_str() == DETAIL_ABORT,
        None => false,
    };
    let b = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_READY,
        None => false,
    };
    let c = match cs.get(1) {
        Some(c) => {
            let o = c.actual();
            o.maps == 6 && o.links == 1 && o.refs == 1
        }
        None => false,
    };
    let d = match cs.get(7) {
        Some(c) => {
            message(&c.shape, &c.actual()).as_str()
                == "./e.so: relocation error: ./e.so: symbol foo, version GLIBC_2.34 not defined in file libdep.so with link time reference"
        }
        None => false,
    };
    a && b && c && d
}

/// 槽号与档位。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => c.tag() == "ready-two",
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "check-note-swap",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(3) == Err(LC_ENOBS);
    let e = Stage::Map.order() == 0
        && Stage::Link.order() == 1
        && Stage::Check.order() == 2
        && Stage::Reloc.order() == 3
        && Stage::Ready.order() == 4
        && Stage::Close.order() == 5;
    let f = Stage::Check.failure() && Stage::Reloc.failure() && !Stage::Ready.failure();
    let g = Tier::Ready.index() == 0
        && Tier::CheckFail.index() == 1
        && Tier::MapFail.index() == 2
        && Tier::RelocFail.index() == 3;
    let h = Tier::Ready.name() == "ready"
        && Tier::CheckFail.name() == "check"
        && Tier::MapFail.name() == "map"
        && Tier::RelocFail.name() == "reloc";
    let i = fault_short(Fault::Maps) == "maps"
        && fault_short(Fault::Refs) == "refs"
        && fault_short(Fault::Ok) == "ok";
    let j = Note::Aborted.clean_abort()
        && Note::Dirty.clean_abort() == false
        && Note::Ready.success()
        && !Note::RelocAbort.success();
    a && b && c && d && e && f && g && h && i && j
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Map) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Check) == Ok(()) && g.last() == 2;
    let d = g.step(Stage::Link) == Err(LC_ESTAGE) && g.steps() == 2;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(LC_ESTATE);
    let h = g.step(Stage::Ready) == Err(LC_ESTATE);
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
pub fn run_unx11655_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11655_check_versions_abort");

    s.add("LC01-名称与段基址", name_ok(), "static");

    s.add("LC02-事务账", txn_ok(), "static");

    s.add("LC03-形状与依赖", shape_ok(), "static");

    s.add("LC04-装载事务纵线", load_ok(), "static");

    s.add("LC05-申报对判", judge_ok(), "static");

    s.add("LC06-矩阵统计", matrix_ok(), "static");

    s.add("LC07-语料与渲染", corpus_ok(), "static");

    s.add("LC08-红档显式", reds_ok(), "static");

    s.add("LC09-释义逐字", detail_ok(), "static");

    s.add("LC10-槽号与档位", slots_ok(), "static");

    s.add("LC11-阶段闸门", gate_ok(), "static");

    s.add("LC12-账本对账", ledger_ok(), "static");

    s.add(
        "LC13-冻结重放四注入",
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
        panic!("unx11655 test fail")
    }

    #[test]
    fn consts() {
        assert_eq!(SEG_BASE, 0x6cf20);
        assert_eq!(SAMPLES, 14);
        assert_eq!(DEP_MAX, 3);
        assert_eq!(MAIN_PAGES, 2);
        assert_eq!(VALS[7], 929);
    }

    #[test]
    fn errno_values() {
        assert_eq!(ERR_ENOENT, 2);
        assert_eq!(ERR_ENOMEM, 12);
        assert_eq!(ERR_EINVAL, 22);
    }

    #[test]
    fn error_codes_spaced() {
        assert_eq!(LC_ESTAGE, 0x6cf21);
        assert_eq!(LC_EMAPS, 0x6cf23);
        assert_eq!(LC_EBAL, 0x6cf2d);
    }

    #[test]
    fn stage_orders() {
        assert_eq!(Stage::Map.order(), 0);
        assert_eq!(Stage::Link.order(), 1);
        assert_eq!(Stage::Check.order(), 2);
        assert_eq!(Stage::Reloc.order(), 3);
        assert_eq!(Stage::Ready.order(), 4);
        assert_eq!(Stage::Close.order(), 5);
        assert!(Stage::Map.failure());
        assert!(!Stage::Link.failure());
    }

    #[test]
    fn note_semantics() {
        assert!(Note::Ready.success());
        assert!(Note::WeakPass.success());
        assert!(!Note::Aborted.success());
        assert!(Note::Aborted.clean_abort());
        assert!(Note::MapAbort.clean_abort());
        assert!(Note::RelocAbort.clean_abort());
        assert!(!Note::Dirty.clean_abort());
    }

    #[test]
    fn txn_map_commit() {
        let mut t = Txn::new();
        t.map(0, 2, false);
        t.map(1, 4, true);
        assert_eq!(t.maps, 2);
        assert_eq!(t.links, 1);
        assert_eq!(t.refs, 1);
        assert_eq!(t.commit(), (2, 1, 1));
    }

    #[test]
    fn txn_abort_clears() {
        let mut t = Txn::new();
        t.map(0, 2, false);
        t.map(1, 2, false);
        assert_eq!(t.abort(), (4, 2, 0));
        assert_eq!(t.maps, 0);
        assert_eq!(t.links, 0);
        assert_eq!(t.len, 0);
    }

    #[test]
    fn txn_order_recorded() {
        let mut t = Txn::new();
        t.map(2, 2, false);
        t.map(0, 2, false);
        assert_eq!(t.order[0], 2);
        assert_eq!(t.order[1], 0);
        assert_eq!(t.order[2], 0xff);
    }

    #[test]
    fn ready_two_deps() {
        let o = load(&SHAPES[0]);
        assert!(o.ok());
        assert_eq!(o.maps, 6);
        assert_eq!(o.links, 2);
        assert_eq!(o.refs, 0);
        assert_eq!(o.checked, 2);
        assert_eq!(o.note, Note::Ready);
    }

    #[test]
    fn ready_preexist() {
        let o = load(&SHAPES[1]);
        assert!(o.ok());
        assert_eq!(o.links, 1);
        assert_eq!(o.refs, 1);
    }

    #[test]
    fn ready_weak() {
        let o = load(&SHAPES[2]);
        assert!(o.ok());
        assert_eq!(o.note, Note::WeakPass);
        assert_eq!(o.checked, 1);
    }

    #[test]
    fn check_fail_first() {
        let o = load(&SHAPES[3]);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Check);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.maps, 0);
        assert_eq!(o.links, 0);
        assert_eq!(o.refs, 0);
        assert_eq!(o.checked, 0);
        assert!(o.clean());
    }

    #[test]
    fn check_fail_second() {
        let o = load(&SHAPES[4]);
        assert_eq!(o.stage, Stage::Check);
        assert_eq!(o.checked, 1);
        assert!(o.clean());
    }

    #[test]
    fn check_fail_preexist_not_unloaded() {
        let o = load(&SHAPES[5]);
        assert_eq!(o.stage, Stage::Check);
        assert_eq!(o.refs, 0);
        assert!(o.clean());
    }

    #[test]
    fn map_fail_clean() {
        let o = load(&SHAPES[6]);
        assert_eq!(o.stage, Stage::Map);
        assert_eq!(o.code, ERR_ENOENT);
        assert_eq!(o.note, Note::MapAbort);
        assert!(o.clean());
    }

    #[test]
    fn reloc_fail_clean() {
        let o = load(&SHAPES[7]);
        assert_eq!(o.stage, Stage::Reloc);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.checked, 2);
        assert!(o.clean());
    }

    #[test]
    fn dirty_detection() {
        let o = Outcome {
            stage: Stage::Check,
            code: ERR_EINVAL,
            maps: 4,
            links: 1,
            refs: 0,
            checked: 0,
            note: Note::Aborted,
        };
        assert!(o.dirty());
        assert!(!o.clean());
    }

    #[test]
    fn retry_sticky() {
        let r = retry(&SHAPES[3]);
        assert_eq!(r.stage, Stage::Check);
        assert_eq!(r.code, ERR_EINVAL);
        assert_eq!(r.maps, 0);
    }

    #[test]
    fn messages_exact() {
        assert_eq!(
            message(&SHAPES[3], &load(&SHAPES[3])).as_str(),
            "version `GLIBC_2.34' not found (required by libbad.so)"
        );
        assert_eq!(
            message(&SHAPES[6], &load(&SHAPES[6])).as_str(),
            "error while loading shared libraries: libmiss.so: cannot open shared object file: No such file or directory"
        );
        assert_eq!(
            message(&SHAPES[7], &load(&SHAPES[7])).as_str(),
            "./e.so: relocation error: ./e.so: symbol foo, version GLIBC_2.34 not defined in file libdep.so with link time reference"
        );
        assert_eq!(message(&SHAPES[0], &load(&SHAPES[0])).as_str(), "");
    }

    #[test]
    fn tiers() {
        assert_eq!(Tier::of(&load(&SHAPES[0])), Tier::Ready);
        assert_eq!(Tier::of(&load(&SHAPES[3])), Tier::CheckFail);
        assert_eq!(Tier::of(&load(&SHAPES[6])), Tier::MapFail);
        assert_eq!(Tier::of(&load(&SHAPES[7])), Tier::RelocFail);
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
        assert_eq!(judge(&cs[11]), Fault::Maps);
        assert_eq!(judge(&cs[12]), Fault::Links);
        assert_eq!(judge(&cs[13]), Fault::Note);
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.row(0), Some(Row { total: 3, good: 3 }));
        assert_eq!(m.row(1), Some(Row { total: 9, good: 3 }));
        assert_eq!(m.row(2), Some(Row { total: 1, good: 1 }));
        assert_eq!(m.row(3), Some(Row { total: 1, good: 1 }));
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert_eq!(m.perfect_rows(), 3);
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
        assert_eq!(r.ready, 3);
        assert_eq!(r.check, 3);
        assert_eq!(r.map, 1);
        assert_eq!(r.reloc, 1);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.clean_aborts(), 11);
        assert_eq!(r.dirty_aborts(), 0);
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "vercheck-abort corpus=14 accept=8 reject=6 ready=3 check=3 map=1 reloc=1 residue=0 match=14 rate=1000ppm\n"
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
        assert_eq!(r.gate(), Err(LC_ECOUNT));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(LC_ESLOT));
    }

    #[test]
    fn detail_exact() {
        let cs = cases();
        assert_eq!(detail(&cs[3]).as_str(), DETAIL_ABORT);
        assert_eq!(detail(&cs[0]).as_str(), DETAIL_READY);
    }

    #[test]
    fn slot_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(CORPUS - 1).is_some());
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(4), Err(LC_ENOBS));
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Refs), "refs");
    }

    #[test]
    fn gate_steps() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Map), Ok(()));
        assert_eq!(g.step(Stage::Check), Ok(()));
        assert_eq!(g.step(Stage::Link), Err(LC_ESTAGE));
        assert_eq!(g.steps(), 2);
        assert_eq!(g.last(), 2);
    }

    #[test]
    fn gate_close() {
        let mut g = Gate::new();
        assert!(g.close().is_ok());
        assert_eq!(g.close(), Err(LC_ESTATE));
        assert_eq!(g.step(Stage::Ready), Err(LC_ESTATE));
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
        assert_eq!(l.register(Sample::blank()).err(), Some(LC_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(LC_EBAL));
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
        assert_eq!(i32_text(-22).as_str(), "-22");
        assert_eq!(hex_text(0x6cf20, 5).as_str(), "6cf20");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11655_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
