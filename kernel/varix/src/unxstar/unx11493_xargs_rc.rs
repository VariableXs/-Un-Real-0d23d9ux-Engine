//! UNX-F11493 xargs 退出码与缺命令语义（0x20ea0 段）
//!
//! xargs 的退出码不是子进程退出码的直传，而是一次归并：子进程成功记 0；任一次
//! 调用以 1..125 退出记 123；子进程以 255 退出记 124；被信号杀死记 125；命令
//! 本身无法执行记 126（如 ENOEXEC），找不到命令记 127；xargs 自己的用法错误
//! 记 1。归并的优先级固定为 124 > 125 > 123 > 0——一个 255 不会被后一次成功
//! 覆盖，这是脚本 `set -e` 能依赖它的原因。
//!
//! 缺命令时 xargs 默认执行 `echo`，所以“没有命令”不是错误；但显式给出空串命令
//! 是调用方违约，返回 ENOCMD。子状态只接受 0..255，超过是 ECODE；信号与零状态
//! 同时出现是自相矛盾的记录，EINVAL。归并列表为空 EEMPTY，超过十六个子状态
//! ECAP，未知的启动失败类别 EKIND。
//!
//! 账本十四格：八格成功覆盖四种归类、三种归并结果与“找不到命令”；六格拒绝
//! 依次是零状态带信号、子状态过多、空命令、状态越界、未知类别与空归并。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ea0;

/// 编号非法。
pub const XR_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const XR_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const XR_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const XR_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const XR_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const XR_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const XR_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const XR_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const XR_ESEAL: u32 = SEG_BASE + 0x09;
/// 记录自相矛盾。
pub const XR_EINVAL: u32 = SEG_BASE + 0x0a;
/// 子状态过多。
pub const XR_ECAP: u32 = SEG_BASE + 0x0b;
/// 命令为空。
pub const XR_ENOCMD: u32 = SEG_BASE + 0x0c;
/// 子状态越界。
pub const XR_ECODE: u32 = SEG_BASE + 0x0d;
/// 启动失败类别未知。
pub const XR_EKIND: u32 = SEG_BASE + 0x0e;
/// 空归并。
pub const XR_EEMPTY: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：子状态归类。
pub const OP_CLASS: u8 = 1;
/// 操作：归并。
pub const OP_AGG: u8 = 2;
/// 操作：命令解析。
pub const OP_RESOLVE: u8 = 3;
/// 操作：启动失败。
pub const OP_SPAWN: u8 = 4;

/// 退出码：成功。
pub const RC_OK: u32 = 0;
/// 退出码：xargs 自身错误。
pub const RC_ERR: u32 = 1;
/// 退出码：有子调用非零。
pub const RC_ANY: u32 = 123;
/// 退出码：子进程返回 255。
pub const RC_255: u32 = 124;
/// 退出码：子进程被信号杀死。
pub const RC_SIG: u32 = 125;
/// 退出码：命令无法执行。
pub const RC_CANNOT: u32 = 126;
/// 退出码：命令不存在。
pub const RC_NOENT: u32 = 127;

/// 子状态上限。
pub const MAX_CHILDREN: usize = 16;
/// 样本格数。
pub const SAMPLES: usize = 14;

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

/// 子状态归类：成功/1..125/255/信号。
pub fn classify(code: u8, signaled: bool) -> Result<u32, u32> {
    if signaled {
        if code == 0 {
            return Err(XR_EINVAL);
        }
        return Ok(RC_SIG);
    }
    if code == 0 {
        Ok(RC_OK)
    } else if code == 255 {
        Ok(RC_255)
    } else {
        Ok(RC_ANY)
    }
}

fn rank(c: u32) -> u8 {
    if c == RC_255 {
        3
    } else if c == RC_SIG {
        2
    } else if c == RC_ANY {
        1
    } else {
        0
    }
}

/// 归并：优先级 124 > 125 > 123 > 0。
pub fn aggregate(classed: &[u32]) -> Result<u32, u32> {
    if classed.is_empty() {
        return Err(XR_EEMPTY);
    }
    if classed.len() > MAX_CHILDREN {
        return Err(XR_ECAP);
    }
    let mut best = RC_OK;
    let mut best_rank = 0u8;
    let mut i = 0usize;
    while i < classed.len() {
        let c = match classed.get(i) {
            Some(&v) => v,
            None => return Err(XR_EEMPTY),
        };
        let rk = rank(c);
        if rk > best_rank {
            best_rank = rk;
            best = c;
        }
        i += 1;
    }
    Ok(best)
}

