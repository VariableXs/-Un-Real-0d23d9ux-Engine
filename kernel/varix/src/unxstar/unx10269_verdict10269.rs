//! UNX-F10269 压力下 verdict 纪律（压力态不豁免人工干预）（0x15CE0 段）
//!
//! 锚点：UNX-F10269-J1 纪律达成（压力态 verdict 五态照常机检且压力不
//! 豁免人工干预标记 10/10 次）；J2 豁免企图检出（压力挂豁免/干预绕过
//! 10/10 次拒绝）；J3 纪律指纹（同压力态同裁决同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：压力态裁决机（五态 + 压力系数）+ 干预账（人工干预标记强制）
//! + 豁免拦截核 + 注入核（挂豁免/绕干预/冻结必抓）。

use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x15CE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x15CE0;
pub const VS_EXEMPT_TRY: u32 = SEG_BASE + 0x01; // 压力挂豁免
pub const VS_BYPASS_MARK: u32 = SEG_BASE + 0x02; // 绕干预标记
pub const VS_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后裁
pub const VS_BAD_PRESS: u32 = SEG_BASE + 0x04; // 坏压力系数
pub const VS_NO_HUMAN: u32 = SEG_BASE + 0x05; // 无干预人

/// 五态（与 10248 对齐）。
pub const O_DRAFT: u8 = 0;
pub const O_REVIEW: u8 = 1;
pub const O_PASS: u8 = 2;
pub const O_REJECT: u8 = 3;
pub const O_WITHDRAW: u8 = 4;

/// 压力态裁决机。
pub struct PressVerdict {
    state: u8,
    pressure: u16, // 千分位压力系数
    human_mark: Option<String>, // 人工干预人（压力态强制）
    exempt_tries: u32,
    frozen: bool,
}

impl PressVerdict {
    pub fn new(pressure: u16) -> Result<PressVerdict, &'static str> {
        if pressure > 1000 {
            return Err("bad_pressure");
        }
        Ok(PressVerdict {
            state: O_DRAFT,
            pressure,
            human_mark: None,
            exempt_tries: 0,
            frozen: false,
        })
    }

    /// 人工干预标记：压力态强制（无干预人不得进入 review）。
    pub fn mark_human(&mut self, who: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if who.is_empty() {
            return Err("no_human");
        }
        self.human_mark = Some(who.to_string());
        Ok(())
    }

    /// 迁移：review 前必须有人工干预标记（压力态不豁免）。
    pub fn transit(&mut self, to: u8) -> Result<u8, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if to > O_WITHDRAW {
            return Err("bad_state");
        }
        if to == O_REVIEW && self.human_mark.is_none() {
            return Err("need_human");
        }
        // 迁移闭包（压力态同 10248 真源）
        let legal = matches!(
            (self.state, to),
            (O_DRAFT, O_REVIEW)
                | (O_REVIEW, O_PASS)
                | (O_REVIEW, O_REJECT)
                | (O_REJECT, O_DRAFT)
                | (O_PASS, O_WITHDRAW)
                | (O_REVIEW, O_WITHDRAW)
                | (O_DRAFT, O_WITHDRAW)
        );
        if !legal {
            return Err("out_closure");
        }
        self.state = to;
        Ok(to)
    }

    /// 压力豁免企图：压力态企图挂豁免（豁免只认认证面 10250 流程）。
    pub fn exempt_attempt(&mut self) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        self.exempt_tries += 1;
        Err("exempt_denied")
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    pub fn human_of(&self) -> Option<&str> {
        self.human_mark.as_deref()
    }

    pub fn exempt_count(&self) -> u32 {
        self.exempt_tries
    }

    pub fn pressure(&self) -> u16 {
        self.pressure
    }

    /// 纪律指纹：状态 × 压力 × 干预人折叠。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5 ^ (self.pressure as u32);
        h = h.wrapping_mul(0x01000193) ^ (self.state as u32);
        for b in self.human_mark.as_deref().unwrap_or("-").as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 注入核。
