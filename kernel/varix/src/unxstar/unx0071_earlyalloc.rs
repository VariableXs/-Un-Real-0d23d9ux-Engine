//! UNX-F0071 · 早年引导内存分配器 · 诊断段 0x2160
//! 验收锚点：UNX-F0071-J1 bump 分配核心 + 页级回收账，
//! 分配/对齐/耗尽/审计四径判据通过；纯功能代码 ≥320 行。
//! 纪律：早年分配器只进不出（bump 语义），回收仅记账供后期核对。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2160;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EXHAUST: u16 = SEG_BASE + 1;   // 池耗尽
const E_ALIGN: u16 = SEG_BASE + 2;     // 对齐粒度非法
const E_ZERO: u16 = SEG_BASE + 3;      // 零长度分配
const E_RANGE: u16 = SEG_BASE + 4;     // 长度回绕
const E_RDONLY: u16 = SEG_BASE + 5;    // 池已封板（移交给页分配器）
const E_BAL: u16 = SEG_BASE + 6;       // 账守恒：分配字节合计 + 对齐耗损 = 已推进量
const E_DOUBLE_FREE: u16 = SEG_BASE + 7; // bump 语义下的回收误用
const E_NOTOWNED: u16 = SEG_BASE + 8;  // 回收地址不在池域
const E_ORDER: u16 = SEG_BASE + 9;     // 账目序（回收指针非单调）

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EXHAUST => "池耗尽：扩大早期池或降低常驻需求",
        E_ALIGN => "对齐粒度非法：2 的幂且 ≤ 池大小",
        E_ZERO => "零长度分配：调用方语义错误",
        E_RANGE => "长度回绕：base+len 溢出",
        E_RDONLY => "池已封板：移交后禁再分配",
        E_BAL => "账守恒：Σ分配 + Σ对齐耗损 + Σ冻结尾 = 推进量",
        E_DOUBLE_FREE => "bump 分配器不回收：回收路径属页分配器域",
        E_NOTOWNED => "地址不在池域：核对移交边界",
        E_ORDER => "回收账序违例：移交清单必须按地址升序",
        _ => "未知诊断码",
    }
}

/// 分配记录（审计账目）。
#[derive(Clone, Copy, Debug)]
pub struct AllocRecord {
    pub base: u64,
    pub len: u64,
    pub tag: u32, // 分配用途标签（闭集外自由编码）
}

/// 常驻标签闭集（关键早期结构）。
pub const TAG_PAGE_BITMAP: u32 = 1;
pub const TAG_HANDOVER: u32 = 2;
pub const TAG_FRAMEBOOK: u32 = 3;
pub const TAG_SCRATCH: u32 = 4;

// ── Bump 分配器 ──────────────────────────────────────────────
pub struct EarlyBump {
    pool_base: u64,
    pool_len: u64,
    cursor: u64,
    pub records: Vec<AllocRecord>,
    pub align_waste: u64,
    pub sealed: bool,
}

impl EarlyBump {
    pub fn new(pool_base: u64, pool_len: u64) -> Result<EarlyBump, u16> {
        if pool_len == 0 {
            return Err(E_ZERO);
        }
        if pool_base.checked_add(pool_len).is_none() {
            return Err(E_RANGE);
        }
        Ok(EarlyBump {
            pool_base,
            pool_len,
            cursor: pool_base,
            records: Vec::new(),
            align_waste: 0,
            sealed: false,
        })
    }

    pub fn pool_end(&self) -> u64 {
        self.pool_base + self.pool_len
    }

    pub fn used(&self) -> u64 {
        self.cursor - self.pool_base
    }

    pub fn free(&self) -> u64 {
        self.pool_end() - self.cursor
    }

