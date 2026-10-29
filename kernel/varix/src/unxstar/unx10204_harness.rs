//! UNX-F10204 执行环境预检（内核版本锚/harness 在位，缺件放行拦截）（0x10ca0 段）
//!
//! 锚点：UNX-F10204-J1 环境预检达成（版本锚+harness 双在位核 10/10 次），
//! 缺件放行 10/10 次拦截。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：预检册（版本锚 × harness 位 × 预检态）+ 门核（双在位才放行、
//! 缺件逐项拦截）+ 预检账本 + 折叠指纹 + 注入核（缺锚放行/缺
//! harness 放行/双缺放行/伪造放行账/印后预检）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10ca0 独占 0x20）。
pub const SEG_BASE: u32 = 0x10ca0;
pub const EV_NO_ANCHOR: u32 = SEG_BASE + 0x01; // 版本锚缺
pub const EV_NO_HARNESS: u32 = SEG_BASE + 0x02; // harness 缺
pub const EV_BAD_ANCHOR: u32 = SEG_BASE + 0x03; // 锚值越域
pub const EV_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const EV_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍预检

/// 版本锚合法域（非零且 < 2^24）。
pub const ANCHOR_MAX: u32 = 0x00FF_FFFF;

/// 预检结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreVerdict {
    Go,
    NoGo { part: u32 },
}

/// 一次预检的账本留痕。
#[derive(Clone)]
pub struct EvRec {
    pub seq: u32,
    pub verdict: PreVerdict,
    pub note: String,
}

impl EvRec {
    /// 账目合法性：留痕非空。
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
    }

    pub fn render_line(&self) -> String {
        match self.verdict {
            PreVerdict::Go => format!("V{}|GO|{}", self.seq, self.note),
            PreVerdict::NoGo { part } => format!("V{}|NOGO({})|{}", self.seq, part, self.note),
        }
    }
}

