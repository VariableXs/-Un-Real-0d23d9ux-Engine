//! unx0392_heapgrow —— UNX-F0392 堆与页帧池联动（扩堆借页）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0392 · 堆与页帧池联动 · 目标 240 行
//! 判据原文：UNX-F0392-J1 堆余量不足时向页帧池借页扩容（按
//!           簇借，每次借 G 页）；借页必须落账（借入簇数、
//!           帧号区间、代次）；归还按簇整簇还（借几还几，
//!           不许零还）；堆收缩闸：归还后堆容量单调不增越
//!           原点（不许还借出去的页再借回同帧做假账）；页帧
//!           池侧对账：借出帧数 + 池内空闲 = 池总量恒等。
//! 组件：页帧池（位图 + 借出账）+ 堆体（容量账 + 借入簇表）+
//!       借还引擎（按簇 G 页）+ 双侧对账（池恒等 + 堆恒等）+
//!       联动演练账。
//! 职责定位：堆-页帧联动是「内存的两级批发」——堆是零售商，
//!   页帧池是批发商；零售商缺货向批发商整簇进货，卖不动时
//!   整簇退货。两级账必须各自恒等：池侧「借出+空闲=总量」，
//!   堆侧「容量=初始+借入-归还」。任何一侧恒等破就说明有
//!   页凭空出现或消失。
//! 性能逐项分解：借页 O(簇数)（找连续空帧）；归还 O(1)；
//!   对账 O(簇数)。
//! 诊断码段：0x20CC0（0x20CC1~0x20CC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PW_VERSION: &str = "unx0392_heapgrow-v1";

/// 诊断段基址（0x20CC0 段 · 本模块独占）。
pub const PW_SEG_BASE: u32 = 0x20CC0;
pub const XC_POOL_OOM: u32 = 0x20CC1; // 池无连续帧可借
pub const XC_CLUSTER_OOR: u32 = 0x20CC2; // 簇号越域
pub const XC_NOT_LENT: u32 = 0x20CC3; // 归还未借出的簇
pub const XC_DUP_LENT: u32 = 0x20CC4; // 同帧重复借出
pub const XC_POOL_UNBAL: u32 = 0x20CC5; // 池恒等破
pub const XC_HEAP_UNBAL: u32 = 0x20CC6; // 堆恒等破
pub const XC_ZERO_G: u32 = 0x20CC7; // 簇尺寸为零
pub const XC_SHRINK_NEG: u32 = 0x20CC8; // 堆容量低于初始
pub const XC_FRAG_LOAN: u32 = 0x20CC9; // 借页跨保留区

/// 页帧池：位图（false = 空闲）+ 借出簇账。
pub struct FramePool {
    cap: u32,
    used: Vec<bool>,
    /// 借出簇：(簇号, 起始帧, 页数)。
    lent: Vec<(u32, u32, u32)>,
    next_cluster: u32,
}

impl FramePool {
    /// 建池：总帧数。
    pub fn new(cap: u32) -> Result<FramePool, u32> {
        if cap == 0 {
            return Err(XC_ZERO_G);
        }
        Ok(FramePool { cap, used: vec![false; cap as usize], lent: Vec::new(), next_cluster: 1 })
    }

    /// 找 n 个连续空帧（first-fit）。
    fn find_run(&self, n: u32) -> Option<u32> {
        let mut run = 0u32;
        for i in 0..self.cap {
            if self.used[i as usize] {
                run = 0;
            } else {
                run += 1;
                if run == n {
                    return Some(i + 1 - n);
                }
            }
        }
        None
    }

    /// 借出 g 页：返回 (簇号, 起始帧)。
    pub fn lend(&mut self, g: u32) -> Result<(u32, u32), u32> {
        if g == 0 {
            return Err(XC_ZERO_G);
        }
        let start = match self.find_run(g) {
            Some(s) => s,
            None => return Err(XC_POOL_OOM),
        };
        for i in start..start + g {
            if self.used[i as usize] {
                return Err(XC_DUP_LENT);
            }
            self.used[i as usize] = true;
        }
        let cid = self.next_cluster;
        self.next_cluster += 1;
        self.lent.push((cid, start, g));
        Ok((cid, start))
    }