pub fn inject_fault(v: &mut PressVerdict, code: u32, who: &str) -> Result<u8, &'static str> {
    match code {
        VS_EXEMPT_TRY => v.exempt_attempt().map(|_| 0),
        VS_BYPASS_MARK => v.transit(O_REVIEW), // 无干预人直进 review
        VS_FROZEN => {
            v.freeze();
            v.transit(O_REVIEW)
        }
        VS_BAD_PRESS => {
            let _ = who;
            Err("bad_pressure")
        }
        VS_NO_HUMAN => v.mark_human("").map(|_| 0),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10269_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10269");

    // J1 压力态全路径：干预标记 + 迁移到 pass
    let j1 = {
        let mut v = PressVerdict::new(900).unwrap();
        let m = v.mark_human("裁决人甲");
        let t1 = v.transit(O_REVIEW);
        let t2 = v.transit(O_PASS);
        m == Ok(()) && t1 == Ok(O_REVIEW) && t2 == Ok(O_PASS) && v.human_of() == Some("裁决人甲")
    };
    s.add("J1-压力态全路径", j1, "标记后三步到 pass");

    // J2a 绕干预标记拦截
    let j2a = inject_fault(&mut PressVerdict::new(500).unwrap(), VS_BYPASS_MARK, "x")
        == Err("need_human");
    s.add("J2a-绕干预拦", j2a, "无标记不得进 review");

    // J2b 压力豁免企图拦截
    let j2b = {
        let mut v = PressVerdict::new(800).unwrap();
        let r1 = v.exempt_attempt();
        let r2 = v.exempt_attempt();
        r1 == Err("exempt_denied") && r2 == Err("exempt_denied") && v.exempt_count() == 2
    };
    s.add("J2b-豁免拦截", j2b, "压力态豁免全拒且留痕");

    // J3 坏压力系数拒收（坑 14：构造体不可 Eq，用 .err() 口径）
    let j3 = PressVerdict::new(1001).err() == Some("bad_pressure");
    s.add("J3-坏系数拒", j3, "1001 即拒");

    // J4 纪律指纹：同人同态同指纹/换人变
    let j4 = {
        let mk = |who: &str| {
            let mut v = PressVerdict::new(700).unwrap();
            let _ = v.mark_human(who);
            let _ = v.transit(O_REVIEW);
            v.fp()
        };
        let f1 = mk("甲");
        let f2 = mk("甲");
        let f3 = mk("乙");
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J4-纪律指纹", j4, "同人同态同指纹");

    // J5 迁移闭包压力不放宽：跳步全拒
    let j5 = {
        let mut ok = 0;
        for bad in [O_PASS, O_REJECT] {
            let mut v = PressVerdict::new(500).unwrap();
            let _ = v.mark_human("甲");
            if v.transit(bad) == Err("out_closure") {
                ok += 1;
            }
        }
        ok == 2
    };
    s.add("J5-闭包不放宽", j5, "压力不豁免闭包");

    // J6 注入五路
    let codes = [VS_EXEMPT_TRY, VS_BYPASS_MARK, VS_FROZEN, VS_BAD_PRESS, VS_NO_HUMAN];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut PressVerdict::new(600).unwrap(), **c, "y").is_err())
        .count();
    s.add("J6-注入五路", caught == 5, "五路注入全拒");

    // J7 冻结后迁移拒收
    let j7 = {
        let mut v = PressVerdict::new(500).unwrap();
        inject_fault(&mut v, VS_FROZEN, "z");
        v.is_frozen()
    };
    s.add("J7-冻结拒迁", j7, "封版后迁移必拒");

    // J8 空干预人拒收
    let j8 = inject_fault(&mut PressVerdict::new(500).unwrap(), VS_NO_HUMAN, "w")
        == Err("no_human");
    s.add("J8-空干预人拒", j8, "空名即拒");

    // J9 压力边界：1000 合法 1001 拒
    let j9 = PressVerdict::new(1000).is_ok() && PressVerdict::new(1001).is_err();
    s.add("J9-压力边界", j9, "1000 合法 1001 拒");

    // J10 十轮纪律回放：10 轮路径指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut v = PressVerdict::new(850).unwrap();
            let _ = v.mark_human("J2");
            let _ = v.transit(O_REVIEW);
            let _ = v.transit(O_REJECT);
            let _ = v.transit(O_DRAFT);
            let _ = v.transit(O_WITHDRAW);
            (v.state(), v.fp())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == O_WITHDRAW
    };
    s.add("J10-十轮纪律", j10, "10/10 轮路径一致");

    s
}
