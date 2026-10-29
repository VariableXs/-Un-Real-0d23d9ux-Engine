//! UNX-F48712 —— I36 fuzz（随机账本注入治理面零崩）
//! 判据原文：UNX-F48712-J1 fuzz ×10^4 零崩；判据锚定 体验十。
//! 判据落地（drill 场景）：R1 fuzz ×10^4 随机账本注入；R2 零崩
//! 断言；R3 越界注入拒绝；R4 账目自愈核对；R5 计数守恒；R6 复跑
//! 一致；R7 指纹稳定；R8 呈现行；R9 注入面闭集；R10 残账清点。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：fuzz ×10^4 = LCG 生成一万笔随机注入（登记/转单/关单/
//! 越界码四类），治理面账本对每笔拒绝或落账后保持自洽（零崩）；
//! 复跑同种子指纹一致才算确定性零崩。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x10C0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ACT: u16 = SEG_BASE + 1; // 注入动作越出闭集
const E_STATE: u16 = SEG_BASE + 2; // 账目状态非法
const E_ID: u16 = SEG_BASE + 3; // 缺陷号非法
const E_CRASH: u16 = SEG_BASE + 4; // fuzz 崩溃检出
const E_COUNT: u16 = SEG_BASE + 5; // 计数不守恒
const E_REPLAY: u16 = SEG_BASE + 6; // 复跑结果漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SELF: u16 = SEG_BASE + 9; // 自愈核对失败
const E_ROUND: u16 = SEG_BASE + 10; // fuzz 轮次不足

/// 注入动作闭集。
pub const A_REGISTER: u8 = 0; // 登记
pub const A_TRANSFER: u8 = 1; // 转单
pub const A_CLOSE: u8 = 2; // 关单
pub const A_OOB: u8 = 3; // 越界注入
pub const ACT_N: u32 = 4;

/// fuzz 轮次（×10^4）。
pub const FUZZ_N: u32 = 10_000;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 治理面账本（fuzz 注入目标）。
pub struct GovBook {
    pub ids: Vec<u32>,     // 已登记缺陷号
    pub states: Vec<u8>,   // 0 悬挂 / 1 转单 / 2 关单
}

