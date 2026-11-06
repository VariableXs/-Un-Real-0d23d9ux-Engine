//! UNX-F11357 procfs 诊断联动账（0xec50 段）
//!
//! 诊断工具（`ps`、`lsof`、`pstack`）各自从 procfs 的不同文件里读线索：maps 给
//! 地址布局，fd 给描述符，status 给状态行。这些线索按同一个进程号聚合之后才能
//! 互相印证。联动账要处理三种关系：
//!
//! 1. **同键多源**：同一个键被多个来源登记。只有**取值一致**时才能合并成一条
//!    账目；取值打架时合并必须被拒绝（`ECONFLICT`）——把两个互相矛盾的结论
//!    缝在一条账上，比不合并更有害。
//! 2. **有向依赖**：账目之间可以建立「A 依赖 B」的边。边不能成环（`ECYCLE`），
//!    否则诊断结论循环引用，永远无法定序。
//! 3. **合并先行**：只有已合并的键才能建边（`ENOENT`），也才能进入账目；
//!    一条账都没有时读取账目是未就绪（`EUNMERGED`）。
//!
//! 闸门次序：
//! - `post`：来源/键非法 → 严重级非法 → 同源同键重复 → 表满；
//! - `merge`：无此键 → 已合并 → 取值冲突 → 合并表满；
//! - `link`：自环 → 端点未合并 → 重复边 → 表满 → 成环。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec50;

/// 来源或键非法。
pub const DL_EINVAL: u32 = SEG_BASE + 0x01;
/// 严重级非法。
pub const DL_ENOSEV: u32 = SEG_BASE + 0x02;
/// 同源同键重复登记，或重复合并，或重复边。
pub const DL_EDUP: u32 = SEG_BASE + 0x03;
/// 表满。
pub const DL_EFULL: u32 = SEG_BASE + 0x04;
/// 键不存在或端点未合并。
pub const DL_ENOENT: u32 = SEG_BASE + 0x05;
/// 同键多源取值冲突。
pub const DL_ECONFLICT: u32 = SEG_BASE + 0x06;
/// 依赖边成环。
pub const DL_ECYCLE: u32 = SEG_BASE + 0x07;
/// 尚无合并账目。
pub const DL_EUNMERGED: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DL_ENOBS: u32 = SEG_BASE + 0x0d;

/// 登记表容量。
pub const CAP: usize = 8;
/// 合并表容量。
pub const MCAP: usize = 6;
/// 边表容量。
pub const ECAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 来源：maps。
pub const S_MAPS: u8 = 1;
/// 来源：fd。
pub const S_FD: u8 = 2;
/// 来源：status。
pub const S_STATUS: u8 = 3;

/// 严重级：提示。
pub const SEV_INFO: u8 = 1;
/// 严重级：告警。
pub const SEV_WARN: u8 = 2;
/// 严重级：错误。
pub const SEV_ERR: u8 = 3;

/// 操作码：登记。
pub const OP_POST: u8 = 1;
/// 操作码：合并。
pub const OP_MERGE: u8 = 2;
/// 操作码：建边。
pub const OP_LINK: u8 = 3;
/// 操作码：读取账目。
pub const OP_ACCOUNT: u8 = 4;
/// 操作码：读取冲突计数。
pub const OP_OBS: u8 = 5;

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

/// 一条原始登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diag {
    /// 来源。
    pub src: u8,
    /// 键。
    pub key: u32,
    /// 严重级。
    pub sev: u8,
    /// 取值。
    pub val: u32,
}

/// 一条合并账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Merged {
    /// 键。
    pub key: u32,
    /// 取值（所有来源一致）。
    pub val: u32,
    /// 来源掩码。
    pub mask: u8,
    /// 最高严重级。
    pub sev: u8,
}

/// 联动账。
#[derive(Clone, Copy, Debug)]
pub struct Linker {
    d: [Diag; CAP],
    n: usize,
    m: [Merged; MCAP],
    mn: usize,
    ef: [u32; ECAP],
    et: [u32; ECAP],
    en: usize,
    posts: usize,
    merges: usize,
    conflict_hits: usize,
}

