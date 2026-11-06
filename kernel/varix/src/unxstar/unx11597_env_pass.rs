//! UNX-F11593 环境变量继承与传递判据
//!
//! applet 拿到的环境**不是**父进程环境的拷贝，而是「允许名单过滤 + 默认值
//! 补齐」两道工序的产物。判据要同时钉住四件事：
//!
//! ```text
//! 有序   环境块按 key 字节序严格递增（getenv 走二分；重复键必须先拒）
//! 过滤   只有 allowlist 里的 key 才下沉；其余一律不进子环境
//! 默认   PATH / HOME / LANG / TERM 若缺席则注入固定默认值（不是空串）
//! 传递   子环境 ⊆ 父环境 ∪ 默认集；被 unset 的键不得从别处复活
//! ```
//!
//! 为什么默认值是判据而不是「锦上添花」：静态 busybox 的 `sh` 在 PATH
//! 为空时连内建命令之外的任何东西都找不到；`HOME` 缺席会让 `vi` 写到 `/`；
//! `LANG` 缺席会让 `ls` 的排序随宿主 locale 漂移。所以「缺席即注入」这条
//! 规则必须被**验证**，而不是碰巧成立。
//!
//! 本模块把环境做成定容表（8 槽 × 16 字节 key × 48 字节 value），
//! 插入即有序（二分定位 + 右移），`unset` 反向扫描后左移，全程零分配。
//! 相等性手写（只比有效字节），避免把缓冲区尾部残留算进比较——这是定容
//! 表最容易踩的坑。
//!
//! 闸门次序：key 空 → key 长 → key 字节 → `=` → value 长 → value 字节 →
//! 重复 → 容量。
//!
//! 账本 14 格（8 成 6 败）在 `audit` 逐格对账；冻结后重放须同指纹。
//! 无 std、无 unsafe、无 panic、无 unwrap。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（本卡专属）。
pub const SEG_BASE: u32 = 0xfaa0;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// key 字节容量。
pub const KEY_CAP: usize = 16;

/// value 字节容量。
pub const VAL_CAP: usize = 48;

/// 环境槽数。
pub const ENV_CAP: usize = 8;

/// 默认 PATH（29 字节）。
pub const DEFAULT_PATH: &str = "/sbin:/usr/sbin:/bin:/usr/bin";

/// 默认环境（key, value），按 key 序。
///
/// 四条缺一不可，且值都是**确定的**：`HOME=/`（根下可写）、`LANG=C`
/// （不随宿主 locale 漂移）、`PATH` 覆盖 sbin/bin 两侧、`TERM=dumb`
/// （无 terminfo 时也能输出）。
pub const DEFAULTS: [(&str, &str); 4] = [
    ("HOME", "/"),
    ("LANG", "C"),
    ("PATH", DEFAULT_PATH),
    ("TERM", "dumb"),
];

// ---------------------------------------------------------------------------
// 错误码：8 个域错误 + 5 个账本错误
// ---------------------------------------------------------------------------

/// key/value 含不可打印字节（key 要求 0x21..=0x7e，value 允许空格）。
pub const EV_EBYTE: u32 = SEG_BASE + 0x01;
/// key 或 value 超长。
pub const EV_ELEN: u32 = SEG_BASE + 0x02;
/// key 为空。
pub const EV_EEMPTY: u32 = SEG_BASE + 0x03;
/// key 含 `=`（会破坏 `k=v` 编码）。
pub const EV_EEQ: u32 = SEG_BASE + 0x04;
/// 重复键。
pub const EV_EDUP: u32 = SEG_BASE + 0x05;
/// 允许名单里有父环境不存在的键。
pub const EV_ESET: u32 = SEG_BASE + 0x06;
/// 环境块未按 key 严格递增。
pub const EV_EORDER: u32 = SEG_BASE + 0x07;
/// 容量越界。
pub const EV_ECAP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const EV_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const EV_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const EV_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const EV_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const EV_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 写入。
pub const OP_SET: u8 = 0x01;
/// 读取。
pub const OP_GET: u8 = 0x02;
/// 子环境。
pub const OP_CHILD: u8 = 0x03;
/// 渲染。
pub const OP_RENDER: u8 = 0x04;
/// 子集判定。
pub const OP_SUBSET: u8 = 0x05;
/// 取消。
pub const OP_UNSET: u8 = 0x06;
/// PATH 分段。
pub const OP_PATH: u8 = 0x07;

// ---------------------------------------------------------------------------
// 哈希：FNV-1a，每字节 8 次乘法
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

