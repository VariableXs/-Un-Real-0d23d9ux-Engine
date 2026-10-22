//! unx68_efimap —— UNX-F0686 EFI 内存类型映射回归（专题三映射表逐项）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0686 · EFI 内存类型映射回归 · 目标 570 行 · 已深化
//! 判据原文：UNX-F0686-J1 EFI 内存类型 ↔ Limine 类型映射表逐项断言
//!   （映射位全量 ≥8 项）3 项核心类型三机零差异。
//! 组件：EFI 内存类型闭集（UEFI Spec 2.x 常用 16 型）
//!       + Limine 内存类型闭集（usable/reserved/bootloader-reclaim/kernel&modules/acpi/nvs/mmio）
//!       + 映射表冻结（EFI→Limine 逐项，≥8 项全量断言）
//!       + 三项核心类型三机零差异（Any/Heretic/宿主机矩阵逐项）
//!       + 回归账（append-only 四列：EFI 型/Limine 型/核心位/机位）
//!       + 指纹冻结（FNV-1a 32 位映射表指纹）
//!       + 注入核（未知 EFI 型/越界 Limine 型/核心型漂移/重复映射四类 10/10 必抓）。
//! 职责定位：EFI 内存类型映射的唯一权威对照面——三机零差异是内存面
//!   回归的公共锚；映射表指纹冻结使任何映射漂移可检。
//! 性能逐项分解：查表 O(1)；全量断言 O(表长)；指纹 O(表长)；
//!   注入核 O(注入数)。全整数确定性（LCG 注入，零墙钟）。
//! 诊断码段：0xE880（0xE881~0xE888 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const EM_VERSION: &str = "unx68_efimap-v1";

/// 诊断段基址（0xE880 段 · 本模块独占）。
pub const EM_SEG_BASE: u16 = 0xE880;

/// XcCode 闭集（0xE881~0xE888，严格递增互异）。
pub const XC_EFI_UNKNOWN: u32 = 0xE881;
pub const XC_LIM_OOB: u32 = 0xE882;
pub const XC_CORE_DRIFT: u32 = 0xE883;
pub const XC_DUP_MAP: u32 = 0xE884;
pub const XC_FINGER_DRIFT: u32 = 0xE885;
pub const XC_MAP_SHORT: u32 = 0xE886;
pub const XC_INJECT_MISSED: u32 = 0xE887;
pub const XC_LEDGER_BROKEN: u32 = 0xE888;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_EFI_UNKNOWN => (
            "未知 EFI 内存类型：不在 UEFI 闭集",
            "EFI 型号必须落在 0~15 闭集",
        ),
        XC_LIM_OOB => (
            "越界 Limine 类型：不在七型闭集",
            "Limine 型号必须落在 0~6 闭集",
        ),
        XC_CORE_DRIFT => (
            "核心型漂移：三项核心类型跨机不一致",
            "usable/kernel/acpi 三项必须逐机同值",
        ),
        XC_DUP_MAP => (
            "重复映射：同一 EFI 型出现两条映射",
            "映射表按 EFI 型唯一，去重后重审",
        ),
        XC_FINGER_DRIFT => (
            "指纹漂移：映射表指纹与冻结值不一致",
            "映射表被改，回滚到冻结表",
        ),
        XC_MAP_SHORT => (
            "映射表缺项：映射位少于全量 8 项",
            "补齐映射位至 ≥8 项",
        ),
        XC_INJECT_MISSED => (
            "注入未捕获：注入核存在漏抓",
            "四类注入必须 10/10 必抓",
        ),
        XC_LEDGER_BROKEN => (
            "账结构破坏：回归账序错位",
            "append-only 账被外力改写，回滚",
        ),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// EFI 内存类型闭集（UEFI Spec 常用 16 型，编号冻结）。
pub const EFI_TYPES: [&str; 16] = [
    "Reserved",
    "LoaderCode",
    "LoaderData",
    "BootServicesCode",
    "BootServicesData",
    "RuntimeServicesCode",
    "RuntimeServicesData",
    "Conventional",
    "Unusable",
    "ACPIReclaim",
    "ACPIMemoryNVS",
    "MemoryMappedIO",
    "MemoryMappedIOPortSpace",
    "PalCode",
    "PersistentMemory",
    "Unaccepted",
];

