//! UNX-F11183 域 800 条满条终对账（四十批×20 终验）（0x1f3a0 段）
//!
//! 纯功能模型：八百单按四十批归档，每批二十格（`present` 位图一格一位）。
//! 终对账做三件事——批面必须满格、批行数不得低于两万五的下限线、
//! 批判据数必须恰为二百六十（20 × 13）；三面全平才允许在本批记账。
//!
//! 第二本账是「单值账」：四十批行数求和后除以八百，商必须整除、余数为零，
//! 且商不得低于单卡下限二百五。整除、余数、下限三者任一不成立即单值不成立。
//!
//! 第三本账是「批链」：每批的批摘要按前一批递推，四十环首尾相接；
//! 任一批摘要与重算结果不符即断链，终对账不得封盘。
//!
//! 规则面：
//!   - 批号越界落 EINVAL；
//!   - 批面未满格、行数低于下限、判据数不符，分别有独立拒码；
//!   - 单值整除且不低于下限才算「800 单值预备成立」。
//!
//! 拒止面：批号越界、重复登记、批序倒挂、未登记即观测、缺测、
//! 不符、断链、冻结后再动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f3a0;
/// 批号越界
pub const DN_EBATCH: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const DN_EDUP: u32 = SEG_BASE + 0x02;
/// 批序倒挂
pub const DN_EORDER: u32 = SEG_BASE + 0x03;
/// 批面未满格
pub const DN_EMASK: u32 = SEG_BASE + 0x04;
/// 未登记即观测
pub const DN_EUNREG: u32 = SEG_BASE + 0x05;
/// 未测即重放
pub const DN_ESHORT: u32 = SEG_BASE + 0x06;
/// 实测与期望不符
pub const DN_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 批链断裂
pub const DN_ECHAIN: u32 = SEG_BASE + 0x08;
/// 已封盘
pub const DN_EFROZEN: u32 = SEG_BASE + 0x09;
/// 行数或判据数不足
pub const DN_ESHORTLINE: u32 = SEG_BASE + 0x0A;

/// 批数。
pub const BATCHES: usize = 40;
/// 每批格数。
pub const PER_BATCH: usize = 20;
/// 总单数。
pub const CARDS: usize = BATCHES * PER_BATCH;
/// 单卡行数下限。
pub const MIN_LINES: u16 = 250;
/// 单卡判据数。
pub const CHECKS_PER_CARD: u8 = 13;
/// 单批行数下限。
pub const BATCH_MIN_LINES: u32 = (MIN_LINES as u32) * (PER_BATCH as u32);
/// 单批判据总数。
pub const BATCH_CHECKS: u32 = (CHECKS_PER_CARD as u32) * (PER_BATCH as u32);
/// 满格位图：低二十位全置。
pub const FULL_MASK: u32 = (1u32 << PER_BATCH) - 1;
/// 单值下限（与单卡下限同值）。
pub const UNIT_MIN: u32 = MIN_LINES as u32;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 规则纯函数：第 `batch` 批应有的（批面, 行数, 判据数）。
pub fn rule_batch(batch: u8) -> (u32, u32, u32) {
    if batch as usize >= BATCHES {
        return (0, 0, 0);
    }
    let lines = BATCH_MIN_LINES.wrapping_add((batch as u32 % 4).wrapping_mul(40));
    (FULL_MASK, lines, BATCH_CHECKS)
}

/// 规则判码：批面/行数/判据数三面对平才回 0。
pub fn rule_error(batch: u8, present: u32, lines: u32, checks: u32) -> u32 {
    if batch as usize >= BATCHES {
        return DN_EBATCH;
    }
    if present != FULL_MASK {
        return DN_EMASK;
    }
    if lines < BATCH_MIN_LINES || checks != BATCH_CHECKS {
        return DN_ESHORTLINE;
    }
    let (_, want_lines, want_checks) = rule_batch(batch);
    if lines != want_lines || checks != want_checks {
        return DN_EMISMATCH;
    }
    0
}

/// 批面置位数。
pub fn mask_bits(mask: u32) -> usize {
    let mut n = 0usize;
    let mut m = mask & FULL_MASK;
    while m != 0 {
        m &= m.wrapping_sub(1);
        n = n.wrapping_add(1);
    }
    n
}

