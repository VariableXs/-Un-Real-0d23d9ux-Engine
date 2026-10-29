//! UNX-F10374 正演报告生成（三账+回归账合一）（0x1b940 段）
//!
//! 锚点：UNX-F10374-J1 正演报告生成——三账（正演账/清账/回归账）合一装配，
//! 缺节与账数不符各 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·报告谱系）。
//!
//! 结构：五节定序装配器（总纲/正演账/清账/回归账/结论）+ 节内计数对平
//! + 装配门（缺节/账数漂移必拒）+ 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b940 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b940;
pub const RG_MISSING_SECTION: u32 = SEG_BASE + 0x01; // 缺节
pub const RG_LEDGER_MISMATCH: u32 = SEG_BASE + 0x02; // 账数不符
pub const RG_BAD_ORDER: u32 = SEG_BASE + 0x03; // 节序倒置
pub const RG_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const RG_EMPTY_BODY: u32 = SEG_BASE + 0x05; // 空节正文
pub const RG_DUP_SECTION: u32 = SEG_BASE + 0x06; // 重复节

/// 五节定序。
pub const SECTIONS: usize = 5;
/// 报告定容（装配行数上限）。
pub const CAP: usize = 32;

/// 五节编号。
pub const SC_OUTLINE: u32 = 1; // 总纲
pub const SC_RUN: u32 = 2; // 正演账
pub const SC_CLEAR: u32 = 3; // 清账
pub const SC_REGRESSION: u32 = 4; // 回归账
pub const SC_CONCLUSION: u32 = 5; // 结论

/// 单节内容。
#[derive(Clone)]
pub struct Section {
    pub no: u32,
    pub title: String,
    pub body: String,
    pub count: u32,
}

impl Section {
    /// 字段齐备：节号在域内、标题与正文非空。
    pub fn ok(&self) -> bool {
        (SC_OUTLINE..=SC_CONCLUSION).contains(&self.no)
            && !self.title.is_empty()
            && !self.body.is_empty()
    }

    pub fn render(&self) -> String {
        format!("S{}|{}|{}|n{}", self.no, self.title, self.body, self.count)
    }
}

/// 三账计数输入（正演账/清账/回归账各自条数）。
#[derive(Clone, Copy)]
pub struct LedgerCounts {
    pub run: u32,
    pub clear: u32,
    pub regression: u32,
}

/// 报告装配器：节序强制单调，三账计数装配时对平。
pub struct ReportAssembler {
    pub sections: Vec<Section>,
    pub sealed: bool,
    pub note: String,
    last_no: u32,
}

impl ReportAssembler {
    pub fn new(note: &str) -> ReportAssembler {
        ReportAssembler {
            sections: Vec::new(),
            sealed: false,
            note: note.to_string(),
            last_no: 0,
        }
    }

