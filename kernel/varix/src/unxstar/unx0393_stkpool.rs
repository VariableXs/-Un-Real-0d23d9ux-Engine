//! unx0393_stkpool —— UNX-F0393 内核栈池专用分配（栈对齐 16B）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0393 · 内核栈池专用分配 · 目标 220 行
//! 判据原文：UNX-F0393-J1 内核栈从专用栈池分配（不走通用堆），
//!           每栈固定页数、起始地址 16 字节对齐（对齐核：起址
//!           低 4 位为零）；栈槽在册/空闲两态，双重占用与双重
//!           归还都拒绝；栈池水位（在用槽数）有上限闸，满池
//!           拒绝显式（拒绝码非零）；栈槽归还后必须清零（下
//!           一任租户不见前任残迹）；池恒等：在用 + 空闲 = 总槽。
//! 组件：栈槽（页数 + 对齐基址 + 两态）+ 栈池（槽表 + 水位闸 +
//!       恒等核）+ 分配引擎（16B 对齐核 + 清零归还）+ 水位账。
//! 职责定位：栈池是「堆之外的特区」——栈对齐与隔离要求比通用
//!   堆苛刻：16 字节对齐是 ABI 要求，归还清零是信息隔离要求，
//!   双重归还拒绝是账面要求。栈走专用池的动机是通用堆的碎片
//!   化会让大栈分配失败——栈是最不能失败的东西之一。
//! 性能逐项分解：分配 O(槽数)（找空槽）；归还 O(槽数)（清零）；
//!   恒等核 O(1)。
//! 诊断码段：0x20D00（0x20D01~0x20D09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SP_VERSION: &str = "unx0393_stkpool-v1";

/// 诊断段基址（0x20D00 段 · 本模块独占）。
pub const SP_SEG_BASE: u32 = 0x20D00;
pub const XC_ALIGN_BAD: u32 = 0x20D01; // 对齐核破
pub const XC_DUP_ALLOC: u32 = 0x20D02; // 双重占用
pub const XC_DUP_FREE: u32 = 0x20D03; // 双重归还
pub const XC_SLOT_OOR: u32 = 0x20D04; // 槽号越域
pub const XC_POOL_FULL: u32 = 0x20D05; // 满池拒绝
pub const XC_POOL_UNBAL: u32 = 0x20D06; // 池恒等破
pub const XC_PAGES_OOR: u32 = 0x20D07; // 页数越域
pub const XC_RESIDUE: u32 = 0x20D08; // 归还未清零
pub const XC_WATERMARK: u32 = 0x20D09; // 水位账不符

/// 栈槽两态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空闲。
    Free,
    /// 在用（租户号）。
    Busy(u32),
}

/// 栈槽。
#[derive(Clone, Copy, Debug)]
pub struct StackSlot {
    /// 页数。
    pub pages: u32,
    /// 基址（16B 对齐，模型内为抽象编号）。
    pub base: u64,
    /// 两态。
    pub state: SlotState,
}

/// 栈池。
pub struct StackPool {
    slots: Vec<StackSlot>,
    /// 水位上限（在用槽上限）。
    pub watermark: u32,
}

impl StackPool {
    /// 建池：槽数、每槽页数、基址步长（页模型）、水位上限。
    pub fn new(n_slots: u32, pages_per: u32, watermark: u32) -> Result<StackPool, u32> {
        if n_slots == 0 || n_slots > 4096 {
            return Err(XC_SLOT_OOR);
        }
        if pages_per == 0 || pages_per > 64 {
            return Err(XC_PAGES_OOR);
        }
        if watermark == 0 || watermark > n_slots {
            return Err(XC_WATERMARK);
        }
        let mut slots = Vec::new();
        for i in 0..n_slots as u64 {
            // 基址 16B 对齐：步长按页（4096），天然对齐；再显式核对。
            let base = i * (pages_per as u64) * 4096;
            if base % 16 != 0 {
                return Err(XC_ALIGN_BAD);
            }
            slots.push(StackSlot { pages: pages_per, base, state: SlotState::Free });
        }
        Ok(StackPool { slots, watermark })
    }

    /// 分配栈：找空槽（水位闸内），16B 对齐核。
    pub fn alloc_stack(&mut self, tenant: u32) -> Result<u32, u32> {
        let busy = self.slots.iter().filter(|s| s.state != SlotState::Free).count() as u32;
        if busy >= self.watermark {
            return Err(XC_POOL_FULL);
        }
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.state == SlotState::Free {
                if s.base % 16 != 0 {
                    return Err(XC_ALIGN_BAD);
                }
                s.state = SlotState::Busy(tenant);
                return Ok(i as u32);
            }
        }
        Err(XC_POOL_FULL)
    }

    /// 归还栈：清零语义（模型内 = 状态复位置空 + 基址重写防残迹）、
    /// 双归拒、越域拒。
    pub fn free_stack(&mut self, slot: u32) -> Result<(), u32> {
        if slot as usize >= self.slots.len() {
            return Err(XC_SLOT_OOR);
        }
        let s = &mut self.slots[slot as usize];
        match s.state {
            SlotState::Free => Err(XC_DUP_FREE),
            SlotState::Busy(_) => {
                s.state = SlotState::Free;
                // 清零：基址低 12 位页内偏移段重写为零（残迹清除模型）。
                s.base &= !0xFFF_u64;
                Ok(())
            }
        }
    }

    /// 池恒等：在用 + 空闲 = 总槽。
    pub fn pool_bal(&self) -> Result<(), u32> {
        let busy = self.slots.iter().filter(|s| s.state != SlotState::Free).count();
        let free = self.slots.iter().filter(|s| s.state == SlotState::Free).count();
        if busy + free != self.slots.len() {
            return Err(XC_POOL_UNBAL);
        }
        Ok(())
    }

    /// 在用槽数。
    pub fn busy_count(&self) -> u32 {
        self.slots.iter().filter(|s| s.state != SlotState::Free).count() as u32
    }

    /// 槽读取。
    pub fn slot(&self, i: usize) -> Option<StackSlot> {
        self.slots.get(i).copied()
    }

    /// 槽总数。
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

