//! mech_slab — slab 对象池分配器本体（AI-K1 深化批次五 · F052）。
//!
//! 主册依据：
//! - F052【设计细节】「堆碎片」——域内有分配谱（mech_buddy 管页阶），
//!   但**内核高频小对象**（inode、task_struct、dentry 同族）的 slab
//!   语义缺席：固定尺寸类、对象 freelist、整页 slab 的满/部分/空三态、
//!   对象复用（churn 不增长）、越界写金丝雀。本件按 SLAB 分配器核心
//!   语义实现：7 个尺寸类（kmalloc 同族）、每类定长对象池 + u16 freelist
//!   链、对象尾金丝雀（越界写检出）、申请/释放统计与高水位、耗尽诚实
//!   计数、全 churn 后零增长（碎片不随时间恶化的 slab 判据）。
//! - 锚点：F052「一周后依然秒级响应」——buddy 管页级碎片，slab 管
//!   对象级碎片，两者合成碎片判据的完整底盘。
//! - 零堆（arena 是定长字节数组）、零浮点。

// ---------------------------------------------------------------------------
// 1. 尺寸类与布局
// ---------------------------------------------------------------------------

/// 尺寸类表（字节；kmalloc-32..2048 同族）。
pub const SIZE_CLASSES: [usize; 7] = [32, 64, 128, 256, 512, 1024, 2048];
/// 每类池的对象数。
pub const OBJS_PER_CLASS: usize = 64;
/// 金丝雀值（对象尾 4 字节）。
const CANARY: u32 = 0xC0FF_EE01;
/// arena 总量（7 类 × 64 对象 × 类尺寸 + 对齐余量）。
pub const ARENA_BYTES: usize = 64 * (32 + 64 + 128 + 256 + 512 + 1024 + 2048);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlabErr {
    /// 请求尺寸超最大类。
    TooBig,
    /// 池满。
    Exhausted,
    /// 释放越界/未对齐。
    BadPtr,
    /// 双重释放。
    DoubleFree,
    /// 金丝雀被踩（越界写）。
    Corrupt,
}

// ---------------------------------------------------------------------------
// 2. 单类池
// ---------------------------------------------------------------------------

struct ClassPool {
    /// 对象存储（每对象 stride = 类尺寸 + 4 字节金丝雀）。
    mem: [u8; OBJS_PER_CLASS * (2048 + 4)],
    stride: usize,
    obj_size: usize,
    /// u16 freelist：值 = 槽号，OBJ_END = 链尾；OBJ_USED 表示已分配
    /// （used 数组语义拆开更清晰——freelist 只挂空闲槽）。
    free_head: u16,
    free_next: [u16; OBJS_PER_CLASS],
    used: [bool; OBJS_PER_CLASS],
    in_use: usize,
    peak: usize,
    exhausted: usize,
}

const OBJ_END: u16 = u16::MAX;

impl ClassPool {
    fn new(obj_size: usize) -> Self {
        let stride = obj_size + 4;
        let mut p = ClassPool {
            mem: [0; OBJS_PER_CLASS * (2048 + 4)],
            stride,
            obj_size,
            free_head: 0,
            free_next: [0; OBJS_PER_CLASS],
            used: [false; OBJS_PER_CLASS],
            in_use: 0,
            peak: 0,
            exhausted: 0,
        };
        // 初始 freelist：0→1→…→63→END。
        for i in 0..OBJS_PER_CLASS {
            p.free_next[i] = if i + 1 < OBJS_PER_CLASS { (i + 1) as u16 } else { OBJ_END };
        }
        p
    }

    fn slot_base(&self, slot: u16) -> usize {
        slot as usize * self.stride
    }

    fn write_canary(&mut self, slot: u16) {
        let base = self.slot_base(slot) + self.obj_size;
        self.mem[base..base + 4].copy_from_slice(&CANARY.to_le_bytes());
    }

    fn check_canary(&self, slot: u16) -> bool {
        let base = self.slot_base(slot) + self.obj_size;
        self.mem[base..base + 4] == CANARY.to_le_bytes()
    }

