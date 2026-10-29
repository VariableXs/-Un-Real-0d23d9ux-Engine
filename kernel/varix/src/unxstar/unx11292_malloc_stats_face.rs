//! UNX-F11292 堆剖面诊断消费（malloc_stats 面）（0x1f670 段）
//!
//! 锚点：UNX-F11292-J1 malloc_stats / malloc_info 输出结构判据绿，数值与
//! 内核映射账交叉一致。
//!
//! 纯功能模型：堆剖面由若干 arena 组成，每个 arena 记 system bytes、
//! in use bytes、mmap 区数与 mmap 字节。`malloc_stats` 面按 glibc 的
//! 「Arena N: / system bytes / in use bytes」三段式逐 arena 渲染，再出
//! Total（含 mmap）四行；`malloc_info` 面渲染 XML 骨架。渲染结果可被本模块
//! 自解析回读，且 system 总和须与内核映射账（brk + mmap）逐字节对平。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f670;
/// arena 号重复（主 arena 号 0 合法）
pub const MS_EBAD: u32 = SEG_BASE + 0x01;
/// in use 大于 system（账面不可能）
pub const MS_EINUSE: u32 = SEG_BASE + 0x02;
/// 与内核映射账不符
pub const MS_EMAP: u32 = SEG_BASE + 0x03;
/// 冻结后改写
pub const MS_EFROZEN: u32 = SEG_BASE + 0x04;
/// 空剖面
pub const MS_EEMPTY: u32 = SEG_BASE + 0x05;
/// 渲染/解析结构破损
pub const MS_ELINE: u32 = SEG_BASE + 0x06;
/// 重复冻结取值
pub const MS_EDUP: u32 = SEG_BASE + 0x07;
/// mmap 字节大于 system
pub const MS_EMAPBYTES: u32 = SEG_BASE + 0x08;

/// `malloc_stats` 的 system 行前缀（glibc 逐字，含 5 空格对齐）。
pub const SYS_PREFIX: &str = "system bytes     = ";
/// `malloc_stats` 的 in use 行前缀。
pub const USE_PREFIX: &str = "in use bytes     = ";
/// Total 段标题。
pub const TOTAL_LINE: &str = "Total (incl. mmap):";

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 单 arena 账面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arena {
    pub id: u32,
    pub system: u64,
    pub in_use: u64,
    pub mmap_regions: u32,
    pub mmap_bytes: u64,
}

impl Arena {
    /// 账面自洽：in use 不超 system、mmap 字节不超 system。
    pub fn coherent(&self) -> bool {
        self.in_use <= self.system && self.mmap_bytes <= self.system
    }
}

/// 堆剖面：arena 表 + 冻结位。
#[derive(Clone, Debug)]
pub struct HeapProfile {
    arenas: Vec<Arena>,
    frozen: bool,
    pub renders: u64,
    pub rejects: u64,
}

impl HeapProfile {
    /// 空剖面。
    pub fn empty() -> HeapProfile {
        HeapProfile { arenas: Vec::new(), frozen: false, renders: 0, rejects: 0 }
    }

