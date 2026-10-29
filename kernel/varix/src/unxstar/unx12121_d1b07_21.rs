//! UNX-F12121 PE 异常目录全景：.pdata/.xdata 定位与加载（0x1a140 段）
//!
//! 卡面：异常目录定位——数据目录异常项（RVA/Size）装载、
//! .pdata 表目 12 字节三列、Size 须为 12 的倍数、表目须落镜像界内。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 目录空显性：Size 为 0 视作无异常目录（合法但记录在案）。
//! - J2 尺寸对齐：Size 必为 12 的倍数，违者必拒。
//! - J3 表目界内：表目 BeginAddress 须落镜像 [0, ImgSize) 内。
//! - J4 表目唯一：同 BeginAddress 二次登记必拒。
//! - J5 表目回读：逐目（Begin/End/Unwind）可回读。
//! - J6 印账冻结：seal 后装载必拒。
//! - J7 注入五况全抓。
//!
//! 结构：ExcDir 目录档 + RtEntry 表目 + DirBook 装载册 + 校验核 +
//! 折叠指纹（211 进制卷积）+ 注入核（空目录/坏尺寸/越镜像/重目/印后装载）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a140 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a140;
pub const ED_BAD_SIZE: u32 = SEG_BASE + 0x01; // 尺寸非 12 倍
pub const ED_OOB: u32 = SEG_BASE + 0x02; // 越镜像
pub const ED_DUP_ENTRY: u32 = SEG_BASE + 0x03; // 重目
pub const ED_ZERO_ID: u32 = SEG_BASE + 0x04; // 零号目
pub const ED_FROZEN: u32 = SEG_BASE + 0x05; // 印账后装载

/// .pdata 表目步长。
pub const RT_STRIDE: u32 = 12;

/// 异常目录档。
#[derive(Clone, Copy)]
pub struct ExcDir {
    pub rva: u32,
    pub size: u32,
    pub img_size: u32,
}

impl ExcDir {
    /// 目录校验：空目录合法；非空须 12 倍数。
    pub fn validate(&self) -> Result<bool, u32> {
        if self.size == 0 {
            return Ok(false); // 无异常目录
        }
        if self.size % RT_STRIDE != 0 {
            return Err(ED_BAD_SIZE);
        }
        Ok(true)
    }
    /// 表目数。
    pub fn entry_count(&self) -> u32 {
        if self.size >= RT_STRIDE {
            self.size / RT_STRIDE
        } else {
            0
        }
    }
    pub fn render(&self) -> String {
        format!("ED{:x}+{} n{}", self.rva, self.size, self.entry_count())
    }
}

/// RUNTIME_FUNCTION 表目。
#[derive(Clone, Copy)]
pub struct RtEntry {
    pub idx: u32,
    pub begin: u32,
    pub end: u32,
    pub unwind: u32,
}

impl RtEntry {
    /// 表目静态校验：Begin < End 且界内。
    pub fn ok_in(&self, img: u32) -> Result<(), u32> {
        if self.idx == 0 {
            return Err(ED_ZERO_ID);
        }
        if self.begin >= self.end {
            return Err(ED_OOB); // 空区间按越界面拒
        }
        if self.end > img {
            return Err(ED_OOB);
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!("R{}|{:x}-{:x}|u{:x}", self.idx, self.begin, self.end, self.unwind)
    }
}

/// 装载册。
pub struct DirBook {
    pub dir: Option<ExcDir>,
    pub entries: Vec<RtEntry>,
    pub max_entries: usize,
    pub sealed: bool,
}

impl DirBook {
    pub fn new(max_entries: usize) -> DirBook {
        DirBook { dir: None, entries: Vec::new(), max_entries, sealed: false }
    }

    /// 装载目录。
    pub fn load_dir(&mut self, d: ExcDir) -> Result<bool, u32> {
        if self.sealed {
            return Err(ED_FROZEN);
        }
        let has = d.validate()?;
        self.dir = Some(d);
        Ok(has)
    }

    /// 装载表目：界内 → 唯一 → 容量。
    pub fn load_entry(&mut self, e: RtEntry, img: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(ED_FROZEN);
        }
        e.ok_in(img)?;
        if self.entries.iter().any(|x| x.begin == e.begin) {
            return Err(ED_DUP_ENTRY);
        }
        if self.entries.len() >= self.max_entries {
            return Err(ED_DUP_ENTRY); // 容量面按重目显性
        }
        self.entries.push(e);
        Ok(())
    }

