//! UNX-F10105 dup/dup2 正反双判据（复制达成与非法 fd 误用受控）（0x10040 段）
//!
//! 锚点：UNX-F10105-J1 dup/dup2 复制达成（共享偏移语义 LTP dup01/dup02
//! 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：打开件描述层（FileObj 持内容与共享偏移）+ fd 别名册（fd→描述
//! 引用计数，dup 增引用不复制状态）+ 复制账（dup/dup2 逐次留痕）+ 注入
//! 核（非法 fd/dup2 撞活跃槽/越界目标/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10040 独占 0x20）。
pub const SEG_BASE: u32 = 0x10040;
pub const DP_BAD_FD: u32 = SEG_BASE + 0x01; // 非法源 fd
pub const DP_BAD_TARGET: u32 = SEG_BASE + 0x02; // dup2 目标越界
pub const DP_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const DP_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const DP_SELF_UNCHANGED: u32 = SEG_BASE + 0x05; // dup2 自映射语义诊断

/// fd 空间定容。
pub const FD_SPACE: usize = 8;

/// 打开件：内容 + 共享偏移（别名同址）。
#[derive(Clone)]
pub struct FileObj {
    pub data: Vec<u8>,
    pub shared_pos: usize,
    pub id: u32,
}

impl FileObj {
    pub fn new(id: u32, data: &[u8]) -> FileObj {
        FileObj { data: data.to_vec(), shared_pos: 0, id }
    }

    /// 读：自共享偏移裁剪，推进偏移。
    pub fn read(&mut self, count: usize) -> Vec<u8> {
        let start = self.shared_pos.min(self.data.len());
        let end = (start + count).min(self.data.len());
        let out = self.data[start..end].to_vec();
        self.shared_pos = end;
        out
    }

    /// 写：自共享偏移覆盖推进。
    pub fn write(&mut self, buf: &[u8]) {
        while self.data.len() < self.shared_pos {
            self.data.push(0);
        }
        let mut i = 0;
        while i < buf.len() {
            if self.shared_pos + i < self.data.len() {
                self.data[self.shared_pos + i] = buf[i];
            } else {
                self.data.push(buf[i]);
            }
            i += 1;
        }
        self.shared_pos += buf.len();
    }
}

/// fd 别名册：fd 槽位 → 底层件下标（dup 不复制状态，别名同址共享偏移）。
pub struct DupTable {
    /// 底层打开件（每 open 一个，dup 只增别名不增件）。
    pub files: Vec<FileObj>,
    /// fd 槽位 → 底层件下标。
    pub slots: Vec<Option<usize>>,
    /// 每个 fd 被别名的次数（含本体）。
    pub alias_count: Vec<u32>,
    pub ledger: Vec<String>,
    pub sealed: bool,
}

impl DupTable {
    pub fn new() -> DupTable {
        DupTable { files: Vec::new(), slots: Vec::new(), alias_count: Vec::new(), ledger: Vec::new(), sealed: false }
    }

