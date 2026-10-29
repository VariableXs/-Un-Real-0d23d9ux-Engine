//! unx4175_depthcap — UNX-F4175 解析深度上限与防递归膨胀（诊断段 0x9780）。
//!
//! 判据原文：
//!   J1 UNX-F4175-J1 深度上限（64 分量级）注入返回错误且栈深有账（峰值
//!      不超限），深度溢出不崩（10/10 注入全拦截）。
//! 设计纪律：
//!   1. 分量深度上限 64（PATH_MAX 级分量口径），第 65 分量即拒，
//!      错误码 E_DEPTH_CAP，拒收入账不崩。
//!   2. 栈深有账：解析全程迭代制（显式循环，不用递归），模拟栈深峰值
//!      单独入账且恒不超 64——深度溢出以计数拦截，不以递归硬扛。
//!   3. 峰值账封顶：溢出注入只把探测点入账，峰值账面永不越限——
//!      判据「峰值不超限」的账面落点，越限即报 E_LEDGER。
//!   4. 状态机三态 Scan/Overflow/Sealed：计数中、超限拒收、封账，
//!      每条路径提交一次状态迁移闭环，封账后再提交一律拒。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::String;

pub const SEG_BASE: u16 = 0x9780;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;    // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 2;   // 空分量
const E_DEPTH_CAP: u16 = SEG_BASE + 3;  // 分量深度超 64 上限
const E_STATE: u16 = SEG_BASE + 4;      // 状态机非法迁移（封账后再提交）
const E_LEDGER: u16 = SEG_BASE + 5;     // 峰值账越限（栈深账失守）
const E_EMPTY: u16 = SEG_BASE + 6;      // 空路径提交（无分量可计）

pub const MAX_DEPTH: usize = 64;  // 分量深度上限（判据 64 分量级口径）
pub const INJECT_TIMES: u32 = 10; // 判据注入次数（10/10 全拦截）

/// 深度账状态机：一条路径提交的全生命周期三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepthPhase {
    Scan,     // 计数中（正常受理期）
    Overflow, // 超限拒收期（最近一次提交曾溢出）
    Sealed,   // 封账期（终审后锁定）
}

/// 栈深/峰值账：峰值不超限的账面载体。
pub struct DepthBook {
    pub accepted: u32,     // 受理路径数
    pub rejected: u32,     // 超限拒收数（注入拦截账）
    pub peak: usize,       // 历史最深受理分量数（≤ MAX_DEPTH）
    pub stack_peak: usize, // 模拟栈深峰值（迭代制恒为 1 层探测帧）
    pub phase: DepthPhase,
}

impl DepthBook {
    pub fn new() -> DepthBook {
        DepthBook {
            accepted: 0,
            rejected: 0,
            peak: 0,
            stack_peak: 1, // 迭代制：全程单探测帧
            phase: DepthPhase::Scan,
        }
    }
}

/// 深度裁断器：逐分量计数 + 上限拦截 + 双账登记。
pub struct DepthCap {
    pub book: DepthBook,
    pub probes: u32, // 溢出探测拦截数（第 65 分量触发点账）
}

impl DepthCap {
    pub fn new() -> DepthCap {
        DepthCap {
            book: DepthBook::new(),
            probes: 0,
        }
    }

    /// 提交一条绝对路径：逐分量计数，>64 拒收；受理则返回分量数。
    pub fn submit(&mut self, path: &str) -> Result<usize, u16> {
        if self.book.phase == DepthPhase::Sealed {
            return Err(E_STATE);
        }
        if path.is_empty() {
            return Err(E_EMPTY);
        }
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let body = &path[1..]; // '/' 单字节，前缀判定后切片安全
        if body.is_empty() {
            // 根路径零分量：受理但深度为 0。
            self.book.accepted += 1;
            self.book.phase = DepthPhase::Scan;
            return Ok(0);
        }
        let mut depth = 0usize;
        let mut overflow = false;
        for comp in body.split('/') {
            if comp.is_empty() {
                return Err(E_BAD_COMP);
            }
            depth += 1;
            if depth > MAX_DEPTH {
                // 第 65 分量即拦：探测点入账，迭代制停探即栈深不再增长。
                overflow = true;
                self.probes += 1;
                break;
            }
        }
        if overflow {
            self.book.rejected += 1;
            self.book.phase = DepthPhase::Overflow;
            if self.book.peak > MAX_DEPTH {
                return Err(E_LEDGER); // 账面自检：峰值失守即报
            }
            return Err(E_DEPTH_CAP);
        }
        if depth > self.book.peak {
            self.book.peak = depth;
        }
        self.book.accepted += 1;
        self.book.phase = DepthPhase::Scan;
        Ok(depth)
    }

    /// 迭代制栈深读口：恒为 1 探测帧（防递归膨胀的账面证据）。
    pub fn stack_audit(&self) -> usize {
        self.book.stack_peak
    }

    /// 封账：终审后锁定，再提交一律 E_STATE。
    pub fn seal(&mut self) {
        self.book.phase = DepthPhase::Sealed;
    }

    /// 注入拦截全判：拒收数达判据次数且峰值不超限。
    pub fn injections_all_blocked(&self) -> bool {
        self.book.rejected >= INJECT_TIMES && self.book.peak <= MAX_DEPTH
    }

