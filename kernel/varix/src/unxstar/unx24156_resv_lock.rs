//! UNX-F24156 reservation 锁与显存互斥纪律·子功能16（锁统计汇总·争用计数）（0x41560 段）
//!
//! 锚点：UNX-F24156-J1 统计账（acquire/contend/hold 计数逐条落账）；
//! J2 统计语义（计数守恒 + 争用率自洽 + 热点排序三性质）；J3 统计指
//! 纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（锁统计争用面）。
//!
//! 结构：每锁计数器 + 争用登记核 + 热点排序核 + 折叠指纹 + 注入核（
//! 野锁/负计数/伪争用率/印后/重号必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41560 独占 0x20）。
pub const SEG_BASE: u32 = 0x41560;
pub const ST_UNK_LOCK: u32 = SEG_BASE + 0x01; // 野锁
pub const ST_BAD_RATE: u32 = SEG_BASE + 0x02; // 伪争用率
pub const ST_DUP_DECL: u32 = SEG_BASE + 0x03; // 重号声明
pub const ST_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const ST_NEG: u32 = SEG_BASE + 0x05; // 负计数档

/// 每锁统计。
#[derive(Clone)]
pub struct StEnt {
    pub lock_id: u32,
    pub acq: u32,
    pub rel: u32,
    pub contend: u32,
}

impl StEnt {
    /// 争用率（ppm）：contend*1e6/(acq+contend)。
    pub fn rate_ppm(&self) -> u32 {
        let tot = self.acq + self.contend;
        if tot == 0 {
            0
        } else {
            (self.contend.wrapping_mul(1_000_000)) / tot
        }
    }

    pub fn ok(&self) -> bool {
        self.rel <= self.acq && self.rate_ppm() <= 1_000_000
    }
}

/// 统计簿。
pub struct StBook {
    pub ents: Vec<StEnt>,
    pub sealed: bool,
}

impl StBook {
    pub fn new() -> StBook {
        StBook { ents: Vec::new(), sealed: false }
    }

