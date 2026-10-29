//! UNX-F12037 状态码文本化与诊断输出面（0x196C0 段）
//!
//! 判据原文：状态码文本化——已知码查名表得短名（前缀按严重级 S/I/W/E）；
//! 未知码回退十六进制面（E-XXXX-YYYY 格式：严重级+设施+码值）；名表
//! 登记唯一（重名登记拒）；渲染逐笔可对账。
//!
//! 结构：名表（NameBook：码 → 短名）+ 渲染核（四码守卫）+ 渲染账
//! （NrRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12037_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x196C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x196C0;
pub const NR_DUP_NAME: u32 = SEG_BASE + 0x01; // 重名登记
pub const NR_BAD_REC: u32 = SEG_BASE + 0x02; // 伪造渲染账
pub const NR_EMPTY_NAME: u32 = SEG_BASE + 0x03; // 空短名
pub const NR_DUP_CODE: u32 = SEG_BASE + 0x04; // 码重复登记
pub const NR_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 严重级前缀。
pub fn severity_prefix(v: u32) -> char {
    match v >> 30 {
        0 => 'S',
        1 => 'I',
        2 => 'W',
        _ => 'E',
    }
}

/// 一笔渲染的账目。
#[derive(Clone)]
pub struct NrRec {
    pub seq: u32,
    pub code: u32,
    pub text: String,
    pub note: String,
}

impl NrRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && !self.text.is_empty()
    }
}

