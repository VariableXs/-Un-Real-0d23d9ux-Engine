//! UNX-F11635 dlopen RTLD_NOLOAD 探测判据
//!
//! `RTLD_NOLOAD` 是 `dlopen` 唯一的**只读探针**：它问句柄表"这个对象现在
//! 在不在"，而不是"帮我装进来"。因此它有两条与普通 `dlopen` 完全相反的
//! 纪律：
//!
//! 1. **未载不落盘**：对象不在表里时返回 `NULL`，文案是
//!    `object not open`——不是 `cannot open shared object file`，因为根本没有
//!    去开文件。判据若只看"返回 NULL"，会把它和"文件不存在"混为一谈；
//!    若真去搜索路径开文件，探针本身就变成了副作用。
//! 2. **已载不增计**：对象在表里时返回可用句柄，但**引用计数不变**。于是
//!    "探测 N 次 + 一次 dlclose"仍能把对象卸干净；如果探针偷偷加了计数，
//!    卸载就会永久欠一笔。这条是 `RTLD_NOLOAD` 与"普通二次 dlopen"的分水岭。
//!
//! 与之对照，普通 `dlopen` 的三条路径：
//!
//! - 已载且就绪：句柄复用，引用计数 +1（`load-hit`）。
//! - 未载：装载并把计数置 1（`fresh-load`）。
//! - 表满：拒绝，`cannot allocate memory`（`full`）。
//!
//! 模式位本身也是一道闸：`RTLD_LAZY` 与 `RTLD_NOW` 必须恰好给一个
//! （两个都给不是"更保险"，而是未定义）；未知位直接 `invalid mode for
//! dlopen()`；只有 `RTLD_NOLOAD` 而没有绑定方式同样是坏模式——探针也要
//! 声明它假设的绑定语义。
//!
//! 还有两条状态纪律：
//!
//! - 表里条目的状态是 `Ready` / `Failed`。`Failed` 条目不会被探针"顺手
//!    重试"，也不会被普通 `dlopen` 再次装载——失败是终态，直到显式卸载；
//!    文案复用缺失文案，但阶段是 `state` 而不是 `lookup`。
//! - `RTLD_NOLOAD | RTLD_GLOBAL` 是一次**提升**：对象已在表里，把它的
//!    可见性从局部提到全局，句柄与计数都不变（`probe-global`）。
//!
//! 判据的六个维度：阶段（mode/lookup/state/ready）、errno、句柄、引用计数、
//! 注解（probe-hit/probe-miss/probe-global/load-hit/fresh-load/failed-entry/
//! bad-mode/full）、文案。少"引用计数"一维，探针增计与普通装载就分不开；
//! 少"句柄"一维，命中与非命中只能靠 errno 猜。
//!
//! 判序：状态 → 阶段 → errno → 句柄 → 引用计数 → 注解。
//!
//! 语料 14 槽：8 槽申报正确（命中 2、未命中 3、装载 2、坏档 1），6 槽各挂
//! 一处错误申报。账本 14 格 8 成 6 败，冻结后重放同指纹。
//!
//! 闸门次序：模式 → 查表 → 状态 → 命中分支 → 装载分支 → 计数 → 文案。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfcc0;

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

/// 句柄表容量。
pub const CAP: usize = 6;

// ---------------------------------------------------------------------------
// RTLD 模式位（glibc 取值）
// ---------------------------------------------------------------------------

/// 惰性绑定。
pub const RTLD_LAZY: u32 = 0x0001;
/// 立即绑定。
pub const RTLD_NOW: u32 = 0x0002;
/// 只查表不装载。
pub const RTLD_NOLOAD: u32 = 0x0004;
/// 依赖优先。
pub const RTLD_DEEPBIND: u32 = 0x0008;
/// 全局可见。
pub const RTLD_GLOBAL: u32 = 0x0100;
/// 不随 dlclose 卸载。
pub const RTLD_NODELETE: u32 = 0x1000;

/// 已知位全集。
pub const MODE_KNOWN: u32 =
    RTLD_LAZY | RTLD_NOW | RTLD_NOLOAD | RTLD_DEEPBIND | RTLD_GLOBAL | RTLD_NODELETE;

/// 绑定位。
pub const MODE_BIND: u32 = RTLD_LAZY | RTLD_NOW;

/// 展示次序。
pub const FLAG_ORDER: [(u32, &'static str); 6] = [
    (RTLD_LAZY, "lazy"),
    (RTLD_NOW, "now"),
    (RTLD_NOLOAD, "noload"),
    (RTLD_GLOBAL, "global"),
    (RTLD_DEEPBIND, "deepbind"),
    (RTLD_NODELETE, "nodelete"),
];

// ---------------------------------------------------------------------------
// errno
// ---------------------------------------------------------------------------

/// 不存在。
pub const ERR_ENOENT: i32 = 2;
/// 内存不足。
pub const ERR_ENOMEM: i32 = 12;
/// 参数非法。
pub const ERR_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const EA_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const EA_ECODE: u32 = SEG_BASE + 0x02;
/// 注解申报不符。
pub const EA_ENOTE: u32 = SEG_BASE + 0x03;
/// 引用计数申报不符。
pub const EA_EREFS: u32 = SEG_BASE + 0x04;
/// 状态矛盾。
pub const EA_ESTATE: u32 = SEG_BASE + 0x05;
/// 计数不符。
pub const EA_ECOUNT: u32 = SEG_BASE + 0x06;
/// 观测值不符。
pub const EA_EVALUE: u32 = SEG_BASE + 0x07;
/// 槽位非法。
pub const EA_ESLOT: u32 = SEG_BASE + 0x08;
/// 指纹不符。
pub const EA_ESEAL: u32 = SEG_BASE + 0x09;
/// 无观测。
pub const EA_ENOBS: u32 = SEG_BASE + 0x0a;
/// 矩阵不平。
pub const EA_EMATRIX: u32 = SEG_BASE + 0x0b;
/// 阶段次序倒置。
pub const EA_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const EA_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 模式检查。
pub const OP_MODE: u8 = 0x01;
/// 查表。
pub const OP_LOOKUP: u8 = 0x02;
/// 状态。
pub const OP_STATE: u8 = 0x03;
/// 装载。
pub const OP_LOAD: u8 = 0x04;
/// 引用计数。
pub const OP_REFS: u8 = 0x05;
/// 语料。
pub const OP_CORPUS: u8 = 0x06;

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

/// 模式位文本：`lazy|noload`。
pub fn mode_text(mode: u32) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < FLAG_ORDER.len() {
        if let Some((bit, name)) = FLAG_ORDER.get(i) {
            if (mode & *bit) != 0 {
                if !s.is_empty() {
                    s.push('|');
                }
                s.push_str(name);
            }
        }
        i += 1;
    }
    let unknown = mode & !MODE_KNOWN;
    if unknown != 0 {
        if !s.is_empty() {
            s.push('|');
        }
        s.push_str("0x");
        s.push_str(hex_text(unknown, 4).as_str());
    }
    if s.is_empty() {
        s.push_str("none");
    }
    s
}

