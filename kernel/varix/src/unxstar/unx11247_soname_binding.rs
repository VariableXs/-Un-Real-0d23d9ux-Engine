//! UNX-F11247 SO_NAME 绑定与 soname 语义（0x1f2f0 段）
//!
//! 锚点：UNX-F11247-J1 soname 优先于文件名绑定判据绿，DT_SONAME/文件名双源
//! 绑定序与 Linux 一致。
//!
//! 纯功能模型：链接器写进 DT_NEEDED 的字符串是**soname**，不是文件名。装载器
//! 拿到一串 NEEDED 时，先在已装载对象里做两遍查：第一遍比 DT_SONAME，第二遍
//! 比文件的 basename（对没有 DT_SONAME 的目标，链接期就用文件名充当 soname）。
//! 两遍都落空才算找不到。本模块把这条双源绑定序写成显式状态机：声明表 →
//! 封表 → 逐 token 绑定，每一步都记账。
//!
//! 规则面：路径必须绝对；路径唯一、soname 唯一（不同路径撞同一个 soname 是
//! 硬错）；token 为空拒；含 '/' 的 token 按路径精确匹配，不走 basename 回退；
//! 绑定必须发生在封表之后，封表必须发生在声明之后；空表既不能封也不能冻结。
//!
//! 判据核心：存在一个 token 同时是「某个对象的 soname」和「另一个更早声明的
//! 对象的 basename」时，绑定必须落在 soname 一方——文件表顺序不得干扰。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2f0;
/// 路径不是绝对路径
pub const SN_EPATH: u32 = SEG_BASE + 0x01;
/// 名字或 token 为空
pub const SN_ENAME: u32 = SEG_BASE + 0x02;
/// 路径重号
pub const SN_EDUP: u32 = SEG_BASE + 0x03;
/// 不同路径撞同一个 soname
pub const SN_ESONADUP: u32 = SEG_BASE + 0x04;
/// token 无法绑定
pub const SN_ENOENT: u32 = SEG_BASE + 0x05;
/// 阶段错（未封表即绑定 / 未声明即封表）
pub const SN_EORDER: u32 = SEG_BASE + 0x06;
/// 声明表超容
pub const SN_EFULL: u32 = SEG_BASE + 0x07;
/// 空表或重放摘要不符
pub const SN_ESTATE: u32 = SEG_BASE + 0x08;
/// 冻结后改写
pub const SN_EFROZEN: u32 = SEG_BASE + 0x09;
/// 要求 soname 绑定时目标没有 DT_SONAME
pub const SN_ENOSONAME: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 声明表容量
pub const TABLE_CAP: usize = 8;
/// 绑定源：DT_SONAME
pub const SRC_SONAME: u8 = 1;
/// 绑定源：文件名 basename
pub const SRC_FILE: u8 = 2;
/// 绑定源：token 自身就是绝对路径
pub const SRC_PATH: u8 = 3;
/// 状态：建场
pub const ST_BUILD: u8 = 0;
/// 状态：已封表
pub const ST_DECLARED: u8 = 1;
/// 状态：冻结
pub const ST_FROZEN: u8 = 2;
const NAME_MAX: usize = 32;
const PATH_MAX: usize = 48;
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 声明表
/// 一行：DT_SONAME（可空）、绝对路径、装载序号
#[derive(Clone, Copy)]
pub struct LibRef {
    soname: [u8; NAME_MAX],
    soname_len: u8,
    path: [u8; PATH_MAX],
    path_len: u8,
    has_soname: bool,
    seq: u8,
}

impl LibRef {
    /// DT_SONAME；没有时返回空串
    pub fn soname(&self) -> &str { str_at(&self.soname, self.soname_len) }
    /// 绝对路径
    pub fn path(&self) -> &str { str_at(&self.path, self.path_len) }
    /// 是否携带 DT_SONAME
    pub fn has_soname(&self) -> bool { self.has_soname }
    /// 装载序号
    pub fn seq(&self) -> u8 { self.seq }
    /// 文件名：路径最后一段
    pub fn basename(&self) -> &str { basename(self.path()) }
}

/// soname 绑定表
pub struct SonameTable {
    rows: [Option<LibRef>; TABLE_CAP],
    count: usize,
    state: u8,
    root: Option<u64>,
    last_slot: u8,
    last_src: u8,
    /// 声明成功次数
    pub declares: u32,
    /// 绑定尝试次数
    pub binds: u32,
    /// 绑定命中次数
    pub hits: u32,
    /// 绑定落空次数
    pub misses: u32,
    /// 按 DT_SONAME 命中次数
    pub soname_hits: u32,
    /// 按文件名命中次数
    pub file_hits: u32,
    /// 按绝对路径命中次数
    pub path_hits: u32,
    /// 拒绝次数
    pub rejects: u32,
}

