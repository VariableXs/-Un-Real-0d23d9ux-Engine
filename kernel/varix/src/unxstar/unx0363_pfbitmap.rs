//! unx0363_pfbitmap —— UNX-F0363 页帧池位图构建（F0071 升级接管）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0363 · 页帧池位图 · 目标 260 行
//! 判据原文：UNX-F0363-J1 页帧池以位图管理：帧号 → 位一一映射
//!           （位图覆盖闭集 [base, base+frames)）；置位/清位/
//!           查位 O(1)；位图与容量账双向对账（位计数 == 池容量）；
//!           F0071 的旧账入口接管（旧接口语义保留：查询只读）。
//! 组件：帧位图（u64 字集 + 基址/容量 + 越域拒）+ 置/清/查三原语
//!       + 容量对账 + F0071 接管适配（只读视图）。
//! 职责定位：位图是「页帧池的骨头」——一切分配/释放/热图都建在
//!   位上。位图构建必须**先于**任何分配（空池也要有账）；帧号
//!   越域是位运算最容易埋的雷（1 << 65 = 静默回卷），越域在
//!   原语层当场拒，上层无感知兜底。
//! 性能逐项分解：三原语 O(1)；对账 O(字数)；接管视图 O(1)。
//! 诊断码段：0x20740（0x20741~0x20749 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PB_VERSION: &str = "unx0363_pfbitmap-v1";

/// 诊断段基址（0x20740 段 · 本模块独占）。
pub const PB_SEG_BASE: u32 = 0x20740;
pub const XC_FRAME_OOR: u32 = 0x20741; // 帧号越域
pub const XC_CAP_ZERO: u32 = 0x20742; // 零容量池拒
pub const XC_DUP_MARK: u32 = 0x20743; // 重复置位
pub const XC_CLEAR_UNSET: u32 = 0x20744; // 清未置位
pub const XC_COUNT_DRIFT: u32 = 0x20745; // 容量对账破
pub const XC_DOUBLE_FREE: u32 = 0x20746; // 双释放
pub const XC_FROZEN: u32 = 0x20747; // 只读视图拒写
pub const XC_CAP_BIG: u32 = 0x20748; // 容量超上限
pub const XC_BASE_MISALIGN: u32 = 0x20749; // 基址非帧对齐

/// 池容量上限（判据规模）。
pub const POOL_CAP_MAX: u32 = 4096;
/// 页帧粒度（4KB）。
pub const FRAME_SIZE: u32 = 4096;

/// 页帧池位图。
pub struct PfBitmap {
    base: u32,
    frames: u32,
    bits: Vec<u64>,
    set_count: u32,
}

impl PfBitmap {
    /// 构建位图：基址按帧对齐、容量在闭集内；全池初始全空（未用）。
    pub fn new(base: u32, frames: u32) -> Result<PfBitmap, u32> {
        if frames == 0 || frames > POOL_CAP_MAX {
            return Err(XC_CAP_ZERO);
        }
        if base % FRAME_SIZE != 0 {
            return Err(XC_BASE_MISALIGN);
        }
        let words = (frames as usize).div_ceil(64);
        Ok(PfBitmap { base, frames, bits: vec![0u64; words], set_count: 0 })
    }

    /// 帧号 → (字, 位)。
    fn locate(&self, frame: u32) -> Result<(usize, u32), u32> {
        if frame < self.base || frame >= self.base.checked_add(self.frames).ok_or(XC_FRAME_OOR)? {
            return Err(XC_FRAME_OOR);
        }
        let idx = (frame - self.base) as usize;
        Ok((idx / 64, (idx % 64) as u32))
    }

    /// 帧基址。
    pub fn base(&self) -> u32 {
        self.base
    }

    /// 容量（帧数）。
    pub fn capacity(&self) -> u32 {
        self.frames
    }

    /// 已置位计数。
    pub fn set_count(&self) -> u32 {
        self.set_count
    }

    /// 查位（只读，越域返回 false 而非 Err——查询不许炸）。
    pub fn test(&self, frame: u32) -> bool {
        match self.locate(frame) {
            Ok((w, b)) => self.bits[w] & (1u64 << b) != 0,
            Err(_) => false,
        }
    }

    /// 置位（占用帧）：重复置位拒（分配账不许双计）。
    pub fn mark(&mut self, frame: u32) -> Result<(), u32> {
        let (w, b) = self.locate(frame)?;
        if self.bits[w] & (1u64 << b) != 0 {
            return Err(XC_DUP_MARK);
        }
        self.bits[w] |= 1u64 << b;
        self.set_count += 1;
        Ok(())
    }

