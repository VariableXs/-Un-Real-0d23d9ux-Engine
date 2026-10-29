//! UNX-F11248 符号解析全局序与 interposition（0x1f300 段）
//!
//! 锚点：UNX-F11248-J1 全局序（preload→主程序→按需逐层）解析判据绿，符号
//! 插装场景行为与 Linux 一致。
//!
//! 纯功能模型：动态链接器的全局作用域是一条**有序链**。链首是 LD_PRELOAD 的
//! 对象（按环境串原序），随后是主程序，再往后是 DT_NEEDED 按装载序展开的
//! 依赖。查一个符号就从链首走到链尾，**第一个命中即返回**——所以 preload 里
//! 的定义会遮蔽主程序和 libc 里的同名定义，这就是 interposition（符号插装）。
//!
//! 规则面：对象插入受序约束——preload 只能在主程序之前入场，主程序只能有一个，
//! NEEDED 只能跟在主程序之后；符号定义只在封表前可写，地址为零拒，同一对象
//! 内重名拒；封表要求主程序在场。
//!
//! 弱符号：现代语义下弱定义同样参与「首定义即命中」。模块另提供
//! `lookup_strong_only` 复刻 LD_DYNAMIC_WEAK 的旧行为：跳过弱定义继续找强定义，
//! 全链只有弱定义时返回 EWEAK。这是唯一一处「顺序不是唯一裁决者」的场景。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f300;
/// 对象名或符号名为空
pub const SC_ENAME: u32 = SEG_BASE + 0x01;
/// 全局序错（preload 晚于主程序 / NEEDED 早于主程序 / 未封表即查）
pub const SC_EORDER: u32 = SEG_BASE + 0x02;
/// 对象重号或同对象符号重名
pub const SC_EDUP: u32 = SEG_BASE + 0x03;
/// 主程序重复或缺失
pub const SC_EMAIN: u32 = SEG_BASE + 0x04;
/// 对象表或符号表超容
pub const SC_EFULL: u32 = SEG_BASE + 0x05;
/// 空链或重放摘要不符
pub const SC_ESTATE: u32 = SEG_BASE + 0x06;
/// 冻结后改写
pub const SC_EFROZEN: u32 = SEG_BASE + 0x07;
/// 符号全链未定义
pub const SC_ENOENT: u32 = SEG_BASE + 0x08;
/// 符号地址为零
pub const SC_EADDR: u32 = SEG_BASE + 0x09;
/// 全链只有弱定义（强查拒绝）
pub const SC_EWEAK: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 对象链容量
pub const OBJ_CAP: usize = 6;
/// 单对象符号容量
pub const SYM_CAP: usize = 4;
/// 对象类：preload
pub const OBJ_PRELOAD: u8 = 0;
/// 对象类：主程序
pub const OBJ_MAIN: u8 = 1;
/// 对象类：DT_NEEDED 依赖
pub const OBJ_NEEDED: u8 = 2;
/// 状态：建场
pub const ST_BUILD: u8 = 0;
/// 状态：已封表
pub const ST_SEALED: u8 = 1;
/// 状态：冻结
pub const ST_FROZEN: u8 = 2;
const NAME_MAX: usize = 24;
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 作用域链
/// 一条符号定义
#[derive(Clone, Copy)]
pub struct SymDef {
    name: [u8; NAME_MAX],
    name_len: u8,
    addr: u32,
    weak: bool,
}

impl SymDef {
    /// 符号名
    pub fn name(&self) -> &str { str_at(&self.name, self.name_len) }
    /// 符号地址
    pub fn addr(&self) -> u32 { self.addr }
    /// 弱符号位
    pub fn weak(&self) -> bool { self.weak }
}

/// 链上一个对象
#[derive(Clone, Copy)]
pub struct ScopeObj {
    name: [u8; NAME_MAX],
    name_len: u8,
    kind: u8,
    syms: [Option<SymDef>; SYM_CAP],
    sym_count: usize,
}

impl ScopeObj {
    /// 对象名
    pub fn name(&self) -> &str { str_at(&self.name, self.name_len) }
    /// 对象类（preload / main / needed）
    pub fn kind(&self) -> u8 { self.kind }
    /// 符号条数
    pub fn sym_count(&self) -> usize { self.sym_count }
    /// 第 i 条符号
    pub fn sym(&self, i: usize) -> Option<SymDef> {
        if i >= self.sym_count { return None; }
        self.syms[i]
    }
}

