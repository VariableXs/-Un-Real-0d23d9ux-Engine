//! cgb841_cert_report — CGPU-F0841 Intel 认证报告。
//!
//! 判据原文（验收行摘录）：Intel 认证报告：报告章结构（结论/指标/证据/
//! 附录四定容章）；证据编号账（证据条目编号连续不可跳——跳号即账错）；
//! 结论闭集（通过/有条件通过/拒绝 三值，附条件项须挂证据）；守恒账（引
//! 用证据数 ≤ 证据总数，章节数 = 4）。判据：四章结构、连续编号、三值
//! 结论、守恒。
//!
//! 诊断段 0x73A0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容章与证据账；drill() 自例程返回 0 表
//! 示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb841-v1";

/// 诊断段基（本单独占 0x73A0）。
pub const SEG_BASE: u16 = 0x73A0;

/// 证据账满（定容耗尽）。
pub const XC_EVID_FULL: u16 = SEG_BASE + 1;
/// 编号跳号（新证据编号 ≠ 上一个 + 1）。
pub const XC_EVID_GAP: u16 = SEG_BASE + 2;
/// 证据不存在（引用未登记证据）。
pub const XC_EVID_MISS: u16 = SEG_BASE + 3;
/// 章未齐（四章未全登记即出结论）。
pub const XC_CHAPTER_SHORT: u16 = SEG_BASE + 4;
/// 章超容（章数 > 4）。
pub const XC_CHAPTER_OVER: u16 = SEG_BASE + 5;
/// 条件未挂证（有条件通过但条件项无证据）。
pub const XC_COND_NO_EVID: u16 = SEG_BASE + 6;
/// 守恒破坏（引用证据数 > 证据总数或章数 ≠ 4）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 7;
/// 重复出结（已出结再改）。
pub const XC_ALREADY_CONCLUDED: u16 = SEG_BASE + 8;

/// 四章闭集（判据"四定容章"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chapter {
    /// 结论章。
    Verdict,
    /// 指标章。
    Metrics,
    /// 证据章。
    Evidence,
    /// 附录章。
    Appendix,
}

impl Chapter {
    /// 章序号。
    pub fn idx(self) -> usize {
        match self {
            Chapter::Verdict => 0,
            Chapter::Metrics => 1,
            Chapter::Evidence => 2,
            Chapter::Appendix => 3,
        }
    }
    /// 章名。
    pub fn name(self) -> &'static str {
        match self {
            Chapter::Verdict => "verdict",
            Chapter::Metrics => "metrics",
            Chapter::Evidence => "evidence",
            Chapter::Appendix => "appendix",
        }
    }
}

/// 结论三值闭集（判据"三值结论"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 通过。
    Pass,
    /// 有条件通过。
    Cond,
    /// 拒绝。
    Reject,
}

impl Verdict {
    /// 值名。
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Cond => "conditional",
            Verdict::Reject => "reject",
        }
    }
}

/// 证据条目。
#[derive(Clone, Debug)]
pub struct Evidence {
    /// 连续编号（1 起）。
    pub no: u64,
    /// 证据摘要。
    pub digest: String,
    /// 挂载章。
    pub chapter: Chapter,
}

/// 认证报告账。
pub struct CertReport {
    /// 报告名。
    pub name: String,
    /// 已登记章位图。
    chapters: [bool; 4],
    /// 证据账（定容）。
    pub evids: Vec<Evidence>,
    /// 上一个证据编号（连续性基点）。
    last_no: u64,
    /// 结论。
    verdict: Option<Verdict>,
    /// 条件项挂的证据编号（有条件通过时非空）。
    pub cond_evids: Vec<u64>,
    /// 结论引用的证据编号集。
    pub cited: Vec<u64>,
}

/// 守恒恒等式：cited 与 cond_evids 中的编号均须在 evids 内（引用 ≤ 总数）；
/// 章数恒 ≤ 4 且出结时恰 = 4；证据编号连续 1..=n。
impl CertReport {
    /// 构造。
    pub fn new(name: &str) -> Self {
        CertReport {
            name: name.to_string(),
            chapters: [false; 4],
            evids: Vec::new(),
            last_no: 0,
            verdict: None,
            cond_evids: Vec::new(),
            cited: Vec::new(),
        }
    }