/// FNV-1a 单步。
pub fn fnv_step(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x ^= ((v >> (i * 8)) & 0xFF) as u64;
        x = x.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    x
}

/// 单批对账行。
#[derive(Clone, Copy, Debug)]
pub struct BatchRow {
    /// 批号。
    pub batch: u8,
    /// 批面位图（二十格一位）。
    pub present: u32,
    /// 批行数。
    pub lines: u32,
    /// 批判据数。
    pub checks: u32,
    /// 规则行数。
    pub want_lines: u32,
    /// 规则判据数。
    pub want_checks: u32,
    /// 批链摘要。
    pub chain: u64,
    /// 是否已登记。
    pub live: bool,
    /// 是否已观测。
    pub seen: bool,
}

impl BatchRow {
    /// 空行。
    pub const fn blank() -> Self {
        BatchRow {
            batch: 0,
            present: 0,
            lines: 0,
            checks: 0,
            want_lines: 0,
            want_checks: 0,
            chain: 0,
            live: false,
            seen: false,
        }
    }

    /// 批面是否满格。
    pub fn full(&self) -> bool {
        self.present == FULL_MASK
    }

    /// 三面是否与规则相平。
    pub fn agrees(&self) -> bool {
        self.full()
            && self.lines == self.want_lines
            && self.checks == self.want_checks
            && self.lines >= BATCH_MIN_LINES
    }
}

/// 终对账账本。
pub struct Tally {
    base: u64,
    rows: [BatchRow; BATCHES],
    cursor: usize,
    closed: usize,
    registered: u32,
    observed: u32,
    rejects: u32,
    frozen: bool,
}

impl Tally {
    /// 新账本。
    pub const fn new() -> Self {
        Tally {
            base: SEG_BASE as u64,
            rows: [BatchRow::blank(); BATCHES],
            cursor: 0,
            closed: 0,
            registered: 0,
            observed: 0,
            rejects: 0,
            frozen: false,
        }
    }

    /// 记一次拒止并回传码。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已登记批数。
    pub fn registered(&self) -> u32 {
        self.registered
    }

    /// 已观测批数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 已闭环批数。
    pub fn closed(&self) -> u32 {
        self.closed as u32
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 取批行。
    pub fn row(&self, batch: usize) -> Option<&BatchRow> {
        self.rows.get(batch)
    }

    /// 批链头：已闭环的最后一环，否则账本基底。
    pub fn chain_head(&self) -> u64 {
        if self.closed == 0 {
            return self.base;
        }
        match self.rows.get(self.closed - 1) {
            Some(r) => r.chain,
            None => self.base,
        }
    }

    /// 登记一批：规则行数与判据数在登记时落定，批链随之递推。
    pub fn register(
        &mut self,
        batch: u8,
        present: u32,
        lines: u32,
        checks: u32,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(DN_EFROZEN));
        }
        if batch as usize >= BATCHES {
            return Err(self.fail(DN_EBATCH));
        }
        let live = match self.rows.get(batch as usize) {
            Some(r) => r.live,
            None => return Err(self.fail(DN_EUNREG)),
        };
        if live {
            return Err(self.fail(DN_EDUP));
        }
        if batch as usize != self.cursor {
            return Err(self.fail(DN_EORDER));
        }
        let (want_mask, want_lines, want_checks) = rule_batch(batch);
        if present & !FULL_MASK != 0 || present == 0 {
            return Err(self.fail(DN_EMASK));
        }
        let _ = want_mask;
        let chain = fnv_step(fnv_step(self.chain_head(), lines), batch as u32);
        let r = match self.rows.get_mut(batch as usize) {
            Some(r) => r,
            None => return Err(self.fail(DN_EUNREG)),
        };
        *r = BatchRow {
            batch,
            present,
            lines,
            checks,
            want_lines,
            want_checks,
            chain,
            live: true,
            seen: false,
        };
        self.cursor = self.cursor.wrapping_add(1);
        self.closed = self.closed.wrapping_add(1);
        self.registered = self.registered.wrapping_add(1);
        Ok(())
    }

    /// 观测一批：三面任一不符先落账再拒。
    pub fn observe(
        &mut self,
        batch: u8,
        present: u32,
        lines: u32,
        checks: u32,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(DN_EFROZEN));
        }
        if batch as usize >= BATCHES {
            return Err(self.fail(DN_EBATCH));
        }
        let r = match self.rows.get_mut(batch as usize) {
            Some(r) => r,
            None => return Err(self.fail(DN_EUNREG)),
        };
        if !r.live {
            return Err(self.fail(DN_EUNREG));
        }
        if r.seen {
            return Err(self.fail(DN_EDUP));
        }
        r.seen = true;
        r.present = present;
        r.lines = lines;
        r.checks = checks;
        self.observed = self.observed.wrapping_add(1);
        if !r.agrees() {
            return Err(self.fail(DN_EMISMATCH));
        }
        Ok(())
    }
}