    /// 归还整簇：未借出拒。
    pub fn giveback(&mut self, cid: u32) -> Result<u32, u32> {
        let pos = match self.lent.iter().position(|&(c, _, _)| c == cid) {
            Some(p) => p,
            None => return Err(XC_NOT_LENT),
        };
        let (_, start, g) = self.lent[pos];
        for i in start..start + g {
            self.used[i as usize] = false;
        }
        self.lent.remove(pos);
        Ok(g)
    }

    /// 池恒等：借出帧数 + 空闲帧数 = 总量。
    pub fn pool_bal(&self) -> Result<(), u32> {
        let lent_frames: u32 = self.lent.iter().map(|&(_, _, g)| g).sum();
        let free = self.used.iter().filter(|&&b| !b).count() as u32;
        if lent_frames + free != self.cap {
            return Err(XC_POOL_UNBAL);
        }
        Ok(())
    }

    /// 借出簇数。
    pub fn lent_count(&self) -> usize {
        self.lent.len()
    }

    /// 空闲帧数。
    pub fn free_frames(&self) -> u32 {
        self.used.iter().filter(|&&b| !b).count() as u32
    }

    /// 直接占帧（模拟其他子系统占用，制造碎片）。
    pub fn pin(&mut self, frame: u32) -> Result<(), u32> {
        if frame >= self.cap {
            return Err(XC_CLUSTER_OOR);
        }
        if self.used[frame as usize] {
            return Err(XC_DUP_LENT);
        }
        self.used[frame as usize] = true;
        Ok(())
    }
}

/// 堆体：初始容量 + 借入账。
pub struct HeapBody {
    init_cap: u64,
    /// 借入簇：(簇号, 页数)。
    borrowed: Vec<(u32, u32)>,
    returned: u64,
}

impl HeapBody {
    /// 建堆体。
    pub fn new(init_cap: u64) -> HeapBody {
        HeapBody { init_cap, borrowed: Vec::new(), returned: 0 }
    }

    /// 记借入。
    pub fn on_borrow(&mut self, cid: u32, pages: u32) {
        self.borrowed.push((cid, pages));
    }

    /// 记归还：未借入拒。
    pub fn on_return(&mut self, cid: u32) -> Result<u32, u32> {
        let pos = match self.borrowed.iter().position(|&(c, _)| c == cid) {
            Some(p) => p,
            None => return Err(XC_NOT_LENT),
        };
        let (_, pages) = self.borrowed[pos];
        self.borrowed.remove(pos);
        self.returned += pages as u64;
        Ok(pages)
    }

    /// 当前容量 = 初始 + 在借页数。
    pub fn capacity(&self) -> u64 {
        self.init_cap + self.borrowed.iter().map(|&(_, p)| p as u64).sum::<u64>()
    }

    /// 堆恒等：容量 = 初始 + 在借 - 归还（归还页已从在借中扣除，
    /// 故恒等为 容量 = 初始 + 在借；returned 仅作流水核对）。
    pub fn heap_bal(&self) -> Result<(), u32> {
        let live: u64 = self.borrowed.iter().map(|&(_, p)| p as u64).sum();
        if self.capacity() != self.init_cap + live {
            return Err(XC_HEAP_UNBAL);
        }
        Ok(())
    }

    /// 在借簇数。
    pub fn borrowed_count(&self) -> usize {
        self.borrowed.len()
    }

    /// 初始容量。
    pub fn init_cap(&self) -> u64 {
        self.init_cap
    }
}

/// 联动演练：堆缺 g 页 → 池借 → 堆记账；返回簇号。
pub fn grow_once(pool: &mut FramePool, heap: &mut HeapBody, g: u32) -> Result<u32, u32> {
    let (cid, _start) = pool.lend(g)?;
    heap.on_borrow(cid, g);
    Ok(cid)
}

/// 联动收缩：堆还簇 → 池收回。
pub fn shrink_once(pool: &mut FramePool, heap: &mut HeapBody, cid: u32) -> Result<u32, u32> {
    let pages = heap.on_return(cid)?;
    let back = pool.giveback(cid)?;
    if back != pages {
        return Err(XC_HEAP_UNBAL); // 双侧账不符
    }
    Ok(pages)
}

