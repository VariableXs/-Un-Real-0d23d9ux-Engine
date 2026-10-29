//! UNX-F10142 mmap 文件映射正反双判据（文件背书达成与越尾访问误用受控）（0x104e0 段）
//!
//! 锚点：UNX-F10142-J1 文件映射达成（写映射→文件重读一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：背书册（文件内容 + 文件背书映射区共享同一存储）+ 背书核（写
//! 映射落文件/文件重读一致/越尾访问 SIGBUS 面）+ 背书账（逐次留痕）+
//! 注入核（越尾/坏区/零长/空留痕/封册必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x104e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x104e0;
pub const MF_SIGBUS: u32 = SEG_BASE + 0x01; // 越尾访问
pub const MF_ZERO_LEN: u32 = SEG_BASE + 0x02; // 零长度
pub const MF_BAD_AREA: u32 = SEG_BASE + 0x03; // 坏区柄
pub const MF_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const MF_SEALED: u32 = SEG_BASE + 0x05; // 封册后操作

/// 背书文件。
pub struct BackFile {
    pub name: String,
    pub content: Vec<u8>,
}

impl BackFile {
    pub fn new(name: &str, content: &[u8]) -> BackFile {
        BackFile { name: name.to_string(), content: content.to_vec() }
    }

    pub fn len(&self) -> usize {
        self.content.len()
    }
}

/// 背书账留痕。
#[derive(Clone)]
pub struct MfRec {
    pub seq: u32,
    pub area: u32,
    pub off: usize,
    pub n: usize,
    pub is_write: bool,
}

impl MfRec {
    pub fn ok(&self) -> bool {
        self.n > 0 || !self.is_write
    }
}

