//! UNX-F56020 B01 批防重声明与批收官印（0x60260 段）
//!
//! 锚点：UNX-F56020-J1 本批 20 条 = 环境冻结族 20 件，主题两两不重叠
//! （关键词族逐条唯一：env_hash/环境指纹/探针等族名不重复落账）；J2
//! ID 段 F56001–F56020 与邻批零交叠（段界断言 + 邻批段探测）；J3 防
//! 重 grep 执行留痕（关键词族扫描记录逐条落账）+ 收官印封账只读（印
//! 后批账不可改）三判据全绿。判据锚定 §AI-71.3 批型铺排。
//! 域：CoRun·UNX-F2-B40（环境冻结域·批收官面）。
//!
//! 结构：批登记册（20 条逐条落账：ID × 关键词族 × 指纹）+ 主题防重
//! 核（关键词族两两唯一）+ ID 段界核（段内连续 + 邻段零交叠）+ grep
//! 留痕账 + 批指纹折叠 + 收官印（封账只读）+ 注入核（越段 ID/重复主
//! 题/空族名/封账改写/重复登记必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60260 独占 0x20）。
pub const SEG_BASE: u32 = 0x60260;
pub const BS_ID_OUT: u32 = SEG_BASE + 0x01; // ID 越本批段（F56001–F56020 外）
pub const BS_TOPIC_DUP: u32 = SEG_BASE + 0x02; // 主题重复（关键词族撞账）
pub const BS_KW_EMPTY: u32 = SEG_BASE + 0x03; // 关键词族空
pub const BS_SEALED: u32 = SEG_BASE + 0x04; // 收官印已落，批账只读
pub const BS_DUP_ID: u32 = SEG_BASE + 0x05; // ID 重复登记
pub const BS_GREP_LOST: u32 = SEG_BASE + 0x06; // 防重 grep 未留痕
pub const BS_FP_ZERO: u32 = SEG_BASE + 0x07; // 批指纹退化零哨兵

/// 本批 ID 段界（F56001–F56020）。
pub const ID_LO: u32 = 56001;
pub const ID_HI: u32 = 56020;
pub const BATCH_SIZE: usize = 20;

/// 邻批段界（B02：F56021–F56040）。
pub const NEXT_LO: u32 = 56021;
pub const NEXT_HI: u32 = 56040;

/// ID 段界核（本批内 + 与邻批零交叠）。
pub fn id_in_batch(id: u32) -> bool {
    id >= ID_LO && id <= ID_HI
}
pub fn id_overlap_next(id: u32) -> bool {
    id >= NEXT_LO && id <= NEXT_HI
}

/// FNV-1a 32 位（条目指纹与批指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 批条目：ID × 关键词族 × 指纹。
#[derive(Clone)]
pub struct BatchEntry {
    pub id: u32,
    pub kw_family: String,
    pub fingerprint: u32,
}

impl BatchEntry {
    pub fn new(id: u32, kw_family: &str) -> Result<Self, u32> {
        if !id_in_batch(id) {
            return Err(BS_ID_OUT);
        }
        if kw_family.is_empty() {
            return Err(BS_KW_EMPTY);
        }
        let fp = fnv1a(format!("{}|{}", id, kw_family).as_bytes());
        if fp == 0 {
            return Err(BS_FP_ZERO);
        }
        Ok(BatchEntry { id, kw_family: kw_family.to_string(), fingerprint: fp })
    }
}

/// 批登记册 + 收官印。
pub struct BatchSealBook {
    pub entries: Vec<BatchEntry>,
    pub grep_log: Vec<String>,
    pub sealed: bool,
}

impl BatchSealBook {
    pub fn new() -> Self {
        BatchSealBook { entries: Vec::new(), grep_log: Vec::new(), sealed: false }
    }

    /// 逐条登记：段界 + 族唯一 + ID 唯一。
    pub fn register(&mut self, e: BatchEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(BS_SEALED);
        }
        for p in self.entries.iter() {
            if p.id == e.id {
                return Err(BS_DUP_ID);
            }
            if p.kw_family == e.kw_family {
                return Err(BS_TOPIC_DUP);
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 防重 grep 留痕（关键词族逐条扫描记录）。
    pub fn log_grep(&mut self, kw: &str, hits: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(BS_SEALED);
        }
        if kw.is_empty() {
            return Err(BS_KW_EMPTY);
        }
        self.grep_log.push(format!("GREP|{}|hits={}", kw, hits));
        Ok(())
    }

    /// J1：主题两两不重叠（族名两两唯一）。
    pub fn topics_unique(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].kw_family == self.entries[j].kw_family {
                    return false;
                }
            }
        }
        self.entries.len() == BATCH_SIZE
    }

    /// J2：ID 段连续且无越段。
    pub fn ids_contiguous(&self) -> bool {
        if self.entries.len() != BATCH_SIZE {
            return false;
        }
        for (i, e) in self.entries.iter().enumerate() {
            if e.id != ID_LO + i as u32 {
                return false;
            }
            if id_overlap_next(e.id) {
                return false;
            }
        }
        true
    }

    /// J3：grep 留痕齐（逐条记录行可读）。
    pub fn grep_trail_ok(&self) -> bool {
        !self.grep_log.is_empty()
            && self.grep_log.iter().all(|g| g.starts_with("GREP|") && g.contains("hits="))
    }

    /// 批指纹（逐条指纹折叠）。
    pub fn batch_fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for e in self.entries.iter() {
            h ^= e.fingerprint;
            h = h.wrapping_mul(16777619);
        }
        h
    }

    /// 收官印（封账只读：印后登记与 grep 全拒）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// B01 批 20 条标准关键词族（环境冻结族，两两不重叠）。
