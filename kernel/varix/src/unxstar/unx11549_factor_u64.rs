//! UNX-F11549 factor 因数分解
//!
//! 完整分解一个 64 位整数需要三件事：判定素数、找一个非平凡因子、对商递归。本
//! 模块把三件事都做成可单独观测的纯函数，因为 `factor` 的输出只有一行，而一行
//! 输出背后任何一步出错都会表现为「分解结果对，但意义不对」：
//!
//! - **素性判定**用确定性 Miller–Rabin。前 12 个素数为基足以覆盖整个 `u64` 域
//!   （已知界远大于 2^64），因此不需要随机基，也就没有「偶发误判」这种可能。
//! - **非平凡因子**用 Pollard rho（Floyd 双指针，常数 c 从 1 递增）。rho 是概率
//!   算法但那是指迭代次数，本实现把迭代上限固定，于是同一个输入必然给出同一个
//!   结果——确定性是账本能重放的前提。
//! - **小因子**先做试除到 1000。rho 对偶数与 3 的倍数最慢，先把小素数剥掉能显著
//!   缩短 ρ 链，也让最常见的输入（合数带小因子）走最短路径。
//!
//! 模乘必须用 `u128` 中间量：`a * b mod m` 在 `a, b` 都接近 2^64 时若先乘再取模
//! 会溢出，而 `u128` 恰好装得下两个 `u64` 的乘积。
//!
//! 分解结果的表示是「升序的 (素因子, 指数) 对」，压掉重复因子。升序不是装饰：
//! `12: 2 2 3` 与 `12: 3 2 2` 是同一个分解，只有规定唯一顺序，两份账本才能逐字
//! 比对。指数的类型是 `u8`——`2^255` 早已越过 `u64`，`u8` 足够且能在插入时直接
//! 报错。
//!
//! 由分解结果导出的三个数论量各自独立实现，不复用中间量：约数个数是
//! `Π(e+1)`，欧拉函数是 `Π p^(e-1)·(p-1)`，根号（无平方核）是 `Π p`。三者都
//! 用饱和乘，宁可给出 `u64::MAX` 也不回绕——回绕会把「溢出」伪装成一个合法值。
//!
//! 闸门次序：
//!
//! - `factorize`：零 → 试除小素数 → ρ 链（失败报剩余因子非素）；
//! - `Fact::push`：非素 → 指数为零 → 次序非升 → 容量已满；
//! - `Fact::render`：未绑定 → 缓冲超限。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf360;

/// 零没有分解。
pub const FA_EZERO: u32 = SEG_BASE + 0x01;
/// 超出支撑域。
pub const FA_ERANGE: u32 = SEG_BASE + 0x02;
/// 因子种类超出上限。
pub const FA_EFULL: u32 = SEG_BASE + 0x03;
/// 指数非法。
pub const FA_EEXP: u32 = SEG_BASE + 0x04;
/// 剩余因子非素。
pub const FA_EPRIME: u32 = SEG_BASE + 0x05;
/// 状态不符（未绑定或已绑定）。
pub const FA_ESTATE: u32 = SEG_BASE + 0x06;
/// 渲染缓冲超限。
pub const FA_EBUF: u32 = SEG_BASE + 0x07;
/// 因子次序非升。
pub const FA_EORDER: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const FA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const FA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const FA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const FA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const FA_ENOBS: u32 = SEG_BASE + 0x0d;

/// 不同素因子的种类上限。
///
/// 前 16 个素数的乘积已越过 `u64`，故 `u64` 内最多只有 15 种不同素因子；留一格
/// 余量让「超限」这条判据可以被真实触发。
pub const MAXF: usize = 16;
/// 分解栈深度：`2^63` 至多 63 个因子，留一格。
pub const MAXSTACK: usize = 64;
/// 试除上限。
pub const SMALL: u64 = 1000;
/// 渲染文本上限。
pub const BUFLEN: usize = 32;
/// ρ 链常数上限。
pub const CMAX: u64 = 32;
/// ρ 链迭代上限。
pub const ITER: u32 = 200_000;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：分解。
pub const OP_FACTOR: u8 = 1;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 2;
/// 操作码：插入因子。
pub const OP_PUSH: u8 = 3;
/// 操作码：欧拉函数。
pub const OP_TOTIENT: u8 = 4;
/// 操作码：约数个数。
pub const OP_DIVISORS: u8 = 5;
/// 操作码：统计。
pub const OP_STAT: u8 = 6;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 模乘：用 `u128` 中间量避免溢出。
pub fn mulmod(a: u64, b: u64, m: u64) -> u64 {
    if m == 0 {
        return 0;
    }
    ((a as u128 * b as u128) % m as u128) as u64
}

