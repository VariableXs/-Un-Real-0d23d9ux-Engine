//! UNX-F56030 NT API 族语义等价映射件（0x603A0 段）
//!
//! 锚点：UNX-F56030-J1 Varix syscall 族↔Windows NT API 族（NtQuery
//! InformationProcess 类替代 getpid 语义等价声明）映射表逐族落账并声
//! 明 L1–L4 等级；J2 映射表冻结走 mini-ADR（ADR 号 × 决议行逐条留痕，
//! 封后不可改）；J3 等级降级留痕（改级必须新 ADR 行，旧行保留）三判
//! 据全绿。判据锚定 §AI-71.2 专题三+§AI-71.4。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·NT API 映射面）。
//!
//! 结构：族映射闭集登记册（Varix 族 × NT API 族 × 等级逐族落账）+
//! mini-ADR 决议账（逐条 ADR 行只追加）+ 等级降级链（新行覆议旧行在
//! 账）+ 双向解析核（族名↔NT 名双向可查）+ 注入核（越闭集族/越闭集等
//! 级/空 NT 名/ADR 封后改写/重复映射必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x603A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x603A0;
pub const NT_FAM_UNK: u32 = SEG_BASE + 0x01; // 越闭集 syscall 族
pub const NT_LEVEL_UNK: u32 = SEG_BASE + 0x02; // 越闭集等级
pub const NT_NAME_EMPTY: u32 = SEG_BASE + 0x03; // NT API 名空
pub const NT_ADR_SEALED: u32 = SEG_BASE + 0x04; // ADR 已封账拒追加
pub const NT_DUP_MAP: u32 = SEG_BASE + 0x05; // 同族重复映射
pub const NT_REV_MISS: u32 = SEG_BASE + 0x06; // 反向解析缺
pub const NT_SEQ_BACK: u32 = SEG_BASE + 0x07; // ADR 行号回退

/// syscall 族闭集。
pub const F_GETPID: usize = 0;
pub const F_READ: usize = 1;
pub const F_WRITE: usize = 2;
pub const F_MMAP: usize = 3;
pub const F_CTX: usize = 4;
pub const F_COUNT: usize = 5;
pub const F_NAMES: [&str; F_COUNT] = ["getpid", "read", "write", "mmap", "ctx_switch"];

/// NT API 侧等价名（同序）。
pub const NT_NAMES: [&str; F_COUNT] = [
    "NtQueryInformationProcess",
    "NtReadFile",
    "NtWriteFile",
    "NtMapViewOfSection",
    "NtWaitForSingleObject",
];

/// 等级闭集（L1 强等价 → L4 弱声明）。
pub const LV_L1: usize = 0;
pub const LV_L2: usize = 1;
pub const LV_L3: usize = 2;
pub const LV_L4: usize = 3;
pub const LV_COUNT: usize = 4;
pub const LV_NAMES: [&str; LV_COUNT] = ["L1", "L2", "L3", "L4"];

/// 标准等级（getpid L1 / read L1 / write L1 / mmap L2 / ctx L3）。
pub const STANDARD_LEVELS: [usize; F_COUNT] = [LV_L1, LV_L1, LV_L1, LV_L2, LV_L3];

pub fn level_ok(l: usize) -> bool {
    l < LV_COUNT
}

/// FNV-1a 32 位（映射表指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 族映射账目：族 × NT 名 × 等级 × 指纹。
#[derive(Clone)]
pub struct MapEntry {
    pub fam: usize,
    pub nt_name: String,
    pub level: usize,
    pub fingerprint: u32,
}

impl MapEntry {
    pub fn new(fam: usize, nt_name: &str, level: usize) -> Result<Self, u32> {
        if fam >= F_COUNT {
            return Err(NT_FAM_UNK);
        }
        if !level_ok(level) {
            return Err(NT_LEVEL_UNK);
        }
        if nt_name.is_empty() {
            return Err(NT_NAME_EMPTY);
        }
        let fp = fnv1a(format!("{}|{}|{}", F_NAMES[fam], nt_name, level).as_bytes());
        Ok(MapEntry { fam, nt_name: nt_name.to_string(), level, fingerprint: fp })
    }
}

/// mini-ADR 决议行：行号 × 族 × 决议（等级）× 说明。
#[derive(Clone)]
pub struct AdrRow {
    pub seq: u32,
    pub fam: usize,
    pub level: usize,
    pub note: String,
}

/// 映射册 + ADR 决议账。
pub struct NtMapBook {
    pub maps: Vec<MapEntry>,
    pub adr: Vec<AdrRow>,
    pub adr_sealed: bool,
}

impl NtMapBook {
    pub fn new() -> Self {
        NtMapBook { maps: Vec::new(), adr: Vec::new(), adr_sealed: false }
    }