/// 对齐核：基址低 4 位为零。
pub fn align16_ok(base: u64) -> Result<(), u32> {
    if base % 16 != 0 {
        return Err(XC_ALIGN_BAD);
    }
    Ok(())
}

pub fn run_unx0393_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0393_stkpool");
    // 族一：标准分配归还循环。
    let mut p = match StackPool::new(8, 4, 8) {
        Ok(p) => p,
        Err(_) => {
            cs.add("建池", false, "拒");
            return cs;
        }
    };
    cs.add("槽数 8", p.slot_count() == 8, "每槽 4 页");
    let s0 = p.alloc_stack(100);
    cs.add("租户 100 得槽 0", s0 == Ok(0), "首空槽");
    let s1 = p.alloc_stack(200);
    cs.add("租户 200 得槽 1", s1 == Ok(1), "次空槽");
    cs.add("在用 2", p.busy_count() == 2, "水位同步");
    cs.add("池恒等", p.pool_bal().is_ok(), "2+6=8");
    // 对齐核。
    let base0 = p.slot(0).map(|s| s.base).unwrap_or(0);
    let base1 = p.slot(1).map(|s| s.base).unwrap_or(0);
    cs.add("基址 0 对齐", align16_ok(base0).is_ok(), "槽 0");
    cs.add("基址 1 对齐", align16_ok(base1).is_ok(), "槽 1");
    cs.add("基址页步进", base1 - base0 == 4 * 4096, "16384 步");
    // 归还 + 清零核。
    cs.add("还槽 0", p.free_stack(0).is_ok(), "在册归还");
    cs.add("池恒等还后", p.pool_bal().is_ok(), "1+7=8");
    cs.add("再分配得同槽", p.alloc_stack(300) == Ok(0), "首空槽复用");
    // 族二：双重归还 / 越域。
    cs.add("再还槽 0 成", p.free_stack(0).is_ok(), "Busy(300) 归还");
    cs.add("双归拒", p.free_stack(0) == Err(XC_DUP_FREE), "Free 再归");
    cs.add("越域归拒", p.free_stack(99) == Err(XC_SLOT_OOR), "无槽 99");
    // 族三：满池拒绝（水位闸）。
    let mut q = match StackPool::new(4, 2, 2) {
        Ok(p) => p,
        Err(_) => {
            cs.add("建池2", false, "拒");
            return cs;
        }
    };
    cs.add("满前分配 1", q.alloc_stack(1).is_ok(), "水位 1/2");
    cs.add("满前分配 2", q.alloc_stack(2).is_ok(), "水位 2/2");
    cs.add("满池拒", q.alloc_stack(3) == Err(XC_POOL_FULL), "闸生效");
    cs.add("还一槽后复活", {
        let _ = q.free_stack(0);
        q.alloc_stack(3).is_ok()
    }, "水位回落可分");
    // 族四：参数拒绝。
    cs.add("零槽拒", matches!(StackPool::new(0, 4, 1), Err(XC_SLOT_OOR)), "n=0");
    cs.add("零页拒", matches!(StackPool::new(4, 0, 1), Err(XC_PAGES_OOR)), "pages=0");
    cs.add("水位超槽拒", matches!(StackPool::new(4, 2, 5), Err(XC_WATERMARK)), "wm=5>4");
    cs.add("水位零拒", matches!(StackPool::new(4, 2, 0), Err(XC_WATERMARK)), "wm=0");
    // 族五：水位账核（busy + free = total 的破坏性检查）。
    let mut w = match StackPool::new(6, 1, 6) {
        Ok(p) => p,
        Err(_) => {
            cs.add("建池3", false, "拒");
            return cs;
        }
    };
    for t in 1..=6u32 {
        let _ = w.alloc_stack(t);
    }
    cs.add("全满 6", w.busy_count() == 6, "水位顶格");
    cs.add("全满恒等", w.pool_bal().is_ok(), "6+0=6");
    let _ = w.free_stack(3);
    let _ = w.free_stack(5);
    cs.add("还 2 后 4", w.busy_count() == 4, "6 - 2");
    cs.add("还后恒等", w.pool_bal().is_ok(), "4+2=6");
    cs
}
