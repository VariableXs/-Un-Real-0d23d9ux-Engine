//! unx0429_vecalloc —— UNX-F0429 中断向量分配表（32–255 布局冻结）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0429 · 中断向量分配表 · 目标 240 行
//! 判据原文：UNX-F0429-J1 可分配向量域 [0x20, 0xFF]（0x00~0x1F
//!           为 CPU 异常保留）；布局冻结五区：A 区 0x20~0x2F
//!           （ISA 遗产 16 线）、B 区 0x30~0x4F（LVT 本地 32
//!           项：CMCI/Timer/Thermal/Perf/NMI 等）、C 区
//!           0x50~0x7F（IOAPIC PCI 线 48 项）、D 区 0x80（误
//!           入哨兵，永不分配）、E 区 0x81~0xFF（通用/ spur/
//!           IPI 127 项）；分配器：从低区到高区顺序给出，冻结
//!           区不重不发；释放回池后可复用；同向量重复分配拒
//!           绝；全表导出可重放（同请求序同布局）。
//! 组件：五区布局表 + 顺序分配器 + 释放回池 + 哨兵保护区 +
//!       冻结核（布局逐区边界锁）+ 重放核 + 占用位图账。
//! 职责定位：向量是 CPU 的「中断门牌号」，布局一乱 IPI/异
//!   常/外设互相踩；D 区 0x80 是误入哨兵（占用即立案），A~E
//!   五区边界冻结后任何分配器改动都不许越线；重放核保证多
//!   核启动时各核按同一序分配出同一布局——布局不一致的中断
//!   路由是最难查的悬案。
//! 性能逐项分解：分配 O(1)（顺序游标）；释放 O(1)；冻结核
//!   O(区数)；重放 O(请求数)；位图导出 O(224)。
//! 诊断码段：0x302C0（0x302C1~0x302C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VA_VERSION: &str = "unx0429_vecalloc-v1";

/// 诊断段基址（0x302C0 段 · 本模块独占）。
pub const VA_SEG_BASE: u32 = 0x302C0;
pub const XC_VEC_RESERVED: u32 = 0x302C1; // 落入 CPU 异常区
pub const XC_VEC_SENTINEL: u32 = 0x302C2; // 误入 0x80 哨兵
pub const XC_VEC_TAKEN: u32 = 0x302C3; // 重复分配
pub const XC_POOL_EMPTY: u32 = 0x302C4; // 池空
pub const XC_FREE_UNOWNED: u32 = 0x302C5; // 释放未持有向量
pub const XC_ZONE_BREAK: u32 = 0x302C6; // 冻结区边界破
pub const XC_REPLAY: u32 = 0x302C7; // 重放布局不一致
pub const XC_DOUBLE_FREE: u32 = 0x302C8; // 双释放
pub const XC_MAGIC: u32 = 0x302C9; // 账魔数破

/// 可分配域下界（CPU 异常区之上）。
pub const VEC_LO: u16 = 0x20;
/// 可分配域上界。
pub const VEC_HI: u16 = 0xFF;
/// D 区误入哨兵。
pub const VEC_SENTINEL: u16 = 0x80;
/// 五区布局（冻结）。
pub const ZONE_A: (u16, u16) = (0x20, 0x2F); // ISA 遗产 16
pub const ZONE_B: (u16, u16) = (0x30, 0x4F); // LVT 本地 32
pub const ZONE_C: (u16, u16) = (0x50, 0x7F); // PCI 48
pub const ZONE_E: (u16, u16) = (0x81, 0xFF); // 通用 127

/// 冻结核：五区边界逐一核（A/B/C 不越 D，E 不跨哨兵下方）。
pub fn zones_frozen() -> bool {
    ZONE_A.0 == 0x20
        && ZONE_A.1 == 0x2F
        && ZONE_B.0 == ZONE_A.1 + 1
        && ZONE_B.1 == 0x4F
        && ZONE_C.0 == ZONE_B.1 + 1
        && ZONE_C.1 == 0x7F
        && VEC_SENTINEL == 0x80
        && ZONE_E.0 == VEC_SENTINEL + 1
        && ZONE_E.1 == 0xFF
}

