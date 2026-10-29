//! UNX-F10343 wave-06 产物面清单（六轴产物清点）（0x1b560 段）
//!
//! 锚点：UNX-F10343-J1 六轴产物清点——wave-06 产物按六轴逐轴清点，
//! 每轴至少两件产物、产物名全账唯一，六轴全达锚才算产物面清单落账，
//! 封单后拒加。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：产物单（件=轴号+产物名，名全账唯一）+ 轴锚（每轴两件）+
//! 缺轴核（未达锚轴可枚举）+ 封单闸（六轴齐方可封单）+ 注入核（坏轴
//! 号/重名/空名/未齐封单四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b560 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b560;
pub const AR_AXIS: u32 = SEG_BASE + 0x01; // 轴号越界
pub const AR_DUP: u32 = SEG_BASE + 0x02; // 产物名重号
pub const AR_EMPTY: u32 = SEG_BASE + 0x03; // 产物名空
pub const AR_CAP: u32 = SEG_BASE + 0x04; // 单超容
pub const AR_NOTREADY: u32 = SEG_BASE + 0x05; // 未齐封单
pub const AR_SEALED: u32 = SEG_BASE + 0x06; // 封单后加件

/// 六轴号。
pub const AXIS_A1: u8 = 1;
pub const AXIS_A2: u8 = 2;
pub const AXIS_A3: u8 = 3;
pub const AXIS_A4: u8 = 4;
pub const AXIS_A5: u8 = 5;
pub const AXIS_A6: u8 = 6;
pub const AXIS_N: u8 = 7;

/// 每轴产物锚。
pub const AXIS_NEED: usize = 2;
/// 单定容。
pub const CAP: usize = 18;

/// 产物件。
#[derive(Clone)]
pub struct Artifact {
    pub axis: u8,
    pub name: String,
}

/// wave-06 产物面清单。
pub struct AxisInv {
    pub items: Vec<Artifact>,
    pub sealed: bool,
}

impl AxisInv {
    pub fn new() -> AxisInv {
        AxisInv { items: Vec::new(), sealed: false }
    }