    /// 释放：off 必须是本池槽首（alloc_off 落账的本地偏移）。
    fn free(&mut self, off: usize) -> Result<(), SlabErr> {
        if off >= self.mem.len() || off % self.stride != 0 {
            return Err(SlabErr::BadPtr);
        }
        let slot = (off / self.stride) as u16;
        if !self.used[slot as usize] {
            return Err(SlabErr::DoubleFree);
        }
        // 金丝雀校验（free 是检出越界写的最佳时机——SLUB 同语义）。
        if !self.check_canary(slot) {
            // 仍要完成释放记账（内存不能漏），但错误如实上报。
            self.used[slot as usize] = false;
            self.free_next[slot as usize] = self.free_head;
            self.free_head = slot;
            self.in_use -= 1;
            return Err(SlabErr::Corrupt);
        }
        self.used[slot as usize] = false;
        self.free_next[slot as usize] = self.free_head;
        self.free_head = slot;
        self.in_use -= 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 3. 顶层分配器
// ---------------------------------------------------------------------------

/// slab 分配器（7 类）。
pub struct SlabAlloc {
    pools: [ClassPool; 7],
    pub allocs: u64,
    pub frees: u64,
    pub canary_faults: u64,
}

impl SlabAlloc {
    pub fn new() -> Self {
        SlabAlloc {
            pools: [
                ClassPool::new(SIZE_CLASSES[0]),
                ClassPool::new(SIZE_CLASSES[1]),
                ClassPool::new(SIZE_CLASSES[2]),
                ClassPool::new(SIZE_CLASSES[3]),
                ClassPool::new(SIZE_CLASSES[4]),
                ClassPool::new(SIZE_CLASSES[5]),
                ClassPool::new(SIZE_CLASSES[6]),
            ],
            allocs: 0,
            frees: 0,
            canary_faults: 0,
        }
    }

    /// 请求字节数 → 类号。
    pub fn class_of(size: usize) -> Option<usize> {
        SIZE_CLASSES.iter().position(|&s| size <= s)
    }

    /// 分配（偏移口径）：返回 (arena 偏移, 类号)。
    pub fn alloc_off(&mut self, size: usize) -> Result<(usize, usize), SlabErr> {
        let cls = Self::class_of(size).ok_or(SlabErr::TooBig)?;
        let pool = &mut self.pools[cls];
        if pool.free_head == OBJ_END {
            pool.exhausted += 1;
            return Err(SlabErr::Exhausted);
        }
        let slot = pool.free_head;
        pool.free_head = pool.free_next[slot as usize];
        pool.used[slot as usize] = true;
        pool.write_canary(slot);
        pool.in_use += 1;
        if pool.in_use > pool.peak {
            pool.peak = pool.in_use;
        }
        let base = pool.slot_base(slot);
        pool.mem[base..base + pool.obj_size].fill(0);
        self.allocs += 1;
        Ok((cls * OBJS_PER_CLASS * (2048 + 4) + base, cls))
    }

    /// 释放（偏移口径）。
    pub fn free_off(&mut self, off: usize) -> Result<(), SlabErr> {
        // 偏移 → 类：每类占 OBJS_PER_CLASS * (2048+4) 字节的逻辑分区，
        // 实际 mem 紧凑排布，用累计量反查。
        let mut acc = 0usize;
        for (cls, pool) in self.pools.iter_mut().enumerate() {
            let region = OBJS_PER_CLASS * (2048 + 4);
            if off < acc + region {
                let r = pool.free(off - acc);
                match r {
                    Ok(()) => self.frees += 1,
                    Err(SlabErr::Corrupt) => {
                        self.frees += 1;
                        self.canary_faults += 1;
                        // 金丝雀检出必须如实上报——记账完成≠错误消失
                        // （v1 在此漏 return，落进 Ok(()) 吞掉检出）。
                        return Err(SlabErr::Corrupt);
                    }
                    Err(e) => return Err(e),
                }
                return Ok(());
            }
            acc += region;
            let _ = cls;
        }
        Err(SlabErr::BadPtr)
    }

    /// 在对象内写/读（宿主与内核同构的字节口径）。
    pub fn write_obj(&mut self, off: usize, data: &[u8]) {
        let mut acc = 0usize;
        for pool in self.pools.iter_mut() {
            let region = OBJS_PER_CLASS * (2048 + 4);
            if off < acc + region {
                let local = off - acc;
                let end = (local + data.len()).min(pool.mem.len());
                pool.mem[local..end].copy_from_slice(&data[..end - local]);
                return;
            }
            acc += region;
        }
    }

    pub fn read_obj(&self, off: usize, len: usize) -> Option<&[u8]> {
        let mut acc = 0usize;
        for pool in self.pools.iter() {
            let region = OBJS_PER_CLASS * (2048 + 4);
            if off < acc + region {
                let local = off - acc;
                return Some(&pool.mem[local..local + len]);
            }
            acc += region;
        }
        None
    }

    /// 在使用对象数（按类）。
    pub fn in_use_of(&self, cls: usize) -> usize {
        self.pools[cls].in_use
    }

    pub fn peak_of(&self, cls: usize) -> usize {
        self.pools[cls].peak
    }

    pub fn exhausted_of(&self, cls: usize) -> usize {
        self.pools[cls].exhausted
    }

    /// 全体在用量。
    pub fn total_in_use(&self) -> usize {
        self.pools.iter().map(|p| p.in_use).sum()
    }
}

impl Default for SlabAlloc {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 4. CheckSet
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_slab");

    // 1) 往返完整性：写指纹→释放→再分配→对象体干净（kzalloc 语义）。
    {
        let mut s = SlabAlloc::new();
        let (o, _) = s.alloc_off(100).unwrap();
        s.write_obj(o, &[0xAB; 100]);
        s.free_off(o).unwrap();
        let (o2, _) = s.alloc_off(100).unwrap();
        let clean = s.read_obj(o2, 8).unwrap() == [0u8; 8];
        cs.add("slab_roundtrip_clean", o2 == o && clean, "");
    }

    // 2) 类隔离：64B 对象写满 64 字节不踩金丝雀；写第 65 字节也不波及
    //    邻居（金丝雀在自己 stride 内，free 时检出）。
    {
        let mut s = SlabAlloc::new();
        let (o, _) = s.alloc_off(64).unwrap();
        s.write_obj(o, &[0x5A; 64]); // 恰好写满对象体
        cs.add("slab_inbounds_ok", s.free_off(o).is_ok(), "");
        let mut s2 = SlabAlloc::new();
        let (o2, _) = s2.alloc_off(64).unwrap();
        s2.write_obj(o2, &[0x5A; 65]); // 越界 1 字节 → 踩金丝雀
        let r = s2.free_off(o2);
        cs.add("slab_overflow_caught", r == Err(SlabErr::Corrupt) && s2.canary_faults == 1, "");
    }

    // 3) 复用与峰值：64 次申请用满、释放一半、再申请立即复用不增长。
    {
        let mut s = SlabAlloc::new();
        let mut offs = [0usize; OBJS_PER_CLASS];
        for o in offs.iter_mut() {
            *o = s.alloc_off(128).unwrap().0;
        }
        let full = s.in_use_of(2) == OBJS_PER_CLASS;
        let over = s.alloc_off(128).is_err() && s.exhausted_of(2) == 1;
        for o in offs.iter().take(32) {
            s.free_off(*o).unwrap();
        }
        let after_free = s.in_use_of(2);
        let (reuse, _) = s.alloc_off(128).unwrap();
        let reused = s.in_use_of(2) == after_free + 1 && offs[..32].contains(&reuse);
        cs.add("slab_reuse_no_growth", full && over && reused, "");
    }

    // 4) 耗尽诚实：满池后再申请返回 Err 且计数（不静默、不越界写）。
    {
        let mut s = SlabAlloc::new();
        for _ in 0..OBJS_PER_CLASS {
            s.alloc_off(2048).unwrap();
        }
        let over = s.alloc_off(2048);
        cs.add("slab_exhaust_honest", over == Err(SlabErr::Exhausted) && s.exhausted_of(6) == 1, "");
    }

    // 5) 双重释放拒绝。
    {
        let mut s = SlabAlloc::new();
        let (o, _) = s.alloc_off(32).unwrap();
        s.free_off(o).unwrap();
        cs.add("slab_double_free", s.free_off(o) == Err(SlabErr::DoubleFree), "");
    }

    // 6) 尺寸类边界：33 字节落到 64 类；65→128；2048→2048；2049 拒绝。
    {
        let c = [
            SlabAlloc::class_of(33),
            SlabAlloc::class_of(65),
            SlabAlloc::class_of(2048),
            SlabAlloc::class_of(2049),
        ];
        cs.add(
            "slab_class_edges",
            c[0] == Some(1) && c[1] == Some(2) && c[2] == Some(6) && c[3].is_none(),
            "",
        );
    }

    cs
}

// ---------------------------------------------------------------------------
// 5. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_free_roundtrip_with_fingerprint() {
        let mut s = SlabAlloc::new();
        let (o, cls) = s.alloc_off(200).unwrap();
        assert_eq!(cls, 3); // 200 → 256 类（SIZE_CLASSES[3]=256；200>128 不入 128 类）
        s.write_obj(o, &(0..64u32).map(|i| i as u8).collect::<Vec<u8>>());
        let readback = s.read_obj(o, 64).unwrap().to_vec();
        assert_eq!(readback, (0..64u8).collect::<Vec<u8>>());
        s.free_off(o).unwrap();
        assert_eq!(s.total_in_use(), 0);
        assert_eq!(s.allocs, 1);
        assert_eq!(s.frees, 1);
    }

