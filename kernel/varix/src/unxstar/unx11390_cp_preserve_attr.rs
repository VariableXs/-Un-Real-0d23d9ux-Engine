//! UNX-F11390 cp -p 属性保留语义
//!
//! `cp -p` 复制的不是裸字节，而是「字节 + 描述字节的四类元属性」：
//! 权限位（mode）、属主（uid:gid）、时间戳（atime/mtime）、扩展属性（xattr）。
//! 本模块把这个动作拆成一颗可对账的状态机：
//!
//! ```text
//! 观察 observe → 请求 want → 套用 apply（严格 K_MODE → K_OWNER → K_TIME → K_XATTR）
//!              → 收尾 finish（不允许缺口）→ 复核 verify（目标值须与源值逐项相等）
//! ```
//!
//! 观察与请求都只能针对真实存在的源属性：没有观察到的属性不可请求，
//! 没有请求的属性不可套用，跳过顺序的套用一律拒绝——这三道门把
//! 「先看、再要、再套、后验」的因果顺序钉死在实现里，而不是交给调用方自觉。
//!
//! ## 门序
//!
//! - `observe`：类型非法 [`PR_EINVAL`] → 重复观察 [`PR_EDUP`]。
//! - `want`：类型非法 [`PR_EINVAL`] → 源未观察 [`PR_EABSENT`] → 重复请求 [`PR_EDUP`]。
//! - `apply`：类型非法 [`PR_EINVAL`] → 未被请求 [`PR_ENOWANT`] → 乱序 [`PR_EORDER`]
//!   → 已全部套用 [`PR_EDUP`] → 无特权改属/改时 [`PR_EPRIV`]。
//! - `finish`：无任何请求 [`PR_EABSENT`] → 仍有缺口 [`PR_EGAP`]。
//! - `verify`：仍有缺口 [`PR_EGAP`] → 目标值与源值不符 [`PR_EMISMATCH`]。
//!
//! 属主与时间戳属于特权属性：未授予特权时套用被拒且不产生任何副作用。
//! 目标侧可以被外部动作漂移（`drift`），`verify` 的职责正是把这种漂移
//! 从「看起来复制成功」里揪出来。
//!
//! ## 账本
//!
//! 十四格：观察三次 → 请求两次 → 套用两次 → 收尾一次（八允），
//! 另记六拒：重复观察、类型非法、未请求套用、乱序套用、无特权改属、缺口收尾。
//! 账本自带五类自检：形状 [`PR_EBAL`]、槽序 [`PR_ESLOT`]、值真 [`PR_EVALUE`]、
//! 重放 [`PR_ESEAL`]、观测缺失 [`PR_ENOBS`]。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 错误段基址（本模块独占 0xee60..=0xee6d）。
pub const SEG_BASE: u32 = 0xee60;
/// 属性类型非法。
pub const PR_EINVAL: u32 = SEG_BASE + 0x01;
/// 动作重复。
pub const PR_EDUP: u32 = SEG_BASE + 0x02;
/// 源属性缺失（或目标尚无值）。
pub const PR_EABSENT: u32 = SEG_BASE + 0x03;
/// 属性未被请求。
pub const PR_ENOWANT: u32 = SEG_BASE + 0x04;
/// 套用乱序。
pub const PR_EORDER: u32 = SEG_BASE + 0x05;
/// 无特权。
pub const PR_EPRIV: u32 = SEG_BASE + 0x06;
/// 目标值与源值不符。
pub const PR_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 缺口未闭环。
pub const PR_EGAP: u32 = SEG_BASE + 0x08;
/// 账目形状失真。
pub const PR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽序错乱。
pub const PR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 记录值失真。
pub const PR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 重放失配。
pub const PR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const PR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 属性类别数。
pub const CAP: usize = 4;

/// 属性类别：权限位。
pub const K_MODE: u8 = 1;
/// 属性类别：属主。
pub const K_OWNER: u8 = 2;
/// 属性类别：时间戳。
pub const K_TIME: u8 = 3;
/// 属性类别：扩展属性。
pub const K_XATTR: u8 = 4;

