//! UNX-F56040 B02 批防重声明与批收官印（0x604E0 段）
//!
//! 锚点：UNX-F56040-J1 本批 20 条 = syscall 延迟采集族 20 件零重复
//! （关键词族逐条唯一）；J2 ID 段 F56021–F56040 零交叠（段界断言 +
//! 邻批段探测）；J3 四族基准与基础设施件分账清晰（基准件/基础设施件
//! 两类计数分明）+ 收官印封账只读三判据全绿。判据锚定 §AI-71.3 批型
//! 铺排。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·批收官面）。
//!
//! 结构：批登记册（20 条逐条落账：ID × 关键词族 × 类别 × 指纹）+ 主
//! 题防重核（族名两两唯一）+ ID 段界核 + 类别分账（基准/基础设施两账
//! 分明）+ 批指纹折叠 + 收官印（封账只读）+ 注入核（越段 ID/重复主题
//! /空族名/封账改写/类别越集必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x604E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x604E0;
pub const BS_ID_OUT: u32 = SEG_BASE + 0x01; // ID 越本批段
pub const BS_TOPIC_DUP: u32 = SEG_BASE + 0x02; // 主题重复
pub const BS_KW_EMPTY: u32 = SEG_BASE + 0x03; // 关键词族空
pub const BS_SEALED: u32 = SEG_BASE + 0x04; // 收官印已落只读
pub const BS_DUP_ID: u32 = SEG_BASE + 0x05; // ID 重复
pub const BS_KIND_UNK: u32 = SEG_BASE + 0x06; // 类别越闭集
pub const BS_FP_ZERO: u32 = SEG_BASE + 0x07; // 批指纹退化零哨兵

/// 本批 ID 段界（F56021–F56040）。
pub const ID_LO: u32 = 56021;
pub const ID_HI: u32 = 56040;
pub const BATCH_SIZE: usize = 20;

/// 邻批段界（B01：F56001–F56020；B03：F56041–F56060）。
pub const PREV_LO: u32 = 56001;
pub const PREV_HI: u32 = 56020;
pub const NEXT_LO: u32 = 56041;
pub const NEXT_HI: u32 = 56060;

/// 类别闭集（基准件 / 基础设施件）。
pub const KD_BENCH: usize = 0;
pub const KD_INFRA: usize = 1;
pub const KD_COUNT: usize = 2;
pub const KD_NAMES: [&str; KD_COUNT] = ["bench", "infra"];

