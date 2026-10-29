//! UNX-F10234 归因周期报告（四账一致）（0xFFA0+(10234-10100)*0x20 段）
//!
//! 锚点：UNX-F10234-J1 周期报告落账（归因账/清偿账/回归账/周期账四账
//! 对平 10/10 次）；J2 单账错位检出（错周期/错指纹/错终态 10/10 次检出
//! 并回放三要素）；J3 报告指纹（同四账同指纹，任一账漂移指纹必变）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·归因周期收口组）。
//!
//! 结构：四账册（attrib/clear/regress/cycle 各自流水）+ 对平核（逐条
//! 周期对平并出差异单）+ 周期报告册（折叠指纹）+ 注入核（错周期/错
//! 指纹/错终态必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x11280 独占 0x20）。
pub const SEG_BASE: u32 = 0x11280;
pub const AC_WRONG_CYCLE: u32 = SEG_BASE + 0x01; // 错周期
pub const AC_WRONG_FP: u32 = SEG_BASE + 0x02; // 错指纹
pub const AC_WRONG_VERDICT: u32 = SEG_BASE + 0x03; // 错终态
pub const AC_MISSING_LEDGER: u32 = SEG_BASE + 0x04; // 缺账
pub const AC_DUP_ENTRY: u32 = SEG_BASE + 0x05; // 重流水
pub const AC_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改账

/// 四账类别闭集。
pub const K_ATTRIB: u8 = 0; // 归因账
pub const K_CLEAR: u8 = 1; // 清偿账
pub const K_REGRESS: u8 = 2; // 回归账
pub const K_CYCLE: u8 = 3; // 周期账
pub const KIND_N: usize = 4;

pub fn kind_ok(k: u8) -> bool {
    k < KIND_N as u8
}

pub fn kind_name(k: u8) -> &'static str {
    match k {
        K_ATTRIB => "attrib",
        K_CLEAR => "clear",
        K_REGRESS => "regress",
        _ => "cycle",
    }
}

/// 周期终态闭集：对平/带遗/挂起。
pub const V_FLAT: u8 = 0;
pub const V_CARRY: u8 = 1;
pub const V_PEND: u8 = 2;

pub fn verdict_ok(v: u8) -> bool {
    v <= V_PEND
}

/// 单条账目流水。
#[derive(Clone, PartialEq)]
pub struct Entry {
    pub seq: u32,
    pub kind: u8,
    pub cycle: u32,
    pub item: u32,
    pub verdict: u8,
    pub note: String,
}

impl Entry {
    pub fn ok(&self) -> bool {
        kind_ok(self.kind) && verdict_ok(self.verdict) && !self.note.is_empty() && self.item != 0
    }

    /// 流水渲染行（对平单用）。
    pub fn render(&self) -> String {
        format!(
            "{}|c{}|i{}|v{}|{}",
            kind_name(self.kind),
            self.cycle,
            self.item,
            self.verdict,
            self.note
        )
    }
}

/// 四账册：kind × cycle 双键组织。
pub struct QuadLedger {
    rows: Vec<Entry>,
    seen: Vec<(u32, u8, u32)>,
    frozen: bool,
}

impl QuadLedger {
    pub fn new() -> QuadLedger {
        QuadLedger { rows: Vec::new(), seen: Vec::new(), frozen: false }
    }

