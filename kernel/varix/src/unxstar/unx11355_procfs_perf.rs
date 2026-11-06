//! UNX-F11355 procfs 性能消费（0xec30 段）
//!
//! procfs 的每一次读取都可能触发现场生成：遍历任务表、拼装 maps、收集 auxv。
//! 对高频消费者（监控、诊断、测试框架）来说，真正稀缺的不是字节而是**生成次数**。
//! 因此本模块把「读」拆成三种消费方式，并把代价显式记账：
//!
//! - `CACHED`：版本未变就复用上次生成的结果，代价 1；版本变了才重新生成，
//!   代价是生成开销。
//! - `FRESH`：无论版本是否变化都重新生成——用于必须看到最新现场的工具。
//! - `REQUIRE`：只接受缓存命中；缓存冷时直接拒绝（`ENOCACHE`），让调用方自己
//!   决定是等下一次周期还是降级，而不是默默付一次生成开销。
//!
//! 记账规则：
//!
//! 1. 成功的读要么是**命中**要么是**重生成**，二者之和恒等于读次数；这条不变式
//!    让监控可以据此反推缓存有效率。
//! 2. 预算不足不是部分成功：一次被拒的读不消耗预算、不改变缓存状态、不计入
//!    计数。否则重试会二次收费。
//! 3. 代价累加一律用 `checked_add`：真到溢出说明计数器体系已经失衡，必须显式
//!    报 `EOVERFLOW`，而不是回绕成「很便宜」。
//!
//! 版本规则：只有**严格前进**的版本才能更新；相等（`ESEQ`）说明调用方没有
//! 推进，落后（`EORDER`）说明拿旧版本覆盖新现场，两者都不允许。
//!
//! 闸门次序：
//! - `attach`：已挂载 → 参数为零；
//! - `update`：未挂载 → 零版本 → 相等 → 回退；
//! - `read`：未挂载 → 零请求 → 方式非法 → 缓存冷 → 预算/溢出。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec30;

/// 参数非法。
pub const EC_EINVAL: u32 = SEG_BASE + 0x01;
/// 已挂载或重复操作。
pub const EC_ERUN: u32 = SEG_BASE + 0x02;
/// 未挂载数据源。
pub const EC_ENOV: u32 = SEG_BASE + 0x03;
/// 版本回退。
pub const EC_EORDER: u32 = SEG_BASE + 0x04;
/// 版本未前进。
pub const EC_ESEQ: u32 = SEG_BASE + 0x05;
/// 预算不足。
pub const EC_EBUDGET: u32 = SEG_BASE + 0x06;
/// 要求命中但缓存是冷的。
pub const EC_ENOCACHE: u32 = SEG_BASE + 0x07;
/// 代价累加溢出。
pub const EC_EOVERFLOW: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const EC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const EC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const EC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const EC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const EC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 消费方式：允许复用缓存。
pub const MODE_CACHED: u8 = 1;
/// 消费方式：强制重新生成。
pub const MODE_FRESH: u8 = 2;
/// 消费方式：只接受缓存命中。
pub const MODE_REQUIRE: u8 = 3;

/// 命中一次的代价。
pub const HIT_COST: u32 = 1;
/// 默认预算。
pub const DEFAULT_BUDGET: u32 = 10_000;

/// 操作码：挂载。
pub const OP_ATTACH: u8 = 1;
/// 操作码：更新版本。
pub const OP_UPDATE: u8 = 2;
/// 操作码：读取。
pub const OP_READ: u8 = 3;

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

/// 数据源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    /// 当前版本。
    pub ver: u32,
    /// 生成出来的字节数。
    pub size: u32,
    /// 一次生成的代价。
    pub gen_cost: u32,
}

/// 性能消费者。
#[derive(Clone, Copy, Debug)]
pub struct Consumer {
    src: Source,
    has: bool,
    cached_ver: u32,
    reads: usize,
    regens: usize,
    hits: usize,
    bytes: u32,
    spent: u32,
    budget: u32,
}

impl Consumer {
    /// 默认预算的空消费者。
    pub const fn new() -> Consumer {
        Consumer {
            src: Source { ver: 0, size: 0, gen_cost: 0 },
            has: false,
            cached_ver: 0,
            reads: 0,
            regens: 0,
            hits: 0,
            bytes: 0,
            spent: 0,
            budget: DEFAULT_BUDGET,
        }
    }

    /// 指定预算的空消费者。
    pub const fn with_budget(budget: u32) -> Consumer {
        Consumer {
            src: Source { ver: 0, size: 0, gen_cost: 0 },
            has: false,
            cached_ver: 0,
            reads: 0,
            regens: 0,
            hits: 0,
            bytes: 0,
            spent: 0,
            budget,
        }
    }

