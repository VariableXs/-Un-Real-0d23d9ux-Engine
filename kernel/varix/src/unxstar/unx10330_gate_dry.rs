//! UNX-F10330 R-C3-003 释放闸预演（干跑零阻塞）（0x1b3c0 段）
//!
//! 锚点：UNX-F10330-J1 干跑零阻塞——释放闸各步按序登记、按序干跑，
//! 干跑中任一步不得挂起（等待刻必须为零），全步零挂起零等待才算预演
//! 通过、释放闸可以进入正演。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：预演步账（步=步号+步名+已跑+等待刻）+ 序核（登记与执行均须
//! 严格递增，乱序即拒）+ 零阻塞核（等待刻非零即判挂起）+ 注入核
//! （乱序/重步/未知步/挂起步四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b3c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b3c0;
pub const DR_ORDER: u32 = SEG_BASE + 0x01; // 乱序
pub const DR_DUP: u32 = SEG_BASE + 0x02; // 重步
pub const DR_NOSTEP: u32 = SEG_BASE + 0x03; // 未知步
pub const DR_BLOCK: u32 = SEG_BASE + 0x04; // 挂起步
pub const DR_CAP: u32 = SEG_BASE + 0x05; // 步账超容
pub const DR_EMPTY: u32 = SEG_BASE + 0x06; // 步名空
pub const DR_RERUN: u32 = SEG_BASE + 0x07; // 已跑再跑

/// 预演步锚（七步）。
pub const STEPS_NEED: usize = 7;
/// 步账定容。
pub const CAP: usize = 12;
/// 干跑零阻塞锚：等待刻必须为零。
pub const DRY_TICKS: u32 = 0;

/// 预演步。
#[derive(Clone)]
pub struct DryStep {
    pub seq: u32,
    pub name: String,
    pub ran: bool,
    pub ticks: u32,
}

/// 释放闸干跑核。
pub struct DryRun {
    pub steps: Vec<DryStep>,
    pub sealed: bool,
}

impl DryRun {
    pub fn new() -> DryRun {
        DryRun { steps: Vec::new(), sealed: false }
    }