/// 操作码：观察。
pub const OP_OBS: u8 = 1;
/// 操作码：请求。
pub const OP_WANT: u8 = 2;
/// 操作码：套用。
pub const OP_APPLY: u8 = 3;
/// 操作码：收尾。
pub const OP_FINISH: u8 = 4;
/// 操作码：复核。
pub const OP_VERIFY: u8 = 5;
/// 操作码：状态读取。
pub const OP_STATE: u8 = 6;

/// 账本格数：八允六拒。
pub const SAMPLES: usize = 14;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// FNV-1a 折叠：把 `v` 的八个字节逐个混入 `h`。
fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut b = 0u32;
    while b < 8 {
        let byte = (v >> (b * 8)) & 0xff;
        x ^= byte;
        x = x.wrapping_mul(FNV_PRIME);
        b += 1;
    }
    x
}

/// 类别 → 槽位下标；非法类别返回 `None`。
fn kind_index(kind: u8) -> Option<usize> {
    if kind >= K_MODE && kind <= K_XATTR {
        Some((kind - K_MODE) as usize)
    } else {
        None
    }
}

/// 一次 `cp -p` 的属性保留计划。
///
/// 四个槽位分别对应 [`K_MODE`]、[`K_OWNER`]、[`K_TIME`]、[`K_XATTR`]；
/// 每个槽位三态：源值（`src/have`）、请求（`want`）、已套用（`dst/done`）。
pub struct Plan {
    src: [u32; CAP],
    dst: [u32; CAP],
    have: [bool; CAP],
    want: [bool; CAP],
    done: [bool; CAP],
    seq: [u8; CAP],
    n_seq: usize,
    observed: usize,
    wanted: usize,
    priv_ok: bool,
    rejects: u32,
}

impl Plan {
    /// 空计划：四槽全空、无特权。
    pub const fn new() -> Plan {
        Plan {
            src: [0; CAP],
            dst: [0; CAP],
            have: [false; CAP],
            want: [false; CAP],
            done: [false; CAP],
            seq: [0; CAP],
            n_seq: 0,
            observed: 0,
            wanted: 0,
            priv_ok: false,
            rejects: 0,
        }
    }

    /// 是否已观察第 `i` 类。
    fn have_flag(&self, i: usize) -> bool {
        match self.have.get(i) {
            Some(v) => *v,
            None => false,
        }
    }

    /// 是否已请求第 `i` 类。
    fn want_flag(&self, i: usize) -> bool {
        match self.want.get(i) {
            Some(v) => *v,
            None => false,
        }
    }

    /// 是否已套用第 `i` 类。
    fn done_flag(&self, i: usize) -> bool {
        match self.done.get(i) {
            Some(v) => *v,
            None => false,
        }
    }

    /// 授予或收回特权（改属/改时必须具备）。
    pub fn set_priv(&mut self, on: bool) {
        self.priv_ok = on;
    }

