//! UNX-F11520 B16 批 ktest 断言集聚合
//!
//! 一批 ktest 断言共 **16 条**，按编号每 4 条一条泳道，四条泳道（0..3）正好
//! 铺满一张 4×4 的批表。聚合器只做三件事：记账、判词、评级。
//!
//! **记账**：每条断言必须有编号（0..15，稠密且不重复）、泳道（必须等于
//! `编号 / 4`，否则表里会出现「第四条断言挂在第二泳道」这种错位）、判词
//! （通过 / 失败 / 跳过）和一个**证据字节**。证据是 4 位的观测掩码，表示
//! 哪几路观测确认了这条断言。
//!
//! **判词与证据必须自洽**，这是本模块最要紧的一条：
//!
//! - `Pass` 必须带至少一位证据，否则就是「幽灵通过」——没人看见它过，它却报
//!   了过，记 `KB_EGHOST`；
//! - `Skip` 必须**零**证据且带非零原因码，有证据说明它其实跑过，记
//!   `KB_ESKIP`；
//! - `Fail` 必须零证据，带证据等于「既失败又确认通过」，记 `KB_EFAIL`。
//!
//! **评级**是聚合而不是投票：只要有一条 `Fail` 就是红，否则只要有一条 `Skip`
//! 就是黄，全通过才是绿。`declare` 拿来核对上游自报的评级，不符即
//! `KB_EGRADE`——自报绿而实际有失败，是最危险的一种谎。
//!
//! 闸门次序（`push`）：编号越界 → 泳道错位 → 表满 → 重号 → 证据自洽。
//! 编号在最前：它定位到行，行都不存在时谈证据没有意义。
//!
//! 账本语义：十四格 = 八格成功（三档评级、通过总数、单泳道计数、失败计数、
//! 跳过计数、证据并集）+ 六格失败（幽灵、跳过矛盾、失败带证、错位、重号、
//! 自报评级不符）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay`
//! 重建比对，`freeze` 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf190;

/// 通过却无证据。
pub const KB_EGHOST: u32 = SEG_BASE + 0x01;
/// 跳过却带证据或原因码为零。
pub const KB_ESKIP: u32 = SEG_BASE + 0x02;
/// 失败却带通过证据。
pub const KB_EFAIL: u32 = SEG_BASE + 0x03;
/// 泳道与编号错位。
pub const KB_ELANE: u32 = SEG_BASE + 0x04;
/// 编号越界或重复。
pub const KB_EID: u32 = SEG_BASE + 0x05;
/// 自报评级与统计不符。
pub const KB_EGRADE: u32 = SEG_BASE + 0x06;
/// 批表已满。
pub const KB_EFULL: u32 = SEG_BASE + 0x07;
/// 批次未满就评级。
pub const KB_EBATCH: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const KB_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const KB_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const KB_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const KB_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const KB_ENOBS: u32 = SEG_BASE + 0x0d;

/// 批表容量。
pub const SLOTS: usize = 16;
/// 泳道数。
pub const LANES: usize = 4;
/// 每泳道断言数。
pub const PER_LANE: usize = 4;
/// 证据位宽掩码。
pub const EV_MASK: u8 = 0b1111;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 判词：通过。
pub const PASS: u8 = 0;
/// 判词：失败。
pub const FAIL: u8 = 1;
/// 判词：跳过。
pub const SKIP: u8 = 2;

/// 评级：绿。
pub const GREEN: u8 = 0;
/// 评级：黄。
pub const YELLOW: u8 = 1;
/// 评级：红。
pub const RED: u8 = 2;

/// 操作码：登记。
pub const OP_PUSH: u8 = 1;
/// 操作码：评级。
pub const OP_GRADE: u8 = 2;
/// 操作码：核对。
pub const OP_DECLARE: u8 = 3;

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

