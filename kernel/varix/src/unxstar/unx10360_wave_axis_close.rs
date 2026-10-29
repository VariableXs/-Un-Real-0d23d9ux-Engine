//! UNX-F10360 wave-06 对接轴闭账（十九项对平+哈希冻结）（0x1b780 段）
//!
//! 锚点：UNX-F10360-J1 wave-06 对接轴闭账——十九项按四维划席（清点 5/
//! 核对 5/会签 4/联排 5），逐项 a/b 双侧报值各一次，双侧同值才算对平
//! 平账，四维席数各齐且十九项全对平后算 u64 轴根并冻结，冻结后再报
//! 一律拒绝。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：轴册（项=号+维+a 值+b 值）+ 席位核（四维配额 5/5/4/5）+
//! 对平核（双报同值）+ 冻结核（轴根 u64 一冻定音）+ 注入核（重号/
//! 重报/零值/带偏冻结四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b780 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b780;
pub const WV_NO: u32 = SEG_BASE + 0x01; // 项号越界
pub const WV_DUP: u32 = SEG_BASE + 0x02; // 项重录
pub const WV_ZERO: u32 = SEG_BASE + 0x03; // 零值报数
pub const WV_REA: u32 = SEG_BASE + 0x04; // a 侧重报
pub const WV_REB: u32 = SEG_BASE + 0x05; // b 侧重报
pub const WV_NOITEM: u32 = SEG_BASE + 0x06; // 项未在册
pub const WV_OPEN: u32 = SEG_BASE + 0x07; // 未对平冻结
pub const WV_SEALED: u32 = SEG_BASE + 0x08; // 冻结后再报

/// 四维。
pub const DIM_CLEAR: u8 = 1;
pub const DIM_CHECK: u8 = 2;
pub const DIM_COSIGN: u8 = 3;
pub const DIM_REHEARSE: u8 = 4;

/// 四维配额（清点 5 / 核对 5 / 会签 4 / 联排 5，共 19）。
pub const QUOTAS: [usize; 4] = [5, 5, 4, 5];
/// 项数锚（十九项）。
pub const ITEMS_NEEDED: usize = 19;
/// 册定容。
pub const CAP: usize = 24;

/// 维号推算：1..=5 清点，6..=10 核对，11..=14 会签，15..=19 联排。
pub fn dim_for(no: u32) -> u8 {
    if no >= 1 && no <= 5 {
        DIM_CLEAR
    } else if no >= 6 && no <= 10 {
        DIM_CHECK
    } else if no >= 11 && no <= 14 {
        DIM_COSIGN
    } else if no >= 15 && no <= 19 {
        DIM_REHEARSE
    } else {
        0
    }
}

/// 轴行：项号 + 维 + 双侧值。
#[derive(Clone)]
pub struct AxisRow {
    pub no: u32,
    pub dim: u8,
    pub va: Option<u32>,
    pub vb: Option<u32>,
}

impl AxisRow {
    /// 双侧已报？
    pub fn both(&self) -> bool {
        self.va.is_some() && self.vb.is_some()
    }

    /// 该项对平。
    pub fn settled(&self) -> bool {
        self.both() && self.va == self.vb
    }
}

/// 对接轴闭账册。
#[derive(Clone)]
pub struct WaveAxisClose {
    pub rows: Vec<AxisRow>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl WaveAxisClose {
    /// 空册。
    pub fn new() -> Self {
        WaveAxisClose {
            rows: Vec::new(),
            frozen: false,
            root: None,
        }
    }

    /// 找项。
    pub fn find(&self, no: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].no == no {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 录项（号定维）。
    pub fn enroll(&mut self, no: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(WV_SEALED);
        }
        if no < 1 || no > 19 {
            return Err(WV_NO);
        }
        if self.rows.len() >= CAP {
            return Err(WV_DUP);
        }
        if self.find(no).is_some() {
            return Err(WV_DUP);
        }
        self.rows.push(AxisRow {
            no,
            dim: dim_for(no),
            va: None,
            vb: None,
        });
        Ok(())
    }

    /// a 侧报值。
    pub fn report_a(&mut self, no: u32, v: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(WV_SEALED);
        }
        let i = match self.find(no) {
            Some(i) => i,
            None => return Err(WV_NOITEM),
        };
        if v == 0 {
            return Err(WV_ZERO);
        }
        if self.rows[i].va.is_some() {
            return Err(WV_REA);
        }
        self.rows[i].va = Some(v);
        Ok(())
    }

