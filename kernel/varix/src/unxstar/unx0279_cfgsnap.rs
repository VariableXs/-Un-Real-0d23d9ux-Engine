//! unx0279_cfgsnap —— UNX-F0279 控制台配置快照（恢复出厂）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0279 · 配置快照 · 目标 220 行
//! 判据原文：UNX-F0279-J1 控制台配置可快照、可恢复出厂：快照与
//!           当前配置 diff 逐项出账；恢复出厂 = 载入厂默认快照
//!           （默认快照冻结不可变）；恢复后与厂默认逐项零漂移。
//! 组件：配置项（键 + 值）+ 配置集（键唯一 + 快照冻结）+ diff 核
//!       （同键异值/单侧有）+ 出厂恢复（厂默认冻结载入 + 零漂移闸）。
//! 职责定位：配置快照是「控制台的存档点」——出厂默认是冻结基线
//!   （谁都不许改），恢复就是把当前配置整体置换回基线；置换后
//!   零漂移是恢复成功的唯一判据。diff 让「改了什么」先可见再动手。
//! 性能逐项分解：快照 O(项数)；diff O(N×M)；恢复 O(项数)。
//! 诊断码段：0x20500（0x20501~0x20509 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CF_VERSION: &str = "unx0279_cfgsnap-v1";

/// 诊断段基址（0x20500 段 · 本模块独占）。
pub const CF_SEG_BASE: u32 = 0x20500;
pub const XC_KEY_DUP: u32 = 0x20501; // 键重复
pub const XC_KEY_EMPTY: u32 = 0x20502; // 空键拒
pub const XC_FROZEN: u32 = 0x20503; // 冻结集拒改
pub const XC_EMPTY_CFG: u32 = 0x20504; // 空配置拒
pub const XC_RESTORE_DRIFT: u32 = 0x20505; // 恢复后漂移
pub const XC_DIFF_EMPTY: u32 = 0x20506; // 无差可出
pub const XC_KEY_OOR: u32 = 0x20507; // 键越域
pub const XC_SNAP_SIG: u32 = 0x20508; // 快照签漂
pub const XC_VAL_OOR: u32 = 0x20509; // 值越域

/// 配置键闭集 1~32。
pub const KEY_MAX: u32 = 32;

/// 配置项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CfgItem {
    pub key: u32,
    pub val: u32,
}

impl CfgItem {
    pub fn new(key: u32, val: u32) -> Result<CfgItem, u32> {
        if key == 0 || key > KEY_MAX {
            return Err(XC_KEY_OOR);
        }
        if val > 0xFFFF {
            return Err(XC_VAL_OOR);
        }
        Ok(CfgItem { key, val })
    }
}

impl CfgItem {
    /// 项自检（键/值域核；CfgSet 侧复用）。
    pub fn valid(&self) -> Result<(), u32> {
        if self.key == 0 || self.key > KEY_MAX {
            return Err(XC_KEY_OOR);
        }
        if self.val > 0xFFFF {
            return Err(XC_VAL_OOR);
        }
        Ok(())
    }
}

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 配置集（键唯一；可冻结）。
pub struct CfgSet {
    items: Vec<CfgItem>,
    frozen: bool,
    sig: u64,
}

impl CfgSet {
    pub fn new(items: &[CfgItem]) -> Result<CfgSet, u32> {
        if items.is_empty() {
            return Err(XC_EMPTY_CFG);
        }
        for it in items.iter() {
            it.valid()?;
            if items.iter().filter(|x| x.key == it.key).count() > 1 {
                return Err(XC_KEY_DUP);
            }
        }
        let sig = cfg_sig(items);
        Ok(CfgSet { items: items.to_vec(), frozen: false, sig })
    }

    fn valid(&self) -> Result<(), u32> {
        for it in self.items.iter() {
            it.valid()?;
        }
        Ok(())
    }

    /// 冻结（出厂默认一次性冻结，不可解冻）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 内容签。
    pub fn sig(&self) -> u64 {
        self.sig
    }

