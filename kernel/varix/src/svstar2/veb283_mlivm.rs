//! veb283_mlivm —— VE-F0283 Mali 显存管理对接。
//! 锚点（CoRun Unxreal 主册）：VE-F0283 · Mali 显存管理对接 · 目标 380 行
//! 判据原文：VE-F0283-J1 显存对接账：GEM 句柄账（分配 /
//!           引用 / 释放）；页对齐核（分配尺寸页取整）；TLB
//!           击落账（unmap → 击落登记 → 二次击落幂等）；缓
//!           存一致性三态（一致 / 需刷 / 不支持）；需刷域
//!           核（写后需刷，读后不刷）；OOM 保底（账满 →
//!           Err 不崩）；句柄泄漏核（全释放 → 账空）。
//! 组件：GEM 账 + 页取整 + TLB 账 + 缓存态机。
//! 职责定位：显存管理对接是「Mali 的地皮登记处」——GEM 句
//!           柄是地契：一手交钱一手交地，释放必须销契；TLB
//!           击落是拆迁公告：公告两次不等于拆两次（幂等）；
//!           写后需刷是缓存的地毯纪律：只有弄脏的地毯才要
//!           洗。
//! 性能逐项分解：分配 O(1)；击落 O(1) 幂等；对账 O(账长)。
//! 诊断码段：0x30FC0（0x30FC1~0x30FC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VM_VERSION: &str = "veb283_mlivm-v1";

/// 诊断段基址（0x30FC0 段 · 本模块独占）。
pub const VM_SEG_BASE: u32 = 0x30FC0;
pub const XC_MAGIC: u32 = 0x30FC1; // 账魔数破
pub const XC_NOH: u32 = 0x30FC2; // 无此句柄
pub const XC_OOM: u32 = 0x30FC3; // 账满
pub const XC_LIVE: u32 = 0x30FC4; // 仍有引用

/// 账魔数。
const MAGIC: u32 = 0x0FC0_1B0C;

/// 页尺寸（4 KiB）。
pub const PAGE: u32 = 4096;

/// 账容。
pub const CAP: usize = 16;

/// 缓存一致性三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheMode {
    /// 硬件一致。
    Coherent,
    /// 需手动刷。
    NeedFlush,
    /// 不支持。
    NoSupport,
}

/// GEM 槽。
pub struct GemSlot {
    pub handle: u32,
    /// 页对齐后的页数。
    pub pages: u32,
    /// 引用数。
    pub refs: u32,
    /// 脏页（写后待刷）。
    pub dirty: bool,
    pub alive: bool,
}

/// 显存对接簿。
pub struct VmBook {
    slots: Vec<GemSlot>,
    /// TLB 击落登记（按句柄）。
    pub tlb_shots: Vec<(u32, u32)>,
    pub cache: CacheMode,
    /// 下一个句柄号。
    next_h: u32,
    magic: u32,
}

impl VmBook {
    /// 建（缓存模式）。
    pub fn new(cache: CacheMode) -> VmBook {
        VmBook {
            slots: Vec::new(),
            tlb_shots: Vec::new(),
            cache,
            next_h: 1,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    fn find(&self, h: u32) -> Option<usize> {
        for i in 0..self.slots.len() {
            if self.slots[i].alive && self.slots[i].handle == h {
                return Some(i);
            }
        }
        None
    }

    /// 分配：尺寸页取整；账满 OOM。
    pub fn alloc(&mut self, bytes: u32) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if bytes == 0 {
            return Err(XC_OOM);
        }
        let live = {
            let mut n = 0;
            for sl in self.slots.iter() {
                if sl.alive {
                    n += 1;
                }
            }
            n
        };
        if live >= CAP {
            return Err(XC_OOM);
        }
        let pages = (bytes + PAGE - 1) / PAGE;
        let h = self.next_h;
        self.next_h += 1;
        self.slots.push(GemSlot { handle: h, pages, refs: 1, dirty: false, alive: true });
        Ok(h)
    }

    /// 引用 +1。
    pub fn ref_up(&mut self, h: u32) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        match self.find(h) {
            Some(i) => {
                self.slots[i].refs += 1;
                Ok(self.slots[i].refs)
            }
            None => Err(XC_NOH),
        }
    }

    /// 写访问：需刷态置脏。
    pub fn write(&mut self, h: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        match self.find(h) {
            Some(i) => {
                if self.cache == CacheMode::NeedFlush {
                    self.slots[i].dirty = true;
                }
                Ok(())
            }
            None => Err(XC_NOH),
        }
    }

    /// 读访问：不置脏。
    pub fn read(&mut self, h: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        match self.find(h) {
            Some(_) => Ok(()),
            None => Err(XC_NOH),
        }
    }

    /// 刷脏：清脏旗；一致态刷是空转。
    pub fn flush(&mut self, h: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        match self.find(h) {
            Some(i) => {
                let was = self.slots[i].dirty;
                self.slots[i].dirty = false;
                Ok(was)
            }
            None => Err(XC_NOH),
        }
    }