    pub fn len(&self) -> usize {
        self.arenas.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn arena(&self, id: u32) -> Option<&Arena> {
        let mut k = 0usize;
        while k < self.arenas.len() {
            if let Some(a) = self.arenas.get(k) {
                if a.id == id {
                    return Some(a);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 挂 arena：重号、in use 超 system、mmap 超 system 三关。
    pub fn add(&mut self, a: Arena) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_EFROZEN);
        }
        let r = self.add_inner(a);
        if r.is_err() {
            self.rejects = self.rejects.wrapping_add(1);
        }
        r
    }

    fn add_inner(&mut self, a: Arena) -> Result<(), u32> {
        if self.arena(a.id).is_some() {
            return Err(MS_EBAD);
        }
        if a.in_use > a.system {
            return Err(MS_EINUSE);
        }
        if a.mmap_bytes > a.system {
            return Err(MS_EMAPBYTES);
        }
        self.arenas.push(a);
        self.arenas.sort_by_key(|x| x.id);
        Ok(())
    }

    /// system 总和。
    pub fn total_system(&self) -> u64 {
        let mut n = 0u64;
        let mut k = 0usize;
        while k < self.arenas.len() {
            if let Some(a) = self.arenas.get(k) {
                n = n.wrapping_add(a.system);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// in use 总和。
    pub fn total_in_use(&self) -> u64 {
        let mut n = 0u64;
        let mut k = 0usize;
        while k < self.arenas.len() {
            if let Some(a) = self.arenas.get(k) {
                n = n.wrapping_add(a.in_use);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// mmap 区数最大值（glibc 语义是「历史最大」，此处取各 arena 当前最大）。
    pub fn max_mmap_regions(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.arenas.len() {
            if let Some(a) = self.arenas.get(k) {
                if a.mmap_regions > n {
                    n = a.mmap_regions;
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// mmap 字节总和。
    pub fn total_mmap_bytes(&self) -> u64 {
        let mut n = 0u64;
        let mut k = 0usize;
        while k < self.arenas.len() {
            if let Some(a) = self.arenas.get(k) {
                n = n.wrapping_add(a.mmap_bytes);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// `malloc_stats` 渲染：逐 arena 三行 + Total 段五行。
    pub fn render_stats(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.arenas.len() {
            if let Some(a) = self.arenas.get(k) {
                out.push(format!("Arena {}:", a.id));
                out.push(format!("{}{}", SYS_PREFIX, a.system));
                out.push(format!("{}{}", USE_PREFIX, a.in_use));
            }
            k = k.wrapping_add(1);
        }
        out.push(TOTAL_LINE.to_string());
        out.push(format!("{}{}", SYS_PREFIX, self.total_system()));
        out.push(format!("{}{}", USE_PREFIX, self.total_in_use()));
        out.push(format!("max mmap regions = {}", self.max_mmap_regions()));
        out.push(format!("max mmap bytes   = {}", self.total_mmap_bytes()));
        out
    }

    /// `malloc_info` XML 骨架：`<malloc version="1">` 下逐 arena 一个 heap 节点。
    pub fn render_info_xml(&self) -> String {
        let mut s = String::new();
        s.push_str("<malloc version=\"1\">\n");
        let mut k = 0usize;
        while k < self.arenas.len() {
            if let Some(a) = self.arenas.get(k) {
                s.push_str(&format!("<heap nr=\"{}\">", a.id));
                s.push_str(&format!(
                    "<total type=\"rest\" count=\"{}\" size=\"{}\"/>",
                    a.mmap_regions, a.system
                ));
                s.push_str("<system type=\"current\" size=\"");
                s.push_str(&a.system.to_string());
                s.push_str("\"/></heap>\n");
            }
            k = k.wrapping_add(1);
        }
        s.push_str("</malloc>\n");
        s
    }

    /// 自解析：核对渲染面与账面逐数一致。
    pub fn parse_rendered(&self, lines: &[String]) -> Result<(u64, u64, u32, u64), u32> {
        let mut at_total = false;
        let mut sys = 0u64;
        let mut useb = 0u64;
        let mut regions = 0u32;
        let mut mmapb = 0u64;
        let mut seen = 0u8;
        let mut k = 0usize;
        while k < lines.len() {
            if let Some(line) = lines.get(k) {
                if line == TOTAL_LINE {
                    at_total = true;
                } else if at_total {
                    if let Some(v) = line.strip_prefix(SYS_PREFIX) {
                        sys = parse_u64(v).ok_or(MS_ELINE)?;
                        seen |= 1;
                    } else if let Some(v) = line.strip_prefix(USE_PREFIX) {
                        useb = parse_u64(v).ok_or(MS_ELINE)?;
                        seen |= 2;
                    } else if let Some(v) = line.strip_prefix("max mmap regions = ") {
                        regions = parse_u32(v).ok_or(MS_ELINE)?;
                        seen |= 4;
                    } else if let Some(v) = line.strip_prefix("max mmap bytes   = ") {
                        mmapb = parse_u64(v).ok_or(MS_ELINE)?;
                        seen |= 8;
                    }
                }
            }
            k = k.wrapping_add(1);
        }
        if seen != 15 {
            return Err(MS_ELINE);
        }
        Ok((sys, useb, regions, mmapb))
    }

    /// 与内核映射账交叉核对：system 总和须等于 brk + mmap 总映射。
    pub fn cross_check(&self, account: &MapAccount) -> Result<u64, u32> {
        if self.arenas.is_empty() {
            return Err(MS_EEMPTY);
        }
        let mapped = account.total();
        if mapped != self.total_system() {
            return Err(MS_EMAP);
        }
        Ok(mapped)
    }

    /// 面指纹：逐 arena 五字段链式 fold。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.arenas.len() {
            if let Some(a) = self.arenas.get(k) {
                h = (h ^ a.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.system).wrapping_mul(FNV_PRIME);
                h = (h ^ a.in_use).wrapping_mul(FNV_PRIME);
                h = (h ^ a.mmap_regions as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.mmap_bytes).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 冻结：幂等取回锁定指纹。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return Some(self.fold());
        }
        self.frozen = true;
        Some(self.fold())
    }
}

/// 内核映射账：brk 段 + mmap 段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapAccount {
    pub brk_bytes: u64,
    pub mmap_bytes: u64,
}

impl MapAccount {
    pub fn new(brk_bytes: u64, mmap_bytes: u64) -> MapAccount {
        MapAccount { brk_bytes, mmap_bytes }
    }

    pub fn total(&self) -> u64 {
        self.brk_bytes.wrapping_add(self.mmap_bytes)
    }
}

/// 十进制解析（仅 ASCII 数字，空串与非法字符返回 None）。
pub fn parse_u64(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut v = 0u64;
    let mut k = 0usize;
    while k < bytes.len() {
        let b = match bytes.get(k) {
            Some(x) => *x,
            None => return None,
        };
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add((b - b'0') as u64);
        k = k.wrapping_add(1);
    }
    Some(v)
}

/// u32 解析（复用量级检查）。
pub fn parse_u32(s: &str) -> Option<u32> {
    match parse_u64(s) {
        Some(v) if v <= u32::MAX as u64 => Some(v as u32),
        _ => None,
    }
}

/// 标准剖面：三 arena（数值为固定样本），映射账与之逐字节对平。
pub fn build_standard() -> (HeapProfile, MapAccount) {
    let mut p = HeapProfile::empty();
    let _ = p.add(Arena { id: 0, system: 135_168, in_use: 74_896, mmap_regions: 2, mmap_bytes: 20_480 });
    let _ = p.add(Arena { id: 1, system: 65_536, in_use: 1_024, mmap_regions: 0, mmap_bytes: 0 });
    let _ = p.add(Arena { id: 2, system: 262_144, in_use: 200_000, mmap_regions: 1, mmap_bytes: 65_536 });
    let acct = MapAccount::new(200_000, 262_848);
    (p, acct)
}

/// 四路注入：重号 / in use 超 system / 映射账不符 / 冻结后改写，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // ① arena 重号
    let mut a = HeapProfile::empty();
    let _ = a.add(Arena { id: 0, system: 4096, in_use: 1, mmap_regions: 0, mmap_bytes: 0 });
    got[0] = a
        .add(Arena { id: 0, system: 8192, in_use: 1, mmap_regions: 0, mmap_bytes: 0 })
        .err()
        == Some(MS_EBAD);

    // ② in use 超 system
    let mut b = HeapProfile::empty();
    got[1] = b
        .add(Arena { id: 5, system: 100, in_use: 101, mmap_regions: 0, mmap_bytes: 0 })
        .err()
        == Some(MS_EINUSE);

    // ③ 内核映射账不符
    let (c, _) = build_standard();
    let bad_acct = MapAccount::new(1, 2);
    got[2] = c.cross_check(&bad_acct).err() == Some(MS_EMAP);

    // ④ 冻结后改写 + 冻结幂等
    let (mut d, acct) = build_standard();
    let root = d.freeze();
    got[3] = root.is_some()
        && d.freeze() == root
        && d.add(Arena { id: 9, system: 1, in_use: 1, mmap_regions: 0, mmap_bytes: 0 })
            .err()
            == Some(MS_EFROZEN)
        && d.cross_check(&acct).is_ok();
    got
}

/// 判据台：堆剖面诊断消费面的十六条判据。
pub fn run_unx11292_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11292");

    let (mut prof, acct) = build_standard();
    let lines = prof.render_stats();
    let xml = prof.render_info_xml();
    let parsed = prof.parse_rendered(&lines);

    // J1 arena 表齐备
    s.add(
        "J1-arena 齐备",
        prof.len() == 3 && prof.arena(0).is_some() && prof.arena(2).is_some(),
        "三 arena 按号可查",
    );

    // J2 总额对平
    s.add(
        "J2-总额对平",
        prof.total_system() == 462_848 && prof.total_in_use() == 275_920,
        "system 462848、in use 275920 与样本一致",
    );

    // J3 逐 arena 账面自洽
    let mut coherent = true;
    let mut k = 0usize;
    while k < prof.len() {
        if let Some(a) = prof.arena(k as u32) {
            if !a.coherent() {
                coherent = false;
            }
        }
        k = k.wrapping_add(1);
    }
    s.add("J3-账面自洽", coherent, "in use 与 mmap 字节均不超 system");

    // J4 渲染行数结构
    s.add(
        "J4-渲染行数",
        lines.len() == prof.len() * 3 + 5
            && lines.iter().filter(|l| l.starts_with("Arena ")).count() == prof.len(),
        "逐 arena 三行 + Total 段五行",
    );

    // J5 渲染可自解析且逐数一致
    s.add(
        "J5-渲染回读",
        parsed
            == Ok((
                prof.total_system(),
                prof.total_in_use(),
                prof.max_mmap_regions(),
                prof.total_mmap_bytes(),
            )),
        "Total 段四数解析结果与账面逐数相等",
    );

    // J6 XML 骨架结构
    let heaps = xml.matches("<heap nr=").count();
    s.add(
        "J6-XML 骨架",
        xml.starts_with("<malloc version=\"1\">")
            && xml.ends_with("</malloc>\n")
            && heaps == prof.len(),
        "malloc_info 骨架与 heap 节点数对位",
    );

    // J7 内核映射账交叉一致
    s.add(
        "J7-映射账交叉",
        prof.cross_check(&acct) == Ok(acct.total()) && acct.total() == prof.total_system(),
        "system 总和等于 brk + mmap 映射总量",
    );

    // J8 映射账不符拒
    s.add(
        "J8-映射账不符拒",
        prof.cross_check(&MapAccount::new(0, 0)).err() == Some(MS_EMAP),
        "映射账归零即拒止",
    );

    // J9 重号拒
    let mut dup = HeapProfile::empty();
    let _ = dup.add(Arena { id: 1, system: 4096, in_use: 1, mmap_regions: 0, mmap_bytes: 0 });
    s.add(
        "J9-重号拒",
        dup.add(Arena { id: 1, system: 1, in_use: 1, mmap_regions: 0, mmap_bytes: 0 }).err()
            == Some(MS_EBAD)
            && dup.len() == 1,
        "同号 arena 二次挂载拒",
    );

    // J10 in use 超 system 拒
    let mut over = HeapProfile::empty();
    s.add(
        "J10-in use 超限拒",
        over.add(Arena { id: 2, system: 10, in_use: 11, mmap_regions: 0, mmap_bytes: 0 }).err()
            == Some(MS_EINUSE),
        "in use 大于 system 必拒",
    );

    // J11 mmap 字节超 system 拒
    let mut mb = HeapProfile::empty();
    s.add(
        "J11-mmap 超限拒",
        mb.add(Arena { id: 3, system: 10, in_use: 1, mmap_regions: 1, mmap_bytes: 11 }).err()
            == Some(MS_EMAPBYTES),
        "mmap 字节大于 system 必拒",
    );

    // J12 渲染破损可检出（尾行数字被破坏）
    let mut broken = lines.clone();
    if let Some(last) = broken.last_mut() {
        *last = "max mmap bytes   = ?".to_string();
    }
    s.add(
        "J12-破损可检出",
        prof.parse_rendered(&broken).err() == Some(MS_ELINE),
        "尾行数字非法即判结构破损",
    );

    // J13 空剖面拒交叉核对
    let e = HeapProfile::empty();
    s.add(
        "J13-空剖面拒",
        e.cross_check(&acct).err() == Some(MS_EEMPTY) && e.total_system() == 0,
        "空剖面不得与映射账对平",
    );

    // J14 冻结幂等
    let root = prof.freeze();
    s.add(
        "J14-冻结幂等",
        root.is_some() && prof.freeze() == root && prof.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J15 冻结拒改、只读仍通
    s.add(
        "J15-冻结拒改",
        prof.add(Arena { id: 9, system: 1, in_use: 1, mmap_regions: 0, mmap_bytes: 0 }).err()
            == Some(MS_EFROZEN)
            && prof.cross_check(&acct).is_ok(),
        "冻结后挂 arena 拒、交叉核对仍可读",
    );

    // J16 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J16-注入四拒", all, "重号/in use/映射账/冻结四路全拒");

    // J17 动态明细
    s.add_dyn(
        "J17-剖面明细",
        prof.total_mmap_bytes() == 86_016 && prof.max_mmap_regions() == 2,
        &format!(
            "arenas={} sys={} use={} mmap={}",
            prof.len(),
            prof.total_system(),
            prof.total_in_use(),
            prof.total_mmap_bytes()
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11292_malloc_flow() {
        let (mut p, acct) = build_standard();
        assert_eq!(p.len(), 3);
        assert_eq!(p.total_system(), 462_848);
        assert_eq!(p.total_in_use(), 275_920);
        assert_eq!(p.total_mmap_bytes(), 86_016);
        assert_eq!(p.cross_check(&acct), Ok(462_848));

        let lines = p.render_stats();
        assert_eq!(lines.len(), 14);
        assert_eq!(lines.first().map(|s| s.as_str()), Some("Arena 0:"));
        assert!(lines.iter().any(|l| l == TOTAL_LINE));
        assert_eq!(
            p.parse_rendered(&lines),
            Ok((462_848, 275_920, 2, 86_016))
        );

        let xml = p.render_info_xml();
        assert!(xml.starts_with("<malloc version=\"1\">"));
        assert_eq!(xml.matches("<heap nr=").count(), 3);

        assert_eq!(p.cross_check(&MapAccount::new(0, 0)).err(), Some(MS_EMAP));
        assert_eq!(
            p.add(Arena { id: 0, system: 1, in_use: 1, mmap_regions: 0, mmap_bytes: 0 }).err(),
            Some(MS_EBAD)
        );
        assert_eq!(
            p.add(Arena { id: 7, system: 1, in_use: 2, mmap_regions: 0, mmap_bytes: 0 }).err(),
            Some(MS_EINUSE)
        );

        assert!(p.freeze().is_some());
        assert_eq!(p.freeze(), p.freeze());
        assert_eq!(
            p.add(Arena { id: 8, system: 1, in_use: 1, mmap_regions: 0, mmap_bytes: 0 }).err(),
            Some(MS_EFROZEN)
        );

        assert_eq!(parse_u64("135168"), Some(135_168));
        assert_eq!(parse_u64(""), None);
        assert_eq!(parse_u64("12x"), None);

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11292_checks_green() {
        let s = run_unx11292_checks();
        assert!(s.all_passed(), "UNX-F11292 判据未全绿");
    }
}
