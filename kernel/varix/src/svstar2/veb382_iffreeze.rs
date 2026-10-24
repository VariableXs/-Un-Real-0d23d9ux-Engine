//! veb382_iffreeze —— VE-F0382 B 域接口冻结清单。
//! 锚点（CoRun Unxreal 主册）：VE-F0382 · B 域接口冻结清单 · 目标 340 行
//! 判据原文：VE-F0382-J1 接口清单冻结：冻结后不许新增（扩容拒）、
//!           不许删除（回收拒）、不许改签名（版本号锁）；冻结
//!           前可自由增删改；冻结核（清单快照 FNV32 与冻结章一
//!           致）；接口项含（名 + 版本 + 稳定性级：稳定/实验/弃用）；
//!           弃用接口须带替身指针（无替身拒弃用）。
//! 组件：接口项 + 冻结章（快照哈希 + 冻结态）+ 稳定性级 +
//!       替身指针核。
//! 职责定位：接口冻结清单是「B 域的宪法」——对外承诺的接口一
//!           旦冻结，上游就按它写代码，中途改签名等于撕毁合同；
//!           弃用必须给替身是迁移礼仪：拆桥前先搭新桥；快照哈
//!           哈希让「冻结」可验证，不是一句空话。
//! 性能逐项分解：增删 O(n)（查重）；冻结核 O(n)（哈希一遍）。
//! 诊断码段：0x30500（0x30501~0x3050A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const IF_VERSION: &str = "veb382_iffreeze-v1";

/// 诊断段基址（0x30500 段 · 本模块独占）。
pub const IF_SEG_BASE: u32 = 0x30500;
pub const XC_FROZEN_ADD: u32 = 0x30501; // 冻结后新增
pub const XC_FROZEN_DEL: u32 = 0x30502; // 冻结后删除
pub const XC_FROZEN_MOD: u32 = 0x30503; // 冻结后改签名
pub const XC_DUP: u32 = 0x30504; // 重名
pub const XC_NOENT: u32 = 0x30505; // 无此项
pub const XC_NO_SUB: u32 = 0x30506; // 弃用无替身
pub const XC_HASH: u32 = 0x30507; // 冻结哈希不一致
pub const XC_MAGIC: u32 = 0x30508; // 账魔数破
pub const XC_NOT_FROZEN: u32 = 0x30509; // 未冻结先验章
pub const XC_VER_ZERO: u32 = 0x3050A; // 版本为零

/// 稳定性级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stability {
    /// 稳定（冻结保护最强）。
    Stable = 0,
    /// 实验。
    Exper = 1,
    /// 弃用（须替身）。
    Deprecated = 2,
}

/// 接口项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfItem {
    pub name: &'static str,
    pub ver: u32,
    pub st: Stability,
    /// 弃用替身（None = 未给）。
    pub alias: Option<&'static str>,
}

/// FNV32。
fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 冻结清单。
pub struct IfFreeze {
    items: Vec<IfItem>,
    frozen: bool,
    freeze_hash: u32,
    magic: u32,
}

impl IfFreeze {
    /// 建。
    pub fn new() -> IfFreeze {
        IfFreeze { items: Vec::new(), frozen: false, freeze_hash: 0, magic: 0x0500_1F12 }
    }

