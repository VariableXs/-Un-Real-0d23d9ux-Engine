//! unx0874_vecalloc —— UNX-F0874 · 设备向量分配算法（位图+首次适配）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0874 · 设备向量分配算法。
//! 判据原文摘录（卡面验收行）：设备向量分配算法（位图+首次适配）
//!           ——向量带位图管理、首次适配命中、设备归属逐向量落账、
//!           保留带与越界全拦截。
//! 诊断段 0x78C0（本单独占，0x78C1~0x78CF 闭集）。
//!
//! 设计纪律：
//! - 向量空间：16~255 共 240 个可用位（0~15 为 CPU 异常/保留带，永不分配）。
//! - 位图驱动：u64 字数组 4 字 × 64 位 = 256 位，[15:0] 恒掩 1（保留带）。
//! - 首次适配：自位 16 起扫首段 ≥cnt 连续空闲带；MSI-X 多向量纪律要求
//!   段基按 cnt 对齐（cnt 为 2 的幂时强制，非幂时按向上取 2 幂对齐）。
//! - 设备归属：每向量记 owner（设备号），释放须 owner 对拍（非主释放即红）。
//! - 守恒恒等式：占用位 + 空闲位 = 240；分配向量数 = 在用数 + 释放数。
//! - 设备账：每设备一行（首次分配即开行），行内持有向量数守恒。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const VA_VERSION: &str = "unx0874_vecalloc-v1";

/// 诊断段基址（0x78C0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x78C0;

/// XcCode 闭集（0x78C1~0x78CF）。
pub const XC_RESERVED: u16 = SEG_BASE + 1; // 触及保留带（<16）
pub const XC_VEC_HI: u16 = SEG_BASE + 2; // 向量越上界（>255）
pub const XC_CAP: u16 = SEG_BASE + 3; // 无足够连续带
pub const XC_ZERO_REQ: u16 = SEG_BASE + 4; // 零长请求
pub const XC_NOT_OWNED: u16 = SEG_BASE + 5; // 非主释放 / 未分配释放
pub const XC_DEV_FULL: u16 = SEG_BASE + 6; // 设备行满（账面 64 行上限）
pub const XC_CONSERVE: u16 = SEG_BASE + 7; // 守恒破
pub const XC_ALIGN: u16 = SEG_BASE + 8; // 对齐破（段基未按纪律对齐）

/// 可用向量下界。
pub const VEC_LO: usize = 16;
/// 向量空间顶（含 255）。
pub const VEC_SPAN: usize = 240;
/// 位图字数（256 位面）。
pub const BM_WORDS: usize = 4;
/// 设备账行上限。
pub const DEV_CAP: usize = 64;

/// 设备账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DevRow {
    /// 设备号。
    pub dev: u32,
    /// 当前持有向量数。
    pub held: usize,
    /// 累计分配向量数。
    pub total_alloc: u64,
    /// 累计释放向量数。
    pub total_free: u64,
}

/// 向量分配器：位图 + 首次适配 + 设备归属账。
pub struct VecAlloc {
    /// 位图（true=占用）；[15:0] 恒 true。
    bm: [u64; BM_WORDS],
    /// 占用计数（可用带内）。
    used: usize,
    /// 设备账。
    devs: Vec<DevRow>,
    /// 归属表：owner[i] = 设备号（i 为可用带内索引 0..240）。
    owner: [u32; VEC_SPAN],
    /// 归属有效位（false=未分配）。
    owned: [bool; VEC_SPAN],
    /// 累计分配 / 释放（向量级）。
    alloc_v: u64,
    free_v: u64,
}

impl VecAlloc {
    pub fn new() -> VecAlloc {
        let mut v = VecAlloc {
            bm: [0u64; BM_WORDS],
            used: 0,
            devs: Vec::new(),
            owner: [0u32; VEC_SPAN],
            owned: [false; VEC_SPAN],
            alloc_v: 0,
            free_v: 0,
        };
        // 保留带恒掩：位 0..16 置 1（u64 字内低位在前）
        for i in 0..VEC_LO {
            v.bm[i / 64] |= 1u64 << (i % 64);
        }
        v
    }

