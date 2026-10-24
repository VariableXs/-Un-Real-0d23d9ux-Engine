//! ves395_s21fuzzgate — VE-F4155 · i18n fuzz 总闸
//! T 域 fuzz 面总归集（T04 三面 + T05 三面 + T06 三面 + T07 三面 + 本组新面 = 总面册）；
//! 总闸通过线（崩溃 0/挂起 0/越权写 0 三零线）确认；最小复现库统一；
//! 四源归一（F4078/F4098/F4114/F4134）；线未达→阻断+归因；新崩溃→立案入库；面缺→补面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0000;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FgCode {
    Ok = 0,
    GateBlocked = SEG_BASE + 0x01,   // 0x0001
    FaceMissing = SEG_BASE + 0x02,   // 0x0002
    CrashFiled = SEG_BASE + 0x03,    // 0x0003
}

/// fuzz 面来源（四源归一 + 本组新面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceSource { T04 = 0, T05 = 1, T06 = 2, T07 = 3, T08New = 4 }

impl FaceSource {
    pub const ALL: [FaceSource; 5] = [
        FaceSource::T04, FaceSource::T05, FaceSource::T06, FaceSource::T07, FaceSource::T08New];
    pub fn name(self) -> &'static str {
        match self {
            FaceSource::T04 => "T04", FaceSource::T05 => "T05", FaceSource::T06 => "T06",
            FaceSource::T07 => "T07", FaceSource::T08New => "T08-new",
        }
    }
}

/// fuzz 面：来源 + 面名 + 语料数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzFace {
    pub source: FaceSource,
    pub name: &'static str,
    pub corpus: u32,
}

/// 总面册：归集 + 三零线确认。
pub struct FaceBook {
    pub faces: Vec<FuzzFace>,
    pub crashes: u32,
    pub hangs: u32,
    pub unauthorized_writes: u32,
    pub repros: Vec<&'static str>, // 最小复现库（统一）
}

impl FaceBook {
    pub fn new() -> Self {
        FaceBook { faces: Vec::new(), crashes: 0, hangs: 0, unauthorized_writes: 0, repros: Vec::new() }
    }

    /// 归集一面；同源同名幂等。
    pub fn add(&mut self, f: FuzzFace) {
        if !self.faces.iter().any(|x| x.source == f.source && x.name == f.name) {
            self.faces.push(f);
        }
    }

    /// 四源（+本组新面）归一完备：每源至少一面。
    pub fn sources_covered(&self) -> bool {
        FaceSource::ALL.iter().all(|s| self.faces.iter().any(|f| f.source == *s))
    }

    /// 缺源补面定位。
    pub fn missing_sources(&self) -> Vec<&'static str> {
        FaceSource::ALL.iter()
            .filter(|s| !self.faces.iter().any(|f| f.source == **s))
            .map(|s| s.name()).collect()
    }

    /// 三零线确认：崩溃 0 / 挂起 0 / 越权写 0。
    pub fn zero_line_ok(&self) -> bool {
        self.crashes == 0 && self.hangs == 0 && self.unauthorized_writes == 0
    }

    /// 总闸裁决：面齐 + 三零线 → 放行；否则阻断。
    pub fn gate(&self) -> Result<(), FgCode> {
        if !self.sources_covered() { Err(FgCode::FaceMissing) }
        else if !self.zero_line_ok() { Err(FgCode::GateBlocked) }
        else { Ok(()) }
    }

    /// 新崩溃立案入库 + 最小复现统一入库。
    pub fn file_crash(&mut self, repro: &'static str) -> bool {
        if repro.is_empty() { return false; }
        self.crashes += 1;
        if !self.repros.contains(&repro) { self.repros.push(repro); }
        true
    }

    /// 归因：给出破线维度名。
    pub fn attribute(&self) -> Option<&'static str> {
        if self.crashes > 0 { Some("crash") }
        else if self.hangs > 0 { Some("hang") }
        else if self.unauthorized_writes > 0 { Some("unauth-write") }
        else { None }
    }

    /// 归一后语料总量。
    pub fn corpus_total(&self) -> u32 { self.faces.iter().map(|f| f.corpus).sum() }
}