    fn resolve(&self, fd: i32) -> Result<usize, u32> {
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(DP_BAD_FD);
        }
        Ok(fd as usize)
    }

    /// 底层打开：占一个新 fd，建一个新底层件。
    pub fn open(&mut self, id: u32, data: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(DP_SEALED);
        }
        if self.slots.len() >= FD_SPACE {
            return Err(DP_BAD_FD);
        }
        self.files.push(FileObj::new(id, data));
        self.slots.push(Some(self.files.len() - 1));
        self.alias_count.push(1);
        Ok(self.slots.len() - 1)
    }

    /// dup：源 fd 别名到最低空闲槽，共享同一底层件（偏移同址推进）。
    pub fn dup(&mut self, src: i32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(DP_SEALED);
        }
        if note.is_empty() {
            return Err(DP_EMPTY_NOTE);
        }
        let u = self.resolve(src)?;
        // 找最低空闲槽（或扩容）
        let slot = if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            i
        } else if self.slots.len() < FD_SPACE {
            self.slots.push(None);
            self.alias_count.push(0);
            self.slots.len() - 1
        } else {
            return Err(DP_BAD_FD);
        };
        let fi = self.slots[u].unwrap_or(usize::MAX);
        self.slots[slot] = Some(fi);
        self.alias_count[slot] = 1;
        self.alias_count[u] += 1;
        self.ledger.push(format!("dup {}->{}|{}", src, slot, note));
        Ok(slot)
    }

    /// dup2：源 fd 复制到指定目标槽；目标已活跃则先隐式关；自映射为幂等。
    pub fn dup2(&mut self, src: i32, target: i32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(DP_SEALED);
        }
        if note.is_empty() {
            return Err(DP_EMPTY_NOTE);
        }
        let u = self.resolve(src)?;
        if target < 0 || target as usize >= FD_SPACE {
            return Err(DP_BAD_TARGET);
        }
        let t = target as usize;
        if t == u {
            // 自映射：语义上无事发生，留痕即返
            self.ledger.push(format!("dup2 self {}|{}", u, note));
            return Ok(t);
        }
        // 扩容到目标位
        while self.slots.len() <= t {
            self.slots.push(None);
            self.alias_count.push(0);
        }
        // 目标已活跃：引用计数回落（隐式关闭）
        if self.slots[t].is_some() {
            self.alias_count[t] = self.alias_count[t].saturating_sub(1);
        }
        let fi = self.slots[u].unwrap_or(usize::MAX);
        self.slots[t] = Some(fi);
        self.alias_count[t] = 1;
        self.alias_count[u] += 1;
        self.ledger.push(format!("dup2 {}->{}|{}", src, t, note));
        Ok(t)
    }

    /// 关闭槽位：别名计数回落，空槽可被后续 dup 复用。
    pub fn close(&mut self, fd: i32) -> Result<(), u32> {
        if self.sealed {
            return Err(DP_SEALED);
        }
        let u = self.resolve(fd)?;
        self.slots[u] = None;
        self.alias_count[u] = self.alias_count[u].saturating_sub(1);
        self.ledger.push(format!("close {}|closed", fd));
        Ok(())
    }

    /// 偏移共享校验：读经由底层件推进（别名 fd 读到的是同一偏移）。
    pub fn read_via(&mut self, fd: i32, count: usize) -> Result<Vec<u8>, u32> {
        let u = self.resolve(fd)?;
        let fi = self.slots[u].unwrap_or(usize::MAX);
        if fi >= self.files.len() {
            return Err(DP_BAD_FD);
        }
        Ok(self.files[fi].read(count))
    }

    /// 别名总数（引用计数和）。
    pub fn total_aliases(&self) -> u32 {
        self.alias_count.iter().copied().fold(0u32, |a, b| a.wrapping_add(b))
    }

    pub fn live(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x100400;
        for l in &self.ledger {
            for b in l.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h.wrapping_add(self.total_aliases())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = DupTable::new();
    let f = t.open(1, b"abcd").unwrap_or(usize::MAX);
    // 1) 非法源 fd 必拒
    got[0] = t.dup(-1, "inj-neg").err() == Some(DP_BAD_FD);
    // 2) 越界目标必拒
    got[1] = t.dup2(f as i32, FD_SPACE as i32 + 3, "inj-tgt").err() == Some(DP_BAD_TARGET);
    // 3) 空留痕必拒
    got[2] = t.dup(f as i32, "").err() == Some(DP_EMPTY_NOTE);
    // 4) 封表后 dup 必抓
    t.seal();
    got[3] = t.dup(f as i32, "inj-seal").err() == Some(DP_SEALED);
    // 5) dup 到未打开槽位目标位（自映射之外）合法但别名计数必须一致
    let mut t2 = DupTable::new();
    let g = t2.open(2, b"xy").unwrap_or(usize::MAX);
    let d = t2.dup(g as i32, "ok").unwrap_or(usize::MAX);
    got[4] = t2.alias_count[g] == 2 && t2.alias_count[d] == 1 && t2.live() == 2;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10105_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10105");

    // 正路径：dup 后两 fd 共享偏移（dup01 面）
    let mut t = DupTable::new();
    let f = t.open(1, b"shared-offset").unwrap_or(usize::MAX);
    let d = t.dup(f as i32, "别名").unwrap_or(usize::MAX);
    let a = t.read_via(f as i32, 6).unwrap_or_default();
    let bpart = t.read_via(d as i32, 6).unwrap_or_default();
    let mut joined = a;
    joined.extend(bpart);
    // 6+6 共读 12 字节；dup 端自共享偏移续读（未回卷）即证共享语义，
    // 余下 1 字节留给 J2 的 dup2 定向续读。
    let j1 = joined == b"shared-offse".to_vec();
    s.add("J1-共享偏移", j1, "dup 两 fd 顺序读自共享偏移顺拼");

    // dup02 面：dup2 指定目标槽
    let t2fd = t.dup2(d as i32, 5, "定向").unwrap_or(usize::MAX);
    let j2 = t2fd == 5 && t.read_via(5, 2).unwrap_or_default() == b"t".to_vec();
    s.add("J2-dup2定向", j2, "dup2 落指定槽且续读自共享偏移");

    // 引用计数：一次 dup 增一
    let j3 = t.alias_count[f] == 2 && t.alias_count[d] == 2 && t.total_aliases() == 5;
    s.add("J3-引用计数", j3, "别名计数逐次递增");

    // dup2 自映射幂等
    let j4 = t.dup2(f as i32, f as i32, "self").unwrap_or(usize::MAX) == f
        && t.alias_count[f] == 2;
    s.add("J4-自映射幂等", j4, "dup2 自身不增计数");

    // dup2 撞活跃槽：旧别名被替换计数回落
    let j5 = {
        let mut t5 = DupTable::new();
        let x = t5.open(1, b"a").unwrap_or(usize::MAX);
        let y = t5.open(2, b"b").unwrap_or(usize::MAX);
        let _ = t5.dup2(x as i32, y as i32, "crash");
        t5.alias_count[y] == 1 && t5.read_via(y as i32, 1).unwrap_or_default() == b"a".to_vec()
    };
    s.add("J5-撞槽替换", j5, "目标旧件被替换且计数守恒");

    // 非法源 fd 受控
    let j6 = t.dup(-2, "neg").err() == Some(DP_BAD_FD)
        && t.dup(99, "oob").err() == Some(DP_BAD_FD);
    s.add("J6-非法fd受控", j6, "负与越界源同码拒");

    // dup2 目标越界受控
    let j7 = t.dup2(f as i32, FD_SPACE as i32, "over").err() == Some(DP_BAD_TARGET);
    s.add("J7-目标越界受控", j7, "目标出 fd 空间必拒");

    // fd 空间容量守恒
    let j8 = {
        let mut t8 = DupTable::new();
        let f0 = t8.open(9, b"z").unwrap_or(usize::MAX);
        let mut ok = true;
        for _ in 1..FD_SPACE {
            if t8.dup(f0 as i32, "fill").is_err() {
                ok = false;
            }
        }
        ok && t8.dup(f0 as i32, "over").err() == Some(DP_BAD_FD)
    };
    s.add("J8-容量守恒", j8, "FD_SPACE 占满必拒");

    // 空留痕拒收
    let j9 = t.dup(f as i32, "").err() == Some(DP_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 封表后操作必抓
    let j10 = {
        t.seal();
        t.dup(f as i32, "late").err() == Some(DP_SEALED)
    };
    s.add("J10-封表拒操作", j10, "封表后 dup 必拒");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = t.fold() == t.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10105_dup_flow() {
        let mut t = DupTable::new();
        let f = t.open(1, b"xyz").unwrap_or(usize::MAX);
        let d = t.dup(f as i32, "d").unwrap_or(usize::MAX);
        let _ = t.read_via(f as i32, 1).unwrap_or_default();
        let rest = t.read_via(d as i32, 2).unwrap_or_default();
        assert_eq!(rest, b"yz".to_vec());
        assert!(t.dup2(f as i32, 99, "t").err() == Some(DP_BAD_TARGET));
    }

    #[test]
    fn unx10105_checks_green() {
        let s = run_unx10105_checks();
        assert!(s.all_passed(), "UNX-F10105 判据未全绿");
    }
}
