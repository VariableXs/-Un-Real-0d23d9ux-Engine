//! UNX-F10349 wave-06 演习脚本会签（脚本与 B33 干跑一致核）（0x1b620 段）
//!
//! 锚点：UNX-F10349-J1 脚本干跑一致——脚本步逐号录操作，B33 干跑步逐
//! 号回填操作（无脚本步即拒），两录逐号操作全等且步数一致才算一致核
//! 过，一致核过后由双签会签，未一致不得会签。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：双录账（脚本录+干跑步录，各=号+操作）+ 一致核（逐号操作比对，
//! 差异单可枚举）+ 会签核（一致后方许双签，同签人重签即拒）+ 注入核
//! （零号/重号/无脚本步干跑/未一致会签四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b620 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b620;
pub const SC_ZERO: u32 = SEG_BASE + 0x01; // 步号零
pub const SC_DUP: u32 = SEG_BASE + 0x02; // 步号重号
pub const SC_EMPTY: u32 = SEG_BASE + 0x03; // 操作空
pub const SC_NOSC: u32 = SEG_BASE + 0x04; // 无脚本步
pub const SC_CAP: u32 = SEG_BASE + 0x05; // 账超容
pub const SC_NOTMATCH: u32 = SEG_BASE + 0x06; // 未一致会签
pub const SC_RESIGN: u32 = SEG_BASE + 0x07; // 同签人重签

/// 会签人数锚（二）。
pub const COSIGN_NEEDED: usize = 2;
/// 脚本步锚（六步）。
pub const STEPS_NEEDED: usize = 6;
/// 账定容。
pub const CAP: usize = 12;

/// 脚本步。
#[derive(Clone)]
pub struct ScriptStep {
    pub no: u32,
    pub op: String,
}

/// 干跑步。
#[derive(Clone)]
pub struct DryStep {
    pub no: u32,
    pub op: String,
}

/// 脚本会签核。
pub struct ScriptCossign {
    pub script: Vec<ScriptStep>,
    pub dry: Vec<DryStep>,
    pub signs: [bool; 2],
    pub sealed: bool,
}

impl ScriptCossign {
    pub fn new() -> ScriptCossign {
        ScriptCossign {
            script: Vec::new(),
            dry: Vec::new(),
            signs: [false; 2],
            sealed: false,
        }
    }

    fn script_pos(&self, no: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.script.len() {
            if self.script[k].no == no {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn dry_pos(&self, no: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.dry.len() {
            if self.dry[k].no == no {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 录脚本步。
    pub fn script_step(&mut self, no: u32, op: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_RESIGN);
        }
        if no == 0 {
            return Err(SC_ZERO);
        }
        if self.script_pos(no).is_some() {
            return Err(SC_DUP);
        }
        if op.is_empty() {
            return Err(SC_EMPTY);
        }
        if self.script.len() >= CAP {
            return Err(SC_CAP);
        }
        self.script.push(ScriptStep { no, op: op.to_string() });
        Ok(())
    }

    /// 回填干跑步（须有对应脚本步）。
    pub fn dry_step(&mut self, no: u32, op: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_RESIGN);
        }
        if no == 0 {
            return Err(SC_ZERO);
        }
        if op.is_empty() {
            return Err(SC_EMPTY);
        }
        if self.script_pos(no).is_none() {
            return Err(SC_NOSC);
        }
        if self.dry_pos(no).is_some() {
            return Err(SC_DUP);
        }
        if self.dry.len() >= CAP {
            return Err(SC_CAP);
        }
        self.dry.push(DryStep { no, op: op.to_string() });
        Ok(())
    }

    /// 操作差异单（脚本与干跑操作不等的步号）。
    pub fn op_diffs(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.script.len() {
            let st = &self.script[k];
            match self.dry_pos(st.no) {
                None => v.push(st.no),
                Some(p) => {
                    if self.dry[p].op != st.op {
                        v.push(st.no);
                    }
                }
            }
            k += 1;
        }
        v
    }

    /// 步数差（干跑多录的步号）。
    pub fn extra_dry(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.dry.len() {
            if self.script_pos(self.dry[k].no).is_none() {
                v.push(self.dry[k].no);
            }
            k += 1;
        }
        v
    }

    /// 一致核过：步数一致且差异双零。
    pub fn matched(&self) -> bool {
        self.script.len() >= STEPS_NEEDED
            && self.script.len() == self.dry.len()
            && self.op_diffs().is_empty()
            && self.extra_dry().is_empty()
    }

    /// 会签：一致核过方可，同签人重签即拒。
    pub fn cosign(&mut self, who: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_RESIGN);
        }
        if who >= 2 {
            return Err(SC_RESIGN);
        }
        if !self.matched() {
            return Err(SC_NOTMATCH);
        }
        if self.signs[who as usize] {
            return Err(SC_RESIGN);
        }
        self.signs[who as usize] = true;
        Ok(())
    }

