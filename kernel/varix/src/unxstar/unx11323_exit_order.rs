//! UNX-F11323 exit 遍历序（0xea30 段）
//!
//! `exit(3)` 的旧式实现是「一条单链表 + head 插头」：登记即插头，退出即从 head 逐个
//! 摘下来执行，所以自然得到 LIFO。真正棘手的是遍历过程中的再入与再登记：
//!
//!   1. 处理器里再登记一个处理器时，新结点挂在 head 上，**紧接着**就会被执行；
//!   2. 处理器里再调 `exit()` 是未定义行为，本模型把它钉成 `EX_EREENT` 并记入账；
//!   3. 自增殖的处理器（每次执行都把自己重新挂上）会无穷无尽，必须由执行计数闸门
//!      兜底，越过容量即 `EX_ELOOP`，而不是让内核真的转圈。
//!
//! 本模块用「索引链表 + 钩子」把这三件事建模成可审计状态机：`arm_hook` 声明
//! 「遇到哪个函数号就补登记几个、是否触发一次嵌套 run」，因此不需要闭包、不需要堆，
//! 纯 `no_std` 也能把 re-heading 的真实顺序跑出来。
//!
//! 闸门次序（`push`）是：函数号为零 → 已收尾 → 结点耗尽。运行闸门是：正在运行 →
//! 已收尾。次序固定，便于账本逐格复算。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea30;

/// 函数号为零。
pub const EX_EINVAL: u32 = SEG_BASE + 0x01;
/// 结点表耗尽。
pub const EX_EFULL: u32 = SEG_BASE + 0x02;
/// 已收尾。
pub const EX_EUSED: u32 = SEG_BASE + 0x03;
/// 重复收尾。
pub const EX_ERUN: u32 = SEG_BASE + 0x04;
/// 遍历中再入。
pub const EX_EREENT: u32 = SEG_BASE + 0x05;
/// 自增殖越过执行闸门。
pub const EX_ELOOP: u32 = SEG_BASE + 0x06;
/// 钩子参数越界。
pub const EX_EHOOK: u32 = SEG_BASE + 0x07;
/// 日志未开写。
pub const EX_EWARM: u32 = SEG_BASE + 0x08;
/// 日志下标越界。
pub const EX_ENOBS: u32 = SEG_BASE + 0x09;
/// 账本形状不符。
pub const EX_EBAL: u32 = SEG_BASE + 0x0a;
/// 槽位乱序。
pub const EX_EORDER: u32 = SEG_BASE + 0x0b;
/// 观测值与复算不符。
pub const EX_EMISMATCH: u32 = SEG_BASE + 0x0c;
/// 冻结后重放不一致。
pub const EX_ESEAL: u32 = SEG_BASE + 0x0d;

/// 结点容量。
pub const CAP: usize = 24;
/// 钩子最多补登记几个。
pub const MAX_HOOK: u8 = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 操作码：登记。
pub const OP_PUSH: u8 = 1;
/// 操作码：装钩子。
pub const OP_ARM: u8 = 2;
/// 操作码：遍历。
pub const OP_RUN: u8 = 3;
/// 操作码：读日志。
pub const OP_LOG: u8 = 4;

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

/// 链表结点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// 函数号。
    pub func: u32,
    /// 后继下标，-1 表示链尾。
    pub next: i32,
}

impl Node {
    /// 空结点。
    pub const fn blank() -> Node {
        Node { func: 0, next: -1 }
    }
}

/// exit 链表。
#[derive(Clone, Copy, Debug)]
pub struct ExitList {
    nodes: [Node; CAP],
    used: usize,
    free: [u8; CAP],
    nfree: usize,
    head: i32,
    count: usize,
    running: bool,
    ran: bool,
    execs: u32,
    log: [u32; CAP],
    log_len: usize,
    hook_watch: u32,
    hook_pushes: u8,
    hook_same: bool,
    hook_reenter: bool,
    nested_code: u32,
}

impl ExitList {
    /// 空链表。
    pub const fn new() -> ExitList {
        ExitList {
            nodes: [Node::blank(); CAP],
            used: 0,
            free: [0; CAP],
            nfree: 0,
            head: -1,
            count: 0,
            running: false,
            ran: false,
            execs: 0,
            log: [0; CAP],
            log_len: 0,
            hook_watch: 0,
            hook_pushes: 0,
            hook_same: false,
            hook_reenter: false,
            nested_code: 0,
        }
    }

    /// 在册条数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 已用结点数。
    pub fn used_nodes(&self) -> usize {
        self.used
    }

    /// 链头函数号。
    pub fn head_func(&self) -> Option<u32> {
        if self.head < 0 {
            return None;
        }
        let i = self.head as usize;
        self.nodes.get(i).map(|n| n.func)
    }

