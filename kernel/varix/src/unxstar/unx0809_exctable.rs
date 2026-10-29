//! unx0809_exctable —— UNX-F0809 · 异常号→处理表静态校验与越界防护（目标 220 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0809 · 异常号→处理表静态校验与越界防护。
//! 判据原文：UNX-F0809-J1 处理表覆盖率 100%（32 异常全注册），越界/空表项访问
//!           拦截率 100%。
//! 设计纪律：
//! - IA-32 定义 32 个预定义异常（0~31），处理表必须全量注册——覆盖率 100% 红。
//! - 静态校验：注册时刻即校验（异常号界内、handler 非空、属性一致），不合法即拒。
//! - 越界防护：查询/分发路径对异常号先比界；空表项（P 位清）访问一律拦截并红账。
//! - 覆盖账：32 位位图记录已注册异常，注册/注销实时维护，覆盖率可随时复算。
//! - 拦截账：越界/空项访问事件入定容账（含异常号与类别），审计可回放。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6280（0x6281~0x6287 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const ET_VERSION: &str = "unx0809_exctable-v1";

/// 诊断段基址（0x6280 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6280;

/// XcCode 闭集。
pub const XC_NUM_RANGE: u16 = SEG_BASE + 1; // 异常号越界（>=32）
pub const XC_DUP_REG: u16 = SEG_BASE + 2; // 重复注册
pub const XC_NOT_REG: u16 = SEG_BASE + 3; // 未注册即查询/注销
pub const XC_EMPTY_SLOT: u16 = SEG_BASE + 4; // 空表项访问（P 位清）
pub const XC_COVER_LOW: u16 = SEG_BASE + 5; // 覆盖率 <100% 仍放行
pub const XC_LED_FULL: u16 = SEG_BASE + 6; // 拦截账溢出
pub const XC_BAD_KIND: u16 = SEG_BASE + 7; // handler 类别非法

/// 预定义异常总数（IA-32：0~31）。
pub const EXCEPTION_TOTAL: usize = 32;

/// handler 类别闭集（三态：快路径/慢路径/终结路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandlerKind {
    /// 快路径：无栈切换、无出错码消费。
    Fast = 0,
    /// 慢路径：带出错码、可能切 IST。
    Slow = 1,
    /// 终结路径：不可恢复，走停机/复位。
    Fatal = 2,
}

impl HandlerKind {
    /// 类别合法性（枚举闭集恒真，防扩展期非法值）。
    pub fn valid(self) -> bool {
        matches!(self, HandlerKind::Fast | HandlerKind::Slow | HandlerKind::Fatal)
    }

    /// 类别编号（账本落账用）。
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// 处理表项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExcEntry {
    /// 异常号（0~31）。
    pub num: u8,
    /// handler 锚（建模地址）。
    pub handler_addr: u32,
    /// 类别。
    pub kind: HandlerKind,
    /// 存在位（P）：注销时清零。
    pub present: bool,
}

impl ExcEntry {
    /// 注册时静态校验：号界内 + 地址非零 + 类别合法。
    pub fn validate(&self) -> Result<(), u16> {
        if (self.num as usize) >= EXCEPTION_TOTAL {
            return Err(XC_NUM_RANGE);
        }
        if self.handler_addr == 0 {
            return Err(XC_EMPTY_SLOT);
        }
        if !self.kind.valid() {
            return Err(XC_BAD_KIND);
        }
        Ok(())
    }
}

/// 处理表：32 槽定容 + 覆盖位图 + 拦截账。
pub struct ExcTable {
    slots: [Option<ExcEntry>; EXCEPTION_TOTAL],
    /// 覆盖位图：bit i = 异常 i 已注册。
    cover: u32,
    /// 拦截账（定容 32）。
    intercept: Vec<InterceptRow>,
    intercept_overflow: usize,
}

