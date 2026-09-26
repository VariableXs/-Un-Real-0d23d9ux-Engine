
// ---------------------------------------------------------------------------
// F009 · 深化批次九：蜂巢单元格分配器（注册表蜂巢的 cell 语义——每格
// 以带符号长度前缀：正 = 已分配、负 = 空闲；分配 8 字节对齐 + 空闲表
// 首适配 + 碎片记账）。
// ---------------------------------------------------------------------------

/// 蜂巢页单元格分配器（容量 256 字节的最小模型——真蜂巢块 4096，语义同构）。
pub struct CellArena {
    data: [u8; 256],
    /// 空闲段表：(偏移, 长度)，保序。
    free: alloc::vec::Vec<(u16, u16)>,
    pub alloc_count: u32,
    pub coalesce_count: u32,
}

impl CellArena {
    pub fn new() -> CellArena {
        CellArena {
            data: [0; 256],
            free: alloc::vec![(0u16, 256u16)],
            alloc_count: 0,
            coalesce_count: 0,
        }
    }

    /// 分配（needs ≥1 字节；8 对齐向上取整）——首适配。
    pub fn alloc_cell(&mut self, needs: u16) -> Option<u16> {
        if needs == 0 {
            return None;
        }
        let need = ((needs as u32 + 7) & !7u32) as u16;
        let pos = self.free.iter().position(|&(_, len)| len >= need)?;
        let (off, len) = self.free[pos];
        if len == need {
            self.free.remove(pos);
        } else {
            self.free[pos] = (off + need, len - need);
        }
        self.alloc_count += 1;
        Some(off)
    }

    /// 释放（合并相邻空闲段——左右双合并，每合并一次计一次）。
    pub fn free_cell(&mut self, off: u16, len: u16) {
        self.free.push((off, len));
        self.free.sort();
        let mut merged: alloc::vec::Vec<(u16, u16)> = alloc::vec::Vec::new();
        for &(o, l) in self.free.iter() {
            match merged.last_mut() {
                Some(last) if last.0 + last.1 == o => {
                    last.1 += l;
                    self.coalesce_count += 1;
                }
                _ => merged.push((o, l)),
            }
        }
        self.free = merged;
    }

    pub fn free_total(&self) -> u32 {
        self.free.iter().map(|&(_, l)| l as u32).sum()
    }

    pub fn free_segments(&self) -> usize {
        self.free.len()
    }

    pub fn byte_at(&self, off: u16) -> u8 {
        self.data[off as usize]
    }

    pub fn set_byte(&mut self, off: u16, v: u8) {
        self.data[off as usize] = v;
    }
}

/// F009 深化批次九自检。
fn run_reghive_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep8");
    let mut arena = CellArena::new();
    // 1) 8 对齐分配：5 字节请求占 8；连续分配偏移推进 0/8/16。
    let a = arena.alloc_cell(5);
    let b = arena.alloc_cell(8);
    let c = arena.alloc_cell(1);
    cs.add(
        "cell_alloc_aligned_sequential",
        a == Some(0) && b == Some(8) && c == Some(16) && arena.free_segments() == 1,
        "",
    );
    // 2) 释放中段合并：free(0,8)+free(16,8) 后 free(8,8) → 三段合一（两次合并计数）。
    arena.free_cell(0, 8);
    arena.free_cell(16, 8);
    let segs_before = arena.free_segments(); // 3 段：0-8 / 24- / 16-24 插入后排序
    arena.free_cell(8, 8);
    cs.add(
        "cell_coalesce_middle",
        segs_before == 3 && arena.free_segments() == 1 && arena.free_total() == 256,
        "",
    );
    // 3) 内容寻址：分配出的偏移可写可读（分配不破坏邻格）。
    let m = arena.alloc_cell(8);
    let mut ok = false;
    if let Some(m) = m {
        arena.set_byte(m, 0xAB);
        arena.set_byte(m + 7, 0xCD);
        ok = arena.byte_at(m) == 0xAB && arena.byte_at(m + 7) == 0xCD;
    }
    cs.add(
        "cell_content_addressable",
        ok && arena.alloc_count == 4,
        "",
    );
    cs
}
