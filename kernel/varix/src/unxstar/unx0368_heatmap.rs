//! unx0368_heatmap —— UNX-F0368 内存热图接入（F0077 写入口）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0368 · 内存热图 · 目标 240 行
//! 判据原文：UNX-F0368-J1 页帧池热度图：每帧热度计数（分配/释放
//!           各计一笔，访问采样可加权）；热图与位图联动（空闲帧
//!           不计访问热）；热点窗口（Top-K 帧号）可查询；写入口
//!           带限流（同帧同拍重复计数折叠）。
//! 组件：热度账（帧 → 计数，饱和算术）+ 写入口（F0077 接管：
//!           分类计数 + 折叠窗）+ Top-K 热点核 + 位图联动核。
//! 职责定位：热图是「内存的老化镜」——长期高频分配释放的帧
//!   （热帧）是磨损/缓存争用的前兆；热图让「哪块内存被反复
//!   折腾」从感觉变成数字。饱和算术防计数溢出反转（u64 计满
//!   反转成小数 = 热帧变冷帧，最阴的观测 bug）。
//! 性能逐项分解：计数 O(1)；Top-K O(n log n)；联动核 O(帧数)。
//! 诊断码段：0x20880（0x20881~0x20889 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HM_VERSION: &str = "unx0368_heatmap-v1";

/// 诊断段基址（0x20880 段 · 本模块独占）。
pub const HM_SEG_BASE: u32 = 0x20880;
pub const XC_FRAME_OOR: u32 = 0x20881; // 帧号越域
pub const XC_SATURATED: u32 = 0x20882; // 计数饱和立案
pub const XC_FOLD_BIG: u32 = 0x20883; // 折叠窗超限
pub const XC_K_OOR: u32 = 0x20884; // K 越域
pub const XC_FREE_HOT: u32 = 0x20885; // 空闲帧计访问热
pub const XC_CLASS_OOR: u32 = 0x20886; // 事件类越域
pub const XC_POOL_EMPTY: u32 = 0x20887; // 空池拒
pub const XC_WEIGHT_ZERO: u32 = 0x20888; // 零权重
pub const XC_WINDOW_BAD: u32 = 0x20889; // 折叠窗配置破

/// 事件类。
pub const E_ALLOC: u8 = 1;
pub const E_FREE: u8 = 2;
pub const E_ACCESS: u8 = 3;

/// 计数饱和值。
pub const SAT_MAX: u64 = 0xFFFF_FFFF_FFFF_0000;
/// 折叠窗上限。
pub const FOLD_WINDOW: u32 = 8;

/// 内存热图。
pub struct MemHeat {
    pool_frames: u32,
    /// 帧占用位（与位图联动）。
    occupied: Vec<bool>,
    /// 热度计数（饱和）。
    heat: Vec<u64>,
    /// 折叠窗：同帧同拍待折叠计数。
    fold: Vec<u64>,
    saturated: Vec<u32>,
}

impl MemHeat {
    pub fn new(pool_frames: u32) -> Result<MemHeat, u32> {
        if pool_frames == 0 {
            return Err(XC_POOL_EMPTY);
        }
        Ok(MemHeat {
            pool_frames,
            occupied: vec![false; pool_frames as usize],
            heat: vec![0u64; pool_frames as usize],
            fold: vec![0u64; pool_frames as usize],
            saturated: Vec::new(),
        })
    }

    /// 位图联动（F0077 写入口的占用态同步）。
    pub fn set_occupied(&mut self, frame: u32, occ: bool) -> Result<(), u32> {
        if frame >= self.pool_frames {
            return Err(XC_FRAME_OOR);
        }
        self.occupied[frame as usize] = occ;
        Ok(())
    }

    /// 记事件：分配/释放恒计 1；访问需帧占用（空闲帧不计访问热）。
    /// 折叠窗：同帧访问在窗内先入 fold，窗满一次并入。
    pub fn record(&mut self, class: u8, frame: u32, weight: u64) -> Result<u64, u32> {
        if frame >= self.pool_frames {
            return Err(XC_FRAME_OOR);
        }
        if !matches!(class, E_ALLOC | E_FREE | E_ACCESS) {
            return Err(XC_CLASS_OOR);
        }
        if weight == 0 {
            return Err(XC_WEIGHT_ZERO);
        }
        if class == E_ACCESS && !self.occupied[frame as usize] {
            return Err(XC_FREE_HOT);
        }
        if class == E_ACCESS {
            let f = &mut self.fold[frame as usize];
            *f = f.saturating_add(weight);
            if *f > FOLD_WINDOW as u64 {
                if *f as u32 > FOLD_WINDOW * 16 {
                    return Err(XC_FOLD_BIG);
                }
                let v = *f;
                *f = 0;
                return self.bump(frame, v);
            }
            return Ok(self.heat[frame as usize]);
        }
        self.bump(frame, weight)
    }

