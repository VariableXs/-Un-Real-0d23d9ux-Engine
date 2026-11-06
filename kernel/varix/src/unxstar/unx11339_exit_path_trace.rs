//! UNX-F11339 退出诊断面（路径追踪）（0xeb30 段）
//!
//! 退出出问题时，日志里最常见的一句话是「回调没跑」。这句话几乎没有信息
//! 量：回调链可能压根没被调用，也可能在 flush 阶段就卡住，还可能跑了一半
//! 被信号打断。要能回答「卡在哪一阶」，退出流程必须自己留下**带次序的阶
//! 段轨迹**，而不是只在最后打一行「exit done」。
//!
//! 本模块把退出流程固定成五阶：
//!
//! `PREPARE(1) → FLUSH(2) → DTOR(3) → ATEXIT(4) → FINAL(5)`
//!
//! 规则只有四条：每阶**恰好进入一次**、必须**逐阶推进**（不许跳阶）、每阶
//! 带一个明细值（用于区分同一阶的多次尝试或不同负载）、`FINAL` 之后才能
//! `close`。轨迹同时维护 `missing`（还差几阶）与 `detail_total`（明细累加），
//! 这两个数让「跑到一半」有量化表达。
//!
//! 闸门次序（`enter`）：已封存 → 阶号非法 → 重复进入 → 跳阶。
//!
//! - 「重复进入」排在「跳阶」之前：回到已经走过的阶，首要结论是「重复」，
//!   而不是「次序不对」——两者修复手段不同，重复是重入缺陷，跳阶是漏调用。
//! - 因为阶必须严格推进，任何「回退」都必然命中「重复进入」，所以本模型
//!   不设单独的「回退」错误码；把回退归到重复，是对实际缺陷更贴切的归类。
//!
//! `close` 的闸门是：已封存 → 未到 `FINAL`。未到终阶的轨迹不能封存，否则
//! 一份「只跑到 flush」的轨迹会被当成完整轨迹收进诊断面。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb30;

/// 阶号非法。
pub const PT_ESTAGE: u32 = SEG_BASE + 0x01;
/// 该阶已进入过。
pub const PT_EDUP: u32 = SEG_BASE + 0x02;
/// 跳阶。
pub const PT_ESKIP: u32 = SEG_BASE + 0x03;
/// 轨迹已封存。
pub const PT_ESEALED: u32 = SEG_BASE + 0x04;
/// 未到终阶。
pub const PT_ENOTFINAL: u32 = SEG_BASE + 0x05;
/// 轨迹下标越界。
pub const PT_ENOBS: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const PT_EBAL: u32 = SEG_BASE + 0x07;
/// 账本槽位乱序。
pub const PT_EORDER: u32 = SEG_BASE + 0x08;
/// 观测值与复算不符。
pub const PT_EMISMATCH: u32 = SEG_BASE + 0x09;
/// 冻结后重放不一致。
pub const PT_ESEAL: u32 = SEG_BASE + 0x0a;
/// 明细值非法。
pub const PT_EDETAIL: u32 = SEG_BASE + 0x0b;
/// 轨迹为空。
pub const PT_EWARM: u32 = SEG_BASE + 0x0c;
/// 阶号表与轨迹不一致。
pub const PT_EGAP: u32 = SEG_BASE + 0x0d;

/// 阶数。
pub const STAGES: usize = 5;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 阶：准备。
pub const ST_PREPARE: u8 = 1;
/// 阶：冲刷。
pub const ST_FLUSH: u8 = 2;
/// 阶：TLS 析构。
pub const ST_DTOR: u8 = 3;
/// 阶：atexit 回调。
pub const ST_ATEXIT: u8 = 4;
/// 阶：收尾。
pub const ST_FINAL: u8 = 5;

/// 操作码：进入阶。
pub const OP_ENTER: u8 = 1;
/// 操作码：封存。
pub const OP_CLOSE: u8 = 2;
/// 操作码：缺阶计数。
pub const OP_MISS: u8 = 3;
/// 操作码：明细累加。
pub const OP_TOTAL: u8 = 4;

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

/// 退出路径轨迹。
#[derive(Clone, Copy, Debug)]
pub struct Trace {
    stage_log: [u8; STAGES],
    detail_log: [u32; STAGES],
    seen: u16,
    n: usize,
    last: u8,
    total: u32,
    sealed: bool,
}

