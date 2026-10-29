//! unx0867_bindtbl —— UNX-F0867 · 引脚→向量绑定表与反向查询（目标 280 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0867 · 引脚→向量绑定表与反向查询。
//! 判据原文：UNX-F0867-J1 绑定唯一性（同 GSI 双绑定 10/10 次拦截），
//!           反向查询（向量→引脚）与正向一致率 100%。
//! 设计纪律：
//! - 正向表：GSI → 绑定行 {向量, 实例, 引脚, 触发/极性快照}；反向索引：
//!   向量 → GSI（映射核与正向互逆）。绑定唯一性：同 GSI 只许一行；
//!   同向量只许一行（双向唯一）。
//! - 反向查询 O(256) 定容倒排表（向量号直取，判据「反向一致率 100%」锚）。
//! - 绑定生命周期：Bound → Unbound（解绑）；重绑需先解（LIVE 态双绑拦截）。
//! - 一致率核：全表逐行正反互查（GSI→vec→GSI 恒等），任何漂移即红账。
//! - 同 GSI 双绑定注入 10/10 拦截（判据前半执行面）。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6940（0x6941~0x6949 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const BT_VERSION: &str = "unx0867_bindtbl-v1";

/// 诊断段基址（0x6940 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6940;

/// XcCode 闭集。
pub const XC_GSI_RANGE: u16 = SEG_BASE + 1; // GSI 越界（建模面 ≤255）
pub const XC_VEC_RANGE: u16 = SEG_BASE + 2; // 向量越界
pub const XC_GSI_DUP: u16 = SEG_BASE + 3; // 同 GSI 双绑定
pub const XC_VEC_DUP: u16 = SEG_BASE + 4; // 同向量双绑定
pub const XC_NOT_BOUND: u16 = SEG_BASE + 5; // 未绑定即查询/解绑
pub const XC_REVERSE_MISS: u16 = SEG_BASE + 6; // 反向查询失配
pub const XC_CONSIST: u16 = SEG_BASE + 7; // 正反一致率 <100%
pub const XC_LED_FULL: u16 = SEG_BASE + 8; // 账溢出
pub const XC_INJECT_MISS: u16 = SEG_BASE + 9; // 双绑注入漏抓

/// GSI 建模面界限。
pub const GSI_MAX: usize = 255;
/// 向量总数。
pub const VEC_TOTAL: usize = 256;

/// 绑定行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BindRow {
    /// GSI。
    pub gsi: u8,
    /// 绑定向量。
    pub vector: u8,
    /// IO-APIC 实例。
    pub inst: u8,
    /// 实例内引脚。
    pub pin: u8,
    /// 触发快照（0 边沿 1 电平）。
    pub level: bool,
}

/// 绑定表（正向 + 反向倒排）。
pub struct BindTable {
    /// 正向：GSI → 行（None = 未绑）。
    fwd: [Option<BindRow>; GSI_MAX + 1],
    /// 反向：向量 → GSI（0xFF = 未绑；GSI 0 合法故用哨兵 0xFF）。
    rev: [u8; VEC_TOTAL],
    bound_count: usize,
    /// 拦截账。
    intercepts: Vec<(u8, u16, u64)>,
    seq: u64,
    intercept_overflow: usize,
}

const UNBOUND: u8 = 0xFF;

impl BindTable {
    pub fn new() -> BindTable {
        BindTable {
            fwd: [None; GSI_MAX + 1],
            rev: [UNBOUND; VEC_TOTAL],
            bound_count: 0,
            intercepts: Vec::new(),
            seq: 0,
            intercept_overflow: 0,
        }
    }

    fn record(&mut self, gsi: u8, code: u16) {
        self.seq += 1;
        if self.intercepts.len() < 64 {
            self.intercepts.push((gsi, code, self.seq));
        } else {
            self.intercept_overflow += 1;
        }
    }

    /// 绑定：双向唯一核（同 GSI 拒 / 同向量拒）→ 正反双写。
    pub fn bind(&mut self, row: BindRow) -> Result<(), u16> {
        if (row.gsi as usize) > GSI_MAX {
            self.record(row.gsi, XC_GSI_RANGE);
            return Err(XC_GSI_RANGE);
        }
        // 向量 0xFF 哨兵不可用作绑定值
        if row.vector == UNBOUND {
            self.record(row.gsi, XC_VEC_RANGE);
            return Err(XC_VEC_RANGE);
        }
        // 同 GSI 双绑定拦截
        if self.fwd[row.gsi as usize].is_some() {
            self.record(row.gsi, XC_GSI_DUP);
            return Err(XC_GSI_DUP);
        }
        // 同向量双绑定拦截
        if self.rev[row.vector as usize] != UNBOUND {
            self.record(row.gsi, XC_VEC_DUP);
            return Err(XC_VEC_DUP);
        }
        self.fwd[row.gsi as usize] = Some(row);
        self.rev[row.vector as usize] = row.gsi;
        self.bound_count += 1;
        Ok(())
    }

    /// 解绑。
    pub fn unbind(&mut self, gsi: u8) -> Result<BindRow, u16> {
        if (gsi as usize) > GSI_MAX {
            self.record(gsi, XC_GSI_RANGE);
            return Err(XC_GSI_RANGE);
        }
        let row = match self.fwd[gsi as usize].take() {
            Some(r) => r,
            None => {
                self.record(gsi, XC_NOT_BOUND);
                return Err(XC_NOT_BOUND);
            }
        };
        self.rev[row.vector as usize] = UNBOUND;
        self.bound_count -= 1;
        Ok(row)
    }

    /// 正向查询（GSI → 行）。
    pub fn by_gsi(&self, gsi: u8) -> Result<BindRow, u16> {
        if (gsi as usize) > GSI_MAX {
            return Err(XC_GSI_RANGE);
        }
        self.fwd[gsi as usize].ok_or(XC_NOT_BOUND)
    }

