//! UNX-F11335 quick_exit 与 at_quick_exit（0xeaf0 段）
//!
//! `quick_exit(3)` 是 C11 给「进程已经没救、但还想按约定收尾」留的窄门：
//! 它只跑 `at_quick_exit` 登记的处理器，**不跑** `atexit` 链，**不冲刷**
//! stdio，也不做静态析构。正因为省略了这些，它才敢在信号处理上下文里被
//! 调用。实现上最容易犯的错，是把它做成 `exit` 的别名——那样一旦有人在
//! quick 路径上依赖「stdio 已刷」，就会静默丢日志。
//!
//! 本模块因此维护两条**彼此隔离**的执行链：
//!
//! - 普通链（`CH_NORMAL`）：只有普通退出会跑，跑完把 `flushed` 置真；
//! - 快速链（`CH_QUICK`）：只有快速退出会跑，跑完 `flushed` 保持假。
//!
//! 两条链共享一套容量与重号规则，但执行次序各自逆序（后登记先执行），执行
//! 记录进同一个 `order` 数组——这样「这次退出到底跑了哪些 handler、按什么
//! 次序」是一份可复盘的证据，而不是两句布尔。
//!
//! 终局是**单向**的：任意一条链跑过，两条链的登记口一起关闭（`ECLOSED`），
//! 并且另一条链的执行请求被判 `EORDER`——这不是「重复执行」，而是「走错
//! 了终结路径」。区分这两者很重要：前者是同一路径的第二次调用，后者是
//! 两条互斥路径被混用，修复手段完全不同。
//!
//! 闸门次序（`register`）：链号非法 → 处理器号为零 → 已有链跑过 → 同链重号
//! → 该链已满。`run`：链号非法 → 本链已跑（`ERUN`）→ 别链已跑（`EORDER`）。
//! 注意 `ERUN` 排在 `EORDER` 前：同一条链的第二次调用永远是 `ERUN`，哪怕
//! 此时另一条链也跑过，诊断仍指向「你自己重复了」。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeaf0;

/// 处理器号为零。
pub const QC_EID: u32 = SEG_BASE + 0x01;
/// 链号非法。
pub const QC_ECHAIN: u32 = SEG_BASE + 0x02;
/// 登记口已关闭。
pub const QC_ECLOSED: u32 = SEG_BASE + 0x03;
/// 同链重号。
pub const QC_EDUP: u32 = SEG_BASE + 0x04;
/// 链已满。
pub const QC_EFULL: u32 = SEG_BASE + 0x05;
/// 本链已执行。
pub const QC_ERUN: u32 = SEG_BASE + 0x06;
/// 终结路径次序错误。
pub const QC_EORDER: u32 = SEG_BASE + 0x07;
/// 执行记录下标越界。
pub const QC_ENOBS: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const QC_EBAL: u32 = SEG_BASE + 0x09;
/// 账本槽位乱序。
pub const QC_EORDL: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const QC_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const QC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 查询尚无可观测结果。
pub const QC_EWARM: u32 = SEG_BASE + 0x0d;

/// 单链容量。
pub const CAP: usize = 5;
/// 执行记录容量（两条链合计）。
pub const LOG_CAP: usize = CAP * 2;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 普通退出链。
pub const CH_NORMAL: u8 = 1;
/// 快速退出链。
pub const CH_QUICK: u8 = 2;

/// 操作码：登记。
pub const OP_REG: u8 = 1;
/// 操作码：执行。
pub const OP_RUN: u8 = 2;
/// 操作码：读链长。
pub const OP_LEN: u8 = 3;
/// 操作码：读执行记录。
pub const OP_ORDER: u8 = 4;

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

/// 两条退出链。
#[derive(Clone, Copy, Debug)]
pub struct Chains {
    normal: [u32; CAP],
    quick: [u32; CAP],
    nn: usize,
    qn: usize,
    nran: bool,
    qran: bool,
    flushed: bool,
    order: [u32; LOG_CAP],
    on: usize,
}

impl Chains {
    /// 空链组。
    pub const fn new() -> Chains {
        Chains {
            normal: [0; CAP],
            quick: [0; CAP],
            nn: 0,
            qn: 0,
            nran: false,
            qran: false,
            flushed: false,
            order: [0; LOG_CAP],
            on: 0,
        }
    }

    /// 某条链是否跑过。
    pub fn has_run(&self, chain: u8) -> bool {
        if chain == CH_NORMAL {
            self.nran
        } else if chain == CH_QUICK {
            self.qran
        } else {
            false
        }
    }

    /// 任意链跑过即关闭登记口。
    pub fn is_closed(&self) -> bool {
        self.nran || self.qran
    }

    /// stdio 是否被冲刷。
    pub fn is_flushed(&self) -> bool {
        self.flushed
    }