impl Trace {
    /// 空轨迹。
    pub const fn new() -> Trace {
        Trace {
            stage_log: [0; STAGES],
            detail_log: [0; STAGES],
            seen: 0,
            n: 0,
            last: 0,
            total: 0,
            sealed: false,
        }
    }

    /// 已进入阶数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空轨迹。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 是否已封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 当前推进到的阶（空轨迹为 0）。
    pub fn last_stage(&self) -> u8 {
        self.last
    }

    /// 某阶是否已进入。
    pub fn stage_seen(&self, stage: u8) -> bool {
        if stage == 0 || stage as usize > STAGES {
            return false;
        }
        let bit = 1u16 << (stage - 1);
        (self.seen & bit) != 0
    }

    /// 还差几阶。
    pub fn missing(&self) -> usize {
        let mut k = 0usize;
        let mut st = 1u8;
        while (st as usize) <= STAGES {
            if !self.stage_seen(st) {
                k += 1;
            }
            st += 1;
        }
        k
    }

    /// 明细累加。
    pub fn detail_total(&self) -> u32 {
        self.total
    }

    /// 第 i 条轨迹的阶号。
    pub fn stage_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.n {
            return Err(PT_ENOBS);
        }
        match self.stage_log.get(i) {
            Some(v) => Ok(*v as u32),
            None => Err(PT_ENOBS),
        }
    }

    /// 第 i 条轨迹的明细。
    pub fn detail_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.n {
            return Err(PT_ENOBS);
        }
        match self.detail_log.get(i) {
            Some(v) => Ok(*v),
            None => Err(PT_ENOBS),
        }
    }

    /// 进入一阶，返回已进入阶数。
    pub fn enter(&mut self, stage: u8, detail: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(PT_ESEALED);
        }
        if stage == 0 || stage as usize > STAGES {
            return Err(PT_ESTAGE);
        }
        if self.stage_seen(stage) {
            return Err(PT_EDUP);
        }
        if stage != self.last + 1 {
            return Err(PT_ESKIP);
        }
        if detail == 0 {
            return Err(PT_EDETAIL);
        }
        let idx = (stage - 1) as usize;
        if let Some(slot) = self.stage_log.get_mut(idx) {
            *slot = stage;
        }
        if let Some(slot) = self.detail_log.get_mut(idx) {
            *slot = detail;
        }
        self.seen |= 1u16 << (stage - 1);
        self.last = stage;
        self.total = self.total.saturating_add(detail);
        self.n += 1;
        Ok(self.n)
    }

    /// 封存轨迹，返回条目数。
    pub fn close(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(PT_ESEALED);
        }
        if !self.stage_seen(ST_FINAL) {
            return Err(PT_ENOTFINAL);
        }
        self.sealed = true;
        Ok(self.n)
    }

    /// 轨迹自洽复核：阶号必须严格递增且无缺口，明细之和必须等于累加值。
    pub fn recount(&self) -> Result<usize, u32> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let st = match self.stage_log.get(i) {
                Some(v) => *v,
                None => return Err(PT_EBAL),
            };
            if st as usize != i + 1 {
                return Err(PT_EGAP);
            }
            if let Some(d) = self.detail_log.get(i) {
                sum = sum.saturating_add(*d);
            }
            i += 1;
        }
        if sum != self.total {
            return Err(PT_EMISMATCH);
        }
        Ok(self.n)
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
            return Err(PT_EORDER);
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
            return Err(PT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 5, 150, 0];
        const CODES: [u32; 6] = [
            PT_EDUP,
            PT_ESKIP,
            PT_ESTAGE,
            PT_ESEALED,
            PT_ENOBS,
            PT_ENOTFINAL,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PT_EBAL),
            };
            if s.slot as usize != i {
                return Err(PT_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PT_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PT_EMISMATCH);
                        }
                    }
                    None => return Err(PT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PT_EMISMATCH);
                        }
                    }
                    None => return Err(PT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PT_EBAL);
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
            return Err(PT_EBAL);
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
            Err(PT_ESEAL)
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