impl SonameTable {
    /// 空表
    pub fn new() -> Self {
        SonameTable {
            rows: [None; TABLE_CAP],
            count: 0,
            state: ST_BUILD,
            root: None,
            last_slot: 0,
            last_src: 0,
            declares: 0,
            binds: 0,
            hits: 0,
            misses: 0,
            soname_hits: 0,
            file_hits: 0,
            path_hits: 0,
            rejects: 0,
        }
    }

    fn bump(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 当前状态
    pub fn stage(&self) -> u8 { self.state }
    /// 声明条数
    pub fn count(&self) -> usize { self.count }
    /// 最近一次绑定的槽位
    pub fn last_slot(&self) -> u8 { self.last_slot }
    /// 最近一次绑定的来源
    pub fn last_src(&self) -> u8 { self.last_src }
    /// 第 i 行的引用
    pub fn row(&self, i: usize) -> Option<LibRef> {
        if i >= self.count { return None; }
        self.rows[i]
    }
    /// 携带 DT_SONAME 的对象数
    pub fn with_soname(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.has_soname { n += 1; }
            }
            i += 1;
        }
        n
    }

    /// 声明一个对象；soname 传空串表示没有 DT_SONAME
    pub fn declare(&mut self, soname: &str, path: &str) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(SN_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(SN_EORDER)); }
        if path.as_bytes().is_empty() { return Err(self.bump(SN_ENAME)); }
        if !is_abs(path) { return Err(self.bump(SN_EPATH)); }
        if basename(path).as_bytes().is_empty() { return Err(self.bump(SN_ENAME)); }
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.path() == path { return Err(self.bump(SN_EDUP)); }
                if !soname.as_bytes().is_empty() && r.has_soname && r.soname() == soname {
                    return Err(self.bump(SN_ESONADUP));
                }
            }
            i += 1;
        }
        if self.count >= TABLE_CAP { return Err(self.bump(SN_EFULL)); }
        let mut row = LibRef {
            soname: [0u8; NAME_MAX],
            soname_len: 0,
            path: [0u8; PATH_MAX],
            path_len: 0,
            has_soname: false,
            seq: self.count as u8,
        };
        if !soname.as_bytes().is_empty() {
            row.soname_len = match put(&mut row.soname, soname) {
                Some(n) => n,
                None => return Err(self.bump(SN_EFULL)),
            };
            row.has_soname = true;
        }
        row.path_len = match put(&mut row.path, path) {
            Some(n) => n,
            None => return Err(self.bump(SN_EFULL)),
        };
        self.rows[self.count] = Some(row);
        self.count = self.count.wrapping_add(1);
        self.declares = self.declares.wrapping_add(1);
        Ok(self.count as u32)
    }

    /// 封表：此后只能绑定，不能再声明
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(SN_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(SN_EORDER)); }
        if self.count == 0 { return Err(self.bump(SN_ESTATE)); }
        self.state = ST_DECLARED;
        Ok(self.count as u32)
    }

    /// 纯查询：token → (槽位, 绑定源)，不动账本
    pub fn resolve(&self, token: &str) -> Result<(u8, u8), u32> {
        if token.as_bytes().is_empty() { return Err(SN_ENAME); }
        if has_slash(token) {
            let mut i = 0;
            while i < self.count {
                if let Some(r) = self.rows[i] {
                    if r.path() == token { return Ok((i as u8, SRC_PATH)); }
                }
                i += 1;
            }
            return Err(SN_ENOENT);
        }
        // 第一遍：DT_SONAME
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.has_soname && r.soname() == token {
                    return Ok((i as u8, SRC_SONAME));
                }
            }
            i += 1;
        }
        // 第二遍：文件名回退
        let mut k = 0;
        while k < self.count {
            if let Some(r) = self.rows[k] {
                if r.basename() == token { return Ok((k as u8, SRC_FILE)); }
            }
            k += 1;
        }
        Err(SN_ENOENT)
    }

    /// 绑定一个 NEEDED token，记账并返回槽位
    pub fn bind(&mut self, token: &str) -> Result<u8, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(SN_EFROZEN)); }
        if self.state != ST_DECLARED { return Err(self.bump(SN_EORDER)); }
        self.binds = self.binds.wrapping_add(1);
        match self.resolve(token) {
            Ok((slot, src)) => {
                self.hits = self.hits.wrapping_add(1);
                self.last_slot = slot;
                self.last_src = src;
                match src {
                    SRC_SONAME => self.soname_hits = self.soname_hits.wrapping_add(1),
                    SRC_FILE => self.file_hits = self.file_hits.wrapping_add(1),
                    _ => self.path_hits = self.path_hits.wrapping_add(1),
                }
                Ok(slot)
            }
            Err(code) => {
                self.misses = self.misses.wrapping_add(1);
                Err(self.bump(code))
            }
        }
    }

    /// 读取第 i 行的 DT_SONAME；没有则报 ENOSONAME
    pub fn soname_at(&self, i: usize) -> Result<&str, u32> {
        if i >= self.count { return Err(SN_ESTATE); }
        match self.rows.get(i) {
            Some(Some(r)) if r.has_soname => Ok(r.soname()),
            _ => Err(SN_ENOSONAME),
        }
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

fn is_abs(s: &str) -> bool {
    match s.as_bytes().first() {
        Some(b'/') => true,
        _ => false,
    }
}

fn has_slash(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' { return true; }
        i += 1;
    }
    false
}

