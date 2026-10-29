//! UNX-F10371 正演现场产物留痕（0x1b8e0 段）
//!
//! 锚点：UNX-F10371-J1 正演现场产物留痕——五段环节产物全留，乱序留痕与
//! 空产物各 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·留痕谱系）。
//!
//! 结构：五段环节定序链（脚本/条件/回归/判定/报告）+ 留痕登记器（段序强制
//! 单调递增）+ 产物指纹（非空产物必携哈希）+ 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b8e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b8e0;
pub const EV_OUT_OF_ORDER: u32 = SEG_BASE + 0x01; // 段序倒置
pub const EV_EMPTY_ARTIFACT: u32 = SEG_BASE + 0x02; // 空产物
pub const EV_DUP_STAGE: u32 = SEG_BASE + 0x03; // 重复段
pub const EV_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const EV_UNKNOWN_STAGE: u32 = SEG_BASE + 0x05; // 未知段号
pub const EV_CAP: u32 = SEG_BASE + 0x06; // 超容量

/// 五段环节定容（链固定五段）。
pub const STAGES: usize = 5;
/// 留痕明细定容（每段允许的产物条数上限）。
pub const CAP: usize = 32;

/// 五段环节编号（固定语义序）。
pub const ST_SCRIPT: u32 = 1; // 正演脚本
pub const ST_COND: u32 = 2; // 条件终检
pub const ST_REGRESSION: u32 = 3; // 全量回归
pub const ST_VERDICT: u32 = 4; // 正演判定
pub const ST_REPORT: u32 = 5; // 正演报告

/// 单条留痕：段号 + 产物名 + 产物指纹。
#[derive(Clone)]
pub struct EvidenceLine {
    pub stage: u32,
    pub artifact: String,
    pub fp: u32,
}

impl EvidenceLine {
    /// 字段齐备：段号在五段域内、产物名非空、指纹非零。
    pub fn ok(&self) -> bool {
        self.artifact_ok() && self.fp != 0
    }

    /// 段号合法（1..=5）。
    pub fn artifact_ok(&self) -> bool {
        (ST_SCRIPT..=ST_REPORT).contains(&self.stage) && !self.artifact.is_empty()
    }

    pub fn render(&self) -> String {
        format!("E{}|{}|{:#x}", self.stage, self.artifact, self.fp)
    }
}

/// 留痕链：段序强制单调，同段可多条但不得回跳。
pub struct EvidenceChain {
    pub lines: Vec<EvidenceLine>,
    pub sealed: bool,
    pub note: String,
    last_stage: u32,
}

impl EvidenceChain {
    pub fn new(note: &str) -> EvidenceChain {
        EvidenceChain {
            lines: Vec::new(),
            sealed: false,
            note: note.to_string(),
            last_stage: 0,
        }
    }

    /// 留痕：段号必须 ≥ 上次段号（同段允许追加，回跳必拒）。
    pub fn record(
        &mut self,
        stage: u32,
        artifact: &str,
        fp: u32,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if !(ST_SCRIPT..=ST_REPORT).contains(&stage) {
            return Err(EV_UNKNOWN_STAGE);
        }
        if artifact.is_empty() {
            return Err(EV_EMPTY_ARTIFACT);
        }
        if stage < self.last_stage {
            return Err(EV_OUT_OF_ORDER);
        }
        if fp == 0 {
            return Err(EV_EMPTY_ARTIFACT);
        }
        if self.lines.len() >= CAP {
            return Err(EV_CAP);
        }
        self.last_stage = stage;
        self.lines.push(EvidenceLine {
            stage,
            artifact: artifact.to_string(),
            fp,
        });
        Ok(())
    }

    /// 段覆盖核：五段各有至少一条留痕。
    pub fn stages_covered(&self) -> bool {
        (ST_SCRIPT..=ST_REPORT).all(|st| self.lines.iter().any(|l| l.stage == st))
    }

    /// 重复段清点（同段多条留痕的段数——允许追加，但审计要看见）。
    pub fn multi_stage_count(&self) -> usize {
        let mut n = 0;
        for st in ST_SCRIPT..=ST_REPORT {
            if self.lines.iter().filter(|l| l.stage == st).count() > 1 {
                n += 1;
            }
        }
        n
    }

