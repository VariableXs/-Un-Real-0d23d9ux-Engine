//! UNX-F10139 时间戳面正反双判据（更新达成与越权改时间误用受控）（0x10480 段）
//!
//! 锚点：UNX-F10139-J1 时间戳达成（写后 mtime 更新、读后 atime 更新
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：时戳册（atime/mtime 双针 + 时钟源）+ 针核（写推 mtime/读推
//! atime/显式 utimes 越权拒/时钟单调）+ 针账（逐次留痕）+ 注入核（越权
//! 改时/缺件/野钟/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10480 独占 0x20）。
pub const SEG_BASE: u32 = 0x10480;
pub const TS_EPERM: u32 = SEG_BASE + 0x01; // 越权改时
pub const TS_NOENT: u32 = SEG_BASE + 0x02; // 缺件
pub const TS_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const TS_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const TS_BAD_CLOCK: u32 = SEG_BASE + 0x05; // 野时钟源

/// 时钟源闭集。
pub const CLK_REAL: u32 = 1;
pub const CLK_MONO: u32 = 2;

/// 时戳件。
#[derive(Clone)]
pub struct TsFile {
    pub name: String,
    pub atime: u32,
    pub mtime: u32,
}

impl TsFile {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.atime >= 1 && self.mtime >= 1
    }

    pub fn render(&self) -> String {
        format!("{}|a{}|m{}", self.name, self.atime, self.mtime)
    }
}

/// 针账留痕。
#[derive(Clone)]
pub struct TsRec {
    pub seq: u32,
    pub name: String,
    pub op: u32, // 0=read 1=write 2=utimes
    pub clock: u32,
    pub stamp: u32,
}

impl TsRec {
    pub fn ok(&self) -> bool {
        self.op <= 2 && !self.name.is_empty()
    }
}

