//! UNX-F0850 · 中断在途审计（ISR 位残留扫描） · 诊断段 0x6720（独占 0x6720~0x672F）
//!
//! **判据原文摘录**：残留扫描覆盖 256 向量（周期 ≤1s），残留定位到 handler
//! 并红账 10/10 次。
//!
//! **设计纪律**：
//! 1. 全量扫描：256 向量 ISR/IRR 位图逐位扫描（[u64;4] 逐字 64 位展开），
//!    每轮扫描窗（模型 ≤1s，以扫描周期账 1 tick = 1 轮计）必须覆盖全部
//!    256 位——覆盖数恒等式 `bits_scanned = 256 × rounds`。
//! 2. 残留定位：残留位 → handler 注册表反查（向量 → handler 名），未注册
//!    向量残留落「幽灵 handler」红账。定位成功率判据 10/10。
//! 3. 红账：每处残留立一条红账（向量、handler、残留龄期 tick），账目
//!    schema 统一；红账只增不删（审计面不可篡改），清除需显式 reconcile。
//! 4. 注入面：10/10 次人为制造 ISR 残留，扫描全部抓获；对照组正常 EOI
//!    零红账（无误报）。
//! 5. 零 unwrap/expect/panic/浮点/std；无墙钟，扫描轮次即周期账。
//!
//! **诊断段声明**：0x6720~0x672F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6720~0x672F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6720;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x672F;

/// 向量号越界。
pub const X_RS_VEC_RANGE: u16 = 0x6721;
/// handler 名非法（空名）。
pub const X_RS_HANDLER_EMPTY: u16 = 0x6722;
/// 向量重复注册 handler。
pub const X_RS_DUP_HANDLER: u16 = 0x6723;
/// 扫描覆盖不全（bits_scanned ≠ 256 × rounds）。
pub const X_RS_COVERAGE: u16 = 0x6724;
/// 残留未定位（ISR 位残留但 handler 与幽灵账皆无——定位器失灵）。
pub const X_RS_LOCATE_MISS: u16 = 0x6725;
/// 红账满（定容拒绝）。
pub const X_RS_RED_FULL: u16 = 0x6726;
/// 红账调和越权（未先清除残留即销账）。
pub const X_RS_RECONCILE: u16 = 0x6727;
/// 注入残留漏抓（应红账未红账）。
pub const X_RS_INJECT_MISS: u16 = 0x6728;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_RS_VEC_RANGE => "向量号越界",
        X_RS_HANDLER_EMPTY => "handler名非法",
        X_RS_DUP_HANDLER => "向量重复注册",
        X_RS_COVERAGE => "扫描覆盖不全",
        X_RS_LOCATE_MISS => "残留未定位",
        X_RS_RED_FULL => "红账满",
        X_RS_RECONCILE => "红账调和越权",
        X_RS_INJECT_MISS => "注入残留漏抓",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、位图与 handler 注册表
// ===========================================================================

/// 256 位 ISR 位图。
#[derive(Clone, Copy)]
pub struct VecBitmap {
    words: [u64; 4],
}

impl VecBitmap {
    pub const EMPTY: VecBitmap = VecBitmap { words: [0; 4] };

    pub fn set(&mut self, v: u8) {
        self.words[(v >> 6) as usize] |= 1u64 << (v & 63);
    }
    pub fn clear(&mut self, v: u8) {
        self.words[(v >> 6) as usize] &= !(1u64 << (v & 63));
    }
    pub fn get(&self, v: u8) -> bool {
        (self.words[(v >> 6) as usize] >> (v & 63)) & 1 == 1
    }
    pub fn popcount(&self) -> u32 {
        let mut n = 0u32;
        let mut w = 0;
        while w < 4 {
            let mut x = self.words[w];
            while x != 0 {
                x &= x - 1;
                n += 1;
            }
            w += 1;
        }
        n
    }
}

/// handler 注册项。
#[derive(Clone)]
pub struct HandlerEntry {
    pub vector: u8,
    pub name: String,
    /// 注册以来服务次数。
    pub served: u64,
}

/// handler 注册表（向量 → handler 名）。
pub struct HandlerTable {
    entries: Vec<HandlerEntry>,
}

