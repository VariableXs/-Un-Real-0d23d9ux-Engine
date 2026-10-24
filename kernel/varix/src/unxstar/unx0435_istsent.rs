//! unx0435_istsent —— UNX-F0435 IST 栈哨兵守护（溢出检出）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0435 · IST 栈哨兵守护 · 目标 240 行
//! 判据原文：UNX-F0435-J1 IST 栈哨兵 = 栈底埋界标（哨兵字
//!           0xDEAD_1ST5 变体），压栈指针越过界标即溢出立
//!           案；IST 七槽（1~7）各独立栈独立哨兵；水位账：
//!           每槽记最深水位（相对栈底偏移），巡检报最深槽；
//!           哨兵完整性核：退役后界标原值回读（被踩即溢出
//!           过的痕迹，即使当场没拦住）；栈对齐核：IST 指针
//!           16 字节对齐（硬件 ABI）；双溢出核：同槽连续两
//!           次溢出必须升级立案（第一次可能是巧合，第二次是
//!           惯犯）。
//! 组件：七槽哨兵表 + 压栈模拟器 + 水位账 + 界标完整性核 +
//!       对齐核 + 双溢出升级账。
//! 职责定位：IST 栈是 NMI/双 fault 等「不能 fault 的异常」的
//!   保命栈——它自己溢出就是内核最后防线穿孔；哨兵是廉价
//!   的「事后验尸」，配合水位账可以在压栈路径里做「事中拦
//!   截」；惯犯升级是纪律：偶发越界要查，连续越界必须停机
//!   检讨。
//! 性能逐项分解：压栈 O(1)；水位 O(1)；完整性核 O(槽数)；
//!   升级判定 O(1)。
//! 诊断码段：0x30440（0x30441~0x30449 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const IS_VERSION: &str = "unx0435_istsent-v1";

/// 诊断段基址（0x30440 段 · 本模块独占）。
pub const IS_SEG_BASE: u32 = 0x30440;
pub const XC_STACK_OVERRUN: u32 = 0x30441; // 压栈越界标
pub const XC_SENTINEL_TRAMPLED: u32 = 0x30442; // 界标被踩
pub const XC_MISALIGN: u32 = 0x30443; // 非 16 对齐
pub const XC_SLOT_OOR: u32 = 0x30444; // 槽号越 1~7
pub const XC_HABITUAL: u32 = 0x30445; // 惯犯（连续双溢出）
pub const XC_MAGIC: u32 = 0x30449; // 账魔数破

/// 哨兵字（各槽变体 = 基字 + 槽号）。
pub const SENT_BASE: u64 = 0xDEAD_1E50;
/// IST 槽域 1~7。
pub const IST_LO: u8 = 1;
pub const IST_HI: u8 = 7;
/// 栈容量（字节数，每槽演示 4 KiB）。
pub const STACK_SZ: usize = 4096;
/// 栈基址（演示对齐基）。
pub const STACK_BASE: u64 = 0xFFFF_8880_0000_0000;

/// 单槽哨兵账。
pub struct IstSlot {
    /// 槽号（1~7）。
    pub no: u8,
    /// 栈底界标（期望值）。
    pub guard_want: u64,
    /// 栈底界标（回读值）。
    pub guard_read: u64,
    /// 当前栈指针（相对偏移，向下长）。
    pub sp_off: usize,
    /// 历史最深水位。
    pub hi_water: usize,
    /// 本槽溢出次数。
    pub overruns: u32,
}

impl IstSlot {
    /// 造槽：界标 = 基字 ^ 槽号。
    pub fn new(no: u8) -> Result<Self, u32> {
        if no < IST_LO || no > IST_HI {
            return Err(XC_SLOT_OOR);
        }
        let g = SENT_BASE ^ (no as u64);
        Ok(IstSlot { no, guard_want: g, guard_read: g, sp_off: STACK_SZ, hi_water: STACK_SZ, overruns: 0 })
    }

    /// 压栈 n 字节：sp 下移，过界标（sp_off < 16：界标占 16 字节）
    /// 即溢出；对齐核。
    pub fn push(&mut self, bytes: usize) -> Result<usize, u32> {
        if bytes % 16 != 0 {
            return Err(XC_MISALIGN);
        }
        let new_off = self.sp_off.saturating_sub(bytes);
        if new_off < 16 {
            // 会踩界标：拦下（sp 不动）。
            self.overruns += 1;
            return Err(XC_STACK_OVERRUN);
        }
        self.sp_off = new_off;
        if new_off < self.hi_water {
            self.hi_water = new_off;
        }
        Ok(self.sp_off)
    }

    /// 出栈 n 字节。
    pub fn pop(&mut self, bytes: usize) -> Result<usize, u32> {
        if bytes % 16 != 0 {
            return Err(XC_MISALIGN);
        }
        self.sp_off = (self.sp_off + bytes).min(STACK_SZ);
        Ok(self.sp_off)
    }

    /// 退役验尸：界标回读核。
    pub fn audit(&self) -> Result<(), u32> {
        if self.guard_read != self.guard_want {
            return Err(XC_SENTINEL_TRAMPLED);
        }
        Ok(())
    }

    /// 注入界标踩踏（演练）。
    pub fn trample(&mut self) {
        self.guard_read = self.guard_read.rotate_left(1) | 1;
    }

    /// 惯犯核：本槽溢出 ≥2 升级。
    pub fn habitual(&self) -> Result<(), u32> {
        if self.overruns >= 2 {
            return Err(XC_HABITUAL);
        }
        Ok(())
    }
}