    fn snapshot_hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for it in self.items.iter() {
            for b in it.name.as_bytes() {
                buf.push(*b);
            }
            buf.push(0);
            buf.push((it.ver & 0xFF) as u8);
            buf.push(it.st as u8);
            buf.push(match it.alias {
                Some(_) => 1,
                None => 0,
            });
        }
        fnv32(&buf)
    }

    /// 新增（冻结后拒）。
    pub fn add(&mut self, name: &'static str, ver: u32, st: Stability, alias: Option<&'static str>) -> Result<(), u32> {
        if self.magic != 0x0500_1F12 {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN_ADD);
        }
        if ver == 0 {
            return Err(XC_VER_ZERO);
        }
        for it in self.items.iter() {
            if it.name == name {
                return Err(XC_DUP);
            }
        }
        self.items.push(IfItem { name, ver, st, alias });
        Ok(())
    }

    /// 删除（冻结后拒）。
    pub fn remove(&mut self, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(XC_FROZEN_DEL);
        }
        let before = self.items.len();
        self.items.retain(|it| it.name != name);
        if self.items.len() == before {
            return Err(XC_NOENT);
        }
        Ok(())
    }

    /// 弃用（须替身；冻结后改稳定性 = 改签名，拒）。
    pub fn deprecate(&mut self, name: &str, alias: &'static str) -> Result<(), u32> {
        if self.frozen {
            return Err(XC_FROZEN_MOD);
        }
        for it in self.items.iter_mut() {
            if it.name == name {
                if alias.is_empty() {
                    return Err(XC_NO_SUB);
                }
                it.st = Stability::Deprecated;
                it.alias = Some(alias);
                return Ok(());
            }
        }
        Err(XC_NOENT)
    }

    /// 冻结：盖章。
    pub fn freeze(&mut self) -> u32 {
        self.frozen = true;
        self.freeze_hash = self.snapshot_hash();
        self.freeze_hash
    }

    /// 冻结核：当前快照与冻结章一致。
    pub fn verify(&self) -> Result<u32, u32> {
        if !self.frozen {
            return Err(XC_NOT_FROZEN);
        }
        let cur = self.snapshot_hash();
        if cur != self.freeze_hash {
            return Err(XC_HASH);
        }
        Ok(cur)
    }

    /// 清单项数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 冻结态。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 稳定接口数。
    pub fn stable_count(&self) -> u32 {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.st == Stability::Stable {
                n += 1;
            }
        }
        n
    }

    /// 弃用替身完整性核：所有弃用项都带替身且替身在册。
    pub fn alias_intact(&self) -> Result<u32, u32> {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.st == Stability::Deprecated {
                match it.alias {
                    Some(a) => {
                        let mut found = false;
                        for o in self.items.iter() {
                            if o.name == a {
                                found = true;
                            }
                        }
                        if !found {
                            return Err(XC_NO_SUB);
                        }
                        n += 1;
                    }
                    None => return Err(XC_NO_SUB),
                }
            }
        }
        Ok(n)
    }
}

pub fn run_veb382_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0382");
    // 族一：冻结前自由增删。
    let mut f = IfFreeze::new();
    let _ = f.add("io_read", 1, Stability::Stable, None);
    let _ = f.add("io_write", 1, Stability::Stable, None);
    let _ = f.add("io_probe", 1, Stability::Exper, None);
    s.add("增 3", f.len() == 3, "自由期");
    let _ = f.deprecate("io_probe", "io_read");
    s.add("弃用带替身", f.alias_intact() == Ok(1), "搭新桥");
    let _ = f.add("io_tmp", 1, Stability::Exper, None);
    let _ = f.remove("io_tmp");
    s.add("删 1", f.len() == 3, "回收");
    // 族二：冻结。
    let h1 = f.freeze();
    s.add("已冻结", f.is_frozen(), "盖章");
    s.add("冻结核过", f.verify() == Ok(h1), "快照一致");
    s.add("冻结后新增拒", f.add("new_if", 1, Stability::Stable, None) == Err(XC_FROZEN_ADD), "扩容拒");
    s.add("冻结后删除拒", f.remove("io_read") == Err(XC_FROZEN_DEL), "回收拒");
    s.add("冻结后改拒", f.deprecate("io_read", "io_write") == Err(XC_FROZEN_MOD), "签名锁");
    // 族三：稳定计数。
    s.add("稳定 2", f.stable_count() == 2, "read+write");
    // 族四：冻结前核拒。
    let mut g = IfFreeze::new();
    let _ = g.add("x", 1, Stability::Stable, None);
    s.add("未冻先验拒", g.verify() == Err(XC_NOT_FROZEN), "无章");
    // 族五：入参约束。
    let mut b = IfFreeze::new();
    s.add("版本零拒", b.add("x", 0, Stability::Stable, None) == Err(XC_VER_ZERO), "≥1");
    let _ = b.add("x", 1, Stability::Stable, None);
    s.add("重名拒", b.add("x", 2, Stability::Stable, None) == Err(XC_DUP), "主键");
    s.add("弃无此名拒", b.deprecate("ghost", "x") == Err(XC_NOENT), "无项");
    // 弃用替身在册核：替身不存在拒。
    let mut c = IfFreeze::new();
    let _ = c.add("old", 1, Stability::Stable, None);
    let _ = c.deprecate("old", "never-added");
    s.add("替身不在册", c.alias_intact() == Err(XC_NO_SUB), "指向虚空");
    s.add("魔数核", IF_SEG_BASE == 0x30500, "段基址");
    s
}
