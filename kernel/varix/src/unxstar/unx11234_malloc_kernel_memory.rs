//! UNX-F11234 glibc malloc 与内核内存面消费（0x1f834 段）
//!
//! 锚点：UNX-F11234-J1 malloc 堆扩策略（brk 优先/mmap 阈值切换）消费判据绿，
//! M_MMAP_THRESHOLD 行为对齐。
//! 域：CoRun·UNX-C5-B02——小于阈值的块走 brk，达到阈值走 mmap；释放大块会
//! 抬高动态阈值（上限三十二兆），但绝不降到一百二十八 KB 以下。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f834;
pub const MM_ARG: u32 = SEG_BASE + 0x01; // 请求为零或超上限
pub const MM_MODE: u32 = SEG_BASE + 0x02; // 途径非 brk/mmap
pub const MM_THRESH: u32 = SEG_BASE + 0x03; // 阈值越界
pub const MM_BRK: u32 = SEG_BASE + 0x04; // brk 下界回退
pub const MM_MMAP: u32 = SEG_BASE + 0x05; // mmap 账不符
pub const MM_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const MM_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 阈值三档（字节）。
pub const THRESH_MIN: u64 = 128 * 1024;
pub const THRESH_MAX: u64 = 32 * 1024 * 1024;
pub const MAX_REQUEST: u64 = 256 * 1024 * 1024;
pub const HEAP_BASE: u64 = 0x55e0_0000_0000;
pub const MAX_ALLOCS: usize = 24;
pub const MM_TAG: u64 = 0x1f834f5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一次分配。
#[derive(Clone, Debug)]
pub struct Alloc {
    pub size: u64,
    pub via: &'static str,
    pub addr: u64,
}

/// malloc 内核消费模型。
#[derive(Clone, Debug)]
pub struct MallocModel {
    pub threshold: u64,
    pub brk_top: u64,
    pub brk_bytes: u64,
    pub mmap_count: u32,
    pub allocs: Vec<Alloc>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl MallocModel {
    pub fn new() -> MallocModel {
        MallocModel {
            threshold: THRESH_MIN,
            brk_top: HEAP_BASE,
            brk_bytes: 0,
            mmap_count: 0,
            allocs: Vec::new(),
            frozen: false,
            root: None,
        }
    }

    /// 分配：零/超上限拒；按阈值选途径。
    pub fn alloc(&mut self, size: u64) -> Result<&'static str, u32> {
        if self.frozen {
            return Err(MM_FROZEN);
        }
        if size == 0 || size > MAX_REQUEST {
            return Err(MM_ARG);
        }
        if self.allocs.len() >= MAX_ALLOCS {
            return Err(MM_OVER);
        }
        let via = if size >= self.threshold { "mmap" } else { "brk" };
        let addr = if via == "brk" {
            let a = self.brk_top;
            self.brk_top = self.brk_top.saturating_add((size + 15) & !15);
            self.brk_bytes = self.brk_bytes.saturating_add((size + 15) & !15);
            a
        } else {
            self.mmap_count = self.mmap_count.saturating_add(1);
            0x7f50_0000_0000u64.saturating_add((self.mmap_count as u64) * 0x10_0000)
        };
        self.allocs.push(Alloc { size, via, addr });
        Ok(via)
    }

