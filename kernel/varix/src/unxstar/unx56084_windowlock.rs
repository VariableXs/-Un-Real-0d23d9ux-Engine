//! UNX-F56084 采集窗互斥锁件（禁并行污染）（0x60A60 段）
//!
//! 锚点：UNX-F56084-J1 锁账全量（采集窗互斥锁逐窗落账：窗号 × 持锁器 ×
//! 锁态三元组）；J2 互斥语义（同窗同刻仅一器持锁，重入/越窗持锁/未解锁
//! 再锁逐闸拒收）；J3 锁审计链（持锁史留痕、锁指纹链、封账只读）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·窗互斥面）。
//!
//! 结构：窗锁登记册（窗号 → 持锁器 × 锁态逐窗落账）+ 互斥守闸（锁定中
//! 二次加锁拒、无锁解锁拒、跨器越权解锁拒）+ 持锁史链（加/解锁逐次
//! 留痕复合指纹）+ 封账语义 + 注入核（双重加锁/无锁解锁/越权解锁/未知
//! 持锁器/封账改锁/空窗查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60A60 独占 0x20）。
pub const SEG_BASE: u32 = 0x60A60;
pub const LK_DOUBLE: u32 = SEG_BASE + 0x01; // 双重加锁（锁定中再锁）
pub const LK_UNLOCK_FREE: u32 = SEG_BASE + 0x02; // 无锁解锁
pub const LK_FOREIGN: u32 = SEG_BASE + 0x03; // 越权解锁（非持锁器解锁）
pub const LK_UNK_COLL: u32 = SEG_BASE + 0x04; // 未知持锁器（五闭集外）
pub const LK_SEALED: u32 = SEG_BASE + 0x05; // 锁账已封账拒改
pub const LK_WIN_MISS: u32 = SEG_BASE + 0x06; // 空窗查询
pub const LK_BACK: u32 = SEG_BASE + 0x07; // 窗号回退（锁序破窗序）

/// 持锁器闭集（与 F56082 注册表同源五器）。
pub const L_NAMES: [&str; 5] = ["mem", "io", "fps", "power", "thermal"];
pub const L_COUNT: usize = 5;

/// 锁态闭集。
pub const LS_FREE: usize = 0;
pub const LS_HELD: usize = 1;
pub const LS_DONE: usize = 2;