impl Linker {
    /// 空联动账。
    pub const fn new() -> Linker {
        Linker {
            d: [Diag { src: 0, key: 0, sev: 0, val: 0 }; CAP],
            n: 0,
            m: [Merged { key: 0, val: 0, mask: 0, sev: 0 }; MCAP],
            mn: 0,
            ef: [0; ECAP],
            et: [0; ECAP],
            en: 0,
            posts: 0,
            merges: 0,
            conflict_hits: 0,
        }
    }

    /// 登记条数。
    pub fn post_count(&self) -> usize {
        self.n
    }

    /// 合并条数。
    pub fn merged_count(&self) -> usize {
        self.mn
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.en
    }

    /// 合并次数。
    pub fn merges(&self) -> usize {
        self.merges
    }

    /// 冲突次数。
    pub fn conflict_count(&self) -> usize {
        self.conflict_hits
    }

    /// 第 i 条登记。
    pub fn diag_at(&self, i: usize) -> Option<Diag> {
        if i >= self.n {
            return None;
        }
        self.d.get(i).copied()
    }

    /// 第 i 条合并账目。
    pub fn merged_at(&self, i: usize) -> Option<Merged> {
        if i >= self.mn {
            return None;
        }
        self.m.get(i).copied()
    }

    /// 第 i 条边。
    pub fn edge_at(&self, i: usize) -> Option<(u32, u32)> {
        if i >= self.en {
            return None;
        }
        match (self.ef.get(i), self.et.get(i)) {
            (Some(a), Some(b)) => Some((*a, *b)),
            _ => None,
        }
    }