    /// 开章（判据"四章结构"；重复开章幂等接受）。
    pub fn open_chapter(&mut self, c: Chapter) -> Result<(), u16> {
        if self.verdict.is_some() {
            return Err(XC_ALREADY_CONCLUDED);
        }
        self.chapters[c.idx()] = true;
        Ok(())
    }

    /// 章数。
    pub fn chapter_count(&self) -> u32 {
        let mut n = 0;
        for b in self.chapters.iter() {
            if *b {
                n += 1;
            }
        }
        n
    }

    /// 挂证据（判据"连续编号"）。
    pub fn add_evidence(&mut self, digest: &str, chapter: Chapter) -> Result<u64, u16> {
        if self.verdict.is_some() {
            return Err(XC_ALREADY_CONCLUDED);
        }
        if self.evids.len() >= 16 {
            return Err(XC_EVID_FULL);
        }
        // 编号必须严格连续（跳号即账错）。
        let no = self.last_no + 1;
        if self.last_no > 0 && no != self.last_no + 1 {
            return Err(XC_EVID_GAP);
        }
        self.evids.push(Evidence { no, digest: digest.to_string(), chapter });
        self.last_no = no;
        self.verify()?;
        Ok(no)
    }

    /// 引用证据（结论章引用编号）。
    pub fn cite(&mut self, no: u64) -> Result<(), u16> {
        if self.verdict.is_some() {
            return Err(XC_ALREADY_CONCLUDED);
        }
        let mut found = false;
        for e in self.evids.iter() {
            if e.no == no {
                found = true;
                break;
            }
        }
        if !found {
            return Err(XC_EVID_MISS);
        }
        for c in self.cited.iter() {
            if *c == no {
                return Err(XC_EVID_MISS);
            }
        }
        self.cited.push(no);
        self.verify()?;
        Ok(())
    }

