//! UNX-F11249 lazy binding 与 PLT 消费（0x1f310 段）
//!
//! 锚点：UNX-F11249-J1 PLT 首调解析（延迟绑定）判据绿，RTLD_NOW 即时绑定
//! 切换语义一致。
//!
//! 纯功能模型：PLT 是给每个外来函数留的一张欠条。RTLD_LAZY 下装载器只把
//! GOT 槽填成「解析器跳板」，函数**第一次被调用**时才真正查符号、写回 GOT，
//! 之后再调直接走缓存——所以每个符号一生只解析一次，与调用次数无关。
//! RTLD_NOW 下欠条在 `seal` 时一次结清，任何缺符号都会让装载当场失败，
//! 而不是等到运行期某条路径踩到才炸。
//!
//! 模式来源有三处：调用方给的 RTLD_NOW、环境变量 LD_BIND_NOW、以及
//! DT_FLAGS_1 的 DF_1_NOW 位。三者**任一为真**即全局即时绑定；这是本模块
//! 唯一的「或」语义，其余判定都是严格顺序。
//!
//! 记账面：calls 只记成功调用，aborts 单独记懒绑定期缺符号的中止，misses 记
//! 解析落空，resolutions 记 GOT 写回次数（首调语义的量化证据）。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f310;
/// 符号名为空
pub const PB_ENAME: u32 = SEG_BASE + 0x01;
/// GOT 槽为零或未八字节对齐
pub const PB_EGOT: u32 = SEG_BASE + 0x02;
/// 符号重号或 GOT 槽重号
pub const PB_EDUP: u32 = SEG_BASE + 0x03;
/// 表满
pub const PB_EFULL: u32 = SEG_BASE + 0x04;
/// 阶段错（未封表即调用 / 重复封表 / 已封表改模式）
pub const PB_EORDER: u32 = SEG_BASE + 0x05;
/// 懒绑定调用期符号无法解析
pub const PB_ENOENT: u32 = SEG_BASE + 0x06;
/// 冻结后改写
pub const PB_EFROZEN: u32 = SEG_BASE + 0x07;
/// 空表或重放摘要不符
pub const PB_ESTATE: u32 = SEG_BASE + 0x08;
/// 对已绑定槽重复绑定
pub const PB_EBIND: u32 = SEG_BASE + 0x09;
/// 即时绑定加载期缺符号
pub const PB_ELOAD: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// GOT 槽容量
pub const GOT_CAP: usize = 6;
/// 解析提供者表容量
pub const PROVIDER_CAP: usize = 4;
/// 相位：建场
pub const ST_BUILD: u8 = 0;
/// 相位：已封表（懒绑定待用）
pub const ST_READY: u8 = 1;
/// 相位：全部绑定（即时绑定完成）
pub const ST_BOUND: u8 = 2;
/// 相位：冻结
pub const ST_FROZEN: u8 = 3;
const NAME_MAX: usize = 24;
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- PLT 表
/// 一张 PLT 欠条
#[derive(Clone, Copy)]
pub struct PltSlot {
    name: [u8; NAME_MAX],
    name_len: u8,
    got: u32,
    addr: u32,
    calls: u32,
    bound: bool,
}

impl PltSlot {
    /// 符号名
    pub fn name(&self) -> &str { str_at(&self.name, self.name_len) }
    /// GOT 槽地址
    pub fn got(&self) -> u32 { self.got }
    /// 已绑定的目标地址（未绑定为零）
    pub fn addr(&self) -> u32 { self.addr }
    /// 经此槽的调用次数
    pub fn calls(&self) -> u32 { self.calls }
    /// 是否已解析
    pub fn is_bound(&self) -> bool { self.bound }
}

