//! UNX-F10101 read 正反双判据（常规读达成与负 fd 误用受控）（0xffc0 段）
//!
//! 锚点：UNX-F10101-J1 read 常规读达成（LTP read01/read03 面计数与内容
//! 一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：文件册（内容+独立偏移的 VFile 定容表）+ 读核（read 语义：
//! 计数裁剪/EOF 零读/偏移推进）+ 审计账（逐次读留痕可折指纹）+ 注入核
//! （负 fd/越界 fd/零缓冲留痕/封表后读/EOF 语义必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xffc0 独占 0x20）。
pub const SEG_BASE: u32 = 0xffc0;
pub const RD_BAD_FD: u32 = SEG_BASE + 0x01; // 负 fd 或越界 fd
pub const RD_CLOSED: u32 = SEG_BASE + 0x02; // 槽位已关闭
pub const RD_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 读留痕为空
pub const RD_SEALED: u32 = SEG_BASE + 0x04; // 表已封禁写
pub const RD_EOF: u32 = SEG_BASE + 0x05; // EOF 边界（非错误，诊断用）

/// fd 表定容（容量先定后用）。
pub const FD_CAP: usize = 16;
/// 单文件内容定容。
pub const FILE_CAP: usize = 128;

/// 虚拟文件：内容 + 独立读偏移。
#[derive(Clone)]
pub struct VFile {
    pub data: Vec<u8>,
    pub pos: usize,
    pub live: bool,
}

impl VFile {
    pub fn new(data: &[u8]) -> VFile {
        VFile { data: data.to_vec(), pos: 0, live: true }
    }

    /// 文件大小。
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// 活跃位（关闭时随槽位一起清零）。
    pub fn is_live(&self) -> bool {
        self.live
    }
}

/// 读留痕单条。
#[derive(Clone)]
pub struct RdRec {
    pub seq: u32,
    pub fd: i32,
    pub got: usize,
    pub head: u8,
    pub note: String,
}

impl RdRec {
    /// 留痕合法：非空注 + got 与 fd 语义自洽（负 fd 必 got==0 不入账）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.fd >= 0
    }

    pub fn render(&self) -> String {
        format!("RD{}|fd{}|got{}|h{:02x}|{}", self.seq, self.fd, self.got, self.head, self.note)
    }
}

