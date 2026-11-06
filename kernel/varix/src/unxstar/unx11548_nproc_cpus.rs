//! UNX-F11548 nproc 处理器数查询
//!
//! `nproc` 的答案不是「机器有几个核」，而是「本进程能用几个核」。两者之间隔着
//! 三道集合运算，任何一道漏掉都会给出一个偏大的数字：
//!
//! ```text
//! 生效集 = 已安装 ∩ 在线 ∩ 亲和掩码 ∩ cgroup cpuset
//! ```
//!
//! - **已安装**（installed）是硬件报告的全部处理器；
//! - **在线**（online）是可被调度器分配任务的处理器，热插拔会改变它；
//! - **亲和掩码**（affinity）是 `sched_setaffinity` 设置的、进程级的上限；
//! - **cpuset** 是容器/控制组再压一层的上限。
//!
//! 四层里任何一层先压后压结果都一样（交集可交换），但记录必须先于运算——把
//! 「设置了亲和掩码」和「亲和掩码恰好等于全集」混为一谈，会让「显式钉在一个核
//! 上」与「从未钉过」变成同一个状态，之后任何一次合并都无法还原。
//!
//! 处理器集合统一用位掩码表示：位下标即 CPU 序号。本模块上限 64 个处理器，
//! 一个 `u64` 装得下，于是并、交、差、计数全部是一条逻辑运算，没有循环。
//!
//! 掩码的文本形式与 `taskset` / `/proc/self/status:Cpus_allowed_list` 一致：
//! 逗号分隔的区间与单点，如 `0-3,6,8-9`。解析要求区间左端不大于右端；渲染反过
//! 来把长度不小于 2 的连续段压成区间——这两条互为逆运算，是掩码层最重要的判据。
//!
//! `--ignore=N` 是 `nproc` 的兼容选项：从生效集里再挖掉 N 个处理器，通常用来
//! 给守护进程留出余量。N 必须小于生效集大小，否则「可用 0 个处理器」是矛盾的，
//! 不能当成合法结果返回。
//!
//! 闸门次序：
//!
//! - `Mask::parse`：空串 → 空段 → 非数字 → 区间反序 → 序号越界；
//! - `Topo::new`：零处理器 → 超上限；
//! - `Topo::restrict` / `Topo::cpuset`：掩码为空或与已安装集无交 → 拒绝；
//! - `Topo::nproc`：`--all` 与 `--ignore` 同用 → 生效集为空 → `--ignore` 越界。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf350;

/// 掩码语法非法。
pub const NR_EINVAL: u32 = SEG_BASE + 0x01;
/// CPU 序号越界。
pub const NR_ERANGE: u32 = SEG_BASE + 0x02;
/// 区间反序。
pub const NR_EORDER: u32 = SEG_BASE + 0x03;
/// 处理器集合为空。
pub const NR_EEMPTY: u32 = SEG_BASE + 0x04;
/// `--ignore` 超出可用数。
pub const NR_EIGN: u32 = SEG_BASE + 0x05;
/// 选项组合非法。
pub const NR_ECFG: u32 = SEG_BASE + 0x06;
/// 渲染缓冲超限。
pub const NR_EFULL: u32 = SEG_BASE + 0x07;
/// 状态不符。
pub const NR_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const NR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const NR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const NR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const NR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const NR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 处理器上限。位掩码是一个 `u64`，故此值必须等于 64。
pub const CAP: usize = 64;
/// 渲染文本上限。
pub const DRAWLEN: usize = 256;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：解析掩码。
pub const OP_PARSE: u8 = 1;
/// 操作码：查询生效集。
pub const OP_QUERY: u8 = 2;
/// 操作码：下线。
pub const OP_OFFLINE: u8 = 3;
/// 操作码：设置亲和掩码。
pub const OP_RESTRICT: u8 = 4;
/// 操作码：求处理器数。
pub const OP_NPROC: u8 = 5;

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

/// 处理器位掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mask {
    bits: u64,
}

impl Mask {
    /// 空集。
    pub const fn none() -> Mask {
        Mask { bits: 0 }
    }

    /// 前 `n` 个处理器。
    pub fn full(n: usize) -> Mask {
        if n == 0 {
            return Mask::none();
        }
        if n >= CAP {
            return Mask { bits: u64::MAX };
        }
        Mask { bits: (1u64 << n) - 1 }
    }