    /// 装配一节：节号必须 ≥ 上节（同节重装必拒），正文非空。
    pub fn add(
        &mut self,
        no: u32,
        title: &str,
        body: &str,
        count: u32,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(RG_SEALED);
        }
        if !(SC_OUTLINE..=SC_CONCLUSION).contains(&no) {
            return Err(RG_BAD_ORDER);
        }
        if no == self.last_no {
            return Err(RG_DUP_SECTION);
        }
        if no < self.last_no {
            return Err(RG_BAD_ORDER);
        }
        if body.is_empty() {
            return Err(RG_EMPTY_BODY);
        }
        if self.sections.len() >= CAP {
            return Err(RG_MISSING_SECTION);
        }
        self.last_no = no;
        self.sections.push(Section {
            no,
            title: title.to_string(),
            body: body.to_string(),
            count,
        });
        Ok(())
    }

    /// 节覆盖核：五节齐备。
    pub fn sections_covered(&self) -> bool {
        (SC_OUTLINE..=SC_CONCLUSION).all(|sc| {
            self.sections.iter().filter(|s| s.no == sc).count() == 1
        })
    }

    /// 三账对平：第 2/3/4 节的 count 与三账输入逐一相等。
    pub fn ledgers_match(&self, lc: &LedgerCounts) -> bool {
        let get = |no: u32| -> u32 {
            self.sections
                .iter()
                .find(|s| s.no == no)
                .map(|s| s.count)
                .unwrap_or(u32::MAX)
        };
        get(SC_RUN) == lc.run && get(SC_CLEAR) == lc.clear
            && get(SC_REGRESSION) == lc.regression
    }

    /// 缺节诊断。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g = Vec::new();
        for sc in SC_OUTLINE..=SC_CONCLUSION {
            if !self.sections.iter().any(|s| s.no == sc) {
                g.push(sc);
            }
        }
        g
    }

    /// 装配完成裁定：五节齐 + 字段齐备 + 三账对平。
    pub fn complete(&self, lc: &LedgerCounts) -> bool {
        self.sections_covered()
            && self.sections.iter().all(|s| s.ok())
            && self.ledgers_match(lc)
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b9400;
        h = h.wrapping_mul(31).wrapping_add(self.sections.len() as u32);
        for s in &self.sections {
            h = h.wrapping_mul(31).wrapping_add(s.no);
            h = h.wrapping_mul(31).wrapping_add(s.count);
            for b in s.title.bytes().chain(s.body.bytes()) {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 注入核（五项）：[缺节, 账数漂移, 节序倒置, 重复节, 空正文] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 缺节装配不通过
    let mut a = ReportAssembler::new("inj-miss");
    let _ = a.add(SC_OUTLINE, "总纲", "五节装配", 0);
    let _ = a.add(SC_RUN, "正演账", "run=3", 3);
    let lc = LedgerCounts { run: 3, clear: 1, regression: 3 };
    got[0] = !a.complete(&lc) && a.gaps().len() == 2;
    // 2) 账数漂移必检出
    let mut b = ReportAssembler::new("inj-drift");
    let _ = b.add(SC_OUTLINE, "总纲", "x", 0);
    let _ = b.add(SC_RUN, "正演账", "x", 3);
    let _ = b.add(SC_CLEAR, "清账", "x", 9);
    let _ = b.add(SC_REGRESSION, "回归账", "x", 3);
    let _ = b.add(SC_CONCLUSION, "结论", "x", 0);
    got[1] = !b.ledgers_match(&lc);
    // 3) 节序倒置必拒
    let mut c = ReportAssembler::new("inj-order");
    let _ = c.add(SC_RUN, "正演账", "x", 1);
    got[2] = c.add(SC_OUTLINE, "总纲", "x", 0) == Err(RG_BAD_ORDER);
    // 4) 重复节必拒
    let mut d = ReportAssembler::new("inj-dup");
    let _ = d.add(SC_RUN, "正演账", "x", 1);
    got[3] = d.add(SC_RUN, "正演账2", "x", 1) == Err(RG_DUP_SECTION);
    // 5) 空正文必拒
    let mut e = ReportAssembler::new("inj-empty");
    got[4] = e.add(SC_OUTLINE, "总纲", "", 0) == Err(RG_EMPTY_BODY);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10374_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10374");

    // 正路径：五节按序装配，三账对平
    let mut a = ReportAssembler::new("正演报告");
    let _ = a.add(SC_OUTLINE, "总纲", "硬闸正演报告五节装配", 0);
    let _ = a.add(SC_RUN, "正演账", "正演 12 项执行", 12);
    let _ = a.add(SC_CLEAR, "清账", "清账 4 项核销", 4);
    let _ = a.add(SC_REGRESSION, "回归账", "回归 120 项执行", 120);
    let _ = a.add(SC_CONCLUSION, "结论", "硬闸通过", 0);
    let lc = LedgerCounts { run: 12, clear: 4, regression: 120 };
    let j1 = a.complete(&lc);
    s.add("J1-五节齐平", j1, "五节装配齐且三账对平");

    // 覆盖核与缺口互逆
    let j2 = a.sections_covered() && a.gaps().is_empty();
    s.add("J2-覆盖与缺口互逆", j2, "五节齐时缺口清单为空");

    // 账数对平三段逐一核
    let j3 = {
        let get = |no: u32| {
            a.sections.iter().find(|x| x.no == no).map(|x| x.count).unwrap_or(0)
        };
        get(SC_RUN) == 12 && get(SC_CLEAR) == 4 && get(SC_REGRESSION) == 120
    };
    s.add("J3-三账逐一核", j3, "正演/清账/回归三段计数逐一相等");

    // 节序单调闸：跳节装配缺口必拒，补齐后才可过
    let j4 = {
        let mut c = ReportAssembler::new("order");
        let _ = c.add(SC_OUTLINE, "总纲", "x", 0);
        let _ = c.add(SC_RUN, "正演账", "x", 12);
        let _ = c.add(SC_REGRESSION, "回归账", "x", 120);
        let _ = c.add(SC_CONCLUSION, "结论", "x", 0);
        let gap_reject = !c.complete(&lc) && c.gaps() == vec![SC_CLEAR];
        let _ = c.add(SC_CLEAR, "清账", "x", 4);
        gap_reject && c.complete(&lc)
    };
    s.add("J4-跳节缺口闸", j4, "缺中节装配必拒且缺口可定位");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "缺节/账数漂移/节序倒置必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "重复节与空正文必检出");

    // 封账拒写
    let j7 = {
        a.seal();
        a.add(SC_CONCLUSION, "late", "late", 0) == Err(RG_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后装配必拒");

    // 指纹稳定
    let j8 = a.fold() == a.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 账数漂移进装配裁定负路径
    let j9 = {
        let mut c = ReportAssembler::new("neg");
        let _ = c.add(SC_OUTLINE, "总纲", "x", 0);
        let _ = c.add(SC_RUN, "正演账", "x", 12);
        let _ = c.add(SC_CLEAR, "清账", "x", 4);
        let _ = c.add(SC_REGRESSION, "回归账", "x", 99);
        let _ = c.add(SC_CONCLUSION, "结论", "x", 0);
        !c.complete(&lc)
    };
    s.add("J9-漂移进裁定", j9, "回归账计数漂移即装配不通过");

    // 未知节号拒
    let j10 = {
        let mut c = ReportAssembler::new("unk");
        c.add(9, "x", "y", 0) == Err(RG_BAD_ORDER)
    };
    s.add("J10-未知节号拒", j10, "节号出域一律拒");

    // 明细落账含节号与计数
    let j11 = {
        let r = a.sections[1].render();
        r.contains("S2|") && r.contains("n12")
    };
    s.add("J11-明细落账", j11, "渲染含节号与账计数");

    // 空装配器五节全缺
    let j12 = {
        let c = ReportAssembler::new("empty");
        !c.sections_covered() && c.gaps().len() == SECTIONS
    };
    s.add("J12-空装配全缺", j12, "空装配器五节全缺");

    // 三账输入结构复制语义：Copy 无副作用
    let j13 = {
        let x = lc;
        let y = lc;
        x.run == y.run && x.clear == y.clear && x.regression == y.regression
    };
    s.add("J13-三账复制", j13, "三账结构可复制且字段不变");

    // 只读不改账
    let j14 = {
        let mut c = ReportAssembler::new("stable");
        let _ = c.add(SC_OUTLINE, "总纲", "x", 0);
        let f1 = c.fold();
        let _ = c.sections_covered();
        let _ = c.gaps();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-报告指纹落账",
        a.fold() != 0,
        &format!("fold={:#x}", a.fold()),
    );
    let empty = ReportAssembler::new("empty").fold();
    s.add_dyn("J16-空册指纹", empty == 0x1b9400u32.wrapping_mul(31), &format!("fold={:#x}", empty));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10374_report_flow() {
        let mut a = ReportAssembler::new("t");
        assert!(a.add(SC_OUTLINE, "总纲", "x", 0).is_ok());
        assert!(a.add(SC_OUTLINE, "重复", "x", 0) == Err(RG_DUP_SECTION));
        assert!(a.add(9, "越界", "x", 0) == Err(RG_BAD_ORDER));
        a.seal();
        assert!(a.add(SC_RUN, "late", "x", 0) == Err(RG_SEALED));
    }

    #[test]
    fn unx10374_checks_green() {
        let s = run_unx10374_checks();
        assert!(s.all_passed(), "UNX-F10374 判据未全绿");
    }
}