    /// 位读写（全 256 位面索引）。
    fn get(&self, i: usize) -> bool {
        self.bm[i / 64] & (1u64 << (i % 64)) != 0
    }
    fn set(&mut self, i: usize, v: bool) {
        if v {
            self.bm[i / 64] |= 1u64 << (i % 64);
        } else {
            self.bm[i / 64] &= !(1u64 << (i % 64));
        }
    }

    /// 段长对齐值：2 的幂取自身；非幂向上取最近 2 幂（MSI-X 多向量纪律）。
    fn align_of(cnt: usize) -> usize {
        if cnt & (cnt - 1) == 0 {
            cnt
        } else {
            let mut a = 1usize;
            while a < cnt {
                a <<= 1;
            }
            a
        }
    }

    /// 首次适配：在 [VEC_LO, 256) 找基址按 align 对齐、长 ≥cnt 的空闲带。
    fn first_fit(&self, cnt: usize, align: usize) -> Result<usize, u16> {
        let mut i = VEC_LO;
        while i < 256 {
            let cand = (i + align - 1) & !(align - 1);
            if cand >= 256 {
                break;
            }
            let mut run = 0usize;
            let mut j = cand;
            while j < 256 && !self.get(j) {
                run += 1;
                if run == cnt {
                    return Ok(cand);
                }
                j += 1;
            }
            if j >= 256 {
                break;
            }
            i = j + 1;
        }
        Err(XC_CAP)
    }

    /// 设备行定位（首次出现即开行）。
    fn dev_row(&mut self, dev: u32) -> Result<usize, u16> {
        if let Some(pos) = self.devs.iter().position(|r| r.dev == dev) {
            return Ok(pos);
        }
        if self.devs.len() >= DEV_CAP {
            return Err(XC_DEV_FULL);
        }
        self.devs.push(DevRow { dev, held: 0, total_alloc: 0, total_free: 0 });
        Ok(self.devs.len() - 1)
    }

    /// 分配 cnt 个连续向量给设备 dev：首次适配 + 对齐 + 归属落账。
    /// 返回段基（绝对向量号）。
    pub fn alloc(&mut self, dev: u32, cnt: usize) -> Result<usize, u16> {
        // 1. 参数门
        if cnt == 0 {
            return Err(XC_ZERO_REQ);
        }
        if cnt > VEC_SPAN - self.used {
            return Err(XC_CAP);
        }
        // 2. 首次适配（段基对齐纪律）
        let align = Self::align_of(cnt);
        let base = self.first_fit(cnt, align)?;
        // 3. 对齐复核（段基必须对齐 + 不触保留带）
        if base < VEC_LO || base % align != 0 {
            return Err(XC_ALIGN);
        }
        // 4. 落位 + 归属
        for j in base..base + cnt {
            self.set(j, true);
            let idx = j - VEC_LO;
            self.owner[idx] = dev;
            self.owned[idx] = true;
        }
        self.used += cnt;
        self.alloc_v += cnt as u64;
        // 5. 设备账
        let r = self.dev_row(dev)?;
        self.devs[r].held += cnt;
        self.devs[r].total_alloc += cnt as u64;
        Ok(base)
    }

    /// 释放段（base, cnt）：owner 对拍 + 逐位回零。
    pub fn free(&mut self, dev: u32, base: usize, cnt: usize) -> Result<(), u16> {
        // 1. 界门：不许触保留带
        if base < VEC_LO || cnt == 0 || base + cnt > 256 {
            return Err(if base < VEC_LO { XC_RESERVED } else { XC_VEC_HI });
        }
        // 2. 持有门：逐位 owner 对拍
        for j in base..base + cnt {
            let idx = j - VEC_LO;
            if !self.owned[idx] || self.owner[idx] != dev {
                return Err(XC_NOT_OWNED);
            }
        }
        // 3. 回零
        for j in base..base + cnt {
            self.set(j, false);
            self.owned[j - VEC_LO] = false;
        }
        self.used -= cnt;
        self.free_v += cnt as u64;
        // 4. 设备账
        if let Some(pos) = self.devs.iter().position(|r| r.dev == dev) {
            self.devs[pos].held -= cnt;
            self.devs[pos].total_free += cnt as u64;
        } else {
            return Err(XC_CONSERVE);
        }
        // 5. 守恒核
        self.check_conserve()
    }

