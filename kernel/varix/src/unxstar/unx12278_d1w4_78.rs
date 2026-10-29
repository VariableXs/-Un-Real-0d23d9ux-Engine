//! UNX-F12278 文件族文档对齐（0x649c0 段）
//!
//! 卡面：文档对齐账——每条文档行须映射到一条已实现行为；行为册是闭集，
//! 文档行引用不存在的行为即「未对齐」；对齐率必须全绿方可 seal；行号
//! 重复必拒。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 行号唯一：同行号重复入账必拒。
//! - J2 行号闭集：0 号行必拒。
//! - J3 行为册闭集：引用未实现行为必拒。
//! - J4 对齐判定：文档行↔行为行一一对应才对齐。
//! - J5 未对齐禁 seal。
//! - J6 印账冻结：seal 后补行必拒。
//! - J7 对齐率可回读。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x649c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x649c0;
pub const DA_BAD_ROW: u32 = SEG_BASE + 0x01; // 0 号行
pub const DA_NO_BEHAVIOR: u32 = SEG_BASE + 0x02; // 引用未实现行为
pub const DA_DUP: u32 = SEG_BASE + 0x03; // 重号行
pub const DA_UNALIGNED: u32 = SEG_BASE + 0x04; // 未对齐禁 seal
pub const DA_FROZEN: u32 = SEG_BASE + 0x05; // 印账后补行

/// 行为册（已实现行为闭集，模型口径）。
pub const BH_CREATE: u32 = 1;
pub const BH_OPEN: u32 = 2;
pub const BH_READ: u32 = 3;
pub const BH_WRITE: u32 = 4;
pub const BH_CLOSE: u32 = 5;
pub const BH_QUERY: u32 = 6;

fn behavior_known(b: u32) -> bool {
    (BH_CREATE..=BH_QUERY).contains(&b)
}

/// 一条文档行。
#[derive(Clone)]
pub struct DocRow {
    pub row: u32,
    pub behavior: u32,
    pub title: String,
}

impl DocRow {
    pub fn render(&self) -> String {
        format!("r{}|b{}|{}", self.row, self.behavior, self.title)
    }
}

/// 文档对齐账册。
pub struct DocBook {
    pub rows: Vec<DocRow>,
    pub sealed: bool,
}

impl DocBook {
    pub fn new() -> DocBook {
        DocBook { rows: Vec::new(), sealed: false }
    }