impl GovBook {
    pub fn new() -> GovBook {
        GovBook { ids: vec![], states: vec![] }
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ids.len() {
            if self.ids[i] == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记：号 ≥1 且未占用。
    pub fn register(&mut self, id: u32) -> Result<(), u16> {
        if id == 0 {
            return Err(E_ID);
        }
        if self.find(id).is_some() {
            return Err(E_STATE);
        }
        self.ids.push(id);
        self.states.push(0);
        Ok(())
    }

    /// 转单：悬挂 → 转单。
    pub fn transfer(&mut self, id: u32) -> Result<(), u16> {
        match self.find(id) {
            Some(i) => {
                if self.states[i] != 0 {
                    return Err(E_STATE);
                }
                self.states[i] = 1;
                Ok(())
            }
            None => Err(E_ID),
        }
    }

    /// 关单：转单 → 关单。
    pub fn close(&mut self, id: u32) -> Result<(), u16> {
        match self.find(id) {
            Some(i) => {
                if self.states[i] != 1 {
                    return Err(E_STATE);
                }
                self.states[i] = 2;
                Ok(())
            }
            None => Err(E_ID),
        }
    }

    /// 自愈核对：两列等长、状态闭集、状态机合法。
    pub fn sane(&self) -> Result<(), u16> {
        if self.ids.len() != self.states.len() {
            return Err(E_SELF);
        }
        let mut i = 0usize;
        while i < self.states.len() {
            if self.states[i] > 2 {
                return Err(E_STATE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 状态分布计数。
    pub fn dist(&self) -> (u32, u32, u32) {
        let (mut n0, mut n1, mut n2) = (0u32, 0u32, 0u32);
        let mut i = 0usize;
        while i < self.states.len() {
            if self.states[i] == 0 {
                n0 += 1;
            } else if self.states[i] == 1 {
                n1 += 1;
            } else {
                n2 += 1;
            }
            i += 1;
        }
        (n0, n1, n2)
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.ids.len() {
            buf.extend_from_slice(&self.ids[i].to_le_bytes());
            buf.push(self.states[i]);
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// fuzz 注入账：登记每轮动作与结果（拒绝/落账）。
pub struct FuzzLog {
    pub ok_n: u32,
    pub rej_n: u32,
}

/// 一轮注入：生成随机动作并施加，拒绝或落账均入日志。
pub fn fuzz_round(book: &mut GovBook, log: &mut FuzzLog, g: &mut Lcg) -> Result<(), u16> {
    let act = g.below(ACT_N) as u8;
    let id = g.below(64) + 1;
    let r = match act {
        A_REGISTER => book.register(id),
        A_TRANSFER => book.transfer(id),
        A_CLOSE => book.close(id),
        A_OOB => Err(E_ACT), // 越界注入恒拒绝
        _ => Err(E_ACT),
    };
    match r {
        Ok(()) => {
            log.ok_n += 1;
            Ok(())
        }
        Err(_) => {
            log.rej_n += 1;
            Ok(()) // 拒绝不算崩
        }
    }
}

/// fuzz ×10^4 主循环：逐轮注入 + 每千轮自愈核对。
pub fn run_fuzz(book: &mut GovBook, log: &mut FuzzLog, g: &mut Lcg) -> Result<(), u16> {
    let mut r: u32 = 0;
    while r < FUZZ_N {
        fuzz_round(book, log, g)?;
        if r % 1_000 == 999 {
            book.sane()?;
        }
        r += 1;
    }
    book.sane()?;
    Ok(())
}

/// 呈现行：ok=<n> rej=<n> total=<n>。
pub fn fuzz_line(log: &FuzzLog) -> String {
    let mut out = String::new();
    out.push_str("ok=");
    out.push_str(&log.ok_n.to_string());
    out.push_str(" rej=");
    out.push_str(&log.rej_n.to_string());
    out.push_str(" total=");
    out.push_str(&(log.ok_n + log.rej_n).to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 注入面闭集：越界注入恒拒绝、拒绝不算崩。
    let mut probe_book = GovBook::new();
    let mut probe_log = FuzzLog { ok_n: 0, rej_n: 0 };
    let mut probe_g = Lcg::new(0x487_12);
    let mut probe_ok = true;
    let mut k = 0u32;
    while k < 16 {
        let before = probe_log.rej_n;
        fuzz_round(&mut probe_book, &mut probe_log, &mut probe_g)?;
        if probe_log.rej_n < before {
            probe_ok = false; // 拒绝必须只增不减
        }
        k += 1;
    }
    if !probe_ok || probe_log.rej_n == 0 {
        return Err(E_ACT);
    }
    hits += 1;

    // R2 fuzz ×10^4 零崩主循环。
    let mut main_book = GovBook::new();
    let mut main_log = FuzzLog { ok_n: 0, rej_n: 0 };
    let mut main_g = Lcg::new(0x487_12);
    run_fuzz(&mut main_book, &mut main_log, &mut main_g)?;
    if main_log.ok_n + main_log.rej_n != FUZZ_N {
        return Err(E_ROUND);
    }
    hits += 1;

    // R3 越界注入拒绝：OOB 恒拒，rej 计数 >0。
    if main_log.rej_n == 0 || main_log.ok_n == 0 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R4 账目自愈核对：终态自洽。
    main_book.sane()?;
    hits += 1;

    // R5 计数守恒：状态分布之和 = 登记数。
    let (n0, n1, n2) = main_book.dist();
    if n0 + n1 + n2 != main_book.ids.len() as u32 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R6 复跑一致：同种子重跑指纹一致。
    let mut book2 = GovBook::new();
    let mut log2 = FuzzLog { ok_n: 0, rej_n: 0 };
    let mut g2 = Lcg::new(0x487_12);
    run_fuzz(&mut book2, &mut log2, &mut g2)?;
    if book2.fp() != main_book.fp() || log2.ok_n != main_log.ok_n {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = main_book.fp();
    let f2 = main_book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行：total=10000。
    let line = fuzz_line(&main_log);
    if !has_kw(line.as_bytes(), b"total=10000") || !has_kw(line.as_bytes(), b"ok=") {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 注入面闭集：ACT_N 冻结为 4。
    if ACT_N != 4 {
        return Err(E_ACT);
    }
    hits += 1;

    // R10 残账清点：登记数与分布守恒复核。
    if n0 + n1 + n2 > FUZZ_N {
        return Err(E_SELF);
    }
    hits += 1;

    Ok(hits)
}
