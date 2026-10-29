//! UNX-F11258 装载完成入口上下文联签终验（0x1f3a0 段）
//!
//! 动态链接器把控制权交给程序入口之前，必须凑齐五要素：argc、argv[0]、
//! AT_PHDR、AT_ENTRY 与 AT_RANDOM。模型用五位掩码表示这五项是否到场，
//! 再逐项检查“到场但值为零”的冲突情形——标志位说有、值却是零，说明
//! 上游装配出错，必须拒绝。缺项不算冲突，但会让联签失败并返回缺项位图。
//! 五要素齐备且无冲突时，对 argc、argv0 长度、phdr、entry、base、seed
//! 做 FNV 折叠得到联签票号，票面同时记录入口地址与到场掩码。
//! 掩码里出现高位未知位直接拒绝，避免未来的第六要素悄悄漏检。
//! 账本登记十二个样本：三个可联签、三个缺项、五个冲突、一个未知位。
//! 冻结前重放到场计数、缺项总数、联签数与逐格票号一致性。
//! 结构面另有五道门：编号越界、重复格、乱序、空上下文与冻结后写入，
//! 全部计入拒绝计数而不扰动已登记的格位，保证账本在任何时刻都可复算。

use crate::checks::CheckSet;
use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x1f3a0;

/// 编号非法。
pub const CT_EID: u32 = SEG_BASE + 0x01;
/// 状态顺序错误。
pub const CT_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CT_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CT_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const CT_ENOBS: u32 = SEG_BASE + 0x05;
/// 观测结果不符。
pub const CT_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const CT_EBAL: u32 = SEG_BASE + 0x07;
/// 空上下文。
pub const CT_EEMPTY: u32 = SEG_BASE + 0x08;
/// 掩码含未知位。
pub const CT_EBITS: u32 = SEG_BASE + 0x09;
/// 到场标志与零值冲突。
pub const CT_ECONFLICT: u32 = SEG_BASE + 0x0a;
/// 要素缺项导致联签失败。
pub const CT_EENTRY: u32 = SEG_BASE + 0x0b;
/// 票号不一致。
pub const CT_ESEAL: u32 = SEG_BASE + 0x0c;

/// argc 到场。
pub const E_ARGC: u8 = 0x01;
/// argv0 到场。
pub const E_ARGV0: u8 = 0x02;
/// AT_PHDR 到场。
pub const E_PHDR: u8 = 0x04;
/// AT_ENTRY 到场。
pub const E_ENTRY: u8 = 0x08;
/// AT_RANDOM 到场。
pub const E_RANDOM: u8 = 0x10;
/// 五要素全集。
pub const E_ALL: u8 = 0x1f;

/// 判决：五要素齐备，可联签。
pub const V_SIGNED: u8 = 0;
/// 判决：要素缺项。
pub const V_MISSING: u8 = 1;
/// 判决：到场标志与零值冲突。
pub const V_CONFLICT: u8 = 2;
/// 判决：掩码含未知位。
pub const V_BITS: u8 = 3;

/// 样本个数。
pub const SAMPLES: usize = 12;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个入口上下文样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxCase {
    pub mask: u8,
    pub argc: u8,
    pub argv0_len: u8,
    pub phdr: u64,
    pub entry: u64,
    pub base: u64,
    pub seed: u64,
}

impl AuxCase {
    /// 空上下文。
    pub const fn blank() -> AuxCase {
        AuxCase { mask: 0, argc: 0, argv0_len: 0, phdr: 0, entry: 0, base: 0, seed: 0 }
    }

    /// 构造上下文。
    pub const fn make(
        mask: u8,
        argc: u8,
        argv0_len: u8,
        phdr: u64,
        entry: u64,
        base: u64,
        seed: u64,
    ) -> AuxCase {
        AuxCase { mask, argc, argv0_len, phdr, entry, base, seed }
    }

    /// 五要素是否全空。
    pub const fn is_blank(&self) -> bool {
        self.mask == 0
            && self.argc == 0
            && self.argv0_len == 0
            && self.phdr == 0
            && self.entry == 0
            && self.base == 0
            && self.seed == 0
    }
}

/// 联签票。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub sig: u64,
    pub entry: u64,
    pub present: u8,
}

impl Ticket {
    /// 空票。
    pub const fn none() -> Ticket {
        Ticket { sig: 0, entry: 0, present: 0 }
    }
}

/// 一次检查的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub present: u8,
    pub missing: u8,
    pub argc: u8,
    pub sig: u64,
}