    /// 观察源属性：类型非法 → [`PR_EINVAL`]，重复观察 → [`PR_EDUP`]。
    ///
    /// 返回已观察类别数。
    pub fn observe(&mut self, kind: u8, value: u32) -> Result<usize, u32> {
        let i = match kind_index(kind) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(PR_EINVAL);
            }
        };
        if self.have_flag(i) {
            self.rejects += 1;
            return Err(PR_EDUP);
        }
        if let Some(s) = self.src.get_mut(i) {
            *s = value;
        }
        if let Some(f) = self.have.get_mut(i) {
            *f = true;
        }
        self.observed += 1;
        Ok(self.observed)
    }

    /// 请求保留某类属性：类型非法 → [`PR_EINVAL`]，源未观察 → [`PR_EABSENT`]，
    /// 重复请求 → [`PR_EDUP`]。返回已请求类别数。
    pub fn want(&mut self, kind: u8) -> Result<usize, u32> {
        let i = match kind_index(kind) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(PR_EINVAL);
            }
        };
        if !self.have_flag(i) {
            self.rejects += 1;
            return Err(PR_EABSENT);
        }
        if self.want_flag(i) {
            self.rejects += 1;
            return Err(PR_EDUP);
        }
        if let Some(f) = self.want.get_mut(i) {
            *f = true;
        }
        self.wanted += 1;
        Ok(self.wanted)
    }

    /// 下一个应套用的类别（最小的已请求未套用类别）。
    pub fn next_kind(&self) -> Option<u8> {
        let mut i = 0usize;
        while i < CAP {
            if self.want_flag(i) && !self.done_flag(i) {
                return Some((i as u8) + K_MODE);
            }
            i += 1;
        }
        None
    }

    /// 套用某类属性。门序：类型 → 请求 → 顺序 → 完成 → 特权。
    ///
    /// 属主与时间戳需要特权；被拒时不产生任何副作用。返回已套用序号。
    pub fn apply(&mut self, kind: u8) -> Result<usize, u32> {
        let i = match kind_index(kind) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(PR_EINVAL);
            }
        };
        if !self.want_flag(i) {
            self.rejects += 1;
            return Err(PR_ENOWANT);
        }
        match self.next_kind() {
            Some(nk) => {
                if nk != kind {
                    self.rejects += 1;
                    return Err(PR_EORDER);
                }
            }
            None => {
                self.rejects += 1;
                return Err(PR_EDUP);
            }
        }
        if (kind == K_OWNER || kind == K_TIME) && !self.priv_ok {
            self.rejects += 1;
            return Err(PR_EPRIV);
        }
        let v = match self.src.get(i) {
            Some(x) => *x,
            None => 0,
        };
        if let Some(d) = self.dst.get_mut(i) {
            *d = v;
        }
        if let Some(f) = self.done.get_mut(i) {
            *f = true;
        }
        if let Some(sl) = self.seq.get_mut(self.n_seq) {
            *sl = kind;
        }
        self.n_seq += 1;
        Ok(self.n_seq)
    }

    /// 收尾：无任何请求 → [`PR_EABSENT`]；仍有缺口 → [`PR_EGAP`]。
    /// 返回已套用的请求数。
    pub fn finish(&self) -> Result<usize, u32> {
        if self.wanted == 0 {
            return Err(PR_EABSENT);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CAP {
            if self.want_flag(i) {
                if self.done_flag(i) {
                    n += 1;
                } else {
                    return Err(PR_EGAP);
                }
            }
            i += 1;
        }
        Ok(n)
    }

    /// 复核：先要求无缺口，再逐项比对目标值与源值。
    pub fn verify(&self) -> Result<usize, u32> {
        let n = self.finish()?;
        let mut i = 0usize;
        while i < CAP {
            if self.want_flag(i) {
                let a = match self.src.get(i) {
                    Some(v) => *v,
                    None => 0,
                };
                let b = match self.dst.get(i) {
                    Some(v) => *v,
                    None => 0,
                };
                if a != b {
                    return Err(PR_EMISMATCH);
                }
            }
            i += 1;
        }
        Ok(n)
    }

    /// 目标侧漂移：模拟复制之后目标属性被外部改动。
    ///
    /// 尚未套用的类别无值可漂 → [`PR_EABSENT`]。
    pub fn drift(&mut self, kind: u8, value: u32) -> Result<u32, u32> {
        let i = match kind_index(kind) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(PR_EINVAL);
            }
        };
        if !self.done_flag(i) {
            self.rejects += 1;
            return Err(PR_EABSENT);
        }
        if let Some(d) = self.dst.get_mut(i) {
            *d = value;
        }
        Ok(value)
    }

    /// 目标侧当前值；尚未套用 → [`PR_EABSENT`]。
    pub fn state(&self, kind: u8) -> Result<u32, u32> {
        let i = match kind_index(kind) {
            Some(v) => v,
            None => return Err(PR_EINVAL),
        };
        if !self.done_flag(i) {
            return Err(PR_EABSENT);
        }
        match self.dst.get(i) {
            Some(v) => Ok(*v),
            None => Err(PR_EABSENT),
        }
    }

    /// 源侧观测值；尚未观察 → [`PR_EABSENT`]。
    pub fn source(&self, kind: u8) -> Result<u32, u32> {
        let i = match kind_index(kind) {
            Some(v) => v,
            None => return Err(PR_EINVAL),
        };
        if !self.have_flag(i) {
            return Err(PR_EABSENT);
        }
        match self.src.get(i) {
            Some(v) => Ok(*v),
            None => Err(PR_EABSENT),
        }
    }

    /// 已观察类别数。
    pub fn observed(&self) -> usize {
        self.observed
    }

    /// 已请求类别数。
    pub fn wanted(&self) -> usize {
        self.wanted
    }

    /// 已套用类别数。
    pub fn applied(&self) -> usize {
        self.n_seq
    }

    /// 第 `i` 个套用的类别。
    pub fn seq_at(&self, i: usize) -> Option<u8> {
        if i >= self.n_seq {
            return None;
        }
        self.seq.get(i).copied()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 不变式自检：已套用 ⊆ 已请求 ⊆ 已观察，且序号不超过请求数。
    pub fn consistent(&self) -> bool {
        let mut i = 0usize;
        while i < CAP {
            if self.want_flag(i) && !self.have_flag(i) {
                return false;
            }
            if self.done_flag(i) && !self.want_flag(i) {
                return false;
            }
            i += 1;
        }
        self.n_seq <= self.wanted && self.wanted <= self.observed
    }
}

