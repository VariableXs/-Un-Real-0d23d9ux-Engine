//! ves405_s22pairaudit — VE-F4165 · 组间两两对账
//! 八组间 28 对两两对账（家族对账机制第十用声明）；对账五维（接口引用一致/语种矩阵口径一致/
//! 地区码口径一致/交接签名衔接/测试语料共享）；对账证据挂指针；口径分歧→仲裁（家族仲裁流程复述）；
//! 引用断裂→补链；缺对→补齐；多语言无障碍口径专列（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0800;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaCode {
    Ok = 0,
    Divergence = SEG_BASE + 0x01, // 0x0801
    RefBroken = SEG_BASE + 0x02,  // 0x0802
    PairMissing = SEG_BASE + 0x03, // 0x0803
}

/// 对账五维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditDim {
    ApiRef = 0, LangMatrix = 1, RegionCode = 2, HandshakeSig = 3, CorpusShare = 4,
}

impl AuditDim {
    pub const ALL: [AuditDim; 5] = [
        AuditDim::ApiRef, AuditDim::LangMatrix, AuditDim::RegionCode,
        AuditDim::HandshakeSig, AuditDim::CorpusShare];
    pub fn name(self) -> &'static str {
        match self {
            AuditDim::ApiRef => "api-ref", AuditDim::LangMatrix => "lang-matrix",
            AuditDim::RegionCode => "region-code", AuditDim::HandshakeSig => "handshake-sig",
            AuditDim::CorpusShare => "corpus-share",
        }
    }
}

/// 一对对账记录：(组A, 组B) × 五维一致性 + 证据指针 + a11y 专列（域本色）。
#[derive(Clone, Copy)]
pub struct PairAudit {
    pub a: u8, // 组 idx 0-7
    pub b: u8,
    pub dims_ok: [bool; 5],
    pub evidence: u32, // 证据指针（0 = 缺）
    pub a11y_col: bool, // 多语言无障碍口径专列（域本色）
}

impl PairAudit {
    pub fn five_ok(&self) -> bool { self.dims_ok.iter().all(|x| *x) }
    pub fn complete(&self) -> bool { self.five_ok() && self.evidence != 0 && self.a11y_col }
}

/// 对账册。
pub struct PairAuditBook {
    pub groups: [&'static str; 8],
    pub pairs: Vec<PairAudit>,
    pub arbitrations: Vec<&'static str>, // 分歧仲裁留痕
    pub relinks: Vec<&'static str>,      // 补链留痕
}

impl PairAuditBook {
    pub fn new() -> Self {
        PairAuditBook { groups: ["T01", "T02", "T03", "T04", "T05", "T06", "T07", "T08"],
            pairs: Vec::new(), arbitrations: Vec::new(), relinks: Vec::new() }
    }

    /// 铺排 28 对（缺对→补齐）。
    pub fn lay_out(&mut self) -> usize {
        let mut added = 0;
        for i in 0..8u8 {
            for j in (i + 1)..8u8 {
                if !self.pairs.iter().any(|p| (p.a == i && p.b == j) || (p.a == j && p.b == i)) {
                    self.pairs.push(PairAudit {
                        a: i, b: j, dims_ok: [false; 5], evidence: 0, a11y_col: false,
                    });
                    added += 1;
                }
            }
        }
        added
    }

    pub fn pairs_complete(&self) -> bool { self.pairs.len() == 28 }

    /// 挂对账结果。
    pub fn fill(&mut self, a: u8, b: u8, dims_ok: [bool; 5], evidence: u32, a11y: bool) -> bool {
        match self.pairs.iter_mut().find(|p| (p.a == a && p.b == b) || (p.a == b && p.b == a)) {
            Some(p) => { p.dims_ok = dims_ok; p.evidence = evidence; p.a11y_col = a11y; true }
            None => false,
        }
    }

    /// 口径分歧定位：五维未全过且未仲裁的对。
    pub fn divergences(&self) -> Vec<(u8, u8)> {
        self.pairs.iter().filter(|p| !p.five_ok()).map(|p| (p.a, p.b)).collect()
    }

    /// 仲裁（留痕）：把对标记为全过（家族仲裁流程复述）。
    pub fn arbitrate(&mut self, a: u8, b: u8) -> bool {
        match self.pairs.iter_mut().find(|p| (p.a == a && p.b == b) || (p.a == b && p.b == a)) {
            Some(p) if !p.five_ok() => {
                p.dims_ok = [true; 5];
                self.arbitrations.push("family-arbitration");
                true
            }
            _ => false,
        }
    }