    /// 闭区间 `[a, b]`。
    pub fn range(a: usize, b: usize) -> Result<Mask, u32> {
        if a >= CAP || b >= CAP {
            return Err(NR_ERANGE);
        }
        if a > b {
            return Err(NR_EORDER);
        }
        let lo = (1u64 << a) - 1;
        let hi = if b + 1 >= CAP { u64::MAX } else { (1u64 << (b + 1)) - 1 };
        Ok(Mask { bits: hi & !lo })
    }

    /// 裸位。
    pub const fn from_bits(bits: u64) -> Mask {
        Mask { bits }
    }

    /// 位模式。
    pub fn bits(&self) -> u64 {
        self.bits
    }

    /// 是否空集。
    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// 处理器数。
    pub fn count(&self) -> u32 {
        self.bits.count_ones()
    }

    /// 是否含 `cpu`。
    pub fn contains(&self, cpu: usize) -> bool {
        if cpu >= CAP {
            return false;
        }
        self.bits & (1u64 << cpu) != 0
    }

    /// 置位。
    pub fn set(&mut self, cpu: usize) -> Result<(), u32> {
        if cpu >= CAP {
            return Err(NR_ERANGE);
        }
        self.bits |= 1u64 << cpu;
        Ok(())
    }

    /// 清位。
    pub fn clear(&mut self, cpu: usize) -> Result<(), u32> {
        if cpu >= CAP {
            return Err(NR_ERANGE);
        }
        self.bits &= !(1u64 << cpu);
        Ok(())
    }

    /// 并集。
    pub fn union(self, o: Mask) -> Mask {
        Mask { bits: self.bits | o.bits }
    }

    /// 交集。
    pub fn intersect(self, o: Mask) -> Mask {
        Mask { bits: self.bits & o.bits }
    }

    /// 差集。
    pub fn difference(self, o: Mask) -> Mask {
        Mask { bits: self.bits & !o.bits }
    }

    /// 是否为 `o` 的子集。
    pub fn is_subset_of(&self, o: Mask) -> bool {
        self.bits & !o.bits == 0
    }

    /// 交集大小。
    pub fn intersect_count(&self, o: Mask) -> u32 {
        (self.bits & o.bits).count_ones()
    }

    /// 最小处理器序号。
    pub fn first(&self) -> Option<usize> {
        if self.bits == 0 {
            return None;
        }
        Some(self.bits.trailing_zeros() as usize)
    }

    /// 最大处理器序号。
    pub fn last(&self) -> Option<usize> {
        if self.bits == 0 {
            return None;
        }
        Some(63 - self.bits.leading_zeros() as usize)
    }

    /// 从 `from`（含）起第一个处理器。
    pub fn next_from(&self, from: usize) -> Option<usize> {
        if from >= CAP {
            return None;
        }
        let hi = self.bits >> from;
        if hi == 0 {
            return None;
        }
        Some(from + hi.trailing_zeros() as usize)
    }

    /// 第 `k` 个处理器（0 起）。这是把「任务序号」映射到「处理器序号」的唯一
    /// 入口：跳过下线的核之后，第 k 个可用核不一定是第 k 号核。
    pub fn nth(&self, k: u32) -> Option<usize> {
        if k >= self.count() {
            return None;
        }
        let mut bits = self.bits;
        let mut left = k;
        loop {
            let idx = bits.trailing_zeros() as usize;
            if left == 0 {
                return Some(idx);
            }
            left -= 1;
            bits &= bits - 1;
            if bits == 0 {
                return None;
            }
        }
    }

    /// `cpu` 在集合内的序号。
    pub fn rank(&self, cpu: usize) -> Option<u32> {
        if !self.contains(cpu) {
            return None;
        }
        if cpu >= CAP {
            return None;
        }
        let below = if cpu == 0 {
            0u64
        } else {
            self.bits & ((1u64 << cpu) - 1)
        };
        Some(below.count_ones())
    }

    /// 解析 `0-3,6,8-9`。
    pub fn parse(t: &[u8]) -> Result<Mask, u32> {
        if t.is_empty() {
            return Err(NR_EINVAL);
        }
        let mut m = Mask::none();
        let mut i = 0usize;
        loop {
            let (tok, next) = split_token(t, i);
            if tok.is_empty() {
                return Err(NR_EINVAL);
            }
            let (lo, hi) = parse_token(tok)?;
            let seg = Mask::range(lo, hi)?;
            m = m.union(seg);
            match next {
                Some(n) => i = n,
                None => break,
            }
        }
        Ok(m)
    }

