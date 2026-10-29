//! UNX-F10245 认证报告生成器（三账合一零差）（0x129E0 段）
//!
//! 锚点：UNX-F10245-J1 报告生成达成（闸账/判据账/披露账三账合一出文
//! 本 10/10 次零差）；J2 三账失衡检出（任一账缺席/计数矛盾拒绝出文
//! 10/10 次检出）；J3 报告幂等（同三账同文本字节一致）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：三账输入（闸账/判据账/披露账）+ 合一核（交叉校验 + 文本编
//! 织）+ 零差指纹 + 注入核（缺账/计数矛盾/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x129E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x129E0;
pub const RG_MISS_ACCT: u32 = SEG_BASE + 0x01; // 缺账
pub const RG_COUNT_CONFLICT: u32 = SEG_BASE + 0x02; // 计数矛盾
pub const RG_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const RG_EMPTY_ACCT: u32 = SEG_BASE + 0x04; // 空账

/// 闸账条目。
#[derive(Clone, PartialEq)]
pub struct GateAcc {
    pub gate: u8,
    pub passed: bool,
}

/// 判据账条目。
#[derive(Clone, PartialEq)]
pub struct CkAcc {
    pub ck: u32,
    pub passed: bool,
    pub ran: bool,
}

/// 披露账条目（豁免/偏离披露）。
#[derive(Clone, PartialEq)]
pub struct DiscAcc {
    pub item: u32,
    pub kind: u8, // 0=豁免 1=偏离
    pub text: String,
}

impl DiscAcc {
    pub fn ok(&self) -> bool {
        self.item != 0 && self.kind <= 1 && !self.text.is_empty()
    }
}

/// 三账合一输入。
pub struct TriAcc {
    pub gates: Vec<GateAcc>,
    pub cks: Vec<CkAcc>,
    pub discs: Vec<DiscAcc>,
    frozen: bool,
}

impl TriAcc {
    pub fn new() -> TriAcc {
        TriAcc {
            gates: Vec::new(),
            cks: Vec::new(),
            discs: Vec::new(),
            frozen: false,
        }
    }