/// 模幂。
pub fn powmod(b: u64, e: u64, m: u64) -> u64 {
    if m == 1 {
        return 0;
    }
    let mut base = b % m;
    let mut exp = e;
    let mut acc = 1u64 % m;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = mulmod(acc, base, m);
        }
        base = mulmod(base, base, m);
        exp >>= 1;
    }
    acc
}

/// 最大公因数（欧几里得）。
pub fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// 差的绝对值。
fn gap(a: u64, b: u64) -> u64 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

/// `(x² + c) mod n`，全用 `u128` 计算。
fn sqc(x: u64, c: u64, n: u64) -> u64 {
    (((x as u128 * x as u128) + c as u128) % n as u128) as u64
}

/// Miller–Rabin 的确定性基组：前 12 个素数覆盖整个 `u64` 域。
const MR_BASES: [u64; 12] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];

/// 素性判定。
pub fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    let mut i = 0usize;
    while i < MR_BASES.len() {
        if let Some(p) = MR_BASES.get(i) {
            if n % *p == 0 {
                return n == *p;
            }
        }
        i += 1;
    }
    let mut d = n - 1;
    let mut s = 0u32;
    while d & 1 == 0 {
        d >>= 1;
        s += 1;
    }
    let mut k = 0usize;
    while k < MR_BASES.len() {
        let a = match MR_BASES.get(k) {
            Some(v) => *v,
            None => break,
        };
        if a % n == 0 {
            k += 1;
            continue;
        }
        let mut x = powmod(a % n, d, n);
        if x == 1 || x == n - 1 {
            k += 1;
            continue;
        }
        let mut seen = false;
        let mut r = 1u32;
        while r < s {
            x = mulmod(x, x, n);
            if x == n - 1 {
                seen = true;
                break;
            }
            r += 1;
        }
        if !seen {
            return false;
        }
        k += 1;
    }
    true
}

/// Pollard rho：找一个非平凡因子。
///
/// 常数 c 从 1 递增重试；迭代上限固定，因此对同一输入结果确定。返回 `None` 表示
/// 所有常数都在上限内没找到因子——这只可能发生在极小概率的病态输入上，此时上层
/// 报「剩余因子非素」而不是编造一个因子。
pub fn rho(n: u64) -> Option<u64> {
    if n < 4 {
        return None;
    }
    if n % 2 == 0 {
        return Some(2);
    }
    if n % 3 == 0 {
        return Some(3);
    }
    let mut c = 1u64;
    while c <= CMAX {
        let mut x = 2u64;
        let mut y = 2u64;
        let mut d = 1u64;
        let mut i = 0u32;
        while d == 1 && i < ITER {
            x = sqc(x, c, n);
            y = sqc(sqc(y, c, n), c, n);
            d = gcd(gap(x, y), n);
            i += 1;
        }
        if d != 1 && d != n {
            return Some(d);
        }
        c += 1;
    }
    None
}

/// 一个分解结果：升序的 (素因子, 指数) 对，外加被分解的原数（0 表示未绑定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fact {
    primes: [u64; MAXF],
    exps: [u8; MAXF],
    len: usize,
    bound: u64,
}

impl Fact {
    /// 空分解。
    pub const fn empty() -> Fact {
        Fact { primes: [0; MAXF], exps: [0; MAXF], len: 0, bound: 0 }
    }

    /// 种类数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空分解。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 项。
    pub fn at(&self, i: usize) -> Option<(u64, u8)> {
        match (self.primes.get(i), self.exps.get(i)) {
            (Some(p), Some(e)) => Some((*p, *e)),
            _ => None,
        }
    }

    /// 被分解的原数（0 表示未绑定）。
    pub fn bound(&self) -> u64 {
        self.bound
    }

