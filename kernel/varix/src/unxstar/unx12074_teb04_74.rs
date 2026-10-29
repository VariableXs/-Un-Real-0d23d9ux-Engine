//! UNX-F12074 TEB 布局静态断言器（0x19b60 段）
//!
//! 卡面：TEB 字段布局静态断言——字段偏移严格递增、按对齐边界落位、
//! 区间不重叠、总跨度不超出页长；断言器逐字段登记逐条过闸。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 严格递增：新字段偏移必须大于前一字段偏移（前字段尾 ≤ 新字段头）。
//! - J2 对齐落位：字段偏移必须按其对齐数整除。
//! - J3 对齐数合法：对齐数必须为 2 的幂且 ≤ 8。
//! - J4 尺寸合法：字段尺寸非零。
//! - J5 总跨度：尾字段不得超过 TEB 页长。
//! - J6 重叠拒：与前字段区间相交必拒。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：LayoutField 字段档 + LayoutAsserter 逐条过闸 + 折叠指纹 +
//! 注入核（零尺寸/坏对齐数/偏移未对齐/重叠/超页长）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19b60 独占 0x20）。
pub const SEG_BASE: u32 = 0x19b60;
pub const LA_BAD_SIZE: u32 = SEG_BASE + 0x01; // 零尺寸
pub const LA_BAD_ALIGN: u32 = SEG_BASE + 0x02; // 对齐数非法
pub const LA_UNALIGNED: u32 = SEG_BASE + 0x03; // 偏移未对齐
pub const LA_OVERLAP: u32 = SEG_BASE + 0x04; // 区间重叠
pub const LA_EXCEED: u32 = SEG_BASE + 0x05; // 超页长
pub const LA_FROZEN: u32 = SEG_BASE + 0x06; // 印账后登记

/// TEB 布局页长（断言口径 4K）。
pub const TEB_PAGE: u32 = 0x1000;

/// 布局字段档。
#[derive(Clone, Copy)]
pub struct LayoutField {
    pub name_id: u32,
    pub off: u32,
    pub size: u32,
    pub align: u32,
}

impl LayoutField {
    pub fn end(&self) -> u32 {
        self.off + self.size
    }
    /// 单字段静态合法：尺寸非零、对齐数为 2 的幂且 ≤ 8、偏移对齐。
    pub fn self_ok(&self) -> bool {
        self.size != 0
            && self.align >= 1
            && self.align <= 8
            && self.align.is_power_of_two()
            && self.off % self.align == 0
    }
    pub fn render(&self) -> String {
        format!("F{:x}|{:x}+{:x}|a{}", self.name_id, self.off, self.size, self.align)
    }
}

/// 布局断言器：字段序列逐条过闸。
pub struct LayoutAsserter {
    pub fields: Vec<LayoutField>,
    pub page: u32,
    pub sealed: bool,
}

impl LayoutAsserter {
    pub fn new(page: u32) -> LayoutAsserter {
        LayoutAsserter { fields: Vec::new(), page, sealed: false }
    }

    /// 登记字段：单字段合法 → 递增不重叠 → 总跨度。
    pub fn register(&mut self, f: LayoutField) -> Result<(), u32> {
        if self.sealed {
            return Err(LA_FROZEN);
        }
        if f.size == 0 {
            return Err(LA_BAD_SIZE);
        }
        if f.align == 0 || f.align > 8 || !f.align.is_power_of_two() {
            return Err(LA_BAD_ALIGN);
        }
        if f.off % f.align != 0 {
            return Err(LA_UNALIGNED);
        }
        if let Some(prev) = self.fields.last() {
            if f.off < prev.end() {
                return Err(LA_OVERLAP);
            }
        }
        if f.end() > self.page {
            return Err(LA_EXCEED);
        }
        self.fields.push(f);
        Ok(())
    }

    /// 全量静态复核：逐字段自洽且序列严格递增。
    pub fn assert_all(&self) -> bool {
        let mut prev_end: u32 = 0;
        for f in &self.fields {
            if !f.self_ok() || f.off < prev_end || f.end() > self.page {
                return false;
            }
            prev_end = f.end();
        }
        true
    }

    /// 字段数。
    pub fn count(&self) -> usize {
        self.fields.len()
    }