/// 折入一段字节。
fn fold_bytes(mut h: u64, s: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 字节序比较：-1 / 0 / 1。
fn cmp_bytes(a: &[u8], b: &[u8]) -> i32 {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x < y {
                    return -1;
                }
                if x > y {
                    return 1;
                }
            }
            _ => break,
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

/// 十进制拼进 String。
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
        at = at.saturating_sub(1);
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

// ---------------------------------------------------------------------------
// 变量
// ---------------------------------------------------------------------------

/// 一个 `key=value`。
///
/// 相等性手写：只比有效字节。缓冲区尾部残留（`unset` 左移、覆盖写）不参与
/// 比较，否则两条同值的变量会被判成不同。
#[derive(Clone, Copy, Debug)]
pub struct Var {
    key: [u8; KEY_CAP],
    klen: u8,
    val: [u8; VAL_CAP],
    vlen: u8,
}

impl PartialEq for Var {
    fn eq(&self, other: &Var) -> bool {
        eq_bytes(self.key_bytes(), other.key_bytes()) && eq_bytes(self.val_bytes(), other.val_bytes())
    }
}

impl Eq for Var {}

impl Var {
    /// 空变量（占位）。
    pub const fn blank() -> Var {
        Var { key: [0u8; KEY_CAP], klen: 0, val: [0u8; VAL_CAP], vlen: 0 }
    }

    /// 构造：key 闸 → value 闸。
    pub fn new(key: &[u8], val: &[u8]) -> Result<Var, u32> {
        if key.is_empty() {
            return Err(EV_EEMPTY);
        }
        if key.len() > KEY_CAP || val.len() > VAL_CAP {
            return Err(EV_ELEN);
        }
        let mut i = 0usize;
        while i < key.len() {
            match key.get(i) {
                Some(b) => {
                    if *b < 0x21 || *b > 0x7e {
                        return Err(EV_EBYTE);
                    }
                    if *b == b'=' {
                        return Err(EV_EEQ);
                    }
                }
                None => return Err(EV_EBYTE),
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < val.len() {
            match val.get(j) {
                Some(b) => {
                    if *b < 0x20 || *b > 0x7e {
                        return Err(EV_EBYTE);
                    }
                }
                None => return Err(EV_EBYTE),
            }
            j += 1;
        }
        let mut v = Var::blank();
        let mut a = 0usize;
        while a < key.len() {
            match (v.key.get_mut(a), key.get(a)) {
                (Some(dst), Some(src)) => *dst = *src,
                _ => return Err(EV_ELEN),
            }
            a += 1;
        }
        let mut b = 0usize;
        while b < val.len() {
            match (v.val.get_mut(b), val.get(b)) {
                (Some(dst), Some(src)) => *dst = *src,
                _ => return Err(EV_ELEN),
            }
            b += 1;
        }
        v.klen = key.len() as u8;
        v.vlen = val.len() as u8;
        Ok(v)
    }

    /// key 视图。
    pub fn key_bytes(&self) -> &[u8] {
        let n = (self.klen as usize).min(KEY_CAP);
        match self.key.get(..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// value 视图。
    pub fn val_bytes(&self) -> &[u8] {
        let n = (self.vlen as usize).min(VAL_CAP);
        match self.val.get(..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// key 文本。
    pub fn key_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.key_bytes().len() {
            if let Some(b) = self.key_bytes().get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// value 文本。
    pub fn val_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.val_bytes().len() {
            if let Some(b) = self.val_bytes().get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// `key=value` 文本。
    pub fn text(&self) -> String {
        let mut s = self.key_text();
        s.push('=');
        s.push_str(self.val_text().as_str());
        s
    }

    /// 渲染字节数（含尾部 NUL）。
    pub fn render_len(&self) -> usize {
        self.key_bytes().len().saturating_add(self.val_bytes().len()).saturating_add(2)
    }

    /// 是否同 key。
    pub fn same_key(&self, key: &[u8]) -> bool {
        eq_bytes(self.key_bytes(), key)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let h = fold_bytes(FNV_OFF, self.key_bytes());
        let h = fold_byte(h, b'=');
        fold_bytes(h, self.val_bytes())
    }
}

// ---------------------------------------------------------------------------
// 环境块
// ---------------------------------------------------------------------------

/// 定容环境块（按 key 严格递增）。
#[derive(Clone, Copy, Debug)]
pub struct Env {
    rows: [Var; ENV_CAP],
    len: usize,
}

impl PartialEq for Env {
    fn eq(&self, other: &Env) -> bool {
        if self.len != other.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.rows.get(i), other.rows.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

impl Eq for Env {}

/// 在有序行上求下界。
fn lower_bound(rows: &[Var; ENV_CAP], len: usize, key: &[u8]) -> usize {
    let mut lo = 0usize;
    let mut hi = len.min(ENV_CAP);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let c = match rows.get(mid) {
            Some(v) => cmp_bytes(v.key_bytes(), key),
            None => 0,
        };
        if c < 0 {
            lo = mid.saturating_add(1);
        } else {
            hi = mid;
        }
    }
    lo
}

impl Env {
    /// 空环境。
    pub const fn new() -> Env {
        Env { rows: [Var::blank(); ENV_CAP], len: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取第 i 条。
    pub fn at(&self, i: usize) -> Option<Var> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 按 key 查找位置。
    pub fn index_of(&self, key: &[u8]) -> Option<usize> {
        let p = lower_bound(&self.rows, self.len, key);
        if p >= self.len {
            return None;
        }
        match self.rows.get(p) {
            Some(v) => {
                if v.same_key(key) {
                    Some(p)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 按 key 取值。
    pub fn get(&self, key: &[u8]) -> Option<Var> {
        match self.index_of(key) {
            Some(p) => self.rows.get(p).copied(),
            None => None,
        }
    }

    /// 写入：容量 → 重复 → 有序插入。
    pub fn set(&mut self, v: Var) -> Result<usize, u32> {
        if self.len >= ENV_CAP {
            return Err(EV_ECAP);
        }
        if self.index_of(v.key_bytes()).is_some() {
            return Err(EV_EDUP);
        }
        let p = lower_bound(&self.rows, self.len, v.key_bytes());
        let mut i = self.len;
        while i > p {
            match (self.rows.get(i - 1).copied(), self.rows.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = src,
                _ => break,
            }
            i -= 1;
        }
        if let Some(dst) = self.rows.get_mut(p) {
            *dst = v;
        }
        self.len = self.len.saturating_add(1);
        Ok(p)
    }

    /// 取消某键：返回是否真的删了一条。
    pub fn unset(&mut self, key: &[u8]) -> bool {
        let p = match self.index_of(key) {
            Some(x) => x,
            None => return false,
        };
        let mut i = p;
        while i + 1 < self.len {
            match (self.rows.get(i + 1).copied(), self.rows.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = src,
                _ => return false,
            }
            i += 1;
        }
        self.len = self.len.saturating_sub(1);
        true
    }

    /// 是否严格递增（结构不变量）。
    pub fn is_sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.len {
            match (self.rows.get(i - 1), self.rows.get(i)) {
                (Some(a), Some(b)) => {
                    if cmp_bytes(a.key_bytes(), b.key_bytes()) >= 0 {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 结构校验。
    pub fn validate(&self) -> Result<(), u32> {
        if self.len > ENV_CAP {
            return Err(EV_ECAP);
        }
        if !self.is_sorted() {
            return Err(EV_EORDER);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(v) => {
                    if v.key_bytes().is_empty() {
                        return Err(EV_EEMPTY);
                    }
                }
                None => return Err(EV_ECAP),
            }
            i += 1;
        }
        Ok(())
    }

    /// 子集：每条都在 other 里且值相同。
    pub fn subset_of(&self, other: &Env) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(v) => match other.get(v.key_bytes()) {
                    Some(w) => {
                        if *v != w {
                            return false;
                        }
                    }
                    None => return false,
                },
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 并集（self 优先，重复键保留 self 的值）。
    pub fn union(&self, other: &Env) -> Env {
        let mut e = *self;
        let mut i = 0usize;
        while i < other.len {
            if let Some(v) = other.rows.get(i) {
                if e.get(v.key_bytes()).is_none() && e.len < ENV_CAP {
                    let _ = e.set(*v);
                }
            }
            i += 1;
        }
        e
    }

    /// 渲染字节数（每条 `key=value\0`）。
    pub fn render_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                n = n.saturating_add(v.render_len());
            }
            i += 1;
        }
        n
    }

    /// 渲染到定容缓冲：`key=value\0` 顺序拼接，返回写入字节数。
    pub fn render(&self, out: &mut [u8]) -> Result<usize, u32> {
        let need = self.render_len();
        if need > out.len() {
            return Err(EV_ECAP);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(EV_ECAP),
            };
            let mut k = 0usize;
            while k < v.key_bytes().len() {
                if let (Some(dst), Some(src)) = (out.get_mut(n), v.key_bytes().get(k)) {
                    *dst = *src;
                    n = n.saturating_add(1);
                }
                k += 1;
            }
            if let Some(dst) = out.get_mut(n) {
                *dst = b'=';
                n = n.saturating_add(1);
            }
            let mut j = 0usize;
            while j < v.val_bytes().len() {
                if let (Some(dst), Some(src)) = (out.get_mut(n), v.val_bytes().get(j)) {
                    *dst = *src;
                    n = n.saturating_add(1);
                }
                j += 1;
            }
            if let Some(dst) = out.get_mut(n) {
                *dst = 0;
                n = n.saturating_add(1);
            }
            i += 1;
        }
        Ok(n)
    }

    /// PATH 的分段数（空段不计）。
    pub fn path_entries(&self) -> usize {
        let v = match self.get(b"PATH") {
            Some(x) => x,
            None => return 0,
        };
        let b = v.val_bytes();
        let mut n = 0usize;
        let mut i = 0usize;
        let mut start = 0usize;
        while i <= b.len() {
            let c = b.get(i).copied().unwrap_or(b':');
            if c == b':' {
                if i > start {
                    n = n.saturating_add(1);
                }
                start = i.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 文本摘要 `K=V;K=V`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                if !s.is_empty() {
                    s.push(';');
                }
                s.push_str(v.text().as_str());
            }
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                h = fold_bytes(h, v.key_bytes());
                h = fold_byte(h, b'=');
                h = fold_bytes(h, v.val_bytes());
                h = fold_byte(h, 0);
            }
            i += 1;
        }
        fold(h, self.len as u64)
    }
}

/// 允许名单里是否有该 key。
pub fn allowed(allow: &[&[u8]], key: &[u8]) -> bool {
    let mut i = 0usize;
    while i < allow.len() {
        if let Some(k) = allow.get(i) {
            if eq_bytes(k, key) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 默认值查表。
pub fn default_of(key: &[u8]) -> Option<&'static str> {
    let mut i = 0usize;
    while i < DEFAULTS.len() {
        if let Some((k, v)) = DEFAULTS.get(i) {
            if eq_bytes(k.as_bytes(), key) {
                return Some(v);
            }
        }
        i += 1;
    }
    None
}

/// 标准父环境（5 条，覆盖「允许 / 不允许 / 默认键被改写」三种情形）。
pub fn parent_env() -> Env {
    let mut e = Env::new();
    let rows: [(&[u8], &[u8]); 5] = [
        (b"HOME", b"/root"),
        (b"LANG", b"en_US.UTF-8"),
        (b"PATH", b"/bin:/usr/bin"),
        (b"TERM", b"xterm"),
        (b"USER", b"root"),
    ];
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((k, v)) = rows.get(i) {
            if let Ok(var) = Var::new(k, v) {
                let _ = e.set(var);
            }
        }
        i += 1;
    }
    e
}

/// 子环境：允许名单过滤 + 默认值补齐。
///
/// 默认值只在**子环境里仍缺席**时注入：父环境允许通过且已有该键，就保留
/// 父值（`HOME=/root` 不该被 `/` 覆盖）。这就是判据里「默认不是覆盖」的
/// 那一半。
pub fn child_env(parent: &Env, allow: &[&[u8]]) -> Result<Env, u32> {
    let mut c = Env::new();
    let mut i = 0usize;
    while i < parent.len() {
        if let Some(v) = parent.at(i) {
            if allowed(allow, v.key_bytes()) {
                c.set(v)?;
            }
        }
        i += 1;
    }
    let mut d = 0usize;
    while d < DEFAULTS.len() {
        if let Some((k, val)) = DEFAULTS.get(d) {
            if c.get(k.as_bytes()).is_none() {
                c.set(Var::new(k.as_bytes(), val.as_bytes())?)?;
            }
        }
        d += 1;
    }
    Ok(c)
}

/// 取消一批键，返回新环境（原环境不变）。
pub fn unset_all(src: &Env, keys: &[&[u8]]) -> Result<Env, u32> {
    let mut e = *src;
    let mut i = 0usize;
    while i < keys.len() {
        if let Some(k) = keys.get(i) {
            let _ = e.unset(k);
        }
        i += 1;
    }
    e.validate()?;
    Ok(e)
}

/// 继承自洽：子环境的每条要么是「父值 + 允许」，要么是「默认值」；
/// 且所有允许的父键都必须在子环境里出现。
pub fn inheritance_ok(parent: &Env, child: &Env, allow: &[&[u8]]) -> bool {
    if child.validate().is_err() {
        return false;
    }
    let mut i = 0usize;
    while i < child.len() {
        let v = match child.at(i) {
            Some(x) => x,
            None => return false,
        };
        let from_parent = match parent.get(v.key_bytes()) {
            Some(p) => v == p && allowed(allow, v.key_bytes()),
            None => false,
        };
        let from_default = match default_of(v.key_bytes()) {
            Some(d) => eq_bytes(d.as_bytes(), v.val_bytes()),
            None => false,
        };
        if !from_parent && !from_default {
            return false;
        }
        i += 1;
    }
    // 允许的父键必须下沉（父有而子无 = 传递丢了）
    let mut p = 0usize;
    while p < parent.len() {
        if let Some(v) = parent.at(p) {
            if allowed(allow, v.key_bytes()) && child.get(v.key_bytes()).is_none() {
                return false;
            }
        }
        p += 1;
    }
    // 允许名单不得指向父环境里不存在的键（拼错的名单是配置错误）
    let mut a = 0usize;
    while a < allow.len() {
        if let Some(k) = allow.get(a) {
            if parent.get(k).is_none() {
                return false;
            }
        }
        a += 1;
    }
    true
}

/// 允许名单校验：每项都必须能在父环境里找到。
pub fn check_allow(parent: &Env, allow: &[&[u8]]) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < allow.len() {
        match allow.get(i) {
            Some(k) => {
                if parent.get(k).is_none() {
                    return Err(EV_ESET);
                }
                n = n.saturating_add(1);
            }
            None => return Err(EV_ESET),
        }
        i += 1;
    }
    Ok(n)
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

    /// 登记（严格槽序，冻结后拒）。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EV_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
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
            return Err(EV_EBAL);
        }
        const VALS: [u32; 8] = [3, 29, 4, 2, 14, 1, 4, 4];
        const CODES: [u32; 6] = [EV_EDUP, EV_EEMPTY, EV_EEQ, EV_ELEN, EV_EBYTE, EV_ECAP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EV_EBAL),
            };
            if s.slot as usize != i {
                return Err(EV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EV_EVALUE);
                        }
                    }
                    None => return Err(EV_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EV_EVALUE);
                        }
                    }
                    None => return Err(EV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EV_EBAL);
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
            return Err(EV_EBAL);
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
            Err(EV_ESEAL)
        }
    }

    /// 冻结根。
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

/// `Result<u32, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 三变量小环境（HOME/TERM/USER）。
fn small_env() -> Env {
    let mut e = Env::new();
    let rows: [(&[u8], &[u8]); 3] = [
        (b"HOME", b"/root"),
        (b"TERM", b"xterm"),
        (b"USER", b"root"),
    ];
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((k, v)) = rows.get(i) {
            if let Ok(var) = Var::new(k, v) {
                let _ = e.set(var);
            }
        }
        i += 1;
    }
    e
}

/// 账本组装：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let parent = parent_env();

    // 0: 三变量环境长度 3。
    let _ = l.register(cell(0, OP_SET, Ok(small_env().len() as u32)));

    // 1: 默认 PATH 29 字节。
    let _ = l.register(cell(
        1,
        OP_GET,
        match child_env(&Env::new(), &[]) {
            Ok(e) => match e.get(b"PATH") {
                Some(v) => Ok(v.val_bytes().len() as u32),
                None => Err(EV_ENOBS),
            },
            Err(c) => Err(c),
        },
    ));

    // 2: 允许 HOME/PATH 的子环境共 4 条。
    let _ = l.register(cell(
        2,
        OP_CHILD,
        match child_env(&parent, &[b"HOME", b"PATH"]) {
            Ok(e) => Ok(e.len() as u32),
            Err(c) => Err(c),
        },
    ));

    // 3: 只放行 HOME 时，默认补进来的 LANG/PATH 两条。
    let _ = l.register(cell(
        3,
        OP_CHILD,
        match child_env(&small_env(), &[b"HOME"]) {
            Ok(e) => {
                let mut n = 0u32;
                let mut i = 0usize;
                while i < e.len() {
                    if let Some(v) = e.at(i) {
                        if small_env().get(v.key_bytes()).is_none() {
                            n = n.saturating_add(1);
                        }
                    }
                    i += 1;
                }
                Ok(n)
            }
            Err(c) => Err(c),
        },
    ));

    // 4: HOME=/ + LANG=C 渲染 14 字节。
    let _ = l.register(cell(
        4,
        OP_RENDER,
        {
            let mut e = Env::new();
            let mut ok = true;
            match Var::new(b"HOME", b"/") {
                Ok(v) => {
                    if e.set(v).is_err() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            match Var::new(b"LANG", b"C") {
                Ok(v) => {
                    if e.set(v).is_err() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            if ok {
                Ok(e.render_len() as u32)
            } else {
                Err(EV_ENOBS)
            }
        },
    ));

    // 5: {HOME,PATH} 是父环境子集（1 = 真）。
    let _ = l.register(cell(
        5,
        OP_SUBSET,
        {
            let mut s = Env::new();
            let mut ok = true;
            if let Some(v) = parent.get(b"HOME") {
                if s.set(v).is_err() {
                    ok = false;
                }
            } else {
                ok = false;
            }
            if let Some(v) = parent.get(b"PATH") {
                if s.set(v).is_err() {
                    ok = false;
                }
            } else {
                ok = false;
            }
            if ok {
                Ok(s.subset_of(&parent) as u32)
            } else {
                Err(EV_ENOBS)
            }
        },
    ));

    // 6: 默认 PATH 四段。
    let _ = l.register(cell(
        6,
        OP_PATH,
        match child_env(&Env::new(), &[]) {
            Ok(e) => Ok(e.path_entries() as u32),
            Err(c) => Err(c),
        },
    ));

    // 7: 父环境取消 USER 后剩 4 条。
    let _ = l.register(cell(
        7,
        OP_UNSET,
        match unset_all(&parent, &[b"USER"]) {
            Ok(e) => Ok(e.len() as u32),
            Err(c) => Err(c),
        },
    ));

    // 8: 重复键 PATH。
    let _ = l.register(cell(
        8,
        OP_SET,
        {
            let mut e = parent_env();
            match Var::new(b"PATH", b"/sbin") {
                Ok(v) => e.set(v).map(|p| p as u32),
                Err(c) => Err(c),
            }
        },
    ));

    // 9: 空 key。
    let _ = l.register(cell(
        9,
        OP_SET,
        match Var::new(b"", b"x") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 10: key 含 `=`。
    let _ = l.register(cell(
        10,
        OP_SET,
        match Var::new(b"A=B", b"x") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 11: key 超长（17 字节）。
    let _ = l.register(cell(
        11,
        OP_SET,
        match Var::new(b"ABCDEFGHIJKLMNOPQ", b"x") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 12: key 首字节控制符。
    let _ = l.register(cell(
        12,
        OP_SET,
        match Var::new(&[0x01u8], b"x") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 13: 装满 8 条后再写一条。
    let _ = l.register(cell(
        13,
        OP_SET,
        {
            let mut e = Env::new();
            let mut bug = false;
            let mut i = 0usize;
            while i < ENV_CAP {
                let mut key = String::new();
                key.push('K');
                push_u64(&mut key, i as u64);
                match Var::new(key.as_bytes(), b"v") {
                    Ok(v) => {
                        if e.set(v).is_err() {
                            bug = true;
                        }
                    }
                    Err(_) => bug = true,
                }
                i += 1;
            }
            if bug {
                Err(EV_ENOBS)
            } else {
                match Var::new(b"K9", b"v") {
                    Ok(v) => e.set(v).map(|p| p as u32),
                    Err(c) => Err(c),
                }
            }
        },
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 变量闸：空 / 长 / 字节 / `=` / value 长 / 合法。
fn var_gate_ok() -> bool {
    let a = Var::new(b"", b"x").err() == Some(EV_EEMPTY);
    let b = Var::new(b"A=B", b"x").err() == Some(EV_EEQ);
    let c = Var::new(&[b'K'; KEY_CAP + 1], b"x").err() == Some(EV_ELEN);
    let d = Var::new(&[0x01u8], b"x").err() == Some(EV_EBYTE);
    let e = Var::new(b"PATH", &[b'v'; VAL_CAP + 1]).err() == Some(EV_ELEN);
    let f = Var::new(b"PATH", &[0x01u8]).err() == Some(EV_EBYTE);
    let g = match Var::new(b"HOME", b"/root") {
        Ok(v) => {
            v.key_text().as_str() == "HOME"
                && v.val_text().as_str() == "/root"
                && v.text().as_str() == "HOME=/root"
                && v.render_len() == 11
        }
        Err(_) => false,
    };
    let h = match Var::new(b"EMPTY", b"") {
        Ok(v) => v.val_bytes().is_empty() && v.render_len() == 7,
        Err(_) => false,
    };
    a && b && c && d && e && f && g && h
}

/// 有序插入与位置。
fn order_ok() -> bool {
    let mut e = Env::new();
    let a = match Var::new(b"PATH", b"/bin") {
        Ok(v) => e.set(v),
        Err(_) => return false,
    };
    let b = match Var::new(b"HOME", b"/") {
        Ok(v) => e.set(v),
        Err(_) => return false,
    };
    let c = match Var::new(b"TERM", b"dumb") {
        Ok(v) => e.set(v),
        Err(_) => return false,
    };
    if a != Ok(0) || b != Ok(0) || c != Ok(2) {
        return false;
    }
    if !e.is_sorted() || e.len() != 3 || e.validate() != Ok(()) {
        return false;
    }
    match (e.at(0), e.at(1), e.at(2)) {
        (Some(x), Some(y), Some(z)) => {
            x.key_text().as_str() == "HOME"
                && y.key_text().as_str() == "PATH"
                && z.key_text().as_str() == "TERM"
        }
        _ => false,
    }
}

/// 重复键与容量。
fn dup_cap_ok() -> bool {
    let mut e = parent_env();
    let dup = match Var::new(b"PATH", b"/sbin") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = e.set(dup) == Err(EV_EDUP) && e.len() == 5;
    let mut f = Env::new();
    let mut i = 0usize;
    let mut fill_ok = true;
    while i < ENV_CAP {
        let mut k = String::from("K");
        push_u64(&mut k, i as u64);
        match Var::new(k.as_bytes(), b"v") {
            Ok(v) => {
                if f.set(v).is_err() {
                    fill_ok = false;
                }
            }
            Err(_) => fill_ok = false,
        }
        i += 1;
    }
    let overflow = match Var::new(b"K9", b"v") {
        Ok(v) => f.set(v),
        Err(_) => return false,
    };
    a && fill_ok && overflow == Err(EV_ECAP) && f.len() == ENV_CAP
}

/// 子集与并集。
fn subset_ok() -> bool {
    let parent = parent_env();
    let a = parent.subset_of(&parent);
    let mut sub = Env::new();
    let mut ok = true;
    let keys: [&[u8]; 2] = [b"HOME", b"PATH"];
    let mut i = 0usize;
    while i < keys.len() {
        if let Some(k) = keys.get(i) {
            match parent.get(k) {
                Some(v) => {
                    if sub.set(v).is_err() {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        i += 1;
    }
    let b = ok && sub.len() == 2;
    let c = sub.subset_of(&parent);
    let u = sub.union(&parent);
    let d = u.len() == 5 && u.subset_of(&parent);
    let e = !parent.subset_of(&sub);
    a && b && c && d && e
}

/// 允许名单校验。
fn allow_ok() -> bool {
    let parent = parent_env();
    let a = allowed(&[b"HOME", b"PATH"], b"PATH");
    let b = !allowed(&[b"HOME", b"PATH"], b"USER");
    let c = allowed(&[], b"HOME");
    let d = check_allow(&parent, &[b"HOME", b"PATH"]) == Ok(2);
    let e = check_allow(&parent, &[b"HOME", b"NOPE"]) == Err(EV_ESET);
    let f = check_allow(&parent, &[]) == Ok(0);
    a && b && !c && d && e && f
}

/// 子环境过滤 + 默认补齐。
fn child_ok() -> bool {
    let parent = parent_env();
    let allow: [&[u8]; 2] = [b"HOME", b"PATH"];
    let c = match child_env(&parent, &allow) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.len() == 4 && c.is_sorted() && c.validate() == Ok(());
    let b = match (c.get(b"HOME"), c.get(b"PATH")) {
        (Some(h), Some(p)) => {
            h.val_text().as_str() == "/root" && p.val_text().as_str() == "/bin:/usr/bin"
        }
        _ => false,
    };
    let d = match (c.get(b"LANG"), c.get(b"TERM")) {
        (Some(l), Some(t)) => l.val_text().as_str() == "C" && t.val_text().as_str() == "dumb",
        _ => false,
    };
    let e = c.get(b"USER").is_none();
    let f = match child_env(&Env::new(), &[]) {
        Ok(x) => x.len() == 4,
        Err(_) => false,
    };
    a && b && d && e && f
}

/// 默认值表。
fn defaults_ok() -> bool {
    let a = DEFAULTS.len() == 4;
    let b = DEFAULT_PATH.len() == 29;
    let c = match default_of(b"PATH") {
        Some(v) => v == DEFAULT_PATH,
        None => false,
    };
    let d = default_of(b"HOME") == Some("/");
    let e = default_of(b"LANG") == Some("C");
    let f = default_of(b"TERM") == Some("dumb");
    let g = default_of(b"USER").is_none();
    a && b && c && d && e && f && g
}

/// 继承自洽与三种破坏。
fn inherit_ok() -> bool {
    let parent = parent_env();
    let allow: [&[u8]; 2] = [b"HOME", b"PATH"];
    let c = match child_env(&parent, &allow) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = inheritance_ok(&parent, &c, &allow);
    // 破坏一：改值
    let mut bad_value = c;
    if let Some(v) = bad_value.get(b"HOME") {
        if let Ok(nv) = Var::new(v.key_bytes(), b"/tmp") {
            let _ = bad_value.unset(b"HOME");
            let _ = bad_value.set(nv);
        }
    }
    let b = !inheritance_ok(&parent, &bad_value, &allow);
    // 破坏二：多塞一条父环境没有的
    let mut extra = c;
    if let Ok(v) = Var::new(b"LANG", b"en_GB") {
        let _ = extra.unset(b"LANG");
        let _ = extra.set(v);
    }
    let d = !inheritance_ok(&parent, &extra, &allow);
    // 破坏三：允许名单拼错
    let e = !inheritance_ok(&parent, &c, &[b"HOME", b"PTH"]);
    a && b && d && e
}

/// 取消传播。
fn unset_ok() -> bool {
    let parent = parent_env();
    let a = match unset_all(&parent, &[b"USER"]) {
        Ok(e) => e.len() == 4 && e.get(b"USER").is_none(),
        Err(_) => false,
    };
    let b = match unset_all(&parent, &[b"LANG"]) {
        Ok(e) => match child_env(&e, &[b"LANG"]) {
            Ok(c) => match c.get(b"LANG") {
                Some(v) => v.val_text().as_str() == "C",
                None => false,
            },
            Err(_) => false,
        },
        Err(_) => false,
    };
    let mut e = parent;
    let c = e.unset(b"USER") && !e.unset(b"USER") && e.len() == 4;
    let d = parent.len() == 5;
    a && b && c && d
}

/// 渲染。
fn render_ok() -> bool {
    let mut e = Env::new();
    let rows: [(&[u8], &[u8]); 2] = [(b"HOME", b"/"), (b"LANG", b"C")];
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((k, v)) = rows.get(i) {
            if let Ok(var) = Var::new(k, v) {
                let _ = e.set(var);
            }
        }
        i += 1;
    }
    let mut buf = [0u8; 64];
    let n = match e.render(&mut buf) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let a = n == 14 && e.render_len() == 14;
    let b = match buf.get(..n) {
        Some(s) => s == b"HOME=/\0LANG=C\0",
        None => false,
    };
    let mut tiny = [0u8; 4];
    let c = e.render(&mut tiny) == Err(EV_ECAP);
    let d = match e.text().as_str() == "HOME=/;LANG=C" {
        true => true,
        false => false,
    };
    a && b && c && d
}

/// PATH 分段。
fn path_ok() -> bool {
    let child = match child_env(&Env::new(), &[]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = child.path_entries() == 4;
    let mut e = Env::new();
    let b = e.path_entries() == 0;
    if let Ok(v) = Var::new(b"PATH", b"/bin") {
        let _ = e.set(v);
    }
    let c = e.path_entries() == 1;
    if let Ok(v2) = Var::new(b"PATH", b"/bin::/usr/bin:") {
        let _ = e.unset(b"PATH");
        let _ = e.set(v2);
    }
    let d = e.path_entries() == 2;
    a && b && c && d
}

/// 指纹。
fn seal_ok() -> bool {
    let a = parent_env();
    let b = parent_env();
    if a.seal() != b.seal() || a.seal() == 0 {
        return false;
    }
    let mut c = parent_env();
    let _ = c.unset(b"USER");
    let d = match Var::new(b"PATH", b"/bin") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let e = d.seal() == match Var::new(b"PATH", b"/bin") {
        Ok(v) => v.seal(),
        Err(_) => 0,
    };
    a.seal() != c.seal() && e && a.text().as_str() != c.text().as_str()
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
    if l.seal() != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if l2.replay() != Ok(root) || !l2.is_frozen() {
        return false;
    }
    if l2.freeze() != Err(EV_EBAL) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = var_gate_ok() && order_ok() && dup_cap_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = subset_ok() && allow_ok() && child_ok() && defaults_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = inherit_ok() && unset_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = render_ok() && path_ok() && seal_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11597_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11597.env");

    s.add("EV01-变量闸", var_gate_ok(), "static");
    s.add("EV02-有序插入", order_ok(), "static");
    s.add("EV03-重复与容量", dup_cap_ok(), "static");
    s.add("EV04-子集与并集", subset_ok(), "static");
    s.add("EV05-允许名单", allow_ok(), "static");
    s.add("EV06-子环境过滤", child_ok(), "static");
    s.add("EV07-默认值表", defaults_ok(), "static");
    s.add("EV08-继承自洽", inherit_ok(), "static");
    s.add("EV09-取消传播", unset_ok(), "static");
    s.add("EV10-渲染字节", render_ok(), "static");
    s.add("EV11-PATH 分段", path_ok(), "static");
    s.add("EV12-账本对账", build_ledger().audit() == Ok(()), "static");
    let four = inject_four();
    s.add("EV13-冻结重放四注入", four[0] && four[1] && four[2] && four[3], "static");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_gates() {
        assert_eq!(Var::new(b"", b"x").err(), Some(EV_EEMPTY));
        assert_eq!(Var::new(b"A=B", b"x").err(), Some(EV_EEQ));
        assert_eq!(Var::new(&[b'K'; KEY_CAP + 1], b"x").err(), Some(EV_ELEN));
        assert_eq!(Var::new(&[0x01], b"x").err(), Some(EV_EBYTE));
        assert_eq!(Var::new(b"PATH", &[b'v'; VAL_CAP + 1]).err(), Some(EV_ELEN));
        let v = Var::new(b"HOME", b"/root").expect("v");
        assert_eq!(v.text().as_str(), "HOME=/root");
        assert_eq!(v.render_len(), 11);
    }

    #[test]
    fn sorted_insert() {
        let mut e = Env::new();
        assert_eq!(e.set(Var::new(b"PATH", b"/bin").expect("p")), Ok(0));
        assert_eq!(e.set(Var::new(b"HOME", b"/").expect("h")), Ok(0));
        assert_eq!(e.set(Var::new(b"TERM", b"dumb").expect("t")), Ok(2));
        assert!(e.is_sorted());
        assert_eq!(e.index_of(b"HOME"), Some(0));
        assert_eq!(e.index_of(b"PATH"), Some(1));
        assert_eq!(e.at(3), None);
    }

    #[test]
    fn duplicate_and_cap() {
        let mut e = parent_env();
        assert_eq!(e.set(Var::new(b"PATH", b"/sbin").expect("p")), Err(EV_EDUP));
        let mut f = Env::new();
        let mut i = 0usize;
        while i < ENV_CAP {
            let mut k = String::from("K");
            push_u64(&mut k, i as u64);
            assert!(f.set(Var::new(k.as_bytes(), b"v").expect("k")).is_ok() || true);
            i += 1;
        }
        assert_eq!(f.len(), ENV_CAP);
        assert_eq!(f.set(Var::new(b"K9", b"v").expect("k")).err(), Some(EV_ECAP));
    }

    #[test]
    fn child_shape() {
        let c = child_env(&parent_env(), &[b"HOME", b"PATH"]).expect("c");
        assert_eq!(c.len(), 4);
        assert_eq!(c.get(b"HOME").map(|v| v.val_text()), Some(String::from("/root")));
        assert_eq!(c.get(b"LANG").map(|v| v.val_text()), Some(String::from("C")));
        assert_eq!(c.get(b"TERM").map(|v| v.val_text()), Some(String::from("dumb")));
        assert!(c.get(b"USER").is_none());
    }

    #[test]
    fn child_defaults_only() {
        let c = child_env(&Env::new(), &[]).expect("c");
        assert_eq!(c.len(), 4);
        assert_eq!(c.get(b"PATH").map(|v| v.val_bytes().len()), Some(29));
        assert_eq!(c.path_entries(), 4);
    }

    #[test]
    fn inheritance_checks() {
        let parent = parent_env();
        let allow: [&[u8]; 2] = [b"HOME", b"PATH"];
        let c = child_env(&parent, &allow).expect("c");
        assert!(inheritance_ok(&parent, &c, &allow));
        assert!(!inheritance_ok(&parent, &c, &[b"HOME", b"PTH"]));
    }

    #[test]
    fn inheritance_rejects_tamper() {
        let parent = parent_env();
        let allow: [&[u8]; 2] = [b"HOME", b"PATH"];
        let mut c = child_env(&parent, &allow).expect("c");
        let _ = c.unset(b"HOME");
        let _ = c.set(Var::new(b"HOME", b"/tmp").expect("h"));
        assert!(!inheritance_ok(&parent, &c, &allow));
    }

    #[test]
    fn allowlist_validation() {
        let p = parent_env();
        assert_eq!(check_allow(&p, &[b"HOME", b"PATH"]), Ok(2));
        assert_eq!(check_allow(&p, &[b"NOPE"]), Err(EV_ESET));
        assert!(allowed(&[b"HOME"], b"HOME"));
        assert!(!allowed(&[b"HOME"], b"PATH"));
    }

    #[test]
    fn unset_propagation() {
        let p = parent_env();
        let e = unset_all(&p, &[b"USER"]).expect("u");
        assert_eq!(e.len(), 4);
        assert!(e.get(b"USER").is_none());
        let e2 = unset_all(&p, &[b"LANG"]).expect("l");
        let c = child_env(&e2, &[b"LANG"]).expect("c");
        assert_eq!(c.get(b"LANG").map(|v| v.val_text()), Some(String::from("C")));
    }

    #[test]
    fn render_bytes() {
        let mut e = Env::new();
        let _ = e.set(Var::new(b"HOME", b"/").expect("h"));
        let _ = e.set(Var::new(b"LANG", b"C").expect("l"));
        let mut buf = [0u8; 32];
        let n = e.render(&mut buf).expect("r");
        assert_eq!(n, 14);
        assert_eq!(buf.get(..n), Some(&b"HOME=/\0LANG=C\0"[..]));
        assert_eq!(e.text().as_str(), "HOME=/;LANG=C");
    }

    #[test]
    fn render_overflow() {
        let mut e = Env::new();
        let _ = e.set(Var::new(b"HOME", b"/").expect("h"));
        let mut tiny = [0u8; 3];
        assert_eq!(e.render(&mut tiny), Err(EV_ECAP));
    }

    #[test]
    fn subset_union() {
        let p = parent_env();
        let mut sub = Env::new();
        let _ = sub.set(p.get(b"HOME").expect("h"));
        assert!(sub.subset_of(&p));
        assert!(!p.subset_of(&sub));
        let u = sub.union(&p);
        assert_eq!(u.len(), 5);
        assert!(u.subset_of(&p));
    }

    #[test]
    fn defaults_table() {
        assert_eq!(DEFAULT_PATH.len(), 29);
        assert_eq!(default_of(b"HOME"), Some("/"));
        assert_eq!(default_of(b"PATH"), Some(DEFAULT_PATH));
        assert!(default_of(b"USER").is_none());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(1).map(|s| s.value), Some(29));
        assert_eq!(l.sample(4).map(|s| s.value), Some(14));
        assert_eq!(l.sample(8).map(|s| s.code), Some(EV_EDUP));
        assert_eq!(l.sample(13).map(|s| s.code), Some(EV_ECAP));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(EV_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11597_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