/// 拦截账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InterceptRow {
    /// 被拦异常号（越界访问时为原始值截断 u8）。
    pub num_raw: u8,
    /// 类别：0=越界，1=空项，2=重复注册，3=未注册注销。
    pub kind: u8,
    /// 事件序号（全局单调）。
    pub seq: u64,
}

impl ExcTable {
    /// 空表。
    pub fn new() -> ExcTable {
        ExcTable {
            slots: [None; EXCEPTION_TOTAL],
            cover: 0,
            intercept: Vec::new(),
            intercept_overflow: 0,
        }
    }

    /// 拦截入账；定容 64，满则计溢出。
    fn record_intercept(&mut self, num_raw: u8, kind: u8, seq: &mut u64) {
        *seq += 1;
        if self.intercept.len() < 64 {
            self.intercept.push(InterceptRow {
                num_raw,
                kind,
                seq: *seq,
            });
        } else {
            self.intercept_overflow += 1;
        }
    }

    /// 注册一异常：静态校验 + 覆盖位图置位。重复注册拦截。
    pub fn register(&mut self, e: ExcEntry, seq: &mut u64) -> Result<(), u16> {
        e.validate()?;
        let idx = e.num as usize;
        if self.slots[idx].is_some() {
            self.record_intercept(e.num, 2, seq);
            return Err(XC_DUP_REG);
        }
        self.slots[idx] = Some(e);
        self.cover |= 1u32 << idx;
        Ok(())
    }

    /// 注销：P 位清 + 位图清位；未注册注销拦截。
    pub fn unregister(&mut self, num: u8, seq: &mut u64) -> Result<(), u16> {
        if (num as usize) >= EXCEPTION_TOTAL {
            self.record_intercept(num, 0, seq);
            return Err(XC_NUM_RANGE);
        }
        if self.slots[num as usize].is_none() {
            self.record_intercept(num, 3, seq);
            return Err(XC_NOT_REG);
        }
        self.slots[num as usize] = None;
        self.cover &= !(1u32 << (num as usize));
        Ok(())
    }

    /// 查询（只读，零副作用）：越界/空项即拦。
    pub fn lookup(&self, num: u8, seq: &mut u64) -> Result<ExcEntry, u16> {
        if (num as usize) >= EXCEPTION_TOTAL {
            // 只读查询无法入账（&self），越界直接拒——账由分发路径记。
            return Err(XC_NUM_RANGE);
        }
        match self.slots[num as usize] {
            Some(e) if e.present => Ok(e),
            _ => Err(XC_EMPTY_SLOT),
        }
    }

    /// 分发路径（可变）：越界/空项访问全量拦截入账，返回 Err。
    pub fn dispatch(&mut self, num: u8, seq: &mut u64) -> Result<ExcEntry, u16> {
        if (num as usize) >= EXCEPTION_TOTAL {
            self.record_intercept(num, 0, seq);
            return Err(XC_NUM_RANGE);
        }
        match self.slots[num as usize] {
            Some(e) if e.present => Ok(e),
            _ => {
                self.record_intercept(num, 1, seq);
                Err(XC_EMPTY_SLOT)
            }
        }
    }

    /// 覆盖率：已注册数 / 32（返回分子，分母为常量 32）。
    pub fn covered(&self) -> usize {
        self.cover.count_ones() as usize
    }

    /// 覆盖率 100% 断言（判据 J1 前半）。
    pub fn full_cover(&self) -> bool {
        self.covered() == EXCEPTION_TOTAL
    }

    /// 全量注册（32 预定义异常，地址按公式生成，类别按三档分派）。
    pub fn register_all_predefined(&mut self, seq: &mut u64) -> Result<(), u16> {
        for i in 0..EXCEPTION_TOTAL {
            let kind = if matches!(i, 8 | 18) {
                HandlerKind::Fatal
            } else if matches!(i, 14 | 13 | 11 | 12 | 17 | 10 | 16 | 1 | 3) {
                HandlerKind::Slow
            } else {
                HandlerKind::Fast
            };
            let e = ExcEntry {
                num: i as u8,
                handler_addr: 0xFEE0_4000 + (i as u32) * 16,
                kind,
                present: true,
            };
            self.register(e, seq)?;
        }
        Ok(())
    }