impl HandlerTable {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn register(&mut self, vector: u8, name: &str) -> Result<(), u16> {
        if vector < 32 || vector > 94 {
            return Err(X_RS_VEC_RANGE);
        }
        if name.is_empty() {
            return Err(X_RS_HANDLER_EMPTY);
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].vector == vector {
                return Err(X_RS_DUP_HANDLER);
            }
            i += 1;
        }
        self.entries.push(HandlerEntry {
            vector,
            name: name.to_string(),
            served: 0,
        });
        Ok(())
    }

    /// 反查 handler 名（未注册 → None）。
    pub fn lookup(&self, vector: u8) -> Option<&str> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].vector == vector {
                return Some(self.entries[i].name.as_str());
            }
            i += 1;
        }
        None
    }

    /// bump 服务计数。
    pub fn bump(&mut self, vector: u8) {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].vector == vector {
                self.entries[i].served += 1;
                return;
            }
            i += 1;
        }
    }
}

// ===========================================================================
// 三、红账与扫描审计器
// ===========================================================================

/// 红账条目（schema 统一）。
#[derive(Clone)]
pub struct RedEntry {
    /// 残留向量。
    pub vector: u8,
    /// 定位到的 handler 名（未注册 = "幽灵handler"）。
    pub handler: String,
    /// 残留龄期（扫描轮 tick）。
    pub age_ticks: u64,
    /// 立案扫描轮次。
    pub raised_at: u64,
}

/// 在途审计器：全量扫描 + 定位 + 红账。
pub struct InflightAuditor {
    pub isr: VecBitmap,
    pub handlers: HandlerTable,
    reds: Vec<RedEntry>,
    red_cap: usize,
    /// 累计扫描位数（守恒审计）。
    pub bits_scanned: u64,
    /// 扫描轮次。
    pub rounds: u64,
    /// 累计残留红账数。
    pub residuals_found: u64,
}

impl InflightAuditor {
    pub fn new(red_cap: usize) -> Self {
        Self {
            isr: VecBitmap::EMPTY,
            handlers: HandlerTable::new(),
            reds: Vec::with_capacity(red_cap),
            red_cap,
            bits_scanned: 0,
            rounds: 0,
            residuals_found: 0,
        }
    }

    /// 一轮全量扫描：256 位逐位扫，残留即定位 + 红账。
    pub fn scan_round(&mut self) -> Result<u32, u16> {
        self.rounds += 1;
        let mut found = 0u32;
        let mut bit = 0u32;
        while bit < 256 {
            self.bits_scanned += 1;
            let v = bit as u8;
            if self.isr.get(v) {
                // 残留：定位 handler。
                let name = match self.handlers.lookup(v) {
                    Some(n) => n.to_string(),
                    None => "幽灵handler".to_string(),
                };
                if self.reds.len() < self.red_cap {
                    self.reds.push(RedEntry {
                        vector: v,
                        handler: name,
                        age_ticks: self.rounds,
                        raised_at: self.rounds,
                    });
                } else {
                    return Err(X_RS_RED_FULL);
                }
                self.residuals_found += 1;
                found += 1;
            }
            bit += 1;
        }
        Ok(found)
    }

    /// 覆盖守恒：bits_scanned == 256 × rounds。
    pub const fn coverage_ok(&self) -> bool {
        self.bits_scanned == self.rounds * 256
    }

    /// 红账条数。
    pub fn red_len(&self) -> usize {
        self.reds.len()
    }

    /// 按向量查红账（定位验收）。
    pub fn red_for(&self, vector: u8) -> Option<&RedEntry> {
        let mut i = 0;
        while i < self.reds.len() {
            if self.reds[i].vector == vector {
                return Some(&self.reds[i]);
            }
            i += 1;
        }
        None
    }

    /// 调和：残留清除后销账（先清位再销，越权拒绝）。
    pub fn reconcile(&mut self, vector: u8) -> Result<(), u16> {
        if self.isr.get(vector) {
            return Err(X_RS_RECONCILE);
        }
        let mut i = 0;
        while i < self.reds.len() {
            if self.reds[i].vector == vector {
                self.reds.remove(i);
                return Ok(());
            }
            i += 1;
        }
        Ok(())
    }
}

// ===========================================================================
// 四、注入演练（10 残留必红账 + 对照零误报）
// ===========================================================================

