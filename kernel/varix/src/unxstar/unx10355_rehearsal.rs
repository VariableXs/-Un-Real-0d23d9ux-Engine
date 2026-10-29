//! UNX-F10355 wave-06 预演联排（全链彩排）（0x1b6e0 段）
//!
//! 锚点：UNX-F10355-J1 预演联排——六场（点名/干跑/正演走位/失绿预演/
//! 清账预演/收场）逐场先开演后收场，场号严格接续登记，前场未收场不得
//! 开演下一场，收场时刻不得早于开演时刻，六场全收场零倒挂方算全链彩
//! 排走通。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：联排账（场=号+开演刻+收场刻）+ 场序核（号接续登记）+ 演序核
//! （前场未收不开演）+ 时刻核（收场不早于开演）+ 注入核（跳场/先收/
//! 倒挂/重开四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b6e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b6e0;
pub const RH_SEQ: u32 = SEG_BASE + 0x01; // 场号不接续
pub const RH_REBEG: u32 = SEG_BASE + 0x02; // 重复开演
pub const RH_ORDER: u32 = SEG_BASE + 0x03; // 前场未收
pub const RH_NOTBEGUN: u32 = SEG_BASE + 0x04; // 未开演先收场
pub const RH_TIME: u32 = SEG_BASE + 0x05; // 收场时刻倒挂
pub const RH_REDONE: u32 = SEG_BASE + 0x06; // 重复收场
pub const RH_NOSTG: u32 = SEG_BASE + 0x07; // 场未在册
pub const RH_NOTREADY: u32 = SEG_BASE + 0x08; // 彩排未完即封

/// 六场联排。
pub const STG_ROLL: u8 = 1;
pub const STG_DRY: u8 = 2;
pub const STG_LIVE: u8 = 3;
pub const STG_FAIL: u8 = 4;
pub const STG_CLEAR: u8 = 5;
pub const STG_WRAP: u8 = 6;
pub const STG_N: u8 = 7;

/// 场数锚（六场全排）。
pub const STAGES_NEEDED: usize = 6;
/// 账定容。
pub const CAP: usize = 8;

/// 场名。
pub fn stage_name(no: u8) -> &'static str {
    match no {
        STG_ROLL => "点名",
        STG_DRY => "干跑",
        STG_LIVE => "正演走位",
        STG_FAIL => "失绿预演",
        STG_CLEAR => "清账预演",
        _ => "收场",
    }
}

/// 联排场：号 + 开演刻 + 收场刻。
#[derive(Clone)]
pub struct Stage {
    pub no: u8,
    pub begun: Option<u32>,
    pub done: Option<u32>,
}

impl Stage {
    /// 已开演？
    pub fn is_begun(&self) -> bool {
        self.begun.is_some()
    }

    /// 已收场？
    pub fn is_done(&self) -> bool {
        self.done.is_some()
    }

    /// 场历时（未收场即 0）。
    pub fn span(&self) -> u32 {
        match (self.begun, self.done) {
            (Some(b), Some(d)) => {
                if d > b {
                    d - b
                } else {
                    0
                }
            }
            _ => 0,
        }
    }
}

/// 联排账。
#[derive(Clone)]
pub struct Rehearsal {
    pub stages: Vec<Stage>,
    pub sealed: bool,
}

impl Rehearsal {
    /// 空账。
    pub fn new() -> Self {
        Rehearsal {
            stages: Vec::new(),
            sealed: false,
        }
    }

    /// 找场。
    pub fn find(&self, no: u8) -> Option<usize> {
        let mut i = 0;
        while i < self.stages.len() {
            if self.stages[i].no == no {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登场：号必须接续（1,2,3,…）。
    pub fn book(&mut self, no: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(RH_NOTREADY);
        }
        if no < STG_ROLL || no >= STG_N {
            return Err(RH_SEQ);
        }
        if self.stages.len() >= CAP {
            return Err(RH_SEQ);
        }
        if self.find(no).is_some() {
            return Err(RH_REBEG);
        }
        if no as usize != self.stages.len() + 1 {
            return Err(RH_SEQ);
        }
        self.stages.push(Stage {
            no,
            begun: None,
            done: None,
        });
        Ok(())
    }

    /// 开演：前场须已收场。
    pub fn begin(&mut self, no: u8, tick: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RH_NOTREADY);
        }
        let idx = match self.find(no) {
            Some(i) => i,
            None => return Err(RH_NOSTG),
        };
        if self.stages[idx].is_begun() {
            return Err(RH_REBEG);
        }
        if no > STG_ROLL {
            let prev = match self.find(no - 1) {
                Some(i) => i,
                None => return Err(RH_NOSTG),
            };
            if !self.stages[prev].is_done() {
                return Err(RH_ORDER);
            }
        }
        self.stages[idx].begun = Some(tick);
        Ok(())
    }