    /// 守恒恒等式核：占用 + 空闲 = 240；分配 = 在用 + 释放；
    /// 设备行 held 总和 = 在用数。
    pub fn check_conserve(&self) -> Result<(), u16> {
        if self.used + self.free_slots() != VEC_SPAN {
            return Err(XC_CONSERVE);
        }
        if self.alloc_v != self.used as u64 + self.free_v {
            return Err(XC_CONSERVE);
        }
        let held_sum: usize = self.devs.iter().map(|r| r.held).sum();
        if held_sum != self.used {
            return Err(XC_CONSERVE);
        }
        Ok(())
    }

    /// 空闲位计数（可用带内）。
    pub fn free_slots(&self) -> usize {
        let mut n = 0usize;
        for i in VEC_LO..256 {
            if !self.get(i) {
                n += 1;
            }
        }
        n
    }

    /// 向量归属查询。
    pub fn owner_of(&self, vec: usize) -> Result<u32, u16> {
        if vec < VEC_LO {
            return Err(XC_RESERVED);
        }
        if vec >= 256 {
            return Err(XC_VEC_HI);
        }
        let idx = vec - VEC_LO;
        if !self.owned[idx] {
            return Err(XC_NOT_OWNED);
        }
        Ok(self.owner[idx])
    }

    /// 设备行读取。
    pub fn dev(&self, i: usize) -> Result<DevRow, u16> {
        if i >= self.devs.len() {
            return Err(XC_DEV_FULL);
        }
        Ok(self.devs[i])
    }

    /// 设备行数。
    pub fn dev_count(&self) -> usize {
        self.devs.len()
    }

    /// 在用向量数。
    pub fn used(&self) -> usize {
        self.used
    }

    /// 最大空闲带（碎片观测）。
    pub fn max_run(&self) -> usize {
        let mut best = 0usize;
        let mut run = 0usize;
        for i in VEC_LO..256 {
            if self.get(i) {
                run = 0;
            } else {
                run += 1;
                if run > best {
                    best = run;
                }
            }
        }
        best
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 基线：dev 0x100 分配 4 向量（对齐 4）→ 基址 16
    let mut a = VecAlloc::new();
    let b1 = a.alloc(0x100, 4)?;
    if b1 != 16 || a.used() != 4 {
        return Err(XC_ALIGN);
    }
    // 2. 归属查询
    if a.owner_of(16)? != 0x100 || a.owner_of(19)? != 0x100 {
        return Err(XC_NOT_OWNED);
    }
    // 3. 段基对齐纪律：cnt=8 → 对齐 8 → 基址 24（跳过 20~23 缝）
    let b2 = a.alloc(0x101, 8)?;
    if b2 != 24 || b2 % 8 != 0 {
        return Err(XC_ALIGN);
    }
    // 4. 填缝：cnt=4 → 基址 20
    let b3 = a.alloc(0x102, 4)?;
    if b3 != 20 {
        return Err(XC_CAP);
    }
    // 5. 错误路径①：零长
    if a.alloc(0x103, 0).is_ok() {
        return Err(XC_ZERO_REQ);
    }
    // 6. 错误路径②：容量不足（申请 > 剩余）
    let need = VEC_SPAN - a.used() + 1;
    if a.alloc(0x103, need).is_ok() {
        return Err(XC_CAP);
    }
    // 7. 错误路径③：非主释放（dev 0x103 释放 0x100 的段）
    if a.free(0x103, b1, 4).is_ok() {
        return Err(XC_NOT_OWNED);
    }
    // 8. 错误路径④：未分配段释放
    if a.free(0x100, 200, 4).is_ok() {
        return Err(XC_NOT_OWNED);
    }
    // 9. 错误路径⑤：触保留带释放
    if a.free(0x100, 12, 4).is_ok() {
        return Err(XC_RESERVED);
    }
    // 10. 正常释放 + 复分
    a.free(0x100, b1, 4)?;
    if a.owner_of(16).is_ok() {
        return Err(XC_CONSERVE);
    }
    let b4 = a.alloc(0x103, 4)?;
    if b4 != 16 {
        return Err(XC_CAP);
    }
    // 11. 守恒总核 + 碎片观测
    a.check_conserve()?;
    if a.max_run() < 8 {
        return Err(XC_CONSERVE);
    }
    if a.dev_count() != 4 {
        return Err(XC_CONSERVE);
    }
    let d = a.dev(0)?;
    if d.dev != 0x100 || d.held != 0 || d.total_alloc != 4 || d.total_free != 4 {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
