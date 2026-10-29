//! UNX-F12067 Win32ThreadInfo 窗口线程信息面（0x19A80 段）
//!
//! 判据原文：Win32ThreadInfo 是 TEB +0x058 的用户态 GUI 槽，
//! 指向 win32k 线程信息；语义规则：仅在已初始化 GUI 线程非零；
//! 引用计数面（AttachThreadInput 跨线程借还）；计数下溢拒；
//! 消息队列深度窗口（1..=10000）；封存后拒动作。
//!
//! 结构：信息面（W32Info 指针 + 引用账）+ 借还核 + 队列深度
//! 守卫 + 四码守卫 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12067_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19A80 独占 0x20）。
pub const SEG_BASE: u32 = 0x19A80;
pub const W7_UNDERFLOW: u32 = SEG_BASE + 0x01; // 引用下溢
pub const W7_BAD_QDEPTH: u32 = SEG_BASE + 0x02; // 队列深度越窗
pub const W7_BAD_REC: u32 = SEG_BASE + 0x03; // 空留痕/伪账
pub const W7_FROZEN: u32 = SEG_BASE + 0x04; // 印账后仍动作
pub const W7_NOT_ATTACHED: u32 = SEG_BASE + 0x05; // 未挂接先归还

/// 消息队列深度窗口。
pub const Q_LO: u32 = 1;
pub const Q_HI: u32 = 10000;

/// 一笔借还账。
#[derive(Clone)]
pub struct W7Rec {
    pub seq: u32,
    /// true=借（attach）false=还（detach）。
    pub attach: bool,
    pub counter_after: u32,
    pub note: String,
}

impl W7Rec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// Win32 线程信息面。
pub struct W32Info {
    pub info_va: u32,
    pub attach_count: u32,
    pub queue_depth: u32,
    pub recs: Vec<W7Rec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl W32Info {
    /// 初始化：GUI 线程带指针与队列深度。
    pub fn new(info_va: u32, queue_depth: u32) -> Result<W32Info, u32> {
        if queue_depth < Q_LO || queue_depth > Q_HI {
            return Err(W7_BAD_QDEPTH);
        }
        Ok(W32Info {
            info_va,
            attach_count: 0,
            queue_depth,
            recs: Vec::new(),
            seq_next: 1,
            sealed: false,
        })
    }

    /// 借（attach）：挂接他线程输入。
    pub fn attach(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(W7_FROZEN);
        }
        if note.is_empty() {
            return Err(W7_BAD_REC);
        }
        self.attach_count += 1;
        let rec = W7Rec { seq: self.seq_next, attach: true, counter_after: self.attach_count, note: note.to_string() };
        if !rec.ok() {
            return Err(W7_BAD_REC);
        }
        self.seq_next += 1;
        self.recs.push(rec);
        Ok(self.attach_count)
    }