impl Context {
    /// 空结论。
    pub const fn none() -> Context {
        Context { present: 0, missing: E_ALL, argc: 0, sig: 0 }
    }
}

/// 折叠一个 64 位字。
fn fold_word(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 上下文的整体折叠值。
pub fn fold_case(c: &AuxCase) -> u64 {
    let mut h = FNV_OFF;
    h = fold_word(h, c.mask as u64);
    h = fold_word(h, c.argc as u64);
    h = fold_word(h, c.argv0_len as u64);
    h = fold_word(h, c.phdr);
    h = fold_word(h, c.entry);
    h = fold_word(h, c.base);
    h = fold_word(h, c.seed);
    h
}

/// 缺项位图。
pub fn missing_of(c: &AuxCase) -> u8 {
    E_ALL & !c.mask
}

/// 冲突位图：标志位为真但值为零的要素。
pub fn conflict_of(c: &AuxCase) -> u8 {
    let mut m = 0u8;
    if c.mask & E_ARGC != 0 && c.argc == 0 {
        m |= E_ARGC;
    }
    if c.mask & E_ARGV0 != 0 && c.argv0_len == 0 {
        m |= E_ARGV0;
    }
    if c.mask & E_PHDR != 0 && c.phdr == 0 {
        m |= E_PHDR;
    }
    if c.mask & E_ENTRY != 0 && c.entry == 0 {
        m |= E_ENTRY;
    }
    if c.mask & E_RANDOM != 0 && c.seed == 0 {
        m |= E_RANDOM;
    }
    m
}

/// 判决一个样本。未知位优先，其次冲突，再次缺项。
pub fn verdict_of(c: &AuxCase) -> u8 {
    if c.mask & !E_ALL != 0 {
        return V_BITS;
    }
    if conflict_of(c) != 0 {
        return V_CONFLICT;
    }
    if missing_of(c) != 0 {
        return V_MISSING;
    }
    V_SIGNED
}

/// 联签：五要素齐备且无冲突时出票。
pub fn sign(c: &AuxCase) -> Result<Ticket, u32> {
    match verdict_of(c) {
        V_BITS => Err(CT_EBITS),
        V_CONFLICT => Err(CT_ECONFLICT),
        V_MISSING => Err(CT_EENTRY),
        _ => {
            let sig = fold_word(fold_case(c), c.entry ^ c.mask as u64);
            Ok(Ticket { sig, entry: c.entry, present: E_ALL })
        }
    }
}

/// 十二格联签账本。
pub struct EntryLedger {
    cases: Vec<AuxCase>,
    verdicts: Vec<u8>,
    tickets: Vec<Ticket>,
    frozen: bool,
    root: u64,
    rejects: u32,
    denials: u32,
}

impl EntryLedger {
    /// 空账本。
    pub fn new() -> EntryLedger {
        EntryLedger {
            cases: Vec::new(),
            verdicts: Vec::new(),
            tickets: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
            denials: 0,
        }
    }

    /// 登记一格。结构面拒绝直接返回错误码，语义面判决以 Ok(判决) 返回。
    pub fn register(&mut self, slot: u8, case: AuxCase) -> Result<u8, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CT_EFROZEN);
        }
        if slot as usize >= SAMPLES {
            self.rejects += 1;
            return Err(CT_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(CT_EDUP);
        }
        if slot as usize != self.cases.len() {
            self.rejects += 1;
            return Err(CT_EORDER);
        }
        if case.is_blank() {
            self.rejects += 1;
            return Err(CT_EEMPTY);
        }
        let v = verdict_of(&case);
        let ticket = match sign(&case) {
            Ok(t) => t,
            Err(_) => {
                self.denials += 1;
                Ticket::none()
            }
        };
        self.cases.push(case);
        self.verdicts.push(v);
        self.tickets.push(ticket);
        Ok(v)
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 取格位样本。
    pub fn slot(&self, i: usize) -> Option<AuxCase> {
        self.cases.get(i).copied()
    }

    /// 取格位判决。
    pub fn verdict(&self, i: usize) -> Option<u8> {
        self.verdicts.get(i).copied()
    }

    /// 取格位票面。
    pub fn ticket(&self, i: usize) -> Option<Ticket> {
        self.tickets.get(i).copied()
    }

    /// 某一类判决的格数。
    pub fn count_verdict(&self, v: u8) -> u32 {
        let mut n = 0u32;
        for x in &self.verdicts {
            if *x == v {
                n += 1;
            }
        }
        n
    }

    /// 已联签格数。
    pub fn signed(&self) -> u32 {
        self.count_verdict(V_SIGNED)
    }

    /// 语义拒绝数（缺项、冲突、未知位）。
    pub fn denials(&self) -> u32 {
        self.denials
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 观测一格，并与票面逐项核对。
    pub fn observe(&self, i: usize) -> Result<Context, u32> {
        let c = match self.cases.get(i) {
            Some(v) => *v,
            None => return Err(CT_ENOBS),
        };
        let t = match self.tickets.get(i) {
            Some(v) => *v,
            None => return Err(CT_ENOBS),
        };
        let expect = match sign(&c) {
            Ok(v) => v,
            Err(_) => Ticket::none(),
        };
        if t.sig != expect.sig || t.present != expect.present || t.entry != expect.entry {
            return Err(CT_EMISMATCH);
        }
        Ok(Context {
            present: c.mask,
            missing: missing_of(&c),
            argc: c.argc,
            sig: t.sig,
        })
    }

    /// 逐格重算票号的一致率（千分比）。
    pub fn agree_permille(&self) -> u32 {
        let mut ok = 0u32;
        for i in 0..self.cases.len() {
            let c = self.cases[i];
            let t = self.tickets[i];
            let same = match sign(&c) {
                Ok(v) => v.sig == t.sig && v.present == t.present,
                Err(_) => t.present == 0 && t.sig == 0,
            };
            if same {
                ok += 1;
            }
        }
        if self.cases.is_empty() {
            return 0;
        }
        (ok * 1000) / self.cases.len() as u32
    }

    /// 账本自洽：三列长度一致、票面与判决对齐、计数复算一致。
    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.verdicts.len() || self.cases.len() != self.tickets.len() {
            return Err(CT_EBAL);
        }
        let mut signed = 0u32;
        let mut denial = 0u32;
        for i in 0..self.cases.len() {
            match self.verdicts[i] {
                V_SIGNED => {
                    signed += 1;
                    if self.tickets[i].present != E_ALL || self.tickets[i].sig == 0 {
                        return Err(CT_EBAL);
                    }
                    if self.tickets[i].entry != self.cases[i].entry {
                        return Err(CT_EBAL);
                    }
                }
                _ => {
                    denial += 1;
                    if self.tickets[i].present != 0 || self.tickets[i].sig != 0 {
                        return Err(CT_EBAL);
                    }
                }
            }
        }
        if signed != self.signed() || denial != self.denials {
            return Err(CT_EBAL);
        }
        Ok(())
    }

    /// 账本折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.cases.len() as u64);
        for i in 0..self.cases.len() {
            h = fold_word(h, fold_case(&self.cases[i]));
            h = fold_word(h, self.verdicts[i] as u64);
            h = fold_word(h, self.tickets[i].sig);
        }
        h = fold_word(h, self.denials as u64);
        h
    }

    /// 冻结账本。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(CT_EFROZEN);
        }
        self.audit()?;
        self.root = self.fold();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：逐格重算票号，任何一个不一致即判票号不符。
    pub fn replay(&self) -> Result<u32, u32> {
        let mut signed = 0u32;
        for i in 0..self.cases.len() {
            let t = self.tickets[i];
            match sign(&self.cases[i]) {
                Ok(v) => {
                    signed += 1;
                    if v.sig != t.sig || v.present != t.present || v.entry != t.entry {
                        return Err(CT_ESEAL);
                    }
                }
                Err(_) => {
                    if t.present != 0 || t.sig != 0 || t.entry != 0 {
                        return Err(CT_ESEAL);
                    }
                }
            }
        }
        if signed != self.signed() {
            return Err(CT_ESEAL);
        }
        Ok(signed)
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 票面列长度。
    pub fn tickets_len(&self) -> usize {
        self.tickets.len()
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 联签票面全览：已联签格按序输出入口地址与票号。
pub fn entry_table(l: &EntryLedger) -> Vec<(u8, u64, u64)> {
    let mut out = Vec::new();
    for i in 0..l.len() {
        if let Some(t) = l.ticket(i) {
            if t.present == E_ALL {
                out.push((i as u8, t.entry, t.sig));
            }
        }
    }
    out
}

/// 缺项摘要：返回 (格号, 缺项位图) 序列。
pub fn gaps_of(l: &EntryLedger) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    for i in 0..l.len() {
        if let Some(c) = l.slot(i) {
            let m = missing_of(&c);
            if m != 0 {
                out.push((i as u8, m));
            }
        }
    }
    out
}

/// 十二样本文本摘要。
pub fn summary(l: &EntryLedger) -> String {
    format!(
        "signed={} denials={} rejects={} agree={}",
        l.signed(),
        l.denials(),
        l.rejects(),
        l.agree_permille()
    )
}

/// 构建标准账本：3 联签 + 3 缺项 + 5 冲突 + 1 未知位。
pub fn build_ledger() -> EntryLedger {
    let mut l = EntryLedger::new();
    let _ = l.register(0, AuxCase::make(E_ALL, 1, 12, 0x40, 0x1090, 0x400000, 0xdead_beef));
    let _ = l.register(1, AuxCase::make(E_ALL, 3, 20, 0x40, 0x2340, 0x400000, 0x1111_2222));
    let _ = l.register(2, AuxCase::make(E_ALL, 1, 1, 0x34, 0x1000, 0, 0xfeed));
    let _ = l.register(3, AuxCase::make(E_ALL & !E_ENTRY, 1, 8, 0x40, 0, 0x400000, 0x51));
    let _ = l.register(4, AuxCase::make(E_ALL & !E_RANDOM, 2, 9, 0x40, 0x1100, 0x400000, 0));
    let _ = l.register(5, AuxCase::make(E_PHDR | E_ENTRY | E_RANDOM, 1, 8, 0x40, 0x1000, 0x400000, 0x77));
    let _ = l.register(6, AuxCase::make(E_ALL, 0, 9, 0x40, 0x1000, 0x400000, 0x10));
    let _ = l.register(7, AuxCase::make(E_ALL, 1, 0, 0x40, 0x1000, 0x400000, 0x11));
    let _ = l.register(8, AuxCase::make(E_ALL, 1, 9, 0, 0x1000, 0x400000, 0x12));
    let _ = l.register(9, AuxCase::make(E_ALL, 1, 9, 0x40, 0, 0x400000, 0x13));
    let _ = l.register(10, AuxCase::make(E_ALL, 1, 9, 0x40, 0x1000, 0x400000, 0));
    let _ = l.register(11, AuxCase::make(E_ALL | 0x20, 1, 9, 0x40, 0x1000, 0x400000, 0x14));
    l
}

/// 四路注入：基线、冲突、缺项、冻结后写入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let audit_ok = l.audit().is_ok();
    let replay_ok = l.replay() == Ok(3);
    let agree_ok = l.agree_permille() == 1000;
    let freeze_ok = l.freeze().is_ok();
    out[0] = audit_ok && replay_ok && agree_ok && freeze_ok;

    let conflict = AuxCase::make(E_ALL, 0, 9, 0x40, 0x1000, 0x400000, 0x14);
    out[1] = sign(&conflict) == Err(CT_ECONFLICT) && conflict_of(&conflict) == E_ARGC;

    let missing = AuxCase::make(E_ALL & !E_PHDR, 1, 9, 0, 0x1000, 0x400000, 0x15);
    out[2] = sign(&missing) == Err(CT_EENTRY) && missing_of(&missing) == E_PHDR;

    let mut f = build_ledger();
    let froz = f.freeze().is_ok();
    let deny = f.register(12, AuxCase::make(E_ALL, 1, 9, 0x40, 0x1000, 0x400000, 0x16));
    out[3] = froz && deny == Err(CT_EFROZEN) && f.rejects() == 1 && f.len() == SAMPLES;

    out
}