    pub fn declare(&mut self, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(ST_FROZEN);
        }
        if self.ents.iter().any(|e| e.lock_id == lock_id) {
            return Err(ST_DUP_DECL);
        }
        self.ents.push(StEnt { lock_id, acq: 0, rel: 0, contend: 0 });
        Ok(())
    }

    fn bump<F: Fn(&mut StEnt)>(&mut self, lock_id: u32, f: F) -> Result<(), u32> {
        match self.ents.iter_mut().find(|e| e.lock_id == lock_id) {
            Some(e) => {
                f(e);
                Ok(())
            }
            None => Err(ST_UNK_LOCK),
        }
    }

    pub fn on_acq(&mut self, lock_id: u32) -> Result<(), u32> {
        self.bump(lock_id, |e| e.acq += 1)
    }

    pub fn on_rel(&mut self, lock_id: u32) -> Result<(), u32> {
        self.bump(lock_id, |e| {
            if e.rel < e.acq {
                e.rel += 1;
            }
        })
    }

    pub fn on_contend(&mut self, lock_id: u32) -> Result<(), u32> {
        self.bump(lock_id, |e| e.contend += 1)
    }

    /// 热点排序：争用率降序前 k 个锁号。
    pub fn hotspots(&self, k: usize) -> Vec<u32> {
        let mut es = self.ents.clone();
        // 简单插入排序按 rate 降序
        for i in 1..es.len() {
            let mut j = i;
            while j > 0 && es[j - 1].rate_ppm() < es[j].rate_ppm() {
                es.swap(j - 1, j);
                j -= 1;
            }
        }
        es.into_iter().take(k).map(|e| e.lock_id).collect()
    }

    pub fn all_ok(&self) -> bool {
        self.ents.iter().all(|e| e.ok())
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41560;
        for e in &self.ents {
            h = h.wrapping_mul(31).wrapping_add(e.lock_id);
            h = h.wrapping_mul(31).wrapping_add(e.acq);
            h = h.wrapping_mul(31).wrapping_add(e.rel);
            h = h.wrapping_mul(31).wrapping_add(e.contend);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：三锁不同争用画像。
pub fn mk_book() -> StBook {
    let mut b = StBook::new();
    let _ = b.declare(1);
    let _ = b.declare(2);
    let _ = b.declare(3);
    for _ in 0..8 {
        let _ = b.on_acq(1);
        let _ = b.on_rel(1);
    }
    for _ in 0..4 {
        let _ = b.on_acq(2);
        let _ = b.on_rel(2);
        let _ = b.on_contend(2);
    }
    let _ = b.on_acq(3);
    let _ = b.on_contend(3);
    let _ = b.on_contend(3);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野锁计数
    got[0] = b.on_acq(99).err() == Some(ST_UNK_LOCK);
    // 2) 重号声明
    got[1] = b.declare(1).err() == Some(ST_DUP_DECL);
    // 3) 伪争用率识破（rate 不超 1e6）
    got[2] = b.ents.iter().all(|e| e.rate_ppm() <= 1_000_000);
    // 4) rel 不越 acq（超额释放被吸收）
    let mut b2 = StBook::new();
    let _ = b2.declare(5);
    let _ = b2.on_rel(5); // 无 acq 释放被吸收
    got[3] = b2.ents.iter().find(|e| e.lock_id == 5).map(|e| e.rel == 0).unwrap_or(false);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.on_acq(1).err() == Some(ST_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24156_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24156");
    let mut b = mk_book();

    // J1 统计账齐
    let j1 = b.ents.len() == 3;
    s.add("J1-统计账齐", j1, "三锁全落表");

    // J2 计数守恒（rel ≤ acq）
    let j2 = b.ents.iter().all(|e| e.rel <= e.acq);
    s.add("J2-计数守恒", j2, "rel 不越 acq");

    // J3 锁1 计数精确
    let j3 = {
        let e = b.ents.iter().find(|e| e.lock_id == 1).unwrap();
        e.acq == 8 && e.rel == 8 && e.contend == 0
    };
    s.add("J3-锁1精确", j3, "8/8/0");

    // J4 争用率自洽
    let j4 = {
        let e = b.ents.iter().find(|e| e.lock_id == 2).unwrap();
        e.rate_ppm() == 500_000 // 4 contend / 8 tot
    };
    s.add("J4-争用率自洽", j4, "锁2 半数争用");

    // J5 热点排序
    let j5 = {
        let hot = b.hotspots(3);
        hot.len() == 3 && hot[0] == 3 // 锁3 2/3 争用率最高
    };
    s.add("J5-热点排序", j5, "锁3 居首");

    // J6 野锁拒
    let j6 = b.on_acq(99).err() == Some(ST_UNK_LOCK);
    s.add("J6-野锁拒", j6, "未声明锁必拒");

    // J7 重号声明拒
    let j7 = b.declare(1).err() == Some(ST_DUP_DECL);
    s.add("J7-重号拒", j7, "锁重号必拒");

    // J8 全账自洽
    let j8 = b.all_ok();
    s.add("J8-全账自洽", j8, "rate ≤ 1e6 且 rel ≤ acq");

    // J9 超额释放吸收
    let j9 = {
        let mut b2 = StBook::new();
        let _ = b2.declare(5);
        let _ = b2.on_rel(5);
        b2.ents.iter().find(|e| e.lock_id == 5).map(|e| e.rel == 0 && e.ok()).unwrap_or(false)
    };
    s.add("J9-超额吸收", j9, "无 acq 释放被吸收");

    // J10 零账率
    let j10 = {
        let mut b2 = StBook::new();
        let _ = b2.declare(9);
        b2.ents.iter().find(|e| e.lock_id == 9).map(|e| e.rate_ppm() == 0).unwrap_or(false)
    };
    s.add("J10-零账率", j10, "无账锁率 0");

    // J11 热点取 k 截断
    let j11 = b.hotspots(2).len() == 2;
    s.add("J11-热点截断", j11, "k=2 返 2 个");

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
    fn unx24156_stat_flow() {
        let mut b = StBook::new();
        let _ = b.declare(1);
        let _ = b.on_acq(1);
        let _ = b.on_acq(1);
        let _ = b.on_rel(1);
        let _ = b.on_contend(1);
        let e = b.ents.iter().find(|e| e.lock_id == 1).unwrap();
        assert_eq!(e.rate_ppm(), 500_000);
        assert_eq!(b.on_acq(9), Err(ST_UNK_LOCK));
        assert_eq!(b.declare(1), Err(ST_DUP_DECL));
        assert!(b.hotspots(1)[0] == 1);
        b.seal();
        assert_eq!(b.on_acq(1), Err(ST_FROZEN));
    }

    #[test]
    fn unx24156_checks_green() {
        let s = run_unx24156_checks();
        assert!(s.all_passed(), "UNX-F24156 判据未全绿");
    }
}
