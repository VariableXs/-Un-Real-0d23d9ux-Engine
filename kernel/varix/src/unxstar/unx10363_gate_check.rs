//! UNX-F10363 正演条件终检（五要素+入场确认书核）（0x1b7e0 段）
//!
//! 锚点：UNX-F10363-J1 正演条件终检——五要素（现场零阻塞/条件自证/
//! 释放登记/回归保护/入场双签）逐项以证据名核哈希，证据哈希与要素基
//! 准哈希不核即该要素失核，一核定音不得复核；入场确认书须两面互异双
//! 签在册；五要素全真且确认书双签方过终检闸。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：终检账（五要素各一判定+确认书双签位）+ 哈希核（证据名 FNV
//! 对基准）+ 双签核（面异双签）+ 闸核（五真双签放行）+ 注入核（失核
//! 强过/复核/同面签/越面签四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b7e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b7e0;
pub const GF_ELEM: u32 = SEG_BASE + 0x01; // 要素号越界
pub const GF_RECHECK: u32 = SEG_BASE + 0x02; // 要素复核
pub const GF_NOEV: u32 = SEG_BASE + 0x03; // 空证据名
pub const GF_FACE: u32 = SEG_BASE + 0x04; // 签发面越界
pub const GF_DUP: u32 = SEG_BASE + 0x05; // 同面双签
pub const GF_RELETTER: u32 = SEG_BASE + 0x06; // 确认书重签
pub const GF_NOTREADY: u32 = SEG_BASE + 0x07; // 终检未过即封
pub const GF_SEALED: u32 = SEG_BASE + 0x08; // 封账后再核

/// 五要素。
pub const E_ZERO_BLOCK: u8 = 1;
pub const E_SELF_PROOF: u8 = 2;
pub const E_REG_LOG: u8 = 3;
pub const E_REGRESS: u8 = 4;
pub const E_ENTRY: u8 = 5;
pub const E_N: u8 = 5;

/// 签发面三面。
pub const FACE_N: u8 = 3;

/// 要素基准名（证据名须与其逐字同）。
pub const ELEM_NAMES: [&str; 5] = [
    "现场零阻塞",
    "条件逐项自证",
    "释放登记在册",
    "回归保护入集",
    "入场双签在册",
];

/// 要素名 u64 FNV-1a。
fn fnv64(name: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u64)).wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 要素基准哈希。
pub fn want_digest(eno: u8) -> u64 {
    if eno < E_ZERO_BLOCK || eno > E_N {
        return 0;
    }
    fnv64(ELEM_NAMES[(eno - 1) as usize])
}

/// 正演条件终检账。
#[derive(Clone)]
pub struct GateFinalCheck {
    pub got: [Option<bool>; E_N as usize],
    pub sign_a: Option<u8>,
    pub sign_b: Option<u8>,
    pub sealed: bool,
}

impl GateFinalCheck {
    /// 空账。
    pub fn new() -> Self {
        GateFinalCheck {
            got: [None; E_N as usize],
            sign_a: None,
            sign_b: None,
            sealed: false,
        }
    }