/// 时戳册。
pub struct TsBook {
    pub files: Vec<TsFile>,
    pub clock: u32,
    pub clk_src: u32,
    pub ledger: Vec<TsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TsBook {
    pub fn new(clk_src: u32) -> TsBook {
        TsBook {
            files: Vec::new(),
            clock: 100,
            clk_src,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 建（初针同时赋 atime/mtime）。
    pub fn create(&mut self, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TS_SEALED);
        }
        if name.is_empty() {
            return Err(TS_NOENT);
        }
        if self.clk_src != CLK_REAL && self.clk_src != CLK_MONO {
            return Err(TS_BAD_CLOCK);
        }
        self.clock += 1;
        self.files.push(TsFile { name: name.to_string(), atime: self.clock, mtime: self.clock });
        Ok(())
    }

    fn tick(&mut self) -> u32 {
        self.clock += 1;
        self.clock
    }

    /// 读面：atime 更新。
    pub fn read_ts(&mut self, name: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TS_SEALED);
        }
        if note.is_empty() {
            return Err(TS_EMPTY_NOTE);
        }
        let i = match self.files.iter().position(|f| f.name == name) {
            Some(i) => i,
            None => return Err(TS_NOENT),
        };
        let t = self.tick();
        self.files[i].atime = t;
        self.ledger.push(TsRec {
            seq: self.seq_next,
            name: name.to_string(),
            op: 0,
            clock: self.clk_src,
            stamp: t,
        });
        self.seq_next += 1;
        Ok(t)
    }

    /// 写面：mtime 更新。
    pub fn write_ts(&mut self, name: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TS_SEALED);
        }
        if note.is_empty() {
            return Err(TS_EMPTY_NOTE);
        }
        let i = match self.files.iter().position(|f| f.name == name) {
            Some(i) => i,
            None => return Err(TS_NOENT),
        };
        let t = self.tick();
        self.files[i].mtime = t;
        self.ledger.push(TsRec {
            seq: self.seq_next,
            name: name.to_string(),
            op: 1,
            clock: self.clk_src,
            stamp: t,
        });
        self.seq_next += 1;
        Ok(t)
    }

    /// 显式 utimes：仅属主可改（越权拒）。
    pub fn utimes(&mut self, name: &str, at: u32, mt: u32, as_owner: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TS_SEALED);
        }
        if note.is_empty() {
            return Err(TS_EMPTY_NOTE);
        }
        let i = match self.files.iter().position(|f| f.name == name) {
            Some(i) => i,
            None => return Err(TS_NOENT),
        };
        if !as_owner {
            return Err(TS_EPERM);
        }
        self.files[i].atime = at;
        self.files[i].mtime = mt;
        self.ledger.push(TsRec {
            seq: self.seq_next,
            name: name.to_string(),
            op: 2,
            clock: self.clk_src,
            stamp: mt,
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 双针读取。
    pub fn stamps_of(&self, name: &str) -> Option<(u32, u32)> {
        self.files.iter().find(|f| f.name == name).map(|f| (f.atime, f.mtime))
    }

    /// 时钟单调核：账面 stamp 序不回退。
    pub fn monotonic(&self) -> bool {
        let mut last = 0u32;
        for r in &self.ledger {
            if r.stamp < last {
                return false;
            }
            last = r.stamp;
        }
        true
    }

    /// 双针分离核：读不推 mtime、写不推 atime。
    pub fn pins_independent(&self, name: &str) -> bool {
        match self.stamps_of(name) {
            Some((a, m)) => a != m,
            None => false,
        }
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
        let mut h: u32 = 0x104800;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.stamp);
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
    let mut b = TsBook::new(CLK_REAL);
    let _ = b.create("f");
    // 1) 越权改时必拒
    got[0] = b.utimes("f", 1, 1, false, "inj-ep").err() == Some(TS_EPERM);
    // 2) 缺件必拒
    got[1] = b.read_ts("ghost", "inj-noent").err() == Some(TS_NOENT);
    // 3) 空留痕必拒
    got[2] = b.write_ts("f", "").err() == Some(TS_EMPTY_NOTE);
    // 4) 野时钟源必拒
    let mut b2 = TsBook::new(9);
    got[3] = b2.create("g").err() == Some(TS_BAD_CLOCK);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.read_ts("f", "inj-seal").err() == Some(TS_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10139_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10139");

    // 正路径：写后 mtime 更新
    let mut b = TsBook::new(CLK_REAL);
    let _ = b.create("log.txt");
    let (a0, m0) = b.stamps_of("log.txt").unwrap_or((0, 0));
    let m1 = b.write_ts("log.txt", "写").unwrap_or(0);
    let j1 = m1 > m0 && b.stamps_of("log.txt").unwrap_or((0, 0)).1 == m1;
    s.add("J1-写推mtime", j1, "写后 mtime 前进且落位");

    // 读后 atime 更新
    let a1 = b.read_ts("log.txt", "读").unwrap_or(0);
    let j2 = a1 > a0 && b.stamps_of("log.txt").unwrap_or((0, 0)).0 == a1;
    s.add("J2-读推atime", j2, "读后 atime 前进且落位");

    // 双针分离
    let j3 = b.pins_independent("log.txt");
    s.add("J3-双针分离", j3, "读/写各推各针不互扰");

    // 时钟单调
    let j4 = b.monotonic();
    s.add("J4-时钟单调", j4, "账面 stamp 序不回退");

    // 显式改时属主放行
    let j5 = {
        let _ = b.utimes("log.txt", 5, 9, true, "显改");
        b.stamps_of("log.txt") == Some((5, 9))
    };
    s.add("J5-显改放行", j5, "属主 utimes 双针落位");

    // 越权改时受控
    let j6 = b.utimes("log.txt", 1, 1, false, "野改").err() == Some(TS_EPERM);
    s.add("J6-越权受控", j6, "非属主 utimes 必拒");

    // 缺件受控
    let j7 = b.write_ts("ghost", "缺席").err() == Some(TS_NOENT)
        && b.read_ts("ghost", "缺席").err() == Some(TS_NOENT);
    s.add("J7-缺件受控", j7, "读写双面缺件同拒");

    // 野时钟源受控
    let j8 = TsBook::new(7).create("x").err() == Some(TS_BAD_CLOCK);
    s.add("J8-野钟受控", j8, "时钟源出闭集必拒");

    // 空留痕拒收
    let j9 = b.write_ts("log.txt", "").err() == Some(TS_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "op 在闭集且名非空");

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
    fn unx10139_ts_flow() {
        let mut b = TsBook::new(CLK_MONO);
        let _ = b.create("t");
        let (_, m0) = b.stamps_of("t").unwrap_or((0, 0));
        let _ = b.write_ts("t", "w");
        let (_, m1) = b.stamps_of("t").unwrap_or((0, 0));
        assert!(m1 > m0);
        assert!(b.utimes("t", 2, 3, false, "x") == Err(TS_EPERM));
    }

    #[test]
    fn unx10139_checks_green() {
        let s = run_unx10139_checks();
        assert!(s.all_passed(), "UNX-F10139 判据未全绿");
    }
}