    /// 逐族落映射（同族重复拒）。
    pub fn map(&mut self, e: MapEntry) -> Result<(), u32> {
        for m in self.maps.iter() {
            if m.fam == e.fam {
                return Err(NT_DUP_MAP);
            }
        }
        self.maps.push(e);
        Ok(())
    }

    /// ADR 决议追加（行号严格递增 + 封账拒）。
    pub fn adr_append(&mut self, row: AdrRow) -> Result<(), u32> {
        if self.adr_sealed {
            return Err(NT_ADR_SEALED);
        }
        if row.fam >= F_COUNT || !level_ok(row.level) {
            return Err(NT_FAM_UNK);
        }
        if let Some(prev) = self.adr.last() {
            if row.seq <= prev.seq {
                return Err(NT_SEQ_BACK);
            }
        }
        self.adr.push(row);
        Ok(())
    }

    /// J3：等级降级留痕——ADR 账内同族多行且末行生效、旧行保留。
    pub fn downgrade_trail_ok(&self, fam: usize) -> bool {
        let mut hits = 0usize;
        for r in self.adr.iter() {
            if r.fam == fam {
                hits += 1;
            }
        }
        hits >= 2
    }

    /// ADR 生效等级（末行）。
    pub fn adr_effective(&self, fam: usize) -> Option<usize> {
        let mut eff: Option<usize> = None;
        for r in self.adr.iter() {
            if r.fam == fam {
                eff = Some(r.level);
            }
        }
        eff
    }

    /// 正向解析：族 → NT 名。
    pub fn to_nt(&self, fam: usize) -> Option<&str> {
        for m in self.maps.iter() {
            if m.fam == fam {
                return Some(&m.nt_name);
            }
        }
        None
    }

    /// 反向解析：NT 名 → 族。
    pub fn to_fam(&self, nt: &str) -> Option<usize> {
        for m in self.maps.iter() {
            if m.nt_name == nt {
                return Some(m.fam);
            }
        }
        None
    }

    /// J1：五族全映射且等级齐。
    pub fn full(&self) -> bool {
        if self.maps.len() != F_COUNT {
            return false;
        }
        for f in 0..F_COUNT {
            match self.to_nt(f) {
                None => return false,
                Some(_) => {}
            }
        }
        true
    }

    /// 映射表指纹（逐族折叠）。
    pub fn map_fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for m in self.maps.iter() {
            h ^= m.fingerprint;
            h = h.wrapping_mul(16777619);
        }
        h
    }

    /// ADR 封账（mini-ADR 冻结）。
    pub fn seal_adr(&mut self) {
        self.adr_sealed = true;
    }
}