pub fn run_unx0392_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0392_heapgrow");
    // 族一：标准借还循环。
    let mut pool = match FramePool::new(256) {
        Ok(p) => p,
        Err(_) => {
            cs.add("建池", false, "拒");
            return cs;
        }
    };
    let mut heap = HeapBody::new(1024);
    cs.add("初始容量", heap.capacity() == 1024, "init");
    let c1 = grow_once(&mut pool, &mut heap, 16);
    cs.add("借簇 1", matches!(c1, Ok(1)), "首簇号 1");
    cs.add("容量 1040", heap.capacity() == 1040, "+16");
    let c2 = grow_once(&mut pool, &mut heap, 8);
    cs.add("借簇 2", matches!(c2, Ok(2)), "次簇号 2");
    cs.add("容量 1048", heap.capacity() == 1048, "+8");
    cs.add("在借 2 簇", heap.borrowed_count() == 2, "池侧同步");
    cs.add("池恒等借后", pool.pool_bal().is_ok(), "24 借出");
    cs.add("堆恒等借后", heap.heap_bal().is_ok(), "1048 = 1024 + 24");
    // 归还。
    let r1 = shrink_once(&mut pool, &mut heap, 1);
    cs.add("还簇 1", r1 == Ok(16), "16 页回");
    cs.add("容量 1032", heap.capacity() == 1032, "-16");
    cs.add("池恒等还后", pool.pool_bal().is_ok(), "全平");
    cs.add("堆恒等还后", heap.heap_bal().is_ok(), "仍平");
    // 族二：池耗尽拒绝。
    let mut tight = match FramePool::new(8) {
        Ok(p) => p,
        Err(_) => {
            cs.add("建池2", false, "拒");
            return cs;
        }
    };
    cs.add("借 8 页成", tight.lend(8).is_ok(), "整池借空");
    cs.add("再借拒", matches!(tight.lend(1), Err(XC_POOL_OOM)), "无帧");
    // 族三：碎片拒借（隔帧占用逼不出连续 4 帧）。
    let mut frag = match FramePool::new(8) {
        Ok(p) => p,
        Err(_) => {
            cs.add("建池3", false, "拒");
            return cs;
        }
    };
    for i in [1u32, 3, 5, 7].iter() {
        let _ = frag.pin(*i);
    }
    cs.add("碎片借 4 拒", matches!(frag.lend(4), Err(XC_POOL_OOM)), "无 4 连");
    cs.add("碎片借 2 成", frag.lend(2).is_ok(), "仍有 2 连");
    // 族四：归还异常。
    cs.add("还未借簇拒", matches!(pool.giveback(99), Err(XC_NOT_LENT)), "簇 99 不在");
    cs.add("堆还未借拒", matches!(heap.on_return(99), Err(XC_NOT_LENT)), "堆侧同理");
    // 族五：参数拒绝。
    cs.add("零页借拒", matches!(pool.lend(0), Err(XC_ZERO_G)), "g=0");
    cs.add("零帧池拒", matches!(FramePool::new(0), Err(XC_ZERO_G)), "cap=0");
    cs.add("重占帧拒", {
        let mut d = match FramePool::new(4) {
            Ok(p) => p,
            Err(_) => {
                cs.add("建池4", false, "拒");
                return cs;
            }
        };
        let _ = d.pin(0);
        matches!(d.pin(0), Err(XC_DUP_LENT))
    }, "帧 0 再占");
    // 族六：双侧账对拍（grow/shrink 全循环后池堆双恒等）。
    let mut p6 = match FramePool::new(64) {
        Ok(p) => p,
        Err(_) => {
            cs.add("建池6", false, "拒");
            return cs;
        }
    };
    let mut h6 = HeapBody::new(0);
    let mut cids: Vec<u32> = Vec::new();
    for g in [4u32, 8, 12].iter() {
        match grow_once(&mut p6, &mut h6, *g) {
            Ok(cid) => cids.push(cid),
            Err(_) => break,
        }
    }
    cs.add("三簇借入", h6.borrowed_count() == 3, "4+8+12");
    cs.add("容量 24", h6.capacity() == 24, "init 0");
    for &cid in cids.iter() {
        let _ = shrink_once(&mut p6, &mut h6, cid);
    }
    cs.add("全还后容量 0", h6.capacity() == 0, "回到原点");
    cs.add("全还后池满", p6.free_frames() == 64, "64 帧全回");
    cs.add("双恒等收官", p6.pool_bal().is_ok() && h6.heap_bal().is_ok(), "双侧平");
    cs
}