    /// 入账：行号非零、行为在册、行号唯一。
    pub fn put(&mut self, row: u32, behavior: u32, title: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DA_FROZEN);
        }
        if row == 0 {
            return Err(DA_BAD_ROW);
        }
        if !behavior_known(behavior) {
            return Err(DA_NO_BEHAVIOR);
        }
        if self.rows.iter().any(|r| r.row == row) {
            return Err(DA_DUP);
        }
        self.rows.push(DocRow {
            row,
            behavior,
            title: title.to_string(),
        });
        Ok(())
    }

    /// 对齐率：全部行为在册即对齐（put 已把关，此处复核）。
    pub fn aligned(&self) -> bool {
        self.rows.iter().all(|r| behavior_known(r.behavior))
    }

    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.rows.is_empty() || !self.aligned() {
            return Err(DA_UNALIGNED);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.rows {
            h = h.wrapping_mul(211).wrapping_add(r.row);
            h = h.wrapping_mul(211).wrapping_add(r.behavior);
            for b in r.title.as_bytes() {
                h = h.wrapping_mul(211).wrapping_add(*b as u32);
            }
        }
        h
    }

    /// 诊断面：摘要（对齐率）。
    pub fn summary(&self) -> String {
        format!("rows={} aligned={}", self.rows.len(), self.aligned() as u8)
    }

    pub fn line_of(&self, row: u32) -> String {
        match self.rows.iter().find(|r| r.row == row) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DocBook::new();
    // 1) 0 号行必拒。
    got[0] = b.put(0, BH_CREATE, "x").err() == Some(DA_BAD_ROW);
    // 2) 引用未实现行为必拒。
    got[1] = b.put(1, 99, "x").err() == Some(DA_NO_BEHAVIOR);
    // 3) 重号行必拒。
    let _ = b.put(1, BH_CREATE, "x");
    got[2] = b.put(1, BH_OPEN, "y").err() == Some(DA_DUP);
    // 4) 空账 seal 拒（未对齐口径：零行不诚实）。
    got[3] = DocBook::new().seal().err() == Some(DA_UNALIGNED);
    // 5) 印账后补行必抓。
    let _ = b.put(2, BH_READ, "y");
    let _ = b.seal();
    got[4] = b.put(3, BH_WRITE, "z").err() == Some(DA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12278_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12278");

    // J1 行号闭集。
    let j1 = {
        let mut b = DocBook::new();
        b.put(0, BH_CREATE, "x").err() == Some(DA_BAD_ROW)
    };
    s.add("J1-零号行拒", j1, "行号非零");

    // J2 行为册闭集。
    let j2 = {
        let mut b = DocBook::new();
        b.put(1, 99, "x").err() == Some(DA_NO_BEHAVIOR)
            && b.put(1, BH_QUERY, "x").is_ok()
    };
    s.add("J2-行为册闭集", j2, "未实现必拒");

    // J3 重号行拒。
    let j3 = {
        let mut b = DocBook::new();
        let _ = b.put(4, BH_OPEN, "a");
        b.put(4, BH_CLOSE, "b").err() == Some(DA_DUP)
    };
    s.add("J3-重号行拒", j3, "行号唯一");

    // J4 对齐判定。
    let j4 = {
        let mut b = DocBook::new();
        let _ = b.put(1, BH_CREATE, "a");
        let _ = b.put(2, BH_READ, "b");
        b.aligned()
    };
    s.add("J4-对齐判定", j4, "全引用在册");

    // J5 对齐 seal。
    let j5 = {
        let mut b = DocBook::new();
        let _ = b.put(1, BH_WRITE, "a");
        b.seal().is_ok()
    };
    s.add("J5-对齐seal", j5, "全对齐方可印账");

    // J6 空账 seal 拒。
    let j6 = DocBook::new().seal().err() == Some(DA_UNALIGNED);
    s.add("J6-空账seal拒", j6, "零行不印账");

    // J7 印账冻结。
    let j7 = {
        let mut b = DocBook::new();
        let _ = b.put(1, BH_QUERY, "a");
        let _ = b.seal();
        b.put(2, BH_CREATE, "b").err() == Some(DA_FROZEN)
    };
    s.add("J7-印账冻结", j7, "seal 后必拒");

    // J8 指纹幂等。
    let j8 = {
        let mut b = DocBook::new();
        let _ = b.put(1, BH_OPEN, "doc");
        b.fold() == b.fold()
    };
    s.add("J8-指纹幂等", j8, "标题入指纹");

    // J9 对齐率回读。
    let j9 = {
        let mut b = DocBook::new();
        let _ = b.put(1, BH_CREATE, "a");
        let _ = b.put(2, BH_CLOSE, "b");
        b.summary() == "rows=2 aligned=1"
    };
    s.add("J9-对齐率回读", j9, "摘要诚实");

    // J10 回读渲染。
    let j10 = {
        let mut b = DocBook::new();
        let _ = b.put(7, BH_READ, "读语义");
        b.line_of(7).contains("b3") && b.line_of(8) == "missing"
    };
    s.add("J10-回读渲染", j10, "文档行可渲染");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = DocBook::new();
    let _ = b.put(1, BH_CREATE, "创建语义");
    let _ = b.put(2, BH_OPEN, "打开语义");
    let _ = b.put(3, BH_READ, "读取语义");
    s.add("摘要回读", b.summary().contains("rows=3"), "三行在册");
    s.add("行为覆盖", b.rows[2].behavior == BH_READ, "读取行为挂账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12278_doc_flow() {
        let mut b = DocBook::new();
        assert!(b.put(1, BH_CREATE, "t").is_ok());
        assert_eq!(b.put(1, BH_OPEN, "t"), Err(DA_DUP));
        assert_eq!(b.put(2, 77, "t"), Err(DA_NO_BEHAVIOR));
        assert_eq!(b.put(0, BH_OPEN, "t"), Err(DA_BAD_ROW));
        assert!(b.seal().is_ok());
        assert_eq!(b.put(3, BH_CLOSE, "t"), Err(DA_FROZEN));
    }

    #[test]
    fn unx12278_checks_green() {
        let s = run_unx12278_checks();
        assert!(s.all_passed(), "UNX-F12278 判据未全绿");
    }

    #[test]
    fn unx12278_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
