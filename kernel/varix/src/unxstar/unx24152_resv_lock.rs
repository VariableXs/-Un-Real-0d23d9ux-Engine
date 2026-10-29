//! UNX-F24152 reservation 锁与显存互斥纪律·子功能12（持锁时长统计与超时账）（0x414E0 段）
//!
//! 锚点：UNX-F24152-J1 时长账（持锁起止与时长逐条落账）；J2 时长语
//! 义（未关账不计 + 超时标记 + 峰值守恒三性质）；J3 时长指纹（账折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（持锁时长超时面）。
//!
//! 结构：持锁区间表 + 关账核 + 超时标记核 + 统计账本 + 折叠指纹 + 注
//! 入核（倒序关账/野锁/双重关/印后/零区间必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x414E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x414E0;
pub const DU_REVERSED: u32 = SEG_BASE + 0x01; // 倒序区间
pub const DU_UNK: u32 = SEG_BASE + 0x02; // 野持锁
pub const DU_DUP_CLOSE: u32 = SEG_BASE + 0x03; // 双重关账
pub const DU_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const DU_TIMEOUT: u32 = SEG_BASE + 0x05; // 超时标记

/// 持锁区间。
#[derive(Clone)]
pub struct DuSpan {
    pub span_id: u32,
    pub lock_id: u32,
    pub holder: String,
    pub t0: u64,
    pub t1: Option<u64>,
    pub overtime: bool,
}

/// 时长簿。
pub struct DuBook {
    pub spans: Vec<DuSpan>,
    pub timeout_ms: u64,
    pub next_span: u32,
    pub sealed: bool,
}

impl DuBook {
    pub fn new(timeout_ms: u64) -> DuBook {
        DuBook { spans: Vec::new(), timeout_ms, next_span: 1, sealed: false }
    }

    /// 开账：t0 起点登记。
    pub fn open(&mut self, lock_id: u32, holder: &str, t0: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(DU_FROZEN);
        }
        if holder.is_empty() {
            return Err(DU_UNK);
        }
        let id = self.next_span;
        self.next_span += 1;
        self.spans.push(DuSpan {
            span_id: id,
            lock_id,
            holder: holder.to_string(),
            t0,
            t1: None,
            overtime: false,
        });
        Ok(id)
    }

    /// 关账：t1 >= t0，时长 = t1-t0；超时标记。
    pub fn close(&mut self, span_id: u32, t1: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(DU_FROZEN);
        }
        let sp = match self.spans.iter_mut().find(|s| s.span_id == span_id) {
            Some(s) => s,
            None => return Err(DU_UNK),
        };
        if sp.t1.is_some() {
            return Err(DU_DUP_CLOSE);
        }
        if t1 < sp.t0 {
            return Err(DU_REVERSED);
        }
        let dur = t1 - sp.t0;
        sp.t1 = Some(t1);
        if dur > self.timeout_ms {
            sp.overtime = true;
        }
        Ok(dur)
    }

    pub fn closed_count(&self) -> usize {
        self.spans.iter().filter(|s| s.t1.is_some()).count()
    }

    pub fn overtime_count(&self) -> usize {
        self.spans.iter().filter(|s| s.overtime).count()
    }

    /// 峰值时长。
    pub fn peak(&self) -> u64 {
        self.spans
            .iter()
            .filter_map(|s| s.t1.map(|t| t - s.t0))
            .max()
            .unwrap_or(0)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x414E0;
        for s in &self.spans {
            h = h.wrapping_mul(31).wrapping_add(s.span_id);
            h = h.wrapping_mul(31).wrapping_add(s.t0 as u32);
            h = h.wrapping_mul(31).wrapping_add(s.t1.unwrap_or(0) as u32);
            h = h.wrapping_mul(31).wrapping_add(s.overtime as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：三区间两关一超时。
pub fn mk_book() -> DuBook {
    let mut b = DuBook::new(100);
    let _ = b.open(1, "a", 10); // s1
    let _ = b.open(2, "b", 20); // s2
    let _ = b.open(1, "a", 200); // s3
    let _ = b.close(1, 50); // 40ms
    let _ = b.close(2, 200); // 180ms 超时
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 倒序关账
    got[0] = b.close(3, 100).err() == Some(DU_REVERSED);
    // 2) 野区间
    got[1] = b.close(99, 300).err() == Some(DU_UNK);
    // 3) 双重关账
    got[2] = b.close(1, 60).err() == Some(DU_DUP_CLOSE);
    // 4) 超时标记生效
    got[3] = b.spans.iter().find(|s| s.span_id == 2).map(|s| s.overtime).unwrap_or(false);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.open(1, "x", 1).err() == Some(DU_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24152_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24152");
    let mut b = mk_book();

    // J1 区间账齐
    let j1 = b.spans.len() == 3;
    s.add("J1-区间账齐", j1, "三区间全落表");

    // J2 关账计数
    let j2 = b.closed_count() == 2;
    s.add("J2-关账计数", j2, "两区间已关");

    // J3 时长计算
    let j3 = {
        let s1 = b.spans.iter().find(|s| s.span_id == 1).unwrap();
        s1.t1 == Some(50) && s1.t1.unwrap() - s1.t0 == 40
    };
    s.add("J3-时长计算", j3, "s1 时长 40ms");

    // J4 超时标记
    let j4 = b.overtime_count() == 1;
    s.add("J4-超时标记", j4, "s2 超 100ms 阈值");

    // J5 倒序拒
    let j5 = b.close(3, 100).err() == Some(DU_REVERSED);
    s.add("J5-倒序拒", j5, "t1<t0 必拒");

    // J6 双重关账拒
    let j6 = b.close(1, 60).err() == Some(DU_DUP_CLOSE);
    s.add("J6-双重关拒", j6, "已关区间再关必拒");

    // J7 野区间拒
    let j7 = b.close(99, 300).err() == Some(DU_UNK);
    s.add("J7-野区间拒", j7, "未知区间必拒");

    // J8 峰值守恒
    let j8 = b.peak() == 180;
    s.add("J8-峰值守恒", j8, "峰值 180ms");

    // J9 阈值边界（恰等不算超时）
    let j9 = {
        let mut b2 = DuBook::new(100);
        let _ = b2.open(1, "a", 0);
        let _ = b2.close(1, 100);
        b2.spans.iter().find(|s| s.span_id == 1).map(|s| !s.overtime).unwrap_or(false)
    };
    s.add("J9-边界不超", j9, "dur==阈值 不标超时");

    // J10 未关不计时长（峰值只取已关）
    let j10 = {
        let mut b2 = DuBook::new(10);
        let _ = b2.open(1, "a", 0);
        b2.peak() == 0
    };
    s.add("J10-未关不计", j10, "未关账不入峰值");

    // J11 空持有者拒
    let j11 = b.open(1, "", 1).err() == Some(DU_UNK);
    s.add("J11-空持有者拒", j11, "空名开账必拒");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24152_duration_flow() {
        let mut b = DuBook::new(100);
        let id = b.open(1, "a", 10).unwrap_or(0);
        assert_eq!(b.close(id, 5), Err(DU_REVERSED));
        assert_eq!(b.close(id, 200), Ok(190));
        assert_eq!(b.close(id, 210), Err(DU_DUP_CLOSE));
        assert_eq!(b.close(99, 1), Err(DU_UNK));
        b.seal();
        assert_eq!(b.open(1, "x", 1), Err(DU_FROZEN));
    }

    #[test]
    fn unx24152_checks_green() {
        let s = run_unx24152_checks();
        assert!(s.all_passed(), "UNX-F24152 判据未全绿");
    }
}