    /// 留痕总数。
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 全留裁定：五段覆盖且每条留痕字段齐备。
    pub fn all_evidenced(&self) -> bool {
        self.stages_covered() && self.lines.iter().all(|l| l.ok())
    }

    /// 段缺口诊断：缺留痕的段号列表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g = Vec::new();
        for st in ST_SCRIPT..=ST_REPORT {
            if !self.lines.iter().any(|l| l.stage == st) {
                g.push(st);
            }
        }
        g
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b8e00;
        h = h.wrapping_mul(31).wrapping_add(self.lines.len() as u32);
        for l in &self.lines {
            h = h.wrapping_mul(31).wrapping_add(l.stage);
            h = h.wrapping_mul(31).wrapping_add(l.fp);
            for b in l.artifact.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 产物指纹合成（段号+产物名折叠）。
pub fn fp_of(stage: u32, artifact: &str) -> u32 {
    let mut h = 0x0e5a_0000u32;
    h = h.wrapping_mul(31).wrapping_add(stage);
    for b in artifact.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    h
}

/// 注入核（五项）：[回跳, 空产物, 未知段, 零指纹, 封账写账] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 段序回跳必拒
    let mut c = EvidenceChain::new("inj-back");
    let _ = c.record(ST_REGRESSION, "reg.out", fp_of(ST_REGRESSION, "reg.out"));
    got[0] = c.record(ST_SCRIPT, "run.sh", fp_of(ST_SCRIPT, "run.sh"))
        == Err(EV_OUT_OF_ORDER);
    // 2) 空产物必拒
    let mut c2 = EvidenceChain::new("inj-empty");
    got[1] = c2.record(ST_SCRIPT, "", fp_of(ST_SCRIPT, "x")) == Err(EV_EMPTY_ARTIFACT);
    // 3) 未知段号必拒
    let mut c3 = EvidenceChain::new("inj-unk");
    got[2] = c3.record(9, "x.out", fp_of(9, "x.out")) == Err(EV_UNKNOWN_STAGE);
    // 4) 零指纹必拒（空产物口径）
    let mut c4 = EvidenceChain::new("inj-fp");
    got[3] = c4.record(ST_SCRIPT, "x.out", 0) == Err(EV_EMPTY_ARTIFACT);
    // 5) 封账后写账必抓
    let mut c5 = EvidenceChain::new("inj-seal");
    let _ = c5.record(ST_SCRIPT, "a.out", fp_of(ST_SCRIPT, "a.out"));
    c5.seal();
    got[4] = c5.record(ST_COND, "b.out", fp_of(ST_COND, "b.out")) == Err(EV_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10371_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10371");

    // 正路径：五段环节按序各留一痕
    let mut chain = EvidenceChain::new("正演现场留痕");
    let names = ["run.sh", "cond.txt", "reg.out", "verdict.txt", "report.md"];
    for (k, nm) in names.iter().enumerate() {
        let st = ST_SCRIPT + k as u32;
        let _ = chain.record(st, nm, fp_of(st, nm));
    }
    let j1 = chain.all_evidenced();
    s.add("J1-五段全留", j1, "五段环节按序留痕且字段齐备");

    // 段覆盖核与缺口诊断互逆
    let j2 = chain.stages_covered() && chain.gaps().is_empty();
    s.add("J2-覆盖与缺口互逆", j2, "全留时缺口清单为空");

    // 缺段诊断：只留三段的链缺口可枚举
    let j3 = {
        let mut c = EvidenceChain::new("gaps");
        let _ = c.record(ST_SCRIPT, "a", fp_of(ST_SCRIPT, "a"));
        let _ = c.record(ST_COND, "b", fp_of(ST_COND, "b"));
        let _ = c.record(ST_REPORT, "c", fp_of(ST_REPORT, "c"));
        c.gaps() == vec![ST_REGRESSION, ST_VERDICT]
    };
    s.add("J3-缺口可枚举", j3, "缺留痕段号全部列出");

    // 容量边界
    let j4 = {
        let mut c = EvidenceChain::new("cap");
        let mut filled = true;
        let mut i = 0u32;
        while i < CAP as u32 {
            if c.record(ST_REPORT, "x", fp_of(ST_REPORT, "x")).is_err() {
                filled = false;
            }
            i += 1;
        }
        filled && c.record(ST_SCRIPT, "over", fp_of(ST_SCRIPT, "over")) == Err(EV_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "回跳/空产物/未知段必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "零指纹与封账写账必检出");

    // 封账拒写
    let j7 = {
        chain.seal();
        chain.record(ST_SCRIPT, "late", fp_of(ST_SCRIPT, "late")) == Err(EV_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后留痕必拒");

    // 指纹稳定
    let j8 = chain.fold() == chain.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 同段追加合法且审计可见
    let j9 = {
        let mut c = EvidenceChain::new("append");
        let _ = c.record(ST_SCRIPT, "a1", fp_of(ST_SCRIPT, "a1"));
        let _ = c.record(ST_SCRIPT, "a2", fp_of(ST_SCRIPT, "a2"));
        c.multi_stage_count() == 1 && c.len() == 2
    };
    s.add("J9-同段追加", j9, "同段多条留痕合法且可审计");

    // 指纹口径：同产物名不同段 → 不同指纹
    let j10 = fp_of(ST_SCRIPT, "x") != fp_of(ST_COND, "x")
        && fp_of(ST_SCRIPT, "x") != fp_of(ST_SCRIPT, "y");
    s.add("J10-指纹口径", j10, "段号或产物名变则指纹变");

    // 明细落账含段号与指纹
    let j11 = {
        let r = chain.lines[0].render();
        r.contains("E1|") && r.contains("|0x")
    };
    s.add("J11-明细落账", j11, "渲染含段号与指纹字样");

    // 空链不构成全留
    let j12 = {
        let c = EvidenceChain::new("empty");
        !c.stages_covered() && !c.all_evidenced() && c.gaps().len() == STAGES
    };
    s.add("J12-空链非全留", j12, "空链五段全缺");

    // 边界段号合法：1 与 5 都收
    let j13 = {
        let mut c = EvidenceChain::new("bounds");
        c.record(ST_SCRIPT, "a", fp_of(ST_SCRIPT, "a")).is_ok()
            && c.record(ST_REPORT, "b", fp_of(ST_REPORT, "b")).is_ok()
    };
    s.add("J13-边界段号", j13, "段号 1 与 5 合法");

    // 只读不改账
    let j14 = {
        let mut c = EvidenceChain::new("stable");
        let _ = c.record(ST_SCRIPT, "a", fp_of(ST_SCRIPT, "a"));
        let f1 = c.fold();
        let _ = c.stages_covered();
        let _ = c.multi_stage_count();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-账面指纹落账",
        chain.fold() != 0,
        &format!("fold={:#x}", chain.fold()),
    );
    let empty = EvidenceChain::new("empty").fold();
    s.add_dyn("J16-空册指纹", empty == 0x1b8e00u32.wrapping_mul(31), &format!("fold={:#x}", empty));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10371_evidence_flow() {
        let mut c = EvidenceChain::new("t");
        assert!(c.record(ST_SCRIPT, "a", fp_of(ST_SCRIPT, "a")).is_ok());
        assert!(
            c.record(ST_SCRIPT, "a2", fp_of(ST_SCRIPT, "a2")).is_ok()
        );
        assert!(
            c.record(ST_COND, "b", fp_of(ST_COND, "b")).is_ok()
        );
        assert!(
            c.record(ST_SCRIPT, "back", fp_of(ST_SCRIPT, "back"))
                == Err(EV_OUT_OF_ORDER)
        );
        c.seal();
        assert!(
            c.record(ST_COND, "late", fp_of(ST_COND, "late")) == Err(EV_SEALED)
        );
    }

    #[test]
    fn unx10371_checks_green() {
        let s = run_unx10371_checks();
        assert!(s.all_passed(), "UNX-F10371 判据未全绿");
    }
}