    /// b 侧报值。
    pub fn report_b(&mut self, no: u32, v: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(WV_SEALED);
        }
        let i = match self.find(no) {
            Some(i) => i,
            None => return Err(WV_NOITEM),
        };
        if v == 0 {
            return Err(WV_ZERO);
        }
        if self.rows[i].vb.is_some() {
            return Err(WV_REB);
        }
        self.rows[i].vb = Some(v);
        Ok(())
    }

    /// 对平项数。
    pub fn settled_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].settled() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 带偏项号清单（双报但不同值）。
    pub fn deviated(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].both() && !self.rows[i].settled() {
                out.push(self.rows[i].no);
            }
            i += 1;
        }
        out
    }

    /// 某维已录项数。
    pub fn dim_enrolled(&self, dim: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].dim == dim {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某维对平项数。
    pub fn plane_count(&self, dim: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].dim == dim && self.rows[i].settled() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 四维配额全齐（每维录满且对平满）。
    pub fn quotas_ok(&self) -> bool {
        let mut d = 0;
        while d < 4 {
            let dim = d as u8 + 1;
            if self.dim_enrolled(dim) != QUOTAS[d] || self.plane_count(dim) != QUOTAS[d] {
                return false;
            }
            d += 1;
        }
        true
    }

    /// 十九项全对平。
    pub fn all_settled(&self) -> bool {
        self.rows.len() == ITEMS_NEEDED && self.settled_count() == ITEMS_NEEDED
    }

    /// 闭账：全对平且配额齐。
    pub fn closed(&self) -> bool {
        self.all_settled() && self.quotas_ok()
    }

    /// u64 轴根：对平项 (号,值) 依序叠 FNV。
    pub fn axis_root(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.settled() {
                let v = r.va.unwrap_or(0) as u64;
                h = (h ^ (r.no as u64)).wrapping_mul(0x100000001b3);
                h = (h ^ v).wrapping_mul(0x100000001b3);
            }
            i += 1;
        }
        h
    }

    /// 冻结：闭账方可，冻后不可再报。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(WV_SEALED);
        }
        if !self.closed() {
            return Err(WV_OPEN);
        }
        let r = self.axis_root();
        self.root = Some(r);
        self.frozen = true;
        Ok(r)
    }

    /// 折叠指纹 u32。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b7801b7;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = (h ^ r.no).wrapping_mul(0x01000193);
            h = (h ^ (r.dim as u32)).wrapping_mul(0x01000193);
            h = (h ^ r.va.unwrap_or(0)).wrapping_mul(0x01000193);
            h = (h ^ r.vb.unwrap_or(0)).wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }
}

/// 报值样例：项 n → a/b 同值 = n*11+2。
pub fn axis_value(no: u32) -> u32 {
    no * 11 + 2
}

/// 注入核：四路坏账必抓。
/// 0: 项重录；1: a 侧重报；2: 零值；3: 带偏冻结。
pub fn inject_four() -> [bool; 4] {
    let mut a = WaveAxisClose::new();
    let mut n = 1;
    while n <= 19 {
        let _ = a.enroll(n);
        let v = axis_value(n);
        let _ = a.report_a(n, v);
        let _ = a.report_b(n, v);
        n += 1;
    }
    let d0 = a.enroll(7).err() == Some(WV_DUP);
    let d1 = a.report_a(7, 9).err() == Some(WV_REA);
    let d2 = a.report_b(8, 0).err() == Some(WV_ZERO);
    let d3 = {
        let mut bad = WaveAxisClose::new();
        let mut m = 1;
        while m <= 19 {
            let _ = bad.enroll(m);
            let v = axis_value(m);
            let _ = bad.report_a(m, v);
            let vb = if m == 12 { v + 1 } else { v };
            let _ = bad.report_b(m, vb);
            m += 1;
        }
        bad.freeze().err() == Some(WV_OPEN)
    };
    [d0, d1, d2, d3]
}

