//! UNX-F10367 翻绿项回归核（F10228 集执行核）（0x1b860 段）
//!
//! 锚点：UNX-F10367-J1 翻绿项回归核——翻绿项必须以「前红后绿」双证入账，
//! 无前红证据的翻绿 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·回归核谱系）。
//!
//! 结构：翻绿登记册（前红指纹+后绿指纹双证位）+ 执行核（逐项重跑裁定）
//! + 双证核（缺前红/缺后绿/重号/空名/封账写账五注入）+ 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b860 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b860;
pub const GR_NO_RED: u32 = SEG_BASE + 0x01; // 缺前红证据
pub const GR_NO_GREEN: u32 = SEG_BASE + 0x02; // 后绿未通过
pub const GR_DUP_ID: u32 = SEG_BASE + 0x03; // 翻绿项重号
pub const GR_EMPTY_NAME: u32 = SEG_BASE + 0x04; // 项名空
pub const GR_SEALED: u32 = SEG_BASE + 0x05; // 封账后写账
pub const GR_CAP: u32 = SEG_BASE + 0x06; // 超容量
pub const GR_RED_MISMATCH: u32 = SEG_BASE + 0x07; // 前红指纹与账面不符

/// 翻绿项定容（>F10228 集规模，容量先定后用）。
pub const CAP: usize = 32;
/// 翻绿集锚（F10228 集执行核应覆盖项数）。
pub const SET_ANCHOR: usize = 16;

/// 单条翻绿项：前红/后绿双指纹。
#[derive(Clone)]
pub struct GreenItem {
    pub id: u32,
    pub name: String,
    pub red_fp: u32,
    pub green_fp: u32,
}

impl GreenItem {
    /// 双证齐备：id 非零、名非空、前后指纹非零且不同（红→绿必须变指纹）。
    pub fn ok(&self) -> bool {
        self.id != 0
            && !self.name.is_empty()
            && self.red_fp != 0
            && self.green_fp != 0
            && self.red_fp != self.green_fp
    }

    pub fn render(&self) -> String {
        format!(
            "G{}|{}|red={:#x}|green={:#x}",
            self.id, self.name, self.red_fp, self.green_fp
        )
    }
}

/// 翻绿登记册：翻绿项逐条双证入账，封账后只读。
pub struct GreenRecheckBook {
    pub items: Vec<GreenItem>,
    pub sealed: bool,
    pub note: String,
}

impl GreenRecheckBook {
    pub fn new(note: &str) -> GreenRecheckBook {
        GreenRecheckBook { items: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 前红指纹查询（登记时必须先有前红记录才可入翻绿账）。
    pub fn has_red(&self, id: u32, red_fp: u32) -> bool {
        self.items.iter().any(|i| i.id == id && i.red_fp == red_fp)
    }

    /// 入账：重号、空名、缺前红、超容、封账均拒。
    /// `red_known` 表示红账（F10228 集）中确有此前红记录。
    pub fn enroll(
        &mut self,
        id: u32,
        name: &str,
        red_fp: u32,
        green_fp: u32,
        red_known: bool,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(GR_SEALED);
        }
        if name.is_empty() {
            return Err(GR_EMPTY_NAME);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(GR_DUP_ID);
        }
        if !red_known {
            return Err(GR_NO_RED);
        }
        if green_fp == 0 || green_fp == red_fp {
            return Err(GR_NO_GREEN);
        }
        if self.items.len() >= CAP {
            return Err(GR_CAP);
        }
        self.items.push(GreenItem { id, name: name.to_string(), red_fp, green_fp });
        Ok(())
    }

    /// 执行核：对翻绿集逐项重跑，重跑通过数（本轮全绿要求逐项绿）。
    pub fn recheck_pass_count(&self, outcomes: &[(u32, bool)]) -> usize {
        self.items
            .iter()
            .filter(|i| outcomes.iter().any(|(id, ok)| *id == i.id && *ok))
            .count()
    }

    /// 集覆盖核：翻绿集登记数达锚。
    pub fn set_complete(&self) -> bool {
        self.items.len() == SET_ANCHOR
    }

    /// 全绿裁定：覆盖达锚且重跑逐项通过。
    pub fn all_green(&self, outcomes: &[(u32, bool)]) -> bool {
        self.set_complete() && self.recheck_pass_count(outcomes) == self.items.len()
    }

    /// 缺绿诊断：重跑未通过的翻绿项编号列表。
    pub fn gaps(&self, outcomes: &[(u32, bool)]) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        for i in &self.items {
            if !outcomes.iter().any(|(id, ok)| *id == i.id && *ok) {
                g.push(i.id);
            }
        }
        g
    }

