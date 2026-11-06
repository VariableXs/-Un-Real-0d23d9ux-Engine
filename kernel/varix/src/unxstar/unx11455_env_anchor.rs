//! UNX-F11455 判据环境锚（LC_ALL=C 联签）（0x20c30 段）
//!
//! `sort` 与 `grep` 的输出是否可复现，取决于进程启动时锁定的那一份本地化环境。
//! 判据脚本必须先把自己锚在 `LC_ALL=C` 上：一旦某台机器继承了 `LANG=zh_CN.UTF-8`
//! 或 `LC_ALL=C.UTF-8`，排序会按语言规则而不是字节序，`[A-Z]` 的区间也随之改变，
//! 同一条命令在两台机器上给出不同的字节流——而两边各自都“没报错”。
//!
//! 锚定规则按 POSIX 的优先级实现：`LC_ALL` 压过 `LC_COLLATE`，`LC_COLLATE` 压过
//! `LANG`，三者都不可用时才落到默认的 C。空串不是“设为空”而是“视为未设”，但
//! 它留下的降级痕迹必须被记住：`LC_ALL=` 配上 `LANG=C` 虽然最终落在 C 上，仍然
//! 记 EDEGRADE——因为下一个人只要把 `LANG` 改掉，锚就漂了。闸门次序固定为
//! 降级（EDEGRADE）→ 未设（EUNSET）→ 非 C（ELOC）→ 合法。
//!
//! 比较器只实现无符号字节序：`B` < `a`，`"10"` < `"9"`，短串是长串的前缀时排在前。
//! 名字里带 “C” 以外任何东西都拒绝比较（ELOC），宁可失败也不退回语言相关排序。
//! 逆序数用插入排序统计，作为“排序真的换了位”的独立证据。
//!
//! 账本十四格：八格成功（LC_ALL 锚、LANG 锚、LC_ALL 压 LANG、空值回落、
//! POSIX 名字长度、两处字节序、逆序数），六格拒绝（非 C、未设、降级、未知键、
//! 未知值、容量满）。审计逐格钉值。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c30;

/// 段内低十六位。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const EA_EORDER: u16 = SEG_LO + 0x01;
/// 重复键或重复登记。
pub const EA_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const EA_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const EA_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const EA_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const EA_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const EA_EFULL: u16 = SEG_LO + 0x07;
/// 非 C 本地化。
pub const EA_ELOC: u16 = SEG_LO + 0x08;
/// 环境未设。
pub const EA_EUNSET: u16 = SEG_LO + 0x09;
/// 锚被显式清空。
pub const EA_EDEGRADE: u16 = SEG_LO + 0x0a;
/// 未知环境键。
pub const EA_EKEY: u16 = SEG_LO + 0x0b;
/// 未知环境值。
pub const EA_EBADVAL: u16 = SEG_LO + 0x0c;
/// 票号不一致。
pub const EA_ESEAL: u16 = SEG_LO + 0x0d;

/// 键：LC_ALL。
pub const K_LC_ALL: u8 = 1;
/// 键：LC_COLLATE。
pub const K_COLLATE: u8 = 2;
/// 键：LANG。
pub const K_LANG: u8 = 3;
/// 键：无（默认锚）。
pub const K_NONE: u8 = 0;

/// 值：C。
pub const V_C: u8 = 1;
/// 值：POSIX。
pub const V_POSIX: u8 = 2;
/// 值：C.UTF-8（非锚）。
pub const V_UTF8: u8 = 3;
/// 值：空串（视为未设）。
pub const V_EMPTY: u8 = 4;
/// 值：en_US.UTF-8（非锚）。
pub const V_EN_US: u8 = 5;

/// 锚类：C。
pub const CLASS_C: u8 = 1;
/// 锚类：POSIX。
pub const CLASS_POSIX: u8 = 2;
/// 锚类：其它本地化。
pub const CLASS_OTHER: u8 = 3;
/// 锚类：默认（全未设）。
pub const CLASS_DEFAULT: u8 = 4;

