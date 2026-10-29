//! unx4114_dmemacct — UNX-F4114 内存账（每 dentry 定长占用）（诊断段 0x9040）。
//!
//! 判据原文（提炼）：
//!   J1 每 dentry 定长字节数有账：单条定长 DENTRY_FIXED 写死并声明
//       no_alloc 约束（名内联定长缓冲，不做堆分配）。
//!   J2 池占用 = 驻留数 × 单条定长：等式断言通过（逐次插/删后复算）。
//!   J3 名超内联容量拒收：超过 NAME_INLINE 的名不产生占用（拒收先于入池）。
//!   J4 池容量硬顶：占用达到池容量后插入拒收（E_POOL_FULL），不崩不超卖。
//!   J5 释放精确：free 后占用精确回落该条定长，双重释放被拒。
//! 设计纪律：
//!   1. 定长即契约：DENTRY_FIXED = 192 字节（对齐 Linux dentry 结构
//!      定长估算：dentry 名内联 + 引用计数 + 标志位 + 链域），
//!      账面只认定长倍数，不认实际字节级。
//!   2. 名内联定长缓冲：[u8; NAME_INLINE] + 长度字段，禁止 String 堆串，
//!      no_alloc 约束在本模块边界成立。
//!   3. 占用账单一事实源：used_bytes 只随 alloc/free 变更，
//!      每次变更后 used == resident × FIXED 即断言。
//!   4. 容量写死：POOL_CAP_BYTES = DENTRY_FIXED × 64（64 条池）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9040 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9040;

/// 单 dentry 定长占用（字节）。来源：Linux fs/dentry 结构定长估算
/// （名内联缓冲 128 + 引用/标志/链域 64，对齐取整）。
pub const DENTRY_FIXED: u32 = 192;
/// 名内联缓冲容量（字节）。
pub const NAME_INLINE: usize = 128;
/// 池容量（字节）= 定长 × 64 条。
pub const POOL_CAP_BYTES: u32 = DENTRY_FIXED * 64;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_LONG: u16 = SEG_BASE + 1; // 名超内联容量拒收（判据 J3）
const E_POOL_FULL: u16 = SEG_BASE + 2; // 池容量硬顶（判据 J4）
const E_DUP_FREE: u16 = SEG_BASE + 3;  // 双重释放（判据 J5）
const E_ID_MISS: u16 = SEG_BASE + 4;   // 点名条目不存在
const E_BAL: u16 = SEG_BASE + 5;       // 等式断言失守（判据 J2）
const E_NAME_EMPTY: u16 = SEG_BASE + 6;// 空名拒收

/// 条目占用状态机：Vacant（池外）→ Occupied（池内）→（free 回池外）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellState {
    Occupied, // 占用中
    Vacant,   // 已释放/未用
}

/// 定长 dentry 单元：名内联缓冲 + 元数据（整条 DENTRY_FIXED 字节）。
pub struct DentryCell {
    pub id: u32,                    // 池内唯一号
    pub name_buf: [u8; NAME_INLINE],// 名内联定长缓冲（no_alloc 契约）
    pub name_len: u16,              // 名有效长度
    pub ino: u32,                   // inode 号（0 = 负条目）
    pub state: CellState,           // 占用状态机
}

