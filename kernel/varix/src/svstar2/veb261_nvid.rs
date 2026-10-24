//! veb261_nvid —— VE-F0261 NVIDIA 设备识别与代际分型。
//! 锚点（CoRun Unxreal 主册）：VE-F0261 · NVIDIA 设备识别与代际分型 · 目标 400 行
//! 判据原文：VE-F0261-J1 PCI 设备 ID 表匹配（vendor 0x10DE +
//!           device id）；按 device id 分型到架构代际（Kepler/
//!           Maxwell/Pascal/Turing/Ampere/Ada）；未知 ID 归
//!           UNKNOWN 档（不猜测）；分型账（识别数 = 表内命中
//!           + 未知数）；代际序核（新代际号 > 旧代际号）。
//! 组件：设备表（静态 PCI ID → 代际）+ 匹配器 + 分型账。
//! 职责定位：设备识别是「NVIDIA 通路的第一道门」——认错代际
//!           就会发错命令流（Kepler 的推送提交和 Turing 的环
//!           形缓冲完全不兼容）；未知 ID 不猜测是工程诚实：宁
//!           可拒载也不把 Ampere 当 Turing 跑到花屏。
//! 性能逐项分解：匹配 O(表长)；分型账 O(n)。
//! 诊断码段：0x30A40（0x30A41~0x30A49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const NV_VERSION: &str = "veb261_nvid-v1";

/// 诊断段基址（0x30A40 段 · 本模块独占）。
pub const NV_SEG_BASE: u32 = 0x30A40;
pub const XC_BAD_VENDOR: u32 = 0x30A41; // vendor 非 0x10DE
pub const XC_UNKNOWN_ID: u32 = 0x30A42; // 未知 device id
pub const XC_MAGIC: u32 = 0x30A43; // 账魔数破
pub const XC_GEN_OOR: u32 = 0x30A44; // 代际越域
pub const XC_LEDGER: u32 = 0x30A45; // 分型账不平
pub const XC_TABLE: u32 = 0x30A46; // 表内重 ID
pub const XC_EMPTY: u32 = 0x30A47; // 空表查询

/// NVIDIA PCI vendor。
pub const VENDOR_NV: u32 = 0x10DE;

/// 架构代际（号越大越新）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Gen {
    /// 未知（不猜测）。
    Unknown = 0,
    /// Kepler。
    Kepler = 1,
    /// Maxwell。
    Maxwell = 2,
    /// Pascal。
    Pascal = 3,
    /// Turing。
    Turing = 4,
    /// Ampere。
    Ampere = 5,
    /// Ada。
    Ada = 6,
}

impl Gen {
    pub fn from_u32(v: u32) -> Option<Gen> {
        match v {
            0 => Some(Gen::Unknown),
            1 => Some(Gen::Kepler),
            2 => Some(Gen::Maxwell),
            3 => Some(Gen::Pascal),
            4 => Some(Gen::Turing),
            5 => Some(Gen::Ampere),
            6 => Some(Gen::Ada),
            _ => None,
        }
    }
}

/// 设备表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevRow {
    pub devid: u32,
    pub gen: Gen,
}

/// 示例设备表（代表性样本，非全集）。
pub const DEV_TABLE: &[DevRow] = &[
    DevRow { devid: 0x1180, gen: Gen::Kepler },   // GK104
    DevRow { devid: 0x1340, gen: Gen::Maxwell },  // GM108
    DevRow { devid: 0x1C02, gen: Gen::Pascal },   // GP106
    DevRow { devid: 0x1E04, gen: Gen::Turing },   // TU102
    DevRow { devid: 0x2184, gen: Gen::Turing },   // TU116
    DevRow { devid: 0x2204, gen: Gen::Ampere },   // GA102
    DevRow { devid: 0x2504, gen: Gen::Ampere },   // GA106
    DevRow { devid: 0x2484, gen: Gen::Ampere },   // GA104
    DevRow { devid: 0x2684, gen: Gen::Ada },      // AD102
    DevRow { devid: 0x2704, gen: Gen::Ada },      // AD103
];

/// 分型记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentRec {
    pub devid: u32,
    pub gen: Gen,
}

/// 识别器。
pub struct NvIdent {
    recs: Vec<IdentRec>,
    magic: u32,
}

impl NvIdent {
    /// 建。
    pub fn new() -> NvIdent {
        NvIdent { recs: Vec::new(), magic: 0x0A40_1DE1 }
    }