/// 一条断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asrt {
    /// 编号 0..15。
    pub id: u8,
    /// 泳道 0..3，必须等于 `id / 4`。
    pub lane: u8,
    /// 判词。
    pub verdict: u8,
    /// 证据位（4 位观测掩码）。
    pub evidence: u8,
    /// 跳过原因码 / 失败码。
    pub reason: u8,
}

impl Asrt {
    /// 空白断言。
    pub const fn blank() -> Asrt {
        Asrt { id: 0, lane: 0, verdict: PASS, evidence: 0, reason: 0 }
    }

    /// 失败断言。
    pub const fn failed(id: u8, reason: u8) -> Asrt {
        Asrt { id, lane: id / PER_LANE as u8, verdict: FAIL, evidence: 0, reason }
    }

    /// 跳过断言。
    pub const fn skipped(id: u8, reason: u8) -> Asrt {
        Asrt { id, lane: id / PER_LANE as u8, verdict: SKIP, evidence: 0, reason }
    }

    /// 通过断言；证据取低 4 位里的循环位。
    pub const fn passed(id: u8) -> Asrt {
        Asrt {
            id,
            lane: id / PER_LANE as u8,
            verdict: PASS,
            evidence: 1 << (id % PER_LANE as u8),
            reason: 0,
        }
    }
}

/// 四泳道计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pass: [u8; LANES],
    fail: [u8; LANES],
    skip: [u8; LANES],
}

impl Tally {
    /// 零计数。
    pub const fn new() -> Tally {
        Tally { pass: [0; LANES], fail: [0; LANES], skip: [0; LANES] }
    }