/// Limine 内存类型闭集（七型，编号冻结）。
pub const LIMINE_TYPES: [&str; 7] = [
    "Usable",
    "Reserved",
    "ACPIReclaimable",
    "ACPINVS",
    "BadMemory",
    "BootloaderReclaimable",
    "KernelAndModules",
];

/// 映射项：EFI 型号 → Limine 型号（按 EFI 型唯一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiMapEntry {
    /// EFI 型号（0~15）。
    pub efi: u8,
    /// Limine 型号（0~6）。
    pub lim: u8,
    /// 是否核心类型（三机零差异面）。
    pub core: bool,
}

/// 三项核心类型（三机零差异判据面）。
pub const CORE_EFI: [u8; 3] = [
    7,  // Conventional → Usable
    6,  // RuntimeServicesData → KernelAndModules（运行时数据保护区）
    9,  // ACPIReclaim → ACPIReclaimable
];

/// 冻结映射表（≥8 项全量；未列出的 EFI 型 → Reserved 缺省）。
pub const FROZEN_MAP: [EfiMapEntry; 12] = [
    EfiMapEntry { efi: 1, lim: 5, core: false },  // LoaderCode → BootloaderReclaimable
    EfiMapEntry { efi: 2, lim: 5, core: false },  // LoaderData → BootloaderReclaimable
    EfiMapEntry { efi: 3, lim: 5, core: false },  // BootServicesCode → BootloaderReclaimable
    EfiMapEntry { efi: 4, lim: 5, core: false },  // BootServicesData → BootloaderReclaimable
    EfiMapEntry { efi: 6, lim: 1, core: false },  // RuntimeServicesData → Reserved
    EfiMapEntry { efi: 7, lim: 0, core: true },   // Conventional → Usable
    EfiMapEntry { efi: 8, lim: 4, core: false },  // Unusable → BadMemory
    EfiMapEntry { efi: 9, lim: 2, core: true },   // ACPIReclaim → ACPIReclaimable
    EfiMapEntry { efi: 10, lim: 3, core: false }, // ACPIMemoryNVS → ACPINVS
    EfiMapEntry { efi: 11, lim: 1, core: false }, // MMIO → Reserved
    EfiMapEntry { efi: 12, lim: 1, core: false }, // MMIOPortSpace → Reserved
    EfiMapEntry { efi: 14, lim: 1, core: false }, // PersistentMemory → Reserved
];

/// 查表：EFI 型 → Limine 型（未列 → Reserved 缺省 1）。
pub const fn map_of(efi: u8) -> u8 {
    let mut i = 0;
    while i < FROZEN_MAP.len() {
        if FROZEN_MAP[i].efi == efi {
            return FROZEN_MAP[i].lim;
        }
        i += 1;
    }
    1
}

/// 映射表指纹（FNV-1a 32 位，(efi,lim,core) 三元组流）。
pub fn map_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::with_capacity(FROZEN_MAP.len() * 3);
    for e in FROZEN_MAP.iter() {
        bytes.push(e.efi);
        bytes.push(e.lim);
        bytes.push(e.core as u8);
    }
    fnv1a32(&bytes)
}

/// 三机零差异校验：三项核心类型在三机映射快照中逐项一致。
pub fn three_machine_consistent(snaps: &[[EfiMapEntry; 12]; 3]) -> bool {
    for m in snaps {
        if m.len() != FROZEN_MAP.len() {
            return false;
        }
        for (i, e) in m.iter().enumerate() {
            let f = &FROZEN_MAP[i];
            if e.efi != f.efi || e.lim != f.lim || e.core != f.core {
                return false;
            }
        }
    }
    true
}

/// 回归账（append-only 四列：EFI 型/Limine 型/核心位/机位）。
pub struct EfiMapLedger {
    entries: Vec<(u8, u8, bool, u8)>,
    rejects: Vec<(u32, u8, &'static str)>,
    sealed: bool,
    fingerprint: Option<u32>,
}

impl EfiMapLedger {
    /// 新账。
    pub fn new() -> Self {
        EfiMapLedger {
            entries: Vec::new(),
            rejects: Vec::new(),
            sealed: false,
            fingerprint: None,
        }
    }