    /// 拦截账读取（越界即拒）。
    pub fn intercept_row(&self, i: usize) -> Result<InterceptRow, u16> {
        if i >= self.intercept.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.intercept[i])
    }

    /// 拦截账长度与溢出。
    pub fn intercept_stat(&self) -> (usize, usize) {
        (self.intercept.len(), self.intercept_overflow)
    }

    /// 守恒核：账长 + 溢出 == 拦截事件总数。
    pub fn conserve(&self, total_events: usize) -> bool {
        let (l, ov) = self.intercept_stat();
        l + ov == total_events
    }
}

/// 静态校验器：独立于表的逐项校验（供构建期二次核）。
pub fn static_validate_entries(entries: &[ExcEntry]) -> Result<usize, u16> {
    let mut seen = 0u32;
    for e in entries {
        e.validate()?;
        let bit = 1u32 << (e.num as usize);
        if seen & bit != 0 {
            return Err(XC_DUP_REG);
        }
        seen |= bit;
    }
    Ok(seen.count_ones() as usize)
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    let mut seq: u64 = 0;
    let mut t = ExcTable::new();
    // 1. 全量注册 32 异常 → 覆盖率 100%
    t.register_all_predefined(&mut seq)?;
    if !t.full_cover() {
        return Err(XC_COVER_LOW);
    }
    // 2. 正常查询零副作用
    let e14 = t.lookup(14, &mut seq)?;
    if e14.kind != HandlerKind::Slow {
        return Err(XC_BAD_KIND);
    }
    // 3. 越界分发拦截（num=32 → u8 截断为 0，故用原始号判界：dispatch 收 u8，
    //    32 截断后是 0——越界防护须在调用侧以 usize 判界；此处验证 255 号拦截）
    if t.dispatch(200, &mut seq).is_ok() {
        return Err(XC_NUM_RANGE);
    }
    // 200 是界内但未注册？——32 预定义已全注册，200<256 但 >=32 会先撞界？不，
    // 200 as usize >= 32 → 越界。正确。
    // 4. 空项访问拦截：先注销 5 号再查
    t.unregister(5, &mut seq)?;
    if t.dispatch(5, &mut seq).is_ok() {
        return Err(XC_EMPTY_SLOT);
    }
    // 5. 重复注册拦截
    let dup = ExcEntry {
        num: 3,
        handler_addr: 0x1234,
        kind: HandlerKind::Fast,
        present: true,
    };
    if t.register(dup, &mut seq).is_ok() {
        return Err(XC_DUP_REG);
    }
    // 6. 未注册注销拦截：31 已注册，第一次注销成功，第二次起拦截
    t.unregister(31, &mut seq)?;
    if t.unregister(31, &mut seq).is_ok() {
        return Err(XC_NOT_REG);
    }
    // 7. 账守恒：4 次拦截（200 越界 / 5 空项 / 3 重复 / 31 未注册再注销）
    if !t.conserve(4) {
        return Err(XC_LED_FULL);
    }
    // 8. 静态校验器：合法集过 / 重复集拒
    let ok_set = [
        ExcEntry { num: 0, handler_addr: 0x100, kind: HandlerKind::Fast, present: true },
        ExcEntry { num: 1, handler_addr: 0x110, kind: HandlerKind::Slow, present: true },
    ];
    if static_validate_entries(&ok_set)? != 2 {
        return Err(XC_COVER_LOW);
    }
    let bad_set = [
        ExcEntry { num: 2, handler_addr: 0x120, kind: HandlerKind::Fast, present: true },
        ExcEntry { num: 2, handler_addr: 0x130, kind: HandlerKind::Fast, present: true },
    ];
    if static_validate_entries(&bad_set).is_ok() {
        return Err(XC_DUP_REG);
    }
    Ok(0)
}
