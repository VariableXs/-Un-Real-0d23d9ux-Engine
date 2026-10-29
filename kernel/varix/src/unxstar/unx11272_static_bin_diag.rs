//! UNX-F11272 静态二进制诊断消费面（0x1f480 段）
//! 纯功能模型：/proc/self/maps 行模型——段区间/权限/偏移/设备/inode/路径
//! 逐字段登记并逐字渲染，反向解析回读；静态件判据是「文件背衬段恰一段」
//! （单 VMA 态），另加 [heap]/[stack]/[vdso] 等匿名段。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f480 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f480;
/// 区间非法（起 >= 止、未页对齐、偏移未页对齐）。
pub const MP_RANGE: u32 = SEG_BASE + 0x01;
/// 与既有段区间相交。
pub const MP_OVERLAP: u32 = SEG_BASE + 0x02;
/// 权限串非法（空或含 rwxp 之外的字符）。
pub const MP_PERM: u32 = SEG_BASE + 0x03;
/// 行解析失败（字段数、十六进制、区间均可能）。
pub const MP_PARSE: u32 = SEG_BASE + 0x04;
/// 路径字段空。
pub const MP_PATH: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const MP_FROZEN: u32 = SEG_BASE + 0x06;
/// 段表已满。
pub const MP_CAP: u32 = SEG_BASE + 0x07;

/// 页粒度。
pub const PAGE: u64 = 0x1000;
/// 段表定容。
pub const MAP_CAP: usize = 24;
/// 权限位。
pub const P_R: u8 = 1;
pub const P_W: u8 = 2;
pub const P_X: u8 = 4;
pub const P_P: u8 = 8;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_72;

/// 单条 maps 段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub perms: u8,
    pub off: u64,
    pub inode: u32,
    pub path: String,
}

impl Vma {
    /// 权限串（r/w/x/p 四字母）。
    pub fn perm_str(&self) -> String {
        let mut s = String::new();
        s.push_str(if self.perms & P_R != 0 { "r" } else { "-" });
        s.push_str(if self.perms & P_W != 0 { "w" } else { "-" });
        s.push_str(if self.perms & P_X != 0 { "x" } else { "-" });
        s.push_str(if self.perms & P_P != 0 { "p" } else { "-" });
        s
    }

    /// 逐字渲染：`00400000-00402000 r-xp 00000000 00:00 0 /bin/hello`。
    pub fn render(&self) -> String {
        format!(
            "{:08x}-{:08x} {} {:08x} 00:00 {} {}",
            self.start, self.end, self.perm_str(), self.off, self.inode, self.path
        )
    }

    /// 文件背衬（inode 非零）。
    pub fn file_backed(&self) -> bool {
        self.inode != 0 && !self.path.is_empty() && self.path.as_bytes().first() != Some(&b'[')
    }

    pub fn exec(&self) -> bool {
        self.perms & P_X != 0
    }
}