    /// 执行次数。
    pub fn execs(&self) -> u32 {
        self.execs
    }

    /// 日志长度。
    pub fn log_len(&self) -> usize {
        self.log_len
    }

    /// 是否正在遍历。
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// 是否已收尾。
    pub fn is_ran(&self) -> bool {
        self.ran
    }

    /// 嵌套 run 的返回码（0 表示没发生过）。
    pub fn nested_code(&self) -> u32 {
        self.nested_code
    }

    /// 日志第 i 位。
    pub fn log_at(&self, i: usize) -> Result<u32, u32> {
        if self.log_len == 0 {
            return Err(EX_EWARM);
        }
        if i >= self.log_len {
            return Err(EX_ENOBS);
        }
        match self.log.get(i) {
            Some(v) => Ok(*v),
            None => Err(EX_ENOBS),
        }
    }

    /// head 插入。返回在册条数。
    pub fn push(&mut self, func: u32) -> Result<usize, u32> {
        if func == 0 {
            return Err(EX_EINVAL);
        }
        if self.ran {
            return Err(EX_EUSED);
        }
        // 摘下来的结点先回收再用：自增殖处理器因此不会把结点表撑爆。
        let slot = if self.nfree > 0 {
            let k = self.nfree - 1;
            let s = match self.free.get(k) {
                Some(v) => *v as usize,
                None => return Err(EX_EBAL),
            };
            self.nfree = k;
            s
        } else {
            if self.used >= CAP {
                return Err(EX_EFULL);
            }
            let s = self.used;
            self.used += 1;
            s
        };
        match self.nodes.get_mut(slot) {
            Some(n) => {
                *n = Node { func, next: self.head };
            }
            None => return Err(EX_EFULL),
        }
        self.head = slot as i32;
        self.count += 1;
        Ok(self.count)
    }

    /// 装钩子：遇到 `watch` 就补登记 `pushes` 个函数、可选触发一次嵌套 run。
    ///
    /// `same` 决定补登记的函数号：置位则仍是 `watch` 本身（自增殖，必撞执行闸门），
    /// 否则抬到千位档（`watch + 1000*k`），钩子产物在日志里一眼可分。
    ///
    /// 返回补登记个数。闸门：`watch` 为零 → 补登记数超限。
    pub fn arm_hook(&mut self, watch: u32, pushes: u8, same: bool, reenter: bool) -> Result<usize, u32> {
        if watch == 0 {
            return Err(EX_EINVAL);
        }
        if pushes > MAX_HOOK {
            return Err(EX_EHOOK);
        }
        self.hook_watch = watch;
        self.hook_pushes = pushes;
        self.hook_same = same;
        self.hook_reenter = reenter;
        Ok(pushes as usize)
    }

    /// 遍历：从 head 摘下来执行，钩子补登记的结点插在 head 上。
    pub fn run(&mut self) -> Result<usize, u32> {
        if self.running {
            return Err(EX_EREENT);
        }
        if self.ran {
            return Err(EX_ERUN);
        }
        self.running = true;
        let mut ran = 0usize;
        loop {
            if self.head < 0 {
                break;
            }
            let i = self.head as usize;
            let node = match self.nodes.get(i) {
                Some(n) => *n,
                None => {
                    self.running = false;
                    return Err(EX_EBAL);
                }
            };
            self.head = node.next;
            self.count -= 1;
            // 回收刚摘下的结点（与表尾无关，索引链表允许空洞）。
            if self.nfree < CAP {
                if let Some(dst) = self.free.get_mut(self.nfree) {
                    *dst = i as u8;
                    self.nfree += 1;
                }
            }
            if let Some(dst) = self.log.get_mut(self.log_len) {
                *dst = node.func;
                self.log_len += 1;
            }
            self.execs += 1;
            ran += 1;
            if self.execs as usize > CAP {
                self.running = false;
                return Err(EX_ELOOP);
            }
            if node.func == self.hook_watch {
                if self.hook_reenter && self.nested_code == 0 {
                    self.nested_code = match self.run() {
                        Err(c) => c,
                        Ok(_) => 0,
                    };
                }
                let mut k = 0u8;
                while k < self.hook_pushes {
                    // 补登记插在 head 上，因此下一个跑的就是它。
                    let f = if self.hook_same {
                        node.func
                    } else {
                        node.func + 1000 * (k as u32 + 1)
                    };
                    if self.push(f).is_err() {
                        break;
                    }
                    k += 1;
                }
            }
        }
        self.running = false;
        self.ran = true;
        Ok(ran)
    }