/// 全局作用域链
pub struct ScopeChain {
    objs: [Option<ScopeObj>; OBJ_CAP],
    count: usize,
    state: u8,
    root: Option<u64>,
    /// 查符号次数
    pub lookups: u32,
    /// 命中次数
    pub hits: u32,
    /// 落空次数
    pub misses: u32,
    /// 弱定义首命中次数
    pub weak_hits: u32,
    /// 遮蔽次数（命中后仍有对象定义同名符号）
    pub interposed: u32,
    /// 定义成功次数
    pub defines: u32,
    /// 拒绝次数
    pub rejects: u32,
}

impl ScopeChain {
    /// 空链
    pub fn new() -> Self {
        ScopeChain {
            objs: [None; OBJ_CAP],
            count: 0,
            state: ST_BUILD,
            root: None,
            lookups: 0,
            hits: 0,
            misses: 0,
            weak_hits: 0,
            interposed: 0,
            defines: 0,
            rejects: 0,
        }
    }

    fn bump(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 当前状态
    pub fn stage(&self) -> u8 { self.state }
    /// 链长
    pub fn count(&self) -> usize { self.count }
    /// 第 i 个对象
    pub fn obj(&self, i: usize) -> Option<ScopeObj> {
        if i >= self.count { return None; }
        self.objs[i]
    }
    /// 主程序槽位
    pub fn main_slot(&self) -> Option<u8> {
        let mut i = 0;
        while i < self.count {
            if let Some(o) = self.objs[i] {
                if o.kind == OBJ_MAIN { return Some(i as u8); }
            }
            i += 1;
        }
        None
    }

    /// 入链：preload 必须在主程序前，主程序唯一，NEEDED 必须在主程序后
    pub fn add_object(&mut self, kind: u8, name: &str) -> Result<u8, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(SC_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(SC_EORDER)); }
        if name.as_bytes().is_empty() { return Err(self.bump(SC_ENAME)); }
        if kind > OBJ_NEEDED { return Err(self.bump(SC_EORDER)); }
        let has_main = self.main_slot().is_some();
        match kind {
            OBJ_PRELOAD => if has_main { return Err(self.bump(SC_EORDER)); },
            OBJ_MAIN => if has_main { return Err(self.bump(SC_EMAIN)); },
            OBJ_NEEDED => if !has_main { return Err(self.bump(SC_EORDER)); },
            _ => {}
        }
        let mut i = 0;
        while i < self.count {
            if let Some(o) = self.objs[i] {
                if o.name() == name { return Err(self.bump(SC_EDUP)); }
            }
            i += 1;
        }
        if self.count >= OBJ_CAP { return Err(self.bump(SC_EFULL)); }
        let mut row = ScopeObj {
            name: [0u8; NAME_MAX],
            name_len: 0,
            kind,
            syms: [None; SYM_CAP],
            sym_count: 0,
        };
        row.name_len = match put(&mut row.name, name) {
            Some(n) => n,
            None => return Err(self.bump(SC_EFULL)),
        };
        match self.objs.get_mut(self.count) {
            Some(slot) => *slot = Some(row),
            None => return Err(self.bump(SC_EFULL)),
        }
        self.count = self.count.wrapping_add(1);
        Ok(self.count as u8)
    }

    /// 在封表前给对象写一条符号定义
    pub fn define(&mut self, obj: usize, name: &str, addr: u32, weak: bool) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(SC_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(SC_EORDER)); }
        if name.as_bytes().is_empty() { return Err(self.bump(SC_ENAME)); }
        if addr == 0 { return Err(self.bump(SC_EADDR)); }
        let snap = match self.objs.get(obj) {
            Some(Some(o)) => *o,
            _ => return Err(self.bump(SC_ESTATE)),
        };
        if snap.sym_count >= SYM_CAP { return Err(self.bump(SC_EFULL)); }
        let mut i = 0;
        while i < snap.sym_count {
            if let Some(Some(s)) = snap.syms.get(i) {
                if s.name() == name { return Err(self.bump(SC_EDUP)); }
            }
            i += 1;
        }
        let mut d = SymDef {
            name: [0u8; NAME_MAX],
            name_len: 0,
            addr,
            weak,
            };
        d.name_len = match put(&mut d.name, name) {
            Some(n) => n,
            None => return Err(self.bump(SC_EFULL)),
        };
        let stored = match self.objs.get_mut(obj) {
            Some(Some(o)) => {
                let idx = o.sym_count;
                match o.syms.get_mut(idx) {
                    Some(slot) => {
                        *slot = Some(d);
                        o.sym_count = idx + 1;
                        true
                    }
                    None => false,
                }
            }
            _ => false,
        };
        if !stored { return Err(self.bump(SC_EFULL)); }
        self.defines = self.defines.wrapping_add(1);
        Ok(snap.sym_count as u32 + 1)
    }

    /// 封表：要求非空且主程序在场
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(SC_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(SC_EORDER)); }
        if self.count == 0 { return Err(self.bump(SC_ESTATE)); }
        if self.main_slot().is_none() { return Err(self.bump(SC_EMAIN)); }
        self.state = ST_SEALED;
        Ok(self.count as u32)
    }

    /// 全局序查找：链首第一个定义即命中（可用 preload 遮蔽）
    pub fn lookup(&mut self, name: &str) -> Result<(u8, u32), u32> {
        if self.state == ST_FROZEN { return Err(self.bump(SC_EFROZEN)); }
        if self.state != ST_SEALED { return Err(self.bump(SC_EORDER)); }
        if name.as_bytes().is_empty() { return Err(self.bump(SC_ENAME)); }
        self.lookups = self.lookups.wrapping_add(1);
        let mut i = 0;
        while i < self.count {
            if let Some(o) = self.objs[i] {
                if let Some(s) = find_in(&o, name) {
                    self.hits = self.hits.wrapping_add(1);
                    if s.weak { self.weak_hits = self.weak_hits.wrapping_add(1); }
                    let mut k = i + 1;
                    let mut shadow = false;
                    while k < self.count {
                        if let Some(o2) = self.objs[k] {
                            if find_in(&o2, name).is_some() { shadow = true; }
                        }
                        k += 1;
                    }
                    if shadow { self.interposed = self.interposed.wrapping_add(1); }
                    return Ok((i as u8, s.addr));
                }
            }
            i += 1;
        }
        self.misses = self.misses.wrapping_add(1);
        Err(self.bump(SC_ENOENT))
    }

    /// 强定义查找（LD_DYNAMIC_WEAK 旧语义）：跳过弱定义，只有弱定义时报 EWEAK
    pub fn lookup_strong_only(&mut self, name: &str) -> Result<(u8, u32), u32> {
        if self.state == ST_FROZEN { return Err(self.bump(SC_EFROZEN)); }
        if self.state != ST_SEALED { return Err(self.bump(SC_EORDER)); }
        if name.as_bytes().is_empty() { return Err(self.bump(SC_ENAME)); }
        self.lookups = self.lookups.wrapping_add(1);
        let mut weak_seen = false;
        let mut i = 0;
        while i < self.count {
            if let Some(o) = self.objs[i] {
                if let Some(s) = find_in(&o, name) {
                    if !s.weak {
                        self.hits = self.hits.wrapping_add(1);
                        return Ok((i as u8, s.addr));
                    }
                    weak_seen = true;
                }
            }
            i += 1;
        }
        self.misses = self.misses.wrapping_add(1);
        if weak_seen { Err(self.bump(SC_EWEAK)) } else { Err(self.bump(SC_ENOENT)) }
    }

    /// 纯查询：某对象里的符号地址
    pub fn addr_in(&self, obj: usize, name: &str) -> Option<u32> {
        match self.objs.get(obj) {
            Some(Some(o)) => match find_in(o, name) {
                Some(s) => Some(s.addr),
                None => None,
            },
            _ => None,
        }
    }
}