/// maps 段表：登记 → 渲染 → 反向解析 → 静态件判据。
#[derive(Clone, Debug)]
pub struct MapTable {
    pub vmas: Vec<Vma>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl MapTable {
    pub fn new() -> MapTable {
        MapTable { vmas: Vec::new(), accepts: 0, rejects: 0, frozen: false, root: None }
    }

    /// 登记一段。判定序：封册 → 区间 → 权限 → 路径 → 相交 → 定容。
    pub fn add_vma(&mut self, start: u64, end: u64, perms: u8, off: u64, inode: u32, path: &str)
        -> Result<usize, u32>
    {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MP_FROZEN);
        }
        if start >= end || start & (PAGE - 1) != 0 || end & (PAGE - 1) != 0 || off & (PAGE - 1) != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MP_RANGE);
        }
        if perms == 0 || perms & !(P_R | P_W | P_X | P_P) != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MP_PERM);
        }
        if perms & (P_W | P_X) == (P_W | P_X) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MP_PERM);
        }
        if path.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MP_PATH);
        }
        let mut k = 0usize;
        while k < self.vmas.len() {
            if let Some(o) = self.vmas.get(k) {
                if start < o.end && o.start < end {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(MP_OVERLAP);
                }
            }
            k = k.wrapping_add(1);
        }
        if self.vmas.len() >= MAP_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MP_CAP);
        }
        self.vmas.push(Vma {
            start,
            end,
            perms,
            off,
            inode,
            path: clone_str(path),
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.vmas.len().wrapping_sub(1))
    }

    /// 全表逐行渲染（换行分隔，末行也带换行）。
    pub fn render_all(&self) -> String {
        let mut out = String::new();
        let mut k = 0usize;
        while k < self.vmas.len() {
            if let Some(v) = self.vmas.get(k) {
                out.push_str(&v.render());
                out.push('\n');
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 文件背衬段数。
    pub fn file_backed(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.vmas.len() {
            if let Some(v) = self.vmas.get(k) {
                if v.file_backed() {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 不同背衬 inode 数（静态件应为 1）。
    pub fn file_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.vmas.len() {
            if let Some(v) = self.vmas.get(k) {
                if v.file_backed() && !ids.iter().any(|x| *x == v.inode) {
                    ids.push(v.inode);
                }
            }
            k = k.wrapping_add(1);
        }
        ids
    }

    /// 静态件单 VMA 判据：恰一个文件 + 至少一个可执行文件背衬段。
    pub fn single_file(&self) -> bool {
        let ids = self.file_ids();
        if ids.len() != 1 {
            return false;
        }
        let mut exec = 0u32;
        let mut k = 0usize;
        while k < self.vmas.len() {
            if let Some(v) = self.vmas.get(k) {
                if v.file_backed() && v.exec() {
                    exec = exec.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        exec >= 1
    }

    /// 反向解析一条 maps 行。
    pub fn parse(line: &str) -> Result<Vma, u32> {
        let mut it = line.split_whitespace();
        let range = match it.next() {
            Some(v) => v,
            None => return Err(MP_PARSE),
        };
        let perms = match it.next() {
            Some(v) => v,
            None => return Err(MP_PARSE),
        };
        let off = match it.next() {
            Some(v) => v,
            None => return Err(MP_PARSE),
        };
        let _dev = match it.next() {
            Some(v) => v,
            None => return Err(MP_PARSE),
        };
        let inode = match it.next() {
            Some(v) => v,
            None => return Err(MP_PARSE),
        };
        let path = match it.next() {
            Some(v) => v,
            None => return Err(MP_PARSE),
        };
        let dash = match range.find('-') {
            Some(i) => i,
            None => return Err(MP_PARSE),
        };
        let start = match u64::from_str_radix(&range[..dash], 16) {
            Ok(v) => v,
            Err(_) => return Err(MP_PARSE),
        };
        let end = match u64::from_str_radix(&range[dash + 1..], 16) {
            Ok(v) => v,
            Err(_) => return Err(MP_PARSE),
        };
        if start >= end {
            return Err(MP_PARSE);
        }
        let mut p = 0u8;
        let pb = perms.as_bytes();
        if pb.len() != 4 {
            return Err(MP_PARSE);
        }
        let mut i = 0usize;
        while i < 4 {
            let c = match pb.get(i) {
                Some(v) => *v,
                None => return Err(MP_PARSE),
            };
            let bit = match c {
                b'r' => P_R,
                b'w' => P_W,
                b'x' => P_X,
                b'p' => P_P,
                b'-' => 0,
                _ => return Err(MP_PARSE),
            };
            p |= bit;
            i = i.wrapping_add(1);
        }
        let offv = match u64::from_str_radix(off, 16) {
            Ok(v) => v,
            Err(_) => return Err(MP_PARSE),
        };
        let inv = match u32::from_str_radix(inode, 10) {
            Ok(v) => v,
            Err(_) => return Err(MP_PARSE),
        };
        Ok(Vma { start, end, perms: p, off: offv, inode: inv, path: clone_str(path) })
    }

    /// FNV-1a 链指纹：段序/区间/权限/偏移/inode/路径敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.vmas.len() {
            if let Some(v) = self.vmas.get(k) {
                h = (h ^ v.start).wrapping_mul(FNV_PRIME);
                h = (h ^ v.end).wrapping_mul(FNV_PRIME);
                h = (h ^ v.perms as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ v.off).wrapping_mul(FNV_PRIME);
                h = (h ^ v.inode as u64).wrapping_mul(FNV_PRIME);
                let b = v.path.as_bytes();
                let mut j = 0usize;
                while j < b.len() {
                    if let Some(x) = b.get(j) {
                        h = (h ^ *x as u64).wrapping_mul(FNV_PRIME);
                    }
                    j = j.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 借串入堆。
fn clone_str(s: &str) -> String {
    let mut o = String::new();
    o.push_str(s);
    o
}

/// Static-link hello world: three file-backed segments of one inode plus
/// anonymous heap/stack regions, mirroring a real /proc/self/maps dump.
pub fn static_hello_maps() -> MapTable {
    let mut t = MapTable::new();
    let _ = t.add_vma(0x0040_0000, 0x0040_1000, P_R | P_X | P_P, 0x0000, 1, "/bin/hello");
    let _ = t.add_vma(0x0040_1000, 0x0040_2000, P_R | P_P, 0x1000, 1, "/bin/hello");
    let _ = t.add_vma(0x0040_2000, 0x0040_3000, P_R | P_W | P_P, 0x2000, 1, "/bin/hello");
    let _ = t.add_vma(0x0060_0000, 0x0062_1000, P_R | P_W | P_P, 0x0000, 0, "[heap]");
    let _ = t.add_vma(0x7ffd_0000_0000, 0x7ffd_0002_1000, P_R | P_W | P_P, 0x0000, 0, "[stack]");
    t
}

/// Four probes: (a) reversed range id, (b) overlapping range,
/// (c) writable+executable permission pair, (d) write after freeze.
pub fn inject_four() -> [bool; 4] {
    let mut a = MapTable::new();
    let bad_a = a.add_vma(0x5000, 0x4000, P_R, 0, 0, "x").err() == Some(MP_RANGE);

    let mut b = MapTable::new();
    let _ = b.add_vma(0x0040_0000, 0x0040_2000, P_R, 0, 1, "/bin/hello");
    let bad_b = b.add_vma(0x0040_1000, 0x0040_3000, P_R, 0x1000, 1, "/bin/hello").err()
        == Some(MP_OVERLAP);

    let mut c = MapTable::new();
    let bad_c = c.add_vma(0x0050_0000, 0x0050_1000, P_R | P_W | P_X, 0, 1, "j").err()
        == Some(MP_PERM);

    let mut d = static_hello_maps();
    let _ = d.freeze();
    let bad_d = d.add_vma(0x0080_0000, 0x0080_1000, P_R, 0, 0, "[vvar]").err()
        == Some(MP_FROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// Thirteen contract checks over the maps renderer, parser and gate set.
pub fn run_unx11272_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11272");

    let m = static_hello_maps();
    s.add("X1-SEGMENT-DOMAIN", SEG_BASE == 0x1f480 && MP_RANGE == SEG_BASE + 0x01
        && MP_OVERLAP == SEG_BASE + 0x02 && MP_PERM == SEG_BASE + 0x03
        && MP_PARSE == SEG_BASE + 0x04 && MP_PATH == SEG_BASE + 0x05
        && MP_FROZEN == SEG_BASE + 0x06 && MP_CAP == SEG_BASE + 0x07,
        "错误码落在 0x1f480 段连续偏移上");

    s.add("X2-SINGLE-FILE", m.single_file() && m.file_ids() == vec![1u32]
        && m.vmas.len() == 5 && m.accepts == 5 && m.rejects == 0,
        "静态 hello 三段同 inode 单文件映射成立");

    let first = match m.vmas.get(0) {
        Some(v) => v.render(),
        None => String::new(),
    };
    s.add_dyn("X3-RENDER", first == "00400000-00401000 r-xp 00000000 00:00 1 /bin/hello",
        &format!("首行渲染={}", first));

    let mut rt = MapTable::new();
    let mut bad = 0u32;
    for line in m.render_all().lines() {
        match MapTable::parse(line) {
            Ok(v) => rt.vmas.push(v),
            Err(_) => bad = bad.wrapping_add(1),
        }
    }
    s.add("X4-PARSE-ROUNDTRIP", bad == 0 && rt.vmas.len() == 5 && rt.file_ids() == m.file_ids()
        && rt.single_file() && rt.rejects == 0, "渲染结果可被 parse 完整还原");

    let mut r1 = MapTable::new();
    let e1 = r1.add_vma(0x3000, 0x2000, P_R, 0, 0, "[bad]").err();
    s.add("X5-RANGE-GATE", r1.rejects == 1 && e1 == Some(MP_RANGE),
        "起始大于结束被 MP_RANGE 拒绝");

    let mut r2 = static_hello_maps();
    let e2 = r2.add_vma(0x0040_1000, 0x0040_4000, P_R, 0, 1, "/bin/hello").err();
    s.add("X6-OVERLAP-GATE", r2.rejects == 1 && e2 == Some(MP_OVERLAP),
        "区间与既有 VMA 相交被 MP_OVERLAP 拒绝");

    let mut p = MapTable::new();
    let ep = p.add_vma(0x0050_0000, 0x0050_1000, P_R | P_W | P_X, 0, 1, "j").err();
    s.add("X7-PERM-GATE", ep == Some(MP_PERM) && p.rejects == 1,
        "同时可写可执行被 MP_PERM 拒绝");

    let short = MapTable::parse("00400000-00401000 r-xp 00000000");
    let badp = MapTable::parse("00400000-00401000 zzxp 00000000 00:00 1 /x");
    s.add("X8-PARSE-GATE", short.err() == Some(MP_PARSE)
        && badp.err() == Some(MP_PARSE), "字段缺失与权限串非法均判 MP_PARSE");

    let mut q = MapTable::new();
    let eq = q.add_vma(0x0050_0000, 0x0050_1000, P_R, 0, 1, "").err();
    s.add("X9-PATH-GATE", eq == Some(MP_PATH) && q.vmas.is_empty(),
        "空路径被 MP_PATH 拒绝且不入表");

    let mut two = static_hello_maps();
    let _ = two.add_vma(0x0080_0000, 0x0080_1000, P_R, 0, 7, "/lib/libc.so");
    s.add("X10-TWO-INODE", !two.single_file() && two.file_ids().len() == 2
        && two.file_backed() == 4, "混入第二个文件后静态件判据为假");

    let a = static_hello_maps();
    let mut b2 = static_hello_maps();
    let h0 = a.fold();
    let h1 = b2.fold();
    if let Some(v) = b2.vmas.get_mut(1) {
        v.perms = P_R | P_W;
    }
    s.add("X11-FOLD", h0 != 0 && h0 == h1 && b2.fold() != h0,
        "同构表指纹相同，改权限后指纹变化");

    let mut f = static_hello_maps();
    let r1 = f.freeze();
    let r2 = f.freeze();
    s.add("X12-FREEZE-IDEM", r1.is_some() && r1 == r2 && f.frozen,
        "封册返回根且二次调用同值");

    let mut z = static_hello_maps();
    let _ = z.freeze();
    let ez = z.add_vma(0x0090_0000, 0x0090_1000, P_R, 0, 0, "[vdso]").err();
    s.add_dyn("X13-FROZEN-GATE", ez == Some(MP_FROZEN) && z.rejects == 1
        && z.render_all().contains("00400000-00401000 r-xp"),
        &format!("封册后写入={:?} 拒绝={}", ez, z.rejects));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11272_flow() {
        let mut m = static_hello_maps();
        assert_eq!(m.vmas.len(), 5);
        assert_eq!(m.accepts, 5);
        assert_eq!(m.rejects, 0);
        assert!(m.single_file());
        assert_eq!(m.file_backed(), 3);
        assert_eq!(m.file_ids(), vec![1u32]);
        let dump = m.render_all();
        assert_eq!(dump.lines().count(), 5);
        assert!(dump.starts_with("00400000-00401000 r-xp 00000000 00:00 1 /bin/hello\n"));
        assert!(dump.contains("[stack]"));
        let mut rt = MapTable::new();
        for line in dump.lines() {
            if let Ok(v) = MapTable::parse(line) {
                rt.vmas.push(v);
            }
        }
        assert_eq!(rt.vmas.len(), 5);
        assert_eq!(rt.file_ids(), m.file_ids());
        assert_eq!(rt.fold(), m.fold());
        assert!(rt.single_file());
        let root = m.freeze();
        assert!(root.is_some());
        assert_eq!(root, m.freeze());
    }

    #[test]
    fn unx11272_gates() {
        let mut t = MapTable::new();
        assert_eq!(t.add_vma(0x10, 0x20, P_R, 0, 0, "x").err(), Some(MP_RANGE));
        assert_eq!(t.add_vma(0x1000, 0x2000, 0, 0, 0, "x").err(), Some(MP_PERM));
        assert_eq!(t.add_vma(0x1000, 0x2000, P_R, 0, 0, "").err(), Some(MP_PATH));
        assert!(t.add_vma(0x1000, 0x2000, P_R, 0, 1, "/a").is_ok());
        assert_eq!(t.add_vma(0x1000, 0x3000, P_R, 0, 1, "/a").err(), Some(MP_OVERLAP));
        assert_eq!(MapTable::parse("garbage").err(), Some(MP_PARSE));
        assert_eq!(t.accepts, 1);
        assert_eq!(t.rejects, 4);
        let _ = t.freeze();
        assert_eq!(t.add_vma(0x3000, 0x4000, P_R, 0, 1, "/a").err(), Some(MP_FROZEN));
        assert_eq!(t.rejects, 5);

        let mut c = MapTable::new();
        let mut i = 0u64;
        while c.vmas.len() < MAP_CAP {
            let base = 0x1000_0000u64.wrapping_add(i.wrapping_mul(0x2000));
            assert!(c.add_vma(base, base.wrapping_add(0x1000), P_R, 0, 0, "[anon]").is_ok());
            i = i.wrapping_add(1);
        }
        assert_eq!(c.add_vma(0x3000_0000, 0x3000_1000, P_R, 0, 0, "[anon]").err(), Some(MP_CAP));
        assert_eq!(c.accepts, MAP_CAP as u32);
        assert_eq!(c.rejects, 1);
    }

    #[test]
    fn unx11272_checks_green() {
        let s = run_unx11272_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