    /// 是否已挂载。
    pub fn attached(&self) -> bool {
        self.has
    }

    /// 当前版本。
    pub fn ver(&self) -> u32 {
        self.src.ver
    }

    /// 缓存版本。
    pub fn cached_ver(&self) -> u32 {
        self.cached_ver
    }

    /// 生成字节数。
    pub fn size(&self) -> u32 {
        self.src.size
    }

    /// 读次数。
    pub fn reads(&self) -> usize {
        self.reads
    }

    /// 重生成次数。
    pub fn regens(&self) -> usize {
        self.regens
    }

    /// 命中次数。
    pub fn hits(&self) -> usize {
        self.hits
    }

    /// 累计字节。
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// 已花预算。
    pub fn spent(&self) -> u32 {
        self.spent
    }

    /// 预算上限。
    pub fn budget(&self) -> u32 {
        self.budget
    }

    /// 挂载数据源。
    pub fn attach(&mut self, ver: u32, size: u32, gen_cost: u32) -> Result<usize, u32> {
        if self.has {
            return Err(EC_ERUN);
        }
        if ver == 0 || size == 0 || gen_cost == 0 {
            return Err(EC_EINVAL);
        }
        self.src = Source { ver, size, gen_cost };
        self.has = true;
        Ok(1)
    }

    /// 推进版本。
    pub fn update(&mut self, ver: u32) -> Result<usize, u32> {
        if !self.has {
            return Err(EC_ENOV);
        }
        if ver == 0 {
            return Err(EC_EINVAL);
        }
        if ver == self.src.ver {
            return Err(EC_ESEQ);
        }
        if ver < self.src.ver {
            return Err(EC_EORDER);
        }
        self.src.ver = ver;
        Ok(ver as usize)
    }