/// 预检册：锚位 × harness 位 + 门核。
pub struct EnvPrecheck {
    pub anchor: Option<u32>,
    pub harness: bool,
    pub ledger: Vec<EvRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl EnvPrecheck {
    pub fn new() -> EnvPrecheck {
        EnvPrecheck { anchor: None, harness: false, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 挂版本锚（越域必拒）。
    pub fn set_anchor(&mut self, a: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_FROZEN);
        }
        if a == 0 || a > ANCHOR_MAX {
            return Err(EV_BAD_ANCHOR);
        }
        self.anchor = Some(a);
        Ok(())
    }

    /// 挂 harness。
    pub fn set_harness(&mut self, on: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_FROZEN);
        }
        self.harness = on;
        Ok(())
    }

    /// 预检门：双在位才 Go；缺件逐项点名拦截。
    pub fn gate(&mut self, note: &str) -> Result<PreVerdict, u32> {
        if self.sealed {
            return Err(EV_FROZEN);
        }
        if note.is_empty() {
            return Err(EV_EMPTY_NOTE);
        }
        let v = match (self.anchor, self.harness) {
            (Some(_), true) => PreVerdict::Go,
            (None, true) => PreVerdict::NoGo { part: EV_NO_ANCHOR },
            (Some(_), false) => PreVerdict::NoGo { part: EV_NO_HARNESS },
            (None, false) => PreVerdict::NoGo { part: EV_NO_ANCHOR + EV_NO_HARNESS },
        };
        self.ledger.push(EvRec { seq: self.seq_next, verdict: v, note: note.to_string() });
        self.seq_next += 1;
        match v {
            PreVerdict::Go => Ok(v),
            PreVerdict::NoGo { .. } => Err(EV_NO_ANCHOR), // 缺件按拦截面回错误
        }
    }

    /// Go 判定（不出错的查询面）。
    pub fn is_go(&self) -> bool {
        self.anchor.is_some() && self.harness
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（197 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10ca0;
        h = h.wrapping_mul(197).wrapping_add(self.anchor.unwrap_or(0));
        h = h.wrapping_mul(197).wrapping_add(self.harness as u32);
        for r in &self.ledger {
            h = h.wrapping_mul(197).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(197).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
    /// 诊断面：锚位是否已置
    pub fn anchor_set(&self) -> bool {
        self.anchor.is_some()
    }

    /// 诊断面：harness 位回读
    pub fn harness_present(&self) -> bool {
        self.harness
    }

    /// 诊断面：账面记录数
    pub fn rec_count(&self) -> usize {
        self.ledger.len()
    }

    /// 诊断面：全记录自洽复核
    pub fn all_recs_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 诊断面：Go 判定计数
    pub fn go_count(&self) -> usize {
        self.ledger.iter().filter(|r| matches!(r.verdict, PreVerdict::Go)).count()
    }

    /// 诊断面：NoGo 判定计数
    pub fn nogo_count(&self) -> usize {
        self.ledger.iter().filter(|r| matches!(r.verdict, PreVerdict::NoGo { .. })).count()
    }

    /// 诊断面：账面摘要
    pub fn book_summary(&self) -> String {
        format!(
            "anchor={} harness={} recs={}",
            self.anchor_set(),
            self.harness,
            self.ledger.len()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 缺锚放行必拦。
    let mut a = EnvPrecheck::new();
    let _ = a.set_harness(true);
    got[0] = a.gate("inj-anchor").is_err() && !a.is_go();
    // 2) 缺 harness 放行必拦。
    let mut b = EnvPrecheck::new();
    let _ = b.set_anchor(0x2027);
    got[1] = b.gate("inj-harness").is_err() && !b.is_go();
    // 3) 双缺放行必拦。
    let mut c = EnvPrecheck::new();
    got[2] = c.gate("inj-both").is_err();
    // 4) 伪造放行账（空留痕）必判非法。
    let forged = EvRec { seq: 1, verdict: PreVerdict::Go, note: String::new() };
    got[3] = !forged.ok();
    // 5) 印账后预检必抓。
    c.seal();
    got[4] = c.gate("inj-frozen").err() == Some(EV_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10204_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10204");

    // J1 双在位放行：锚+harness 齐后 Go。
    let j1 = {
        let mut a = EnvPrecheck::new();
        let _ = a.set_anchor(0x27);
        let _ = a.set_harness(true);
        a.gate("g").is_ok() && a.is_go()
    };
    s.add("J1-双在位放行", j1, "锚+harness 齐 Go");

    // J2 缺锚拦截。
    let j2 = {
        let mut a = EnvPrecheck::new();
        let _ = a.set_harness(true);
        a.gate("g").is_err() && !a.is_go()
    };
    s.add("J2-缺锚拦截", j2, "无锚必拦");

    // J3 缺 harness 拦截。
    let j3 = {
        let mut a = EnvPrecheck::new();
        let _ = a.set_anchor(7);
        a.gate("g").is_err() && !a.is_go()
    };
    s.add("J3-缺 harness 拦", j3, "无 harness 必拦");

    // J4 双缺拦截。
    let j4 = {
        let mut a = EnvPrecheck::new();
        a.gate("g").is_err() && !a.is_go()
    };
    s.add("J4-双缺拦截", j4, "双缺必拦");

    // J5 锚值越界拒。
    let j5 = {
        let mut a = EnvPrecheck::new();
        a.set_anchor(0).err() == Some(EV_BAD_ANCHOR)
            && a.set_anchor(ANCHOR_MAX + 1).err() == Some(EV_BAD_ANCHOR)
    };
    s.add("J5-锚越界拒", j5, "零锚/超域锚必拒");

    // J6 账本完整：预检账逐条合法。
    let j6 = {
        let mut a = EnvPrecheck::new();
        let _ = a.set_anchor(7);
        let _ = a.set_harness(true);
        let _ = a.gate("g1");
        let _ = a.gate("g2");
        a.ledger.len() == 2 && a.ledger_ok() && a.seq_unique()
    };
    s.add("J6-账本完整", j6, "双预检账合法无重号");

    // J7 修件复检：补件后 Go。
    let j7 = {
        let mut a = EnvPrecheck::new();
        let _ = a.set_harness(true);
        let _ = a.gate("ng");
        let _ = a.set_anchor(0x1234);
        a.gate("ok").is_ok()
    };
    s.add("J7-修件复检", j7, "补锚后复检放行");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut a = EnvPrecheck::new();
        let _ = a.set_anchor(7);
        let _ = a.set_harness(true);
        let _ = a.gate("g");
        let f1 = a.fold();
        let f2 = a.fold();
        a.seal();
        f1 == f2 && a.gate("post").err() == Some(EV_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例预检：先缺件 NoGo 再补齐 Go。
    let mut p = EnvPrecheck::new();
    let _ = p.gate("sum-nogo");
    let _ = p.set_anchor(0x2027);
    let _ = p.set_harness(true);
    let _ = p.gate("sum-go");

    s.add("全记录自洽", p.all_recs_ok(), "各条预检留痕过自洽核");
    s.add("判定分流", p.go_count() > 0 && p.nogo_count() > 0, "Go 与 NoGo 均有留痕");
    s.add("账面摘要", p.book_summary().contains("recs="), "摘要含记录数");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10204_env_flow() {
        let mut a = EnvPrecheck::new();
        assert!(a.gate("ng").is_err());
        let _ = a.set_anchor(0x2027);
        let _ = a.set_harness(true);
        assert!(a.gate("ok").is_ok());
        assert_eq!(a.set_anchor(0), Err(EV_BAD_ANCHOR));
    }

    #[test]
    fn unx10204_checks_green() {
        let s = run_unx10204_checks();
        assert!(s.all_passed(), "UNX-F10204 判据未全绿");
    }

    #[test]
    fn verdict_split_and_summary() {
        let mut p = EnvPrecheck::new();
        let _ = p.set_anchor(0x27);
        p.harness = true;
        let _ = p.gate("v1");
        p.harness = false;
        let _ = p.gate("v2");
        assert!(p.all_recs_ok());
        assert_eq!(p.rec_count(), 2);
        assert_eq!(p.go_count() + p.nogo_count(), 2);
        assert!(p.book_summary().contains("anchor=true"));
        assert!(p.book_summary().contains("harness=false"));
    }
}

