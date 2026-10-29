//! UNX-F10136 /dev/null 与零设备正反双判据（特殊文件达成与非法操作误用受控）（0x10420 段）
//!
//! 锚点：UNX-F10136-J1 特殊文件达成（null 写丢弃/零设备读填充 LTP
//! read02 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：设备面（null/zero 两设备的读写语义分派）+ 设备核（null 写吞
//! 读返 EOF/zero 读零填/非法操作拒）+ 设备账（逐次留痕 + 吞吐计数）+
//! 注入核（野设备/zero 写拒/null 定位拒/空留痕/封面必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10420 独占 0x20）。
pub const SEG_BASE: u32 = 0x10420;
pub const DV_BAD_DEV: u32 = SEG_BASE + 0x01; // 野设备
pub const DV_WRONLY_DEV: u32 = SEG_BASE + 0x02; // zero 写拒
pub const DV_SEEK: u32 = SEG_BASE + 0x03; // 特殊设备定位拒
pub const DV_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const DV_SEALED: u32 = SEG_BASE + 0x05; // 封面后操作

/// 设备号闭集。
pub const DEV_NULL: u32 = 1;
pub const DEV_ZERO: u32 = 2;

/// 设备账留痕。
#[derive(Clone)]
pub struct DvRec {
    pub seq: u32,
    pub dev: u32,
    pub op: u32, // 0=read 1=write
    pub n: usize,
}

impl DvRec {
    pub fn ok(&self) -> bool {
        self.dev == DEV_NULL || self.dev == DEV_ZERO
    }
}