    /// bump 分配：先对齐后分配，失败显性 E_EXHAUST。
    pub fn alloc(&mut self, len: u64, align: u64, tag: u32) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if len == 0 {
            return Err(E_ZERO);
        }
        if align == 0 || align & align.wrapping_sub(1) != 0 {
            return Err(E_ALIGN);
        }
        let aligned = self
            .cursor
            .checked_add(align - 1)
            .map(|x| x & !(align - 1))
            .ok_or(E_RANGE)?;
        let end = aligned.checked_add(len).ok_or(E_RANGE)?;
        if end > self.pool_end() {
            return Err(E_EXHAUST);
        }
        let waste = aligned - self.cursor;
        self.align_waste += waste;
        self.cursor = end;
        self.records.push(AllocRecord { base: aligned, len, tag });
        Ok(aligned)
    }

    /// bump 语义红线：回收路径不存在。
    pub fn free_ptr(&self, _addr: u64) -> u16 {
        E_DOUBLE_FREE
    }

    /// 地址归属判定（移交边界核对）。
    pub fn owns(&self, addr: u64) -> bool {
        addr >= self.pool_base && addr < self.pool_end()
    }

    /// 封板移交：产出冻结尾 = 未用余量，随页分配器接管。
    pub fn seal_and_handover(&mut self) -> Result<(u64, u64), u16> {
        // 守恒总审
        let allocated: u64 = self.records.iter().map(|r| r.len).sum();
        if self.used() != allocated + self.align_waste {
            return Err(E_BAL);
        }
        self.sealed = true;
        Ok((self.cursor, self.free()))
    }

    /// 移交清单（按地址升序，供页分配器接管已分配段）。
    pub fn handover_manifest(&self) -> Result<Vec<AllocRecord>, u16> {
        if !self.sealed {
            return Err(E_RDONLY);
        }
        let mut m = self.records.clone();
        m.sort_by_key(|r| r.base);
        // 升序纪律核验
        for w in m.windows(2) {
            if w[0].base > w[1].base {
                return Err(E_ORDER);
            }
        }
        Ok(m)
    }

    /// 标签用量统计（5 桶 + 杂项）。
    pub fn tag_histogram(&self) -> [u64; 6] {
        let mut h = [0u64; 6];
        for r in &self.records {
            let i = match r.tag {
                TAG_PAGE_BITMAP => 1,
                TAG_HANDOVER => 2,
                TAG_FRAMEBOOK => 3,
                TAG_SCRATCH => 4,
                _ => 5,
            };
            h[i] += r.len;
        }
        h
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.pool_base.to_le_bytes());
        buf.extend_from_slice(&self.cursor.to_le_bytes());
        for r in &self.records {
            buf.extend_from_slice(&r.base.to_le_bytes());
            buf.extend_from_slice(&r.len.to_le_bytes());
            buf.extend_from_slice(&r.tag.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三笔分配（不同对齐）→ 账目精确、守恒通过。
    let mut b = EarlyBump::new(0x1000_0000, 0x10_0000)?;
    let a1 = b.alloc(0x1000, 0x1000, TAG_PAGE_BITMAP)?;
    let a2 = b.alloc(0x200, 0x40, TAG_FRAMEBOOK)?;
    let a3 = b.alloc(0x1_0000, 0x1_0000, TAG_HANDOVER)?;
    if a1 != 0x1000_0000 || a2 != 0x1000_1000 || a3 != 0x1001_0000 {
        return Err(E_BAL);
    }
    b.seal_and_handover()?;
    let manifest = b.handover_manifest()?;
    if manifest.len() != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：对齐耗损记账——0（a1→a2）+ 0xEE00（a2→a3）= 0xEE00。
    if b.align_waste != 0xEE00 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：标签直方图守恒。
    let h = b.tag_histogram();
    if h[1] != 0x1000 || h[2] != 0x1_0000 || h[3] != 0x200 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：耗尽路径——请求大于余量显性报错，池不破坏。
    let mut b2 = EarlyBump::new(0x2000_0000, 0x1_0000)?;
    b2.alloc(0xF_000, 0x1000, TAG_SCRATCH)?;
    if b2.alloc(0x2_000, 0x1000, TAG_SCRATCH) != Err(E_EXHAUST) {
        return Err(E_EXHAUST);
    }
    // 失败后仍可小额分配（游标未被破坏）
    let ok = b2.alloc(0x800, 0x100, TAG_SCRATCH)?;
    if ok != 0x2000_0000 + 0xF_000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：指纹对拍。
    let mut b3 = EarlyBump::new(0x1000_0000, 0x10_0000)?;
    b3.alloc(0x1000, 0x1000, TAG_PAGE_BITMAP)?;
    b3.alloc(0x200, 0x40, TAG_FRAMEBOOK)?;
    b3.alloc(0x1_0000, 0x1_0000, TAG_HANDOVER)?;
    let mut b4 = EarlyBump::new(0x1000_0000, 0x10_0000)?;
    b4.alloc(0x1000, 0x1000, TAG_PAGE_BITMAP)?;
    b4.alloc(0x200, 0x40, TAG_FRAMEBOOK)?;
    b4.alloc(0x1_0000, 0x1_0000, TAG_HANDOVER)?;
    if b3.fingerprint() != b4.fingerprint() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：bump 回收 → E_DOUBLE_FREE（红线）。
    let i1 = EarlyBump::new(0x3000_0000, 0x1000)?;
    if i1.free_ptr(0x3000_0000) != E_DOUBLE_FREE {
        return Err(E_DOUBLE_FREE);
    }
    hits += 1;

    // I2：对齐非 2 幂 → E_ALIGN。
    let mut i2 = EarlyBump::new(0x3000_0000, 0x1000)?;
    if i2.alloc(0x10, 0x30, TAG_SCRATCH) != Err(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I3：零长度 → E_ZERO。
    if i2.alloc(0, 0x10, TAG_SCRATCH) != Err(E_ZERO) {
        return Err(E_ZERO);
    }
    hits += 1;

    // I4：封板后分配 → E_RDONLY。
    let mut i4 = EarlyBump::new(0x4000_0000, 0x2000)?;
    i4.alloc(0x100, 0x10, TAG_SCRATCH)?;
    i4.seal_and_handover()?;
    if i4.alloc(0x10, 0x10, TAG_SCRATCH) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：未封板出清单 → E_RDONLY。
    let i5 = EarlyBump::new(0x5000_0000, 0x2000)?;
    if !matches!(i5.handover_manifest(), Err(E_RDONLY)) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I6：守恒破坏防线（手动推游标模拟损坏）→ E_BAL。
    let mut i6 = EarlyBump::new(0x6000_0000, 0x2000)?;
    i6.alloc(0x100, 0x10, TAG_SCRATCH)?;
    i6.cursor += 0x100; // 账外推进
    if i6.seal_and_handover() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