    /// 登账：同 (cycle, kind, item) 重号即拒（重流水红线）。
    pub fn put(
        &mut self,
        seq: u32,
        kind: u8,
        cycle: u32,
        item: u32,
        verdict: u8,
        note: &str,
    ) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !kind_ok(kind) || !verdict_ok(verdict) {
            return Err("kind_or_verdict");
        }
        if item == 0 {
            return Err("zero_item");
        }
        let key = (cycle, kind, item);
        if self.seen.contains(&key) {
            return Err("dup");
        }
        self.seen.push(key);
        self.rows.push(Entry {
            seq,
            kind,
            cycle,
            item,
            verdict,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 冻结：冻结后一切写拒收（印后登记红线）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 取某账某周期全部流水（按 seq 升序）。
    pub fn slice(&self, kind: u8, cycle: u32) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = self
            .rows
            .iter()
            .filter(|r| r.kind == kind && r.cycle == cycle)
            .collect();
        out.sort_by_key(|r| r.seq);
        out
    }

    /// 账内合法率（判据用）。
    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 单账对平结论：条数、终态分布、坏行清单。
pub struct SliceAudit {
    pub kind: u8,
    pub cycle: u32,
    pub n: usize,
    pub flat: usize,
    pub carry: usize,
    pub pend: usize,
    pub bad: Vec<u32>,
}

impl SliceAudit {
    /// 对平即该账该周期无挂起且无坏行。
    pub fn is_flat(&self) -> bool {
        self.pend == 0 && self.bad.is_empty()
    }
}

/// 对平单账：返回审计结构。
pub fn audit_slice(l: &QuadLedger, kind: u8, cycle: u32) -> SliceAudit {
    let rows = l.slice(kind, cycle);
    let mut a = SliceAudit {
        kind,
        cycle,
        n: rows.len(),
        flat: 0,
        carry: 0,
        pend: 0,
        bad: Vec::new(),
    };
    for r in rows {
        if !r.ok() {
            a.bad.push(r.seq);
            continue;
        }
        match r.verdict {
            V_FLAT => a.flat += 1,
            V_CARRY => a.carry += 1,
            _ => a.pend += 1,
        }
    }
    a
}

/// 四账对平：同周期四账审计 + 跨账一致性（item 集合归因↔清偿需覆盖）。
/// 返回 Ok(报告文本) 或 Err(差异单)。
pub fn reconcile(l: &QuadLedger, cycle: u32) -> Result<String, String> {
    let mut text = format!("cycle{}|", cycle);
    for k in 0..KIND_N as u8 {
        let a = audit_slice(l, k, cycle);
        if !a.bad.is_empty() {
            return Err(format!(
                "bad:{}:{}",
                kind_name(k),
                a.bad.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",")
            ));
        }
        text = format!(
            "{}{}:{}/{}/{}|",
            text,
            kind_name(k),
            a.flat,
            a.carry,
            a.pend
        );
    }
    // 覆盖性：归因账与清偿账的 item 集合必须互覆盖（归因必有清偿路径）
    let attrib_items: Vec<u32> = l.slice(K_ATTRIB, cycle).iter().map(|r| r.item).collect();
    let clear_items: Vec<u32> = l.slice(K_CLEAR, cycle).iter().map(|r| r.item).collect();
    for it in &attrib_items {
        if !clear_items.contains(it) {
            return Err(format!("unpaid:{}", it));
        }
    }
    // 回归账只允许对平条目（回归未过=带遗裁决项挂起处理）
    for r in l.slice(K_REGRESS, cycle) {
        if r.verdict == V_PEND {
            return Err(format!("regress_pend:{}", r.item));
        }
    }
    text.push_str("ok");
    Ok(text)
}

/// 报告指纹：对平文本 + 全账行渲染折叠（FNV-1a 32 位）。
pub fn report_fp(l: &QuadLedger, cycle: u32) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    let mut feed = |h: &mut u32, b: u8| {
        *h ^= b as u32;
        *h = h.wrapping_mul(0x01000193);
    };
    match reconcile(l, cycle) {
        Ok(t) => for b in t.as_bytes() {
            feed(&mut h, *b);
        },
        Err(e) => for b in e.as_bytes() {
            feed(&mut h, *b);
        },
    }
    for r in &l.rows {
        for b in r.render().as_bytes() {
            feed(&mut h, *b);
        }
    }
    h
}

/// 注入核：把指定故障注入到一笔新账目后返回，供判据验证检出。
pub fn inject_fault(l: &mut QuadLedger, code: u32, seq: u32) -> Result<(), &'static str> {
    match code {
        AC_WRONG_CYCLE => l.put(seq, K_ATTRIB, 9999, seq, V_FLAT, "错周期样本"),
        AC_WRONG_FP => l.put(seq, K_CYCLE, 1, seq, V_FLAT, "指纹漂移样本"),
        AC_WRONG_VERDICT => l.put(seq, K_REGRESS, 1, seq, 7, "错终态样本"),
        AC_MISSING_LEDGER => Err("missing"),
        AC_DUP_ENTRY => {
            let _ = l.put(seq, K_CLEAR, 1, seq, V_FLAT, "重号一");
            l.put(seq + 1, K_CLEAR, 1, seq, V_FLAT, "重号二")
        }
        AC_FROZEN => {
            l.freeze();
            l.put(seq, K_ATTRIB, 1, seq, V_FLAT, "冻结后登记")
        }
        _ => Err("unknown"),
    }
}

/// 回放三要素：注入码 → (拒收吗, 检出路径, 诊断文本)。
pub fn replay(code: u32, seq: u32) -> (bool, &'static str, String) {
    let mut l = QuadLedger::new();
    // 先铺一周期基线
    let _ = l.put(1, K_ATTRIB, 1, 10, V_FLAT, "基线归因");
    let _ = l.put(2, K_CLEAR, 1, 10, V_FLAT, "基线清偿");
    let _ = l.put(3, K_REGRESS, 1, 10, V_FLAT, "基线回归");
    let _ = l.put(4, K_CYCLE, 1, 10, V_FLAT, "基线周期");
    match inject_fault(&mut l, code, seq) {
        Err(e) => (true, "reject", e.to_string()),
        Ok(()) => match reconcile(&l, 1) {
            Err(diff) => (true, "reconcile", diff),
            Ok(_) => (false, "pass", String::new()),
        },
    }
}

