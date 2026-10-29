//! UNX-F12103 RtlInitUnicodeString 语义（0x19f00 段）
//!
//! 卡面：RtlInitUnicodeString——以 NUL 终止串初始化：字符计数 ×2 定长、
//! 容量为长 + 2、内部 NUL（未真正终止）显性拒、容量越上限拒。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 计数定长：len = n×2、max = len + 2。
//! - J2 内部 NUL 拒：UTF-16 单元序列中途含 0 必拒（未真正终止）。
//! - J3 空串放行：单 NUL 串初始化为 len0/max2。
//! - J4 越上限拒：n×2 + 2 > 0xFFFE 必拒。
//! - J5 登记唯一：实例编号非零唯一。
//! - J6 印账冻结：seal 后初始化必拒。
//! - J7 注入五况全抓。
//!
//! 结构：InitRec 初始化档 + RtlInitBook 登记册 + 计数核 + 折叠指纹
//! （211 进制卷积）+ 注入核（内部 NUL/越上限/重号/零号/印后初始化）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19f00 独占 0x20）。
pub const SEG_BASE: u32 = 0x19f00;
pub const RI_NO_TERM: u32 = SEG_BASE + 0x01; // 内部 NUL
pub const RI_MAX_OVER: u32 = SEG_BASE + 0x02; // 越上限
pub const RI_DUP_ID: u32 = SEG_BASE + 0x03; // 重号（含零号）
pub const RI_FROZEN: u32 = SEG_BASE + 0x04; // 印账后初始化

/// 容量上限。
pub const RI_CAP: u32 = 0xFFFE;

/// 初始化档。
#[derive(Clone, Debug, PartialEq)]
pub struct InitRec {
    pub id: u32,
    pub nchars: u32,
    pub len: u32,
    pub max: u32,
    pub tag: String,
}

impl InitRec {
    pub fn render(&self) -> String {
        format!("I{}|n{}|{}+{}", self.id, self.nchars, self.len, self.max)
    }
}

/// 计数核：UTF-16 单元序列计数。模型：序列必须以 NUL 终止，NUL 仅可为
/// 末位（终止符，不计入字符数）；中途 NUL 即未真正终止 → RI_NO_TERM；
/// 空序列无终止符 → RI_NO_TERM。
pub fn count_units(units: &[u16]) -> Result<u32, u32> {
    let (last, head) = match units.split_last() {
        Some((l, h)) => (l, h),
        None => return Err(RI_NO_TERM), // 无终止符
    };
    if *last != 0 {
        return Err(RI_NO_TERM); // 未以 NUL 终止
    }
    if head.iter().any(|&u| u == 0) {
        return Err(RI_NO_TERM); // 内部 NUL（未真正终止）
    }
    Ok(head.len() as u32)
}

/// 登记册。
pub struct RtlInitBook {
    pub recs: Vec<InitRec>,
    pub sealed: bool,
}

impl RtlInitBook {
    pub fn new() -> RtlInitBook {
        RtlInitBook { recs: Vec::new(), sealed: false }
    }

    /// 初始化登记：计数 → 越上限 → 编号唯一。
    pub fn init(&mut self, id: u32, units: &[u16], tag: &str) -> Result<InitRec, u32> {
        if self.sealed {
            return Err(RI_FROZEN);
        }
        let n = count_units(units)?;
        let len = n * 2;
        let max = len + 2;
        if max > RI_CAP {
            return Err(RI_MAX_OVER);
        }
        if id == 0 || self.recs.iter().any(|r| r.id == id) {
            return Err(RI_DUP_ID);
        }
        let rec = InitRec { id, nchars: n, len, max, tag: tag.to_string() };
        self.recs.push(rec.clone());
        Ok(rec)
    }