/// 判据：run_unx10360_checks。
pub fn run_unx10360_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10360");
    // J1 正路径：十九项按号入册。
    let mut a = WaveAxisClose::new();
    let mut j1 = true;
    let mut n = 1;
    while n <= 19 {
        if a.enroll(n).is_err() {
            j1 = false;
        }
        n += 1;
    }
    s.add("J1-十九项录", j1 && a.rows.len() == 19, "十九项全入册");
    // J2 边界：项号越界拒。
    let mut a2 = WaveAxisClose::new();
    let j2 = a2.enroll(0).err() == Some(WV_NO) && a2.enroll(20).err() == Some(WV_NO);
    s.add("J2-越号拒", j2, "号不在 1..=19 即 WV_NO");
    // J3 注入：项重录必抓。
    let mut a3 = WaveAxisClose::new();
    let _ = a3.enroll(3);
    let j3 = a3.enroll(3).err() == Some(WV_DUP);
    s.add("J3-重录抓", j3, "同号两录即 WV_DUP");
    // J4 注入：重报必抓。
    let mut a4 = WaveAxisClose::new();
    let _ = a4.enroll(4);
    let _ = a4.report_a(4, 46);
    let _ = a4.report_b(4, 46);
    let j4 = a4.report_a(4, 47).err() == Some(WV_REA)
        && a4.report_b(4, 47).err() == Some(WV_REB);
    s.add("J4-重报抓", j4, "双侧再报即 WV_REA/WV_REB");
    // J5 注入：零值必抓。
    let mut a5 = WaveAxisClose::new();
    let _ = a5.enroll(5);
    let j5 = a5.report_a(5, 0).err() == Some(WV_ZERO);
    s.add("J5-零值抓", j5, "零值即 WV_ZERO");
    // J6 边界：未在册报值拒。
    let mut a6 = WaveAxisClose::new();
    let j6 = a6.report_a(99, 1).err() == Some(WV_NOITEM);
    s.add("J6-无项拒", j6, "未在册报值即 WV_NOITEM");
    // J7 正路径：十九项双侧同值全对平。
    let mut ok7 = true;
    let mut n7 = 1;
    while n7 <= 19 {
        let v = axis_value(n7);
        if a.report_a(n7, v).is_err() || a.report_b(n7, v).is_err() {
            ok7 = false;
        }
        n7 += 1;
    }
    let j7 = ok7 && a.all_settled() && a.deviated().is_empty();
    s.add("J7-全对平", j7, "十九项双报同值");
    // J8 正路径：四维配额全齐。
    let j8 = a.quotas_ok()
        && a.plane_count(DIM_CLEAR) == 5
        && a.plane_count(DIM_CHECK) == 5
        && a.plane_count(DIM_COSIGN) == 4
        && a.plane_count(DIM_REHEARSE) == 5;
    s.add("J8-配额齐", j8, "5/5/4/5 四维全满");
    // J9 正路径：闭账冻结出根。
    let mut a9 = a.clone();
    let r9 = a9.freeze();
    let j9 = r9.is_ok() && a9.frozen && a9.root.is_some();
    s.add("J9-冻结束", j9, "闭账冻结 u64 轴根");
    // J10 边界：带偏冻结拒。
    let mut a10 = WaveAxisClose::new();
    let mut n10 = 1;
    while n10 <= 19 {
        let _ = a10.enroll(n10);
        let v = axis_value(n10);
        let _ = a10.report_a(n10, v);
        let vb = if n10 == 16 { v + 1 } else { v };
        let _ = a10.report_b(n10, vb);
        n10 += 1;
    }
    let j10 = a10.freeze().err() == Some(WV_OPEN) && a10.deviated() == vec![16];
    s.add("J10-偏冻拒", j10, "项 16 带偏即 WV_OPEN");
    // J11 边界：缺项冻结拒。
    let mut a11 = WaveAxisClose::new();
    let mut n11 = 1;
    while n11 <= 18 {
        let _ = a11.enroll(n11);
        let v = axis_value(n11);
        let _ = a11.report_a(n11, v);
        let _ = a11.report_b(n11, v);
        n11 += 1;
    }
    let j11 = a11.freeze().err() == Some(WV_OPEN);
    s.add("J11-缺冻拒", j11, "十九缺一即 WV_OPEN");
    // J12 注入：冻结后再报拒。
    let mut a12 = a.clone();
    let _ = a12.freeze();
    let j12 = a12.report_a(1, 13).err() == Some(WV_SEALED)
        && a12.enroll(1).err() == Some(WV_SEALED);
    s.add("J12-冻后拒", j12, "冻结后报/录即 WV_SEALED");
    // J13 动态：轴根与指纹。
    let root = a.axis_root();
    let j13 = root != 0 && a.fold() != 0 && a.closed();
    s.add_dyn(
        "J13-根出数",
        j13,
        &format!(
            "十九项对平闭账，轴根 {:016x}，指纹 {:08x}",
            root,
            a.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "重录/重报/零值/偏冻四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_freeze() {
        let mut a = WaveAxisClose::new();
        let mut n = 1;
        while n <= 19 {
            assert!(a.enroll(n).is_ok());
            let v = axis_value(n);
            assert!(a.report_a(n, v).is_ok());
            assert!(a.report_b(n, v).is_ok());
            n += 1;
        }
        assert!(a.closed());
        let r = a.freeze();
        assert!(r.is_ok());
        assert!(a.report_a(1, 13).err() == Some(WV_SEALED));
        assert!(a.fold() != 0);
        assert_eq!(a.plane_count(DIM_COSIGN), 4);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10360_checks();
        assert!(s.all_passed());
    }
}