/// 账面度量与单值账。
impl Tally {
    /// 四十批行数总和。
    pub fn total_lines(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(r) = self.rows.get(i) {
                if r.live {
                    t = t.wrapping_add(r.lines as u64);
                }
            }
            i = i.wrapping_add(1);
        }
        t
    }

    /// 四十批判据数总和。
    pub fn total_checks(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(r) = self.rows.get(i) {
                if r.live {
                    t = t.wrapping_add(r.checks as u64);
                }
            }
            i = i.wrapping_add(1);
        }
        t
    }

    /// 满格批数。
    pub fn full_batches(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(r) = self.rows.get(i) {
                if r.live && r.full() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 满条批数：行数与判据数双达线。
    pub fn full_cards_batches(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(r) = self.rows.get(i) {
                if r.live && r.agrees() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 单值：总行数除以总单数。
    pub fn unit_value(&self) -> u32 {
        let total = self.total_lines();
        let cards = CARDS as u64;
        if cards == 0 {
            return 0;
        }
        (total / cards) as u32
    }

    /// 单值余数。
    pub fn unit_remainder(&self) -> u64 {
        let total = self.total_lines();
        let cards = CARDS as u64;
        if cards == 0 {
            return total;
        }
        total % cards
    }

    /// 800 单值预备成立：整除、余零、不低于下限。
    pub fn unit_ready(&self) -> bool {
        self.registered() == BATCHES as u32
            && self.unit_remainder() == 0
            && self.unit_value() >= UNIT_MIN
            && self.total_checks() == (BATCH_CHECKS as u64) * (BATCHES as u64)
    }

    /// 批链逐环重算。
    pub fn chain_ok(&self) -> bool {
        let mut head = self.base;
        let mut i = 0usize;
        while i < BATCHES {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => return false,
            };
            if !r.live || !r.seen || r.batch as usize != i {
                return false;
            }
            head = fnv_step(fnv_step(head, r.lines), i as u32);
            if r.chain != head {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 摘要：四十批逐行混入。
    pub fn fold(&self) -> u64 {
        let mut h = self.base;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(r) = self.rows.get(i) {
                h = fnv_step(h, r.batch as u32);
                h = fnv_step(h, r.present);
                h = fnv_step(h, r.lines);
                h = fnv_step(h, r.checks);
                h = fnv_step(h, (r.chain & 0xFFFF_FFFF) as u32);
                h = fnv_step(h, (r.chain >> 32) as u32);
                let fl = (r.live as u32) | ((r.seen as u32) << 1);
                h = fnv_step(h, fl);
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 终重放：四十批全登记全观测全相符、批链闭合、单值成立。
    pub fn replay(&self) -> Result<u64, u32> {
        let mut i = 0usize;
        while i < BATCHES {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => return Err(DN_EUNREG),
            };
            if !r.live {
                return Err(DN_EUNREG);
            }
            if !r.seen {
                return Err(DN_ESHORT);
            }
            if !r.full() {
                return Err(DN_EMASK);
            }
            if !r.agrees() {
                return Err(DN_EMISMATCH);
            }
            i = i.wrapping_add(1);
        }
        if !self.chain_ok() {
            return Err(DN_ECHAIN);
        }
        if !self.unit_ready() {
            return Err(DN_ESHORTLINE);
        }
        Ok(self.fold())
    }

    /// 封盘。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(DN_EFROZEN));
        }
        let d = match self.replay() {
            Ok(d) => d,
            Err(e) => return Err(self.fail(e)),
        };
        self.frozen = true;
        Ok(d)
    }
}

/// 批号 → 样本三面。
pub fn spec_batch(batch: u8) -> (u32, u32, u32) {
    rule_batch(batch)
}

/// 建一本完整账：四十批全部登记与观测。
pub fn build_tally() -> Tally {
    let mut t = Tally::new();
    let mut b = 0usize;
    while b < BATCHES {
        let (present, lines, checks) = spec_batch(b as u8);
        t.register(b as u8, present, lines, checks).ok();
        t.observe(b as u8, present, lines, checks).ok();
        b = b.wrapping_add(1);
    }
    t
}

/// 四向注入：批序倒挂、重复观测、越位批面、封盘后再登记。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = Tally::new();
    out[0] = a.register(1, FULL_MASK, BATCH_MIN_LINES, BATCH_CHECKS) == Err(DN_EORDER);
    let mut b = build_tally();
    out[1] = b.observe(0, FULL_MASK, BATCH_MIN_LINES, BATCH_CHECKS) == Err(DN_EDUP);
    let mut c = Tally::new();
    out[2] = c.register(0, FULL_MASK | 0x10_0000, BATCH_MIN_LINES, BATCH_CHECKS) == Err(DN_EMASK);
    let mut d = build_tally();
    let _ = d.freeze();
    out[3] = d.register(0, FULL_MASK, BATCH_MIN_LINES, BATCH_CHECKS) == Err(DN_EFROZEN);
    out
}

/// 判据集：N1–N13（条数终对平 10 条 + 单值 3 条）。
pub fn run_unx11183_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11183");
    let t = build_tally();

    s.add(
        "N1 四十批登记、观测、闭环齐备且零拒止",
        t.registered() == BATCHES as u32
            && t.observed() == BATCHES as u32
            && t.closed() == BATCHES as u32
            && t.rejects() == 0,
        "40/40/40 拒止 0",
    );

    let d = t.fold();
    s.add(
        "N2 终重放通过且摘要等于 fold",
        t.replay() == Ok(d),
        "replay == fold",
    );

    s.add(
        "N3 三面全平：四十批行数与判据数均与规则相符",
        t.full_cards_batches() == BATCHES as u32,
        "满条 40/40",
    );

    let mut bits_ok = true;
    let mut i = 0usize;
    while i < BATCHES {
        if let Some(r) = t.row(i) {
            if mask_bits(r.present) != PER_BATCH {
                bits_ok = false;
            }
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "N4 批面满格：每批二十格全置",
        bits_ok && t.full_batches() == BATCHES as u32,
        "20 × 40 格",
    );

    s.add(
        "N5 批行数下限：四十批均不低于五千",
        t.full_cards_batches() == BATCHES as u32 && BATCH_MIN_LINES == 5000,
        "5000 下限 40/40",
    );

    s.add(
        "N6 批判据数：四十批均恰为二百六十",
        t.total_checks() == (BATCH_CHECKS as u64) * (BATCHES as u64) && BATCH_CHECKS == 260,
        "260 × 40 = 10400",
    );

    s.add_dyn(
        "N7 总行数账面值 202400",
        t.total_lines() == 202_400,
        &format!("总行数 {}", t.total_lines()),
    );

    s.add_dyn(
        "N8 800 单值整除且不低于下限（253，余 0）",
        t.unit_value() == 253 && t.unit_remainder() == 0 && t.unit_ready(),
        &format!(
            "单值 {} 余 {}",
            t.unit_value(),
            t.unit_remainder()
        ),
    );

    s.add(
        "N9 批链四十环首尾相接",
        t.chain_ok() && t.chain_head() != t.base,
        "链 40/40",
    );

    let mut inj_ok = false;
    let mut partial = Tally::new();
    if partial
        .register(0, FULL_MASK & !1, BATCH_MIN_LINES, BATCH_CHECKS)
        .is_ok()
    {
        inj_ok = partial.observe(0, FULL_MASK & !1, BATCH_MIN_LINES, BATCH_CHECKS)
            == Err(DN_EMISMATCH);
    }
    s.add(
        "N10 残格批面在观测处被检出（DN_EMISMATCH）",
        inj_ok,
        "残格 → 不符",
    );

    let mut line_short = false;
    let mut ls = Tally::new();
    if ls
        .register(0, FULL_MASK, BATCH_MIN_LINES - 100, BATCH_CHECKS)
        .is_ok()
    {
        line_short = ls.observe(0, FULL_MASK, BATCH_MIN_LINES - 100, BATCH_CHECKS)
            == Err(DN_EMISMATCH);
    }
    s.add(
        "N11 行数低于下限在观测处被检出",
        line_short && rule_error(0, FULL_MASK, 4999, BATCH_CHECKS) == DN_ESHORTLINE,
        "短行 → DN_ESHORTLINE",
    );

    let mut o = Tally::new();
    let order = o.register(2, FULL_MASK, BATCH_MIN_LINES, BATCH_CHECKS) == Err(DN_EORDER);
    let mut dup = build_tally();
    let dup_ok = dup.observe(0, FULL_MASK, BATCH_MIN_LINES, BATCH_CHECKS) == Err(DN_EDUP);
    s.add(
        "N12 批序倒挂与重复观测均被拒",
        order && dup_ok,
        "EORDER / EDUP",
    );

    let inj = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < 4 {
        if !inj[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let mut fz = build_tally();
    let once = fz.freeze().is_ok() && fz.freeze() == Err(DN_EFROZEN);
    s.add(
        "N13 四向注入全真、封盘一次性",
        all && once,
        "注入 4/4 + 封盘",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tally_builds_clean() {
        let t = build_tally();
        assert_eq!(t.registered(), BATCHES as u32);
        assert_eq!(t.observed(), BATCHES as u32);
        assert_eq!(t.rejects(), 0);
        assert_eq!(t.replay(), Ok(t.fold()));
    }

    #[test]
    fn totals_and_unit_value() {
        let t = build_tally();
        assert_eq!(t.total_lines(), 202_400);
        assert_eq!(t.total_checks(), 10_400);
        assert_eq!(t.unit_value(), 253);
        assert_eq!(t.unit_remainder(), 0);
        assert!(t.unit_ready());
    }

    #[test]
    fn rule_edges() {
        assert_eq!(rule_error(0, FULL_MASK, BATCH_MIN_LINES, BATCH_CHECKS), 0);
        assert_eq!(rule_error(BATCHES as u8, FULL_MASK, 5000, 260), DN_EBATCH);
        assert_eq!(rule_error(0, FULL_MASK & !1, 5000, 260), DN_EMASK);
        assert_eq!(rule_error(0, FULL_MASK, 4999, 260), DN_ESHORTLINE);
        assert_eq!(rule_error(0, FULL_MASK, 5000, 259), DN_ESHORTLINE);
        assert_eq!(rule_error(1, FULL_MASK, 5000, 260), DN_EMISMATCH);
        assert_eq!(mask_bits(FULL_MASK), PER_BATCH);
    }

    #[test]
    fn refusal_gates() {
        let mut t = Tally::new();
        assert_eq!(
            t.register(1, FULL_MASK, 5000, 260),
            Err(DN_EORDER)
        );
        assert_eq!(t.register(0, 0, 5000, 260), Err(DN_EMASK));
        assert_eq!(t.register(0, FULL_MASK | 0x200_000, 5000, 260), Err(DN_EMASK));
        assert!(t.register(0, FULL_MASK, 5000, 260).is_ok());
        assert_eq!(t.register(0, FULL_MASK, 5000, 260), Err(DN_EDUP));
        assert_eq!(t.register(BATCHES as u8, FULL_MASK, 5000, 260), Err(DN_EBATCH));
    }

    #[test]
    fn short_and_frozen() {
        let mut t = Tally::new();
        assert_eq!(t.replay(), Err(DN_EUNREG));
        assert!(t.register(0, FULL_MASK, 5000, 260).is_ok());
        assert_eq!(t.replay(), Err(DN_ESHORT));
        assert_eq!(t.observe(0, FULL_MASK, 5000, 259), Err(DN_EMISMATCH));
        let mut f = build_tally();
        assert!(f.freeze().is_ok());
        assert_eq!(f.freeze(), Err(DN_EFROZEN));
        assert_eq!(
            f.observe(0, FULL_MASK, 5000, 260),
            Err(DN_EFROZEN)
        );
    }

    #[test]
    fn checks_all_pass() {
        let s = run_unx11183_checks();
        assert!(s.all_passed(), "{:?}", s);
    }
}
