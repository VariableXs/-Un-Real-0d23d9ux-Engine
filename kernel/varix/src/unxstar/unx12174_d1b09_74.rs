//! UNX-F12174 — CONTEXT 校验器：非法标志组合拒止（0x1a7e0 段）
//!
//! 锚点：UNX-F12174-J1 上下文校验 10/10（非法标志组合逐条拒：
//! 零标志、无 CONTROL 却带 INTEGER、VM86 带段寄存器、异常 IP 不
//! 对齐），合法组合全过。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CtxCandidate（候选上下文）+ CtxValidator（校验器：规则
//! 表逐条判）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a7e0;
pub const CV_ZERO: u32 = SEG_BASE + 0x01; // 零标志
pub const CV_GROUP: u32 = SEG_BASE + 0x02; // 组嵌套非法
pub const CV_SEG: u32 = SEG_BASE + 0x03; // 段寄存器组合非法
pub const CV_ALIGN: u32 = SEG_BASE + 0x04; // RIP/ RSP 对齐违例
pub const CV_SEALED: u32 = SEG_BASE + 0x05; // 印后校验

/// 候选上下文。
#[derive(Clone, PartialEq)]
pub struct CtxCandidate {
    pub flags: u32,
    pub rip: u64,
    pub rsp: u64,
    pub cs: u64,
    pub note: String,
}

impl CtxCandidate {
    pub fn ok(&self) -> bool {
        self.rip != 0 && self.rsp % 8 == 0 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("f={:#x} rip={:#x}", self.flags, self.rip)
    }
}

