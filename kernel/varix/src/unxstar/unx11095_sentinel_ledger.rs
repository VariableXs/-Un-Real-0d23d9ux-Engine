//! UNX-F11095 长稳哨兵总账（异常零静默断言）（0x1ebe0 段）
//!
//! 锚点：UNX-F11095-J1 三哨总账逐段对平——计数哨/序哨/指纹哨三路哨兵
//! 逐段留痕对平；J2 异常零静默断言——任一哨位异常必翻异常旗并留痕，
//! 绝不静默吞掉。
//! 域：CoRun·UNX-C4（长稳监控域）。
//!
//! 结构：哨兵册（计数哨：事件计数对账；序哨：事件序号连续性；指纹哨：
//! 事件流 FNV 链）+ 异常旗（任一哨失配即翻旗并计一笔异常留痕）+ 注入核
//! （序断裂/计数失配/指纹漂移/封账写入四路必抓）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x1ebe0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ebe0;
pub const SN_SEQ: u32 = SEG_BASE + 0x01; // 序哨断裂
pub const SN_CNT: u32 = SEG_BASE + 0x02; // 计数哨失配
pub const SN_FP: u32 = SEG_BASE + 0x03; // 指纹哨漂移
pub const SN_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写账
pub const SN_NOENT: u32 = SEG_BASE + 0x05; // 段不在册

/// 段哨位账。
#[derive(Clone, Copy, Debug)]
pub struct SegSent {
    pub seg: u32,
    pub events: u32,
    pub last_no: u32,
    pub fp: u64,
}

/// 哨兵总账。
pub struct SentinelBook {
    pub segs: Vec<SegSent>,
    pub anomalies: u32,
    pub sealed: bool,
}

impl SentinelBook {
    pub fn new() -> SentinelBook {
        SentinelBook { segs: Vec::new(), anomalies: 0, sealed: false }
    }