    /// 渲染为规范文本：长度不小于 2 的连续段写成区间。
    pub fn render(&self) -> Result<String, u32> {
        if self.bits == 0 {
            return Err(NR_EEMPTY);
        }
        let mut s = String::new();
        let mut i = 0usize;
        while i < CAP {
            if !self.contains(i) {
                i += 1;
                continue;
            }
            let mut j = i;
            while j + 1 < CAP && self.contains(j + 1) {
                j += 1;
            }
            if !s.is_empty() {
                s.push(',');
            }
            push_u32(&mut s, i as u32);
            if j > i {
                s.push('-');
                push_u32(&mut s, j as u32);
            }
            i = j + 1;
        }
        if s.len() > DRAWLEN {
            return Err(NR_EFULL);
        }
        Ok(s)
    }
}

/// 切出下一个逗号分隔的段，返回段与下一段的起点。
fn split_token(t: &[u8], from: usize) -> (&[u8], Option<usize>) {
    let mut i = from;
    while i < t.len() {
        match t.get(i) {
            Some(b',') => {
                let seg = t.get(from..i).unwrap_or(b"");
                return (seg, Some(i + 1));
            }
            _ => {}
        }
        i += 1;
    }
    (t.get(from..).unwrap_or(b""), None)
}

/// 解析一段：`n` 或 `a-b`。
fn parse_token(tok: &[u8]) -> Result<(usize, usize), u32> {
    let mut dash = tok.len();
    let mut i = 0usize;
    while i < tok.len() {
        match tok.get(i) {
            Some(b'-') => {
                dash = i;
                break;
            }
            _ => {}
        }
        i += 1;
    }
    if dash == tok.len() {
        let v = parse_u32(tok)?;
        if v as usize >= CAP {
            return Err(NR_ERANGE);
        }
        return Ok((v as usize, v as usize));
    }
    let left = tok.get(..dash).unwrap_or(b"");
    let right = tok.get(dash + 1..).unwrap_or(b"");
    if left.is_empty() || right.is_empty() {
        return Err(NR_EINVAL);
    }
    let a = parse_u32(left)?;
    let b = parse_u32(right)?;
    if a as usize >= CAP || b as usize >= CAP {
        return Err(NR_ERANGE);
    }
    if a > b {
        return Err(NR_EORDER);
    }
    Ok((a as usize, b as usize))
}

/// 解析十进制无符号数，溢出报越界。
fn parse_u32(d: &[u8]) -> Result<u32, u32> {
    let mut v = 0u32;
    let mut i = 0usize;
    while i < d.len() {
        let c = match d.get(i) {
            Some(v) => *v,
            None => return Err(NR_EINVAL),
        };
        if c < b'0' || c > b'9' {
            return Err(NR_EINVAL);
        }
        let dig = (c - b'0') as u32;
        v = match v.checked_mul(10) {
            Some(x) => match x.checked_add(dig) {
                Some(y) => y,
                None => return Err(NR_ERANGE),
            },
            None => return Err(NR_ERANGE),
        };
        i += 1;
    }
    if d.is_empty() {
        return Err(NR_EINVAL);
    }
    Ok(v)
}

/// 追加十进制数字。
fn push_u32(s: &mut String, v: u32) {
    if v >= 10 {
        push_u32(s, v / 10);
    }
    let d = (v % 10) as u8 + b'0';
    s.push(d as char);
}

/// `nproc` 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    all: bool,
    ignore: u32,
}

impl Policy {
    /// 默认：按生效集计数。
    pub const fn new() -> Policy {
        Policy { all: false, ignore: 0 }
    }

    /// `--all`：统计已安装处理器。
    pub const fn all() -> Policy {
        Policy { all: true, ignore: 0 }
    }

    /// `--ignore=N`。
    pub fn with_ignore(self, n: u32) -> Policy {
        Policy { all: self.all, ignore: n }
    }

    /// 是否 `--all`。
    pub fn is_all(&self) -> bool {
        self.all
    }

    /// 忽略数。
    pub fn ignore(&self) -> u32 {
        self.ignore
    }
}

/// 处理器拓扑视图。
///
/// 四层集合都保留原始取值：不把「未设置的亲和掩码」折叠成全集，因为「钉在一核」
/// 与「从未钉过」在后续 `nproc` 之外还会被别的工具区分（例如 TLS 迁移策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topo {
    installed: Mask,
    online: Mask,
    allowed: Mask,
    cpuset: Mask,
    rejects: u32,
}