    #[test]
    fn canary_catches_off_by_one() {
        let mut s = SlabAlloc::new();
        let (o, _) = s.alloc_off(256).unwrap();
        // 恰好写满（合法）→ free 干净。
        s.write_obj(o, &[7u8; 256]);
        assert!(s.free_off(o).is_ok());
        // 写满 + 1（非法）→ free 报 Corrupt。
        let (o2, _) = s.alloc_off(256).unwrap();
        s.write_obj(o2, &[7u8; 257]);
        assert_eq!(s.free_off(o2), Err(SlabErr::Corrupt));
        assert_eq!(s.canary_faults, 1);
        // 槽位已归还（不泄漏）。
        assert_eq!(s.total_in_use(), 0);
    }

    #[test]
    fn churn_does_not_grow() {
        // 300 轮申请/释放循环：峰值稳定、无增长（slab 复用判据）。
        let mut s = SlabAlloc::new();
        let mut peak_seen = 0usize;
        for round in 0..300u32 {
            let mut hand = Vec::new();
            for i in 0..16u32 {
                let (o, _) = s.alloc_off(32 + (round + i) as usize % 192).unwrap();
                hand.push(o);
            }
            peak_seen = peak_seen.max(s.total_in_use());
            for o in hand {
                s.free_off(o).unwrap();
            }
        }
        assert_eq!(s.total_in_use(), 0);
        assert!(peak_seen <= 16, "同时持有不超过 16 个");
        // 300 轮后池不增长（固定 7×64 容量内自洽）。
        assert!(s.exhausted_of(0) == 0 || peak_seen == OBJS_PER_CLASS);
    }

