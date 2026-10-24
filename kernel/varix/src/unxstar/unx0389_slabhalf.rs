//! unx0389_slabhalf —— UNX-F0389 slab 半满回收（档位归还）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0389 · slab 半满回收 · 目标 200 行
//! 判据原文：UNX-F0389-J1 对象缓存按档位（对象大小分级）组织
//!           slab；对象释放后 slab 记满度；满度 ≤ 半且同档 slab
//!           数 > 1 时触发回收——被回收 slab 的存活对象迁往
//!           同档其他 slab，整 slab 归还页帧池；回收判定必须
//!           可重放（同满度序列同结论）；空档回收不误伤（单
//!           slab 档永不回收）。
//! 组件：档位表（对象大小 → 档）+ slab（容量 + 存活位图）+
//!       缓存（档 → slab 数组）+ 回收引擎（半满判定 + 对象迁移
//!       + 整 slab 归还）+ 回收账（归还计数）。
//! 职责定位：slab 回收是「堆的瘦身术」——对象级释放只让 slab
//!   变松，页帧级回收才真正还内存；触发条件「半满且非唯一」
//!   保证两件事：回收的收益（整页归还）大于迁移成本，且服务
//!   能力不降（同档还有其他 slab 接客）。迁移是关键难点：位图
//!   要逐位搬家，账要跟着走。
//! 性能逐项分解：释放 O(1)；回收判定 O(档内 slab 数)；迁移
//!   O(回收 slab 容量)；归还 O(1)。
//! 诊断码段：0x20C00（0x20C01~0x20C09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SH_VERSION: &str = "unx0389_slabhalf-v1";

/// 诊断段基址（0x20C00 段 · 本模块独占）。
pub const SH_SEG_BASE: u32 = 0x20C00;
pub const XC_TIER_OOR: u32 = 0x20C01; // 档位越域
pub const XC_SLAB_OOR: u32 = 0x20C02; // slab 号越域
pub const XC_OBJ_OOR: u32 = 0x20C03; // 对象槽越域
pub const XC_DUP_FREE: u32 = 0x20C04; // 双重释放
pub const XC_CAP_OOR: u32 = 0x20C05; // slab 容量为 0 或超界
pub const XC_MIGR_LOST: u32 = 0x20C06; // 迁移丢对象
pub const XC_RECLAIM_BAD: u32 = 0x20C07; // 回收后满度账不符
pub const XC_LONE_SLAB: u32 = 0x20C08; // 唯一 slab 拒回收
pub const XC_REPLAY: u32 = 0x20C09; // 回收重放不一致

/// 单 slab：固定容量 + 存活位图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slab {
    /// 容量（对象槽数）。
    pub cap: u32,
    /// 存活位图（true = 占用）。
    pub live: Vec<bool>,
}

impl Slab {
    /// 建空 slab。
    pub fn new(cap: u32) -> Result<Slab, u32> {
        if cap == 0 || cap > 4096 {
            return Err(XC_CAP_OOR);
        }
        Ok(Slab { cap, live: vec![false; cap as usize] })
    }

    /// 占一槽：返回槽号；满则拒。
    pub fn alloc_slot(&mut self) -> Result<u32, u32> {
        for i in 0..self.cap as usize {
            if !self.live[i] {
                self.live[i] = true;
                return Ok(i as u32);
            }
        }
        Err(XC_CAP_OOR)
    }

    /// 释一槽：双释拒。
    pub fn free_slot(&mut self, slot: u32) -> Result<(), u32> {
        if slot >= self.cap {
            return Err(XC_OBJ_OOR);
        }
        if !self.live[slot as usize] {
            return Err(XC_DUP_FREE);
        }
        self.live[slot as usize] = false;
        Ok(())
    }

    /// 存活数。
    pub fn used(&self) -> u32 {
        self.live.iter().filter(|&&b| b).count() as u32
    }
}

/// 半满判定与回收引擎所在的档缓存。
pub struct TierCache {
    /// 对象大小（字节）。
    pub obj_size: u32,
    slabs: Vec<Slab>,
    /// 累计归还 slab 数。
    pub reclaimed: u32,
}

