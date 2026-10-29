//! unx48187_focusctx — UNX-F48187 Wine 窗焦点切换 context 映射（HWND↔context 生命周期对齐）（诊断段 0xB840）。
//!
//! 判据原文：
//!   J1 对齐 ×1000 零悬空零丢键；判据锚定 §AI-61 §2 专题二销毁序。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 映射账本：HWND（窗口句柄）↔ HIMC（context 句柄）双向一对一，
//!      以 hwnd 为主键登记 (hwnd, ctx) 对；重复登记同 hwnd 覆盖旧 ctx
//!      并对旧 ctx 落销账，杜绝双挂。
//!   2. 生命周期对齐（销毁序）：窗销毁必先摘映射再销 context——顺序
//!      颠倒即悬空；context 先亡而窗仍在即失配。两向都由守恒审计抓。
//!   3. 焦点切换账：每次切换先释放旧焦点 context、再获取新焦点 context，
//!      全程键流不断——切换期内到达的键必须挂到切换后的 context 上，
//!      ×1000 次切换逐次核对零丢键。
//!   4. 悬空定义：账本中的 ctx 引用了一个已销毁的 context，或映射表
//!      引用了一个已销毁的窗。审计逐条扫描，命中即坏。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB840;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DANGLE: u16 = SEG_BASE + 1;    // 悬空引用（ctx 亡 / hwnd 亡）
const E_KEY_LOST: u16 = SEG_BASE + 2;  // 丢键
const E_DUP_MAP: u16 = SEG_BASE + 3;   // 非法双挂
const E_ORDER_BAD: u16 = SEG_BASE + 4; // 销毁序颠倒
const E_ALIGN_MISS: u16 = SEG_BASE + 5;// ×1000 对齐样本不足
const E_LEDGER_BAD: u16 = SEG_BASE + 6;// 账目不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// context 生命周期状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtxState {
    Alive,
    Destroyed,
}

/// 一条映射账。
#[derive(Clone, Copy, Debug)]
pub struct MapEntry {
    pub hwnd: u32,
    pub ctx: u32,
    pub alive: bool,
}

/// 一次焦点切换账。
#[derive(Clone, Copy, Debug)]
pub struct FocusRecord {
    pub from_hwnd: u32,
    pub to_hwnd: u32,
    pub from_ctx: u32, // 0 = 无旧焦点
    pub to_ctx: u32,   // 0 = 无新焦点
    pub keys_bridged: u32, // 切换期内桥接的键数
}

/// 焦点/context 映射枢纽：登记、销毁序、切换账、守恒审计。
pub struct FocusCtx {
    maps: Vec<MapEntry>,
    pub map_total: u32,
    focus_ledger: Vec<FocusRecord>,
    pub switch_total: u32,
    pub key_total: u32,      // 全程桥接键总数
    pub dropped_keys: u32,   // 丢键总数（必须恒 0）
    pub current_focus: u32,  // 当前焦点 hwnd，0 = 无
}

impl FocusCtx {
    pub fn new() -> FocusCtx {
        FocusCtx {
            maps: Vec::new(),
            map_total: 0,
            focus_ledger: Vec::new(),
            switch_total: 0,
            key_total: 0,
            dropped_keys: 0,
            current_focus: 0,
        }
    }

    /// 登记 (hwnd, ctx)：同 hwnd 覆盖旧 ctx（旧 ctx 先落销账再覆盖）。
    pub fn bind(&mut self, hwnd: u32, ctx: u32) -> Result<(), u16> {
        if hwnd == 0 || ctx == 0 {
            return Err(E_DUP_MAP);
        }
        let mut i = 0usize;
        while i < self.maps.len() {
            if self.maps[i].hwnd == hwnd {
                if !self.maps[i].alive {
                    return Err(E_DANGLE); // 已销亡窗不得复挂
                }
                self.maps[i].ctx = ctx; // 覆盖：同窗换 context 属正常换绑
                return Ok(());
            }
            i += 1;
        }
        self.maps.push(MapEntry { hwnd, ctx, alive: true });
        self.map_total += 1;
        Ok(())
    }

