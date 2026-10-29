//! UNX-F10321 联签开卷总纲（pty/进程/基建三面齐备）（0x1b2a0 段）
//!
//! 锚点：UNX-F10321-J1 三面齐备——pty/进程/基建三面各按需求锚登记开卷
//! 条目（键+题名），三面计数同时达锚才算总纲齐备，齐备后方可开卷。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：三面册（面号 0..2，条目=面号+键+题名，键在同面内唯一、跨面
//! 可复用）+ 需求锚（pty 三条/进程四条/基建三条）+ 开卷闸（三面齐备
//! 方可开卷，开卷一次且此后拒加）+ 注入核（坏面号/同面重键/空题名/
//! 超容四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b2a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b2a0;
pub const TC_FACE: u32 = SEG_BASE + 0x01; // 面号越界
pub const TC_DUP: u32 = SEG_BASE + 0x02; // 同面键重号
pub const TC_EMPTY: u32 = SEG_BASE + 0x03; // 题名空
pub const TC_CAP: u32 = SEG_BASE + 0x04; // 总纲超容
pub const TC_NOTREADY: u32 = SEG_BASE + 0x05; // 三面未齐即开卷
pub const TC_OPENED: u32 = SEG_BASE + 0x06; // 已开卷再开卷
pub const TC_SEALED: u32 = SEG_BASE + 0x07; // 开卷后加条

/// 三面号。
pub const FACE_PTY: u8 = 0;
pub const FACE_PROC: u8 = 1;
pub const FACE_INFRA: u8 = 2;
/// 面数上界（开区间）。
pub const FACE_N: u8 = 3;

/// 各面需求锚：pty 三条 / 进程四条 / 基建三条。
pub const FACE_NEED: [usize; 3] = [3, 4, 3];
/// 总纲总容量。
pub const CAP: usize = 14;

/// 开卷条目。
#[derive(Clone)]
pub struct CharterEntry {
    pub face: u8,
    pub key: String,
    pub name: String,
}

/// 三面开卷总纲册。
pub struct TriCharter {
    pub entries: Vec<CharterEntry>,
    pub opened: bool,
}

impl TriCharter {
    pub fn new() -> TriCharter {
        TriCharter { entries: Vec::new(), opened: false }
    }

