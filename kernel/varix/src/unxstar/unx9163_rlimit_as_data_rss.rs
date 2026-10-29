//! UNX-F9163 RLIMIT_AS/DATA/RSS 内存限额族（0x63420 段）
//!
//! 锚点：UNX-F9163-J1 内存格账（AS/DATA/RSS 每格消费触发落账：资
//! 源、请求、限额、放行/拒收、留痕）；J2 内存格语义（AS 全局地址
//! 空间闸 + DATA brk 段闸 + RSS 驻留软账三性质）；J3 内存格指纹（
//! 账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（内存限额消费族）。
//!
//! 结构：三格限额簿 + 消费核（mmap 挂点走 AS / brk 挂点走 DATA /
//! 驻留记账走 RSS 软告警）+ 消费账本 + 折叠指纹 + 注入核（错码/空
//! 留痕/越限放行/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63420 独占 0x20）。
pub const SEG_BASE: u32 = 0x63420;
pub const RL3_UNK_RES: u32 = SEG_BASE + 0x01; // 伪资源
pub const RL3_BAD_LIMIT: u32 = SEG_BASE + 0x02; // 越限放行伪样本
pub const RL3_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const RL3_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const RL3_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 资源闭集。
pub const M_AS: usize = 0;
pub const M_DATA: usize = 1;
pub const M_RSS: usize = 2;
pub const M_COUNT: usize = 3;

pub const M_NAMES: [&str; M_COUNT] = ["AS", "DATA", "RSS"];

/// RLIM_INFINITY。
pub const INF: u32 = u32::MAX;

/// 单条消费账。
#[derive(Clone)]
pub struct MemRec {
    pub seq: u32,
    pub res: usize,
    pub want: u32,
    pub limit: u32,
    pub allowed: bool,
    pub note: String,
}

impl MemRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.res < M_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "M3{}|{}|w{}<l{}|{}|{}",
            self.seq,
            M_NAMES[self.res],
            self.want,
            self.limit,
            if self.allowed { "GO" } else { "DENY" },
            self.note
        )
    }
}

