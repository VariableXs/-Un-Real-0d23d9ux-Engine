//! UNX-F12155 — 展开路径锁序巡检对接（A5 死锁检测器联签）（0x1a580 段）
//!
//! 锚点：UNX-F12155-J1 展开路径锁序 10/10（handler 求值期间持锁
//! 序必须单调升序，倒序持锁 = A5 检测器联签红线），巡检留痕全账
//! 自洽。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：LockHold（持锁记录：锁 id + 阶段）+ PathPatrol（展开路径
//! 锁序巡检器）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a580;
pub const PP_ORDER: u32 = SEG_BASE + 0x01; // 锁序倒挂
pub const PP_TWICE: u32 = SEG_BASE + 0x02; // 重复持锁
pub const PP_GHOST: u32 = SEG_BASE + 0x03; // 释放未持锁
pub const PP_SEALED: u32 = SEG_BASE + 0x04; // 印后巡检
pub const PP_LEAK: u32 = SEG_BASE + 0x05; // 路径结束持锁未释

/// 持锁记录。
#[derive(Clone, PartialEq)]
pub struct LockHold {
    pub lock_id: u32,
    pub held: bool,
    pub note: String,
}

/// 锁序巡检器（展开路径维度）。
pub struct PathPatrol {
    pub holds: Vec<LockHold>,
    pub paths: Vec<(u32, usize)>, // (路径号, 违例数)
    cur_path: u32,
    sealed: bool,
}

impl PathPatrol {
    pub fn new() -> PathPatrol {
        PathPatrol { holds: Vec::new(), paths: Vec::new(), cur_path: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn cur_path(&self) -> u32 {
        self.cur_path
    }

    /// 持锁（锁序必须大于当前所有已持锁 = 升序持锁；重复持锁必拒）。
    pub fn acquire(&mut self, lock_id: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PP_SEALED);
        }
        if note.is_empty() {
            return Err(PP_GHOST);
        }
        if self.holds.iter().any(|h| h.held && h.lock_id == lock_id) {
            return Err(PP_TWICE);
        }
        if self.holds.iter().any(|h| h.held && h.lock_id > lock_id) {
            self.paths.push((self.cur_path, 1));
            return Err(PP_ORDER);
        }
        self.holds.push(LockHold { lock_id, held: true, note: note.to_string() });
        Ok(())
    }