/// PLT / GOT 绑定表
pub struct PltTable {
    slots: [Option<PltSlot>; GOT_CAP],
    count: usize,
    state: u8,
    rtld_now: bool,
    env_now: bool,
    df1_now: bool,
    root: Option<u64>,
    /// 成功调用次数
    pub calls: u32,
    /// GOT 写回次数
    pub resolutions: u32,
    /// 解析落空次数
    pub misses: u32,
    /// 懒绑定期缺符号的中止次数
    pub aborts: u32,
    /// 拒绝次数
    pub rejects: u32,
    /// 即时绑定时一次结清的条数
    pub loads: u32,
}

impl PltTable {
    /// 空表
    pub fn new() -> Self {
        PltTable {
            slots: [None; GOT_CAP],
            count: 0,
            state: ST_BUILD,
            rtld_now: false,
            env_now: false,
            df1_now: false,
            root: None,
            calls: 0,
            resolutions: 0,
            misses: 0,
            aborts: 0,
            rejects: 0,
            loads: 0,
        }
    }

    fn bump(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 当前相位
    pub fn stage(&self) -> u8 { self.state }
    /// 槽数
    pub fn count(&self) -> usize { self.count }
    /// 第 i 个槽（值拷贝）
    pub fn slot(&self, i: usize) -> Option<PltSlot> {
        if i >= self.count { return None; }
        self.peek(i)
    }

    fn peek(&self, i: usize) -> Option<PltSlot> {
        match self.slots.get(i) {
            Some(v) => *v,
            None => None,
        }
    }
    /// 未绑定槽数
    pub fn pending(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(s) = self.peek(i) {
                if !s.bound { n += 1; }
            }
            i += 1;
        }
        n
    }
    /// 生效绑定模式：三处来源任一为真即即时绑定
    pub fn effective_now(&self) -> bool {
        self.rtld_now || self.env_now || self.df1_now
    }

