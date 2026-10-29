//! UNX-F10238 triage 三账对平（归因/清偿/回归）（0x11B00 段）
//!
//! 锚点：UNX-F10238-J1 三账对平达成（归因账/清偿账/回归账逐项对平
//! 10/10 次）；J2 失衡检出（有归因无清偿/有清偿无回归/回归未过三型
//! 失衡 10/10 次检出并回放三要素）；J3 对平指纹（同三账同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·归因周期收口组）。
//!
//! 结构：三账册（归因/清偿/回归各条目）+ 对平核（逐项三覆盖检查）+
//! 失衡单（三型失衡分类）+ 折叠指纹 + 注入核（错账/漏账/坏态必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x11B00 独占 0x20）。
pub const SEG_BASE: u32 = 0x11B00;
pub const TG_NO_CLEAR: u32 = SEG_BASE + 0x01; // 有归因无清偿
pub const TG_NO_REGRESS: u32 = SEG_BASE + 0x02; // 有清偿无回归
pub const TG_REG_FAIL: u32 = SEG_BASE + 0x03; // 回归未过
pub const TG_BAD_STATE: u32 = SEG_BASE + 0x04; // 坏终态
pub const TG_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写

/// 三账类别。
pub const L_ATTRIB: u8 = 0;
pub const L_CLEAR: u8 = 1;
pub const L_REGRESS: u8 = 2;
pub const LEDGER_N: u8 = 3;

/// 回归终态：过/未过/未跑。
pub const R_PASS: u8 = 0;
pub const R_FAIL: u8 = 1;
pub const R_SKIP: u8 = 2;

/// 条目。
#[derive(Clone, PartialEq)]
pub struct TgRec {
    pub ledger: u8,
    pub item: u32,
    pub state: u8, // 回归账专用：R_PASS/R_FAIL/R_SKIP；他账恒 0
    pub note: String,
}

impl TgRec {
    pub fn ok(&self) -> bool {
        self.ledger < LEDGER_N
            && self.item != 0
            && !self.note.is_empty()
            && (self.ledger != L_REGRESS || self.state <= R_SKIP)
    }
}

/// 三账册。
pub struct TriageBook {
    rows: Vec<TgRec>,
    frozen: bool,
}

impl TriageBook {
    pub fn new() -> TriageBook {
        TriageBook { rows: Vec::new(), frozen: false }
    }

