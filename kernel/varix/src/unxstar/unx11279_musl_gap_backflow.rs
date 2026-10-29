//! UNX-F11279 musl 内核缺口回流账（0x1f4f0 段）
//!
//! 缺口条目只认三要素：**现象**（症状码 1..=5）、**最小样本**（字节数非零，
//! 附样本摘要）、**缺陷号**（非零）。三要素之外再记严重度（1..=4）、
//! 流转状态（0 开放 / 1 已分诊 / 2 已修复）与责任方代号（非零）。
//! 判据函数按固定序取第一条缺项：现象 > 样本 > 缺陷号 > 严重度 > 状态 > 责任方。
//! 「回流零遗漏」由两张表保证：状态表把每条样本按 0/1/2/非法 落槽，
//! 合计必须等于登记条数；另要求**每条已修复条目**都带齐缺陷号、责任方与
//! 样本长度——修复单不许无缺陷号裸奔。
//! 状态 3 是故意留的非法态样本，判据函数据此产 GV_STATE，登记面照收。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、封册后写账；
//! 症状码越界（如 9）、样本长度为 0、严重度 5 都只是样本，不拒收。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;

/// 诊断段（0x1f4f0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f4f0;

/// 条目 id 越界。
pub const GP_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const GP_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const GP_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const GP_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const GP_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const GP_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算判据与登记时不符。
pub const GP_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const GP_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const GP_EDOBS: u32 = SEG_BASE + 0x09;
/// 状态表/尝试表不平。
pub const GP_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：现象缺失或越界。
pub const GV_SYM: u32 = 1;
/// 判据码：最小样本缺失。
pub const GV_SAMPLE: u32 = 2;
/// 判据码：缺陷号缺失。
pub const GV_DEFECT: u32 = 3;
/// 判据码：严重度越界。
pub const GV_SEV: u32 = 4;
/// 判据码：状态非法。
pub const GV_STATE: u32 = 5;
/// 判据码：责任方缺失。
pub const GV_OWNER: u32 = 6;

/// 症状码上界（1..=5 合法）。
pub const SYM_MAX: u32 = 5;
/// 严重度上界（1..=4 合法）。
pub const SEV_MAX: u32 = 4;
/// 状态：开放。
pub const ST_OPEN: u32 = 0;
/// 状态：已分诊。
pub const ST_TRIAGED: u32 = 1;
/// 状态：已修复。
pub const ST_FIXED: u32 = 2;
/// 状态表槽数（0/1/2/非法）。
pub const ST_SLOTS: usize = 4;
/// 回流账基准日期（YYYYMMDD）。
pub const BACKFLOW_DATE: u32 = 2026_1030;
/// 条目表定容。
pub const CAP: usize = 12;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_79;

/// 一条缺口样本：症状 + 样本长 + 缺陷号 + 严重度 + 状态 + 责任方 + 摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    pub id: u32,
    pub sym: u32,
    pub sample_len: u32,
    pub defect: u32,
    pub sev: u32,
    pub state: u32,
    pub owner: u32,
    pub hash: u64,
}

impl Gap {
    pub const fn blank() -> Gap {
        Gap { id: 0, sym: 0, sample_len: 0, defect: 0, sev: 0, state: 0, owner: 0, hash: 0 }
    }

    pub fn render(&self) -> String {
        format!("G{} sym{} n{} bug#{} sev{} st{} own{}",
            self.id, self.sym, self.sample_len, self.defect, self.sev, self.state, self.owner)
    }
}

/// 纯判据：现象 → 样本 → 缺陷号 → 严重度 → 状态 → 责任方。
pub fn gap_verdict(g: &Gap) -> u32 {
    if g.sym == 0 || g.sym > SYM_MAX {
        return GV_SYM;
    }
    if g.sample_len == 0 {
        return GV_SAMPLE;
    }
    if g.defect == 0 {
        return GV_DEFECT;
    }
    if g.sev == 0 || g.sev > SEV_MAX {
        return GV_SEV;
    }
    if g.state > ST_FIXED {
        return GV_STATE;
    }
    if g.owner == 0 {
        return GV_OWNER;
    }
    0
}

/// 状态槽下标：0/1/2 原样，非法态落第 3 槽。
pub fn state_slot(state: u32) -> usize {
    if state <= ST_FIXED {
        state as usize
    } else {
        ST_SLOTS - 1
    }
}