/// 命令解析：缺省 echo，显式空串拒绝。
pub fn resolve<'a>(cmd: Option<&'a [u8]>) -> Result<&'a [u8], u32> {
    match cmd {
        None => Ok(b"echo"),
        Some(c) => {
            if c.is_empty() {
                Err(XR_ENOCMD)
            } else {
                Ok(c)
            }
        }
    }
}

/// 启动失败：1 找不到，2 无法执行。
pub fn spawn_err(kind: u8) -> Result<u32, u32> {
    match kind {
        1 => Ok(RC_NOENT),
        2 => Ok(RC_CANNOT),
        _ => Err(XR_EKIND),
    }
}

/// 子状态收窄。
pub fn status_low(code: u32) -> Result<u8, u32> {
    if code > 255 {
        Err(XR_ECODE)
    } else {
        Ok(code as u8)
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
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

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    cap: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap: SAMPLES,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 编号 → 重复 → 顺序 → 容量。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(XR_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(XR_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(XR_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(XR_EORDER);
        }
        if self.len >= self.cap {
            return Err(XR_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(XR_EID),
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

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功计数。
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

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(XR_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(XR_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(XR_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(XR_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(XR_EMISMATCH);
                    }
                }
                None => return Err(XR_ENOBS),
            }
            i += 1;
        }
        let vals = [0u32, 123, 124, 125, 0, 123, 124, 127];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(XR_EMISMATCH);
                    }
                }
                _ => return Err(XR_EBAL),
            }
            k += 1;
        }
        let codes = [XR_EINVAL, XR_ECAP, XR_ENOCMD, XR_ECODE, XR_EKIND, XR_EEMPTY];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(XR_EMISMATCH);
                        }
                    }
                    None => return Err(XR_EBAL),
                },
                None => return Err(XR_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(XR_EBAL);
        }
        Ok(())
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
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(XR_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(XR_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(XR_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
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

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(cell(0, OP_CLASS, classify(0, false)));
    let _ = l.register(cell(1, OP_CLASS, classify(4, false)));
    let _ = l.register(cell(2, OP_CLASS, classify(255, false)));
    let _ = l.register(cell(3, OP_CLASS, classify(9, true)));
    let _ = l.register(cell(4, OP_AGG, aggregate(&[0, 0, 0])));
    let _ = l.register(cell(5, OP_AGG, aggregate(&[0, 123, 0])));
    let _ = l.register(cell(6, OP_AGG, aggregate(&[0, 124, 123])));
    let _ = l.register(cell(7, OP_SPAWN, spawn_err(1)));

    let _ = l.register(cell(8, OP_CLASS, classify(0, true)));
    let many = [0u32; 17];
    let _ = l.register(cell(9, OP_AGG, aggregate(&many)));
    let _ = l.register(cell(10, OP_RESOLVE, resolve(Some(b"")).map(|c| c.len() as u32)));
    let _ = l.register(cell(11, OP_SPAWN, status_low(300).map(u32::from)));
    let _ = l.register(cell(12, OP_SPAWN, spawn_err(3)));
    let _ = l.register(cell(13, OP_AGG, aggregate(&[])));

    l
}

/// 四路注入：账本自洽、归类、归并、拒绝后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(XR_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    out[1] = classify(0, false) == Ok(RC_OK)
        && classify(1, false) == Ok(RC_ANY)
        && classify(255, false) == Ok(RC_255)
        && classify(9, true) == Ok(RC_SIG)
        && classify(126, false) == Ok(RC_ANY);

    out[2] = aggregate(&[0, RC_ANY, 0]) == Ok(RC_ANY)
        && aggregate(&[RC_ANY, RC_255, 0]) == Ok(RC_255)
        && aggregate(&[0, RC_SIG, RC_ANY]) == Ok(RC_SIG)
        && aggregate(&[0, 0, 0]) == Ok(RC_OK);

    let default_cmd = resolve(None) == Ok(&b"echo"[..]);
    let empty = resolve(Some(b"")) == Err(XR_ENOCMD);
    let missing = spawn_err(1) == Ok(RC_NOENT);
    let recovered = status_low(255) == Ok(255) && status_low(0) == Ok(0);
    out[3] = default_cmd && empty && missing && recovered;
    out
}

/// 十三判据。
pub fn run_unx11493_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11493.xargsrc");
    let l = build_ledger();

    s.add(
        "XR1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_CLASS),
        "static",
    );
    s.add_dyn(
        "XR2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "XR3-成功与非零",
        classify(0, false) == Ok(RC_OK)
            && classify(1, false) == Ok(RC_ANY)
            && classify(125, false) == Ok(RC_ANY)
            && l.sample(1).map(|x| x.value) == Some(RC_ANY),
        "static",
    );
    s.add(
        "XR4-255 特例",
        classify(255, false) == Ok(RC_255)
            && l.sample(2).map(|x| x.value) == Some(RC_255),
        "static",
    );
    s.add(
        "XR5-信号杀",
        classify(9, true) == Ok(RC_SIG)
            && classify(1, true) == Ok(RC_SIG)
            && l.sample(3).map(|x| x.value) == Some(RC_SIG),
        "static",
    );
    s.add(
        "XR6-归并优先级",
        aggregate(&[0, 0, 0]) == Ok(RC_OK)
            && aggregate(&[0, RC_ANY, 0]) == Ok(RC_ANY)
            && aggregate(&[RC_ANY, RC_255, 0]) == Ok(RC_255)
            && aggregate(&[RC_255, RC_SIG, RC_ANY]) == Ok(RC_255)
            && l.sample(6).map(|x| x.value) == Some(RC_255),
        "static",
    );
    s.add(
        "XR7-缺命令默认 echo",
        resolve(None) == Ok(&b"echo"[..])
            && resolve(Some(b"cat")) == Ok(&b"cat"[..])
            && resolve(Some(b"echo")) == Ok(&b"echo"[..]),
        "static",
    );
    s.add(
        "XR8-启动失败码",
        spawn_err(1) == Ok(RC_NOENT)
            && spawn_err(2) == Ok(RC_CANNOT)
            && l.sample(7).map(|x| x.value) == Some(RC_NOENT),
        "static",
    );
    s.add(
        "XR9-零状态带信号",
        classify(0, true) == Err(XR_EINVAL)
            && l.sample(8).map(|x| x.code) == Some(XR_EINVAL),
        "static",
    );
    s.add(
        "XR10-子状态过多",
        {
            let many = [0u32; 17];
            aggregate(&many) == Err(XR_ECAP)
                && l.sample(9).map(|x| x.code) == Some(XR_ECAP)
        },
        "static",
    );
    s.add(
        "XR11-空命令与越界",
        resolve(Some(b"")) == Err(XR_ENOCMD)
            && status_low(300) == Err(XR_ECODE)
            && l.sample(10).map(|x| x.code) == Some(XR_ENOCMD)
            && l.sample(11).map(|x| x.code) == Some(XR_ECODE),
        "static",
    );
    s.add(
        "XR12-未知类别与空归并",
        spawn_err(3) == Err(XR_EKIND)
            && spawn_err(0) == Err(XR_EKIND)
            && aggregate(&[]) == Err(XR_EEMPTY)
            && l.sample(12).map(|x| x.code) == Some(XR_EKIND)
            && l.sample(13).map(|x| x.code) == Some(XR_EEMPTY),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(XR_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "XR13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && f.root() == root && inj == [true; 4],
        &format!("root={:016x} inj={:?}", root, inj),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_statuses() {
        assert_eq!(classify(0, false), Ok(RC_OK));
        assert_eq!(classify(1, false), Ok(RC_ANY));
        assert_eq!(classify(255, false), Ok(RC_255));
        assert_eq!(classify(9, true), Ok(RC_SIG));
        assert_eq!(classify(0, true), Err(XR_EINVAL));
    }

    #[test]
    fn aggregate_precedence() {
        assert_eq!(aggregate(&[0, 0]), Ok(RC_OK));
        assert_eq!(aggregate(&[0, RC_ANY]), Ok(RC_ANY));
        assert_eq!(aggregate(&[RC_ANY, RC_SIG]), Ok(RC_SIG));
        assert_eq!(aggregate(&[RC_SIG, RC_255]), Ok(RC_255));
    }

    #[test]
    fn missing_command_defaults_to_echo() {
        assert_eq!(resolve(None), Ok(&b"echo"[..]));
        assert_eq!(resolve(Some(b"wc")), Ok(&b"wc"[..]));
        assert_eq!(resolve(Some(b"")), Err(XR_ENOCMD));
    }

    #[test]
    fn spawn_error_codes() {
        assert_eq!(spawn_err(1), Ok(127));
        assert_eq!(spawn_err(2), Ok(126));
        assert_eq!(spawn_err(3), Err(XR_EKIND));
    }

    #[test]
    fn aggregate_bounds() {
        assert_eq!(aggregate(&[]), Err(XR_EEMPTY));
        let many = [0u32; 17];
        assert_eq!(aggregate(&many), Err(XR_ECAP));
    }

    #[test]
    fn status_narrowing() {
        assert_eq!(status_low(0), Ok(0));
        assert_eq!(status_low(255), Ok(255));
        assert_eq!(status_low(256), Err(XR_ECODE));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11493_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