/// 标准夹具：五族标准映射 + ADR 两行（getpid 初议 L1）。
pub fn standard_map() -> NtMapBook {
    let mut b = NtMapBook::new();
    for f in 0..F_COUNT {
        b.map(MapEntry::new(f, NT_NAMES[f], STANDARD_LEVELS[f]).expect("fixture map"))
            .expect("fixture reg");
    }
    b.adr_append(AdrRow {
        seq: 1,
        fam: F_GETPID,
        level: LV_L1,
        note: "初议：NtQueryInformationProcess 覆盖 getpid 语义".to_string(),
    })
    .expect("adr1");
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集族（fam=9）
    if MapEntry::new(9, "NtX", LV_L1).err() == Some(NT_FAM_UNK) {
        caught += 1;
    }
    // 2) 越闭集等级（level=7）
    if MapEntry::new(F_READ, "NtReadFile", 7).err() == Some(NT_LEVEL_UNK) {
        caught += 1;
    }
    // 3) NT 名空
    if MapEntry::new(F_READ, "", LV_L1).err() == Some(NT_NAME_EMPTY) {
        caught += 1;
    }
    // 4) 重复映射
    let mut b = standard_map();
    if b.map(MapEntry::new(F_GETPID, "NtDup", LV_L2).expect("dup")) == Err(NT_DUP_MAP) {
        caught += 1;
    }
    // 5) ADR 封账拒追加 + 行号回退
    let mut b2 = standard_map();
    b2.seal_adr();
    if b2.adr_append(AdrRow { seq: 2, fam: F_READ, level: LV_L1, note: "x".to_string() })
        == Err(NT_ADR_SEALED)
    {
        caught += 1;
    }
    let mut b3 = standard_map();
    if b3.adr_append(AdrRow { seq: 1, fam: F_READ, level: LV_L1, note: "y".to_string() })
        == Err(NT_SEQ_BACK)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 映射表渲染（对照面：逐族一行）。
pub fn render_map(b: &NtMapBook) -> String {
    let mut out = String::new();
    for f in 0..F_COUNT {
        match b.to_nt(f) {
            Some(nt) => {
                out.push_str(&format!("{}|{}|{}\n", F_NAMES[f], nt, LV_NAMES[level_of(b, f)]));
            }
            None => {
                out.push_str(&format!("{}|UNMAPPED\n", F_NAMES[f]));
            }
        }
    }
    out
}

/// 某族等级（无映射返回 9 哨兵）。
fn level_of(b: &NtMapBook, fam: usize) -> usize {
    for m in b.maps.iter() {
        if m.fam == fam {
            return m.level;
        }
    }
    9
}

/// 判据入口（CheckSet）。
pub fn run_unx56030_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56030");
    // J1-1 五族全映射
    let b = standard_map();
    let ok = b.full();
    s.add("56030-J1-五族全映射", ok, if ok { "" } else { "映射缺族" });
    // J1-2 getpid ↔ NtQueryInformationProcess 语义声明
    let ok = b.to_nt(F_GETPID) == Some("NtQueryInformationProcess");
    s.add("56030-J1-getpid等价声明", ok, if ok { "" } else { "等价声明缺" });
    // J1-3 等级账与标准一致
    let mut lv_ok = true;
    for f in 0..F_COUNT {
        let mut lv = 9usize;
        for m in b.maps.iter() {
            if m.fam == f {
                lv = m.level;
            }
        }
        if lv != STANDARD_LEVELS[f] {
            lv_ok = false;
        }
    }
    let ok = lv_ok;
    s.add("56030-J1-等级账一致", ok, if ok { "" } else { "等级漂移" });
    // J2-1 mini-ADR 决议账留痕（行 1 初议 getpid L1）
    let ok = b.adr.len() == 1
        && b.adr[0].seq == 1
        && b.adr_effective(F_GETPID) == Some(LV_L1)
        && b.adr[0].note.contains("NtQueryInformationProcess");
    s.add("56030-J2-ADR决议留痕", ok, if ok { "" } else { "ADR 留痕缺" });
    // J2-2 等级降级留痕（追加 L2 覆议行：旧行在账 + 末行生效）
    let mut b2 = standard_map();
    b2.adr_append(AdrRow {
        seq: 2,
        fam: F_GETPID,
        level: LV_L2,
        note: "覆议：句柄语义差异降 L2".to_string(),
    })
    .expect("adr2");
    let ok = b2.adr.len() == 2
        && b2.adr[0].level == LV_L1
        && b2.adr_effective(F_GETPID) == Some(LV_L2)
        && b2.downgrade_trail_ok(F_GETPID);
    s.add("56030-J2-降级留痕旧行在账", ok, if ok { "" } else { "降级链断" });
    // J2-3 映射表指纹确定且敏感
    let fp1 = standard_map().map_fingerprint();
    let fp2 = standard_map().map_fingerprint();
    let mut b3 = standard_map();
    b3.maps[0].level = LV_L2;
    let ok = fp1 == fp2 && fp1 != b3.map_fingerprint() && fp1 != 0;
    s.add("56030-J2-表指纹敏感", ok, if ok { "" } else { "指纹漂移" });
    // J3-1 双向解析闭包
    let mut bidir = true;
    for f in 0..F_COUNT {
        match b.to_nt(f) {
            None => bidir = false,
            Some(nt) => {
                if b.to_fam(nt) != Some(f) {
                    bidir = false;
                }
            }
        }
    }
    let ok = bidir;
    s.add("56030-J3-双向解析闭包", ok, if ok { "" } else { "双向解析断" });
    // J3-2 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56030-J3-注入核必抓", ok, &detail);
    // J3-3 缺射显性（未映射名反向 None）
    let ok = b.to_fam("NtGhost").is_none();
    s.add("56030-J3-缺射显性", ok, if ok { "" } else { "缺射被猜" });
    // J3-4 ADR 封账冻结
    let mut b4 = standard_map();
    b4.seal_adr();
    let ok = b4.adr_sealed
        && b4.adr_append(AdrRow { seq: 9, fam: F_READ, level: LV_L1, note: "s".to_string() })
            == Err(NT_ADR_SEALED);
    s.add("56030-J3-ADR封账冻结", ok, if ok { "" } else { "封账后可追加" });
    // J1-4 渲染对照面齐（五行无 UNMAPPED）
    let text = render_map(&standard_map());
    let ok = text.lines().count() == F_COUNT
        && !text.contains("UNMAPPED")
        && text.contains("getpid|NtQueryInformationProcess|L1");
    s.add("56030-J1-渲染对照面齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-5 族名/等级名闭集同源
    let ok = F_NAMES[0] == "getpid" && F_NAMES[4] == "ctx_switch" && LV_NAMES[3] == "L4";
    s.add("56030-J1-命名同源", ok, if ok { "" } else { "命名漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56030_all_checks_green() {
        let set = run_unx56030_checks();
        assert!(set.all_passed(), "UNX-F56030 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56030_bidirectional() {
        let b = standard_map();
        assert_eq!(b.to_fam("NtReadFile"), Some(F_READ));
        assert_eq!(b.to_nt(F_MMAP), Some("NtMapViewOfSection"));
    }
}