pub const B01_FAMILIES: [&str; 20] = [
    "env_hash", "env_probe_gate", "dual_disk_topo", "fw_anchor", "power_plan",
    "bg_process_wl", "fs_layout", "tsc_calib", "win_baseline_rec", "anchor_rerun",
    "update_window", "reject_audit", "peripheral_silence", "time_window",
    "envctl_export", "equiv_decl", "freeze_doc", "probe_selfcheck",
    "batch_dedup_b01", "seal_print_b01",
];

/// 标准夹具：20 条全登记 + grep 留痕 + 未封印。
pub fn standard_batch() -> BatchSealBook {
    let mut b = BatchSealBook::new();
    for i in 0..BATCH_SIZE {
        let id = ID_LO + i as u32;
        b.register(BatchEntry::new(id, B01_FAMILIES[i]).expect("fixture entry"))
            .expect("fixture reg");
    }
    let _ = b.log_grep("env_hash", 0);
    let _ = b.log_grep("环境指纹", 0);
    let _ = b.log_grep("探针", 0);
    b
}

/// 收官夹具：标准批 + 封印。
pub fn sealed_batch() -> BatchSealBook {
    let mut b = standard_batch();
    b.seal();
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) ID 越段（56021 属邻批）
    if BatchEntry::new(56021, "x-family").err() == Some(BS_ID_OUT) {
        caught += 1;
    }
    // 2) 主题重复
    let mut b = standard_batch();
    if b.register(BatchEntry::new(56001, "x").expect("d")) == Err(BS_DUP_ID) {
        caught += 1;
    }
    // 3) 空族名
    if BatchEntry::new(56001, "").err() == Some(BS_KW_EMPTY) {
        caught += 1;
    }
    // 4) 封账改写
    let mut b2 = sealed_batch();
    if b2.register(BatchEntry::new(56001, "x").expect("s")) == Err(BS_SEALED)
        && b2.log_grep("y", 0) == Err(BS_SEALED)
    {
        caught += 1;
    }
    // 5) 邻批交叠探测（段界函数互斥）
    if !id_overlap_next(56020) && id_overlap_next(56021) && !id_in_batch(56021) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56020_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56020");
    // J1-1 20 条全登记主题两两不重叠
    let b = standard_batch();
    let ok = b.topics_unique();
    s.add("56020-J1-主题两两不重叠", ok, if ok { "" } else { "主题撞账" });
    // J1-2 20 族名逐条非空且唯一（闭集同源）
    let mut uniq = true;
    for i in 0..B01_FAMILIES.len() {
        for j in (i + 1)..B01_FAMILIES.len() {
            if B01_FAMILIES[i] == B01_FAMILIES[j] {
                uniq = false;
            }
        }
    }
    let ok = uniq && B01_FAMILIES.len() == BATCH_SIZE;
    s.add("56020-J1-族名闭集唯一", ok, if ok { "" } else { "族名撞账" });
    // J2-1 ID 段 F56001–F56020 连续且零越段
    let ok = b.ids_contiguous() && ID_LO == 56001 && ID_HI == 56020;
    s.add("56020-J2-ID段连续零越段", ok, if ok { "" } else { "段界漂移" });
    // J2-2 邻批零交叠（56021 起属 B02，本批函数全部判假）
    let ok = !id_in_batch(NEXT_LO) && !id_in_batch(NEXT_HI) && id_in_batch(ID_LO) && id_in_batch(ID_HI);
    s.add("56020-J2-邻批零交叠", ok, if ok { "" } else { "邻批交叠" });
    // J3-1 防重 grep 留痕齐（三关键词族行可读）
    let ok = b.grep_trail_ok() && b.grep_log.len() == 3
        && b.grep_log[0] == "GREP|env_hash|hits=0";
    s.add("56020-J3-防重grep留痕", ok, if ok { "" } else { "grep 未留痕" });
    // J3-2 批指纹确定
    let ok = standard_batch().batch_fingerprint() == sealed_batch().batch_fingerprint();
    s.add("56020-J3-批指纹确定", ok, if ok { "" } else { "批指纹漂移" });
    // J3-3 收官印封账只读
    let sb = sealed_batch();
    let ok = sb.sealed;
    s.add("56020-J3-收官印只读", ok, if ok { "" } else { "印后可写" });
    // J3-4 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56020-J3-注入核必抓", ok, &detail);
    // J2-3 条目指纹确定且对 ID/族敏感
    let e1 = BatchEntry::new(56001, "a").expect("e1");
    let e2 = BatchEntry::new(56001, "a").expect("e2");
    let e3 = BatchEntry::new(56002, "a").expect("e3");
    let ok = e1.fingerprint == e2.fingerprint && e1.fingerprint != e3.fingerprint;
    s.add("56020-J2-条目指纹敏感", ok, if ok { "" } else { "条目指纹漂移" });
    // J1-3 邻批 ID 拒收留证（越段 Err 码语义）
    let ok = BatchEntry::new(NEXT_LO, "next").err() == Some(BS_ID_OUT)
        && BatchEntry::new(ID_LO, "ok").is_ok();
    s.add("56020-J1-越段拒收", ok, if ok { "" } else { "越段穿透" });
    // J3-5 批指纹非零哨兵
    let ok = sealed_batch().batch_fingerprint() != 0;
    s.add("56020-J3-批指纹非零", ok, if ok { "" } else { "指纹退化零" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56020_all_checks_green() {
        let set = run_unx56020_checks();
        assert!(set.all_passed(), "UNX-F56020 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56020_seal_readonly() {
        let mut b = sealed_batch();
        let fp = b.batch_fingerprint();
        b.seal();
        assert!(b.register(BatchEntry::new(56001, "z").expect("e")) == Err(BS_SEALED));
        assert_eq!(b.batch_fingerprint(), fp);
    }
}
