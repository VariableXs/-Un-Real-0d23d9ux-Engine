//! UNX-F11282 ld.so --list 直调路径（0x1f520 段）
//!
//! 同一份依赖必须能被**两条独立实现**渲染成同一串：递归 DFS 的 trace 路径，
//! 与 `ld.so --list ./prog` 直调路径。直调路径先把 argv 摆好（argv[0] 是
//! 装载器、argv[1] 必须是 `--list`），核对 AT_BASE 存在，再用**显式栈**
//! 迭代展开依赖：出栈即访问，子依赖逆序压栈，从而与递归版的前序一致。
//! 两条路径都只装载不执行；输出行格式统一为 `名字 => 路径 (0x基址)`，
//! 缺库渲染 `名字 => not found`。
//! 本账本对每条样本同时跑两条路径，落两张表：一致/不一致计数与总行数；
//! replay 要求不一致恒为 0、总行数等于重跑值，且两条路径的摘要逐字节相等。
//! 判据函数拒五种输入：路径号非法、prog 掩码为空或越界、--list 缺参、
//! trace 路径 argc 不为 1、AT_BASE 缺失。拒绝面只有 id 越界、顺序倒退、
//! 同 id 重复、封册后写账；argc=0、prog=0 都是样本，进规则产码，绝不拒收。

use crate::alloc::format;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::string::String;

/// 诊断段（0x1f520 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f520;

/// 条目 id 越界。
pub const LS_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const LS_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const LS_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const LS_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const LS_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const LS_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算判据与登记时不符。
pub const LS_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const LS_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const LS_EDOBS: u32 = SEG_BASE + 0x09;
/// 一致表/行数账不平。
pub const LS_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：路径号非法。
pub const LV_PATH: u32 = 1;
/// 判据码：prog 掩码为空或越界。
pub const LV_PROG: u32 = 2;
/// 判据码：--list 参数/argc 不合契约。
pub const LV_ARGV: u32 = 3;
/// 判据码：AT_BASE 缺失。
pub const LV_BASE: u32 = 4;

/// 路径号：trace（递归）。
pub const PATH_TRACE: u32 = 1;
/// 路径号：--list 直调（显式栈）。
pub const PATH_LIST: u32 = 2;
/// --list 旗标位（argv[1] 已就位）。
pub const F_FLAG: u32 = 1 << 0;
/// auxv 里 AT_BASE 存在。
pub const F_AT_BASE: u32 = 1 << 1;
/// 合法标志位并集。
pub const FLAG_MASK: u32 = F_FLAG | F_AT_BASE;
/// 目录定容。
pub const LIB_CAP: usize = 6;
/// 目录位掩码。
pub const CAT_MASK: u32 = 0x3f;
/// 深度上限。
pub const DEPTH_MAX: u32 = 6;
/// 样本条数。
pub const SAMPLES: usize = 30;
/// 条目表定容。
pub const CAP: usize = 32;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_82;

/// 目录条目：名字 + 路径 + 基址 + 依赖位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lib {
    pub name: &'static str,
    pub path: &'static str,
    pub base: u64,
    pub deps: u32,
}

/// 六个目录条目；第 5 条路径为空（缺库）。
pub const CATALOG: [Lib; LIB_CAP] = [
    Lib { name: "libc.so.6", path: "/lib/libc.so.6", base: 0x7f10_0000_0000, deps: 1 << 1 },
    Lib { name: "ld-musl-x86_64.so.1", path: "/lib/ld-musl-x86_64.so.1", base: 0x7f10_0001_0000, deps: 0 },
    Lib { name: "libm.so.6", path: "/lib/libm.so.6", base: 0x7f10_0002_0000, deps: 1 << 0 },
    Lib { name: "libssl.so.3", path: "/usr/lib/libssl.so.3", base: 0x7f10_0003_0000, deps: (1 << 0) | (1 << 4) },
    Lib { name: "libcrypto.so.3", path: "/usr/lib/libcrypto.so.3", base: 0x7f10_0004_0000, deps: 1 << 0 },
    Lib { name: "libfoo.so.1", path: "", base: 0, deps: 0 },
];

/// 输出：逐行 + 找到/缺失 + 摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Out {
    pub lines: Vec<String>,
    pub found: u32,
    pub missing: u32,
}