/// 名表模型。
pub struct NameBook {
    pub names: Vec<(u32, String)>,
    pub ledger: Vec<NrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl NameBook {
    pub fn new() -> NameBook {
        NameBook { names: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记核：码唯一 + 短名非空。
    pub fn enlist(&mut self, code: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(NR_FROZEN);
        }
        if name.is_empty() {
            return Err(NR_EMPTY_NAME);
        }
        if self.names.iter().any(|n| n.0 == code) {
            return Err(NR_DUP_CODE);
        }
        self.names.push((code, name.to_string()));
        Ok(())
    }

    /// 渲染核：命中查名表（前缀+短名）、未命中回退十六进制面。
    pub fn render(&mut self, code: u32, note: &str) -> Result<String, u32> {
        if self.sealed {
            return Err(NR_FROZEN);
        }
        if note.is_empty() {
            return Err(NR_BAD_REC);
        }
        let text = match self.names.iter().find(|n| n.0 == code) {
            Some((_, name)) => format!("{}-{}", severity_prefix(code), name),
            None => {
                let sev = severity_prefix(code);
                let fac = (code >> 16) & 0xFFF;
                let val = code & 0xFFFF;
                format!("{}-{:03X}-{:04X}", sev, fac, val)
            }
        };
        let r = NrRec { seq: self.seq_next, code, text: text.clone(), note: note.to_string() };
        if !r.ok() {
            return Err(NR_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(r);
        Ok(text)
    }

    /// 名表数。
    pub fn count(&self) -> usize {
        self.names.len()
    }

    /// 渲染账清点。
    pub fn rec_count(&self) -> usize {
        self.ledger.len()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for n in &self.names {
            h = h.wrapping_mul(211).wrapping_add(n.0);
            for by in n.1.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = NameBook::new();
    let _ = b.enlist(0xC000_0005, "ACCESS_VIOLATION");
    // 1) 码重复登记 → NR_DUP_CODE。
    got[0] = b.enlist(0xC000_0005, "DUP").err() == Some(NR_DUP_CODE);
    // 2) 空短名 → NR_EMPTY_NAME。
    got[1] = b.enlist(0xC000_0008, "").err() == Some(NR_EMPTY_NAME);
    // 3) 空留痕渲染 → NR_BAD_REC。
    got[2] = b.render(0xC000_0005, "").err() == Some(NR_BAD_REC);
    // 4) 未命中回退面：渲染文本含十六进制段（语义核对）。
    match b.render(0xC123_4567, "fallback") {
        Ok(t) => got[3] = t.starts_with("E-123-4567"),
        Err(_) => got[3] = false,
    }
    // 5) 印账后渲染 → NR_FROZEN。
    b.seal();
    got[4] = b.render(0xC000_0005, "post").err() == Some(NR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12037_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12037");

    // J1 已知码渲染：前缀+短名。
    let j1 = {
        let mut b = NameBook::new();
        let _ = b.enlist(0xC000_0005, "ACCESS_VIOLATION");
        b.render(0xC000_0005, "a") == Ok("E-ACCESS_VIOLATION".to_string())
    };
    s.add("J1-命中渲染", j1, "E-ACCESS_VIOLATION");

    // J2 严重级前缀四值。
    let j2 = severity_prefix(0x0000_0000) == 'S'
        && severity_prefix(0x4000_0000) == 'I'
        && severity_prefix(0x8000_0000) == 'W'
        && severity_prefix(0xC000_0005) == 'E';
    s.add("J2-前缀四值", j2, "S/I/W/E");

    // J3 未命中回退面：E-fac-code 格式。
    let j3 = {
        let mut b = NameBook::new();
        match b.render(0xC000_1234, "u") {
            Ok(t) => t == "E-000-1234",
            Err(_) => false,
        }
    };
    s.add("J3-回退渲染", j3, "E-000-1234");

    // J4 回退面含设施段。
    let j4 = {
        let mut b = NameBook::new();
        match b.render(0xC001_2345, "f") {
            Ok(t) => t.contains("001") && t.contains("2345"),
            Err(_) => false,
        }
    };
    s.add("J4-设施段", j4, "001/2345 分段");

    // J5 码重复登记拒。
    let j5 = {
        let mut b = NameBook::new();
        let _ = b.enlist(0xC000_0005, "AV");
        b.enlist(0xC000_0005, "AV2").err() == Some(NR_DUP_CODE)
    };
    s.add("J5-码重复拒", j5, "一码一名");

    // J6 空短名拒。
    let j6 = {
        let mut b = NameBook::new();
        b.enlist(0xC000_0008, "") == Err(NR_EMPTY_NAME)
    };
    s.add("J6-空名拒", j6, "短名必填");

    // J7 渲染账对账：账数与渲染次数一致。
    let j7 = {
        let mut b = NameBook::new();
        let _ = b.enlist(0xC000_0005, "AV");
        let _ = b.render(0xC000_0005, "a");
        let _ = b.render(0xC000_0008, "b");
        b.rec_count() == 2 && b.ledger_ok()
    };
    s.add("J7-渲染账", j7, "两笔对账");

    // J8 渲染账留痕：码与文本同记。
    let j8 = {
        let mut b = NameBook::new();
        let _ = b.enlist(0x4000_0015, "REPARSE");
        let _ = b.render(0x4000_0015, "a");
        b.ledger[0].code == 0x4000_0015 && b.ledger[0].text == "I-REPARSE"
    };
    s.add("J8-留痕对码", j8, "I-REPARSE");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = NameBook::new();
        let _ = a.enlist(0xC000_0005, "AV");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = NameBook::new();
        let _ = c.enlist(0xC000_0005, "XX");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异表异纹");

    // J10 印账冻结。
    let j10 = {
        let mut b = NameBook::new();
        let _ = b.enlist(0xC000_0005, "AV");
        b.seal();
        b.render(0xC000_0005, "post").err() == Some(NR_FROZEN)
            && b.enlist(0xC000_0008, "IH").err() == Some(NR_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：四码名表渲染面。
    let mut b = NameBook::new();
    let _ = b.enlist(0xC000_0005, "ACCESS_VIOLATION");
    let _ = b.enlist(0xC000_0008, "INVALID_HANDLE");
    let _ = b.enlist(0x0000_0000, "SUCCESS");
    let r1 = b.render(0xC000_0005, "s1");
    let r2 = b.render(0xC000_7777, "s2");
    s.add("样例一新一回", r1 == Ok("E-ACCESS_VIOLATION".to_string()) && r2.is_ok(), "命中+回退");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12037_render_flow() {
        let mut b = NameBook::new();
        assert!(b.enlist(0xC000_0005, "ACCESS_VIOLATION").is_ok());
        assert_eq!(b.render(0xC000_0005, "a"), Ok("E-ACCESS_VIOLATION".to_string()));
        assert_eq!(b.render(0xC000_1234, "b"), Ok("E-000-1234".to_string()));
        assert_eq!(b.enlist(0xC000_0005, "X"), Err(NR_DUP_CODE));
    }

    #[test]
    fn unx12037_checks_green() {
        let s = run_unx12037_checks();
        assert!(s.all_passed(), "UNX-F12037 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12037_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