    /// 释放大块：动态阈值抬到两倍尺寸（夹在上下限之间）。
    pub fn release_big(&mut self, size: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(MM_FROZEN);
        }
        if size < THRESH_MIN || size > MAX_REQUEST {
            return Err(MM_ARG);
        }
        let want = size.saturating_mul(2);
        self.threshold = if want > THRESH_MAX { THRESH_MAX } else if want < THRESH_MIN { THRESH_MIN } else { want };
        Ok(self.threshold)
    }

    /// 收缩堆：不得低于起点。
    pub fn trim(&mut self, bytes: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(MM_FROZEN);
        }
        if bytes > self.brk_bytes {
            return Err(MM_BRK);
        }
        self.brk_bytes -= bytes;
        self.brk_top = self.brk_top.saturating_sub(bytes);
        Ok(self.brk_top)
    }

    pub fn by(&self, via: &str) -> usize {
        self.allocs.iter().filter(|a| a.via == via).count()
    }

    pub fn all_kept(&self) -> bool {
        self.allocs.iter().all(|a| a.size >= 1 && a.size <= MAX_REQUEST)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(MM_FROZEN);
        }
        if self.threshold < THRESH_MIN || self.threshold > THRESH_MAX {
            return Err(MM_THRESH);
        }
        if self.brk_top < HEAP_BASE {
            return Err(MM_BRK);
        }
        if self.mmap_count != self.by("mmap") as u32 {
            return Err(MM_MMAP);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("malloc thr {} brk {} mmap {}", self.threshold, self.by("brk"), self.by("mmap"))
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = MM_TAG;
        h = (h ^ self.threshold).wrapping_mul(FP_STEP);
        h = (h ^ self.brk_top).wrapping_mul(FP_STEP);
        h = (h ^ self.brk_bytes).wrapping_mul(FP_STEP);
        h = (h ^ self.mmap_count as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.allocs.len() {
            h = (h ^ self.allocs[k].size).wrapping_mul(FP_STEP);
            let b = self.allocs[k].via.as_bytes();
            let mut i = 0usize;
            while i < b.len() {
                h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            k += 1;
        }
        h
    }
}

/// 组一张十小三大、释放抬阈、再收缩一页的账。
pub fn build() -> MallocModel {
    let mut m = MallocModel::new();
    let mut k = 0usize;
    while k < 10 {
        let _ = m.alloc(4096);
        k += 1;
    }
    k = 0;
    while k < 3 {
        let _ = m.alloc(THRESH_MIN * 2);
        k += 1;
    }
    let _ = m.release_big(THRESH_MIN * 2);
    let _ = m.trim(4096);
    m
}

/// 四路对抗：零请求、超上限、收缩穿底、冻后分配。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = MallocModel::new();
    out[0] = a.alloc(0) == Err(MM_ARG);
    out[1] = a.alloc(MAX_REQUEST + 1) == Err(MM_ARG);
    out[2] = a.trim(1) == Err(MM_BRK);
    let mut b = build();
    let _ = b.freeze();
    out[3] = b.alloc(16) == Err(MM_FROZEN);
    out
}

/// 判据入口。
pub fn run_unx11234_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11234");
    let mut m = build();
    let root = m.freeze().unwrap_or(0);

    s.add("J1-小块十次", m.by("brk") == 10, "十次 4KiB 分配全部走 brk");
    s.add("J2-大块三次", m.by("mmap") == 3, "三次 256KiB 分配全部走 mmap");
    s.add("J3-分配落账", m.allocs.len() == 13, "十三次分配动作逐条落账");
    s.add("J4-brk 增长", m.brk_bytes == 10 * 4096 - 4096, "brk 用量为十页减一页");
    s.add("J5-堆顶对齐", m.brk_top % 16 == 0, "brk 顶保持十六字节对齐");
    s.add("J6-动态阈值", m.threshold == 524_288, "释放 256KiB 大块后阈值抬到 512KiB");
    s.add("J7-阈值上限", {
        let mut x = MallocModel::new();
        x.release_big(MAX_REQUEST).unwrap_or(0) == THRESH_MAX
    }, "释放超大块后阈值夹到三十二兆");
    s.add("J8-阈值两倍", {
        let mut x = MallocModel::new();
        x.release_big(THRESH_MIN).unwrap_or(0) == THRESH_MIN * 2
    }, "释放阈值下限大小的块后阈值恰为两倍下限");
    s.add("J9-请求有界", m.all_kept(), "全部请求落在一字节到上限之间");
    s.add("J10-冻存根", root != 0 && m.root == Some(root), "内存账冻存产出非零根");

    s.add("J11-冻后分配拒", m.alloc(16) == Err(MM_FROZEN), "冻存后分配拒为 MM_FROZEN");
    s.add("J12-零请求拒", {
        let mut x = MallocModel::new();
        x.alloc(0) == Err(MM_ARG)
    }, "零字节请求拒为 MM_ARG");
    s.add("J13-收缩穿底拒", {
        let mut x = MallocModel::new();
        x.trim(1) == Err(MM_BRK)
    }, "空堆收缩拒为 MM_BRK");

    let four = inject_four();
    s.add("J14-四路注入", four.iter().all(|x| *x), "零请求/超上限/穿底/冻后分配四路全拒");
    s.add_dyn(
        "J15-双途径明细",
        m.by("brk") == 10 && m.by("mmap") == 3,
        &format!("brk {} 次 / mmap {} 次，当前阈值 {}，mmap 计数 {}", m.by("brk"), m.by("mmap"), m.threshold, m.mmap_count),
    );
    s.add_dyn(
        "J16-账根",
        root == m.fold(),
        &format!("根 0x{:x}，堆顶 0x{:x}", root, m.brk_top),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11234_malloc_memory_flow() {
        let mut m = build();
        assert_eq!(m.by("brk"), 10);
        assert_eq!(m.by("mmap"), 3);
        assert_eq!(m.threshold, 524_288);
        let r = m.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(m.alloc(16), Err(MM_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11234_checks_green() {
        let s = run_unx11234_checks();
        assert!(s.all_passed(), "UNX-F11234 判据未全绿");
    }
}
