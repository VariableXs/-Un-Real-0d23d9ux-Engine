//! UNX-F11636 dlclose 计数与卸载判据
//!
//! 判据原文：
//! UNX-F11636-J1 引用计数归零卸载 / `__attribute__((destructor))` 执行判据绿，
//! 与 B07 联签。
//!
//! 本模块把 `dlclose` 的完整语义抽成一个可判定模型：句柄表里每个条目带
//! 引用计数、装载状态、NODELETE 常驻位、析构器个数、TLS 析构位与依赖边。
//! 一次 `dlclose` 只做一件事：把计数减一；只有计数归零的条目才进入卸载链，
//! 链上的析构器按“逆装载序”（后装载者先析构）执行，卸载完才把映射交还。
//!
//! 闸门次序：句柄 → 计数 → 依赖 → 析构 → 卸载 → 就绪
//!
//! 1. 句柄：下标越界出 `E_DLHANDLE`，已卸载条目出 `E_DLSTALE`，两者都不动表。
//! 2. 计数：计数大于一，只减一，条目仍在映射中，析构器一次也不跑。
//! 3. 依赖：归零之后，被依赖条目的引用减一；被依赖者若也归零则入链，
//!    否则停留在“依赖仍被持有”档。
//! 4. 析构：链上条目收集析构器个数，NODELETE 条目整体跳过。
//! 5. 卸载：非 NODELETE 条目按逆装载序清映射、回空闲态，重放后表恢复原样。
//! 6. 就绪：返回阶段、注解与最终计数，供上层对判。
//!
//! 关键差异（本卡深度所在）：
//! - “计数减一”与“归零卸载”共用同一个入口，但注解与析构个数完全不同；
//! - 依赖引用减一与依赖入链是两条边：前者只改计数，后者才触发卸载；
//! - NODELETE 条目计数可以归零、映射不能消失、析构器不能跑，三条同时成立；
//! - 卸载链的析构顺序由装载序决定，倒序即错档；
//! - 重新 `dlopen` 复用的是空闲槽位，引用从一重新起算，不继承旧计数。
//!
//! 本模块只做纯计算：没有系统调用、没有全局状态、没有堆分配以外的副作用。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段基址。
pub const SEG_BASE: u32 = 0xfcd0;

/// 句柄下标越界。
pub const E_DLHANDLE: u32 = SEG_BASE + 0x01;
/// 句柄指向已卸载条目。
pub const E_DLSTALE: u32 = SEG_BASE + 0x02;
/// 引用计数下溢（表被外部改坏）。
pub const E_DLZERO: u32 = SEG_BASE + 0x03;
/// NODELETE 条目状态错。
pub const E_DLNODE: u32 = SEG_BASE + 0x04;
/// 依赖边错。
pub const E_DLDEP: u32 = SEG_BASE + 0x05;
/// 析构器个数错。
pub const E_DLDTOR: u32 = SEG_BASE + 0x06;
/// 映射状态错。
pub const E_DLMAP: u32 = SEG_BASE + 0x07;
/// 未卸载。
pub const E_DLUNMAP: u32 = SEG_BASE + 0x08;
/// 重载句柄复用错。
pub const E_DLRELOAD: u32 = SEG_BASE + 0x09;
/// 卸载顺序错。
pub const E_DLORDER: u32 = SEG_BASE + 0x0a;
/// 计数错。
pub const E_DLCOUNT: u32 = SEG_BASE + 0x0b;
/// 状态错。
pub const E_DLSTATE: u32 = SEG_BASE + 0x0c;
/// 阶段错。
pub const E_DLSTAGE: u32 = SEG_BASE + 0x0d;

/// 语料槽数。
pub const SAMPLES: usize = 14;

/// 语料槽数（与账本格数同源）。
pub const CORPUS: usize = 14;

/// 前 8 槽为申报正确档。
pub const ACCEPT: usize = 8;

/// 后 6 槽为申报错误档。
pub const REJECT: usize = 6;

/// 档位行数：计数 / 卸载 / 常驻 / 陈旧。
pub const TIER_COUNT: usize = 4;

/// 句柄表容量。
pub const CAP: usize = 6;

/// 操作码：关闭。
pub const OP_CLOSE: u8 = 0x01;

/// 操作码：计数。
pub const OP_COUNT: u8 = 0x02;

/// 操作码：卸载。
pub const OP_UNMAP: u8 = 0x03;

/// 操作码：析构。
pub const OP_DTOR: u8 = 0x04;

/// 操作码：顺序。
pub const OP_ORDER: u8 = 0x05;

/// 操作码：语料汇总。
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

/// 十六进制文本（至少 `min` 位）。
fn hex_text(v: u64, min: usize) -> String {
    let digits = b"0123456789abcdef";
    let mut buf = [0u8; 16];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        buf[0] = b'0';
        n = 1;
    }
    while x > 0 && n < buf.len() {
        buf[n] = digits[(x & 0xf) as usize];
        x >>= 4;
        n += 1;
    }
    while n < min && n < buf.len() {
        buf[n] = b'0';
        n += 1;
    }
    let mut s = String::from("0x");
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

/// 布尔文本。
pub fn bool_text(v: bool) -> String {
    if v {
        String::from("yes")
    } else {
        String::from("no")
    }
}

// ---------------------------------------------------------------------------
// 条目与句柄表
// ---------------------------------------------------------------------------

/// 条目装载状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// 空闲（已卸载，槽位可复用）。
    Free,
    /// 已映射。
    Mapped,
    /// 计数归零但 NODELETE 常驻。
    Kept,
}

impl State {
    /// 是否仍在地址空间里。
    pub fn mapped(self) -> bool {
        matches!(self, State::Mapped | State::Kept)
    }

    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            State::Free => "free",
            State::Mapped => "mapped",
            State::Kept => "kept",
        }
    }
}

/// 一个共享对象条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lib {
    /// 名字。
    pub name: &'static str,
    /// 引用计数。
    pub refs: u32,
    /// 状态。
    pub state: State,
    /// NODELETE 常驻位。
    pub nodelete: bool,
    /// 析构器个数。
    pub dtors: u32,
    /// TLS 析构位（线程退出时才跑）。
    pub tls: bool,
    /// 依赖边（最多两条）。
    pub deps: [u8; 2],
    /// 依赖边条数。
    pub ndep: u8,
}

impl Lib {
    /// 空白条目。
    pub const fn blank() -> Lib {
        Lib {
            name: "",
            refs: 0,
            state: State::Free,
            nodelete: false,
            dtors: 0,
            tls: false,
            deps: [0, 0],
            ndep: 0,
        }
    }

    /// 新条目（已映射，引用一）。
    pub const fn live(name: &'static str, dtors: u32) -> Lib {
        Lib {
            name,
            refs: 1,
            state: State::Mapped,
            nodelete: false,
            dtors,
            tls: false,
            deps: [0, 0],
            ndep: 0,
        }
    }

    /// 常驻条目。
    pub const fn resident(name: &'static str, dtors: u32) -> Lib {
        Lib {
            name,
            refs: 1,
            state: State::Mapped,
            nodelete: true,
            dtors,
            tls: false,
            deps: [0, 0],
            ndep: 0,
        }
    }

    /// 是否仍在映射中。
    pub fn mapped(&self) -> bool {
        self.state.mapped()
    }

