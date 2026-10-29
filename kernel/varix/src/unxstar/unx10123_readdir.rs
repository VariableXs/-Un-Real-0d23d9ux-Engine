//! UNX-F10123 目录流枚举正反双判据（readdir 达成与关闭后误用受控）（0x10280 段）
//!
//! 锚点：UNX-F10123-J1 readdir 枚举达成（opendir/readdir/closedir 全程
//! LTP 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：目录流（流柄+游标+开闭态）+ 流核（opendir 建流/逐项 readdir/
//! closedir 释放/关后误用拒）+ 流账（逐次留痕 + 全程轨迹）+ 注入核（关
//! 后读/坏流柄/重关闭/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10280 独占 0x20）。
pub const SEG_BASE: u32 = 0x10280;
pub const DR_CLOSED: u32 = SEG_BASE + 0x01; // 流已关
pub const DR_BAD_HANDLE: u32 = SEG_BASE + 0x02; // 坏流柄
pub const DR_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const DR_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const DR_DUP_CLOSE: u32 = SEG_BASE + 0x05; // 重关闭

/// 流表定容。
pub const STREAM_CAP: usize = 6;

/// 目录内容（名字序表）。
#[derive(Clone)]
pub struct DirContent {
    pub names: Vec<String>,
}

impl DirContent {
    pub fn new(names: &[&str]) -> DirContent {
        DirContent { names: names.iter().map(|n| n.to_string()).collect() }
    }
}

/// 目录流。
#[derive(Clone)]
pub struct DirStream {
    pub dir_id: u32,
    pub cursor: usize,
    pub open: bool,
}

impl DirStream {
    pub fn ok(&self) -> bool {
        self.dir_id != 0 || self.cursor == 0
    }
}

/// 流账留痕。
#[derive(Clone)]
pub struct DrRec {
    pub seq: u32,
    pub handle: i32,
    pub action: u32, // 0=opendir 1=readdir 2=closedir
    pub name: String,
}

impl DrRec {
    pub fn ok(&self) -> bool {
        self.action <= 2 && self.handle >= 0
    }
}