    /// 第 l 泳道通过数。
    pub fn lane_pass(&self, l: usize) -> u8 {
        match self.pass.get(l) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 第 l 泳道失败数。
    pub fn lane_fail(&self, l: usize) -> u8 {
        match self.fail.get(l) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 第 l 泳道跳过数。
    pub fn lane_skip(&self, l: usize) -> u8 {
        match self.skip.get(l) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 通过总数。
    pub fn total_pass(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < LANES {
            n = n.saturating_add(self.lane_pass(i) as u32);
            i += 1;
        }
        n
    }

    /// 失败总数。
    pub fn total_fail(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < LANES {
            n = n.saturating_add(self.lane_fail(i) as u32);
            i += 1;
        }
        n
    }

    /// 跳过总数。
    pub fn total_skip(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < LANES {
            n = n.saturating_add(self.lane_skip(i) as u32);
            i += 1;
        }
        n
    }

    /// 断言总数。
    pub fn total(&self) -> u32 {
        self.total_pass()
            .saturating_add(self.total_fail())
            .saturating_add(self.total_skip())
    }
}

/// 一张 4×4 批表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    rows: [Asrt; SLOTS],
    n: usize,
    seen: u32,
}

impl Batch {
    /// 空批表。
    pub const fn new() -> Batch {
        Batch { rows: [Asrt::blank(); SLOTS], n: 0, seen: 0 }
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 是否已满 16 条。
    pub fn is_full(&self) -> bool {
        self.n == SLOTS
    }

    /// 第 i 条。
    pub fn row(&self, i: usize) -> Option<Asrt> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 登记一条：编号 → 泳道 → 容量 → 证据。
    pub fn push(&mut self, a: Asrt) -> Result<usize, u32> {
        if a.id as usize >= SLOTS {
            return Err(KB_EID);
        }
        let bit = 1u32 << (a.id & 31);
        if a.lane as usize != a.id as usize / PER_LANE {
            return Err(KB_ELANE);
        }
        if self.n >= SLOTS {
            return Err(KB_EFULL);
        }
        if self.seen & bit != 0 {
            return Err(KB_EID);
        }
        if a.evidence & !EV_MASK != 0 {
            return Err(KB_EGHOST);
        }
        match a.verdict {
            PASS => {
                if a.evidence == 0 {
                    return Err(KB_EGHOST);
                }
            }
            SKIP => {
                if a.evidence != 0 || a.reason == 0 {
                    return Err(KB_ESKIP);
                }
            }
            _ => {
                if a.evidence != 0 {
                    return Err(KB_EFAIL);
                }
            }
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = a;
        }
        self.seen |= bit;
        let idx = self.n;
        self.n += 1;
        Ok(idx)
    }

    /// 四泳道计数。
    pub fn tally(&self) -> Tally {
        let mut t = Tally::new();
        let mut i = 0usize;
        while i < self.n {
            if let Some(a) = self.rows.get(i) {
                let l = a.lane as usize;
                match a.verdict {
                    PASS => {
                        if let Some(d) = t.pass.get_mut(l) {
                            *d = d.saturating_add(1);
                        }
                    }
                    SKIP => {
                        if let Some(d) = t.skip.get_mut(l) {
                            *d = d.saturating_add(1);
                        }
                    }
                    _ => {
                        if let Some(d) = t.fail.get_mut(l) {
                            *d = d.saturating_add(1);
                        }
                    }
                }
            }
            i += 1;
        }
        t
    }

    /// 证据并集。
    pub fn evidence_mask(&self) -> u8 {
        let mut m = 0u8;
        let mut i = 0usize;
        while i < self.n {
            if let Some(a) = self.rows.get(i) {
                m |= a.evidence;
            }
            i += 1;
        }
        m
    }

    /// 评级：失败即红，跳过即黄，全过即绿。
    pub fn grade(&self) -> Result<u8, u32> {
        if self.n != SLOTS {
            return Err(KB_EBATCH);
        }
        let t = self.tally();
        if t.total_fail() > 0 {
            return Ok(RED);
        }
        if t.total_skip() > 0 {
            return Ok(YELLOW);
        }
        if t.total_pass() as usize != SLOTS {
            return Err(KB_EGRADE);
        }
        Ok(GREEN)
    }

    /// 核对上游自报评级。
    pub fn declare(&self, g: u8) -> Result<u8, u32> {
        let real = self.grade()?;
        if real != g {
            return Err(KB_EGRADE);
        }
        Ok(real)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > SLOTS {
            return false;
        }
        let t = self.tally();
        if t.total() as usize != self.n {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            match self.rows.get(i) {
                Some(a) => {
                    if a.id as usize >= SLOTS || a.lane as usize != a.id as usize / PER_LANE {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(KB_ESLOT);
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

    /// 对账：八成功值 + 六失败码，且槽位连续。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(KB_EBAL);
        }
        const VALS: [u32; 8] = [0, 2, 1, 16, 4, 1, 1, 15];
        const CODES: [u32; 6] =
            [KB_EGHOST, KB_ESKIP, KB_EFAIL, KB_ELANE, KB_EID, KB_EGRADE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(KB_EBAL),
            };
            if s.slot as usize != i {
                return Err(KB_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(KB_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(KB_EVALUE);
                        }
                    }
                    None => return Err(KB_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(KB_EVALUE);
                        }
                    }
                    None => return Err(KB_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(KB_EBAL);
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
            return Err(KB_EBAL);
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
            Err(KB_ESEAL)
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

/// 全绿批表：16 条通过。
fn green() -> Batch {
    let mut b = Batch::new();
    let mut i = 0u8;
    while i < 16 {
        let _ = b.push(Asrt::passed(i));
        i += 1;
    }
    b
}

/// 见红批表：第 5 条失败。
fn red() -> Batch {
    let mut b = Batch::new();
    let mut i = 0u8;
    while i < 16 {
        if i == 5 {
            let _ = b.push(Asrt::failed(i, 0x2a));
        } else {
            let _ = b.push(Asrt::passed(i));
        }
        i += 1;
    }
    b
}

/// 见黄批表：第 9 条跳过。
fn yellow() -> Batch {
    let mut b = Batch::new();
    let mut i = 0u8;
    while i < 16 {
        if i == 9 {
            let _ = b.push(Asrt::skipped(i, 0x13));
        } else {
            let _ = b.push(Asrt::passed(i));
        }
        i += 1;
    }
    b
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_GRADE, green().grade().map(|g| g as u32)));
    let _ = l.register(word_cell(1, OP_GRADE, red().grade().map(|g| g as u32)));
    let _ = l.register(word_cell(2, OP_GRADE, yellow().grade().map(|g| g as u32)));
    let _ = l.register(word_cell(3, OP_GRADE, Ok(green().tally().total_pass())));
    let _ = l.register(word_cell(4, OP_GRADE, Ok(green().tally().lane_pass(0) as u32)));
    let _ = l.register(word_cell(5, OP_GRADE, Ok(red().tally().total_fail())));
    let _ = l.register(word_cell(6, OP_GRADE, Ok(yellow().tally().total_skip())));
    let _ = l.register(word_cell(7, OP_GRADE, Ok(green().evidence_mask() as u32)));

    let _ = l.register(word_cell(
        8,
        OP_PUSH,
        Batch::new()
            .push(Asrt { id: 0, lane: 0, verdict: PASS, evidence: 0, reason: 0 })
            .map(|i| i as u32),
    ));
    let _ = l.register(word_cell(
        9,
        OP_PUSH,
        Batch::new()
            .push(Asrt { id: 1, lane: 0, verdict: SKIP, evidence: 1, reason: 5 })
            .map(|i| i as u32),
    ));
    let _ = l.register(word_cell(
        10,
        OP_PUSH,
        Batch::new()
            .push(Asrt { id: 2, lane: 0, verdict: FAIL, evidence: 2, reason: 7 })
            .map(|i| i as u32),
    ));
    let _ = l.register(word_cell(
        11,
        OP_PUSH,
        Batch::new()
            .push(Asrt { id: 5, lane: 2, verdict: PASS, evidence: 1, reason: 0 })
            .map(|i| i as u32),
    ));
    let _ = l.register(word_cell(12, OP_PUSH, {
        let mut b = Batch::new();
        let _ = b.push(Asrt::passed(4));
        b.push(Asrt::passed(4)).map(|i| i as u32)
    }));
    let _ = l.register(word_cell(13, OP_DECLARE, green().declare(RED).map(|g| g as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：全绿。
    let g = green();
    out[1] = g.grade() == Ok(GREEN)
        && g.is_full()
        && g.tally().total_pass() == 16
        && g.tally().lane_pass(0) == 4
        && g.tally().lane_pass(3) == 4
        && g.evidence_mask() == EV_MASK
        && g.consistent();

    // 二路：红黄两档。
    let r = red();
    let y = yellow();
    out[2] = r.grade() == Ok(RED)
        && r.tally().total_fail() == 1
        && r.tally().lane_fail(1) == 1
        && y.grade() == Ok(YELLOW)
        && y.tally().total_skip() == 1
        && y.tally().lane_skip(2) == 1
        && r.tally().total_pass() == 15
        && y.tally().total_pass() == 15;

    // 三路：证据自洽与闸门。
    let mut e = Batch::new();
    let ghost = e.push(Asrt { id: 0, lane: 0, verdict: PASS, evidence: 0, reason: 0 });
    let skip_ev = e.push(Asrt { id: 1, lane: 0, verdict: SKIP, evidence: 3, reason: 9 });
    let skip_no = e.push(Asrt { id: 2, lane: 0, verdict: SKIP, evidence: 0, reason: 0 });
    let fail_ev = e.push(Asrt { id: 3, lane: 0, verdict: FAIL, evidence: 1, reason: 4 });
    let lane_bad = e.push(Asrt { id: 6, lane: 2, verdict: PASS, evidence: 1, reason: 0 });
    let wide = e.push(Asrt { id: 7, lane: 1, verdict: PASS, evidence: 0x10, reason: 0 });
    out[3] = ghost == Err(KB_EGHOST)
        && skip_ev == Err(KB_ESKIP)
        && skip_no == Err(KB_ESKIP)
        && fail_ev == Err(KB_EFAIL)
        && lane_bad == Err(KB_ELANE)
        && wide == Err(KB_EGHOST)
        && e.is_empty()
        && g.declare(RED) == Err(KB_EGRADE)
        && Batch::new().grade() == Err(KB_EBATCH);

    out
}

/// 十三项判据。
pub fn run_unx11520_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11520.b16_ktest_batch");
    let l = build_ledger();

    s.add_dyn(
        "KB1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "KB2-四泳道各四断言",
        {
            let b = green();
            b.len() == SLOTS
                && b.row(0).map(|a| a.lane) == Some(0)
                && b.row(4).map(|a| a.lane) == Some(1)
                && b.row(15).map(|a| a.lane) == Some(3)
                && b.tally().lane_pass(0) == 4
                && b.tally().lane_pass(3) == 4
                && b.tally().total() == 16
                && l.sample(4).map(|x| x.value) == Some(4)
        },
        "static",
    );

    s.add(
        "KB3-全绿评级",
        green().grade() == Ok(GREEN)
            && green().declare(GREEN) == Ok(GREEN)
            && green().evidence_mask() == EV_MASK
            && l.sample(0).map(|x| x.value) == Some(0)
            && l.sample(7).map(|x| x.value) == Some(15),
        "static",
    );

    s.add(
        "KB4-失败即红",
        {
            let r = red();
            r.grade() == Ok(RED)
                && r.declare(RED) == Ok(RED)
                && r.tally().total_fail() == 1
                && r.tally().lane_fail(1) == 1
                && r.tally().total_pass() == 15
                && l.sample(1).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "KB5-跳过即黄",
        {
            let y = yellow();
            y.grade() == Ok(YELLOW)
                && y.declare(YELLOW) == Ok(YELLOW)
                && y.tally().total_skip() == 1
                && y.tally().lane_skip(2) == 1
                && y.tally().total_fail() == 0
                && l.sample(2).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "KB6-幽灵通过拒",
        {
            let mut b = Batch::new();
            b.push(Asrt { id: 0, lane: 0, verdict: PASS, evidence: 0, reason: 0 })
                == Err(KB_EGHOST)
                && b.push(Asrt { id: 0, lane: 0, verdict: PASS, evidence: 0x10, reason: 0 })
                    == Err(KB_EGHOST)
                && b.is_empty()
                && l.sample(8).map(|x| x.code) == Some(KB_EGHOST)
        },
        "static",
    );

    s.add(
        "KB7-跳过必须无证有因",
        {
            let mut b = Batch::new();
            b.push(Asrt { id: 0, lane: 0, verdict: SKIP, evidence: 1, reason: 3 })
                == Err(KB_ESKIP)
                && b.push(Asrt { id: 0, lane: 0, verdict: SKIP, evidence: 0, reason: 0 })
                    == Err(KB_ESKIP)
                && b.push(Asrt::skipped(0, 3)).is_ok()
                && l.sample(9).map(|x| x.code) == Some(KB_ESKIP)
        },
        "static",
    );

    s.add(
        "KB8-失败不得带证",
        {
            let mut b = Batch::new();
            b.push(Asrt { id: 0, lane: 0, verdict: FAIL, evidence: 4, reason: 1 })
                == Err(KB_EFAIL)
                && b.push(Asrt::failed(0, 1)).is_ok()
                && l.sample(10).map(|x| x.code) == Some(KB_EFAIL)
        },
        "static",
    );

    s.add(
        "KB9-泳道错位拒",
        {
            let mut b = Batch::new();
            b.push(Asrt { id: 5, lane: 0, verdict: PASS, evidence: 1, reason: 0 })
                == Err(KB_ELANE)
                && b.push(Asrt { id: 5, lane: 2, verdict: PASS, evidence: 1, reason: 0 })
                    == Err(KB_ELANE)
                && b.push(Asrt { id: 5, lane: 5, verdict: PASS, evidence: 1, reason: 0 })
                    == Err(KB_ELANE)
                && l.sample(11).map(|x| x.code) == Some(KB_ELANE)
        },
        "static",
    );

    s.add(
        "KB10-编号越界与重号",
        {
            let mut b = Batch::new();
            let six = Asrt::passed(6);
            b.push(Asrt { id: 16, lane: 4, verdict: PASS, evidence: 1, reason: 0 })
                == Err(KB_EID)
                && b.push(six).is_ok()
                && b.push(six) == Err(KB_EID)
                && b.len() == 1
                && l.sample(12).map(|x| x.code) == Some(KB_EID)
        },
        "static",
    );

    s.add(
        "KB11-未满与表满",
        {
            let mut b = Batch::new();
            let mut i = 0u8;
            while i < 15 {
                let _ = b.push(Asrt::passed(i));
                i += 1;
            }
            b.grade() == Err(KB_EBATCH)
                && !b.is_full()
                && b.push(Asrt::passed(15)).is_ok()
                && b.is_full()
                && b.push(Asrt::passed(0)) == Err(KB_EFULL)
                && b.grade() == Ok(GREEN)
        },
        "static",
    );

    s.add(
        "KB12-统计与评级同源",
        {
            let g = green();
            let t = g.tally();
            t.total_pass() + t.total_fail() + t.total_skip() == 16
                && t.total() == 16
                && g.grade() == Ok(GREEN)
                && g.declare(YELLOW) == Err(KB_EGRADE)
                && yellow().declare(GREEN) == Err(KB_EGRADE)
                && red().declare(GREEN) == Err(KB_EGRADE)
                && l.sample(3).map(|x| x.value) == Some(16)
                && l.sample(5).map(|x| x.value) == Some(1)
                && l.sample(6).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "KB13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
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
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11520_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn green_is_sixteen_passes() {
        let b = green();
        assert_eq!(b.len(), 16);
        assert!(b.is_full());
        assert_eq!(b.grade(), Ok(GREEN));
        assert_eq!(b.tally().total_pass(), 16);
        assert_eq!(b.evidence_mask(), EV_MASK);
    }

    #[test]
    fn red_and_yellow() {
        assert_eq!(red().grade(), Ok(RED));
        assert_eq!(yellow().grade(), Ok(YELLOW));
        assert_eq!(red().tally().total_fail(), 1);
        assert_eq!(yellow().tally().total_skip(), 1);
    }

    #[test]
    fn evidence_rules() {
        let mut b = Batch::new();
        assert_eq!(
            b.push(Asrt { id: 0, lane: 0, verdict: PASS, evidence: 0, reason: 0 }),
            Err(KB_EGHOST)
        );
        assert_eq!(
            b.push(Asrt { id: 0, lane: 0, verdict: SKIP, evidence: 2, reason: 1 }),
            Err(KB_ESKIP)
        );
        assert_eq!(
            b.push(Asrt { id: 0, lane: 0, verdict: FAIL, evidence: 2, reason: 1 }),
            Err(KB_EFAIL)
        );
        assert!(b.is_empty());
    }

    #[test]
    fn batch_gates() {
        assert_eq!(Batch::new().grade(), Err(KB_EBATCH));
        let mut b = Batch::new();
        assert_eq!(
            b.push(Asrt { id: 4, lane: 2, verdict: PASS, evidence: 1, reason: 0 }),
            Err(KB_ELANE)
        );
        assert_eq!(b.push(Asrt::passed(0)), Ok(0));
        assert_eq!(b.push(Asrt::passed(0)), Err(KB_EID));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