// ---------------------------------------------------------------------------
// 状态与条目
// ---------------------------------------------------------------------------

/// 表内条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// 空位。
    Free,
    /// 就绪。
    Ready,
    /// 装载失败（终态）。
    Failed,
}

impl State {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            State::Free => "free",
            State::Ready => "ready",
            State::Failed => "failed",
        }
    }

    /// 是否可用句柄。
    pub fn usable(&self) -> bool {
        match self {
            State::Ready => true,
            _ => false,
        }
    }
}

/// 表内条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 名字。
    pub name: &'static str,
    /// 状态。
    pub state: State,
    /// 引用计数。
    pub refs: u16,
    /// RTLD_NODELETE。
    pub nodelete: bool,
}

impl Entry {
    /// 空条目。
    pub const fn blank() -> Entry {
        Entry { name: "", state: State::Free, refs: 0, nodelete: false }
    }

    /// 就绪条目。
    pub const fn ready(name: &'static str, refs: u16) -> Entry {
        Entry { name, state: State::Ready, refs, nodelete: false }
    }

    /// 失败条目。
    pub const fn failed(name: &'static str) -> Entry {
        Entry { name, state: State::Failed, refs: 0, nodelete: false }
    }
}

// ---------------------------------------------------------------------------
// 句柄表
// ---------------------------------------------------------------------------

/// 句柄表。
#[derive(Clone, Copy, Debug)]
pub struct Store {
    entries: [Entry; CAP],
    len: usize,
}

impl Store {
    /// 空表。
    pub const fn new() -> Store {
        Store { entries: [Entry::blank(); CAP], len: 0 }
    }

    /// 表长（含失败条目）。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取条目。
    pub fn get(&self, i: usize) -> Option<Entry> {
        if i >= self.len {
            return None;
        }
        self.entries.get(i).copied()
    }

    /// 改名查找。
    pub fn find(&self, name: &str) -> Option<usize> {
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

    /// 取名字。
    pub fn name_at(&self, i: usize) -> Option<&'static str> {
        self.get(i).map(|e| e.name)
    }

    /// 取计数。
    pub fn refs(&self, i: usize) -> u16 {
        match self.get(i) {
            Some(e) => e.refs,
            None => 0,
        }
    }

    /// 加一次引用。
    pub fn add_ref(&mut self, i: usize) -> u16 {
        match self.entries.get_mut(i) {
            Some(e) => {
                e.refs = e.refs.saturating_add(1);
                e.refs
            }
            None => 0,
        }
    }

    /// 减一次引用；归零时置空位（由卸载卡对照）。
    pub fn drop_ref(&mut self, i: usize) -> u16 {
        match self.entries.get_mut(i) {
            Some(e) => {
                if e.refs > 0 {
                    e.refs -= 1;
                }
                e.refs
            }
            None => 0,
        }
    }

    /// 追加条目；满表给 `ERR_ENOMEM`。
    pub fn push(&mut self, e: Entry) -> Result<usize, u32> {
        if self.len >= CAP {
            return Err(ERR_ENOMEM as u32);
        }
        let i = self.len;
        if let Some(dst) = self.entries.get_mut(i) {
            *dst = e;
        }
        self.len += 1;
        Ok(i)
    }

    /// 就绪条目数。
    pub fn ready_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                if e.state == State::Ready {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败条目数。
    pub fn failed_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                if e.state == State::Failed {
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
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                h = fold_str(h, e.name);
                h = fold_word(h, e.state as u64);
                h = fold_word(h, e.refs as u64);
                h = fold_word(h, e.nodelete as u64);
            }
            i += 1;
        }
        h
    }
}

/// 预置：空表。
fn store_empty() -> Store {
    Store::new()
}

/// 预置：一个就绪条目（计数 1）。
fn store_ready(name: &'static str) -> Store {
    let mut s = Store::new();
    let _ = s.push(Entry::ready(name, 1));
    s
}

/// 预置：一个就绪条目（计数 2）。
fn store_ready2(name: &'static str) -> Store {
    let mut s = Store::new();
    let _ = s.push(Entry::ready(name, 2));
    s
}

/// 预置：一个失败条目。
fn store_failed(name: &'static str) -> Store {
    let mut s = Store::new();
    let _ = s.push(Entry::failed(name));
    s
}

/// 预置：一个就绪（计数 2）+ 一个失败。
fn store_mixed() -> Store {
    let mut s = Store::new();
    let _ = s.push(Entry::ready("libc.so.6", 2));
    let _ = s.push(Entry::failed("./broken.so"));
    s
}

/// 预置：满表（6 个就绪）。
fn store_full() -> Store {
    let mut s = Store::new();
    let _ = s.push(Entry::ready("liba.so", 1));
    let _ = s.push(Entry::ready("libb.so", 1));
    let _ = s.push(Entry::ready("libc.so", 1));
    let _ = s.push(Entry::ready("libd.so", 1));
    let _ = s.push(Entry::ready("libe.so", 1));
    let _ = s.push(Entry::ready("libf.so", 1));
    s
}

/// 预置表构造器。
pub type Seed = fn() -> Store;

/// 空表种子。
pub const SEED_EMPTY: Seed = store_empty;

/// 单就绪种子名（用函数包装以保留 `fn` 类型）。
fn seed_c() -> Store {
    store_ready("libc.so.6")
}