fn basename(path: &str) -> &str {
    let b = path.as_bytes();
    let mut cut = 0usize;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' { cut = i + 1; }
        i += 1;
    }
    match path.get(cut..) {
        Some(s) => s,
        None => "",
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

impl SonameTable {
    /// FNV-1a 摘要：声明表与全部账本入账
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                h = mix_str(h, r.soname());
                h = mix_str(h, r.path());
                h = mix(h, if r.has_soname { 1 } else { 0 });
                h = mix(h, r.seq() as u64);
            }
            i += 1;
        }
        h = mix(h, self.count as u64);
        h = mix(h, self.declares as u64);
        h = mix(h, self.binds as u64);
        h = mix(h, self.hits as u64);
        h = mix(h, self.misses as u64);
        h = mix(h, self.soname_hits as u64);
        h = mix(h, self.file_hits as u64);
        h = mix(h, self.path_hits as u64);
        h = mix(h, self.rejects as u64);
        h
    }

    /// 冻结：只在已封表态成立，幂等
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            if let Some(r) = self.root { return Ok(r); }
        }
        if self.state != ST_DECLARED { return Err(self.bump(SN_EORDER)); }
        if self.count == 0 { return Err(self.bump(SN_ESTATE)); }
        self.state = ST_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 重放：摘要必须与冻结时一致
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.state != ST_FROZEN { return Err(self.bump(SN_EORDER)); }
        let v = self.fold();
        match self.root {
            Some(r) if r == v => Ok(v),
            _ => Err(self.bump(SN_ESTATE)),
        }
    }

    /// J1 不变量：每个携带 DT_SONAME 的对象，按自身 soname 解析必须命中自己
    pub fn soname_priority_ok(&self) -> bool {
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.has_soname {
                    match self.resolve(r.soname()) {
                        Ok((slot, src)) => {
                            if src != SRC_SONAME || slot as usize != i { return false; }
                        }
                        Err(_) => return false,
                    }
                }
            }
            i += 1;
        }
        true
    }
}

/// 样例声明表：六行，其中一行没有 DT_SONAME，一行是文件名撞车对象
pub fn build_simple() -> SonameTable {
    let mut t = SonameTable::new();
    let _ = t.declare("libc.so.6", "/lib/libc.so.6");
    let _ = t.declare("libm.so.6", "/lib/libm.so.6");
    let _ = t.declare("libhook.so", "/opt/pre/libhook.so");
    let _ = t.declare("libbar.so.7", "/lib/link/libbar.so.1");
    let _ = t.declare("libbar.so.1", "/lib/libbar.so.1");
    let _ = t.declare("", "/opt/pre/libnosoname.so");
    t
}

/// 已封表的样例
pub fn build_sealed() -> SonameTable {
    let mut t = build_simple();
    let _ = t.seal();
    t
}

/// 封表并完成四次绑定：soname×2、文件名×1、绝对路径×1
pub fn build_bound() -> SonameTable {
    let mut t = build_sealed();
    let _ = t.bind("libc.so.6");
    let _ = t.bind("libbar.so.1");
    let _ = t.bind("libnosoname.so");
    let _ = t.bind("/opt/pre/libhook.so");
    t
}