    /// 引用断裂：证据指针缺的对 → 补链。
    pub fn broken_refs(&self) -> Vec<(u8, u8)> {
        self.pairs.iter().filter(|p| p.five_ok() && p.evidence == 0).map(|p| (p.a, p.b)).collect()
    }

    pub fn relink(&mut self, a: u8, b: u8, ev: u32) -> bool {
        match self.pairs.iter_mut().find(|p| (p.a == a && p.b == b) || (p.a == b && p.b == a)) {
            Some(p) => { p.evidence = ev; self.relinks.push("ref-relinked"); true }
            None => false,
        }
    }

    /// 全册核验：28 对全部 complete（含 a11y 专列）。
    pub fn all_complete(&self) -> bool { self.pairs.iter().all(|p| p.complete()) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves405_checks() -> CheckSet {
    let mut set = CheckSet::new("ves405");
    let mut bk = PairAuditBook::new();

    // C405-01 28 对铺排。
    chk(&mut set, "C405-01", bk.lay_out() == 28 && bk.pairs_complete(), "28 对铺齐");

    // C405-02 铺排幂等。
    chk(&mut set, "C405-02", bk.lay_out() == 0, "重复铺排不加");

    // C405-03 挂全部对账结果（五维过 + 证据 + a11y 专列）。
    for idx in 0..28usize {
        let (a, b) = { let p = bk.pairs[idx]; (p.a, p.b) };
        bk.fill(a, b, [true; 5], 0xE000 + idx as u32, true);
    }
    chk(&mut set, "C405-03", bk.all_complete(), "28 对全complete");

    // C405-04 口径分歧检出与仲裁。
    let mut bk2 = PairAuditBook::new();
    bk2.lay_out();
    for idx in 0..28usize {
        let (a, b) = { let p = bk2.pairs[idx]; (p.a, p.b) };
        let dims = if idx == 5 { [true, true, false, true, true] } else { [true; 5] };
        bk2.fill(a, b, dims, 0xE100 + idx as u32, true);
    }
    chk(&mut set, "C405-04", bk2.divergences().len() == 1 && !bk2.all_complete(), "一对口径分歧");

    // C405-05 仲裁兜平 + 留痕。
    let div = bk2.divergences()[0];
    chk(&mut set, "C405-05", bk2.arbitrate(div.0, div.1)
        && bk2.arbitrations == vec!["family-arbitration"] && bk2.all_complete(), "仲裁留痕兜平");

    // C405-06 引用断裂检出与补链。
    let mut bk3 = PairAuditBook::new();
    bk3.lay_out();
    for idx in 0..28usize {
        let (a, b) = { let p = bk3.pairs[idx]; (p.a, p.b) };
        let ev = if idx == 9 { 0 } else { 0xE200 + idx as u32 };
        bk3.fill(a, b, [true; 5], ev, true);
    }
    chk(&mut set, "C405-06", bk3.broken_refs().len() == 1, "断链一对");

    // C405-07 补链复通。
    let br = bk3.broken_refs()[0];
    chk(&mut set, "C405-07", bk3.relink(br.0, br.1, 0xE2FF) && bk3.all_complete()
        && bk3.relinks == vec!["ref-relinked"], "补链复通留痕");

    // C405-08 a11y 专列缺 → 不 complete（域本色）。
    let mut bk4 = PairAuditBook::new();
    bk4.lay_out();
    for idx in 0..28usize {
        let (a, b) = { let p = bk4.pairs[idx]; (p.a, p.b) };
        bk4.fill(a, b, [true; 5], 0xE300 + idx as u32, idx != 11);
    }
    chk(&mut set, "C405-08", !bk4.all_complete(), "缺专列不complete");

    // C405-09 五维名单单源。
    chk(&mut set, "C405-09", AuditDim::ALL.len() == 5 && AuditDim::ALL[0].name() == "api-ref"
        && AuditDim::ALL[4].name() == "corpus-share", "五维单源");

    // C405-10 组名单单源。
    chk(&mut set, "C405-10", bk.groups.len() == 8 && bk.groups[0] == "T01", "八组单源");

    // C405-11 缺对补齐：抽走一对后补。
    bk4.pairs.remove(3);
    chk(&mut set, "C405-11", !bk4.pairs_complete() && bk4.lay_out() == 1 && bk4.pairs_complete(), "缺对补齐");

    // C405-12 仲裁只对分歧对生效。
    chk(&mut set, "C405-12", !bk4.arbitrate(0, 1), "无分歧不仲裁");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves405_smoke() {
        let set = run_ves405_checks();
        assert!(set.failed_count() == 0, "ves405 red: {}", set.red_items_text());
    }
}