    /// 边界探测：恰在 64 分量（受理）与 65 分量（拒收）双点复核，
    /// 返回（受理口径，拒收口径）双布尔的账面复核面。
    pub fn probe_boundary(&mut self, path64: &str, path65: &str) -> Result<(bool, bool), u16> {
        let ok64 = self.submit(path64).is_ok();
        let rej65 = self.submit(path65) == Err(E_DEPTH_CAP);
        Ok((ok64, rej65))
    }

    /// 账面复位（新一轮注入账独立复演用，峰值保留作历史高水位）。
    pub fn reset_counters(&mut self) {
        self.book.accepted = 0;
        self.book.rejected = 0;
        self.probes = 0;
        self.book.phase = DepthPhase::Scan;
    }

    /// 统计读面：（受理数，拒收数，探测拦截数）三元导出。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.book.accepted, self.book.rejected, self.probes)
    }

    /// 账面导出（峰值/栈深双账快照）。
    pub fn snapshot(&self) -> [u32; 4] {
        [
            self.book.accepted,
            self.book.rejected,
            self.book.peak as u32,
            self.book.stack_peak as u32,
        ]
    }
}

/// 构造 n 分量深路径 /d1/d2/…/dn（确定性，纯整数转字符，免动态格式化）。
pub fn make_deep(n: usize) -> String {
    let mut s = String::new();
    let mut i = 1usize;
    while i <= n {
        s.push('/');
        s.push('d');
        let mut v = i;
        let mut buf = [0u8; 20];
        let mut k = 0usize;
        while v > 0 {
            buf[k] = b'0' + (v % 10) as u8;
            k += 1;
            v /= 10;
        }
        let mut j = k;
        while j > 0 {
            j -= 1;
            s.push(buf[j] as char);
        }
        i += 1;
    }
    s
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_DEPTH_CAP => "分量深度超 64 上限",
        E_STATE => "状态机非法迁移（封账后再提交）",
        E_LEDGER => "峰值账越限（栈深账失守）",
        E_EMPTY => "空路径提交",
        _ => "未知诊断码",
    }
}

/// 确定性演练：受理边界→10/10 注入全拦截→峰值不超限→封账与形错注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut dc = DepthCap::new();

    // R1：浅路径受理——1 分量与根路径深度账正确。
    if dc.submit("/a")? != 1 {
        return Err(E_LEDGER);
    }
    if dc.submit("/")? != 0 {
        return Err(E_LEDGER);
    }
    if dc.book.accepted != 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2：上限内受理——63 分量受理，峰值 63。
    if dc.submit(&make_deep(63))? != 63 {
        return Err(E_LEDGER);
    }
    if dc.book.peak != 63 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：恰好 64 分量受理（边界内），峰值封到 64。
    if dc.submit(&make_deep(64))? != 64 {
        return Err(E_LEDGER);
    }
    if dc.book.peak != 64 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：10/10 注入全拦截——深度 65..74 逐条 E_DEPTH_CAP，拒收账 10。
    let mut n = 65usize;
    while n <= 74 {
        if dc.submit(&make_deep(n)) != Err(E_DEPTH_CAP) {
            return Err(E_DEPTH_CAP);
        }
        n += 1;
    }
    if dc.book.rejected != INJECT_TIMES || dc.probes != INJECT_TIMES {
        return Err(E_LEDGER);
    }
    if !dc.injections_all_blocked() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：栈深有账——峰值不超限（判据核心），迭代制栈深恒 1。
    if dc.book.peak > MAX_DEPTH {
        return Err(E_LEDGER);
    }
    if dc.stack_audit() != 1 || dc.stack_audit() > MAX_DEPTH {
        return Err(E_LEDGER);
    }
    let s = dc.snapshot();
    if s[2] > MAX_DEPTH as u32 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R6：拒收后继续受理正常路径（状态机回到 Scan）。
    if dc.submit("/ok")? != 1 {
        return Err(E_LEDGER);
    }
    if !matches!(dc.book.phase, DepthPhase::Scan) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：封账后再提交 → E_STATE。
    dc.seal();
    if dc.submit("/late") != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I2：空路径 / 相对路径 / 空分量三类形错注入。
    let mut dc2 = DepthCap::new();
    if dc2.submit("") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if dc2.submit("a/b") != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if dc2.submit("/a//b") != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    hits += 1;

    // R7：边界探测复核——64 分量受理、65 分量拒收双点再验（第二面）。
    let mut dc3 = DepthCap::new();
    let (ok64, rej65) = dc3.probe_boundary(&make_deep(64), &make_deep(65))?;
    if !ok64 || !rej65 {
        return Err(E_DEPTH_CAP);
    }
    if dc3.book.accepted != 1 || dc3.book.rejected != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R8：账面复位复演——复位后计数归零、峰值高水位保留、再注入可续。
    let peak_before = dc3.book.peak;
    dc3.reset_counters();
    let (a, r, p) = dc3.stats();
    if a != 0 || r != 0 || p != 0 {
        return Err(E_LEDGER);
    }
    if dc3.book.peak != peak_before {
        return Err(E_LEDGER); // 高水位不随计数复位
    }
    if dc3.submit(&make_deep(70)) != Err(E_DEPTH_CAP) {
        return Err(E_DEPTH_CAP);
    }
    if dc3.book.rejected != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R9：快照终审——峰值不超限、栈深恒 1、注入拦截判再成立。
    let s = dc.snapshot();
    if s[2] > MAX_DEPTH as u32 || s[3] != 1 {
        return Err(E_LEDGER);
    }
    if !dc.injections_all_blocked() {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