    fn at(&self, seg: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].seg == seg {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 开段。
    pub fn open(&mut self, seg: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SN_FROZEN);
        }
        if self.at(seg).is_some() {
            return Err(SN_NOENT);
        }
        self.segs.push(SegSent { seg, events: 0, last_no: 0, fp: 0x1ebe_02a5 });
        Ok(())
    }

    /// 事件入账：序号必须连续（last_no+1），指纹链推进。
    pub fn event(&mut self, seg: u32, expect_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SN_FROZEN);
        }
        match self.at(seg) {
            None => Err(SN_NOENT),
            Some(i) => {
                let next = self.segs[i].last_no.wrapping_add(1);
                if expect_no != next {
                    self.anomalies = self.anomalies.wrapping_add(1);
                    return Err(SN_SEQ);
                }
                self.segs[i].last_no = next;
                self.segs[i].events = self.segs[i].events.wrapping_add(1);
                self.segs[i].fp = self.segs[i].fp.wrapping_mul(0x100000001b3)
                    .wrapping_add(expect_no as u64);
                Ok(())
            }
        }
    }

    /// 计数哨：events 与 last_no 对平。
    pub fn count_ok(&self, seg: u32) -> Result<bool, u32> {
        match self.at(seg) {
            None => Err(SN_NOENT),
            Some(i) => Ok(self.segs[i].events == self.segs[i].last_no),
        }
    }

    /// 指纹哨：重放链一致（对已记事件流 fold 稳定）。
    pub fn fp_ok(&self, seg: u32, twice: u64) -> Result<bool, u32> {
        match self.at(seg) {
            None => Err(SN_NOENT),
            Some(i) => Ok(self.segs[i].fp == twice),
        }
    }

    /// 全段三哨对平裁断。
    pub fn all_ok(&self) -> bool {
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].events != self.segs[k].last_no || self.segs[k].fp == 0 {
                return false;
            }
            k += 1;
        }
        self.anomalies == 0
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1ebe_02a5;
        let mut k = 0usize;
        while k < self.segs.len() {
            h = (h ^ self.segs[k].seg as u64).wrapping_mul(0x100000001b3);
            h = (h ^ (self.segs[k].events as u64) << 16).wrapping_mul(0x100000001b3);
            h = (h ^ self.segs[k].fp).wrapping_mul(0x100000001b3);
            k = k.wrapping_add(1);
        }
        h = (h ^ self.anomalies as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 千次三哨压力：3 段各 333+ 轮事件流，全程对平。
pub fn stress_sentinel(rounds: u32) -> (u32, bool) {
    let mut b = SentinelBook::new();
    let mut s = 1u32;
    while s <= 3 {
        let _ = b.open(s);
        s = s.wrapping_add(1);
    }
    let mut ok = true;
    // 每段独立序号。
    let mut seq = [0u32; 3];
    let mut r = 0u32;
    while r < rounds {
        let t = (r % 3) as usize;
        seq[t] = seq[t].wrapping_add(1);
        if b.event((t + 1) as u32, seq[t]).is_err() {
            ok = false;
        }
        if !b.all_ok() {
            ok = false;
        }
        r = r.wrapping_add(1);
    }
    (b.anomalies, ok && b.all_ok())
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 序哨断裂必抓（跳号）
    let mut b = SentinelBook::new();
    let _ = b.open(1);
    let _ = b.event(1, 1);
    got[0] = b.event(1, 3).err() == Some(SN_SEQ) && b.anomalies == 1;
    // 2) 计数哨失配必抓（手工漂移 events）
    let mut c = SentinelBook::new();
    let _ = c.open(1);
    let _ = c.event(1, 1);
    c.segs[0].events = 9;
    got[1] = c.count_ok(1) == Ok(false);
    // 3) 指纹哨漂移必抓（对拍值不同）
    let mut d = SentinelBook::new();
    let _ = d.open(1);
    let _ = d.event(1, 1);
    let fp = d.segs[0].fp;
    got[2] = d.fp_ok(1, fp) == Ok(true) && d.fp_ok(1, fp.wrapping_add(1)) == Ok(false);
    // 4) 封账写入必抓
    d.seal();
    got[3] = d.event(1, 2).err() == Some(SN_FROZEN)
        && d.open(2).err() == Some(SN_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11095_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11095");

    // J1 三哨总账逐段对平
    let (anom, ok) = stress_sentinel(1000);
    let inj = inject_four();
    let j1 = ok && anom == 0 && inj[0];
    s.add("J1-三哨逐段对平", j1, "计数/序/指纹三哨千次事件流全程对平");

    // J2 异常零静默断言
    let mut b = SentinelBook::new();
    let _ = b.open(1);
    let _ = b.event(1, 1);
    let e1 = b.event(1, 5);
    let j2 = e1.err() == Some(SN_SEQ) && b.anomalies == 1
        && b.count_ok(1) == Ok(true) && inj[1];
    s.add("J2-异常零静默", j2, "序断裂必翻旗留痕绝不静默吞掉");

    // J3 计数哨失配必抓
    s.add("J3-计数失配抓", inj[1], "events 漂移必被计数哨抓出");

    // J4 指纹哨漂移必抓
    s.add("J4-指纹漂移抓", inj[2], "对拍值失配必被指纹哨抓出");

    // J5 封账写入必抓
    s.add("J5-封账拒写", inj[3], "封账后事件与开段两路均必拒");

    // J6 未知段事件必抓
    let mut c = SentinelBook::new();
    let j6 = c.event(9, 1).err() == Some(SN_NOENT)
        && c.count_ok(9).err() == Some(SN_NOENT);
    s.add("J6-未知段抓", j6, "未开段事件与查询必拒");

    // J7 序号从 1 起：首事件非 1 必抓
    let mut d = SentinelBook::new();
    let _ = d.open(2);
    let j7 = d.event(2, 0).err() == Some(SN_SEQ)
        && d.event(2, 1).is_ok();
    s.add("J7-序号起点", j7, "首事件非 1 必拒且 1 号事件可入");

    // J8 指纹链推进稳定：同事件流重放指纹一致
    let mut e = SentinelBook::new();
    let _ = e.open(1);
    let mut k = 1u32;
    while k <= 16 {
        let _ = e.event(1, k);
        k = k.wrapping_add(1);
    }
    let fp = e.segs[0].fp;
    let mut f = SentinelBook::new();
    let _ = f.open(1);
    let mut k = 1u32;
    while k <= 16 {
        let _ = f.event(1, k);
        k = k.wrapping_add(1);
    }
    let j8 = f.segs[0].fp == fp && e.count_ok(1) == Ok(true);
    s.add("J8-重放指纹", j8, "同事件流重放指纹一致零漂移");

    // J9 异常后账面不损：断裂后合法事件可续
    let mut g = SentinelBook::new();
    let _ = g.open(1);
    let _ = g.event(1, 1);
    let _ = g.event(1, 9);
    let ok2 = g.event(1, 2);
    let j9 = ok2.is_ok() && g.all_ok() == false && g.count_ok(1) == Ok(true);
    s.add("J9-异常后续接", j9, "异常留痕后合法序号续接且异常旗不撤");

    // J10 fold 指纹与封账根
    let fp0 = g.fold();
    let root = g.seal();
    let j10 = fp0 == root && g.fold() == root;
    s.add("J10-指纹冻结", j10, "封账根与 fold 一致且二次 fold 稳定");

    // J11 千次异常计数落账
    s.add_dyn("J11-异常计数", anom == 0, &format!("anomalies={}", anom));

    // J12 指纹落账
    let fp = b.fold();
    s.add_dyn("J12-账指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11095_sentinel_flow() {
        let mut b = SentinelBook::new();
        assert!(b.open(1).is_ok());
        assert!(b.open(1).err() == Some(SN_NOENT));
        assert!(b.event(1, 1).is_ok());
        assert!(b.event(1, 3).err() == Some(SN_SEQ));
        assert!(b.anomalies == 1);
        assert!(b.event(1, 2).is_ok());
        assert!(b.count_ok(1) == Ok(true));
        assert!(b.fp_ok(1, b.segs[0].fp) == Ok(true));
        b.seal();
        assert!(b.event(1, 3).err() == Some(SN_FROZEN));
    }

    #[test]
    fn unx11095_checks_green() {
        let s = run_unx11095_checks();
        assert!(s.all_passed(), "UNX-F11095 判据未全绿");
    }
}