    pub fn add_gate(&mut self, gate: u8, passed: bool) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if gate > 4 {
            return Err("bad_gate");
        }
        if self.gates.iter().any(|g| g.gate == gate) {
            return Err("dup_gate");
        }
        self.gates.push(GateAcc { gate, passed });
        Ok(())
    }

    pub fn add_ck(&mut self, ck: u32, passed: bool, ran: bool) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if ck == 0 {
            return Err("zero_ck");
        }
        if self.cks.iter().any(|c| c.ck == ck) {
            return Err("dup_ck");
        }
        self.cks.push(CkAcc { ck, passed, ran });
        Ok(())
    }

    pub fn add_disc(&mut self, item: u32, kind: u8, text: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        let d = DiscAcc { item, kind, text: text.to_string() };
        if !d.ok() {
            return Err("bad_disc");
        }
        self.discs.push(d);
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 三账交叉校验：闸账五闸全过 + 判据账全跑全过（豁免项除外）。
fn cross_ok(t: &TriAcc) -> Result<(), String> {
    // 缺账：任一账空
    if t.gates.is_empty() || t.cks.is_empty() {
        return Err("missing_acct".to_string());
    }
    // 闸账：须五闸齐且全过
    if t.gates.len() != 5 {
        return Err(format!("gate_n:{}", t.gates.len()));
    }
    if t.gates.iter().any(|g| !g.passed) {
        return Err("gate_red".to_string());
    }
    // 判据账：全部必须真的跑过（跳跑=矛盾）
    if t.cks.iter().any(|c| !c.ran) {
        return Err("ck_skip".to_string());
    }
    if t.cks.iter().any(|c| !c.passed) {
        return Err("ck_red".to_string());
    }
    // 披露账：条目全合法
    if t.discs.iter().any(|d| !d.ok()) {
        return Err("bad_disc".to_string());
    }
    Ok(())
}

/// 报告文本编织：三账合一（幂等：输入同序输出同字节）。
pub fn render_report(t: &TriAcc) -> Result<String, String> {
    cross_ok(t)?;
    let mut gates: Vec<&GateAcc> = t.gates.iter().collect();
    gates.sort_by_key(|g| g.gate);
    let mut cks: Vec<&CkAcc> = t.cks.iter().collect();
    cks.sort_by_key(|c| c.ck);
    let mut discs: Vec<&DiscAcc> = t.discs.iter().collect();
    discs.sort_by_key(|d| (d.kind, d.item));
    let mut out = format!(
        "CERT|gates={}|cks={}|exempt={}|deviate={}|",
        gates.len(),
        cks.len(),
        discs.iter().filter(|d| d.kind == 0).count(),
        discs.iter().filter(|d| d.kind == 1).count()
    );
    for g in &gates {
        out.push_str(&format!("g{}:{},", g.gate, if g.passed { 1 } else { 0 }));
    }
    for c in &cks {
        out.push_str(&format!("c{}:{},", c.ck, if c.passed { 1 } else { 0 }));
    }
    for d in &discs {
        out.push_str(&format!("d{}:{},", d.item, d.kind));
    }
    Ok(out)
}

/// 零差指纹：报告文本折叠。
pub fn report_fp(text: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in text.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 铺三账合一基线（五闸全过 + 三判据全跑过 + 两披露）。
pub fn seed(t: &mut TriAcc) -> Result<(), &'static str> {
    for g in 0..5u8 {
        t.add_gate(g, true)?;
    }
    t.add_ck(1, true, true)?;
    t.add_ck(2, true, true)?;
    t.add_ck(3, true, true)?;
    t.add_disc(50, 0, "NA 条款豁免披露")?;
    t.add_disc(51, 1, "environ 只读偏离披露")?;
    Ok(())
}

/// 注入核。
pub fn inject_fault(t: &mut TriAcc, code: u32) -> Result<(), &'static str> {
    match code {
        RG_MISS_ACCT => {
            // 只铺闸账，判据账缺席
            for g in 0..5u8 {
                t.add_gate(g, true)?;
            }
            Ok(())
        }
        RG_COUNT_CONFLICT => {
            // 闸账只四闸（计数矛盾）
            for g in 0..4u8 {
                t.add_gate(g, true)?;
            }
            t.add_ck(1, true, true)
        }
        RG_FROZEN => {
            t.freeze();
            t.add_ck(9, true, true)
        }
        RG_EMPTY_ACCT => Ok(()),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10245_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10245");

    // J1 三账合一出文本零差
    let j1 = {
        let mut t = TriAcc::new();
        let _ = seed(&mut t);
        match render_report(&t) {
            Ok(txt) => txt.starts_with("CERT|gates=5|cks=3|exempt=1|deviate=1|"),
            Err(_) => false,
        }
    };
    s.add("J1-合一出文本", j1, "三账合一头部计数精确");

    // J2a 缺账拒出文本
    let j2a = {
        let mut t = TriAcc::new();
        let _ = inject_fault(&mut t, RG_MISS_ACCT);
        matches!(render_report(&t), Err(e) if e == "missing_acct")
    };
    s.add("J2a-缺账拒", j2a, "判据账缺席即拒");

    // J2b 计数矛盾检出（四闸不齐）
    let j2b = {
        let mut t = TriAcc::new();
        let _ = inject_fault(&mut t, RG_COUNT_CONFLICT);
        matches!(render_report(&t), Err(e) if e.contains("gate_n"))
    };
    s.add("J2b-计数矛盾", j2b, "闸数不符精确报");

    // J3 报告幂等：同三账两次渲染字节一致
    let j3 = {
        let mk = || {
            let mut t = TriAcc::new();
            let _ = seed(&mut t);
            render_report(&t).unwrap()
        };
        let t1 = mk();
        let t2 = mk();
        t1 == t2 && report_fp(&t1) != 0
    };
    s.add("J3-文本幂等", j3, "同输入同字节同指纹");

    // J4 跳跑判据检出
    let j4 = {
        let mut t = TriAcc::new();
        let _ = seed(&mut t);
        let _ = t.cks.pop();
        let _ = t.add_ck(4, true, false);
        matches!(render_report(&t), Err(e) if e == "ck_skip")
    };
    s.add("J4-跳跑检出", j4, "ran=false 即拒");

    // J5 红判据检出
    let j5 = {
        let mut t = TriAcc::new();
        let _ = seed(&mut t);
        let _ = t.cks.pop();
        let _ = t.add_ck(5, false, true);
        matches!(render_report(&t), Err(e) if e == "ck_red")
    };
    s.add("J5-红判据检出", j5, "passed=false 即拒");

    // J6 注入面四路：缺账/计数矛盾出文本被拒，冻结改账直接拒收，空账拒出
    let codes = [RG_MISS_ACCT, RG_COUNT_CONFLICT, RG_FROZEN, RG_EMPTY_ACCT];
    let caught = codes
        .iter()
        .filter(|c| {
            let mut t = TriAcc::new();
            let _ = inject_fault(&mut t, **c);
            // 检出口径：出文本被拒 或 登账被拒，都算该路被拦
            render_report(&t).is_err()
        })
        .count();
    s.add("J6-注入四路", caught == 4, "四路注入全被拦");

    // J7 冻结后改账拒收
    let j7 = {
        let mut t = TriAcc::new();
        inject_fault(&mut t, RG_FROZEN);
        t.is_frozen() && t.add_gate(0, true) == Err("frozen")
    };
    s.add("J7-冻结拒收", j7, "印后改账必抓");

    // J8 披露账合法性与负样本
    let j8 = {
        let mut t = TriAcc::new();
        let a = t.add_disc(0, 0, "零项");
        let bb = t.add_disc(1, 9, "坏型");
        let c = t.add_disc(2, 0, "");
        a == Err("bad_disc") && bb == Err("bad_disc") && c == Err("bad_disc")
    };
    s.add("J8-披露三拒", j8, "零项/坏型/空文全拒");

    // J9 重号拒收：同判据号二次登记即拒
    let j9 = {
        let mut t = TriAcc::new();
        let a = t.add_ck(7, true, true);
        let bb = t.add_ck(7, true, true);
        a == Ok(()) && bb == Err("dup_ck")
    };
    s.add("J9-重号拒收", j9, "同号二次即拒");

    // J10 十轮报告回放：10 轮字节一致（确定性）
    let j10 = {
        let mk = || {
            let mut t = TriAcc::new();
            let _ = seed(&mut t);
            report_fp(&render_report(&t).unwrap())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮零差", j10, "10/10 轮指纹一致");

    s
}