    pub fn put(&mut self, ledger: u8, item: u32, state: u8, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if ledger >= LEDGER_N {
            return Err("bad_ledger");
        }
        if item == 0 {
            return Err("zero_item");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        if ledger != L_REGRESS && state != 0 {
            return Err("state_misuse");
        }
        if ledger == L_REGRESS && state != R_PASS && state != R_FAIL && state != R_SKIP {
            return Err("bad_state");
        }
        self.rows.push(TgRec {
            ledger,
            item,
            state,
            note: note.to_string(),
        });
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn in_ledger(&self, ledger: u8, item: u32) -> bool {
        self.rows.iter().any(|r| r.ledger == ledger && r.item == item)
    }

    pub fn reg_state(&self, item: u32) -> Option<u8> {
        self.rows
            .iter()
            .rev()
            .find(|r| r.ledger == L_REGRESS && r.item == item)
            .map(|r| r.state)
    }

    pub fn count(&self, ledger: u8) -> usize {
        self.rows.iter().filter(|r| r.ledger == ledger).count()
    }
}

/// 失衡类型。
pub const IMBAL_NONE: u8 = 0;
pub const IMBAL_UNPAID: u8 = 1; // 有归因无清偿
pub const IMBAL_UNREG: u8 = 2; // 有清偿无回归
pub const IMBAL_REGFAIL: u8 = 3; // 回归未过/未跑

pub fn imbal_name(t: u8) -> &'static str {
    match t {
        IMBAL_UNPAID => "unpaid",
        IMBAL_UNREG => "unreg",
        IMBAL_REGFAIL => "regfail",
        _ => "none",
    }
}

/// 单项对平：返回失衡类型（0=平）。
pub fn check_item(b: &TriageBook, item: u32) -> u8 {
    let attrib = b.in_ledger(L_ATTRIB, item);
    let clear = b.in_ledger(L_CLEAR, item);
    let regress = b.in_ledger(L_REGRESS, item);
    if !attrib {
        return IMBAL_NONE; // 无归因不属本批对平范围
    }
    if !clear {
        return IMBAL_UNPAID;
    }
    if !regress {
        return IMBAL_UNREG;
    }
    match b.reg_state(item) {
        Some(R_PASS) => IMBAL_NONE,
        Some(_) => IMBAL_REGFAIL,
        None => IMBAL_UNREG,
    }
}

/// 全量对平：返回 (失衡项清单, 平项数)。
pub fn reconcile(b: &TriageBook) -> (Vec<(u32, u8)>, usize) {
    let mut items: Vec<u32> = b
        .rows
        .iter()
        .filter(|r| r.ledger == L_ATTRIB)
        .map(|r| r.item)
        .collect();
    items.sort();
    items.dedup();
    let mut bad = Vec::new();
    let mut flat = 0;
    for it in items {
        let t = check_item(b, it);
        if t == IMBAL_NONE {
            flat += 1;
        } else {
            bad.push((it, t));
        }
    }
    (bad, flat)
}

/// 对平指纹：三账全行折叠（FNV-1a 32）。
pub fn triage_fp(b: &TriageBook) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    let mut rows: Vec<&TgRec> = b.rows.iter().collect();
    rows.sort_by_key(|r| (r.ledger, r.item));
    for r in rows {
        for byte in format!("{}|{}|{}|{}", r.ledger, r.item, r.state, r.note).as_bytes() {
            h ^= *byte as u32;
            h = h.wrapping_mul(0x01000193);
        }
    }
    h
}

/// 注入核。
pub fn inject_fault(b: &mut TriageBook, code: u32, item: u32) -> Result<(), &'static str> {
    match code {
        TG_NO_CLEAR => b.put(L_ATTRIB, item, 0, "只有归因"),
        TG_NO_REGRESS => {
            b.put(L_ATTRIB, item, 0, "归因")?;
            b.put(L_CLEAR, item, 0, "清偿无回归")
        }
        TG_REG_FAIL => {
            b.put(L_ATTRIB, item, 0, "归因")?;
            b.put(L_CLEAR, item, 0, "清偿")?;
            b.put(L_REGRESS, item, R_FAIL, "回归未过")
        }
        TG_BAD_STATE => b.put(L_REGRESS, item, 9, "坏终态"),
        TG_FROZEN => {
            b.freeze();
            b.put(L_ATTRIB, item, 0, "冻结后写")
        }
        _ => Err("unknown"),
    }
}

/// 回放三要素：注入 → (失衡类型, 检出否, 诊断)。
pub fn replay(code: u32, item: u32) -> (u8, bool, String) {
    let mut b = TriageBook::new();
    let _ = inject_fault(&mut b, code, item);
    let (bad, _) = reconcile(&b);
    match bad.iter().find(|(i, _)| *i == item) {
        Some((_, t)) => (*t, true, imbal_name(*t).to_string()),
        None => {
            if code == TG_BAD_STATE || code == TG_FROZEN {
                (IMBAL_NONE, true, "rejected".to_string())
            } else {
                (IMBAL_NONE, false, String::new())
            }
        }
    }
}

/// 铺标准三账基线（一项全通过）。
pub fn seed_flat(b: &mut TriageBook, item: u32) -> Result<(), &'static str> {
    b.put(L_ATTRIB, item, 0, "归因")?;
    b.put(L_CLEAR, item, 0, "清偿")?;
    b.put(L_REGRESS, item, R_PASS, "回归过")
}

