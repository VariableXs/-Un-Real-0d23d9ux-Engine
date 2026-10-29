//! UNX-F12104 RtlInitAnsiString/OEM_STRING 语义（0x19f20 段）
//!
//! 卡面：RtlInitAnsiString / RtlInitOemString——单字节串初始化：
//! 字节计数定长、容量为长 + 1（NUL 位）、ANSI/OEM 串种类互斥分账。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 字节定长：len = n、max = len + 1。
//! - J2 串种闭集：ANSI/OEM 双种，越种必拒。
//! - J3 种别互斥：按 ANSI 登记的串不得按 OEM 查（错种查询必拒）。
//! - J4 越上限拒：len + 1 > 0xFFFE 必拒。
//! - J5 登记唯一：实例编号非零唯一。
//! - J6 印账冻结：seal 后初始化必拒。
//! - J7 注入五况全抓。
//!
//! 结构：StrKind 双种枚举 + AnsiRec 初始化档 + AnsiBook 登记册 +
//! 折叠指纹（211 进制卷积）+ 注入核（越种/错种查询/越上限/重号/印后初始化）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19f20 独占 0x20）。
pub const SEG_BASE: u32 = 0x19f20;
pub const AN_BAD_KIND: u32 = SEG_BASE + 0x01; // 越种
pub const AN_KIND_MIX: u32 = SEG_BASE + 0x02; // 错种查询
pub const AN_MAX_OVER: u32 = SEG_BASE + 0x03; // 越上限
pub const AN_DUP_ID: u32 = SEG_BASE + 0x04; // 重号（含零号）
pub const AN_FROZEN: u32 = SEG_BASE + 0x05; // 印账后初始化

/// 容量上限。
pub const AN_CAP: u32 = 0xFFFE;

/// 串种枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrKind {
    Ansi = 1,
    Oem = 2,
}

impl StrKind {
    pub fn from_code(code: u32) -> Result<StrKind, u32> {
        match code {
            1 => Ok(StrKind::Ansi),
            2 => Ok(StrKind::Oem),
            _ => Err(AN_BAD_KIND),
        }
    }
}

/// ANSI 档。
#[derive(Clone)]
pub struct AnsiRec {
    pub id: u32,
    pub kind: StrKind,
    pub nbytes: u32,
    pub len: u32,
    pub max: u32,
    pub tag: String,
}

impl AnsiRec {
    pub fn render(&self) -> String {
        format!("A{}|k{}|{}+{}", self.id, self.kind as u16, self.len, self.max)
    }
}

/// 登记册。
pub struct AnsiBook {
    pub recs: Vec<AnsiRec>,
    pub sealed: bool,
}

impl AnsiBook {
    pub fn new() -> AnsiBook {
        AnsiBook { recs: Vec::new(), sealed: false }
    }

    /// 初始化登记：计数 → 越上限 → 编号唯一。
    pub fn init(&mut self, id: u32, kind: StrKind, nbytes: u32, tag: &str) -> Result<AnsiRec, u32> {
        if self.sealed {
            return Err(AN_FROZEN);
        }
        let len = nbytes;
        let max = len + 1;
        if max > AN_CAP {
            return Err(AN_MAX_OVER);
        }
        if id == 0 || self.recs.iter().any(|r| r.id == id) {
            return Err(AN_DUP_ID);
        }
        let rec = AnsiRec { id, kind, nbytes, len, max, tag: tag.to_string() };
        self.recs.push(rec.clone());
        Ok(rec)
    }