fn find_in(o: &ScopeObj, name: &str) -> Option<SymDef> {
    let mut i = 0;
    while i < o.sym_count {
        if let Some(Some(s)) = o.syms.get(i) {
            if s.name() == name { return Some(*s); }
        }
        i += 1;
    }
    None
}

fn put(dst: &mut [u8], s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() > dst.len() { return None; }
    let mut i = 0;
    while i < b.len() { dst[i] = b[i]; i += 1; }
    Some(b.len() as u8)
}

fn str_at(buf: &[u8], len: u8) -> &str {
    let n = len as usize;
    if n > buf.len() { return ""; }
    match core::str::from_utf8(&buf[..n]) {
        Ok(s) => s,
        Err(_) => "",
    }
}

fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

fn mix_str(h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut x = h;
    let mut i = 0;
    while i < b.len() {
        x = mix(x, b[i] as u64);
        i += 1;
    }
    x
}

impl ScopeChain {
    /// FNV-1a 摘要：链上对象、符号与账本入账
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.count {
            if let Some(o) = self.objs[i] {
                h = mix_str(h, o.name());
                h = mix(h, o.kind() as u64);
                let mut k = 0;
                while k < o.sym_count() {
                    if let Some(Some(s)) = o.syms.get(k) {
                        h = mix_str(h, s.name());
                        h = mix(h, s.addr() as u64);
                        h = mix(h, if s.weak() { 1 } else { 0 });
                    }
                    k += 1;
                }
                h = mix(h, o.sym_count() as u64);
            }
            i += 1;
        }
        h = mix(h, self.count as u64);
        h = mix(h, self.lookups as u64);
        h = mix(h, self.hits as u64);
        h = mix(h, self.misses as u64);
        h = mix(h, self.weak_hits as u64);
        h = mix(h, self.interposed as u64);
        h = mix(h, self.defines as u64);
        h = mix(h, self.rejects as u64);
        h
    }

    /// 冻结：只在已封表态成立，幂等
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            if let Some(r) = self.root { return Ok(r); }
        }
        if self.state != ST_SEALED { return Err(self.bump(SC_EORDER)); }
        if self.count == 0 { return Err(self.bump(SC_ESTATE)); }
        self.state = ST_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 重放：摘要必须与冻结时一致
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.state != ST_FROZEN { return Err(self.bump(SC_EORDER)); }
        let v = self.fold();
        match self.root {
            Some(r) if r == v => Ok(v),
            _ => Err(self.bump(SC_ESTATE)),
        }
    }
}

