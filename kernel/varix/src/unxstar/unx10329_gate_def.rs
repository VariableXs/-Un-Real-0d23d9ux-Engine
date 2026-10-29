//! UNX-F10329 R-C3-003 释放闸定义（释放条件五要素成文）（0x1b3a0 段）
//!
//! 锚点：UNX-F10329-J1 五要素成文——零阻塞/零丢失/全归因/回归过/
//! 双签讫五条释放条件逐条成文（要素号 1..=5，文不空），五条齐文且
//! 封定才算释放闸定义完毕。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：要素账（条=要素号+成文）+ 成文核（重号即拒、空文即拒）+
//! 封定闸（五条齐文方可封定，封定后拒改）+ 注入核（坏要素号/重号/
//! 空文/未齐封定四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b3a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b3a0;
pub const GD_ENO: u32 = SEG_BASE + 0x01; // 要素号越界
pub const GD_DUP: u32 = SEG_BASE + 0x02; // 要素号重号
pub const GD_EMPTY: u32 = SEG_BASE + 0x03; // 成文空
pub const GD_CAP: u32 = SEG_BASE + 0x04; // 账超容
pub const GD_SEALED: u32 = SEG_BASE + 0x05; // 封定后改
pub const GD_NOTREADY: u32 = SEG_BASE + 0x06; // 未齐封定

/// 五要素号。
pub const E_NO_BLOCK: u32 = 1;
pub const E_NO_LOSS: u32 = 2;
pub const E_ATTRIBUTED: u32 = 3;
pub const E_REGRESSED: u32 = 4;
pub const E_SIGNED: u32 = 5;
/// 要素数锚。
pub const ELEM_N: usize = 5;
/// 账定容（=ELEM_N，多一条即拒）。
pub const CAP: usize = 6;

/// 释放要素条。
#[derive(Clone)]
pub struct GateElement {
    pub eno: u32,
    pub text: String,
}

/// 释放闸定义账。
pub struct GateDef {
    pub elems: Vec<GateElement>,
    pub sealed: bool,
}

impl GateDef {
    pub fn new() -> GateDef {
        GateDef { elems: Vec::new(), sealed: false }
    }