/// 双就绪种子（首项为 `libc.so.6`）。
fn seed_c2() -> Store {
    store_ready2("libc.so.6")
}

/// 失败种子。
fn seed_broken() -> Store {
    store_failed("./broken.so")
}

// ---------------------------------------------------------------------------
// 阶段与注解
// ---------------------------------------------------------------------------

/// 探测阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 模式。
    Mode,
    /// 查表。
    Lookup,
    /// 状态。
    State,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Mode => 0,
            Stage::Lookup => 1,
            Stage::State => 2,
            Stage::Ready => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Mode => "mode",
            Stage::Lookup => "lookup",
            Stage::State => "state",
            Stage::Ready => "ready",
        }
    }
}

/// 本次调用的注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 探针命中。
    ProbeHit,
    /// 探针命中并提升可见性。
    ProbeGlobal,
    /// 探针未命中。
    ProbeMiss,
    /// 普通调用命中已载条目。
    LoadHit,
    /// 普通调用新装载。
    FreshLoad,
    /// 条目处于失败终态。
    FailedEntry,
    /// 模式非法。
    BadMode,
    /// 表满。
    Full,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::ProbeHit => "probe-hit",
            Note::ProbeGlobal => "probe-global",
            Note::ProbeMiss => "probe-miss",
            Note::LoadHit => "load-hit",
            Note::FreshLoad => "fresh-load",
            Note::FailedEntry => "failed-entry",
            Note::BadMode => "bad-mode",
            Note::Full => "full",
        }
    }

    /// 是否只读探针。
    pub fn probe(&self) -> bool {
        match self {
            Note::ProbeHit | Note::ProbeGlobal | Note::ProbeMiss => true,
            _ => false,
        }
    }

    /// 是否成功路径。
    pub fn success(&self) -> bool {
        match self {
            Note::ProbeHit
            | Note::ProbeGlobal
            | Note::LoadHit
            | Note::FreshLoad => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次调用的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 句柄（0 为 NULL）。
    pub handle: u32,
    /// 阶段。
    pub stage: Stage,
    /// errno（成功为 0）。
    pub code: i32,
    /// 目标条目调用后的引用计数。
    pub refs: u16,
    /// 注解。
    pub note: Note,
}

impl Outcome {
    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.handle != 0 && self.code == 0
    }

    /// 是否 NULL。
    pub fn null(&self) -> bool {
        self.handle == 0
    }

    /// 是否只读探针。
    pub fn probe(&self) -> bool {
        self.note.probe()
    }
}

/// 模式合法性。
///
/// 三条：未知位、绑定位必须恰好一个（LAZY/NOW 二选一）、`NOLOAD` 不能
/// 单独出现（探针也要声明绑定方式）。
pub fn bad_mode(mode: u32) -> bool {
    if (mode & !MODE_KNOWN) != 0 {
        return true;
    }
    let bind = mode & MODE_BIND;
    if bind != RTLD_LAZY && bind != RTLD_NOW {
        return true;
    }
    false
}

/// `dlopen` 的确定性模型。
///
/// 探针分支（`RTLD_NOLOAD`）只在查表结果上分叉：命中返回句柄且**不动计数**，
/// 未命中返回 NULL 且**不建条目**。普通分支命中则计数 +1，未命中则追加条目
/// 并把计数置 1；表满一律 `ERR_ENOMEM`。
pub fn dlopen(store: &mut Store, name: &'static str, mode: u32) -> Outcome {
    if bad_mode(mode) {
        return Outcome {
            handle: 0,
            stage: Stage::Mode,
            code: ERR_EINVAL,
            refs: 0,
            note: Note::BadMode,
        };
    }
    let noload = (mode & RTLD_NOLOAD) != 0;
    if let Some(i) = store.find(name) {
        let state = match store.get(i) {
            Some(e) => e.state,
            None => State::Free,
        };
        match state {
            State::Ready => {
                let refs = store.refs(i);
                if noload {
                    let note = if (mode & RTLD_GLOBAL) != 0 {
                        Note::ProbeGlobal
                    } else {
                        Note::ProbeHit
                    };
                    return Outcome {
                        handle: i as u32 + 1,
                        stage: Stage::Ready,
                        code: 0,
                        refs,
                        note,
                    };
                }
                let after = store.add_ref(i);
                Outcome {
                    handle: i as u32 + 1,
                    stage: Stage::Ready,
                    code: 0,
                    refs: after,
                    note: Note::LoadHit,
                }
            }
            State::Failed => Outcome {
                handle: 0,
                stage: Stage::State,
                code: ERR_ENOENT,
                refs: store.refs(i),
                note: Note::FailedEntry,
            },
            State::Free => Outcome {
                handle: 0,
                stage: Stage::Lookup,
                code: ERR_ENOENT,
                refs: 0,
                note: Note::ProbeMiss,
            },
        }
    } else if noload {
        Outcome {
            handle: 0,
            stage: Stage::Lookup,
            code: ERR_ENOENT,
            refs: 0,
            note: Note::ProbeMiss,
        }
    } else {
        let entry = Entry {
            name,
            state: State::Ready,
            refs: 1,
            nodelete: (mode & RTLD_NODELETE) != 0,
        };
        match store.push(entry) {
            Ok(i) => Outcome {
                handle: i as u32 + 1,
                stage: Stage::Ready,
                code: 0,
                refs: 1,
                note: Note::FreshLoad,
            },
            Err(c) => Outcome {
                handle: 0,
                stage: Stage::State,
                code: c as i32,
                refs: 0,
                note: Note::Full,
            },
        }
    }
}