    /// 设置三处绑定模式来源；只能在封表前
    pub fn set_flags(&mut self, rtld_now: bool, env_now: bool, df1_now: bool) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PB_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(PB_EORDER)); }
        self.rtld_now = rtld_now;
        self.env_now = env_now;
        self.df1_now = df1_now;
        Ok(if self.effective_now() { 1 } else { 0 })
    }

    /// 登记一条 PLT 欠条：符号名唯一、GOT 槽唯一且八字节对齐
    pub fn add(&mut self, name: &str, got: u32) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PB_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(PB_EORDER)); }
        if name.as_bytes().is_empty() { return Err(self.bump(PB_ENAME)); }
        if got == 0 || got % 8 != 0 { return Err(self.bump(PB_EGOT)); }
        let mut i = 0;
        while i < self.count {
            if let Some(s) = self.peek(i) {
                if s.name() == name || s.got == got { return Err(self.bump(PB_EDUP)); }
            }
            i += 1;
        }
        if self.count >= GOT_CAP { return Err(self.bump(PB_EFULL)); }
        let mut row = PltSlot {
            name: [0u8; NAME_MAX],
            name_len: 0,
            got,
            addr: 0,
            calls: 0,
            bound: false,
        };
        row.name_len = match put(&mut row.name, name) {
            Some(n) => n,
            None => return Err(self.bump(PB_EFULL)),
        };
        match self.slots.get_mut(self.count) {
            Some(slot) => *slot = Some(row),
            None => return Err(self.bump(PB_EFULL)),
        }
        self.count = self.count.wrapping_add(1);
        Ok(self.count as u32)
    }

    /// 封表：即时绑定在此一次结清，懒绑定只把相位推到 READY
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PB_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(PB_EORDER)); }
        if self.count == 0 { return Err(self.bump(PB_ESTATE)); }
        if self.effective_now() {
            let mut addrs = [0u32; GOT_CAP];
            let mut i = 0;
            while i < self.count {
                let snap = match self.peek(i) {
                    Some(s) => s,
                    None => return Err(self.bump(PB_ESTATE)),
                };
                match provider(snap.name()) {
                    Some(a) => addrs[i] = a,
                    None => return Err(self.bump(PB_ELOAD)),
                }
                i += 1;
            }
            let mut k = 0;
            while k < self.count {
                if let Some(Some(s)) = self.slots.get_mut(k) {
                    s.addr = addrs[k];
                    s.bound = true;
                }
                k += 1;
            }
            self.resolutions = self.resolutions.wrapping_add(self.count as u32);
            self.loads = self.count as u32;
            self.state = ST_BOUND;
            Ok(self.count as u32)
        } else {
            self.state = ST_READY;
            Ok(self.count as u32)
        }
    }

    /// 调用一个 PLT 符号：懒绑定下首调解析，之后走缓存
    pub fn call(&mut self, name: &str) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PB_EFROZEN)); }
        if self.state == ST_BUILD { return Err(self.bump(PB_EORDER)); }
        if name.as_bytes().is_empty() { return Err(self.bump(PB_ENAME)); }
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.count {
            if let Some(s) = self.peek(i) {
                if s.name() == name { idx = Some(i); }
            }
            i += 1;
        }
        let at = match idx {
            Some(x) => x,
            None => {
                self.misses = self.misses.wrapping_add(1);
                return Err(self.bump(PB_ENOENT));
            }
        };
        let snap = match self.peek(at) {
            Some(s) => s,
            None => return Err(self.bump(PB_ESTATE)),
        };
        if snap.bound {
            self.calls = self.calls.wrapping_add(1);
            if let Some(Some(s)) = self.slots.get_mut(at) {
                s.calls = s.calls.wrapping_add(1);
            }
            return Ok(snap.addr);
        }
        match provider(snap.name()) {
            None => {
                self.misses = self.misses.wrapping_add(1);
                self.aborts = self.aborts.wrapping_add(1);
                Err(self.bump(PB_ENOENT))
            }
            Some(a) => {
                if let Some(Some(s)) = self.slots.get_mut(at) {
                    s.addr = a;
                    s.bound = true;
                    s.calls = s.calls.wrapping_add(1);
                }
                self.calls = self.calls.wrapping_add(1);
                self.resolutions = self.resolutions.wrapping_add(1);
                Ok(a)
            }
        }
    }

    /// 撤销一次绑定，使下次调用重新解析；未绑定槽报 EBIND
    pub fn rebind(&mut self, name: &str) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PB_EFROZEN)); }
        if self.state == ST_BUILD { return Err(self.bump(PB_EORDER)); }
        let mut at: Option<usize> = None;
        let mut i = 0;
        while i < self.count {
            if let Some(s) = self.peek(i) {
                if s.name() == name { at = Some(i); }
            }
            i += 1;
        }
        let idx = match at {
            Some(x) => x,
            None => return Err(self.bump(PB_ENOENT)),
        };
        let snap = match self.peek(idx) {
            Some(s) => s,
            None => return Err(self.bump(PB_ESTATE)),
        };
        if !snap.bound { return Err(self.bump(PB_EBIND)); }
        if let Some(Some(s)) = self.slots.get_mut(idx) {
            s.bound = false;
            s.addr = 0;
        }
        Ok(1)
    }
}

fn provider(name: &str) -> Option<u32> {
    match name {
        "printf" => Some(0x7f0020),
        "malloc" => Some(0x7f0000),
        "strlen" => Some(0x7f0100),
        "exit" => Some(0x7f0200),
        _ => None,
    }
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

impl PltTable {
    /// FNV-1a 摘要：槽表、三处模式位与账本入账
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.count {
            if let Some(s) = self.peek(i) {
                h = mix_str(h, s.name());
                h = mix(h, s.got as u64);
                h = mix(h, s.addr as u64);
                h = mix(h, s.calls as u64);
                h = mix(h, if s.bound { 1 } else { 0 });
            }
            i += 1;
        }
        h = mix(h, self.count as u64);
        h = mix(h, if self.rtld_now { 1 } else { 0 });
        h = mix(h, if self.env_now { 1 } else { 0 });
        h = mix(h, if self.df1_now { 1 } else { 0 });
        h = mix(h, self.calls as u64);
        h = mix(h, self.resolutions as u64);
        h = mix(h, self.misses as u64);
        h = mix(h, self.aborts as u64);
        h = mix(h, self.loads as u64);
        h = mix(h, self.rejects as u64);
        h
    }

    /// 冻结：READY 或 BOUND 均可冻结，幂等
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            if let Some(r) = self.root { return Ok(r); }
        }
        if self.state != ST_READY && self.state != ST_BOUND {
            return Err(self.bump(PB_EORDER));
        }
        self.state = ST_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 重放：摘要必须与冻结时一致
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.state != ST_FROZEN { return Err(self.bump(PB_EORDER)); }
        let v = self.fold();
        match self.root {
            Some(r) if r == v => Ok(v),
            _ => Err(self.bump(PB_ESTATE)),
        }
    }
}

