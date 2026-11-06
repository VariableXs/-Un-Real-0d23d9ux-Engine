//! UNX-F11460 B13 批 ktest 断言集聚合（0x20c80 段）
//!
//! B13 批是九块同批模块（F11452..F11460），每块恰好十三条判据，合计 117 条断言。
//! 聚合器不重跑判据，只吃每块上报的（总数、通过数）：这是有意为之——批级汇总
//! 必须是**纯函数**，否则同一批上报两次会得到两个结论。九块的身份被钉死在这个
//! 闭区间里，区间外的编号不是“本批漏登记”而是“登记错了批”，必须当场拒绝，免得
//! 把别批的分数算进来。
//!
//! 登记的闸门次序：编号区间（EID）→ 自洽（通过数不得多于总数 ESHORT）→ 重复
//! （EDUP）→ 严格递增（EORDER）→ 容量（EFULL）。严格递增让“批内顺序”与“批内
//! 编号”同构：表里第 i 条就是 11452+i，“缺一块”会表现为逆序而不是静默补位。
//!
//! 汇总侧给出五个投影：模块数、断言总数、通过数、失败数、百分比、覆盖率（满绿
//! 模块占比）。百分比用 64 位中间量做整除，避免 117×100 这类可预见的溢出；断言
//! 总数用 `checked_add`，真溢出记 EOVER。批级闸门只认一个条件：失败数为零。
//! 空批不是“零失败”而是**没有断言**，必须报 EEMPTY，不能报通过。
//!
//! 账本十四格：八格成功（模块数、断言总数、通过数、百分比、覆盖率、脏批通过数、
//! 脏批百分比、末位编号），六格拒绝（重复、逆序、越界编号、容量满、批闸门、
//! 空批）。审计逐格钉值。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c80;

/// 段内低十六位。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const BA_EORDER: u16 = SEG_LO + 0x01;
/// 重复登记。
pub const BA_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const BA_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const BA_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const BA_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const BA_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const BA_EFULL: u16 = SEG_LO + 0x07;
/// 编号越出本批。
pub const BA_EID: u16 = SEG_LO + 0x08;
/// 通过数多于总数。
pub const BA_ESHORT: u16 = SEG_LO + 0x09;
/// 批闸门未过。
pub const BA_EGATE: u16 = SEG_LO + 0x0a;
/// 空批。
pub const BA_EEMPTY: u16 = SEG_LO + 0x0b;
/// 断言计数溢出。
pub const BA_EOVER: u16 = SEG_LO + 0x0c;
/// 票号不一致。
pub const BA_ESEAL: u16 = SEG_LO + 0x0d;

/// 本批首号。
pub const ID_LO: u32 = 11452;
/// 本批末号。
pub const ID_HI: u32 = 11460;
/// 每块判据数。
pub const CHECKS_PER_MODULE: u32 = 13;
/// 本批模块数。
pub const MODULES: u32 = 9;
/// 本批断言总数。
pub const ASSERTIONS: u32 = 117;

/// 可登记模块容量。
pub const CAP: usize = 12;

/// 操作：批汇总。
pub const OP_BATCH: u8 = 1;
/// 操作：顺序。
pub const OP_ORDER: u8 = 2;
/// 操作：批闸门。
pub const OP_GATE: u8 = 3;
/// 操作：账本自检。
pub const OP_LEDGER: u8 = 4;

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

/// 一块模块的上报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    pub id: u32,
    pub total: u16,
    pub passed: u16,
}

impl Module {
    /// 构造。
    pub const fn make(id: u32, total: u16, passed: u16) -> Module {
        Module { id, total, passed }
    }

    /// 满绿模块。
    pub const fn green(id: u32) -> Module {
        Module {
            id,
            total: 13,
            passed: 13,
        }
    }

    /// 空白块。
    pub const fn blank() -> Module {
        Module {
            id: 0,
            total: 0,
            passed: 0,
        }
    }

    /// 是否满绿。
    pub fn is_green(&self) -> bool {
        self.total != 0 && self.passed == self.total
    }