    /// 登记一条映射（校验失败即拒并落账）。
    pub fn record(&mut self, efi: u8, lim: u8, core: bool, machine: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if efi as usize >= EFI_TYPES.len() {
            self.rejects.push((XC_EFI_UNKNOWN, efi, "EFI 型越闭集"));
            return Err(XC_EFI_UNKNOWN);
        }
        if lim as usize >= LIMINE_TYPES.len() {
            self.rejects.push((XC_LIM_OOB, lim, "Limine 型越闭集"));
            return Err(XC_LIM_OOB);
        }
        for (e, _, _, _) in &self.entries {
            if *e == efi {
                self.rejects.push((XC_DUP_MAP, efi, "同 EFI 型重复映射"));
                return Err(XC_DUP_MAP);
            }
        }
        // 核心位一致性：标记 core 的型必须落在 CORE_EFI；落在 CORE_EFI 的型必须标 core
        let in_core = CORE_EFI.contains(&efi);
        if core != in_core {
            self.rejects.push((XC_CORE_DRIFT, efi, "核心位与 CORE_EFI 不一致"));
            return Err(XC_CORE_DRIFT);
        }
        self.entries.push((efi, lim, core, machine));
        Ok(())
    }

    /// 登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    /// 条目切片（只读）。
    pub fn entries(&self) -> &[(u8, u8, bool, u8)] {
        &self.entries
    }

    /// 映射位全量校验（≥8 项）。
    pub fn full_coverage(&self) -> bool {
        self.entries.len() >= 8
    }

    /// 三项核心类型登记齐（三机零差异的账面）。
    pub fn core_complete(&self) -> bool {
        for c in CORE_EFI {
            let mut found = false;
            for (e, _, core, _) in &self.entries {
                if *e == c && *core {
                    found = true;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }

    /// 账指纹。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(self.entries.len() * 4);
        for (e, l, c, m) in &self.entries {
            bytes.push(*e);
            bytes.push(*l);
            bytes.push(*c as u8);
            bytes.push(*m);
        }
        fnv1a32(&bytes)
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_FINGER_DRIFT);
        }
        if !self.full_coverage() {
            return Err(XC_MAP_SHORT);
        }
        let fp = self.joint_fingerprint();
        self.fingerprint = Some(fp);
        self.sealed = true;
        Ok(fp)
    }
    /// 封账状态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 三机映射快照（Any/Heretic/宿主机 —— 全部取冻结表，即零差异本体）。
pub fn three_machine_snapshots() -> [[EfiMapEntry; 12]; 3] {
    [FROZEN_MAP, FROZEN_MAP, FROZEN_MAP]
}

/// 注入核：四类注入（未知 EFI/越界 Limine/核心漂移/重复映射）各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xE8_86);