/// 四路独立注入：每条都在新实例上证明一条拒绝路径会触发
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];

    let mut a = SonameTable::new();
    match a.declare("libx.so", "lib/libx.so") {
        Err(code) if code == SN_EPATH => { ok[0] = a.rejects == 1; }
        _ => {}
    }

    let mut b = SonameTable::new();
    let _ = b.declare("libx.so", "/lib/libx.so");
    match b.declare("liby.so", "/lib/libx.so") {
        Err(code) if code == SN_EDUP => { ok[1] = b.rejects == 1; }
        _ => {}
    }

    let mut c = SonameTable::new();
    let _ = c.declare("libx.so", "/lib/libx.so");
    match c.declare("libx.so", "/lib/liby.so") {
        Err(code) if code == SN_ESONADUP => { ok[2] = c.rejects == 1; }
        _ => {}
    }

    let mut d = SonameTable::new();
    let _ = d.declare("libx.so", "/lib/libx.so");
    match d.bind("libx.so") {
        Err(code) if code == SN_EORDER => { ok[3] = d.rejects == 1 && d.binds == 0; }
        _ => {}
    }

    ok
}

/// 十三项静态/动态判据
pub fn run_unx11247_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11247.soname");

    let fresh = SonameTable::new();
    s.add(
        "SN1-建场清零",
        fresh.stage() == ST_BUILD
            && fresh.count() == 0
            && fresh.with_soname() == 0
            && fresh.rejects == 0,
        "static",
    );

    let simple = build_simple();
    s.add(
        "SN2-声明与soname计数",
        simple.count() == 6 && simple.with_soname() == 5 && simple.declares == 6,
        "static",
    );

    let mut a = SonameTable::new();
    s.add(
        "SN3-相对路径拒",
        a.declare("libx.so", "lib/libx.so") == Err(SN_EPATH) && a.rejects == 1,
        "static",
    );

    let mut b = SonameTable::new();
    let e1 = b.declare("libx.so", "/lib/") == Err(SN_ENAME);
    let e2 = b.declare("libx.so", "") == Err(SN_ENAME);
    s.add(
        "SN4-空文件名拒",
        e1 && e2 && b.rejects == 2 && b.count() == 0,
        "static",
    );

    let mut c = SonameTable::new();
    let _ = c.declare("libx.so", "/lib/libx.so");
    s.add(
        "SN5-路径重号拒",
        c.declare("liby.so", "/lib/libx.so") == Err(SN_EDUP) && c.rejects == 1,
        "static",
    );

    let mut d = SonameTable::new();
    let _ = d.declare("libx.so", "/lib/libx.so");
    s.add(
        "SN6-soname重号拒",
        d.declare("libx.so", "/lib/liby.so") == Err(SN_ESONADUP) && d.rejects == 1,
        "static",
    );

    let mut e = SonameTable::new();
    let empty_seal = e.seal() == Err(SN_ESTATE);
    let _ = e.declare("libx.so", "/lib/libx.so");
    let first = e.seal() == Ok(1);
    let again = e.seal() == Err(SN_EORDER);
    s.add(
        "SN7-空表不可封",
        empty_seal && first && again && e.rejects == 2,
        "static",
    );

    let mut f = build_simple();
    s.add(
        "SN8-未封表不绑定",
        f.bind("libc.so.6") == Err(SN_EORDER) && f.binds == 0 && f.rejects == 1,
        "static",
    );

    let sealed = build_sealed();
    s.add_dyn(
        "SN9-soname优先于文件名",
        sealed.soname_priority_ok()
            && sealed.resolve("libbar.so.1") == Ok((4, SRC_SONAME))
            && sealed.resolve("libbar.so.7") == Ok((3, SRC_SONAME)),
        &format!("bar1={:?}", sealed.resolve("libbar.so.1")),
    );

    let bound = build_bound();
    s.add(
        "SN10-文件名回退与路径直绑",
        bound.resolve("libnosoname.so") == Ok((5, SRC_FILE))
            && bound.resolve("/opt/pre/libhook.so") == Ok((2, SRC_PATH))
            && bound.file_hits == 1
            && bound.path_hits == 1
            && bound.soname_hits == 2,
        "static",
    );

    let mut g = build_bound();
    let unknown = g.bind("libghost.so") == Err(SN_ENOENT);
    let blank = g.bind("") == Err(SN_ENAME);
    s.add_dyn(
        "SN11-未知token落空",
        unknown && blank && g.misses == 2 && g.hits == 4,
        &format!("misses={} hits={}", g.misses, g.hits),
    );

    let mut z = build_bound();
    let f1 = z.freeze();
    let f2 = z.freeze();
    let rp = z.replay();
    let locked_decl = z.declare("libz.so", "/lib/libz.so") == Err(SN_EFROZEN);
    let locked_bind = z.bind("libc.so.6") == Err(SN_EFROZEN);
    let rootv = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add_dyn(
        "SN12-冻结重放幂等",
        f1.is_ok() && f1 == f2 && rp == f1 && locked_decl && locked_bind && z.rejects == 2,
        &format!("root={:x}", rootv),
    );

    let inj = inject_four();
    s.add(
        "SN13-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_double_source_binding() {
        let t = build_bound();
        assert_eq!(t.stage(), ST_DECLARED);
        assert_eq!(t.count(), 6);
        assert_eq!(t.with_soname(), 5);
        assert_eq!(t.binds, 4);
        assert_eq!(t.hits, 4);
        assert_eq!(t.misses, 0);
        assert_eq!(t.soname_hits, 2);
        assert_eq!(t.file_hits, 1);
        assert_eq!(t.path_hits, 1);
        assert_eq!(t.last_slot(), 2);
        assert_eq!(t.last_src(), SRC_PATH);
        assert!(t.soname_priority_ok());
        assert_eq!(t.soname_at(4), Ok("libbar.so.1"));
        assert_eq!(t.soname_at(5), Err(SN_ENOSONAME));
        match t.row(5) {
            Some(r) => {
                assert!(!r.has_soname());
                assert_eq!(r.basename(), "libnosoname.so");
                assert_eq!(r.seq(), 5);
            }
            None => panic_guard(),
        }
    }

    fn panic_guard() {
        assert!(false);
    }

    #[test]
    fn gates_exact_rejects() {
        let mut t = SonameTable::new();
        assert_eq!(t.seal(), Err(SN_ESTATE));
        assert_eq!(t.declare("libc.so.6", "lib/libc.so.6"), Err(SN_EPATH));
        assert_eq!(t.declare("libc.so.6", "/lib/libc.so.6"), Ok(1));
        assert_eq!(t.declare("libc.so.6", "/lib/libc.so.6"), Err(SN_EDUP));
        assert_eq!(t.declare("libc.so.6", "/lib/other.so"), Err(SN_ESONADUP));
        assert_eq!(t.declare("libx.so", "/lib/"), Err(SN_ENAME));
        assert_eq!(t.bind("libc.so.6"), Err(SN_EORDER));
        assert_eq!(t.seal(), Ok(1));
        assert_eq!(t.bind("libghost.so"), Err(SN_ENOENT));
        assert_eq!(t.declare("libm.so.6", "/lib/libm.so.6"), Err(SN_EORDER));
        assert!(t.freeze().is_ok());
        assert!(t.replay().is_ok());
        assert_eq!(t.bind("libc.so.6"), Err(SN_EFROZEN));
        assert_eq!(t.seal(), Err(SN_EFROZEN));
        assert_eq!(t.replay(), Err(SN_ESTATE));
        assert_eq!(t.rejects, 11);
        assert_eq!(t.binds, 1);
        assert_eq!(t.misses, 1);
        assert_eq!(t.declares, 1);
    }

    #[test]
    fn rules_precedence_and_codes() {
        let mut t = SonameTable::new();
        let _ = t.declare("liba.so.1", "/lib/liba.so");
        // 路径重号先于 soname 冲突判定
        assert_eq!(t.declare("liba.so.2", "/lib/liba.so"), Err(SN_EDUP));
        // soname 冲突：不同路径、同名
        assert_eq!(t.declare("liba.so.1", "/lib/libb.so"), Err(SN_ESONADUP));
        // 无 soname 的声明合法
        assert_eq!(t.declare("", "/lib/libplain.so"), Ok(2));
        // 含斜杠 token 只走路径，不回退 basename
        assert_eq!(t.seal(), Ok(2));
        assert_eq!(t.bind("lib/liba.so"), Err(SN_ENOENT));
        assert_eq!(t.bind("/lib/plain.so"), Err(SN_ENOENT));
        assert_eq!(t.bind("/lib/libplain.so"), Ok(1));

        let codes = [
            SN_EPATH, SN_ENAME, SN_EDUP, SN_ESONADUP, SN_ENOENT,
            SN_EORDER, SN_EFULL, SN_ESTATE, SN_EFROZEN, SN_ENOSONAME,
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
        assert_eq!(SEG_BASE, 0x1f2f0);
    }

    #[test]
    fn injection_paths_fire() {
        let inj = inject_four();
        assert!(inj[0], "relative path must be rejected");
        assert!(inj[1], "duplicate path must be rejected");
        assert!(inj[2], "duplicate soname must be rejected");
        assert!(inj[3], "bind before seal must be rejected");
    }

    #[test]
    fn checks_green() {
        let cs = run_unx11247_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }
}