    /// 加一条依赖边。
    pub fn link(&mut self, to: u8) -> bool {
        if self.ndep as usize >= self.deps.len() || to as usize >= CAP {
            return false;
        }
        let k = self.ndep as usize;
        match self.deps.get_mut(k) {
            Some(slot) => {
                *slot = to;
                self.ndep += 1;
                true
            }
            None => false,
        }
    }

    /// 依赖表切片。
    pub fn dep_list(&self) -> &[u8] {
        let n = self.ndep as usize;
        if n > self.deps.len() {
            return &self.deps[..];
        }
        &self.deps[..n]
    }
}

/// 句柄表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    libs: [Lib; CAP],
    len: usize,
    seq: [u8; CAP],
    nseq: u8,
}

impl Image {
    /// 空表。
    pub const fn new() -> Image {
        Image { libs: [Lib::blank(); CAP], len: 0, seq: [0; CAP], nseq: 0 }
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
    pub fn get(&self, i: usize) -> Option<&Lib> {
        if i >= self.len {
            return None;
        }
        self.libs.get(i)
    }

    /// 取可变条目。
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Lib> {
        if i >= self.len {
            return None;
        }
        self.libs.get_mut(i)
    }

    /// 按名字查条目。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.libs.get(i) {
                if l.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 装入一个新条目（引用一）。
    pub fn load(
        &mut self,
        name: &'static str,
        refs: u32,
        dtors: u32,
        nodelete: bool,
        tls: bool,
    ) -> Option<usize> {
        if self.len >= CAP {
            return None;
        }
        let i = self.len;
        if let Some(slot) = self.libs.get_mut(i) {
            *slot = Lib {
                name,
                refs,
                state: State::Mapped,
                nodelete,
                dtors,
                tls,
                deps: [0, 0],
                ndep: 0,
            };
        }
        if let Some(sp) = self.seq.get_mut(self.nseq as usize) {
            *sp = i as u8;
            self.nseq += 1;
        }
        self.len += 1;
        Some(i)
    }

    /// 增引用（`dlopen` 命中已映射条目）。
    pub fn hold(&mut self, i: usize) -> Option<u32> {
        match self.get_mut(i) {
            Some(l) => {
                if !l.state.mapped() {
                    return None;
                }
                l.refs = l.refs.saturating_add(1);
                Some(l.refs)
            }
            None => None,
        }
    }

    /// 加依赖边。
    pub fn set_dep(&mut self, a: usize, b: usize) -> bool {
        if a >= self.len || b >= self.len || a == b {
            return false;
        }
        match self.libs.get_mut(a) {
            Some(l) => l.link(b as u8),
            None => false,
        }
    }

    /// 条目在装载序中的位置。
    pub fn seq_pos(&self, i: usize) -> i32 {
        let mut k = 0usize;
        while k < self.nseq as usize {
            if let Some(v) = self.seq.get(k) {
                if *v as usize == i {
                    return k as i32;
                }
            }
            k += 1;
        }
        -1
    }

    /// 引用数。
    pub fn refs_at(&self, i: usize) -> u32 {
        match self.get(i) {
            Some(l) => l.refs,
            None => 0,
        }
    }

    /// 析构器个数。
    pub fn dtors_at(&self, i: usize) -> u32 {
        match self.get(i) {
            Some(l) => l.dtors,
            None => 0,
        }
    }

    /// 状态。
    pub fn state_at(&self, i: usize) -> State {
        match self.get(i) {
            Some(l) => l.state,
            None => State::Free,
        }
    }

    /// 映射中的条目数。
    pub fn mapped_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.libs.get(i) {
                if l.mapped() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 重新装载一个空闲条目：引用从一重算，装载序追加在末尾。
    pub fn reload(&mut self, i: usize) -> Option<u32> {
        if self.nseq as usize >= CAP {
            return None;
        }
        match self.libs.get_mut(i) {
            Some(l) => {
                if l.state != State::Free {
                    return None;
                }
                l.state = State::Mapped;
                l.refs = 1;
                if let Some(sp) = self.seq.get_mut(self.nseq as usize) {
                    *sp = i as u8;
                    self.nseq += 1;
                }
                Some(l.refs)
            }
            None => None,
        }
    }

    /// 除 `skip` 之外仍被持有的引用总数。
    pub fn held_refs(&self, skip: Option<usize>) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            let skipped = match skip {
                Some(s) => s == i,
                None => false,
            };
            if !skipped {
                if let Some(l) = self.libs.get(i) {
                    if l.mapped() {
                        n = n.saturating_add(l.refs);
                    }
                }
            }
            i += 1;
        }
        n
    }

    /// 表指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.libs.get(i) {
                h = fold_str(h, l.name);
                h = fold_word(h, l.refs as u64);
                h = fold_word(h, l.state as u64);
                h = fold_word(h, l.nodelete as u64);
                h = fold_word(h, l.dtors as u64);
                h = fold_word(h, l.tls as u64);
                h = fold_word(h, l.ndep as u64);
            }
            i += 1;
        }
        fold_word(h, self.len as u64)
    }
}

// ---------------------------------------------------------------------------
// 阶段与注解
// ---------------------------------------------------------------------------

/// 关闭阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 句柄校验。
    Handle,
    /// 计数递减。
    Count,
    /// 依赖传播。
    Deps,
    /// 析构收集。
    Dtor,
    /// 解除映射。
    Unmap,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(self) -> u8 {
        match self {
            Stage::Handle => 0,
            Stage::Count => 1,
            Stage::Deps => 2,
            Stage::Dtor => 3,
            Stage::Unmap => 4,
            Stage::Ready => 5,
        }
    }

    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Handle => "handle",
            Stage::Count => "count",
            Stage::Deps => "deps",
            Stage::Dtor => "dtor",
            Stage::Unmap => "unmap",
            Stage::Ready => "ready",
        }
    }
}

/// 关闭注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 只减计数。
    CountDown,
    /// 归零卸载。
    Unmapped,
    /// NODELETE 常驻。
    KeptNodelete,
    /// 依赖计数减一。
    DepDown,
    /// 依赖一并卸载。
    Chain,
    /// 句柄指向已卸载条目。
    StaleHandle,
    /// 句柄越界。
    BadHandle,
    /// 计数下溢。
    ZeroRefs,
    /// 重载后重新存活。
    Reloaded,
}

impl Note {
    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Note::CountDown => "count-down",
            Note::Unmapped => "unmapped",
            Note::KeptNodelete => "kept-nodelete",
            Note::DepDown => "dep-down",
            Note::Chain => "chain",
            Note::StaleHandle => "stale-handle",
            Note::BadHandle => "bad-handle",
            Note::ZeroRefs => "zero-refs",
            Note::Reloaded => "reloaded",
        }
    }
}

/// 关闭结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 阶段。
    pub stage: Stage,
    /// 错误码（零为成功）。
    pub code: u32,
    /// 关闭条目最终引用数。
    pub refs: u32,
    /// 其他条目仍被持有的引用总数。
    pub deps: u32,
    /// 关闭条目是否仍在映射中。
    pub mapped: bool,
    /// 本次实际执行的析构器个数。
    pub dtors: u32,
    /// 卸载顺序打包（高位先卸载）。
    pub order: u64,
    /// 是否有 TLS 析构被推迟。
    pub tls: bool,
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
            stage: Stage::Handle,
            code: 0,
            refs: 0,
            deps: 0,
            mapped: false,
            dtors: 0,
            order: 0,
            tls: false,
            note: Note::BadHandle,
        }
    }
}