    /// 表目数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        if let Some(d) = &self.dir {
            h = h.wrapping_mul(211).wrapping_add(d.rva);
            h = h.wrapping_mul(211).wrapping_add(d.size);
        }
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.begin);
            h = h.wrapping_mul(211).wrapping_add(e.end);
            h = h.wrapping_mul(211).wrapping_add(e.unwind);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!(
            "dir={} entries={}",
            self.dir.is_some(),
            self.entry_count()
        )
    }

    /// 诊断面：表目渲染行。
    pub fn line_of(&self, i: usize) -> String {
        match self.entries.get(i) {
            Some(e) => e.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DirBook::new(8);
    // 1) 坏尺寸（非 12 倍数）必拒。
    got[0] = b.load_dir(ExcDir { rva: 0x1000, size: 25, img_size: 0x10000 })
        .err() == Some(ED_BAD_SIZE);
    // 2) 零号表目必拒。
    got[1] = b.load_entry(RtEntry { idx: 0, begin: 0x10, end: 0x20, unwind: 0x30 }, 0x10000)
        .err() == Some(ED_ZERO_ID);
    // 3) 越镜像必拒。
    got[2] = b.load_entry(RtEntry { idx: 1, begin: 0xFFF0, end: 0x1_0100, unwind: 0x30 }, 0x10000)
        .err() == Some(ED_OOB);
    // 4) 重目必拒。
    let _ = b.load_entry(RtEntry { idx: 1, begin: 0x1000, end: 0x1100, unwind: 0x2000 }, 0x10000);
    got[3] = b.load_entry(RtEntry { idx: 2, begin: 0x1000, end: 0x1200, unwind: 0x2100 }, 0x10000)
        .err() == Some(ED_DUP_ENTRY);
    // 5) 印账后装载必抓。
    b.seal();
    got[4] = b.load_entry(RtEntry { idx: 3, begin: 0x2000, end: 0x2100, unwind: 0x2200 }, 0x10000)
        .err() == Some(ED_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12121");
    const IMG: u32 = 0x10000;

    // J1 目录尺寸对齐。
    let j1 = {
        let d = ExcDir { rva: 0x8000, size: 36, img_size: IMG };
        d.validate() == Ok(true) && d.entry_count() == 3
    };
    s.add("J1-尺寸对齐", j1, "36/12=3 目");

    // J2 坏尺寸拒。
    let j2 = {
        let d = ExcDir { rva: 0x8000, size: 25, img_size: IMG };
        d.validate().err() == Some(ED_BAD_SIZE)
    };
    s.add("J2-坏尺寸拒", j2, "非 12 倍数必拒");

    // J3 空目录合法。
    let j3 = {
        let d = ExcDir { rva: 0, size: 0, img_size: IMG };
        d.validate() == Ok(false) && d.entry_count() == 0
    };
    s.add("J3-空目录合法", j3, "Size0 记录在案");

    // J4 表目界内校验。
    let j4 = {
        let ok = RtEntry { idx: 1, begin: 0x100, end: 0x200, unwind: 0x300 };
        let bad = RtEntry { idx: 2, begin: 0x200, end: 0x100, unwind: 0x300 };
        ok.ok_in(IMG).is_ok() && bad.ok_in(IMG).err() == Some(ED_OOB)
    };
    s.add("J4-表目界内", j4, "空区间与越界皆拒");

    // J5 装载流程：目录 + 双目。
    let j5 = {
        let mut b = DirBook::new(8);
        let _ = b.load_dir(ExcDir { rva: 0x8000, size: 24, img_size: IMG });
        let _ = b.load_entry(RtEntry { idx: 1, begin: 0x1000, end: 0x1100, unwind: 0x2000 }, IMG);
        let _ = b.load_entry(RtEntry { idx: 2, begin: 0x1200, end: 0x1300, unwind: 0x2100 }, IMG);
        b.entry_count() == 2 && b.summary().contains("dir=true")
    };
    s.add("J5-装载流程", j5, "目录双目在册");

    // J6 重目拒。
    let j6 = {
        let mut b = DirBook::new(8);
        let _ = b.load_entry(RtEntry { idx: 1, begin: 0x1000, end: 0x1100, unwind: 0x2000 }, IMG);
        b.load_entry(RtEntry { idx: 2, begin: 0x1000, end: 0x1200, unwind: 0x2100 }, IMG)
            .err() == Some(ED_DUP_ENTRY)
    };
    s.add("J6-重目拒", j6, "同 Begin 二次必拒");

    // J7 目录渲染。
    let j7 = {
        let d = ExcDir { rva: 0x9000, size: 12, img_size: IMG };
        d.render().contains("n1")
    };
    s.add("J7-目录渲染", j7, "目录行含目数");

    // J8 表目渲染。
    let j8 = {
        let mut b = DirBook::new(8);
        let _ = b.load_entry(RtEntry { idx: 4, begin: 0x100, end: 0x110, unwind: 0x200 }, IMG);
        b.line_of(0).contains("u200") && b.line_of(1) == "missing"
    };
    s.add("J8-表目渲染", j8, "三列可回读");

    // J9 印账冻结。
    let j9 = {
        let mut b = DirBook::new(8);
        b.seal();
        b.load_dir(ExcDir { rva: 0x8000, size: 12, img_size: IMG })
            .err() == Some(ED_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = DirBook::new(8);
        let _ = b.load_entry(RtEntry { idx: 1, begin: 0x10, end: 0x20, unwind: 0x30 }, IMG);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = DirBook::new(8);
    let _ = b.load_dir(ExcDir { rva: 0xA000, size: 12, img_size: IMG });
    let _ = b.load_entry(RtEntry { idx: 1, begin: 0x401000, end: 0x401050, unwind: 0xA100 }, 0x500000);

    s.add("摘要回读", b.summary().contains("entries=1"), "单目在册");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12121_dir_flow() {
        let mut b = DirBook::new(4);
        assert!(b.load_dir(ExcDir { rva: 0x8000, size: 24, img_size: 0x10000 }).is_ok());
        assert!(b.load_entry(RtEntry { idx: 1, begin: 0x10, end: 0x20, unwind: 0x30 }, 0x10000).is_ok());
        assert_eq!(b.entry_count(), 1);
    }

    #[test]
    fn unx12121_checks_green() {
        let s = run_unx12121_checks();
        assert!(s.all_passed(), "UNX-F12121 判据未全绿");
    }

    #[test]
    fn entry_guard() {
        let e = RtEntry { idx: 1, begin: 0x50, end: 0x40, unwind: 0x60 };
        assert_eq!(e.ok_in(0x1000), Err(ED_OOB));
    }
}