/// 已载探测（`RTLD_LAZY | RTLD_NOLOAD`）。
pub fn probe(store: &mut Store, name: &'static str) -> Outcome {
    dlopen(store, name, RTLD_LAZY | RTLD_NOLOAD)
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// `<name>: <msg>`。
pub fn named(name: &str, msg: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(msg);
    s
}

/// 按注解生成错误文案。
pub fn message_for(name: &str, note: Note) -> String {
    match note {
        Note::ProbeMiss => String::from("object not open"),
        Note::BadMode => String::from("invalid mode for dlopen()"),
        Note::FailedEntry => named(name, "cannot open shared object file: No such file or directory"),
        Note::Full => String::from("cannot allocate memory"),
        _ => String::new(),
    }
}

/// 观测文案。
pub fn message(o: &Outcome, name: &str) -> String {
    message_for(name, o.note)
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 判据档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 命中档。
    Hit,
    /// 未命中档。
    Miss,
    /// 装载档。
    Load,
    /// 坏档。
    Bad,
}

impl Tier {
    /// 由注解归位。
    pub fn of(note: Note) -> Tier {
        match note {
            Note::ProbeHit | Note::ProbeGlobal => Tier::Hit,
            Note::ProbeMiss | Note::FailedEntry => Tier::Miss,
            Note::LoadHit | Note::FreshLoad => Tier::Load,
            Note::BadMode | Note::Full => Tier::Bad,
        }
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Hit => 0,
            Tier::Miss => 1,
            Tier::Load => 2,
            Tier::Bad => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Hit => "hit",
            Tier::Miss => "miss",
            Tier::Load => "load",
            Tier::Bad => "bad",
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
    /// 句柄错。
    Handle,
    /// 引用计数错。
    Refs,
    /// 注解错。
    Note,
    /// 成功/失败互报。
    Estate,
    /// 计数错。
    Count,
    /// 观测值错。
    Value,
    /// 槽位错。
    Slot,
    /// 指纹错。
    Seal,
    /// 无观测。
    Nobs,
    /// 矩阵不平。
    Matrix,
    /// 阶段次序倒置。
    Order,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 缺陷码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Stage => EA_ESTAGE,
            Fault::Code => EA_ECODE,
            Fault::Handle => EA_EVALUE,
            Fault::Refs => EA_EREFS,
            Fault::Note => EA_ENOTE,
            Fault::Estate => EA_ESTATE,
            Fault::Count => EA_ECOUNT,
            Fault::Value => EA_EVALUE,
            Fault::Slot => EA_ESLOT,
            Fault::Seal => EA_ESEAL,
            Fault::Nobs => EA_ENOBS,
            Fault::Matrix => EA_EMATRIX,
            Fault::Order => EA_EORDER,
            Fault::Bal => EA_EBAL,
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
        Fault::Handle => "handle",
        Fault::Refs => "refs",
        Fault::Note => "note",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Value => "value",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Order => "order",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 申报与语料
// ---------------------------------------------------------------------------

/// 一条申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报阶段。
    pub stage: Stage,
    /// 申报 errno。
    pub code: i32,
    /// 申报句柄。
    pub handle: u32,
    /// 申报引用计数。
    pub refs: u16,
    /// 申报注解。
    pub note: Note,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Mode, code: 0, handle: 0, refs: 0, note: Note::BadMode }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.handle != 0 && self.code == 0
    }
}

/// 一个语料形状。
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 请求名。
    pub name: &'static str,
    /// 模式位。
    pub mode: u32,
    /// 预置句柄表。
    pub seed: Seed,
}

/// 形状等值：只比标签、请求名与模式，函数指针不参与比较。
impl PartialEq for Shape {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag && self.name == other.name && self.mode == other.mode
    }
}

impl Eq for Shape {}

impl Shape {
    /// 空形状。
    pub const fn blank() -> Shape {
        Shape { tag: "", name: "", mode: 0, seed: SEED_EMPTY }
    }

    /// 实际结果（每次用新表）。
    pub fn actual(&self) -> Outcome {
        let mut s = (self.seed)();
        dlopen(&mut s, self.name, self.mode)
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

    /// 请求名。
    pub fn name(&self) -> &'static str {
        self.shape.name
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

/// 14 个形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "probe-hit",
        name: "libc.so.6",
        mode: RTLD_LAZY | RTLD_NOLOAD,
        seed: seed_c,
    },
    Shape {
        tag: "probe-miss",
        name: "libm.so.6",
        mode: RTLD_LAZY | RTLD_NOLOAD,
        seed: SEED_EMPTY,
    },
    Shape {
        tag: "probe-miss-other",
        name: "libz.so",
        mode: RTLD_NOW | RTLD_NOLOAD,
        seed: seed_c,
    },
    Shape {
        tag: "probe-failed",
        name: "./broken.so",
        mode: RTLD_LAZY | RTLD_NOLOAD,
        seed: seed_broken,
    },
    Shape {
        tag: "noload-no-bind",
        name: "libc.so.6",
        mode: RTLD_NOLOAD,
        seed: seed_c,
    },
    Shape {
        tag: "load-hit",
        name: "libc.so.6",
        mode: RTLD_NOW,
        seed: seed_c,
    },
    Shape {
        tag: "fresh-load",
        name: "libx.so",
        mode: RTLD_LAZY,
        seed: SEED_EMPTY,
    },
    Shape {
        tag: "probe-promote",
        name: "libc.so.6",
        mode: RTLD_LAZY | RTLD_NOLOAD | RTLD_GLOBAL,
        seed: seed_c,
    },
    Shape {
        tag: "miss-as-load",
        name: "libm.so.6",
        mode: RTLD_LAZY | RTLD_NOLOAD,
        seed: SEED_EMPTY,
    },
    Shape {
        tag: "probe-refs",
        name: "libc.so.6",
        mode: RTLD_LAZY | RTLD_NOLOAD,
        seed: seed_c2,
    },
    Shape {
        tag: "badmode-as-miss",
        name: "libc.so.6",
        mode: RTLD_NOLOAD,
        seed: seed_c,
    },
    Shape {
        tag: "unknownflag-as-enoent",
        name: "libc.so.6",
        mode: 0x2000,
        seed: seed_c,
    },
    Shape {
        tag: "failed-as-ready",
        name: "./broken.so",
        mode: RTLD_LAZY | RTLD_NOLOAD,
        seed: seed_broken,
    },
    Shape {
        tag: "full-as-miss",
        name: "libq.so",
        mode: RTLD_NOW,
        seed: store_full,
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 1, note: Note::ProbeHit },
    Claim { stage: Stage::Lookup, code: ERR_ENOENT, handle: 0, refs: 0, note: Note::ProbeMiss },
    Claim { stage: Stage::Lookup, code: ERR_ENOENT, handle: 0, refs: 0, note: Note::ProbeMiss },
    Claim { stage: Stage::State, code: ERR_ENOENT, handle: 0, refs: 0, note: Note::FailedEntry },
    Claim { stage: Stage::Mode, code: ERR_EINVAL, handle: 0, refs: 0, note: Note::BadMode },
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 2, note: Note::LoadHit },
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 1, note: Note::FreshLoad },
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 1, note: Note::ProbeGlobal },
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 1, note: Note::FreshLoad },
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 3, note: Note::ProbeHit },
    Claim { stage: Stage::Lookup, code: ERR_ENOENT, handle: 0, refs: 0, note: Note::ProbeMiss },
    Claim { stage: Stage::Mode, code: ERR_ENOENT, handle: 0, refs: 0, note: Note::BadMode },
    Claim { stage: Stage::Ready, code: 0, handle: 1, refs: 0, note: Note::ProbeHit },
    Claim { stage: Stage::State, code: ERR_ENOENT, handle: 0, refs: 0, note: Note::Full },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Refs, Fault::Stage, Fault::Code, Fault::Estate, Fault::Code];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] =
    [EA_ESTATE, EA_EREFS, EA_ESTAGE, EA_ECODE, EA_ESTATE, EA_ECODE];

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
        Err(EA_ENOBS)
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

