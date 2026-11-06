//! UNX-F11348 glibc 高频探测序列消费（0xebc0 段）
//!
//! 动态加载器与 C 库在启动阶段并不「一次问到底」：它按一条固定序列逐项探测内核
//! 能力（读 auxv、取 tid、试探 `rseq`、问 `prctl`……），每项探针的结果只影响它
//! 自己那一步。序列因此有三种结局：
//!
//! - 全部成功：序列走完，零降级；
//! - **必选项**失败：序列立即截断，后面的探针一个都不执行，进程启动失败；
//! - **可选项**失败：只记一次降级，序列继续，后续步骤照跑。
//!
//! 把这三条混成一条「遇到错就停」的规则，是这类代码最常见的失真：可选项一旦失败
//! 就停，会把可用的内核能力误判成不可用；必选项失败不停，会在后面拿空数据继续
//! 计算，错误点被推迟到别处爆发。
//!
//! 本模块把脚本、执行日志、截断位置、致命探针号与降级计数分开记：日志里每一格
//! 只能是「未执行/成功/容忍失败/致命失败」四态，截断之后的位置必须保持「未执行」，
//! 这一条把「停得对不对」变成可观测的事实。
//!
//! 闸门次序：
//! - `add`：零号 → 已运行 → 表满 → 重复；
//! - `run`：已运行 → 空脚本。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xebc0;

/// 探针号为零。
pub const PR_EINVAL: u32 = SEG_BASE + 0x01;
/// 序列已运行。
pub const PR_ERUN: u32 = SEG_BASE + 0x02;
/// 脚本表满。
pub const PR_EFULL: u32 = SEG_BASE + 0x03;
/// 同号探针重复。
pub const PR_EDUP: u32 = SEG_BASE + 0x04;
/// 空脚本无法运行。
pub const PR_ESCRIPT: u32 = SEG_BASE + 0x05;
/// 必选探针失败，序列截断。
pub const PR_EPROBE: u32 = SEG_BASE + 0x06;
/// 观察下标越界。
pub const PR_ENOBS: u32 = SEG_BASE + 0x07;
/// 账本形状不符。
pub const PR_EBAL: u32 = SEG_BASE + 0x08;
/// 槽位乱序。
pub const PR_EORDER: u32 = SEG_BASE + 0x09;
/// 观测值与复算不符。
pub const PR_EMISMATCH: u32 = SEG_BASE + 0x0a;
/// 冻结后重放不一致。
pub const PR_ESEAL: u32 = SEG_BASE + 0x0b;
/// 状态表与截断位置矛盾。
pub const PR_ESTALE: u32 = SEG_BASE + 0x0c;
/// 环境掩码越界。
pub const PR_EENV: u32 = SEG_BASE + 0x0d;

/// 脚本容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 未执行。
pub const ST_SKIP: u8 = 0;
/// 成功。
pub const ST_OK: u8 = 1;
/// 失败但被容忍。
pub const ST_TOLERATED: u8 = 2;
/// 致命失败。
pub const ST_FATAL: u8 = 3;

/// 操作码：挂探针。
pub const OP_ADD: u8 = 1;
/// 操作码：运行序列。
pub const OP_RUN: u8 = 2;
/// 操作码：读计数。
pub const OP_COUNT: u8 = 3;

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

/// 一个探针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// 探针号。
    pub id: u32,
    /// 是否必选。
    pub need: bool,
}

/// 探测序列。
#[derive(Clone, Copy, Debug)]
pub struct Prober {
    tab: [Probe; CAP],
    n: usize,
    log: [u8; CAP],
    log_len: usize,
    ran: bool,
    step: usize,
    aborted: usize,
    fatal: u32,
    degraded: u32,
    tolerated_limit: u32,
}

impl Prober {
    /// 空序列。
    pub const fn new() -> Prober {
        Prober {
            tab: [Probe { id: 0, need: false }; CAP],
            n: 0,
            log: [ST_SKIP; CAP],
            log_len: 0,
            ran: false,
            step: 0,
            aborted: CAP,
            fatal: 0,
            degraded: 0,
            tolerated_limit: CAP as u32,
        }
    }