/// 阻断归因报告。
pub struct BlockReport { pub reason: &'static str, pub gate: FgCode }

pub fn block_report(fb: &FaceBook) -> Option<BlockReport> {
    match fb.gate() {
        Ok(()) => None,
        Err(c) => Some(BlockReport {
            reason: match c {
                FgCode::FaceMissing => "面缺——补面后重闸",
                FgCode::GateBlocked => "三零线破——归因立案",
                _ => "未知",
            },
            gate: c,
        }),
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves395_checks() -> CheckSet {
    let mut set = CheckSet::new("ves395");
    let mut fb = FaceBook::new();

    // C395-01 四源归一：T04-T07 各三面 + 本组新面（审查器畸形/兼容矩阵/扩展契约）。
    fb.add(FuzzFace { source: FaceSource::T04, name: "extract", corpus: 120 });
    fb.add(FuzzFace { source: FaceSource::T05, name: "manifest", corpus: 90 });
    fb.add(FuzzFace { source: FaceSource::T06, name: "locale-tag", corpus: 80 });
    fb.add(FuzzFace { source: FaceSource::T07, name: "cost-input", corpus: 60 });
    fb.add(FuzzFace { source: FaceSource::T08New, name: "linter-rules", corpus: 45 });
    chk(&mut set, "C395-01", fb.sources_covered() && fb.faces.len() == 5, "五源归一");

    // C395-02 同源同名幂等。
    fb.add(FuzzFace { source: FaceSource::T04, name: "extract", corpus: 121 });
    chk(&mut set, "C395-02", fb.faces.len() == 5, "归集幂等");

    // C395-03 三零线确认（全零）。
    chk(&mut set, "C395-03", fb.zero_line_ok() && fb.gate().is_ok(), "三零线过闸");

    // C395-04 语料总量归一。
    chk(&mut set, "C395-04", fb.corpus_total() == 395, "语料 395");

    // C395-05 新崩溃立案入库 + 复现统一。
    chk(&mut set, "C395-05", fb.file_crash("repro:locale-tag-ovf") && fb.crashes == 1
        && fb.repros == vec!["repro:locale-tag-ovf"], "崩溃立案入库");

    // C395-06 破线 → 阻断 + 归因。
    let br = block_report(&fb);
    chk(&mut set, "C395-06", br.is_some() && br.unwrap().gate == FgCode::GateBlocked
        && fb.attribute() == Some("crash"), "破线阻断归因 crash");

    // C395-07 复现幂等：同签名不重复入库。
    fb.file_crash("repro:locale-tag-ovf");
    chk(&mut set, "C395-07", fb.repros.len() == 1 && fb.crashes == 2, "复现幂等计数仍增");

    // C395-08 立案清零复闸：修复后三零线恢复。
    fb.crashes = 0;
    chk(&mut set, "C395-08", fb.gate().is_ok() && block_report(&fb).is_none(), "清零复闸放行");

    // C395-09 面缺 → 补面定位。
    let mut fb2 = FaceBook::new();
    fb2.add(FuzzFace { source: FaceSource::T04, name: "extract", corpus: 10 });
    fb2.add(FuzzFace { source: FaceSource::T06, name: "locale-tag", corpus: 10 });
    chk(&mut set, "C395-09", fb2.missing_sources() == vec!["T05", "T07", "T08-new"]
        && fb2.gate() == Err(FgCode::FaceMissing), "缺三源定位");

    // C395-10 补面后归齐。
    fb2.add(FuzzFace { source: FaceSource::T05, name: "manifest", corpus: 10 });
    fb2.add(FuzzFace { source: FaceSource::T07, name: "cost-input", corpus: 10 });
    fb2.add(FuzzFace { source: FaceSource::T08New, name: "linter-rules", corpus: 10 });
    chk(&mut set, "C395-10", fb2.sources_covered() && fb2.gate().is_ok(), "补面归齐过闸");

    // C395-11 五源名单单源。
    chk(&mut set, "C395-11", FaceSource::ALL.len() == 5 && FaceSource::ALL[0].name() == "T04"
        && FaceSource::ALL[4].name() == "T08-new", "五源单源");

    // C395-12 越权写破线归因。
    let mut fb3 = FaceBook::new();
    for s in FaceSource::ALL.iter() {
        fb3.add(FuzzFace { source: *s, name: "x", corpus: 1 });
    }
    fb3.unauthorized_writes = 1;
    chk(&mut set, "C395-12", !fb3.zero_line_ok() && fb3.attribute() == Some("unauth-write")
        && fb3.gate() == Err(FgCode::GateBlocked), "越权写破线归因");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves395_smoke() {
        let set = run_ves395_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves395 red: {:?}", red);
    }

    #[test]
    fn ves395_hang_break() {
        let mut fb = FaceBook::new();
        for s in FaceSource::ALL.iter() { fb.add(FuzzFace { source: *s, name: "x", corpus: 1 }); }
        fb.hangs = 3;
        assert_eq!(fb.attribute(), Some("hang"));
        assert!(fb.gate().is_err());
    }
}