/// 演练结论。
pub struct ResidualVerdict {
    pub injected: u64,
    pub raised: u64,
    pub located: u64,
    pub false_alarms: u64,
    pub coverage_ok: bool,
}

/// 注入演练：10 向量制造残留（其中 8 个已注册 handler、2 个未注册走
/// 幽灵账），扫描 1 轮必须 10/10 红账；再跑 1 轮对照（无残留）零新增。
pub fn run_residual_rounds() -> Result<ResidualVerdict, u16> {
    let mut aud = InflightAuditor::new(32);
    // 注册 8 个 handler（向量 40..=47）。
    let mut v = 40;
    while v <= 47 {
        aud.handlers.register(v, "hnd_x")?;
        v += 1;
    }
    // 注入残留：40..=49（48、49 未注册 → 幽灵）。
    let mut v = 40;
    while v <= 49 {
        aud.isr.set(v);
        v += 1;
    }
    // 扫描一轮：必须 10 红。
    let found = aud.scan_round()?;
    if found != 10 {
        return Err(X_RS_INJECT_MISS);
    }
    // 定位验收：10/10 有红账且 handler 字段非空。
    let mut located = 0u64;
    let mut v = 40;
    while v <= 49 {
        match aud.red_for(v) {
            Some(e) if !e.handler.is_empty() => located += 1,
            _ => {}
        }
        v += 1;
    }
    if located != 10 {
        return Err(X_RS_LOCATE_MISS);
    }
    // 对照轮：清残留后扫描零新增。
    let mut v = 40;
    while v <= 49 {
        aud.isr.clear(v);
        v += 1;
    }
    let found2 = aud.scan_round()?;
    if found2 != 0 {
        return Err(X_RS_INJECT_MISS);
    }
    Ok(ResidualVerdict {
        injected: 10,
        raised: 10,
        located,
        false_alarms: found2 as u64,
        coverage_ok: aud.coverage_ok(),
    })
}

// ===========================================================================
// 五、drill 自例程
// ===========================================================================

/// 自例程：10/10 红账定位 + 覆盖守恒 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) handler 名空拒绝 / 向量越界拒绝 / 重复注册拒绝。
    let mut aud = InflightAuditor::new(8);
    if aud.handlers.register(40, "").is_ok() {
        return Ok(1);
    }
    if aud.handlers.register(10, "h").is_ok() {
        return Ok(2);
    }
    aud.handlers.register(40, "hnd_a")?;
    if aud.handlers.register(40, "hnd_b").is_ok() {
        return Ok(3);
    }
    // 2) 主路径：注入演练。
    let v = run_residual_rounds()?;
    if !v.coverage_ok {
        return Err(X_RS_COVERAGE);
    }
    if v.raised != 10 || v.located != 10 || v.false_alarms != 0 {
        return Ok(4);
    }
    // 3) 幽灵 handler 定位：48/49 未注册但红账存在且名为幽灵。
    let mut aud2 = InflightAuditor::new(16);
    aud2.handlers.register(40, "hnd_t")?;
    aud2.isr.set(40);
    aud2.isr.set(60); // 60 未注册。
    let f = aud2.scan_round()?;
    if f != 2 {
        return Ok(5);
    }
    let g = aud2.red_for(60);
    match g {
        Some(e) if e.handler == "幽灵handler" => {}
        _ => return Err(X_RS_LOCATE_MISS),
    }
    // 4) 覆盖守恒：两轮 = 512 位。
    if aud2.bits_scanned != 512 || !aud2.coverage_ok() {
        return Err(X_RS_COVERAGE);
    }
    // 5) 调和越权：残留未清销账拒绝；清后销账成功。
    if aud2.reconcile(40).is_ok() {
        return Ok(6);
    }
    aud2.isr.clear(40);
    aud2.reconcile(40)?;
    if aud2.red_len() != 1 {
        return Ok(7);
    }
    // 6) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_RS_VEC_RANGE, X_RS_HANDLER_EMPTY, X_RS_DUP_HANDLER, X_RS_COVERAGE,
        X_RS_LOCATE_MISS, X_RS_RED_FULL, X_RS_RECONCILE, X_RS_INJECT_MISS,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(8);
        }
    }
    Ok(0)
}