    fn has_name(&self, name: &str) -> bool {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].name == name {
                return true;
            }
            k += 1;
        }
        false
    }

    fn axis_count(&self, axis: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].axis == axis {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 登产物。
    pub fn put(&mut self, axis: u8, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(AR_SEALED);
        }
        if axis == 0 || axis >= AXIS_N {
            return Err(AR_AXIS);
        }
        if self.has_name(name) {
            return Err(AR_DUP);
        }
        if name.is_empty() {
            return Err(AR_EMPTY);
        }
        if self.items.len() >= CAP {
            return Err(AR_CAP);
        }
        self.items.push(Artifact { axis, name: name.to_string() });
        Ok(())
    }

    /// 轴件数。
    pub fn count_of(&self, axis: u8) -> usize {
        if axis == 0 || axis >= AXIS_N {
            return 0;
        }
        self.axis_count(axis)
    }

    /// 轴达锚。
    pub fn axis_ok(&self, axis: u8) -> bool {
        self.count_of(axis) >= AXIS_NEED
    }

    /// 未达锚轴清单。
    pub fn missing_axes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        let mut a = 1u8;
        while a < AXIS_N {
            if !self.axis_ok(a) {
                v.push(a);
            }
            a += 1;
        }
        v
    }

    /// 清单齐：六轴全达锚。
    pub fn complete(&self) -> bool {
        self.missing_axes().is_empty()
    }

    /// 轴产物名清单。
    pub fn names_of(&self, axis: u8) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].axis == axis {
                v.push(self.items[k].name.clone());
            }
            k += 1;
        }
        v
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(AR_SEALED);
        }
        if !self.complete() {
            return Err(AR_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 产物单指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b5600;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = h.wrapping_mul(31).wrapping_add(it.axis as u32);
            let nb = it.name.as_bytes();
            let mut i = 0usize;
            while i < nb.len() {
                h = h.wrapping_mul(31).wrapping_add(nb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut v = AxisInv::new();
    // 1) 坏轴号必拒
    got[0] = v.put(0, "x").err() == Some(AR_AXIS)
        && v.put(7, "y").err() == Some(AR_AXIS);
    // 2) 重名必拒（跨轴亦重）
    let _ = v.put(AXIS_A1, "判据库快照");
    got[1] = v.put(AXIS_A2, "判据库快照").err() == Some(AR_DUP);
    // 3) 空名必拒
    got[2] = v.put(AXIS_A3, "").err() == Some(AR_EMPTY);
    // 4) 未齐封单必拒
    got[3] = v.seal().err() == Some(AR_NOTREADY);
    got
}

/// 六轴样例产物（每轴两件）。
pub fn artifact_samples() -> Vec<(u8, &'static str)> {
    let mut v: Vec<(u8, &'static str)> = Vec::new();
    v.push((AXIS_A1, "判据库快照甲"));
    v.push((AXIS_A1, "判据库快照乙"));
    v.push((AXIS_A2, "归因账快照甲"));
    v.push((AXIS_A2, "归因账快照乙"));
    v.push((AXIS_A3, "干跑账快照甲"));
    v.push((AXIS_A3, "干跑账快照乙"));
    v.push((AXIS_A4, "确认书快照甲"));
    v.push((AXIS_A4, "确认书快照乙"));
    v.push((AXIS_A5, "预算账快照甲"));
    v.push((AXIS_A5, "预算账快照乙"));
    v.push((AXIS_A6, "闭账快照甲"));
    v.push((AXIS_A6, "闭账快照乙"));
    v
}

/// 批内判据。
pub fn run_unx10343_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10343");

    // J1 正路径：六轴各两件清点齐并封单
    let mut v = AxisInv::new();
    let samples = artifact_samples();
    let mut ok = true;
    let mut k = 0usize;
    while k < samples.len() {
        if v.put(samples[k].0, samples[k].1).is_err() {
            ok = false;
        }
        k += 1;
    }
    let sealed = v.seal();
    s.add(
        "J1-六轴产物清点",
        ok && v.complete() && sealed.is_ok() && v.sealed,
        "六轴各两件产物清点齐并成功封单",
    );

    // J2 逐轴达锚
    let j2 = v.axis_ok(AXIS_A1)
        && v.axis_ok(AXIS_A2)
        && v.axis_ok(AXIS_A3)
        && v.axis_ok(AXIS_A4)
        && v.axis_ok(AXIS_A5)
        && v.axis_ok(AXIS_A6);
    s.add("J2-逐轴达锚", j2, "六轴件数各达二");

    // J3 注入检出：坏轴号 + 重名
    let inj = inject_four();
    s.add("J3-轴号重名检出", inj[0] && inj[1], "坏轴号与跨轴重名必拒");

    // J4 注入检出：空名 + 未齐封单
    s.add("J4-空名未齐检出", inj[2] && inj[3], "空名与未齐封单必拒");

    // J5 缺轴可枚举（只清点 A1 两件）
    let j5 = {
        let mut t = AxisInv::new();
        let _ = t.put(AXIS_A1, "甲件");
        let _ = t.put(AXIS_A1, "乙件");
        t.missing_axes() == [AXIS_A2, AXIS_A3, AXIS_A4, AXIS_A5, AXIS_A6]
            && !t.complete()
    };
    s.add("J5-缺轴枚举", j5, "未达锚轴按序枚举");

    // J6 封单后加件必拒
    let j6 = {
        let mut t = AxisInv::new();
        let samples2 = artifact_samples();
        let mut q = 0usize;
        while q < samples2.len() {
            let _ = t.put(samples2[q].0, samples2[q].1);
            q += 1;
        }
        let _ = t.seal();
        t.put(AXIS_A1, "迟件").err() == Some(AR_SEALED)
    };
    s.add("J6-封单拒加", j6, "封单后登产物必拒");

    // J7 轴件名清单按登记序
    let j7 = {
        let ns = v.names_of(AXIS_A1);
        ns.len() == 2 && ns[0] == "判据库快照甲" && ns[1] == "判据库快照乙"
    };
    s.add("J7-名序一致", j7, "轴内名清单保持登记顺序");

    // J8 总件数对平
    s.add(
        "J8-总件对平",
        v.items.len() == artifact_samples().len(),
        "总件数恰为十二",
    );

    // J9 容量边界
    let j9 = {
        let mut t = AxisInv::new();
        let mut n = 0u8;
        let mut filled = true;
        while (n as usize) < CAP {
            let a = 1 + (n % 6);
            let nm = format!("件{}", n);
            if t.put(a, &nm).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.put(AXIS_A1, "超件").err() == Some(AR_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原单不损");

    // J10 指纹随登件推进且封单后稳定
    let j10 = {
        let mut t = AxisInv::new();
        let f0 = t.fold();
        let _ = t.put(AXIS_A1, "甲");
        let f1 = t.fold();
        let _ = t.put(AXIS_A2, "乙");
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J10-指纹推进", j10, "每登一件指纹一变且终态稳定");

    // J11 空单缺轴全列
    let j11 = AxisInv::new().missing_axes().len() == 6;
    s.add("J11-空单全缺", j11, "空单六轴全未达锚");

    // J12 动态明细：缺轴计数落账
    let mz = v.missing_axes().len();
    s.add_dyn("J12-缺轴计数", mz == 0, &format!("missing={}", mz));

    // J13 动态明细：产物单指纹落账
    let fp = v.fold();
    s.add_dyn("J13-产物指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10343_axis_inv_flow() {
        let mut v = AxisInv::new();
        assert!(v.put(0, "零").err() == Some(AR_AXIS));
        assert!(v.put(7, "越").err() == Some(AR_AXIS));
        assert!(v.put(AXIS_A1, "").err() == Some(AR_EMPTY));
        assert!(v.put(AXIS_A1, "甲件").is_ok());
        assert!(v.put(AXIS_A2, "甲件").err() == Some(AR_DUP));
        assert!(v.put(AXIS_A1, "乙件").is_ok());
        assert!(!v.complete());
        assert!(v.seal().err() == Some(AR_NOTREADY));
        let samples = artifact_samples();
        let mut k = 2usize;
        while k < samples.len() {
            let _ = v.put(samples[k].0, samples[k].1);
            k += 1;
        }
        assert!(v.complete());
        assert!(v.seal().is_ok());
        assert!(v.put(AXIS_A1, "迟").err() == Some(AR_SEALED));
    }

    #[test]
    fn unx10343_checks_green() {
        let s = run_unx10343_checks();
        assert!(s.all_passed(), "UNX-F10343 判据未全绿");
    }
}