/// 序：小于。
pub const ORD_LT: u8 = 1;
/// 序：相等。
pub const ORD_EQ: u8 = 2;
/// 序：大于。
pub const ORD_GT: u8 = 3;

/// 操作：解析锚。
pub const OP_RESOLVE: u8 = 1;
/// 操作：比较。
pub const OP_ORDER: u8 = 2;
/// 操作：排序统计。
pub const OP_SWAPS: u8 = 3;
/// 操作：账本自检。
pub const OP_LEDGER: u8 = 4;

/// 环境表容量。
pub const ENV_CAP: usize = 4;
/// 待排序名字上限。
pub const TOK_CAP: usize = 4;
/// 账本格数。
pub const ROWS: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入。
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

/// 定容环境表。
#[derive(Clone, Copy, Debug)]
pub struct Env {
    keys: [u8; ENV_CAP],
    vals: [u8; ENV_CAP],
    n: usize,
}

impl Env {
    /// 空环境。
    pub const fn new() -> Env {
        Env {
            keys: [0; ENV_CAP],
            vals: [0; ENV_CAP],
            n: 0,
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 查值。
    pub fn lookup(&self, key: u8) -> Option<u8> {
        let mut i = 0usize;
        while i < self.n {
            if self.keys.get(i) == Some(&key) {
                return self.vals.get(i).copied();
            }
            i += 1;
        }
        None
    }

    /// 设值：键 → 值 → 重复 → 容量。
    pub fn push(&mut self, key: u8, val: u8) -> Result<(), u16> {
        if key != K_LC_ALL && key != K_COLLATE && key != K_LANG {
            return Err(EA_EKEY);
        }
        if val != V_C && val != V_POSIX && val != V_UTF8 && val != V_EMPTY && val != V_EN_US {
            return Err(EA_EBADVAL);
        }
        if self.lookup(key).is_some() {
            return Err(EA_EDUP);
        }
        if self.n >= ENV_CAP {
            return Err(EA_EFULL);
        }
        if let Some(slot) = self.keys.get_mut(self.n) {
            *slot = key;
        }
        if let Some(slot) = self.vals.get_mut(self.n) {
            *slot = val;
        }
        self.n += 1;
        Ok(())
    }
}

/// 锚解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub class: u8,
    pub key: u8,
    pub len: u8,
    pub degraded: bool,
}

/// 按 LC_ALL → LC_COLLATE → LANG 解析；空值跳过并留下降级痕迹。
pub fn resolve(env: &Env) -> Anchor {
    let order = [K_LC_ALL, K_COLLATE, K_LANG];
    let mut degraded = false;
    let mut i = 0usize;
    while i < order.len() {
        let k = match order.get(i) {
            Some(v) => *v,
            None => break,
        };
        if let Some(v) = env.lookup(k) {
            if v == V_EMPTY {
                degraded = true;
            } else {
                let (class, len) = match v {
                    V_C => (CLASS_C, 1u8),
                    V_POSIX => (CLASS_POSIX, 5u8),
                    _ => (CLASS_OTHER, 0u8),
                };
                return Anchor {
                    class,
                    key: k,
                    len,
                    degraded,
                };
            }
        }
        i += 1;
    }
    Anchor {
        class: CLASS_DEFAULT,
        key: K_NONE,
        len: 0,
        degraded,
    }
}

/// 要求锚在 C：降级 → 未设 → 非 C。
pub fn require_c(env: &Env) -> Result<u8, u16> {
    let a = resolve(env);
    if a.degraded {
        return Err(EA_EDEGRADE);
    }
    if a.class == CLASS_DEFAULT {
        return Err(EA_EUNSET);
    }
    if a.class != CLASS_C && a.class != CLASS_POSIX {
        return Err(EA_ELOC);
    }
    Ok(a.len)
}

/// 比较闸门：只有 C / POSIX 放行。
pub fn cmp_style(class: u8) -> Result<(), u16> {
    if class == CLASS_C || class == CLASS_POSIX {
        Ok(())
    } else {
        Err(EA_ELOC)
    }
}

/// 无符号字节序：短前缀在前。
pub fn ord(a: &[u8], b: &[u8]) -> u8 {
    let mut i = 0usize;
    loop {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x < y {
                    return ORD_LT;
                }
                if x > y {
                    return ORD_GT;
                }
                i += 1;
            }
            (None, Some(_)) => return ORD_LT,
            (Some(_), None) => return ORD_GT,
            (None, None) => return ORD_EQ,
        }
    }
}