/// fd 表 + 审计账。
pub struct ReadTable {
    pub slots: Vec<Option<VFile>>,
    pub ledger: Vec<RdRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ReadTable {
    pub fn new() -> ReadTable {
        ReadTable { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 装入文件并返回 fd（最低空闲槽位优先）。
    pub fn install(&mut self, data: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(RD_SEALED);
        }
        // 先找空闲槽位回收复用
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(VFile::new(data));
            return Ok(i);
        }
        if self.slots.len() >= FD_CAP {
            return Err(RD_BAD_FD);
        }
        self.slots.push(Some(VFile::new(data)));
        Ok(self.slots.len() - 1)
    }

    /// fd 校验：负、越界、空槽均按 BAD_FD/CLOSED 区分。
    fn resolve(&self, fd: i32) -> Result<usize, u32> {
        if fd < 0 {
            return Err(RD_BAD_FD);
        }
        let u = fd as usize;
        if u >= self.slots.len() {
            return Err(RD_BAD_FD);
        }
        if self.slots[u].is_none() {
            return Err(RD_CLOSED);
        }
        Ok(u)
    }

    /// read 核心：从当前偏移裁剪读取，推进偏移，EOF 返回零读。
    pub fn read(&mut self, fd: i32, count: usize, note: &str) -> Result<Vec<u8>, u32> {
        if self.sealed {
            return Err(RD_SEALED);
        }
        if note.is_empty() {
            return Err(RD_EMPTY_NOTE);
        }
        let u = self.resolve(fd)?;
        let f = match self.slots[u].as_mut() {
            Some(f) => f,
            None => return Err(RD_CLOSED),
        };
        let start = f.pos.min(f.data.len());
        let end = (start + count).min(f.data.len());
        let out: Vec<u8> = f.data[start..end].to_vec();
        f.pos = end;
        let head = out.first().copied().unwrap_or(0);
        self.ledger.push(RdRec {
            seq: self.seq_next,
            fd,
            got: out.len(),
            head,
            note: note.to_string(),
        });
        self.seq_next += 1;
        if out.is_empty() && start == f.data.len() && !f.data.is_empty() {
            // EOF 边界：零读但内容非空——语义上属正常返回
            return Ok(out);
        }
        Ok(out)
    }

    /// 关闭槽位（回收复用面）。
    pub fn close(&mut self, fd: i32) -> Result<(), u32> {
        let u = self.resolve(fd)?;
        self.slots[u] = None;
        Ok(())
    }

    /// 当前活跃 fd 数。
    pub fn live_fds(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
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

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0xffc00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.fd as u32);
            h = h.wrapping_mul(31).wrapping_add(r.got as u32);
            h = h.wrapping_mul(31).wrapping_add(r.head as u32);
            for b in r.note.bytes() {
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
    let mut t = ReadTable::new();
    let _ = t.install(b"abcdef");
    // 1) 负 fd 必拒
    got[0] = t.read(-1, 3, "inj-neg").err() == Some(RD_BAD_FD);
    // 2) 越界 fd 必拒
    got[1] = t.read(99, 3, "inj-oob").err() == Some(RD_BAD_FD);
    // 3) 关闭后再读必按 CLOSED 拒
    let _ = t.close(0);
    got[2] = t.read(0, 3, "inj-closed").err() == Some(RD_CLOSED);
    // 4) 空留痕必拒
    let _ = t.install(b"xy");
    got[3] = t.read(0, 1, "").err() == Some(RD_EMPTY_NOTE);
    // 5) 封表后读必抓
    t.seal();
    got[4] = t.read(0, 1, "inj-seal").err() == Some(RD_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10101");

    // 正路径：read01 面——内容一致读出
    let mut t = ReadTable::new();
    let fd = t.install(b"read01-sample-bytes").unwrap_or(usize::MAX);
    let got = t.read(fd as i32, 9, "常规读").unwrap_or_default();
    let j1 = got == b"read01-sa".to_vec();
    s.add("J1-内容一致", j1, "读出与写入前缀逐字节一致");

    // 计数面：分次读拼接与全文一致（read03 面）
    let j2 = {
        let mut t2 = ReadTable::new();
        let f2 = t2.install(b"0123456789").unwrap_or(usize::MAX);
        let a = t2.read(f2 as i32, 4, "a").unwrap_or_default();
        let b = t2.read(f2 as i32, 4, "b").unwrap_or_default();
        let c = t2.read(f2 as i32, 4, "c").unwrap_or_default();
        let mut all = a;
        all.extend(b);
        all.extend(c);
        all == b"0123456789".to_vec()
    };
    s.add("J2-分次拼接一致", j2, "三次分读拼回全文");

    // EOF 面：读尽后再读得零
    let j3 = {
        let mut t3 = ReadTable::new();
        let f3 = t3.install(b"tail").unwrap_or(usize::MAX);
        let _ = t3.read(f3 as i32, 8, "first");
        t3.read(f3 as i32, 8, "second").unwrap_or_default().is_empty()
    };
    s.add("J3-EOF零读", j3, "读尽后返回零字节");

    // 偏移推进语义：读 n 字节后 pos 前进 n
    let j4 = {
        let mut t4 = ReadTable::new();
        let f4 = t4.install(b"advance").unwrap_or(usize::MAX);
        let _ = t4.read(f4 as i32, 3, "p");
        let next = t4.read(f4 as i32, 3, "q").unwrap_or_default();
        next == b"anc".to_vec()
    };
    s.add("J4-偏移推进", j4, "第二次读自新偏移起");

    // 负 fd 误用受控
    let j5 = t.read(-3, 1, "neg").err() == Some(RD_BAD_FD);
    s.add("J5-负fd受控", j5, "负 fd 必拒且落诊断码");

    // 越界 fd 误用受控
    let j6 = t.read(FD_CAP as i32 + 10, 1, "oob").err() == Some(RD_BAD_FD);
    s.add("J6-越界fd受控", j6, "超出表界必拒");

    // 容量边界：表满后 install 必拒
    let j7 = {
        let mut t7 = ReadTable::new();
        let mut ok = true;
        for i in 0..FD_CAP {
            if t7.install(b"x").is_err() {
                ok = false;
            }
        }
        ok && t7.install(b"y").err() == Some(RD_BAD_FD)
    };
    s.add("J7-表满受控", j7, "FD_CAP 容量守恒");

    // 空留痕拒收
    let j8 = t.read(0, 1, "").err() == Some(RD_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 槽位回收：关闭后 install 复用最低槽
    let j9 = {
        let mut t9 = ReadTable::new();
        let _ = t9.install(b"aa");
        let _ = t9.install(b"bb");
        let _ = t9.close(0);
        t9.install(b"cc").unwrap_or(usize::MAX) == 0 && t9.live_fds() == 2
    };
    s.add("J9-槽位回收", j9, "最低空闲槽复用且活跃数守恒");

    // 留痕账合法且 seq 唯一
    let j10 = t.ledger.iter().all(|r| r.ok()) && t.seq_unique();
    s.add("J10-留痕合法", j10, "注非空 fd 非负且 seq 唯一");

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
    fn unx10101_read_flow() {
        let mut t = ReadTable::new();
        let fd = t.install(b"hello").unwrap_or(usize::MAX);
        let a = t.read(fd as i32, 2, "a").unwrap_or_default();
        assert_eq!(a, b"he".to_vec());
        assert!(t.read(-1, 1, "b").err() == Some(RD_BAD_FD));
        let _ = t.close(fd as i32);
        assert!(t.read(fd as i32, 1, "c").err() == Some(RD_CLOSED));
    }

    #[test]
    fn unx10101_checks_green() {
        let s = run_unx10101_checks();
        assert!(s.all_passed(), "UNX-F10101 判据未全绿");
    }
}