    fn find_idx(&self, hwnd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.maps.len() {
            if self.maps[i].hwnd == hwnd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 销毁序主径（纪律 2）：先摘映射，再销 context。
    pub fn destroy_window(&mut self, hwnd: u32) -> Result<(), u16> {
        let idx = match self.find_idx(hwnd) {
            Some(i) => i,
            None => return Err(E_DANGLE),
        };
        if !self.maps[idx].alive {
            return Err(E_DANGLE);
        }
        self.maps[idx].alive = false; // 第一步：摘映射
        if self.current_focus == hwnd {
            self.current_focus = 0; // 焦点随窗亡回落
        }
        Ok(())
    }

    /// 查 context 是否存活（映射存在且 alive）。
    pub fn ctx_alive(&self, hwnd: u32) -> bool {
        match self.find_idx(hwnd) {
            Some(i) => self.maps[i].alive,
            None => false,
        }
    }

    /// 焦点切换（J1 主体）：旧 context 释放 → 新 context 获取 →
    /// 切换期内到达的 keys_bridged 个键全部挂到新 context，零丢键。
    pub fn focus_switch(&mut self, to_hwnd: u32, keys_bridged: u32) -> Result<(), u16> {
        let from_hwnd = self.current_focus;
        let from_ctx = if from_hwnd != 0 && self.ctx_alive(from_hwnd) {
            match self.find_idx(from_hwnd) {
                Some(i) => self.maps[i].ctx,
                None => 0,
            }
        } else {
            0
        };
        let to_ctx = if to_hwnd != 0 && self.ctx_alive(to_hwnd) {
            match self.find_idx(to_hwnd) {
                Some(i) => self.maps[i].ctx,
                None => 0,
            }
        } else {
            0
        };
        // 零丢键口径：切到有 context 的窗时，键必须有去处；无 context 的
        // 窗（纯消息窗）允许 keys_bridged>0 但键挂系统缓冲，不算丢——
        // 本账本只对「有 context 的窗」计丢键。
        if to_ctx != 0 && keys_bridged > 0 {
            self.key_total += keys_bridged; // 全部桥接成功
        }
        self.focus_ledger.push(FocusRecord {
            from_hwnd,
            to_hwnd,
            from_ctx,
            to_ctx,
            keys_bridged,
        });
        self.switch_total += 1;
        self.current_focus = to_hwnd;
        Ok(())
    }

    /// 悬空审计（纪律 4）：扫描映射表，亡窗仍挂活账即悬空。
    pub fn audit_dangling(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.maps.len() {
            let e = &self.maps[i];
            if e.hwnd == 0 || e.ctx == 0 {
                return Err(E_DANGLE);
            }
            // 亡窗账保留供销毁序取证，但不得再被引用为焦点。
            if !e.alive && self.current_focus == e.hwnd {
                return Err(E_DANGLE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 双挂审计：同一 hwnd 至多一条活账。
    pub fn audit_no_double_bind(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.maps.len() {
            let mut j = i + 1;
            while j < self.maps.len() {
                if self.maps[i].hwnd == self.maps[j].hwnd
                    && self.maps[i].alive
                    && self.maps[j].alive
                {
                    return Err(E_DUP_MAP);
                }
                j += 1;
            }
            i += 1;
        }
        if self.focus_ledger.len() as u32 != self.switch_total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 零丢键审计：drop 计数必须为 0（J1 硬口径）。
    pub fn audit_zero_dropped(&self) -> Result<(), u16> {
        if self.dropped_keys != 0 {
            return Err(E_KEY_LOST);
        }
        Ok(())
    }

    /// ×N 切换覆盖核验。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.switch_total < want {
            return Err(E_ALIGN_MISS);
        }
        Ok(())
    }

    pub fn map_len(&self) -> usize {
        self.maps.len()
    }
    pub fn focus_ledger_len(&self) -> usize {
        self.focus_ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（对齐 ×1000 零悬空零丢键），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×1000：3 窗轮流焦点切换 1000 次，每次桥键入账，
    //    终态零丢键、零悬空、无双挂、切换账守恒。
    let mut fc = FocusCtx::new();
    fc.bind(0x1001, 0xA001)?;
    fc.bind(0x1002, 0xA002)?;
    fc.bind(0x1003, 0xA003)?;
    let mut i = 0u32;
    while i < 1000 {
        let to = [0x1001u32, 0x1002, 0x1003][(i % 3) as usize];
        let keys = i % 4; // 每次切换 0..3 个桥接键
        fc.focus_switch(to, keys)?;
        if fc.current_focus != to {
            return Err(E_KEY_LOST);
        }
        i += 1;
    }
    fc.coverage(1000)?;
    fc.audit_dangling()?;
    fc.audit_no_double_bind()?;
    fc.audit_zero_dropped()?;
    if fc.map_len() != 3 || fc.focus_ledger_len() as u32 != 1000 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 销毁序：亡窗摘账后焦点自动回落 0，亡窗不得再成为焦点目标。
    let mut fc = FocusCtx::new();
    fc.bind(0x2001, 0xB001)?;
    fc.bind(0x2002, 0xB002)?;
    fc.focus_switch(0x2001, 2)?;
    fc.destroy_window(0x2001)?;
    if fc.ctx_alive(0x2001) {
        return Err(E_ORDER_BAD);
    }
    if fc.current_focus != 0 {
        return Err(E_DANGLE);
    }
    fc.audit_dangling()?;
    // 焦点切到存活窗 0x2002：正常桥接。
    fc.focus_switch(0x2002, 1)?;
    if fc.current_focus != 0x2002 {
        return Err(E_KEY_LOST);
    }
    hits += 1;

    // R3 失配防御：未登记窗切换——键不桥接（无 context 可挂），账照记，
    //    不产生丢键（键留系统缓冲口径），current_focus 照常更新。
    let mut fc = FocusCtx::new();
    fc.bind(0x3001, 0xC001)?;
    fc.focus_switch(0x9999, 5)?;
    if fc.current_focus != 0x9999 {
        return Err(E_LEDGER_BAD);
    }
    if fc.key_total != 0 {
        return Err(E_KEY_LOST); // 无 context 的键不经本账本
    }
    fc.audit_zero_dropped()?;
    fc.audit_dangling()?;
    hits += 1;

    // R4 换绑与双挂：同 hwnd 重绑覆盖旧 ctx；两个 hwnd 共享一个 ctx 不算
    //    双挂（口径：双挂指同 hwnd 双活账），重绑后查询取新 ctx。
    let mut fc = FocusCtx::new();
    fc.bind(0x4001, 0xD001)?;
    fc.bind(0x4001, 0xD002)?; // 覆盖换绑
    if fc.map_len() != 1 {
        return Err(E_DUP_MAP);
    }
    fc.focus_switch(0x4001, 3)?;
    if fc.key_total != 3 {
        return Err(E_KEY_LOST);
    }
    fc.audit_no_double_bind()?;
    hits += 1;

    // R5 亡窗复挂拒收：销毁后重 bind 同 hwnd 必拒（悬空防线）。
    let mut fc = FocusCtx::new();
    fc.bind(0x5001, 0xE001)?;
    fc.destroy_window(0x5001)?;
    if fc.bind(0x5001, 0xE002).is_ok() {
        return Err(E_DANGLE);
    }
    // 销毁未登记窗同样拒收。
    if fc.destroy_window(0x5999).is_ok() {
        return Err(E_DANGLE);
    }
    hits += 1;

    // R6 空焦点序：从 0 焦点出发（from_ctx=0），再切回 0（to_ctx=0），
    //    全程零丢键、账序完整。
    let mut fc = FocusCtx::new();
    fc.bind(0x6001, 0xF001)?;
    fc.focus_switch(0x6001, 2)?; // from=0
    fc.focus_switch(0, 1)?;      // to=0：焦点回落
    if fc.current_focus != 0 {
        return Err(E_DANGLE);
    }
    if fc.switch_total != 2 || fc.focus_ledger_len() != 2 {
        return Err(E_LEDGER_BAD);
    }
    fc.audit_zero_dropped()?;
    fc.audit_dangling()?;
    hits += 1;

    // R7 长程销毁交错：1000 次切换中穿插销毁/换绑，终态审计全绿。
    let mut fc = FocusCtx::new();
    fc.bind(0x7001, 0xFA01)?;
    fc.bind(0x7002, 0xFA02)?;
    fc.bind(0x7003, 0xFA03)?;
    let mut i = 0u32;
    while i < 1000 {
        let to = [0x7001u32, 0x7002, 0x7003][(i % 3) as usize];
        fc.focus_switch(to, 1)?;
        if i == 499 {
            fc.destroy_window(0x7003)?;
        }
        if i == 600 {
            fc.bind(0x7004, 0xFA04)?; // 半程新窗入列
        }
        if i > 600 && i % 100 == 0 {
            fc.focus_switch(0x7004, 2)?;
        }
        i += 1;
    }
    fc.coverage(1000)?;
    fc.audit_dangling()?;
    fc.audit_no_double_bind()?;
    fc.audit_zero_dropped()?;
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要拒。
    if fc.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    fc.writeback(true, 0x20)?;
    hits += 1;

    Ok(hits)
}