/// 插入排序的交换次数（逆序数）。
pub fn sort_swaps(tokens: &[&[u8]]) -> Result<u32, u16> {
    if tokens.len() > TOK_CAP {
        return Err(EA_EFULL);
    }
    let mut arr: [&[u8]; TOK_CAP] = [b""; TOK_CAP];
    let mut n = 0usize;
    while n < tokens.len() {
        match tokens.get(n) {
            Some(t) => {
                if let Some(slot) = arr.get_mut(n) {
                    *slot = t;
                }
            }
            None => return Err(EA_ENOBS),
        }
        n += 1;
    }
    let mut swaps = 0u32;
    let mut i = 1usize;
    while i < n {
        let mut j = i;
        while j > 0 {
            let prev = match arr.get(j - 1) {
                Some(v) => *v,
                None => return Err(EA_ENOBS),
            };
            let cur = match arr.get(j) {
                Some(v) => *v,
                None => return Err(EA_ENOBS),
            };
            if ord(prev, cur) == ORD_GT {
                arr.swap(j - 1, j);
                swaps = swaps.saturating_add(1);
                j -= 1;
            } else {
                break;
            }
        }
        i += 1;
    }
    Ok(swaps)
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u16,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// 审计回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub rows: u32,
    pub ok: u32,
    pub err: u32,
    pub root: u64,
}