    /// 是否已绑定。
    pub fn is_bound(&self) -> bool {
        self.bound != 0
    }

    /// 插入一个素因子。
    ///
    /// 只接受素数、正指数、严格升序；三条都拒绝「勉强能存但语义已错」的输入。
    pub fn push(&mut self, p: u64, e: u8) -> Result<u32, u32> {
        if self.bound != 0 {
            return Err(FA_ESTATE);
        }
        if !is_prime(p) {
            return Err(FA_EPRIME);
        }
        if e == 0 {
            return Err(FA_EEXP);
        }
        if self.len > 0 {
            let last = match self.primes.get(self.len - 1) {
                Some(v) => *v,
                None => return Err(FA_ESTATE),
            };
            if p <= last {
                return Err(FA_EORDER);
            }
        }
        if self.len >= MAXF {
            return Err(FA_EFULL);
        }
        if let Some(d) = self.primes.get_mut(self.len) {
            *d = p;
        }
        if let Some(d) = self.exps.get_mut(self.len) {
            *d = e;
        }
        self.len += 1;
        Ok(self.len as u32)
    }

    /// 绑定原数。
    pub fn bind(mut self, n: u64) -> Fact {
        self.bound = n;
        self
    }

    /// 重算乘积（`u128`，装得下 `u64` 的全部分解）。
    pub fn product(&self) -> u128 {
        let mut r = 1u128;
        let mut i = 0usize;
        while i < self.len {
            if let (Some(p), Some(e)) = (self.primes.get(i), self.exps.get(i)) {
                let mut k = 0u8;
                while k < *e {
                    r = r.saturating_mul(*p as u128);
                    k += 1;
                }
            }
            i += 1;
        }
        r
    }

    /// 分解是否与绑定值一致。
    pub fn check(&self) -> bool {
        self.bound != 0 && self.product() == self.bound as u128
    }

    /// 欧拉函数值。
    pub fn totient(&self) -> u64 {
        let mut r = 1u64;
        let mut i = 0usize;
        while i < self.len {
            if let (Some(p), Some(e)) = (self.primes.get(i), self.exps.get(i)) {
                let mut k = 1u8;
                while k < *e {
                    r = r.saturating_mul(*p);
                    k += 1;
                }
                r = r.saturating_mul(p.saturating_sub(1));
            }
            i += 1;
        }
        r
    }

    /// 约数个数。
    pub fn divisors(&self) -> u64 {
        let mut r = 1u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.exps.get(i) {
                r = r.saturating_mul(*e as u64 + 1);
            }
            i += 1;
        }
        r
    }

    /// 根号：无平方核。
    pub fn radical(&self) -> u64 {
        let mut r = 1u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.primes.get(i) {
                r = r.saturating_mul(*p);
            }
            i += 1;
        }
        r
    }

    /// 渲染成 `12: 2 2 3` 的形式（重复因子逐个列出）。
    pub fn render(&self) -> Result<String, u32> {
        if self.bound == 0 {
            return Err(FA_ESTATE);
        }
        let mut s = String::new();
        push_u64(&mut s, self.bound);
        s.push(':');
        let mut i = 0usize;
        while i < self.len {
            if let (Some(p), Some(e)) = (self.primes.get(i), self.exps.get(i)) {
                let mut k = 0u8;
                while k < *e {
                    s.push(' ');
                    push_u64(&mut s, *p);
                    k += 1;
                }
            }
            i += 1;
        }
        if s.len() > BUFLEN {
            return Err(FA_EBUF);
        }
        Ok(s)
    }
}

/// 追加十进制数字。
fn push_u64(s: &mut String, v: u64) {
    if v >= 10 {
        push_u64(s, v / 10);
    }
    let d = (v % 10) as u8 + b'0';
    s.push(d as char);
}

/// 插入排序，升序。
fn sort_prefix(a: &mut [u64; MAXSTACK], n: usize) {
    let mut i = 1usize;
    while i < n {
        let key = match a.get(i) {
            Some(v) => *v,
            None => return,
        };
        let mut j = i;
        while j > 0 {
            let prev = match a.get(j - 1) {
                Some(v) => *v,
                None => break,
            };
            if prev <= key {
                break;
            }
            if let Some(d) = a.get_mut(j) {
                *d = prev;
            }
            j -= 1;
        }
        if let Some(d) = a.get_mut(j) {
            *d = key;
        }
        i += 1;
    }
}