    pub fn by_id(&self, id: u32) -> Option<&InitRec> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.nchars);
            h = h.wrapping_mul(211).wrapping_add(r.len);
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
        match self.by_id(id) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = RtlInitBook::new();
    // 1) 内部 NUL 必拒。
    got[0] = b.init(1, &[0x41, 0, 0x42, 0], "mid")
        .err() == Some(RI_NO_TERM);
    // 2) 越上限必拒。
    got[1] = {
        let mut big = vec![0x41u16; 0x8000]; // n=0x8000 → max 0x10002
        big.push(0);
        b.init(1, &big, "big").err() == Some(RI_MAX_OVER)
    };
    // 3) 重号必拒。
    let _ = b.init(2, &[0x41, 0], "a");
    got[2] = b.init(2, &[0x42, 0], "b").err() == Some(RI_DUP_ID);
    // 4) 零号必拒。
    got[3] = b.init(0, &[0x41, 0], "z").err() == Some(RI_DUP_ID);
    // 5) 印账后初始化必抓。
    b.seal();
    got[4] = b.init(3, &[0x41, 0], "post").err() == Some(RI_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12103_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12103");

    // J1 计数定长。
    let j1 = {
        let mut b = RtlInitBook::new();
        let r = b.init(1, &[0x41, 0x42, 0x43, 0], "abc");
        match r {
            Ok(rec) => rec.nchars == 3 && rec.len == 6 && rec.max == 8,
            Err(_) => false,
        }
    };
    s.add("J1-计数定长", j1, "len=n×2、max=len+2");

    // J2 内部 NUL 拒。
    let j2 = {
        let mut b = RtlInitBook::new();
        b.init(1, &[0x41, 0, 0x42, 0], "mid").err() == Some(RI_NO_TERM)
    };
    s.add("J2-内部NUL拒", j2, "中途零必拒");

    // J3 空串放行。
    let j3 = {
        let mut b = RtlInitBook::new();
        let r = b.init(1, &[0], "empty");
        match r {
            Ok(rec) => rec.nchars == 0 && rec.len == 0 && rec.max == 2,
            Err(_) => false,
        }
    };
    s.add("J3-空串放行", j3, "单 NUL 串合法");

    // J4 无终止符拒。
    let j4 = count_units(&[0x41, 0x42]).err() == Some(RI_NO_TERM);
    s.add("J4-无终止拒", j4, "无 NUL 序列必拒");

    // J5 越上限拒。
    let j5 = {
        let mut b = RtlInitBook::new();
        let mut big = vec![0x41u16; 0x8000];
        big.push(0);
        b.init(1, &big, "big").err() == Some(RI_MAX_OVER)
    };
    s.add("J5-越上限拒", j5, "容量越 0xFFFE 必拒");

    // J6 上限边界放行。
    let j6 = {
        let mut b = RtlInitBook::new();
        let mut edge = vec![0x41u16; 0x7FFD]; // n=0x7FFD → max = 0xFFFE 恰达
        edge.push(0);
        b.init(1, &edge, "edge").is_ok()
    };
    s.add("J6-边界放行", j6, "恰达上限不拒");

    // J7 编号唯一。
    let j7 = {
        let mut b = RtlInitBook::new();
        let _ = b.init(3, &[0x41, 0], "a");
        b.init(3, &[0x42, 0], "b").err() == Some(RI_DUP_ID)
    };
    s.add("J7-编号唯一", j7, "重号必拒");

    // J8 回读渲染。
    let j8 = {
        let mut b = RtlInitBook::new();
        let _ = b.init(5, &[0x41, 0x42, 0], "ab");
        b.line_of(5).contains("n2") && b.line_of(6) == "missing"
    };
    s.add("J8-回读渲染", j8, "档行可渲染");

    // J9 印账冻结。
    let j9 = {
        let mut b = RtlInitBook::new();
        b.seal();
        b.init(1, &[0x41, 0], "p").err() == Some(RI_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = RtlInitBook::new();
        let _ = b.init(1, &[0x41, 0], "a");
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = RtlInitBook::new();
    let _ = b.init(1, &[0x5C, 0x44, 0x65, 0x76, 0x69, 0x63, 0x65, 0x73, 0], "devices");
    let _ = b.init(2, &[0], "empty");

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("计数回读", b.by_id(1).map(|r| r.nchars) == Some(8), "8 字符计数");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12103_count_flow() {
        assert_eq!(count_units(&[0x41, 0x42, 0x43, 0]), Ok(3));
        assert_eq!(count_units(&[0]), Ok(0));
        assert_eq!(count_units(&[0x41, 0]), Ok(1));
        assert_eq!(count_units(&[0x41, 0, 0x42, 0]), Err(RI_NO_TERM));
        assert_eq!(count_units(&[]), Err(RI_NO_TERM));
    }

    #[test]
    fn unx12103_checks_green() {
        let s = run_unx12103_checks();
        assert!(s.all_passed(), "UNX-F12103 判据未全绿");
    }

    #[test]
    fn book_flow() {
        let mut b = RtlInitBook::new();
        assert!(b.init(1, &[0x41, 0], "a").is_ok());
        assert_eq!(b.init(1, &[0x42, 0], "b"), Err(RI_DUP_ID));
        assert!(b.by_id(1).is_some());
    }
}