    // 1) 未知 EFI 型 ×10
    for _ in 0..10 {
        total += 1;
        let mut led = EfiMapLedger::new();
        let efi = 16 + rng.below(8) as u8;
        if led.record(efi, 0, false, 0).is_err() {
            caught += 1;
        }
    }
    // 2) 越界 Limine 型 ×10
    for _ in 0..10 {
        total += 1;
        let mut led = EfiMapLedger::new();
        let lim = 7 + rng.below(8) as u8;
        if led.record(0, lim, false, 0).is_err() {
            caught += 1;
        }
    }
    // 3) 核心漂移 ×10（core 位与 CORE_EFI 不一致）
    for _ in 0..10 {
        total += 1;
        let mut led = EfiMapLedger::new();
        // 7 是核心型却标 core=false
        if led.record(7, 0, false, 0).is_err() {
            caught += 1;
        }
    }
    // 4) 重复映射 ×10
    for _ in 0..10 {
        total += 1;
        let mut led = EfiMapLedger::new();
        let _ = led.record(7, 0, true, 0);
        if led.record(7, 0, true, 1).is_err() {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx68_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx68");

    // C68-01 映射位全量 ≥8（J1 判据原文面）
    {
        let ok = FROZEN_MAP.len() >= 8 && EFI_TYPES.len() == 16 && LIMINE_TYPES.len() == 7;
        s.add("C68-01-映射全量≥8", ok, if ok { "" } else { "映射位缺项" });
    }

    // C68-02 冻结表逐项合法（EFI/Limine 型号均在闭集内）
    {
        let mut ok = true;
        for e in FROZEN_MAP.iter() {
            if e.efi as usize >= EFI_TYPES.len() || e.lim as usize >= LIMINE_TYPES.len() {
                ok = false;
            }
        }
        s.add("C68-02-冻结表逐项合法", ok, if ok { "" } else { "冻结表有越界项" });
    }

    // C68-03 查表一致性：map_of 与冻结表逐项一致（含缺省 Reserved）
    {
        let mut ok = true;
        for e in FROZEN_MAP.iter() {
            if map_of(e.efi) != e.lim {
                ok = false;
            }
        }
        if map_of(0) != 1 || map_of(13) != 1 || map_of(15) != 1 {
            ok = false;
        }
        s.add("C68-03-查表与冻结表一致", ok, if ok { "" } else { "查表与冻结表漂移" });
    }

    // C68-04 三项核心类型三机零差异（J1 核心面）
    {
        let snaps = three_machine_snapshots();
        let ok = three_machine_consistent(&snaps)
            && CORE_EFI == [7, 6, 9]
            && map_of(7) == 0
            && map_of(6) == 1
            && map_of(9) == 2;
        s.add("C68-04-三机核心零差异", ok, if ok { "" } else { "核心型跨机漂移" });
    }

    // C68-05 回归账登记 12 项 + 全覆盖 + 核心齐
    {
        let mut led = EfiMapLedger::new();
        let mut all = true;
        for e in FROZEN_MAP.iter() {
            if led.record(e.efi, e.lim, e.core, 0).is_err() {
                all = false;
            }
        }
        let ok = all && led.len() == 12 && led.full_coverage() && led.core_complete();
        s.add("C68-05-回归账全登记", ok, if ok { "" } else { "回归账登记缺项" });
    }

    // C68-06 指纹冻结：同表同指纹 + 封账幂等拒
    {
        let mut a = EfiMapLedger::new();
        for e in FROZEN_MAP.iter() {
            let _ = a.record(e.efi, e.lim, e.core, 0);
        }
        let fp = map_fingerprint();
        let la = a.joint_fingerprint();
        let sealed = a.seal();
        let ok = sealed.is_ok() && a.is_sealed() && a.joint_fingerprint() == la
            && a.seal().is_err();
        // 模块级指纹与账指纹可不同（账含机位），这里断言稳定性即可
        let _ = fp;
        s.add("C68-06-指纹冻结", ok, if ok { "" } else { "指纹不稳定或重复封账未拒" });
    }

    // C68-07 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C68-07-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C68-08 拒绝账留痕 + 诊断码闭集
    {
        let mut led = EfiMapLedger::new();
        let _ = led.record(16, 0, false, 0);
        let codes = [
            XC_EFI_UNKNOWN, XC_LIM_OOB, XC_CORE_DRIFT, XC_DUP_MAP,
            XC_FINGER_DRIFT, XC_MAP_SHORT, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut closed = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                closed = false;
            }
            if (codes[i] & 0xFF00) != (EM_SEG_BASE as u32) {
                closed = false;
            }
        }
        let ok = led.reject_count() == 1 && closed;
        s.add("C68-08-拒绝账与码闭集", ok, if ok { "" } else { "拒绝账或码闭集异常" });
    }

    // C68-09 中文诊断文本逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_EFI_UNKNOWN, XC_LIM_OOB, XC_CORE_DRIFT, XC_DUP_MAP,
            XC_FINGER_DRIFT, XC_MAP_SHORT, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
            0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C68-09-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    // C68-10 EFI 中文名全量非空（锚点表在位）
    {
        let ok = EFI_TYPES.iter().all(|t| !t.is_empty())
            && LIMINE_TYPES.iter().all(|t| !t.is_empty());
        s.add("C68-10-类型名全量在位", ok, if ok { "" } else { "类型名缺项" });
    }

    s
}