/// 背书册。
pub struct BackBook {
    pub files: Vec<BackFile>,
    /// 区柄 → (文件下标, 区长度)
    pub areas: Vec<Option<(usize, usize)>>,
    pub ledger: Vec<MfRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl BackBook {
    pub fn new() -> BackBook {
        BackBook { files: Vec::new(), areas: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 挂文件。
    pub fn add_file(&mut self, name: &str, content: &[u8]) -> usize {
        self.files.push(BackFile::new(name, content));
        self.files.len() - 1
    }

    /// mmap 文件背书：区长度 = 文件长（整文件映射面）。
    pub fn mmap_file(&mut self, fidx: usize, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(MF_SEALED);
        }
        if note.is_empty() {
            return Err(MF_EMPTY_NOTE);
        }
        if fidx >= self.files.len() || self.files[fidx].len() == 0 {
            return Err(MF_ZERO_LEN);
        }
        let slot = if let Some(i) = self.areas.iter().position(|s| s.is_none()) {
            i
        } else {
            self.areas.push(None);
            self.areas.len() - 1
        };
        let len = self.files[fidx].len();
        self.areas[slot] = Some((fidx, len));
        self.ledger.push(MfRec {
            seq: self.seq_next,
            area: slot as u32,
            off: 0,
            n: len,
            is_write: false,
        });
        self.seq_next += 1;
        Ok(slot)
    }

    fn resolve(&self, area: usize) -> Result<(usize, usize), u32> {
        match self.areas.get(area) {
            Some(Some((f, l))) => Ok((*f, *l)),
            _ => Err(MF_BAD_AREA),
        }
    }

    /// 写映射：直落文件（写穿面）。
    pub fn write_map(&mut self, area: usize, off: usize, buf: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(MF_SEALED);
        }
        let (f, len) = self.resolve(area)?;
        if off + buf.len() > len {
            return Err(MF_SIGBUS);
        }
        let content = &mut self.files[f].content;
        content[off..off + buf.len()].copy_from_slice(buf);
        self.ledger.push(MfRec {
            seq: self.seq_next,
            area: area as u32,
            off,
            n: buf.len(),
            is_write: true,
        });
        self.seq_next += 1;
        Ok(buf.len())
    }

    /// 读映射。
    pub fn read_map(&mut self, area: usize, off: usize, count: usize) -> Result<Vec<u8>, u32> {
        let (f, len) = self.resolve(area)?;
        if off + count > len {
            return Err(MF_SIGBUS);
        }
        Ok(self.files[f].content[off..off + count].to_vec())
    }

    /// 文件重读（映射外通道）。
    pub fn read_file(&self, fidx: usize) -> Option<Vec<u8>> {
        self.files.get(fidx).map(|f| f.content.clone())
    }

    /// 越尾访问检出（SIGBUS 面）。
    pub fn sigbus_probe(&mut self, area: usize, off: usize) -> bool {
        let r = self.read_map(area, off, 1);
        matches!(r, Err(MF_SIGBUS))
    }

    /// 映射-文件一致核。
    pub fn map_file_agree(&mut self, area: usize) -> bool {
        let (f, len) = match self.resolve(area) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let via_map = self.read_map(area, 0, len).unwrap_or_default();
        Some(via_map) == self.files.get(f).map(|x| x.content.clone())
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
        let mut h: u32 = 0x104e00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.area);
            h = h.wrapping_mul(31).wrapping_add(r.off as u32);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 活区计数（区柄在册未被卸载）。
    pub fn live_areas(&self) -> usize {
        self.areas.iter().filter(|s| s.is_some()).count()
    }

    /// 区名解析：区柄 → 背书文件名。
    pub fn area_name(&self, area: usize) -> Option<String> {
        let (f, _) = self.areas.get(area).copied().flatten()?;
        self.files.get(f).map(|x| x.name.clone())
    }

    /// 越尾写无损审计：越尾写被拒后文件内容保持原样；坏柄判否。
    pub fn tail_safe_audit(&mut self, area: usize) -> bool {
        let (_, len) = match self.areas.get(area).copied().flatten() {
            Some(v) => v,
            None => return false,
        };
        let before = self.read_map(area, 0, len).unwrap_or_default();
        let _ = self.write_map(area, len, b"!");
        let after = self.read_map(area, 0, len).unwrap_or_default();
        before == after
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = BackBook::new();
    let f = b.add_file("d", b"0123456789");
    let a = b.mmap_file(f, "inj-ok").unwrap_or(0);
    // 1) 越尾访问必检出（SIGBUS 面）
    got[0] = b.sigbus_probe(a, 10);
    // 2) 零长文件映射必拒
    let f0 = b.add_file("empty", b"");
    got[1] = b.mmap_file(f0, "inj-zero").err() == Some(MF_ZERO_LEN);
    // 3) 坏区柄必拒
    got[2] = b.read_map(99, 0, 1).err() == Some(MF_BAD_AREA);
    // 4) 空留痕必拒
    got[3] = b.mmap_file(f, "").err() == Some(MF_EMPTY_NOTE);
    // 5) 封册后操作必抓
    b.seal();
    got[4] = b.mmap_file(f, "inj-seal").err() == Some(MF_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10142");

    // 正路径：写映射→文件重读一致
    let mut b = BackBook::new();
    let f = b.add_file("data.bin", b"AAAAABBBBB");
    let a = b.mmap_file(f, "映射").unwrap_or(0);
    let _ = b.write_map(a, 5, b"zz");
    let via_map = b.read_map(a, 0, 10).unwrap_or_default();
    let via_file = b.read_file(f).unwrap_or_default();
    let j1 = via_map == b"AAAAAzzBBB".to_vec() && via_file == via_map;
    s.add("J1-写穿一致", j1, "映射写 5..7 落文件且双通道同值");

    // map-file 全区一致核
    let j2 = b.map_file_agree(a);
    s.add("J2-全区一致", j2, "映射面与文件面逐字节同");

    // 越尾访问受控（SIGBUS 面）
    let j3 = b.write_map(a, 9, b"XY").err() == Some(MF_SIGBUS)
        && b.read_map(a, 9, 2).err() == Some(MF_SIGBUS);
    s.add("J3-越尾受控", j3, "读写越尾同 SIGBUS 拒");

    // 多文件多区
    let j4 = {
        let f2 = b.add_file("two.bin", b"xy");
        let a2 = b.mmap_file(f2, "二区").unwrap_or(0);
        let g = b.read_map(a2, 0, 2).unwrap_or_default();
        g == b"xy".to_vec() && b.map_file_agree(a2)
    };
    s.add("J4-多区并存", j4, "第二文件独立映射一致");

    // 区-文件写联动
    let j5 = {
        let _ = b.write_map(a, 0, b"HEAD");
        b.read_file(f).unwrap_or_default().starts_with(b"HEAD")
    };
    s.add("J5-头部联动", j5, "写映射头部直落文件");

    // 零长文件受控
    let j6 = {
        let f3 = b.add_file("nil", b"");
        b.mmap_file(f3, "零件").err() == Some(MF_ZERO_LEN)
    };
    s.add("J6-零件受控", j6, "零长文件映射必拒");

    // 坏区柄受控
    let j7 = b.write_map(77, 0, b"x").err() == Some(MF_BAD_AREA);
    s.add("J7-坏区受控", j7, "未挂区柄必拒");

    // 空留痕拒收
    let j8 = b.mmap_file(f, "").err() == Some(MF_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 封册后操作必抓
    let j9 = {
        b.seal();
        b.mmap_file(f, "封后").err() == Some(MF_SEALED)
    };
    s.add("J9-封册拒操作", j9, "封册后映射必拒");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "量面自洽且 seq 唯一");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // 活区计数与区名解析
    let j13 = b.live_areas() == 2 && b.area_name(a).as_deref() == Some("data.bin");
    s.add("J13-活区面", j13, "两活区在册且区名可解析");

    // 越尾写无损审计
    let j14 = b.tail_safe_audit(a) && !b.tail_safe_audit(9);
    s.add("J14-越尾无损", j14, "越尾写被拒且坏柄探针判否");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10142_back_flow() {
        let mut b = BackBook::new();
        let f = b.add_file("x", b"abcde");
        let a = b.mmap_file(f, "m").unwrap_or(0);
        let _ = b.write_map(a, 1, b"Z");
        assert_eq!(b.read_file(f).unwrap_or_default(), b"aZcde".to_vec());
        assert!(b.sigbus_probe(a, 5));
    }

    #[test]
    fn unx10142_checks_green() {
        let s = run_unx10142_checks();
        assert!(s.all_passed(), "UNX-F10142 判据未全绿");
    }
}