/// 对判：状态 → 阶段 → errno → 句柄 → 计数 → 注解。
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
    if claim.note != a.note {
        return Fault::Note;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 释义
// ---------------------------------------------------------------------------

/// 单行释义：
/// `<name> mode=<m> stage=<s> handle=<h> refs=<n> note=<n>`。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(c.shape.name);
    s.push_str(" mode=");
    s.push_str(mode_text(c.shape.mode).as_str());
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" handle=");
    s.push_str(u32_text(a.handle).as_str());
    s.push_str(" refs=");
    s.push_str(dec_text(a.refs as u32).as_str());
    s.push_str(" note=");
    s.push_str(a.note.name());
    s.push_str("\n");
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
    /// 行（hit/miss/load/bad）。
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
    /// 命中档正确数。
    pub hit: u32,
    /// 未命中档正确数。
    pub miss: u32,
    /// 装载档正确数。
    pub load: u32,
    /// 坏档正确数。
    pub bad: u32,
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
            Err(EA_ECOUNT)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("noload corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" hit=");
        s.push_str(u32_text(self.hit).as_str());
        s.push_str(" miss=");
        s.push_str(u32_text(self.miss).as_str());
        s.push_str(" load=");
        s.push_str(u32_text(self.load).as_str());
        s.push_str(" bad=");
        s.push_str(u32_text(self.bad).as_str());
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
            return Err(EA_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut hit = 0u32;
    let mut miss = 0u32;
    let mut load = 0u32;
    let mut bad = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(EA_ENOBS),
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
            None => return Err(EA_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(a.note) {
                Tier::Hit => hit += 1,
                Tier::Miss => miss += 1,
                Tier::Load => load += 1,
                Tier::Bad => bad += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, hit, miss, load, bad, matched })
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
            return Err(EA_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(EA_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(EA_ESTATE);
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
            return Err(EA_ESLOT);
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
            return Err(EA_EBAL);
        }
        const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 3, 2, 1, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EA_EBAL),
            };
            if s.slot as usize != i {
                return Err(EA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EA_EVALUE);
                        }
                    }
                    None => return Err(EA_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EA_EVALUE);
                        }
                    }
                    None => return Err(EA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(EA_EBAL);
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
            return Err(EA_EBAL);
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
            Err(EA_ESEAL)
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
    let _ = l.register(count_cell(3, OP_LOOKUP, r.map(|x| x.hit)));
    let _ = l.register(count_cell(4, OP_LOOKUP, r.map(|x| x.miss)));
    let _ = l.register(count_cell(5, OP_LOAD, r.map(|x| x.load)));
    let _ = l.register(count_cell(6, OP_MODE, r.map(|x| x.bad)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REFS, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_STATE,
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
    out[0] = name_ok() && mode_ok() && store_ok();
    out[1] = pipe_ok() && probe_ok() && judge_ok();
    out[2] = matrix_ok() && corpus_ok() && reds_ok() && message_ok();
    out[3] = slots_ok()
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
    Stage::Lookup,
    Stage::Lookup,
    Stage::State,
    Stage::Mode,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Lookup,
    Stage::Ready,
    Stage::Mode,
    Stage::Mode,
    Stage::State,
    Stage::State,
];

/// 第 i 槽的期望 errno。
const WANT_CODE: [i32; CORPUS] = [0, 2, 2, 2, 22, 0, 0, 0, 2, 0, 22, 22, 2, 12];

/// 第 i 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::ProbeHit,
    Note::ProbeMiss,
    Note::ProbeMiss,
    Note::FailedEntry,
    Note::BadMode,
    Note::LoadHit,
    Note::FreshLoad,
    Note::ProbeGlobal,
    Note::ProbeMiss,
    Note::ProbeHit,
    Note::BadMode,
    Note::BadMode,
    Note::FailedEntry,
    Note::Full,
];

/// 名称与段基址。
fn name_ok() -> bool {
    let codes = [
        EA_ESTAGE, EA_ECODE, EA_ENOTE, EA_EREFS, EA_ESTATE, EA_ECOUNT, EA_EVALUE, EA_ESLOT,
        EA_ESEAL, EA_ENOBS, EA_EMATRIX, EA_EORDER, EA_EBAL,
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
    let a = SEG_BASE == 0xfcc0;
    let b = EA_ESTAGE == SEG_BASE + 0x01 && EA_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = CAP == 6 && TIER_COUNT == 4;
    let e = RTLD_LAZY == 1 && RTLD_NOW == 2 && RTLD_NOLOAD == 4;
    let f = RTLD_GLOBAL == 0x100 && RTLD_NODELETE == 0x1000 && MODE_BIND == 3;
    a && b && c && d && e && f
}

/// 模式位与文本。
fn mode_ok() -> bool {
    let a = !bad_mode(RTLD_LAZY) && !bad_mode(RTLD_NOW);
    let b = !bad_mode(RTLD_LAZY | RTLD_NOLOAD) && !bad_mode(RTLD_NOW | RTLD_NOLOAD);
    let c = bad_mode(RTLD_NOLOAD) && bad_mode(0) && bad_mode(0x2000);
    let d = bad_mode(RTLD_LAZY | RTLD_NOW);
    let e = !bad_mode(RTLD_NOW | RTLD_GLOBAL) && !bad_mode(RTLD_LAZY | RTLD_NODELETE);
    let f = mode_text(RTLD_LAZY | RTLD_NOLOAD).as_str() == "lazy|noload";
    let g = mode_text(RTLD_NOW).as_str() == "now" && mode_text(0).as_str() == "none";
    let h = mode_text(RTLD_NOLOAD).as_str() == "noload";
    let i = mode_text(RTLD_LAZY | RTLD_NOLOAD | RTLD_GLOBAL).as_str() == "lazy|noload|global";
    let j = mode_text(0x2000).as_str() == "0x2000";
    let k = mode_text(RTLD_NOW | RTLD_NODELETE).as_str() == "now|nodelete";
    a && b && c && d && e && f && g && h && i && j && k
}

/// 句柄表。
fn store_ok() -> bool {
    let mut s = Store::new();
    let a = s.is_empty() && s.len() == 0 && s.ready_count() == 0;
    let b = s.push(Entry::ready("libc.so.6", 1)) == Ok(0);
    let c = s.push(Entry::failed("./broken.so")) == Ok(1);
    let d = s.len() == 2 && s.ready_count() == 1 && s.failed_count() == 1;
    let e = s.find("libc.so.6") == Some(0) && s.find("./broken.so") == Some(1);
    let f = s.find("libq.so").is_none() && s.get(2).is_none();
    let g = s.refs(0) == 1 && s.add_ref(0) == 2 && s.refs(0) == 2;
    let h = s.drop_ref(0) == 1 && s.drop_ref(1) == 0;
    let i = s.name_at(0) == Some("libc.so.6") && s.name_at(2).is_none();
    let full = store_full();
    let mut f2 = full;
    let j = f2.len() == CAP && f2.push(Entry::ready("libg.so", 1)) == Err(ERR_ENOMEM as u32);
    let k = store_empty().seal() == Store::new().seal();
    let l = store_ready("libc.so.6").seal() != store_ready2("libc.so.6").seal();
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 状态机逐槽。
fn pipe_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let st = match WANT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let code = match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let note = match WANT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != st || a.code != code || a.note != note {
            return false;
        }
        i += 1;
    }
    true
}

/// 探针不落盘、不增计。
fn probe_ok() -> bool {
    let mut s = store_ready("libc.so.6");
    let a = probe(&mut s, "libc.so.6");
    let b = probe(&mut s, "libc.so.6");
    let c = a.handle == 1 && b.handle == 1;
    let d = a.refs == 1 && b.refs == 1 && s.refs(0) == 1;
    let mut e = Store::new();
    let miss = probe(&mut e, "libz.so");
    let f = miss.null() && miss.stage == Stage::Lookup && miss.note == Note::ProbeMiss;
    let g = e.is_empty() && e.find("libz.so").is_none();
    let mut h = store_ready("libc.so.6");
    let l1 = dlopen(&mut h, "libc.so.6", RTLD_NOW);
    let l2 = dlopen(&mut h, "libc.so.6", RTLD_NOW);
    let i = l1.refs == 2 && l2.refs == 3 && l1.note == Note::LoadHit && l2.note == Note::LoadHit;
    let mut f2 = store_empty();
    let fresh = dlopen(&mut f2, "libx.so", RTLD_LAZY);
    let j = fresh.handle == 1 && fresh.refs == 1 && f2.len() == 1 && f2.refs(0) == 1;
    let k = probe(&mut f2, "libx.so").refs == 1;
    let mut m = store_mixed();
    let fm = probe(&mut m, "./broken.so");
    let l = fm.null() && fm.stage == Stage::State && fm.note == Note::FailedEntry;
    let n = probe(&mut m, "libc.so.6").handle == 1; 
    a.ok() && b.ok() && c && d && f && g && i && j && k && l && n
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
    m.row(0) == Some(Row { total: 3, good: 2 })
        && m.row(1) == Some(Row { total: 5, good: 3 })
        && m.row(2) == Some(Row { total: 2, good: 2 })
        && m.row(3) == Some(Row { total: 4, good: 1 })
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
        if a.ok() != a.note.success() {
            return false;
        }
        if a.null() && a.handle != 0 {
            return false;
        }
        if a.probe() && a.refs != c.actual().refs {
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
    let b = r.hit == 2 && r.miss == 3 && r.load == 2 && r.bad == 1;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = tags_unique() && notes_ok() && table_ok();
    a && b && c && d
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(EA_ECOUNT) && !r.green();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(EA_ESLOT);
    a && b && c
}

/// 文案逐字。
fn message_ok() -> bool {
    let miss = message_for("libm.so.6", Note::ProbeMiss);
    let a = miss.as_str() == "object not open";
    let b = message_for("libc.so.6", Note::BadMode).as_str() == "invalid mode for dlopen()";
    let c = message_for("./broken.so", Note::FailedEntry).as_str()
        == "./broken.so: cannot open shared object file: No such file or directory";
    let d = message_for("libq.so", Note::Full).as_str() == "cannot allocate memory";
    let e = message_for("libc.so.6", Note::ProbeHit).is_empty();
    let f = message_for("libc.so.6", Note::LoadHit).is_empty();
    let g = message(&probe(&mut Store::new(), "libz.so"), "libz.so").as_str() == "object not open";
    a && b && c && d && e && f && g
}

/// 槽号与档位。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => c.tag() == "probe-hit" && c.name() == "libc.so.6",
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "full-as-miss",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(EA_ENOBS);
    let e = Stage::Mode.order() == 0
        && Stage::Lookup.order() == 1
        && Stage::State.order() == 2
        && Stage::Ready.order() == 3;
    let f = Stage::Lookup.name() == "lookup" && Stage::State.name() == "state";
    let g = Tier::Hit.index() == 0
        && Tier::Miss.index() == 1
        && Tier::Load.index() == 2
        && Tier::Bad.index() == 3;
    let h = Tier::Hit.name() == "hit"
        && Tier::Miss.name() == "miss"
        && Tier::Load.name() == "load"
        && Tier::Bad.name() == "bad";
    let i = State::Free.name() == "free" && State::Ready.name() == "ready";
    let j = State::Ready.usable() && !State::Failed.usable() && !State::Free.usable();
    let k = Note::ProbeGlobal.probe() && Note::ProbeMiss.probe() && !Note::FreshLoad.probe();
    let l = Note::LoadHit.success() && !Note::Full.success() && !Note::FailedEntry.success();
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 释义逐字。
const DETAIL_HIT: &str = "libc.so.6 mode=lazy|noload stage=ready handle=1 refs=1 note=probe-hit\n";
/// 未命中释义。
const DETAIL_MISS: &str = "libm.so.6 mode=lazy|noload stage=lookup handle=0 refs=0 note=probe-miss\n";
/// 坏模式释义。
const DETAIL_BAD: &str = "libc.so.6 mode=noload stage=mode handle=0 refs=0 note=bad-mode\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_HIT,
        None => false,
    };
    let b = match cs.get(1) {
        Some(c) => detail(c).as_str() == DETAIL_MISS,
        None => false,
    };
    let d = match cs.get(4) {
        Some(c) => detail(c).as_str() == DETAIL_BAD,
        None => false,
    };
    let e = match cs.get(5) {
        Some(c) => {
            let o = c.actual();
            o.refs == 2 && o.handle == 1 && o.stage == Stage::Ready
        }
        None => false,
    };
    a && b && d && e
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Lookup) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Mode) == Err(EA_EORDER);
    let d = g.step(Stage::Ready) == Ok(()) && g.last() == 3;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(EA_ESTATE);
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
pub fn run_unx11635_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11635_noload_probe");

    s.add("NL01-名称与段基址", name_ok(), "static");

    s.add("NL02-模式位与文本", mode_ok(), "static");

    s.add("NL03-句柄表", store_ok(), "static");

    s.add("NL04-状态机逐槽", pipe_ok(), "static");

    s.add("NL05-探针无副作用", probe_ok(), "static");

    s.add("NL06-申报对判", judge_ok(), "static");

    s.add("NL07-矩阵统计", matrix_ok(), "static");

    s.add("NL08-语料与渲染", corpus_ok(), "static");

    s.add("NL09-红档显式", reds_ok(), "static");

    s.add("NL10-文案逐字", message_ok(), "static");

    s.add("NL11-槽号与档位", slots_ok(), "static");

    s.add("NL12-释义与账本", detail_ok() && ledger_ok() && gate_ok(), "static");

    s.add(
        "NL13-冻结重放四注入",
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

    /// 测试用最小闭包断言：失败即 panic。
    fn fail() -> ! {
        panic!("unx11635 test fail")
    }

    #[test]
    fn flag_consts() {
        assert_eq!(RTLD_LAZY, 1);
        assert_eq!(RTLD_NOW, 2);
        assert_eq!(RTLD_NOLOAD, 4);
        assert_eq!(RTLD_GLOBAL, 0x100);
        assert_eq!(RTLD_NODELETE, 0x1000);
        assert_eq!(MODE_BIND, 3);
    }

    #[test]
    fn bad_mode_rules() {
        assert!(!bad_mode(RTLD_LAZY));
        assert!(!bad_mode(RTLD_NOW));
        assert!(!bad_mode(RTLD_LAZY | RTLD_NOLOAD));
        assert!(bad_mode(RTLD_NOLOAD));
        assert!(bad_mode(0));
        assert!(bad_mode(RTLD_LAZY | RTLD_NOW));
        assert!(bad_mode(0x2000));
    }

    #[test]
    fn mode_texts() {
        assert_eq!(mode_text(RTLD_LAZY).as_str(), "lazy");
        assert_eq!(mode_text(RTLD_NOW | RTLD_NOLOAD).as_str(), "now|noload");
        assert_eq!(mode_text(0).as_str(), "none");
        assert_eq!(mode_text(0x2000).as_str(), "0x2000");
    }

    #[test]
    fn store_push_find() {
        let mut s = Store::new();
        assert!(s.is_empty());
        assert_eq!(s.push(Entry::ready("libc.so.6", 1)), Ok(0));
        assert_eq!(s.push(Entry::failed("./broken.so")), Ok(1));
        assert_eq!(s.find("libc.so.6"), Some(0));
        assert_eq!(s.find("./broken.so"), Some(1));
        assert_eq!(s.find("libq.so"), None);
        assert_eq!(s.ready_count(), 1);
        assert_eq!(s.failed_count(), 1);
        assert_eq!(s.name_at(0), Some("libc.so.6"));
    }

    #[test]
    fn store_full_cap() {
        let mut s = store_full();
        assert_eq!(s.len(), CAP);
        assert_eq!(s.push(Entry::ready("libg.so", 1)), Err(ERR_ENOMEM as u32));
        assert_eq!(s.len(), CAP);
    }

    #[test]
    fn store_refs() {
        let mut s = store_ready("libc.so.6");
        assert_eq!(s.refs(0), 1);
        assert_eq!(s.add_ref(0), 2);
        assert_eq!(s.drop_ref(0), 1);
        assert_eq!(s.drop_ref(0), 0);
        assert_eq!(s.drop_ref(0), 0);
    }

    #[test]
    fn store_entries() {
        assert_eq!(Entry::blank().state, State::Free);
        assert_eq!(Entry::ready("a", 1).refs, 1);
        assert_eq!(Entry::failed("b").state, State::Failed);
        assert!(Entry::ready("a", 1).state.usable());
    }

    #[test]
    fn probe_hit() {
        let mut s = store_ready("libc.so.6");
        let o = probe(&mut s, "libc.so.6");
        assert_eq!(o.handle, 1);
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.note, Note::ProbeHit);
        assert_eq!(o.refs, 1);
        assert!(o.ok());
        assert!(o.probe());
    }

    #[test]
    fn probe_hit_twice() {
        let mut s = store_ready("libc.so.6");
        let a = probe(&mut s, "libc.so.6");
        let b = probe(&mut s, "libc.so.6");
        assert_eq!(a.refs, 1);
        assert_eq!(b.refs, 1);
        assert_eq!(s.refs(0), 1);
    }

    #[test]
    fn probe_miss_no_entry() {
        let mut s = Store::new();
        let o = probe(&mut s, "libz.so");
        assert!(o.null());
        assert_eq!(o.stage, Stage::Lookup);
        assert_eq!(o.code, ERR_ENOENT);
        assert_eq!(o.note, Note::ProbeMiss);
        assert!(s.is_empty());
    }

    #[test]
    fn probe_failed_entry() {
        let mut s = store_failed("./broken.so");
        let o = probe(&mut s, "./broken.so");
        assert!(o.null());
        assert_eq!(o.stage, Stage::State);
        assert_eq!(o.code, ERR_ENOENT);
        assert_eq!(o.note, Note::FailedEntry);
    }

    #[test]
    fn probe_promote() {
        let mut s = store_ready("libc.so.6");
        let o = dlopen(&mut s, "libc.so.6", RTLD_LAZY | RTLD_NOLOAD | RTLD_GLOBAL);
        assert_eq!(o.note, Note::ProbeGlobal);
        assert_eq!(o.handle, 1);
        assert_eq!(o.refs, 1);
    }

    #[test]
    fn probe_bad_mode() {
        let mut s = store_ready("libc.so.6");
        let o = dlopen(&mut s, "libc.so.6", RTLD_NOLOAD);
        assert_eq!(o.stage, Stage::Mode);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.note, Note::BadMode);
        assert!(o.null());
    }

    #[test]
    fn load_hit_increments() {
        let mut s = store_ready("libc.so.6");
        let a = dlopen(&mut s, "libc.so.6", RTLD_NOW);
        assert_eq!(a.note, Note::LoadHit);
        assert_eq!(a.refs, 2);
        let b = dlopen(&mut s, "libc.so.6", RTLD_NOW);
        assert_eq!(b.refs, 3);
        assert_eq!(s.refs(0), 3);
    }

    #[test]
    fn fresh_load() {
        let mut s = Store::new();
        let o = dlopen(&mut s, "libx.so", RTLD_LAZY);
        assert_eq!(o.handle, 1);
        assert_eq!(o.refs, 1);
        assert_eq!(o.note, Note::FreshLoad);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn fresh_load_nodelete() {
        let mut s = Store::new();
        let o = dlopen(&mut s, "libx.so", RTLD_LAZY | RTLD_NODELETE);
        assert!(o.ok());
        assert!(s.get(0).map(|e| e.nodelete).unwrap_or(false));
    }

    #[test]
    fn fresh_load_full() {
        let mut s = store_full();
        let o = dlopen(&mut s, "libq.so", RTLD_NOW);
        assert!(o.null());
        assert_eq!(o.stage, Stage::State);
        assert_eq!(o.code, ERR_ENOMEM);
        assert_eq!(o.note, Note::Full);
    }

    #[test]
    fn shape_table() {
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert!(tags_unique());
        assert_eq!(SHAPES[0].tag, "probe-hit");
        assert_eq!(SHAPES[13].tag, "full-as-miss");
    }

    #[test]
    fn pipe_table() {
        assert!(pipe_ok());
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
    fn judge_refs() {
        let c = &cases()[9];
        let a = c.actual();
        assert_eq!(a.refs, 2);
        assert_eq!(a.note, Note::ProbeHit);
        assert_eq!(judge(c), Fault::Refs);
    }

    #[test]
    fn judge_handle() {
        let mut c = cases()[0];
        c.claim.handle = 2;
        assert_eq!(judge(&c), Fault::Handle);
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.row(0), Some(Row { total: 3, good: 2 }));
        assert_eq!(m.row(1), Some(Row { total: 5, good: 3 }));
        assert_eq!(m.row(2), Some(Row { total: 2, good: 2 }));
        assert_eq!(m.row(3), Some(Row { total: 4, good: 1 }));
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
        assert_eq!(r.hit, 2);
        assert_eq!(r.miss, 3);
        assert_eq!(r.load, 2);
        assert_eq!(r.bad, 1);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "noload corpus=14 accept=8 reject=6 hit=2 miss=3 load=2 bad=1 match=14 rate=1000ppm\n"
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
        assert_eq!(r.gate(), Err(EA_ECOUNT));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(EA_ESLOT));
    }

    #[test]
    fn message_exact() {
        assert_eq!(
            message_for("libm.so.6", Note::ProbeMiss).as_str(),
            "object not open"
        );
        assert_eq!(
            message_for("libc.so.6", Note::BadMode).as_str(),
            "invalid mode for dlopen()"
        );
        assert_eq!(
            message_for("./broken.so", Note::FailedEntry).as_str(),
            "./broken.so: cannot open shared object file: No such file or directory"
        );
        assert!(message_for("libc.so.6", Note::ProbeHit).is_empty());
    }

    #[test]
    fn detail_exact() {
        let cs = cases();
        assert_eq!(detail(&cs[0]).as_str(), DETAIL_HIT);
        assert_eq!(detail(&cs[1]).as_str(), DETAIL_MISS);
        assert_eq!(detail(&cs[4]).as_str(), DETAIL_BAD);
    }

    #[test]
    fn gate_steps() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Lookup), Ok(()));
        assert_eq!(g.step(Stage::Mode), Err(EA_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Ready), Ok(()));
        assert_eq!(g.last(), 3);
        assert_eq!(g.close(), Ok(2));
        assert_eq!(g.close(), Err(EA_ESTATE));
    }

    #[test]
    fn slot_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(CORPUS - 1).is_some());
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(1), Err(EA_ENOBS));
        assert_eq!(fault_short(Fault::Refs), "refs");
        assert_eq!(fault_short(Fault::Ok), "ok");
    }

    #[test]
    fn extra_seed() {
        let s = seed_c2();
        assert_eq!(s.refs(0), 2);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
        assert!(!l.is_empty());
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
        assert_eq!(l.register(Sample::blank()).err(), Some(EA_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(EA_EBAL));
        assert_eq!(l.rejects(), 2);
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
        assert_eq!(hex_text(0xfcc0, 4).as_str(), "fcc0");
        assert_eq!(i32_text(-22).as_str(), "-22");
        assert_eq!(i32_text(12).as_str(), "12");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11635_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