/// 完整分解。
pub fn factorize(n: u64) -> Result<Fact, u32> {
    if n == 0 {
        return Err(FA_EZERO);
    }
    let mut f = Fact::empty();
    let mut m = n;

    let mut p = 2u64;
    while p <= SMALL && p.saturating_mul(p) <= m {
        if m % p == 0 {
            let mut e = 0u8;
            while m % p == 0 {
                m /= p;
                e = e.saturating_add(1);
            }
            let _ = f.push(p, e);
        }
        p += 1;
    }

    if m > 1 {
        let mut stack = [0u64; MAXSTACK];
        let mut primes = [0u64; MAXSTACK];
        let mut np = 0usize;
        if let Some(d) = stack.get_mut(0) {
            *d = m;
        }
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let x = match stack.get(sp) {
                Some(v) => *v,
                None => break,
            };
            if x == 1 {
                continue;
            }
            if is_prime(x) {
                if np >= MAXSTACK {
                    return Err(FA_EFULL);
                }
                if let Some(d) = primes.get_mut(np) {
                    *d = x;
                }
                np += 1;
                continue;
            }
            let d = match rho(x) {
                Some(v) => v,
                None => return Err(FA_EPRIME),
            };
            if sp + 2 > MAXSTACK {
                return Err(FA_EFULL);
            }
            if let Some(s) = stack.get_mut(sp) {
                *s = d;
            }
            sp += 1;
            if let Some(s) = stack.get_mut(sp) {
                *s = x / d;
            }
            sp += 1;
        }

        sort_prefix(&mut primes, np);
        let mut i = 0usize;
        while i < np {
            let cur = match primes.get(i) {
                Some(v) => *v,
                None => break,
            };
            let mut e = 0u8;
            while i < np {
                match primes.get(i) {
                    Some(v) => {
                        if *v != cur {
                            break;
                        }
                        e = e.saturating_add(1);
                        i += 1;
                    }
                    None => break,
                }
            }
            let _ = f.push(cur, e);
        }
    }

    Ok(f.bind(n))
}

/// 分解工作台：统计调用、拒绝与累计种类。
#[derive(Clone, Copy, Debug)]
pub struct Work {
    calls: u32,
    rejects: u32,
    kinds: u32,
}

impl Work {
    /// 空工作台。
    pub const fn new() -> Work {
        Work { calls: 0, rejects: 0, kinds: 0 }
    }

    /// 调用次数。
    pub fn calls(&self) -> u32 {
        self.calls
    }

    /// 拒绝次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 累计因子种类数。
    pub fn kinds(&self) -> u32 {
        self.kinds
    }

    /// 分解一个数。
    pub fn factor(&mut self, n: u64) -> Result<Fact, u32> {
        self.calls = self.calls.saturating_add(1);
        match factorize(n) {
            Ok(f) => {
                self.kinds = self.kinds.saturating_add(f.len() as u32);
                Ok(f)
            }
            Err(c) => {
                self.rejects = self.rejects.saturating_add(1);
                Err(c)
            }
        }
    }
}

