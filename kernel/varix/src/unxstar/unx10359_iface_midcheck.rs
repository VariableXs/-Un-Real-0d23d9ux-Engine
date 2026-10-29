//! UNX-F10359 对接面中检（十八项对平）（0x1b760 段）
//!
//! 锚点：UNX-F10359-J1 对接面中检——十八个对平槽逐槽双侧报数（x 侧/
//! y 侧各报 标识+数值），只许各报一次，双侧标识与数值全同才算该槽对
//! 平，十八槽全对平方算中检通过，半报与错平在封账时分别分诊。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：对平栅（十八槽，槽=双侧标识+双侧数值）+ 重报核（一侧一报）+
//! 同值核（标识/数值双同）+ 分诊核（半报/错平分诊）+ 注入核（重报/
//! 零值/坏槽/错平四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b760 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b760;
pub const IM_SLOT: u32 = SEG_BASE + 0x01; // 槽号越界
pub const IM_REX: u32 = SEG_BASE + 0x02; // x 侧重报
pub const IM_REY: u32 = SEG_BASE + 0x03; // y 侧重报
pub const IM_ZERO: u32 = SEG_BASE + 0x04; // 零值报数
pub const IM_ID: u32 = SEG_BASE + 0x05; // 标识/数值错平
pub const IM_PART: u32 = SEG_BASE + 0x06; // 半报未齐
pub const IM_NOTREADY: u32 = SEG_BASE + 0x07; // 栅未建齐
pub const IM_SEALED: u32 = SEG_BASE + 0x08; // 封账后再报

/// 槽锚（十八项对平）。
pub const SLOTS_NEEDED: usize = 18;
/// 槽号界。
pub const SLOT_LO: usize = 1;
pub const SLOT_HI: usize = 18;

/// 对平槽：双侧标识 + 双侧数值。
#[derive(Clone)]
pub struct MidPair {
    pub slot: usize,
    pub x_id: Option<u32>,
    pub y_id: Option<u32>,
    pub dx: Option<u32>,
    pub dy: Option<u32>,
}

impl MidPair {
    /// 双侧已报？
    pub fn both(&self) -> bool {
        self.x_id.is_some() && self.y_id.is_some()
    }

    /// 该槽对平（双侧报齐且标识数值全同）。
    pub fn matched(&self) -> bool {
        self.both()
            && self.x_id == self.y_id
            && self.dx.is_some()
            && self.dx == self.dy
    }
}

/// 对接面中检栅。
#[derive(Clone)]
pub struct IfaceMidCheck {
    pub pairs: Vec<MidPair>,
    pub sealed: bool,
}

impl IfaceMidCheck {
    /// 建栅：十八槽一次建齐。
    pub fn new() -> Self {
        let mut pairs = Vec::new();
        let mut s = SLOT_LO;
        while s <= SLOT_HI {
            pairs.push(MidPair {
                slot: s,
                x_id: None,
                y_id: None,
                dx: None,
                dy: None,
            });
            s += 1;
        }
        IfaceMidCheck { pairs, sealed: false }
    }

    /// 槽下标。
    fn idx(&self, slot: usize) -> Result<usize, u32> {
        if slot < SLOT_LO || slot > SLOT_HI || slot > self.pairs.len() {
            return Err(IM_SLOT);
        }
        Ok(slot - 1)
    }