/// 样本账本。
pub struct Ledger {
    rows: [Sample; ROWS],
    len: usize,
    cap: usize,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 默认容量。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); ROWS],
            len: 0,
            cap: ROWS,
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); ROWS],
            len: 0,
            cap: if cap > ROWS { ROWS } else { cap },
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 重复 → 顺序 → 容量 → 一致性。
    pub fn register(&mut self, s: Sample) -> Result<(), u16> {
        if self.frozen {
            self.rejects += 1;
            return Err(EA_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(EA_EDUP);
                    }
                }
                None => return Err(EA_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(EA_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(EA_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(EA_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(EA_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(EA_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(EA_EFULL),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
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
    pub fn audit(&self) -> Result<AuditReport, u16> {
        if self.len != ROWS {
            return Err(EA_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(EA_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EA_ENOBS),
            };
            if s.slot as usize != i {
                return Err(EA_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EA_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(EA_EMISMATCH);
            }
            i += 1;
        }
        let vals = [1u32, 1, 3, 1, 5, 1, 3, 2];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(EA_EMISMATCH);
                    }
                }
                _ => return Err(EA_EBAL),
            }
            k += 1;
        }
        let codes = [
            EA_ELOC, EA_EUNSET, EA_EDEGRADE, EA_EKEY, EA_EBADVAL, EA_EFULL,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(EA_EMISMATCH);
                        }
                    }
                    None => return Err(EA_EBAL),
                },
                None => return Err(EA_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(EA_EBAL);
        }
        Ok(AuditReport {
            rows: self.len as u32,
            ok: self.ok_count() as u32,
            err: self.err_count() as u32,
            root: self.seal(),
        })
    }

    /// 熔断票号。
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
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            self.rejects += 1;
            return Err(EA_EFROZEN);
        }
        let s = self.seal();
        self.root = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u16> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(EA_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(EA_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 越闸次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 成功格。
fn ok_row(slot: u8, op: u8, value: u32) -> Sample {
    Sample {
        slot,
        op,
        ok: true,
        code: 0,
        value,
    }
}

/// 失败格。
fn err_row(slot: u8, op: u8, code: u16) -> Sample {
    Sample {
        slot,
        op,
        ok: false,
        code,
        value: 0,
    }
}

/// 取登记错误码。
fn reg_code(r: Result<(), u16>) -> u16 {
    match r {
        Ok(()) => 0,
        Err(c) => c,
    }
}

/// 结果取数值。
fn value_of(r: Result<u32, u16>) -> u32 {
    match r {
        Ok(v) => v,
        Err(_) => 0xffff_ffff,
    }
}

/// 单键环境。
fn env1(k: u8, v: u8) -> Env {
    let mut e = Env::new();
    let _ = e.push(k, v);
    e
}

/// 双键环境。
fn env2(k1: u8, v1: u8, k2: u8, v2: u8) -> Env {
    let mut e = Env::new();
    let _ = e.push(k1, v1);
    let _ = e.push(k2, v2);
    e
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let all_c = resolve(&env1(K_LC_ALL, V_C));
    let _ = l.register(ok_row(0, OP_RESOLVE, u32::from(all_c.class)));

    let lang_c = resolve(&env1(K_LANG, V_C));
    let _ = l.register(ok_row(1, OP_RESOLVE, u32::from(lang_c.class)));

    let all_wins = resolve(&env2(K_LC_ALL, V_UTF8, K_LANG, V_C));
    let _ = l.register(ok_row(2, OP_RESOLVE, u32::from(all_wins.class)));

    let fallback = resolve(&env2(K_LC_ALL, V_EMPTY, K_COLLATE, V_C));
    let _ = l.register(ok_row(3, OP_RESOLVE, u32::from(fallback.class)));

    let posix_len = match require_c(&env1(K_LC_ALL, V_POSIX)) {
        Ok(v) => u32::from(v),
        Err(_) => 0,
    };
    let _ = l.register(ok_row(4, OP_RESOLVE, posix_len));

    let _ = l.register(ok_row(5, OP_ORDER, u32::from(ord(b"B", b"a"))));
    let _ = l.register(ok_row(6, OP_ORDER, u32::from(ord(b"a", b"A"))));

    let toks: [&[u8]; 3] = [b"b", b"A", b"a"];
    let _ = l.register(ok_row(7, OP_SWAPS, value_of(sort_swaps(&toks))));

    let loc = match require_c(&env1(K_LC_ALL, V_UTF8)) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(8, OP_RESOLVE, loc));

    let unset = match require_c(&Env::new()) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(9, OP_RESOLVE, unset));

    let degrade = match require_c(&env1(K_LC_ALL, V_EMPTY)) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(10, OP_RESOLVE, degrade));

    let mut bad_key = Env::new();
    let key = match bad_key.push(9, V_C) {
        Ok(()) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(11, OP_RESOLVE, key));

    let mut bad_val = Env::new();
    let val = match bad_val.push(K_LC_ALL, 9) {
        Ok(()) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(12, OP_RESOLVE, val));

    let mut small = Ledger::with_cap(3);
    let _ = small.register(ok_row(0, OP_RESOLVE, 1));
    let _ = small.register(ok_row(1, OP_RESOLVE, 1));
    let _ = small.register(ok_row(2, OP_RESOLVE, 1));
    let full = reg_code(small.register(ok_row(3, OP_RESOLVE, 1)));
    let _ = l.register(err_row(13, OP_LEDGER, full));

    l
}