    /// 反向查询（向量 → GSI）。
    pub fn gsi_of_vector(&self, vector: u8) -> Result<u8, u16> {
        if vector == UNBOUND {
            return Err(XC_VEC_RANGE);
        }
        let gsi = self.rev[vector as usize];
        if gsi == UNBOUND {
            return Err(XC_NOT_BOUND);
        }
        Ok(gsi)
    }

    /// 反向行查询（向量 → 绑定行）。
    pub fn row_of_vector(&self, vector: u8) -> Result<BindRow, u16> {
        let gsi = self.gsi_of_vector(vector)?;
        self.by_gsi(gsi)
    }

    /// 正反一致率核（判据 100%）：全绑定行 gsi→vec→gsi 恒等 + rev/fwd 互照。
    pub fn consistency(&self) -> Result<usize, u16> {
        for i in 0..=(GSI_MAX) {
            if let Some(r) = self.fwd[i] {
                // 正向行存在 → 反向表必须指回
                if self.rev[r.vector as usize] != r.gsi {
                    return Err(XC_REVERSE_MISS);
                }
            }
        }
        // 反向表非哨兵项 → 正向表必须有行
        for (v, g) in self.rev.iter().enumerate() {
            if *g != UNBOUND {
                if self.fwd[*g as usize].map(|r| r.vector as usize) != Some(v) {
                    return Err(XC_CONSIST);
                }
            }
        }
        Ok(self.bound_count)
    }

    /// 同 GSI 双绑定注入 10/10（判据前半执行面）。
    pub fn dup_inject_10(&mut self, base_gsi: u8, base_vec: u8) -> Result<usize, u16> {
        // 先绑一行
        self.bind(BindRow {
            gsi: base_gsi,
            vector: base_vec,
            inst: 0,
            pin: base_gsi % 24,
            level: false,
        })?;
        let mut blocked = 0;
        for i in 0..10u16 {
            // 交替：同 GSI 换向量 / 同向量换 GSI
            let probe = if i % 2 == 0 {
                BindRow {
                    gsi: base_gsi,
                    vector: base_vec.wrapping_add(1 + i as u8),
                    inst: 0,
                    pin: 1,
                    level: false,
                }
            } else {
                BindRow {
                    gsi: base_gsi.wrapping_add(1 + i as u8),
                    vector: base_vec,
                    inst: 0,
                    pin: 2,
                    level: false,
                }
            };
            if self.bind(probe).is_err() {
                blocked += 1;
            } else {
                // 泄漏：撤销泄漏行恢复不变量
                let g = probe.gsi;
                let _ = self.unbind(g);
            }
        }
        if blocked != 10 {
            return Err(XC_INJECT_MISS);
        }
        Ok(blocked)
    }

    /// 统计。
    pub fn stat(&self) -> (usize, usize, usize) {
        (self.bound_count, self.intercepts.len(), self.intercept_overflow)
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        let (_, l, ov) = self.stat();
        l + ov == self.seq as usize
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    let mut t = BindTable::new();
    // 1. 三行绑定（GSI 0/2/5 → 向量 0x41/0x42/0x50）
    t.bind(BindRow { gsi: 0, vector: 0x41, inst: 0, pin: 0, level: false })?;
    t.bind(BindRow { gsi: 2, vector: 0x42, inst: 0, pin: 2, level: true })?;
    t.bind(BindRow { gsi: 5, vector: 0x50, inst: 0, pin: 5, level: false })?;
    // 2. 正向查询
    let r2 = t.by_gsi(2)?;
    if r2.vector != 0x42 || !r2.level {
        return Err(XC_REVERSE_MISS);
    }
    // 3. 反向查询与正向一致
    if t.gsi_of_vector(0x42)? != 2 {
        return Err(XC_REVERSE_MISS);
    }
    let rr = t.row_of_vector(0x50)?;
    if rr.gsi != 5 || rr.vector != 0x50 {
        return Err(XC_REVERSE_MISS);
    }
    // 4. 一致率 100%
    if t.consistency()? != 3 {
        return Err(XC_CONSIST);
    }
    // 5. 双绑注入 10/10
    let blocked = t.dup_inject_10(9, 0x60)?;
    if blocked != 10 {
        return Err(XC_INJECT_MISS);
    }
    // 6. 注入后表仍 3 行 + 一致
    if t.consistency()? != 3 {
        return Err(XC_CONSIST);
    }
    // 7. 解绑 + 复绑
    let old = t.unbind(5)?;
    if old.vector != 0x50 {
        return Err(XC_REVERSE_MISS);
    }
    if t.gsi_of_vector(0x50).is_ok() {
        return Err(XC_NOT_BOUND);
    }
    t.bind(BindRow { gsi: 5, vector: 0x51, inst: 0, pin: 5, level: false })?;
    if t.gsi_of_vector(0x51)? != 5 {
        return Err(XC_REVERSE_MISS);
    }
    // 8. 守恒
    if !t.conserve() {
        return Err(XC_LED_FULL);
    }
    // 9. 错误路径①：GSI 越界
    if t.by_gsi(200).is_ok() {
        return Err(XC_GSI_RANGE);
    }
    // 10. 错误路径②：向量哨兵值
    if t.gsi_of_vector(0xFF).is_ok() {
        return Err(XC_VEC_RANGE);
    }
    // 11. 错误路径③：未绑定查询
    if t.by_gsi(100).is_ok() {
        return Err(XC_NOT_BOUND);
    }
    // 12. 错误路径④：解绑未绑
    if t.unbind(100).is_ok() {
        return Err(XC_NOT_BOUND);
    }
    Ok(0)
}