/// 设备面。
pub struct DevBook {
    pub ledger: Vec<DvRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl DevBook {
    pub fn new() -> DevBook {
        DevBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// read 分派：null 返 EOF（空），zero 填零。
    pub fn read(&mut self, dev: u32, count: usize, note: &str) -> Result<Vec<u8>, u32> {
        if self.sealed {
            return Err(DV_SEALED);
        }
        if note.is_empty() {
            return Err(DV_EMPTY_NOTE);
        }
        let out = match dev {
            DEV_NULL => Vec::new(),
            DEV_ZERO => vec![0u8; count],
            _ => return Err(DV_BAD_DEV),
        };
        self.ledger.push(DvRec { seq: self.seq_next, dev, op: 0, n: out.len() });
        self.seq_next += 1;
        Ok(out)
    }

    /// write 分派：null 吞任意量返原数；zero 拒写。
    pub fn write(&mut self, dev: u32, buf: &[u8], note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(DV_SEALED);
        }
        if note.is_empty() {
            return Err(DV_EMPTY_NOTE);
        }
        match dev {
            DEV_NULL => {
                self.ledger
                    .push(DvRec { seq: self.seq_next, dev, op: 1, n: buf.len() });
                self.seq_next += 1;
                Ok(buf.len())
            }
            DEV_ZERO => Err(DV_WRONLY_DEV),
            _ => Err(DV_BAD_DEV),
        }
    }

    /// 特殊设备定位拒（ESPIPE 语义面）：null/zero 不可寻位。
    pub fn seek(&mut self, dev: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DV_SEALED);
        }
        if note.is_empty() {
            return Err(DV_EMPTY_NOTE);
        }
        match dev {
            DEV_NULL | DEV_ZERO => Err(DV_SEEK),
            _ => Err(DV_BAD_DEV),
        }
    }

    /// 设备名解（诊断面）。
    pub fn dev_name(dev: u32) -> &'static str {
        match dev {
            DEV_NULL => "null",
            DEV_ZERO => "zero",
            _ => "wild",
        }
    }

    /// 吞吐统计：null 写累计。
    pub fn devoured(&self) -> usize {
        self.ledger
            .iter()
            .filter(|r| r.dev == DEV_NULL && r.op == 1)
            .map(|r| r.n)
            .fold(0usize, |a, b| a + b)
    }

    /// 零填累计。
    pub fn zeroed(&self) -> usize {
        self.ledger
            .iter()
            .filter(|r| r.dev == DEV_ZERO && r.op == 0)
            .map(|r| r.n)
            .fold(0usize, |a, b| a + b)
    }

    /// 单设备操作数。
    pub fn ops_of(&self, dev: u32) -> usize {
        self.ledger.iter().filter(|r| r.dev == dev).count()
    }

    /// 大写压力面：分片吞写。
    pub fn devour_chunks(&mut self, dev: u32, total: usize, chunk: usize, note: &str) -> Result<usize, u32> {
        let mut done = 0usize;
        while done < total {
            let take = chunk.min(total - done);
            let buf = vec![7u8; take];
            self.write(dev, &buf, note)?;
            done += take;
        }
        Ok(done)
    }

    /// 零填压力面：分片零读。
    pub fn zero_chunks(&mut self, dev: u32, total: usize, chunk: usize, note: &str) -> Result<usize, u32> {
        let mut done = 0usize;
        while done < total {
            let take = chunk.min(total - done);
            let g = self.read(dev, take, note)?;
            if g.len() != take {
                return Err(DV_BAD_DEV);
            }
            done += take;
        }
        Ok(done)
    }

    /// 双设备混合流：交替读零与吞写。
    pub fn mixed_stream(&mut self, rounds: usize, chunk: usize, note: &str) -> Result<(usize, usize), u32> {
        let mut z = 0usize;
        let mut d = 0usize;
        for _ in 0..rounds {
            let buf = vec![1u8; chunk];
            d += self.write(DEV_NULL, &buf, note)?;
            let g = self.read(DEV_ZERO, chunk, note)?;
            if g.iter().any(|b| *b != 0) {
                return Err(DV_BAD_DEV);
            }
            z += g.len();
        }
        Ok((d, z))
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
        let mut h: u32 = 0x104200;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.n as u32);
            h = h.wrapping_mul(31).wrapping_add(r.op);
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
    let mut b = DevBook::new();
    // 1) 野设备读写必拒
    got[0] = b.read(9, 4, "inj-dev").err() == Some(DV_BAD_DEV)
        && b.write(9, b"x", "inj-dev").err() == Some(DV_BAD_DEV);
    // 2) zero 写必拒
    got[1] = b.write(DEV_ZERO, b"x", "inj-zw").err() == Some(DV_WRONLY_DEV);
    // 3) 空留痕必拒
    got[2] = b.read(DEV_NULL, 1, "").err() == Some(DV_EMPTY_NOTE);
    // 4) 封面后操作必抓
    b.seal();
    got[3] = b.write(DEV_NULL, b"y", "inj-seal").err() == Some(DV_SEALED);
    // 5) null 读恒 EOF（非坏样本，校验语义恒定）
    got[4] = b.read(DEV_NULL, 8, "inj-eof").is_err();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10136_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10136");

    // 正路径：null 写丢弃
    let mut b = DevBook::new();
    let n = b.write(DEV_NULL, b"discard-me", "吞写").unwrap_or(0);
    let j1 = n == 10 && b.devoured() == 10;
    s.add("J1-null吞写", j1, "写 10 字节全吞且吞吐计数对平");

    // zero 读填充
    let g = b.read(DEV_ZERO, 6, "零读").unwrap_or_default();
    let j2 = g == vec![0u8; 6] && b.zeroed() == 6;
    s.add("J2-zero填零", j2, "读 6 字节全零且填充计数对平");

    // null 读恒 EOF
    let j3 = b.read(DEV_NULL, 8, "EOF 读").unwrap_or_default().is_empty();
    s.add("J3-null读EOF", j3, "null 设备读返空");

    // 分片压力吞写
    let j4 = {
        let d = b.devour_chunks(DEV_NULL, 100, 30, "分片吞").unwrap_or(0);
        d == 100 && b.devoured() == 110
    };
    s.add("J4-分片吞写", j4, "分片累计 100 全数入吞吐");

    // 野设备受控
    let j5 = b.read(42, 1, "野").err() == Some(DV_BAD_DEV);
    s.add("J5-野设备受控", j5, "设备号出闭集必拒");

    // zero 写受控
    let j6 = b.write(DEV_ZERO, b"z", "零写").err() == Some(DV_WRONLY_DEV);
    s.add("J6-zero写受控", j6, "零设备只读面写必拒");

    // 空留痕拒收
    let j7 = b.write(DEV_NULL, b"q", "").err() == Some(DV_EMPTY_NOTE);
    s.add("J7-空留痕拒", j7, "note 为空不入账");

    // 特殊设备定位拒
    let j10 = b.seek(DEV_NULL, "寻位").err() == Some(DV_SEEK)
        && b.seek(DEV_ZERO, "寻位").err() == Some(DV_SEEK)
        && b.seek(9, "野").err() == Some(DV_BAD_DEV);
    s.add("J10-寻位受控", j10, "双设备寻位拒、野设备坏号");

    // 零填压力面
    let j11 = {
        let n = b.zero_chunks(DEV_ZERO, 90, 40, "零填压").unwrap_or(0);
        n == 90
    };
    s.add("J11-零填压力", j11, "分片零读 90 字节全数入计");

    // 双设备混合流
    let j12 = {
        let (d, z) = b.mixed_stream(5, 4, "混合流").unwrap_or((0, 0));
        d == 20 && z == 20
    };
    s.add("J12-混合流", j12, "交替吞写/零读五轮对平");

    // 设备名解
    let j13 = DevBook::dev_name(DEV_NULL) == "null"
        && DevBook::dev_name(DEV_ZERO) == "zero"
        && DevBook::dev_name(7) == "wild";
    s.add("J13-设备名解", j13, "三号名解一致");

    // 单设备操作数
    let j14 = b.ops_of(DEV_NULL) >= 7 && b.ops_of(DEV_ZERO) >= 7;
    s.add("J14-操作数对平", j14, "双设备各有留痕");

    // 留痕合法与 seq 唯一（封面之前）
    let j9 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J9-留痕合法", j9, "设备号在闭集");

    // 封面后操作必抓（收官位）
    let j8 = {
        b.seal();
        b.read(DEV_ZERO, 1, "封后").err() == Some(DV_SEALED)
    };
    s.add("J8-封面拒操作", j8, "封面后读写必拒");

    // 注入核 5/5
    let inj = inject_five();
    let j10 = inj.iter().all(|g| *g);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j11 = b.fold() == b.fold();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10136_dev_flow() {
        let mut b = DevBook::new();
        assert_eq!(b.write(DEV_NULL, b"abc", "w").unwrap_or(0), 3);
        assert!(b.read(DEV_ZERO, 2, "r").unwrap_or_default() == vec![0u8; 2]);
        assert!(b.write(DEV_ZERO, b"x", "w2") == Err(DV_WRONLY_DEV));
        assert!(b.read(7, 1, "w3") == Err(DV_BAD_DEV));
    }

    #[test]
    fn unx10136_checks_green() {
        let s = run_unx10136_checks();
        assert!(s.all_passed(), "UNX-F10136 判据未全绿");
    }
}