    /// 读取一次：返回实际取到的字节数。
    pub fn read(&mut self, want: u32, mode: u8) -> Result<usize, u32> {
        if !self.has {
            return Err(EC_ENOV);
        }
        if want == 0 {
            return Err(EC_EINVAL);
        }
        if mode < MODE_CACHED || mode > MODE_REQUIRE {
            return Err(EC_EINVAL);
        }
        let stale = self.cached_ver != self.src.ver;
        if mode == MODE_REQUIRE && stale {
            return Err(EC_ENOCACHE);
        }
        let regen = mode == MODE_FRESH || stale;
        let cost = if regen { self.src.gen_cost } else { HIT_COST };
        let next = match self.spent.checked_add(cost) {
            Some(v) => v,
            None => return Err(EC_EOVERFLOW),
        };
        if next > self.budget {
            return Err(EC_EBUDGET);
        }
        self.spent = next;
        if regen {
            self.regens += 1;
            self.cached_ver = self.src.ver;
        } else {
            self.hits += 1;
        }
        self.reads += 1;
        let n = if want < self.src.size { want } else { self.src.size };
        self.bytes = self.bytes.saturating_add(n);
        Ok(n as usize)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.spent > self.budget {
            return false;
        }
        if self.hits + self.regens != self.reads {
            return false;
        }
        if !self.has {
            return self.reads == 0 && self.bytes == 0 && self.spent == 0;
        }
        if self.cached_ver > self.src.ver {
            return false;
        }
        if self.regens == 0 && self.cached_ver != 0 {
            return false;
        }
        let cap = self.src.size.saturating_mul(self.reads as u32);
        self.bytes <= cap
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

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
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
            self.rejects += 1;
            return Err(EC_ESLOT);
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

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
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
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(EC_EBAL);
        }
        const VALS: [u32; 8] = [1, 1024, 1024, 1024, 1024, 2, 4096, 4096];
        const CODES: [u32; 6] =
            [EC_EINVAL, EC_ERUN, EC_ENOV, EC_ESEQ, EC_EBUDGET, EC_ENOCACHE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EC_EBAL),
            };
            if s.slot as usize != i {
                return Err(EC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EC_EVALUE);
                        }
                    }
                    None => return Err(EC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EC_EVALUE);
                        }
                    }
                    None => return Err(EC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EC_EBAL);
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
            self.rejects += 1;
            return Err(EC_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(EC_ESEAL)
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

/// 十四格账本：挂载 → 首读重生成 → 连续命中 → 换版 → 强制新鲜 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Consumer::new();

    let _ = l.register(cell(0, OP_ATTACH, c.attach(1, 4096, 100)));
    let _ = l.register(cell(1, OP_READ, c.read(1024, MODE_CACHED)));
    let _ = l.register(cell(2, OP_READ, c.read(1024, MODE_CACHED)));
    let _ = l.register(cell(3, OP_READ, c.read(1024, MODE_CACHED)));
    let _ = l.register(cell(4, OP_READ, c.read(1024, MODE_CACHED)));
    let _ = l.register(cell(5, OP_UPDATE, c.update(2)));
    let _ = l.register(cell(6, OP_READ, c.read(4096, MODE_CACHED)));
    let _ = l.register(cell(7, OP_READ, c.read(4096, MODE_FRESH)));

    let mut z = Consumer::new();
    let _ = l.register(cell(8, OP_ATTACH, z.attach(0, 1, 1)));

    let mut d = Consumer::new();
    let _ = d.attach(1, 1, 1);
    let _ = l.register(cell(9, OP_ATTACH, d.attach(2, 1, 1)));

    let mut n = Consumer::new();
    let _ = l.register(cell(10, OP_READ, n.read(1, MODE_CACHED)));

    let mut s = Consumer::new();
    let _ = s.attach(5, 8, 4);
    let _ = l.register(cell(11, OP_UPDATE, s.update(5)));

    let mut b = Consumer::with_budget(50);
    let _ = b.attach(1, 100, 100);
    let _ = l.register(cell(12, OP_READ, b.read(10, MODE_CACHED)));

    let mut r = Consumer::new();
    let _ = r.attach(1, 10, 5);
    let _ = l.register(cell(13, OP_READ, r.read(1, MODE_REQUIRE)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：缓存经济学——命中与重生成计数守恒。
    let mut a = Consumer::with_budget(1000);
    let _ = a.attach(1, 1000, 50);
    let r1 = a.read(10, MODE_CACHED);
    let r2 = a.read(10, MODE_CACHED);
    let up = a.update(2);
    let r3 = a.read(10, MODE_CACHED);
    let rq = a.read(10, MODE_REQUIRE);
    out[1] = r1 == Ok(10)
        && r2 == Ok(10)
        && up == Ok(2)
        && r3 == Ok(10)
        && rq == Ok(10)
        && a.reads() == 4
        && a.regens() == 2
        && a.hits() == 2
        && a.spent() == 102
        && a.bytes() == 40
        && a.cached_ver() == 2
        && a.consistent();

    // 二路：五类拒绝各自可辨且不改变状态。
    let mut v = Consumer::new();
    let notyet = v.read(1, MODE_CACHED) == Err(EC_ENOV);
    let upd = v.update(1) == Err(EC_ENOV);
    let _ = v.attach(3, 8, 9);
    let same = v.update(3) == Err(EC_ESEQ);
    let back = v.update(2) == Err(EC_EORDER);
    let zero = v.read(0, MODE_CACHED) == Err(EC_EINVAL);
    let badmode = v.read(1, 9) == Err(EC_EINVAL);
    let cold = v.read(1, MODE_REQUIRE) == Err(EC_ENOCACHE);
    let untouched = v.reads() == 0 && v.spent() == 0 && v.regens() == 0 && v.cached_ver() == 0;
    let mut poor = Consumer::with_budget(5);
    let _ = poor.attach(1, 8, 9);
    let broke = poor.read(1, MODE_CACHED) == Err(EC_EBUDGET);
    let broke_clean = poor.reads() == 0 && poor.spent() == 0;
    out[2] = notyet
        && upd
        && same
        && back
        && zero
        && badmode
        && cold
        && untouched
        && broke
        && broke_clean;

    // 三路：挂载闸门与代价溢出。
    let mut q = Consumer::new();
    let q1 = q.attach(0, 1, 1) == Err(EC_EINVAL);
    let q2 = q.attach(1, 0, 1) == Err(EC_EINVAL);
    let q3 = q.attach(1, 1, 0) == Err(EC_EINVAL);
    let q4 = q.attach(1, 8, 4) == Ok(1);
    let q5 = q.attach(2, 8, 4) == Err(EC_ERUN);
    let mut o = Consumer::with_budget(u32::MAX);
    let _ = o.attach(1, 1, u32::MAX);
    let first = o.read(1, MODE_CACHED) == Ok(1);
    let _ = o.update(2);
    let over = o.read(1, MODE_CACHED) == Err(EC_EOVERFLOW);
    out[3] = q1 && q2 && q3 && q4 && q5 && first && over && q.consistent() && o.spent() == u32::MAX;

    out
}

/// 十三项判据。
pub fn run_unx11355_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11355.procfs_perf");
    let l = build_ledger();

    s.add_dyn(
        "EC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "EC2-挂载一次",
        l.sample(0).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "EC3-首读重生成返回字节",
        l.sample(1).map(|x| x.value) == Some(1024),
        "static",
    );

    s.add(
        "EC4-三次命中仍是千字节",
        l.sample(2).map(|x| x.value) == Some(1024)
            && l.sample(3).map(|x| x.value) == Some(1024)
            && l.sample(4).map(|x| x.value) == Some(1024),
        "static",
    );

    s.add(
        "EC5-版本推进返回新版本",
        l.sample(5).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "EC6-换版后重生成全量",
        l.sample(6).map(|x| x.value) == Some(4096),
        "static",
    );

    s.add(
        "EC7-强制新鲜再生成",
        l.sample(7).map(|x| x.value) == Some(4096),
        "static",
    );

    s.add(
        "EC8-零版本挂载被拒",
        l.sample(8).map(|x| x.code) == Some(EC_EINVAL),
        "static",
    );

    s.add(
        "EC9-重复挂载被拒",
        l.sample(9).map(|x| x.code) == Some(EC_ERUN),
        "static",
    );

    s.add(
        "EC10-未挂载读取被拒",
        l.sample(10).map(|x| x.code) == Some(EC_ENOV),
        "static",
    );

    s.add(
        "EC11-版本未前进被拒",
        l.sample(11).map(|x| x.code) == Some(EC_ESEQ),
        "static",
    );

    s.add(
        "EC12-预算不足与冷缓存分离",
        l.sample(12).map(|x| x.code) == Some(EC_EBUDGET)
            && l.sample(13).map(|x| x.code) == Some(EC_ENOCACHE),
        "static",
    );

    s.add(
        "EC13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11355_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn cached_reads_reuse_generation() {
        let mut c = Consumer::with_budget(1000);
        assert_eq!(c.attach(1, 512, 40), Ok(1));
        assert_eq!(c.read(64, MODE_CACHED), Ok(64));
        assert_eq!(c.read(64, MODE_CACHED), Ok(64));
        assert_eq!(c.read(64, MODE_CACHED), Ok(64));
        assert_eq!(c.regens(), 1);
        assert_eq!(c.hits(), 2);
        assert_eq!(c.spent(), 42);
        assert_eq!(c.cached_ver(), 1);
        assert!(c.consistent());
    }

    #[test]
    fn fresh_always_regenerates() {
        let mut c = Consumer::with_budget(1000);
        assert_eq!(c.attach(1, 32, 10), Ok(1));
        assert_eq!(c.read(8, MODE_FRESH), Ok(8));
        assert_eq!(c.read(8, MODE_FRESH), Ok(8));
        assert_eq!(c.regens(), 2);
        assert_eq!(c.hits(), 0);
        assert_eq!(c.spent(), 20);
    }

    #[test]
    fn budget_failure_is_atomic() {
        let mut c = Consumer::with_budget(9);
        assert_eq!(c.attach(1, 32, 10), Ok(1));
        assert_eq!(c.read(8, MODE_CACHED), Err(EC_EBUDGET));
        assert_eq!(c.reads(), 0);
        assert_eq!(c.spent(), 0);
        assert_eq!(c.cached_ver(), 0);
        assert!(c.consistent());
    }

    #[test]
    fn require_needs_warm_cache() {
        let mut c = Consumer::with_budget(100);
        assert_eq!(c.attach(1, 16, 7), Ok(1));
        assert_eq!(c.read(4, MODE_REQUIRE), Err(EC_ENOCACHE));
        assert_eq!(c.read(4, MODE_CACHED), Ok(4));
        assert_eq!(c.read(4, MODE_REQUIRE), Ok(4));
        assert_eq!(c.hits(), 1);
        assert_eq!(c.regens(), 1);
    }

    #[test]
    fn version_rules() {
        let mut c = Consumer::new();
        assert_eq!(c.update(1), Err(EC_ENOV));
        assert_eq!(c.attach(3, 16, 2), Ok(1));
        assert_eq!(c.update(0), Err(EC_EINVAL));
        assert_eq!(c.update(3), Err(EC_ESEQ));
        assert_eq!(c.update(2), Err(EC_EORDER));
        assert_eq!(c.update(4), Ok(4));
        assert_eq!(c.ver(), 4);
    }

    #[test]
    fn overflow_is_explicit() {
        let mut c = Consumer::with_budget(u32::MAX);
        assert_eq!(c.attach(1, 1, u32::MAX), Ok(1));
        assert_eq!(c.read(1, MODE_CACHED), Ok(1));
        assert_eq!(c.spent(), u32::MAX);
        assert_eq!(c.update(2), Ok(2));
        assert_eq!(c.read(1, MODE_FRESH), Err(EC_EOVERFLOW));
        assert_eq!(c.reads(), 1);
    }

    #[test]
    fn short_reads_clamp_to_size() {
        let mut c = Consumer::with_budget(100);
        assert_eq!(c.attach(1, 10, 5), Ok(1));
        assert_eq!(c.read(4096, MODE_CACHED), Ok(10));
        assert_eq!(c.read(4096, MODE_CACHED), Ok(10));
        assert_eq!(c.bytes(), 20);
        assert!(c.consistent());
    }
}