/// 未封表样例：preload + 主程序 + 若干定义
pub fn build_unsealed() -> ScopeChain {
    let mut c = ScopeChain::new();
    let _ = c.add_object(OBJ_PRELOAD, "libhook.so");
    let _ = c.add_object(OBJ_MAIN, "a.out");
    let _ = c.define(0, "malloc", 0x1000, false);
    let _ = c.define(1, "main", 0x400100, false);
    c
}

/// 完整样例：preload、主程序、libc、libm、空依赖
pub fn build_chain() -> ScopeChain {
    let mut c = ScopeChain::new();
    let _ = c.add_object(OBJ_PRELOAD, "libhook.so");
    let _ = c.add_object(OBJ_MAIN, "a.out");
    let _ = c.add_object(OBJ_NEEDED, "libc.so.6");
    let _ = c.add_object(OBJ_NEEDED, "libm.so.6");
    let _ = c.add_object(OBJ_NEEDED, "libpthread.so.0");
    let _ = c.define(0, "malloc", 0x1000, false);
    let _ = c.define(0, "fopen", 0x1100, false);
    let _ = c.define(0, "memcpy", 0x1200, true);
    let _ = c.define(0, "wonly", 0x1300, true);
    let _ = c.define(1, "malloc", 0x400000, false);
    let _ = c.define(1, "main", 0x400100, false);
    let _ = c.define(1, "getopt", 0x400200, false);
    let _ = c.define(2, "malloc", 0x7f0000, false);
    let _ = c.define(2, "printf", 0x7f0020, false);
    let _ = c.define(2, "fopen", 0x7f0040, false);
    let _ = c.define(2, "memcpy", 0x7f0080, false);
    let _ = c.define(3, "sin", 0x7e0000, false);
    let _ = c.seal();
    c
}

/// 四路独立注入：每条都在新实例上证明一条拒绝路径会触发
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];

    let mut a = ScopeChain::new();
    match a.add_object(OBJ_NEEDED, "libc.so.6") {
        Err(code) if code == SC_EORDER => { ok[0] = a.rejects == 1; }
        _ => {}
    }

    let mut b = ScopeChain::new();
    let _ = b.add_object(OBJ_MAIN, "a.out");
    match b.add_object(OBJ_MAIN, "a.out2") {
        Err(code) if code == SC_EMAIN => { ok[1] = b.rejects == 1; }
        _ => {}
    }

    let mut c = ScopeChain::new();
    let _ = c.add_object(OBJ_MAIN, "a.out");
    match c.define(0, "main", 0, false) {
        Err(code) if code == SC_EADDR => { ok[2] = c.rejects == 1; }
        _ => {}
    }

    let mut d = build_unsealed();
    match d.lookup("malloc") {
        Err(code) if code == SC_EORDER => { ok[3] = d.rejects == 1 && d.lookups == 0; }
        _ => {}
    }

    ok
}