    /// 设值：冻结拒；键不存在则新增（键唯一仍核）。
    pub fn set(&mut self, key: u32, val: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(XC_FROZEN);
        }
        let item = CfgItem::new(key, val)?;
        if let Some(slot) = self.items.iter_mut().find(|x| x.key == key) {
            slot.val = val;
        } else {
            self.items.push(item);
        }
        self.sig = cfg_sig(&self.items);
        Ok(())
    }

    /// 查值。
    pub fn get(&self, key: u32) -> Option<u32> {
        self.items.iter().find(|x| x.key == key).map(|x| x.val)
    }

    /// 项数。
    pub fn len(&self) -> u32 {
        self.items.len() as u32
    }

    /// diff：返回漂移项清单（同键异值 / 单侧有）。
    pub fn diff(&self, other: &CfgSet) -> Result<Vec<(u32, u32, u32)>, u32> {
        // (key, self 值, other 值)；单侧缺省用 0xFFFFFFFF 标记。
        const MISSING: u32 = 0xFFFF_FFFF;
        let mut out = Vec::new();
        for it in self.items.iter() {
            match other.get(it.key) {
                Some(v) if v != it.val => out.push((it.key, it.val, v)),
                Some(_) => {}
                None => out.push((it.key, it.val, MISSING)),
            }
        }
        for it in other.items.iter() {
            if self.get(it.key).is_none() {
                out.push((it.key, MISSING, it.val));
            }
        }
        if out.is_empty() {
            return Err(XC_DIFF_EMPTY);
        }
        Ok(out)
    }

    /// 恢复出厂：整体置换为厂默认（冻结基线），恢复后零漂移。
    pub fn restore_factory(&mut self, factory: &CfgSet) -> Result<(), u32> {
        if !factory.frozen {
            return Err(XC_FROZEN); // 厂默认必须冻结态才可作基线
        }
        self.items = factory.items.clone();
        self.sig = factory.sig;
        // 零漂移闸：恢复后 diff 必须空。
        if self.diff(factory).is_ok() {
            return Err(XC_RESTORE_DRIFT);
        }
        Ok(())
    }
}

/// 配置签名：键序排定后 FNV(key,val 交替)。
pub fn cfg_sig(items: &[CfgItem]) -> u64 {
    let mut sorted: Vec<&CfgItem> = items.iter().collect();
    sorted.sort_unstable_by_key(|x| x.key);
    let mut bytes = Vec::new();
    for it in sorted.iter() {
        bytes.extend_from_slice(&it.key.to_le_bytes());
        bytes.extend_from_slice(&it.val.to_le_bytes());
    }
    fnv64(&bytes)
}

pub fn run_unx0279_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0279_cfgsnap");
    // 族一：配置集构造。
    let base = vec![
        CfgItem::new(1, 9600).unwrap_or(CfgItem { key: 1, val: 9600 }),
        CfgItem { key: 2, val: 115200 },
        CfgItem { key: 3, val: 80 },
    ];
    let mut cfg = match CfgSet::new(&base) {
        Ok(c) => c,
        Err(_) => {
            cs.add("配置集构造", false, "被拒");
            return cs;
        }
    };
    cs.add("项数", cfg.len() == 3, "三键");
    cs.add("查值", cfg.get(2) == Some(115200), "波特率");
    // 族二：改值与签动。
    let sig0 = cfg.sig();
    let _ = cfg.set(3, 132);
    cs.add("改值生效", cfg.get(3) == Some(132), "列宽 132");
    cs.add("签随动", cfg.sig() != sig0, "内容变签变");
    let _ = cfg.set(3, 80);
    cs.add("改回签复原", cfg.sig() == sig0, "签是内容函数");
    // 族三：diff。
    let mut other = CfgSet::new(&base).unwrap_or(CfgSet { items: base.clone(), frozen: false, sig: 0 });
    let _ = other.set(1, 115200);
    let _ = other.set(9, 7);
    let d = cfg.diff(&other);
    cs.add("diff 三项", matches!(&d, Ok(v) if v.len() == 3), "异值 1 + 多键 9");
    cs.add("同配置无差", cfg.diff(&cfg).is_err(), "DIFF_EMPTY");
    // 族四：出厂冻结与恢复。
    let mut factory = CfgSet::new(&base).unwrap_or(CfgSet { items: base.clone(), frozen: false, sig: 0 });
    factory.freeze();
    cs.add("厂默认冻结", factory.frozen(), "一次性");
    let _ = cfg.set(2, 1);
    let r = cfg.restore_factory(&factory);
    cs.add("恢复出厂", r.is_ok(), "置换回基线");
    cs.add("零漂移", cfg.sig() == factory.sig(), "签与厂默认同");
    cs.add("恢复后查值", cfg.get(2) == Some(115200), "键 2 复位");
    // 族五：未冻结基线拒。
    let mut cfg2 = CfgSet::new(&base).unwrap_or(CfgSet { items: base.clone(), frozen: false, sig: 0 });
    let _ = cfg2.set(1, 5);
    let unfrozen = CfgSet::new(&base).unwrap_or(CfgSet { items: base.clone(), frozen: false, sig: 0 });
    cs.add("未冻结基线拒", cfg2.restore_factory(&unfrozen).is_err(), "FROZEN");
    // 族六：构造拒。
    cs.add("空配置拒", CfgSet::new(&[]).is_err(), "EMPTY");
    cs.add("键重拒", CfgSet::new(&[
        CfgItem { key: 1, val: 1 },
        CfgItem { key: 1, val: 2 },
    ]).is_err(), "KEY_DUP");
    cs.add("键越域拒", CfgItem::new(33, 1).is_err(), "KEY_OOR");
    cs.add("值越域拒", CfgItem::new(1, 0x1_0000).is_err(), "VAL_OOR");
    cs.add("冻结后设值拒", {
        let mut f2 = CfgSet::new(&base).unwrap_or(CfgSet { items: base.clone(), frozen: false, sig: 0 });
        f2.freeze();
        f2.set(1, 7).is_err()
    }, "FROZEN");
    cs
}