    /// 脚本条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否已运行。
    pub fn is_ran(&self) -> bool {
        self.ran
    }

    /// 实际执行步数。
    pub fn executed(&self) -> usize {
        self.step
    }

    /// 降级次数。
    pub fn degraded(&self) -> u32 {
        self.degraded
    }

    /// 截断位置（未截断时为 `CAP`）。
    pub fn aborted_at(&self) -> usize {
        self.aborted
    }

    /// 致命探针号（未截断为 0）。
    pub fn fatal(&self) -> u32 {
        self.fatal
    }

    /// 第 i 格日志。
    pub fn status_at(&self, i: usize) -> Result<u8, u32> {
        if i >= self.log_len {
            return Err(PR_ENOBS);
        }
        match self.log.get(i) {
            Some(v) => Ok(*v),
            None => Err(PR_ENOBS),
        }
    }

    /// 成功步数。
    pub fn ok_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.log_len {
            if let Some(v) = self.log.get(i) {
                if *v == ST_OK {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 容忍失败步数。
    pub fn tolerated_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.log_len {
            if let Some(v) = self.log.get(i) {
                if *v == ST_TOLERATED {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 日志与截断位置是否自洽：致命之后全是未执行，未截断则无致命格。
    pub fn consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.log_len {
            let v = match self.log.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if v == ST_FATAL && self.aborted != i {
                return false;
            }
            if self.aborted != CAP && i > self.aborted && v != ST_SKIP {
                return false;
            }
            i += 1;
        }
        if self.aborted == CAP && self.fatal != 0 {
            return false;
        }
        true
    }

    /// 挂一个探针。
    pub fn add(&mut self, id: u32, need: bool) -> Result<usize, u32> {
        if id == 0 {
            return Err(PR_EINVAL);
        }
        if self.ran {
            return Err(PR_ERUN);
        }
        if self.n >= CAP {
            return Err(PR_EFULL);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.tab.get(i) {
                Some(p) => {
                    if p.id == id {
                        return Err(PR_EDUP);
                    }
                }
                None => return Err(PR_EFULL),
            }
            i += 1;
        }
        if let Some(slot) = self.tab.get_mut(self.n) {
            *slot = Probe { id, need };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 按掩码运行序列：第 k 位为 1 表示第 k 个探针成功。
    pub fn run(&mut self, mask: u32) -> Result<usize, u32> {
        if self.ran {
            return Err(PR_ERUN);
        }
        if self.n == 0 {
            return Err(PR_ESCRIPT);
        }
        self.ran = true;
        self.log_len = self.n;
        let mut k = 0usize;
        while k < self.n {
            let p = match self.tab.get(k) {
                Some(v) => *v,
                None => break,
            };
            let ok = match 1u32.checked_shl(k as u32) {
                Some(bit) => mask & bit != 0,
                None => false,
            };
            if ok {
                if let Some(slot) = self.log.get_mut(k) {
                    *slot = ST_OK;
                }
                k += 1;
                continue;
            }
            if p.need {
                if let Some(slot) = self.log.get_mut(k) {
                    *slot = ST_FATAL;
                }
                self.aborted = k;
                self.fatal = p.id;
                self.step = k + 1;
                return Err(PR_EPROBE);
            }
            if let Some(slot) = self.log.get_mut(k) {
                *slot = ST_TOLERATED;
            }
            self.degraded = self.degraded.saturating_add(1);
            k += 1;
        }
        self.step = k;
        Ok(k)
    }

    /// 降级余量：容忍失败超过脚本长度视为整条序列不可信。
    pub fn within_budget(&self) -> bool {
        self.degraded <= self.tolerated_limit
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
            return Err(PR_EORDER);
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
            return Err(PR_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 4, 1, 4, 3];
        const CODES: [u32; 6] =
            [PR_EINVAL, PR_EDUP, PR_EFULL, PR_ESCRIPT, PR_EPROBE, PR_ERUN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PR_EBAL),
            };
            if s.slot as usize != i {
                return Err(PR_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PR_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PR_EMISMATCH);
                        }
                    }
                    None => return Err(PR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PR_EMISMATCH);
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
            return Err(PR_EBAL);
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
            Err(PR_ESEAL)
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

/// 十四格账本：四项脚本（两必选两可选）→ 一次带容忍失败的运行 → 读计数 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut p = Prober::new();

    let _ = l.register(cell(0, OP_ADD, p.add(1, true)));
    let _ = l.register(cell(1, OP_ADD, p.add(2, true)));
    let _ = l.register(cell(2, OP_ADD, p.add(3, false)));
    let _ = l.register(cell(3, OP_ADD, p.add(4, false)));
    let _ = l.register(cell(4, OP_RUN, p.run(0b1011)));
    let _ = l.register(cell(5, OP_COUNT, Ok(p.degraded() as usize)));
    let _ = l.register(cell(6, OP_COUNT, Ok(p.executed())));
    let _ = l.register(cell(7, OP_COUNT, Ok(p.ok_count())));

    let mut e = Prober::new();
    let _ = l.register(cell(8, OP_ADD, e.add(0, true)));

    let mut d = Prober::new();
    let _ = d.add(5, true);
    let _ = l.register(cell(9, OP_ADD, d.add(5, true)));

    let mut full = Prober::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.add(i, false);
        i += 1;
    }
    let _ = l.register(cell(10, OP_ADD, full.add(0x100, false)));

    let mut empty = Prober::new();
    let _ = l.register(cell(11, OP_RUN, empty.run(0)));

    let mut r = Prober::new();
    let _ = r.add(7, true);
    let _ = l.register(cell(12, OP_RUN, r.run(0)));

    let _ = l.register(cell(13, OP_RUN, p.run(0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：全成功时日志逐格为 OK，无降级无截断。
    let mut a = Prober::new();
    let _ = a.add(11, true);
    let _ = a.add(12, false);
    let _ = a.add(13, true);
    let r = a.run(0b111);
    out[1] = r == Ok(3)
        && a.status_at(0) == Ok(ST_OK)
        && a.status_at(1) == Ok(ST_OK)
        && a.status_at(2) == Ok(ST_OK)
        && a.status_at(3) == Err(PR_ENOBS)
        && a.degraded() == 0
        && a.aborted_at() == CAP
        && a.consistent();

    // 二路：必选失败立即截断，后续保持未执行。
    let mut b = Prober::new();
    let _ = b.add(21, true);
    let _ = b.add(22, true);
    let _ = b.add(23, false);
    let r = b.run(0b001);
    out[2] = r == Err(PR_EPROBE)
        && b.executed() == 2
        && b.aborted_at() == 1
        && b.fatal() == 22
        && b.status_at(0) == Ok(ST_OK)
        && b.status_at(1) == Ok(ST_FATAL)
        && b.status_at(2) == Ok(ST_SKIP)
        && b.degraded() == 0
        && b.consistent();

    // 三路：可选项失败只降级不停；同时闸门齐备。
    let mut c = Prober::new();
    let _ = c.add(31, false);
    let _ = c.add(32, false);
    let _ = c.add(33, true);
    let r = c.run(0b100);
    let g1 = c.add(34, false) == Err(PR_ERUN);
    let g2 = c.run(0) == Err(PR_ERUN);
    let mut g = Prober::new();
    let g3 = g.add(0, true) == Err(PR_EINVAL);
    let g4 = g.run(0) == Err(PR_ESCRIPT);
    out[3] = r == Ok(3)
        && c.degraded() == 2
        && c.ok_count() == 1
        && c.tolerated_count() == 2
        && c.aborted_at() == CAP
        && c.status_at(0) == Ok(ST_TOLERATED)
        && c.status_at(2) == Ok(ST_OK)
        && c.within_budget()
        && c.consistent()
        && g1
        && g2
        && g3
        && g4;

    out
}

/// 十三项判据。
pub fn run_unx11348_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11348.glibc_probe");
    let l = build_ledger();

    s.add_dyn(
        "PR1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PR2-脚本挂接计数递增",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "PR3-四步脚本满挂",
        l.sample(2).map(|x| x.value) == Some(3) && l.sample(3).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "PR4-带容忍失败跑完四步",
        l.sample(4).map(|x| x.value) == Some(4) && l.sample(6).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "PR5-降级只记一次",
        l.sample(5).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "PR6-成功步数为三",
        l.sample(7).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "PR7-零号探针被拒",
        l.sample(8).map(|x| x.code) == Some(PR_EINVAL),
        "static",
    );

    s.add(
        "PR8-重号与表满",
        l.sample(9).map(|x| x.code) == Some(PR_EDUP)
            && l.sample(10).map(|x| x.code) == Some(PR_EFULL),
        "static",
    );

    s.add(
        "PR9-空脚本与必选截断",
        l.sample(11).map(|x| x.code) == Some(PR_ESCRIPT)
            && l.sample(12).map(|x| x.code) == Some(PR_EPROBE),
        "static",
    );

    s.add(
        "PR10-重复运行被拒",
        l.sample(13).map(|x| x.code) == Some(PR_ERUN),
        "static",
    );

    s.add(
        "PR11-截断点与日志四态",
        {
            let mut a = Prober::new();
            let _ = a.add(41, true);
            let _ = a.add(42, true);
            let _ = a.add(43, true);
            let r = a.run(0b011);
            r == Err(PR_EPROBE)
                && a.status_at(2) == Ok(ST_FATAL)
                && a.fatal() == 43
                && a.aborted_at() == 2
                && a.executed() == 3
                && a.consistent()
        },
        "static",
    );

    s.add(
        "PR12-全成功零降级",
        {
            let mut a = Prober::new();
            let _ = a.add(51, true);
            let _ = a.add(52, false);
            let _ = a.add(53, true);
            let r = a.run(0b111);
            r == Ok(3)
                && a.degraded() == 0
                && a.ok_count() == 3
                && a.tolerated_count() == 0
                && a.fatal() == 0
                && a.aborted_at() == CAP
                && a.within_budget()
                && a.consistent()
        },
        "static",
    );

    s.add(
        "PR13-冻结重放四注入",
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
        let s = run_unx11348_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn optional_failure_is_tolerated() {
        let mut p = Prober::new();
        let _ = p.add(1, false);
        let _ = p.add(2, true);
        assert_eq!(p.run(0b10), Ok(2));
        assert_eq!(p.status_at(0), Ok(ST_TOLERATED));
        assert_eq!(p.status_at(1), Ok(ST_OK));
        assert_eq!(p.degraded(), 1);
        assert_eq!(p.aborted_at(), CAP);
    }

    #[test]
    fn required_failure_aborts() {
        let mut p = Prober::new();
        let _ = p.add(1, true);
        let _ = p.add(2, true);
        let _ = p.add(3, false);
        assert_eq!(p.run(0b001), Err(PR_EPROBE));
        assert_eq!(p.fatal(), 2);
        assert_eq!(p.aborted_at(), 1);
        assert_eq!(p.executed(), 2);
        assert_eq!(p.status_at(2), Ok(ST_SKIP));
    }

    #[test]
    fn gates() {
        let mut p = Prober::new();
        assert_eq!(p.add(0, true), Err(PR_EINVAL));
        assert_eq!(p.run(0), Err(PR_ESCRIPT));
        assert_eq!(p.add(5, false), Ok(1));
        assert_eq!(p.add(5, true), Err(PR_EDUP));
        assert_eq!(p.run(0), Ok(1));
        assert_eq!(p.add(6, false), Err(PR_ERUN));
        assert_eq!(p.run(0), Err(PR_ERUN));
        assert_eq!(p.status_at(1), Err(PR_ENOBS));
    }

    #[test]
    fn capacity_is_exact() {
        let mut p = Prober::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(p.add(i, false).is_ok());
            i += 1;
        }
        assert_eq!(p.add(0xFFFF, false), Err(PR_EFULL));
        assert_eq!(p.count(), CAP);
    }

    #[test]
    fn all_success_has_no_degradation() {
        let mut p = Prober::new();
        let _ = p.add(1, true);
        let _ = p.add(2, false);
        assert_eq!(p.run(0b11), Ok(2));
        assert_eq!(p.degraded(), 0);
        assert_eq!(p.ok_count(), 2);
        assert_eq!(p.fatal(), 0);
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