/// 十三项静态/动态判据
pub fn run_unx11248_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11248.globalscope");

    let fresh = ScopeChain::new();
    s.add(
        "GS1-建场清零",
        fresh.stage() == ST_BUILD
            && fresh.count() == 0
            && fresh.main_slot().is_none()
            && fresh.rejects == 0,
        "static",
    );

    let chain = build_chain();
    let kinds_ok = match chain.obj(0) {
        Some(o) => o.kind() == OBJ_PRELOAD,
        None => false,
    } && match chain.obj(1) {
        Some(o) => o.kind() == OBJ_MAIN,
        None => false,
    } && match chain.obj(4) {
        Some(o) => o.kind() == OBJ_NEEDED && o.sym_count() == 0,
        None => false,
    };
    s.add(
        "GS2-三类对象入链",
        chain.count() == 5 && kinds_ok && chain.defines == 12 && chain.main_slot() == Some(1),
        "static",
    );

    let mut a = ScopeChain::new();
    let n1 = a.add_object(OBJ_MAIN, "") == Err(SC_ENAME);
    let _ = a.add_object(OBJ_MAIN, "a.out");
    let n2 = a.define(0, "", 1, false) == Err(SC_ENAME);
    s.add(
        "GS3-空名拒",
        n1 && n2 && a.rejects == 2 && a.count() == 1,
        "static",
    );

    let mut b = ScopeChain::new();
    let _ = b.add_object(OBJ_MAIN, "a.out");
    s.add(
        "GS4-preload晚于主程序拒",
        b.add_object(OBJ_PRELOAD, "libhook.so") == Err(SC_EORDER) && b.rejects == 1,
        "static",
    );

    let mut c = ScopeChain::new();
    s.add(
        "GS5-NEEDED早于主程序拒",
        c.add_object(OBJ_NEEDED, "libc.so.6") == Err(SC_EORDER) && c.rejects == 1,
        "static",
    );

    let mut d = ScopeChain::new();
    let _ = d.add_object(OBJ_MAIN, "a.out");
    s.add(
        "GS6-主程序唯一",
        d.add_object(OBJ_MAIN, "a.out2") == Err(SC_EMAIN) && d.rejects == 1,
        "static",
    );

    let mut e = ScopeChain::new();
    let _ = e.add_object(OBJ_PRELOAD, "libhook.so");
    s.add(
        "GS7-缺主程序不可封",
        e.seal() == Err(SC_EMAIN) && e.stage() == ST_BUILD && e.rejects == 1,
        "static",
    );

    let mut f = build_unsealed();
    s.add(
        "GS8-未封表不查符号",
        f.lookup("malloc") == Err(SC_EORDER) && f.lookups == 0 && f.rejects == 1,
        "static",
    );

    let mut g = build_chain();
    s.add_dyn(
        "GS9-全局序首定义命中",
        g.lookup("malloc") == Ok((0, 0x1000))
            && g.lookup("fopen") == Ok((0, 0x1100))
            && g.lookup("malloc") == Ok((0, 0x1000)),
        &format!("malloc={:?}", g.addr_in(0, "malloc")),
    );

    let mut h = build_chain();
    let _ = h.lookup("malloc");
    let _ = h.lookup("fopen");
    let _ = h.lookup("printf");
    let _ = h.lookup("sin");
    s.add_dyn(
        "GS10-插装记账",
        h.hits == 4 && h.interposed == 2 && h.addr_in(2, "printf") == Some(0x7f0020),
        &format!("hit={} inter={}", h.hits, h.interposed),
    );

    let mut w = build_chain();
    let weak_first = w.lookup("memcpy") == Ok((0, 0x1200));
    let strong_skip = w.lookup_strong_only("memcpy") == Ok((2, 0x7f0080));
    let weak_only = w.lookup_strong_only("wonly") == Err(SC_EWEAK);
    let plain_weak = w.lookup("wonly") == Ok((0, 0x1300));
    s.add(
        "GS11-弱符号双语义",
        weak_first && strong_skip && weak_only && plain_weak && w.weak_hits == 2,
        "static",
    );

    let mut z = build_chain();
    let _ = z.lookup("printf");
    let f1 = z.freeze();
    let f2 = z.freeze();
    let rp = z.replay();
    let locked_def = z.define(0, "late", 1, false) == Err(SC_EFROZEN);
    let locked_look = z.lookup("printf") == Err(SC_EFROZEN);
    let rootv = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add_dyn(
        "GS12-冻结重放幂等",
        f1.is_ok() && f1 == f2 && rp == f1 && locked_def && locked_look && z.rejects == 2,
        &format!("root={:x}", rootv),
    );

    let inj = inject_four();
    s.add(
        "GS13-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_global_order() {
        let mut c = build_chain();
        assert_eq!(c.count(), 5);
        assert_eq!(c.main_slot(), Some(1));
        assert_eq!(c.lookup("malloc"), Ok((0, 0x1000)));
        assert_eq!(c.lookup("fopen"), Ok((0, 0x1100)));
        assert_eq!(c.lookup("main"), Ok((1, 0x400100)));
        assert_eq!(c.lookup("printf"), Ok((2, 0x7f0020)));
        assert_eq!(c.lookup("sin"), Ok((3, 0x7e0000)));
        assert_eq!(c.lookup("libc_start_main"), Err(SC_ENOENT));
        assert_eq!(c.hits, 5);
        assert_eq!(c.misses, 1);
        assert_eq!(c.interposed, 2);
        assert_eq!(c.addr_in(2, "printf"), Some(0x7f0020));
        assert_eq!(c.addr_in(4, "printf"), None);
    }

    #[test]
    fn gates_exact_rejects() {
        let mut c = ScopeChain::new();
        assert_eq!(c.seal(), Err(SC_ESTATE));
        assert_eq!(c.define(0, "x", 1, false), Err(SC_ESTATE));
        assert_eq!(c.add_object(OBJ_NEEDED, "libc.so.6"), Err(SC_EORDER));
        assert_eq!(c.add_object(OBJ_MAIN, ""), Err(SC_ENAME));
        assert_eq!(c.add_object(OBJ_MAIN, "a.out"), Ok(1));
        assert_eq!(c.add_object(OBJ_MAIN, "a.out2"), Err(SC_EMAIN));
        assert_eq!(c.add_object(OBJ_PRELOAD, "libhook.so"), Err(SC_EORDER));
        assert_eq!(c.add_object(OBJ_NEEDED, "libc.so.6"), Ok(2));
        assert_eq!(c.define(0, "main", 0, false), Err(SC_EADDR));
        assert_eq!(c.define(0, "main", 0x400100, false), Ok(1));
        assert_eq!(c.define(0, "main", 0x400200, false), Err(SC_EDUP));
        assert_eq!(c.seal(), Ok(2));
        assert_eq!(c.define(0, "x", 1, false), Err(SC_EORDER));
        assert_eq!(c.lookup("nope"), Err(SC_ENOENT));
        assert!(c.freeze().is_ok());
        assert!(c.replay().is_ok());
        assert_eq!(c.add_object(OBJ_NEEDED, "libm.so.6"), Err(SC_EFROZEN));
        assert_eq!(c.lookup("nope"), Err(SC_EFROZEN));
        assert_eq!(c.replay(), Err(SC_ESTATE));
        assert_eq!(c.rejects, 13);
        assert_eq!(c.lookups, 1);
        assert_eq!(c.misses, 1);
        assert_eq!(c.hits, 0);
        assert_eq!(c.defines, 1);
    }

    #[test]
    fn rules_weak_and_order() {
        let mut c = build_chain();
        // 弱定义同样「首定义即命中」
        assert_eq!(c.lookup("memcpy"), Ok((0, 0x1200)));
        // LD_DYNAMIC_WEAK 旧语义：跳过弱，取强
        assert_eq!(c.lookup_strong_only("memcpy"), Ok((2, 0x7f0080)));
        // 全链只有弱定义：强查报 EWEAK
        assert_eq!(c.lookup_strong_only("wonly"), Err(SC_EWEAK));
        assert_eq!(c.lookup("wonly"), Ok((0, 0x1300)));
        assert_eq!(c.lookup_strong_only("getopt"), Ok((1, 0x400200)));
        assert_eq!(c.lookup_strong_only("nope"), Err(SC_ENOENT));

        let codes = [
            SC_ENAME, SC_EORDER, SC_EDUP, SC_EMAIN, SC_EFULL,
            SC_ESTATE, SC_EFROZEN, SC_ENOENT, SC_EADDR, SC_EWEAK,
        ];
        let mut i = 0;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + (i as u32) + 1);
            let mut j = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j]);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(SEG_BASE, 0x1f300);
    }

    #[test]
    fn injection_paths_fire() {
        let inj = inject_four();
        assert!(inj[0], "needed before main must be rejected");
        assert!(inj[1], "second main must be rejected");
        assert!(inj[2], "zero address definition must be rejected");
        assert!(inj[3], "lookup before seal must be rejected");
    }

    #[test]
    fn checks_green() {
        let cs = run_unx11248_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }
}
