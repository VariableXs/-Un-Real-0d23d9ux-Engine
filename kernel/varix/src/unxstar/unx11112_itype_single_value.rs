//! UNX-F11112 I 型总收口断言（六批全绿单值）（0x1f592 段）
//!
//! 锚点：UNX-F11112-J1 六批全绿单值断言成立（10/10），收口账版本冻结
//! （10/10）。
//! 域：CoRun·UNX-C4-B36（I 型收口）。
//!
//! 结构：收口册（TotalClosure：六批绿位 + 批指纹 → 全绿单值 → 版本冻结 →
//! 闭环）+ 批标（BatchMark）+ 冻结根 + 注入核（重号/越段/红批求单值/零版本
//! 四路必抓）。

use crate::alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f592 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f592;
pub const CL_NOMARK: u32 = SEG_BASE + 0x01; // 批标缺失
pub const CL_DUP: u32 = SEG_BASE + 0x02; // 批号重号
pub const CL_RANGE: u32 = SEG_BASE + 0x03; // 批号越段
pub const CL_RED: u32 = SEG_BASE + 0x04; // 有红批
pub const CL_ZERO: u32 = SEG_BASE + 0x05; // 零指纹或零版本
pub const CL_NOTFROZEN: u32 = SEG_BASE + 0x06; // 版本未冻
pub const CL_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const CL_VER: u32 = SEG_BASE + 0x08; // 版本回退

/// 六批区间（B30–B35）。
pub const BATCH_FIRST: u32 = 30;
pub const BATCH_LAST: u32 = 35;
pub const ROWS: usize = 6;
/// 收口账标（fold 种子）。
pub const CL_TAG: u64 = 0x1f5_9292;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单批标：批号 + 绿位 + 批指纹。
#[derive(Clone, Debug, PartialEq)]
pub struct BatchMark {
    pub no: u32,
    pub green: bool,
    pub fp: u64,
}

impl BatchMark {
    pub fn render(&self) -> String {
        format!("B{}|{}|{:#x}", self.no, if self.green { "G" } else { "R" }, self.fp)
    }
}

/// 总收口册：六批标 + 版本 + 冻结根。
#[derive(Clone, Debug)]
pub struct TotalClosure {
    pub marks: Vec<BatchMark>,
    pub version: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl TotalClosure {
    pub fn new() -> TotalClosure {
        TotalClosure { marks: Vec::new(), version: 0, frozen: false, root: None }
    }

    /// 批标登记：越段/重号/零指纹/超六批皆拒，登记后按批号定序。
    pub fn mark(&mut self, no: u32, green: bool, fp: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(CL_FROZEN);
        }
        if no < BATCH_FIRST || no > BATCH_LAST {
            return Err(CL_RANGE);
        }
        if self.marks.iter().any(|m| m.no == no) {
            return Err(CL_DUP);
        }
        if fp == 0 {
            return Err(CL_ZERO);
        }
        if self.marks.len() >= ROWS {
            return Err(CL_NOMARK);
        }
        self.marks.push(BatchMark { no, green, fp });
        self.marks.sort_by_key(|m| m.no);
        Ok(())
    }