/// 缺口回流账本：样本 + 判据 + 七槽直方 + 四槽状态表。
#[derive(Clone, Copy, Debug)]
pub struct GapLedger {
    cases: [Gap; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 7],
    states: [u32; ST_SLOTS],
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl GapLedger {
    pub const fn new() -> GapLedger {
        GapLedger {
            cases: [Gap::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 7],
            states: [0u32; ST_SLOTS],
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
    pub fn register(&mut self, g: Gap) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(GP_EFROZEN));
        }
        if g.id < ID_MIN || g.id > ID_MAX {
            return Err(self.fail(GP_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == g.id {
                    return Err(self.fail(GP_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if g.id <= last.id {
                    return Err(self.fail(GP_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(GP_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = g;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：直方与状态表同步落槽。
    pub fn register_ruled(&mut self, g: Gap) -> Result<(usize, u32), u32> {
        let idx = self.register(g)?;
        let v = gap_verdict(&g);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if let Some(s) = self.states.get_mut(state_slot(g.state)) {
            *s = s.wrapping_add(1);
        }
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(GP_EFROZEN);
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
            None => return Err(GP_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(GP_EDOBS),
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
    pub fn hist_at(&self, i: usize) -> u32 {
        match self.hist.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn hist_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 7 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn state_at(&self, i: usize) -> u32 {
        match self.states.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn state_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < ST_SLOTS {
            if let Some(v) = self.states.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn zero_verdicts(&self) -> u32 {
        self.hist_at(0)
    }
    pub fn case_at(&self, i: usize) -> Option<Gap> {
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

    /// 已修复条目里三要素不齐的条数（回流零遗漏的反向证据，应为 0）。
    pub fn fixed_unbacked(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.len {
            if let Some(g) = self.cases.get(k) {
                if g.state == ST_FIXED
                    && (g.defect == 0 || g.owner == 0 || g.sample_len == 0)
                {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// FNV-1a：样本全字段 + 判据 + 观测位 + 直方 + 状态表 + 计数器。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.attempts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.observed as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.matched as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frozen as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.len {
            if let Some(c) = self.cases.get(k) {
                h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.sym as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.sample_len as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.defect as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.sev as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.state as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.owner as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.hash).wrapping_mul(FNV_PRIME);
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
        while j < 7 {
            if let Some(v) = self.hist.get(j) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        let mut t = 0usize;
        while t < ST_SLOTS {
            if let Some(v) = self.states.get(t) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            t = t.wrapping_add(1);
        }
        h
    }

    /// 复账：全登记、全观测、全复算、直方与状态表双平衡、修复单可回流。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 {
            return Err(GP_EUNOBS);
        }
        if self.observed as usize != self.len || self.matched as usize != self.len {
            return Err(GP_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(GP_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 || self.state_sum() != self.len as u32 {
            return Err(GP_EBALANCE);
        }
        if self.fixed_unbacked() != 0 {
            return Err(GP_EBALANCE);
        }
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(GP_EUNOBS),
            };
            let v = match self.verdicts.get(k) {
                Some(x) => *x,
                None => return Err(GP_EMISMATCH),
            };
            if gap_verdict(&c) != v {
                return Err(GP_EMISMATCH);
            }
            match self.seen.get(k) {
                Some(true) => {}
                _ => return Err(GP_EUNOBS),
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：复账后上锁，二次封册即 EFROZEN。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(GP_EFROZEN);
        }
        let h = self.replay()?;
        self.frozen = true;
        self.root = Some(h);
        Ok(h)
    }
}

/// 构造一条缺口样本（id 由调用方补）。
pub fn gap(sym: u32, sample_len: u32, defect: u32, sev: u32, state: u32, owner: u32, hash: u64) -> Gap {
    Gap { id: 1, sym, sample_len, defect, sev, state, owner, hash }
}

/// 十二条规格样本：三条挂账成功、九条逐项缺口。
pub fn spec_of(id: usize) -> (Gap, u32) {
    match id {
        1 => (gap(1, 40, 9001, 3, ST_OPEN, 7, 0x7901), 0),
        2 => (gap(0, 40, 9001, 3, ST_OPEN, 7, 0x7902), GV_SYM),
        3 => (gap(1, 0, 9001, 3, ST_OPEN, 7, 0x7903), GV_SAMPLE),
        4 => (gap(1, 40, 0, 3, ST_OPEN, 7, 0x7904), GV_DEFECT),
        5 => (gap(1, 40, 9001, 0, ST_OPEN, 7, 0x7905), GV_SEV),
        6 => (gap(1, 40, 9001, SEV_MAX + 1, ST_OPEN, 7, 0x7906), GV_SEV),
        7 => (gap(1, 40, 9001, 3, ST_FIXED + 1, 7, 0x7907), GV_STATE),
        8 => (gap(1, 40, 9001, 3, ST_OPEN, 0, 0x7908), GV_OWNER),
        9 => (gap(SYM_MAX + 4, 40, 9001, 3, ST_OPEN, 7, 0x7909), GV_SYM),
        10 => (gap(1, 0, 0, 3, ST_OPEN, 7, 0x790a), GV_SAMPLE),
        11 => (gap(3, 128, 9002, 2, ST_FIXED, 4, 0x790b), 0),
        12 => (gap(4, 64, 9003, 1, ST_TRIAGED, 2, 0x790c), 0),
        _ => (Gap::blank(), GP_EID),
    }
}

/// 逐条登记 + 观测。
pub fn build_ledger() -> GapLedger {
    let mut l = GapLedger::new();
    let mut i = 1usize;
    while i <= CAP {
        let (mut g, expect) = spec_of(i);
        g.id = i as u32;
        if l.register_ruled(g).is_ok() {
            let _ = l.observe(g.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、越界、封册。
pub fn inject_four() -> [bool; 4] {
    let mut a = GapLedger::new();
    let mut hi = gap(1, 40, 1, 1, ST_OPEN, 1, 0x81);
    hi.id = 2;
    let _ = a.register(hi);
    let mut lo = gap(1, 40, 1, 1, ST_OPEN, 1, 0x82);
    lo.id = 1;
    let bad_a = a.register(lo).err() == Some(GP_EORDER);

    let mut b = GapLedger::new();
    let g = gap(1, 40, 1, 1, ST_OPEN, 1, 0x83);
    let _ = b.register(g);
    let bad_b = b.register(g).err() == Some(GP_EDUP);

    let mut c = GapLedger::new();
    let mut wide = gap(1, 40, 1, 1, ST_OPEN, 1, 0x84);
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(GP_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = gap(1, 40, 1, 1, ST_OPEN, 1, 0x85);
    late.id = 13;
    let bad_d = d.register(late).err() == Some(GP_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压三要素挂账与回流表，后四项压拒绝/观测/封册。
pub fn run_unx11279_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11279");

    let layout = SEG_BASE == 0x1f4f0
        && GP_EID == SEG_BASE + 0x01
        && GP_EORDER == SEG_BASE + 0x02
        && GP_EDUP == SEG_BASE + 0x03
        && GP_EFULL == SEG_BASE + 0x04
        && GP_EFROZEN == SEG_BASE + 0x05
        && GP_EUNSEEN == SEG_BASE + 0x06
        && GP_EMISMATCH == SEG_BASE + 0x07
        && GP_EUNOBS == SEG_BASE + 0x08
        && GP_EDOBS == SEG_BASE + 0x09
        && GP_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1f4f0 段十码连续");

    let tri = gap_verdict(&gap(0, 40, 9001, 3, ST_OPEN, 7, 1)) == GV_SYM
        && gap_verdict(&gap(1, 0, 9001, 3, ST_OPEN, 7, 1)) == GV_SAMPLE
        && gap_verdict(&gap(1, 40, 0, 3, ST_OPEN, 7, 1)) == GV_DEFECT
        && gap_verdict(&gap(1, 40, 9001, 3, ST_OPEN, 7, 1)) == 0;
    s.add("S2-现象样本缺陷号三要素挂账", tri,
        "三要素缺一各得专码，齐备才放行（100% 挂账）");

    let sev = gap_verdict(&gap(1, 40, 1, 0, ST_OPEN, 1, 1)) == GV_SEV
        && gap_verdict(&gap(1, 40, 1, SEV_MAX, ST_OPEN, 1, 1)) == 0
        && gap_verdict(&gap(1, 40, 1, SEV_MAX + 1, ST_OPEN, 1, 1)) == GV_SEV;
    s.add("S3-严重度域 1 到 4", sev, "0 与 5 越界，1..=4 在域");

    let st = gap_verdict(&gap(1, 40, 1, 1, ST_OPEN, 1, 1)) == 0
        && gap_verdict(&gap(1, 40, 1, 1, ST_TRIAGED, 1, 1)) == 0
        && gap_verdict(&gap(1, 40, 1, 1, ST_FIXED, 1, 1)) == 0
        && gap_verdict(&gap(1, 40, 1, 1, ST_FIXED + 1, 1, 1)) == GV_STATE
        && state_slot(ST_FIXED + 9) == ST_SLOTS - 1;
    s.add("S4-流转状态三态加非法槽", st,
        "开放/分诊/修复放行，非法态落第 4 槽并判 GV_STATE");

    let own = gap_verdict(&gap(1, 40, 1, 1, ST_OPEN, 0, 1)) == GV_OWNER
        && gap_verdict(&gap(1, 40, 1, 1, ST_OPEN, 9, 1)) == 0;
    s.add("S5-责任方非零门", own, "无主缺口不挂账，有主即通过");

    let prio = gap_verdict(&gap(0, 0, 0, 0, ST_FIXED + 1, 0, 0)) == GV_SYM
        && gap_verdict(&gap(1, 0, 0, 0, ST_FIXED + 1, 0, 0)) == GV_SAMPLE
        && gap_verdict(&gap(1, 1, 0, 0, ST_FIXED + 1, 0, 0)) == GV_DEFECT
        && gap_verdict(&gap(1, 1, 1, 0, ST_FIXED + 1, 0, 0)) == GV_SEV
        && gap_verdict(&gap(1, 1, 1, 1, ST_FIXED + 1, 0, 0)) == GV_STATE
        && gap_verdict(&gap(1, 1, 1, 1, ST_OPEN, 0, 0)) == GV_OWNER;
    s.add("S6-六缺口逐级优先序", prio,
        "现象 > 样本 > 缺陷号 > 严重度 > 状态 > 责任方，一次只报最前");

    let led = build_ledger();
    let st_bal = led.len() == 12
        && led.zero_verdicts() == 3
        && led.hist_at(GV_SYM as usize) == 2
        && led.hist_at(GV_SAMPLE as usize) == 2
        && led.hist_at(GV_DEFECT as usize) == 1
        && led.hist_at(GV_SEV as usize) == 2
        && led.hist_at(GV_STATE as usize) == 1
        && led.hist_at(GV_OWNER as usize) == 1
        && led.hist_sum() == 12
        && led.state_at(ST_OPEN as usize) == 9
        && led.state_at(ST_TRIAGED as usize) == 1
        && led.state_at(ST_FIXED as usize) == 1
        && led.state_at(ST_SLOTS - 1) == 1
        && led.state_sum() == 12;
    s.add("S7-直方与状态表双平衡", st_bal,
        "12 条 = 挂账 3 + 缺口 9；状态 9/1/1/1 合计 12");

    let mut fixed_ok = led.fixed_unbacked() == 0;
    let mut k = 0usize;
    while k < led.len() {
        if let Some(g) = led.case_at(k) {
            if g.state == ST_FIXED && (g.defect == 0 || g.owner == 0 || g.sample_len == 0) {
                fixed_ok = false;
            }
        }
        k = k.wrapping_add(1);
    }
    s.add("S8-已修条目回流零遗漏", fixed_ok && led.state_at(ST_FIXED as usize) == 1,
        "每条修复单都带缺陷号、责任方与最小样本");

    s.add("S9-全观测且判据复算一致", led.replay().is_ok()
        && led.observed() == 12 && led.matched() == 12,
        "十二条逐条复算并观测对齐，修复单可回溯");

    let mut spec_ok = true;
    let mut j = 1usize;
    while j <= CAP {
        let (mut g, want) = spec_of(j);
        g.id = j as u32;
        if gap_verdict(&g) != want {
            spec_ok = false;
        }
        j = j.wrapping_add(1);
    }
    s.add("S10-十二条规格自洽", spec_ok
        && BACKFLOW_DATE == 2026_1030
        && spec_of(7).0.render().contains("st3"),
        "规格期望与判据函数逐条一致，回流基准日固定");

    let mut g = GapLedger::new();
    let mut c2 = gap(1, 40, 1, 1, ST_OPEN, 1, 1);
    c2.id = 2;
    let _ = g.register(c2);
    let mut c1 = gap(1, 40, 1, 1, ST_OPEN, 1, 1);
    c1.id = 1;
    let bad_a = g.register(c1).err() == Some(GP_EORDER);
    let bad_b = g.register(c2).err() == Some(GP_EDUP);
    let mut wide = gap(1, 40, 1, 1, ST_OPEN, 1, 1);
    wide.id = 0;
    let bad_c = g.register(wide).err() == Some(GP_EID);
    let mut full = build_ledger();
    let mut extra = gap(1, 40, 1, 1, ST_OPEN, 1, 1);
    extra.id = 13;
    let bad_d = full.register(extra).err() == Some(GP_EFULL);
    s.add("S11-顺序重复越界定容四闸", bad_a && bad_b && bad_c && bad_d
        && g.accepts() == 1 && g.rejects() == 3 && g.attempts() == 4
        && inject_four() == [true, true, true, true],
        "四闸独立触发且尝试数配平");

    let mut ob = GapLedger::new();
    let mut one = gap(1, 40, 1, 1, ST_OPEN, 1, 1);
    one.id = 1;
    let _ = ob.register_ruled(one);
    let miss = ob.replay().err() == Some(GP_EUNOBS);
    let unseen = ob.observe(9, 0).err() == Some(GP_EUNSEEN);
    let wrong = ob.observe(1, GV_SEV) == Ok(false);
    let dob = ob.observe(1, 0).err() == Some(GP_EDOBS);
    s.add("S12-观测面三门", miss && unseen && wrong && dob,
        "未观测 EUNOBS、未登记 EUNSEEN、错观测 false、重观测 EDOBS");

    let mut fz = build_ledger();
    let root = match fz.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = fz.freeze();
    let mut late = gap(1, 40, 1, 1, ST_OPEN, 1, 1);
    late.id = 13;
    let post = fz.register(late).err();
    s.add_dyn("S13-封册先复账后上锁", root != 0
        && again == Err(GP_EFROZEN)
        && post == Some(GP_EFROZEN)
        && fz.root() == Some(root)
        && fz.replay().is_ok(),
        &format!("根={:x} 二次={:?} 封后写={:?}", root, again, post));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11279_tri_element() {
        assert_eq!(gap_verdict(&gap(1, 40, 9001, 3, ST_OPEN, 7, 1)), 0);
        assert_eq!(gap_verdict(&gap(0, 40, 9001, 3, ST_OPEN, 7, 1)), GV_SYM);
        assert_eq!(gap_verdict(&gap(1, 0, 9001, 3, ST_OPEN, 7, 1)), GV_SAMPLE);
        assert_eq!(gap_verdict(&gap(1, 40, 0, 3, ST_OPEN, 7, 1)), GV_DEFECT);
        assert_eq!(gap_verdict(&gap(0, 0, 0, 0, 9, 0, 0)), GV_SYM);
    }

    #[test]
    fn unx11279_state_and_sev() {
        assert_eq!(state_slot(ST_OPEN), 0);
        assert_eq!(state_slot(ST_FIXED), 2);
        assert_eq!(state_slot(77), 3);
        assert_eq!(gap_verdict(&gap(1, 8, 1, SEV_MAX, ST_TRIAGED, 1, 1)), 0);
        assert_eq!(gap_verdict(&gap(1, 8, 1, SEV_MAX + 1, ST_TRIAGED, 1, 1)), GV_SEV);
        assert_eq!(gap_verdict(&gap(1, 8, 1, 1, ST_FIXED + 1, 1, 1)), GV_STATE);
        assert_eq!(gap_verdict(&gap(1, 8, 1, 1, ST_OPEN, 0, 1)), GV_OWNER);
    }

    #[test]
    fn unx11279_ledger_balance() {
        let l = build_ledger();
        assert_eq!(l.len(), 12);
        assert_eq!(l.zero_verdicts(), 3);
        assert_eq!(l.hist_sum(), 12);
        assert_eq!(l.state_sum(), 12);
        assert_eq!(l.state_at(0), 9);
        assert_eq!(l.state_at(2), 1);
        assert_eq!(l.fixed_unbacked(), 0);
        assert!(l.replay().is_ok());
    }

    #[test]
    fn unx11279_refusals() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = GapLedger::new();
        let g = gap(1, 8, 1, 1, ST_OPEN, 1, 1);
        assert!(l.register(g).is_ok());
        assert_eq!(l.register(g).err(), Some(GP_EDUP));
        assert_eq!(l.attempts(), 2);
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn unx11279_freeze_and_checks() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(root != 0);
        assert_eq!(l.freeze(), Err(GP_EFROZEN));
        assert!(l.replay().is_ok());
        let s = run_unx11279_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