    /// 核要素：证据名哈希对基准，一核定音。
    pub fn verify(&mut self, eno: u8, evidence: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(GF_SEALED);
        }
        if eno < E_ZERO_BLOCK || eno > E_N {
            return Err(GF_ELEM);
        }
        if evidence.is_empty() {
            return Err(GF_NOEV);
        }
        if self.got[(eno - 1) as usize].is_some() {
            return Err(GF_RECHECK);
        }
        let ok = fnv64(evidence) == want_digest(eno);
        self.got[(eno - 1) as usize] = Some(ok);
        Ok(ok)
    }

    /// 确认书双签：面异、各一笔。
    pub fn letter(&mut self, a: u8, b: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(GF_SEALED);
        }
        if a >= FACE_N || b >= FACE_N {
            return Err(GF_FACE);
        }
        if self.sign_a.is_some() && self.sign_b.is_some() {
            return Err(GF_RELETTER);
        }
        if self.sign_a == Some(a) {
            return Err(GF_DUP);
        }
        if a == b {
            return Err(GF_DUP);
        }
        if self.sign_a.is_none() {
            self.sign_a = Some(a);
            self.sign_b = Some(b);
        } else {
            return Err(GF_RELETTER);
        }
        Ok(())
    }

    /// 要素失核清单。
    pub fn failed_elems(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < E_N as usize {
            if self.got[i] == Some(false) {
                out.push(i as u8 + 1);
            }
            i += 1;
        }
        out
    }

    /// 未核要素数。
    pub fn unverified(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < E_N as usize {
            if self.got[i].is_none() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 终检通过：五真 + 双签在册。
    pub fn final_ok(&self) -> bool {
        if self.sign_a.is_none() || self.sign_b.is_none() {
            return false;
        }
        let mut i = 0;
        while i < E_N as usize {
            if self.got[i] != Some(true) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 封账：未过闸即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(GF_SEALED);
        }
        if !self.final_ok() {
            return Err(GF_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：五判定+双签叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b7e01b7;
        let mut i = 0;
        while i < E_N as usize {
            let v = match self.got[i] {
                Some(true) => 1u32,
                Some(false) => 2,
                None => 0,
            };
            h = (h ^ v).wrapping_mul(0x01000193);
            i += 1;
        }
        h = (h ^ self.sign_a.unwrap_or(9) as u32).wrapping_mul(0x01000193);
        h = (h ^ self.sign_b.unwrap_or(9) as u32).wrapping_mul(0x01000193);
        h
    }
}

/// 注入核：四路坏账必抓。
/// 0: 失核强过（要素核否后封账拒）；1: 要素复核；2: 同面双签；3: 越面签。
pub fn inject_four() -> [bool; 4] {
    let mut g = GateFinalCheck::new();
    let mut e = E_ZERO_BLOCK;
    while e <= E_N {
        let _ = g.verify(e, ELEM_NAMES[(e - 1) as usize]);
        e += 1;
    }
    // 要素 3 改核为否：另建坏账。
    let d0 = {
        let mut bad = GateFinalCheck::new();
        let mut k = E_ZERO_BLOCK;
        while k <= E_N {
            let ev = if k == E_REG_LOG {
                "错证名"
            } else {
                ELEM_NAMES[(k - 1) as usize]
            };
            let _ = bad.verify(k, ev);
            k += 1;
        }
        let _ = bad.letter(0, 1);
        bad.seal().err() == Some(GF_NOTREADY)
    };
    let d1 = g.verify(E_SELF_PROOF, ELEM_NAMES[1]).err() == Some(GF_RECHECK);
    let d2 = {
        let mut twin = GateFinalCheck::new();
        twin.letter(1, 1).err() == Some(GF_DUP)
    };
    let d3 = {
        let mut out = GateFinalCheck::new();
        out.letter(3, 0).err() == Some(GF_FACE)
    };
    [d0, d1, d2, d3]
}

/// 判据：run_unx10363_checks。
pub fn run_unx10363_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10363");
    // J1 正路径：五要素以基准名逐项核真。
    let mut g = GateFinalCheck::new();
    let mut j1 = true;
    let mut e = E_ZERO_BLOCK;
    while e <= E_N {
        match g.verify(e, ELEM_NAMES[(e - 1) as usize]) {
            Ok(true) => {}
            _ => j1 = false,
        }
        e += 1;
    }
    s.add("J1-五真", j1 && g.unverified() == 0, "五要素证据哈希全核");
    // J2 边界：要素号越界拒。
    let mut g2 = GateFinalCheck::new();
    let j2 = g2.verify(0, "x").err() == Some(GF_ELEM)
        && g2.verify(6, "x").err() == Some(GF_ELEM);
    s.add("J2-越素拒", j2, "要素号不在 1..=5 即 GF_ELEM");
    // J3 边界：空证据拒。
    let mut g3 = GateFinalCheck::new();
    let j3 = g3.verify(E_ZERO_BLOCK, "").err() == Some(GF_NOEV);
    s.add("J3-空证拒", j3, "空证据名即 GF_NOEV");
    // J4 注入：失核检出。
    let mut g4 = GateFinalCheck::new();
    let _ = g4.verify(E_REGRESS, "错证名");
    let j4 = g4.failed_elems() == vec![E_REGRESS];
    s.add("J4-失核录", j4, "证据名不核即录失核");
    // J5 注入：要素复核必抓。
    let mut g5 = GateFinalCheck::new();
    let _ = g5.verify(E_SELF_PROOF, ELEM_NAMES[1]);
    let j5 = g5
        .verify(E_SELF_PROOF, ELEM_NAMES[1])
        .err()
        == Some(GF_RECHECK);
    s.add("J5-复核抓", j5, "一素两核即 GF_RECHECK");
    // J6 注入：同面双签必抓。
    let mut g6 = GateFinalCheck::new();
    let j6 = g6.letter(2, 2).err() == Some(GF_DUP);
    s.add("J6-同面抓", j6, "两面同号即 GF_DUP");
    // J7 注入：越面签必抓。
    let mut g7 = GateFinalCheck::new();
    let j7 = g7.letter(0, 3).err() == Some(GF_FACE);
    s.add("J7-越面抓", j7, "面号越界即 GF_FACE");
    // J8 注入：确认书重签必抓。
    let mut g8 = GateFinalCheck::new();
    let _ = g8.letter(0, 1);
    let j8 = g8.letter(1, 2).err() == Some(GF_RELETTER);
    s.add("J8-重签抓", j8, "双签后再签即 GF_RELETTER");
    // J9 正路径：五真+双签过闸封账。
    let mut g9 = g.clone();
    let _ = g9.letter(0, 2);
    let j9 = g9.final_ok() && g9.seal().is_ok() && g9.sealed;
    s.add("J9-过闸封", j9, "五真双签终检过闸");
    // J10 边界：缺确认书封账拒。
    let mut g10 = g.clone();
    let j10 = g10.seal().err() == Some(GF_NOTREADY);
    s.add("J10-缺签拒", j10, "无双签封账即 GF_NOTREADY");
    // J11 边界：缺要素封账拒。
    let mut g11 = GateFinalCheck::new();
    let mut e11 = E_ZERO_BLOCK;
    while e11 <= E_N - 1 {
        let _ = g11.verify(e11, ELEM_NAMES[(e11 - 1) as usize]);
        e11 += 1;
    }
    let _ = g11.letter(0, 1);
    let j11 = g11.seal().err() == Some(GF_NOTREADY) && g11.unverified() == 1;
    s.add("J11-缺素拒", j11, "要素缺一封账即 GF_NOTREADY");
    // J12 边界：封账后再核拒。
    let mut g12 = g.clone();
    let _ = g12.letter(0, 2);
    let _ = g12.seal();
    let j12 = g12
        .verify(E_ZERO_BLOCK, ELEM_NAMES[0])
        .err()
        == Some(GF_SEALED);
    s.add("J12-封后核拒", j12, "封账后核要素即 GF_SEALED");
    // J13 动态：要素基准哈希互异出数。
    let h1 = want_digest(E_ZERO_BLOCK);
    let h5 = want_digest(E_N);
    let j13 = h1 != h5 && h1 != 0 && g.fold() != 0;
    s.add_dyn(
        "J13-基准出数",
        j13,
        &format!(
            "要素基准哈希首 {:016x} 末 {:016x} 互异，指纹 {:08x}",
            h1,
            h5,
            g.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "失核/复核/同面/越面四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut g = GateFinalCheck::new();
        let mut e = E_ZERO_BLOCK;
        while e <= E_N {
            assert!(g.verify(e, ELEM_NAMES[(e - 1) as usize]) == Ok(true));
            e += 1;
        }
        assert!(g.letter(0, 2).is_ok());
        assert!(g.letter(1, 2).err() == Some(GF_RELETTER));
        assert!(g.final_ok());
        assert!(g.seal().is_ok());
        assert!(g.verify(1, "x").err() == Some(GF_SEALED));
        assert!(g.fold() != 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10363_checks();
        assert!(s.all_passed());
    }
}