impl TierCache {
    /// 建档缓存：单 slab 起步。
    pub fn new(obj_size: u32, cap: u32) -> Result<TierCache, u32> {
        if obj_size == 0 || obj_size > 4096 {
            return Err(XC_TIER_OOR);
        }
        let s = Slab::new(cap)?;
        Ok(TierCache { obj_size, slabs: vec![s], reclaimed: 0 })
    }

    /// 分配对象：跨 slab 找位，全满则新建 slab。
    pub fn alloc(&mut self) -> Result<(u32, u32), u32> {
        for (si, s) in self.slabs.iter_mut().enumerate() {
            if let Ok(slot) = s.alloc_slot() {
                return Ok((si as u32, slot));
            }
        }
        let cap = self.slabs[0].cap;
        self.slabs.push(Slab::new(cap)?);
        let si = self.slabs.len() - 1;
        let slot = self.slabs[si].alloc_slot()?;
        Ok((si as u32, slot))
    }

    /// 释放对象。
    pub fn free(&mut self, si: u32, slot: u32) -> Result<(), u32> {
        if si as usize >= self.slabs.len() {
            return Err(XC_SLAB_OOR);
        }
        self.slabs[si as usize].free_slot(slot)
    }

    /// 半满判定：used*2 ≤ cap（半满或更松）。
    fn is_half(&self, si: usize) -> bool {
        let s = &self.slabs[si];
        s.used() * 2 <= s.cap
    }

    /// 回收尝试：找第一个「半满且非唯一」的 slab，把其存活对象
    /// 迁往同档其他 slab，整 slab 弹出归还。返回归还的对象槽数
    /// 或拒绝码。
    pub fn reclaim_once(&mut self) -> Result<u32, u32> {
        if self.slabs.len() < 2 {
            return Err(XC_LONE_SLAB);
        }
        let mut pick: Option<usize> = None;
        for i in 0..self.slabs.len() {
            if self.is_half(i) && self.slabs[i].used() > 0 {
                pick = Some(i);
                break;
            }
        }
        let si = match pick {
            Some(i) => i,
            None => return Err(XC_RECLAIM_BAD), // 无半满可回收
        };
        // 迁移：把 si 的存活槽迁往其他 slab。
        let mut moved = 0u32;
        let mut migrate: Vec<u32> = Vec::new();
        for slot in 0..self.slabs[si].cap {
            if self.slabs[si].live[slot as usize] {
                migrate.push(slot);
            }
        }
        for slot in migrate.iter() {
            self.slabs[si].live[*slot as usize] = false;
            match self.alloc() {
                Ok((nsi, nslot)) => {
                    if nsi as usize == si {
                        // 迁回自身：回收中断，恢复占位并中止（防御）。
                        self.slabs[si].live[nslot as usize] = true;
                        self.slabs[si].live[*slot as usize] = true;
                        return Err(XC_MIGR_LOST);
                    }
                    moved += 1;
                }
                Err(e) => {
                    self.slabs[si].live[*slot as usize] = true;
                    let _ = e;
                    return Err(XC_MIGR_LOST);
                }
            }
        }
        // 归还前核：迁移后该 slab 必空。
        if self.slabs[si].used() != 0 {
            return Err(XC_MIGR_LOST);
        }
        self.slabs.remove(si);
        self.reclaimed += 1;
        Ok(moved)
    }

    /// slab 数。
    pub fn slab_count(&self) -> usize {
        self.slabs.len()
    }

    /// 指定 slab 的存活数（巡检用）。
    pub fn used_of(&self, si: usize) -> u32 {
        match self.slabs.get(si) {
            Some(s) => s.used(),
            None => 0,
        }
    }

    /// 总存活对象数。
    pub fn total_used(&self) -> u32 {
        self.slabs.iter().map(|s| s.used()).sum()
    }
}