/// 内存限额簿。
pub struct MemBook {
    pub limits: [u32; M_COUNT],
    /// 当前占用：AS 总映射、DATA brk 段、RSS 驻留
    pub used: [u32; M_COUNT],
    pub ledger: Vec<MemRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl MemBook {
    pub fn new() -> MemBook {
        MemBook {
            limits: [INF, 64 * 1024 * 1024, 256 * 1024 * 1024],
            used: [0, 0, 0],
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn set_limit(&mut self, res: usize, v: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RL3_FROZEN);
        }
        if res >= M_COUNT {
            return Err(RL3_UNK_RES);
        }
        self.limits[res] = v;
        Ok(())
    }

    fn fits(&self, res: usize, add: u32) -> bool {
        let lim = self.limits[res];
        lim == INF || self.used[res] + add <= lim
    }

    fn record(&mut self, res: usize, want: u32, allowed: bool, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(MemRec {
            seq,
            res,
            want,
            limit: self.limits[res],
            allowed,
            note: note.to_string(),
        });
    }

    /// AS 消费核：mmap 挂点。
    pub fn mmap_alloc(&mut self, size: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL3_FROZEN);
        }
        if note.is_empty() {
            return Err(RL3_EMPTY_NOTE);
        }
        if !self.fits(M_AS, size) {
            self.record(M_AS, size, false, note);
            return Err(RL3_BAD_LIMIT);
        }
        self.used[M_AS] += size;
        self.record(M_AS, size, true, note);
        Ok(self.used[M_AS])
    }

    /// DATA 消费核：brk 挂点。
    pub fn brk_grow(&mut self, inc: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL3_FROZEN);
        }
        if note.is_empty() {
            return Err(RL3_EMPTY_NOTE);
        }
        if !self.fits(M_DATA, inc) {
            self.record(M_DATA, inc, false, note);
            return Err(RL3_BAD_LIMIT);
        }
        self.used[M_DATA] += inc;
        self.record(M_DATA, inc, true, note);
        Ok(self.used[M_DATA])
    }

    /// RSS 软账：驻留增长只告警不拒（allowed=未越限）。
    pub fn rss_touch(&mut self, pages: u32, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(RL3_FROZEN);
        }
        if note.is_empty() {
            return Err(RL3_EMPTY_NOTE);
        }
        let over = !self.fits(M_RSS, pages);
        self.used[M_RSS] += pages;
        self.record(M_RSS, pages, !over, note);
        Ok(!over) // true=仍在限内
    }

    pub fn unmap(&mut self, size: u32) {
        self.used[M_AS] = self.used[M_AS].saturating_sub(size);
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91630;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.res as u32);
            h = h.wrapping_mul(31).wrapping_add(r.want);
            h = h.wrapping_mul(31).wrapping_add(r.allowed as u32);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 伪资源
    let bad = MemRec {
        seq: 1,
        res: 9,
        want: 1,
        limit: 1,
        allowed: true,
        note: "inj-res".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = MemRec {
        seq: 2,
        res: 0,
        want: 1,
        limit: 1,
        allowed: true,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) AS 越限拒
    let mut b = MemBook::new();
    let _ = b.set_limit(M_AS, 1024);
    got[2] = b.mmap_alloc(2048, "inj-as").err() == Some(RL3_BAD_LIMIT);
    // 4) brk 越限拒
    let _ = b.set_limit(M_DATA, 512);
    got[3] = b.brk_grow(1024, "inj-brk").err() == Some(RL3_BAD_LIMIT);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.mmap_alloc(8, "inj-frozen").err() == Some(RL3_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9163_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9163");
    let mut b = MemBook::new();

    // J1 三格账齐
    let j1 = {
        let _ = b.mmap_alloc(4096, "mmap 走 AS");
        let _ = b.brk_grow(1024, "brk 走 DATA");
        let _ = b.rss_touch(16, "驻留走 RSS");
        b.ledger.iter().filter(|r| r.res == M_AS).count() >= 1
            && b.ledger.iter().filter(|r| r.res == M_DATA).count() >= 1
            && b.ledger.iter().filter(|r| r.res == M_RSS).count() >= 1
    };
    s.add("J1-三格账齐", j1, "AS/DATA/RSS 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且格闭集");

    // J3 AS 越限拒
    let j3 = {
        let mut b2 = MemBook::new();
        let _ = b2.set_limit(M_AS, 8192);
        let _ = b2.mmap_alloc(4096, "a");
        b2.mmap_alloc(8192, "越").err() == Some(RL3_BAD_LIMIT)
    };
    s.add("J3-AS越限拒", j3, "地址空间超限 mmap 必拒");

    // J4 AS 累计放行
    let j4 = {
        let mut b2 = MemBook::new();
        let _ = b2.set_limit(M_AS, 8192);
        let _ = b2.mmap_alloc(4096, "a");
        b2.mmap_alloc(4096, "b") == Ok(8192)
    };
    s.add("J4-AS累计", j4, "累计恰达限放行");

    // J5 AS unmap 回收
    let j5 = {
        let mut b2 = MemBook::new();
        let _ = b2.set_limit(M_AS, 8192);
        let _ = b2.mmap_alloc(8192, "满");
        let denied = b2.mmap_alloc(8, "拒").is_err();
        b2.unmap(8192);
        denied && b2.mmap_alloc(8, "回收后再申").is_ok()
    };
    s.add("J5-unmap回收", j5, "munmap 释放后可再申");

    // J6 DATA brk 闸
    let j6 = {
        let mut b2 = MemBook::new();
        let _ = b2.set_limit(M_DATA, 1024);
        b2.brk_grow(2048, "越").err() == Some(RL3_BAD_LIMIT)
    };
    s.add("J6-brk闸", j6, "brk 段超限必拒");

    // J7 DATA 无穷直通
    let j7 = {
        let mut b2 = MemBook::new();
        b2.limits[M_DATA] = INF;
        b2.brk_grow(1 << 30, "无限") == Ok(1 << 30)
    };
    s.add("J7-无穷直通", j7, "RLIM_INFINITY 绕闸");

    // J8 RSS 软告警不拒
    let j8 = {
        let mut b2 = MemBook::new();
        let _ = b2.set_limit(M_RSS, 100);
        let r = b2.rss_touch(200, "超软");
        r == Ok(false) && b2.used[M_RSS] == 200
    };
    s.add("J8-RSS软告警", j8, "RSS 超限只告警照记");

    // J9 伪资源拒
    let j9 = b.set_limit(9, 1).err() == Some(RL3_UNK_RES);
    s.add("J9-伪资源拒", j9, "格出闭集必拒");

    // J10 拒收留痕
    let j10 = b.ledger.iter().any(|r| !r.allowed);
    s.add("J10-拒收留痕", j10, "DENY 路径有账");

    // J11 三格独立：AS 满不影响 brk
    let j11 = {
        let mut b2 = MemBook::new();
        let _ = b2.set_limit(M_AS, 8);
        let _ = b2.mmap_alloc(8, "AS 满");
        b2.brk_grow(64, "DATA 不受牵连").is_ok()
    };
    s.add("J11-三格独立", j11, "格间互不牵连");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

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
    fn unx9163_mem_flow() {
        let mut b = MemBook::new();
        let _ = b.set_limit(M_AS, 8192);
        assert_eq!(b.mmap_alloc(4096, "a"), Ok(4096));
        assert_eq!(b.mmap_alloc(8192, "b"), Err(RL3_BAD_LIMIT));
        b.unmap(4096);
        assert_eq!(b.mmap_alloc(4096, "c"), Ok(8192));
        // DATA
        let _ = b.set_limit(M_DATA, 1024);
        assert_eq!(b.brk_grow(2048, "d"), Err(RL3_BAD_LIMIT));
        assert_eq!(b.brk_grow(512, "e"), Ok(512));
        // RSS 软
        let _ = b.set_limit(M_RSS, 100);
        assert_eq!(b.rss_touch(200, "f"), Ok(false));
    }

    #[test]
    fn unx9163_checks_green() {
        let s = run_unx9163_checks();
        assert!(s.all_passed(), "UNX-F9163 判据未全绿");
    }
}