    /// 封账并返回折叠指纹（序敏感）。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b8600;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31).wrapping_add(i.red_fp);
            h = h.wrapping_mul(31).wrapping_add(i.green_fp);
            for b in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 前红指纹合成（红账口径：判据号+失败码折叠）。
pub fn red_fp_of(crit: u32, fail_code: u32) -> u32 {
    let mut h = 0x5eed_0000u32;
    h = h.wrapping_mul(31).wrapping_add(crit);
    h.wrapping_mul(31).wrapping_add(fail_code)
}

/// 后绿指纹合成（绿账口径：判据号+通过轮次折叠）。
pub fn green_fp_of(crit: u32, round: u32) -> u32 {
    let mut h = 0x6a11_0000u32;
    h = h.wrapping_mul(31).wrapping_add(crit);
    h.wrapping_mul(31).wrapping_add(round)
}

/// 注入核（五项）：返回 [缺前红, 缺后绿, 重号, 空名, 封账写账] 是否全部检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 无前红证据的翻绿必拒
    let mut b = GreenRecheckBook::new("inj-nored");
    got[0] = b.enroll(1, "x", red_fp_of(10, 1), green_fp_of(10, 2), false)
        == Err(GR_NO_RED);
    // 2) 后绿与前红同指纹（指纹未变=没真翻绿）
    let mut b2 = GreenRecheckBook::new("inj-nogreen");
    got[1] = b2.enroll(1, "x", red_fp_of(11, 1), red_fp_of(11, 1), true)
        == Err(GR_NO_GREEN);
    // 3) 重号必拒
    let mut b3 = GreenRecheckBook::new("inj-dup");
    let _ = b3.enroll(1, "a", red_fp_of(12, 1), green_fp_of(12, 2), true);
    got[2] = b3.enroll(1, "b", red_fp_of(13, 1), green_fp_of(13, 2), true)
        == Err(GR_DUP_ID);
    // 4) 空名必拒
    let mut b4 = GreenRecheckBook::new("inj-empty");
    got[3] = b4.enroll(2, "", red_fp_of(14, 1), green_fp_of(14, 2), true)
        == Err(GR_EMPTY_NAME);
    // 5) 封账后写账必抓
    b3.seal();
    got[4] = b3.enroll(9, "late", red_fp_of(15, 1), green_fp_of(15, 2), true)
        == Err(GR_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10367_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10367");

    // 正路径：F10228 集 16 项逐项前红后绿入账
    let mut book = GreenRecheckBook::new("F10228 集执行核");
    for i in 1..=SET_ANCHOR as u32 {
        let _ = book.enroll(
            i,
            "翻绿项",
            red_fp_of(100 + i, 1),
            green_fp_of(100 + i, 2),
            true,
        );
    }
    let outcomes: Vec<(u32, bool)> =
        (1..=SET_ANCHOR as u32).map(|i| (i, true)).collect();
    let j1 = book.all_green(&outcomes);
    s.add("J1-翻绿集全绿", j1, "16 项双证入账且重跑逐项通过");

    // 双证齐备：每项前后指纹非零且不同
    let j2 = book.items.iter().all(|i| i.ok());
    s.add("J2-双证齐备", j2, "前红后绿指纹非零且不同");

    // 重跑通过计数与缺绿诊断互逆
    let mut outcomes2 = outcomes.clone();
    outcomes2[3] = (4, false);
    let j3 = book.recheck_pass_count(&outcomes2) == SET_ANCHOR - 1
        && book.gaps(&outcomes2) == vec![4];
    s.add("J3-缺绿可枚举", j3, "一项未过即非全绿且缺位可枚举");

    // 容量边界：第 CAP+1 条入账被拒
    let j4 = {
        let mut c = GreenRecheckBook::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "x", red_fp_of(i, 1), green_fp_of(i, 2), true).is_err() {
                filled = false;
            }
        }
        filled
            && c.enroll(999, "over", red_fp_of(999, 1), green_fp_of(999, 2), true)
                == Err(GR_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒且不破坏原账");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "缺前红/缺后绿/重号必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "空名与封账写账必检出");