    /// 链长。
    pub fn chain_len(&self, chain: u8) -> Result<usize, u32> {
        if chain == CH_NORMAL {
            Ok(self.nn)
        } else if chain == CH_QUICK {
            Ok(self.qn)
        } else {
            Err(QC_ECHAIN)
        }
    }

    /// 第 i 个普通链处理器号。
    pub fn normal_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.nn {
            return Err(QC_ENOBS);
        }
        match self.normal.get(i) {
            Some(v) => Ok(*v),
            None => Err(QC_ENOBS),
        }
    }

    /// 第 i 个快速链处理器号。
    pub fn quick_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.qn {
            return Err(QC_ENOBS);
        }
        match self.quick.get(i) {
            Some(v) => Ok(*v),
            None => Err(QC_ENOBS),
        }
    }

    /// 第 i 条执行记录。
    pub fn order_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.on {
            return Err(QC_ENOBS);
        }
        match self.order.get(i) {
            Some(v) => Ok(*v),
            None => Err(QC_ENOBS),
        }
    }

    /// 执行记录长度。
    pub fn executed(&self) -> usize {
        self.on
    }

    /// 同链查号。
    fn dup_in(&self, chain: u8, id: u32) -> bool {
        let mut i = 0usize;
        if chain == CH_NORMAL {
            while i < self.nn {
                if let Some(v) = self.normal.get(i) {
                    if *v == id {
                        return true;
                    }
                }
                i += 1;
            }
        } else if chain == CH_QUICK {
            while i < self.qn {
                if let Some(v) = self.quick.get(i) {
                    if *v == id {
                        return true;
                    }
                }
                i += 1;
            }
        }
        false
    }

    /// 登记一个处理器，成功返回该链长度。
    ///
    /// 两条链的重号判定彼此独立：同一个号可以同时挂在两条链上，因为两条
    /// 链是两套互斥的收尾协议，不是同一张表的两段。
    pub fn register(&mut self, chain: u8, id: u32) -> Result<usize, u32> {
        if chain != CH_NORMAL && chain != CH_QUICK {
            return Err(QC_ECHAIN);
        }
        if id == 0 {
            return Err(QC_EID);
        }
        if self.is_closed() {
            return Err(QC_ECLOSED);
        }
        if self.dup_in(chain, id) {
            return Err(QC_EDUP);
        }
        if chain == CH_NORMAL {
            if self.nn >= CAP {
                return Err(QC_EFULL);
            }
            if let Some(slot) = self.normal.get_mut(self.nn) {
                *slot = id;
            }
            self.nn += 1;
            Ok(self.nn)
        } else {
            if self.qn >= CAP {
                return Err(QC_EFULL);
            }
            if let Some(slot) = self.quick.get_mut(self.qn) {
                *slot = id;
            }
            self.qn += 1;
            Ok(self.qn)
        }
    }

    /// 执行一条链，返回实际执行的处理器数。
    ///
    /// 执行顺序是登记的逆序；记录写进共享 `order`。普通链执行时把
    /// `flushed` 置真，快速链保持原值（构造时恒假，且绝不主动置真）。
    pub fn run(&mut self, chain: u8) -> Result<usize, u32> {
        if chain != CH_NORMAL && chain != CH_QUICK {
            return Err(QC_ECHAIN);
        }
        if chain == CH_NORMAL {
            if self.nran {
                return Err(QC_ERUN);
            }
            if self.qran {
                return Err(QC_EORDER);
            }
            let mut i = self.nn;
            while i > 0 {
                i -= 1;
                let v = match self.normal.get(i) {
                    Some(x) => *x,
                    None => 0,
                };
                if self.on < LOG_CAP {
                    if let Some(dst) = self.order.get_mut(self.on) {
                        *dst = v;
                    }
                    self.on += 1;
                }
            }
            self.nran = true;
            self.flushed = true;
            Ok(self.nn)
        } else {
            if self.qran {
                return Err(QC_ERUN);
            }
            if self.nran {
                return Err(QC_EORDER);
            }
            let mut i = self.qn;
            while i > 0 {
                i -= 1;
                let v = match self.quick.get(i) {
                    Some(x) => *x,
                    None => 0,
                };
                if self.on < LOG_CAP {
                    if let Some(dst) = self.order.get_mut(self.on) {
                        *dst = v;
                    }
                    self.on += 1;
                }
            }
            self.qran = true;
            Ok(self.qn)
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
            return Err(QC_EORDL);
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
            return Err(QC_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 2, 2, 2, 22, 21];
        const CODES: [u32; 6] = [
            QC_EDUP,
            QC_EID,
            QC_EORDER,
            QC_ECLOSED,
            QC_ERUN,
            QC_ECHAIN,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(QC_EBAL),
            };
            if s.slot as usize != i {
                return Err(QC_EORDL);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(QC_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(QC_EMISMATCH);
                        }
                    }
                    None => return Err(QC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(QC_EMISMATCH);
                        }
                    }
                    None => return Err(QC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(QC_EBAL);
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
            return Err(QC_EBAL);
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
            Err(QC_ESEAL)
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

/// 十四格账本：两链登记 → 重号 → 快速链执行 → 隔离观测 → 终局拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Chains::new();

    let _ = l.register(cell(0, OP_REG, c.register(CH_NORMAL, 7)));
    let _ = l.register(cell(1, OP_REG, c.register(CH_NORMAL, 7)));
    let _ = l.register(cell(2, OP_REG, c.register(CH_NORMAL, 8)));
    let _ = l.register(cell(3, OP_REG, c.register(CH_QUICK, 21)));
    let _ = l.register(cell(4, OP_REG, c.register(CH_QUICK, 22)));
    let _ = l.register(cell(5, OP_REG, c.register(CH_QUICK, 0)));
    let _ = l.register(cell(6, OP_RUN, c.run(CH_QUICK)));
    let _ = l.register(cell(7, OP_LEN, c.chain_len(CH_NORMAL)));
    let _ = l.register(word_cell(8, OP_ORDER, c.order_at(0)));
    let _ = l.register(word_cell(9, OP_ORDER, c.order_at(1)));
    let _ = l.register(cell(10, OP_RUN, c.run(CH_NORMAL)));
    let _ = l.register(cell(11, OP_REG, c.register(CH_NORMAL, 9)));
    let _ = l.register(cell(12, OP_RUN, c.run(CH_QUICK)));
    let _ = l.register(cell(13, OP_REG, c.register(3, 9)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：快速链只跑自己，不碰普通链、不冲刷。
    let mut a = Chains::new();
    let _ = a.register(CH_NORMAL, 1);
    let _ = a.register(CH_NORMAL, 2);
    let _ = a.register(CH_QUICK, 3);
    let quick = a.run(CH_QUICK);
    out[1] = quick == Ok(1)
        && a.has_run(CH_QUICK)
        && !a.has_run(CH_NORMAL)
        && !a.is_flushed()
        && a.chain_len(CH_NORMAL) == Ok(2)
        && a.order_at(0) == Ok(3)
        && a.order_at(1) == Err(QC_ENOBS)
        && a.register(CH_NORMAL, 4) == Err(QC_ECLOSED)
        && a.register(CH_NORMAL, 1) == Err(QC_ECLOSED)
        && a.run(CH_NORMAL) == Err(QC_EORDER);

    // 二路：普通链逆序执行并冲刷。
    let mut b = Chains::new();
    let _ = b.register(CH_NORMAL, 1);
    let _ = b.register(CH_NORMAL, 2);
    let _ = b.register(CH_NORMAL, 3);
    let _ = b.register(CH_QUICK, 4);
    let normal = b.run(CH_NORMAL);
    out[2] = normal == Ok(3)
        && b.is_flushed()
        && b.has_run(CH_NORMAL)
        && !b.has_run(CH_QUICK)
        && b.order_at(0) == Ok(3)
        && b.order_at(1) == Ok(2)
        && b.order_at(2) == Ok(1)
        && b.order_at(3) == Err(QC_ENOBS)
        && b.chain_len(CH_QUICK) == Ok(1)
        && b.run(CH_QUICK) == Err(QC_EORDER)
        && b.run(CH_NORMAL) == Err(QC_ERUN);

    // 三路：闸门（链号 / 零号 / 重号 / 满 / 执行链号）。
    let mut c = Chains::new();
    let g1 = c.register(0, 1) == Err(QC_ECHAIN);
    let g2 = c.register(3, 1) == Err(QC_ECHAIN);
    let g3 = c.register(CH_QUICK, 0) == Err(QC_EID);
    let g4 = c.register(CH_QUICK, 5) == Ok(1);
    let g5 = c.register(CH_QUICK, 5) == Err(QC_EDUP);
    let g6 = c.register(CH_NORMAL, 5) == Ok(1);
    let mut i = 2u32;
    while (i as usize) <= CAP + 1 {
        let _ = c.register(CH_QUICK, i);
        i += 1;
    }
    let g7 = c.register(CH_QUICK, 0x30) == Err(QC_EFULL);
    let g8 = c.run(9) == Err(QC_ECHAIN);
    let g9 = c.chain_len(9) == Err(QC_ECHAIN);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9;

    out
}

/// 十三项判据。
pub fn run_unx11335_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11335.quick_exit_chain");
    let l = build_ledger();

    s.add_dyn(
        "QC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "QC2-两条链各自计数",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(2).map(|x| x.value) == Some(2)
            && l.sample(3).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "QC3-同链重号被拒",
        l.sample(1).map(|x| x.code) == Some(QC_EDUP),
        "static",
    );

    s.add(
        "QC4-零号处理器被拒",
        l.sample(5).map(|x| x.code) == Some(QC_EID),
        "static",
    );

    s.add(
        "QC5-快速链逆序执行",
        l.sample(6).map(|x| x.value) == Some(2)
            && l.sample(8).map(|x| x.value) == Some(22)
            && l.sample(9).map(|x| x.value) == Some(21),
        "static",
    );

    s.add(
        "QC6-快速链不动普通链",
        l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "QC7-两链登记口同时关闭",
        l.sample(11).map(|x| x.code) == Some(QC_ECLOSED),
        "static",
    );

    s.add(
        "QC8-快速线后普通退出被拒",
        l.sample(10).map(|x| x.code) == Some(QC_EORDER),
        "static",
    );

    s.add(
        "QC9-快速链二次执行被拒",
        l.sample(12).map(|x| x.code) == Some(QC_ERUN),
        "static",
    );

    s.add(
        "QC10-非法链号被拒",
        l.sample(13).map(|x| x.code) == Some(QC_ECHAIN),
        "static",
    );

    s.add(
        "QC11-快速退出不冲刷",
        {
            let mut c = Chains::new();
            let _ = c.register(CH_QUICK, 4);
            let _ = c.register(CH_QUICK, 5);
            let r = c.run(CH_QUICK);
            r == Ok(2)
                && !c.is_flushed()
                && c.order_at(0) == Ok(5)
                && c.order_at(1) == Ok(4)
                && c.register(CH_QUICK, 6) == Err(QC_ECLOSED)
                && c.register(CH_QUICK, 4) == Err(QC_ECLOSED)
        },
        "static",
    );

    s.add(
        "QC12-普通退出冲刷且逆序",
        {
            let mut c = Chains::new();
            let _ = c.register(CH_NORMAL, 11);
            let _ = c.register(CH_QUICK, 12);
            let _ = c.register(CH_NORMAL, 13);
            let r = c.run(CH_NORMAL);
            r == Ok(2)
                && c.is_flushed()
                && c.order_at(0) == Ok(13)
                && c.order_at(1) == Ok(11)
                && c.run(CH_QUICK) == Err(QC_EORDER)
                && c.quick_at(0) == Ok(12)
                && c.normal_at(2) == Err(QC_ENOBS)
        },
        "static",
    );

    s.add(
        "QC13-冻结重放四注入",
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
        let s = run_unx11335_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn quick_line_isolated() {
        let mut c = Chains::new();
        let _ = c.register(CH_NORMAL, 1);
        let _ = c.register(CH_QUICK, 2);
        assert_eq!(c.run(CH_QUICK), Ok(1));
        assert!(!c.is_flushed());
        assert_eq!(c.chain_len(CH_NORMAL), Ok(1));
        assert_eq!(c.run(CH_NORMAL), Err(QC_EORDER));
        assert_eq!(c.run(CH_QUICK), Err(QC_ERUN));
    }

    #[test]
    fn normal_line_flushes() {
        let mut c = Chains::new();
        let _ = c.register(CH_NORMAL, 1);
        let _ = c.register(CH_NORMAL, 2);
        assert_eq!(c.run(CH_NORMAL), Ok(2));
        assert!(c.is_flushed());
        assert_eq!(c.order_at(0), Ok(2));
        assert_eq!(c.order_at(1), Ok(1));
        assert_eq!(c.register(CH_QUICK, 3), Err(QC_ECLOSED));
    }

    #[test]
    fn chain_gates() {
        let mut c = Chains::new();
        assert_eq!(c.register(0, 1), Err(QC_ECHAIN));
        assert_eq!(c.register(CH_NORMAL, 0), Err(QC_EID));
        assert_eq!(c.register(CH_NORMAL, 7), Ok(1));
        assert_eq!(c.register(CH_NORMAL, 7), Err(QC_EDUP));
        assert_eq!(c.register(CH_QUICK, 7), Ok(1));
        assert_eq!(c.chain_len(9), Err(QC_ECHAIN));
        assert_eq!(c.run(9), Err(QC_ECHAIN));
    }

    #[test]
    fn capacity_is_per_chain() {
        let mut c = Chains::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(c.register(CH_QUICK, i).is_ok());
            i += 1;
        }
        assert_eq!(c.register(CH_QUICK, 100), Err(QC_EFULL));
        assert_eq!(c.register(CH_NORMAL, 100), Ok(1));
        assert_eq!(c.chain_len(CH_QUICK), Ok(CAP));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn inject_paths_agree() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