/// 四路注入：重放冻结、优先级链、降级痕迹、篡改复原。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let before = l.len();
    let rep = l.replay();
    let one = l.freeze();
    let two = l.freeze();
    let root = l.root();
    out[0] = rep == one
        && one == Ok(root)
        && two == Err(EA_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    out[1] = {
        let a = resolve(&env2(K_LC_ALL, V_UTF8, K_LANG, V_C));
        let b = resolve(&env2(K_LC_ALL, V_C, K_LANG, V_UTF8));
        let c = resolve(&env2(K_COLLATE, V_C, K_LANG, V_UTF8));
        a.class == CLASS_OTHER
            && a.key == K_LC_ALL
            && b.class == CLASS_C
            && b.key == K_LC_ALL
            && c.class == CLASS_C
            && c.key == K_COLLATE
    };

    out[2] = {
        let a = resolve(&env2(K_LC_ALL, V_EMPTY, K_LANG, V_C));
        let b = resolve(&Env::new());
        a.class == CLASS_C
            && a.degraded
            && a.key == K_LANG
            && require_c(&env2(K_LC_ALL, V_EMPTY, K_LANG, V_C)) == Err(EA_EDEGRADE)
            && b.class == CLASS_DEFAULT
            && !b.degraded
            && require_c(&Env::new()) == Err(EA_EUNSET)
    };

    out[3] = {
        let mut t = build_ledger();
        let clean = t.audit().is_ok();
        let old = match t.sample(7) {
            Some(s) => s.value,
            None => 0,
        };
        if let Some(r) = t.rows.get_mut(7) {
            r.value = old.saturating_add(5);
        }
        let caught = t.audit() == Err(EA_EMISMATCH);
        if let Some(r) = t.rows.get_mut(7) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok()
    };

    out
}