    fn pos(&self, seq: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].seq == seq {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 期望的下一步号（空账从 1 起，否则末步号加一）。
    pub fn expect_seq(&self) -> u32 {
        match self.steps.len() {
            0 => 1,
            n => self.steps[n - 1].seq + 1,
        }
    }

    /// 登记步：严格按序。
    pub fn plan(&mut self, seq: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DR_RERUN);
        }
        if self.pos(seq).is_some() {
            return Err(DR_DUP);
        }
        if seq != self.expect_seq() {
            return Err(DR_ORDER);
        }
        if name.is_empty() {
            return Err(DR_EMPTY);
        }
        if self.steps.len() >= CAP {
            return Err(DR_CAP);
        }
        self.steps.push(DryStep { seq, name: name.to_string(), ran: false, ticks: 0 });
        Ok(())
    }

    /// 干跑一步：按序且零等待刻。
    pub fn run(&mut self, seq: u32, ticks: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DR_RERUN);
        }
        match self.pos(seq) {
            None => Err(DR_NOSTEP),
            Some(p) => {
                if self.steps[p].ran {
                    return Err(DR_RERUN);
                }
                // 序核：只许跑当前期望步
                if seq != self.next_run_seq() {
                    return Err(DR_ORDER);
                }
                // 零阻塞核：干跑不得挂起
                if ticks != DRY_TICKS {
                    self.steps[p].ran = true;
                    self.steps[p].ticks = ticks;
                    return Err(DR_BLOCK);
                }
                self.steps[p].ran = true;
                self.steps[p].ticks = 0;
                Ok(())
            }
        }
    }

    /// 期望执行的步号（首个未跑步）。
    pub fn next_run_seq(&self) -> u32 {
        let mut k = 0usize;
        while k < self.steps.len() {
            if !self.steps[k].ran {
                return self.steps[k].seq;
            }
            k += 1;
        }
        0
    }

    /// 挂起步清单（已跑且等待刻非零）。
    pub fn blocked_steps(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].ran && self.steps[k].ticks != DRY_TICKS {
                v.push(self.steps[k].seq);
            }
            k += 1;
        }
        v
    }

    /// 未跑清单。
    pub fn unrun(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.steps.len() {
            if !self.steps[k].ran {
                v.push(self.steps[k].seq);
            }
            k += 1;
        }
        v
    }

    /// 干跑通过：步数达锚、全跑、零挂起。
    pub fn dry_ok(&self) -> bool {
        self.steps.len() >= STEPS_NEED
            && self.unrun().is_empty()
            && self.blocked_steps().is_empty()
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(DR_RERUN);
        }
        if !self.dry_ok() {
            return Err(DR_ORDER);
        }
        self.sealed = true;
        Ok(())
    }

    /// 干跑指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b3c00;
        h = h.wrapping_mul(31).wrapping_add(self.steps.len() as u32);
        let mut k = 0usize;
        while k < self.steps.len() {
            let st = &self.steps[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(st.seq)
                .wrapping_mul(31)
                .wrapping_add(st.ran as u32)
                .wrapping_mul(31)
                .wrapping_add(st.ticks);
            let nb = st.name.as_bytes();
            let mut i = 0usize;
            while i < nb.len() {
                h = h.wrapping_mul(31).wrapping_add(nb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut d = DryRun::new();
    // 1) 乱序登记必拒（首步必须为 1）
    got[0] = d.plan(2, "跳步").err() == Some(DR_ORDER);
    // 2) 重步必拒
    let _ = d.plan(1, "甲步");
    got[1] = d.plan(1, "重步").err() == Some(DR_DUP);
    // 3) 未知步执行必拒
    got[2] = d.run(9, 0).err() == Some(DR_NOSTEP);
    // 4) 挂起步必抓（等待刻非零）
    let _ = d.plan(2, "乙步");
    got[3] = d.run(1, 40).err() == Some(DR_BLOCK) && d.blocked_steps() == [1u32];
    got
}

/// 预演七步样例步名。
pub fn dry_step_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.push("条件读盘");
    v.push("判据装订");
    v.push("回执预检");
    v.push("归因预检");
    v.push("回归预检");
    v.push("双签预检");
    v.push("释放预登");
    v
}

/// 批内判据。
pub fn run_unx10330_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10330");

    // J1 正路径：七步按序登记零阻塞干跑并封定
    let mut d = DryRun::new();
    let names = dry_step_names();
    let mut ok = true;
    let mut k = 0usize;
    while k < names.len() {
        if d.plan(k as u32 + 1, names[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < names.len() {
        if d.run(k as u32 + 1, 0).is_err() {
            ok = false;
        }
        k += 1;
    }
    let sealed = d.seal();
    s.add(
        "J1-干跑零阻塞",
        ok && d.dry_ok() && sealed.is_ok(),
        "七步按序登记零等待干跑全过并封定",
    );

    // J2 全步零等待刻
    let mut tick_sum = 0u32;
    let mut q = 0usize;
    while q < d.steps.len() {
        tick_sum += d.steps[q].ticks;
        q += 1;
    }
    s.add("J2-零等待账", tick_sum == 0, "全步等待刻之和为零");

    // J3 注入检出：乱序 + 重步
    let inj = inject_four();
    s.add("J3-乱序重步检出", inj[0] && inj[1], "乱序登记与重步必拒");

    // J4 注入检出：未知步 + 挂起步
    s.add("J4-未知挂起检出", inj[2] && inj[3], "未知步与挂起步必抓");

    // J5 执行乱序必拒（跳过未跑步）
    let j5 = {
        let mut t = DryRun::new();
        let _ = t.plan(1, "甲");
        let _ = t.plan(2, "乙");
        let _ = t.plan(3, "丙");
        t.run(2, 0).err() == Some(DR_ORDER)
    };
    s.add("J5-执行乱序拒", j5, "执行必须自首步起按序推进");

    // J6 挂起步入单且不入干跑过
    let j6 = {
        let mut t = DryRun::new();
        let _ = t.plan(1, "甲");
        let _ = t.run(1, 25);
        !t.dry_ok() && t.blocked_steps() == [1u32]
    };
    s.add("J6-挂起不过", j6, "等待刻非零即不算干跑通过");

    // J7 重跑必拒
    let j7 = {
        let mut t = DryRun::new();
        let _ = t.plan(1, "甲");
        let _ = t.run(1, 0);
        t.run(1, 0).err() == Some(DR_RERUN)
    };
    s.add("J7-重跑拒收", j7, "一步只许干跑一次");

    // J8 空步名必拒
    let j8 = DryRun::new().plan(1, "").err() == Some(DR_EMPTY);
    s.add("J8-空名拒收", j8, "步名空必拒");

    // J9 缺步不封定
    let j9 = {
        let mut t = DryRun::new();
        let mut q = 0usize;
        while q < STEPS_NEED - 1 {
            let _ = t.plan(q as u32 + 1, "缺步样");
            let _ = t.run(q as u32 + 1, 0);
            q += 1;
        }
        !t.dry_ok() && t.seal().err() == Some(DR_ORDER)
    };
    s.add("J9-缺步不封", j9, "步数不达锚即不得封定");

    // J10 容量边界
    let j10 = {
        let mut t = DryRun::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            if t.plan(n, "填步").is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.plan(CAP as u32 + 1, "超").err() == Some(DR_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原账不损");

    // J11 期望步号推进核
    let j11 = {
        let mut t = DryRun::new();
        let e0 = t.expect_seq();
        let _ = t.plan(1, "甲");
        let e1 = t.expect_seq();
        let _ = t.plan(2, "乙");
        e0 == 1 && e1 == 2 && t.expect_seq() == 3
    };
    s.add("J11-期望推进", j11, "期望步号随登记推进");

    // J12 动态明细：未跑计数落账
    let un = d.unrun().len();
    s.add_dyn("J12-未跑计数", un == 0, &format!("unrun={}", un));

    // J13 动态明细：干跑指纹落账
    let fp = d.fold();
    s.add_dyn("J13-干跑指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10330_gate_dry_flow() {
        let mut d = DryRun::new();
        assert!(d.plan(2, "跳").err() == Some(DR_ORDER));
        assert!(d.plan(1, "甲").is_ok());
        assert!(d.plan(1, "重").err() == Some(DR_DUP));
        assert!(d.plan(2, "").err() == Some(DR_EMPTY));
        assert!(d.run(9, 0).err() == Some(DR_NOSTEP));
        assert!(d.run(2, 0).err() == Some(DR_ORDER));
        assert!(d.run(1, 30).is_err());
        assert!(d.blocked_steps().contains(&1));
        assert!(d.run(1, 30).err() == Some(DR_RERUN));
        // 挂起步允许以零刻重试？不许：已跑再跑即拒，须重开账
        let mut d2 = DryRun::new();
        let _ = d2.plan(1, "甲");
        assert!(d2.run(1, 0).is_ok());
        assert!(d2.run(1, 0).err() == Some(DR_RERUN));
        assert!(d2.next_run_seq() == 0 || d2.unrun().is_empty());
    }

    #[test]
    fn unx10330_checks_green() {
        let s = run_unx10330_checks();
        assert!(s.all_passed(), "UNX-F10330 判据未全绿");
    }
}