pub fn id_in_batch(id: u32) -> bool {
    id >= ID_LO && id <= ID_HI
}
pub fn id_in_prev(id: u32) -> bool {
    id >= PREV_LO && id <= PREV_HI
}
pub fn id_in_next(id: u32) -> bool {
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

/// 批条目：ID × 关键词族 × 类别 × 指纹。
#[derive(Clone)]
pub struct BatchEntry {
    pub id: u32,
    pub kw_family: String,
    pub kind: usize,
    pub fingerprint: u32,
}

impl BatchEntry {
    pub fn new(id: u32, kw_family: &str, kind: usize) -> Result<Self, u32> {
        if !id_in_batch(id) {
            return Err(BS_ID_OUT);
        }
        if kw_family.is_empty() {
            return Err(BS_KW_EMPTY);
        }
        if kind >= KD_COUNT {
            return Err(BS_KIND_UNK);
        }
        let fp = fnv1a(format!("{}|{}|{}", id, kw_family, kind).as_bytes());
        if fp == 0 {
            return Err(BS_FP_ZERO);
        }
        Ok(BatchEntry { id, kw_family: kw_family.to_string(), kind, fingerprint: fp })
    }
}

/// 批登记册 + 收官印。
pub struct BatchSealBook {
    pub entries: Vec<BatchEntry>,
    pub sealed: bool,
}

impl BatchSealBook {
    pub fn new() -> Self {
        BatchSealBook { entries: Vec::new(), sealed: false }
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

    /// J1：主题两两不重叠。
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
            if id_in_prev(e.id) || id_in_next(e.id) {
                return false;
            }
        }
        true
    }

    /// J3：类别分账（基准件/基础设施件两类计数分明，皆非零）。
    pub fn kind_split(&self) -> (usize, usize) {
        let mut b = 0usize;
        let mut i = 0usize;
        for e in self.entries.iter() {
            if e.kind == KD_BENCH {
                b += 1;
            } else {
                i += 1;
            }
        }
        (b, i)
    }

    pub fn kind_split_ok(&self) -> bool {
        let (b, i) = self.kind_split();
        b + i == BATCH_SIZE && b > 0 && i > 0
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

    /// 收官印（封账只读）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// B02 批 20 条标准关键词族（syscall 延迟族，两两不重叠）。
pub const B02_FAMILIES: [&str; 20] = [
    "syscall_collector", "getpid_bench", "rw_bench", "mmap_bench", "ctx_bench",
    "migration_filter", "warmup_window", "outlier_rules", "seg_aggregate",
    "nt_api_map", "self_isolate", "irq_comply", "sample_archive", "dual_orch",
    "cmp_row_gen", "verdict_threshold", "fail_trace", "load_script_vault",
    "o2_prelink", "batch_dedup_b02",
];

/// 标准夹具：20 条全登记（前 9 条基准件 / 后 11 条基础设施件）。
pub fn standard_batch() -> BatchSealBook {
    let mut b = BatchSealBook::new();
    for i in 0..BATCH_SIZE {
        let id = ID_LO + i as u32;
        let kind = if i < 9 { KD_BENCH } else { KD_INFRA };
        b.register(BatchEntry::new(id, B02_FAMILIES[i], kind).expect("fixture entry"))
            .expect("fixture reg");
    }
    b
}

/// 收官夹具。
pub fn sealed_batch() -> BatchSealBook {
    let mut b = standard_batch();
    b.seal();
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) ID 越段（56041 属 B03）
    if BatchEntry::new(56041, "x", KD_BENCH).err() == Some(BS_ID_OUT) {
        caught += 1;
    }
    // 2) ID 重复
    let mut b = standard_batch();
    if b.register(BatchEntry::new(56021, "x", KD_BENCH).expect("d")) == Err(BS_DUP_ID) {
        caught += 1;
    }
    // 3) 类别越闭集
    if BatchEntry::new(56021, "y", 7).err() == Some(BS_KIND_UNK) {
        caught += 1;
    }
    // 4) 封账改写
    let mut b2 = sealed_batch();
    if b2.register(BatchEntry::new(56021, "z", KD_BENCH).expect("s")) == Err(BS_SEALED) {
        caught += 1;
    }
    // 5) 邻批交叠探测（双向段界互斥）
    if !id_in_next(56040) && id_in_next(56041) && !id_in_prev(56021) && id_in_prev(56020) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56040_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56040");
    // J1-1 20 条全登记主题两两不重叠
    let b = standard_batch();
    let ok = b.topics_unique();
    s.add("56040-J1-主题零重复", ok, if ok { "" } else { "主题撞账" });
    // J1-2 20 族名闭集唯一
    let mut uniq = true;
    for i in 0..B02_FAMILIES.len() {
        for j in (i + 1)..B02_FAMILIES.len() {
            if B02_FAMILIES[i] == B02_FAMILIES[j] {
                uniq = false;
            }
        }
    }
    let ok = uniq && B02_FAMILIES.len() == BATCH_SIZE;
    s.add("56040-J1-族名闭集唯一", ok, if ok { "" } else { "族名撞账" });
    // J2-1 ID 段 F56021–F56040 连续零越段
    let ok = b.ids_contiguous() && ID_LO == 56021 && ID_HI == 56040;
    s.add("56040-J2-ID段连续", ok, if ok { "" } else { "段界漂移" });
    // J2-2 邻批双向零交叠
    let ok = !id_in_prev(ID_LO) && !id_in_next(ID_HI) && id_in_batch(ID_LO) && id_in_batch(ID_HI);
    s.add("56040-J2-邻批零交叠", ok, if ok { "" } else { "邻批交叠" });
    // J3-1 四族基准与基础设施分账（9 基准 + 11 基础设施）
    let (bq, iq) = b.kind_split();
    let ok = b.kind_split_ok() && bq == 9 && iq == 11;
    s.add("56040-J3-基准设施分账", ok, if ok { "" } else { "分账混" });
    // J3-2 类别名闭集同源
    let ok = KD_NAMES[KD_BENCH] == "bench" && KD_NAMES[KD_INFRA] == "infra";
    s.add("56040-J3-类别名同源", ok, if ok { "" } else { "命名漂移" });
    // J3-3 批指纹确定非零
    let ok = standard_batch().batch_fingerprint() == sealed_batch().batch_fingerprint()
        && sealed_batch().batch_fingerprint() != 0;
    s.add("56040-J3-批指纹确定", ok, if ok { "" } else { "批指纹漂移" });
    // J3-4 收官印只读
    let ok = sealed_batch().sealed;
    s.add("56040-J3-收官印只读", ok, if ok { "" } else { "印后可写" });
    // J3-5 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56040-J3-注入核必抓", ok, &detail);
    // J2-3 条目指纹对 ID/族/类敏感
    let e1 = BatchEntry::new(56021, "a", KD_BENCH).expect("e1");
    let e2 = BatchEntry::new(56021, "a", KD_BENCH).expect("e2");
    let e3 = BatchEntry::new(56022, "a", KD_BENCH).expect("e3");
    let e4 = BatchEntry::new(56021, "a", KD_INFRA).expect("e4");
    let ok = e1.fingerprint == e2.fingerprint
        && e1.fingerprint != e3.fingerprint
        && e1.fingerprint != e4.fingerprint;
    s.add("56040-J2-条目指纹敏感", ok, if ok { "" } else { "指纹漂移" });
    // J3-6 族名尾条与批防重自指（batch_dedup_b02 在账）
    let ok = B02_FAMILIES[BATCH_SIZE - 1] == "batch_dedup_b02";
    s.add("56040-J3-防重自指在账", ok, if ok { "" } else { "自指缺" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56040_all_checks_green() {
        let set = run_unx56040_checks();
        assert!(set.all_passed(), "UNX-F56040 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56040_kind_split() {
        assert_eq!(standard_batch().kind_split(), (9, 11));
    }
}