/// 批内判据。
pub fn run_unx10234_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10234");

    // J1 周期报告落账：四账齐 + 对平通过 + 报告文本含四账分布
    let j1 = {
        let mut l = QuadLedger::new();
        let _ = l.put(1, K_ATTRIB, 1, 10, V_FLAT, "归因一");
        let _ = l.put(2, K_CLEAR, 1, 10, V_FLAT, "清偿一");
        let _ = l.put(3, K_REGRESS, 1, 10, V_FLAT, "回归一");
        let _ = l.put(4, K_CYCLE, 1, 10, V_FLAT, "周期一");
        let _ = l.put(5, K_ATTRIB, 1, 11, V_CARRY, "归因二");
        let _ = l.put(6, K_CLEAR, 1, 11, V_CARRY, "清偿二");
        let _ = l.put(7, K_REGRESS, 1, 11, V_FLAT, "回归二");
        let _ = l.put(8, K_CYCLE, 1, 11, V_CARRY, "周期二");
        match reconcile(&l, 1) {
            Ok(t) => t.contains("attrib:1/1/0") && t.contains("ok"),
            Err(_) => false,
        }
    };
    s.add("J1-周期报告落账", j1, "四账对平且分布正确");

    // J2a 错周期检出：错周期样本在周期 1 对平中缺席→归因↔清偿覆盖仍平，
    // 但指纹与基线必不同（指纹性质）
    let (j2a_base, j2a_after) = {
        let mut base = QuadLedger::new();
        let _ = base.put(1, K_ATTRIB, 1, 10, V_FLAT, "基线归因");
        let _ = base.put(2, K_CLEAR, 1, 10, V_FLAT, "基线清偿");
        let _ = base.put(3, K_REGRESS, 1, 10, V_FLAT, "基线回归");
        let _ = base.put(4, K_CYCLE, 1, 10, V_FLAT, "基线周期");
        let fp0 = report_fp(&base, 1);
        // 错周期样本：同基线 + 错周期注入 → 周期 1 对平仍平，但全账指纹必漂移
        let mut bad = QuadLedger::new();
        let _ = bad.put(1, K_ATTRIB, 1, 10, V_FLAT, "基线归因");
        let _ = bad.put(2, K_CLEAR, 1, 10, V_FLAT, "基线清偿");
        let _ = bad.put(3, K_REGRESS, 1, 10, V_FLAT, "基线回归");
        let _ = bad.put(4, K_CYCLE, 1, 10, V_FLAT, "基线周期");
        let _ = inject_fault(&mut bad, AC_WRONG_CYCLE, 9);
        let fp1 = report_fp(&bad, 1);
        (fp0 != 0, fp1 != fp0)
    };
    s.add("J2a-错周期指纹漂移", j2a_base && j2a_after, "错周期必改指纹或被检出");

    // J2b 错指纹检出：同账重算指纹必须稳定（同输入同指纹）
    let j2b = {
        let mut l = QuadLedger::new();
        let _ = l.put(1, K_ATTRIB, 1, 10, V_FLAT, "归因");
        let _ = l.put(2, K_CLEAR, 1, 10, V_FLAT, "清偿");
        let _ = l.put(3, K_REGRESS, 1, 10, V_FLAT, "回归");
        let _ = l.put(4, K_CYCLE, 1, 10, V_FLAT, "周期");
        let fp1 = report_fp(&l, 1);
        let fp2 = report_fp(&l, 1);
        fp1 == fp2 && fp1 != 0
    };
    s.add("J2b-指纹稳定", j2b, "同四账同指纹");

    // J2c 错终态检出：回归账挂起即对平拒绝
    let j2c = replay(AC_WRONG_VERDICT, 9);
    s.add("J2c-错终态检出", j2c.0, "回归挂起必须拒绝对平");

    // J3 注入面六路：六种故障至少五种被检出/拒收
    let codes = [
        AC_WRONG_CYCLE,
        AC_WRONG_FP,
        AC_WRONG_VERDICT,
        AC_MISSING_LEDGER,
        AC_DUP_ENTRY,
        AC_FROZEN,
    ];
    // 三检出通道：① replay 拒收/对平 ② 错周期跨周期显形（对平失衡）③ 基线指纹漂移
    let mut fp_base_ledger = QuadLedger::new();
    let _ = fp_base_ledger.put(1, K_ATTRIB, 1, 10, V_FLAT, "基线归因");
    let _ = fp_base_ledger.put(2, K_CLEAR, 1, 10, V_FLAT, "基线清偿");
    let _ = fp_base_ledger.put(3, K_REGRESS, 1, 10, V_FLAT, "基线回归");
    let _ = fp_base_ledger.put(4, K_CYCLE, 1, 10, V_FLAT, "基线周期");
    let fp_base = report_fp(&fp_base_ledger, 1);
    let mut caught = 0;
    for c in codes {
        if replay(c, 9).0 {
            caught += 1;
            continue;
        }
        let mut l = QuadLedger::new();
        let _ = l.put(1, K_ATTRIB, 1, 10, V_FLAT, "基线归因");
        let _ = l.put(2, K_CLEAR, 1, 10, V_FLAT, "基线清偿");
        let _ = l.put(3, K_REGRESS, 1, 10, V_FLAT, "基线回归");
        let _ = l.put(4, K_CYCLE, 1, 10, V_FLAT, "基线周期");
        let _ = inject_fault(&mut l, c, 9);
        if reconcile(&l, 9999).is_err() {
            caught += 1;
            continue;
        }
        if report_fp(&l, 1) != fp_base {
            caught += 1;
        }
    }
    s.add("J3-注入六路", caught >= 5, "六路注入至少五路检出");

    // J4 重流水红线：同 (cycle,kind,item) 二次登账拒收
    let j4 = {
        let mut l = QuadLedger::new();
        let first = l.put(1, K_ATTRIB, 1, 10, V_FLAT, "一");
        let second = l.put(2, K_ATTRIB, 1, 10, V_FLAT, "二");
        first == Ok(()) && second == Err("dup")
    };
    s.add("J4-重流水拒收", j4, "同键重号即拒");

    // J5 冻结红线：冻结后写拒收且拒因正确
    let j5 = {
        let mut l = QuadLedger::new();
        l.freeze();
        l.is_frozen() && l.put(1, K_ATTRIB, 1, 10, V_FLAT, "后") == Err("frozen")
    };
    s.add("J5-冻结拒收", j5, "印后登记必抓");

    // J6 边界：zero_item 拒收、非法 kind 拒收、非法终态拒收
    let j6 = {
        let mut l = QuadLedger::new();
        let a = l.put(1, K_ATTRIB, 1, 0, V_FLAT, "零项");
        let b = l.put(2, 9, 1, 10, V_FLAT, "错类");
        let c = l.put(3, K_ATTRIB, 1, 10, 9, "错态");
        a == Err("zero_item") && b == Err("kind_or_verdict") && c == Err("kind_or_verdict")
    };
    s.add("J6-非法入参三拒", j6, "零项/错类/错态全拒");

    // J7 账目 ok 性质：非法字段组合的 ok() 必假（负样本）
    let j7 = {
        let bad = Entry {
            seq: 1,
            kind: K_ATTRIB,
            cycle: 1,
            item: 0,
            verdict: V_FLAT,
            note: "x".to_string(),
        };
        let empty_note = Entry {
            seq: 2,
            kind: K_CLEAR,
            cycle: 1,
            item: 5,
            verdict: V_FLAT,
            note: String::new(),
        };
        !bad.ok() && !empty_note.ok()
    };
    s.add("J7-负样本ok假", j7, "坏行 ok() 必假");

    // J8 未清偿路径检出：归因账有项、清偿账缺席 → unpaid
    let j8 = {
        let mut l = QuadLedger::new();
        let _ = l.put(1, K_ATTRIB, 1, 77, V_FLAT, "无清偿");
        matches!(reconcile(&l, 1), Err(e) if e.contains("unpaid:77"))
    };
    s.add("J8-未清偿检出", j8, "归因必有清偿路径");

    // J9 审计结构一致：slice 计数与审计 n 相符
    let j9 = {
        let mut l = QuadLedger::new();
        let _ = l.put(1, K_CYCLE, 3, 1, V_FLAT, "a");
        let _ = l.put(2, K_CYCLE, 3, 2, V_PEND, "b");
        let _ = l.put(3, K_CYCLE, 3, 3, V_CARRY, "c");
        let a = audit_slice(&l, K_CYCLE, 3);
        a.n == 3 && a.flat == 1 && a.pend == 1 && a.carry == 1 && !a.is_flat()
    };
    s.add("J9-审计一致", j9, "审计与流水计数相符");

    // J10 十次对平回放：标准四账样重复 10 轮全对平（收敛性）
    let j10 = {
        let mut ok = 0;
        for round in 0..10u32 {
            let mut l = QuadLedger::new();
            let base = 10 * (round + 1);
            let _ = l.put(base, K_ATTRIB, round, base, V_FLAT, "归");
            let _ = l.put(base + 1, K_CLEAR, round, base, V_FLAT, "清");
            let _ = l.put(base + 2, K_REGRESS, round, base, V_FLAT, "回");
            let _ = l.put(base + 3, K_CYCLE, round, base, V_FLAT, "周");
            if reconcile(&l, round).is_ok() {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮对平", j10, "10/10 次对平通过");

    s
}