impl Topo {
    /// 空视图：四层都是空集。仅用于解析失败时的占位，任何查询都会报空集错。
    pub const fn blank() -> Topo {
        Topo {
            installed: Mask::none(),
            online: Mask::none(),
            allowed: Mask::none(),
            cpuset: Mask::none(),
            rejects: 0,
        }
    }

    /// 以 `n` 个处理器建视图：四层都等于全集。
    pub fn new(n: usize) -> Result<Topo, u32> {
        if n == 0 {
            return Err(NR_EEMPTY);
        }
        if n > CAP {
            return Err(NR_ERANGE);
        }
        let all = Mask::full(n);
        Ok(Topo { installed: all, online: all, allowed: all, cpuset: all, rejects: 0 })
    }

    /// 已安装集。
    pub fn installed(&self) -> Mask {
        self.installed
    }

    /// 在线集。
    pub fn online(&self) -> Mask {
        self.online
    }

    /// 亲和掩码。
    pub fn allowed(&self) -> Mask {
        self.allowed
    }

    /// cpuset 上限。
    pub fn cpuset(&self) -> Mask {
        self.cpuset
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已安装数。
    pub fn installed_count(&self) -> u32 {
        self.installed.count()
    }

    /// 在线数。
    pub fn online_count(&self) -> u32 {
        self.online.count()
    }

    /// 生效集。
    ///
    /// 只与在线集求交：已安装集是上界，先建视图时四层同源，故无需重复求交；但
    /// 单独提供 `installed` 仍会参与 `--all` 的计数，两个数字必须分开算。
    pub fn effective_mask(&self) -> Mask {
        self.online.intersect(self.allowed).intersect(self.cpuset)
    }

    /// 生效处理器数。为零是矛盾状态，必须报错而不是返回 0。
    pub fn effective(&self) -> Result<u32, u32> {
        let n = self.effective_mask().count();
        if n == 0 {
            return Err(NR_EEMPTY);
        }
        Ok(n)
    }

    /// 下线若干处理器。
    pub fn offline(&mut self, cpus: &[usize]) -> Result<u32, u32> {
        let mut i = 0usize;
        let mut off = 0u32;
        while i < cpus.len() {
            let cpu = match cpus.get(i) {
                Some(v) => *v,
                None => return Err(NR_EINVAL),
            };
            if cpu >= CAP || !self.installed.contains(cpu) {
                self.rejects = self.rejects.saturating_add(1);
                return Err(NR_ERANGE);
            }
            if self.online.contains(cpu) {
                let _ = self.online.clear(cpu);
                off += 1;
            }
            i += 1;
        }
        Ok(off)
    }

    /// 上线若干处理器。
    pub fn bring_online(&mut self, cpus: &[usize]) -> Result<u32, u32> {
        let mut i = 0usize;
        let mut on = 0u32;
        while i < cpus.len() {
            let cpu = match cpus.get(i) {
                Some(v) => *v,
                None => return Err(NR_EINVAL),
            };
            if cpu >= CAP || !self.installed.contains(cpu) {
                self.rejects = self.rejects.saturating_add(1);
                return Err(NR_ERANGE);
            }
            if !self.online.contains(cpu) {
                let _ = self.online.set(cpu);
                on += 1;
            }
            i += 1;
        }
        Ok(on)
    }

    /// 设置亲和掩码，返回设置后的亲和处理器数。
    ///
    /// 掩码与已安装集无交时拒绝：`sched_setaffinity` 对「一个可跑的核都没有」的
    /// 掩码本身就返回 `EINVAL`，内核不会接受这种请求。
    pub fn restrict(&mut self, m: Mask) -> Result<u32, u32> {
        if m.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NR_EEMPTY);
        }
        if m.intersect(self.installed).is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NR_EEMPTY);
        }
        self.allowed = m;
        Ok(m.count())
    }

    /// 设置 cpuset 上限。
    pub fn set_cpuset(&mut self, m: Mask) -> Result<u32, u32> {
        if m.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NR_EEMPTY);
        }
        if m.intersect(self.installed).is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NR_EEMPTY);
        }
        self.cpuset = m;
        Ok(m.count())
    }

    /// 恢复亲和掩码为全集（已安装集）。
    pub fn relax(&mut self) -> Mask {
        self.allowed = self.installed;
        self.allowed
    }

    /// 求 `nproc` 的答案。
    pub fn nproc(&self, p: Policy) -> Result<u32, u32> {
        if p.all && p.ignore > 0 {
            return Err(NR_ECFG);
        }
        if p.all {
            let n = self.installed.count();
            if n == 0 {
                return Err(NR_EEMPTY);
            }
            return Ok(n);
        }
        let base = self.effective()?;
        if p.ignore >= base {
            return Err(NR_EIGN);
        }
        Ok(base - p.ignore)
    }

    /// 把任务序号映射到处理器序号：在生效集上按序号取模。
    pub fn assign(&self, job: u32) -> Result<usize, u32> {
        let m = self.effective_mask();
        let n = m.count();
        if n == 0 {
            return Err(NR_EEMPTY);
        }
        match m.nth(job % n) {
            Some(c) => Ok(c),
            None => Err(NR_EEMPTY),
        }
    }

    /// 处理器在生效集里的序号。
    pub fn rank(&self, cpu: usize) -> Option<u32> {
        self.effective_mask().rank(cpu)
    }

    /// 拓扑自洽：在线集 ⊆ 已安装集，四层均不越界。
    pub fn verify(&self) -> bool {
        self.online.is_subset_of(self.installed)
            && !self.online.is_empty()
            && !self.allowed.is_empty()
            && !self.cpuset.is_empty()
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
            return Err(NR_ESLOT);
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
            return Err(NR_EBAL);
        }
        if self.frozen && self.seal() != self.root {
            return Err(NR_ESEAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NR_ENOBS),
            };
            if s.slot as usize != i {
                return Err(NR_ESLOT);
            }
            if s.op == 0 {
                return Err(NR_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if *w != s.value {
                            return Err(NR_EVALUE);
                        }
                    }
                    None => return Err(NR_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(NR_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if *w != s.code {
                            return Err(NR_EVALUE);
                        }
                    }
                    None => return Err(NR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NR_EBAL);
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
            return Err(NR_EBAL);
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
            Err(NR_ESEAL)
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
const VALS: [u32; 8] = [5, 8, 2, 6, 5, 3, 2, 8];

/// 六个失败格的错误码，按槽位顺序。
const CODES: [u32; 6] = [NR_EORDER, NR_ERANGE, NR_EINVAL, NR_EIGN, NR_ECFG, NR_EEMPTY];

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let m = match Mask::parse(b"0-3,6") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(0, OP_PARSE, Ok(m.count() as usize)));

    let mut t = match Topo::new(8) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(1, OP_QUERY, t.effective()));
    let _ = l.register(word_cell(2, OP_OFFLINE, t.offline(&[1, 6])));
    let _ = l.register(word_cell(3, OP_QUERY, t.effective()));
    let _ = l.register(word_cell(4, OP_RESTRICT, t.restrict(m)));
    let _ = l.register(word_cell(5, OP_QUERY, t.effective()));
    let _ = l.register(word_cell(6, OP_NPROC, t.nproc(Policy::new().with_ignore(1))));
    let _ = l.register(word_cell(7, OP_NPROC, t.nproc(Policy::all())));

    let _ = l.register(count_cell(8, OP_PARSE, Mask::parse(b"5-3").map(|v| v.count() as usize)));
    let _ = l.register(count_cell(9, OP_PARSE, Mask::parse(b"0-64").map(|v| v.count() as usize)));
    let _ = l.register(count_cell(10, OP_PARSE, Mask::parse(b"1,,2").map(|v| v.count() as usize)));
    let _ = l.register(word_cell(11, OP_NPROC, t.nproc(Policy::new().with_ignore(3))));
    let _ = l.register(word_cell(12, OP_NPROC, t.nproc(Policy::all().with_ignore(2))));
    let _ = l.register(word_cell(13, OP_RESTRICT, t.restrict(Mask::none())));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：掩码解析、渲染与逆运算。
    let a = match Mask::parse(b"0-3,6") {
        Ok(m) => m.count() == 5 && m.contains(0) && m.contains(3) && m.contains(6) && !m.contains(4),
        Err(_) => false,
    };
    let b = match Mask::parse(b"0-3,6") {
        Ok(m) => m.render().map(|s| s == "0-3,6") == Ok(true),
        Err(_) => false,
    };
    let c = match Mask::parse(b"2-2") {
        Ok(m) => m.count() == 1 && m.first() == Some(2) && m.last() == Some(2),
        Err(_) => false,
    };
    let d = match Mask::parse(b"0-7") {
        Ok(m) => match m.render() {
            Ok(s) => Mask::parse(s.as_bytes()) == Ok(m),
            Err(_) => false,
        },
        Err(_) => false,
    };
    let e = match Mask::range(0, 63) {
        Ok(m) => m.count() == 64 && m.last() == Some(63),
        Err(_) => false,
    };
    out[1] = a && b && c && d && e;

    // 二路：四层求交、下线/上线与任务映射。
    let mut t = match Topo::new(8) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let base = t.effective() == Ok(8);
    let off = t.offline(&[1, 6]) == Ok(2);
    let six = t.effective() == Ok(6);
    let rng = match Mask::parse(b"0-3") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let res = t.restrict(rng) == Ok(4);
    let three = t.effective() == Ok(3);
    let nth = t.assign(0) == Ok(0) && t.assign(1) == Ok(2) && t.assign(2) == Ok(3) && t.assign(3) == Ok(0);
    let rank = t.rank(2) == Some(1) && t.rank(1).is_none();
    let back = t.bring_online(&[1, 6]) == Ok(2);
    let four_eff = t.effective() == Ok(4);
    let relax = t.relax().count() == 8;
    out[2] = base && off && six && res && three && nth && rank && back && four_eff && relax && t.verify();

    // 三路：六类拒绝与冻结重放。
    let order = Mask::parse(b"5-3") == Err(NR_EORDER);
    let rng2 = Mask::parse(b"0-64") == Err(NR_ERANGE);
    let inv = Mask::parse(b"1,,2") == Err(NR_EINVAL);
    let empty = Mask::parse(b"") == Err(NR_EINVAL);
    let ign = t.nproc(Policy::new().with_ignore(8)) == Err(NR_EIGN);
    let cfg = t.nproc(Policy::all().with_ignore(1)) == Err(NR_ECFG);
    let none = t.restrict(Mask::none()) == Err(NR_EEMPTY);
    let zero = Topo::new(0) == Err(NR_EEMPTY);

    let mut led = build_ledger();
    let one = led.freeze();
    let rep2 = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = order
        && rng2
        && inv
        && empty
        && ign
        && cfg
        && none
        && zero
        && one.is_ok()
        && rep2 == one
        && two.is_err()
        && reg == Err(NR_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11548_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11548.nproc");

    s.add(
        "NP01-掩码解析",
        {
            let a = match Mask::parse(b"0-3,6") {
                Ok(m) => m.count() == 5 && m.contains(6) && !m.contains(4),
                Err(_) => false,
            };
            let b = match Mask::parse(b"7") {
                Ok(m) => m.count() == 1 && m.first() == Some(7) && m.last() == Some(7),
                Err(_) => false,
            };
            let c = match Mask::parse(b"0-0") {
                Ok(m) => m.count() == 1 && m.contains(0),
                Err(_) => false,
            };
            let d = match Mask::parse(b"60-63") {
                Ok(m) => m.count() == 4 && m.contains(63) && m.last() == Some(63),
                Err(_) => false,
            };
            let e = match Mask::parse(b"0-1,0-1,2") {
                Ok(m) => m.count() == 3,
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "NP02-掩码语法拒绝",
        {
            Mask::parse(b"") == Err(NR_EINVAL)
                && Mask::parse(b"1,,2") == Err(NR_EINVAL)
                && Mask::parse(b"1,") == Err(NR_EINVAL)
                && Mask::parse(b"a") == Err(NR_EINVAL)
                && Mask::parse(b"1-") == Err(NR_EINVAL)
                && Mask::parse(b"-3") == Err(NR_EINVAL)
                && Mask::parse(b"5-3") == Err(NR_EORDER)
                && Mask::parse(b"64") == Err(NR_ERANGE)
                && Mask::parse(b"0-64") == Err(NR_ERANGE)
                && Mask::parse(b"99999999999999999999") == Err(NR_ERANGE)
        },
        "static",
    );

    s.add(
        "NP03-渲染与逆运算",
        {
            let a = match Mask::parse(b"0-3,6") {
                Ok(m) => m.render().map(|s| s == "0-3,6") == Ok(true),
                Err(_) => false,
            };
            let b = match Mask::parse(b"0,2,3,4,9") {
                Ok(m) => m.render().map(|s| s == "0,2-4,9") == Ok(true),
                Err(_) => false,
            };
            let c = match Mask::parse(b"5") {
                Ok(m) => m.render().map(|s| s == "5") == Ok(true),
                Err(_) => false,
            };
            let d = Mask::none().render() == Err(NR_EEMPTY);
            let e = match Mask::parse(b"0-5,9,11-13") {
                Ok(m) => match m.render() {
                    Ok(t) => Mask::parse(t.as_bytes()) == Ok(m),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "NP04-集合运算",
        {
            let a = match Mask::parse(b"0-3") {
                Ok(v) => v,
                Err(_) => Mask::none(),
            };
            let b = match Mask::parse(b"2-5") {
                Ok(v) => v,
                Err(_) => Mask::none(),
            };
            let u = a.union(b).count() == 6;
            let i = a.intersect(b).count() == 2;
            let d = a.difference(b).count() == 2;
            let sub = a.difference(b).is_subset_of(a) && !b.is_subset_of(a);
            let ic = a.intersect_count(b) == 2;
            a.count() == 4 && b.count() == 4 && u && i && d && sub && ic
        },
        "static",
    );

    s.add(
        "NP05-序号与迭代",
        {
            let m = match Mask::parse(b"1,3-4,9") {
                Ok(v) => v,
                Err(_) => Mask::none(),
            };
            m.count() == 4
                && m.nth(0) == Some(1)
                && m.nth(1) == Some(3)
                && m.nth(2) == Some(4)
                && m.nth(3) == Some(9)
                && m.nth(4).is_none()
                && m.rank(4) == Some(2)
                && m.rank(9) == Some(3)
                && m.rank(2).is_none()
                && m.next_from(2) == Some(3)
                && m.next_from(5) == Some(9)
                && m.next_from(10).is_none()
        },
        "static",
    );

    s.add(
        "NP06-建视图与全集",
        {
            let t = match Topo::new(8) {
                Ok(v) => v,
                Err(_) => Topo::blank(),
            };
            t.installed_count() == 8
                && t.online_count() == 8
                && t.effective() == Ok(8)
                && t.installed().bits() == 0xff
                && t.verify()
                && Topo::new(0) == Err(NR_EEMPTY)
                && Topo::new(CAP + 1) == Err(NR_ERANGE)
        },
        "static",
    );

    s.add(
        "NP07-下线与上线",
        {
            let mut t = match Topo::new(8) {
                Ok(v) => v,
                Err(_) => Topo::blank(),
            };
            let a = t.offline(&[1, 6]) == Ok(2);
            let b = t.effective() == Ok(6);
            let c = t.offline(&[1]) == Ok(0);
            let d = t.online_count() == 6;
            let e = t.bring_online(&[1]) == Ok(1);
            let f = t.effective() == Ok(7);
            let g = t.offline(&[9]) == Err(NR_ERANGE);
            a && b && c && d && e && f && g && t.verify()
        },
        "static",
    );

    s.add(
        "NP08-四层求交",
        {
            let mut t = match Topo::new(8) {
                Ok(v) => v,
                Err(_) => Topo::blank(),
            };
            let aff = match Mask::parse(b"0-3,6") {
                Ok(v) => v,
                Err(_) => Mask::none(),
            };
            let cset = match Mask::parse(b"2-7") {
                Ok(v) => v,
                Err(_) => Mask::none(),
            };
            let a = t.restrict(aff) == Ok(5);
            let b = t.set_cpuset(cset) == Ok(6);
            let c = t.effective() == Ok(3);
            let inner = t.effective_mask();
            let d = inner.count() == 3
                && inner.contains(2)
                && inner.contains(3)
                && inner.contains(6)
                && !inner.contains(0)
                && !inner.contains(1);
            a && b && c && d && t.verify()
        },
        "static",
    );

    s.add(
        "NP09-目标集为空即拒绝",
        {
            let mut t = match Topo::new(4) {
                Ok(v) => v,
                Err(_) => Topo::blank(),
            };
            let a = t.restrict(Mask::none()) == Err(NR_EEMPTY);
            let far = match Mask::parse(b"40-50") {
                Ok(v) => v,
                Err(_) => Mask::none(),
            };
            let b = t.restrict(far) == Err(NR_EEMPTY);
            let c = t.rejects() == 2;
            let d = t.effective() == Ok(4);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "NP10-处理器数查询",
        {
            let mut t = match Topo::new(8) {
                Ok(v) => v,
                Err(_) => Topo::blank(),
            };
            let _ = t.offline(&[1, 6]);
            let base = t.nproc(Policy::new()) == Ok(6);
            let all = t.nproc(Policy::all()) == Ok(8);
            let one = t.nproc(Policy::new().with_ignore(1)) == Ok(5);
            let five = t.nproc(Policy::new().with_ignore(5)) == Ok(1);
            let over = t.nproc(Policy::new().with_ignore(6)) == Err(NR_EIGN);
            let cfg = t.nproc(Policy::all().with_ignore(1)) == Err(NR_ECFG);
            base && all && one && five && over && cfg
        },
        "static",
    );

    s.add(
        "NP11-任务映射",
        {
            let mut t = match Topo::new(8) {
                Ok(v) => v,
                Err(_) => Topo::blank(),
            };
            let _ = t.offline(&[1]);
            let _ = t.offline(&[6]);
            let a = t.assign(0) == Ok(0);
            let b = t.assign(1) == Ok(2);
            let c = t.assign(5) == Ok(7);
            let d = t.assign(6) == Ok(0);
            let e = t.rank(7) == Some(5);
            let f = t.rank(6).is_none();
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "NP12-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|s| s.value) == Some(5)
                && l.sample(13).map(|s| s.code) == Some(NR_EEMPTY)
        },
        "static",
    );

    s.add(
        "NP13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_parses_lists() {
        let m = match Mask::parse(b"0-3,6,8-9") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(m.count(), 7);
        assert!(m.contains(0) && m.contains(3) && m.contains(6) && m.contains(9));
        assert!(!m.contains(4) && !m.contains(7));
        assert_eq!(m.first(), Some(0));
        assert_eq!(m.last(), Some(9));
    }

    #[test]
    fn mask_rejects_bad_syntax() {
        assert_eq!(Mask::parse(b""), Err(NR_EINVAL));
        assert_eq!(Mask::parse(b"1,,2"), Err(NR_EINVAL));
        assert_eq!(Mask::parse(b"5-3"), Err(NR_EORDER));
        assert_eq!(Mask::parse(b"64"), Err(NR_ERANGE));
        assert_eq!(Mask::parse(b"x"), Err(NR_EINVAL));
    }

    #[test]
    fn mask_render_roundtrips() {
        let m = match Mask::parse(b"0-1,4,7-9") {
            Ok(v) => v,
            Err(_) => return,
        };
        let t = match m.render() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(t, "0-1,4,7-9");
        assert_eq!(Mask::parse(t.as_bytes()), Ok(m));
        assert_eq!(Mask::none().render(), Err(NR_EEMPTY));
    }

    #[test]
    fn mask_set_ops() {
        let a = Mask::from_bits(0x0f);
        let b = Mask::from_bits(0x3c);
        assert_eq!(a.union(b).count(), 6);
        assert_eq!(a.intersect(b).count(), 2);
        assert_eq!(a.difference(b).count(), 2);
        assert!(a.difference(b).is_subset_of(a));
        assert_eq!(a.rank(3), Some(3));
        assert_eq!(a.nth(3), Some(3));
        assert!(a.nth(4).is_none());
    }

    #[test]
    fn topo_layers_intersect() {
        let mut t = match Topo::new(8) {
            Ok(v) => v,
            Err(_) => return,
        };
        let _ = t.offline(&[1, 6]);
        assert_eq!(t.effective(), Ok(6));
        let m = match Mask::parse(b"0-3") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(t.restrict(m), Ok(4));
        assert_eq!(t.effective(), Ok(3));
        assert_eq!(t.assign(1), Ok(2));
        assert!(t.verify());
    }

    #[test]
    fn nproc_subtracts_ignore() {
        let mut t = match Topo::new(4) {
            Ok(v) => v,
            Err(_) => return,
        };
        let _ = t.offline(&[2]);
        assert_eq!(t.nproc(Policy::new()), Ok(3));
        assert_eq!(t.nproc(Policy::all()), Ok(4));
        assert_eq!(t.nproc(Policy::new().with_ignore(2)), Ok(1));
        assert_eq!(t.nproc(Policy::new().with_ignore(3)), Err(NR_EIGN));
        assert_eq!(t.nproc(Policy::all().with_ignore(1)), Err(NR_ECFG));
    }

    #[test]
    fn empty_target_rejected() {
        let mut t = match Topo::new(2) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(t.restrict(Mask::none()), Err(NR_EEMPTY));
        let far = match Mask::parse(b"10-12") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(t.set_cpuset(far), Err(NR_EEMPTY));
        assert_eq!(t.rejects(), 2);
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
        assert_eq!(l.register(Sample::blank()), Err(NR_ESLOT));
        assert_eq!(l.rejects(), 2);
    }
}