    /// 释锁。
    pub fn release(&mut self, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PP_SEALED);
        }
        match self.holds.iter_mut().find(|h| h.held && h.lock_id == lock_id) {
            Some(h) => {
                h.held = false;
                Ok(())
            }
            None => Err(PP_GHOST),
        }
    }

    /// 换路径（旧路径残留持锁 = 泄漏）。
    pub fn switch_path(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(PP_SEALED);
        }
        let leaked = self.holds.iter().filter(|h| h.held).count();
        if leaked > 0 {
            self.paths.push((self.cur_path, leaked));
            let ids: Vec<u32> = self.holds.iter().filter(|h| h.held).map(|h| h.lock_id).collect();
            for id in ids {
                let _ = self.release(id);
            }
            self.cur_path += 1;
            return Err(PP_LEAK);
        }
        self.cur_path += 1;
        Ok(())
    }

    /// 当前持锁序是否升序（真源校验）。
    pub fn held_ascending(&self) -> bool {
        let ids: Vec<u32> = self.holds.iter().filter(|h| h.held).map(|h| h.lock_id).collect();
        ids.windows(2).all(|w| w[0] < w[1])
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a580;
        for hd in &self.holds {
            h = h.wrapping_mul(211).wrapping_add(hd.lock_id);
            h = h.wrapping_mul(211).wrapping_add(hd.held as u32);
            for by in hd.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "holds={} held={} paths={}",
            self.holds.len(),
            self.holds.iter().filter(|h| h.held).count(),
            self.paths.len()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = PathPatrol::new();
    // 1) 倒序持锁必拒。
    let _ = b.acquire(10, "L10");
    got[0] = b.acquire(5, "L5").err() == Some(PP_ORDER);
    // 2) 重复持锁必拒。
    got[1] = b.acquire(10, "dup").err() == Some(PP_TWICE);
    // 3) 释放未持锁必拒。
    got[2] = b.release(99).err() == Some(PP_GHOST);
    // 4) 路径结束持锁未释 = 泄漏。
    got[3] = b.switch_path().err() == Some(PP_LEAK) && b.paths.len() == 2;
    // 5) 留痕空持锁必拒。
    got[4] = b.acquire(20, "").err() == Some(PP_GHOST);
    got
}

/// 判据入口：UNX-F12155
pub fn run_unx12155_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12155");
    let mut b = PathPatrol::new();

    // J1 升序持锁 4 把。
    let j1 = b.acquire(2, "ldr").is_ok()
        && b.acquire(5, "heap").is_ok()
        && b.acquire(8, "tbl").is_ok()
        && b.acquire(11, "dbg").is_ok();
    s.add("J1_asc4", j1 && b.held_ascending(), "四锁升序全持");

    // J2 倒序持锁拒。
    s.add(
        "J2_inverted",
        b.acquire(3, "low").err() == Some(PP_ORDER),
        "低于现持最大锁 id 拒 ORDER",
    );

    // J3 重复持锁拒。
    s.add(
        "J3_twice",
        b.acquire(8, "dup").err() == Some(PP_TWICE),
        "重复持锁拒 TWICE",
    );

    // J4 逆序释放合法（LIFO 释放不查序）。
    let r4 = b.release(11);
    s.add(
        "J4_release_any",
        r4.is_ok() && b.release(11).err() == Some(PP_GHOST),
        "释放持锁成功、再释拒 GHOST",
    );

    // J5 释放未持锁拒。
    s.add(
        "J5_ghost_release",
        b.release(42).err() == Some(PP_GHOST),
        "未持锁释放拒 GHOST",
    );

    // J6 路径结束全释干净（先释净 J1~J5 残留持锁）。
    let _ = b.release(2);
    let _ = b.release(5);
    let _ = b.release(8);
    let r6 = b.switch_path();
    s.add(
        "J6_clean_path",
        r6.is_ok() && b.cur_path() == 2 && b.holds.iter().all(|h| !h.held),
        "干净换路径 cur=2",
    );

    // J7 泄漏路径记账。
    let _ = b.acquire(4, "leak");
    let r7 = b.switch_path();
    s.add(
        "J7_leak",
        r7.err() == Some(PP_LEAK) && b.paths.last().map(|&(p, n)| p == 2 && n == 1) == Some(true),
        "泄漏路径记 (path=2, n=1)",
    );

    // J8 泄漏后自动清持锁。
    s.add(
        "J8_leak_cleaned",
        b.holds.iter().all(|h| !h.held),
        "泄漏路径持锁全清",
    );

    // J9 持锁留痕对平（held 位次）。
    let held_note = b.holds.iter().find(|h| h.lock_id == 4).unwrap().note.clone();
    s.add(
        "J9_note_trace",
        held_note == "leak",
        "持锁留痕备注对平",
    );

    // J10 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J10_fingerprint", f1 == f2, "巡检器折叠指纹一致");

    // J11 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 印账受控。
    b.seal();
    s.add(
        "J12_sealed",
        b.acquire(30, "post").err() == Some(PP_SEALED)
            && b.switch_path().err() == Some(PP_SEALED),
        "印账后持锁/换路径皆拒",
    );

    // J13 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J13_summary",
        sm.contains("paths=2") && sm.contains("held=0"),
        "摘要含持锁数与路径数",
    );

    // J14 十锁全升序持有再全释。
    let mut b14 = PathPatrol::new();
    let mut ok14 = true;
    for i in 1..=10u32 {
        if b14.acquire(i, "m").is_err() {
            ok14 = false;
        }
    }
    let asc14 = b14.held_ascending();
    for i in 1..=10u32 {
        let _ = b14.release(i);
    }
    s.add(
        "J14_ten_locks",
        ok14 && asc14 && b14.holds.iter().all(|h| !h.held),
        "十锁持/释闭环",
    );

    // J15 泄漏计数多锁。
    let mut b15 = PathPatrol::new();
    let _ = b15.acquire(1, "a");
    let _ = b15.acquire(2, "b");
    let _ = b15.acquire(3, "c");
    let _ = b15.switch_path();
    s.add(
        "J15_leak3",
        b15.paths.last().map(|&(_, n)| n) == Some(3),
        "三锁泄漏计 3",
    );

    // J16 换路径序号递增。
    s.add("J16_path_seq", b15.cur_path() == 2, "泄漏后路径号 2");

    // J17 留痕空持锁拒。
    let mut b17 = PathPatrol::new();
    s.add(
        "J17_empty_note",
        b17.acquire(1, "").err() == Some(PP_GHOST),
        "留痕空拒 GHOST",
    );

    // J18 空巡检器指纹稳定。
    let b18 = PathPatrol::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空巡检器指纹稳定");

    // J19 升序真源校验（持 2/9 序真）。
    let mut b19 = PathPatrol::new();
    let _ = b19.acquire(2, "a");
    let _ = b19.acquire(9, "b");
    s.add("J19_asc_true", b19.held_ascending(), "持 2/9 升序真");

    // J20 泄漏记录多路径累计。
    let mut b20 = PathPatrol::new();
    let _ = b20.acquire(1, "x");
    let _ = b20.switch_path();
    let _ = b20.acquire(1, "y");
    let _ = b20.switch_path();
    s.add(
        "J20_paths2",
        b20.paths.len() == 2 && b20.cur_path() == 3,
        "两路径泄漏累计、路径号 3",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12155_patrol() {
        let mut b = PathPatrol::new();
        let _ = b.acquire(1, "a");
        let _ = b.acquire(2, "b");
        assert!(b.held_ascending());
        assert_eq!(b.acquire(1, "dup"), Err(PP_TWICE));
    }

    #[test]
    fn unx12155_checks_green() {
        let s = run_unx12155_checks();
        assert!(s.all_passed(), "UNX-F12155 判据未全绿");
    }

    #[test]
    fn unx12155_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