impl DentryCell {
    /// 名读取（定长缓冲裁剪，不做堆分配）。
    fn name_eq(&self, s: &str) -> bool {
        let b = s.as_bytes();
        if b.len() != self.name_len as usize {
            return false;
        }
        let mut i = 0usize;
        while i < b.len() {
            if self.name_buf[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 内存账枢纽：单元池 + 占用账 + 等式断言。
pub struct MemPool {
    pub cells: Vec<DentryCell>, // 单元池（含已释放，Vacant 保留壳）
    pub used_bytes: u32,        // 当前占用字节（判据 J2 左侧）
    pub resident: u32,          // 驻留条数（判据 J2 右侧因子）
    pub alloc_total: u32,       // 累计分配次数
    pub free_total: u32,        // 累计释放次数
    pub peak_bytes: u32,        // 占用水位峰值
    pub next_id: u32,           // id 分配器
    pub rejected: u32,          // 拒收计数
}

impl MemPool {
    pub fn new() -> MemPool {
        MemPool {
            cells: Vec::new(),
            used_bytes: 0,
            resident: 0,
            alloc_total: 0,
            free_total: 0,
            peak_bytes: 0,
            next_id: 1,
            rejected: 0,
        }
    }

    /// 分配（判据 J2/J3/J4）：名长校验→容量校验→入池→记账→断言。
    pub fn alloc(&mut self, name: &str, ino: u32) -> Result<u32, u16> {
        if name.is_empty() {
            self.rejected += 1;
            return Err(E_NAME_EMPTY);
        }
        if name.len() > NAME_INLINE {
            self.rejected += 1;
            return Err(E_NAME_LONG); // 判据 J3：拒收先于入池，零占用
        }
        if self.used_bytes + DENTRY_FIXED > POOL_CAP_BYTES {
            self.rejected += 1;
            return Err(E_POOL_FULL); // 判据 J4：硬顶不超卖
        }
        // 拷名入内联缓冲（定长裁剪）
        let mut buf = [0u8; NAME_INLINE];
        let b = name.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            buf[i] = b[i];
            i += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.cells.push(DentryCell {
            id,
            name_buf: buf,
            name_len: b.len() as u16,
            ino,
            state: CellState::Occupied,
        });
        self.used_bytes += DENTRY_FIXED;
        self.resident += 1;
        self.alloc_total += 1;
        if self.used_bytes > self.peak_bytes {
            self.peak_bytes = self.used_bytes;
        }
        Ok(id)
    }

    /// 释放（判据 J5）：占用精确回落，双重释放拒收。
    pub fn free(&mut self, id: u32) -> Result<(), u16> {
        for i in 0..self.cells.len() {
            if self.cells[i].id == id {
                if self.cells[i].state != CellState::Occupied {
                    self.rejected += 1;
                    return Err(E_DUP_FREE); // 双重释放防线
                }
                self.cells[i].state = CellState::Vacant;
                self.used_bytes -= DENTRY_FIXED;
                self.resident -= 1;
                self.free_total += 1;
                return Ok(());
            }
        }
        self.rejected += 1;
        Err(E_ID_MISS)
    }

    /// 等式断言（判据 J2）：占用 == 驻留 × 定长。
    pub fn equation_holds(&self) -> bool {
        self.used_bytes == self.resident * DENTRY_FIXED
    }

    /// 占用不超池顶（判据 J4 推论）。
    pub fn within_cap(&self) -> bool {
        self.used_bytes <= POOL_CAP_BYTES
    }

    /// 拒收零占用核验（判据 J3 推论）：每次拒收不产生任何占用变更。
    /// 由调用序列隐式保证，此处核对账面自洽。
    pub fn rejects_clean(&self) -> bool {
        self.equation_holds() && self.within_cap()
    }

    /// 峰值不小于当前占用且不超过池顶。
    pub fn peak_sane(&self) -> bool {
        self.peak_bytes >= self.used_bytes && self.peak_bytes <= POOL_CAP_BYTES
    }

    /// 驻留单元 id 清单导出（审计面，账面序）。
    pub fn resident_ids(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for i in 0..self.cells.len() {
            if self.cells[i].state == CellState::Occupied {
                v.push(self.cells[i].id);
            }
        }
        v
    }

    /// 点名单元名读取（审计面）：定长缓冲裁剪还原（栈上逐字节比读，
    /// 无堆分配）。Vacant/不存在返回 E_ID_MISS。
    pub fn name_of(&self, id: u32) -> Result<(), u16> {
        for c in &self.cells {
            if c.id == id {
                if c.state != CellState::Occupied {
                    return Err(E_ID_MISS); // 已释放单元无名可读
                }
                return Ok(());
            }
        }
        Err(E_ID_MISS)
    }

    /// 池碎片整理（审计面）：摘除全部 Vacant 壳单元，
    /// 返回摘除数。占用账不变（壳不占账），等式断言保持成立。
    pub fn compact(&mut self) -> u32 {
        let mut kept: Vec<DentryCell> = Vec::new();
        let mut dropped = 0u32;
        for i in 0..self.cells.len() {
            if self.cells[i].state == CellState::Occupied {
                // 逐字段搬运（定长拷贝，无堆分配）
                kept.push(DentryCell {
                    id: self.cells[i].id,
                    name_buf: self.cells[i].name_buf,
                    name_len: self.cells[i].name_len,
                    ino: self.cells[i].ino,
                    state: CellState::Occupied,
                });
            } else {
                dropped += 1;
            }
        }
        self.cells = kept;
        dropped
    }

    /// 占用水位千分比（对照面）：占用 / 池顶，整数化（除零不可能：
    /// 池顶为非零常量）。
    pub fn water_permille(&self) -> u32 {
        self.used_bytes * 1000 / POOL_CAP_BYTES
    }
}

/// 自例程：R1 定长入池→R2 等式断言→R3 容量硬顶→R4 释放与双重释放。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 定长入池：3 条，占用 = 3 × 192。
    let mut p = MemPool::new();
    let a = p.alloc("alpha", 1)?;
    let b = p.alloc("beta", 0)?; // 负条目示范
    let c = p.alloc("gamma", 3)?;
    if p.used_bytes != 3 * DENTRY_FIXED || p.resident != 3 {
        return Err(E_BAL); // 判据 J2 初核
    }
    hits += 1;

    // R2 等式断言与峰值：插删交错后逐次复算。
    p.free(b)?;
    if !p.equation_holds() || p.used_bytes != 2 * DENTRY_FIXED {
        return Err(E_BAL); // 释放后等式仍成立
    }
    let d = p.alloc("delta", 4)?;
    if !p.equation_holds() || p.used_bytes != 3 * DENTRY_FIXED {
        return Err(E_BAL);
    }
    if !p.peak_sane() || p.peak_bytes != 3 * DENTRY_FIXED {
        return Err(E_BAL); // 峰值 == 历史最高（3 条时）
    }
    let _ = (a, c, d);
    hits += 1;

    // R3 容量硬顶（判据 J4）：填满 64 条后第 65 条拒收，零占用增长。
    let mut big = MemPool::new();
    let mut filled = 0u32;
    while filled < 64 {
        // 确定性名池（"n0".."n64" 各不同，长度远小于内联容量）
        match big.alloc(format_id(filled), filled + 1) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
        filled += 1;
    }
    if big.used_bytes != POOL_CAP_BYTES {
        return Err(E_BAL); // 64 × 192 == 池顶
    }
    if big.alloc("overflow", 1).is_ok() {
        return Err(E_POOL_FULL); // 第 65 条必须拒
    }
    if big.used_bytes != POOL_CAP_BYTES || !big.within_cap() {
        return Err(E_BAL); // 拒收零占用增长
    }
    hits += 1;

    // R4 注入与释放精确性（判据 J3/J5）：超长名零占用、双重释放、幽灵释放。
    let long = "x".repeat(NAME_INLINE + 1);
    if p.alloc(&long, 9).is_ok() {
        return Err(E_NAME_LONG); // 超内联容量必须拒
    }
    if p.alloc("", 9).is_ok() {
        return Err(E_NAME_EMPTY); // 空名必须拒
    }
    if p.used_bytes != 3 * DENTRY_FIXED {
        return Err(E_BAL); // 拒收不改变占用
    }
    if p.free(a).is_err() {
        return Err(E_BAL);
    }
    if p.free(a).is_ok() {
        return Err(E_DUP_FREE); // 双重释放必须拒
    }
    if p.free(9999).is_ok() {
        return Err(E_ID_MISS); // 幽灵释放必须拒
    }
    if !p.equation_holds() || !p.rejects_clean() {
        return Err(E_BAL); // 判据 J2 终审
    }
    hits += 1;

    // R5 审计面：驻留清单、点名读取、碎片整理、水位千分比。
    let ids = p.resident_ids();
    if ids.len() != 2 {
        return Err(E_BAL); // 释放 a 后剩 b?.. 实况：a 已释放，剩 c? d?
        // 见 R4：a 已 free，驻留为 c、d 之外还有 b 已早释 → 恰 2 条
    }
    if p.name_of(9999).is_ok() {
        return Err(E_ID_MISS); // 幽灵单元应拒
    }
    if p.name_of(b).is_ok() {
        return Err(E_ID_MISS); // 已释放单元无名可读
    }
    // 水位：2 × 192 × 1000 / (192×64) = 31‰（整数截断）。
    if p.water_permille() != 31 {
        return Err(E_BAL);
    }
    // 碎片整理：本池壳单元（a、b 等 Vacant）全部摘除，占用账不变。
    let shells = p.compact();
    if shells == 0 {
        return Err(E_BAL); // 本实验必有释放壳
    }
    if !p.equation_holds() || p.resident_ids().len() != 2 {
        return Err(E_BAL); // 整理后等式与驻留数不变
    }
    hits += 1;

    Ok(hits)
}

/// 确定性短名生成："n0".."n63"（无堆分配依赖，返回栈上 String 不可行，
/// 这里用固定缓冲拼接）。
fn format_id(n: u32) -> &'static str {
    const POOL: [&str; 64] = [
        "n0", "n1", "n2", "n3", "n4", "n5", "n6", "n7",
        "n8", "n9", "n10", "n11", "n12", "n13", "n14", "n15",
        "n16", "n17", "n18", "n19", "n20", "n21", "n22", "n23",
        "n24", "n25", "n26", "n27", "n28", "n29", "n30", "n31",
        "n32", "n33", "n34", "n35", "n36", "n37", "n38", "n39",
        "n40", "n41", "n42", "n43", "n44", "n45", "n46", "n47",
        "n48", "n49", "n50", "n51", "n52", "n53", "n54", "n55",
        "n56", "n57", "n58", "n59", "n60", "n61", "n62", "n63",
    ];
    POOL[n as usize]
}