/// 十三项判据。
pub fn run_unx11258_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11258");
    let l = build_ledger();

    s.add_dyn(
        "CT1 账本十二格齐备",
        l.len() == SAMPLES && !l.is_empty(),
        &format!("slots={}", l.len()),
    );

    let signed = l.count_verdict(V_SIGNED);
    let missing = l.count_verdict(V_MISSING);
    let conflict = l.count_verdict(V_CONFLICT);
    let bits = l.count_verdict(V_BITS);
    s.add_dyn(
        "CT2 三类判决分布正确",
        signed == 3 && missing == 3 && conflict == 5 && bits == 1,
        &format!("s={} m={} c={} b={}", signed, missing, conflict, bits),
    );

    let mut r = EntryLedger::new();
    let ok0 = r.register(0, AuxCase::make(E_ALL, 1, 1, 1, 1, 0, 1)) == Ok(V_SIGNED);
    let dup = r.register(0, AuxCase::make(E_ALL, 1, 1, 1, 2, 0, 2)) == Err(CT_EDUP);
    let order = r.register(5, AuxCase::make(E_ALL, 1, 1, 1, 3, 0, 3)) == Err(CT_EORDER);
    let id = r.register(12, AuxCase::make(E_ALL, 1, 1, 1, 4, 0, 4)) == Err(CT_EID);
    let empty = r.register(1, AuxCase::blank()) == Err(CT_EEMPTY);
    let froze = r.freeze() == Ok(r.root()) || r.is_frozen();
    let quiet = r.register(1, AuxCase::make(E_ALL, 1, 1, 1, 5, 0, 5)) == Err(CT_EFROZEN);
    s.add_dyn(
        "CT3 结构拒绝矩阵五门齐",
        ok0 && dup && order && id && empty && froze && quiet && r.rejects() == 5 && r.len() == 1,
        &format!("rejects={} len={}", r.rejects(), r.len()),
    );

    let m3 = l.slot(3).map(|c| missing_of(&c)) == Some(E_ENTRY);
    let m4 = l.slot(4).map(|c| missing_of(&c)) == Some(E_RANDOM);
    let m5 = l.slot(5).map(|c| missing_of(&c)) == Some(E_ARGC | E_ARGV0);
    s.add_dyn(
        "CT4 缺项位图精确",
        m3 && m4 && m5,
        &format!("m3={} m4={} m5={}", m3, m4, m5),
    );

    let mut c_hits = 0u32;
    let mut c_bits_ok = true;
    for i in 6..11 {
        let c = match l.slot(i) {
            Some(v) => v,
            None => {
                c_bits_ok = false;
                continue;
            }
        };
        if conflict_of(&c).count_ones() == 1 && sign(&c) == Err(CT_ECONFLICT) {
            c_hits += 1;
        }
    }
    s.add_dyn(
        "CT5 冲突逐格定位",
        c_hits == 5 && c_bits_ok,
        &format!("hits={}", c_hits),
    );

    let ub = l.slot(11).map(|c| c.mask & !E_ALL) == Some(0x20);
    s.add_dyn(
        "CT6 未知位拒绝",
        ub && l.slot(11).map(|c| sign(&c)) == Some(Err(CT_EBITS)) && l.verdict(11) == Some(V_BITS),
        &format!("verdict={:?}", l.verdict(11)),
    );

    let mut m_hits = 0u32;
    for i in 3..6 {
        if l.slot(i).map(|c| sign(&c)) == Some(Err(CT_EENTRY)) {
            m_hits += 1;
        }
    }
    s.add_dyn(
        "CT7 缺项联签被拒",
        m_hits == 3,
        &format!("hits={}", m_hits),
    );

    let t0 = l.ticket(0);
    let t1 = l.ticket(1);
    let t2 = l.ticket(2);
    let distinct = match (t0, t1, t2) {
        (Some(a), Some(b), Some(c)) => {
            a.sig != 0 && b.sig != 0 && c.sig != 0 && a.sig != b.sig && b.sig != c.sig && a.sig != c.sig
        }
        _ => false,
    };
    let entry_ok = t0.map(|t| t.entry) == Some(0x1090) && t1.map(|t| t.entry) == Some(0x2340);
    s.add_dyn(
        "CT8 联签票号两两不同",
        distinct && entry_ok,
        &format!("t0={:?} t1={:?}", t0, t1),
    );

    s.add_dyn(
        "CT9 逐格重放千分一致",
        l.replay() == Ok(3) && l.agree_permille() == 1000,
        &format!("agree={} replay={:?}", l.agree_permille(), l.replay()),
    );

    let mut f = build_ledger();
    let first = f.freeze();
    let root_a = f.root();
    let again = f.freeze() == Err(CT_EFROZEN);
    let locked = f.register(0, AuxCase::make(E_ALL, 1, 1, 1, 9, 0, 9)) == Err(CT_EFROZEN);
    s.add_dyn(
        "CT10 冻结幂等且写入被锁",
        first.is_ok() && root_a != 0 && again && locked && f.root() == root_a,
        &format!("root={:016x}", root_a),
    );

    let obs0 = l.observe(0);
    let obs_ok = match obs0 {
        Ok(c) => c.present == E_ALL && c.missing == 0 && c.argc == 1 && c.sig != 0,
        Err(_) => false,
    };
    let obs_conf = match l.observe(6) {
        Ok(c) => c.missing == 0 && c.sig == 0,
        Err(_) => false,
    };
    let obs_miss = match l.observe(3) {
        Ok(c) => c.missing == E_ENTRY && c.sig == 0,
        Err(_) => false,
    };
    s.add_dyn(
        "CT11 观测票面与上下文一致",
        obs_ok && obs_conf && obs_miss && l.observe(12) == Err(CT_ENOBS),
        &format!("obs0={:?}", obs0),
    );

    s.add_dyn(
        "CT12 账本平衡复算一致",
        l.audit().is_ok() && l.signed() == 3 && l.denials() == 9 && l.tickets_len() == SAMPLES,
        &format!("signed={} denials={} tickets={}", l.signed(), l.denials(), l.tickets_len()),
    );

    let inj = inject_four();
    s.add_dyn(
        "CT13 四路注入全绿",
        inj == [true; 4],
        &format!("inject={:?}", inj),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_distribution() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.count_verdict(V_SIGNED), 3);
        assert_eq!(l.count_verdict(V_MISSING), 3);
        assert_eq!(l.count_verdict(V_CONFLICT), 5);
        assert_eq!(l.count_verdict(V_BITS), 1);
        assert_eq!(l.denials(), 9);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn missing_bitmaps() {
        let l = build_ledger();
        assert_eq!(l.slot(3).map(|c| missing_of(&c)), Some(E_ENTRY));
        assert_eq!(l.slot(4).map(|c| missing_of(&c)), Some(E_RANDOM));
        assert_eq!(l.slot(5).map(|c| missing_of(&c)), Some(E_ARGC | E_ARGV0));
        assert_eq!(l.slot(0).map(|c| missing_of(&c)), Some(0));
    }

    #[test]
    fn conflict_locates_zero_only() {
        let l = build_ledger();
        assert_eq!(l.slot(6).map(|c| conflict_of(&c)), Some(E_ARGC));
        assert_eq!(l.slot(7).map(|c| conflict_of(&c)), Some(E_ARGV0));
        assert_eq!(l.slot(8).map(|c| conflict_of(&c)), Some(E_PHDR));
        assert_eq!(l.slot(9).map(|c| conflict_of(&c)), Some(E_ENTRY));
        assert_eq!(l.slot(10).map(|c| conflict_of(&c)), Some(E_RANDOM));
        assert_eq!(l.slot(0).map(|c| conflict_of(&c)), Some(0));
        assert_eq!(sign(&AuxCase::make(E_ALL, 0, 1, 1, 1, 0, 1)), Err(CT_ECONFLICT));
    }

    #[test]
    fn refusal_matrix_exact_codes() {
        let mut l = EntryLedger::new();
        assert_eq!(l.register(0, AuxCase::make(E_ALL, 1, 1, 1, 1, 0, 1)), Ok(V_SIGNED));
        assert_eq!(l.register(0, AuxCase::make(E_ALL, 1, 1, 1, 2, 0, 2)), Err(CT_EDUP));
        assert_eq!(l.register(7, AuxCase::make(E_ALL, 1, 1, 1, 3, 0, 3)), Err(CT_EORDER));
        assert_eq!(l.register(12, AuxCase::make(E_ALL, 1, 1, 1, 4, 0, 4)), Err(CT_EID));
        assert_eq!(l.register(1, AuxCase::blank()), Err(CT_EEMPTY));
        let _ = l.freeze();
        assert_eq!(l.register(1, AuxCase::make(E_ALL, 1, 1, 1, 5, 0, 5)), Err(CT_EFROZEN));
        assert_eq!(l.rejects(), 5);
        assert_eq!(l.len(), 1);
        assert_eq!(l.slot(0).map(|c| fold_case(&c)), Some(fold_case(&AuxCase::make(E_ALL, 1, 1, 1, 1, 0, 1))));
    }

    #[test]
    fn freeze_replay_idempotent() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(3));
        let f1 = l.freeze();
        let root = match f1 {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert_eq!(l.replay(), Ok(3));
        assert_eq!(l.freeze(), Err(CT_EFROZEN));
        assert_eq!(l.root(), root);
        assert_eq!(l.agree_permille(), 1000);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_green_and_injection() {
        let s = run_unx11258_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert_eq!(inject_four(), [true; 4]);
        assert!(summary(&build_ledger()).starts_with("signed=3"));
        assert_eq!(entry_table(&build_ledger()).len(), 3);
        assert_eq!(gaps_of(&build_ledger()).len(), 3);
    }
}