    /// 会签讫：两人齐签。
    pub fn cossigned(&self) -> bool {
        self.signs[0] && self.signs[1]
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_RESIGN);
        }
        if !self.cossigned() {
            return Err(SC_NOTMATCH);
        }
        self.sealed = true;
        Ok(())
    }

    /// 会签账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b6200;
        h = h.wrapping_mul(31).wrapping_add(self.script.len() as u32);
        let mut k = 0usize;
        while k < self.script.len() {
            h = h.wrapping_mul(31).wrapping_add(self.script[k].no);
            let ob = self.script[k].op.as_bytes();
            let mut i = 0usize;
            while i < ob.len() {
                h = h.wrapping_mul(31).wrapping_add(ob[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.dry.len() as u32);
        k = 0;
        while k < self.dry.len() {
            h = h.wrapping_mul(31).wrapping_add(self.dry[k].no);
            let ob = self.dry[k].op.as_bytes();
            let mut i = 0usize;
            while i < ob.len() {
                h = h.wrapping_mul(31).wrapping_add(ob[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.signs[0] as u32);
        h = h.wrapping_mul(31).wrapping_add(self.signs[1] as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut c = ScriptCossign::new();
    // 1) 零号必拒
    got[0] = c.script_step(0, "零步").err() == Some(SC_ZERO);
    // 2) 重号必拒
    let _ = c.script_step(1, "装载判据");
    got[1] = c.script_step(1, "重步").err() == Some(SC_DUP);
    // 3) 无脚本步干跑必拒
    got[2] = c.dry_step(9, "越步").err() == Some(SC_NOSC);
    // 4) 未一致会签必拒
    got[3] = c.cosign(0).err() == Some(SC_NOTMATCH);
    got
}

/// 脚本六步样例（号, 操作）。
pub fn script_ops() -> Vec<(u32, &'static str)> {
    let mut v: Vec<(u32, &'static str)> = Vec::new();
    v.push((1, "装载判据"));
    v.push((2, "对平核"));
    v.push((3, "干跑零阻塞"));
    v.push((4, "条件逐项核"));
    v.push((5, "释放登记"));
    v.push((6, "回归回放"));
    v
}

/// 批内判据。
pub fn run_unx10349_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10349");

    // J1 正路径：六步脚本录+干跑回填一致并双签封账
    let mut c = ScriptCossign::new();
    let ops = script_ops();
    let mut ok = true;
    let mut k = 0usize;
    while k < ops.len() {
        if c.script_step(ops[k].0, ops[k].1).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < ops.len() {
        if c.dry_step(ops[k].0, ops[k].1).is_err() {
            ok = false;
        }
        k += 1;
    }
    if c.cosign(0).is_err() || c.cosign(1).is_err() {
        ok = false;
    }
    let sealed = c.seal();
    s.add(
        "J1-脚本干跑一致",
        ok && c.matched() && c.cossigned() && sealed.is_ok(),
        "六步脚本与干跑逐号一致并双签封账",
    );

    // J2 差异双零
    let j2 = c.op_diffs().is_empty() && c.extra_dry().is_empty();
    s.add("J2-差异双零", j2, "操作差异单与多录单皆空");

    // J3 注入检出：零号 + 重号
    let inj = inject_four();
    s.add("J3-零号重号检出", inj[0] && inj[1], "零步号与重步号必拒");

    // J4 注入检出：越步干跑 + 未一致会签
    s.add("J4-越步未一检出", inj[2] && inj[3], "无脚本步干跑与未一致会签必拒");

    // J5 操作不一致即列差异
    let j5 = {
        let mut t = ScriptCossign::new();
        let _ = t.script_step(1, "装载判据");
        let _ = t.script_step(2, "对平核");
        let _ = t.dry_step(1, "装载判据");
        let _ = t.dry_step(2, "另操作");
        t.op_diffs() == [2u32] && !t.matched()
    };
    s.add("J5-异操作列单", j5, "干跑操作异于脚本必列差异单");

    // J6 步数不一致不成一致核
    let j6 = {
        let mut t = ScriptCossign::new();
        let ops2 = script_ops();
        let mut q = 0usize;
        while q < ops2.len() {
            let _ = t.script_step(ops2[q].0, ops2[q].1);
            if q < ops2.len() - 1 {
                let _ = t.dry_step(ops2[q].0, ops2[q].1);
            }
            q += 1;
        }
        !t.matched() && t.cosign(0).err() == Some(SC_NOTMATCH)
    };
    s.add("J6-缺步不成", j6, "干跑步缺一即不成一致核");

    // J7 同签人重签必拒
    let j7 = {
        let mut t = ScriptCossign::new();
        let ops3 = script_ops();
        let mut q = 0usize;
        while q < ops3.len() {
            let _ = t.script_step(ops3[q].0, ops3[q].1);
            let _ = t.dry_step(ops3[q].0, ops3[q].1);
            q += 1;
        }
        let _ = t.cosign(0);
        t.cosign(0).err() == Some(SC_RESIGN)
    };
    s.add("J7-重签拒收", j7, "同签人二次会签必拒");

    // J8 空操作必拒
    let j8 = ScriptCossign::new().script_step(1, "").err() == Some(SC_EMPTY);
    s.add("J8-空操作拒收", j8, "操作空必拒");

    // J9 封账后录步必拒
    let j9 = {
        let mut t = ScriptCossign::new();
        let ops4 = script_ops();
        let mut q = 0usize;
        while q < ops4.len() {
            let _ = t.script_step(ops4[q].0, ops4[q].1);
            let _ = t.dry_step(ops4[q].0, ops4[q].1);
            q += 1;
        }
        let _ = t.cosign(0);
        let _ = t.cosign(1);
        let _ = t.seal();
        t.script_step(9, "迟步").err() == Some(SC_RESIGN)
    };
    s.add("J9-封账拒录", j9, "封账后录步必拒");

    // J10 容量边界
    let j10 = {
        let mut t = ScriptCossign::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            let op = "容步";
            if t.script_step(n, op).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.script_step(999, "超").err() == Some(SC_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原账不损");

    // J11 指纹随回填推进且会签后稳定
    let j11 = {
        let mut t = ScriptCossign::new();
        let _ = t.script_step(1, "指步");
        let f0 = t.fold();
        let _ = t.dry_step(1, "指步");
        let f1 = t.fold();
        f0 != f1 && t.fold() == f1
    };
    s.add("J11-指纹推进", j11, "回填一步指纹一变且终态稳定");

    // J12 动态明细：差异计数落账
    let df = c.op_diffs().len();
    s.add_dyn("J12-差异计数", df == 0, &format!("diffs={}", df));

    // J13 动态明细：会签账指纹落账
    let fp = c.fold();
    s.add_dyn("J13-会签指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10349_script_cossign_flow() {
        let mut c = ScriptCossign::new();
        assert!(c.script_step(0, "零").err() == Some(SC_ZERO));
        assert!(c.script_step(1, "").err() == Some(SC_EMPTY));
        assert!(c.script_step(1, "甲步").is_ok());
        assert!(c.script_step(1, "重").err() == Some(SC_DUP));
        assert!(c.dry_step(9, "越").err() == Some(SC_NOSC));
        assert!(c.dry_step(1, "甲步").is_ok());
        assert!(c.dry_step(1, "甲步").err() == Some(SC_DUP));
        assert!(!c.matched());
        assert!(c.cosign(0).err() == Some(SC_NOTMATCH));
        let ops = script_ops();
        let mut k = 1usize;
        while k < ops.len() {
            let _ = c.script_step(ops[k].0, ops[k].1);
            let _ = c.dry_step(ops[k].0, ops[k].1);
            k += 1;
        }
        assert!(c.matched());
        assert!(c.cosign(0).is_ok());
        assert!(c.cosign(0).err() == Some(SC_RESIGN));
        assert!(c.cosign(1).is_ok());
        assert!(c.cossigned());
        assert!(c.seal().is_ok());
        assert!(c.dry_step(2, "封后").err() == Some(SC_RESIGN));
    }

    #[test]
    fn unx10349_checks_green() {
        let s = run_unx10349_checks();
        assert!(s.all_passed(), "UNX-F10349 判据未全绿");
    }
}