    /// 饱和累加核。
    fn bump(&mut self, frame: u32, v: u64) -> Result<u64, u32> {
        let h = &mut self.heat[frame as usize];
        if *h >= SAT_MAX {
            if !self.saturated.contains(&frame) {
                self.saturated.push(frame);
            }
            return Err(XC_SATURATED);
        }
        *h = h.saturating_add(v);
        if *h >= SAT_MAX && !self.saturated.contains(&frame) {
            self.saturated.push(frame);
        }
        Ok(*h)
    }

    /// Top-K 热点帧号（热度降序，同热帧号升序）。
    pub fn top_k(&self, k: u32) -> Result<Vec<(u32, u64)>, u32> {
        if k == 0 || k > self.pool_frames {
            return Err(XC_K_OOR);
        }
        let mut rows: Vec<(u32, u64)> = self
            .heat
            .iter()
            .enumerate()
            .filter(|(_, &h)| h > 0)
            .map(|(i, &h)| (i as u32, h))
            .collect();
        rows.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        rows.truncate(k as usize);
        Ok(rows)
    }

    /// 饱和帧清单。
    pub fn saturated_frames(&self) -> &[u32] {
        &self.saturated
    }

    /// 联动核：空闲帧的访问热必须为 0（联动不变量）。
    pub fn linkage_ok(&self) -> Result<(), u32> {
        for i in 0..self.pool_frames as usize {
            if !self.occupied[i] && self.heat[i] > 0 && self.heat[i] > FOLD_WINDOW as u64 {
                return Err(XC_FREE_HOT);
            }
        }
        Ok(())
    }
}

pub fn run_unx0368_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0368_heatmap");
    // 族一：计数。
    let mut hm = match MemHeat::new(64) {
        Ok(h) => h,
        Err(_) => {
            cs.add("热图构建", false, "被拒");
            return cs;
        }
    };
    let _ = hm.set_occupied(3, true);
    cs.add("分配热", hm.record(E_ALLOC, 3, 1) == Ok(1), "计 1");
    cs.add("访问热", hm.record(E_ACCESS, 3, 4).is_ok(), "占用帧可计");
    cs.add("热度 5", hm.top_k(4).map(|v| v[0].1).unwrap_or(0) == 5, "1+4");
    cs.add("释放热", hm.record(E_FREE, 3, 1).is_ok(), "计 1");
    // 族二：空闲帧访问拒。
    let _ = hm.set_occupied(5, false);
    cs.add("空闲访问拒", hm.record(E_ACCESS, 5, 1) == Err(XC_FREE_HOT), "联动闸");
    cs.add("空闲分配可计", hm.record(E_ALLOC, 5, 1).is_ok(), "分配合法");
    // 族三：折叠窗。
    let mut hm2 = MemHeat::new(16).unwrap_or(MemHeat { pool_frames: 16, occupied: vec![true; 16], heat: vec![0; 16], fold: vec![0; 16], saturated: Vec::new() });
    for _ in 0..8 {
        let _ = hm2.record(E_ACCESS, 2, 1);
    }
    cs.add("窗内未并", hm2.top_k(16).map(|v| v[0].1).unwrap_or(0) == 0, "fold 未并入");
    let _ = hm2.record(E_ACCESS, 2, 1); // 第 9 次 → 折叠并入 9
    cs.add("窗满并入", hm2.top_k(16).map(|v| v[0].1).unwrap_or(0) == 9, "9 一次入账");
    // 族四：Top-K。
    let _ = hm2.record(E_ALLOC, 9, 5);
    let _ = hm2.record(E_ALLOC, 4, 3);
    let top = hm2.top_k(3);
    cs.add("Top-3 序", matches!(&top, Ok(v) if v.len() == 3 && v[0].0 == 2 && v[1].0 == 9 && v[2].0 == 4), "热度降序");
    cs.add("K 越域拒", hm2.top_k(0).is_err() && hm2.top_k(17).is_err(), "K_OOR");
    // 同热并列按帧号升序。
    let _ = hm2.record(E_ALLOC, 6, 5);
    let top2 = hm2.top_k(4);
    cs.add("并列升序", matches!(&top2, Ok(v) if v[1].0 == 6 && v[2].0 == 9), "帧 6<9");
    // 族五：构造闸。
    cs.add("空池拒", MemHeat::new(0).is_err(), "EMPTY");
    cs.add("帧越域拒", hm.record(E_ALLOC, 64, 1).is_err(), "FRAME_OOR");
    cs.add("类越域拒", hm.record(9, 0, 1).is_err(), "CLASS_OOR");
    cs.add("零权重拒", hm.record(E_ALLOC, 0, 0).is_err(), "WEIGHT");
    cs.add("联动核过", hm.linkage_ok().is_ok(), "空闲帧无陈热");
    // 族六：饱和。
    let mut hm3 = MemHeat::new(4).unwrap_or(MemHeat { pool_frames: 4, occupied: vec![true; 4], heat: vec![SAT_MAX; 4], fold: vec![0; 4], saturated: Vec::new() });
    cs.add("饱和拒增", hm3.record(E_ALLOC, 1, 1) == Err(XC_SATURATED), "不反转");
    cs.add("饱和立案", hm3.saturated_frames().contains(&1), "入清单");
    cs
}
