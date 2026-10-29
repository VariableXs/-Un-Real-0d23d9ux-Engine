//! UNX-F11277 三级自举链一级→二级衔接账（0x1f4d0 段）
//!
//! 自举链分三级：0 级装载器、1 级 hello（静态探路件）、2 级 busybox。
//! 本模块只负责 **1→2 这一道衔接**：hello 跑通之后，把控制权交给 busybox
//! 之前必须逐项核对八条前置条件——PT_INTERP 可解、入口符号可解析、
//! argv 契约、auxv 的 AT_PHDR/AT_ENTRY、栈 16 对齐、environ 非空、
//! brk 与装载基址不重叠、重定位表可遍历。八条各有位、权、责任方。
//! 判据函数按**固定优先序**取第一条缺口：解释器 > 符号 > argv > auxv >
//! 栈 > 环境 > brk > 重定位；全满足才返回 0（放行）。
//! 门禁规格与 F11218 对齐：门禁号 0x118、门禁版本 3，门禁码 = 2 + 条目下标，
//! 故门禁码与清单顺序是同一张表推导出来的，不是两处手抄。
//! 账本登记十类样本（一条放行 + 八条逐项缺口 + 一条阶段序错），
//! 直方按判据码落槽，replay 要求「零判据 + 直方总和 == 登记条数」。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、封册后写账四道；
//! 阶段号与缺失位**不拒收**——它们是喂给判据函数产出门禁码的。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;

/// 诊断段（0x1f4d0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f4d0;

/// 条目 id 越界。
pub const HS_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const HS_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const HS_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const HS_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const HS_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const HS_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算门禁码与登记时不符。
pub const HS_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const HS_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const HS_EDOBS: u32 = SEG_BASE + 0x09;
/// 直方/尝试表不平。
pub const HS_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 门禁码：阶段对非法（不是 1→2）。
pub const PRC_STAGE: u32 = 1;
/// 门禁码：PT_INTERP 缺失或不可解。
pub const PRC_INTERP: u32 = 2;
/// 门禁码：入口符号不可解析。
pub const PRC_SYMBOL: u32 = 3;
/// 门禁码：argv 契约破裂。
pub const PRC_ARGV: u32 = 4;
/// 门禁码：auxv 缺 AT_PHDR / AT_ENTRY。
pub const PRC_AUXV: u32 = 5;
/// 门禁码：栈未 16 字节对齐。
pub const PRC_STACK: u32 = 6;
/// 门禁码：environ 为空。
pub const PRC_ENV: u32 = 7;
/// 门禁码：brk 与装载基址重叠。
pub const PRC_HEAP: u32 = 8;
/// 门禁码：重定位表不可遍历。
pub const PRC_RELOC: u32 = 9;

/// 一级启动件名。
pub const STAGE1: &str = "hello";
/// 二级启动件名。
pub const STAGE2: &str = "busybox";
/// 衔接的起点级号。
pub const FROM_STAGE: u32 = 1;
/// 衔接的终点级号。
pub const TO_STAGE: u32 = 2;
/// 门禁号（与 F11218 规格一致）。
pub const GATE_ID: u32 = 0x118;
/// 门禁版本。
pub const GATE_VERSION: u32 = 3;
/// 前置条件总权重（清单必须凑满 100）。
pub const WEIGHT_TOTAL: u32 = 100;
/// 条目表定容。
pub const CAP: usize = 12;
/// 条目 id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

/// PT_INTERP 可解。
pub const B_INTERP: u32 = 1 << 0;
/// 入口符号可解析。
pub const B_SYMBOL: u32 = 1 << 1;
/// argv 契约。
pub const B_ARGV: u32 = 1 << 2;
/// auxv 齐备。
pub const B_AUXV: u32 = 1 << 3;
/// 栈对齐。
pub const B_STACK: u32 = 1 << 4;
/// environ 非空。
pub const B_ENV: u32 = 1 << 5;
/// brk 不重叠。
pub const B_HEAP: u32 = 1 << 6;
/// 重定位表可遍历。
pub const B_RELOC: u32 = 1 << 7;
/// 八条全满足的位掩码。
pub const REQUIRED: u32 = B_INTERP | B_SYMBOL | B_ARGV | B_AUXV | B_STACK | B_ENV | B_HEAP | B_RELOC;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_77;