    /// 按来源与键找登记。
    fn find_post(&self, src: u8, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.d.get(i) {
                Some(e) => {
                    if e.src == src && e.key == key {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 按键找合并账目。
    fn find_merged(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.mn {
            match self.m.get(i) {
                Some(e) => {
                    if e.key == key {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 某条边是否存在。
    fn has_edge(&self, from: u32, to: u32) -> bool {
        let mut i = 0usize;
        while i < self.en {
            match (self.ef.get(i), self.et.get(i)) {
                (Some(a), Some(b)) => {
                    if *a == from && *b == to {
                        return true;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        false
    }

    /// 从 `from` 出发能否到达 `to`。
    fn reachable(&self, from: u32, to: u32) -> bool {
        let mut seen: u64 = 0;
        let mut stack = [0u32; ECAP + 2];
        if let Some(slot) = stack.get_mut(0) {
            *slot = from;
        }
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let cur = match stack.get(sp) {
                Some(v) => *v,
                None => return false,
            };
            if cur == to {
                return true;
            }
            let bit = 1u64 << (cur & 63);
            if seen & bit != 0 {
                continue;
            }
            seen |= bit;
            let mut i = 0usize;
            while i < self.en {
                match (self.ef.get(i), self.et.get(i)) {
                    (Some(a), Some(b)) => {
                        if *a == cur {
                            if let Some(slot) = stack.get_mut(sp) {
                                *slot = *b;
                                sp += 1;
                            }
                        }
                    }
                    _ => break,
                }
                i += 1;
            }
        }
        false
    }

    /// 登记一条线索。
    pub fn post(&mut self, src: u8, key: u32, sev: u8, val: u32) -> Result<usize, u32> {
        if src == 0 || src > S_STATUS || key == 0 {
            return Err(DL_EINVAL);
        }
        if sev == 0 || sev > SEV_ERR {
            return Err(DL_ENOSEV);
        }
        if self.find_post(src, key).is_some() {
            return Err(DL_EDUP);
        }
        if self.n >= CAP {
            return Err(DL_EFULL);
        }
        if let Some(slot) = self.d.get_mut(self.n) {
            *slot = Diag { src, key, sev, val };
        }
        self.n += 1;
        self.posts += 1;
        Ok(self.n)
    }

    /// 合并同一个键的全部线索。
    pub fn merge(&mut self, key: u32) -> Result<usize, u32> {
        if self.find_merged(key).is_some() {
            return Err(DL_EDUP);
        }
        let mut cnt = 0usize;
        let mut first = true;
        let mut val = 0u32;
        let mut mask = 0u8;
        let mut sev = 0u8;
        let mut i = 0usize;
        while i < self.n {
            let e = match self.d.get(i) {
                Some(v) => *v,
                None => return Err(DL_EBAL),
            };
            if e.key == key {
                if first {
                    val = e.val;
                    first = false;
                } else if e.val != val {
                    self.conflict_hits += 1;
                    return Err(DL_ECONFLICT);
                }
                cnt += 1;
                mask |= 1u8 << (e.src & 7);
                if e.sev > sev {
                    sev = e.sev;
                }
            }
            i += 1;
        }
        if cnt == 0 {
            return Err(DL_ENOENT);
        }
        if self.mn >= MCAP {
            return Err(DL_EFULL);
        }
        let mut at = 0usize;
        while at < self.mn {
            match self.m.get(at) {
                Some(x) => {
                    if x.key > key {
                        break;
                    }
                }
                None => break,
            }
            at += 1;
        }
        let mut k = self.mn;
        while k > at {
            match self.m.get(k - 1).copied() {
                Some(prev) => {
                    if let Some(dst) = self.m.get_mut(k) {
                        *dst = prev;
                    }
                }
                None => break,
            }
            k -= 1;
        }
        if let Some(dst) = self.m.get_mut(at) {
            *dst = Merged { key, val, mask, sev };
        }
        self.mn += 1;
        self.merges += 1;
        Ok(cnt)
    }

    /// 建立一条依赖边。
    pub fn link(&mut self, from: u32, to: u32) -> Result<usize, u32> {
        if from == to {
            return Err(DL_EINVAL);
        }
        if self.find_merged(from).is_none() || self.find_merged(to).is_none() {
            return Err(DL_ENOENT);
        }
        if self.has_edge(from, to) {
            return Err(DL_EDUP);
        }
        if self.en >= ECAP {
            return Err(DL_EFULL);
        }
        if self.reachable(to, from) {
            return Err(DL_ECYCLE);
        }
        if let Some(slot) = self.ef.get_mut(self.en) {
            *slot = from;
        }
        if let Some(slot) = self.et.get_mut(self.en) {
            *slot = to;
        }
        self.en += 1;
        Ok(self.en)
    }

    /// 账目条数（要求已合并且按升序存放）。
    pub fn account(&self) -> Result<usize, u32> {
        if self.mn == 0 {
            return Err(DL_EUNMERGED);
        }
        let mut i = 1usize;
        while i < self.mn {
            match (self.m.get(i - 1), self.m.get(i)) {
                (Some(a), Some(b)) => {
                    if a.key >= b.key {
                        return Err(DL_EBAL);
                    }
                }
                _ => return Err(DL_EBAL),
            }
            i += 1;
        }
        Ok(self.mn)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP || self.mn > MCAP || self.en > ECAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let e = match self.d.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.src == 0 || e.src > S_STATUS || e.key == 0 || e.sev == 0 || e.sev > SEV_ERR {
                return false;
            }
            let mut j = i + 1;
            while j < self.n {
                match self.d.get(j) {
                    Some(o) => {
                        if o.src == e.src && o.key == e.key {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.mn {
            let m = match self.m.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if m.key == 0 || m.mask == 0 || m.sev == 0 {
                return false;
            }
            if k > 0 {
                match self.m.get(k - 1) {
                    Some(prev) => {
                        if prev.key >= m.key {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            let mut found = false;
            let mut t = 0usize;
            while t < self.n {
                match self.d.get(t) {
                    Some(e) => {
                        if e.key == m.key {
                            found = true;
                            if e.val != m.val {
                                return false;
                            }
                        }
                    }
                    None => return false,
                }
                t += 1;
            }
            if !found {
                return false;
            }
            k += 1;
        }
        let mut e = 0usize;
        while e < self.en {
            match (self.ef.get(e), self.et.get(e)) {
                (Some(a), Some(b)) => {
                    if a == b || self.find_merged(*a).is_none() || self.find_merged(*b).is_none() {
                        return false;
                    }
                }
                _ => return false,
            }
            e += 1;
        }
        true
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
            return Err(DL_ESLOT);
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
            return Err(DL_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 1, 1, 2, 0];
        const CODES: [u32; 6] =
            [DL_EINVAL, DL_EDUP, DL_ENOENT, DL_EFULL, DL_ECYCLE, DL_ECONFLICT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DL_EBAL),
            };
            if s.slot as usize != i {
                return Err(DL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DL_EVALUE);
                        }
                    }
                    None => return Err(DL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DL_EVALUE);
                        }
                    }
                    None => return Err(DL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DL_EBAL);
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
            return Err(DL_EBAL);
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
            Err(DL_ESEAL)
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

/// 十四格账本：三源登记 → 两键合并 → 建边 → 账目 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut k = Linker::new();

    let _ = l.register(cell(0, OP_POST, k.post(S_MAPS, 7, SEV_WARN, 100)));
    let _ = l.register(cell(1, OP_POST, k.post(S_FD, 7, SEV_INFO, 100)));
    let _ = l.register(cell(2, OP_POST, k.post(S_STATUS, 8, SEV_ERR, 200)));
    let _ = l.register(cell(3, OP_MERGE, k.merge(7)));
    let _ = l.register(cell(4, OP_MERGE, k.merge(8)));
    let _ = l.register(cell(5, OP_LINK, k.link(7, 8)));
    let _ = l.register(cell(6, OP_ACCOUNT, k.account()));
    let _ = l.register(cell(7, OP_OBS, Ok(k.conflict_count())));

    let mut e = Linker::new();
    let _ = l.register(cell(8, OP_POST, e.post(0, 1, SEV_INFO, 1)));

    let mut d = Linker::new();
    let _ = d.post(S_MAPS, 5, SEV_INFO, 1);
    let _ = l.register(cell(9, OP_POST, d.post(S_MAPS, 5, SEV_WARN, 2)));

    let mut n = Linker::new();
    let _ = l.register(cell(10, OP_MERGE, n.merge(42)));

    let mut f = Linker::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let src = ((i % 3) as u8) + 1;
        let _ = f.post(src, i, SEV_INFO, 1);
        i += 1;
    }
    let _ = l.register(cell(11, OP_POST, f.post(S_MAPS, 99, SEV_INFO, 1)));

    let mut c = Linker::new();
    let _ = c.post(S_MAPS, 1, SEV_INFO, 5);
    let _ = c.post(S_MAPS, 2, SEV_INFO, 6);
    let _ = c.merge(1);
    let _ = c.merge(2);
    let _ = c.link(1, 2);
    let _ = l.register(cell(12, OP_LINK, c.link(2, 1)));

    let mut x = Linker::new();
    let _ = x.post(S_MAPS, 3, SEV_INFO, 9);
    let _ = x.post(S_FD, 3, SEV_WARN, 10);
    let _ = l.register(cell(13, OP_MERGE, x.merge(3)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三源同值合并 + 两键建边 + 账目升序。
    let mut a = Linker::new();
    let p1 = a.post(S_MAPS, 1, SEV_INFO, 50);
    let p2 = a.post(S_FD, 1, SEV_WARN, 50);
    let p3 = a.post(S_STATUS, 1, SEV_ERR, 50);
    let p4 = a.post(S_MAPS, 2, SEV_INFO, 60);
    let m1 = a.merge(1);
    let m2 = a.merge(2);
    let e1 = a.link(1, 2);
    let acct = a.account();
    out[1] = p1 == Ok(1)
        && p2 == Ok(2)
        && p3 == Ok(3)
        && p4 == Ok(4)
        && m1 == Ok(3)
        && m2 == Ok(1)
        && e1 == Ok(1)
        && acct == Ok(2)
        && a.merged_at(0) == Some(Merged { key: 1, val: 50, mask: 0b1110, sev: SEV_ERR })
        && a.merged_at(1).map(|m| m.key) == Some(2)
        && a.merged_at(2).is_none()
        && a.edge_at(1).is_none()
        && a.conflict_count() == 0
        && a.consistent();

    // 二路：六类失败各自可辨。
    let mut b = Linker::new();
    let sev = b.post(S_MAPS, 1, 9, 1) == Err(DL_ENOSEV);
    let bad = b.post(0, 1, SEV_INFO, 1) == Err(DL_EINVAL);
    let zero = b.post(S_MAPS, 0, SEV_INFO, 1) == Err(DL_EINVAL);
    let _ = b.post(S_MAPS, 1, SEV_INFO, 1);
    let dup = b.post(S_MAPS, 1, SEV_WARN, 2) == Err(DL_EDUP);
    let none = b.merge(9) == Err(DL_ENOENT);
    let unmerged = Linker::new().account() == Err(DL_EUNMERGED);
    let link_unmerged = b.link(1, 2) == Err(DL_ENOENT);
    let _ = b.post(S_FD, 2, SEV_INFO, 3);
    let _ = b.merge(1);
    let _ = b.merge(2);
    let selfloop = b.link(1, 1) == Err(DL_EINVAL);
    let edge_dup = b.link(1, 2) == Ok(1) && b.link(1, 2) == Err(DL_EDUP);
    let cyc = b.link(2, 1) == Err(DL_ECYCLE);
    let mut cf = Linker::new();
    let _ = cf.post(S_MAPS, 3, SEV_INFO, 7);
    let _ = cf.post(S_FD, 3, SEV_WARN, 8);
    let conflict = cf.merge(3) == Err(DL_ECONFLICT) && cf.conflict_count() == 1 && cf.merged_count() == 0;
    out[2] = sev
        && bad
        && zero
        && dup
        && none
        && unmerged
        && link_unmerged
        && selfloop
        && edge_dup
        && cyc
        && conflict
        && b.consistent();

    // 三路：表满与重复合并。
    let mut q = Linker::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let src = ((i % 3) as u8) + 1;
        let _ = q.post(src, i, SEV_INFO, i);
        i += 1;
    }
    let q1 = q.post(S_MAPS, 99, SEV_INFO, 1) == Err(DL_EFULL);
    let m1 = q.merge(1) == Ok(1);
    let m2 = q.merge(1) == Err(DL_EDUP);
    let mut cap = Linker::new();
    let mut k = 1u32;
    while (k as usize) <= MCAP {
        let _ = cap.post(S_MAPS, k, SEV_INFO, k);
        let _ = cap.merge(k);
        k += 1;
    }
    let q2 = cap.account() == Ok(MCAP);
    let q3 = cap.merged_count() == MCAP;
    let mut big = Linker::new();
    let mut t = 1u32;
    while (t as usize) <= MCAP {
        let _ = big.post(S_MAPS, t, SEV_INFO, t);
        let _ = big.merge(t);
        t += 1;
    }
    let mut e = 0usize;
    while e < ECAP {
        let from = (e as u32) + 1;
        let to = from + 1;
        if to <= MCAP as u32 {
            let _ = big.link(from, to);
        } else {
            let _ = big.link(from, 1);
        }
        e += 1;
    }
    let q4 = big.edge_count() <= ECAP;
    out[3] = q1 && m1 && m2 && q2 && q3 && q4 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11357_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11357.procfs_diag_link");
    let l = build_ledger();

    s.add_dyn(
        "DL1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DL2-三源依次登记",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "DL3-同键两源合并",
        l.sample(3).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DL4-单源合并一条",
        l.sample(4).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DL5-建立第一条边",
        l.sample(5).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DL6-账目两条",
        l.sample(6).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DL7-初始无冲突",
        l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "DL8-零源被拒",
        l.sample(8).map(|x| x.code) == Some(DL_EINVAL),
        "static",
    );

    s.add(
        "DL9-重复登记被拒",
        l.sample(9).map(|x| x.code) == Some(DL_EDUP),
        "static",
    );

    s.add(
        "DL10-合并未知键不存在",
        l.sample(10).map(|x| x.code) == Some(DL_ENOENT),
        "static",
    );

    s.add(
        "DL11-登记表满被拒",
        l.sample(11).map(|x| x.code) == Some(DL_EFULL),
        "static",
    );

    s.add(
        "DL12-成环与取值冲突",
        {
            let mut c = Linker::new();
            let _ = c.post(S_MAPS, 1, SEV_INFO, 5);
            let _ = c.post(S_MAPS, 2, SEV_INFO, 6);
            let _ = c.merge(1);
            let _ = c.merge(2);
            let _ = c.link(1, 2);
            let cyc = c.link(2, 1) == Err(DL_ECYCLE) && c.edge_count() == 1;
            let _ = c.post(S_MAPS, 3, SEV_INFO, 7);
            let _ = c.post(S_FD, 3, SEV_WARN, 8);
            let con = c.merge(3) == Err(DL_ECONFLICT) && c.conflict_count() == 1;
            let ledger_cyc = l.sample(12).map(|x| x.code) == Some(DL_ECYCLE);
            let ledger_con = l.sample(13).map(|x| x.code) == Some(DL_ECONFLICT);
            cyc && con && ledger_cyc && ledger_con && c.consistent()
        },
        "static",
    );

    s.add(
        "DL13-冻结重放四注入",
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
        let s = run_unx11357_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn merge_requires_agreement() {
        let mut k = Linker::new();
        assert_eq!(k.post(S_MAPS, 1, SEV_INFO, 10), Ok(1));
        assert_eq!(k.post(S_FD, 1, SEV_WARN, 10), Ok(2));
        assert_eq!(k.merge(1), Ok(2));
        assert_eq!(k.merged_at(0).map(|m| m.sev), Some(SEV_WARN));
        assert_eq!(k.merged_at(0).map(|m| m.mask), Some(0b110));
        let mut c = Linker::new();
        assert_eq!(c.post(S_MAPS, 2, SEV_INFO, 1), Ok(1));
        assert_eq!(c.post(S_FD, 2, SEV_INFO, 2), Ok(2));
        assert_eq!(c.merge(2), Err(DL_ECONFLICT));
        assert_eq!(c.conflict_count(), 1);
        assert_eq!(c.merged_count(), 0);
    }

    #[test]
    fn merged_entries_stay_sorted() {
        let mut k = Linker::new();
        for key in [5u32, 1, 3] {
            let before = k.post_count();
            assert_eq!(k.post(S_MAPS, key, SEV_INFO, key), Ok(before + 1));
            assert_eq!(k.merge(key), Ok(1));
        }
        assert_eq!(k.account(), Ok(3));
        assert_eq!(k.merged_at(0).map(|m| m.key), Some(1));
        assert_eq!(k.merged_at(1).map(|m| m.key), Some(3));
        assert_eq!(k.merged_at(2).map(|m| m.key), Some(5));
        assert!(k.consistent());
    }

    #[test]
    fn cycles_are_refused() {
        let mut k = Linker::new();
        let _ = k.post(S_MAPS, 1, SEV_INFO, 1);
        let _ = k.post(S_MAPS, 2, SEV_INFO, 2);
        let _ = k.post(S_MAPS, 3, SEV_INFO, 3);
        let _ = k.merge(1);
        let _ = k.merge(2);
        let _ = k.merge(3);
        assert_eq!(k.link(1, 2), Ok(1));
        assert_eq!(k.link(2, 3), Ok(2));
        assert_eq!(k.link(3, 1), Err(DL_ECYCLE));
        assert_eq!(k.edge_count(), 2);
        assert_eq!(k.link(1, 3), Ok(3));
        assert!(k.consistent());
    }

    #[test]
    fn gate_order() {
        let mut k = Linker::new();
        assert_eq!(k.post(0, 1, SEV_INFO, 1), Err(DL_EINVAL));
        assert_eq!(k.post(S_MAPS, 0, SEV_INFO, 1), Err(DL_EINVAL));
        assert_eq!(k.post(S_MAPS, 1, 0, 1), Err(DL_ENOSEV));
        assert_eq!(k.post(S_MAPS, 1, 9, 1), Err(DL_ENOSEV));
        assert_eq!(k.post(S_MAPS, 1, SEV_INFO, 1), Ok(1));
        assert_eq!(k.post(S_MAPS, 1, 9, 1), Err(DL_ENOSEV));
        assert_eq!(k.merge(1), Ok(1));
        assert_eq!(k.merge(1), Err(DL_EDUP));
        assert_eq!(k.account(), Ok(1));
        assert_eq!(Linker::new().account(), Err(DL_EUNMERGED));
    }

    #[test]
    fn link_needs_merged_endpoints() {
        let mut k = Linker::new();
        let _ = k.post(S_MAPS, 1, SEV_INFO, 1);
        assert_eq!(k.link(1, 2), Err(DL_ENOENT));
        let _ = k.merge(1);
        assert_eq!(k.link(1, 2), Err(DL_ENOENT));
        assert_eq!(k.link(1, 1), Err(DL_EINVAL));
        assert_eq!(k.edge_count(), 0);
    }

    #[test]
    fn diag_capacity_is_exact() {
        let mut k = Linker::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            let src = ((i % 3) as u8) + 1;
            assert_eq!(k.post(src, i, SEV_INFO, i), Ok(i as usize));
            i += 1;
        }
        assert_eq!(k.post(S_MAPS, 99, SEV_INFO, 1), Err(DL_EFULL));
        assert!(k.consistent());
    }
}
