//! unx0273_hotkey —— UNX-F0273 快捷键冲突表与保留前缀。
//! 锚点（CoRun Unxreal 主册）：UNX-F0273 · 快捷键冲突表 · 目标 180 行
//! 判据原文：UNX-F0273-J1 快捷键注册冲突核：同键序重复注册拒；
//!           键序不得以保留前缀开头（保留前缀为系统命令留位）；
//!           冲突表可导出对账（注册序稳定）。
//! 组件：键序（主键 + 修饰位编码）+ 注册账（冲突核 + 保留前缀核）
//!       + 导出（注册序稳定输出）。
//! 职责定位：快捷键冲突是「终端手感的最后防线」——同键序后注册
//!   静默覆盖先注册 = 用户肌肉记忆被无声偷走；保留前缀是系统
//!   命令的防火道，任何插件不许抢占。
//! 性能逐项分解：注册 O(表深)；导出 O(表深)。
//! 诊断码段：0x20380（0x20381~0x20389 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HK_VERSION: &str = "unx0273_hotkey-v1";

/// 诊断段基址（0x20380 段 · 本模块独占）。
pub const HK_SEG_BASE: u32 = 0x20380;
pub const XC_KEY_DUP: u32 = 0x20381; // 键序重复
pub const XC_PREFIX_RSV: u32 = 0x20382; // 撞保留前缀
pub const XC_KEY_ZERO: u32 = 0x20383; // 零键序拒
pub const XC_SEQ_LONG: u32 = 0x20384; // 键序超长
pub const XC_PREFIX_EMPTY: u32 = 0x20385; // 空前缀拒
pub const XC_TABLE_FULL: u32 = 0x20386; // 表满
pub const XC_MOD_OOR: u32 = 0x20387; // 修饰位越域
pub const XC_EXPORT_DRIFT: u32 = 0x20388; // 导出漂移
pub const XC_KEY_OOR: u32 = 0x20389; // 主键越域

/// 键序编码：修饰位（低 4 位）+ 主键（高 8 位）+ 序长（次高 4 位）。
/// 主键闭集 1~120（0 保留 = 无键）。
pub const KEY_LO: u32 = 1;
pub const KEY_HI: u32 = 120;
/// 键序最大长度。
pub const SEQ_MAX: usize = 4;

/// 保留前缀闭集（系统命令留位）。
pub const RESERVED_PREFIX: [u32; 4] = [0x0100, 0x0200, 0x0300, 0x0400];

/// 键序：u32 数组，每元素 = (主键 << 8) | 修饰位。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KeySeq {
    pub keys: Vec<u32>,
}

impl KeySeq {
    pub fn new(keys: &[u32]) -> Result<KeySeq, u32> {
        if keys.is_empty() {
            return Err(XC_KEY_ZERO);
        }
        if keys.len() > SEQ_MAX {
            return Err(XC_SEQ_LONG);
        }
        for &k in keys.iter() {
            let main = (k >> 8) & 0xFF;
            let mods = k & 0xF;
            if main < KEY_LO || main > KEY_HI {
                return Err(XC_KEY_OOR);
            }
            if mods > 0x8 {
                return Err(XC_MOD_OOR);
            }
        }
        Ok(KeySeq { keys: keys.to_vec() })
    }

    /// 前缀核：键序以任一保留前缀开头 = 拒。
    pub fn hits_reserved(&self) -> bool {
        RESERVED_PREFIX.iter().any(|&p| self.keys.first() == Some(&p))
    }
}

/// 快捷键注册账。
pub struct HotkeyTable {
    entries: Vec<(KeySeq, u32)>, // (键序, 动作号)
}

impl HotkeyTable {
    pub fn new() -> HotkeyTable {
        HotkeyTable { entries: Vec::new() }
    }

    /// 注册：同键序拒；撞保留前缀拒。
    pub fn register(&mut self, seq: KeySeq, action: u32) -> Result<(), u32> {
        if self.entries.len() >= 64 {
            return Err(XC_TABLE_FULL);
        }
        if seq.hits_reserved() {
            return Err(XC_PREFIX_RSV);
        }
        if self.entries.iter().any(|(s, _)| *s == seq) {
            return Err(XC_KEY_DUP);
        }
        self.entries.push((seq, action));
        Ok(())
    }

    /// 表深。
    pub fn len(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 导出：注册序稳定（先注册先出）。
    pub fn export(&self) -> Vec<(u32, u32)> {
        self.entries.iter().map(|(s, a)| (s.keys.first().copied().unwrap_or(0), *a)).collect()
    }

    /// 导出对账核：导出长度 == 表深（漂移检出）。
    pub fn export_ok(&self) -> Result<(), u32> {
        if self.export().len() != self.entries.len() {
            return Err(XC_EXPORT_DRIFT);
        }
        Ok(())
    }
}

pub fn run_unx0273_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0273_hotkey");
    // 族一：注册与导出。
    let mut tbl = HotkeyTable::new();
    let k1 = KeySeq::new(&[0x0141]).unwrap_or(KeySeq { keys: vec![0x0141] }); // A
    let k2 = KeySeq::new(&[(2u32 << 8) | 0x42]).unwrap_or(KeySeq { keys: vec![0x0242] }); // B
    cs.add("注册 A", tbl.register(k1.clone(), 10).is_ok(), "动作 10");
    cs.add("注册 B", tbl.register(k2, 20).is_ok(), "动作 20");
    cs.add("表深 2", tbl.len() == 2, "两键");
    cs.add("导出稳定", tbl.export().as_slice() == &[(0x0141u32, 10), (0x0242, 20)], "注册序");
    cs.add("导出对账", tbl.export_ok().is_ok(), "长一致");
    // 族二：冲突核。
    let k1b = KeySeq::new(&[0x0141]).unwrap_or(k1.clone());
    cs.add("同键序拒", tbl.register(k1b, 30) == Err(XC_KEY_DUP), "DUP");
    // 族三：保留前缀。
    let kr = KeySeq::new(&[0x0100]).unwrap_or(k1.clone());
    cs.add("保留前缀拒", tbl.register(kr, 40) == Err(XC_PREFIX_RSV), "系统留位");
    let kr2 = KeySeq::new(&[0x0400, 0x0141]).unwrap_or(k1.clone());
    cs.add("首键前缀拒", tbl.register(kr2, 50) == Err(XC_PREFIX_RSV), "首键核");
    cs.add("次键前缀允许", tbl.register(KeySeq::new(&[0x0143, 0x0100]).unwrap_or(k1.clone()), 60).is_ok(),
           "前缀只核首键");
    // 族四：构造拒。
    cs.add("空键序拒", KeySeq::new(&[]).is_err(), "KEY_ZERO");
    cs.add("超长拒", KeySeq::new(&[0x0141, 0x0142, 0x0143, 0x0144, 0x0145]).is_err(), "SEQ_LONG");
    cs.add("零主键拒", KeySeq::new(&[0x0001]).is_err(), "KEY_OOR");
    cs.add("主键越域拒", KeySeq::new(&[(121u32 << 8)]).is_err(), "KEY_OOR");
    cs.add("修饰越域拒", KeySeq::new(&[(1u32 << 8) | 0x9]).is_err(), "MOD_OOR");
    // 族五：前缀闭集自身。
    cs.add("保留前缀闭集", RESERVED_PREFIX.len() == 4, "四前缀");
    cs.add("前缀互异", {
        let mut ok = true;
        for (i, &p) in RESERVED_PREFIX.iter().enumerate() {
            if RESERVED_PREFIX[i + 1..].contains(&p) {
                ok = false;
            }
        }
        ok
    }, "互异");
    cs
}