    /// x 侧报数（一次为限）。
    pub fn report_x(&mut self, slot: usize, id: u32, d: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IM_SEALED);
        }
        let i = self.idx(slot)?;
        if d == 0 {
            return Err(IM_ZERO);
        }
        if self.pairs[i].x_id.is_some() {
            return Err(IM_REX);
        }
        self.pairs[i].x_id = Some(id);
        self.pairs[i].dx = Some(d);
        Ok(())
    }

    /// y 侧报数（一次为限）。
    pub fn report_y(&mut self, slot: usize, id: u32, d: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IM_SEALED);
        }
        let i = self.idx(slot)?;
        if d == 0 {
            return Err(IM_ZERO);
        }
        if self.pairs[i].y_id.is_some() {
            return Err(IM_REY);
        }
        self.pairs[i].y_id = Some(id);
        self.pairs[i].dy = Some(d);
        Ok(())
    }

    /// 对平槽数。
    pub fn matched_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.pairs.len() {
            if self.pairs[i].matched() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 错平槽号清单（双侧报齐但不平）。
    pub fn mismatched(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.pairs.len() {
            if self.pairs[i].both() && !self.pairs[i].matched() {
                out.push(self.pairs[i].slot);
            }
            i += 1;
        }
        out
    }

    /// 半报槽号清单（只报一侧）。
    pub fn half_reported(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.pairs.len() {
            let p = &self.pairs[i];
            if p.x_id.is_some() != p.y_id.is_some() {
                out.push(p.slot);
            }
            i += 1;
        }
        out
    }

    /// 十八槽全对平。
    pub fn flat(&self) -> bool {
        self.matched_count() == SLOTS_NEEDED
    }

    /// 封账：半报/错平分诊，不平即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(IM_SEALED);
        }
        if self.pairs.len() < SLOTS_NEEDED {
            return Err(IM_NOTREADY);
        }
        if !self.half_reported().is_empty() {
            return Err(IM_PART);
        }
        if !self.mismatched().is_empty() {
            return Err(IM_ID);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：槽号/双值逐槽叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b7601b7;
        let mut i = 0;
        while i < self.pairs.len() {
            let p = &self.pairs[i];
            h = (h ^ (p.slot as u32)).wrapping_mul(0x01000193);
            h = (h ^ p.dx.unwrap_or(0)).wrapping_mul(0x01000193);
            h = (h ^ p.dy.unwrap_or(0)).wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }
}

/// 对平报数样例：槽 n → 标识 3300+n，值 = n*7+3。
pub fn pair_value(slot: usize) -> u32 {
    slot as u32 * 7 + 3
}

/// 注入核：四路坏报必抓。
/// 0: x 侧重报；1: 零值报数；2: 坏槽号；3: 错平封账拒。
pub fn inject_four() -> [bool; 4] {
    let mut g = IfaceMidCheck::new();
    let mut s = SLOT_LO;
    while s <= SLOT_HI {
        let d = pair_value(s);
        let _ = g.report_x(s, 3300 + s as u32, d);
        let _ = g.report_y(s, 3300 + s as u32, d);
        s += 1;
    }
    let d0 = g.report_x(1, 1, 1).err() == Some(IM_REX);
    let d1 = {
        let mut z = IfaceMidCheck::new();
        z.report_x(2, 3302, 0).err() == Some(IM_ZERO)
    };
    let d2 = g.report_y(19, 1, 1).err() == Some(IM_SLOT);
    let d3 = {
        let mut bad = IfaceMidCheck::new();
        let _ = bad.report_x(5, 3305, 11);
        let _ = bad.report_y(5, 3305, 12);
        bad.seal().err() == Some(IM_ID)
    };
    [d0, d1, d2, d3]
}