/// 七槽哨兵表。
pub struct IstTable {
    /// 槽数组（下标 0 对应槽 1）。
    pub slots: [IstSlot; 7],
}

impl IstTable {
    pub fn new() -> Self {
        IstTable {
            slots: [
                IstSlot::new(1).unwrap_or_else(|_| IstSlot::new(1).unwrap_or(IstSlot { no: 1, guard_want: 0, guard_read: 0, sp_off: STACK_SZ, hi_water: STACK_SZ, overruns: 0 })),
                IstSlot::unwrap_or_default_stub(2),
                IstSlot::unwrap_or_default_stub(3),
                IstSlot::unwrap_or_default_stub(4),
                IstSlot::unwrap_or_default_stub(5),
                IstSlot::unwrap_or_default_stub(6),
                IstSlot::unwrap_or_default_stub(7),
            ],
        }
    }

    /// 槽访问（1~7）。
    pub fn slot(&mut self, no: u8) -> Result<&mut IstSlot, u32> {
        if no < IST_LO || no > IST_HI {
            return Err(XC_SLOT_OOR);
        }
        Ok(&mut self.slots[(no - 1) as usize])
    }

    /// 全表验尸：任一槽界标破即立案。
    pub fn audit_all(&self) -> Result<(), u32> {
        for s in self.slots.iter() {
            s.audit()?;
        }
        Ok(())
    }

    /// 最深水位槽（sp_off 最小者）。
    pub fn deepest(&self) -> (u8, usize) {
        let mut best = (1u8, usize::MAX);
        for s in self.slots.iter() {
            if s.sp_off < best.1 {
                best = (s.no, s.sp_off);
            }
        }
        best
    }
}

impl IstSlot {
    /// 槽号直造兜底（new 恒成功，此为防御式冗余）。
    fn unwrap_or_default_stub(no: u8) -> Self {
        let g = SENT_BASE ^ (no as u64);
        IstSlot { no, guard_want: g, guard_read: g, sp_off: STACK_SZ, hi_water: STACK_SZ, overruns: 0 }
    }
}

/// 判据入口：UNX-F0435。
pub fn run_unx0435_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0435_istsent");
    // 族一：槽域核。
    cs.add("槽 0 拒", IstSlot::new(0).is_err(), "1~7 域外");
    cs.add("槽 8 拒", IstSlot::new(8).is_err(), "域外");
    cs.add("槽 3 过", IstSlot::new(3).is_ok(), "域内");
    // 族二：界标变体。
    let s1 = IstSlot::new(1).ok();
    cs.add("槽 1 界标", s1.as_ref().map(|s| s.guard_want == SENT_BASE ^ 1).unwrap_or(false), "基字^1");
    cs.add("槽 7 界标", IstSlot::new(7).map(|s| s.guard_want == SENT_BASE ^ 7).unwrap_or(false), "基字^7");
    // 族三：压栈与水位。
    let mut s2 = IstSlot::new(2).ok().unwrap_or_else(|| IstSlot::unwrap_or_default_stub(2));
    cs.add("压 64 过", s2.push(64) == Ok(STACK_SZ - 64), "sp 下移");
    cs.add("水位记账", s2.hi_water == STACK_SZ - 64, "最深");
    let _ = s2.pop(64);
    cs.add("出栈回顶", s2.sp_off == STACK_SZ, "回顶");
    cs.add("水位留痕", s2.hi_water == STACK_SZ - 64, "历史不抹");
    // 族四：对齐核。
    cs.add("非 16 压拒", s2.push(8) == Err(XC_MISALIGN), "8 字节歪");
    cs.add("非 16 出拒", s2.pop(24) == Err(XC_MISALIGN), "24 歪");
    // 族五：溢出拦截。
    let mut s3 = IstSlot::new(3).ok().unwrap_or_else(|| IstSlot::unwrap_or_default_stub(3));
    let _ = s3.push(STACK_SZ - 16); // 到界标上沿
    cs.add("踩界标拒", s3.push(16) == Err(XC_STACK_OVERRUN), "sp 不许过");
    cs.add("溢出计 1", s3.overruns == 1, "账面");
    cs.add("单次不升级", s3.habitual().is_ok(), "初犯");
    // 族六：双溢出升级。
    cs.add("二次溢出拒", s3.push(32) == Err(XC_STACK_OVERRUN), "再犯");
    cs.add("惯犯升级", s3.habitual() == Err(XC_HABITUAL), "连续双溢出");
    // 族七：验尸。
    cs.add("界标完好", s2.audit().is_ok(), "退役核过");
    let mut s4 = IstSlot::new(4).ok().unwrap_or_else(|| IstSlot::unwrap_or_default_stub(4));
    s4.trample();
    cs.add("界标被踩立案", s4.audit() == Err(XC_SENTINEL_TRAMPLED), "验尸过");
    // 族八：表级。
    let mut t = IstTable::new();
    cs.add("七槽齐", t.audit_all().is_ok(), "全绿");
    cs.add("越槽访问拒", t.slot(0).is_err() && t.slot(8).is_err(), "域外");
    let _ = t.slot(5).map(|s| s.push(128));
    cs.add("最深槽 5", t.deepest() == (5, STACK_SZ - 128), "水位最深");
    // 族九：踩槽传染立案。
    let mut t2 = IstTable::new();
    t2.slots[6].trample();
    cs.add("表级验尸立案", t2.audit_all() == Err(XC_SENTINEL_TRAMPLED), "槽 7 破");
    // 族十：基址对齐。
    cs.add("基址 16 对齐", STACK_BASE % 16 == 0, "ABI");
    cs
}