    /// 出结（判据"三值结论"；有条件通过须挂证据——判据"条件项须挂证据"）。
    pub fn conclude(&mut self, v: Verdict, cond_evids: &[u64]) -> Result<Verdict, u16> {
        if self.verdict.is_some() {
            return Err(XC_ALREADY_CONCLUDED);
        }
        if self.chapter_count() != 4 {
            return Err(XC_CHAPTER_SHORT);
        }
        if self.evids.is_empty() {
            return Err(XC_CHAPTER_SHORT);
        }
        match v {
            Verdict::Cond => {
                if cond_evids.is_empty() {
                    return Err(XC_COND_NO_EVID);
                }
                for no in cond_evids {
                    let mut found = false;
                    for e in self.evids.iter() {
                        if e.no == *no {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(XC_EVID_MISS);
                    }
                    self.cond_evids.push(*no);
                }
            }
            _ => {}
        }
        self.verdict = Some(v);
        self.verify()?;
        Ok(v)
    }

    /// 结论只读。
    pub fn verdict(&self) -> Option<Verdict> {
        self.verdict
    }

    /// 证据数。
    pub fn evid_count(&self) -> usize {
        self.evids.len()
    }

    /// 守恒校验（判据"守恒账"）。
    pub fn verify(&self) -> Result<(), u16> {
        if self.chapter_count() > 4 {
            return Err(XC_LEDGER_DRIFT);
        }
        // 引用 ≤ 总数且全部在册。
        if self.cited.len() > self.evids.len() {
            return Err(XC_LEDGER_DRIFT);
        }
        for no in self.cited.iter() {
            let mut found = false;
            for e in self.evids.iter() {
                if e.no == *no {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(XC_LEDGER_DRIFT);
            }
        }
        for no in self.cond_evids.iter() {
            let mut found = false;
            for e in self.evids.iter() {
                if e.no == *no {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(XC_LEDGER_DRIFT);
            }
        }
        // 编号连续 1..=n。
        for (i, e) in self.evids.iter().enumerate() {
            if e.no != i as u64 + 1 {
                return Err(XC_LEDGER_DRIFT);
            }
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb841[");
        s.push_str(&self.name);
        s.push_str("] chapters=");
        s.push_str(&self.chapter_count().to_string());
        s.push_str(" evid=");
        s.push_str(&self.evid_count().to_string());
        s.push_str(" cited=");
        s.push_str(&self.cited.len().to_string());
        s.push_str(" verdict=");
        s.push_str(match self.verdict {
            None => "none",
            Some(v) => v.name(),
        });
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut r = CertReport::new("w3-drill-841");

    // 负样本 1：证据未挂即出结（章未齐 + 无证据）。
    match r.conclude(Verdict::Pass, &[]) {
        Err(code) if code == XC_CHAPTER_SHORT => {}
        Ok(_) => return Err(XC_CHAPTER_SHORT),
        Err(other) => return Err(other),
    }
    // 开四章。
    r.open_chapter(Chapter::Verdict)?;
    r.open_chapter(Chapter::Metrics)?;
    r.open_chapter(Chapter::Evidence)?;
    if r.chapter_count() != 3 {
        return Err(XC_CHAPTER_OVER);
    }
    // 负样本 2：仍缺附录章。
    match r.conclude(Verdict::Pass, &[]) {
        Err(code) if code == XC_CHAPTER_SHORT => {}
        Ok(_) => return Err(XC_CHAPTER_SHORT),
        Err(other) => return Err(other),
    }
    r.open_chapter(Chapter::Appendix)?;
    if r.chapter_count() != 4 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 负样本 3：零证据出结。
    match r.conclude(Verdict::Pass, &[]) {
        Err(code) if code == XC_CHAPTER_SHORT => {}
        Ok(_) => return Err(XC_CHAPTER_SHORT),
        Err(other) => return Err(other),
    }

    // 挂证据：连续编号 1..5。
    let e1 = r.add_evidence("硬门三项过", Chapter::Metrics)?;
    let e2 = r.add_evidence("压力 12 帧满频", Chapter::Metrics)?;
    let e3 = r.add_evidence("媒体全链路回放", Chapter::Evidence)?;
    let e4 = r.add_evidence("TDR 恢复 800‰", Chapter::Evidence)?;
    let e5 = r.add_evidence("遥测 64 事件封账", Chapter::Appendix)?;
    if e5 != 5 {
        return Err(XC_EVID_GAP);
    }
    r.verify()?;

    // 负样本 4：引用未登记证据。
    match r.cite(99) {
        Err(code) if code == XC_EVID_MISS => {}
        Ok(_) => return Err(XC_EVID_MISS),
        Err(other) => return Err(other),
    }
    // 引用 3 条。
    r.cite(e1)?;
    r.cite(e2)?;
    r.cite(e3)?;
    // 负样本 5：重复引用。
    match r.cite(e1) {
        Err(code) if code == XC_EVID_MISS => {}
        Ok(_) => return Err(XC_EVID_MISS),
        Err(other) => return Err(other),
    }

    // 负样本 6：有条件通过但不挂条件证据。
    match r.conclude(Verdict::Cond, &[]) {
        Err(code) if code == XC_COND_NO_EVID => {}
        Ok(_) => return Err(XC_COND_NO_EVID),
        Err(other) => return Err(other),
    }
    // 负样本 7：条件证据引用未登记号。
    match r.conclude(Verdict::Cond, &[77]) {
        Err(code) if code == XC_EVID_MISS => {}
        Ok(_) => return Err(XC_EVID_MISS),
        Err(other) => return Err(other),
    }
    // 正样本：有条件通过（条件挂 e4）。
    let v = r.conclude(Verdict::Cond, &[e4])?;
    if v != Verdict::Cond {
        return Err(XC_LEDGER_DRIFT);
    }
    r.verify()?;
    // 负样本 8：出结后再改。
    match r.add_evidence("迟到的证据", Chapter::Appendix) {
        Err(code) if code == XC_ALREADY_CONCLUDED => {}
        Ok(_) => return Err(XC_ALREADY_CONCLUDED),
        Err(other) => return Err(other),
    }
    match r.cite(e5) {
        Err(code) if code == XC_ALREADY_CONCLUDED => {}
        Ok(_) => return Err(XC_ALREADY_CONCLUDED),
        Err(other) => return Err(other),
    }
    match r.conclude(Verdict::Pass, &[]) {
        Err(code) if code == XC_ALREADY_CONCLUDED => {}
        Ok(_) => return Err(XC_ALREADY_CONCLUDED),
        Err(other) => return Err(other),
    }
    if r.verdict() != Some(Verdict::Cond) {
        return Err(XC_LEDGER_DRIFT);
    }
    if r.evid_count() != 5 || r.cited.len() != 3 || r.cond_evids.len() != 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