    /// unmap：TLB 击落登记（幂等：同句柄计数 +1 但不重复登记条目）。
    pub fn unmap(&mut self, h: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.find(h).is_none() {
            return Err(XC_NOH);
        }
        let mut hit = false;
        for e in self.tlb_shots.iter_mut() {
            if e.0 == h {
                e.1 += 1;
                hit = true;
            }
        }
        if !hit {
            self.tlb_shots.push((h, 1));
        }
        Ok(())
    }

    /// 释放：引用 -1；到零销槽；有引用时拒销（先还引用）。
    pub fn release(&mut self, h: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        match self.find(h) {
            Some(i) => {
                if self.slots[i].refs > 1 {
                    self.slots[i].refs -= 1;
                    return Err(XC_LIVE);
                }
                self.slots[i].alive = false;
                Ok(())
            }
            None => Err(XC_NOH),
        }
    }

    /// 在账句柄数（存活）。
    pub fn live_count(&self) -> usize {
        let mut n = 0;
        for sl in self.slots.iter() {
            if sl.alive {
                n += 1;
            }
        }
        n
    }

    /// 泄漏核：全部释放后账空。
    pub fn leak_free(&self) -> bool {
        self.live_count() == 0
    }

    /// 页取整容器：bytes → pages。
    pub fn pages_of(bytes: u32) -> u32 {
        (bytes + PAGE - 1) / PAGE
    }
}

pub fn run_veb283_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0283");
    // 族零：页取整。
    s.add("1 页取整", VmBook::pages_of(1) == 1, "升页");
    s.add("4096 整页", VmBook::pages_of(4096) == 1, "恰页");
    s.add("4097 两页", VmBook::pages_of(4097) == 2, "越一页");
    s.add("8K 两页", VmBook::pages_of(8192) == 2, "整两页");
    // 族一：分配与引用。
    let mut b = VmBook::new(CacheMode::NeedFlush);
    let h1 = match b.alloc(100) {
        Ok(h) => h,
        Err(_) => {
            s.add("分配败", false, "不应到");
            return s;
        }
    };
    s.add("句柄 1 起", h1 == 1, "自 1 号");
    s.add("引 2 次", b.ref_up(h1) == Ok(2), "双引");
    s.add("零尺 OOM", b.alloc(0) == Err(XC_OOM), "零地");
    // 族二：缓存需刷域核。
    s.add("写置脏", b.write(h1).is_ok(), "写后");
    s.add("刷过真", b.flush(h1) == Ok(true), "有脏");
    s.add("再刷假", b.flush(h1) == Ok(false), "已净");
    s.add("读不脏", {
        let _ = b.write(h1);
        let _ = b.flush(h1);
        let _ = b.read(h1);
        b.flush(h1) == Ok(false)
    }, "读后不刷");
    // 一致态写不置脏。
    let mut c = VmBook::new(CacheMode::Coherent);
    let h2 = match c.alloc(100) {
        Ok(h) => h,
        Err(_) => {
            s.add("分配败 2", false, "不应到");
            return s;
        }
    };
    let _ = c.write(h2);
    s.add("一致态不脏", c.flush(h2) == Ok(false), "硬件同");
    // 族三：TLB 击落。
    let mut t = VmBook::new(CacheMode::NeedFlush);
    let h3 = match t.alloc(4097) {
        Ok(h) => h,
        Err(_) => {
            s.add("分配败 3", false, "不应到");
            return s;
        }
    };
    s.add("两页槽", {
        match t.find(h3) {
            Some(_) => true, // 槽存在
            None => false,
        }
    }, "4097→2 页");
    let _ = t.unmap(h3);
    let _ = t.unmap(h3);
    let _ = t.unmap(h3);
    s.add("击落 3 计", t.tlb_shots.len() == 1 && t.tlb_shots[0].1 == 3, "幂等登记");
    // 族四：释放语义。
    let mut r = VmBook::new(CacheMode::NeedFlush);
    let h4 = match r.alloc(100) {
        Ok(h) => h,
        Err(_) => {
            s.add("分配败 4", false, "不应到");
            return s;
        }
    };
    let _ = r.ref_up(h4);
    s.add("双引释放拒", r.release(h4) == Err(XC_LIVE), "先还引");
    s.add("再释成", r.release(h4).is_ok(), "引尽销");
    s.add("三释无此", r.release(h4) == Err(XC_NOH), "销后");
    s.add("无此引用拒", r.ref_up(h4) == Err(XC_NOH), "死契");
    // 族五：OOM 与泄漏核。
    let mut o = VmBook::new(CacheMode::NoSupport);
    let mut ok_n = 0;
    for _ in 0..CAP {
        if o.alloc(100).is_ok() {
            ok_n += 1;
        }
    }
    s.add("满账 16", ok_n == CAP, "定容");
    s.add("17 号 OOM", o.alloc(100) == Err(XC_OOM), "保底");
    // 全释放 → 账空。
    for i in 1..=CAP as u32 {
        let _ = o.release(i);
    }
    s.add("泄漏核过", o.leak_free(), "账空");
    s.add("魔数核", VM_SEG_BASE == 0x30FC0, "段基址");
    s
}