/// 判据：run_unx10359_checks。
pub fn run_unx10359_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10359");
    // J1 正路径：建栅十八槽。
    let mut g = IfaceMidCheck::new();
    let j1 = g.pairs.len() == SLOTS_NEEDED;
    s.add("J1-建栅十八", j1, "十八对平槽一次建齐");
    // J2 正路径：十八槽双侧对平报数。
    let mut ok2 = true;
    let mut sn = SLOT_LO;
    while sn <= SLOT_HI {
        let d = pair_value(sn);
        if g.report_x(sn, 3300 + sn as u32, d).is_err()
            || g.report_y(sn, 3300 + sn as u32, d).is_err()
        {
            ok2 = false;
        }
        sn += 1;
    }
    s.add("J2-双报对平", ok2 && g.flat(), "十八槽双侧全平");
    // J3 注入：x 侧重报必抓。
    let mut g3 = IfaceMidCheck::new();
    let _ = g3.report_x(1, 3301, 10);
    let j3 = g3.report_x(1, 3301, 11).err() == Some(IM_REX);
    s.add("J3-重报抓", j3, "x 侧两报即 IM_REX");
    // J4 注入：y 侧重报必抓。
    let mut g4 = IfaceMidCheck::new();
    let _ = g4.report_y(2, 3302, 17);
    let j4 = g4.report_y(2, 3302, 18).err() == Some(IM_REY);
    s.add("J4-重y抓", j4, "y 侧两报即 IM_REY");
    // J5 注入：零值报数必抓。
    let mut g5 = IfaceMidCheck::new();
    let j5 = g5.report_x(3, 3303, 0).err() == Some(IM_ZERO)
        && g5.report_y(3, 3303, 0).err() == Some(IM_ZERO);
    s.add("J5-零值抓", j5, "零值即 IM_ZERO");
    // J6 边界：坏槽号拒。
    let mut g6 = IfaceMidCheck::new();
    let j6 = g6.report_x(0, 1, 1).err() == Some(IM_SLOT)
        && g6.report_y(19, 1, 1).err() == Some(IM_SLOT);
    s.add("J6-坏槽拒", j6, "槽号越界即 IM_SLOT");
    // J7 注入：标识错平必抓。
    let mut g7 = IfaceMidCheck::new();
    let _ = g7.report_x(5, 3305, 42);
    let _ = g7.report_y(5, 3405, 42);
    let j7 = g7.mismatched() == vec![5];
    s.add("J7-标识错", j7, "标识不同即错平");
    // J8 注入：数值错平必抓。
    let mut g8 = IfaceMidCheck::new();
    let _ = g8.report_x(6, 3306, 11);
    let _ = g8.report_y(6, 3306, 12);
    let j8 = g8.mismatched() == vec![6];
    s.add("J8-数值错", j8, "数值不同即错平");
    // J9 边界：半报封账拒。
    let mut g9 = IfaceMidCheck::new();
    let mut s9 = SLOT_LO;
    while s9 <= SLOT_HI {
        let d = pair_value(s9);
        if s9 <= 17 {
            let _ = g9.report_x(s9, 3300 + s9 as u32, d);
            let _ = g9.report_y(s9, 3300 + s9 as u32, d);
        } else {
            let _ = g9.report_x(s9, 3300 + s9 as u32, d);
        }
        s9 += 1;
    }
    let j9 = g9.seal().err() == Some(IM_PART) && g9.half_reported() == vec![18];
    s.add("J9-半报拒", j9, "槽 18 半报即 IM_PART");
    // J10 边界：错平封账拒。
    let mut g10 = IfaceMidCheck::new();
    let mut s10 = SLOT_LO;
    while s10 <= SLOT_HI {
        let d = pair_value(s10);
        let dmod = if s10 == 9 { d + 1 } else { d };
        let _ = g10.report_x(s10, 3300 + s10 as u32, d);
        let _ = g10.report_y(s10, 3300 + s10 as u32, dmod);
        s10 += 1;
    }
    let j10 = g10.seal().err() == Some(IM_ID) && g10.mismatched() == vec![9];
    s.add("J10-错封拒", j10, "槽 9 错平即 IM_ID");
    // J11 正路径：对平后封账。
    let mut g11 = g.clone();
    let j11 = g11.seal().is_ok() && g11.sealed;
    s.add("J11-封账成", j11, "十八槽全平封账");
    // J12 边界：封账后再报拒。
    let mut g12 = g.clone();
    let _ = g12.seal();
    let j12 = g12.report_x(1, 1, 1).err() == Some(IM_SEALED);
    s.add("J12-封后报拒", j12, "封账后报数即 IM_SEALED");
    // J13 动态：对平出数与指纹。
    let j13 = g.matched_count() == 18 && g.fold() != 0;
    s.add_dyn(
        "J13-对平出数",
        j13,
        &format!(
            "对平 {}/{}，槽 18 值 {}，指纹 {:08x}",
            g.matched_count(),
            SLOTS_NEEDED,
            pair_value(18),
            g.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "重报/零值/坏槽/错平四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut g = IfaceMidCheck::new();
        assert_eq!(g.pairs.len(), 18);
        let mut sn = SLOT_LO;
        while sn <= SLOT_HI {
            let d = pair_value(sn);
            assert!(g.report_x(sn, 3300 + sn as u32, d).is_ok());
            assert!(g.report_y(sn, 3300 + sn as u32, d).is_ok());
            sn += 1;
        }
        assert!(g.flat());
        assert!(g.mismatched().is_empty());
        assert!(g.seal().is_ok());
        assert!(g.report_x(1, 1, 1).err() == Some(IM_SEALED));
        assert!(g.fold() != 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10359_checks();
        assert!(s.all_passed());
    }
}