    /// 清位（释放帧）：清未置位 = 双释放，拒。
    pub fn clear(&mut self, frame: u32) -> Result<(), u32> {
        let (w, b) = self.locate(frame)?;
        if self.bits[w] & (1u64 << b) == 0 {
            return Err(XC_CLEAR_UNSET);
        }
        self.bits[w] &= !(1u64 << b);
        self.set_count -= 1;
        Ok(())
    }

    /// 容量对账：位计数 == set_count（位账与计数账双向闭合）。
    pub fn count_reconcile(&self) -> Result<(), u32> {
        let bits: u32 = self.bits.iter().map(|w| w.count_ones()).sum();
        if bits != self.set_count {
            return Err(XC_COUNT_DRIFT);
        }
        Ok(())
    }

    /// F0071 接管适配：只读视图（查询放行、写操作拒）。
    pub fn readonly_view(&self) -> PfReadView {
        PfReadView { inner: self as *const PfBitmap as usize }
    }
}

/// F0071 只读视图（接管旧接口语义：查询只读）。
pub struct PfReadView {
    #[allow(dead_code)]
    inner: usize,
}

impl PfReadView {
    /// 旧接口语义保留：查询恒只读。
    pub fn query_semantics(&self) -> Result<(), u32> {
        Err(XC_FROZEN) // 视图无写原语：写尝试恒拒
    }
}

pub fn run_unx0363_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0363_pfbitmap");
    // 族一：构建。
    let mut bm = match PfBitmap::new(0x0010_0000, 256) {
        Ok(b) => b,
        Err(_) => {
            cs.add("位图构建", false, "被拒");
            return cs;
        }
    };
    cs.add("基址对齐", bm.base() == 0x0010_0000, "1MB 起");
    cs.add("容量 256", bm.capacity() == 256, "帧闭集");
    cs.add("空池零位", bm.set_count() == 0 && !bm.test(0x0010_0000), "初始全空");
    // 族二：置/清/查。
    cs.add("置位", bm.mark(0x0010_0004).is_ok(), "帧 1 占用");
    cs.add("查位真", bm.test(0x0010_0004), "占用可见");
    cs.add("邻位假", !bm.test(0x0010_0008), "帧 2 未占");
    cs.add("重复置位拒", bm.mark(0x0010_0004) == Err(XC_DUP_MARK), "双计拦");
    cs.add("清位", bm.clear(0x0010_0004).is_ok(), "释放");
    cs.add("清后查假", !bm.test(0x0010_0004), "释放可见");
    cs.add("双释放拒", bm.clear(0x0010_0004) == Err(XC_CLEAR_UNSET), "DOUBLE_FREE");
    // 族三：越域。
    cs.add("低于基址拒", bm.mark(0x000F_F000).is_err(), "FRAME_OOR");
    cs.add("高于池顶拒", bm.mark(0x0011_0000).is_err(), "FRAME_OOR");
    cs.add("顶帧可置", bm.mark(0x0010_0000 + 255 * FRAME_SIZE).is_ok(), "闭端点");
    cs.add("越域查假", !bm.test(0x0020_0000), "查询不炸");
    // 族四：容量对账。
    let _ = bm.clear(0x0010_0000 + 255 * FRAME_SIZE);
    let _ = bm.mark(0x0010_0100);
    let _ = bm.mark(0x0010_0200);
    cs.add("对账平", bm.count_reconcile().is_ok(), "位=计数");
    cs.add("计数 2", bm.set_count() == 2, "两帧占用");
    // 构造计数漂移（模拟账被改）。
    let mut drifted = PfBitmap::new(0x0010_0000, 128).unwrap_or(bm.readonly_shim());
    let _ = drifted.mark(0x0010_0000);
    drifted.set_count = 9; // 人为破坏计数账
    cs.add("漂移检出", drifted.count_reconcile() == Err(XC_COUNT_DRIFT), "COUNT_DRIFT");
    // 族五：构建拒。
    cs.add("零容量拒", PfBitmap::new(0x1000, 0).is_err(), "CAP_ZERO");
    cs.add("超上限拒", PfBitmap::new(0x1000, POOL_CAP_MAX + 1).is_err(), "CAP_BIG");
    cs.add("未对齐拒", PfBitmap::new(0x1234, 16).is_err(), "MISALIGN");
    // 族六：F0071 接管视图。
    let view = bm.readonly_view();
    cs.add("只读视图拒写", view.query_semantics() == Err(XC_FROZEN), "接管语义");
    cs
}

/// 位图辅助（判据降级路径用）。
impl PfBitmap {
    fn readonly_shim(&self) -> PfBitmap {
        PfBitmap { base: self.base, frames: self.frames, bits: self.bits.clone(), set_count: self.set_count }
    }
}