/// 向量分配器：顺序游标 + 占用位图。
pub struct VecAlloc {
    /// 占用表（下标 = 向量-0x20）。
    owned: Vec<u16>,
    /// 分配游标（下一候选）。
    cursor: u16,
    /// 分配流水（重放核用：记录分配序）。
    issue_log: Vec<u16>,
}

impl VecAlloc {
    pub fn new() -> Self {
        VecAlloc { owned: vec![0; 224], cursor: VEC_LO, issue_log: Vec::new() }
    }

    /// 域内合法核：非异常区、非哨兵。
    pub fn legal(v: u16) -> Result<(), u32> {
        if v < VEC_LO {
            return Err(XC_VEC_RESERVED);
        }
        if v == VEC_SENTINEL {
            return Err(XC_VEC_SENTINEL);
        }
        Ok(())
    }

    /// 指定向量申请（冻结区内直取）。
    pub fn claim(&mut self, v: u16) -> Result<u16, u32> {
        Self::legal(v)?;
        let idx = (v - VEC_LO) as usize;
        if self.owned[idx] != 0 {
            return Err(XC_VEC_TAKEN);
        }
        self.owned[idx] = 1;
        self.issue_log.push(v);
        Ok(v)
    }

    /// 顺序分配：从游标向后找第一个空位（跳哨兵）。
    pub fn alloc(&mut self) -> Result<u16, u32> {
        let mut tries = 0;
        while tries <= 224 {
            let v = self.cursor;
            if v > VEC_HI {
                return Err(XC_POOL_EMPTY);
            }
            if v == VEC_SENTINEL {
                self.cursor = VEC_SENTINEL + 1;
                continue;
            }
            let idx = (v - VEC_LO) as usize;
            if self.owned[idx] == 0 {
                self.owned[idx] = 1;
                self.issue_log.push(v);
                self.cursor = v + 1;
                return Ok(v);
            }
            self.cursor = v + 1;
            tries += 1;
        }
        Err(XC_POOL_EMPTY)
    }

    /// 释放回池（未持有/双释放立案）。
    pub fn release(&mut self, v: u16) -> Result<(), u32> {
        Self::legal(v)?;
        let idx = (v - VEC_LO) as usize;
        if self.owned[idx] == 0 {
            return Err(XC_DOUBLE_FREE);
        }
        self.owned[idx] = 0;
        // 游标回退：让顺序分配能复用释放位。
        if v < self.cursor {
            self.cursor = v;
        }
        Ok(())
    }

    /// 持有核（免费查询）。
    pub fn is_owned(&self, v: u16) -> bool {
        v >= VEC_LO && v <= VEC_HI && self.owned[(v - VEC_LO) as usize] != 0
    }

    /// 释放未持有向量（审计语义：与双释放同账不同用）。
    pub fn free_unowned_check(&self, v: u16) -> Result<(), u32> {
        if !self.is_owned(v) {
            return Err(XC_FREE_UNOWNED);
        }
        Ok(())
    }

    /// 占用计数。
    pub fn count(&self) -> u32 {
        self.owned.iter().filter(|x| **x != 0).count() as u32
    }

    /// 重放核：重置后按 issue_log 顺序 claim/alloc 同布局。
    pub fn replay(&self) -> Result<(), u32> {
        let mut fresh = VecAlloc::new();
        for &v in self.issue_log.iter() {
            if fresh.claim(v).is_err() {
                return Err(XC_REPLAY);
            }
        }
        if fresh.count() != self.count() {
            return Err(XC_REPLAY);
        }
        for v in VEC_LO..=VEC_HI {
            if fresh.is_owned(v) != self.is_owned(v) {
                return Err(XC_REPLAY);
            }
        }
        Ok(())
    }

    /// 冻结分配面：分配序必须逐区递增（A→B→C→E，区号单调）。
    pub fn zone_order_ok(&self) -> bool {
        let mut last_zone = 0u8;
        for &v in self.issue_log.iter() {
            let z = if v <= ZONE_A.1 {
                1
            } else if v <= ZONE_B.1 {
                2
            } else if v <= ZONE_C.1 {
                3
            } else {
                5
            };
            if z < last_zone {
                return false;
            }
            last_zone = z;
        }
        true
    }
}