impl Out {
    pub fn new() -> Out {
        Out { lines: Vec::new(), found: 0, missing: 0 }
    }

    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.lines.len() {
            if let Some(l) = self.lines.get(k) {
                let bytes = l.as_bytes();
                let mut b = 0usize;
                while b < bytes.len() {
                    if let Some(v) = bytes.get(b) {
                        h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
                    }
                    b = b.wrapping_add(1);
                }
                h = (h ^ 0x0a).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 行渲染：缺库 / 普通件两种格式。
fn emit(out: &mut Out, idx: usize) {
    match CATALOG.get(idx) {
        Some(l) => {
            if l.path.is_empty() {
                out.missing = out.missing.wrapping_add(1);
                out.lines.push(format!("{} => not found", l.name));
            } else {
                out.found = out.found.wrapping_add(1);
                out.lines.push(format!("{} => {} (0x{:x})", l.name, l.path, l.base));
            }
        }
        None => {}
    }
}

fn dfs(idx: usize, visited: &mut u32, out: &mut Out, depth: u32) {
    if depth > DEPTH_MAX {
        return;
    }
    let bit = 1u32 << (idx as u32 & 31);
    if *visited & bit != 0 {
        return;
    }
    *visited |= bit;
    emit(out, idx);
    let mut b = 0u32;
    while b < LIB_CAP as u32 {
        if let Some(l) = CATALOG.get(idx) {
            if l.deps & (1u32 << b) != 0 {
                dfs(b as usize, visited, out, depth.wrapping_add(1));
            }
        }
        b = b.wrapping_add(1);
    }
}

/// trace 路径（递归，参考实现）。
pub fn trace_path(root: u32) -> Out {
    let mut out = Out::new();
    let mut visited = 0u32;
    let mut b = 0u32;
    while b < LIB_CAP as u32 {
        if root & (1u32 << b) != 0 {
            dfs(b as usize, &mut visited, &mut out, 0);
        }
        b = b.wrapping_add(1);
    }
    out
}

/// --list 直调路径（显式栈，独立实现）。
/// argv 契约：argc 至少 2 且 argv[1] 是 --list 旗标，否则返回 None。
pub fn list_path(root: u32, argc: u32, flag_ok: bool) -> Option<Out> {
    if argc < 2 || !flag_ok {
        return None;
    }
    let mut out = Out::new();
    let mut visited = 0u32;
    let mut stack: Vec<u32> = Vec::new();
    let mut b = LIB_CAP as u32;
    while b > 0 {
        b = b.wrapping_sub(1);
        if root & (1u32 << b) != 0 {
            stack.push(b);
        }
    }
    while let Some(idx) = stack.pop() {
        let bit = 1u32 << (idx & 31);
        if visited & bit != 0 {
            continue;
        }
        visited |= bit;
        emit(&mut out, idx as usize);
        let mut d = LIB_CAP as u32;
        while d > 0 {
            d = d.wrapping_sub(1);
            if let Some(l) = CATALOG.get(idx as usize) {
                if l.deps & (1u32 << d) != 0 {
                    stack.push(d);
                }
            }
        }
    }
    Some(out)
}

/// 一条直调样本：prog 掩码 + 路径号 + argc + 标志位 + AT_BASE。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListCase {
    pub id: u32,
    pub prog: u32,
    pub path: u32,
    pub argc: u32,
    pub flags: u32,
    pub at_base: u64,
}

impl ListCase {
    pub const fn blank() -> ListCase {
        ListCase { id: 0, prog: 0, path: 0, argc: 0, flags: 0, at_base: 0 }
    }

    pub fn render(&self) -> String {
        format!("L{} prog={:02x} path{} argc{} fl{:x} base={:x}",
            self.id, self.prog, self.path, self.argc, self.flags, self.at_base)
    }
}

/// 纯判据：路径号 → prog → argv → AT_BASE。
pub fn list_verdict(c: &ListCase) -> u32 {
    if c.path != PATH_LIST && c.path != PATH_TRACE {
        return LV_PATH;
    }
    if c.prog == 0 || c.prog & !CAT_MASK != 0 {
        return LV_PROG;
    }
    if c.path == PATH_LIST {
        if c.argc < 2 || c.flags & F_FLAG == 0 {
            return LV_ARGV;
        }
    } else if c.argc != 1 {
        return LV_ARGV;
    }
    if c.flags & F_AT_BASE == 0 || c.at_base == 0 {
        return LV_BASE;
    }
    0
}

/// 双路径账本：样本 + 判据 + 五槽直方 + 一致/不一致双槽 + 总行数。
#[derive(Clone, Copy, Debug)]
pub struct ListLedger {
    cases: [ListCase; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 5],
    agree: [u32; 2],
    lines_total: u32,
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl ListLedger {
    pub const fn new() -> ListLedger {
        ListLedger {
            cases: [ListCase::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 5],
            agree: [0u32; 2],
            lines_total: 0,
            len: 0,
            attempts: 0,
            accepts: 0,
            rejects: 0,
            observed: 0,
            matched: 0,
            frozen: false,
            root: None,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记：封册 → id 域 → 重复 → 顺序 → 定容。
    pub fn register(&mut self, c: ListCase) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(LS_EFROZEN));
        }
        if c.id < ID_MIN || c.id > ID_MAX {
            return Err(self.fail(LS_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == c.id {
                    return Err(self.fail(LS_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if c.id <= last.id {
                    return Err(self.fail(LS_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(LS_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = c;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：放行的样本双路径各跑一遍，落一致表与行数账。
    pub fn register_ruled(&mut self, c: ListCase) -> Result<(usize, u32), u32> {
        let idx = self.register(c)?;
        let v = list_verdict(&c);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if v == 0 {
            let t = trace_path(c.prog);
            let l = list_path(c.prog, 2, true);
            let same = match l {
                Some(o) => o.lines == t.lines && o.digest() == t.digest(),
                None => false,
            };
            let slot_i = if same { 0usize } else { 1usize };
            if let Some(a) = self.agree.get_mut(slot_i) {
                *a = a.wrapping_add(1);
            }
            self.lines_total = self.lines_total.wrapping_add(t.lines.len() as u32);
        }
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(LS_EFROZEN);
        }
        let mut k = 0usize;
        let mut hit: Option<usize> = None;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == id {
                    hit = Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        let idx = match hit {
            Some(v) => v,
            None => return Err(LS_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(LS_EDOBS),
            _ => {}
        }
        if let Some(slot) = self.seen.get_mut(idx) {
            *slot = true;
        }
        self.observed = self.observed.wrapping_add(1);
        let expect = match self.verdicts.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if seen_code == expect {
            self.matched = self.matched.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn observed(&self) -> u32 {
        self.observed
    }
    pub fn matched(&self) -> u32 {
        self.matched
    }
    pub fn agree_at(&self, i: usize) -> u32 {
        match self.agree.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn lines_total(&self) -> u32 {
        self.lines_total
    }
    pub fn hist_at(&self, i: usize) -> u32 {
        match self.hist.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn hist_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 5 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn zero_verdicts(&self) -> u32 {
        self.hist_at(0)
    }
    pub fn case_at(&self, i: usize) -> Option<ListCase> {
        self.cases.get(i).copied()
    }
    pub fn verdict_at(&self, i: usize) -> Option<u32> {
        self.verdicts.get(i).copied()
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    /// 重跑双路径：返回 (一致数, 不一致数, 总行数)。
    pub fn recompare(&self) -> (u32, u32, u32) {
        let mut same = 0u32;
        let mut diff = 0u32;
        let mut lines = 0u32;
        let mut k = 0usize;
        while k < self.len {
            if let Some(c) = self.cases.get(k) {
                if list_verdict(&c) == 0 {
                    let t = trace_path(c.prog);
                    lines = lines.wrapping_add(t.lines.len() as u32);
                    let ok = match list_path(c.prog, 2, true) {
                        Some(o) => o.lines == t.lines && o.digest() == t.digest(),
                        None => false,
                    };
                    if ok {
                        same = same.wrapping_add(1);
                    } else {
                        diff = diff.wrapping_add(1);
                    }
                }
            }
            k = k.wrapping_add(1);
        }
        (same, diff, lines)
    }

    /// FNV-1a：样本全字段 + 判据 + 观测位 + 直方 + 一致表 + 行数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.attempts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.observed as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.matched as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.lines_total as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.agree_at(0) as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.agree_at(1) as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frozen as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.len {
            if let Some(c) = self.cases.get(k) {
                h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.prog as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.path as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.argc as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.flags as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.at_base).wrapping_mul(FNV_PRIME);
            }
            if let Some(v) = self.verdicts.get(k) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(b) = self.seen.get(k) {
                h = (h ^ *b as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < 5 {
            if let Some(v) = self.hist.get(j) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h
    }

    /// 复账：全登记、全观测、全复算，且一致表/行数账与重跑相同、不一致恒 0。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 {
            return Err(LS_EUNOBS);
        }
        if self.observed as usize != self.len || self.matched as usize != self.len {
            return Err(LS_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(LS_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 {
            return Err(LS_EBALANCE);
        }
        let (same, diff, lines) = self.recompare();
        if diff != 0 || same != self.agree_at(0) || lines != self.lines_total {
            return Err(LS_EBALANCE);
        }
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(LS_EUNOBS),
            };
            let v = match self.verdicts.get(k) {
                Some(x) => *x,
                None => return Err(LS_EMISMATCH),
            };
            if list_verdict(&c) != v {
                return Err(LS_EMISMATCH);
            }
            match self.seen.get(k) {
                Some(true) => {}
                _ => return Err(LS_EUNOBS),
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：复账后上锁，二次封册即 EFROZEN。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(LS_EFROZEN);
        }
        let h = self.replay()?;
        self.frozen = true;
        self.root = Some(h);
        Ok(h)
    }
}

/// 构造一条直调样本（id 由调用方补）。
pub fn list_case(prog: u32, path: u32, argc: u32, flags: u32, at_base: u64) -> ListCase {
    ListCase { id: 1, prog, path, argc, flags, at_base }
}

/// 三十条规格样本：奇数为直调、偶数为 trace，prog 按 5 的倍数铺开。
pub fn spec_of(id: usize) -> (ListCase, u32) {
    if id < 1 || id > SAMPLES {
        return (ListCase::blank(), LS_EID);
    }
    let prog = (id as u32).wrapping_mul(5) & CAT_MASK;
    let (path, argc, mut flags) = if id % 2 == 1 {
        (PATH_LIST, 2u32, F_FLAG)
    } else {
        (PATH_TRACE, 1u32, 0u32)
    };
    flags |= F_AT_BASE;
    (list_case(prog, path, argc, flags, 0x7f20_0000_0000), 0)
}

/// 逐条登记 + 观测。
pub fn build_ledger() -> ListLedger {
    let mut l = ListLedger::new();
    let mut i = 1usize;
    while i <= SAMPLES {
        let (mut c, expect) = spec_of(i);
        c.id = i as u32;
        if l.register_ruled(c).is_ok() {
            let _ = l.observe(c.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、越界、封册。
pub fn inject_four() -> [bool; 4] {
    let mut a = ListLedger::new();
    let mut hi = list_case(0x01, PATH_LIST, 2, F_FLAG | F_AT_BASE, 0x1000);
    hi.id = 2;
    let _ = a.register(hi);
    let bad_a = a.register(list_case(0x01, PATH_LIST, 2, F_FLAG | F_AT_BASE, 0x1000)).err()
        == Some(LS_EORDER);

    let mut b = ListLedger::new();
    let one = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    let _ = b.register(one);
    let bad_b = b.register(one).err() == Some(LS_EDUP);

    let mut c = ListLedger::new();
    let mut wide = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(LS_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    late.id = 40;
    let bad_d = d.register(late).err() == Some(LS_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压双路径一致性与 argv 契约，后四项压拒绝面。
pub fn run_unx11282_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11282");

    let layout = SEG_BASE == 0x1f520
        && LS_EID == SEG_BASE + 0x01
        && LS_EORDER == SEG_BASE + 0x02
        && LS_EDUP == SEG_BASE + 0x03
        && LS_EFULL == SEG_BASE + 0x04
        && LS_EFROZEN == SEG_BASE + 0x05
        && LS_EUNSEEN == SEG_BASE + 0x06
        && LS_EMISMATCH == SEG_BASE + 0x07
        && LS_EUNOBS == SEG_BASE + 0x08
        && LS_EDOBS == SEG_BASE + 0x09
        && LS_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1f520 段十码连续");

    let one = trace_path(0x01);
    let two = match list_path(0x01, 2, true) {
        Some(v) => v,
        None => Out::new(),
    };
    s.add("S2-单根双路径逐字相同", one.lines == two.lines
        && one.digest() == two.digest()
        && one.found == 2
        && two.found == 2,
        "libc 与解释器两行正文与摘要完全一致");

    let mut all_same = true;
    let mut root = 1u32;
    while root <= CAT_MASK {
        let t = trace_path(root);
        let l = match list_path(root, 2, true) {
            Some(v) => v,
            None => Out::new(),
        };
        if t.lines != l.lines || t.digest() != l.digest() || t.found != l.found || t.missing != l.missing {
            all_same = false;
        }
        root = root.wrapping_add(1);
    }
    s.add("S3-全 63 个根掩码两路同序", all_same,
        "递归 DFS 与显式栈迭代在 1..=0x3f 每个根上逐行相等");

    let led = build_ledger();
    s.add("S4-三十样本一致表全绿", led.len() == SAMPLES
        && led.zero_verdicts() == SAMPLES as u32
        && led.agree_at(0) == SAMPLES as u32
        && led.agree_at(1) == 0,
        "30 条样本双路径全部一致，不一致槽为空");

    let argv_gate = list_verdict(&list_case(0x01, PATH_LIST, 1, F_FLAG | F_AT_BASE, 0x1000)) == LV_ARGV
        && list_verdict(&list_case(0x01, PATH_LIST, 1, F_AT_BASE, 0x1000)) == LV_ARGV
        && list_verdict(&list_case(0x01, PATH_TRACE, 2, F_AT_BASE, 0x1000)) == LV_ARGV
        && list_verdict(&list_case(0x01, PATH_LIST, 2, F_FLAG | F_AT_BASE, 0x1000)) == 0;
    s.add("S5-list 参数旗标契约", argv_gate,
        "直调 argc<2 或缺 --list 判 LV_ARGV；trace argc 非 1 同判");

    let path_gate = list_verdict(&list_case(0x01, 0, 2, F_FLAG | F_AT_BASE, 0x1000)) == LV_PATH
        && list_verdict(&list_case(0x01, 9, 2, F_FLAG | F_AT_BASE, 0x1000)) == LV_PATH
        && list_verdict(&list_case(0x00, PATH_TRACE, 1, F_AT_BASE, 0x1000)) == LV_PROG
        && list_verdict(&list_case(0x40, PATH_TRACE, 1, F_AT_BASE, 0x1000)) == LV_PROG;
    s.add("S6-路径号与 prog 域", path_gate,
        "路径 0/9 判 LV_PATH，prog 0/0x40 判 LV_PROG");

    let base_gate = list_verdict(&list_case(0x01, PATH_TRACE, 1, 0, 0x1000)) == LV_BASE
        && list_verdict(&list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0)) == LV_BASE
        && list_verdict(&list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000)) == 0;
    s.add("S7-AT_BASE 必备", base_gate, "位缺或基址为 0 判 LV_BASE");

    let (same, diff, lines) = led.recompare();
    s.add("S8-行数账与重跑一致", same == SAMPLES as u32
        && diff == 0
        && lines == led.lines_total()
        && led.lines_total() > 0,
        "重跑 30 条行数合计与登记账相等，不一致为 0");

    s.add("S9-直方与一致表双平衡", led.hist_sum() == 30
        && led.agree_at(0) + led.agree_at(1) == led.zero_verdicts()
        && led.observed() == 30
        && led.matched() == 30,
        "直方 30 条、一致表合计等于放行数、观测 30 条");

    s.add("S10-复账与双摘要稳定", led.replay().is_ok()
        && build_ledger().fold() == led.fold()
        && trace_path(0x0c).digest() == match list_path(0x0c, 2, true) {
            Some(v) => v.digest(),
            None => 0,
        },
        "两条独立实现同根同摘要，两次构建同指纹");

    let mut g = ListLedger::new();
    let mut c2 = list_case(0x02, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    c2.id = 2;
    let _ = g.register(c2);
    let mut c1 = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    c1.id = 1;
    let bad_a = g.register(c1).err() == Some(LS_EORDER);
    let bad_b = g.register(c2).err() == Some(LS_EDUP);
    let mut wide = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    wide.id = 0;
    let bad_c = g.register(wide).err() == Some(LS_EID);
    let mut full = build_ledger();
    let mut extra = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    extra.id = 31;
    let ok_extra = full.register(extra).is_ok();
    let mut second = list_case(0x02, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    second.id = 32;
    let ok_second = full.register(second).is_ok();
    let mut over = list_case(0x03, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    over.id = 33;
    let bad_d = full.register(over).err() == Some(LS_EFULL);
    s.add("S11-顺序重复越界定容四闸", bad_a && bad_b && bad_c && bad_d
        && ok_extra && ok_second
        && g.accepts() == 1 && g.rejects() == 3 && g.attempts() == 4
        && inject_four() == [true, true, true, true],
        "四闸独立触发；容量 32 条，第 33 条 EFULL");

    let mut ob = ListLedger::new();
    let mut one_c = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    one_c.id = 1;
    let _ = ob.register_ruled(one_c);
    let miss = ob.replay().err() == Some(LS_EUNOBS);
    let unseen = ob.observe(9, 0).err() == Some(LS_EUNSEEN);
    let wrong = ob.observe(1, LV_PATH) == Ok(false);
    let dob = ob.observe(1, 0).err() == Some(LS_EDOBS);
    s.add("S12-观测面三门", miss && unseen && wrong && dob,
        "未观测 EUNOBS、未登记 EUNSEEN、错观测 false、重观测 EDOBS");

    let mut fz = build_ledger();
    let root = match fz.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = fz.freeze();
    let mut late = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 0x1000);
    late.id = 40;
    let post = fz.register(late).err();
    s.add_dyn("S13-封册先复账后上锁", root != 0
        && again == Err(LS_EFROZEN)
        && post == Some(LS_EFROZEN)
        && fz.root() == Some(root)
        && fz.replay().is_ok(),
        &format!("根={:x} 二次={:?} 封后写={:?}", root, again, post));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11282_two_paths_agree() {
        let mut root = 1u32;
        while root <= CAT_MASK {
            let t = trace_path(root);
            let l = match list_path(root, 2, true) {
                Some(v) => v,
                None => Out::new(),
            };
            assert_eq!(t.lines, l.lines);
            assert_eq!(t.digest(), l.digest());
            assert_eq!(t.found, l.found);
            root += 1;
        }
        assert_eq!(trace_path(1 << 5).missing, 1);
        assert_eq!(trace_path(1 << 5).lines.get(0).map(|l| l.as_str()),
            Some("libfoo.so.1 => not found"));
    }

    #[test]
    fn unx11282_arg_contract() {
        assert!(list_path(0x01, 1, true).is_none());
        assert!(list_path(0x01, 2, false).is_none());
        assert!(list_path(0x01, 2, true).is_some());
        assert_eq!(list_verdict(&list_case(1, PATH_LIST, 1, F_FLAG | F_AT_BASE, 1)), LV_ARGV);
        assert_eq!(list_verdict(&list_case(1, PATH_TRACE, 2, F_AT_BASE, 1)), LV_ARGV);
        assert_eq!(list_verdict(&list_case(1, 4, 1, F_AT_BASE, 1)), LV_PATH);
        assert_eq!(list_verdict(&list_case(0, PATH_TRACE, 1, F_AT_BASE, 1)), LV_PROG);
        assert_eq!(list_verdict(&list_case(1, PATH_TRACE, 1, 0, 1)), LV_BASE);
        assert_eq!(list_verdict(&list_case(1, PATH_TRACE, 1, F_AT_BASE, 1)), 0);
    }

    #[test]
    fn unx11282_ledger_agree() {
        let l = build_ledger();
        assert_eq!(l.len(), 30);
        assert_eq!(l.zero_verdicts(), 30);
        assert_eq!(l.agree_at(0), 30);
        assert_eq!(l.agree_at(1), 0);
        assert!(l.lines_total() > 30);
        assert!(l.replay().is_ok());
        assert_eq!(l.hist_sum(), 30);
    }

    #[test]
    fn unx11282_refusals() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = ListLedger::new();
        let c = list_case(0x01, PATH_TRACE, 1, F_AT_BASE, 1);
        assert!(l.register(c).is_ok());
        assert_eq!(l.register(c).err(), Some(LS_EDUP));
        assert_eq!(l.attempts(), 2);
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn unx11282_freeze_and_checks() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(root != 0);
        assert_eq!(l.freeze(), Err(LS_EFROZEN));
        assert!(l.replay().is_ok());
        let s = run_unx11282_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