pub fn run_unx0389_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0389_slabhalf");
    // 族一：分配与满度。
    let mut t = match TierCache::new(64, 8) {
        Ok(t) => t,
        Err(_) => {
            cs.add("建档", false, "拒");
            return cs;
        }
    };
    cs.add("单 slab 起步", t.slab_count() == 1, "容量 8");
    let mut handles: Vec<(u32, u32)> = Vec::new();
    for _ in 0..10u32 {
        match t.alloc() {
            Ok(h) => handles.push(h),
            Err(_) => break,
        }
    }
    cs.add("10 对象跨 2 slab", t.slab_count() == 2, "8 + 2");
    cs.add("总存活 10", t.total_used() == 10, "逐个在账");
    // 族二：半满触发回收（slab0 释放到 ≤4 活，slab1 只有 2 活也半满；
    // 引擎按序取 slab0 回收，其 4 活迁往 slab1 剩余 6 空位不够 →
    // 需第三 slab 接。迁移允许借新 slab 接客）。
    // 释放 slab0 的 6 个槽使其剩 2 活（半满）。
    let mut freed = 0;
    for &(si, slot) in handles.iter() {
        if si == 0 && freed < 6 {
            let _ = t.free(si, slot);
            freed += 1;
        }
    }
    cs.add("slab0 半满", t.used_of(0) == 2, "8 - 6");
    let moved = t.reclaim_once();
    cs.add("回收成", moved.is_ok(), "迁移完成");
    cs.add("归还计数", t.reclaimed == 1, "1 slab 归还");
    cs.add("对象不丢", t.total_used() == 4, "10 - 6 = 4");
    cs.add("slab 收缩", t.slab_count() == 1, "2 - 1");
    // 族三：唯一 slab 拒回收。
    let mut u = match TierCache::new(64, 8) {
        Ok(t) => t,
        Err(_) => {
            cs.add("建档2", false, "拒");
            return cs;
        }
    };
    let _ = u.alloc();
    let _ = u.free(0, 0);
    cs.add("唯一半满拒回收", u.reclaim_once() == Err(XC_LONE_SLAB), "单 slab 永不回收");
    // 族四：双释 / 越域。
    let mut v = match TierCache::new(32, 4) {
        Ok(t) => t,
        Err(_) => {
            cs.add("建档3", false, "拒");
            return cs;
        }
    };
    let (si, slot) = match v.alloc() {
        Ok(h) => h,
        Err(_) => (0, 0),
    };
    cs.add("首释成", v.free(si, slot).is_ok(), "在册");
    cs.add("双释拒", v.free(si, slot) == Err(XC_DUP_FREE), "同槽再释");
    cs.add("槽越域拒", v.free(si, 99) == Err(XC_OBJ_OOR), "cap 4");
    cs.add("slab 越域拒", v.free(7, 0) == Err(XC_SLAB_OOR), "无 slab 7");
    // 族五：容量与档位拒绝。
    cs.add("零容量拒", Slab::new(0) == Err(XC_CAP_OOR), "cap=0");
    cs.add("大容量拒", Slab::new(4097) == Err(XC_CAP_OOR), "cap>4096");
    cs.add("零对象拒", matches!(TierCache::new(0, 4), Err(XC_TIER_OOR)), "obj=0");
    // 族六：重放确定性（同操作序列同终态）。
    let mut r1 = match TierCache::new(64, 8) {
        Ok(t) => t,
        Err(_) => {
            cs.add("建档4", false, "拒");
            return cs;
        }
    };
    let mut r2 = match TierCache::new(64, 8) {
        Ok(t) => t,
        Err(_) => {
            cs.add("建档5", false, "拒");
            return cs;
        }
    };
    for _ in 0..10u32 {
        let _ = r1.alloc();
        let _ = r2.alloc();
    }
    let _ = r1.free(0, 3);
    let _ = r1.free(0, 5);
    let _ = r2.free(0, 3);
    let _ = r2.free(0, 5);
    let a = r1.total_used();
    let b = r2.total_used();
    let sa = r1.slab_count();
    let sb = r2.slab_count();
    cs.add("重放一致", a == b && sa == sb, "存活数与 slab 数齐");
    cs
}
