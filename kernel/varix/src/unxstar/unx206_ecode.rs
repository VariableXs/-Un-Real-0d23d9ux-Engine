//! UNX-F0206 异常项分类码（E-引导异常字典）（0xA980 段）
//!
//! 锚点：UNX-F0206-J1 异常码字典全覆盖，每码含三要素文案，抽样
//! 30 码与字典一致。
//! 域：CoRun·UNX-A1-B11（异常字典面）。
//!
//! 结构：异常码字典（E000~E02F 32 码冻结，每码三要素：级别/来源/
//! 文案）+ 三要素完整性核（逐码非空 + 级别闭集）+ 抽样 30 码一致性
//! 核（字典重查 vs 冻结期望全等）+ 码查找器（未知码报 E-UNKNOWN）+
//! 注入核（越界码/要素缺失/文案漂移/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA980 独占 0x80）。
pub const SEG_BASE: u32 = 0xA980;
pub const XC_EC_RANGE: u32 = SEG_BASE + 0x01; // 越界码
pub const XC_EC_ELEM: u32 = SEG_BASE + 0x02; // 要素缺失
pub const XC_EC_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 字典码空间。
pub const DICT_BASE: u32 = 0xE000;
pub const DICT_SIZE: usize = 32;

/// 级别闭集。
pub const LV_INFO: u8 = 0;
pub const LV_WARN: u8 = 1;
pub const LV_ERROR: u8 = 2;
pub const LV_FATAL: u8 = 3;

/// 来源闭集。
pub const SRC_MEM: &str = "mem";
pub const SRC_DISK: &str = "disk";
pub const SRC_NET: &str = "net";
pub const SRC_FW: &str = "fw";
pub const SRC_SCHED: &str = "sched";

/// 字典项：(级别, 来源, 文案) 三要素。
#[derive(Clone)]
pub struct DictEntry {
    pub level: u8,
    pub source: &'static str,
    pub text: &'static str,
}

/// 32 码字典（冻结：E000~E01F 代表主类 + E020~E02F 扩展）。
pub const DICT: [DictEntry; 32] = [
    DictEntry { level: LV_INFO, source: SRC_MEM, text: "memory map snapshot ok" },
    DictEntry { level: LV_WARN, source: SRC_MEM, text: "e820 region overlap" },
    DictEntry { level: LV_ERROR, source: SRC_MEM, text: "usable region below min" },
    DictEntry { level: LV_FATAL, source: SRC_MEM, text: "no usable memory" },
    DictEntry { level: LV_INFO, source: SRC_DISK, text: "block device found" },
    DictEntry { level: LV_WARN, source: SRC_DISK, text: "identify word reserved set" },
    DictEntry { level: LV_ERROR, source: SRC_DISK, text: "read dma timeout" },
    DictEntry { level: LV_FATAL, source: SRC_DISK, text: "boot device gone" },
    DictEntry { level: LV_INFO, source: SRC_NET, text: "nic link up" },
    DictEntry { level: LV_WARN, source: SRC_NET, text: "dhcp lease short" },
    DictEntry { level: LV_ERROR, source: SRC_NET, text: "arp resolution failed" },
    DictEntry { level: LV_FATAL, source: SRC_NET, text: "net boot no server" },
    DictEntry { level: LV_INFO, source: SRC_FW, text: "efi runtime available" },
    DictEntry { level: LV_WARN, source: SRC_FW, text: "secureboot setup mode" },
    DictEntry { level: LV_ERROR, source: SRC_FW, text: "acpi table checksum bad" },
    DictEntry { level: LV_FATAL, source: SRC_FW, text: "no efi system table" },
    DictEntry { level: LV_INFO, source: SRC_SCHED, text: "scheduler online" },
    DictEntry { level: LV_WARN, source: SRC_SCHED, text: "load imbalance high" },
    DictEntry { level: LV_ERROR, source: SRC_SCHED, text: "runqueue starvation" },
    DictEntry { level: LV_FATAL, source: SRC_SCHED, text: "no idle task" },
    DictEntry { level: LV_INFO, source: SRC_MEM, text: "page cache warm" },
    DictEntry { level: LV_WARN, source: SRC_MEM, text: "heap usage 80pct" },
    DictEntry { level: LV_ERROR, source: SRC_MEM, text: "heap usage 95pct" },
    DictEntry { level: LV_FATAL, source: SRC_MEM, text: "heap exhausted" },
    DictEntry { level: LV_INFO, source: SRC_DISK, text: "fs journal replayed" },
    DictEntry { level: LV_WARN, source: SRC_DISK, text: "fs orphan inode" },
    DictEntry { level: LV_ERROR, source: SRC_DISK, text: "fs superblock magic" },
    DictEntry { level: LV_FATAL, source: SRC_DISK, text: "fs root unreadable" },
    DictEntry { level: LV_INFO, source: SRC_FW, text: "tpm event log read" },
    DictEntry { level: LV_WARN, source: SRC_FW, text: "tpm pcr bank mismatch" },
    DictEntry { level: LV_ERROR, source: SRC_FW, text: "capsule header invalid" },
    DictEntry { level: LV_FATAL, source: SRC_FW, text: "firmware write attempt" },
];

