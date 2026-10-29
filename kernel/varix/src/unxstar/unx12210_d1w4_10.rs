//! UNX-F12210 坏 .pdata 拒止矩阵扩展（SEG 0x64140..0x64160 段窗）
//!
//! 卡面：.pdata RUNTIME_FUNCTION 条目拒止矩阵——坏区间/乱序/坏
//! unwind info 偏移/重复起始四类坏样本逐类拒止并留痕扩展。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 好条目入表：begin<end、升序不重叠、info 偏移在镜像内。
//! - J2 坏区间拒止：begin>=end 必拒 PD_BAD_RANGE。
//! - J3 乱序拒止：begin 回退或与前条半重叠必拒 PD_UNSORTED。
//! - J4 坏 info 偏移拒止：info 偏移越出镜像必拒 PD_BAD_INFO。
//! - J5 重复起始拒止：同 begin 双条必拒 PD_DUP。
//! - J6 归属查找：ip 命中覆盖条目，域外必拒 PD_NO_ENTRY。
//! - J7 拒止留痕：每类坏样本拒止即入扩展账。
//! - J8 拒止计数：四类坏样本各至少一次后 reject_count>=4。
//! - J9 印账冻结：seal 后入表/查找必拒 PD_FROZEN。
//! - J10 指纹幂等：同态同指纹。
//! - J11 摘要口径：好条/拒止量回读。
//! - J12 注入五况全抓。
//!
//! 结构：PdataMatrix 条目矩阵（好条账/拒止账/操作账）+ 入表核
//! （add）+ 查找核（lookup）+ 折叠指纹 + 注入核（坏区间/乱序/坏
//! info/重复起始/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64140 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x64140;
pub const PD_BAD_RANGE: u32 = SEG_BASE + 0x01; // 坏区间
pub const PD_UNSORTED: u32 = SEG_BASE + 0x02; // 乱序/半重叠
pub const PD_BAD_INFO: u32 = SEG_BASE + 0x03; // info 偏移越界
pub const PD_DUP: u32 = SEG_BASE + 0x04; // 重复起始
pub const PD_NO_ENTRY: u32 = SEG_BASE + 0x05; // 无覆盖条目
pub const PD_FROZEN: u32 = SEG_BASE + 0x06; // 印账后操作

/// .pdata 条目。
#[derive(Clone, Copy)]
pub struct RtFunc {
    pub begin: u32,
    pub end: u32,
    pub info: u32,
}