    /// 指纹：结点表、链头、计数、日志、钩子与嵌套码一并折入。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.used as u64);
        let mut i = 0usize;
        while i < self.used {
            if let Some(n) = self.nodes.get(i) {
                h = fold(h, n.func as u64);
                h = fold(h, n.next as u32 as u64);
            }
            i += 1;
        }
        h = fold(h, self.head as u32 as u64);
        h = fold(h, self.count as u64);
        h = fold(h, self.nfree as u64);
        h = fold(h, self.execs as u64);
        h = fold(h, self.log_len as u64);
        let mut j = 0usize;
        while j < self.log_len {
            if let Some(v) = self.log.get(j) {
                h = fold(h, *v as u64);
            }
            j += 1;
        }
        h = fold(h, self.hook_watch as u64);
        h = fold(h, self.hook_pushes as u64);
        h = fold(h, self.hook_same as u64);
        h = fold(h, self.hook_reenter as u64);
        fold(h, self.nested_code as u64)
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

/// 把 `Result<usize, u32>` 落成样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
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
            self.rejects += 1;
            return Err(EX_EORDER);
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
            return Err(EX_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 5, 3, 2003, 1];
        const CODES: [u32; 6] = [EX_EINVAL, EX_EUSED, EX_ERUN, EX_EHOOK, EX_EFULL, EX_ELOOP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EX_EBAL),
            };
            if s.slot as usize != i {
                return Err(EX_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EX_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EX_EMISMATCH);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EX_EMISMATCH);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EX_EBAL);
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
            return Err(EX_EBAL);
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
            Err(EX_ESEAL)
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