impl Default for Plan {
    fn default() -> Plan {
        Plan::new()
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
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: 0,
            value: v as u32,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: 0,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
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
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格；槽位必须严格递增且不越界。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(PR_ESLOT);
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

    /// 第 `i` 格。
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

    /// 对账：形状、槽序、逐格值与错误码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(PR_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1, 2, 1, 2, 2];
        const CODES: [u32; 6] = [
            PR_EDUP, PR_EINVAL, PR_ENOWANT, PR_EORDER, PR_EPRIV, PR_EGAP,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PR_EBAL),
            };
            if s.slot as usize != i {
                return Err(PR_ESLOT);
            }
            if s.op == 0 {
                return Err(PR_ENOBS);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PR_EVALUE);
                        }
                    }
                    None => return Err(PR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PR_EVALUE);
                        }
                    }
                    None => return Err(PR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PR_EBAL);
        }
        Ok(())
    }

    /// 指纹：逐格折叠槽位、操作码、成败、错误码与值，并混入拒绝计数。
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

    /// 冻结：记录指纹，此后再登记一律被拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(PR_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建账本，指纹与当前账本一致才算通过。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(PR_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

impl Default for Ledger {
    fn default() -> Ledger {
        Ledger::new()
    }
}

/// 十四格账本：观察三次 → 请求两次 → 套用两次 → 收尾一次，
/// 再记六拒：重复观察、类型非法、未请求套用、乱序套用、无特权改属、缺口收尾。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut p = Plan::new();
    p.set_priv(true);

    let _ = l.register(cell(0, OP_OBS, p.observe(K_MODE, 0o644)));
    let _ = l.register(cell(1, OP_OBS, p.observe(K_OWNER, 1000)));
    let _ = l.register(cell(2, OP_OBS, p.observe(K_TIME, 1_700_000_000)));
    let _ = l.register(cell(3, OP_WANT, p.want(K_MODE)));
    let _ = l.register(cell(4, OP_WANT, p.want(K_OWNER)));
    let _ = l.register(cell(5, OP_APPLY, p.apply(K_MODE)));
    let _ = l.register(cell(6, OP_APPLY, p.apply(K_OWNER)));
    let _ = l.register(cell(7, OP_FINISH, p.finish()));

    let _ = l.register(cell(8, OP_OBS, p.observe(K_MODE, 0o644)));
    let _ = l.register(cell(9, OP_OBS, p.observe(9, 0)));
    let _ = l.register(cell(10, OP_APPLY, p.apply(K_XATTR)));

    let mut o = Plan::new();
    let _ = o.observe(K_MODE, 0o600);
    let _ = o.observe(K_OWNER, 7);
    let _ = o.want(K_MODE);
    let _ = o.want(K_OWNER);
    let _ = l.register(cell(11, OP_APPLY, o.apply(K_OWNER)));

    let mut u = Plan::new();
    let _ = u.observe(K_OWNER, 1000);
    let _ = u.want(K_OWNER);
    let _ = l.register(cell(12, OP_APPLY, u.apply(K_OWNER)));

    let mut g = Plan::new();
    let _ = g.observe(K_MODE, 0o644);
    let _ = g.observe(K_OWNER, 1000);
    let _ = g.want(K_MODE);
    let _ = g.want(K_OWNER);
    let _ = g.apply(K_MODE);
    let _ = l.register(cell(13, OP_FINISH, g.finish()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：四属性全链——观察、请求、按序套用、收尾、复核。
    let mut p = Plan::new();
    p.set_priv(true);
    let a1 = p.observe(K_MODE, 0o755);
    let a2 = p.observe(K_OWNER, 1001);
    let a3 = p.observe(K_TIME, 1_700_000_777);
    let a4 = p.observe(K_XATTR, 0x5a);
    let w1 = p.want(K_MODE);
    let w2 = p.want(K_OWNER);
    let w3 = p.want(K_TIME);
    let w4 = p.want(K_XATTR);
    let s1 = p.apply(K_MODE);
    let s2 = p.apply(K_OWNER);
    let s3 = p.apply(K_TIME);
    let s4 = p.apply(K_XATTR);
    out[1] = a1 == Ok(1)
        && a2 == Ok(2)
        && a3 == Ok(3)
        && a4 == Ok(4)
        && w1 == Ok(1)
        && w2 == Ok(2)
        && w3 == Ok(3)
        && w4 == Ok(4)
        && s1 == Ok(1)
        && s2 == Ok(2)
        && s3 == Ok(3)
        && s4 == Ok(4)
        && p.finish() == Ok(4)
        && p.verify() == Ok(4)
        && p.state(K_MODE) == Ok(0o755)
        && p.state(K_XATTR) == Ok(0x5a)
        && p.seq_at(0) == Some(K_MODE)
        && p.seq_at(3) == Some(K_XATTR)
        && p.next_kind().is_none()
        && p.consistent();

    // 二路：各类拒绝各自可辨。
    let mut b = Plan::new();
    let bad_kind = b.observe(0, 1) == Err(PR_EINVAL) && b.observe(5, 1) == Err(PR_EINVAL);
    let absent = b.want(K_MODE) == Err(PR_EABSENT);
    let obs1 = b.observe(K_MODE, 0o644) == Ok(1);
    let dup_obs = b.observe(K_MODE, 0o644) == Err(PR_EDUP);
    let not_wanted = b.apply(K_MODE) == Err(PR_ENOWANT);
    let want1 = b.want(K_MODE) == Ok(1);
    let dup_want = b.want(K_MODE) == Err(PR_EDUP);
    let apply1 = b.apply(K_MODE) == Ok(1);
    let over_apply = b.apply(K_MODE) == Err(PR_EDUP);
    let _ = b.observe(K_OWNER, 0);
    let _ = b.want(K_OWNER);
    let priv_gate = b.apply(K_OWNER) == Err(PR_EPRIV);
    let drift_ok = b.drift(K_MODE, 0o600) == Ok(0o600);
    out[2] = bad_kind
        && absent
        && obs1
        && dup_obs
        && not_wanted
        && want1
        && dup_want
        && apply1
        && over_apply
        && priv_gate
        && drift_ok
        && b.consistent();

    // 三路：缺口未闭环、目标漂移可辨、冻结重放同指纹。
    let mut c = Plan::new();
    c.set_priv(true);
    let _ = c.observe(K_MODE, 0o644);
    let _ = c.observe(K_TIME, 9);
    let _ = c.want(K_MODE);
    let _ = c.want(K_TIME);
    let _ = c.apply(K_MODE);
    let gap = c.verify() == Err(PR_EGAP);
    let unobserved_state = c.state(K_TIME) == Err(PR_EABSENT);
    let _ = c.apply(K_TIME);
    let drift = c.drift(K_TIME, 10) == Ok(10) && c.verify() == Err(PR_EMISMATCH);
    let mut fin = build_ledger();
    let seal0 = fin.seal();
    let frozen = fin.freeze();
    out[3] = gap
        && unobserved_state
        && drift
        && frozen == Ok(seal0)
        && fin.replay() == Ok(seal0)
        && fin.freeze() == Err(PR_EBAL)
        && fin.is_frozen()
        && fin.root() == seal0;

    out
}

/// PR1..PR13 十三条判据。
pub fn run_unx11390_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11390");
    let l = build_ledger();
    let inj = inject_four();

    s.add_dyn(
        "PR1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PR2-重复观察被拒",
        l.sample(8).map(|x| x.code) == Some(PR_EDUP),
        "static",
    );

    s.add(
        "PR3-类型非法被拒",
        l.sample(9).map(|x| x.code) == Some(PR_EINVAL),
        "static",
    );

    s.add(
        "PR4-未请求不可套用",
        l.sample(10).map(|x| x.code) == Some(PR_ENOWANT),
        "static",
    );

    s.add(
        "PR5-乱序套用被拒",
        l.sample(11).map(|x| x.code) == Some(PR_EORDER),
        "static",
    );

    s.add(
        "PR6-无特权拒改属",
        l.sample(12).map(|x| x.code) == Some(PR_EPRIV),
        "static",
    );

    s.add(
        "PR7-缺口收尾被拒",
        l.sample(13).map(|x| x.code) == Some(PR_EGAP),
        "static",
    );

    s.add(
        "PR8-四属性全链闭环",
        {
            let mut p = Plan::new();
            p.set_priv(true);
            p.observe(K_MODE, 0o755) == Ok(1)
                && p.observe(K_OWNER, 1001) == Ok(2)
                && p.observe(K_TIME, 11) == Ok(3)
                && p.observe(K_XATTR, 12) == Ok(4)
                && p.want(K_MODE) == Ok(1)
                && p.want(K_OWNER) == Ok(2)
                && p.want(K_TIME) == Ok(3)
                && p.want(K_XATTR) == Ok(4)
                && p.apply(K_MODE) == Ok(1)
                && p.apply(K_OWNER) == Ok(2)
                && p.apply(K_TIME) == Ok(3)
                && p.apply(K_XATTR) == Ok(4)
                && p.finish() == Ok(4)
                && p.verify() == Ok(4)
                && p.state(K_OWNER) == Ok(1001)
                && p.applied() == 4
                && p.consistent()
        },
        "static",
    );

    s.add(
        "PR9-源缺失不可请求",
        {
            let mut p = Plan::new();
            p.want(K_MODE) == Err(PR_EABSENT)
                && p.source(K_MODE) == Err(PR_EABSENT)
                && p.observe(K_MODE, 0o600) == Ok(1)
                && p.source(K_MODE) == Ok(0o600)
                && p.want(K_MODE) == Ok(1)
                && p.rejects() == 1
        },
        "static",
    );

    s.add(
        "PR10-目标漂移可辨",
        {
            let mut p = Plan::new();
            p.set_priv(true);
            let _ = p.observe(K_MODE, 0o640);
            let _ = p.want(K_MODE);
            let _ = p.apply(K_MODE);
            p.verify() == Ok(1)
                && p.drift(K_MODE, 0o600) == Ok(0o600)
                && p.verify() == Err(PR_EMISMATCH)
                && p.drift(K_MODE, 0o640) == Ok(0o640)
                && p.verify() == Ok(1)
        },
        "static",
    );

    s.add(
        "PR11-未套用无观测",
        {
            let mut p = Plan::new();
            let _ = p.observe(K_TIME, 5);
            let _ = p.want(K_TIME);
            p.state(K_TIME) == Err(PR_EABSENT)
                && p.drift(K_TIME, 6) == Err(PR_EABSENT)
                && p.next_kind() == Some(K_TIME)
                && p.applied() == 0
                && p.seq_at(0).is_none()
        },
        "static",
    );

    s.add(
        "PR12-冻结重放同指纹",
        {
            let mut q = build_ledger();
            let seal = q.seal();
            q.freeze() == Ok(seal)
                && q.root() == seal
                && q.is_frozen()
                && q.replay() == Ok(seal)
                && q.register(Sample::blank()) == Err(PR_ESLOT)
                && q.rejects() >= 1
        },
        "static",
    );

    s.add(
        "PR13-冻结重放四注入",
        inj[0] && inj[1] && inj[2] && inj[3] && l.seal() != 0,
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
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        assert!(!l.is_empty());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11390_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn full_preserve_chain() {
        let mut p = Plan::new();
        p.set_priv(true);
        assert_eq!(p.observe(K_MODE, 0o644), Ok(1));
        assert_eq!(p.observe(K_OWNER, 1000), Ok(2));
        assert_eq!(p.observe(K_TIME, 42), Ok(3));
        assert_eq!(p.observe(K_XATTR, 7), Ok(4));
        assert_eq!(p.want(K_MODE), Ok(1));
        assert_eq!(p.want(K_OWNER), Ok(2));
        assert_eq!(p.want(K_TIME), Ok(3));
        assert_eq!(p.want(K_XATTR), Ok(4));
        assert_eq!(p.next_kind(), Some(K_MODE));
        assert_eq!(p.apply(K_MODE), Ok(1));
        assert_eq!(p.next_kind(), Some(K_OWNER));
        assert_eq!(p.apply(K_OWNER), Ok(2));
        assert_eq!(p.apply(K_TIME), Ok(3));
        assert_eq!(p.apply(K_XATTR), Ok(4));
        assert_eq!(p.next_kind(), None);
        assert_eq!(p.finish(), Ok(4));
        assert_eq!(p.verify(), Ok(4));
        assert_eq!(p.state(K_XATTR), Ok(7));
        assert_eq!(p.source(K_OWNER), Ok(1000));
        assert_eq!(p.seq_at(2), Some(K_TIME));
        assert!(p.consistent());
    }

    #[test]
    fn duplicate_guards() {
        let mut p = Plan::new();
        assert_eq!(p.observe(K_MODE, 1), Ok(1));
        assert_eq!(p.observe(K_MODE, 1), Err(PR_EDUP));
        assert_eq!(p.observe(0, 1), Err(PR_EINVAL));
        assert_eq!(p.want(K_MODE), Ok(1));
        assert_eq!(p.want(K_MODE), Err(PR_EDUP));
        assert_eq!(p.apply(K_MODE), Ok(1));
        assert_eq!(p.apply(K_MODE), Err(PR_EDUP));
        assert_eq!(p.finish(), Ok(1));
        assert_eq!(p.rejects(), 4);
    }

    #[test]
    fn order_and_priv() {
        let mut p = Plan::new();
        assert_eq!(p.observe(K_MODE, 0o600), Ok(1));
        assert_eq!(p.observe(K_OWNER, 5), Ok(2));
        assert_eq!(p.want(K_MODE), Ok(1));
        assert_eq!(p.want(K_OWNER), Ok(2));
        assert_eq!(p.apply(K_OWNER), Err(PR_EORDER));
        assert_eq!(p.apply(K_MODE), Ok(1));
        assert_eq!(p.apply(K_OWNER), Err(PR_EPRIV));
        assert_eq!(p.applied(), 1);
        p.set_priv(true);
        assert_eq!(p.apply(K_OWNER), Ok(2));
        assert_eq!(p.verify(), Ok(2));
    }

    #[test]
    fn drift_detected() {
        let mut p = Plan::new();
        p.set_priv(true);
        assert_eq!(p.observe(K_TIME, 100), Ok(1));
        assert_eq!(p.want(K_TIME), Ok(1));
        assert_eq!(p.apply(K_TIME), Ok(1));
        assert_eq!(p.verify(), Ok(1));
        assert_eq!(p.drift(K_TIME, 101), Ok(101));
        assert_eq!(p.verify(), Err(PR_EMISMATCH));
        assert_eq!(p.state(K_TIME), Ok(101));
        assert_eq!(p.drift(9, 0), Err(PR_EINVAL));
    }

    #[test]
    fn gap_blocks_verify() {
        let mut p = Plan::new();
        assert_eq!(p.finish(), Err(PR_EABSENT));
        assert_eq!(p.observe(K_MODE, 1), Ok(1));
        assert_eq!(p.observe(K_XATTR, 2), Ok(2));
        assert_eq!(p.want(K_MODE), Ok(1));
        assert_eq!(p.want(K_XATTR), Ok(2));
        assert_eq!(p.apply(K_MODE), Ok(1));
        assert_eq!(p.finish(), Err(PR_EGAP));
        assert_eq!(p.verify(), Err(PR_EGAP));
        assert_eq!(p.want(K_TIME), Err(PR_EABSENT));
        assert_eq!(p.apply(K_XATTR), Ok(2));
        assert_eq!(p.finish(), Ok(2));
        assert!(p.consistent());
    }

    #[test]
    fn inject_four_holds() {
        let out = inject_four();
        assert!(out[0]);
        assert!(out[1]);
        assert!(out[2]);
        assert!(out[3]);
    }
}