/// 一条前置条件：名称 + 位 + 权重 + 责任方。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Precondition {
    pub name: &'static str,
    pub bit: u32,
    pub weight: u32,
    pub owner: &'static str,
}

/// 八条前置条件，顺序即判据优先序（门禁码 = 2 + 下标）。
pub const PRECONDITIONS: [Precondition; 8] = [
    Precondition { name: "PT_INTERP", bit: B_INTERP, weight: 20, owner: "loader" },
    Precondition { name: "ENTRY_SYMBOL", bit: B_SYMBOL, weight: 15, owner: "linker" },
    Precondition { name: "ARGV_CONTRACT", bit: B_ARGV, weight: 15, owner: "hello" },
    Precondition { name: "AUXV", bit: B_AUXV, weight: 15, owner: "kernel" },
    Precondition { name: "STACK_ALIGN", bit: B_STACK, weight: 10, owner: "loader" },
    Precondition { name: "ENVIRON", bit: B_ENV, weight: 10, owner: "init" },
    Precondition { name: "BRK_BASE", bit: B_HEAP, weight: 10, owner: "mm" },
    Precondition { name: "RELOC_TABLE", bit: B_RELOC, weight: 5, owner: "linker" },
];

/// 一次衔接尝试：id + 起止级 + 已满足位掩码 + 指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handoff {
    pub id: u32,
    pub from: u32,
    pub to: u32,
    pub sat: u32,
    pub hash: u64,
}

impl Handoff {
    pub const fn blank() -> Handoff {
        Handoff { id: 0, from: 0, to: 0, sat: 0, hash: 0 }
    }

    pub fn render(&self) -> String {
        format!("H{} {}->{} sat={:02x} h={:x}", self.id, self.from, self.to, self.sat, self.hash)
    }
}

/// 纯判据：阶段对先判，再按清单优先序取第一条缺口。
pub fn handoff_verdict(h: &Handoff) -> u32 {
    if h.from != FROM_STAGE || h.to != TO_STAGE {
        return PRC_STAGE;
    }
    let mut k = 0usize;
    while k < PRECONDITIONS.len() {
        if let Some(p) = PRECONDITIONS.get(k) {
            if h.sat & p.bit == 0 {
                return 2u32.wrapping_add(k as u32);
            }
        }
        k = k.wrapping_add(1);
    }
    0
}

/// 取第 i 条前置条件；越界返回 None（调用方自行处理）。
pub fn precondition_at(i: usize) -> Option<Precondition> {
    PRECONDITIONS.get(i).copied()
}

/// 清单权重合计。
pub fn weight_sum() -> u32 {
    let mut s = 0u32;
    let mut k = 0usize;
    while k < PRECONDITIONS.len() {
        if let Some(p) = PRECONDITIONS.get(k) {
            s = s.wrapping_add(p.weight);
        }
        k = k.wrapping_add(1);
    }
    s
}