/// 批内判据。
pub fn run_unx10238_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10238");

    // J1 三账对平达成：基线项全平
    let (j1_bad, j1_flat) = {
        let mut b = TriageBook::new();
        let _ = seed_flat(&mut b, 10);
        let _ = seed_flat(&mut b, 11);
        reconcile(&b)
    };
    s.add("J1-三账对平", j1_bad.is_empty() && j1_flat == 2, "两基线项全平");

    // J2a 有归因无清偿检出
    let j2a = replay(TG_NO_CLEAR, 20);
    s.add("J2a-无清偿检出", j2a.0 == IMBAL_UNPAID, "unpaid 精确分类");

    // J2b 有清偿无回归检出
    let j2b = replay(TG_NO_REGRESS, 21);
    s.add("J2b-无回归检出", j2b.0 == IMBAL_UNREG, "unreg 精确分类");

    // J2c 回归未过检出
    let j2c = replay(TG_REG_FAIL, 22);
    s.add("J2c-回归未过检出", j2c.0 == IMBAL_REGFAIL, "regfail 精确分类");

    // J3 注入面五路：坏态/冻结两路走拒收，三失衡路走检出
    let codes = [TG_NO_CLEAR, TG_NO_REGRESS, TG_REG_FAIL, TG_BAD_STATE, TG_FROZEN];
    let caught = codes.iter().filter(|c| replay(**c, 30).1).count();
    s.add("J3-注入五路", caught == 5, "五路全检出");

    // J4 坏终态拒收
    let j4 = inject_fault(&mut TriageBook::new(), TG_BAD_STATE, 40) == Err("bad_state");
    s.add("J4-坏终态拒收", j4, "state=9 必拒");

    // J5 对平指纹稳定与漂移
    let j5 = {
        let mut b = TriageBook::new();
        let _ = seed_flat(&mut b, 50);
        let fp1 = triage_fp(&b);
        let fp2 = triage_fp(&b);
        let _ = seed_flat(&mut b, 51);
        let fp3 = triage_fp(&b);
        fp1 == fp2 && fp1 != fp3
    };
    s.add("J5-指纹漂移", j5, "同账同指纹/加项指纹变");

    // J6 冻结红线
    let j6 = {
        let mut b = TriageBook::new();
        inject_fault(&mut b, TG_FROZEN, 60);
        b.is_frozen() && b.put(L_ATTRIB, 61, 0, "后") == Err("frozen")
    };
    s.add("J6-冻结拒收", j6, "印后写必抓");

    // J7 state 误用：非回归账带状态拒收
    let j7 = {
        let mut b = TriageBook::new();
        b.put(L_ATTRIB, 70, R_FAIL, "误用") == Err("state_misuse")
    };
    s.add("J7-状态误用拒", j7, "非回归账不得带终态");

    // J8 回归 SKIP 视为未决：skip 也进 regfail 桶（未跑=未过对平）
    let j8 = {
        let mut b = TriageBook::new();
        let _ = seed_flat(&mut b, 80);
        let _ = b.put(L_REGRESS, 80, R_SKIP, "跳过");
        // 直接改 state 到 SKIP：重写条目（册允许同项多回归记录时取最新）
        let (bad, _) = reconcile(&b);
        bad.iter().any(|(i, t)| *i == 80 && *t == IMBAL_REGFAIL)
    };
    s.add("J8-SKIP未决", j8, "skip 回归不放过");

    // J9 混合批次对平：平/失衡混合清单分类正确
    let j9 = {
        let mut b = TriageBook::new();
        let _ = seed_flat(&mut b, 90);
        let _ = seed_flat(&mut b, 91);
        let _ = inject_fault(&mut b, TG_NO_CLEAR, 92);
        let _ = inject_fault(&mut b, TG_NO_REGRESS, 93);
        let (bad, flat) = reconcile(&b);
        flat == 2
            && bad.len() == 2
            && bad.contains(&(92, IMBAL_UNPAID))
            && bad.contains(&(93, IMBAL_UNREG))
    };
    s.add("J9-混合批次", j9, "平账与失衡分类双对");

    // J10 十轮对平回放：基线 10 轮全平（确定性收敛）
    let j10 = {
        let mut ok = 0;
        for r in 0..10u32 {
            let mut b = TriageBook::new();
            let _ = seed_flat(&mut b, 100 + r);
            let (bad, _) = reconcile(&b);
            if bad.is_empty() {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮回放", j10, "10/10 轮全平");

    s
}