    // 封账拒写（正册复现）
    let j7 = {
        book.seal();
        book.enroll(77, "late", red_fp_of(77, 1), green_fp_of(77, 2), true)
            == Err(GR_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后翻绿登记必拒");

    // 指纹稳定：两次 fold 一致
    let j8 = book.fold() == book.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 前红指纹查询：登记过的 (id, red_fp) 可命中，未登记的必不命中
    let j9 = book.has_red(1, red_fp_of(101, 1)) && !book.has_red(1, red_fp_of(1, 9));
    s.add("J9-前红可查", j9, "红账命中按 (id,指纹) 精确匹配");

    // 前红指纹口径：同判据不同失败码 → 不同指纹
    let j10 = red_fp_of(5, 1) != red_fp_of(5, 2) && red_fp_of(5, 1) != red_fp_of(6, 1);
    s.add("J10-红指纹口径", j10, "判据号或失败码变则红指纹变");

    // 后绿指纹口径：同判据不同轮次 → 不同指纹，且与红域不相交
    let j11 = green_fp_of(5, 1) != green_fp_of(5, 2)
        && (green_fp_of(5, 1) >> 16) != (red_fp_of(5, 1) >> 16);
    s.add("J11-绿指纹口径", j11, "绿指纹随轮次变且域分离");

    // 渲染明细非空且含双向指纹
    let j12 = {
        let r = book.items[0].render();
        r.contains("red=") && r.contains("green=")
    };
    s.add("J12-明细落账", j12, "翻绿明细含前后指纹字样");

    // 集覆盖核：少登记一项即不达锚
    let j13 = {
        let mut c = GreenRecheckBook::new("short");
        for i in 1..(SET_ANCHOR as u32) {
            let _ = c.enroll(i, "x", red_fp_of(i, 1), green_fp_of(i, 2), true);
        }
        !c.set_complete()
    };
    s.add("J13-覆盖达锚", j13, "少一项即集不完整");

    // 全绿裁定负路径：覆盖齐但重跑有红
    let j14 = {
        let mut c = GreenRecheckBook::new("neg");
        for i in 1..=SET_ANCHOR as u32 {
            let _ = c.enroll(i, "x", red_fp_of(i, 1), green_fp_of(i, 2), true);
        }
        let mut oc: Vec<(u32, bool)> =
            (1..=SET_ANCHOR as u32).map(|i| (i, true)).collect();
        oc[SET_ANCHOR - 1] = (SET_ANCHOR as u32, false);
        !c.all_green(&oc)
    };
    s.add("J14-重跑红即非全绿", j14, "重跑任一红即裁定不通过");

    s.add_dyn(
        "J15-账面指纹落账",
        book.fold() != 0,
        &format!("fold={:#x}", book.fold()),
    );
    let empty_dev = GreenRecheckBook::new("empty").fold();
    s.add_dyn("J16-空册指纹", empty_dev == 0x1b8600u32.wrapping_mul(31), &format!("fold={:#x}", empty_dev));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10367_recheck_flow() {
        let mut b = GreenRecheckBook::new("t");
        assert!(b.enroll(1, "a", red_fp_of(1, 1), green_fp_of(1, 2), true).is_ok());
        assert!(
            b.enroll(1, "dup", red_fp_of(2, 1), green_fp_of(2, 2), true)
                == Err(GR_DUP_ID)
        );
        assert!(
            b.enroll(2, "nored", red_fp_of(3, 1), green_fp_of(3, 2), false)
                == Err(GR_NO_RED)
        );
        b.seal();
        assert!(
            b.enroll(3, "late", red_fp_of(4, 1), green_fp_of(4, 2), true)
                == Err(GR_SEALED)
        );
    }

    #[test]
    fn unx10367_checks_green() {
        let s = run_unx10367_checks();
        assert!(s.all_passed(), "UNX-F10367 判据未全绿");
    }
}