/// 未封表样例：三条欠条
pub fn build_unsealed() -> PltTable {
    let mut t = PltTable::new();
    let _ = t.add("printf", 0x6000);
    let _ = t.add("malloc", 0x6008);
    let _ = t.add("strlen", 0x6010);
    t
}

/// RTLD_LAZY 样例
pub fn build_lazy() -> PltTable {
    let mut t = build_unsealed();
    let _ = t.seal();
    t
}

/// RTLD_NOW 样例：封表即结清
pub fn build_now() -> PltTable {
    let mut t = build_unsealed();
    let _ = t.set_flags(true, false, false);
    let _ = t.seal();
    t
}

/// 环境变量 LD_BIND_NOW 覆盖 LAZY
pub fn build_env_now() -> PltTable {
    let mut t = build_unsealed();
    let _ = t.set_flags(false, true, false);
    let _ = t.seal();
    t
}

/// DF_1_NOW 位覆盖 LAZY
pub fn build_df1_now() -> PltTable {
    let mut t = build_unsealed();
    let _ = t.set_flags(false, false, true);
    let _ = t.seal();
    t
}

/// 四路独立注入：每条都在新实例上证明一条拒绝路径会触发
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];

    let mut a = PltTable::new();
    match a.add("", 0x1000) {
        Err(code) if code == PB_ENAME => { ok[0] = a.rejects == 1; }
        _ => {}
    }

    let mut b = PltTable::new();
    match b.add("printf", 0x1004) {
        Err(code) if code == PB_EGOT => { ok[1] = b.rejects == 1; }
        _ => {}
    }

    let mut c = build_unsealed();
    match c.call("printf") {
        Err(code) if code == PB_EORDER => { ok[2] = c.rejects == 1 && c.calls == 0; }
        _ => {}
    }

    let mut d = build_unsealed();
    let _ = d.add("ghost", 0x6018);
    let _ = d.seal();
    match d.call("ghost") {
        Err(code) if code == PB_ENOENT => { ok[3] = d.aborts == 1 && d.pending() == 4; }
        _ => {}
    }

    ok
}