    /// 识别一次。
    pub fn identify(&mut self, vendor: u32, devid: u32) -> Result<Gen, u32> {
        if self.magic != 0x0A40_1DE1 {
            return Err(XC_MAGIC);
        }
        if vendor != VENDOR_NV {
            return Err(XC_BAD_VENDOR);
        }
        let mut gen = Gen::Unknown;
        for r in DEV_TABLE.iter() {
            if r.devid == devid {
                gen = r.gen;
            }
        }
        self.recs.push(IdentRec { devid, gen });
        if gen == Gen::Unknown {
            return Err(XC_UNKNOWN_ID);
        }
        Ok(gen)
    }

    /// 按表内重 ID 核（构造期自检）。
    pub fn table_audit() -> Result<u32, u32> {
        let mut n = 0u32;
        for i in 0..DEV_TABLE.len() {
            for j in (i + 1)..DEV_TABLE.len() {
                if DEV_TABLE[i].devid == DEV_TABLE[j].devid {
                    return Err(XC_TABLE);
                }
            }
            n += 1;
        }
        Ok(n)
    }

    /// 分型账核：命中数 + 未知数 = 总识别数；命中条目 gen 非 Unknown。
    pub fn audit(&self) -> Result<(u32, u32), u32> {
        if self.magic != 0x0A40_1DE1 {
            return Err(XC_MAGIC);
        }
        let mut hit = 0u32;
        let mut unk = 0u32;
        for r in self.recs.iter() {
            if r.gen == Gen::Unknown {
                unk += 1;
            } else {
                hit += 1;
            }
        }
        if hit + unk != self.recs.len() as u32 {
            return Err(XC_LEDGER);
        }
        Ok((hit, unk))
    }

    /// 识别数。
    pub fn count(&self) -> usize {
        self.recs.len()
    }

    /// 最近一次代际。
    pub fn last_gen(&self) -> Option<Gen> {
        self.recs.last().map(|r| r.gen)
    }
}

pub fn run_veb261_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0261");
    // 族一：设备表自检。
    let ta = NvIdent::table_audit();
    s.add("表无重 ID", ta == Ok(10), "10 条样本");
    // 代际序。
    s.add("代际单调", Gen::Kepler < Gen::Maxwell && Gen::Maxwell < Gen::Pascal
        && Gen::Pascal < Gen::Turing && Gen::Turing < Gen::Ampere && Gen::Ampere < Gen::Ada, "新旧有序");
    s.add("代际闭集", Gen::from_u32(6) == Some(Gen::Ada) && Gen::from_u32(7).is_none(), "0~6");
    // 族二：逐代际命中。
    let mut id = NvIdent::new();
    s.add("Kepler 命中", id.identify(VENDOR_NV, 0x1180) == Ok(Gen::Kepler), "GK104");
    s.add("Maxwell 命中", id.identify(VENDOR_NV, 0x1340) == Ok(Gen::Maxwell), "GM108");
    s.add("Pascal 命中", id.identify(VENDOR_NV, 0x1C02) == Ok(Gen::Pascal), "GP106");
    s.add("Turing 命中", id.identify(VENDOR_NV, 0x1E04) == Ok(Gen::Turing), "TU102");
    s.add("Ampere 命中", id.identify(VENDOR_NV, 0x2204) == Ok(Gen::Ampere), "GA102");
    s.add("Ada 命中", id.identify(VENDOR_NV, 0x2684) == Ok(Gen::Ada), "AD102");
    s.add("识别 6", id.count() == 6, "六笔账");
    s.add("最近 Ada", id.last_gen() == Some(Gen::Ada), "末笔");
    // 族三：未知不猜测。
    let mut u = NvIdent::new();
    s.add("未知 ID 拒", u.identify(VENDOR_NV, 0xFFFF) == Err(XC_UNKNOWN_ID), "黑盒");
    s.add("未知入账", u.count() == 1, "拒也记账");
    s.add("账核 0+1", u.audit() == Ok((0, 1)), "零命中一未知");
    // 族四：vendor 闸。
    s.add("异 vendor 拒", id.identify(0x8086, 0x1180) == Err(XC_BAD_VENDOR), "Intel 不是 NV");
    s.add("AMD 拒", id.identify(0x1002, 0x2204) == Err(XC_BAD_VENDOR), "AMD 不是 NV");
    // 族五：混合账。
    let (h, k) = match id.audit() {
        Ok(x) => x,
        Err(_) => (0, 0),
    };
    s.add("混合账 6+1", h == 6 && k == 1, "七笔");
    let mut m = NvIdent::new();
    let _ = m.identify(VENDOR_NV, 0x2184);
    let _ = m.identify(VENDOR_NV, 0x2504);
    s.add("同代多 ID", m.audit() == Ok((2, 0)), "Turing+GA106");
    s.add("魔数核", NV_SEG_BASE == 0x30A40, "段基址");
    s
}