/// 一个账本样本。
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FA_ESLOT);
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

    /// 是否空账本。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.ok {
                        n += 1;
                    }
                }
                None => return n,
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(FA_EBAL);
        }
        if self.frozen && self.seal() != self.root {
            return Err(FA_ESEAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(FA_ENOBS),
            };
            if s.slot as usize != i {
                return Err(FA_ESLOT);
            }
            if s.op == 0 {
                return Err(FA_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(FA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if *w != s.value {
                            return Err(FA_EVALUE);
                        }
                    }
                    None => return Err(FA_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(FA_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if *w != s.code {
                            return Err(FA_EVALUE);
                        }
                    }
                    None => return Err(FA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(FA_EBAL);
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
            return Err(FA_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(FA_ESEAL)
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

/// 八个成功格的真实观测值。
const VALS: [u32; 8] = [2, 9, 1, 0, 4, 4, 6, 6];

/// 六个失败格的错误码，按槽位顺序。
const CODES: [u32; 6] = [FA_EZERO, FA_EORDER, FA_EEXP, FA_EFULL, FA_EBUF, FA_ESTATE];

/// 首个 17 个素因子，用于打满容量。
const SEED: [u64; 17] =
    [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59];

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut w = Work::new();

    let _ = l.register(count_cell(0, OP_FACTOR, w.factor(12).map(|f| f.len())));

    let twelve = match factorize(12) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(1, OP_RENDER, twelve.render().map(|s| s.len())));

    let _ = l.register(count_cell(2, OP_FACTOR, w.factor(97).map(|f| f.len())));
    let _ = l.register(count_cell(3, OP_FACTOR, w.factor(1).map(|f| f.len())));
    let _ = l.register(count_cell(4, OP_FACTOR, w.factor(600_851_475_143).map(|f| f.len())));
    let _ = l.register(count_cell(5, OP_TOTIENT, w.factor(12).map(|f| f.totient() as usize)));
    let _ = l.register(count_cell(6, OP_DIVISORS, w.factor(12).map(|f| f.divisors() as usize)));
    let _ = l.register(count_cell(7, OP_STAT, Ok(w.calls() as usize)));

    let _ = l.register(count_cell(8, OP_FACTOR, w.factor(0).map(|f| f.len())));

    let mut order = Fact::empty();
    let _ = order.push(5, 1);
    let _ = l.register(word_cell(9, OP_PUSH, order.push(3, 1)));

    let mut zero = Fact::empty();
    let _ = l.register(word_cell(10, OP_PUSH, zero.push(7, 0)));

    let mut big = Fact::empty();
    let mut last = Ok(0u32);
    let mut i = 0usize;
    while i < SEED.len() {
        if let Some(p) = SEED.get(i) {
            last = big.push(*p, 1);
        }
        i += 1;
    }
    let _ = l.register(word_cell(11, OP_PUSH, last));

    let many = match factorize(223_092_870) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(12, OP_RENDER, many.render().map(|s| s.len())));

    let _ = l.register(count_cell(13, OP_RENDER, Fact::empty().render().map(|s| s.len())));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：模运算、素性判定与公因数。
    let a = mulmod(1 << 62, 4, 1_000_000_007) == ((1u128 << 64) % 1_000_000_007) as u64;
    let b = powmod(2, 10, 1000) == 24 && powmod(3, 0, 7) == 1 && powmod(5, 3, 7) == 6;
    let c = gcd(1071, 462) == 21 && gcd(0, 5) == 5 && gcd(5, 0) == 5;
    let d = is_prime(2) && is_prime(97) && is_prime(1_000_000_007) && !is_prime(1) && !is_prime(0);
    let e = !is_prime(91) && !is_prime(600_851_475_143) && is_prime(1471) && is_prime(6857);
    out[1] = a && b && c && d && e;

    // 二路：完整分解与三个数论导出量。
    let mut w = Work::new();
    let f = match w.factor(2100) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let shape = f.len() == 4
        && f.at(0) == Some((2, 2))
        && f.at(1) == Some((3, 1))
        && f.at(2) == Some((5, 2))
        && f.at(3) == Some((7, 1));
    let derived = f.totient() == 480 && f.divisors() == 36 && f.radical() == 210;
    let check = f.check() && f.product() == 2100;
    let semi = match factorize(1_000_000_007 * 1_000_000_009) {
        Ok(v) => v.len() == 2 && v.check(),
        Err(_) => false,
    };
    let pow2 = match factorize(1u64 << 40) {
        Ok(v) => v.len() == 1 && v.at(0) == Some((2, 40)) && v.check(),
        Err(_) => false,
    };
    out[2] = shape && derived && check && semi && pow2 && w.calls() == 1 && w.rejects() == 0;

    // 三路：六类拒绝与冻结重放。
    let zero = factorize(0) == Err(FA_EZERO);
    let order = {
        let mut f0 = Fact::empty();
        let _ = f0.push(5, 1);
        f0.push(3, 1) == Err(FA_EORDER)
    };
    let exp = {
        let mut f0 = Fact::empty();
        f0.push(7, 0) == Err(FA_EEXP)
    };
    let nonprime = {
        let mut f0 = Fact::empty();
        f0.push(9, 1) == Err(FA_EPRIME)
    };
    let full = {
        let mut f0 = Fact::empty();
        let mut r = Ok(0u32);
        let mut i = 0usize;
        while i < SEED.len() {
            if let Some(p) = SEED.get(i) {
                r = f0.push(*p, 1);
            }
            i += 1;
        }
        r == Err(FA_EFULL) && f0.len() == MAXF
    };
    let unbound = Fact::empty().render() == Err(FA_ESTATE);
    let buf = match factorize(223_092_870) {
        Ok(v) => v.render() == Err(FA_EBUF),
        Err(_) => false,
    };

    let mut led = build_ledger();
    let one = led.freeze();
    let rep2 = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = zero
        && order
        && exp
        && nonprime
        && full
        && unbound
        && buf
        && one.is_ok()
        && rep2 == one
        && two.is_err()
        && reg == Err(FA_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11549_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11549.factor");

    s.add(
        "FA01-素性判定",
        is_prime(2)
            && is_prime(3)
            && is_prime(97)
            && is_prime(1_000_000_007)
            && is_prime(1_000_000_009)
            && !is_prime(0)
            && !is_prime(1)
            && !is_prime(4)
            && !is_prime(91)
            && !is_prime(600_851_475_143)
            && !is_prime(u64::MAX),
        "static",
    );

    s.add(
        "FA02-分解升序",
        {
            let f = match factorize(12) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            f.len() == 2
                && f.at(0) == Some((2, 2))
                && f.at(1) == Some((3, 1))
                && f.render() == Ok(String::from("12: 2 2 3"))
                && f.check()
        },
        "static",
    );

    s.add(
        "FA03-素数自身",
        {
            let f = match factorize(97) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            f.len() == 1
                && f.at(0) == Some((97, 1))
                && f.totient() == 96
                && f.divisors() == 2
                && f.radical() == 97
                && f.check()
        },
        "static",
    );

    s.add(
        "FA04-单位元与零",
        {
            let one = match factorize(1) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            one.len() == 0
                && one.is_empty()
                && one.product() == 1
                && one.check()
                && one.render() == Ok(String::from("1:"))
                && factorize(0) == Err(FA_EZERO)
        },
        "static",
    );

    s.add(
        "FA05-四因子半素数",
        {
            let f = match factorize(600_851_475_143) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            f.len() == 4
                && f.at(0) == Some((71, 1))
                && f.at(1) == Some((839, 1))
                && f.at(2) == Some((1471, 1))
                && f.at(3) == Some((6857, 1))
                && f.check()
                && f.radical() == 600_851_475_143
                && f.divisors() == 16
        },
        "static",
    );

    s.add(
        "FA06-大素数乘积",
        {
            let n = 1_000_000_007u64 * 1_000_000_009u64;
            let f = match factorize(n) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            f.len() == 2
                && f.at(0) == Some((1_000_000_007, 1))
                && f.at(1) == Some((1_000_000_009, 1))
                && f.check()
                && f.product() == n as u128
        },
        "static",
    );

    s.add(
        "FA07-幂次压缩",
        {
            let a = match factorize(1u64 << 40) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            let b = match factorize(1024) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            let c = match factorize(3u64.pow(20)) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            a.len() == 1
                && a.at(0) == Some((2, 40))
                && b.at(0) == Some((2, 10))
                && c.at(0) == Some((3, 20))
                && a.check()
                && b.check()
                && c.check()
        },
        "static",
    );

    s.add(
        "FA08-数论导出量",
        {
            let a = match factorize(12) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            let b = match factorize(36) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            let c = match factorize(2100) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            a.totient() == 4
                && a.divisors() == 6
                && a.radical() == 6
                && b.totient() == 12
                && b.divisors() == 9
                && b.radical() == 6
                && c.totient() == 480
                && c.divisors() == 36
                && c.radical() == 210
        },
        "static",
    );

    s.add(
        "FA09-插入次序",
        {
            let mut f = Fact::empty();
            let a = f.push(2, 3) == Ok(1);
            let b = f.push(3, 1) == Ok(2);
            let c = f.push(3, 1) == Err(FA_EORDER);
            let d = f.push(5, 1) == Ok(3);
            let e = f.push(2, 1) == Err(FA_EORDER);
            a && b && c && d && e && f.len() == 3 && !f.is_bound()
        },
        "static",
    );

    s.add(
        "FA10-容量与指数",
        {
            let mut f = Fact::empty();
            let mut i = 0usize;
            while i < MAXF {
                if let Some(p) = SEED.get(i) {
                    let _ = f.push(*p, 1);
                }
                i += 1;
            }
            let capped = f.len() == MAXF;
            let over = f.push(59, 1) == Err(FA_EFULL);
            let mut g = Fact::empty();
            let zero = g.push(2, 0) == Err(FA_EEXP);
            let a = match factorize(1u64 << 63) {
                Ok(v) => v.at(0) == Some((2, 63)),
                Err(_) => false,
            };
            capped && over && zero && a
        },
        "static",
    );

    s.add(
        "FA11-绑定与渲染约束",
        {
            let unbound = Fact::empty().render() == Err(FA_ESTATE);
            let many = match factorize(223_092_870) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            let long = many.render() == Err(FA_EBUF);
            let short = match factorize(12) {
                Ok(v) => v,
                Err(_) => Fact::empty(),
            };
            short.render().map(|t| t.len()) == Ok(9) && short.check() && unbound && long
        },
        "static",
    );

    s.add(
        "FA12-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(1).map(|x| x.value) == Some(9)
                && l.sample(11).map(|x| x.code) == Some(FA_EFULL)
        },
        "static",
    );

    s.add(
        "FA13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::string::String;

    #[test]
    fn primality_known_values() {
        assert!(is_prime(2));
        assert!(is_prime(7919));
        assert!(is_prime(1_000_000_007));
        assert!(!is_prime(0));
        assert!(!is_prime(1));
        assert!(!is_prime(561));
        assert!(!is_prime(1_100_000_007));
    }

    #[test]
    fn modular_arithmetic() {
        assert_eq!(mulmod(1 << 62, 4, 1_000_000_007), ((1u128 << 64) % 1_000_000_007) as u64);
        assert_eq!(powmod(2, 10, 1000), 24);
        assert_eq!(powmod(7, 0, 13), 1);
        assert_eq!(gcd(1071, 462), 21);
        assert_eq!(gcd(0, 9), 9);
    }

    #[test]
    fn factorizes_small() {
        let f = match factorize(12) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(f.len(), 2);
        assert_eq!(f.at(0), Some((2, 2)));
        assert_eq!(f.at(1), Some((3, 1)));
        assert_eq!(f.render(), Ok(String::from("12: 2 2 3")));
        assert!(f.check());
    }

    #[test]
    fn factorizes_semiprime() {
        let n = 1_000_000_007u64 * 1_000_000_009u64;
        let f = match factorize(n) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(f.len(), 2);
        assert!(f.check());
        assert_eq!(f.divisors(), 4);
    }

    #[test]
    fn factorizes_power() {
        let f = match factorize(1u64 << 40) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(f.at(0), Some((2, 40)));
        assert_eq!(f.totient(), 1u64 << 39);
        assert_eq!(f.divisors(), 41);
    }

    #[test]
    fn push_rejects_bad_input() {
        let mut f = Fact::empty();
        assert_eq!(f.push(9, 1), Err(FA_EPRIME));
        assert_eq!(f.push(2, 0), Err(FA_EEXP));
        assert_eq!(f.push(2, 1), Ok(1));
        assert_eq!(f.push(2, 1), Err(FA_EORDER));
        assert_eq!(f.push(3, 1), Ok(2));
        assert!(f.render().is_err());
    }

    #[test]
    fn capacity_is_bounded() {
        let mut f = Fact::empty();
        let mut i = 0usize;
        while i < MAXF {
            if let Some(p) = SEED.get(i) {
                assert!(f.push(*p, 1).is_ok());
            }
            i += 1;
        }
        assert_eq!(f.len(), MAXF);
        assert_eq!(f.push(59, 1), Err(FA_EFULL));
    }

    #[test]
    fn ledger_audits_and_freezes() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        let one = l.freeze();
        assert!(one.is_ok());
        assert_eq!(l.replay(), one);
        assert!(l.freeze().is_err());
        assert_eq!(l.register(Sample::blank()), Err(FA_ESLOT));
        assert_eq!(l.rejects(), 2);
    }
}