/// 十四格账本：逐阶推进 → 重复/跳阶/非法阶 → 补齐 → 封存 → 观测 → 终局拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Trace::new();

    let _ = l.register(cell(0, OP_ENTER, t.enter(ST_PREPARE, 10)));
    let _ = l.register(cell(1, OP_ENTER, t.enter(ST_FLUSH, 20)));
    let _ = l.register(cell(2, OP_ENTER, t.enter(ST_PREPARE, 11)));
    let _ = l.register(cell(3, OP_ENTER, t.enter(ST_ATEXIT, 40)));
    let _ = l.register(cell(4, OP_ENTER, t.enter(0, 1)));
    let _ = l.register(cell(5, OP_ENTER, t.enter(ST_DTOR, 30)));
    let _ = l.register(cell(6, OP_ENTER, t.enter(ST_ATEXIT, 40)));
    let _ = l.register(cell(7, OP_ENTER, t.enter(ST_FINAL, 50)));
    let _ = l.register(cell(8, OP_CLOSE, t.close()));
    let _ = l.register(cell(9, OP_TOTAL, Ok(t.detail_total() as usize)));
    let _ = l.register(cell(10, OP_MISS, Ok(t.missing())));
    let _ = l.register(cell(11, OP_ENTER, t.enter(ST_FLUSH, 21)));

    let mut u = Trace::new();
    let _ = u.enter(ST_PREPARE, 1);
    let _ = l.register(cell(12, OP_ENTER, u.stage_at(4).map(|_| 0usize)));

    let mut w = Trace::new();
    let _ = l.register(cell(13, OP_CLOSE, w.close()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：五阶全走通，轨迹逐条可对。
    let mut a = Trace::new();
    let e1 = a.enter(ST_PREPARE, 10);
    let e2 = a.enter(ST_FLUSH, 20);
    let e3 = a.enter(ST_DTOR, 30);
    let e4 = a.enter(ST_ATEXIT, 40);
    let e5 = a.enter(ST_FINAL, 50);
    let cl = a.close();
    out[1] = e1 == Ok(1)
        && e2 == Ok(2)
        && e3 == Ok(3)
        && e4 == Ok(4)
        && e5 == Ok(5)
        && cl == Ok(5)
        && a.is_sealed()
        && a.last_stage() == ST_FINAL
        && a.missing() == 0
        && a.detail_total() == 150
        && a.stage_at(0) == Ok(ST_PREPARE as u32)
        && a.stage_at(4) == Ok(ST_FINAL as u32)
        && a.detail_at(3) == Ok(40)
        && a.stage_seen(ST_DTOR)
        && !a.stage_seen(0)
        && a.recount() == Ok(5);

    // 二路：重复与跳阶各自归因，失败后仍可继续推进。
    let mut b = Trace::new();
    let _ = b.enter(ST_PREPARE, 1);
    let _ = b.enter(ST_FLUSH, 2);
    let dup = b.enter(ST_FLUSH, 3);
    let skip = b.enter(ST_ATEXIT, 4);
    let next = b.enter(ST_DTOR, 5);
    let continue_ok = b.enter(ST_ATEXIT, 6);
    out[2] = dup == Err(PT_EDUP)
        && skip == Err(PT_ESKIP)
        && next == Ok(3)
        && continue_ok == Ok(4)
        && b.last_stage() == ST_ATEXIT
        && b.missing() == 1
        && b.detail_total() == 1 + 2 + 5 + 6
        && b.stage_at(2) == Ok(ST_DTOR as u32);

    // 三路：闸门（非法阶 / 未到终阶 / 封存后）。
    let mut c = Trace::new();
    let g1 = c.enter(0, 1) == Err(PT_ESTAGE);
    let g2 = c.enter((STAGES + 1) as u8, 1) == Err(PT_ESTAGE);
    let g3 = c.enter(ST_FINAL, 1) == Err(PT_ESKIP);
    let g4 = c.close() == Err(PT_ENOTFINAL);
    let g5 = c.enter(ST_PREPARE, 0) == Err(PT_EDETAIL);
    let g6 = c.enter(ST_PREPARE, 7) == Ok(1);
    let _ = c.enter(ST_FLUSH, 8);
    let _ = c.enter(ST_DTOR, 9);
    let _ = c.enter(ST_ATEXIT, 10);
    let _ = c.enter(ST_FINAL, 11);
    let g7 = c.close() == Ok(5);
    let g8 = c.close() == Err(PT_ESEALED);
    let g9 = c.enter(ST_FLUSH, 12) == Err(PT_ESEALED);
    let g10 = c.stage_at(5) == Err(PT_ENOBS);
    let g11 = c.detail_at(5) == Err(PT_ENOBS);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9 && g10 && g11;

    out
}

/// 十三项判据。
pub fn run_unx11339_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11339.exit_path_trace");
    let l = build_ledger();

    s.add_dyn(
        "PT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PT2-阶段推进计数",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "PT3-重复进入被拒",
        l.sample(2).map(|x| x.code) == Some(PT_EDUP),
        "static",
    );

    s.add(
        "PT4-跳阶被拒",
        l.sample(3).map(|x| x.code) == Some(PT_ESKIP),
        "static",
    );

    s.add(
        "PT5-非法阶号被拒",
        l.sample(4).map(|x| x.code) == Some(PT_ESTAGE),
        "static",
    );

    s.add(
        "PT6-补齐中段与终段",
        l.sample(5).map(|x| x.value) == Some(3)
            && l.sample(6).map(|x| x.value) == Some(4)
            && l.sample(7).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "PT7-封存返回条目数",
        l.sample(8).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "PT8-明细累加",
        l.sample(9).map(|x| x.value) == Some(150),
        "static",
    );

    s.add(
        "PT9-缺阶计数归零",
        l.sample(10).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "PT10-封存后进入被拒",
        l.sample(11).map(|x| x.code) == Some(PT_ESEALED),
        "static",
    );

    s.add(
        "PT11-轨迹下标越界",
        l.sample(12).map(|x| x.code) == Some(PT_ENOBS),
        "static",
    );

    s.add(
        "PT12-未到终阶不能封存",
        l.sample(13).map(|x| x.code) == Some(PT_ENOTFINAL),
        "static",
    );

    s.add(
        "PT13-冻结重放四注入",
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
        let s = run_unx11339_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn stages_advance_in_order() {
        let mut t = Trace::new();
        assert_eq!(t.enter(ST_PREPARE, 1), Ok(1));
        assert_eq!(t.enter(ST_FLUSH, 2), Ok(2));
        assert_eq!(t.enter(ST_DTOR, 3), Ok(3));
        assert_eq!(t.enter(ST_ATEXIT, 4), Ok(4));
        assert_eq!(t.enter(ST_FINAL, 5), Ok(5));
        assert_eq!(t.missing(), 0);
        assert_eq!(t.detail_total(), 15);
        assert_eq!(t.close(), Ok(5));
        assert_eq!(t.recount(), Ok(5));
    }

    #[test]
    fn duplicate_beats_skip() {
        let mut t = Trace::new();
        let _ = t.enter(ST_PREPARE, 1);
        assert_eq!(t.enter(ST_PREPARE, 2), Err(PT_EDUP));
        assert_eq!(t.enter(ST_FINAL, 3), Err(PT_ESKIP));
        assert_eq!(t.last_stage(), ST_PREPARE);
    }

    #[test]
    fn close_needs_final() {
        let mut t = Trace::new();
        assert_eq!(t.close(), Err(PT_ENOTFINAL));
        let _ = t.enter(ST_PREPARE, 1);
        assert_eq!(t.close(), Err(PT_ENOTFINAL));
        assert!(!t.is_sealed());
    }

    #[test]
    fn sealed_trace_is_read_only() {
        let mut t = Trace::new();
        let mut st = 1u8;
        while (st as usize) <= STAGES {
            assert!(t.enter(st, st as u32).is_ok());
            st += 1;
        }
        assert_eq!(t.close(), Ok(5));
        assert_eq!(t.enter(ST_PREPARE, 1), Err(PT_ESEALED));
        assert_eq!(t.close(), Err(PT_ESEALED));
    }

    #[test]
    fn invalid_stage_and_detail() {
        let mut t = Trace::new();
        assert_eq!(t.enter(0, 1), Err(PT_ESTAGE));
        assert_eq!(t.enter(9, 1), Err(PT_ESTAGE));
        assert_eq!(t.enter(ST_PREPARE, 0), Err(PT_EDETAIL));
        assert_eq!(t.len(), 0);
        assert!(!t.stage_seen(9));
        assert_eq!(t.stage_at(0), Err(PT_ENOBS));
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