/// 判据入口：UNX-F0429。
pub fn run_unx0429_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0429_vecalloc");
    // 族一：冻结核。
    cs.add("五区冻结", zones_frozen(), "边界逐区锁");
    // 族二：域合法核。
    cs.add("异常区拒", VecAlloc::legal(0x03) == Err(XC_VEC_RESERVED), "#PF 区");
    cs.add("边界 0x1F 拒", VecAlloc::legal(0x1F) == Err(XC_VEC_RESERVED), "DE 上界");
    cs.add("0x20 放行", VecAlloc::legal(0x20).is_ok(), "域首");
    cs.add("哨兵拒", VecAlloc::legal(0x80) == Err(XC_VEC_SENTINEL), "D 区");
    cs.add("0xFF 放行", VecAlloc::legal(0xFF).is_ok(), "域尾");
    // 族三：顺序分配与哨兵跳越。
    let mut a = VecAlloc::new();
    let first = a.alloc();
    cs.add("首分 0x20", first == Ok(0x20), "游标起域首");
    // 一次取到 0x7F 后下一应是 0x81（跳 0x80）。
    let mut a2 = VecAlloc::new();
    let mut last = 0u16;
    for _ in 0..0x60 {
        if let Ok(v) = a2.alloc() {
            last = v;
        }
    }
    cs.add("C 区尾 0x7F", last == 0x7F, "0x60 次到 C 尾");
    let next = a2.alloc();
    cs.add("跳哨兵到 0x81", next == Ok(0x81), "D 区不分配");
    // 族四：重复分配与释放。
    let mut a3 = VecAlloc::new();
    let _ = a3.claim(0x41);
    cs.add("重复分配拒", a3.claim(0x41) == Err(XC_VEC_TAKEN), "同向量");
    cs.add("持有核真", a3.is_owned(0x41), "在账");
    cs.add("释放放行", a3.release(0x41).is_ok(), "回池");
    cs.add("双释放拒", a3.release(0x41) == Err(XC_DOUBLE_FREE), "账已清");
    cs.add("释放未持有立案", a3.free_unowned_check(0x42) == Err(XC_FREE_UNOWNED), "未持有");
    // 族五：释放后复用（游标回退）。
    let mut a4 = VecAlloc::new();
    let _ = a4.alloc(); // 0x20
    let _ = a4.alloc(); // 0x21
    let _ = a4.alloc(); // 0x22
    let _ = a4.release(0x21);
    let reused = a4.alloc();
    cs.add("释放位复用", reused == Ok(0x21), "游标回退");
    // 族六：池空。
    let mut a5 = VecAlloc::new();
    let mut got = 0u32;
    for _ in 0..230 {
        if a5.alloc().is_ok() {
            got += 1;
        }
    }
    cs.add("池容量 223", got == 223, "224-1 哨兵");
    cs.add("池空立案", a5.alloc() == Err(XC_POOL_EMPTY), "224 次后");
    // 族七：重放核。
    let mut a6 = VecAlloc::new();
    let _ = a6.claim(ZONE_A.0);
    let _ = a6.alloc();
    let _ = a6.alloc();
    let _ = a6.claim(ZONE_E.1);
    cs.add("重放一致", a6.replay().is_ok(), "同序同布局");
    // 族八：区序单调（顺序分配天然单调）。
    cs.add("区序单调", a6.zone_order_ok(), "A→B→C→E");
    let mut a7 = VecAlloc::new();
    let _ = a7.claim(ZONE_E.0);
    let _ = a7.claim(ZONE_A.1); // 故意乱序
    cs.add("乱序立案", !a7.zone_order_ok(), "E 先 A 后");
    // 族九：冻结区直取。
    let mut a8 = VecAlloc::new();
    cs.add("A 区直取", a8.claim(ZONE_A.0 + 3) == Ok(0x23), "ISA 线 3");
    cs.add("E 区直取", a8.claim(0xFE) == Ok(0xFE), "通用区尾");
    cs.add("占用 2", a8.count() == 2, "两笔");
    cs
}