    /// 六批全绿：单值布尔（缺批即假）。
    pub fn all_green(&self) -> bool {
        if self.marks.len() != ROWS {
            return false;
        }
        let mut k = 0usize;
        while k < self.marks.len() {
            if !self.marks[k].green {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 全绿单值：六批全绿时把批指纹折成唯一 u64，否则 None。
    pub fn single_value(&self) -> Option<u64> {
        if !self.all_green() {
            return None;
        }
        let mut h: u64 = CL_TAG;
        let mut k = 0usize;
        while k < self.marks.len() {
            h = (h ^ self.marks[k].no as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.marks[k].fp).wrapping_mul(FP_STEP);
            k += 1;
        }
        if h == 0 {
            None
        } else {
            Some(h)
        }
    }

    /// 版本冻结：版本为零拒、单值未成立拒、版本回退拒。
    pub fn freeze_version(&mut self, v: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CL_FROZEN);
        }
        if v == 0 {
            return Err(CL_ZERO);
        }
        if self.single_value().is_none() {
            return Err(CL_RED);
        }
        if v < self.version {
            return Err(CL_VER);
        }
        self.version = v;
        Ok(())
    }

    /// 收口闭合：全绿单值成立且版本已冻。
    pub fn closed(&self) -> bool {
        self.single_value().is_some() && self.version > 0
    }

    /// 红批点名。
    pub fn reds(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.marks.len() {
            if !self.marks[k].green {
                v.push(self.marks[k].no);
            }
            k += 1;
        }
        v
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 册指纹：批标链 + 单值 + 版本。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = CL_TAG;
        let mut k = 0usize;
        while k < self.marks.len() {
            let m = &self.marks[k];
            h = (h ^ m.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ m.green as u64).wrapping_mul(FP_STEP);
            h = (h ^ m.fp).wrapping_mul(FP_STEP);
            k += 1;
        }
        if let Some(sv) = self.single_value() {
            h = (h ^ sv).wrapping_mul(FP_STEP);
        }
        if self.version != 0 {
            h = (h ^ self.version as u64).wrapping_mul(FP_STEP);
        }
        h
    }
}

/// 批指纹样本：批号敏感且非零。
pub fn batch_fp(no: u32) -> u64 {
    0x9e37_79b9u64.wrapping_mul(no as u64).wrapping_add(0x11)
}

/// 批名样本。
pub fn batch_name(no: u32) -> String {
    format!("B{} 收口批", no)
}

/// 标准收口册：六批全绿并冻版本 3。
pub fn build_closure() -> (TotalClosure, bool) {
    let mut c = TotalClosure::new();
    let mut ok = true;
    let mut no = BATCH_FIRST;
    while no <= BATCH_LAST {
        if c.mark(no, true, batch_fp(no)).is_err() {
            ok = false;
        }
        no += 1;
    }
    if c.freeze_version(3).is_err() {
        ok = false;
    }
    (c, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重号必拒
    let mut a = TotalClosure::new();
    let _ = a.mark(BATCH_FIRST, true, batch_fp(BATCH_FIRST));
    got[0] = a.mark(BATCH_FIRST, true, batch_fp(BATCH_FIRST)).err() == Some(CL_DUP);
    // 2) 越段必拒
    let mut b = TotalClosure::new();
    got[1] = b.mark(BATCH_LAST + 1, true, 7).err() == Some(CL_RANGE)
        && b.mark(BATCH_FIRST - 1, true, 7).err() == Some(CL_RANGE);
    // 3) 有红批时单值必无
    let mut c = TotalClosure::new();
    let mut no = BATCH_FIRST;
    while no <= BATCH_LAST {
        let _ = c.mark(no, no != 33, batch_fp(no));
        no += 1;
    }
    got[2] = c.single_value().is_none() && c.reds().len() == 1 && !c.all_green();
    // 4) 零版本必拒
    let mut d = TotalClosure::new();
    let mut no2 = BATCH_FIRST;
    while no2 <= BATCH_LAST {
        let _ = d.mark(no2, true, batch_fp(no2));
        no2 += 1;
    }
    got[3] = d.freeze_version(0).err() == Some(CL_ZERO) && !d.closed();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11112_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11112");

    let (mut c, built) = build_closure();
    // J1 六批全绿单值断言成立
    s.add(
        "J1-全绿单值成立",
        built && c.all_green() && c.single_value().is_some(),
        "六批全绿且单值可算出",
    );

    // J2 收口账版本冻结
    s.add(
        "J2-版本冻结",
        c.version == 3 && c.closed(),
        "版本冻结在 3 且收口闭合",
    );

    // J3 单值对批指纹敏感
    let j3 = {
        let base = c.single_value();
        let mut d = TotalClosure::new();
        let mut no = BATCH_FIRST;
        while no <= BATCH_LAST {
            let fp = if no == 31 { batch_fp(no).wrapping_add(1) } else { batch_fp(no) };
            let _ = d.mark(no, true, fp);
            no += 1;
        }
        let _ = d.freeze_version(3);
        base.is_some() && d.single_value().is_some() && base != d.single_value()
    };
    s.add("J3-单值敏感", j3, "任一批评分变动即单值变动");

    // J4 红批单值为无
    let j4 = {
        let mut d = TotalClosure::new();
        let mut no = BATCH_FIRST;
        while no <= BATCH_LAST {
            let _ = d.mark(no, no != 35, batch_fp(no));
            no += 1;
        }
        d.single_value().is_none() && d.reds().contains(&35) && d.freeze_version(1).err() == Some(CL_RED)
    };
    s.add("J4-红批无单值", j4, "红批存在时单值不可算且版本不得冻");

    // J5 重号拒
    let j5 = {
        let mut d = TotalClosure::new();
        let _ = d.mark(BATCH_FIRST, true, 5);
        d.mark(BATCH_FIRST, false, 6).err() == Some(CL_DUP)
    };
    s.add("J5-重号拒", j5, "同批二次登记必拒");

    // J6 越段拒
    let j6 = {
        let mut d = TotalClosure::new();
        d.mark(BATCH_LAST + 1, true, 5).err() == Some(CL_RANGE)
    };
    s.add("J6-越段拒", j6, "越段批号不得入册");

    // J7 零指纹拒
    let j7 = {
        let mut d = TotalClosure::new();
        d.mark(BATCH_FIRST, true, 0).err() == Some(CL_ZERO) && d.marks.is_empty()
    };
    s.add("J7-零指纹拒", j7, "零批指纹不得入册");

    // J8 版本回退拒
    let j8 = {
        let (mut d, _) = build_closure();
        d.freeze_version(2).err() == Some(CL_VER)
    };
    s.add("J8-版本回退拒", j8, "版本不得低于已冻值");

    // J9 注入检出：重号 + 越段
    let inj = inject_four();
    s.add("J9-重号越段检出", inj[0] && inj[1], "批号重号与越段两路必抓");

    // J10 注入检出：红批 + 零版本
    s.add("J10-红批零版检出", inj[2] && inj[3], "红批无单值与零版本拒冻两路必抓");

    // J11 缺批不算全绿
    let j11 = {
        let mut d = TotalClosure::new();
        let mut no = BATCH_FIRST;
        while no < BATCH_LAST {
            let _ = d.mark(no, true, batch_fp(no));
            no += 1;
        }
        !d.all_green() && d.single_value().is_none()
    };
    s.add("J11-缺批不绿", j11, "少一批即全绿为假");

    // J12 冻结拒写
    let root = c.seal();
    let j12 = c.mark(BATCH_FIRST, true, 1).err() == Some(CL_FROZEN)
        && c.freeze_version(9).err() == Some(CL_FROZEN);
    s.add("J12-冻结拒写", j12, "封存后登记与改版两路必拒");

    // J13 冻结根稳定
    s.add(
        "J13-冻结根稳定",
        c.root == Some(root) && root == c.fold() && root != 0,
        "收口册根可重算零漂移",
    );

    // J14 渲染面
    let j14 = c.marks[0].render().contains("|G|")
        && c.marks[0].render() != c.marks[1].render()
        && batch_name(30).contains("B30");
    s.add("J14-批标渲染", j14, "全绿批标带 G 旗且逐批可区分");

    // J15 动态明细
    s.add_dyn(
        "J15-收口明细",
        c.closed() && c.fold() == root,
        &format!("marks={} ver={} reds={} root={:#x}", c.marks.len(), c.version, c.reds().len(), root),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11112_closure_flow() {
        let (mut c, built) = build_closure();
        assert!(built);
        assert!(c.all_green());
        assert!(c.single_value().is_some());
        assert!(c.closed());
        assert!(c.seal() != 0);
        assert!(c.mark(30, true, 1).err() == Some(CL_FROZEN));
        let mut d = TotalClosure::new();
        assert!(d.mark(30, true, 5).is_ok());
        assert!(d.single_value().is_none());
    }

    #[test]
    fn unx11112_checks_green() {
        let s = run_unx11112_checks();
        assert!(s.all_passed(), "UNX-F11112 判据未全绿");
    }
}