/// 卸载链结算：析构数、顺序包、依赖减计数、依赖入链数、TLS 推迟。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainSum {
    /// 析构器个数。
    pub dtors: u32,
    /// 顺序包。
    pub order: u64,
    /// 依赖计数被减的次数。
    pub dep_events: u32,
    /// 依赖入链卸载的条目数。
    pub dep_closed: u32,
    /// 是否有 TLS 析构被推迟。
    pub tls: bool,
}

impl ChainSum {
    /// 空白。
    pub const fn blank() -> ChainSum {
        ChainSum { dtors: 0, order: 0, dep_events: 0, dep_closed: 0, tls: false }
    }
}

/// 收集卸载链：从 `start` 出发，沿依赖边传播，返回结算。
fn unload_chain(img: &mut Image, start: usize) -> ChainSum {
    let mut sum = ChainSum::blank();
    let mut closed = [false; CAP];
    if start >= CAP {
        return sum;
    }
    closed[start] = true;

    let mut pass = 0usize;
    loop {
        let mut changed = false;
        let mut i = 0usize;
        while i < img.len() {
            if closed[i] {
                let (deps, ndep) = match img.get(i) {
                    Some(l) => (l.deps, l.ndep),
                    None => ([0u8; 2], 0),
                };
                let mut k = 0usize;
                while k < (ndep as usize) {
                    if let Some(d) = deps.get(k) {
                        let d = *d as usize;
                        if d < CAP && !closed[d] {
                            if let Some(dep) = img.get_mut(d) {
                                if dep.refs > 0 {
                                    dep.refs -= 1;
                                    sum.dep_events = sum.dep_events.saturating_add(1);
                                }
                                if dep.refs == 0 {
                                    if dep.nodelete {
                                        if dep.state == State::Mapped {
                                            dep.state = State::Kept;
                                        }
                                    } else if dep.state.mapped() {
                                        closed[d] = true;
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        pass += 1;
        if !changed || pass >= CAP {
            break;
        }
    }

    let seq = order_of(img, &closed);
    let mut k = 0usize;
    while k < CAP {
        let cur = match seq.get(k) {
            Some(v) => *v,
            None => break,
        };
        if cur != 0xff {
            let idx = cur as usize;
            if idx < CAP && closed[idx] {
                if let Some(l) = img.get_mut(idx) {
                    l.state = State::Free;
                    l.refs = 0;
                    sum.dtors = sum.dtors.saturating_add(l.dtors);
                    if l.tls {
                        sum.tls = true;
                    }
                    sum.order = (sum.order << 8) | ((idx as u64) + 1);
                    sum.dep_closed = sum.dep_closed.saturating_add(1);
                }
            }
        }
        k += 1;
    }
    sum
}

/// 卸载顺序：链上非 NODELETE 条目按装载序逆序，未用槽位填 `0xff`。
fn order_of(img: &Image, closed: &[bool; CAP]) -> [u8; CAP] {
    let mut cand = [0xffu8; CAP];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < img.len() {
        if closed[i] {
            if let Some(l) = img.get(i) {
                if !l.nodelete && n < CAP {
                    cand[n] = i as u8;
                    n += 1;
                }
            }
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < n {
        let mut best = k;
        let mut m = k + 1;
        while m < n {
            let a = img.seq_pos(cand[m] as usize);
            let b = img.seq_pos(cand[best] as usize);
            if a > b {
                best = m;
            }
            m += 1;
        }
        let t = cand[k];
        cand[k] = cand[best];
        cand[best] = t;
        k += 1;
    }
    cand
}

/// 关闭一个句柄。
pub fn dlclose(img: &mut Image, idx: usize) -> Outcome {
    if idx >= img.len() || idx >= CAP {
        return Outcome {
            stage: Stage::Handle,
            code: E_DLHANDLE,
            refs: 0,
            deps: 0,
            mapped: false,
            dtors: 0,
            order: 0,
            tls: false,
            note: Note::BadHandle,
        };
    }
    let (state, refs, nodelete) = match img.get(idx) {
        Some(l) => (l.state, l.refs, l.nodelete),
        None => {
            return Outcome {
                stage: Stage::Handle,
                code: E_DLHANDLE,
                refs: 0,
                deps: 0,
                mapped: false,
                dtors: 0,
                order: 0,
                tls: false,
                note: Note::BadHandle,
            }
        }
    };
    if state == State::Free {
        return Outcome {
            stage: Stage::Handle,
            code: E_DLSTALE,
            refs: 0,
            deps: img.held_refs(Some(idx)),
            mapped: false,
            dtors: 0,
            order: 0,
            tls: false,
            note: Note::StaleHandle,
        };
    }
    if refs == 0 {
        return Outcome {
            stage: Stage::Count,
            code: E_DLZERO,
            refs: 0,
            deps: img.held_refs(Some(idx)),
            mapped: state.mapped(),
            dtors: 0,
            order: 0,
            tls: false,
            note: Note::ZeroRefs,
        };
    }
    if refs > 1 {
        let left = match img.get_mut(idx) {
            Some(l) => {
                l.refs -= 1;
                l.refs
            }
            None => 0,
        };
        return Outcome {
            stage: Stage::Count,
            code: 0,
            refs: left,
            deps: img.held_refs(Some(idx)),
            mapped: true,
            dtors: 0,
            order: 0,
            tls: false,
            note: Note::CountDown,
        };
    }

    if nodelete {
        if let Some(l) = img.get_mut(idx) {
            l.refs = 0;
            l.state = State::Kept;
        }
        return Outcome {
            stage: Stage::Count,
            code: 0,
            refs: 0,
            deps: img.held_refs(Some(idx)),
            mapped: true,
            dtors: 0,
            order: 0,
            tls: false,
            note: Note::KeptNodelete,
        };
    }

    let sum = unload_chain(img, idx);
    let note = if sum.dep_closed > 1 {
        Note::Chain
    } else if sum.dep_events > 0 {
        Note::DepDown
    } else {
        Note::Unmapped
    };
    Outcome {
        stage: Stage::Ready,
        code: 0,
        refs: 0,
        deps: img.held_refs(Some(idx)),
        mapped: false,
        dtors: sum.dtors,
        order: sum.order,
        tls: sum.tls,
        note,
    }
}

/// 关闭后重载同一槽位。
pub fn dlclose_reload(img: &mut Image, idx: usize) -> Outcome {
    let first = dlclose(img, idx);
    if !first.ok() {
        return first;
    }
    match img.reload(idx) {
        Some(_) => Outcome {
            stage: Stage::Ready,
            code: 0,
            refs: 1,
            deps: img.held_refs(Some(idx)),
            mapped: true,
            dtors: 0,
            order: 0,
            tls: false,
            note: Note::Reloaded,
        },
        None => first,
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 关闭说明文案。
pub fn message(o: &Outcome, name: &str) -> String {
    if o.ok() {
        return String::new();
    }
    let mut s = String::from(name);
    s.push_str(": ");
    match o.note {
        Note::BadHandle => s.push_str("invalid handle"),
        Note::StaleHandle => s.push_str("handle already closed"),
        Note::ZeroRefs => s.push_str("refcount underflow"),
        _ => s.push_str("dlclose failed"),
    }
    s.push('\n');
    s
}

/// 成功路径的释义文案。
pub fn explain(o: &Outcome) -> &'static str {
    match o.note {
        Note::CountDown => "still referenced",
        Note::Unmapped => "unloaded",
        Note::KeptNodelete => "NODELETE keeps image mapped",
        Note::DepDown => "unloaded, dependency still referenced",
        Note::Chain => "unloaded with dependencies",
        Note::Reloaded => "reloaded",
        Note::StaleHandle => "handle already closed",
        Note::BadHandle => "invalid handle",
        Note::ZeroRefs => "refcount underflow",
    }
}

// ---------------------------------------------------------------------------
// 档位与缺陷
// ---------------------------------------------------------------------------

/// 四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 计数档。
    Count,
    /// 卸载档。
    Unmap,
    /// 常驻档。
    Keep,
    /// 陈旧档。
    Stale,
}

impl Tier {
    /// 行下标。
    pub fn index(self) -> usize {
        match self {
            Tier::Count => 0,
            Tier::Unmap => 1,
            Tier::Keep => 2,
            Tier::Stale => 3,
        }
    }

    /// 由注解归行。
    pub fn of(n: Note) -> Tier {
        match n {
            Note::CountDown | Note::Reloaded => Tier::Count,
            Note::Unmapped | Note::DepDown | Note::Chain => Tier::Unmap,
            Note::KeptNodelete => Tier::Keep,
            Note::StaleHandle | Note::BadHandle | Note::ZeroRefs => Tier::Stale,
        }
    }

    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Count => "count",
            Tier::Unmap => "unmap",
            Tier::Keep => "keep",
            Tier::Stale => "stale",
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
    /// 错误码不符。
    Code,
    /// 关闭条目计数不符。
    Refs,
    /// 其他条目持有数不符。
    Deps,
    /// 映射状态不符。
    Mapped,
    /// 析构个数不符。
    Dtor,
    /// 卸载顺序不符。
    Order,
    /// TLS 推迟不符。
    Tls,
    /// 注解不符。
    Note,
}

impl Fault {
    /// 缺陷码。
    pub fn code(self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => E_DLSTATE,
            Fault::Stage => E_DLSTAGE,
            Fault::Code => E_DLCOUNT,
            Fault::Refs => E_DLSTALE,
            Fault::Deps => E_DLDEP,
            Fault::Mapped => E_DLMAP,
            Fault::Dtor => E_DLDTOR,
            Fault::Order => E_DLORDER,
            Fault::Tls => E_DLNODE,
            Fault::Note => E_DLRELOAD,
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
        Fault::Refs => "refs",
        Fault::Deps => "deps",
        Fault::Mapped => "mapped",
        Fault::Dtor => "dtor",
        Fault::Order => "order",
        Fault::Tls => "tls",
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
    /// 错误码。
    pub code: u32,
    /// 关闭条目计数。
    pub refs: u32,
    /// 其他条目持有数。
    pub deps: u32,
    /// 映射状态。
    pub mapped: bool,
    /// 析构个数。
    pub dtors: u32,
    /// 顺序包。
    pub order: u64,
    /// TLS 推迟。
    pub tls: bool,
    /// 注解。
    pub note: Note,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Handle,
            code: 0,
            refs: 0,
            deps: 0,
            mapped: false,
            dtors: 0,
            order: 0,
            tls: false,
            note: Note::BadHandle,
        }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.code == 0
    }
}

/// 操作种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// 只关闭。
    Close,
    /// 关闭后重载。
    CloseReload,
}

/// 预置表构造器。
pub type Setup = fn() -> Image;

/// 一个语料形状。
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// 槽标签。
    pub tag: &'static str,
    /// 目标名字。
    pub name: &'static str,
    /// 目标下标。
    pub target: usize,
    /// 操作。
    pub op: Op,
    /// 预置表。
    pub setup: Setup,
}

/// 形状等值：只比标签、目标与操作，函数指针不参与比较。
impl PartialEq for Shape {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag
            && self.name == other.name
            && self.target == other.target
            && self.op == other.op
    }
}

impl Eq for Shape {}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape { tag: "", name: "", target: 0, op: Op::Close, setup: setup_empty }
    }

    /// 实际结果（每次新表）。
    pub fn actual(&self) -> Outcome {
        let mut img = (self.setup)();
        match self.op {
            Op::Close => dlclose(&mut img, self.target),
            Op::CloseReload => dlclose_reload(&mut img, self.target),
        }
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
// 预置表：八种场景
// ---------------------------------------------------------------------------

/// 空表。
fn setup_empty() -> Image {
    Image::new()
}

/// 计数二、析构一。
fn setup_count_down() -> Image {
    let mut im = Image::new();
    let _ = im.load("liba.so", 2, 1, false, false);
    im
}

/// 计数一、析构二。
fn setup_unmap() -> Image {
    let mut im = Image::new();
    let _ = im.load("liba.so", 1, 2, false, false);
    im
}

/// 计数一、NODELETE、析构二。
fn setup_nodelete() -> Image {
    let mut im = Image::new();
    let _ = im.load("liba.so", 1, 2, true, false);
    im
}

/// 主条目计数一，依赖条目计数二：只减依赖计数，不卸载依赖。
fn setup_dep() -> Image {
    let mut im = Image::new();
    let _ = im.load("liba.so", 1, 1, false, false);
    let _ = im.load("libb.so", 2, 1, false, false);
    let _ = im.set_dep(0, 1);
    im
}

/// 主条目计数一，依赖条目计数一：两条一起卸载，逆装载序析构。
fn setup_chain() -> Image {
    let mut im = Image::new();
    let _ = im.load("liba.so", 1, 1, false, false);
    let _ = im.load("libb.so", 1, 2, false, false);
    let _ = im.set_dep(0, 1);
    im
}

/// 条目已卸载：句柄陈旧。
fn setup_stale() -> Image {
    let mut im = Image::new();
    let _ = im.load("liba.so", 0, 1, false, false);
    if let Some(l) = im.get_mut(0) {
        l.state = State::Free;
        l.refs = 0;
    }
    im
}

/// 计数一、析构一、TLS 位。
fn setup_tls() -> Image {
    let mut im = Image::new();
    let _ = im.load("liba.so", 1, 1, false, true);
    im
}

/// 计数一、析构一，供重载槽位复用。
fn setup_reload() -> Image {
    let mut im = Image::new();
    let _ = im.load("liba.so", 1, 1, false, false);
    im
}

/// 8 个预置构造器总表。
pub const SETUPS: [Setup; 8] = [
    setup_count_down,
    setup_unmap,
    setup_nodelete,
    setup_dep,
    setup_chain,
    setup_stale,
    setup_tls,
    setup_reload,
];

/// 14 个形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape { tag: "close-count", name: "liba.so", target: 0, op: Op::Close, setup: setup_count_down },
    Shape { tag: "close-unmap", name: "liba.so", target: 0, op: Op::Close, setup: setup_unmap },
    Shape { tag: "close-nodelete", name: "liba.so", target: 0, op: Op::Close, setup: setup_nodelete },
    Shape { tag: "dep-release", name: "liba.so", target: 0, op: Op::Close, setup: setup_dep },
    Shape { tag: "chain-unload", name: "liba.so", target: 0, op: Op::Close, setup: setup_chain },
    Shape { tag: "stale-close", name: "liba.so", target: 0, op: Op::Close, setup: setup_stale },
    Shape { tag: "tls-dtor", name: "liba.so", target: 0, op: Op::Close, setup: setup_tls },
    Shape { tag: "reload-fresh", name: "liba.so", target: 0, op: Op::CloseReload, setup: setup_reload },
    Shape { tag: "count-as-unload", name: "liba.so", target: 0, op: Op::Close, setup: setup_count_down },
    Shape { tag: "nodelete-as-unmapped", name: "liba.so", target: 0, op: Op::Close, setup: setup_nodelete },
    Shape { tag: "dep-no-refs", name: "liba.so", target: 0, op: Op::Close, setup: setup_dep },
    Shape { tag: "chain-order-reversed", name: "liba.so", target: 0, op: Op::Close, setup: setup_chain },
    Shape { tag: "stale-as-success", name: "liba.so", target: 0, op: Op::Close, setup: setup_stale },
    Shape { tag: "reload-stale-handle", name: "liba.so", target: 0, op: Op::CloseReload, setup: setup_reload },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0 计数二减一：仍在映射，未跑析构。
    Claim { stage: Stage::Count, code: 0, refs: 1, deps: 0, mapped: true, dtors: 0, order: 0, tls: false, note: Note::CountDown },
    // 1 归零卸载：跑两个析构，顺序包只含自身。
    Claim { stage: Stage::Ready, code: 0, refs: 0, deps: 0, mapped: false, dtors: 2, order: 0x1, tls: false, note: Note::Unmapped },
    // 2 NODELETE：计数归零、映射留下、析构不跑。
    Claim { stage: Stage::Count, code: 0, refs: 0, deps: 0, mapped: true, dtors: 0, order: 0, tls: false, note: Note::KeptNodelete },
    // 3 依赖计数减一：依赖仍在映射，自身卸载。
    Claim { stage: Stage::Ready, code: 0, refs: 0, deps: 1, mapped: false, dtors: 1, order: 0x1, tls: false, note: Note::DepDown },
    // 4 依赖一并卸载：逆装载序 b 后 a，顺序包 0x201。
    Claim { stage: Stage::Ready, code: 0, refs: 0, deps: 0, mapped: false, dtors: 3, order: 0x201, tls: false, note: Note::Chain },
    // 5 陈旧句柄：E_DLSTALE，表不动。
    Claim { stage: Stage::Handle, code: E_DLSTALE, refs: 0, deps: 0, mapped: false, dtors: 0, order: 0, tls: false, note: Note::StaleHandle },
    // 6 TLS 析构推迟：普通析构照跑，TLS 位置真。
    Claim { stage: Stage::Ready, code: 0, refs: 0, deps: 0, mapped: false, dtors: 1, order: 0x1, tls: true, note: Note::Unmapped },
    // 7 重载：引用从一重算，映射恢复。
    Claim { stage: Stage::Ready, code: 0, refs: 1, deps: 0, mapped: true, dtors: 0, order: 0, tls: false, note: Note::Reloaded },
    // 8 错判：把计数减一当成卸载。
    Claim { stage: Stage::Count, code: 0, refs: 1, deps: 0, mapped: false, dtors: 0, order: 0, tls: false, note: Note::CountDown },
    // 9 错判：把 NODELETE 当普通卸载。
    Claim { stage: Stage::Count, code: 0, refs: 0, deps: 0, mapped: false, dtors: 0, order: 0, tls: false, note: Note::KeptNodelete },
    // 10 错判：依赖计数没减。
    Claim { stage: Stage::Ready, code: 0, refs: 0, deps: 2, mapped: false, dtors: 1, order: 0x1, tls: false, note: Note::DepDown },
    // 11 错判：卸载顺序倒置。
    Claim { stage: Stage::Ready, code: 0, refs: 0, deps: 0, mapped: false, dtors: 3, order: 0x102, tls: false, note: Note::Chain },
    // 12 错判：陈旧句柄当成功。
    Claim { stage: Stage::Ready, code: 0, refs: 0, deps: 0, mapped: false, dtors: 0, order: 0, tls: false, note: Note::Reloaded },
    // 13 错判：重载复用旧计数。
    Claim { stage: Stage::Ready, code: 0, refs: 2, deps: 0, mapped: true, dtors: 0, order: 0, tls: false, note: Note::Reloaded },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Mapped, Fault::Mapped, Fault::Deps, Fault::Order, Fault::Estate, Fault::Refs];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] =
    [E_DLMAP, E_DLMAP, E_DLDEP, E_DLORDER, E_DLSTATE, E_DLSTALE];

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
        Err(E_DLCOUNT)
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

/// 对判：成败 → 阶段 → 错误码 → 计数 → 持有数 → 映射 → 析构 → 顺序 → TLS → 注解。
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
    if claim.refs != a.refs {
        return Fault::Refs;
    }
    if claim.deps != a.deps {
        return Fault::Deps;
    }
    if claim.mapped != a.mapped {
        return Fault::Mapped;
    }
    if claim.dtors != a.dtors {
        return Fault::Dtor;
    }
    if claim.order != a.order {
        return Fault::Order;
    }
    if claim.tls != a.tls {
        return Fault::Tls;
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
/// `<name> stage=<s> code=<c> refs=<n> deps=<n> mapped=<y/n> dtors=<n> order=<o> note=<n>`。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(c.shape.name);
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" code=");
    s.push_str(u32_text(a.code).as_str());
    s.push_str(" refs=");
    s.push_str(u32_text(a.refs).as_str());
    s.push_str(" deps=");
    s.push_str(u32_text(a.deps).as_str());
    s.push_str(" mapped=");
    s.push_str(bool_text(a.mapped).as_str());
    s.push_str(" dtors=");
    s.push_str(u32_text(a.dtors).as_str());
    s.push_str(" order=");
    s.push_str(hex_text(a.order, 1).as_str());
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
    /// 行（计数 / 卸载 / 常驻 / 陈旧）。
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
    /// 计数档正确数。
    pub count: u32,
    /// 卸载档正确数。
    pub unmap: u32,
    /// 常驻档正确数。
    pub keep: u32,
    /// 陈旧档正确数。
    pub stale: u32,
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
            Err(E_DLCOUNT)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("dlclose corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" count=");
        s.push_str(u32_text(self.count).as_str());
        s.push_str(" unmap=");
        s.push_str(u32_text(self.unmap).as_str());
        s.push_str(" keep=");
        s.push_str(u32_text(self.keep).as_str());
        s.push_str(" stale=");
        s.push_str(u32_text(self.stale).as_str());
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
            return Err(E_DLCOUNT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut count = 0u32;
    let mut unmap = 0u32;
    let mut keep = 0u32;
    let mut stale = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(E_DLCOUNT),
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
            None => return Err(E_DLCOUNT),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(a.note) {
                Tier::Count => count += 1,
                Tier::Unmap => unmap += 1,
                Tier::Keep => keep += 1,
                Tier::Stale => stale += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, count, unmap, keep, stale, matched })
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
            return Err(E_DLSTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(E_DLORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(E_DLSTATE);
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
            return Err(E_DLCOUNT);
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
            return Err(E_DLSTATE);
        }
        const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 4, 1, 1, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(E_DLSTATE),
            };
            if s.slot as usize != i {
                return Err(E_DLCOUNT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(E_DLCOUNT);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(E_DLCOUNT);
                        }
                    }
                    None => return Err(E_DLSTATE),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(E_DLCOUNT);
                        }
                    }
                    None => return Err(E_DLSTATE),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(E_DLSTATE);
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
            return Err(E_DLSTATE);
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
            Err(E_DLSTATE)
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
    let _ = l.register(count_cell(3, OP_COUNT, r.map(|x| x.count)));
    let _ = l.register(count_cell(4, OP_UNMAP, r.map(|x| x.unmap)));
    let _ = l.register(count_cell(5, OP_UNMAP, r.map(|x| x.keep)));
    let _ = l.register(count_cell(6, OP_CLOSE, r.map(|x| x.stale)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_DTOR, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_ORDER,
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

/// 四路注入：表、链、报、账。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && lib_ok() && refs_ok();
    out[1] = chain_ok() && nodelete_ok() && judge_ok();
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
    Stage::Count,
    Stage::Ready,
    Stage::Count,
    Stage::Ready,
    Stage::Ready,
    Stage::Handle,
    Stage::Ready,
    Stage::Ready,
    Stage::Count,
    Stage::Count,
    Stage::Ready,
    Stage::Ready,
    Stage::Handle,
    Stage::Ready,
];

/// 第 i 槽的期望错误码。
const WANT_CODE: [u32; CORPUS] = [
    0, 0, 0, 0, 0, E_DLSTALE, 0, 0, 0, 0, 0, 0, E_DLSTALE, 0,
];

/// 第 i 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::CountDown,
    Note::Unmapped,
    Note::KeptNodelete,
    Note::DepDown,
    Note::Chain,
    Note::StaleHandle,
    Note::Unmapped,
    Note::Reloaded,
    Note::CountDown,
    Note::KeptNodelete,
    Note::DepDown,
    Note::Chain,
    Note::StaleHandle,
    Note::Reloaded,
];

/// 名称与段基址。
fn name_ok() -> bool {
    let codes = [
        E_DLHANDLE, E_DLSTALE, E_DLZERO, E_DLNODE, E_DLDEP, E_DLDTOR, E_DLMAP, E_DLUNMAP,
        E_DLRELOAD, E_DLORDER, E_DLCOUNT, E_DLSTATE, E_DLSTAGE,
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
    let a = SEG_BASE == 0xfcd0;
    let b = E_DLHANDLE == SEG_BASE + 0x01 && E_DLSTAGE == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = CAP == 6 && TIER_COUNT == 4;
    a && b && c && d
}

/// 条目与句柄表。
fn lib_ok() -> bool {
    let mut im = Image::new();
    let a = im.is_empty() && im.len() == 0 && im.mapped_count() == 0;
    let b = im.load("liba.so", 2, 3, false, false) == Some(0);
    let c = im.load("libb.so", 1, 1, true, true) == Some(1);
    let d = im.find("libb.so") == Some(1) && im.find("nope") == None;
    let e = im.refs_at(0) == 2 && im.dtors_at(1) == 1 && im.state_at(0) == State::Mapped;
    let f = im.hold(0) == Some(3) && im.refs_at(0) == 3;
    let g = im.set_dep(0, 1) && im.get(0).map(|l| l.ndep) == Some(1);
    let h = !im.set_dep(1, 1) && !im.set_dep(0, 9);
    let mut full = Image::new();
    let mut k = 0usize;
    while k < CAP {
        let _ = full.load("lib.so", 1, 0, false, false);
        k += 1;
    }
    let i = full.len() == CAP && full.load("over.so", 1, 0, false, false) == None;
    a && b && c && d && e && f && g && h && i
}

/// 引用计数语义。
fn refs_ok() -> bool {
    let mut im = setup_count_down();
    let a = dlclose(&mut im, 0);
    let ok1 = a.stage == Stage::Count
        && a.code == 0
        && a.refs == 1
        && a.mapped
        && a.dtors == 0
        && a.note == Note::CountDown;
    let b = dlclose(&mut im, 0);
    let ok2 = b.stage == Stage::Ready
        && b.refs == 0
        && !b.mapped
        && b.dtors == 1
        && b.order == 0x1
        && b.note == Note::Unmapped;
    let mut zero = setup_stale();
    if let Some(l) = zero.get_mut(0) {
        l.state = State::Mapped;
        l.refs = 0;
    }
    let c = dlclose(&mut zero, 0);
    let ok3 = c.stage == Stage::Count && c.code == E_DLZERO && c.note == Note::ZeroRefs;
    let mut bad = Image::new();
    let _ = bad.load("liba.so", 1, 0, false, false);
    let d = dlclose(&mut bad, 4);
    let ok4 = d.stage == Stage::Handle && d.code == E_DLHANDLE && d.note == Note::BadHandle;
    ok1 && ok2 && ok3 && ok4
}

/// 卸载链语义。
fn chain_ok() -> bool {
    let mut im = setup_chain();
    let a = dlclose(&mut im, 0);
    let ok1 = a.stage == Stage::Ready
        && a.refs == 0
        && a.deps == 0
        && !a.mapped
        && a.dtors == 3
        && a.order == 0x201
        && a.note == Note::Chain;
    let ok2 = im.state_at(0) == State::Free && im.state_at(1) == State::Free;
    let ok3 = im.mapped_count() == 0;
    let mut rel = setup_dep();
    let b = dlclose(&mut rel, 0);
    let ok4 = b.note == Note::DepDown
        && b.dtors == 1
        && b.order == 0x1
        && rel.refs_at(1) == 1
        && rel.state_at(1) == State::Mapped;
    let mut one = setup_unmap();
    let c = dlclose(&mut one, 0);
    let ok5 = c.note == Note::Unmapped && c.dtors == 2 && one.mapped_count() == 0;
    ok1 && ok2 && ok3 && ok4 && ok5
}

/// 常驻与重载语义。
fn nodelete_ok() -> bool {
    let mut im = setup_nodelete();
    let a = dlclose(&mut im, 0);
    let ok1 = a.stage == Stage::Count
        && a.refs == 0
        && a.mapped
        && a.dtors == 0
        && a.note == Note::KeptNodelete
        && im.state_at(0) == State::Kept;
    let ok2 = im.hold(0) == Some(1) && im.refs_at(0) == 1;
    let mut rl = setup_reload();
    let b = dlclose_reload(&mut rl, 0);
    let ok3 = b.note == Note::Reloaded
        && b.refs == 1
        && b.mapped
        && rl.state_at(0) == State::Mapped
        && rl.refs_at(0) == 1;
    let mut st = setup_stale();
    let c = dlclose(&mut st, 0);
    let ok4 = c.code == E_DLSTALE && c.note == Note::StaleHandle && st.state_at(0) == State::Free;
    ok1 && ok2 && ok3 && ok4
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
    b.shape = Shape { tag: "x", name: "liba.so", target: 0, op: Op::Close, setup: setup_unmap };
    b.claim = CLAIMS[1];
    let c = b.judge() == Fault::Ok;
    b.claim.refs = 9;
    let d = b.judge() == Fault::Refs;
    c && d
}

/// 矩阵统计。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    let a = m.total() == CORPUS && m.good() == ACCEPT && m.balanced();
    let b = m.row(0) == Some(Row { total: 4, good: 2 });
    let c = m.row(1) == Some(Row { total: 6, good: 4 });
    let d = m.row(2) == Some(Row { total: 2, good: 1 });
    let e = m.row(3) == Some(Row { total: 2, good: 1 });
    let f = m.perfect_rows() == 0;
    let g = m.row(9).is_none() && m.seal() != 0;
    a && b && c && d && e && f && g
}

/// 语料与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let want = "dlclose corpus=14 accept=8 reject=6 count=2 unmap=4 keep=1 stale=1 match=14 rate=1000ppm\n";
    r.total == 14
        && r.accept == 8
        && r.reject == 6
        && r.count == 2
        && r.unmap == 4
        && r.keep == 1
        && r.stale == 1
        && r.matched == 14
        && r.rate() == 1000
        && r.green()
        && r.gate() == Ok(())
        && r.render().as_str() == want
        && cover(CORPUS) == Ok(())
        && cover(13) == Err(E_DLCOUNT)
}

/// 红档显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929 && !r.green() && r.gate() == Err(E_DLCOUNT);
    let b = r.accept == 7 && r.reject == 7;
    let c = run_with(Some((14, Fault::Code))).is_err();
    let d = run_with(Some((5, Fault::Note))).map(|x| x.matched) == Ok(13);
    a && b && c && d
}

/// 文案逐字。
fn message_ok() -> bool {
    let mut im = setup_stale();
    let a = dlclose(&mut im, 0);
    let ok1 = message(&a, "liba.so").as_str() == "liba.so: handle already closed\n";
    let mut bad = Image::new();
    let _ = bad.load("liba.so", 1, 0, false, false);
    let b = dlclose(&mut bad, 3);
    let ok2 = message(&b, "liba.so").as_str() == "liba.so: invalid handle\n";
    let mut zero = setup_stale();
    if let Some(l) = zero.get_mut(0) {
        l.state = State::Mapped;
        l.refs = 0;
    }
    let c = dlclose(&mut zero, 0);
    let ok3 = message(&c, "liba.so").as_str() == "liba.so: refcount underflow\n";
    let mut okc = setup_unmap();
    let d = dlclose(&mut okc, 0);
    let ok4 = message(&d, "liba.so").is_empty() && explain(&d) == "unloaded";
    let ok5 = explain(&Outcome { note: Note::KeptNodelete, ..Outcome::blank() })
        == "NODELETE keeps image mapped";
    let ok6 = explain(&Outcome { note: Note::Chain, ..Outcome::blank() })
        == "unloaded with dependencies";
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
        "close-count",
        "close-unmap",
        "close-nodelete",
        "dep-release",
        "chain-unload",
        "stale-close",
        "tls-dtor",
        "reload-fresh",
        "count-as-unload",
        "nodelete-as-unmapped",
        "dep-no-refs",
        "chain-order-reversed",
        "stale-as-success",
        "reload-stale-handle",
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
    case_at(0).is_some() && case_at(CORPUS).is_none() && case_at(9).map(|c| c.tag()) == Some("nodelete-as-unmapped")
}

/// 释义逐字。
const DETAIL_COUNT: &str = "liba.so stage=count code=0 refs=1 deps=0 mapped=yes dtors=0 order=0x0 note=count-down\n";

/// 卸载链释义。
const DETAIL_CHAIN: &str = "liba.so stage=ready code=0 refs=0 deps=0 mapped=no dtors=3 order=0x201 note=chain\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_COUNT,
        None => false,
    };
    let b = match cs.get(4) {
        Some(c) => detail(c).as_str() == DETAIL_CHAIN,
        None => false,
    };
    let d = match cs.get(5) {
        Some(c) => {
            let o = c.actual();
            o.code == E_DLSTALE && o.stage == Stage::Handle && !o.mapped
        }
        None => false,
    };
    let e = match cs.get(7) {
        Some(c) => {
            let o = c.actual();
            o.refs == 1 && o.mapped && o.note == Note::Reloaded
        }
        None => false,
    };
    a && b && d && e
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Count) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Handle) == Err(E_DLORDER);
    let d = g.step(Stage::Deps) == Ok(()) && g.last() == 2;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(E_DLSTATE);
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
pub fn run_unx11636_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11636_dlclose_refs");

    s.add("DC01-名称与段基址", name_ok(), "static");

    s.add("DC02-条目与句柄表", lib_ok(), "static");

    s.add("DC03-引用计数", refs_ok(), "static");

    s.add("DC04-卸载链", chain_ok(), "static");

    s.add("DC05-常驻与重载", nodelete_ok(), "static");

    s.add("DC06-申报对判", judge_ok(), "static");

    s.add("DC07-矩阵统计", matrix_ok(), "static");

    s.add("DC08-语料与渲染", corpus_ok(), "static");

    s.add("DC09-红档显式", reds_ok(), "static");

    s.add("DC10-文案逐字", message_ok(), "static");

    s.add("DC11-槽号与档位", slots_ok(), "static");

    s.add("DC12-释义与账本", detail_ok() && ledger_ok() && gate_ok(), "static");

    s.add(
        "DC13-冻结重放四注入",
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

    /// 测试内断言失败。
    fn fail() -> ! {
        panic!("unx11636 test fail")
    }

    #[test]
    fn seg_and_codes() {
        assert_eq!(SEG_BASE, 0xfcd0);
        assert_eq!(E_DLHANDLE, SEG_BASE + 0x01);
        assert_eq!(E_DLSTAGE, SEG_BASE + 0x0d);
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(CAP, 6);
        assert_eq!(TIER_COUNT, 4);
        assert!(name_ok());
    }

    #[test]
    fn fnv_is_stable() {
        let a = fold_str(FNV_OFF, "liba.so");
        let b = fold_str(FNV_OFF, "liba.so");
        let c = fold_str(FNV_OFF, "libb.so");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(fold_word(FNV_OFF, 0), fold_word(FNV_OFF, 0));
        assert_ne!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 2));
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0, 1).as_str(), "0x0");
        assert_eq!(hex_text(0x201, 1).as_str(), "0x201");
        assert_eq!(hex_text(0x1, 2).as_str(), "0x01");
        assert_eq!(bool_text(true).as_str(), "yes");
        assert_eq!(bool_text(false).as_str(), "no");
    }

    #[test]
    fn state_mapping() {
        assert!(State::Mapped.mapped());
        assert!(State::Kept.mapped());
        assert!(!State::Free.mapped());
        assert_eq!(State::Mapped.name(), "mapped");
    }

    #[test]
    fn lib_blank_and_link() {
        let l = Lib::blank();
        assert_eq!(l.refs, 0);
        assert!(!l.mapped());
        let mut live = Lib::live("liba.so", 2);
        assert!(live.link(1));
        assert_eq!(live.dep_list(), &[1u8]);
        assert!(live.link(2));
        assert!(!live.link(3));
        assert_eq!(live.ndep, 2);
        assert!(Lib::resident("libb.so", 1).nodelete);
    }

    #[test]
    fn image_load_and_find() {
        let mut im = Image::new();
        assert!(im.is_empty());
        assert_eq!(im.load("liba.so", 1, 0, false, false), Some(0));
        assert_eq!(im.load("libb.so", 2, 1, false, false), Some(1));
        assert_eq!(im.find("liba.so"), Some(0));
        assert_eq!(im.find("zz"), None);
        assert_eq!(im.refs_at(1), 2);
        assert_eq!(im.seq_pos(1), 1);
        assert_eq!(im.seq_pos(9), -1);
    }

    #[test]
    fn image_capacity() {
        let mut im = Image::new();
        let mut i = 0usize;
        while i < CAP {
            assert!(im.load("lib.so", 1, 0, false, false).is_some());
            i += 1;
        }
        assert_eq!(im.len(), CAP);
        assert_eq!(im.load("over.so", 1, 0, false, false), None);
        assert_eq!(im.get(CAP), None);
    }

    #[test]
    fn close_count_down() {
        let mut im = setup_count_down();
        let o = dlclose(&mut im, 0);
        assert_eq!(o.stage, Stage::Count);
        assert!(o.ok());
        assert_eq!(o.refs, 1);
        assert!(o.mapped);
        assert_eq!(o.dtors, 0);
        assert_eq!(o.order, 0);
        assert_eq!(o.note, Note::CountDown);
    }

    #[test]
    fn close_unmap_runs_dtors() {
        let mut im = setup_unmap();
        let o = dlclose(&mut im, 0);
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.dtors, 2);
        assert_eq!(o.order, 0x1);
        assert!(!o.mapped);
        assert_eq!(im.state_at(0), State::Free);
    }

    #[test]
    fn close_nodelete_keeps() {
        let mut im = setup_nodelete();
        let o = dlclose(&mut im, 0);
        assert_eq!(o.stage, Stage::Count);
        assert_eq!(o.refs, 0);
        assert!(o.mapped);
        assert_eq!(o.dtors, 0);
        assert_eq!(o.note, Note::KeptNodelete);
        assert_eq!(im.state_at(0), State::Kept);
    }

    #[test]
    fn close_dep_release() {
        let mut im = setup_dep();
        let o = dlclose(&mut im, 0);
        assert_eq!(o.note, Note::DepDown);
        assert_eq!(o.deps, 1);
        assert_eq!(o.dtors, 1);
        assert_eq!(im.refs_at(1), 1);
        assert_eq!(im.state_at(1), State::Mapped);
    }

    #[test]
    fn close_chain_reverse_order() {
        let mut im = setup_chain();
        let o = dlclose(&mut im, 0);
        assert_eq!(o.note, Note::Chain);
        assert_eq!(o.dtors, 3);
        assert_eq!(o.order, 0x201);
        assert_eq!(o.deps, 0);
        assert_eq!(im.state_at(0), State::Free);
        assert_eq!(im.state_at(1), State::Free);
    }

    #[test]
    fn close_stale() {
        let mut im = setup_stale();
        let o = dlclose(&mut im, 0);
        assert_eq!(o.stage, Stage::Handle);
        assert_eq!(o.code, E_DLSTALE);
        assert_eq!(o.note, Note::StaleHandle);
    }

    #[test]
    fn close_bad_handle() {
        let mut im = setup_unmap();
        let o = dlclose(&mut im, 5);
        assert_eq!(o.stage, Stage::Handle);
        assert_eq!(o.code, E_DLHANDLE);
        assert_eq!(o.note, Note::BadHandle);
    }

    #[test]
    fn close_zero_refs() {
        let mut im = setup_stale();
        if let Some(l) = im.get_mut(0) {
            l.state = State::Mapped;
            l.refs = 0;
        }
        let o = dlclose(&mut im, 0);
        assert_eq!(o.code, E_DLZERO);
        assert_eq!(o.note, Note::ZeroRefs);
    }

    #[test]
    fn close_tls_postponed() {
        let mut im = setup_tls();
        let o = dlclose(&mut im, 0);
        assert!(o.tls);
        assert_eq!(o.dtors, 1);
    }

    #[test]
    fn reload_resets_refs() {
        let mut im = setup_reload();
        let o = dlclose_reload(&mut im, 0);
        assert_eq!(o.note, Note::Reloaded);
        assert_eq!(o.refs, 1);
        assert!(o.mapped);
        assert_eq!(im.refs_at(0), 1);
        assert_eq!(im.state_at(0), State::Mapped);
    }

    #[test]
    fn reload_of_stale_fails() {
        let mut im = setup_stale();
        let o = dlclose_reload(&mut im, 0);
        assert_eq!(o.code, E_DLSTALE);
    }

    #[test]
    fn claims_match_all_slots() {
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => fail(),
            };
            assert_eq!(judge(&c), expected(i).unwrap_or(Fault::Note));
            i += 1;
        }
    }

    #[test]
    fn expected_table() {
        assert_eq!(expected(0), Some(Fault::Ok));
        assert_eq!(expected(7), Some(Fault::Ok));
        assert_eq!(expected(8), Some(Fault::Mapped));
        assert_eq!(expected(9), Some(Fault::Mapped));
        assert_eq!(expected(10), Some(Fault::Deps));
        assert_eq!(expected(11), Some(Fault::Order));
        assert_eq!(expected(12), Some(Fault::Estate));
        assert_eq!(expected(13), Some(Fault::Refs));
        assert_eq!(expected(14), None);
    }

    #[test]
    fn reject_codes_align() {
        let cs = cases();
        let mut i = 0usize;
        while i < REJECT {
            let c = match cs.get(ACCEPT + i) {
                Some(v) => *v,
                None => fail(),
            };
            assert_eq!(judge(&c).code(), REJECT_CODES[i]);
            i += 1;
        }
    }

    #[test]
    fn corpus_runs_green() {
        let r = run().unwrap_or_else(|_| fail());
        assert_eq!(r.total, 14);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert!(r.green());
    }

    #[test]
    fn red_rate_929() {
        let r = run_with(Some((0, Fault::Code))).unwrap_or_else(|_| fail());
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert!(!r.green());
    }

    #[test]
    fn render_exact() {
        let r = run().unwrap_or_else(|_| fail());
        assert_eq!(
            r.render().as_str(),
            "dlclose corpus=14 accept=8 reject=6 count=2 unmap=4 keep=1 stale=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn matrix_shape() {
        let m = Matrix::of();
        assert!(m.balanced());
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert_eq!(m.row(0), Some(Row { total: 4, good: 2 }));
        assert_eq!(m.row(1), Some(Row { total: 6, good: 4 }));
        assert_eq!(m.perfect_rows(), 0);
    }

    #[test]
    fn detail_lines_exact() {
        let a = case_at(0).unwrap_or_else(|| fail());
        let b = case_at(4).unwrap_or_else(|| fail());
        assert_eq!(detail(&a).as_str(), DETAIL_COUNT);
        assert_eq!(detail(&b).as_str(), DETAIL_CHAIN);
    }

    #[test]
    fn message_lines_exact() {
        assert!(message_ok());
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
        let root = l.freeze().unwrap_or_else(|_| fail());
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
        let s = run_unx11636_checks();
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
    fn setups_all_construct() {
        let mut i = 0usize;
        while i < SETUPS.len() {
            let im = (SETUPS[i])();
            assert!(im.len() >= 1);
            i += 1;
        }
    }

    #[test]
    fn held_refs_counts_others() {
        let im = setup_dep();
        assert_eq!(im.held_refs(None), 3);
        assert_eq!(im.held_refs(Some(0)), 2);
        assert_eq!(im.held_refs(Some(1)), 1);
    }

    #[test]
    fn cover_bounds() {
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(0), Err(E_DLCOUNT));
        assert!(case_at(13).is_some());
        assert!(case_at(14).is_none());
    }
}