    /// 收场：须先开演且时刻不倒挂。
    pub fn complete(&mut self, no: u8, tick: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RH_NOTREADY);
        }
        let idx = match self.find(no) {
            Some(i) => i,
            None => return Err(RH_NOSTG),
        };
        let b = match self.stages[idx].begun {
            Some(b) => b,
            None => return Err(RH_NOTBEGUN),
        };
        if self.stages[idx].is_done() {
            return Err(RH_REDONE);
        }
        if tick < b {
            return Err(RH_TIME);
        }
        self.stages[idx].done = Some(tick);
        Ok(())
    }

    /// 当前应演场（第一未收场）。
    pub fn current(&self) -> Option<u8> {
        let mut i = 0;
        while i < self.stages.len() {
            if !self.stages[i].is_done() {
                return Some(self.stages[i].no);
            }
            i += 1;
        }
        None
    }

    /// 未收场数。
    pub fn undone(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.stages.len() {
            if !self.stages[i].is_done() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 全链彩排走通：六场全登全收。
    pub fn rehearsed(&self) -> bool {
        self.stages.len() == STAGES_NEEDED && self.undone() == 0
    }

    /// 总历时。
    pub fn total_span(&self) -> u32 {
        let mut t = 0;
        let mut i = 0;
        while i < self.stages.len() {
            t += self.stages[i].span();
            i += 1;
        }
        t
    }

    /// 封账：彩排未走通即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(RH_NOTREADY);
        }
        if !self.rehearsed() {
            return Err(RH_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：号/开演刻/收场刻逐场叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b6e01b6;
        let mut i = 0;
        while i < self.stages.len() {
            let st = &self.stages[i];
            h = (h ^ (st.no as u32)).wrapping_mul(0x01000193);
            h = (h ^ st.begun.unwrap_or(0)).wrapping_mul(0x01000193);
            h = (h ^ st.done.unwrap_or(0)).wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }
}

/// 演刻样例：六场开/收时刻。
pub fn tick_samples() -> [(u8, u32, u32); 6] {
    [
        (STG_ROLL, 100, 140),
        (STG_DRY, 150, 640),
        (STG_LIVE, 700, 1900),
        (STG_FAIL, 1950, 2600),
        (STG_CLEAR, 2650, 3100),
        (STG_WRAP, 3150, 3400),
    ]
}

/// 注入核：四路坏演必抓。
/// 0: 跳场开演；1: 未开演先收场；2: 收场倒挂；3: 重复开演。
pub fn inject_four() -> [bool; 4] {
    let mut r = Rehearsal::new();
    let mut k = 0;
    while k < tick_samples().len() {
        let (no, _, _) = tick_samples()[k];
        let _ = r.book(no);
        k += 1;
    }
    let _ = r.begin(STG_ROLL, 100);
    let _ = r.complete(STG_ROLL, 140);
    let d0 = r.begin(STG_LIVE, 700).err() == Some(RH_ORDER);
    let d1 = r.complete(STG_DRY, 640).err() == Some(RH_NOTBEGUN);
    let d2 = {
        let _ = r.begin(STG_DRY, 150);
        r.complete(STG_DRY, 120).err() == Some(RH_TIME)
    };
    let d3 = {
        let _ = r.complete(STG_DRY, 640);
        r.begin(STG_DRY, 200).err() == Some(RH_REBEG)
    };
    [d0, d1, d2, d3]
}

/// 判据：run_unx10355_checks。
pub fn run_unx10355_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10355");
    // J1 正路径：六场全登场。
    let mut r = Rehearsal::new();
    let mut j1 = true;
    let mut k = 0;
    while k < tick_samples().len() {
        let (no, _, _) = tick_samples()[k];
        if r.book(no).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-六场登场", j1 && r.stages.len() == 6, "六场接续登记");
    // J2 边界：跳号登场拒。
    let mut r2 = Rehearsal::new();
    let j2 = r2.book(STG_DRY).err() == Some(RH_SEQ);
    s.add("J2-跳号拒", j2, "首场不接续即 RH_SEQ");
    // J3 注入：跳场开演必抓。
    let mut r3 = Rehearsal::new();
    let mut k3 = 0;
    while k3 < tick_samples().len() {
        let (no, _, _) = tick_samples()[k3];
        let _ = r3.book(no);
        k3 += 1;
    }
    let _ = r3.begin(STG_ROLL, 100);
    let _ = r3.complete(STG_ROLL, 140);
    let j3 = r3.begin(STG_FAIL, 1950).err() == Some(RH_ORDER);
    s.add("J3-跳演抓", j3, "前场未收开演即 RH_ORDER");
    // J4 注入：未开演先收场必抓。
    let mut r4 = Rehearsal::new();
    let mut k4 = 0;
    while k4 < tick_samples().len() {
        let (no, _, _) = tick_samples()[k4];
        let _ = r4.book(no);
        k4 += 1;
    }
    let j4 = r4.complete(STG_ROLL, 140).err() == Some(RH_NOTBEGUN);
    s.add("J4-先收抓", j4, "未开演收场即 RH_NOTBEGUN");
    // J5 注入：收场倒挂必抓。
    let mut r5 = Rehearsal::new();
    let mut k5 = 0;
    while k5 < tick_samples().len() {
        let (no, _, _) = tick_samples()[k5];
        let _ = r5.book(no);
        k5 += 1;
    }
    let _ = r5.begin(STG_ROLL, 100);
    let j5 = r5.complete(STG_ROLL, 90).err() == Some(RH_TIME);
    s.add("J5-倒挂抓", j5, "收场早于开演即 RH_TIME");
    // J6 注入：重复开演必抓。
    let mut r6 = Rehearsal::new();
    let mut k6 = 0;
    while k6 < tick_samples().len() {
        let (no, _, _) = tick_samples()[k6];
        let _ = r6.book(no);
        k6 += 1;
    }
    let _ = r6.begin(STG_ROLL, 100);
    let j6 = r6.begin(STG_ROLL, 120).err() == Some(RH_REBEG);
    s.add("J6-重开抓", j6, "一场两开即 RH_REBEG");
    // J7 边界：未在册场开演拒。
    let mut r7 = Rehearsal::new();
    let j7 = r7.begin(STG_ROLL, 100).err() == Some(RH_NOSTG);
    s.add("J7-无场拒", j7, "场未登场即 RH_NOSTG");
    // J8 正路径：六场逐开逐收零倒挂。
    let mut r8 = Rehearsal::new();
    let mut k8 = 0;
    while k8 < tick_samples().len() {
        let (no, b, d) = tick_samples()[k8];
        let _ = r8.book(no);
        let _ = r8.begin(no, b);
        let _ = r8.complete(no, d);
        k8 += 1;
    }
    let j8 = r8.rehearsed() && r8.undone() == 0 && r8.current().is_none();
    s.add("J8-全链通", j8, "六场全收场彩排走通");
    // J9 正路径：走通后封账。
    let mut r9 = r8.clone();
    let j9 = r9.seal().is_ok() && r9.sealed;
    s.add("J9-封账成", j9, "彩排走通封账通过");
    // J10 边界：未走通封账拒。
    let mut r10 = Rehearsal::new();
    let mut k10 = 0;
    while k10 < 3 {
        let (no, b, d) = tick_samples()[k10];
        let _ = r10.book(no);
        let _ = r10.begin(no, b);
        let _ = r10.complete(no, d);
        k10 += 1;
    }
    let j10 = r10.seal().err() == Some(RH_NOTREADY) && r10.undone() == 3;
    s.add("J10-缺封拒", j10, "三场未收封账即 RH_NOTREADY");
    // J11 边界：封账后再开演拒。
    let mut r11 = r8.clone();
    let _ = r11.seal();
    let j11 = r11.begin(STG_ROLL, 999).err() == Some(RH_NOTREADY);
    s.add("J11-封后演拒", j11, "封账后开演即 RH_NOTREADY");
    // J12 动态：当前应演场推进核。
    let mut r12 = Rehearsal::new();
    let mut k12 = 0;
    while k12 < 2 {
        let (no, b, d) = tick_samples()[k12];
        let _ = r12.book(no);
        let _ = r12.begin(no, b);
        let _ = r12.complete(no, d);
        k12 += 1;
    }
    let j12 = r12.current() == Some(STG_LIVE);
    s.add_dyn(
        "J12-推进核",
        j12,
        &format!(
            "已收两场，当前应演「{}」",
            stage_name(r12.current().unwrap_or(0))
        ),
    );
    // J13 动态：总历时与指纹。
    let j13 = r8.total_span() == 3080 && r8.fold() != 0;
    s.add_dyn(
        "J13-历时出数",
        j13,
        &format!(
            "六场总历时 {} tick，首场「{}」，指纹 {:08x}",
            r8.total_span(),
            stage_name(STG_ROLL),
            r8.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "跳场/先收/倒挂/重开四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut r = Rehearsal::new();
        let mut k = 0;
        while k < tick_samples().len() {
            let (no, b, d) = tick_samples()[k];
            assert!(r.book(no).is_ok());
            assert!(r.begin(no, b).is_ok());
            assert!(r.complete(no, d).is_ok());
            k += 1;
        }
        assert!(r.rehearsed());
        assert!(r.seal().is_ok());
        assert!(r.begin(STG_ROLL, 9).err() == Some(RH_NOTREADY));
        assert!(r.fold() != 0);
        assert_eq!(r.total_span(), 3080);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10355_checks();
        assert!(s.all_passed());
    }
}