/// 锁指纹：FNV-1a 32 位（窗号|器|态复合）。
pub fn lock_fingerprint(window: u32, coll: usize, state: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let name = if coll < L_COUNT { L_NAMES[coll] } else { "?" };
    let line = format!("{}|{}|{}", window, name, state);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 单窗锁账：窗号 × 持锁器 × 锁态。
#[derive(Clone)]
pub struct WinLock {
    pub window: u32,
    pub holder: usize,
    pub state: usize,
}

/// 锁总账（窗序登记 + 持锁史 + 封账）。
pub struct LockBook {
    pub locks: Vec<WinLock>,
    pub history: Vec<String>,
    pub sealed: bool,
}

impl LockBook {
    pub fn new() -> Self {
        LockBook { locks: Vec::new(), history: Vec::new(), sealed: false }
    }

    fn find(&self, window: u32) -> Option<usize> {
        for i in 0..self.locks.len() {
            if self.locks[i].window == window {
                return Some(i);
            }
        }
        None
    }

    /// 加锁：器闭集 + 窗号严格递增 + 锁定中再锁拒三闸。
    pub fn lock(&mut self, window: u32, coll: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        if coll >= L_COUNT {
            return Err(LK_UNK_COLL);
        }
        if let Some(prev) = self.locks.last() {
            if window <= prev.window {
                return Err(LK_BACK);
            }
        }
        match self.find(window) {
            Some(i) => {
                if self.locks[i].state == LS_HELD {
                    return Err(LK_DOUBLE);
                }
                self.locks[i].holder = coll;
                self.locks[i].state = LS_HELD;
            }
            None => {
                self.locks.push(WinLock { window, holder: coll, state: LS_HELD });
            }
        }
        self.history.push(format!("lock|w{}|{}", window, L_NAMES[coll]));
        Ok(())
    }

    /// 解锁：无锁拒 + 越权拒，解锁后置 DONE。
    pub fn unlock(&mut self, window: u32, coll: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        if coll >= L_COUNT {
            return Err(LK_UNK_COLL);
        }
        let idx = match self.find(window) {
            Some(i) => i,
            None => return Err(LK_UNLOCK_FREE),
        };
        if self.locks[idx].state != LS_HELD {
            return Err(LK_UNLOCK_FREE);
        }
        if self.locks[idx].holder != coll {
            return Err(LK_FOREIGN);
        }
        self.locks[idx].state = LS_DONE;
        self.history.push(format!("unlock|w{}|{}", window, L_NAMES[coll]));
        Ok(())
    }

    /// 窗锁查询（空窗显性拒绝）。
    pub fn state_at(&self, window: u32) -> Result<(usize, usize), u32> {
        match self.find(window) {
            Some(i) => Ok((self.locks[i].holder, self.locks[i].state)),
            None => Err(LK_WIN_MISS),
        }
    }

    /// J1：锁账全量（逐窗锁态合法）。
    pub fn locks_ok(&self) -> bool {
        !self.locks.is_empty()
            && self.locks.iter().all(|l| l.holder < L_COUNT && l.state <= LS_DONE)
    }

    /// J2：互斥核（任意时刻 HELD 窗数 ≤ 1：窗序登记下即逐窗唯一持锁）。
    pub fn mutex_ok(&self) -> bool {
        let mut held = 0usize;
        for l in self.locks.iter() {
            if l.state == LS_HELD {
                held += 1;
            }
        }
        held <= 1
    }

    /// J2：持锁史配对核（逐窗 lock/unlock 成对，DONE 窗数 = 成对数）。
    pub fn pair_ok(&self) -> usize {
        let mut done = 0usize;
        for l in self.locks.iter() {
            if l.state == LS_DONE {
                done += 1;
            }
        }
        done
    }

    /// J3：持锁史完整（逐条 lock|/unlock| 前缀）。
    pub fn history_ok(&self) -> bool {
        !self.history.is_empty()
            && self.history.iter().all(|h| h.starts_with("lock|") || h.starts_with("unlock|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐窗复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for l in self.locks.iter() {
            h ^= lock_fingerprint(l.window, l.holder, l.state);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }
}

/// 标准夹具：3 窗流转（mem 锁 1 解 1、io 锁 2 解 2、fps 持 3 未解）。
pub fn standard_book() -> LockBook {
    let mut b = LockBook::new();
    if b.lock(1, 0).is_err() || b.unlock(1, 0).is_err() {
        panic!("standard fixture window1 must flow");
    }
    if b.lock(2, 1).is_err() || b.unlock(2, 1).is_err() {
        panic!("standard fixture window2 must flow");
    }
    if b.lock(3, 2).is_err() {
        panic!("standard fixture window3 must lock");
    }
    b
}

/// 注入核：7 注入必抓（双重加锁/无锁解锁/越权解锁/未知持锁器/窗号回退/封账改锁/空窗查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 双重加锁
    let mut b = LockBook::new();
    if b.lock(1, 0).is_ok() && b.lock(1, 1) == Err(LK_DOUBLE) {
        caught += 1;
    }
    // 2) 无锁解锁（空窗）
    if b.unlock(9, 0) == Err(LK_UNLOCK_FREE) {
        caught += 1;
    }
    // 3) 未知持锁器
    if b.lock(2, 9) == Err(LK_UNK_COLL) {
        caught += 1;
    }
    // 4) 越权解锁
    if b.unlock(1, 1) == Err(LK_FOREIGN) {
        caught += 1;
    }
    // 5) 窗号回退
    if b.unlock(1, 0).is_ok() && b.lock(1, 0) == Err(LK_BACK) {
        caught += 1;
    }
    // 6) 空窗查询
    if b.state_at(9).err() == Some(LK_WIN_MISS) {
        caught += 1;
    }
    // 7) 封账改锁拒
    b.seal();
    if b.lock(3, 2) == Err(LK_SEALED) && b.unlock(1, 0) == Err(LK_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 锁账渲染导出（逐窗一行）。
pub fn render_book(b: &LockBook) -> String {
    let mut out = String::new();
    for l in b.locks.iter() {
        let st = ["free", "held", "done"][l.state];
        out.push_str(&format!("w{}|{}|{}\n", l.window, L_NAMES[l.holder], st));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行三段 + 锁态闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 3
            && (parts[2] == "free" || parts[2] == "held" || parts[2] == "done")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56084_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56084");
    // J1-1 锁账全量（三窗逐窗合法）
    let b = standard_book();
    let ok = b.locks_ok() && b.locks.len() == 3;
    s.add("56084-J1-锁账全量", ok, if ok { "" } else { "锁账残缺" });
    // J1-2 锁态闭集齐
    let ok = b.state_at(1) == Ok((0, LS_DONE)) && b.state_at(3) == Ok((2, LS_HELD));
    s.add("56084-J1-锁态查询", ok, if ok { "" } else { "锁态漂移" });
    // J1-3 持锁器闭集齐
    let ok = L_NAMES.len() == L_COUNT && L_NAMES[0] == "mem";
    s.add("56084-J1-持锁器闭集", ok, if ok { "" } else { "持锁器漂移" });
    // J2-1 互斥核（单 HELD 窗 ≤ 1）
    let ok = b.mutex_ok();
    s.add("56084-J2-互斥核", ok, if ok { "" } else { "互斥破约" });
    // J2-2 双重加锁拒
    let mut b2 = standard_book();
    let ok = b2.lock(3, 3) == Err(LK_DOUBLE);
    s.add("56084-J2-双重加锁拒", ok, if ok { "" } else { "重锁闸失效" });
    // J2-3 越权解锁拒
    let ok = b2.unlock(3, 0) == Err(LK_FOREIGN);
    s.add("56084-J2-越权解锁拒", ok, if ok { "" } else { "越权闸失效" });
    // J2-4 无锁解锁拒
    let ok = b2.unlock(9, 0) == Err(LK_UNLOCK_FREE);
    s.add("56084-J2-无锁解锁拒", ok, if ok { "" } else { "空解锁闸失效" });
    // J2-5 持锁史配对（2 窗 DONE + 1 窗 HELD）
    let ok = b.pair_ok() == 2;
    s.add("56084-J2-持锁史配对", ok, if ok { "" } else { "配对计数漂移" });
    // J3-1 持锁史完整（5 条：2lock+2unlock+1lock）
    let ok = b.history_ok() && b.history.len() == 5;
    s.add("56084-J3-持锁史完整", ok, if ok { "" } else { "持锁史断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56084-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（3 窗解锁后指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b3 = standard_book();
    let _ = b3.unlock(3, 2);
    let ok = b3.seal_fingerprint() != fp0;
    s.add("56084-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账改锁拒
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.lock(4, 3) == Err(LK_SEALED);
    s.add("56084-J3-封账拒改", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.contains("w3|fps|held");
    s.add("56084-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56084-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56084_all_checks_green() {
        let set = run_unx56084_checks();
        assert!(set.all_passed(), "UNX-F56084 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56084_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