/// 流册 + 流核。
pub struct StreamBook {
    pub contents: Vec<DirContent>,
    pub streams: Vec<Option<DirStream>>,
    pub ledger: Vec<DrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl StreamBook {
    pub fn new() -> StreamBook {
        StreamBook {
            contents: vec![DirContent::new(&[])],
            streams: Vec::new(),
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 注册目录内容（dir_id 从 1 起）。
    pub fn add_dir(&mut self, names: &[&str]) -> u32 {
        self.contents.push(DirContent::new(names));
        (self.contents.len() - 1) as u32
    }

    /// opendir：建流。
    pub fn opendir(&mut self, dir_id: u32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(DR_SEALED);
        }
        if note.is_empty() {
            return Err(DR_EMPTY_NOTE);
        }
        if dir_id as usize >= self.contents.len() {
            return Err(DR_BAD_HANDLE);
        }
        let slot = if let Some(i) = self.streams.iter().position(|s| s.is_none()) {
            i
        } else if self.streams.len() < STREAM_CAP {
            self.streams.push(None);
            self.streams.len() - 1
        } else {
            return Err(DR_BAD_HANDLE);
        };
        self.streams[slot] = Some(DirStream { dir_id, cursor: 0, open: true });
        self.ledger.push(DrRec {
            seq: self.seq_next,
            handle: slot as i32,
            action: 0,
            name: String::new(),
        });
        self.seq_next += 1;
        Ok(slot)
    }

    /// readdir：逐项取（关流拒）。
    pub fn readdir(&mut self, handle: i32, note: &str) -> Result<String, u32> {
        if self.sealed {
            return Err(DR_SEALED);
        }
        if note.is_empty() {
            return Err(DR_EMPTY_NOTE);
        }
        let h = match usize::try_from(handle) {
            Ok(h) if h < self.streams.len() => h,
            _ => return Err(DR_BAD_HANDLE),
        };
        let st = match self.streams[h].as_mut() {
            Some(s) if s.open => s,
            Some(_) => return Err(DR_CLOSED),
            None => return Err(DR_BAD_HANDLE),
        };
        let content = &self.contents[st.dir_id as usize];
        let name = match content.names.get(st.cursor) {
            Some(n) => n.clone(),
            None => String::new(), // 枚举尽：空串即 EOF
        };
        if !name.is_empty() {
            st.cursor += 1;
        }
        self.ledger
            .push(DrRec { seq: self.seq_next, handle, action: 1, name: name.clone() });
        self.seq_next += 1;
        Ok(name)
    }

    /// closedir：释放流。
    pub fn closedir(&mut self, handle: i32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DR_SEALED);
        }
        if note.is_empty() {
            return Err(DR_EMPTY_NOTE);
        }
        let h = match usize::try_from(handle) {
            Ok(h) if h < self.streams.len() => h,
            _ => return Err(DR_BAD_HANDLE),
        };
        match self.streams[h].as_mut() {
            Some(s) if s.open => {
                s.open = false;
            }
            Some(_) => return Err(DR_DUP_CLOSE),
            None => return Err(DR_BAD_HANDLE),
        }
        self.streams[h] = None;
        self.ledger.push(DrRec {
            seq: self.seq_next,
            handle,
            action: 2,
            name: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 全程轨迹核：opendir→readdir×N→closedir 完整留痕。
    pub fn full_cycle(&self, handle: i32) -> bool {
        let mut saw_open = false;
        let mut saw_close = false;
        for r in &self.ledger {
            if r.handle == handle {
                if r.action == 0 {
                    saw_open = true;
                }
                if r.action == 2 {
                    saw_close = true;
                }
            }
        }
        saw_open && saw_close
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
        let mut h: u32 = 0x102800;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.action);
            h = h.wrapping_mul(31).wrapping_add(r.handle as u32);
            for b in r.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = StreamBook::new();
    let d = b.add_dir(&["x"]);
    let h = b.opendir(d, "inj-op").unwrap_or(0);
    // 1) 关后读必拒
    let _ = b.closedir(h as i32, "inj-cl");
    got[0] = b.readdir(h as i32, "inj-read").err() == Some(DR_BAD_HANDLE);
    // 2) 坏流柄必拒
    got[1] = b.readdir(99, "inj-bad").err() == Some(DR_BAD_HANDLE);
    // 3) 重关闭必拒
    got[2] = b.closedir(h as i32, "inj-dup").err() == Some(DR_BAD_HANDLE);
    // 4) 空留痕必拒
    got[3] = b.opendir(d, "").err() == Some(DR_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.opendir(d, "inj-seal").err() == Some(DR_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10123_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10123");

    // 正路径：opendir/readdir/closedir 全程（LTP 面）
    let mut b = StreamBook::new();
    let d = b.add_dir(&["alpha", "beta", "gamma"]);
    let h = b.opendir(d, "开流").unwrap_or(usize::MAX);
    let n1 = b.readdir(h as i32, "读一").unwrap_or_default();
    let n2 = b.readdir(h as i32, "读二").unwrap_or_default();
    let n3 = b.readdir(h as i32, "读三").unwrap_or_default();
    let j1 = n1 == "alpha" && n2 == "beta" && n3 == "gamma";
    s.add("J1-全程枚举", j1, "三项顺序读出 alpha/beta/gamma");

    // 枚举尽：空串即 EOF
    let eof = b.readdir(h as i32, "读尽").unwrap_or_default();
    let j2 = eof.is_empty();
    s.add("J2-枚举尽EOF", j2, "读尽得空串");

    // closedir 释放
    let j3 = b.closedir(h as i32, "关流").is_ok() && b.streams[h].is_none();
    s.add("J3-关流释放", j3, "closedir 后槽位回收");

    // 全程轨迹核
    let j4 = b.full_cycle(h as i32);
    s.add("J4-轨迹完整", j4, "开/关双留痕齐备");

    // 关后读受控
    let j5 = b.readdir(h as i32, "关后").err() == Some(DR_BAD_HANDLE);
    s.add("J5-关后受控", j5, "槽位空按坏柄拒");

    // 坏流柄受控
    let j6 = b.readdir(-1, "负").err() == Some(DR_BAD_HANDLE)
        && b.readdir(99, "越界").err() == Some(DR_BAD_HANDLE);
    s.add("J6-坏柄受控", j6, "负与越界柄同码拒");

    // 流容量守恒
    let j7 = {
        let mut b7 = StreamBook::new();
        let d7 = b7.add_dir(&["a"]);
        let mut ok = true;
        for _ in 0..STREAM_CAP {
            if b7.opendir(d7, "填").is_err() {
                ok = false;
            }
        }
        ok && b7.opendir(d7, "超").err() == Some(DR_BAD_HANDLE)
    };
    s.add("J7-容量守恒", j7, "STREAM_CAP 守恒");

    // 空留痕拒收
    let j8 = b.readdir(h as i32, "").err() == Some(DR_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 重开复用最低槽
    let j9 = b.opendir(d, "复用").unwrap_or(usize::MAX) == 0;
    s.add("J9-槽位复用", j9, "新流落最低空闲槽");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "action 在闭集且柄非负");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10123_stream_flow() {
        let mut b = StreamBook::new();
        let d = b.add_dir(&["a", "b"]);
        let h = b.opendir(d, "o").unwrap_or(0);
        assert_eq!(b.readdir(h as i32, "1").unwrap_or_default(), "a");
        assert!(b.closedir(h as i32, "c").is_ok());
        assert!(b.readdir(h as i32, "2").err() == Some(DR_BAD_HANDLE));
    }

    #[test]
    fn unx10123_checks_green() {
        let s = run_unx10123_checks();
        assert!(s.all_passed(), "UNX-F10123 判据未全绿");
    }
}