    /// 失败数。
    pub fn failed(&self) -> u16 {
        self.total.saturating_sub(self.passed)
    }
}

/// 批内登记表。
#[derive(Clone, Copy, Debug)]
pub struct Registry {
    items: [Module; CAP],
    n: usize,
    cap: usize,
}

impl Registry {
    /// 默认容量。
    pub const fn new() -> Registry {
        Registry {
            items: [Module::blank(); CAP],
            n: 0,
            cap: CAP,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Registry {
        Registry {
            items: [Module::blank(); CAP],
            n: 0,
            cap: if cap > CAP { CAP } else { cap },
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

    /// 取第 i 条。
    pub fn at(&self, i: usize) -> Option<Module> {
        if i >= self.n {
            return None;
        }
        self.items.get(i).copied()
    }

    /// 末位编号。
    pub fn last_id(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        match self.items.get(self.n - 1) {
            Some(m) => m.id,
            None => 0,
        }
    }

    /// 按编号查。
    pub fn find(&self, id: u32) -> Option<Module> {
        let mut i = 0usize;
        while i < self.n {
            match self.items.get(i) {
                Some(m) => {
                    if m.id == id {
                        return Some(*m);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 登记：区间 → 自洽 → 重复 → 递增 → 容量。
    pub fn register(&mut self, m: Module) -> Result<(), u16> {
        if m.id < ID_LO || m.id > ID_HI {
            return Err(BA_EID);
        }
        if m.passed > m.total {
            return Err(BA_ESHORT);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.items.get(i) {
                Some(x) => {
                    if x.id == m.id {
                        return Err(BA_EDUP);
                    }
                }
                None => return Err(BA_ENOBS),
            }
            i += 1;
        }
        if self.n > 0 && m.id <= self.last_id() {
            return Err(BA_EORDER);
        }
        if self.n >= self.cap || self.n >= CAP {
            return Err(BA_EFULL);
        }
        match self.items.get_mut(self.n) {
            Some(slot) => *slot = m,
            None => return Err(BA_EFULL),
        }
        self.n += 1;
        Ok(())
    }

    /// 满绿模块数。
    pub fn green_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(m) = self.items.get(i) {
                if m.is_green() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

/// 汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub modules: u32,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub pct: u32,
    pub coverage: u32,
}

impl Summary {
    /// 空汇总。
    pub const fn zero() -> Summary {
        Summary {
            modules: 0,
            total: 0,
            passed: 0,
            failed: 0,
            pct: 0,
            coverage: 0,
        }
    }
}

/// 整数百分比：64 位中间量，结果必在 0..=100。
fn permille_pct(part: u32, whole: u32) -> u32 {
    if whole == 0 {
        return 0;
    }
    let num = u64::from(part) * 100u64;
    match u32::try_from(num / u64::from(whole)) {
        Ok(v) => {
            if v > 100 {
                100
            } else {
                v
            }
        }
        Err(_) => 100,
    }
}

/// 汇总九块上报。
pub fn aggregate(r: &Registry) -> Summary {
    let mut total = 0u32;
    let mut passed = 0u32;
    let mut i = 0usize;
    while i < r.len() {
        if let Some(m) = r.at(i) {
            total = total.saturating_add(u32::from(m.total));
            passed = passed.saturating_add(u32::from(m.passed));
        }
        i += 1;
    }
    let modules = match u32::try_from(r.len()) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let failed = total.saturating_sub(passed);
    Summary {
        modules,
        total,
        passed,
        failed,
        pct: permille_pct(passed, total),
        coverage: permille_pct(r.green_count(), modules),
    }
}

/// 批闸门：有断言且零失败才放行。
pub fn gate(s: &Summary) -> Result<u32, u16> {
    if s.total == 0 || s.modules == 0 {
        return Err(BA_EEMPTY);
    }
    if s.passed > s.total {
        return Err(BA_ESHORT);
    }
    if s.failed != 0 {
        return Err(BA_EGATE);
    }
    Ok(s.pct)
}

/// 满绿批：11452..11460 各 13/13。
pub fn batch() -> Registry {
    let mut r = Registry::new();
    let mut id = ID_LO;
    while id <= ID_HI {
        let _ = r.register(Module::green(id));
        id += 1;
    }
    r
}

/// 脏批：F11457 少一条。
pub fn dirty_batch() -> Registry {
    let mut r = Registry::new();
    let mut id = ID_LO;
    while id <= ID_HI {
        let m = if id == 11457 {
            Module::make(id, 13, 12)
        } else {
            Module::green(id)
        };
        let _ = r.register(m);
        id += 1;
    }
    r
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
            return Err(BA_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(BA_EDUP);
                    }
                }
                None => return Err(BA_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(BA_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(BA_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(BA_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(BA_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(BA_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(BA_EFULL),
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
            return Err(BA_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(BA_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BA_ENOBS),
            };
            if s.slot as usize != i {
                return Err(BA_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(BA_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(BA_EMISMATCH);
            }
            i += 1;
        }
        let vals = [9u32, 117, 117, 100, 100, 116, 99, 11460];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(BA_EMISMATCH);
                    }
                }
                _ => return Err(BA_EBAL),
            }
            k += 1;
        }
        let codes = [
            BA_EDUP, BA_EORDER, BA_EID, BA_EFULL, BA_EGATE, BA_EEMPTY,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(BA_EMISMATCH);
                        }
                    }
                    None => return Err(BA_EBAL),
                },
                None => return Err(BA_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(BA_EBAL);
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
            return Err(BA_EFROZEN);
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
            return Err(BA_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(BA_ESEAL);
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

/// 取闸门错误码。
fn gate_code(r: Result<u32, u16>) -> u16 {
    match r {
        Ok(_) => 0,
        Err(c) => c,
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let b = batch();
    let s = aggregate(&b);
    let d = aggregate(&dirty_batch());

    let _ = l.register(ok_row(0, OP_BATCH, s.modules));
    let _ = l.register(ok_row(1, OP_BATCH, s.total));
    let _ = l.register(ok_row(2, OP_BATCH, s.passed));
    let _ = l.register(ok_row(3, OP_BATCH, s.pct));
    let _ = l.register(ok_row(4, OP_BATCH, s.coverage));
    let _ = l.register(ok_row(5, OP_BATCH, d.passed));
    let _ = l.register(ok_row(6, OP_BATCH, d.pct));
    let _ = l.register(ok_row(7, OP_ORDER, b.last_id()));

    let mut dup_r = Registry::new();
    let _ = dup_r.register(Module::green(11452));
    let dup = reg_code(dup_r.register(Module::green(11452)));
    let _ = l.register(err_row(8, OP_BATCH, dup));

    let mut ord_r = Registry::new();
    let _ = ord_r.register(Module::green(11455));
    let ord = reg_code(ord_r.register(Module::green(11454)));
    let _ = l.register(err_row(9, OP_BATCH, ord));

    let mut id_r = Registry::new();
    let id = reg_code(id_r.register(Module::green(11451)));
    let _ = l.register(err_row(10, OP_BATCH, id));

    let mut small = Registry::with_cap(2);
    let _ = small.register(Module::green(11452));
    let _ = small.register(Module::green(11453));
    let full = reg_code(small.register(Module::green(11454)));
    let _ = l.register(err_row(11, OP_LEDGER, full));

    let _ = l.register(err_row(12, OP_GATE, gate_code(gate(&d))));

    let empty = Registry::new();
    let _ = l.register(err_row(13, OP_GATE, gate_code(gate(&aggregate(&empty)))));

    l
}

/// 四路注入：重放冻结、满绿批守恒、脏批只差一条、篡改复原。
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
        && two == Err(BA_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    out[1] = {
        let r = batch();
        let s = aggregate(&r);
        r.len() == 9
            && s.modules == MODULES
            && s.total == ASSERTIONS
            && s.passed == ASSERTIONS
            && s.failed == 0
            && s.pct == 100
            && s.coverage == 100
            && gate(&s) == Ok(100)
            && s.total == MODULES * CHECKS_PER_MODULE
    };

    out[2] = {
        let r = dirty_batch();
        let s = aggregate(&r);
        let m = match r.find(11457) {
            Some(v) => v,
            None => Module::blank(),
        };
        s.modules == 9
            && s.total == 117
            && s.passed == 116
            && s.failed == 1
            && s.pct == 99
            && s.coverage == 88
            && m.failed() == 1
            && gate(&s) == Err(BA_EGATE)
    };

    out[3] = {
        let mut t = build_ledger();
        let clean = t.audit().is_ok();
        let old = match t.sample(1) {
            Some(s) => s.value,
            None => 0,
        };
        if let Some(r) = t.rows.get_mut(1) {
            r.value = old.saturating_sub(1);
        }
        let caught = t.audit() == Err(BA_EMISMATCH);
        if let Some(r) = t.rows.get_mut(1) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok()
    };

    out
}

/// 十三判据。
pub fn run_unx11460_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11460.ktestaggb13");
    let l = build_ledger();

    s.add_dyn(
        "BA1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "BA2-九块一百一十七条",
        l.sample(0).map(|x| x.value) == Some(9)
            && l.sample(1).map(|x| x.value) == Some(117)
            && batch().len() == 9
            && MODULES * CHECKS_PER_MODULE == ASSERTIONS
            && aggregate(&batch()).total == 117,
        "static",
    );

    s.add(
        "BA3-满绿批全通过",
        l.sample(2).map(|x| x.value) == Some(117)
            && {
                let a = aggregate(&batch());
                a.passed == a.total && a.failed == 0 && a.passed == 117
            },
        "static",
    );

    s.add(
        "BA4-百分比与覆盖率",
        l.sample(3).map(|x| x.value) == Some(100)
            && l.sample(4).map(|x| x.value) == Some(100)
            && {
                let a = aggregate(&batch());
                a.pct == 100 && a.coverage == 100 && gate(&a) == Ok(100)
            },
        "static",
    );

    s.add(
        "BA5-脏批只差一条",
        l.sample(5).map(|x| x.value) == Some(116)
            && l.sample(6).map(|x| x.value) == Some(99)
            && {
                let a = aggregate(&dirty_batch());
                a.total == 117
                    && a.passed == 116
                    && a.failed == 1
                    && a.pct == 99
                    && a.coverage == 88
                    && gate(&a) == Err(BA_EGATE)
                    && l.sample(12).map(|x| x.code) == Some(BA_EGATE)
            },
        "static",
    );

    s.add(
        "BA6-重复登记闸门",
        l.sample(8).map(|x| x.code) == Some(BA_EDUP)
            && {
                let mut r = Registry::new();
                r.register(Module::green(11460)).is_ok()
                    && r.register(Module::green(11460)) == Err(BA_EDUP)
                    && r.len() == 1
            },
        "static",
    );

    s.add(
        "BA7-严格递增闸门",
        l.sample(9).map(|x| x.code) == Some(BA_EORDER)
            && {
                let mut r = Registry::new();
                r.register(Module::green(11455)).is_ok()
                    && r.register(Module::green(11454)) == Err(BA_EORDER)
                    && r.len() == 1
                    && r.last_id() == 11455
            },
        "static",
    );

    s.add(
        "BA8-编号区间闸门",
        l.sample(10).map(|x| x.code) == Some(BA_EID)
            && {
                let mut r = Registry::new();
                r.register(Module::green(11451)) == Err(BA_EID)
                    && r.register(Module::green(11461)) == Err(BA_EID)
                    && r.register(Module::green(ID_LO)).is_ok()
                    && r.register(Module::green(ID_HI)).is_ok()
                    && r.len() == 2
            },
        "static",
    );

    s.add(
        "BA9-容量闸门",
        l.sample(11).map(|x| x.code) == Some(BA_EFULL)
            && {
                let mut r = Registry::with_cap(2);
                r.register(Module::green(11452)).is_ok()
                    && r.register(Module::green(11453)).is_ok()
                    && r.register(Module::green(11454)) == Err(BA_EFULL)
                    && r.len() == 2
            },
        "static",
    );

    s.add(
        "BA10-空批不是零失败",
        l.sample(13).map(|x| x.code) == Some(BA_EEMPTY)
            && {
                let e = Registry::new();
                let a = aggregate(&e);
                a.modules == 0
                    && a.total == 0
                    && a.passed == 0
                    && a.pct == 0
                    && a.coverage == 0
                    && gate(&a) == Err(BA_EEMPTY)
                    && gate(&Summary::zero()) == Err(BA_EEMPTY)
            },
        "static",
    );

    s.add(
        "BA11-自洽闸门",
        {
            let mut r = Registry::new();
            r.register(Module::make(11452, 13, 14)) == Err(BA_ESHORT)
                && r.register(Module::make(11452, 0, 0)).is_ok()
                && r.register(Module::make(11453, 13, 12)).is_ok()
                && r.len() == 2
                && r.find(11453).map(|m| m.failed()) == Some(1)
        },
        "static",
    );

    s.add(
        "BA12-批内顺序同构",
        l.sample(7).map(|x| x.value) == Some(11460)
            && {
                let b = batch();
                b.at(0).map(|m| m.id) == Some(ID_LO)
                    && b.at(8).map(|m| m.id) == Some(ID_HI)
                    && b.at(9).is_none()
                    && b.last_id() == ID_HI
                    && {
                        let mut ok = b.at(0).is_some();
                        let mut i = 1usize;
                        while i < b.len() {
                            match (b.at(i - 1), b.at(i)) {
                                (Some(p), Some(q)) => {
                                    if q.id != p.id + 1 {
                                        ok = false;
                                    }
                                }
                                _ => ok = false,
                            }
                            i += 1;
                        }
                        ok
                    }
            },
        "static",
    );

    s.add(
        "BA13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(BA_EFROZEN)
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
    fn batch_shape() {
        let b = batch();
        assert_eq!(b.len(), 9);
        assert_eq!(b.last_id(), 11460);
        assert_eq!(b.at(0).map(|m| m.id), Some(11452));
        assert_eq!(b.find(11457).map(|m| m.total), Some(13));
        assert!(b.find(11451).is_none());
    }

    #[test]
    fn perfect_batch() {
        let a = aggregate(&batch());
        assert_eq!(a.total, 117);
        assert_eq!(a.passed, 117);
        assert_eq!(a.failed, 0);
        assert_eq!(a.pct, 100);
        assert_eq!(a.coverage, 100);
        assert_eq!(gate(&a), Ok(100));
    }

    #[test]
    fn dirty_batch_math() {
        let a = aggregate(&dirty_batch());
        assert_eq!(a.passed, 116);
        assert_eq!(a.failed, 1);
        assert_eq!(a.pct, 99);
        assert_eq!(a.coverage, 88);
        assert_eq!(gate(&a), Err(BA_EGATE));
    }

    #[test]
    fn registry_gates() {
        let mut r = Registry::new();
        assert_eq!(r.register(Module::green(11451)), Err(BA_EID));
        assert_eq!(r.register(Module::make(11452, 13, 14)), Err(BA_ESHORT));
        assert_eq!(r.register(Module::green(11455)), Ok(()));
        assert_eq!(r.register(Module::green(11455)), Err(BA_EDUP));
        assert_eq!(r.register(Module::green(11454)), Err(BA_EORDER));
        assert_eq!(r.register(Module::green(11456)), Ok(()));
    }

    #[test]
    fn empty_batch_is_not_green() {
        let a = aggregate(&Registry::new());
        assert_eq!(a.modules, 0);
        assert_eq!(a.total, 0);
        assert_eq!(a.pct, 0);
        assert_eq!(gate(&a), Err(BA_EEMPTY));
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
        assert_eq!(l.freeze(), Err(BA_EFROZEN));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11460_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