    /// 尾字段结束偏移。
    pub fn high_water(&self) -> u32 {
        self.fields.last().map(|f| f.end()).unwrap_or(0)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for f in &self.fields {
            h = h.wrapping_mul(211).wrapping_add(f.name_id);
            h = h.wrapping_mul(211).wrapping_add(f.off);
            h = h.wrapping_mul(211).wrapping_add(f.size);
            h = h.wrapping_mul(211).wrapping_add(f.align);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("fields={} high={:x}", self.count(), self.high_water())
    }

    /// 诊断面：字段渲染行。
    pub fn field_line(&self, i: usize) -> String {
        match self.fields.get(i) {
            Some(f) => f.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut a = LayoutAsserter::new(TEB_PAGE);
    // 1) 零尺寸必拒。
    got[0] = a.register(LayoutField { name_id: 1, off: 0x00, size: 0, align: 8 })
        .err() == Some(LA_BAD_SIZE);
    // 2) 对齐数非法（3 非 2 的幂）必拒。
    got[1] = a.register(LayoutField { name_id: 1, off: 0x00, size: 8, align: 3 })
        .err() == Some(LA_BAD_ALIGN);
    // 3) 偏移未对齐必拒。
    got[2] = a.register(LayoutField { name_id: 1, off: 0x04, size: 8, align: 8 })
        .err() == Some(LA_UNALIGNED);
    // 4) 重叠必拒：前字段 [0,0x10)，新字段偏移 0x08。
    let _ = a.register(LayoutField { name_id: 1, off: 0x00, size: 0x10, align: 8 });
    got[3] = a.register(LayoutField { name_id: 2, off: 0x08, size: 4, align: 4 })
        .err() == Some(LA_OVERLAP);
    // 5) 印账后登记必抓（另册复核超页长：起点贴前档尾、尾出页）。
    let mut a2 = LayoutAsserter::new(TEB_PAGE);
    let _ = a2.register(LayoutField { name_id: 1, off: 0xff8, size: 8, align: 8 });
    got[4] = a2.register(LayoutField { name_id: 2, off: 0x1000, size: 8, align: 8 })
        .err() == Some(LA_EXCEED);
    a.seal();
    let _ = got;
    got[4] = got[4] && a.register(LayoutField { name_id: 2, off: 0x10, size: 4, align: 4 })
        .err() == Some(LA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12074_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12074");

    // J1 标准布局全量过闸：TIB 头 8 字段模拟。
    let j1 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        let seq: [(u32, u32, u32); 4] = [(0x00, 0x08, 8), (0x08, 0x08, 8), (0x10, 0x04, 4), (0x18, 0x40, 8)];
        let mut ok = true;
        for (i, e) in seq.iter().enumerate() {
            let r = a.register(LayoutField { name_id: i as u32 + 1, off: e.0, size: e.1, align: e.2 });
            ok = ok && r.is_ok();
        }
        ok && a.assert_all() && a.high_water() == 0x58
    };
    s.add("J1-标准布局", j1, "四字段递增过闸");

    // J2 零尺寸拒。
    let j2 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        a.register(LayoutField { name_id: 1, off: 0, size: 0, align: 4 })
            .err() == Some(LA_BAD_SIZE)
    };
    s.add("J2-零尺寸拒", j2, "零尺寸必拒");

    // J3 对齐数非法拒。
    let j3 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        a.register(LayoutField { name_id: 1, off: 0, size: 4, align: 0 })
            .err() == Some(LA_BAD_ALIGN)
            && a.register(LayoutField { name_id: 1, off: 0, size: 4, align: 16 })
                .err() == Some(LA_BAD_ALIGN)
    };
    s.add("J3-对齐数闭集", j3, "非 2 幂与超 8 皆拒");

    // J4 偏移未对齐拒。
    let j4 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        a.register(LayoutField { name_id: 1, off: 0x06, size: 4, align: 4 })
            .err() == Some(LA_UNALIGNED)
    };
    s.add("J4-偏移对齐", j4, "未对齐必拒");

    // J5 重叠拒。
    let j5 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        let _ = a.register(LayoutField { name_id: 1, off: 0x10, size: 0x08, align: 8 });
        a.register(LayoutField { name_id: 2, off: 0x14, size: 4, align: 4 })
            .err() == Some(LA_OVERLAP)
    };
    s.add("J5-重叠拒", j5, "区间相交必拒");

    // J6 超页长拒。
    let j6 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        a.register(LayoutField { name_id: 1, off: 0xff0, size: 0x20, align: 8 })
            .err() == Some(LA_EXCEED)
    };
    s.add("J6-超页拒", j6, "尾出页必拒");

    // J7 恰贴页尾放行。
    let j7 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        a.register(LayoutField { name_id: 1, off: 0xff8, size: 8, align: 8 }).is_ok()
            && a.high_water() == TEB_PAGE
    };
    s.add("J7-贴尾放行", j7, "恰达页长不拒");

    // J8 印账冻结。
    let j8 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        let _ = a.register(LayoutField { name_id: 1, off: 0, size: 8, align: 8 });
        a.seal();
        a.register(LayoutField { name_id: 2, off: 0x10, size: 4, align: 4 })
            .err() == Some(LA_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        let _ = a.register(LayoutField { name_id: 1, off: 0, size: 8, align: 8 });
        a.fold() == a.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut a = LayoutAsserter::new(TEB_PAGE);
    let _ = a.register(LayoutField { name_id: 11, off: 0x00, size: 0x08, align: 8 });
    let _ = a.register(LayoutField { name_id: 12, off: 0x08, size: 0x08, align: 8 });

    s.add("摘要回读", a.summary().contains("high=10"), "高位线 0x10");
    s.add("字段渲染", a.field_line(0).contains("b"), "name_id 11 渲染十六进制 b");
    s.add("全量复核", a.assert_all(), "全字段过静态断言");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12074_register_flow() {
        let mut a = LayoutAsserter::new(TEB_PAGE);
        assert!(a.register(LayoutField { name_id: 1, off: 0x00, size: 8, align: 8 }).is_ok());
        assert_eq!(
            a.register(LayoutField { name_id: 2, off: 0x04, size: 4, align: 4 }),
            Err(LA_OVERLAP)
        );
        assert!(a.register(LayoutField { name_id: 3, off: 0x08, size: 4, align: 4 }).is_ok());
    }

    #[test]
    fn unx12074_checks_green() {
        let s = run_unx12074_checks();
        assert!(s.all_passed(), "UNX-F12074 判据未全绿");
    }

    #[test]
    fn field_self_check() {
        let ok = LayoutField { name_id: 1, off: 0x08, size: 4, align: 4 };
        let bad = LayoutField { name_id: 2, off: 0x06, size: 4, align: 4 };
        assert!(ok.self_ok());
        assert!(!bad.self_ok());
    }
}