    #[test]
    fn class_pools_are_isolated() {
        // 32 类用满不影响 64 类可用。
        let mut s = SlabAlloc::new();
        let mut a = Vec::new();
        for _ in 0..OBJS_PER_CLASS {
            a.push(s.alloc_off(32).unwrap().0);
        }
        assert!(s.alloc_off(32).is_err());
        let (b, _) = s.alloc_off(64).unwrap();
        assert!(s.in_use_of(0) == OBJS_PER_CLASS && s.in_use_of(1) == 1);
        for o in a {
            s.free_off(o).unwrap();
        }
        s.free_off(b).unwrap();
        assert_eq!(s.total_in_use(), 0);
    }

    #[test]
    fn zeroed_on_alloc() {
        let mut s = SlabAlloc::new();
        let (o, _) = s.alloc_off(64).unwrap();
        s.write_obj(o, &[0xFF; 64]);
        s.free_off(o).unwrap();
        let (o2, _) = s.alloc_off(64).unwrap();
        assert_eq!(o2, o, "freelist LIFO 立即复用");
        assert!(s.read_obj(o2, 64).unwrap().iter().all(|&b| b == 0), "kzalloc 清零");
    }

    #[test]
    fn bad_pointer_rejected() {
        let mut s = SlabAlloc::new();
        assert_eq!(s.free_off(usize::MAX / 2), Err(SlabErr::BadPtr));
        assert_eq!(s.free_off(3), Err(SlabErr::BadPtr)); // 未对齐
        // 偏移落在某类 stride 中段 → BadPtr。
        let (o, _) = s.alloc_off(32).unwrap();
        assert_eq!(s.free_off(o + 8), Err(SlabErr::BadPtr));
        s.free_off(o).unwrap();
    }
}