/// 十四格账本：三条 LIFO + 钩子补登记 + 收尾后拒收。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut e = ExitList::new();

    let _ = l.register(cell(0, OP_PUSH, e.push(1)));
    let _ = l.register(cell(1, OP_PUSH, e.push(2)));
    let _ = l.register(cell(2, OP_PUSH, e.push(3)));
    let _ = l.register(cell(3, OP_ARM, e.arm_hook(3, 2, false, false)));
    let _ = l.register(cell(4, OP_RUN, e.run()));

    let first = match e.log_at(0) {
        Ok(v) => Sample { slot: 5, op: OP_LOG, ok: true, code: 0, value: v },
        Err(c) => Sample { slot: 5, op: OP_LOG, ok: false, code: c, value: 0 },
    };
    let _ = l.register(first);
    let second = match e.log_at(1) {
        Ok(v) => Sample { slot: 6, op: OP_LOG, ok: true, code: 0, value: v },
        Err(c) => Sample { slot: 6, op: OP_LOG, ok: false, code: c, value: 0 },
    };
    let _ = l.register(second);
    let last = match e.log_at(4) {
        Ok(v) => Sample { slot: 7, op: OP_LOG, ok: true, code: 0, value: v },
        Err(c) => Sample { slot: 7, op: OP_LOG, ok: false, code: c, value: 0 },
    };
    let _ = l.register(last);

    let _ = l.register(cell(8, OP_PUSH, e.push(0)));
    let _ = l.register(cell(9, OP_PUSH, e.push(9)));
    let _ = l.register(cell(10, OP_RUN, e.run()));
    let _ = l.register(cell(11, OP_ARM, e.arm_hook(1, MAX_HOOK + 1, false, false)));

    let mut full = ExitList::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = full.push(0x100 + i);
        i += 1;
    }
    let _ = l.register(cell(12, OP_PUSH, full.push(0x200)));

    // 第 13 格：自增殖钩子撞上执行闸门。
    let mut loopee = ExitList::new();
    let _ = loopee.arm_hook(5, 1, true, false);
    let _ = loopee.push(5);
    let _ = l.register(cell(13, OP_RUN, loopee.run()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：纯 LIFO（无钩子）。
    let mut a = ExitList::new();
    let _ = a.push(1);
    let _ = a.push(2);
    let _ = a.push(3);
    let n = a.run();
    out[1] = n == Ok(3)
        && a.log_at(0) == Ok(3)
        && a.log_at(1) == Ok(2)
        && a.log_at(2) == Ok(1)
        && a.count() == 0
        && a.head_func().is_none();

    // 二路：钩子补登记的结点立刻插在 head 上，先于旧结点执行。
    let mut b = ExitList::new();
    let _ = b.push(10);
    let _ = b.push(20);
    let _ = b.arm_hook(20, 2, false, false);
    let m = b.run();
    out[2] = m == Ok(4)
        && b.log_at(0) == Ok(20)
        && b.log_at(1) == Ok(2020)
        && b.log_at(2) == Ok(1020)
        && b.log_at(3) == Ok(10);

    // 三路：再入闸门与日志越界。
    let mut g = ExitList::new();
    let _ = g.push(7);
    let _ = g.arm_hook(7, 0, false, true);
    let r = g.run();
    let warm = {
        let w = ExitList::new();
        w.log_at(0) == Err(EX_EWARM)
    };
    out[3] = r == Ok(1)
        && g.nested_code() == EX_EREENT
        && g.log_at(1) == Err(EX_ENOBS)
        && warm
        && g.run() == Err(EX_ERUN)
        && g.push(8) == Err(EX_EUSED);

    out
}

/// 十三项判据。
pub fn run_unx11323_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11323.exit_order");
    let l = build_ledger();

    s.add_dyn(
        "EX1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "EX2-插头计数递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "EX3-钩子装载返回补登记数",
        l.sample(3).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "EX4-钩子使执行数超出在册数",
        l.sample(4).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "EX5-钩子结点先跑",
        l.sample(5).map(|x| x.value) == Some(3)
            && l.sample(6).map(|x| x.value) == Some(2003),
        "static",
    );

    s.add(
        "EX6-最后执行最老结点",
        l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "EX7-零号函数拒绝",
        l.sample(8).map(|x| x.code) == Some(EX_EINVAL),
        "static",
    );

    s.add(
        "EX8-收尾后登记与再收尾",
        l.sample(9).map(|x| x.code) == Some(EX_EUSED)
            && l.sample(10).map(|x| x.code) == Some(EX_ERUN),
        "static",
    );

    s.add(
        "EX9-钩子参数越界",
        l.sample(11).map(|x| x.code) == Some(EX_EHOOK),
        "static",
    );

    s.add(
        "EX10-结点耗尽与自增殖闸门",
        l.sample(12).map(|x| x.code) == Some(EX_EFULL)
            && l.sample(13).map(|x| x.code) == Some(EX_ELOOP),
        "static",
    );

    s.add(
        "EX11-三节点纯 LIFO",
        {
            let mut a = ExitList::new();
            let _ = a.push(4);
            let _ = a.push(5);
            let _ = a.push(6);
            let n = a.run();
            n == Ok(3)
                && a.log_at(0) == Ok(6)
                && a.log_at(2) == Ok(4)
                && a.used_nodes() == 3
        },
        "static",
    );

    s.add(
        "EX12-钩子补登记仍守 LIFO",
        {
            let mut a = ExitList::new();
            let _ = a.push(1);
            let _ = a.push(2);
            let _ = a.arm_hook(2, 1, false, false);
            let n = a.run();
            n == Ok(3) && a.log_at(0) == Ok(2) && a.log_at(1) == Ok(1002) && a.log_at(2) == Ok(1)
        },
        "static",
    );

    s.add(
        "EX13-冻结重放四注入",
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
        let s = run_unx11323_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn head_insertion_is_lifo() {
        let mut a = ExitList::new();
        assert_eq!(a.push(1), Ok(1));
        assert_eq!(a.push(2), Ok(2));
        assert_eq!(a.head_func(), Some(2));
        assert_eq!(a.run(), Ok(2));
        assert_eq!(a.log_at(0), Ok(2));
        assert_eq!(a.log_at(1), Ok(1));
    }

    #[test]
    fn hook_reheads_immediately() {
        let mut a = ExitList::new();
        let _ = a.push(10);
        let _ = a.push(20);
        let _ = a.arm_hook(20, 1, false, false);
        assert_eq!(a.run(), Ok(3));
        assert_eq!(a.log_at(0), Ok(20));
        assert_eq!(a.log_at(1), Ok(1020));
        assert_eq!(a.log_at(2), Ok(10));
    }

    #[test]
    fn reentry_is_flagged() {
        let mut a = ExitList::new();
        let _ = a.push(7);
        let _ = a.arm_hook(7, 0, false, true);
        assert_eq!(a.run(), Ok(1));
        assert_eq!(a.nested_code(), EX_EREENT);
        assert_eq!(a.run(), Err(EX_ERUN));
    }

    #[test]
    fn self_arming_is_bounded() {
        let mut a = ExitList::new();
        let _ = a.arm_hook(5, 1, true, false);
        let _ = a.push(5);
        assert_eq!(a.run(), Err(EX_ELOOP));
        assert_eq!(a.execs() as usize, CAP + 1);
    }

    #[test]
    fn gates_are_ordered() {
        let mut a = ExitList::new();
        assert_eq!(a.push(0), Err(EX_EINVAL));
        assert_eq!(a.arm_hook(0, 1, false, false), Err(EX_EINVAL));
        assert_eq!(a.arm_hook(1, MAX_HOOK + 1, false, false), Err(EX_EHOOK));
        assert_eq!(a.arm_hook(1, 0, false, false), Ok(0));
        assert_eq!(a.run(), Ok(0));
        assert_eq!(a.push(1), Err(EX_EUSED));
        assert_eq!(a.run(), Err(EX_ERUN));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