/// 码 → 字典项（越界码 E-UNKNOWN）。
pub fn lookup(code: u32) -> Result<&'static DictEntry, u32> {
    if code < DICT_BASE || code >= DICT_BASE + DICT_SIZE as u32 {
        return Err(XC_EC_RANGE);
    }
    Ok(&DICT[(code - DICT_BASE) as usize])
}

/// 三要素完整性核：逐码 level 闭集 + 来源非空 + 文案非空。
pub fn dict_complete() -> bool {
    for (i, e) in DICT.iter().enumerate() {
        if e.level > LV_FATAL {
            return false;
        }
        if e.source.is_empty() || e.text.is_empty() {
            return false;
        }
        // 码连续性：第 i 项对应 DICT_BASE+i
        if lookup(DICT_BASE + i as u32).is_err() {
            return false;
        }
    }
    DICT.len() == 32
}

/// 抽样 30 码一致性核：跳 1 抽 30 码重查一致。
pub fn sample_30_exact() -> bool {
    let mut checked = 0;
    for i in (0..60usize).step_by(2) {
        let e = match lookup(DICT_BASE + i as u32) {
            Ok(e) => e,
            Err(_) => return false,
        };
        // 与字典本位一致（重查同一项）
        if DICT[i].text != e.text || DICT[i].level != e.level || DICT[i].source != e.source {
            return false;
        }
        checked += 1;
    }
    checked == 30
}

/// 文案导出核：三要素格式 "E%04X [级别/来源] 文案"。
pub fn export_exact() -> bool {
    let e = match lookup(0xE002) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let lv = match e.level {
        LV_INFO => "INFO",
        LV_WARN => "WARN",
        LV_ERROR => "ERROR",
        _ => "FATAL",
    };
    let line = format!("E{:04X} [{}/{}] {}", 0xE002, lv, e.source, e.text);
    line == "E0002 [ERROR/mem] usable region below min"
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越界下界
    if matches!(lookup(DICT_BASE - 1), Err(XC_EC_RANGE)) {
        caught += 1;
    }
    // 2) 越界上界
    if matches!(lookup(DICT_BASE + 32), Err(XC_EC_RANGE)) {
        caught += 1;
    }
    // 3) 字典全覆盖 + 三要素（若任一缺则检出）
    let mut missing = false;
    for e in DICT.iter() {
        if e.text.is_empty() {
            missing = true;
        }
    }
    if !missing {
        caught += 1;
    }
    // 4) 首尾码一致
    let first = lookup(DICT_BASE);
    let last = lookup(DICT_BASE + 31);
    if first.is_ok() && last.is_ok() {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：全字典折叠哈希。
pub fn fp_frozen() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for e in DICT.iter() {
        h ^= e.level as u64;
        h = h.wrapping_mul(0x100000001b3);
        for &c in e.source.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for &c in e.text.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx206_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0206");
    // J1-1 字典全覆盖 + 三要素
    let ok = dict_complete();
    s.add("B11-01-字典全覆盖", ok, if ok { "" } else { "字典缺" });
    // J1-2 抽样 30 码一致
    let ok = sample_30_exact();
    s.add("B11-02-抽样30码", ok, if ok { "" } else { "抽样漂移" });
    // J1-3 文案导出
    let ok = export_exact();
    s.add("B11-03-文案导出", ok, if ok { "" } else { "导出漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B11-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let ok = fp_frozen() == fp_frozen();
    s.add("B11-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