/// 拒止矩阵。
pub struct PdataMatrix {
    pub image_size: u32,
    pub good: Vec<RtFunc>,
    pub rejects: Vec<(u32, String)>, // (拒止码, 样本标签)
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl PdataMatrix {
    pub fn new(image_size: u32) -> PdataMatrix {
        PdataMatrix {
            image_size,
            good: Vec::new(),
            rejects: Vec::new(),
            ops: Vec::new(),
            sealed: false,
        }
    }

    /// 入表核：四类守卫，任何一拒即记账扩展。
    pub fn add(&mut self, e: RtFunc, tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PD_FROZEN);
        }
        let r = self.check(e);
        if let Err(code) = r {
            self.rejects.push((code, tag.to_string()));
            self.ops.push(format!("rej{:x}", code));
            return Err(code);
        }
        self.good.push(e);
        self.ops.push(format!("ok{:x}", e.begin));
        Ok(())
    }

    fn check(&self, e: RtFunc) -> Result<(), u32> {
        if e.begin >= e.end {
            return Err(PD_BAD_RANGE);
        }
        if e.info >= self.image_size {
            return Err(PD_BAD_INFO);
        }
        if self.good.iter().any(|g| g.begin == e.begin) {
            return Err(PD_DUP);
        }
        if let Some(last) = self.good.last() {
            if e.begin < last.end {
                return Err(PD_UNSORTED);
            }
        }
        Ok(())
    }

    /// 归属查找。
    pub fn lookup(&self, ip: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(PD_FROZEN);
        }
        for i in (0..self.good.len()).rev() {
            let g = self.good[i];
            if ip >= g.begin && ip < g.end {
                return Ok(i);
            }
        }
        Err(PD_NO_ENTRY)
    }

    /// 拒止账条数。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// 按码计拒止。
    pub fn rejects_of(&self, code: u32) -> usize {
        self.rejects.iter().filter(|r| r.0 == code).count()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for g in &self.good {
            h = h.wrapping_mul(211).wrapping_add(g.begin);
            h = h.wrapping_mul(211).wrapping_add(g.end);
            h = h.wrapping_mul(211).wrapping_add(g.info);
        }
        for r in &self.rejects {
            h = h.wrapping_mul(211).wrapping_add(r.0);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("good={} rej={}", self.good.len(), self.reject_count())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 坏区间必拒。
    got[0] = PdataMatrix::new(0x1000)
        .add(RtFunc { begin: 0x100, end: 0x100, info: 0 }, "range")
        .err()
        == Some(PD_BAD_RANGE);
    // 2) 乱序必拒。
    let mut m = PdataMatrix::new(0x1000);
    let _ = m.add(RtFunc { begin: 0x200, end: 0x300, info: 0 }, "ok");
    got[1] = m.add(RtFunc { begin: 0x150, end: 0x280, info: 0 }, "unsorted").err()
        == Some(PD_UNSORTED);
    // 3) 坏 info 必拒。
    got[2] = PdataMatrix::new(0x1000)
        .add(RtFunc { begin: 0x100, end: 0x200, info: 0x2000 }, "info")
        .err()
        == Some(PD_BAD_INFO);
    // 4) 重复起始必拒。
    let mut d = PdataMatrix::new(0x1000);
    let _ = d.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
    got[3] = d.add(RtFunc { begin: 0x100, end: 0x180, info: 0 }, "dup").err() == Some(PD_DUP);
    // 5) 印账后操作必抓。
    let mut f = PdataMatrix::new(0x1000);
    f.seal();
    got[4] = f.lookup(0x100).err() == Some(PD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12210_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12210");

    // J1 好条目入表。
    let j1 = {
        let mut m = PdataMatrix::new(0x1000);
        m.add(RtFunc { begin: 0x100, end: 0x200, info: 0x10 }, "ok1").is_ok()
            && m.add(RtFunc { begin: 0x200, end: 0x300, info: 0x20 }, "ok2").is_ok()
    };
    s.add("J1-好条目入表", j1, "升序不重叠入表");

    // J2 坏区间拒止。
    let j2 = PdataMatrix::new(0x1000)
        .add(RtFunc { begin: 0x300, end: 0x200, info: 0 }, "rev")
        .err()
        == Some(PD_BAD_RANGE);
    s.add("J2-坏区间拒止", j2, "倒序区间必拒");

    // J3 乱序拒止。
    let j3 = {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
        m.add(RtFunc { begin: 0x180, end: 0x280, info: 0 }, "half").err() == Some(PD_UNSORTED)
    };
    s.add("J3-乱序拒止", j3, "半重叠回退必拒");

    // J4 坏 info 偏移拒止。
    let j4 = PdataMatrix::new(0x1000)
        .add(RtFunc { begin: 0x100, end: 0x200, info: 0x1000 }, "edge")
        .err()
        == Some(PD_BAD_INFO);
    s.add("J4-坏info拒止", j4, "偏移达镜像界即拒");

    // J5 重复起始拒止。
    let j5 = {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
        m.add(RtFunc { begin: 0x100, end: 0x200, info: 8 }, "dup").err() == Some(PD_DUP)
    };
    s.add("J5-重复起始拒止", j5, "同 begin 双条必拒");

    // J6 归属查找。
    let j6 = {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
        m.lookup(0x150) == Ok(0) && m.lookup(0x250).err() == Some(PD_NO_ENTRY)
    };
    s.add("J6-归属查找", j6, "域内命中域外拒");

    // J7 拒止留痕。
    let j7 = {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x300, end: 0x200, info: 0 }, "rev");
        m.rejects_of(PD_BAD_RANGE) == 1 && m.reject_count() == 1
    };
    s.add("J7-拒止留痕", j7, "拒止即入扩展账");

    // J8 拒止计数四类齐。
    let j8 = {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x300, end: 0x200, info: 0 }, "rev");
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0x2000 }, "info");
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
        let _ = m.add(RtFunc { begin: 0x100, end: 0x180, info: 0 }, "dup");
        let _ = m.add(RtFunc { begin: 0x180, end: 0x280, info: 0 }, "half");
        m.rejects_of(PD_BAD_RANGE) == 1
            && m.rejects_of(PD_BAD_INFO) == 1
            && m.rejects_of(PD_DUP) == 1
            && m.rejects_of(PD_UNSORTED) == 1
            && m.reject_count() == 4
    };
    s.add("J8-拒止计数", j8, "四类各一次计 4");

    // J9 印账冻结。
    let j9 = {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
        m.seal();
        m.add(RtFunc { begin: 0x200, end: 0x300, info: 0 }, "late").err() == Some(PD_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后入表必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
        m.fold() == m.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 摘要口径。
    let j11 = {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
        let _ = m.add(RtFunc { begin: 0x300, end: 0x200, info: 0 }, "rev");
        m.summary() == "good=1 rej=1"
    };
    s.add("J11-摘要口径", j11, "好条/拒止量回读");

    // J12 边界 info 合法。
    let j12 = {
        let mut m = PdataMatrix::new(0x1000);
        m.add(RtFunc { begin: 0x100, end: 0x200, info: 0xFFF }, "edge-ok").is_ok()
    };
    s.add("J12-边界info合法", j12, "偏移=镜像界-1 可入表");

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-指纹非零", {
        let mut m = PdataMatrix::new(0x1000);
        let _ = m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok");
        m.fold() != 0
    }, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12210_pdata_flow() {
        let mut m = PdataMatrix::new(0x1000);
        assert!(m.add(RtFunc { begin: 0x100, end: 0x200, info: 0 }, "ok").is_ok());
        assert_eq!(m.add(RtFunc { begin: 0x100, end: 0x180, info: 0 }, "dup"), Err(PD_DUP));
        assert_eq!(m.lookup(0x150), Ok(0));
        assert_eq!(m.lookup(0x999), Err(PD_NO_ENTRY));
    }

    #[test]
    fn unx12210_checks_green() {
        let s = run_unx12210_checks();
        assert!(s.all_passed(), "UNX-F12210 判据未全绿");
    }

    #[test]
    fn unx12210_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