    fn pos(&self, eno: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.elems.len() {
            if self.elems[k].eno == eno {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 成文一条要素。
    pub fn define(&mut self, eno: u32, text: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(GD_SEALED);
        }
        if eno == 0 || eno > ELEM_N as u32 {
            return Err(GD_ENO);
        }
        if self.pos(eno).is_some() {
            return Err(GD_DUP);
        }
        if text.is_empty() {
            return Err(GD_EMPTY);
        }
        if self.elems.len() >= CAP {
            return Err(GD_CAP);
        }
        self.elems.push(GateElement { eno, text: text.to_string() });
        Ok(())
    }

    /// 要素是否已成文。
    pub fn has(&self, eno: u32) -> bool {
        self.pos(eno).is_some()
    }

    /// 取成文（未成文返回空串）。
    pub fn text_of(&self, eno: u32) -> String {
        match self.pos(eno) {
            None => String::new(),
            Some(p) => self.elems[p].text.clone(),
        }
    }

    /// 五条齐文。
    pub fn codified(&self) -> bool {
        let mut e = 1u32;
        while e <= ELEM_N as u32 {
            if !self.has(e) {
                return false;
            }
            e += 1;
        }
        true
    }

    /// 已成文条数。
    pub fn elem_count(&self) -> usize {
        self.elems.len()
    }

    /// 封定：五条齐方可封定。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(GD_SEALED);
        }
        if !self.codified() {
            return Err(GD_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 定义账指纹（按要素号序敏感）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b3a00;
        h = h.wrapping_mul(31).wrapping_add(self.elems.len() as u32);
        let mut k = 0usize;
        while k < self.elems.len() {
            let e = &self.elems[k];
            h = h.wrapping_mul(31).wrapping_add(e.eno);
            let tb = e.text.as_bytes();
            let mut i = 0usize;
            while i < tb.len() {
                h = h.wrapping_mul(31).wrapping_add(tb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 五要素样例成文。
pub fn gate_def_samples() -> Vec<(u32, &'static str)> {
    let mut v: Vec<(u32, &'static str)> = Vec::new();
    v.push((E_NO_BLOCK, "干跑零阻塞：预演全步无挂起"));
    v.push((E_NO_LOSS, "零丢失：触达回执闭环无缺"));
    v.push((E_ATTRIBUTED, "全归因：失绿件逐件归主"));
    v.push((E_REGRESSED, "回归过：释放项回归集全绿"));
    v.push((E_SIGNED, "双签讫：入场确认书双签在账"));
    v
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut g = GateDef::new();
    // 1) 坏要素号（0 与 6）必拒
    got[0] = g.define(0, "零号").err() == Some(GD_ENO)
        && g.define(ELEM_N as u32 + 1, "越界").err() == Some(GD_ENO);
    // 2) 重号必拒
    let _ = g.define(E_NO_BLOCK, "甲文");
    got[1] = g.define(E_NO_BLOCK, "重文").err() == Some(GD_DUP);
    // 3) 空文必拒
    got[2] = g.define(E_NO_LOSS, "").err() == Some(GD_EMPTY);
    // 4) 未齐封定必拒
    got[3] = g.seal().err() == Some(GD_NOTREADY);
    got
}

/// 批内判据。
pub fn run_unx10329_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10329");

    // J1 正路径：五要素成文封定
    let mut g = GateDef::new();
    let samples = gate_def_samples();
    let mut ok = true;
    let mut k = 0usize;
    while k < samples.len() {
        let (eno, text) = (samples[k].0, samples[k].1);
        if g.define(eno, text).is_err() {
            ok = false;
        }
        k += 1;
    }
    let sealed = g.seal();
    s.add(
        "J1-五要素成文",
        ok && g.codified() && sealed.is_ok() && g.sealed,
        "五条释放条件全成文并成功封定",
    );

    // J2 逐要素在账
    let j2 = g.has(E_NO_BLOCK)
        && g.has(E_NO_LOSS)
        && g.has(E_ATTRIBUTED)
        && g.has(E_REGRESSED)
        && g.has(E_SIGNED);
    s.add("J2-逐要素在账", j2, "五要素逐号在账");

    // J3 注入检出：坏号 + 重号
    let inj = inject_four();
    s.add("J3-坏号重号检出", inj[0] && inj[1], "坏要素号与重号必拒");

    // J4 注入检出：空文 + 未齐封定
    s.add("J4-空文未齐检出", inj[2] && inj[3], "空文与未齐封定必拒");

    // J5 缺一条不成文（缺 E_SIGNED）
    let j5 = {
        let mut t = GateDef::new();
        let _ = t.define(E_NO_BLOCK, "甲");
        let _ = t.define(E_NO_LOSS, "乙");
        let _ = t.define(E_ATTRIBUTED, "丙");
        let _ = t.define(E_REGRESSED, "丁");
        !t.codified() && t.seal().err() == Some(GD_NOTREADY)
    };
    s.add("J5-缺条不成文", j5, "缺任一要素即不成文且不得封定");

    // J6 封定后改文必拒
    let j6 = {
        let mut t = GateDef::new();
        let samples2 = gate_def_samples();
        let mut q = 0usize;
        while q < samples2.len() {
            let _ = t.define(samples2[q].0, samples2[q].1);
            q += 1;
        }
        let _ = t.seal();
        t.define(E_SIGNED, "改文").err() == Some(GD_SEALED)
    };
    s.add("J6-封定拒改", j6, "封定后成文必拒");

    // J7 未成文取文得空串
    let j7 = GateDef::new().text_of(E_NO_BLOCK).is_empty();
    s.add("J7-取文空串", j7, "未成文要素取文为空");

    // J8 已成文取文回原文
    let j8 = {
        let mut t = GateDef::new();
        let _ = t.define(E_ATTRIBUTED, "归因成文样");
        t.text_of(E_ATTRIBUTED) == "归因成文样"
    };
    s.add("J8-取文回原", j8, "已成文要素取文回原文");

    // J9 超出五条必拒（第六条）
    let j9 = {
        let mut t = GateDef::new();
        let samples3 = gate_def_samples();
        let mut q = 0usize;
        while q < samples3.len() {
            let _ = t.define(samples3[q].0, samples3[q].1);
            q += 1;
        }
        t.define(6, "六号越界").err() == Some(GD_ENO)
    };
    s.add("J9-六号越界", j9, "要素号域封闭于五");

    // J10 指纹随成文推进且封定后稳定
    let j10 = {
        let mut t = GateDef::new();
        let f0 = t.fold();
        let _ = t.define(E_NO_BLOCK, "甲");
        let f1 = t.fold();
        let _ = t.define(E_NO_LOSS, "乙");
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J10-指纹推进", j10, "每成文一步指纹一步且终态稳定");

    // J11 已成文条数对平
    s.add(
        "J11-条数对平",
        g.elem_count() == ELEM_N,
        "已成文条数恰为五",
    );

    // J12 动态明细：成文字节量落账
    let mut bytes_n = 0usize;
    let mut q = 0usize;
    while q < g.elems.len() {
        bytes_n += g.elems[q].text.as_bytes().len();
        q += 1;
    }
    s.add_dyn(
        "J12-成文量",
        bytes_n > 0,
        &format!("bytes={}", bytes_n),
    );

    // J13 动态明细：定义账指纹落账
    let fp = g.fold();
    s.add_dyn("J13-定义指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10329_gate_def_flow() {
        let mut g = GateDef::new();
        assert!(g.define(0, "零").err() == Some(GD_ENO));
        assert!(g.define(6, "越").err() == Some(GD_ENO));
        assert!(g.define(E_NO_BLOCK, "").err() == Some(GD_EMPTY));
        assert!(g.define(E_NO_BLOCK, "甲文").is_ok());
        assert!(g.define(E_NO_BLOCK, "重").err() == Some(GD_DUP));
        assert!(g.seal().err() == Some(GD_NOTREADY));
        let samples = gate_def_samples();
        let mut k = 1usize;
        while k < samples.len() {
            let _ = g.define(samples[k].0, samples[k].1);
            k += 1;
        }
        assert!(g.codified());
        assert!(g.seal().is_ok());
        assert!(g.define(E_ATTRIBUTED, "封后").err() == Some(GD_SEALED));
        assert!(g.seal().err() == Some(GD_SEALED));
    }

    #[test]
    fn unx10329_checks_green() {
        let s = run_unx10329_checks();
        assert!(s.all_passed(), "UNX-F10329 判据未全绿");
    }
}