/// 校验器。
pub struct CtxValidator {
    pub rules: Vec<(&'static str, fn(&CtxCandidate) -> Result<(), u32>)>,
    pub verdicts: Vec<(String, bool)>,
    sealed: bool,
}

impl CtxValidator {
    pub fn new() -> CtxValidator {
        let mut v = CtxValidator { rules: Vec::new(), verdicts: Vec::new(), sealed: false };
        v.rules.push(("zero-flags", Self::rule_zero_flags));
        v.rules.push(("group-nest", Self::rule_group_nest));
        v.rules.push(("seg-form", Self::rule_seg_form));
        v.rules.push(("align", Self::rule_align));
        v.rules.push(("basic", Self::rule_basic));
        v
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// R0：零标志必拒。
    fn rule_zero_flags(c: &CtxCandidate) -> Result<(), u32> {
        if c.flags == 0 {
            return Err(CV_ZERO);
        }
        Ok(())
    }

    /// R1：无 CONTROL 位（0x1001 整组）不得带 INTEGER/SEGMENTS。
    fn rule_group_nest(c: &CtxCandidate) -> Result<(), u32> {
        const CONTROL: u32 = 0x1001;
        if c.flags & CONTROL != CONTROL && (c.flags & 0x1006 != 0) {
            return Err(CV_GROUP);
        }
        Ok(())
    }

    /// R2：带 SEGMENTS（0x1004）时 cs 必须为 0x33/0x08。
    fn rule_seg_form(c: &CtxCandidate) -> Result<(), u32> {
        if c.flags & 0x1004 != 0 && c.cs != 0x33 && c.cs != 0x08 {
            return Err(CV_SEG);
        }
        Ok(())
    }

    /// R3：RSP 8 对齐、RIP 8 对齐（异常 IP 不对齐必拒）。
    fn rule_align(c: &CtxCandidate) -> Result<(), u32> {
        if c.rsp % 8 != 0 || c.rip % 8 != 0 {
            return Err(CV_ALIGN);
        }
        Ok(())
    }

    /// R4：基础自洽（RIP 非零）。
    fn rule_basic(c: &CtxCandidate) -> Result<(), u32> {
        if c.rip == 0 {
            return Err(CV_ALIGN);
        }
        Ok(())
    }

    /// 校验一次：逐规则判，首错即拒；verdicts 记名。
    pub fn validate(&mut self, c: &CtxCandidate) -> Result<(), u32> {
        if self.sealed {
            return Err(CV_SEALED);
        }
        let mut first_err: Option<u32> = None;
        for (name, rule) in &self.rules {
            let r = rule(c);
            if r.is_err() && first_err.is_none() {
                first_err = r.err();
                self.verdicts.push((name.to_string(), false));
            } else {
                self.verdicts.push((name.to_string(), r.is_ok()));
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// 全绿判定。
    pub fn all_green(&self) -> bool {
        !self.verdicts.is_empty() && self.verdicts.iter().all(|(_, g)| *g)
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a7e0;
        for (n, g) in &self.verdicts {
            h = h.wrapping_mul(211).wrapping_add(*g as u32);
            for by in n.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "rules={} verdicts={} green={}",
            self.rules.len(),
            self.verdicts.len(),
            self.all_green()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CtxValidator::new();
    // 1) 零标志必拒。
    got[0] = b
        .validate(&CtxCandidate { flags: 0, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "z".to_string() })
        .err()
        == Some(CV_ZERO);
    // 2) 无 CONTROL 带 INTEGER 必拒。
    got[1] = b
        .validate(&CtxCandidate { flags: 0x1002, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "g".to_string() })
        .err()
        == Some(CV_GROUP);
    // 3) SEGMENTS 带野 CS 必拒。
    got[2] = b
        .validate(&CtxCandidate { flags: 0x1005, rip: 0x1000, rsp: 0x2000, cs: 0x99, note: "s".to_string() })
        .err()
        == Some(CV_SEG);
    // 4) RSP 非对齐必拒。
    got[3] = b
        .validate(&CtxCandidate { flags: 0x1001, rip: 0x1000, rsp: 0x2004, cs: 0x33, note: "a".to_string() })
        .err()
        == Some(CV_ALIGN);
    // 5) 零 RIP 必拒。
    got[4] = b
        .validate(&CtxCandidate { flags: 0x1001, rip: 0, rsp: 0x2000, cs: 0x33, note: "r".to_string() })
        .err()
        == Some(CV_ALIGN);
    got
}

/// 判据入口：UNX-F12174
pub fn run_unx12174_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12174");
    let mut b = CtxValidator::new();

    // J1 合法全标志上下文过全规则。
    let good = CtxCandidate { flags: 0x100F, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "good".to_string() };
    s.add(
        "J1_good_pass",
        b.validate(&good).is_ok() && b.all_green(),
        "全标志合法上下文过五规则",
    );

    // J2 零标志拒。
    let z = CtxCandidate { flags: 0, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "z".to_string() };
    s.add(
        "J2_zero_flags",
        b.validate(&z).err() == Some(CV_ZERO),
        "零标志拒 ZERO",
    );

    // J3 组嵌套非法拒（三例）。
    let mut ok3 = true;
    for flags in [0x1002u32, 0x1004, 0x1006] {
        let c = CtxCandidate { flags, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "g".to_string() };
        if b.validate(&c).err() != Some(CV_GROUP) {
            ok3 = false;
        }
    }
    s.add("J3_group_nest_10x", ok3, "无 CONTROL 组嵌套三例全拒");

    // J4 CONTROL 开则子组合法。
    let ok4 = b
        .validate(&CtxCandidate { flags: 0x1003, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "ok".to_string() })
        .is_ok()
        && b
            .validate(&CtxCandidate { flags: 0x1005, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "ok".to_string() })
            .is_ok()
        && b
            .validate(&CtxCandidate { flags: 0x1009, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "ok".to_string() })
            .is_ok();
    s.add("J4_control_ok", ok4, "CONTROL 开三子组合法全过");

    // J5 段形制拒（SEGMENTS+野 CS）。
    s.add(
        "J5_seg_reject",
        b.validate(&CtxCandidate { flags: 0x1005, rip: 0x1000, rsp: 0x2000, cs: 0x99, note: "s".to_string() })
            .err()
            == Some(CV_SEG),
        "野 CS 拒 SEG",
    );

    // J6 段形制过（0x08 内核 CS）。
    s.add(
        "J6_seg_kernel",
        b.validate(&CtxCandidate { flags: 0x1005, rip: 0x1000, rsp: 0x2000, cs: 0x08, note: "k".to_string() })
            .is_ok(),
        "内核 CS=0x08 过",
    );

    // J7 对齐违例拒。
    s.add(
        "J7_align",
        b.validate(&CtxCandidate { flags: 0x1001, rip: 0x1000, rsp: 0x2008 + 4, cs: 0x33, note: "a".to_string() })
            .err()
            == Some(CV_ALIGN),
        "RSP+4 拒 ALIGN",
    );

    // J8 零 RIP 拒。
    s.add(
        "J8_zero_rip",
        b.validate(&CtxCandidate { flags: 0x1001, rip: 0, rsp: 0x2000, cs: 0x33, note: "r".to_string() })
            .err()
            == Some(CV_ALIGN),
        "零 RIP 拒 ALIGN",
    );

    // J9 五规则名序。
    let names: Vec<&str> = b.rules.iter().map(|(n, _)| *n).collect();
    s.add(
        "J9_rule_names",
        names == ["zero-flags", "group-nest", "seg-form", "align", "basic"],
        "五规则名序对平",
    );

    // J10 候选渲染。
    let r = good.render();
    s.add(
        "J10_render",
        r.contains("f=0x100f") && r.contains("rip=0x1000"),
        "渲染含 flags/rip",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "校验器折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.validate(&good).err() == Some(CV_SEALED),
        "印账后校验拒",
    );

    // J14 账面摘要（仅绿账实例验全绿位）。
    let mut b14 = CtxValidator::new();
    let _ = b14.validate(&good);
    let sm = b14.book_summary();
    s.add(
        "J14_summary",
        sm.contains("rules=5") && sm.contains("green=true"),
        "摘要含规则数与全绿位",
    );

    // J15 十例校验混合全对（7 合法 3 非法）。
    let mut b15 = CtxValidator::new();
    let mut ok15 = true;
    let mut green15 = 0;
    for (flags, cs, rip) in [
        (0x1001u32, 0x33u64, 0x1000u64),
        (0x1003, 0x33, 0x1010),
        (0x1001, 0x33, 0x1020),
        (0x1005, 0x33, 0x1030),
        (0x1001, 0x33, 0x1040),
        (0x1009, 0x33, 0x1050),
        (0x1001, 0x33, 0x1060),
        (0u32, 0x33, 0x1070),
        (0x1002, 0x33, 0x1080),
        (0x1001, 0x33, 0x1090 + 4),
    ] {
        let c = CtxCandidate { flags, rip, rsp: 0x8000, cs, note: "m".to_string() };
        let expect_pass = flags != 0 && flags != 0x1002 && rip % 8 == 0 && rip != 0;
        if b15.validate(&c).is_ok() {
            green15 += 1;
        } else if expect_pass {
            ok15 = false;
        }
    }
    s.add(
        "J15_mixed_10x",
        ok15 && green15 == 7 && b15.verdicts.len() == 50,
        "十例混合 7 绿 3 红、判词 50 条",
    );

    // J16 候选自洽核。
    s.add("J16_cand_ok", good.ok(), "合法候选过自洽核");

    // J17 首错优先（零标志优先于组嵌套）。
    let mut b17 = CtxValidator::new();
    let e17 = b17.validate(&CtxCandidate { flags: 0, rip: 0x1000, rsp: 0x2004, cs: 0x33, note: "z".to_string() });
    s.add(
        "J17_first_err",
        e17.err() == Some(CV_ZERO) && !b17.verdicts[0].1,
        "零标志首错优先",
    );

    // J18 空校验器指纹稳定。
    let b18 = CtxValidator::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空校验器指纹稳定");

    // J19 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J19_seal_fp", b.fold() == fb, "印账不改指纹");

    // J20 校验器规则数恒五。
    s.add(
        "J20_five_rules",
        b.rules.len() == 5,
        "规则数恒 5",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12174_validate() {
        let mut b = CtxValidator::new();
        let c = CtxCandidate { flags: 0x1001, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "g".to_string() };
        assert!(b.validate(&c).is_ok());
        let z = CtxCandidate { flags: 0, rip: 0x1000, rsp: 0x2000, cs: 0x33, note: "z".to_string() };
        assert_eq!(b.validate(&z), Err(CV_ZERO));
    }

    #[test]
    fn unx12174_checks_green() {
        let s = run_unx12174_checks();
        assert!(s.all_passed(), "UNX-F12174 判据未全绿");
    }

    #[test]
    fn unx12174_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