/// 十三判据。
pub fn run_unx11455_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11455.envanchor");
    let l = build_ledger();

    s.add_dyn(
        "EA1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "EA2-LC_ALL优先",
        l.sample(2).map(|x| x.value) == Some(3)
            && resolve(&env2(K_LC_ALL, V_UTF8, K_LANG, V_C)).class == CLASS_OTHER
            && resolve(&env2(K_LC_ALL, V_UTF8, K_LANG, V_C)).key == K_LC_ALL,
        "static",
    );

    s.add(
        "EA3-回落链",
        l.sample(1).map(|x| x.value) == Some(1)
            && resolve(&env1(K_LANG, V_C)).key == K_LANG
            && resolve(&env2(K_COLLATE, V_C, K_LANG, V_UTF8)).key == K_COLLATE
            && resolve(&env2(K_COLLATE, V_C, K_LANG, V_UTF8)).class == CLASS_C,
        "static",
    );

    s.add(
        "EA4-空值降级",
        l.sample(3).map(|x| x.value) == Some(1)
            && resolve(&env2(K_LC_ALL, V_EMPTY, K_COLLATE, V_C)).degraded
            && require_c(&env2(K_LC_ALL, V_EMPTY, K_LANG, V_C)) == Err(EA_EDEGRADE)
            && l.sample(10).map(|x| x.code) == Some(EA_EDEGRADE),
        "static",
    );

    s.add(
        "EA5-未设即拒",
        l.sample(9).map(|x| x.code) == Some(EA_EUNSET)
            && require_c(&Env::new()) == Err(EA_EUNSET)
            && resolve(&Env::new()).class == CLASS_DEFAULT
            && !resolve(&Env::new()).degraded,
        "static",
    );

    s.add(
        "EA6-非C本地化拒绝",
        l.sample(8).map(|x| x.code) == Some(EA_ELOC)
            && require_c(&env1(K_LC_ALL, V_UTF8)) == Err(EA_ELOC)
            && require_c(&env1(K_LC_ALL, V_EN_US)) == Err(EA_ELOC)
            && cmp_style(CLASS_OTHER) == Err(EA_ELOC)
            && cmp_style(CLASS_C).is_ok(),
        "static",
    );

    s.add(
        "EA7-POSIX名字长度",
        l.sample(4).map(|x| x.value) == Some(5)
            && require_c(&env1(K_LC_ALL, V_POSIX)) == Ok(5)
            && require_c(&env1(K_LC_ALL, V_C)) == Ok(1),
        "static",
    );

    s.add(
        "EA8-字节序上界",
        l.sample(5).map(|x| x.value) == Some(1)
            && ord(b"B", b"a") == ORD_LT
            && ord(b"10", b"9") == ORD_LT
            && ord(b"Z", b"a") == ORD_LT,
        "static",
    );

    s.add(
        "EA9-大小写与原序",
        l.sample(6).map(|x| x.value) == Some(3)
            && ord(b"a", b"A") == ORD_GT
            && ord(b"a", b"a") == ORD_EQ
            && ord(b"ab", b"a") == ORD_GT
            && ord(b"a", b"ab") == ORD_LT,
        "static",
    );

    s.add(
        "EA10-逆序数",
        l.sample(7).map(|x| x.value) == Some(2)
            && {
                let t: [&[u8]; 3] = [b"b", b"A", b"a"];
                sort_swaps(&t) == Ok(2)
            }
            && {
                let t: [&[u8]; 3] = [b"A", b"a", b"b"];
                sort_swaps(&t) == Ok(0)
            }
            && {
                let t: [&[u8]; 2] = [b"9", b"10"];
                sort_swaps(&t) == Ok(1)
            },
        "static",
    );

    s.add(
        "EA11-键值闸门",
        l.sample(11).map(|x| x.code) == Some(EA_EKEY)
            && l.sample(12).map(|x| x.code) == Some(EA_EBADVAL)
            && {
                let mut e = Env::new();
                e.push(0, V_C) == Err(EA_EKEY)
                    && e.push(K_LC_ALL, 0) == Err(EA_EBADVAL)
                    && e.push(K_LC_ALL, V_C).is_ok()
                    && e.push(K_LC_ALL, V_POSIX) == Err(EA_EDUP)
                    && e.len() == 1
            },
        "static",
    );

    s.add(
        "EA12-容量闸门",
        l.sample(13).map(|x| x.code) == Some(EA_EFULL)
            && {
                let mut small = Ledger::with_cap(2);
                let _ = small.register(ok_row(0, OP_RESOLVE, 1));
                let _ = small.register(ok_row(1, OP_RESOLVE, 1));
                small.register(ok_row(2, OP_RESOLVE, 1)) == Err(EA_EFULL)
                    && small.len() == 2
            },
        "static",
    );

    s.add(
        "EA13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(EA_EFROZEN)
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && inj == [true, true, true, true]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_chain() {
        assert_eq!(resolve(&env1(K_LC_ALL, V_C)).class, CLASS_C);
        assert_eq!(resolve(&env1(K_LANG, V_C)).key, K_LANG);
        assert_eq!(
            resolve(&env2(K_LC_ALL, V_UTF8, K_LANG, V_C)).class,
            CLASS_OTHER
        );
        assert_eq!(
            resolve(&env2(K_COLLATE, V_C, K_LANG, V_UTF8)).key,
            K_COLLATE
        );
    }

    #[test]
    fn empty_value_degrades() {
        let e = env2(K_LC_ALL, V_EMPTY, K_COLLATE, V_C);
        let a = resolve(&e);
        assert_eq!(a.class, CLASS_C);
        assert!(a.degraded);
        assert_eq!(require_c(&e), Err(EA_EDEGRADE));
        assert_eq!(require_c(&env1(K_LC_ALL, V_EMPTY)), Err(EA_EDEGRADE));
    }

    #[test]
    fn unset_and_foreign_locale() {
        assert_eq!(require_c(&Env::new()), Err(EA_EUNSET));
        assert_eq!(require_c(&env1(K_LC_ALL, V_UTF8)), Err(EA_ELOC));
        assert_eq!(require_c(&env1(K_LC_ALL, V_POSIX)), Ok(5));
        assert_eq!(require_c(&env1(K_LC_ALL, V_C)), Ok(1));
    }

    #[test]
    fn byte_order() {
        assert_eq!(ord(b"B", b"a"), ORD_LT);
        assert_eq!(ord(b"a", b"A"), ORD_GT);
        assert_eq!(ord(b"a", b"a"), ORD_EQ);
        assert_eq!(ord(b"a", b"ab"), ORD_LT);
        assert_eq!(ord(b"10", b"9"), ORD_LT);
    }

    #[test]
    fn inversions() {
        let a: [&[u8]; 3] = [b"b", b"A", b"a"];
        let b: [&[u8]; 3] = [b"A", b"a", b"b"];
        assert_eq!(sort_swaps(&a), Ok(2));
        assert_eq!(sort_swaps(&b), Ok(0));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(EA_EFROZEN));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11455_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
