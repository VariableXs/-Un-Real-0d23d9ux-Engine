//! UNX-F10880 unix socket 错误矩阵一段总收口（B24 断言聚合）（0x1c680 段）
//! 锚点：UNX-F10880-J1 错误矩阵一段总收口（B24 面 19 单锚聚合）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c680;
pub const EV_ANCHOR_MISSING: u32 = SEG_BASE + 1;
pub const EV_SEALED: u32 = SEG_BASE + 2;
pub const EV_COUNT_DRIFT: u32 = SEG_BASE + 3;
pub const EV_TWICE: u32 = SEG_BASE + 4;
pub const EV_RANGE_OUT: u32 = SEG_BASE + 5;
pub const EV_RED: u32 = SEG_BASE + 6;

pub const SEG_FIRST: u32 = 0x1c4e0;
pub const SEG_LAST: u32 = 0x1c660;
pub const FACE_ANCHOR: usize = 12;

/// B24 面单锚：段基址 + 绿标
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceAnchor {
    pub seg: u32,
    pub green: bool,
}

impl FaceAnchor {
    pub fn in_range(&self) -> bool {
        self.seg >= SEG_FIRST && self.seg <= SEG_LAST && (self.seg - SEG_FIRST) % 0x20 == 0
    }
}

/// 一段总收口册：十二单锚聚合对平
pub struct FaceClose {
    anchors: Vec<FaceAnchor>,
    sealed: bool,
    verdict: Option<bool>,
}

impl FaceClose {
    pub fn new() -> Self {
        FaceClose { anchors: Vec::new(), sealed: false, verdict: None }
    }

    pub fn put(&mut self, a: FaceAnchor) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.anchors.len() >= FACE_ANCHOR {
            return Err(EV_COUNT_DRIFT);
        }
        if !a.in_range() {
            return Err(EV_RANGE_OUT);
        }
        for e in self.anchors.iter() {
            if e.seg == a.seg {
                return Err(EV_TWICE);
            }
        }
        self.anchors.push(a);
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.anchors.len()
    }

    pub fn red_count(&self) -> usize {
        self.anchors.iter().filter(|a| !a.green).count()
    }

    pub fn red_segs(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for a in self.anchors.iter() {
            if !a.green {
                v.push(a.seg);
            }
        }
        v
    }

    /// 收口：十二单齐且全绿
    pub fn close(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.anchors.len() != FACE_ANCHOR {
            self.sealed = true;
            return Err(EV_ANCHOR_MISSING);
        }
        if self.red_count() > 0 {
            self.sealed = true;
            self.verdict = Some(false);
            return Err(EV_RED);
        }
        self.sealed = true;
        self.verdict = Some(true);
        Ok(())
    }

    pub fn verdict_of(&self) -> Option<bool> {
        self.verdict
    }

    /// 段位连续性：入册段基址按 0x20 步进无缺
    pub fn continuity(&self) -> bool {
        if self.anchors.is_empty() {
            return false;
        }
        let mut segs: Vec<u32> = Vec::new();
        for a in self.anchors.iter() {
            segs.push(a.seg);
        }
        // 插入排序
        let mut i = 1;
        while i < segs.len() {
            let key = segs[i];
            let mut j = i;
            while j > 0 && segs[j - 1] > key {
                segs[j] = segs[j - 1];
                j -= 1;
            }
            segs[j] = key;
            i += 1;
        }
        let mut i = 1;
        while i < segs.len() {
            if segs[i] - segs[i - 1] != 0x20 {
                return false;
            }
            i += 1;
        }
        true
    }
}

fn full_faces() -> FaceClose {
    let mut f = FaceClose::new();
    let mut n = 0;
    while n < FACE_ANCHOR {
        f.put(FaceAnchor { seg: SEG_FIRST + (n as u32) * 0x20, green: true }).unwrap();
        n += 1;
    }
    f
}

pub fn run_unx10880_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10880");
    s.add("J1-段锚", SEG_BASE == 0x1c680, "B24 收口段基址 0x1c680");
    s.add("J2-首段锚", SEG_FIRST == 0x1c4e0, "B24 首段 0x1c4e0（F10867）");
    s.add("J3-尾段锚", SEG_LAST == 0x1c660, "B24 尾段 0x1c660（F10879）");
    s.add("J4-面数锚", FACE_ANCHOR == 12, "十二单锚");
    s.add("J5-满册十二锚", full_faces().count() == 12, "十二锚齐入册");
    s.add("J6-段界内校验", {
        FaceAnchor { seg: 0x1c4e0, green: true }.in_range()
            && FaceAnchor { seg: 0x1c660, green: true }.in_range()
    }, "首尾段界内真");
    s.add("J7-段界外拒", {
        let mut f = FaceClose::new();
        f.put(FaceAnchor { seg: 0x1c4c0, green: true }) == Err(EV_RANGE_OUT)
            && f.put(FaceAnchor { seg: 0x1c680, green: true }) == Err(EV_RANGE_OUT)
    }, "界外前后拒收");
    s.add("J8-段对齐校验", {
        let mut f = FaceClose::new();
        f.put(FaceAnchor { seg: 0x1c4e1, green: true }) == Err(EV_RANGE_OUT)
    }, "非 0x20 对齐拒");
    s.add("J9-重复锚拒", {
        let mut f = FaceClose::new();
        f.put(FaceAnchor { seg: 0x1c4e0, green: true }).unwrap();
        f.put(FaceAnchor { seg: 0x1c4e0, green: true }) == Err(EV_TWICE)
    }, "重复段 EV_TWICE");
    s.add("J10-满册收口过", full_faces().close().is_ok(), "十二全绿收口 Ok");
    s.add("J11-收口裁决真", {
        let mut f = full_faces();
        f.close().unwrap();
        f.verdict_of() == Some(true)
    }, "收口裁决 true");
    s.add("J12-红锚收口拒", {
        let mut f = FaceClose::new();
        let mut n = 0;
        while n < FACE_ANCHOR {
            let green = n != 5;
            f.put(FaceAnchor { seg: SEG_FIRST + (n as u32) * 0x20, green }).unwrap();
            n += 1;
        }
        f.close().unwrap_err() == EV_RED && f.red_segs() == vec![SEG_FIRST + 5 * 0x20]
    }, "第六锚红 EV_RED 单红号");
    s.add("J13-缺锚收口拒", {
        let mut f = full_faces();
        f.anchors.pop();
        f.close().unwrap_err() == EV_ANCHOR_MISSING
    }, "十一锚 EV_ANCHOR_MISSING");
    s.add("J14-连续性真", {
        let f = full_faces();
        f.continuity()
    }, "十二段 0x20 连续真");
    s.add("J15-连续性破", {
        let mut f = FaceClose::new();
        let mut n = 0;
        while n < FACE_ANCHOR {
            let seg = if n == 7 { SEG_FIRST + (n as u32) * 0x20 + 0x40 } else { SEG_FIRST + (n as u32) * 0x20 };
            f.put(FaceAnchor { seg, green: true }).unwrap();
            n += 1;
        }
        !f.continuity()
    }, "第 8 锚错位连续性假");
    s.add("J16-收口封账", {
        let mut f = full_faces();
        f.close().unwrap();
        f.put(FaceAnchor { seg: SEG_FIRST, green: true }) == Err(EV_SEALED)
    }, "收口后入锚 EV_SEALED");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_face_close() {
        let mut f = full_faces();
        assert!(f.close().is_ok());
        assert_eq!(f.verdict_of(), Some(true));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10880_checks();
        assert!(s.all_passed(), "UNX-F10880 判据全绿");
    }
}