    fn hit(&self, face: u8, key: &str) -> bool {
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            if e.face == face && e.key == key {
                return true;
            }
            k += 1;
        }
        false
    }

    /// 某面条目数。
    pub fn count_face(&self, face: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].face == face {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 登记开卷条目：坏面号/同面重键/空题名/超容均拒。
    pub fn add(&mut self, face: u8, key: &str, name: &str) -> Result<(), u32> {
        if face >= FACE_N {
            return Err(TC_FACE);
        }
        if self.opened {
            return Err(TC_SEALED);
        }
        if self.hit(face, key) {
            return Err(TC_DUP);
        }
        if key.is_empty() || name.is_empty() {
            return Err(TC_EMPTY);
        }
        if self.entries.len() >= CAP {
            return Err(TC_CAP);
        }
        self.entries.push(CharterEntry {
            face,
            key: key.to_string(),
            name: name.to_string(),
        });
        Ok(())
    }

    /// 单面齐备。
    pub fn face_ready(&self, face: u8) -> bool {
        if face >= FACE_N {
            return false;
        }
        self.count_face(face) >= FACE_NEED[face as usize]
    }

    /// 三面齐备。
    pub fn three_ready(&self) -> bool {
        let mut f = 0u8;
        while f < FACE_N {
            if !self.face_ready(f) {
                return false;
            }
            f += 1;
        }
        true
    }

    /// 开卷：三面齐备且未开过，返回总纲指纹。
    pub fn open(&mut self) -> Result<u32, u32> {
        if self.opened {
            return Err(TC_OPENED);
        }
        if !self.three_ready() {
            return Err(TC_NOTREADY);
        }
        self.opened = true;
        Ok(self.fold())
    }

    /// 某面已登记键清单（按登记序）。
    pub fn keys_of(&self, face: u8) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].face == face {
                v.push(self.entries[k].key.clone());
            }
            k += 1;
        }
        v
    }

    /// 总纲指纹（登记序敏感）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b2a00;
        h = h.wrapping_mul(31).wrapping_add(self.entries.len() as u32);
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = h.wrapping_mul(31).wrapping_add(e.face as u32);
            let kb = e.key.as_bytes();
            let mut i = 0usize;
            while i < kb.len() {
                h = h.wrapping_mul(31).wrapping_add(kb[i] as u32);
                i += 1;
            }
            let nb = e.name.as_bytes();
            let mut i = 0usize;
            while i < nb.len() {
                h = h.wrapping_mul(31).wrapping_add(nb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.opened as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut c = TriCharter::new();
    // 1) 坏面号必拒
    got[0] = c.add(3, "x", "y").err() == Some(TC_FACE);
    // 2) 同面重键必拒
    let _ = c.add(FACE_PTY, "termios", "行规程");
    got[1] = c.add(FACE_PTY, "termios", "重键").err() == Some(TC_DUP);
    // 3) 空题名必拒
    got[2] = c.add(FACE_PTY, "winsize", "").err() == Some(TC_EMPTY);
    // 4) 超容必拒
    let mut full = TriCharter::new();
    let mut n = 0u8;
    while (n as usize) < CAP {
        let key = format!("k{}", n);
        let _ = full.add(n % FACE_N, &key, "填");
        n += 1;
    }
    got[3] = full.add(FACE_PTY, "over", "超").err() == Some(TC_CAP);
    got
}

/// 三面样例键名（pty/进程/基建）。
pub fn charter_samples() -> Vec<(u8, &'static str, &'static str)> {
    let mut v: Vec<(u8, &'static str, &'static str)> = Vec::new();
    v.push((FACE_PTY, "termios", "行规程判据"));
    v.push((FACE_PTY, "winsize", "窗尺寸判据"));
    v.push((FACE_PTY, "ctty", "控制终端判据"));
    v.push((FACE_PROC, "fork", "派生判据"));
    v.push((FACE_PROC, "exec", "替换映像判据"));
    v.push((FACE_PROC, "wait", "回收判据"));
    v.push((FACE_PROC, "exit", "退出码判据"));
    v.push((FACE_INFRA, "sched", "调度判据"));
    v.push((FACE_INFRA, "mem", "内存判据"));
    v.push((FACE_INFRA, "irq", "中断判据"));
    v
}

/// 批内判据。
pub fn run_unx10321_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10321");

    // J1 正路径：三面样例登记齐备并开卷
    let mut c = TriCharter::new();
    let samples = charter_samples();
    let mut ok = true;
    let mut k = 0usize;
    while k < samples.len() {
        let (f, key, name) = (samples[k].0, samples[k].1, samples[k].2);
        if c.add(f, key, name).is_err() {
            ok = false;
        }
        k += 1;
    }
    let opened = c.open();
    s.add(
        "J1-三面齐备开卷",
        ok && c.three_ready() && opened.is_ok() && c.opened,
        "pty/进程/基建三面达锚并成功开卷",
    );

    // J2 逐面计数达锚
    let j2 = c.count_face(FACE_PTY) == FACE_NEED[0]
        && c.count_face(FACE_PROC) == FACE_NEED[1]
        && c.count_face(FACE_INFRA) == FACE_NEED[2];
    s.add("J2-逐面达锚", j2, "三面计数分别为 3/4/3");

    // J3 注入检出：坏面号 + 同面重键
    let inj = inject_four();
    s.add("J3-面号重键检出", inj[0] && inj[1], "坏面号与同面重键必拒");

    // J4 注入检出：空题名 + 超容
    s.add("J4-空名超容检出", inj[2] && inj[3], "空题名与超容必拒");

    // J5 单面缺条不算齐备（pty 缺一条）
    let j5 = {
        let mut t = TriCharter::new();
        let _ = t.add(FACE_PTY, "termios", "行规程");
        let _ = t.add(FACE_PTY, "winsize", "窗尺寸");
        let _ = t.add(FACE_PROC, "fork", "派生");
        let _ = t.add(FACE_PROC, "exec", "替换");
        let _ = t.add(FACE_PROC, "wait", "回收");
        let _ = t.add(FACE_PROC, "exit", "退出");
        let _ = t.add(FACE_INFRA, "sched", "调度");
        let _ = t.add(FACE_INFRA, "mem", "内存");
        let _ = t.add(FACE_INFRA, "irq", "中断");
        !t.three_ready()
    };
    s.add("J5-缺条不齐", j5, "任一面缺一条即不算三面齐备");

    // J6 未齐开卷必拒
    let j6 = TriCharter::new().open().err() == Some(TC_NOTREADY);
    s.add("J6-未齐拒开", j6, "三面未齐时开卷必拒");

    // J7 重复开卷必拒
    let j7 = {
        let mut t = c.clone_book();
        let _ = t.open();
        t.open().err() == Some(TC_OPENED)
    };
    s.add("J7-重开拒收", j7, "开卷只许一次");

    // J8 开卷后加条必拒
    let j8 = {
        let mut t = c.clone_book();
        t.add(FACE_PTY, "late", "迟条").err() == Some(TC_SEALED)
    };
    s.add("J8-开卷拒加", j8, "开卷后登记必拒");

    // J9 跨面同键允许（不同面可复用键）
    let j9 = {
        let mut t = TriCharter::new();
        t.add(FACE_PTY, "存活", "pty 存活").is_ok()
            && t.add(FACE_PROC, "存活", "进程存活").is_ok()
    };
    s.add("J9-跨面复用", j9, "同键跨面不判重号");

    // J10 键清单按登记序返回
    let j10 = {
        let ks = c.keys_of(FACE_PTY);
        ks.len() == 3 && ks[0] == "termios" && ks[1] == "winsize" && ks[2] == "ctty"
    };
    s.add("J10-键序一致", j10, "键清单保持登记顺序");

    // J11 指纹随登记推进且开卷后稳定
    let j11 = {
        let mut t = TriCharter::new();
        let f0 = t.fold();
        let _ = t.add(FACE_PTY, "a", "甲");
        let f1 = t.fold();
        let _ = t.add(FACE_PTY, "b", "乙");
        let f2 = t.fold();
        let _ = t.add(FACE_PROC, "c", "丙");
        let _ = t.add(FACE_PROC, "d", "丁");
        let _ = t.add(FACE_PROC, "e", "戊");
        let _ = t.add(FACE_PROC, "g", "庚");
        let _ = t.add(FACE_INFRA, "h", "辛");
        let _ = t.add(FACE_INFRA, "i", "壬");
        let _ = t.add(FACE_INFRA, "j", "癸");
        let f9 = t.fold();
        let _ = t.open();
        let f10 = t.fold();
        f0 != f1 && f1 != f2 && f2 != f9 && f10 != f9 && t.fold() == f10
    };
    s.add("J11-指纹推进", j11, "每次登记与开卷均改指纹且终态稳定");

    // J12 动态明细：总纲条目总数
    let total = c.entries.len();
    s.add_dyn("J12-条目总数", total == 10, &format!("entries={}", total));

    // J13 动态明细：总纲指纹落账
    let fp = c.fold();
    s.add_dyn("J13-总纲指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

/// 克隆整册（判据/测试复用）。
impl TriCharter {
    pub fn clone_book(&self) -> TriCharter {
        TriCharter { entries: self.entries.clone(), opened: self.opened }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10321_charter_flow() {
        let mut c = TriCharter::new();
        assert!(c.add(FACE_PTY, "termios", "行规程").is_ok());
        assert!(c.add(FACE_PTY, "termios", "重键").err() == Some(TC_DUP));
        assert!(c.add(9, "x", "y").err() == Some(TC_FACE));
        assert!(c.add(FACE_PTY, "winsize", "").err() == Some(TC_EMPTY));
        assert!(!c.three_ready());
        assert!(c.open().err() == Some(TC_NOTREADY));
        let _ = c.add(FACE_PTY, "ctty", "控制终端");
        let _ = c.add(FACE_PROC, "fork", "派生");
        let _ = c.add(FACE_PROC, "exec", "替换");
        let _ = c.add(FACE_PROC, "wait", "回收");
        let _ = c.add(FACE_PROC, "exit", "退出");
        let _ = c.add(FACE_INFRA, "sched", "调度");
        let _ = c.add(FACE_INFRA, "mem", "内存");
        let _ = c.add(FACE_INFRA, "irq", "中断");
        assert!(c.three_ready());
        assert!(c.open().is_ok());
        assert!(c.open().err() == Some(TC_OPENED));
        assert!(c.add(FACE_PTY, "late", "迟").err() == Some(TC_SEALED));
    }

    #[test]
    fn unx10321_checks_green() {
        let s = run_unx10321_checks();
        assert!(s.all_passed(), "UNX-F10321 判据未全绿");
    }
}