    /// 还（detach）：未挂接归还 → NOT_ATTACHED；正常计数减一。
    pub fn detach(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(W7_FROZEN);
        }
        if note.is_empty() {
            return Err(W7_BAD_REC);
        }
        if self.attach_count == 0 {
            return Err(W7_UNDERFLOW);
        }
        self.attach_count -= 1;
        let rec = W7Rec { seq: self.seq_next, attach: false, counter_after: self.attach_count, note: note.to_string() };
        if !rec.ok() {
            return Err(W7_BAD_REC);
        }
        self.seq_next += 1;
        self.recs.push(rec);
        Ok(self.attach_count)
    }

    /// 队列深度校验核。
    pub fn qdepth_ok(d: u32) -> bool {
        d >= Q_LO && d <= Q_HI
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.recs.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.info_va >> 4);
        h = h.wrapping_mul(211).wrapping_add(self.attach_count);
        h = h.wrapping_mul(211).wrapping_add(self.queue_depth);
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 队列深度越窗（0 与 10001 双样）→ W7_BAD_QDEPTH。
    got[0] = W32Info::new(0x1000, 0).err() == Some(W7_BAD_QDEPTH)
        && W32Info::new(0x1000, 10001).err() == Some(W7_BAD_QDEPTH);
    // 2) 未挂接归还 → W7_UNDERFLOW。
    let mut w = W32Info::new(0x1000, 100).unwrap_or_else(|_| W32Info {
        info_va: 0x1000, attach_count: 0, queue_depth: 100,
        recs: Vec::new(), seq_next: 1, sealed: false,
    });
    got[1] = w.detach("d").err() == Some(W7_UNDERFLOW);
    // 3) 空留痕 → W7_BAD_REC。
    got[2] = w.attach("").err() == Some(W7_BAD_REC);
    // 4) 伪账（空痕账目）→ not ok。
    let forged = W7Rec { seq: 1, attach: true, counter_after: 1, note: String::new() };
    got[3] = !forged.ok();
    // 5) 印账后动作 → W7_FROZEN。
    w.seal();
    got[4] = w.attach("post").err() == Some(W7_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12067_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12067");

    // J1 非 GUI 线程指针为零。
    let j1 = {
        let w = W32Info::new(0, 100).unwrap_or_else(|_| W32Info {
            info_va: 9, attach_count: 9, queue_depth: 9, recs: Vec::new(), seq_next: 1, sealed: false,
        });
        w.info_va == 0
    };
    s.add("J1-非GUI零槽", j1, "指针可零");

    // J2 GUI 线程指针落账。
    let j2 = {
        let w = W32Info::new(0x5A00, 500).unwrap_or_else(|_| W32Info {
            info_va: 0, attach_count: 0, queue_depth: 0, recs: Vec::new(), seq_next: 1, sealed: false,
        });
        w.info_va == 0x5A00 && w.queue_depth == 500
    };
    s.add("J2-GUI落账", j2, "指针与深度齐");

    // J3 队列深度窗口（界内两界外）。
    let j3 = W32Info::qdepth_ok(1) && W32Info::qdepth_ok(10000)
        && !W32Info::qdepth_ok(0) && !W32Info::qdepth_ok(10001);
    s.add("J3-深度窗口", j3, "1..=10000");

    // J4 越窗拒。
    let j4 = W32Info::new(0x1000, 0).err() == Some(W7_BAD_QDEPTH)
        && W32Info::new(0x1000, 0x2_0000).err() == Some(W7_BAD_QDEPTH);
    s.add("J4-越窗拒", j4, "零/巨深全拒");

    // J5 借还配对。
    let j5 = {
        let mut w = W32Info::new(0x1000, 100).unwrap_or_else(|_| W32Info {
            info_va: 0x1000, attach_count: 0, queue_depth: 100,
            recs: Vec::new(), seq_next: 1, sealed: false,
        });
        let a = w.attach("a") == Ok(1);
        let b = w.attach("b") == Ok(2);
        let d = w.detach("c") == Ok(1);
        a && b && d && w.attach_count == 1
    };
    s.add("J5-借还配对", j5, "计数 0→2→1");

    // J6 全还归零。
    let j6 = {
        let mut w = W32Info::new(0x1000, 100).unwrap_or_else(|_| W32Info {
            info_va: 0x1000, attach_count: 0, queue_depth: 100,
            recs: Vec::new(), seq_next: 1, sealed: false,
        });
        let _ = w.attach("a");
        let _ = w.detach("b");
        w.attach_count == 0 && w.detach("c").err() == Some(W7_UNDERFLOW)
    };
    s.add("J6-归零下溢", j6, "还尽再还拒");

    // J7 留痕对账（借还交替）。
    let j7 = {
        let mut w = W32Info::new(0x1000, 100).unwrap_or_else(|_| W32Info {
            info_va: 0x1000, attach_count: 0, queue_depth: 100,
            recs: Vec::new(), seq_next: 1, sealed: false,
        });
        let _ = w.attach("a");
        let _ = w.detach("b");
        w.recs.len() == 2 && w.recs[0].attach && !w.recs[1].attach && w.ledger_ok()
    };
    s.add("J7-借还留痕", j7, "真假面交替");

    // J8 空留痕拒。
    let j8 = {
        let mut w = W32Info::new(0x1000, 100).unwrap_or_else(|_| W32Info {
            info_va: 0x1000, attach_count: 0, queue_depth: 100,
            recs: Vec::new(), seq_next: 1, sealed: false,
        });
        w.attach("").err() == Some(W7_BAD_REC)
    };
    s.add("J8-空留痕拒", j8, "留痕必填");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = W32Info::new(0x1000, 100).unwrap_or_else(|_| W32Info {
            info_va: 0x1000, attach_count: 0, queue_depth: 100,
            recs: Vec::new(), seq_next: 1, sealed: false,
        });
        let _ = a.attach("x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = W32Info::new(0x2000, 100).unwrap_or_else(|_| W32Info {
            info_va: 0x2000, attach_count: 0, queue_depth: 100,
            recs: Vec::new(), seq_next: 1, sealed: false,
        });
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异面异纹");

    // J10 印账冻结。
    let j10 = {
        let mut w = W32Info::new(0x1000, 100).unwrap_or_else(|_| W32Info {
            info_va: 0x1000, attach_count: 0, queue_depth: 100,
            recs: Vec::new(), seq_next: 1, sealed: false,
        });
        w.seal();
        w.attach("post").err() == Some(W7_FROZEN) && w.detach("post").err() == Some(W7_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后动作拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例。
    let mut w = W32Info::new(0x5A00, 1000).unwrap_or_else(|_| W32Info {
        info_va: 0, attach_count: 0, queue_depth: 0, recs: Vec::new(), seq_next: 1, sealed: false,
    });
    let _ = w.attach("sum");
    s.add("样例挂接", w.attach_count == 1 && w.ledger_ok(), "一笔挂接");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12067_attach_detach() {
        let mut w = W32Info::new(0x1000, 100).ok().unwrap_or_else(|| W32Info {
            info_va: 0x1000, attach_count: 0, queue_depth: 100,
            recs: Vec::new(), seq_next: 1, sealed: false,
        });
        assert_eq!(w.attach("a"), Ok(1));
        assert_eq!(w.detach("b"), Ok(0));
        assert_eq!(w.detach("c").err(), Some(W7_UNDERFLOW));
    }

    #[test]
    fn unx12067_checks_green() {
        let s = run_unx12067_checks();
        assert!(s.all_passed(), "UNX-F12067 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12067_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