/// 清单位是否两两互异（重复位会让门禁码与清单脱钩）。
pub fn bits_distinct() -> bool {
    let mut i = 0usize;
    while i < PRECONDITIONS.len() {
        let mut j = i.wrapping_add(1);
        while j < PRECONDITIONS.len() {
            if let (Some(a), Some(b)) = (PRECONDITIONS.get(i), PRECONDITIONS.get(j)) {
                if a.bit == b.bit {
                    return false;
                }
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 位并集是否恰好覆盖 REQUIRED（多一位或少一位都算脱钩）。
pub fn bits_cover() -> bool {
    let mut u = 0u32;
    let mut k = 0usize;
    while k < PRECONDITIONS.len() {
        if let Some(p) = PRECONDITIONS.get(k) {
            u |= p.bit;
        }
        k = k.wrapping_add(1);
    }
    u == REQUIRED
}

/// 三级自举衔接账本：样本 + 门禁码 + 十槽直方 + 来源双槽。
#[derive(Clone, Copy, Debug)]
pub struct LinkLedger {
    cases: [Handoff; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 10],
    origin: [u32; 2],
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    zero: u32,
    frozen: bool,
    root: Option<u64>,
}

impl LinkLedger {
    pub const fn new() -> LinkLedger {
        LinkLedger {
            cases: [Handoff::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 10],
            origin: [0u32; 2],
            len: 0,
            attempts: 0,
            accepts: 0,
            rejects: 0,
            observed: 0,
            matched: 0,
            zero: 0,
            frozen: false,
            root: None,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记：id 域 → 重复 → 顺序 → 定容 → 封册（封册最先判）。
    pub fn register(&mut self, h: Handoff) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(HS_EFROZEN));
        }
        if h.id < ID_MIN || h.id > ID_MAX {
            return Err(self.fail(HS_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == h.id {
                    return Err(self.fail(HS_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if h.id <= last.id {
                    return Err(self.fail(HS_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(HS_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = h;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记并当场算门禁码，直方与来源槽同步落账。
    pub fn register_ruled(&mut self, h: Handoff) -> Result<(usize, u32), u32> {
        let idx = self.register(h)?;
        let v = handoff_verdict(&h);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if v == 0 {
            self.zero = self.zero.wrapping_add(1);
        } else if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        let o = if h.from == FROM_STAGE { 0usize } else { 1usize };
        if let Some(c) = self.origin.get_mut(o) {
            *c = c.wrapping_add(1);
        }
        Ok((idx, v))
    }

    /// 观测一条实测门禁码；与登记判据一致返回 true。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(HS_EFROZEN);
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
            None => return Err(HS_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(HS_EDOBS),
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
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn observed(&self) -> u32 {
        self.observed
    }
    pub fn matched(&self) -> u32 {
        self.matched
    }
    pub fn zero_verdicts(&self) -> u32 {
        self.zero
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
        while k < 10 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn origin_at(&self, i: usize) -> u32 {
        match self.origin.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn case_at(&self, i: usize) -> Option<Handoff> {
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

    /// FNV-1a：样本、门禁码、观测位、十槽直方、来源双槽、全部计数器。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.attempts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.observed as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.matched as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.zero as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frozen as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.len {
            if let Some(c) = self.cases.get(k) {
                h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.from as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.to as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.sat as u64).wrapping_mul(FNV_PRIME);
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
        while j < 10 {
            if let Some(v) = self.hist.get(j) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h = (h ^ self.origin_at(0) as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.origin_at(1) as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 复账：全登记、全观测、全复算、直方与来源槽双平衡。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 {
            return Err(HS_EUNOBS);
        }
        if self.observed as usize != self.len || self.matched as usize != self.len {
            return Err(HS_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(HS_EBALANCE);
        }
        if self.zero.wrapping_add(self.hist_sum()) != self.len as u32 {
            return Err(HS_EBALANCE);
        }
        if self.origin_at(0).wrapping_add(self.origin_at(1)) != self.len as u32 {
            return Err(HS_EBALANCE);
        }
        if weight_sum() != WEIGHT_TOTAL || !bits_distinct() || !bits_cover() {
            return Err(HS_EBALANCE);
        }
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(HS_EUNOBS),
            };
            let v = match self.verdicts.get(k) {
                Some(x) => *x,
                None => return Err(HS_EMISMATCH),
            };
            if handoff_verdict(&c) != v {
                return Err(HS_EMISMATCH);
            }
            match self.seen.get(k) {
                Some(true) => {}
                _ => return Err(HS_EUNOBS),
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：先复账再上锁；二次封册即 EFROZEN。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(HS_EFROZEN);
        }
        let h = self.replay()?;
        self.frozen = true;
        self.root = Some(h);
        Ok(h)
    }
}

/// 构造一条衔接样本（id 由调用方补）。
pub fn handoff(from: u32, to: u32, sat: u32, hash: u64) -> Handoff {
    Handoff { id: 1, from, to, sat, hash }
}

/// 十二条规格样本：一条放行、八条逐项缺口、三条阶段序错。
pub fn spec_of(id: usize) -> (Handoff, u32) {
    let all = REQUIRED;
    match id {
        1 => (handoff(1, 2, all, 0x7701), 0),
        2 => (handoff(1, 2, all & !B_INTERP, 0x7702), PRC_INTERP),
        3 => (handoff(1, 2, all & !B_SYMBOL, 0x7703), PRC_SYMBOL),
        4 => (handoff(1, 2, all & !B_ARGV, 0x7704), PRC_ARGV),
        5 => (handoff(1, 2, all & !B_AUXV, 0x7705), PRC_AUXV),
        6 => (handoff(1, 2, all & !B_STACK, 0x7706), PRC_STACK),
        7 => (handoff(1, 2, all & !B_ENV, 0x7707), PRC_ENV),
        8 => (handoff(1, 2, all & !B_HEAP, 0x7708), PRC_HEAP),
        9 => (handoff(1, 2, all & !B_RELOC, 0x7709), PRC_RELOC),
        10 => (handoff(0, 1, all, 0x770a), PRC_STAGE),
        11 => (handoff(2, 1, all, 0x770b), PRC_STAGE),
        12 => (handoff(1, 3, all, 0x770c), PRC_STAGE),
        _ => (Handoff::blank(), HS_EID),
    }
}

/// 逐条登记 + 观测，判据取规格里的期望码。
pub fn build_ledger() -> LinkLedger {
    let mut l = LinkLedger::new();
    let mut i = 1usize;
    while i <= CAP {
        let (mut h, expect) = spec_of(i);
        h.id = i as u32;
        if l.register_ruled(h).is_ok() {
            let _ = l.observe(h.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、越界、封册。
pub fn inject_four() -> [bool; 4] {
    let mut a = LinkLedger::new();
    let mut hi = handoff(1, 2, REQUIRED, 0x81);
    hi.id = 2;
    let _ = a.register(hi);
    let bad_a = a.register(handoff(1, 2, REQUIRED, 0x82)).err() == Some(HS_EORDER);

    let mut b = LinkLedger::new();
    let _ = b.register(handoff(1, 2, REQUIRED, 0x83));
    let bad_b = b.register(handoff(1, 2, REQUIRED, 0x84)).err() == Some(HS_EDUP);

    let mut c = LinkLedger::new();
    let mut wide = handoff(1, 2, REQUIRED, 0x85);
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(HS_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = handoff(1, 2, REQUIRED, 0x86);
    late.id = 13;
    let bad_d = d.register(late).err() == Some(HS_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

impl LinkLedger {
    /// 复账测试钩子：只改样本的已满足位，不改已落账的判据副本，
    /// 用于把「复算不一致」这条路径真正走到（HS_EMISMATCH）。
    pub fn retune(&mut self, i: usize, sat: u32) -> bool {
        match self.cases.get_mut(i) {
            Some(c) => {
                c.sat = sat;
                true
            }
            None => false,
        }
    }
}

/// 十三条自检：前九项压前置清单与门禁面，后四项压拒绝/复账/封册面。
pub fn run_unx11277_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11277");

    let layout = SEG_BASE == 0x1f4d0
        && HS_EID == SEG_BASE + 0x01
        && HS_EORDER == SEG_BASE + 0x02
        && HS_EDUP == SEG_BASE + 0x03
        && HS_EFULL == SEG_BASE + 0x04
        && HS_EFROZEN == SEG_BASE + 0x05
        && HS_EUNSEEN == SEG_BASE + 0x06
        && HS_EMISMATCH == SEG_BASE + 0x07
        && HS_EUNOBS == SEG_BASE + 0x08
        && HS_EDOBS == SEG_BASE + 0x09
        && HS_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1f4d0 段十码连续无洞");

    s.add("S2-八条前置条件清单", PRECONDITIONS.len() == 8
        && bits_distinct()
        && bits_cover()
        && weight_sum() == WEIGHT_TOTAL,
        "八条位互异、并集恰为 REQUIRED、权重合计 100");

    let mut gate_ok = GATE_ID == 0x118 && GATE_VERSION == 3;
    let mut k = 0usize;
    while k < PRECONDITIONS.len() {
        if let Some(p) = PRECONDITIONS.get(k) {
            let want = 2u32.wrapping_add(k as u32);
            if handoff_verdict(&handoff(1, 2, REQUIRED & !p.bit, 0x90)) != want {
                gate_ok = false;
            }
        }
        k = k.wrapping_add(1);
    }
    s.add("S3-门禁规格与 F11218 一致", gate_ok,
        "门禁号 0x118、版本 3，门禁码由清单下标推导");

    s.add("S4-hello 到 busybox 放行", handoff_verdict(&handoff(1, 2, REQUIRED, 0x91)) == 0
        && STAGE1 == "hello" && STAGE2 == "busybox",
        "八条全满足才返回 0，一级/二级件名固定");

    let mut each_ok = true;
    let mut j = 0usize;
    while j < PRECONDITIONS.len() {
        if let Some(p) = PRECONDITIONS.get(j) {
            let got = handoff_verdict(&handoff(1, 2, REQUIRED & !p.bit, 0x92));
            if got != 2u32.wrapping_add(j as u32) {
                each_ok = false;
            }
        }
        j = j.wrapping_add(1);
    }
    s.add("S5-缺一项即闭门", each_ok,
        "逐位清空各得唯一门禁码，八码无一重叠");

    let prio = handoff_verdict(&handoff(1, 2, 0, 0x93)) == PRC_INTERP
        && handoff_verdict(&handoff(1, 2, REQUIRED & !(B_ENV | B_HEAP), 0x94)) == PRC_ENV
        && handoff_verdict(&handoff(1, 2, REQUIRED & !(B_HEAP | B_RELOC), 0x95)) == PRC_HEAP
        && handoff_verdict(&handoff(1, 2, REQUIRED & !(B_AUXV | B_STACK), 0x96)) == PRC_AUXV;
    s.add("S6-多缺口按优先序取首条", prio,
        "解释器 > argv/auxv/栈 > 环境 > brk > 重定位，只报最前一条");

    let stage = handoff_verdict(&handoff(0, 1, REQUIRED, 0x97)) == PRC_STAGE
        && handoff_verdict(&handoff(2, 1, REQUIRED, 0x98)) == PRC_STAGE
        && handoff_verdict(&handoff(1, 3, REQUIRED, 0x99)) == PRC_STAGE
        && handoff_verdict(&handoff(2, 2, REQUIRED, 0x9a)) == PRC_STAGE
        && handoff_verdict(&handoff(1, 2, REQUIRED, 0x9b)) == 0;
    s.add("S7-只认一级到二级这一对", stage,
        "0→1、2→1、1→3、2→2 全部 PRC_STAGE，仅 1→2 可放行");

    let led = build_ledger();
    let bal = led.len() == 12
        && led.zero_verdicts() == 1
        && led.hist_at(1) == 3
        && led.hist_at(PRC_INTERP as usize) == 1
        && led.hist_at(PRC_RELOC as usize) == 1
        && led.hist_sum() == 11
        && led.origin_at(0) == 10
        && led.origin_at(1) == 2;
    s.add("S8-账本直方与来源双槽平衡", bal,
        "12 条 = 放行 1 + 门禁 11，一级来源 10、其余 2");

    s.add("S9-全观测且门禁码复算一致", led.replay().is_ok()
        && led.observed() == 12 && led.matched() == 12,
        "十二条逐条复算并观测对齐，来源槽合计等于登记数");

    let a = build_ledger();
    let b = build_ledger();
    let h0 = a.fold();
    let same = h0 != 0 && h0 == b.fold();
    let mut t = build_ledger();
    let touched = t.retune(0, REQUIRED & !B_INTERP);
    s.add("S10-同构指纹相同且改写即失效", same && touched
        && t.fold() != h0
        && t.replay().err() == Some(HS_EMISMATCH),
        "同构账折叠同值，改写已满足位后复账即 EMISMATCH");

    let mut g = LinkLedger::new();
    let mut c2 = handoff(1, 2, REQUIRED, 0xa1);
    c2.id = 2;
    let _ = g.register(c2);
    let bad_a = g.register(handoff(1, 2, REQUIRED, 0xa2)).err() == Some(HS_EORDER);
    let bad_b = g.register(c2).err() == Some(HS_EDUP);
    let mut wide = handoff(1, 2, REQUIRED, 0xa3);
    wide.id = ID_MAX + 1;
    let bad_c = g.register(wide).err() == Some(HS_EID);
    let mut full = build_ledger();
    let mut extra = handoff(1, 2, REQUIRED, 0xa4);
    extra.id = 13;
    let bad_d = full.register(extra).err() == Some(HS_EFULL);
    s.add("S11-顺序重复越界定容四闸", bad_a && bad_b && bad_c && bad_d
        && g.accepts() == 1 && g.rejects() == 3 && g.attempts() == 4
        && inject_four() == [true, true, true, true],
        "四闸各自独立触发，拒绝数与尝试数配平");

    let mut ob = LinkLedger::new();
    let mut one = handoff(1, 2, REQUIRED, 0xa5);
    one.id = 1;
    let _ = ob.register_ruled(one);
    let miss = ob.replay().err() == Some(HS_EUNOBS);
    let unseen = ob.observe(99, 0).err() == Some(HS_EUNSEEN);
    let wrong = ob.observe(1, PRC_ENV) == Ok(false);
    let dob = ob.observe(1, 0).err() == Some(HS_EDOBS);
    s.add("S12-观测面三门与错观测", miss && unseen && wrong && dob,
        "未观测 EUNOBS、未登记 EUNSEEN、错观测 false、重观测 EDOBS");

    let mut fz = build_ledger();
    let root = match fz.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = fz.freeze();
    let mut late = handoff(1, 2, REQUIRED, 0xa6);
    late.id = 13;
    let post = fz.register(late).err();
    s.add_dyn("S13-封册先复账后上锁", root != 0
        && again == Err(HS_EFROZEN)
        && post == Some(HS_EFROZEN)
        && fz.root() == Some(root)
        && fz.replay().is_ok(),
        &format!("根={:x} 二次={:?} 封后写={:?}", root, again, post));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11277_checklist_shape() {
        assert_eq!(PRECONDITIONS.len(), 8);
        assert!(bits_distinct());
        assert!(bits_cover());
        assert_eq!(weight_sum(), 100);
        assert_eq!(REQUIRED, 0xff);
        assert_eq!(GATE_ID, 0x118);
        assert_eq!(GATE_VERSION, 3);
    }

    #[test]
    fn unx11277_gate_codes() {
        assert_eq!(handoff_verdict(&handoff(1, 2, REQUIRED, 0)), 0);
        assert_eq!(handoff_verdict(&handoff(1, 2, 0, 0)), PRC_INTERP);
        assert_eq!(handoff_verdict(&handoff(1, 2, REQUIRED & !B_RELOC, 0)), PRC_RELOC);
        assert_eq!(handoff_verdict(&handoff(1, 2, REQUIRED & !B_HEAP, 0)), PRC_HEAP);
        assert_eq!(handoff_verdict(&handoff(0, 1, REQUIRED, 0)), PRC_STAGE);
        assert_eq!(handoff_verdict(&handoff(1, 3, 0, 0)), PRC_STAGE);
        assert_eq!(handoff_verdict(&handoff(1, 1, REQUIRED, 0)), PRC_STAGE);
    }

    #[test]
    fn unx11277_ledger_balance() {
        let l = build_ledger();
        assert_eq!(l.len(), 12);
        assert_eq!(l.zero_verdicts(), 1);
        assert_eq!(l.hist_at(1), 3);
        assert_eq!(l.hist_sum(), 11);
        assert_eq!(l.origin_at(0) + l.origin_at(1), 12);
        assert_eq!(l.attempts(), 12);
        assert_eq!(l.rejects(), 0);
        assert!(l.replay().is_ok());
    }

    #[test]
    fn unx11277_refusals() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = LinkLedger::new();
        let mut c = handoff(1, 2, REQUIRED, 1);
        c.id = 5;
        assert!(l.register(c).is_ok());
        assert_eq!(l.register(c).err(), Some(HS_EDUP));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.accepts(), 1);
        assert_eq!(l.attempts(), 2);
    }

    #[test]
    fn unx11277_freeze_and_checks() {
        let mut l = build_ledger();
        let h = l.freeze();
        assert!(h.is_ok());
        assert_eq!(l.freeze(), Err(HS_EFROZEN));
        assert!(l.replay().is_ok());
        let s = run_unx11277_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