    /// 按种查询：错种必拒。
    pub fn by_kind(&self, id: u32, kind: StrKind) -> Result<&AnsiRec, u32> {
        match self.recs.iter().find(|r| r.id == id) {
            None => Err(AN_KIND_MIX), // 无档视作错种面
            Some(r) => {
                if r.kind == kind {
                    Ok(r)
                } else {
                    Err(AN_KIND_MIX)
                }
            }
        }
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.kind as u32);
            h = h.wrapping_mul(211).wrapping_add(r.nbytes);
            for by in r.tag.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("recs={}", self.recs.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.recs.iter().find(|r| r.id == id) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AnsiBook::new();
    // 1) 越种必拒。
    got[0] = StrKind::from_code(3).err() == Some(AN_BAD_KIND);
    // 2) 错种查询必拒（ANSI 档按 OEM 查）。
    let _ = b.init(1, StrKind::Ansi, 4, "ansi");
    got[1] = b.by_kind(1, StrKind::Oem).is_err();
    // 3) 越上限必拒。
    got[2] = b.init(2, StrKind::Ansi, 0xFFFE, "big").err() == Some(AN_MAX_OVER);
    // 4) 重号必拒。
    got[3] = b.init(1, StrKind::Oem, 2, "dup").err() == Some(AN_DUP_ID);
    // 5) 印账后初始化必抓。
    b.seal();
    got[4] = b.init(3, StrKind::Oem, 2, "post").err() == Some(AN_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12104_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12104");

    // J1 字节定长。
    let j1 = {
        let mut b = AnsiBook::new();
        let r = b.init(1, StrKind::Ansi, 5, "five");
        match r {
            Ok(rec) => rec.len == 5 && rec.max == 6,
            Err(_) => false,
        }
    };
    s.add("J1-字节定长", j1, "len=n、max=len+1");

    // J2 串种闭集。
    let j2 = StrKind::from_code(0).err() == Some(AN_BAD_KIND)
        && StrKind::from_code(2).is_ok();
    s.add("J2-串种闭集", j2, "0 拒 2 过");

    // J3 种别互斥：双种各查各的。
    let j3 = {
        let mut b = AnsiBook::new();
        let _ = b.init(1, StrKind::Ansi, 4, "a");
        let _ = b.init(2, StrKind::Oem, 6, "o");
        b.by_kind(1, StrKind::Ansi).is_ok()
            && b.by_kind(2, StrKind::Oem).is_ok()
            && b.by_kind(1, StrKind::Oem).is_err()
            && b.by_kind(2, StrKind::Ansi).is_err()
    };
    s.add("J3-种别互斥", j3, "同种过、异种拒");

    // J4 无档查询拒。
    let j4 = {
        let mut b = AnsiBook::new();
        b.by_kind(9, StrKind::Ansi).is_err()
    };
    s.add("J4-无档拒", j4, "未知号必拒");

    // J5 越上限拒与边界放行。
    let j5 = {
        let mut b = AnsiBook::new();
        b.init(1, StrKind::Ansi, 0xFFFE, "over").err() == Some(AN_MAX_OVER)
            && b.init(2, StrKind::Ansi, 0xFFFD, "edge").is_ok()
    };
    s.add("J5-越上限", j5, "边界过、越界拒");

    // J6 编号唯一。
    let j6 = {
        let mut b = AnsiBook::new();
        let _ = b.init(3, StrKind::Oem, 2, "a");
        b.init(3, StrKind::Oem, 2, "b").err() == Some(AN_DUP_ID)
            && b.init(0, StrKind::Oem, 2, "z").err() == Some(AN_DUP_ID)
    };
    s.add("J6-编号唯一", j6, "零号重号皆拒");

    // J7 回读渲染。
    let j7 = {
        let mut b = AnsiBook::new();
        let _ = b.init(7, StrKind::Oem, 9, "oem9");
        b.line_of(7).contains("k2") && b.line_of(8) == "missing"
    };
    s.add("J7-回读渲染", j7, "档行含种别码");

    // J8 印账冻结。
    let j8 = {
        let mut b = AnsiBook::new();
        b.seal();
        b.init(1, StrKind::Ansi, 2, "p").err() == Some(AN_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = AnsiBook::new();
        let _ = b.init(1, StrKind::Ansi, 2, "a");
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = AnsiBook::new();
    let _ = b.init(1, StrKind::Ansi, 12, "cmdline");
    let _ = b.init(2, StrKind::Oem, 8, "oempath");

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12104_kind_flow() {
        let mut b = AnsiBook::new();
        assert!(b.init(1, StrKind::Ansi, 3, "a").is_ok());
        assert!(b.by_kind(1, StrKind::Ansi).is_ok());
        assert!(b.by_kind(1, StrKind::Oem).is_err());
    }

    #[test]
    fn unx12104_checks_green() {
        let s = run_unx12104_checks();
        assert!(s.all_passed(), "UNX-F12104 判据未全绿");
    }

    #[test]
    fn kind_enum_codes() {
        assert_eq!(StrKind::Ansi as u16, 1);
        assert_eq!(StrKind::Oem as u16, 2);
    }
}