/// 十三项静态/动态判据
pub fn run_unx11249_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11249.plt");

    let fresh = PltTable::new();
    s.add(
        "PB1-建场清零",
        fresh.stage() == ST_BUILD
            && fresh.count() == 0
            && fresh.pending() == 0
            && !fresh.effective_now()
            && fresh.rejects == 0,
        "static",
    );

    let mut a = build_unsealed();
    let d1 = a.add("printf", 0x6100) == Err(PB_EDUP);
    let d2 = a.add("memcpy", 0x6000) == Err(PB_EDUP);
    s.add(
        "PB2-登记查重",
        d1 && d2 && a.count() == 3 && a.rejects == 2,
        "static",
    );

    let mut b = PltTable::new();
    let n1 = b.add("", 0x1000) == Err(PB_ENAME);
    let n2 = b.add("printf", 0) == Err(PB_EGOT);
    let n3 = b.add("printf", 0x1004) == Err(PB_EGOT);
    s.add(
        "PB3-空名与槽对齐",
        n1 && n2 && n3 && b.count() == 0 && b.rejects == 3,
        "static",
    );

    let mut c = PltTable::new();
    let mut i = 0;
    while i < GOT_CAP {
        let name = match i {
            0 => "fa",
            1 => "fb",
            2 => "fc",
            3 => "fd",
            4 => "fe",
            _ => "ff",
        };
        let _ = c.add(name, 0x7000 + (i as u32) * 8);
        i += 1;
    }
    s.add(
        "PB4-槽表容量满了拒",
        c.count() == GOT_CAP && c.add("fg", 0x7100) == Err(PB_EFULL) && c.rejects == 1,
        "static",
    );

    let mut d = build_unsealed();
    s.add(
        "PB5-未封表不调用",
        d.call("printf") == Err(PB_EORDER) && d.calls == 0 && d.rejects == 1,
        "static",
    );

    let mut e = build_lazy();
    let again = e.seal() == Err(PB_EORDER);
    let flags = e.set_flags(true, false, false) == Err(PB_EORDER);
    s.add(
        "PB6-封表后只读",
        again && flags && e.effective_now() == false && e.rejects == 2,
        "static",
    );

    let mut f = build_lazy();
    let first = f.call("printf") == Ok(0x7f0020);
    let second = f.call("printf") == Ok(0x7f0020);
    s.add_dyn(
        "PB7-懒绑定首调解析一次",
        first && second && f.resolutions == 1 && f.calls == 2 && f.pending() == 2,
        &format!("res={} calls={}", f.resolutions, f.calls),
    );

    let mut g = build_unsealed();
    let _ = g.add("ghost", 0x6018);
    let _ = g.seal();
    let abort = g.call("ghost") == Err(PB_ENOENT);
    s.add_dyn(
        "PB8-懒绑定缺符号中止",
        abort && g.aborts == 1 && g.misses == 1 && g.pending() == 4 && g.stage() == ST_READY,
        &format!("aborts={} pending={}", g.aborts, g.pending()),
    );

    let mut h = build_now();
    let cached = h.call("printf") == Ok(0x7f0020);
    s.add(
        "PB9-即时绑定一次结清",
        h.stage() == ST_BOUND
            && h.pending() == 0
            && h.resolutions == 3
            && h.loads == 3
            && cached
            && h.resolutions == 3,
        "static",
    );

    let mut k = PltTable::new();
    let _ = k.add("ghost", 0x8000);
    let _ = k.set_flags(true, false, false);
    s.add(
        "PB10-即时绑定缺符号装载失败",
        k.seal() == Err(PB_ELOAD) && k.stage() == ST_BUILD && k.loads == 0 && k.rejects == 1,
        "static",
    );

    let env = build_env_now();
    let df1 = build_df1_now();
    s.add(
        "PB11-环境与DF_1_NOW覆盖LAZY",
        env.effective_now()
            && env.stage() == ST_BOUND
            && env.pending() == 0
            && df1.effective_now()
            && df1.stage() == ST_BOUND
            && df1.resolutions == 3,
        "static",
    );

    let mut z = build_lazy();
    let _ = z.call("printf");
    let f1 = z.freeze();
    let f2 = z.freeze();
    let rp = z.replay();
    let locked = z.call("printf") == Err(PB_EFROZEN);
    let rootv = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add_dyn(
        "PB12-冻结重放幂等",
        f1.is_ok() && f1 == f2 && rp == f1 && locked && z.rejects == 1,
        &format!("root={:x}", rootv),
    );

    let inj = inject_four();
    s.add(
        "PB13-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_first_call_binds() {
        let mut t = build_lazy();
        assert_eq!(t.stage(), ST_READY);
        assert_eq!(t.pending(), 3);
        assert_eq!(t.call("printf"), Ok(0x7f0020));
        assert_eq!(t.pending(), 2);
        assert_eq!(t.call("printf"), Ok(0x7f0020));
        assert_eq!(t.resolutions, 1);
        assert_eq!(t.calls, 2);
        assert_eq!(t.slot(0).map(|s| s.calls()), Some(2));
        assert_eq!(t.call("malloc"), Ok(0x7f0000));
        assert_eq!(t.resolutions, 2);
        assert_eq!(t.call("strlen"), Ok(0x7f0100));
        assert_eq!(t.pending(), 0);
        assert_eq!(t.rebind("printf"), Ok(1));
        assert_eq!(t.pending(), 1);
        assert_eq!(t.call("printf"), Ok(0x7f0020));
        assert_eq!(t.resolutions, 4);
    }

    #[test]
    fn gates_exact_rejects() {
        let mut t = PltTable::new();
        assert_eq!(t.seal(), Err(PB_ESTATE));
        assert_eq!(t.add("", 0x1000), Err(PB_ENAME));
        assert_eq!(t.add("printf", 0), Err(PB_EGOT));
        assert_eq!(t.add("printf", 0x1001), Err(PB_EGOT));
        assert_eq!(t.add("printf", 0x1000), Ok(1));
        assert_eq!(t.add("printf", 0x2000), Err(PB_EDUP));
        assert_eq!(t.add("malloc", 0x1000), Err(PB_EDUP));
        assert_eq!(t.add("malloc", 0x2000), Ok(2));
        assert_eq!(t.call("printf"), Err(PB_EORDER));
        assert_eq!(t.seal(), Ok(2));
        assert_eq!(t.seal(), Err(PB_EORDER));
        assert_eq!(t.set_flags(false, false, false), Err(PB_EORDER));
        assert_eq!(t.call("ghost"), Err(PB_ENOENT));
        assert_eq!(t.call("printf"), Ok(0x7f0020));
        assert_eq!(t.call("printf"), Ok(0x7f0020));
        assert_eq!(t.call("malloc"), Ok(0x7f0000));
        assert!(t.freeze().is_ok());
        assert!(t.replay().is_ok());
        assert_eq!(t.call("printf"), Err(PB_EFROZEN));
        assert_eq!(t.replay(), Err(PB_ESTATE));
        assert_eq!(t.rejects, 12);
        assert_eq!(t.calls, 3);
        assert_eq!(t.resolutions, 2);
        assert_eq!(t.misses, 1);
        assert_eq!(t.aborts, 0);
    }

    #[test]
    fn rules_mode_precedence() {
        let lazy = build_lazy();
        assert!(!lazy.effective_now());
        assert_eq!(lazy.pending(), 3);

        let now = build_now();
        assert!(now.effective_now());
        assert_eq!(now.stage(), ST_BOUND);
        assert_eq!(now.pending(), 0);
        assert_eq!(now.resolutions, 3);

        let env = build_env_now();
        assert!(env.effective_now());
        assert_eq!(env.stage(), ST_BOUND);

        let df1 = build_df1_now();
        assert!(df1.effective_now());
        assert_eq!(df1.stage(), ST_BOUND);

        let mut r = build_lazy();
        assert_eq!(r.rebind("printf"), Err(PB_EBIND));
        assert_eq!(r.rebind("ghost"), Err(PB_ENOENT));

        let codes = [
            PB_ENAME, PB_EGOT, PB_EDUP, PB_EFULL, PB_EORDER,
            PB_ENOENT, PB_EFROZEN, PB_ESTATE, PB_EBIND, PB_ELOAD,
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
        assert_eq!(SEG_BASE, 0x1f310);
        assert_eq!(PROVIDER_CAP, 4);
    }

    #[test]
    fn injection_paths_fire() {
        let inj = inject_four();
        assert!(inj[0], "empty symbol name must be rejected");
        assert!(inj[1], "misaligned GOT slot must be rejected");
        assert!(inj[2], "call before seal must be rejected");
        assert!(inj[3], "lazy miss must abort only on call");
    }

    #[test]
    fn checks_green() {
        let cs = run_unx11249_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }
}
